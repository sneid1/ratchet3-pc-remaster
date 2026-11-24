# Runs one command under a memory and time watch: its private memory is sampled every second and the process is stopped at
# -LimitMB (default 3072) or after -Seconds. Output goes to -Log (stderr to -Log.err); a last line in -Log.watch gives the
# outcome and the peak private memory. Every uya and Python run over the disc, dumps or packs goes through this.
#
#   & .\scripts\watch-run.ps1 -Exe port\target\release\uya.exe -ArgLine 'frames ...' -Log captures\x.log -Seconds 900
# Call it with & from PowerShell: `powershell -File` splits an -ArgLine holding a quoted path with spaces.
# The run holds the PC's run slot (heavy-slot.ps1): while another session's run holds it, this one waits (a build does not hold
# it), and -Seconds counts from the run's start. -Exclusive takes the build slot too, for a run whose numbers are timings (fields
# a second, ms per field): no build shares the CPU with it.
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$ArgLine,
    [Parameter(Mandatory = $true)][string]$Log,
    [Parameter(Mandatory = $true)][int]$Seconds,
    [int]$LimitMB = 3072,
    # Only for a measured workload whose peak fits below this with room for the desktop. Zero keeps the PC's default.
    [ValidateRange(0, 128)][double]$MinimumFreeGB = 0,
    [switch]$Exclusive
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'heavy-slot.ps1')
if ($MinimumFreeGB -gt 0) { $script:FreeGB = $MinimumFreeGB }
$slot = Enter-HeavySlot "run of $Exe $ArgLine" 'run' -Exclusive:$Exclusive
try {
    $p = Start-Process -FilePath $Exe -ArgumentList $ArgLine -RedirectStandardOutput $Log -RedirectStandardError "$Log.err" -NoNewWindow -PassThru
    # Holding the handle keeps the exit code readable after the process ends.
    $null = $p.Handle
    $t0 = Get-Date
    $peak = 0
    $outcome = 'ended'
    while (-not $p.HasExited) {
        $p.Refresh()
        $priv = $p.PrivateMemorySize64
        if ($priv -gt $peak) { $peak = $priv }
        if ($priv -gt $LimitMB * 1MB) { Stop-Process -Id $p.Id -Force; $outcome = "stopped: private memory over $LimitMB MB"; break }
        if (((Get-Date) - $t0).TotalSeconds -gt $Seconds) { Stop-Process -Id $p.Id -Force; $outcome = "stopped: over $Seconds s"; break }
        Start-Sleep -Seconds 1
    }
    $p.WaitForExit()
} finally {
    Exit-HeavySlot $slot
}
$line = "{0}, exit {1}, peak private {2:N0} MB, {3:N0} s" -f $outcome, $p.ExitCode, ($peak / 1MB), ((Get-Date) - $t0).TotalSeconds
Set-Content -Path "$Log.watch" -Value $line
Write-Output $line
