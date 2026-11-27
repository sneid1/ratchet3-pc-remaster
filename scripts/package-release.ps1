# Assembles the release of the remaster: the game folder zipped for RecompLauncher (the launcher picks the asset whose name
# carries the player's system and processor), and the updated textures as split 7z volumes, named without system words so the
# launcher never takes them for the game; a player downloads them from the release page and extracts them into the game's folder.
#
#   powershell -ExecutionPolicy Bypass -File scripts\package-release.ps1 [-Build DIR] [-Textures DIR] [-Out DIR] [-Version v0.1.0]
#
# -Build is a build of the port's `uya` package (scripts\build.ps1): uya.exe, the level libraries tr_l01.dll to tr_l36.dll and
# the DLSS/Streamline DLLs beside them. -Textures is the player profile's texture_replacements folder: the two community packs
# go under their own names, and the remaster's own pack (0-riftapart in the profile) is published as updated-textures, the name
# the launcher passes to the game as --rift.
param(
    [string]$Build = "$env:USERPROFILE\ratchet3-remaster\port\target\release",
    [string]$Textures = "$env:USERPROFILE\Games\Ratchet and Clank 3\texture_replacements",
    [string]$Out = "$env:USERPROFILE\ratchet3-pc-remaster\release",
    [string]$Version = 'v0.1.0',
    # The updated textures only when they changed: a build-only release reuses the last release's volumes.
    [switch]$NoTextures
)
$ErrorActionPreference = 'Stop'
$7z = 'C:\Program Files\7-Zip\7z.exe'
if (-not (Test-Path $7z)) { throw "package-release: no 7z at $7z" }
$repo = Split-Path -Parent $PSScriptRoot
$name = "ratchet3-pc-remaster-$Version"
$game = Join-Path $Out "$name-Windows-x64"
New-Item -ItemType Directory -Force $game | Out-Null

# The game, its level libraries and the DLSS/Streamline DLLs.
Copy-Item -Force (Join-Path $Build 'uya.exe') $game
# The game says its release in its update notice (port/uya/src/update.rs): a zip whose uya.exe says another would tell its players
# to update to the release they have, so port/uya/Cargo.toml's version must be this one. The pipe is what makes Windows
# PowerShell wait for the windowed uya.exe and hand it an output to print to (called plainly it gave it none, and read '').
$says = (& (Join-Path $game 'uya.exe') --version | Out-String).Trim()
if ($says -ne $Version) { throw "package-release: uya.exe says it is '$says', not ${Version}: set port/uya/Cargo.toml's version and build again" }
Copy-Item -Force (Join-Path $Build 'tr_l*.dll') $game
foreach ($f in 'NvLowLatencyVk.dll', 'nvngx_dlss.dll', 'nvngx_dlssg.dll', 'sl.common.dll', 'sl.dlss_g.dll',
    'sl.interposer.dll', 'sl.pcl.dll', 'sl.reflex.dll') {
    if (-not (Test-Path (Join-Path $Build $f))) { throw "package-release: no $f in $Build" }
    Copy-Item -Force (Join-Path $Build $f) $game
}
# The prompt atlas the game reads from the profile, and the launcher's file next to the program.
Copy-Item -Force (Join-Path (Split-Path -Parent $Textures) 'prompts.bin') $game
Copy-Item -Force (Join-Path $repo 'public\recomplauncher.json') $game
# The packs folder must exist for the game to start; the community packs arrive with the textures volumes.
New-Item -ItemType Directory -Force (Join-Path $game 'texture_replacements') | Out-Null
Set-Content -Encoding utf8 (Join-Path $game 'texture_replacements\README.txt') @'
The remaster's updated textures arrive here. Download the release's updated-textures volumes (the .7z parts), open the
first with 7-Zip, and extract into this game's folder: updated-textures goes beside the program (the launcher passes it
to the game as --rift), and the two community packs land in texture_replacements. Without them the game plays with the
original PlayStation 2 textures.
'@

# The Windows-x64 zip the launcher installs.
& $7z a -tzip '-mx=7' (Join-Path $Out "$name-Windows-x64.zip") (Join-Path $game '*') | Out-Null
if ($LASTEXITCODE) { throw "package-release: zipping the game failed" }

if ($NoTextures) { Get-ChildItem $Out -File | Select-Object Name, @{n='MB';e={ [int]($_.Length / 1MB) }}; exit 0 }

# The updated textures as store-mode 7z volumes of 1900 MB (GitHub's 2 GiB a file): a staging folder gives them the layout
# they extract to. PNG and DDS store no smaller, so the volumes are stored, not compressed.
$staging = Join-Path $Out 'textures-staging'
New-Item -ItemType Directory -Force (Join-Path $staging 'texture_replacements') | Out-Null
robocopy (Join-Path $Textures '0-riftapart') (Join-Path $staging 'updated-textures') /E /MT:12 /NFL /NDL /NJH /NP
robocopy (Join-Path $Textures 'reddyxd-uya') (Join-Path $staging 'texture_replacements\reddyxd-uya') /E /MT:12 /NFL /NDL /NJH /NP
robocopy (Join-Path $Textures 'texmaster-pal-gapfill') (Join-Path $staging 'texture_replacements\texmaster-pal-gapfill') /E /MT:12 /NFL /NDL /NJH /NP
if ($LASTEXITCODE -ge 8) { throw "package-release: staging the textures failed" }
& $7z a '-v1900m' '-mx=0' (Join-Path $Out "$name-updated-textures.7z") (Join-Path $staging '*') | Out-Null
if ($LASTEXITCODE) { throw "package-release: the textures volumes failed" }
Remove-Item -Recurse -Force $staging

Get-ChildItem $Out -File | Select-Object Name, @{n='MB';e={ [int]($_.Length / 1MB) }}
