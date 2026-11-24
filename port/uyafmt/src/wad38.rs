//! WAD3 and WAD8 entries other than the 2FIP image pairs wads.rs places: the credits texts and their background frames, the
//! frame stream sets, WAD8's text tables, level maps, packed images, front-end object bank and RGBA frame.
//!
//! The headers are (sector, sectors) pairs after the size and sector words (headers.rs); which code reads which pair is by the
//! base pair each reader addresses (found with captures/ghidra/xrefs.py over 0x1612D4.. for WAD3 and 0x165590.. for WAD8: the
//! readers add a runtime index to that base, see the notes on each line).
//!
//! WAD3: pair 183 + language (183, 185-188; 184 is empty) credits text tables (func_L01_003FC8C0); 189-201 the credits
//! background frames, RGBA 512 x 416 (func_L01_003FC8C0, drawn by func_L01_003FD2F0); 317-328 sets of LZ streams that each
//! unpack to such a frame (no code found reads these pairs); 329-337 and 349-380 are 2FIP images (wads.rs).
//!
//! WAD8: 0-73, 400-409, 783-784 raw images (wads.rs); 74-101 and 122 level map blocks (no reader found); 655-683 and 715-743
//! level maps (func_L01_0042AD60, level 653/713 + number); 774-781 the global text table per language (func_L01_0049F628); 782
//! the front end's BE_H object bank (frontbin func_003A9B10); 1364 an RGBA frame (func_L01_00491890's base pair 1354 + 10);
//! every other non-empty pair is a raw 2FIP image or an LZ stream that unpacks to one.

use crate::disc::{Disc, SECTOR, u32_at};
use crate::rd::Fields;
use crate::track::{NOTHING, Node, Space, Tracker};

const LZ_GAME: &str = "func_L01_00433EA0 (each program's copy)";

/// The pair `p` of a WAD header as the TOC holds it: (sector, sectors).
fn pair(header: &[u8], p: usize) -> Result<(u64, u64), String> {
    Ok((u32_at(header, 8 + 8 * p)? as u64, u32_at(header, 12 + 8 * p)? as u64))
}

/// An entry of a kind no decoder knows: its bytes on a line of their own, failed.
fn unrecognised(t: &mut Tracker, root: &mut Node, g: &str, at: u64, size: u64, why: String) {
    let l = t.line(g, "entry of a kind no decoder knows", "", "", Space::Disc);
    let mut n = t.child(root, l, at, size);
    n.fail(why);
    t.close(n);
}

// ---------------------------------------------------------------------------------------------------------------------------
// Text tables

const CREDITS_GAME: &str = "func_L01_003FC8C0 (loads pair 183 + the language byte 0x143A07 into 0x1E6548), func_L01_003FE190 (relocates the offsets; the count is the first offset / 16), func_L01_003FE150 (finds a record by its id at +4 and returns its text)";
const TEXT_GAME: &str = "func_L01_0049F628 (loads pair 774 + the language byte 0x143A07 and relocates the offsets; the count is the first offset / 16), func_L01_00418130 and func_L01_004181A0 (find a record by its id at +4 and return its text), +8 (u32): func_L01_00418478 (compares it with a value), +0xC (s16): func_L01_004181E0 and func_L01_00418478 (a count tested against zero), +0xE (s16): func_L01_00418F00, func_L01_0049EB98 and func_L01_0049F068";

/// A text table: 16-byte records (u32 text offset, u32 text id, u32 +8, s16 +0xC, s16 +0xE), the first offset the table's size;
/// then the NUL-terminated texts; then zeros to the end of the entry. `credits`: +8..+0x10 are the builder's 0xCC fill (six
/// bytes) and two zeros (func_L01_003FE150 reads only the id and the text).
fn text_table(t: &mut Tracker, root: &mut Node, g: &str, at: u64, b: &[u8], credits: bool) {
    let (name, game) = if credits { ("credits text table (records, texts)", CREDITS_GAME) } else { ("text table (records, texts)", TEXT_GAME) };
    let l = t.line(g, name, "wad38::text_table", game, Space::Disc);
    let mut n = t.child(root, l, at, b.len() as u64);
    if let Err(e) = text_decode(&mut n, b, credits) {
        n.fail(e);
    }
    t.close(n);
}

fn text_decode(n: &mut Node, b: &[u8], credits: bool) -> Result<usize, String> {
    let first = u32_at(b, 0)? as usize;
    if first == 0 || first % 16 != 0 || first > b.len() {
        return Err(format!("first text offset {first:#x}: not a whole number of 16-byte records inside the entry"));
    }
    let count = first / 16;
    let mut f = Fields::new(b, n);
    let mut texts: Vec<usize> = Vec::new();
    for j in 0..count {
        let o = f.u32(16 * j)? as usize;
        f.u32(16 * j + 4)?;
        if credits {
            f.same(16 * j + 8, &[0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0, 0], "record filler")?;
        } else {
            f.u32(16 * j + 8)?;
            f.mark(16 * j + 12, 2);
            f.mark(16 * j + 14, 2);
        }
        // An offset of 0 is a text the language lacks (the Japanese table's last 172 records).
        if o != 0 {
            if o < first || o >= b.len() || texts.last().is_some_and(|&p| o <= p) {
                return Err(format!("record {j}: text offset {o:#x} not ascending inside {first:#x}..{:#x}", b.len()));
            }
            if let Some(&p) = texts.last() {
                if b[o - 1] != 0 {
                    return Err(format!("record {j}: the text before {o:#x} (from {p:#x}) is not NUL-terminated"));
                }
            }
            texts.push(o);
        }
    }
    if texts.first() != Some(&first) {
        return Err(format!("the first text is at {:?}, not at the table's end {first:#x}", texts.first()));
    }
    let last = *texts.last().unwrap();
    let z = b[last..].iter().position(|&c| c == 0).ok_or_else(|| format!("the last text at {last:#x} has no NUL"))?;
    let end = last + z + 1;
    f.mark(first, end - first);
    f.fill(end, b.len() - end, 0, "after the last text")?;
    Ok(count)
}

// ---------------------------------------------------------------------------------------------------------------------------
// RGBA frames

const FRAME_BYTES: usize = 512 * 416 * 4;

/// A frame (u32 512, u32 416, two zero words, then 512 x 416 RGBA pixels, alpha 0..0x80) at `at` in `parent`. `b` holds at
/// least the frame; only its own bytes are placed. The 16 header bytes are not read by any code: func_L01_003FD2F0 hands the
/// buffer + 0x10 and the constants 0x200 and 0x1A0 to func_L01_004C25F8, so the header line checks the builder's values and
/// leaves its bytes unread.
fn frame(t: &mut Tracker, parent: &mut Node, space: Space, g: &str, at: u64, b: &[u8], px_game: &'static str) {
    let lh = t.line(g, "RGBA frame header (16 bytes before the pixels: +0 width 512, +4 height 416, +8 and +0xC zero)", "wad38::frame", "no reader: func_L01_003FD2F0 hands the buffer + 0x10 and the constants 0x200 x 0x1A0 to func_L01_004C25F8", space);
    let lp = t.line(g, "RGBA frame pixels (512 x 416, R, G, B, alpha 0..0x80)", "wad38::frame", px_game, space);
    let mut h = t.child(parent, lh, at, 16.min(b.len() as u64));
    if b.len() < 16 || u32_at(b, 0) != Ok(512) || u32_at(b, 4) != Ok(416) || b[8..16].iter().any(|&v| v != 0) {
        h.fail(format!("frame header {:02x?}: not 512, 416 and two zero words", &b[..16.min(b.len())]));
    }
    h.skip(0, 16.min(b.len() as u64));
    t.close(h);
    if b.len() <= 16 {
        return;
    }
    let len = (b.len() - 16).min(FRAME_BYTES);
    let mut p = t.child(parent, lp, at + 16, len as u64);
    if len < FRAME_BYTES {
        p.fail(format!("{len:#x} pixel bytes, the frame needs {FRAME_BYTES:#x}"));
    } else if let Some(k) = (0..512 * 416).find(|&k| b[16 + 4 * k + 3] > 0x80) {
        p.fail(format!("pixel {k}: alpha {:#04x}", b[16 + 4 * k + 3]));
    } else {
        p.mark(0, FRAME_BYTES as u64);
    }
    t.close(p);
}

// ---------------------------------------------------------------------------------------------------------------------------
// WAD3 frame stream sets

/// The header of a stream set: u32 offsets of the LZ streams (the first is the header's own size), 0xFFFFFFFF for an unused
/// slot, zeros at the end. Returns the offsets in use.
fn stream_set(t: &mut Tracker, root: &mut Node, g: &str, at: u64, b: &[u8]) -> Result<(), String> {
    let l = t.ignored_line(g, "frame stream set (a header of u32 stream offsets, then LZ streams that each unpack to an RGBA frame)", "searched: no code addresses the WAD3 pair-table entries of pairs 317-328 (0x161CC0..0x161D20) in any overlay or front end", Space::Disc);
    let n = t.child(root, l, at, b.len() as u64);
    t.close(n);
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------------
// 2FIP images unpacked from LZ streams (pif.rs's decoder in unpacked space; see pif.rs for what each field is)

const LEVEL1_WHY: &str = "func_L01_0042A598 starts level 2 at 5/4 of level 0's size, so it steps over level 1 without reading or uploading it; the other loaders (frontbin func_00395EB8, func_00393E90, func_00393A18, front end func_003824F0) take the palette and level 0 only";
const LEVEL3_WHY: &str = "func_L01_0042A598 uploads levels 0 and 2 only (the level count is clamped to 3); no loader addresses a level 3";
const IMG_HEADER: u64 = 0x20;
const IMG_PALETTE: u64 = 0x400;

/// One 2FIP image at `at` in `parent` (unpacked space); `b` starts at the image. Returns the image's length.
pub(crate) fn image_unpacked(t: &mut Tracker, parent: &mut Node, g: &str, at: u64, b: &[u8], game: &'static str) -> Result<u64, String> {
    let lh = t.line(
        g,
        "2FIP header (+0 tag, +4 no reader (skipped), +8 width, +0xC height, +0x10 0x13, +0x14 palette format 0, +0x18 zero, +0x1C mip levels)",
        "wad38::image_unpacked",
        game,
        Space::Unpacked,
    );
    if b.len() < IMG_HEADER as usize {
        let mut h = t.child(parent, lh, at, b.len() as u64);
        h.fail(format!("{:#x} bytes, the header needs {IMG_HEADER:#x}", b.len()));
        t.close(h);
        return Err("short image".to_string());
    }
    let mut h = t.child(parent, lh, at, IMG_HEADER);
    let head = (|| -> Result<(u32, u32, u32), String> {
        let mut f = Fields::new(&b[..IMG_HEADER as usize], &mut h);
        f.same(0, b"2FIP", "tag")?;
        f.n.skip(4, 4);
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
    })();
    let (w, ht, levels) = match head {
        Ok(v) => v,
        Err(e) => {
            let place = h.place.clone();
            h.fail(e.clone());
            t.close(h);
            return Err(format!("{place}: {e}"));
        }
    };
    t.close(h);
    let total: u64 = IMG_HEADER + IMG_PALETTE + (0..levels).map(|k| ((w >> k) * (ht >> k)) as u64).sum::<u64>();
    if total > b.len() as u64 {
        return Err(format!("image of {total:#x} bytes in {:#x}", b.len()));
    }
    let lp = t.line(g, "palette (256 colours of R, G, B, alpha 0..0x80)", "wad38::image_unpacked", game, Space::Unpacked);
    let mut p = t.child(parent, lp, at + IMG_HEADER, IMG_PALETTE);
    if let Some(k) = (0..256).find(|&k| b[IMG_HEADER as usize + 4 * k + 3] > 0x80) {
        p.fail(format!("palette colour {k}: alpha {:#04x}", b[IMG_HEADER as usize + 4 * k + 3]));
    } else {
        p.mark(0, IMG_PALETTE);
    }
    t.close(p);
    let mut off = IMG_HEADER + IMG_PALETTE;
    for k in 0..levels {
        let size = ((w >> k) * (ht >> k)) as u64;
        let l = match k {
            0 => t.line(g, "pixels, level 0 (width x height palette indices)", "wad38::image_unpacked", game, Space::Unpacked),
            1 => t.ignored_line(g, "pixels, level 1 (half size; never uploaded)", LEVEL1_WHY, Space::Unpacked),
            2 => t.line(g, "pixels, level 2 (quarter size)", "wad38::image_unpacked", game, Space::Unpacked),
            _ => t.ignored_line(g, "pixels, level 3 (eighth size; never uploaded)", LEVEL3_WHY, Space::Unpacked),
        };
        let mut n = t.child(parent, l, at + off, size);
        if k == 0 || k == 2 {
            n.mark(0, size);
        }
        t.close(n);
        off += size;
    }
    Ok(total)
}

const PACKED_IMAGE_GAME: &str = "func_L01_00433EA0 unpacks the stream; the reader of its pair base loads it through the descriptor of func_L01_00490B98: pair 410 func_L01_00496AB0, 469 and 868 func_L01_004832F8, 511 func_L01_0048A848, 878 func_L01_00481298, 979 func_L01_00488C28, 996 func_L01_004910F0, 1052 func_L01_004BE9A0 (func_L01_004BFD68, func_L01_004C07D8, func_L01_004C1440), 1122 and 1124 func_L01_004879F8, 1123 func_L01_004849E8, 1146 func_L01_0049A930, 1197 func_L01_0048E5D8, 1206 func_L01_0048F7E8, 1211 func_L01_0048D030 (frontbin func_003B0580), 1220 func_L01_00499898, 1248 func_L01_00498868, 1251 func_L01_0049E448, 1253 func_L01_004879F8 / func_L01_0048A848 / func_L01_0048B650 (frontbin func_003B22C0)";
const RAW_IMAGE_GAME: &str = "the reader of the pair's base loads it raw through the descriptor of func_L01_00490B98: pair 996 func_L01_004910F0, 1052 func_L01_004BE9A0 (func_L01_004BFD68, func_L01_004C07D8, func_L01_004C1440), 1313 func_L01_00492BD0, 1354 func_L01_00491890";

// ---------------------------------------------------------------------------------------------------------------------------
// WAD8 level maps (pairs 655-683 and 715-743: pair 654 + n, or 714 + n when byte 0x142CA5 is set, for the map index n 1-29 held
// at 0x1F4430; func_L01_0042AD60 reads the pair's table entry at 0x166A08 + n * 8 (sector, sectors) or 0x166BE8 + n * 8)

const MAP_G: &str = "WAD8 unpacked level maps";
const MAP_TABLE_GAME: &str = "func_L01_0042AD60 (words 0-2: copies [word 0, word 1) as part 0 and [word 1, word 2) as part 1), func_L01_0048BC68 (words 0-6: the pointers of parts 0-6 into the unpacked record at 0x1F4448; part 2 starts at word 2, part 3 at word 3, parts 4-6 at words 4-6)";
const MAP_PLANE_GAME: &str = "func_L01_0042B8F0 (copies one plane's pixel or the other's into part 3, by a 512 x 512 bit mask that func_L01_0042D020 or func_L01_0042DBF0 expands from part 0 or part 1)";
const MAP_IMAGE: usize = 0x4420;

fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// The unpacked record: eight u32 (the starts of parts 0-6, then the record's size), then the parts. Parts 0 and 1 are the two
/// forms of the 512 x 512 bit mask that tells which of the planes in parts 2 and 3 shows at each pixel; parts 2 and 3 are those
/// planes (512 x 512 palette indices); parts 4-6 are 128 x 128 images (in two records only).
fn level_map(t: &mut Tracker, root: &mut Node, d: &[u8]) {
    let lw = t.line(MAP_G, "part table words 0-6 (u32 starts of parts 0-6; word 0 = 0x20)", "wad38::level_map", MAP_TABLE_GAME, Space::Unpacked);
    let l7 = t.line(MAP_G, "part table word 7 (the record's unpacked size)", "wad38::level_map", "no reader in func_L01_0042AD60, func_L01_0048BC68 or the other users of 0x1F4448", Space::Unpacked);
    if d.len() < 0x20 {
        let mut n = t.child(root, lw, 0, d.len() as u64);
        n.fail(format!("{:#x} bytes, the part table needs 0x20", d.len()));
        t.close(n);
        return;
    }
    let w: Vec<usize> = (0..8).map(|k| le32(d, 4 * k) as usize).collect();
    let bad = if w[0] != 0x20 {
        Some(format!("part 0 starts at {:#x}, not 0x20", w[0]))
    } else if w.windows(2).any(|p| p[0] > p[1]) || w.iter().any(|&v| v % 16 != 0) || w[0] >= w[1] || w[1] >= w[2] {
        Some(format!("part starts {w:x?} not ascending 16-byte aligned"))
    } else if w[7] != d.len() {
        Some(format!("the last word is {:#x}, the record is {:#x} bytes", w[7], d.len()))
    } else if w[3] - w[2] != 0x48000 || w[4] - w[3] != 0x40000 {
        Some(format!("planes of {:#x} and {:#x} bytes, not 0x48000 and 0x40000", w[3] - w[2], w[4] - w[3]))
    } else if !(w[4] == w[5] && w[5] == w[6] && w[6] == w[7]) && !(w[5] - w[4] == MAP_IMAGE && w[6] - w[5] == MAP_IMAGE && w[7] - w[6] == MAP_IMAGE) {
        Some(format!("images {:x?}: neither none nor three of 0x4420 bytes", &w[4..]))
    } else {
        None
    };
    let mut a = t.child(root, lw, 0, 28);
    let mut b = t.child(root, l7, 28, 4);
    if let Some(e) = bad {
        a.fail(e);
        t.close(a);
        t.close(b);
        return;
    }
    a.mark(0, 28);
    b.skip(0, 4);
    t.close(a);
    t.close(b);
    map_part0(t, root, w[0], &d[w[0]..w[1]]);
    map_part1(t, root, w[1], &d[w[1]..w[2]]);
    let l2 = t.line(MAP_G, "part 2 plane A (512 x 512 palette indices)", "wad38::level_map", MAP_PLANE_GAME, Space::Unpacked);
    let lt = t.line(MAP_G, "part 2 padding after the plane (0x8000 zero bytes)", "wad38::level_map", NOTHING, Space::Unpacked);
    let l3 = t.line(
        MAP_G,
        "part 3 plane B (512 x 512 palette indices)",
        "wad38::level_map",
        "func_L01_0042B8F0 (the merge target, written in place), func_L01_0048BC68 (queues it as a 512 x 512 texture through func_L01_0042ABB0), func_L01_00433A28 (sends the merged plane's 0x40000 bytes to the GS, and its first 0x400 bytes as the texture's palette)",
        Space::Unpacked,
    );
    let mut n = t.child(root, l2, w[2] as u64, 0x40000);
    n.mark(0, 0x40000);
    t.close(n);
    let mut n = t.child(root, lt, (w[2] + 0x40000) as u64, 0x8000);
    if let Err(e) = Fields::new(&d[w[2] + 0x40000..w[3]], &mut n).fill(0, 0x8000, 0, "padding after plane A") {
        n.fail(e);
    }
    t.close(n);
    let mut n = t.child(root, l3, w[3] as u64, 0x40000);
    n.mark(0, 0x40000);
    t.close(n);
    if w[5] != w[4] {
        map_images(t, root, &w, d);
    }
}

fn pad_zero(t: &mut Tracker, root: &mut Node, line: usize, at: usize, b: &[u8], used: usize) {
    if used == b.len() {
        return;
    }
    let mut n = t.child(root, line, (at + used) as u64, (b.len() - used) as u64);
    if b.len() - used >= 16 {
        n.fail(format!("{} padding bytes", b.len() - used));
    } else if let Err(e) = Fields::new(&b[used..], &mut n).fill(0, b.len() - used, 0, "padding to 16 bytes") {
        n.fail(e);
    }
    t.close(n);
}

/// Part 0: u32 0x100, u32 0x20 (unread), 256 s16 tile ends counted from byte 8, the tiles' run-length records (a 32 x 32 tile of
/// 4-bit pixels each), zero padding to 16 bytes.
fn map_part0(t: &mut Tracker, root: &mut Node, at: usize, b: &[u8]) {
    let lh = t.line(MAP_G, "part 0 header (u32 0x100, u32 0x20)", "wad38::map_part0", "no reader: func_L01_0042AD60 skips these 8 bytes, func_L01_0042D020 counts 256 tiles of 32 x 32 itself", Space::Unpacked);
    let le = t.line(MAP_G, "part 0 tile end offsets (256 s16 counted from byte 8 of the part)", "wad38::map_part0", "func_L01_0042D170 (entry k is the end of tile k, entry k - 1 the start; tile 0 starts at 0x200)", Space::Unpacked);
    let lr = t.line(
        MAP_G,
        "part 0 tile runs (per tile 3-nibble records: pixel value, run length low and high nibble, 0 meaning 256; 1024 pixels a tile)",
        "wad38::map_part0",
        "func_L01_0042D170 (one tile; func_L01_0042D020 calls it for the 256 tiles and sets a mask bit for each nonzero pixel; reached from func_L01_0042AD60 and func_L01_0048BC68 when the map's flag is clear)",
        Space::Unpacked,
    );
    let lp = t.line(MAP_G, "part 0 padding to 16 bytes (zero)", "wad38::map_part0", NOTHING, Space::Unpacked);
    if b.len() < 0x208 {
        let mut n = t.child(root, lh, at as u64, b.len() as u64);
        n.fail(format!("part 0 is {:#x} bytes, the header and tile table need 0x208", b.len()));
        t.close(n);
        return;
    }
    let mut h = t.child(root, lh, at as u64, 8);
    if le32(b, 0) != 0x100 || le32(b, 4) != 0x20 {
        h.fail(format!("part 0 header {:#x}, {:#x}: not 0x100, 0x20", le32(b, 0), le32(b, 4)));
    }
    h.skip(0, 8);
    t.close(h);
    let base = &b[8..];
    let mut ends = [0usize; 256];
    let res = (|| -> Result<(), String> {
        let mut start = 0x200usize;
        for k in 0..256 {
            let e = i16::from_le_bytes([base[2 * k], base[2 * k + 1]]);
            if e < 0 || e as usize <= start || 8 + e as usize > b.len() {
                return Err(format!("tile {k} ends at {e:#x}, it starts at {start:#x}, the part is {:#x} bytes", b.len()));
            }
            ends[k] = e as usize;
            start = e as usize;
        }
        Ok(())
    })();
    let mut en = t.child(root, le, (at + 8) as u64, 0x200);
    match &res {
        Ok(()) => en.mark(0, 0x200),
        Err(e) => en.fail(e.clone()),
    }
    t.close(en);
    if res.is_err() {
        return;
    }
    let last = ends[255];
    let mut rn = t.child(root, lr, (at + 8 + 0x200) as u64, (last - 0x200) as u64);
    let tiles = (|| -> Result<(), String> {
        let mut start = 0x200usize;
        for (k, &e) in ends.iter().enumerate() {
            let nibbles = 2 * (e - start);
            let triples = nibbles / 3;
            let nib = |p: usize| -> usize {
                let v = base[start + p / 2];
                (if p % 2 == 0 { v & 15 } else { v >> 4 }) as usize
            };
            let left = nibbles - 3 * triples;
            if left == 2 || (left == 1 && nib(nibbles - 1) != 0) {
                return Err(format!("tile {k}: {left} nibbles after the last record, the last is {:#x}", nib(nibbles - 1)));
            }
            let pixels: usize = (0..triples).map(|j| nib(3 * j + 1) | nib(3 * j + 2) << 4).map(|r| if r == 0 { 256 } else { r }).sum();
            if pixels != 1024 {
                return Err(format!("tile {k}: runs add up to {pixels} pixels, not 1024"));
            }
            start = e;
        }
        Ok(())
    })();
    match tiles {
        Ok(()) => rn.mark(0, (last - 0x200) as u64),
        Err(e) => rn.fail(e),
    }
    t.close(rn);
    pad_zero(t, root, lp, at, b, 8 + last);
}

/// Part 1: (u8 skip, u8 count) pairs that advance the 512 x 512 mask position by skip + count until it reaches 0x40000, then
/// zero padding to 16 bytes.
fn map_part1(t: &mut Tracker, root: &mut Node, at: usize, b: &[u8]) {
    let lr = t.line(
        MAP_G,
        "part 1 mask runs (u8 skip, u8 count pairs; skips and counts add up to 0x40000 mask positions)",
        "wad38::map_part1",
        "func_L01_0042DC68 (reached through func_L01_0042DBF0 from func_L01_0042AD60 and func_L01_0048BC68 when the map's flag is set and bit 0 of its 0x800-byte progress block at 0x1A9318 + n * 0x800 is set; func_L01_0042AD60 copies the part to the buffer it passes)",
        Space::Unpacked,
    );
    let lp = t.line(MAP_G, "part 1 padding to 16 bytes (zero)", "wad38::map_part1", NOTHING, Space::Unpacked);
    let mut pos = 0usize;
    let mut used = 0usize;
    while pos < 0x40000 && used + 2 <= b.len() {
        pos += b[used] as usize + b[used + 1] as usize;
        used += 2;
    }
    let mut n = t.child(root, lr, at as u64, used as u64);
    if pos != 0x40000 {
        n.fail(format!("the pairs run to {pos:#x} mask positions in {used:#x} bytes, not to 0x40000"));
    } else {
        n.mark(0, used as u64);
    }
    t.close(n);
    pad_zero(t, root, lp, at, b, used);
}

/// Parts 4-6: three 128 x 128 images (0x20 header, 0x400 palette, 0x4000 pixels), the three textures of one map that share the
/// first image's palette.
fn map_images(t: &mut Tracker, root: &mut Node, w: &[usize], d: &[u8]) {
    let lh = t.line(MAP_G, "map image header (0x20 bytes: 2FIP, 128 x 128, format 0x13, one level)", "wad38::map_images", "no reader: func_L01_0048BC68 steps over it (palette at +0x20, pixels at +0x420)", Space::Unpacked);
    let lp = t.line(
        MAP_G,
        "map image palette of part 4 (256 colours of R, G, B, alpha 0..0x80)",
        "wad38::map_images",
        "func_L01_0048BC68 (passes word 4's part + 0x20 as the palette of all three textures to func_L01_0042ABB0), func_L01_00433A28 (sends its 0x40 quadwords to the GS)",
        Space::Unpacked,
    );
    let lq = t.line(MAP_G, "map image palettes of parts 5 and 6 (each equals part 4's)", "wad38::map_images", "no reader: func_L01_0048BC68 gives every texture part 4's palette", Space::Unpacked);
    let lx = t.line(
        MAP_G,
        "map image pixels (128 x 128 palette indices)",
        "wad38::map_images",
        "func_L01_0048BC68 (passes the part + 0x420 to func_L01_0042ABB0 as a 128 x 128 texture), func_L01_00433A28 (sends its 0x400 quadwords to the GS)",
        Space::Unpacked,
    );
    for k in 4..7usize {
        let (s, img) = (w[k], &d[w[k]..w[k] + MAP_IMAGE]);
        let mut h = t.child(root, lh, s as u64, 0x20);
        let ok = &img[..4] == b"2FIP" && le32(img, 8) == 128 && le32(img, 0xc) == 128 && le32(img, 0x10) == 0x13 && le32(img, 0x14) == 0 && le32(img, 0x18) == 0 && le32(img, 0x1c) == 1;
        if !ok {
            h.fail(format!("image header {:02x?}", &img[..0x20]));
        }
        h.skip(0, 0x20);
        t.close(h);
        let mut p = t.child(root, if k == 4 { lp } else { lq }, (s + 0x20) as u64, 0x400);
        if k == 4 {
            if let Some(c) = (0..256).find(|&c| img[0x20 + 4 * c + 3] > 0x80) {
                p.fail(format!("palette colour {c}: alpha {:#04x}", img[0x20 + 4 * c + 3]));
            } else {
                p.mark(0, 0x400);
            }
        } else if img[0x20..0x420] != d[w[4] + 0x20..w[4] + 0x420] {
            p.fail("the palette differs from part 4's".to_string());
        } else {
            p.skip(0, 0x400);
        }
        t.close(p);
        let mut x = t.child(root, lx, (s + 0x420) as u64, 0x4000);
        x.mark(0, 0x4000);
        t.close(x);
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// WAD8 level map blocks (pairs 74-101 and 122). No reader was found (see the module notes); the structure below is what the data
// settles, and every byte stays unread.

const BLOCK_G: &str = "WAD8 unpacked level map blocks";

const BLOCK_WHY: &str = "searched: no pair-table address 0x1657E8..0x165968 (pairs 74-101, 122) in any of the 34 story overlays, the multiplayer overlays or the two front ends (lui/addiu chains, immediates, data words), the sectors absent from the Veldin RAM image, no descriptor index leading there";

/// A level map block (pairs 74-101) or a set of ten (pair 122): nothing in the game reads them.
fn map_block(t: &mut Tracker, root: &mut Node, at: u64, b: &[u8]) {
    let l = t.ignored_line(BLOCK_G, "level map block (four f32, a u32 offset table, records of 8-byte entries)", BLOCK_WHY, Space::Unpacked);
    let n = t.child(root, l, at, b.len() as u64);
    t.close(n);
}

fn map_block_set(t: &mut Tracker, root: &mut Node, d: &[u8]) {
    let l = t.ignored_line(BLOCK_G, "level map block set (a 0x40-byte table of ten block starts, then ten blocks)", BLOCK_WHY, Space::Unpacked);
    let n = t.child(root, l, 0, d.len() as u64);
    t.close(n);
}

// ---------------------------------------------------------------------------------------------------------------------------
// WAD8 pair 782: the front end's BE_H object bank

/// "BE_HW3D\0", u32 3, 0x1B, 0x7D4, 0x2A, u32 count at +0x18 (146), then (u32 id, u32 object offset) records from +0x1C, a
/// 20-byte trailer (nine 0xFF, three zero, "BE_HW3D\0"), then the objects, 16-byte aligned, "EN_HW3D\0" and zeros to the
/// sector's end.
///
/// An object (frontbin func_003A4768 relocates it; func_003A4E70 draws it, func_003A4A78 measures it, func_003A4860 poses it):
/// a 0x30-byte header, the vertices (16 bytes each when +0x24 is zero, else 4), the index list, the texture entries and their
/// image data (palette 0x400 and 8-bit pixels), and the pose list (+8 poses of +0xA bytes: each joint 0x18 bytes).
fn be_h(t: &mut Tracker, root: &mut Node, g: &str, at: u64, b: &[u8]) {
    const GAME_E: &str = "frontbin func_003A6830 (finds an object by its id: compares the id of each entry from +0x1C with the wanted one and returns the entry's object), frontbin func_003A6790 (reads the count at +0x18 and relocates each entry's offset)";
    const GAME: &str = "frontbin func_003A9B10 (loads the pair into the menu object at +0x1C24 through func_0039D668), frontbin func_003A6790 (reads the count at +0x18 and relocates each record's offset); +8..+0x17: no reader in func_003A6790, func_003A6830 or func_003A9B10 (the only code that addresses the bank)";
    const OBJ: &str = "frontbin func_003A4768 (relocates +0x10, +0x14, +0x18, +0x1C and +0x20, scales the float at +0xC, reads the count at +6), func_003A4E70 (+0, +2, +4 gate, +6, +0x10, +0x14, +0x18, +0x1C, +0x24 and +0x28), func_003A4A78 (+0, +0xC, +0x24), func_003A4860 (+4, +8, +0xA, +0x20); +0x2C: zero in every object, no reader";
    let lh = t.line(g, "BE_H header (+0 tag, +0x18 entry count; +8..+0x17 no reader (skipped))", "wad38::be_h", GAME, Space::Disc);
    let le = t.line(g, "BE_H entry table (u32 id, u32 object offset)", "wad38::be_h", GAME_E, Space::Disc);
    let lt = t.ignored_line(g, "BE_H entry table trailer (nine 0xFF, three zero, BE_HW3D)", "searched: frontbin func_003A6790 and func_003A6830 walk the entries by the count at +0x18 and never address the 20 bytes after them (the only code that addresses the bank, with func_003A9B10 and func_003A4768, which start at an object's own offset)", Space::Disc);
    let lo = t.line(g, "BE_H object header (+0 vertex count, +2 index count, +4 joint count, +6 texture count, +8 pose count, +0xA pose size, +0xC scale, +0x10..+0x20 section offsets, +0x24 vertex form, +0x28 vertex colour)", "wad38::be_h", OBJ, Space::Disc);
    let lv = t.line(g, "BE_H vertices (16 bytes: 8 bytes of position and joint, 8 bytes of texture data; or 4 packed bytes when +0x24 is not zero)", "wad38::be_h", "frontbin func_003A4A78 (positions and the joint: +6 of a 16-byte vertex, the top 4 bits of a 4-byte one) and func_0039B4E8 / func_0039B628 (the vertex packets, VU)", Space::Disc);
    let lk = t.line(g, "BE_H index list marker (u32 0xFFFFFFFF before the indices of an object without textures)", "wad38::be_h", "frontbin func_003A4E70 starts the list at +4 when the object has no textures: the word is not read", Space::Disc);
    let lq = t.line(g, "BE_H index quad (four vertex indices: a triangle strip of two)", "wad38::be_h", "frontbin func_003A4E70 (copies the vertices at 0x70000000 + index * 0x20 into the draw packet)", Space::Disc);
    let lc = t.line(g, "BE_H texture switch (u8 texture number, three 0xFF)", "wad38::be_h", "frontbin func_003A4E70 (an entry whose bytes 1-3 are 0xFF selects the texture entry)", Space::Disc);
    let lx = t.line(g, "BE_H texture entry (+0 palette offset, +4 pixel offset, +8 width, +0xC height)", "wad38::be_h", "frontbin func_003A4768 (shifts +0 and +4 right by 4 into +0xA and +8, takes the log2 of +8 and +0xC), func_003A4E70 (addresses the palette and pixels in the image data)", Space::Disc);
    let lp = t.line(g, "BE_H texture palette (256 colours, RGBA)", "wad38::be_h", "frontbin func_003A4E70 (sends it to the GS: the source address is +0x1C + the entry's +0 offset)", Space::Disc);
    let ls = t.line(g, "BE_H texture pixels (width x height palette indices)", "wad38::be_h", "frontbin func_003A4E70 (sends them to the GS: the source address is +0x1C + the entry's +4 offset)", Space::Disc);
    let lj = t.line(g, "BE_H pose joint (0x18 bytes: +0..+0x15 the joint's transform, +0x16 s16 parent joint, -1 for none)", "wad38::be_h", "frontbin func_003A4860 (+0x16 of the pose's joint; the 0x16 bytes before it go to func_003892D8 and func_00389330, which build and blend the matrix)", Space::Disc);
    let lz = t.line(g, "BE_H alignment padding (zero)", "wad38::be_h", NOTHING, Space::Disc);
    let le2 = t.ignored_line(g, "BE_H end tag (EN_HW3D)", "searched: no code compares or addresses the tag after the last object: the objects are reached by their entries' offsets and nothing derives a length from the bank's end", Space::Disc);
    let res = (|| -> Result<(usize, Vec<usize>, usize, usize), String> {
        if b.len() < 0x1c {
            return Err(format!("{:#x} bytes", b.len()));
        }
        let count = u32_at(b, 0x18)? as usize;
        let table_end = 0x1c + 8 * count;
        if table_end + 20 > b.len() {
            return Err(format!("{count} entries run past the end"));
        }
        let offs: Vec<usize> = (0..count).map(|j| u32_at(b, 0x20 + 8 * j).map(|v| v as usize)).collect::<Result<_, _>>()?;
        let mut z = b.len();
        while z > 0 && b[z - 1] == 0 {
            z -= 1;
        }
        // The end tag "EN_HW3D\0": its last byte is the zero the trimming took.
        if z < 7 || &b[z - 7..z] != b"EN_HW3D" || z + 1 > b.len() {
            return Err("no EN_HW3D end tag after the objects".into());
        }
        let end = z - 7;
        if offs.first() != Some(&(table_end + 20)) || offs.windows(2).any(|p| p[0] >= p[1] || p[0] % 16 != 0) || offs.last().is_some_and(|&o| o >= end) || end % 16 != 0 {
            return Err(format!("object offsets {:x?}... not ascending 16-byte aligned from {:#x} to {end:#x}", &offs[..offs.len().min(4)], table_end + 20));
        }
        Ok((count, offs, end, z + 1))
    })();
    let (count, offs, end, z) = match res {
        Ok(v) => v,
        Err(e) => {
            let mut n = t.child(root, lh, at, b.len() as u64);
            n.fail(e);
            t.close(n);
            return;
        }
    };
    let mut h = t.child(root, lh, at, 0x1c);
    {
        let mut f = Fields::new(&b[..0x1c], &mut h);
        if let Err(e) = f.same(0, b"BE_HW3D\0", "tag") {
            h.fail(e);
        } else {
            h.mark(0x18, 4);
            h.skip(8, 0x10);
        }
    }
    t.close(h);
    let mut e = t.child(root, le, at + 0x1c, 8 * count as u64);
    for j in 0..count {
        e.mark(8 * j as u64, 8);
    }
    t.close(e);
    let trailer = &b[0x1c + 8 * count..0x1c + 8 * count + 20];
    let mut tr = t.child(root, lt, at + 0x1c + 8 * count as u64, 20);
    if trailer[..9].iter().any(|&v| v != 0xff) || trailer[9..12] != [0, 0, 0] || &trailer[12..] != b"BE_HW3D\0" {
        tr.fail(format!("trailer {trailer:02x?}"));
    }
    t.close(tr);
    for k in 0..count {
        let (o, x) = (offs[k], offs.get(k + 1).copied().unwrap_or(end));
        let obj = &b[o..x];
        match be_h_plan(obj) {
            Ok(plan) => be_h_emit(t, root, at + o as u64, obj, &plan, [lo, lv, lk, lq, lc, lx, lp, ls, lj, lz]),
            Err(e) => {
                let mut n = t.child(root, lo, at + o as u64, (x - o) as u64);
                n.fail(format!("object {k}: {e}"));
                t.close(n);
            }
        }
    }
    let n = t.child(root, le2, at + end as u64, 8);
    t.close(n);
    if z < b.len() {
        let mut n = t.child(root, lz, at + z as u64, (b.len() - z) as u64);
        n.mark(0, (b.len() - z) as u64);
        t.close(n);
    }
}

struct ObjPlan {
    nv: usize,
    nq: usize,
    j: usize,
    nt: usize,
    pj: usize,
    po: usize,
    vs: usize,
    p10: usize,
    p14: usize,
    p18: usize,
    p1c: usize,
    p20: usize,
    /// The index list's end and where the next section starts.
    qend: usize,
    next: usize,
    /// (offset, is a switch) of each list entry after the marker.
    list: Vec<(usize, bool)>,
}

/// The sections of a BE_H object from its header, every offset and count checked against the object's bytes.
fn be_h_plan(obj: &[u8]) -> Result<ObjPlan, String> {
    let sz = obj.len();
    if sz < 0x30 {
        return Err(format!("{sz:#x} bytes"));
    }
    let h = |o: usize| u16::from_le_bytes([obj[o], obj[o + 1]]);
    let (nv, nq, j, nt, pj, po) = (h(0) as usize, h(2) as usize, h(4) as i16, h(6) as i16, h(8) as i16, h(10) as i16);
    if j < 0 || nt < 0 || pj < 0 || po < 0 {
        return Err(format!("joint count {j}, texture count {nt}, pose count {pj}, pose size {po}"));
    }
    let (j, nt, pj, po) = (j as usize, nt as usize, pj as usize, po as usize);
    let w = |o: usize| u32_at(obj, o).map(|v| v as usize);
    let (p10, p14, p18, p1c, p20, w24, w28, w2c) = (w(0x10)?, w(0x14)?, w(0x18)?, w(0x1c)?, w(0x20)?, w(0x24)?, w(0x28)?, w(0x2c)?);
    if p10 != 0x30 || w2c != 0 || (w24 == 0 && w28 != 0) {
        return Err(format!("+0x10 {p10:#x}, +0x24 {w24:#x}, +0x28 {w28:#x}, +0x2C {w2c:#x}"));
    }
    let vs = if w24 == 0 { 16 } else { 4 };
    if p14 != p10 + nv * vs || p14 > sz {
        return Err(format!("{nv} vertices of {vs} bytes from {p10:#x} end at {:#x}, the index list is at {p14:#x}", p10 + nv * vs));
    }
    for i in 0..nv {
        let v = &obj[p10 + vs * i..];
        let jt = if vs == 16 { i16::from_le_bytes([v[6], v[7]]) as i64 } else { (u32::from_le_bytes([v[0], v[1], v[2], v[3]]) >> 28) as i64 };
        if (vs == 16 || j > 0) && !(0..j as i64).contains(&jt) {
            return Err(format!("vertex {i}: joint {jt}, the object has {j}"));
        }
    }
    let mut list = Vec::new();
    let mut q;
    if nt == 0 {
        if u32_at(obj, p14)? != 0xffff_ffff || p18 != 0 || p1c != 0 {
            return Err(format!("no textures but marker {:#x}, +0x18 {p18:#x}, +0x1C {p1c:#x}", u32_at(obj, p14)?));
        }
        q = p14 + 4;
        for _ in 0..nq {
            list.push((q, false));
            q += 4;
        }
    } else {
        q = p14;
        let mut real = 0;
        while real < nq {
            if q + 4 > sz {
                return Err("index list runs past the object".into());
            }
            let sw = obj[q + 1..q + 4] == [0xff, 0xff, 0xff];
            if sw && obj[q] as usize >= nt {
                return Err(format!("texture switch {} of {nt}", obj[q]));
            }
            list.push((q, sw));
            q += 4;
            real += !sw as usize;
        }
    }
    if q > sz {
        return Err("index list runs past the object".into());
    }
    for &(a, sw) in &list {
        if !sw && obj[a..a + 4].iter().any(|&v| v as usize >= nv) {
            return Err(format!("index quad at {a:#x}: {:02x?} with {nv} vertices", &obj[a..a + 4]));
        }
    }
    let next = if nt > 0 { p18 } else if p20 != 0 { p20 } else { sz };
    if next < q || next - q >= 16 || next > sz || obj[q..next].iter().any(|&v| v != 0) {
        return Err(format!("the index list ends at {q:#x}, the next section is at {next:#x}"));
    }
    if nt > 0 {
        let mut pos = p1c;
        if p1c != p18 + 16 * nt {
            return Err(format!("image data at {p1c:#x}, the texture entries end at {:#x}", p18 + 16 * nt));
        }
        for tx in 0..nt {
            let (c, pix, ww, hh) = (w(p18 + 16 * tx)?, w(p18 + 16 * tx + 4)?, w(p18 + 16 * tx + 8)?, w(p18 + 16 * tx + 12)?);
            if c != 0 || pix != 0x400 || !ww.is_power_of_two() || !hh.is_power_of_two() {
                return Err(format!("texture {tx}: palette at {c:#x}, pixels at {pix:#x}, {ww} x {hh}"));
            }
            pos = p1c + pix + ww * hh;
        }
        if pos != p20 || p20 > sz {
            return Err(format!("the images end at {pos:#x}, the poses start at {p20:#x}"));
        }
    }
    if p20 != 0 {
        if po != (j * 0x18).div_ceil(16) * 16 || sz - p20 != pj * po {
            return Err(format!("{pj} poses of {po:#x} bytes for {j} joints in {:#x} bytes", sz - p20));
        }
        for f in 0..pj {
            for jt in 0..j {
                let par = i16::from_le_bytes([obj[p20 + f * po + jt * 0x18 + 0x16], obj[p20 + f * po + jt * 0x18 + 0x17]]);
                if !(-1..j as i16).contains(&par) {
                    return Err(format!("pose {f} joint {jt}: parent {par}"));
                }
            }
            if obj[p20 + f * po + j * 0x18..p20 + (f + 1) * po].iter().any(|&v| v != 0) {
                return Err(format!("pose {f}: padding after the joints is not zero"));
            }
        }
    } else if pj != 0 || j != 0 {
        return Err(format!("no pose list but {pj} poses and {j} joints"));
    }
    Ok(ObjPlan { nv, nq, j, nt, pj, po, vs, p10, p14, p18, p1c, p20, qend: q, next, list })
}

fn be_h_emit(t: &mut Tracker, root: &mut Node, at: u64, obj: &[u8], p: &ObjPlan, l: [usize; 10]) {
    let [lo, lv, lk, lq, lc, lx, lp, ls, lj, lz] = l;
    let u = |o: usize| u32::from_le_bytes([obj[o], obj[o + 1], obj[o + 2], obj[o + 3]]);
    let mut n = t.child(root, lo, at, 0x30);
    for a in [0u64, 2, 4, 6, 8, 0xa] {
        n.mark(a, 2);
    }
    for a in [0xcu64, 0x10, 0x14, 0x18, 0x1c, 0x20, 0x24, 0x28] {
        n.mark(a, 4);
    }
    n.skip(0x2c, 4);
    t.close(n);
    let mut n = t.child(root, lv, at + p.p10 as u64, (p.nv * p.vs) as u64);
    for i in 0..p.nv {
        let o = (i * p.vs) as u64;
        if p.vs == 16 {
            n.mark(o, 8);
            n.mark(o + 8, 8);
        } else {
            n.mark(o, 4);
        }
    }
    t.close(n);
    if p.nt == 0 {
        let mut n = t.child(root, lk, at + p.p14 as u64, 4);
        n.skip(0, 4);
        t.close(n);
    }
    let _ = p.nq;
    for &(a, sw) in &p.list {
        let mut n = t.child(root, if sw { lc } else { lq }, at + a as u64, 4);
        n.mark(0, if sw { 1 } else { 4 });
        if sw {
            n.mark(1, 3);
        }
        t.close(n);
    }
    if p.next > p.qend {
        let mut n = t.child(root, lz, at + p.qend as u64, (p.next - p.qend) as u64);
        n.mark(0, (p.next - p.qend) as u64);
        t.close(n);
    }
    for tx in 0..p.nt {
        let mut n = t.child(root, lx, at + (p.p18 + 16 * tx) as u64, 16);
        for a in [0u64, 4, 8, 12] {
            n.mark(a, 4);
        }
        t.close(n);
        let (pix, ww, hh) = (u(p.p18 + 16 * tx + 4) as usize, u(p.p18 + 16 * tx + 8) as usize, u(p.p18 + 16 * tx + 12) as usize);
        let mut n = t.child(root, lp, at + (p.p1c + u(p.p18 + 16 * tx) as usize) as u64, 0x400);
        n.mark(0, 0x400);
        t.close(n);
        let mut n = t.child(root, ls, at + (p.p1c + pix) as u64, (ww * hh) as u64);
        n.mark(0, (ww * hh) as u64);
        t.close(n);
    }
    for f in 0..p.pj {
        for jt in 0..p.j {
            let mut n = t.child(root, lj, at + (p.p20 + f * p.po + jt * 0x18) as u64, 0x18);
            n.mark(0, 0x16);
            n.mark(0x16, 2);
            t.close(n);
        }
        if p.po > p.j * 0x18 {
            let mut n = t.child(root, lz, at + (p.p20 + f * p.po + p.j * 0x18) as u64, (p.po - p.j * 0x18) as u64);
            n.mark(0, (p.po - p.j * 0x18) as u64);
            t.close(n);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------

/// WAD3: the credits texts and frames, and the frame stream sets (pairs 329-337 and 349-380, the images, are wads.rs's).
pub fn wad3(t: &mut Tracker, d: &mut Disc, root: &mut Node, header: &[u8], sector: u64, region: u64) -> Result<(), String> {
    let g = "WAD3";
    for p in 0..(header.len() - 8) / 8 {
        let (s, c) = pair(header, p)?;
        if s == 0 && c == 0 {
            continue;
        }
        if s == 0 || c == 0 || s + c > region {
            return Err(format!("WAD3 pair {p}: ({s}, {c}) outside the WAD's {region} sectors"));
        }
        if (329..=337).contains(&p) || (349..=380).contains(&p) {
            continue;
        }
        let at = s * SECTOR;
        if root.overlaps(at, c * SECTOR) {
            return Err(format!("WAD3 pair {p} at sector {s} overlaps a structure already placed"));
        }
        let b = d.sectors(sector + s, c)?;
        match p {
            183 | 185..=188 => text_table(t, root, g, at, &b, true),
            189..=201 => frame(t, root, Space::Disc, g, at, &b, "func_L01_003FD2F0 (func_L01_004C25F8 uploads the pixels, 512 x 416 from constants), func_L01_003FC8C0 loads pairs 189-201 in turn"),
            317..=328 => stream_set(t, root, g, at, &b)?,
            _ => unrecognised(t, root, g, at, c * SECTOR, format!("pair {p} ({s}, {c}) starts {:02x?}", &b[..4])),
        }
    }
    Ok(())
}

/// WAD8: everything but the raw images of pairs 0-73, 400-409 and 783-784 (wads.rs's).
/// WAD8's header in the table of contents (RAM 0x165590, the copy at 0x160C40).
const WAD8_AT: usize = 0x4950;
/// The save slots' pictures: WAD8 pairs 1253 to 1281 (frontbin func_003B22C0 sets the slots' picture objects up from pair 1253;
/// func_003B5C68's byte table, at 0x1D8C20 in the front end, gives a level's picture number n, 1 to 29, and the slot reads pair
/// 1252 + n: an LZ stream of a 2FIP image, unpacked by func_0039B760).
pub const SLOT_PICTURES: usize = 1253;
pub const SLOT_PICTURE_COUNT: usize = 29;

/// The save slots' 29 pictures (`SLOT_PICTURES`), in picture number order (picture n at index n - 1), as the GS samples them.
pub fn slot_pictures(iso: &str) -> Result<Vec<crate::core::Texture>, String> {
    let mut d = Disc::open(iso)?;
    let tc = crate::toc::read(&mut d)?;
    let w = tc.wads.iter().find(|w| w.at == WAD8_AT).ok_or_else(|| format!("the table of contents has no WAD8 header at {WAD8_AT:#x}"))?;
    (SLOT_PICTURES..SLOT_PICTURES + SLOT_PICTURE_COUNT)
        .map(|pair| {
            let (s, c) = w.pairs.get(pair).copied().ok_or_else(|| format!("WAD8: no pair {pair}"))?;
            let b = d.sectors(w.sector + s as u64, c as u64)?;
            let u = crate::lz::unpack(&b).map_err(|e| format!("WAD8 pair {pair}: {e}"))?;
            crate::pif::texture(&u.data).map_err(|e| format!("WAD8 pair {pair}: {e}"))
        })
        .collect()
}

pub fn wad8(t: &mut Tracker, d: &mut Disc, root: &mut Node, header: &[u8], sector: u64, region: u64) -> Result<(), String> {
    let g = "WAD8";
    for p in 0..(header.len() - 8) / 8 {
        let (s, c) = pair(header, p)?;
        if s == 0 && c == 0 {
            continue;
        }
        if s == 0 || c == 0 || s + c > region {
            return Err(format!("WAD8 pair {p}: ({s}, {c}) outside the WAD's {region} sectors"));
        }
        if p <= 73 || (400..=409).contains(&p) || (783..=784).contains(&p) {
            continue;
        }
        let at = s * SECTOR;
        if root.overlaps(at, c * SECTOR) {
            return Err(format!("WAD8 pair {p} at sector {s} overlaps a structure already placed"));
        }
        if (785..=848).contains(&p) || p == 858 {
            let l = t.ignored_line(g, "entry of pairs 783-848 and 858 (a raw image or an LZ stream)", "no reader's index range reaches it (bases 782 and 868; see headers.rs)", Space::Disc);
            let n = t.child(root, l, at, c * SECTOR);
            t.close(n);
            continue;
        }
        let b = d.sectors(sector + s, c)?;
        let head = &b[..4];
        if head == b"2FIP" {
            // A raw image: pif.rs (its own group: wads.rs's lines for the images it places keep their strings).
            let _ = crate::pif::image(t, root, "WAD8 raw images", at, &b, RAW_IMAGE_GAME);
        } else if &head[..3] == b"WAD" {
            let unpacked = crate::lz::unpack(&b).map(|s| s.data);
            let kind = match (&unpacked, p) {
                (Ok(u), _) if u.starts_with(b"2FIP") => 0,
                (Ok(_), 74..=101) => 1,
                (Ok(_), 122) => 2,
                (Ok(_), 655..=683 | 715..=743) => 3,
                _ => 4,
            };
            let (name, group): (&str, &str) = match kind {
                0 => ("image stream: LZ (unpacks to a 2FIP image)", "WAD8 unpacked images"),
                1 => ("level map block stream: LZ", BLOCK_G),
                2 => ("level map block set stream: LZ", BLOCK_G),
                3 => ("level map stream: LZ (func_L01_0042AD60: pair 654 + the map index, or 714 + it)", MAP_G),
                _ => ("LZ stream of a kind no decoder knows", "WAD8 unpacked other"),
            };
            let l = t.line(g, name, "lz::unpack", LZ_GAME, Space::Disc);
            crate::walk::lz_block(t, root, l, &b, at, c * SECTOR, group, |t, r, u| {
                match kind {
                    0 => {
                        let _ = image_unpacked(t, r, "WAD8 unpacked images", 0, u, PACKED_IMAGE_GAME);
                    }
                    1 => map_block(t, r, 0, u),
                    2 => map_block_set(t, r, u),
                    3 => level_map(t, r, u),
                    _ => {}
                }
                Ok(())
            })?;
        } else if (774..=781).contains(&p) {
            text_table(t, root, g, at, &b, false);
        } else if p == 782 && head == b"BE_H" {
            be_h(t, root, g, at, &b);
        } else if p == 1364 {
            frame(t, root, Space::Disc, g, at, &b, "func_L01_00491890 (the reader of base pair 1354, loads it raw), the pixels go to the GS as 512 x 416 RGBA like func_L01_003FD2F0's frames");
        } else {
            unrecognised(t, root, g, at, c * SECTOR, format!("pair {p} ({s}, {c}) starts {head:02x?}"));
        }
    }
    Ok(())
}
