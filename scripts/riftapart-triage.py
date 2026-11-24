"""Triage sheets for the objects' textures (PLAN 9.2: every level's props, shrubs and enemies): the unmatched object textures
of docs/surfaces-objects.csv (not only the vid-comic levels'), 48 a sheet, each with its number, size, levels and whether a
saved gameplay frame drew it, so an agent can sort them (a surface to match, a character's atlas, or none: effects, glows,
lights, screens, UI, plants' leaves) before the matching.

Usage:
  python -I scripts/riftapart-triage.py <out dir> [--per 48]

Writes <out dir>/sheet_NNN.png and <out dir>/sheet_NNN.tsv (number, stem, size, levels, seen), largest share first, then the
unseen ones by how many levels load them.
"""
import argparse
import csv
import importlib.util
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont

_here = Path(__file__).resolve().parent
ROOT = _here.parent
_spec = importlib.util.spec_from_file_location('pair', _here / 'riftapart-pair.py')
pair = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(pair)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('out')
    ap.add_argument('--per', type=int, default=48)
    args = ap.parse_args()
    rows = [r for r in csv.DictReader((ROOT / 'docs/surfaces-objects.csv').read_text().splitlines()[1:]) if r['kind'] == 'object']
    done = set()
    for l in (ROOT / 'textures/riftapart-matches.tsv').read_text(encoding='utf-8').splitlines():
        if not l.startswith('#') and not l.startswith('stem\t'):
            done.add(l.split('\t')[0])
    comic = lambda r: all(30 <= int(x) <= 36 for x in r['levels'].split(';'))
    todo = [r for r in rows if r['name'] not in done and not comic(r)]
    todo.sort(key=lambda r: (-float(r['share']), -len(r['levels'].split(';'))))
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    cols, cell = 8, 200
    f = ImageFont.truetype(r'C:\Windows\Fonts\arial.ttf', 12)
    fb = ImageFont.truetype(r'C:\Windows\Fonts\arialbd.ttf', 15)
    for page in range(0, len(todo), args.per):
        chunk = todo[page:page + args.per]
        rows_n = (len(chunk) + cols - 1) // cols
        sheet = Image.new('RGB', (cols * (cell + 10) + 10, rows_n * (cell + 44) + 10), (40, 40, 46))
        d = ImageDraw.Draw(sheet)
        lines = ['number\tstem\tsize\tlevels\tseen']
        for i, r in enumerate(chunk):
            x, y = 10 + (i % cols) * (cell + 10), 10 + (i // cols) * (cell + 44)
            im = Image.open(pair.texture_file(r['name'], str(pair.PROFILE))).convert('RGBA')
            a = np.asarray(im).copy()
            a[..., 3] = np.clip(a[..., 3].astype(np.int32) * 2, 0, 255)
            bg = Image.new('RGBA', im.size, (90, 90, 100, 255))
            bg.alpha_composite(Image.fromarray(a, 'RGBA'))
            big = bg.convert('RGB').resize((cell, cell), Image.NEAREST if im.width < cell else Image.LANCZOS)
            sheet.paste(big, (x, y))
            n = page + i + 1
            seen = float(r['share']) > 0
            d.text((x + 4, y + 2), str(n), fill=(255, 255, 0), font=fb, stroke_width=2, stroke_fill=(0, 0, 0))
            d.text((x, y + cell + 2), f'{r["width"]}x{r["height"]}  ' + ('SEEN ' + f'{float(r["share"]) * 100:.2f}%' if seen else 'unseen'), fill=(150, 230, 150) if seen else (200, 200, 200), font=f)
            lv = r['levels'].split(';')
            d.text((x, y + cell + 18), 'levels ' + (';'.join(lv[:8]) + ('...' if len(lv) > 8 else '')), fill=(170, 170, 170), font=f)
            lines.append(f'{n}\t{r["name"]}\t{r["width"]}x{r["height"]}\t{r["levels"]}\t{int(seen)}')
        k = page // args.per + 1
        sheet.save(out / f'sheet_{k:03d}.png')
        (out / f'sheet_{k:03d}.tsv').write_text('\n'.join(lines) + '\n', encoding='utf-8')
    print(f'{len(todo)} textures on {(len(todo) + args.per - 1) // args.per} sheets: {out}')


if __name__ == '__main__':
    main()
