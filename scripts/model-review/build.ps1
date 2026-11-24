# Build only the isolated review snapshot. Reuse compiled dependencies; never build uya.
param([int]$Jobs=2, [int]$LimitMB=2800, [int]$Minutes=12, [double]$MinimumFreeGB=3, [switch]$LinkOnly, [switch]$ReuseSnapshot, [switch]$Game)
$ErrorActionPreference='Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$build = Join-Path $root 'captures/rift-texture-review/viewer-build'
if (-not $LinkOnly -and -not $ReuseSnapshot) {
    python (Join-Path $PSScriptRoot 'prepare.py')
    if ($LASTEXITCODE -ne 0) { throw 'Could not prepare the isolated viewer' }
}
if ($ReuseSnapshot) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'material_review.rs') -Destination (Join-Path $build 'src/material_review.rs')
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'review_main.rs') -Destination (Join-Path $build 'src/review_main.rs')
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'mesh_edit.rs') -Destination (Join-Path $build 'src/mesh_edit.rs')
    python (Join-Path $PSScriptRoot 'channels.py')
    if ($LASTEXITCODE -ne 0) { throw 'Could not update review channel hooks' }
    python (Join-Path $PSScriptRoot 'source_lod.py')
    if ($LASTEXITCODE -ne 0) { throw 'Could not update source LOD hooks' }
    python (Join-Path $PSScriptRoot 'family_shader.py')
    if ($LASTEXITCODE -ne 0) { throw 'Could not update material family shader' }
    python (Join-Path $PSScriptRoot 'probe_shader.py')
    if ($LASTEXITCODE -ne 0) { throw 'Could not update captured level reflections' }
    python (Join-Path $PSScriptRoot 'emissive_shader.py')
    if ($LASTEXITCODE -ne 0) { throw 'Could not update explicit emissive materials' }
    python (Join-Path $PSScriptRoot 'pbr_mixed_atlas.py')
    if ($LASTEXITCODE -ne 0) { throw 'Could not preserve mixed-atlas fur and lens masks' }
}
. (Join-Path $root 'scripts/heavy-slot.ps1')
$script:FreeGB=$MinimumFreeGB
$held=Enter-HeavySlot 'isolated model review build' 'build'
$log=Join-Path $build 'build.log'
$bat=Join-Path $build 'build.cmd'
$vcvars='C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat'
$target=Join-Path $root 'port/target'
try {
    $libraryCommand=if ($LinkOnly) { '' } else { "rustc @library.rsp > `"$log`" 2>&1`r`nif errorlevel 1 exit /b 1`r`n" }
    $gameCommand=if ($Game) { "if errorlevel 1 exit /b 1`r`nrustc @game.rsp >> `"$log`" 2>&1`r`n" } else { '' }
    Set-Content -LiteralPath $bat -Encoding ascii -Value "@echo off`r`ncall `"$vcvars`" >nul`r`ncd /d `"$build`"`r`n${libraryCommand}rustc @binary.rsp >> `"$log`" 2>&1`r`n${gameCommand}exit /b %ERRORLEVEL%`r`n"
    $process=Start-Process cmd.exe -ArgumentList "/c `"$bat`"" -PassThru -WindowStyle Hidden
    $null=$process.Handle
    $timer=[Diagnostics.Stopwatch]::StartNew()
    $peak=0
    $reason=$null
    while (-not $process.HasExited) {
        # Count only descendants of this builder; other sessions keep their own caps.
        $all=@(Get-CimInstance Win32_Process)
        $ids=[System.Collections.Generic.HashSet[int]]::new()
        $null=$ids.Add($process.Id)
        do {
            $count=$ids.Count
            foreach ($p in $all) { if ($ids.Contains([int]$p.ParentProcessId)) { $null=$ids.Add([int]$p.ProcessId) } }
        } while ($ids.Count -gt $count)
        $mb=0
        foreach ($idValue in $ids) {
            $p=Get-Process -Id $idValue -ErrorAction SilentlyContinue
            if ($p) { $mb += $p.PrivateMemorySize64/1MB }
        }
        $peak=[Math]::Max($peak,[int]$mb)
        if ($mb -gt $LimitMB) { $reason="memory cap $LimitMB MB"; break }
        if ($timer.Elapsed.TotalMinutes -gt $Minutes) { $reason="time cap $Minutes minutes"; break }
        Start-Sleep -Seconds 2
    }
    if ($reason) { taskkill /T /F /PID $process.Id | Out-Null; throw "Review build stopped: $reason" }
    $process.WaitForExit()
    if ($process.ExitCode -ne 0) { Get-Content -LiteralPath $log -Tail 75; throw "Review build failed: $($process.ExitCode)" }
    Write-Host "Review build complete; peak $peak MB; $([int]$timer.Elapsed.TotalSeconds) s; $log"
} finally { Exit-HeavySlot $held }
