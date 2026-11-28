//! The machine the translated code runs on, and the frame loop.
//!
//! `Machine::boot` lays the disc's boot program (SCUS_973.53, an ELF) into RAM as the PS2's loader would and makes its main
//! thread. `Machine::field` runs one field: the vertical blank (the kernel's VBLANK handlers, the alarms), then the game's threads
//! until every one of them waits (for a semaphore, for the next vertical blank: a loop that only polls, `Cpu::spin`). What the
//! field's DMA sent to the GS (`hw::Job`) is handed to the caller, which draws it.
//!
//! Each kernel thread runs on a host thread of its own; one runs at a time, by turns (`turn`): the turn passes at the kernel calls
//! that block or switch, and comes back to the host when no thread can run.

use crate::hw::Job;
use crate::vu0::Vu0Prog;
use crate::{Cpu, Ext, Program};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// Whose turn it is: 0 the host (between fields), N kernel thread N.
static TURN: Mutex<u32> = Mutex::new(0);
static TURN_CV: Condvar = Condvar::new();

/// Turns given (`give`), the time stamp counter ticks from a turn given to its thread running (`wait_turn`, and the host's in
/// `wait_field_end`), and when the last turn was given: for the host's report (`Machine::take_prof`).
static TURNS: AtomicU64 = AtomicU64::new(0);
static TURN_TICKS: AtomicU64 = AtomicU64::new(0);
static GIVEN: AtomicU64 = AtomicU64::new(0);
/// The same for the field running now (`Machine::field`, `FieldWaits`): its turns, their ticks from given to running and the
/// longest, and the nanoseconds the host waited past the field's deadline for a running thread to reach a loop.
static FIELD_TURNS: AtomicU64 = AtomicU64::new(0);
static FIELD_TURN_TICKS: AtomicU64 = AtomicU64::new(0);
static FIELD_TURN_MAX: AtomicU64 = AtomicU64::new(0);
static FIELD_PAST_NS: AtomicU64 = AtomicU64::new(0);
/// Whether a host service (a movie, `SERVICE`) ran in the field running now (`FieldWaits::service`).
static FIELD_SERVICE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn give(to: u32) {
    GIVEN.store(tsc(), Ordering::Relaxed);
    TURNS.fetch_add(1, Ordering::Relaxed);
    FIELD_TURNS.fetch_add(1, Ordering::Relaxed);
    *TURN.lock().expect("turn lock") = to;
    TURN_CV.notify_all();
}

/// The turns given since the host's last report (`give`): a kick sees by it whether its handlers passed the turn on
/// (`hw::kick_timed`).
pub(crate) fn turns() -> u64 {
    TURNS.load(Ordering::Relaxed)
}

/// The turn given (`give`) has reached its thread.
fn turn_taken() {
    let d = tsc().saturating_sub(GIVEN.load(Ordering::Relaxed));
    TURN_TICKS.fetch_add(d, Ordering::Relaxed);
    FIELD_TURN_TICKS.fetch_add(d, Ordering::Relaxed);
    FIELD_TURN_MAX.fetch_max(d, Ordering::Relaxed);
}

/// What a loop's back edge (`Cpu::back`) acts on, set by the host while it waits for a field's end: `PREEMPT`, the field's
/// time is used up; `STALL`, the field has run `STALL_SECS`.
pub static SAFE: AtomicU32 = AtomicU32::new(0);
pub const PREEMPT: u32 = 1;
pub const STALL: u32 = 2;
pub const STALL_SECS: u64 = 10;
/// Set while a host service runs inside a field (the movie player: a movie plays within one field, as the console's player
/// blocks the game), so the field's stall clock waits for it.
pub static SERVICE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// A field's time, as the console's: 1/59.94 s.
pub const FIELD_TIME: Duration = Duration::from_nanos(16_683_333);

/// How long the host waits before it cuts a running thread off at its next loop (`wait_field_end`): `FIELD_TIME`, or
/// UYA_FIELD_TIME_US=N microseconds, an instrument. A shorter field cuts long loops off as a loaded PC does, so what only load
/// showed happens every time (level 35's LZ unpacker cut off part way through its stream, R3-034).
pub fn field_time() -> Duration {
    static T: std::sync::OnceLock<Duration> = std::sync::OnceLock::new();
    *T.get_or_init(|| match std::env::var("UYA_FIELD_TIME_US") {
        Ok(v) => Duration::from_micros(v.parse().unwrap_or_else(|_| panic!("UYA_FIELD_TIME_US={v}: a number of microseconds"))),
        Err(_) => FIELD_TIME,
    })
}

/// The host's wait for a field's end. At `deadline` the field's time is used up: a thread still running goes to the host at its
/// next loop back edge (if interrupts reach it), as the console's vertical blank comes in the middle of a loop that waits by
/// running (the front end's sound wait at 0x3a2a10: the sound engine's clock moves only between fields), and goes on there next
/// field. Past `STALL_SECS` more (a loop with interrupts off) the next back edge stops the game; a thread that reaches none in
/// 5 s more is stuck inside the runtime, and the host stops the game.
pub(crate) fn wait_field_end(field: u64, deadline: Instant, thread: impl Fn() -> u32) {
    let mut t = TURN.lock().expect("turn lock");
    let (g, r) = TURN_CV.wait_timeout_while(t, deadline.saturating_duration_since(Instant::now()), |t| *t != 0).expect("turn lock");
    t = g;
    let past = Instant::now();
    if r.timed_out() {
        SAFE.fetch_or(PREEMPT, Ordering::Relaxed);
        // A host service (a movie) runs as long as it runs: the stall clock starts again while one does.
        let r = loop {
            let (g, r) = TURN_CV.wait_timeout_while(t, Duration::from_secs(STALL_SECS), |t| *t != 0).expect("turn lock");
            t = g;
            if !r.timed_out() || !SERVICE.load(Ordering::Relaxed) {
                break r;
            }
        };
        if r.timed_out() {
            SAFE.fetch_or(STALL, Ordering::Relaxed);
            let (g, r) = TURN_CV.wait_timeout_while(t, Duration::from_secs(5), |t| *t != 0).expect("turn lock");
            t = g;
            if r.timed_out() {
                drop(t);
                panic!("field {field} did not end in {STALL_SECS} s, and thread {} reached no loop in 5 s more: it is stuck in the runtime", thread());
            }
        }
    }
    drop(t);
    if r.timed_out() {
        FIELD_PAST_NS.fetch_add(past.elapsed().as_nanos() as u64, Ordering::Relaxed);
    }
    turn_taken();
    SAFE.store(0, Ordering::Relaxed);
}

pub(crate) fn wait_turn(me: u32) {
    let mut t = TURN.lock().expect("turn lock");
    while *t != me {
        t = TURN_CV.wait(t).expect("turn lock");
    }
    drop(t);
    turn_taken();
}

/// The machine pointer as it crosses to the threads (only the thread whose turn it is touches the machine).
#[derive(Clone, Copy)]
pub(crate) struct MPtr(pub *mut Machine);
unsafe impl Send for MPtr {}

/// VU0's registers between two contexts (one VU0, whichever thread runs).
#[derive(Clone, Copy)]
pub struct Vu0Regs {
    pub vf: [[u32; 4]; 32],
    pub vacc: [u32; 4],
    pub vi: [u32; 32],
}

/// What a field leaves for the host.
pub struct FieldOut {
    /// The jobs of the field's last frame (`hw::kick`): its draw lists' words still in `Machine::frame_ram` (`hw::Chain`).
    pub jobs: Vec<Job>,
    /// The skin job's joint matrices of the field (`hw::SkinJoints`).
    pub skin: crate::hw::SkinJoints,
    /// How many jobs of frames before it the field had (the game ran late and caught up), not handed over, and those jobs.
    pub earlier: usize,
    pub earlier_jobs: Vec<Job>,
    /// The field's image transfers into GS memory (`hw::GsUpload`), every frame's, their jobs counted from the field's first.
    pub uploads: Vec<crate::hw::GsUpload>,
    /// Whether a frame was kicked this field: `Machine::frame_ram` holds the RAM as it stood at the last frame's first kick.
    pub kicked: bool,
    /// The thread the field's end found running, the loop's back edge it stopped at and its return address (`Cpu::preempt`);
    /// None when every thread waited.
    pub preempted: Option<(u32, u32, u32)>,
    /// Where the field's time went (`FieldWaits`).
    pub waits: FieldWaits,
}

/// Where a field's time went (ms), for the host's line on a slow field (`rtgame`).
#[derive(Clone, Copy, Default)]
pub struct FieldWaits {
    /// The field in all: its vertical blank, the game's threads until all of them wait or the field's time is up, and the GS
    /// events' walk joined at its end.
    pub ms: f64,
    /// Waited past the field's deadline for a thread still running to reach a loop.
    pub past_deadline: f64,
    /// The turns passed between the game's threads and the host, the time from a turn given to its thread running in all, and
    /// the longest.
    pub turns: u64,
    pub turn_wait: f64,
    pub turn_wait_max: f64,
    /// The RAM copied for the field's frames at their first kicks, and the kicks' and the field end's waits for the GS
    /// events' walk on its worker (`gsstate::GsState::join`).
    pub ram_copy: f64,
    pub walk_wait: f64,
    /// A host service (a movie) ran in the field: the field is as long as the movie.
    pub service: bool,
}

pub struct Machine {
    /// Watched for writes, so a copy of it takes only the pages written since (`ram::Ram::copy_to`).
    pub ram: crate::ram::Ram,
    pub spr: Vec<u8>,
    pub vu0d: Vec<u8>,
    pub vu0m: Vec<u8>,
    pub vu1d: Vec<u8>,
    pub vu1m: Vec<u8>,
    programs: &'static [Program],
    active: Vec<bool>,
    /// The level libraries loaded so far, by program: what each holds (`load_lib`). A library stays loaded once its level has
    /// been played.
    libs: Vec<Option<&'static Program>>,
    vu0_progs: &'static [Vu0Prog],
    /// Bumped by every change of micro memory; the programs found in it at `vu0_ok_gen`.
    pub vu0_gen: u64,
    vu0_ok_gen: u64,
    vu0_ok: Vec<bool>,
    pub vu0_parked: Option<usize>,
    pub vu0r: Vu0Regs,
    pub k: crate::kernel::Kernel,
    pub hw: crate::hw::Hw,
    pub iop: crate::iop::Iop,
    pub iso: String,
    pub field: u64,
    /// When the field being run started (its time ends `FIELD_TIME` later).
    pub field_start: Instant,
    /// This field's preemption (`FieldOut::preempted`).
    pub preempted: Option<(u32, u32, u32)>,
    /// A control warp waiting for the main thread's next idle poll.
    pub(crate) queued_chunk: Option<(u32, u32)>,
    loading_chunk: bool,
    /// The RAM (its first `RAM_SIZE` bytes) as it stood at the first kick of the last frame a thread kicked (`hw::kick`), with
    /// the blocks of the frame's draw lists read from it and the scratchpad as it stood then (`hw::Chain`): the field's jobs
    /// are built from it, so it goes with them to the renderer.
    pub frame_ram: crate::ram::RamCopy,
    /// Time stamp counter ticks spent in the fields, and of them in VU0 microprograms, in DMA kicks' own work and, in the kicks,
    /// in copying the RAM for the frame, and in the game code of the DMA end handlers the kicks ran, for the host's report
    /// (`take_prof`).
    pub prof: [u64; 5],
    /// For the same report: each DMA channel's kicks and their own ticks, and each translated VU0 microprogram's runs and ticks.
    pub kicks: [(u64, u64); 10],
    pub vu0_runs: Vec<(u64, u64)>,
    /// A thread's error, for the host.
    pub error: Option<String>,
    /// The host's movie service (`movie_player`).
    pub movie: Option<MovieService>,
    /// The last call that came back somewhere other than the instruction after it (`Cpu::came_back`): its address and where.
    pub came_back: Option<(u32, u32)>,
}

impl Machine {
    /// The machine with the disc's boot program in RAM, its main thread ready at the ELF's entry.
    pub fn boot(iso: &str, programs: &'static [Program], vu0_progs: &'static [Vu0Prog]) -> Result<Box<Machine>, String> {
        let mut vf = [[0u32; 4]; 32];
        vf[0] = [0, 0, 0, 0x3f80_0000];
        let mut m = Box::new(Machine {
            ram: crate::ram::Ram::new(crate::RAM_SIZE as usize + 64)?,
            spr: vec![0u8; 0x4000 + 16],
            vu0d: vec![0u8; 0x1000],
            vu0m: vec![0u8; 0x1000],
            vu1d: vec![0u8; 0x4000],
            vu1m: vec![0u8; 0x4000],
            programs,
            active: vec![false; programs.len()],
            libs: vec![None; programs.len()],
            vu0_progs,
            vu0_gen: 1,
            vu0_ok_gen: 0,
            vu0_ok: vec![false; vu0_progs.len()],
            vu0_parked: None,
            vu0r: Vu0Regs { vf, vacc: [0; 4], vi: [0; 32] },
            k: crate::kernel::Kernel::default(),
            hw: crate::hw::Hw::default(),
            iop: crate::iop::Iop::new(iso)?,
            iso: iso.to_string(),
            field: 0,
            field_start: Instant::now(),
            preempted: None,
            queued_chunk: None,
            loading_chunk: false,
            frame_ram: crate::ram::RamCopy::new(),
            prof: [0; 5],
            kicks: [(0, 0); 10],
            vu0_runs: vec![(0, 0); vu0_progs.len()],
            error: None,
            movie: None,
            came_back: None,
        });
        let entry = m.load_elf("/SCUS_973.53")?;
        m.k.boot_thread(entry);
        m.flush_cache();
        Ok(m)
    }

    /// Lays out an ELF from the disc's file system (its PT_LOAD segments, zero-filled to their memory size); its entry.
    fn load_elf(&mut self, path: &str) -> Result<u32, String> {
        let elf = self.iop.disc_file(path)?;
        let u32_at = |o: usize| -> Result<u32, String> { Ok(u32::from_le_bytes(elf.get(o..o + 4).ok_or_else(|| format!("{path}: short ELF"))?.try_into().unwrap())) };
        if elf.get(..4) != Some(&b"\x7fELF"[..]) {
            return Err(format!("{path}: not an ELF"));
        }
        let entry = u32_at(0x18)?;
        let (phoff, phnum) = (u32_at(0x1c)? as usize, u16::from_le_bytes([elf[0x2c], elf[0x2d]]) as usize);
        for k in 0..phnum {
            let p = phoff + 32 * k;
            if u32_at(p)? != 1 {
                continue;
            }
            let (off, vaddr, filesz, memsz) = (u32_at(p + 4)? as usize, u32_at(p + 8)? as usize, u32_at(p + 16)? as usize, u32_at(p + 20)? as usize);
            let src = elf.get(off..off + filesz).ok_or_else(|| format!("{path}: segment past the file"))?;
            let dst = self.ram.get_mut(vaddr..vaddr + memsz).ok_or_else(|| format!("{path}: segment at {vaddr:#x} outside RAM"))?;
            dst[..filesz].copy_from_slice(src);
            dst[filesz..].fill(0);
        }
        Ok(entry)
    }

    fn word(&self, a: u32) -> u32 {
        u32::from_le_bytes(self.ram[a as usize..a as usize + 4].try_into().unwrap())
    }

    fn holds(&self, p: &Program) -> bool {
        if p.text.1 > crate::RAM_SIZE {
            return false;
        }
        if !p.sample.iter().all(|&(a, w)| self.word(a) == w) {
            return false;
        }
        let h = (p.text.0..p.text.1).step_by(4).fold(0xcbf2_9ce4_8422_2325u64, |h, a| (h ^ self.word(a) as u64).wrapping_mul(0x100_0000_01b3));
        h == p.hash
    }

    /// The kernel's FlushCache (and the boot): which translated programs' code is in RAM now. A program whose code left RAM
    /// leaves the table; one whose code arrived enters it (checked word for word by its hash).
    /// A story level's program is a library of its own (`Program::lib`), loaded the first time its code is in RAM.
    pub fn flush_cache(&mut self) {
        for (i, p) in self.programs.iter().enumerate() {
            let sampled = p.text.1 <= crate::RAM_SIZE && p.sample.iter().all(|&(a, w)| self.word(a) == w);
            if self.active[i] && !sampled {
                crate::install(self.libs[i].unwrap_or(p), false);
                self.active[i] = false;
            }
        }
        for (i, p) in self.programs.iter().enumerate() {
            if !self.active[i] && self.holds(p) {
                let code = if p.lib.is_empty() {
                    p
                } else {
                    *self.libs[i].get_or_insert_with(|| load_lib(p).unwrap_or_else(|e| panic!("{e}")))
                };
                crate::install(code, true);
                self.active[i] = true;
            }
        }
        // The story level whose code is in RAM now (a program linked apart: `Program::lib`, named lNN), for the copies made
        // from here on (`RamCopy::level`).
        self.ram.level = self.programs.iter().zip(&self.active).find(|(p, a)| **a && !p.lib.is_empty()).map(|(p, _)| p.name[1..].parse().unwrap_or_else(|_| panic!("story level program {}: not named lNN", p.name)));
    }

    /// Whether a front end's program (the menus', the space scenes') is in RAM: a level's code is not then (they share their
    /// addresses), so the frames are the front end's own.
    pub fn front_end(&self) -> bool {
        self.programs.iter().zip(&self.active).any(|(p, a)| *a && p.name.starts_with("fe"))
    }

    /// The code, [lo, hi), of each program in RAM now (`flush_cache`): a level's own functions are there, and not in what an
    /// earlier level's code left in RAM past the end of the code that replaced it.
    pub fn resident_text(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.programs.iter().zip(&self.active).filter(|(_, a)| **a).map(|(p, _)| p.text)
    }

    /// The program in RAM (`flush_cache`) whose code holds `a`, by name: the table runs its translation there.
    pub(crate) fn program_at(&self, a: u32) -> Option<&'static str> {
        self.programs.iter().zip(&self.active).find(|(p, on)| **on && (p.text.0..p.text.1).contains(&a)).map(|(p, _)| p.name)
    }

    /// The programs whose code is in RAM, for messages; for each other program, how many of its sample words RAM holds and
    /// the first it does not, or that its samples match but its hash does not.
    pub fn resident(&self) -> String {
        let v: Vec<&str> = self.programs.iter().zip(&self.active).filter(|(_, a)| **a).map(|(p, _)| p.name).collect();
        let mut s = if v.is_empty() { "none".to_string() } else { v.join(", ") };
        for (p, _) in self.programs.iter().zip(&self.active).filter(|(_, a)| !**a) {
            if p.text.1 > crate::RAM_SIZE {
                continue;
            }
            let ok = p.sample.iter().filter(|&&(a, w)| self.word(a) == w).count();
            match p.sample.iter().find(|&&(a, w)| self.word(a) != w) {
                Some(&(a, w)) => s += &format!("; {}: {ok} of {} samples, {a:08x} holds {:08x}, not {w:08x}", p.name, p.sample.len(), self.word(a)),
                None => s += &format!("; {}: every sample, but not the hash", p.name),
            }
        }
        s
    }

    /// The translated VU0 microprogram whose code micro memory holds around `a`.
    pub fn vu0_program(&mut self, a: usize) -> Option<&'static Vu0Prog> {
        self.vu0_index(a).map(|(_, p)| p)
    }

    /// The translated microprogram that micro memory holds at `a`, and its index in `vu0_progs`.
    pub fn vu0_index(&mut self, a: usize) -> Option<(usize, &'static Vu0Prog)> {
        if self.vu0_ok_gen != self.vu0_gen {
            for (i, p) in self.vu0_progs.iter().enumerate() {
                self.vu0_ok[i] = p.base + p.code.len() <= self.vu0m.len() && self.vu0m[p.base..p.base + p.code.len()] == p.code[..];
            }
            self.vu0_ok_gen = self.vu0_gen;
        }
        let progs = self.vu0_progs;
        self.vu0_ok.iter().zip(progs).position(|(ok, p)| *ok && a >= p.base && a < p.base + p.code.len()).map(|i| (i, &progs[i]))
    }

    /// Which translated microprograms micro memory holds, for messages.
    pub fn vu0_census(&mut self) -> String {
        let _ = self.vu0_program(usize::MAX);
        let v: Vec<&str> = self.vu0_progs.iter().zip(&self.vu0_ok).filter(|(_, ok)| **ok).map(|(p, _)| p.name).collect();
        let h = self.vu0m.chunks(4).fold(0xcbf2_9ce4_8422_2325u64, |h, w| (h ^ u32::from_le_bytes(w.try_into().unwrap()) as u64).wrapping_mul(0x100_0000_01b3));
        format!("holds {} of the translated ones, FNV {h:016x}", if v.is_empty() { "none".to_string() } else { v.join(", ") })
    }

    /// One field: the vertical blank, then the game until all its threads wait. The pad state is the field's report
    /// (`iop::pad`).
    pub fn field(&mut self, pad: [u8; 18]) -> Result<FieldOut, String> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        let old = crate::fpu::set_mode(crate::fpu::MXCSR);
        let t0 = tsc();
        self.field += 1;
        self.field_start = Instant::now();
        for s in [&FIELD_TURNS, &FIELD_TURN_TICKS, &FIELD_TURN_MAX, &FIELD_PAST_NS] {
            s.store(0, Ordering::Relaxed);
        }
        FIELD_SERVICE.store(false, Ordering::Relaxed);
        let (copied0, walked0) = (self.prof[3], self.hw.walk.waited);
        self.hw.field_start();
        self.iop.pad.set(pad);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mp = self as *mut Machine;
            crate::kernel::vblank(mp);
            crate::kernel::run_threads(mp);
            // The walk of the field's last kicks' GS events on its worker thread (`hw::gsstate`): done, and its panic raised, here.
            unsafe { &mut *mp }.hw.walk.join();
        }));
        crate::fpu::set_mode(old);
        let ticks = tsc() - t0;
        self.prof[0] += ticks;
        let ms = self.field_start.elapsed().as_secs_f64() * 1e3;
        let tick_ms = ms / ticks as f64;
        let ticks_ms = |t: u64| t as f64 * tick_ms;
        let waits = FieldWaits {
            ms,
            past_deadline: FIELD_PAST_NS.load(Ordering::Relaxed) as f64 / 1e6,
            turns: FIELD_TURNS.load(Ordering::Relaxed),
            turn_wait: ticks_ms(FIELD_TURN_TICKS.load(Ordering::Relaxed)),
            turn_wait_max: ticks_ms(FIELD_TURN_MAX.load(Ordering::Relaxed)),
            ram_copy: ticks_ms(self.prof[3] - copied0),
            walk_wait: ticks_ms(self.hw.walk.waited - walked0),
            service: FIELD_SERVICE.load(Ordering::Relaxed),
        };
        if let Err(p) = r {
            let e = panic_text(&p);
            self.error = Some(format!("field {}: {e}", self.field));
        }
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        let mut jobs = std::mem::take(&mut self.hw.jobs);
        let earlier = self.hw.frame_from;
        let earlier_jobs: Vec<Job> = jobs.drain(..earlier).collect();
        let uploads = self.hw.walk.take_uploads();
        let skin = self.hw.skin.take_field(self.preempted.is_some());
        Ok(FieldOut { jobs, skin, earlier, earlier_jobs, uploads, kicked: self.hw.kicked, preempted: self.preempted.take(), waits })
    }
}

/// The time stamp counter (the report's clock: cheap enough to read around every microprogram run).
#[inline(always)]
pub fn tsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

impl Machine {
    /// The game's disc-backed terrain and collision loader can yield across fields.
    pub fn queue_chunk(&mut self, loader: u32, chunk: u32) -> Result<(), String> {
        if self.loading_chunk { return Err("a level chunk is already loading".into()); }
        self.queued_chunk = Some((loader, chunk));
        self.loading_chunk = true;
        Ok(())
    }

    pub fn loading_chunk(&self) -> bool { self.loading_chunk }

    pub(crate) fn chunk_loaded(&mut self) { self.loading_chunk = false; }

    /// The main thread's gp (kernel thread 1's, as the boot program set it up).
    pub fn main_gp(&self) -> Result<u32, String> {
        self.k.threads.first().and_then(|t| t.as_ref()).map(|t| t.gp).ok_or_else(|| "the game's main thread is not made yet".to_string())
    }

    /// Calls the game's function at `f` between fields, on the host (a level request for --start and --warp: the control
    /// channel's `request_level`): a context of its own on the interrupt stack, with the main thread's gp, interrupts off and the
    /// arguments in a0..t3. Returns the low word of v0. The game's threads are where the field left them.
    pub fn call_between(&mut self, f: u32, args: &[u32]) -> Result<u32, String> {
        assert!(args.len() <= 8, "the EE passes eight arguments in registers (a0 to t3)");
        let gp = self.main_gp()?;
        let mp = self as *mut Machine;
        let old = crate::fpu::set_mode(crate::fpu::MXCSR);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut c = crate::cpu(mp);
            c.thread = 0;
            c.status &= !1;
            c.r[29] = crate::kernel::INT_STACK as u64;
            c.r[28] = gp as i32 as i64 as u64;
            // a0 to a3, then t0 to t3; those not given 0.
            for k in 0..8 {
                c.r[4 + k] = args.get(k).map_or(0, |&a| a as i32 as i64 as u64);
            }
            c.r[31] = crate::kernel::HANDLER_RET as u64;
            c.call(f, 0);
            (c.ret, c.r[2] as u32)
        }));
        crate::fpu::set_mode(old);
        match r {
            Ok((crate::kernel::HANDLER_RET, value)) => Ok(value),
            Ok((ret, _)) => Err(format!("the call of {f:08x} between fields came back to {ret:08x}{}", crate::came_back_note(self))),
            Err(p) => Err(format!("the call of {f:08x} between fields: {}", panic_text(&p))),
        }
    }

    /// Where the fields' time went since the last call (`Prof`).
    pub fn take_prof(&mut self) -> Prof {
        let [all, vu0, dma, copy, handlers] = std::mem::take(&mut self.prof);
        let all = all.max(1) as f64;
        let share = |ticks: u64| ticks as f64 / all;
        let kicks = std::mem::take(&mut self.kicks).map(|(n, t)| (n, share(t)));
        let progs = self.vu0_progs;
        let vu0_progs = self.vu0_runs.iter_mut().zip(progs).filter(|(r, _)| r.0 > 0).map(|(r, p)| (p.name, std::mem::take(r))).map(|(name, (n, t))| (name, n, share(t))).collect();
        let turns = (TURNS.swap(0, Ordering::Relaxed), share(TURN_TICKS.swap(0, Ordering::Relaxed)));
        Prof { vu0: share(vu0), dma: share(dma), copy: share(copy), handlers: share(handlers), kicks, vu0_progs, turns }
    }
}

/// Where the fields' time went (`Machine::take_prof`), as shares of it.
pub struct Prof {
    /// VU0 microprograms, DMA kicks' own work (each kick once: a kick inside another is not in its time) and, in the kicks,
    /// copying the RAM for the frame.
    pub vu0: f64,
    pub dma: f64,
    pub copy: f64,
    /// The game code of the DMA end handlers the kicks ran (the VIF1 end handler's chaining the frame's draw lists among them),
    /// apart from the kicks they made (`hw::kick_timed`).
    pub handlers: f64,
    /// Each DMA channel's kicks and their own work's share.
    pub kicks: [(u64, f64); 10],
    /// The translated VU0 microprograms run: their names, runs and shares.
    pub vu0_progs: Vec<(&'static str, u64, f64)>,
    /// The turns passed between the host threads of the game's threads and the host (`give`), and the share from a turn given
    /// to its thread running.
    pub turns: (u64, f64),
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut std::ffi::c_void;
    fn GetProcAddress(module: *mut std::ffi::c_void, name: *const u8) -> *mut std::ffi::c_void;
    fn GetLastError() -> u32;
}

/// Loads the library holding level program `p`'s code (`tr_lNN.dll` beside the executable: `port/uyatr` writes it,
/// `scripts\build-levels.ps1` builds it) and checks it is that program's translation: the layout of `uycpu` it was built against,
/// the program's name, text and hash.
fn load_lib(p: &Program) -> Result<&'static Program, String> {
    let t = Instant::now();
    let exe = std::env::current_exe().map_err(|e| format!("the executable's path: {e}"))?;
    let path = exe.parent().ok_or("the executable's folder")?.join(format!("{}.dll", p.lib));
    let wide: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain([0]).collect();
    let h = unsafe { LoadLibraryW(wide.as_ptr()) };
    if h.is_null() {
        return Err(format!("{}'s code is in RAM, but its library {} did not load (error {}; scripts\\build-levels.ps1 builds the level libraries)", p.name, path.display(), unsafe { GetLastError() }));
    }
    let sym = |name: &[u8]| {
        let f = unsafe { GetProcAddress(h, name.as_ptr()) };
        if f.is_null() { Err(format!("{}: no {}", path.display(), String::from_utf8_lossy(&name[..name.len() - 1]))) } else { Ok(f) }
    };
    let abi: extern "C" fn() -> u64 = unsafe { std::mem::transmute(sym(b"uya_abi\0")?) };
    let program: extern "C" fn() -> &'static Program = unsafe { std::mem::transmute(sym(b"uya_program\0")?) };
    if abi() != uycpu::ABI {
        return Err(format!("{} was built against another uycpu (ABI {:#x}, the executable's {:#x})", path.display(), abi(), uycpu::ABI));
    }
    let q = program();
    if q.name != p.name || q.text != p.text || q.hash != p.hash {
        return Err(format!("{} holds {} {:08x}..{:08x} hash {:016x}, not {} {:08x}..{:08x} hash {:016x}: it is from another translation", path.display(), q.name, q.text.0, q.text.1, q.hash, p.name, p.text.0, p.text.1, p.hash));
    }
    let labels: usize = q.funcs.iter().map(|f| f.len()).sum();
    println!("uyart: {} loaded for {} in {:.0} ms ({labels} labels)", path.display(), p.name, t.elapsed().as_secs_f64() * 1e3);
    Ok(q)
}

/// A movie service: called with the player's arguments (a0, a1, a3, t0); true when the player skipped the movie.
pub type MovieService = fn(&mut Cpu, [u32; 4]) -> bool;

/// The game's movie player (native, `uyatr`'s NATIVE): the movie goes to the host's movie service. With none attached the
/// port has no movie decoder (item 0.5) and the movie is skipped, as when Start ends it: 1. On the way out bit 0x80 of
/// 0x1D4D40 is cleared, as the player does.
pub fn movie_player(c: &mut Cpu) {
    let args = [c.r[4] as u32, c.r[5] as u32, c.r[7] as u32, c.r[8] as u32];
    let service = c.mach().movie;
    let skipped = match service {
        Some(f) => {
            SERVICE.store(true, Ordering::Relaxed);
            FIELD_SERVICE.store(true, Ordering::Relaxed);
            let skipped = f(c, args);
            SERVICE.store(false, Ordering::Relaxed);
            skipped
        }
        None => {
            println!("uyart: movie {:08x} {:08x}: skipped (no movie decoder attached)", args[0], args[1]);
            true
        }
    };
    let f = c.rd32(0x1d_4d40);
    c.wr32(0x1d_4d40, f & !0x80);
    c.r[2] = skipped as u64;
}

/// A panic's message.
pub fn panic_text(p: &Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "a panic without a message".into())
}
