//! The ties drawn farther and finer than the PS2 drew them, by the game's own cull (`play`): the player, 14 November 2026:
//! "fixing some light texture pop in that still appears ... It probably also happened in the game, i just want it fixed".
//!
//! On level 28 the plant pods changed model as Ratchet walked up to them and small ties came into sight a few steps away. The
//! game's tie cull (func_L01_004AFA28) picks each tie's LOD by its view depth against its class's three LOD distances (a header's
//! +0x10, +0x14 and +0x18: 16, 32 and 64 units for the pods, 12, 25 and 40 for some) and lists it only within 16 x the smaller of
//! its own distance byte (an instance's +0x1C: 8, 128 units, for level 28's small ones) and the level's limit (its tie limit in
//! units, `TieAddrs::limit`, over 16: 960 units, 60, on levels 1, 10 and 28). The cull reads every class's LOD distances as past
//! any depth it lists a tie at, so the game draws every tie at its most detailed LOD (and lights its colours for it, as it does
//! at a LOD change), and every instance's distance byte that is below the level's limit as the limit, so the limit alone ends
//! the distance; a byte at or above it (most of Veldin's and level 10's are 250) is the level's own, and so is a byte of 0, a tie
//! the level hides. The game builds its chain from that; the renderer draws the LOD the game drew (`objects::Ties::frame`). The
//! shrubs, which the renderer culls itself, go out to the level's shrub limit (`objects::Shrubs::visible`).
//!
//! Both keep to the room the game's frame packet buffer has (`uyart::tiebudget`; their words are each level's, `publish`): the
//! finest LOD out to a reach that follows the frames' packets, and the instances out to the limit while the frames have room.
//! Level 28's arrival with every tie at its finest LOD took 1,429 of the buffer's 1,536 KB at 16:9 and ran past it at FIELD OF
//! VIEW 110, which stopped the game.
//!
//! Every byte read as 255 instead drew the ties levels hide (Veldin's six, the water's and the sea's VU1 programs run every
//! frame for them) and took Veldin's game code from 12.5 to 14.2 ms a field and its render from 7.1 to 9.5 ms.
//!
//! The mobies change model by distance too: the game draws a class's near meshes, or its far ones when the view depth passes the
//! instance's near distance (its +0x62 byte times 1024; func_L01_0043EA78 at 0x43F118 to 0x43F168, `uyafmt::moby` "Which mesh
//! records"). Veldin's silver crates (class 0x16A3080 there: 13 near meshes, 10 far, near to 20 units) changed look as Ratchet
//! walked up to them (the player, 16 November 2026: "The silver environment boxes texture/lighting changes at a certain distance
//! and you can see it flip between the two walking forward and back"). While a level is in play, the cull reads each moby of a
//! class with far meshes and no joints, a prop, as having the largest near distance, 255 units, so the game draws its near
//! meshes wherever it draws it. The classes with joints are left: an enemy's near meshes are skinned with more vertices, and
//! every moby at 255 took Veldin's game code from 10.1 to 11.9 ms a field in its first fight, with more fields overrunning.
//!
//! None of it while an in-engine cutscene plays (the game's mode 2): the culls read the game's own values.
//!
//! Each of these is applied by the native cull as it reads the value (`uyart::range`); nothing is written into the game's
//! memory. Until 24 November 2026 the port wrote them there before each field, at the addresses the level's globals named, and
//! twice that broke the game: level 9's cutscene 0 crashed where the game overwrote a class the port had written (the hero body
//! class 0x7A24F0, in the scene's block), and on level 35 under load the tie globals still named an array in the buffer the next
//! WAD streams into, so the port's write raised 220 bytes of the stream the LZ unpacker was part way through, and it trapped
//! (R3-034). Here, before each field, the port says which ranges hold (`publish`).

use uyafmt::levelcode::LevelGlobals;

fn r32(ram: &[u8], a: usize) -> u32 {
    u32::from_le_bytes(ram[a..a + 4].try_into().expect("4 bytes"))
}

pub struct Range {
    /// Every story level's globals (`uyafmt::levelcode::all`), and the word of its memory map holding its second frame packet
    /// buffer's start (`uyafmt::scenecode::SceneGlobals::packet_b`; the first's is the word before).
    levels: Vec<LevelGlobals>,
    packet_b: std::collections::HashMap<u32, u32>,
}

impl Range {
    pub fn new(levels: Vec<LevelGlobals>, scenes: &[uyafmt::scenecode::SceneGlobals]) -> Range {
        Range { levels, packet_b: scenes.iter().map(|s| (s.level as u32, s.packet_b)).collect() }
    }

    /// Before a field: the ranges the culls apply in it (`uyart::range::set`). The ties' while the level's code is in RAM (a
    /// level change loads the next overlay over the last); the props' while the level is in play (`playing`, `Menus::in_play`:
    /// its setup has made its mobies). None in a cutscene (`cutscene`, the game's mode 2).
    pub fn publish(&self, ram: &[u8], playing: bool, cutscene: bool) -> Result<(), String> {
        let level = r32(ram, crate::LEVEL_CURRENT as usize);
        let g = self.levels.iter().find(|g| g.level as u32 == level).filter(|g| [r32(ram, g.anchor as usize), r32(ram, g.anchor as usize + 4)] == g.anchor_words);
        let (Some(g), false) = (g, cutscene) else {
            uyart::range::set(uyart::range::Ranges::default());
            return Ok(());
        };
        let t = &g.ties;
        let ties = Some(uyart::range::Ties { class_count: t.class_count, classes: t.classes, count: t.count, instances: t.instances, limit: t.limit });
        let props = if playing { props(ram, g, level)? } else { None };
        // The room the tie ranges keep to (`uyart::tiebudget`): the level's frame packet buffers.
        let b = *self.packet_b.get(&level).ok_or_else(|| format!("range: level {level} has no frame packet buffers' words"))?;
        let packets = Some(uyart::range::Packets { starts: [b - 4, b], level });
        uyart::range::set(uyart::range::Ranges { ties, props, packets });
        Ok(())
    }
}

/// A moby instance's size in the level's array.
const MOBY: usize = 0x100;

/// The level's props, drawn with their near meshes at every distance (the module doc): the classes in its class table (a word
/// per index up to the table of their ids, `GadgetAddrs::classes`) with far meshes (class +5) and no joints (class +8). The cull
/// applies it to each moby in use in the level's array (what the draw loop does not skip, func_L01_0043EA78: its +0x20 byte
/// not negative, 0x43EE90; its +0x34 halfword without 0x81, 0x43EEE8) whose +0x24 is one of them (`uyart::range::prop`): on
/// Florana a slot that passed the draw loop's tests held 0xC8004403 there for a field, and that is no prop. None until the
/// level's setup has stored its array's start and then its end (func_L01_004252F0 +0x133C, +0x1944; the game is already in
/// play while it runs: Florana by warp, field 1413, the start stored and the end still 0).
fn props(ram: &[u8], g: &LevelGlobals, level: u32) -> Result<Option<uyart::range::Props>, String> {
    let (start, end) = (r32(ram, g.mobies as usize) as usize, r32(ram, g.moby_end as usize) as usize);
    if end <= start {
        return Ok(None);
    }
    if end > ram.len() || (end - start) % MOBY != 0 {
        return Err(format!("range: level {level}'s moby array from {start:#x} to {end:#x} is no whole number of instances in RAM"));
    }
    let (table, ids) = (g.gadget.classes as usize, g.gadget.class_ids as usize);
    if ids <= table || (ids - table) % 4 != 0 || ids > ram.len() {
        return Err(format!("range: level {level}'s class table at {table:#x} does not end at its ids at {ids:#x}"));
    }
    let classes = (table..ids)
        .step_by(4)
        .map(|a| r32(ram, a) as usize)
        .filter(|&c| c != 0 && c + 0x10 <= ram.len() && ram[c + 5] != 0 && ram[c + 8] == 0)
        .map(|c| c as u32)
        .collect();
    Ok(Some(uyart::range::Props { mobies: g.mobies, moby_end: g.moby_end, classes }))
}
