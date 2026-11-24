"""HD colour and depth textures for every colour of Ratchet's ship, drawn by an image session from the game's originals
to the standard of the finished Blargian Red atlas, then checked, compiled and installed in the Rift Apart folder.

Usage:
  python -I scripts/model-review/ship_colours.py pack <disc.iso> <installed 0-riftapart> <out dir>
  python -I scripts/model-review/ship_colours.py intake <disc.iso> <installed 0-riftapart> <out dir> <returned zip or folder>...
  python -I scripts/model-review/ship_colours.py install <out dir> <installed 0-riftapart> <backups dir>
  python -I scripts/model-review/ship_colours.py sheet <uya.exe> <disc.iso> <vendor ram.bin> <out dir> [--colours N,...]

The ship's 32 colours are one texture each, the 2FIP images of pairs 350 to 381 of the WAD whose header is at 0x1280 in
the table of contents (`uyafmt::moby::wad_image`, `levelcode::SkinTexture`; the skin number at 0x16C5B0 picks one). The
wings, hull, cockpit and engines all sample it: nothing else the ship draws changes with its colour (a texture dump of
the vendor in red and in green differs in that one texture). All 32 share one UV layout; colour 0 (Blargian Red) is the
finished master: its 1024 art in `replacements/`, its material regions in `regions.tsv`, its normal and ORM maps
(authored from its own generated height) in `pbr.tsv`.

pack writes the image session's batches (BATCHES): per batch a zip of each colour's original at 1024 with hard texels,
the master's original, its finished colour and its depth (`master_depth`), the batch's prompt (`prompt`) and a sheet.
The session returns `ship-NN-colour.png` and `ship-NN-depth.png` for each colour.

intake checks each returned pair as the prompt does (`check`: the colour against its original, the depth against the
master's depth; a pair that fails is named and left out) and compiles a passing one into `<out>/0-riftapart`: the colour as
`replacements/<stem>.png` (RGBA, alpha on the GS scale as the master's), the depth as the normal map
`materials/authored_ship_colour_NN_n.png` (`normal_from_depth`, at the master's relief), and the colour's rows: its
`regions.tsv` row the master's (its region map, as the pieces are the same) and its `pbr.tsv` row its normal map with the
master's ORM map.

install copies the compiled files into the installed folder (backing up what it replaces under <backups dir>) and puts the
rows in its tables, replacing rows of the same stems. It is the step that changes the player's folder; nothing else does.

sheet draws the ship vendor's picture of each colour from a vendor RAM dump (`uya native`, the skin number written at
0x16C5B0 of a copy), with UPDATED TEXTURES off (the original) and on (the new art), side by side.
"""
import argparse
import hashlib
import importlib.util
import io
import json
import re
import shutil
import subprocess
import sys
import time
import zipfile
from pathlib import Path

import cv2
import numpy as np
from PIL import Image, ImageDraw, ImageFont

_here = Path(__file__).resolve().parents[1]
_spec = importlib.util.spec_from_file_location('uya_pcsx2name', _here / 'uya_pcsx2name.py')
pcsx2 = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(pcsx2)

SECTOR = 0x800
TOC_SECTOR, TOC_SECTORS = 1001, 16
SKIN_WAD, SKIN_PAIR, COLOURS = 0x1280, 350, 32
MASTER = 0
SIZE = 1024
GS_OPAQUE = 128
# The PS2's CLUT storage mode 1 order: palette entry i is stored at CSM1[i] (the 8 x 2 blocks of 8 swapped in pairs).
CSM1 = np.array([(i & ~0x18) | ((i & 0x08) << 1) | ((i & 0x10) >> 1) for i in range(256)])


def u32(b, o):
    return int.from_bytes(b[o:o + 4], 'little')


def skins(iso_path):
    """The 32 ship colours: [{n, indices, clut, rgba, stem}], as `uyafmt::moby::wad_image` reads them."""
    out = []
    with open(iso_path, 'rb') as iso:
        def sectors(s, n):
            iso.seek(s * SECTOR)
            return iso.read(n * SECTOR)
        toc = sectors(TOC_SECTOR, TOC_SECTORS)
        base = u32(toc, SKIN_WAD + 4)
        for n in range(COLOURS):
            pair = SKIN_PAIR + n
            o, c = u32(toc, SKIN_WAD + 8 + 8 * pair), u32(toc, SKIN_WAD + 12 + 8 * pair)
            b = sectors(base + o, c)
            if b[:4] != b'2FIP' or u32(b, 0x10) != 0x13 or u32(b, 0x14) != 0:
                sys.exit(f'ship colour {n}: pair {pair} is not a 2FIP image with a 32-bit palette ({b[:4]!r})')
            w, h = u32(b, 8), u32(b, 0xc)
            if (w, h) != (256, 256):
                sys.exit(f'ship colour {n}: {w} x {h}, not 256 x 256')
            clut = np.frombuffer(b[0x20:0x420], np.uint8).reshape(256, 4)[CSM1]
            indices = np.frombuffer(b[0x420:0x420 + w * h], np.uint8).reshape(h, w)
            out.append(dict(n=n, indices=indices, clut=clut, rgba=clut[indices], stem=pcsx2.stem(indices, clut, 0x13)))
    return out


def hard(rgba):
    """A 256 x 256 skin at 1024 with hard texels (RGB)."""
    return Image.fromarray(np.ascontiguousarray(rgba[..., :3])).resize((SIZE, SIZE), Image.Resampling.NEAREST)


DEPTH_SPAN = 100  # grey levels from 128 that the master depth's 0.5 % / 99.5 % relief reaches
DRIFT = 24        # px: broad relief taken out of a depth (Gaussian sigma), which the normal maps do not carry


def master_relief(installed):
    """The master's relief (height in its normal map's units, broad drift out), integrated from its normal map
    (`pbr.tsv`'s normal_map: X in red, Y in green pointing down the atlas) by Frankot-Chellappa; and its span (the larger
    of its 0.5 % and 99.5 % points' distances from 0)."""
    row = table(installed / 'pbr.tsv')[master_stem(installed)]
    n = np.asarray(Image.open(installed / 'materials' / (row['normal_map'] + '.png')).convert('RGB'), np.float64) / 255 * 2 - 1
    nz = np.maximum(n[..., 2], .2)
    p, q = -n[..., 0] / nz, n[..., 1] / nz
    wx, wy = np.meshgrid(np.fft.fftfreq(p.shape[1]) * 2 * np.pi, np.fft.fftfreq(p.shape[0]) * 2 * np.pi)
    den = wx ** 2 + wy ** 2
    den[0, 0] = 1
    z = np.fft.fft2(p) * (-1j * wx) + np.fft.fft2(q) * (-1j * wy)
    z /= den
    z[0, 0] = 0
    z = np.real(np.fft.ifft2(z))
    z -= cv2.GaussianBlur(z, (0, 0), DRIFT)
    lo, hi = np.percentile(z, [.5, 99.5])
    return z, max(-lo, hi)


def master_depth(installed):
    """The master's depth as a grey image: 128 flat, darker recessed, lighter raised, its span at DEPTH_SPAN levels."""
    z, span = master_relief(installed)
    return Image.fromarray(np.clip(128 + z / span * DEPTH_SPAN, 0, 255).astype(np.uint8))


def slopes(depth, span):
    """d(height)/dx and d(height)/dy of a depth image read at the master's relief: its broad drift out, its own span
    scaled to the master's (`span`, `master_relief`)."""
    d = depth.astype(np.float64) - 128
    d -= cv2.GaussianBlur(d, (0, 0), DRIFT)
    lo, hi = np.percentile(d, [.5, 99.5])
    dzdy, dzdx = np.gradient(d / max(-lo, hi) * span)
    return dzdx, dzdy


def relief_gain(installed, mdepth, span):
    """What the slopes of the master's depth image are multiplied by to reach its normal map's: the depth image is
    rounded to grey levels and its drift taken out twice, so its slopes come out a little flatter (1.16 measured)."""
    row = table(installed / 'pbr.tsv')[master_stem(installed)]
    n = np.asarray(Image.open(installed / 'materials' / (row['normal_map'] + '.png')).convert('RGB'), np.float64) / 255 * 2 - 1
    nz = np.maximum(n[..., 2], .2)
    dzdx, dzdy = slopes(mdepth, span)
    return float(np.sqrt(((n[..., 0] / nz) ** 2 + (n[..., 1] / nz) ** 2).mean() / (dzdx ** 2 + dzdy ** 2).mean()))


def normal_from_depth(depth, span, gain):
    """The normal map (RGBA: X in red, Y in green pointing down the atlas, Z in blue, A 255 as the master's) of a depth
    image at the master's relief (`slopes`, `relief_gain`)."""
    dzdx, dzdy = slopes(depth, span)
    dzdx, dzdy = dzdx * gain, dzdy * gain
    n = np.stack([-dzdx, dzdy, np.ones_like(dzdx)], -1)
    n /= np.linalg.norm(n, axis=-1, keepdims=True)
    rgb = np.clip((n + 1) / 2 * 255 + .5, 0, 255).astype(np.uint8)
    return np.dstack([rgb, np.full(dzdx.shape, 255, np.uint8)])


def edges(a, s):
    a = cv2.GaussianBlur(a, (0, 0), s)
    return np.hypot(cv2.Sobel(a, cv2.CV_32F, 1, 0), cv2.Sobel(a, cv2.CV_32F, 0, 1))


def corr(a, b):
    a, b = a - a.mean(), b - b.mean()
    return float((a * b).sum() / np.sqrt((a * a).sum() * (b * b).sum()))


# What a returned pair must reach (the prompt's numbers): measured on the master, whose colour against its original
# scores a shift of 0.3 px, edges 0.95 and a difference of 11; one original texel off scores 4.3 px and 0.79.
LIMITS = dict(colour_shift_px=2.0, colour_edges=0.9, colour_difference=16.0, depth_shift_px=2.0, depth_edges=0.6)


def check(original, colour, depth, master_depth_grey):
    """The prompt's check of one returned pair: original (256 x 256 x 3), colour (1024 x 1024 x 3), depth (1024 x 1024),
    master_depth_grey (1024 x 1024), as float32 arrays. Its numbers and the names of the limits it misses."""
    o = cv2.resize(original.astype(np.float32), (SIZE, SIZE), interpolation=cv2.INTER_NEAREST)
    shift = lambda a, b: float(np.hypot(*cv2.phaseCorrelate(edges(a, 1.5), edges(b, 1.5))[0]))
    small = cv2.resize(colour, (256, 256), interpolation=cv2.INTER_AREA)
    got = dict(colour_shift_px=shift(cv2.cvtColor(o, cv2.COLOR_RGB2GRAY), cv2.cvtColor(colour, cv2.COLOR_RGB2GRAY)),
               colour_edges=corr(edges(original.astype(np.float32).mean(2), 1), edges(small.mean(2), 1)),
               colour_difference=float(np.abs(original.astype(np.float32) - small).mean()),
               depth_shift_px=shift(master_depth_grey, depth),
               depth_edges=corr(edges(master_depth_grey, 1.5), edges(depth, 1.5)))
    misses = [k for k, v in got.items() if (v < LIMITS[k] if k.endswith('edges') else v > LIMITS[k])]
    return got, misses


def table(path):
    lines = [l for l in path.read_text(encoding='utf-8').splitlines() if l and not l.startswith('#')]
    head = lines[0].split('\t')
    return {r[0]: dict(zip(head, r)) for r in (l.split('\t') for l in lines[1:])}


def master_stem(installed):
    stem = '8b45ad96956dc5cb-2f12be7762d79200-00002213'
    for name in ('regions.tsv', 'pbr.tsv'):
        if stem not in table(installed / name):
            sys.exit(f'{installed / name}: no row for the master ({stem})')
    if not (installed / 'replacements' / (stem + '.png')).is_file():
        sys.exit(f'{installed}: no replacements/{stem}.png')
    return stem


# The batches the image session draws, the vendor's colours and the ones the player named first; each colour's look as
# its original shows it, for the prompt.
BATCHES = [
    ('1', [22, 23, 24, 25, 26]),
    ('2', [1, 2, 4, 6, 16]),
    ('3', [3, 5, 7, 8, 9]),
    ('4', [10, 11, 12, 13, 14, 15]),
    ('5', [17, 18, 19, 20, 21]),
    ('6', [27, 28, 29, 30, 31]),
]
LOOKS = {
    1: 'green hull (the red scheme repainted green) with lime-yellow trim and markings, olive parts',
    2: 'dark steel-blue hull with light blue trim and markings, blue-grey parts',
    3: 'black and silver hull with glowing light-blue round lights and a light-blue storm-cloud pattern along the sides',
    4: 'dark navy-blue hull with its panel lines and no bright trim, dark parts',
    5: 'very dark purple-black hull with thin glowing red bars and strips, dark red parts',
    6: 'army camouflage: soft patches of muted greens with small yellow stencil marks, olive parts',
    7: 'cow print: off-white hull with irregular black spots, pink-maroon parts',
    8: 'dusty pink-maroon hull painted with flowers (white daisies, purple and multicoloured flowers, green vines) and two small smiley faces',
    9: 'brown hull painted as a shark: an open mouth with white teeth at the nose, yellow-brown zigzag markings, brown parts',
    10: 'deep blue-purple hull with orange and yellow hot-rod flames outlined in light blue, purple parts',
    11: 'brick-red hull with pink polka dots ringed in green, orange-and-green striped panels, purple parts',
    12: 'battered grey hull with rust stains and scratches, lime-yellow side panels, rust-brown parts',
    13: 'golden-yellow hull with black tiger stripes and a chrome trim stripe, brown parts',
    14: 'grey and white splotchy camouflage with red panels and red stripes, dark grey parts',
    15: 'taxi-yellow hull with black-and-yellow checker stripes and small number plates, grey parts',
    16: 'matte black hull with a thin glowing red stripe along each side, dark grey parts',
    17: 'sweeps from bright lime-yellow through teal into near-black, olive-khaki parts',
    18: 'sweeps from bright yellow into dark purple-black, lilac-grey parts with purple trim',
    19: 'sweeps from violet and light blue into dark navy-purple, blue-grey parts',
    20: 'green hull with yellow lightning-bolt flashes and yellow trim stripes, navy-blue parts',
    21: 'red hull with yellow lightning-bolt flashes and yellow trim stripes, navy-blue parts',
    22: 'deep blue hull with yellow-gold lightning-bolt flashes along its edges, yellow trim stripes and markings, blue-grey parts',
    23: 'green hull shading from bright lime highlights to deep green, with purple base areas and purple trim around the parts',
    24: 'orange-gold hull shading from pale gold to deep red-orange, tan and brown parts',
    25: 'deep violet-to-blue hull with lilac and light-blue highlights, green base areas',
    26: 'pale sea-green hull with teal sweeps and highlights, grey-teal parts',
    27: 'pale blue-grey hull with violet sweeps and white highlights, purple parts',
    28: 'pale sea-green hull with copper-orange sweeps, brown parts',
    29: 'pale mint hull with dark plum-maroon panels and pink trim, maroon parts',
    30: 'light sky-blue hull with deep purple panels, purple parts',
    31: 'bright yellow hull with dark green and navy-blue sweeps and stripes, teal-blue parts',
}


def prompt(name, colours):
    files = '\n'.join(f'  - `ship-{n:02d}.png`: {LOOKS[n]}' for n in colours)
    outs = '\n'.join(f'- `ship-{n:02d}-colour.png` and `ship-{n:02d}-depth.png`' for n in colours)
    k = len(colours)
    return f"""Redraw {k} paint schemes of Ratchet's spaceship from Ratchet & Clank: Up Your Arsenal (PS2) at high resolution, as texture maps: for each one a colour texture and a depth texture. They are UV texture atlases, not pictures: flat pieces of the ship (hull panels, wings, engines, cockpit, guns, seat) laid out on one square sheet, which the game wraps onto the 3D ship. Every piece must stay exactly where it is, at its exact size and shape, or the paint lands on the wrong part of the ship.

**What is in the zip `ship-skins-batch-{name}.zip`.**
- `red-original.png`: the ship's standard red paint scheme, the game's 256 x 256 original enlarged to 1024 x 1024 with hard pixels.
- `red-colour.png`: the finished 1024 x 1024 colour texture made from it. This is the standard to reach: the same layout, every panel line, seam, vent, grille, bolt and marking drawn cleanly and crisply, the paint smooth, the shading and colours of the original kept.
- `red-depth.png`: the finished 1024 x 1024 depth texture of the same ship: mid-grey (128) for flat surfaces, darker where the surface is recessed (seams, vents, grilles, gaps between panels), lighter where it stands out (bolts, raised plates, rims). Painted colours and patterns are not in it.
- One image per paint scheme to redraw, each the game's 256 x 256 original enlarged to 1024 x 1024 with hard pixels:
{files}

All of them share one layout: every scheme is the same ship with the same pieces in the same places; only the paint differs.

**What to make, for each paint scheme.**
1. `ship-NN-colour.png`, 1024 x 1024 RGB: the scheme's own image redrawn at full resolution to the standard of `red-colour.png`.
   - Its own colours and its own paint design exactly as its image shows them: the same patterns, stripes, flames, flowers, spots, sweeps, markings and their shapes, sizes and places. Only the detail gets sharper: crisp edges on the paint design, clean panel lines and seams, small parts made clear.
   - The ship's structure (panel lines, seams, vents, grilles, bolts, rims, the cockpit and engine parts) as `red-colour.png` draws it, in this scheme's colours.
   - Keep the small glowing swatches (the orange engine glow, the lights) and the pure black areas as the scheme's image has them.
   - Do not restyle it: no realism, no new lighting, highlights, shadows, perspective or grunge; nothing added (no new parts, panels, logos, text or effects).
2. `ship-NN-depth.png`, 1024 x 1024 greyscale: the depth of that scheme's ship, made the way `red-depth.png` is: mid-grey 128 for flat surfaces, recesses darker, raised parts lighter, shallow relief only. It is physical relief, not the colour image in grey: the paint design (flames, flowers, stripes, spots, sweeps, colour changes) is flat paint and does not show in it. Where the scheme's ship has the red one's structure, its depth matches `red-depth.png`.

**How.**
- Use the image tool's edit mode with the scheme's own image as the image to keep, so every piece stays put; show it `red-colour.png` (or `red-depth.png` for the depth) as the example of the result.
- Generate each texture on its own, never several in one image.
- If a result moves, resizes, redraws or reshapes any piece, changes the scheme's colours or paint design, or invents detail, throw it away and do it again.

**It must line up with the original.** Check every texture with this Python before you deliver (numpy, OpenCV, Pillow):

```python
import numpy as np, cv2
from PIL import Image
grey = lambda p: np.asarray(Image.open(p).convert('L'), np.float32)
rgb = lambda p: np.asarray(Image.open(p).convert('RGB'), np.float32)
def edges(a, s):
    a = cv2.GaussianBlur(a, (0, 0), s)
    return np.hypot(cv2.Sobel(a, cv2.CV_32F, 1, 0), cv2.Sobel(a, cv2.CV_32F, 0, 1))
def corr(a, b):
    a, b = a - a.mean(), b - b.mean()
    return float((a * b).sum() / np.sqrt((a * a).sum() * (b * b).sum()))
def check(n):
    orig, col, dep = f'ship-{{n:02d}}.png', f'ship-{{n:02d}}-colour.png', f'ship-{{n:02d}}-depth.png'
    shift = lambda a, b: np.hypot(*cv2.phaseCorrelate(edges(a, 1.5), edges(b, 1.5))[0])
    small = lambda p, how: cv2.resize(rgb(p), (256, 256), interpolation=how)
    o, c = small(orig, cv2.INTER_NEAREST), small(col, cv2.INTER_AREA)
    return dict(colour_shift_px=shift(grey(orig), grey(col)),          # must be 2 or less
                colour_edges=corr(edges(o.mean(2), 1), edges(c.mean(2), 1)),  # must be 0.9 or more
                colour_difference=float(np.abs(o - c).mean()),          # must be under 16
                depth_shift_px=shift(grey('red-depth.png'), grey(dep)),  # must be 2 or less
                depth_edges=corr(edges(grey('red-depth.png'), 1.5), edges(grey(dep), 1.5)))  # must be 0.6 or more
```

- `red-colour.png` against `red-original.png` scores a shift of 0.3 px, edges 0.95 and a difference of 11. A colour texture moved by one original pixel (4 px at 1024) scores a shift of 4 and edges 0.79: that fails. A depth texture that is the colour image in grey scores edges about 0.5: that fails.
- If a piece shifted, scaled, rotated or got cropped, fix it or generate again before you deliver. Report each texture's five numbers.

**Deliver.**
- One zip named `ship-skins-batch-{name}-done.zip` holding only these {2 * k} PNGs at its top level, each exactly 1024 x 1024:
{outs}
- A contact sheet PNG: for each scheme, its image beside your colour and depth textures, the file name under each.

Do all {k}.
"""


def pack(iso, installed, out):
    S = skins(iso)
    stem = master_stem(installed)
    if S[MASTER]['stem'] != stem:
        sys.exit(f'colour {MASTER} is {S[MASTER]["stem"]}, not the master {stem}')
    colour = Image.open(installed / 'replacements' / (stem + '.png'))
    if colour.size != (SIZE, SIZE):
        sys.exit(f'the master is {colour.size}, not {SIZE} x {SIZE}')
    shared = {'red-original.png': hard(S[MASTER]['rgba']), 'red-colour.png': colour.convert('RGB'), 'red-depth.png': master_depth(installed)}
    if sorted(n for _, c in BATCHES for n in c) != list(range(1, COLOURS)) or set(LOOKS) != set(range(1, COLOURS)):
        sys.exit('the batches or the looks do not cover colours 1 to 31 once each')
    out.mkdir(parents=True, exist_ok=True)
    for name, colours in BATCHES:
        folder = out / f'batch-{name}'
        folder.mkdir(exist_ok=True)
        images = dict(shared)
        images.update({f'ship-{n:02d}.png': hard(S[n]['rgba']) for n in colours})
        for f, im in images.items():
            im.save(folder / f)
        z = out / f'ship-skins-batch-{name}.zip'
        with zipfile.ZipFile(z, 'w', zipfile.ZIP_DEFLATED) as zf:
            for f in images:
                zf.write(folder / f, f)
        (out / f'prompt-batch-{name}.md').write_text(prompt(name, colours), encoding='utf-8')
        sheet = Image.new('RGB', (len(images) * 260, 290), (24, 24, 28))
        draw = ImageDraw.Draw(sheet)
        for i, (f, im) in enumerate(images.items()):
            sheet.paste(im.convert('RGB').resize((256, 256), Image.Resampling.LANCZOS), (i * 260, 0))
            draw.text((i * 260 + 4, 264), f, fill=(230, 230, 230))
        sheet.save(out / f'batch-{name}-sheet.png')
        print(f'batch {name}: {", ".join(str(n) for n in colours)} -> {z}')
    (out / 'stems.json').write_text(json.dumps({s['n']: s['stem'] for s in S}, indent=1) + '\n')


RETURNED = re.compile(r'ship-(\d\d)-(colour|depth)\.png')


def returned(sources):
    """{file name: bytes} of the PNGs in the returned zips and folders (any folder in them); a name twice stops the run."""
    found = {}
    def add(name, data):
        if name in found:
            sys.exit(f'{name} is returned twice')
        found[name] = data
    for src in sources:
        if src.is_dir():
            for p in sorted(src.rglob('*.png')):
                add(p.name, p.read_bytes())
        elif src.suffix.lower() == '.zip':
            with zipfile.ZipFile(src) as zf:
                for i in zf.infolist():
                    if i.filename.lower().endswith('.png'):
                        add(Path(i.filename).name, zf.read(i))
        else:
            sys.exit(f'{src}: not a folder or a zip')
    return found


def intake(iso, installed, out, sources):
    S = skins(iso)
    master = master_stem(installed)
    if S[MASTER]['stem'] != master:
        sys.exit(f'colour {MASTER} is {S[MASTER]["stem"]}, not the master {master}')
    relief, span = master_relief(installed)
    mdepth = np.clip(128 + relief / span * DEPTH_SPAN, 0, 255).astype(np.uint8).astype(np.float32)
    gain = relief_gain(installed, mdepth, span)
    print(f'relief gain {gain:.3f}')
    files = returned(sources)
    pairs = {}
    for name in files:
        m = RETURNED.fullmatch(name)
        if m:
            n = int(m.group(1))
            if not 1 <= n < COLOURS:
                sys.exit(f'{name}: there is no ship colour {n} to replace (1 to {COLOURS - 1})')
            pairs.setdefault(n, {})[m.group(2)] = files[name]
        else:
            print(f'{name}: not a texture (left out)')
    pack = out / '0-riftapart'
    rows_path = pack / 'rows.json'
    rows = json.loads(rows_path.read_text()) if rows_path.exists() else dict(regions={}, pbr={})
    regions, pbr = table(installed / 'regions.tsv')[master], table(installed / 'pbr.tsv')[master]
    report_path = out / 'intake.json'
    report = json.loads(report_path.read_text()) if report_path.exists() else {}
    failed = []
    for n in sorted(pairs):
        p = pairs[n]
        if set(p) != {'colour', 'depth'}:
            failed.append(n)
            print(f'ship-{n:02d}: only its {", ".join(p)} came back')
            continue
        colour = Image.open(io.BytesIO(p['colour']))
        depth = Image.open(io.BytesIO(p['depth']))
        if colour.size != (SIZE, SIZE) or depth.size != (SIZE, SIZE):
            failed.append(n)
            print(f'ship-{n:02d}: colour {colour.size}, depth {depth.size}, not {SIZE} x {SIZE}')
            continue
        c = np.asarray(colour.convert('RGB'), np.float32)
        d = np.asarray(depth.convert('L'), np.float32)
        got, misses = check(S[n]['rgba'][..., :3], c, d, mdepth)
        report[str(n)] = dict(stem=S[n]['stem'], **{k: round(v, 3) for k, v in got.items()}, misses=misses)
        print(f'ship-{n:02d}: ' + ', '.join(f'{k} {v:.2f}' for k, v in got.items()) + (f'  FAILS {", ".join(misses)}' if misses else '  passes'))
        if misses:
            failed.append(n)
            continue
        stem = S[n]['stem']
        (pack / 'replacements').mkdir(parents=True, exist_ok=True)
        (pack / 'materials').mkdir(parents=True, exist_ok=True)
        rgba = np.dstack([np.asarray(colour.convert('RGB'), np.uint8), np.full((SIZE, SIZE), GS_OPAQUE, np.uint8)])
        Image.fromarray(rgba).save(pack / 'replacements' / (stem + '.png'))
        normal = f'authored_ship_colour_{n:02d}_n'
        Image.fromarray(normal_from_depth(d, span, gain)).save(pack / 'materials' / (normal + '.png'))
        rows['regions'][stem] = dict(regions, stem=stem)
        rows['pbr'][stem] = dict(pbr, stem=stem, normal_map=normal, emissive_map='')
    rows_path.parent.mkdir(parents=True, exist_ok=True)
    rows_path.write_text(json.dumps(rows, indent=1) + '\n')
    report_path.write_text(json.dumps(report, indent=1) + '\n')
    done = sorted(int(k) for k, v in report.items() if not v['misses'] and v['stem'] in rows['pbr'])
    print(f'{len(done)} colours compiled in {pack}: {done}; failed this time: {failed}; '
          f'not returned yet: {[n for n in range(1, COLOURS) if str(n) not in report]}')


def install(out, installed, backups):
    pack = out / '0-riftapart'
    rows = json.loads((pack / 'rows.json').read_text())
    if not rows['pbr']:
        sys.exit(f'{pack}: nothing compiled')
    stamp = time.strftime('%Y%m%d-%H%M%S')
    backup = backups / f'0-riftapart-ship-colours-{stamp}'
    record = dict(date=stamp, backup=str(backup), files=[], tables={})
    files = [f'replacements/{s}.png' for s in rows['pbr']] + [f'materials/{r["normal_map"]}.png' for r in rows['pbr'].values()]
    for rel in files:
        src, dst = pack / rel, installed / rel
        if not src.is_file():
            sys.exit(f'{src}: compiled row without its file')
        if dst.exists():
            (backup / rel).parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(dst, backup / rel)
        shutil.copy2(src, dst)
        record['files'].append(dict(file=rel, sha256=hashlib.sha256(dst.read_bytes()).hexdigest(), replaced=(backup / rel).exists()))
    for name, new in (('regions.tsv', rows['regions']), ('pbr.tsv', rows['pbr'])):
        path = installed / name
        backup.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, backup / name)
        lines = path.read_text(encoding='utf-8').splitlines()
        notes = [l for l in lines if l.startswith('#')]
        body = [l for l in lines if l and not l.startswith('#')]
        fields = body[0].split('\t')
        kept = [l for l in body[1:] if l.split('\t')[0] not in new]
        path.write_text('\n'.join(notes + [body[0]] + kept + ['\t'.join(r[f] for f in fields) for r in new.values()]) + '\n', encoding='utf-8')
        record['tables'][name] = sorted(new)
    (out / f'installed-{stamp}.json').write_text(json.dumps(record, indent=1) + '\n')
    print(f'installed {len(record["files"])} files and {len(rows["pbr"])} colours\' rows in {installed}; backup {backup}')


SKIN_NUMBER = 0x16C5B0
VENDOR_LEVEL = 3
# The player's graphics settings (docs: the sweep's), UPDATED TEXTURES set per picture.
SETTINGS = 'dlss=performance,ambient_occlusion=high,shadows=high,light_shafts=high,water_reflections=high,lighting=per_pixel'


def sheet(uya, iso, ram, hd, out, colours, crop):
    jobs = ram.with_name('jobs' + ram.name[3:])
    if not ram.name.startswith('ram') or not jobs.is_file():
        sys.exit(f'{ram}: a ramNNNNN.bin with its jobsNNNNN.bin beside it')
    base = bytearray(ram.read_bytes())
    runs = out / 'sheet-runs'
    cells = []
    for n in colours:
        d = runs / f'{n:02d}'
        d.mkdir(parents=True, exist_ok=True)
        shutil.copy2(jobs, d / jobs.name)
        base[SKIN_NUMBER:SKIN_NUMBER + 4] = n.to_bytes(4, 'little')
        (d / ram.name).write_bytes(base)
        pics = []
        # A colour drawn both ways by an earlier run (stopped part way) is kept; one drawn only one way is drawn again.
        kept = all((d / f'{mode}.png').is_file() for mode in ('off', 'on'))
        for mode in ('off', 'on'):
            png = d / f'{mode}.png'
            if kept:
                pic = Image.open(png).convert('RGB').crop(crop)
                pics.append(pic.resize((pic.width // 2, pic.height // 2), Image.Resampling.LANCZOS))
                continue
            cmd = [str(uya), 'native', str(iso), str(d / ram.name), str(VENDOR_LEVEL), str(png), '--height', '1440', '--hd', str(hd),
                   '--set', f'{SETTINGS},rift_apart_surfaces={mode}']
            r = subprocess.run(cmd, capture_output=True, text=True, timeout=300)
            (d / f'{mode}.log').write_text(r.stdout + r.stderr)
            if r.returncode or not png.is_file():
                sys.exit(f'colour {n}, UPDATED TEXTURES {mode}: uya native failed (exit {r.returncode}); {d / (mode + ".log")}')
            pic = Image.open(png).convert('RGB').crop(crop)
            pics.append(pic.resize((pic.width // 2, pic.height // 2), Image.Resampling.LANCZOS))
        cells.append((n, pics))
        print(f'colour {n} drawn')
    w, h = pics[0].size
    cols = 2
    rows_n = (len(cells) + cols - 1) // cols
    img = Image.new('RGB', (cols * (2 * w + 30), rows_n * (h + 28)), (20, 20, 24))
    draw = ImageDraw.Draw(img)
    for i, (n, pics) in enumerate(cells):
        r, c = divmod(i, cols)
        x, y = c * (2 * w + 30), r * (h + 28)
        img.paste(pics[0], (x, y + 24))
        img.paste(pics[1], (x + w, y + 24))
        draw.text((x + 4, y + 4), f'colour {n}: original | new HD', fill=(240, 220, 120))
    img.save(out / 'sheet.png')
    print(f'{out / "sheet.png"}')


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest='cmd', required=True)
    n = sub.add_parser('names')
    n.add_argument('iso')
    p = sub.add_parser('pack')
    p.add_argument('iso')
    p.add_argument('installed', type=Path)
    p.add_argument('out', type=Path)
    i = sub.add_parser('intake')
    i.add_argument('iso')
    i.add_argument('installed', type=Path)
    i.add_argument('out', type=Path)
    i.add_argument('sources', type=Path, nargs='+')
    s = sub.add_parser('install')
    s.add_argument('out', type=Path)
    s.add_argument('installed', type=Path)
    s.add_argument('backups', type=Path)
    h = sub.add_parser('sheet')
    h.add_argument('uya', type=Path)
    h.add_argument('iso', type=Path)
    h.add_argument('ram', type=Path)
    h.add_argument('hd', type=Path, help="the texture_replacements folder the picture draws (read only)")
    h.add_argument('out', type=Path)
    h.add_argument('--colours', default=','.join(str(n) for n in range(COLOURS)))
    h.add_argument('--crop', default='760,300,1980,820', help="x0,y0,x1,y1 of the 2560 x 1440 picture (the vendor's ship)")
    args = ap.parse_args()
    if args.cmd == 'names':
        for s in skins(args.iso):
            print(s['n'], s['stem'])
    elif args.cmd == 'pack':
        pack(args.iso, args.installed, args.out)
    elif args.cmd == 'intake':
        intake(args.iso, args.installed, args.out, args.sources)
    elif args.cmd == 'install':
        install(args.out, args.installed, args.backups)
    elif args.cmd == 'sheet':
        sheet(args.uya, args.iso, args.ram, args.hd, args.out, [int(x) for x in args.colours.split(',')],
              tuple(int(x) for x in args.crop.split(',')))


if __name__ == '__main__':
    main()
