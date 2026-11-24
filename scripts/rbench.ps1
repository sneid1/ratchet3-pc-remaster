# The native renderer on saved tour dumps (-Dumps, default captures/tour8/ram in the main checkout): one picture per level into captures\$Out,
# and 20 redraws of Veldin's gameplay dump (field 1800) timed, with UYA_NATIVE_TIMES set. -Exe redraws with another build of `uya`
# (a baseline's executable kept aside), default this checkout's release build.
param([string]$Out = 'rbase', [string]$Dumps = '', [string]$Exe = 'port\target\release\uya.exe')
$wt = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $wt
# The dumps live in the main checkout (a worktree has no captures): the parent of git's common directory.
if (-not $Dumps) { $Dumps = Join-Path (Split-Path -Parent (git -C $wt rev-parse --path-format=absolute --git-common-dir)) 'captures\tour8\ram' }
$ram = $Dumps
$iso = "$wt\disc\SCUS_973.53.iso"
$dir = "$wt\captures\$Out"
New-Item -ItemType Directory -Force $dir | Out-Null
$env:UYA_NATIVE_TIMES = '1'
# Each level's last dump (its level_current at 0x1D545C read from the dumps).
$set = @(@{ L = 1; F = 1800 }, @{ L = 2; F = 3600 }, @{ L = 4; F = 6300 }, @{ L = 6; F = 9000 }, @{ L = 8; F = 12600 }, @{ L = 10; F = 16200 }, @{ L = 14; F = 21600 }, @{ L = 19; F = 27000 }, @{ L = 23; F = 32400 }, @{ L = 28; F = 37800 }, @{ L = 31; F = 43200 })
foreach ($s in $set) {
    $f = '{0:D5}' -f $s.F
    & .\scripts\watch-run.ps1 -Exe $Exe -ArgLine "native $iso $ram\ram$f.bin $($s.L) $dir\L$($s.L).png" -Log "$dir\L$($s.L).log" -Seconds 120 -LimitMB 4096 | Select-Object -Last 1
}
$many = (1..21 | ForEach-Object { "$ram\ram01800.bin" }) -join ','
& .\scripts\watch-run.ps1 -Exe $Exe -ArgLine "native $iso $many 1 $dir\L1x21.png" -Log "$dir\L1x21.log" -Seconds 300 -LimitMB 4096 -Exclusive | Select-Object -Last 1
Get-ChildItem $dir -Filter *.png | Select-Object Name, Length
