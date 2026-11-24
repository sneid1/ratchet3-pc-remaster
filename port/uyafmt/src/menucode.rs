//! The addresses of a level's menu globals (the pause menu, its list panels, the text table, the save messages), read from the
//! level's own code as `levelcode` reads the level's world globals: each level overlay links its own copy of the menu code, so
//! the variables it owns sit at a different address in each level (Veldin's save messages at 0x1EA740, level 5's at 0x1EA400).
//! A function is found in a level by its code with the linker's immediates taken out (`levelcode::locate_function`), and the
//! instruction at the same offset from its start gives the level's address.
//!
//! Functions (rac3-decomp's level catalogue, extern/rac3-decomp/config/us/levels/functions.tsv: each the same function in all 34
//! story levels; Veldin's addresses and the instructions read):
//! - func_L01_00456C70: `lui` +0x160, `lw` +0x164: the game's mode word (1 while paused). The function is the game's mode
//!   switch: `lui` +0x18, `lw` +0x1C read the current mode (0x1D6694 on Veldin and level 7): its first switch runs the leaving
//!   mode's close, its second the requested mode's open (the request at the next word, -2 for none). Mode 3 is the pause and
//!   every screen of its family a level opens from the world (the battle missions on levels 12 and 18, the galactic map, the
//!   ship's QWARK COMICS: func_L03_00396020 requests mode 3 with screen 15), mode 5 the vendor (its open func_L01_004B7948
//!   and close in this switch), 4 the quick-select ring held in play, 0 play.
//! - func_L01_0047BCC0 (opens the pause list): `sw $v0, gp` +0x88 the list's table pointer, `sw $v1, gp` +0xF0 its item count,
//!   `lw $v0, gp` +0xFC the highlighted item.
//! - func_003B46F0 (a list panel's address: the base plus 0xB4 per panel): `lui $s3` +0x10, `addiu $s1, $s3` +0x2C. Its twin
//!   func_003AEDF8 is the same code on other panels, so it is found with the function after it (func_003B4778).
//! - func_0037DF28 (the text lookup): `lui $v1` +0x1C, `lw $v1` +0x20 the table pointer; `lui $v0` +0x0, `addiu $v0` +0xC,
//!   `lw $v1, 0x2C($v0)` +0x10 the entry count.
//! - func_003DBEC8 (the save messages' box): `lui $v1` +0x4, `addiu $s1, $v1` +0x14: the struct whose +0x40 is the message
//!   and +0x5C its frame count.
//! - func_0039FF28 (the menus' sound, play_ui_sound(id, 0, 0); Veldin's 0x4A7368, the main program's 0x3A5608 for the front
//!   end): the screens call it with 3 when a cursor steps (Up, Down, Left, Right on a list, a grid, the ring) and 4 for a choice
//!   (Cross, Triangle, an option's value changed with Left, Right or Cross). It only fills a sound slot (no system call below
//!   it), which the sound code plays later.
//! - func_L01_0049D7F0 (CONTROLS / CAMERA's ROTATION SPEED knob): `lui $v1` +0x0, `lw $v1` +0x4 the camera mode the screen sets
//!   (0 to 2; Veldin's 0x1E0D50), whose rotation speed is the word at 0x1439C4 + 0x10 x the mode.
//!
//! The option lists' sliders (a pause's OPTIONS has SAVE, LOAD, CONTROLS / CAMERA, AUDIO / VISUAL and QUIT GAME, Cross only;
//! the values are in those two sub-screens, both on list panel 0). AUDIO / VISUAL's rows are its table (Veldin 0x2EB078, 12 bytes
//! a row: text id, callback, index; the same eight in every level): SOUND, SUBTITLES, MUSIC VOLUME (text 0x234), EFFECTS VOLUME
//! (0x233), 16:9, PROGRESSIVE SCAN, HELPDESK TEXT, HELPDESK VOICE. The two volumes are sliders: the words 0x14395C and 0x143960,
//! 0 to 0x400, Left takes 0x1E off and Right adds it (clamped), Cross changes nothing; every other row flips on Cross, Left or
//! Right. CONTROLS / CAMERA has six rows, the fourth ROTATION SPEED (text 0x23C) a slider: 0 to 100, 2 a press. The game takes a
//! press on the field it comes (held & ~previous), and repeats only the d-pad held 20 fields. A slider's bar runs from the label
//! column's x plus its width (panel +0x20 + +0x40) to the value column's x (+0x24), its knob at that left end + 0.008 + (the bar's
//! length - 0.008) x value / 0x400 (func_L01_0048D720; ROTATION SPEED: + 0.007 and - 0.012, / 100, func_L01_0049D7F0). The game
//! applies a volume from its word every field, but its presses also click and clamp, so the port sets a slider with them.
//!
//! - The pause list's screen (the pause family's screen 1; its getter func_L01_0047B380 sets up its record as the screens'
//!   getters below do, with the update routine func_L01_0047AF70): its record (Veldin's gp + 0x3DE0, 0x1E0690) has +0x58 set
//!   while the pause list itself takes the input and clear while OPTIONS or one of OPTIONS' lists does; the screen id the
//!   family's dispatcher (func_L01_0047AAD0) reads stays 1 through all of them. A list panel keeps its active byte and its rows
//!   after its screen closes (OPTIONS' rows are still panel 0's, byte for byte, when the pause list is back), so a panel is on
//!   the screen only while the pause list does not take the input.
//!
//! The pause's screens. Each is a record {+0 state (1 while the screen is showing), +4.. five routines (+0xC the update routine),
//! +0x18 a flag}, set up by a small function that is one code for every screen (the getter: `lui`s and `addiu`s of its five
//! routines, then `addiu $a0, $gp, record`; its first function after another getter, so it cannot be found by its code). The
//! getter is found by the update routine it hands over: the `lui $a3` / `addiu $a3` pair that forms the update routine's
//! address, and eight bytes on the `addiu $a0, $gp, off`: the record is gp + off. The screen's update routine holds the
//! cursor in `$gp`-relative words, each found at its offset in the routine (Veldin's addresses; every level's are its own):
//! - MISSIONS (func_L01_0048B198, the list's Up and Down): the highlighted row of the three shown `lw` +0x88 (gp-0x3C00), the
//!   scroll `lw` +0xA4 (gp-0x3C04) and the number of missions `lw` +0xC4 (gp-0x3C08).
//! - WEAPONS (func_L01_00482B78): column `lw` +0x104 (gp-0x4324, 0 to 9), row +0xC4 (gp-0x4320, 0 to 1), the x of column 0
//!   `lwc1` +0x2AC (gp-0x42D4) and the step per column `lwc1` +0x29C (gp-0x42D0), the y of row 0 `lwc1` +0x2CC (gp-0x42CC) and
//!   of row 1 +0x2C0 (gp-0x42C8), shares of the picture (the cell at (column, row) is placed at x0 + dx * column, y of its row,
//!   by the loop at +0x294 to +0x2E0), and the byte that tells the screen the highlight moved `sb` +0xCC (gp-0x4328).
//! - GADGETS (func_L01_004840D0): column `lw` +0xBC (gp-0x4198, 0 to 4), row +0xC8 (gp-0x4194, 0 to 1). Its cursor is placed by
//!   func_L01_00483D98 at x0 + dx * column (+ dx / 2 on row 1), y0 + dy * row: x0 `lwc1` +0x40 (gp-0x41B8), y0 +0x54
//!   (gp-0x41B4), dx +0x14 (gp-0x41B0), dy +0x34 (gp-0x41AC).
//! - QUICK SELECT (func_L01_004864A8): the three words +0x34, +0x44, +0x50 (gp-0x4048, -0x4044, -0x4038).
//! - The QUICK SELECT ring (func_L01_00485D70, the screen's buttons: L1 and R1 step the slot, Square switches the ring): the highlighted
//!   slot `lw` +0x2C (gp-0x403C, 0 to 7). The ring turns to bring the slot to the top (func_005129D8 on element 0x190001).
//! - ITEMS (func_L01_00489D10): the index `lw` +0x2C (gp-0x3CA0, 0 to 9: two rows of five, row = index / 5).
//! - The save slots (the SAVE and LOAD screen's update routine is func_003B27A8, the slot's cursor is moved by func_003B23F8; both
//!   are in all 34 levels): the screen's state `lw` +0x1C of func_003B27A8 (gp-0x44F0: 0 while a slot is to be chosen), the
//!   slot `lw` +0x28 of func_003B23F8 (gp-0x447C, 0 to 3), and the cursor's place, func_003B23F8's end: y = gp-0x44C4 +
//!   gp-0x4498 + slot * gp-0x44C8, `lwc1` +0x208, +0x1FC and +0x200 (shares of the picture; the slot's row is centred on gp-0x4498
//!   + slot * gp-0x44C8, the cursor is drawn gp-0x44C4 lower).
//!
//! The vendor (the game's current mode 5). func_L01_004B7948 opens it and clears its struct (0x790 bytes, `lui $v0` +0x4,
//! `addiu $v0` +0x1C): +0 its phase (1 while it takes input), +0x50 the page (0 weapons, 2 ship, 3 armor, 5 SKINS), +0x6C the
//! carousel's scroll (GS pixels), +0x80 the highlighted item, +0x84 the sub-state (0 browsing, 1 and 2 the Purchase? box),
//! +0x144 the items' records (20 bytes each), +0x740 the number of items. On pages 0, 2 and 3 the items are a carousel:
//! func_L01_004B9A80 steps to the next item on Right while the scroll is under 57 (the scroll + 56) and to the one before on
//! Left while it is over -57 (the scroll - 56), and Cross buys the highlighted item; func_L01_004BB1B8 draws it and brings the
//! scroll back to 0 by 4 a field. It draws the items from 5 before the highlighted one to 6 after it: item k after the
//! highlighted one at GS x 238 + 56 k + scroll (the loop at +0x21C to +0x730, x +0x2C8), 38 pixels wide, at
//! y = round(308 s) + 5 (1 when the item record's first byte is set) + 4 while the word 0x143950 is set, round(38 s) high,
//! s the float `lui` +0x120, `lwc1` +0x124 (0x1E1050 on Veldin, 1 or 1.0769); faded by 0.75 of alpha a pixel away from x 256
//! (alpha 128 there), so items 3 or more away are barely seen or not at all. Page 5 (SKINS) is list panel 0.
//!
//! The UI elements. The pause family's newer screens are made of elements the game finds by id (0xSSSSIIII), from the root that
//! func_003E16B8 sets up (`lui $s0` +0x10, `addiu $s0` +0x18; Veldin's 0x1E1660; func_L01_00512D70 and its kin, three copies of
//! one code in each level, start from it too): root +4 is the screens' stack
//! S, the screen's container is the word at S + 4 * (S +0x14), the container's first word + 0x1C4 the table that
//! func_003E1150 searches: 1024 slots of {+8 id, +0xC element}, probe j at ((id & 0x3FF) + (id % 1023) * j + j) & 0x3FF, an
//! empty slot ending the search and a slot holding the removed mark (`lui $t3` +0x14, `addiu $t3` +0x18; Veldin's 0x1E24C8)
//! skipped. An element's +0x10 and +0x14 are its centre and +0x18 and +0x1C its size, in shares of the picture
//! (func_L01_00512480 sets the place; read on the Phoenix's QWARK COMICS: its five boxes at x 0.15, y 0.21 + 0.145 a box,
//! 0.112 x 0.135, the boxes on the picture).
//! - QWARK COMICS (screen 15; update func_L01_004922A0, its record set up by its getter like the screens above): the highlighted
//!   comic `lw gp` +0xD8 (Veldin 0x1D9058; Up and Down step it, wrapping and skipping the comics the game does not offer), the
//!   screen's sub-state `lw gp` +0x38 (0x1D9170: 0 while a comic is to be chosen, 1 and 2 the question Cross asks); comic k's box
//!   is the element 0x11000C + k. The game opens it only where the level registers the comics (the Phoenix): its open routine
//!   calls through that registration (func_L01_00491890, at 0x491A10 on Veldin), which no other level has: opened elsewhere the
//!   game calls address 0 there and the port stops, naming it.
//! - Screen 5 (update func_L01_00480578; a vid-comic's pause: CONTINUE, RESTART LEVEL, EXIT): the highlighted row `lw gp` +0x3C
//!   (0 to 2; Up and Down step it, wrapping and skipping rows that cannot be chosen), the rows' table `addiu gp` +0x30 (16 bytes a
//!   row: +0 its element, +8 whether it can be chosen, +0xC what Cross does), the rows' pitch `lwc1 gp` +0x16C; the highlight
//!   (the element 0xD0028) is put at x 0.5, y 0.5 + (row - 1) * pitch every field.

use crate::disc::Disc;
use crate::levelcode::{self, Overlay};
use crate::toc;

/// One story level's menu globals (EE addresses).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuGlobals {
    pub level: usize,
    /// The game's mode word: 1 while the game is paused (its menus and their screens).
    pub mode: u32,
    /// The game's current mode (the module doc): 3 the pause and the screens of its family a level opens from the world, 5 the
    /// vendor.
    pub current: u32,
    /// The pause list's table pointer (20-byte items, +8 enabled), its item count and its highlighted item.
    pub pause_table: u32,
    pub pause_count: u32,
    pub pause_index: u32,
    /// The first of the four list panels (0xB4 bytes each).
    pub panels: u32,
    /// The text table's pointer word and entry count word (16-byte entries {string, id, -1, 0xFFFF}).
    pub text: u32,
    pub text_count: u32,
    /// The save messages' struct: +0x40 the message (2: the autosave notice), +0x5C its frame count.
    pub saves: u32,
    /// Two words of the level's own mode function (its `lui` and `lw`, which carry the level's own address of the mode word), at
    /// `anchor` and `anchor + 4`. The level's menu code is in RAM only while they are there: a level change loads the next
    /// overlay over the last, and in between the addresses above hold another level's code and data.
    pub anchor: u32,
    pub anchor_words: [u32; 2],
    /// The pause screens' records (MISSIONS, WEAPONS, GADGETS, QUICK SELECT, ITEMS, and the save slots): the first word is 1 while
    /// that screen shows.
    pub screens: [u32; 6],
    /// The pause list's screen record (the module doc): its word at +0x58 is set while the pause list itself takes the input, and
    /// clear while OPTIONS or one of its lists does (the pause family's screen id stays 1 through them).
    pub pause_record: u32,
    /// MISSIONS: the highlighted row of the three shown, the scroll and the number of missions.
    pub missions: [u32; 3],
    /// WEAPONS: the column, the row, the x of column 0 and the step per column, the y of row 0 and of row 1 (floats, shares of
    /// the picture), and the byte the screen reads as "the highlight moved".
    pub weapons: [u32; 7],
    /// GADGETS: the column, the row, then the cursor's x0, y0, dx and dy (floats, shares of the picture).
    pub gadgets: [u32; 6],
    /// QUICK SELECT: its three cursor words.
    pub quick: [u32; 3],
    /// QUICK SELECT's ring: the highlighted slot (0 to 7).
    pub quick_ring: u32,
    /// ITEMS: the highlighted item (0 to 9).
    pub items: u32,
    /// The save slots: the screen's state (0 while a slot is to be chosen), the highlighted slot (0 to 3), the y of slot 0's row
    /// and the step per slot (floats, shares of the picture).
    pub save: [u32; 4],
    /// The vendor's struct (the module doc's "The vendor"; Veldin's 0x244740, level 7's 0x2422C0, level 12's 0x244480).
    pub vendor: u32,
    /// The vendor carousel's scale (a float, 1 or 1.0769; it scales the icons' row and height).
    pub vendor_scale: u32,
    /// The UI elements (the module doc's "UI elements"): the root and the table's mark of a removed element.
    pub ui: [u32; 2],
    /// QWARK COMICS: its record, the highlighted comic (0 to 4; the elements 0x11000C + comic are their boxes) and the screen's
    /// sub-state (0 while a comic is to be chosen, 1 and 2 its question).
    pub comics: [u32; 3],
    /// The pause family's screen 5, a few rows (a vid-comic's pause): its record, the highlighted row, the rows' table (16 bytes a
    /// row: +0 the row's element, +8 whether it can be chosen, +0xC what Cross does) and the rows' pitch (a float).
    pub rows: [u32; 4],
    /// The menus' sound, play_ui_sound(id, 0, 0) (the module doc).
    pub ui_sound: u32,
    /// CONTROLS / CAMERA's camera mode being set (the module doc), whose ROTATION SPEED its slider shows.
    pub controls_mode: u32,
}

struct Func {
    name: &'static str,
    address: u32,
    size: u32,
}

const MODE: Func = Func { name: "func_L01_00456C70", address: 0x45_6C70, size: 0x5C0 };
const PAUSE: Func = Func { name: "func_L01_0047BCC0", address: 0x47_BCC0, size: 0x5A4 };
// func_003B46F0 has a twin, func_003AEDF8 (the same code on the panels at 0x236818); with the function after it,
// func_003B4778 (a panel's cursor row), it is found once.
const PANELS: Func = Func { name: "func_003B46F0 and func_003B4778", address: 0x49_4DD0, size: 0x98 };
const TEXT: Func = Func { name: "func_0037DF28", address: 0x41_8130, size: 0x6C };
const SAVES: Func = Func { name: "func_003DBEC8", address: 0x41_3938, size: 0xE40 };
const MISSIONS_UPDATE: Func = Func { name: "func_L01_0048B318", address: 0x48_B318, size: 0x218 };
const MISSIONS_LIST: Func = Func { name: "func_L01_0048B198", address: 0x48_B198, size: 0x180 };
const GADGETS_CURSOR: Func = Func { name: "func_L01_00483D98", address: 0x48_3D98, size: 0x74 };
const WEAPONS_UPDATE: Func = Func { name: "func_L01_00482B78", address: 0x48_2B78, size: 0x4FC };
const GADGETS_UPDATE: Func = Func { name: "func_L01_004840D0", address: 0x48_40D0, size: 0x4C8 };
const QUICK_UPDATE: Func = Func { name: "func_L01_004864A8", address: 0x48_64A8, size: 0x1DC };
const RING_INPUT: Func = Func { name: "func_L01_00485D70", address: 0x48_5D70, size: 0x16C };
const ITEMS_UPDATE: Func = Func { name: "func_L01_00489D10", address: 0x48_9D10, size: 0x298 };
const SLOTS_UPDATE: Func = Func { name: "func_003B23F8", address: 0x47_E680, size: 0x248 };
const SAVE_UPDATE: Func = Func { name: "func_003B27A8", address: 0x47_EA30, size: 0x1AC };
const VENDOR_OPEN: Func = Func { name: "func_L01_004B7948", address: 0x4B_7948, size: 0x8AC };
const CAROUSEL: Func = Func { name: "func_L01_004BB1B8", address: 0x4B_B1B8, size: 0x938 };
const UI_ROOT: Func = Func { name: "func_003E16B8", address: 0x51_1BF8, size: 0xA8 };
const UI_FIND: Func = Func { name: "func_003E1150", address: 0x51_1690, size: 0x80 };
const COMICS_UPDATE: Func = Func { name: "func_L01_004922A0", address: 0x49_22A0, size: 0x850 };
const ROWS_UPDATE: Func = Func { name: "func_L01_00480578", address: 0x48_0578, size: 0x1E8 };
const PAUSE_UPDATE: Func = Func { name: "func_L01_0047AF70", address: 0x47_AF70, size: 0x138 };
const UI_SOUND: Func = Func { name: "func_0039FF28", address: 0x4A_7368, size: 0x90 };
const ROTATION_KNOB: Func = Func { name: "func_L01_0049D7F0", address: 0x49_D7F0, size: 0xCC };

fn sext(w: u32) -> i64 {
    (w & 0xFFFF) as u16 as i16 as i64
}

/// The address the `lui` at function offset `lui`, an optional `addiu` at `add` on its register and the load, store or `addiu`
/// at `at` on the result form together.
fn formed(ov: &Overlay, start: u32, f: &Func, lui: u32, add: Option<u32>, at: u32) -> Result<u32, String> {
    let w = |off: u32| ov.word(start + off).ok_or_else(|| format!("L{:02} {} +{off:#x}: no word", ov.level, f.name));
    let l = w(lui)?;
    if l >> 26 != 0x0F {
        return Err(format!("L{:02} {} +{lui:#x}: {l:08x} is not a lui", ov.level, f.name));
    }
    let (mut base, mut reg) = ((((l & 0xFFFF) << 16) as i64), (l >> 16) & 31);
    if let Some(add) = add {
        let a = w(add)?;
        if a >> 26 != 0x09 || (a >> 21) & 31 != reg {
            return Err(format!("L{:02} {} +{add:#x}: {a:08x} is not an addiu on the lui's register", ov.level, f.name));
        }
        base += sext(a);
        reg = (a >> 16) & 31;
    }
    let u = w(at)?;
    if !matches!(u >> 26, 0x09 | 0x23 | 0x2B | 0x31) || (u >> 21) & 31 != reg {
        return Err(format!("L{:02} {} +{at:#x}: {u:08x} is not an addiu, lw, sw or lwc1 on the register formed at +{lui:#x}", ov.level, f.name));
    }
    Ok((base + sext(u)) as u32)
}

/// The address a gp-relative `lw`, `sw`, `lbu`, `sb`, `lwc1`, `swc1` or `addiu` at function offset `at` reads, writes or forms.
fn via_gp(ov: &Overlay, start: u32, f: &Func, gp: u32, at: u32) -> Result<u32, String> {
    let u = ov.word(start + at).ok_or_else(|| format!("L{:02} {} +{at:#x}: no word", ov.level, f.name))?;
    if (u >> 21) & 31 != 28 || !matches!(u >> 26, 0x23 | 0x2B | 0x24 | 0x28 | 0x31 | 0x39 | 0x09) {
        return Err(format!("L{:02} {} +{at:#x}: {u:08x} is not a gp-relative lw, sw, lbu, sb, lwc1, swc1 or addiu", ov.level, f.name));
    }
    Ok((gp as i64 + sext(u)) as u32)
}

fn word(ov: &Overlay, at: u32) -> Result<u32, String> {
    ov.word(at).ok_or_else(|| format!("L{:02}: no word at {at:#x}", ov.level))
}

/// The record of the pause screen whose update routine is at `update`: the getter forms the routine's address in `$a3` (`lui`,
/// then `addiu $a3, $a3`) and sets up the record eight bytes after that `addiu` (`addiu $a0, $gp, off`).
fn screen_record(ov: &Overlay, name: &str, update: u32, gp: u32) -> Result<u32, String> {
    let (hi, lo) = (update.wrapping_add(0x8000) >> 16, update & 0xFFFF);
    let mut found = Vec::new();
    for r in &ov.records {
        let w = |i: usize| u32::from_le_bytes([r.data[4 * i], r.data[4 * i + 1], r.data[4 * i + 2], r.data[4 * i + 3]]);
        for i in 8..r.data.len() / 4 {
            // addiu $a3, $a3, lo
            if w(i) >> 16 != 0x24E7 || w(i) & 0xFFFF != lo {
                continue;
            }
            // lui $a3, hi, in the eight words before
            if !(i - 8..i).any(|j| w(j) == 0x3C07_0000 | hi) {
                continue;
            }
            // addiu $a0, $gp, off, two words on
            if i + 2 < r.data.len() / 4 && w(i + 2) >> 16 == 0x2784 {
                found.push(gp.wrapping_add(sext(w(i + 2)) as u32));
            }
        }
    }
    match found.as_slice() {
        [one] => Ok(*one),
        [] => Err(format!("L{:02}: no function sets up the record of {name} (its update routine at {update:#x})", ov.level)),
        many => Err(format!("L{:02}: {} functions set up the record of {name}: {many:x?}", ov.level, many.len())),
    }
}

/// The menu globals of an already read overlay; `reference` is level 1's, `gp` the level's gp (`levelcode::LevelGlobals::gp`).
pub fn from_overlay(reference: &Overlay, ov: &Overlay, gp: u32) -> Result<MenuGlobals, String> {
    let start = |f: &Func| levelcode::locate_function(reference, ov, f.name, f.address, f.size);
    let (mode, pause, panels, text, saves) = (start(&MODE)?, start(&PAUSE)?, start(&PANELS)?, start(&TEXT)?, start(&SAVES)?);
    let gp_at = |f: &Func, at: u32| -> Result<u32, String> { via_gp(ov, start(f)?, f, gp, at) };
    let record = |f: &Func| -> Result<u32, String> { screen_record(ov, f.name, start(f)?, gp) };
    Ok(MenuGlobals {
        level: ov.level,
        mode: formed(ov, mode, &MODE, 0x160, None, 0x164)?,
        current: formed(ov, mode, &MODE, 0x18, None, 0x1C)?,
        pause_table: via_gp(ov, pause, &PAUSE, gp, 0x88)?,
        pause_count: via_gp(ov, pause, &PAUSE, gp, 0xF0)?,
        pause_index: via_gp(ov, pause, &PAUSE, gp, 0xFC)?,
        panels: formed(ov, panels, &PANELS, 0x10, None, 0x2C)?,
        text: formed(ov, text, &TEXT, 0x1C, None, 0x20)?,
        text_count: formed(ov, text, &TEXT, 0x0, Some(0xC), 0x10)?,
        saves: formed(ov, saves, &SAVES, 0x4, None, 0x14)?,
        anchor: mode + 0x160,
        anchor_words: [word(ov, mode + 0x160)?, word(ov, mode + 0x164)?],
        screens: [record(&MISSIONS_UPDATE)?, record(&WEAPONS_UPDATE)?, record(&GADGETS_UPDATE)?, record(&QUICK_UPDATE)?, record(&ITEMS_UPDATE)?, record(&SAVE_UPDATE)?],
        pause_record: record(&PAUSE_UPDATE)?,
        missions: [gp_at(&MISSIONS_LIST, 0x88)?, gp_at(&MISSIONS_LIST, 0xA4)?, gp_at(&MISSIONS_LIST, 0xC4)?],
        weapons: [gp_at(&WEAPONS_UPDATE, 0x104)?, gp_at(&WEAPONS_UPDATE, 0xC4)?, gp_at(&WEAPONS_UPDATE, 0x2AC)?, gp_at(&WEAPONS_UPDATE, 0x29C)?, gp_at(&WEAPONS_UPDATE, 0x2CC)?, gp_at(&WEAPONS_UPDATE, 0x2C0)?, gp_at(&WEAPONS_UPDATE, 0xCC)?],
        gadgets: [gp_at(&GADGETS_UPDATE, 0xBC)?, gp_at(&GADGETS_UPDATE, 0xC8)?, gp_at(&GADGETS_CURSOR, 0x40)?, gp_at(&GADGETS_CURSOR, 0x54)?, gp_at(&GADGETS_CURSOR, 0x14)?, gp_at(&GADGETS_CURSOR, 0x34)?],
        quick: [gp_at(&QUICK_UPDATE, 0x34)?, gp_at(&QUICK_UPDATE, 0x44)?, gp_at(&QUICK_UPDATE, 0x50)?],
        quick_ring: gp_at(&RING_INPUT, 0x2C)?,
        items: gp_at(&ITEMS_UPDATE, 0x2C)?,
        save: [gp_at(&SAVE_UPDATE, 0x1C)?, gp_at(&SLOTS_UPDATE, 0x28)?, gp_at(&SLOTS_UPDATE, 0x1FC)?, gp_at(&SLOTS_UPDATE, 0x200)?],
        vendor: formed(ov, start(&VENDOR_OPEN)?, &VENDOR_OPEN, 0x4, None, 0x1C)?,
        vendor_scale: formed(ov, start(&CAROUSEL)?, &CAROUSEL, 0x120, None, 0x124)?,
        ui: [formed(ov, start(&UI_ROOT)?, &UI_ROOT, 0x10, None, 0x18)?, formed(ov, start(&UI_FIND)?, &UI_FIND, 0x14, None, 0x18)?],
        comics: [record(&COMICS_UPDATE)?, gp_at(&COMICS_UPDATE, 0xD8)?, gp_at(&COMICS_UPDATE, 0x38)?],
        rows: [record(&ROWS_UPDATE)?, gp_at(&ROWS_UPDATE, 0x3C)?, gp_at(&ROWS_UPDATE, 0x30)?, gp_at(&ROWS_UPDATE, 0x16C)?],
        ui_sound: start(&UI_SOUND)?,
        controls_mode: formed(ov, start(&ROTATION_KNOB)?, &ROTATION_KNOB, 0x0, None, 0x4)?,
    })
}

/// Every story level's menu globals, in level order.
pub fn all(iso_path: &str) -> Result<Vec<MenuGlobals>, String> {
    let mut d = Disc::open(iso_path)?;
    let tc = toc::read(&mut d)?;
    let reference = levelcode::overlay(&mut d, &tc, 1)?;
    let gp = levelcode::from_overlay(&reference, &reference)?.gp;
    levelcode::STORY_LEVELS
        .iter()
        .map(|&l| {
            let ov = levelcode::overlay(&mut d, &tc, l)?;
            from_overlay(&reference, &ov, gp)
        })
        .collect()
}
