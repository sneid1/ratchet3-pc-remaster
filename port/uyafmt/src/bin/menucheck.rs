//! menucheck DISC.iso [RAMDIR]: prints `uyafmt::menucode::all` for the 34 story levels and checks it.
//!
//! The checks: level 1 equals Veldin's addresses as found in Veldin's RAM (play window session, docs/HANDOVER.md "Play window and
//! controls" and the pause screens' records and cursors, found in Veldin's code); with RAMDIR, every RAM dump there (`uya frames --ram-dir`, `uya play` with UYA_RAM_AT) is read with the menu
//! globals of the level it is in: the text table pointer must lead to a table of that count whose entry for text 0x509 reads
//! "PROGRESSIVE SCAN", the save messages' word must be a message number (0 to 11), the mode word and the pause list's count small,
//! the pause screens' cursors in their grids, the vendor's struct its own (small phase, page and item count; the carousel's scale
//! 0, 1 or 1.0769).
//! A dump taken while a level's overlay is still loading (the level word already the next level's, the level's own code not yet
//! in RAM: its two anchor words, `MenuGlobals::anchor`, are not there) is named and not counted.

use uyafmt::menucode::{self, MenuGlobals};

fn r32(ram: &[u8], a: u32) -> Option<u32> {
    ram.get(a as usize..a as usize + 4).map(|b| u32::from_le_bytes(b.try_into().expect("4 bytes")))
}

/// The dump read with `g`: None when it agrees, else why not.
fn check(ram: &[u8], g: &MenuGlobals) -> Option<String> {
    let (ptr, n) = (r32(ram, g.text)?, r32(ram, g.text_count)?);
    // The level's data is loaded after its code: until then the text table is not set up.
    if ptr == 0 && n == 0 {
        return Some("loading".into());
    }
    if !(1000..10000).contains(&n) || ptr as usize + 16 * n as usize > ram.len() {
        return Some(format!("text table {ptr:#x}, {n} entries"));
    }
    let s = (0..n).find(|&i| r32(ram, ptr + 16 * i + 4) == Some(0x509)).and_then(|i| r32(ram, ptr + 16 * i)).map(|a| {
        let b = &ram[a as usize..(a as usize + 32).min(ram.len())];
        String::from_utf8_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]).into_owned()
    });
    if s.as_deref() != Some("PROGRESSIVE SCAN") {
        return Some(format!("text 0x509 is {s:?}"));
    }
    let (msg, mode, count) = (r32(ram, g.saves + 0x40)?, r32(ram, g.mode)?, r32(ram, g.pause_count)?);
    // The mode word is the game's state (0 playing, 1 paused, 6, 14, and in some levels' cutscenes up to the 190s).
    if msg > 11 || mode > 255 || count > 16 {
        return Some(format!("save message {msg}, mode {mode}, pause count {count}"));
    }
    // The pause screens: a record's state is 0, 1 or 2; the cursors are inside their grids; the weapons' column x is a share of the
    // picture.
    for (name, at) in ["MISSIONS", "WEAPONS", "GADGETS", "QUICK SELECT", "ITEMS", "SAVE"].iter().zip(g.screens) {
        let state = r32(ram, at)?;
        if state > 2 {
            return Some(format!("{name} record {at:#x}: state {state}"));
        }
    }
    let (col, row, x0, dx) = (r32(ram, g.weapons[0])?, r32(ram, g.weapons[1])?, f32::from_bits(r32(ram, g.weapons[2])?), f32::from_bits(r32(ram, g.weapons[3])?));
    if col >= 10 || row >= 2 || !(0.0..1.0).contains(&x0) || !(0.0..0.2).contains(&dx) {
        return Some(format!("WEAPONS column {col}, row {row}, x {x0}, step {dx}"));
    }
    let fl = |a: u32| r32(ram, a).map(f32::from_bits);
    let (gx, gy, gdx, gdy) = (fl(g.gadgets[2])?, fl(g.gadgets[3])?, fl(g.gadgets[4])?, fl(g.gadgets[5])?);
    if !(0.0..1.0).contains(&gx) || !(0.0..1.0).contains(&gy) || !(0.0..0.3).contains(&gdx) || !(0.0..0.3).contains(&gdy) {
        return Some(format!("GADGETS layout x {gx}, y {gy}, step {gdx}, {gdy}"));
    }
    let (mrow, mscroll, mcount) = (r32(ram, g.missions[0])?, r32(ram, g.missions[1])?, r32(ram, g.missions[2])?);
    if mrow > 2 || mscroll > 64 || mcount > 64 {
        return Some(format!("MISSIONS row {mrow}, scroll {mscroll}, count {mcount}"));
    }
    let ring = r32(ram, g.quick_ring)?;
    if ring >= 8 {
        return Some(format!("QUICK SELECT ring slot {ring}"));
    }
    let (gc, gr, item, slot, state) = (r32(ram, g.gadgets[0])?, r32(ram, g.gadgets[1])?, r32(ram, g.items)?, r32(ram, g.save[1])?, r32(ram, g.save[0])?);
    if gc >= 5 || gr >= 2 || item >= 10 || slot >= 4 || state > 6 {
        return Some(format!("GADGETS {gc}, {gr}, ITEMS {item}, save slot {slot}, save state {state}"));
    }
    let (sb, sp) = (fl(g.save[2])?, fl(g.save[3])?);
    if !(0.0..0.5).contains(&sb) || !(0.1..0.3).contains(&sp) {
        return Some(format!("save slots at {sb}, step {sp}"));
    }
    // The vendor's struct: its phase, page, highlighted item and item count small (all 0 before the vendor first opens), the
    // carousel's scale 0 (never opened), 1 or 1.0769.
    let (phase, page, cursor, items, scale) = (r32(ram, g.vendor)?, r32(ram, g.vendor + 0x50)?, r32(ram, g.vendor + 0x80)?, r32(ram, g.vendor + 0x740)?, fl(g.vendor_scale)?);
    if phase > 3 || page > 5 || items > 64 || cursor > items.max(1) || ![0.0, 1.0].contains(&scale) && (scale - 1.0769).abs() > 0.001 {
        return Some(format!("vendor phase {phase}, page {page}, item {cursor} of {items}, scale {scale}"));
    }
    None
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let all = match menucode::all(&a[0]) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("menucheck: {e}");
            std::process::exit(1);
        }
    };
    for g in &all {
        println!("L{:02}: mode {:#x}, pause table {:#x} count {:#x} index {:#x}, panels {:#x}, text {:#x} count {:#x}, saves {:#x}", g.level, g.mode, g.pause_table, g.pause_count, g.pause_index, g.panels, g.text, g.text_count, g.saves);
        println!("     screens {:x?}, missions {:x?}, weapons {:x?}, gadgets {:x?}, quick {:x?}, items {:#x}, save {:x?}, vendor {:#x} scale {:#x}, ui {:x?}, comics {:x?}, rows {:x?}, ui sound {:#x}", g.screens, g.missions, g.weapons, g.gadgets, g.quick, g.items, g.save, g.vendor, g.vendor_scale, g.ui, g.comics, g.rows, g.ui_sound);
    }
    let veldin = MenuGlobals { level: 1, mode: 0x1D_7DC0, current: 0x1D_6694, pause_table: 0x1D_8310, pause_count: 0x1D_831C, pause_index: 0x1D_8318, panels: 0x23_6868, text: 0x1D_FD40, text_count: 0x2E_5F9C, saves: 0x1E_A740, anchor: all[0].anchor, anchor_words: all[0].anchor_words, screens: [0x1E_09B8, 0x1E_0830, 0x1E_08B0, 0x1E_08D0, 0x1E_0960, 0x1E_0750], pause_record: 0x1E_0690, missions: [0x1D_8CB0, 0x1D_8CAC, 0x1D_8CA8], weapons: [0x1D_858C, 0x1D_8590, 0x1D_85DC, 0x1D_85E0, 0x1D_85E4, 0x1D_85E8, 0x1D_8588], gadgets: [0x1D_8718, 0x1D_871C, 0x1D_86F8, 0x1D_86FC, 0x1D_8700, 0x1D_8704], quick: [0x1D_8868, 0x1D_886C, 0x1D_8878], quick_ring: 0x1D_8874, items: 0x1D_8C10, save: [0x1D_83C0, 0x1D_8434, 0x1D_8418, 0x1D_83E8], vendor: 0x24_4740, vendor_scale: 0x1E_1050, ui: [0x1E_1660, 0x1E_24C8], comics: [0x1E_0BD0, 0x1D_9058, 0x1D_9170], rows: [0x1E_0780, 0x1D_8510, 0x1D_84E0, 0x1D_8534], ui_sound: 0x4A_7368, controls_mode: 0x1E_0D50 };
    let mut bad = 0;
    if all[0] != veldin {
        println!("level 1 differs from Veldin's RAM: {:?}", all[0]);
        bad += 1;
    }
    if let Some(dir) = a.get(1) {
        let mut paths: Vec<_> = std::fs::read_dir(dir).expect("RAMDIR").filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("ram") && n.to_string_lossy().ends_with(".bin"))).collect();
        paths.sort();
        let (mut ok, mut loading) = (0, 0);
        for p in paths {
            let ram = std::fs::read(&p).expect("dump");
            let level = r32(&ram, 0x1D_545C).expect("level word");
            let Some(g) = all.iter().find(|g| g.level as u32 == level) else { continue };
            // The level's own code not yet in RAM: another level's overlay (or none) holds the addresses.
            if r32(&ram, g.anchor) != Some(g.anchor_words[0]) || r32(&ram, g.anchor + 4) != Some(g.anchor_words[1]) {
                println!("{}: level {level}, its overlay is not in RAM yet", p.display());
                loading += 1;
                continue;
            }
            match check(&ram, g) {
                None => ok += 1,
                Some(why) if why == "loading" => {
                    println!("{}: level {level}, its data is not loaded yet", p.display());
                    loading += 1;
                }
                Some(why) => {
                    println!("{}: level {level}: {why}", p.display());
                    bad += 1;
                }
            }
        }
        println!("{ok} dumps agree, {loading} taken while a level loads, {bad} disagree");
    }
    if bad > 0 {
        std::process::exit(1);
    }
}
