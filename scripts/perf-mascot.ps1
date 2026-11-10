# The notch's performance scenarios (AINA_DEMO=mascot in demo.rs) on the release build, with a fresh
# profile and voice off. While they run, every ~5 s: CPU and memory of aina.exe and each WebView2
# process by type (browser, renderer, gpu-process, utility), and the 3D engine use and dedicated GPU
# memory of those processes. Then perf-summary.mjs joins it with the page's own numbers.
#   .\scripts\perf-mascot.ps1 -Label A                       # every scenario (~50 min)
#   .\scripts\perf-mascot.ps1 -Label smoke -Scale 0.05       # a quick check of the harness
#   .\scripts\perf-mascot.ps1 -Label B -Scenarios toggle100,expr
param([string]$Label = 'run', [double]$Scale = 1, [string[]]$Scenarios = @('all'), [int]$Every = 5)

$root = Split-Path $PSScriptRoot -Parent
$out = Join-Path $root "perf\$Label"
$profileDir = Join-Path $env:TEMP "aina-perf-profile-$Label"
Remove-Item -Recurse -Force $out, $profileDir -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $out, $profileDir | Out-Null
[IO.File]::WriteAllText("$profileDir\config.json", '{ "tts": "off", "wakeWord": false, "launchAtLogin": false }')

$env:AINA_PROFILE = $profileDir
$env:AINA_DEMO = 'mascot'
$env:AINA_DEMO_OUT = $out
$env:AINA_DEMO_MASCOT = $Scenarios -join ','
$env:AINA_PERF_SCALE = "$Scale"
$exe = Join-Path $root 'target\release\aina.exe'
$app = Start-Process -FilePath $exe -WorkingDirectory $root -PassThru -RedirectStandardOutput "$out\stdout.log" -RedirectStandardError "$out\stderr.log"
$cores = [Environment]::ProcessorCount

function Tree($rootId) {
  $all = Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, CommandLine, Name
  $ids = @($rootId)
  do {
    $more = $all | Where-Object { $ids -contains $_.ParentProcessId -and $ids -notcontains $_.ProcessId } | ForEach-Object { $_.ProcessId }
    $ids += $more
  } while ($more)
  $all | Where-Object { $ids -contains $_.ProcessId } | ForEach-Object {
    $type = if ($_.ProcessId -eq $rootId) { 'app' } elseif ($_.CommandLine -match '--type=(\S+)') { $Matches[1] } else { 'browser' }
    # [int]: CIM ids are UInt32, and hashtable keys of another type never match the counters' ids.
    [pscustomobject]@{ Id = [int]$_.ProcessId; Type = $type }
  }
}

$csv = "$out\process.csv"
't,type,count,cpu_pct,ws_mb,private_mb,gpu3d_pct,gpu_dedicated_mb' | Set-Content $csv -Encoding ascii
$last = @{}
while (-not $app.HasExited) {
  # WebView2 starts its renderers and GPU process after a moment, so the tree is read every time.
  $tree = @(Tree $app.Id)
  # Each process's CPU time with the moment it was read, so a slow counter query can't skew it.
  $procs = @{}
  foreach ($p in $tree) {
    $gp = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
    if ($gp) { $procs[$p.Id] = @{ P = $gp; Cpu = $gp.TotalProcessorTime.TotalSeconds; At = [Diagnostics.Stopwatch]::GetTimestamp() } }
  }
  $gpu = @{}; $mem = @{}
  try {
    $c = Get-Counter '\GPU Engine(*engtype_3D)\Utilization Percentage', '\GPU Process Memory(*)\Dedicated Usage' -ErrorAction Stop
    foreach ($s in $c.CounterSamples) {
      if ($s.InstanceName -match '^pid_(\d+)_') {
        $id = [int]$Matches[1]
        if ($s.Path -like '*utilization percentage') { $gpu[$id] = $gpu[$id] + $s.CookedValue } else { $mem[$id] = $mem[$id] + $s.CookedValue }
      }
    }
  } catch {}
  $t = [DateTimeOffset]::Now.ToUnixTimeMilliseconds()
  foreach ($g in ($tree | Group-Object Type)) {
    $cpu = 0; $ws = 0; $priv = 0; $g3 = 0; $gm = 0; $n = 0
    foreach ($p in $g.Group) {
      $r = $procs[$p.Id]; if (-not $r) { continue }
      $n++
      $prev = $last[$p.Id]
      if ($prev) {
        $secs = ($r.At - $prev.At) / [Diagnostics.Stopwatch]::Frequency
        if ($secs -gt 0) { $cpu += ($r.Cpu - $prev.Cpu) / $secs * 100 / $cores }
      }
      $last[$p.Id] = $r
      $ws += $r.P.WorkingSet64 / 1MB; $priv += $r.P.PrivateMemorySize64 / 1MB
      $g3 += $gpu[$p.Id]; $gm += $mem[$p.Id] / 1MB
    }
    '{0},{1},{2},{3:F3},{4:F1},{5:F1},{6:F3},{7:F1}' -f $t, $g.Name, $n, $cpu, $ws, $priv, $g3, $gm | Add-Content $csv -Encoding ascii
  }
  Start-Sleep -Seconds $Every
}
Write-Host "aina.exe exited ($($app.ExitCode)); summarizing $out"
node (Join-Path $PSScriptRoot 'perf-summary.mjs') $out
