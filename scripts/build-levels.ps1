# Builds the story levels' libraries (translated\uya\levels, the workspace port\uyatr writes: one cdylib per level, tr_lNN.dll)
# into port\target\release, beside the executable that loads them (uyart::machine, `load_lib`). One cargo run for every level,
# through Visual Studio's vcvars64 from cmd, holding the PC's build slot (heavy-slot.ps1) for its whole length, as build.ps1 does
# for the port. The levels' profile (no debug info, one codegen unit a crate, no LTO, opt-level 3) is in the workspace's Cargo.toml, the
# linker (rust-lld) in the repository's .cargo\config.toml; PLAN 5.3 says why. --keep-going: one crate that fails does not stop
# the others. Output goes to captures\build-levels.log; the last line printed gives the outcome, the jobs, the peak rustc memory
# and the time.
#
#   powershell -ExecutionPolicy Bypass -File scripts\build-levels.ps1 [-Jobs N] [-LimitMB M] [-Minutes 120] [-Levels 1,2]
#
# -Levels builds only those levels' libraries (a change to the translator's level output checked on a few levels first; the
# others stay as they were, and one built against another uycpu refuses to load).
#
# -Jobs defaults to what the PC can take when the build starts: one job per 0.6 GB of free RAM above 2 GB (a rustc on one of the
# translated crates of about 5,000 instructions, with no debug info), at most 16 (the logical CPUs), at least 2. -LimitMB, the
# rustc processes' total at which the build is stopped, defaults to 0.7 GB a job plus 2 GB, and never more than the free RAM
# less 1 GB.
param(
    [int]$Jobs = 0,
    [int]$LimitMB = 0,
    [int]$Minutes = 120,
    [string]$Levels = ''
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$ws = Join-Path $root 'translated\uya\levels'
if (-not (Test-Path (Join-Path $ws 'Cargo.toml'))) {
    throw "no levels' workspace at ${ws}: run the translator first (scripts\uyatr.ps1 -Run '... levels')"
}
$vcvars = 'C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat'
$dir = Join-Path $root 'captures'
New-Item -ItemType Directory -Force $dir | Out-Null
$log = Join-Path $dir 'build-levels.log'
$bat = Join-Path $dir 'build-levels.bat'
$target = Join-Path $root 'port\target'

. (Join-Path $PSScriptRoot 'heavy-slot.ps1')
$slot = Enter-HeavySlot "build of $ws" 'build'
try {
    $freeGB = (Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB
    if ($Jobs -le 0) {
        $Jobs = [int][Math]::Max(2, [Math]::Min(16, [Math]::Floor(($freeGB - 2) / 0.6)))
    }
    $what = if ($Levels) { ($Levels.Split(",") | ForEach-Object { '-p tr-l{0:d2}' -f [int]$_ }) -join ' ' } else { '--workspace' }
    if ($LimitMB -le 0) { $LimitMB = [int][Math]::Min($Jobs * 700 + 2048, ($freeGB - 1) * 1024) }
    Set-Content -Encoding ascii $bat "@echo off`r`ncall `"$vcvars`" >nul`r`ncd /d `"$ws`"`r`ncargo build --release $what --keep-going -j $Jobs --target-dir `"$target`" > `"$log`" 2>&1`r`nexit /b %ERRORLEVEL%`r`n"
    $p = Start-Process cmd.exe -ArgumentList "/c `"$bat`"" -PassThru -WindowStyle Hidden
    $null = $p.Handle
    $peak = 0
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $why = $null
    while (-not $p.HasExited) {
        # The build's rustc processes: every rustc whose command line names this repository.
        $mb = 0
        Get-CimInstance Win32_Process -Filter "Name='rustc.exe'" | Where-Object { $_.CommandLine -like "*$root*" } | ForEach-Object {
            $proc = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
            if ($proc) { $mb += $proc.PrivateMemorySize64 / 1MB }
        }
        if ($mb -gt $peak) { $peak = [int]$mb }
        if ($mb -gt $LimitMB) { $why = "stopped: rustc at $([int]$mb) MB (limit $LimitMB MB at $Jobs jobs)"; break }
        if ($sw.Elapsed.TotalMinutes -gt $Minutes) { $why = "stopped: over $Minutes minutes"; break }
        Start-Sleep -Seconds 5
    }
    if ($why) {
        taskkill /T /F /PID $p.Id | Out-Null
        throw "$why; peak rustc $peak MB; lower -Jobs. Log: $log"
    }
    $p.WaitForExit()
} finally {
    Exit-HeavySlot $slot
}
$built = @(Get-ChildItem (Join-Path $target 'release') -Filter 'tr_l*.dll' | Where-Object { $_.LastWriteTime -gt (Get-Date).AddMinutes(-$sw.Elapsed.TotalMinutes - 1) }).Count
"exit $($p.ExitCode); $Jobs jobs; peak rustc $peak MB; $([int]$sw.Elapsed.TotalSeconds) s; $built level libraries written; log $log"
if ($p.ExitCode -ne 0) { Get-Content $log | Select-String -Pattern '^error' -Context 0, 6 | Select-Object -First 5 }
exit $p.ExitCode
