//! levelglobals DISC.iso [rac3-decomp/config/us]: prints `uyafmt::levelcode::globals` for the 34 story levels and checks it.
//!
//! The checks: level 1 equals Veldin's addresses (the tables in native::globals); in every level each address lies inside the
//! level's .bss (levels.tsv's section), the overlay's own records agree (a record of the .bss's address); GameCamera equals the
//! address rac3-decomp's built.tsv gives for level N; every function the instructions were read from is where the level
//! catalogue (functions.tsv) puts it; the shrub group and the tie group keep Veldin's order and spacing. The rac3-decomp files
//! are only a cross-check: the addresses come from the overlays.

use std::collections::BTreeMap;
use uyafmt::disc::Disc;
use uyafmt::levelcode::{self, LevelGlobals, STORY_LEVELS};
use uyafmt::toc;

fn fields(g: &LevelGlobals) -> Vec<(&'static str, u32)> {
    vec![
        ("camera", g.camera),
        ("tangents", g.tangents),
        ("zones", g.zones),
        ("sky", g.sky),
        ("shr.count", g.shrubs.count),
        ("shr.inst", g.shrubs.instances),
        ("shr.mat", g.shrubs.matrices),
        ("shr.col", g.shrubs.colours),
        ("shr.limit", g.shrubs.limit),
        ("tie.count", g.ties.count),
        ("tie.inst", g.ties.instances),
        ("tie.mat", g.ties.matrices),
        ("tie.col", g.ties.colours),
        ("tie.limit", g.ties.limit),
        ("mobies", g.mobies),
        ("mobyend", g.moby_end),
        ("lights", g.light_sets),
        ("chunk", g.level_chunk),
        ("viewcam", g.view_camera),
        ("tie.list", g.ties.list),
        ("tie.lbytes", g.ties.list_bytes),
        ("tie.recs", g.ties.records),
        ("mobyjobs", g.moby_jobs),
        ("core", g.core),
        ("armorent", g.armor_entry),
        ("armorbuf", g.armor_buffer),
        ("wrench", g.wrench_block),
        ("clankskin", g.clank_texture.skin),
        ("shipskin", g.ship_texture.skin),
        ("hudheader", g.hud_header),
        ("continue", g.continue_point),
    ]
}

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s.trim_start_matches("0x"), 16).expect("hex")
}

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let disc = a.get(1).ok_or("usage: levelglobals DISC.iso [CONFIG_DIR]")?;
    let cfg = a.get(2).cloned().unwrap_or_else(|| "extern/rac3-decomp/config/us".to_string());
    let mut d = Disc::open(disc)?;
    let tc = toc::read(&mut d)?;
    let reference = levelcode::overlay(&mut d, &tc, 1)?;

    // levels.tsv: level -> .bss (address, size).
    let mut bss: BTreeMap<usize, (u32, u32)> = BTreeMap::new();
    for l in std::fs::read_to_string(format!("{cfg}/levels/levels.tsv")).map_err(|e| e.to_string())?.lines() {
        if l.starts_with('#') {
            continue;
        }
        let p: Vec<&str> = l.split('\t').collect();
        let level: usize = p[0].parse().map_err(|_| format!("levels.tsv: {l}"))?;
        let s = p[6].split(',').find(|s| s.starts_with(".bss:")).ok_or(format!("levels.tsv level {level}: no .bss"))?;
        let f: Vec<&str> = s.split(':').collect();
        bss.insert(level, (hex(f[1]), hex(f[2])));
    }
    // built.tsv: level_NN: D_L01_001E2A40 is H here, C for func_...  (C is GameCamera's address).
    let mut cam: BTreeMap<usize, (u32, u32)> = BTreeMap::new();
    for l in std::fs::read_to_string(format!("{cfg}/built.tsv")).map_err(|e| e.to_string())?.lines() {
        if !l.contains("D_L01_001E2A40 is") {
            continue;
        }
        for part in l.split(';') {
            let part = part.rsplit('\t').next().unwrap_or(part);
            let Some(rest) = part.strip_prefix("level_") else { continue };
            let (n, rest) = rest.split_once(": D_L01_001E2A40 is ").ok_or(format!("built.tsv: {part}"))?;
            let (h, rest) = rest.split_once(" here, ").ok_or(format!("built.tsv: {part}"))?;
            let c = rest.split_once(" for ").ok_or(format!("built.tsv: {part}"))?.0;
            cam.insert(n.parse().unwrap(), (hex(h), hex(c)));
        }
    }
    // functions.tsv: place "01:ADDR" -> the row's places; and every function start in every level -> its row's name.
    let funcs = std::fs::read_to_string(format!("{cfg}/levels/functions.tsv")).map_err(|e| e.to_string())?;
    let mut places: BTreeMap<u32, BTreeMap<usize, u32>> = BTreeMap::new();
    let mut starts: BTreeMap<(usize, u32), String> = BTreeMap::new();
    for l in funcs.lines() {
        if l.starts_with('#') {
            continue;
        }
        let Some(pl) = l.split('\t').nth(6) else { continue };
        let row: BTreeMap<usize, u32> = pl.split(',').filter_map(|s| s.split_once(':')).map(|(a, b)| (a.parse().unwrap(), hex(b))).collect();
        let name = l.split('\t').next().unwrap_or_default();
        for (&lv, &a) in &row {
            starts.insert((lv, a), name.to_string());
        }
        if let Some(&first) = row.get(&1) {
            places.insert(first, row);
        }
    }

    let mut all: Vec<LevelGlobals> = Vec::new();
    let mut fails = Vec::new();
    println!("lv  gp       {}", ["camera", "tangents", "zones", "sky", "shr.count", "shr.inst", "shr.mat", "shr.col", "shr.limit", "tie.count", "tie.inst", "tie.mat", "tie.col", "tie.limit", "mobies", "mobyend", "lights", "chunk", "viewcam", "tie.list", "tie.lbytes", "tie.recs", "mobyjobs", "core", "armorent", "armorbuf", "wrench", "clankskin", "shipskin", "hudheader", "continue"].iter().map(|n| format!("{n:>9}")).collect::<Vec<_>>().join(" "));
    for &lv in STORY_LEVELS.iter() {
        let ov = levelcode::overlay(&mut d, &tc, lv)?;
        let g = match levelcode::from_overlay(&reference, &ov) {
            Ok(g) => g,
            Err(e) => {
                fails.push(format!("L{lv:02}: {e}"));
                continue;
            }
        };
        println!("{lv:02}  {:06x}   {}", g.gp, fields(&g).iter().map(|f| format!("{:>9x}", f.1)).collect::<Vec<_>>().join(" "));
        // .bss, records
        let (b0, bn) = bss[&lv];
        for (name, v) in fields(&g) {
            // The hero armor's two variables are the main program's (below the overlays at 0x1D5680), the same in every level.
            if matches!(name, "armorent" | "armorbuf" | "clankskin" | "shipskin") {
                if v >= 0x1D_5680 || ov.record_of(v).is_some() {
                    fails.push(format!("L{lv:02} {name} {v:#x} is not a main program variable"));
                }
                continue;
            }
            if !(b0..b0 + bn).contains(&v) {
                fails.push(format!("L{lv:02} {name} {v:#x} is outside .bss {b0:#x}+{bn:#x}"));
            }
            match ov.record_of(v) {
                Some(r) if r.address == b0 => {}
                Some(r) => fails.push(format!("L{lv:02} {name} {v:#x} lies in the overlay record at {:#x} (kind {:#x}), not the .bss record at {b0:#x}", r.address, r.kind)),
                None => fails.push(format!("L{lv:02} {name} {v:#x} lies in no overlay record")),
            }
        }
        // built.tsv
        match cam.get(&lv) {
            Some(&(h, c)) => {
                if c != g.camera || h + 0x80 != c {
                    fails.push(format!("L{lv:02} camera {:#x}, built.tsv: {h:#x} here, {c:#x}", g.camera));
                }
            }
            None => fails.push(format!("L{lv:02} built.tsv has no GameCamera entry")),
        }
        // The class table's addresses run up to its ids (`uya` range.rs reads the level's classes that way).
        let (ct, ci) = (g.gadget.classes, g.gadget.class_ids);
        if ci <= ct || (ci - ct) % 4 != 0 || (ci - ct) / 4 > 4096 {
            fails.push(format!("L{lv:02} class table {ct:#x} does not run up to its ids at {ci:#x}"));
        } else if lv <= 3 {
            println!("L{lv:02} class table {ct:#x}, ids {ci:#x}: {} entries", (ci - ct) / 4);
        }
        // functions.tsv places
        for (name, l1, at) in levelcode::function_starts(&reference, &ov)? {
            match places.get(&l1).and_then(|r| r.get(&lv)) {
                Some(&p) if p == at => {}
                Some(&p) => fails.push(format!("L{lv:02} {name} found at {at:#x}, functions.tsv has {p:#x}")),
                // No place in the function's own row: the catalogue's split of that level may have made it a row of its own (level
                // 16's func_003A9B10 is func_L16_004DFE30, 468 bytes: its `jr $ra`'s delay slot made a 4-byte remnant, a place of
                // func_L02_004BB340's row), so a function the catalogue starts where it was found, by any name, places it.
                None => match starts.get(&(lv, at)) {
                    Some(other) => println!("L{lv:02} {name} found at {at:#x}: functions.tsv starts {other} there"),
                    None => fails.push(format!("L{lv:02} {name} (L01 {l1:#x}) has no functions.tsv place, and no function starts at {at:#x}")),
                },
            }
        }
        // order and spacing
        let (s, t) = (&g.shrubs, &g.ties);
        let shr = [s.instances - s.count, s.matrices - s.count, s.colours - s.count, s.limit - s.count];
        let tie = [t.instances.wrapping_sub(t.count), t.matrices.wrapping_sub(t.count), t.colours.wrapping_sub(t.count), t.limit.wrapping_sub(t.count)];
        if shr != [4, 0xC, 0x10, 0x14] {
            fails.push(format!("L{lv:02} shrub group offsets from the count {shr:x?}, Veldin has [4, c, 10, 14]"));
        }
        if tie != [0xFFFF_FFF4, 0xFFFF_FFFC, 4, 0x34] {
            fails.push(format!("L{lv:02} tie group offsets from the count {tie:x?}, Veldin has [fffffff4, fffffffc, 4, 34]"));
        }
        all.push(g);
    }
    // Veldin
    let Some(v) = all.iter().find(|g| g.level == 1) else {
        for f in &fails {
            println!("FAIL {f}");
        }
        return Err("level 1 failed".into());
    };
    let want: [(&str, u32, u32); 30] = [
        ("camera", v.camera, 0x1E_2AC0),
        ("tangents", v.tangents, 0x1E_76B0),
        ("zones", v.zones, 0x1E_A8C0),
        ("sky", v.sky, 0x1E_0E70),
        ("shr.count", v.shrubs.count, 0x1E_0E44),
        ("shr.inst", v.shrubs.instances, 0x1E_0E48),
        ("shr.mat", v.shrubs.matrices, 0x1E_0E50),
        ("shr.col", v.shrubs.colours, 0x1E_0E54),
        ("shr.limit", v.shrubs.limit, 0x1E_0E58),
        ("tie.count", v.ties.count, 0x1E_0F60),
        ("tie.inst", v.ties.instances, 0x1E_0F54),
        ("tie.mat", v.ties.matrices, 0x1E_0F5C),
        ("tie.col", v.ties.colours, 0x1E_0F64),
        ("tie.limit", v.ties.limit, 0x1E_0F94),
        ("mobies", v.mobies, 0x1E_009C),
        ("mobyend", v.moby_end, 0x1E_00A0),
        ("lights", v.light_sets, 0x1F_17C0),
        ("chunk", v.level_chunk, 0x1F_3FC0),
        ("viewcam", v.view_camera, 0x1E_2A60),
        ("tie.list", v.ties.list, 0x24_0880),
        ("tie.lbytes", v.ties.list_bytes, 0x1E_0F6C),
        ("tie.recs", v.ties.records, 0x1E_0F68),
        ("mobyjobs", v.moby_jobs, 0x1D_FBF0),
        ("core", v.core, 0x1E_A9D8),
        ("armorent", v.armor_entry, 0x1D_4B60),
        ("armorbuf", v.armor_buffer, 0x1D_4B64),
        ("wrench", v.wrench_block, 0x1E_AA34),
        ("clankskin", v.clank_texture.skin, 0x1D_4BA0),
        ("shipskin", v.ship_texture.skin, 0x16_C5B0),
        ("hudheader", v.hud_header, 0x1D_FD98),
    ];
    // Clank's image sectors at 0x162E78 are the armor WAD's (its header at 0x1EB0, the base sector at 0x162AF4) pair 112; the
    // ship's at 0x1629B8 are the header at 0x1280's pair 350.
    for (n, t, w) in [("clank", v.clank_texture, (0x1EB0, 112)), ("ship", v.ship_texture, (0x1280, 350))] {
        if (t.wad, t.pair) != w {
            fails.push(format!("L01 {n} texture: header {:#x} pair {}, Veldin {:#x} {}", t.wad, t.pair, w.0, w.1));
        }
    }
    for (n, got, w) in want {
        if got != w {
            fails.push(format!("L01 {n} {got:#x}, Veldin {w:#x}"));
        }
    }
    if v.gp != 0x1D_C8B0 {
        fails.push(format!("L01 gp {:#x}, Veldin 0x1dc8b0", v.gp));
    }
    // gp is one value; per field, how many distinct addresses over the levels.
    let gps: std::collections::BTreeSet<u32> = all.iter().map(|g| g.gp).collect();
    println!("\ngp values over {} levels: {:x?}", all.len(), gps);
    for (k, (n, _, _)) in want.iter().enumerate() {
        let vals: std::collections::BTreeSet<u32> = all.iter().map(|g| fields(g)[k].1).collect();
        println!("{n:>9}: {} distinct addresses over {} levels", vals.len(), all.len());
    }
    println!("\n{} levels, {} failures", all.len(), fails.len());
    for f in &fails {
        println!("FAIL {f}");
    }
    if fails.is_empty() { Ok(()) } else { Err(format!("{} checks failed", fails.len())) }
}
