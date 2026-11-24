//! The LEVEL level data's GS RAM image (part 2), HUD header (part 3) and HUD banks (parts 4-8).
//!
//! GS RAM image: func_L01_00428750 hands part 2 to func_L01_00423B48 with the core index's GS RAM table (index word 1 is its
//! offset; words 0 and 0x21 are its two counts). The 16-byte entries are +0 format (0x13 or 0 in the first table), +4 width
//! (u16) and height (s16), +8 and +0xC offsets of the image in part 2. For each entry of the first table the function builds a
//! GS image-transfer packet (func_0012C9A0) and sends the image from part 2 +0xC (func_0012CCC8): format 0x13 is an 8-bit
//! (PSMT8) width x height image whose GS block slot is max(width x height, 0x100) bytes; any other format (0 on disc) is a
//! fixed 16 x 16 32-bit image, 0x400 bytes. The second table (14 entries) is sent as 8-bit images (the format word is not
//! read) from +8, one after another.
//!
//! HUD: func_L01_004233F8 (the HUD loader, called from func_L01_00428750) copies part 3, the HUD header, to a heap block
//! (0x1DFD98) and points 0x1DFD9C, 0x1DFDA0, 0x1DFDA8 and 0x1DFDA4 at its four tables (offsets +4, +8, +0xC, +0x10). Banks 0
//! and 1 are unpacked (func_L01_00423698 through func_L01_00433EA0); banks 2 to 4 are unpacked on demand by func_L01_00419A68.
//! func_L01_00419C38 turns each table pointer's bank offset (bit 31 set on disc) into an address when a bank is loaded;
//! func_L01_00419D98 turns it back. The tables are read by the HUD sprite code (func_L01_00419888, func_L01_0041A3F0,
//! func_L01_0041D298, func_L01_0041D880, func_L01_0041D950, func_L01_0041E1B0).

use crate::disc::{u16_at, u32_at};
use crate::rd::Fields;
use crate::track::{Node, Space, Tracker};
use std::cell::RefCell;

const GG: &str = "LEVEL GS RAM image";
const GH: &str = "LEVEL HUD header";
const GB: &str = "LEVEL HUD banks";

const UPLOAD: &str = "func_L01_00423B48 (func_L01_00428750 calls it with part 2 and the core index's GS RAM table; func_0012C9A0 builds the GS transfer packet, func_0012CCC8 sends it)";

/// What the walk needs of the header when it reaches the banks: per bank, its unpacked size, its palettes' and its textures'
/// offsets.
#[derive(Clone, Default)]
struct Bank {
    size: u64,
    palettes: Vec<u64>,
    /// (offset, log2 width, log2 height)
    textures: Vec<(u64, u32, u32)>,
}

thread_local! {
    static HEADER: RefCell<Option<Vec<Bank>>> = const { RefCell::new(None) };
}

/// A HUD texture: its bank and its texels' offset there (8-bit, 2^`lw` x 2^`lh` texels).
#[derive(Clone, Debug)]
pub struct HudTexture {
    pub bank: usize,
    pub offset: usize,
    pub lw: u32,
    pub lh: u32,
}

/// The HUD the port loads: the header's palette table (each palette's bank and offset: 256 PSMCT32 colours, 0x400 bytes) and
/// texture table, where in the header each table is (its 8-byte entries' +4 is the GS block func_L01_0041D950 writes when it
/// uploads the entry: the header's copy in RAM, `levelcode::LevelGlobals::hud_header`, holds them), and the unpacked banks
/// (None for an empty one).
#[derive(Clone, Default)]
pub struct HudData {
    pub palettes: Vec<(usize, usize)>,
    pub textures: Vec<HudTexture>,
    pub palette_table: usize,
    pub texture_table: usize,
    pub banks: Vec<Option<Vec<u8>>>,
}

/// What a HUD part gives: the header's tables, or a bank's unpacked bytes.
pub enum HudPart {
    Header(HudData),
    Bank(usize, Option<Vec<u8>>),
}

/// An image of the GS RAM table's first loop: func_L01_00423B48 sends it to GS block (base + `offset` / 256), the base being
/// the word at 0x1A1EF0 that func_L01_00424DE8 also adds to the texture records' palette and mip addresses.
#[derive(Clone, Debug)]
pub struct GsRamImage {
    /// Offset in part 2 (a multiple of 0x100).
    pub offset: usize,
    /// A 16 x 16 32-bit image (a 256-colour palette, PSMCT32); otherwise an 8-bit (PSMT8) `width` x `height` image.
    pub clut: bool,
    pub width: usize,
    pub height: usize,
}

/// The GS RAM image (level data part 2) at `at` in `parent`; `index` is the core index (part 1). Returns the first loop's
/// images.
pub fn gs_ram(t: &mut Tracker, parent: &mut Node, at: u64, b: &[u8], index: &[u8]) -> Result<Vec<GsRamImage>, String> {
    let n0 = u32_at(index, 0)? as usize;
    let tab = u32_at(index, 4)? as usize;
    let n1 = u32_at(index, 0x84)? as usize;
    if tab + 16 * (n0 + n1) > index.len() {
        return Err(format!("GS RAM table {n0}+{n1} entries at {tab:#x} past the index's {:#x} bytes", index.len()));
    }
    let mut region = t.region(parent, GG, at, b.len() as u64);
    let mut clut = Vec::new();
    let mut first = Vec::new();
    let mut second = Vec::new();
    let mut next = 0u64; // the next image's offset in part 2
    let mut sum = 0u64; // the second table's running width x height
    let mut images = Vec::new();
    for k in 0..n0 + n1 {
        let e = tab + 16 * k;
        let (fmt, wh, o8, oc) = (u32_at(index, e)?, u32_at(index, e + 4)?, u32_at(index, e + 8)? as u64, u32_at(index, e + 0xc)? as u64);
        let (w, h) = ((wh & 0xffff) as u64, (wh >> 16) as u64);
        if k < n0 {
            // The first loop: +0 selects the transfer, +0xC is the image.
            let (len, stride, list) = match fmt {
                0x13 if w > 0 && h > 0 => (w * h, (w * h).max(0x100), &mut first),
                0 if w == 0 && h == 0 => (0x400, 0x400, &mut clut),
                _ => return Err(format!("GS RAM table entry {k}: format {fmt:#x}, {w} x {h}")),
            };
            // +8 is not read; it repeats +0xC.
            if o8 != oc || oc != next {
                return Err(format!("GS RAM table entry {k}: offsets {o8:#x}, {oc:#x}, expected {next:#x}"));
            }
            list.push((oc, len));
            images.push(GsRamImage { offset: oc as usize, clut: fmt == 0, width: w as usize, height: h as usize });
            next = oc + stride;
        } else {
            // The second loop: the format is not read (0x13 on disc), +8 is the image, +0xC the running width x height.
            if fmt != 0x13 || w == 0 || h == 0 || o8 != next || oc != sum {
                return Err(format!("GS RAM table entry {k}: format {fmt:#x}, {w} x {h}, offsets {o8:#x}, {oc:#x}, expected {next:#x}, {sum:#x}"));
            }
            second.push((o8, w * h));
            next = o8 + w * h;
            sum += w * h;
        }
    }
    if next != b.len() as u64 {
        return Err(format!("GS RAM images end at {next:#x}, the part is {:#x} bytes", b.len()));
    }
    for (name, pieces) in [
        ("16 x 16 32-bit images (0x400 bytes each: 256 colours)", &clut),
        ("8-bit images, first table (width x height texels; an 8 x 8 image fills a 0x100-byte slot)", &first),
        ("8-bit images, second table (width x height texels)", &second),
    ] {
        if pieces.is_empty() {
            continue;
        }
        let l = t.line(GG, name, "hud::gs_ram", UPLOAD, Space::Disc);
        let mut n = t.pieces(&mut region, l, pieces);
        let total = n.size;
        n.mark(0, total);
        t.close(n);
    }
    crate::walk::pad_mem(t, &mut region, b, GG);
    t.close(region);
    Ok(images)
}

/// A HUD part: 3 is the header, 4 to 8 are banks 0 to 4. The header (part 3) must come first: the banks are split by its
/// tables.
pub fn hud(t: &mut Tracker, parent: &mut Node, part: usize, at: u64, b: &[u8]) -> Result<HudPart, String> {
    match part {
        3 => header(t, parent, at, b),
        4..=8 => bank(t, parent, part - 4, at, b),
        _ => Err(format!("level data part {part} is not a HUD part")),
    }
}

/// The header's fixed part, in bytes.
const FIXED: usize = 0xb4;

fn header(t: &mut Tracker, parent: &mut Node, at: u64, b: &[u8]) -> Result<HudPart, String> {
    let (n1, n2) = (u16_at(b, 0)? as usize, u16_at(b, 2)? as usize);
    let off = |i: usize| u32_at(b, i).map(|v| v as usize);
    let (t1, t2, t3, t4) = (off(4)?, off(8)?, off(0xc)?, off(0x10)?);
    let cum3: Vec<usize> = (0..5).map(|i| off(0x14 + 4 * i)).collect::<Result<_, _>>()?;
    let cum4: Vec<usize> = (0..5).map(|i| off(0x34 + 4 * i)).collect::<Result<_, _>>()?;
    let sizes: Vec<u64> = (0..5).map(|i| off(0x54 + 4 * i).map(|v| v as u64)).collect::<Result<_, _>>()?;
    let (c3, c4) = (cum3[4], cum4[4]);
    if t1 != FIXED || t2 != t1 + 8 * n1 || t3 != t2 + 4 * n2 || t4 != t3 + 8 * c3 || t4 + 8 * c4 != b.len() {
        return Err(format!("HUD header: tables at {t1:#x}, {t2:#x}, {t3:#x}, {t4:#x} for counts {n1}, {n2}, {c3}, {c4} in {:#x} bytes", b.len()));
    }
    if cum3.windows(2).any(|w| w[0] > w[1]) || cum4.windows(2).any(|w| w[0] > w[1]) {
        return Err(format!("HUD header: per-bank counts {cum3:?}, {cum4:?} are not cumulative"));
    }

    // The fixed part.
    let l = t.line(
        GH,
        "HUD header: table offsets, per-bank table counts and bank sizes (45 words)",
        "hud::hud",
        "func_L01_004233F8 (+4, +8, +0xC, +0x10 table offsets; +0x54 to +0x64 bank sizes, also func_L01_00419A68), func_L01_00419C38 and func_L01_00419D98 (+0x14 to +0x24 and +0x34 to +0x44 cumulative palette and texture counts per bank; +0x74 to +0x84 written by func_L01_00419C38, +0x94 to +0xA4 by func_L01_004233F8); +0 and +2 (table counts) and the zero words: nothing reads them (checked by the decoder)",
        Space::Disc,
    );
    let mut n = t.child(parent, l, at, FIXED as u64);
    let mut f = Fields::new(b, &mut n);
    f.mark(0, 4);
    for i in [4usize, 8, 0xc, 0x10] {
        f.u32(i)?;
    }
    for i in 0..5 {
        f.u32(0x14 + 4 * i)?;
        f.u32(0x34 + 4 * i)?;
        f.u32(0x54 + 4 * i)?;
    }
    f.fill(0x28, 12, 0, "words between the palette and texture counts")?;
    f.fill(0x48, 12, 0, "words between the texture counts and bank sizes")?;
    f.fill(0x68, 12, 0, "words after the bank sizes")?;
    f.fill(0x74, FIXED - 0x74, 0, "bank bases (written when a bank loads), unused words and queue handles (written by the loader)")?;
    t.close(n);

    // The sprite table: 8-byte entries ending with id 0xFFFF; each entry's frames are a run of the frame table.
    let l = t.line(
        GH,
        "HUD sprite table (8-byte entries: id u16, frame count u16, first frame u16, kind u8, tick divisor u8; ends with id 0xFFFF)",
        "hud::hud",
        "func_L01_00419888 (+0 id, searched to the 0xFFFF entry), func_L01_0041A3F0 (+0 to +6), func_L01_0041D298 (+2 count, +4 first frame, +7 divisor), func_L01_0041D880",
        Space::Disc,
    );
    let mut n = t.child(parent, l, at + t1 as u64, 8 * n1 as u64);
    let mut frame = 0usize;
    for k in 0..n1 {
        let e = t1 + 8 * k;
        let (id, count, first) = (u16_at(b, e)?, u16_at(b, e + 2)? as usize, u16_at(b, e + 4)? as usize);
        let last = k + 1 == n1;
        if (id == 0xffff) != last || (last && (count != 0 || first != 0 || b[e + 6] != 0 || b[e + 7] != 0)) || (!last && first != frame) {
            n.fail(format!("entry {k}: id {id:#x}, {count} frames from {first}, expected the next frame {frame}"));
            break;
        }
        frame += count;
        n.mark((e - t1) as u64, 8);
    }
    if frame != n2 && !n.failed() {
        n.fail(format!("sprite entries run over {frame} frames, the frame table has {n2}"));
    }
    t.close(n);

    // The frame table.
    let l = t.line(
        GH,
        "HUD frame table (4-byte entries: palette index u16, texture index u16)",
        "hud::hud",
        "func_L01_0041D880 (+0 palette index, +2 texture index), func_L01_0041D950 (resolves both and queues their uploads), func_L01_0041E1B0 (+2)",
        Space::Disc,
    );
    let mut n = t.child(parent, l, at + t2 as u64, 4 * n2 as u64);
    for k in 0..n2 {
        let e = t2 + 4 * k;
        let (p, x) = (u16_at(b, e)? as usize, u16_at(b, e + 2)? as usize);
        if p >= c3 || x >= c4 {
            n.fail(format!("frame {k}: palette {p} of {c3}, texture {x} of {c4}"));
            break;
        }
        n.mark((e - t2) as u64, 4);
    }
    t.close(n);

    // The palette table and the texture table: 8-byte entries, a bank offset with bit 31 set.
    let l3 = t.line(
        GH,
        "HUD palette table (8-byte entries: offset in the bank | 0x80000000, GS address u16 written at run time, 2 unused bytes)",
        "hud::hud",
        "func_L01_00419C38 (+0, relocates it when the bank loads), func_L01_00419D98 (restores it), func_L01_0041D950 (+4 GS address written when the palette is uploaded), func_L01_00401978 (clears +4); +6: nothing reads it (checked by the decoder)",
        Space::Disc,
    );
    let mut n3 = t.child(parent, l3, at + t3 as u64, 8 * c3 as u64);
    let l4 = t.line(
        GH,
        "HUD texture table (8-byte entries: offset in the bank | 0x80000000, GS address u16 written at run time, log2 width u8, log2 height u8)",
        "hud::hud",
        "func_L01_00419C38 (+0, relocates it when the bank loads), func_L01_00419D98 (restores it), func_L01_0041D950 (+4 GS address written when the texture is uploaded, +6 and +7 sizes), func_L01_0041E1B0 (+6, +7), func_L01_00401978 (clears +4)",
        Space::Disc,
    );
    let mut n4 = t.child(parent, l4, at + t4 as u64, 8 * c4 as u64);
    let mut banks = vec![Bank::default(); 5];
    for (k, bank) in banks.iter_mut().enumerate() {
        bank.size = sizes[k];
        let (a3, a4) = (if k == 0 { 0 } else { cum3[k - 1] }, if k == 0 { 0 } else { cum4[k - 1] });
        for i in a3..cum3[k] {
            let e = t3 + 8 * i;
            let (o, rt) = (u32_at(b, e)?, u32_at(b, e + 4)?);
            if o & 0x8000_0000 == 0 || rt != 0 {
                n3.fail(format!("entry {i}: {o:#x}, {rt:#x}"));
                break;
            }
            n3.mark((e - t3) as u64, 8);
            bank.palettes.push((o & 0x7fff_ffff) as u64);
        }
        for i in a4..cum4[k] {
            let e = t4 + 8 * i;
            let (o, rt) = (u32_at(b, e)?, u16_at(b, e + 4)?);
            if o & 0x8000_0000 == 0 || rt != 0 || b[e + 6] > 9 || b[e + 7] > 9 {
                n4.fail(format!("entry {i}: {o:#x}, {rt:#x}, log2 {}, {}", b[e + 6], b[e + 7]));
                break;
            }
            n4.mark((e - t4) as u64, 8);
            bank.textures.push(((o & 0x7fff_ffff) as u64, b[e + 6] as u32, b[e + 7] as u32));
        }
    }
    t.close(n3);
    t.close(n4);
    let data = HudData {
        palettes: banks.iter().enumerate().flat_map(|(k, bk)| bk.palettes.iter().map(move |&o| (k, o as usize))).collect(),
        textures: banks.iter().enumerate().flat_map(|(k, bk)| bk.textures.iter().map(move |&(o, lw, lh)| HudTexture { bank: k, offset: o as usize, lw, lh })).collect(),
        palette_table: t3,
        texture_table: t4,
        banks: vec![None; 5],
    };
    HEADER.with(|h| *h.borrow_mut() = Some(banks));
    Ok(HudPart::Header(data))
}

/// The empty stream every LEVEL file holds for a bank of size 0 (also part 4 of levels 30-36).
const EMPTY: [u8; 16] = [0x57, 0x41, 0x44, 0x10, 0, 0, 0, 0, 9, 0, 2, 0, 0, 1, 8, 0];

fn bank(t: &mut Tracker, parent: &mut Node, k: usize, at: u64, b: &[u8]) -> Result<HudPart, String> {
    let bk = HEADER.with(|h| h.borrow().as_ref().map(|v| v[k].clone())).ok_or("HUD bank before the HUD header")?;
    if bk.size == 0 {
        let l = t.line(
            GB,
            "empty bank: a 16-byte LZ stream header (size 0x10) with no data",
            "hud::hud",
            "func_L01_004233F8 and func_L01_00419A68 skip a bank whose size word (+0x54 to +0x64 of the HUD header) is 0; nothing reads these bytes (checked by the decoder)",
            Space::Disc,
        );
        let mut n = t.child(parent, l, at, b.len() as u64);
        if b.len() != 16 {
            n.fail(format!("an empty bank is {:#x} bytes", b.len()));
        } else {
            Fields::new(b, &mut n).same(0, &EMPTY, "empty LZ stream header")?;
        }
        t.close(n);
        return Ok(HudPart::Bank(k, None));
    }
    let l = t.line(
        GB,
        &format!("HUD bank {k}: LZ stream"),
        "lz::unpack",
        "func_L01_00433EA0 (each program's copy; called by func_L01_00423698 for banks 0 and 1 from func_L01_004233F8, and by func_L01_00419A68 for banks 0 and 2 to 4)",
        Space::Disc,
    );
    let group = format!("LEVEL HUD bank {k} (unpacked)");
    let g = group.clone();
    let data = crate::walk::lz_data(t, parent, l, b, at, b.len() as u64, &group, move |t, root, data| {
        if data.len() as u64 != bk.size {
            return Err(format!("HUD bank {k} unpacks to {:#x} bytes, the header says {:#x}", data.len(), bk.size));
        }
        let pal: Vec<(u64, u64)> = bk.palettes.iter().map(|&o| (o, 0x400)).collect();
        let mut tex: Vec<(u64, u64)> = bk.textures.iter().map(|&(o, w, h)| (o, (1u64 << w) * (1u64 << h))).collect();
        tex.sort_unstable();
        for (name, game, pieces) in [
            ("HUD palettes (256 x 32-bit colours, 0x400 bytes each)", "func_L01_0041D950 queues the upload of each palette to the GS (0x400 bytes)", pal),
            (
                "HUD texture texels (8-bit: 2^w x 2^h)",
                "func_L01_0041D950 queues the upload of each texture to the GS (8-bit, PSMT8; sizes from the texture table's +6 and +7)",
                tex,
            ),
        ] {
            if pieces.is_empty() {
                continue;
            }
            if let Some(&(o, n)) = pieces.iter().find(|&&(o, n)| o + n > root.size) {
                return Err(format!("HUD bank {k}: {name} at {o:#x}+{n:#x} past the bank's {:#x} bytes", root.size));
            }
            let l = t.line(&g, name, "hud::hud", game, Space::Unpacked);
            let mut n = t.pieces(root, l, &pieces);
            let total = n.size;
            n.mark(0, total);
            t.close(n);
        }
        Ok(data.to_vec())
    })?;
    Ok(HudPart::Bank(k, data))
}
