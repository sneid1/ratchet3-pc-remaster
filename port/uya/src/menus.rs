//! The game's menus as a PC game's (`play`): the mouse points at an item and clicks it, the right button goes back, and the
//! port's PC settings (`settings.rs`) are a page of the game's own option lists, drawn in the game's own look.
//!
//! Everything here is the game's own state in its RAM, read and written between fields (docs/HANDOVER.md "Play window and
//! controls"):
//! - The front end's screen showing: the word at 0x306F5C points at its record (main menu 0x1DA1C8, Options 0x1DA1E8). Its list
//!   panel keeps its rows after Options closes, so it is a list only while Options shows.
//! - Main menu: 5 items (table 0x3376C0, 16 bytes each, +8 enabled); the highlighted one is the word at 0x1D8978; item i's bar
//!   is centred at (the float at 0x1D89A8, the float i at 0x1D89B0), sized (0x1D89A4, the float i at 0x1D89C8), in shares of the
//!   picture. Cross picks the highlighted item.
//! - A level's pause: the game's mode word is 1 while it is paused (its menus and their screens). The pause list
//!   (MISSIONS ... SPECIAL) is the table the pause table word points at (20 bytes an item, +8 enabled), as many items as the count
//!   word, the highlighted one the index word; item i's bar is centred at (0.502, 0.5 - (n - 1) * 0.03 + 0.06 i), 0.468 x 0.05
//!   (Veldin's program, 0x47C400). Every one of those words, the list panels, the text table and the save messages sit at a
//!   different address in each level program (the level's own copy of the menu code links its globals where it likes): they
//!   are `uyafmt::menucode::all`, read from each level's own code (Veldin's: mode 0x1D7DC0, table 0x1D8310, index 0x1D8318,
//!   count 0x1D831C), and used only while the level's code is in RAM (`MenuGlobals::anchor`).
//! - The option lists (the front end's Options, a pause's OPTIONS and AUDIO / VISUAL screens) are list panels: the front end's
//!   at 0x280D70, a level's the first of `MenuGlobals::panels` (Veldin's 0x236868; +0 active, +4 scroll, +8 cursor, +0xC rows, +0x20 the two columns' x, +0x40 their
//!   widths, +0x50 the first row's centre y, +0x54 the row pitch, +0xB0 the cells, 0x44 bytes each, row by row, label then
//!   value). The game writes the cells again every field before it draws them, so the port's text is put in the picture instead
//!   (`Line`, drawn by `front.rs` in the game's font, colours and alignment, over the game's letters on that line).
//!
//! - The pause's screens (WEAPONS, GADGETS, QUICK SELECT, ITEMS, and the save slots): the screen showing is the record (`MenuGlobals::screens`) whose
//!   first word is 1. Each is a grid of cells under the pointer and its cursor is a few words of the level's globals
//!   (`MenuGlobals`): the pointer writes the cell's numbers there, as the d-pad would, and the game's own update does the rest
//!   (the highlight, the picture and the texts of the item). The cells' places are the screens' layouts: the weapons grid's
//!   (`weapons`: x of column 0, the step, the y of both rows, read from the level's RAM) is also the columns of the ITEMS and
//!   the quick-select grid; the gadgets' (`gadgets`) are the cursor's own; the other rows are the game's own pictures measured
//!   (the layout is one data in every level: 0.1241 + 0.0832 column, in shares of the picture). The footer prompts stay hidden
//!   as on every pointer screen (`Frame::menu`).
//!
//! - The vendor (the game's current mode 5; `MenuGlobals::vendor`, `uyafmt::menucode`'s doc): on its carousel pages (weapons,
//!   ship, armor) a click on an item beside the highlighted one brings it to the middle, the carousel stepping to it with the
//!   game's own Left and Right a field at a time as it allows them; a click on the highlighted item is Cross (BUY). The items'
//!   places are the carousel's own (item k after the highlighted one at GS x 238 + 56 k + the scroll, 38 pixels square, on the
//!   row the carousel's scale sets), those 3 or more away faded out as the game fades them. The SKINS page is a list panel.
//!
//! A list's PROGRESSIVE SCAN row (its label the game's text 0x509 with a colon: a PS2 video mode, nothing on a PC) is the
//! PC SETTINGS row. The game does not see Cross, Left or Right on it; Cross (or a click) turns the list into the settings page,
//! laid out as a PC game's graphics settings are, in the game's own list, font and colours (`settings.rs` holds the options):
//! - a tab per section of `settings::TABS` in the title band (DISPLAY, GRAPHICS, CAMERA, KEYBOARD, CONTROLLER, GAMEPLAY,
//!   AUDIO): a click on its button, L1 or R1, L2 or R2 (Z or C, Shift on the keyboard) changes tab;
//! - GRAPHICS opens with the quality preset (ORIGINAL, LOW, MEDIUM, HIGH, ULTRA; CUSTOM when the options are at none of them),
//!   then a row per option of the tab (`settings::OPTIONS`), then RESET TO DEFAULTS (the tab's options back to their defaults:
//!   GRAPHICS to ORIGINAL);
//! - the highlighted row's value with two arrow buttons after it; Right, Cross or a click on the row (or on its right arrow)
//!   takes the next value, Left or a click on the left arrow the one before; a changed option is applied and saved at once;
//! - the highlighted option's description under the list (what it does and what it costs);
//! - KEYBOARD and CONTROLLER are the bindings (`bindings.rs`): a row per action with its two slots as buttons (the keyboard and
//!   mouse) or per PS2 button with the controller button that presses it; a click on a slot (or Cross on the row, Left and Right
//!   for the first and second slot) waits for the input to put there (`Menus::capture`), and the bindings are applied and saved
//!   at once;
//! - a tab with more rows than the panel shows scrolls as the cursor reaches the panel's edge (the front end's panel shows 5
//!   rows, a pause's 8); the rows past a tab's last are emptied.
//!
//! The game sees none of the buttons that would change or leave its own list; Triangle, Start or the right button returns to
//! it. R1 (the front end's PS2 screen position) moves nothing.

use crate::bindings::{self, Bindings, Key};
use crate::input::{Device, Pad, Presses, CENTRE, CROSS, DOWN, L1, L2, LEFT, R1, R2, RIGHT, SELECT, START, TRIANGLE, UP};
use crate::settings::{Settings, Tab, OPTIONS as OPTS, PRESETS, TABS};
use winit::keyboard::KeyCode;
use crate::input::Answer;
use uyafmt::menucode::MenuGlobals;

/// Where the cursor is on the picture (shares of it; None: off it) and what the mouse did since the last field: moved, the left
/// button clicked, the left button held now (a drag), the wheel turned (notches, up positive).
#[derive(Clone, Copy, Default, Debug)]
pub struct Pointer {
    pub at: Option<(f32, f32)>,
    pub moved: bool,
    pub click: bool,
    pub down: bool,
    pub wheel: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
    Centre,
}

/// Text of the port's in place of the game's: the game's letters centred in the band (shares of the picture: y the centre, `h`
/// its height, x0 to x1) are replaced by `text`, aligned as the game's were; `all`: everything drawn in the band goes (a row
/// emptied, its slider too). `at`: the text is placed by the line itself instead (at that x, by `align`, on the band's centre),
/// in the style of its band's letters or, when the band has none, of the last line before it that had (`front.rs`), `size`
/// times as large; `dim`: its colour at half; `prose`: at the font's own letter widths (the game's lines are stretched to the
/// game's span, which spreads a sentence's small letters apart); `plate`: a button the port draws behind the text, over the band
/// (`front.rs`).
#[derive(Clone, Debug)]
pub struct Line {
    pub y: f32,
    pub h: f32,
    pub x0: f32,
    pub x1: f32,
    pub text: String,
    pub align: Align,
    pub all: bool,
    pub at: Option<f32>,
    pub dim: bool,
    pub size: f32,
    pub prose: bool,
    pub plate: Option<(Plate, Shape)>,
}

/// A tab of the settings page's tab bar as the window draws it (`Menus::tab_bar`): its button (shares of the picture), its name
/// and its look.
#[derive(Clone, Copy, Debug)]
pub struct TabShown {
    pub rect: [f32; 4],
    pub name: &'static str,
    pub look: Plate,
}

/// A button's look (`Line::plate`), as the labels' plates are drawn (`prompts::plate`): dark with a gold edge, lit amber under the
/// pointer, and `On` (a tab showing, a scroll bar's thumb) amber whatever the pointer does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plate {
    Off,
    Lit,
    On,
}

/// What a plate is: a box (its text drawn over it), an arrow up or down (a scroll bar's), or left or right (an option's value
/// before and after).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Box,
    Up,
    Down,
    Left,
    Right,
}

impl Line {
    /// The game's letters in the band replaced, aligned as theirs were.
    fn game(y: f32, h: f32, (x0, x1): (f32, f32), text: &str, align: Align, all: bool) -> Line {
        Line { y, h, x0, x1, text: text.into(), align, all, at: None, dim: false, size: 1.0, prose: false, plate: None }
    }

    /// A button the port draws: a plate over `rect` (shares of the picture), its text centred on it at `size` of the last game
    /// line's letters.
    fn button(rect: [f32; 4], text: &str, look: (Plate, Shape), size: f32) -> Line {
        let (y, h) = ((rect[1] + rect[3]) / 2.0, rect[3] - rect[1]);
        Line { y, h, x0: rect[0], x1: rect[2], text: text.into(), align: Align::Centre, all: false, at: Some((rect[0] + rect[2]) / 2.0), dim: false, size, prose: false, plate: Some(look) }
    }

    /// The same text placed at `at` by the line.
    fn at(self, at: f32, dim: bool, size: f32) -> Line {
        Line { at: Some(at), dim, size, ..self }
    }

    /// The same at the font's own letter widths.
    fn prose(self) -> Line {
        Line { prose: true, ..self }
    }
}

/// A row of the settings page: the quality preset, an option (its place in `settings::OPTIONS`), a binding (an action of the
/// keyboard and mouse, its place in `bindings::ACTIONS`; a PS2 button of a controller's, its place in `bindings::PAD`), the reset.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Item {
    Preset,
    Opt(usize),
    Key(usize),
    Pad(usize),
    Reset,
}

/// Tab `tab`'s rows.
fn items(tab: Tab) -> Vec<Item> {
    let mut v = Vec::new();
    match tab {
        Tab::Graphics => v.push(Item::Preset),
        Tab::Keyboard => v.extend((0..bindings::ACTIONS.len()).map(Item::Key)),
        Tab::Controller => v.extend((0..bindings::PAD.len()).map(Item::Pad)),
        _ => {}
    }
    v.extend(OPTS.iter().enumerate().filter(|(_, o)| o.tab == tab).map(|(i, _)| Item::Opt(i)));
    v.push(Item::Reset);
    v
}

/// The settings page: the tab showing (its place in `TABS`) and the first of its rows the panel shows.
#[derive(Clone, Copy, Debug)]
struct Page {
    tab: usize,
    scroll: u32,
}

/// The settings page's tab bar: a button a tab across the top of the option screen's picture (the front end's OPTIONS and a
/// pause's AUDIO / VISUAL both start theirs 0.14 down the frame, 0.09 to 0.91 across): from x 0.11 to 0.89, centred 0.178 down,
/// 0.046 high, 0.008 apart, drawn by the window over the picture (`Menus::tab_bar`). The tab showing is lit, the one under the
/// pointer lights, a click on one shows it (the player: "there needs to be something that shows how to go into the other pc menu
/// pages ... there is no button and no mouse thing to do it"). L1 / R1, L2 / R2, Tab and their keys step the tabs too.
const TAB_BAR: (f32, f32, f32, f32, f32) = (0.11, 0.89, 0.178, 0.046, 0.008);

/// Tab `k`'s button (x0, y0, x1, y1, shares of the picture).
fn tab_rect(k: usize) -> [f32; 4] {
    let (x0, x1, y, h, gap) = TAB_BAR;
    let w = (x1 - x0 - gap * (TABS.len() - 1) as f32) / TABS.len() as f32;
    let l = x0 + k as f32 * (w + gap);
    [l, y - h / 2.0, l + w, y + h / 2.0]
}

/// The scroll bar at the right end of the page's rows when a tab has more rows than the panel shows: from `SCROLL.0` left of the
/// value column's right edge to `SCROLL.1` right of it (the rows' bands run on to it), an arrow on the first row shown and one on
/// the last, the track between them with its thumb over the part shown; the values end `SCROLL.2` left of the column's edge. A
/// click on an arrow scrolls a row, on the track a panel's worth; the mouse wheel scrolls a row a notch, over any of the page.
const SCROLL: (f32, f32, f32) = (0.014, 0.008, 0.02);

/// The scroll bar's up arrow, down arrow and track, for rows centred from `top` at `pitch`, `shown` of them, the value column's
/// right edge at `right`.
fn scroll_rects(top: f32, pitch: f32, shown: u32, right: f32) -> [[f32; 4]; 3] {
    let (x0, x1) = (right - SCROLL.0, right + SCROLL.1);
    let band = |r: u32| {
        let y = top + r as f32 * pitch;
        [x0, y - pitch * 0.42, x1, y + pitch * 0.42]
    };
    let (up, down) = (band(0), band(shown - 1));
    [up, down, [x0 + 0.004, up[3] + 0.006, x1 - 0.004, down[1] - 0.006]]
}

fn inside(r: [f32; 4], (u, v): (f32, f32)) -> bool {
    u >= r[0] && u <= r[2] && v >= r[1] && v <= r[3]
}

/// A binding row's slots: buttons across the value column (`left` to `right`), `SLOT_GAP` apart, the KEYBOARD tab's two and the
/// CONTROLLER tab's one, each showing what is bound at `SLOT_SIZE` of the rows' letters (smaller when a name is longer than its
/// button: `front.rs`); the one waiting for an input lit.
const SLOT_GAP: f32 = 0.012;
const SLOT_SIZE: f32 = 0.8;

/// The slots of the panel's row `r` (rows centred from `top` at `pitch`), `n` of them.
fn slot_rects(top: f32, pitch: f32, r: u32, (left, right): (f32, f32), n: usize) -> Vec<[f32; 4]> {
    let y = top + r as f32 * pitch;
    let w = (right - left - SLOT_GAP * (n - 1) as f32) / n as f32;
    (0..n).map(|k| left + k as f32 * (w + SLOT_GAP)).map(|l| [l, y - pitch * 0.4, l + w, y + pitch * 0.4]).collect()
}

/// While a keyboard slot waits for its input, CLEAR (the slot emptied) at the right end of the description row (centred at `y`),
/// `CLEAR_W` wide, the description beside it.
const CLEAR_W: f32 = 0.11;

fn clear_rect(y: f32, pitch: f32, right: f32) -> [f32; 4] {
    [right - CLEAR_W, y - pitch * 0.38, right, y + pitch * 0.38]
}

/// The highlighted option's arrows: two buttons `ARROW_W` wide side by side at the right end of the value column, `ARROW_GAP`
/// apart, the value right-aligned before them (a value of any length clear of both). A click on the left arrow takes the value
/// before; anywhere else on the row the next (the player: "You should be able to click the thing you want with a mouse").
const ARROW_W: f32 = 0.026;
const ARROW_GAP: f32 = 0.01;

/// The arrows of the panel's row `r` (rows centred from `top` at `pitch`), the value column ending at `right`: left, right.
fn arrow_rects(top: f32, pitch: f32, r: u32, right: f32) -> [[f32; 4]; 2] {
    let y = top + r as f32 * pitch;
    let (y0, y1) = (y - pitch * 0.34, y + pitch * 0.34);
    let l = right - 2.0 * ARROW_W - ARROW_GAP;
    [[l, y0, l + ARROW_W, y1], [right - ARROW_W, y0, right, y1]]
}
/// The description in the panel's last row: its two lines' centres either side of the row's middle (in rows of the panel's
/// pitch) and their size.
const ABOUT: (f32, f32) = (0.24, 0.64);
/// The longest description line in letters (a longer one breaks at a space).
const ABOUT_WIDTH: usize = 66;
/// The front end's footer: its middle label (the R1 glyph's ADJUST SCREEN, the PS2's picture position) in shares of the picture
/// (its centre y, height, x0, x1), which reads NEXT TAB on the page (R1 changes tab there).
const FOOTER_R1: (f32, f32, f32, f32) = (0.929, 0.04, 0.3, 0.7);

/// The front end's slot screens (NEW GAME's and LOAD GAME's four slots): they show while the memory card message word is 18
/// (`play::MC_MESSAGE`), and use the same globals as a level's SAVE and LOAD screen (`MenuGlobals::save`: gp-relative to the main
/// program's gp, 0x1DCF60, so one address for the front end and every level): the screen's state (gp-0x44F0, 0 while a slot is to
/// be chosen), the highlighted slot (gp-0x447C, 0 to 3), slot 0's row centre and the rows' step (gp-0x4498, gp-0x44C8: shares of
/// the picture, 0.218 and 0.182). On the LOAD screen the slot word stepped 0, 1, 2 with two Down presses (RAM at fields 899, 999,
/// 1099 of a scripted boot) and nothing else did; until 14 November 2026 a click there was Cross on the highlighted slot, so a
/// click on slot 2 loaded slot 1.
const MC_MESSAGE: usize = 0x1D4CE8;
const MC_SLOTS: u32 = 18;
const FRONT_SLOTS: [usize; 4] = [0x1D8A70, 0x1D8AE4, 0x1D8AC8, 0x1D8A98];

const SCREEN: usize = 0x306F5C;
const MAIN_MENU: u32 = 0x1DA1C8;
const OPTIONS: u32 = 0x1DA1E8;
const MAIN_TABLE: usize = 0x3376C0;
const MAIN_INDEX: usize = 0x1D8978;
const MAIN_X: usize = 0x1D89A8;
const MAIN_W: usize = 0x1D89A4;
const MAIN_Y: usize = 0x1D89B0;
const MAIN_H: usize = 0x1D89C8;
const MAIN_ITEMS: usize = 5;

const PAUSED: u32 = 1;
/// The pause list's record's word set while the pause list itself takes the input (`MenuGlobals::pause_record`).
const PAUSE_LIST_ACTIVE: usize = 0x58;
/// The game's current modes that show a menu (`MenuGlobals::current`): the pause and its family's screens, the vendor.
const MENU_MODES: [u32; 2] = [3, 5];
/// The game's current mode in play (`MenuGlobals::current`; 2 is a cutscene, 4 the quick select, 6 the ship).
const PLAY_MODE: u32 = 0;
/// The menus' sounds (play_ui_sound's ids, `MenuGlobals::ui_sound`): a cursor's step, and a choice.
const CURSOR_SOUND: u32 = 3;
const CHOICE_SOUND: u32 = 4;
/// The main program's play_ui_sound (rac3-decomp func_0039FF28), the front end's menus' sound, and its words at +0 and +0xC to
/// +0x18, the same in every copy (+4 and +8 load its sound table).
const FRONT_UI_SOUND: u32 = 0x3A_5608;
const UI_SOUND_WORDS: [u32; 5] = [0x27bd_ffe0, 0xffb0_0000, 0xffb1_0008, 0x0080_802d, 0xffbf_0010];

/// The vendor's carousel pages (weapons, ship, armor; `uyafmt::menucode`'s doc): its words in the vendor's struct, the frame's size
/// in GS pixels, the word 0x143950 (set: the carousel 4 pixels lower).
const VENDOR_CAROUSEL: [u32; 3] = [0, 2, 3];
const VENDOR_MODE: u32 = 5;
const GS_FRAME: (f32, f32) = (512.0, 416.0);
const VENDOR_LOW: usize = 0x14_3950;

/// QWARK COMICS' comics, the element of comic 0's box; screen 5's highlight (`uyafmt::menucode`'s doc, "The UI elements").
const COMICS: u32 = 5;
const COMIC_BOX: u32 = 0x11_000C;
const ROWS_BAR: u32 = 0xD_0028;

/// The UI element `id` of the screen showing, as a cell (its centre and half size, shares of the picture), when the screen has
/// it: the root's screen stack, the screen's container, its table of 1024 slots searched as func_003E1150 searches it
/// (`uyafmt::menucode`'s doc, "The UI elements").
fn element(ram: &[u8], ui: [u32; 2], id: u32) -> Option<Cell> {
    let word = |a: u32| (a as usize + 4 <= ram.len()).then(|| r32(ram, a as usize));
    let stack = word(ui[0] + 4).filter(|&s| s != 0)?;
    let container = word(stack + 4 * word(stack + 0x14)?).filter(|&c| c != 0)?;
    let table = word(container)? + 0x1C4;
    for j in 0..1024 {
        let slot = ((id & 0x3FF) + (id % 1023) * j + j) & 0x3FF;
        let e = word(table + 0xC + 8 * slot)?;
        if e == 0 {
            return None;
        }
        if word(table + 8 + 8 * slot)? == id && e != ui[1] {
            let f = |o: u32| word(e + o).map(f32::from_bits);
            return Some(Cell { x: f(0x10)?, y: f(0x14)?, hw: f(0x18)? / 2.0, hh: f(0x1C)? / 2.0 });
        }
    }
    None
}

/// The front end's slot screens' pictures (NEW GAME's and LOAD GAME's, while the memory card message word is `MC_SLOTS`). Slot
/// k's level (the word at 0x142460 + 0x1C k, as the front end read it from the save: 27, 1, 3 on the player's card; -1 an empty
/// slot) is picture n of frontbin func_003B5C68's table at 0x1D8C20 (a byte a level, 1 to 29); an empty slot's is picture 1, the
/// frame its EMPTY is written over (the game reads it for the empty slot: the 5-sector read at lba 0x11B34D, and a level's SAVE
/// screen, which the game draws itself, shows it),
/// which the game draws in the box on the slot's left (its element 0x4C0008 + k, which func_003B1EB0 sets up) from a disc read of
/// WAD8 pair 1252 + n. The port's runtime serves the reads (20 November 2026: the four came back, the pictures' streams in RAM),
/// but the picture object never reaches the state its draw waits for and the boxes stayed blank, so the port draws each picture
/// in its box itself (`prompts::prepare`, the pictures `uyafmt::wad38::slot_pictures`; the player: "Cant we just dump the images
/// and use them in the save slots based on the planet"). Each is placed as a level's SAVE screen, whose pictures the game draws
/// itself, places them (`SLOT_PICTURE`), on the slots' rows as the game places them (`FRONT_SLOTS`: slot 0's centre and the step,
/// 0.218 and 0.182). The rectangles in shares of the picture (x0, y0, x1, y1), with the picture's index (n - 1).
pub fn slot_pictures(ram: &[u8]) -> Vec<([f32; 4], usize)> {
    if !Menus::front_end(ram) || r32(ram, MC_MESSAGE) != MC_SLOTS {
        return Vec::new();
    }
    let (y0, dy) = (rf(ram, FRONT_SLOTS[2]), rf(ram, FRONT_SLOTS[3]));
    let mut out = Vec::new();
    for k in 0..4u32 {
        let level = r32(ram, SLOT_LEVELS + 0x1C * k as usize) as i32;
        if level as usize >= SLOT_TABLE_LEVELS && level >= 1 {
            continue;
        }
        let n = if level < 1 { 1 } else { ram[SLOT_TABLE + level as usize] as usize };
        if !(1..=uyafmt::wad38::SLOT_PICTURE_COUNT).contains(&n) {
            continue;
        }
        let (x0, x1, above, h) = SLOT_PICTURE;
        let top = ((y0 + dy * k as f32) * GS_FRAME.1 - above).floor() + 0.5;
        out.push(([x0 / GS_FRAME.0, top / GS_FRAME.1, x1 / GS_FRAME.0, (top + h) / GS_FRAME.1], n - 1));
    }
    out
}

/// A slot's picture as a level's SAVE screen draws it (Veldin's, field 2400, 20 November 2026: the 256 x 128 picture over window
/// x 44.5 to 307.5 and y 60.5, 136.5, 212.5, 287.5 to 65 below, the rows' centres at 90.7 + 75.7 k; its element 0x4C0008 + k is
/// the front end's own, centre (0.3468, 0.23 + 0.182 k), 0.5119 x 0.1585): x0, x1, its top above the row's centre (rounded down
/// to a pixel, at its half), its height, in GS pixels.
const SLOT_PICTURE: (f32, f32, f32, f32) = (44.5, 307.5, 30.0, 65.0);

/// The slots' levels (`slot_pictures`), the level to picture table and its length (levels 0 to 36).
const SLOT_LEVELS: usize = 0x14_2460;
const SLOT_TABLE: usize = 0x1D_8C20;
const SLOT_TABLE_LEVELS: usize = 37;

/// Where the grids' rows are on the picture (the game's own pictures measured: shares of the picture, the rows' centres).
const ITEM_ROWS: [f32; 2] = [0.701, 0.814];
const QUICK_ROWS: [f32; 4] = [0.427, 0.552, 0.676, 0.801];
/// The missions list (up to three rows shown under the mission's picture): the y of row 0 and the step.
const MISSION_ROWS: (f32, f32) = (0.68, 0.07);
/// The quick-select ring: its centre and radii on the picture (the 8 slots at every 45 degrees from the top, clockwise) and an
/// octagon's half size. Slot k's element sits k steps clockwise of the top (slot 0 at the top, slot 2 on the right: the elements'
/// rectangles in the RAM do not move when the ring turns) and the ring turns to bring the highlighted slot to the top, so the
/// slot shown `p` steps clockwise of the top is the highlighted one plus `p`.
const RING: ((f32, f32), (f32, f32), (f32, f32)) = ((0.7075, 0.3855), (0.1405, 0.1675), (0.05, 0.062));
/// The quick-select screen's gadget grid (4 columns, 2 rows): x of column 0, the step and the rows.
const QUICK_GADGETS: (f32, f32, [f32; 2]) = (0.613, 0.0855, [0.703, 0.808]);

/// The pause screens that have pointer targets, in the order of `MenuGlobals::screens`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Screen {
    Missions,
    Weapons,
    Gadgets,
    Quick,
    Items,
    /// OPTIONS > SAVE and LOAD: four slots one above the other.
    Save,
}

/// A cell on the picture: the centre and the half size (shares of the picture).
#[derive(Clone, Copy)]
struct Cell {
    x: f32,
    y: f32,
    hw: f32,
    hh: f32,
}

impl Cell {
    fn has(&self, (u, v): (f32, f32)) -> bool {
        (u - self.x).abs() <= self.hw && (v - self.y).abs() <= self.hh
    }
}

/// The front end's list panel and text table (pointer, count: 16-byte entries {string, id, -1, 0xFFFF}); a level's are in its
/// `MenuGlobals`.
const FRONT_PANEL: usize = 0x280D70;
const FRONT_TEXT: (usize, usize) = (0x1D9A20, 0x31CD2C);
/// PROGRESSIVE SCAN's text (its row is the PC SETTINGS row), and 16:9 (WIDESCREEN)'s (its row shows and changes the port's
/// WIDESCREEN setting: the game's own 16:9 option crops the picture, `settings.rs`).
const PROGRESSIVE: u32 = 0x509;
const WIDESCREEN: u32 = 0x508;
/// An option screen's title band, measured on the picture (the front end's OPTIONS, a pause's AUDIO / VISUAL).
const TITLE: (f32, f32) = (0.071, 0.04);

fn r32(ram: &[u8], a: usize) -> u32 {
    u32::from_le_bytes(ram[a..a + 4].try_into().expect("4 bytes"))
}
fn w32(ram: &mut [u8], a: usize, v: u32) {
    ram[a..a + 4].copy_from_slice(&v.to_le_bytes());
}
fn rf(ram: &[u8], a: usize) -> f32 {
    f32::from_bits(r32(ram, a))
}
/// A NUL-terminated string in RAM.
fn string(ram: &[u8], a: usize) -> &[u8] {
    let s = &ram[a.min(ram.len())..(a + 0x100).min(ram.len())];
    &s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())]
}

/// An option list showing, with a Display row: its panel, its rows and that row.
#[derive(Clone)]
struct List {
    panel: usize,
    rows: u32,
    display: u32,
    /// The 16:9 (WIDESCREEN) row, where the list has one.
    wide: Option<u32>,
    /// The list's sliders (a pause's AUDIO / VISUAL volumes).
    sliders: Vec<Slider>,
}

/// The row of the list panel at `panel` whose label is the text `id` of the loaded language (the text table's pointer and count
/// words: the first entry with the id, as the game's lookup finds it), with or without a colon after it.
fn labelled(ram: &[u8], panel: usize, (text, count): (usize, usize), id: u32) -> Option<u32> {
    let cells = r32(ram, panel + 0xB0) as usize;
    let (table, n) = (r32(ram, text) as usize, r32(ram, count) as usize);
    let label = (0..n).find(|&i| r32(ram, table + 16 * i + 4) == id).map(|i| string(ram, r32(ram, table + 16 * i) as usize))?;
    (0..r32(ram, panel + 0xC)).find(|&r| {
        let c = string(ram, cells + r as usize * 2 * 0x44);
        c == label || c.len() == label.len() + 1 && c.starts_with(label) && c.ends_with(b":")
    })
}

/// The game's sliders in its option lists (`uyafmt::menucode`'s doc, "The option lists' sliders"): a pause's AUDIO / VISUAL
/// volumes and CONTROLS / CAMERA's ROTATION SPEED, each found by its label in the panel. A slider: its row in the panel, its
/// value word, its largest value and the step a press of Left or Right makes, its knob's pad and span on the bar (the knob at the
/// bar's left end + pad + (the bar's length - span) x value / max).
#[derive(Clone, Copy, Debug)]
struct Slider {
    row: u32,
    word: usize,
    max: u32,
    step: u32,
    pad: f32,
    span: f32,
}

/// The volumes' texts and words; ROTATION SPEED's text and the words, 0x10 apart, of the three camera modes.
const VOLUMES: [(u32, usize); 2] = [(0x234, 0x14_395C), (0x233, 0x14_3960)];
const ROTATION_SPEED: u32 = 0x23C;
const ROTATION_SPEEDS: usize = 0x14_39C4;

/// The sliders of the level `g`'s list panel at `panel`.
fn sliders(ram: &[u8], g: &MenuGlobals, panel: usize) -> Vec<Slider> {
    let text = (g.text as usize, g.text_count as usize);
    let mut v: Vec<Slider> = VOLUMES.iter().filter_map(|&(id, word)| labelled(ram, panel, text, id).map(|row| Slider { row, word, max: 0x400, step: 0x1E, pad: 0.008, span: 0.008 })).collect();
    if let Some(row) = labelled(ram, panel, text, ROTATION_SPEED) {
        let word = ROTATION_SPEEDS + 0x10 * r32(ram, g.controls_mode as usize) as usize;
        v.push(Slider { row, word, max: 100, step: 2, pad: 0.007, span: 0.012 });
    }
    v
}

/// A list panel's rows and bar as the sliders use them: the first row shown, the first shown row's centre and the pitch, a
/// slider's bar from `l` (the label column's x plus its width) to `r` (the value column's x).
#[derive(Clone, Copy, Debug)]
struct Bars {
    first: u32,
    top: f32,
    pitch: f32,
    l: f32,
    r: f32,
}

fn bars(ram: &[u8], panel: usize) -> Bars {
    Bars { first: r32(ram, panel + 4), top: rf(ram, panel + 0x50), pitch: rf(ram, panel + 0x54), l: rf(ram, panel + 0x20) + rf(ram, panel + 0x40), r: rf(ram, panel + 0x24) }
}

/// A slider the pointer is setting (`Menus::slider`): its row, value word and step, the value it is taken to, the value it had
/// when the port pressed Left or Right on the last field (None: no press then), and whether the left button was down at the last
/// field (where it is let go still sets the value: a drag shorter than a field).
#[derive(Clone, Copy, Debug)]
struct Slide {
    row: u32,
    word: usize,
    step: u32,
    to: u32,
    pressed: Option<u32>,
    held: bool,
}

impl List {
    /// The option list showing with a PROGRESSIVE SCAN row, if there is one: the front end's (`g` None), or the level `g`'s.
    fn find(ram: &[u8], g: Option<&MenuGlobals>) -> Option<List> {
        let (panel, (text, count)) = match g {
            None if Menus::front_end(ram) && r32(ram, SCREEN) == OPTIONS => (FRONT_PANEL, FRONT_TEXT),
            // A level's list panel keeps its active byte after its screen closes: it is on the screen only while the pause list
            // is not (`Menus::pause_list`).
            Some(g) if !Menus::pause_list(ram, g) => (g.panels as usize, (g.text as usize, g.text_count as usize)),
            _ => return None,
        };
        let cells = r32(ram, panel + 0xB0) as usize;
        if ram[panel] == 0 || cells == 0 || cells >= ram.len() {
            return None;
        }
        let rows = r32(ram, panel + 0xC);
        let display = labelled(ram, panel, (text, count), PROGRESSIVE)?;
        let wide = labelled(ram, panel, (text, count), WIDESCREEN);
        let sliders = g.map(|g| sliders(ram, g, panel)).unwrap_or_default();
        Some(List { panel, rows, display, wide, sliders })
    }

    /// The row the cursor is on.
    fn row(&self, ram: &[u8]) -> u32 {
        r32(ram, self.panel + 4) + r32(ram, self.panel + 8)
    }

    /// The first row's centre, the pitch, the columns' x and widths.
    fn geometry(&self, ram: &[u8]) -> (f32, f32, [f32; 2], [f32; 2]) {
        let p = self.panel;
        (rf(ram, p + 0x50), rf(ram, p + 0x54), [rf(ram, p + 0x20), rf(ram, p + 0x24)], [rf(ram, p + 0x40), rf(ram, p + 0x44)])
    }

    /// The cursor onto row `r` (the panel's window at its top); whether that moved it.
    fn select(&self, ram: &mut [u8], r: u32) -> bool {
        let moved = r32(ram, self.panel + 4) != 0 || r32(ram, self.panel + 8) != r;
        w32(ram, self.panel + 4, 0);
        w32(ram, self.panel + 8, r);
        moved
    }
}

pub struct Menus {
    pub settings: Settings,
    /// The settings page, while it is up.
    page: Option<Page>,
    /// The buttons held at the last field (a press is a button held now and not then), and the presses the port took: the game
    /// does not see them until they are let go (it would see a fresh press on the next field).
    held: u16,
    taken: u16,
    /// Every story level's menu globals (`uyafmt::menucode::all`).
    levels: Vec<MenuGlobals>,
    /// The cursor moves the port makes with a screen's own buttons: the vendor's carousel, QWARK COMICS, a list panel.
    vendor: Steps,
    comics: Steps,
    /// The comic under the pointer at the last field (a comic is asked for when the pointer comes onto it).
    comics_hover: Option<u32>,
    /// The same for a list panel (the place in its window under the pointer).
    list: Steps,
    list_hover: Option<i32>,
    /// Presses the port gives the game this field past the presses it holds back (a Cross due once a cursor arrives).
    inject: u16,
    /// Where the pointer was at the last field (`lines`: the buttons it is over light).
    pointer: Option<(f32, f32)>,
    /// The menus' sound due before this field (`take_sound`): the cursor's when the pointer moved a highlight itself or came onto
    /// one of the port's buttons, a choice's for one made on the port's own rows. The game sounds the steps and the presses it
    /// takes itself; the player, "Only some of the menus have hover sounds".
    sound: Option<u32>,
    /// The port's button under the pointer (a box's answer, a screen's control, a settings tab, a scroll arrow) this field and
    /// the last.
    hot: Option<[f32; 4]>,
    hot_was: Option<[f32; 4]>,
    /// The player's bindings (`bindings.rs`), and whether they changed since `take_bindings`.
    pub bindings: Bindings,
    bindings_changed: bool,
    /// A binding waiting for its input on the KEYBOARD or CONTROLLER tab: the row (an action, a PS2 button) and the slot.
    capture: Option<(usize, usize)>,
    /// A slider of the game's option lists being set by the pointer (`slider`).
    slide: Option<Slide>,
    /// This field's presses as the devices gave them (`input::Presses`), and the controller the CONTROLLER tab names buttons for.
    presses: Presses,
    pad_kind: Device,
}

/// A move of a screen's cursor that the port makes with the screen's own buttons, as the d-pad would, so that the screen's update
/// does everything a step does (its sound, its picture, the items it skips): a press a field down and a field up.
#[derive(Clone, Copy, Default)]
struct Steps {
    /// The item asked for, the cursor when it was asked for, and whether Cross follows once the cursor is there.
    to: Option<(i32, i32, bool)>,
    /// The cursor when the port pressed on the last field (None: no press then).
    pressed: Option<i32>,
}

impl Steps {
    fn ask(&mut self, to: i32, cursor: i32, cross: bool) {
        *self = Steps { to: Some((to, cursor, cross)), pressed: None };
    }

    /// One field: `less` and `more` step the cursor down and up, `ready(up)` says whether the screen takes a step that way now.
    /// The move ends at the item, once the cursor has gone past it (the screen skipped it, or wrapped round), or when a step left
    /// the cursor where it was (the screen offers nothing else that way); returns whether Cross is due now (the cursor arrived and
    /// the click asked for it).
    fn field(&mut self, cursor: i32, pad: &mut Pad, less: u16, more: u16, ready: impl Fn(bool) -> bool) -> bool {
        let Some((to, from, cross)) = self.to else { return false };
        pad.buttons &= !(less | more);
        let up = to > from;
        if cursor == to || self.pressed == Some(cursor) || if up { cursor > to || cursor < from } else { cursor < to || cursor > from } {
            *self = Steps::default();
            return cursor == to && cross;
        }
        self.pressed = if self.pressed.is_none() && ready(up) {
            pad.buttons |= if up { more } else { less };
            Some(cursor)
        } else {
            None
        };
        false
    }
}

impl Menus {
    pub fn new(settings: Settings, bindings: Bindings, levels: Vec<MenuGlobals>) -> Menus {
        Menus {
            settings,
            page: None,
            held: 0,
            taken: 0,
            levels,
            vendor: Steps::default(),
            comics: Steps::default(),
            comics_hover: None,
            list: Steps::default(),
            list_hover: None,
            inject: 0,
            pointer: None,
            sound: None,
            hot: None,
            hot_was: None,
            bindings,
            bindings_changed: false,
            capture: None,
            slide: None,
            presses: Presses::default(),
            pad_kind: Device::Keyboard,
        }
    }

    /// The bindings when the player changed them since the last call.
    pub fn take_bindings(&mut self) -> Option<Bindings> {
        std::mem::take(&mut self.bindings_changed).then_some(self.bindings)
    }

    /// The menus' sound due before this field, if any (`before`): play_ui_sound's id.
    pub fn take_sound(&mut self) -> Option<u32> {
        self.sound.take()
    }

    /// The menus' sound routine of the program in RAM (play_ui_sound(id, 0, 0)): the main program's while the front end runs and
    /// its code is there (its first words: a quit to the title sets the level before the main program is back), the level's
    /// while its code is there (`globals`).
    pub fn ui_sound(&self, ram: &[u8]) -> Option<u32> {
        if Menus::front_end(ram) {
            let a = FRONT_UI_SOUND as usize;
            (r32(ram, a) == UI_SOUND_WORDS[0] && (0..4).all(|k| r32(ram, a + 0xC + 4 * k) == UI_SOUND_WORDS[1 + k])).then_some(FRONT_UI_SOUND)
        } else {
            self.globals(ram).map(|g| g.ui_sound)
        }
    }

    /// The pointer's highlight: `v` into the cursor word at `a`, with the cursor's sound when the highlight moved.
    fn point(&mut self, ram: &mut [u8], a: usize, v: u32) {
        if r32(ram, a) != v {
            w32(ram, a, v);
            self.sound = Some(CURSOR_SOUND);
        }
    }

    /// The menu globals of the level the game is in, while that level's own code is in RAM (a level change loads the next
    /// overlay over the last, and until it has, the addresses hold another level's code and data).
    fn globals(&self, ram: &[u8]) -> Option<&MenuGlobals> {
        let level = r32(ram, crate::LEVEL_CURRENT as usize);
        let g = self.levels.iter().find(|g| g.level as u32 == level)?;
        (r32(ram, g.anchor as usize) == g.anchor_words[0] && r32(ram, g.anchor as usize + 4) == g.anchor_words[1]).then_some(g)
    }

    /// A level's save messages: the addresses of the message word and its frame count (None in the front end, or while the
    /// level's code is not in RAM).
    pub fn save_words(&self, ram: &[u8]) -> Option<(u32, u32)> {
        self.globals(ram).map(|g| (g.saves + 0x40, g.saves + 0x5C))
    }

    /// The pause's SAVE screen's state word while that screen is up (0 while a slot is to be chosen; `play::SAVE_CARD`).
    pub fn save_state(&self, ram: &[u8]) -> Option<u32> {
        let g = self.globals(ram)?;
        (Menus::screen(ram, g) == Some(Screen::Save)).then(|| r32(ram, g.save[0] as usize))
    }

    /// Whether a level's pause list itself takes the input (paused, its record's word `PAUSE_LIST_ACTIVE` set): then the list
    /// panels OPTIONS and its lists set up are not on the screen, whatever their active byte says (OPTIONS leaves panel 0 active,
    /// its rows in place, when it closes; the port stepped that panel's cursor with Up and Down, which the pause list took, and its
    /// highlight ran off the pointer: the player, "if you hover a few menu options up and down the selected row is quickly
    /// missaligned to the place your mouse is", "only starts after you go into options").
    fn pause_list(ram: &[u8], g: &MenuGlobals) -> bool {
        r32(ram, g.mode as usize) == PAUSED && r32(ram, g.pause_record as usize + PAUSE_LIST_ACTIVE) != 0
    }

    /// Whether the game is in its front end.
    fn front_end(ram: &[u8]) -> bool {
        matches!(r32(ram, crate::LEVEL_CURRENT as usize), 0 | u32::MAX)
    }

    /// The game's current mode while a level's own code is in RAM (`MenuGlobals::current`).
    pub fn game_mode(&self, ram: &[u8]) -> Option<u32> {
        self.globals(ram).map(|g| r32(ram, g.current as usize))
    }

    /// Whether the game itself asks for a level in the state it is in: its front end, or a level in play (its current mode 0).
    /// Not in a cutscene (mode 2): entering that mode takes the music's lock (gp-0x4B50: func_L01_004B5C78 from the mode switch)
    /// and only leaving it through the mode switch gives it back (func_L01_004B6188), so a level change made in a cutscene keeps
    /// the lock, and the next level's scenes ask for their streams in vain (func_0039DAE0 drops the request while it is held):
    /// Annihilation Nation asked for Aquatos during its arrival scene, Aquatos's first scene (0x11) then waited for its stream
    /// forever, black. Nor in a menu, the quick select or the ship, from which the game travels through its own screens.
    pub fn asks_levels(&self, ram: &[u8]) -> bool {
        Menus::front_end(ram) || self.game_mode(ram) == Some(PLAY_MODE)
    }

    /// Whether a story level is in play with its own camera (the game's current mode 0, or 4 the quick select over the world):
    /// the field of view setting's views (`settings::FOVS`). Not the front end, a cutscene (2), a menu (3, 5), the ship (6), nor
    /// the vid-comics (levels 30 to 36, the levels with the vid-comic camera, func_L30_003A7A28 and its copies: the comic's
    /// panel framed by its own camera, whose on-screen tests read the game's own tangents).
    pub fn in_play(&self, ram: &[u8]) -> bool {
        !Menus::front_end(ram) && !(30..=36).contains(&r32(ram, crate::LEVEL_CURRENT as usize)) && matches!(self.game_mode(ram), Some(0 | 4))
    }

    /// Whether the game shows a menu: the front end, a level paused, a screen of the pause's family that a level opens from the
    /// world (the battle missions, the galactic map, the ship's QWARK COMICS: the game's current mode 3), or the vendor (mode
    /// 5; `MenuGlobals::current`). The cursor is shown and the mouse points and clicks.
    pub fn menu(&self, ram: &[u8]) -> bool {
        Menus::front_end(ram) || self.globals(ram).is_some_and(|g| r32(ram, g.mode as usize) == PAUSED || MENU_MODES.contains(&r32(ram, g.current as usize)))
    }

    /// Before a field: LOAD GAME on the front end's main menu (item 0 of `MAIN_TABLE`) offered only while the memory card holds
    /// the game's save (`saved`, `uyart::Iop::has_save`; the player, 25 November 2026: "Just make it not show load game when theres
    /// no save file"). With none the game's LOAD GAME checks the card and asks for one that holds a save (memory card message 14:
    /// text 0x195, "Please insert a Memory Card (PS2) in MEMORY CARD slot 1 that contains Ratchet & Clank: Up Your Arsenal game
    /// data", with 0x197 "Go back?" on Triangle, frontbin 0x3B8350 and 0x3B8A40), a box `play::MC_HIDDEN` keeps hidden, so the
    /// menu looked frozen. The item is disabled as the game's menu disables an item (its byte +8 clear: the menu's update, 0x3B4ED8, draws it
    /// grey, Up and Down pass over it, Cross on it does nothing; the pointer passes over it too, `field`), and the highlight moves
    /// off it to NEW GAME. The main menu shows only after the game's first calls of the card, which read its folder.
    pub fn offer_load(&self, ram: &mut [u8], saved: Option<bool>) {
        if !Menus::front_end(ram) || r32(ram, SCREEN) != MAIN_MENU {
            return;
        }
        let saved = saved.expect("the front end's main menu before the game's first call of the memory card");
        ram[MAIN_TABLE + 8] = saved as u8;
        if !saved && r32(ram, MAIN_INDEX) == 0 {
            ram[MAIN_INDEX..MAIN_INDEX + 4].copy_from_slice(&1u32.to_le_bytes());
        }
    }

    /// Before a field: the pointer onto the menu showing (the item under it highlighted; a click off every item is no Cross) and
    /// the Display row's and page's buttons; `presses` what the devices pressed since the last field, `pad_kind` the controller the
    /// CONTROLLER tab names. Returns the settings when the player changed them.
    pub fn before(&mut self, ram: &mut [u8], p: Pointer, pad: &mut Pad, answers: &[Answer], presses: &Presses, pad_kind: Device) -> Option<Settings> {
        self.pointer = p.at;
        (self.presses, self.pad_kind) = (presses.clone(), pad_kind);
        let raw = pad.buttons;
        let fresh = raw & !self.held;
        self.held = raw;
        self.taken &= raw;
        self.inject = 0;
        (self.sound, self.hot) = (None, None);
        let r = self.field(ram, p, pad, fresh, answers);
        // The pointer coming onto one of the port's buttons sounds as a highlight moving does.
        if p.moved && self.hot.is_some() && self.hot != self.hot_was {
            self.sound.get_or_insert(CURSOR_SOUND);
        }
        self.hot_was = self.hot;
        // A press stripped now stays the port's while it is held.
        self.taken |= fresh & !pad.buttons;
        pad.buttons = pad.buttons & !self.taken | self.inject;
        r
    }

    fn field(&mut self, ram: &mut [u8], p: Pointer, pad: &mut Pad, mut fresh: u16, answers: &[Answer]) -> Option<Settings> {
        if !self.menu(ram) {
            (self.page, self.capture, self.slide) = (None, None, None);
            (self.vendor, self.comics, self.comics_hover, self.list, self.list_hover) = (Steps::default(), Steps::default(), None, Steps::default(), None);
            return None;
        }
        // A box offering answers (their buttons are on the frame, `front.rs`): a click on one gives the game the press its glyph
        // stands for, a click off every one does nothing; the screen under the box does not hear the pointer.
        let inside = |a: &&Answer| p.at.is_some_and(|(u, v)| u >= a.rect[0] && u <= a.rect[2] && v >= a.rect[1] && v <= a.rect[3]);
        self.hot = answers.iter().find(inside).map(|a| a.rect);
        // A screen's control (`front.rs`: the arrow or tab the shoulder buttons' names labelled): a click on it is that button.
        // On the settings page it is the page's own press (BACK, NEXT TAB), which the page takes as it takes the button
        // (`list`; while a binding waits, a click anywhere else leaves it).
        if let Some(a) = answers.iter().filter(|a| !a.modal).find(inside) {
            if p.click {
                pad.buttons = pad.buttons & !CROSS | a.button;
                if self.page.is_none() {
                    return None;
                }
                fresh = fresh & !CROSS | a.button;
            }
        }
        let answers: Vec<&Answer> = answers.iter().filter(|a| a.modal).collect();
        if !answers.is_empty() {
            let hit = answers.iter().copied().find(inside);
            match hit {
                Some(a) if p.click => pad.buttons = pad.buttons & !CROSS | a.button,
                None if p.click => pad.buttons &= !CROSS,
                _ => {}
            }
            return None;
        }
        let front = Menus::front_end(ram);
        let g = self.globals(ram).copied();
        if g.as_ref().is_some_and(|g| self.vendor(ram, g, p, pad) || self.comics(ram, g, p, pad) || self.rows(ram, g, p, pad)) {
            return None;
        }
        if let Some(list) = List::find(ram, g.as_ref()) {
            return self.list(ram, list, p, pad, fresh & pad.buttons);
        }
        (self.page, self.capture) = (None, None);
        if let Some((g, screen)) = g.as_ref().and_then(|g| Menus::screen(ram, g).map(|s| (g, s))) {
            self.screen_pointer(ram, g, screen, p, pad);
            return None;
        }
        // Another list (the galactic map's destinations, the battle missions, the vendor's SKINS, HELP's lists): the first active
        // one of the four list panels (0xB4 bytes each; func_003B46F0's panels: +4 the first row shown, +8 the cursor's row among
        // those shown, +0xC the rows, +0x10 the rows shown, func_L01_00494E68 sets them). Its row is moved with Up and Down, as the
        // d-pad would (`Steps`): the screens see a new row only while the panel takes its input (the battle missions and the vendor
        // refresh their picture and item then), so a row written into the panel would leave the screen showing the last one.
        if let Some((gl, panel)) = g.filter(|g| !Menus::pause_list(ram, g)).and_then(|g| (0..4).map(|k| g.panels as usize + 0xB4 * k).find(|&p| ram[p] != 0 && r32(ram, p + 0xB0) != 0).map(|p| (g, p))) {
            let (first, count, window) = (r32(ram, panel + 4) as i32, r32(ram, panel + 0xC) as i32, r32(ram, panel + 0x10) as i32);
            let row = first + r32(ram, panel + 8) as i32;
            // A row is as wide as the panel's highlight bar (centred at +0x7C, +0x84 wide: func_L01_00495868 puts the bar's element
            // there; the battle missions' 0.09 to 0.91, the galactic map's 0.09 to 0.45). The rows the window shows, and the row
            // just above and below it, which a list that scrolls draws cut off at its edges (the vendor's SKINS shows one row).
            let (top, pitch, bar, half) = (rf(ram, panel + 0x50), rf(ram, panel + 0x54), rf(ram, panel + 0x7C), rf(ram, panel + 0x84) / 2.0);
            let hit = p.at.and_then(|(u, v)| (-1..=window).filter(|&k| (0..count).contains(&(first + k))).find(|&k| (v - (top + k as f32 * pitch)).abs() <= pitch / 2.0 && (u - bar).abs() <= half));
            // The pointer coming onto a place in the window highlights the row there (a place, not a row: a window that scrolls as
            // the cursor moves brings another row under the pointer, which is not asked for; one beside the window is not either).
            let inside = hit.filter(|&k| k >= 0 && k < window);
            // A slider being dragged keeps its row (CONTROLS / CAMERA's ROTATION SPEED).
            let dragging = self.slide.is_some() && p.down;
            if p.moved && !dragging && inside != self.list_hover {
                self.list_hover = inside;
                if let Some(r) = inside.map(|k| first + k).filter(|&r| r != row) {
                    self.list.ask(r, row, false);
                }
            }
            // A click on a slider's bar sets it: the cursor goes to its row and the game's own Left and Right take it there.
            let slid = self.slider(ram, &sliders(ram, &gl, panel), bars(ram, panel), row as u32, p, pad);
            if let Some(r) = slid {
                pad.buttons &= !CROSS;
                if r as i32 != row {
                    self.list.ask(r as i32, row, false);
                }
            }
            if p.click && slid.is_none() {
                pad.buttons &= !CROSS;
                match hit.map(|k| first + k) {
                    Some(r) if r == row => {
                        self.list = Steps::default();
                        pad.buttons |= CROSS;
                    }
                    // A row beside the window: the list steps to it; a row in it: chosen once it is highlighted.
                    Some(r) => self.list.ask(r, row, inside.map(|k| first + k) == Some(r)),
                    None => {}
                }
            }
            if self.list.field(row, pad, UP, DOWN, |_| true) {
                self.inject |= CROSS;
            }
            return None;
        }
        (self.list, self.list_hover, self.slide) = (Steps::default(), None, None);
        if front && r32(ram, MC_MESSAGE) == MC_SLOTS {
            // The four slots, each a row across the panel, as a level's (`Screen::Save`); only while a slot is to be chosen (the
            // screen's other states are its confirmations, answered by Cross as they are).
            if r32(ram, FRONT_SLOTS[0]) == 0 {
                let (y0, dy) = (rf(ram, FRONT_SLOTS[2]), rf(ram, FRONT_SLOTS[3]));
                let hit = p.at.and_then(|at| (0..4u32).find(|&k| Cell { x: 0.5, y: y0 + dy * k as f32, hw: 0.42, hh: dy / 2.0 }.has(at)));
                match hit {
                    Some(k) if p.moved || p.click => self.point(ram, FRONT_SLOTS[1], k),
                    None if p.click => pad.buttons &= !CROSS,
                    _ => {}
                }
            }
        } else if front && r32(ram, SCREEN) == MAIN_MENU {
            let hit = p.at.and_then(|(u, v)| {
                (0..MAIN_ITEMS).find(|&i| {
                    let (x, w, y, h) = (rf(ram, MAIN_X), rf(ram, MAIN_W), rf(ram, MAIN_Y + 4 * i), rf(ram, MAIN_H + 4 * i));
                    ram[MAIN_TABLE + 16 * i + 8] != 0 && (u - x).abs() <= w / 2.0 && (v - y).abs() <= h / 2.0
                })
            });
            match hit {
                Some(i) if p.moved || p.click => self.point(ram, MAIN_INDEX, i as u32),
                None if p.click => pad.buttons &= !CROSS,
                _ => {}
            }
        } else if let Some(g) = g.filter(|g| r32(ram, g.mode as usize) == PAUSED && r32(ram, g.pause_table as usize) != 0) {
            let (n, table) = (r32(ram, g.pause_count as usize) as usize, r32(ram, g.pause_table as usize) as usize);
            let hit = p.at.and_then(|(u, v)| (0..n).find(|&i| ram[table + 20 * i + 8] != 0 && (u - 0.502).abs() <= 0.234 && (v - (0.5 - (n as f32 - 1.0) * 0.03 + 0.06 * i as f32)).abs() <= 0.025));
            if let Some(i) = hit.filter(|_| p.moved || p.click) {
                self.point(ram, g.pause_index as usize, i as u32);
            }
        }
        None
    }

    /// The pointer on the vendor (returns whether it took it): on the carousel a click on an item beside the highlighted one
    /// steps the carousel to it, one on the highlighted item is Cross (BUY), one off every item nothing; while it asks
    /// "Purchase?" (its answers are plates on the footer's line, `prompts::FOOTER`) a click off them is nothing. The SKINS page is a
    /// list panel.
    fn vendor(&mut self, ram: &[u8], g: &MenuGlobals, p: Pointer, pad: &mut Pad) -> bool {
        let v = g.vendor as usize;
        if r32(ram, g.current as usize) != VENDOR_MODE || r32(ram, v) != 1 {
            self.vendor = Steps::default();
            return false;
        }
        if r32(ram, v + 0x84) != 0 {
            self.vendor = Steps::default();
            if p.click {
                pad.buttons &= !CROSS;
            }
            return true;
        }
        if !VENDOR_CAROUSEL.contains(&r32(ram, v + 0x50)) {
            self.vendor = Steps::default();
            return false;
        }
        let (scroll, cursor, count) = (r32(ram, v + 0x6C) as i32, r32(ram, v + 0x80) as i32, r32(ram, v + 0x740) as i32);
        if p.click {
            let s = rf(ram, g.vendor_scale as usize);
            let low = if r32(ram, VENDOR_LOW) != 0 { 4.0 } else { 0.0 };
            let (y0, y1) = ((308.0 * s + 0.5).floor() + 1.0 + low, (308.0 * s + 0.5).floor() + 5.0 + low + (38.0 * s + 0.5).floor());
            // Item k's icon from x to x + 38, its alpha 128 less 0.75 a pixel from x 256 at either edge; the band between two
            // icons is shared by both.
            let shown = |x: f32| [x, x + 38.0].iter().map(|e| 128 - ((256.0 - e).abs() * 0.75) as i32).max().expect("two edges") >= 16;
            let hit = p.at.and_then(|(u, w)| {
                let (x, y) = (u * GS_FRAME.0, w * GS_FRAME.1);
                (-5..=6).filter(|k| (0..count).contains(&(cursor + k))).find(|&k| {
                    let left = (238 + 56 * k + scroll) as f32;
                    shown(left) && x >= left - 9.0 && x < left + 47.0 && y >= y0 - 4.0 && y <= y1 + 4.0
                })
            });
            match hit {
                // The highlighted item: the click is Cross.
                Some(0) => self.vendor = Steps::default(),
                Some(k) => {
                    pad.buttons &= !CROSS;
                    self.vendor.ask(cursor + k, cursor, false);
                }
                None => pad.buttons &= !CROSS,
            }
        }
        // A step only while the scroll is under 57 the way it goes.
        self.vendor.field(cursor, pad, LEFT, RIGHT, |up| if up { scroll < 57 } else { scroll > -57 });
        true
    }

    /// The pointer on QWARK COMICS (returns whether it shows): while a comic is to be chosen the comic the pointer comes onto is
    /// stepped to with Up and Down, a click on a comic chooses it (Cross once it is highlighted), a click off every comic nothing.
    /// The comics' boxes are the game's elements 0x11000C + comic.
    fn comics(&mut self, ram: &[u8], g: &MenuGlobals, p: Pointer, pad: &mut Pad) -> bool {
        let [record, cursor, state] = g.comics.map(|a| a as usize);
        if r32(ram, record) != 1 {
            (self.comics, self.comics_hover) = (Steps::default(), None);
            return false;
        }
        // Its question (Yes, CANCEL: plates): a click off them is nothing.
        if r32(ram, state) != 0 {
            (self.comics, self.comics_hover) = (Steps::default(), None);
            if p.click {
                pad.buttons &= !CROSS;
            }
            return true;
        }
        let cursor = r32(ram, cursor) as i32;
        let hit = p.at.and_then(|at| (0..COMICS).find(|&k| element(ram, g.ui, COMIC_BOX + k).is_some_and(|c| c.has(at))));
        if p.moved && hit != self.comics_hover {
            self.comics_hover = hit;
            if let Some(k) = hit.filter(|&k| k as i32 != cursor) {
                self.comics.ask(k as i32, cursor, false);
            }
        }
        if p.click {
            pad.buttons &= !CROSS;
            match hit {
                Some(k) if k as i32 == cursor => {
                    self.comics = Steps::default();
                    pad.buttons |= CROSS;
                }
                Some(k) => self.comics.ask(k as i32, cursor, true),
                None => {}
            }
        }
        if self.comics.field(cursor, pad, UP, DOWN, |_| true) {
            self.inject |= CROSS;
        }
        true
    }

    /// The pointer on the pause family's screen 5 (a vid-comic's pause; returns whether it shows): the row under the pointer is
    /// highlighted when it can be chosen, a click on it chooses it, a click off every row nothing. The rows are where the screen
    /// puts its highlight (y 0.5 + (row - 1) * pitch), as wide as the highlight's element.
    fn rows(&mut self, ram: &mut [u8], g: &MenuGlobals, p: Pointer, pad: &mut Pad) -> bool {
        let [record, cursor, table, pitch] = g.rows.map(|a| a as usize);
        if r32(ram, record) != 1 {
            return false;
        }
        let pitch = rf(ram, pitch);
        let Some(bar) = element(ram, g.ui, ROWS_BAR) else { return true };
        let hit = p.at.and_then(|at| (0..3u32).find(|&r| ram[table + 16 * r as usize + 8] != 0 && Cell { x: bar.x, y: 0.5 + (r as f32 - 1.0) * pitch, hw: bar.hw, hh: pitch / 2.0 }.has(at)));
        match hit {
            Some(r) if p.moved || p.click => self.point(ram, cursor, r),
            None if p.click => pad.buttons &= !CROSS,
            _ => {}
        }
        true
    }

    /// The pause screen showing, if it is one with pointer targets: the record whose first word is 1.
    fn screen(ram: &[u8], g: &MenuGlobals) -> Option<Screen> {
        [Screen::Missions, Screen::Weapons, Screen::Gadgets, Screen::Quick, Screen::Items, Screen::Save].into_iter().zip(g.screens).find(|&(_, at)| r32(ram, at as usize) == 1).map(|(s, _)| s)
    }

    /// The cells of `screen` with the words each sets (address, value): the cursor the d-pad would have moved there.
    fn cells(ram: &[u8], g: &MenuGlobals, screen: Screen) -> Vec<(Cell, Vec<(usize, u32)>)> {
        let f = |a: u32| rf(ram, a as usize);
        let w = |a: u32| a as usize;
        // The weapons grid's columns and rows (read from the level's RAM): 10 columns, 2 rows.
        let (x0, dx, y0, y1) = (f(g.weapons[2]), f(g.weapons[3]), f(g.weapons[4]), f(g.weapons[5]));
        let mut cells = Vec::new();
        match screen {
            Screen::Weapons => {
                for (r, y) in [y0, y1].into_iter().enumerate() {
                    for c in 0..10 {
                        let cursor = vec![(w(g.weapons[0]), c), (w(g.weapons[1]), r as u32)];
                        cells.push((Cell { x: x0 + dx * c as f32, y, hw: dx / 2.0, hh: (y1 - y0) / 2.0 }, cursor));
                    }
                }
            }
            Screen::Gadgets => {
                let (gx, gy, gdx, gdy) = (f(g.gadgets[2]), f(g.gadgets[3]), f(g.gadgets[4]), f(g.gadgets[5]));
                // Row 0 has 5 cells, row 1 four, set half a step to the right.
                for r in 0..2 {
                    for c in 0..5 - r {
                        let cursor = vec![(w(g.gadgets[0]), c), (w(g.gadgets[1]), r)];
                        cells.push((Cell { x: gx + gdx * (c as f32 + 0.5 * r as f32), y: gy + gdy * r as f32, hw: gdx / 2.0, hh: gdy / 2.0 }, cursor));
                    }
                }
            }
            Screen::Items => {
                for (r, y) in ITEM_ROWS.into_iter().enumerate() {
                    for c in 0..5 {
                        cells.push((Cell { x: x0 + dx * c as f32, y, hw: dx / 2.0, hh: (ITEM_ROWS[1] - ITEM_ROWS[0]) / 2.0 }, vec![(w(g.items), (r * 5 + c) as u32)]));
                    }
                }
            }
            Screen::Quick => {
                // The weapons grid (5 columns, 4 rows) and the gadgets' (4 columns, 2 rows): the cursor is (column, row, which).
                for (r, y) in QUICK_ROWS.into_iter().enumerate() {
                    for c in 0..5 {
                        let cursor = vec![(w(g.quick[0]), c as u32), (w(g.quick[1]), r as u32), (w(g.quick[2]), 0)];
                        cells.push((Cell { x: x0 + dx * c as f32, y, hw: dx / 2.0, hh: (QUICK_ROWS[1] - QUICK_ROWS[0]) / 2.0 }, cursor));
                    }
                }
                let (qx, qdx, rows) = QUICK_GADGETS;
                for (r, y) in rows.into_iter().enumerate() {
                    for c in 0..4 {
                        let cursor = vec![(w(g.quick[0]), c as u32), (w(g.quick[1]), r as u32), (w(g.quick[2]), 1)];
                        cells.push((Cell { x: qx + qdx * c as f32, y, hw: qdx / 2.0, hh: (rows[1] - rows[0]) / 2.0 }, cursor));
                    }
                }
            }
            // The four slots, each a row across the panel (the cursor sits on the right half of it); only while a slot is to be
            // chosen (the screen's other states are its confirmations, answered by Cross as they are).
            Screen::Save if r32(ram, g.save[0] as usize) == 0 => {
                let (y, dy) = (f(g.save[2]), f(g.save[3]));
                for k in 0..4u32 {
                    cells.push((Cell { x: 0.5, y: y + dy * k as f32, hw: 0.42, hh: dy / 2.0 }, vec![(w(g.save[1]), k)]));
                }
            }
            // The list under the mission's picture: up to three rows are shown (the level's words: the highlighted row of the
            // three, the scroll and the number of missions).
            Screen::Missions => {
                let (scroll, count) = (r32(ram, g.missions[1] as usize), r32(ram, g.missions[2] as usize));
                for r in (0..3u32).filter(|r| scroll + r < count) {
                    cells.push((Cell { x: 0.5, y: MISSION_ROWS.0 + MISSION_ROWS.1 * r as f32, hw: 0.4, hh: MISSION_ROWS.1 / 2.0 }, vec![(w(g.missions[0]), r)]));
                }
            }
            Screen::Save => {}
        }
        cells
    }

    /// The pointer on a pause screen: the cell under it becomes the cursor (when the pointer moved or clicked), and a click off
    /// every cell is no Cross.
    fn screen_pointer(&mut self, ram: &mut [u8], g: &MenuGlobals, screen: Screen, p: Pointer, pad: &mut Pad) {
        if screen == Screen::Save && r32(ram, g.save[0] as usize) != 0 {
            return;
        }
        // The ring: a click on a slot highlights it (the ring turns) and is not a Cross (Cross would put the grid's item in it).
        if screen == Screen::Quick {
            let s = r32(ram, g.quick_ring as usize);
            let slot = p.at.and_then(|at| {
                (0..8u32)
                    .find(|&k| {
                        let a = k as f32 * std::f32::consts::FRAC_PI_4;
                        Cell { x: RING.0 .0 + RING.1 .0 * a.sin(), y: RING.0 .1 - RING.1 .1 * a.cos(), hw: RING.2 .0, hh: RING.2 .1 }.has(at)
                    })
                    .map(|k| (s + k) % 8)
            });
            if let Some(slot) = slot {
                if p.moved || p.click {
                    self.point(ram, g.quick_ring as usize, slot);
                }
                if p.click {
                    pad.buttons &= !CROSS;
                }
                return;
            }
        }
        let hit = p.at.and_then(|at| Menus::cells(ram, g, screen).into_iter().find(|(c, _)| c.has(at)));
        match hit {
            Some((_, cursor)) if p.moved || p.click => {
                let changed = cursor.iter().any(|&(a, v)| r32(ram, a) != v);
                for (a, v) in cursor {
                    w32(ram, a, v);
                }
                if changed {
                    self.sound = Some(CURSOR_SOUND);
                }
                // The weapons screen refreshes its texts when told the highlight moved.
                if changed && screen == Screen::Weapons {
                    ram[g.weapons[6] as usize] = 1;
                }
            }
            None if p.click => pad.buttons &= !CROSS,
            _ => {}
        }
    }

    /// An option list with a PC SETTINGS row: the pointer on its rows, the PC SETTINGS row's and the 16:9 row's buttons, the
    /// settings page.
    fn list(&mut self, ram: &mut [u8], list: List, p: Pointer, pad: &mut Pad, fresh: u16) -> Option<Settings> {
        // R1 moves the PS2's picture on the TV (the front end's Options).
        pad.buttons &= !R1;
        let (top, pitch, x, w) = list.geometry(ram);
        let row_at = |count: u32, right: f32| p.at.and_then(|(u, v)| (0..count).find(|&r| (v - (top + r as f32 * pitch)).abs() <= pitch / 2.0 && u >= x[0] - 0.03 && u <= right));
        let Some(mut page) = self.page else {
            // A slider being dragged keeps its row (a pause's AUDIO / VISUAL volumes).
            let dragging = self.slide.is_some() && p.down;
            match row_at(list.rows, x[1] + 0.03) {
                Some(r) if (p.moved || p.click) && !dragging => {
                    if list.select(ram, r) {
                        self.sound = Some(CURSOR_SOUND);
                    }
                }
                None if p.click => pad.buttons &= !CROSS,
                _ => {}
            }
            // A click on a slider's bar sets it (the row is the cursor's now): the game's own Left and Right take it there.
            if self.slider(ram, &list.sliders, bars(ram, list.panel), list.row(ram), p, pad).is_some() {
                pad.buttons &= !CROSS;
            }
            let row = list.row(ram);
            if row == list.display {
                // The game's PROGRESSIVE SCAN row: its buttons are the port's.
                pad.buttons &= !(CROSS | LEFT | RIGHT);
                if fresh & CROSS != 0 {
                    self.page = Some(Page { tab: 0, scroll: 0 });
                    list.select(ram, 0);
                    self.sound = Some(CHOICE_SOUND);
                }
            } else if Some(row) == list.wide {
                // The game's 16:9 row: the port's WIDESCREEN, which Cross (a click), Left and Right change.
                pad.buttons &= !(CROSS | LEFT | RIGHT);
                if fresh & (CROSS | LEFT | RIGHT) != 0 {
                    let i = crate::settings::index("widescreen");
                    self.settings.values[i] = (self.settings.values[i] + 1) % OPTS[i].values.len() as u8;
                    self.sound = Some(CHOICE_SOUND);
                    return Some(self.settings);
                }
            }
            return None;
        };
        // The page: the game sees no button that changes or leaves its list.
        self.slide = None;
        // R1 changes the page's tab: the game's own options screen saw it too (the front end's ADJUST SCREEN, a pause's next page).
        pad.buttons &= !(CROSS | LEFT | RIGHT | TRIANGLE | START | SELECT | L1 | L2 | R1 | R2);
        let value_right = |scrolls: bool| if scrolls { x[1] - SCROLL.2 } else { x[1] };
        // A binding waiting for its input: the game and the page get nothing else meanwhile.
        if let Some(c) = self.capture {
            *pad = CENTRE;
            let n = items(TABS[page.tab].0).len() as u32;
            let shown = n.min(list.rows - 1);
            let row = list.row(ram).min(shown - 1);
            let keyboard = TABS[page.tab].0 == Tab::Keyboard;
            let slot = slot_rects(top, pitch, row, (x[1] - w[1], value_right(n > shown)), if keyboard { 2 } else { 1 })[c.1];
            let clear = clear_rect(top + (list.rows - 1) as f32 * pitch, pitch, value_right(n > shown));
            self.bind(c, keyboard, slot, clear, p);
            return None;
        }
        if fresh & (TRIANGLE | START) != 0 {
            self.page = None;
            list.select(ram, list.display);
            self.sound = Some(CHOICE_SOUND);
            return None;
        }
        let click = p.at.filter(|_| p.click);
        // The tab buttons light under the pointer (`lines`).
        if let Some(k) = p.at.and_then(|at| (0..TABS.len()).find(|&k| inside(tab_rect(k), at))) {
            self.hot = Some(tab_rect(k));
        }
        // Another tab: a click on its button in the tab bar, or a shoulder button.
        let to = match click.and_then(|at| (0..TABS.len()).find(|&k| inside(tab_rect(k), at))) {
            Some(k) => Some(k),
            // R1 and R2, or Select: the keyboard's Tab (MAP), as no key presses R1 or R2 in the keyboard's layout.
            None if fresh & (R1 | R2 | SELECT) != 0 => Some((page.tab + 1) % TABS.len()),
            None if fresh & (L1 | L2) != 0 => Some((page.tab + TABS.len() - 1) % TABS.len()),
            None => None,
        };
        if let Some(k) = to {
            if k != page.tab {
                self.page = Some(Page { tab: k, scroll: 0 });
                list.select(ram, 0);
                self.sound = Some(CHOICE_SOUND);
            }
            return None;
        }
        let rows = items(TABS[page.tab].0);
        let n = rows.len() as u32;
        let shown = n.min(list.rows - 1);
        let scrolls = n > shown;
        // The scroll bar and the wheel move the rows the panel shows; the cursor keeps its place in the panel.
        if scrolls {
            let [up, down, track] = scroll_rects(top, pitch, shown, x[1]);
            let thumb = thumb_rect(track, page.scroll, shown, n);
            if let Some(r) = p.at.and_then(|at| [up, down].into_iter().find(|&r| inside(r, at))) {
                self.hot = Some(r);
            }
            let mut by = -p.wheel;
            match click {
                Some(at) if inside(up, at) => by -= 1,
                Some(at) if inside(down, at) => by += 1,
                Some(at) if inside(track, at) && at.1 < thumb[1] => by -= shown as i32,
                Some(at) if inside(track, at) && at.1 > thumb[3] => by += shown as i32,
                _ => {}
            }
            let was = page.scroll;
            page.scroll = (page.scroll as i32 + by).clamp(0, (n - shown) as i32) as u32;
            if page.scroll != was {
                self.sound = Some(CURSOR_SOUND);
            }
        }
        // The binding rows' slots light under the pointer (`lines`).
        let slots = |scroll: u32, r: u32| match rows.get((scroll + r) as usize) {
            Some(Item::Key(_)) => slot_rects(top, pitch, r, (x[1] - w[1], value_right(scrolls)), 2),
            Some(Item::Pad(_)) => slot_rects(top, pitch, r, (x[1] - w[1], value_right(scrolls)), 1),
            _ => Vec::new(),
        };
        if let Some(r) = p.at.and_then(|at| (0..shown).flat_map(|r| slots(page.scroll, r)).find(|&r| inside(r, at))) {
            self.hot = Some(r);
        }
        // The rows: the one under the pointer (left of the scroll bar) is highlighted; a click elsewhere changes nothing.
        let hit = row_at(shown, if scrolls { x[1] - SCROLL.0 - 0.002 } else { x[1] + 0.03 });
        if hit.filter(|_| p.moved || p.click).is_some_and(|r| list.select(ram, r)) {
            self.sound = Some(CURSOR_SOUND);
        }
        // Up and down move the game's cursor (its sound with it) inside the panel; at the panel's edge the page scrolls, and past
        // the tab's last row (or before its first) the cursor wraps round, with the cursor's sound the game does not make then.
        let row = list.row(ram).min(shown - 1);
        let sel = page.scroll + row;
        if fresh & DOWN != 0 {
            if sel + 1 >= n {
                pad.buttons &= !DOWN;
                page.scroll = 0;
                list.select(ram, 0);
            } else if row + 1 >= shown {
                pad.buttons &= !DOWN;
                page.scroll += 1;
            }
        } else if fresh & UP != 0 {
            if sel == 0 {
                pad.buttons &= !UP;
                page.scroll = n - shown;
                list.select(ram, shown - 1);
            } else if row == 0 {
                pad.buttons &= !UP;
                page.scroll -= 1;
            }
        }
        if fresh & (UP | DOWN) != 0 && pad.buttons & (UP | DOWN) == 0 {
            self.sound = Some(CURSOR_SOUND);
        }
        self.page = Some(page);
        let sel = page.scroll + list.row(ram).min(shown - 1);
        // A click on the highlighted row: on the left half of its value (its left arrow) the value before, anywhere else on the
        // row the next. Cross from the keyboard or a pad is the next; a click off the rows is nothing.
        let on_row = hit == Some(list.row(ram).min(shown - 1));
        // A binding row: a click on a slot waits for the input to put there, and so do Cross (the first slot), Left (the first) and
        // Right (the second, on the KEYBOARD tab).
        if let Item::Key(a) | Item::Pad(a) = rows[sel as usize] {
            let rects = slots(page.scroll, list.row(ram).min(shown - 1));
            let slot = match click {
                Some(at) => rects.iter().position(|&r| inside(r, at)),
                None if fresh & (CROSS | LEFT) != 0 => Some(0),
                None if fresh & RIGHT != 0 && rects.len() == 2 => Some(1),
                None => None,
            };
            if let Some(slot) = slot {
                self.capture = Some((a, slot));
                self.sound = Some(CHOICE_SOUND);
            }
            return None;
        }
        let value_right = value_right(scrolls);
        let arrows = arrow_rects(top, pitch, list.row(ram).min(shown - 1), value_right);
        if let Some(r) = p.at.and_then(|at| arrows.into_iter().find(|&r| inside(r, at))) {
            self.hot = Some(r);
        }
        let back = click.is_some_and(|at| on_row && inside(arrows[0], at));
        let next = fresh & RIGHT != 0 || (fresh & CROSS != 0 && (!p.click || on_row));
        let step: i32 = if back || fresh & LEFT != 0 {
            -1
        } else if next {
            1
        } else {
            return None;
        };
        let s = &mut self.settings;
        match rows[sel as usize] {
            Item::Preset => {
                let k = PRESETS.len() as i32;
                // From CUSTOM, Right takes ORIGINAL and Left ULTRA.
                let at = s.preset().map_or(if step > 0 { -1 } else { k }, |p| p as i32);
                s.set_preset((at + step).rem_euclid(k) as usize);
            }
            Item::Opt(i) => {
                let k = OPTS[i].values.len() as i32;
                s.values[i] = (s.values[i] as i32 + step).rem_euclid(k) as u8;
            }
            // The reset takes Cross or a click anywhere on it only; KEYBOARD's and CONTROLLER's put the bindings back.
            Item::Reset if fresh & CROSS != 0 => match TABS[page.tab].0 {
                Tab::Keyboard | Tab::Controller => {
                    if TABS[page.tab].0 == Tab::Keyboard {
                        self.bindings.reset_keys();
                    } else {
                        self.bindings.reset_pad();
                    }
                    (self.bindings_changed, self.sound) = (true, Some(CHOICE_SOUND));
                    return None;
                }
                tab => s.reset(tab),
            },
            Item::Reset => return None,
            Item::Key(_) | Item::Pad(_) => unreachable!("a binding row is handled above"),
        }
        // The ray-traced options only with DLSS on (`Settings::constrain`).
        s.constrain();
        self.sound = Some(CHOICE_SOUND);
        Some(self.settings)
    }

    /// The pointer on the sliders of an option list (`sliders`, the panel's rows and bars `b`, the game's cursor on row `cursor`):
    /// a click on a slider's bar starts taking it to the value under the pointer, and while the button is held the value follows
    /// the pointer. The port presses the game's own Right or Left, a field apart, until the value is the step nearest to it, as the
    /// d-pad would (the game applies the value, clicks and clamps), once the cursor is on the slider's row; the slider is let go
    /// when it gets there, when a press leaves the value where it was (an end of the bar), or when the cursor goes to another row.
    /// Returns the row of the slider a click started (its Cross is the slider's, not the game's).
    fn slider(&mut self, ram: &[u8], sliders: &[Slider], b: Bars, cursor: u32, p: Pointer, pad: &mut Pad) -> Option<u32> {
        let value_at = |s: &Slider, u: f32| (((u - b.l - s.pad) / (b.r - b.l - s.span)).clamp(0.0, 1.0) * s.max as f32).round() as u32;
        let band = |s: &Slider| b.top + (s.row as f32 - b.first as f32) * b.pitch;
        let hit = p.at.filter(|_| p.click).and_then(|(u, v)| sliders.iter().find(|s| s.row >= b.first && (v - band(s)).abs() <= b.pitch / 2.0 && u >= b.l - 0.02 && u <= b.r + 0.02).map(|s| (*s, u)));
        if let Some((s, u)) = hit {
            self.slide = Some(Slide { row: s.row, word: s.word, step: s.step, to: value_at(&s, u), pressed: None, held: true });
            return Some(s.row);
        }
        let mut sl = self.slide?;
        let Some(s) = sliders.iter().find(|s| s.row == sl.row && s.word == sl.word) else {
            self.slide = None;
            return None;
        };
        if let Some((u, _)) = p.at.filter(|&(_, v)| p.down || sl.held && (v - band(s)).abs() <= b.pitch / 2.0) {
            sl.to = value_at(s, u);
        }
        sl.held = p.down;
        if cursor != sl.row {
            // The cursor on its way to the slider's row (`Steps`), or gone to another one.
            self.slide = self.list.to.is_some().then_some(sl);
            return None;
        }
        let value = r32(ram, sl.word);
        match sl.pressed {
            // The field after a press has none, so that the next is a press again.
            Some(was) => {
                sl.pressed = None;
                if value == was && !p.down {
                    self.slide = None;
                    return None;
                }
            }
            None if value + sl.step / 2 < sl.to => {
                pad.buttons = pad.buttons & !LEFT | RIGHT;
                sl.pressed = Some(value);
            }
            None if value > sl.to + sl.step / 2 => {
                pad.buttons = pad.buttons & !RIGHT | LEFT;
                sl.pressed = Some(value);
            }
            None if !p.down => {
                self.slide = None;
                return None;
            }
            None => {}
        }
        self.slide = Some(sl);
        None
    }

    /// A field of a binding waiting for its input (`capture`: the row and slot; `keyboard`: the KEYBOARD tab's, else the
    /// CONTROLLER's; `slot` its button, `clear` CLEAR's). On the KEYBOARD tab the next key or mouse button pressed is bound to the
    /// slot (a click on the slot is the left button), Delete or a click on CLEAR empties it; on the CONTROLLER tab the next
    /// controller button is bound to the PS2 button. Esc, or a click anywhere else, leaves it as it was.
    fn bind(&mut self, (row, slot): (usize, usize), keyboard: bool, slot_rect: [f32; 4], clear: [f32; 4], p: Pointer) {
        let pressed = |k: KeyCode| self.presses.keys.contains(&Key::Key(k));
        if keyboard {
            self.hot = p.at.and_then(|at| [slot_rect, clear].into_iter().find(|&r| inside(r, at)));
        }
        let click = p.at.filter(|_| p.click);
        let done = if pressed(KeyCode::Escape) {
            true
        } else if keyboard {
            match click {
                Some(at) if inside(slot_rect, at) => {
                    self.bindings.set_key(row, slot, Key::Mouse(0));
                    self.bindings_changed = true;
                    true
                }
                Some(at) if inside(clear, at) => {
                    self.bindings.clear(row, slot);
                    self.bindings_changed = true;
                    true
                }
                Some(_) => true,
                None if pressed(KeyCode::Delete) => {
                    self.bindings.clear(row, slot);
                    self.bindings_changed = true;
                    true
                }
                None => match self.presses.keys.iter().copied().find(|&k| k != Key::Mouse(0) && k.bindable()) {
                    Some(k) => {
                        self.bindings.set_key(row, slot, k);
                        self.bindings_changed = true;
                        true
                    }
                    None => false,
                },
            }
        } else if click.is_some() {
            true
        } else if self.presses.pad != 0 {
            // The lowest PS2 button of those pressed (two at once: one of them).
            self.bindings.set_pad(row, self.presses.pad & self.presses.pad.wrapping_neg());
            self.bindings_changed = true;
            true
        } else {
            false
        };
        if done {
            self.capture = None;
            self.sound = Some(CHOICE_SOUND);
            // The input just bound (or Esc, or the click) is still down: from the next field it presses its new button, which is
            // not fresh, and neither the page nor the game sees it until it is let go.
            (self.held, self.taken) = (u16::MAX, u16::MAX);
        }
    }

    /// After a field: the settings page's tab bar while the page shows, a button a tab (`TAB_BAR`), for the window to draw over
    /// the finished picture as it draws the FPS counter (`play.rs` `Screen::tab_bar`). Placed among the game's primitives as the
    /// page's other lines are, it took its look from the game's letters, its place from theirs and the GS state they were drawn
    /// in, and the tabs went unseen however that was arranged (the player: they "dont appear until you press the controller button
    /// that makes you switch between them", "There is no buttons to change tab on keyboard and mouse and nothing indicates there
    /// are other tabs", and after two fixes of that kind, "the tabs still dont appear in PC settings menu").
    pub fn tab_bar(&self, ram: &[u8]) -> Vec<TabShown> {
        let Some(page) = self.page.filter(|_| self.menu(ram) && List::find(ram, self.globals(ram)).is_some()) else { return Vec::new() };
        let lit = |r: [f32; 4], on: bool| if on { Plate::On } else if self.pointer.is_some_and(|a| inside(r, a)) { Plate::Lit } else { Plate::Off };
        TABS.iter().enumerate().map(|(k, &(_, name))| TabShown { rect: tab_rect(k), name, look: lit(tab_rect(k), k == page.tab) }).collect()
    }

    /// After a field: the port's text on an option list with a PC SETTINGS row (that row and the 16:9 row's value, or the
    /// settings page with its scroll bar; its tab bar is the window's, `tab_bar`).
    pub fn lines(&self, ram: &[u8]) -> Vec<Line> {
        if !self.menu(ram) {
            return Vec::new();
        }
        let g = self.globals(ram);
        let Some(list) = List::find(ram, g) else { return Vec::new() };
        let s = self.settings;
        let preset = s.preset().map_or(crate::settings::CUSTOM, |p| PRESETS[p]);
        let (top, pitch, x, w) = list.geometry(ram);
        let label_band = (x[0] - 0.02, x[0] + w[0]);
        let value_band = (x[1] - w[1] - 0.02, x[1] + 0.02);
        let Some(page) = self.page else {
            let y = top + list.display as f32 * pitch;
            let mut v = vec![Line::game(y, pitch, label_band, "PC SETTINGS:", Align::Left, false), Line::game(y, pitch, value_band, &format!("{}P, {preset}", s.height()), Align::Right, false)];
            // The 16:9 row reads the port's WIDESCREEN.
            if let Some(r) = list.wide {
                let i = crate::settings::index("widescreen");
                v.push(Line::game(top + r as f32 * pitch, pitch, value_band, OPTS[i].values[s.values[i] as usize].1, Align::Right, false));
            }
            // The front end's footer: R1's ADJUST SCREEN moved the PS2's picture on the TV, which a PC does not have (the port
            // takes R1 here): not drawn.
            if g.is_none() {
                let (fy, fh, fx0, fx1) = FOOTER_R1;
                v.push(Line::game(fy, fh, (fx0, fx1), "", Align::Left, true));
            }
            return v;
        };
        let rows = items(TABS[page.tab].0);
        let n = rows.len() as u32;
        let shown = n.min(list.rows - 1);
        let scrolls = n > shown;
        let row = list.row(ram).min(shown - 1);
        let sel = rows[(page.scroll + row) as usize];
        let lit = |r: [f32; 4], on: bool| if on { Plate::On } else if self.pointer.is_some_and(|a| inside(r, a)) { Plate::Lit } else { Plate::Off };
        // The title plate: the page's name, in the title's letters. The tab bar under it is pushed after the rows (below).
        let mut v = vec![Line::game(TITLE.0, TITLE.1, (0.2, 0.8), "PC SETTINGS", Align::Centre, false)];
        // Each row the panel shows: its label in place of the row's own, its value at the value column's right edge (left of the
        // scroll bar when there is one; in the row's letters' style, or the label's beside a slider) between arrows on the
        // highlighted row; the rows past the tab's last emptied (their letters and sliders: the panel's art stays).
        let value_right = if scrolls { x[1] - SCROLL.2 } else { x[1] };
        for r in 0..list.rows - 1 {
            let y = top + r as f32 * pitch;
            let item = rows.get((page.scroll + r) as usize).copied();
            let (label, value) = match item {
                Some(Item::Preset) => ("QUALITY PRESET", Some(preset)),
                // FRAME GENERATION where it cannot run this session (`streamline::status`): said so in place of its value.
                Some(Item::Opt(i)) if OPTS[i].key == "frame_generation" && matches!(crate::streamline::status(), crate::streamline::Status::Unavailable(_)) => (OPTS[i].name, Some("UNAVAILABLE")),
                // DLSS where this PC cannot run it (`dlss::unavailable`): kept off (`Settings::constrain`), said so.
                Some(Item::Opt(i)) if OPTS[i].key == "dlss" && crate::dlss::unavailable().is_some() => (OPTS[i].name, Some("UNAVAILABLE")),
                Some(Item::Opt(i)) => (OPTS[i].name, Some(OPTS[i].values[s.values[i] as usize].1)),
                Some(Item::Key(a)) => (bindings::ACTIONS[a].name, None),
                Some(Item::Pad(b)) => (bindings::PAD[b].1, None),
                Some(Item::Reset) => ("RESET TO DEFAULTS", None),
                None => ("", None),
            };
            v.push(Line::game(y, pitch, label_band, label, Align::Left, false));
            v.push(Line::game(y, pitch, value_band, value.unwrap_or(""), Align::Right, true).at(value_right - 2.0 * (ARROW_W + ARROW_GAP), false, 1.0));
            // The highlighted option's arrows.
            if r == row && matches!(item, Some(Item::Preset | Item::Opt(_))) {
                let [left, right] = arrow_rects(top, pitch, r, value_right);
                v.push(Line::button(left, "", (lit(left, false), Shape::Left), 1.0));
                v.push(Line::button(right, "", (lit(right, false), Shape::Right), 1.0));
            }
            // A binding row's slots: what each holds, the one waiting for its input saying so.
            let slots: Vec<(usize, String)> = match item {
                Some(Item::Key(a)) => (0..2).map(|k| (a, self.bindings.keys[a][k].map_or(String::new(), |key| key.label().to_string()))).collect(),
                Some(Item::Pad(b)) => vec![(b, bindings::pad_name(self.pad_kind, self.bindings.pad[b]).to_string())],
                _ => Vec::new(),
            };
            let rects = slot_rects(top, pitch, r, (x[1] - w[1], value_right), slots.len());
            for (k, ((at, name), rect)) in slots.into_iter().zip(rects).enumerate() {
                let waiting = self.capture == Some((at, k));
                let text = match (waiting, item) {
                    (true, Some(Item::Key(_))) => "PRESS A KEY".to_string(),
                    (true, _) => "PRESS A BUTTON".to_string(),
                    (false, _) => name,
                };
                v.push(Line::button(rect, &text, (lit(rect, waiting), Shape::Box), SLOT_SIZE).prose());
            }
        }
        // The highlighted row's description in the panel's last row, in one or two lines; while a binding waits for its input,
        // what to press, and on the KEYBOARD tab CLEAR beside it.
        let about = match (self.capture, sel) {
            (Some((a, _)), Item::Key(_)) => format!("Press the key or mouse button for {} (clicking its slot: the left button). Esc cancels.", bindings::ACTIONS[a].name),
            (Some((b, _)), _) => format!("Press the controller button for {}: it swaps with the one there. Esc or a click leaves it.", bindings::PAD[b].1),
            (None, Item::Preset) => "Sets every graphics option at once. Original is the game as it was on the PS2; Ultra is every option at its highest.".to_string(),
            // FRAME GENERATION: why it cannot run on this PC, when it cannot.
            (None, Item::Opt(i)) if OPTS[i].key == "frame_generation" => match crate::streamline::status() {
                crate::streamline::Status::Unavailable(why) => format!("Frame generation cannot run on this PC: {why}"),
                _ => OPTS[i].about.to_string(),
            },
            // DLSS: why it cannot run on this PC, when it cannot.
            (None, Item::Opt(i)) if OPTS[i].key == "dlss" && crate::dlss::unavailable().is_some() => format!("DLSS cannot run on this PC: {}.", crate::dlss::unavailable().expect("checked")),
            (None, Item::Opt(i)) => OPTS[i].about.to_string(),
            (None, Item::Key(_)) => "Click a slot, then press the key or mouse button to put there. A key put in a slot leaves the slot it was in.".to_string(),
            (None, Item::Pad(_)) => "Click the button, then press the controller button that is to press this one; the two swap places.".to_string(),
            (None, Item::Reset) => match TABS[page.tab].0 {
                Tab::Display => "Puts the display options back to windowed at 1080p, widescreen on.",
                Tab::Graphics => "Puts every graphics option back to off: the Original preset, the game as it was.",
                Tab::Controls => "Puts the camera back to the game's own: its field of view, speed and directions.",
                Tab::Keyboard => "Puts every key and mouse button back to the port's own layout.",
                Tab::Controller => "Puts every controller button back to the game's own.",
                Tab::Gameplay => "Puts the gameplay options back to the game's own.",
                Tab::Audio => "Puts the sound back to the game's own mix.",
            }
            .to_string(),
        };
        let y = top + (list.rows - 1) as f32 * pitch;
        v.push(Line::game(y, pitch, label_band, "", Align::Left, false));
        v.push(Line::game(y, pitch, value_band, "", Align::Right, true));
        let clear = (self.capture.is_some() && TABS[page.tab].0 == Tab::Keyboard).then(|| clear_rect(y, pitch, value_right));
        let (about_x, about_width) = match clear {
            Some(c) => ((x[0] + c[0]) / 2.0, ABOUT_WIDTH * 5 / 6),
            None => ((x[0] + x[1]) / 2.0, ABOUT_WIDTH),
        };
        let lines = wrap(&about, about_width);
        for (k, text) in lines.iter().enumerate() {
            let dy = (k as f32 - (lines.len() - 1) as f32 / 2.0) * 2.0 * ABOUT.0;
            v.push(Line::game(y + dy * pitch, pitch * ABOUT.0, (0.0, 0.0), text, Align::Centre, false).at(about_x, false, ABOUT.1).prose());
        }
        if let Some(c) = clear {
            v.push(Line::button(c, "CLEAR", (lit(c, false), Shape::Box), SLOT_SIZE).prose());
        }
        // In the front end's footer, R1's label: it changes tab here.
        if g.is_none() {
            let (fy, fh, fx0, fx1) = FOOTER_R1;
            v.push(Line::game(fy, fh, (fx0, fx1), "NEXT TAB", Align::Left, false));
        }
        // The tab bar is the window's to draw over the picture (`tab_bar`), not a line among the game's primitives.
        // The scroll bar, over the rows' right ends, last: the track and its thumb over the part shown, the two arrows.
        if scrolls {
            let [up, down, track] = scroll_rects(top, pitch, shown, x[1]);
            v.push(Line::button(track, "", (Plate::Off, Shape::Box), 1.0));
            v.push(Line::button(thumb_rect(track, page.scroll, shown, n), "", (Plate::On, Shape::Box), 1.0));
            v.push(Line::button(up, "", (lit(up, false), Shape::Up), 1.0));
            v.push(Line::button(down, "", (lit(down, false), Shape::Down), 1.0));
        }
        v
    }
}

/// The scroll bar's thumb in `track`: the part of a tab's `n` rows the panel shows, `shown` of them from `scroll`.
fn thumb_rect(track: [f32; 4], scroll: u32, shown: u32, n: u32) -> [f32; 4] {
    let h = track[3] - track[1];
    [track[0], track[1] + h * scroll as f32 / n as f32, track[2], track[1] + h * (scroll + shown) as f32 / n as f32]
}

/// `text` in lines of at most `width` letters, broken at spaces.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in text.split(' ') {
        match out.last_mut() {
            Some(l) if l.len() + 1 + word.len() <= width => {
                l.push(' ');
                l.push_str(word);
            }
            _ => out.push(word.to_string()),
        }
    }
    out
}
