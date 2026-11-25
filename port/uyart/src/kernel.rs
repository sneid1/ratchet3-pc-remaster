//! The EE kernel's calls, native: what the PS2's kernel does for each syscall number the game's code makes (`Cpu::syscall`,
//! $v1 the number, negative for the interrupt-context "i" calls), with the thread and semaphore rules of that kernel: the highest
//! priority ready thread runs (a lower number is higher), a thread runs until it waits, ends or a higher one is ready; threads of
//! one priority take turns only at RotateThreadReadyQueue.
//!
//! Interrupts: the vertical blank is raised by the frame loop (`vblank`), a DMA channel's end by its kick (`hw`). Their
//! handlers (AddIntcHandler, AddDmacHandler) run on a context of their own with the kernel's interrupt stack; one at a time, as
//! with the hardware's exception level: an interrupt raised inside a handler, or while the running thread has them disabled,
//! waits until the handler returns or the thread enables them.
//!
//! A call this kernel does not answer stops the game with its number and arguments.

use crate::machine::{give, wait_turn, MPtr, Machine};
use crate::{Cpu, Ext};
use std::collections::VecDeque;

/// $ra given to a thread's entry and to handlers: their return comes back to the kernel.
pub const THREAD_EXIT: u32 = 0x8000_1000;
pub const HANDLER_RET: u32 = 0x8000_1010;
/// The kernel's interrupt stack (kernel RAM below the program), and the stack of the calls the port makes between fields
/// (`Machine::call_between`). Above 0x9E000: RAM from 0x10000 to there holds a cutscene's SCENE block (`uya::scenespace`).
pub(crate) const INT_STACK: u32 = 0x000f_f000;

const THS_RUN: u32 = 1;
const THS_READY: u32 = 2;
const THS_WAIT: u32 = 4;
const THS_SUSPEND: u32 = 8;
const THS_DORMANT: u32 = 0x10;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wait {
    None,
    Sleep,
    Sema(u32),
}

pub struct Thread {
    pub status: u32,
    pub prio: u32,
    pub init_prio: u32,
    pub entry: u32,
    pub stack: u32,
    pub stack_size: u32,
    pub gp: u32,
    pub attr: u32,
    pub option: u32,
    pub wait: Wait,
    pub wakeups: u32,
    pub suspended: bool,
    /// Ready-queue order within a priority.
    pub order: u64,
    /// The host thread's token (a terminated thread's host thread never gets a turn again), 0 until it is started.
    pub token: u32,
    /// The registers it starts with: (entry, sp, gp, a0, a1).
    pub start: Option<(u32, u32, u32, u32, u32)>,
}

pub struct Sema {
    pub count: i32,
    pub max: i32,
    pub init: i32,
    pub attr: u32,
    pub option: u32,
    pub waiters: VecDeque<u32>,
}

#[derive(Clone, Copy)]
struct Handler {
    id: u32,
    func: u32,
    arg: u32,
    gp: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Int {
    Intc(u32),
    Dmac(u32),
}

#[derive(Default)]
pub struct Kernel {
    pub threads: Vec<Option<Thread>>,
    pub semas: Vec<Option<Sema>>,
    intc: Vec<Vec<Handler>>,
    dmac: Vec<Vec<Handler>>,
    next_handler: u32,
    pub current: u32,
    order: u64,
    next_token: u32,
    /// SetVSyncFlag's (flag, csr) pointers, written at the next vertical blank.
    vsync: Option<(u32, u32)>,
    pub heap_end: u32,
    pub stack_bottom: u32,
    pending: VecDeque<Int>,
    in_handler: u32,
    pub gs_crt: (u32, u32, u32),
    pub gs_imr: u32,
    pub sif_regs: std::collections::HashMap<u32, u32>,
    /// Where each thread last polled (`Cpu::spin`), for the report.
    pub polled: std::collections::HashMap<u32, u32>,
    /// Each thread's last pass of a marked loop (`spin`): where, its registers' hash, the field, and the passes this field that
    /// moved its registers.
    spun: std::collections::HashMap<u32, (u32, u64, u64, u32)>,
    pub sif_dma_id: u32,
    pub hsync: u64,
}

/// What unwinds a thread's code back to its host thread's top: the game restarted it (ExecPS2) or ended it.
pub(crate) enum Unwind {
    Restart(u32, u32, u32, u32),
    Exit,
}

impl Kernel {
    /// The threads, for the host's report: id, priority, what each is doing (run, ready, waiting on a semaphore with its count,
    /// sleeping, suspended, dormant), and the handlers' pending interrupts.
    pub fn summary(&self) -> String {
        let mut v = Vec::new();
        for (i, t) in self.threads.iter().enumerate() {
            let Some(t) = t else { continue };
            let what = match (t.status, t.wait) {
                (s, _) if s & THS_DORMANT != 0 => "dormant".to_string(),
                (s, Wait::Sema(id)) if s & THS_WAIT != 0 => {
                    match self.semas.get((id as usize).wrapping_sub(1)).and_then(|s| s.as_ref()) {
                        Some(s) => format!("waits on semaphore {id} (count {})", s.count),
                        None => format!("waits on semaphore {id}, deleted"),
                    }
                }
                (s, Wait::Sleep) if s & THS_WAIT != 0 => "sleeps".to_string(),
                (s, _) if s & THS_RUN != 0 => "runs".to_string(),
                (s, _) if s & THS_READY != 0 => "ready".to_string(),
                (s, _) => format!("status {s:#x}"),
            };
            let polled = self.polled.get(&(i as u32 + 1)).map(|pc| format!(", last polled at {pc:08x}")).unwrap_or_default();
            v.push(format!("{} (prio {}) {what}{}{polled}", i + 1, t.prio, if t.suspended { ", suspended" } else { "" }));
        }
        let handlers = |l: &Vec<Vec<Handler>>| l.iter().enumerate().filter(|(_, h)| !h.is_empty()).map(|(n, h)| format!("{n}: {}", h.iter().map(|h| format!("{:08x}", h.func)).collect::<Vec<_>>().join(" "))).collect::<Vec<_>>().join(", ");
        format!("threads: {}; pending {:?}; INTC handlers {}; DMAC handlers {}", v.join(", "), self.pending, handlers(&self.intc), handlers(&self.dmac))
    }

    pub fn boot_thread(&mut self, entry: u32) {
        self.intc = vec![Vec::new(); 16];
        self.dmac = vec![Vec::new(); 16];
        self.next_handler = 1;
        self.next_token = 1;
        self.threads.clear();
        self.threads.push(Some(Thread {
            status: THS_READY,
            prio: 0,
            init_prio: 0,
            entry,
            stack: 0,
            stack_size: 0,
            gp: 0,
            attr: 0,
            option: 0,
            wait: Wait::None,
            wakeups: 0,
            suspended: false,
            order: 0,
            token: 0,
            start: Some((entry, crate::RAM_SIZE - 0x10, 0, 0, 0)),
        }));
        self.semas.clear();
        self.current = 0;
    }

    fn th(&mut self, id: u32) -> Option<&mut Thread> {
        self.threads.get_mut((id as usize).wrapping_sub(1)).and_then(|t| t.as_mut())
    }

    fn ready(&mut self, id: u32) {
        self.order += 1;
        let o = self.order;
        if let Some(t) = self.th(id) {
            t.status = THS_READY;
            t.wait = Wait::None;
            t.order = o;
        }
    }

    /// The thread that should run: the highest priority (lowest number) ready one; the running one keeps the CPU against its
    /// equals.
    fn pick(&self) -> Option<u32> {
        let mut best: Option<(u32, u32, u64, u32)> = None;
        for (i, t) in self.threads.iter().enumerate() {
            let Some(t) = t else { continue };
            let id = i as u32 + 1;
            if (t.status != THS_READY && t.status != THS_RUN) || t.suspended {
                continue;
            }
            let key = (t.prio, if id == self.current { 0 } else { 1 }, t.order, id);
            if best.is_none_or(|b| key < b) {
                best = Some(key);
            }
        }
        best.map(|b| b.3)
    }
}

/// What the host thread of each of the game's threads runs first, set by the program that runs the game before it starts
/// (`uya play`: the core kept for the game's threads). The game's threads take turns with the host's field loop, one running
/// at a time.
pub static THREAD_START: std::sync::OnceLock<fn()> = std::sync::OnceLock::new();

/// The host thread of kernel thread `id`: waits for its turn, runs the thread's code, and gives the turn on when it ends.
fn spawn(mp: MPtr, id: u32, token: u32) {
    std::thread::Builder::new()
        .name(format!("ee thread {id}"))
        .stack_size(64 << 20)
        .spawn(move || {
            if let Some(start) = THREAD_START.get() {
                start();
            }
            let mp = mp;
            wait_turn(token);
            crate::fpu::set_mode(crate::fpu::MXCSR);
            let m = unsafe { &mut *mp.0 };
            let mut c = crate::cpu(mp.0);
            c.thread = id;
            loop {
                let Some((entry, sp, gp, a0, a1)) = m.k.th(id).and_then(|t| t.start.take()) else { break };
                c.r = [0; 32];
                c.rh = [0; 32];
                c.r[29] = sp as i32 as i64 as u64;
                c.r[28] = gp as i32 as i64 as u64;
                c.r[4] = a0 as i32 as i64 as u64;
                c.r[5] = a1 as i32 as i64 as u64;
                c.r[31] = THREAD_EXIT as u64;
                load_vu0(&mut c);
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    c.call(entry, 0);
                    if c.ret != THREAD_EXIT {
                        let note = crate::came_back_note(c.mach());
                        panic!("thread {id}'s entry {entry:08x} returned to {:08x}{note}", c.ret);
                    }
                }));
                match r {
                    Ok(()) => exit_current(&mut c, false),
                    Err(p) => match p.downcast::<Unwind>() {
                        Ok(u) => match *u {
                            Unwind::Restart(e, g, argc, argv) => {
                                let t = m.k.th(id).expect("the running thread");
                                t.start = Some((e, crate::RAM_SIZE - 0x10, g, argc, argv));
                                t.entry = e;
                                continue;
                            }
                            Unwind::Exit => {}
                        },
                        Err(p) => {
                            let e = crate::machine::panic_text(&p);
                            m.error = Some(format!("thread {id}: {e}"));
                            m.k.current = 0;
                            give(0);
                        }
                    },
                }
                break;
            }
        })
        .expect("a host thread for a kernel thread");
}

fn load_vu0(c: &mut Cpu) {
    let r = c.mach().vu0r;
    c.vf = r.vf;
    c.vacc = r.vacc;
    c.vi = r.vi;
}
fn save_vu0(c: &mut Cpu) {
    let (vf, vacc, vi) = (c.vf, c.vacc, c.vi);
    c.mach().vu0r = crate::machine::Vu0Regs { vf, vacc, vi };
}

/// Hands the CPU to `next` (0: the host) and waits for this thread's next turn.
fn switch_to(c: &mut Cpu, next: u32) {
    let me = c.thread;
    let mp = MPtr(c.mp());
    save_vu0(c);
    let m = c.mach();
    let my_token = m.k.th(me).map(|t| t.token).expect("the running thread");
    if let Some(t) = m.k.th(me)
        && t.status == THS_RUN
    {
        t.status = THS_READY;
    }
    m.k.current = next;
    if next == 0 {
        give(0);
    } else {
        start_turn(mp, next);
    }
    wait_turn(my_token);
    let m = c.mach();
    m.k.current = me;
    if let Some(t) = m.k.th(me) {
        t.status = THS_RUN;
    }
    let failed = m.error.is_some();
    load_vu0(c);
    if failed {
        std::panic::resume_unwind(Box::new(Unwind::Exit));
    }
}

/// Gives the turn to kernel thread `id`, starting its host thread the first time.
fn start_turn(mp: MPtr, id: u32) {
    let m = unsafe { &mut *mp.0 };
    let field = m.field;
    guard_note(|| format!("field {field} thread {id}"));
    let tok = m.k.next_token;
    let t = m.k.th(id).expect("a thread to run");
    t.status = THS_RUN;
    if t.token == 0 {
        t.token = tok;
        m.k.next_token += 1;
        spawn(mp, id, tok);
    }
    let token = m.k.th(id).expect("a thread to run").token;
    give(token);
}

/// After the running thread's state changed: the CPU goes to whoever should have it now.
fn reschedule(c: &mut Cpu) {
    if c.thread == 0 {
        return;
    }
    let next = c.mach().k.pick();
    if next != Some(c.thread) {
        switch_to(c, next.unwrap_or(0));
    }
}

/// The running thread waits (its status already set): the next thread runs, or the host.
fn block(c: &mut Cpu) {
    let next = c.mach().k.pick();
    switch_to(c, next.unwrap_or(0));
}

fn exit_current(c: &mut Cpu, delete: bool) {
    let me = c.thread;
    let m = c.mach();
    if let Some(t) = m.k.th(me) {
        t.status = THS_DORMANT;
        t.token = 0;
        t.prio = t.init_prio;
    }
    if delete {
        m.k.threads[(me - 1) as usize] = None;
    }
    let next = m.k.pick();
    m.k.current = next.unwrap_or(0);
    match next {
        Some(n) => start_turn(MPtr(c.mp()), n),
        None => give(0),
    }
}

/// InitSys's search of the BIOS kernel (native, `uyatr`'s NATIVE): there is no BIOS kernel; nothing to find.
pub fn no_bios_scan(_c: &mut Cpu) {}

/// The host's part of a field: the vertical blank's interrupts.
pub(crate) fn vblank(mp: *mut Machine) {
    let m = unsafe { &mut *mp };
    m.k.hsync += if m.field % 2 == 0 { 262 } else { 263 };
    {
        let mut c = crate::cpu(mp);
        crate::iop::pad_frames(&mut c);
        crate::iop::land_sound_writes(&mut c);
    }
    for cause in m.hw.timer_events() {
        raise(mp, None, Int::Intc(cause));
    }
    m.hw.vblank_start();
    if let Some((flag, csr)) = m.k.vsync.take() {
        let mut c = crate::cpu(mp);
        c.wr32(flag, 1);
        let v = m.hw.gs_csr();
        c.wr64(csr, v);
    }
    raise(mp, None, Int::Intc(2));
    m.hw.vblank_end();
    raise(mp, None, Int::Intc(3));
}

/// The host's part of a field: the game's threads until none can run.
pub(crate) fn run_threads(mp: *mut Machine) {
    let m = unsafe { &mut *mp };
    let Some(next) = m.k.pick() else { return };
    m.k.current = next;
    start_turn(MPtr(mp), next);
    crate::machine::wait_field_end(m.field, m.field_start + crate::machine::field_time(), || unsafe { (*mp).k.current });
}

/// An interrupt: its handlers run now, unless one is running or the thread `c` (None: the host) has interrupts off.
pub(crate) fn raise(mp: *mut Machine, c: Option<&mut Cpu>, i: Int) {
    let m = unsafe { &mut *mp };
    let enabled = match i {
        Int::Intc(n) => m.hw.intc_mask & (1 << n) != 0,
        Int::Dmac(n) => m.hw.dmac_mask() & (1 << n) != 0,
    };
    if !enabled {
        return;
    }
    let off = c.as_ref().is_some_and(|c| !c.ints_on());
    if m.k.in_handler > 0 || off {
        if !m.k.pending.contains(&i) {
            m.k.pending.push_back(i);
        }
        return;
    }
    match c {
        Some(c) => {
            save_vu0(c);
            run_handlers(mp, i);
            drain(mp);
            load_vu0(c);
            reschedule(c);
        }
        None => {
            run_handlers(mp, i);
            drain(mp);
        }
    }
}

fn drain(mp: *mut Machine) {
    loop {
        let m = unsafe { &mut *mp };
        let Some(i) = m.k.pending.pop_front() else { break };
        run_handlers(mp, i);
    }
}

fn run_handlers(mp: *mut Machine, i: Int) {
    let m = unsafe { &mut *mp };
    let (list, cause) = match i {
        Int::Intc(n) => (m.k.intc.get(n as usize).cloned().unwrap_or_default(), n),
        Int::Dmac(n) => (m.k.dmac.get(n as usize).cloned().unwrap_or_default(), n),
    };
    m.k.in_handler += 1;
    let field = m.field;
    guard_note(|| format!("field {field} {i:?} handlers {:x?}", list.iter().map(|h| h.func).collect::<Vec<_>>()));
    for h in list {
        let mut c = crate::cpu(mp);
        c.thread = 0;
        c.r[29] = INT_STACK as u64;
        c.r[28] = h.gp as i32 as i64 as u64;
        c.r[4] = cause as u64;
        c.r[5] = h.arg as i32 as i64 as u64;
        c.r[6] = 0;
        c.r[31] = HANDLER_RET as u64;
        load_vu0(&mut c);
        c.call(h.func, 0);
        if c.ret != HANDLER_RET {
            let note = crate::came_back_note(c.mach());
            panic!("the handler {:08x} of {i:?} returned to {:08x}{note}", h.func, c.ret);
        }
        save_vu0(&mut c);
    }
    let m = unsafe { &mut *mp };
    m.k.in_handler -= 1;
    if i == Int::Dmac(5) && m.k.in_handler == 0 {
        let mut c = crate::cpu(mp);
        crate::iop::pump(&mut c);
    }
}

/// DMAC channel `ch`'s handler (its function and gp) when it is all that raising the channel's interrupt from a thread now
/// runs: the channel's only handler, no interrupt waiting for `drain` after it, no handler running (`raise`). For `hw::burst`.
pub(crate) fn dmac_alone(m: &Machine, ch: u32) -> Option<(u32, u32)> {
    match m.k.dmac.get(ch as usize).map(Vec::as_slice) {
        Some(&[h]) if m.k.pending.is_empty() && m.k.in_handler == 0 => Some((h.func, h.gp)),
        _ => None,
    }
}

fn arg(c: &Cpu, n: usize) -> u32 {
    c.r[4 + n] as u32
}

/// The kernel's side of a `Cpu`: its SIF registers, its calls, whether interrupts reach it.
pub(crate) trait K {
    fn sif_reg(&mut self, r: u32) -> u32;
    fn kcall(&mut self, n: i32, pc: u32) -> u32;
    fn ints_on(&self) -> bool;
}

/// A loop that only polls (`uycpu::Rt::spin`): this thread can go on only when something outside it changes (an interrupt,
/// another thread). The CPU goes to a higher priority thread if one is ready, else to the host, which ends the field.
pub(crate) extern "C-unwind" fn spin(c: &mut Cpu, pc: u32) {
    let me = c.thread;
    c.mach().k.polled.insert(me, pc);
    if c.thread == 0 {
        panic!("a handler polls at {pc:08x}: nothing can change while it runs");
    }
    // The translator marks every loop that stores nothing; one whose registers moved since its last pass is counting or
    // searching, not waiting, and runs on as any loop (its back edge still gives the field up when the field's time is used:
    // `Cpu::back`). The pause menu's quit question lays its text out in such a loop (0x3e7e2c on Florana): ending the field at
    // each pass made a frame of five fields, and the game read the pad once in five. A loop moving more than `SPIN_PASSES`
    // times in a field (a wait with a count) waits as a poll does, so what it waits for gets the field's end.
    const SPIN_PASSES: u32 = 4096;
    let regs = c.r[1..].iter().chain([c.hi, c.lo].iter()).fold(0xcbf2_9ce4_8422_2325u64, |h, r| (h ^ r).wrapping_mul(0x100_0000_01b3));
    let regs = c.f.iter().fold(regs, |h, f| (h ^ *f as u64).wrapping_mul(0x100_0000_01b3));
    let field = c.mach().field;
    let (moved, passes) = match c.mach().k.spun.get(&me) {
        Some(&(p, h, _, _)) if p == pc && h == regs => (false, 0),
        Some(&(_, _, f, n)) if f == field => (true, n + 1),
        _ => (true, 1),
    };
    if moved && passes <= SPIN_PASSES {
        c.mach().k.spun.insert(me, (pc, regs, field, passes));
        c.back(pc);
        return;
    }
    c.mach().k.spun.insert(me, (pc, regs, field, 0));
    // Time has passed: the timers' events first (the loop may be waiting for one).
    let events = c.mach().hw.timer_events();
    if !events.is_empty() {
        let mp = c.mp();
        for cause in events {
            raise(mp, Some(&mut *c), Int::Intc(cause));
        }
        return;
    }
    let higher = {
        let me = c.thread;
        let k = &c.mach().k;
        let my = k.threads[(me - 1) as usize].as_ref().map(|t| t.prio).unwrap_or(u32::MAX);
        k.pick().filter(|&n| n != me && k.threads[(n - 1) as usize].as_ref().is_some_and(|t| t.prio < my))
    };
    switch_to(c, higher.unwrap_or(0));
    // A control-channel chunk warp runs on the main thread, after its idle
    // poll yields. The disc loader itself waits for further interrupts.
    if me == 1 && let Some((loader, chunk)) = c.mach().queued_chunk.take() {
        let mut call = crate::cpu(c.mp());
        call.thread = me;
        call.status = c.status;
        call.r = c.r;
        call.r[29] = (c.r[29] - 0x100) & !15;
        call.r[4] = u64::from(chunk);
        call.r[5] = 0;
        call.r[31] = u64::from(HANDLER_RET);
        call.call(loader, 0);
        if call.ret != HANDLER_RET {
            panic!("level chunk {chunk}: loader at {loader:08x} came back to {:08x}", call.ret);
        }
        save_vu0(c);
        c.mach().chunk_loaded();
    }
}

/// The field's time is used up (`machine::PREEMPT`) and this thread is still running: a kernel thread that interrupts reach
/// goes to the host, which ends the field; it goes on from here next field. An interrupt handler, or a thread with
/// interrupts off, runs on (the console's vertical blank would wait for it too).
pub(crate) fn preempt(c: &mut Cpu, pc: u32) {
    if c.thread == 0 || !c.ints_on() || c.mach().k.in_handler > 0 {
        return;
    }
    c.mach().preempted = Some((c.thread, pc, c.r[31] as u32));
    let guarded = guard_range().is_some_and(|(lo, hi)| (lo..hi).contains(&pc));
    if guarded {
        // The rest of the stream the LZ unpacker reads (its next chunk t3 to the stream's end, the WAD header at a0 giving its
        // size), to compare too.
        let (a0, t3) = (c.r[4] as u32 & 0x01ff_ffff, c.r[11] as u32 & 0x01ff_ffff);
        let m = c.mach();
        let head = m.ram.get(a0 as usize..a0 as usize + 7).map(|h| (h[0..3] == *b"WAD", u32::from_le_bytes([h[3], h[4], h[5], h[6]])));
        let stream = match head {
            Some((true, size)) if t3 >= a0 && t3 < a0 + size && ((a0 + size) as usize) <= m.ram.len() => Some((t3, m.ram[t3 as usize..(a0 + size) as usize].to_vec())),
            _ => None,
        };
        *GUARD.lock().expect("the scratchpad guard") = Some(Guard { field: m.field, spr: m.spr.to_vec(), stream, log: Vec::new() });
        GUARDED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    switch_to(c, 0);
    if guarded {
        let g = GUARD.lock().expect("the scratchpad guard").take().expect("the guard taken at the preemption");
        let (thread, ra) = (c.thread, c.r[31] as u32);
        let m = c.mach();
        if let Some((at, before)) = &g.stream
            && m.ram[*at as usize..*at as usize + before.len()] == before[..]
        {
            println!("UYA_SPR_GUARD: thread {thread} preempted at {pc:08x} in field {}, back in field {}: the rest of its stream in RAM ({at:08x}..{:08x}) unchanged", g.field, m.field, *at as usize + before.len());
        }
        if let Some((at, before)) = &g.stream
            && m.ram[*at as usize..*at as usize + before.len()] != before[..]
        {
            STREAM_CHANGED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let now = &m.ram[*at as usize..*at as usize + before.len()];
            let first = before.iter().zip(now).position(|(a, b)| a != b).expect("a difference");
            let last = before.len() - before.iter().rev().zip(now.iter().rev()).position(|(a, b)| a != b).expect("a difference");
            println!(
                "UYA_SPR_GUARD: thread {thread} preempted at {pc:08x} in field {}, back in field {}: the rest of its stream in RAM ({:08x}..{:08x}) changed at {:08x}..{:08x}; meanwhile: {}",
                g.field,
                m.field,
                at,
                *at as usize + before.len(),
                *at as usize + first,
                *at as usize + last,
                g.log.join("; ")
            );
        }
        if g.spr[..] != m.spr[..] {
            CHANGED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let (mut ranges, mut i) = (Vec::new(), 0);
            while i < g.spr.len() {
                if g.spr[i] == m.spr[i] {
                    i += 1;
                    continue;
                }
                let s = i;
                while i < g.spr.len() && (g.spr[i] != m.spr[i] || (i + 16 < g.spr.len() && g.spr[i..i + 16] != m.spr[i..i + 16])) {
                    i += 1;
                }
                ranges.push(format!("{s:#06x}..{i:#06x}"));
            }
            println!(
                "UYA_SPR_GUARD: thread {thread} preempted at {pc:08x} (ra {ra:08x}) in field {}, back in field {}: the scratchpad changed at {}; meanwhile: {}",
                g.field,
                m.field,
                ranges.join(" "),
                g.log.join("; ")
            );
        }
    }
}

/// UYA_SPR_GUARD=LO-HI (hex code addresses), read once: a thread preempted at a back edge in [LO, HI) has the scratchpad copied
/// there and compared when it goes on, and the rest of the stream it reads (level 35's LZ unpacker, 0x40BD20: its next chunk
/// to the end of the WAD at a0); a change is printed with what ran meanwhile, and a stream unchanged is printed too, so a run
/// shows each cut-off it watched (an instrument: R3-034 found the port's own writes in an unpacker's stream with it).
fn guard_range() -> Option<(u32, u32)> {
    static R: std::sync::OnceLock<Option<(u32, u32)>> = std::sync::OnceLock::new();
    *R.get_or_init(|| {
        let v = std::env::var("UYA_SPR_GUARD").ok()?;
        let (a, b) = v.split_once('-').unwrap_or_else(|| panic!("UYA_SPR_GUARD={v}: LO-HI, hex code addresses"));
        let hex = |x: &str| u32::from_str_radix(x.trim_start_matches("0x"), 16).unwrap_or_else(|_| panic!("UYA_SPR_GUARD={v}: {x} is not hex"));
        Some((hex(a), hex(b)))
    })
}

/// A preempted thread's scratchpad, while it waits (`preempt`), and what ran meanwhile.
struct Guard {
    field: u64,
    spr: Vec<u8>,
    /// The rest of the stream in RAM: its address and bytes.
    stream: Option<(u32, Vec<u8>)>,
    log: Vec<String>,
}

static GUARD: std::sync::Mutex<Option<Guard>> = std::sync::Mutex::new(None);
/// Preemptions the guard copied the scratchpad at, and of them those it found changed, for a stop's message (`guard_counts`).
static GUARDED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static CHANGED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static STREAM_CHANGED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// "the scratchpad guard: N preemptions in [LO, HI), M changed", or nothing when UYA_SPR_GUARD is not set.
pub(crate) fn guard_counts() -> String {
    match guard_range() {
        Some((lo, hi)) => format!(
            "; the scratchpad guard: {} preemptions in {lo:08x}..{hi:08x}, {} with the scratchpad changed, {} with the rest of the stream in RAM changed",
            GUARDED.load(std::sync::atomic::Ordering::Relaxed),
            CHANGED.load(std::sync::atomic::Ordering::Relaxed),
            STREAM_CHANGED.load(std::sync::atomic::Ordering::Relaxed)
        ),
        None => String::new(),
    }
}

/// What ran while a guarded thread waits (`UYA_SPR_GUARD`), for its report.
pub(crate) fn guard_note(f: impl FnOnce() -> String) {
    if guard_range().is_some()
        && let Some(g) = GUARD.lock().expect("the scratchpad guard").as_mut()
        && g.log.len() < 64
    {
        g.log.push(f());
    }
}

/// The interrupt and SIF state, for a stop's message.
pub fn state(c: &mut Cpu) -> String {
    let status = c.status;
    let m = c.mach();
    let rbuf = m.iop.ee_rbuf.map(|r| format!("{r:08x} (first byte {})", m.ram[(r & 0x1fff_ffff) as usize]));
    format!(
        "status {status:08x}, D_STAT {:08x}, INTC mask {:08x}, pending {:?}, in handler {}, SIF receive buffer {}, {} answers waiting for the EE",
        m.hw.d_stat,
        m.hw.intc_mask,
        m.k.pending,
        m.k.in_handler,
        rbuf.unwrap_or_else(|| "never set".into()),
        m.iop.to_ee.len()
    )
}

/// EI (`uycpu::Rt::ei`): an interrupt that waited for it runs now.
pub(crate) extern "C-unwind" fn ei(c: &mut Cpu) {
    c.status |= 0x1_0000;
    let pending = !c.mach().k.pending.is_empty() && c.mach().k.in_handler == 0;
    if pending && c.thread != 0 {
        let mp = c.mp();
        save_vu0(c);
        drain(mp);
        load_vu0(c);
        reschedule(c);
    }
}

/// MFC0 (`uycpu::Rt::mfc0`).
pub(crate) extern "C-unwind" fn mfc0(c: &mut Cpu, r: u32, sel: u32) -> u32 {
    match r {
        9 => c.mach().hw.cop0_count(),
        12 => c.status,
        14 => c.epc,
        30 => c.errorepc,
        15 => 0x2e20,
        // PCCR/PCR0/PCR1 (the performance counters: MFPC with sel), the debug registers.
        25 => c.mach().hw.cop0_count(),
        // TagLo after a CACHE index-load-tag (libkernel's SyncDCache 0x11f5f0 and InvalidDCache 0x11f720 walk the data
        // cache's lines and write back those whose tag is in their range): the port has no data cache (CACHE is
        // emitted as nothing), every line is empty, its tag 0.
        28 => 0,
        // Wired: the game's code writes it in the movie-skip path and may read it back; the port has no TLB, so 0.
        6 => 0,
        _ => panic!("MFC0 ${r} (sel {sel:#x}): a COP0 register the game's code is not expected to read"),
    }
}

/// MTC0 (`uycpu::Rt::mtc0`).
pub(crate) extern "C-unwind" fn mtc0(c: &mut Cpu, r: u32, sel: u32, v: u32) {
    match r {
        12 => c.status = v,
        14 => c.epc = v,
        30 => c.errorepc = v,
        // Count, Compare, the performance counters' setup and values.
        9 | 11 | 25 => {}
        // Wired (the game's code writes it 0 in the movie-skip path; the port has no TLB, so it is
        // meaningless here).
        6 => {}
        _ => panic!("MTC0 ${r} (sel {sel:#x}) = {v:#x}: a COP0 register the game's code is not expected to write"),
    }
}

/// SYSCALL (`uycpu::Rt::syscall`): the kernel call in $v1 with its arguments in $a0..$a3; the result in $v0.
pub(crate) extern "C-unwind" fn syscall(c: &mut Cpu, pc: u32) {
    let n = c.r[3] as u32 as i32;
    let v = c.kcall(n, pc);
    c.r[2] = v as i32 as i64 as u64;
    if c.thread != 0 {
        let events = c.mach().hw.timer_events();
        let mp = c.mp();
        for cause in events {
            raise(mp, Some(&mut *c), Int::Intc(cause));
        }
    }
    if c.thread != 0 {
        let m = c.mach();
        if !m.k.pending.is_empty() && m.k.in_handler == 0 && c.ints_on() {
            let mp = c.mp();
            save_vu0(c);
            drain(mp);
            load_vu0(c);
            reschedule(c);
        }
    }
}

impl K for Cpu {
    fn sif_reg(&mut self, r: u32) -> u32 {
        match r {
            2 => crate::iop::CMDBUF,
            4 => 0x7_0000,
            _ => self.mach().k.sif_regs.get(&r).copied().unwrap_or(0),
        }
    }

    /// Interrupts reach this thread: IE and EIE on, not at an exception level.
    fn ints_on(&self) -> bool {
        self.status & 0x1_0001 == 0x1_0001 && self.status & 6 == 0
    }

    fn kcall(&mut self, n: i32, pc: u32) -> u32 {
        let (a0, a1, a2, a3) = (arg(self, 0), arg(self, 1), arg(self, 2), arg(self, 3));
        match n.unsigned_abs() {
            // SetGsCrt(interlace, mode, ffmd).
            0x02 => {
                self.mach().k.gs_crt = (a0, a1, a2);
                0
            }
            0x04 => panic!("the game called Exit({a0}) at {pc:08x}"),
            // ExecPS2(entry, gp, argc, argv): the running thread starts over at entry.
            0x07 => {
                self.mach().flush_cache();
                std::panic::resume_unwind(Box::new(Unwind::Restart(a0, a1, a2, a3)));
            }
            // AddIntcHandler(2)(cause, handler, next, arg) / AddDmacHandler(2).
            0x10 | 0x12 => {
                let gp = self.r[28] as u32;
                let k = &mut self.mach().k;
                let id = k.next_handler;
                k.next_handler += 1;
                let list = if n.unsigned_abs() == 0x10 { &mut k.intc } else { &mut k.dmac };
                let l = list.get_mut(a0 as usize).unwrap_or_else(|| panic!("handler for cause {a0} at {pc:08x}"));
                let h = Handler { id, func: a1, arg: a3, gp };
                if a2 as i32 == -1 { l.insert(0, h) } else { l.push(h) }
                id
            }
            // RemoveIntcHandler / RemoveDmacHandler(cause, id).
            0x11 | 0x13 => {
                let k = &mut self.mach().k;
                let list = if n.unsigned_abs() == 0x11 { &mut k.intc } else { &mut k.dmac };
                if let Some(l) = list.get_mut(a0 as usize) {
                    l.retain(|h| h.id != a1);
                }
                0
            }
            // _EnableIntc / _DisableIntc / _EnableDmac / _DisableDmac (and their i variants, 0x1A..0x1D): 1 if it changed.
            0x14 | 0x1a => self.mach().hw.set_intc_mask(a0, true) as u32,
            0x15 | 0x1b => self.mach().hw.set_intc_mask(a0, false) as u32,
            0x16 | 0x1c => self.mach().hw.set_dmac_mask(a0, true) as u32,
            0x17 | 0x1d => self.mach().hw.set_dmac_mask(a0, false) as u32,
            // CreateThread(ee_thread_t *): status, func, stack, stack_size, gp, initial_priority, current_priority, attr, option.
            0x20 => {
                let p = a0;
                let (func, stack, size, gp, prio) = (self.rd32(p + 4), self.rd32(p + 8), self.rd32(p + 12), self.rd32(p + 16), self.rd32(p + 20));
                let (attr, option) = (self.rd32(p + 28), self.rd32(p + 32));
                let k = &mut self.mach().k;
                let t = Thread { status: THS_DORMANT, prio, init_prio: prio, entry: func, stack, stack_size: size, gp, attr, option, wait: Wait::None, wakeups: 0, suspended: false, order: 0, token: 0, start: None };
                k.threads.push(Some(t));
                k.threads.len() as u32
            }
            // DeleteThread(id).
            0x21 => {
                let k = &mut self.mach().k;
                match k.th(a0) {
                    Some(t) if t.status == THS_DORMANT => {
                        k.threads[(a0 - 1) as usize] = None;
                        0
                    }
                    _ => u32::MAX,
                }
            }
            // StartThread(id, arg).
            0x22 => {
                let k = &mut self.mach().k;
                let Some(t) = k.th(a0) else { return u32::MAX };
                if t.status != THS_DORMANT {
                    return u32::MAX;
                }
                t.start = Some((t.entry, t.stack.wrapping_add(t.stack_size).wrapping_sub(0x10), t.gp, a1, 0));
                t.token = 0;
                k.ready(a0);
                reschedule(self);
                0
            }
            0x23 | 0x24 => {
                exit_current(self, n.unsigned_abs() == 0x24);
                std::panic::resume_unwind(Box::new(Unwind::Exit));
            }
            // TerminateThread(id) (iTerminateThread 0x26): another thread made dormant.
            0x25 | 0x26 => {
                let me = self.thread;
                let k = &mut self.mach().k;
                if a0 == me || a0 == 0 {
                    panic!("TerminateThread of the running thread at {pc:08x}");
                }
                let Some(t) = k.th(a0) else { return u32::MAX };
                if let Wait::Sema(s) = t.wait
                    && let Some(Some(sm)) = k.semas.get_mut((s - 1) as usize)
                {
                    sm.waiters.retain(|&w| w != a0);
                }
                let t = k.th(a0).expect("checked");
                t.status = THS_DORMANT;
                t.wait = Wait::None;
                t.token = 0;
                t.prio = t.init_prio;
                0
            }
            // ChangeThreadPriority(id, prio) (and iChange, 0x2A).
            0x29 | 0x2a => {
                let id = if a0 == 0 { self.thread } else { a0 };
                let k = &mut self.mach().k;
                let Some(t) = k.th(id) else { return u32::MAX };
                let old = t.prio;
                t.prio = a1;
                if n.unsigned_abs() == 0x29 {
                    reschedule(self);
                }
                old
            }
            // RotateThreadReadyQueue(prio) (and _iRotate, 0x2C): the first thread of that priority goes to the back.
            0x2b | 0x2c => {
                let k = &mut self.mach().k;
                let cur = k.current;
                let first = k
                    .threads
                    .iter()
                    .enumerate()
                    .filter_map(|(i, t)| t.as_ref().map(|t| (i as u32 + 1, t)))
                    .filter(|(_, t)| t.prio == a0 && (t.status == THS_READY || t.status == THS_RUN))
                    .min_by_key(|(id, t)| (if *id == cur { 0 } else { 1 }, t.order))
                    .map(|(id, _)| id);
                if let Some(id) = first {
                    k.order += 1;
                    let o = k.order;
                    let t = k.th(id).expect("found");
                    t.order = o;
                    if id == cur {
                        // The running thread gives way to an equal one.
                        let next = k.threads.iter().enumerate().filter_map(|(i, t)| t.as_ref().map(|t| (i as u32 + 1, t))).filter(|(i, t)| *i != cur && t.prio <= a0 && t.status == THS_READY && !t.suspended).min_by_key(|(_, t)| (t.prio, t.order)).map(|(i, _)| i);
                        if let Some(nx) = next
                            && n.unsigned_abs() == 0x2b
                        {
                            switch_to(self, nx);
                        }
                    }
                }
                0
            }
            0x2f => self.thread,
            // ReferThreadStatus(id, ee_thread_status_t *).
            0x30 | 0x31 => {
                let id = if a0 == 0 { self.thread } else { a0 };
                let Some(t) = self.mach().k.th(id) else { return u32::MAX };
                let (wt, wid) = match t.wait {
                    Wait::None => (0, 0),
                    Wait::Sleep => (1, 0),
                    Wait::Sema(s) => (2, s),
                };
                let st = if t.suspended { t.status | THS_SUSPEND } else { t.status };
                let v = [st, t.entry, t.stack, t.stack_size, t.gp, t.init_prio, t.prio, t.attr, t.option, wt, wid, t.wakeups];
                if a1 != 0 {
                    for (k, w) in v.iter().enumerate() {
                        self.wr32(a1 + 4 * k as u32, *w);
                    }
                }
                st
            }
            // SleepThread.
            0x32 => {
                let me = self.thread;
                let k = &mut self.mach().k;
                let t = k.th(me).expect("the running thread");
                if t.wakeups > 0 {
                    t.wakeups -= 1;
                    return me;
                }
                t.status = THS_WAIT;
                t.wait = Wait::Sleep;
                block(self);
                me
            }
            // WakeupThread(id) (iWakeupThread 0x34).
            0x33 | 0x34 => {
                let k = &mut self.mach().k;
                let Some(t) = k.th(a0) else { return u32::MAX };
                if t.status == THS_WAIT && t.wait == Wait::Sleep {
                    k.ready(a0);
                    if n.unsigned_abs() == 0x33 {
                        reschedule(self);
                    }
                } else {
                    t.wakeups += 1;
                }
                0
            }
            // CancelWakeupThread (iCancel 0x36): the count before.
            0x35 | 0x36 => {
                let id = if a0 == 0 { self.thread } else { a0 };
                let Some(t) = self.mach().k.th(id) else { return u32::MAX };
                std::mem::take(&mut t.wakeups)
            }
            // SuspendThread / ResumeThread (and the i variants).
            0x37 | 0x38 | 0x39 | 0x3a => {
                let on = matches!(n.unsigned_abs(), 0x37 | 0x38);
                let Some(t) = self.mach().k.th(a0) else { return u32::MAX };
                t.suspended = on;
                if n.unsigned_abs() == 0x39 {
                    reschedule(self);
                }
                0
            }
            // SetupThread(gp, stack, stack_size, args, root): the main thread's stack (stack -1: at the end of RAM), its sp 0x2A0
            // below the stack's top, as the BIOS's (and the reference's, rac-rs `cold.rs`): the game's code stores into its outer
            // frames' room above that sp, and with the sp 0x10 below the end of RAM a 16-byte store there went past RAM (level 9,
            // a cutscene: "hardware 16-byte write at 02000000").
            0x3c => {
                let top = if a1 == u32::MAX { crate::RAM_SIZE } else { a1.wrapping_add(a2) };
                let me = self.thread;
                let k = &mut self.mach().k;
                k.stack_bottom = top.wrapping_sub(a2);
                if let Some(t) = k.th(me) {
                    t.gp = a0;
                    t.stack = top.wrapping_sub(a2);
                    t.stack_size = a2;
                }
                // The argument block (argc, argv[16], then the strings): one argument, the boot path.
                if a3 != 0 {
                    let path = b"cdrom0:\\SCUS_973.53;1\0";
                    self.wr32(a3, 1);
                    self.wr32(a3 + 4, a3 + 0x44);
                    for (i, &b) in path.iter().enumerate() {
                        self.wr8(a3 + 0x44 + i as u32, b);
                    }
                }
                top.wrapping_sub(0x2a0)
            }
            // SetupHeap(start, size): size -1 runs the heap to the stack's bottom.
            0x3d => {
                let k = &mut self.mach().k;
                k.heap_end = if a1 == u32::MAX { k.stack_bottom } else { a0.wrapping_add(a1) };
                a0
            }
            0x3e => self.mach().k.heap_end,
            // CreateSema(ee_sema_t *): count, max_count, init_count, wait_threads, attr, option.
            0x40 => {
                let p = a0;
                let (max, init, attr, option) = (self.rd32(p + 4) as i32, self.rd32(p + 8) as i32, self.rd32(p + 16), self.rd32(p + 20));
                let k = &mut self.mach().k;
                k.semas.push(Some(Sema { count: init, max, init, attr, option, waiters: VecDeque::new() }));
                k.semas.len() as u32
            }
            // DeleteSema(id): its waiters are released.
            0x41 => {
                let k = &mut self.mach().k;
                let Some(s) = k.semas.get_mut((a0 as usize).wrapping_sub(1)).and_then(|s| s.take()) else { return u32::MAX };
                for w in s.waiters {
                    k.ready(w);
                }
                reschedule(self);
                a0
            }
            // SignalSema(id) / iSignalSema.
            0x42 | 0x43 => {
                let k = &mut self.mach().k;
                let Some(s) = k.semas.get_mut((a0 as usize).wrapping_sub(1)).and_then(|s| s.as_mut()) else { return u32::MAX };
                if let Some(w) = s.waiters.pop_front() {
                    k.ready(w);
                    if n.unsigned_abs() == 0x42 {
                        reschedule(self);
                    }
                } else if s.count < s.max {
                    s.count += 1;
                } else {
                    return u32::MAX;
                }
                a0
            }
            // WaitSema(id).
            0x44 => {
                let me = self.thread;
                let k = &mut self.mach().k;
                let Some(s) = k.semas.get_mut((a0 as usize).wrapping_sub(1)).and_then(|s| s.as_mut()) else { return u32::MAX };
                if s.count > 0 {
                    s.count -= 1;
                    return a0;
                }
                if me == 0 {
                    panic!("WaitSema({a0}) from a handler at {pc:08x} on a zero count");
                }
                s.waiters.push_back(me);
                let t = k.th(me).expect("the running thread");
                t.status = THS_WAIT;
                t.wait = Wait::Sema(a0);
                block(self);
                a0
            }
            // PollSema(id) / iPollSema: KE_SEMA_ZERO (-419) when it would wait.
            0x45 | 0x46 => {
                let k = &mut self.mach().k;
                let Some(s) = k.semas.get_mut((a0 as usize).wrapping_sub(1)).and_then(|s| s.as_mut()) else { return u32::MAX };
                if s.count > 0 {
                    s.count -= 1;
                    a0
                } else {
                    (-419i32) as u32
                }
            }
            // ReferSemaStatus(id, ee_sema_t *).
            0x47 | 0x48 => {
                let Some(s) = self.mach().k.semas.get((a0 as usize).wrapping_sub(1)).and_then(|s| s.as_ref()) else { return u32::MAX };
                let v = [s.count as u32, s.max as u32, s.init as u32, s.waiters.len() as u32, s.attr, s.option];
                for (k, w) in v.iter().enumerate() {
                    self.wr32(a1 + 4 * k as u32, *w);
                }
                a0
            }
            // SetOsdConfigParam: nothing to keep.
            0x4a => 0,
            // GetOsdConfigParam(param *): English, 4:3, RGB.
            0x4b => {
                self.wr32(a0, 1 << 16);
                0
            }
            // The kernel patches libkernel's InitSys installs (kernel copies, SetSyscall): this kernel has its own code.
            0x5a | 0x5b | 0x74 => 0,
            // FlushCache(op): the code in RAM may have changed.
            0x64 | 0x68 => {
                self.mach().flush_cache();
                0
            }
            // sceSifStopDma (sceSifResetIop at 0x124168, before the IOP reset): the IOP-to-EE DMA stops, and what the IOP had
            // queued for the EE goes with it.
            0x6b => {
                let m = self.mach();
                m.iop.to_ee.clear();
                m.hw.ch[5][0] &= !0x100;
                0
            }
            0x70 => self.mach().k.gs_imr,
            0x71 => {
                let k = &mut self.mach().k;
                let old = k.gs_imr;
                k.gs_imr = a0;
                old
            }
            // SetVSyncFlag(flag *, csr *).
            0x73 => {
                self.mach().k.vsync = Some((a0, a1));
                0
            }
            // SifDmaStat(id): -1, the transfer is done (every SifSetDma completes at once).
            0x76 => u32::MAX,
            // SifSetDma(transfers, count): each {EE address, IOP address, size, mode} copied into IOP RAM (`iop`).
            0x77 => {
                for i in 0..a1 {
                    let p = a0 + 16 * i;
                    let (src, dst, size, mode) = (self.rd32(p), self.rd32(p + 4), self.rd32(p + 8), self.rd32(p + 12));
                    crate::iop::sif_dma(self, src, dst, size, mode);
                }
                let k = &mut self.mach().k;
                k.sif_dma_id = k.sif_dma_id.wrapping_add(1).max(1);
                let id = k.sif_dma_id;
                crate::iop::sif_dma_done(self);
                id
            }
            0x78 => 0,
            // SifSetReg(reg, value) / SifGetReg(reg): the SIF's registers (2 the IOP's command buffer, SUBADDR; 4 SMFLAG, the
            // IOP's state: SIFINIT, CMDINIT and BOOTEND, it is up; writing bits there clears them on the hardware, and a booted
            // IOP sets them again at once) and the software registers from 0x80000000 (0x80000002, RPC init, the EE sets).
            0x79 => {
                let old = self.sif_reg(a0);
                if a0 != 4 {
                    self.mach().k.sif_regs.insert(a0, a1);
                }
                old
            }
            0x7a => self.sif_reg(a0),
            // Deci2Call: no debugger.
            0x7c => (-1i32) as u32,
            0x7f => crate::RAM_SIZE,
            // FindAddress(start, end, value) over the kernel's memory: the kernel InitSys would patch is not there.
            0x83 => 0,
            _ => panic!("syscall {n:#x} at {pc:08x} (a0 {a0:#x}, a1 {a1:#x}, a2 {a2:#x}, a3 {a3:#x}): not answered by the port's kernel"),
        }
    }
}
