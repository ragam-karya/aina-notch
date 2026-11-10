// Packs the release build into an MSIX for the Microsoft Store: target/msix/aina_<version>_x64.msix.
// The package holds what the NSIS installer installs (aina.exe and tauri.conf.json's bundle.resources),
// the Store logos and AppxManifest.xml filled from crates/aina-app/msix. Run after `tauri build
// --no-bundle` (npm run build:msix does both). The Store signs what it publishes, so the upload needs no
// signature; for installing it here first, AINA_MSIX_CERT (a .pfx whose subject is the manifest's
// Publisher) and AINA_MSIX_CERT_PASSWORD sign it.
import { execFileSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const app = join(root, 'crates', 'aina-app');
const conf = JSON.parse(readFileSync(join(app, 'tauri.conf.json'), 'utf8'));
const identity = JSON.parse(readFileSync(join(app, 'msix', 'identity.json'), 'utf8'));
const exe = join(root, 'target', 'release', `${conf.mainBinaryName}.exe`);
const out = join(root, 'target', 'msix');
const stage = join(out, 'stage');

const fail = (msg) => {
  console.error(`build-msix: ${msg}`);
  process.exit(1);
};

// The Store wants four numbers with the last one 0.
const version = `${conf.version.split(/[-+]/)[0]}.0`;
if (!/^\d+\.\d+\.\d+\.0$/.test(version)) fail(`version "${conf.version}" can't be an MSIX version`);
if (!existsSync(exe)) fail(`${exe} is missing: run "tauri build --no-bundle" first (npm run build:msix)`);

const msixEnv = (k) => process.env[`AINA_MSIX_${k}`];
const xml = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const fields = {
  name: msixEnv('NAME') || identity.name,
  publisher: msixEnv('PUBLISHER') || identity.publisher,
  publisherDisplayName: msixEnv('PUBLISHER_NAME') || identity.publisherDisplayName,
  version,
  description: conf.bundle.shortDescription,
};

rmSync(stage, { recursive: true, force: true });
mkdirSync(join(stage, 'Assets'), { recursive: true });
cpSync(exe, join(stage, `${conf.mainBinaryName}.exe`));
for (const [from, to] of Object.entries(conf.bundle.resources || {})) {
  const src = join(app, from);
  if (!existsSync(src)) fail(`resource ${from} is missing`);
  cpSync(src, join(stage, to), { recursive: true });
}
for (const logo of ['Square44x44Logo', 'Square150x150Logo', 'StoreLogo']) {
  cpSync(join(app, 'icons', `${logo}.png`), join(stage, 'Assets', `${logo}.png`));
}
const manifest = readFileSync(join(app, 'msix', 'AppxManifest.xml'), 'utf8').replace(/\{\{(\w+)\}\}/g, (_, k) => {
  if (!(k in fields)) fail(`unknown manifest field ${k}`);
  return xml(fields[k]);
});
writeFileSync(join(stage, 'AppxManifest.xml'), manifest);

// makeappx and signtool from the newest Windows SDK.
function sdkTool(name) {
  const bin = 'C:\\Program Files (x86)\\Windows Kits\\10\\bin';
  const versions = existsSync(bin) ? readdirSync(bin).filter((v) => /^10\./.test(v)).sort().reverse() : [];
  const found = versions.map((v) => join(bin, v, 'x64', name)).find(existsSync);
  if (!found) fail(`${name} not found: install the Windows SDK`);
  return found;
}

const msix = join(out, `${conf.productName}_${version}_x64.msix`);
execFileSync(sdkTool('makeappx.exe'), ['pack', '/o', '/d', stage, '/p', msix], { stdio: 'inherit' });

if (msixEnv('CERT')) {
  const args = ['sign', '/fd', 'SHA256', '/f', msixEnv('CERT')];
  if (msixEnv('CERT_PASSWORD')) args.push('/p', msixEnv('CERT_PASSWORD'));
  execFileSync(sdkTool('signtool.exe'), [...args, msix], { stdio: 'inherit' });
}
console.log(`build-msix: ${msix} (${fields.name} ${version}, ${msixEnv('CERT') ? 'signed for sideloading' : 'unsigned, for the Store'})`);
