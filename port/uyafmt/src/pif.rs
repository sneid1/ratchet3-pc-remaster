//! The "2FIP" images in the global WADs: an 8-bit paletted picture the game sends straight to the GS.
//!
//! Layout (little-endian), as the game's loaders read it:
//! - +0x00 the tag `2FIP` (no loader checks it; the WAD walk finds the images by it);
//! - +0x04 a u32 that differs from image to image and equals for identical pictures; no loader reads it (func_L01_0042A598,
//!   func_L01_0042AA88, frontbin func_00395EB8, func_00393E90, func_00393A18 and front end func_003824F0 load +8, +0xC, +0x14,
//!   +0x1C, +0x20 and +0x420 only), so the decoder skips these 4 bytes;
//! - +0x08 width and +0x0C height, u32 (power of two; every loader reads them: `lw`/`lh` at +8 and +0xC, then `plzcw` for
//!   the GS TEX0's TW and TH): frontbin func_00395EB8 (level 1's func_L01_0042AA88, in all 34 story levels), frontbin
//!   func_00393E90, frontbin func_00393A18 and front end func_003824F0;
//! - +0x10 0x13 (the GS's PSMT8) and +0x18 zero: no loader reads them (they upload as PSMT8H, 0x1B, whatever this says);
//! - +0x14 the palette's pixel format: front end func_003824F0 reads it (zero: a 0x400-byte palette of 32-bit colours,
//!   nonzero: 0x200 bytes of 16-bit colours); every other loader takes 0x400 bytes. Zero in every image on the disc;
//! - +0x1C the mip level count: func_L01_0042A598 (shared, all 34 story levels) reads it, 1 to 4;
//! - +0x20 the palette, 256 colours of R, G, B, alpha (alpha 0..0x80, the GS's range), as the GS reads it with a 16x16
//!   upload to its CLUT;
//! - +0x420 level 0, width x height palette indices; then, for each further level k, (width >> k) x (height >> k) indices.
//!
//! The image's own length is therefore 0x420 + the sum of the levels' sizes. The sectors of its WAD entry hold exactly
//! that many bytes rounded up to a sector, the rest zero (checked over all 278 images on the disc).
//!
//! Where the game's code reads the levels: level 0 by every loader; level 2 by func_L01_0042A598 (it uploads levels 0 and 2
//! of an image with three or more levels, starting level 2 at 5/4 of level 0's size, so it steps over level 1); nothing
//! reads level 1 or level 3, and those pixels go on ignored lines.

use crate::rd::Fields;
use crate::track::{Node, Space, Tracker};

const LEVEL1_WHY: &str = "func_L01_0042A598 starts level 2 at 5/4 of level 0's size, so it steps over level 1 without reading or uploading it; the other loaders (frontbin func_00395EB8, func_00393E90, func_00393A18, front end func_003824F0) take the palette and level 0 only";
const LEVEL3_WHY: &str = "func_L01_0042A598 uploads levels 0 and 2 only (the level count is clamped to 3); no loader addresses a level 3";
const HEADER: u64 = 0x20;
const PALETTE: u64 = 0x400;

/// One 2FIP image at `at` in `parent` (Space::Disc). `b` starts at the image and holds at least its length; only the image's own
/// bytes are marked. `game` names the function that loads (and uploads) the picture. On a bad value the failing node records it
/// and the error is returned.
pub fn image(t: &mut Tracker, parent: &mut Node, group: &str, at: u64, b: &[u8], game: &'static str) -> Result<(), String> {
    let lh = t.line(
        group,
        "2FIP header (+0 tag, +4 no reader (skipped), +8 width, +0xC height, +0x10 0x13, +0x14 palette format 0, +0x18 zero, +0x1C mip levels)",
        "pif::image",
        game,
        Space::Disc,
    );
    let mut h = t.child(parent, lh, at, HEADER);
    let head = header(b, &mut h);
    h.skip(4, 4);
    let place = h.place.clone();
    let (w, ht, levels) = match head {
        Ok(v) => v,
        Err(e) => {
            h.fail(e.clone());
            t.close(h);
            return Err(format!("{place}: {e}"));
        }
    };
    t.close(h);

    let lp = t.line(group, "palette (256 colours of R, G, B, alpha 0..0x80)", "pif::image", game, Space::Disc);
    let mut p = t.child(parent, lp, at + HEADER, PALETTE);
    let pal = (|| -> Result<(), String> {
        let s = b.get(HEADER as usize..(HEADER + PALETTE) as usize).ok_or_else(|| format!("palette past the end {:#x}", b.len()))?;
        if let Some(k) = (0..256).find(|&k| s[4 * k + 3] > 0x80) {
            return Err(format!("palette colour {k}: alpha {:#04x}", s[4 * k + 3]));
        }
        p.mark(0, PALETTE);
        Ok(())
    })();
    if let Err(e) = pal {
        let place = p.place.clone();
        p.fail(e.clone());
        t.close(p);
        return Err(format!("{place}: {e}"));
    }
    t.close(p);

    let mut off = HEADER + PALETTE;
    for k in 0..levels {
        let size = ((w >> k) * (ht >> k)) as u64;
        let l = match k {
            0 => t.line(group, "pixels, level 0 (width x height palette indices)", "pif::image", game, Space::Disc),
            1 => t.ignored_line(group, "pixels, level 1 (half size; never uploaded)", LEVEL1_WHY, Space::Disc),
            2 => t.line(group, "pixels, level 2 (quarter size)", "pif::image", "func_L01_0042A598 (shared, all 34 story levels) uploads it when the level count is 3 or more", Space::Disc),
            _ => t.ignored_line(group, "pixels, level 3 (eighth size; never uploaded)", LEVEL3_WHY, Space::Disc),
        };
        let mut n = t.child(parent, l, at + off, size);
        if off + size > b.len() as u64 {
            let place = n.place.clone();
            let e = format!("level {k} of {size:#x} bytes at +{off:#x} past the end {:#x}", b.len());
            n.fail(e.clone());
            t.close(n);
            return Err(format!("{place}: {e}"));
        }
        // Palette indices: every byte value is one.
        if k == 0 || k == 2 {
            n.mark(0, size);
        }
        t.close(n);
        off += size;
    }
    Ok(())
}

fn header(b: &[u8], h: &mut Node) -> Result<(u32, u32, u32), String> {
    if b.len() < HEADER as usize {
        return Err(format!("{:#x} bytes, header needs {HEADER:#x}", b.len()));
    }
    let mut f = Fields::new(&b[..HEADER as usize], h);
    f.same(0, b"2FIP", "tag")?;
    let w = f.u32(8)?;
    let ht = f.u32(0xC)?;
    if !(8..=1024).contains(&w) || !w.is_power_of_two() || !(8..=1024).contains(&ht) || !ht.is_power_of_two() {
        return Err(format!("size {w} x {ht}: not a power of two from 8 to 1024"));
    }
    f.u32_eq(0x10, 0x13, "pixel format (PSMT8)")?;
    let cpsm = f.u32(0x14)?;
    if cpsm != 0 {
        return Err(format!("+0x14 palette format {cpsm:#x}: only 0 (a 0x400-byte palette) is in the data"));
    }
    f.u32_eq(0x18, 0, "zero")?;
    let levels = f.u32(0x1C)?;
    if !(1..=4).contains(&levels) || (w >> (levels - 1)) == 0 || (ht >> (levels - 1)) == 0 {
        return Err(format!("mip level count {levels} for {w} x {ht}"));
    }
    Ok((w, ht, levels))
}

/// The picture's level 0 as the GS samples it: each texel the palette colour of its index, the palette taken in the order a 16x16
/// CLUT upload leaves it (CSM1: indices 8 to 15 and 16 to 23 of each 32 swapped, as `core::texture` does), alpha in the GS's range
/// (0x80 = 1.0). Only level 0: the loaders upload level 0 (and func_L01_0042A598 level 2), and what mip levels the GS samples
/// for a hero armor or wrench texture is not settled here.
pub fn texture(b: &[u8]) -> Result<crate::core::Texture, String> {
    let u32_at = |o: usize| b.get(o..o + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]])).ok_or_else(|| format!("2FIP image of {:#x} bytes: no word at {o:#x}", b.len()));
    if b.get(0..4) != Some(b"2FIP".as_slice()) {
        return Err("no 2FIP tag".into());
    }
    let (w, h) = (u32_at(8)? as usize, u32_at(0xC)? as usize);
    if !(8..=1024).contains(&w) || !w.is_power_of_two() || !(8..=1024).contains(&h) || !h.is_power_of_two() || u32_at(0x10)? != 0x13 || u32_at(0x14)? != 0 {
        return Err(format!("2FIP image {w} x {h}, format {:#x}, palette format {:#x}", u32_at(0x10)?, u32_at(0x14)?));
    }
    let pal = b.get(HEADER as usize..(HEADER + PALETTE) as usize).ok_or("2FIP image shorter than its palette")?;
    let idx = b.get((HEADER + PALETTE) as usize..(HEADER + PALETTE) as usize + w * h).ok_or("2FIP image shorter than its level 0")?;
    let colour = |i: usize| {
        let j = (i & !0x18) | ((i & 8) << 1) | ((i & 0x10) >> 1);
        [pal[4 * j], pal[4 * j + 1], pal[4 * j + 2], pal[4 * j + 3]]
    };
    let clut: Vec<[u8; 4]> = (0..256).map(colour).collect();
    Ok(crate::core::Texture { width: w, height: h, levels: vec![idx.iter().map(|&i| colour(i as usize)).collect()], gs: Some(crate::core::GsLevel { psm: 0x13, texels: idx.to_vec(), clut }) })
}
