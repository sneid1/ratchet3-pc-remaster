"""A level's continue points (the checkpoints the game respawns Ratchet at), read from a RAM dump of the running level, and the
uyactl commands that put Ratchet at one of them in a running game with RAM writes and `run` only.

    python scripts/checkpoints.py RAM.bin LEVEL            list the continue points
    python scripts/checkpoints.py RAM.bin LEVEL --go K     print the uyactl command string that respawns Ratchet at point K

RAM.bin is a `dump DIR` of the running level (uyactl: `dump captures/x/d; run 5`) once the level plays; K counts from 1 in the
order the level chains its checkpoints, K 0 is the respawn point the level start made (not every level makes one). Levels whose
checkpoint code is the shared checkpoint_reached (rac3-decomp config/us/levels/functions.tsv: levels 1, 4, 9, 12, 16, 18, 20,
22, 26, 29) are handled, and levels 6, 14, 17, 19 and 24 (OWN_CODE below, last section of this text). Metropolis example (the level's intro is skipped first, then K 1):
    until level 16; run 600; press 800 3; run 100; press 40 3; run 600; <the --go string>

How the game does it (rac3-decomp src/levels/shared/L01_003241A0.c checkpoint_reached; Metropolis's addresses, each level's own are
worked out here from the same shared functions in the catalogue and from their code):
- The level has one class-6625 moby (the controller; its update function calls checkpoint_reached) whose pvars +4 hold the first of
  its checkpoints, class-6626 mobys chained by pvars +0x6C (next); a checkpoint's pvars: +0x00 id, +0x28 the spawn point's index
  among the level's 0x80-byte volume records (the table's pointer: Metropolis 0x1E0240; a record's +0x30 is the position quad, +0x70
  the rotation quad), +0x30 reached, +0x53 gate. The controller's +0xB0 byte is the area the hero takes.
- checkpoint_reached ends in save_respawn(spawn + 0x30, spawn + 0x70) (func_L01_004A3610, Metropolis 0x4B0C58): it fills the
  snapshot, 0xC70 bytes at 0x236E40 (+0 valid = 1, +0x10 position, +0x20 rotation, +0x30 to +0x60 hero state, +0x60 on a copy of the
  level state at 0x2361D0 + 0x60), and the level's death handling (func_L01_004A3848, 0x4B0E90) copies it back when the hero dies
  (health 0x1A7430 below 1): position to 0x1A4C60, health full, camera behind him. A level with no snapshot at the dump (first word
  0) restores nothing, so the whole image is written, not only the position: save_respawn's own code is run over the dump
  (snapshot_image; for Metropolis it reproduces the game's own snapshot byte for byte).
- So point K is: write the snapshot image for its spawn record, the controller's spawn index (pvars +0x28), the area (0x1A71D7) and
  health 0, then `run` about 400 fields: the death, the respawn and the hero standing at the point. Hubs with a mission menu
  (Aridia 18, Mylon 22) open it on a Cross press and Start: do not press them there.

Levels 6, 14, 17, 19 and 24 have no checkpoint_reached. Every caller of save_respawn in their overlays (a jal scan of
captures/ghidra/overlays/LNN.bin, decompiled with captures/ghidra Decomp.java; the functions are named by their level addresses):
- the level init (6 0x49BBD0, 14 0x4A2A90, 17 0x491480, 19 0x4AC608, 24 0x49CFE8) calls save_respawn(0x1A4C60, 0x1A4C70), the hero's
  own position and rotation, when it places him: K 0, the level start (the dump's snapshot).
- the save pad, a class-805 moby (14 0x3030E0, 17 0x309008, 19 0x3047B8, 24 0x2FF160; 6 has none): once the hero stands in its
  volume (pvars +0 is the volume index) it calls save_respawn(pad + 0x10, pad + 0xF0) with the position's z put on the ground
  (within 2 units of the pad's own z, else the pad's z), or, in level 24, copies the same snapshot inline. So K is the pad's moby
  position and rotation, in moby array order.
- the objective, a class-3950 moby (6 0x321B50, 14 0x32BA88, 17 0x32EA18, 19 0x336358): when it completes (state 4) and its pvars
  byte +0xB is above 0 and the high half of pvars +0xC is not 0, it calls save_respawn(volume record pvars +4 + 0x30/0x70), or, with
  pvars +4 = -1, save_respawn(0x1A4C60, 0x1A4C70): the hero's position then, which no dump holds (listed, not reachable). Only
  level 17's second objective qualifies, with -1. (Its state 3 call happens in planet 0 only.)
- level 19's class-6492 moby (0x3A7C20): in state 2 it puts the hero at the volume record of pvars +0x124 and calls
  save_respawn(0x1A4C60, 0x1A4C70): K is that volume record. Its other call site, 0x3ABAB0, is a touch test that saves the hero's
  position where he touches a gate moby: none in the dump (no moby has it as an update function).
A level with only K 0 has no other continue point.
"""

import argparse
import os
import struct
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FUNCTIONS = os.path.join(ROOT, 'extern', 'rac3-decomp', 'config', 'us', 'levels', 'functions.tsv')
HERO_POS = 0x1A4C60
HERO_HEALTH = 0x1A7430
HERO_AREA = 0x1A71D7
CHECKPOINT_CLASS = 6626
CONTROLLER_CLASS = 6625
# the code of levels without checkpoint_reached (functions found as callers of save_respawn, see the module text): the save pad and
# objective update functions, the volume table pointer (a global), the class-6492 event moby (level 19)
OWN_CODE = {
    6: dict(objective=0x321B50, volumes=0x1DEE80),
    14: dict(pad=0x3030E0, objective=0x32BA88, volumes=0x1DF800),
    17: dict(pad=0x309008, objective=0x32EA18, volumes=0x1DF180),
    19: dict(pad=0x3047B8, objective=0x336358, event=0x3A7C20, volumes=0x1DF800),
    24: dict(pad=0x2FF160),
}


def level_addresses(level, names):
    """{name: (address in this level, size)} from the level catalogue (names are the L01 spellings of the Veldin addresses)."""
    want = {n: None for n in names}
    with open(FUNCTIONS) as f:
        for line in f:
            p = line.rstrip('\n').split('\t')
            if len(p) >= 7 and p[0] in want:
                for place in p[6].split(','):
                    lv, addr = place.split(':')
                    if int(lv) == level:
                        want[p[0]] = (int(addr, 16), int(p[2]))
    for n, v in want.items():
        if v is None:
            raise SystemExit(f'{n} is not in level {level} of {FUNCTIONS}')
    return want


class Ram:
    def __init__(self, path):
        self.b = open(path, 'rb').read()

    def u32(self, a):
        return struct.unpack_from('<I', self.b, a)[0]

    def s16(self, a):
        return struct.unpack_from('<h', self.b, a)[0]

    def i32(self, a):
        return struct.unpack_from('<i', self.b, a)[0]

    def f32(self, a):
        return struct.unpack_from('<f', self.b, a)[0]


def absolute_accesses(ram, start, size):
    """The memory ops in [start, start+size) whose address is a lui constant plus an offset: [(pc, op, effective address)]. Only
    lui/addiu/ori/move chains are followed, straight through the code (branches do not reset them)."""
    regs = {}
    out = []
    for pc in range(start, start + size, 4):
        w = ram.u32(pc)
        op, rs, rt, imm = w >> 26, (w >> 21) & 31, (w >> 16) & 31, w & 0xFFFF
        simm = imm - 0x10000 if imm & 0x8000 else imm
        if op == 0x0F:
            regs[rt] = imm << 16
        elif op in (0x09, 0x0D) and rs in regs:
            regs[rt] = (regs[rs] + simm) & 0xFFFFFFFF if op == 0x09 else regs[rs] | imm
        elif op in (0x20, 0x24, 0x21, 0x25, 0x23, 0x28, 0x29, 0x2B, 0x31, 0x39) and rs in regs:
            out.append((pc, op, (regs[rs] + simm) & 0xFFFFFFFF))
            if op in (0x20, 0x24, 0x21, 0x25, 0x23, 0x31):
                regs.pop(rt, None)
        elif op == 0 and (w & 0x3F) in (0x25, 0x2D, 0x21) and (w >> 16) & 31 == 0 and ((w >> 21) & 31) in regs:
            regs[(w >> 11) & 31] = regs[(w >> 21) & 31]
        else:
            if op in (0x09, 0x0D, 0x0F, 0x23, 0x24) or (op == 0 and (w & 0x3F) in (0x21, 0x25, 0x2D, 0x00, 0x2A, 0x2B)):
                dest = rt if op else (w >> 11) & 31
                if op != 0x0F:
                    regs.pop(dest, None)
    return out


def callers(ram, target):
    w = 0x0C000000 | (target >> 2) & 0x3FFFFFF
    return [o for o in range(0x100000, 0x800000, 4) if ram.u32(o) == w]


def function_start(ram, pc):
    while pc > 0x100000:
        w = ram.u32(pc)
        if w >> 16 == 0x27BD and w & 0x8000:
            return pc
        pc -= 4
    raise SystemExit('no function start')


def own_points(ram, level):
    code = OWN_CODE[level]
    a = level_addresses(level, ['func_L01_0049FDB0'])
    save, save_size = a['func_L01_0049FDB0']
    snap = next(ea for pc, op, ea in absolute_accesses(ram, save, save_size) if op == 0x2B)
    g = dict(save=save, snap=snap, mobys=None, reached=None, controller_fn=None, spawn_ptr=None, ctrl_pv28=None)
    fns = {v for k, v in code.items() if k != 'volumes'}
    first = next((m for m in range(0x1000000, len(ram.b) - 0x100, 4) if ram.u32(m + 0x64) in fns and ram.s16(m + 0xAA) in (805, 3950, 6492)), None)
    if first is None:
        raise SystemExit('no moby with a save pad, objective or event update function in this dump')
    # the moby array: the global pair (start, end) of 0x100-byte records that holds it
    for at in range(0x100000, 0x200000, 4):
        s, e = ram.u32(at), ram.u32(at + 4)
        if s <= first < e and (first - s) % 256 == 0 and (e - s) % 256 == 0 and 50 < (e - s) // 256 < 4096 and s % 16 == 0:
            g['mobys'] = (s, e)
            break
    if g['mobys'] is None:
        raise SystemExit('the moby array was not found')
    s, e = g['mobys']
    volumes = ram.u32(code['volumes']) if 'volumes' in code else None
    rec = lambda i: (ram.b[volumes + i * 0x80 + 0x30:volumes + i * 0x80 + 0x40], ram.b[volumes + i * 0x80 + 0x70:volumes + i * 0x80 + 0x80])
    area = ram.b[HERO_AREA]
    valid = ram.u32(snap) == 1
    qf = lambda raw: [struct.unpack_from('<f', raw, 4 * i)[0] for i in range(3)]
    pts = [dict(src='level start (init: the hero)', id=None, area=area, spawn=None, moby=None, pos=qf(ram.b[snap + 0x10:snap + 0x20]),
                rot=qf(ram.b[snap + 0x20:snap + 0x30]), pos_raw=ram.b[snap + 0x10:snap + 0x20] if valid else None,
                rot_raw=ram.b[snap + 0x20:snap + 0x30] if valid else None)]
    notes = []
    def add(src, mid, pos_raw, rot_raw, moby=None):
        pts.append(dict(src=src, id=mid, area=area, spawn=None, moby=moby, pos=qf(pos_raw), rot=qf(rot_raw), pos_raw=pos_raw, rot_raw=rot_raw))
    for m in range(s, e, 256):
        fn = ram.u32(m + 0x64)
        pv = ram.u32(m + 0x68)
        if fn == code.get('pad'):
            add('save pad', ram.u32(pv), ram.b[m + 0x10:m + 0x20], ram.b[m + 0xF0:m + 0x100], m)
        elif fn == code.get('objective'):
            if ram.b[pv + 0xB] > 0 and ram.u32(pv + 0xC) >> 16:
                if ram.i32(pv + 4) == -1:
                    notes.append(f'objective {m:#x} (id {ram.u32(pv)}, moby at ({ram.f32(m + 0x10):.1f}, {ram.f32(m + 0x14):.1f}, '
                                 f'{ram.f32(m + 0x18):.1f})) saves the hero position where it completes: not reachable from a dump')
                else:
                    add('objective volume', ram.u32(pv), *rec(ram.i32(pv + 4)), m)
        elif fn == code.get('event'):
            if ram.i32(pv + 0x124) >= 0:
                add('event volume', ram.u32(pv), *rec(ram.i32(pv + 0x124)), m)
    if not valid:
        notes.append(f'the dump holds no respawn snapshot (word at {snap:#x} is not 1): the level init saved none, and with no pad, '
                     'qualifying objective or event moby this level has no continue point besides its start')
    g['notes'] = notes
    return g, pts


def find(ram, level):
    a = level_addresses(level, ['func_L01_00325A60', 'func_L01_0049FDB0'])
    reached, reached_size = a['func_L01_00325A60']
    save, save_size = a['func_L01_0049FDB0']
    calls = callers(ram, reached)
    if len(calls) != 1:
        raise SystemExit(f'checkpoint_reached ({reached:#x}) has {len(calls)} callers in this dump, expected 1')
    controller_fn = function_start(ram, calls[0])
    # the volume table pointer: the absolute lw within 10 instructions before the `sll rd, rt, 7` that scales the spawn index
    acc = absolute_accesses(ram, reached, reached_size)
    spawn_ptr = None
    for pc in range(reached, reached + reached_size, 4):
        w = ram.u32(pc)
        if w & 0xFFE0003F == 0x00000000 and (w >> 6) & 31 == 7 and w:
            for p2, op, ea in acc:
                if op == 0x23 and pc - 40 <= p2 < pc:
                    spawn_ptr = ea
    if spawn_ptr is None:
        raise SystemExit('the spawn table pointer was not found in checkpoint_reached')
    # the snapshot: the first store through a lui constant in save_respawn (its valid word)
    snap = next(ea for pc, op, ea in absolute_accesses(ram, save, save_size) if op == 0x2B)
    # the moby array: the absolute lw in the controller's code that feeds `sll rd, rt, 8` (index * 0x100)
    mobys = None
    for cand in sorted({ea for pc, op, ea in absolute_accesses(ram, controller_fn, 0x1000) if op == 0x23}):
        s, e = ram.u32(cand), ram.u32(cand + 4)
        if 0x100000 < s < e < 0x2000000 and (e - s) % 256 == 0 and 64 < (e - s) // 256 < 4096:
            if any(ram.u32(m + 0x64) == controller_fn for m in range(s, e, 256)):
                mobys = (s, e)
                break
    if mobys is None:
        raise SystemExit('the moby array was not found')
    return dict(reached=reached, save=save, controller_fn=controller_fn, spawn_ptr=spawn_ptr, snap=snap, mobys=mobys)


def points(ram, level):
    if level in OWN_CODE:
        return own_points(ram, level)
    g = find(ram, level)
    s, e = g['mobys']
    ctrl = [m for m in range(s, e, 256) if ram.u32(m + 0x64) == g['controller_fn']]
    if len(ctrl) != 1:
        raise SystemExit(f'{len(ctrl)} controller mobys, expected 1')
    c = ctrl[0]
    area = ram.b[c + 0xB0]
    table = ram.u32(g['spawn_ptr'])
    pv = ram.u32(c + 0x68)
    m = ram.u32(pv + 4)
    out = []
    seen = set()
    while m and m not in seen:
        seen.add(m)
        if ram.s16(m + 0xAA) != CHECKPOINT_CLASS:
            raise SystemExit(f'moby {m:#x} in the checkpoint chain is class {ram.s16(m + 0xAA)}')
        p = ram.u32(m + 0x68)
        spawn = ram.u32(p + 0x28)
        rec = table + spawn * 0x80
        out.append(dict(id=ram.u32(p), area=area, spawn=spawn, moby=m,
                        pos=[ram.f32(rec + 0x30 + 4 * i) for i in range(3)], rot=[ram.f32(rec + 0x70 + 4 * i) for i in range(3)],
                        pos_raw=ram.b[rec + 0x30:rec + 0x40], rot_raw=ram.b[rec + 0x70:rec + 0x80]))
        m = ram.u32(p + 0x6C)
    valid = ram.u32(g['snap']) == 1
    start = dict(id=None, area=ram.b[HERO_AREA], spawn=None, moby=None,
                 pos=[ram.f32(g['snap'] + 0x10 + 4 * i) for i in range(3)], rot=[ram.f32(g['snap'] + 0x20 + 4 * i) for i in range(3)],
                 pos_raw=ram.b[g['snap'] + 0x10:g['snap'] + 0x20] if valid else None,
                 rot_raw=ram.b[g['snap'] + 0x20:g['snap'] + 0x30] if valid else None)
    g['ctrl_pv28'] = ram.u32(c + 0x68) + 0x28
    g['notes'] = []
    return g, [start] + out


def snapshot_image(ram, g, pos_raw, rot_raw):
    """The snapshot (0xC70 bytes at g['snap']) the game's save_respawn would store for this position and rotation, from the dump's
    live state: save_respawn's own code run over a copy of the dump (a small MIPS interpreter for the instructions it uses; its one
    call before the last is the memcpy of the level state)."""
    m = bytearray(ram.b)
    R = [0] * 32
    pos_at, rot_at, R[29] = 0x1FF0000, 0x1FF0010, 0x1FFE000
    m[pos_at:pos_at + 16] = pos_raw
    m[rot_at:rot_at + 16] = rot_raw
    R[4], R[5] = pos_at, rot_at
    M = 0xFFFFFFFF
    u = lambda a, n: int.from_bytes(m[a & M:(a & M) + n], 'little')
    def st(a, v, n):
        m[a & M:(a & M) + n] = (v & ((1 << (8 * n)) - 1)).to_bytes(n, 'little')
    sx = lambda v, b: v - (1 << b) if v & (1 << (b - 1)) else v
    pc, calls, delay = g['save'], 0, None
    def step(pc):
        """Run the instruction at pc; return a branch target, 'call' or None."""
        w = ram_word(m, pc)
        op, rs, rt, rd, sh, fn, imm = w >> 26, (w >> 21) & 31, (w >> 16) & 31, (w >> 11) & 31, (w >> 6) & 31, w & 63, w & 0xFFFF
        si = sx(imm, 16)
        a = (R[rs] + si) & M
        if op == 0:
            if fn == 0: R[rd] = (R[rt] << sh) & M
            elif fn == 2: R[rd] = (R[rt] & M) >> sh
            elif fn == 3: R[rd] = sx(R[rt] & M, 32) >> sh & M
            elif fn == 8: return 'ret'
            elif fn in (0x21, 0x2D, 0x25): R[rd] = (R[rs] + R[rt]) & M if fn != 0x25 else R[rs] | R[rt]
            else: raise SystemExit(f'save_respawn: special {fn:#x} at {pc:#x} is not handled')
        elif op == 3: return 'call'
        elif op in (4, 5, 0x14, 0x15):
            taken = (R[rs] & M) == (R[rt] & M) if op in (4, 0x14) else (R[rs] & M) != (R[rt] & M)
            return ('branch', pc + 4 + si * 4 if taken else None, op in (0x14, 0x15))
        elif op in (9, 0x19): R[rt] = (R[rs] + si) & M
        elif op == 0xA: R[rt] = int(sx(R[rs] & M, 32) < si)
        elif op == 0xB: R[rt] = int((R[rs] & M) < (si & M))
        elif op == 0xC: R[rt] = R[rs] & imm
        elif op == 0xD: R[rt] = R[rs] | imm
        elif op == 0xF: R[rt] = (imm << 16) & M
        elif op in (0x23, 0x31): R[rt] = u(a, 4) if op == 0x23 else R[rt]; F[rt] = u(a, 4) if op == 0x31 else F.get(rt, 0)
        elif op == 0x24: R[rt] = u(a, 1)
        elif op == 0x25: R[rt] = u(a, 2)
        elif op == 0x21: R[rt] = sx(u(a, 2), 16) & M
        elif op == 0x20: R[rt] = sx(u(a, 1), 8) & M
        elif op == 0x28: st(a, R[rt], 1)
        elif op == 0x29: st(a, R[rt], 2)
        elif op == 0x2B: st(a, R[rt], 4)
        elif op == 0x39: st(a, F.get(rt, 0), 4)
        elif op == 0x37: R[rt] = u(a, 8)
        elif op == 0x3F: st(a, R[rt], 8)
        elif op == 0x1E: R[rt] = u(a, 16)
        elif op == 0x1F: st(a, R[rt], 16)
        else: raise SystemExit(f'save_respawn: opcode {op:#x} at {pc:#x} is not handled')
        R[0] = 0
        return None
    F = {}
    while True:
        r = step(pc)
        if r is None:
            pc += 4
        elif r == 'call':
            nxt = step(pc + 4)
            calls += 1
            if calls == 1:                       # the memcpy of the level state: (dst, src, size)
                m[R[4]:R[4] + R[6]] = m[R[5]:R[5] + R[6]]
                pc += 8
            else:
                break
        elif r == 'ret':
            raise SystemExit('save_respawn returned before its last call')
        else:
            _, target, likely = r
            if target is None:
                pc += 8 if likely else 4         # not taken: a likely branch skips its delay slot, another runs it next
            else:
                step(pc + 4)
                pc = target
    return bytes(m[g['snap']:g['snap'] + 0xC70])


def ram_word(m, pc):
    return int.from_bytes(m[pc:pc + 4], 'little')


def go(ram, g, pt, wait=400):
    if pt['pos_raw'] is None:
        raise SystemExit('point 0: the dump has no respawn snapshot (its first word is not 1), the level start has no respawn point')
    img = snapshot_image(ram, g, pt['pos_raw'], pt['rot_raw'])
    cmds = [f"write {g['snap'] + o:x} {img[o:o + 0x400].hex()}" for o in range(0, len(img), 0x400)]
    if pt['spawn'] is not None and g['ctrl_pv28']:
        cmds.append(f"write {g['ctrl_pv28']:x} " + struct.pack('<I', pt['spawn']).hex())
    cmds += [f"write {HERO_AREA:x} {pt['area']:02x}", f'write {HERO_HEALTH:x} 00000000', f'run {wait}']
    return '; '.join(cmds)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('ram')
    ap.add_argument('level', type=int)
    ap.add_argument('--go', type=int, metavar='K')
    a = ap.parse_args()
    ram = Ram(a.ram)
    g, pts = points(ram, a.level)
    if a.go is None:
        if g['reached'] is not None:
            print(f"level {a.level}: checkpoint_reached {g['reached']:#x}, save_respawn {g['save']:#x}, controller {g['controller_fn']:#x}, "
                  f"volume table pointer {g['spawn_ptr']:#x}, snapshot {g['snap']:#x}, moby array {g['mobys'][0]:#x}-{g['mobys'][1]:#x}")
        else:
            print(f"level {a.level}: save_respawn {g['save']:#x}, snapshot {g['snap']:#x}, moby array {g['mobys'][0]:#x}-{g['mobys'][1]:#x}")
        print('K  id    area spawn  position                      rotation                  source')
        for k, p in enumerate(pts):
            print(f"{k:<2} {str(p['id']):<5} {p['area']:<4} {str(p['spawn']):<5} "
                  f"({p['pos'][0]:8.2f}, {p['pos'][1]:8.2f}, {p['pos'][2]:7.2f})  ({p['rot'][0]:.3f}, {p['rot'][1]:.3f}, {p['rot'][2]:.3f})  "
                  f"{p.get('src', 'checkpoint chain')}")
        for n in g.get('notes', []):
            print('note:', n)
    else:
        print(go(ram, g, pts[a.go]))


if __name__ == '__main__':
    main()
