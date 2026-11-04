# Idle RAM and CPU of v2 (Tauri) and v1 (Electron): each with its own fresh profile and voice off.
param([int]$Settle = 25, [int]$Window = 20)

function Tree($rootId) {
  $all = Get-CimInstance Win32_Process
  $ids = @($rootId)
  do {
    $more = $all | Where-Object { $ids -contains $_.ParentProcessId -and $ids -notcontains $_.ProcessId } | ForEach-Object { $_.ProcessId }
    $ids += $more
  } while ($more)
  $ids
}

function Probe($name, $exe, $argList, $cwd, $profile) {
  New-Item -ItemType Directory -Force $profile | Out-Null
  [IO.File]::WriteAllText("$profile\config.json", '{ "tts": "off", "wakeWord": false, "launchAtLogin": false }')
  $env:BADI_PROFILE = $profile
  $t0 = Get-Date
  if ($argList.Count) { $p = Start-Process -FilePath $exe -ArgumentList $argList -WorkingDirectory $cwd -PassThru } else { $p = Start-Process -FilePath $exe -WorkingDirectory $cwd -PassThru }
  Start-Sleep -Seconds $Settle
  $ids = Tree $p.Id
  $before = (Get-Process -Id $ids -ErrorAction SilentlyContinue | Measure-Object CPU -Sum).Sum
  Start-Sleep -Seconds $Window
  $ids = Tree $p.Id
  $procs = Get-Process -Id $ids -ErrorAction SilentlyContinue
  $after = ($procs | Measure-Object CPU -Sum).Sum
  $ram = ($procs | Measure-Object WorkingSet64 -Sum).Sum / 1MB
  $priv = ($procs | Measure-Object PrivateMemorySize64 -Sum).Sum / 1MB
  "{0}: {1} processes, working set {2:N0} MB, private {3:N0} MB, idle CPU {4:N2}% over {5}s" -f $name, $procs.Count, $ram, $priv, (($after - $before) / $Window * 100 / [Environment]::ProcessorCount), $Window
  foreach ($id in $ids) { Stop-Process -Id $id -Force -ErrorAction SilentlyContinue }
  Start-Sleep -Seconds 2
}

$root = "D:\foto lamaran\test-rea\winclicky"
Probe "v2 (Tauri, release)" "$root\badi-v2\target\release\badi.exe" @() "$root\badi-v2" "D:\badi-perf-v2"
Probe "v1 (Electron)" "$root\node_modules\electron\dist\electron.exe" @('.') $root "D:\badi-perf-v1"
