"""Every texture the game draws, which of them the remaster has redrawn, and the priority queue of the rest, packed in
batches for the image session (a web session) to redraw as a colour and a depth texture each; what comes back is checked,
compiled and installed in the Rift Apart folder.

Usage:
  python -I scripts/model-review/texture_queue.py list <uya.exe> <disc.iso> <installed 0-riftapart> <dumps dir> <review dir> <out dir>
  python -I scripts/model-review/texture_queue.py pack <out dir> <first batch>[-<last batch>]
  python -I scripts/model-review/texture_queue.py generate <out dir> <first batch>[-<last batch>] [--workers N]
  python -I scripts/model-review/texture_queue.py intake <out dir> [--generated] [<returned zip or folder>...]
  python -I scripts/model-review/texture_queue.py install <out dir> <installed 0-riftapart> <backups dir>

list makes the full list, <out>/textures.tsv, one row a texture (its PCSX2 name with bit 14 clear, as texnames.rs reads a
pack file's name). Where the textures come from:
- each story level's scene (terrain, sky, shrubs, ties, mobies, effects, region 0x1A quads): `uya texnames <disc> <level>
  <out>/levels/NN --write` writes them as the game names them; a level the disc has no LEVEL file for, and the multiplayer
  levels, are not story levels;
- <dumps dir>, the textures the game drew in play (texnames.rs `dump`);
- the old texture review's catalogues in <review dir> (captures/rift-texture-review, kept on the share since 20 November):
  the NPC catalogue's textures and source export (cutscene classes), the gear queue's equipment, the NPC image queue's
  atlases and the level priority queue;
- the HUD's and menus' pictures (riftapart-icons.py) and the ship's 32 paints (ship_colours.py).
Each texture's state:
- replaced: the Rift Apart folder's replacements/ has its redrawn picture;
- generated: the old level queue's art for it (colour and height) is in <review dir> and was never installed: intake
  --generated checks it as it checks the image session's;
- flat: nothing for a redraw to add (`flat`): at most 16 texels a side, or no detail;
- no original: no picture of it in any source above;
- queued: the rest, in the queue.
Its other columns: its size and format, sources, story levels, its largest share of a level's gameplay picture and kind
(docs/surfaces-objects.csv), its alpha (`alpha_kind`), whether a Rift Apart material (surfaces.tsv, regions.tsv, pbr.tsv),
a relief map (relief.tsv) or a fan pack draws on it, and the old queues' description.

The queue (<out>/queue.json) ranks the queued textures by share of the picture: first those with at least 0.2 % of a level's
picture (the project's world-texture threshold), then the rest seen in play, then those never measured (cutscene classes,
gear, textures not on screen in the measured frames); within each, the larger share, then the more levels, then the name.
Each gets an id (q00001, ...) and a batch of BATCH once and keeps them: a later list marks done what was installed since
and gives new textures ids and batches after the last.

pack writes each batch's zip for the session (<out>/batches/texture-batch-NNN.zip), its prompt (prompt-batch-NNN.md) and a
sheet: per texture its original enlarged to SIZE on its long side with hard texels (qNNNNN.png, RGB), where it has
transparency its alpha (qNNNNN-alpha.png, white where it is drawn), and where the same picture in another palette is
already redrawn, that art (qNNNNN-variant.png). The session returns qNNNNN-colour.png and qNNNNN-depth.png.

generate does the same through OpenRouter's Image API (OPENROUTER_API_KEY; MODEL draws the colour, DEPTH_MODEL the depth
from it): each texture's colour until it passes `check`, then its depth, at most TRIES times each, into <out>/returned
(every attempt in <out>/attempts, a line a texture in <out>/generate.log with its cost); no texture starts with less than
its most cost (TRY_COST a try) for each worker left, read from the account before each texture, so balance added during a run
is used. intake <out> <out>/returned then takes them as it takes the session's.

intake checks each returned pair (`check`, the prompt's numbers) and compiles a passing one into <out>/0-riftapart: the
colour as replacements/<name>.png (alpha the original's, enlarged smoothly; colours under transparent texels filled from the
nearest drawn ones so that filtering brings in no foreign rim), the depth as the relief map relief/<name>.png
(relief_maps.py's `relief`), and the relief row in rows.json. A pair that fails is named and left out.

install copies the compiled files into the installed folder (backing up what it replaces under <backups dir>) and puts the
rows in its relief.tsv, replacing rows of the same names. It is the step that changes the player's folder.
"""
import argparse
import base64
import csv
import hashlib
import importlib.util
import io
import json
import math
import os
import re
import shutil
import struct
import subprocess
import threading
import sys
import time
import urllib.error
import urllib.request
import zipfile
from collections import Counter
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

import cv2
import numpy as np
from PIL import Image, ImageDraw

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
import relief_maps  # noqa: E402
import ship_colours  # noqa: E402

_spec = importlib.util.spec_from_file_location('riftapart_icons', ROOT / 'scripts/riftapart-icons.py')
icons = importlib.util.module_from_spec(_spec)

STORY_LEVELS = range(1, 39)
SIZE = 1024          # the long side of a redrawn texture
BATCH = 8            # textures a batch
GS_OPAQUE = 128
TIER_SHARE = 0.002   # the project's world-texture threshold: 0.2 % of a level's gameplay picture (uya-surfaces-objects.py)
FLAT_SIDE = 16       # textures this small or smaller have nothing for a redraw to add
FLAT_DETAIL = 1.0    # mean luminance step between neighbouring drawn texels (0-255) below which a texture has no detail
NAME = re.compile(r'([0-9a-f]{1,16})-([0-9a-f]{1,16})-([0-9a-f]{1,8})\.png', re.I)
PSM = {0x00: 'CT32', 0x01: 'CT24', 0x02: 'CT16', 0x0a: 'CT16S', 0x13: 'T8', 0x14: 'T4', 0x1b: 'T8H', 0x24: 'T4HL', 0x2c: 'T4HH'}


def canon(file_name):
    """The canonical name of a PCSX2-named PNG (bit 14 of the bits clear, as texnames.rs `of_file`), or None."""
    m = NAME.fullmatch(file_name)
    if not m:
        return None
    return f'{int(m[1], 16):x}-{int(m[2], 16):x}-{int(m[3], 16) & ~(1 << 14):08x}'


def size_of(name):
    bits = int(name.split('-')[2], 16)
    return 1 << ((bits >> 6) & 15), 1 << ((bits >> 10) & 15), PSM.get(bits & 63, f'psm{bits & 63:02x}')


def names_in(folder):
    """{canonical name: path} of the PCSX2-named PNGs in a folder (not its subfolders)."""
    out = {}
    for p in sorted(Path(folder).iterdir()):
        n = canon(p.name)
        if n:
            out.setdefault(n, p)
    return out


def table(path):
    lines = [l for l in path.read_text(encoding='utf-8').splitlines() if l and not l.startswith('#')]
    head = lines[0].split('\t')
    return {canon(r[0] + '.png'): dict(zip(head, r)) for r in (l.split('\t') for l in lines[1:])}


def level_scenes(uya, iso, out):
    """{name: [story levels]} and {name: path of its picture} of every story level's scene textures (`uya texnames`)."""
    levels, pictures = {}, {}
    for level in STORY_LEVELS:
        folder = out / 'levels' / f'{level:02d}'
        folder.mkdir(parents=True, exist_ok=True)
        r = subprocess.run([str(uya), 'texnames', str(iso), str(level), str(folder), '--write'], capture_output=True, text=True, timeout=600)
        said = (r.stdout + r.stderr).strip().splitlines()
        if r.returncode != 0:
            if said and re.search(rf'no LEVEL file for level {level}$|level {level} is a multiplayer level$', said[-1]):
                folder.rmdir()
                continue
            sys.exit(f'uya texnames level {level}: exit {r.returncode}: {said[-3:]}')
        for n, p in names_in(folder).items():
            levels.setdefault(n, []).append(level)
            pictures.setdefault(n, p)
    return levels, pictures


def review_sources(review):
    """The old texture review's textures: {name: set of sources}, {name: picture path}, {name: description},
    {name: tiling (True/False)} and {name: (colour.png, height.png)} of the level queue's art."""
    src, pictures, about, tiling, generated = {}, {}, {}, {}, {}
    def add(n, s):
        if n:
            src.setdefault(n, set()).add(s)
    cat = json.loads((review / 'npc-catalogue/full/catalogue.json').read_text(encoding='utf-8'))
    for e in cat['entries']:
        for t in e.get('textures', []):
            n = canon(t + '.png')
            add(n, 'catalogue')
            if n and e.get('name'):
                about.setdefault(n, e['name'])
    for n, p in names_in(review / 'npc-catalogue/source-export/tex').items():
        add(n, 'catalogue')
        pictures.setdefault(n, p)
    gear = json.loads((review / 'authored/gear-texture-priority-queue.json').read_text(encoding='utf-8'))
    for r in gear['requests']:
        n = canon(r['id'] + '.png')
        add(n, 'gear')
        ref = review.parent.parent / r['reference']
        if ref.is_file():
            pictures.setdefault(n, ref)
        about.setdefault(n, 'player equipment (weapon, gadget or armour) atlas')
    npc = json.loads((review / 'authored/imagegen-queue/queue.json').read_text(encoding='utf-8'))
    for r in npc['requests']:
        for a in r['atlases']:
            add(canon(a + '.png'), 'npc queue')
    lq = json.loads((review / 'authored/texture-priority-queue.json').read_text(encoding='utf-8'))
    for r in lq['requests'] + lq['coverage']:
        n = canon(r['id'] + '.png')
        add(n, 'level queue')
        if r.get('description'):
            about.setdefault(n, r['description'])
        elif r.get('catalogue_identities'):
            about.setdefault(n, ', '.join(r['catalogue_identities']))
        if 'tiling' in r:
            tiling[n] = bool(r['tiling'])
    for r in lq['requests']:
        d = review / 'authored' / ('noncomic' if r['scope'] == 'noncomic' else 'vidcomic') / r['id']
        if (d / 'colour.png').is_file() and (d / 'height.png').is_file():
            generated[canon(r['id'] + '.png')] = (d / 'colour.png', d / 'height.png')
    return src, pictures, about, tiling, generated


def ui_names(iso):
    """Every name a HUD, menu, weapon, item or vendor picture is drawn by (riftapart-icons.py `names`)."""
    if not hasattr(icons, 'names'):
        _spec.loader.exec_module(icons)
    pictures = {}
    for f in (icons.hud_pictures, icons.panel_pictures, icons.weapon_pictures, icons.item_pictures, icons.store_pictures):
        pictures.update(f(str(iso)))
    return {canon(s + '.png') for p in pictures.values() for s in icons.names(p)}


def read_rgba(path):
    return np.asarray(Image.open(path).convert('RGBA'))


def alpha_kind(a):
    """'opaque' (every texel at least GS opaque), 'cutout' (only clear or opaque texels) or 'graded'."""
    if (a >= GS_OPAQUE).all():
        return 'opaque'
    if ((a == 0) | (a >= GS_OPAQUE)).all():
        return 'cutout'
    return 'graded'


def detail(rgba):
    """Mean luminance step between neighbouring texels both drawn (alpha above 0), 0-255."""
    lum = rgba[..., :3].astype(np.float32) @ np.array([0.299, 0.587, 0.114], np.float32)
    seen = rgba[..., 3] > 0
    steps = [np.abs(np.diff(lum, axis=ax))[np.logical_and(np.take(seen, range(seen.shape[ax] - 1), ax), np.take(seen, range(1, seen.shape[ax]), ax))] for ax in (0, 1)]
    steps = np.concatenate(steps)
    return float(steps.mean()) if steps.size else 0.0


def shares(path):
    lines = path.read_text(encoding='utf-8').splitlines()
    return {canon(r['name'] + '.png'): r for r in csv.DictReader(lines[1:])}


COLUMNS = ['name', 'width', 'height', 'format', 'state', 'id', 'batch', 'share', 'kind', 'levels', 'sources', 'alpha',
           'detail', 'material', 'relief', 'fan_pack', 'tiling', 'about', 'picture', 'variant']


def list_(uya, iso, installed, dumps, review, out):
    out.mkdir(parents=True, exist_ok=True)
    levels, pictures = level_scenes(uya, iso, out)
    print(f'{len(levels)} textures in the scenes of {len(set(l for v in levels.values() for l in v))} story levels')
    sources = {n: {'level'} for n in levels}
    for n, p in names_in(dumps).items():
        sources.setdefault(n, set()).add('drawn in play')
        pictures.setdefault(n, p)
    rsrc, rpics, about, tiling, generated = review_sources(review)
    for n, s in rsrc.items():
        sources.setdefault(n, set()).update(s)
    for n, p in rpics.items():
        pictures.setdefault(n, p)
    ui = ui_names(iso)
    for n in ui:
        sources.setdefault(n, set()).add('ui')
    for s in ship_colours.skins(str(iso)):
        sources.setdefault(canon(s['stem'] + '.png'), set()).add('ship paint')
    replaced = names_in(installed / 'replacements')
    material = set(table(installed / 'surfaces.tsv')) | set(table(installed / 'regions.tsv')) | set(table(installed / 'pbr.tsv'))
    relief = set(table(installed / 'relief.tsv'))
    fan = set()
    for pack in sorted(installed.parent.iterdir()):
        if pack.is_dir() and pack != installed:
            fan |= {canon(p.name) for p in pack.rglob('*.png')} - {None}
    share = shares(ROOT / 'docs/surfaces-objects.csv')
    # Old art that intake --generated checked and failed is drawn again: back in the queue.
    checked = json.loads((out / 'intake.json').read_text()) if (out / 'intake.json').exists() else {}
    generated = {n: v for n, v in generated.items() if not checked.get(n, {}).get('misses')}
    rows = {}
    for n in sorted(sources):
        w, h, fmt = size_of(n)
        sh = share.get(n)
        row = dict(name=n, width=w, height=h, format=fmt, id='', batch='', share=float(sh['share']) if sh else 0.0,
                   kind=sh['kind'] if sh else '', levels=','.join(map(str, levels.get(n, []))), sources=','.join(sorted(sources[n])),
                   alpha='', detail='', material='yes' if n in material else '', relief='yes' if n in relief else '',
                   fan_pack='yes' if n in fan else '', tiling={True: 'yes', False: 'no'}.get(tiling.get(n), ''),
                   about=about.get(n, ''), picture=str(pictures.get(n, '')))
        if n in pictures:
            rgba = read_rgba(pictures[n])
            row['alpha'], row['detail'] = alpha_kind(rgba[..., 3]), round(detail(rgba), 2)
        if n in replaced:
            row['state'] = 'replaced'
        elif n in generated:
            row['state'] = 'generated'
        elif n not in pictures:
            row['state'] = 'no original'
        elif max(w, h) <= FLAT_SIDE or row['detail'] < FLAT_DETAIL:
            row['state'] = 'flat'
        else:
            row['state'] = 'queued'
        rows[n] = row
    # The redrawn art of the same picture in another palette (the same level-0 hash), for the session to match.
    art_by_tex0 = {}
    for n, p in replaced.items():
        art_by_tex0.setdefault(n.split('-')[0], str(p))
    for r in rows.values():
        r['variant'] = art_by_tex0.get(r['name'].split('-')[0], '') if r['state'] == 'queued' else ''
    (out / 'generated.json').write_text(json.dumps({n: [str(c), str(h)] for n, (c, h) in generated.items() if rows[n]['state'] == 'generated'}, indent=1) + '\n')
    unknown = sorted(set(replaced) - set(rows))
    queue_path = out / 'queue.json'
    queue = json.loads(queue_path.read_text()) if queue_path.exists() else dict(items={})
    items = queue['items']
    for n, it in items.items():
        if n in rows and rows[n]['state'] == 'replaced':
            it['done'] = True
    tier = lambda r: 0 if r['share'] >= TIER_SHARE else 1 if r['share'] > 0 else 2
    new = sorted((r for r in rows.values() if r['state'] == 'queued' and r['name'] not in items),
                 key=lambda r: (tier(r), -r['share'], -len(r['levels'].split(',')) if r['levels'] else 0, r['name']))
    last = max((int(it['id'][1:]) for it in items.values()), default=0)
    first_batch = max((it['batch'] for it in items.values()), default=0) + 1
    for i, r in enumerate(new):
        items[r['name']] = dict(id=f'q{last + 1 + i:05d}', batch=first_batch + i // BATCH, tier=tier(r) + 1, done=False)
    for n, it in items.items():
        if n in rows:
            rows[n]['id'], rows[n]['batch'] = it['id'], it['batch']
    queue_path.write_text(json.dumps(queue, indent=1) + '\n')
    with open(out / 'textures.tsv', 'w', encoding='utf-8', newline='') as f:
        wr = csv.DictWriter(f, COLUMNS, delimiter='\t', lineterminator='\n')
        wr.writeheader()
        for n in sorted(rows, key=lambda n: (rows[n]['batch'] == '', rows[n]['batch'] or 0, rows[n]['id'], -rows[n]['share'], n)):
            wr.writerow(rows[n])
    states = Counter(r['state'] for r in rows.values())
    print(f'{len(rows)} textures: ' + ', '.join(f'{v} {k}' for k, v in states.most_common()))
    print(f'{len(unknown)} installed replacements name no texture listed here (aliases a pack draws under another name)')
    open_items = [it for it in items.values() if not it['done']]
    tiers = Counter(it['tier'] for it in open_items)
    batches = sorted({it['batch'] for it in open_items})
    print(f'queue: {len(open_items)} textures to redraw (tier 1, at least {TIER_SHARE:.1%} of a level\'s picture: {tiers[1]}; '
          f'tier 2, seen in play: {tiers[2]}; tier 3, not measured: {tiers[3]}), batches {batches[0]}-{batches[-1]} '
          f'of {BATCH}; {sum(it["done"] for it in items.values())} done since queued')
    print(f'{out / "textures.tsv"}, {queue_path}')


def rows_of(out):
    with open(out / 'textures.tsv', encoding='utf-8') as f:
        return {r['name']: r for r in csv.DictReader(f, delimiter='\t')}


# How much a moby class's role counts (the NPC catalogue's roles); a texture takes its strongest class's.
ROLE_WEIGHT = {'enemy': 1.0, 'boss': 1.0, 'story': 1.0, 'companion': 1.0, 'vehicle': 1.0, 'player': 1.0, 'vidcomic': 1.0,
               'prop': 0.7, 'summon': 0.7, 'museum': 0.5, 'ambient': 0.5, 'unknown': 0.5}


def usage(review):
    """Per texture, how much the game uses it, from the old review's indexes of every story level (no camera involved):
    terrain triangles, tie triangles (each placed instance's, shared among its class's textures), the levels whose shrubs
    use it, the levels loading a moby class that uses it, the cutscenes whose classes use it, and those classes' roles."""
    u = {}
    def get(n):
        return u.setdefault(n, dict(terrain=0.0, tie=0.0, shrub=set(), moby=set(), classes=set(), scenes=set(), roles=set()))
    for f in sorted((review / 'npc-catalogue/terrain-index').glob('L*.tsv')):
        for r in csv.DictReader(open(f, encoding='utf-8'), delimiter='\t'):
            if canon(r['stem'] + '.png'):
                get(canon(r['stem'] + '.png'))['terrain'] += float(r['stem_triangles'])
    for f in sorted((review / 'npc-catalogue/tie-index').glob('L*.tsv')):
        for r in csv.DictReader(open(f, encoding='utf-8'), delimiter='\t'):
            stems = [canon(s + '.png') for s in r['stems'].split(',') if canon(s + '.png')]
            for n in stems:
                get(n)['tie'] += float(r['triangles']) / len(stems)
    for d in sorted((review / 'npc-catalogue/shrub-index').glob('L*')):
        for p in d.iterdir():
            if canon(p.name):
                get(canon(p.name))['shrub'].add(d.name)
    for r in csv.DictReader(open(review / 'library/uya-mobies/occurrences.tsv', encoding='utf-8'), delimiter='\t'):
        for s in r['textures'].split(','):
            if canon(s + '.png'):
                get(canon(s + '.png'))['moby'].add(r['level'])
                get(canon(s + '.png'))['classes'].add((int(r['level']), int(r['class_id'])))
    for e in json.loads((review / 'npc-catalogue/full/catalogue.json').read_text(encoding='utf-8'))['entries']:
        scenes = {(o.get('level'), o.get('scene_record')) for o in e.get('occurrences', []) if 'SCENE' in o.get('container', '')}
        for t in e.get('textures', []):
            if canon(t + '.png'):
                get(canon(t + '.png'))['roles'].add(e['role'])
                get(canon(t + '.png'))['scenes'] |= scenes
    return u


KEY_ROLES = {'vehicle', 'player'}  # props that count 1.5 times
# Characters, enemy or friendly alike (the player: "enemies and friendly are interchangable all mobies count"): first after
# the cutscenes, the most placed first.
# The vid-comic levels' characters are ranked with the props (the player: "move the vid com ones back").
CHARACTER_ROLES = {'enemy', 'boss', 'story', 'companion', 'museum', 'ambient'}
EFFECT_WORDS = re.compile(r'effect|glow|sprite|fog|haze|flash|sky|hud|decal|plain colou?r|backdrop|noise|gradient|screen|menu', re.I)
TEXT_WORDS = re.compile(r'text|sign|logo|letter|emblem|glyph', re.I)


def moby_instances(gameplay, out):
    """{(level, moby class id): placed instances} from each level's saved gameplay RAM (`<gameplay>/LNN/ram*.bin`, the
    frames the share was measured on): the live moby array, 0x100-byte records with the class id (s16) at +0xAA and a
    record whose +0x20 is 0x80 or more not live, found as the global (start, end) pair in 0x100000-0x200000 that bounds
    the run of records with the most distinct classes, all with plausible ids and most at a real position (level 1: 720
    records, 56 classes, 123 crates; level 9: 1,338, 72 classes, 551 crates). Kept in <out>/moby-instances.tsv."""
    cache = out / 'moby-instances.tsv'
    if cache.exists():
        return {(int(r['level']), int(r['class_id'])): int(r['instances']) for r in csv.DictReader(open(cache, encoding='utf-8'), delimiter='\t')}
    found = {}
    for d in sorted(gameplay.glob('L*')):
        rams = sorted(d.glob('ram*.bin'))
        if len(rams) != 1:
            sys.exit(f'{d}: {len(rams)} RAM dumps, not one')
        b = rams[0].read_bytes()
        u32 = lambda a: struct.unpack_from('<I', b, a)[0]
        s16 = lambda a: struct.unpack_from('<h', b, a)[0]
        f32 = lambda a: struct.unpack_from('<f', b, a)[0]
        best = None
        for at in range(0x100000, 0x200000, 4):
            s, e = u32(at), u32(at + 4)
            if not (0x100000 <= s < e < len(b)) or s % 16 or (e - s) % 256 or not 50 < (e - s) // 256 < 4096:
                continue
            cls = [s16(m + 0xAA) for m in range(s, e, 256)]
            n = len(cls)
            placed = sum(1 for m in range(s, e, 256) if all(abs(f32(m + 0x10 + 4 * i)) < 1e5 for i in range(3)) and any(f32(m + 0x10 + 4 * i) for i in range(3)))
            if sum(0 <= c < 12000 for c in cls) / n > 0.98 and placed / n > 0.5 and len(set(cls)) > 15 and (best is None or len(set(cls)) > best[2]):
                best = (s, e, len(set(cls)))
        if best is None:
            sys.exit(f'{rams[0]}: no moby array found')
        level = int(d.name[1:])
        for m in range(best[0], best[1], 256):
            if b[m + 0x20] < 0x80:
                found[(level, s16(m + 0xAA))] = found.get((level, s16(m + 0xAA)), 0) + 1
    with open(cache, 'w', encoding='utf-8', newline='') as f:
        f.write('level\tclass_id\tinstances\n' + ''.join(f'{l}\t{c}\t{v}\n' for (l, c), v in sorted(found.items())))
    return found


def rank(out, review, gameplay, first):
    """Ranks every texture still to redraw in batch `first` or later in two lists, each on its own measure, merged by rank
    (the player: "rank each one separately and combine them in the final ranking"; a prop's share of one level frame
    says nothing about how much a prop is seen).

    Level surfaces (ground, walls, placed structures, foliage): expected screen presence, the measured share blended half
    and half with a prior by geometry tier (the mean measured share of each tier: terrain + half its tie triangles of
    3,000 or more 0.0065, 300 or more 0.0035, any 0.0007, foliage only 0.0005, none 0.0002), the prior alone where the
    share was never measured (no kind: never in the frames' survey), most used first within a tier. Props and models:
    their placed instances summed over every level (`moby_instances`) plus half the log of the levels loading them, a
    vehicle's or the player's 1.5 times; no screen share. Characters, enemy and friendly alike (CHARACTER_ROLES), come
    first after the cutscenes, the most placed first. Both times the improvement a
    redraw brings (a Rift Apart material and a fan upscale 0.6, a material 0.7, a fan upscale 0.85) and its likelihood
    of passing (soft see-through 0.6, cut-out 0.85, effects, skies, haze, backdrops 0.5, text and logos 0.7, plain
    originals 0.7, 32 x 32 or smaller 0.6). The two lists are merged by each texture's percentile in its own list. Textures a
    cutscene shows come first; textures used nowhere last. Ids stay."""
    rows = rows_of(out)
    queue_path = out / 'queue.json'
    queue = json.loads(queue_path.read_text())
    items = queue['items']
    todo = [n for n, it in items.items() if not it['done'] and it['batch'] >= first]
    u = usage(review)
    inst = moby_instances(gameplay, out)
    none = dict(terrain=0.0, tie=0.0, shrub=set(), moby=set(), classes=set(), scenes=set(), roles=set())
    score, why, amount = {}, {}, {}
    for n in todo:
        r, x = rows[n], u.get(n, none)
        share, kind, geo, moby, shrub = float(r['share']), r['kind'], x['terrain'] + 0.5 * x['tie'], len(x['moby']), len(x['shrub'])
        placed = sum(inst.get(c, 0) for c in x['classes'])
        gain = {(True, True): 0.6, (True, False): 0.7, (False, True): 0.85, (False, False): 1.0}[(r['material'] == 'yes', r['fan_pack'] == 'yes')]
        odds = ((0.6 if r['alpha'] == 'graded' else 0.85 if r['alpha'] == 'cutout' else 1.0)
                * (0.5 if EFFECT_WORDS.search(r['about']) else 1.0) * (0.7 if TEXT_WORDS.search(r['about']) else 1.0)
                * (0.7 if r['detail'] and float(r['detail']) < 1.5 else 1.0) * (0.6 if int(r['width']) * int(r['height']) <= 1024 else 1.0))
        surface = kind not in ('object', '') or geo > 0 or (shrub and not moby)
        if surface:
            prior = 0.0065 if geo >= 3000 else 0.0035 if geo >= 300 else 0.0007 if geo > 0 else 0.0005 if shrub else 0.0002
            score[n] = (prior if kind == '' else 0.5 * share + 0.5 * prior) * gain * odds
            amount[n] = geo
            why[n] = 'level surface'
        elif moby or placed:
            score[n] = (math.log1p(placed) + 0.5 * math.log1p(moby)) * (1.5 if x['roles'] & KEY_ROLES else 1.0) * gain * odds
            amount[n] = placed
            why[n] = 'prop'
        else:
            score[n], amount[n], why[n] = 0.0, 0, 'nothing'
    # Every texture a cutscene shows comes first (the player: "cutscene is important"), then every enemy's and boss's
    # (the player: "Enemies are tier 1 no matter what", "after cutscenes"), the most used first in each.
    for n in todo:
        x = u.get(n, none)
        if x['scenes']:
            score[n], why[n] = 3.0 + len(x['scenes']) / 1000, 'cutscenes'
        elif x['roles'] & CHARACTER_ROLES and 'vidcomic' not in x['roles']:
            placed = sum(inst.get(c, 0) for c in x['classes'])
            score[n], why[n] = 2.0 + math.log1p(placed) / 100 + len(x['moby']) / 1000, 'character'
            items[n]['tier'] = 1
    ranked = lambda group: sorted((n for n in todo if why[n] == group), key=lambda n: (-round(score[n], 7), -amount.get(n, 0), -float(rows[n]['share']), n))
    lists = [ranked('level surface'), ranked('prop')]
    merged = sorted((i / len(lst), k, n) for k, lst in enumerate(lists) for i, n in enumerate(lst))
    order = ranked('cutscenes') + ranked('character') + [n for _, _, n in merged] + ranked('nothing')
    for i, n in enumerate(order):
        items[n].update(batch=first + i // BATCH, score=round(score[n], 4), ranked_by=why[n] or 'nothing')
    queue_path.write_text(json.dumps(queue, indent=1) + '\n')
    for r in rows.values():
        if r['name'] in items:
            r['batch'] = items[r['name']]['batch']
    with open(out / 'textures.tsv', 'w', encoding='utf-8', newline='') as f:
        wr = csv.DictWriter(f, COLUMNS, delimiter='\t', lineterminator='\n', extrasaction='ignore')
        wr.writeheader()
        for n in sorted(rows, key=lambda n: (rows[n]['batch'] == '', int(rows[n]['batch'] or 0), rows[n]['id'], n)):
            wr.writerow(rows[n])
    by = Counter(why[n] or 'nothing' for n in order)
    top = order[:BATCH * 40]
    print(f'{len(order)} textures ranked into batches {first}-{first + (len(order) - 1) // BATCH}; ranked by: '
          + ', '.join(f'{k} {v}' for k, v in by.most_common()))
    print(f'first {len(top)}: ' + ', '.join(f'{k} {v}' for k, v in Counter(why[n] for n in top).most_common()))
    for n in order[:12]:
        print(f"  {items[n]['id']} {score[n]:.3f} {why[n]:13} {rows[n]['kind'] or '-':7} {rows[n]['about'][:60]}")


def scaled_size(w, h):
    k = SIZE / max(w, h)
    return round(w * k), round(h * k)


KIND_WORDS = {'object': 'a texture of a model (a character, enemy, prop or machine)', 'panel': 'a designed panel (doors, plates, vents, walls)',
              'metal': 'a metal surface', 'floor': 'a floor', 'sand': 'sand or soil', 'rock': 'rock', 'grass': 'grass',
              'plant': 'a plant', 'wood': 'wood', 'ice': 'ice', 'other': 'a world surface'}


def describe(r):
    parts = [f"{r['width']} x {r['height']} original", KIND_WORDS.get(r['kind'], 'a texture')]
    if r['about']:
        parts.append(f"catalogued as: {r['about']}")
    if r['tiling'] == 'yes':
        parts.append('it repeats across its surface: its opposite edges must meet seamlessly')
    if r['alpha'] in ('cutout', 'graded'):
        parts.append('it has transparent parts (its alpha file shows where it is drawn)')
    return '; '.join(parts)


def prompt(batch, items):
    k = len(items)
    files = '\n'.join(f"  - `{it['id']}.png`: {describe(it['row'])}" + (f"; `{it['id']}-alpha.png` its alpha" if it['alpha'] else '')
                      + (f"; `{it['id']}-variant.png` the same picture in other colours, already redrawn" if it['variant'] else '') for it in items)
    outs = '\n'.join(f"- `{it['id']}-colour.png` and `{it['id']}-depth.png`, {it['size'][0]} x {it['size'][1]}" for it in items)
    return f"""Redraw {k} textures from Ratchet & Clank: Up Your Arsenal (PS2) at high resolution, as texture maps: for each one a colour texture and a depth texture. They are the game's own textures, drawn again sharp: the same design, the same colours, every shape where it is. Some are texture atlases (flat pieces of a model laid out on one sheet, which the game wraps onto the 3D model), some are world surfaces that repeat. Every piece and every line must stay exactly where it is, at its exact size and shape, or the texture lands wrong on the model or the wall.

**What is in the zip `texture-batch-{batch:03d}.zip`.** For each texture:
{files}

Each `qNNNNN.png` is the game's original enlarged to {SIZE} on its long side with hard pixels (the original is 8 or more times smaller). An `-alpha.png` is white where the texture is drawn and black where it is clear; the game keeps the original's transparency, so only the colours you draw where it is white are seen. A `-variant.png` is the finished redraw of the same picture in another palette: match its drawing and detail, in this texture's own colours.

**What to make, for each texture.**
1. `qNNNNN-colour.png`, RGB, the size of its `qNNNNN.png`: the texture redrawn at full resolution.
   - Its own colours and its own design exactly as the original shows them: the same shapes, panel lines, patterns, markings, lettering and their sizes and places. Only the detail gets sharper: crisp edges, clean lines, small parts made clear, fine surface detail (grain, scratches, weave, rivets) only where the original shows it.
   - Smooth areas stay smooth; keep the original's shading, brightness and contrast.
   - Do not restyle it: no realism, no new lighting, highlights, shadows, perspective or grunge; nothing added (no new parts, panels, logos, text or effects).
   - Where the alpha is black, carry on the neighbouring colours (no black or flat-colour background there).
2. `qNNNNN-depth.png`, greyscale, the same size: the physical relief of that surface: mid-grey 128 for flat surfaces, darker where it is recessed (seams, grooves, vents, gaps, cracks), lighter where it stands out (bolts, raised plates, rims, stones), shallow relief only. It is relief, not the colour image in grey: painted colours, markings and lettering are flat paint and do not show in it.

**How.**
- Use the image tool's edit mode with the texture's own `qNNNNN.png` as the image to keep, so every shape stays put.
- Generate each texture on its own, never several in one image.
- If a result moves, resizes, redraws or reshapes anything, changes the colours or the design, or invents detail, throw it away and do it again.

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
def shift(a, b, reach=32):     # how far b's edges sit from a's (the cross-correlation's peak within reach px)
    ea, eb = edges(a, 1.5), edges(b, 1.5)
    ea, eb = ea - ea.mean(), eb - eb.mean()
    xc = np.real(np.fft.ifft2(np.fft.fft2(eb) * np.conj(np.fft.fft2(ea))))
    H, W = xc.shape
    ys, xs = np.r_[0:reach + 1, H - reach:H], np.r_[0:reach + 1, W - reach:W]
    iy, ix = np.unravel_index(xc[np.ix_(ys, xs)].argmax(), (len(ys), len(xs)))
    return float(np.hypot(xs[ix] - (W if xs[ix] > reach else 0), ys[iy] - (H if ys[iy] > reach else 0)))
def check(q, original_size):   # q = 'q00001', original_size = (width, height) of the game's original, from the list above
    orig, col, dep = q + '.png', q + '-colour.png', q + '-depth.png'
    small = lambda p, how: cv2.resize(rgb(p), original_size, interpolation=how)
    o, c = small(orig, cv2.INTER_NEAREST), small(col, cv2.INTER_AREA)
    W, H = Image.open(col).size
    k = W / original_size[0]                                    # redrawn pixels per original pixel
    up = cv2.resize(o.mean(2), (W, H), interpolation=cv2.INTER_CUBIC)
    er = edges(grey(col), k / 2)
    eo = cv2.dilate(edges(up, k / 2), np.ones((int(2 * k) + 1,) * 2, np.uint8))
    return dict(colour_shift_px=shift(grey(orig), grey(col)),          # must be {SHIFT_TEXELS:g} original pixel or less (and at most 2 px is always fine)
                colour_edges=corr(edges(o.mean(2), 1), edges(c.mean(2), 1)),  # must be {EDGES_LIMIT:g} or more ({EDGES_DITHER:g} on a nearly plain texture)
                colour_difference=float(np.abs(o - c).mean()),          # must be under {DIFFERENCE_LIMIT:g}
                colour_novelty=float(np.maximum(er - 1.5 * eo, 0).sum() / (er.sum() + 1e-6)),  # new edges: must be {NOVELTY_LIMIT:g} or less
                depth_shift_px=shift(grey(col), grey(dep)))             # as colour_shift_px
```

- A texture moved by one original pixel scores a shift of one original pixel or more and fails. If anything shifted, scaled, rotated or got cropped, fix it or generate it again before you deliver. A new line, ring, panel or rivet that the original does not have shows as novelty above {NOVELTY_LIMIT:g}. Report each texture's five numbers.

**Deliver.**
- One zip named `texture-batch-{batch:03d}-done.zip` holding only these {2 * k} PNGs at its top level, each at exactly its size:
{outs}
- A contact sheet PNG: each original beside your colour and depth textures, the file name under each.

Do all {k}.
"""


def pack(out, first, last):
    rows = rows_of(out)
    queue = json.loads((out / 'queue.json').read_text())['items']
    folder_root = out / 'batches'
    folder_root.mkdir(exist_ok=True)
    for batch in range(first, last + 1):
        names = sorted((n for n, it in queue.items() if it['batch'] == batch and not it['done']), key=lambda n: queue[n]['id'])
        if not names:
            print(f'batch {batch}: nothing left to redraw')
            continue
        folder = folder_root / f'batch-{batch:03d}'
        folder.mkdir(exist_ok=True)
        items, images = [], {}
        for n in names:
            r, qid = rows[n], queue[n]['id']
            rgba = read_rgba(r['picture'])
            if rgba.shape[:2] != (int(r['height']), int(r['width'])):
                sys.exit(f"{n}: its picture {r['picture']} is {rgba.shape[1]} x {rgba.shape[0]}, not {r['width']} x {r['height']}")
            size = scaled_size(int(r['width']), int(r['height']))
            images[f'{qid}.png'] = Image.fromarray(np.ascontiguousarray(rgba[..., :3])).resize(size, Image.Resampling.NEAREST)
            alpha = r['alpha'] in ('cutout', 'graded')
            if alpha:
                images[f'{qid}-alpha.png'] = Image.fromarray(np.uint8(np.clip(rgba[..., 3].astype(np.int32) * 255 // GS_OPAQUE, 0, 255))).resize(size, Image.Resampling.NEAREST)
            if r['variant']:
                images[f'{qid}-variant.png'] = Image.open(r['variant']).convert('RGB').resize(size, Image.Resampling.LANCZOS)
            items.append(dict(id=qid, row=r, size=size, alpha=alpha, variant=bool(r['variant'])))
        for f, im in images.items():
            im.save(folder / f)
        z = folder_root / f'texture-batch-{batch:03d}.zip'
        with zipfile.ZipFile(z, 'w', zipfile.ZIP_DEFLATED) as zf:
            for f in images:
                zf.write(folder / f, f)
        (folder_root / f'prompt-batch-{batch:03d}.md').write_text(prompt(batch, items), encoding='utf-8')
        cols = len(items)
        sheet = Image.new('RGB', (cols * 260, 300), (24, 24, 28))
        draw = ImageDraw.Draw(sheet)
        for i, it in enumerate(items):
            im = images[f"{it['id']}.png"].copy()
            im.thumbnail((256, 256))
            sheet.paste(im, (i * 260, 0))
            draw.text((i * 260 + 4, 262), f"{it['id']}  {float(it['row']['share']):.2%}", fill=(230, 230, 230))
            draw.text((i * 260 + 4, 278), it['row']['kind'] or it['row']['sources'][:30], fill=(180, 180, 180))
        sheet.save(folder_root / f'batch-{batch:03d}-sheet.png')
        print(f'batch {batch}: {", ".join(it["id"] for it in items)} -> {z}')


# What a returned pair must reach (the prompt's numbers). Set 24 November 2026 on the first 107 textures of the OpenRouter
# run, every try looked at for the 8 that failed the first limits (ship_colours.py's: edges 0.9 and 2 px):
# - the edges' correlation at the original's pixels measures how much of its dither a redraw keeps: faithful redraws of
#   plain plates, skies and concrete scored 0.73 to 0.90; an invented circle on a plain panel scored 0.88 and invented rivet
#   panels on a haze 0.95. It only catches a redraw that lost the original's structure (0.19), so its limit is 0.8, and
#   0.5 on a nearly plain original (detail under DITHER_DETAIL), whose edges are its dither;
# - the novelty (`check`) is what catches invented structure: the share of the redraw's edge strength standing where the
#   original, enlarged smoothly, has no edge within a pixel of it: inventions 0.15 to 0.55, faithful redraws 0.00 to 0.08
#   (an embossed plate drawn crisper 0.14 to 0.39);
# - the shift is limited to SHIFT_TEXELS of an original pixel (2 px was an eighth of a pixel on a 64-pixel texture): of the
#   120 textures' tries, 16 failed on a shift of 0.32 to 0.56 of a pixel alone, within what the game's filtering blurs,
#   and 16 on 0.6 to 4 pixels, a real misplacement.
# The player, shown the 15 tier-1 textures still failing after 8 or more tries each: "these checks are too strict". The
# limits were loosened that day (from 0.8, 0.5, 16, 0.15 and half a pixel), which passed 12 of the 15, and the player
# kept them as they were for that test; where several tries pass, `score` picks.
EDGES_LIMIT, EDGES_DITHER, DITHER_DETAIL = 0.6, 0.3, 1.5
DIFFERENCE_LIMIT, NOVELTY_LIMIT, SHIFT_TEXELS = 24.0, 0.3, 1.0


def score(got, k):
    """How far a passing colour is from its original, lower better: new edges, lost edges, shift in original pixels and
    colour difference, each over its limit."""
    return (got['colour_novelty'] / NOVELTY_LIMIT + (1 - got['colour_edges']) / (1 - EDGES_LIMIT)
            + got['colour_shift_px'] / k / SHIFT_TEXELS + got['colour_difference'] / DIFFERENCE_LIMIT)


def edges(a, s):
    a = cv2.GaussianBlur(a, (0, 0), s)
    return np.hypot(cv2.Sobel(a, cv2.CV_32F, 1, 0), cv2.Sobel(a, cv2.CV_32F, 0, 1))


def corr(a, b):
    a, b = a - a.mean(), b - b.mean()
    return float((a * b).sum() / np.sqrt((a * a).sum() * (b * b).sum()))


REACH = 32  # px: the farthest shift `shift` looks for


def shift(a, b):
    """How far b's edges sit from a's: the peak of their circular cross-correlation within REACH px. Not a global phase
    correlation: on a texture that repeats inside itself that finds the repeat (511 px on a 2 x 2 tile), not a shift."""
    ea, eb = edges(a, 1.5), edges(b, 1.5)
    ea, eb = ea - ea.mean(), eb - eb.mean()
    xc = np.real(np.fft.ifft2(np.fft.fft2(eb) * np.conj(np.fft.fft2(ea))))
    H, W = xc.shape
    ys, xs = np.r_[0:REACH + 1, H - REACH:H], np.r_[0:REACH + 1, W - REACH:W]
    iy, ix = np.unravel_index(xc[np.ix_(ys, xs)].argmax(), (len(ys), len(xs)))
    dy, dx = ys[iy] - (H if ys[iy] > REACH else 0), xs[ix] - (W if xs[ix] > REACH else 0)
    return float(np.hypot(dx, dy))


def check(original, colour, depth, no_shapes=False):
    """The prompt's check of one returned pair: original (h x w x 3), colour and depth (H x W x 3 and H x W) at the
    redrawn size, as float32 arrays. Its numbers and the names of the limits it misses."""
    h, w = original.shape[:2]
    H, W = colour.shape[:2]
    k = W / w
    grey = lambda a: cv2.cvtColor(a, cv2.COLOR_RGB2GRAY)
    big = cv2.resize(original, (W, H), interpolation=cv2.INTER_NEAREST)
    small = cv2.resize(colour, (w, h), interpolation=cv2.INTER_AREA)
    er = edges(grey(colour), k / 2)
    eo = cv2.dilate(edges(cv2.resize(original.mean(2), (W, H), interpolation=cv2.INTER_CUBIC), k / 2), np.ones((int(2 * k) + 1,) * 2, np.uint8))
    got = dict(colour_shift_px=shift(grey(big), grey(colour)),
               colour_edges=corr(edges(original.mean(2), 1), edges(small.mean(2), 1)),
               colour_difference=float(np.abs(original - small).mean()),
               colour_novelty=float(np.maximum(er - 1.5 * eo, 0).sum() / (er.sum() + 1e-6)),
               depth_shift_px=shift(grey(colour), depth))
    plain = detail(np.dstack([original, np.full((h, w), 255, np.float32)])) < DITHER_DETAIL
    limit = dict(colour_shift_px=max(2.0, SHIFT_TEXELS * k), colour_edges=EDGES_DITHER if plain else EDGES_LIMIT,
                 colour_difference=DIFFERENCE_LIMIT, colour_novelty=NOVELTY_LIMIT, depth_shift_px=max(2.0, SHIFT_TEXELS * k))
    misses = [n for n, v in got.items() if (v < limit[n] if n.endswith('edges') else v > limit[n])]
    # A surface with no shapes, only colour and grain (the visual pass's "shapes": false), has no structure to lose: the
    # edge match there compares random grain speck by speck and came out near 0 for faithful redraws (8 such textures, all
    # approved by the player by eye, 24 November 2026; the player: "Yes, both"). The other limits still apply.
    if no_shapes:
        misses = [n for n in misses if n != 'colour_edges']
    return got, misses


def smooth_alpha(a, size, kind):
    """The original's alpha (GS scale) at the redrawn size: enlarged smoothly; a cut-out's kept a hard edge one texel wide."""
    up = cv2.resize(a.astype(np.float32), size, interpolation=cv2.INTER_CUBIC)
    if kind == 'cutout':
        k = size[0] / a.shape[1]
        up = np.clip((up / GS_OPAQUE - 0.5) * k + 0.5, 0, 1) * a.max()
    return np.clip(np.round(up), 0, max(int(a.max()), 0)).astype(np.uint8)


def fill_clear(rgb, alpha):
    """Colours under clear texels taken from the nearest drawn ones (blurs of growing size, normalised by coverage), so the
    game's filtering brings no foreign colour in at an outline."""
    seen = (alpha > 0).astype(np.float32)[..., None]
    out, todo = rgb.copy(), alpha == 0
    for radius in (2, 4, 8, 16, 32, 64, 128):
        num, den = cv2.GaussianBlur(rgb * seen, (0, 0), radius), cv2.GaussianBlur(seen, (0, 0), radius)[..., None]
        ok = todo & (den[..., 0] > 1e-3)
        out[ok] = num[ok] / den[ok]
        todo &= ~ok
    return out


RETURNED = re.compile(r'(q\d{5})-(colour|depth)\.png')


def returned_files(sources):
    """{file name: (a function reading its bytes, its time or None)} of the PNGs in the returned folders (any folder in
    them) and zips; read when needed, as a folder grows to thousands of pairs. A name twice stops the run."""
    found = {}
    def add(name, read, when):
        if name in found:
            sys.exit(f'{name} is returned twice')
        found[name] = (read, when)
    for src in sources:
        if src.is_dir():
            for p in sorted(src.rglob('*.png')):
                add(p.name, p.read_bytes, p.stat().st_mtime)
        elif src.suffix.lower() == '.zip':
            zf = zipfile.ZipFile(src)
            for i in zf.infolist():
                if i.filename.lower().endswith('.png'):
                    add(Path(i.filename).name, lambda i=i, zf=zf: zf.read(i), None)
        else:
            sys.exit(f'{src}: not a folder or a zip')
    return found


def intake(out, sources, generated, accept=frozenset(), shard=(0, 1)):
    rows = rows_of(out)
    queue = json.loads((out / 'queue.json').read_text())['items']
    by_id = {it['id']: n for n, it in queue.items()}
    pack_dir = out / '0-riftapart'
    rows_path = pack_dir / 'rows.json'
    done_rows = json.loads(rows_path.read_text()) if rows_path.exists() else dict(relief={})
    pairs = {}
    for name, f in returned_files(sources).items():
        m = RETURNED.fullmatch(name)
        if not m:
            print(f'{name}: not a returned texture (left out)')
            continue
        if m[1] not in by_id:
            sys.exit(f'{name}: there is no queued texture {m[1]}')
        pairs.setdefault(by_id[m[1]], {})[m[2]] = f
    if generated:
        # The old level queue's art never installed (list's generated.json): its colour and its height, checked alike.
        for n, (colour, height) in json.loads((out / 'generated.json').read_text()).items():
            pairs[n] = dict(colour=(Path(colour).read_bytes, None), depth=(Path(height).read_bytes, None))
    # A pair compiled since it came back is not checked again.
    def compiled(n, p):
        rep = pack_dir / 'replacements' / f'{n}.png'
        return (n in done_rows['relief'] and rep.exists() and all(when is not None and when <= rep.stat().st_mtime for _, when in p.values()))
    # Several compiles at once (shard k of n checks every n-th pair) each merge what they compiled into the tables at the end.
    names = sorted(n for n, p in pairs.items() if not compiled(n, p))[shard[0]::shard[1]]
    pairs = {n: {k: read() for k, (read, _) in pairs[n].items()} for n in names}
    report_path = out / 'intake.json'
    report = json.loads(report_path.read_text()) if report_path.exists() else {}
    added, mine = {}, {}
    failed = []
    for n in sorted(pairs, key=lambda n: queue.get(n, {}).get('id', n)):
        p, r = pairs[n], rows[n]
        label = queue[n]['id'] if n in queue else n
        if set(p) != {'colour', 'depth'}:
            failed.append(label)
            print(f'{label}: only its {", ".join(p)} came back')
            continue
        colour, depth = Image.open(io.BytesIO(p['colour'])), Image.open(io.BytesIO(p['depth']))
        size = scaled_size(int(r['width']), int(r['height']))
        # Another size of the same shape (the old queue's art is 1254 square) is brought to the size; another shape fails.
        shape = lambda im: im.size[0] / im.size[1]
        if abs(shape(colour) / (size[0] / size[1]) - 1) > 0.01 or abs(shape(depth) / (size[0] / size[1]) - 1) > 0.01:
            failed.append(label)
            print(f'{label}: colour {colour.size}, depth {depth.size}, not the shape of {size}')
            continue
        colour, depth = colour.resize(size, Image.Resampling.LANCZOS), depth.resize(size, Image.Resampling.LANCZOS)
        p['depth'] = io.BytesIO()
        depth.save(p['depth'], 'PNG')
        p['depth'] = p['depth'].getvalue()
        original = read_rgba(r['picture'])
        c = np.asarray(colour.convert('RGB'), np.float32)
        d = np.asarray(depth.convert('L'), np.float32)
        got, misses = check(original[..., :3].astype(np.float32), c, d, plain_by_pass(out, n))
        if misses and label in accept:
            # The player looked at it and took it (the check misreads it, as a logo's crisp letters read as invented).
            print(f'{label}: fails {", ".join(misses)}; accepted by the player')
            report[n] = dict(id=label, **{k: round(v, 3) for k, v in got.items()}, misses=[], accepted_over=misses)
            misses = []
        else:
            report[n] = dict(id=label, **{k: round(v, 3) for k, v in got.items()}, misses=misses)
        print(f'{label} {n}: ' + ', '.join(f'{k} {v:.2f}' for k, v in got.items()) + (f'  FAILS {", ".join(misses)}' if misses else '  passes'))
        if misses:
            failed.append(label)
            continue
        a = original[..., 3]
        if r['alpha'] == 'opaque':
            alpha = cv2.resize(a, size, interpolation=cv2.INTER_NEAREST)
            rgb = c
        else:
            alpha = smooth_alpha(a, size, r['alpha'])
            rgb = fill_clear(c, alpha)
        (pack_dir / 'replacements').mkdir(parents=True, exist_ok=True)
        (pack_dir / 'relief').mkdir(parents=True, exist_ok=True)
        Image.fromarray(np.dstack([np.uint8(np.clip(np.round(rgb), 0, 255)), alpha])).save(pack_dir / 'replacements' / f'{n}.png')
        relief_maps.relief(io.BytesIO(p['depth'])).save(pack_dir / 'relief' / f'{n}.png')
        done_rows['relief'][n] = added[n] = f'{n}.png'
    mine = {n: report[n] for n in pairs if n in report}
    rows_path.parent.mkdir(parents=True, exist_ok=True)
    # Merged into the tables as they are now (another compile may have written them since this one read them), one writer
    # at a time, each table written whole and swapped in.
    with table_lock(pack_dir / 'rows.lock'):
        latest = read_json_when_free(rows_path) if rows_path.exists() else dict(relief={})
        latest['relief'].update(added)
        part = rows_path.with_suffix('.part')
        part.write_text(json.dumps(latest, indent=1) + '\n')
        replace_when_free(part, rows_path)
        latest_report = read_json_when_free(report_path) if report_path.exists() else {}
        latest_report.update(mine)
        part = report_path.with_suffix('.part')
        part.write_text(json.dumps(latest_report, indent=1) + '\n')
        replace_when_free(part, report_path)
    print(f'{len(latest["relief"])} textures compiled in {pack_dir}; failed this time: {failed}')


class table_lock:
    """A lock file held while a compile merges its rows (taken with O_EXCL; a minute's wait, then a loud stop)."""
    def __init__(self, path):
        self.path = path

    def __enter__(self):
        for wait in range(600):
            try:
                os.close(os.open(self.path, os.O_CREAT | os.O_EXCL | os.O_WRONLY))
                return self
            except FileExistsError:
                time.sleep(0.1)
        raise RuntimeError(f'{self.path}: held for a minute (a compile stopped while merging? remove it once none runs)')

    def __exit__(self, *exc):
        os.remove(self.path)


API = 'https://openrouter.ai/api/v1/images'
# The models (24 November 2026, eleven tried on the same three tier-1 textures, model-test-*.png beside the queue): the colour
# by Qwen Image 2.1 Turbo ($0.018 an image; the player judged it best on the robot atlas and the cobble; crisp and faithful),
# the depth by MAI-Image 2.7 Flash ($0.016; a full-range height map, where Qwen's came out at a third of the range). Nano
# Banana 2 ($0.068) dropped a panel's groove; FLUX was refused by its provider's moderation; GPT Image 1 Mini moved everything.
MODEL = 'qwen/qwen-image-2.1-turbo'
DEPTH_MODEL = 'microsoft/mai-image-2.7-flash'
# The output shapes the model takes (OpenRouter's Image API `aspect_ratio`); a texture of another shape is sent stretched
# to a square and its result brought back to its own shape.
ASPECTS = ('1:1', '1:4', '1:8', '4:1', '8:1')
TRIES = 3  # generations of one texture's colour, or depth, before it is left failed


def aspect_of(w, h):
    a = f'{w // min(w, h)}:{h // min(w, h)}'
    return a if a in ASPECTS else '1:1'


def data_url(im):
    b = io.BytesIO()
    im.save(b, 'PNG')
    return 'data:image/png;base64,' + base64.b64encode(b.getvalue()).decode()


_params = {}


class Blocked(RuntimeError):
    """A provider's content filter refused the request (OpenRouter 400 "... blocked this request ..."); not billed."""


# Where a request a provider blocks goes next (24 November 2026: Azure blocked MAI's depth of seven enemy textures whose
# Qwen colours had passed, and Alibaba blocked Qwen's colour of another as copyrighted).
BLOCKED_NEXT = {'microsoft/mai-image-2.7-flash': 'qwen/qwen-image-2.1-turbo', 'qwen/qwen-image-2.1-turbo': 'google/gemini-3.1-flash-image'}


def draw(key, prompt_text, images, aspect, model, notes):
    """`openrouter` with `model`, and on a block the next model of BLOCKED_NEXT, noted; the last block is raised."""
    while True:
        try:
            return openrouter(key, prompt_text, images, aspect, model)
        except Blocked:
            if model not in BLOCKED_NEXT:
                raise
            notes.append(f'{model} blocked the request: sent to {BLOCKED_NEXT[model]}')
            model = BLOCKED_NEXT[model]


def model_params(key, model):
    """The request parameters a model's first endpoint takes (OpenRouter's image model endpoints listing)."""
    if model not in _params:
        req = urllib.request.Request(f'https://openrouter.ai/api/v1/images/models/{model}/endpoints', headers={'Authorization': f'Bearer {key}'})
        with urllib.request.urlopen(req, timeout=60) as r:
            _params[model] = json.load(r)['endpoints'][0]['supported_parameters']
    return _params[model]


def openrouter(key, prompt_text, images, aspect, model=MODEL):
    """One generation: (image, cost in USD). Retries what OpenRouter says to retry (429, 402 in-flight budget, 5xx).
    The shape and the 1K size are asked for where the model takes them; the result is brought to the size anyway."""
    sp = model_params(key, model)
    body = dict(model=model, prompt=prompt_text, n=1, response_format='b64_json',
                input_references=[dict(type='image_url', image_url=dict(url=data_url(im))) for im in images])
    if aspect in sp.get('aspect_ratio', {}).get('values', []):
        body['aspect_ratio'] = aspect
    if '1K' in sp.get('resolution', {}).get('values', []):
        body['resolution'] = '1K'
    body = json.dumps(body).encode()
    for attempt in range(6):
        req = urllib.request.Request(API, body, headers={'Authorization': f'Bearer {key}', 'Content-Type': 'application/json'})
        try:
            with urllib.request.urlopen(req, timeout=600) as r:
                resp = json.load(r)
        except urllib.error.HTTPError as e:
            text = e.read().decode('utf-8', 'replace')[:400]
            # A 402 for the account's credit (limit_source "openrouter_credits": the request's worst-case cost is more
            # than the balance) does not pass by waiting; only the in-flight budget's does.
            if e.code in (402, 429, 500, 502, 503, 504) and attempt < 5 and 'openrouter_credits' not in text:
                time.sleep(float(e.headers.get('Retry-After') or 10 * (attempt + 1)))
                continue
            if e.code == 400 and 'blocked this request' in text:
                raise Blocked(f'{model}: {text}')
            raise RuntimeError(f'OpenRouter {e.code}: {text}')
        d = resp['data'][0]
        if d.get('b64_json'):
            raw = base64.b64decode(d['b64_json'])
        else:  # a model that answers with a link only
            with urllib.request.urlopen(d['url'], timeout=300) as r:
                raw = r.read()
        return Image.open(io.BytesIO(raw)).convert('RGB'), float(resp.get('usage', {}).get('cost') or 0)
    raise RuntimeError('OpenRouter: no answer after 6 tries')


PROMPTS = HERE / 'texture_prompts'  # the reconstruction prompt and its parts (README.md: why each is as it is)


def _clauses():
    """The optional-input clauses file's sections, by heading: VARIANT, RGB-ONLY ALPHA NOTE, OPTIONAL HARD-PIXEL SOURCE, TILING."""
    out, head, lines = {}, None, []
    for line in (PROMPTS / 'optional-input-clauses.txt').read_text(encoding='utf-8').splitlines():
        if line and line == line.upper() and not line.startswith(('{', '"', '}')):
            if head:
                out[head] = '\n'.join(lines).strip()
            head, lines = line.split(' (')[0], []
        else:
            lines.append(line)
    out[head] = '\n'.join(lines).strip()
    out['TILING'] = json.loads(out['TILING'])
    return out


def treatment_for(r):
    """The treatment (texture_prompts/treatments.json) a texture's known data points to; '' where it does not settle one, which
    the reconstruction prompt allows (README.md section 2: choose from appearance and known role, an unknown role gets the
    base rules only). Text, logos and stripes: flat_graphic; effects, glows, skies and soft see-through: soft_effect (a
    gradient: smooth_gradient); model textures: uv_atlas; ground, rock, sand, grass, plants, wood and ice: grain; panels and
    metal: structured. soft_bands is chosen by hand for a texture that fails as too sharp and clean."""
    about, kind = r['about'], r['kind']
    if TEXT_WORDS.search(about) or re.search(r'stripe', about, re.I):
        return 'flat_graphic'
    if EFFECT_WORDS.search(about) or r['alpha'] == 'graded':
        return 'smooth_gradient' if re.search(r'gradient', about, re.I) else 'soft_effect'
    if kind == 'object':
        return 'uv_atlas'
    if kind in ('rock', 'sand', 'grass', 'plant', 'wood', 'floor', 'ice'):
        return 'grain'
    if kind in ('panel', 'metal'):
        return 'structured'
    return ''


DESCRIBER = 'anthropic/claude-haiku-5.5'  # the visual pass: about $0.0004 a texture
_described_lock = threading.Lock()


_seen_cache = {}


def plain_by_pass(out, name):
    """Whether the visual pass found the texture to be only colour and grain, no shapes (`check`'s plain). The cache file is
    read once a change (intake asks for every pair, and reading it each time held it open against the visual pass's swap)."""
    path = out / 'descriptions.json'
    if not path.exists():
        return False
    when = path.stat().st_mtime
    if _seen_cache.get('path') != path or _seen_cache.get('when') != when:
        _seen_cache.update(path=path, when=when, seen=read_json_when_free(path))
    return _seen_cache['seen'].get(name, {}).get('shapes') is False


def read_json_when_free(path):
    """The JSON file, waiting up to a minute while another process (a run beside this one, Windows) swaps it in."""
    for wait in range(60):
        try:
            return json.loads(path.read_text(encoding='utf-8'))
        except PermissionError:
            if wait == 59:
                raise
            time.sleep(1)


def replace_when_free(part, path):
    """os.replace, waiting up to a minute for another process (a run's intake, Windows) to let go of the file it replaces."""
    for wait in range(60):
        try:
            return os.replace(part, path)
        except PermissionError:
            if wait == 59:
                raise
            time.sleep(1)


def describe_visually(key, out, r, original):
    """What the texture visibly is, for its reconstruction prompt (the player: "the prompts should be more tailored to each
    image instead of guessing"; the review's "small visual classification pass"): a vision model (DESCRIBER) is shown the
    original with hard pixels and returns a short description of the visible forms, the treatment it needs and the features
    a redraw must keep. Cached by texture name in <out>/descriptions.json."""
    path = out / 'descriptions.json'
    with _described_lock:
        cache = read_json_when_free(path) if path.exists() else {}
    if r['name'] in cache and 'shapes' in cache[r['name']]:
        return cache[r['name']]
    treatments = json.loads((PROMPTS / 'treatments.json').read_text(encoding='utf-8'))
    choices = '\n'.join(f'- {k}: {v.split(". ")[0]}.' for k, v in treatments.items())
    w, h = int(r['width']), int(r['height'])
    known = f"{w} x {h} px; {KIND_WORDS.get(r['kind'], 'a texture, role unknown')}" + (f"; catalogued as: {r['about']}" if r['about'] else '') + \
            ('; the game draws it with transparency (the image shows its colours only)' if r['alpha'] in ('cutout', 'graded') else '')
    ask = (f"This image is a {w} x {h} texture from a PS2 game, enlarged with hard pixels. Known: {known}.\n"
           f"Describe only what is visible, for an artist who will redraw it at high resolution without changing its design. Do not "
           f"guess a material or object you cannot see. Fine noise, speckle or grain in the pixels is part of the surface, not "
           f"compression: say how strong and how coarse it is, and choose grain (not smooth_gradient) for a surface that has it; "
           f"smooth_gradient is only for a surface with no grain at all. Then choose the one treatment that fits it, or \"\" if none "
           f"clearly does:\n{choices}\n"
           f"Answer with JSON only: {{\"description\": \"one or two sentences: the layout (single surface, UV atlas of pieces, "
           f"graphic, effect), the colour regions, hard versus soft boundaries, grain or mottling, any text or markings\", "
           f"\"treatment\": \"one key above or empty\", \"preserve\": \"one sentence naming the specific features a redraw must keep "
           f"exactly (positions, counts, widths, colours)\", \"shapes\": true if there is any shape, line, region, marking or "
           f"pattern beyond colour and fine grain, false if it is only colour and grain}}")
    big = Image.fromarray(np.ascontiguousarray(original)).resize((512, round(512 * h / w)), Image.Resampling.NEAREST)
    body = json.dumps(dict(model=DESCRIBER, max_tokens=1200, messages=[dict(role='user', content=[
        dict(type='image_url', image_url=dict(url=data_url(big))), dict(type='text', text=ask)])])).encode()
    req = urllib.request.Request('https://openrouter.ai/api/v1/chat/completions', body, headers={'Authorization': f'Bearer {key}', 'Content-Type': 'application/json'})
    with urllib.request.urlopen(req, timeout=300) as resp:
        answer = json.load(resp)
    text = answer['choices'][0]['message']['content']
    if '{' not in text or '}' not in text:
        raise RuntimeError(f"{r['name']}: the visual pass answered without JSON: {text[:300]!r}")
    found = json.loads(text[text.index('{'):text.rindex('}') + 1])
    if found['treatment'] not in treatments and found['treatment'] != '':
        raise RuntimeError(f"{r['name']}: the visual pass chose no known treatment: {found['treatment']!r}")
    found['cost'] = float(answer.get('usage', {}).get('cost') or 0)
    with _described_lock:
        cache = read_json_when_free(path) if path.exists() else {}
        cache[r['name']] = found
        # Written whole and swapped in: another run (a retry beside the main run) may read it at any moment.
        part = path.with_suffix(f'.{os.getpid()}.part')
        part.write_text(json.dumps(cache, indent=1) + '\n', encoding='utf-8')
        replace_when_free(part, path)
    return found


def reconstruction_prompt(r, original, variant, treatment=None, retry_note='', seen=None):
    """The reconstruction prompt (texture_prompts/colour-template.txt, from the player's online review, 24 November 2026),
    filled for a texture: its size, kind and catalogue name (and the review's own visual description where it wrote one,
    texture_prompts/example-routing.json), its average colour, its treatment, the tiling clause where it repeats, the variant
    clause when image 2 is sent, the RGB-only note for a texture with alpha (the model is sent RGB only), and a retry note."""
    w, h = int(r['width']), int(r['height'])
    known = {x['id']: x for x in json.loads((PROMPTS / 'example-routing.json').read_text(encoding='utf-8'))}.get(r['id'])
    if known:
        describe = f"{w} x {h} original; {known['visual_description']}"
        treatment = known['treatment'] if treatment is None else treatment
    elif seen:
        describe = (f"{w} x {h} original" + (f", catalogued as {r['about']}" if r['about'] else '') + f"; {seen['description'].rstrip('.')}. "
                    f"{seen['preserve'].rstrip('.')}")
        treatment = seen['treatment'] if treatment is None else treatment
    else:
        describe = f"{w} x {h} original; {KIND_WORDS.get(r['kind'], 'a texture')}" + (f", catalogued as {r['about']}" if r['about'] else '')
    treatment = treatment_for(r) if treatment is None else treatment
    treatments = json.loads((PROMPTS / 'treatments.json').read_text(encoding='utf-8'))
    c = _clauses()
    fill = dict(describe=describe, mean='#%02x%02x%02x' % tuple(int(x) for x in original.reshape(-1, 3).mean(0)),
                treatment=treatments[treatment] if treatment else '', tiling=c['TILING']['xy'] if r['tiling'] == 'yes' else '',
                variant=c['VARIANT'] if variant else '', pixel_reference='', alpha=c['RGB-ONLY ALPHA NOTE'] if r['alpha'] in ('cutout', 'graded') else '',
                retry_note=retry_note)
    text = (PROMPTS / 'colour-template.txt').read_text(encoding='utf-8').format(**fill)
    return re.sub(r'\n{3,}', '\n\n', text).strip()


def colour_prompt(r, original, variant, prompt='default'):
    """The colour generation's prompt. It names the original's mean colour and brightness range: told only to keep its
    colours, the model raised a subtle texture's contrast and invented blotches and bands (24 November: 3 tier-1 textures,
    hard-texel input and no numbers failed one of three; this prompt with the smooth input passed all three).

    prompt='soft' is for a texture whose redraws come out too sharp and clean: a soft, banded original that four models drew
    as crisp flat plates with thin seams (a ship panel, 14 tries, edge match 0.0 to 0.29). It is the prompt the player
    brought from another agent for that panel (24 November 2026), word for word, with the texture's own size, kind,
    tones and tiling put in: its point is "Do not collapse them into thin dark seam lines" (the divisions are wide
    painted tonal bands), keeping the cloudy shading at full strength, and reconstructing the painted marks in focus
    without reproducing the enlargement's blur. It is given the original alone."""
    mean = '#%02x%02x%02x' % tuple(int(x) for x in original.reshape(-1, 3).mean(0))
    lo, hi = np.percentile(original.astype(np.float32).mean(2), [2, 98])
    tiling = ' It tiles: its opposite edges must still meet seamlessly.' if r['tiling'] == 'yes' else ''
    v = (' Image 2 is the finished high-resolution redraw of the same texture in other colours: match its drawing and detail,'
         ' but keep image 1\'s own colours.') if variant else ''
    if prompt == 'v2':
        return reconstruction_prompt(r, original, variant)
    if prompt == 'soft':
        w, h = int(r['width']), int(r['height'])
        W, H = scaled_size(w, h)
        px = original.reshape(-1, 3).astype(np.float32)
        lum = px @ np.array([0.299, 0.587, 0.114], np.float32)
        tone = lambda sel: '#%02x%02x%02x' % tuple(int(round(x)) for x in px[sel].mean(0))
        dark, light = tone(lum <= np.percentile(lum, 2)), tone(lum >= np.percentile(lum, 98))
        neutral = np.abs(px.mean(0) - px.mean()).max() < 6
        noun = {'object': 'model', 'other': 'surface', '': 'game'}.get(r['kind'], r['kind'])
        colour = (f"Keep neutral greyscale with no colour cast, approximately {mean} on average, with the existing darkest shading "
                  f"around {dark} and lightest around {light}." if neutral else
                  f"Keep its exact hues, approximately {mean} on average, with the existing darkest shading around {dark} and "
                  f"lightest around {light}.")
        repeat = 'Preserve the repeating tile and match both pairs of opposite edges. ' if r['tiling'] == 'yes' else ''
        return (f"Repaint the attached PS2 {noun} texture at native {W} × {H} resolution, faithfully matching the visible design. "
                f"The attachment is an enlarged {w} × {h} source. Reconstruct its painted surface in focus; do not reproduce the "
                f"enlargement blur.\n"
                f"Keep every existing panel, strip and tonal band in exactly the same place, with the same dimensions. Preserve the "
                f"original uneven, subdued {'grey ' if neutral else ''}shading across the whole image.\n"
                f"The panel divisions are WIDE PAINTED TONAL BANDS. Keep their original widths and faint contrast. Resolve gentle "
                f"internal contours within those bands, with subtly irregular edges and softly feathered outer transitions. Do not "
                f"collapse them into thin dark seam lines. The broad dark-to-light band shapes are part of the artwork.\n"
                f"At full resolution, show a trace of fine matte paint texture and softly broken painted edges, just enough to make the "
                f"surface feel resolved and in focus. This detail must be extremely low contrast, integrated into the original tones and "
                f"invisible at thumbnail size. Keep the restrained, hand-painted game-art character. Do not make the surface look "
                f"polished, sterile or mechanically perfect.\n"
                f"Match the reference's actual light and dark areas. {colour} Preserve the original cloudy tonal variation and baked "
                f"shading at full strength. Add no new lighting, shadows or highlights.\n"
                f"No new geometry, rivets, bolts, scratches, cracks, grilles, markings or decoration. No crisp bevels, metallic glints, "
                f"sharpening halos, global blur, fog or heavy grain. {repeat}No crop, zoom, rotation or perspective.\n"
                f"The result should closely match the source when reduced to {w} × {h}, while the existing painted marks have "
                f"restrained surface definition at full size. Output only the {'square ' if W == H else ''}texture, filling the frame.")
    return (f"Image 1 is a low-resolution texture from the PS2 game Ratchet & Clank: Up Your Arsenal, shown enlarged: {describe(r)}. "
            f"Make the same texture at high resolution, as if the artist's original high-resolution source had been found: identical "
            f"composition with every shape, line, crack, rivet, panel edge, stripe and marking at exactly the same position and size, "
            f"only sharper and with finer detail inside the existing shapes. Keep the exact colours: its average colour is {mean} and its "
            f"brightness range is only about {hi - lo:.0f} of 255, so keep it exactly that subtle; do not add contrast, highlights, "
            f"shadows, vignettes, lighting gradients, blotches, new patterns or new shapes. Do not crop, zoom, rotate or reframe: fill the "
            f"frame edge to edge exactly as image 1.{tiling}{v} Output only the texture.")


DEPTH_PROMPT = ("Image 1 is a high-resolution game texture (image 2 is its low-resolution original). Make its height map: a "
                "greyscale image with exactly the same framing, every feature in exactly the same place. Mid-grey is the flat "
                "base surface, darker is recessed (seams, grooves, gaps, cracks, vents, holes), lighter is raised (bolts, "
                "plates, rims, stones, ridges); shallow relief only. Painted colours, markings and lettering are flat paint and "
                "do not appear. No lighting, shading, perspective or border. Output only the height map.")


def generate_one(key, out, n, qid, r, tries=TRIES, depth_model=DEPTH_MODEL, model=MODEL, prompt='default'):
    """A queued texture's colour, then its depth, each generated until it passes `check` (at most TRIES new tries). The
    passing pair goes to <out>/returned, every attempt to <out>/attempts; returns (passed, cost, notes). A pair already
    returned that passes is kept; colour attempts already made are checked before a new one is paid for (new ones are
    numbered after them), and a depth is made from the colour chosen (its file names that colour's try)."""
    w, h = int(r['width']), int(r['height'])
    size, aspect = scaled_size(w, h), aspect_of(w, h)
    # A shape both models take is asked for; any other shape is sent stretched to a square and brought back.
    if any(aspect not in model_params(key, m).get('aspect_ratio', {}).get('values', []) for m in (model, depth_model) if m != LOCAL_DEPTH):
        aspect = '1:1'
    original = read_rgba(r['picture'])[..., :3]
    api_size = size if aspect != '1:1' else (SIZE, SIZE)
    # Enlarged smoothly: hard texels made the model draw their steps as shapes.
    big = Image.fromarray(np.ascontiguousarray(original)).resize(api_size, Image.Resampling.LANCZOS)
    refs = [big] + ([Image.open(r['variant']).convert('RGB').resize(api_size, Image.Resampling.LANCZOS)] if r['variant'] else [])
    (out / 'attempts').mkdir(exist_ok=True)
    (out / 'returned').mkdir(exist_ok=True)
    cost, notes, colour = 0.0, [], None
    orig_f = original.astype(np.float32)
    returned = (out / 'returned' / f'{qid}-colour.png', out / 'returned' / f'{qid}-depth.png')
    if all(p.exists() for p in returned):
        got, misses = check(orig_f, np.asarray(Image.open(returned[0]).convert('RGB'), np.float32), np.asarray(Image.open(returned[1]).convert('L'), np.float32),
                            plain_by_pass(out, n))
        if not misses:
            return True, 0.0, ['kept: the pair already returned passes']
        for p in returned:  # its colour and depth tries stay in attempts/
            p.unlink()
        notes.append(f'the pair already returned fails {misses}')

    seen = describe_visually(key, out, r, original) if prompt == 'v2' else None
    plain = bool(seen) and seen['shapes'] is False

    def colour_ok(t, im, label):
        """The colour's score if it passes, else None."""
        arr = np.asarray(im, np.float32)
        got, misses = check(orig_f, arr, cv2.cvtColor(arr, cv2.COLOR_RGB2GRAY), plain)
        misses = [m for m in misses if not m.startswith('depth')]
        notes.append(f"colour {t}{label}: " + ', '.join(f'{k} {v:.2f}' for k, v in got.items() if not k.startswith('depth')) + (f' FAILS {misses}' if misses else ''))
        return None if misses else score(got, size[0] / w)

    # Of the tries made before, the best that passes.
    tried = sorted((int(p.stem.rsplit('-', 1)[1]), p) for p in (out / 'attempts').glob(f'{qid}-colour-*.png'))
    chosen, best = None, None
    for t, p in tried:
        im = Image.open(p).convert('RGB')
        s = colour_ok(t, im, ' (made before)')
        if s is not None and (best is None or s < best):
            colour, chosen, best = im, t, s
    if chosen is not None:
        notes.append(f'colour {chosen} chosen, score {best:.2f}')
    start = tried[-1][0] + 1 if tried else 0
    if colour is None:
        # v2: the reconstruction prompt, tailored to the texture by the visual pass.
        text = (reconstruction_prompt(r, original, bool(r['variant']), seen=seen) if prompt == 'v2'
                else colour_prompt(r, original, bool(r['variant']), prompt))
    for t in range(start, start + tries) if colour is None else ():
        im, c = draw(key, text, refs, aspect, model, notes)
        cost += c
        im = im.resize(size, Image.Resampling.LANCZOS)
        im.save(out / 'attempts' / f'{qid}-colour-{t}.png')
        if colour_ok(t, im, '') is not None:
            colour, chosen = im, t
            break
    if colour is None:
        return False, cost, notes
    if depth_model == LOCAL_DEPTH:
        dep = local_depth(colour, size[0] / w)
        dep.save(out / 'attempts' / f'{qid}-depth-c{chosen}-local.png')
        notes.append('depth: made locally from the colour')
        colour.save(out / 'returned' / f'{qid}-colour.png')
        dep.save(out / 'returned' / f'{qid}-depth.png')
        return True, cost, notes
    for t in range(max(tries, 1)):
        # The retry is Qwen's: MAI's two depths of a tank tread both sat a stripe off (28 and 32 px).
        im, c = draw(key, DEPTH_PROMPT, [colour.resize(api_size, Image.Resampling.LANCZOS), big], aspect, depth_model if t == 0 else MODEL, notes)
        cost += c
        dep = im.convert('L').resize(size, Image.Resampling.LANCZOS)
        dep.save(out / 'attempts' / f'{qid}-depth-c{chosen}-{t}.png')
        got, misses = check(orig_f, np.asarray(colour, np.float32), np.asarray(dep, np.float32), plain)
        notes.append(f"depth {t}: depth_shift_px {got['depth_shift_px']:.2f}" + (' FAILS' if 'depth_shift_px' in misses else ''))
        if 'depth_shift_px' not in misses:
            colour.save(out / 'returned' / f'{qid}-colour.png')
            dep.save(out / 'returned' / f'{qid}-depth.png')
            return True, cost, notes
    return False, cost, notes


LOCAL_DEPTH = 'local'
# Past the queue's cutoff (queue.json "cheap_from_batch"; the player: "switch to MAI colour and local depth before balance runs
# out, at a reasonable cutoff of importance"): the colour by MAI-Image 2.7 Flash ($0.013 against Qwen's $0.018) and the depth made
# here from the colour (free against $0.016), about $0.014 a texture against $0.036.
CHEAP_MODEL = 'microsoft/mai-image-2.7-flash'


def local_depth(colour, k):
    """A height map made from the redrawn colour itself, with no depth model: its luminance with the broad shading taken out (a
    Gaussian of 8 original pixels, k redrawn pixels each), lightly smoothed, darker read as recessed (seams, grooves, cracks),
    its 1st to 99th percentile spread to 40 grey levels either side of 128. Shallower than a model's, aligned by construction.
    A spread under LOCAL_DEPTH_FLOOR luminance levels is not stretched to the full 40: a plain yellow panel's colour varies by
    under one level, and stretched it became relief as rough as lava's."""
    l = np.asarray(colour.convert('RGB'), np.float32) @ np.array([0.299, 0.587, 0.114], np.float32)
    hp = cv2.GaussianBlur(l - cv2.GaussianBlur(l, (0, 0), 8 * k), (0, 0), max(0.5, k / 4))
    lo, hi = np.percentile(hp, [1, 99])
    return Image.fromarray(np.clip(128 + hp / max(hi, -lo, LOCAL_DEPTH_FLOOR) * 40, 0, 255).astype(np.uint8))


# Grain and seams spread 7 to 15 levels or more (a slate metal's grain 7..10, paving seams 6..15) and keep their full relief.
LOCAL_DEPTH_FLOOR = 8.0


def credit_left(key):
    req = urllib.request.Request('https://openrouter.ai/api/v1/credits', headers={'Authorization': f'Bearer {key}'})
    with urllib.request.urlopen(req, timeout=60) as r:
        d = json.load(r)['data']
    return float(d['total_credits']) - float(d['total_usage'])


# A try of a colour and a depth costs about this (MODEL $0.0184, DEPTH_MODEL $0.0156); no texture starts with less than its
# most (tries of each) a worker left, so the textures in flight when the floor is reached cannot overrun the credit.
TRY_COST = 0.035


def generate(out, first, last, workers, tries=TRIES, ids=None, depth_model=DEPTH_MODEL, model=MODEL, prompt='default'):
    key = os.environ.get('OPENROUTER_API_KEY')
    if not key:
        sys.exit('OPENROUTER_API_KEY is not set')
    rows = rows_of(out)
    queue_file = json.loads((out / 'queue.json').read_text())
    queue = queue_file['items']
    cheap_from = queue_file.get('cheap_from_batch')
    # The last batch an image model draws (the player: "Go to 450 on the cheaper pass. Then 650 on local methods").
    last = min(last, queue_file['generate_until_batch'])
    # A texture already compiled (installed or accepted since the queue was last listed) is not drawn again.
    compiled = set(json.loads((out / '0-riftapart/rows.json').read_text())['relief']) if (out / '0-riftapart/rows.json').exists() else set()
    todo = sorted(((n, it['id']) for n, it in queue.items() if first <= it['batch'] <= last and not it['done']
                   and n not in compiled and (ids is None or it['id'] in ids)), key=lambda x: x[1])
    print(f'{len(todo)} textures of batches {first}-{last} to generate (colour {model}, depth {depth_model}; from batch {cheap_from}: '
          f'colour {CHEAP_MODEL}, depth made locally), {workers} at a time; '
          f'${credit_left(key):.2f} of credit left', flush=True)
    log = out / 'generate.log'
    total, passed, skipped = 0.0, 0, 0
    floor = TRY_COST * max(tries, 1) * workers
    out_of_credit = threading.Event()

    def one(n, qid):
        if out_of_credit.is_set():
            return None
        if credit_left(key) < floor:
            out_of_credit.set()
            return None
        if cheap_from is not None and queue[n]['batch'] >= cheap_from:
            return generate_one(key, out, n, qid, rows[n], tries, LOCAL_DEPTH, CHEAP_MODEL, prompt)
        return generate_one(key, out, n, qid, rows[n], tries, depth_model, model, prompt)

    with ThreadPoolExecutor(workers) as pool:
        futures = {pool.submit(one, n, qid): (n, qid) for n, qid in todo}
        for f in as_completed(futures):
            n, qid = futures[f]
            try:
                result = f.result()
                if result is None:
                    skipped += 1
                    continue
                ok, cost, notes = result
            except Exception as e:  # noqa: BLE001 - one texture's API failure is reported and the rest go on
                ok, cost, notes = False, 0.0, [f'error: {e}']
            total += cost
            passed += ok
            line = dict(id=qid, name=n, passed=ok, cost=round(cost, 4), notes=notes, date=time.strftime('%Y-%m-%d %H:%M:%S'))
            with open(log, 'a', encoding='utf-8') as fh:
                fh.write(json.dumps(line) + '\n')
            print(f"{qid} {'passed' if ok else 'FAILED'} ${cost:.3f}: {'; '.join(notes)}", flush=True)
    print(f'{passed} of {len(todo) - skipped} passed; ${total:.2f} this run; log {log}'
          + (f'; {skipped} not started: the credit left reached ${floor:.2f}' if skipped else ''), flush=True)


REASON = {'colour_shift_px': 'moved', 'colour_edges': 'lost structure', 'colour_difference': 'colours off', 'colour_novelty': 'invented detail'}


def report(out, since):
    """What the run since `since` (YYYY-MM-DD HH:MM) did, to tune it: each texture's outcome (first try, after a retry,
    failed, error), why each colour try failed, depth failures, provider blocks and errors, by the treatment the visual pass
    chose and by the texture's size and alpha. Written to <out>/run-report.md and printed."""
    rows = rows_of(out)
    seen = json.loads((out / 'descriptions.json').read_text(encoding='utf-8')) if (out / 'descriptions.json').exists() else {}
    last = {}
    for line in open(out / 'generate.log', encoding='utf-8'):
        d = json.loads(line)
        if d['date'] >= since:
            last[d['name']] = d
    def outcome(d):
        if any(n.startswith('error') for n in d['notes']):
            return 'error'
        if d['passed']:
            if any(n.startswith('kept') for n in d['notes']):
                return 'kept'
            made = [n for n in d['notes'] if re.match(r'colour \d+:', n)]
            return 'first try' if len(made) <= 1 else 'after a retry'
        return 'failed'
    outcomes, reasons, by_treat, depth_fails, blocks, errors, sizes = Counter(), Counter(), {}, 0, Counter(), Counter(), {}
    for n, d in last.items():
        o = outcome(d)
        outcomes[o] += 1
        t = seen.get(n, {}).get('treatment', '?') or 'none'
        by_treat.setdefault(t, Counter())[o] += 1
        k = f"{rows[n]['width']}px {rows[n]['alpha']}"
        sizes.setdefault(k, Counter())[o] += 1
        for note in d['notes']:
            m = re.search(r"FAILS \[(.*)\]", note)
            if note.startswith('colour') and m:
                for x in m[1].split(','):
                    reasons[REASON[x.strip(" '")]] += 1
            if note.startswith('depth') and 'FAILS' in note:
                depth_fails += 1
            if 'blocked the request' in note:
                blocks[note.split(' blocked')[0]] += 1
            if note.startswith('error'):
                errors[note[:120]] += 1
    cost = sum(d['cost'] for d in last.values())
    table = lambda c: ', '.join(f'{k} {v}' for k, v in c.most_common())
    lines = [f'# Run report since {since}', '', f'{len(last)} textures, ${cost:.2f}: {table(outcomes)}', '',
             f'Colour tries failed for: {table(reasons) or "none"}', f'Depth tries failed: {depth_fails}',
             f'Provider blocks (sent to the next model): {table(blocks) or "none"}', f'Errors: {table(errors) or "none"}', '',
             '## By treatment (the visual pass)', ''] + [f'- {t}: {table(c)}' for t, c in sorted(by_treat.items(), key=lambda x: -sum(x[1].values()))] + \
            ['', '## By size and alpha', ''] + [f'- {k}: {table(c)}' for k, c in sorted(sizes.items(), key=lambda x: -sum(x[1].values()))] + \
            ['', '## Failed', ''] + [f"- {d['id']} {rows[n]['width']}px {seen.get(n, {}).get('treatment', '?') or 'none'}: "
                                     + '; '.join(x for x in d['notes'] if 'FAILS' in x or x.startswith('error'))[:300]
                                     for n, d in last.items() if outcome(d) in ('failed', 'error')]
    (out / 'run-report.md').write_text('\n'.join(lines) + '\n', encoding='utf-8')
    print('\n'.join(lines[:14 + len(by_treat) + len(sizes)]))


def install(out, installed, backups):
    pack_dir = out / '0-riftapart'
    rows = json.loads((pack_dir / 'rows.json').read_text())['relief']
    if not rows:
        sys.exit(f'{pack_dir}: nothing compiled')
    stamp = time.strftime('%Y%m%d-%H%M%S')
    backup = backups / f'0-riftapart-texture-queue-{stamp}'
    record = dict(date=stamp, backup=str(backup), files=[], relief=sorted(rows))
    for n in rows:
        for rel in (f'replacements/{n}.png', f'relief/{n}.png'):
            src, dst = pack_dir / rel, installed / rel
            if not src.is_file():
                sys.exit(f'{src}: compiled row without its file')
            if dst.exists() and dst.stat().st_size == src.stat().st_size and dst.stat().st_mtime == src.stat().st_mtime:
                continue  # installed by an earlier install (copy2 keeps the compiled file's time)
            if dst.exists():
                (backup / rel).parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(dst, backup / rel)
            shutil.copy2(src, dst)
            record['files'].append(dict(file=rel, sha256=hashlib.sha256(dst.read_bytes()).hexdigest(), replaced=(backup / rel).exists()))
    path = installed / 'relief.tsv'
    backup.mkdir(parents=True, exist_ok=True)
    shutil.copy2(path, backup / 'relief.tsv')
    lines = path.read_text(encoding='utf-8').splitlines()
    notes = [l for l in lines if l.startswith('#')]
    body = [l for l in lines if l and not l.startswith('#')]
    kept = [l for l in body[1:] if canon(l.split('\t')[0] + '.png') not in rows]
    path.write_text('\n'.join(notes + [body[0]] + kept + [f'{n}\t{m}\t1' for n, m in rows.items()]) + '\n', encoding='utf-8')
    # The materials `materials` picked, added to surfaces.tsv; a surface given another material since the list was made
    # keeps that one.
    added = out / 'materials-added.tsv'
    if added.exists():
        picked = {r['stem']: r for r in csv.DictReader(added.open(encoding='utf-8'), delimiter='\t') if r['material'] != 'none'}
        path = installed / 'surfaces.tsv'
        shutil.copy2(path, backup / 'surfaces.tsv')
        lines = path.read_text(encoding='utf-8').splitlines()
        notes = [l for l in lines if l.startswith('#')]
        body = [l for l in lines if l and not l.startswith('#')]
        header = body[0].split('\t')
        there = {l.split('\t')[0] for l in body[1:]}
        new = ['\t'.join(str(picked[n][k]) for k in header) for n in sorted(picked) if n not in there]
        path.write_text('\n'.join(notes + body + new) + '\n', encoding='utf-8')
        record['surfaces'] = [l.split('\t')[0] for l in new]
        print(f'{len(new)} material rows added to surfaces.tsv ({len(set(picked) & there)} picked were there already)')
    (out / f'installed-{stamp}.json').write_text(json.dumps(record, indent=1) + '\n')
    print(f'installed {len(record["files"])} files and {len(rows)} relief rows in {installed}; backup {backup}')


def upscale(out, first, last, installed, weights, shard=(0, 1), made_since=None, ids=None):
    """The queue's batches past the image models' last (the player: "Then 650 on local methods", "can we do local upscaling
    too", "Can you upscale fan pack ones too"), with no image API, enlarged 4x by Real-ESRGAN on this PC's GPU
    (local_upscale.py): a texture a fan pack already enlarged 4x from its fan picture (the player, beside the original
    enlarged: "upscaled fan is better than upscaled original"), any other from the original. Each is brought to the redrawn
    size and given a depth made locally from it; the pair goes to <out>/returned, for intake's check like any. The model is
    the weights given (the player: "is there a better upscale model we can use?": Kim2091's 4x-PBRify_UpscalerV4, trained on
    game textures, kept the grain and mottling Real-ESRGAN smoothed away). A texture an image model already drew (its tries
    in <out>/attempts and its pair returned) keeps that pair."""
    from local_upscale import SpandrelUpscaler
    rows = rows_of(out)
    queue_file = json.loads((out / 'queue.json').read_text())
    # Named textures are taken whatever their batch (the player: "upscale the 138": those the image models never got through
    # sit in the image models' batches).
    first = max(first, queue_file['generate_until_batch'] + 1) if ids is None else first
    # Made again whole each time (it is deterministic): intake compiles again a pair newer than its compiled picture.
    drawn = {p.stem.rsplit('-colour-', 1)[0] for p in (out / 'attempts').glob('q*-colour-*.png')}
    # Not the textures used nowhere (the player: "we dont need to do the never used ones").
    todo = sorted((it['id'], n) for n, it in queue_file['items'].items() if first <= it['batch'] <= last and not it['done']
                  and (ids is None or it['id'] in ids) and it.get('ranked_by') != 'nothing'
                  and not (it['id'] in drawn and (out / 'returned' / f"{it['id']}-colour.png").exists()))
    # Most important first (the player: "can you make it go in batch order?"), so a pass stopped early has done the batches
    # that matter most.
    batch_of = {it['id']: it['batch'] for it in queue_file['items'].values()}
    todo.sort(key=lambda x: (batch_of[x[0]], x[0]))
    # Run as several processes at once (shard k of n takes every n-th texture), and taken up again after a stop: a texture
    # whose pair this pass already wrote (at or after made_since) is not made again.
    todo = todo[shard[0]::shard[1]]
    if made_since is not None:
        todo = [(q, n) for q, n in todo if not ((out / 'returned' / f'{q}-depth.png').exists()
                                                and (out / 'returned' / f'{q}-depth.png').stat().st_mtime >= made_since)]
    fan = {}
    for pack in sorted(installed.parent.iterdir()):
        if pack.is_dir() and pack != installed:
            for p in sorted(pack.rglob('*.png')):
                fan.setdefault(canon(p.name), p)
    up = SpandrelUpscaler(weights)
    (out / 'returned').mkdir(exist_ok=True)
    made = Counter()
    for qid, n in todo:
        r = rows[n]
        w, h = int(r['width']), int(r['height'])
        size = scaled_size(w, h)
        source = np.asarray(Image.open(fan[n]).convert('RGB')) if n in fan else read_rgba(r['picture'])[..., :3].copy()
        # A fan picture already at the redrawn size (two are 8x) is not enlarged again.
        big = up(source, r['tiling'] == 'yes') if max(source.shape[:2]) < SIZE else source
        colour = Image.fromarray(big).resize(size, Image.Resampling.LANCZOS)
        # Written whole under another name first: the run's intake may be reading returned/ meanwhile.
        for im, part in ((colour, 'colour'), (local_depth(colour, size[0] / w), 'depth')):
            im.save(out / 'returned' / f'{qid}-{part}.part', 'PNG')
            os.replace(out / 'returned' / f'{qid}-{part}.part', out / 'returned' / f'{qid}-{part}.png')
        made['fan picture' if n in fan else 'original'] += 1
    print(f'{len(todo)} textures of batches {first}-{last} enlarged by {weights.name}, each with a depth made from it: '
          + ', '.join(f'{v} from the {k}' for k, v in made.items()))


def enlarge(out, installed, backups, weights, stems_file):
    """The main cast's installed art taken from 1024 to 2048 (the player: "use it for ... the most important ones like ratchet
    and the galactic rangers and maybe some other things"): Kim2091's 4x-PBRify_UpscalerV4 over the installed picture, brought
    to twice its size, its alpha enlarged smoothly. Beside the 1024 art at 2048 it sharpened edges and bevels and kept the
    grain (Real-ESRGAN had smoothed it away). Only the cast seen up close: the player's gear is in every level, and all of it at
    2048 would add about 7 GB of video memory to each. The 1024 file is backed up before the 2048 one replaces it; a picture
    already 2048 is counted as done."""
    from local_upscale import SpandrelUpscaler
    stems = json.loads(stems_file.read_text())
    # A cast picture this queue redrew is installed from its compiled copy: the 2048 picture replaces that copy too, or the
    # next install puts the 1024 one back (it did, for 9 of the 84).
    compiled = set(json.loads((out / '0-riftapart/rows.json').read_text())['relief'])
    up = SpandrelUpscaler(weights)
    stamp = time.strftime('%Y%m%d-%H%M%S')
    backup = backups / f'0-riftapart-enlarged-{stamp}' / 'replacements'
    record, done = [], 0
    for n in stems:
        path = installed / 'replacements' / f'{n}.png'
        im = Image.open(path).convert('RGBA')
        if im.size[0] >= 2048 or im.size[1] >= 2048:
            done += 1
            continue
        size = (im.size[0] * 2, im.size[1] * 2)
        rgba = np.asarray(im)
        rgb = Image.fromarray(up(rgba[..., :3].copy(), False)).resize(size, Image.Resampling.LANCZOS)
        alpha = Image.fromarray(rgba[..., 3]).resize(size, Image.Resampling.LANCZOS)
        big = Image.merge('RGBA', (*rgb.split(), alpha))
        (out / 'enlarged').mkdir(exist_ok=True)
        big.save(out / 'enlarged' / f'{n}.png')
        backup.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, backup / f'{n}.png')
        shutil.copy2(out / 'enlarged' / f'{n}.png', path)
        if n in compiled:
            shutil.copy2(out / 'enlarged' / f'{n}.png', out / '0-riftapart' / 'replacements' / f'{n}.png')
        record.append(dict(stem=n, size=list(size), sha256=hashlib.sha256(path.read_bytes()).hexdigest()))
    (out / f'enlarged-{stamp}.json').write_text(json.dumps(dict(backup=str(backup), files=record), indent=1) + '\n')
    print(f'{len(record)} cast pictures installed at 2048 ({done} were already); the 1024 ones in {backup}')


MATCHER = 'anthropic/claude-sonnet-5.5'  # picks a material for 25 described surfaces a call, from the approved ones
UNITS_PER_METRE = 1.0967  # docs/surfaces-objects.csv's first line


def materials(out, first, last, installed, matches, surfaces_csv, workers):
    """A Rift Apart material for each bare level surface of the queue's batches first..last (the player: "Then 650 on local
    methods"; the method offered: grain, relief and gloss over the picture, as the approved surfaces have, each material picked
    from the visual pass's description). The choice is among the materials already approved on other surfaces (surfaces.tsv),
    each shown to the matcher with what the hand matches (textures/riftapart-matches.tsv) put it on and their tile sizes; a
    surface the hand matching found nothing for (flag none) is left out, as is one the matcher finds nothing for. Row values
    as riftapart-pair.py `write` makes them: tiles = the measured repeat in metres over the tile's metres (at least one), the
    material's reflectance, grain, normal, gloss and metal as its approved rows have them. The rows go to
    <out>/materials-picked.tsv; those that pass a look at the picture beside the material go to <out>/materials-added.tsv,
    which `install` merges into the player's surfaces.tsv."""
    key = os.environ.get('OPENROUTER_API_KEY')
    if not key:
        sys.exit('OPENROUTER_API_KEY is not set')
    rows = rows_of(out)
    queue = json.loads((out / 'queue.json').read_text())['items']
    lines = surfaces_csv.read_text(encoding='utf-8').splitlines()
    measured = {r['name']: r for r in csv.DictReader(lines[1:])}
    hand = list(csv.DictReader((l for l in matches.read_text(encoding='utf-8').splitlines() if l and not l.startswith('#')), delimiter='\t'))
    judged_none = {r['stem'] for r in hand if r['flag'] == 'none'}
    approved = list(csv.DictReader((l for l in (installed / 'surfaces.tsv').read_text(encoding='utf-8').splitlines() if l and not l.startswith('#')), delimiter='\t'))
    by_material, by_stem = {}, {r['stem'] for r in approved}
    for r in approved:
        by_material.setdefault(r['material'], []).append(r)
    # A bare metal's colour is its reflection (riftapart-pair.py): told to keep them for all-metal textures, the matcher still
    # put bronze on mixed atlases and anodised metal on painted panels, so they are not offered here; the hand matches have them.
    by_material = {m: rs for m, rs in by_material.items() if float(Counter(r['reflectance'] for r in rs).most_common(1)[0][0]) <= 0.3}
    # Every surface measured in a level, redrawn or not ("do we need to add materials for batches before 451": "We need to do
    # them"); characters, enemies and the moby families take theirs by region (regions.tsv), not here.
    todo = sorted((it['id'], n) for n, it in queue.items() if first <= it['batch'] <= last
                  and it.get('ranked_by') not in ('character', 'enemy', 'cutscenes', 'mobies')
                  and not rows[n]['material'] and n not in by_stem and n not in judged_none
                  and n in measured and float(measured[n]['repeat_u'] or 0) > 0 and float(measured[n]['repeat_v'] or 0) > 0)
    print(f'{len(todo)} bare surfaces of batches {first}-{last}; describing them', flush=True)
    with ThreadPoolExecutor(workers) as pool:
        seen = dict(zip((n for _, n in todo), pool.map(lambda x: describe_visually(key, out, rows[x[1]], read_rgba(rows[x[1]]['picture'])[..., :3]), todo)))
    catalogue = []
    for m in sorted(by_material):
        uses = [r for r in hand if r['material'] == m and r['flag'] != 'none']
        shows = list(dict.fromkeys(r['shows'] for r in sorted(uses, key=lambda r: len(r['shows']))))[:4]
        # A hand match's tile_m is metres, or xN (N tiles a repeat, where the repeat was not measured).
        metres = [float(r['tile_m']) for r in uses if re.fullmatch(r'[0-9.]+', r['tile_m'])]
        tile = f'usual tile {float(np.median(metres)):g} m; ' if metres else ''
        catalogue.append(f"- {m} ({tile}on: {'; '.join(shows) or 'no hand match'})")
    catalogue = '\n'.join(catalogue)
    path = out / 'materials-picked.tsv'
    done = {r['stem']: r for r in csv.DictReader(path.open(encoding='utf-8'), delimiter='\t')} if path.exists() else {}
    todo = [(q, n) for q, n in todo if n not in done]

    def pick(chunk):
        items = []
        for q, n in chunk:
            r, mm = rows[n], measured[n]
            ru, rv = float(mm['repeat_u']) / UNITS_PER_METRE, float(mm['repeat_v']) / UNITS_PER_METRE
            items.append(f"{q}: {r['width']} x {r['height']} px, one repeat covers {ru:.2f} x {rv:.2f} m in the level, kind label "
                         f"{r['kind'] or 'none'}" + (f", catalogued as {r['about']}" if r['about'] else '') + f"; {seen[n]['description']}")
        ask = (f"These are surface textures of a PS2 platformer's levels and props. Each gets a Rift Apart material laid over its own picture: "
               f"only the material's fine grain, surface relief and gloss show, tiled at a real-world size; the texture keeps its "
               f"own colours. Pick for each the material from this list whose physical surface matches what the texture shows (painted "
               f"metal for painted metal, stone for stone, bark for bark), or \"none\" when nothing in the list fits or the texture is "
               f"not a physical surface (a glow, a lit or glowing panel, a sky, a decal, a screen, an effect). One material covers the "
               f"whole texture: for an atlas of differently coloured parts choose the material most of its area is. Give the world "
               f"size in metres of one tile of the "
               f"material, judged from what the texture shows and how big one repeat is (bricks, planks and panels set the scale).\n\n"
               f"Materials (with what the hand matching put them on):\n{catalogue}\n\nTextures:\n" + '\n'.join(items) +
               f"\n\nAnswer with a JSON list only, one entry per texture: "
               f"[{{\"id\": \"q01234\", \"material\": \"name or none\", \"tile_m\": 1.0, \"shows\": \"a few words: what the surface is\"}}]")
        body = json.dumps(dict(model=MATCHER, max_tokens=8000, messages=[dict(role='user', content=ask)])).encode()
        req = urllib.request.Request('https://openrouter.ai/api/v1/chat/completions', body, headers={'Authorization': f'Bearer {key}', 'Content-Type': 'application/json'})
        # OpenRouter refuses a request while the image run's requests in flight hold the balance (402
        # in_flight_budget_exhausted) and says when to retry; any other refusal stops the step.
        for attempt in range(30):
            try:
                with urllib.request.urlopen(req, timeout=600) as resp:
                    answer = json.load(resp)
                break
            except urllib.error.HTTPError as e:
                text = e.read().decode()
                if e.code != 402 or 'in_flight_budget_exhausted' not in text or attempt == 29:
                    raise RuntimeError(f'the matcher: HTTP {e.code}: {text[:600]}') from e
                time.sleep(int(e.headers.get('Retry-After') or 120))
        text = answer['choices'][0]['message']['content']
        got = json.loads(text[text.index('['):text.rindex(']') + 1])
        want = {q for q, _ in chunk}
        if not {g['id'] for g in got} <= want:
            raise RuntimeError(f'the matcher answered for {sorted(g["id"] for g in got)}, not {sorted(want)}')
        bad = [g for g in got if g['material'] != 'none' and g['material'] not in by_material]
        if bad:
            raise RuntimeError(f'the matcher chose materials not on the list: {bad}')
        return got, float(answer.get('usage', {}).get('cost') or 0)

    name_of = {q: n for q, n in todo}
    fields = ['stem', 'id', 'shows', 'material', 'tile_m', 'tiles_u', 'tiles_v', 'reflectance', 'grain', 'normal', 'specular', 'metal']

    def pick_round(todo):
        cost = 0.0
        with ThreadPoolExecutor(min(workers, 8)) as pool:
            for got, c in pool.map(pick, [todo[i:i + 25] for i in range(0, len(todo), 25)]):
                cost += c
                for g in got:
                    n = name_of[g['id']]
                    if g['material'] == 'none':
                        done[n] = dict(stem=n, id=g['id'], shows=g['shows'], material='none', tile_m='', tiles_u='', tiles_v='',
                                       reflectance='', grain='', normal='', specular='', metal='')
                        continue
                    look = Counter((r['reflectance'], r['grain'], r['normal'], r['specular'], r['metal']) for r in by_material[g['material']]).most_common(1)[0][0]
                    mm, tm = measured[n], float(g['tile_m'])
                    done[n] = dict(stem=n, id=g['id'], shows=g['shows'], material=g['material'], tile_m=f'{tm:g}',
                                   tiles_u=max(1, round(float(mm['repeat_u']) / UNITS_PER_METRE / tm)),
                                   tiles_v=max(1, round(float(mm['repeat_v']) / UNITS_PER_METRE / tm)),
                                   **dict(zip(('reflectance', 'grain', 'normal', 'specular', 'metal'), look)))
                # Written whole and swapped in: the run's `install` may read it at any moment, and a broken surfaces.tsv stops the game.
                part = path.with_suffix('.part')
                with part.open('w', encoding='utf-8', newline='\n') as f:
                    w = csv.DictWriter(f, fields, delimiter='\t', lineterminator='\n')
                    w.writeheader()
                    w.writerows(done.values())
                replace_when_free(part, path)
        return cost

    # A texture the matcher left out of its answer is asked again in a later round, three rounds at most.
    cost = 0.0
    for _ in range(3):
        todo = [(q, n) for q, n in todo if n not in done]
        if not todo:
            break
        cost += pick_round(todo)
    missing = [q for q, n in name_of.items() if n not in done]
    print(f"{sum(r['material'] != 'none' for r in done.values())} surfaces given a material, "
          f"{sum(r['material'] == 'none' for r in done.values())} none; ${cost:.2f}; {path}"
          + (f'; the matcher never answered for {missing}' if missing else ''))


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    sub = ap.add_subparsers(dest='cmd', required=True)
    l = sub.add_parser('list')
    for a in ('uya', 'iso', 'installed', 'dumps', 'review', 'out'):
        l.add_argument(a, type=Path)
    p = sub.add_parser('pack')
    p.add_argument('out', type=Path)
    p.add_argument('batches')
    i = sub.add_parser('intake')
    i.add_argument('out', type=Path)
    i.add_argument('sources', type=Path, nargs='*')
    i.add_argument('--generated', action='store_true')
    i.add_argument('--accept', default='', help='queued textures the player approved past the check, comma-separated')
    i.add_argument('--shard', default='0/1', help='k/n: this compile checks every n-th pair from the k-th')
    s = sub.add_parser('install')
    for a in ('out', 'installed', 'backups'):
        s.add_argument(a, type=Path)
    rp = sub.add_parser('report')
    rp.add_argument('out', type=Path)
    rp.add_argument('since', help='YYYY-MM-DD HH:MM')
    k = sub.add_parser('rank')
    k.add_argument('out', type=Path)
    k.add_argument('review', type=Path)
    k.add_argument('gameplay', type=Path, help='the saved gameplay frames, one folder a level (captures/gameplay)')
    k.add_argument('first', type=int, help='the first batch ranked again (batches before it keep their textures)')
    g = sub.add_parser('generate')
    g.add_argument('out', type=Path)
    g.add_argument('batches')
    g.add_argument('--workers', type=int, default=4)
    g.add_argument('--tries', type=int, default=TRIES, help='new colour tries a texture gets beyond those already made')
    g.add_argument('--ids', help='only these queued textures, comma-separated (q00001,q00002)')
    g.add_argument('--depth-model', default=DEPTH_MODEL, help='the model drawing the depth')
    g.add_argument('--model', default=MODEL, help='the model drawing the colour')
    g.add_argument('--prompt', default='default', choices=('default', 'soft', 'v2'), help="the colour's prompt (`colour_prompt`; v2: `reconstruction_prompt`)")
    u = sub.add_parser('upscale')
    u.add_argument('out', type=Path)
    u.add_argument('batches')
    u.add_argument('installed', type=Path, help="the player's texture_replacements/0-riftapart (fan packs are its siblings)")
    u.add_argument('weights', type=Path, help='a 4x model spandrel reads (4x-PBRify_UpscalerV4.safetensors)')
    u.add_argument('--shard', default='0/1', help='k/n: this process takes every n-th texture from the k-th')
    u.add_argument('--made-since', help='YYYY-MM-DD HH:MM: textures whose pair was written since then are not made again')
    u.add_argument('--ids', help='only these queued textures, comma-separated, whatever their batch')
    mt = sub.add_parser('materials')
    mt.add_argument('out', type=Path)
    mt.add_argument('batches')
    mt.add_argument('installed', type=Path, help="the player's texture_replacements/0-riftapart")
    mt.add_argument('--matches', type=Path, default=ROOT / 'textures/riftapart-matches.tsv')
    mt.add_argument('--surfaces', type=Path, default=ROOT / 'docs/surfaces-objects.csv')
    mt.add_argument('--workers', type=int, default=16)
    el = sub.add_parser('enlarge')
    for a in ('out', 'installed', 'backups', 'weights', 'stems'):
        el.add_argument(a, type=Path)
    args = ap.parse_args()
    if args.cmd == 'enlarge':
        return enlarge(args.out, args.installed, args.backups, args.weights, args.stems)
    if args.cmd == 'materials':
        a, _, b = args.batches.partition('-')
        return materials(args.out, int(a), int(b or a), args.installed, args.matches, args.surfaces, args.workers)
    if args.cmd == 'upscale':
        a, _, b = args.batches.partition('-')
        k, n = map(int, args.shard.split('/'))
        since = time.mktime(time.strptime(args.made_since, '%Y-%m-%d %H:%M')) if args.made_since else None
        return upscale(args.out, int(a), int(b or a), args.installed, args.weights, (k, n), since,
                       set(args.ids.split(',')) if args.ids else None)
    if args.cmd == 'generate':
        a, _, b = args.batches.partition('-')
        return generate(args.out, int(a), int(b or a), args.workers, args.tries,
                        set(args.ids.split(',')) if args.ids else None, args.depth_model, args.model, args.prompt)
    if args.cmd == 'report':
        return report(args.out, args.since)
    if args.cmd == 'rank':
        return rank(args.out, args.review, args.gameplay, args.first)
    if args.cmd == 'list':
        list_(args.uya, args.iso, args.installed, args.dumps, args.review, args.out)
    elif args.cmd == 'pack':
        a, _, b = args.batches.partition('-')
        pack(args.out, int(a), int(b or a))
    elif args.cmd == 'intake':
        k, n = map(int, args.shard.split('/'))
        intake(args.out, args.sources, args.generated, frozenset(x for x in args.accept.split(',') if x), (k, n))
    else:
        install(args.out, args.installed, args.backups)


if __name__ == '__main__':
    main()
