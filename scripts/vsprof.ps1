# A CPU profile of `uya play --start N` in gameplay, without elevation: Visual Studio's standard collector attached to
# the running game for -Seconds after -After seconds of play, the trace written to captures\prof\<Out>.diagsession and expanded
# there (its .etl is read with the Windows Performance Toolkit's xperf). The game runs under the watcher with -Exclusive.
# Then, with the build folder's PDBs (the executable's, and the level libraries' public symbols):
#   $env:_NT_SYMBOL_PATH = "$PWD\port\target\release"
#   & 'C:\Program Files (x86)\Windows Kits\10\Windows Performance Toolkit\xperf.exe' -i <the .etl> -o captures\prof\x.txt -symbols -a profile -detail
# and sort its uya.exe lines by weight (the second column).
param([int]$Level = 1, [int]$After = 40, [int]$Seconds = 15, [string]$Out = 'veldin')
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$vsd = 'C:\Program Files\Microsoft Visual Studio\18\Community\Team Tools\DiagnosticsHub\Collector\VSDiagnostics.exe'
$cfg = 'C:\Program Files\Microsoft Visual Studio\18\Community\Team Tools\DiagnosticsHub\Collector\AgentConfigs\CpuUsageHigh.json'
$dir = Join-Path $root 'captures\prof'
New-Item -ItemType Directory -Force $dir | Out-Null
$prof = "captures/prof/profile"
& .\scripts\fresh-profile.ps1 -Profile $prof
$play = $After + $Seconds + 15
$job = Start-Job -ArgumentList $root, $Level, $play, $prof, $Out -ScriptBlock {
    param($root, $Level, $play, $prof, $Out)
    Set-Location $root
    & .\scripts\watch-run.ps1 -Exe port\target\release\uya.exe -ArgLine "play $root\disc\SCUS_973.53.iso --start $Level --press 15:800:12 --seconds $play --profile $prof --mute" -Log "captures\prof\$Out.log" -Seconds ($play + 60) -Exclusive
}
# The game's process, once it runs.
$p = $null
$sw = [Diagnostics.Stopwatch]::StartNew()
while (-not $p -and $sw.Elapsed.TotalSeconds -lt 300) {
    $p = Get-CimInstance Win32_Process -Filter "Name='uya.exe'" | Where-Object { $_.CommandLine -like "*$prof*" } | Select-Object -First 1
    if (-not $p) { Start-Sleep -Seconds 1 }
}
if (-not $p) { Receive-Job $job; throw 'the game did not start' }
Start-Sleep -Seconds $After
& $vsd start 77 /attach:$($p.ProcessId) /loadConfig:$cfg
Start-Sleep -Seconds $Seconds
$ds = Join-Path $dir "$Out.diagsession"
if (Test-Path $ds) { Remove-Item -Force $ds }
& $vsd stop 77 /output:$ds
Wait-Job $job | Out-Null
Receive-Job $job
& $vsd expandDiagSession $ds
Get-ChildItem -Recurse $dir -Filter *.etl | Select-Object FullName, Length
