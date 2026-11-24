"""Timings across a whole level, not only at its start: Ratchet respawned (scripts/checkpoints.py's way: the game's own respawn
snapshot, then a death) at spots spread over the level, a paced hidden timing run held at each, and a table of every spot's
pictures a second, GPU and presenter time and the game's EE time, worst first.

    python scripts/coverage.py plan RAM.bin LEVEL OUT_DIR [--points N] [--hold F] [--pre "CMD; CMD"] [--save]
    python scripts/coverage.py report OUT_DIR

`plan` reads a RAM dump of the running level (uyactl `dump DIR; run 5` once the level plays) and writes OUT_DIR/plan.json and
OUT_DIR/cmds.txt, then prints the uyactl line that runs it (paced, exclusive, hidden). `report` reads OUT_DIR/run.log, the
run's output, and prints the table (and writes OUT_DIR/report.txt).

The spots: the level's live mobies (the moby array checkpoints.find gives: 0x100-byte records, +0x10 the position, +0x20 the state
byte, negative for none living) stand where the level's objects are (crates, enemies, vendors, doors), so on and near its floors;
crates first (classes 0x1F4-0x200: they stand on the player's way), one a 32-unit cell (the lowest, a floor's more than a flying one's; none more than 60 units below the hero's place or 150
above it), then the farthest from those already taken, from the
hero's own place on, up to --points. Ratchet respawns 2 units above it facing the middle of the spots (the respawn sets the
camera behind him). The level's own fighting goes on: his health is set to 1000 once he stands there, so a hold measures the
place and not his deaths; his place and health are read after the hold, and a spot he fell from (more than 8 units down, or
dead, still where the spot before left him, or not standing at its end: falling into a void) is marked in the report; one he fell from to a floor below is measured where
he landed. `--teleport` writes his place instead of respawning him (a Battle Mission's level, 18's or 26's, ends the mission on a
death and goes back to its menu); `--recover "cmds"` goes before every spot but the first (for a mission: the presses that
start it again from its menu, which in play only make him jump). `--save` plans for a run that starts from the profile's save (LOAD GAME, the first slot) instead
of `--start LEVEL`; `--pre` adds commands after the level is up (a cutscene skip).

The numbers come from the run's UYA_FRAME_TIMES lines: each picture presented in the hold ('frame stages: present F ...', F the
game field it shows) with its encode, GPU and display ms; each field's EE ms ('frame stages: game F EE ms').
"""

import argparse
import json
import math
import os
import re
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import checkpoints  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CELL = 32.0
# Crates: classes 0x1F4 to 0x200 (rac3-decomp config/names.tsv, crate_update).
CRATES = (0x1F4, 0x200)
# The hero's states that stand on a floor (rac3-decomp include/enums/hero_state.h): idle, look, walk, skid, crouch, hit, aiming.
STANDING = {0x0, 0x1, 0x2, 0x3, 0x4, 0x16, 0x1D}
WAIT = 400


def spots(ram, g, n):
    s, e = g['mobys']
    hero = [ram.f32(0x1A4C60 + 4 * i) for i in range(3)]
    crates, others = {}, {}
    for m in range(s, e, 256):
        if ram.b[m + 0x20] >= 0x80:
            continue
        p = [ram.f32(m + 0x10 + 4 * i) for i in range(3)]
        # Flying and buried ones left out (a ship 560 units up: a fall the next respawn cannot start in).
        if not all(math.isfinite(v) and abs(v) < 4096 for v in p) or p == [0.0, 0.0, 0.0] or not hero[2] - 60 < p[2] < hero[2] + 150:
            continue
        cls = ram.s16(m + 0xAA)
        cells = crates if CRATES[0] <= cls <= CRATES[1] else others
        k = (int(p[0] // CELL), int(p[1] // CELL))
        if k not in cells or p[2] < cells[k][1][2]:
            cells[k] = (cls, p)
    if not crates and not others:
        raise SystemExit('no live moby in the dump')
    taken = []
    far = lambda q: min(math.dist(q[1][:2], t[1][:2]) for t in taken) if taken else math.dist(q[1][:2], hero[:2])
    # The crates' cells first (a crate stands on the player's way), then other objects' cells where no crate is.
    for pool in (list(crates.values()), [q for k, q in others.items() if k not in crates]):
        while pool and len(taken) < n:
            q = max(pool, key=far) if taken else min(pool, key=far)
            taken.append(q)
            pool.remove(q)
    return hero, taken


def plan(a):
    ram = checkpoints.Ram(a.ram)
    g, pts = checkpoints.points(ram, a.level)
    hero, taken = spots(ram, g, a.points)
    cx = sum(t[1][0] for t in taken) / len(taken)
    cy = sum(t[1][1] for t in taken) / len(taken)
    os.makedirs(a.out, exist_ok=True)
    out = os.path.abspath(a.out).replace('\\', '/')
    if a.save:
        cmds = ['until field 700', 'press 40 4', 'run 200', 'press 40 4', 'run 300', f'until level {a.level} max 6000', 'run 600']
    else:
        cmds = [f'until level {a.level} max 9000', 'run 600']
    cmds += [c.strip() for c in a.pre.split(';') if c.strip()]
    rows = []
    for k, (cls, p) in enumerate(taken):
        if k:
            cmds += [c.strip() for c in a.recover.split(';') if c.strip()]
        turn = math.atan2(cy - p[1], cx - p[0])
        pt = dict(spawn=None, area=ram.b[checkpoints.HERO_AREA],
                  pos_raw=struct.pack('<4f', p[0], p[1], p[2] + 2.0, 1.0), rot_raw=struct.pack('<4f', 0.0, 0.0, turn, 0.0))
        if a.teleport:
            # In a mission a death ends it (the level goes back to its Battle Missions menu): his place written instead, the
            # game's own fall and landing from 2 units above the spot.
            cmds += [f"write 1a4c60 {struct.pack('<3f', p[0], p[1], p[2] + 2.0).hex()}", 'run 120']
        else:
            cmds += [c.strip() for c in checkpoints.go(ram, g, pt, WAIT).split(';')]
        cmds += ['write 1a7430 e8030000', f'run {a.hold}', 'ram 1a4c60 12', 'ram 1a7430 4', 'ram 1a71a4 4', f'shot {out}/S{k:02d}.png']
        rows.append(dict(k=k, cls=cls, pos=p, turn=turn))
    with open(os.path.join(a.out, 'cmds.txt'), 'w') as f:
        f.write('\n'.join(cmds) + '\n')
    with open(os.path.join(a.out, 'plan.json'), 'w') as f:
        json.dump(dict(level=a.level, hold=a.hold, save=a.save, hero=hero, spots=rows), f, indent=1)
    start = '' if a.save else f' --start {a.level} --press 15:800:12'
    prof = 'captures/metro/profile' if a.save else 'captures/sweep/profile'
    print(f'{len(rows)} spots; run (about {len(rows) * (WAIT + a.hold + 20) // 3600 + 2} minutes):')
    print(f'UYA_CONTROL_HIDDEN=1 UYA_CONTROL_PACED=1 UYA_FRAME_TIMES=1 timeout 7200 python scripts/uyactl.py -v --exclusive --keep-profile '
          f'--profile {prof} --limit-mb 12000 -f {a.out}/cmds.txt --{start} --mute --prompts keyboard > {a.out}/run.log 2>&1')


def report(a):
    p = json.load(open(os.path.join(a.out, 'plan.json')))
    log = open(os.path.join(a.out, 'run.log'), errors='replace').read()
    # The hold's last field: the `run HOLD` answer after each `write 1a7430 e8030000`; its place and health: the two ram reads after.
    ends = [int(m[1]) for m in re.finditer(rf'run {p["hold"]}\s+-> ok field (\d+)', log)]
    reads = list(zip(re.findall(r'ram 1a4c60 12\s+-> ok ([0-9a-f]{24})', log), re.findall(r'ram 1a7430 4\s+-> ok ([0-9a-f]{8})', log),
                     re.findall(r'ram 1a71a4 4\s+-> ok ([0-9a-f]{8})', log)))
    if len(ends) != len(p['spots']) or len(reads) != len(p['spots']):
        raise SystemExit(f"the run held {len(ends)} spots and read {len(reads)} of the plan's {len(p['spots'])}: it ended early (see run.log)")
    pres = [(int(m[1]), float(m[2]), float(m[3]), float(m[4])) for m in
            re.finditer(r'frame stages: present (\d+) \w+ encode ([\d.]+) GPU ([\d.]+)(?: first)? display ([\d.]+)', log)]
    ee = {int(m[1]): float(m[2]) for m in re.finditer(r'frame stages: game (\d+) EE ([\d.]+)', log)}
    rows = []
    for s, end, (pos_hex, hp_hex, state_hex) in zip(p['spots'], ends, reads):
        lo = end - p['hold'] + 60
        w = [x for x in pres if lo <= x[0] < end]
        fields = end - lo
        here = struct.unpack('<3f', bytes.fromhex(pos_hex))
        hp = struct.unpack('<i', bytes.fromhex(hp_hex))[0]
        # Not respawned there (still where the spot before left him), dead, or under the level (no world drawn): no measure of a
        # place. Fallen and standing elsewhere below it: a measure of where he landed.
        state = struct.unpack('<i', bytes.fromhex(state_hex))[0]
        missed = math.dist(here[:2], s['pos'][:2]) > 30.0
        fell = hp <= 0 or missed or not w or state not in STANDING
        landed = not fell and here[2] < s['pos'][2] - 8.0
        e = [ee[f] for f in range(lo, end) if f in ee]
        mean = lambda v: sum(v) / len(v) if v else float('nan')
        rows.append(dict(k=s['k'], pos=s['pos'], pics=len(w) / (fields / 59.94), gpu=mean([x[2] for x in w]), enc=mean([x[1] for x in w]),
                         disp=mean([x[3] for x in w]), ee=mean(e), ee_max=max(e) if e else float('nan'), here=here, hp=hp, fell=fell, landed=landed,
                         why='dead' if hp <= 0 else 'not respawned there' if missed else 'no world drawn' if not w else f'not standing (state {state}: falling or in the air)' if state not in STANDING else ''))
    lines = [f"level {p['level']}: {len(rows)} spots, {p['hold']} fields each (the first 60 after his health is set left out)",
             ' K  pictures/s  GPU ms  encode ms  display ms  EE ms (max)   spot (x, y, z)              after: place, health']
    for r in sorted(rows, key=lambda r: (r['fell'], r['pics'])):
        lines.append(f" {r['k']:>2}  {r['pics']:9.1f}  {r['gpu']:6.2f}  {r['enc']:9.2f}  {r['disp']:10.2f}  {r['ee']:5.1f} ({r['ee_max']:5.1f})  "
                     f"({r['pos'][0]:7.1f}, {r['pos'][1]:7.1f}, {r['pos'][2]:6.1f})  ({r['here'][0]:7.1f}, {r['here'][1]:7.1f}, {r['here'][2]:6.1f}) {r['hp']}"
                     + (f"  NOT MEASURED: {r['why']}" if r['fell'] else '  (fell; measured where he landed)' if r['landed'] else '')
                     # Health far under the 1000 set: killed and respawned with his own health in the hold (its fade in the numbers).
                     + ('  (died and respawned in the hold)' if not r['fell'] and r['hp'] < 200 else ''))
    text = '\n'.join(lines)
    print(text)
    open(os.path.join(a.out, 'report.txt'), 'w').write(text + '\n')


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest='cmd', required=True)
    pl = sub.add_parser('plan')
    pl.add_argument('ram')
    pl.add_argument('level', type=int)
    pl.add_argument('out')
    pl.add_argument('--points', type=int, default=24)
    pl.add_argument('--hold', type=int, default=360)
    pl.add_argument('--pre', default='')
    pl.add_argument('--save', action='store_true')
    pl.add_argument('--teleport', action='store_true')
    pl.add_argument('--recover', default='')
    rp = sub.add_parser('report')
    rp.add_argument('out')
    a = ap.parse_args()
    plan(a) if a.cmd == 'plan' else report(a)


if __name__ == '__main__':
    main()
