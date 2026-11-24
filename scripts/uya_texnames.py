"""PCSX2 replacement-texture file names, as the port reads them (rac-rs texpack/texname.rs, PCSX2's
GSTextureReplacements::ParseReplacementName): the key a file replaces, or None for a file PCSX2 would
ignore. Shared by the texture scripts (they load it with importlib, since `python -I` does not put
the script's folder on the path).

A key is (tex0 hash, clut hash or 0, region width, region height, bits with bit 14 cleared). Formats,
tried in PCSX2's order: region + CLUT, region, old region + CLUT, old region, CLUT, plain:
  <tex0>-<clut>-r<W>x<H>-<bits>   <tex0>-r<W>x<H>-<bits>   (W, H decimal)
  <tex0>-<clut>-r<hex>-<bits>     <tex0>-r<hex>-<bits>     (old: the 64-bit region, x in the low half)
  <tex0>-<clut>-<bits>            <tex0>-<bits>
Hashes are up to 16 hex digits, bits up to 8; the extension is png or dds (any case); `-mipN` dumps
never parse.
"""
import re

H = r'([0-9a-fA-F]{1,16})'
B = r'([0-9a-fA-F]{1,8})'
EXT = r'\.(png|dds)$'
PATTERNS = [
    ('clut', 'new', re.compile(rf'^{H}-{H}-r(\d+)x(\d+)-{B}{EXT}', re.I)),
    ('plain', 'new', re.compile(rf'^{H}-r(\d+)x(\d+)-{B}{EXT}', re.I)),
    ('clut', 'old', re.compile(rf'^{H}-{H}-r{H}-{B}{EXT}', re.I)),
    ('plain', 'old', re.compile(rf'^{H}-r{H}-{B}{EXT}', re.I)),
    ('clut', None, re.compile(rf'^{H}-{H}-{B}{EXT}', re.I)),
    ('plain', None, re.compile(rf'^{H}-{B}{EXT}', re.I)),
]
UNUSED_BIT = 1 << 14
PALETTE_PSMS = {0x13, 0x14, 0x1b, 0x24, 0x2c}


def old_region_size(bits):
    def f(shift):
        v = (bits >> shift) & 0xffff
        return v - 0x10000 if v & 0x8000 else v
    return (f(16) - f(0)) & 0xffffffff, (f(48) - f(32)) & 0xffffffff


def parse(file_name):
    """The key of a replacement file name, or None."""
    for clut, region, rx in PATTERNS:
        m = rx.match(file_name)
        if not m:
            continue
        g = list(m.groups())[:-1]  # the extension is last
        tex0 = int(g.pop(0), 16)
        clut_hash = int(g.pop(0), 16) if clut == 'clut' else 0
        rw = rh = 0
        if region == 'new':
            rw, rh = int(g.pop(0)), int(g.pop(0))
        elif region == 'old':
            rw, rh = old_region_size(int(g.pop(0), 16))
        bits = int(g.pop(0), 16) & ~UNUSED_BIT
        return tex0, clut_hash, rw, rh, bits
    return None


def stem(key):
    """The file stem PCSX2 dumps a key under (`%llx-[%llx-][rWxH-]%08x`)."""
    tex0, clut, rw, rh, bits = key
    s = f'{tex0:x}'
    if (bits & 0x3f) in PALETTE_PSMS:
        s += f'-{clut:x}'
    if rw or rh:
        s += f'-r{rw}x{rh}'
    return s + f'-{bits:08x}'


def size_of(key):
    """The size in texels of the rectangle a key names: the region, else 2^TW x 2^TH from the bits."""
    _, _, rw, rh, bits = key
    return (rw or 1 << ((bits >> 6) & 0xf), rh or 1 << ((bits >> 10) & 0xf))
