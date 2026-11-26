//! Live input for `uya play` (src/play.rs): the keyboard and mouse, Xbox controllers (XInput) and DualSense and DualShock 4
//! controllers (their HID input reports, read straight from the device), merged into the one DualShock 2 the game reads: port 0
//! slot 0, the 18-byte report padman's frames carry (`Cpu::pad`; ee-oracle rc3/pad.rs: buttons active low, the sticks, the 12
//! pressures). The device used last (a button pressed, a stick pushed, a key, a click, the mouse moved) picks the button
//! prompts (src/prompts.rs).
//!
//! Every device is read every field; the controllers are found again every second, so one plugged in or out mid-game is picked
//! up or dropped (hot-plug).
//!
//! Keyboard and mouse: the keys and mouse buttons press the PS2 buttons and push the left stick as the player's bindings say
//! (`bindings.rs`, set on the settings page's KEYBOARD tab); the mouse's motion turns the camera through the mouse aim (below).
//! By default, the game's own lock-strafe layout (uyatr's `CAMERA_SETTING`: while the mouse aims every read of the camera option,
//! the button remaps' too, is lock-strafe):
//!   W A S D  left stick (move)    mouse          the camera (the mouse aim)
//!   Space    Cross (jump)         left button    Circle (fire)
//!   E        Square (wrench)      right button   L2 (first-person aim)
//!   Q        Triangle (quick sel) middle button  R3
//!   Shift    R2 (crouch)          Ctrl           L3
//!   Z        L2 (first-person)    C              R2 (crouch)
//!   Tab      Select (map)         Esc            Start
//!   arrows   d-pad                Enter          Cross (menus)
//!   Backspace Triangle (menus)
//! R1 fires in lock-strafe as Circle does, so no key presses it: Shift pressed R1 until 15 November 2026 and crouched and shot
//! at once (the player: "SHFT shoots gun instead of just being crouch"). A controller's buttons go through its bindings too
//! (`bindings::Bindings::remap`, the CONTROLLER tab).
//!
//! In the game's menus (`menus.rs`) the mouse is a pointer instead: the cursor shows, the item under it is highlighted, the left
//! button is Cross and the right button Triangle (back); its motion does not turn the camera.
//!
//! The mouse aim (PLAN 10; always on with the keyboard and mouse, not a setting): in play the game runs its own lock-strafe
//! camera for the keyboard and mouse (`rtgame.rs`, `uycpu::Host::aim`), and while that camera is the hero's (`Input::direct`)
//! the mouse turns it itself, its motion an angle (`Input::look`). Where the game's follow camera runs instead (the hero's camera
//! not allowed: swimming, a ledge, a vehicle) the mouse turns that, by angle too: `Input::direct` is set while it is the camera in
//! use, and `rtgame.rs` moves its arm (`follow_turn`). The mouse never pushes the right stick, the pad's or a virtual one (the
//! player, 16 November 2026, of a stick push: "i dont want that"); in the menus its motion is dropped.

use crate::bindings::{Bindings, Key};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use winit::keyboard::KeyCode;

/// The DualShock 2's buttons as `Machine::set_pad` numbers them (bit set = pressed).
pub const L2: u16 = 1;
pub const R2: u16 = 2;
pub const L1: u16 = 4;
pub const R1: u16 = 8;
pub const TRIANGLE: u16 = 0x10;
pub const CIRCLE: u16 = 0x20;
pub const CROSS: u16 = 0x40;
pub const SQUARE: u16 = 0x80;
pub const SELECT: u16 = 0x100;
pub const L3: u16 = 0x200;
pub const R3: u16 = 0x400;
pub const START: u16 = 0x800;
pub const UP: u16 = 0x1000;
pub const RIGHT: u16 = 0x2000;
pub const DOWN: u16 = 0x4000;
pub const LEFT: u16 = 0x8000;

/// The kinds of device whose prompts the game shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Device {
    Keyboard,
    Xbox,
    DualSense,
    DualShock4,
}

impl Device {
    pub fn parse(s: &str) -> Result<Device, String> {
        match s {
            "keyboard" => Ok(Device::Keyboard),
            "xbox" => Ok(Device::Xbox),
            "dualsense" => Ok(Device::DualSense),
            "dualshock4" => Ok(Device::DualShock4),
            _ => Err(format!("--prompts keyboard|xbox|dualsense|dualshock4, not {s}")),
        }
    }
}

/// One device's state as a DualShock 2's: buttons, sticks (right x, right y, left x, left y; 0x80 centre, 0 left / up) and the
/// analog L2 and R2 (0 released).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pad {
    pub buttons: u16,
    pub sticks: [u8; 4],
    pub l2: u8,
    pub r2: u8,
}

pub const CENTRE: Pad = Pad { buttons: 0, sticks: [0x80; 4], l2: 0, r2: 0 };

impl Pad {
    /// The report padman's frame carries: buttons active low (the high byte first), the sticks, then the pressures of right,
    /// left, up, down, triangle, circle, cross, square, L1, R1, L2, R2 (0xFF pressed; L2 and R2 as far as they are pulled).
    pub fn report(&self) -> [u8; 18] {
        let b = !self.buttons;
        let mut r = [0u8; 18];
        r[0] = (b >> 8) as u8;
        r[1] = b as u8;
        r[2..6].copy_from_slice(&self.sticks);
        for (k, m) in [RIGHT, LEFT, UP, DOWN, TRIANGLE, CIRCLE, CROSS, SQUARE, L1, R1, L2, R2].iter().enumerate() {
            r[6 + k] = if self.buttons & m != 0 { 0xff } else { 0 };
        }
        r[16] = r[16].max(self.l2);
        r[17] = r[17].max(self.r2);
        r
    }

    /// Whether `self` shows the player using the device since `before`: a button newly pressed, or a stick or trigger pushed past
    /// half way that was not before.
    fn used_since(&self, before: &Pad) -> bool {
        let far = |v: u8| (v as i32 - 0x80).abs() > 0x40;
        self.buttons & !before.buttons != 0 || (0..4).any(|k| far(self.sticks[k]) && !far(before.sticks[k])) || (self.l2 > 0x80 && before.l2 <= 0x80) || (self.r2 > 0x80 && before.r2 <= 0x80)
    }

    /// The merge of two devices: either's buttons, per stick axis the one pushed further, the further pulled trigger.
    fn merge(self, o: &Pad) -> Pad {
        let further = |a: u8, b: u8| if (a as i32 - 0x80).abs() >= (b as i32 - 0x80).abs() { a } else { b };
        Pad { buttons: self.buttons | o.buttons, sticks: [0, 1, 2, 3].map(|k| further(self.sticks[k], o.sticks[k])), l2: self.l2.max(o.l2), r2: self.r2.max(o.r2) }
    }
}

/// An answer the game's box offers, as the 2D layer found it (`front.rs`): where its button is on the picture (shares: x0, y0, x1, y1)
/// and the press its glyph stands for (a pad bit).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Answer {
    pub rect: [f32; 4],
    pub button: u16,
    /// A box's answer (the screen under the box does not hear the pointer), or a control on a screen (the shoulder buttons'
    /// arrows and tabs: the rest of the screen still does).
    pub modal: bool,
}

/// The keyboard and mouse as the window sees them (written by the window's event loop, read once per field).
#[derive(Default)]
pub struct Kbm {
    pub keys: HashSet<KeyCode>,
    /// Left, right, middle, back, forward.
    pub mouse: [bool; 5],
    /// The keys and mouse buttons pressed since the last field: each is held for that field even if it was let go before it (a
    /// tap or a click shorter than a field, 16.7 ms, would else never reach the game).
    pub pressed: HashSet<KeyCode>,
    pub mouse_pressed: [bool; 5],
    /// The mouse wheel since the last field, in notches (up positive); a part of a notch carries to the next field.
    pub wheel: f64,
    /// Raw mouse motion since the last field.
    pub motion: (f64, f64),
    /// Set by any key or click since the last field (the device in use).
    pub touched: bool,
    /// The cursor in the window (pixels; None: outside it), whether it moved and whether the left button was pressed since the
    /// last field, and where the picture is in the window (x, y, width, height: written by the renderer).
    pub cursor: Option<(f64, f64)>,
    pub moved: bool,
    pub click: bool,
    pub viewport: [f32; 4],
    /// The answers on the last frame drawn (written by the renderer) and that frame's field.
    pub answers: (u64, Vec<Answer>),
    /// While the quick-select ring is held open with the keyboard and mouse, where the mouse points on it (`RING_REACH`'s share,
    /// x right, y down: the rim at 1), for the window to draw a pointer there (`play.rs` `Screen::pointer`); None otherwise.
    pub ring_pointer: Option<(f64, f64)>,
    /// While the settings page shows, its tab bar (`menus::Menus::tab_bar`), for the window to draw over the picture (`play.rs`
    /// `Screen::tab_bar`); empty otherwise.
    pub tab_bar: Vec<crate::menus::TabShown>,
}

/// The quick-select ring held open in play (`Input::ring`): the mouse points at a slot as a PC weapon wheel does. Its raw motion
/// (the mouse aim's, counts at the mouse's speed, x right, y down) moves a point kept within `RING_REACH` of the ring's middle,
/// and once the point is `RING_DEAD` out the left stick is pushed all the way toward it: the game's ring highlights the slot that
/// way (its own highlight and sound), and letting go of Q, or a click, takes it (`rtgame.rs`). The window draws a pointer at the
/// point (`Kbm::ring_pointer`; the player, 16 November 2026: "you cant see what your selecting"). Not the window's cursor: pointed
/// by the cursor's place in the window (15 November 2026) the ring stopped taking the mouse (the player, 16 November 2026:
/// "quick select doesnt work on mouse anymore"); by the motion it was checked by real input (HANDOVER, the ring by mouse). The
/// reach was 60 counts, a quarter less mouse travel (the player, 16 November 2026: "lightly reduce the quick select pointer mouse
/// sensitivity"); the dead zone stays a quarter of it.
const RING_REACH: f64 = 80.0;
const RING_DEAD: f64 = 20.0;

/// The camera settings the devices' input is changed by before the game reads it (`settings.rs`, PLAN 10): the mouse's speed (a
/// factor on its motion) and its inversion, the right stick's inversion (left and right, up and down), and the controllers'
/// sticks' dead zones (left, right: shares of a stick's travel, `shape`). The right stick's speed is the game's own
/// (`uyart::set_camera_speed`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Camera {
    pub mouse_speed: f32,
    pub mouse_invert: (bool, bool),
    pub stick_invert: (bool, bool),
    pub dead: (f32, f32),
}

/// As the game takes the devices: no change, and the dead zones `settings.rs` starts with.
pub const CAMERA_GAME: Camera = Camera { mouse_speed: 1.0, mouse_invert: (false, false), stick_invert: (false, false), dead: (0.15, 0.20) };

/// How much of a stick's travel past the dead zone is left for the rim: a stick that stops short of its gate's edge still reads
/// a full push.
const STICK_OUTER: f32 = 0.05;

/// A controller's sticks (`Pad::sticks`: right x, y, then left x, y; 0..255, up 0) as the game is to read them, from the stick's
/// own place. The game reads a byte's distance from 127 as nothing below 48 and as (d - 48) / 76 from there to 124 (its own
/// square dead zone, 38 % of each axis, the DualShock 2's). It measures the left stick's push as the mean of its larger axis and
/// its length, capped at 1, and takes a push measured under 0.25 as none (both found by reading the game's floats for pushes
/// through this path: 0.5526 on each axis measured 0.6671, 0.6053 and 0.1974 measured 0.6209, a push to 0.1875 read 0): with its
/// zone that was a dead zone of 53 % of a stick's travel, and a round-gated stick (an Xbox pad's, the DualSense's) pushed fully on
/// the diagonal measured 0.67, so the player could not run on a diagonal as with the keys (a v0.1.3 player: "stick diagonals
/// don't reach full run speed (unlike KB)", "deadzones feel off"). Here each stick has a round dead zone of its own (`dead`, the
/// settings' MOVE STICK and CAMERA STICK DEAD ZONE), the push past it rescaled to the whole range. The left stick's push is
/// written so that the game measures 0.25 just past the dead zone (its slowest walk) up to 1 at the rim, the same in every
/// direction (the game's measure inverted: 2 / (1 + the larger share of the direction)); the right stick keeps its circle, since
/// the camera turns by each axis alone. Each value is then written as the byte the game's own zone and range read back as
/// exactly that value.
pub fn shape(sticks: [u8; 4], dead: (f32, f32)) -> [u8; 4] {
    let mut out = [0x80; 4];
    for (at, dead, left) in [(0, dead.1, false), (2, dead.0, true)] {
        let x = (sticks[at] as f32 - 127.5) / 127.5;
        let y = (sticks[at + 1] as f32 - 127.5) / 127.5;
        let r = x.hypot(y);
        if r <= dead {
            continue;
        }
        let s = ((r - dead) / (1.0 - dead - STICK_OUTER)).min(1.0);
        let (ux, uy) = (x / r, y / r);
        let k = if left { (0.25 + 0.75 * s) * 2.0 / (1.0 + ux.abs().max(uy.abs())) } else { s };
        let byte = |a: f32| if a == 0.0 { 0x80 } else { (127.0 + a.signum() * (48.0 + 76.0 * a.abs().min(1.0)).round()).clamp(3.0, 251.0) as u8 };
        out[at] = byte(ux * k);
        out[at + 1] = byte(uy * k);
    }
    out
}

impl Pad {
    /// The right stick flipped left and right (`x`) and up and down (`y`) about the game's centre, 127 (`shape`).
    pub fn invert_right(mut self, (x, y): (bool, bool)) -> Pad {
        let flip = |v: u8| (0xFE - v as i32).clamp(0, 0xff) as u8;
        if x {
            self.sticks[0] = flip(self.sticks[0]);
        }
        if y {
            self.sticks[1] = flip(self.sticks[1]);
        }
        self
    }
}

/// The mouse buttons (left, right, middle, back, forward) in the game's menus: the pointer's, whatever the bindings say.
pub const MOUSE_MENUS: [u16; 5] = [CROSS, TRIANGLE, 0, 0, 0];

/// The inputs pressed since the last field (`Input::field`), for a binding being set (`menus.rs`): keys and mouse buttons, and the
/// controllers' buttons as the PS2 buttons they press unchanged.
#[derive(Clone, Default, Debug)]
pub struct Presses {
    pub keys: Vec<Key>,
    pub pad: u16,
}

impl Kbm {
    /// The keyboard and mouse as a pad, through the bindings `b`; in the menus the mouse buttons are Cross and Triangle and the
    /// mouse's motion is dropped. The keys and buttons pressed since the last field go into `fresh`.
    #[allow(clippy::too_many_arguments)]
    fn pad(&mut self, menu: bool, cam: &Camera, b: &Bindings, direct: bool, look: &mut (f64, f64), ring: Option<&mut (f64, f64)>, fresh: &mut Vec<Key>) -> Pad {
        // A key or button is down this field if it is held, or was pressed since the last field (`pressed`).
        let pressed = std::mem::take(&mut self.pressed);
        let mouse_pressed = std::mem::take(&mut self.mouse_pressed);
        fresh.extend(pressed.iter().map(|&c| Key::Key(c)).chain((0..5).filter(|&i| mouse_pressed[i]).map(|i| Key::Mouse(i as u8))));
        let (keys, mouse) = (&self.keys, self.mouse);
        let down = |k: Key| match k {
            Key::Key(c) => keys.contains(&c) || pressed.contains(&c),
            // In the menus the mouse buttons are the pointer's (`MOUSE_MENUS`).
            Key::Mouse(i) => !menu && (mouse[i as usize] || mouse_pressed[i as usize]),
        };
        let (mut buttons, mut lx, mut ly) = b.kbm(down);
        if menu {
            for (i, m) in MOUSE_MENUS.iter().enumerate() {
                if self.mouse[i] || mouse_pressed[i] {
                    buttons |= m;
                }
            }
            self.motion = (0.0, 0.0);
        }
        // The right stick stays centred for the keyboard and mouse: the mouse turns the camera only through the mouse aim.
        if let Some(at) = ring {
            // The quick-select ring: the motion points at a slot (`RING_REACH`), the camera stays.
            let (x, y) = (at.0 + self.motion.0 * cam.mouse_speed as f64, at.1 + self.motion.1 * cam.mouse_speed as f64);
            let len = (x * x + y * y).sqrt();
            *at = if len > RING_REACH { (x / len * RING_REACH, y / len * RING_REACH) } else { (x, y) };
            // The ring takes a slot when the stick, after the game's dead zone, is pushed past 0.9 (quick_select_ring_update,
            // Veldin 0x41AB20, from the pad's +0x148 and +0x14C): the stick is pushed to its square rim the way the point lies, so
            // a diagonal reads (1, 1) and not 0.78 of the way.
            if len.min(RING_REACH) >= RING_DEAD {
                let m = at.0.abs().max(at.1.abs());
                (lx, ly) = ((128.0 + at.0 / m * 127.0).round() as u8, (128.0 + at.1 / m * 127.0).round() as u8);
            }
        } else if direct {
            // The mouse aim: the motion turns the camera itself (counts at the mouse's speed, flipped as its inversion says).
            let flip = |d: f64, f: bool| d * cam.mouse_speed as f64 * if f { -1.0 } else { 1.0 };
            *look = (flip(self.motion.0, cam.mouse_invert.0), flip(self.motion.1, cam.mouse_invert.1));
        }
        self.motion = (0.0, 0.0);
        Pad { buttons, sticks: [0x80, 0x80, lx, ly], l2: 0, r2: 0 }
    }
}

/// A DualSense's or DualShock 4's state, written by its reader thread from each input report.
struct SonyPad {
    device: Device,
    pad: Pad,
}

/// Every input device and the one used last.
pub struct Input {
    pub kbm: Arc<Mutex<Kbm>>,
    /// Each XInput slot's last state (None: no controller there) and when an empty slot is asked again (XInputGetState on an
    /// empty slot is slow, so empty slots are asked once a second).
    xinput: [Option<Pad>; 4],
    xinput_next: [Instant; 4],
    /// The DualSense and DualShock 4 controllers open, by HID device path, and each one's state at the last field.
    sony: Arc<Mutex<HashMap<String, SonyPad>>>,
    sony_before: HashMap<String, Pad>,
    kbm_before: Pad,
    /// The device used last, and a prompt set fixed by `--prompts` (None: the device used last).
    pub last: Device,
    fixed: Option<Device>,
    /// The camera settings (`settings.rs`), set by the game thread from the player's settings.
    pub camera: Camera,
    /// Set by the game thread: the mouse turns the hero's camera itself this field (the mouse aim), and the field's mouse motion
    /// then (counts at the mouse's speed, its inversion applied: x right, y down), which `field` leaves here.
    pub direct: bool,
    pub look: (f64, f64),
    /// Set by the game thread: the quick-select ring is held open in play with the keyboard and mouse, and where the mouse points
    /// on it (`RING_REACH`).
    pub ring: bool,
    ring_at: (f64, f64),
    /// The player's bindings (`bindings.rs`), set by the game thread.
    pub bindings: Bindings,
    /// The control channel's Xbox controller (`control.rs` `controller`): its buttons this field and at the last, and its sticks
    /// as a controller reports them, set by the game thread; read as a plugged-in controller's are.
    pub controller: u16,
    controller_was: u16,
    pub controller_sticks: [u8; 4],
    /// What `field` saw pressed since the last field.
    pub presses: Presses,
}

impl Input {
    pub fn new(fixed: Option<Device>) -> Input {
        let sony = Arc::new(Mutex::new(HashMap::new()));
        let s = sony.clone();
        // The HID controllers are looked for every second on a thread of their own; each one found gets a reader thread.
        std::thread::Builder::new()
            .name("uya pads".into())
            .spawn(move || loop {
                hid::scan(&s);
                std::thread::sleep(Duration::from_secs(1));
            })
            .expect("pad scan thread");
        let now = Instant::now();
        Input { kbm: Arc::new(Mutex::new(Kbm::default())), xinput: [None; 4], xinput_next: [now; 4], sony, sony_before: HashMap::new(), kbm_before: CENTRE, last: fixed.unwrap_or(Device::Keyboard), fixed, camera: CAMERA_GAME, direct: false, look: (0.0, 0.0), ring: false, ring_at: (0.0, 0.0), bindings: Bindings::default(), presses: Presses::default(), controller: 0, controller_was: 0, controller_sticks: [0x80; 4] }
    }

    /// The controller the CONTROLLER tab names buttons for: the device in use if it is a controller, else one plugged in (an Xbox
    /// controller first), else none (`Device::Keyboard`: the PS2's names).
    pub fn pad_kind(&self) -> Device {
        if self.last != Device::Keyboard {
            return self.last;
        }
        if self.xinput.iter().any(Option::is_some) {
            return Device::Xbox;
        }
        self.sony.lock().expect("input lock").values().next().map_or(Device::Keyboard, |s| s.device)
    }

    /// The prompts' device: the one fixed by `--prompts`, else the one used last.
    pub fn prompts(&self) -> Device {
        self.fixed.unwrap_or(self.last)
    }

    /// The answers the last frame drawn offers, and its field.
    pub fn answers(&self) -> (u64, Vec<Answer>) {
        self.kbm.lock().expect("input lock").answers.clone()
    }

    /// Where the mouse points on the game's frame (shares of it) and what it did since the last call. A picture wider than the
    /// original (0.62 / 0.4805: PLAN 4.2) shows the frame's 2D layer unstretched in its middle (`eedraw::unstretched`), so a
    /// point on it is that far from the middle times the widening; off the frame at the sides is off it.
    pub fn pointer(&self) -> crate::menus::Pointer {
        let mut k = self.kbm.lock().expect("input lock");
        let [x, y, w, h] = k.viewport;
        let wide = w / h / (0.62 / 0.4805);
        let at = k.cursor.filter(|_| w > 0.0 && h > 0.0).map(|(cx, cy)| (0.5 + (((cx as f32) - x) / w - 0.5) * wide, ((cy as f32) - y) / h)).filter(|&(u, v)| (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v));
        // Whole notches of the wheel; the part of a notch carries.
        let wheel = k.wheel.trunc();
        k.wheel -= wheel;
        crate::menus::Pointer { at, moved: std::mem::take(&mut k.moved), click: std::mem::take(&mut k.click), down: k.mouse[0], wheel: wheel as i32 }
    }

    /// This field's pad: every device read and merged (`script`: buttons pressed by `--press`; `menu`: the game shows a menu),
    /// and the device used last updated.
    pub fn field(&mut self, script: u16, menu: bool) -> Pad {
        let mut used: Option<Device> = None;
        self.presses = Presses::default();
        let kbm = {
            let mut k = self.kbm.lock().expect("input lock");
            let moved = k.motion.0.abs() + k.motion.1.abs() > 4.0;
            let touched = std::mem::take(&mut k.touched);
            self.look = (0.0, 0.0);
            if !self.ring {
                self.ring_at = (0.0, 0.0);
            }
            let ring = if self.ring && !menu { Some(&mut self.ring_at) } else { None };
            let p = k.pad(menu, &self.camera, &self.bindings, self.direct && !menu, &mut self.look, ring, &mut self.presses.keys);
            k.ring_pointer = (self.ring && !menu).then(|| (self.ring_at.0 / RING_REACH, self.ring_at.1 / RING_REACH));
            if touched || moved || p.used_since(&self.kbm_before) {
                used = Some(Device::Keyboard);
            }
            p
        };
        self.kbm_before = kbm;
        let mut pad = kbm;
        let now = Instant::now();
        for slot in 0..4 {
            if self.xinput[slot].is_none() && now < self.xinput_next[slot] {
                continue;
            }
            // The sticks as the game is to read them (`shape`): a stick resting inside its dead zone does not count as the pad
            // being used.
            let p = xinput(slot as u32).map(|p| Pad { sticks: shape(p.sticks, self.camera.dead), ..p });
            if p.is_none() {
                if self.xinput[slot].is_some() {
                    println!("input: Xbox controller {slot} unplugged");
                }
                self.xinput_next[slot] = now + Duration::from_secs(1);
            } else if self.xinput[slot].is_none() {
                println!("input: Xbox controller {slot} plugged in");
            }
            if let Some(p) = p {
                let was = self.xinput[slot].unwrap_or(CENTRE);
                if p.used_since(&was) {
                    used = Some(Device::Xbox);
                }
                self.presses.pad |= p.buttons & !was.buttons;
                pad = pad.merge(&self.bindings.remap(p).invert_right(self.camera.stick_invert));
            }
            self.xinput[slot] = p;
        }
        {
            let (p, was) = (Pad { buttons: self.controller, sticks: shape(self.controller_sticks, self.camera.dead), ..CENTRE }, Pad { buttons: self.controller_was, ..CENTRE });
            if p.used_since(&was) {
                used = Some(Device::Xbox);
            }
            self.presses.pad |= p.buttons & !was.buttons;
            pad = pad.merge(&self.bindings.remap(p));
            self.controller_was = self.controller;
        }
        {
            let sony = self.sony.lock().expect("input lock");
            self.sony_before.retain(|k, _| sony.contains_key(k));
            for (path, s) in sony.iter() {
                let p = Pad { sticks: shape(s.pad.sticks, self.camera.dead), ..s.pad };
                let was = *self.sony_before.get(path).unwrap_or(&CENTRE);
                if p.used_since(&was) {
                    used = Some(s.device);
                }
                self.presses.pad |= p.buttons & !was.buttons;
                self.sony_before.insert(path.clone(), p);
                pad = pad.merge(&self.bindings.remap(p).invert_right(self.camera.stick_invert));
            }
        }
        if let Some(d) = used {
            if d != self.last {
                println!("input: {d:?} in use");
            }
            self.last = d;
        }
        pad.buttons |= script;
        pad
    }
}

/// XInput slot `slot`'s controller as a DualShock 2 (None when no controller is there): A Cross, B Circle, X Square, Y Triangle,
/// the bumpers L1 and R1, the triggers L2 and R2 (analog), Back Select, Start Start, the stick presses L3 and R3.
fn xinput(slot: u32) -> Option<Pad> {
    use windows::Win32::UI::Input::XboxController::{XINPUT_STATE, XInputGetState};
    let mut s = XINPUT_STATE::default();
    if unsafe { XInputGetState(slot, &mut s) } != 0 {
        return None;
    }
    let g = s.Gamepad;
    let w = g.wButtons.0;
    let map = [(0x1, UP), (0x2, DOWN), (0x4, LEFT), (0x8, RIGHT), (0x10, START), (0x20, SELECT), (0x40, L3), (0x80, R3), (0x100, L1), (0x200, R1), (0x1000, CROSS), (0x2000, CIRCLE), (0x4000, SQUARE), (0x8000, TRIANGLE)];
    let mut buttons = map.iter().filter(|(x, _)| w & x != 0).fold(0, |b, (_, p)| b | p);
    // XInput's trigger threshold (30 of 255) presses the digital L2 and R2.
    if g.bLeftTrigger > 30 {
        buttons |= L2;
    }
    if g.bRightTrigger > 30 {
        buttons |= R2;
    }
    // The axes to 0..255 with up as 0, as the stick reads: the dead zones and the range are `shape`'s.
    let axis = |v: i16, flip: bool| -> u8 {
        let u = ((v as i32 + 32768) >> 8) as u8;
        if flip { 255 - u } else { u }
    };
    let sticks = [axis(g.sThumbRX, false), axis(g.sThumbRY, true), axis(g.sThumbLX, false), axis(g.sThumbLY, true)];
    Some(Pad { buttons, sticks, l2: g.bLeftTrigger, r2: g.bRightTrigger })
}

/// The DualSense and DualShock 4 controllers through Windows' HID API: every HID interface of Sony's (vendor 0x054C) that is a
/// game pad (usage page 1, usage 5), opened and read on a thread of its own until it is unplugged.
mod hid {
    use super::{Device, Pad, SonyPad, CENTRE};
    use super::{CIRCLE, CROSS, DOWN, L1, L2, L3, LEFT, R1, R2, R3, RIGHT, SELECT, SQUARE, START, TRIANGLE, UP};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use windows::Win32::Devices::DeviceAndDriverInstallation::{SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W};
    use windows::Win32::Devices::HumanInterfaceDevice::{HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetHidGuid, HidD_GetPreparsedData, HidP_GetCaps, HIDD_ATTRIBUTES, HIDP_CAPS, PHIDP_PREPARSED_DATA};
    use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE};
    use windows::Win32::Storage::FileSystem::{CreateFileW, ReadFile, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
    use windows::core::PCWSTR;

    /// Sony's product ids: the DualSense and DualSense Edge, the DualShock 4 (both models and the wireless adapter).
    fn kind(pid: u16) -> Option<Device> {
        match pid {
            0x0CE6 | 0x0DF2 => Some(Device::DualSense),
            0x05C4 | 0x09CC | 0x0BA0 => Some(Device::DualShock4),
            _ => None,
        }
    }

    /// The Sony interfaces that could not be opened, each said once.
    static NOT_OPENED: Mutex<Vec<String>> = Mutex::new(Vec::new());

    /// Opens every Sony game pad not open yet. A pad's interface path names Sony's vendor id as `vid_054c&pid_` over USB and as
    /// `_vid&0002054c_pid&` over Bluetooth (`{00001124-...}`, the HID service's id, before it): only the USB form was looked for,
    /// so a DualSense paired by Bluetooth, the usual way on a PC, was never opened and nothing said why (a v0.1.3 player:
    /// "DualSense not recognized"). The pad is opened to read only (the port writes nothing to it), so another program reading it
    /// too does not keep it closed; one that cannot be opened (held by a program that wraps it, such as Steam Input or DS4Windows,
    /// which then shows it as an Xbox pad) is said in the log.
    pub fn scan(open: &Arc<Mutex<HashMap<String, SonyPad>>>) {
        for path in interfaces() {
            let p = path.to_ascii_lowercase();
            if !(p.contains("054c&pid") || p.contains("054c_pid")) || open.lock().expect("input lock").contains_key(&path) {
                continue;
            }
            let bluetooth = !p.contains("vid_054c");
            let wide: Vec<u16> = path.encode_utf16().chain([0]).collect();
            let h = match unsafe { CreateFileW(PCWSTR(wide.as_ptr()), GENERIC_READ.0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None) } {
                Ok(h) => h,
                Err(e) => {
                    let mut said = NOT_OPENED.lock().expect("input lock");
                    if !said.contains(&path) {
                        println!("input: a Sony pad's interface could not be opened ({path}): {e}");
                        said.push(path);
                    }
                    continue;
                }
            };
            let mut attrs = HIDD_ATTRIBUTES { Size: std::mem::size_of::<HIDD_ATTRIBUTES>() as u32, ..Default::default() };
            let mut caps = HIDP_CAPS::default();
            let ok = unsafe {
                let mut pre = PHIDP_PREPARSED_DATA::default();
                let got = HidD_GetAttributes(h, &mut attrs) && HidD_GetPreparsedData(h, &mut pre);
                let capped = got && HidP_GetCaps(pre, &mut caps).0 == 0x0011_0000;
                if got {
                    let _ = HidD_FreePreparsedData(pre);
                }
                capped
            };
            let device = kind(attrs.ProductID).filter(|_| attrs.VendorID == 0x054C);
            match (ok && caps.UsagePage == 1 && caps.Usage == 5, device) {
                (true, Some(device)) => {
                    println!("input: {device:?} found ({path}, {}, input reports of {} bytes)", if bluetooth { "Bluetooth" } else { "USB" }, caps.InputReportByteLength);
                    open.lock().expect("input lock").insert(path.clone(), SonyPad { device, pad: CENTRE });
                    let (open, len, raw) = (open.clone(), caps.InputReportByteLength as usize, h.0 as isize);
                    std::thread::Builder::new().name("uya pad".into()).spawn(move || read(HANDLE(raw as *mut _), len, device, bluetooth, &path, &open)).expect("pad reader thread");
                }
                _ => unsafe {
                    let _ = CloseHandle(h);
                },
            }
        }
    }

    /// Every present HID interface's device path.
    fn interfaces() -> Vec<String> {
        let mut paths = Vec::new();
        unsafe {
            let guid = HidD_GetHidGuid();
            let Ok(set) = SetupDiGetClassDevsW(Some(&guid), PCWSTR::null(), None, DIGCF_PRESENT | DIGCF_DEVICEINTERFACE) else { return paths };
            for i in 0.. {
                let mut d = SP_DEVICE_INTERFACE_DATA { cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32, ..Default::default() };
                if SetupDiEnumDeviceInterfaces(set, None, &guid, i, &mut d).is_err() {
                    break;
                }
                let mut need = 0u32;
                let _ = SetupDiGetDeviceInterfaceDetailW(set, &d, None, 0, Some(&mut need), None);
                // The detail record: its size word, then the path's UTF-16 characters, 8-byte aligned.
                let mut buf = vec![0u64; (need as usize).div_ceil(8).max(1)];
                let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
                (*detail).cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
                if SetupDiGetDeviceInterfaceDetailW(set, &d, Some(detail), need, None, None).is_err() {
                    continue;
                }
                let p = std::ptr::addr_of!((*detail).DevicePath) as *const u16;
                let n = (0..).take_while(|&k| *p.add(k) != 0).count();
                paths.push(String::from_utf16_lossy(std::slice::from_raw_parts(p, n)));
            }
            let _ = SetupDiDestroyDeviceInfoList(set);
        }
        paths
    }

    /// Reads the pad's input reports until it is unplugged, keeping its state in `open`.
    fn read(h: HANDLE, len: usize, device: Device, bluetooth: bool, path: &str, open: &Arc<Mutex<HashMap<String, SonyPad>>>) {
        let mut buf = vec![0u8; len.max(64)];
        loop {
            let mut n = 0u32;
            if unsafe { ReadFile(h, Some(&mut buf[..len]), Some(&mut n), None) }.is_err() {
                break;
            }
            if let Some(p) = report(device, bluetooth, &buf[..n as usize]) {
                if let Some(s) = open.lock().expect("input lock").get_mut(path) {
                    s.pad = p;
                }
            }
        }
        println!("input: {device:?} unplugged ({path})");
        open.lock().expect("input lock").remove(path);
        unsafe {
            let _ = CloseHandle(h);
        }
    }

    /// One input report as a DualShock 2. The DualSense's full report (0x01 over USB, 0x31 over Bluetooth one byte further on):
    /// sticks, L2, R2, a counter, then the button bytes; the DualShock 4's (0x01, 0x11 over Bluetooth two bytes further on) and
    /// either pad's short Bluetooth report (0x01 of 10 bytes): sticks, the button bytes, then L2 and R2. Button bytes: the d-pad
    /// as a hat (0 up, clockwise, 8 none) and Square, Cross, Circle, Triangle; L1, R1, L2, R2, Share / Create, Options, L3, R3;
    /// PS and the touchpad's click. Share / Create and the touchpad press Select, Options Start. Which layout a 0x01 report has
    /// is the connection's, not the report's length: over Bluetooth it is always the short one (Windows may hand it over padded
    /// to the interface's report length, which the DualSense's full USB report shares).
    fn report(device: Device, bluetooth: bool, d: &[u8]) -> Option<Pad> {
        let (o, full) = match (device, bluetooth, d.first()?, d.len()) {
            (Device::DualSense, false, 0x01, n) if n >= 64 => (1, true),
            (Device::DualSense, true, 0x31, n) if n >= 12 => (2, true),
            (Device::DualShock4, false, 0x01, n) if n >= 10 => (1, false),
            (_, true, 0x01, n) if n >= 10 => (1, false),
            (Device::DualShock4, true, 0x11, n) if n >= 12 => (3, false),
            _ => return None,
        };
        let (b0, b1, b2, l2, r2) = if full { (d[o + 7], d[o + 8], d[o + 9], d[o + 4], d[o + 5]) } else { (d[o + 4], d[o + 5], d[o + 6], d[o + 7], d[o + 8]) };
        let hat = [UP, UP | RIGHT, RIGHT, DOWN | RIGHT, DOWN, DOWN | LEFT, LEFT, UP | LEFT];
        let mut buttons = hat.get((b0 & 15) as usize).copied().unwrap_or(0);
        for (bit, b) in [(0x10, SQUARE), (0x20, CROSS), (0x40, CIRCLE), (0x80, TRIANGLE)] {
            if b0 & bit != 0 {
                buttons |= b;
            }
        }
        for (bit, b) in [(1, L1), (2, R1), (4, L2), (8, R2), (0x10, SELECT), (0x20, START), (0x40, L3), (0x80, R3)] {
            if b1 & bit != 0 {
                buttons |= b;
            }
        }
        if b2 & 2 != 0 {
            buttons |= SELECT;
        }
        Some(Pad { buttons, sticks: [d[o + 2], d[o + 3], d[o], d[o + 1]], l2, r2 })
    }
}
