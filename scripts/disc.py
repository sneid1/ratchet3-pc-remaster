"""Put the player's PS2 disc image into the project, from the drive or from an existing image.

    python scripts/disc.py dump D: disc/SCUS-97353.iso      read the disc in drive D: sector by sector
    python scripts/disc.py import <image.iso> disc/<name>.iso  copy an image the player already made

PS2 DVDs are plain 2048-byte-sector discs with no copy protection, so the image is the
volume's sectors from 0 to the end of the ISO 9660 volume, whose size comes from the primary
volume descriptor (sector 16). Both commands print the image's size, CRC32, MD5 and SHA-1, to
compare with the redump.org entry for the disc, and the disc's SYSTEM.CNF.
"""

import hashlib
import re
import sys
import zlib
from pathlib import Path

SECTOR = 2048
CHUNK_SECTORS = 512  # 1 MiB per read


def volume_sectors(src) -> int:
    src.seek(16 * SECTOR)
    pvd = src.read(SECTOR)
    if pvd[0] != 1 or pvd[1:6] != b"CD001":
        raise SystemExit("sector 16 is not an ISO 9660 primary volume descriptor")
    total = int.from_bytes(pvd[80:84], "little")
    print(f"volume {pvd[40:72].decode('ascii').strip()!r}: {total} sectors ({total * SECTOR / 2**30:.2f} GiB)")
    return total


def copy_sectors(src, total: int, out_path: Path) -> None:
    out_path.parent.mkdir(parents=True, exist_ok=True)
    crc, md5, sha1 = 0, hashlib.md5(), hashlib.sha1()
    src.seek(0)
    done = 0
    with open(out_path, "xb") as out:
        while done < total:
            n = min(CHUNK_SECTORS, total - done)
            data = src.read(n * SECTOR)
            if len(data) != n * SECTOR:
                raise SystemExit(f"short read at sector {done}: got {len(data)} bytes, wanted {n * SECTOR}")
            out.write(data)
            crc = zlib.crc32(data, crc)
            md5.update(data)
            sha1.update(data)
            done += n
            if done % (CHUNK_SECTORS * 512) == 0 or done == total:
                print(f"  {done}/{total} sectors ({100 * done / total:.0f}%)", flush=True)
    print(f"image  {out_path}")
    print(f"size   {total * SECTOR}")
    print(f"crc32  {crc:08x}")
    print(f"md5    {md5.hexdigest()}")
    print(f"sha1   {sha1.hexdigest()}")


def print_system_cnf(iso: Path) -> None:
    """Find SYSTEM.CNF in the root directory and print it."""
    with open(iso, "rb") as f:
        f.seek(16 * SECTOR)
        root = f.read(SECTOR)[156:190]
        f.seek(int.from_bytes(root[2:6], "little") * SECTOR)
        directory = f.read(int.from_bytes(root[10:14], "little"))
        pos = 0
        while pos < len(directory):
            length = directory[pos]
            if length == 0:
                pos = (pos // SECTOR + 1) * SECTOR
                continue
            rec = directory[pos:pos + length]
            if rec[33:33 + rec[32]].decode("ascii", "replace").upper().startswith("SYSTEM.CNF"):
                f.seek(int.from_bytes(rec[2:6], "little") * SECTOR)
                cnf = f.read(int.from_bytes(rec[10:14], "little")).decode("ascii", "replace")
                print("SYSTEM.CNF:\n  " + cnf.strip().replace("\r", "").replace("\n", "\n  "))
                return
            pos += length
    raise SystemExit("SYSTEM.CNF not found in the root directory")


def main() -> None:
    command, source, out_path = sys.argv[1], sys.argv[2], Path(sys.argv[3])
    if command == "dump":
        drive = source.rstrip("\\/")
        if not re.fullmatch(r"[A-Za-z]:", drive):
            raise SystemExit(f"drive must look like D:, got {drive!r}")
        path = rf"\\.\{drive}"
    elif command == "import":
        path = source
    else:
        raise SystemExit(f"unknown command {command!r}: use dump or import")

    with open(path, "rb", buffering=0) as src:
        total = volume_sectors(src)
        if command == "import":
            size = Path(source).stat().st_size
            if size != total * SECTOR:
                raise SystemExit(f"image is {size} bytes but its volume is {total * SECTOR}: not a plain dump")
        copy_sectors(src, total, out_path)
    print_system_cnf(out_path)


if __name__ == "__main__":
    main()
