"""Reads Ratchet & Clank: Rift Apart's PC assets (the player's own Steam copy): the toc, the dag and
the DSAR archives in d/, and its textures as RGBA pictures (plan item 9.2, step 2).

Nothing this reads is ever committed or uploaded; it is the player's own copy.

Layout, from the files themselves and RiftApart.exe:

  toc    u32 0x34E89035, u32 size, then a DAT1 block (u32 '1TAD', u32 type 0x4D7CF320, u32 size,
         u32 section count, sections of (u32 hash, u32 offset from the DAT1, u32 size)):
           0x398ABFF0  archives, 66 bytes each: the name ("d\\texture") in 40 bytes, 24 bytes of the
                       build machine's pointers, u8 0, u8 language (0 for every language)
           0x506D7B8A  asset ids, u64, sorted within each span
           0x65BCF461  per asset (u32 size, u32 archive, u32 offset in the archive's unpacked
                       stream, u32 offset of its 36-byte header in 0x654BDED9 or 0xFFFFFFFF)
           0x654BDED9  36-byte asset headers (u32 type hash, u32 DAT1 size, u32 data size, zeros)
           0xEDE8ADA9  256 spans of (u32 first asset, u32 count): span 0 is the game, the others
                       each language's sound
           0x62297090  u32 count of the streamed textures; 0x36A6C8CC their ids (u64);
           0xC9FB9DDA  their 72-byte descriptions
  dag    u32 0xB8EF3955, u32 size, u32 0, then a DAT1 (type 0x2A077A51, "DependencyDag"):
           0xD101A6CC  per asset the offset of its path in the DAT1, 0x7A0266BC its type (u8)
  ids    an asset's id is (crc64(path) >> 2) | 1 << 63: CRC-64 with the reflected ECMA-182 table
         (poly 0xC96C5795D7870F42), initial value 0xC96C5795D7870F42, no final xor, over the path
         lowercased (A-Z only), every run of / and \\ written as one /, leading and trailing ones
         dropped (RiftApart.exe 0x141595490, called from 0x14158F9B0).
  d\\*    DSAR archives: 'DSAR', u32 0x00010003 or 0x80010003, u32 block count, u32 header size,
         u64 unpacked size, "PADDING*", then 32-byte blocks (u64 unpacked offset, u64 file offset,
         u32 unpacked size, u32 packed size, u8 compression, 7 bytes 0x55). Compression 3 is an LZ4
         block; 2 is GDeflate, unpacked by the game's own dstorage.dll. The movie, soundbank and
         wem archives are not DSAR (Bink, DAT1 sound banks and RIFF wems); no texture is in them.

  textures  Every .texture is one id with up to two assets. Span 0: a DAT1 (type 0x8F53A199, 0x5C
         bytes, one section 0x4EDE3593 at 0x30 of 44 bytes, then "Texture Built File") followed by
         the small mips; its toc header repeats the type, 0x5C and the small mips' size. Span 1,
         when the texture is larger than its small mips: the large mips, raw. The 44 bytes:
           0x00 u32 small mips' bytes      0x04 u32 large mips' bytes
           0x08 u16 width, 0x0A height     0x0C u16 width, 0x0E height of the first small mip
           0x10 u16 array slices           0x12 u16 flags (0x40: a cubemap, six faces a slice)
           0x14 u16 DXGI format            0x1E u8 small mip count, 0x20 u8 large mip count
         Within each part the mips run layer by layer (each slice, or each face of a cubemap's
         slice), each layer from its largest mip down: the large part holds mips 0 .. large-1, the
         small part the next ones. Over all 12,502 textures this gives every part's size exactly
         (the 108 with flag 0x40 at six faces, the others at one).
         Albedo (_c) is BC7 or BC1, sRGB or not, usually 4 slices of colour variants; normals (_n)
         are BC7 with X in red and Y in green (blue 0, alpha 255).
"""
import ctypes
import os
import struct
from pathlib import Path

import lz4.block

DEFAULT_GAME = Path(r"C:\Program Files (x86)\Steam\steamapps\common\Ratchet & Clank - Rift Apart")


class FormatError(Exception):
    """A value this reader has not settled: the file, the offset and the value."""

    def __init__(self, file, offset, what, value):
        super().__init__(file, offset, what, value)  # kept as args, so it crosses a process pool

    def __str__(self):
        file, offset, what, value = self.args
        return f"{file} at {offset:#x}: {what} = {value!r}"


def _crc64_table():
    tab = []
    for i in range(256):
        c = i
        for _ in range(8):
            c = (c >> 1) ^ 0xC96C5795D7870F42 if c & 1 else c >> 1
        tab.append(c)
    return tab


_CRC64 = _crc64_table()


def normalize_path(path):
    out = []
    for ch in path:
        if ch in "/\\":
            if out and out[-1] != "/":
                out.append("/")
        else:
            out.append(chr(ord(ch) + 32) if "A" <= ch <= "Z" else ch)
    while out and out[-1] == "/":
        out.pop()
    return "".join(out)


def asset_id(path):
    c = 0xC96C5795D7870F42
    for b in normalize_path(path).encode():
        c = _CRC64[(c ^ b) & 0xFF] ^ (c >> 8)
    return (c >> 2) | 0x8000000000000000


def dat1_sections(data, base, file):
    magic, typ, size, count = struct.unpack_from("<IIII", data, base)
    if magic != 0x44415431:
        raise FormatError(file, base, "DAT1 magic", hex(magic))
    secs = {}
    for i in range(count):
        h, o, z = struct.unpack_from("<III", data, base + 16 + 12 * i)
        secs[h] = (base + o, z)
    return typ, size, secs


class Asset:
    __slots__ = ("index", "id", "archive", "offset", "size", "header", "span", "name")


class Toc:
    def __init__(self, game=DEFAULT_GAME):
        self.game = Path(game)
        path = self.game / "toc"
        t = path.read_bytes()
        magic, size = struct.unpack_from("<II", t, 0)
        if magic != 0x34E89035:
            raise FormatError(path, 0, "toc magic", hex(magic))
        typ, dsize, S = dat1_sections(t, 8, path)
        if typ != 0x4D7CF320:
            raise FormatError(path, 12, "toc DAT1 type", hex(typ))
        need = (0x398ABFF0, 0x506D7B8A, 0x65BCF461, 0x654BDED9, 0xEDE8ADA9)
        for h in need:
            if h not in S:
                raise FormatError(path, 8, "missing toc section", hex(h))
        o, z = S[0x398ABFF0]
        if z % 66:
            raise FormatError(path, o, "archive table size", z)
        self.archives = []
        for i in range(z // 66):
            e = t[o + 66 * i:o + 66 * i + 66]
            self.archives.append(e[:40].split(b"\0", 1)[0].decode())
        o, z = S[0x506D7B8A]
        n = z // 8
        ids = struct.unpack_from("<%dQ" % n, t, o)
        o2, z2 = S[0x65BCF461]
        if z2 != 16 * n:
            raise FormatError(path, o2, "size table entries", (z2, n))
        ent = struct.unpack_from("<%dI" % (4 * n), t, o2)
        ho, hz = S[0x654BDED9]
        so, _ = S[0xEDE8ADA9]
        spans = [struct.unpack_from("<II", t, so + 8 * i) for i in range(256)]
        span_of = [0] * n
        for s, (first, count) in enumerate(spans):
            for k in range(first, first + count):
                span_of[k] = s
        self.assets = []
        self.by_id = {}
        for i in range(n):
            a = Asset()
            a.index, a.id = i, ids[i]
            a.size, a.archive, a.offset, hoff = ent[4 * i:4 * i + 4]
            a.header = None if hoff == 0xFFFFFFFF else t[ho + hoff:ho + hoff + 36]
            if a.archive >= len(self.archives):
                raise FormatError(path, o2 + 16 * i + 4, "archive index", a.archive)
            a.span = span_of[i]
            a.name = None
            self.assets.append(a)
            self.by_id.setdefault(a.id, []).append(a)
        self._archives = {}

    def name_assets(self, dag):
        for path in dag.names:
            for a in self.by_id.get(asset_id(path), ()):
                a.name = path

    def archive(self, index):
        if index not in self._archives:
            self._archives[index] = Dsar(self.game / self.archives[index])
        return self._archives[index]

    def read(self, asset):
        return self.archive(asset.archive).read(asset.offset, asset.size)


class Dag:
    def __init__(self, game=DEFAULT_GAME):
        path = Path(game) / "dag"
        d = path.read_bytes()
        magic, = struct.unpack_from("<I", d, 0)
        if magic != 0xB8EF3955:
            raise FormatError(path, 0, "dag magic", hex(magic))
        typ, size, S = dat1_sections(d, 12, path)
        if typ != 0x2A077A51:
            raise FormatError(path, 16, "dag DAT1 type", hex(typ))
        o, z = S[0xD101A6CC]
        n = z // 4
        offs = struct.unpack_from("<%dI" % n, d, o)
        to, tz = S[0x7A0266BC]
        if tz != n:
            raise FormatError(path, to, "dag type table size", (tz, n))
        self.names = []
        self.types = d[to:to + n]
        for k in offs:
            s = 12 + k
            self.names.append(d[s:d.index(0, s)].decode())


class _GDeflate:
    """The game's own DirectStorage CPU codec (dstorage.dll, IDStorageCompressionCodec)."""

    def __init__(self, game):
        game = str(game)
        # dstorage.dll loads dstoragecore.dll by name; loading it first by its full path makes
        # that find the game's own copy.
        self._core = ctypes.WinDLL(os.path.join(game, "dstoragecore.dll"))
        self._ds = ctypes.WinDLL(os.path.join(game, "dstorage.dll"))

        class GUID(ctypes.Structure):
            _fields_ = [("a", ctypes.c_uint32), ("b", ctypes.c_uint16), ("c", ctypes.c_uint16),
                        ("d", ctypes.c_ubyte * 8)]

        iid = GUID(0x84EF5121, 0x9B43, 0x4D03, (ctypes.c_ubyte * 8)(0xB5, 0xC1, 0xCC, 0x34, 0x60, 0x6B, 0x26, 0x2D))
        self.codec = ctypes.c_void_p()
        create = self._ds.DStorageCreateCompressionCodec
        create.restype = ctypes.c_long
        create.argtypes = [ctypes.c_int, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_void_p]
        hr = create(1, 0, ctypes.byref(iid), ctypes.byref(self.codec))  # 1 = DSTORAGE_COMPRESSION_FORMAT_GDEFLATE
        if hr != 0:
            raise OSError(f"DStorageCreateCompressionCodec failed: {hr & 0xFFFFFFFF:#010x}")
        vtbl = ctypes.cast(self.codec, ctypes.POINTER(ctypes.POINTER(ctypes.c_void_p))).contents
        proto = ctypes.WINFUNCTYPE(ctypes.c_long, ctypes.c_void_p, ctypes.c_char_p, ctypes.c_size_t,
                                   ctypes.c_void_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t))
        self._decompress = proto(vtbl[4])

    def decompress(self, data, size, where):
        out = ctypes.create_string_buffer(size)
        written = ctypes.c_size_t()
        hr = self._decompress(self.codec, data, len(data), out, size, ctypes.byref(written))
        if hr != 0 or written.value != size:
            raise FormatError(*where, "GDeflate block", (hr & 0xFFFFFFFF, written.value, size))
        return out.raw


_gdeflate = None


class Dsar:
    def __init__(self, path):
        self.path = Path(path)
        self.f = open(self.path, "rb")
        h = self.f.read(32)
        if h[:4] != b"DSAR":
            raise FormatError(self.path, 0, "archive magic", h[:4])
        ver, nb, hsize, self.size = struct.unpack_from("<IIIQ", h, 4)
        if ver not in (0x00010003, 0x80010003) or h[24:32] != b"PADDING*" or hsize != 32 + 32 * nb:
            raise FormatError(self.path, 4, "DSAR header", (hex(ver), hsize, h[24:32]))
        bt = self.f.read(32 * nb)
        self.blocks = []
        for i in range(nb):
            ro, co, rs, cs, ty = struct.unpack_from("<QQIIB", bt, 32 * i)
            if ty not in (2, 3):
                raise FormatError(self.path, 32 + 32 * i + 24, "block compression", ty)
            self.blocks.append((ro, co, rs, cs, ty))
        self.starts = [b[0] for b in self.blocks]
        self._cache = (None, None)

    def _block(self, i):
        if self._cache[0] == i:
            return self._cache[1]
        global _gdeflate
        ro, co, rs, cs, ty = self.blocks[i]
        self.f.seek(co)
        c = self.f.read(cs)
        if ty == 3:
            d = lz4.block.decompress(c, uncompressed_size=rs)
            if len(d) != rs:
                raise FormatError(self.path, co, "LZ4 block size", (len(d), rs))
        else:
            if _gdeflate is None:
                _gdeflate = _GDeflate(self.path.parent.parent)
            d = _gdeflate.decompress(c, rs, (self.path, co))
        self._cache = (i, d)
        return d

    def read(self, offset, size):
        import bisect
        i = bisect.bisect_right(self.starts, offset) - 1
        out = bytearray()
        end = offset + size
        while len(out) < size:
            if i < 0 or i >= len(self.blocks):
                raise FormatError(self.path, offset, "read past the blocks", (offset, size))
            ro, co, rs, cs, ty = self.blocks[i]
            d = self._block(i)
            a = max(offset, ro) - ro
            b = min(end, ro + rs) - ro
            out += d[a:b]
            i += 1
        return bytes(out)


TEXTURE_TYPE = 0x8F53A199
TEXTURE_SECTION = 0x4EDE3593

# DXGI format: (name, bytes per 4x4 block or per pixel, block-compressed, Pillow decoder or None)
DXGI = {
    2: ("R32G32B32A32_FLOAT", 16, False, None),
    10: ("R16G16B16A16_FLOAT", 8, False, None),
    16: ("R32G32_FLOAT", 8, False, None),
    28: ("R8G8B8A8_UNORM", 4, False, ("raw", "RGBA")),
    29: ("R8G8B8A8_UNORM_SRGB", 4, False, ("raw", "RGBA")),
    34: ("R16G16_FLOAT", 4, False, None),
    35: ("R16G16_UNORM", 4, False, ("rg16",)),  # red and green at 8 bits, blue 0; the 16 bits are in the .dds
    49: ("R8G8_UNORM", 2, False, ("raw", "LA")),
    54: ("R16_FLOAT", 2, False, None),
    61: ("R8_UNORM", 1, False, ("raw", "L")),
    71: ("BC1_UNORM", 8, True, ("bcn", "RGBA", 1)),
    72: ("BC1_UNORM_SRGB", 8, True, ("bcn", "RGBA", 1)),
    77: ("BC3_UNORM", 16, True, ("bcn", "RGBA", 3)),
    78: ("BC3_UNORM_SRGB", 16, True, ("bcn", "RGBA", 3)),
    80: ("BC4_UNORM", 8, True, ("bcn", "L", 4)),
    83: ("BC5_UNORM", 16, True, ("bcn", "RGB", 5)),
    95: ("BC6H_UF16", 16, True, ("bcn", "RGB", 6)),  # 8 bits a channel: values above 1 clip
    98: ("BC7_UNORM", 16, True, ("bcn", "RGBA", 7)),
    99: ("BC7_UNORM_SRGB", 16, True, ("bcn", "RGBA", 7)),
}


class Texture:
    """One .texture: its header, and its pixels read from the archives on demand."""

    def __init__(self, toc, name):
        self.toc, self.name = toc, name
        parts = toc.by_id.get(asset_id(name))
        if not parts:
            raise KeyError(name)
        small = [a for a in parts if a.span == 0]
        large = [a for a in parts if a.span != 0]
        if len(small) != 1 or len(large) > 1:
            raise FormatError(name, 0, "texture parts (span 0, others)", (len(small), len(large)))
        self.small_asset = small[0]
        self.large_asset = large[0] if large else None
        a = self.small_asset
        if a.header is None or struct.unpack_from("<II", a.header) != (TEXTURE_TYPE, 0x5C):
            raise FormatError(name, 0, "texture toc header", a.header)
        self.archive = toc.archives[a.archive]
        head = toc.archive(a.archive).read(a.offset, 0x5C)
        typ, size, secs = dat1_sections(head, 0, name)
        if typ != TEXTURE_TYPE or size != 0x5C or secs != {TEXTURE_SECTION: (0x30, 44)}:
            raise FormatError(name, 0, "texture DAT1", (hex(typ), size, secs))
        (self.small_bytes, self.large_bytes, self.width, self.height, self.small_width,
         self.small_height, self.slices, self.flags, self.format) = struct.unpack_from("<IIHHHHHHH", head, 0x30)
        self.small_mips, self.large_mips = head[0x30 + 0x1E], head[0x30 + 0x20]
        if a.size != 0x5C + self.small_bytes:
            raise FormatError(name, 0x30, "small mips' bytes against the asset size", (self.small_bytes, a.size))
        if (self.large_asset is None) != (self.large_bytes == 0):
            raise FormatError(name, 0x34, "large mips' bytes without a large part", self.large_bytes)
        if self.large_asset is not None and self.large_asset.size != self.large_bytes:
            raise FormatError(name, 0x34, "large mips' bytes against the part", (self.large_bytes, self.large_asset.size))
        if self.format not in DXGI:
            raise FormatError(name, 0x44, "DXGI format", self.format)
        self.layers = self.slices * (6 if self.flags & 0x40 else 1)

    @property
    def format_name(self):
        return DXGI[self.format][0]

    def mip_bytes(self, w, h):
        _, unit, block, _ = DXGI[self.format]
        w, h = max(1, w), max(1, h)
        return ((w + 3) // 4) * ((h + 3) // 4) * unit if block else w * h * unit

    def _layout(self):
        """(part, offset, width, height) for each (layer, mip); stops on a size this does not explain."""
        lay = {}
        for part, first, count, total, base_w, base_h in (
                ("large", 0, self.large_mips, self.large_bytes, self.width, self.height),
                ("small", self.large_mips, self.small_mips, self.small_bytes, self.small_width, self.small_height)):
            if part == "small" and count and (max(1, self.width >> first), max(1, self.height >> first)) != (base_w, base_h):
                raise FormatError(self.name, 0x3C, "first small mip size", (base_w, base_h, first))
            off = 0
            for s in range(self.layers):
                for m in range(count):
                    w, h = max(1, base_w >> m), max(1, base_h >> m)
                    lay[(s, first + m)] = (part, off, w, h)
                    off += self.mip_bytes(w, h)
            if off != total:
                raise FormatError(self.name, 0x30, f"{part} mips' bytes against layers x mips", (total, off, hex(self.flags)))
        return lay

    def image(self, slice=0, mip=0):
        """The mip of one layer (a slice, or face slice*6+f of a cubemap) as a Pillow image (RGBA,
        RGB for BC5/BC6H/R8G8, L for one-channel formats)."""
        from PIL import Image
        dec = DXGI[self.format][3]
        if dec is None:
            raise FormatError(self.name, 0x44, "pixel format not decoded", self.format_name)
        part, off, w, h = self._layout()[(slice, mip)]
        if part == "large":
            a, base = self.large_asset, 0
        else:
            a, base = self.small_asset, 0x5C
        data = self.toc.archive(a.archive).read(a.offset + base + off, self.mip_bytes(w, h))
        if dec[0] == "rg16":
            import numpy as np
            rg = np.frombuffer(data, "<u2").reshape(h, w, 2).astype(np.uint32)
            rg8 = ((rg * 255 + 32767) // 65535).astype(np.uint8)
            return Image.fromarray(np.dstack([rg8, np.zeros((h, w, 1), np.uint8)]), "RGB")
        if dec[0] == "raw":
            im = Image.frombytes(dec[1], (w, h), data)
            if dec[1] == "LA":  # R8G8: red and green, blue 0
                l, a = im.split()
                im = Image.merge("RGB", (l, a, Image.new("L", (w, h), 0)))
            return im
        if w % 4 or h % 4:
            # a block-compressed mip smaller than a block: decode the whole block, keep its corner
            bw, bh = (w + 3) // 4 * 4, (h + 3) // 4 * 4
            return Image.frombytes(dec[1], (bw, bh), data, "bcn", dec[2]).crop((0, 0, w, h))
        return Image.frombytes(dec[1], (w, h), data, "bcn", dec[2])


def normal_rgb(image):
    """A _n map (X in red, Y in green) as an ordinary tangent-space normal map: Z rebuilt in blue."""
    import numpy as np
    from PIL import Image
    a = np.asarray(image.convert("RGBA")).astype(np.float32)
    x = a[..., 0] / 127.5 - 1.0
    y = a[..., 1] / 127.5 - 1.0
    z = np.sqrt(np.clip(1.0 - x * x - y * y, 0.0, 1.0))
    out = np.stack([a[..., 0], a[..., 1], (z * 127.5 + 127.5)], axis=-1)
    return Image.fromarray(np.clip(out + 0.5, 0, 255).astype(np.uint8), "RGB")


def textures(toc):
    """Every texture's name, in toc order."""
    for a in toc.assets:
        if a.span == 0 and a.header is not None and struct.unpack_from("<I", a.header)[0] == TEXTURE_TYPE:
            if a.name is None:
                raise FormatError(toc.game / "toc", a.index, "texture without a dag name", hex(a.id))
            yield a.name


# ---- The surface catalogue (PLAN 9.2): every texture's kind; the material library exported ----
#
# A material is the textures sharing one name before their role suffix (_c albedo, _n normal, _g,
# _m, ...). _c is the diffuse colour: black on bare metals, whose colour is in _g (specular colour in
# RGB, gloss in alpha, on the metals looked at; the channels of _g and _m are not settled further).
# _n holds X in red and Y in green, blue 0. A 4-slice _c packs the material: slice 0 the albedo, and
# slices 1, 2 and 3 alike to _g, _m and _n (the normal with Z stored): where the separate maps exist,
# the median mean difference is 2.0, 0.0 and 1.8 of 255 over 116, 112 and 119 materials, but some
# differ (slice 2 of concrete_paver_001 or plastic_001 by over 150), so every slice is exported.

import re

ROLES = {"c": "albedo", "n": "normal", "g": "gloss", "m": "mask", "ao": "ao", "h": "height", "id": "id",
         "cma": "mask", "cmb": "mask", "mask": "mask", "s": "other", "v": "other", "p": "other", "x": "other"}

# The kind a surface looks like, from the file's own name and its folder's (first match wins).
KINDS = (
    ("other", re.compile(r"mask|noise|sparkle|blockout")),
    ("sand", re.compile(r"(^|_| )sand(?!stone|wich|shark|shrak)|dune|dust|dirt(?!y)|soil|(^|_)mud|(^|_)ash(_|$)|soot")),
    ("grass", re.compile(r"grass|moss|lawn|turf|clover|ground_cover")),
    ("rock", re.compile(r"rock(?!et)|cliff|stone|gravel|pebble|boulder|obsidian|shale|coral")),
    ("floor", re.compile(r"floor|foor|linoleum|paver|asphalt|(^|_)road|plank|pavement|cobble|brick|concrete|rubber_mat|wood_tile")),
    ("metal", re.compile(r"metal|steel|brass|bronze|copper|gold|silver|chrom|alumin|grate|rust|corrugated|anodized|tech_panel|perf_")),
)
SURFACE_KINDS = ("sand", "rock", "metal", "grass", "floor")
# Characters, gear, vehicles, effects, the UI, skies, lighting and cinematics are never scenery.
NOT_SCENERY = re.compile(r"^(characters|equipment|vehicle|cinematics|coretest|levels|test|ui|materialgraph)\\"
                         r"|\\(ui|effects|visualeffect|hdr|sky|characters?|weapons?|equipment|vehicles?|cinematics|props|lighting)\\"
                         r"|^textures\\(ui|effects|visualeffect|character|equipment|vehicle|cinematics)\\")
# Outside the library, scenery whose path names ground, terrain, rock, stone, nature or tiling.
SCENERY_SURFACE = re.compile(r"\\(ground|terrain|concrete|natural|stone|rocks?)\\|(^|_|\\)gnd_|(^|_|\\)ground|terrain|"
                             r"asphalt|(^|_)road|soil|greenmetal|cliff_rock|brick|(^|_)tile|tiling|(^|_)sand(?!wich)|dirt|grass|moss")


def describe(name):
    """(base, role, group, kind, exported) for a texture path. base is the material's path without
    the role suffix; exported: the texture is in the material library export."""
    parts = name.split("\\")
    stem = parts[-1][:-len(".texture")]
    m = re.search(r"_([a-z0-9]+)$", stem)
    if m and m.group(1) in ROLES:
        role, base = ROLES[m.group(1)], "\\".join(parts[:-1] + [stem[:m.start()]])
    else:
        role, base = "other", "\\".join(parts[:-1] + [stem])
    sbs = len(parts) > 3 and parts[0] == "textures" and parts[1] == "sbs"
    group = "sbs\\" + parts[2] if sbs else "\\".join(parts[:2])
    scenery = not NOT_SCENERY.search(name.lower())
    kind = "other"
    if scenery:
        words = base.split("\\")[-1].lower() + " " + parts[-2].lower()
        for k, rx in KINDS:
            if rx.search(words):
                kind = k
                break
    exported = sbs or (scenery and bool(SCENERY_SURFACE.search(name.lower())))
    return base, role, group, kind, exported


def save_dds(t, path):
    """The texture as it is stored (every slice and mip, BCn data untouched) in a DX10 DDS file."""
    lay = t._layout()
    mips = t.large_mips + t.small_mips
    blob = bytearray()
    for s in range(t.layers):
        for m in range(mips):
            part, off, w, h = lay[(s, m)]
            a, base = (t.large_asset, 0) if part == "large" else (t.small_asset, 0x5C)
            blob += t.toc.archive(a.archive).read(a.offset + base + off, t.mip_bytes(w, h))
    _, unit, block, _ = DXGI[t.format]
    pitch = t.mip_bytes(t.width, t.height) if block else t.width * unit
    hdr = struct.pack("<4sIIIIIII44x", b"DDS ", 124, 0x1 | 0x2 | 0x4 | 0x1000 | 0x20000 | (0x80000 if block else 0x8),
                      t.height, t.width, pitch, 0, mips)
    hdr += struct.pack("<II4sIIIII", 32, 0x4, b"DX10", 0, 0, 0, 0, 0)  # pixel format: FourCC DX10
    cube = t.layers != t.slices
    hdr += struct.pack("<IIIII", 0x1000 | 0x400000 | 0x8, 0xFE00 if cube else 0, 0, 0, 0)  # caps; cubemap faces
    hdr += struct.pack("<IIIII", t.format, 3, 0x4 if cube else 0, t.slices, 0)  # DX10: format, 2D, cube, array size
    with open(path, "wb") as f:
        f.write(hdr)
        f.write(blob)


def _png_ready(name, im, role):
    """A decoded mip as PNG pixels: a _n map with Z rebuilt when its blue is 0 (448 of the library's
    450), as stored when its blue holds Z (cotton_001_n, snow_basic_002_n: blue 191 and up);
    RGB when alpha is all 255."""
    import numpy as np
    if role == "normal" and name.endswith("_n.texture"):
        blue = np.asarray(im.convert("RGB"))[..., 2]
        if blue.max() <= 8:
            return normal_rgb(im)
        if blue.min() < 128:
            raise FormatError(name, 0, "a _n map's blue neither 0 nor a Z (min, max)", (int(blue.min()), int(blue.max())))
        return im.convert("RGB")
    if im.mode == "RGBA" and np.asarray(im)[..., 3].min() == 255:
        return im.convert("RGB")
    return im


def _export_one(job):
    """Worker: one texture as PNGs, every slice at full size: <stem>.png, <stem>_slice<k>.png. BC6H
    is written at 8 bits (above 1 clipped) and R16G16 at 8 bits, each also as it is stored, <stem>.dds."""
    game, name, out_stem = job
    toc = _worker_toc(game)
    t = Texture(toc, name)
    _, role, _, _, _ = describe(name)
    written = []
    for s in range(t.layers):
        im = _png_ready(name, t.image(s, 0), role)
        p = f"{out_stem}{'' if s == 0 else f'_slice{s}'}{'_8bit' if t.format == 95 else ''}.png"
        im.save(p)
        written.append(p)
    if t.format in (35, 95):
        save_dds(t, f"{out_stem}.dds")
        written.append(f"{out_stem}.dds")
    return name, written


_WORKER_TOCS = {}


def _worker_toc(game):
    if game not in _WORKER_TOCS:
        _WORKER_TOCS[game] = Toc(game)
    return _WORKER_TOCS[game]


def catalogue(toc, out_csv):
    import csv
    names = sorted(textures(toc))
    info = {}
    with open(out_csv, "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f)
        w.writerow(["name", "archive", "id", "width", "height", "slices", "cube", "mips", "format", "bytes",
                    "role", "group", "kind", "exported"])
        for n in names:
            t = Texture(toc, n)
            base, role, group, kind, exported = describe(n)
            w.writerow([n, t.archive, f"{asset_id(n):016x}", t.width, t.height, t.slices,
                        "yes" if t.layers != t.slices else "", t.large_mips + t.small_mips, t.format_name, t.small_bytes + t.large_bytes,
                        role, group, kind, "yes" if exported else ""])
            info[n] = (base, role, group, kind, exported)
    return names, info


def materials(info):
    """{base: [texture names]} of the exported textures, with each base's kind and group."""
    mats = {}
    for n, (base, role, group, kind, exported) in info.items():
        if exported:
            mats.setdefault(base, []).append(n)
    return mats


def _folder(kind, group):
    return kind if kind in SURFACE_KINDS else "other\\" + group.replace("\\", "_")


def _font(size):
    from PIL import ImageFont
    return ImageFont.truetype(r"C:\Windows\Fonts\arial.ttf", size)


def sheets(toc, info, out_dir, per_page=16, tile=192):
    """Contact sheets of the exported materials by kind (the rest by group). Each material shows four
    maps: its albedo (_c), normal (_n, Z rebuilt), _g and _m, each taken from the 4-slice _c (slices
    3, 1, 2) when the separate file is missing; free places show its other maps. Materials with an
    albedo come first."""
    from PIL import Image, ImageDraw
    mats = materials(info)
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    font, small = _font(14), _font(12)
    pages = {}
    for base, ns in mats.items():
        _, _, group, kind, _ = info[ns[0]]
        pages.setdefault(_folder(kind, group), []).append(base)
    order = list(SURFACE_KINDS) + sorted(k for k in pages if k not in SURFACE_KINDS)
    written = []

    def mip_for(t):
        m = 0
        while max(t.width, t.height) >> (m + 1) >= tile:
            m += 1
        return min(m, t.large_mips + t.small_mips - 1)

    def thumb(name, slice):
        t = Texture(toc, name)
        im = t.image(slice, mip_for(t))
        im = _png_ready(name, im, describe(name)[1]).convert("RGB")
        s = tile / max(im.size)
        return im.resize((max(1, round(im.size[0] * s)), max(1, round(im.size[1] * s))), Image.LANCZOS)

    def slots(base, ns):
        by_suffix = {n.split("\\")[-1][len(base.split("\\")[-1]):-len(".texture")]: n for n in ns}
        c = by_suffix.get("_c")
        c4 = c is not None and Texture(toc, c).slices == 4
        out = []
        for suf, s in (("_c", 0), ("_n", 3), ("_g", 1), ("_m", 2)):
            if suf in by_suffix:
                out.append((by_suffix[suf], 0, suf))
            elif c4:
                out.append((c, s, f"_c slice {s}"))
        used = {n for n, _, _ in out}
        for suf, n in sorted(by_suffix.items()):
            if len(out) == 4:
                break
            if n not in used:
                out.append((n, 0, suf or "(no suffix)"))
        return out

    for key in order:
        bases = sorted(pages.get(key, []), key=lambda b: (not any(info[n][1] == "albedo" for n in mats[b]), b.split("\\")[-1]))
        for page in range(0, len(bases), per_page):
            chunk = bases[page:page + per_page]
            cols = 2
            cw, ch = 4 * tile + 3 * 6, tile + 42
            rows_n = (len(chunk) + cols - 1) // cols
            sheet = Image.new("RGB", (cols * (cw + 24) + 24, rows_n * (ch + 14) + 52), (24, 24, 26))
            d = ImageDraw.Draw(sheet)
            d.text((24, 14), f"Rift Apart materials: {key} ({page + 1}-{page + len(chunk)} of {len(bases)}): "
                             f"albedo, normal, _g, _m", fill=(230, 230, 230), font=_font(20))
            for i, base in enumerate(chunk):
                x = 24 + (i % cols) * (cw + 24)
                y = 52 + (i // cols) * (ch + 14)
                ns = sorted(mats[base])
                for k, (n, s, label) in enumerate(slots(base, ns)):
                    sheet.paste(thumb(n, s), (x + k * (tile + 6), y))
                    d.text((x + k * (tile + 6) + 4, y + tile - 16), label, fill=(255, 255, 255), font=small,
                           stroke_width=2, stroke_fill=(0, 0, 0))
                t = Texture(toc, ns[0])
                d.text((x, y + tile + 4), base.split("\\")[-1], fill=(235, 235, 235), font=font)
                maps = " ".join(n.split("\\")[-1][len(base.split("\\")[-1]):-len(".texture")] or "-" for n in ns)
                d.text((x, y + tile + 22), f"{t.width}x{t.height}  {info[ns[0]][2]}  maps {maps}", fill=(150, 150, 150), font=small)
            path = out_dir / f"{key.replace(chr(92), '_')}_{page // per_page + 1}.png"
            sheet.save(path)
            written.append(path)
            print(f"{path}: {len(chunk)} materials", flush=True)
    return written


def export(game, info, out_dir, workers):
    """Every exported texture at full size under <kind>\\ (sand, rock, metal, grass, floor) or
    other\\<group>\\: <name>.png per slice (a normal with Z rebuilt), and for BC6H an 8-bit PNG and
    the stored data as a .dds. Returns the files written."""
    from concurrent.futures import ProcessPoolExecutor
    mats = materials(info)
    jobs = []
    out = {}
    for base, ns in mats.items():
        for n in ns:
            _, role, group, kind, _ = info[n]
            out.setdefault(Path(out_dir) / _folder(kind, group) / n.split("\\")[-1][:-len(".texture")], []).append(n)
    for stem, ns in out.items():
        if len(ns) > 1:
            # one file name in two source folders (textures\sbs\rock\rv_sargasso_cliff_rocks_01_n and
            # textures\sargasso\rocks\...): each keeps its group in its name
            named = [Path(f"{stem}__{info[n][2].replace(chr(92), '_')}") for n in ns]
            if len(set(named)) != len(ns):
                raise FormatError(ns[0], 0, "export names still collide", ns)
        else:
            named = [stem]
        for n, p in zip(ns, named):
            p.parent.mkdir(parents=True, exist_ok=True)
            jobs.append((str(game), n, str(p)))
    files = []
    with ProcessPoolExecutor(workers) as ex:
        for k, (name, written) in enumerate(ex.map(_export_one, jobs, chunksize=4)):
            files += written
            if k % 100 == 0:
                print(f"{k}/{len(jobs)} {name}", flush=True)
    return jobs, files


def main(argv=None):
    import argparse
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--game", default=str(DEFAULT_GAME), help="the Rift Apart install folder")
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("png", help="one texture's mip as a PNG (a _n map with Z rebuilt)")
    p.add_argument("name", help=r"its path, e.g. textures\sbs\dirt\sav_dune_sand_001_c.texture")
    p.add_argument("out")
    p.add_argument("--slice", type=int, default=0)
    p.add_argument("--mip", type=int, default=0)
    p = sub.add_parser("surfaces", help="catalogue.csv of every texture, contact sheets of the material "
                                        "library by kind, and with --export every texture of it at full size")
    p.add_argument("out_dir", help=r"a folder outside the repository, e.g. C:\riftapart-export")
    p.add_argument("--no-sheets", action="store_true", help="skip the contact sheets")
    p.add_argument("--export", action="store_true", help=r"also write every library texture under <kind>\ or other\<group>\ ")
    p.add_argument("--workers", type=int, default=6, help="processes for --export")
    args = ap.parse_args(argv)
    toc = Toc(args.game)
    if args.cmd == "surfaces":
        toc.name_assets(Dag(args.game))
        out = Path(args.out_dir)
        out.mkdir(parents=True, exist_ok=True)
        names, info = catalogue(toc, out / "catalogue.csv")
        print(f"{out / 'catalogue.csv'}: {len(names)} textures, {sum(1 for v in info.values() if v[4])} in the library export", flush=True)
        if not args.no_sheets:
            sheets(toc, info, out / "sheets")
        if args.export:
            jobs, files = export(args.game, info, out, args.workers)
            print(f"{len(jobs)} textures exported as {len(files)} files under {out}")
        return
    if args.cmd == "png":
        t = Texture(toc, args.name)
        im = t.image(args.slice, args.mip)
        if args.name.endswith("_n.texture"):
            im = normal_rgb(im)
        im.save(args.out)
        print(f"{args.name}: {t.width}x{t.height} {t.format_name}, {t.slices} slice(s), "
              f"{t.large_mips}+{t.small_mips} mips, in {t.archive} -> {args.out} ({im.size[0]}x{im.size[1]})")


if __name__ == "__main__":
    main()
