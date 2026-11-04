// Compiles the "Hey aina" listener (native/wake/BadiWake.cs, System.Speech) with the C# compiler that
// ships with Windows (.NET Framework 4), so it needs no SDK. Tauri bundles the exe (tauri.conf.json
// resources). Skipped when the exe is newer than its source (--force rebuilds).
import { execFileSync } from 'node:child_process';
import { existsSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

if (process.platform !== 'win32') process.exit(0);

const win = process.env.WINDIR || 'C:\Windows';
const csc = join(win, 'Microsoft.NET', 'Framework64', 'v4.0.30319', 'csc.exe');
const speech = join(win, 'Microsoft.NET', 'assembly', 'GAC_MSIL', 'System.Speech', 'v4.0_4.0.0.0__31bf3856ad364e35', 'System.Speech.dll');
const dir = join(dirname(fileURLToPath(import.meta.url)), '..', 'native', 'wake');
const cs = join(dir, 'BadiWake.cs');
const exe = join(dir, 'BadiWake.exe');

if (!existsSync(csc)) {
  console.error('build-wake: csc.exe not found (.NET Framework 4); "Hey aina" cannot be built.');
  process.exit(1);
}
if (existsSync(exe) && statSync(exe).mtimeMs >= statSync(cs).mtimeMs && !process.argv.includes('--force')) process.exit(0);
execFileSync(csc, ['-nologo', '-optimize+', '-target:exe', `-out:${exe}`, `-r:${speech}`, cs], { stdio: 'inherit' });
console.log('build-wake: native/wake/BadiWake.exe');
