# Builds port/ (release) through Visual Studio's vcvars64 from cmd, with a -j limit, and stops the build when its rustc
# processes pass -LimitMB of private memory in total or after -Minutes. Output goes to captures\build.log; the last line printed
# gives the outcome, the jobs, the peak rustc memory and the time.
#
#   powershell -ExecutionPolicy Bypass -File scripts\build.ps1 [-Jobs N] [-LimitMB M] [-Minutes 40] [-Package uyafmt|uyaref]
#
# Without -Package, port/'s workspace is built: the game (uya.exe, linking the translated programs) and its own crates uyafmt,
# uyart and uycpu; it reads nothing of rac-rs or generated/uya. -Package NAME builds that member of it only (cargo build -p).
# -Package uyaref builds the emulated reference instead, a workspace of its own in port\uyaref (its own Cargo.lock and target
# folder, port\uyaref\target\release\uyaref.exe: rac-rs, generated/uya, and the game's library and translated programs again).
# The build holds the PC's build slot (heavy-slot.ps1): while another session's build holds it, this one waits (a game run
# does not hold it), and -Minutes counts from the build's start.
# -Jobs defaults to what the PC can take when the build starts: every logical CPU but two, at most one job per GB of free RAM
# above 4 GB (a rustc on the generated EE crates peaks near 0.85 GB), at least 2. -LimitMB defaults to 1 GB a job plus 2 GB.
param(
    [int]$Jobs = 0,
    [int]$LimitMB = 0,
    # A known incremental build can use a lower launch threshold while keeping the memory/time caps. Zero keeps the default.
    [ValidateRange(0, 128)][double]$MinimumFreeGB = 0,
    [int]$Minutes = 40,
    [string]$Package = ''
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$vcvars = 'C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat'
$dir = Join-Path $root 'captures'
New-Item -ItemType Directory -Force $dir | Out-Null
$log = Join-Path $dir 'build.log'
$bat = Join-Path $dir 'build.bat'
$cwd = if ($Package -eq 'uyaref') { "$root\port\uyaref" } else { "$root\port" }
$only = if ($Package -and $Package -ne 'uyaref') { " -p $Package" } else { '' }

. (Join-Path $PSScriptRoot 'heavy-slot.ps1')
if ($MinimumFreeGB -gt 0) { $script:FreeGB = $MinimumFreeGB }
$slot = Enter-HeavySlot "build of $cwd$only" 'build'
try {
    if ($Jobs -le 0) {
        $cores = (Get-CimInstance Win32_Processor | Measure-Object NumberOfLogicalProcessors -Sum).Sum
        $freeGB = (Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB
        # Another session's build running beside this one: half the CPU each.
        $cpu = if (Test-OtherBuild $slot) { [Math]::Floor(($cores - 2) / 2) } else { $cores - 2 }
        $Jobs = [int][Math]::Max(2, [Math]::Min($cpu, [Math]::Floor($freeGB - 4)))
    }
    if ($LimitMB -le 0) { $LimitMB = $Jobs * 1024 + 2048 }
    Set-Content -Encoding ascii $bat "@echo off`r`ncall `"$vcvars`" >nul`r`ncd /d `"$cwd`"`r`ncargo build --release -j $Jobs$only > `"$log`" 2>&1`r`nexit /b %ERRORLEVEL%`r`n"
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
        throw "$why; peak rustc $peak MB; lower -Jobs or the generator's --chunk. Log: $log"
    }
    $p.WaitForExit()
} finally {
    Exit-HeavySlot $slot
}
# DLSS Frame Generation's DLLs (port/uya/src/streamline.rs) beside uya.exe, from STREAMLINE_SDK (NVIDIA's Streamline SDK folder,
# bin\x64): a file the SDK lacks fails the build.
if ($p.ExitCode -eq 0 -and $env:STREAMLINE_SDK -and -not $Package) {
    foreach ($f in 'sl.interposer.dll', 'sl.common.dll', 'sl.dlss_g.dll', 'sl.reflex.dll', 'sl.pcl.dll', 'nvngx_dlssg.dll', 'NvLowLatencyVk.dll') {
        $from = Join-Path $env:STREAMLINE_SDK "bin\x64\$f"
        if (-not (Test-Path $from)) { throw "STREAMLINE_SDK: $from is missing" }
        Copy-Item -Force $from "$root\port\target\release\$f"
    }
}
"exit $($p.ExitCode); $Jobs jobs; peak rustc $peak MB; $([int]$sw.Elapsed.TotalSeconds) s; log $log"
if ($p.ExitCode -ne 0) { Get-Content $log | Select-String -Pattern '^error' -Context 0, 6 | Select-Object -First 5 }
exit $p.ExitCode
