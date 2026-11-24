//! The front end's WAD blocks: WAD2 pairs 2 and 7, WAD4 pairs 0-14 and 347-349, WAD7's gadget blocks.
//!
//! WAD2 pair 2 (the memory card save template; func_L01_004303A0 and frontbin func_00397380 load it, func_L01_004304B0 and
//! frontbin func_00397490 write it to the card):
//! - six u32: the offset (0x18) of the icon.sys, its size (the constant 0x3C4 in the writer), the offset and size of the icon
//!   file, the offset of the save data and its size;
//! - the icon.sys and the icon file, written to the card as they are (nothing reads their insides);
//! - the save data: two u32 (the byte count of the first block and of each level block, compared by frontbin func_003972A0
//!   with the sizes its field tables give), the first block, then 37 level blocks (the default new-game save). A block is a
//!   payload size, a CRC-16 of the payload (frontbin func_003997F0 checks both with func_00399748), records (u32 variable id, u32
//!   byte count, the value padded to 4 bytes: frontbin func_00399A00 copies each value into the variable the id names) and a
//!   terminator (id -1, then a zero word).
//!
//! WAD2 pair 7 (frontbin func_0037D200 and the boot front end's func_00381280 load it): 16 (offset, size) u32 pairs, 13 streams
//! at 0x40-aligned offsets, and three empty pairs. Streams 0-4 are LZ blocks of the game's text, one per language (the
//! language number picks one: 0 and 1 the first); the unpacked block is a table of 16-byte records (func_003B0DA0 relocates
//! the offsets and takes the count from the first offset) and the strings after it. Stream 5 is a raw resource directory
//! (func_00394060 copies it); streams 6 and 7 are LZ blocks it sizes (func_00394300 unpacks them); 8-10 are empty LZ streams;
//! 11 is a 512 x 416 RGBA image (func_003A42E0 draws it) and 12 a sound bank that func_0039C1C8 sends to the IOP.
//!
//! WAD4 pairs 0-5 (func_L01_00429998 loads pair p of the table: a header sector of (offset, size) pairs, then the LZ streams
//! at those offsets from the end of the header sector; each unpacks to a SCENE chunk). Pairs 6-14 (the boot front end's
//! func_003BBCE8 and frontbin func_003B6528 load pair 6 + the screen number): one LZ stream, a front end screen. Pairs
//! 347-349 (frontbin func_00393DC8 loads pair 347 + a number from 0 to 2): a model of the moby class form that func_00394C58
//! relocates; its insides belong to moby.rs and are one extent here.
//!
//! WAD7 gadget blocks, words 1-2 and 6-7 of each 10-word record (the record is two 5-word entries: id, block sector and
//! sectors, bank sector and sectors; frontbin func_00395BC0 and func_00395E18 search the ids): four u32 (each LZ stream's
//! offset in the WAD and size), a moby class LZ stream (extent only) and a texture LZ stream (a list and 2FIP images).

use crate::disc::{Disc, SECTOR, u16_at, u32_at};
use crate::rd::Fields;
use crate::track::{Node, Space, Tracker};

const G2: &str = "WAD2";
const G4: &str = "WAD4";
const G7: &str = "WAD7";

const SAVE_FN: &str = "frontbin func_003972A0 (new game; func_L01_004302C0 in the levels) reads the sizes and func_003997F0 the block header, func_00399A00 the records";

/// (id, RAM address, byte count) of frontbin's save variable descriptors at 0x32E3C8 (the first block) and 0x32E658 (each level block).
const ICON_WHY: &str = "searched: the writers func_L01_004304B0 and frontbin func_00397490 hand the template's icon.sys and icon file to the memory card whole (the sizes 0x3C4 and the header's +8, +0xC), and the loaders func_L01_004303A0 and frontbin func_00397380 read only the header; no code reads their insides (the PS2 browser does)";

const VARS_FIRST: [(u32, u32, u32); 40] = [
    (0x0, 0x1d545c, 0x4),
    (0x27, 0x142660, 0x80),
    (0x3, 0x1d5528, 0x4),
    (0x4, 0x1d4be0, 0x8),
    (0x3eb, 0x1d4c38, 0x4),
    (0x5, 0x1426e0, 0x110),
    (0x7, 0x1d5468, 0x13),
    (0x8, 0x1d54b0, 0x20),
    (0x9, 0x1427f0, 0x270),
    (0xa, 0x142ca0, 0x9c),
    (0xb, 0x142d40, 0x9c),
    (0x28, 0x142de0, 0x270),
    (0xc, 0x1d54d8, 0x40),
    (0xd, 0x1d4c60, 0x20),
    (0xe, 0x1d5530, 0x3c),
    (0x14, 0x143050, 0xf0),
    (0xf, 0x143230, 0x720),
    (0x10, 0x1a7e18, 0x10b0),
    (0x11, 0x1a7c10, 0x204),
    (0x12, 0x1a74c0, 0x750),
    (0x26, 0x143950, 0xbc),
    (0x1e, 0x1425c0, 0xa0),
    (0x20, 0x1d4c40, 0x1c),
    (0x25, 0x1d5498, 0x13),
    (0x29, 0x1a2018, 0x10c),
    (0x2a, 0x142a60, 0x9c),
    (0x2d, 0x142b00, 0x9c),
    (0x2b, 0x142ba0, 0x100),
    (0x2c, 0x1d54d0, 0x4),
    (0x2e, 0x1d4be8, 0x28),
    (0x2f, 0x1a1f18, 0x100),
    (0x3ed, 0x1d4ca0, 0x4),
    (0x3f2, 0x1a8ec8, 0x320),
    (0x3f3, 0x1d5598, 0x4),
    (0x1b5a, 0x1d4c80, 0x20),
    (0x1b5b, 0x1d55a0, 0xc),
    (0x1b5c, 0x1d55b0, 0x6),
    (0x1b5d, 0x143b30, 0xc0),
    (0x1b5e, 0x143bf0, 0xc0),
    (0x1b5f, 0x143cb0, 0xc0),
];
const VARS_LEVEL: [(u32, u32, u32); 9] = [
    (0xbb9, 0x1d5570, 0x1),
    (0xbba, 0x1a9318, 0x800),
    (0xbbb, 0x1bbb18, 0x8),
    (0xbbc, 0x1bbc40, 0x10),
    (0xbbd, 0x1bbe90, 0x400),
    (0xfa2, 0x143d70, 0x4),
    (0xfa5, 0x1c5290, 0x40),
    (0xfa7, 0x1a91e8, 0x4),
    (0xfa8, 0x1a9280, 0x4),
];

/// A child of `parent` at `at`, `size` bytes, checked against the parent's bytes and the structures already placed.
fn put(t: &mut Tracker, parent: &mut Node, line: usize, at: usize, size: usize) -> Result<Node, String> {
    if at + size > parent.size as usize {
        return Err(format!("{}: {} of {size:#x} bytes at {at:#x} past the {:#x} bytes", parent.place, t.lines[line].name, parent.size));
    }
    if parent.overlaps(at as u64, size as u64) {
        return Err(format!("{}: {} at {at:#x}+{size:#x} overlaps another structure", parent.place, t.lines[line].name));
    }
    Ok(t.child(parent, line, at as u64, size as u64))
}

/// Closes a node; a decoder error becomes the node's failure.
fn done(t: &mut Tracker, mut n: Node, r: Result<(), String>) {
    if let Err(e) = r {
        n.fail(e);
    }
    t.close(n);
}

/// A structure line of bytes nothing in the game reads: `why` says what was searched.
fn ignored(t: &mut Tracker, parent: &mut Node, group: &str, name: &str, why: &'static str, space: Space, at: usize, size: usize) -> Result<(), String> {
    let l = t.ignored_line(group, name, why, space);
    let n = put(t, parent, l, at, size)?;
    t.close(n);
    Ok(())
}

/// The bytes of the entry at `sector` (sectors `sectors`) of a WAD that starts at `wad_sector`, handed to its decoder. The pair
/// numbers: WAD2 (`wad` 1) pairs 2 and 7, WAD4 (`wad` 3) pairs 0-14 and 347-349, WAD7 (`wad` 6) 2 * record + 0 for the block
/// of words 1-2 and 2 * record + 1 for words 6-7. An entry already placed or outside the WAD is skipped.
pub fn block(t: &mut Tracker, d: &mut Disc, root: &mut Node, wad_sector: u64, region: u64, wad: usize, pair: usize, sector: u64, sectors: u64) -> Result<(), String> {
    let at = sector * SECTOR;
    if sector == 0 || sectors == 0 || sector + sectors > region || root.overlaps(at, sectors * SECTOR) {
        return Ok(());
    }
    let b = d.sectors(wad_sector + sector, sectors)?;
    match (wad, pair) {
        (1, 2) => wad2_save(t, root, at, &b),
        (1, 7) => wad2_front(t, root, at, &b),
        (3, 0..=1) => wad4_chunks(t, root, at, &b),
        (3, 2..=5) => {
            let l = t.ignored_line(G4, "scene chunk set (pairs 2-5: a header sector of (offset, size) pairs and LZ streams)", "func_L01_00429998 has two callers in all overlays, passing 1 and 0, and the front ends read pair 6 on: pairs 2-5 are never loaded (see headers.rs)", Space::Disc);
            let n = t.child(root, l, at, b.len() as u64);
            t.close(n);
            Ok(())
        }
        (3, 6..=14) => wad4_screen(t, root, at, &b),
        (3, 347..=349) => {
            let mut r = t.region(root, G4, at, b.len() as u64);
            let res = crate::moby::class_in(t, &mut r, G4, 0, &b, Space::Disc).map(|_| ());
            t.close(r);
            res
        }
        (6, p) => wad7_gadget(t, root, at, &b, sector, p),
        _ => Err(format!("frontwad::block: WAD{} pair {pair} has no decoder", wad + 1)),
    }
}

// ---- WAD2 pair 2: the save template ----

/// The save data's CRC-16 (frontbin func_00399748): a 32-bit register starting at 0xEDB88320 whose low 16 bits take each byte
/// at bit 8, then eight shifts with the polynomial 0x1F45.
fn crc16(b: &[u8]) -> u32 {
    let mut v: u32 = 0xedb8_8320;
    for &x in b {
        v ^= (x as u32) << 8;
        for _ in 0..8 {
            v = if v & 0x8000 != 0 { (v << 1) ^ 0x1f45 } else { v << 1 };
        }
    }
    v & 0xffff
}

fn wad2_save(t: &mut Tracker, root: &mut Node, at: u64, b: &[u8]) -> Result<(), String> {
    let mut r = t.region(root, G2, at, b.len() as u64);
    let w: Vec<usize> = (0..6).map(|i| u32_at(b, 4 * i).map(|v| v as usize)).collect::<Result<_, _>>()?;
    const SYS: usize = 0x3c4;
    let (o_sys, o_ico, s_ico, o_dat) = (w[0], w[2], w[3], w[4]);
    if o_sys != 0x18 || o_ico != o_sys + SYS || o_dat != o_ico + s_ico {
        return Err(format!("save template header {w:x?}: icon.sys, icon and save data are not consecutive from 0x18"));
    }
    let (s1, s2) = (u32_at(b, o_dat)? as usize, u32_at(b, o_dat + 4)? as usize);
    let size = 8 + s1 + 37 * s2;
    if o_dat + size > b.len() {
        return Err(format!("save data of {size:#x} bytes at {o_dat:#x} past the entry's {:#x}", b.len()));
    }
    let l = t.line(
        G2,
        "save template header (+0 icon.sys offset, +4 its size, +8 icon file offset, +0xC its size, +0x10 save data offset, +0x14 its size)",
        "frontwad::wad2_save",
        "func_L01_004304B0 and frontbin func_00397490 (memory card save: +0, +8, +0xC and +0x10; the icon.sys size is the constant 0x3C4 there); frontbin func_00397380 and func_L01_004303A0 read +0x10; nothing reads +0x14 (checked by the decoder)",
        Space::Disc,
    );
    let mut n = put(t, &mut r, l, 0, 0x18)?;
    let res = (|| -> Result<(), String> {
        let mut f = Fields::new(&b[..0x18], &mut n);
        f.u32(0)?;
        f.u32_eq(4, SYS as u32, "icon.sys size")?;
        f.u32(8)?;
        f.u32(12)?;
        f.u32(16)?;
        f.u32_eq(20, size as u32, "save data size (8 + first block + 37 level blocks)")
    })();
    done(t, n, res);
    ignored(t, &mut r, G2, "memory card icon.sys (PS2D)", ICON_WHY, Space::Disc, o_sys, SYS)?;
    ignored(t, &mut r, G2, "memory card icon file", ICON_WHY, Space::Disc, o_ico, s_ico)?;
    let l = t.line(G2, "save data sizes (+0 bytes of the first block, +4 bytes of each of the 37 level blocks)", "frontwad::wad2_save", SAVE_FN, Space::Disc);
    let mut n = put(t, &mut r, l, o_dat, 8)?;
    let res = (|| -> Result<(), String> {
        let mut f = Fields::new(&b[o_dat..o_dat + 8], &mut n);
        f.u32(0)?;
        f.u32(4)?;
        Ok(())
    })();
    done(t, n, res);
    save_block(t, &mut r, b, o_dat + 8, s1, &VARS_FIRST)?;
    for k in 0..37 {
        save_block(t, &mut r, b, o_dat + 8 + s1 + k * s2, s2, &VARS_LEVEL)?;
    }
    crate::walk::pad_mem(t, &mut r, b, G2);
    t.close(r);
    Ok(())
}

/// A save block: payload size, CRC-16, records, terminator.
fn save_block(t: &mut Tracker, r: &mut Node, b: &[u8], at: usize, size: usize, vars: &[(u32, u32, u32)]) -> Result<(), String> {
    let lh = t.line(G2, "save data block header (+0 payload byte count, +4 CRC-16 of the payload)", "frontwad::save_block", SAVE_FN, Space::Disc);
    let lr = t.line(G2, "save record header (+0 variable id, +4 byte count)", "frontwad::save_block", SAVE_FN, Space::Disc);
    let lp = t.line(G2, "save record padding (to 4 bytes, zero)", "frontwad::save_block", crate::track::NOTHING, Space::Disc);
    let lt = t.line(G2, "save record terminator (+0 id -1, +4 zero)", "frontwad::save_block", SAVE_FN, Space::Disc);
    let mut n = put(t, r, lh, at, 8)?;
    let res = (|| -> Result<(), String> {
        let mut f = Fields::new(&b[at..at + 8], &mut n);
        let psz = f.u32(0)? as usize;
        let crc = f.u32(4)?;
        if psz != size - 8 {
            return Err(format!("payload byte count {psz:#x}, the block holds {:#x}", size - 8));
        }
        if crc16(&b[at + 8..at + 8 + psz]) != crc {
            return Err(format!("CRC-16 {crc:#x}, computed {:#x}", crc16(&b[at + 8..at + 8 + psz])));
        }
        Ok(())
    })();
    done(t, n, res);
    let (mut p, end) = (at + 8, at + size);
    loop {
        if p + 8 > end {
            return Err(format!("save block at {at:#x}: records run past the block's end {end:#x} at {p:#x}"));
        }
        let (id, s) = (u32_at(b, p)?, u32_at(b, p + 4)? as usize);
        if id == 0xffff_ffff {
            let mut n = put(t, r, lt, p, 8)?;
            let res = (|| -> Result<(), String> {
                let mut f = Fields::new(&b[p..p + 8], &mut n);
                f.u32(0)?;
                f.fill(4, 4, 0, "terminator count")
            })();
            done(t, n, res);
            p += 8;
            break;
        }
        let padded = (s + 3) & !3;
        if p + 8 + padded > end {
            return Err(format!("save record {id} at {p:#x} of {s:#x} bytes runs past the block's end {end:#x}"));
        }
        let mut n = put(t, r, lr, p, 8)?;
        let res = (|| -> Result<(), String> {
            let mut f = Fields::new(&b[p..p + 8], &mut n);
            f.u32(0)?;
            f.u32(4)?;
            Ok(())
        })();
        done(t, n, res);
        let &(_, dest, vsize) = vars.iter().find(|v| v.0 == id).ok_or_else(|| format!("save record id {id:#x} is in no descriptor"))?;
        if vsize as usize != s {
            return Err(format!("save record {id:#x}: {s:#x} bytes, its descriptor says {vsize:#x}"));
        }
        if s > 0 {
            // The value is copied whole into the variable the descriptor (id, RAM address, byte count) names. A variable of 1, 2,
            // 4 or 8 bytes is one scalar; a larger one is the game's structure: the bytes some load reads (VAR_FIELDS, found by
            // scanning every program for loads that land in the variable's RAM range) are its fields.
            let name = format!("save variable {id:#x} ({s} bytes, RAM {dest:#x})");
            if matches!(s, 1 | 2 | 4 | 8) {
                let lv = t.line(G2, &name, "frontwad::save_block", SAVE_FN, Space::Disc);
                let mut n = put(t, r, lv, p + 8, s)?;
                n.mark(0, s as u64);
                t.close(n);
            } else if id == 0x2e || id == 0x12 {
                let lv = t.ignored_line(G2, &name, "searched: no instruction of boot_elf, the 34 story overlays or the two front ends forms an address inside this variable's RAM range (flow-sensitive scan of lui/addiu/ori chains and constant pointer tables; no data word holds one); only the save loader copies the value (frontbin func_00399A00)", Space::Disc);
                let n = put(t, r, lv, p + 8, s)?;
                t.close(n);
            } else if let Some(&(_, fields, game)) = VAR_FIELDS.iter().find(|v| v.0 == id) {
                let lv = t.line(G2, &name, "frontwad::save_block", game, Space::Disc);
                let mut n = put(t, r, lv, p + 8, s)?;
                let mut cov = vec![false; s];
                for &(o, w, st, _) in fields {
                    let (o, w, st) = (o as usize, w as usize, st as usize);
                    if o + w > s {
                        return Err(format!("save variable {id:#x}: field +{o:#x}+{w:#x} past its {s:#x} bytes"));
                    }
                    let mut at = o;
                    loop {
                        for k in at..(at + w).min(s) {
                            cov[k] = true;
                        }
                        if st == 0 || at + st >= s {
                            break;
                        }
                        at += st;
                    }
                }
                let mut k = 0;
                while k < s {
                    let j = (k..s).find(|&x| cov[x] != cov[k]).unwrap_or(s);
                    if cov[k] {
                        n.mark(k as u64, (j - k) as u64);
                    } else {
                        n.skip(k as u64, (j - k) as u64);
                    }
                    k = j;
                }
                t.close(n);
            } else if id == 0x2d || id == 0xb {
                let lv = t.line(G2, &name, "frontwad::save_block", "searched boot_elf, the 34 story overlays and both front ends: the variable's address is formed only to zero or store into it (frontbin func_003B3558 and its level copies); no load reads any byte", Space::Disc);
                let mut n = put(t, r, lv, p + 8, s)?;
                n.skip(0, s as u64);
                t.close(n);
            } else {
                let lv = t.line(G2, &name, "", SAVE_FN, Space::Disc);
                let n = put(t, r, lv, p + 8, s)?;
                t.close(n);
            }
        }
        if padded > s {
            let mut n = put(t, r, lp, p + 8 + s, padded - s)?;
            let res = Fields::new(&b[p + 8 + s..p + 8 + padded], &mut n).fill(0, padded - s, 0, "record padding");
            done(t, n, res);
        }
        p += 8 + padded;
    }
    if p != end {
        return Err(format!("save block at {at:#x}: terminator ends at {p:#x}, the block at {end:#x}"));
    }
    Ok(())
}

// ---- WAD2 pair 7: the front end pack ----


const PACK_FN: &str = "frontbin func_0037D200 and front end func_00381280 (the language picks streams 0-4: 0 and 1 the first; words 10-20 are decremented by 0x10 after the load)";

const TEXT_GROUP: &str = "WAD2 text (unpacked)";

/// The unpacked text: a table of 16-byte records and the strings after it.
fn text_table(t: &mut Tracker, root: &mut Node, d: &[u8]) -> Result<(), String> {
    let n = (u32_at(d, 0)? >> 4) as usize;
    let lr = t.line(
        TEXT_GROUP,
        "text record (+0 string offset, +4 string id, +0xC short; +8, +0xA and +0xE unread)",
        "frontwad::text_table",
        "frontbin func_003B0DA0 (count from the first offset, offsets relocated), func_0037DF28 (id), func_0037DF98 (string), func_0037DFD8 (+0xC)",
        Space::Unpacked,
    );
    let ls = t.line(TEXT_GROUP, "strings (one NUL-terminated string per record, back to back)", "frontwad::text_table", "frontbin func_0037DF98 returns each string to the text drawing", Space::Unpacked);
    if 16 * n > d.len() {
        return Err(format!("text table of {n} records past the block's {:#x} bytes", d.len()));
    }
    for i in 0..n {
        let mut c = put(t, root, lr, 16 * i, 16)?;
        let res = (|| -> Result<(), String> {
            let mut f = Fields::new(&d[16 * i..16 * i + 16], &mut c);
            f.u32(0)?;
            f.u32(4)?;
            f.mark(12, 2);
            f.n.skip(8, 4);
            f.n.skip(14, 2);
            Ok(())
        })();
        done(t, c, res);
    }
    let mut c = put(t, root, ls, 16 * n, d.len() - 16 * n)?;
    let res = (|| -> Result<(), String> {
        let offs: Vec<usize> = (0..n).map(|i| u32_at(d, 16 * i).map(|v| v as usize)).collect::<Result<_, _>>()?;
        if offs.first() != Some(&(16 * n)) || offs.windows(2).any(|w| w[0] >= w[1]) {
            return Err("string offsets do not start at the table's end and ascend".into());
        }
        for (i, &o) in offs.iter().enumerate() {
            let e = offs.get(i + 1).copied().unwrap_or(d.len());
            let s = &d[o..e];
            if s.last() != Some(&0) || s[..s.len() - 1].contains(&0) {
                return Err(format!("string {i} at {o:#x}..{e:#x} is not one NUL-terminated string"));
            }
        }
        c.mark(0, (d.len() - 16 * n) as u64);
        Ok(())
    })();
    done(t, c, res);
    Ok(())
}

/// The front end resource directory (stream 5 of the pack): a 0xB4-byte header, then four tables.
struct FrontDir {
    w: Vec<usize>,
    /// Table 3 (8-byte records at +0xC: u32 offset of a 0x400-byte palette in a stream's data with the top bit set, zero word).
    t3: Vec<usize>,
    /// Table 4 (8-byte records at +0x10: u32 offset of an image with the top bit set, u16 the GS address the loader writes, u8
    /// log2 width, u8 log2 height; the image has 1 << (log2 width + log2 height) bytes).
    t4: Vec<(usize, usize)>,
}

fn front_dir(seg: &[u8]) -> Result<FrontDir, String> {
    if seg.len() < 0xb4 {
        return Err(format!("resource directory of {:#x} bytes", seg.len()));
    }
    let w: Vec<usize> = (0..45).map(|k| u32_at(seg, 4 * k).map(|v| v as usize)).collect::<Result<_, _>>()?;
    let (c1, c2, n3, n4) = (w[0] & 0xffff, w[0] >> 16, w[9], w[17]);
    if w[1] != 0xb4 || w[2] != w[1] + 8 * c1 || w[3] != w[2] + 4 * c2 || w[4] != w[3] + 8 * n3 || seg.len() != w[4] + 8 * n4 {
        return Err(format!("resource directory tables at {:#x} {:#x} {:#x} {:#x} for counts {c1}, {c2}, {n3}, {n4} in {:#x} bytes", w[1], w[2], w[3], w[4], seg.len()));
    }
    if (5..9).any(|k| w[k] > w[k + 1]) || (13..17).any(|k| w[k] > w[k + 1]) || w[9] != n3 || w[17] != n4 {
        return Err("resource directory: the per-stream record counts at +0x14 and +0x34 do not ascend to the table sizes".into());
    }
    let t3 = (0..n3).map(|i| u32_at(seg, w[3] + 8 * i).map(|v| (v & 0x7fff_ffff) as usize)).collect::<Result<_, _>>()?;
    let t4 = (0..n4)
        .map(|i| u32_at(seg, w[4] + 8 * i).map(|v| ((v & 0x7fff_ffff) as usize, 1usize << (seg[w[4] + 8 * i + 6] as usize + seg[w[4] + 8 * i + 7] as usize))))
        .collect::<Result<_, _>>()?;
    Ok(FrontDir { w, t3, t4 })
}

fn wad2_front(t: &mut Tracker, root: &mut Node, at: u64, b: &[u8]) -> Result<(), String> {
    let mut r = t.region(root, G2, at, b.len() as u64);
    let w: Vec<usize> = (0..32).map(|i| u32_at(b, 4 * i).map(|v| v as usize)).collect::<Result<_, _>>()?;
    let ents: Vec<(usize, usize)> = (0..13).map(|i| (w[2 * i], w[2 * i + 1])).collect();
    if ents[0].0 != 0x80 || ents.iter().any(|&(o, s)| o % 0x40 != 0 || o + s > b.len()) || ents.windows(2).any(|e| e[0].0 + e[0].1 > e[1].0) {
        return Err(format!("front end pack header {w:x?}: streams out of order or past the entry"));
    }
    let l = t.line(
        G2,
        "front end pack header (16 (offset, size) pairs: 13 streams, then three empty pairs)",
        "frontwad::wad2_front",
        PACK_FN,
        Space::Disc,
    );
    let mut n = put(t, &mut r, l, 0, 0x80)?;
    let res = (|| -> Result<(), String> {
        let mut f = Fields::new(&b[..0x80], &mut n);
        for (i, &(o, s)) in ents.iter().enumerate() {
            f.u32(8 * i)?;
            if (5..=10).contains(&i) {
                // The sizes of the raw directory and of streams 6-10: func_00394060 rounds them up to 0x40.
                f.u32(8 * i + 4)?;
            } else {
                // The LZ streams carry their own size; no code reads this copy.
                if &b[o..o + 3] != b"WAD" {
                    return Err(format!("stream {i} at {o:#x} is not an LZ stream"));
                }
                f.u32_eq(8 * i + 4, u32_at(b, o + 3)?, &format!("stream {i} size"))?;
                if u32_at(b, o + 3)? as usize != s {
                    return Err(format!("stream {i}: pair size {s:#x}, stream size {:#x}", u32_at(b, o + 3)?));
                }
            }
        }
        f.fill(8 * 13, 0x18, 0, "empty pairs")
    })();
    done(t, n, res);
    for (i, &(o, s)) in ents.iter().enumerate() {
        let seg = &b[o..o + s];
        match i {
            0..=4 => {
                let l = t.line(G2, "text table: LZ stream", "lz::unpack", "frontbin func_0039B760 unpacks the language's stream (func_0037D200, func_003B0DA0)", Space::Disc);
                crate::walk::lz_block(t, &mut r, l, seg, o as u64, s as u64, TEXT_GROUP, |t, root, d| text_table(t, root, d))?;
            }
            5 => {
                let fd = front_dir(seg)?;
                let l = t.line(
                    G2,
                    "front end resource directory header (+4, +8, +0xC and +0x10 table offsets, +0x14..+0x24 and +0x34..+0x44 the per-stream record counts, +0x54..+0x64 the unpacked sizes of streams 6-10; +0 the two table counts, no reader (skipped))",
                    "frontwad::wad2_front",
                    "frontbin func_00394060 copies it and reads +4, +8, +0xC, +0x10 and +0x54..+0x64; func_0038E798 and func_0038E930 read the counts at +0x14..+0x24 and +0x34..+0x44 (the range of table records each stream owns); the other words are zero",
                    Space::Disc,
                );
                let mut c = put(t, &mut r, l, o, 0xb4)?;
                let res = (|| -> Result<(), String> {
                    let mut f = Fields::new(&seg[..0xb4], &mut c);
                    // +0 holds the two table counts (u16 each); no code of the directory's users reads it (func_00394060 copies the
                    // directory and addresses +4.., func_0038E798, func_0038E930, func_00394300 and the front ends' table readers
                    // use +4..+0x64 and +0x74..): the decoder takes the counts from it and skips it.
                    f.skip(0, 4);
                    for k in [4usize, 8, 0xc, 0x10] {
                        f.u32(k)?;
                    }
                    for k in (0x14..0x28).step_by(4).chain((0x34..0x48).step_by(4)).chain((0x54..0x68).step_by(4)) {
                        f.u32(k)?;
                    }
                    f.fill(0x28, 0xc, 0, "zero words between the counts")?;
                    f.fill(0x48, 0xc, 0, "zero words between the counts")?;
                    f.fill(0x68, 0xb4 - 0x68, 0, "zero words after the sizes")
                })();
                done(t, c, res);
                let w = &fd.w;
                let l = t.line(
                    G2,
                    "front end resource directory table 1 (8-byte records: +0 u16 id, +2 u16 frame count, +4 u16 first record of table 2, +6 u8, +7 u8 ticks per frame; the last record's id is 0xFFFF)",
                    "frontwad::wad2_front",
                    "frontbin func_0038E730 (finds a record by its id: +0, up to the 0xFFFF one), func_0038ED00 (+0, +4, +6), func_00391B70 (+0, +2, +4, +7: the frame is the record's first + (tick / +7) % count), func_00392108 (+0, +2, +4)",
                    Space::Disc,
                );
                let (c1, c2) = (w[0] & 0xffff, w[0] >> 16);
                let mut c = put(t, &mut r, l, o + w[1], 8 * c1)?;
                let res = (|| -> Result<(), String> {
                    let tb = &seg[w[1]..w[1] + 8 * c1];
                    for i in 0..c1 {
                        let (id, cnt, first) = (u16_at(tb, 8 * i)? as usize, u16_at(tb, 8 * i + 2)? as usize, u16_at(tb, 8 * i + 4)? as usize);
                        if (id == 0xffff) != (i + 1 == c1) || first + cnt > c2 {
                            return Err(format!("table 1 record {i}: id {id:#x}, {cnt} frames from {first} of {c2} table 2 records"));
                        }
                        c.mark(8 * i as u64, 8);
                    }
                    Ok(())
                })();
                done(t, c, res);
                let l = t.line(
                    G2,
                    "front end resource directory table 2 (4-byte records: +0 s16 palette (a table 3 record), +2 s16 image (a table 4 record))",
                    "frontwad::wad2_front",
                    "frontbin func_00392108 (+0 and +2 index tables 3 and 4 for the record's frame), func_003921D8, func_00392400, func_003925F0 and func_00392A40 (table 3 record = +0 * 8, table 4 record = +2 * 8)",
                    Space::Disc,
                );
                let mut c = put(t, &mut r, l, o + w[2], 4 * c2)?;
                let res = (|| -> Result<(), String> {
                    let tb = &seg[w[2]..w[2] + 4 * c2];
                    for i in 0..c2 {
                        let (pal, img) = (u16_at(tb, 4 * i)? as i16, u16_at(tb, 4 * i + 2)? as i16);
                        if pal < 0 || pal as usize >= w[9] || img < 0 || img as usize >= w[17] {
                            return Err(format!("table 2 record {i}: palette {pal}, image {img} of {} and {} records", w[9], w[17]));
                        }
                        c.mark(4 * i as u64, 4);
                    }
                    Ok(())
                })();
                done(t, c, res);
                let l = t.line(
                    G2,
                    "front end resource directory table 3 (8-byte records: +0 u32 palette offset with the top bit set, +4 zero)",
                    "frontwad::wad2_front",
                    "frontbin func_0038E798 (adds the stream's base to +0 and clears the top bit; the records of stream p are those from the count at +0x14 + 4p - 4 to the one at +0x14 + 4p)",
                    Space::Disc,
                );
                let mut c = put(t, &mut r, l, o + w[3], 8 * w[9])?;
                let res = (|| -> Result<(), String> {
                    let mut f = Fields::new(&seg[w[3]..w[3] + 8 * w[9]], &mut c);
                    for i in 0..w[9] {
                        if f.u32(8 * i)? >> 31 != 1 {
                            return Err(format!("table 3 record {i}: the top bit of the offset is clear"));
                        }
                        f.fill(8 * i + 4, 4, 0, "zero word")?;
                    }
                    Ok(())
                })();
                done(t, c, res);
                let l = t.line(
                    G2,
                    "front end resource directory table 4 (8-byte records: +0 u32 image offset with the top bit set, +4 u16 zero (the loader writes the GS address there), +6 u8 log2 width, +7 u8 log2 height)",
                    "frontwad::wad2_front",
                    "frontbin func_0038E798 (adds the stream's base to +0 and clears the top bit), func_0038E930 (reads +0, +6 and +7: func_00393120 uploads the image to the GS and writes the address into +4)",
                    Space::Disc,
                );
                let mut c = put(t, &mut r, l, o + w[4], 8 * w[17])?;
                let res = (|| -> Result<(), String> {
                    let mut f = Fields::new(&seg[w[4]..w[4] + 8 * w[17]], &mut c);
                    for i in 0..w[17] {
                        if f.u32(8 * i)? >> 31 != 1 {
                            return Err(format!("table 4 record {i}: the top bit of the offset is clear"));
                        }
                        f.fill(8 * i + 4, 2, 0, "zero half word")?;
                        f.mark(8 * i + 6, 1);
                        f.mark(8 * i + 7, 1);
                    }
                    Ok(())
                })();
                done(t, c, res);
            }
            6 | 7 => {
                let fd = front_dir(&b[ents[5].0..ents[5].0 + ents[5].1])?;
                let st = i - 6;
                let exp = fd.w[0x15 + st];
                let (lo3, hi3) = (if st == 0 { 0 } else { fd.w[4 + st] }, fd.w[5 + st]);
                let (lo4, hi4) = (if st == 0 { 0 } else { fd.w[12 + st] }, fd.w[13 + st]);
                let pals: Vec<usize> = fd.t3[lo3..hi3].to_vec();
                let imgs: Vec<(usize, usize)> = fd.t4[lo4..hi4].to_vec();
                let name = if st == 0 { "front end stream 0: LZ stream" } else { "front end stream 1: LZ stream" };
                let l = t.line(G2, name, "lz::unpack", "frontbin func_0039B760 (through func_00394300)", Space::Disc);
                crate::walk::lz_block(t, &mut r, l, seg, o as u64, s as u64, "WAD2 front end stream (unpacked)", move |t, root, d| {
                    if d.len() != exp {
                        return Err(format!("front end stream unpacks to {:#x} bytes, its directory says {exp:#x}", d.len()));
                    }
                    let lp = t.line(
                        "WAD2 front end stream (unpacked)",
                        "front end palette (256 colours, RGBA)",
                        "frontwad::wad2_front",
                        "front end data of the directory's table 3: frontbin func_0038E798 relocates each pointer; the readers of table 3 (func_00392108, func_003921D8, func_00384B68) hand the palettes to the GS",
                        Space::Unpacked,
                    );
                    let li = t.line(
                        "WAD2 front end stream (unpacked)",
                        "front end 8-bit image (palette indices, 1 << (log2 width + log2 height) bytes)",
                        "frontwad::wad2_front",
                        "frontbin func_0038E930 (func_00393120 uploads each image of table 4 to the GS, from the relocated +0 with the log2 sizes at +6 and +7)",
                        Space::Unpacked,
                    );
                    for &o in &pals {
                        let mut c = put(t, root, lp, o, 0x400)?;
                        c.mark(0, 0x400);
                        t.close(c);
                    }
                    for &(o, n) in &imgs {
                        let mut c = put(t, root, li, o, n)?;
                        c.mark(0, n as u64);
                        t.close(c);
                    }
                    let total: usize = pals.len() * 0x400 + imgs.iter().map(|x| x.1).sum::<usize>();
                    if total != d.len() {
                        return Err(format!("the directory's palettes and images cover {total:#x} of the stream's {:#x} bytes", d.len()));
                    }
                    Ok(())
                })?;
            }
            8..=10 => {
                // An LZ stream of the header alone: the directory gives its unpacked size as zero, so no code unpacks it.
                let l = t.line(G2, "empty LZ stream (header only: the resource directory gives its unpacked size as zero)", "frontwad::wad2_front", PACK_FN, Space::Disc);
                let mut c = put(t, &mut r, l, o, s)?;
                let res = (|| -> Result<(), String> {
                    if s != 0x10 {
                        return Err(format!("stream {i} of {s:#x} bytes"));
                    }
                    let mut f = Fields::new(seg, &mut c);
                    f.same(0, b"WAD", "tag")?;
                    f.u32_eq(3, 0x10, "stream size")
                })();
                crate::walk::lz_header(t, &mut c);
                done(t, c, res);
            }
            11 => {
                let l = t.line(G2, "background image: LZ stream", "lz::unpack", "frontbin func_0039B760 (func_0037D200)", Space::Disc);
                crate::walk::lz_block(t, &mut r, l, seg, o as u64, s as u64, "WAD2 background image (unpacked)", |t, root, d| {
                    const PIXELS: usize = 0x200 * 0x1a0 * 4;
                    if d.len() != 0x10 + PIXELS {
                        return Err(format!("background image unpacks to {:#x} bytes", d.len()));
                    }
                    let lh = t.line("WAD2 background image (unpacked)", "background image header (+0 width, +4 height, 8 zero bytes)", "frontwad::wad2_front", "frontbin func_003A42E0 is called with the width 0x200 and height 0x1A0 and reads from +0x10; nothing reads the header", Space::Unpacked);
                    let mut c = put(t, root, lh, 0, 0x10)?;
                    let res = (|| -> Result<(), String> {
                        let mut f = Fields::new(&d[..0x10], &mut c);
                        f.u32_eq(0, 0x200, "width")?;
                        f.u32_eq(4, 0x1a0, "height")?;
                        f.fill(8, 8, 0, "zero")
                    })();
                    done(t, c, res);
                    let lp = t.line("WAD2 background image (unpacked)", "background image pixels (0x200 x 0x1A0 RGBA, alpha 0..0x80)", "frontwad::wad2_front", "frontbin func_003A42E0 (called by func_0037D200 with the image at +0x10)", Space::Unpacked);
                    let mut c = put(t, root, lp, 0x10, PIXELS)?;
                    let res = (|| -> Result<(), String> {
                        if let Some(k) = (0..PIXELS / 4).find(|&k| d[0x10 + 4 * k + 3] > 0x80) {
                            return Err(format!("pixel {k}: alpha {:#04x}", d[0x10 + 4 * k + 3]));
                        }
                        c.mark(0, PIXELS as u64);
                        Ok(())
                    })();
                    done(t, c, res);
                    Ok(())
                })?;
            }
            _ => {
                let l = t.line(G2, "sound bank: LZ stream", "lz::unpack", "frontbin func_0039B760 (func_0037D200), then func_0039C1C8 sends it to the IOP", Space::Disc);
                crate::walk::lz_block(t, &mut r, l, seg, o as u64, s as u64, "WAD2 sound bank (unpacked)", |t, root, d| {
                    if d.len() < 0x1c || u32_at(d, 0)? != 3 || u32_at(d, 4)? != 2 || &d[0x18..0x1c] != b"SBlk" {
                        return Err("the bank wrapper is not type 3, version 2 with an SBlk part at +0x18".into());
                    }
                    // A bank of the sbank.rs form, sent from EE memory (989snd FUN_0000898c).
                    let (o10, sz) = (u32_at(d, 0x10)? as usize, u32_at(d, 0x14)? as usize);
                    if o10 + sz > d.len() {
                        return Err(format!("sound bank wrapper: sample data to {:#x}, the block has {:#x} bytes", o10 + sz, d.len()));
                    }
                    crate::sbank::bank_in(t, root, "WAD2 sound bank (unpacked)", 0, &d[..o10 + sz], "frontbin func_0039C1C8 (func_0037D200 passes the unpacked stream) hands the bank in EE memory to func_0013BC30, which sends it to the IOP (RPC 0x57); its 989snd module parses it in FUN_0000898c", Space::Unpacked)?;
                    crate::walk::pad_mem(t, root, d, "WAD2 sound bank (unpacked)");
                    Ok(())
                })?;
            }
        }
    }
    crate::walk::pad_mem(t, &mut r, b, G2);
    t.close(r);
    Ok(())
}

// ---- WAD4 ----

/// Pairs 0-5: a header sector of (offset, size) pairs and the LZ streams, each a SCENE chunk.
fn wad4_chunks(t: &mut Tracker, root: &mut Node, at: u64, b: &[u8]) -> Result<(), String> {
    let mut r = t.region(root, G4, at, b.len() as u64);
    let mut pairs = Vec::new();
    for i in 0..0x44 {
        let (o, s) = (u32_at(b, 8 * i)? as usize, u32_at(b, 8 * i + 4)? as usize);
        if s == 0 {
            break;
        }
        pairs.push((o, s));
    }
    if pairs.is_empty() || pairs.iter().any(|&(o, s)| o % 0x800 != 0 || s % 0x800 != 0 || 0x800 + o + s > b.len()) {
        return Err(format!("scene chunk block header: pairs {pairs:x?}"));
    }
    let l = t.line(
        G4,
        "scene chunk block header ((offset, size) pairs of the LZ streams, then zero to 0x800)",
        "frontwad::wad4_chunks",
        "func_L01_00429998 (same in all 34 story levels): each offset (+0x800 is the stream); every size is tested for zero, which ends the list; its callers pass pair 0 or 1",
        Space::Disc,
    );
    let mut n = put(t, &mut r, l, 0, 0x800)?;
    let res = (|| -> Result<(), String> {
        let mut f = Fields::new(&b[..0x800], &mut n);
        for k in 0..pairs.len() {
            f.u32(8 * k)?;
            f.u32(8 * k + 4)?;
        }
        f.fill(8 * pairs.len(), 0x800 - 8 * pairs.len(), 0, "after the pairs")
    })();
    done(t, n, res);
    for &(o, s) in &pairs {
        let l = t.line(G4, "scene chunk: LZ stream", "lz::unpack", "func_L01_00433EA0 (each program's copy)", Space::Disc);
        crate::walk::lz_block(t, &mut r, l, &b[0x800 + o..0x800 + o + s], (0x800 + o) as u64, s as u64, "WAD4 scene chunk (unpacked)", |t, root, d| crate::scene::chunk(t, root, d))?;
    }
    crate::walk::pad_mem(t, &mut r, b, G4);
    t.close(r);
    Ok(())
}

const SCREEN_GROUP: &str = "WAD4 front end screen (unpacked)";
const BUNDLE_GROUP: &str = "WAD4 front end screen bundle stream (unpacked)";
const PORTRAIT_GROUP: &str = "WAD4 front end screen image (unpacked)";


/// Pairs 6-14: one LZ stream, a front end screen: a 0xC4-byte header, tables, GS images, and a second region (from the base
/// the header's +8 gives) of data, sprite images, bundles of LZ streams, sound banks and model objects.
fn wad4_screen(t: &mut Tracker, root: &mut Node, at: u64, b: &[u8]) -> Result<(), String> {
    let mut r = t.region(root, G4, at, b.len() as u64);
    let l = t.line(G4, "front end screen: LZ stream", "lz::unpack", "front end func_003A0A20 unpacks it (func_003BBCE8); frontbin func_003B6528 loads the same pairs", Space::Disc);
    crate::walk::lz_block(t, &mut r, l, b, 0, b.len() as u64, SCREEN_GROUP, |t, root, d| screen_body(t, root, d))?;
    crate::walk::pad_mem(t, &mut r, b, G4);
    t.close(r);
    Ok(())
}

/// A table of `n` records of `size` bytes at `at`, each record's `reads` (offset, length) marked, its `skips` (no reader) skipped
/// and the rest left unread.
fn table(t: &mut Tracker, root: &mut Node, line: usize, at: usize, n: usize, size: usize, d: &[u8], reads: &[(usize, usize)], skips: &[(usize, usize)]) -> Result<(), String> {
    let mut c = put(t, root, line, at, n * size)?;
    for i in 0..n {
        let _ = &d[at + i * size..at + (i + 1) * size];
        for &(o, l) in reads {
            c.mark((i * size + o) as u64, l as u64);
        }
        for &(o, l) in skips {
            c.skip((i * size + o) as u64, l as u64);
        }
    }
    t.close(c);
    Ok(())
}

fn screen_body(t: &mut Tracker, root: &mut Node, d: &[u8]) -> Result<(), String> {
    if d.len() < 0xc4 {
        return Err(format!("front end screen of {:#x} bytes", d.len()));
    }
    let w: Vec<usize> = (0..49).map(|k| u32_at(d, 4 * k).map(|v| v as usize)).collect::<Result<_, _>>()?;
    let (fl, base) = (w[0], w[2]);
    if fl & !0x3f != 0 || w[5] != 0xc4 || w[3] > d.len() || base > d.len() || w[1] > base {
        return Err(format!("screen header: flags {fl:#x}, +4 {:#x}, +8 {base:#x}, +0xC {:#x}, +0x14 {:#x}", w[1], w[3], w[5]));
    }
    // ---- the header
    let lh = t.line(
        SCREEN_GROUP,
        "screen header (0xC4 bytes: +0 flags, table and section offsets and counts)",
        "frontwad::screen_body",
        "front end func_003BBCE8 (frontbin func_003B6528): +0 flags; +4 image base, +0x10 and +0x14 the GS image table (func_00399868); +8 the second region's base; +0xC the offset func_003BC868 adds the LZ stream list to; +0x18, +0x1C and +0x20 (flag 2), +0x24 (flag 1), +0x28, +0x30, +0x38 and +0x3C (flag 4), +0x2C and +0x34 (flags 4 or 8), +0x68 (flag 8), +0x40, +0x44, +0x48 and +0x60 (flag 0x10), +0x4C, +0x50, +0x54, +0x58, +0x5C and +0x64 (every screen), +0x94 and +0x98 (the record and its word); front end func_003BC978 reads +0x6C..+0x88 (bundle offsets), func_003BC6E8 +0x9C..+0xB8 (sound bank offsets); a word of a flag that is not set is zero or, in screens 12-14, a value nothing reads (+0x3C, +0x40 and +0x48: skipped: func_003BBCE8 loads them only under flags 4 and 0x10), and +0x8C, +0x90, +0xBC, +0xC0 are zero",
        Space::Unpacked,
    );
    let mut c = put(t, root, lh, 0, 0xc4)?;
    let res = (|| -> Result<(), String> {
        let mut f = Fields::new(&d[..0xc4], &mut c);
        for k in [0usize, 1, 2, 3, 4, 5, 0x13, 0x14, 0x15, 0x16, 0x17, 0x19, 0x25, 0x26].into_iter().chain(0x1b..=0x22).chain(0x27..=0x2e) {
            f.u32(4 * k)?;
        }
        for (mask, ws) in [(1usize, &[9usize][..]), (2, &[6, 7, 8][..]), (4, &[0xa, 0xc, 0xe, 0xf][..]), (0xc, &[0xb, 0xd][..]), (8, &[0x1a][..]), (0x10, &[0x10, 0x11, 0x12, 0x18][..])] {
            for &k in ws {
                if fl & mask != 0 {
                    f.u32(4 * k)?;
                } else if w[k] == 0 {
                    f.fill(4 * k, 4, 0, "word of a flag that is not set")?;
                } else {
                    // Read only under its flag (func_003BBCE8): the value is the builder's, unused.
                    f.skip(4 * k, 4);
                }
            }
        }
        for k in [0x23usize, 0x24, 0x2f, 0x30] {
            f.fill(4 * k, 4, 0, "zero word")?;
        }
        Ok(())
    })();
    done(t, c, res);
    // ---- the GS image table and the images it lists (func_00399868)
    let nt = w[4];
    let lt = t.line(
        SCREEN_GROUP,
        "screen GS image table (16-byte records: +0 pixel format, +4 width and height (u16 each), +8 no reader (skipped), +0xC data offset)",
        "frontwad::screen_body",
        "front end func_00399868 (frontbin func_003947B0): +0 the format (0x13: width x height palette indices; 2: 16 x 16 PSMCT16; 0: a 16 x 16 PSMCT32 palette), +4 the width and height of the GS transfer, +0xC the data's offset from the image base; +8 repeats +0xC and no reader reads it",
        Space::Unpacked,
    );
    let mut c = put(t, root, lt, w[5], 16 * nt)?;
    for i in 0..nt {
        c.mark((16 * i) as u64, 4);
        c.mark((16 * i + 4) as u64, 4);
        c.mark((16 * i + 0xc) as u64, 4);
        c.skip((16 * i + 8) as u64, 4);
    }
    t.close(c);
    let l8 = t.line(SCREEN_GROUP, "screen GS image: palette indices (width x height, PSMT8)", "frontwad::screen_body", "front end func_00399868 (the GS transfer func_0012CCC8 reads width x height bytes at the image base + the record's +0xC)", Space::Unpacked);
    let l0 = t.line(SCREEN_GROUP, "screen GS image: 16 x 16 palette (PSMCT32, 0x400 bytes)", "frontwad::screen_body", "front end func_00399868 (the GS transfer reads 0x400 bytes)", Space::Unpacked);
    let l2 = t.line(SCREEN_GROUP, "screen GS image: 16 x 16 block (PSMCT16, 0x200 bytes)", "frontwad::screen_body", "front end func_00399868 (the GS transfer reads 0x200 bytes)", Space::Unpacked);
    let mut pos = w[1];
    for i in 0..nt {
        let (ty, dim, off) = (u32_at(d, w[5] + 16 * i)? as usize, u32_at(d, w[5] + 16 * i + 4)? as usize, u32_at(d, w[5] + 16 * i + 12)? as usize);
        if off != pos - w[1] {
            return Err(format!("GS image {i}: data offset {off:#x}, the previous image ends at {:#x}", pos - w[1]));
        }
        let (line, size, slot) = match ty {
            0x13 => (l8, (dim & 0xffff) * (dim >> 16), ((dim & 0xffff) * (dim >> 16)).max(0x100)),
            0 if dim == 0 => (l0, 0x400, 0x400),
            2 if dim == 0 => (l2, 0x200, 0x200),
            _ => return Err(format!("GS image {i}: format {ty:#x}, size word {dim:#x}")),
        };
        let mut c = put(t, root, line, pos, size)?;
        c.mark(0, size as u64);
        t.close(c);
        pos += slot;
    }
    if pos != base {
        return Err(format!("the GS images end at {pos:#x}, the second region starts at {base:#x}"));
    }
    // ---- the tables the flags enable
    // The pool's images: (offset, width, mip levels) from the region table and the texture list.
    let mut pool_ents: Vec<(usize, usize, i16)> = Vec::new();
    if fl & 2 != 0 {
        for i in 0..w[7] {
            pool_ents.push((u32_at(d, w[8] + 16 * i)? as usize, u16_at(d, w[8] + 16 * i + 4)? as usize, u16_at(d, w[8] + 16 * i + 8)? as i16));
        }
    }
    if fl & 4 != 0 {
        for i in 0..w[14] {
            pool_ents.push((u32_at(d, w[15] + 16 * i)? as usize, u16_at(d, w[15] + 16 * i + 4)? as usize, u16_at(d, w[15] + 16 * i + 8)? as i16));
        }
    }
    if fl & 2 != 0 {
        let l = t.line(
            SCREEN_GROUP,
            "screen region table (flag 2: 16-byte records: +0 u32 data offset, +4 u16 width, +6 u16 height, +8, +0xA, +0xC, +0xE s16)",
            "frontwad::screen_body",
            "front end func_003BBCE8 (+0 offset and +4 width into the table at 0x2FD280) and func_0039A148 (+4, +6, +8, +0xA, +0xC and +0xE build the GS texture words)",
            Space::Unpacked,
        );
        table(t, root, l, w[8], w[7], 16, d, &[(0, 4), (4, 2), (6, 2), (8, 2), (10, 2), (12, 2), (14, 2)], &[])?;
    }
    if fl & 4 != 0 {
        let l = t.line(
            SCREEN_GROUP,
            "screen texture list (flag 4: 16-byte records: +0 u32 pool offset, +4 u16 width, +0xA s16; +6, +8, +0xC and +0xE no reader (skipped))",
            "frontwad::screen_body",
            "front end func_003BBCE8 (+0 and +4 into the table at 0x2DD4B0) and func_00399D10 (+0xA); no other code addresses the list (func_0039A030 only passes it on to func_00399D10): +6, +8, +0xC and +0xE have no reader",
            Space::Unpacked,
        );
        table(t, root, l, w[15], w[14], 16, d, &[(0, 4), (4, 2), (10, 2)], &[(6, 2), (8, 2), (12, 2), (14, 2)])?;
        let l = t.line(
            SCREEN_GROUP,
            "screen object table (flag 4: 32-byte records: +0 u32 object offset, +4 u32 object id, +8 u32 form, +0x10..+0x17 eight texture list numbers; +0xC and +0x18..+0x1F no reader (skipped))",
            "frontwad::screen_body",
            "front end func_003BBCE8 (+0, +4 and +8 choose func_0039A030's path; +0x10 is passed on as the texture numbers) and func_00399D10 (copies +0x10..+0x17); +0xC and +0x18..+0x1F are read by neither",
            Space::Unpacked,
        );
        table(t, root, l, w[12], w[10], 32, d, &[(0, 4), (4, 4), (8, 4), (0x10, 8)], &[(0xc, 4), (0x18, 8)])?;
    }
    if fl & 0xc != 0 {
        let l = t.line(
            SCREEN_GROUP,
            "screen placement table (flags 4 or 8: 40-byte records: +0 u32 model id, +4..+0x18 position, +0x1C scale (f32 each); +0x10..+0x18 rotation; +0x20 and +0x24 no reader (skipped))",
            "frontwad::screen_body",
            "front end func_003BBCE8 (copies +0 into func_003C2898, +4..+0x18 and +0x10..+0x18 into the object's position and angles, scales +0x1C: +0x20 and +0x24 are not loaded)",
            Space::Unpacked,
        );
        table(t, root, l, w[13], w[11], 40, d, &[(0, 4), (4, 4), (8, 4), (12, 4), (16, 4), (20, 4), (24, 4), (28, 4)], &[(0x20, 8)])?;
    }
    let mut dblk: Vec<(usize, usize, usize, usize)> = Vec::new();
    if fl & 0x10 != 0 {
        let l = t.line(
            SCREEN_GROUP,
            "screen data block table (flag 0x10: 16-byte records of four u32)",
            "frontwad::screen_body",
            "front end func_00398760 (reads all four words of each record)",
            Space::Unpacked,
        );
        table(t, root, l, w[18], w[17], 16, d, &[(0, 4), (4, 4), (8, 4), (12, 4)], &[])?;
        for i in 0..w[17] {
            let r = (u32_at(d, w[18] + 16 * i)? as usize, u32_at(d, w[18] + 16 * i + 4)? as usize, u32_at(d, w[18] + 16 * i + 8)? as usize, u32_at(d, w[18] + 16 * i + 12)? as usize);
            dblk.push(r);
        }
        let o = w[16];
        let (cnt, off2, sz) = (u32_at(d, o)? as usize, u32_at(d, o + 8)? as usize, u32_at(d, o + 12)? as usize);
        if 0x10 + 4 * cnt > off2 || o + off2 + sz > base {
            return Err(format!("data block at {o:#x}: {cnt} pointers, data at {off2:#x} of {sz:#x} bytes"));
        }
        let l = t.line(
            SCREEN_GROUP,
            "screen data block header (+0 pointer count, +4 no reader (skipped), +8 data offset, +0xC data size)",
            "frontwad::screen_body",
            "front end func_00398760 (reads +0, +8 and +0xC; +4 is not loaded)",
            Space::Unpacked,
        );
        let mut c = put(t, root, l, o, 0x10)?;
        for a in [0u64, 8, 12] {
            c.mark(a, 4);
        }
        c.skip(4, 4);
        t.close(c);
        let l = t.line(SCREEN_GROUP, "screen data block pointers (u32 each)", "frontwad::screen_body", "front end func_00398760 (turns each into a pointer into the copied data)", Space::Unpacked);
        table(t, root, l, o + 0x10, cnt, 4, d, &[(0, 4)], &[])?;
        let l = t.line(
            SCREEN_GROUP,
            "screen data block data (bytes copied by func_0038CFB0 to 0x2FD170; the pointers func_00398760 builds at 0x2FCF18 address them)",
            "frontwad::screen_body",
            "front end func_003BE920 (reads the byte at the pointer 0x2FCFA0 holds), func_003BFD78 (a byte at the pointer 0x2FD07C holds plus an index), func_003BF410 (a byte at +4 of the pointer 0x2FD084 holds)",
            Space::Unpacked,
        );
        let mut c = put(t, root, l, o + off2, sz)?;
        c.mark(0, sz as u64);
        t.close(c);
    }
    let l = t.line(
        SCREEN_GROUP,
        "screen sprite table (16-byte records: +0 palette offset, +4 pixel offset, +8 width; +0xC no reader (skipped))",
        "frontwad::screen_body",
        "front end func_00398880 (builds the sprite records at 0x226080 from +0, +4 and +8: +8 negative is the 4-bit form); +0xC is not loaded (the height is the sprite's row count)",
        Space::Unpacked,
    );
    table(t, root, l, w[20], w[19], 16, d, &[(0, 4), (4, 4), (8, 4)], &[(12, 4)])?;
    if w[0x25] != 0 {
        let l = t.line(
            SCREEN_GROUP,
            "screen record (ten u32: +0..+0x14 three colours and a word each, +0x18..+0x24 four floats)",
            "frontwad::screen_body",
            "front end func_003BBCE8 (copies the ten words to 0x225BD0.. and 0x1D9CC8..)",
            Space::Unpacked,
        );
        table(t, root, l, w[0x25], 1, 40, d, &[(0, 4), (4, 4), (8, 4), (12, 4), (16, 4), (20, 4), (24, 4), (28, 4), (32, 4), (36, 4)], &[])?;
    }
    // ---- the sprite pack: the second region's pieces
    let sp = base + w[0x19];
    let mut cover = std::collections::BTreeSet::new();
    for i in 0..w[19] {
        let (c, im, ww, hh) = (u32_at(d, w[20] + 16 * i)? as i32, u32_at(d, w[20] + 16 * i + 4)? as i32, u32_at(d, w[20] + 16 * i + 8)? as i32, u32_at(d, w[20] + 16 * i + 12)? as i32);
        if c < 0 || im < 0 || ww <= 0 || hh <= 0 {
            return Err(format!("sprite {i}: palette {c:#x}, pixels {im:#x}, {ww} x {hh}"));
        }
        cover.insert((c as usize, 0x400usize, false));
        cover.insert((im as usize, (ww * hh) as usize, true));
    }
    let lsp = t.line(SCREEN_GROUP, "screen sprite palette (256 colours, RGBA)", "frontwad::screen_body", "front end func_00398880 (the sprite table's +0 offset into the pack at +0x64 + the base); the sprite code sends it to the GS", Space::Unpacked);
    let lsi = t.line(SCREEN_GROUP, "screen sprite pixels (width x height palette indices)", "frontwad::screen_body", "front end func_00398880 (the sprite table's +4 offset into the pack at +0x64 + the base); the sprite code sends them to the GS", Space::Unpacked);
    let mut spos = 0;
    for &(o, n, pix) in &cover {
        if o != spos {
            return Err(format!("sprite pack: data at {o:#x}, the previous ends at {spos:#x}"));
        }
        let mut c = put(t, root, if pix { lsi } else { lsp }, sp + o, n)?;
        c.mark(0, n as u64);
        t.close(c);
        spos = o + n;
    }
    // ---- the second region's sections, in file order
    let mut secs: Vec<(usize, char)> = Vec::new();
    if fl & 2 != 0 {
        secs.push((base + w[6], 'A'));
    }
    if fl & 1 != 0 {
        secs.push((base + w[9], 'B'));
    }
    secs.push((base + w[0x17], 'P'));
    if fl & 0x10 != 0 {
        secs.push((base + w[0x18], 'D'));
    }
    secs.push((sp, 'S'));
    for k in 0..8 {
        if w[0x1b + k] != 0 {
            secs.push((base + w[0x1b + k], 'U'));
        }
    }
    for k in 0..8 {
        if w[0x27 + k] != 0 {
            secs.push((base + w[0x27 + k], 'K'));
        }
    }
    if fl & 4 != 0 {
        let mut os = std::collections::BTreeSet::new();
        for i in 0..w[10] {
            let o = u32_at(d, w[12] + 32 * i)? as usize;
            if o != 0 {
                os.insert(base + o);
            }
        }
        secs.extend(os.into_iter().map(|o| (o, 'O')));
    }
    if w[3] < d.len() {
        secs.push((w[3], 'W'));
    }
    secs.push((d.len(), 'E'));
    secs.sort_by_key(|&(a, k)| (a, k != 'D'));
    if secs.iter().any(|&(a, _)| a > d.len()) {
        return Err("a section starts past the screen's end".into());
    }
    let lbh = t.line(
        SCREEN_GROUP,
        "screen bundle header ((offset, size) pairs of LZ streams, zero to 0x800)",
        "frontwad::screen_body",
        "front end func_003BC978 (reads each pair's offset, and the size of every pair after the first: a zero size ends the list; at most 0x44 pairs); the first pair's size: no reader",
        Space::Unpacked,
    );
    let lbs = t.line(SCREEN_GROUP, "screen bundle: LZ stream", "lz::unpack", "front end func_003BC978 (points 0x225810 + 4k at each stream), func_0039A958 (hands stream k to func_0039A700, which unpacks it with func_003A0A20)", Space::Unpacked);
    let lwh = t.line(
        SCREEN_GROUP,
        "screen late stream list (u32 stream count, 12 zero bytes, the streams' u32 offsets)",
        "frontwad::screen_body",
        "front end func_003BC868 (base + +0xC: the count at +0, the stream's offset at +0x10 + 4 * its number)",
        Space::Unpacked,
    );
    let lws = t.line(SCREEN_GROUP, "screen late stream: LZ stream (a 128 x 128 2FIP image)", "lz::unpack", "front end func_003BC868 (unpacks stream n with func_003A0A20)", Space::Unpacked);
    for i in 0..secs.len() - 1 {
        let (a, k) = secs[i];
        let e = secs[i + 1].0;
        if e == a && k != 'S' {
            continue;
        }
        match k {
            'S' => {
                if secs.iter().find(|&&(x, kk)| x > sp && kk != 'D').map(|x| x.0) != Some(sp + spos) {
                    return Err(format!("sprite pack of {spos:#x} bytes at {sp:#x}: the next section is elsewhere"));
                }
            }
            'U' => {
                let mut pairs = Vec::new();
                for p in 0..0x44 {
                    let (po, ps) = (u32_at(d, a + 8 * p)? as usize, u32_at(d, a + 8 * p + 4)? as usize);
                    if ps == 0 {
                        break;
                    }
                    pairs.push((po, ps));
                }
                if pairs.is_empty() || 0x800 + pairs.iter().map(|x| x.1).sum::<usize>() != e - a || a + 8 * pairs.len() + 8 > e {
                    return Err(format!("bundle at {a:#x}: pairs {pairs:x?} do not fill {:#x} bytes", e - a));
                }
                let mut c = put(t, root, lbh, a, 0x800)?;
                let res = (|| -> Result<(), String> {
                    let mut f = Fields::new(&d[a..a + 0x800], &mut c);
                    for (p, _) in pairs.iter().enumerate() {
                        f.u32(8 * p)?;
                        if p == 0 {
                            f.n.skip(4, 4);
                        } else {
                            f.u32(8 * p + 4)?;
                        }
                    }
                    f.fill(8 * pairs.len(), 0x800 - 8 * pairs.len(), 0, "after the pairs")
                })();
                done(t, c, res);
                for &(po, ps) in &pairs {
                    if a + 0x800 + po + ps > e {
                        return Err(format!("bundle stream at {po:#x} of {ps:#x} bytes runs past the bundle"));
                    }
                    lz_block_in(t, root, lbs, &d[a + 0x800 + po..a + 0x800 + po + ps], (a + 0x800 + po) as u64, ps as u64, BUNDLE_GROUP, |t, r2, du| cutscene(t, r2, du))?;
                }
            }
            'K' => {
                let (ty, ver, o10, sz) = (u32_at(d, a)?, u32_at(d, a + 4)?, u32_at(d, a + 0x10)? as usize, u32_at(d, a + 0x14)? as usize);
                if a + 0x1c > d.len() || ty != 3 || ver != 2 || &d[a + 0x18..a + 0x1c] != b"SBlk" {
                    return Err(format!("sound bank at {a:#x}: type {ty}, version {ver}, no SBlk at +0x18"));
                }
                let ln = (o10 + sz).div_ceil(64) * 64;
                if e - a < ln || o10 + sz > e - a {
                    return Err(format!("sound bank at {a:#x}: {:#x} bytes, the wrapper needs {ln:#x}", e - a));
                }
                crate::sbank::bank_in(t, root, SCREEN_GROUP, a as u64, &d[a..a + o10 + sz], "front end func_003BC6E8 (offsets at header +0x9C..+0xB8) hands each bank in EE memory through func_003BBAA8 to func_0013BC30, which sends it to the IOP (RPC 0x57); its 989snd module parses it in FUN_0000898c", Space::Unpacked)?;
            }
            'W' => {
                let cnt = u32_at(d, a)? as usize;
                let offs: Vec<usize> = (0..cnt).map(|x| u32_at(d, a + 16 + 4 * x).map(|v| v as usize)).collect::<Result<_, _>>()?;
                if offs.first() != Some(&(16 + 4 * cnt)) || offs.windows(2).any(|p| p[0] >= p[1]) || offs.last().is_some_and(|&o| a + o >= e) {
                    return Err(format!("late stream list at {a:#x}: {cnt} streams at {offs:x?}"));
                }
                let mut c = put(t, root, lwh, a, 16 + 4 * cnt)?;
                let res = (|| -> Result<(), String> {
                    let mut f = Fields::new(&d[a..a + 16 + 4 * cnt], &mut c);
                    f.u32(0)?;
                    f.fill(4, 12, 0, "zero words")?;
                    for x in 0..cnt {
                        f.u32(16 + 4 * x)?;
                    }
                    Ok(())
                })();
                done(t, c, res);
                for (x, &o) in offs.iter().enumerate() {
                    let end = offs.get(x + 1).map_or(e - a, |&n| n);
                    lz_block_in(t, root, lws, &d[a + o..a + end], (a + o) as u64, (end - o) as u64, PORTRAIT_GROUP, |t, r2, du| {
                        let n = crate::wad38::image_unpacked(t, r2, PORTRAIT_GROUP, 0, du, "front end func_003BC868 (unpacks stream n of the list at +0xC into an image; the image loaders read +8, +0xC, +0x14, +0x1C, +0x20 and the pixels)")?;
                        if n as usize != du.len() {
                            return Err(format!("late stream image of {n:#x} bytes in {:#x}", du.len()));
                        }
                        Ok(())
                    })?;
                }
            }
            // A model object in the moby class form (front end func_00399D10 relocates it as func_L01_00424048 does a class):
            // moby.rs's lines; a class that fits no plan fails its own line.
            'O' => {
                let _ = crate::moby::class(t, root, SCREEN_GROUP, a as u64, &d[a..e]);
            }
            'A' => layout_block(t, root, a, &d[a..e])?,
            'B' => region_block(t, root, a, &d[a..e])?,
            'P' => pool_images(t, root, a, e - a, &mut pool_ents)?,
            'D' => d_block(t, root, a, e - a, &dblk)?,
            _ => {}
        }
    }
    crate::walk::pad_mem(t, root, d, SCREEN_GROUP);
    Ok(())
}

// ---- the screen's second-region sections: image regions (flag 1), layout (flag 2), pool, data block D, the bundles' cutscene streams ----

/// `crate::walk::lz_block` for a stream inside unpacked data: the LZ header bytes 7-15 get an ignored line of the node's own space
/// (walk.rs's is in disc space, whose child takes no bytes of an unpacked parent). These streams are unpacked by the front end's
/// own copy of the unpacker, func_003A0A20.
fn lz_block_in(
    t: &mut Tracker,
    parent: &mut Node,
    line: usize,
    b: &[u8],
    at: u64,
    size: u64,
    group: &str,
    inside: impl FnOnce(&mut Tracker, &mut Node, &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    let mut n = t.child(parent, line, at, size);
    match crate::lz::unpack(b) {
        Ok(s) => {
            n.mark(0, 7);
            let l = t.ignored_line(
                "LZ streams",
                "LZ header bytes 7-15 (stream inside unpacked data)",
                "read by nothing: front end func_003A0A20 (the unpacker of these streams) reads bytes 0-6, byte 7 is the dropped high half of its 8-byte load at +3, and no other code reads a stream's header",
                Space::Unpacked,
            );
            let h = t.child(&mut n, l, 7, 9);
            t.close(h);
            for (x, y, fill) in [(s.size, b.len(), 0u8)].into_iter().chain(s.skipped.iter().map(|&(x, y)| (x, y, 0xee))) {
                if b[x..y].iter().all(|&v| v == fill) {
                    n.mark(x as u64, (y - x) as u64);
                }
            }
            let mut covered = vec![(0x10usize, s.size)];
            for (x, y) in &s.skipped {
                covered = covered.into_iter().flat_map(|(a, c)| if *x >= c || *y <= a { vec![(a, c)] } else { vec![(a, *x), (*y, c)] }).filter(|r| r.0 < r.1).collect();
            }
            for (x, y) in covered {
                n.mark(x as u64, (y - x) as u64);
            }
            let mut root = t.unpacked_root(&mut n, group, s.data.len() as u64);
            inside(t, &mut root, &s.data)?;
            t.close(root);
        }
        Err(e) => n.fail(e),
    }
    t.close(n);
    Ok(())
}

/// The image-region block (screen flag 1; front end func_00398930 relocates it, func_003CE400 draws it): a 0x40-byte header, the
/// region table, the regions' images (a 0x400-byte palette and 8-bit pixels each), then the blocks: a 0x10-byte header, 0x20-byte
/// entries (a bounding sphere, a pointer to the entry's data and its counts and offsets) and each entry's data (vertices as four
/// s16, a word per vertex, triangles as a word of three vertex indices and a region number), padded to 16 bytes.
fn region_block(t: &mut Tracker, root: &mut Node, a: usize, b: &[u8]) -> Result<(), String> {
    let n = u16_at(b, 6)? as usize;
    let r = u16_at(b, 0xc)? as usize;
    let img = u32_at(b, 0x14)? as usize;
    if b.len() < 0x40 || n == 0 || n > 8 || u32_at(b, 0x10)? != 0x40 || u32_at(b, 0x18)? != 0x40 || u32_at(b, 0x1c)? != 0 || img < 0x40 + 16 * r || img % 0x40 != 0 || img - 0x40 - 16 * r >= 0x40 {
        return Err(format!("image-region block at {a:#x}: header {:02x?}", &b[..b.len().min(0x40)]));
    }
    const G: &str = "front end func_00398930 (relocates +0x10, +0x14, +0x18 and +0x1C and reads +6, +0xC and the block list at +0x20), func_003CE400 (+6), func_003CE4A0 (+0xC, +0x10), func_003CEC70 (+0x14); +4 is 0 on the disc and func_00398930 sets it to 1";
    let lh = t.line(
        SCREEN_GROUP,
        "image-region block header (0x40 bytes: +6 block count, +0xC region count, +0x10 region table, +0x14 image data, +0x18 and +0x1C offsets, +0x20 the block offsets; +0, +4, +8 and +0xE zero)",
        "frontwad::region_block",
        G,
        Space::Unpacked,
    );
    let mut c = put(t, root, lh, a, 0x40)?;
    let res = (|| -> Result<(), String> {
        let mut f = Fields::new(&b[..0x40], &mut c);
        f.fill(0, 4, 0, "zero word")?;
        f.fill(4, 2, 0, "flag func_00398930 sets")?;
        f.mark(6, 2);
        f.fill(8, 4, 0, "zero word")?;
        f.mark(0xc, 2);
        f.fill(0xe, 2, 0, "zero")?;
        for k in [0x10usize, 0x14, 0x18, 0x1c] {
            f.u32(k)?;
        }
        for k in 0..n {
            f.u32(0x20 + 4 * k)?;
        }
        f.fill(0x20 + 4 * n, 0x20 - 4 * n, 0, "unused block offsets")
    })();
    done(t, c, res);
    let lr = t.line(
        SCREEN_GROUP,
        "image-region table (16-byte records: +0 palette offset, +4 pixel offset, +8 width, +0xC height; offsets from the image data)",
        "frontwad::region_block",
        "front end func_00398930 (all four words: +0 and +4 divided by 16 for the GS addresses, +8 and +0xC as log2 sizes)",
        Space::Unpacked,
    );
    let mut c = put(t, root, lr, a + 0x40, 16 * r)?;
    c.mark(0, 16 * r as u64);
    t.close(c);
    if img > 0x40 + 16 * r {
        let l = t.line(SCREEN_GROUP, "image-region table padding (zero to a 0x40 boundary)", "frontwad::region_block", crate::track::NOTHING, Space::Unpacked);
        let mut c = put(t, root, l, a + 0x40 + 16 * r, img - 0x40 - 16 * r)?;
        let res = Fields::new(&b[0x40 + 16 * r..img], &mut c).fill(0, img - 0x40 - 16 * r, 0, "padding");
        done(t, c, res);
    }
    let lp = t.line(SCREEN_GROUP, "image-region palette (256 colours, RGBA)", "frontwad::region_block", "front end func_00398930 and func_003CEC70 (the GS address of the palette: +0 of the region record, from the image data)", Space::Unpacked);
    let li = t.line(SCREEN_GROUP, "image-region pixels (width x height palette indices)", "frontwad::region_block", "front end func_003CEC70 (uploads the region's pixels at the record's +4 offset: width x height bytes)", Space::Unpacked);
    let mut pos = img;
    for k in 0..r {
        let (co, po, w, h) = (u32_at(b, 0x40 + 16 * k)? as usize, u32_at(b, 0x44 + 16 * k)? as usize, u32_at(b, 0x48 + 16 * k)? as usize, u32_at(b, 0x4c + 16 * k)? as usize);
        if co != pos - img || po != co + 0x400 || w == 0 || h == 0 || pos + 0x400 + w * h > b.len() {
            return Err(format!("image-region block at {a:#x}: region {k} has offsets {co:#x} and {po:#x} and {w} x {h} pixels, the image data is at {pos:#x}"));
        }
        let mut c = put(t, root, lp, a + pos, 0x400)?;
        c.mark(0, 0x400);
        t.close(c);
        let mut c = put(t, root, li, a + pos + 0x400, w * h)?;
        c.mark(0, (w * h) as u64);
        t.close(c);
        pos += 0x400 + w * h;
    }
    let offs: Vec<usize> = (0..n).map(|k| u32_at(b, 0x20 + 4 * k).map(|v| v as usize)).collect::<Result<_, _>>()?;
    if offs[0] != pos || offs.windows(2).any(|p| p[0] >= p[1]) || offs[n - 1] >= b.len() {
        return Err(format!("image-region block at {a:#x}: block offsets {offs:x?}, the images end at {pos:#x}"));
    }
    let lb = t.line(
        SCREEN_GROUP,
        "image-region block header (0x10 bytes: +0 entry count, +2 flags (bit 0: untextured triangles, bit 1), +4, +6, +8 angles, +0xA, +0xC, +0xE angle steps)",
        "frontwad::region_block",
        "front end func_003CE630 (+2 bit 0 picks the draw path), func_003CE690 and func_003CE838 (+0, +2), func_003CE2E8 (+4..+0xE: the angles advance by their steps and build the block's rotation), func_00398930 (when bit 0 of +2 is clear and +4 is not zero, +4 replaces +2)",
        Space::Unpacked,
    );
    let le = t.line(
        SCREEN_GROUP,
        "image-region entry (0x20 bytes: +0 bounding sphere x, y, z, radius (f32), +0x10 data pointer, +0x14 vertex count, +0x16 triangle count, +0x18, +0x1A, +0x1C offsets of the vertices, vertex words and triangles; +0x1E no reader (skipped))",
        "frontwad::region_block",
        "front end func_003CEF58 (the sphere, with VU0), func_00398930 (relocates the pointer), func_003CE690 and func_003CE838 (+0x10, +0x14, +0x16, +0x18, +0x1A, +0x1C); +0x1E (the end of the entry's data, which the next entry's pointer repeats) is read by neither",
        Space::Unpacked,
    );
    let lv = t.line(SCREEN_GROUP, "image-region vertices (8 bytes: x, y, z, w as s16)", "frontwad::region_block", "front end func_003CEA78 (loads each vertex into the VU0 registers and transforms it)", Space::Unpacked);
    let lc = t.line(SCREEN_GROUP, "image-region vertex words (u32 each: RGBA colour in a block with flag bit 0, else two u16 texture coordinates)", "frontwad::region_block", "front end func_003CEB70 (the word becomes the vertex colour) and func_003CEC70 (the texture coordinates)", Space::Unpacked);
    let lt = t.line(SCREEN_GROUP, "image-region triangles (u32 each: three vertex indices in bytes 0-2, the region number in byte 3, which a block with flag bit 0 does not read: it holds 0xFF)", "frontwad::region_block", "front end func_003CEC70 (bytes 0-2 index the transformed vertices, byte 3 picks the region record) and func_003CEB70 (bytes 0-2)", Space::Unpacked);
    let lz = t.line(SCREEN_GROUP, "image-region entry padding (zero to 16 bytes)", "frontwad::region_block", crate::track::NOTHING, Space::Unpacked);
    let lt2 = t.line(SCREEN_GROUP, "image-region block padding (zero)", "frontwad::region_block", crate::track::NOTHING, Space::Unpacked);
    for (bi, &o) in offs.iter().enumerate() {
        let e = offs.get(bi + 1).copied().unwrap_or(b.len());
        let cnt = u16_at(b, o)? as usize;
        let flags = u16_at(b, o + 2)?;
        let mut c = put(t, root, lb, a + o, 0x10)?;
        c.mark(0, 0x10);
        t.close(c);
        let mut c = put(t, root, le, a + o + 0x10, 0x20 * cnt)?;
        let mut p = o + 0x10 + 0x20 * cnt;
        let mut plan = Vec::new();
        let res = (|| -> Result<(), String> {
            for k in 0..cnt {
                let q = o + 0x10 + 0x20 * k;
                let (ptr, nv, nt, a0, a1, a2, a3) = (
                    u32_at(b, q + 0x10)? as usize,
                    u16_at(b, q + 0x14)? as usize,
                    u16_at(b, q + 0x16)? as usize,
                    u16_at(b, q + 0x18)? as usize,
                    u16_at(b, q + 0x1a)? as usize,
                    u16_at(b, q + 0x1c)? as usize,
                    u16_at(b, q + 0x1e)? as usize,
                );
                let tail = a3.wrapping_sub(a2 + 4 * nt);
                if ptr != p || a0 != 0 || a1 != 8 * nv || a2 != a1 + 4 * nv || a3 % 16 != 0 || tail >= 16 || p + a3 > e {
                    return Err(format!("entry {k}: pointer {ptr:#x} (expected {p:#x}), {nv} vertices, {nt} triangles, offsets {a0:#x} {a1:#x} {a2:#x} {a3:#x}"));
                }
                c.mark(32 * k as u64, 0x1e);
                c.skip(32 * k as u64 + 0x1e, 2);
                plan.push((p, nv, nt, a2, tail));
                p += a3;
            }
            Ok(())
        })();
        done(t, c, res);
        for &(p0, nv, nt, a2, tail) in &plan {
            let mut c = put(t, root, lv, a + p0, 8 * nv)?;
            c.mark(0, 8 * nv as u64);
            t.close(c);
            let mut c = put(t, root, lc, a + p0 + 8 * nv, 4 * nv)?;
            c.mark(0, 4 * nv as u64);
            t.close(c);
            if nt > 0 {
                let mut c = put(t, root, lt, a + p0 + a2, 4 * nt)?;
                let res = (|| -> Result<(), String> {
                    for i in 0..nt {
                        c.mark(4 * i as u64, 3);
                        if flags & 1 == 0 {
                            c.mark(4 * i as u64 + 3, 1);
                        } else if b[p0 + a2 + 4 * i + 3] != 0xff {
                            return Err(format!("untextured triangle {i} has byte 3 {:#04x}", b[p0 + a2 + 4 * i + 3]));
                        } else {
                            c.skip(4 * i as u64 + 3, 1);
                        }
                    }
                    Ok(())
                })();
                done(t, c, res);
            }
            if tail > 0 {
                let mut c = put(t, root, lz, a + p0 + a2 + 4 * nt, tail)?;
                let res = Fields::new(&b[p0 + a2 + 4 * nt..p0 + a2 + 4 * nt + tail], &mut c).fill(0, tail, 0, "padding");
                done(t, c, res);
            }
        }
        if p < e {
            let mut c = put(t, root, lt2, a + p, e - p)?;
            let res = Fields::new(&b[p..e], &mut c).fill(0, e - p, 0, "padding");
            done(t, c, res);
        }
    }
    Ok(())
}

/// The bytes of a VIF1 stream `b[p..e]` must be whole VIF codes, each UNPACK, STROW, STCOL, STMASK, MPG, DIRECT with the data it
/// takes (the unpack's size from the STCYCL values in force and the element size).
fn vif_check(b: &[u8], mut p: usize, e: usize) -> Result<(), String> {
    let (mut cl, mut wl) = (0usize, 0usize);
    while p < e {
        if p + 4 > e {
            return Err(format!("VIF code at {p:#x} past the end {e:#x}"));
        }
        let wd = u32_at(b, p)?;
        let (cmd, num, imm) = ((wd >> 24) & 0x7f, ((wd >> 16) & 0xff) as usize, (wd & 0xffff) as usize);
        let n = match cmd {
            0 | 2..=7 | 0x10 | 0x11 | 0x13 | 0x14 | 0x15 | 0x17 => 4,
            1 => {
                cl = imm & 0xff;
                wl = imm >> 8;
                4
            }
            0x20 => 8,
            0x30 | 0x31 => 20,
            0x4a => 4 + 8 * if num == 0 { 256 } else { num },
            0x50 | 0x51 => 4 + 16 * if imm == 0 { 0x10000 } else { imm },
            0x60..=0x6f => {
                let bits = match cmd {
                    0x60 => 32,
                    0x61 => 16,
                    0x62 => 8,
                    0x64 => 64,
                    0x65 => 32,
                    0x66 => 16,
                    0x68 => 96,
                    0x69 => 48,
                    0x6a => 24,
                    0x6c => 128,
                    0x6d => 64,
                    0x6e => 32,
                    0x6f => 16,
                    _ => return Err(format!("UNPACK format {cmd:#x} at {p:#x}")),
                };
                let nn = if num == 0 { 256 } else { num };
                let src = if wl <= cl || cl == 0 {
                    if wl != 0 {
                        wl * (nn / wl) + (nn % wl).min(wl)
                    } else {
                        nn
                    }
                } else {
                    cl * (nn / wl) + (nn % wl).min(cl)
                };
                4 + (src * bits).div_ceil(32) * 4
            }
            _ => return Err(format!("VIF command {cmd:#x} at {p:#x}")),
        };
        p += n;
    }
    if p != e {
        return Err(format!("the VIF stream's last code runs to {p:#x}, past {e:#x}"));
    }
    Ok(())
}

/// The layout block (screen flag 2; front end func_0039A148 relocates it, func_003CF418 culls and draws it): a 0x40-byte header,
/// 0x40-byte records and each record's data: a VIF1 stream whose UNPACK of five quadwords is the texture packet func_0039A148
/// rewrites, the colours, a part of the stream nothing reads, a list of 16-byte sprites and the bounding box corners.
fn layout_block(t: &mut Tracker, root: &mut Node, a: usize, b: &[u8]) -> Result<(), String> {
    let (off, n) = (u32_at(b, 0)? as usize, u32_at(b, 4)? as usize);
    if off != 0x40 || n == 0 || u32_at(b, 0xc)? as usize != n || b.len() < 0x40 + 0x40 * n {
        return Err(format!("layout block at {a:#x}: table at {off:#x}, {n} records, count word {:#x}", u32_at(b, 0xc)?));
    }
    let lh = t.line(
        SCREEN_GROUP,
        "layout block header (0x40 bytes: +0 record table offset, +4 record count, +8 f32 scale, +0xC record count again; the rest zero)",
        "frontwad::layout_block",
        "front end func_0039A148 (reads +0, +4, +8 and +0xC: the scale builds the depth ranges func_003CF048 uses)",
        Space::Unpacked,
    );
    let mut c = put(t, root, lh, a, 0x40)?;
    let res = (|| -> Result<(), String> {
        let mut f = Fields::new(&b[..0x40], &mut c);
        for k in [0usize, 4, 8, 0xc] {
            f.u32(k)?;
        }
        f.fill(0x10, 0x30, 0, "zero words")
    })();
    done(t, c, res);
    let lr = t.line(
        SCREEN_GROUP,
        "layout record (0x40 bytes: +0 bounding sphere x, y, z, radius (f32), +0x10 data pointer, +0x14, +0x16, +0x1A, +0x1C, +0x1E u16 offsets into the data, +0x20..+0x26 byte counts, +0x27 flag, +0x28 packet count, +0x2A byte, +0x2C sprite count, +0x2E sprite offset, +0x35 byte, +0x36 s16, +0x38 box offset, +0x3A, +0x3B bytes, +0x3E u16; +0x18, +0x29, +0x2B, +0x30, +0x32, +0x34, +0x3C and +0x3D no reader (skipped))",
        "frontwad::layout_block",
        "front end func_0039A148 (relocates +0x10, reads +0x1C and +0x28), func_003CF418 (every other field: the sphere with VU0; the offsets and counts make the DMA tags of the three detail levels); no load of +0x18, +0x29, +0x2B, +0x30, +0x32, +0x34, +0x3C or +0x3D in func_003CF418, func_0039A148, func_003CF2A0 or func_003D0020",
        Space::Unpacked,
    );
    let mut ptrs = Vec::new();
    let mut c = put(t, root, lr, a + 0x40, 0x40 * n)?;
    for i in 0..n {
        let q = 0x40 + 0x40 * i;
        let r = 0x40 * i as u64;
        c.mark(r, 0x18);
        c.skip(r + 0x18, 2);
        c.mark(r + 0x1a, 6);
        c.mark(r + 0x20, 8);
        c.mark(r + 0x28, 1);
        c.skip(r + 0x29, 1);
        c.mark(r + 0x2a, 1);
        c.skip(r + 0x2b, 1);
        c.mark(r + 0x2c, 4);
        c.skip(r + 0x30, 5);
        c.mark(r + 0x35, 7);
        c.skip(r + 0x3c, 2);
        c.mark(r + 0x3e, 2);
        ptrs.push(u32_at(b, q + 0x10)? as usize);
    }
    t.close(c);
    if ptrs[0] != 0x40 * n || ptrs.windows(2).any(|p| p[0] > p[1]) || off + ptrs[n - 1] > b.len() {
        return Err(format!("layout block at {a:#x}: record data pointers {:#x}.. do not follow the record table", ptrs[0]));
    }
    let ls = t.line(
        SCREEN_GROUP,
        "layout VIF1 stream (VIF codes: STROW, STMOD, STCYCL, UNPACK of V4-8, V3-16, V4-16 or V4-32, each followed by its data)",
        "frontwad::layout_block",
        "front end func_003CF418 (the DMA tags that send the record's stream to VIF1, the three detail levels at +0x14, +0x16 and +0x1A, with VU1 program data), func_0039A148 (rewrites the texture words of the packet its last UNPACK carries)",
        Space::Unpacked,
    );
    let lp = t.line(
        SCREEN_GROUP,
        "layout texture packet (five GS A+D writes of 16 bytes: data, register number: TEX0_1 (+0 the region number func_0039A148 replaces), TEX1_1, CLAMP_1, MIPTBP1_1, MIPTBP2_1)",
        "frontwad::layout_block",
        "front end func_0039A148 (reads the region number at +0, +0x10 and +0x24 words and builds the TEX0, TEX1, CLAMP and MIPTBP words), func_003CF2A0 (a byte of the TEX1 word picks a texture slot); the register numbers go to the GS unchanged",
        Space::Unpacked,
    );
    let lcol = t.line(SCREEN_GROUP, "layout colours (RGBA, 4 bytes each, the count at record +0x24)", "frontwad::layout_block", "front end func_003CF418 (the colour stream the third DMA tag sends: byte +0x24 colours, a quarter of its value in quadwords)", Space::Unpacked);
    let lpad = t.line(SCREEN_GROUP, "layout colour padding (zero to 16 bytes)", "frontwad::layout_block", crate::track::NOTHING, Space::Unpacked);
    let lg = t.ignored_line(
        SCREEN_GROUP,
        "layout stream part at record +0x30 (a STROW row and 16-bit vertex data, 0x30-0x90 bytes)",
        "searched: no load of record +0x30 or +0x32 in func_003CF418, func_0039A148, func_003CF2A0 or func_003D0020, and no DMA tag of func_003CF418 starts or ends there (the tags run from +0x14, +0x16, +0x1A and +0x1E for the counts in +0x20..+0x26, from +0x1C for +0x28 packets, from +0x2E and from +0x38)",
        Space::Unpacked,
    );
    let lsp = t.line(SCREEN_GROUP, "layout sprite list (16-byte entries: x, y, z (f32), +0xC u16 id, +0xE and +0xF bytes)", "frontwad::layout_block", "front end func_003D0020 (the culling pass loads each entry with VU0 and reads +0xC, +0xE and +0xF)", Space::Unpacked);
    let lbb = t.line(SCREEN_GROUP, "layout bounding box (eight corners of 8 bytes: x, y, z, w as s16)", "frontwad::layout_block", "front end func_003CF418 (loads the corners from +0x38 to cull the record against the view)", Space::Unpacked);
    let lfail = t.line(SCREEN_GROUP, "layout record data (not split)", "", "", Space::Unpacked);
    for i in 0..n {
        let r = &b[off + 0x40 * i..off + 0x40 * i + 0x40];
        let base = off + ptrs[i];
        let end = ptrs.get(i + 1).map_or(b.len(), |&p| off + p);
        let h = |o: usize| u16_at(r, o).map(|v| v as usize);
        let plan = (|| -> Result<(usize, usize, usize, usize, usize, usize, usize), String> {
            let (o1c, o1e, o30, o2e, o38, cnt, nc) = (h(0x1c)?, h(0x1e)?, h(0x30)?, h(0x2e)?, h(0x38)?, r[0x28] as usize, r[0x24] as usize);
            let (ns, sz) = (h(0x2c)?, end - base);
            if o1c < 4 || o1c + 0x50 * cnt > o1e || o30 < o1e + 4 * nc || o30 - o1e - 4 * nc >= 16 || o30 > o2e || o38 != o2e + 16 * ns || sz != o38 + 0x40 {
                return Err(format!("record {i}: offsets {o1c:#x} {o1e:#x} {o30:#x} {o2e:#x} {o38:#x}, {cnt} packets, {nc} colours, {ns} sprites in {sz:#x} bytes"));
            }
            vif_check(b, base, base + o1c - 4)?;
            let wd = u32_at(b, base + o1c - 4)?;
            if (wd >> 24) & 0x7f != 0x6c || ((wd >> 16) & 0xff) as usize != 5 * cnt {
                return Err(format!("record {i}: the code before the packets is {wd:#x}"));
            }
            vif_check(b, base + o1c + 0x50 * cnt, base + o1e)?;
            for k in 0..cnt {
                for j in 0..5 {
                    let reg = u32_at(b, base + o1c + 0x50 * k + 16 * j + 8)? & 0xff;
                    if reg != [6, 0x14, 8, 0x34, 0x36][j] {
                        return Err(format!("record {i}: packet {k} register word {j} is {reg:#x}"));
                    }
                }
            }
            if b[base + o1e + 4 * nc..base + o30].iter().any(|&v| v != 0) {
                return Err(format!("record {i}: colour padding is not zero"));
            }
            Ok((o1c, o1e, o30, o2e, o38, cnt, nc))
        })();
        let (o1c, o1e, o30, o2e, o38, cnt, nc) = match plan {
            Ok(v) => v,
            Err(e) => {
                let mut c = put(t, root, lfail, a + base, end - base)?;
                c.fail(e);
                t.close(c);
                continue;
            }
        };
        let mut c = put(t, root, ls, a + base, o1c)?;
        c.mark(0, o1c as u64);
        t.close(c);
        let mut c = put(t, root, lp, a + base + o1c, 0x50 * cnt)?;
        c.mark(0, 0x50 * cnt as u64);
        t.close(c);
        let s2 = o1c + 0x50 * cnt;
        let mut c = put(t, root, ls, a + base + s2, o1e - s2)?;
        c.mark(0, (o1e - s2) as u64);
        t.close(c);
        if nc > 0 {
            let mut c = put(t, root, lcol, a + base + o1e, 4 * nc)?;
            c.mark(0, 4 * nc as u64);
            t.close(c);
        }
        if o30 > o1e + 4 * nc {
            let mut c = put(t, root, lpad, a + base + o1e + 4 * nc, o30 - o1e - 4 * nc)?;
            c.mark(0, (o30 - o1e - 4 * nc) as u64);
            t.close(c);
        }
        let n = put(t, root, lg, a + base + o30, o2e - o30)?;
        t.close(n);
        if o38 > o2e {
            let mut c = put(t, root, lsp, a + base + o2e, o38 - o2e)?;
            c.mark(0, (o38 - o2e) as u64);
            t.close(c);
        }
        let mut c = put(t, root, lbb, a + base + o38, 0x40)?;
        c.mark(0, 0x40);
        t.close(c);
    }
    Ok(())
}

/// The pool (func_003BBCE8 adds its start to the +0 offsets of the region table and the texture list): the images they name, each
/// a width x width block of palette indices and, when the record's +8 is above 1, the half-size image after it (func_003CB2A0 and
/// func_003D0188 upload the pair).
fn pool_images(t: &mut Tracker, root: &mut Node, a: usize, size: usize, ents: &mut Vec<(usize, usize, i16)>) -> Result<(), String> {
    ents.sort();
    let l0 = t.line(
        SCREEN_GROUP,
        "screen pool image (width x width palette indices)",
        "frontwad::pool_images",
        "front end func_003BBCE8 (the table entry = the pool start + the record's +0, tagged with the log2 of +4), func_003CB2A0 and func_003D0188 (upload width x width bytes to the GS)",
        Space::Unpacked,
    );
    let l1 = t.line(
        SCREEN_GROUP,
        "screen pool image, level 1 (half width x half width palette indices, after the image; records with +8 above 1)",
        "frontwad::pool_images",
        "front end func_003CB2A0 and func_003D0188 (upload the half-size level at the image's address + width x width)",
        Space::Unpacked,
    );
    let mut pos = 0;
    for &(o, w, m) in ents.iter() {
        if o != pos || w < 2 || !w.is_power_of_two() {
            return Err(format!("screen pool: image at {o:#x} of width {w}, the previous ends at {pos:#x}"));
        }
        let mut c = put(t, root, l0, a + o, w * w)?;
        c.mark(0, (w * w) as u64);
        t.close(c);
        pos += w * w;
        if m > 1 {
            let mut c = put(t, root, l1, a + pos, (w / 2) * (w / 2))?;
            c.mark(0, ((w / 2) * (w / 2)) as u64);
            t.close(c);
            pos += (w / 2) * (w / 2);
        }
    }
    if pos != size {
        return Err(format!("screen pool: the images end at {pos:#x}, the pool has {size:#x} bytes"));
    }
    Ok(())
}

/// Data block D (flag 0x10): for each record of the table (four u32: palette offset, zero, image offset, image width) a 0x400-byte
/// palette and a width x width image (front end func_00398760 turns the offsets into the GS source addresses at 0x2FC8C0).
fn d_block(t: &mut Tracker, root: &mut Node, a: usize, size: usize, recs: &[(usize, usize, usize, usize)]) -> Result<(), String> {
    let mut items: Vec<(usize, usize, bool)> = Vec::new();
    for &(p, z, i, w) in recs {
        if z != 0 {
            return Err(format!("data block D: record with +4 {z:#x}"));
        }
        items.push((p, 0x400, false));
        items.push((i, w * w, true));
    }
    items.sort();
    let lp = t.line(SCREEN_GROUP, "screen data block D palette (256 colours, RGBA)", "frontwad::d_block", "front end func_00398760 (the record's +0 offset plus D's address makes the palette's GS source address at 0x2FC8C0; func_003CD628 sends it)", Space::Unpacked);
    let li = t.line(SCREEN_GROUP, "screen data block D image (width x width palette indices)", "frontwad::d_block", "front end func_00398760 (the record's +8 offset and +0xC width make the image's GS source address and size at 0x2FC8C4; func_003CD628 sends it)", Space::Unpacked);
    let mut pos = 0;
    for &(o, n, img) in &items {
        if o != pos {
            return Err(format!("data block D: item at {o:#x}, the previous ends at {pos:#x}"));
        }
        let mut c = put(t, root, if img { li } else { lp }, a + o, n)?;
        c.mark(0, n as u64);
        t.close(c);
        pos += n;
    }
    if pos != size {
        return Err(format!("data block D: the records cover {pos:#x} of its {size:#x} bytes"));
    }
    Ok(())
}

/// A bundle stream (a front end cutscene; func_003BC978 points 0x225810 + 4k at it and func_0039A958 gives stream k to
/// func_0039A700, which unpacks and relocates it): a header, the camera records, and for each actor a 0x10-byte header, an animation
/// sequence in the moby form and the position records; the bytes after the last actor's positions are not reached by any offset.
fn cutscene(t: &mut Tracker, root: &mut Node, u: &[u8]) -> Result<(), String> {
    let na = u16_at(u, 0xc)? as usize;
    let tt = u32_at(u, 0x10)? as usize;
    let hend = 0x14 + 4 * na;
    if na == 0 || tt < hend || tt % 16 != 0 || tt - hend >= 16 || u16_at(u, 0xe)? != 0 {
        return Err(format!("cutscene stream: {na} actors, camera records at {tt:#x}"));
    }
    let offs: Vec<usize> = (0..na).map(|k| u32_at(u, 0x14 + 4 * k).map(|v| v as usize)).collect::<Result<_, _>>()?;
    if offs[0] < tt || (offs[0] - tt) % 0x20 != 0 {
        return Err(format!("cutscene stream: camera records {tt:#x}..{:#x}", offs[0]));
    }
    let lh = t.line(
        BUNDLE_GROUP,
        "cutscene header (+0 u16 length in ticks, +4 u32 two u16, +8 s16 tick, +0xC u16 actor count, +0x10 camera records offset, +0x14 one offset per actor; +2, +0xA and +0xE no reader (skipped))",
        "frontwad::cutscene",
        "front end func_0039A700 (+0 the length, +4 a non-zero word stored as two halves, +8 the tick at which func_003A1A58 runs, +0xC, +0x10 and the actor offsets from +0x14), func_003BDEC0 and func_003A6B18 (the tick counter against +0 and +8); +2, +0xA and +0xE are not loaded",
        Space::Unpacked,
    );
    let mut c = put(t, root, lh, 0, hend)?;
    c.mark(0, 2);
    c.skip(2, 2);
    c.mark(4, 4);
    c.mark(8, 2);
    c.skip(0xa, 2);
    c.mark(0xc, 2);
    c.skip(0xe, 2);
    c.mark(0x10, 4 + 4 * na as u64);
    t.close(c);
    if tt > hend {
        let l = t.line(BUNDLE_GROUP, "cutscene header padding (zero to 16 bytes)", "frontwad::cutscene", crate::track::NOTHING, Space::Unpacked);
        let mut c = put(t, root, l, hend, tt - hend)?;
        let res = Fields::new(&u[hend..tt], &mut c).fill(0, tt - hend, 0, "padding");
        done(t, c, res);
    }
    let nrec = (offs[0] - tt) / 0x20;
    let lc = t.line(
        BUNDLE_GROUP,
        "cutscene camera records (0x20 bytes: +0 position x, y, z (f32) and +0xC a word whose byte is the end flag, +0x10, +0x14, +0x18 rotation angles (f32), +0x1C f32; record 0x60 and later no reader (skipped))",
        "frontwad::cutscene",
        "front end func_003BD928 (loads the 16 bytes at +0 as the camera position, the byte +0xC as the flag, +0x10, +0x14, +0x18 as the three rotations and +0x1C into 0x225A30); func_003BDEC0 reloads the next stream when the tick passes 0x5F, so a record 0x60 is never reached",
        Space::Unpacked,
    );
    let mut c = put(t, root, lc, tt, 0x20 * nrec)?;
    c.mark(0, 0x20 * nrec.min(0x60) as u64);
    if nrec > 0x60 {
        c.skip(0x20 * 0x60, 0x20 * (nrec - 0x60) as u64);
    }
    t.close(c);
    let la = t.line(
        BUNDLE_GROUP,
        "cutscene actor header (+0 object class id, +0xC offset of its position records; +4 and +8 no reader (skipped))",
        "frontwad::cutscene",
        "front end func_0039A700 (+0 picks the object (the id 0x215 and 0x10D1 become 0xD54), +0xC is added to the stream's address for the object's position records); +4 and +8 are not loaded",
        Space::Unpacked,
    );
    let lq = t.line(BUNDLE_GROUP, "cutscene actor sequence (not split)", "", "", Space::Unpacked);
    let lpos = t.line(
        BUNDLE_GROUP,
        "cutscene position records (16 bytes: x, y, z (f32) and a zero word, one per frame of the sequence)",
        "frontwad::cutscene",
        "front end func_003BDD20 (blends the records of frames tick / 2 and tick / 2 + 1 with func_0038D148, which loads each as a vector)",
        Space::Unpacked,
    );
    let mut pos = offs[0];
    for (k, &ao) in offs.iter().enumerate() {
        if ao != pos {
            return Err(format!("cutscene stream: actor {k} at {ao:#x}, the previous part ends at {pos:#x}"));
        }
        let po = u32_at(u, ao + 12)? as usize;
        let s = ao + 0x10;
        let nf = u[s + 0x10] as usize;
        if po <= s + 0x1c || po + 16 * nf > u.len() {
            return Err(format!("cutscene stream: actor {k} has its positions at {po:#x}"));
        }
        let mut c = put(t, root, la, ao, 0x10)?;
        c.mark(0, 4);
        c.skip(4, 8);
        c.mark(12, 4);
        t.close(c);
        if let Err(e) = crate::moby::slot_sequence(t, root, BUNDLE_GROUP, s as u64, &u[s..po], 0) {
            let mut c = put(t, root, lq, s, po - s)?;
            c.fail(e);
            t.close(c);
        }
        let mut c = put(t, root, lpos, po, 16 * nf)?;
        let res = (|| -> Result<(), String> {
            for i in 0..nf {
                c.mark(16 * i as u64, 12);
                if u32_at(u, po + 16 * i + 12)? != 0 {
                    return Err(format!("position record {i}: w word {:#x}", u32_at(u, po + 16 * i + 12)?));
                }
                c.mark(16 * i as u64 + 12, 4);
            }
            Ok(())
        })();
        done(t, c, res);
        pos = po + 16 * nf;
    }
    let l = t.ignored_line(
        BUNDLE_GROUP,
        "cutscene stream tail (the bytes after the last actor's position records)",
        "searched: func_0039A700 reaches the stream only through its header offsets (+0x10, the actors from +0x14), the actors' +0xC and the sequences' frame offsets, and func_003BD928 and func_003BDD20 index the camera and position records by tick: no offset or count reaches the bytes after the last actor's position records",
        Space::Unpacked,
    );
    let c = put(t, root, l, pos, u.len() - pos)?;
    t.close(c);
    Ok(())
}

// ---- WAD7: the gadget blocks ----

const LEVEL1_WHY: &str = "func_L01_0042A598 starts level 2 at 5/4 of level 0's size, so it steps over level 1 without reading or uploading it; the other loaders (frontbin func_00395EB8, func_00393E90, func_00393A18, front end func_003824F0) take the palette and level 0 only";
const LEVEL3_WHY: &str = "func_L01_0042A598 uploads levels 0 and 2 only (the level count is clamped to 3); no loader addresses a level 3";
const GADGET_TEXTURES: &str = "WAD7 gadget textures (unpacked)";
const TEX_FN: &str = "func_L01_0042A598 (same in all 34 story levels; the form of the armor texture lists)";

/// A 2FIP image in unpacked space (pif.rs's form, which is for disc space): a 0x20-byte header, a 0x400-byte palette and the
/// levels' palette indices. Returns the image's end.
fn image(t: &mut Tracker, root: &mut Node, d: &[u8], o: usize) -> Result<usize, String> {
    let (w, h, lv) = (u32_at(d, o + 8)? as usize, u32_at(d, o + 12)? as usize, u32_at(d, o + 0x1c)? as usize);
    if !(8..=1024).contains(&w) || !w.is_power_of_two() || !(8..=1024).contains(&h) || !h.is_power_of_two() || !(1..=4).contains(&lv) || (w >> (lv - 1)) == 0 || (h >> (lv - 1)) == 0 {
        return Err(format!("image at {o:#x}: {w} x {h} with {lv} levels"));
    }
    let lh = t.line(
        GADGET_TEXTURES,
        "texture header (+0 tag, +4 no reader (skipped), +8 width, +0xC height, +0x10 0x13, +0x14 palette format 0, +0x18 zero, +0x1C mip levels)",
        "frontwad::image",
        "func_L01_0042A598 (same in all 34 story levels; the form of the armor texture lists); +4: no reader in it or in the other image loaders (pif.rs lists them)",
        Space::Unpacked,
    );
    let mut c = put(t, root, lh, o, 0x20)?;
    let res = (|| -> Result<(), String> {
        let mut f = Fields::new(&d[o..o + 0x20], &mut c);
        f.same(0, b"2FIP", "tag")?;
        f.u32(8)?;
        f.u32(0xc)?;
        f.u32_eq(0x10, 0x13, "pixel format (PSMT8)")?;
        f.u32_eq(0x14, 0, "palette format")?;
        f.u32_eq(0x18, 0, "zero")?;
        f.u32(0x1c)?;
        f.n.skip(4, 4);
        Ok(())
    })();
    done(t, c, res);
    let lp = t.line(GADGET_TEXTURES, "texture palette (256 colours of R, G, B, alpha 0..0x80)", "frontwad::image", TEX_FN, Space::Unpacked);
    let mut c = put(t, root, lp, o + 0x20, 0x400)?;
    let res = (|| -> Result<(), String> {
        if let Some(k) = (0..256).find(|&k| d[o + 0x20 + 4 * k + 3] > 0x80) {
            return Err(format!("palette colour {k}: alpha {:#04x}", d[o + 0x20 + 4 * k + 3]));
        }
        c.mark(0, 0x400);
        Ok(())
    })();
    done(t, c, res);
    let mut at = o + 0x420;
    for k in 0..lv {
        let size = (w >> k) * (h >> k);
        let l = match k {
            0 => t.line(GADGET_TEXTURES, "texture pixels, level 0 (width x height palette indices)", "frontwad::image", TEX_FN, Space::Unpacked),
            1 => t.ignored_line(GADGET_TEXTURES, "texture pixels, level 1 (half size; never uploaded)", LEVEL1_WHY, Space::Unpacked),
            2 => t.line(GADGET_TEXTURES, "texture pixels, level 2 (quarter size)", "frontwad::image", "func_L01_0042A598 (same in all 34 story levels) uploads it when the level count is 3 or more", Space::Unpacked),
            _ => t.ignored_line(GADGET_TEXTURES, "texture pixels, level 3 (eighth size; never uploaded)", LEVEL3_WHY, Space::Unpacked),
        };
        let mut c = put(t, root, l, at, size)?;
        if k == 0 || k == 2 {
            c.mark(0, size as u64);
        }
        t.close(c);
        at += size;
    }
    Ok(at)
}

/// The gadget's texture block: a list (a count of 1 to 3, three offsets) and the images, each right after the previous.
fn gadget_textures(t: &mut Tracker, root: &mut Node, d: &[u8]) -> Result<(), String> {
    let l = t.line(GADGET_TEXTURES, "texture list (u32 count 1-3, three u32 offsets, the unused ones zero)", "frontwad::gadget_textures", TEX_FN, Space::Unpacked);
    let mut c = put(t, root, l, 0, 16)?;
    let count = u32_at(d, 0)? as usize;
    let offs: Vec<usize> = (0..3).map(|i| u32_at(d, 4 + 4 * i).map(|v| v as usize)).collect::<Result<_, _>>()?;
    let res = (|| -> Result<(), String> {
        let mut f = Fields::new(&d[..16], &mut c);
        f.u32(0)?;
        if !(1..=3).contains(&count) || offs[0] != 16 {
            return Err(format!("texture list: count {count}, offsets {offs:x?}"));
        }
        for (i, &o) in offs.iter().enumerate() {
            if i < count {
                f.u32(4 + 4 * i)?;
            } else {
                f.fill(4 + 4 * i, 4, 0, "unused offset")?;
                if o != 0 {
                    return Err(format!("unused offset {i}: {o:#x}"));
                }
            }
        }
        Ok(())
    })();
    done(t, c, res);
    let mut pos = 16;
    for (i, &o) in offs.iter().take(count.min(3)).enumerate() {
        if o != pos {
            return Err(format!("image {i} at {o:#x}, the previous image ends at {pos:#x}"));
        }
        pos = image(t, root, d, o)?;
    }
    if pos != d.len() {
        return Err(format!("the images end at {pos:#x}, the block at {:#x}", d.len()));
    }
    Ok(())
}

/// A gadget block (`sector` is its sector in the WAD): four u32, the moby class stream and the texture stream.
fn wad7_gadget(t: &mut Tracker, root: &mut Node, at: u64, b: &[u8], sector: u64, pair: usize) -> Result<(), String> {
    let mut r = t.region(root, G7, at, b.len() as u64);
    if b.len() < 0x20 || &b[0x10..0x13] != b"WAD" {
        return Err(format!("gadget block {pair}: no LZ stream at +0x10"));
    }
    let s1 = u32_at(b, 0x13)? as usize;
    let o2 = (0x10 + s1 + 15) & !15;
    if o2 + 0x10 > b.len() || &b[o2..o2 + 3] != b"WAD" {
        return Err(format!("gadget block {pair}: no second LZ stream at {o2:#x}"));
    }
    let s2 = u32_at(b, o2 + 3)? as usize;
    let l = t.line(
        G7,
        "gadget block header (+0 and +8 the two streams' offsets in the WAD, +4 and +0xC their sizes)",
        "frontwad::wad7_gadget",
        "no code reads them (they repeat the streams' own offsets, sector * 0x800 + 0x10 and the 16-aligned next start, and the streams' size fields; checked by the decoder); frontbin func_003959A8 and func_L01_00429C30 load the block by its sector and sector count",
        Space::Disc,
    );
    let mut n = put(t, &mut r, l, 0, 16)?;
    let res = (|| -> Result<(), String> {
        let mut f = Fields::new(&b[..16], &mut n);
        f.u32_eq(0, (sector * SECTOR + 0x10) as u32, "first stream's offset in the WAD")?;
        f.u32_eq(4, s1 as u32, "first stream's size")?;
        f.u32_eq(8, (sector * SECTOR + o2 as u64) as u32, "second stream's offset in the WAD")?;
        f.u32_eq(12, s2 as u32, "second stream's size")
    })();
    done(t, n, res);
    let l1 = t.line(G7, "gadget moby class: LZ stream", "lz::unpack", "func_L01_00433EA0 (each program's copy)", Space::Disc);
    crate::walk::lz_block(t, &mut r, l1, &b[0x10..o2], 0x10, (o2 - 0x10) as u64, "WAD7 gadget class (unpacked)", |t, root, d| crate::moby::class(t, root, "WAD7 gadget class (unpacked)", 0, d).map(|_| ()))?;
    let l2 = t.line(G7, "gadget textures: LZ stream", "lz::unpack", "func_L01_00433EA0 (each program's copy)", Space::Disc);
    crate::walk::lz_block(t, &mut r, l2, &b[o2..], o2 as u64, (b.len() - o2) as u64, GADGET_TEXTURES, |t, root, d| gadget_textures(t, root, d))?;
    crate::walk::pad_mem(t, &mut r, b, G7);
    t.close(r);
    Ok(())
}

// ---- generated by captures/agent-savevars/gen.py: the loads found in each save variable larger than 8 bytes ----

/// (variable id, fields (offset, width, stride, reader), what was scanned). Stride 0: a load at a constant address; a stride: an array read through an index,
/// one field covering offset + i * stride for every i up to the variable's byte count (the descriptor size is the only bound the readers give).
const VAR_FIELDS: &[(u32, &[(u16, u8, u16, &str)], &str)] = &[
    (0x1e, &[(0x0, 1, 0, "fe100_func_003B8D18 +14"), (0x0, 1, 1, "fe100_func_00393E68 +105"), (0x1, 1, 0, "func_L01_004E7148"), (0x5, 1, 0, "func_L01_004E7148"), (0x9, 1, 0, "func_L01_00338088 +2"), (0x9, 1, 0, "func_L01_00337228 +1"), (0x10, 1, 0, "func_L01_00306F68 +3"), (0x10, 1, 0, "func_L01_0030A3D0 +2"), (0x11, 1, 0, "func_L01_00304888 +6"), (0x11, 1, 0, "func_L01_00303F88 +5"), (0x13, 1, 0, "func_L01_0031D0B0"), (0x13, 1, 0, "func_L01_0031C890 +2"), (0x15, 1, 0, "func_L01_0032C320 +2"), (0x15, 1, 0, "func_L01_0032BF28 +7"), (0x16, 1, 0, "func_L01_00341C10 +3"), (0x16, 1, 0, "func_L01_003410E8 +2"), (0x1a, 1, 0, "func_L01_004E7148"), (0x1e, 1, 0, "func_L01_004E7148"), (0x20, 1, 0, "func_L01_004E7148"), (0x27, 1, 0, "func_L01_00385948 +1"), (0x27, 1, 0, "func_L01_0034BF98 +4"), (0x2f, 1, 0, "func_L01_0035B390 +4"), (0x2f, 1, 0, "func_L01_0034BF98 +8"), (0x37, 1, 0, "func_L01_00368FE8 +1"), (0x37, 1, 0, "func_L01_003688E8 +3"), (0x3f, 1, 0, "func_L01_003985C0 +1"), (0x3f, 1, 0, "func_L01_0031E488 +5"), (0x47, 1, 0, "func_L01_00321CB8 +1"), (0x47, 1, 0, "func_L01_0031FB90 +4"), (0x4f, 1, 0, "func_L01_0036F720 +1"), (0x4f, 1, 0, "func_L01_0036EFC0"), (0x57, 1, 0, "func_L01_0035C9F0 +4"), (0x57, 1, 0, "func_L01_0035E2F8 +6"), (0x5f, 1, 0, "func_L01_004C4F88 +1"), (0x5f, 1, 0, "func_L01_0034DFD0 +2"), (0x67, 1, 0, "func_L01_003717A8 +4"), (0x67, 1, 0, "func_L01_0034DFD0 +1"), (0x6f, 1, 0, "func_L01_00346A00 +2"), (0x6f, 1, 0, "func_L01_003444B0 +5"), (0x77, 1, 0, "func_L01_0035E5D8 +11"), (0x77, 1, 0, "func_L01_0034BF98 +5"), (0x7f, 1, 0, "func_L01_0031DB28 +7"), (0x7f, 1, 0, "func_L01_0031DF08 +5"), (0x87, 1, 0, "func_L01_00362908 +3"), (0x87, 1, 0, "func_L01_003619C8 +4"), (0x8b, 1, 0, "func_L01_00394068"), (0x8f, 1, 0, "func_L01_00380140"), (0x8f, 1, 0, "func_L01_0037DEB0 +7"), (0x97, 1, 0, "func_L01_00382E18 +4"), (0x97, 1, 0, "func_L01_003805B0 +6"), (0x98, 4, 4, "func_L01_00503930"), (0x9c, 4, 4, "func_L01_00503930")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0x27, &[(0x0, 4, 0, "fe100_func_0039EEF0 +31"), (0x0, 4, 4, "func_L01_00503930"), (0x0, 8, 0, "fe100_func_003B8D18 +3"), (0x0, 8, 32, "fe100_func_003B8D18 +3"), (0x4, 4, 4, "func_L01_00503930"), (0x8, 1, 0, "func_L07_003B1CB0 +2"), (0x8, 4, 0, "fe100_func_003E40B0 +19"), (0x8, 4, 0, "func_L01_003183C8 +10"), (0x8, 4, 4, "func_L01_00503930"), (0x8, 8, 0, "fe100_func_003B8D18 +3"), (0x8, 8, 32, "fe100_func_003B8D18 +3"), (0xc, 4, 4, "func_L01_00503930"), (0x10, 4, 0, "func_L01_00503760 +3"), (0x10, 4, 1, "func_L03_004F1168"), (0x10, 4, 4, "func_L01_00503930 +1"), (0x10, 8, 0, "fe100_func_003B8D18 +3"), (0x10, 8, 32, "fe100_func_003B8D18 +3"), (0x14, 4, 0, "fe100_func_0039AD00 +2"), (0x14, 4, 0, "func_L01_003EE988 +3"), (0x14, 4, 4, "func_L01_00503760 +1"), (0x18, 4, 0, "func_L01_00475078 +1"), (0x18, 4, 4, "func_L01_00503930"), (0x18, 8, 0, "fe100_func_003B8D18 +3"), (0x18, 8, 32, "fe100_func_003B8D18 +3"), (0x1c, 4, 0, "func_L01_00503930"), (0x1c, 4, 4, "func_L01_00503930"), (0x20, 4, 4, "func_L01_00503930"), (0x24, 4, 4, "func_L01_00503930"), (0x28, 4, 0, "func_L01_00503930"), (0x30, 1, 0, "func_L01_004E7148"), (0x30, 4, 0, "fe100_func_003A6B18 +2"), (0x30, 4, 0, "func_L01_0044A0E8"), (0x31, 1, 0, "func_L01_004E7148 +1"), (0x32, 1, 0, "func_L01_00376960 +5"), (0x32, 1, 0, "fe100_func_0039EEF0 +10"), (0x33, 1, 0, "func_L01_00498378 +103"), (0x33, 1, 0, "func_L01_0044A0E8"), (0x34, 4, 0, "fe100_func_003E46B0 +2"), (0x34, 4, 0, "func_L01_004412D0 +1"), (0x38, 4, 0, "func_L01_00304A18 +1"), (0x38, 4, 0, "func_L01_003177F8 +4"), (0x3c, 4, 0, "func_L01_002F8F60 +3"), (0x40, 1, 0, "func_L07_003B1CB0 +2"), (0x40, 2, 0, "func_L01_004B7948 +18"), (0x40, 2, 0, "fe100_func_003BBCE8 +10"), (0x42, 2, 0, "func_L28_00309590"), (0x42, 2, 0, "func_L01_00488878 +1"), (0x44, 4, 0, "func_L28_00309590"), (0x48, 4, 0, "func_L01_004BBD58 +2"), (0x48, 4, 0, "func_L01_004BBAF0"), (0x4c, 2, 0, "func_L01_004E7148"), (0x50, 1, 0, "func_L01_0050F920"), (0x50, 1, 1, "func_L01_0050F920"), (0x5a, 1, 0, "func_L01_004412D0 +1"), (0x5a, 1, 0, "func_L01_004410E8 +3"), (0x5b, 1, 0, "func_L01_004410E8"), (0x60, 4, 4, "func_L30_00356CD0 +2"), (0x70, 2, 0, "func_L30_00357258"), (0x70, 2, 0, "func_L30_00357278"), (0x74, 1, 0, "func_L30_00357278"), (0x75, 1, 0, "func_L30_00357278"), (0x76, 1, 0, "func_L30_00357278"), (0x77, 1, 0, "func_L30_00357278 +7"), (0x78, 1, 0, "func_L01_00422BD0 +1"), (0x78, 2, 0, "func_L01_004DB748 +6"), (0x78, 4, 0, "func_L01_00422BD0 +3")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0x5, &[(0x0, 1, 0, "func_L01_003135C0"), (0x0, 1, 1, "fe100_func_0039FC40 +20"), (0x1, 1, 0, "func_L01_003ADAC0"), (0x1, 1, 0, "func_L01_003135C0"), (0x2, 1, 0, "func_L09_003ECB60 +2"), (0x8, 1, 0, "func_L03_00395788 +1"), (0x8, 1, 0, "func_L03_0039BA28"), (0x9, 1, 0, "func_L03_00321C80"), (0xf, 1, 0, "func_L04_00317198"), (0x11, 1, 0, "func_L04_00317198"), (0x14, 1, 0, "func_L03_00396E88 +2"), (0x14, 1, 0, "fe100_func_00393060 +2"), (0x17, 1, 0, "func_L02_003A32D0"), (0x20, 1, 0, "func_L01_004B73B8 +3"), (0x20, 1, 0, "func_L03_003AE398"), (0x21, 1, 0, "func_L11_003ACC68"), (0x25, 1, 0, "fe100_func_0039F9A0 +2"), (0x33, 1, 0, "fe100_func_0039F9A0 +2"), (0x3c, 1, 0, "func_L01_0049B1D8"), (0x3c, 1, 0, "func_L09_003BA2E0"), (0x3d, 1, 0, "func_L01_004175C8 +35"), (0x3d, 1, 0, "func_L03_003AE398"), (0x3e, 1, 0, "func_L01_004B7370"), (0x3e, 1, 0, "fe100_func_003B8D18 +6"), (0x40, 1, 1, "fe100_func_0039FC40 +3"), (0x45, 1, 0, "func_L01_0044F470"), (0x45, 1, 0, "fe100_func_003B8D18 +4"), (0x4a, 1, 1, "func_L01_004922A0"), (0x4b, 1, 0, "func_L03_003C99E8"), (0x4c, 1, 0, "func_L03_003C99E8"), (0x4d, 1, 0, "func_L03_003C99E8"), (0x4e, 1, 0, "func_L03_003C99E8"), (0x4f, 1, 0, "func_L03_003C99E8"), (0x4f, 1, 1, "func_L03_00396378"), (0x50, 1, 0, "func_L30_0034C1A8"), (0x50, 1, 1, "func_L03_00396378"), (0x51, 1, 0, "func_L30_0034BB98"), (0x51, 1, 0, "func_L01_004922A0 +2"), (0x51, 1, 1, "func_L03_00396378"), (0x52, 1, 0, "func_L30_00357510"), (0x52, 1, 1, "func_L03_00396378"), (0x53, 1, 1, "func_L03_00396378"), (0x54, 1, 0, "fe100_func_0039F7B0 +6"), (0x54, 1, 0, "func_L01_00491890 +2"), (0x54, 1, 1, "func_L03_00396378"), (0x55, 1, 1, "func_L03_00396378"), (0x56, 1, 1, "func_L03_00396378"), (0x57, 1, 0, "func_L03_00396378"), (0x58, 1, 0, "func_L01_003B1618"), (0x71, 1, 0, "func_L01_004915D0"), (0x72, 1, 0, "func_L01_004915D0"), (0x73, 1, 0, "func_L28_00399D10"), (0x74, 1, 0, "func_L03_00396E88"), (0x74, 1, 0, "func_L08_00313B58"), (0x75, 1, 0, "func_L01_004B7948 +1"), (0x76, 1, 0, "func_L03_00321C80"), (0x77, 1, 0, "func_L03_003C9AF8"), (0x77, 1, 0, "func_L03_003C96D0"), (0x7a, 1, 0, "func_L09_003ECB60 +2"), (0x7b, 1, 0, "func_L01_00304A18 +22"), (0x7b, 1, 0, "func_L01_0038EF90 +6"), (0x7c, 1, 0, "func_L01_0034FC70 +2"), (0x7c, 1, 0, "func_L01_0039F150 +1"), (0x7e, 1, 0, "func_L01_00488878"), (0x7f, 1, 0, "func_L11_00315F10"), (0x81, 1, 0, "func_L13_003C3330"), (0x82, 1, 0, "fe100_func_00393060 +3"), (0x83, 1, 0, "func_L07_003D7790"), (0x83, 1, 0, "func_L07_003B4A30 +1"), (0x84, 1, 0, "func_L07_003D7790"), (0x84, 1, 0, "func_L07_003B1CB0"), (0x85, 1, 0, "func_L03_003A0290"), (0x85, 1, 0, "func_L01_004DD928 +2"), (0x85, 1, 1, "func_L03_0032F380"), (0x8e, 1, 0, "func_L07_003BF678 +1"), (0x8e, 1, 1, "func_L07_003BF678 +5"), (0x8f, 1, 0, "func_L07_003BF298 +2"), (0x91, 1, 0, "func_L07_003BF298"), (0x9c, 1, 0, "func_L07_003BF7E8"), (0xac, 1, 0, "func_L07_003BF7E8"), (0xad, 1, 1, "func_L07_003342A8"), (0xb0, 1, 0, "func_L02_003B3118"), (0xb0, 1, 0, "fe100_func_003B8D18 +5"), (0xb0, 1, 1, "fe100_func_003B8D18 +4"), (0xb1, 1, 0, "func_L03_003BEFE8"), (0xb1, 1, 0, "func_L01_0042D8E8"), (0xb2, 1, 0, "func_L03_003BFEF0"), (0xb3, 1, 0, "func_L03_003C0DF8"), (0xb4, 1, 0, "func_L03_003C1D00"), (0xb5, 1, 0, "func_L03_003C2C08"), (0xb6, 1, 0, "func_L03_003C3B00"), (0xb7, 1, 0, "func_L03_003C4A08"), (0xb8, 1, 0, "func_L03_003C5908"), (0xb9, 1, 0, "func_L03_003C6058"), (0xb9, 1, 0, "func_L02_00326348"), (0xba, 1, 0, "func_L03_003C60E0"), (0xba, 1, 0, "func_L01_00456960"), (0xbb, 1, 0, "func_L03_003C6168"), (0xbb, 1, 0, "func_L01_0050F920"), (0xbc, 1, 0, "func_L03_003C61F0"), (0xbc, 1, 0, "func_L07_003BF7E8"), (0xbd, 1, 0, "func_L03_003C6250"), (0xbe, 1, 0, "func_L03_003C62B0"), (0xbe, 1, 0, "func_L01_00416CC8"), (0xbf, 1, 0, "func_L01_004B75A8"), (0xc1, 1, 0, "func_L01_0047C900"), (0xc1, 1, 0, "fe100_func_003B8D18 +4"), (0xc2, 1, 0, "func_L01_0049B1D8"), (0xc3, 1, 0, "fe100_func_003B8D18 +3"), (0xc3, 1, 1, "fe100_func_003B8D18 +4"), (0xc8, 1, 0, "func_L01_004B7370"), (0xc8, 1, 0, "fe100_func_003B8D18 +4"), (0xcb, 1, 0, "fe100_func_00381BE8 +2"), (0xcc, 1, 0, "func_L01_0049B1D8"), (0xcd, 1, 0, "func_L01_0049B1D8"), (0xce, 1, 0, "func_L01_0049B1D8"), (0xcf, 1, 0, "func_L01_0049B1D8"), (0xd0, 1, 0, "func_L01_0049B1D8"), (0xd1, 1, 0, "func_L01_0049B1D8"), (0xd2, 1, 0, "fe100_func_0039F980 +5"), (0xd2, 1, 0, "func_L09_00316408"), (0xd3, 1, 0, "fe100_func_003E4778 +3"), (0xd4, 1, 0, "fe100_func_003E4778 +2"), (0xd5, 1, 0, "func_L21_003A7D98 +1"), (0xd6, 1, 0, "func_L01_004DD928 +1"), (0xd7, 1, 0, "func_L07_003B4A30 +1"), (0xd8, 1, 0, "func_L07_003B1CB0"), (0xd9, 1, 0, "func_L07_003BFAF8"), (0xdb, 1, 0, "func_L03_003A1EE8"), (0xdb, 1, 0, "func_L03_00349FA0"), (0xde, 1, 0, "func_L03_003A2C68"), (0xdf, 1, 0, "func_L01_004DD928"), (0xe2, 1, 0, "func_L01_004BE628"), (0xe3, 1, 0, "func_L03_0032C270 +3"), (0xe4, 1, 0, "func_L01_004412D0"), (0xe5, 1, 0, "func_L01_00326398"), (0xe6, 1, 0, "func_L01_00326398"), (0xe9, 1, 0, "func_L01_0042D8E8"), (0xea, 1, 0, "func_L01_0042D8E8"), (0xeb, 1, 0, "func_L01_0042D8E8")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0x9, &[(0x0, 2, 1, "func_L07_0032F698 +1"), (0x0, 4, 0, "func_L01_00440E98 +3"), (0x0, 4, 1, "fe100_func_00393F50 +15"), (0x0, 4, 4, "func_L01_0030F1F8 +17"), (0x58, 1, 0, "func_L03_0032C940 +1"), (0x58, 4, 0, "func_L07_00330370"), (0x9c, 4, 0, "func_L01_00385948")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0x2a, &[(0x0, 1, 0, "fe100_func_003B8D18 +3"), (0x0, 1, 1, "fe100_func_003B8D18 +5"), (0x10, 1, 0, "func_L01_00315638"), (0x11, 1, 0, "func_L01_00303F88"), (0x13, 1, 0, "func_L01_0031C890"), (0x27, 1, 0, "func_L01_0036A790 +1"), (0x2f, 1, 0, "func_L01_003A5928"), (0x3f, 1, 0, "func_L01_0036D5A0"), (0x5f, 1, 0, "func_L01_00372990"), (0x77, 1, 0, "func_L01_0035E5D8 +1")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0x2b, &[(0x0, 4, 4, "fe100_func_00392ED8 +123"), (0x4, 4, 0, "fe100_func_00381280 +1"), (0x20, 4, 0, "func_L30_00358298 +1"), (0x24, 1, 0, "func_L30_003584B8"), (0x24, 4, 0, "func_L30_00358700"), (0x28, 4, 0, "func_L30_00358878"), (0x64, 1, 0, "func_L01_003135C0"), (0x64, 4, 0, "func_L01_003135C0"), (0x68, 4, 0, "func_L01_004A9F80"), (0x6c, 4, 0, "func_L01_0049B1D8"), (0x6c, 4, 0, "func_L02_0034BB78"), (0x70, 4, 0, "func_L03_00321C80 +2"), (0x74, 4, 0, "func_L03_00396E88 +1"), (0x78, 4, 0, "func_L01_00488878 +1"), (0x7c, 4, 0, "func_L03_0039FFF0"), (0x88, 4, 0, "func_L04_003BDDF8"), (0x88, 4, 0, "func_L01_004A9F80"), (0x8c, 4, 0, "func_L01_004A9F80"), (0x90, 4, 0, "func_L07_003BF298"), (0x94, 4, 0, "fe100_func_0039F8A0 +3"), (0x94, 4, 0, "func_L01_0042ECC8 +2"), (0x98, 4, 0, "func_L10_00359C48"), (0x98, 4, 0, "func_L09_00316408 +1"), (0x9c, 4, 0, "func_L10_003B2688"), (0xa0, 4, 0, "func_L03_003AE398 +1"), (0xa4, 1, 0, "func_L13_003B51A8"), (0xa8, 4, 0, "func_L13_00312530 +1"), (0xac, 4, 0, "fe100_func_0039F9C8 +4"), (0xb0, 4, 0, "func_L16_00320B68"), (0xb4, 4, 0, "func_L01_004A9F80 +2"), (0xb8, 4, 0, "func_L20_003EC728 +1"), (0xbc, 4, 0, "func_L01_0049B1D8 +2"), (0xc8, 4, 0, "func_L27_0039F9D8")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0xa, &[(0x0, 1, 0, "fe100_func_003B8D18 +10"), (0x0, 1, 1, "fe100_func_0039F8A0 +36"), (0x2, 1, 0, "func_L01_004ED860 +10"), (0x5, 1, 0, "func_L01_0042B180 +5"), (0x5, 1, 0, "func_L01_0042AD60 +3"), (0x6, 1, 0, "func_L01_004200A8 +1"), (0x7, 1, 0, "func_L01_004FAA68 +3"), (0x7, 1, 0, "func_L01_00488878"), (0xb, 1, 0, "func_L01_004DE348 +3"), (0xb, 1, 0, "func_L03_003A0290"), (0xd, 1, 0, "func_L01_004E3EF0 +16"), (0xe, 1, 0, "func_L01_004E5118 +1"), (0x10, 1, 0, "func_L01_004BBAF0"), (0x11, 1, 0, "func_L01_004BBAF0"), (0x12, 1, 0, "func_L04_003E12F8"), (0x13, 1, 0, "func_L01_004BBAF0"), (0x14, 1, 0, "func_L01_00488878 +1"), (0x15, 1, 0, "func_L01_004BBAF0"), (0x16, 1, 0, "func_L01_004BBAF0 +1"), (0x16, 1, 0, "func_L03_0032C940"), (0x17, 1, 0, "func_L01_004200A8 +1"), (0x1a, 1, 0, "func_L01_00337E58 +11"), (0x1d, 1, 0, "func_L01_00501678 +4"), (0x1e, 1, 0, "func_L01_004E7148 +8"), (0x1e, 1, 0, "func_L03_003B03A8"), (0x1f, 1, 0, "fe100_func_0039F9E0 +3"), (0x1f, 1, 0, "func_L19_003D4E40 +1"), (0x20, 1, 0, "fe100_func_003E4778 +5"), (0x20, 1, 0, "fe100_func_003E40B0 +6"), (0x21, 1, 0, "func_L01_004B73B8"), (0x21, 1, 0, "func_L01_00488878 +1"), (0x22, 1, 0, "func_L03_003AE398 +1"), (0x22, 1, 0, "func_L01_00488878"), (0x23, 1, 0, "func_L19_003D10C0 +1"), (0x24, 1, 0, "func_L01_004200A8 +1")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0x28, &[(0x0, 4, 0, "fe100_func_003B8D18 +4"), (0x0, 4, 1, "fe100_func_00394918 +3"), (0x0, 4, 4, "fe100_func_003B8D18 +10"), (0x44, 4, 0, "func_L01_00304A18"), (0x58, 4, 0, "func_L01_003410E8"), (0xdc, 4, 0, "func_L01_0038EF90 +1"), (0x21c, 4, 0, "func_L01_00362908"), (0x25c, 4, 0, "func_L01_003DC180"), (0x268, 4, 4, "func_L01_0048BC68"), (0x26c, 4, 4, "func_L01_0048BC68")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0x14, &[(0x0, 4, 0, "func_L01_0042B598 +3"), (0x0, 4, 1, "func_L01_0042B6F0"), (0x0, 4, 4, "func_L01_0042B598 +3"), (0x4, 4, 0, "func_L01_0042B598 +1"), (0x4, 4, 4, "func_L01_0042B598 +1"), (0x8, 4, 4, "func_L01_0048BC68")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0xf, &[(0x0, 4, 16, "func_L01_00434F68"), (0x4, 4, 16, "func_L01_00434F68"), (0x8, 4, 16, "func_L01_00434F68"), (0xc, 1, 16, "func_L01_00434F68"), (0xc, 4, 16, "func_L04_0046C990")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0x26, &[(0x0, 4, 0, "fe100_func_00381280 +36"), (0x0, 188, 0, "fe100_func_0039C5C8 +2"), (0x4, 1, 0, "func_L01_0048EC60"), (0x4, 1, 0, "func_L01_00418478 +2"), (0x4, 2, 0, "func_L03_00396578 +3"), (0x4, 2, 0, "func_L01_004182C8 +2"), (0x5, 1, 0, "func_L01_0048EC60 +1"), (0x5, 1, 0, "func_L01_00418F00 +2"), (0x6, 1, 0, "func_L01_0048EC60"), (0x6, 1, 0, "fe100_func_0038A8A0 +12"), (0x8, 4, 0, "fe100_func_003B3858 +11"), (0x8, 4, 0, "fe100_func_003A4390 +7"), (0xc, 4, 0, "fe100_func_003A42A0 +5"), (0x10, 4, 0, "fe100_func_003A42A0 +5"), (0x14, 4, 0, "func_L04_00317198"), (0x14, 4, 0, "func_L01_004DBB60 +1"), (0x14, 4, 1, "func_L01_0049C368 +1"), (0x14, 4, 16, "func_L01_004DBB60"), (0x24, 4, 0, "func_L04_003CF6C8"), (0x34, 4, 0, "func_L04_003CF6C8"), (0x44, 4, 0, "func_L01_004DBB60"), (0x44, 4, 1, "func_L01_0049C368 +1"), (0x44, 4, 16, "func_L01_004DBB60"), (0x64, 4, 0, "func_L04_003CF6C8"), (0x74, 4, 0, "func_L01_004DBB60"), (0x74, 4, 1, "func_L01_0049C368 +3"), (0x74, 4, 16, "func_L01_004DBB60"), (0xa4, 1, 0, "func_L01_003444B0 +38"), (0xa4, 1, 0, "func_L01_0049BA68 +4"), (0xa4, 1, 1, "fe100_func_003A3E78 +2"), (0xad, 1, 0, "fe100_func_00381CC0 +11"), (0xad, 1, 0, "fe100_func_00387778 +21"), (0xae, 1, 0, "func_L01_003F07A0"), (0xae, 1, 1, "func_L01_003F07A0"), (0xb6, 1, 0, "fe100_func_00395288 +13"), (0xb6, 1, 0, "fe100_func_00393F50 +3"), (0xb7, 1, 0, "fe100_func_003A1630 +16"), (0xb7, 1, 0, "fe100_func_00381280 +17")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0x1b5d, &[(0x0, 1, 1, "fe100_func_0039FA58 +2"), (0x0, 1, 0, "fe100_func_0039F778 +6"), (0x0, 1, 1, "fe100_func_0039F778 +5"), (0x0, 1, 2, "fe100_func_0039F778 +9"), (0x0, 1, 192, "fe100_func_0039F778 +5"), (0x1, 1, 0, "fe100_func_0039FA58 +2"), (0x1, 1, 1, "fe100_func_0039FA58 +2"), (0x1, 1, 2, "fe100_func_0039FA10 +5"), (0x1, 1, 192, "fe100_func_0039FA58 +2"), (0x2, 1, 1, "fe100_func_0039FBE0 +2"), (0x2, 1, 0, "fe100_func_0039FA58 +2"), (0x2, 1, 192, "fe100_func_0039FA58 +2"), (0x3, 1, 0, "fe100_func_0039FA58 +2"), (0x3, 1, 192, "fe100_func_0039FA58 +2")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0x1b5e, &[(0x0, 1, 0, "func_0039A518 +1"), (0x0, 1, 2, "func_0039A518 +1")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0x1b5f, &[(0x0, 1, 1, "fe100_func_0039FA58 +2"), (0x0, 1, 0, "fe100_func_0039F778 +5"), (0x0, 1, 1, "fe100_func_0039F778 +5"), (0x0, 1, 2, "fe100_func_0039F778 +8"), (0x1, 1, 0, "fe100_func_0039FA58 +2"), (0x1, 1, 1, "fe100_func_0039FA58 +2"), (0x1, 1, 2, "fe100_func_0039FA10 +5"), (0x2, 1, 0, "fe100_func_0039FA58 +2"), (0x3, 1, 0, "fe100_func_0039FA58 +2")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0x2f, &[(0x0, 2, 2, "func_L01_00441170"), (0x0, 2, 4, "func_L01_00441170")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0x29, &[(0x0, 4, 1, "func_L01_00441238 +1")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0x11, &[(0x0, 2, 0, "func_L02_003137E8"), (0x18, 2, 0, "func_L01_004DD928 +1"), (0x1c, 4, 0, "func_L01_004DD928"), (0x20, 4, 0, "func_L01_004DD928"), (0x30, 2, 0, "func_L01_004DD928 +1"), (0x34, 4, 0, "func_L01_004DD928"), (0x38, 4, 0, "func_L01_004DD928"), (0x3c, 2, 0, "func_L01_004DD928 +1"), (0x40, 4, 0, "func_L01_004DD928"), (0x44, 4, 0, "func_L01_004DD928"), (0x60, 2, 0, "func_L01_003135C0 +2"), (0x64, 4, 0, "func_L01_003E7478 +1"), (0x68, 4, 0, "func_L01_003E7478"), (0x6c, 2, 0, "func_L01_004B5368"), (0x70, 4, 0, "func_L01_004B5368"), (0x74, 4, 0, "func_L01_004B5368"), (0x84, 2, 0, "func_L08_00313B58"), (0x84, 2, 0, "func_L01_004F2090 +7"), (0x88, 4, 0, "func_L01_004F2090 +7"), (0x8c, 4, 0, "func_L01_004F2090 +7"), (0xd8, 2, 0, "func_L01_004F2090 +8"), (0xdc, 4, 0, "func_L01_004F2090 +8"), (0xe0, 4, 0, "func_L01_004F2090 +8"), (0xf0, 2, 0, "func_L03_00321C80 +1"), (0xf0, 2, 0, "fe100_func_00393F50 +2"), (0xf4, 4, 0, "func_L01_004DD928"), (0xf4, 4, 0, "fe100_func_00393F50 +2"), (0xf8, 4, 0, "fe100_func_00393F50 +2"), (0xfc, 2, 0, "func_L01_004849E8 +1"), (0x100, 4, 0, "func_L01_004849E8"), (0x104, 4, 0, "func_L01_004849E8"), (0x174, 2, 0, "func_L01_005037C0"), (0x178, 4, 0, "func_L01_005037C0"), (0x17c, 4, 0, "func_L01_005037C0"), (0x198, 2, 0, "func_L07_003B87F8"), (0x198, 2, 0, "func_L01_003E8DD8 +1"), (0x19c, 4, 0, "func_L01_003E8DD8"), (0x1a0, 4, 0, "func_L01_003E8DD8"), (0x1a4, 2, 0, "func_L01_004B5368"), (0x1a8, 4, 0, "func_L01_004B5368"), (0x1ac, 4, 0, "func_L01_004B5368"), (0x1b0, 2, 0, "func_L01_00503428"), (0x1b4, 4, 0, "func_L01_00503428"), (0x1b8, 4, 0, "func_L01_00503428"), (0x1bc, 2, 0, "func_L01_004DD928 +2"), (0x1c0, 4, 0, "func_L01_003B1618 +2"), (0x1c0, 4, 0, "func_L01_003135C0 +3"), (0x1c4, 4, 0, "func_L01_004DD928 +2"), (0x1d4, 2, 0, "func_L01_004DD928 +1"), (0x1e0, 2, 0, "func_L01_004DD928"), (0x1e4, 4, 0, "func_L01_004DD928"), (0x1e8, 4, 0, "func_L01_004DD928")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0x10, &[(0x0, 2, 0, "func_L01_0050F920"), (0x0, 2, 1, "func_L01_00418478 +1"), (0x0, 2, 12, "func_L01_00417F60 +2"), (0x4, 4, 1, "func_L01_00418478"), (0x8, 4, 0, "func_L01_004180F8"), (0x8, 4, 1, "func_L01_00418478 +1"), (0x8, 4, 12, "func_L01_004180F8 +1"), (0xc, 2, 0, "func_L01_00416CC8"), (0x20, 4, 0, "func_L01_003135C0"), (0x30, 2, 0, "func_L03_00321C80"), (0x48, 2, 0, "func_L03_00321C80"), (0x54, 2, 0, "func_L03_00321C80"), (0x60, 2, 0, "func_L03_00321C80"), (0x90, 2, 0, "func_L03_00321C80"), (0x9c, 2, 0, "func_L03_00321C80"), (0xa8, 2, 0, "func_L03_00321C80"), (0xc8, 4, 0, "func_L04_00317198"), (0xe0, 4, 0, "func_L02_003137E8"), (0xe4, 2, 0, "func_L01_004DD928"), (0xfc, 2, 0, "func_L01_004DD928"), (0x10c, 4, 0, "func_L02_003137E8"), (0x110, 4, 0, "func_L09_00316408"), (0x120, 2, 0, "func_L01_003EF2D8"), (0x120, 2, 0, "fe100_func_003B8D18 +7"), (0x130, 4, 0, "func_L01_003135C0"), (0x134, 4, 0, "func_L07_003B87F8"), (0x134, 4, 0, "func_L04_00317198"), (0x144, 2, 0, "func_L01_003444B0"), (0x150, 2, 0, "func_L01_00356110"), (0x158, 4, 0, "func_L03_0032C1E8"), (0x15c, 2, 0, "func_L01_004DD928"), (0x170, 4, 0, "func_L01_003135C0"), (0x170, 4, 0, "func_L02_003137E8"), (0x188, 4, 0, "func_L01_003135C0"), (0x1a0, 4, 0, "func_L01_003135C0"), (0x1a4, 2, 0, "func_L02_003137E8"), (0x1b8, 4, 0, "func_L02_003137E8"), (0x1c4, 4, 0, "func_L02_003137E8"), (0x1c8, 2, 0, "func_L02_003137E8"), (0x1d4, 2, 0, "func_L02_003137E8"), (0x1e8, 4, 0, "func_L02_003137E8"), (0x1f0, 4, 0, "func_L02_003137E8"), (0x200, 4, 0, "func_L07_003B87F8"), (0x200, 4, 0, "func_L03_00321C80"), (0x248, 4, 0, "func_L07_003B87F8"), (0x258, 2, 0, "func_L07_003BFAF8"), (0x278, 4, 0, "func_L09_00316408"), (0x284, 4, 0, "func_L08_00313B58"), (0x29c, 4, 0, "func_L27_003A0CD0"), (0x2d4, 4, 0, "func_L01_004DD928"), (0x2d8, 4, 0, "func_L01_004DD928"), (0x308, 4, 0, "func_L01_003135C0"), (0x30c, 2, 0, "func_L03_003A2C68"), (0x318, 2, 0, "func_L03_003A2C68"), (0x324, 2, 0, "func_L03_003A2C68"), (0x344, 4, 0, "func_L27_003A0CD0"), (0x350, 4, 0, "func_L27_003A0CD0"), (0x35c, 4, 0, "func_L27_003A0CD0"), (0x374, 4, 0, "func_L27_003A02B8"), (0x428, 4, 0, "func_L08_00313B58"), (0x444, 2, 0, "func_L01_00416CC8"), (0x494, 4, 0, "func_L28_00399D10"), (0x4d4, 2, 0, "func_L01_0031FB90"), (0x4f0, 4, 0, "func_L04_00317198"), (0x4f4, 4, 0, "func_L04_00317198"), (0x54c, 2, 0, "func_L03_003A2C68"), (0x558, 2, 0, "func_L13_00312530"), (0x5a4, 4, 0, "func_L19_003D4E40"), (0x5c4, 2, 0, "func_L19_003D10C0"), (0x5c6, 2, 0, "func_L19_003D10C0"), (0x5c8, 4, 0, "func_L19_003D10C0"), (0x5fc, 4, 0, "func_L01_003B1618"), (0x608, 4, 0, "func_L09_00316408"), (0x614, 4, 0, "func_L01_003B1618"), (0x648, 2, 0, "func_L01_00356110"), (0x66c, 2, 0, "func_L02_003137E8"), (0x678, 2, 0, "func_L02_003137E8"), (0x698, 4, 0, "func_L08_00313B58"), (0x710, 4, 0, "func_L01_003135C0"), (0x714, 2, 0, "func_L02_003137E8"), (0x740, 4, 0, "func_L05_003C95B0"), (0x770, 4, 0, "func_L10_003B2688"), (0x7ac, 4, 0, "func_L16_00320B68"), (0x7d4, 2, 0, "func_L19_003D6BB0"), (0x7d8, 4, 0, "func_L19_003D6BB0"), (0x7dc, 4, 0, "func_L19_003D6BB0"), (0x80c, 4, 0, "func_L22_003186C0"), (0x834, 2, 0, "func_L03_0039BA28"), (0x834, 2, 0, "func_L03_00321C80"), (0x920, 4, 0, "func_L13_00312530"), (0x92c, 4, 0, "func_L13_00312530"), (0x974, 4, 0, "func_L04_00317198"), (0xbf4, 2, 0, "func_L08_003C5510"), (0xc0c, 2, 0, "func_L08_003C5510"), (0xc18, 2, 0, "func_L08_003C5510"), (0xc24, 2, 0, "func_L08_003C5510"), (0xc54, 2, 0, "func_L01_0033BE30"), (0xc74, 4, 0, "func_L02_003137E8"), (0xc84, 2, 0, "func_L01_0033B940"), (0xca4, 4, 0, "func_L27_003A0CD0"), (0xcc8, 4, 0, "func_L09_00316408"), (0xcd4, 4, 0, "func_L05_003C95B0 +1"), (0xd10, 4, 0, "func_L16_00320B68"), (0xd20, 2, 0, "func_L08_003C5510"), (0xd98, 2, 0, "func_L01_003688E8"), (0xda4, 2, 0, "func_L01_003DE4F0"), (0xe20, 4, 0, "func_L04_00317198"), (0xe28, 2, 0, "func_L11_00315F10"), (0xeac, 2, 0, "func_L19_003D4E40"), (0xec0, 4, 0, "func_L04_00317198"), (0xef0, 4, 0, "func_L09_00316408"), (0xefc, 4, 0, "func_L01_003135C0"), (0xffc, 2, 0, "func_L01_00416CC8"), (0x100c, 4, 0, "func_L04_00317198"), (0x1010, 4, 0, "func_L04_00317198"), (0x101c, 4, 0, "func_L04_00317198"), (0x1020, 2, 0, "func_L03_003A2C68"), (0x1058, 4, 0, "func_L02_003137E8"), (0x105c, 2, 0, "fe100_func_0039F9E0 +2"), (0x1074, 2, 0, "func_L01_003444B0"), (0x1098, 2, 0, "func_L02_00325618")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0x3f2, &[(0x0, 2, 0, "func_L01_00419640"), (0x0, 2, 0, "func_L01_0049F380"), (0x0, 2, 1, "func_L01_0049EB98 +2"), (0x0, 2, 2, "func_L01_0049F380"), (0x2, 2, 0, "func_L01_00419640"), (0x2, 2, 2, "func_L01_00419640")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0xbba, &[(0x0, 1, 1, "fe100_func_0038E048 +8"), (0x1, 1, 1, "func_L01_0042DC68")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0xbbc, &[(0x0, 1, 1, "func_L01_003135C0 +73"), (0x0, 1, 1, "func_L01_00428750")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0xbbd, &[(0x0, 1, 1, "func_L01_00304A18 +25")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0xfa5, &[(0x0, 4, 1, "func_L01_00325A60 +5"), (0x0, 4, 4, "func_L01_003FE420")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0x20, &[(0x0, 4, 0, "fe100_func_0039AD00 +11"), (0x0, 4, 0, "func_L01_00510748 +1"), (0x0, 4, 1, "func_L01_004E8830 +1"), (0x0, 4, 4, "func_L01_004E8830 +2"), (0x4, 4, 0, "func_L01_004E7148 +3"), (0x8, 4, 0, "func_L01_004E7148 +1"), (0xc, 4, 0, "func_L01_004E7148 +1")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0xd, &[(0x0, 4, 0, "func_L01_00416A10"), (0x0, 4, 0, "func_L01_00416A10 +2"), (0x0, 4, 4, "fe100_func_00393E68 +5"), (0x0, 4, 32, "fe100_func_00393E68 +2"), (0x4, 4, 0, "func_L01_00416A10"), (0x4, 4, 4, "func_L01_00416A10")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0x1b5a, &[(0x0, 4, 0, "func_L01_00416A10"), (0x0, 4, 0, "func_L01_00416A10 +2"), (0x0, 4, 4, "func_L01_00416A10 +2"), (0x4, 4, 0, "func_L01_00416A10"), (0x4, 4, 4, "func_L01_00416A10")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0x7, &[(0x0, 1, 0, "fe100_func_003B8D18 +4"), (0x0, 1, 1, "fe100_func_003B8D18 +4"), (0x1, 1, 0, "func_L01_0050BB98 +14"), (0x7, 1, 0, "func_L30_004E1888"), (0x8, 1, 0, "func_L01_004DCC10 +8"), (0x9, 1, 0, "fe100_func_003E5950 +15"), (0xa, 1, 0, "func_L01_004E2A30 +1"), (0xb, 1, 0, "func_L01_0044EB78 +1"), (0xc, 1, 0, "fe100_func_003E3D20 +3"), (0xd, 1, 0, "func_L01_003EE988 +1"), (0xe, 1, 0, "func_L01_003B7330 +2"), (0xf, 1, 0, "fe100_func_003865A8 +15"), (0x11, 1, 0, "func_L01_00338088 +1")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0x25, &[(0x0, 1, 0, "fe100_func_003B8D18 +3"), (0x0, 1, 1, "fe100_func_003B8D18 +3")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0x8, &[(0x0, 1, 0, "fe100_func_003B8D18 +6"), (0x0, 1, 1, "fe100_func_003B8D18 +7"), (0x9, 1, 0, "func_L07_003BF7E8"), (0x10, 1, 0, "func_L13_003C3330")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0xc, &[(0x0, 2, 0, "fe100_func_003B8D18 +6"), (0x0, 2, 1, "func_L01_0041FF98"), (0x0, 2, 2, "fe100_func_003B8D18 +6"), (0x2, 2, 0, "func_L01_0041FF98")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0xe, &[(0x0, 1, 1, "func_L01_00449A50 +3"), (0x3, 1, 0, "func_L01_004A87D0"), (0xb, 1, 0, "func_L13_00312530"), (0xd, 1, 0, "func_L07_003BF298 +1"), (0x15, 1, 0, "func_L11_003AC568"), (0x1e, 1, 0, "func_L01_00491890"), (0x1f, 1, 0, "func_L01_00488878 +2"), (0x20, 1, 0, "fe100_func_0039F848 +4"), (0x20, 1, 0, "func_L03_00321C80"), (0x20, 1, 1, "func_L03_00321C80"), (0x21, 1, 0, "func_L01_00488878 +3"), (0x22, 1, 0, "func_L01_00488878 +1"), (0x23, 1, 0, "func_L01_00488878 +1")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
    (0x1b5b, &[(0x0, 2, 0, "fe100_func_003B8D18 +3"), (0x0, 2, 2, "fe100_func_003B8D18 +5")], "scanned: boot_elf, all 34 story overlays, both front ends (flow-sensitive pointer scan seeded at every lui/addiu/ori that forms an address in the variable and every constant-address pointer table; calls, stack spills and jump tables followed); bytes no listed field reaches are read by no load"),
];
