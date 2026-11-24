# Every story level played on the port's runtime and timed (PLAN 5.1): `uya play --start N`, muted, one level after
# another under watch-run.ps1 -Exclusive (no build or other run shares the CPU), with UYA_NATIVE_TIMES=1 (the drawing's stages of
# every frame). Each level gets the presses its start needs to reach play (FIELD:BUTTONS:FIELDS, the game's pad word in hex):
# Start at field 15 (Veldin's intro movies; every other level ignores it); on the vid-comics (31-35) Cross at field 420 as well,
# which skips the intro movie that starts near field 359; on the battle levels (18, 20, 26, 29) Cross at fields 1300 and 1500,
# which pick and start a mission on the BATTLE MISSIONS menu the level opens on (75 s). Level 8 runs 95 s: the game plays its
# 44 s travel stream before it reads the level, so its world first appears near field 3270. Level 12 opens on its skydive
# mission and goes back to its BATTLE MISSIONS menu when that ends.
#
#   powershell -ExecutionPolicy Bypass -File scripts\level-times.ps1 [-Levels 1,2,...] [-Out captures\levels] [-Display fullscreen] [-Set KEY=VALUE,...]
#
# -Display: the profile's display mode (windowed, the default: a window at the monitor's shape; fullscreen: the whole monitor),
# the picture taking the shape of either (PLAN 4.2). -Set: more of the profile's settings, as settings.txt writes them
# (port/uya/src/settings.rs: the graphics options, `ambient_occlusion=high,shadows=high`); every one not named is off. -Exe: another
# build's uya.exe (main's, kept aside, for its rows on this PC).
#
# Per level the game's log in -Out\LNN.log and one row of -Out\table.md: the rate of the last 300 fields and the game code's ms
# a field (the every-300-fields line); the render over the frames that drew the world (their native-times line has the
# terrain's thread): how many, mean / 95th percentile / worst ms, how many over a field's 16.68 ms; the times the sound ran dry
# after the world first appeared; and the first stop, if any.
param([string[]]$Levels = @('1,2,3,4,5,6,7,8,9,10,11,12,13,14,16,17,18,19,20,21,22,23,24,26,27,28,29,30,31,32,33,34,35,36'), [string]$Out = 'captures\levels', [ValidateSet('windowed', 'fullscreen')][string]$Display = 'windowed', [string]$Set = '', [string]$Exe = 'port\target\release\uya.exe')
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
New-Item -ItemType Directory -Force $Out | Out-Null
$table = Join-Path $Out 'table.md'
Set-Content $table @('| Level | Fields a second (the last 300) | Game code ms a field | World frames | Render ms: mean / 95th / worst | Over 16.68 ms | Sound ran dry | Stop |', '| --- | --- | --- | --- | --- | --- | --- | --- |')
$env:UYA_NATIVE_TIMES = '1'
try {
    # -Levels 1,2 from PowerShell is an array, from `powershell -File` one string: both split to the numbers.
    foreach ($n in $Levels -split '[,\s]+' | Where-Object { $_ } | ForEach-Object { [int]$_ }) {
        $press = '15:800:12'
        $seconds = 45
        if ($n -ge 31 -and $n -le 35) { $press += ',420:40:4' }
        if ($n -in 18, 20, 26, 29) { $press += ',1300:40:4,1500:40:4'; $seconds = 75 }
        if ($n -eq 8) { $seconds = 95 }
        $l = '{0:D2}' -f $n
        # A fresh profile for each level, with the pad's prompt atlas the player's profile holds (without it every run builds its
        # own) and its kept copy of the texture packs (the game always draws them).
        $prof = Join-Path $Out 'profile'
        & .\scripts\fresh-profile.ps1 -Profile $prof
        $more = ($Set -split ',' | Where-Object { $_ } | ForEach-Object { ($_ -replace '=', ' = ') + "`n" }) -join ''
        Set-Content -Encoding ascii (Join-Path $prof 'settings.txt') "display = $Display`nresolution = 1080`n$more"
        $log = Join-Path $Out "L$l.log"
        & .\scripts\watch-run.ps1 -Exe $Exe -ArgLine "play $root\disc\SCUS_973.53.iso --start $n --press $press --seconds $seconds --profile $prof --mute" -Log $log -Seconds ($seconds + 60) -Exclusive | Out-Null
        $lines = Get-Content $log
        $rate = $lines | Select-String 'the last 300 at' | Select-Object -Last 1
        $r = if ($rate -and $rate.Line -match 'the last 300 at ([\d.]+); ms per field: input [\d.]+, EE ([\d.]+)') { @($Matches[1], $Matches[2]) } else { @('-', '-') }
        $w = @($lines | Select-String "native times: level $n\b" | Where-Object { $_.Line -match 'terrain \(its thread\)' } | ForEach-Object { [double]($_.Line -replace '.*: ([\d.]+) ms:.*', '$1') })
        $stats = '-'; $over = '-'
        if ($w.Count -gt 0) {
            $s = @($w | Sort-Object)
            $stats = '{0:N1} / {1:N1} / {2:N1}' -f ($w | Measure-Object -Average).Average, $s[[int][Math]::Floor($s.Count * 0.95)], $s[-1]
            $over = @($w | Where-Object { $_ -gt 16.68 }).Count
        }
        $first = $lines | Select-String "level $n\b.*the native picture" | Select-Object -First 1
        $dry = if ($first) { @($lines[$first.LineNumber..($lines.Count - 1)] | Select-String 'the ring ran dry').Count } else { '-' }
        $stop = (Get-Content $log, "$log.err" | Select-String -Pattern 'panicked|no translated code|stops' | Select-Object -First 1 | ForEach-Object { $_.Line.Substring(0, [Math]::Min(120, $_.Line.Length)) }) -join ''
        $row = "| $n | $($r[0]) | $($r[1]) | $($w.Count) | $stats | $over | $dry | $stop |"
        Add-Content $table $row
        $row
    }
}
finally {
    Remove-Item Env:\UYA_NATIVE_TIMES
}
