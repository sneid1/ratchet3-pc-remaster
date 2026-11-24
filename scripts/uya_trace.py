"""Read a `uya frames --trace FIELD` file: what the game uploads to VU1 for each program run, and where in the level data it came from.

    python -I scripts/uya_trace.py DISC TRACE RAM LEVEL [--runs N] [--prog P]

TRACE is captures/<dir>/trace-fNNNNN.bin, RAM the EE RAM of the same field (`--ram-dir`), LEVEL the level being played. Each
VIF1 job's DMA blocks carry their EE source addresses, so every UNPACK's data is placed: in the level's core data (its offset), in
its gameplay block, in the level's other blocks as they sit in RAM, or elsewhere in RAM (buffers the game builds). The report:
per VU1 program, its runs, GS output and the sources of its unpacks; with --runs, the first N runs of program --prog in full.
"""

import struct
import sys
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from uya_disc import wad_decompress  # noqa: E402
from uya_vugen import PROGS  # noqa: E402

# The RAM address of each 0x800-byte piece of each VU1 program (uya_vugen.PROGS): an MPG whose data comes from there uploads it.
PIECES = {a: (pid, k) for pid, (_, addrs, _) in PROGS.items() for k, a in enumerate(addrs)}

SECTOR = 0x800
TOC = 1001


def u32(b, o):
    return struct.unpack_from('<I', b, o)[0]


def read_trace(path):
    d = Path(path).read_bytes()
    w = struct.unpack_from('<%dI' % (len(d) // 4), d)
    if w[0] != u32(b'UYAT', 0) or w[1] != 1:
        sys.exit('not a version 1 trace')
    field, njobs, i = w[2], w[3], 4
    jobs = []
    for _ in range(njobs):
        n = w[i]; i += 1
        words = w[i:i + n]; i += n
        nb = w[i]; i += 1
        blocks = [w[i + 4 * k:i + 4 * k + 4] for k in range(nb)]; i += 4 * nb
        ne = w[i]; i += 1
        events = [w[i + 8 * k:i + 8 * k + 8] for k in range(ne)]; i += 8 * ne
        jobs.append((words, blocks, events))
    nq = w[i]; i += 1
    qs = [w[i + 4 * k:i + 4 * k + 4] for k in range(nq)]; i += 4 * nq
    ns = w[i]; i += 1
    segs = [w[i + 3 * k:i + 3 * k + 3] for k in range(ns)]
    return field, jobs, qs, segs


def level_blocks(disc, level):
    """The level's primary parts and gameplay block as the disc holds them (decompressed), by name."""
    with open(disc, 'rb') as f:
        f.seek(TOC * SECTOR)
        toc = f.read(0x10000)
        at = 0
        while 8 <= u32(toc, at) <= 0xffff:
            at += u32(toc, at)
        hdr = None
        for k in range(3):
            lba = u32(toc, at + level * 24 + k * 8)
            if lba > TOC:
                f.seek(lba * SECTOR)
                h = f.read(0x60)
                if u32(h, 0) == 0x60:
                    hdr = h
        if hdr is None:
            sys.exit(f'level {level}: no LEVEL header')
        file = u32(hdr, 4)

        def sec(k):
            f.seek((file + u32(hdr, 0x10 + 8 * k)) * SECTOR)
            return f.read(u32(hdr, 0x14 + 8 * k) * SECTOR)
        prim = sec(0)
        names = ['overlay', 'index', 'gs_ram', 'hud_header', 'hud0', 'hud1', 'hud2', 'hud3', 'hud4', 'core']
        out = {}
        for k, n in enumerate(names):
            o, s = u32(prim, 8 * k), u32(prim, 8 * k + 4)
            out[n] = prim[o:o + s]
        out['core'] = wad_decompress(out['core'])
        out['gameplay'] = wad_decompress(sec(2))
        return out


def locate(ram, blob, name):
    """Where `blob` sits in RAM. The game patches level data as it loads it (pointers, texture slots), so the block is found by
    vote: 64-byte pieces from 32 places through it, each found once in RAM, give base = address - offset; the base most pieces
    agree on wins (at least 4)."""
    votes = Counter()
    for k in range(32):
        off = (len(blob) - 64) * k // 32 & ~15
        piece = blob[off:off + 64]
        if len(set(piece)) < 8:
            continue
        i = ram.find(piece)
        if i >= 0 and ram.find(piece, i + 1) < 0:
            votes[i - off] += 1
    if votes:
        base, n = votes.most_common(1)[0]
        if n >= 4:
            return base
    print(f'  {name}: not found in RAM (votes {votes.most_common(3)})')
    return None


def main():
    a = sys.argv[1:]
    disc, trace, ram_path, level = a[0], a[1], a[2], int(a[3])
    opt = lambda k, d: int(a[a.index(k) + 1]) if k in a else d
    nruns, prog = opt('--runs', 0), opt('--prog', -1)
    field, jobs, qs, segs = read_trace(trace)
    ram = Path(ram_path).read_bytes()
    lv = level_blocks(disc, level)
    bases = {}
    for name in ('core', 'index', 'gameplay', 'gs_ram', 'hud_header', 'hud0'):
        b = locate(ram, lv[name], name)
        if b is not None:
            bases[name] = (b, len(lv[name]))
    print(f'field {field}: {len(jobs)} VIF1 jobs, {len(qs)} GS qwords; level {level} in RAM: ' + ', '.join(f'{n} {b:#x}+{s:#x}' for n, (b, s) in bases.items()))

    def where(addr):
        if addr & 0x80000000:
            return ('scratchpad', addr & 0x3fff)
        a_ = addr & 0x1ffffff
        for n, (b, s) in bases.items():
            if b <= a_ < b + s:
                return (n, a_ - b)
        return ('ram', a_)

    per_prog = defaultdict(lambda: {'runs': 0, 'qw': 0, 'src': Counter(), 'unpacks': 0})
    shown = 0
    resident = -1
    for jn, (words, blocks, events) in enumerate(jobs):
        starts = [b[0] for b in blocks]
        pending = []
        for e in events:
            kind = e[0]
            if kind == 1:
                at = e[1]
                k = max((i for i, s in enumerate(starts) if s <= at), default=None)
                if k is None:
                    src = None
                else:
                    src = blocks[k][1] + (at - blocks[k][0]) * 4
                pending.append((e, src))
            elif kind == 4:
                at = e[1]
                k = max((i for i, s_ in enumerate(starts) if s_ <= at), default=None)
                src = blocks[k][1] + (at - blocks[k][0]) * 4 if k is not None else None
                piece = PIECES.get(src)
                if piece is None:
                    print(f'job {jn}: MPG of {e[3]} words to micro {e[2]:#x} from {src:#x} is no known program piece')
                elif piece[1] == 0:
                    resident = piece[0]
            elif kind == 2:
                pid = resident
                p = per_prog[pid]
                p['runs'] += 1
                p['qw'] += e[5] - e[4]
                for (u, src) in pending:
                    p['unpacks'] += 1
                    region = where(src)[0] if src is not None else 'tag'
                    p['src'][region] += 1
                if nruns and pid == prog and shown < nruns:
                    shown += 1
                    print(f'\njob {jn} run of program {pid} at micro {e[2]:#x}: GS qwords {e[4]}..{e[5]} ({e[5] - e[4]}), TOPS {e[6]:#x} ITOPS {e[7]:#x}')
                    for (u, src) in pending:
                        cmd, num, dest, cyc, words_n = u[2], u[3], u[4], u[5], u[7]
                        loc = where(src) if src is not None else ('tag', 0)
                        print(f'   unpack {cmd:#04x} x{num:<4} -> VU {dest:#05x} cycle {cyc & 0xff}/{(cyc >> 8) & 0xff} mode {cyc >> 16}  from {loc[0]}+{loc[1]:#x} ({words_n} words)')
                pending = []
    print('\nprogram  runs   GS qwords  unpacks  sources')
    for pid in sorted(per_prog):
        p = per_prog[pid]
        print(f'{pid:7} {p["runs"]:6} {p["qw"]:10} {p["unpacks"]:8}  ' + ', '.join(f'{n} {c}' for n, c in p['src'].most_common()))


if __name__ == '__main__':
    main()
