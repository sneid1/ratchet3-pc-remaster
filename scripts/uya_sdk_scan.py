"""Find the PS2 SDK functions rac-rs's core hooks, in Up Your Arsenal's main program (NTSC-U boot_elf).

    python -I scripts/uya_sdk_scan.py extracted/boot_ram.bin extern/rac3-decomp/config/us/inventory.tsv

The RAM image (scripts/uya_disc.py boot) starts at 0x100000. Function starts come from rac3-decomp's NTSC-U inventory. Prints, per finding,
the function and the instructions that identify it.
"""
import struct
import sys

import capstone

BASE = 0x100000
payload = open(sys.argv[1], "rb").read()
funcs = []
for line in open(sys.argv[2], encoding="utf-8"):
    if line.startswith("#"):
        continue
    prog, sect, addr, size, kind = line.rstrip("\n").split("\t")
    # Only the functions inside the image (the engine core; the front end loads elsewhere).
    if prog == "boot_elf" and kind == "function" and int(addr, 16) + int(size) <= BASE + len(payload):
        funcs.append((int(addr, 16), int(size)))
funcs.sort()
md = capstone.Cs(capstone.CS_ARCH_MIPS, capstone.CS_MODE_MIPS64 + capstone.CS_MODE_LITTLE_ENDIAN)


def word(a):
    return struct.unpack_from("<I", payload, a - BASE)[0]


def insns(a, n):
    return list(md.disasm(payload[a - BASE:a - BASE + n], a))


syscalls = {}
for a, n in funcs:
    for k in range(a, a + n, 4):
        w = word(k)
        if w == 0x0000000C:  # syscall
            prev = [word(k - 4), word(k + 4)]
            for p in prev:
                if p >> 16 == 0x2403:  # addiu v1, zero, imm
                    num = p & 0xFFFF
                    num = num - 0x10000 if num & 0x8000 else num
                    syscalls.setdefault(num, []).append(a)
print("syscall wrappers (number: function):")
for num in sorted(syscalls):
    print(f"  {num:#x}: {', '.join(hex(f) for f in syscalls[num])}")

print("functions loading 0x8000_0009 (RPC bind) / 0x8000_000A (RPC call) / 0x8000_000C:")
for a, n in funcs:
    ins = insns(a, n)
    for i, x in enumerate(ins[:-1]):
        if x.mnemonic == "lui" and x.op_str.endswith("0x8000"):
            for y in ins[i + 1:i + 6]:
                if y.mnemonic == "ori" and y.op_str.split(", ")[-1] in ("9", "0xa", "0xc", "0x80000009", "0x8000000a"):
                    print(f"  {a:#x} (size {n}): {x.address:#x} {x.mnemonic} {x.op_str} / {y.address:#x} {y.mnemonic} {y.op_str}")

print("functions touching GS CSR (lui 0x1200) with an andi 8 poll:")
for a, n in funcs:
    ins = insns(a, n)
    if any(x.mnemonic == "lui" and x.op_str.endswith(", 0x1200") for x in ins):
        polls = [x for x in ins if x.mnemonic == "andi" and x.op_str.endswith(", 8")]
        branches = [x for x in ins if x.mnemonic.startswith("b") and int(x.op_str.split(", ")[-1], 16) <= x.address]
        print(f"  {a:#x} (size {n}): andi-8 at {[hex(x.address) for x in polls]}, back branches {[hex(x.address) + '->' + x.op_str.split(', ')[-1] for x in branches]}")
