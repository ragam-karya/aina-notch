// Copies Badi's UI into ui-dist/ for Tauri, unchanged. The v1 pages load their runtimes and assets by
// relative paths (../../node_modules/…, ../../assets/…), so ui-dist mirrors the v1 layout instead of
// rewriting them: src/ui/*, assets/*, and only the runtime files the pages use from node_modules.
// The UI stays one source of truth (../src/ui) until the cutover; v2 only swaps the window.badi bridge.
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const v1 = join(here, '..', '..');
const out = join(here, '..', 'ui-dist');

const copies = [
  ['src/ui', 'src/ui'],
  ['assets/badi.riv', 'assets/badi.riv'],
  ['assets/cloud.json', 'assets/cloud.json'],
  ['assets/file-upload.json', 'assets/file-upload.json'],
  ['assets/fonts', 'assets/fonts'], // Urbanist (src/ui/fonts.css)
  ['assets/brand', 'assets/brand'], // aina's logo (Settings)
  ['node_modules/@rive-app/canvas/rive.js', 'node_modules/@rive-app/canvas/rive.js'],
  ['node_modules/@rive-app/canvas/rive.wasm', 'node_modules/@rive-app/canvas/rive.wasm'],
  ['node_modules/@rive-app/canvas/rive_fallback.wasm', 'node_modules/@rive-app/canvas/rive_fallback.wasm'],
  ['node_modules/lottie-web/build/player/lottie_light.min.js', 'node_modules/lottie-web/build/player/lottie_light.min.js'],
];

rmSync(out, { recursive: true, force: true });
for (const [from, to] of copies) {
  const src = join(v1, from);
  if (!existsSync(src)) {
    console.error(`sync-ui: missing ${from} (run npm install in the v1 folder first)`);
    process.exit(1);
  }
  // Tauri refuses a frontendDist holding a node_modules folder, so the runtimes go to vendor/.
  const dest = join(out, to.replace(/^node_modules\//, 'vendor/'));
  mkdirSync(dirname(dest), { recursive: true });
  cpSync(src, dest, { recursive: true });
}
// Badi's mascot is built in this repo (badi-mascot/, Rive CLI), not in v1.
const mascot = join(here, '..', 'badi-mascot', 'dist', 'BADI_Mascot.riv');
if (!existsSync(mascot)) {
  console.error('sync-ui: missing badi-mascot/dist/BADI_Mascot.riv (build it with `rive .` in badi-mascot)');
  process.exit(1);
}
cpSync(mascot, join(out, 'assets', 'badi-mascot.riv'));
// …and the pages load them from there (the v1 source keeps its node_modules paths).
for (const name of readdirSync(join(out, 'src', 'ui')).filter((n) => /\.(html|js)$/.test(n))) {
  const file = join(out, 'src', 'ui', name);
  const text = readFileSync(file, 'utf8');
  if (text.includes('../../node_modules/')) writeFileSync(file, text.replaceAll('../../node_modules/', '../../vendor/'));
}

// The island labels and the rest of the UI strings, from v1's i18n module (plain CommonJS).
const i18n = createRequire(import.meta.url)(join(v1, 'src', 'i18n.js'));
writeFileSync(join(out, 'i18n.json'), JSON.stringify(i18n.STRINGS));

// Tauri needs an index page; Badi itself opens its windows from Rust.
writeFileSync(join(out, 'index.html'), '<!doctype html><meta charset="utf-8"><title>aina</title>');

console.log(`sync-ui: ${copies.length} entries → ${out}`);
