# Makes the desktop shortcut that starts the game: `uya play` on the player's disc image with the player's profile folder
# (saves, settings, the button-prompt atlas, the window's icon). PLAN item 4.4.
#
#   powershell -ExecutionPolicy Bypass -File scripts\make-shortcut.ps1 [-Profile DIR] [-Iso FILE] [-Exe FILE] [-Out FILE] [-Rift DIR]
#
# The shortcut's target is the release build of `uya` of the checkout this script is in, its arguments are `play ISO --profile
# DIR`, its working folder the profile (the folder a run writes RAM dumps and logs to), its icon the release executable's embedded portrait.
# Without -Out the shortcut is `Ratchet & Clank - Up Your Arsenal.lnk` on the desktop.
# -Rift DIR adds `--rift DIR`: RIFT APART SURFACES draws the Rift Apart folder DIR (PLAN 9.2) instead of the profile's
# texture_replacements\0-riftapart.
param(
    [string]$Profile = (Join-Path $env:USERPROFILE 'Games\Ratchet and Clank 3'),
    [string]$Iso = '',
    [string]$Exe = '',
    [string]$Out = '',
    [string]$Rift = ''
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
if (-not $Iso) { $Iso = Join-Path $root 'disc\SCUS_973.53.iso' }
if (-not $Exe) { $Exe = Join-Path $root 'port\target\release\uya.exe' }
if (-not $Out) { $Out = Join-Path ([Environment]::GetFolderPath('Desktop')) 'Ratchet & Clank - Up Your Arsenal.lnk' }
foreach ($f in @($Iso, $Exe)) { if (-not (Test-Path -LiteralPath $f)) { throw "make-shortcut: $f does not exist" } }
# The story levels 1-14, 16-24 and 26-36: a level whose library is missing stops the game when it loads.
$levels = @(1..14) + @(16..24) + @(26..36)
$missing = @($levels | Where-Object { -not (Test-Path -LiteralPath (Join-Path (Split-Path -Parent $Exe) ('tr_l{0:D2}.dll' -f $_))) })
if ($missing.Count) { throw "make-shortcut: no level library beside $Exe for levels $($missing -join ', ') (scripts\build-levels.ps1 builds them)" }
New-Item -ItemType Directory -Force $Profile | Out-Null
$Iso = (Resolve-Path -LiteralPath $Iso).Path
$Exe = (Resolve-Path -LiteralPath $Exe).Path
$sh = New-Object -ComObject WScript.Shell
$lnk = $sh.CreateShortcut($Out)
$lnk.TargetPath = $Exe
$lnk.Arguments = "play `"$Iso`" --profile `"$Profile`""
if ($Rift) {
    if (-not (Test-Path -LiteralPath (Join-Path $Rift 'surfaces.tsv'))) { throw "make-shortcut: $Rift has no surfaces.tsv" }
    $lnk.Arguments += " --rift `"$((Resolve-Path -LiteralPath $Rift).Path)`""
}
$lnk.WorkingDirectory = $Profile
$lnk.IconLocation = "$Exe,0"
$lnk.Description = 'Ratchet & Clank: Up Your Arsenal'
$lnk.Save()
Write-Host "make-shortcut: $Out"
Write-Host "  target    $($lnk.TargetPath)"
Write-Host "  arguments $($lnk.Arguments)"
Write-Host "  start in  $($lnk.WorkingDirectory)"
Write-Host "  icon      $($lnk.IconLocation)"
