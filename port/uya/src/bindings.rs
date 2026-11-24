//! The player's key, mouse and controller bindings (PLAN 10; the player, 15 November 2026: "I want to be able to set keyboard
//! and mouse and controller key bindings in game"), saved in the profile's `bindings.txt` as soon as one changes and set on the
//! settings page's KEYBOARD and CONTROLLER tabs (`menus.rs`).
//!
//! - The keyboard and mouse: each action of `ACTIONS` (a PS2 button pressed, or the left stick pushed one way) has two slots,
//!   each a key or a mouse button (`Key`). An input is in one slot at most: binding it to a slot takes it from the slot it was
//!   in. The defaults are the port's layout of PLAN 10 (`input.rs`'s table).
//! - A controller: each PS2 button of `PAD` is pressed by one of the controller's buttons. A controller button is named here by
//!   the PS2 button it presses with nothing changed (an Xbox controller's A is Cross, a DualSense's Cross is Cross: `input.rs`
//!   reads them so), and shown by its own name (`pad_name`). Binding a controller button to a PS2 button swaps it with the one
//!   bound there, so every PS2 button keeps a controller button.
//!
//! The prompts show what is bound (`prompts.rs`): on the keyboard the glyph of a PS2 button is the key of the action that
//! presses it (`Bindings::shown`), on a controller the controller button bound to it.

use crate::input::{Device, Pad, CIRCLE, CROSS, DOWN, L1, L2, L3, LEFT, R1, R2, R3, RIGHT, SELECT, SQUARE, START, TRIANGLE, UP};
use winit::keyboard::KeyCode;

/// A key, or a mouse button (0 left, 1 right, 2 middle, 3 back, 4 forward).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Key {
    Key(KeyCode),
    Mouse(u8),
}

/// What an action does: presses a PS2 button, or pushes the left stick one way (x, y: -1 left or up, 1 right or down).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Drive {
    Button(u16),
    Stick(i8, i8),
}

pub struct Action {
    /// The action's name in `bindings.txt`.
    pub key: &'static str,
    /// The row's label.
    pub name: &'static str,
    pub drive: Drive,
    pub default: [Option<Key>; 2],
}

const fn k(c: KeyCode) -> Option<Key> {
    Some(Key::Key(c))
}

/// The keyboard and mouse's actions, in the KEYBOARD tab's order. In play the game runs its lock-strafe layout for the keyboard
/// and mouse (`rtgame.rs`): Cross jumps, Circle fires, Square swings the wrench, Triangle held is the quick select, R2 crouches
/// and L2 is the first-person aim (L1 and R1 jump and fire there too, so no key needs them).
pub const ACTIONS: &[Action] = &[
    Action { key: "move_forward", name: "MOVE FORWARD", drive: Drive::Stick(0, -1), default: [k(KeyCode::KeyW), None] },
    Action { key: "move_back", name: "MOVE BACK", drive: Drive::Stick(0, 1), default: [k(KeyCode::KeyS), None] },
    Action { key: "move_left", name: "MOVE LEFT", drive: Drive::Stick(-1, 0), default: [k(KeyCode::KeyA), None] },
    Action { key: "move_right", name: "MOVE RIGHT", drive: Drive::Stick(1, 0), default: [k(KeyCode::KeyD), None] },
    Action { key: "jump", name: "JUMP", drive: Drive::Button(CROSS), default: [k(KeyCode::Space), k(KeyCode::Enter)] },
    Action { key: "fire", name: "FIRE", drive: Drive::Button(CIRCLE), default: [Some(Key::Mouse(0)), None] },
    Action { key: "wrench", name: "WRENCH", drive: Drive::Button(SQUARE), default: [k(KeyCode::KeyE), None] },
    Action { key: "quick_select", name: "QUICK SELECT", drive: Drive::Button(TRIANGLE), default: [k(KeyCode::KeyQ), k(KeyCode::Backspace)] },
    Action { key: "crouch", name: "CROUCH", drive: Drive::Button(R2), default: [k(KeyCode::ShiftLeft), k(KeyCode::KeyC)] },
    Action { key: "aim", name: "FIRST-PERSON AIM", drive: Drive::Button(L2), default: [Some(Key::Mouse(1)), k(KeyCode::KeyZ)] },
    Action { key: "map", name: "MAP", drive: Drive::Button(SELECT), default: [k(KeyCode::Tab), None] },
    Action { key: "pause", name: "PAUSE", drive: Drive::Button(START), default: [k(KeyCode::Escape), None] },
    Action { key: "l3", name: "L3", drive: Drive::Button(L3), default: [k(KeyCode::ControlLeft), None] },
    Action { key: "r3", name: "R3", drive: Drive::Button(R3), default: [Some(Key::Mouse(2)), None] },
    Action { key: "up", name: "D-PAD UP", drive: Drive::Button(UP), default: [k(KeyCode::ArrowUp), None] },
    Action { key: "down", name: "D-PAD DOWN", drive: Drive::Button(DOWN), default: [k(KeyCode::ArrowDown), None] },
    Action { key: "left", name: "D-PAD LEFT", drive: Drive::Button(LEFT), default: [k(KeyCode::ArrowLeft), None] },
    Action { key: "right", name: "D-PAD RIGHT", drive: Drive::Button(RIGHT), default: [k(KeyCode::ArrowRight), None] },
];

/// The CONTROLLER tab's rows, the PS2 buttons: the name in `bindings.txt` (also a controller button's there), the row's label
/// and the button. With a controller the game plays its own camera option's layout.
pub const PAD: [(&str, &str, u16); 16] = [
    ("cross", "CROSS (JUMP)", CROSS),
    ("circle", "CIRCLE (FIRE)", CIRCLE),
    ("square", "SQUARE (WRENCH)", SQUARE),
    ("triangle", "TRIANGLE (WEAPONS)", TRIANGLE),
    ("l1", "L1", L1),
    ("r1", "R1", R1),
    ("l2", "L2", L2),
    ("r2", "R2", R2),
    ("l3", "L3", L3),
    ("r3", "R3", R3),
    ("select", "SELECT (MAP)", SELECT),
    ("start", "START (PAUSE)", START),
    ("up", "D-PAD UP", UP),
    ("down", "D-PAD DOWN", DOWN),
    ("left", "D-PAD LEFT", LEFT),
    ("right", "D-PAD RIGHT", RIGHT),
];

/// The keys a slot takes, each with the name its plate shows (`bindings.txt` and the prompt atlas name a key by winit's name
/// for it, `KeyCode`'s Debug form: "KeyW", "ShiftLeft").
const KEYS: &[(KeyCode, &str)] = &[
    (KeyCode::KeyA, "A"),
    (KeyCode::KeyB, "B"),
    (KeyCode::KeyC, "C"),
    (KeyCode::KeyD, "D"),
    (KeyCode::KeyE, "E"),
    (KeyCode::KeyF, "F"),
    (KeyCode::KeyG, "G"),
    (KeyCode::KeyH, "H"),
    (KeyCode::KeyI, "I"),
    (KeyCode::KeyJ, "J"),
    (KeyCode::KeyK, "K"),
    (KeyCode::KeyL, "L"),
    (KeyCode::KeyM, "M"),
    (KeyCode::KeyN, "N"),
    (KeyCode::KeyO, "O"),
    (KeyCode::KeyP, "P"),
    (KeyCode::KeyQ, "Q"),
    (KeyCode::KeyR, "R"),
    (KeyCode::KeyS, "S"),
    (KeyCode::KeyT, "T"),
    (KeyCode::KeyU, "U"),
    (KeyCode::KeyV, "V"),
    (KeyCode::KeyW, "W"),
    (KeyCode::KeyX, "X"),
    (KeyCode::KeyY, "Y"),
    (KeyCode::KeyZ, "Z"),
    (KeyCode::Digit0, "0"),
    (KeyCode::Digit1, "1"),
    (KeyCode::Digit2, "2"),
    (KeyCode::Digit3, "3"),
    (KeyCode::Digit4, "4"),
    (KeyCode::Digit5, "5"),
    (KeyCode::Digit6, "6"),
    (KeyCode::Digit7, "7"),
    (KeyCode::Digit8, "8"),
    (KeyCode::Digit9, "9"),
    (KeyCode::F1, "F1"),
    (KeyCode::F2, "F2"),
    (KeyCode::F3, "F3"),
    (KeyCode::F4, "F4"),
    (KeyCode::F5, "F5"),
    (KeyCode::F6, "F6"),
    (KeyCode::F7, "F7"),
    (KeyCode::F8, "F8"),
    (KeyCode::F9, "F9"),
    (KeyCode::F10, "F10"),
    (KeyCode::F11, "F11"),
    (KeyCode::F12, "F12"),
    (KeyCode::ArrowUp, "UP"),
    (KeyCode::ArrowDown, "DOWN"),
    (KeyCode::ArrowLeft, "LEFT"),
    (KeyCode::ArrowRight, "RIGHT"),
    (KeyCode::Space, "SPACE"),
    (KeyCode::Enter, "ENTER"),
    (KeyCode::Tab, "TAB"),
    (KeyCode::Backspace, "BACKSPACE"),
    (KeyCode::Escape, "ESC"),
    (KeyCode::CapsLock, "CAPS LOCK"),
    (KeyCode::ShiftLeft, "LEFT SHIFT"),
    (KeyCode::ShiftRight, "RIGHT SHIFT"),
    (KeyCode::ControlLeft, "LEFT CTRL"),
    (KeyCode::ControlRight, "RIGHT CTRL"),
    (KeyCode::AltLeft, "LEFT ALT"),
    (KeyCode::AltRight, "RIGHT ALT"),
    (KeyCode::Backquote, "GRAVE"),
    (KeyCode::Minus, "MINUS"),
    (KeyCode::Equal, "EQUALS"),
    (KeyCode::BracketLeft, "L BRACKET"),
    (KeyCode::BracketRight, "R BRACKET"),
    (KeyCode::Backslash, "BACKSLASH"),
    (KeyCode::Semicolon, "SEMICOLON"),
    (KeyCode::Quote, "QUOTE"),
    (KeyCode::Comma, "COMMA"),
    (KeyCode::Period, "PERIOD"),
    (KeyCode::Slash, "SLASH"),
    (KeyCode::Insert, "INSERT"),
    (KeyCode::Delete, "DELETE"),
    (KeyCode::Home, "HOME"),
    (KeyCode::End, "END"),
    (KeyCode::PageUp, "PAGE UP"),
    (KeyCode::PageDown, "PAGE DOWN"),
    (KeyCode::Numpad0, "NUM 0"),
    (KeyCode::Numpad1, "NUM 1"),
    (KeyCode::Numpad2, "NUM 2"),
    (KeyCode::Numpad3, "NUM 3"),
    (KeyCode::Numpad4, "NUM 4"),
    (KeyCode::Numpad5, "NUM 5"),
    (KeyCode::Numpad6, "NUM 6"),
    (KeyCode::Numpad7, "NUM 7"),
    (KeyCode::Numpad8, "NUM 8"),
    (KeyCode::Numpad9, "NUM 9"),
    (KeyCode::NumpadAdd, "NUM PLUS"),
    (KeyCode::NumpadSubtract, "NUM MINUS"),
    (KeyCode::NumpadMultiply, "NUM STAR"),
    (KeyCode::NumpadDivide, "NUM SLASH"),
    (KeyCode::NumpadDecimal, "NUM PERIOD"),
    (KeyCode::NumpadEnter, "NUM ENTER"),
];

/// The mouse buttons' names in `bindings.txt` and the atlas, and on their plates.
const MOUSE: [(&str, &str); 5] = [("MouseLeft", "LEFT MOUSE"), ("MouseRight", "RIGHT MOUSE"), ("MouseMiddle", "MIDDLE MOUSE"), ("MouseBack", "MOUSE 4"), ("MouseForward", "MOUSE 5")];

impl Key {
    /// Whether a slot takes this input (a key of `KEYS`, or a mouse button).
    pub fn bindable(self) -> bool {
        match self {
            Key::Key(c) => KEYS.iter().any(|(k, _)| *k == c),
            Key::Mouse(m) => (m as usize) < MOUSE.len(),
        }
    }

    /// Its name in `bindings.txt` and the prompt atlas.
    pub fn name(self) -> String {
        match self {
            Key::Key(c) => format!("{c:?}"),
            Key::Mouse(m) => MOUSE[m as usize].0.to_string(),
        }
    }

    /// The name its plate shows.
    pub fn label(self) -> &'static str {
        match self {
            Key::Key(c) => KEYS.iter().find(|(k, _)| *k == c).map(|(_, l)| *l).unwrap_or_else(|| panic!("bindings: {c:?} is no key a slot takes")),
            Key::Mouse(m) => MOUSE[m as usize].1,
        }
    }

    /// The input `name` names (`Key::name`).
    fn parse(name: &str) -> Option<Key> {
        if let Some(m) = MOUSE.iter().position(|(n, _)| *n == name) {
            return Some(Key::Mouse(m as u8));
        }
        KEYS.iter().find(|(k, _)| format!("{k:?}") == name).map(|(k, _)| Key::Key(*k))
    }
}

/// Every key and mouse button a slot takes (the prompt atlas has an icon for each: `scripts/prompt-icons.py`).
pub fn all_keys() -> impl Iterator<Item = Key> {
    KEYS.iter().map(|(k, _)| Key::Key(*k)).chain((0..MOUSE.len() as u8).map(Key::Mouse))
}

/// A controller button (by the PS2 button it presses unchanged) as the controller names it: an Xbox controller's A, LB, VIEW; a
/// DualSense's CROSS, L1, CREATE; a DualShock 4's SHARE; with no controller in use the PS2's names.
pub fn pad_name(device: Device, button: u16) -> &'static str {
    let i = PAD.iter().position(|p| p.2 == button).unwrap_or_else(|| panic!("bindings: {button:#x} is no PS2 button"));
    const XBOX: [&str; 16] = ["A", "B", "X", "Y", "LB", "RB", "LT", "RT", "LS", "RS", "VIEW", "MENU", "D-PAD UP", "D-PAD DOWN", "D-PAD LEFT", "D-PAD RIGHT"];
    const PS: [&str; 16] = ["CROSS", "CIRCLE", "SQUARE", "TRIANGLE", "L1", "R1", "L2", "R2", "L3", "R3", "SELECT", "START", "D-PAD UP", "D-PAD DOWN", "D-PAD LEFT", "D-PAD RIGHT"];
    match (device, i) {
        (Device::Xbox, _) => XBOX[i],
        (Device::DualSense, 10) => "CREATE",
        (Device::DualShock4, 10) => "SHARE",
        (Device::DualSense | Device::DualShock4, 11) => "OPTIONS",
        _ => PS[i],
    }
}

const NA: usize = ACTIONS.len();

/// The bindings: each action's two slots (by its place in `ACTIONS`), and the controller button (by the PS2 button it presses
/// unchanged) that presses each of `PAD`'s PS2 buttons.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bindings {
    pub keys: [[Option<Key>; 2]; NA],
    pub pad: [u16; 16],
}

impl Default for Bindings {
    /// The port's own layout, and each controller button pressing its own PS2 button.
    fn default() -> Bindings {
        let mut keys = [[None; 2]; NA];
        for (s, a) in keys.iter_mut().zip(ACTIONS) {
            *s = a.default;
        }
        Bindings { keys, pad: PAD.map(|p| p.2) }
    }
}

impl Bindings {
    /// `key` into action `a`'s slot `slot`, taken from any slot it was in.
    pub fn set_key(&mut self, a: usize, slot: usize, key: Key) {
        for s in self.keys.iter_mut().flatten() {
            if *s == Some(key) {
                *s = None;
            }
        }
        self.keys[a][slot] = Some(key);
    }

    pub fn clear(&mut self, a: usize, slot: usize) {
        self.keys[a][slot] = None;
    }

    /// Controller button `button` to PS2 button `row` of `PAD`; the row it pressed takes `row`'s button.
    pub fn set_pad(&mut self, row: usize, button: u16) {
        if let Some(j) = self.pad.iter().position(|&b| b == button) {
            self.pad[j] = self.pad[row];
        }
        self.pad[row] = button;
    }

    /// The keyboard and mouse back to the port's layout.
    pub fn reset_keys(&mut self) {
        self.keys = Bindings::default().keys;
    }

    /// The controller back to the game's own buttons.
    pub fn reset_pad(&mut self) {
        self.pad = Bindings::default().pad;
    }

    /// The keyboard and mouse as a PS2 pad's buttons and left stick (x, y: 0 left or up, 0x80 centre, 0xFF right or down),
    /// `down` saying which inputs are down.
    pub fn kbm(&self, down: impl Fn(Key) -> bool) -> (u16, u8, u8) {
        let (mut buttons, mut x, mut y) = (0u16, 0i32, 0i32);
        for (slots, a) in self.keys.iter().zip(ACTIONS) {
            if !slots.iter().flatten().any(|&k| down(k)) {
                continue;
            }
            match a.drive {
                Drive::Button(b) => buttons |= b,
                Drive::Stick(dx, dy) => (x, y) = (x + dx as i32, y + dy as i32),
            }
        }
        let axis = |d: i32| match d.signum() {
            -1 => 0x00,
            1 => 0xff,
            _ => 0x80,
        };
        (buttons, axis(x), axis(y))
    }

    /// A controller's pad (its buttons as the PS2 buttons they press unchanged) through the bindings; the analog L2 and R2 are those
    /// of the buttons bound to them (a trigger's own pull, a button's all or nothing).
    pub fn remap(&self, p: Pad) -> Pad {
        let mut buttons = 0;
        for (row, &b) in PAD.iter().zip(&self.pad) {
            if p.buttons & b != 0 {
                buttons |= row.2;
            }
        }
        let analog = |b: u16| match b {
            L2 => p.l2,
            R2 => p.r2,
            _ if p.buttons & b != 0 => 0xff,
            _ => 0,
        };
        let at = |ps2: u16| self.pad[PAD.iter().position(|r| r.2 == ps2).expect("L2 and R2 are rows")];
        Pad { buttons, sticks: p.sticks, l2: analog(at(L2)), r2: analog(at(R2)) }
    }

    /// The input a keyboard prompt of PS2 button `button` shows: the first slot set of the action that presses it.
    pub fn shown(&self, button: u16) -> Option<Key> {
        ACTIONS.iter().zip(&self.keys).filter(|(a, _)| a.drive == Drive::Button(button)).find_map(|(_, s)| s.iter().flatten().next().copied())
    }

    /// The keys a keyboard prompt of the left stick shows: the first slot set of MOVE FORWARD, LEFT, BACK and RIGHT.
    pub fn moves(&self) -> [Option<Key>; 4] {
        [(0, -1), (-1, 0), (0, 1), (1, 0)].map(|d| ACTIONS.iter().zip(&self.keys).find(|(a, _)| a.drive == Drive::Stick(d.0, d.1)).and_then(|(_, s)| s.iter().flatten().next().copied()))
    }

    /// The controller button that presses PS2 button `button`.
    pub fn pad_for(&self, button: u16) -> u16 {
        self.pad[PAD.iter().position(|r| r.2 == button).unwrap_or_else(|| panic!("bindings: {button:#x} is no PS2 button"))]
    }

    /// The profile's bindings (`name = value` lines: an action's two slots `KEY, KEY` with `none` for an empty one, a PS2 button's
    /// controller button as `pad.NAME = NAME`); a profile without the file has set none yet, and what the file does not name is
    /// at its default.
    pub fn load(path: &str) -> Result<Bindings, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Bindings::default()),
            Err(e) => return Err(format!("{path}: {e}")),
        };
        let mut b = Bindings::default();
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let (name, value) = line.split_once('=').map(|(k, v)| (k.trim(), v.trim())).ok_or_else(|| format!("{path}: \"{line}\" is not name = value"))?;
            if let Some(row) = name.strip_prefix("pad.") {
                let r = PAD.iter().position(|p| p.0 == row).ok_or_else(|| format!("{path}: no PS2 button \"{row}\""))?;
                let button = PAD.iter().find(|p| p.0 == value).ok_or_else(|| format!("{path}: {name} = {value}: no controller button \"{value}\""))?.2;
                b.pad[r] = button;
                continue;
            }
            let a = ACTIONS.iter().position(|a| a.key == name).ok_or_else(|| format!("{path}: no action \"{name}\""))?;
            let slots: Vec<&str> = value.split(',').map(str::trim).collect();
            if slots.len() != 2 {
                return Err(format!("{path}: {name} = {value}: not two slots (KEY, KEY; none for an empty one)"));
            }
            for (s, v) in slots.iter().enumerate() {
                b.keys[a][s] = match *v {
                    "none" => None,
                    v => Some(Key::parse(v).ok_or_else(|| format!("{path}: {name} = {value}: no key \"{v}\""))?),
                };
            }
        }
        // Every PS2 button keeps one controller button (a file whose rows name one twice is not one the page wrote).
        let mut seen = 0u16;
        for &p in &b.pad {
            if seen & p != 0 {
                return Err(format!("{path}: one controller button bound to two PS2 buttons"));
            }
            seen |= p;
        }
        Ok(b)
    }

    pub fn save(&self, path: &str) -> Result<(), String> {
        let slot = |k: Option<Key>| k.map_or("none".to_string(), Key::name);
        let mut text: String = ACTIONS.iter().zip(&self.keys).map(|(a, s)| format!("{} = {}, {}\n", a.key, slot(s[0]), slot(s[1]))).collect();
        for (row, &b) in PAD.iter().zip(&self.pad) {
            let name = PAD.iter().find(|p| p.2 == b).expect("a PS2 button").0;
            text.push_str(&format!("pad.{} = {name}\n", row.0));
        }
        std::fs::write(path, text).map_err(|e| format!("{path}: {e}"))
    }
}
