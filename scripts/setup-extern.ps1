# Recreates extern/ (gitignored): rac-rs and rac3-decomp at their pinned commits, the project's rac-rs patches applied,
# and the Python environment for rac3-decomp's tools. Downloads: the two repositories from GitHub and the Python
# packages in rac3-decomp/tools/requirements.txt from PyPI.
#
#   powershell -ExecutionPolicy Bypass -File scripts\setup-extern.ps1
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$ext = Join-Path $root 'extern'
New-Item -ItemType Directory -Force $ext | Out-Null

$pins = @(
    @{ Name = 'rac-rs'; Url = 'https://github.com/AsobiCodes/rac-rs'; Commit = '4060c1aa776e1655ff83bce9cf043fd9a391b0da' },
    @{ Name = 'rac3-decomp'; Url = 'https://github.com/OpenRAC/rac3-decomp'; Commit = '409d1c3c3d86e32ecae286fb1d46161663f9c2da' }
)
foreach ($p in $pins) {
    $dir = Join-Path $ext $p.Name
    if (-not (Test-Path $dir)) {
        git init -q $dir
        git -C $dir remote add origin $p.Url
    }
    git -C $dir fetch -q --depth 1 origin $p.Commit
    git -C $dir checkout -q -B main $p.Commit
}

# The project's changes to rac-rs (patches/rac-rs, `git format-patch` from the uya branch), on a branch of their own.
$racrs = Join-Path $ext 'rac-rs'
git -C $racrs checkout -q -B uya main
Get-ChildItem (Join-Path $root 'patches\rac-rs') -Filter *.patch | Sort-Object Name | ForEach-Object {
    git -C $racrs -c user.name=setup -c user.email=setup@localhost am -q $_.FullName
}

$venv = Join-Path $ext 'venv'
if (-not (Test-Path $venv)) { python -m venv $venv }
& (Join-Path $venv 'Scripts\python.exe') -m pip install -q -r (Join-Path $ext 'rac3-decomp\tools\requirements.txt')
Write-Host "extern ready: rac-rs (uya branch), rac3-decomp, venv"
