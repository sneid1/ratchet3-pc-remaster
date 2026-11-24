"""Writes a texture pack selection: the files of an extracted PCSX2 pack that keep the original's look.

Usage:
  python -I scripts/texture-select.py <pack folder> <selection.txt> --dumps DIR
      [--min-scale N] [--max-hue-shift X --max-lightness-shift Y] [--skip PREFIX=REASON ...]
      [--undumped include|leave-out] [--covered SELECTION ...] [--header TEXT]

<pack folder> is the pack extracted into its own folder (its files keep the pack's paths). --dumps
is the port's dump folder (`uya frames ... --dump DIR`): the game's own textures under their PCSX2
names, the originals each pack file is judged against.

Every pack file whose name is a PCSX2 replacement key (scripts/uya_texnames.py) is a candidate; the
first in sorted path order wins a key, as in the port. A candidate is left out, and the selection
says which and why, when:
  --skip PREFIX=REASON   it is under a path prefix (a folder, or one file);
  (always)               it is smaller than the original along either side (no added detail);
                         another aspect ratio is fine: like PCSX2, the port stretches a file over the
                         rectangle it replaces;
  --min-scale N          it is less than N times the original along its shorter scaled side (a 1x
                         file is the original re-encoded, with no added detail);
  --max-hue-shift X / --max-lightness-shift Y
                         scaled down to the original's size, it moves the mean colour of the
                         original's opaque texels by more than X in CIELAB a*b* (hue and saturation)
                         or Y in L* (lightness): "more detail, same look";
  --covered SELECTION    its key is in an earlier selection (a gap fill takes only what the packs
                         before it leave);
  --undumped leave-out   the game has not drawn its texture in a dumped run yet, so there is no
                         original to judge it by (the default); `include` takes such files
                         unjudged and lists them apart.

The selection lists one pack-relative file per line, for install-texture-pack.py, each under a
comment with its key, scale and colour change.
"""
import argparse
import collections
import importlib.util
import os
import struct
import sys
from pathlib import Path

_spec = importlib.util.spec_from_file_location('uya_texnames', Path(__file__).with_name('uya_texnames.py'))
names = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(names)


def long_path(path):
    """The path in a form Windows reads past 260 characters (pack folders can nest deep)."""
    return Path('\\\\?\\' + str(path.resolve())) if os.name == 'nt' else path


def image_size(path):
    """(width, height) from a PNG or DDS header."""
    with open(path, 'rb') as f:
        head = f.read(32)
    if head[:8] == b'\x89PNG\r\n\x1a\n':
        return struct.unpack('>II', head[16:24])
    if head[:4] == b'DDS ':
        h, w = struct.unpack('<II', head[12:20])
        return w, h
    sys.exit(f'{path}: neither PNG nor DDS')


def lab(rgb):
    """CIELAB (D65) of sRGB texels in 0-255."""
    import numpy as np
    c = rgb / 255.0
    c = np.where(c > 0.04045, ((c + 0.055) / 1.055) ** 2.4, c / 12.92)
    xyz = c @ np.array([[0.4124, 0.2126, 0.0193], [0.3576, 0.7152, 0.1192], [0.1805, 0.0722, 0.9505]])
    xyz /= [0.95047, 1.0, 1.08883]
    f = np.where(xyz > 0.008856, np.cbrt(xyz), 7.787 * xyz + 16 / 116)
    return np.stack([116 * f[:, 1] - 16, 500 * (f[:, 0] - f[:, 1]), 200 * (f[:, 1] - f[:, 2])], 1)


def colour_change(original, replacement):
    """(a*b* shift, L* shift) of the mean colour of the original's opaque texels (alpha on the GS scale: 0x80 = opaque)."""
    import numpy as np
    from PIL import Image
    orig = np.asarray(Image.open(original).convert('RGBA'), dtype=np.float32)
    new = np.asarray(Image.open(replacement).convert('RGBA').resize((orig.shape[1], orig.shape[0]), Image.BOX),
                     dtype=np.float32)
    mask = orig[..., 3] > 63
    if mask.sum() < 4:
        mask = np.ones(mask.shape, bool)
    d = lab(new[..., :3][mask]).mean(0) - lab(orig[..., :3][mask]).mean(0)
    return float(np.hypot(d[1], d[2])), float(d[0])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('pack', type=Path)
    ap.add_argument('selection', type=Path)
    ap.add_argument('--dumps', type=Path, required=True)
    ap.add_argument('--min-scale', type=float, default=0)
    ap.add_argument('--max-hue-shift', type=float)
    ap.add_argument('--max-lightness-shift', type=float)
    ap.add_argument('--skip', action='append', default=[], help='PREFIX=REASON')
    ap.add_argument('--undumped', choices=['include', 'leave-out'], default='leave-out')
    ap.add_argument('--covered', type=Path, action='append', default=[])
    ap.add_argument('--header', default='')
    args = ap.parse_args()
    covered = {}
    for sel in args.covered:
        for line in sel.read_text(encoding='utf-8').splitlines():
            line = line.strip()
            if line and not line.startswith('#'):
                key = names.parse(line.rsplit('/', 1)[-1])
                if key is None:
                    sys.exit(f'{sel}: not a replacement file name: {line}')
                covered.setdefault(key, sel.name)
    if (args.max_hue_shift is None) != (args.max_lightness_shift is None):
        sys.exit('--max-hue-shift and --max-lightness-shift go together')
    skips = [s.split('=', 1) for s in args.skip]

    root = long_path(args.pack)
    files = {}
    ignored = collections.Counter()
    for p in sorted(root.rglob('*')):
        if not p.is_file():
            continue
        key = names.parse(p.name)
        rel = p.relative_to(root).as_posix()
        if key is None:
            ignored[str(Path(rel).parent)] += 1
            continue
        files.setdefault(key, []).append(rel)
    for folder, n in ignored.most_common():
        print(f'{n} files the port would not load (no replacement key in the name) in {folder}')

    hue_rule = (f'changes the hue or saturation of the original (mean colour moves more than '
                f'{args.max_hue_shift:g} in CIELAB a*b*)') if args.max_hue_shift is not None else None
    light_rule = (f'changes the lightness of the original (mean colour moves more than '
                  f'{args.max_lightness_shift:g} in CIELAB L*)') if args.max_lightness_shift is not None else None
    scale_rule = f'less than {args.min_scale:g}x the original width: no more detail than the original, only re-encoded'
    size_rule = 'smaller than the original along a side: no added detail'
    undumped_rule = 'not drawn in a dumped run yet: no original to judge it against'

    chosen = []  # (path, comment)
    unjudged = []
    left_out = collections.defaultdict(list)  # reason -> (path, detail)
    for key, paths in sorted(files.items(), key=lambda kv: kv[1][0]):
        path = paths[0]
        stem = names.stem(key)
        extra = f'; other files with this key, not taken: {", ".join(paths[1:])}' if len(paths) > 1 else ''
        skip = next((r for prefix, r in skips if path.startswith(prefix)), None)
        if key in covered:
            left_out[f'covered by {covered[key]}'].append((path, stem))
            continue
        if skip:
            left_out[skip].append((path, stem))
            continue
        ow, oh = names.size_of(key)
        w, h = image_size(root / path)
        if w < ow or h < oh:
            left_out[size_rule].append((path, f'{w}x{h} for {ow}x{oh}'))
            continue
        scale = min(w / ow, h / oh)
        if scale < args.min_scale:
            left_out[scale_rule].append((path, f'{scale:g}x'))
            continue
        original = args.dumps / f'{stem}.png'
        if not original.exists():
            if args.undumped == 'include':
                unjudged.append((path, f'# {stem}, {scale:g}x, not judged (not drawn in a dumped run yet){extra}'))
            else:
                left_out[undumped_rule].append((path, f'{scale:g}x'))
            continue
        detail = f'{scale:g}x'
        if hue_rule:
            hue, light = colour_change(original, root / path)
            detail += f', a*b* {hue:.1f}, L* {light:+.1f}'
            if hue > args.max_hue_shift:
                left_out[hue_rule].append((path, detail))
                continue
            if abs(light) > args.max_lightness_shift:
                left_out[light_rule].append((path, detail))
                continue
        chosen.append((path, f'# {stem}, {detail}{extra}'))

    lines = [f'# {l}' if l else '#' for l in args.header.splitlines()]
    lines += [f'# Made by scripts/texture-select.py from {args.pack.name} against the dumps in {args.dumps}: '
              f'{len(chosen)} files judged against the original'
              + (f', {len(unjudged)} taken unjudged' if unjudged else '') + '.', '']
    for path, comment in chosen:
        lines += [comment, path]
    if unjudged:
        lines += ['', f'# Taken unjudged ({len(unjudged)} files): {undumped_rule}']
        for path, comment in unjudged:
            lines += [comment, path]
    for reason, items in left_out.items():
        lines += ['', f'# Left out ({len(items)} files): {reason}']
        lines += [f'#   {path}  ({detail})' for path, detail in items]
    args.selection.write_text('\n'.join(lines) + '\n', encoding='utf-8')
    print(f'{len(files)} keys in the pack: {len(chosen)} selected, {len(unjudged)} unjudged; left out: '
          + ', '.join(f'{len(v)} ({k[:50]})' for k, v in left_out.items()) + f' -> {args.selection}')


if __name__ == '__main__':
    main()
