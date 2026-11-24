"""Every Rift Apart material's grain contrast as the game shows it, for the matching (PLAN 9.2): per material in the export its
folder and its grain's standard deviation with one tile at 64, 128 and 256 texels on the screen (a character's tile, a prop's,
a wall's: detail finer than a screen texel is filtered away), and its normal map's mean slope (the relief the lighting shows).

Usage:
  python -I scripts/riftapart-contrast.py <out.csv> [--export DIR]

The materials whose grain shows at a character's size are those with a contrast near or above the approved cliff's (cliff_003:
0.031 at 256) at 64 or 128: detail normals and fine paints fall to almost nothing there (15 November 2026: the first character
matches, `detail_skinhippo` and `paint_003` at 0.3 m, barely changed level 3's brute; `blz_rock_006` on him showed plainly).
"""
import argparse
import csv
import importlib.util
from pathlib import Path

import numpy as np
from PIL import Image

_here = Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location('pair', _here / 'riftapart-pair.py')
pair = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(pair)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('out')
    ap.add_argument('--export', default=r'C:\riftapart-export')
    args = ap.parse_args()
    rows = []
    idx = pair.materials_index(args.export)
    for name in sorted(idx):
        f = pair.find_material(args.export, name)
        if isinstance(f, str):
            continue
        c, n, g = f
        if c is not None and (np.asarray(Image.open(c).convert('RGB').resize((64, 64))) @ pair.LUMA).mean() < 8:
            continue
        mp, _ = pair.material(args.export, name, 512)
        grain = mp[..., 3].astype(np.float32) / pair.GRAIN_ONE
        xy = mp[..., :2].astype(np.float32) / 127.5 - 1
        seen = {k: float(np.asarray(Image.fromarray(grain).resize((k, k), Image.BOX)).std()) for k in (64, 128, 256)}
        rows.append(dict(material=name, folder=str(idx[name][0].parent.relative_to(args.export)), seen64=f'{seen[64]:.3f}',
                         seen128=f'{seen[128]:.3f}', seen256=f'{seen[256]:.3f}', slope=f'{float(np.sqrt((xy ** 2).sum(-1)).mean()):.3f}',
                         detail='yes' if c is None else ''))
        print(name, rows[-1]['seen64'], flush=True)
    with open(args.out, 'w', newline='', encoding='utf-8') as fh:
        w = csv.DictWriter(fh, fieldnames=list(rows[0].keys()), lineterminator='\n')
        w.writeheader()
        w.writerows(rows)
    print(f'{len(rows)} materials: {args.out}')


if __name__ == '__main__':
    main()
