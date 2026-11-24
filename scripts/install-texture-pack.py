"""Installs the selected files of a downloaded texture pack into the game's texture_replacements folder.

Usage:
  python -I scripts/install-texture-pack.py <pack.zip | extracted pack folder> <selection.txt> <texture_replacements dir>

The selection file (textures/*.txt) lists pack paths, one per line (# starts a comment): a folder
prefix installs everything below it, keeping its folders; a line naming one file installs that file
directly in the target. Paths are relative to the pack's top folder for a zip and to the folder
itself for an extracted pack (a .rar or .7z is extracted first, into a new empty folder, with
7-Zip). Everything goes under <dir>/<selection file stem>/, which is emptied first, so re-running
with a changed selection replaces the previous install, and each selection can be removed on its
own.

The port loads every png or dds file whose name is a PCSX2 replacement key (scripts/uya_texnames.py);
a selected file of another type or with a name that is no key stops the install, as does a
selected line that matches nothing. Two installed files with the same key are reported: the port
keeps the first in sorted path order.
"""
import importlib.util
import os
import shutil
import sys
import zipfile
from pathlib import Path, PurePosixPath

_spec = importlib.util.spec_from_file_location('uya_texnames', Path(__file__).with_name('uya_texnames.py'))
names = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(names)


def zip_files(pack: Path):
    """(path below the pack's top folder, opener, size) for every file in a zipped pack."""
    archive = zipfile.ZipFile(pack)
    for info in archive.infolist():
        if info.is_dir():
            continue
        path = PurePosixPath(info.filename)
        if path.is_absolute() or ".." in path.parts or len(path.parts) < 2:
            sys.exit(f"unsafe path in pack: {info.filename}")
        yield PurePosixPath(*path.parts[1:]), (lambda i=info: archive.open(i)), info.file_size


def long_path(path: Path) -> Path:
    """The path in a form Windows reads past 260 characters (pack folders can nest deep)."""
    return Path("\\\\?\\" + str(path.resolve())) if os.name == "nt" else path


def folder_files(pack: Path):
    """(path below the folder, opener, size) for every file in an extracted pack."""
    root = long_path(pack)
    for path in sorted(root.rglob("*")):
        if path.is_file():
            yield PurePosixPath(path.relative_to(root).as_posix()), (lambda p=path: open(p, "rb")), path.stat().st_size


def main(pack: str, selection: str, target: str) -> None:
    lines = [
        line.strip().replace("\\", "/") for line in Path(selection).read_text(encoding="utf-8").splitlines()
        if line.strip() and not line.strip().startswith("#")
    ]
    if not lines:
        sys.exit(f"{selection}: nothing selected")
    source = Path(pack)
    if source.is_file() and source.suffix.lower() != ".zip":
        sys.exit(f"{pack}: extract a {source.suffix} pack into a new empty folder first (7-Zip), then install from that folder")

    out = Path(target) / Path(selection).stem
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)

    files_in_pack = folder_files(source) if source.is_dir() else zip_files(source)
    used = {s: 0 for s in lines}
    keys = {}
    duplicates = []
    files = size = 0
    for inner, opener, file_size in files_in_pack:
        line = next((s for s in lines if str(inner) == s or str(inner).startswith(s.rstrip("/") + "/")), None)
        if line is None:
            continue
        if inner.suffix.lower() not in (".png", ".dds"):
            sys.exit(f"not a texture file, in a selected line: {inner}")
        key = names.parse(inner.name)
        if key is None:
            sys.exit(f"not a PCSX2 replacement name (the port would ignore it): {inner}")
        if key in keys:
            duplicates.append(f"{inner} (same key as {keys[key]})")
        else:
            keys[key] = inner
        dest = out / inner.name if str(inner) == line else out.joinpath(*inner.parts)
        dest.parent.mkdir(parents=True, exist_ok=True)
        with opener() as src, open(dest, "wb") as dst:
            shutil.copyfileobj(src, dst, 1 << 20)
        used[line] += 1
        files += 1
        size += file_size

    missing = [s for s, n in used.items() if n == 0]
    if missing:
        sys.exit(f"selected lines that match nothing in the pack: {missing[:20]}{' ...' if len(missing) > 20 else ''}")
    for d in duplicates[:20]:
        print(f"duplicate key: {d}")
    if len(lines) <= 60:
        for s, n in used.items():
            print(f"{n:6d}  {s}")
    print(f"installed {files} textures ({len(keys)} keys), {size / 1e9:.2f} GB, into {out}")


if __name__ == "__main__":
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2], sys.argv[3])
