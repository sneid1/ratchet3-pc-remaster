"""The remaster's own UI art (PLAN 9.2, the HUD and menus): the game's UI pictures drawn again sharp, for the Rift Apart folder's
`replacements/` (port/uya/src/texnames.rs: drawn ahead of the packs while RIFT APART SURFACES is on).

Usage:
  python -I scripts/riftapart-ui.py <ui dumps dir> <out dir> [--packs DIR] [--sheet PNG]

<ui dumps dir> holds the 2D layer's textures as `uya native --dump-ui` writes them (PCSX2-named PNGs, alpha on the GS scale,
0x80 opaque). A texture a pack already draws sharp (`--packs`, the profile's texture_replacements) is left to the pack. The
rest are drawn again at 512 texels on their long side, written under the same name to <out dir>:

- A flat icon (up to 128 texels, its opaque texels a few flat colours: the HUD's digits, the bolt, the clock, arrows, R1 and R2, slot frames,
  the quick-select ring's icons): each colour's region and the alpha are redrawn as vector shapes. The outline is traced from
  the coverage's half-way contour (sub-texel), corners are found and kept sharp, runs within 0.12 texel of a straight line
  become straight lines (corners at the lines' intersection), the rest are smoothed curves; the outlines are filled at the new
  size with 4x supersampling (a one-texel soft edge). The shape is traced inside a replicated border, so an icon the texture
  edge cuts is cut there at the same place and nothing is rounded off or added.
- A shaded icon (flat colours, an alpha that fades through many levels: a weapon's or gadget's icon, the target): the same, the
  alpha traced at 24 levels and the sharp shapes added, so its fade is contours at the new size and not blurred texels.
- A picture (a portrait, an item's painting): its colours upsampled with Lanczos and sharpened, its alpha's edge rebuilt as an
  icon's (a translucent picture's alpha is upsampled like its colours).

--sheet writes a sheet of each texture as it is beside its new art, named.
"""
import argparse
import importlib.util
import sys
from pathlib import Path

import cv2
import numpy as np
from scipy import ndimage
from PIL import Image, ImageDraw, ImageFilter, ImageFont

_here = Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location('uya_texnames', _here / 'uya_texnames.py')
names = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(names)

TARGET = 512
GS_OPAQUE = 128


def contour(cover, s):
    """`cover` (0 to 1, the texture's size) upsampled `s` times and cut at 0.5 with an edge one output texel wide."""
    h, w = cover.shape
    up = np.asarray(Image.fromarray((cover * 255).astype(np.uint8)).resize((w * s, h * s), Image.BICUBIC), np.float32) / 255
    # A one-texel ramp at the 0.5 contour: the slope of `up` across an edge is about 1 / s per output texel.
    k = 0.5 / s
    return np.clip((up - 0.5) / (2 * k) + 0.5, 0, 1)


PAD = 3            # texels of replicated border the shapes are traced and drawn in, so a shape cut by the texture's edge stays cut there
SS = 4             # supersampling of the fill: the AA edge is one output texel
CORNER_DEG = 50.0  # a turn of more than this over +-0.6 texel along the contour is a corner
STRAIGHT = 0.12    # a run whose points stay within this many texels of its chord is a straight line


def smooth_cover(cover, s):
    """`cover` (0 to 1, the texture's size) with its edge replicated by PAD texels, upsampled `s` times (bicubic) and lightly
    blurred; the 0.5 level of the result is the shape's outline at sub-texel accuracy."""
    p = np.pad(cover, PAD, mode='edge').astype(np.float32)
    h, w = p.shape
    up = cv2.resize(p, (w * s, h * s), interpolation=cv2.INTER_CUBIC)
    return cv2.GaussianBlur(up, (0, 0), 0.18 * s)


def _gauss_open(pts, sigma):
    """Gaussian smoothing along an open run of points, its ends held fixed (odd reflection about each end)."""
    n = len(pts)
    r = int(3 * sigma) + 1
    if n < 3 or sigma <= 0:
        return pts
    k = np.exp(-0.5 * (np.arange(-r, r + 1) / sigma) ** 2)
    k /= k.sum()
    idx = np.arange(-r, n + r)
    ext = np.empty((len(idx), 2))
    lo, hi = idx < 0, idx >= n
    mid = ~lo & ~hi
    ext[mid] = pts[idx[mid]]
    ext[lo] = 2 * pts[0] - pts[np.clip(-idx[lo], 0, n - 1)]
    ext[hi] = 2 * pts[-1] - pts[np.clip(2 * (n - 1) - idx[hi], 0, n - 1)]
    out = np.stack([np.convolve(ext[:, c], k, mode='valid') for c in range(2)], 1)
    out[0], out[-1] = pts[0], pts[-1]
    return out


def _intersect(p0, d0, p1, d1):
    m = np.array([d0, -d1]).T
    if abs(np.linalg.det(m)) < 1e-3:
        return None
    t = np.linalg.solve(m, p1 - p0)
    return p0 + d0 * t[0]


def trace(contour_pts, s):
    """One closed contour (N x 2 output-texel points) as a clean outline: corners kept sharp, straight runs straight lines,
    the rest smooth curves. Returns the outline's points."""
    P = contour_pts.astype(np.float64)
    n = len(P)
    if n < 8:
        return P
    k = max(2, int(round(0.6 * s)))
    a = np.roll(P, k, 0)
    b = np.roll(P, -k, 0)
    v1, v2 = P - a, b - P
    ang = np.degrees(np.abs(np.arctan2(v1[:, 0] * v2[:, 1] - v1[:, 1] * v2[:, 0], (v1 * v2).sum(1))))
    cand = np.where(ang > CORNER_DEG)[0]
    corners = []
    for i in cand:  # the sharpest point of each cluster of candidates
        w = (np.arange(i - k, i + k + 1)) % n
        if ang[i] >= ang[w].max() - 1e-9 and (not corners or (i - corners[-1]) % n > k):
            corners.append(i)
    if len(corners) >= 2 and (corners[0] - corners[-1]) % n <= k:
        corners.pop()
    if not corners:
        # No corner: a closed smooth curve (smoothed in a ring).
        sig = 0.5 * s
        r = int(3 * sig) + 1
        ker = np.exp(-0.5 * (np.arange(-r, r + 1) / sig) ** 2)
        ker /= ker.sum()
        ext = np.concatenate([P[-r:], P, P[:r]])
        return np.stack([np.convolve(ext[:, c], ker, mode='valid') for c in range(2)], 1)
    runs = []
    for j, c0 in enumerate(corners):
        c1 = corners[(j + 1) % len(corners)]
        idx = np.arange(c0, c1 + 1 if c1 > c0 else c1 + n + 1) % n
        run = P[idx]
        chord = run[-1] - run[0]
        L = np.hypot(*chord)
        if L > 1e-6:
            dev = np.abs(chord[0] * (run[:, 1] - run[0, 1]) - chord[1] * (run[:, 0] - run[0, 0])) / L
            straight = dev.max() <= STRAIGHT * s and L > 1.0 * s
        else:
            straight = False
        runs.append((run, straight))
    # Corner points: where two straight runs meet, their lines' intersection (a sharp corner exactly).
    ends = [[r[0].copy(), r[-1].copy()] for r, _ in runs]
    for j in range(len(runs)):
        nj = (j + 1) % len(runs)
        if runs[j][1] and runs[nj][1]:
            r0, r1 = runs[j][0], runs[nj][0]
            d0, d1 = r0[-1] - r0[0], r1[-1] - r1[0]
            q = _intersect(r0[0], d0, r1[0], d1)
            if q is not None and np.hypot(*(q - r0[-1])) < 1.5 * s:
                ends[j][1] = q
                ends[nj][0] = q
    out = []
    for (run, straight), (e0, e1) in zip(runs, ends):
        if straight:
            out.extend([e0, e1])
        else:
            run = run.copy()
            run[0], run[-1] = e0, e1
            sm = _gauss_open(run, 0.5 * s)
            sm[0], sm[-1] = e0, e1
            out.extend(sm[::max(1, s // 4)])
            out.append(e1)
    return np.array(out)


def shape_mask(up, s):
    """The 0..1 anti-aliased drawing (texture size x `s`, PAD margin cut) of the shape `up` >= 0.5 (a smooth_cover field): its
    outline traced (trace(): corners sharp, straight runs straight, curves smooth) and filled with SS x supersampling."""
    binary = (up >= 0.5).astype(np.uint8)
    H, W = binary.shape
    cs, hier = cv2.findContours(binary, cv2.RETR_LIST, cv2.CHAIN_APPROX_NONE)
    acc = np.zeros((H * SS, W * SS), np.uint8)
    for c in cs:
        pts = c[:, 0, :].astype(np.float64) + 0.5  # pixel centres to pixel-corner coordinates
        if len(pts) < 4:
            continue
        o = trace(pts, s)
        poly = np.round(o * SS * 16).astype(np.int32)
        tmp = np.zeros_like(acc)
        cv2.fillPoly(tmp, [poly.reshape(-1, 1, 2)], 1, lineType=cv2.LINE_8, shift=4)
        acc ^= tmp
    m = cv2.resize(acc.astype(np.float32), (W, H), interpolation=cv2.INTER_AREA)
    o = PAD * s
    return m[o:H - o, o:W - o]


SHADES = 24          # alpha levels a shaded icon's outline is traced at
SHADED = 0.30        # an icon is shaded when this share of its visible texels are neither clear nor opaque


def shaded(alpha):
    """Whether `alpha` (0 to 1) is a shaded shape (a weapon's or gadget's icon: an alpha that fades through many levels inside
    the shape) and not a flat one with an anti-aliased edge."""
    visible = alpha > 0.05
    if not visible.any():
        return False
    return ((alpha > 0.12) & (alpha < 0.88))[visible].mean() > SHADED


def shaded_mask(alpha, s):
    """The 0..1 drawing (texture size x `s`) of a shaded `alpha`: the outline at each of SHADES levels is traced as a flat
    shape's is (trace(): corners sharp, straight runs straight, curves smooth) and the filled shapes are added, so the fade is
    the stack of sharp contours at the new size, not a blurred copy of the old texels."""
    up = smooth_cover(alpha, s)
    acc = 0
    for k in range(SHADES):
        acc = acc + shape_mask(up - (k + 0.5) / SHADES + 0.5, s)
    return acc / SHADES


def flat_colours(rgb, opaque, most=8, tolerance=10.0):
    """The texture's flat colours when its opaque texels are a few (each within `tolerance` of one of at most `most`
    colours), as (colours, index of each texel); None for a picture."""
    px = rgb[opaque]
    if len(px) == 0:
        return None
    colours = []
    idx = np.full(len(px), -1)
    for _ in range(most):
        free = idx < 0
        if not free.any():
            break
        # The commonest remaining colour, then every texel near it.
        q = (px[free] // 8).astype(np.int32)
        keys, counts = np.unique(q[:, 0] * 1024 + q[:, 1] * 32 + q[:, 2], return_counts=True)
        top = keys[np.argmax(counts)]
        c = px[free][(q[:, 0] * 1024 + q[:, 1] * 32 + q[:, 2]) == top].mean(0)
        near = free & (np.abs(px - c).max(1) <= tolerance)
        idx[near] = len(colours)
        colours.append(px[near].mean(0))
    if (idx < 0).any():
        return None
    full = np.full(opaque.shape, -1)
    full[opaque] = idx
    return np.array(colours), full


def redraw(path):
    """The texture at `path` drawn again (RGBA, alpha on the GS scale) and how ('icon' or 'picture')."""
    a = np.asarray(Image.open(path).convert('RGBA'), np.float32)
    h, w = a.shape[:2]
    s = max(1, TARGET // max(w, h))
    alpha = np.clip(a[..., 3] / GS_OPAQUE, 0, 1)
    rgb = a[..., :3]
    opaque = alpha > 0.5
    # A big texture (the quick-select ring's 256 x 256 panel) is a painting, not an icon: it stays a picture.
    flat = flat_colours(rgb, opaque) if max(w, h) <= 128 else None
    if flat is not None:
        colours, idx = flat
        out_a = (shaded_mask(alpha, s) if shaded(alpha) else shape_mask(smooth_cover(alpha, s), s)) * GS_OPAQUE
        # Every texel takes its nearest opaque texel's colour class, so each colour's region also fills the cut-out texels
        # around it (an edge does not darken toward black where the alpha fades). The regions are the argmax of each class's
        # smoothly upsampled one-hot, so they tile the picture.
        _, (iy, ix) = ndimage.distance_transform_edt(idx < 0, return_indices=True)
        lab = idx[iy, ix]
        ups = np.stack([smooth_cover((lab == j).astype(np.float32), s) for j in range(len(colours))], 0)
        weights = np.zeros((h * s, w * s, len(colours)), np.float32)
        for j in range(len(colours)):
            others = np.delete(ups, j, 0)
            field = 0.5 + ups[j] - (others.max(0) if len(others) else 0.5)
            weights[..., j] = shape_mask(field, s)
        total = weights.sum(-1, keepdims=True)
        out_rgb = (weights @ colours) / np.maximum(total, 1e-6)
        nearest = colours[np.argmax(ups, 0)][PAD * s:-PAD * s, PAD * s:-PAD * s]
        out_rgb = np.where(total > 1e-3, out_rgb, nearest)
        how = 'icon'
    else:
        if shaded(alpha):
            # A translucent painting (a weapon's picture and its glow, the quick-select ring's panel): its alpha is part of the
            # picture, so it is upsampled as the colours are; cut at one level it would lose the glow.
            out_a = np.clip(np.asarray(Image.fromarray(alpha.astype(np.float32), 'F').resize((w * s, h * s), Image.LANCZOS)), 0, 1) * GS_OPAQUE
        else:
            out_a = contour(alpha, s) * GS_OPAQUE
        im = Image.fromarray(rgb.astype(np.uint8)).resize((w * s, h * s), Image.LANCZOS)
        im = im.filter(ImageFilter.UnsharpMask(radius=max(1, s // 2), percent=60, threshold=2))
        out_rgb = np.asarray(im, np.float32)
        how = 'picture'
    out = np.dstack([np.clip(out_rgb, 0, 255), out_a]).astype(np.uint8)
    return Image.fromarray(out, 'RGBA'), how


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('dumps')
    ap.add_argument('out')
    ap.add_argument('--packs', default=r'C:\Users\<you>\Games\Ratchet and Clank 3\texture_replacements')
    ap.add_argument('--sheet')
    args = ap.parse_args()
    packs = set()
    for p in Path(args.packs).rglob('*.png'):
        k = names.parse(p.name)
        if k is not None and '0-riftapart' not in p.parts:
            packs.add(k)
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    done = []
    for p in sorted(Path(args.dumps).glob('*.png')):
        key = names.parse(p.name)
        if key is None:
            sys.exit(f'{p}: no PCSX2 name')
        if key in packs:
            continue
        im, how = redraw(p)
        im.save(out / p.name)
        done.append((p, out / p.name, how))
        print(f'{p.stem}: {how}, {im.width} x {im.height}')
    print(f'{len(done)} UI pictures drawn again: {out}')
    if args.sheet:
        cell = 256
        cols = 4
        rows = (len(done) + cols - 1) // cols
        sheet = Image.new('RGB', (cols * (2 * cell + 24) + 12, rows * (cell + 40) + 40), (40, 40, 46))
        d = ImageDraw.Draw(sheet)
        f = ImageFont.truetype(r'C:\Windows\Fonts\arial.ttf', 13)
        d.text((12, 10), "the game's UI pictures (left, blown up) and the remaster's (right), on grey", fill=(240, 240, 240), font=ImageFont.truetype(r'C:\Windows\Fonts\arialbd.ttf', 16))
        for i, (src, new, how) in enumerate(done):
            x, y = 12 + (i % cols) * (2 * cell + 24), 40 + (i // cols) * (cell + 40)
            for j, (pth, rs) in enumerate(((src, Image.NEAREST), (new, Image.LANCZOS))):
                im = Image.open(pth).convert('RGBA')
                im = Image.fromarray(np.dstack([np.asarray(im)[..., :3], np.clip(np.asarray(im)[..., 3].astype(np.int32) * 2, 0, 255).astype(np.uint8)]), 'RGBA')
                bg = Image.new('RGBA', im.size, (90, 90, 100, 255))
                bg.alpha_composite(im)
                sheet.paste(bg.convert('RGB').resize((cell, cell), rs), (x + j * (cell + 4), y))
            d.text((x, y + cell + 4), f'{src.stem[:34]}  {how}', fill=(220, 220, 220), font=f)
        sheet.save(args.sheet)
        print(args.sheet)


if __name__ == '__main__':
    main()
