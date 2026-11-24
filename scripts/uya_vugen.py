"""Compile Up Your Arsenal's VU1 microprograms to native code, from the player's disc image.

    python scripts/uya_vugen.py disc/SCUS_973.53.iso

Cuts the 8 VU1 programs the game uploads (VIF1 MPG, each at micro address 0, in 0x800-byte pieces) out of the unpacked main
program (`uya_disc.ram_image`), checks each against its SHA-256, and runs rac-rs's tools/vu/vu1gen.py on them (profile:
tools/vu/profile_uya.txt, the run entries seen from New Game to Veldin). The output is
extern/rac-rs/rac-rs/src/engine/gsfull/vu1_uya<N>.rs and vu1aot_uya_gen.rs, selected by the engine's cargo feature uya-aot,
which the port enables. Nothing game-derived is committed (extern/ is gitignored); without these files the port does not build.
"""

import hashlib
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import uya_disc  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
RAC = ROOT / "extern" / "rac-rs"
GSFULL = RAC / "rac-rs" / "src" / "engine" / "gsfull"
BASE = 0x100000
PIECE = 0x800

# id -> (size in bytes, RAM address of each 0x800-byte piece of the program, SHA-256 of the assembled microcode).
# Programs 1, 5 and 7 are byte-identical to the first game's programs 5, 4 and 6.
PROGS = {
    0: (10680, [0x110798, 0x110FA0, 0x1117A8, 0x111FB0, 0x1127B8, 0x112FC0], "9a765a1e5e4793bbcaea412cbc7b3c780775cb0fd4801dc8bdca3633a6a633fc"),
    1: (14944, [0x103688, 0x103E90, 0x104698, 0x104EA0, 0x1056A8, 0x105EB0, 0x1066B8, 0x106EC0], "ef440f7dcb75a8ab0f82706100831db53811ec9e2d73c868a92f992571d9c794"),
    2: (9160, [0x108F38, 0x109740, 0x109F48, 0x10A750, 0x10AF58], "158cf4e04cdda98e2a69edbb0864835ed8ed54d8761f9decb383d73021d9b898"),
    3: (15920, [0x10B338, 0x10BB40, 0x10C348, 0x10CB50, 0x10D358, 0x10DB60, 0x10E368, 0x10EB70], "defc5fa9dd6fd27c9c9a535485f9ce79e902b039a4a870bd20c0c250af8f6fcb"),
    4: (2920, [0x101868, 0x102070], "459907a2895454f5a5cb0ecde4e22db53e0b95f7ef62935ad5b885b370ecdb13"),
    5: (4712, [0x1023F8, 0x102C00, 0x103408], "8f39178ec8d1c51c81e0681b7bdb6989b7c44997e14c6f7b76e452198bd425e2"),
    6: (1776, [0x101158], "f5b8b8052954d17e7092e8a9027920f9e96ed727c5bb252a012d7a51db60574f"),
    7: (7632, [0x107138, 0x107940, 0x108148, 0x108950], "7952469db1cc4a24f6d9f8901856583186bd617ec8511f1f7a94f94f6e6225e6"),
}


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    ram = uya_disc.ram_image(uya_disc.boot_payload(Path(sys.argv[1])))
    with tempfile.TemporaryDirectory() as bins:
        for n, (size, addrs, want) in PROGS.items():
            code = bytearray()
            for a in addrs:
                code += ram[a - BASE:a - BASE + min(PIECE, size - len(code))]
            got = hashlib.sha256(code).hexdigest()
            if len(code) != size or got != want:
                sys.exit(f"program {n}: microcode does not match the NTSC-U v1.00 main program (sha256 {got})")
            (Path(bins) / f"prog{n}.bin").write_bytes(code)
        subprocess.run(
            [sys.executable, str(RAC / "tools" / "vu" / "vu1gen.py"), "--progs", ",".join(map(str, PROGS)), "--bins", bins,
             "--prefix", "uya", "--profile", str(RAC / "tools" / "vu" / "profile_uya.txt"), "--out", str(GSFULL)],
            check=True,
        )


if __name__ == "__main__":
    main()
