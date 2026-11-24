# PLAN 9.2 step 1: each terrain and tie texture's share of the picture on RAM dumps. One `uya native --surfaces` redraw per
# dump (every ram*.bin under -Dumps: the gameplay dumps of scripts\surfaces-gameplay.py, one folder a level; by default the
# saved tour's, captures\tour8\ram in the main checkout, many of them loads and cutscenes), under the run watcher, into
# captures\$Out: per dump
# fNNNNN-LL.tsv (the counts, `port/uya/src/surfaces.rs`) and fNNNNN-LL.png (the picture in the textures' id colours). The
# level is the dump's level_current (u32 at 0x1D545C). scripts\uya-surfaces.py makes docs\surfaces.csv from the folder.
param([string]$Out = 'surfaces', [string]$Dumps = '', [string]$Exe = 'port\target\release\uya.exe', [int]$Height = 896, [switch]$Skip)
$ErrorActionPreference = 'Stop'
$wt = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $wt
if (-not $Dumps) { $Dumps = Join-Path (Split-Path -Parent (git -C $wt rev-parse --path-format=absolute --git-common-dir)) 'captures\tour8\ram' }
$iso = "$wt\disc\SCUS_973.53.iso"
$dir = "$wt\captures\$Out"
New-Item -ItemType Directory -Force $dir | Out-Null
foreach ($ram in Get-ChildItem $Dumps -Filter 'ram*.bin' -Recurse | Sort-Object FullName) {
    $f = $ram.BaseName.Substring(3)
    $fs = [System.IO.File]::OpenRead($ram.FullName)
    try {
        $b = New-Object byte[] 4
        $fs.Position = 0x1D545C
        if ($fs.Read($b, 0, 4) -ne 4) { throw "$($ram.FullName): shorter than 0x1D5460" }
    } finally { $fs.Close() }
    $level = [BitConverter]::ToUInt32($b, 0)
    $name = 'f{0}-{1:D2}' -f $f, $level
    # -Skip: a dump already measured (its tsv there) is not measured again.
    if ($Skip -and (Test-Path "$dir\$name.tsv")) { continue }
    $line = & .\scripts\watch-run.ps1 -Exe $Exe -ArgLine "native $iso $($ram.FullName) $level $dir\$name.png --height $Height --surfaces $dir\$name.tsv" -Log "$dir\$name.log" -Seconds 180 -LimitMB 4096 | Select-Object -Last 1
    "{0}: level {1}: {2}" -f $f, $level, $line
}
