"""Makes docs/surfaces.csv (plan item 9.2 step 1): this game's terrain and tie textures by their share of the picture.

Usage:
  python -I scripts/uya-surfaces.py <measure dir> --dumps DIR --kinds textures/uya-surface-kinds.txt --out docs/surfaces.csv
      [--min-share S] [--ratchet-m M] [--sheet DIR]

<measure dir> is scripts/surfaces-measure.ps1's folder: per tour dump a tsv of each terrain and tie texture's pixels in the
picture (`uya native --surfaces`, port/uya/src/surfaces.rs) and the hero armor's extent in world units. --dumps is the
profile's texture_dumps (the game's textures under their PCSX2 names, `uya texnames --write`).

Per texture: its share on a level is the mean of its share over the level's dumps that drew the world; `share` is its largest
level share, `levels` the levels where it covers at least 0.1%, its repeat (world units per 1.0 of u and v) the one measured
on the level of its largest share. L, a, b: the mean CIELAB of the dump's opaque texels (alpha over 63 on the GS scale, as
scripts/texture-select.py judges colour). The kind (sand, rock, metal, grass, floor, other) is judged by eye from the
textures themselves and kept in the kinds file (one `stem kind` a line); every texture of at least --min-share must have one:
the run stops naming those without, and with --sheet writes contact sheets of them to judge (numbered, with
unjudged.txt giving each number's stem; texels the game cuts out in magenta).

The first line gives the game's units per metre: Ratchet's height (the hero armor's vertical extent, Z, median over the dumps
that drew him) over --ratchet-m metres (1.2 by default: Rift Apart's materials are judged in metres at Ratchet's scale).
"""
import argparse
import csv
import importlib.util
import statistics
import sys
from collections import defaultdict
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont

_here = Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location('texture_select', _here / 'texture-select.py')
_ts = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_ts)
KINDS = ('sand', 'rock', 'metal', 'panel', 'grass', 'floor', 'wood', 'ice', 'plant', 'other')


def read_tsv(path):
    lines = path.read_text(encoding='utf-8').splitlines()
    head = lines[0]
    if not head.startswith('# level '):
        sys.exit(f'{path}: no "# level" line')
    level = int(head.split()[2].rstrip(','))
    hero = None
    if 'hero armor extent' in head:
        part = head.split('hero armor extent ')[1].split()
        hero = (float(part[1]), float(part[3]), float(part[5]))
    rows = list(csv.DictReader(lines[1:], delimiter='\t'))
    return level, hero, rows


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('measure')
    ap.add_argument('--dumps', required=True)
    ap.add_argument('--kinds', required=True)
    ap.add_argument('--out', required=True)
    ap.add_argument('--min-share', type=float, default=0.005)
    ap.add_argument('--ratchet-m', type=float, default=1.2)
    ap.add_argument('--sheet')
    args = ap.parse_args()

    tsvs = sorted(Path(args.measure).glob('f*.tsv'))
    if not tsvs:
        sys.exit(f'{args.measure}: no f*.tsv')
    per_level = defaultdict(list)
    heroes = []
    info = {}
    for p in tsvs:
        level, hero, rows = read_tsv(p)
        if hero is not None:
            heroes.append(hero)
        if not rows:
            continue
        per_level[level].append({r['stem']: r for r in rows})
        for r in rows:
            info.setdefault(r['stem'], r)
    if not heroes:
        sys.exit(f'{args.measure}: no dump drew the hero armor')
    height = statistics.median(h[2] for h in heroes)
    upm = height / args.ratchet_m

    shares = defaultdict(dict)
    repeats = defaultdict(dict)
    for level, frames in per_level.items():
        stems = {s for f in frames for s in f}
        for s in stems:
            shares[s][level] = sum(float(f[s]['share']) for f in frames if s in f) / len(frames)
            r = next(f[s] for f in frames if s in f)
            repeats[s][level] = (r['repeat_u'], r['repeat_v'])

    kinds = {}
    for n, line in enumerate(Path(args.kinds).read_text(encoding='utf-8').splitlines(), 1):
        line = line.split('#', 1)[0].strip()
        if not line:
            continue
        parts = line.split()
        if len(parts) != 2 or parts[1] not in KINDS:
            sys.exit(f'{args.kinds}:{n}: expected "stem kind", kind one of {KINDS}: {line!r}')
        kinds[parts[0]] = parts[1]

    rows = []
    for s, by_level in shares.items():
        best = max(by_level, key=by_level.get)
        if by_level[best] < args.min_share:
            continue
        rows.append((s, by_level, best))
    rows.sort(key=lambda r: -r[1][r[2]])
    missing = [r for r in rows if r[0] not in kinds]
    if missing:
        if args.sheet:
            sheet(missing, args.dumps, Path(args.sheet))
        sys.exit(f'{len(missing)} textures of at least {args.min_share:.3f} have no kind in {args.kinds}: '
                 + ' '.join(r[0] for r in missing[:20]) + (' ...' if len(missing) > 20 else ''))

    out = [f'# units per metre {upm:.4f} (Ratchet {height:.3f} units tall, median of {len(heroes)} dumps, taken as {args.ratchet_m:g} m)',
           'name,levels,share,width,height,repeat_u,repeat_v,L,a,b,kind']
    for s, by_level, best in rows:
        dump = Path(args.dumps) / f'{s}.png'
        if not dump.exists():
            sys.exit(f'{dump}: not in the dumps')
        px = np.asarray(Image.open(dump).convert('RGBA'), np.float32).reshape(-1, 4)
        opaque = px[px[:, 3] > 63]
        if len(opaque) < 4:
            opaque = px
        L, a, b = _ts.lab(opaque[:, :3]).mean(0)
        levels = ';'.join(str(lv) for lv in sorted(by_level) if by_level[lv] >= 0.001)
        ru, rv = repeats[s][best]
        i = info[s]
        out.append(f'{s},{levels},{by_level[best]:.5f},{i["width"]},{i["height"]},{ru},{rv},{L:.2f},{a:.2f},{b:.2f},{kinds[s]}')
    Path(args.out).write_text('\n'.join(out) + '\n', encoding='utf-8')
    print(f'{args.out}: {len(rows)} textures; {out[0][2:]}')


def sheet(rows, dumps, out, cell=160, cols=8):
    out.mkdir(parents=True, exist_ok=True)
    (out / 'unjudged.txt').write_text(''.join(f'{i + 1} {r[0]}\n' for i, r in enumerate(rows)), encoding='utf-8')
    font = ImageFont.truetype(r'C:\Windows\Fonts\arial.ttf', 12)
    for page in range(0, len(rows), cols * 6):
        chunk = rows[page:page + cols * 6]
        img = Image.new('RGB', (cols * (cell + 8) + 8, ((len(chunk) + cols - 1) // cols) * (cell + 44) + 8), (24, 24, 26))
        d = ImageDraw.Draw(img)
        for i, (s, by_level, best) in enumerate(chunk):
            x, y = 8 + (i % cols) * (cell + 8), 8 + (i // cols) * (cell + 44)
            # The texels the game cuts out (alpha under a quarter of opaque, 0x80 on the GS scale) in magenta.
            px = np.asarray(Image.open(Path(dumps) / f'{s}.png').convert('RGBA')).copy()
            px[px[..., 3] < 32, :3] = (255, 0, 255)
            t = Image.fromarray(np.ascontiguousarray(px[..., :3])).resize((cell, cell), Image.NEAREST)
            img.paste(t, (x, y))
            d.text((x, y + cell + 2), f'{page + i + 1}. {s[:24]}', fill=(230, 230, 230), font=font)
            d.text((x, y + cell + 18), f'{by_level[best] * 100:.1f}% L{best}  ' + ' '.join(str(lv) for lv in sorted(by_level) if by_level[lv] >= 0.001)[:30], fill=(150, 150, 150), font=font)
        p = out / f'unjudged_{page // (cols * 6) + 1}.png'
        img.save(p)
        print(p)


if __name__ == '__main__':
    main()
