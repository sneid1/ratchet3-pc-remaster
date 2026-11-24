//! `uyafmt disc`: the walk over the whole disc. Every sector is claimed by a file or a volume structure; every file is split into
//! its structures by the decoders that know it, and what no decoder claims is listed as its file kind's unknown bytes.

use crate::disc::{Disc, SECTOR, u32_at};
use crate::headers;
use crate::iso;
use crate::wads;
use crate::lz;
use crate::toc::{self, FileKind};
use crate::track::{Node, Space, Tracker};
use std::collections::BTreeSet;
use std::path::Path;

pub struct Walk {
    pub tracker: Tracker,
    pub disc_bytes: u64,
    pub overlays: usize,
    pub overlays_recompiled: usize,
}

/// The levels whose overlay is recompiled: generated/uya/rec/r3l<N>_* beside the disc folder's parent.
fn recompiled_levels(iso_path: &str) -> BTreeSet<usize> {
    let root = Path::new(iso_path).parent().and_then(|p| p.parent()).unwrap_or(Path::new("."));
    let mut out = BTreeSet::new();
    if let Ok(rd) = std::fs::read_dir(root.join("generated/uya/rec")) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if let Some(rest) = n.strip_prefix("r3l") {
                if let Some(num) = rest.split('_').next() {
                    if let Ok(v) = num.parse::<usize>() {
                        out.insert(v);
                    }
                }
            }
        }
    }
    out
}

/// Why a zero chunk-list entry (SCENE records, WAD4 comic records) is read by nothing.
pub(crate) const ZERO_ENTRY: &str = "read by nothing: func_L01_004296B0 loads the entry as the read-ahead of a segment's last chunk and nothing unpacks it (func_L01_00433EA0 traps on a zero header; the players func_L01_004B66E0 and func_L01_004A9F80 stop at the tick count in the last unpacked chunk's header, and the first zero entry's index is ceil(ticks / 0x60) of chunk 0 in every record); entries after the first zero are reached by no scene start (func_L01_004B5C78, func_L01_004A91D8) or tick jump found";

/// The multiplayer menu and maps, left out of the decode by the player (12 November 2026).
fn multiplayer(level: usize) -> bool {
    (39..=55).contains(&level)
}

/// The files only online multiplayer uses (the network configuration GUI, its save icons in /CNF/, and DNAS), left out with
/// it.
fn online(path: &str) -> bool {
    path.starts_with("/NETGUI/") || path.starts_with("/CNF/") || path == "/DNAS300.IMG"
}

/// The Sly 2 demo on the disc (its program, archive and IOP image), left out by the player (12 November 2026).
fn sly2(path: &str) -> bool {
    path.starts_with("/SLY2/")
}

pub fn walk(iso_path: &str) -> Result<Walk, String> {
    let mut d = Disc::open(iso_path)?;
    let mut t = Tracker::new();
    let recompiled = recompiled_levels(iso_path);

    // ISO 9660 volume structures.
    let v = iso::volume(&mut d)?;
    if v.space != d.sectors {
        return Err(format!("the volume descriptor gives {} sectors, the image has {}", v.space, d.sectors));
    }
    // The volume structures (ISO 9660 and UDF) in sectors 0-999 and the disc's last 13 sectors: volume.rs.
    crate::volume::structures(&mut t, &mut d, &v)?;

    // Where each file starts: a global WAD runs to the next file.
    let tc = toc::read(&mut d)?;
    let mut starts: BTreeSet<u64> = BTreeSet::new();
    for f in &v.files {
        starts.insert(f.sector);
    }
    for f in &tc.files {
        starts.insert(f.sector);
    }
    for w in &tc.wads {
        starts.insert(w.sector);
    }
    let mut regions = [0u64; 8];
    for (k, w) in tc.wads.iter().enumerate() {
        regions[k] = starts.range(w.sector + 1..).next().copied().unwrap_or(d.sectors) - w.sector;
    }

    // The files in the ISO 9660 directory. Program files are code lines.
    for f in &v.files {
        if online(&f.path) {
            t.excluded("online-play files (network GUI, DNAS)", &f.path, f.sector, f.sectors());
            continue;
        }
        if sly2(&f.path) {
            t.excluded("the Sly 2 demo (/SLY2/)", &f.path, f.sector, f.sectors());
            continue;
        }
        t.claim(f.sector, f.sectors(), &format!("file {}", f.path));
        let group = format!("ISO file {}", f.path);
        let name = f.path.rsplit('/').next().unwrap_or("").to_string();
        let mut r = t.root(&group, &name, f.sectors() * SECTOR, Space::Disc);
        let upper = name.to_ascii_uppercase();
        let code = upper == "SCUS_973.53" || upper.ends_with(".ELF") || upper.ends_with(".IRX") || upper.ends_with(".IMG");
        if code {
            let what = match upper.as_str() {
                "SCUS_973.53" => "SCUS_973.53: the boot program, boot_elf inside it".to_string(),
                "I5BOOTN.ELF" => "I5BOOTN.ELF: the launcher".to_string(),
                _ if upper.ends_with(".IRX") || upper.ends_with(".IMG") => "IOP modules and module images".to_string(),
                _ => format!("{}: a program on the disc", f.path),
            };
            let l = t.code_line("Code", &what, false);
            let c = t.child(&mut r, l, 0, f.size);
            t.close(c);
        }
        if f.path == "/SYSTEM.CNF" {
            let bytes = d.bytes_at(f.sector * SECTOR, f.size)?;
            // A bad value fails the node (the tracker lists it); the walk goes on.
            let _ = crate::volume::system_cnf(&mut t, &mut r, &bytes);
        }
        if upper == "Z6TAIL.DUP" {
            // A file of zeros that places the game's data on the disc; nothing reads it.
            let l = t.line(&group, "zero-filled file", "walk::zero_file", crate::track::NOTHING, Space::Disc);
            let mut n = t.child(&mut r, l, 0, f.size);
            let mut at = 0;
            while at < f.size {
                let len = (f.size - at).min(0x400000);
                if d.bytes_at(f.sector * SECTOR + at, len)?.iter().any(|&v| v != 0) {
                    n.fail(format!("a non-zero byte in {at:#x}+{len:#x}"));
                    break;
                }
                at += len;
            }
            if !n.failed() {
                n.mark(0, f.size);
            }
            t.close(n);
        }
        pad_disc(&mut t, &mut d, &mut r, f.sector * SECTOR, "ISO 9660 files")?;
        t.close(r);
    }

    // The table of contents.
    t.claim(toc::TOC_SECTOR, toc::TOC_SECTORS, "table of contents");
    let mut r = t.root("Table of contents", "TOC", toc::TOC_SECTORS * SECTOR, Space::Disc);
    if tc.wads_end != toc::LEVEL_TABLE {
        return Err(format!("the WAD headers end at {:#x}, the level table is at {:#x}", tc.wads_end, toc::LEVEL_TABLE));
    }
    headers::toc(&mut t, &mut r, &tc, &regions);
    t.close(r);

    // The level headers' sectors.
    for f in &tc.files {
        let (_, hsec) = f.kind.header();
        let holder = format!("L{:02}", f.level);
        if multiplayer(f.level) {
            t.excluded("multiplayer levels 39-55: header sectors", &holder, f.header_sector, hsec);
            continue;
        }
        t.claim(f.header_sector, hsec, &format!("{holder} {} header", f.kind.name()));
        let mut r = t.root("Level headers", &holder, hsec * SECTOR, Space::Disc);
        headers::header_line(&mut t, &mut r, f, 0, &f.header, false);
        t.close(r);
    }

    // The global WADs: from the WAD's sector to the next file's.
    for (k, w) in tc.wads.iter().enumerate() {
        let name = format!("WAD{}", k + 1);
        t.claim(w.sector, regions[k], &name);
        let mut r = t.root(&name, &name, regions[k] * SECTOR, Space::Disc);
        wads::wad(&mut t, &mut d, &mut r, k, w, &tc.bytes[w.at..w.at + w.size], regions[k], &recompiled)?;
        pad_disc(&mut t, &mut d, &mut r, w.sector * SECTOR, &name)?;
        t.close(r);
    }

    // AUDIO, LEVEL and SCENE files.
    let mut overlays = 0;
    let mut overlays_recompiled = 0;
    for f in &tc.files {
        let holder = format!("L{:02}", f.level);
        let g = f.kind.name();
        if multiplayer(f.level) {
            t.excluded(&format!("multiplayer levels 39-55: {g} files"), &holder, f.sector, f.sectors);
            continue;
        }
        t.claim(f.sector, f.sectors, &format!("{holder} {g}"));
        let mut r = t.root(g, &holder, f.sectors * SECTOR, Space::Disc);
        let (_, hsec) = f.kind.header();
        let first = d.sectors(f.sector, hsec)?;
        headers::header_line(&mut t, &mut r, f, 0, &first, true);
        match f.kind {
            FileKind::Level => {
                let rec = recompiled.contains(&f.level);
                overlays += 1;
                overlays_recompiled += rec as usize;
                level_file(&mut d, &mut t, &mut r, f, rec)?;
            }
            FileKind::Audio => audio_file(&mut t, &mut d, &mut r, f)?,
            FileKind::Scene => scene_file(&mut t, &mut d, &mut r, f)?,
        }
        pad_disc(&mut t, &mut d, &mut r, f.sector * SECTOR, g)?;
        t.close(r);
    }

    t.finish_sectors(d.sectors)?;
    Ok(Walk { tracker: t, disc_bytes: d.bytes, overlays, overlays_recompiled })
}

/// Story level `level`'s LEVEL file, decoded by the functions the walk runs: what the port loads.
pub fn level(iso_path: &str, level: usize) -> Result<LevelData, String> {
    if multiplayer(level) {
        return Err(format!("level {level} is a multiplayer level"));
    }
    let mut d = Disc::open(iso_path)?;
    let tc = toc::read(&mut d)?;
    let f = tc.files.iter().find(|f| f.level == level && f.kind == FileKind::Level).ok_or_else(|| format!("no LEVEL file for level {level}"))?;
    let mut t = Tracker::new();
    let holder = format!("L{level:02}");
    let mut r = t.root(f.kind.name(), &holder, f.sectors * SECTOR, Space::Disc);
    let data = level_file(&mut d, &mut t, &mut r, f, false)?;
    t.close(r);
    if data.core.is_none() {
        return Err(format!("L{level:02}: the core does not unpack"));
    }
    Ok(data)
}

/// The bytes of `node` (at `disc_at` on the disc) that no structure holds and that are zero: padding between structures,
/// which nothing reads. A hole that is not all zero stays unknown.
pub fn pad_disc(t: &mut Tracker, d: &mut Disc, node: &mut Node, disc_at: u64, group: &str) -> Result<(), String> {
    let holes = node.holes();
    let l = t.line(group, "zero padding between structures", "walk::pad", crate::track::NOTHING, Space::Disc);
    for (a, b) in holes {
        if b - a > 0x100000 {
            continue;
        }
        let bytes = d.bytes_at(disc_at + a, b - a)?;
        if bytes.iter().all(|&v| v == 0) {
            let mut c = t.child(node, l, a, b - a);
            c.mark(0, b - a);
            t.close(c);
        }
    }
    Ok(())
}

/// The same for a node whose bytes are in memory.
pub fn pad_mem(t: &mut Tracker, node: &mut Node, bytes: &[u8], group: &str) {
    let holes = node.holes();
    let l = t.line(group, "zero padding between structures", "walk::pad", crate::track::NOTHING, node.space);
    for (a, b) in holes {
        if bytes[a as usize..b as usize].iter().all(|&v| v == 0) {
            let mut c = t.child(node, l, a, b - a);
            c.mark(0, b - a);
            t.close(c);
        }
    }
}

/// The SCENE file's blocks, from its header's 30 records (headers::scene_header): each record's header block, its second
/// block and its chunks. What is in them, and the records' words 0-1 and 4-11, are not decoded yet.
fn scene_file(t: &mut Tracker, d: &mut Disc, file: &mut Node, f: &toc::LevelFile) -> Result<(), String> {
    for r in 0..30usize {
        let at = 8 + r * 0x14c;
        let w = |i: usize| u32_at(&f.header, at + 4 * i).map(|v| v as u64);
        // Words 2-3: the record's header block, its subtitle table (func_L01_00429618 loads it into 0x1E6E4C).
        let (hs, hc) = (w(2)?, w(3)?);
        if hs != 0 && hc != 0 && hs + hc <= f.sectors && !file.overlaps(hs * SECTOR, hc * SECTOR) {
            let b = d.sectors(f.sector + hs, hc)?;
            crate::subs::table(t, file, "SCENE", hs * SECTOR, &b);
        }
        // Words 0-1, 4-11: VAG clips.
        for i in [0usize, 1, 4, 5, 6, 7, 8, 9, 10, 11] {
            let s = w(i)?;
            if s == 0 || s >= f.sectors {
                continue;
            }
            let h = d.bytes_at((f.sector + s) * SECTOR, 0x30)?;
            let size = 0x30 + crate::disc::u32_be(&h, 0xc)? as u64;
            if &h[0..4] != b"VAGp" || s * SECTOR + size > f.sectors * SECTOR || file.overlaps(s * SECTOR, size) {
                continue;
            }
            let bytes = d.bytes_at((f.sector + s) * SECTOR, size)?;
            crate::vag::clip(t, file, "SCENE", s * SECTOR, &bytes, "func_L01_0045B5A0 resolves it by language, func_0013CB20 streams it to the IOP");
        }
        // Words 12-13: an LZ stream (func_L01_00429450 loads it, func_L01_004294D0 unpacks it); words 14 on: chunk
        // boundaries, each chunk an LZ stream (func_L01_004296B0 loads it).
        let mut blocks = vec![("scene block (words 12-13): LZ stream", "SCENE block (unpacked)", w(12)?, w(13)?)];
        for i in 14..82 {
            let (a, b) = (w(i)?, w(i + 1)?);
            if a == 0 || b <= a {
                break;
            }
            blocks.push(("scene chunk: LZ stream", "SCENE chunk (unpacked)", a, b - a));
        }
        for (name, group, s, c) in blocks {
            if s == 0 || c == 0 || s + c > f.sectors || file.overlaps(s * SECTOR, c * SECTOR) {
                continue;
            }
            let bytes = d.sectors(f.sector + s, c)?;
            if bytes.iter().all(|&v| v == 0) {
                // A chunk-list entry that holds zero sectors: loaded as a read-ahead and never unpacked.
                let l = t.ignored_line("SCENE", "zero chunk-list entry", ZERO_ENTRY, Space::Disc);
                let z = t.child(file, l, s * SECTOR, c * SECTOR);
                t.close(z);
                continue;
            }
            let l = t.line("SCENE", name, "lz::unpack", "func_L01_00433EA0 (each program's copy)", Space::Disc);
            let is_block = group == "SCENE block (unpacked)";
            lz_block(t, file, l, &bytes, s * SECTOR, c * SECTOR, group, move |t, r, d| if is_block { crate::scene::block(t, r, d).map(|_| ()) } else { crate::scene::chunk(t, r, d) })?;
        }
    }
    Ok(())
}

/// The AUDIO file's streams: the header's (sector, bytes) pairs from +8. The game streams a track from one pair's sector to the
/// next pair's (func_L01_0045BA68 ...); what is in a stream is not decoded yet.
fn audio_file(t: &mut Tracker, d: &mut Disc, file: &mut Node, f: &toc::LevelFile) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for p in 0..(0x1818 - 8) / 8 {
        let (s, c) = (u32_at(&f.header, 8 + 8 * p)? as u64, u32_at(&f.header, 12 + 8 * p)? as u64);
        if s == 0 || !seen.insert(s) {
            continue;
        }
        if s * SECTOR + c > f.sectors * SECTOR || file.overlaps(s * SECTOR, c) {
            continue;
        }
        // A mono VAG clip (each stereo track is two: left and right).
        let bytes = d.bytes_at((f.sector + s) * SECTOR, c)?;
        crate::vag::clip(t, file, "AUDIO", s * SECTOR, &bytes, "func_0013CB20 (func_L01_0045BA68 and the other AUDIO readers stream it to the IOP)");
    }
    Ok(())
}

/// The LEVEL file's blocks: the header's (sector offset, sectors) pairs from +0x10.
/// What a LEVEL file gives the port: the core (part 9 of block 0) and the level chunks' terrains (blocks 4-6), each None when its
/// stream does not unpack (the walk lists the error).
pub struct LevelData {
    pub core: Option<crate::core::Core>,
    /// Level chunk N's terrain (block 4 + N: func_L01_004282C0 reads its sector and size at the LEVEL header +0x30 + 8N), None
    /// when the file has no such block or its terrain stream does not unpack.
    pub chunk_terrains: [Option<crate::terrain::Terrain>; 3],
    pub gameplay: Option<crate::gameplay::Gameplay>,
    /// The HUD's tables and banks (`hud::hud`).
    pub hud: Option<crate::hud::HudData>,
    /// The GS RAM image (level data part 2) and its first loop's images (`hud::gs_ram`).
    pub gs_ram: Option<(Vec<u8>, Vec<crate::hud::GsRamImage>)>,
}

fn level_file(d: &mut Disc, t: &mut Tracker, file: &mut Node, f: &toc::LevelFile, recompiled: bool) -> Result<LevelData, String> {
    const NAMES: [&str; 4] = ["level data", "sound bank", "gameplay", "occlusion"];
    let mut out = LevelData { core: None, chunk_terrains: [None, None, None], gameplay: None, hud: None, gs_ram: None };
    // The gameplay block (2) first: its light sets light the terrain of blocks 0 and 4-6 (func_L01_004252F0 copies them to
    // 0x1F17C0 before it calls the terrain light pass).
    for k in std::iter::once(2).chain((0..10).filter(|&k| k != 2)) {
        let (off, n) = (u32_at(&f.header, 0x10 + 8 * k)? as u64, u32_at(&f.header, 0x14 + 8 * k)? as u64);
        if n == 0 {
            continue;
        }
        if off + n > f.sectors {
            return Err(format!("L{:02} LEVEL block {k}: sectors {off}+{n} past the file's {}", f.level, f.sectors));
        }
        let bytes = d.sectors(f.sector + off, n)?;
        let at = off * SECTOR;
        let size = n * SECTOR;
        match k {
            0 => (out.core, out.hud, out.gs_ram) = level_data(t, file, &bytes, at, f.level, recompiled, gameplay(&out.gameplay, f.level)?)?,
            2 => {
                let l = t.line("LEVEL", "gameplay block: LZ stream", "lz::unpack", "func_L01_00433EA0 (each program's copy)", Space::Disc);
                out.gameplay = lz_data(t, file, l, &bytes, at, size, "LEVEL gameplay (unpacked)", |t, root, data| crate::gameplay::gameplay(t, root, data)).map_err(|e| format!("L{:02} gameplay: {e}", f.level))?;
            }
            3 => crate::occl::block3(t, file, at, &bytes)?,
            4..=6 => {
                let g = gameplay(&out.gameplay, f.level)?;
                let core = out.core.as_ref().ok_or_else(|| format!("L{:02}: a level chunk, and the core does not unpack", f.level))?;
                let zones = match (&g.chunk_zones, core.level_chunk_keys) {
                    (Some(p), true) => crate::terrain::ZoneKeys::Indexed(p),
                    _ => crate::terrain::ZoneKeys::Cleared,
                };
                out.chunk_terrains[k - 4] = level_chunk(t, file, &bytes, at, size, &g.light_sets, zones)?
            }
            1 | 7..=9 => crate::sbank::bank(t, file, "LEVEL", at, &bytes, "func_L01_0045B378 and frontbin func_0039C170 (block 1), func_L01_0045B3D0 and frontbin func_0039C210 (blocks 7-9: banks 1-3) send the sector to the IOP (func_0013BAB8, RPC 3); its 989snd module (WAD2 pair 1 entry 7) FUN_00008bac loads it").map_err(|e| format!("L{:02} LEVEL block {k}: {e}", f.level))?,
            _ => {
                let name = format!("block {k}: {}", NAMES[k]);
                let l = t.line("LEVEL", &name, "", "", Space::Disc);
                let c = t.child(file, l, at, size);
                t.close(c);
            }
        }
    }
    Ok(out)
}

/// The level's gameplay block, which the walk decodes before the blocks whose terrain it lights.
fn gameplay(g: &Option<crate::gameplay::Gameplay>, level: usize) -> Result<&crate::gameplay::Gameplay, String> {
    g.as_ref().ok_or_else(|| format!("L{level:02}: the gameplay block (the level's light sets) does not unpack"))
}

/// Blocks 4-6, a level chunk, as func_L01_004282C0 loads it: a 16-byte header (+0 and +4 the offsets of two LZ streams,
/// +8..+0x10 zero), stream A (terrain: func_L01_00424DE8 sets it up with the terrain texture records) and stream B
/// (collision: func_L01_004250B0), then zero padding.
fn level_chunk(t: &mut Tracker, file: &mut Node, b: &[u8], at: u64, size: u64, sets: &[crate::gameplay::LightSet], zones: crate::terrain::ZoneKeys) -> Result<Option<crate::terrain::Terrain>, String> {
    let mut region = t.region(file, "LEVEL level chunk", at, size);
    let l = t.line("LEVEL level chunk", "chunk header (offsets of the terrain and collision streams, zero words)", "walk::level_chunk", "func_L01_004282C0", Space::Disc);
    let mut h = t.child(&mut region, l, 0, 16);
    let (a, c) = (u32_at(b, 0)? as usize, u32_at(b, 4)? as usize);
    let ok = a == 16 && c > a && c < b.len() && b[8..16].iter().all(|&v| v == 0);
    if ok {
        h.mark(0, 16);
    } else {
        h.fail(format!("chunk header {:02x?}", &b[..16]));
    }
    t.close(h);
    let mut terrain = None;
    if ok {
        let la = t.line("LEVEL level chunk", "terrain stream: LZ stream", "lz::unpack", "func_L01_00433EA0 (called by func_L01_004282C0)", Space::Disc);
        terrain = lz_data(t, &mut region, la, &b[a..c], a as u64, (c - a) as u64, "LEVEL level chunk terrain (unpacked)", |t, root, data| {
            crate::terrain::terrain(t, root, 0, data, sets, zones)
        })?;
        let lc = t.line("LEVEL level chunk", "collision stream: LZ stream", "lz::unpack", "func_L01_00433EA0 (called by func_L01_004282C0)", Space::Disc);
        lz_block(t, &mut region, lc, &b[c..], c as u64, (b.len() - c) as u64, "LEVEL level chunk collision (unpacked)", |t, root, data| {
            let _ = crate::collision::collision(t, root, 0, data);
            Ok(())
        })?;
    }
    pad_mem(t, &mut region, b, "LEVEL level chunk");
    t.close(region);
    Ok(terrain)
}

/// Block 0, the level data: ten (offset, size) byte pairs at its start give its parts.
#[allow(clippy::type_complexity)]
fn level_data(t: &mut Tracker, file: &mut Node, b: &[u8], at: u64, level: usize, recompiled: bool, gameplay: &crate::gameplay::Gameplay) -> Result<(Option<crate::core::Core>, Option<crate::hud::HudData>, Option<(Vec<u8>, Vec<crate::hud::GsRamImage>)>), String> {
    const PARTS: [&str; 10] = ["overlay", "core index", "GS RAM image", "HUD header", "HUD bank 0", "HUD bank 1", "HUD bank 2", "HUD bank 3", "HUD bank 4", "core"];
    let mut region = t.region(file, "LEVEL level data", at, b.len() as u64);
    // Ten (offset, size) pairs, then a terminating pair (0xFFFFFFFF, 0). The front end loads block 0 to the buffer at
    // [0x1D4B50]; the readers of the pairs: func_0013AFF8 (boot_elf, from func_0013B330) the overlay's offset (+0),
    // func_L01_00428750 the core index (+8, +0xC), the GS RAM image's offset (+0x10) and the core stream's offset (+0x48),
    // func_L01_004233F8 the HUD header (+0x18, +0x1C) and the HUD banks' offsets and sizes (+0x20 to +0x44), func_L01_00423698
    // banks 0 and 1 (+0x20, +0x28). Sizes +4, +0x14 and +0x4C (the overlay copies records until its entry changes, the GS RAM
    // loader walks its own table, the core stream carries its own length) and the terminator are read by nothing.
    let l = t.line(
        "LEVEL level data",
        "part table (ten (offset, size) pairs and a (0xFFFFFFFF, 0) terminator)",
        "walk::level_data",
        "func_0013AFF8 (+0), func_L01_00428750 (+8, +0xC, +0x10, +0x48), func_L01_004233F8 (+0x18 to +0x44), func_L01_00423698 (+0x20, +0x28); +4, +0x14, +0x4C and the terminator are read by nothing (skipped)",
        Space::Disc,
    );
    let mut c = t.child(&mut region, l, 0, 0x58);
    if u32_at(b, 0x50)? == 0xffff_ffff && u32_at(b, 0x54)? == 0 {
        c.mark(0, 0x50);
        for o in [4u64, 0x14, 0x4c] {
            c.skip(o, 4);
        }
        c.skip(0x50, 8);
    } else {
        c.fail(format!("terminator {:#x}, {:#x}", u32_at(b, 0x50)?, u32_at(b, 0x54)?));
    }
    t.close(c);
    let mut core_offsets = Vec::new();
    let mut textures = Vec::new();
    let mut index: &[u8] = &[];
    let mut index_header: Option<Node> = None;
    let mut gs: &[u8] = &[];
    let mut gs_images = Vec::new();
    let mut core = None;
    let mut hud = None;
    let mut gs_ram = None;
    for (k, name) in PARTS.iter().enumerate() {
        let (o, s) = (u32_at(b, 8 * k)? as u64, u32_at(b, 8 * k + 4)? as u64);
        if s == 0 {
            continue;
        }
        if o + s > b.len() as u64 {
            return Err(format!("L{level:02} level data part {k} ({name}) at {o:#x}+{s:#x} past the block's {:#x} bytes", b.len()));
        }
        if k == 0 {
            let l = t.code_line("Code", if recompiled { "level overlay (EE program), recompiled levels" } else { "level overlay (EE program), levels not recompiled" }, recompiled);
            let c = t.child(&mut region, l, o, s);
            t.close(c);
        } else if k == 1 {
            index = &b[o as usize..(o + s) as usize];
            let (c, x, h) = crate::core::index(t, &mut region, o, index).map_err(|e| format!("L{level:02} core index: {e}"))?;
            (core_offsets, textures, index_header) = (c, x, Some(h));
        } else if k == 2 {
            gs = &b[o as usize..(o + s) as usize];
            gs_images = crate::hud::gs_ram(t, &mut region, o, gs, index).map_err(|e| format!("L{level:02} GS RAM image: {e}"))?;
            gs_ram = Some((gs.to_vec(), gs_images.clone()));
        } else if (3..=8).contains(&k) {
            match crate::hud::hud(t, &mut region, k, o, &b[o as usize..(o + s) as usize]).map_err(|e| format!("L{level:02} HUD part {k}: {e}"))? {
                crate::hud::HudPart::Header(h) => hud = Some(h),
                crate::hud::HudPart::Bank(n, data) => hud.as_mut().ok_or_else(|| format!("L{level:02} HUD bank {n} before the HUD header"))?.banks[n] = data,
            }
        } else if k == 9 {
            let l = t.line("LEVEL level data", "core: LZ stream", "lz::unpack", "func_L01_00433EA0 (each program's copy)", Space::Disc);
            let offs = core_offsets.clone();
            let tex = textures.clone();
            let mut h = index_header.take().ok_or_else(|| format!("L{level:02}: the core (part 9) before the core index (part 1)"))?;
            let gsi = std::mem::take(&mut gs_images);
            core = lz_data(t, &mut region, l, &b[o as usize..(o + s) as usize], o, s, "LEVEL core (unpacked)", move |t, root, data| {
                if let Err(e) = crate::coremisc::header_words(&mut h, index, s, data.len() as u64) {
                    h.fail(e);
                }
                t.close(h);
                crate::core::split(t, root, &offs, &tex, index, data, gs, &gsi, gameplay).map_err(|e| format!("L{level:02} core: {e}"))
            })?;
        } else {
            let l = t.line("LEVEL level data", name, "", "", Space::Disc);
            let c = t.child(&mut region, l, o, s);
            t.close(c);
        }
    }
    pad_mem(t, &mut region, b, "LEVEL level data");
    t.close(region);
    Ok((core, hud, gs_ram))
}

/// An LZ stream at `at` in `parent` (`size` bytes, the stream then padding): the stream's bytes are read by the unpacker; what it
/// does not read must be zero. The unpacked block becomes a root of `group`, split by `inside`.
/// An LZ stream's header bytes 7-15 (at +7 of the stream node `n`): read by nothing, so ignored. The unpacker loads 8 bytes at +3
/// and keeps the low 4 (the size); nothing else reads a stream's header. Zero in the core and gameplay streams, stale packer bytes
/// in SCENE, comic and HUD bank streams.
pub(crate) fn lz_header(t: &mut Tracker, n: &mut Node) {
    let l = t.ignored_line(
        "LZ streams",
        "LZ header bytes 7-15",
        "read by nothing: func_L01_00433EA0 (every program's copy) reads bytes 0-6, byte 7 is the dropped high half of its 8-byte load at +3, and no other code reads a stream's header",
        Space::Disc,
    );
    let h = t.child(n, l, 7, 9);
    t.close(h);
}

pub(crate) fn lz_block(
    t: &mut Tracker,
    parent: &mut Node,
    line: usize,
    b: &[u8],
    at: u64,
    size: u64,
    group: &str,
    inside: impl FnOnce(&mut Tracker, &mut Node, &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    lz_data(t, parent, line, b, at, size, group, inside).map(|_| ())
}

/// `lz_block` for a block whose decoder returns data: returns it, or None when the stream does not unpack (the stream's node
/// then holds the error).
pub(crate) fn lz_data<T>(
    t: &mut Tracker,
    parent: &mut Node,
    line: usize,
    b: &[u8],
    at: u64,
    size: u64,
    group: &str,
    inside: impl FnOnce(&mut Tracker, &mut Node, &[u8]) -> Result<T, String>,
) -> Result<Option<T>, String> {
    let mut out = None;
    let mut n = t.child(parent, line, at, size);
    match lz::unpack(b) {
        Ok(s) => {
            n.mark(0, 7);
            lz_header(t, &mut n);
            // The bytes after the stream are zero; the chunk tails a switch token skips hold the packer's fill, 0xEE.
            for (x, y, fill) in [(s.size, b.len(), 0u8)].into_iter().chain(s.skipped.iter().map(|&(x, y)| (x, y, 0xee))) {
                if b[x..y].iter().all(|&v| v == fill) {
                    n.mark(x as u64, (y - x) as u64);
                }
            }
            let mut covered = vec![(0x10usize, s.size)];
            // The token bytes: the stream minus the skipped tails.
            for (x, y) in &s.skipped {
                covered = covered.into_iter().flat_map(|(a, c)| if *x >= c || *y <= a { vec![(a, c)] } else { vec![(a, *x), (*y, c)] }).filter(|r| r.0 < r.1).collect();
            }
            for (x, y) in covered {
                n.mark(x as u64, (y - x) as u64);
            }
            let mut root = t.unpacked_root(&mut n, group, s.data.len() as u64);
            out = Some(inside(t, &mut root, &s.data)?);
            t.close(root);
        }
        Err(e) => n.fail(e),
    }
    t.close(n);
    Ok(out)
}

