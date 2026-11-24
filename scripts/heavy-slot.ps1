# The PC's heavy-job slots (the project's workflow notes): build slots and run slots (a run: a game, a generator, a
# translator, a Python run over the disc), as many as the PC can take (below). A job holds its slot for its whole length, so
# sessions working in parallel in their own worktrees take turns: up to two builds at once on a big PC (a long build no longer
# holds every other session's back; a build started while another runs takes half the CPU, build.ps1), and up to three runs beside them (each run is held under 2 GB by
# watch-run.ps1), so a one-second tool run does not wait behind a five-minute game run. A run whose numbers are timings (fields a
# second, ms per field) takes every slot (-Exclusive), so nothing shares the CPU with it. Each slot is a named mutex of the
# Windows logon session: a holder that dies releases it (the next waiter takes it over as abandoned). After taking its slot, a
# job waits for the free RAM below. Dot-sourced by build.ps1, watch-run.ps1 and uyatr.ps1; uyactl.py takes the same mutexes.
# The slot counts and the free RAM a job waits for come from the PC itself, so a smaller machine runs fewer heavy jobs at once:
# two build slots only with 28 GB of RAM and 12 logical CPUs (a build takes about 1 GB a rustc job and half the CPUs when two
# run); three run slots with 28 GB, two with 14 GB, else one; and a job waits for a fifth of the RAM free, at most 6 GB. On the
# 32 GB, 16-thread PC the project started on that is 2, 3 and 6 GB. uyactl.py works the same numbers out the same way.
$script:Pc = Get-CimInstance Win32_ComputerSystem
$script:TotalGB = $script:Pc.TotalPhysicalMemory / 1GB
$script:BuildSlots = if ($script:TotalGB -ge 28 -and $script:Pc.NumberOfLogicalProcessors -ge 12) { 2 } else { 1 }
$script:RunSlots = if ($script:TotalGB -ge 28) { 3 } elseif ($script:TotalGB -ge 14) { 2 } else { 1 }
$script:FreeGB = [Math]::Min(6, [Math]::Floor($script:TotalGB / 5))

function Slot([string]$Name) { [Threading.Mutex]::new($false, "Local\uya-$Name-slot") }

# Waits for one of the mutexes $ms and returns the one taken.
function Wait-Any([Threading.Mutex[]]$ms, [string]$What, [string]$Name) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $said = -1
    while ($true) {
        try {
            $i = [Threading.WaitHandle]::WaitAny($ms, 30000)
            if ($i -ne [Threading.WaitHandle]::WaitTimeout) { return $ms[$i] }
        } catch [Threading.AbandonedMutexException] {
            # The last holder died holding it; the slot is this job's now. PowerShell may wrap the exception.
            $e = $_.Exception
            while (-not ($e -is [Threading.AbandonedMutexException])) { $e = $e.InnerException }
            return $ms[[Math]::Max(0, $e.MutexIndex)]
        }
        $min = [int]$sw.Elapsed.TotalMinutes
        if ($said -lt 0 -or $min - $said -ge 5) {
            $said = $min
            $busy = Get-CimInstance Win32_Process | Where-Object { $_.Name -in 'cargo.exe', 'uya.exe', 'uyafmt.exe' } | ForEach-Object { "$($_.Name) $($_.ProcessId)" }
            Write-Host "heavy slot: $What waits for a $Name slot ($min min so far); running: $($busy -join ', ')"
        }
    }
}

# -Kind build or run; -Exclusive (runs only) takes every build slot and every run slot. Returns the mutexes held, for
# Exit-HeavySlot.
function Enter-HeavySlot([string]$What, [string]$Kind, [switch]$Exclusive) {
    if ($Kind -notin 'build', 'run') { throw "heavy slot: kind '$Kind' is neither build nor run" }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $builds = @(1..$script:BuildSlots | ForEach-Object { Slot "build$_" })
    $runs = @(1..$script:RunSlots | ForEach-Object { Slot "run$_" })
    $held = @()
    if ($Kind -eq 'build') {
        $held += Wait-Any $builds $What 'build'
    } elseif ($Exclusive) {
        # Always the build slots first, then the run slots, each in order, so two jobs can never each hold what the other waits for.
        foreach ($m in $builds) { $held += Wait-Any @($m) $What 'build' }
        foreach ($m in $runs) { $held += Wait-Any @($m) $What 'run' }
    } else {
        $held += Wait-Any $runs $What 'run'
    }
    while ((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1MB -lt $script:FreeGB) {
        Write-Host "heavy slot: $What waits for $($script:FreeGB) GB of free RAM"
        Start-Sleep -Seconds 30
    }
    Write-Host ("heavy slot: {0} starts after {1:N0} s ({2}{3})" -f $What, $sw.Elapsed.TotalSeconds, $Kind, $(if ($Exclusive) { ', exclusive' } else { '' }))
    return , $held
}

# Whether a build other than the caller's holds a build slot now (the caller holds its own).
function Test-OtherBuild($held) {
    foreach ($n in 1..$script:BuildSlots) {
        $m = Slot "build$n"
        try {
            # The caller's own slot is re-entrant for its thread: it answers true here and is released at once, as is a free one.
            $got = $m.WaitOne(0)
        } catch [Threading.AbandonedMutexException] {
            $got = $true
        }
        if ($got) { $m.ReleaseMutex() } else { $m.Dispose(); return $true }
        $m.Dispose()
    }
    return $false
}

function Exit-HeavySlot($held) {
    # Released in the reverse of the order taken.
    for ($i = $held.Count - 1; $i -ge 0; $i--) {
        $held[$i].ReleaseMutex()
        $held[$i].Dispose()
    }
}
