//! The table of contents' structures and the level headers, field by field, with the game functions that read each field.
//!
//! Function names are NTSC-U: `func_XXXXXXXX` in boot_elf's core, `func_L01_XXXXXXXX` for level 1's copy of a function that
//! rac3-decomp's level catalogue (config/us/levels/functions.tsv) lists as the same function in all 34 story levels,
//! `frontbin func_XXXXXXXX` / `front end func_XXXXXXXX` in frontbin and in boot_elf's own front end.

use crate::rd::Fields;
use crate::toc::{FileKind, LevelFile, Toc, WadHeader};
use crate::track::{NOTHING, Node, Space, Tracker};

/// Where the game's code places each WAD header and the level table in the table of contents (0x160C40 in RAM): the offsets
/// in the readers' addresses (WAD2's sector at 0x16128C, ...).
pub const WAD_AT: [usize; 9] = [0x0, 0x648, 0x690, 0x1280, 0x1EB0, 0x2248, 0x4588, 0x4950, 0x7400];

const WAD_GAME: [&str; 8] = [
    "func_L01_004AC588, func_L01_004B4AF0, func_L01_004B4D48 (frontbin func_003A21C0, func_003A3368, func_003A35C0): the sector and 100 movie records (info block sector, movie sector and bytes); the size word and the info blocks' byte counts are read by nothing",
    "front end func_003826C8 (pair 0), front end func_003982E0 (pair 1), func_L01_004303A0 and func_L01_004304B0 (pair 2), func_L01_00428008 (pair 3), frontbin func_0037D200 and front end func_00381280 (pair 7); pair 4 (an EE program) is read by no code (skipped), pairs 5 and 6 are zero",
    "381 (sector, sectors) pairs at 0x1612D8 + 8p, each word read where a reader loads the pair (through the descriptor of func_L01_00490B98), zero otherwise: pairs 183 + the language byte 0x143A07 (183, 185-188; 184 is empty) and 189 + the frame 0-12 (189-201): func_L01_003FC8C0; pairs 329-337: func_L01_0047C910; pairs 349-379: func_L01_0048FE88; pair 380: func_L01_00497028; pairs 317-328 (the frame stream sets): no code addresses 0x161CC0..0x161D20, skipped; pairs 0-182, 202-316 and 338-348 are empty",
    "389 (sector, sectors) pairs at 0x161EC8 + 8p: pairs 0-1: func_L01_00429998 (called only with 0 and 1; pairs 2-5 are addressed by no code in the story levels or the front ends, skipped); pairs 6-14: front end func_003BBCE8 and frontbin func_003B6528 (index * 8 + 0x161EF8); pairs 15-346 (words 30-693): eight comic records of 83 words in the SCENE record form (sound ids 4000-4007 through the resolver at 0x1DBC0 + id * 0x14C: func_L01_0045B5A0 / frontbin func_0039C2E8 words 0-1 and 4-11, func_L01_00429618 words 2-3, func_L01_00429450 / func_L01_004294D0 words 12-13, func_L01_004296B0 words 14 on, func_L01_00429A70 the hand-off); pairs 347-349: frontbin func_00393DC8 (n * 8 + 0x1629A0); pairs 350-381: frontbin func_00393E90 (func_L01_00423068) and front end func_00398F48; pairs 382-388: the VAG clips, pair 382 + id - 5000, the resolver reads their sector words only (the sectors words are skipped)",
    "57 armor records of two (sector, sectors) pairs at 0x162AF8 + 16r (record r: model block, texture list): func_L01_00422838 (frontbin func_00393A18) and front end func_00398AD0 load the texture pair (+0x162B00 + 16r) into the buffer, draw its images, then load the model pair (+0x162AF8 + 16r) over it; func_L01_00422D70 loads record 31's model pair (0x162CE8) and texture pair (0x162CF0); func_L01_00423238 loads record 56's two pairs (0x162E78 + 8n)",
    "func_L01_0045B5A0 (frontbin func_0039C2E8): u32 sector offsets, indexed by sound id - 1000 plus language",
    "48 entries of five words: the id (searched by frontbin func_00395BC0 and func_00395E18, func_L01_0042A300, func_L01_0042A520, func_L01_004E7148), the gadget block's sector and sectors (frontbin func_003959A8, func_L01_00429C30; func_003959A8 also tests the bank's sectors for zero), the bank's sector and sectors (frontbin func_00395AC8)",
    "1365 (sector, sectors) pairs at 0x165598 + 8p, each loaded through the descriptor of func_L01_00490B98 (at 0x236798 + n * 0x20: field 5 the base pair, field 7 the index; func_L01_00490AF0 and func_L01_00490BF8 load base[index]); the index is a runtime frame, map or language number set by the screens, so a base pair's range runs to the next base pair: base 71 func_L01_0049CA58 (pair 71 only), 410 func_L01_00496AB0, 469 and 868 func_L01_004832F8, 511 func_L01_0048A848, 654 and 714 + the map index (655-683, 715-743) func_L01_0042AD60, func_L01_0042B180, func_L01_0048BC68, 774 + the language (774-781) func_L01_0049F628, 782 frontbin func_003A9B10 and front end func_003AF2D0, 878 func_L01_00481298, 979 func_L01_00488C28, 996 func_L01_004910F0, 1052 func_L01_004BE9A0, func_L01_004BFD68, func_L01_004C07D8, func_L01_004C1440, 1122 and 1124 func_L01_004879F8, 1123 func_L01_004849E8, 1146 func_L01_0049A930, 1197 func_L01_0048E5D8, 1206 func_L01_0048F7E8, 1211 func_L01_0048D030 and frontbin func_003B0580, 1220 func_L01_00499898, 1248 func_L01_00498868, 1251 func_L01_0049E448, 1253 func_L01_004879F8, func_L01_0048A848, func_L01_0048B650 and frontbin func_003B22C0, 1313 func_L01_00492BD0, 1354 + 0-10 func_L01_00491890; skipped: pairs 74-101 and 122 (map blocks: no code addresses them) and pairs 0-70, 72-73, 400-409, 783-848 and 858 (no reader's index range reaches them: 0-70 lie below the first base pair 71, 72-73 lie beyond the one image its reader loads (index 0), 400-409 below base 410, 783-848 and 858 above base 782, which is loaded once at index 0, and below base 868); empty pairs are zero",
];

/// The eight WAD headers, the level table and the bytes after it: the table of contents (sector 1001, read by func_0013D2E0).
pub fn toc(t: &mut Tracker, root: &mut Node, tc: &Toc, regions: &[u64; 8]) {
    let g = "Table of contents";
    for (k, w) in tc.wads.iter().enumerate() {
        let l = t.line(g, &format!("WAD{} header", k + 1), "headers::wad_header", WAD_GAME[k], Space::Disc);
        let mut n = t.child(root, l, w.at as u64, w.size as u64);
        if let Err(e) = wad_header(&mut n, &tc.bytes[w.at..w.at + w.size], k, w, regions[k]) {
            n.fail(e);
        }
        t.close(n);
    }
    let l = t.line(g, "level table (60 slots of AUDIO, LEVEL, SCENE (header sector, file sectors))", "headers::level_table", "func_0013D310 (header sectors; the file sectors are read by nothing and are the sectors the walk claims for each file)", Space::Disc);
    let mut n = t.child(root, l, 0x7400, 0x5a0);
    if let Err(e) = level_table(&mut n, &tc.bytes[0x7400..0x79a0]) {
        n.fail(e);
    }
    t.close(n);
    let l = t.line(g, "bytes after the level table (zero)", "headers::toc_tail", "func_0013D2E0 reads them, func_0013D310 overwrites them with the level's headers", Space::Disc);
    let mut n = t.child(root, l, 0x79a0, 0x8000 - 0x79a0);
    if let Err(e) = Fields::new(&tc.bytes[0x79a0..0x8000], &mut n).fill(0, 0x8000 - 0x79a0, 0, "after the level table") {
        n.fail(e);
    }
    t.close(n);
}

fn wad_header(n: &mut Node, b: &[u8], k: usize, w: &WadHeader, region: u64) -> Result<(), String> {
    let mut f = Fields::new(b, n);
    f.u32_eq(0, (WAD_AT[k + 1] - WAD_AT[k]) as u32, "header size (the code places the next header there)")?;
    f.u32(4)?;
    let words = (w.size - 8) / 4;
    let word = |i: usize| crate::disc::u32_at(b, 8 + 4 * i);
    match k {
        // 100 movie records of 16 bytes.
        0 => {
            for r in 0..words / 4 {
                let at = 8 + 16 * r;
                let (info, info_bytes, movie, movie_bytes) = (word(4 * r)?, word(4 * r + 1)?, word(4 * r + 2)?, word(4 * r + 3)?);
                if info == 0 && info_bytes == 0 && movie == 0 && movie_bytes == 0 {
                    f.fill(at, 16, 0, "empty movie record")?;
                    continue;
                }
                if movie == 0 || movie_bytes == 0 || movie as u64 + (movie_bytes as u64).div_ceil(0x800) > region {
                    return Err(format!("movie record {r}: movie at sector {movie} with {movie_bytes:#x} bytes outside the WAD's {region} sectors"));
                }
                f.u32(at + 8)?;
                f.u32(at + 12)?;
                f.u32(at)?;
                if info != 0 {
                    // Read as 16 sectors by the players; the byte count is not read.
                    if info_bytes == 0 || info_bytes > 0x8000 || info as u64 + (info_bytes as u64).div_ceil(0x800) > movie as u64 {
                        return Err(format!("movie record {r}: info block at sector {info} with {info_bytes:#x} bytes"));
                    }
                } else if info_bytes != 0 {
                    return Err(format!("movie record {r}: no info block but {info_bytes:#x} info bytes"));
                }
                f.mark(at + 4, 4);
            }
        }
        // Eight (sector, sectors) pairs; pairs 4-6 are read by no story code (4 holds an EE program, 5-6 are zero).
        1 => {
            for p in [0usize, 1, 2, 3, 7] {
                let (s, c) = (word(2 * p)?, word(2 * p + 1)?);
                if s == 0 || c == 0 || (s + c) as u64 > region {
                    return Err(format!("pair {p}: ({s}, {c}) outside the WAD's {region} sectors"));
                }
                f.u32(8 + 8 * p)?;
                f.u32(12 + 8 * p)?;
            }
            for p in [5usize, 6] {
                f.fill(8 + 8 * p, 8, 0, "unused pair")?;
            }
            f.skip(8 + 8 * 4, 8);
        }
        // 57 armor records: (sector, sectors) of the model block and of the texture block.
        4 => {
            for p in 0..words / 2 {
                let (s, c) = (word(2 * p)?, word(2 * p + 1)?);
                if s == 0 || c == 0 || (s + c) as u64 > region {
                    return Err(format!("pair {p}: ({s}, {c}) outside the WAD's {region} sectors"));
                }
                f.u32(8 + 8 * p)?;
                f.u32(12 + 8 * p)?;
            }
        }
        // 48 gadget entries: id, gadget block (sector, sectors), sound bank (sector, sectors); an empty block or bank is (0, 0).
        6 => {
            for e in 0..words / 5 {
                let at = 8 + 20 * e;
                f.u32(at)?;
                for (s_at, what) in [(at + 4, "gadget block"), (at + 12, "sound bank")] {
                    let (s, c) = (crate::disc::u32_at(b, s_at)?, crate::disc::u32_at(b, s_at + 4)?);
                    if (s == 0) != (c == 0) || (s as u64 + c as u64) > region {
                        return Err(format!("entry {e}: {what} ({s}, {c}) outside the WAD's {region} sectors"));
                    }
                    f.u32(s_at)?;
                    f.u32(s_at + 4)?;
                }
            }
        }
        // u32 sector offsets of the dialogue clips; 0 is no clip.
        5 => {
            for i in 0..words {
                let s = word(i)?;
                if s as u64 >= region {
                    return Err(format!("word {i}: sector {s} outside the WAD's {region} sectors"));
                }
                f.u32(8 + 4 * i)?;
            }
        }
        // 381 pairs: the credits text tables and frames, 2FIP image sets and the frame stream sets.
        2 => {
            for p in 0..words / 2 {
                let treat = match p {
                    183 | 185..=201 | 329..=337 | 349..=380 => Pair::Read,
                    317..=328 => Pair::Skip,
                    _ => Pair::None,
                };
                pair(&mut f, b, p, region, treat)?;
            }
        }
        // 389 pairs, with the eight comic records (words 30-693) between pair 14 and pair 347.
        3 => {
            for p in 0..words / 2 {
                let treat = match p {
                    0..=1 | 6..=14 | 347..=381 => Pair::Read,
                    2..=5 => Pair::Skip,
                    382..=388 => Pair::Sector,
                    15..=346 => continue,
                    _ => Pair::None,
                };
                pair(&mut f, b, p, region, treat)?;
            }
            scene_records(&mut f, b, 8 + 4 * 30, 8, region)?;
        }
        // 1365 pairs read through the descriptors of func_L01_00490B98.
        7 => {
            for p in 0..words / 2 {
                let treat = match p {
                    74..=101 | 122 => Pair::Skip,
                    0..=70 | 72..=73 | 400..=409 | 783..=848 | 858 => Pair::Skip,
                    _ => Pair::Read,
                };
                pair(&mut f, b, p, region, treat)?;
            }
        }
        _ => unreachable!("eight WAD headers"),
    }
    Ok(())
}

/// What the game does with a (sector, sectors) pair of a WAD header.
#[derive(Clone, Copy)]
enum Pair {
    /// A reader loads the pair: both words.
    Read,
    /// A reader takes only the sector word.
    Sector,
    /// No code addresses the pair: its words are skipped.
    Skip,
    /// No reader found: a non-empty pair fails the node.
    None,
}

/// Pair `p` of a WAD header: an empty (0, 0) pair is required zero; otherwise it is treated as `treat` says, a read pair
/// lying inside the WAD's `region` sectors.
fn pair(f: &mut Fields, b: &[u8], p: usize, region: u64, treat: Pair) -> Result<(), String> {
    let at = 8 + 8 * p;
    let (s, c) = (crate::disc::u32_at(b, at)?, crate::disc::u32_at(b, at + 4)?);
    if s == 0 && c == 0 {
        return f.fill(at, 8, 0, "empty pair");
    }
    match treat {
        Pair::Skip => f.skip(at, 8),
        Pair::None => return Err(format!("pair {p}: ({s}, {c}) non-empty and no reader found")),
        Pair::Read | Pair::Sector => {
            let (s, c) = (s as u64, c as u64);
            if s == 0 || c == 0 || s + c > region {
                return Err(format!("pair {p}: ({s}, {c}) outside the WAD's {region} sectors"));
            }
            f.u32(at)?;
            if matches!(treat, Pair::Sector) {
                f.skip(at + 4, 4);
            } else {
                f.u32(at + 4)?;
            }
        }
    }
    Ok(())
}

fn level_table(n: &mut Node, b: &[u8]) -> Result<(), String> {
    let mut f = Fields::new(b, n);
    for slot in 0..60 {
        for k in 0..3 {
            let at = slot * 24 + k * 8;
            let (hs, c) = (crate::disc::u32_at(b, at)?, crate::disc::u32_at(b, at + 4)?);
            if hs == 0 && c == 0 {
                f.fill(at, 8, 0, "empty level table entry")?;
                continue;
            }
            if hs <= 1001 || c == 0 {
                return Err(format!("slot {slot} file {k}: header sector {hs}, {c} sectors"));
            }
            f.u32(at)?;
            f.u32(at + 4)?;
        }
    }
    Ok(())
}

/// A level header, at its header sectors (`copy` false: +4 is the file's sector) or as the copy at the file's start (`copy`
/// true: the same bytes with +4 zero, read by nothing). `bytes` are the header's whole sectors.
pub fn header_line(t: &mut Tracker, parent: &mut Node, f: &LevelFile, at: u64, bytes: &[u8], copy: bool) {
    let (hsize, hsec) = f.kind.header();
    let g = if copy { f.kind.name() } else { "Level headers" };
    if copy {
        let l = t.line(g, "header copy (the header with +4 zero)", "headers::header_line", NOTHING, Space::Disc);
        let mut n = t.child(parent, l, at, hsize as u64);
        let mut want = f.header[..hsize].to_vec();
        want[4..8].fill(0);
        if let Err(e) = Fields::new(&bytes[..hsize], &mut n).same(0, &want, "header copy") {
            n.fail(e);
        }
        t.close(n);
    } else {
        let (decoder, game) = match f.kind {
            FileKind::Level => (
                "headers::level_header",
                "func_0013D310 (copies 0x60 bytes), func_L01_004252F0 (+4, block 2), func_L01_00428518 / frontbin func_003953F0 (blocks 0, 2), func_L01_004A57B0 (+0xC), func_L01_0045B378 (block 1 sector), func_L01_004282C0 (blocks 4-6), func_L01_0045B3D0 (blocks 7-9 sectors); +0, +8 and the sound banks' sector counts are read by nothing",
            ),
            FileKind::Audio => (
                "headers::audio_header",
                "func_0013D310 (copies 0x1818 bytes), func_L01_0045BA68, func_L01_0045BB60, func_L01_0045BC58, func_L01_0045BD70, func_L01_0045B5A0 (+4 and each pair's sector); +0 and the pairs' byte counts are read by nothing",
            ),
            FileKind::Scene => (
                "headers::scene_header",
                "func_0013D310 (copies 0x26F0 bytes), func_L01_00429618 (+4, each record's words 2-3), func_L01_00429450 and func_L01_00456C70 (words 12-13), func_L01_004296B0 (words 14 on), func_L01_0045B5A0 / frontbin func_0039C2E8 (words 0-1 and 4-11, by language); +0 is read by nothing",
            ),
        };
        let l = t.line(g, &format!("{} header", f.kind.name()), decoder, game, Space::Disc);
        let mut n = t.child(parent, l, at, hsize as u64);
        let h = &bytes[..hsize];
        let r = match f.kind {
            FileKind::Level => level_header(&mut n, h, f.level, f.sectors),
            FileKind::Audio => audio_header(&mut n, h, f.sectors),
            FileKind::Scene => scene_header(&mut n, h, f.sectors),
        };
        if let Err(e) = r {
            n.fail(e);
        }
        t.close(n);
    }
    // The rest of the header's sectors.
    let tail = hsec as usize * 0x800 - hsize;
    let (name, game) = if copy {
        ("header copy sectors after the header (zero)".to_string(), NOTHING)
    } else {
        (format!("{} header sectors after the header (zero)", f.kind.name()), "func_0013D310 reads the sectors and copies only the header")
    };
    let l = t.line(g, &name, "headers::header_line", game, Space::Disc);
    let mut n = t.child(parent, l, at + hsize as u64, tail as u64);
    if let Err(e) = Fields::new(&bytes[hsize..hsize + tail], &mut n).fill(0, tail, 0, "after the header") {
        n.fail(e);
    }
    t.close(n);
}

fn level_header(n: &mut Node, h: &[u8], level: usize, sectors: u64) -> Result<(), String> {
    let mut f = Fields::new(h, n);
    f.u32_eq(0, 0x60, "header size")?;
    f.u32(4)?;
    f.u32_eq(8, level as u32, "level number")?;
    f.u32(0xc)?;
    for k in 0..10usize {
        let (o, c) = (crate::disc::u32_at(h, 0x10 + 8 * k)?, crate::disc::u32_at(h, 0x14 + 8 * k)?);
        if o == 0 && c == 0 {
            f.fill(0x10 + 8 * k, 8, 0, "empty block pair")?;
            continue;
        }
        if o as u64 + c as u64 > sectors {
            return Err(format!("block {k}: sectors {o}+{c} past the file's {sectors}"));
        }
        match k {
            // Block 3 (a copy of the gameplay block's +0x90 table) is loaded by nothing: its pair is read by nothing.
            3 => f.skip(0x28, 8),
            _ => {
                f.u32(0x10 + 8 * k)?;
                f.u32(0x14 + 8 * k)?;
            }
        }
    }
    Ok(())
}

fn audio_header(n: &mut Node, h: &[u8], sectors: u64) -> Result<(), String> {
    let mut f = Fields::new(h, n);
    f.u32_eq(0, 0x1818, "header size")?;
    f.u32(4)?;
    let pairs = (0x1818 - 8) / 8;
    let mut used: Vec<(u32, u32)> = Vec::new();
    for p in 0..pairs {
        let (s, c) = (crate::disc::u32_at(h, 8 + 8 * p)?, crate::disc::u32_at(h, 12 + 8 * p)?);
        if s == 0 && c == 0 {
            f.fill(8 + 8 * p, 8, 0, "empty pair")?;
            continue;
        }
        if s == 0 || c == 0 || s as u64 * 0x800 + c as u64 > sectors * 0x800 {
            return Err(format!("pair {p}: ({s:#x}, {c:#x}) outside the file's {sectors} sectors"));
        }
        used.push((s, c));
        f.u32(8 + 8 * p)?;
        f.mark(12 + 8 * p, 4);
    }
    used.sort();
    for w in used.windows(2) {
        if w[0].0 as u64 * 0x800 + w[0].1 as u64 > w[1].0 as u64 * 0x800 {
            return Err(format!("stream at sector {:#x} ({:#x} bytes) runs into the next at {:#x}", w[0].0, w[0].1, w[1].0));
        }
    }
    Ok(())
}

/// The SCENE header: size, base sector, then 30 records of 83 words. Words 0-1: the record's stream sectors (the sound-id
/// resolver, ids below 1000); words 2-3: (sector, sectors) of its header block; words 12-13: (sector, sectors) of another
/// block; words 14 on: increasing chunk boundaries read pairwise until a difference is not positive (zero ends the list).
fn scene_header(n: &mut Node, h: &[u8], sectors: u64) -> Result<(), String> {
    let mut f = Fields::new(h, n);
    f.u32_eq(0, 0x26f0, "header size")?;
    f.u32(4)?;
    scene_records(&mut f, h, 8, 30, sectors)
}

/// `count` records of 83 words from `first` (a SCENE header's, and WAD4's eight comic records, which have the same form).
fn scene_records(f: &mut Fields, h: &[u8], first: usize, count: usize, sectors: u64) -> Result<(), String> {
    for r in 0..count {
        let at = first + r * 0x14c;
        let w = |i: usize| crate::disc::u32_at(h, at + 4 * i);
        if (0..83).all(|i| w(i).is_ok_and(|v| v == 0)) {
            f.fill(at, 0x14c, 0, "empty scene record")?;
            continue;
        }
        // Words 0-1 and 4-11: the record's VAG clips, left and right for languages 0, 2, 3, 4, 5 (the resolver reads words
        // 2k and 2k+1 for language byte k; k = 1 is never used, so words 2-3 hold the subtitle block).
        for i in [0usize, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13] {
            if w(i)? as u64 > sectors {
                return Err(format!("record {r} word {i}: sector {:#x} past the file's {sectors}", w(i)?));
            }
            f.u32(at + 4 * i)?;
        }
        let mut prev = 0;
        let mut ended = false;
        for i in 14..83 {
            let v = w(i)?;
            if ended || v == 0 {
                if v != 0 {
                    return Err(format!("record {r} word {i}: {v:#x} after the chunk list ended"));
                }
                ended = true;
                f.mark(at + 4 * i, 4);
                continue;
            }
            if v <= prev || v as u64 > sectors {
                return Err(format!("record {r} word {i}: chunk boundary {v:#x} after {prev:#x} (file {sectors} sectors)"));
            }
            prev = v;
            f.u32(at + 4 * i)?;
        }
    }
    Ok(())
}
