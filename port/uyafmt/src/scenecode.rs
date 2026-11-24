//! What the game puts in the two model regions that are neither the core's classes nor the hero's armor or the wrench, and where
//! each level's own code keeps the variables that say so.
//!
//! **The SCENE block region.** A cutscene's SCENE block (the SCENE file's record `k`, words 12 and 13: first sector from the file's
//! start and sector count; see `scene`) is loaded by func_L01_00429450 (same in all 34 story levels: the catalogue lists it, and
//! this module finds it by its code in every level) from the sector `[0x169E60 + 0x14C * k + 0x30] + [0x169E5C]`,
//! `0x160C40 + 0x8000 + 0x14C * k + 0x1250` and `+0x121C`, count at `+0x1254`) into the buffer `[S + 0x68]`; func_L01_004294D0 (same
//! in all 34) unpacks it with the LZ unpacker (func_L01_00433EA0) to `[S + 0x58]`, moves the unpacked bytes to the end of the
//! 0x8E000-byte region (`[S + 0x58] += 0x8E000 - size` when that is more than 0x10), stores the new `[S + 0x58]`, and for each entry
//! calls func_L01_0042A598, which registers the class and its images (`scene`). `S` is the level's scene state (Veldin 0x1E6E00;
//! level 3 0x1E6880); `[S + 0x30]` is the SCENE record number `k` func_L01_00429998 stores. The class `c` of the block is at the RAM
//! address `[S + 0x58] + c.offset`.
//!
//! **The hero body region** (Veldin 0x1F0C000, in level 3 too; the 22 runs in 0x1F0C000 to 0x1F17FFF of level 3's cutscene frame).
//! func_L01_00422FA0 (the level's start-up func_L01_00428750 calls it when the body variant changed, and the front end's copy
//! frontbin func_00393580-ish loads the same blocks) stores its argument at `0x16C598`, reads the pair `body_pair + variant` of
//! global WAD 4 (the header at TOC +0x1280; pair p's sector is `[0x1D60 + 0x160C40 - 8 * 347 + 8 * p]`, i.e. the instructions at
//! +0x48 `lw a3, 0x1D60($a0)` and +0x4C `lw a1, 0x1284($v0)`: pair 347 + variant, sector base `[0x161EC4]` = the WAD's sector) into
//! the buffer `[S2 + 0x70]` (Veldin 0x1EAA30; `S2` is the level buffer struct that holds the core's base at +0x18) with
//! func_L01_0045CA00, sets slot 0 of the register table `slots` (0x1F3E40) to the TEX0 at `[0x16C5B8]` and calls func_L01_00424048
//! (the class relocator, which relocates the block in place at that address) with class id -1, no record table (`a1 = 0`) and the
//! 16 remap bytes at `gp - 0x5BA0`: a setup's number `local >= 0` is mapped to `remap[local]`, a slot of the table, and func_L01_004239F8
//! copies the slot's TEX0, TEX1 and MIPTBP1 into the setup. The body's texture is the 2FIP image of pair `texture_pair + t` where `t`
//! is `[0x16C5B0]` (func_L01_00423068 stores its argument there, reads the pair at +0x78/+0x7C `lw a3, 0x1D78($v0)`, 350 + t, and
//! builds the TEX0 it stores at `[0x16C5B8]`). The variant is `[0x1D54D0] & 3`, the texture `([0x1D54D0] >> 16) & 0x1F`
//! (func_L01_00428750 at its call sites, see core.c).
//!
//! **The region's place.** func_L01_004B5C78 (same in all 34), which starts a cutscene, carves the region from the top of the
//! second of the game's two frame packet buffers (each `0x180000` bytes: the words at `map + 0xC` and `map + 0x10` hold their
//! starts, Veldin's `map` 0x1EA9C0, level 9's 0x1E9880): at +0x15C to +0x1BC it lowers the bytes each buffer's packets may fill
//! (`room`, Veldin 0x1E10F0, level 9 0x1E1170) by 0x90000 (+0x16C `lui $v0, 0xFFF7`) and sets `[S + 0x58]` to `[map + 0x10] +
//! room` and `[S + 0x68]` to `[map + 0xC] + room`; after the unpack, +0x3C0 to +0x3E8, `room` becomes the smaller of `[S + 0x58] -
//! [map + 0x10]` and the streams' place in the first buffer, `[S + 0x68] - [map + 0xC]`.
//!
//! The functions are found in each level's overlay by their code, as `levelcode` finds its own.

use crate::disc::{Disc, u32_at};
use crate::levelcode::{self, Overlay, STORY_LEVELS};
use crate::toc;

struct Func {
    name: &'static str,
    address: u32,
    size: u32,
}

const SCENE_LOAD: Func = Func { name: "func_L01_00429450", address: 0x42_9450, size: 124 };
const SCENE_UNPACK: Func = Func { name: "func_L01_004294D0", address: 0x42_94D0, size: 248 };
const BODY_LOAD: Func = Func { name: "func_L01_00422FA0", address: 0x42_2FA0, size: 0xC4 };
const BODY_TEXTURE: Func = Func { name: "func_L01_00423068", address: 0x42_3068, size: 0x1CC };
const CLASS10_TEXTURE: Func = Func { name: "func_L01_00423238", address: 0x42_3238, size: 448 };
const SCENE_START: Func = Func { name: "func_L01_004B5C78", address: 0x4B_5C78, size: 1292 };

/// The variables of a level's scene and hero body loading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneGlobals {
    pub level: usize,
    /// `S + 0x30`: the SCENE record number the loader reads (func_L01_00429450 +0x18 `lw $v0, 0x30($a0)`, `lui` +4, `addiu` +0xC).
    pub scene_index: u32,
    /// `S + 0x68`: the buffer the compressed block is read to (func_L01_004294D0 +0x30 `lw $a0, 0x68($s1)`; `lui` +4, `addiu` +0x14;
    /// func_L01_00429450 +0x24 reads the same).
    pub scene_buffer: u32,
    /// `S + 0x58`: the unpacked block's RAM address, after the move to the region's end (func_L01_004294D0 +0x38 `lw $a1, 0x58($s1)`
    /// before it, +0x74 `sw $s0, 0x58($s1)` after it).
    pub scene_base: u32,
    /// func_L01_004294D0's address in the level's code: the unpacker.
    pub unpack: u32,
    /// func_L01_004B5C78's address in the level's code: a cutscene's start, which calls the loader and the unpacker.
    pub start: u32,
    /// The word of the game's memory map that holds the second frame packet buffer's start, whose top the SCENE region is carved
    /// from (func_L01_004B5C78 +0x170 `lw $a0, 0x10($v1)`, `addiu $v1, $s6, lo` +0x164, `lui $s6` +0xAC; the first's is the word
    /// before, +0x168 `lw $a2, 0xC($v1)`).
    pub packet_b: u32,
    /// `[S2 + 0x70]`: the hero body block's RAM address (func_L01_00422FA0 +0x40 `lw $s2, lo($v1)`, `lui` +0x38).
    pub body_base: u32,
    /// The variable func_L01_00422FA0 stores its argument in, the body variant (+0x28 `sw $s0, 0x18($s1)`; `lui` +0xC, `addiu` +0x14).
    pub body_variant: u32,
    /// The variable func_L01_00423068 stores its argument in, the body texture number (+0x38 `sw $s0, 0x30($s4)`; `lui` +0xC, `addiu` +0x14).
    pub body_texture: u32,
    /// The body texture's TEX0 (the doubleword func_L01_00423068 +0x1AC `sd $v1, 0x38($s4)` stores; func_L01_00422FA0 +0x60 `ld $a0,
    /// 0x38($s1)` copies it to slot 0 of the register table).
    pub body_tex0: u32,
    /// The register table's address (func_L01_00422FA0 +0x78 `sd $a0, lo($v1)`, `lui` +0x64): three doublewords per slot.
    pub slots: u32,
    /// The 16 bytes func_L01_00422FA0 +0xA0 passes as the remap table (`addiu $a2, $gp, imm`), as the overlay holds them.
    pub body_remap: [u8; 16],
    /// The pair of global WAD 4 that is body variant 0 (the instruction at +0x48 `lw $a3, imm($a0)`: (imm - 0x1288) / 8).
    pub body_pair: usize,
    /// The pair that is body texture 0 (func_L01_00423068 +0x80 `lw $a3, imm($v0)`: (imm - 0x1288) / 8).
    pub texture_pair: usize,
    /// The variable func_L01_00423238 stores its argument in, the texture number of scene class 10 (+0x2C `sw $s0, imm($gp)`; Veldin
    /// 0x1D4BA0 = gp - 0x7D10). Its callers pass the byte at 0x1D5475 (func_L01_003EEA10 and others read it with `lbu`).
    pub class10_texture: u32,
    /// The TEX0 func_L01_00423238 stores for it (+0x1B4 `sd $v1, lo($at)`, `lui` +0x1B0; Veldin 0x1D4BA8): func_L01_0042A598 copies
    /// it to slot 0 of the register table for a class whose id is 10 (+0x80 `xori $a2, $fp, 0xA`; relocator called with no record
    /// table, class id 10 and the remap bytes at `gp - 0x5BA0`, so each setup of the class gets the TEX0 of slot `remap[local]`).
    pub class10_tex0: u32,
    /// The pair of WAD 5 (the header at TOC +0x1EB0) that is texture 0 of class 10 (func_L01_00423238 +0x74 `lw $a3, 0x2238($v0)`:
    /// (imm - 0x1EB8) / 8; +0x6C `lw $a1, 0x1EB4($v1)` is the WAD's sector): a 2FIP image.
    pub class10_pair: usize,
}

fn word(ov: &Overlay, address: u32) -> Option<u32> {
    ov.record_of(address).map(|r| {
        let o = (address - r.address) as usize;
        u32::from_le_bytes([r.data[o], r.data[o + 1], r.data[o + 2], r.data[o + 3]])
    })
}

fn norm(w: u32) -> u32 {
    match w >> 26 {
        0x02 | 0x03 => w & 0xFC00_0000,
        0x08..=0x0F | 0x18..=0x1B | 0x1E | 0x1F | 0x20..=0x2F | 0x30..=0x3F => w & 0xFFFF_0000,
        _ => w,
    }
}

/// Level 1's function `f` (words with the linker's fields taken out) found exactly once in `ov`'s records.
fn locate(reference: &Overlay, ov: &Overlay, f: &Func) -> Result<u32, String> {
    let n = (f.size / 4) as usize;
    let want: Vec<u32> = (0..n)
        .map(|i| word(reference, f.address + 4 * i as u32).map(norm).ok_or_else(|| format!("{}: level 1 has no word at {:#x}", f.name, f.address + 4 * i as u32)))
        .collect::<Result<_, _>>()?;
    let mut hits = Vec::new();
    for r in &ov.records {
        let ws: Vec<u32> = r.data.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        if ws.len() < n {
            continue;
        }
        for i in 0..=ws.len() - n {
            if ws[i..i + n].iter().zip(&want).all(|(w, x)| norm(*w) == *x) {
                hits.push(r.address + 4 * i as u32);
            }
        }
    }
    match hits.as_slice() {
        [one] => Ok(*one),
        [] => Err(format!("L{:02}: {} is not in the overlay", ov.level, f.name)),
        many => Err(format!("L{:02}: {} is at {} places: {many:x?}", ov.level, f.name, many.len())),
    }
}

fn sext(w: u32) -> i64 {
    (w & 0xFFFF) as u16 as i16 as i64
}

/// The address a `lui` at function offset `lui`, an optional `addiu` at `add` and the load or store at `at` form.
fn formed(ov: &Overlay, start: u32, f: &Func, lui: u32, add: Option<u32>, at: u32) -> Result<u32, String> {
    let w = |off: u32| word(ov, start + off).ok_or_else(|| format!("L{:02} {} +{off:#x}: no word", ov.level, f.name));
    let l = w(lui)?;
    if l >> 26 != 0x0F {
        return Err(format!("L{:02} {} +{lui:#x}: {l:08x} is not a lui", ov.level, f.name));
    }
    let mut base = ((l & 0xFFFF) << 16) as i64;
    let mut reg = (l >> 16) & 31;
    if let Some(add) = add {
        let a = w(add)?;
        if a >> 26 != 0x09 || (a >> 21) & 31 != reg {
            return Err(format!("L{:02} {} +{add:#x}: {a:08x} is not an addiu on the lui's register", ov.level, f.name));
        }
        base += sext(a);
        reg = (a >> 16) & 31;
    }
    let u = w(at)?;
    if !matches!(u >> 26, 0x09 | 0x20..=0x2B | 0x31 | 0x37 | 0x39 | 0x3F) || (u >> 21) & 31 != reg {
        return Err(format!("L{:02} {} +{at:#x}: {u:08x} does not use the base register formed at +{lui:#x}", ov.level, f.name));
    }
    Ok((base + sext(u)) as u32)
}

/// The immediate of the instruction at function offset `at`, which must be the load `lw rt, imm(rs)` (opcode 0x23).
fn lw_imm(ov: &Overlay, start: u32, f: &Func, at: u32) -> Result<i64, String> {
    let u = word(ov, start + at).ok_or_else(|| format!("L{:02} {} +{at:#x}: no word", ov.level, f.name))?;
    if u >> 26 != 0x23 {
        return Err(format!("L{:02} {} +{at:#x}: {u:08x} is not a lw", ov.level, f.name));
    }
    Ok(sext(u))
}

/// `level`'s variables, read from its overlay (`reference` is level 1's, `gp` the boot constant `levelcode::LevelGlobals::gp`).
pub fn from_overlay(reference: &Overlay, ov: &Overlay, gp: u32) -> Result<SceneGlobals, String> {
    let lv = ov.level;
    let load = locate(reference, ov, &SCENE_LOAD)?;
    let unpack = locate(reference, ov, &SCENE_UNPACK)?;
    let body = locate(reference, ov, &BODY_LOAD)?;
    let tex = locate(reference, ov, &BODY_TEXTURE)?;
    let scene_index = formed(ov, load, &SCENE_LOAD, 0x4, Some(0xC), 0x18)?;
    let scene_buffer = formed(ov, unpack, &SCENE_UNPACK, 0x4, Some(0x14), 0x30)?;
    let scene_base = formed(ov, unpack, &SCENE_UNPACK, 0x4, Some(0x14), 0x38)?;
    if formed(ov, load, &SCENE_LOAD, 0x4, Some(0xC), 0x24)? != scene_buffer || formed(ov, unpack, &SCENE_UNPACK, 0x4, Some(0x14), 0x74)? != scene_base {
        return Err(format!("L{lv:02}: the scene loader and unpacker disagree on the scene state"));
    }
    if scene_buffer != scene_index + 0x38 || scene_base != scene_index + 0x28 {
        return Err(format!("L{lv:02}: scene state fields {scene_index:#x}, {scene_buffer:#x}, {scene_base:#x} are not at +0x30, +0x68, +0x58"));
    }
    let start = locate(reference, ov, &SCENE_START)?;
    let packet_a = formed(ov, start, &SCENE_START, 0xAC, Some(0x164), 0x168)?;
    let packet_b = formed(ov, start, &SCENE_START, 0xAC, Some(0x164), 0x170)?;
    if packet_b != packet_a + 4 || word(ov, start + 0x16C) != Some(0x3C02_FFF7) {
        return Err(format!("L{lv:02}: func_L01_004B5C78 does not carve the 0x90000-byte SCENE region from the second packet buffer"));
    }
    let body_base = formed(ov, body, &BODY_LOAD, 0x38, None, 0x40)?;
    let slots = formed(ov, body, &BODY_LOAD, 0x64, None, 0x78)?;
    let body_variant = formed(ov, body, &BODY_LOAD, 0xC, Some(0x14), 0x28)?;
    let body_tex0 = formed(ov, body, &BODY_LOAD, 0xC, Some(0x14), 0x60)?;
    let body_texture = formed(ov, tex, &BODY_TEXTURE, 0xC, Some(0x14), 0x38)?;
    if formed(ov, tex, &BODY_TEXTURE, 0xC, Some(0x14), 0x1AC)? != body_tex0 {
        return Err(format!("L{lv:02}: func_L01_00423068 stores the body TEX0 elsewhere than func_L01_00422FA0 reads it"));
    }
    // The pair tables: `lw a3, 0x1D60(...)` = [0x160C40 + 0x1D60] is the pair's sector (the pairs of the WAD at TOC +0x1280 start at
    // +0x1288), `lw a1, 0x1284(...)` the WAD's own sector.
    let pair_of = |imm: i64| -> Result<usize, String> {
        let d = imm - 0x1288;
        if d < 0 || d % 8 != 0 {
            return Err(format!("L{lv:02}: pair table offset {imm:#x} is not a pair of the WAD at +0x1280"));
        }
        Ok((d / 8) as usize)
    };
    if lw_imm(ov, body, &BODY_LOAD, 0x4C)? != 0x1284 || lw_imm(ov, tex, &BODY_TEXTURE, 0x78)? != 0x1284 {
        return Err(format!("L{lv:02}: the body loaders do not read the sector base of the WAD at +0x1280"));
    }
    let body_pair = pair_of(lw_imm(ov, body, &BODY_LOAD, 0x48)?)?;
    let texture_pair = pair_of(lw_imm(ov, tex, &BODY_TEXTURE, 0x80)?)?;
    let c10 = locate(reference, ov, &CLASS10_TEXTURE)?;
    let class10_texture = {
        let u = word(ov, c10 + 0x2C).ok_or_else(|| format!("L{lv:02} func_L01_00423238 +0x2C: no word"))?;
        if u >> 26 != 0x2B || (u >> 21) & 31 != 28 || (u >> 16) & 31 != 16 {
            return Err(format!("L{lv:02} func_L01_00423238 +0x2C: {u:08x} is not `sw $s0, imm($gp)`"));
        }
        (gp as i64 + sext(u)) as u32
    };
    let class10_tex0 = formed(ov, c10, &CLASS10_TEXTURE, 0x1B0, None, 0x1B4)?;
    if lw_imm(ov, c10, &CLASS10_TEXTURE, 0x6C)? != 0x1EB4 {
        return Err(format!("L{lv:02}: func_L01_00423238 does not read the sector base of the WAD at +0x1EB0"));
    }
    let class10_pair = {
        let d = lw_imm(ov, c10, &CLASS10_TEXTURE, 0x74)? - 0x1EB8;
        if d < 0 || d % 8 != 0 {
            return Err(format!("L{lv:02}: func_L01_00423238 pair table offset is not a pair of the WAD at +0x1EB0"));
        }
        (d / 8) as usize
    };
    let a = word(ov, body + 0xA0).ok_or_else(|| format!("L{lv:02} func_L01_00422FA0 +0xA0: no word"))?;
    if a >> 26 != 0x09 || (a >> 21) & 31 != 28 || (a >> 16) & 31 != 6 {
        return Err(format!("L{lv:02} func_L01_00422FA0 +0xA0: {a:08x} is not `addiu $a2, $gp, imm`"));
    }
    let t = (gp as i64 + sext(a)) as u32;
    let mut body_remap = [0u8; 16];
    for (k, b) in body_remap.iter_mut().enumerate() {
        let w = word(ov, (t + k as u32) & !3).ok_or_else(|| format!("L{lv:02}: the body remap table at {t:#x} is in no overlay record"))?;
        *b = (w >> (8 * ((t as usize + k) & 3))) as u8;
    }
    Ok(SceneGlobals { level: lv, scene_index, scene_buffer, scene_base, unpack, start, packet_b, body_base, body_variant, body_texture, body_tex0, slots, body_remap, body_pair, texture_pair, class10_texture, class10_tex0, class10_pair })
}

/// Story level `level`'s variables, read from its own code on the disc.
pub fn globals(iso_path: &str, level: usize) -> Result<SceneGlobals, String> {
    let mut d = Disc::open(iso_path)?;
    let tc = toc::read(&mut d)?;
    let reference = levelcode::overlay(&mut d, &tc, 1)?;
    let ov = levelcode::overlay(&mut d, &tc, level)?;
    let g = levelcode::from_overlay(&reference, &ov)?;
    from_overlay(&reference, &ov, g.gp)
}

/// All 34 story levels' variables, in level order.
pub fn all(iso_path: &str) -> Result<Vec<SceneGlobals>, String> {
    let mut d = Disc::open(iso_path)?;
    let tc = toc::read(&mut d)?;
    let reference = levelcode::overlay(&mut d, &tc, 1)?;
    STORY_LEVELS
        .iter()
        .map(|&l| {
            let ov = levelcode::overlay(&mut d, &tc, l)?;
            let g = levelcode::from_overlay(&reference, &ov)?;
            from_overlay(&reference, &ov, g.gp)
        })
        .collect()
}

/// The hero body block the game loads at `body_base`: global WAD 4's pair `g.body_pair + variant` decoded as a moby class, and the
/// image of pair `g.texture_pair + texture`.
pub struct HeroBody {
    /// The pair's bytes (the class as the disc holds it; RAM holds it relocated at `body_base`).
    pub bytes: Vec<u8>,
    pub class: crate::moby::MobyClass,
    pub image: crate::core::Texture,
}

impl HeroBody {
    /// The image a texture setup's number names: `remap[local]` must be slot 0, whose TEX0 `[0x16C5B8]` is the body texture's.
    pub fn image(&self, g: &SceneGlobals, local: i32) -> Result<&crate::core::Texture, String> {
        let slot = usize::try_from(local).ok().and_then(|i| g.body_remap.get(i).copied()).ok_or_else(|| format!("hero body: texture number {local}"))?;
        if slot != 0 {
            return Err(format!("hero body: texture number {local} maps to slot {slot}, only slot 0 holds the body's texture"));
        }
        Ok(&self.image)
    }
}

/// Loads the hero body for `variant` (`[0x1D54D0] & 3`) and `texture` (`([0x1D54D0] >> 16) & 0x1F`).
pub fn hero_body(d: &mut Disc, tc: &toc::Toc, g: &SceneGlobals, variant: usize, texture: usize) -> Result<HeroBody, String> {
    let w = tc.wads.get(3).ok_or("the table of contents has no WAD 4")?;
    let pair = |k: usize| w.pairs.get(k).copied().ok_or_else(|| format!("WAD 4 has no pair {k}"));
    let (ms, mc) = pair(g.body_pair + variant)?;
    let (ts, tcnt) = pair(g.texture_pair + texture)?;
    let bytes = d.sectors(w.sector + ms as u64, mc as u64)?;
    let mut t = crate::track::Tracker::new();
    let mut root = t.root("hero body", &format!("WAD 4 pair {}", g.body_pair + variant), bytes.len() as u64, crate::track::Space::Disc);
    let class = crate::moby::class_in(&mut t, &mut root, "hero body", 0, &bytes, crate::track::Space::Disc)?;
    let tb = d.sectors(w.sector + ts as u64, tcnt as u64)?;
    let image = crate::pif::texture(&tb)?;
    let _ = u32_at;
    Ok(HeroBody { bytes, class, image })
}

/// The image of scene class 10's texture `n` (the value of `g.class10_texture` in RAM): WAD 5's pair `g.class10_pair + n`. The class's
/// setups map `remap[local]` to slot 0, whose TEX0 is `g.class10_tex0`'s.
pub fn class10_image(d: &mut Disc, tc: &toc::Toc, g: &SceneGlobals, n: usize) -> Result<crate::core::Texture, String> {
    let w = tc.wads.get(4).ok_or("the table of contents has no WAD 5")?;
    let (s, c) = w.pairs.get(g.class10_pair + n).copied().ok_or_else(|| format!("WAD 5 has no pair {}", g.class10_pair + n))?;
    crate::pif::texture(&d.sectors(w.sector + s as u64, c as u64)?)
}
