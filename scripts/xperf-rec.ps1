# Elevated: waits for $Dir\prof-go, samples every CPU with stacks for $Seconds (xperf, 1 ms), writes $Dir\prof.etl, then $Dir\prof-done.
param([string]$Dir, [int]$Seconds = 12)
$x = 'C:\Program Files (x86)\Windows Kits\10\Windows Performance Toolkit\xperf.exe'
$log = Join-Path $Dir 'xperf-rec.log'
"started $(Get-Date -Format o)" | Set-Content $log
$sw = [Diagnostics.Stopwatch]::StartNew()
while (-not (Test-Path (Join-Path $Dir 'prof-go'))) {
    if ($sw.Elapsed.TotalSeconds -gt 600) { "no go in 600 s" | Add-Content $log; exit 1 }
    Start-Sleep -Milliseconds 200
}
& $x -on PROC_THREAD+LOADER+PROFILE -stackwalk Profile 2>&1 | Add-Content $log
Start-Sleep -Seconds $Seconds
& $x -d (Join-Path $Dir 'prof.etl') 2>&1 | Add-Content $log
"done $(Get-Date -Format o)" | Add-Content $log
New-Item -ItemType File -Force (Join-Path $Dir 'prof-done') | Out-Null
