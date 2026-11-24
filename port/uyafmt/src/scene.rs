//! The unpacked SCENE blocks and chunks, split into structures as R&C3's own code reads them.
//!
//! A SCENE block (func_L01_00429450 loads it, func_L01_004294D0 unpacks it and walks it):
//! - 16-byte entries (class id, offset of a texture list or 0, offset of the class data, a zero word) until an entry whose
//!   first word is negative (the terminator, -1 in all four words); func_L01_004294D0 calls func_L01_0042A598 per entry;
//! - a texture list (a count, then offsets from the list's start) and, at those offsets, 2FIP images (func_L01_0042A598: the
//!   palette goes to the GS from +0x20, the pixels from +0x420 on);
//! - a moby class (func_L01_00424368 registers it, func_L01_00424048 relocates it), decoded by moby.rs; it runs to the next
//!   entry's class or texture list.
//!
//! A SCENE chunk (func_L01_00429740 unpacks it and sets the mobies up; func_L01_004B66E0 plays one per 0x60 ticks):
//! - a 0x14-byte header, an offset per object, the camera records (0x20 bytes per tick, func_L01_004B6518) and the objects, in
//!   the order of their offsets;
//! - an object: a 0x10-byte record header, an animation sequence header (0x1C bytes), one offset per frame (the count is the
//!   byte at +0x20), zero padding, the frames, and the position records (16 bytes per frame; the object record's +0xC is their
//!   offset in the chunk, func_L01_0040BE90 reads them). A frame runs to the next frame's offset; the last runs to the position
//!   records. After the last object a chunk may hold stale bytes (a prefix of one 9216-byte block, the same in every chunk
//!   that has it), which nothing reads (ignored). The chunk's camera records are one per tick of the chunk plus one: the tick
//!   counter that indexes them stops one short, so the last record is never read (ignored).
//!
//! `block` returns the block's classes (`SceneBlock`): each class as `moby::MobyClass` with its id, its offset in the unpacked block
//! and its images (decoded with `pif::texture`). In the game's RAM the unpacked block is at the address the level's variable
//! `S + 0x58` holds after func_L01_004294D0 has run (see `scenecode`), so class `c`'s packet A of mesh `m` is at that address plus
//! `c.offset + m.a_at`: func_L01_00424048 relocates the class in place at its RAM address.
//!
//! Texture numbers: func_L01_0042A598 registers entry `e`'s images one after the other: image `i` of the entry's list gets the
//! global texture number `g = [0x1E008C] at that moment` (the count of moby textures the core registered, then one more per scene
//! image: Veldin's `iRam001e008c`, a level variable), stores `g` as byte `i` of a local 16-byte table (`auStack_1c0`, at most the first 8
//! bytes are initialised; the list's count is not checked against 16) and a 16-byte record per image (+4 width, +6 height, +8 levels
//! clamped to 3, +0xA the palette's GS block) at record `g` of a local table, and then calls func_L01_00424368 with that table and the
//! byte table, which passes both to func_L01_00424048 and so to func_L01_00423E10 (`a1` the record table, `a2` the byte table):
//! per texture setup it reads word 8 (`local`), maps a non-negative one through the byte table (0x423E8C `lbu t1, 0(s7 + t1)`) to
//! `g`, and func_L01_00423700 builds the setup's TEX0 from the record at table + 16 * g. So setup number `local` is image `local`
//! of the entry's list: `SceneClass::image`. A class whose entry has no list (word 1 zero) has no images; a setup of such a class
//! with a non-negative number would name a record the loader never wrote, and `image` returns an error for it. -1 to -3 are the
//! special register sets of func_L01_00423700 (not images).
//!
//! What stays undecoded here: the animation frames of the chunk objects (VU0 data); anim.rs reads their known fields.

use crate::disc::u32_at;
use crate::rd::Fields;
use crate::track::{Node, Space, Tracker};
use std::collections::BTreeSet;

const GB: &str = "SCENE block (unpacked)";
const GC: &str = "SCENE chunk (unpacked)";
const DB: &str = "scene::block";
const DC: &str = "scene::chunk";

const ENTRY_FN: &str = "func_L01_004294D0 (same in all 34 story levels)";
const TEX_FN: &str = "func_L01_0042A598 (same in all 34 story levels)";
const CHUNK_FN: &str = "func_L01_00429740 (same in all 34 story levels)";
const CAMERA_FN: &str = "func_L01_004B6518 (same in all 34 story levels)";
const POSITION_FN: &str = "func_L01_004B66E0 and func_L01_0040BE90 (same in all 34 story levels)";
const TEX_HDR_FN: &str = "func_L01_0042A598 (same in all 34 story levels; +8, +0xC, +0x10 .. +0x1C); +4: no reader in func_L01_0042A598, func_L01_0042AA88, frontbin func_00395EB8, func_00393E90, func_00393A18 or front end func_003824F0 (they read +8, +0xC, +0x14, +0x1C, +0x20 and +0x420 only)";
const LEVEL1_WHY: &str = "func_L01_0042A598 starts level 2 at 5/4 of level 0's size, so it steps over level 1 without reading or uploading it; no other loader of a 2FIP image touches a level but 0 and 2";
const LEVEL3_WHY: &str = "func_L01_0042A598 uploads levels 0 and 2 only (the level count is clamped to 3); nothing addresses a level 3";
const HEAD_FN: &str = "func_L01_00429740 (+0 class, +0xC offset of the position records; +4 and +8: no reader: func_L01_00429740 keeps the record pointer in a local and stores only +0x10 (the sequence header) and the +0xC offset, func_L01_004B66E0, func_L01_004B6518, func_L01_0040BE90 and func_L01_00436350 never load from the record header)";
const SEQ_FN: &str = "func_L01_00436D30, func_L01_004389D8, func_L01_00435F98, func_L01_00436350, func_L01_00438180, func_L01_00437500, func_L01_00423DB8, func_L01_00450D80 and func_L01_00450FE0 (+0x10 .. +0x1B of the header; +0 .. +0xF: no reader in any of them, all loads through the sequence pointer are +0x10 on); func_L01_00429740 relocates the frame offsets and func_L01_00436350 and func_L01_004B66E0 read them";
const CAM_WHY: &str = "func_L01_004B6518 and func_L01_004A9F80 read the record indexed by the tick inside the chunk (0 to 0x5F), the counter func_L01_004B66E0 resets at 0x60; a chunk holds one record more than it has ticks, the last is never indexed (checked: records = ticks in the chunk + 1 in every chunk)";
const TAIL_WHY: &str = "every reader indexes by the object offsets, the camera records (one per tick) and the position records (the object record's +0xC, 16 bytes per frame: func_L01_004B66E0, func_L01_0040BE90); none reaches past the last object's position records";
const OFFSETS_FN: &str = "func_L01_00429740 relocates them; func_L01_00436350 and func_L01_004B66E0 read them (same in all 34 story levels)";

fn line(t: &mut Tracker, group: &str, name: &str, decoder: &'static str, game: &'static str) -> usize {
    t.line(group, name, decoder, game, Space::Unpacked)
}

/// A child of `root` at `at`, `size` bytes, checked against the root's bytes and the structures already placed.
fn put(t: &mut Tracker, root: &mut Node, line: usize, at: usize, size: usize) -> Result<Node, String> {
    if at + size > root.size as usize {
        return Err(format!("{}: {} of {size:#x} bytes at {at:#x} past the {:#x} bytes", root.place, t.lines[line].name, root.size));
    }
    if root.overlaps(at as u64, size as u64) {
        return Err(format!("{}: {} at {at:#x}+{size:#x} overlaps another structure", root.place, t.lines[line].name));
    }
    Ok(t.child(root, line, at as u64, size as u64))
}

/// Closes a node; a decoder error becomes the node's failure.
fn done(t: &mut Tracker, mut n: Node, r: Result<(), String>) {
    if let Err(e) = r {
        n.fail(e);
    }
    t.close(n);
}

fn byte(b: &[u8], at: usize) -> Result<usize, String> {
    b.get(at).map(|&v| v as usize).ok_or_else(|| format!("byte +{at:#x} past the end {:#x}", b.len()))
}

fn word(b: &[u8], at: usize) -> Result<usize, String> {
    u32_at(b, at).map(|v| v as usize)
}

// ---- the block ----

/// One class of a SCENE block, as func_L01_004294D0 registers it.
pub struct SceneClass {
    /// The entry's first word: the class id func_L01_0042A598 registers it under (instances name it).
    pub id: u32,
    /// The class data's offset in the unpacked block (the entry's third word); the class's RAM address is the block's plus this.
    pub offset: usize,
    pub class: crate::moby::MobyClass,
    /// The entry's texture list's images in list order (level 0 as the GS samples it; empty when the entry has no list).
    pub images: Vec<crate::core::Texture>,
}

impl SceneClass {
    /// The image a texture setup's number names: `local` is the index in the entry's texture list (see the module doc).
    pub fn image(&self, local: i32) -> Result<&crate::core::Texture, String> {
        usize::try_from(local).ok().and_then(|i| self.images.get(i)).ok_or_else(|| format!("scene class {}: texture number {local} with {} images", self.id, self.images.len()))
    }
}

/// A SCENE block's classes in entry order.
pub struct SceneBlock {
    pub classes: Vec<SceneClass>,
}

/// A SCENE block: `root` is its unpacked root, `d` its bytes.
pub fn block(t: &mut Tracker, root: &mut Node, d: &[u8]) -> Result<SceneBlock, String> {
    let me = root.place.clone();
    if d.len() as u64 != root.size {
        return Err(format!("{me}: {:#x} bytes for a root of {:#x}", d.len(), root.size));
    }
    // The entries.
    let mut ents: Vec<(usize, usize)> = Vec::new();
    let mut ids: Vec<u32> = Vec::new();
    let mut p = 0usize;
    loop {
        let a = u32_at(d, p).map_err(|e| format!("{me}: entries: {e}"))? as i32;
        if a < 0 {
            break;
        }
        ids.push(a as u32);
        ents.push((word(d, p + 4)?, word(d, p + 8)?));
        p += 16;
    }
    let l = line(t, GB, "entries (class id, texture list, class data, zero word; 16 bytes each, then a terminator of -1)", DB, ENTRY_FN);
    let mut n = put(t, root, l, 0, p + 16).map_err(|e| format!("{e} (no room for the terminator)"))?;
    let r = entry_fields(&d[..p + 16], &mut n, ents.len());
    done(t, n, r);

    // The texture lists and their images; every start in the block.
    let mut starts: BTreeSet<usize> = BTreeSet::new();
    for &(b, c) in &ents {
        starts.insert(c);
        if b == 0 {
            continue;
        }
        let count = word(d, b).map_err(|e| format!("{me}: texture list at {b:#x}: {e}"))?;
        starts.insert(b);
        let l = line(t, GB, "texture list (a count, then offsets from the list)", DB, TEX_FN);
        let mut n = put(t, root, l, b, 4 + 4 * count)?;
        let r = (|| -> Result<(), String> {
            let mut f = Fields::new(&d[b..b + 4 + 4 * count], &mut n);
            for i in 0..=count {
                f.u32(4 * i)?;
            }
            Ok(())
        })();
        done(t, n, r);
        for i in 0..count {
            let o = b + word(d, b + 4 + 4 * i)?;
            starts.insert(o);
            image(t, root, d, o)?;
        }
    }
    let mut lists: Vec<Vec<crate::core::Texture>> = Vec::new();
    for &(b, _) in &ents {
        let mut v = Vec::new();
        if b != 0 {
            for i in 0..word(d, b)? {
                let o = b + word(d, b + 4 + 4 * i)?;
                v.push(crate::pif::texture(&d[o..]).map_err(|e| format!("{me}: image at {o:#x}: {e}"))?);
            }
        }
        lists.push(v);
    }
    // The classes: each runs to the next start.
    let starts: Vec<usize> = starts.into_iter().collect();
    let mut classes = Vec::new();
    for (k, &(_, c)) in ents.iter().enumerate() {
        let end = starts.iter().copied().find(|&s| s > c).unwrap_or(d.len());
        if c >= d.len() {
            return Err(format!("{me}: class data at {c:#x} past the block's {:#x} bytes", d.len()));
        }
        let class = crate::moby::class(t, root, GB, c as u64, &d[c..end])?;
        classes.push(SceneClass { id: ids[k], offset: c, class, images: std::mem::take(&mut lists[k]) });
    }
    crate::walk::pad_mem(t, root, d, GB);
    Ok(SceneBlock { classes })
}

fn entry_fields(d: &[u8], n: &mut Node, count: usize) -> Result<(), String> {
    let mut f = Fields::new(d, n);
    for i in 0..count {
        f.u32(16 * i)?;
        f.u32(16 * i + 4)?;
        f.u32(16 * i + 8)?;
        f.u32_eq(16 * i + 12, 0, "unused word")?;
    }
    f.u32(16 * count)?;
    for w in 1..4 {
        f.u32_eq(16 * count + 4 * w, 0xffff_ffff, "terminator word")?;
    }
    Ok(())
}

/// A 2FIP image: a 0x20-byte header, a 0x400-byte palette, then the levels' palette indices (func_L01_0042A598).
fn image(t: &mut Tracker, root: &mut Node, d: &[u8], o: usize) -> Result<(), String> {
    let me = format!("{} image at {o:#x}", root.place);
    let (w, h, lv) = (word(d, o + 8)?, word(d, o + 12)?, word(d, o + 0x1c)?);
    if !(8..=1024).contains(&w) || !w.is_power_of_two() || !(8..=1024).contains(&h) || !h.is_power_of_two() || !(1..=4).contains(&lv) || (w >> (lv - 1)) == 0 || (h >> (lv - 1)) == 0 {
        return Err(format!("{me}: {w} x {h} with {lv} levels"));
    }
    let l = line(
        t,
        GB,
        "texture header (+0 tag, +4 no reader, +8 width, +0xC height, +0x10 0x13, +0x14 palette format 0, +0x18 zero, +0x1C mip levels)",
        DB,
        TEX_HDR_FN,
    );
    let mut n = put(t, root, l, o, 0x20)?;
    let r = (|| -> Result<(), String> {
        let mut f = Fields::new(&d[o..o + 0x20], &mut n);
        f.same(0, b"2FIP", "tag")?;
        f.u32(8)?;
        f.u32(0xc)?;
        f.u32_eq(0x10, 0x13, "pixel format (PSMT8)")?;
        f.u32_eq(0x14, 0, "palette format")?;
        f.u32_eq(0x18, 0, "zero")?;
        f.u32(0x1c)?;
        Ok(())
    })();
    n.skip(4, 4);
    done(t, n, r);
    let l = line(t, GB, "texture palette (256 colours of R, G, B, alpha)", DB, TEX_FN);
    let mut n = put(t, root, l, o + 0x20, 0x400)?;
    n.mark(0, 0x400);
    t.close(n);
    let mut at = o + 0x420;
    for k in 0..lv {
        let size = (w >> k) * (h >> k);
        let l = match k {
            0 => line(t, GB, "texture pixels, level 0 (width x height palette indices)", DB, TEX_FN),
            1 => t.ignored_line(GB, "texture pixels, level 1 (half size; never uploaded)", LEVEL1_WHY, Space::Unpacked),
            2 => line(t, GB, "texture pixels, level 2 (quarter size)", DB, "func_L01_0042A598 (same in all 34 story levels) uploads it when the level count is 3 or more"),
            _ => t.ignored_line(GB, "texture pixels, level 3 (eighth size; never uploaded)", LEVEL3_WHY, Space::Unpacked),
        };
        let mut n = put(t, root, l, at, size)?;
        if k == 0 || k == 2 {
            n.mark(0, size as u64);
        }
        t.close(n);
        at += size;
    }
    Ok(())
}

// ---- the chunk ----

/// A SCENE chunk: `root` is its unpacked root, `d` its bytes.
pub fn chunk(t: &mut Tracker, root: &mut Node, d: &[u8]) -> Result<(), String> {
    let me = root.place.clone();
    if d.len() as u64 != root.size {
        return Err(format!("{me}: {:#x} bytes for a root of {:#x}", d.len(), root.size));
    }
    let cnt = crate::disc::u16_at(d, 0xc).map_err(|e| format!("{me}: {e}"))? as i16;
    if cnt <= 0 {
        return Err(format!("{me}: object count {cnt}"));
    }
    let cnt = cnt as usize;
    let cam = word(d, 0x10)?;
    let l = line(t, GC, "chunk header (+0 ticks, +4 sector counts, +8 tick, +0xC object count, +0x10 camera offset)", DC, CHUNK_FN);
    let mut n = put(t, root, l, 0, 0x14)?;
    let r = (|| -> Result<(), String> {
        let mut f = Fields::new(&d[..0x14], &mut n);
        f.mark(0, 2);
        f.fill(2, 2, 0, "zero")?;
        f.u32(4)?;
        f.mark(8, 2);
        let v = crate::disc::u16_at(d, 8)?;
        f.fill(0xa, 2, if v & 0x8000 != 0 { 0xff } else { 0 }, "sign extension of the +8 half-word")?;
        f.mark(0xc, 2);
        f.fill(0xe, 2, 0, "zero")?;
        f.u32(0x10)?;
        Ok(())
    })();
    done(t, n, r);
    let l = line(t, GC, "object offsets (one per object)", DC, CHUNK_FN);
    let mut n = put(t, root, l, 0x14, 4 * cnt)?;
    n.mark(0, 4 * cnt as u64);
    t.close(n);
    let offs: Vec<usize> = (0..cnt).map(|i| word(d, 0x14 + 4 * i)).collect::<Result<_, _>>()?;
    if offs.windows(2).any(|w| w[0] >= w[1]) {
        return Err(format!("{me}: object offsets are not ascending"));
    }
    let first = offs[0];
    if first < cam + 0x20 || (first - cam) % 0x20 != 0 {
        return Err(format!("{me}: camera records at {cam:#x} to the first object at {first:#x}"));
    }
    let ncam = (first - cam) / 0x20;
    // One record per tick, read by func_L01_004B6518; the last one (after the final tick) has no reader found.
    if ncam > 1 {
        let l = line(t, GC, "camera records (0x20 bytes per tick: position, three angles, field of view, cut flag)", DC, CAMERA_FN);
        let mut n = put(t, root, l, cam, 0x20 * (ncam - 1))?;
        n.mark(0, 0x20 * (ncam as u64 - 1));
        t.close(n);
    }
    let l = t.ignored_line(GC, "camera record after the last tick (never indexed)", CAM_WHY, Space::Unpacked);
    let n = put(t, root, l, cam + 0x20 * (ncam - 1), 0x20)?;
    t.close(n);

    let l_head = line(t, GC, "object record header (+0 class, +0xC offset of the position records; +4 and +8 no reader)", DC, HEAD_FN);
    let l_seq = line(t, GC, "animation sequence header (+0x10 frame count, +0x11 and +0x12 bytes, +0x13 0xFF, +0x14 and +0x18 zero; +0 to +0xF no reader)", DC, SEQ_FN);
    let l_offs = line(t, GC, "animation frame offsets (one per frame, from the sequence header)", DC, OFFSETS_FN);
    let l_pos = line(t, GC, "frame position records (16 bytes per frame)", DC, POSITION_FN);
    let l_tail = t.ignored_line(GC, "stale bytes after the last object (read by nothing)", TAIL_WHY, Space::Unpacked);
    for (i, &o) in offs.iter().enumerate() {
        let n = byte(d, o + 0x20).map_err(|e| format!("{me}: object {i}: {e}"))?;
        if n == 0 {
            return Err(format!("{me}: object {i} at {o:#x} has no frames"));
        }
        let mo = word(d, o + 0xc)?;
        let end = offs.get(i + 1).copied().unwrap_or(d.len());
        let mut hn = put(t, root, l_head, o, 0x10)?;
        hn.mark(0, 4);
        hn.mark(0xc, 4);
        hn.skip(4, 8);
        t.close(hn);
        let mut sn = put(t, root, l_seq, o + 0x10, 0x1c)?;
        let r = (|| -> Result<(), String> {
            let mut f = Fields::new(&d[o + 0x10..o + 0x2c], &mut sn);
            f.mark(0x10, 3);
            f.fill(0x13, 1, 0xff, "sequence header byte")?;
            f.fill(0x14, 4, 0, "sequence record offset")?;
            f.fill(0x18, 4, 0, "rate multiplier")?;
            Ok(())
        })();
        sn.skip(0, 0x10);
        done(t, sn, r);
        let mut on = put(t, root, l_offs, o + 0x2c, 4 * n)?;
        on.mark(0, 4 * n as u64);
        t.close(on);
        let mut fr = Vec::with_capacity(n);
        for k in 0..n {
            fr.push(o + 0x10 + word(d, o + 0x2c + 4 * k)?);
        }
        if fr.windows(2).any(|w| w[0] > w[1]) || fr[0] < o + 0x2c + 4 * n || fr[n - 1] >= mo {
            return Err(format!("{me}: object {i} at {o:#x}: frames at {:#x}.. do not lie ascending before the position records at {mo:#x}", fr[0]));
        }
        if mo + 16 * n > end || mo + 16 * n > d.len() {
            return Err(format!("{me}: object {i} at {o:#x}: position records at {mo:#x} overrun the next object at {end:#x}"));
        }
        // The frames are contiguous inside [first frame, mo), clear of what was placed above.
        if root.overlaps(fr[0] as u64, (mo - fr[0]) as u64) {
            return Err(format!("{me}: object {i} at {o:#x}: frames overlap another structure"));
        }
        for k in 0..n {
            let stop = if k + 1 < n { fr[k + 1] } else { mo };
            if stop > fr[k] {
                // anim.rs: a bad value fails the frame's node (the tracker lists it).
                let _ = crate::anim::frame(t, root, GC, fr[k] as u64, &d[fr[k]..stop], Space::Unpacked);
            }
        }
        let mut pn = put(t, root, l_pos, mo, 16 * n)?;
        pn.mark(0, 16 * n as u64);
        t.close(pn);
        if i + 1 == cnt && d.len() > mo + 16 * n {
            let n = put(t, root, l_tail, mo + 16 * n, d.len() - mo - 16 * n)?;
            t.close(n);
        }
    }
    crate::walk::pad_mem(t, root, d, GC);
    Ok(())
}

/// A SCENE block as the game loads it: the unpacked bytes (what RAM holds at `[S + 0x58]`, before the classes are relocated in place)
/// and the classes decoded from them.
pub struct LoadedBlock {
    pub bytes: Vec<u8>,
    pub block: SceneBlock,
}

/// SCENE record `k` of a level's SCENE file `f` (header +8 + 0x14C * k; words 12 and 13 are the block's first sector from the file's
/// start and its sector count, as func_L01_00429450 reads them at `0x160C40 + 0x8000 + 0x14C * k + 0x1250` and `+0x1254`, the file's
/// first sector at `+0x121C`): its LZ stream unpacked (func_L01_004294D0) and decoded. None when the record has no block (no
/// sectors, or sectors of zeros).
pub fn load(d: &mut crate::disc::Disc, f: &crate::toc::LevelFile, k: usize) -> Result<Option<LoadedBlock>, String> {
    if f.kind != crate::toc::FileKind::Scene || k >= 30 {
        return Err(format!("scene record {k} of a {} file", f.kind.name()));
    }
    let at = 8 + k * 0x14c;
    let (s, c) = (word(&f.header, at + 4 * 12)? as u64, word(&f.header, at + 4 * 13)? as u64);
    if s == 0 || c == 0 || s + c > f.sectors {
        return Ok(None);
    }
    let raw = d.sectors(f.sector + s, c)?;
    if raw.iter().all(|&v| v == 0) {
        return Ok(None);
    }
    let st = crate::lz::unpack(&raw).map_err(|e| format!("L{:02} scene record {k}: {e}", f.level))?;
    let mut t = Tracker::new();
    let mut root = t.root("SCENE", &format!("L{:02} record {k} block", f.level), st.data.len() as u64, Space::Unpacked);
    let block = block(&mut t, &mut root, &st.data)?;
    Ok(Some(LoadedBlock { bytes: st.data, block }))
}
