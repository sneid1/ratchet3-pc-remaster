# Builds the port's translator (port/uyatr, a workspace of its own: the port's workspace needs the translator's output first)
# through Visual Studio's vcvars64 with a -j limit, under the PC's heavy-job slot, and stops rustc above -LimitMB as build.ps1
# does. With -Run, then runs it under the watcher (scripts\watch-run.ps1) with that argument line, from port\ (so its relative
# paths are the port's): for example
#
#   powershell -ExecutionPolicy Bypass -File scripts\uyatr.ps1 -Run '<the repository>/disc/SCUS_973.53.iso ../translated/bounds.tsv ../translated/uya loader boot fe100 fe101 levels'
#
# translates those programs into translated/uya (gitignored: game code made from the player's disc; `levels` is every story
# level, each a library of its own in translated/uya/levels, a member of the port's workspace). The build's log is
# captures\uyatr-build.log, the run's captures\uyatr.log (-Log to name another).
param(
    [int]$Jobs = 4,
    [int]$LimitMB = 8192,
    [string]$Run = '',
    [string]$Log = 'uyatr.log',
    [int]$Seconds = 1800,
    # As build.ps1's: a lower free-RAM threshold for the build and the run, the memory caps kept.
    [ValidateRange(0, 128)][double]$MinimumFreeGB = 0
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$vcvars = 'C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat'
$dir = Join-Path $root 'captures'
New-Item -ItemType Directory -Force $dir | Out-Null
$blog = Join-Path $dir 'uyatr-build.log'
$bat = Join-Path $dir 'uyatr-build.bat'
Set-Content -Encoding ascii $bat "@echo off`r`ncall `"$vcvars`" >nul`r`ncd /d `"$root\port\uyatr`"`r`ncargo build --release -j $Jobs > `"$blog`" 2>&1`r`nexit /b %ERRORLEVEL%`r`n"

. (Join-Path $PSScriptRoot 'heavy-slot.ps1')
if ($MinimumFreeGB -gt 0) { $script:FreeGB = $MinimumFreeGB }
$slot = Enter-HeavySlot "build of $root\port\uyatr" 'build'
try {
    $p = Start-Process cmd.exe -ArgumentList "/c `"$bat`"" -PassThru -WindowStyle Hidden
    $null = $p.Handle
    $peak = 0
    $why = $null
    while (-not $p.HasExited) {
        $mb = 0
        Get-CimInstance Win32_Process -Filter "Name='rustc.exe'" | Where-Object { $_.CommandLine -like "*$root*" } | ForEach-Object {
            $proc = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
            if ($proc) { $mb += $proc.PrivateMemorySize64 / 1MB }
        }
        if ($mb -gt $peak) { $peak = [int]$mb }
        if ($mb -gt $LimitMB) { $why = "stopped: rustc at $([int]$mb) MB"; break }
        Start-Sleep -Seconds 2
    }
    if ($why) {
        taskkill /T /F /PID $p.Id | Out-Null
        throw "$why; log $blog"
    }
    $p.WaitForExit()
} finally {
    Exit-HeavySlot $slot
}
"uyatr build: exit $($p.ExitCode); peak rustc $peak MB; log $blog"
if ($p.ExitCode -ne 0) {
    Get-Content $blog | Select-String -Pattern '^error' -Context 0, 8 | Select-Object -First 8
    exit $p.ExitCode
}
if ($Run) {
    Set-Location (Join-Path $root 'port')
    & (Join-Path $PSScriptRoot 'watch-run.ps1') -Exe (Join-Path $root 'port\uyatr\target\release\uyatr.exe') -ArgLine $Run -Log (Join-Path $dir $Log) -Seconds $Seconds -LimitMB 6144 -MinimumFreeGB $MinimumFreeGB
}
