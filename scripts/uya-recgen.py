"""Writes the recompiled EE code of Up Your Arsenal from the player's disc: generated/uya/ (gitignored), which port/uya depends on.

    python scripts/uya-recgen.py disc/SCUS_973.53.iso [--levels 1,2,...] [--chunk N]

Runs rac-rs's tools/recgen_iso.py (extern/rac-rs, the uya branch) once for the resident engine core (boot), the two front ends (the
boot program's own, pseudo-level 100, and frontbin, 101) and each level asked for (default: 1, Veldin), over the function bounds
scripts/uya_bounds.py writes from rac3-decomp. Any level 1..55 of the disc can be named. The generated crates are Rust source made
from the disc's code bytes, so they stay on this PC (generated/ is gitignored); `cargo build` in port/ fails without them.
"""

import argparse
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FLAGS = ["--no-comments", "--compact", "--shared-slow", "--outline-mem", "--direct-mem", "--no-win-mem", "--no-holes"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("iso")
    ap.add_argument("--levels", default="1")
    ap.add_argument("--chunk", type=int, default=12000, help="instructions per crate (smaller crates build in less memory)")
    a = ap.parse_args()
    out = ROOT / "generated" / "uya"
    bounds = out / "fn_bounds_rc3.tsv"
    run = [sys.executable, "-I"]
    subprocess.run(run + [str(ROOT / "scripts" / "uya_bounds.py"), str(ROOT / "extern" / "rac3-decomp"), str(bounds)], check=True)
    gen = run + [str(ROOT / "extern" / "rac-rs" / "tools" / "recgen_iso.py"), "--iso", a.iso, "--bounds", str(bounds), "--out", str(out),
                 "--oracle", str(ROOT / "extern" / "rac-rs" / "oracle"), "--chunk", str(a.chunk)] + FLAGS
    # 0: the core alone; 100 / 101: the front ends; the rest: levels (their crates leave the core's alone)
    for lvl in [0, 100, 101] + [int(x) for x in a.levels.split(",") if x]:
        subprocess.run(gen + ["--level", str(lvl)] + (["--keep-boot"] if lvl else []), check=True)


if __name__ == "__main__":
    main()
