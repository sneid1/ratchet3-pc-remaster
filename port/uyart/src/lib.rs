//! uyart: the port's own runtime for Up Your Arsenal (plan item 0, zero emulation). The game's EE code, translated ahead of time
//! into Rust by `port/uyatr`, runs on it as native code: nothing here interprets an instruction or models the PS2's timing.
//!
//! - `Cpu` (`uycpu`, the crate the translated code is compiled against): one EE thread's registers and the machine's memories:
//!   32 MB of RAM, the 16 KB scratchpad, VU0's memories (`mem`).
//! - The table of translated code (`Cpu::call`): every label of every program whose code is in RAM, swapped when the game loads
//!   an overlay (checked at its FlushCache). The loader, the boot program and both front ends are linked into the executable; each
//!   story level's code is a library of its own (`tr_lNN.dll` beside the executable), loaded when its overlay is in RAM
//!   (`machine::Machine::flush_cache`). An address with no translation stops the game with the address.
//! - The runtime's entries the translated code reaches through `uycpu::Rt` (`RT`): every one that changes the machine's state
//!   or stops the game, so its state and its panics stay here, in the executable.
//! - The EE kernel's calls (`kernel`): threads, semaphores, interrupt and DMA handlers, alarms, as native code.
//! - The hardware registers the code touches (`hw`): the DMA kicks hand the game's draw lists to the renderer, the vertical blank
//!   comes from the frame loop, the timers from the clock.
//! - The IOP's servers answered natively (`iop`): the disc image, the heap and module loads, file calls, pads, the memory card,
//!   the sound servers.
//! - The game's VU0 microprograms, translated like the rest of its code (`vu0`).

pub(crate) mod anim;
pub(crate) mod collide;
pub mod drawdist;
pub(crate) mod fxpool;
pub mod hw;
pub mod iop;
pub mod kernel;
pub mod machine;
pub mod mc;
pub mod mem;
pub(crate) mod mobydraw;
pub mod ram;
pub mod range;
pub(crate) mod shrubcull;
pub(crate) mod tiecull;
pub(crate) mod skycull;
pub mod vu0;
pub(crate) mod vu0native;

pub use machine::Machine;
pub use ram::{Ram, RamCopy};
pub use uycpu::{Cpu, Fn, Program, RAM_SIZE, SPR_BASE, TABLE_END, div32, divu32, fpu, mmi};

static mut TABLE: [usize; (TABLE_END / 4) as usize] = [0; (TABLE_END / 4) as usize];

/// The translated function holding label `a`, if its program is in RAM.
#[inline(always)]
pub fn lookup(a: u32) -> Option<Fn> {
    if a >= TABLE_END || a & 3 != 0 {
        return None;
    }
    let f = unsafe { *(&raw const TABLE as *const usize).add((a >> 2) as usize) };
    if f == 0 { None } else { Some(unsafe { std::mem::transmute::<usize, Fn>(f) }) }
}

/// Puts a program's labels in the table (`on`) or takes them out.
pub(crate) fn install(p: &Program, on: bool) {
    let t = &raw mut TABLE as *mut usize;
    for part in p.funcs {
        for &(a, f) in part.iter() {
            if a < TABLE_END {
                unsafe { *t.add((a >> 2) as usize) = if on { f as usize } else { 0 } };
            }
        }
    }
    for h in HOOKS.lock().expect("hooks").iter_mut().filter(|h| h.program == p.name) {
        if !on {
            h.own = 0;
            continue;
        }
        let own = unsafe { *t.add((h.at >> 2) as usize) };
        if own == 0 {
            panic!("hook: program {} has no function at {:08x}", p.name, h.at);
        }
        h.own = own;
        unsafe { *t.add((h.at >> 2) as usize) = h.f as usize };
    }
}

/// A port function the table holds in place of a program's own one (`hook`), and the program's own function there while the
/// program is in RAM (0 while it is not).
struct Hook {
    program: String,
    at: u32,
    f: Fn,
    own: usize,
}

static HOOKS: std::sync::Mutex<Vec<Hook>> = std::sync::Mutex::new(Vec::new());

/// Has the table hold `f` at `at` in place of program `program`'s own function there (a level's program is `lNN`), from the
/// next time that program's code comes into RAM (`machine::Machine::flush_cache`): registered before then. `f` runs the
/// program's own function with `own`.
pub fn hook(program: &str, at: u32, f: Fn) {
    let mut hooks = HOOKS.lock().expect("hooks");
    hooks.retain(|h| !(h.program == program && h.at == at));
    hooks.push(Hook { program: program.to_string(), at, f, own: 0 });
}

/// The program's own function that the hook at `at` stands in for, while that program is in RAM.
pub fn own(at: u32) -> Fn {
    let own = HOOKS.lock().expect("hooks").iter().find(|h| h.at == at && h.own != 0).map(|h| h.own);
    let own = own.unwrap_or_else(|| panic!("hook: no program in RAM has a hooked function at {at:08x}"));
    unsafe { std::mem::transmute::<usize, Fn>(own) }
}

/// A context on machine `m`: every register 0, the machine's memories, the runtime's entries, table and deadline, the host's word.
pub fn cpu(m: *mut Machine) -> Cpu {
    let mm = unsafe { &mut *m };
    Cpu::new(m.cast(), mm.ram.as_mut_ptr(), mm.spr.as_mut_ptr(), mm.vu0d.as_mut_ptr(), &RT, &raw const TABLE as *const usize, &machine::SAFE, &HOST)
}

/// Whether the skin job's joint matrices are kept for the renderer (`hw::SkinJoints`): the per-pixel lighting's (port/uya
/// `lighting.rs`), set by the renderer's options.
pub static SKIN_JOINTS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The host's words to the game's code (`uycpu::Host`): the screen's shape for the view, 1.0 until the window says otherwise;
/// whether a level shows a menu; the field of view's factor, 1.0 (the game's own view) until the port says otherwise; the camera
/// speed's factor, 1.0 (the game's own option) until the port says otherwise; whether the mouse aims (off until the port says).
pub static HOST: uycpu::Host = uycpu::Host {
    view_k: std::sync::atomic::AtomicU32::new(0x3f80_0000),
    menu: std::sync::atomic::AtomicBool::new(false),
    fov: std::sync::atomic::AtomicU32::new(0x3f80_0000),
    cam_speed: std::sync::atomic::AtomicU32::new(0x3f80_0000),
    aim: std::sync::atomic::AtomicBool::new(false),
};

/// The screen's width over the original picture's at the same height (`uycpu::Host::view_k`): the view the game's code makes
/// from the next load of its horizontal tangent on.
pub fn set_view_k(k: f32) {
    HOST.view_k.store(k.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// The screen's widening `set_view_k` set last.
pub fn view_k() -> f32 {
    f32::from_bits(HOST.view_k.load(std::sync::atomic::Ordering::Relaxed))
}

/// Whether a level shows a menu now (`uycpu::Host::menu`), set before each field.
pub fn set_menu(menu: bool) {
    HOST.menu.store(menu, std::sync::atomic::Ordering::Relaxed);
}

/// The field of view's factor on both of the view's tangents (`uycpu::Host::fov`; 1.0 the game's own view), from the next load
/// of a tangent on.
pub fn set_view_fov(f: f32) {
    HOST.fov.store(f.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// Whether the keyboard and mouse play (`uycpu::Host::aim`): the hero's code reads its camera option as lock-strafe.
pub fn set_aim(aim: bool) {
    HOST.aim.store(aim, std::sync::atomic::Ordering::Relaxed);
}

/// The camera speed setting's factor on the game's own camera speed option (`uycpu::Host::cam_speed`; 1.0 the game's own), from
/// the next read of it on.
pub fn set_camera_speed(s: f32) {
    HOST.cam_speed.store(s.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// The runtime's side of a `Cpu`: the machine it runs on.
pub trait Ext {
    fn mach(&mut self) -> &mut Machine;
    fn mp(&self) -> *mut Machine;
}

impl Ext for Cpu {
    #[inline(always)]
    fn mach(&mut self) -> &mut Machine {
        unsafe { &mut *(self.m as *mut Machine) }
    }
    #[inline(always)]
    fn mp(&self) -> *mut Machine {
        self.m as *mut Machine
    }
}

/// The entries the translated code reaches through `uycpu::Rt`.
pub static RT: uycpu::Rt = uycpu::Rt {
    miss,
    safe_point,
    bad_label,
    came_back,
    trap,
    brk,
    kernel_only,
    spin: kernel::spin,
    syscall: kernel::syscall,
    ei: kernel::ei,
    mfc0: kernel::mfc0,
    mtc0: kernel::mtc0,
    cfc2: vu0::cfc2,
    ctc2: vu0::ctc2,
    vcallms: vu0::vcallms,
    vu0_busy: vu0::vu0_busy,
    rd_slow: mem::rd_slow,
    wr_slow: mem::wr_slow,
    rd128_slow: mem::rd128_slow,
    wr128_slow: mem::wr128_slow,
    native,
};

/// No translation at `t` in the table: code may have arrived in RAM since the last check (the loader jumps into the main
/// program it unpacked without a FlushCache), so the programs in RAM are checked again; still none, the game stops.
extern "C-unwind" fn miss(c: &mut Cpu, t: u32, from: u32) {
    c.mach().flush_cache();
    match lookup(t) {
        Some(f) => f(c, t),
        None => no_code(c, t, from),
    }
}

#[cold]
#[inline(never)]
pub fn no_code(c: &mut Cpu, t: u32, from: u32) -> ! {
    let what = c.mach().resident();
    panic!("no translated code at {t:08x} (called from {from:08x}; programs in RAM: {what})")
}

/// A loop's back edge while the host waits for the field's end: the field's time used up, the thread goes to the host; the
/// field run far too long, the game stops here with the loop's address.
extern "C-unwind" fn safe_point(c: &mut Cpu, pc: u32) {
    if machine::SAFE.load(std::sync::atomic::Ordering::Relaxed) & machine::STALL != 0 {
        stalled(c, pc)
    }
    kernel::preempt(c, pc);
}

#[cold]
#[inline(never)]
fn stalled(c: &mut Cpu, pc: u32) -> ! {
    let f = c.mach().field;
    let st = kernel::state(c);
    panic!("field {f} did not end in {} s: thread {} loops at {pc:08x} (ra {:08x}); {st}", machine::STALL_SECS, c.thread, c.r[31] as u32)
}

extern "C-unwind" fn bad_label(_c: &mut Cpu, f: u32, l: u32) -> ! {
    panic!("function {f:08x} entered at {l:08x}, which is not one of its labels")
}

/// `Cpu::came_back`: the last call that came back elsewhere, for the stop that names where a thread or handler ended.
extern "C-unwind" fn came_back(c: &mut Cpu, at: u32) {
    let ret = c.ret;
    c.mach().came_back = Some((at, ret));
}

/// The last call that came back somewhere other than the instruction after it, for a stop's message.
pub fn came_back_note(m: &Machine) -> String {
    match m.came_back {
        Some((at, ret)) => format!(" (the last call that came back elsewhere: the call at {at:08x}, to {ret:08x})"),
        None => String::new(),
    }
}

extern "C-unwind" fn trap(c: &mut Cpu, at: u32) -> ! {
    // What the code at the trap was working on (its registers; level 35's LZ unpacker: a0 the stream, t3 its next chunk, t5 the
    // output, t7 the token pointer in the scratchpad), the stream's first bytes and the last disc reads, so a stream overwritten
    // by a read is seen in the stop's message.
    let r: Vec<String> = (1..32).map(|k| format!("r{k} {:x}", c.r[k] as u32)).collect();
    // UYA_TRAP_DUMP=DIR: the RAM and the scratchpad as they stand at the trap, to compare with the disc.
    if let Some(dir) = std::env::var_os("UYA_TRAP_DUMP") {
        let dir = std::path::PathBuf::from(dir);
        let f = c.mach().field;
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("UYA_TRAP_DUMP {}: {e}", dir.display()));
        std::fs::write(dir.join(format!("ram{f:05}.bin")), &c.mach().ram[..]).unwrap_or_else(|e| panic!("UYA_TRAP_DUMP: {e}"));
        std::fs::write(dir.join(format!("spr{f:05}.bin")), &c.mach().spr[..]).unwrap_or_else(|e| panic!("UYA_TRAP_DUMP: {e}"));
    }
    let a0 = c.r[4] as u32 & 0x01ff_ffff;
    let head = c.mach().ram.get(a0 as usize..a0 as usize + 16).map(|b| format!("{b:02x?}")).unwrap_or_default();
    panic!(
        "trap instruction at {at:08x} (ra {:08x}) in field {}; registers {}; the 16 bytes at a0: {head}; the last disc reads: {}{}",
        c.r[31] as u32,
        c.mach().field,
        r.join(" "),
        iop::reads_note(),
        kernel::guard_counts()
    )
}

extern "C-unwind" fn brk(c: &mut Cpu, at: u32, code: u32) -> ! {
    panic!("BREAK {code:#x} at {at:08x} (ra {:08x})", c.r[31] as u32)
}

extern "C-unwind" fn kernel_only(_c: &mut Cpu, at: u32, what: *const u8, n: usize) -> ! {
    let what = String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(what, n) });
    panic!("{what} at {at:08x}: a kernel instruction in the game's code")
}

/// The game functions answered whole (`uyatr`'s NATIVE).
extern "C-unwind" fn native(c: &mut Cpu, k: u32) {
    match k {
        0 => kernel::no_bios_scan(c),
        1 => machine::movie_player(c),
        2 | 3 => hw::skin_normal(c, k == 3),
        4 | 6 => mobydraw::start(c, k),
        5 | 7 => mobydraw::after(c, k),
        8 | 10 | 12 | 14 => collide::start(c, k),
        9 | 11 | 13 | 15 => collide::after(c, k),
        16 | 18 => shrubcull::start(c, k),
        17 | 19 => shrubcull::after(c, k),
        20 | 24 => tiecull::start(c, k),
        21 | 25 => tiecull::after(c, k),
        28 => fxpool::start(c, k),
        29 => fxpool::after(c, k),
        32 | 36 => skycull::start(c, k),
        33 | 37 => skycull::after(c, k),
        40 | 42 | 44 | 46 | 48 | 50 | 52 | 54 | 56 => anim::start(c, k),
        41 | 43 | 45 | 47 | 49 | 51 | 53 | 55 | 57 => anim::after(c, k),
        _ => panic!("native function {k}: none such"),
    }
}
