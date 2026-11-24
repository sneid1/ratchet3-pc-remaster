"""PCSX2 texture names as the port makes them (port/uya/src/texnames.rs `name`, `xxh3_64`, `t8_block_byte`), with numpy only:
`stem(indices, clut, psm)` is the file stem of an 8-bit texture (indices h x w, clut 256 x RGBA in index order, alpha on the GS
scale) sampled as PSMT8 (0x13) or PSMT8H (0x1B). Loaded with importlib by the texture scripts, as uya_texnames.py is."""
import numpy as np

M = (1 << 64) - 1
SECRET = bytes([
    0xb8, 0xfe, 0x6c, 0x39, 0x23, 0xa4, 0x4b, 0xbe, 0x7c, 0x01, 0x81, 0x2c, 0xf7, 0x21, 0xad, 0x1c, 0xde, 0xd4, 0x6d, 0xe9, 0x83, 0x90, 0x97, 0xdb, 0x72, 0x40, 0xa4, 0xa4, 0xb7, 0xb3, 0x67, 0x1f, 0xcb, 0x79, 0xe6, 0x4e, 0xcc, 0xc0, 0xe5, 0x78, 0x82, 0x5a, 0xd0, 0x7d, 0xcc, 0xff, 0x72, 0x21, 0xb8, 0x08, 0x46, 0x74, 0xf7, 0x43, 0x24, 0x8e, 0xe0, 0x35, 0x90, 0xe6, 0x81, 0x3a, 0x26, 0x4c, 0x3c, 0x28, 0x52, 0xbb, 0x91, 0xc3, 0x00, 0xcb, 0x88, 0xd0, 0x65, 0x8b, 0x1b, 0x53, 0x2e, 0xa3, 0x71, 0x64, 0x48, 0x97, 0xa2, 0x0d, 0xf9, 0x4e, 0x38, 0x19, 0xef, 0x46, 0xa9, 0xde, 0xac, 0xd8, 0xa8, 0xfa, 0x76, 0x3f, 0xe3, 0x9c, 0x34, 0x3f, 0xf9, 0xdc, 0xbb, 0xc7, 0xc7, 0x0b, 0x4f, 0x1d, 0x8a, 0x51, 0xe0, 0x4b, 0xcd, 0xb4, 0x59, 0x31, 0xc8, 0x9f, 0x7e, 0xc9, 0xd9, 0x78, 0x73, 0x64, 0xea, 0xc5, 0xac, 0x83, 0x34, 0xd3, 0xeb, 0xc3, 0xc5, 0x81, 0xa0, 0xff, 0xfa, 0x13, 0x63, 0xeb, 0x17, 0x0d, 0xdd, 0x51, 0xb7, 0xf0, 0xda, 0x49, 0xd3, 0x16, 0x55, 0x26, 0x29, 0xd4, 0x68, 0x9e, 0x2b, 0x16, 0xbe, 0x58, 0x7d, 0x47, 0xa1, 0xfc, 0x8f, 0xf8, 0xb8, 0xd1, 0x7a, 0xd0, 0x31, 0xce, 0x45, 0xcb, 0x3a, 0x8f, 0x95, 0x16, 0x04, 0x28, 0xaf, 0xd7, 0xfb, 0xca, 0xbb, 0x4b, 0x40, 0x7e])
P32_1, P32_2, P32_3 = 0x9E3779B1, 0x85EBCA77, 0xC2B2AE3D
P64_1, P64_2, P64_3, P64_4, P64_5 = 0x9E3779B185EBCA87, 0xC2B2AE3D27D4EB4F, 0x165667B19E3779F9, 0x85EBCA77C2B2AE63, 0x27D4EB2F165667C5


def _r64(b, o):
    return int.from_bytes(b[o:o + 8], 'little')


def _r32(b, o):
    return int.from_bytes(b[o:o + 4], 'little')


def _fold(a, b):
    m = a * b
    return (m & M) ^ (m >> 64)


def _aval(h):
    h ^= h >> 37
    h = (h * 0x165667919E3779F9) & M
    return h ^ (h >> 32)


def _rotl(x, r):
    return ((x << r) | (x >> (64 - r))) & M


def xxh3_64(inp: bytes) -> int:
    n = len(inp)
    mix16 = lambda i, s: _fold(_r64(inp, i) ^ _r64(SECRET, s), _r64(inp, i + 8) ^ _r64(SECRET, s + 8))
    if n == 0:
        h = _r64(SECRET, 56) ^ _r64(SECRET, 64)
        h ^= h >> 33; h = (h * P64_2) & M; h ^= h >> 29; h = (h * P64_3) & M
        return h ^ (h >> 32)
    if n <= 3:
        combo = inp[0] << 16 | inp[n >> 1] << 24 | inp[n - 1] | n << 8
        h = combo ^ (_r32(SECRET, 0) ^ _r32(SECRET, 4))
        h ^= h >> 33; h = (h * P64_2) & M; h ^= h >> 29; h = (h * P64_3) & M
        return h ^ (h >> 32)
    if n <= 8:
        flip = _r64(SECRET, 8) ^ _r64(SECRET, 16)
        h = ((_r32(inp, n - 4) + (_r32(inp, 0) << 32)) & M) ^ flip
        h ^= _rotl(h, 49) ^ _rotl(h, 24)
        h = (h * 0x9FB21C651E98DF25) & M
        h ^= ((h >> 35) + n) & M
        h = (h * 0x9FB21C651E98DF25) & M
        return h ^ (h >> 28)
    if n <= 16:
        lo = _r64(inp, 0) ^ (_r64(SECRET, 24) ^ _r64(SECRET, 32))
        hi = _r64(inp, n - 8) ^ (_r64(SECRET, 40) ^ _r64(SECRET, 48))
        swapped = int.from_bytes(lo.to_bytes(8, 'little'), 'big')
        return _aval((n + swapped + hi + _fold(lo, hi)) & M)
    if n <= 128:
        acc = (n * P64_1) & M
        if n > 32:
            if n > 64:
                if n > 96:
                    acc = (acc + mix16(48, 96) + mix16(n - 64, 112)) & M
                acc = (acc + mix16(32, 64) + mix16(n - 48, 80)) & M
            acc = (acc + mix16(16, 32) + mix16(n - 32, 48)) & M
        acc = (acc + mix16(0, 0) + mix16(n - 16, 16)) & M
        return _aval(acc)
    if n <= 240:
        acc = (n * P64_1) & M
        for i in range(8):
            acc = (acc + mix16(16 * i, 16 * i)) & M
        acc = _aval(acc)
        for i in range(8, n // 16):
            acc = (acc + mix16(16 * i, 16 * (i - 8) + 3)) & M
        return _aval((acc + mix16(n - 16, 136 - 17)) & M)
    # Long input, vectorised over stripes with numpy uint64 (wrapping arithmetic).
    acc = np.array([P32_3, P64_1, P64_2, P64_3, P64_4, P32_2, P64_5, P32_1], np.uint64)
    secw = lambda s: np.frombuffer(SECRET[s:s + 64], '<u8')

    def accumulate(at, s):
        nonlocal acc
        v = np.frombuffer(inp[at:at + 64], '<u8')
        k = v ^ secw(s)
        acc[[1, 0, 3, 2, 5, 4, 7, 6]] += v
        acc += (k & np.uint64(0xFFFFFFFF)) * (k >> np.uint64(32))
    stripes = (192 - 64) // 8
    block = 64 * stripes
    blocks = (n - 1) // block
    with np.errstate(over='ignore'):
        for b in range(blocks):
            for s in range(stripes):
                accumulate(b * block + s * 64, s * 8)
            scr = np.frombuffer(SECRET[192 - 64:192], '<u8')
            acc = ((acc ^ (acc >> np.uint64(47))) ^ scr) * np.uint64(P32_1)
        for s in range(((n - 1) - block * blocks) // 64):
            accumulate(blocks * block + s * 64, s * 8)
        accumulate(n - 64, 192 - 64 - 7)
    a = [int(x) for x in acc]
    h = (n * P64_1) & M
    for i in range(4):
        h = (h + _fold(a[2 * i] ^ _r64(SECRET, 11 + 16 * i), a[2 * i + 1] ^ _r64(SECRET, 11 + 16 * i + 8))) & M
    return _aval(h)


def _t8_order():
    order = np.zeros((16, 16), np.int64)
    for y in range(16):
        for x in range(16):
            swap = ((y + 2) >> 2 & 1) * 4
            v = (((y & ~3) >> 1) + (y & 1)) & 7
            u = (x + swap) & 7
            word = (v >> 1) * 16 + (u >> 1) * 4 + (v & 1) * 2 + (u & 1)
            order[y, x] = word * 4 + (x >> 3 & 1) * 2 + (y >> 1 & 1)
    return order


T8 = _t8_order()


def stem(indices: np.ndarray, clut: np.ndarray, psm: int) -> str:
    h, w = indices.shape
    if psm == 0x13 and w >= 16 and h >= 16:
        blocks = indices.reshape(h // 16, 16, w // 16, 16).transpose(0, 2, 1, 3).reshape(-1, 16, 16)
        out = np.zeros((blocks.shape[0], 256), np.uint8)
        out[:, T8.ravel()] = blocks.reshape(-1, 256)
        texels = out.tobytes()
    elif psm in (0x13, 0x1B):
        texels = indices.astype(np.uint8).tobytes()
    else:
        raise ValueError(f'PSM {psm:#x}')
    bits = psm | (w.bit_length() - 1) << 6 | (h.bit_length() - 1) << 10
    return f'{xxh3_64(texels):x}-{xxh3_64(np.ascontiguousarray(clut, np.uint8).tobytes()):x}-{bits:08x}'
