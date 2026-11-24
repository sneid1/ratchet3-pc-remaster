"""Rift Apart materials on this game's surfaces (plan item 9.2) by the method the player approved, for the port's RIFT APART
SURFACES.

Usage:
  python -I scripts/riftapart-pair.py write <matches.tsv> <out dir> [--store DIR] [--export DIR] [--surfaces CSV]
      The Rift Apart folder the port draws (`uya native --rift DIR`, `uya play --rift DIR`, or the profile's
      texture_replacements\\0-riftapart once a set is installed): <out dir>/surfaces.tsv from the matches, and
      <out dir>/materials, each matched material's map, written once into the store (default <out dir>/materials) and
      skipped when it is there; a store apart from <out dir> is linked in as <out dir>/materials (a junction), so test
      folders share one store.
  python -I scripts/riftapart-pair.py card <out dir> <stem> ... [--profile DIR] [--surfaces CSV]
      A texture's card for the matching: the texture as the game draws it (the pack's, else the game's own), one and two by
      two; where it is (each level's gameplay frame with the surface's pixels marked, from the measurement's id pictures in
      captures/surfaces-all); its label, levels, share of the picture and world size per repeat.
  python -I scripts/riftapart-pair.py preview <stem> <out.png> <material:metres[:grain]> ... [--profile DIR]
      [--export DIR] [--surfaces CSV]
      The approved method on that texture with that material, as the port's grain is drawn: the texture times the
      material's grain, tiled at the material's real size, one repeat of the texture and two by two, and one tile of the
      material at Rift Apart's own resolution (up close at 4K), beside the texture as it is. The relief and gloss are the
      port's lighting and are not in it.
  python -I scripts/riftapart-pair.py regions <stem> <out.png> [--k K]
  python -I scripts/riftapart-pair.py region-preview <stem> <out.png> <N=MATERIAL@METRES,N=-,...> [--k K]
      A character's atlas (fur, cloth, leather, metal and skin in one texture) matched by region: `regions` splits it into its
      K colour regions (k-means in CIELAB from a fixed start, numbered by size) and draws them; `region-preview` lays each
      region's material on it at its real size. The rows go to textures/riftapart-regions.tsv (stem, shows, k, assign), which
      `write` turns into the folder's regions.tsv and regions/<stem>.png (the region map: up to four materials, one a channel).
  python -I scripts/riftapart-pair.py sheets <folder of the export> <out dir> [--export DIR]
      Contact sheets of every material in that folder (the export's `more` has none of its own): its grain on a plain surface
      and its normal map, named. A detail normal (no albedo or gloss of its own) is a material too: its grain is its cavities.

<matches.tsv> (textures/riftapart-matches.tsv; names only, nothing of Rift Apart's) has a row per texture matched by hand
(PLAN 9.2, the project’s handover notes "Matching"): stem, shows (what the surface is), material, tile_m (the world size of one tile of
the material in metres, judged from what it shows), grain (the grain's power: 1, Rift Apart's own contrast, as approved),
normal (the normal map's strength: 1, its own), runner_up, flag (`none` where nothing in Rift Apart fits, `label` where the
kind label is wrong; a row flagged `none` has no material and is left out). Lines starting with # are notes.

surfaces.tsv, a row per surface: its stem, material, tiles along u and v (n = round(repeat in metres / tile metres), at least
one, so that a tile never breaks at the texture's own wrap), the square root of the material's reflectance at normal
incidence (the mean of its _g's green: 56 of 255 for the plain materials, 0.048 squared), grain and normal, as the port reads
them (port/uya/src/texnames.rs, SURFACES_HEADER).

materials/<material>.png, its map at its own size (2048 x 2048), all linear: red and green the normal map's X and Y (Y up),
blue the gloss (Rift Apart's _g red, or slice 1 of a 4-slice _c, which holds the same), alpha the grain: the albedo's
luminance times its cavities (the normal map's z: its slopes and cracks darker from any light) over the mean, held between
0.25 and 2.5 (a hole or a glint does not blacken or burn out the texture), 1.0 at 102. The albedo's colour is not used: the
surface keeps its own (port/uya/src/lighting.rs, MATERIAL). A bare metal (chromium, silver, steel, brass: its albedo black,
its colour in its _g) has a grain of 1: a clean polish; its reflection is the renderer's, by its metalness.

<export dir> (default C:\\riftapart-export) is the player's Rift Apart as PNG (`scripts/riftapart.py surfaces DIR --export`,
and its `more` folder), never committed. <surfaces.csv> (default docs/surfaces-objects.csv, `scripts/uya-surfaces-objects.py`; docs/surfaces.csv without the objects) is `scripts/uya-surfaces.py`'s list: per
texture its PCSX2 name, levels, share of the picture, size, world size of one repeat along u and v in game units, mean CIELAB
colour and kind; its first line gives the game units per metre.
"""
import argparse
import csv
import importlib.util
import os
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont

_here = Path(__file__).resolve().parent
ROOT = _here.parent
_spec = importlib.util.spec_from_file_location('uya_texnames', _here / 'uya_texnames.py')
names = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(names)

LUMA = np.array([0.2126, 0.7152, 0.0722])
GRAIN = (0.25, 2.5)
# The map's alpha of a grain of 1.0.
GRAIN_ONE = 102
MAP_SIZE = 2048
MATCH_COLUMNS = ['stem', 'shows', 'material', 'tile_m', 'grain', 'normal', 'runner_up', 'flag']
SURFACES_HEADER = 'stem\tmaterial\ttiles_u\ttiles_v\treflectance\tgrain\tnormal\tspecular\tmetal'
# regions.tsv: a surface of regions' stem, region map and normal's strength, then per material (four, `-` and zeros for one it
# has not) its name, tiles, reflectance's root, grain's power and specular's strength (port/uya/src/texnames.rs, REGIONS_HEADER).
REGIONS_HEADER = 'stem\tmap\tnormal\t' + '\t'.join(
    f'material{j or ""}\ttiles_u{j or ""}\ttiles_v{j or ""}\treflectance{j or ""}\tgrain{j or ""}\tspecular{j or ""}\tfur{j or ""}\tcatchlight{j or ""}\tmetal{j or ""}'
    for j in range(4))
# Fur in a region's assignment (`N=fur:MM@SIZE`: strands MM millimetres long, 32 of them across SIZE metres; the renderer's
# shells, port/uya/src/objects.rs fs_fur) has no map; its reflectance is hair's (F0 0.04).
FUR = 'fur'
FUR_REFLECTANCE = 0.2
# The materials' looks set in the game by the main session (material, specular: the strength of its highlight; metal: its
# metalness, 0 to 1); a material not listed: specular 1, metal 0.
MATERIAL_LOOKS = ROOT / 'textures' / 'riftapart-material-looks.tsv'
PROFILE = Path(os.environ['USERPROFILE']) / 'Games' / 'Ratchet and Clank 3'


def font(size):
    return ImageFont.truetype(r'C:\Windows\Fonts\arial.ttf', size)


_index = {}
# A detail normal (Rift Apart's `textures\detailmaps`: a normal map made to be laid over another material, with no albedo or
# gloss of its own): its grain is its cavities alone, its gloss 0 and its reflectance the plain materials' (56 of 255).
DETAIL_REFLECTANCE = 56 / 255


def materials_index(export):
    """{material: [its _c, or its _n where it has no _c]} over the export (its sheets left out)."""
    if export not in _index:
        idx = defaultdict(list)
        for p in Path(export).rglob('*_c.png'):
            if 'sheets' not in p.parts:
                idx[p.name[:-6]].append(p)
        for p in Path(export).rglob('*_n.png'):
            if 'sheets' not in p.parts and p.name[:-6] not in idx:
                idx[p.name[:-6]].append(p)
        _index[export] = idx
    return _index[export]


def find_material(export, material):
    """(albedo, normal, gloss) files of the material in the export (one folder holds them; albedo and gloss None for a
    detail normal), or the reason it is no material (a texture with no normal map or no gloss: a decal, a glow, a sign)."""
    found = materials_index(export).get(material, [])
    if len(found) != 1:
        return f'{export}: material {material} found {len(found)} times'
    if found[0].name.endswith('_n.png'):
        return None, found[0], None
    d = found[0].parent
    n = d / f'{material}_n.png'
    if not n.exists():
        n = d / f'{material}_c_slice3.png'
    if not n.exists():
        return f'{d}: {material} has no normal map (_n or _c slice 3)'
    g = d / f'{material}_g.png'
    if not g.exists():
        g = d / f'{material}_c_slice1.png'
    if not g.exists():
        return f'{d}: {material} has no gloss map (_g or _c slice 1)'
    return found[0], n, g


def material_files(export, material):
    f = find_material(export, material)
    if isinstance(f, str):
        sys.exit(f)
    return f


def material(export, name, size):
    """The material at `size` x `size`: its map (uint8 RGBA: the normal's X and Y, the gloss, the grain) and its reflectance's
    root (0 to 1)."""
    c, n, g = material_files(export, name)
    if c is None:
        albedo = np.full((size, size, 3), 128, np.float32)
    else:
        albedo = np.asarray(Image.open(c).convert('RGB').resize((size, size), Image.LANCZOS), np.float32)
    nm = np.asarray(Image.open(n).convert('RGB').resize((size, size), Image.LANCZOS))
    if g is None:
        gm = np.dstack([np.zeros((size, size), np.uint8), np.full((size, size), 56, np.uint8), np.zeros((size, size), np.uint8)])
    else:
        gm = np.asarray(Image.open(g).convert('RGB').resize((size, size), Image.LANCZOS))
    xy = nm[..., :2].astype(np.float32) / 127.5 - 1
    z = np.sqrt(np.clip(1 - (xy ** 2).sum(-1), 0, 1))
    rgb = np.clip(albedo * (z / max(float(z.mean()), 1e-3))[..., None], 0, 255)
    y = rgb @ LUMA
    if y.mean() < 8:
        # A bare metal (chromium, silver, polished steel): its albedo is black, its colour its reflectance (the gloss map's
        # green); it has no cavities in its albedo, and a clean polish has none: a grain of 1.
        grain = np.ones_like(y)
    else:
        grain = np.clip(y / y.mean(), *GRAIN)
    if c is None:
        # A detail normal has no albedo, and its normals' tilt alone (z) is nearly flat: its cavities are taken from its relief
        # (`relief_cavities`), at its own resolution.
        grain = np.asarray(Image.fromarray(relief_cavities(Image.open(n).convert('RGB'))).resize((size, size), Image.BICUBIC), np.float32)
        grain = np.clip(grain, *GRAIN)
    alpha = np.clip(grain * GRAIN_ONE + 0.5, 0, 255).astype(np.uint8)
    return np.dstack([nm[..., 0], nm[..., 1], gm[..., 0], alpha]), float(gm[..., 1].mean()) / 255


def relief_cavities(normal_image):
    """A detail normal's cavities as a grain (1.0 its mean): its height, integrated from the normals' slopes over the tiling map
    (Frankot-Chellappa, in the frequency domain), less the height blurred over a 64th of the map, over that radius: a pit or crease
    as deep as it is wide darkens by one, a ridge lightens. On level 3's brute the detail normals' grain went from 0.016 (their z)
    to 0.05 to 0.19 as seen, and the skin's wrinkles and the vest's weave showed (15 November 2026)."""
    nm = np.asarray(normal_image, np.float32)
    x = nm[..., 0] / 127.5 - 1
    y = nm[..., 1] / 127.5 - 1
    z = np.sqrt(np.clip(1 - x * x - y * y, 0.05, 1))
    gx, gy = -x / z, y / z
    h, w = gx.shape
    ky = np.fft.fftfreq(h)[:, None] * 2 * np.pi
    kx = np.fft.fftfreq(w)[None, :] * 2 * np.pi
    d = kx ** 2 + ky ** 2
    d[0, 0] = 1
    height = np.real(np.fft.ifft2((-1j * kx * np.fft.fft2(gx) - 1j * ky * np.fft.fft2(gy)) / d))
    sigma = max(2.0, w / 64)
    blur = np.real(np.fft.ifft2(np.fft.fft2(height) * np.exp(-0.5 * d * sigma ** 2)))
    grain = np.clip(1 + (height - blur) / sigma, *GRAIN)
    return (grain / grain.mean()).astype(np.float32)


def reflectance(export, name):
    _, _, g = material_files(export, name)
    if g is None:
        return DETAIL_REFLECTANCE
    return float(np.asarray(Image.open(g).convert('RGB').resize((256, 256), Image.BILINEAR))[..., 1].mean()) / 255


def read_surfaces(path):
    """(units per metre, {stem: row})."""
    lines = Path(path).read_text(encoding='utf-8').splitlines()
    if not lines or not lines[0].startswith('# units per metre '):
        sys.exit(f'{path}: the first line is "# units per metre N ..."')
    upm = float(lines[0].split()[4])
    rows = list(csv.DictReader(lines[1:]))
    need = ['name', 'levels', 'share', 'width', 'height', 'repeat_u', 'repeat_v', 'L', 'a', 'b', 'kind']
    if not rows or list(rows[0].keys()) != need:
        sys.exit(f'{path}: columns {list(rows[0].keys()) if rows else "none"}, expected {need}')
    return upm, {r['name']: r for r in rows}


def read_matches(path):
    lines = [l for l in Path(path).read_text(encoding='utf-8').splitlines() if l and not l.startswith('#')]
    if not lines or lines[0].split('\t') != MATCH_COLUMNS:
        sys.exit(f'{path}: the header is not {MATCH_COLUMNS}')
    rows, seen = [], set()
    for n, line in enumerate(lines[1:], 2):
        f = line.split('\t')
        if len(f) != len(MATCH_COLUMNS):
            sys.exit(f'{path}: row {n} has {len(f)} columns: {line!r}')
        r = dict(zip(MATCH_COLUMNS, f))
        if r['stem'] in seen:
            sys.exit(f'{path}: {r["stem"]} has two rows')
        seen.add(r['stem'])
        rows.append(r)
    return rows


def tiles(repeat_m, metres):
    return max(1, round(repeat_m / metres))


def write(args):
    upm, surf = read_surfaces(args.surfaces)
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    store = Path(args.store) if args.store else out / 'materials'
    store.mkdir(parents=True, exist_ok=True)
    link = out / 'materials'
    if store.resolve() != link.resolve():
        if link.exists():
            sys.exit(f'{link} is there and is not the store {store}')
        subprocess.run(['cmd', '/c', 'mklink', '/J', str(link), str(store.resolve())], check=True, capture_output=True)
    rows = []
    for m in read_matches(args.matches):
        if m['flag'] in ('none', 'character') or not m['material']:
            continue
        s = surf.get(m['stem'])
        if s is None:
            sys.exit(f'{args.surfaces}: no row for {m["stem"]}')
        if names.parse(m['stem'] + '.png') is None:
            sys.exit(f'{args.matches}: {m["stem"]} is no PCSX2 replacement name')
        if m['tile_m'].startswith('x'):
            # A texture whose world size no frame measured (an object never drawn in a saved frame): the matching gives the
            # tiles across one repeat itself (`xN`), judged from what the texture shows.
            n = int(m['tile_m'][1:])
            rows.append((m['stem'], m['material'], n, n, float(m['grain']), float(m['normal'])))
            continue
        if not s['repeat_u'] or not s['repeat_v']:
            sys.exit(f'{args.surfaces}: {m["stem"]} has no repeat: its coordinates are not a repeat (give tile_m as xN)')
        metres = float(m['tile_m'])
        rows.append((m['stem'], m['material'], tiles(float(s['repeat_u']) / upm, metres), tiles(float(s['repeat_v']) / upm, metres),
                     float(m['grain']), float(m['normal'])))
    looks = {}
    for n, line in enumerate(MATERIAL_LOOKS.read_text(encoding='utf-8').splitlines(), 1):
        if line.startswith('#') or line == 'material\tspecular\tmetal':
            continue
        f = line.split('\t')
        if len(f) != 3:
            sys.exit(f'{MATERIAL_LOOKS}:{n}: {line!r}')
        looks[f[0]] = (float(f[1]), float(f[2]))
    refl = {}
    for mat in sorted({r[1] for r in rows}):
        f = store / f'{mat}.png'
        if not f.exists():
            mp, refl[mat] = material(args.export, mat, MAP_SIZE)
            Image.fromarray(mp, 'RGBA').save(f)
            print(f'{mat}: map written, {MAP_SIZE} x {MAP_SIZE}', flush=True)
        else:
            refl[mat] = reflectance(args.export, mat)
    with open(out / 'surfaces.tsv', 'w', newline='\n', encoding='utf-8') as f:
        f.write(f'# Rift Apart materials on this game\'s surfaces from {args.matches} (scripts/riftapart-pair.py; the player\'s own Rift Apart, never committed)\n')
        f.write(SURFACES_HEADER + '\n')
        for stem, mat, tu, tv, grain, normal in rows:
            spec, metal = looks.get(mat, (1.0, 0.0))
            f.write(f'{stem}\t{mat}\t{tu}\t{tv}\t{refl[mat]:.4f}\t{grain:g}\t{normal:g}\t{spec:g}\t{metal:g}\n')
    print(f'{len(rows)} surfaces, {len(refl)} materials: {out}')
    write_regions(args, upm, surf, out, store, looks)
    write_lights(args, out)
    write_water(args, out)


# A light's look (`lights.tsv`, port/uya/src/texnames.rs LIGHTS_HEADER): its texel at this many times its own colour whatever
# light falls on it, and its glow, the share of the frame's alpha above 0x80 it writes (the game's glow pass spreads what is
# above 0x80: level 6's hangar tubes).
LIGHT_BOOST = 1.15
LIGHT_GLOW = 0.6
# The water's ripples (`water_n.png`, port/uya/src/reflections.rs): Rift Apart's tiling water normal.
WATER_NORMAL = 'kal_plastic_water_01'


def write_lights(args, out):
    """`lights.tsv`: every texture the matches flag `light` (a lamp's panel, a strip, neon, a screen) with its boost and glow."""
    rows = [m['stem'] for m in read_matches(args.matches) if m['flag'] == 'light']
    with open(out / 'lights.tsv', 'w', newline='\n', encoding='utf-8') as f:
        f.write(f'# The textures that are lights, from {args.matches} (scripts/riftapart-pair.py; never committed)\n')
        f.write('stem\tboost\tglow\n')
        for stem in rows:
            f.write(f'{stem}\t{LIGHT_BOOST:g}\t{LIGHT_GLOW:g}\n')
    print(f'{len(rows)} lights: {out / "lights.tsv"}')


def write_water(args, out):
    """`water_n.png`: Rift Apart's water normal (WATER_NORMAL's `_n`), as it is."""
    _, n, _ = material_files(args.export, WATER_NORMAL)
    Image.open(n).convert('RGB').save(out / 'water_n.png')
    print(f'water: {out / "water_n.png"} from {n}')


def write_regions(args, upm, surf, out, store, looks):
    """`regions.tsv` and `regions/<stem>.png` from REGIONS (the characters' atlases matched by region): per row the texture's
    colour regions (`clusters`) each given a material, at most four materials (a channel of the region map each: red the
    first, green the second, blue the third, alpha the fourth; a region given none has no weight)."""
    if not REGIONS.exists():
        return
    (out / 'regions').mkdir(exist_ok=True)
    lines = [l for l in REGIONS.read_text(encoding='utf-8').splitlines() if l and not l.startswith('#')]
    if lines[0].split('\t') != REGION_COLUMNS:
        sys.exit(f'{REGIONS}: the header is not {REGION_COLUMNS}')
    rows = []
    for line in lines[1:]:
        r = dict(zip(REGION_COLUMNS, line.split('\t')))
        if len(line.split('\t')) != len(REGION_COLUMNS):
            sys.exit(f'{REGIONS}: {line!r}')
        s = surf.get(r['stem'])
        if s is None:
            sys.exit(f'{args.surfaces}: {r["stem"]} has no row')
        labels, k = drawn_clusters(r['stem'], int(r['k']), args.profile, out)
        assign, discs = parse_assign(r['assign'], k)
        labels = apply_discs(labels, k, discs)
        # A slot per distinct assignment: one material at two sizes, or with and without a catchlight (Clank's glossy coat
        # on his body and on his lens), takes two slots.
        mats = []
        for m in assign.values():
            if m and m not in mats:
                mats.append(m)
        if not mats:
            continue
        if len(mats) > 4:
            sys.exit(f'{REGIONS}: {r["stem"]} gives {len(mats)} materials (at most four)')
        weights = np.zeros(labels.shape + (4,), np.uint8)
        for c, m in assign.items():
            if m:
                weights[labels == c, mats.index(m)] = 255
        Image.fromarray(weights, 'RGBA').save(out / 'regions' / f'{r["stem"]}.png')
        ru, rv = (float(s['repeat_u']) / upm, float(s['repeat_v']) / upm) if s['repeat_u'] else (None, None)
        for mat, *_ in mats:
            f = store / f'{mat}.png'
            if mat != FUR and not f.exists():
                mp, _ = material(args.export, mat, MAP_SIZE)
                Image.fromarray(mp, 'RGBA').save(f)
        cols = []
        for j in range(4):
            if j < len(mats):
                m, size, fur_mm, catch = mats[j]
                refl = FUR_REFLECTANCE if m == FUR else reflectance(args.export, m)
                cols += [m, str(region_tiles(ru, size, r['stem'])), str(region_tiles(rv, size, r['stem'])),
                         f'{refl:.4f}', '1', f'{looks.get(m, (1.0, 0.0))[0]:g}', f'{fur_mm / 1000 * upm:.5f}', f'{catch:g}',
                         f'{looks.get(m, (1.0, 0.0))[1]:g}']
            else:
                cols += ['-', '0', '0', '0', '0', '0', '0', '0', '0']
        rows.append([r['stem'], f'{r["stem"]}.png', r['normal']] + cols)
    with open(out / 'regions.tsv', 'w', newline='\n', encoding='utf-8') as f:
        f.write(f'# Rift Apart materials by region on this game\'s characters from {REGIONS} (scripts/riftapart-pair.py; never committed)\n')
        f.write(REGIONS_HEADER + '\n')
        for row in rows:
            f.write('\t'.join(row) + '\n')
    print(f'{len(rows)} surfaces of regions: {out / "regions.tsv"}')


# The characters' atlases matched by region (names only): stem, what it is, how many colour regions (`clusters`), and each
# region's material: `N=MATERIAL@METRES` or `N=-` (no material), comma-separated.
REGIONS = ROOT / 'textures' / 'riftapart-regions.tsv'
REGION_COLUMNS = ['stem', 'shows', 'k', 'assign', 'normal']


def region_tiles(repeat_m, size, stem):
    """Tiles across one repeat for a region's material: size is its tile in metres, or xN (N tiles) for an atlas no frame measured."""
    if size.startswith('x'):
        return int(size[1:])
    if repeat_m is None:
        sys.exit(f'{stem}: no repeat measured: give the region tile as xN')
    return tiles(repeat_m, float(size))


def parse_assign(text, k):
    """({region: (material, metres, fur's millimetres, catchlight) or None}, discs) for regions 1 to k; every region must be
    given. A region is `N=MATERIAL@SIZE`, `N=MATERIAL@SIZE+catch` (an eye: a catchlight of 1), `N=fur:MM@SIZE` or `N=-`. A
    disc of the atlas, `disc(X;Y;R)=...` (its centre and radius as shares of the atlas's width and height), is a region of its
    own over the colour regions (an eye whose pupil, iris and shine fall in three colour regions): region k + 1 for the first,
    and so on, with the disc's (x, y, r) in `discs`."""
    out, discs = {}, []
    for part in text.split(','):
        n, _, m = part.partition('=')
        if n.startswith('disc(') and n.endswith(')'):
            x, y, rad = (float(v) for v in n[5:-1].split(';'))
            discs.append((x, y, rad))
            n = k + len(discs)
        else:
            n = int(n)
            if not 1 <= n <= k or n in out:
                sys.exit(f'{text!r}: region {n} (1 to {k}, once each)')
        if m == '-':
            out[n] = None
        else:
            m, plus, extra = m.partition('+')
            if plus and extra != 'catch':
                sys.exit(f'{text!r}: {part!r}: +catch is the only suffix')
            mat, _, size = m.partition('@')
            if not size.startswith('x'):
                float(size)
            mat, colon, mm = mat.partition(':')
            if (mat == FUR) != bool(colon) or (colon and float(mm) <= 0):
                sys.exit(f'{text!r}: {part!r}: fur is fur:MM@SIZE, MM above 0')
            out[n] = (mat, size, float(mm) if colon else 0.0, 1.0 if plus else 0.0)
    if sorted(out) != list(range(1, k + 1 + len(discs))):
        sys.exit(f'{text!r}: every region 1 to {k} needs its material or -')
    return out, discs


def apply_discs(labels, k, discs):
    """The colour regions with each disc of `parse_assign` made region k + 1, k + 2, ... (its opaque texels)."""
    h, w = labels.shape
    yy, xx = np.mgrid[0:h, 0:w]
    labels = labels.copy()
    for j, (x, y, rad) in enumerate(discs):
        inside = ((xx + 0.5) / w - x) ** 2 + ((yy + 0.5) / h - y) ** 2 <= rad ** 2
        labels[inside & (labels > 0)] = k + 1 + j
    return labels


def original_file(stem, profile):
    """The game's own picture of the texture (its dump: the profile's texture_dumps, else captures/rift/dumps-all)."""
    for p in (Path(profile) / 'texture_dumps' / f'{stem}.png', ROOT / 'captures' / 'rift' / 'dumps-all' / f'{stem}.png'):
        if p.exists():
            return p
    sys.exit(f'{stem}: no dump of it')


def clusters(stem, k, profile):
    """The texture's k colour regions: each opaque texel's region 1 to k (0: cut out), k-means in CIELAB from a fixed start
    (the same regions every run), the regions numbered by their share, largest first."""
    labels, _ = _kmeans(stem, k, profile)
    return labels, k


def drawn_clusters(stem, k, profile, rift):
    """The texture's k colour regions (`clusters`) at the size the port draws it with the Rift Apart folder `rift` (its own
    `replacements/` picture, else the pack's, else the game's): a picture larger than the game's
    (a 1024 x 1024 replacement of a 256 x 256 atlas) has its own region map, each of its texels in the region whose colour is
    nearest among the regions of the game's texel under it and that texel's eight neighbours, then each texel given the
    region most of its 3 x 3 neighbourhood has (a lone texel of a stroke takes its surroundings'). The region map the game's
    size gave changed materials in blocks of the scale's texels across a face, and its edges, the game's, missed the pack's.
    A translucent texel of the game's (its alpha above the cut-out's 32 and below TRANSLUCENT, the GS's 128 being opaque: a
    visor's tinted glass) takes no region: an opaque material's metal and gloss on Ratchet's visor (steel, the dark plating's
    colour) turned the glass he is seen through grey and dark."""
    labels, centres = _kmeans(stem, k, profile)
    alpha = np.asarray(Image.open(original_file(stem, profile)).convert('RGBA'))[..., 3]
    glass = (alpha > 32) & (alpha < TRANSLUCENT)
    drawn = Path(rift) / 'replacements' / f'{stem}.png'
    if not drawn.exists():
        drawn = texture_file(stem, profile)
    hd = np.asarray(Image.open(drawn).convert('RGBA'), np.float32)
    (h, w), (H, W) = labels.shape, hd.shape[:2]
    if (H, W) == (h, w):
        labels[glass] = 0
        return labels, k
    if W % w or H % h or W // w != H // h:
        sys.exit(f'{stem}: {drawn} is {W} x {H}, not a whole multiple of the game\'s {w} x {h}')
    s = W // w
    # The regions each game texel's 3 x 3 neighbourhood has (region 0, cut out, never a choice).
    onehot = np.zeros((h + 2, w + 2, k + 1), bool)
    onehot[1:-1, 1:-1][np.arange(h)[:, None], np.arange(w)[None, :], labels] = True
    near = np.zeros((h, w, k + 1), bool)
    for dy in range(3):
        for dx in range(3):
            near |= onehot[dy:dy + h, dx:dx + w]
    near = np.repeat(np.repeat(near[..., 1:], s, 0), s, 1)
    dist = ((_lab(hd[..., :3].reshape(-1, 3)).reshape(H, W, 1, 3) - centres.reshape(1, 1, k, 3)) ** 2).sum(-1)
    dist[~near] = np.inf
    hd_labels = np.where(np.isfinite(dist.min(-1)), dist.argmin(-1) + 1, 0)
    hd_labels[hd[..., 3] <= 32] = 0
    counts = np.zeros((H, W, k + 1), np.int32)
    padded = np.pad(hd_labels, 1)
    for dy in range(3):
        for dx in range(3):
            counts[np.arange(H)[:, None], np.arange(W)[None, :], padded[dy:dy + H, dx:dx + W]] += 1
    counts[..., 0] = 0
    out = counts.argmax(-1)
    out[hd_labels == 0] = 0
    out[np.repeat(np.repeat(glass, s, 0), s, 1)] = 0
    return out.astype(np.int32), k


# The game's alpha below which a texel is drawn see-through (the GS's 128 is opaque; a little under it is still opaque paint).
TRANSLUCENT = 120


def _kmeans(stem, k, profile):
    """`clusters`' regions of the game's texture and their mean colours in CIELAB, in the regions' order (region j + 1's at j)."""
    sys.path.insert(0, str(_here))
    a = np.asarray(Image.open(original_file(stem, profile)).convert('RGBA'), np.float32)
    opaque = a[..., 3] > 32
    px = a[..., :3][opaque]
    lab = _lab(px)
    rng = np.random.default_rng(0)
    centres = [lab[rng.integers(len(lab))]]
    for _ in range(1, k):
        d = np.min([((lab - c) ** 2).sum(1) for c in centres], axis=0)
        centres.append(lab[int(np.argmax(d))])
    centres = np.array(centres)
    for _ in range(30):
        lbl = np.argmin(((lab[:, None, :] - centres[None]) ** 2).sum(2), 1)
        centres = np.array([lab[lbl == j].mean(0) if (lbl == j).any() else centres[j] for j in range(k)])
    order = np.argsort(-np.bincount(lbl, minlength=k))
    rank = np.empty(k, int)
    rank[order] = np.arange(1, k + 1)
    labels = np.zeros(opaque.shape, np.int32)
    labels[opaque] = rank[lbl]
    by_rank = np.empty_like(centres)
    by_rank[rank - 1] = centres
    return labels, by_rank


def _lab(rgb):
    c = rgb / 255.0
    c = np.where(c > 0.04045, ((c + 0.055) / 1.055) ** 2.4, c / 12.92)
    xyz = c @ np.array([[0.4124, 0.2126, 0.0193], [0.3576, 0.7152, 0.1192], [0.1805, 0.0722, 0.9505]]) / [0.95047, 1.0, 1.08883]
    f = np.where(xyz > 0.008856, np.cbrt(xyz), 7.787 * xyz + 16 / 116)
    return np.stack([116 * f[:, 1] - 16, 500 * (f[:, 0] - f[:, 1]), 200 * (f[:, 1] - f[:, 2])], 1)


REGION_COLOURS = [(0, 0, 0), (230, 25, 75), (60, 180, 75), (255, 225, 25), (0, 130, 200), (245, 130, 48), (145, 30, 180),
                  (70, 240, 240), (240, 50, 230), (210, 245, 60)]


def regions_cmd(args):
    """A character's atlas split into its colour regions, for the matching: the texture, its regions in distinct colours with
    their numbers, and each region alone with its share of the opaque texels."""
    labels, k = clusters(args.stem, args.k, args.profile)
    tex = Image.open(original_file(args.stem, args.profile)).convert('RGB')
    h, w = labels.shape
    s = max(1, 384 // max(w, h))
    big = tex.resize((w * s, h * s), Image.NEAREST)
    col = np.array(REGION_COLOURS)[labels].astype(np.uint8)
    cm = Image.fromarray(col).resize((w * s, h * s), Image.NEAREST)
    d0 = ImageDraw.Draw(cm)
    for c in range(1, k + 1):
        ys, xs = np.nonzero(labels == c)
        if len(xs):
            d0.text((int(np.median(xs)) * s, int(np.median(ys)) * s), str(c), fill=(255, 255, 255), font=font(18), stroke_width=2, stroke_fill=(0, 0, 0))
    cell = 192
    sheet = Image.new('RGB', (max(2 * w * s + 36, 4 * (cell + 10) + 12), 60 + h * s + 20 + ((k + 3) // 4) * (cell + 30)), (24, 24, 26))
    d = ImageDraw.Draw(sheet)
    d.text((12, 10), f'{args.stem}: {k} colour regions (numbered by size); assign each a material or -', fill=(235, 235, 235), font=font(17))
    sheet.paste(big, (12, 50))
    sheet.paste(cm, (24 + w * s, 50))
    total = max(1, int((labels > 0).sum()))
    a = np.asarray(tex)
    for c in range(1, k + 1):
        m = labels == c
        im = np.where(m[..., None], a, (a * 0.15).astype(np.uint8))
        x, y = 12 + ((c - 1) % 4) * (cell + 10), 70 + h * s + ((c - 1) // 4) * (cell + 30)
        sheet.paste(Image.fromarray(im).resize((cell, cell), Image.NEAREST), (x, y))
        d.text((x, y + cell + 4), f'region {c}: {m.sum() / total * 100:.0f}% of the texels', fill=tuple(REGION_COLOURS[c]), font=font(14))
    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    sheet.save(args.out)
    print(args.out)


def region_preview(args):
    """The atlas as it is beside it with each region's material grain at its real size (`assign` as in REGIONS), and a close
    view of its middle at 4x."""
    upm, surf = read_surfaces(args.surfaces)
    s = surf.get(args.stem)
    if s is None:
        sys.exit(f'{args.surfaces}: {args.stem} has no row')
    labels, k = clusters(args.stem, args.k, args.profile)
    assign, discs = parse_assign(args.assign, k)
    labels = apply_discs(labels, k, discs)
    tex = Image.open(texture_file(args.stem, args.profile)).convert('RGB')
    size = 768
    t = np.asarray(tex.resize((size, size), Image.BICUBIC), np.float32)
    lab = np.asarray(Image.fromarray(labels.astype(np.uint8)).resize((size, size), Image.NEAREST))
    g = np.ones((size, size), np.float32)
    ru, rv = (float(s['repeat_u']) / upm, float(s['repeat_v']) / upm) if s['repeat_u'] else (None, None)
    lines = []
    for c, m in assign.items():
        if not m:
            continue
        if m[0] == FUR:
            lines.append(f'region {c}: fur {m[2]:g} mm, 32 strands across {m[1]} (drawn by the renderer\'s shells, not here)')
            continue
        mp, _ = material(args.export, m[0], MAP_SIZE)
        grain = mp[..., 3].astype(np.float32) / GRAIN_ONE
        tu, tv = region_tiles(ru, m[1], args.stem), region_tiles(rv, m[1], args.stem)
        gt = np.asarray(Image.fromarray(np.tile(grain, (tv, tu))).resize((size, size), Image.LANCZOS), np.float32)
        g = np.where(lab == c, gt, g)
        lines.append(f'region {c}: {m[0]} at {m[1]} ({tu} x {tv} tiles), contrast as seen {seen_contrast(grain):.3f}')
    after = np.clip(t * g[..., None] + 0.5, 0, 255).astype(np.uint8)
    q = size // 4
    close = Image.fromarray(after).crop((size // 2 - q // 2, size // 2 - q // 2, size // 2 + q // 2, size // 2 + q // 2)).resize((size, size), Image.LANCZOS)
    sheet = Image.new('RGB', (3 * size + 40, size + 60 + 20 * len(lines)), (24, 24, 26))
    d = ImageDraw.Draw(sheet)
    d.text((10, 8), f'{args.stem}: one repeat is ' + (f'{ru:.2f} x {rv:.2f} m' if ru else 'not measured') + '; the atlas as it is | with the regions\' materials | its middle at 4x', fill=(235, 235, 235), font=font(16))
    for j, ln in enumerate(lines):
        d.text((10, 32 + 20 * j), ln, fill=(170, 170, 170), font=font(14))
    y = 40 + 20 * len(lines)
    for j, im in enumerate((Image.fromarray(t.astype(np.uint8)), Image.fromarray(after), close)):
        sheet.paste(im, (10 + j * (size + 10), y))
    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    sheet.save(args.out)
    print(args.out)


_packs = {}


def texture_file(stem, profile):
    """The file the port draws for the texture: the pack's (the profile's texture_replacements as `texnames::open` scans it:
    entries in sorted order, folders where they sort, the first file of a key kept, the Rift Apart folder left out), else the
    game's own (texture_dumps)."""
    if profile not in _packs:
        files = {}
        rep = Path(profile) / 'texture_replacements'

        def scan(d):
            for e in sorted(d.iterdir(), key=lambda e: e.name):
                if e.is_dir():
                    if e != rep / '0-riftapart':
                        scan(e)
                    continue
                key = names.parse(e.name)
                if key is not None and e.suffix.lower() == '.png' and key not in files:
                    files[key] = e
        scan(rep)
        _packs[profile] = files
    best = _packs[profile].get(names.parse(stem + '.png'))
    if best is None:
        best = Path(profile) / 'texture_dumps' / f'{stem}.png'
        if not best.exists():
            # The textures `uya native --dump-textures` wrote drawing every gameplay dump (the objects' among them).
            best = ROOT / 'captures' / 'rift' / 'dumps-all' / f'{stem}.png'
        if not best.exists():
            sys.exit(f'{stem}: in neither the packs nor the dumps of {profile}, nor captures/rift/dumps-all')
    return best


_measured = {}


def measured():
    """Per measured gameplay frame (captures/surfaces-all): (level, frame picture, {stem: pixels}, {pixels: stems}, id
    picture path)."""
    if not _measured:
        for tsv in sorted((ROOT / 'captures/surfaces-objects').glob('f*-*.tsv')):
            rows = [l.split('\t') for l in tsv.read_text().splitlines()[2:]]
            level = int(tsv.stem.split('-')[1])
            frame = next((ROOT / f'captures/gameplay/L{level:02d}').glob(f'L{level:02d}-f*.png'), None)
            by_count = defaultdict(list)
            for r in rows:
                by_count[int(r[4])].append(r[0])
            _measured[tsv] = (level, frame, {r[0]: int(r[4]) for r in rows}, by_count, tsv.with_suffix('.png'), {})
    return _measured


def frames_of(stem):
    """[(level, gameplay frame path, mask of the surface's pixels)] from captures/surfaces-all, most pixels first. An id's
    stem is found by its pixel count in the measurement's list; a count two stems share marks none."""
    out = []
    for tsv, (level, frame, px, by_count, idpath, cache) in measured().items():
        count = px.get(stem, 0)
        if count == 0 or frame is None or len(by_count[count]) != 1:
            continue
        if 'ids' not in cache:
            idp = np.asarray(Image.open(idpath).convert('RGB'))
            g, b = idp[..., 1].astype(np.int32), idp[..., 2].astype(np.int32)
            cache['ids'] = np.where((b == 0x5A) & ((g & 0xF0) == 0xA0), idp[..., 0].astype(np.int32) | ((g & 0xF) << 8), 0)
            found, counts = np.unique(cache['ids'][cache['ids'] > 0], return_counts=True)
            cache['by_px'] = defaultdict(list)
            for i, c in zip(found.tolist(), counts.tolist()):
                cache['by_px'][c].append(i)
        hit = cache['by_px'].get(count, [])
        if len(hit) != 1:
            continue
        out.append((count, level, frame, cache['ids'] == hit[0]))
    out.sort(key=lambda x: -x[0])
    return [(lv, fr, m) for _, lv, fr, m in out]


def card(args):
    upm, surf = read_surfaces(args.surfaces)
    for stem in args.stems:
        s = surf.get(stem)
        if s is None:
            sys.exit(f'{args.surfaces}: no row for {stem}')
        one_card(stem, s, upm, args.profile, Path(args.out) / f'{stem}.png')


def one_card(stem, s, upm, profile, out):
    src = texture_file(stem, profile)
    tex = Image.open(src).convert('RGB')
    frames = frames_of(stem)[:2]
    W = 512 + 16 + 960 + 24
    H = 60 + max(512 + 40 + 256, len(frames) * (540 + 30)) + 20
    sheet = Image.new('RGB', (W, max(H, 900)), (24, 24, 26))
    d = ImageDraw.Draw(sheet)
    ru, rv = (float(s['repeat_u']) / upm, float(s['repeat_v']) / upm) if s['repeat_u'] else (None, None)
    size = f'{ru:.2f} x {rv:.2f} m a repeat' if ru else 'no repeat (its coordinates are not a repeat)'
    d.text((12, 10), f'{stem}   label {s["kind"]}   levels {s["levels"]}   {float(s["share"]) * 100:.2f}% of the gameplay pictures', fill=(235, 235, 235), font=font(18))
    d.text((12, 34), f'{s["width"]} x {s["height"]} texels in the game; {size}; drawn from {src.parent.name}', fill=(170, 170, 170), font=font(15))
    big = tex.resize((512, 512), Image.NEAREST if tex.width <= 256 else Image.LANCZOS)
    sheet.paste(big, (12, 60))
    d.text((12, 60 + 514), 'the texture, one repeat', fill=(170, 170, 170), font=font(14))
    two = Image.new('RGB', (256, 256))
    h2 = tex.resize((128, 128), Image.LANCZOS)
    for j in range(4):
        two.paste(h2, ((j % 2) * 128, (j // 2) * 128))
    sheet.paste(two, (12, 60 + 540))
    d.text((12 + 262, 60 + 540), 'two by two\n(its own seams)', fill=(170, 170, 170), font=font(14))
    x0 = 12 + 512 + 16
    if not frames:
        d.text((x0, 60), 'not in any measured gameplay frame (or its pixel count is shared)', fill=(220, 120, 120), font=font(16))
    for k, (level, frame, mask) in enumerate(frames):
        f = np.asarray(Image.open(frame).convert('RGB').resize((960, 540), Image.LANCZOS), np.float32)
        m = np.asarray(Image.fromarray(mask.astype(np.uint8) * 255).resize((960, 540), Image.NEAREST)) > 0
        edge = m & ~(np.roll(m, 1, 0) & np.roll(m, -1, 0) & np.roll(m, 1, 1) & np.roll(m, -1, 1))
        f[~m] *= 0.45
        f[edge] = (255, 0, 255)
        y = 60 + k * 570
        sheet.paste(Image.fromarray(f.astype(np.uint8)), (x0, y))
        d.text((x0, y + 542), f'level {level}, gameplay frame {frame.name}: the surface lit, the rest dimmed, outlined magenta', fill=(170, 170, 170), font=font(14))
    out.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(out)
    print(out, flush=True)


# The grain contrast of the material the player approved on Veldin's cliff (`seen_contrast` of cliff_003): in the game's frames
# (15 November 2026) a grain much below it hardly showed (painted steels at 0.012 to 0.016), and leaf atlases at 0.16 to 0.36
# laid a noise of small leaves over the painted ones.
SEEN_APPROVED = 0.031


def seen_contrast(grain):
    """The grain's contrast as the game shows it: its standard deviation with one tile at 256 x 256 (the size a tile spans on
    the screen from the game's camera; detail finer than that is filtered away)."""
    return float(np.asarray(Image.fromarray(grain.astype(np.float32)).resize((256, 256), Image.BOX)).std())


def preview(args):
    """The texture as it is, then per candidate (MATERIAL:METRES[:GRAIN]) the texture times its grain at its real size: one
    repeat, two by two, and one tile of the material at half Rift Apart's resolution on the texture's own texels under it."""
    upm, surf = read_surfaces(args.surfaces)
    s = surf.get(args.stem)
    if s is None:
        sys.exit(f'{args.surfaces}: {args.stem} has no row')
    if not s['repeat_u'] and not args.repeat:
        sys.exit(f'{args.stem}: no repeat measured: give --repeat M, your estimate of one repeat in metres (the row then takes tile_m xN)')
    src = texture_file(args.stem, args.profile)
    tex = Image.open(src).convert('RGB')
    ru, rv = (float(s['repeat_u']) / upm, float(s['repeat_v']) / upm) if s['repeat_u'] else (args.repeat, args.repeat)
    cell = 480
    t = np.asarray(tex.resize((cell, cell), Image.BICUBIC), np.float32)
    rows = [('the texture as it is', Image.fromarray(t.astype(np.uint8)), None, None)]
    for cand in args.candidates:
        parts = cand.split(':')
        if len(parts) not in (2, 3):
            sys.exit(f'{cand}: a candidate is MATERIAL:METRES[:GRAIN]')
        mat, metres = parts[0], float(parts[1])
        power = float(parts[2]) if len(parts) == 3 else 1.0
        f = find_material(args.export, mat)
        if isinstance(f, str):
            rows.append((f'{mat}: not usable ({f.split(": ", 1)[-1]})', None, None, None))
            continue
        mp, _ = material(args.export, mat, MAP_SIZE)
        grain = (mp[..., 3].astype(np.float32) / GRAIN_ONE) ** power
        seen = seen_contrast(grain)
        tu, tv = tiles(ru, metres), tiles(rv, metres)
        g = np.asarray(Image.fromarray(np.tile(grain, (tv, tu))).resize((cell, cell), Image.LANCZOS), np.float32)
        one = Image.fromarray(np.clip(t * g[..., None] + 0.5, 0, 255).astype(np.uint8))
        two = Image.new('RGB', (cell, cell))
        small = one.resize((cell // 2, cell // 2), Image.LANCZOS)
        for j in range(4):
            two.paste(small, ((j % 2) * cell // 2, (j // 2) * cell // 2))
        # One tile of the material on the texels of the texture under it (its first tile).
        ct = np.asarray(tex.crop((0, 0, max(1, round(tex.width / tu)), max(1, round(tex.height / tv)))).resize((cell, cell), Image.BICUBIC), np.float32)
        cg = np.asarray(Image.fromarray(grain).resize((cell * 2, cell * 2), Image.LANCZOS).crop((0, 0, cell, cell)), np.float32)
        close = Image.fromarray(np.clip(ct[:cell, :cell] * cg[..., None] + 0.5, 0, 255).astype(np.uint8))
        rows.append((f'{mat} at {metres:g} m a tile ({tu} x {tv} tiles a repeat), grain power {power:g}; grain contrast as seen {seen:.3f} '
                     f'(the approved cliff_003 {SEEN_APPROVED}: below about 0.03 it barely shows in the game; leaf atlases at 0.16-0.36 made noise)', one, two, close))
    W = 3 * cell + 4 * 10
    sheet = Image.new('RGB', (W, 56 + len(rows) * (cell + 30)), (24, 24, 26))
    d = ImageDraw.Draw(sheet)
    d.text((10, 8), f'{args.stem} ({s["kind"]}): one repeat is {ru:.2f} x {rv:.2f} m; drawn from {src.parent.name}', fill=(235, 235, 235), font=font(17))
    d.text((10, 32), 'per row: one repeat | two by two | one material tile up close (quarter of the tile at half its resolution); grain only, the relief and gloss are the port\'s lighting', fill=(160, 160, 160), font=font(13))
    for k, (label, one, two, close) in enumerate(rows):
        y = 56 + k * (cell + 30)
        d.text((10, y), f'{k}. {label}' if k else label, fill=(235, 235, 235), font=font(15))
        for j, im in enumerate((one, two, close)):
            if im is not None:
                sheet.paste(im, (10 + j * (cell + 10), y + 24))
    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    sheet.save(args.out)
    print(args.out)


def sheets(args):
    """Contact sheets of every material under `args.folder` of the export: each as its grain looks on a plain mid-grey
    surface (its albedo times its cavities, two by two tiles: what the approved method multiplies in) beside its normal map,
    with its name and folder; 24 to a sheet, in folder order."""
    root = Path(args.export)
    base = root / args.folder
    every = sorted((p for m, ps in materials_index(args.export).items() for p in ps if base in p.parents),
                   key=lambda p: (str(p.parent).lower(), p.name.lower()))
    mats, not_materials = [], []
    for p in every:
        f = find_material(args.export, p.name[:-6])
        (not_materials if isinstance(f, str) else mats).append(p if not isinstance(f, str) else f)
    for why in not_materials:
        print(f'not a material: {why}')
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    cols, rows, cell = 6, 4, 230
    per = cols * rows
    tag = args.folder.replace('\\', '_').replace('/', '_')
    for page in range(0, len(mats), per):
        chunk = mats[page:page + per]
        sheet = Image.new('RGB', (cols * (cell * 2 + 24) + 12, rows * (cell + 52) + 50), (24, 24, 26))
        d = ImageDraw.Draw(sheet)
        d.text((12, 12), f'Rift Apart: {args.folder} ({page + 1}-{page + len(chunk)} of {len(mats)}): grain (albedo x cavities, 2 x 2 tiles) and normal', fill=(230, 230, 230), font=font(20))
        for i, p in enumerate(chunk):
            name = p.name[:-6]
            c, n, _ = material_files(args.export, name)
            half = cell // 2
            nm = Image.open(n).convert('RGB').resize((half, half), Image.LANCZOS)
            al = np.full((half, half, 3), 128, np.float32) if c is None else np.asarray(Image.open(c).convert('RGB').resize((half, half), Image.LANCZOS), np.float32)
            xy = np.asarray(nm, np.float32)[..., :2] / 127.5 - 1
            z = np.sqrt(np.clip(1 - (xy ** 2).sum(-1), 0, 1))
            g = np.clip(al * (z / max(float(z.mean()), 1e-3))[..., None], 0, 255)
            x, y = 12 + (i % cols) * (cell * 2 + 24), 50 + (i // cols) * (cell + 52)
            sheet.paste(Image.fromarray(np.tile(g, (2, 2, 1)).astype(np.uint8)), (x, y))
            sheet.paste(nm.resize((cell, cell), Image.LANCZOS), (x + cell + 4, y))
            d.text((x, y + cell + 4), name if c is not None else f'{name} (detail normal)', fill=(235, 235, 235), font=font(13))
            d.text((x, y + cell + 22), str(p.parent.relative_to(root))[-60:], fill=(140, 140, 140), font=font(11))
        f = out / f'{tag}_{page // per + 1}.png'
        sheet.save(f)
        print(f)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    sub = ap.add_subparsers(dest='cmd', required=True)
    common = argparse.ArgumentParser(add_help=False)
    common.add_argument('--surfaces', default=str(ROOT / 'docs/surfaces-objects.csv'))
    common.add_argument('--export', default=r'C:\riftapart-export')
    common.add_argument('--profile', default=str(PROFILE), help="the profile whose packs and dumps are read (never written)")
    p = sub.add_parser('write', parents=[common])
    p.add_argument('matches')
    p.add_argument('out')
    p.add_argument('--store', help='the folder the materials are written to once and shared from')
    p = sub.add_parser('card', parents=[common])
    p.add_argument('out', help='the folder the cards go to, one <stem>.png each')
    p.add_argument('stems', nargs='+')
    p = sub.add_parser('preview', parents=[common])
    p.add_argument('stem')
    p.add_argument('out')
    p.add_argument('candidates', nargs='+', help='MATERIAL:METRES[:GRAIN], up to three')
    p.add_argument('--repeat', type=float, help='for a texture no frame measured: one repeat in metres, estimated')
    p = sub.add_parser('sheets', parents=[common])
    p.add_argument('folder', help='a folder of the export, e.g. more\\textures\\environment')
    p.add_argument('out')
    p = sub.add_parser('regions', parents=[common])
    p.add_argument('stem')
    p.add_argument('out')
    p.add_argument('--k', type=int, default=6)
    p = sub.add_parser('region-preview', parents=[common])
    p.add_argument('stem')
    p.add_argument('out')
    p.add_argument('assign', help='N=MATERIAL@METRES or N=- for every region 1 to k, comma-separated')
    p.add_argument('--k', type=int, default=6)
    args = ap.parse_args()
    {'write': write, 'card': card, 'preview': preview, 'sheets': sheets, 'regions': regions_cmd, 'region-preview': region_preview}[args.cmd](args)


if __name__ == '__main__':
    main()
