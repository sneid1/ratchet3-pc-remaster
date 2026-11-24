# A profile of Veldin gameplay on the port's runtime: the elevated xperf helper (one UAC prompt), then NEW GAME into Veldin
# (no pause), with the sampling started once the log reaches field 1800.
param([int]$Seconds = 60, [string]$Out = 'prof1', [string]$Play = '--press 350:800:100,1100:40:4,1140:800:30 --click 800:0.502:0.417', [string]$Trigger = 'play: field 1800,')
$wt = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $wt
$dir = "$wt\captures\$Out"
if (Test-Path $dir) { Remove-Item -Recurse -Force $dir }
New-Item -ItemType Directory -Force $dir | Out-Null
Start-Process powershell -Verb RunAs -WindowStyle Hidden -ArgumentList "-ExecutionPolicy Bypass -File `"$wt\scripts\xperf-rec.ps1`" -Dir `"$dir`" -Seconds 12"
$prof = "$wt\captures\prof-profile"
& .\scripts\fresh-profile.ps1 -Profile $prof
$log = "$dir\run.log"
$go = Start-Job -ArgumentList $log, "$dir\prof-go", $Trigger -ScriptBlock {
    param($log, $go, $trig)
    $sw = [Diagnostics.Stopwatch]::StartNew()
    while ($sw.Elapsed.TotalSeconds -lt 300) {
        if ((Test-Path $log) -and (Select-String -Path $log -Pattern $trig -Quiet)) { New-Item -ItemType File -Force $go | Out-Null; break }
        Start-Sleep -Milliseconds 250
    }
}
& .\scripts\watch-run.ps1 -Exe port\target\release\uya.exe -ArgLine "play $wt\disc\SCUS_973.53.iso --seconds $Seconds --profile $($prof -replace '\\','/') --mute $Play" -Log $log -Seconds ($Seconds + 60) -LimitMB 4096 -Exclusive
Remove-Job $go -Force
$sw = [Diagnostics.Stopwatch]::StartNew()
while (-not (Test-Path "$dir\prof-done") -and $sw.Elapsed.TotalSeconds -lt 120) { Start-Sleep -Milliseconds 500 }
Get-Content "$dir\xperf-rec.log" -ErrorAction SilentlyContinue
Get-ChildItem $dir | Select-Object Name, Length
