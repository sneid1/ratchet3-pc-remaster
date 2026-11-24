//! A LEVEL file's core: the core index (level data part 1) and the core data (part 9, an LZ stream), as the level core loader
//! func_L01_00428750 reads them (the same function in all 34 story levels).
//!
//! The index starts with 48 words. Counts and index offsets of tables in the index: words 6/7 moby class records (32 bytes;
//! func_L01_00424368, func_L01_00425170), 8/9 tie class records (32 bytes; func_L01_00424478), 10/11 shrub class records (48
//! bytes; func_L01_00424850), 12/13, 14/15, 16/17, 18/19 the terrain, moby, tie and shrub texture records (16 bytes: an offset
//! from the texture data, a u16 format the loader turns into (0x1E - format) << 28), 20/21 and 22/23 two more 16-byte texture
//! tables (func_L01_004224C8, func_L01_004225E8), 0/1 the GS RAM table (16-byte records, func_L01_00423B48). Index offsets of
//! further tables: words 0x1B (func_L01_004224C8), 0x1C (0x20-byte records copied out), 0x1E (core offsets for the class at
//! 0x1FD200), 0x2B (s16 list ended by a negative value). Core offsets: word 2 terrain (func_L01_00424DE8), 3 (0 = none), 4
//! sky (func_L01_00422698), 5 collision (func_L01_004250B0), 0x18 the texture data, 0x19 (func_L01_004224C8), 0x1A, 0x29
//! (func_L01_00422470), 0x2A, 0x2C, 0x2D (a counted table of 16-byte entries), 0x2E; each class record's word 0.

use crate::disc::u32_at;
use crate::track::{Node, Space, Tracker};
use std::collections::BTreeMap;

const GI: &str = "LEVEL core index";
const GC: &str = "LEVEL core (unpacked)";
const LOADER: &str = "func_L01_00428750";

/// A texture record of the terrain (0), moby (1), tie (2) or shrub (3) table (16 bytes): +0 the offset of its texels in the
/// texture data, +4 width, +6 height, +8 level count, +0xA the palette's GS block, +0xC and +0xE the GS blocks of mip levels 2
/// and 3 (-1 for none). func_L01_00424DE8 builds TEX0 (CBP = +0xA plus the base at 0x1A1EF0 / 256), TEX1 (MXL = +8 - 1) and
/// MIPTBP1 (TBP2 = +0xC, TBP3 = +0xE plus the same base) from them; levels 0 and 1 are streamed from the texels each frame.
#[derive(Clone)]
pub struct TexRecord {
    pub table: usize,
    pub offset: u64,
    pub width: u64,
    pub height: u64,
    pub levels: u16,
    pub palette: i16,
    pub mip2: i16,
    pub mip3: i16,
}

/// A texture as the GS samples it: each level's pixels (RGBA, alpha 0x80 = 1.0 as on the GS), level 0 first.
pub struct Texture {
    pub width: usize,
    pub height: usize,
    pub levels: Vec<Vec<[u8; 4]>>,
    /// Level 0 as the GS holds it, which names the texture as PCSX2 does (texture packs: `port/uya` texnames); None for a texture
    /// not decoded from indices and a palette.
    pub gs: Option<GsLevel>,
}

/// An 8-bit texture's level 0 in GS terms: the PSM the game samples it with (0x13 PSMT8, 0x1B PSMT8H), one index per texel row by
/// row, and the palette's 256 colours in index order (a 16 x 16 PSMCT32 CLUT read as CSM1; alpha 0x80 = 1.0).
pub struct GsLevel {
    pub psm: u32,
    pub texels: Vec<u8>,
    pub clut: Vec<[u8; 4]>,
}

/// A tie or shrub class record of the core index (words 8/9: 32 bytes, words 10/11: 48 bytes), as func_L01_00428750 passes it
/// to the class loader: the class data's core offset (+0), the class id the gameplay block's instances name (+4), and the 16
/// texture index bytes (+0x10): the class's texture register j uses the tie (or shrub) texture record of byte j
/// (func_L01_00424478 at 0x424658: lbu, then the record at table base + 16 x byte builds TEX0).
pub struct ClassRecord {
    pub offset: u64,
    pub id: u32,
    /// A moby class record's +8 (`coremisc::class_records`): 0, 1 or other: which texture func_L01_00428750 gives the class's
    /// setups whose index byte is 0xFF (none, the TEX0 at 0x16C5B8, Clank's at 0x1D4BA8). 0 for tie and shrub records.
    pub mode: u32,
    pub textures: [u8; 16],
}

/// What the core gives the port: the terrain, the terrain, tie and shrub textures (indexed as the terrain's materials and the
/// class records' texture bytes name them) and the tie and shrub class records.
pub struct Core {
    pub terrain: crate::terrain::Terrain,
    pub terrain_textures: Vec<Texture>,
    pub tie_textures: Vec<Texture>,
    pub shrub_textures: Vec<Texture>,
    /// The moby texture records' textures (the index's table 1).
    pub moby_textures: Vec<Texture>,
    pub tie_classes: Vec<ClassRecord>,
    pub shrub_classes: Vec<ClassRecord>,
    /// The shrub classes' data, in the order of `shrub_classes` (each record's class at its core offset).
    pub shrubs: Vec<crate::shrub::ShrubClass>,
    /// The tie classes' data, in the order of `tie_classes`.
    pub ties: Vec<crate::tie::TieClass>,
    /// The moby class records (index words 6/7, 32 bytes: +0 the class data's core offset, 0 for none; +4 the class id; +0x10 the
    /// 16 texture index bytes, func_L01_00423E10 at 0x423E8C) and each one's class data (None when its offset is 0).
    pub moby_classes: Vec<ClassRecord>,
    pub mobys: Vec<Option<crate::moby::MobyClass>>,
    /// The sky (index word 4; 0 in level 23, which has none).
    pub sky: Option<crate::coremisc::Sky>,
    /// The level chunks' terrains get their visibility keys through the gameplay block's pairs (`terrain::ZoneKeys::Indexed`); false:
    /// they get 0x7F80.
    pub level_chunk_keys: bool,
    /// The core index has the visibility grid (word 3): the loader applies the gameplay block's visibility pairs.
    pub has_grid: bool,
    /// The moby textures -2 (index 0) and -3 (index 1): see `special_textures`.
    pub special_textures: [Texture; 2],
    /// The effect textures (core region 0x19; program 6's `EffectTexture::index` and an effect entry's byte +2 index it):
    /// `coremisc::effect_textures`.
    pub effect_textures: Vec<Texture>,
    /// The textures of core region 0x1A (the primitives the EE writes into the moby chain itself): `coremisc::quad_textures`.
    pub quad_textures: Vec<crate::coremisc::QuadTexture>,
    /// Region 0x1A's offset in the core (core index word 0x1A): its RAM address is the core's plus this (func_L01_00428750
    /// +0x8A4 stores it at 0x1DFA04 on Veldin, where func_L01_00401CD0 reads it).
    pub quad_region: u32,
}

/// The index (part 1) at `at` in the level data region. Returns the core offsets it gives, with their kinds.

/// The index header's node is returned open: coremisc::header_words checks words 0x22 and 0x23 against the core stream, which
/// the walk unpacks later, and the walk closes it then.
pub fn index(t: &mut Tracker, region: &mut Node, at: u64, b: &[u8]) -> Result<(Vec<(u64, &'static str)>, Vec<TexRecord>, Node), String> {
    let w = |i: usize| u32_at(b, 4 * i).map(|v| v as u64);
    // The header: the words func_L01_00428750 reads are marked here; coremisc::header_words checks 0x1F, 0x22, 0x23, 0x28 and
    // 0x2F against the values they derive from; word 0x1D has no reader and no relation found.
    let l = t.line(GI, "index header (48 words)", "core::index", "func_L01_00428750; words 0x1F, 0x22, 0x23, 0x28 and 0x2F checked against the values they derive from (the word 0x1E table's lowest offset, the core stream's size and unpacked size, region 0x1A's end, zero); word 0x1D: no reader found", Space::Disc);
    let mut h = t.child(region, l, at, 0xc0);
    for i in 0..48usize {
        if ![0x1d, 0x1f, 0x22, 0x23, 0x28, 0x2f].contains(&i) {
            h.mark(4 * i as u64, 4);
        }
    }
    // Tables in the index: (name, count word, offset word, record size).
    // The other tables (words 0x14/0x15, 0x16/0x17, the GS RAM table, 0x1B, 0x1C, 0x1E, 0x2B) are coremisc::index_tables'.
    // The class record tables (words 6/7, 8/9, 10/11) are coremisc::class_records'.
    let tables: [(&str, usize, usize, u64); 4] = [
        ("terrain texture records (16 bytes)", 12, 13, 16),
        ("moby texture records (16 bytes)", 14, 15, 16),
        ("tie texture records (16 bytes)", 16, 17, 16),
        ("shrub texture records (16 bytes)", 18, 19, 16),
    ];
    let mut starts: BTreeMap<u64, String> = BTreeMap::new();
    let mut placed = Vec::new();
    for (name, cw, ow, size) in tables {
        let (n, o) = (w(cw)?, w(ow)?);
        if n == 0 {
            continue;
        }
        if o < 0xc0 || o + n * size > b.len() as u64 {
            return Err(format!("index table {name}: {n} records at {o:#x} outside the index's {:#x} bytes", b.len()));
        }
        placed.push((name.to_string(), o, n * size));
        starts.insert(o, name.to_string());
    }
    // A table ends at its count, and never past the next table.
    for i in [0x1b, 0x1c, 0x1e, 0x2b, 0x14 + 1, 0x16 + 1, 1, 7, 9, 11] {
        let o = w(i)?;
        if o != 0 && o < b.len() as u64 {
            starts.entry(o).or_insert_with(String::new);
        }
    }
    let ends: Vec<u64> = starts.keys().copied().chain(std::iter::once(b.len() as u64)).collect();
    for (k, (o, name)) in starts.iter().enumerate() {
        if name.is_empty() {
            continue;
        }
        let size = placed.iter().find(|p| p.1 == *o).map(|p| p.2).unwrap_or(ends[k + 1] - o);
        let size = size.min(ends[k + 1] - o);
        // The texture record tables: +0 offset in the texture data, +4 width, +6 height, +8 level count, +0xA palette
        // address, +0xC and +0xE the mip levels' GS addresses (-1 for none).
        let tex = [
            ("terrain texture records", "func_L01_00428750 (+0, +4), func_L01_00424DE8 (+4 to +0xE: TEX0, TEX1, MIPTBP1)", true),
            ("tie texture records", "func_L01_00428750 (+0, +4), func_L01_00424478 (+4 to +0xE: TEX0, TEX1, MIPTBP1)", true),
            ("shrub texture records", "func_L01_00428750 (+0, +4), func_L01_00424850 (+4 to +0xE: TEX0, TEX1, MIPTBP1)", true),
            ("moby texture records", "func_L01_00428750 (+0, +4), func_L01_00423700 (+4 to +0xE: reached through func_L01_00423E10 from func_L01_00424048)", true),
        ];
        if let Some(&(_, game, full)) = tex.iter().find(|x| name.starts_with(x.0)) {
            let l = t.line(GI, name, "core::index", game, Space::Disc);
            let mut n = t.child(region, l, at + o, size);
            for r in 0..size / 16 {
                let e = (o + 16 * r) as usize;
                let (w, h) = (crate::disc::u16_at(b, e + 4)? as u64, crate::disc::u16_at(b, e + 6)? as u64);
                if !w.is_power_of_two() || !h.is_power_of_two() || w < 8 || h < 8 {
                    n.fail(format!("record {r}: {w} x {h} texels"));
                    break;
                }
                if full {
                    n.mark(16 * r, 16);
                }
            }
            t.close(n);
            continue;
        }
        let l = t.line(GI, name, "", "", Space::Disc);
        let n = t.child(region, l, at + o, size);
        t.close(n);
    }
    // coremisc.rs: a bad value fails its table's line (the tracker lists it).
    let _ = crate::coremisc::index_tables(t, region, at, b);
    let _ = crate::coremisc::class_records(t, region, at, b);
    // Core offsets.
    let mut core = Vec::new();
    for (i, kind) in [
        (2usize, "terrain (word 2; func_L01_00424DE8)"),
        (3, "visibility grid (word 3; func_L01_003FECA8)"),
        (4, "sky (word 4; func_L01_00422698)"),
        (5, "collision (word 5; func_L01_004250B0)"),
        (0x18, "texture data (word 0x18)"),
        (0x19, "region at word 0x19 (func_L01_004224C8)"),
        (0x1a, "region at word 0x1A"),
        (0x29, "region at word 0x29 (func_L01_00422470)"),
        (0x2a, "region at word 0x2A (func_L01_003FEE38)"),
        (0x2c, "region at word 0x2C (func_L01_003FEE38)"),
        (0x2d, "region at word 0x2D (func_L01_00428750)"),
        (0x2e, "region at word 0x2E (func_L01_003FEE38)"),
    ] {
        let v = w(i)?;
        if v != 0 || i == 2 {
            core.push((v, kind));
        }
    }
    // The word 0x1E table: 256 slots of core offsets of moby class 0's animation sequences (func_L01_00428750,
    // func_L01_00423DB8).
    let e = w(0x1e)? as usize;
    for k in 0..256 {
        let v = u32_at(b, e + 4 * k)? as u64;
        if v != 0 {
            core.push((v, "slot sequence (word 0x1E table)"));
        }
    }
    for (cw, ow, size, kind) in [(6usize, 7usize, 32usize, "moby class data"), (8, 9, 32, "tie class data"), (10, 11, 48, "shrub class data")] {
        let (n, o) = (w(cw)? as usize, w(ow)? as usize);
        for r in 0..n {
            let v = u32_at(b, o + r * size)? as u64;
            if v != 0 {
                core.push((v, kind));
            }
        }
    }
    // Texture records of the terrain, moby, tie and shrub tables: offset from the texture data, width and height.
    let mut tex = Vec::new();
    for (k, (cw, ow)) in [(12usize, 13usize), (14, 15), (16, 17), (18, 19)].into_iter().enumerate() {
        let (n, o) = (w(cw)? as usize, w(ow)? as usize);
        for r in 0..n {
            let at = o + 16 * r;
            let wh = u32_at(b, at + 4)? as u64;
            let h16 = |o: usize| crate::disc::u16_at(b, at + o);
            tex.push(TexRecord { table: k, offset: u32_at(b, at)? as u64, width: wh & 0xffff, height: wh >> 16, levels: h16(8)?, palette: h16(0xa)? as i16, mip2: h16(0xc)? as i16, mip3: h16(0xe)? as i16 });
        }
    }
    Ok((core, tex, h))
}

/// The unpacked core, split at every offset the index gives; each piece is named by its kind and runs to the next offset.
/// `gs` is the GS RAM image (level data part 2) and `gs_images` its first loop's images. Returns the terrain and the terrain
/// textures.
/// `gameplay` is the level's gameplay block: its light sets light the terrain, and its chunk visibility pairs give the terrain's
/// chunks their mask and zone when the core has the visibility grid (func_L01_004252F0 at 0x427910 tests the grid's pointer).
pub fn split(t: &mut Tracker, root: &mut Node, offsets: &[(u64, &'static str)], textures: &[TexRecord], idx: &[u8], core: &[u8], gs: &[u8], gs_images: &[crate::hud::GsRamImage], gameplay: &crate::gameplay::Gameplay) -> Result<Core, String> {
    let has_grid = offsets.iter().any(|(_, k)| k.starts_with("visibility grid"));
    let terrain_at = offsets.iter().find(|(_, k)| k.starts_with("terrain")).ok_or("the core index gives no terrain")?.0 as usize;
    let header_count = u32_at(core, terrain_at + 0xc)? as usize;
    let zones = match (&gameplay.chunk_zones, has_grid) {
        (Some(p), true) if gameplay.level_chunks == 0 => crate::terrain::ZoneKeys::Direct(p),
        (Some(p), true) if header_count == p.len() => crate::terrain::ZoneKeys::Indexed(p),
        _ => crate::terrain::ZoneKeys::Cleared,
    };
    let level_chunk_keys = matches!(zones, crate::terrain::ZoneKeys::Indexed(_));
    let mut zones = Some(zones);
    let mut terrain = None;
    let mut terrain_textures = None;
    let mut tie_textures = None;
    let mut shrub_textures = None;
    let mut moby_textures = None;
    let mut sky = None;
    let mut effect_textures = None;
    let mut quad_textures = None;
    let mut quad_region = None;
    let mut shrub_data: BTreeMap<u64, crate::shrub::ShrubClass> = BTreeMap::new();
    let mut tie_data: BTreeMap<u64, crate::tie::TieClass> = BTreeMap::new();
    let mut moby_data: BTreeMap<u64, crate::moby::MobyClass> = BTreeMap::new();
    let mut by: BTreeMap<u64, &'static str> = BTreeMap::new();
    for &(o, k) in offsets {
        if o < root.size {
            by.entry(o).or_insert(k);
        }
    }
    let ends: Vec<u64> = by.keys().copied().skip(1).chain(std::iter::once(root.size)).collect();
    for ((o, kind), end) in by.iter().zip(ends) {
        if kind.starts_with("collision") {
            // collision.rs: a bad value fails its line (the tracker lists it).
            let _ = crate::collision::collision(t, root, *o, &core[*o as usize..end as usize]);
            continue;
        }
        if kind.starts_with("texture data") {
            let data = &core[*o as usize..end as usize];
            texels(t, root, *o, end - o, textures, data);
            let table = |k: usize, what: &str| -> Result<Vec<Texture>, String> {
                textures.iter().filter(|r| r.table == k).enumerate().map(|(i, r)| texture(r, data, idx, gs, gs_images).map_err(|e| format!("{what} texture {i}: {e}"))).collect()
            };
            terrain_textures = Some(table(0, "terrain")?);
            tie_textures = Some(table(2, "tie")?);
            shrub_textures = Some(table(3, "shrub")?);
            moby_textures = Some(table(1, "moby")?);
            continue;
        }
        if kind.starts_with("terrain") {
            let z = zones.take().ok_or("the core index gives two terrains")?;
            terrain = Some(crate::terrain::terrain(t, root, *o, &core[*o as usize..end as usize], &gameplay.light_sets, z)?);
            continue;
        }
        let piece = &core[*o as usize..end as usize];
        // coremisc.rs: a bad value fails its region's line (the tracker lists it).
        let misc = match *kind {
            k if k.starts_with("sky") => {
                sky = Some(crate::coremisc::sky(t, root, *o, piece)?);
                Some(Ok(()))
            }
            k if k.starts_with("region at word 0x19") => {
                effect_textures = Some(crate::coremisc::region_19(t, root, *o, piece, idx)?);
                Some(Ok(()))
            }
            k if k.starts_with("region at word 0x1A") => {
                quad_textures = Some(crate::coremisc::region_1a(t, root, *o, piece, idx)?);
                quad_region = Some(*o as u32);
                Some(Ok(()))
            }
            k if k.starts_with("region at word 0x29") => Some(crate::coremisc::region_29(t, root, *o, piece)),
            k if k.starts_with("region at word 0x2A") => Some(crate::coremisc::region_2a(t, root, *o, piece)),
            k if k.starts_with("region at word 0x2C") => Some(crate::coremisc::region_2c(t, root, *o, piece)),
            k if k.starts_with("region at word 0x2D") => Some(crate::coremisc::region_2d(t, root, *o, piece)),
            k if k.starts_with("region at word 0x2E") => Some(crate::coremisc::region_2e(t, root, *o, piece)),
            _ => None,
        };
        if misc.is_some() {
            continue;
        }
        if kind.starts_with("slot sequence") {
            // The slots hold moby class 0's sequences: its joint count is that class's byte +8 (class 0 is the first record).
            let c0 = u32_at(idx, u32_at(idx, 4 * 7)? as usize)? as usize;
            let joints = *core.get(c0 + 8).ok_or_else(|| format!("moby class 0 at {c0:#x} past the core"))? as usize;
            crate::moby::slot_sequence(t, root, GC, *o, piece, joints)?;
            continue;
        }
        if *kind == "moby class data" {
            moby_data.insert(*o, crate::moby::class(t, root, GC, *o, &core[*o as usize..end as usize])?);
            continue;
        }
        if *kind == "tie class data" {
            tie_data.insert(*o, crate::tie::tie(t, root, *o, &core[*o as usize..end as usize])?);
            continue;
        }
        if *kind == "shrub class data" {
            shrub_data.insert(*o, crate::shrub::shrub(t, root, *o, &core[*o as usize..end as usize])?);
            continue;
        }
        if kind.starts_with("visibility grid") {
            crate::terrain::region3(t, root, *o, &core[*o as usize..end as usize])?;
            continue;
        }
        let l = t.line(GC, kind, "", "", Space::Unpacked);
        let n = t.child(root, l, *o, end - o);
        t.close(n);
    }
    let _ = LOADER;
    let terrain = terrain.ok_or("the core index gives no terrain")?;
    let terrain_textures = terrain_textures.ok_or("the core index gives no texture data")?;
    let (tie_textures, shrub_textures) = (tie_textures.ok_or("the core index gives no texture data")?, shrub_textures.ok_or("the core index gives no texture data")?);
    let moby_textures = moby_textures.ok_or("the core index gives no texture data")?;
    let records = |cw: usize, ow: usize, size: usize, what: &str| -> Result<Vec<ClassRecord>, String> {
        let (n, o) = (u32_at(idx, 4 * cw)? as usize, u32_at(idx, 4 * ow)? as usize);
        (0..n)
            .map(|r| {
                let at = o + size * r;
                let b = idx.get(at..at + 0x20).ok_or_else(|| format!("{what} class record {r} past the index"))?;
                let textures: [u8; 16] = b[0x10..0x20].try_into().expect("16 bytes");
                // +8 is the moby records' mode; zero in the tie and shrub records.
                Ok(ClassRecord { offset: u32_at(b, 0)? as u64, id: u32_at(b, 4)?, mode: u32_at(b, 8)?, textures })
            })
            .collect()
    };
    let tie_classes = records(8, 9, 32, "tie")?;
    let shrub_classes = records(10, 11, 48, "shrub")?;
    let moby_classes = records(6, 7, 32, "moby")?;
    let mobys = moby_classes
        .iter()
        .enumerate()
        .map(|(r, c)| match c.offset {
            0 => Ok(None),
            o => moby_data.remove(&o).map(Some).ok_or_else(|| format!("moby class record {r}: no class data at core offset {o:#x}")),
        })
        .collect::<Result<Vec<_>, String>>()?;
    let ties = tie_classes
        .iter()
        .enumerate()
        .map(|(r, c)| tie_data.remove(&c.offset).ok_or_else(|| format!("tie class record {r}: no class data at core offset {:#x}", c.offset)))
        .collect::<Result<Vec<_>, String>>()?;
    let shrubs = shrub_classes
        .iter()
        .enumerate()
        .map(|(r, c)| shrub_data.remove(&c.offset).ok_or_else(|| format!("shrub class record {r}: no class data at core offset {:#x}", c.offset)))
        .collect::<Result<Vec<_>, String>>()?;
    for (k, c) in terrain.chunks.iter().enumerate() {
        if let Some(m) = c.materials.iter().find(|m| m.texture as usize >= terrain_textures.len()) {
            return Err(format!("terrain chunk {k}: texture {} of {} terrain texture records", m.texture, terrain_textures.len()));
        }
    }
    let special_textures = special_textures(idx, gs, gs_images)?;
    let effect_textures = effect_textures.ok_or("the core index gives no region at word 0x19")?;
    let quad_textures = quad_textures.ok_or("the core index gives no region at word 0x1A")?;
    let quad_region = quad_region.expect("set with the region's textures");
    Ok(Core { effect_textures, quad_textures, quad_region, terrain, terrain_textures, tie_textures, shrub_textures, moby_textures, tie_classes, shrub_classes, shrubs, ties, moby_classes, mobys, sky, level_chunk_keys, has_grid, special_textures })
}

/// A 16 x 16 PSMCT32 palette image the GS reads as CSM1: entry i is at texel j (index bits 3 and 4 swapped). RGBA, alpha as stored
/// (0x80 = 1.0).
fn clut_colours(gs: &[u8], pal: &crate::hud::GsRamImage) -> Vec<[u8; 4]> {
    let raw = &gs[pal.offset..pal.offset + 0x400];
    (0..256usize)
        .map(|i| {
            let j = (i & !0x18) | ((i & 8) << 1) | ((i & 0x10) >> 1);
            [raw[4 * j], raw[4 * j + 1], raw[4 * j + 2], raw[4 * j + 3]]
        })
        .collect()
}

/// The texture the GS reads at `offset` bytes from the GS RAM image's base (the word at 0x1A1EF0: `GsRamImage`) as a PSMT8
/// `width` x `height` image through the palette at `clut` bytes from the base (CSM1): the first loop's image there, which the
/// EE's own packets sample where it stays in GS memory (level 2's world jobs).
pub fn gs_ram_texture(gs: &[u8], gs_images: &[crate::hud::GsRamImage], offset: usize, width: usize, height: usize, clut: usize) -> Result<Texture, String> {
    let img = gs_images.iter().find(|g| !g.clut && g.offset == offset).ok_or_else(|| format!("no 8-bit image of the GS RAM table at offset {offset:#x}"))?;
    if (img.width, img.height) != (width, height) {
        return Err(format!("the GS RAM table image at offset {offset:#x} is {} x {}, sampled as {width} x {height}", img.width, img.height));
    }
    let pal = gs_images.iter().find(|g| g.clut && g.offset == clut).ok_or_else(|| format!("no palette of the GS RAM table at offset {clut:#x}"))?;
    let colours = clut_colours(gs, pal);
    let texels = gs.get(offset..offset + width * height).ok_or_else(|| format!("texels {offset:#x} past the GS RAM image"))?;
    Ok(Texture { width, height, levels: vec![texels.iter().map(|&p| colours[p as usize]).collect()], gs: Some(GsLevel { psm: 0x13, texels: texels.to_vec(), clut: colours }) })
}

/// The moby textures -2 and -3 (a packet A texture setup whose `local` is -2 or -3; Ratchet's helmet uses -2): index 0 is -2, a
/// 128 x 128 texture, index 1 is -3, 64 x 64; one level each (TEX1 MXL 0 in func_L01_00423700's packet).
///
/// func_L01_00428750 at 0x428840 to 0x428920 builds their TEX0 from the core index: TBP = (the texture area base, word 0x34 of
/// the GS layout at 0x1A1ED0, + 4 x index word 0x24 (-2) or 0x26 (-3)) >> 8, CBP = (the CLUT base, word 0x20 of the layout
/// = 0x1A1EF0, + word 0x25 or 0x27) >> 8, PSM 0x1B (PSMT8H), TW = TH = 7 (-2) or 6 (-3) (constants of the loader: 0x428854 and 0x428848 for -2,
/// 0x4288C8, 0x428890 and 0x4288D8 for -3; not read from the data), TCC 1, CSM1, CSA 0, CLD 4; stored at 0x1DFF10 and 0x1DFF28.
/// func_L01_00423B48 (called from func_L01_00428750 at 0x428814 with part 2 and the GS RAM table) puts the data there: its second
/// loop (0x423CF8 to 0x423D7C) sends the table's images (index word 0x21 entries from the table's entry n0) one after another to
/// GS blocks starting at the texture area base (a2 = block, PSM 0x1B = PSMT8H), each image advancing the GS address by width x
/// height x 4 (0x423D5C: s5 += s0 << 2) while the entry's +0xC holds the running width x height; so word 0x24 / 0x26 is the +0xC
/// of the image that is at the TBP. Its first loop (0x423BA8 to 0x423CD8) sends the palettes (16 x 16 PSMCT32) from the CLUT base
/// on, each at its offset in part 2, so word 0x25 / 0x27 is the offset of the palette in part 2. On all 34 story levels the words
/// are 0, 0, 0x4000, 0x400: -2 is the second loop's first image (128 x 128) with the palette at offset 0, -3 its second (64 x 64)
/// with the palette at 0x400. Anything else fails.
/// The image of the GS RAM table that func_L01_00423B48's second loop sends to the texture area at running size `at` (the entry
/// among the index word 0x21 entries from entry n0 whose +0xC is `at`; its GS block is the area's base + 4 x `at` / 256): its
/// width, height and the offset of its texels in the GS RAM image.
fn gs_table_image(idx: &[u8], at: usize) -> Result<(usize, usize, usize), String> {
    let w = |i: usize| u32_at(idx, 4 * i).map(|v| v as usize);
    let (n0, tab, n1) = (w(0)?, w(1)?, w(0x21)?);
    let image = (n0..n0 + n1)
        .map(|e| tab + 16 * e)
        .find(|&e| u32_at(idx, e + 0xc).map(|v| v as usize) == Ok(at))
        .ok_or_else(|| format!("no GS RAM table image starts at running size {at:#x}"))?;
    let wh = u32_at(idx, image + 4)?;
    Ok(((wh & 0xffff) as usize, (wh >> 16) as usize, u32_at(idx, image + 8)? as usize))
}

pub fn special_textures(idx: &[u8], gs: &[u8], gs_images: &[crate::hud::GsRamImage]) -> Result<[Texture; 2], String> {
    let w = |i: usize| u32_at(idx, 4 * i).map(|v| v as usize);
    let mut out = Vec::new();
    for (k, &(tex_word, pal_word, size)) in [(0x24usize, 0x25usize, 128usize), (0x26, 0x27, 64)].iter().enumerate() {
        let (tw, cw) = (w(tex_word)?, w(pal_word)?);
        let which = ["-2", "-3"][k];
        let (iw, ih, o8) = gs_table_image(idx, tw).map_err(|e| format!("moby texture {which}: {e}"))?;
        if (iw, ih) != (size, size) {
            return Err(format!("moby texture {which}: the image at {tw:#x} is {iw} x {ih}, the loader's TEX0 says {size} x {size}"));
        }
        let pal = gs_images.iter().find(|g| g.clut && g.offset == cw).ok_or_else(|| format!("moby texture {which}: no palette at offset {cw:#x} of the GS RAM image"))?;
        let texels = gs.get(o8..o8 + size * size).ok_or_else(|| format!("moby texture {which}: texels {o8:#x}+{:#x} past the GS RAM image", size * size))?;
        let colours = clut_colours(gs, pal);
        out.push(Texture { width: size, height: size, levels: vec![texels.iter().map(|&p| colours[p as usize]).collect()], gs: Some(GsLevel { psm: 0x1B, texels: texels.to_vec(), clut: colours }) });
    }
    let b = out.pop().expect("two");
    let a = out.pop().expect("two");
    Ok([a, b])
}

/// The pixels of texture record `r`: levels 0 and 1 (w x h, then (w/2) x (h/2)) from `data` (the core's texture data) as the
/// uploaders stream them, levels 2 and 3 from the GS RAM image (`gs`) at their GS blocks; each 8-bit level through the
/// palette at the record's GS block, a 16 x 16 PSMCT32 image the GS reads as CSM1 (index bits 3 and 4 swapped).
///
/// A moby record with 0 levels is not streamed: its texture stays in GS memory, and its offset is the running size of the GS
/// RAM table image there (`gs_table_image`): func_L01_00423700 builds its TEX0 with TBP = the texture area's base + 4 x the
/// offset / 256 (Veldin: base block 0xF00, record 16's offset 0xA400 gives block 0x1190, the block the traced frame's GS
/// samples for Ratchet's helmet), PSM 0x1B (PSMT8H), one level; its texels are that image's.
pub fn texture(r: &TexRecord, data: &[u8], idx: &[u8], gs: &[u8], gs_images: &[crate::hud::GsRamImage]) -> Result<Texture, String> {
    let (w, h) = (r.width as usize, r.height as usize);
    let at_block = |blk: i16, what: &str| -> Result<&crate::hud::GsRamImage, String> {
        gs_images.iter().find(|g| blk >= 0 && g.offset == blk as usize * 0x100).ok_or_else(|| format!("{what} at GS block {blk} is no image of the GS RAM table"))
    };
    let pal = at_block(r.palette, "palette")?;
    if !pal.clut {
        return Err(format!("palette at GS block {} is an 8-bit image", r.palette));
    }
    let colours = clut_colours(gs, pal);
    // A moby record with 0 levels is kept in GS memory as one level (func_L01_00423700 at 0x4237AC builds its TEX0 with PSM
    // 0x1B, PSMT8H, and no mip levels) from the same 8-bit texels.
    if !(0..=4).contains(&r.levels) || (r.levels < 3) != (r.mip2 < 0) || (r.levels < 4) != (r.mip3 < 0) || (r.levels == 0 && r.table != 1) {
        return Err(format!("{} levels with mip blocks {} and {}", r.levels, r.mip2, r.mip3));
    }
    let o = r.offset as usize;
    let mut levels: Vec<&[u8]> = vec![if r.levels == 0 {
        let (iw, ih, o8) = gs_table_image(idx, o)?;
        if (iw, ih) != (w, h) {
            return Err(format!("the GS RAM table image at running size {o:#x} is {iw} x {ih}, the record {w} x {h}"));
        }
        gs.get(o8..o8 + w * h).ok_or_else(|| format!("texels {o8:#x}+{:#x} past the GS RAM image", w * h))?
    } else {
        &data[o..o + w * h]
    }];
    if r.levels >= 2 {
        levels.push(&data[o + w * h..o + w * h + (w / 2) * (h / 2)]);
    }
    for (lv, blk) in [(2usize, r.mip2), (3, r.mip3)] {
        if r.levels as usize > lv {
            let g = at_block(blk, "mip level")?;
            if !g.clut && g.width <= w >> lv && g.height <= h >> lv && (g.width, g.height) != (w >> lv, h >> lv) && g.width * g.height < 0x100 {
                // A smaller image in the level's one GS block (level 36: two 64 x 64 tie textures name block 36, which holds an
                // 8 x 8 image, for their 16 x 16 level 2): the GS reads that image in one corner and GS memory that no upload of
                // the level writes in the rest, so the level is not on the disc. The levels before it are returned.
                break;
            }
            if g.clut || g.width != w >> lv || g.height != h >> lv {
                return Err(format!("mip level {lv} at GS block {blk}: {} x {} (clut {}), expected {} x {}", g.width, g.height, g.clut, w >> lv, h >> lv));
            }
            levels.push(&gs[g.offset..g.offset + g.width * g.height]);
        }
    }
    // The GS samples a record of 0 levels as PSMT8H (above), the others as PSMT8 (func_L01_00424DE8).
    let gs = GsLevel { psm: if r.levels == 0 { 0x1B } else { 0x13 }, texels: levels[0].to_vec(), clut: colours.clone() };
    Ok(Texture { width: w, height: h, levels: levels.iter().map(|l| l.iter().map(|&p| colours[p as usize]).collect()).collect(), gs: Some(gs) })
}

/// The texture data: each record's texels, 8-bit, a w x h base level then one (w/2) x (h/2) mip, as the uploaders read them
/// (func_L01_004AEA08 terrain, func_L01_004B27F0 tie, func_L01_004A3AD8 shrub, func_L01_0043E6E8 moby, which uploads base and
/// mip or the mip alone by distance; the loader tags each pointer with log2 w through plzcw, and the uploaders read the mip
/// right after the base). The data is a shared pool: records of all four tables interleave and overlap, so each byte is
/// claimed once, in table order. Zero runs no record reaches are padding.
fn texels(t: &mut Tracker, root: &mut Node, base: u64, size: u64, textures: &[TexRecord], data: &[u8]) {
    let mut region = t.region(root, "LEVEL core texture data (unpacked)", base, size);
    const NAMES: [(&str, &str); 4] = [
        ("terrain texture texels (8-bit: w x h, then a (w/2) x (h/2) mip)", "func_L01_004AEA08 uploads them to the GS"),
        ("moby texture texels (8-bit: w x h, then a (w/2) x (h/2) mip)", "func_L01_0043E6E8 uploads them (base and mip, or the mip alone, by distance)"),
        ("tie texture texels (8-bit: w x h, then a (w/2) x (h/2) mip)", "func_L01_004B27F0 uploads them to the GS"),
        ("shrub texture texels (8-bit: w x h, then a (w/2) x (h/2) mip)", "func_L01_004A3AD8 uploads them to the GS"),
    ];
    let mut claimed = crate::track::Ranges::default();
    for (k, (name, game)) in NAMES.iter().enumerate() {
        let mut pieces = Vec::new();
        let mut bad = None;
        for r in textures {
            let (table, off, w, h) = (r.table, r.offset, r.width, r.height);
            if table != k {
                continue;
            }
            let len = w * h + (w / 2) * (h / 2);
            if !w.is_power_of_two() || !h.is_power_of_two() || off + len > size {
                bad = Some(format!("record at {off:#x}: {w} x {h} texels past the texture data's {size:#x} bytes"));
                continue;
            }
            for (a, b) in claimed.missing(off, off + len) {
                pieces.push((a, b - a));
                claimed.add(a, b);
            }
        }
        if pieces.is_empty() && bad.is_none() {
            continue;
        }
        pieces.sort_unstable();
        let l = t.line("LEVEL core texture data (unpacked)", name, "core::texels", game, Space::Unpacked);
        let mut n = t.pieces(&mut region, l, &pieces);
        let total = n.size;
        n.mark(0, total);
        if let Some(e) = bad {
            n.fail(e);
        }
        t.close(n);
    }
    crate::walk::pad_mem(t, &mut region, data, "LEVEL core texture data (unpacked)");
    t.close(region);
}
