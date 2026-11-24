"""Groups the objects' textures that are the same picture (the same object loaded by several levels under other PCSX2 names:
its palette or texels moved by a few steps), so that each picture is matched once and its copies take the same row (PLAN 9.2).

Usage:
  python -I scripts/riftapart-same.py <triage dir> <out.tsv> [--tolerance T]

Two textures are the same picture when they have the same size and class and their 32 x 32 thumbnails (colour and alpha) differ
by at most T (default 3 of 255) on average and have the same structure (`same`): a looser rule at 6 grouped a dark stone with an
olive panel. <out.tsv>: stem, class, kind, shows, group (the stem of the group's first, the one matched), largest share first
within each group.
"""
import argparse
import csv
import importlib.util
from pathlib import Path

import numpy as np
from PIL import Image

_here = Path(__file__).resolve().parent
ROOT = _here.parent
_spec = importlib.util.spec_from_file_location('pair', _here / 'riftapart-pair.py')
pair = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(pair)


def same(a, b, tolerance):
    """The same picture: colour (RGB) within `tolerance` on average, the alpha within it too, and the same structure (the
    luminance's correlation above 0.9; two flat pictures, both of spread under 3, by colour alone)."""
    if np.abs(a[..., :3] - b[..., :3]).mean() > tolerance or np.abs(a[..., 3] - b[..., 3]).mean() > tolerance:
        return False
    la, lb = a[..., :3].mean(-1), b[..., :3].mean(-1)
    if la.std() < 3 and lb.std() < 3:
        return True
    return float(np.corrcoef(la.ravel(), lb.ravel())[0, 1]) > 0.9


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('triage')
    ap.add_argument('out')
    ap.add_argument('--tolerance', type=float, default=3.0)
    args = ap.parse_args()
    share = {r['name']: float(r['share']) for r in csv.DictReader((ROOT / 'docs/surfaces-objects.csv').read_text().splitlines()[1:])}
    rows = []
    for f in sorted(Path(args.triage).glob('triage-t*.tsv')):
        for l in f.read_text(encoding='utf-8-sig').splitlines()[1:]:
            if l.strip():
                n, stem, cls, kind, shows = l.split('\t')
                rows.append(dict(stem=stem, cls=cls, kind=kind, shows=shows))
    rows.sort(key=lambda r: -share.get(r['stem'], 0))
    thumbs = {}
    for r in rows:
        if r['cls'] == 'none':
            continue
        im = Image.open(pair.texture_file(r['stem'], str(pair.PROFILE))).convert('RGBA')
        thumbs[r['stem']] = (im.size, np.asarray(im.resize((32, 32), Image.BOX), np.float32))
    groups = []
    for r in rows:
        if r['cls'] == 'none':
            r['group'] = ''
            continue
        size, t = thumbs[r['stem']]
        for g in groups:
            if g['cls'] == r['cls'] and g['size'] == size and same(g['thumb'], t, args.tolerance):
                r['group'] = g['stem']
                g['n'] += 1
                break
        else:
            groups.append(dict(stem=r['stem'], cls=r['cls'], size=size, thumb=t, n=1))
            r['group'] = r['stem']
    with open(args.out, 'w', newline='', encoding='utf-8') as fh:
        w = csv.writer(fh, delimiter='\t', lineterminator='\n')
        w.writerow(['stem', 'class', 'kind', 'shows', 'group'])
        for r in rows:
            w.writerow([r['stem'], r['cls'], r['kind'], r['shows'], r['group']])
    for cls in ('surface', 'character'):
        n = sum(1 for r in rows if r['cls'] == cls)
        g = sum(1 for x in groups if x['cls'] == cls)
        print(f'{cls}: {n} textures in {g} pictures')


if __name__ == '__main__':
    main()
