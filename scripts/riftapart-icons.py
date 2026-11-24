"""The HUD's pictures and the menus' see-through panels redrawn by the player, put in the Rift Apart folder's `replacements/` (PLAN 9.2; port/uya/src/texnames.rs
draws them ahead of the packs while RIFT APART SURFACES is on).

Usage:
  py -3.12 scripts/riftapart-icons.py cutouts <disc.iso> <out dir>
  py -3.12 scripts/riftapart-icons.py install <disc.iso> <art dir> <replacements dir> [--sheet PNG]

Every picture the HUD draws is in the LEVEL files (port/uyafmt/src/hud.rs): the level data's part 3 is the HUD header (sprite
table, frame table of (palette, texture) pairs, palette table, texture table) and parts 4-8 are HUD banks 0-4 (WAD LZ). A
picture is one (texture, palette) pair, once however many levels have it. Its id, which names the files, is the first 12 hex
digits of the SHA-1 of its RGBA (palette in index order, alpha on the GS scale, 0x80 opaque) and its log2 width and height.

The panels are 2FIP images (port/uyafmt/src/pif.rs) in WAD8 entries the level code loads (headers.rs names the readers): the
QUICK SELECT screen's octagons (1123) and the CONTROLS screen's controller (71), seen in play; the three yellow tech panels
loaded with the weapon mods' cards (1093-1095), the ship map's orbit and frame (1122, 1253) and the arena menu's frame (1353).
A panel's id is the first 12 hex digits of the SHA-1 of its RGBA.

cutouts writes each picture at 512 texels on its long side (a panel at 1024, or at the shape the game shows it), blocky, for
an image generator to redraw:
- white\\  a picture in one grey (the weapons', gadgets' and menus' icons, which the game tints as it draws them): white on
  black, its brightness the texel's alpha times its grey (128 is white);
- colour\\  any other picture (and the SHADED_GREYS): its colours as they are over flat magenta (#FF00FF), by its alpha;
- panels\\  a panel as the game shows it over black: each colour times its opacity, so a fainter part is darker;
- weapons\\  a WEAPONS picture the same way, over black;
- items\\  an ITEMS picture (the picture beside the item's text) the same way, over black, at ITEM_TARGET: the reference for
  the item's place and size in its new picture;
- store\\  a vendor picture the same way, over black, enlarged 4x with hard texels: the reference sent to the image session
  (captures/store/prompt.md), which draws it again at STORE_TARGET.

install reads the redrawn pictures from <art dir> (any image whose name starts with a picture's id, in any folder in it), turns
each back into a texture of the game's form and writes it under both PCSX2 names it is drawn by (sampled as PSMT8 and as
PSMT8H: uya_pcsx2name.stem), 512 on its long side, alpha on the GS scale:
- white: its colour the picture's own greys, its alpha the brightness over that grey;
- colour: each art pixel read as one of the PS2 picture's colours around it over the magenta, its share the alpha, its colour
  with that magenta taken out; a colour of the art's own keeps its colour and the PS2 picture's opacity there. The art's
  brightness is kept (the cutouts are at the game's);
- panel: each pixel read as one of the PS2 panel's own colours around it times a share, the share giving its opacity;
- weapon: solid inside the PS2 picture's solid core (the weapon and its glow), elsewhere read as a panel is, with the art's
  colours;
- store: as a weapon is, the art at STORE_TARGET (RGB over black, as the cutout shows the picture; another size stops the run);
- item: the new picture as it is, the item alone (RGBA, ITEM_TARGET square, clear around the item): its colours, its
  opacity on the GS scale; nothing scaled or added. Its clear pixels take the colour of the nearest visible one, so the item's
  edge filters to no dark or orange rim.
A file whose name starts with no picture's id stops the run, naming it, and so do two files for one id. --sheet writes each
picture as the game has it beside its new form, on the menus' orange.
"""
import argparse
import hashlib
import importlib.util
import re
import struct
import sys
from pathlib import Path

import cv2
import numpy as np
from PIL import Image, ImageDraw, ImageFont
from scipy import ndimage

_here = Path(__file__).resolve().parent
def _load(name):
    spec = importlib.util.spec_from_file_location(name, _here / f'{name}.py')
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod
disc = _load('uya_disc')
pcsx2 = _load('uya_pcsx2name')

SECTOR = 0x800
TOC_SECTOR, TOC_SECTORS = 1001, 16
LEVEL_TABLE, LEVEL_SLOTS = 0x7400, 60
TARGET = 512
GS_OPAQUE = 128
MAGENTA = np.array([255.0, 0.0, 255.0])
WHITE_SAT = 14      # a texel this close to grey (max - min of R, G, B) is grey
WHITE_SHARE = 0.97  # a picture is white when this share of its visible texels are grey
EDGE = 1.5          # texels: a colour picture's art pixel is read against the PS2 texels this near it
PURE_MAGENTA = 15.0  # an art pixel this near the magenta (RGB distance) is clear
OWN_COLOUR = 60.0    # an art pixel this far from every line between a nearby PS2 colour and the magenta is the art's own colour
# Grey pictures that are shaded greys, not one grey tinted (the bars of levels 30-36 and the memory card): their darker greys
# are solid darkened parts, which art drawn white on black cannot tell from transparency (a black gap between the bar's
# segments came out clear). They are colour pictures: cut out over magenta at their own greys.
SHADED_GREYS = {'28a3652d2053', '27b3e311976e', '4a4f25af945f'}
PANELS = [1123, 71, 1093, 1094, 1095, 1122, 1253, 1353]  # WAD8 entries
# The WEAPONS tab's pictures: WAD8 entries 878-977 (func_L01_004832F8 loads them from base pair 868), weapon w's upgrade level l
# at 878 + 5 (w - 1) + (l - 1). 128 x 128: the weapon, solid, inside an opaque orange glow, over a see-through orange ring whose
# lines are in its opacity alone (its colour is one orange).
WEAPONS_FIRST = 878
# The ITEMS screen's pictures: WAD8 entries 979-995. The item table's word +0xC is the item's picture number n
# (func_L28_00450138 gives it to the picture element), and the element streams WAD8 entry 978 + n into its slot (level 28's
# RAM: the slot's pair table is entry 979's, its entry n - 1). The WEAPONS pictures' form: 128 x 128, the item solid in an
# opaque orange glow over the see-through orange ring. Entry 995 (number 17, the empty cells' and the sixth vid-comic's) is
# clear: nothing to redraw.
ITEMS_FIRST, ITEMS_COUNT = 979, 17
# The vendor's pictures: WAD8 entries 1052-1092 and 1098-1116, the WEAPONS pictures' form (128 x 128, the item solid in an
# opaque orange glow over the see-through orange ring and circuit lines): 1052-1071 each weapon's ammo (the Gadgetron
# canister over that weapon's outline), 1072-1091 the weapons as the vendor sells them, 1092 MAX OUT AMMO, 1098-1116 the
# weapons at their last upgrade. 31 of their 59 pictures are WEAPONS pictures too (the Rift Inducer's, 1087, is weapon 16's
# levels 1-4): drawn again here, they are drawn so in both places. Seen in the Phoenix's weapon vendor (the player's play.log
# names them, 23 November 2026: MAX OUT AMMO, 8fc0503808545070-2c9844acd4b6877a-00001dd3, is entry 1092's).
STORE_ENTRIES = list(range(1052, 1093)) + list(range(1098, 1117))
STORE_TARGET = 1024  # a vendor picture's side as its art is made (the vendor draws it about 620 x 500 px at 1440 lines)
ITEM_TARGET = 1024  # an item picture's side, as its art is made (the ITEMS screen draws it about 750 x 500 px at 1440 lines)
WEAPON_CORE = 0.95  # PS2 opacity from which a weapon picture's texel is its solid core (the weapon and its glow)
FIT_TIE = 1.0       # fits this near in distance are equal; the most opaque is taken
PANEL_REACH = 2     # texels: a panel pixel is read as one of the PS2 panel's colours this near it
PANEL_TARGET = 1024  # a panel's long side (they are drawn large: the QUICK SELECT panel about 540 px at 1080 lines)
# A panel the game stretches, at the shape it shows it: the CONTROLS controller (512 x 128) fills about 1150 x 460 px at 1080
# lines, so it is drawn at 1280 x 512 (its texture coordinates are 0 to 1 whatever its size).
PANEL_SHOWN = {'7702aa760bbc': (1280, 512)}
WAD8 = 7            # its header's index among the table of contents' eight


def u32(b, o):
    return struct.unpack_from('<I', b, o)[0]


def u16(b, o):
    return struct.unpack_from('<H', b, o)[0]


# The CLUT's CSM1 order: index bits 3 and 4 swapped (eedraw.rs hud_decode).
_x = np.arange(256)
CSM1 = (_x & ~0x18) | ((_x & 8) << 1) | ((_x & 0x10) >> 1)


def hud_pictures(iso_path):
    """{id: picture} of every HUD frame's (texture, palette) in every level: indices, clut (index order), rgba, size."""
    pictures = {}
    with open(iso_path, 'rb') as iso:
        def sectors(s, n):
            iso.seek(s * SECTOR)
            return iso.read(n * SECTOR)
        toc = sectors(TOC_SECTOR, TOC_SECTORS)
        for level in range(LEVEL_SLOTS):
            e = LEVEL_TABLE + level * 24 + 8  # the slot's LEVEL pair (header sector, file sectors)
            hs, n = u32(toc, e), u32(toc, e + 4)
            if hs == 0 and n == 0:
                continue
            h = sectors(hs, 1)
            if u32(h, 0) != 0x60:
                sys.exit(f'level {level}: LEVEL header size {u32(h, 0):#x}, not 0x60')
            off, cnt = u32(h, 0x10), u32(h, 0x14)  # block 0, the level data
            if cnt == 0:
                continue
            b = sectors(u32(h, 4) + off, cnt)
            parts = [(u32(b, 8 * k), u32(b, 8 * k + 4)) for k in range(10)]
            po, ps = parts[3]
            if ps == 0:
                continue
            hd = b[po:po + ps]
            n1, n2 = u16(hd, 0), u16(hd, 2)
            t1, t2, t3, t4 = (u32(hd, i) for i in (4, 8, 0xc, 0x10))
            cum3 = [u32(hd, 0x14 + 4 * i) for i in range(5)]
            cum4 = [u32(hd, 0x34 + 4 * i) for i in range(5)]
            sizes = [u32(hd, 0x54 + 4 * i) for i in range(5)]
            if t1 != 0xb4 or t2 != t1 + 8 * n1 or t3 != t2 + 4 * n2 or t4 != t3 + 8 * cum3[4] or t4 + 8 * cum4[4] != ps:
                sys.exit(f'level {level}: HUD header tables at {t1:#x}, {t2:#x}, {t3:#x}, {t4:#x} for counts {n1}, {n2}, {cum3[4]}, {cum4[4]} in {ps:#x} bytes')
            pals, texs = [], []
            for k in range(5):
                for i in range(cum3[k - 1] if k else 0, cum3[k]):
                    pals.append((k, u32(hd, t3 + 8 * i) & 0x7fffffff))
                for i in range(cum4[k - 1] if k else 0, cum4[k]):
                    texs.append((k, u32(hd, t4 + 8 * i) & 0x7fffffff, hd[t4 + 8 * i + 6], hd[t4 + 8 * i + 7]))
            banks = []
            for k in range(5):
                o, s = parts[4 + k]
                if sizes[k] == 0:
                    banks.append(None)
                    continue
                data = disc.wad_decompress(b[o:o + s])
                if len(data) != sizes[k]:
                    sys.exit(f'level {level}: HUD bank {k} unpacks to {len(data):#x} bytes, the header says {sizes[k]:#x}')
                banks.append(data)
            for si in range(n1 - 1):  # the last entry is the 0xFFFF end
                e = t1 + 8 * si
                count, first = u16(hd, e + 2), u16(hd, e + 4)
                for fi in range(first, first + count):
                    pi, ti = u16(hd, t2 + 4 * fi), u16(hd, t2 + 4 * fi + 2)
                    tb, to, lw, lh = texs[ti]
                    pb, pofs = pals[pi]
                    if banks[tb] is None or banks[pb] is None:
                        sys.exit(f'level {level}: HUD frame {fi} uses empty bank {tb if banks[tb] is None else pb}')
                    w, hh = 1 << lw, 1 << lh
                    indices = np.frombuffer(banks[tb][to:to + w * hh], np.uint8).reshape(hh, w)
                    clut = np.frombuffer(banks[pb][pofs:pofs + 0x400], np.uint8).reshape(256, 4)[CSM1]
                    rgba = clut[indices]
                    pid = hashlib.sha1(rgba.tobytes() + bytes([lw, lh])).hexdigest()[:12]
                    if pid not in pictures:
                        pictures[pid] = dict(id=pid, indices=indices, clut=clut, rgba=rgba, w=w, h=hh, levels=set())
                    pictures[pid]['levels'].add(level)
    if not pictures:
        sys.exit(f'{iso_path}: no HUD pictures')
    return pictures


def wad8_pictures(iso_path, entries, kind):
    """{id: picture} of the given WAD8 entries: each one's 2FIP image (raw, or in a WAD LZ stream), its palette in index
    order; a picture that several entries hold once, with all of them (`entries`)."""
    pictures = {}
    with open(iso_path, 'rb') as iso:
        def sectors(s, n):
            iso.seek(s * SECTOR)
            return iso.read(n * SECTOR)
        toc = sectors(TOC_SECTOR, TOC_SECTORS)
        at = 0
        for _ in range(WAD8):
            at += u32(toc, at)
        base = u32(toc, at + 4)
        for pair in entries:
            o, n = u32(toc, at + 8 + 8 * pair), u32(toc, at + 12 + 8 * pair)
            b = sectors(base + o, n)
            if b[:3] == b'WAD':
                b = disc.wad_decompress(b)
            if b[:4] != b'2FIP' or u32(b, 0x10) != 0x13 or u32(b, 0x14) != 0:
                sys.exit(f'WAD8 entry {pair}: not a 2FIP image with a 32-bit palette ({b[:4]!r}, {u32(b, 0x10):#x}, {u32(b, 0x14):#x})')
            w, h = u32(b, 8), u32(b, 0xc)
            clut = np.frombuffer(b[0x20:0x420], np.uint8).reshape(256, 4)[CSM1]
            indices = np.frombuffer(b[0x420:0x420 + w * h], np.uint8).reshape(h, w)
            rgba = clut[indices]
            pid = hashlib.sha1(rgba.tobytes()).hexdigest()[:12]
            if pid not in pictures:
                pictures[pid] = dict(id=pid, indices=indices, clut=clut, rgba=rgba, w=w, h=h, kind=kind, entries=[])
            pictures[pid]['entries'].append(pair)
    return pictures


def panel_pictures(iso_path):
    """{id: picture} of the PANELS."""
    return wad8_pictures(iso_path, PANELS, 'panel')


def weapon_pictures(iso_path):
    """{id: picture} of the WEAPONS pictures: 20 weapons, 5 upgrade levels each, WAD8 entries WEAPONS_FIRST on; a weapon's
    levels that share a picture give it once. Each picture's `weapon` (1-20) and `levels` (1-5)."""
    pictures = wad8_pictures(iso_path, range(WEAPONS_FIRST, WEAPONS_FIRST + 100), 'weapon')
    for p in pictures.values():
        weapons = {(e - WEAPONS_FIRST) // 5 + 1 for e in p['entries']}
        if len(weapons) != 1:
            sys.exit(f"weapon picture {p['id']}: held by entries of more than one weapon: {p['entries']}")
        p['weapon'] = weapons.pop()
        p['levels'] = sorted((e - WEAPONS_FIRST) % 5 + 1 for e in p['entries'])
    return pictures


def item_pictures(iso_path):
    """{id: picture} of the ITEMS pictures that show anything (WAD8 entries ITEMS_FIRST on; the clear one left out), each
    one's `number` (the item table's picture number, 1-17)."""
    pictures = wad8_pictures(iso_path, range(ITEMS_FIRST, ITEMS_FIRST + ITEMS_COUNT), 'item')
    for p in pictures.values():
        p['number'] = p['entries'][0] - ITEMS_FIRST + 1
    return {k: p for k, p in pictures.items() if p['rgba'][..., 3].any()}


def store_pictures(iso_path):
    """{id: picture} of the vendor's pictures (STORE_ENTRIES)."""
    return wad8_pictures(iso_path, STORE_ENTRIES, 'store')


def kind_of(p):
    return p.get('kind') or ('white' if is_white(p['rgba']) and p['id'] not in SHADED_GREYS else 'colour')


def is_white(rgba):
    a = rgba.astype(np.int32)
    vis = a[..., 3] > 32
    if not vis.any():
        return False
    sat = a[..., :3].max(-1) - a[..., :3].min(-1)
    return (sat[vis] <= WHITE_SAT).mean() >= WHITE_SHARE


def scale_of(p):
    return max(1, TARGET // max(p['w'], p['h']))


def size_of(p):
    """The size a picture is cut out and installed at: TARGET on its long side (a whole multiple of its texels), a panel
    PANEL_TARGET, or the shape the game shows it at (PANEL_SHOWN)."""
    if kind_of(p) == 'item':
        return (ITEM_TARGET, ITEM_TARGET)
    if kind_of(p) == 'store':
        return (STORE_TARGET, STORE_TARGET)
    if kind_of(p) == 'panel':
        if p['id'] in PANEL_SHOWN:
            return PANEL_SHOWN[p['id']]
        k = max(1, PANEL_TARGET // max(p['w'], p['h']))
        return (p['w'] * k, p['h'] * k)
    k = scale_of(p)
    return (p['w'] * k, p['h'] * k)


def grey_of(rgba):
    """A white picture's own grey: the median grey of its most solid texels (alpha within 10 % of its highest)."""
    a = rgba.astype(np.float32)
    al = a[..., 3]
    solid = al >= 0.9 * al.max()
    return float(np.median(a[..., :3][solid].mean(-1)))


def cutout(p):
    a = p['rgba'].astype(np.float32)
    al = np.clip(a[..., 3] / GS_OPAQUE, 0, 1)
    if kind_of(p) == 'store':
        return Image.fromarray((a[..., :3] * al[..., None]).round().astype(np.uint8)).resize((p['w'] * 4, p['h'] * 4), Image.NEAREST)
    if kind_of(p) in ('panel', 'weapon', 'item'):
        img = Image.fromarray((a[..., :3] * al[..., None]).round().astype(np.uint8))
    elif kind_of(p) == 'white':
        img = Image.fromarray((al * np.clip(a[..., :3].mean(-1) / GS_OPAQUE, 0, 1) * 255).astype(np.uint8)).convert('RGB')
    else:
        img = Image.fromarray((a[..., :3] * al[..., None] + MAGENTA * (1 - al[..., None])).round().astype(np.uint8))
    return img.resize(size_of(p), Image.NEAREST)


def from_white(p, art):
    """A white picture's redrawn form: the picture's own greys (each texel's, the clear texels taking their nearest visible
    texel's, upsampled smoothly), the art's brightness over that grey as the alpha."""
    a = p['rgba'].astype(np.float32)
    grey = a[..., :3].mean(-1)
    _, (iy, ix) = ndimage.distance_transform_edt(a[..., 3] <= 4, return_indices=True)
    b = np.asarray(art.convert('RGB'), np.float32).mean(-1) / 255
    size = (b.shape[1], b.shape[0])
    G = np.clip(cv2.resize(grey[iy, ix], size, interpolation=cv2.INTER_CUBIC), 1, 255)
    rgb = np.repeat(G[..., None], 3, -1)
    alpha = np.clip(b / np.minimum(1.0, G / GS_OPAQUE), 0, 1)
    return rgb, alpha


def from_colour(p, art):
    """A colour picture's redrawn form (see the module's doc). Each art pixel is read as one of the PS2 picture's own colours c
    around it (its texels within EDGE texels, the one whose line to the magenta passes nearest) over the magenta, p = a c +
    (1 - a) M: a is its alpha, and its colour is p with that magenta taken out, so no magenta is left in it. A pixel far from
    every such line is a colour of the art's own: it keeps its colour and takes the PS2 picture's opacity there."""
    px = np.asarray(art.convert('RGB'), np.float32)
    H, W = px.shape[:2]
    rgba = p['rgba'].astype(np.float32)
    al = np.clip(rgba[..., 3] / GS_OPAQUE, 0, 1)
    reach = max(1, int(round(EDGE)))
    ty = np.minimum(np.arange(H) * p['h'] // H, p['h'] - 1)
    tx = np.minimum(np.arange(W) * p['w'] // W, p['w'] - 1)
    best = np.full((H, W), np.inf, np.float32)
    alpha = np.zeros((H, W), np.float32)
    near_opacity = np.zeros((H, W), np.float32)
    v = px - MAGENTA
    for dy in range(-reach, reach + 1):
        for dx in range(-reach, reach + 1):
            yy = np.clip(ty + dy, 0, p['h'] - 1)[:, None]
            xx = np.clip(tx + dx, 0, p['w'] - 1)[None, :]
            visible = al[yy, xx] > 0.02
            near_opacity = np.maximum(near_opacity, al[yy, xx])
            d = rgba[..., :3][yy, xx] - MAGENTA
            a = np.clip((v * d).sum(-1) / np.maximum((d * d).sum(-1), 1.0), 0, 1)
            dist = np.where(visible, np.linalg.norm(v - a[..., None] * d, axis=-1), np.inf)
            closer = dist < best
            best = np.where(closer, dist, best)
            alpha = np.where(closer, a, alpha)
    own = best > OWN_COLOUR
    alpha = np.where(own, near_opacity, alpha)
    alpha = np.where(np.linalg.norm(v, axis=-1) < PURE_MAGENTA, 0.0, alpha)
    rgb = np.where(own[..., None], px, (px - (1 - alpha[..., None]) * MAGENTA) / np.maximum(alpha[..., None], 0.05))
    return np.clip(rgb, 0, 255), alpha


def from_panel(p, art):
    """A panel's redrawn form from art drawn as the game shows the panel over black (each colour times its opacity): each
    pixel is read as one of the panel's own colours c (its texels within PANEL_REACH texels of it, the one whose line from
    black passes nearest) at a share s of it, p = s c; the alpha is s times c's opacity, the colour c."""
    px = np.asarray(art.convert('RGB'), np.float32)
    H, W = px.shape[:2]
    rgba = p['rgba'].astype(np.float32)
    al = np.clip(rgba[..., 3] / GS_OPAQUE, 0, 1)
    shown = rgba[..., :3] * al[..., None]  # the panel over black
    ty = np.minimum(np.arange(H) * p['h'] // H, p['h'] - 1)
    tx = np.minimum(np.arange(W) * p['w'] // W, p['w'] - 1)
    best = np.full((H, W), np.inf, np.float32)
    alpha = np.zeros((H, W), np.float32)
    rgb = np.zeros((H, W, 3), np.float32)
    for dy in range(-PANEL_REACH, PANEL_REACH + 1):
        for dx in range(-PANEL_REACH, PANEL_REACH + 1):
            yy = np.clip(ty + dy, 0, p['h'] - 1)[:, None]
            xx = np.clip(tx + dx, 0, p['w'] - 1)[None, :]
            c = shown[yy, xx]
            cc = (c * c).sum(-1)
            s = np.clip((px * c).sum(-1) / np.maximum(cc, 1.0), 0, 1)
            s = np.where(cc < 1.0, 0.0, s)  # a clear texel
            d = np.linalg.norm(px - s[..., None] * c, axis=-1)
            closer = d < best - 1e-3
            best = np.where(closer, d, best)
            alpha = np.where(closer, s * al[yy, xx], alpha)
            rgb = np.where(closer[..., None], rgba[..., :3][yy, xx], rgb)
    return rgb, alpha


def from_weapon(p, art):
    """A weapon picture's redrawn form from art drawn as the game shows it over black. Inside the PS2 picture's solid core
    (its texels at least WEAPON_CORE opaque, shrunk by a texel: the weapon and its glow) the art is solid. Elsewhere each pixel
    is read as one of the PS2 picture's colours c around it (within PANEL_REACH texels) at a share s, p = s c over black: its
    alpha is s times c's opacity (of near-equal fits the most opaque, as the ring's orange is one colour at many opacities), its
    colour p over that alpha. A pixel far from every such fit is a colour of the art's own: solid as the PS2 picture is around
    it, its colour as drawn."""
    px = np.asarray(art.convert('RGB'), np.float32)
    H, W = px.shape[:2]
    rgba = p['rgba'].astype(np.float32)
    al = np.clip(rgba[..., 3] / GS_OPAQUE, 0, 1)
    shown = rgba[..., :3] * al[..., None]
    ty = np.minimum(np.arange(H) * p['h'] // H, p['h'] - 1)
    tx = np.minimum(np.arange(W) * p['w'] // W, p['w'] - 1)
    core = cv2.erode((al >= WEAPON_CORE).astype(np.uint8), np.ones((3, 3), np.uint8)).astype(bool)[ty][:, tx]
    best = np.full((H, W), np.inf, np.float32)
    alpha = np.zeros((H, W), np.float32)
    near_opacity = np.zeros((H, W), np.float32)
    for dy in range(-PANEL_REACH, PANEL_REACH + 1):
        for dx in range(-PANEL_REACH, PANEL_REACH + 1):
            yy = np.clip(ty + dy, 0, p['h'] - 1)[:, None]
            xx = np.clip(tx + dx, 0, p['w'] - 1)[None, :]
            near_opacity = np.maximum(near_opacity, al[yy, xx])
            c = shown[yy, xx]
            cc = (c * c).sum(-1)
            s = np.where(cc < 1.0, 0.0, np.clip((px * c).sum(-1) / np.maximum(cc, 1.0), 0, 1))
            d = np.linalg.norm(px - s[..., None] * c, axis=-1)
            a = s * al[yy, xx]
            take = (d < best - FIT_TIE) | ((np.abs(d - best) <= FIT_TIE) & (a > alpha))
            best = np.where(take, np.minimum(d, best), best)
            alpha = np.where(take, a, alpha)
    own = best > OWN_COLOUR
    alpha = np.where(core, 1.0, np.where(own, near_opacity, alpha))
    rgb = np.where((core | own)[..., None], px, px / np.maximum(alpha[..., None], 0.05))
    return np.clip(rgb, 0, 255), alpha


def from_item(p, art):
    """An item picture's new form: the art as it is (the item alone, clear around it), its alpha on the GS scale; a clear
    pixel's colour is the nearest visible pixel's (only where it cannot be seen)."""
    a = np.asarray(art, np.float32)
    rgb, alpha = a[..., :3], a[..., 3] / 255
    clear = alpha <= 0
    if clear.all():
        sys.exit(f"item picture {p['id']}: its art is clear everywhere")
    _, (iy, ix) = ndimage.distance_transform_edt(clear, return_indices=True)
    return np.where(clear[..., None], rgb[iy, ix], rgb), alpha


def install(p, art_path):
    art = Image.open(art_path)
    size = size_of(p)
    note = ''
    if kind_of(p) == 'item' and (art.mode != 'RGBA' or art.size != size):
        sys.exit(f"{art_path}: an item's art is the item alone on a clear background, RGBA {size[0]} x {size[1]}, not {art.mode} {art.width} x {art.height}")
    if kind_of(p) == 'store' and art.size != size:
        sys.exit(f"{art_path}: a vendor picture's art is {size[0]} x {size[1]} over black, not {art.width} x {art.height}")
    if art.size != size:
        if abs(art.width / art.height - size[0] / size[1]) > 0.02:
            note = f' (its {art.width} x {art.height} stretched to {size[0]} x {size[1]})'
        art = art.convert('RGB').resize(size, Image.LANCZOS)
    rgb, alpha = {'white': from_white, 'colour': from_colour, 'panel': from_panel, 'weapon': from_weapon, 'store': from_weapon, 'item': from_item}[kind_of(p)](p, art)
    out = np.dstack([np.clip(rgb, 0, 255), np.clip(alpha, 0, 1) * GS_OPAQUE]).round().astype(np.uint8)
    return Image.fromarray(out, 'RGBA'), note


def names(p):
    """The PCSX2 stems the picture is drawn by: sampled as PSMT8 and as PSMT8H."""
    return [pcsx2.stem(p['indices'], p['clut'], psm) for psm in (0x13, 0x1B)]


def on_orange(rgba, size):
    a = np.asarray(rgba, np.float32)
    al = np.clip(a[..., 3] / GS_OPAQUE, 0, 1)[..., None]
    rgb = a[..., :3] * (2 if is_white(np.asarray(rgba)) else 1)  # the game draws its white icons at twice their grey
    im = Image.fromarray((np.clip(rgb, 0, 255) * al + np.array([150, 95, 25]) * (1 - al)).astype(np.uint8))
    s = size / max(im.size)
    return im.resize((max(1, round(im.width * s)), max(1, round(im.height * s))), Image.NEAREST if im.width < size else Image.LANCZOS)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    sub = ap.add_subparsers(dest='cmd', required=True)
    c = sub.add_parser('cutouts')
    c.add_argument('iso')
    c.add_argument('out')
    i = sub.add_parser('install')
    i.add_argument('iso')
    i.add_argument('art')
    i.add_argument('replacements')
    i.add_argument('--sheet')
    args = ap.parse_args()
    pictures = hud_pictures(args.iso)
    panels = panel_pictures(args.iso)
    weapons = weapon_pictures(args.iso)
    items = item_pictures(args.iso)
    store = store_pictures(args.iso)
    print(f'{len(pictures)} HUD pictures, {len(panels)} panels, {len(weapons)} weapon pictures, {len(items)} item pictures and {len(store)} vendor pictures on the disc')
    pictures.update(panels)
    pictures.update(weapons)
    pictures.update(items)
    # A vendor picture that is a WEAPONS picture too is drawn again as the vendor's (its art at STORE_TARGET).
    pictures.update(store)
    if args.cmd == 'cutouts':
        out = Path(args.out)
        for p in pictures.values():
            d = out / {'white': 'white', 'colour': 'colour', 'panel': 'panels', 'weapon': 'weapons', 'item': 'items', 'store': 'store'}[kind_of(p)]
            d.mkdir(parents=True, exist_ok=True)
            cutout(p).save(d / f"{p['id']}.png")
        print(f'cutouts: {out}')
        return
    found = {}
    stray = []
    for f in sorted(Path(args.art).rglob('*')):
        if f.suffix.lower() not in ('.png', '.jpg', '.jpeg', '.webp'):
            continue
        m = re.match(r'([0-9a-f]{12})', f.name.lower())
        if not m or m.group(1) not in pictures:
            stray.append(f)
            continue
        if m.group(1) in found:
            sys.exit(f'two files for picture {m.group(1)}: {found[m.group(1)]} and {f}')
        found[m.group(1)] = f
    if stray:
        sys.exit('images whose names start with no picture\'s id:\n  ' + '\n  '.join(str(s) for s in stray))
    dest = Path(args.replacements)
    if not dest.is_dir():
        sys.exit(f'{dest} is not a folder')
    done = []
    for pid, f in sorted(found.items()):
        p = pictures[pid]
        im, note = install(p, f)
        for stem in names(p):
            im.save(dest / f'{stem}.png')
        done.append((p, im))
        print(f"{pid}: {kind_of(p)}, {im.width} x {im.height} -> {' and '.join(names(p))}{note}")
    missing = sorted(set(pictures) - set(found))
    print(f'{len(done)} of {len(pictures)} pictures installed in {dest}; {len(missing)} not redrawn yet')
    if args.sheet:
        cell, cols = 160, 6
        rows = (len(done) + cols - 1) // cols
        sheet = Image.new('RGB', (cols * (2 * cell + 16), rows * (cell + 18)), (30, 30, 34))
        d = ImageDraw.Draw(sheet)
        font = ImageFont.truetype(r'C:\Windows\Fonts\arial.ttf', 11)
        for n, (p, im) in enumerate(done):
            x, y = (n % cols) * (2 * cell + 16), (n // cols) * (cell + 18)
            sheet.paste(on_orange(Image.fromarray(p['rgba'], 'RGBA'), cell), (x, y))
            sheet.paste(on_orange(im, cell), (x + cell + 4, y))
            d.text((x, y + cell + 2), p['id'], fill=(220, 220, 220), font=font)
        sheet.save(args.sheet)
        print(args.sheet)


if __name__ == '__main__':
    main()
