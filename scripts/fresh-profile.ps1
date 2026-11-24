# A fresh profile folder for a scripted `uya play` run: everything in -Profile removed but its copy of the player's texture packs
# (`texture_replacements`, which the game always draws and stops without: 1.3 GB, copied from the player's profile the first time
# only), then the player's prompt atlas (`prompts.bin`) copied in. scripts\uyactl.py keeps its profile the same way.
#
#   & .\scripts\fresh-profile.ps1 -Profile captures\levels\profile
param([Parameter(Mandatory = $true)][string]$Profile)
$ErrorActionPreference = 'Stop'
$player = Join-Path $env:USERPROFILE 'Games\Ratchet and Clank 3'
$packs = 'texture_replacements'
New-Item -ItemType Directory -Force $Profile | Out-Null
Get-ChildItem -LiteralPath $Profile -Force | Where-Object { $_.Name -ne $packs } | Remove-Item -Recurse -Force
Copy-Item -LiteralPath (Join-Path $player 'prompts.bin') -Destination $Profile
if (-not (Test-Path -LiteralPath (Join-Path $Profile $packs))) {
    Write-Host "fresh-profile: copying the texture packs into $Profile (once)"
    Copy-Item -LiteralPath (Join-Path $player $packs) -Destination $Profile -Recurse
}
