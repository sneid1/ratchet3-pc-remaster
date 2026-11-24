"""Read Up Your Arsenal's programs out of the player's disc image.

    python scripts/uya_disc.py boot disc/SCUS_973.53.iso extracted/boot_ram.bin

The disc boots `SCUS_973.53`, a loader linked at 0x800000 whose .rodata holds the main program
(rac3-decomp's `boot_elf`) as a WAD-compressed image at file offset 0x7080. `boot` writes that image
decompressed, checked against the SHA-1 rac3-decomp records for it (config/us/programs.tsv), and laid out
as the loader leaves it in RAM from 0x100000 (its records placed at their addresses).
"""

import hashlib
import struct
import sys
from pathlib import Path

SECTOR = 0x800
BOOT_FILE = "SCUS_973.53"
PAYLOAD_OFFSET = 0x7080
PAYLOAD_SHA1 = "f0d7eba4c9500247be705a3055e0a19bfeb6f698"


def u32(b: bytes, o: int) -> int:
    return struct.unpack_from("<I", b, o)[0]


def iso_find(iso, path: str) -> tuple[int, int]:
    """(lba, size) of a file, walking the ISO 9660 directories."""
    iso.seek(16 * SECTOR)
    pvd = iso.read(SECTOR)
    if pvd[1:6] != b"CD001":
        raise ValueError("not an ISO 9660 image")
    lba, size = u32(pvd, 158), u32(pvd, 166)
    for part in path.upper().split("/"):
        iso.seek(lba * SECTOR)
        directory = iso.read(size)
        o, hit = 0, None
        while o < len(directory):
            ln = directory[o]
            if ln == 0:
                o = (o // SECTOR + 1) * SECTOR
                continue
            name = directory[o + 33:o + 33 + directory[o + 32]].decode("latin1").upper()
            if name.split(";")[0] == part:
                hit = (u32(directory, o + 2), u32(directory, o + 10))
                break
            o += ln
        if hit is None:
            raise FileNotFoundError(path)
        lba, size = hit
    return lba, size


def read_file(iso_path: Path, name: str) -> bytes:
    with open(iso_path, "rb") as iso:
        lba, size = iso_find(iso, name)
        iso.seek(lba * SECTOR)
        return iso.read(size)


def wad_decompress(data: bytes) -> bytes:
    """Insomniac's WAD LZ, as rac-rs's tools/recgen_iso.py decodes it."""
    if data[:3] != b"WAD":
        raise ValueError("no WAD header")
    end = min(u32(data, 3), len(data))
    out = bytearray()
    src, last = 16, 0xFF
    if src < end and data[src] > 0x11:
        n = data[src] - 0x11
        out += data[src + 1:src + 1 + n]
        src += 1 + n
        last = 0
    while src < end:
        flag = data[src]
        if flag < 0x10:
            if last < 0x10:
                raise ValueError("double literal")
            ln, st = (data[src + 1] + 18, src + 2) if flag == 0 else (flag + 3, src + 1)
            out += data[st:st + ln]
            src = st + ln
        else:
            if flag >= 0x40:
                b = data[src + 1]
                cl, dist, tail, nxt = (flag >> 5) + 1, b * 8 + ((flag >> 2) & 7) + 1, flag & 3, src + 2
            elif flag >= 0x20:
                cl, p = (data[src + 1] + 33, src + 2) if flag == 0x20 else ((flag & 0x1F) + 2, src + 1)
                an, b = data[p], data[p + 1]
                dist, tail, nxt = (an >> 2) + b * 0x40 + 1, an & 3, p + 2
            else:
                a, m3 = (flag >> 3) & 1, flag & 7
                cl, p = (data[src + 1] + 9, src + 2) if m3 == 0 else (m3 + 2, src + 1)
                bn, c = data[p], data[p + 1]
                b, tail, nxt = bn >> 2, bn & 3, p + 2
                if a == 0 and b == 0 and c == 0:
                    if m3 != 1:
                        s = nxt
                        while s < end and (s % 0x2000) != 0x10:
                            s += 1
                        src, last = s, flag
                        continue
                    cl = dist = 0
                else:
                    dist = 0x4000 + a * 0x4000 + b + c * 0x40
            if cl > 0:
                if dist > len(out):
                    raise ValueError("bad offset")
                frm = len(out) - dist
                for i in range(cl):
                    out.append(out[frm + i])
            out += data[nxt:nxt + tail]
            src = nxt + tail
        last = flag
    return bytes(out)


def records(payload: bytes) -> tuple[int, list[tuple[int, int, int, bytes]]]:
    """The payload's records: {address, size, kind, entry} then `size` bytes, for every kind (the .bss record, kind 8,
    carries its bytes too), as rac3-decomp's tools/levels.py `parse_records` reads them and as the loader copies them
    (SCUS_973.53 0x800488: unpacked to 0x8bc700, then each record copied to its address until the entry word changes)."""
    out, p, entry = [], 0, None
    while p + 16 <= len(payload):
        dest, n, kind, e = struct.unpack_from("<4I", payload, p)
        entry = e if entry is None else entry
        if e != entry:
            break
        out.append((dest, n, kind, payload[p + 16:p + 16 + n]))
        p += 16 + n
    return entry, out


def ram_image(payload: bytes, base: int = 0x100000) -> bytes:
    """The main program as the loader leaves it in RAM, from `base` to the end of its last record."""
    _, recs = records(payload)
    loaded = [(a, n, b) for a, n, k, b in recs if a >= base]
    end = max(a + n for a, n, _ in loaded)
    ram = bytearray(end - base)
    for a, n, b in loaded:
        ram[a - base:a - base + len(b)] = b
    return bytes(ram)


def boot_payload(iso_path: Path) -> bytes:
    loader = read_file(iso_path, BOOT_FILE)
    payload = wad_decompress(loader[PAYLOAD_OFFSET:])
    digest = hashlib.sha1(payload).hexdigest()
    if digest != PAYLOAD_SHA1:
        raise ValueError(f"boot payload SHA-1 {digest}, expected {PAYLOAD_SHA1}")
    return payload


def main() -> None:
    command, iso_path, out = sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3])
    if command != "boot":
        raise SystemExit(f"unknown command {command!r}")
    payload = boot_payload(iso_path)
    entry, recs = records(payload)
    for a, n, k, _ in recs:
        print(f"  record {a:#09x} size {n:#08x} kind {k:#x}")
    ram = ram_image(payload)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(ram)
    print(f"payload SHA-1 {PAYLOAD_SHA1} (matches rac3-decomp), entry {entry:#x}")
    print(f"{out}: RAM 0x100000..{0x100000 + len(ram):#x}")


if __name__ == "__main__":
    main()
