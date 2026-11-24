"""Tiling materials derived from Rift Apart's own textures where its library has none (PLAN 9.2; the player: "water, glass,
lights, plant leaves"), written into the export's `more/derived/<name>/` as `<name>_c.png`, `_n.png`, `_g.png`, so
`riftapart-pair.py` takes them as any other material (never committed: the export is the player's own Rift Apart).

- `leaf_surface_001`: the inside of the Sargasso walnut leaf (`gm_sargasso_walnut_plant_leaf_01`: its colour, normal and gloss),
  a crop between its rim and its midrib, mirrored into a tile (a leaf's veins with no leaf's outline: the earlier waves found
  Rift Apart's leaf textures are whole leaves, which made noise on the game's own leaves).
- `leaf_surface_002`: the inside of the Metropolis broad leaf (`met_plant_broad_leaf_01`: its normal and gloss; it has no
  colour map, so it is a detail normal and its grain its relief).
- `glass_001`: Rift Apart's clean glass (`glass_001_g`, its gloss: the export has no colour or normal map of it) with a flat
  normal and an even grain.

A crop is mirrored into a 2 x 2 tile so it repeats with no seam: a normal map mirrored across x has its x (red) turned over, and
across y its y (green).

Usage:
  py -3.12 -I scripts/riftapart-derive.py [--export C:\\riftapart-export] [--sheet PNG]
"""
import argparse
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

EXPORT = Path(r'C:\riftapart-export')
# Each derived leaf: its source folder (relative to the export), the source's name, and the crop (left, top, right, bottom)
# as shares of the source image: inside one leaf, clear of its rim and its midrib.
LEAVES = {
    'leaf_surface_001': ('more/textures/sargasso/shrub/gm_sargasso_walnut_plant_leaf_01', 'gm_sargasso_walnut_plant_leaf_01', (0.10, 0.36, 0.22, 0.60)),
    'leaf_surface_002': ('more/textures/environment/metropolis/plant/met_plant_broad_leaf_01', 'met_plant_broad_leaf_01', (0.18, 0.30, 0.32, 0.62)),
}
# The tile's size in texels.
SIZE = 1024


def mirror_tile(a, normal):
    """`a` (H x W x C) and its mirror images as a 2 x 2 tile that repeats with no seam."""
    right = a[:, ::-1].copy()
    if normal:
        right[..., 0] = 255 - right[..., 0]
    top = np.concatenate([a, right], axis=1)
    bottom = top[::-1].copy()
    if normal:
        bottom[..., 1] = 255 - bottom[..., 1]
    return np.concatenate([top, bottom], axis=0)


def crop(path, box, mode):
    im = Image.open(path).convert(mode)
    w, h = im.size
    c = im.crop((int(box[0] * w), int(box[1] * h), int(box[2] * w), int(box[3] * h)))
    return np.asarray(c.resize((SIZE // 2, SIZE // 2), Image.LANCZOS))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--export', default=str(EXPORT))
    ap.add_argument('--sheet')
    args = ap.parse_args()
    export = Path(args.export)
    derived = export / 'more' / 'derived'
    made = []
    for name, (folder, src, box) in LEAVES.items():
        out = derived / name
        out.mkdir(parents=True, exist_ok=True)
        base = export / folder
        for suffix, mode, normal in (('_c', 'RGB', False), ('_n', 'RGB', True), ('_g', 'RGB', False)):
            f = base / f'{src}{suffix}.png'
            if not f.exists():
                if suffix == '_n':
                    raise SystemExit(f'{f}: a leaf needs its normal map')
                continue
            Image.fromarray(mirror_tile(crop(f, box, mode), normal)).save(out / f'{name}{suffix}.png')
        made.append(name)
    # Clean glass: Rift Apart's own gloss, a flat normal, an even mid grey (a grain of 1).
    g = export / 'other' / 'sbs_glass' / 'glass_001_g.png'
    if not g.exists():
        raise SystemExit(f'{g}: Rift Apart\'s glass gloss is not exported')
    out = derived / 'glass_001'
    out.mkdir(parents=True, exist_ok=True)
    gl = Image.open(g).convert('RGB')
    gl.save(out / 'glass_001_g.png')
    Image.new('RGB', gl.size, (128, 128, 255)).save(out / 'glass_001_n.png')
    Image.new('RGB', gl.size, (128, 128, 128)).save(out / 'glass_001_c.png')
    made.append('glass_001')
    print('derived:', ', '.join(made), 'in', derived)
    if args.sheet:
        tiles = []
        for name in made:
            for suffix in ('_c', '_n'):
                f = derived / name / f'{name}{suffix}.png'
                if f.exists():
                    tiles.append((f'{name}{suffix}', Image.open(f).convert('RGB').resize((256, 256))))
        sheet = Image.new('RGB', (len(tiles) * 266 + 10, 300), (24, 24, 26))
        d = ImageDraw.Draw(sheet)
        for j, (t, im) in enumerate(tiles):
            # two by two repeats of the tile, to show it has no seam
            rep = Image.new('RGB', (256, 256))
            small = im.resize((128, 128))
            for y in (0, 128):
                for x in (0, 128):
                    rep.paste(small, (x, y))
            sheet.paste(rep, (10 + j * 266, 30))
            d.text((10 + j * 266, 8), t, fill=(235, 235, 235))
        sheet.save(args.sheet)
        print(args.sheet)


if __name__ == '__main__':
    main()
