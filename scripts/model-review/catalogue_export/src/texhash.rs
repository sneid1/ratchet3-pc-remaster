// Copied verbatim from port/uya/src/texnames.rs (TexName::stem, name, indexed_key, t8_block_byte, xxh3_64) so
// exported texture stems equal the PCSX2 dump / HD pack names. Do not edit here; re-copy if the original changes.
#![allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TexName { tex0: u64, clut: u64, bits: u32 }
impl TexName { pub fn stem(&self) -> String { format!("{:x}-{:x}-{:08x}", self.tex0, self.clut, self.bits) } }
pub fn name(t: &uyafmt::core::Texture) -> Result<Option<TexName>, String> {
    let Some(g) = &t.gs else { return Ok(None) };
    let (w, h) = (t.width, t.height);
    if !w.is_power_of_two() || !h.is_power_of_two() || w > 1024 || h > 1024 || g.texels.len() != w * h || g.clut.len() != 256 {
        return Err(format!("texnames: a {w} x {h} texture of {} texels and {} palette colours", g.texels.len(), g.clut.len()));
    }
    let (tex0, bits) = indexed_key(g.psm, w, h, &g.texels)?;
    let clut: Vec<u8> = g.clut.iter().flatten().copied().collect();
    Ok(Some(TexName { tex0, clut: xxh3_64(&clut), bits }))
}

/// The image half of a name, shared by the draw path and load-time HUD/GS RAM discovery. The palette a sprite uses is
/// selected by the game's draw, so hashing images separately avoids constructing every image/palette combination.
fn indexed_key(psm: u32, w: usize, h: usize, indices: &[u8]) -> Result<(u64, u32), String> {
    if !w.is_power_of_two() || !h.is_power_of_two() || w > 1024 || h > 1024 || indices.len() != w * h {
        return Err(format!("texnames: a {w} x {h} indexed texture of {} texels", indices.len()));
    }
    let texels: Vec<u8> = match psm {
        // PSMT8 of at least one block: each 16 x 16 block's 256 bytes as GS memory holds them, blocks in rows.
        0x13 if w >= 16 && h >= 16 => {
            let mut b = vec![0u8; w * h];
            for by in 0..h / 16 {
                for bx in 0..w / 16 {
                    let block = &mut b[(by * (w / 16) + bx) * 256..][..256];
                    for y in 0..16 {
                        for x in 0..16 {
                            block[t8_block_byte(x, y)] = indices[(by * 16 + y) * w + bx * 16 + x];
                        }
                    }
                }
            }
            b
        }
        // A smaller PSMT8 texture, and PSMT8H (the index in a 32-bit texel's top byte): the rows of indices.
        0x13 | 0x1B => indices.to_vec(),
        p => return Err(format!("texnames: PSM {p:#x}")),
    };
    let bits = psm | w.trailing_zeros() << 6 | h.trailing_zeros() << 10;
    Ok((xxh3_64(&texels), bits))
}
fn t8_block_byte(x: usize, y: usize) -> usize {
    let swap = ((y + 2) >> 2 & 1) * 4;
    let v = (((y & !3) >> 1) + (y & 1)) & 7;
    let u = (x + swap) & 7;
    let word = (v >> 1) * 16 + (u >> 1) * 4 + (v & 1) * 2 + (u & 1);
    word * 4 + (x >> 3 & 1) * 2 + (y >> 1 & 1)
}

/// XXH3-64 with seed 0 and the default secret (the hash PCSX2 names textures by), for the input sizes a texture or a palette has.
fn xxh3_64(input: &[u8]) -> u64 {
    const SECRET: [u8; 192] = [
        0xb8, 0xfe, 0x6c, 0x39, 0x23, 0xa4, 0x4b, 0xbe, 0x7c, 0x01, 0x81, 0x2c, 0xf7, 0x21, 0xad, 0x1c, 0xde, 0xd4, 0x6d, 0xe9, 0x83, 0x90, 0x97, 0xdb, 0x72, 0x40, 0xa4, 0xa4, 0xb7, 0xb3, 0x67, 0x1f, 0xcb, 0x79, 0xe6, 0x4e, 0xcc, 0xc0, 0xe5, 0x78, 0x82, 0x5a, 0xd0, 0x7d, 0xcc, 0xff, 0x72, 0x21, 0xb8, 0x08, 0x46, 0x74, 0xf7, 0x43, 0x24, 0x8e, 0xe0, 0x35, 0x90, 0xe6, 0x81, 0x3a, 0x26, 0x4c, 0x3c, 0x28, 0x52, 0xbb, 0x91, 0xc3, 0x00, 0xcb, 0x88, 0xd0, 0x65, 0x8b, 0x1b, 0x53, 0x2e, 0xa3, 0x71, 0x64, 0x48, 0x97, 0xa2, 0x0d, 0xf9, 0x4e, 0x38, 0x19, 0xef, 0x46, 0xa9, 0xde, 0xac, 0xd8, 0xa8, 0xfa, 0x76, 0x3f, 0xe3, 0x9c, 0x34, 0x3f, 0xf9, 0xdc, 0xbb, 0xc7, 0xc7, 0x0b, 0x4f, 0x1d, 0x8a, 0x51, 0xe0, 0x4b, 0xcd, 0xb4, 0x59, 0x31, 0xc8, 0x9f, 0x7e, 0xc9, 0xd9, 0x78, 0x73, 0x64, 0xea, 0xc5, 0xac, 0x83, 0x34, 0xd3, 0xeb, 0xc3, 0xc5, 0x81, 0xa0, 0xff, 0xfa, 0x13, 0x63, 0xeb, 0x17, 0x0d, 0xdd, 0x51, 0xb7, 0xf0, 0xda, 0x49, 0xd3, 0x16, 0x55, 0x26, 0x29, 0xd4, 0x68, 0x9e, 0x2b, 0x16, 0xbe, 0x58, 0x7d, 0x47, 0xa1, 0xfc, 0x8f, 0xf8, 0xb8, 0xd1, 0x7a, 0xd0, 0x31, 0xce, 0x45, 0xcb, 0x3a, 0x8f, 0x95, 0x16, 0x04, 0x28, 0xaf, 0xd7, 0xfb, 0xca, 0xbb, 0x4b, 0x40, 0x7e,
    ];
    const P32_1: u64 = 0x9E37_79B1;
    const P32_2: u64 = 0x85EB_CA77;
    const P32_3: u64 = 0xC2B2_AE3D;
    const P64_1: u64 = 0x9E37_79B1_85EB_CA87;
    const P64_2: u64 = 0xC2B2_AE3D_27D4_EB4F;
    const P64_3: u64 = 0x1656_67B1_9E37_79F9;
    const P64_4: u64 = 0x85EB_CA77_C2B2_AE63;
    const P64_5: u64 = 0x27D4_EB2F_1656_67C5;
    let r64 = |b: &[u8], o: usize| u64::from_le_bytes(b[o..o + 8].try_into().expect("8 bytes"));
    let r32 = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().expect("4 bytes")) as u64;
    let fold = |a: u64, b: u64| {
        let m = a as u128 * b as u128;
        m as u64 ^ (m >> 64) as u64
    };
    let avalanche = |mut h: u64| {
        h ^= h >> 37;
        h = h.wrapping_mul(0x1656_6791_9E37_79F9);
        h ^ h >> 32
    };
    let mix16 = |i: usize, s: usize| fold(r64(input, i) ^ r64(&SECRET, s), r64(input, i + 8) ^ r64(&SECRET, s + 8));
    let len = input.len();
    if len == 0 {
        let mut h = r64(&SECRET, 56) ^ r64(&SECRET, 64);
        h ^= h >> 33;
        h = h.wrapping_mul(P64_2);
        h ^= h >> 29;
        h = h.wrapping_mul(P64_3);
        return h ^ h >> 32;
    }
    if len <= 3 {
        let combo = (input[0] as u64) << 16 | (input[len >> 1] as u64) << 24 | input[len - 1] as u64 | (len as u64) << 8;
        let mut h = combo ^ (r32(&SECRET, 0) ^ r32(&SECRET, 4));
        h ^= h >> 33;
        h = h.wrapping_mul(P64_2);
        h ^= h >> 29;
        h = h.wrapping_mul(P64_3);
        return h ^ h >> 32;
    }
    if len <= 8 {
        let flip = r64(&SECRET, 8) ^ r64(&SECRET, 16);
        let mut h = (r32(input, len - 4) + (r32(input, 0) << 32)) ^ flip;
        h ^= h.rotate_left(49) ^ h.rotate_left(24);
        h = h.wrapping_mul(0x9FB2_1C65_1E98_DF25);
        h ^= (h >> 35).wrapping_add(len as u64);
        h = h.wrapping_mul(0x9FB2_1C65_1E98_DF25);
        return h ^ h >> 28;
    }
    if len <= 16 {
        let lo = r64(input, 0) ^ (r64(&SECRET, 24) ^ r64(&SECRET, 32));
        let hi = r64(input, len - 8) ^ (r64(&SECRET, 40) ^ r64(&SECRET, 48));
        return avalanche((len as u64).wrapping_add(lo.swap_bytes()).wrapping_add(hi).wrapping_add(fold(lo, hi)));
    }
    if len <= 128 {
        let mut acc = (len as u64).wrapping_mul(P64_1);
        if len > 32 {
            if len > 64 {
                if len > 96 {
                    acc = acc.wrapping_add(mix16(48, 96)).wrapping_add(mix16(len - 64, 112));
                }
                acc = acc.wrapping_add(mix16(32, 64)).wrapping_add(mix16(len - 48, 80));
            }
            acc = acc.wrapping_add(mix16(16, 32)).wrapping_add(mix16(len - 32, 48));
        }
        acc = acc.wrapping_add(mix16(0, 0)).wrapping_add(mix16(len - 16, 16));
        return avalanche(acc);
    }
    if len <= 240 {
        let mut acc = (len as u64).wrapping_mul(P64_1);
        for i in 0..8 {
            acc = acc.wrapping_add(mix16(16 * i, 16 * i));
        }
        acc = avalanche(acc);
        for i in 8..len / 16 {
            acc = acc.wrapping_add(mix16(16 * i, 16 * (i - 8) + 3));
        }
        return avalanche(acc.wrapping_add(mix16(len - 16, 136 - 17)));
    }
    // The long input: 8 accumulators over 64-byte stripes, the secret stepping 8 bytes a stripe, scrambled after each block.
    let mut acc = [P32_3, P64_1, P64_2, P64_3, P64_4, P32_2, P64_5, P32_1];
    let accumulate = |acc: &mut [u64; 8], at: usize, s: usize| {
        for i in 0..8 {
            let v = r64(input, at + 8 * i);
            let k = v ^ r64(&SECRET, s + 8 * i);
            acc[i ^ 1] = acc[i ^ 1].wrapping_add(v);
            acc[i] = acc[i].wrapping_add((k & 0xFFFF_FFFF).wrapping_mul(k >> 32));
        }
    };
    let stripes = (192 - 64) / 8;
    let block = 64 * stripes;
    let blocks = (len - 1) / block;
    for b in 0..blocks {
        for n in 0..stripes {
            accumulate(&mut acc, b * block + n * 64, n * 8);
        }
        for (i, a) in acc.iter_mut().enumerate() {
            *a = ((*a ^ *a >> 47) ^ r64(&SECRET, 192 - 64 + 8 * i)).wrapping_mul(P32_1);
        }
    }
    for n in 0..((len - 1) - block * blocks) / 64 {
        accumulate(&mut acc, blocks * block + n * 64, n * 8);
    }
    accumulate(&mut acc, len - 64, 192 - 64 - 7);
    let mut h = (len as u64).wrapping_mul(P64_1);
    for i in 0..4 {
        h = h.wrapping_add(fold(acc[2 * i] ^ r64(&SECRET, 11 + 16 * i), acc[2 * i + 1] ^ r64(&SECRET, 11 + 16 * i + 8)));
    }
    avalanche(h)
}
