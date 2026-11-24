//! The game's PS2 button glyphs drawn as the buttons of the device being played with (`play`: `input::Input::prompts`).
//!
//! # The game's glyphs
//!
//! Every frame, in the front end and in every level, the jobs upload from the main program the HUD's two fonts, each with its
//! glyph sheet and palettes (IMAGE transfers from these addresses, in this order; `scripts/` none: read from the frames' jobs and
//! RAM, front end field 900 and Veldin field 1800):
//!
//! | set | font CLUT | sheet palettes 0-3 | keycap CLUTs | sheet palettes 4-5 | CLUT | font (256 x 128, PSMT4) | sheet (64 x 64, PSMT8) |
//! | A | 0x144550 | 0x144590 + 0x400 k | 0x145590 + 0x40 k | 0x145690, 0x145A90 | 0x145E90 | 0x145ED0 | 0x1494D0 |
//! | B | 0x14A950 | 0x14A990 + 0x400 k | 0x14B990 + 0x40 k | 0x14BA90, 0x14BE90 | 0x14C290 | 0x14C2D0 | 0x14F2D0 |
//!
//! - The glyph sheet holds six 16 x 16 glyphs (the cell at u = 1 + 16 column, v = 1 + 17 row) that only show through their own
//!   palette: palette 0 Cross (blue), 1 Circle (red), 2 Triangle (green), 3 Square (pink), 4 the left stick, 5 the right stick.
//!   The game's text names them with the codes 0x10, 0x11, 0x12, 0x13, 0x18 ("the Left\x18") and 0x19 ("the Right\x19"), the
//!   palettes' order (the level's help texts in the gameplay block: "press \x10 in mid-air", "hold \x12 for your QuickSelect").
//!   A glyph is drawn as a sprite or strip of the sheet with REGION_CLAMP on its cell, through its palette (CBP), the vertex
//!   colour 0xC0 (the save prompt's "\x10 Yes", "\x12 No").
//! - The font holds the shoulder buttons as keycaps (text codes 0x14 to 0x17: "hold \x15 to crouch" is R1, "strafe, hold \x16 or
//!   \x17" L2 or R2): L1, R1, L2, R2 and a blank key, at these texels (inclusive, found in the font's texels):
//!   set A in a row at v 90..104, u 4..29, 37..62, 70..95, 103..128 (blank 136..161); set B L1 134..159, R1 167..192, L2
//!   200..225 at v 61..76 and R2 4..29 (blank 37..62) at v 78..93.
//!
//! So a primitive is a glyph by what it samples: texels uploaded from a set's sheet through one of its six palettes (the
//! button), or texels uploaded from a set's font within one of its four keycaps (the button). Never by where it is on screen.
//!
//! # The icons
//!
//! For a controller (an Xbox controller, a DualSense, a DualShock 4: the game's own PS2 glyphs are low resolution) and for the
//! keyboard and mouse, the glyph samples the icon of what is bound to it (`Atlas::icon`, `bindings.rs`)
//! from the atlas `scripts/prompt-icons.py` builds from the player's downloaded icons (Kenney's Input Prompts, CC0, as
//! Mirrorshard uses them) into the profile: the primitive's texture coordinates are moved from the glyph's cell to the icon's
//! (the icon's shape kept: `fitted`), REGION_CLAMP to the icon's cell, and the sheet glyphs' vertex colour 0xC0 (the sheet's
//! palettes are made to be brightened by half) to 0x80, so that the icon shows its own colours.
//!
//! # Labels (the keyboard and mouse in a menu)
//!
//! A PC game shows no pad or key glyph where a click does the work (the player, 13 November 2026: "the PC mode dont need any
//! button icons/click icon ... you just have a Yes or No button you click on"). The game's prompts are its own texts with glyph
//! codes: "\x10 SELECT", "\x12 BACK", "\x10 BLAST OFF", "\x12 Local Map", and the battle missions' footer "select \x10",
//! "Vendor \x13", "exit \x12" (the glyph after the word). `labels` finds them on the frame, one rule for the two layers: a glyph
//! labels the short phrase after it on its line, or, on a line that ends with a glyph, the phrase that ends at it; a glyph inside a
//! sentence or at a paragraph's margin labels nothing, so the hints ("press \x13 three times") keep the glyph, drawn as the key it
//! is bound to. With the keyboard and mouse in a menu (`menus::Menus::menu`: the front end, a pause and the screens of its family
//! a level opens from the world, the vendor), in both layers, the 2D layer (`front.rs`) and a native frame (`prepare`,
//! `eedraw::build`, for a menu over the running level), every label is a plate in its glyph's place behind its words, dark with a
//! gold edge, amber under the pointer, a click on it the press the glyph stood for (`input::Answer`): a footer's (SELECT, BACK,
//! EXIT, VENDOR, BUY, below `FOOTER`) leaves the screen's items to the pointer, a box's answers ("\x10 Yes",
//! "\x12 No") are the only targets while the box is up. No label is left out: a screen that puts back a picture it saved (the
//! vendor) would show the label an earlier frame drew under a label taken away.

use crate::bindings::{Bindings, Key};
use crate::input::Device;
use crate::mobies::EePrim;
use std::sync::Arc;

/// The PS2 buttons the glyphs show.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Button {
    Cross = 0,
    Circle = 1,
    Triangle = 2,
    Square = 3,
    L1 = 4,
    R1 = 5,
    L2 = 6,
    R2 = 7,
    LeftStick = 8,
    RightStick = 9,
}

const SHEET_BUTTONS: [Button; 6] = [Button::Cross, Button::Circle, Button::Triangle, Button::Square, Button::LeftStick, Button::RightStick];
const KEY_BUTTONS: [Button; 4] = [Button::L1, Button::R1, Button::L2, Button::R2];

/// One of the HUD's two glyph sets (module doc).
struct Set {
    font: u32,
    sheet: u32,
    palettes: [u32; 6],
    /// L1, R1, L2, R2: (u0, v0, u1, v1), inclusive.
    keys: [(u32, u32, u32, u32); 4],
}

const SETS: [Set; 2] = [
    Set { font: 0x14_5ED0, sheet: 0x14_94D0, palettes: [0x14_4590, 0x14_4990, 0x14_4D90, 0x14_5190, 0x14_5690, 0x14_5A90], keys: [(4, 90, 29, 104), (37, 90, 62, 104), (70, 90, 95, 104), (103, 90, 128, 104)] },
    Set { font: 0x14_C2D0, sheet: 0x14_F2D0, palettes: [0x14_A990, 0x14_AD90, 0x14_B190, 0x14_B590, 0x14_BA90, 0x14_BE90], keys: [(134, 61, 159, 76), (167, 61, 192, 76), (200, 61, 225, 76), (4, 78, 29, 93)] },
];

/// The icon atlas (`scripts/prompt-icons.py`): its size, each icon's device (1 Xbox, 2 keyboard and mouse, 3 DualSense, 4
/// DualShock 4), name and cell (x, y, width, height; each icon at its own shape, with a transparent edge), and the texels in the
/// native textures' units (colour as shown, alpha 0x80 opaque). The keyboard's `MOVES` cell is drawn by the port: the four move
/// keys bound, as an inverted T (`with_moves`).
pub struct Atlas {
    pub size: (u32, u32),
    cells: Vec<(u32, String, [u32; 4])>,
    pub texture: uyafmt::core::Texture,
    /// The move keys the `MOVES` cell shows, and a count of the times it was drawn (a texture made of the atlas is made again
    /// when this changes: `swap`).
    moves: [Option<Key>; 4],
    pub version: u32,
}

const XBOX: u32 = 1;
const KBM: u32 = 2;
/// The keyboard's left stick: the move keys (`Atlas::with_moves`), and its right stick: the mouse.
const MOVES: &str = "Moves";
const MOUSE_MOVE: &str = "MouseMove";

impl Atlas {
    pub fn read(path: &str) -> Result<Atlas, String> {
        let b = std::fs::read(path).map_err(|e| format!("{path}: {e} (the button prompt atlas: python -I scripts/prompt-icons.py <Kenney Input Prompts folder> \"{path}\")"))?;
        let w = |i: usize| b.get(4 * i..4 * i + 4).map(|x| u32::from_le_bytes(x.try_into().expect("4 bytes"))).ok_or_else(|| format!("{path}: ends at word {i}"));
        if b.get(..4) != Some(b"UYAP") || w(1)? != 2 {
            return Err(format!("{path}: not a version 2 prompt atlas (python -I scripts/prompt-icons.py <Kenney Input Prompts folder> \"{path}\" makes one)"));
        }
        let (width, height, n) = (w(2)?, w(3)?, w(4)? as usize);
        if !width.is_power_of_two() || !height.is_power_of_two() || width > 1024 || height > 1024 {
            return Err(format!("{path}: a {width} x {height} atlas (REGION_CLAMP and TEX0 take powers of two up to 1024)"));
        }
        // A cell: its device, its name (16 bytes, NUL-padded), x, y, width, height.
        let mut cells = Vec::with_capacity(n);
        for k in 0..n {
            let at = 5 + 9 * k;
            let device = w(at)?;
            if !(1..=4).contains(&device) {
                return Err(format!("{path}: icon {k}: device {device}"));
            }
            let name = b.get(4 * (at + 1)..4 * (at + 5)).ok_or_else(|| format!("{path}: icon {k}: ends in its name"))?;
            let name = std::str::from_utf8(name).map_err(|_| format!("{path}: icon {k}: its name is not text"))?.trim_end_matches('\0').to_string();
            cells.push((device, name, [w(at + 5)?, w(at + 6)?, w(at + 7)?, w(at + 8)?]));
        }
        let px = b.get(4 * (5 + 9 * n)..).filter(|p| p.len() == (4 * width * height) as usize).ok_or_else(|| format!("{path}: not {width} x {height} pixels after the cells"))?;
        let texels = px.chunks_exact(4).map(|p| [p[0], p[1], p[2], ((p[3] as u32 * 128 + 127) / 255) as u8]).collect();
        let atlas = Atlas { size: (width, height), cells, texture: uyafmt::core::Texture { width: width as usize, height: height as usize, levels: vec![texels], gs: None }, moves: [None; 4], version: 0 };
        // Every input a binding can take has its icon, and the sticks theirs.
        for k in crate::bindings::all_keys() {
            atlas.cell(KBM, &k.name()).ok_or_else(|| format!("{path}: no icon for {}", k.name()))?;
        }
        for (device, names) in [(KBM, &[MOVES, MOUSE_MOVE][..]), (XBOX, &["left_stick", "right_stick"][..]), (3, &["left_stick", "right_stick"][..]), (4, &["left_stick", "right_stick"][..])] {
            for name in names {
                atlas.cell(device, name).ok_or_else(|| format!("{path}: no icon {name} of device {device}"))?;
            }
        }
        for device in [XBOX, 3, 4] {
            for (name, _, _) in crate::bindings::PAD {
                atlas.cell(device, name).ok_or_else(|| format!("{path}: no icon {name} of device {device}"))?;
            }
        }
        Ok(atlas)
    }

    fn cell(&self, device: u32, name: &str) -> Option<[u32; 4]> {
        self.cells.iter().find(|(d, n, _)| *d == device && n == name).map(|c| c.2)
    }

    /// The icon a glyph of `button` shows on `device` with bindings `b` (None: the game's own glyph). On the keyboard, the input
    /// bound to the action that presses the button: the game's lock-strafe layout, which the keyboard and mouse play, does L1's
    /// actions with L2 and R1's with R2 (uyatr's `CAMERA_SETTING`), so a glyph of L1 or L2 shows the key of the action on L2 and
    /// one of R1 or R2 the key of the action on R2; the left stick the move keys, the right stick the mouse; a button no action
    /// is bound for keeps its glyph. On a controller, the controller button bound to it (a PlayStation controller's icons are the
    /// atlas's, not the game's own low-resolution glyphs).
    pub fn icon(&self, device: Device, b: &Bindings, button: Button) -> Option<[u32; 4]> {
        let pad = |kind: u32, bit: u16| self.cell(kind, crate::bindings::PAD.iter().find(|r| r.2 == b.pad_for(bit)).expect("a PS2 button").0);
        let kind = if device == Device::DualSense { 3 } else { 4 };
        match (device, button.pad()) {
            (Device::Keyboard, None) => self.cell(KBM, if button == Button::LeftStick { MOVES } else { MOUSE_MOVE }),
            (Device::Keyboard, Some(bit)) => {
                let bit = match bit {
                    crate::input::L1 => crate::input::L2,
                    crate::input::R1 => crate::input::R2,
                    bit => bit,
                };
                b.shown(bit).and_then(|k| self.cell(KBM, &k.name()))
            }
            (Device::Xbox, None) => self.cell(XBOX, if button == Button::LeftStick { "left_stick" } else { "right_stick" }),
            (Device::Xbox, Some(bit)) => pad(XBOX, bit),
            (Device::DualSense | Device::DualShock4, None) => self.cell(kind, if button == Button::LeftStick { "left_stick" } else { "right_stick" }),
            (Device::DualSense | Device::DualShock4, Some(bit)) => pad(kind, bit),
        }
    }

    /// The atlas cell (x, y, width, height) at `k` of `named_icon`.
    fn cell_at(&self, k: usize) -> [u32; 4] {
        self.cells[k].2
    }

    /// The index of the cell of the icon of PS2 button `bit` (SELECT, START, L3, R3: the buttons the game's texts name in words,
    /// with no glyph) on `device` with bindings `b`: the input bound to it on the keyboard and mouse, the controller button bound
    /// to it on a controller (a PlayStation controller's own: the PS2's words are not its buttons' names).
    fn named_icon(&self, device: Device, b: &Bindings, bit: u16) -> Option<usize> {
        let pad = |kind: u32| (kind, crate::bindings::PAD.iter().find(|r| r.2 == b.pad_for(bit)).expect("a PS2 button").0.to_string());
        let (kind, name) = match device {
            Device::Keyboard => (KBM, b.shown(bit)?.name()),
            Device::Xbox => pad(XBOX),
            Device::DualSense => pad(3),
            Device::DualShock4 => pad(4),
        };
        Some(self.cells.iter().position(|(d, n, _)| *d == kind && *n == name).unwrap_or_else(|| panic!("prompts: no icon {name} of device {kind}")))
    }

    /// The move keys `MOVES` shows.
    pub fn moves(&self) -> [Option<Key>; 4] {
        self.moves
    }

    /// The atlas with its `MOVES` cell showing `moves` (forward, left, back, right): forward's icon in the top row's middle, the
    /// others along the bottom row, each scaled to its third of the cell's width and half its height, its shape kept; a move with no
    /// key bound leaves its place empty.
    pub fn with_moves(&self, moves: [Option<Key>; 4]) -> Atlas {
        let [mx, my, mw, mh] = self.cell(KBM, MOVES).expect("checked by read");
        let mut texels = self.texture.levels[0].clone();
        let width = self.size.0 as usize;
        for y in my..my + mh {
            for x in mx..mx + mw {
                texels[y as usize * width + x as usize] = [0; 4];
            }
        }
        let (bw, bh) = (mw as f32 / 3.0, mh as f32 / 2.0);
        for (k, place) in moves.iter().zip([(1.0, 0.0), (0.0, 1.0), (1.0, 1.0), (2.0, 1.0)]) {
            let Some([sx, sy, sw, sh]) = k.and_then(|k| self.cell(KBM, &k.name())) else { continue };
            // The icon scaled into its box, its shape kept, centred.
            let s = (bw / sw as f32).min(bh / sh as f32);
            let (dw, dh) = (sw as f32 * s, sh as f32 * s);
            let (x0, y0) = (mx as f32 + place.0 * bw + (bw - dw) / 2.0, my as f32 + place.1 * bh + (bh - dh) / 2.0);
            for y in y0.floor() as u32..(y0 + dh).ceil() as u32 {
                for x in x0.floor() as u32..(x0 + dw).ceil() as u32 {
                    // The source texel under this one's centre, bilinear.
                    let (u, v) = ((x as f32 + 0.5 - x0) / s - 0.5, (y as f32 + 0.5 - y0) / s - 0.5);
                    if u < -0.5 || v < -0.5 || u > sw as f32 - 0.5 || v > sh as f32 - 0.5 {
                        continue;
                    }
                    let at = |i: i64, j: i64| self.texture.levels[0][(sy as i64 + j.clamp(0, sh as i64 - 1)) as usize * width + (sx as i64 + i.clamp(0, sw as i64 - 1)) as usize];
                    let (i, j) = (u.floor() as i64, v.floor() as i64);
                    let (fu, fv) = (u - u.floor(), v - v.floor());
                    let mut out = [0u8; 4];
                    for c in 0..4 {
                        let top = at(i, j)[c] as f32 * (1.0 - fu) + at(i + 1, j)[c] as f32 * fu;
                        let bottom = at(i, j + 1)[c] as f32 * (1.0 - fu) + at(i + 1, j + 1)[c] as f32 * fu;
                        out[c] = (top * (1.0 - fv) + bottom * fv).round() as u8;
                    }
                    texels[y as usize * width + x as usize] = out;
                }
            }
        }
        Atlas { size: self.size, cells: self.cells.clone(), texture: uyafmt::core::Texture { width: self.texture.width, height: self.texture.height, levels: vec![texels], gs: None }, moves, version: self.version + 1 }
    }
}

/// How the window shows the game's text and button glyphs: the device whose prompts it shows, the icons, the letters.
#[derive(Clone)]
pub struct Look {
    pub device: Device,
    pub atlas: Arc<Atlas>,
    pub text: Arc<crate::text::Text>,
    /// The keyboard and mouse in a menu: the labels as a PC game's (the module doc's "Labels").
    pub clicks: bool,
    /// The port's text in place of the game's on this frame (`menus.rs`).
    pub lines: Arc<Vec<crate::menus::Line>>,
    /// Where the mouse points on the picture (shares), for the answers' buttons.
    pub pointer: Option<(f32, f32)>,
    /// The player's bindings: what the icons show (`Atlas::icon`).
    pub bindings: Bindings,
    /// The answers a native frame's labels offer (`prepare` writes them, the window hands them to the game thread).
    pub answers: Arc<std::sync::Mutex<Vec<crate::input::Answer>>>,
    /// The save slots' pictures (`uyafmt::wad38::slot_pictures`), and where this frame shows which (`menus::slot_pictures`).
    pub pictures: Arc<Vec<uyafmt::core::Texture>>,
    pub slots: Vec<([f32; 4], usize)>,
}

/// A mark of the frame's text: a button glyph, or a letter of the text pass (not its shadow).
#[derive(Clone, Copy, Debug)]
pub(crate) enum Mark {
    Glyph(Button),
    Letter,
}

/// A button glyph that labels a short phrase on its line (`labels`): the glyph's mark, the phrase's letters and their box
/// (shares of the picture).
#[derive(Clone, Debug)]
pub(crate) struct Label {
    pub glyph: usize,
    pub button: Button,
    pub words: Vec<usize>,
    pub rect: [f32; 4],
}

/// The widest a label's phrase is on a line (shares of the picture; the vendor's "Test Weapon In" is 0.211).
const WIDEST: f32 = 0.25;
/// A label whose glyph is lower than this (its middle, a share of the picture) is a footer's (SELECT, BACK, the vendor's BUY and
/// EXIT at 0.893): it leaves the screen's own items to the pointer. Higher ones are a box's answers when there are two or more of
/// them (`boxes`).
pub(crate) const FOOTER: f32 = 0.85;

/// A box offers a choice: the labels above `FOOTER` are its answers, the only targets while it shows (a click off them is no
/// press), when there are two or more of them ("\x10 Yes", "\x12 No"). One alone is a screen's own control, as a footer's is (the
/// vendor's "\x13 Test Weapon In VR Arena" over the item's picture): the screen's items keep the pointer.
pub(crate) fn boxes(answers: &mut [crate::input::Answer]) {
    if answers.iter().filter(|a| a.modal).count() < 2 {
        for a in answers.iter_mut() {
            a.modal = false;
        }
    }
}

/// The labels among the frame's marks (each with its box, shares of the picture). A glyph labels the phrase from it to the next
/// glyph on its line ("✕ SELECT  △ BACK"), or, on a line that ends with a glyph, the phrase that ends at it, back to the glyph
/// before it or the line's start ("select ✕  Vendor □  exit △", the battle missions' footer); a phrase that wraps onto a line
/// centred under it takes that line too. A glyph that sits inside a sentence (letters just before it and after it), is on
/// a paragraph's line (a line next to it starts where its line does), or whose phrase is wider than `WIDEST` labels nothing:
/// the hints' and HELP's inline glyphs stay glyphs. A glyph drawn twice at one place (the same box) labels once.
pub(crate) fn labels(marks: &[(Mark, [f32; 4])]) -> Vec<Label> {
    // A glyph and a glyph on one line (a glyph is taller than the lines' step, so a letter is on its line when the letter's middle
    // is within half the letter's own height of the glyph's).
    let line = |g: &[f32; 4], b: &[f32; 4]| ((b[1] + b[3]) / 2.0 - (g[1] + g[3]) / 2.0).abs() < (g[3] - g[1]).max(0.01);
    let on_line = |g: &[f32; 4], b: &[f32; 4]| ((b[1] + b[3]) / 2.0 - (g[1] + g[3]) / 2.0).abs() < 0.5 * (b[3] - b[1]).max(0.01);
    let glyphs: Vec<(usize, Button, [f32; 4])> = marks.iter().enumerate().filter_map(|(i, m)| if let (Mark::Glyph(b), r) = m { Some((i, *b, *r)) } else { None }).collect();
    let letters: Vec<(usize, [f32; 4])> = marks.iter().enumerate().filter(|(_, m)| matches!(m.0, Mark::Letter)).map(|(i, m)| (i, m.1)).collect();
    let mut out: Vec<Label> = Vec::new();
    let mut done: Vec<[f32; 4]> = Vec::new();
    for &(gi, button, g) in &glyphs {
        if done.iter().any(|d| (d[0] - g[0]).abs() < 0.004 && (d[1] - g[1]).abs() < 0.004) {
            continue;
        }
        // The line ends with a glyph when no letter on it lies right of its rightmost glyph.
        let right_glyph = glyphs.iter().filter(|(_, _, h)| line(&g, h)).map(|(_, _, h)| h[2]).fold(f32::MIN, f32::max);
        let ends_with_glyph = !letters.iter().any(|(_, b)| on_line(&g, b) && b[0] >= right_glyph - 0.002);
        let between = |a: f32, b: f32| glyphs.iter().any(|(_, _, h)| line(&g, h) && h[0] > a && h[0] < b);
        let words: Vec<usize> = if ends_with_glyph {
            // The phrase before it, back to the glyph before it on the line.
            letters.iter().filter(|(_, b)| on_line(&g, b) && b[2] <= g[0] + 0.005 && !between(b[2], g[0])).map(|(i, _)| *i).collect()
        } else {
            letters.iter().filter(|(_, b)| on_line(&g, b) && b[0] >= g[2] - 0.005 && !between(g[2] + 0.002, b[0])).map(|(i, _)| *i).collect()
        };
        if words.is_empty() {
            continue;
        }
        let r = words.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |r, &i| {
            let b = marks[i].1;
            [r[0].min(b[0]), r[1].min(b[1]), r[2].max(b[2]), r[3].max(b[3])]
        });
        let inline = !ends_with_glyph && letters.iter().any(|(_, b)| on_line(&g, b) && b[2] <= g[0] + 0.005 && g[0] - b[2] < 0.03);
        // A paragraph's line: a line next to the glyph's (within 1.7 of the phrase's letter height) starts where the glyph's line
        // starts, in letters of the phrase's size (the line's tallest letter's: a lowercase letter's box is shorter than a capital's).
        // Both forms: HELP's Swim reads "to dive and \x10 to rise. Push \x10", a line that ends with a glyph inside a paragraph. The
        // vendor's BUY is not a paragraph's line: the caption of the carousel's item two before the highlighted one ("Ammo", in
        // letters two thirds of BUY's height, about 1.7 of BUY's height above) starts where BUY's glyph does.
        let wh = words.iter().map(|&i| marks[i].1[3] - marks[i].1[1]).fold(0.0f32, f32::max).max(0.01);
        let start = letters.iter().filter(|(_, b)| on_line(&g, b)).map(|(_, b)| b[0]).chain(glyphs.iter().filter(|(_, _, h)| line(&g, h)).map(|(_, _, h)| h[0])).fold(f32::MAX, f32::min);
        let margin = letters.iter().any(|(_, b)| {
            let dy = ((b[1] + b[3]) / 2.0 - (g[1] + g[3]) / 2.0).abs();
            let starts = dy > 0.6 * wh && dy < 1.7 * wh && (b[0] - start).abs() < 0.012 && !letters.iter().any(|(_, c)| on_line(b, c) && c[2] <= b[0] + 0.002);
            starts && (0.85..1.2).contains(&(letters.iter().filter(|(_, c)| on_line(b, c)).map(|(_, c)| c[3] - c[1]).fold(0.0f32, f32::max) / wh))
        });
        if r[2] - r[0] > WIDEST || inline || margin {
            continue;
        }
        // A phrase that wraps onto the line under it, centred under its first line (the vendor's "\x13 Test Weapon In" over
        // "VR Arena"): that line goes with it when the phrase ends its own line, the line is in letters of its size, holds no glyph
        // and lies inside the first line's span.
        let (mut words, mut rect) = (words, r);
        if !ends_with_glyph && right_glyph <= g[2] + 0.002 {
            let mut last = (r[1] + r[3]) / 2.0;
            loop {
                let under = |b: &[f32; 4]| {
                    let dy = (b[1] + b[3]) / 2.0 - last;
                    dy > 0.5 * wh && dy < 1.6 * wh
                };
                let next: Vec<usize> = letters.iter().filter(|(_, b)| under(b)).map(|(i, _)| *i).collect();
                // (a lowercase letter's box is shorter than a capital's: the line's tallest letter is its size)
                let fits = |i: &usize| marks[*i].1[0] >= r[0] - 0.01 && marks[*i].1[2] <= r[2] + 0.01;
                let size = next.iter().map(|&i| marks[i].1[3] - marks[i].1[1]).fold(0.0f32, f32::max) / wh;
                if next.is_empty() || !(0.85..1.2).contains(&size) || !next.iter().all(fits) || glyphs.iter().any(|(_, _, h)| under(h)) {
                    break;
                }
                for &i in &next {
                    let b = marks[i].1;
                    rect = [rect[0].min(b[0]), rect[1].min(b[1]), rect[2].max(b[2]), rect[3].max(b[3])];
                }
                last = next.iter().map(|&i| (marks[i].1[1] + marks[i].1[3]) / 2.0).sum::<f32>() / next.len() as f32;
                words.extend(next);
            }
        }
        done.push(g);
        out.push(Label { glyph: gi, button, words, rect });
    }
    out
}

/// A label's plate (`labels`): its rectangle around the words, amber under the pointer, dark otherwise, with a gold edge; the fill
/// and the four edges, as (rectangle, colour) in shares of the picture, the colour's alpha in the GS's range (0x80 = 1). Opaque:
/// a screen that puts back a picture it saved (the vendor's IMAGE into the main frame) has the label as an earlier frame drew it
/// under its own, which must not show through.
pub(crate) fn plate(words: [f32; 4], pointer: Option<(f32, f32)>) -> ([f32; 4], Vec<([f32; 4], [u8; 4])>) {
    let rect = [words[0] - 0.012, words[1] - 0.006, words[2] + 0.012, words[3] + 0.006];
    let lit = pointer.is_some_and(|(u, v)| u >= rect[0] && u <= rect[2] && v >= rect[1] && v <= rect[3]);
    let (fill, edge) = if lit { ([0x6c, 0x44, 0x0c, 0x80], [0xff, 0xe0, 0x90, 0x80]) } else { ([0x14, 0x0c, 0x04, 0x80], [0xe8, 0xa8, 0x30, 0x80]) };
    let (bx, by) = (0.003, 0.004);
    let mut parts = vec![(rect, fill)];
    for e in [[rect[0], rect[1], rect[2], rect[1] + by], [rect[0], rect[3] - by, rect[2], rect[3]], [rect[0], rect[1], rect[0] + bx, rect[3]], [rect[2] - bx, rect[1], rect[2], rect[3]]] {
        parts.push((e, edge));
    }
    (rect, parts)
}

impl Button {
    /// The pad bit of the press this glyph stands for (the sticks are pushed, not pressed).
    pub fn pad(self) -> Option<u16> {
        use crate::input;
        match self {
            Button::Cross => Some(input::CROSS),
            Button::Circle => Some(input::CIRCLE),
            Button::Triangle => Some(input::TRIANGLE),
            Button::Square => Some(input::SQUARE),
            Button::L1 => Some(input::L1),
            Button::R1 => Some(input::R1),
            Button::L2 => Some(input::L2),
            Button::R2 => Some(input::R2),
            Button::LeftStick | Button::RightStick => None,
        }
    }
}

/// The look of one level's EE textures (None: the game's own glyphs and font), and the atlases' indices in the EE textures once
/// made.
#[derive(Default)]
pub struct Prompts {
    pub look: Option<Look>,
    /// The atlas's texture: the atlas's version it was made of (`Atlas::version`) and its index.
    pub index: Option<(u32, u32)>,
    pub text_index: Option<u32>,
    /// This frame's letters laid out (`prepare`): by the letter's REGION_CLAMP, its box (GS pixels, rounded) and colour, the
    /// rectangle it is drawn at.
    pub rects: std::collections::HashMap<(u64, [i64; 4], [u8; 4]), [f64; 4]>,
    /// With the keyboard and mouse in a menu (`prepare`): the primitives not drawn (the labels' glyphs), and the plates, each to go
    /// in before the primitive at its index (`eedraw::build`).
    pub hidden: std::collections::HashSet<usize>,
    pub(crate) plates: Vec<(usize, EePrim)>,
    /// The save slots' pictures made among the EE textures, by picture.
    pub pictures: std::collections::HashMap<usize, u32>,
}

/// The picture's frame in GS pixels (the game draws its 2D layer into 512 x 416), for a primitive's place in shares of it.
const FRAME: (f64, f64) = (512.0, 416.0);

/// A primitive's box in shares of the picture (its vertices less XYOFFSET).
fn share_box(p: &EePrim) -> [f32; 4] {
    let (xo, yo) = ((p.xyoffset & 0xffff) as f64 / 16.0, (p.xyoffset >> 32 & 0xffff) as f64 / 16.0);
    let (x0, x1) = p.verts.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(v.x), b.max(v.x)));
    let (y0, y1) = p.verts.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(v.y), b.max(v.y)));
    [((x0 - xo) / FRAME.0) as f32, ((y0 - yo) / FRAME.1) as f32, ((x1 - xo) / FRAME.0) as f32, ((y1 - yo) / FRAME.1) as f32]
}

/// With the keyboard and mouse in a menu, the frame's labels as a PC game's (`labels`): each label's glyph is not drawn and its
/// words get a plate that lights under the pointer, a click on it giving the game the press the glyph stood for
/// (`input::Answer`: a footer's leaves the menu's items to the pointer, a box's answers are the only targets while the box is up).
/// The plates are sprites made from the label's glyph (its XYOFFSET, SCISSOR, FRAME, TEST, ALPHA), untextured.
fn menu_labels(prims: &[(usize, EePrim)], look: &Look, out: &mut Prompts, answers: &mut Vec<crate::input::Answer>) {
    let mut marks: Vec<(Mark, [f32; 4])> = Vec::new();
    let mut of: Vec<usize> = Vec::new();
    // The shadows (the text pass's letters drawn black) go with their letters.
    let mut shadows: Vec<usize> = Vec::new();
    for (i, (_, p)) in prims.iter().enumerate() {
        // A port line's letters (`port_lines`) are not drawn.
        if out.hidden.contains(&i) {
            continue;
        }
        if let Some((button, _, _)) = glyph(p) {
            marks.push((Mark::Glyph(button), share_box(p)));
            of.push(i);
        } else if let Some(d) = drawn(p, &look.text) {
            if d.ch.is_whitespace() {
                continue;
            }
            if d.colour[..3] == [0, 0, 0] {
                shadows.push(i);
            } else {
                marks.push((Mark::Letter, share_box(p)));
                of.push(i);
            }
        }
    }
    for l in labels(&marks) {
        let g = marks[l.glyph].1;
        // The glyph's twins (drawn twice at one place) go with it.
        let glyph_prims: Vec<usize> = marks.iter().enumerate().filter(|(_, (m, b))| matches!(m, Mark::Glyph(_)) && (b[0] - g[0]).abs() < 0.004 && (b[1] - g[1]).abs() < 0.004).map(|(k, _)| of[k]).collect();
        let Some(pad) = l.button.pad() else { continue };
        let word_prims: Vec<usize> = l.words.iter().map(|&k| of[k]).collect();
        // The plate goes in before the label's first primitive: its glyph, its letters or their shadows (drawn just before them).
        let near = word_prims.iter().copied().min().expect("a word").saturating_sub(24);
        let shadow_first = shadows.iter().copied().filter(|&k| k >= near).find(|&k| {
            let b = share_box(&prims[k].1);
            b[0] >= l.rect[0] - 0.002 && b[0] <= l.rect[2] + 0.008 && b[1] >= l.rect[1] - 0.012 && b[1] <= l.rect[3]
        });
        out.hidden.extend(glyph_prims.iter().copied());
        let footer = (g[1] + g[3]) / 2.0 > FOOTER;
        let (rect, parts) = plate(l.rect, look.pointer);
        let first = glyph_prims.iter().chain(&word_prims).copied().chain(shadow_first).min().expect("a glyph");
        let t = &prims[of[l.glyph]].1;
        let (xo, yo) = ((t.xyoffset & 0xffff) as f64 / 16.0, (t.xyoffset >> 32 & 0xffff) as f64 / 16.0);
        let z = t.verts[0].z;
        for (r, rgba) in parts {
            let corner = |u: f32, v: f32| crate::effects::GsVertex { x: u as f64 * FRAME.0 + xo, y: v as f64 * FRAME.1 + yo, z, rgba, ..t.verts[0] };
            // A sprite (PRIM 6), alpha blended (ABE), untextured.
            out.plates.push((first, EePrim { prim: 6 | 0x40, verts: crate::mobies::Verts::new(&[corner(r[0], r[1]), corner(r[2], r[3])]), texels: None, clut: None, ..t.clone() }));
        }
        answers.push(crate::input::Answer { rect, button: pad, modal: !footer });
    }
}

/// A line's style of the game's text it takes the place of (`port_lines`): its font set, texel size down, baseline, stretch
/// across, the game's text's left and right (GS coordinates), the line's band, the pass's colour, the pass's first letter (the
/// GS state the line's letters draw with) and where the line's letters go (after the band's last letter).
#[derive(Clone)]
struct LineStyle {
    set: usize,
    ty: f64,
    base: f64,
    stretch: f64,
    l: f64,
    r: f64,
    y: f32,
    colour: [u8; 4],
    template: EePrim,
    at: usize,
}

/// The port's text in place of the game's (`menus.rs` `Line`: the option lists' settings pages): the game's letters whose
/// centres are in a line's band (and everything else wholly in it, for a row emptied: `Line::all`) are not drawn
/// (`Prompts::hidden`), and the line's text is typeset (`text::Text::typeset`) in their font, colour and pass (the shadow and the
/// text apart), from where the game's text started, to where it ended or centred on it. A line placed on its own (`Line::at`) is
/// typeset at its own band and x, in the style of the last line before it that had letters. Its letters are primitives of the
/// game's font made on the font's cells (REGION_CLAMP), drawn from the vector font as the game's own letters are (`letter`, at the
/// rectangles `made` gives `rects`), after the band's last letter; a button's plate (`Line::plate`) just before them. What cannot
/// be typeset is named in `problems` and not drawn.
fn port_lines(prims: &[(usize, EePrim)], look: &Look, out: &mut Prompts, made: &mut Vec<(crate::text::Drawn, u64, [f64; 4])>, problems: &mut Vec<String>) {
    let mut style: Vec<LineStyle> = Vec::new();
    for line in look.lines.iter() {
        let mut taken: Vec<(usize, crate::text::Drawn)> = Vec::new();
        for (i, (_, p)) in prims.iter().enumerate() {
            if out.hidden.contains(&i) {
                continue;
            }
            let b = share_box(p);
            match drawn(p, &look.text) {
                Some(d) => {
                    let (cx, cy) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
                    if cx >= line.x0 && cx <= line.x1 && (cy - line.y).abs() <= line.h / 2.0 {
                        out.hidden.insert(i);
                        taken.push((i, d));
                    }
                }
                // What else a line empties lies wholly in its band: the pause's dimming strips, which run the picture's height
                // through every row, stay.
                None if line.all && b[0] >= line.x0 && b[2] <= line.x1 && b[1] >= line.y - line.h / 2.0 && b[3] <= line.y + line.h / 2.0 => {
                    out.hidden.insert(i);
                }
                None => {}
            }
        }
        let mut colours: Vec<[u8; 4]> = Vec::new();
        for (_, d) in &taken {
            if !colours.contains(&d.colour) {
                colours.push(d.colour);
            }
        }
        let after = taken.iter().map(|(i, _)| i + 1).max();
        let mut own: Vec<LineStyle> = Vec::new();
        for colour in colours {
            let pass: Vec<&(usize, crate::text::Drawn)> = taken.iter().filter(|(_, d)| d.colour == colour).collect();
            let ds: Vec<&crate::text::Drawn> = pass.iter().map(|(_, d)| d).collect();
            // Letters drawn with no height (a scale growing from nothing) are not seen: no line in their style.
            let (Some((ty, base)), Some(stretch)) = (look.text.line(ds[0]), look.text.stretch(&ds)) else { continue };
            let (l, r) = (ds.iter().map(|d| d.game[0]).fold(f64::MAX, f64::min), ds.iter().map(|d| d.game[2]).fold(f64::MIN, f64::max));
            own.push(LineStyle { set: ds[0].set, ty, base, stretch, l, r, y: line.y, colour, template: prims[pass[0].0].1.clone(), at: after.expect("a pass has letters") });
        }
        if !own.is_empty() {
            style = own.clone();
        } else if line.at.is_some() {
            own = style.clone();
        }
        let Some(first) = own.first() else {
            if line.plate.is_some() || line.at.is_some() {
                problems.push(format!("prompts: the port's line {:?} has no game text in its band or before it to take its style from", line.text));
            }
            continue;
        };
        // A button's plate (`Line::plate`), behind its text: over the line's band.
        if let Some((look_, shape)) = line.plate {
            let r = [line.x0, line.y - line.h / 2.0, line.x1, line.y + line.h / 2.0];
            for prim in plate_prims(r, look_, shape, &first.template) {
                out.plates.push((first.at, prim));
            }
        }
        for st in &own {
            let (ox, oy) = ((st.template.xyoffset & 0xffff) as f64 / 16.0, (st.template.xyoffset >> 32 & 0xffff) as f64 / 16.0);
            // Where the letters go, at `scale` of the line's size.
            let place = |scale: f64| {
                if let Some(x) = line.at {
                    // The style's baseline moved to the line's band, its letters `size` times as large about the band's centre.
                    let centre = st.y as f64 * FRAME.1 + oy;
                    let base = centre + (st.base - st.y as f64 * FRAME.1 - oy) * line.size as f64 * scale + (line.y - st.y) as f64 * FRAME.1;
                    (x as f64 * FRAME.0 + ox, base, st.ty * line.size as f64 * scale)
                } else {
                    let x = match line.align {
                        crate::menus::Align::Left => st.l,
                        crate::menus::Align::Right => st.r,
                        crate::menus::Align::Centre => (st.l + st.r) / 2.0,
                    };
                    (x, st.base, st.ty)
                }
            };
            let stretch = if line.prose { 1.0 } else { st.stretch };
            let typeset = |(x, base, ty): (f64, f64, f64)| look.text.typeset(st.set, &line.text, ty, base, stretch, x, line.align);
            // A button's text no wider than nine tenths of its plate (a key's name on a binding's slot): set smaller to fit.
            let set = typeset(place(1.0)).and_then(|set| {
                let width = set.iter().map(|(r, _)| r[2]).fold(f64::MIN, f64::max) - set.iter().map(|(r, _)| r[0]).fold(f64::MAX, f64::min);
                let room = (line.x1 - line.x0) as f64 * FRAME.0 * 0.9;
                if line.plate.is_some() && line.at.is_some() && width > room { typeset(place(room / width)) } else { Ok(set) }
            });
            let set = match set {
                Ok(set) => set,
                Err(e) => {
                    problems.push(e);
                    continue;
                }
            };
            // A dimmed line's text pass at half its colour (the shadow pass, black, as it is).
            let mut colour = st.colour;
            if line.dim && colour[..3] != [0, 0, 0] {
                for c in &mut colour[..3] {
                    *c /= 2;
                }
            }
            let t = &st.template;
            // `typeset` gives a rectangle for each character but the spaces.
            for (ch, (r, _)) in line.text.chars().filter(|&c| c != ' ').zip(set) {
                let Some(cell) = look.text.cell(st.set, ch) else {
                    problems.push(format!("prompts: font set {} has no {ch:?} (the port's line {:?})", st.set, line.text));
                    continue;
                };
                let clamp = 0xa | (cell[0] as u64) << 4 | (cell[2] as u64) << 14 | (cell[1] as u64) << 24 | (cell[3] as u64) << 34;
                let corner = |x: f64, y: f64, u: u32, v: u32| crate::effects::GsVertex { x, y, u: u as f32, v: v as f32, s: u as f32, t: v as f32, q: 1.0, rgba: colour, ..t.verts[0] };
                let p = EePrim { prim: t.prim & !7 | 6 | 0x100, clamp, verts: crate::mobies::Verts::new(&[corner(r[0], r[1], cell[0], cell[1]), corner(r[2], r[3], cell[2] + 1, cell[3] + 1)]), ..t.clone() };
                let d = drawn(&p, &look.text).expect("a letter made on its font's cell");
                made.push((d, clamp, r));
                out.plates.push((st.at, p));
            }
        }
    }
}

/// A port's button (`menus::Line::plate`) over `r` (shares of the frame), in the labels' plates' palette (`plate`): dark with a
/// gold edge, lit amber under the pointer, brighter still when on (the tab showing, a scroll bar's thumb); an arrow is the plate
/// with a triangle in the edge's colour pointing its way. The fill, the four edges (sprites) and the arrow (a triangle), alpha
/// blended and untextured, drawn with `t`'s GS state (a letter of the line's style: its XYOFFSET, SCISSOR, FRAME, TEST, ALPHA).
fn plate_prims(r: [f32; 4], look: crate::menus::Plate, shape: crate::menus::Shape, t: &EePrim) -> Vec<EePrim> {
    use crate::menus::{Plate, Shape};
    let (fill, edge) = match look {
        Plate::Off => ([0x14, 0x0c, 0x04, 0x80], [0xe8, 0xa8, 0x30, 0x80]),
        Plate::Lit => ([0x48, 0x2e, 0x08, 0x80], [0xff, 0xe0, 0x90, 0x80]),
        Plate::On => ([0x8a, 0x58, 0x10, 0x80], [0xff, 0xf0, 0xc0, 0x80]),
    };
    let (xo, yo) = ((t.xyoffset & 0xffff) as f64 / 16.0, (t.xyoffset >> 32 & 0xffff) as f64 / 16.0);
    let v = |u: f32, w: f32, rgba: [u8; 4]| crate::effects::GsVertex { x: u as f64 * FRAME.0 + xo, y: w as f64 * FRAME.1 + yo, rgba, ..t.verts[0] };
    let sprite = |e: [f32; 4], rgba: [u8; 4]| EePrim { prim: 6 | 0x40, verts: crate::mobies::Verts::new(&[v(e[0], e[1], rgba), v(e[2], e[3], rgba)]), texels: None, clut: None, ..t.clone() };
    let (bx, by) = (0.002, 0.003);
    let mut out = vec![sprite(r, fill)];
    for e in [[r[0], r[1], r[2], r[1] + by], [r[0], r[3] - by, r[2], r[3]], [r[0], r[1], r[0] + bx, r[3]], [r[2] - bx, r[1], r[2], r[3]]] {
        out.push(sprite(e, edge));
    }
    let (cx, cy, hw, hh) = ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0, (r[2] - r[0]) * 0.32, (r[3] - r[1]) * 0.26);
    let tri = |a: (f32, f32), b: (f32, f32), c: (f32, f32)| EePrim { prim: 3 | 0x40, verts: crate::mobies::Verts::new(&[v(a.0, a.1, edge), v(b.0, b.1, edge), v(c.0, c.1, edge)]), texels: None, clut: None, ..t.clone() };
    match shape {
        Shape::Box => {}
        Shape::Up => out.push(tri((cx, cy - hh), (cx + hw, cy + hh), (cx - hw, cy + hh))),
        Shape::Down => out.push(tri((cx - hw, cy - hh), (cx + hw, cy - hh), (cx, cy + hh))),
        Shape::Left => out.push(tri((cx - hw, cy), (cx + hw, cy - hh * 1.4), (cx + hw, cy + hh * 1.4))),
        Shape::Right => out.push(tri((cx + hw, cy), (cx - hw, cy + hh * 1.4), (cx - hw, cy - hh * 1.4))),
    }
    out
}

/// With the keyboard and mouse in a menu, the shoulder buttons' names the screens write in text ("L1" and "R1" over QUICK
/// SELECT's ring arrows, "L2" and "R2" on the pause screens' side tabs) are not drawn: what they label is a control instead, lit
/// under the pointer (the textured art inside it drawn again, added at three quarters), a click on it giving the game the button
/// (`menus.rs`: a control `Answer`, which leaves the rest of the screen to the pointer).
fn shoulders(prims: &[(usize, EePrim)], look: &Look, out: &mut Prompts, answers: &mut Vec<crate::input::Answer>) {
    // The text pass's letters (and whether each is a shadow: the same letter, black, a few thousandths down and right).
    let text: Vec<(usize, [f32; 4], char, bool)> = prims.iter().enumerate().filter(|(i, _)| !out.hidden.contains(i)).filter_map(|(i, (_, p))| drawn(p, &look.text).map(|d| (i, share_box(p), d.ch, d.colour[..3] == [0, 0, 0]))).collect();
    let mut gone: Vec<usize> = Vec::new();
    for &(_, a, ch, _) in text.iter().filter(|t| !t.3 && matches!(t.2, 'L' | 'R')) {
        let h = (a[3] - a[1]).max(0.01);
        let on_line = |b: &[f32; 4]| ((b[1] + b[3]) / 2.0 - (a[1] + a[3]) / 2.0).abs() < 0.3 * h;
        // Its digit: the letter just after it on its line.
        let Some(&(_, b, digit, _)) = text.iter().find(|t| !t.3 && matches!(t.2, '1' | '2') && on_line(&t.1) && t.1[0] > a[0] && t.1[0] - a[0] < 1.5 * (a[2] - a[0])) else { continue };
        // A word of its own: no other letter just before it or just after its digit.
        let word = |t: &(usize, [f32; 4], char, bool)| t.1 == a || t.1 == b;
        if text.iter().any(|t| !t.3 && !word(t) && on_line(&t.1) && ((t.1[2] <= a[0] + 0.002 && a[0] - t.1[2] < 0.5 * h) || (t.1[0] >= b[2] - 0.002 && t.1[0] - b[2] < 0.5 * h))) {
            continue;
        }
        let button = match (ch, digit) {
            ('L', '1') => crate::input::L1,
            ('R', '1') => crate::input::R1,
            ('L', _) => crate::input::L2,
            _ => crate::input::R2,
        };
        let label = [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])];
        for t in &text {
            let shadow = t.3 && (t.2 == ch || t.2 == digit) && t.1[0] >= label[0] - 0.002 && t.1[0] <= label[2] + 0.008 && (t.1[1] - label[1]).abs() < 0.012;
            if word(t) || shadow {
                gone.push(t.0);
            }
        }
        // The control: the ring's arrow under its "L1" or "R1" (the label and the arrow below it, which is 0.034 wide, its middle
        // 0.008 left of the label's: Veldin's QUICK SELECT, L1 at 0.521..0.581 over the arrow at 0.524..0.558, 0.219..0.306);
        // the middle of the side tab its "L2" or "R2" is on, the tab's own column to the picture's edge.
        let rect = if digit == '1' {
            let mid = (label[0] + label[2]) / 2.0 - 0.008;
            [mid - 0.026, label[1] - 0.006, mid + 0.026, label[3] + 0.122]
        } else if ch == 'L' {
            [0.0, label[1] - 0.2, label[2] + 0.002, label[3] + 0.2]
        } else {
            [label[0] - 0.002, label[1] - 0.2, 1.0, label[3] + 0.2]
        };
        if look.pointer.is_some_and(|(u, v)| u >= rect[0] && u <= rect[2] && v >= rect[1] && v <= rect[3]) {
            for (k, (_, p)) in prims.iter().enumerate() {
                let b = share_box(p);
                let art = p.prim & 0x10 != 0 && !p.texels.is_some_and(|u| font_or_sheet(u.src));
                if out.hidden.contains(&k) || !art || b[0] >= rect[2] || b[2] <= rect[0] || b[1] >= rect[3] || b[3] <= rect[1] {
                    continue;
                }
                // Drawn again just after itself, added (ALPHA_1 0x48: Cs x As + Cd) at three quarters, in the control's scissor.
                let sc = p.scissor;
                let f = |sh: u32| (sc >> sh & 0x7ff) as f32;
                let (x0, x1) = (f(0).max((rect[0] * FRAME.0 as f32).floor()), f(16).min((rect[2] * FRAME.0 as f32).ceil()));
                let (y0, y1) = (f(32).max((rect[1] * FRAME.1 as f32).floor()), f(48).min((rect[3] * FRAME.1 as f32).ceil()));
                if x0 > x1 || y0 > y1 {
                    continue;
                }
                let verts: Vec<crate::effects::GsVertex> = p.verts.iter().map(|v| crate::effects::GsVertex { rgba: [v.rgba[0], v.rgba[1], v.rgba[2], 0x60], ..*v }).collect();
                let lit = EePrim { prim: p.prim | 0x40, alpha: 0x48, scissor: x0 as u64 | (x1 as u64) << 16 | (y0 as u64) << 32 | (y1 as u64) << 48, verts: crate::mobies::Verts::new(&verts), ..p.clone() };
                out.plates.push((k + 1, lit));
            }
        }
        answers.push(crate::input::Answer { rect, button, modal: false });
    }
    out.hidden.extend(gone);
}

/// The words of the game's texts that name a button it has no glyph for ("press SELECT or R3", "Press START to Quit Game", "L3"),
/// and the button each stands for.
const NAMED: [(&str, u16); 4] = [("SELECT", crate::input::SELECT), ("START", crate::input::START), ("L3", crate::input::L3), ("R3", crate::input::R3)];

/// The game's texts that name a button in a word (`NAMED`) show the icon of the input bound to it instead: the word's letters (and
/// their shadows) are not drawn and the icon, at its own shape and the letters' height, takes the word's place. A word that is
/// part of a longer one ("QUICK SELECT", the menu's item) or that labels a glyph ("\x10 SELECT", a footer's label: `labels`) is
/// not one.
fn named_buttons(prims: &[(usize, EePrim)], hud: &[usize], look: &Look, out: &mut Prompts) {
    // The text pass's letters: primitive, box, character, and whether it is a shadow.
    let text: Vec<(usize, [f32; 4], char, bool)> = hud.iter().copied().filter(|i| !out.hidden.contains(i)).filter_map(|i| drawn(&prims[i].1, &look.text).filter(|d| !d.ch.is_whitespace()).map(|d| (i, share_box(&prims[i].1), d.ch, d.colour[..3] == [0, 0, 0]))).collect();
    let glyphs: Vec<[f32; 4]> = hud.iter().map(|&i| &prims[i].1).filter(|p| glyph(p).is_some()).map(share_box).collect();
    let letters: Vec<&(usize, [f32; 4], char, bool)> = text.iter().filter(|t| !t.3).collect();
    let mut gone: Vec<usize> = Vec::new();
    let mut found: Vec<[f32; 4]> = Vec::new();
    for (word, bit) in NAMED {
        let chars: Vec<char> = word.chars().collect();
        for &first in letters.iter().filter(|t| t.2 == chars[0]) {
            let h = (first.1[3] - first.1[1]).max(0.01);
            let on_line = |b: &[f32; 4]| ((b[1] + b[3]) / 2.0 - (first.1[1] + first.1[3]) / 2.0).abs() < 0.3 * h;
            // The word's letters: each the next on the line, edge to edge.
            let mut run = vec![first];
            for &c in &chars[1..] {
                let at = run.last().expect("a letter").1;
                let Some(&next) = letters.iter().find(|t| t.2 == c && on_line(&t.1) && t.1[0] >= at[2] - 0.002 && t.1[0] - at[2] < 0.2 * h) else { break };
                run.push(next);
            }
            if run.len() != chars.len() {
                continue;
            }
            let (x0, x1) = (run[0].1[0], run[run.len() - 1].1[2]);
            if found.iter().any(|f| (f[0] - x0).abs() < 0.002 && (f[1] - first.1[1]).abs() < 0.004) {
                continue;
            }
            // A whole word: no letter joined to its front, nor to its back (but for a mark of punctuation).
            let joined_before = letters.iter().any(|t| on_line(&t.1) && t.1[2] <= x0 + 0.002 && x0 - t.1[2] < 0.2 * h);
            let joined_after = letters.iter().any(|t| on_line(&t.1) && t.1[0] >= x1 - 0.002 && t.1[0] - x1 < 0.2 * h && !matches!(t.2, '.' | ',' | '!' | '?' | ':' | ';' | ')'));
            // The word before it on its line: "QUICK" makes QUICK SELECT.
            let mut before: Vec<(f32, char)> = Vec::new();
            let mut edge = x0;
            while before.len() < 8 {
                let Some(t) = letters.iter().filter(|t| on_line(&t.1) && t.1[0] < edge - 0.0005 && t.1[2] <= edge + 0.01 && edge - t.1[2] < 1.5 * h).max_by(|a, b| a.1[0].total_cmp(&b.1[0])) else { break };
                before.push((t.1[0], t.2));
                edge = t.1[0];
            }
            before.reverse();
            let before: String = before.into_iter().map(|(_, c)| c).collect();
            let after_glyph = glyphs.iter().any(|g| on_line(g) && g[2] <= x0 + 0.005 && x0 - g[2] < 0.06);
            if joined_before || joined_after || before.ends_with("QUICK") || after_glyph {
                continue;
            }
            let Some(k) = look.atlas.named_icon(look.device, &look.bindings, bit) else { continue };
            found.push([x0, first.1[1], x1, first.1[3]]);
            let [_, _, aw, ah] = look.atlas.cell_at(k);
            // The icon at its shape, the letters' height, centred on the word.
            let (top, bottom) = run.iter().fold((f32::MAX, f32::MIN), |(a, b), t| (a.min(t.1[1]), b.max(t.1[3])));
            let (gh, cx, cy) = (bottom - top, (x0 + x1) / 2.0, (top + bottom) / 2.0);
            let gh = gh * 1.3;
            let gw = gh * (aw as f32 / ah as f32) * (FRAME.1 / FRAME.0) as f32;
            let word: Vec<usize> = run.iter().map(|t| t.0).collect();
            // The letters' shadows: the same letter, black, a few thousandths down and right.
            for t in text.iter().filter(|t| t.3) {
                if run.iter().any(|r| r.2 == t.2 && t.1[0] >= r.1[0] - 0.002 && t.1[0] <= r.1[0] + 0.008 && (t.1[1] - r.1[1]).abs() < 0.012) {
                    gone.push(t.0);
                }
            }
            // A letter is two triangles: every primitive on its box.
            gone.extend(text.iter().filter(|t| !t.3 && run.iter().any(|r| r.2 == t.2 && r.1 == t.1)).map(|t| t.0));
            let t = &prims[*word.iter().min().expect("a letter")].1;
            let (xo, yo) = ((t.xyoffset & 0xffff) as f64 / 16.0, (t.xyoffset >> 32 & 0xffff) as f64 / 16.0);
            let corner = |u: f32, v: f32| crate::effects::GsVertex { x: u as f64 * FRAME.0 + xo, y: v as f64 * FRAME.1 + yo, rgba: [0x80; 4], ..t.verts[0] };
            let icon = EePrim { prim: 6 | 0x10 | 0x40 | 0x100, texels: Some(crate::mobies::Upload { src: ICONS + k as u32, psm: 0x13, w: 1, h: 1, regs: None }), clut: None, verts: crate::mobies::Verts::new(&[corner(cx - gw / 2.0, cy - gh / 2.0), corner(cx + gw / 2.0, cy + gh / 2.0)]), ..t.clone() };
            out.plates.push((word.iter().max().expect("a letter") + 1, icon));
        }
    }
    out.hidden.extend(gone);
}

/// A primitive's letter, if it draws one of a HUD font: its font set, character, colour and the game's box for it.
fn drawn(p: &EePrim, text: &crate::text::Text) -> Option<crate::text::Drawn> {
    let set = font_set(p.texels?.src)?;
    if p.clamp & 0xf != 0xa {
        return None;
    }
    let r = |sh: u32| (p.clamp >> sh & 0x3ff) as u32;
    let ch = text.char_at(set, [r(4), r(24), r(14), r(34)])?;
    let (x0, x1) = p.verts.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(v.x), b.max(v.x)));
    let (y0, y1) = p.verts.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(v.y), b.max(v.y)));
    Some(crate::text::Drawn { set, ch, colour: p.verts[p.verts.len() - 1].rgba, game: [x0, y0, x1, y1] })
}

fn key(d: &crate::text::Drawn, clamp: u64) -> (u64, [i64; 4], [u8; 4]) {
    (clamp, d.game.map(|c| (c * 16.0).round() as i64), d.colour)
}

/// The frame's text before its primitives are drawn (`eedraw::build`): the port's lines in place of the game's text
/// (`port_lines`), with the keyboard and mouse in a menu the labels and the shoulder buttons' controls (`menu_labels`,
/// `shoulders`: the answers the window hands the game thread), and the letters laid out (`text::Text::layout`). What it could
/// not make, named.
pub(crate) fn prepare(prims: &[(usize, EePrim)], tex: &mut crate::eedraw::EeTextures) -> Vec<String> {
    tex.prompts.rects.clear();
    tex.prompts.hidden.clear();
    tex.prompts.plates.clear();
    let Some(look) = tex.prompts.look.clone() else { return Vec::new() };
    // The primitives that can be a letter or a glyph (`drawn`, `glyph`): those sampling one of the HUD's fonts or glyph sheets (a
    // few hundred of level 9's 20,000 a frame, which each pass below read whole before).
    let hud: Vec<usize> = prims.iter().enumerate().filter(|(_, (_, p))| p.texels.is_some_and(|u| font_or_sheet(u.src))).map(|(i, _)| i).collect();
    let (mut made, mut problems) = (Vec::new(), Vec::new());
    port_lines(prims, &look, &mut tex.prompts, &mut made, &mut problems);
    let mut answers = Vec::new();
    if look.clicks {
        menu_labels(prims, &look, &mut tex.prompts, &mut answers);
        shoulders(prims, &look, &mut tex.prompts, &mut answers);
    }
    named_buttons(prims, &hud, &look, &mut tex.prompts);
    slot_pictures(prims, &look, &mut tex.prompts);
    boxes(&mut answers);
    *look.answers.lock().expect("answers lock") = answers;
    // Each before the primitive at its index, in the order made (a line's plate before its letters).
    tex.prompts.plates.sort_by_key(|(at, _)| *at);
    let mut letters: Vec<(crate::text::Drawn, u64)> = Vec::new();
    for &i in &hud {
        let p = &prims[i].1;
        if tex.prompts.hidden.contains(&i) {
            continue;
        }
        if let Some(d) = drawn(p, &look.text) {
            // A letter's two triangles once.
            match letters.last() {
                Some((l, c)) if *c == p.clamp && l.game == d.game && l.colour == d.colour => {}
                _ => letters.push((d, p.clamp)),
            }
        }
    }
    let ds: Vec<crate::text::Drawn> = letters.iter().map(|(d, _)| crate::text::Drawn { set: d.set, ch: d.ch, colour: d.colour, game: d.game }).collect();
    for ((d, clamp), r) in letters.iter().zip(look.text.layout(&ds)) {
        if let Some(r) = r {
            tex.prompts.rects.insert(key(d, *clamp), r);
        }
    }
    // The port's lines' letters where `port_lines` typeset them.
    for (d, clamp, r) in made {
        tex.prompts.rects.insert(key(&d, clamp), r);
    }
    problems
}

/// The font set whose font the main program holds at `src`.
pub fn font_set(src: u32) -> Option<usize> {
    SETS.iter().position(|s| s.font == src)
}

/// The button a primitive's glyph shows, and the glyph's cell in its texture ([u0, v0, u1, v1), texels), or None when it is no
/// glyph.
fn glyph(p: &EePrim) -> Option<(Button, [f32; 4], bool)> {
    let (up, clut) = (p.texels?, p.clut);
    // Only a primitive sampling one of the HUD's fonts or glyph sheets can be a glyph: the rest of the frame's (the sea's
    // 20,000 textured triangles among them) go no further, with nothing made for them.
    let s = SETS.iter().find(|s| up.src == s.sheet || up.src == s.font)?;
    // REGION_CLAMP's cell, else the texture coordinates' box.
    let region = (p.clamp & 3 == 2 && p.clamp >> 2 & 3 == 2).then(|| {
        let r = |sh: u32| (p.clamp >> sh & 0x3ff) as f32;
        [r(4), r(24), r(14) + 1.0, r(34) + 1.0]
    });
    let uv = |v: &crate::effects::GsVertex| if p.prim & 0x100 != 0 { (v.u, v.v) } else { (v.s / v.q * (1u32 << (p.tex0 >> 26 & 15)) as f32, v.t / v.q * (1u32 << (p.tex0 >> 30 & 15)) as f32) };
    let (mut lo, mut hi) = ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN));
    for (u, v) in p.verts.iter().map(uv) {
        (lo, hi) = ((lo.0.min(u), lo.1.min(v)), (hi.0.max(u), hi.1.max(v)));
    }
    let mid = ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0);
    if up.src == s.sheet {
        let k = s.palettes.iter().position(|&a| clut.is_some_and(|c| c.src == a))?;
        // The sheet's grid when no region names the cell.
        let cell = region.unwrap_or_else(|| {
            let (c, r) = (((mid.0 - 1.0) / 16.0).floor(), ((mid.1 - 1.0) / 17.0).floor());
            [1.0 + 16.0 * c, 1.0 + 17.0 * r, 17.0 + 16.0 * c, 17.0 + 17.0 * r]
        });
        return Some((SHEET_BUTTONS[k], cell, true));
    }
    let (cu, cv) = match region {
        Some(r) => ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0),
        None => mid,
    };
    let k = s.keys.iter().position(|&(u0, v0, u1, v1)| cu >= u0 as f32 && cu <= u1 as f32 + 1.0 && cv >= v0 as f32 && cv <= v1 as f32 + 1.0)?;
    let (u0, v0, u1, v1) = s.keys[k];
    Some((KEY_BUTTONS[k], region.unwrap_or([u0 as f32, v0 as f32, u1 as f32 + 1.0, v1 as f32 + 1.0]), false))
}

/// `p` drawn with the icon of the active device in place of its glyph, or with the vector font's letter in place of the game
/// font's (`text.rs`), and that atlas's texture index (made in the EE textures the first time); None when `p` is neither or the
/// game's own glyphs and font are shown.
pub(crate) fn swap(p: &EePrim, tex: &mut crate::eedraw::EeTextures, device: &wgpu::Device, queue: &wgpu::Queue) -> Option<(EePrim, u32)> {
    // A named button's icon (`named_buttons`): the atlas's texture, its cell the source's offset.
    if let Some(k) = p.texels?.src.checked_sub(ICONS).filter(|&k| k < 0x1_0000) {
        let look = tex.prompts.look.clone()?;
        let [ax, ay, aw, ah] = look.atlas.cell_at(k as usize);
        let index = atlas_index(tex, &look.atlas, device, queue);
        let mut q = p.clone();
        for (v, (u, w)) in q.verts.iter_mut().zip([(ax, ay), (ax + aw, ay + ah)]) {
            (v.u, v.v) = (u as f32, w as f32);
            (v.s, v.t, v.q) = (u as f32 / look.atlas.size.0 as f32, w as f32 / look.atlas.size.1 as f32, 1.0);
            v.rgba = [0x80; 4];
        }
        q.prim = 6 | 0x10 | 0x40 | 0x100;
        q.tex0 = q.tex0 & !(0xff << 26) | (look.atlas.size.0.trailing_zeros() as u64) << 26 | (look.atlas.size.1.trailing_zeros() as u64) << 30;
        q.clamp = 2 | 2 << 2 | (ax as u64) << 4 | ((ax + aw - 1) as u64) << 14 | (ay as u64) << 24 | ((ay + ah - 1) as u64) << 34;
        (q.texels, q.clut) = (None, None);
        return Some((q, index));
    }
    // A save slot's picture (`slot_pictures`): its texture, made once.
    if let Some(k) = p.texels?.src.checked_sub(PICTURES).map(|k| k as usize).filter(|&k| k < uyafmt::wad38::SLOT_PICTURE_COUNT) {
        let look = tex.prompts.look.clone()?;
        let i = match tex.prompts.pictures.get(&k) {
            Some(&i) => i,
            None => {
                let i = tex.push_texture(device, queue, &look.pictures[k]);
                tex.prompts.pictures.insert(k, i);
                i
            }
        };
        return Some((EePrim { texels: None, ..p.clone() }, i));
    }
    // A glyph or a letter samples one of the HUD's fonts or glyph sheets (`glyph`, `letter`); any other primitive is neither, and
    // goes no further (the sea's 20,000 textured triangles a frame come through here).
    if !font_or_sheet(p.texels?.src) {
        return None;
    }
    let look = tex.prompts.look.clone()?;
    let icon = glyph(p).and_then(|(button, cell, sheet)| look.atlas.icon(look.device, &look.bindings, button).map(|c| (cell, sheet, c)));
    let Some((cell, sheet, [ax, ay, aw, ah])) = icon else { return letter(p, tex, &look, device, queue) };
    let atlas = look.atlas.clone();
    let index = atlas_index(tex, &atlas, device, queue);
    let mut q = p.clone();
    let [fx, fy, fw, fh] = fitted(cell, [ax, ay, aw, ah]);
    let (sx, sy) = (fw / (cell[2] - cell[0]), fh / (cell[3] - cell[1]));
    let uv = |v: &crate::effects::GsVertex| if p.prim & 0x100 != 0 { (v.u, v.v) } else { (v.s / v.q * (1u32 << (p.tex0 >> 26 & 15)) as f32, v.t / v.q * (1u32 << (p.tex0 >> 30 & 15)) as f32) };
    for (v, o) in q.verts.iter_mut().zip(&p.verts) {
        let (u, w) = uv(o);
        (v.u, v.v) = (fx + (u - cell[0]) * sx, fy + (w - cell[1]) * sy);
        (v.s, v.t, v.q) = (v.u / atlas.size.0 as f32, v.v / atlas.size.1 as f32, 1.0);
        if sheet {
            for c in 0..3 {
                v.rgba[c] = (v.rgba[c] as u32 * 0x80 / 0xc0).min(0xff) as u8;
            }
        }
    }
    // FST, TEX0's TW and TH the atlas's, REGION_CLAMP on the icon's cell.
    q.prim |= 0x100;
    q.tex0 = q.tex0 & !(0xff << 26) | (atlas.size.0.trailing_zeros() as u64) << 26 | (atlas.size.1.trailing_zeros() as u64) << 30;
    q.clamp = 2 | 2 << 2 | (ax as u64) << 4 | ((ax + aw - 1) as u64) << 14 | (ay as u64) << 24 | ((ay + ah - 1) as u64) << 34;
    (q.texels, q.clut) = (None, None);
    Some((q, index))
}

/// The atlas's texture, made again when the atlas was (its move keys' cell redrawn: `Atlas::with_moves`).
fn atlas_index(tex: &mut crate::eedraw::EeTextures, atlas: &Atlas, device: &wgpu::Device, queue: &wgpu::Queue) -> u32 {
    match tex.prompts.index {
        Some((v, i)) if v == atlas.version => i,
        _ => {
            let i = tex.push_texture(device, queue, &atlas.texture);
            tex.prompts.index = Some((atlas.version, i));
            i
        }
    }
}

/// The part of the atlas a glyph's cell (u0, v0, u1, v1) is drawn from for the icon at `c` (x, y, width, height): the icon's cell
/// widened or made taller about its middle to the glyph cell's shape, so the icon keeps its own shape in a glyph of another (a key
/// in a shoulder button's keycap, the space bar in a face button's disc); REGION_CLAMP keeps the samples in the icon's cell, whose
/// edge is transparent. Returns x, y, width, height.
pub(crate) fn fitted(glyph: [f32; 4], c: [u32; 4]) -> [f32; 4] {
    let (gw, gh) = (glyph[2] - glyph[0], glyph[3] - glyph[1]);
    let (cw, ch) = (c[2] as f32, c[3] as f32);
    let (w, h) = if cw / ch < gw / gh { (ch * gw / gh, ch) } else { (cw, cw * gh / gw) };
    [c[0] as f32 + (cw - w) / 2.0, c[1] as f32 + (ch - h) / 2.0, w, h]
}

/// `p` (a letter of a HUD font, REGION_CLAMP on its cell) drawn from the vector font's atlas, placed as `text.rs` places it.
fn letter(p: &EePrim, tex: &mut crate::eedraw::EeTextures, look: &Look, device: &wgpu::Device, queue: &wgpu::Queue) -> Option<(EePrim, u32)> {
    let set = font_set(p.texels?.src)?;
    if p.clamp & 0xf != 0xa {
        return None;
    }
    let r = |sh: u32| (p.clamp >> sh & 0x3ff) as u32;
    let placed = look.text.place(set, [r(4), r(24), r(14), r(34)])?;
    let index = match tex.prompts.text_index {
        Some(i) => i,
        None => {
            let i = tex.push_texture(device, queue, &look.text.texture);
            tex.prompts.text_index = Some(i);
            i
        }
    };
    let uv = |v: &crate::effects::GsVertex| if p.prim & 0x100 != 0 { (v.u, v.v) } else { (v.s / v.q * (1u32 << (p.tex0 >> 26 & 15)) as f32, v.t / v.q * (1u32 << (p.tex0 >> 30 & 15)) as f32) };
    let laid = drawn(p, &look.text).and_then(|d| tex.prompts.rects.get(&key(&d, p.clamp)).copied());
    let moved = match laid {
        Some(rect) => crate::text::to_rect(rect, placed.atlas, &p.verts.iter().map(|v| (v.x, v.y)).collect::<Vec<_>>()),
        None => crate::text::corners(&placed, &p.verts.iter().map(|v| (v.x, v.y, uv(v).0, uv(v).1)).collect::<Vec<_>>()),
    };
    let mut q = p.clone();
    let (w, h) = look.text.size;
    for (v, (x, y, u, t)) in q.verts.iter_mut().zip(moved) {
        (v.x, v.y, v.u, v.v) = (x, y, u, t);
        (v.s, v.t, v.q) = (u / w as f32, t / h as f32, 1.0);
    }
    q.prim |= 0x100;
    q.tex0 = q.tex0 & !(0xff << 26) | (w.trailing_zeros() as u64) << 26 | (h.trailing_zeros() as u64) << 30;
    let a = placed.atlas.map(|c| c as u64);
    q.clamp = 2 | 2 << 2 | a[0] << 4 | (a[2] - 1) << 14 | a[1] << 24 | (a[3] - 1) << 34;
    (q.texels, q.clut) = (None, None);
    Some((q, index))
}

/// What an upload in GS memory holds, known by its bytes (`Glyphs`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Held {
    Font(usize),
    Sheet(usize),
    /// The two sets' palettes are the same bytes: the palette's place gives the button.
    Palette(usize),
}

/// The glyph sets' bytes in the main program, to know an upload in the emulated GS's packets (which carry the texels, not where
/// they were uploaded from) as a set's font, sheet or palette: its bytes are the main program's there (`front.rs`).
pub struct Glyphs {
    /// Per set: the font's, the sheet's and the six palettes' bytes.
    bytes: Vec<(Vec<u8>, Vec<u8>, Vec<Vec<u8>>)>,
}

impl Glyphs {
    pub fn new(boot: &uyafmt::boot::Boot) -> Result<Glyphs, String> {
        let b = |a: u32, n: usize| boot.bytes(a, n).map(|x| x.to_vec());
        let bytes = SETS.iter().map(|s| Ok((b(s.font, 256 * 128 / 2)?, b(s.sheet, 64 * 64)?, s.palettes.iter().map(|&p| b(p, 1024)).collect::<Result<Vec<_>, String>>()?))).collect::<Result<Vec<_>, String>>()?;
        Ok(Glyphs { bytes })
    }

    /// What an IMAGE upload of `bytes` (BITBLTBUF's DPSM, TRXREG's width and height) is, if it is one of the glyph sets' parts.
    pub fn held(&self, dpsm: u64, w: u64, h: u64, bytes: &[u8]) -> Option<Held> {
        self.bytes.iter().enumerate().find_map(|(s, (font, sheet, pals))| match (dpsm, w, h) {
            (0x14, 256, 128) if bytes == font.as_slice() => Some(Held::Font(s)),
            (0x13, 64, 64) if bytes == sheet.as_slice() => Some(Held::Sheet(s)),
            (0, 16, 16) => pals.iter().position(|p| bytes == p.as_slice()).map(Held::Palette),
            _ => None,
        })
    }
}

/// The button of a glyph drawn from texels holding `texels` through a palette holding `clut`, on the cell of `clamp`
/// (REGION_CLAMP), or None when it is no glyph.
pub fn button(texels: Option<Held>, clut: Option<Held>, clamp: u64) -> Option<Button> {
    match texels? {
        Held::Sheet(_) => match clut? {
            Held::Palette(k) => Some(SHEET_BUTTONS[k]),
            _ => None,
        },
        Held::Font(s) if clamp & 0xf == 0xa => {
            let r = |sh: u32| (clamp >> sh & 0x3ff) as u32;
            let (cu, cv) = ((r(4) + r(14)) / 2, (r(24) + r(34)) / 2);
            SETS[s].keys.iter().position(|&(u0, v0, u1, v1)| (u0..=u1).contains(&cu) && (v0..=v1).contains(&cv)).map(|k| KEY_BUTTONS[k])
        }
        _ => None,
    }
}

/// Where the main program holds the glyph sets' part `h`: a font or sheet of its own set, a palette of set `set`'s (both sets'
/// palettes are the same bytes; a glyph's palette is its sheet's set's: `glyph`).
pub fn home(h: Held, set: usize) -> u32 {
    match h {
        Held::Font(s) => SETS[s].font,
        Held::Sheet(s) => SETS[s].sheet,
        Held::Palette(k) => SETS[set].palettes[k],
    }
}

/// `EePrim::texels`'s source of the save slots' picture k (`slot_pictures`): the port's own, `swap` gives its texture.
const PICTURES: u32 = 0xE000_0000;

/// `EePrim::texels`'s source of a named button's icon (`named_buttons`): `ICONS` + the atlas cell's index, `swap` gives its texture.
const ICONS: u32 = 0xE100_0000;

/// The save slots' pictures this frame shows (`Look::slots`), each a sprite of the picture over its rectangle and the thin frame a
/// level's SAVE screen draws around it (strips of colour 183, 101, 20 at alpha 51, 1 to 3 pixels outside: Veldin's SAVE screen,
/// 20 November 2026), before the frame's first letter (the slot screen draws its boxes and art, then its text: an empty slot's
/// EMPTY over its picture): drawn into the main frame, blended by their alpha, the picture filtered.
fn slot_pictures(prims: &[(usize, EePrim)], look: &Look, out: &mut Prompts) {
    if look.slots.is_empty() {
        return;
    }
    // The frame's 2D state (its main frame's XYOFFSET): the last primitive into the main frame.
    let Some((at, t)) = prims.iter().rev().find(|(_, p)| p.frame & 0x1ff == 0xF4) else { return };
    let (xo, yo) = ((t.xyoffset & 0xffff) as f64 / 16.0, (t.xyoffset >> 32 & 0xffff) as f64 / 16.0);
    let before = prims.iter().position(|(_, p)| p.texels.is_some_and(|u| font_set(u.src).is_some())).unwrap_or(prims.len());
    for &(r, k) in &look.slots {
        let tx = &look.pictures[k];
        let (w, h) = (tx.width as u32, tx.height as u32);
        let corner = |u: f32, v: f32, s: u32, t: u32| crate::effects::GsVertex { s: 0.0, t: 0.0, q: 1.0, rgba: [0x80; 4], x: xo + u as f64 * FRAME.0, y: yo + v as f64 * FRAME.1, z: 0.0, f: 0, u: s as f32, v: t as f32 };
        let p = EePrim {
            after: *at,
            // A sprite, textured, blended, UV in texels.
            prim: 6 | 0x10 | 0x40 | 0x100,
            frame: 0x8_00F4,
            zbuf: t.zbuf,
            test: 0,
            alpha: 0x44,
            // PSMT8, 2^TW x 2^TH, RGBA, MODULATE; both axes clamped; filtered (MMAG and MMIN linear).
            tex0: 0x13 << 20 | (w.trailing_zeros() as u64) << 26 | (h.trailing_zeros() as u64) << 30 | 1 << 34,
            clamp: 5,
            tex1: 0x60,
            // One level (TEX1 MXL 0).
            mip: [0, 0],
            xyoffset: t.xyoffset,
            scissor: 0x19F_0000_01FF_0000,
            xyoffset1: t.xyoffset1,
            texels: Some(crate::mobies::Upload { src: PICTURES + k as u32, psm: 0x13, w, h, regs: None }),
            clut: None,
            texa: t.texa,
            verts: crate::mobies::Verts::new(&[corner(r[0], r[1], 0, 0), corner(r[2], r[3], w, h)]),
            fill: false,
        };
        out.plates.push((before, p.clone()));
        // The frame: left, right, top and bottom strips (GS pixels about the picture's corners).
        let (px0, py0, px1, py1) = (r[0] as f64 * FRAME.0, r[1] as f64 * FRAME.1, r[2] as f64 * FRAME.0, r[3] as f64 * FRAME.1);
        for [a, b, c, d] in [[px0 - 1.0, py0, px0 + 1.0, py1 + 2.0], [px1, py0, px1 + 2.0, py1 + 2.0], [px0 - 1.0, py0 - 3.0, px1 + 2.0, py0], [px0 - 1.0, py1 + 2.0, px1 + 2.0, py1 + 4.0]] {
            let v = |x: f64, y: f64| crate::effects::GsVertex { s: 0.0, t: 0.0, q: 1.0, rgba: [183, 101, 20, 51], x: xo + x, y: yo + y, z: 0.0, f: 0, u: 0.0, v: 0.0 };
            out.plates.push((before, EePrim { prim: 6 | 0x40, tex0: 0, texels: None, verts: crate::mobies::Verts::new(&[v(a, b), v(c, d)]), ..p.clone() }));
        }
    }
}

/// Whether `src` is where the main program holds one of the HUD's fonts or glyph sheets.
pub fn font_or_sheet(src: u32) -> bool {
    SETS.iter().any(|s| src == s.font || src == s.sheet)
}

/// Whether `button`'s glyph is on the sheet (whose palettes the game brightens by half with the vertex colour 0xC0).
pub fn on_sheet(button: Button) -> bool {
    SHEET_BUTTONS.contains(&button)
}

