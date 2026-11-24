"""Function bounds of Up Your Arsenal (NTSC-U) for rac-rs's recompiler, from rac3-decomp's configuration.

    python scripts/uya_bounds.py extern/rac3-decomp generated/uya/fn_bounds_rc3.tsv

Writes `entry end level` lines (hex addresses of a function [entry, end); addresses only, no decompiled code), the format of
rac-rs's tools/fn_bounds.tsv, for:
  boot  the resident engine core: boot_elf's core.text (config/us/inventory.tsv)
  100   the front end inside boot_elf (its `.text` at 0x381180, which runs from the first frame until the first level loads)
  101   frontbin, the front end the game reloads after Quit Game (its `.text` at 0x37D100)
  N     level N's `.text` (config/us/levels/functions.tsv, one row per function with the place in every level that has it)
Remnants (the 4-byte leftovers between functions) are left out; the recompiler's gap scan finds any code they hold.
"""

import sys
from pathlib import Path

FRONT_A, FRONT_B = 100, 101


def inventory(cfg: Path):
    boot, front_a, front_b = [], [], []
    for line in (cfg / "us" / "inventory.tsv").read_text().splitlines():
        if line.startswith("#") or not line.strip():
            continue
        program, section, addr, size, kind = line.split("\t")
        if kind != "function":
            continue
        a, n = int(addr, 16), int(size)
        if program == "boot_elf" and section == "core.text":
            boot.append((a, a + n))
        elif program == "boot_elf" and section == ".text":
            front_a.append((a, a + n))
        elif program == "frontbin" and section == ".text":
            front_b.append((a, a + n))
    return boot, front_a, front_b


def levels(cfg: Path):
    out: dict[int, list[tuple[int, int]]] = {}
    for line in (cfg / "us" / "levels" / "functions.tsv").read_text().splitlines():
        if line.startswith("#") or not line.strip():
            continue
        _name, _kind, size, _fp, remnant, _n, places = line.split("\t")
        if remnant == "1":
            continue
        for place in places.split(","):
            lvl, addr = place.split(":")
            a = int(addr, 16)
            out.setdefault(int(lvl), []).append((a, a + int(size)))
    return out


def check(fns, what):
    fns.sort()
    for (a0, e0), (a1, _) in zip(fns, fns[1:]):
        if a1 < e0:
            raise SystemExit(f"{what}: functions {a0:#x}..{e0:#x} and {a1:#x} overlap")


def main():
    cfg = Path(sys.argv[1]) / "config"
    boot, front_a, front_b = inventory(cfg)
    lv = levels(cfg)
    rows = [("boot", boot), (str(FRONT_A), front_a), (str(FRONT_B), front_b)] + [(str(k), v) for k, v in sorted(lv.items())]
    out = ["# entry\tend\tlevel   (hex; boot = core.text, 100 = boot_elf's front end, 101 = frontbin, N = level N's .text; from rac3-decomp config/us)"]
    for tag, fns in rows:
        check(fns, tag)
        out += [f"{a:x}\t{e:x}\t{tag}" for a, e in fns]
    path = Path(sys.argv[2])
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(out) + "\n")
    print(f"{path}: boot {len(boot)}, front end {len(front_a)}, frontbin {len(front_b)}, {len(lv)} levels ({len(lv.get(1, []))} in level 1)")


if __name__ == "__main__":
    main()
