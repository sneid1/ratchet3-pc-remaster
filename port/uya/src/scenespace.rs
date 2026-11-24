//! An in-engine cutscene's SCENE block, unpacked where the frame packets cannot reach it. The player, 21 November 2026, on level
//! 9's cutscene 0 from their save: "Crashed instantly again", and "Happens at a specific point, at the end of the scene".
//!
//! The game carves the block's 0x90000-byte region from the top of its second frame packet buffer and lowers the bytes each
//! buffer's packets may fill to what is left (`uyafmt::scenecode`, "The region's place": on level 9 the buffer starts at
//! 0x6B1000, the block at 0x7A24A0, 0xF14A0 bytes in). Nothing in the game holds a frame's packets to that line: the moby
//! update's effect sprites (frontbin func_003CD830's level copy, level 9's 0x41F310) take their GIF packets with the bump of
//! func_003CDE70 (level 9's 0x41F950), which does not look at it. The PS2's 4:3 frames stayed under it. The port's wider view
//! (WIDESCREEN) draws more of the level at the sides, and its frames ran past the line. From the player's save, at the game's
//! pace, the hero body's class at 0x7A24F0 (the block's first) was written over with packet words about 570 fields in, and the
//! game then followed a pointer from it out of RAM. With WIDESCREEN off the second buffer's packets came within 12 to 42 KB of
//! the block, and three runs of three played the cutscene through.
//!
//! The port gives the block its own room instead: RAM from `ROOM` to `ROOM + REGION`, which the PS2's kernel held and the port's
//! does not use (its interrupt stack is above it, `uyart::kernel::INT_STACK`). Two of each story level's functions have the
//! port's in their place in the table (`uyart::hook`; their addresses from `uyafmt::scenecode`):
//!
//! - The unpacker, func_L01_004294D0, the one call that writes the block (`unpack`). Before the game's own runs, the port sets
//!   the region's start, `[S + 0x58]`, to `ROOM`, so the block is unpacked and its classes registered there. After it, the port
//!   moves the map's start of the second buffer (`SceneGlobals::packet_b`) by as much as it moved the region. The game then works
//!   out the room its packets keep from the two (`[S + 0x58] - [map + 0x10]`, func_L01_004B5C78 +0x3C0), and the room comes out
//!   as it would have been.
//! - The cutscene's start, func_L01_004B5C78, which calls the loader, the unpacker and then that sum (`start`). When the game's
//!   own returns, the port puts the map's word back.
//!
//! Neither move can come earlier or end sooner. The loader before the unpacker sets the frame buffers from the map:
//! func_L01_00429450 calls frontbin func_00385B60's level copy (level 9's 0x41C368), which clears the screen for two frames and
//! calls func_003A3BB8's (0x4DBB08), the frame buffers' start. The port's first try moved the word while the game waited for the
//! disc, before the loader. The loader then built a frame at the moved address, and the game wrote outside RAM two fields later.
//! Between the unpack and the sum, func_L01_004B5C78 waits for the disc again (+0x330, the streams' read) across fields. The
//! second try put the word back at the next field, before the sum. The room came out below zero, and five fields later the game
//! read its frame memory outside RAM. From the unpacker's return to func_L01_004B5C78's, level 9's code reads no word of the map
//! and starts no frame (every function it calls on the way, and theirs).

use uyafmt::scenecode::SceneGlobals;
use uyart::Cpu;

/// The block's room: past the reach of a null pointer's offsets.
pub const ROOM: u32 = 0x1_0000;
/// The bytes the unpacker fills: it moves the unpacked block to the end of a 0x8E000-byte region (func_L01_004294D0 +0x48).
pub const REGION: u32 = 0x8_E000;

/// Every story level's scene variables, for the stand-ins.
static SCENES: std::sync::OnceLock<Vec<SceneGlobals>> = std::sync::OnceLock::new();

/// What the unpacker's stand-in did: the map word it moved and the word's own value, the record, the game's region and where
/// the block went; and whether the start's stand-in has put the word back.
struct Moved {
    packet_b: u32,
    b: u32,
    record: u32,
    region: u32,
    block: u32,
    back: bool,
}

static MOVED: std::sync::Mutex<Option<Moved>> = std::sync::Mutex::new(None);

/// Puts the port's stand-ins in the table for every story level's unpacker and cutscene start (`uyart::hook`), before any
/// level's code is in RAM.
pub fn install(scenes: &[SceneGlobals]) {
    let scenes = SCENES.get_or_init(|| scenes.to_vec());
    for g in scenes {
        let program = format!("l{:02}", g.level);
        uyart::hook(&program, g.unpack, unpack);
        uyart::hook(&program, g.start, start);
    }
}

/// The scene variables of the level whose code runs.
fn running(c: &mut Cpu) -> SceneGlobals {
    let level = c.rd32(crate::LEVEL_CURRENT);
    let g = SCENES.get().expect("scenespace::install ran").iter().find(|g| g.level as u32 == level).copied();
    g.unwrap_or_else(|| panic!("scenespace: a cutscene's code ran on level {level}, which is not a story level"))
}

/// The unpacker's stand-in: the game's own unpacker, with the region at `ROOM` (the module doc).
extern "C-unwind" fn unpack(c: &mut Cpu, l: u32) {
    let g = running(c);
    let (region, b) = (c.rd32(g.scene_base), c.rd32(g.packet_b));
    c.wr32(g.scene_base, ROOM);
    uyart::own(g.unpack)(c, l);
    let block = c.rd32(g.scene_base);
    if !(ROOM..ROOM + REGION).contains(&block) {
        panic!("scenespace: level {}'s SCENE block was unpacked at {block:#x}, outside its room from {ROOM:#x}", g.level);
    }
    c.wr32(g.packet_b, b.wrapping_add(ROOM).wrapping_sub(region));
    *MOVED.lock().expect("moved") = Some(Moved { packet_b: g.packet_b, b, record: c.rd32(g.scene_index), region, block, back: false });
}

/// The cutscene start's stand-in: the game's own, then the map's word put back if the unpacker's stand-in moved it.
extern "C-unwind" fn start(c: &mut Cpu, l: u32) {
    let g = running(c);
    uyart::own(g.start)(c, l);
    if let Some(m) = MOVED.lock().expect("moved").as_mut().filter(|m| !m.back) {
        c.wr32(m.packet_b, m.b);
        m.back = true;
    }
}

/// Before a field: what the stand-ins did by now, for the log, once the map's word is back.
pub fn before() -> Option<String> {
    let mut moved = MOVED.lock().expect("moved");
    if !moved.as_ref()?.back {
        return None;
    }
    let m = moved.take()?;
    Some(format!("SCENE record {} unpacked at {:#x} (the game's region {:#x})", m.record, m.block, m.region))
}
