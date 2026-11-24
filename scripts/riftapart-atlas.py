"""A character's atlas drawn again at 8x for the Rift Apart folder's `replacements/` (PLAN 9.2; port/uya/src/texnames.rs: drawn
ahead of the packs while RIFT APART SURFACES is on). The player, 15 November 2026, of Ratchet with the materials on: "Surely
using the rift textures on ratchet would at least make it look high res. This doesnt even do that": his atlas is the game's
256 x 256 with no pack, drawn blurred. Rift Apart's own Ratchet textures fit its own model's coordinates (and its fur's colour
is a 256 tint under strands), so the atlas is the game's own, colours and design, drawn again at the new size:

- its colour regions (`riftapart-pair.py` `clusters`, k-means in CIELAB; specks of a texel or two folded into the region
  around them, `tidy`), each region's coverage upsampled smoothly (bicubic
  of the region's mask, softened by a fraction of a texel so a diagonal edge is a line, not steps) and the texel at the new
  size the region whose coverage is greatest there, worked out at twice the size and averaged down (anti-aliased edges);
- each region's own colours, carried past its edge (the region's colours spread by a normalised blur), upsampled smoothly:
  its shading as the painter left it, a neighbour's colour never bleeding over the edge;
- that crisp picture only where the original has a hard edge (a colour step of more than EDGE_LO in CIELAB to a neighbouring
  texel, fully past EDGE_HI), the plain smooth upsample elsewhere, so a soft gradient stays soft.

The texture's materials (grain, fur shells, metal, eyes) are drawn on top as before: they bring the fine detail.

Usage:
  py -3.12 -I scripts/riftapart-atlas.py <stem> <out dir> [--k 12] [--scale 8] [--sheet PNG --crop X0,Y0,X1,Y1]
"""
import argparse
import importlib.util
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

_here = Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location('riftapart_pair', _here / 'riftapart-pair.py')
pair = importlib.util.module_from_spec(_spec)
sys.argv, _argv = [sys.argv[0]], sys.argv
_spec.loader.exec_module(pair)
sys.argv = _argv

# The colour step (CIELAB, to a neighbouring texel) where an edge starts to be drawn crisp, and where it is all crisp.
EDGE_LO = 6.0
EDGE_HI = 16.0
# A region's mask softened by this many texels before it is upsampled.
MASK_SOFT = 0.7
# The picture is softened by this many texels before its edges are measured (a painted texel's own noise is no edge).
EDGE_SOFT = 0.7
# In fur the colour step where an edge starts to be drawn crisp, and where it is all crisp (the stripes' soft edges).
FUR_EDGE_LO = 2.5
FUR_EDGE_HI = 8.0
# Fur's painted strands (`fur_strokes`): how far each is drawn out each way, in new texels, and its contrast.
FUR_LIC_STEPS = 12
FUR_STROKE = 0.12
# The shock filter's rounds at 8x and its Laplacian's softening in new texels at 8x (`shock`).
SHOCK_ITERS = 12
SHOCK_SIGMA = 5.0
# The smoothing along an edge each round (`shock`).
SHOCK_ALONG = 0.25
# Rounds of the majority filter that fold specks of a texel or two into the region around them.
TIDY = 2


def tidy(labels, k, rounds):
    """Each opaque texel given the region most of its 3 x 3 neighbourhood has (its own on a tie), `rounds` times."""
    h, w = labels.shape
    for _ in range(rounds):
        p = np.pad(labels, 1, mode='edge')
        votes = np.zeros((k + 1, h, w), np.int32)
        for dy in range(3):
            for dx in range(3):
                n = p[dy:dy + h, dx:dx + w]
                np.add.at(votes, (n, *np.indices((h, w))), 1)
        votes[0] = -1
        own = np.take_along_axis(votes, labels[None], 0)[0]
        top = votes.argmax(0)
        labels = np.where((labels > 0) & (votes.max(0) > own), top, labels)
    return labels


def blur(a, sigma):
    """A Gaussian blur over the first two axes (edges repeated)."""
    r = max(1, int(3 * sigma + 0.5))
    x = np.arange(-r, r + 1, dtype=np.float32)
    k = np.exp(-x * x / (2 * sigma * sigma))
    k /= k.sum()
    for axis in (0, 1):
        pad = [(0, 0)] * a.ndim
        pad[axis] = (r, r)
        p = np.pad(a, pad, mode='edge')
        a = sum(k[j] * np.take(p, range(j, j + a.shape[axis]), axis=axis) for j in range(2 * r + 1))
    return a


def up(a, w, h):
    """`a` (2D, or 2D with channels) resized to w x h, bicubic, as floats."""
    if a.ndim == 2:
        return np.asarray(Image.fromarray(a.astype(np.float32), 'F').resize((w, h), Image.BICUBIC), np.float32)
    return np.dstack([up(a[..., c], w, h) for c in range(a.shape[2])])


def shock(img, iters, dt, sigma, along):
    """Edges made steep in place (Alvarez and Mazorra's shock filter, upwind): each texel moved toward the side of its edge the
    second derivative of its smoothed luminance across the edge points to, so a blurred edge becomes a step at its middle; and
    the picture smoothed along each edge (`along`), so the step's contour comes out a clean line, not the texel grid's fray. The
    colour channels move together (the luminance's direction and sign)."""
    img = img.copy()
    for _ in range(iters):
        ls = blur(img @ pair.LUMA.astype(np.float32), sigma)
        gy, gx = np.gradient(ls)
        gyy, gyx = np.gradient(gy)
        gxy, gxx = np.gradient(gx)
        n2 = gx * gx + gy * gy + 1e-6
        # The second derivative across the edge (along the gradient).
        across = (gxx * gx * gx + 2 * gxy * gx * gy + gyy * gy * gy) / n2
        sgn = np.sign(across)[..., None]
        q = np.pad(img, ((1, 1), (1, 1), (0, 0)), mode='edge')
        dxp, dxm = q[1:-1, 2:] - img, img - q[1:-1, :-2]
        dyp, dym = q[2:, 1:-1] - img, img - q[:-2, 1:-1]
        ero = np.sqrt(np.minimum(dxp, 0) ** 2 + np.maximum(dxm, 0) ** 2 + np.minimum(dyp, 0) ** 2 + np.maximum(dym, 0) ** 2)
        dil = np.sqrt(np.maximum(dxp, 0) ** 2 + np.minimum(dxm, 0) ** 2 + np.maximum(dyp, 0) ** 2 + np.minimum(dym, 0) ** 2)
        # Each channel's second derivative along the edge (across the luminance's gradient).
        ixx = q[1:-1, 2:] + q[1:-1, :-2] - 2 * img
        iyy = q[2:, 1:-1] + q[:-2, 1:-1] - 2 * img
        ixy = (q[2:, 2:] - q[2:, :-2] - q[:-2, 2:] + q[:-2, :-2]) / 4
        tx, ty = (-gy / np.sqrt(n2))[..., None], (gx / np.sqrt(n2))[..., None]
        lengthwise = ixx * tx * tx + 2 * ixy * tx * ty + iyy * ty * ty
        img -= dt * np.where(sgn > 0, ero, -dil)
        img += along * lengthwise
    return img


def local_range(rgb, big):
    """Per new texel the darkest and brightest of the original's colours around it (its 3 x 3 texels, channel by channel), so
    the edges made steep bring no colour the painter did not use there: no halo, no ring."""
    h, w = rgb.shape[:2]
    p = np.pad(rgb, ((1, 1), (1, 1), (0, 0)), mode='edge')
    stack = np.stack([p[dy:dy + h, dx:dx + w] for dy in range(3) for dx in range(3)])
    lo, hi = stack.min(0), stack.max(0)
    near = lambda a: np.dstack([np.asarray(Image.fromarray(a[..., c], 'F').resize(big, Image.BILINEAR)) for c in range(3)])
    return near(lo), near(hi)


def redraw(rgba, scale, fur=None):
    rgb = rgba[..., :3].astype(np.float32)
    h, w = rgb.shape[:2]
    big = (w * scale, h * scale)
    smooth = up(rgb, *big)
    # The original's edges: each texel's largest colour step to a neighbour, on the picture softened a little.
    lab = pair._lab(blur(rgb, EDGE_SOFT).reshape(-1, 3)).reshape(h, w, 3)
    p = np.pad(lab, ((1, 1), (1, 1), (0, 0)), mode='edge')
    step = np.max([np.linalg.norm(lab - p[1 + dy:1 + dy + h, 1 + dx:1 + dx + w], axis=2) for dy, dx in ((0, 1), (0, -1), (1, 0), (-1, 0))], axis=0)
    edge = np.clip((step - EDGE_LO) / (EDGE_HI - EDGE_LO), 0, 1)
    if fur is not None:
        # In fur the stripes' soft edges are drawn crisp too (FUR_EDGE_LO, FUR_EDGE_HI).
        edge = np.maximum(edge, fur * np.clip((step - FUR_EDGE_LO) / (FUR_EDGE_HI - FUR_EDGE_LO), 0, 1))
    crisp_w = np.clip(up(blur(edge, 1.0), *big), 0, 1)
    lo, hi = local_range(rgb, big)
    crisp = np.clip(shock(smooth, SHOCK_ITERS * scale // 8, 0.5, SHOCK_SIGMA * scale / 8, SHOCK_ALONG), lo, hi)
    out = smooth * (1 - crisp_w[..., None]) + crisp * crisp_w[..., None]
    if fur is not None:
        out = out * fur_strokes(rgb, fur, big)[..., None]
    alpha = np.asarray(Image.fromarray(rgba[..., 3]).resize(big, Image.NEAREST))
    return np.dstack([np.clip(out + 0.5, 0, 255).astype(np.uint8), alpha])


def fur_strokes(rgb, fur, big):
    """Fur's strands at the new size, as a factor on the colour (1 on average): white noise drawn out along the way the
    painter's own strokes run (the original's luminance's structure: the direction across its gradient, smoothed over a few
    texels; a line-integral convolution, FUR_LIC_STEPS new texels each way), its light and dark over its mean raised to
    FUR_STROKE contrast, only in the fur (`fur`, the fur regions' share of each texel, softened over a texel)."""
    lum = rgb @ pair.LUMA.astype(np.float32)
    gy, gx = np.gradient(blur(lum, 0.7))
    # The structure tensor, smoothed: its main direction is across the strokes; the strokes run perpendicular to it.
    jxx, jxy, jyy = blur(gx * gx, 2.0), blur(gx * gy, 2.0), blur(gy * gy, 2.0)
    theta = 0.5 * np.arctan2(2 * jxy, jxx - jyy) + np.pi / 2
    c2, s2 = up(np.cos(2 * theta), *big), up(np.sin(2 * theta), *big)
    ang = 0.5 * np.arctan2(s2, c2)
    dx, dy = np.cos(ang).astype(np.float32), np.sin(ang).astype(np.float32)
    H, W = big[1], big[0]
    rng = np.random.default_rng(0)
    noise = rng.random((H, W), dtype=np.float32)
    yy, xx = np.mgrid[0:H, 0:W].astype(np.float32)
    acc = noise.copy()
    for sign in (1.0, -1.0):
        px, py = xx.copy(), yy.copy()
        for _ in range(FUR_LIC_STEPS):
            px = (px + sign * dx) % W
            py = (py + sign * dy) % H
            acc += noise[py.astype(np.int32) % H, px.astype(np.int32) % W]
    acc /= 2 * FUR_LIC_STEPS + 1
    # Its light and dark over its local mean (the convolution's own mean is 0.5; its spread shrinks with the length).
    hp = acc - blur(acc, 6.0)
    hp /= max(float(hp.std()), 1e-6)
    m = np.clip(up(blur(fur.astype(np.float32), 0.7), *big), 0, 1)
    return 1.0 + FUR_STROKE * np.clip(hp, -2.5, 2.5) * m


def fur_of(stem, labels_k, profile):
    """The texels of the atlas's fur regions (`textures/riftapart-regions.tsv`'s row for `stem`, its `fur:` materials; its
    discs over them as the row gives them), 1 in fur, 0 elsewhere; None for an atlas with no row or no fur."""
    for line in pair.REGIONS.read_text(encoding='utf-8').splitlines():
        f = line.split('\t')
        if f[0] != stem:
            continue
        labels, k = pair.clusters(stem, int(f[2]), profile)
        assign, discs = pair.parse_assign(f[3], k)
        labels = pair.apply_discs(labels, k, discs)
        regions = [c for c, m in assign.items() if m and m[0] == pair.FUR]
        return np.isin(labels, regions).astype(np.float32) if regions else None
    return None


# Ratchet's own eye in Rift Apart (the player's copy, `C:\riftapart-export\more`): its iris's centre, its pupil's radius and its
# iris's outer radius, in its 2048 texels.
RA_EYE = (Path('C:/riftapart-export/more/characters/hero/hero_ratchet/textures/ratchet_eye_c.png'), (1014.3, 1023.5), 170.0, 300.0)


# Rift Apart's iris's light and dark over its mean, raised to this power on the game's ring (its fibres read at a character's size).
IRIS_CONTRAST = 1.5


def eye(new, rgba, scale, centre, pupil, outer):
    """An eye of the atlas given Rift Apart's iris: its ring (`pupil` to `outer` texels from `centre`, in the game's texels)
    painted again as the game's own ring (its ring's texels' colours carried across the ring's whole width, upsampled smoothly:
    its green and its shading) times Rift Apart's iris there (its pupil's edge to its ring's, matched radius for radius and
    angle for angle) over that iris's mean, to the power IRIS_CONTRAST: its fibres, crypts and darker rim, its edges
    anti-aliased. The pupil, the painted shine and everything past
    the ring stay the game's."""
    src, (ex, ey), ep, eo = RA_EYE
    e = np.asarray(Image.open(src).convert('RGB'), np.float32) @ pair.LUMA.astype(np.float32)
    # The game's ring, its colours carried over the pupil and the outline next to it (a normalised blur) and upsampled.
    gy, gx = np.mgrid[0:rgba.shape[0], 0:rgba.shape[1]]
    gr = np.sqrt((gx + 0.5 - centre[0]) ** 2 + (gy + 0.5 - centre[1]) ** 2)
    m = ((gr > pupil + 0.5) & (gr < outer - 0.5)).astype(np.float32)
    src_rgb = rgba[..., :3].astype(np.float32)
    spread = blur(src_rgb * m[..., None], 1.5) / np.maximum(blur(m, 1.5), 1e-6)[..., None]
    ringcol = np.where(m[..., None] > 0, src_rgb, spread)
    h, w = new.shape[:2]
    colour = up(ringcol, w, h)
    cx, cy = centre[0] * scale, centre[1] * scale
    r0, r1 = pupil * scale, outer * scale
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float32)
    dx, dy = xx + 0.5 - cx, yy + 0.5 - cy
    r = np.sqrt(dx * dx + dy * dy)
    band = (r > r0 - 2) & (r < r1 + 2)
    t = np.clip((r[band] - r0) / (r1 - r0), 0, 1)
    rr = ep + t * (eo - ep)
    ang = np.arctan2(dy[band], dx[band])
    sx = np.clip(ex + rr * np.cos(ang), 0, e.shape[1] - 1).astype(int)
    sy = np.clip(ey + rr * np.sin(ang), 0, e.shape[0] - 1).astype(int)
    ey2, ex2 = np.mgrid[0:e.shape[0], 0:e.shape[1]]
    rad = np.sqrt((ex2 - ex) ** 2 + (ey2 - ey) ** 2)
    iris = e[(rad > ep) & (rad < eo)].mean()
    detail = np.clip((e[sy, sx] / iris) ** IRIS_CONTRAST, 0.3, 2.0)
    # Anti-aliased over a new texel at the pupil's edge and the ring's; not over the painted shine.
    cover = np.clip(r[band] - (r0 - 0.5), 0, 1) * np.clip((r1 + 0.5) - r[band], 0, 1)
    rgb = new[..., :3].astype(np.float32)
    px = rgb[band]
    shine = np.clip(((px @ pair.LUMA.astype(np.float32)) - 170) / 40, 0, 1)
    k = (cover * (1 - shine))[:, None]
    rgb[band] = px * (1 - k) + np.clip(colour[band] * detail[:, None], 0, 255) * k
    new[..., :3] = np.clip(rgb + 0.5, 0, 255).astype(np.uint8)
    return new


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('stem')
    ap.add_argument('out')
    ap.add_argument('--k', type=int, default=8)
    ap.add_argument('--scale', type=int, default=8)
    ap.add_argument('--profile', default=r'C:\Users\<you>\Games\Ratchet and Clank 3')
    ap.add_argument("--eye", help="X,Y,PUPIL,OUTER: an eye's centre and its iris's inner and outer radius, in the game's texels (eye)")
    ap.add_argument('--sheet')
    ap.add_argument('--crop', help='X0,Y0,X1,Y1 of the atlas (shares of its width and height) for the sheet')
    args = ap.parse_args()
    src = pair.original_file(args.stem, args.profile)
    rgba = np.asarray(Image.open(src).convert('RGBA'))
    new = redraw(rgba, args.scale, fur_of(args.stem, None, args.profile))
    if args.eye:
        x, y, pupil, outer = (float(v) for v in args.eye.split(','))
        new = eye(new, rgba, args.scale, (x, y), pupil, outer)
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    Image.fromarray(new, 'RGBA').save(out / f'{args.stem}.png')
    print(f'{args.stem}: {rgba.shape[1]} x {rgba.shape[0]} -> {new.shape[1]} x {new.shape[0]}: {out / (args.stem + ".png")}')
    if args.sheet:
        x0, y0, x1, y1 = (float(v) for v in args.crop.split(',')) if args.crop else (0, 0, 1, 1)
        h, w = rgba.shape[:2]
        box = (int(x0 * w), int(y0 * h), int(x1 * w), int(y1 * h))
        size = ((box[2] - box[0]) * args.scale, (box[3] - box[1]) * args.scale)
        opaque = rgba.copy()
        opaque[..., 3] = 255
        nn = Image.fromarray(opaque).crop(box).resize(size, Image.NEAREST)
        bc = Image.fromarray(opaque).crop(box).resize(size, Image.BICUBIC)
        nw = Image.fromarray(new[..., :3]).crop(tuple(v * args.scale for v in box))
        sheet = Image.new('RGB', (3 * size[0] + 40, size[1] + 40), (24, 24, 26))
        d = ImageDraw.Draw(sheet)
        for j, (im, label) in enumerate(((nn, 'the game\'s 256 texels'), (bc, 'as drawn now (smooth upsample)'), (nw, f'drawn again at {args.scale}x'))):
            sheet.paste(im, (10 + j * (size[0] + 10), 30))
            d.text((10 + j * (size[0] + 10), 8), label, fill=(235, 235, 235), font=pair.font(16))
        Path(args.sheet).parent.mkdir(parents=True, exist_ok=True)
        sheet.save(args.sheet)
        print(args.sheet)


if __name__ == '__main__':
    main()
