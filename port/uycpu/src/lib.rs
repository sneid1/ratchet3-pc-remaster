//! uycpu: what the game's translated code is compiled against (PLAN items 0.3, 5.3). The translated programs are linked two
//! ways: the loader, the boot program and both front ends into the game's executable (`uya-game`), and each story level into a
//! library of its own (`tr_lNN.dll`), which the runtime (`port/uyart`) loads when the game loads that level's overlay. Both
//! link this crate and nothing else of the runtime, so a library never holds a copy of the runtime's state:
//!
//! - `Cpu`: one EE thread's registers (the R5900's 128-bit GPRs, HI/LO of both pipelines, SA, the FPU, the VU0 registers the
//!   macro instructions use) and pointers to the machine's memories, its call table and its field deadline, `#[repr(C)]`.
//! - The inline paths: a call through the table (`Cpu::call`), a loop's back edge (`Cpu::back`), loads and stores to RAM and
//!   the scratchpad (every other address is one cold call), the FPU's, the multimedia instructions' and VU0's macro
//!   arithmetic (`fpu`, `mmi`, `vu0`). None of them can panic. The translated code is most of what the port compiles (3,600
//!   crates), so what it expands to per instruction is what a build of it costs: a call's miss and the stops are cold
//!   out-of-line calls.
//! - Any change here rebuilds every translated crate: behaviour that may change belongs behind `Rt`.
//! - Every entry that changes the machine's state or stops the game (a call the table has no code for, the slow memory paths,
//!   the kernel's calls, COP0, CTC2 and VCALLMS, a polling loop, a stop) goes through the runtime's table of functions (`Rt`),
//!   so the call table, the turn lock and the panics that stop the game stay the executable's: a stop unwinds from the
//!   runtime's code through a library's frames to the executable's `catch_unwind`, never from a library's own panic runtime.

pub mod fpu;
pub mod mmi;
pub mod vu0;

use std::sync::atomic::{AtomicU32, Ordering};

/// A translated function: entered at a label (its entry or any label the table names for it). `C-unwind`: a library's
/// functions are called through the executable's table, and a stop unwinds through them.
pub type Fn = extern "C-unwind" fn(&mut Cpu, u32);

/// A translated program: its code's place and checks, and its functions' tables (one per crate). `lib` names the library that
/// holds the functions of a program linked apart (a story level: `tr_lNN`, its `funcs` empty in the executable's list), "" for
/// a program linked into the executable.
pub struct Program {
    pub name: &'static str,
    pub text: (u32, u32),
    /// FNV-1a over the text's words, and a few (address, word) pairs to look at first.
    pub hash: u64,
    pub sample: &'static [(u32, u32)],
    pub funcs: &'static [&'static [(u32, Fn)]],
    pub lib: &'static str,
}

pub const RAM_SIZE: u32 = 0x0200_0000;
pub const SPR_BASE: u32 = 0x7000_0000;
/// The call table covers EE addresses 0 .. TABLE_END (the main program, the overlays and the loader at 0x800000).
pub const TABLE_END: u32 = 0x0082_0000;

/// What a library and the executable must agree on: the layout of `Cpu`, `Rt` and `Program` (checked when a library is
/// loaded: `uya_abi`). Bumped by hand with any change to them that keeps their sizes.
pub const ABI: u64 = 8 | (std::mem::size_of::<Cpu>() as u64) << 8 | (std::mem::size_of::<Rt>() as u64) << 24 | (std::mem::size_of::<Program>() as u64) << 40;

/// The runtime's entries, in the executable (`uyart::RT`). Every one takes the running thread's `Cpu`.
#[repr(C)]
pub struct Rt {
    /// No code at the address in the table: programs may have arrived in RAM since the last check; else the game stops.
    pub miss: extern "C-unwind" fn(&mut Cpu, u32, u32),
    /// A loop's back edge while the host waits for the field's end (`SAFE` not 0).
    pub safe_point: extern "C-unwind" fn(&mut Cpu, u32),
    pub bad_label: extern "C-unwind" fn(&mut Cpu, u32, u32) -> !,
    /// A call came back somewhere other than the instruction after it (`Cpu::came_back`).
    pub came_back: extern "C-unwind" fn(&mut Cpu, u32),
    pub trap: extern "C-unwind" fn(&mut Cpu, u32) -> !,
    pub brk: extern "C-unwind" fn(&mut Cpu, u32, u32) -> !,
    /// A TLB instruction (its name's bytes).
    pub kernel_only: extern "C-unwind" fn(&mut Cpu, u32, *const u8, usize) -> !,
    pub spin: extern "C-unwind" fn(&mut Cpu, u32),
    pub syscall: extern "C-unwind" fn(&mut Cpu, u32),
    pub ei: extern "C-unwind" fn(&mut Cpu),
    pub mfc0: extern "C-unwind" fn(&mut Cpu, u32, u32) -> u32,
    pub mtc0: extern "C-unwind" fn(&mut Cpu, u32, u32, u32),
    pub cfc2: extern "C-unwind" fn(&mut Cpu, u32) -> u32,
    pub ctc2: extern "C-unwind" fn(&mut Cpu, u32, u32, u32),
    pub vcallms: extern "C-unwind" fn(&mut Cpu, u32, u32),
    pub vu0_busy: extern "C-unwind" fn(&mut Cpu) -> bool,
    pub rd_slow: extern "C-unwind" fn(&mut Cpu, u32, u32) -> u64,
    pub wr_slow: extern "C-unwind" fn(&mut Cpu, u32, u32, u64),
    pub rd128_slow: extern "C-unwind" fn(&mut Cpu, u32, &mut [u64; 2]),
    pub wr128_slow: extern "C-unwind" fn(&mut Cpu, u32, u64, u64),
    /// A game function the runtime answers whole (`uyatr`'s NATIVE): 0 InitSys's BIOS scan, 1 the movie player.
    pub native: extern "C-unwind" fn(&mut Cpu, u32),
}

/// What the host (the port, not the PS2) tells the game's code: the screen's shape and the player's field of view for the view.
/// `view_k` (f32 bits) is the screen's width over the original picture's at the same height, 1.0 on an original-shaped screen;
/// the loads of the view's horizontal tangent that make the view and its culling (uyatr's `VIEW_LOADS`) read it times `view_k`
/// (`Cpu::widen`). `menu`: a level shows a menu (paused, or one of the screens it opens from the world, `menus.rs`), whose lists
/// the UI element system lays out in the HUD's screen too: no HUD group moves to the wide screen's edges then (`Cpu::ui_screen`).
/// `fov` (f32 bits) is the field of view setting's factor on both tangents, 1.0 for the game's own view (and outside play: the
/// port sets it each field); the same loads read the horizontal tangent times it too, and the loads of the vertical tangent
/// where the view is made, and of the horizontal where the game sizes things on screen or picks its targets (uyatr's
/// `FOV_LOADS`), read theirs times it (`Cpu::fov`). The game's field of view itself (what its camera, cutscene and menu code
/// set, save and restore) is not changed. `cam_speed` (f32 bits) is the camera speed setting's factor on the game's own camera
/// speed option where its cameras read it to turn by the right stick (uyatr's `CAMERA_SPEED`, `Cpu::cam_speed`), 1.0 for the
/// game's own. `aim`: the keyboard and mouse play, so the hero's and its camera's reads of the camera option read lock-strafe
/// (uyatr's `CAMERA_SETTING`, `Cpu::camera_setting`; the mouse aim, PLAN 10). It lives in the runtime, outside the PS2's memory.
#[repr(C)]
pub struct Host {
    pub view_k: AtomicU32,
    pub menu: std::sync::atomic::AtomicBool,
    pub fov: AtomicU32,
    pub cam_speed: AtomicU32,
    pub aim: std::sync::atomic::AtomicBool,
}

/// The camera option of player 0 (camera_setting: 0 third person, 1 lock-strafe, 2 first person).
pub const CAMERA_SETTING: u32 = 0x14_39F4;

/// One EE thread's registers, and the machine they run on. `#[repr(C)]` keeps the hot fields together and the layout the same
/// in the executable and every library.
#[repr(C)]
pub struct Cpu {
    /// The GPRs' low 64 bits (what every instruction but MMI, LQ and SQ uses) and their high 64 bits.
    pub r: [u64; 32],
    pub rh: [u64; 32],
    pub lo: u64,
    pub hi: u64,
    pub lo1: u64,
    pub hi1: u64,
    /// The shift amount register (bits), for QFSRV.
    pub sa: u32,
    /// Where the last function returned to (its `jr $ra`): the caller checks it.
    pub ret: u32,
    /// FPU registers (bits), its accumulator, FCR31.
    pub f: [u32; 32],
    pub facc: u32,
    pub fcr31: u32,
    /// VU0's registers as the macro instructions see them: VF (bits), ACC, VI (0..15 the integer registers, 16 status, 17 MAC,
    /// 18 clip, 20 R, 21 I, 22 Q, 26 TPC, 27 CMSAR0, 28 FBRST, 29 VPU-STAT, 31 CMSAR1). One VU0: the kernel's thread switch carries
    /// them from thread to thread.
    pub vf: [[u32; 4]; 32],
    pub vacc: [u32; 4],
    pub vi: [u32; 32],
    pub ram: *mut u8,
    pub spr: *mut u8,
    /// VU0's data memory (4 KB).
    pub vu0d: *mut u8,
    /// The runtime's machine (`uyart::Machine`), opaque here.
    pub m: *mut std::ffi::c_void,
    /// The runtime's entries, its call table (TABLE_END / 4 words: a `Fn` or 0) and its field deadline flag (`uyart::machine::SAFE`).
    pub rt: *const Rt,
    pub table: *const usize,
    pub safe: *const AtomicU32,
    /// COP0 Status as this thread runs (IE, EIE, EXL, ERL), EPC and ErrorEPC.
    pub status: u32,
    pub epc: u32,
    pub errorepc: u32,
    /// The kernel thread this context runs (0 for an interrupt handler's context).
    pub thread: u32,
    /// The view's horizontal tangent as the game wrote it times the field of view's factor, at the last load of it that read it
    /// widened (`widen`): the view at the original shape.
    pub view_h: u32,
    /// The host's word to the game's code (`Host`).
    pub host: *const Host,
    /// The HUD's elements moved out to the wide screen's edges (`ui_screen`, `ui_shift`): (element, shift in shares of the
    /// original picture's width), `ui_n` of them.
    pub ui: [(u32, f32); UI_MAX],
    pub ui_n: u32,
}

/// The most HUD elements `ui_screen` places (Veldin's HUD screen has 53).
pub const UI_MAX: usize = 256;


unsafe impl Send for Cpu {}

impl Cpu {
    /// A context with every register 0 (vf0 = 0, 0, 0, 1), on the machine `m` with its memories, entries, table and deadline.
    #[allow(clippy::too_many_arguments)]
    pub fn new(m: *mut std::ffi::c_void, ram: *mut u8, spr: *mut u8, vu0d: *mut u8, rt: *const Rt, table: *const usize, safe: *const AtomicU32, host: *const Host) -> Cpu {
        let mut vf = [[0u32; 4]; 32];
        vf[0] = [0, 0, 0, 0x3f80_0000];
        Cpu {
            r: [0; 32],
            rh: [0; 32],
            lo: 0,
            hi: 0,
            lo1: 0,
            hi1: 0,
            sa: 0,
            ret: 0,
            f: [0; 32],
            facc: 0,
            fcr31: 0,
            vf,
            vacc: [0; 4],
            vi: [0; 32],
            ram,
            spr,
            vu0d,
            m,
            rt,
            table,
            safe,
            status: 0x7001_0c11,
            epc: 0,
            errorepc: 0,
            thread: 0,
            view_h: 0,
            host,
            ui: [(0, 0.0); UI_MAX],
            ui_n: 0,
        }
    }

    fn view_k(&self) -> f32 {
        f32::from_bits(unsafe { &*self.host }.view_k.load(Ordering::Relaxed))
    }

    fn view_fov(&self) -> f32 {
        f32::from_bits(unsafe { &*self.host }.fov.load(Ordering::Relaxed))
    }

    /// A load of the view's horizontal tangent where it makes the view (uyatr's `VIEW_LOADS`): the word times the host's
    /// `view_k` and `fov`. The word times `fov` alone is kept (`view_h`): the view at the original shape. With both 1.0, the word.
    #[inline(always)]
    pub fn widen(&mut self, w: u32) -> u32 {
        let f = self.view_fov();
        self.view_h = (f32::from_bits(w) * f).to_bits();
        (f32::from_bits(w) * self.view_k() * f).to_bits()
    }

    /// A read of the camera option by the hero's or its camera's code (uyatr's `CAMERA_SETTING`, the byte `v` at `a`): player
    /// 0's reads lock-strafe (1) while the host's `aim` is set; any other read as written.
    #[inline(always)]
    pub fn camera_setting(&self, a: u32, v: u8) -> u8 {
        if a == CAMERA_SETTING && unsafe { &*self.host }.aim.load(Ordering::Relaxed) { 1 } else { v }
    }

    /// A load of the game's camera speed option (an integer word, 50 in a new game) where a camera turns by it (uyatr's
    /// `CAMERA_SPEED`): the word times the host's `cam_speed`, rounded; the word itself at 1.0.
    #[inline(always)]
    pub fn cam_speed(&self, w: u32) -> u32 {
        let s = f32::from_bits(unsafe { &*self.host }.cam_speed.load(Ordering::Relaxed));
        if s == 1.0 { w } else { ((w as i32) as f32 * s).round() as i32 as u32 }
    }

    /// A load of a view tangent read times the field of view's factor (uyatr's `FOV_LOADS`, and the vertical tangent of a
    /// quadword `VIEW_LOADS` widens): the word itself at 1.0.
    #[inline(always)]
    pub fn fov(&self, w: u32) -> u32 {
        (f32::from_bits(w) * self.view_fov()).to_bits()
    }

    /// The moby cull's side test as the original view makes it (uyatr's `DRAWN`): func_L01_0043EA78 takes a moby that passed
    /// its tests, with vf2 = (|x|, |y|, z) of its bounding sphere in view space and vf1.w its radius, as within the view's sides
    /// when vf7.x = h z - (|x| - r sqrt(1 + h^2)) is not negative (vf20.x and vf22.x: the tangent, and the 1/cos(atan h) the
    /// view's setup made of it). Here with the tangent as the game wrote it: what the game decided before the view was widened.
    /// Always true on an original-shaped screen, where nothing was widened.
    pub fn in_original_view(&self) -> bool {
        if self.view_k() == 1.0 {
            return true;
        }
        let h = f32::from_bits(self.view_h);
        let f = |r: usize, l: usize| f32::from_bits(self.vf[r][l]);
        let side = h * f(2, 2) - (f(2, 0) - (1.0 + h * h).sqrt() * f(1, 3));
        side.to_bits() >> 31 == 0
    }

    /// The moby cull's drawn byte (moby +0x31, `flag`) where the moby passed the widened view's tests (uyatr's `DRAWN`, before
    /// the branch on `was`, the byte as the cull read it): when the original view would have culled it, the byte becomes what
    /// the cull's culled path makes it (cleared, unless it was already clear or ra's high doubleword is positive, the cull's
    /// mode that keeps it), so the game's code that reads it (the lists of targets, the moby updates' on-screen tests) sees
    /// the original view's decision while the moby is drawn.
    pub fn drawn_side(&mut self, flag: u32, was: u64) {
        if !self.in_original_view() && was as u8 != 0 && self.rh[31] as i64 <= 0 {
            self.wr8(flag, 0);
        }
    }

    /// The UI element system's draw of its current screen (uyatr's `UI_SCREEN`: func_L01_00511BA8's entry, `manager` its
    /// argument, Veldin's 0x1E1660; in every level, not in the front ends): the HUD (screen 0 of the manager's screens, +4; its
    /// current one at +0x14) is laid out for the original picture, in shares of its width, and on a wider screen each group of
    /// its elements moves out by half the extra width to the side it sits on, so it keeps its distance from that edge and its
    /// size, and a group in the middle stays (PLAN 4.2). The screen record's first word is its root (elements at +0x50, 0x23
    /// slots); an element's +8 is its class (the class's +0x28 its kind: 5 a container, its children at +0x2C's +0, as many as
    /// +0x2C's +0x48; 8 a picture), +0x10 its x. A group is a container whose children are no containers (Veldin: the bolts
    /// top right, the ammo top centre, the health left, each bottom corner; a vid-comic: the health, the timer, the score); a
    /// container of containers lets each decide when each sits wholly at one side, else stays in the middle whole (`ui_place`).
    /// The other screens (menus and boxes) are drawn centred, as everything else of
    /// the 2D layer is (the renderer, `eedraw::unstretched`). While a level shows a menu (`Host::menu`) nothing moves: the
    /// menus' lists are drawn by elements of screen 0 too (the pause's AUDIO / VISUAL list: its labels, in the left third, went
    /// out to the left past the panel's edge and its sliders to the right), and the HUD is not up then.
    pub fn ui_screen(&mut self, manager: u32) {
        self.ui_n = 0;
        let k = self.view_k();
        if k == 1.0 || unsafe { &*self.host }.menu.load(Ordering::Relaxed) {
            return;
        }
        let Some(screens) = self.ram32(manager + 4).filter(|&s| s != 0) else { return };
        if self.ram32(screens + 0x14) != Some(0) {
            return;
        }
        let Some(root) = self.ram32(screens).filter(|&s| s != 0).and_then(|s| self.ram32(s)) else { return };
        let top: Vec<u32> = (0..0x23).filter_map(|n| self.ram32(root + 0x50 + 4 * n)).filter(|&e| e != 0).collect();
        self.ui_place(&top, (k - 1.0) / 2.0, 0);
        if std::env::var_os("UYA_UI_DUMP").is_some() {
            self.ui_dump(&top);
        }
    }

    /// UYA_UI_DUMP: the HUD's element tree (each element's address, kind, x, y, width, height and shift), printed when the tree's
    /// elements or their shifts change (a debugging switch, for which group an element is placed with).
    fn ui_dump(&mut self, top: &[u32]) {
        let mut lines = Vec::new();
        let mut stack: Vec<(u32, usize)> = top.iter().rev().map(|&e| (e, 0)).collect();
        let mut shape = Vec::new();
        while let Some((e, d)) = stack.pop() {
            let kind = self.ui_kind(e).unwrap_or(u32::MAX);
            let f = |c: &mut Self, o: u32| c.ram32(e + o).map_or(f32::NAN, f32::from_bits);
            let (x, y, w, h) = (f(self, 0x10), f(self, 0x14), f(self, 0x18), f(self, 0x1c));
            let shift = self.ui_shift(e).unwrap_or(0.0);
            shape.push((e, kind, shift.to_bits()));
            lines.push(format!("{}{e:#x} kind {kind} x {x:.3} y {y:.3} w {w:.3} h {h:.3} shift {shift:+.3}", "  ".repeat(d)));
            if kind == 5 && d < 8 {
                for c in self.ui_children(e).into_iter().rev() {
                    stack.push((c, d + 1));
                }
            }
        }
        let sig = shape.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &(e, k, s)| ((h ^ e as u64 ^ (k as u64) << 32).wrapping_mul(0x100_0000_01b3) ^ s as u64).wrapping_mul(0x100_0000_01b3));
        // The last tree printed (a static: the Cpu's layout is the level libraries' ABI).
        static DUMPED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        if DUMPED.swap(sig, Ordering::Relaxed) != sig {
            eprintln!("ui: {} elements
{}", lines.len(), lines.join("
"));
        }
    }

    /// The shift of an element `ui_screen` placed (uyatr's `UI_LOADS`: the loads of its x, +0x10, where its kind lays it out or
    /// draws a picture at its own place); None for any other.
    #[inline(always)]
    pub fn ui_shift(&self, e: u32) -> Option<f32> {
        self.ui[..self.ui_n as usize].iter().find(|u| u.0 == e).map(|u| u.1)
    }

    /// How far past the original frame's edges the wide screen reaches, in the GS's 12.4 coordinates: half the extra width of
    /// the 512-pixel frame, (k - 1) x 256 x 16 (uyatr's `UI_EDGES`: the 2D drawers' screen test); 0 on an original-shaped screen.
    #[inline(always)]
    pub fn ui_margin(&self) -> i64 {
        ((self.view_k() - 1.0) * 4096.0).round() as i64
    }

    /// A word of RAM, None outside it (the UI's pointers are followed only within it).
    fn ram32(&mut self, a: u32) -> Option<u32> {
        (a < RAM_SIZE - 3).then(|| self.rd32(a))
    }

    /// An element's kind (its class's +0x28).
    fn ui_kind(&mut self, e: u32) -> Option<u32> {
        self.ram32(e + 8).and_then(|c| self.ram32(c + 0x28))
    }

    /// A container's children (+0x2C's words, as many as its +0x48, at most the 18 it holds).
    fn ui_children(&mut self, e: u32) -> Vec<u32> {
        let Some(list) = self.ram32(e + 0x2c).filter(|&l| l != 0) else { return Vec::new() };
        let n = self.ram32(list + 0x48).unwrap_or(0).min(18);
        (0..n).filter_map(|i| self.ram32(list + 4 * i)).filter(|&c| c != 0).collect()
    }

    /// The shift of a group of elements: from the x of those that are not pictures (a picture's place is its art's, the HUD's
    /// panels being drawn at the screen's centre at its size), the group's middle in the left third moves left, in the right
    /// third right; a group over the screen's middle, or with its middle in the middle third, stays (0).
    fn ui_group_shift(&mut self, group: &[u32], half: f32) -> f32 {
        let mut xs = Vec::new();
        for &e in group {
            if self.ui_kind(e) != Some(8) {
                xs.push(f32::from_bits(self.ram32(e + 0x10).unwrap_or(0x3f00_0000)));
            }
        }
        if xs.is_empty() {
            return 0.0;
        }
        let (lo, hi) = xs.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &x| (lo.min(x), hi.max(x)));
        let mid = (lo + hi) / 2.0;
        if lo < 0.5 && hi > 0.5 {
            0.0
        } else if mid < 1.0 / 3.0 {
            -half
        } else if mid > 2.0 / 3.0 {
            half
        } else {
            0.0
        }
    }

    /// Places the elements `list` (the root's, or a container's children): the ones that are no containers as one group; each
    /// container whose children are no containers as a group of its own (with them); any other container's children in turn,
    /// when the groups under it each sit wholly at one side (`ui_sided`), else none of them moves. A container of containers is
    /// one widget made of groups: the HUD's bottom corners (four groups, each in a corner) go apart to their edges, but the
    /// Tyhrranoid talk's display (level 27's 0x39E020: the frame 0x120093, its art centred on the press point x 0.5 and its
    /// texts at x 0.665 to 0.835, and the notes and their hold bars, each a group, placed every frame at 0.5 + (note frame -
    /// timer) / 270 along the track from 0.135 to 0.865) is one piece in the middle: decided group by group, the frame went
    /// right and the notes wherever they were at the time, and the marker stood right of where the game took the press (the
    /// player, 20 November 2026: "the marker that indicates where youre meant to press isnt aligned with where the game wants
    /// you to press it").
    fn ui_place(&mut self, list: &[u32], half: f32, depth: u32) {
        let (mut leaves, mut parents) = (Vec::new(), Vec::new());
        for &e in list {
            if self.ui_kind(e) == Some(5) { parents.push(e) } else { leaves.push(e) }
        }
        let shift = self.ui_group_shift(&leaves, half);
        for e in leaves {
            self.ui_mark(e, shift);
        }
        for c in parents {
            let kids = self.ui_children(c);
            if depth < 8 && kids.iter().any(|&k| self.ui_kind(k) == Some(5)) {
                if self.ui_sided(&kids, depth + 1) {
                    self.ui_place(&kids, half, depth + 1);
                }
            } else {
                let shift = self.ui_group_shift(&kids, half);
                self.ui_mark(c, shift);
                for k in kids {
                    self.ui_mark(k, shift);
                }
            }
        }
    }

    /// Whether each group `ui_place` makes of `list` (its leaves, its containers of leaves, and those of its containers of
    /// containers in turn) has the x of every element that is no picture in one side third (a group of pictures alone has
    /// none, and is no reason either way).
    fn ui_sided(&mut self, list: &[u32], depth: u32) -> bool {
        let (mut leaves, mut parents) = (Vec::new(), Vec::new());
        for &e in list {
            if self.ui_kind(e) == Some(5) { parents.push(e) } else { leaves.push(e) }
        }
        if !self.ui_one_side(&leaves) {
            return false;
        }
        for c in parents {
            let kids = self.ui_children(c);
            let sided = if depth < 8 && kids.iter().any(|&k| self.ui_kind(k) == Some(5)) { self.ui_sided(&kids, depth + 1) } else { self.ui_one_side(&kids) };
            if !sided {
                return false;
            }
        }
        true
    }

    /// Whether the x of every element of `group` that is no picture is in the left third, or every one in the right third.
    fn ui_one_side(&mut self, group: &[u32]) -> bool {
        let mut xs = Vec::new();
        for &e in group {
            if self.ui_kind(e) != Some(8) {
                xs.push(f32::from_bits(self.ram32(e + 0x10).unwrap_or(0x3f00_0000)));
            }
        }
        xs.iter().all(|&x| x < 1.0 / 3.0) || xs.iter().all(|&x| x > 2.0 / 3.0)
    }

    /// Element `e` shifted by `shift` (none kept: an element that stays reads its x as it is).
    fn ui_mark(&mut self, e: u32, shift: f32) {
        if shift != 0.0 {
            self.ui[self.ui_n as usize] = (e, shift);
            self.ui_n += 1;
        }
    }

    #[inline(always)]
    fn rt(&self) -> &'static Rt {
        unsafe { &*self.rt }
    }

    /// The translated function holding label `a`, if its program is in RAM.
    #[inline(always)]
    pub fn lookup(&self, a: u32) -> Option<Fn> {
        if a >= TABLE_END || a & 3 != 0 {
            return None;
        }
        let f = unsafe { *self.table.add((a >> 2) as usize) };
        if f == 0 { None } else { Some(unsafe { std::mem::transmute::<usize, Fn>(f) }) }
    }

    /// Calls the translated code at `t` (from the instruction at `from`).
    #[inline(always)]
    pub fn call(&mut self, t: u32, from: u32) {
        match self.lookup(t) {
            Some(f) => f(self, t),
            None => self.miss(t, from),
        }
    }

    /// No code at `t` in the table (`Rt::miss`).
    #[cold]
    #[inline(never)]
    pub fn miss(&mut self, t: u32, from: u32) {
        (self.rt().miss)(self, t, from)
    }

    /// A loop's back edge (`uyatr` emits it at every backward branch inside a function), where the host's field deadline
    /// reaches a running thread.
    #[inline(always)]
    pub fn back(&mut self, pc: u32) {
        if unsafe { &*self.safe }.load(Ordering::Relaxed) != 0 {
            self.safe_point(pc)
        }
    }

    #[cold]
    #[inline(never)]
    fn safe_point(&mut self, pc: u32) {
        (self.rt().safe_point)(self, pc)
    }

    // The entries below are out of line: a translated function's use of one is one direct call, and the stops are cold.

    #[cold]
    #[inline(never)]
    pub fn bad_label(&mut self, f: u32, l: u32) -> ! {
        (self.rt().bad_label)(self, f, l)
    }
    /// The call at `at` came back to `ret`, not to the instruction after it: the code after the call already ran in the frame
    /// that went there (a routine's handler branched back into its caller's loop: level 4's 0x4b12b8 calls its clip states
    /// with `jalr a2, a3`, and a state with no vertex to make branches straight back to the loop), so the caller returns too,
    /// with `ret`, until the frame that continues at `ret`; no frame does, the thread's or the handler's top stops the game.
    /// The runtime keeps the call for that message.
    #[cold]
    #[inline(never)]
    pub fn came_back(&mut self, at: u32) {
        (self.rt().came_back)(self, at)
    }
    #[cold]
    #[inline(never)]
    pub fn trap(&mut self, at: u32) -> ! {
        (self.rt().trap)(self, at)
    }
    #[cold]
    #[inline(never)]
    pub fn brk(&mut self, at: u32, code: u32) -> ! {
        (self.rt().brk)(self, at, code)
    }
    #[cold]
    #[inline(never)]
    pub fn kernel_only(&mut self, at: u32, what: &'static str) -> ! {
        (self.rt().kernel_only)(self, at, what.as_ptr(), what.len())
    }
    /// A loop that only polls: the runtime lets time and the other threads go on.
    #[inline(never)]
    pub fn spin(&mut self, pc: u32) {
        (self.rt().spin)(self, pc)
    }
    /// SYSCALL: the kernel call in $v1 with its arguments in $a0..$a3; the result in $v0.
    #[inline(never)]
    pub fn syscall(&mut self, pc: u32) {
        (self.rt().syscall)(self, pc)
    }
    #[inline(never)]
    pub fn ei(&mut self) {
        (self.rt().ei)(self)
    }
    #[inline(always)]
    pub fn di(&mut self) {
        self.status &= !0x1_0000;
    }
    /// ERET: ERL (or else EXL) cleared, and where the program goes on.
    #[inline(always)]
    pub fn eret(&mut self) -> u32 {
        if self.status & 4 != 0 {
            self.status &= !4;
            self.errorepc
        } else {
            self.status &= !2;
            self.epc
        }
    }
    #[inline(never)]
    pub fn mfc0(&mut self, r: usize, sel: u32) -> u32 {
        (self.rt().mfc0)(self, r as u32, sel)
    }
    #[inline(never)]
    pub fn mtc0(&mut self, r: usize, sel: u32, v: u32) {
        (self.rt().mtc0)(self, r as u32, sel, v)
    }
    #[inline(never)]
    pub fn cfc2(&mut self, r: usize) -> u32 {
        (self.rt().cfc2)(self, r as u32)
    }
    #[inline(never)]
    pub fn ctc2(&mut self, r: usize, v: u32, pc: u32) {
        (self.rt().ctc2)(self, r as u32, v, pc)
    }
    /// VCALLMS / VCALLMSR: the microprogram at micro address `a`.
    #[inline(never)]
    pub fn vcallms(&mut self, a: u32, pc: u32) {
        (self.rt().vcallms)(self, a, pc)
    }
    /// COP2's condition: a microprogram is waiting for the EE.
    #[inline(never)]
    pub fn vu0_busy(&mut self) -> bool {
        (self.rt().vu0_busy)(self)
    }
    /// A game function the runtime answers whole (`Rt::native`).
    #[inline(never)]
    pub fn native(&mut self, k: u32) {
        (self.rt().native)(self, k)
    }

    // ---- memory -------------------------------------------------------------------------------------------------------
    //
    // RAM and the scratchpad inline; every other address (RAM's other windows, the hardware registers) through one cold call.
    // (With the scratchpad out of line too, a call that is not cold on every access's other side, Veldin's game code took 13.6
    // ms a field instead of 6.8: the translated code is a third of the IR, but the game's speed comes first.)

    #[inline(always)]
    pub fn rd8(&mut self, a: u32) -> u8 {
        if a < RAM_SIZE {
            unsafe { *self.ram.add(a as usize) }
        } else if a & 0xffff_c000 == SPR_BASE {
            unsafe { *self.spr.add((a & 0x3fff) as usize) }
        } else {
            self.rd_slow(a, 1) as u8
        }
    }
    #[inline(always)]
    pub fn rd16(&mut self, a: u32) -> u16 {
        if a < RAM_SIZE {
            unsafe { (self.ram.add(a as usize) as *const u16).read_unaligned() }
        } else if a & 0xffff_c000 == SPR_BASE {
            unsafe { (self.spr.add((a & 0x3fff) as usize) as *const u16).read_unaligned() }
        } else {
            self.rd_slow(a, 2) as u16
        }
    }
    #[inline(always)]
    pub fn rd32(&mut self, a: u32) -> u32 {
        if a < RAM_SIZE {
            unsafe { (self.ram.add(a as usize) as *const u32).read_unaligned() }
        } else if a & 0xffff_c000 == SPR_BASE {
            unsafe { (self.spr.add((a & 0x3fff) as usize) as *const u32).read_unaligned() }
        } else {
            self.rd_slow(a, 4) as u32
        }
    }
    #[inline(always)]
    pub fn rd64(&mut self, a: u32) -> u64 {
        if a < RAM_SIZE {
            unsafe { (self.ram.add(a as usize) as *const u64).read_unaligned() }
        } else if a & 0xffff_c000 == SPR_BASE {
            unsafe { (self.spr.add((a & 0x3fff) as usize) as *const u64).read_unaligned() }
        } else {
            self.rd_slow(a, 8)
        }
    }
    /// LQ: the 16 bytes of the quadword holding `a` (the low four address bits are ignored).
    #[inline(always)]
    pub fn rd128(&mut self, a: u32) -> (u64, u64) {
        let a = a & !15;
        if a < RAM_SIZE {
            unsafe { ((self.ram.add(a as usize) as *const u64).read_unaligned(), (self.ram.add(a as usize + 8) as *const u64).read_unaligned()) }
        } else if a & 0xffff_c000 == SPR_BASE {
            let o = (a & 0x3fff) as usize;
            unsafe { ((self.spr.add(o) as *const u64).read_unaligned(), (self.spr.add(o + 8) as *const u64).read_unaligned()) }
        } else {
            self.rd128_slow(a)
        }
    }
    #[inline(always)]
    pub fn wr8(&mut self, a: u32, v: u8) {
        if a < RAM_SIZE {
            unsafe { *self.ram.add(a as usize) = v }
        } else if a & 0xffff_c000 == SPR_BASE {
            unsafe { *self.spr.add((a & 0x3fff) as usize) = v }
        } else {
            self.wr_slow(a, 1, v as u64)
        }
    }
    #[inline(always)]
    pub fn wr16(&mut self, a: u32, v: u16) {
        if a < RAM_SIZE {
            unsafe { (self.ram.add(a as usize) as *mut u16).write_unaligned(v) }
        } else if a & 0xffff_c000 == SPR_BASE {
            unsafe { (self.spr.add((a & 0x3fff) as usize) as *mut u16).write_unaligned(v) }
        } else {
            self.wr_slow(a, 2, v as u64)
        }
    }
    #[inline(always)]
    pub fn wr32(&mut self, a: u32, v: u32) {
        if a < RAM_SIZE {
            unsafe { (self.ram.add(a as usize) as *mut u32).write_unaligned(v) }
        } else if a & 0xffff_c000 == SPR_BASE {
            unsafe { (self.spr.add((a & 0x3fff) as usize) as *mut u32).write_unaligned(v) }
        } else {
            self.wr_slow(a, 4, v as u64)
        }
    }
    #[inline(always)]
    pub fn wr64(&mut self, a: u32, v: u64) {
        if a < RAM_SIZE {
            unsafe { (self.ram.add(a as usize) as *mut u64).write_unaligned(v) }
        } else if a & 0xffff_c000 == SPR_BASE {
            unsafe { (self.spr.add((a & 0x3fff) as usize) as *mut u64).write_unaligned(v) }
        } else {
            self.wr_slow(a, 8, v)
        }
    }
    #[inline(always)]
    pub fn wr128(&mut self, a: u32, lo: u64, hi: u64) {
        let a = a & !15;
        if a < RAM_SIZE {
            unsafe {
                (self.ram.add(a as usize) as *mut u64).write_unaligned(lo);
                (self.ram.add(a as usize + 8) as *mut u64).write_unaligned(hi);
            }
        } else if a & 0xffff_c000 == SPR_BASE {
            let o = (a & 0x3fff) as usize;
            unsafe {
                (self.spr.add(o) as *mut u64).write_unaligned(lo);
                (self.spr.add(o + 8) as *mut u64).write_unaligned(hi);
            }
        } else {
            self.wr128_slow(a, lo, hi)
        }
    }

    /// Every address but RAM and the scratchpad: the runtime's (`uyart::mem`), cold.
    #[cold]
    #[inline(never)]
    pub fn rd_slow(&mut self, a: u32, n: u32) -> u64 {
        (self.rt().rd_slow)(self, a, n)
    }
    #[cold]
    #[inline(never)]
    pub fn wr_slow(&mut self, a: u32, n: u32, v: u64) {
        (self.rt().wr_slow)(self, a, n, v)
    }
    #[cold]
    #[inline(never)]
    pub fn rd128_slow(&mut self, a: u32) -> (u64, u64) {
        let mut q = [0u64; 2];
        (self.rt().rd128_slow)(self, a, &mut q);
        (q[0], q[1])
    }
    #[cold]
    #[inline(never)]
    pub fn wr128_slow(&mut self, a: u32, lo: u64, hi: u64) {
        (self.rt().wr128_slow)(self, a, lo, hi)
    }

    // The unaligned loads and stores (little-endian MIPS): LWL / LWR merge the word's bytes into rt from the high or low end.
    pub fn lwl(&mut self, a: u32, rt: u64) -> u64 {
        let s = (a & 3) * 8;
        let m = self.rd32(a & !3);
        let keep = if s == 24 { 0 } else { 0x00ff_ffffu32 >> s };
        ((rt as u32 & keep) | (m << (24 - s))) as i32 as i64 as u64
    }
    pub fn lwr(&mut self, a: u32, rt: u64) -> u64 {
        let s = (a & 3) * 8;
        let m = self.rd32(a & !3);
        if s == 0 {
            return m as i32 as i64 as u64;
        }
        let keep = !(0xffff_ffffu32 >> s);
        (rt & !0xffff_ffff) | ((rt as u32 & keep) | (m >> s)) as u64
    }
    pub fn ldl(&mut self, a: u32, rt: u64) -> u64 {
        let s = (a & 7) * 8;
        let m = self.rd64(a & !7);
        let keep = if s == 56 { 0 } else { 0x00ff_ffff_ffff_ffffu64 >> s };
        (rt & keep) | (m << (56 - s))
    }
    pub fn ldr(&mut self, a: u32, rt: u64) -> u64 {
        let s = (a & 7) * 8;
        let m = self.rd64(a & !7);
        if s == 0 {
            return m;
        }
        let keep = !(u64::MAX >> s);
        (rt & keep) | (m >> s)
    }
    pub fn swl(&mut self, a: u32, rt: u64) {
        let s = (a & 3) * 8;
        let m = self.rd32(a & !3);
        let keep = if s == 24 { 0 } else { 0xffff_ff00u32 << s };
        self.wr32(a & !3, ((rt as u32) >> (24 - s)) | (m & keep));
    }
    pub fn swr(&mut self, a: u32, rt: u64) {
        let s = (a & 3) * 8;
        let m = self.rd32(a & !3);
        let keep = if s == 0 { 0 } else { 0xffff_ffffu32 >> (32 - s) };
        self.wr32(a & !3, ((rt as u32) << s) | (m & keep));
    }
    pub fn sdl(&mut self, a: u32, rt: u64) {
        let s = (a & 7) * 8;
        let m = self.rd64(a & !7);
        let keep = if s == 56 { 0 } else { 0xffff_ffff_ffff_ff00u64 << s };
        self.wr64(a & !7, (rt >> (56 - s)) | (m & keep));
    }
    pub fn sdr(&mut self, a: u32, rt: u64) {
        let s = (a & 7) * 8;
        let m = self.rd64(a & !7);
        let keep = if s == 0 { 0 } else { u64::MAX >> (64 - s) };
        self.wr64(a & !7, (rt << s) | (m & keep));
    }

    // ---- COP1 control, COP2 control ----------------------------------------------------------------------------------

    pub fn cfc1(&mut self, r: usize) -> u32 {
        match r {
            0 => 0x2e00,
            31 => self.fcr31,
            _ => 0,
        }
    }
    pub fn ctc1(&mut self, r: usize, v: u32) {
        if r == 31 {
            self.fcr31 = v;
        }
    }
    #[inline(always)]
    pub fn set_fc(&mut self, c: bool) {
        if c {
            self.fcr31 |= 0x0080_0000
        } else {
            self.fcr31 &= !0x0080_0000
        }
    }

    /// VI writes: vi0 stays 0, vi1..vi15 hold 16 bits.
    #[inline(always)]
    pub fn set_vi(&mut self, i: usize, v: u32) {
        if i != 0 {
            self.vi[i] = if i < 16 { v & 0xffff } else { v };
        }
    }

    /// COP0's condition 0: the DMA channels named in D_PCR's CPC bits have ended. A kick ends at once here.
    #[inline(always)]
    pub fn cpcond0(&mut self) -> bool {
        true
    }
}

/// DIV: the R5900's results, including the ones for a zero divisor (LO = 1 or -1 by the dividend's sign, HI = the dividend) and
/// for 0x80000000 / -1.
#[inline(always)]
pub fn div32(a: i32, b: i32) -> (i32, i32) {
    if b == 0 {
        (if a < 0 { 1 } else { -1 }, a)
    } else if a == i32::MIN && b == -1 {
        (i32::MIN, 0)
    } else {
        (a / b, a % b)
    }
}

/// DIVU: a zero divisor gives LO = 0xFFFFFFFF, HI = the dividend.
#[inline(always)]
pub fn divu32(a: u32, b: u32) -> (u32, u32) {
    if b == 0 { (u32::MAX, a) } else { (a / b, a % b) }
}
