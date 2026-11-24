//! Animation (captures/native-port/plan.md unit 3), done natively in front of its translation (port/uyatr `JOBS`): the layer
//! update (level 1 0x436D30), the frame unpack (0x4397F8) and the pose build (0x43B7D0) it calls, and the six functions of the
//! animation code the game calls on their own: 0x43AAF8, 0x43CF90 and 0x43D9D8 (the poses into the scratchpad's joint rows,
//! with VU0's macro instructions and the scratchpad DMA), 0x4389D8, and the frame readers 0x436500 and 0x4365E0 (a layer's
//! frame number as a float). The translated wrapper at each entry asks `start` (`Rt::native` 40, 42, ... 56, one even number
//! an entry in the order of `L1`) and answers `after` (41, 43, ... 57) once the translation ran when `start` let it run; `c.ret`
//! brings the function's address with 1 in its low bit.
//!
//! The nine are the same code in every story level, six in both front ends too (0x4389D8, 0x43AAF8 and 0x43CF90), and 0x4365E0
//! is in every level but 2 and 9 (uyatr finds each by size and shape: one in each program and nothing else of its size and
//! shape). Only the layer update calls anything: the frame unpack and the pose build (directly here, as the unit's own), and
//! through the call table 0x437200 (twice), 0x43E558 and boot's FlushCache 0x11F0A0 (twice); 0x4389D8 leaves by two J's to
//! 0x4A7200 and 0x436410, which it tail-calls. The words relocation changes are read from the code in RAM (`F`), named by
//! level 1's address of the instruction holding them: those calls' and jumps' targets, the address 0x1F4BC0 (0x43B354 and
//! 0x43B358), the address 0x1FCDC0 (0x43CF90 and 0x43CF94) and 0x4389D8's gp offset (0x438CC0). Every other word is the same
//! in all 36 programs (each copy's words against level 1's, every program the translator found it in).
//!
//! Each function here is level 1's translation's statements in order (`translated/uya/l01_063` to `l01_065`: every
//! instruction, every path) on the thread's own registers. What goes away is the translated code's machinery: the calls
//! between the unit's functions are direct, the loops' back edges are not safe points and the spin loops do not yield (the
//! unit is one block of the thread's time, as the moby draw path and the collision sweeps are; BC0F's waits are not made,
//! `Cpu::cpcond0` being always true). So the unit leaves what the translation leaves, register for register and byte for
//! byte (the output contract, which the check compares whole): every GPR both halves, HI/LO, the FPU's registers and FCR31,
//! VU0's registers, ACC, Q and flags, RAM (the layers' records, the poses, the stack frames), the scratchpad (the joint rows
//! and the poses' staging), the scratchpad DMA channels and D_STAT as the code's kicks leave them, and what the functions it
//! calls out do (they run as themselves).
//!
//! With UYA_NATIVE_CHECK=anim:N each entry's first N calls from the game are checked: the translation, then the native unit on the state as it was before; every difference in that state afterwards
//! (`differences`: all of it) is reported with the entries' counts, and the game goes on from the translation's state. Both
//! runs go with the thread's interrupts held, as the moby draw path's check holds them (`uyart::mobydraw`), so that the field's
//! end and the timers land after them, once. A call of an entry inside a checked call's translation runs as its translation
//! (the comparison is the whole translation against the whole native unit), and inside its native run (the layer update's
//! calls out reach the other entries) natively and unchecked. Another unit's job leaves this one unchecked.

use crate::{Cpu, Ext};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

// ---- the code in RAM ------------------------------------------------------------------------------------------------------

/// A function of the unit as it lies in RAM: its address there and level 1's, by which this file names its instructions.
#[derive(Clone, Copy)]
struct F {
    at: u32,
    l1: u32,
}

impl F {
    /// The instruction at level 1's `pc`, in this program (code is in RAM).
    #[inline(always)]
    fn w(self, c: &Cpu, pc: u32) -> u32 {
        let a = self.pc(pc);
        assert!(a < uycpu::RAM_SIZE - 4, "the animation unit's code at {a:08x}: not in RAM");
        unsafe { (c.ram.add(a as usize) as *const u32).read_unaligned() }
    }
    /// LUI's value.
    #[inline(always)]
    fn hi(self, c: &Cpu, pc: u32) -> u32 {
        self.w(c, pc) << 16
    }
    /// The 16-bit immediate, sign-extended (an address's low part or a gp offset).
    #[inline(always)]
    fn lo(self, c: &Cpu, pc: u32) -> u32 {
        self.w(c, pc) as u16 as i16 as i32 as u32
    }
    /// The function the JAL (or J) at `pc` goes to.
    #[inline(always)]
    fn jal(self, c: &Cpu, pc: u32) -> u32 {
        (self.w(c, pc) & 0x03ff_ffff) << 2
    }
    /// This program's address of level 1's `pc` (a return address).
    #[inline(always)]
    fn pc(self, pc: u32) -> u32 {
        self.at.wrapping_add(pc.wrapping_sub(self.l1))
    }
}

/// A 32-bit result as the R5900 keeps it in 64 bits.
#[inline(always)]
fn sx(v: u32) -> u64 {
    v as i32 as i64 as u64
}

/// The entries, by level 1's address, in the order of their native numbers (40, 42, ... 56).
const L1: [u32; 9] = [0x43_6d30, 0x43_97f8, 0x43_b7d0, 0x43_aaf8, 0x43_cf90, 0x43_d9d8, 0x43_89d8, 0x43_6500, 0x43_65e0];
const ENTRY: [&str; 9] = [
    "the layer update",
    "the frame unpack",
    "the pose build",
    "0x43AAF8",
    "0x43CF90",
    "0x43D9D8",
    "0x4389D8",
    "the frame reader 0x436500",
    "the frame reader 0x4365E0",
];

/// The native runs of each entry (from the game, not the unit's own calls), and the calls out (`out`).
static RUNS: [AtomicU64; 9] = [const { AtomicU64::new(0) }; 9];
static OUT: AtomicU64 = AtomicU64::new(0);

/// A call of a game function outside the unit (`t`, by the JAL at `from`, this program's) through the call table, with the
/// thread's registers as the code has them; none of the three returns anywhere but to `ret`.
#[inline(always)]
fn out(c: &mut Cpu, t: u32, from: u32, ret: u32) {
    c.call(t, from);
    assert!(c.ret == ret, "the animation unit's call of {t:08x} at {from:08x} came back to {:08x}, not {ret:08x}", c.ret);
    OUT.fetch_add(1, Relaxed);
}

/// A call of the unit's own function that came back somewhere else (the translation's `came_back`): its functions return
/// only to their caller.
#[cold]
fn came_back(c: &Cpu, from: u32) -> ! {
    panic!("the animation unit's call at level 1's {from:08x} came back to {:08x}", c.ret)
}

/// Entry `e` natively at `at` (this program's address of it).
fn run(c: &mut Cpu, e: usize, at: u32) {
    let k = F { at, l1: L1[e] };
    match e {
        0 => n00436d30(c, k),
        1 => n004397f8(c, k),
        2 => n0043b7d0(c, k),
        3 => n0043aaf8(c, k),
        4 => n0043cf90(c, k),
        5 => n0043d9d8(c, k),
        6 => n004389d8(c, k),
        7 => n00436500(c, k),
        _ => n004365e0(c, k),
    }
    RUNS[e].fetch_add(1, Relaxed);
}

// ---- the check -------------------------------------------------------------------------------------------------------------

/// The machine's state a call of the unit can change.
struct State {
    ram: Vec<u8>,
    spr: Vec<u8>,
    vu0d: Vec<u8>,
    vu0m: Vec<u8>,
    r: [u64; 32],
    rh: [u64; 32],
    hilo: [u64; 4],
    sa: u32,
    f: [u32; 32],
    facc: u32,
    fcr31: u32,
    vf: [[u32; 4]; 32],
    vacc: [u32; 4],
    vi: [u32; 32],
    view_h: u32,
    ret: u32,
    ch: [[u32; 9]; 10],
    d_stat: u32,
    intc_stat: u32,
    parked: Option<usize>,
}

impl State {
    fn take(c: &mut Cpu) -> State {
        let (r, rh, hilo, sa, f, facc, fcr31) = (c.r, c.rh, [c.lo, c.hi, c.lo1, c.hi1], c.sa, c.f, c.facc, c.fcr31);
        let (vf, vacc, vi, view_h, ret) = (c.vf, c.vacc, c.vi, c.view_h, c.ret);
        let m = c.mach();
        State {
            ram: m.ram.to_vec(),
            spr: m.spr.clone(),
            vu0d: m.vu0d.clone(),
            vu0m: m.vu0m.clone(),
            r,
            rh,
            hilo,
            sa,
            f,
            facc,
            fcr31,
            vf,
            vacc,
            vi,
            view_h,
            ret,
            ch: m.hw.ch,
            d_stat: m.hw.d_stat,
            intc_stat: m.hw.intc_stat,
            parked: m.vu0_parked,
        }
    }

    fn put(&self, c: &mut Cpu) {
        (c.r, c.rh, c.sa, c.f, c.facc, c.fcr31) = (self.r, self.rh, self.sa, self.f, self.facc, self.fcr31);
        [c.lo, c.hi, c.lo1, c.hi1] = self.hilo;
        (c.vf, c.vacc, c.vi, c.view_h, c.ret) = (self.vf, self.vacc, self.vi, self.view_h, self.ret);
        let m = c.mach();
        m.ram.copy_from_slice(&self.ram);
        m.spr.copy_from_slice(&self.spr);
        m.vu0d.copy_from_slice(&self.vu0d);
        if m.vu0m[..] != self.vu0m[..] {
            m.vu0m.copy_from_slice(&self.vu0m);
            m.vu0_gen += 1;
        }
        m.hw.ch = self.ch;
        m.hw.d_stat = self.d_stat;
        m.hw.intc_stat = self.intc_stat;
        m.vu0_parked = self.parked;
    }
}

/// The byte ranges where `a` and `b` differ, as (start, end) pairs, at most `most`, and how many there are.
fn ranges(a: &[u8], b: &[u8], base: u32, most: usize) -> (Vec<String>, usize) {
    let (mut out, mut n, mut i) = (Vec::new(), 0, 0);
    while i < a.len() {
        if a[i] == b[i] {
            i += 1;
            continue;
        }
        let s = i;
        while i < a.len() && (a[i] != b[i] || (i + 16 < a.len() && a[i..i + 16] != b[i..i + 16])) {
            i += 1;
        }
        n += 1;
        if out.len() < most {
            let e = i.min(s + 16);
            out.push(format!("{:#x}..{:#x} [{:02x?} / {:02x?}]", base as usize + s, base as usize + i, &a[s..e], &b[s..e]));
        }
    }
    (out, n)
}

/// What differs between the translation's state `want` and the native unit's `got`: all of it (the unit is its translation's
/// statements on the same state, so every register, both halves, and every byte of the memories is what the translation
/// leaves).
fn differences(want: &State, got: &State) -> Vec<String> {
    let mut d = Vec::new();
    for (what, a, b, base) in [
        ("RAM", &want.ram, &got.ram, 0u32),
        ("scratchpad", &want.spr, &got.spr, 0x7000_0000u32),
        ("VU0 data", &want.vu0d, &got.vu0d, 0),
        ("VU0 code", &want.vu0m, &got.vu0m, 0),
    ] {
        let (r, n) = ranges(a, b, base, 12);
        if n > 0 {
            d.push(format!("{what}: {n} ranges {}", r.join(" ")));
        }
    }
    let regs: Vec<String> = (1..32)
        .filter(|&k| want.r[k] != got.r[k] || want.rh[k] != got.rh[k])
        .map(|k| format!("r{k} {:x}:{:x}/{:x}:{:x}", want.rh[k], want.r[k], got.rh[k], got.r[k]))
        .collect();
    if !regs.is_empty() || want.sa != got.sa || want.hilo != got.hilo || want.ret != got.ret {
        d.push(format!(
            "registers: {} SA {}/{} HI/LO {:x?}/{:x?} ret {:08x}/{:08x}",
            regs.join(" "),
            want.sa,
            got.sa,
            want.hilo,
            got.hilo,
            want.ret,
            got.ret
        ));
    }
    let fregs: Vec<String> = (0..32).filter(|&k| want.f[k] != got.f[k]).map(|k| format!("f{k} {:x}/{:x}", want.f[k], got.f[k])).collect();
    if !fregs.is_empty() || want.facc != got.facc || want.fcr31 != got.fcr31 {
        d.push(format!("FPU: {} ACC {:x}/{:x} FCR31 {:x}/{:x}", fregs.join(" "), want.facc, got.facc, want.fcr31, got.fcr31));
    }
    let vf: Vec<String> = (0..32).filter(|&k| want.vf[k] != got.vf[k]).map(|k| format!("vf{k} {:08x?}/{:08x?}", want.vf[k], got.vf[k])).collect();
    let vi: Vec<String> = (0..32).filter(|&k| want.vi[k] != got.vi[k]).map(|k| format!("vi{k} {:x}/{:x}", want.vi[k], got.vi[k])).collect();
    if !vf.is_empty() || !vi.is_empty() || want.vacc != got.vacc || want.parked != got.parked {
        d.push(format!("VU0: {} {} ACC {:08x?}/{:08x?} waiting {:?}/{:?}", vf.join(" "), vi.join(" "), want.vacc, got.vacc, want.parked, got.parked));
    }
    if want.view_h != got.view_h {
        d.push(format!("view_h {:#x}/{:#x}", want.view_h, got.view_h));
    }
    let ch: Vec<String> = (0..10).filter(|&k| want.ch[k] != got.ch[k]).map(|k| format!("channel {k} {:x?}/{:x?}", want.ch[k], got.ch[k])).collect();
    if !ch.is_empty() || want.d_stat != got.d_stat || want.intc_stat != got.intc_stat {
        d.push(format!("DMA: {} D_STAT {:#x}/{:#x} INTC {:#x}/{:#x}", ch.join(" "), want.d_stat, got.d_stat, want.intc_stat, got.intc_stat));
    }
    d
}

struct Check {
    /// The checked calls each entry is given.
    n: u64,
    /// Each entry's calls from the game, checked or not, and its checked calls and those that differed.
    seen: [u64; 9],
    done: [u64; 9],
    differ: [u64; 9],
    kinds: std::collections::BTreeSet<String>,
    /// The checked call: the state before it, sp, the entry and its address.
    before: Option<(State, u32)>,
    entry: usize,
    at: u32,
    /// The entries' calls inside the checked call's translation, not yet answered by `after`.
    inner: u64,
    /// The checked call's native run is going on.
    native: bool,
    /// Whether the thread had its interrupts on at the checked call: both runs go with them held (DI), and EI after the
    /// comparison takes what waited (`mobydraw`'s `Check::ints`).
    ints: bool,
}

static CHECK: std::sync::Mutex<Option<Check>> = std::sync::Mutex::new(None);

/// UYA_NATIVE_CHECK=anim:N, read once: N checked calls of each entry; another unit's job leaves this one unchecked.
fn checked() -> Option<u64> {
    static C: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
    *C.get_or_init(|| {
        let v = std::env::var("UYA_NATIVE_CHECK").ok()?;
        let (job, n) = v.split_once(':').unwrap_or_else(|| panic!("UYA_NATIVE_CHECK={v}: <job>:N"));
        match job {
            "anim" => Some(n.parse().unwrap_or_else(|_| panic!("UYA_NATIVE_CHECK={v}: N is a number of calls"))),
            _ => None,
        }
    })
}

/// `Rt::native` 40, 42, ... 56: the entry natively (`c.ret` left as its code leaves it: its return address), or (checking)
/// the state saved and `c.ret` 0 so the translation runs.
pub(crate) fn start(c: &mut Cpu, k: u32) {
    let e = ((k - 40) / 2) as usize;
    let at = c.ret & !3;
    if let Some(n) = checked() {
        let mut g = CHECK.lock().expect("the animation unit's check");
        let ck = g.get_or_insert(Check {
            n,
            seen: [0; 9],
            done: [0; 9],
            differ: [0; 9],
            kinds: Default::default(),
            before: None,
            entry: 0,
            at: 0,
            inner: 0,
            native: false,
            ints: false,
        });
        if ck.before.is_some() {
            ck.inner += 1;
            c.ret = 0;
            return;
        }
        if !ck.native {
            ck.seen[e] += 1;
            if ck.done[e] < ck.n {
                (ck.entry, ck.at) = (e, at);
                // Interrupts on as the kernel sees them (`kernel::K::ints_on`): EIE and IE, not in an exception.
                ck.ints = c.status & 0x1_0001 == 0x1_0001 && c.status & 6 == 0;
                if ck.ints {
                    c.di();
                }
                ck.before = Some((State::take(c), c.r[29] as u32));
                c.ret = 0;
                return;
            }
        }
    }
    run(c, e, at);
}

/// `Rt::native` 41, 43, ... 57: the translation ran; the entry runs natively from the state before it and the two are
/// compared.
pub(crate) fn after(c: &mut Cpu, _k: u32) {
    let (before, sp, e, at, ints) = {
        let mut g = CHECK.lock().expect("the animation unit's check");
        let Some(ck) = g.as_mut() else { return };
        if ck.inner > 0 {
            ck.inner -= 1;
            return;
        }
        let Some((before, sp)) = ck.before.take() else { return };
        ck.native = true;
        (before, sp, ck.entry, ck.at, ck.ints)
    };
    let ret = c.ret;
    let want = State::take(c);
    before.put(c);
    run(c, e, at);
    let got = State::take(c);
    let d = differences(&want, &got);
    want.put(c);
    c.ret = ret;
    let mut g = CHECK.lock().expect("the animation unit's check");
    let ck = g.as_mut().expect("checking");
    ck.native = false;
    ck.done[e] += 1;
    if !d.is_empty() {
        ck.differ[e] += 1;
        // The first calls that differ, and the first of each other kind of difference (the entry and what differs).
        let kind: String = std::iter::once(ENTRY[e]).chain(d.iter().map(|x| x.split(':').next().unwrap_or(x))).collect::<Vec<_>>().join("+");
        if ck.differ.iter().sum::<u64>() <= 6 || ck.kinds.insert(kind) {
            println!("UYA_NATIVE_CHECK: anim: {} call {} (at {at:08x}, sp {sp:08x}): {}", ENTRY[e], ck.done[e], d.join("; "));
        }
    }
    let total: u64 = ck.done.iter().sum();
    // Every 100th checked call, an entry's last, and each call of the entries the game calls least (all but 0x43CF90 and
    // 0x4389D8), so that a run's last line counts them all.
    if total % 100 == 0 || ck.done[e] == ck.n || (e != 4 && e != 6) {
        let each: Vec<String> = (0..9).map(|i| format!("{} {}/{} of {}", ENTRY[i], ck.done[i] - ck.differ[i], ck.done[i], ck.seen[i])).collect();
        println!(
            "UYA_NATIVE_CHECK: anim: {total} calls checked, {} differ (same/checked of the calls: {}; native runs {}, calls out {})",
            ck.differ.iter().sum::<u64>(),
            each.join(", "),
            RUNS.iter().map(|a| a.load(Relaxed)).sum::<u64>(),
            OUT.load(Relaxed)
        );
    }
    drop(g);
    // The interrupts held for the two runs (`Check::ints`): what came due runs now, as at the game's own EI.
    if ints {
        c.ei();
        c.ret = ret;
    }
}

// ---- the functions -----------------------------------------------------------------------------------------------------------
//
// Level 1's translation's statements in order, the delay slots where the translation puts them (a branch's condition is read
// before its delay slot runs), each block named by level 1's address where the code turns.

#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn n00436d30(c: &mut Cpu, k: F) {
let mut l: u32 = 0x00436d30;
loop { match l {
0x00436d30 => {
c.r[29] = (((c.r[29] as u32).wrapping_add(0xffffffc0)) as i32 as i64 as u64);
c.f[2] = 0u32;
c.wr64((c.r[29] as u32).wrapping_add(0x00000010), c.r[18]);
c.wr32((c.r[29] as u32).wrapping_add(0x00000030), c.f[22]);
c.r[18] = c.r[5].wrapping_add(0u64);
c.wr64((c.r[29] as u32), c.r[16]);
c.wr64((c.r[29] as u32).wrapping_add(0x00000008), c.r[17]);
c.wr64((c.r[29] as u32).wrapping_add(0x00000018), c.r[31]);
c.wr32((c.r[29] as u32).wrapping_add(0x00000028), c.f[21]);
c.wr32((c.r[29] as u32).wrapping_add(0x00000020), c.f[20]);
c.wr8((c.r[18] as u32).wrapping_add(0x00000005), 0u64 as u8);
c.f[1] = c.rd32((c.r[18] as u32).wrapping_add(0x00000028));
c.f[0] = c.rd32((c.r[18] as u32).wrapping_add(0x0000002c));
c.f[22] = uycpu::fpu::mul(c.f[1], c.f[0]);
c.set_fc(uycpu::fpu::eq(c.f[22], c.f[2]));
let t = c.fcr31 & 0x0080_0000 != 0;  if t { c.r[2] = c.rd8((c.r[18] as u32).wrapping_add(0x00000007)) as u64;
 l = 0x00436f30; continue; }
c.f[0] = c.rd32((c.r[18] as u32).wrapping_add(0x00000024));
c.r[1] = 0x000000003f7d0000;
c.r[1] = c.r[1] | 0x70a4;
c.f[1] = (c.r[1] as u32);
c.f[2] = uycpu::fpu::add(c.f[0], c.f[22]);
c.set_fc(uycpu::fpu::lt(c.f[1], c.f[2]));
let t = c.fcr31 & 0x0080_0000 == 0;  c.wr32((c.r[18] as u32).wrapping_add(0x00000024), c.f[2]);
 if t { l = 0x00436dcc; continue; }
c.r[1] = 0x000000003f810000;
c.r[1] = c.r[1] | 0x47ae;
c.f[0] = (c.r[1] as u32);
c.set_fc(uycpu::fpu::lt(c.f[2], c.f[0]));
let t = c.fcr31 & 0x0080_0000 == 0;  if t { c.f[0] = c.rd32((c.r[18] as u32).wrapping_add(0x00000024));
 l = 0x00436dd0; continue; }
c.r[1] = 0x000000003f800000;
c.f[0] = (c.r[1] as u32);
c.wr32((c.r[18] as u32).wrapping_add(0x00000024), c.f[0]);
l = 0x00436dcc; continue; }
0x00436dcc => {
c.f[0] = c.rd32((c.r[18] as u32).wrapping_add(0x00000024));
l = 0x00436dd0; continue; }
0x00436dd0 => {
c.r[1] = 0x000000003f800000;
c.f[1] = (c.r[1] as u32);
c.set_fc(uycpu::fpu::le(c.f[1], c.f[0]));
let t = c.fcr31 & 0x0080_0000 == 0;  if t { c.r[2] = c.rd8((c.r[18] as u32).wrapping_add(0x00000007)) as u64;
 l = 0x00436f30; continue; }
c.r[8] = 0x000000000fff0000;
c.r[9] = c.rd8((c.r[18] as u32).wrapping_add(0x00000007)) as u64;
c.f[3] = c.f[1];
c.r[10] = ((0u32.wrapping_add(0x000000ff)) as i32 as i64 as u64);
c.r[8] = c.r[8] | 0xffff;
l = 0x00436e00; continue; }
0x00436e00 => {
c.r[2] = c.rd8((c.r[18] as u32).wrapping_add(0x00000005)) as u64;
c.r[3] = c.rd8((c.r[18] as u32).wrapping_add(0x00000021)) as u64;
c.r[2] = c.r[2] | 0x1;
c.wr8((c.r[18] as u32).wrapping_add(0x00000005), c.r[2] as u8);
let t = c.r[9] == 0u64;  c.wr8((c.r[18] as u32).wrapping_add(0x00000020), c.r[3] as u8);
 if t { l = 0x00436e2c; continue; }
c.r[2] = c.rd8((c.r[18] as u32).wrapping_add(0x00000006)) as u64;
let t = c.r[2] != 0u64;  if t { c.r[3] = c.rd8((c.r[18] as u32).wrapping_add(0x00000021)) as u64;
 l = 0x00436e30; continue; }
c.r[2] = c.rd32((c.r[18] as u32).wrapping_add(0x0000003c)) as i32 as i64 as u64;
c.wr32((c.r[18] as u32).wrapping_add(0x00000038), c.r[2] as u32);
l = 0x00436e2c; continue; }
0x00436e2c => {
c.r[3] = c.rd8((c.r[18] as u32).wrapping_add(0x00000021)) as u64;
l = 0x00436e30; continue; }
0x00436e30 => {
c.r[7] = c.rd8((c.r[18] as u32).wrapping_add(0x00000023)) as u64;
c.r[6] = c.rd8((c.r[18] as u32).wrapping_add(0x00000007)) as u64;
c.r[3] = (((c.r[3] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.r[2] = c.r[7] & 0xff;
c.r[4] = c.rd32((c.r[18] as u32).wrapping_add(0x00000014)) as i32 as i64 as u64;
c.wr8((c.r[18] as u32).wrapping_add(0x00000006), c.r[6] as u8);
c.r[2] = (((c.r[2] as u32) << 2) as i32 as i64 as u64);
c.wr8((c.r[18] as u32).wrapping_add(0x00000022), c.r[7] as u8);
c.r[4] = (((c.r[4] as u32).wrapping_add((c.r[2] as u32))) as i32 as i64 as u64);
c.wr8((c.r[18] as u32).wrapping_add(0x00000021), c.r[3] as u8);
c.r[5] = c.r[3] & 0xff;
c.r[2] = c.rd32((c.r[4] as u32).wrapping_add(0x00000048)) as i32 as i64 as u64;
c.r[3] = c.rd8((c.r[2] as u32).wrapping_add(0x00000010)) as u64;
c.r[5] = (c.r[5] < c.r[3]) as u64;
let t = c.r[5] != 0u64;  if t { c.f[0] = c.rd32((c.r[18] as u32).wrapping_add(0x00000024));
 l = 0x00436e84; continue; }
c.r[2] = c.rd8((c.r[18] as u32).wrapping_add(0x00000005)) as u64;
c.wr8((c.r[18] as u32).wrapping_add(0x00000021), 0u64 as u8);
c.r[2] = c.r[2] | 0x2;
c.wr8((c.r[18] as u32).wrapping_add(0x00000005), c.r[2] as u8);
c.f[0] = c.rd32((c.r[18] as u32).wrapping_add(0x00000024));
l = 0x00436e84; continue; }
0x00436e84 => {
c.r[1] = 0x000000003f800000;
c.f[2] = (c.r[1] as u32);
c.f[1] = c.rd32((c.r[18] as u32).wrapping_add(0x0000002c));
c.f[0] = uycpu::fpu::sub(c.f[0], c.f[2]);
c.r[2] = c.rd8((c.r[18] as u32).wrapping_add(0x00000022)) as u64;
c.r[3] = c.rd32((c.r[18] as u32).wrapping_add(0x00000014)) as i32 as i64 as u64;
c.r[2] = (((c.r[2] as u32) << 2) as i32 as i64 as u64);
c.f[0] = uycpu::fpu::div(c, c.f[0], c.f[1]);
c.r[3] = (((c.r[3] as u32).wrapping_add((c.r[2] as u32))) as i32 as i64 as u64);
c.wr32((c.r[18] as u32).wrapping_add(0x00000024), c.f[0]);
c.r[3] = c.rd32((c.r[3] as u32).wrapping_add(0x00000048)) as i32 as i64 as u64;
c.r[2] = c.rd8((c.r[3] as u32).wrapping_add(0x00000013)) as u64;
let t = c.r[2] != c.r[10];  c.r[2] = c.rd8((c.r[18] as u32).wrapping_add(0x00000020)) as u64;
 if t { l = 0x00436ed8; continue; }
c.r[2] = (((c.r[2] as u32) << 2) as i32 as i64 as u64);
c.r[2] = (((c.r[3] as u32).wrapping_add((c.r[2] as u32))) as i32 as i64 as u64);
c.r[3] = c.rd32((c.r[2] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
let t = 0u64 == 0u64;  c.f[0] = c.rd32((c.r[3] as u32));
 if t { l = 0x00436f0c; continue; }
l = 0x00436ed8; continue; }
0x00436ed8 => {
c.r[2] = (((c.r[2] as u32) << 2) as i32 as i64 as u64);
c.r[2] = (((c.r[3] as u32).wrapping_add((c.r[2] as u32))) as i32 as i64 as u64);
c.r[3] = c.rd32((c.r[2] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
c.r[3] = c.r[3] & c.r[8];
c.r[2] = c.rd16((c.r[3] as u32)) as i16 as i64 as u64;
let t = c.r[2] != 0u64;   if t { l = 0x00436efc; continue; }
let t = 0u64 == 0u64;  c.wr32((c.r[18] as u32).wrapping_add(0x0000002c), 0u64 as u32);
 if t { l = 0x00436f10; continue; }
l = 0x00436efc; continue; }
0x00436efc => {
c.f[0] = (c.r[2] as u32);
c.f[0] = uycpu::fpu::cvt_s(c.f[0]);
c.f[0] = uycpu::fpu::div(c, c.f[2], c.f[0]);
l = 0x00436f0c; continue; }
0x00436f0c => {
c.wr32((c.r[18] as u32).wrapping_add(0x0000002c), c.f[0]);
l = 0x00436f10; continue; }
0x00436f10 => {
c.f[1] = c.rd32((c.r[18] as u32).wrapping_add(0x00000024));
c.f[0] = c.rd32((c.r[18] as u32).wrapping_add(0x0000002c));
c.f[1] = uycpu::fpu::mul(c.f[1], c.f[0]);
c.set_fc(uycpu::fpu::le(c.f[3], c.f[1]));
let t = c.fcr31 & 0x0080_0000 != 0;  c.wr32((c.r[18] as u32).wrapping_add(0x00000024), c.f[1]);
 if t { l = 0x00436e00; continue; }
c.r[2] = c.rd8((c.r[18] as u32).wrapping_add(0x00000007)) as u64;
l = 0x00436f30; continue; }
0x00436f30 => {
let t = c.r[2] == 0u64;  c.r[4] = c.r[18].wrapping_add(0u64);
 if t { l = 0x00436f50; continue; }
c.r[5] = c.rd8((c.r[18] as u32).wrapping_add(0x00000023)) as u64;
c.r[6] = c.rd8((c.r[18] as u32).wrapping_add(0x00000021)) as u64;
c.r[31] = k.pc(0x00436f48) as u64; c.r[7] = c.rd32((c.r[18] as u32).wrapping_add(0x0000003c)) as i32 as i64 as u64;
 out(c, k.jal(c, 0x00436f40), k.pc(0x00436f40), k.pc(0x00436f48));
let t = 0u64 == 0u64;  c.r[2] = c.rd8((c.r[18] as u32).wrapping_add(0x00000022)) as u64;
 if t { l = 0x00436fcc; continue; }
l = 0x00436f50; continue; }
0x00436f50 => {
c.r[2] = c.rd8((c.r[18] as u32).wrapping_add(0x00000023)) as u64;
c.r[4] = ((0u32.wrapping_add(0x000000ff)) as i32 as i64 as u64);
c.r[6] = c.rd32((c.r[18] as u32).wrapping_add(0x00000014)) as i32 as i64 as u64;
c.r[2] = (((c.r[2] as u32) << 2) as i32 as i64 as u64);
c.r[2] = (((c.r[6] as u32).wrapping_add((c.r[2] as u32))) as i32 as i64 as u64);
c.r[5] = c.rd32((c.r[2] as u32).wrapping_add(0x00000048)) as i32 as i64 as u64;
c.r[3] = c.rd8((c.r[5] as u32).wrapping_add(0x00000013)) as u64;
let t = c.r[3] != c.r[4];  if t { c.r[4] = c.rd8((c.r[5] as u32).wrapping_add(0x00000010)) as u64;
 l = 0x00436f8c; continue; }
c.r[2] = c.rd8((c.r[18] as u32).wrapping_add(0x00000021)) as u64;
c.r[2] = (((c.r[2] as u32) << 2) as i32 as i64 as u64);
c.r[2] = (((c.r[5] as u32).wrapping_add((c.r[2] as u32))) as i32 as i64 as u64);
c.r[3] = c.rd32((c.r[2] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
let t = 0u64 == 0u64;  c.wr32((c.r[18] as u32).wrapping_add(0x0000003c), c.r[3] as u32);
 if t { l = 0x00436fc8; continue; }
l = 0x00436f8c; continue; }
0x00436f8c => {
c.r[2] = c.rd8((c.r[18] as u32).wrapping_add(0x00000021)) as u64;
c.r[4] = (((c.r[4] as u32) << 2) as i32 as i64 as u64);
c.r[3] = c.rd8((c.r[5] as u32).wrapping_add(0x00000013)) as u64;
c.r[2] = (((c.r[2] as u32) << 2) as i32 as i64 as u64);
c.r[4] = (((c.r[4] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64);
c.r[4] = (((c.r[5] as u32).wrapping_add((c.r[4] as u32))) as i32 as i64 as u64);
c.r[2] = (((c.r[5] as u32).wrapping_add((c.r[2] as u32))) as i32 as i64 as u64);
c.r[7] = c.rd8((c.r[6] as u32).wrapping_add(0x00000008)) as u64;
c.r[3] = (((c.r[3] as u32) << 2) as i32 as i64 as u64);
c.r[5] = c.rd32((c.r[2] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
c.r[4] = (((c.r[4] as u32).wrapping_add((c.r[3] as u32))) as i32 as i64 as u64);
c.r[31] = k.pc(0x00436fc0) as u64; c.r[6] = c.rd32((c.r[18] as u32).wrapping_add(0x0000003c)) as i32 as i64 as u64;
 n004397f8(c, F { at: k.jal(c, 0x00436fb8), l1: 0x004397f8 }); if c.ret != k.pc(0x00436fc0) { came_back(c, 0x00436fb8); }
c.r[31] = k.pc(0x00436fc8) as u64; c.r[4] = 0u64.wrapping_add(0u64);
 out(c, k.jal(c, 0x00436fc0), k.pc(0x00436fc0), k.pc(0x00436fc8));
l = 0x00436fc8; continue; }
0x00436fc8 => {
c.r[2] = c.rd8((c.r[18] as u32).wrapping_add(0x00000022)) as u64;
l = 0x00436fcc; continue; }
0x00436fcc => {
c.r[4] = ((0u32.wrapping_add(0x000000ff)) as i32 as i64 as u64);
let t = c.r[2] != c.r[4];  if t { c.r[2] = c.rd8((c.r[18] as u32).wrapping_add(0x00000006)) as u64;
 l = 0x00437040; continue; }
c.f[21] = c.rd32((c.r[18] as u32).wrapping_add(0x00000024));
c.r[4] = c.r[18].wrapping_add(0u64);
c.r[1] = 0x000000003f800000;
c.f[20] = (c.r[1] as u32);
c.f[0] = uycpu::fpu::sub(c.f[21], c.f[22]);
c.r[16] = c.rd32((c.r[18] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64;
c.r[2] = c.rd32((c.r[18] as u32).wrapping_add(0x0000003c)) as i32 as i64 as u64;
c.r[17] = c.rd32((c.r[18] as u32).wrapping_add(0x00000038)) as i32 as i64 as u64;
c.f[20] = uycpu::fpu::sub(c.f[20], c.f[0]);
c.wr32((c.r[18] as u32).wrapping_add(0x00000024), 0u64 as u32);
c.wr32((c.r[18] as u32).wrapping_add(0x00000010), 0u64 as u32);
c.f[20] = uycpu::fpu::div(c, c.f[22], c.f[20]);
c.r[31] = k.pc(0x00437018) as u64; c.wr32((c.r[18] as u32).wrapping_add(0x00000038), c.r[2] as u32);
 n0043b7d0(c, F { at: k.jal(c, 0x00437010), l1: 0x0043b7d0 }); if c.ret != k.pc(0x00437018) { came_back(c, 0x00437010); }
c.wr32((c.r[18] as u32).wrapping_add(0x00000024), c.f[21]);
c.r[4] = c.r[16].wrapping_add(0u64);
c.wr32((c.r[18] as u32).wrapping_add(0x00000038), c.r[17] as u32);
c.r[5] = 0x0000000070000000;
c.wr32((c.r[18] as u32).wrapping_add(0x00000010), c.r[16] as u32);
c.r[6] = c.rd16((c.r[18] as u32).wrapping_add(0x00000002)) as i16 as i64 as u64;
c.r[31] = k.pc(0x00437038) as u64; c.f[12] = c.f[20];
 out(c, k.jal(c, 0x00437030), k.pc(0x00437030), k.pc(0x00437038));
let t = 0u64 == 0u64;  c.r[2] = ((0u32.wrapping_add(0x00000002)) as i32 as i64 as u64);
 if t { l = 0x004370e0; continue; }
l = 0x00437040; continue; }
0x00437040 => {
let t = c.r[2] == 0u64;  if t { c.r[2] = c.rd8((c.r[18] as u32).wrapping_add(0x00000022)) as u64;
 l = 0x00437064; continue; }
c.r[5] = c.rd8((c.r[18] as u32).wrapping_add(0x00000022)) as u64;
c.r[4] = c.r[18].wrapping_add(0u64);
c.r[6] = c.rd8((c.r[18] as u32).wrapping_add(0x00000020)) as u64;
c.r[31] = k.pc(0x0043705c) as u64; c.r[7] = c.rd32((c.r[18] as u32).wrapping_add(0x00000038)) as i32 as i64 as u64;
 out(c, k.jal(c, 0x00437054), k.pc(0x00437054), k.pc(0x0043705c));
let t = 0u64 == 0u64;   if t { l = 0x004370d4; continue; }
l = 0x00437064; continue; }
0x00437064 => {
c.r[6] = c.rd32((c.r[18] as u32).wrapping_add(0x00000014)) as i32 as i64 as u64;
c.r[2] = (((c.r[2] as u32) << 2) as i32 as i64 as u64);
c.r[2] = (((c.r[6] as u32).wrapping_add((c.r[2] as u32))) as i32 as i64 as u64);
c.r[5] = c.rd32((c.r[2] as u32).wrapping_add(0x00000048)) as i32 as i64 as u64;
c.r[3] = c.rd8((c.r[5] as u32).wrapping_add(0x00000013)) as u64;
let t = c.r[3] != c.r[4];  if t { c.r[4] = c.rd8((c.r[5] as u32).wrapping_add(0x00000010)) as u64;
 l = 0x00437098; continue; }
c.r[2] = c.rd8((c.r[18] as u32).wrapping_add(0x00000020)) as u64;
c.r[2] = (((c.r[2] as u32) << 2) as i32 as i64 as u64);
c.r[2] = (((c.r[5] as u32).wrapping_add((c.r[2] as u32))) as i32 as i64 as u64);
c.r[3] = c.rd32((c.r[2] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
let t = 0u64 == 0u64;  c.wr32((c.r[18] as u32).wrapping_add(0x00000038), c.r[3] as u32);
 if t { l = 0x004370d4; continue; }
l = 0x00437098; continue; }
0x00437098 => {
c.r[2] = c.rd8((c.r[18] as u32).wrapping_add(0x00000020)) as u64;
c.r[4] = (((c.r[4] as u32) << 2) as i32 as i64 as u64);
c.r[3] = c.rd8((c.r[5] as u32).wrapping_add(0x00000013)) as u64;
c.r[2] = (((c.r[2] as u32) << 2) as i32 as i64 as u64);
c.r[4] = (((c.r[4] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64);
c.r[4] = (((c.r[5] as u32).wrapping_add((c.r[4] as u32))) as i32 as i64 as u64);
c.r[2] = (((c.r[5] as u32).wrapping_add((c.r[2] as u32))) as i32 as i64 as u64);
c.r[7] = c.rd8((c.r[6] as u32).wrapping_add(0x00000008)) as u64;
c.r[3] = (((c.r[3] as u32) << 2) as i32 as i64 as u64);
c.r[5] = c.rd32((c.r[2] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
c.r[4] = (((c.r[4] as u32).wrapping_add((c.r[3] as u32))) as i32 as i64 as u64);
c.r[31] = k.pc(0x004370cc) as u64; c.r[6] = c.rd32((c.r[18] as u32).wrapping_add(0x00000038)) as i32 as i64 as u64;
 n004397f8(c, F { at: k.jal(c, 0x004370c4), l1: 0x004397f8 }); if c.ret != k.pc(0x004370cc) { came_back(c, 0x004370c4); }
c.r[31] = k.pc(0x004370d4) as u64; c.r[4] = 0u64.wrapping_add(0u64);
 out(c, k.jal(c, 0x004370cc), k.pc(0x004370cc), k.pc(0x004370d4));
l = 0x004370d4; continue; }
0x004370d4 => {
c.r[31] = k.pc(0x004370dc) as u64; c.r[4] = c.r[18].wrapping_add(0u64);
 n0043b7d0(c, F { at: k.jal(c, 0x004370d4), l1: 0x0043b7d0 }); if c.ret != k.pc(0x004370dc) { came_back(c, 0x004370d4); }
c.r[2] = ((0u32.wrapping_add(0x00000002)) as i32 as i64 as u64);
l = 0x004370e0; continue; }
0x004370e0 => {
c.r[16] = c.rd64((c.r[29] as u32));
c.wr8((c.r[18] as u32).wrapping_add(0x00000004), c.r[2] as u8);
c.r[17] = c.rd64((c.r[29] as u32).wrapping_add(0x00000008));
c.r[18] = c.rd64((c.r[29] as u32).wrapping_add(0x00000010));
c.r[31] = c.rd64((c.r[29] as u32).wrapping_add(0x00000018));
c.f[22] = c.rd32((c.r[29] as u32).wrapping_add(0x00000030));
c.f[21] = c.rd32((c.r[29] as u32).wrapping_add(0x00000028));
c.f[20] = c.rd32((c.r[29] as u32).wrapping_add(0x00000020));
let t = (c.r[31] as u32); c.r[29] = (((c.r[29] as u32).wrapping_add(0x00000040)) as i32 as i64 as u64);
 c.ret = t; return;
}
_ => panic!("the animation unit: no label {l:08x} in 0x00436D30"), } }
}

#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn n004397f8(c: &mut Cpu, k: F) {
let mut l: u32 = 0x004397f8;
loop { match l {
0x004397f8 => {
c.r[24] = c.r[4].wrapping_add(0u64);
c.r[25] = (((c.r[5] as u32) << 4) as i32 as i64 as u64);
c.r[15] = c.r[6].wrapping_add(0u64);
c.r[14] = c.r[7].wrapping_add(0u64);
c.r[25] = (((c.r[25] as u32) >> 4) as i32 as i64 as u64);
c.r[1] = c.rd16((c.r[25] as u32)) as u64;
c.r[7] = (((c.r[14] as u32) << 1) as i32 as i64 as u64);
c.r[5] = (((c.r[14] as u32) << 2) as i32 as i64 as u64);
c.r[7] = (((c.r[24] as u32).wrapping_add((c.r[7] as u32))) as i32 as i64 as u64);
c.r[7] = (((c.r[7] as u32).wrapping_add((c.r[5] as u32))) as i32 as i64 as u64);
c.f[1] = (c.r[1] as u32);
c.r[5] = c.rd8((c.r[7] as u32).wrapping_add(0x00000004)) as u64;
c.r[2] = 0x000000003f800000;
c.f[1] = uycpu::fpu::cvt_s(c.f[1]);
c.r[4] = (((c.r[14] as u32) << 3) as i32 as i64 as u64);
c.f[2] = (c.r[2] as u32);
c.r[3] = (((c.r[14] as u32).wrapping_add((c.r[5] as u32))) as i32 as i64 as u64);
c.r[7] = c.rd8((c.r[7] as u32).wrapping_add(0x00000005)) as u64;
c.r[6] = (((c.r[5] as u32) << 3) as i32 as i64 as u64);
c.r[2] = c.rd16((c.r[25] as u32).wrapping_add(0x00000002)) as u64;
c.r[6] = (((c.r[6] as u32).wrapping_add((c.r[4] as u32))) as i32 as i64 as u64);
c.f[1] = uycpu::fpu::div(c, c.f[2], c.f[1]);
c.r[3] = (((c.r[3] as u32).wrapping_add((c.r[7] as u32))) as i32 as i64 as u64);
c.wr16((c.r[15] as u32).wrapping_add(0x00000004), c.r[2] as u16);
c.r[3] = (((c.r[3] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.wr16((c.r[15] as u32).wrapping_add(0x00000008), c.r[4] as u16);
c.r[3] = (((c.r[3] as u32) >> 1) as i32 as i64 as u64);
c.wr16((c.r[15] as u32).wrapping_add(0x00000006), c.r[3] as u16);
c.wr16((c.r[15] as u32).wrapping_add(0x0000000a), c.r[5] as u16);
c.wr16((c.r[15] as u32).wrapping_add(0x0000000c), c.r[6] as u16);
c.wr16((c.r[15] as u32).wrapping_add(0x0000000e), c.r[7] as u16);
let t = c.r[1] == 0u64;  c.wr32((c.r[15] as u32), 0u64 as u32);
 if t { l = 0x00439888; continue; }
c.wr32((c.r[15] as u32), c.f[1]);
l = 0x00439888; continue; }
0x00439888 => {
c.r[1] = (((c.r[24] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
c.r[2] = (((c.r[15] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
c.r[3] = c.r[14].wrapping_add(0u64);
c.r[13] = c.rd32((c.r[24] as u32)) as i32 as i64 as u64;
c.r[10] = ((0u32.wrapping_add(0x00000016)) as i32 as i64 as u64);
c.r[11] = (((c.r[25] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
c.r[12] = (((c.r[13] as u32) >> 10) as i32 as i64 as u64);
c.r[13] = c.r[13] & 0x3ff;
c.r[12] = c.r[12] & 0x7ff;
c.r[13] = (((c.r[13] as u32).wrapping_add((c.r[11] as u32))) as i32 as i64 as u64);
c.r[12] = (((c.r[12] as u32).wrapping_add((c.r[11] as u32))) as i32 as i64 as u64);
l = 0x004398b8; continue; }
0x004398b8 => {
c.r[4] = c.rd16((c.r[1] as u32)) as i16 as i64 as u64;
c.r[5] = c.rd16((c.r[1] as u32).wrapping_add(0x00000002)) as i16 as i64 as u64;
c.r[9] = ((0u32.wrapping_add(0x00000001)) as i32 as i64 as u64);
c.r[6] = c.rd16((c.r[1] as u32).wrapping_add(0x00000004)) as i16 as i64 as u64;
c.r[7] = c.r[4] & 0x3;
let t = c.r[7] == c.r[9];  c.r[7] = (((c.r[7] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x00439938; continue; }
let t = c.r[7] == c.r[9];  c.r[7] = (((c.r[7] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x00439950; continue; }
let t = c.r[7] == c.r[9];  c.r[7] = c.r[5] & 0x3;
 if t { l = 0x00439980; continue; }
l = 0x004398e8; continue; }
0x004398e8 => {
let t = c.r[7] == c.r[9];  c.r[7] = (((c.r[7] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x00439990; continue; }
let t = c.r[7] == c.r[9];  c.r[7] = (((c.r[7] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x004399a8; continue; }
let t = c.r[7] == c.r[9];  c.r[7] = c.r[6] & 0x3;
 if t { l = 0x004399d8; continue; }
l = 0x00439900; continue; }
0x00439900 => {
let t = c.r[7] == c.r[9];  c.r[7] = (((c.r[7] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x004399e8; continue; }
let t = c.r[7] == c.r[9];  c.r[7] = (((c.r[7] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x00439a00; continue; }
let t = c.r[7] == c.r[9];  c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000006)) as i32 as i64 as u64);
 if t { l = 0x00439a30; continue; }
l = 0x00439918; continue; }
0x00439918 => {
c.wr16((c.r[2] as u32), c.r[4] as u16);
c.r[3] = (((c.r[3] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
c.wr16((c.r[2] as u32).wrapping_add(0x00000002), c.r[5] as u16);
c.r[2] = (((c.r[2] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
let t = (c.r[3] as i64) > 0;  c.wr16((c.r[2] as u32).wrapping_add(0xfffffffc), c.r[6] as u16);
 if t { l = 0x004398b8; continue; }
 l = 0x00439a40; continue;
}
0x00439938 => {
c.r[8] = c.rd8((c.r[12] as u32)) as i8 as i64 as u64;
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.r[7] = c.r[5] & 0x3;
c.r[8] = (((c.r[8] as u32) << 2) as i32 as i64 as u64);
c.r[4] = (((c.r[4] as u32).wrapping_add((c.r[8] as u32))) as i32 as i64 as u64);
 l = 0x004398e8; continue;
}
0x00439950 => {
c.r[8] = c.rd32((c.r[11] as u32)) as i32 as i64 as u64;
c.r[8] = (((c.r[8] as u32).wrapping_shl((c.r[10] as u32) & 31)) as i32 as i64 as u64);
c.r[10] = (((c.r[10] as u32).wrapping_add(0xfffffff6)) as i32 as i64 as u64);
let t = (c.r[10] as i64) > 0;  c.r[8] = (((((c.r[8] as u32) as i32) >> 22) as u32) as i32 as i64 as u64);
 if t { l = 0x00439970; continue; }
c.r[10] = ((0u32.wrapping_add(0x00000016)) as i32 as i64 as u64);
c.r[11] = (((c.r[11] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
l = 0x00439970; continue; }
0x00439970 => {
c.r[7] = c.r[5] & 0x3;
c.r[8] = (((c.r[8] as u32) << 2) as i32 as i64 as u64);
c.r[4] = (((c.r[4] as u32).wrapping_add((c.r[8] as u32))) as i32 as i64 as u64);
 l = 0x004398e8; continue;
}
0x00439980 => {
c.r[4] = c.rd16((c.r[13] as u32)) as i16 as i64 as u64;
c.r[13] = (((c.r[13] as u32).wrapping_add(0x00000002)) as i32 as i64 as u64);
c.r[7] = c.r[5] & 0x3;
 l = 0x004398e8; continue;
}
0x00439990 => {
c.r[8] = c.rd8((c.r[12] as u32)) as i8 as i64 as u64;
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.r[7] = c.r[6] & 0x3;
c.r[8] = (((c.r[8] as u32) << 2) as i32 as i64 as u64);
c.r[5] = (((c.r[5] as u32).wrapping_add((c.r[8] as u32))) as i32 as i64 as u64);
 l = 0x00439900; continue;
}
0x004399a8 => {
c.r[8] = c.rd32((c.r[11] as u32)) as i32 as i64 as u64;
c.r[8] = (((c.r[8] as u32).wrapping_shl((c.r[10] as u32) & 31)) as i32 as i64 as u64);
c.r[10] = (((c.r[10] as u32).wrapping_add(0xfffffff6)) as i32 as i64 as u64);
let t = (c.r[10] as i64) > 0;  c.r[8] = (((((c.r[8] as u32) as i32) >> 22) as u32) as i32 as i64 as u64);
 if t { l = 0x004399c8; continue; }
c.r[10] = ((0u32.wrapping_add(0x00000016)) as i32 as i64 as u64);
c.r[11] = (((c.r[11] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
l = 0x004399c8; continue; }
0x004399c8 => {
c.r[7] = c.r[6] & 0x3;
c.r[8] = (((c.r[8] as u32) << 2) as i32 as i64 as u64);
c.r[5] = (((c.r[5] as u32).wrapping_add((c.r[8] as u32))) as i32 as i64 as u64);
 l = 0x00439900; continue;
}
0x004399d8 => {
c.r[5] = c.rd16((c.r[13] as u32)) as i16 as i64 as u64;
c.r[13] = (((c.r[13] as u32).wrapping_add(0x00000002)) as i32 as i64 as u64);
c.r[7] = c.r[6] & 0x3;
 l = 0x00439900; continue;
}
0x004399e8 => {
c.r[8] = c.rd8((c.r[12] as u32)) as i8 as i64 as u64;
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000006)) as i32 as i64 as u64);
c.r[8] = (((c.r[8] as u32) << 2) as i32 as i64 as u64);
c.r[6] = (((c.r[6] as u32).wrapping_add((c.r[8] as u32))) as i32 as i64 as u64);
 l = 0x00439918; continue;
}
0x00439a00 => {
c.r[8] = c.rd32((c.r[11] as u32)) as i32 as i64 as u64;
c.r[8] = (((c.r[8] as u32).wrapping_shl((c.r[10] as u32) & 31)) as i32 as i64 as u64);
c.r[10] = (((c.r[10] as u32).wrapping_add(0xfffffff6)) as i32 as i64 as u64);
let t = (c.r[10] as i64) > 0;  c.r[8] = (((((c.r[8] as u32) as i32) >> 22) as u32) as i32 as i64 as u64);
 if t { l = 0x00439a20; continue; }
c.r[10] = ((0u32.wrapping_add(0x00000016)) as i32 as i64 as u64);
c.r[11] = (((c.r[11] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
l = 0x00439a20; continue; }
0x00439a20 => {
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000006)) as i32 as i64 as u64);
c.r[8] = (((c.r[8] as u32) << 2) as i32 as i64 as u64);
c.r[6] = (((c.r[6] as u32).wrapping_add((c.r[8] as u32))) as i32 as i64 as u64);
 l = 0x00439918; continue;
}
0x00439a30 => {
c.r[6] = c.rd16((c.r[13] as u32)) as i16 as i64 as u64;
c.r[13] = (((c.r[13] as u32).wrapping_add(0x00000002)) as i32 as i64 as u64);
 l = 0x00439918; continue;
}
0x00439a40 => {
c.r[3] = c.rd32((c.r[24] as u32)) as i32 as i64 as u64;
c.r[4] = (((c.r[25] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
c.r[1] = (((c.r[15] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
c.r[2] = c.r[14].wrapping_add(0u64);
c.r[3] = (((c.r[3] as u32) >> 21) as i32 as i64 as u64);
c.r[5] = ((0u32.wrapping_add(0x00000008)) as i32 as i64 as u64);
c.r[3] = (((c.r[3] as u32).wrapping_add((c.r[4] as u32))) as i32 as i64 as u64);
c.r[4] = c.rd8((c.r[3] as u32)) as u64;
c.r[3] = (((c.r[3] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.r[6] = c.rd64((c.r[1] as u32));
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pextlh((c.r[6], c.rh[6]), (0u64, 0u64)); c.r[6] = a; c.rh[6] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[6], c.rh[6]), 16); c.r[6] = a; c.rh[6] = b; }
c.vf[1] = [c.r[6] as u32, (c.r[6] >> 32) as u32, c.rh[6] as u32, (c.rh[6] >> 32) as u32];
uycpu::vu0::itof::<3, 14>(c, 1, 1);
uycpu::vu0::fmac::<6, 14>(c, 1, 1, c.vf[1]);
uycpu::vu0::fmac::<1, 1>(c, 32, 0, uycpu::vu0::bcast(c.vf[1][0]));
uycpu::vu0::fmac::<3, 1>(c, 32, 0, uycpu::vu0::bcast(c.vf[1][1]));
uycpu::vu0::fmac::<3, 1>(c, 1, 0, uycpu::vu0::bcast(c.vf[1][2]));
uycpu::vu0::fmac::<4, 1>(c, 2, 1, uycpu::vu0::bcast(c.vf[0][0]));
l = 0x00439a98; continue; }
0x00439a98 => {
uycpu::vu0::qop::<1>(c, c.vf[0][0], c.vf[2][3]);
c.r[6] = c.rd64((c.r[1] as u32));
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pextlh((c.r[6], c.rh[6]), (0u64, 0u64)); c.r[6] = a; c.rh[6] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[6], c.rh[6]), 16); c.r[6] = a; c.rh[6] = b; }
c.vf[1] = [c.r[6] as u32, (c.r[6] >> 32) as u32, c.rh[6] as u32, (c.rh[6] >> 32) as u32];
uycpu::vu0::itof::<3, 14>(c, 1, 1);
uycpu::vu0::fmac::<0, 8>(c, 2, 0, uycpu::vu0::bcast(c.vi[22]));
uycpu::vu0::fmac::<6, 14>(c, 1, 1, c.vf[1]);
uycpu::vu0::fmac::<1, 1>(c, 32, 0, uycpu::vu0::bcast(c.vf[1][0]));
uycpu::vu0::fmac::<3, 1>(c, 32, 0, uycpu::vu0::bcast(c.vf[1][1]));
uycpu::vu0::ftoi::<3, 8>(c, 3, 2);
uycpu::vu0::fmac::<3, 1>(c, 1, 0, uycpu::vu0::bcast(c.vf[1][2]));
c.r[2] = (((c.r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
let t = (c.r[5] as i64) <= 0;  c.r[5] = (((c.r[5] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x00439b48; continue; }
l = 0x00439b10; continue; }
0x00439b10 => {
{ let (a, b) = { let v = c.vf[3]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; c.r[6] = a; c.rh[6] = b; }
c.r[7] = c.r[4] & 0x1;
uycpu::vu0::fmac::<4, 1>(c, 2, 1, uycpu::vu0::bcast(c.vf[0][0]));
c.r[8] = 0u64 | 0x7fff;
{ let (a, b) = uycpu::mmi::pminw((c.r[6], c.rh[6]), (c.r[8], c.rh[8])); c.r[6] = a; c.rh[6] = b; }
let t = c.r[7] == 0u64;  c.r[4] = (((c.r[4] as u32) >> 1) as i32 as i64 as u64);
 if t { l = 0x00439b38; continue; }
c.r[6] = ((0u32.wrapping_sub((c.r[6] as u32))) as i32 as i64 as u64);
l = 0x00439b38; continue; }
0x00439b38 => {
let t = c.r[2] != 0u64;  c.wr16((c.r[1] as u32).wrapping_add(0xfffffff6), c.r[6] as u16);
 if t { l = 0x00439a98; continue; }
 l = 0x00439b58; continue;
}
0x00439b48 => {
c.r[4] = c.rd8((c.r[3] as u32)) as u64;
c.r[3] = (((c.r[3] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.r[5] = ((0u32.wrapping_add(0x00000007)) as i32 as i64 as u64);
 l = 0x00439b10; continue;
}
0x00439b58 => {
c.r[2] = c.r[3].wrapping_add(0u64);
c.r[1] = (((c.r[14] as u32) << 1) as i32 as i64 as u64);
c.r[3] = (((c.r[14] as u32) << 3) as i32 as i64 as u64);
c.r[4] = (((c.r[14] as u32) << 2) as i32 as i64 as u64);
c.r[1] = (((c.r[24] as u32).wrapping_add((c.r[1] as u32))) as i32 as i64 as u64);
c.r[1] = (((c.r[1] as u32).wrapping_add((c.r[4] as u32))) as i32 as i64 as u64);
c.r[3] = (((c.r[3] as u32).wrapping_add((c.r[15] as u32))) as i32 as i64 as u64);
c.r[4] = c.rd8((c.r[1] as u32).wrapping_add(0x00000004)) as u64;
c.r[3] = (((c.r[3] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
c.r[11] = c.rd8((c.r[1] as u32).wrapping_add(0x00000005)) as u64;
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000006)) as i32 as i64 as u64);
let t = c.r[4] == 0u64;   if t { l = 0x00439cc0; continue; }
l = 0x00439b90; continue; }
0x00439b90 => {
c.r[5] = c.rd16((c.r[1] as u32).wrapping_add(0x00000006)) as i16 as i64 as u64;
c.r[9] = ((0u32.wrapping_add(0x00000001)) as i32 as i64 as u64);
c.r[6] = c.rd8((c.r[2] as u32)) as u64;
c.r[2] = (((c.r[2] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.wr16((c.r[3] as u32).wrapping_add(0x00000006), c.r[5] as u16);
c.r[10] = ((0u32.wrapping_add(0x00000002)) as i32 as i64 as u64);
c.r[5] = c.r[6] & 0x3;
c.r[8] = c.rd16((c.r[1] as u32)) as i16 as i64 as u64;
let t = c.r[5] == c.r[9];  c.r[4] = (((c.r[4] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x00439c18; continue; }
let t = c.r[5] == c.r[10];   if t { l = 0x00439c30; continue; }
l = 0x00439bc0; continue; }
0x00439bc0 => {
c.wr16((c.r[3] as u32), c.r[8] as u16);
c.r[6] = (((c.r[6] as u32) >> 2) as i32 as i64 as u64);
c.r[8] = c.rd16((c.r[1] as u32).wrapping_add(0x00000002)) as i16 as i64 as u64;
c.r[5] = c.r[6] & 0x3;
let t = c.r[5] == c.r[9];   if t { l = 0x00439c50; continue; }
let t = c.r[5] == c.r[10];   if t { l = 0x00439c68; continue; }
l = 0x00439be0; continue; }
0x00439be0 => {
c.wr16((c.r[3] as u32).wrapping_add(0x00000002), c.r[8] as u16);
c.r[6] = (((c.r[6] as u32) >> 2) as i32 as i64 as u64);
c.r[8] = c.rd16((c.r[1] as u32).wrapping_add(0x00000004)) as i16 as i64 as u64;
c.r[5] = c.r[6] & 0x3;
let t = c.r[5] == c.r[9];  c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
 if t { l = 0x00439c88; continue; }
let t = c.r[5] == c.r[10];   if t { l = 0x00439ca0; continue; }
l = 0x00439c00; continue; }
0x00439c00 => {
c.wr16((c.r[3] as u32).wrapping_add(0x00000004), c.r[8] as u16);
let t = (c.r[4] as i64) > 0;  c.r[3] = (((c.r[3] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
 if t { l = 0x00439b90; continue; }
 l = 0x00439cc0; continue;
}
0x00439c18 => {
c.r[7] = c.rd8((c.r[2] as u32)) as i8 as i64 as u64;
c.r[2] = (((c.r[2] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.r[7] = (((c.r[7] as u32) << 2) as i32 as i64 as u64);
c.r[8] = (((c.r[8] as u32).wrapping_add((c.r[7] as u32))) as i32 as i64 as u64);
 l = 0x00439bc0; continue;
}
0x00439c30 => {
c.r[8] = c.rd8((c.r[2] as u32).wrapping_add(0x00000001)) as i8 as i64 as u64;
c.r[7] = c.rd8((c.r[2] as u32)) as u64;
c.r[2] = (((c.r[2] as u32).wrapping_add(0x00000002)) as i32 as i64 as u64);
c.r[8] = (((c.r[8] as u32) << 8) as i32 as i64 as u64);
c.r[8] = c.r[8] | c.r[7];
 l = 0x00439bc0; continue;
}
0x00439c50 => {
c.r[7] = c.rd8((c.r[2] as u32)) as i8 as i64 as u64;
c.r[2] = (((c.r[2] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.r[7] = (((c.r[7] as u32) << 2) as i32 as i64 as u64);
c.r[8] = (((c.r[8] as u32).wrapping_add((c.r[7] as u32))) as i32 as i64 as u64);
 l = 0x00439be0; continue;
}
0x00439c68 => {
c.r[8] = c.rd8((c.r[2] as u32).wrapping_add(0x00000001)) as i8 as i64 as u64;
c.r[7] = c.rd8((c.r[2] as u32)) as u64;
c.r[2] = (((c.r[2] as u32).wrapping_add(0x00000002)) as i32 as i64 as u64);
c.r[8] = (((c.r[8] as u32) << 8) as i32 as i64 as u64);
c.r[8] = c.r[8] | c.r[7];
 l = 0x00439be0; continue;
}
0x00439c88 => {
c.r[7] = c.rd8((c.r[2] as u32)) as i8 as i64 as u64;
c.r[2] = (((c.r[2] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.r[7] = (((c.r[7] as u32) << 2) as i32 as i64 as u64);
c.r[8] = (((c.r[8] as u32).wrapping_add((c.r[7] as u32))) as i32 as i64 as u64);
 l = 0x00439c00; continue;
}
0x00439ca0 => {
c.r[8] = c.rd8((c.r[2] as u32).wrapping_add(0x00000001)) as i8 as i64 as u64;
c.r[7] = c.rd8((c.r[2] as u32)) as u64;
c.r[2] = (((c.r[2] as u32).wrapping_add(0x00000002)) as i32 as i64 as u64);
c.r[8] = (((c.r[8] as u32) << 8) as i32 as i64 as u64);
c.r[8] = c.r[8] | c.r[7];
 l = 0x00439c00; continue;
}
0x00439cc0 => {
let t = c.r[11] == 0u64;  c.r[4] = c.r[11].wrapping_add(0u64);
 if t { l = 0x00439df8; continue; }
l = 0x00439cc8; continue; }
0x00439cc8 => {
c.r[5] = c.rd16((c.r[1] as u32).wrapping_add(0x00000006)) as i16 as i64 as u64;
c.r[9] = ((0u32.wrapping_add(0x00000001)) as i32 as i64 as u64);
c.r[6] = c.rd8((c.r[2] as u32)) as u64;
c.r[2] = (((c.r[2] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.wr16((c.r[3] as u32).wrapping_add(0x00000006), c.r[5] as u16);
c.r[10] = ((0u32.wrapping_add(0x00000002)) as i32 as i64 as u64);
c.r[5] = c.r[6] & 0x3;
c.r[8] = c.rd16((c.r[1] as u32)) as i16 as i64 as u64;
let t = c.r[5] == c.r[9];  c.r[4] = (((c.r[4] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x00439d50; continue; }
let t = c.r[5] == c.r[10];   if t { l = 0x00439d68; continue; }
l = 0x00439cf8; continue; }
0x00439cf8 => {
c.wr16((c.r[3] as u32), c.r[8] as u16);
c.r[6] = (((c.r[6] as u32) >> 2) as i32 as i64 as u64);
c.r[8] = c.rd16((c.r[1] as u32).wrapping_add(0x00000002)) as i16 as i64 as u64;
c.r[5] = c.r[6] & 0x3;
let t = c.r[5] == c.r[9];   if t { l = 0x00439d88; continue; }
let t = c.r[5] == c.r[10];   if t { l = 0x00439da0; continue; }
l = 0x00439d18; continue; }
0x00439d18 => {
c.wr16((c.r[3] as u32).wrapping_add(0x00000002), c.r[8] as u16);
c.r[6] = (((c.r[6] as u32) >> 2) as i32 as i64 as u64);
c.r[8] = c.rd16((c.r[1] as u32).wrapping_add(0x00000004)) as i16 as i64 as u64;
c.r[5] = c.r[6] & 0x3;
let t = c.r[5] == c.r[9];  c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
 if t { l = 0x00439dc0; continue; }
let t = c.r[5] == c.r[10];   if t { l = 0x00439dd8; continue; }
l = 0x00439d38; continue; }
0x00439d38 => {
c.wr16((c.r[3] as u32).wrapping_add(0x00000004), c.r[8] as u16);
let t = (c.r[4] as i64) > 0;  c.r[3] = (((c.r[3] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
 if t { l = 0x00439cc8; continue; }
 l = 0x00439df8; continue;
}
0x00439d50 => {
c.r[7] = c.rd8((c.r[2] as u32)) as i8 as i64 as u64;
c.r[2] = (((c.r[2] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.r[7] = (((c.r[7] as u32) << 2) as i32 as i64 as u64);
c.r[8] = (((c.r[8] as u32).wrapping_add((c.r[7] as u32))) as i32 as i64 as u64);
 l = 0x00439cf8; continue;
}
0x00439d68 => {
c.r[8] = c.rd8((c.r[2] as u32).wrapping_add(0x00000001)) as i8 as i64 as u64;
c.r[7] = c.rd8((c.r[2] as u32)) as u64;
c.r[2] = (((c.r[2] as u32).wrapping_add(0x00000002)) as i32 as i64 as u64);
c.r[8] = (((c.r[8] as u32) << 8) as i32 as i64 as u64);
c.r[8] = c.r[8] | c.r[7];
 l = 0x00439cf8; continue;
}
0x00439d88 => {
c.r[7] = c.rd8((c.r[2] as u32)) as i8 as i64 as u64;
c.r[2] = (((c.r[2] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.r[7] = (((c.r[7] as u32) << 2) as i32 as i64 as u64);
c.r[8] = (((c.r[8] as u32).wrapping_add((c.r[7] as u32))) as i32 as i64 as u64);
 l = 0x00439d18; continue;
}
0x00439da0 => {
c.r[8] = c.rd8((c.r[2] as u32).wrapping_add(0x00000001)) as i8 as i64 as u64;
c.r[7] = c.rd8((c.r[2] as u32)) as u64;
c.r[2] = (((c.r[2] as u32).wrapping_add(0x00000002)) as i32 as i64 as u64);
c.r[8] = (((c.r[8] as u32) << 8) as i32 as i64 as u64);
c.r[8] = c.r[8] | c.r[7];
 l = 0x00439d18; continue;
}
0x00439dc0 => {
c.r[7] = c.rd8((c.r[2] as u32)) as i8 as i64 as u64;
c.r[2] = (((c.r[2] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.r[7] = (((c.r[7] as u32) << 2) as i32 as i64 as u64);
c.r[8] = (((c.r[8] as u32).wrapping_add((c.r[7] as u32))) as i32 as i64 as u64);
 l = 0x00439d38; continue;
}
0x00439dd8 => {
c.r[8] = c.rd8((c.r[2] as u32).wrapping_add(0x00000001)) as i8 as i64 as u64;
c.r[7] = c.rd8((c.r[2] as u32)) as u64;
c.r[2] = (((c.r[2] as u32).wrapping_add(0x00000002)) as i32 as i64 as u64);
c.r[8] = (((c.r[8] as u32) << 8) as i32 as i64 as u64);
c.r[8] = c.r[8] | c.r[7];
 l = 0x00439d38; continue;
}
0x00439df8 => {
let t = (c.r[31] as u32); c.wr64((c.r[3] as u32), 0u64);
 c.ret = t; return;
}
_ => panic!("the animation unit: no label {l:08x} in 0x004397F8"), } }
}

#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn n0043b7d0(c: &mut Cpu, k: F) {
let mut l: u32 = 0x0043b7d0;
loop { match l {
0x0043b7d0 => {
{ let (a, b) = uycpu::mmi::pcpyld((c.r[31], c.rh[31]), (0u64, 0u64)); c.r[31] = a; c.rh[31] = b; }
c.r[5] = c.rd32((c.r[4] as u32).wrapping_add(0x00000014)) as i32 as i64 as u64;
c.r[24] = (((c.r[4] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
c.r[31] = 0x0000000070000000;
c.r[12] = c.rd32((c.r[24] as u32).wrapping_add(0x00000038)) as i32 as i64 as u64;
c.r[8] = 0x0000000010000000;
c.r[13] = c.rd32((c.r[24] as u32).wrapping_add(0x0000003c)) as i32 as i64 as u64;
c.r[8] = c.r[8] | 0xd400;
c.r[7] = c.rd16((c.r[24] as u32).wrapping_add(0x00000002)) as u64;
c.r[25] = c.rd32((c.r[24] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
c.r[11] = c.rd32((c.r[5] as u32).wrapping_add(0x00000018)) as i32 as i64 as u64;
uycpu::vu0::fmac::<0, 14>(c, 5, 0, uycpu::vu0::bcast(c.vf[0][3]));
c.r[10] = c.rd8((c.r[5] as u32).wrapping_add(0x00000008)) as u64;
c.r[1] = 0u64 | 0x2000;
c.wr32((c.r[8] as u32).wrapping_add(0x00000080), c.r[1] as u32);
c.wr32((c.r[8] as u32).wrapping_add(0x00000010), c.r[11] as u32);
c.r[15] = (((c.r[10] as u32) << 4) as i32 as i64 as u64);
c.wr32((c.r[8] as u32).wrapping_add(0x00000020), c.r[10] as u32);
c.r[15] = (((c.r[15] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.wr64((c.r[15] as u32).wrapping_add(0x00001ff8), 0u64);
c.r[1] = 0u64 | 0x100;
c.wr32((c.r[8] as u32), c.r[1] as u32);
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
c.r[9] = c.rd32((c.r[24] as u32).wrapping_add(0x00000024)) as i32 as i64 as u64;
c.r[13] = (((c.r[13] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
c.r[1] = (((c.r[31] as u32).wrapping_add(0x00002800)) as i32 as i64 as u64);
c.r[2] = ((0u32.wrapping_add(0x00000080)) as i32 as i64 as u64);
c.r[3] = 0u64 | 0x7f7f;
{ let (a, b) = uycpu::mmi::pcpyh((c.r[3], c.rh[3])); c.r[3] = a; c.rh[3] = b; }
l = 0x0043b848; continue; }
0x0043b848 => {
c.wr64((c.r[1] as u32), c.r[3]);
c.wr64((c.r[1] as u32).wrapping_add(0x00000008), c.r[3]);
c.r[2] = (((c.r[2] as u32).wrapping_add(0xfffffff0)) as i32 as i64 as u64);
let t = (c.r[2] as i64) > 0;  c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
 if t { l = 0x0043b848; continue; }
c.r[1] = (((c.r[25] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
c.r[2] = (((c.r[25] as u32).wrapping_add((c.r[7] as u32))) as i32 as i64 as u64);
c.r[3] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
c.r[4] = c.rd8((c.r[1] as u32)) as u64;
l = 0x0043b878; continue; }
0x0043b878 => {
c.r[5] = (((c.r[4] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.r[4] = c.rd8((c.r[1] as u32).wrapping_add(0x00000001)) as u64;
c.wr8((c.r[5] as u32).wrapping_add(0x00002800), c.r[3] as u8);
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
let t = c.r[1] != c.r[2];  c.r[3] = (((c.r[3] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
 if t { l = 0x0043b878; continue; }
uycpu::vu0::fmac::<6, 1>(c, 5, 0, uycpu::vu0::bcast(c.vf[0][0]));
c.r[4] = c.rd16((c.r[12] as u32).wrapping_add(0xfffffff6)) as i16 as i64 as u64;
c.r[14] = (((c.r[4] as u32) << 4) as i32 as i64 as u64);
uycpu::vu0::fmac::<6, 15>(c, 1, 0, uycpu::vu0::bcast(c.vf[0][0]));
c.r[2] = c.rd16((c.r[12] as u32).wrapping_add(0xfffffffa)) as i16 as i64 as u64;
uycpu::vu0::fmac::<6, 15>(c, 2, 0, uycpu::vu0::bcast(c.vf[0][0]));
c.r[14] = (((c.r[14] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
{ let v = c.vf[0]; uycpu::vu0::mov::<15>(c, 3, [v[1], v[2], v[3], v[0]]); }
c.r[3] = c.rd16((c.r[12] as u32).wrapping_add(0xfffffffe)) as i16 as i64 as u64;
uycpu::vu0::fmac::<0, 8>(c, 1, 1, uycpu::vu0::bcast(c.vf[0][3]));
c.wr128((c.r[14] as u32).wrapping_add(0x00002ff0), 0u64, 0u64);
uycpu::vu0::fmac::<0, 4>(c, 2, 2, uycpu::vu0::bcast(c.vf[0][3]));
c.r[5] = (((c.r[31] as u32).wrapping_add(0xffffff80)) as i32 as i64 as u64);
c.r[6] = (((c.r[7] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
l = 0x0043b8d0; continue; }
0x0043b8d0 => {
{ let v = c.vf[5]; c.wr128((c.r[5] as u32).wrapping_add(0x00000080), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[5]; c.wr128((c.r[5] as u32).wrapping_add(0x00000090), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[5] = (((c.r[5] as u32).wrapping_add(0x00000080)) as i32 as i64 as u64);
{ let v = c.vf[5]; c.wr128((c.r[5] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[6] = (((c.r[6] as u32).wrapping_add(0xfffffffe)) as i32 as i64 as u64);
let t = (c.r[6] as i64) > 0;  { let v = c.vf[5]; c.wr128((c.r[5] as u32).wrapping_add(0x00000050), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043b8d0; continue; }
{ let v = c.vf[5]; c.wr128((c.r[31] as u32).wrapping_add(0x00001fc0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[5]; c.wr128((c.r[31] as u32).wrapping_add(0x00001fd0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
l = 0x0043b8f8; continue; }
0x0043b8f8 => {
c.r[1] = c.rd64((c.r[15] as u32).wrapping_add(0x00001ff8));
c.r[5] = (((c.r[31] as u32).wrapping_add(0x00003000)) as i32 as i64 as u64);
c.r[6] = 0u64 | 0x100;
let t = c.r[1] == 0u64;   if t { l = 0x0043b8f8; continue; }
c.wr32((c.r[8] as u32).wrapping_add(0x00000080), c.r[5] as u32);
c.wr32((c.r[8] as u32).wrapping_add(0x00000010), c.r[12] as u32);
c.wr32((c.r[8] as u32).wrapping_add(0x00000020), c.r[4] as u32);
c.wr32((c.r[8] as u32), c.r[6] as u32);
c.r[4] = (((c.r[31] as u32).wrapping_add(0xffffffc0)) as i32 as i64 as u64);
c.r[5] = (((c.r[25] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
c.r[6] = (((c.r[5] as u32).wrapping_add((c.r[7] as u32))) as i32 as i64 as u64);
c.r[1] = c.rd8((c.r[5] as u32)) as u64;
c.r[1] = (((c.r[1] as u32) << 4) as i32 as i64 as u64);
c.r[1] = (((c.r[1] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
l = 0x0043b940; continue; }
0x0043b940 => {
{ let (a, b) = c.rd128((c.r[1] as u32).wrapping_add(0x00002000)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
c.r[4] = (((c.r[4] as u32).wrapping_add(0x00000040)) as i32 as i64 as u64);
c.r[1] = c.rd8((c.r[5] as u32).wrapping_add(0x00000001)) as u64;
c.r[5] = (((c.r[5] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
{ let v = c.vf[6]; c.wr128((c.r[4] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[6]; c.wr128((c.r[4] as u32).wrapping_add(0x00000030), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[1] = (((c.r[1] as u32) << 4) as i32 as i64 as u64);
c.wr32((c.r[4] as u32).wrapping_add(0x0000003c), 0u64 as u32);
let t = c.r[5] != c.r[6];  c.r[1] = (((c.r[1] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
 if t { l = 0x0043b940; continue; }
c.wr128((c.r[31] as u32).wrapping_add(0x00001fe0), 0u64, 0u64);
c.wr128((c.r[31] as u32).wrapping_add(0x00001ff0), 0u64, 0u64);
let t = c.r[9] == 0u64;   if t { l = 0x0043bd30; continue; }
{ let (a, b) = uycpu::mmi::pcpyld((c.r[7], c.rh[7]), (c.r[10], c.rh[10])); c.r[10] = a; c.rh[10] = b; }
c.r[7] = c.rd16((c.r[13] as u32).wrapping_add(0xfffffff6)) as i16 as i64 as u64;
c.r[5] = c.rd16((c.r[13] as u32).wrapping_add(0xfffffffa)) as i16 as i64 as u64;
c.vf[4] = [c.r[9] as u32, (c.r[9] >> 32) as u32, c.rh[9] as u32, (c.rh[9] >> 32) as u32];
c.r[15] = (((c.r[7] as u32) << 4) as i32 as i64 as u64);
c.r[6] = c.rd16((c.r[13] as u32).wrapping_add(0xfffffffe)) as i16 as i64 as u64;
c.r[15] = (((c.r[15] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.wr128((c.r[15] as u32).wrapping_add(0x000035f0), 0u64, 0u64);
uycpu::vu0::fmac::<1, 1>(c, 4, 0, uycpu::vu0::bcast(c.vf[4][0]));
l = 0x0043b9a8; continue; }
0x0043b9a8 => {
c.r[1] = c.rd64((c.r[14] as u32).wrapping_add(0x00002ff0));
c.r[8] = 0x0000000010000000;
c.r[8] = c.r[8] | 0xd400;
let t = c.r[1] == 0u64;   if t { l = 0x0043b9a8; continue; }
c.wr32((c.r[8] as u32).wrapping_add(0x00000010), c.r[13] as u32);
c.r[1] = (((c.r[31] as u32).wrapping_add(0x00003600)) as i32 as i64 as u64);
c.wr32((c.r[8] as u32).wrapping_add(0x00000080), c.r[1] as u32);
c.r[14] = (((c.r[31] as u32).wrapping_add(0x00003d00)) as i32 as i64 as u64);
c.wr32((c.r[8] as u32).wrapping_add(0x00000020), c.r[7] as u32);
c.r[1] = 0u64 | 0x100;
c.wr32((c.r[8] as u32), c.r[1] as u32);
c.r[13] = (((c.r[31] as u32).wrapping_add(0x00003c00)) as i32 as i64 as u64);
c.r[7] = (((c.r[10] as u32) << 3) as i32 as i64 as u64);
c.r[4] = (((c.r[31] as u32).wrapping_add(0x00003000)) as i32 as i64 as u64);
uycpu::vu0::fmac::<6, 15>(c, 5, 0, uycpu::vu0::bcast(c.vf[0][0]));
c.r[4] = (((c.r[4] as u32).wrapping_add((c.r[7] as u32))) as i32 as i64 as u64);
let t = c.r[2] == 0u64;  c.r[7] = c.rd64((c.r[4] as u32));
 if t { l = 0x0043ba68; continue; }
c.r[8] = c.rd8((c.r[4] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[9] = a; c.rh[9] = b; }
{ let (a, b) = uycpu::mmi::psrlw((c.r[9], c.rh[9]), 13); c.r[9] = a; c.rh[9] = b; }
l = 0x0043ba10; continue; }
0x0043ba10 => {
c.vf[6] = [c.r[9] as u32, (c.r[9] >> 32) as u32, c.rh[9] as u32, (c.rh[9] >> 32) as u32];
c.r[8] = (((c.r[8] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.r[8] = c.rd8((c.r[8] as u32).wrapping_add(0x00002800)) as u64;
c.r[12] = c.r[7] >> 62;
c.r[7] = c.rd64((c.r[4] as u32).wrapping_add(0x00000008));
c.r[4] = (((c.r[4] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 14>(c, 5, 6);
c.r[11] = (((c.r[8] as u32) << 6) as i32 as i64 as u64);
c.r[11] = (((c.r[11] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.r[2] = (((c.r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
c.r[8] = c.rd8((c.r[4] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[9] = a; c.rh[9] = b; }
{ let (a, b) = uycpu::mmi::psrlw((c.r[9], c.rh[9]), 13); c.r[9] = a; c.rh[9] = b; }
c.wr32((c.r[13] as u32), c.r[11] as u32);
c.r[13] = (((c.r[13] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
c.r[12] = (((c.r[12] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
c.wr32((c.r[11] as u32).wrapping_add(0x0000001c), c.r[12] as u32);
{ let v = c.vf[5]; c.wr128((c.r[11] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[1] = ((0u32.wrapping_add(0xfffffffc)) as i32 as i64 as u64);
let t = (c.r[2] as i64) > 0;  c.wr32((c.r[11] as u32).wrapping_add(0x0000000c), c.r[1] as u32);
 if t { l = 0x0043ba10; continue; }
l = 0x0043ba68; continue; }
0x0043ba68 => {
let t = c.r[3] == 0u64;  c.r[7] = c.rd64((c.r[4] as u32));
 if t { l = 0x0043bad0; continue; }
c.r[8] = c.rd8((c.r[4] as u32).wrapping_add(0x00000006)) as u64;
uycpu::vu0::mov::<1>(c, 5, c.vf[0]);
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
l = 0x0043ba80; continue; }
0x0043ba80 => {
c.vf[6] = [c.r[7] as u32, (c.r[7] >> 32) as u32, c.rh[7] as u32, (c.rh[7] >> 32) as u32];
c.r[8] = (((c.r[8] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.r[8] = c.rd8((c.r[8] as u32).wrapping_add(0x00002800)) as u64;
c.r[4] = (((c.r[4] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
c.r[7] = c.rd64((c.r[4] as u32));
c.r[12] = (((c.r[8] as u32) << 6) as i32 as i64 as u64);
uycpu::vu0::itof::<0, 14>(c, 5, 6);
c.r[12] = (((c.r[12] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.r[8] = c.rd8((c.r[4] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
c.wr32((c.r[14] as u32), c.r[12] as u32);
c.r[14] = (((c.r[14] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
c.r[11] = c.rd32((c.r[12] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
{ let v = c.vf[5]; c.wr128((c.r[12] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[3] = (((c.r[3] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
c.wr32((c.r[12] as u32).wrapping_add(0x0000002c), c.r[11] as u32);
c.r[1] = ((0u32.wrapping_add(0xfffffffc)) as i32 as i64 as u64);
let t = (c.r[3] as i64) > 0;  c.wr32((c.r[12] as u32).wrapping_add(0x0000003c), c.r[1] as u32);
 if t { l = 0x0043ba80; continue; }
l = 0x0043bad0; continue; }
0x0043bad0 => {
c.r[1] = c.rd64((c.r[15] as u32).wrapping_add(0x000035f0));
c.r[7] = (((c.r[10] as u32) << 3) as i32 as i64 as u64);
c.r[4] = (((c.r[31] as u32).wrapping_add(0x00003600)) as i32 as i64 as u64);
uycpu::vu0::fmac::<6, 15>(c, 5, 0, uycpu::vu0::bcast(c.vf[0][0]));
let t = c.r[1] == 0u64;  c.r[4] = (((c.r[4] as u32).wrapping_add((c.r[7] as u32))) as i32 as i64 as u64);
 if t { l = 0x0043bad0; continue; }
let t = c.r[5] == 0u64;  c.r[7] = c.rd64((c.r[4] as u32));
 if t { l = 0x0043bb60; continue; }
c.r[8] = c.rd8((c.r[4] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[9] = a; c.rh[9] = b; }
{ let (a, b) = uycpu::mmi::psrlw((c.r[9], c.rh[9]), 13); c.r[9] = a; c.rh[9] = b; }
l = 0x0043bb08; continue; }
0x0043bb08 => {
c.vf[6] = [c.r[9] as u32, (c.r[9] >> 32) as u32, c.rh[9] as u32, (c.rh[9] >> 32) as u32];
c.r[8] = (((c.r[8] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.r[8] = c.rd8((c.r[8] as u32).wrapping_add(0x00002800)) as u64;
c.r[12] = c.r[7] >> 62;
c.r[7] = c.rd64((c.r[4] as u32).wrapping_add(0x00000008));
c.r[4] = (((c.r[4] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 14>(c, 5, 6);
c.r[11] = (((c.r[8] as u32) << 6) as i32 as i64 as u64);
c.r[8] = c.rd8((c.r[4] as u32).wrapping_add(0x00000006)) as u64;
c.r[11] = (((c.r[11] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[9] = a; c.rh[9] = b; }
c.wr32((c.r[13] as u32), c.r[11] as u32);
{ let (a, b) = uycpu::mmi::psrlw((c.r[9], c.rh[9]), 13); c.r[9] = a; c.rh[9] = b; }
c.r[1] = c.rd32((c.r[11] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
c.r[5] = (((c.r[5] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
c.r[12] = (((c.r[12] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
{ let v = c.vf[5]; c.wr128((c.r[11] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.wr32((c.r[11] as u32).wrapping_add(0x0000001c), c.r[12] as u32);
c.r[13] = (((c.r[13] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
let t = c.r[5] != 0u64;  c.r[13] = (((c.r[13] as u32).wrapping_add((c.r[1] as u32))) as i32 as i64 as u64);
 if t { l = 0x0043bb08; continue; }
l = 0x0043bb60; continue; }
0x0043bb60 => {
let t = c.r[6] == 0u64;  c.r[7] = c.rd64((c.r[4] as u32));
 if t { l = 0x0043bbc8; continue; }
c.r[8] = c.rd8((c.r[4] as u32).wrapping_add(0x00000006)) as u64;
uycpu::vu0::mov::<1>(c, 5, c.vf[0]);
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
l = 0x0043bb78; continue; }
0x0043bb78 => {
c.vf[6] = [c.r[7] as u32, (c.r[7] >> 32) as u32, c.rh[7] as u32, (c.rh[7] >> 32) as u32];
c.r[8] = (((c.r[8] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.r[8] = c.rd8((c.r[8] as u32).wrapping_add(0x00002800)) as u64;
c.r[4] = (((c.r[4] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
c.r[7] = c.rd64((c.r[4] as u32));
c.r[12] = (((c.r[8] as u32) << 6) as i32 as i64 as u64);
uycpu::vu0::itof::<0, 14>(c, 5, 6);
c.r[12] = (((c.r[12] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.r[8] = c.rd8((c.r[4] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
c.wr32((c.r[14] as u32), c.r[12] as u32);
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
c.r[1] = c.rd32((c.r[12] as u32).wrapping_add(0x0000003c)) as i32 as i64 as u64;
c.r[14] = (((c.r[14] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
{ let v = c.vf[5]; c.wr128((c.r[12] as u32).wrapping_add(0x00000030), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[6] = (((c.r[6] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
c.wr32((c.r[12] as u32).wrapping_add(0x0000003c), c.r[1] as u32);
let t = (c.r[6] as i64) > 0;  c.r[14] = (((c.r[14] as u32).wrapping_add((c.r[1] as u32))) as i32 as i64 as u64);
 if t { l = 0x0043bb78; continue; }
l = 0x0043bbc8; continue; }
0x0043bbc8 => {
c.r[1] = (((c.r[31] as u32).wrapping_add(0x00003c00)) as i32 as i64 as u64);
c.r[2] = c.rd32((c.r[1] as u32)) as i32 as i64 as u64;
c.r[3] = (((c.r[31] as u32).wrapping_add(0x00001e00)) as i32 as i64 as u64);
let t = c.r[1] == c.r[13];  c.wr32((c.r[13] as u32), c.r[3] as u32);
 if t { l = 0x0043bc18; continue; }
{ let (a, b) = c.rd128((c.r[2] as u32)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[2] as u32).wrapping_add(0x00000010)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
l = 0x0043bbe8; continue; }
0x0043bbe8 => {
uycpu::vu0::fmac::<6, 14>(c, 32, 5, uycpu::vu0::bcast(c.vf[4][3]));
c.r[3] = (((c.r[2] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
uycpu::vu0::fmac::<2, 14>(c, 7, 6, uycpu::vu0::bcast(c.vf[4][0]));
c.r[4] = c.rd32((c.r[2] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
c.r[2] = c.rd32((c.r[1] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[2] as u32)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[2] as u32).wrapping_add(0x00000010)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let v = c.vf[7]; c.wr128((c.r[3] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
let t = c.r[1] != c.r[13];  c.wr32((c.r[3] as u32).wrapping_add(0x0000001c), c.r[4] as u32);
 if t { l = 0x0043bbe8; continue; }
l = 0x0043bc18; continue; }
0x0043bc18 => {
c.r[1] = (((c.r[31] as u32).wrapping_add(0x00003d00)) as i32 as i64 as u64);
c.r[2] = c.rd32((c.r[1] as u32)) as i32 as i64 as u64;
c.r[3] = (((c.r[31] as u32).wrapping_add(0x00001e00)) as i32 as i64 as u64);
let t = c.r[1] == c.r[14];  c.wr32((c.r[14] as u32), c.r[3] as u32);
 if t { l = 0x0043bc68; continue; }
{ let (a, b) = c.rd128((c.r[2] as u32).wrapping_add(0x00000020)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[2] as u32).wrapping_add(0x00000030)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
l = 0x0043bc38; continue; }
0x0043bc38 => {
uycpu::vu0::fmac::<6, 14>(c, 32, 5, uycpu::vu0::bcast(c.vf[4][3]));
c.r[3] = (((c.r[2] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
uycpu::vu0::fmac::<2, 14>(c, 7, 6, uycpu::vu0::bcast(c.vf[4][0]));
c.r[4] = c.rd32((c.r[2] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
c.r[2] = c.rd32((c.r[1] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[2] as u32).wrapping_add(0x00000020)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[2] as u32).wrapping_add(0x00000030)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let v = c.vf[7]; c.wr128((c.r[3] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
let t = c.r[1] != c.r[14];  c.wr32((c.r[3] as u32).wrapping_add(0x0000002c), c.r[4] as u32);
 if t { l = 0x0043bc38; continue; }
l = 0x0043bc68; continue; }
0x0043bc68 => {
c.r[7] = c.rd8((c.r[24] as u32).wrapping_add(0x00000022)) as u64;
{ let (a, b) = uycpu::mmi::pcpyud((c.r[10], c.rh[10]), (0u64, 0u64)); c.r[10] = a; c.rh[10] = b; }
c.r[8] = c.rd8((c.r[24] as u32).wrapping_add(0x00000023)) as u64;
c.r[1] = (((c.r[25] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
c.r[5] = c.rd8((c.r[24] as u32).wrapping_add(0x00000020)) as u64;
c.r[2] = (((c.r[25] as u32).wrapping_add((c.r[10] as u32))) as i32 as i64 as u64);
c.r[6] = c.rd8((c.r[24] as u32).wrapping_add(0x00000021)) as u64;
c.r[15] = (((c.r[7] as u32).wrapping_sub((c.r[8] as u32))) as i32 as i64 as u64);
c.r[5] = (((c.r[5] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.r[5] = (((c.r[5] as u32).wrapping_sub((c.r[6] as u32))) as i32 as i64 as u64);
c.r[15] = c.r[15] | c.r[5];
c.r[3] = (((c.r[31] as u32).wrapping_add(0xffffffc0)) as i32 as i64 as u64);
let t = c.r[15] != 0u64;  c.r[4] = c.rd8((c.r[1] as u32)) as u64;
 if t { l = 0x0043bf38; continue; }
c.r[6] = (((c.r[4] as u32) << 3) as i32 as i64 as u64);
c.r[7] = (((c.r[4] as u32) << 6) as i32 as i64 as u64);
c.r[5] = (((c.r[6] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.r[6] = (((c.r[6] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.r[5] = c.rd64((c.r[5] as u32).wrapping_add(0x00003000));
c.r[6] = c.rd64((c.r[6] as u32).wrapping_add(0x00003600));
{ let (a, b) = uycpu::mmi::pextlh((c.r[5], c.rh[5]), (0u64, 0u64)); c.r[5] = a; c.rh[5] = b; }
{ let (a, b) = uycpu::mmi::pextlh((c.r[6], c.rh[6]), (0u64, 0u64)); c.r[6] = a; c.rh[6] = b; }
l = 0x0043bcc0; continue; }
0x0043bcc0 => {
c.r[4] = c.rd8((c.r[1] as u32).wrapping_add(0x00000001)) as u64;
{ let (a, b) = uycpu::mmi::psraw((c.r[5], c.rh[5]), 16); c.r[5] = a; c.rh[5] = b; }
c.vf[5] = [c.r[5] as u32, (c.r[5] >> 32) as u32, c.rh[5] as u32, (c.rh[5] >> 32) as u32];
{ let (a, b) = uycpu::mmi::psraw((c.r[6], c.rh[6]), 16); c.r[6] = a; c.rh[6] = b; }
c.vf[6] = [c.r[6] as u32, (c.r[6] >> 32) as u32, c.rh[6] as u32, (c.rh[6] >> 32) as u32];
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 15>(c, 5, 5);
c.r[6] = (((c.r[4] as u32) << 3) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 15>(c, 6, 6);
c.r[8] = (((c.r[7] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.r[5] = (((c.r[6] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.r[6] = (((c.r[6] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.r[3] = (((c.r[3] as u32).wrapping_add(0x00000040)) as i32 as i64 as u64);
c.r[7] = (((c.r[4] as u32) << 6) as i32 as i64 as u64);
uycpu::vu0::fmac::<6, 15>(c, 32, 5, uycpu::vu0::bcast(c.vf[4][3]));
c.r[5] = c.rd64((c.r[5] as u32).wrapping_add(0x00003000));
uycpu::vu0::fmac::<2, 15>(c, 7, 6, uycpu::vu0::bcast(c.vf[4][0]));
c.r[6] = c.rd64((c.r[6] as u32).wrapping_add(0x00003600));
{ let (a, b) = uycpu::mmi::pextlh((c.r[5], c.rh[5]), (0u64, 0u64)); c.r[5] = a; c.rh[5] = b; }
{ let (a, b) = uycpu::mmi::pextlh((c.r[6], c.rh[6]), (0u64, 0u64)); c.r[6] = a; c.rh[6] = b; }
c.wr32((c.r[3] as u32).wrapping_add(0x0000002c), c.r[8] as u32);
let t = c.r[1] != c.r[2];  { let v = c.vf[7]; c.wr128((c.r[3] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043bcc0; continue; }
 l = 0x0043be88; continue;
}
0x0043bd30 => {
c.r[1] = c.rd64((c.r[14] as u32).wrapping_add(0x00002ff0));
c.r[5] = (((c.r[10] as u32) << 3) as i32 as i64 as u64);
c.r[12] = (((c.r[31] as u32).wrapping_add(0x00003000)) as i32 as i64 as u64);
uycpu::vu0::fmac::<6, 15>(c, 5, 0, uycpu::vu0::bcast(c.vf[0][0]));
let t = c.r[1] == 0u64;  c.r[12] = (((c.r[12] as u32).wrapping_add((c.r[5] as u32))) as i32 as i64 as u64);
 if t { l = 0x0043bd30; continue; }
c.r[10] = (((c.r[7] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
let t = c.r[2] == 0u64;  c.r[5] = c.rd64((c.r[12] as u32));
 if t { l = 0x0043bdb8; continue; }
c.r[6] = c.rd8((c.r[12] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((c.r[5], c.rh[5]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
{ let (a, b) = uycpu::mmi::psrlw((c.r[7], c.rh[7]), 13); c.r[7] = a; c.rh[7] = b; }
l = 0x0043bd70; continue; }
0x0043bd70 => {
c.vf[6] = [c.r[7] as u32, (c.r[7] >> 32) as u32, c.rh[7] as u32, (c.rh[7] >> 32) as u32];
c.r[8] = (((c.r[6] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.r[8] = c.rd8((c.r[8] as u32).wrapping_add(0x00002800)) as u64;
c.r[9] = c.r[5] >> 62;
c.r[5] = c.rd64((c.r[12] as u32).wrapping_add(0x00000008));
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 14>(c, 5, 6);
c.r[8] = (((c.r[8] as u32) << 6) as i32 as i64 as u64);
c.r[8] = (((c.r[8] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.r[2] = (((c.r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
c.r[6] = c.rd8((c.r[12] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((c.r[5], c.rh[5]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
c.r[9] = (((c.r[9] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::psrlw((c.r[7], c.rh[7]), 13); c.r[7] = a; c.rh[7] = b; }
{ let v = c.vf[5]; c.wr128((c.r[8] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
let t = (c.r[2] as i64) > 0;  c.wr32((c.r[8] as u32).wrapping_add(0x0000001c), c.r[9] as u32);
 if t { l = 0x0043bd70; continue; }
l = 0x0043bdb8; continue; }
0x0043bdb8 => {
let t = c.r[3] == 0u64;  c.r[5] = c.rd64((c.r[12] as u32));
 if t { l = 0x0043be10; continue; }
c.r[6] = c.rd8((c.r[12] as u32).wrapping_add(0x00000006)) as u64;
uycpu::vu0::mov::<1>(c, 5, c.vf[0]);
{ let (a, b) = uycpu::mmi::pextlh((c.r[5], c.rh[5]), (0u64, 0u64)); c.r[5] = a; c.rh[5] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[5], c.rh[5]), 16); c.r[5] = a; c.rh[5] = b; }
l = 0x0043bdd0; continue; }
0x0043bdd0 => {
c.vf[6] = [c.r[5] as u32, (c.r[5] >> 32) as u32, c.rh[5] as u32, (c.rh[5] >> 32) as u32];
c.r[8] = (((c.r[6] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.r[8] = c.rd8((c.r[8] as u32).wrapping_add(0x00002800)) as u64;
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
c.r[5] = c.rd64((c.r[12] as u32));
c.r[8] = (((c.r[8] as u32) << 6) as i32 as i64 as u64);
uycpu::vu0::itof::<0, 14>(c, 5, 6);
c.r[8] = (((c.r[8] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.r[6] = c.rd8((c.r[12] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((c.r[5], c.rh[5]), (0u64, 0u64)); c.r[5] = a; c.rh[5] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[5], c.rh[5]), 16); c.r[5] = a; c.rh[5] = b; }
c.r[3] = (((c.r[3] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
let t = (c.r[3] as i64) > 0;  { let v = c.vf[5]; c.wr128((c.r[8] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043bdd0; continue; }
l = 0x0043be10; continue; }
0x0043be10 => {
c.r[1] = (((c.r[25] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
c.r[2] = (((c.r[25] as u32).wrapping_add((c.r[10] as u32))) as i32 as i64 as u64);
c.r[3] = (((c.r[31] as u32).wrapping_add(0xffffffc0)) as i32 as i64 as u64);
c.r[4] = c.rd8((c.r[1] as u32)) as u64;
c.r[5] = (((c.r[4] as u32) << 3) as i32 as i64 as u64);
c.r[6] = (((c.r[4] as u32) << 6) as i32 as i64 as u64);
c.r[5] = (((c.r[5] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.r[5] = c.rd64((c.r[5] as u32).wrapping_add(0x00003000));
c.r[6] = (((c.r[6] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pextlh((c.r[5], c.rh[5]), (0u64, 0u64)); c.r[5] = a; c.rh[5] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[5], c.rh[5]), 16); c.r[5] = a; c.rh[5] = b; }
l = 0x0043be40; continue; }
0x0043be40 => {
c.r[4] = c.rd8((c.r[1] as u32).wrapping_add(0x00000001)) as u64;
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.vf[5] = [c.r[5] as u32, (c.r[5] >> 32) as u32, c.rh[5] as u32, (c.rh[5] >> 32) as u32];
c.r[7] = (((c.r[6] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
c.r[5] = (((c.r[4] as u32) << 3) as i32 as i64 as u64);
c.r[6] = (((c.r[4] as u32) << 6) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 15>(c, 5, 5);
c.r[5] = (((c.r[5] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.r[5] = c.rd64((c.r[5] as u32).wrapping_add(0x00003000));
c.r[3] = (((c.r[3] as u32).wrapping_add(0x00000040)) as i32 as i64 as u64);
c.r[6] = (((c.r[6] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pextlh((c.r[5], c.rh[5]), (0u64, 0u64)); c.r[5] = a; c.rh[5] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[5], c.rh[5]), 16); c.r[5] = a; c.rh[5] = b; }
c.wr32((c.r[3] as u32).wrapping_add(0x0000002c), c.r[7] as u32);
let t = c.r[1] != c.r[2];  { let v = c.vf[5]; c.wr128((c.r[3] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043be40; continue; }
l = 0x0043be88; continue; }
0x0043be88 => {
c.r[1] = (((c.r[31] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
c.r[2] = (((c.r[31] as u32).wrapping_add(0xffffffd0)) as i32 as i64 as u64);
c.r[3] = (((c.r[10] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
l = 0x0043be98; continue; }
0x0043be98 => {
{ let (a, b) = c.rd128((c.r[1] as u32)); c.r[4] = a; c.rh[4] = b; }
c.r[3] = (((c.r[3] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[1] as u32).wrapping_add(0x00000010)); c.r[5] = a; c.rh[5] = b; }
c.r[2] = (((c.r[2] as u32).wrapping_add(0x00000030)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[1] as u32).wrapping_add(0x00000020)); c.r[6] = a; c.rh[6] = b; }
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000040)) as i32 as i64 as u64);
c.wr128((c.r[2] as u32), c.r[4], c.rh[4]);
c.wr128((c.r[2] as u32).wrapping_add(0x00000010), c.r[5], c.rh[5]);
let t = (c.r[3] as i64) > 0;  c.wr128((c.r[2] as u32).wrapping_add(0x00000020), c.r[6], c.rh[6]);
 if t { l = 0x0043be98; continue; }
c.r[24] = c.rd32((c.r[24] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64;
c.r[31] = 0x0000000010000000;
c.r[31] = c.r[31] | 0xd000;
c.r[1] = (((c.r[10] as u32) << 1) as i32 as i64 as u64);
c.wr32((c.r[31] as u32).wrapping_add(0x00000080), 0u64 as u32);
c.r[10] = (((c.r[10] as u32).wrapping_add((c.r[1] as u32))) as i32 as i64 as u64);
let t = c.r[24] == 0u64;  c.wr32((c.r[31] as u32).wrapping_add(0x00000020), c.r[10] as u32);
 if t { l = 0x0043bf28; continue; }
c.wr32((c.r[31] as u32).wrapping_add(0x00000010), c.r[24] as u32);
c.r[1] = 0u64 | 0x100;
c.wr32((c.r[31] as u32).wrapping_add(0x00001010), c.r[1] as u32);
c.r[1] = ((0u32.wrapping_add(0x00000100)) as i32 as i64 as u64);
c.wr32((c.r[31] as u32), c.r[1] as u32);
l = 0x0043bf08; continue; }
0x0043bf08 => {
let t = !c.cpcond0();   if t { l = 0x0043bf08; continue; }
l = 0x0043bf28; continue; }
0x0043bf28 => {
{ let (a, b) = uycpu::mmi::pcpyud((c.r[31], c.rh[31]), (0u64, 0u64)); c.r[31] = a; c.rh[31] = b; }
let t = (c.r[31] as u32);  c.ret = t; return;
}
0x0043bf38 => {
c.r[6] = (((c.r[4] as u32) << 3) as i32 as i64 as u64);
c.r[7] = (((c.r[4] as u32) << 6) as i32 as i64 as u64);
c.r[5] = (((c.r[6] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.r[6] = (((c.r[6] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.r[5] = c.rd64((c.r[5] as u32).wrapping_add(0x00003000));
c.r[6] = c.rd64((c.r[6] as u32).wrapping_add(0x00003600));
{ let (a, b) = uycpu::mmi::pextlh((c.r[5], c.rh[5]), (0u64, 0u64)); c.r[5] = a; c.rh[5] = b; }
{ let (a, b) = uycpu::mmi::pextlh((c.r[6], c.rh[6]), (0u64, 0u64)); c.r[6] = a; c.rh[6] = b; }
l = 0x0043bf58; continue; }
0x0043bf58 => {
c.r[4] = c.rd8((c.r[1] as u32).wrapping_add(0x00000001)) as u64;
{ let (a, b) = uycpu::mmi::psraw((c.r[5], c.rh[5]), 16); c.r[5] = a; c.rh[5] = b; }
c.vf[5] = [c.r[5] as u32, (c.r[5] >> 32) as u32, c.rh[5] as u32, (c.rh[5] >> 32) as u32];
{ let (a, b) = uycpu::mmi::psraw((c.r[6], c.rh[6]), 16); c.r[6] = a; c.rh[6] = b; }
c.vf[6] = [c.r[6] as u32, (c.r[6] >> 32) as u32, c.rh[6] as u32, (c.rh[6] >> 32) as u32];
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 15>(c, 5, 5);
c.r[6] = (((c.r[4] as u32) << 3) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 15>(c, 6, 6);
c.r[8] = (((c.r[7] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.r[5] = (((c.r[6] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.r[6] = (((c.r[6] as u32).wrapping_add((c.r[31] as u32))) as i32 as i64 as u64);
c.r[3] = (((c.r[3] as u32).wrapping_add(0x00000040)) as i32 as i64 as u64);
c.r[7] = (((c.r[4] as u32) << 6) as i32 as i64 as u64);
uycpu::vu0::fmac::<6, 15>(c, 5, 5, uycpu::vu0::bcast(c.vf[4][3]));
c.r[5] = c.rd64((c.r[5] as u32).wrapping_add(0x00003000));
uycpu::vu0::fmac::<6, 15>(c, 6, 6, uycpu::vu0::bcast(c.vf[4][0]));
c.r[6] = c.rd64((c.r[6] as u32).wrapping_add(0x00003600));
{ let (a, b) = uycpu::mmi::pextlh((c.r[5], c.rh[5]), (0u64, 0u64)); c.r[5] = a; c.rh[5] = b; }
{ let (a, b) = uycpu::mmi::pextlh((c.r[6], c.rh[6]), (0u64, 0u64)); c.r[6] = a; c.rh[6] = b; }
c.wr32((c.r[3] as u32).wrapping_add(0x0000002c), c.r[8] as u32);
uycpu::vu0::fmac::<0, 15>(c, 7, 5, c.vf[6]);
uycpu::vu0::fmac::<6, 15>(c, 8, 5, c.vf[6]);
uycpu::vu0::fmac::<6, 15>(c, 9, 7, c.vf[7]);
uycpu::vu0::fmac::<0, 4>(c, 32, 8, uycpu::vu0::bcast(c.vf[8][0]));
uycpu::vu0::fmac::<2, 4>(c, 32, 2, uycpu::vu0::bcast(c.vf[8][2]));
uycpu::vu0::fmac::<2, 4>(c, 10, 2, uycpu::vu0::bcast(c.vf[8][3]));
uycpu::vu0::fmac::<0, 8>(c, 32, 9, uycpu::vu0::bcast(c.vf[9][1]));
uycpu::vu0::fmac::<2, 8>(c, 32, 1, uycpu::vu0::bcast(c.vf[9][2]));
uycpu::vu0::fmac::<2, 8>(c, 9, 1, uycpu::vu0::bcast(c.vf[9][3]));
{ let (a, b) = { let v = c.vf[10]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; c.r[9] = a; c.rh[9] = b; }
let t = (c.r[9] as i64) >= 0;   if t { l = 0x0043c000; continue; }
uycpu::vu0::fmac::<1, 15>(c, 7, 5, c.vf[6]);
uycpu::vu0::fmac::<6, 15>(c, 9, 7, c.vf[7]);
uycpu::vu0::fmac::<0, 8>(c, 32, 9, uycpu::vu0::bcast(c.vf[9][1]));
uycpu::vu0::fmac::<2, 8>(c, 32, 1, uycpu::vu0::bcast(c.vf[9][2]));
uycpu::vu0::fmac::<2, 8>(c, 9, 1, uycpu::vu0::bcast(c.vf[9][3]));
l = 0x0043c000; continue; }
0x0043c000 => {
uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[9][0]);
uycpu::vu0::fmac::<6, 15>(c, 7, 7, uycpu::vu0::bcast(c.vi[22]));
let t = c.r[1] != c.r[2];  { let v = c.vf[7]; c.wr128((c.r[3] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043bf58; continue; }
 l = 0x0043be88; continue;
}
_ => panic!("the animation unit: no label {l:08x} in 0x0043B7D0"), } }
}

#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn n0043aaf8(c: &mut Cpu, k: F) {
let mut l: u32 = 0x0043aaf8;
loop { match l {
0x0043aaf8 => {
{ let (a, b) = uycpu::mmi::pcpyld((c.r[31], c.rh[31]), (0u64, 0u64)); c.r[31] = a; c.rh[31] = b; }
c.r[15] = c.rd32((c.r[4] as u32).wrapping_add(0x00000024)) as i32 as i64 as u64;
c.r[24] = (((c.r[4] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
c.r[25] = 0x0000000070000000;
c.r[12] = c.rd32((c.r[24] as u32).wrapping_add(0x00000058)) as i32 as i64 as u64;
c.r[8] = 0x0000000010000000;
c.r[13] = c.rd32((c.r[24] as u32).wrapping_add(0x0000005c)) as i32 as i64 as u64;
c.r[8] = c.r[8] | 0xd400;
c.r[11] = c.rd32((c.r[15] as u32).wrapping_add(0x00000018)) as i32 as i64 as u64;
uycpu::vu0::fmac::<0, 14>(c, 5, 0, uycpu::vu0::bcast(c.vf[0][3]));
c.r[10] = c.rd8((c.r[15] as u32).wrapping_add(0x00000008)) as u64;
c.r[1] = 0u64 | 0x2000;
c.wr32((c.r[8] as u32).wrapping_add(0x00000080), c.r[1] as u32);
{ let (a, b) = uycpu::mmi::pcpyld((c.r[5], c.rh[5]), (c.r[25], c.rh[25])); c.r[25] = a; c.rh[25] = b; }
c.wr32((c.r[8] as u32).wrapping_add(0x00000010), c.r[11] as u32);
c.r[15] = (((c.r[10] as u32) << 4) as i32 as i64 as u64);
c.wr32((c.r[8] as u32).wrapping_add(0x00000020), c.r[10] as u32);
c.r[15] = (((c.r[15] as u32).wrapping_add((c.r[25] as u32))) as i32 as i64 as u64);
c.wr64((c.r[15] as u32).wrapping_add(0x00001ff8), 0u64);
c.r[1] = 0u64 | 0x100;
c.wr32((c.r[8] as u32), c.r[1] as u32);
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
c.r[4] = c.rd16((c.r[12] as u32).wrapping_add(0xfffffff6)) as i16 as i64 as u64;
c.r[13] = (((c.r[13] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
c.r[9] = c.rd32((c.r[24] as u32).wrapping_add(0x00000044)) as i32 as i64 as u64;
uycpu::vu0::fmac::<6, 1>(c, 5, 0, uycpu::vu0::bcast(c.vf[0][0]));
c.r[14] = (((c.r[4] as u32) << 4) as i32 as i64 as u64);
uycpu::vu0::fmac::<6, 15>(c, 1, 0, uycpu::vu0::bcast(c.vf[0][0]));
c.r[2] = c.rd16((c.r[12] as u32).wrapping_add(0xfffffffa)) as i16 as i64 as u64;
uycpu::vu0::fmac::<6, 15>(c, 2, 0, uycpu::vu0::bcast(c.vf[0][0]));
c.r[14] = (((c.r[14] as u32).wrapping_add((c.r[25] as u32))) as i32 as i64 as u64);
{ let v = c.vf[0]; uycpu::vu0::mov::<15>(c, 3, [v[1], v[2], v[3], v[0]]); }
c.r[3] = c.rd16((c.r[12] as u32).wrapping_add(0xfffffffe)) as i16 as i64 as u64;
uycpu::vu0::fmac::<0, 8>(c, 1, 1, uycpu::vu0::bcast(c.vf[0][3]));
c.wr128((c.r[14] as u32).wrapping_add(0x00002ff0), 0u64, 0u64);
uycpu::vu0::fmac::<0, 4>(c, 2, 2, uycpu::vu0::bcast(c.vf[0][3]));
c.r[5] = (((c.r[25] as u32).wrapping_add(0xffffff80)) as i32 as i64 as u64);
c.r[6] = (((c.r[10] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
l = 0x0043ab90; continue; }
0x0043ab90 => {
{ let v = c.vf[5]; c.wr128((c.r[5] as u32).wrapping_add(0x00000080), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[5]; c.wr128((c.r[5] as u32).wrapping_add(0x00000090), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[5] = (((c.r[5] as u32).wrapping_add(0x00000080)) as i32 as i64 as u64);
{ let v = c.vf[5]; c.wr128((c.r[5] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[6] = (((c.r[6] as u32).wrapping_add(0xfffffffe)) as i32 as i64 as u64);
let t = (c.r[6] as i64) > 0;  { let v = c.vf[5]; c.wr128((c.r[5] as u32).wrapping_add(0x00000050), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043ab90; continue; }
l = 0x0043abb0; continue; }
0x0043abb0 => {
c.r[1] = c.rd64((c.r[15] as u32).wrapping_add(0x00001ff8));
c.r[5] = (((c.r[25] as u32).wrapping_add(0x00003000)) as i32 as i64 as u64);
c.r[6] = 0u64 | 0x100;
let t = c.r[1] == 0u64;   if t { l = 0x0043abb0; continue; }
c.wr32((c.r[8] as u32).wrapping_add(0x00000080), c.r[5] as u32);
uycpu::vu0::fmac::<6, 1>(c, 9, 0, uycpu::vu0::bcast(c.vf[0][0]));
c.wr32((c.r[8] as u32).wrapping_add(0x00000010), c.r[12] as u32);
uycpu::vu0::fmac::<6, 1>(c, 10, 0, uycpu::vu0::bcast(c.vf[0][0]));
c.wr32((c.r[8] as u32).wrapping_add(0x00000020), c.r[4] as u32);
uycpu::vu0::fmac::<6, 1>(c, 11, 0, uycpu::vu0::bcast(c.vf[0][0]));
c.wr32((c.r[8] as u32), c.r[6] as u32);
uycpu::vu0::fmac::<6, 1>(c, 12, 0, uycpu::vu0::bcast(c.vf[0][0]));
c.r[5] = (((c.r[10] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
c.r[6] = (((c.r[25] as u32).wrapping_add(0x00002000)) as i32 as i64 as u64);
c.r[7] = (((c.r[25] as u32).wrapping_add(0xffffff00)) as i32 as i64 as u64);
l = 0x0043ac00; continue; }
0x0043ac00 => {
{ let (a, b) = c.rd128((c.r[6] as u32)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
c.r[7] = (((c.r[7] as u32).wrapping_add(0x00000100)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[6] as u32).wrapping_add(0x00000010)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
c.r[5] = (((c.r[5] as u32).wrapping_add(0xfffffffc)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[6] as u32).wrapping_add(0x00000020)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::fmac::<6, 14>(c, 9, 5, uycpu::vu0::bcast(c.vf[0][3]));
{ let (a, b) = c.rd128((c.r[6] as u32).wrapping_add(0x00000030)); c.vf[8] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::fmac::<6, 14>(c, 10, 6, uycpu::vu0::bcast(c.vf[0][3]));
{ let v = c.vf[5]; c.wr128((c.r[7] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
uycpu::vu0::fmac::<6, 14>(c, 11, 7, uycpu::vu0::bcast(c.vf[0][3]));
{ let v = c.vf[6]; c.wr128((c.r[7] as u32).wrapping_add(0x00000060), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
uycpu::vu0::fmac::<6, 14>(c, 12, 8, uycpu::vu0::bcast(c.vf[0][3]));
{ let v = c.vf[7]; c.wr128((c.r[7] as u32).wrapping_add(0x000000a0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[8]; c.wr128((c.r[7] as u32).wrapping_add(0x000000e0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[9]; c.wr128((c.r[7] as u32).wrapping_add(0x00000030), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[10]; c.wr128((c.r[7] as u32).wrapping_add(0x00000070), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[11]; c.wr128((c.r[7] as u32).wrapping_add(0x000000b0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[6] = (((c.r[6] as u32).wrapping_add(0x00000040)) as i32 as i64 as u64);
let t = (c.r[5] as i64) > 0;  { let v = c.vf[12]; c.wr128((c.r[7] as u32).wrapping_add(0x000000f0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043ac00; continue; }
let t = c.r[9] == 0u64;   if t { l = 0x0043b000; continue; }
c.r[7] = c.rd8((c.r[24] as u32).wrapping_add(0x00000042)) as u64;
c.r[8] = c.rd8((c.r[24] as u32).wrapping_add(0x00000043)) as u64;
c.r[5] = c.rd8((c.r[24] as u32).wrapping_add(0x00000040)) as u64;
c.r[6] = c.rd8((c.r[24] as u32).wrapping_add(0x00000041)) as u64;
c.r[31] = (((c.r[7] as u32).wrapping_sub((c.r[8] as u32))) as i32 as i64 as u64);
c.r[7] = c.rd16((c.r[13] as u32).wrapping_add(0xfffffff6)) as i16 as i64 as u64;
c.r[5] = (((c.r[5] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.vf[4] = [c.r[9] as u32, (c.r[9] >> 32) as u32, c.rh[9] as u32, (c.rh[9] >> 32) as u32];
c.r[5] = (((c.r[5] as u32).wrapping_sub((c.r[6] as u32))) as i32 as i64 as u64);
c.r[31] = c.r[31] | c.r[5];
c.r[15] = (((c.r[7] as u32) << 4) as i32 as i64 as u64);
c.r[5] = c.rd16((c.r[13] as u32).wrapping_add(0xfffffffa)) as i16 as i64 as u64;
c.r[15] = (((c.r[15] as u32).wrapping_add((c.r[25] as u32))) as i32 as i64 as u64);
c.r[6] = c.rd16((c.r[13] as u32).wrapping_add(0xfffffffe)) as i16 as i64 as u64;
c.wr128((c.r[15] as u32).wrapping_add(0x000035f0), 0u64, 0u64);
uycpu::vu0::fmac::<1, 1>(c, 4, 0, uycpu::vu0::bcast(c.vf[4][0]));
l = 0x0043aca0; continue; }
0x0043aca0 => {
c.r[1] = c.rd64((c.r[14] as u32).wrapping_add(0x00002ff0));
c.r[8] = 0x0000000010000000;
c.r[8] = c.r[8] | 0xd400;
let t = c.r[1] == 0u64;   if t { l = 0x0043aca0; continue; }
c.wr32((c.r[8] as u32).wrapping_add(0x00000010), c.r[13] as u32);
c.r[1] = (((c.r[25] as u32).wrapping_add(0x00003600)) as i32 as i64 as u64);
c.wr32((c.r[8] as u32).wrapping_add(0x00000080), c.r[1] as u32);
c.r[14] = (((c.r[25] as u32).wrapping_add(0x00003d00)) as i32 as i64 as u64);
c.wr32((c.r[8] as u32).wrapping_add(0x00000020), c.r[7] as u32);
c.r[1] = 0u64 | 0x100;
c.wr32((c.r[8] as u32), c.r[1] as u32);
c.r[13] = (((c.r[25] as u32).wrapping_add(0x00003c00)) as i32 as i64 as u64);
c.r[7] = (((c.r[10] as u32) << 3) as i32 as i64 as u64);
c.r[4] = (((c.r[25] as u32).wrapping_add(0x00003000)) as i32 as i64 as u64);
uycpu::vu0::fmac::<6, 15>(c, 5, 0, uycpu::vu0::bcast(c.vf[0][0]));
c.r[4] = (((c.r[4] as u32).wrapping_add((c.r[7] as u32))) as i32 as i64 as u64);
let t = c.r[2] == 0u64;  c.r[7] = c.rd64((c.r[4] as u32));
 if t { l = 0x0043ad58; continue; }
c.r[8] = c.rd8((c.r[4] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[9] = a; c.rh[9] = b; }
l = 0x0043ad00; continue; }
0x0043ad00 => {
{ let (a, b) = uycpu::mmi::psrlw((c.r[9], c.rh[9]), 13); c.r[9] = a; c.rh[9] = b; }
l = 0x0043ad08; continue; }
0x0043ad08 => {
c.vf[6] = [c.r[9] as u32, (c.r[9] >> 32) as u32, c.rh[9] as u32, (c.rh[9] >> 32) as u32];
c.r[12] = c.r[7] >> 62;
c.r[11] = (((c.r[8] as u32) << 6) as i32 as i64 as u64);
c.r[7] = c.rd64((c.r[4] as u32).wrapping_add(0x00000008));
uycpu::vu0::itof::<3, 14>(c, 5, 6);
c.r[11] = (((c.r[11] as u32).wrapping_add((c.r[25] as u32))) as i32 as i64 as u64);
c.r[4] = (((c.r[4] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
c.r[2] = (((c.r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
c.r[8] = c.rd8((c.r[4] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[9] = a; c.rh[9] = b; }
{ let (a, b) = uycpu::mmi::psrlw((c.r[9], c.rh[9]), 13); c.r[9] = a; c.rh[9] = b; }
c.wr32((c.r[13] as u32), c.r[11] as u32);
c.r[13] = (((c.r[13] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
c.r[12] = (((c.r[12] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
c.wr32((c.r[11] as u32).wrapping_add(0x0000001c), c.r[12] as u32);
{ let v = c.vf[5]; c.wr128((c.r[11] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[1] = ((0u32.wrapping_add(0xfffffffc)) as i32 as i64 as u64);
let t = (c.r[2] as i64) > 0;  c.wr32((c.r[11] as u32).wrapping_add(0x0000000c), c.r[1] as u32);
 if t { l = 0x0043ad08; continue; }
l = 0x0043ad58; continue; }
0x0043ad58 => {
let t = c.r[3] == 0u64;  c.r[7] = c.rd64((c.r[4] as u32));
 if t { l = 0x0043adb8; continue; }
c.r[8] = c.rd8((c.r[4] as u32).wrapping_add(0x00000006)) as u64;
uycpu::vu0::mov::<1>(c, 5, c.vf[0]);
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
l = 0x0043ad70; continue; }
0x0043ad70 => {
c.vf[6] = [c.r[7] as u32, (c.r[7] >> 32) as u32, c.rh[7] as u32, (c.rh[7] >> 32) as u32];
c.r[12] = (((c.r[8] as u32) << 6) as i32 as i64 as u64);
c.r[12] = (((c.r[12] as u32).wrapping_add((c.r[25] as u32))) as i32 as i64 as u64);
c.r[7] = c.rd64((c.r[4] as u32).wrapping_add(0x00000008));
uycpu::vu0::itof::<0, 14>(c, 5, 6);
c.r[4] = (((c.r[4] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
c.r[8] = c.rd8((c.r[4] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
c.wr32((c.r[14] as u32), c.r[12] as u32);
c.r[14] = (((c.r[14] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
c.r[11] = c.rd32((c.r[12] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
{ let v = c.vf[5]; c.wr128((c.r[12] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[3] = (((c.r[3] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
c.wr32((c.r[12] as u32).wrapping_add(0x0000002c), c.r[11] as u32);
c.r[1] = ((0u32.wrapping_add(0xfffffffc)) as i32 as i64 as u64);
let t = (c.r[3] as i64) > 0;  c.wr32((c.r[12] as u32).wrapping_add(0x0000003c), c.r[1] as u32);
 if t { l = 0x0043ad70; continue; }
l = 0x0043adb8; continue; }
0x0043adb8 => {
c.r[1] = c.rd64((c.r[15] as u32).wrapping_add(0x000035f0));
c.r[7] = (((c.r[10] as u32) << 3) as i32 as i64 as u64);
c.r[4] = (((c.r[25] as u32).wrapping_add(0x00003600)) as i32 as i64 as u64);
let t = c.r[1] == 0u64;  c.r[4] = (((c.r[4] as u32).wrapping_add((c.r[7] as u32))) as i32 as i64 as u64);
 if t { l = 0x0043adb8; continue; }
uycpu::vu0::fmac::<6, 15>(c, 5, 0, uycpu::vu0::bcast(c.vf[0][0]));
let t = c.r[5] == 0u64;  c.r[7] = c.rd64((c.r[4] as u32));
 if t { l = 0x0043ae48; continue; }
c.r[8] = c.rd8((c.r[4] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[9] = a; c.rh[9] = b; }
{ let (a, b) = uycpu::mmi::psrlw((c.r[9], c.rh[9]), 13); c.r[9] = a; c.rh[9] = b; }
l = 0x0043adf8; continue; }
0x0043adf8 => {
c.vf[6] = [c.r[9] as u32, (c.r[9] >> 32) as u32, c.rh[9] as u32, (c.rh[9] >> 32) as u32];
c.r[12] = c.r[7] >> 62;
c.r[11] = (((c.r[8] as u32) << 6) as i32 as i64 as u64);
c.r[7] = c.rd64((c.r[4] as u32).wrapping_add(0x00000008));
uycpu::vu0::itof::<3, 14>(c, 5, 6);
c.r[11] = (((c.r[11] as u32).wrapping_add((c.r[25] as u32))) as i32 as i64 as u64);
c.r[8] = c.rd8((c.r[4] as u32).wrapping_add(0x0000000e)) as u64;
c.r[4] = (((c.r[4] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[9] = a; c.rh[9] = b; }
c.wr32((c.r[13] as u32), c.r[11] as u32);
{ let (a, b) = uycpu::mmi::psrlw((c.r[9], c.rh[9]), 13); c.r[9] = a; c.rh[9] = b; }
c.r[1] = c.rd32((c.r[11] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
c.r[5] = (((c.r[5] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
c.r[12] = (((c.r[12] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
{ let v = c.vf[5]; c.wr128((c.r[11] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.wr32((c.r[11] as u32).wrapping_add(0x0000001c), c.r[12] as u32);
c.r[13] = (((c.r[13] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
let t = c.r[5] != 0u64;  c.r[13] = (((c.r[13] as u32).wrapping_add((c.r[1] as u32))) as i32 as i64 as u64);
 if t { l = 0x0043adf8; continue; }
l = 0x0043ae48; continue; }
0x0043ae48 => {
let t = c.r[6] == 0u64;  c.r[7] = c.rd64((c.r[4] as u32));
 if t { l = 0x0043aea0; continue; }
c.r[8] = c.rd8((c.r[4] as u32).wrapping_add(0x00000006)) as u64;
uycpu::vu0::mov::<1>(c, 5, c.vf[0]);
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
l = 0x0043ae60; continue; }
0x0043ae60 => {
c.vf[6] = [c.r[7] as u32, (c.r[7] >> 32) as u32, c.rh[7] as u32, (c.rh[7] >> 32) as u32];
c.r[12] = (((c.r[8] as u32) << 6) as i32 as i64 as u64);
c.r[12] = (((c.r[12] as u32).wrapping_add((c.r[25] as u32))) as i32 as i64 as u64);
c.r[7] = c.rd64((c.r[4] as u32).wrapping_add(0x00000008));
uycpu::vu0::itof::<0, 14>(c, 5, 6);
c.r[4] = (((c.r[4] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
c.r[8] = c.rd8((c.r[4] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
c.wr32((c.r[14] as u32), c.r[12] as u32);
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
c.r[1] = c.rd32((c.r[12] as u32).wrapping_add(0x0000003c)) as i32 as i64 as u64;
c.r[14] = (((c.r[14] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
{ let v = c.vf[5]; c.wr128((c.r[12] as u32).wrapping_add(0x00000030), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[6] = (((c.r[6] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
let t = (c.r[6] as i64) > 0;  c.r[14] = (((c.r[14] as u32).wrapping_add((c.r[1] as u32))) as i32 as i64 as u64);
 if t { l = 0x0043ae60; continue; }
l = 0x0043aea0; continue; }
0x0043aea0 => {
c.r[1] = (((c.r[25] as u32).wrapping_add(0x00003c00)) as i32 as i64 as u64);
c.r[2] = c.rd32((c.r[1] as u32)) as i32 as i64 as u64;
c.r[3] = (((c.r[25] as u32).wrapping_add(0x00001e00)) as i32 as i64 as u64);
let t = c.r[1] == c.r[13];  c.wr32((c.r[13] as u32), c.r[3] as u32);
 if t { l = 0x0043aef0; continue; }
{ let (a, b) = c.rd128((c.r[2] as u32)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[2] as u32).wrapping_add(0x00000010)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
l = 0x0043aec0; continue; }
0x0043aec0 => {
uycpu::vu0::fmac::<6, 14>(c, 32, 5, uycpu::vu0::bcast(c.vf[4][3]));
c.r[3] = (((c.r[2] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
uycpu::vu0::fmac::<2, 14>(c, 7, 6, uycpu::vu0::bcast(c.vf[4][0]));
c.r[4] = c.rd32((c.r[2] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
c.r[2] = c.rd32((c.r[1] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[2] as u32)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[2] as u32).wrapping_add(0x00000010)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let v = c.vf[7]; c.wr128((c.r[3] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
let t = c.r[1] != c.r[13];  c.wr32((c.r[3] as u32).wrapping_add(0x0000001c), c.r[4] as u32);
 if t { l = 0x0043aec0; continue; }
l = 0x0043aef0; continue; }
0x0043aef0 => {
c.r[1] = (((c.r[25] as u32).wrapping_add(0x00003d00)) as i32 as i64 as u64);
c.r[2] = c.rd32((c.r[1] as u32)) as i32 as i64 as u64;
c.r[3] = (((c.r[25] as u32).wrapping_add(0x00001e00)) as i32 as i64 as u64);
let t = c.r[1] == c.r[14];  c.wr32((c.r[14] as u32), c.r[3] as u32);
 if t { l = 0x0043af40; continue; }
{ let (a, b) = c.rd128((c.r[2] as u32).wrapping_add(0x00000020)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[2] as u32).wrapping_add(0x00000030)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
l = 0x0043af10; continue; }
0x0043af10 => {
uycpu::vu0::fmac::<6, 14>(c, 32, 5, uycpu::vu0::bcast(c.vf[4][3]));
c.r[3] = (((c.r[2] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
uycpu::vu0::fmac::<2, 14>(c, 7, 6, uycpu::vu0::bcast(c.vf[4][0]));
c.r[4] = c.rd32((c.r[2] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
c.r[2] = c.rd32((c.r[1] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[2] as u32).wrapping_add(0x00000020)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[2] as u32).wrapping_add(0x00000030)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let v = c.vf[7]; c.wr128((c.r[3] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
let t = c.r[1] != c.r[14];  c.wr32((c.r[3] as u32).wrapping_add(0x0000002c), c.r[4] as u32);
 if t { l = 0x0043af10; continue; }
l = 0x0043af40; continue; }
0x0043af40 => {
c.r[1] = (((c.r[25] as u32).wrapping_add(0x00003000)) as i32 as i64 as u64);
c.r[2] = (((c.r[25] as u32).wrapping_add(0x00003600)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[1] as u32)); c.r[6] = a; c.rh[6] = b; }
c.r[3] = (((c.r[10] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[2] as u32)); c.r[8] = a; c.rh[8] = b; }
c.r[4] = (((c.r[25] as u32).wrapping_add(0xffffff80)) as i32 as i64 as u64);
let t = c.r[31] != 0u64;  { let (a, b) = uycpu::mmi::pextlh((c.r[6], c.rh[6]), (0u64, 0u64)); c.r[5] = a; c.rh[5] = b; }
 if t { l = 0x0043b3b8; continue; }
{ let (a, b) = uycpu::mmi::psraw((c.r[5], c.rh[5]), 16); c.r[5] = a; c.rh[5] = b; }
c.vf[5] = [c.r[5] as u32, (c.r[5] >> 32) as u32, c.rh[5] as u32, (c.rh[5] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((c.r[6], c.rh[6]), (0u64, 0u64)); c.r[6] = a; c.rh[6] = b; }
l = 0x0043af70; continue; }
0x0043af70 => {
{ let (a, b) = uycpu::mmi::psraw((c.r[6], c.rh[6]), 16); c.r[6] = a; c.rh[6] = b; }
c.vf[6] = [c.r[6] as u32, (c.r[6] >> 32) as u32, c.rh[6] as u32, (c.rh[6] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextlh((c.r[8], c.rh[8]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
c.vf[7] = [c.r[7] as u32, (c.r[7] >> 32) as u32, c.rh[7] as u32, (c.rh[7] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((c.r[8], c.rh[8]), (0u64, 0u64)); c.r[8] = a; c.rh[8] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[8], c.rh[8]), 16); c.r[8] = a; c.rh[8] = b; }
c.vf[8] = [c.r[8] as u32, (c.r[8] >> 32) as u32, c.rh[8] as u32, (c.rh[8] >> 32) as u32];
c.r[4] = (((c.r[4] as u32).wrapping_add(0x00000080)) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 15>(c, 5, 5);
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 15>(c, 6, 6);
c.r[2] = (((c.r[2] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 15>(c, 7, 7);
uycpu::vu0::itof::<3, 15>(c, 8, 8);
uycpu::vu0::fmac::<6, 15>(c, 32, 5, uycpu::vu0::bcast(c.vf[4][3]));
uycpu::vu0::fmac::<2, 15>(c, 9, 7, uycpu::vu0::bcast(c.vf[4][0]));
uycpu::vu0::fmac::<6, 15>(c, 32, 6, uycpu::vu0::bcast(c.vf[4][3]));
{ let (a, b) = c.rd128((c.r[1] as u32)); c.r[6] = a; c.rh[6] = b; }
uycpu::vu0::fmac::<2, 15>(c, 10, 8, uycpu::vu0::bcast(c.vf[4][0]));
{ let (a, b) = c.rd128((c.r[2] as u32)); c.r[8] = a; c.rh[8] = b; }
{ let (a, b) = uycpu::mmi::pextlh((c.r[6], c.rh[6]), (0u64, 0u64)); c.r[5] = a; c.rh[5] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[5], c.rh[5]), 16); c.r[5] = a; c.rh[5] = b; }
c.vf[5] = [c.r[5] as u32, (c.r[5] >> 32) as u32, c.rh[5] as u32, (c.rh[5] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((c.r[6], c.rh[6]), (0u64, 0u64)); c.r[6] = a; c.rh[6] = b; }
{ let v = c.vf[9]; c.wr128((c.r[4] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[3] = (((c.r[3] as u32).wrapping_add(0xfffffffe)) as i32 as i64 as u64);
let t = (c.r[3] as i64) > 0;  { let v = c.vf[10]; c.wr128((c.r[4] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043af70; continue; }
 l = 0x0043b160; continue;
}
0x0043b000 => {
c.r[1] = c.rd64((c.r[14] as u32).wrapping_add(0x00002ff0));
c.r[5] = (((c.r[10] as u32) << 3) as i32 as i64 as u64);
c.r[12] = (((c.r[25] as u32).wrapping_add(0x00003000)) as i32 as i64 as u64);
uycpu::vu0::fmac::<6, 15>(c, 5, 0, uycpu::vu0::bcast(c.vf[0][0]));
let t = c.r[1] == 0u64;  c.r[12] = (((c.r[12] as u32).wrapping_add((c.r[5] as u32))) as i32 as i64 as u64);
 if t { l = 0x0043b000; continue; }
let t = c.r[2] == 0u64;  c.r[5] = c.rd64((c.r[12] as u32));
 if t { l = 0x0043b078; continue; }
c.r[6] = c.rd8((c.r[12] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((c.r[5], c.rh[5]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
{ let (a, b) = uycpu::mmi::psrlw((c.r[7], c.rh[7]), 13); c.r[7] = a; c.rh[7] = b; }
l = 0x0043b038; continue; }
0x0043b038 => {
c.vf[6] = [c.r[7] as u32, (c.r[7] >> 32) as u32, c.rh[7] as u32, (c.rh[7] >> 32) as u32];
c.r[9] = c.r[5] >> 62;
c.r[8] = (((c.r[6] as u32) << 6) as i32 as i64 as u64);
c.r[5] = c.rd64((c.r[12] as u32).wrapping_add(0x00000008));
uycpu::vu0::itof::<3, 14>(c, 5, 6);
c.r[8] = (((c.r[8] as u32).wrapping_add((c.r[25] as u32))) as i32 as i64 as u64);
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
c.r[2] = (((c.r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
c.r[6] = c.rd8((c.r[12] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((c.r[5], c.rh[5]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
c.r[9] = (((c.r[9] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::psrlw((c.r[7], c.rh[7]), 13); c.r[7] = a; c.rh[7] = b; }
{ let v = c.vf[5]; c.wr128((c.r[8] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
let t = (c.r[2] as i64) > 0;  c.wr32((c.r[8] as u32).wrapping_add(0x0000001c), c.r[9] as u32);
 if t { l = 0x0043b038; continue; }
l = 0x0043b078; continue; }
0x0043b078 => {
let t = c.r[3] == 0u64;  c.r[5] = c.rd64((c.r[12] as u32));
 if t { l = 0x0043b0c8; continue; }
c.r[6] = c.rd8((c.r[12] as u32).wrapping_add(0x00000006)) as u64;
uycpu::vu0::mov::<1>(c, 5, c.vf[0]);
{ let (a, b) = uycpu::mmi::pextlh((c.r[5], c.rh[5]), (0u64, 0u64)); c.r[5] = a; c.rh[5] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[5], c.rh[5]), 16); c.r[5] = a; c.rh[5] = b; }
l = 0x0043b090; continue; }
0x0043b090 => {
c.vf[6] = [c.r[5] as u32, (c.r[5] >> 32) as u32, c.rh[5] as u32, (c.rh[5] >> 32) as u32];
c.r[8] = (((c.r[6] as u32) << 6) as i32 as i64 as u64);
c.r[8] = (((c.r[8] as u32).wrapping_add((c.r[25] as u32))) as i32 as i64 as u64);
c.r[5] = c.rd64((c.r[12] as u32).wrapping_add(0x00000008));
uycpu::vu0::itof::<0, 14>(c, 5, 6);
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
c.r[6] = c.rd8((c.r[12] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((c.r[5], c.rh[5]), (0u64, 0u64)); c.r[5] = a; c.rh[5] = b; }
c.r[7] = c.rd32((c.r[8] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
{ let (a, b) = uycpu::mmi::psraw((c.r[5], c.rh[5]), 16); c.r[5] = a; c.rh[5] = b; }
{ let v = c.vf[5]; c.wr128((c.r[8] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[3] = (((c.r[3] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
let t = (c.r[3] as i64) > 0;  c.wr32((c.r[8] as u32).wrapping_add(0x0000002c), c.r[7] as u32);
 if t { l = 0x0043b090; continue; }
l = 0x0043b0c8; continue; }
0x0043b0c8 => {
c.r[1] = (((c.r[25] as u32).wrapping_add(0x00003000)) as i32 as i64 as u64);
c.r[2] = (((c.r[25] as u32).wrapping_add(0xffffff00)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[1] as u32)); c.r[6] = a; c.rh[6] = b; }
c.r[3] = (((c.r[10] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[1] as u32).wrapping_add(0x00000010)); c.r[8] = a; c.rh[8] = b; }
l = 0x0043b0e0; continue; }
0x0043b0e0 => {
{ let (a, b) = uycpu::mmi::pextlh((c.r[6], c.rh[6]), (0u64, 0u64)); c.r[5] = a; c.rh[5] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[5], c.rh[5]), 16); c.r[5] = a; c.rh[5] = b; }
c.vf[5] = [c.r[5] as u32, (c.r[5] >> 32) as u32, c.rh[5] as u32, (c.rh[5] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((c.r[6], c.rh[6]), (0u64, 0u64)); c.r[6] = a; c.rh[6] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[6], c.rh[6]), 16); c.r[6] = a; c.rh[6] = b; }
c.vf[6] = [c.r[6] as u32, (c.r[6] >> 32) as u32, c.rh[6] as u32, (c.rh[6] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextlh((c.r[8], c.rh[8]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
c.vf[7] = [c.r[7] as u32, (c.r[7] >> 32) as u32, c.rh[7] as u32, (c.rh[7] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((c.r[8], c.rh[8]), (0u64, 0u64)); c.r[8] = a; c.rh[8] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[8], c.rh[8]), 16); c.r[8] = a; c.rh[8] = b; }
c.vf[8] = [c.r[8] as u32, (c.r[8] >> 32) as u32, c.rh[8] as u32, (c.rh[8] >> 32) as u32];
uycpu::vu0::itof::<3, 15>(c, 5, 5);
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 15>(c, 6, 6);
c.r[2] = (((c.r[2] as u32).wrapping_add(0x00000100)) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 15>(c, 7, 7);
{ let (a, b) = c.rd128((c.r[1] as u32)); c.r[6] = a; c.rh[6] = b; }
uycpu::vu0::itof::<3, 15>(c, 8, 8);
{ let (a, b) = c.rd128((c.r[1] as u32).wrapping_add(0x00000010)); c.r[8] = a; c.rh[8] = b; }
{ let v = c.vf[5]; c.wr128((c.r[2] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[6]; c.wr128((c.r[2] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[7]; c.wr128((c.r[2] as u32).wrapping_add(0x00000080), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[3] = (((c.r[3] as u32).wrapping_add(0xfffffffc)) as i32 as i64 as u64);
let t = (c.r[3] as i64) > 0;  { let v = c.vf[8]; c.wr128((c.r[2] as u32).wrapping_add(0x000000c0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043b0e0; continue; }
l = 0x0043b160; continue; }
0x0043b160 => {
c.r[15] = c.rd64((c.r[24] as u32).wrapping_add(0x00000050));
{ let (a, b) = uycpu::mmi::pcpyud((c.r[25], c.rh[25]), (0u64, 0u64)); c.r[24] = a; c.rh[24] = b; }
c.r[1] = c.r[24] & 0x200;
let t = c.r[15] != 0u64;  c.r[14] = (((c.r[15] as u32) >> 0) as i32 as i64 as u64);
 if t { l = 0x0043b540; continue; }
l = 0x0043b178; continue; }
0x0043b178 => {
c.r[12] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
c.r[11] = (((c.r[25] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
c.r[9] = (((c.r[25] as u32).wrapping_add(0x00002000)) as i32 as i64 as u64);
c.r[13] = (((c.r[25] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
c.r[14] = (((c.r[25] as u32).wrapping_add(0x00002800)) as i32 as i64 as u64);
c.r[15] = (((c.r[25] as u32).wrapping_add(0x00002c00)) as i32 as i64 as u64);
c.r[7] = ((0u32.wrapping_add(0xffff8000)) as i32 as i64 as u64);
c.r[8] = ((0u32.wrapping_add(0x00007fff)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pextlw((c.r[7], c.rh[7]), (c.r[7], c.rh[7])); c.r[7] = a; c.rh[7] = b; }
{ let (a, b) = c.rd128((c.r[11] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = uycpu::mmi::pextlw((c.r[8], c.rh[8]), (c.r[8], c.rh[8])); c.r[8] = a; c.rh[8] = b; }
{ let (a, b) = c.rd128((c.r[11] as u32).wrapping_add(0x00000010)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = uycpu::mmi::pextlw((c.r[7], c.rh[7]), (c.r[7], c.rh[7])); c.r[7] = a; c.rh[7] = b; }
{ let (a, b) = c.rd128((c.r[11] as u32).wrapping_add(0x00000020)); c.vf[3] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = uycpu::mmi::pextlw((c.r[8], c.rh[8]), (c.r[8], c.rh[8])); c.r[8] = a; c.rh[8] = b; }
{ let (a, b) = c.rd128((c.r[9] as u32)); c.vf[4] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
c.r[5] = 0u64 | 0x1000;
{ let (a, b) = uycpu::mmi::pcpyh((c.r[5], c.rh[5])); c.r[5] = a; c.rh[5] = b; }
c.r[5] = c.r[5] << 16;
c.r[6] = 0u64 | 0xffff;
{ let (a, b) = uycpu::mmi::pextlw((c.r[6], c.rh[6]), (c.r[6], c.rh[6])); c.r[6] = a; c.rh[6] = b; }
{ let (a, b) = uycpu::mmi::pextlw((c.r[6], c.rh[6]), (c.r[6], c.rh[6])); c.r[6] = a; c.rh[6] = b; }
l = 0x0043b1d0; continue; }
0x0043b1d0 => {
uycpu::vu0::ftoi::<3, 15>(c, 1, 1);
uycpu::vu0::ftoi::<3, 15>(c, 2, 2);
uycpu::vu0::ftoi::<0, 15>(c, 3, 3);
uycpu::vu0::ftoi::<0, 15>(c, 4, 4);
{ let (a, b) = { let v = c.vf[1]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; c.r[1] = a; c.rh[1] = b; }
{ let (a, b) = { let v = c.vf[2]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; c.r[2] = a; c.rh[2] = b; }
{ let (a, b) = { let v = c.vf[3]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; c.r[3] = a; c.rh[3] = b; }
{ let (a, b) = uycpu::mmi::pmaxw((c.r[1], c.rh[1]), (c.r[7], c.rh[7])); c.r[1] = a; c.rh[1] = b; }
{ let (a, b) = { let v = c.vf[4]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; c.r[4] = a; c.rh[4] = b; }
{ let (a, b) = uycpu::mmi::pminw((c.r[1], c.rh[1]), (c.r[8], c.rh[8])); c.r[1] = a; c.rh[1] = b; }
{ let (a, b) = uycpu::mmi::ppach((0u64, 0u64), (c.r[1], c.rh[1])); c.r[1] = a; c.rh[1] = b; }
{ let (a, b) = c.rd128((c.r[11] as u32).wrapping_add(0x00000040)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = uycpu::mmi::psrlw((c.r[2], c.rh[2]), 3); c.r[2] = a; c.rh[2] = b; }
{ let (a, b) = c.rd128((c.r[11] as u32).wrapping_add(0x00000050)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = uycpu::mmi::pmaxw((c.r[2], c.rh[2]), (0u64, 0u64)); c.r[2] = a; c.rh[2] = b; }
{ let (a, b) = c.rd128((c.r[11] as u32).wrapping_add(0x00000060)); c.vf[3] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = uycpu::mmi::pminw((c.r[2], c.rh[2]), (c.r[6], c.rh[6])); c.r[2] = a; c.rh[2] = b; }
{ let (a, b) = c.rd128((c.r[9] as u32).wrapping_add(0x00000010)); c.vf[4] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = uycpu::mmi::ppach((0u64, 0u64), (c.r[2], c.rh[2])); c.r[2] = a; c.rh[2] = b; }
c.wr64((c.r[13] as u32), c.r[1]);
c.r[1] = c.r[2] << 16;
c.r[13] = (((c.r[13] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
let t = c.r[1] == c.r[5];  { let (a, b) = uycpu::mmi::ppach((0u64, 0u64), (c.r[3], c.rh[3])); c.r[3] = a; c.rh[3] = b; }
 if t { l = 0x0043b250; continue; }
c.r[1] = c.rd32((c.r[11] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
c.wr64((c.r[14] as u32), c.r[2]);
c.r[1] = c.r[1] & 0x80;
c.wr8((c.r[14] as u32).wrapping_add(0x00000006), c.r[12] as u8);
c.r[1] = c.r[1] ^ 0x80;
c.wr8((c.r[14] as u32).wrapping_add(0x00000007), c.r[1] as u8);
c.r[14] = (((c.r[14] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
l = 0x0043b250; continue; }
0x0043b250 => {
{ let (a, b) = uycpu::mmi::ppach((0u64, 0u64), (c.r[4], c.rh[4])); c.r[4] = a; c.rh[4] = b; }
c.r[1] = c.r[3] << 16;
c.r[2] = c.r[4] << 16;
let t = c.r[1] == c.r[2];   if t { l = 0x0043b278; continue; }
c.wr64((c.r[15] as u32), c.r[3]);
c.wr16((c.r[15] as u32).wrapping_add(0x00000006), c.r[12] as u16);
c.r[15] = (((c.r[15] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
l = 0x0043b278; continue; }
0x0043b278 => {
c.r[9] = (((c.r[9] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
let t = c.r[12] != c.r[10];  c.r[11] = (((c.r[11] as u32).wrapping_add(0x00000040)) as i32 as i64 as u64);
 if t { l = 0x0043b1d0; continue; }
c.r[11] = (((c.r[25] as u32).wrapping_add(0x00002800)) as i32 as i64 as u64);
c.r[12] = (((c.r[25] as u32).wrapping_add(0x00002c00)) as i32 as i64 as u64);
c.r[11] = (((c.r[14] as u32).wrapping_sub((c.r[11] as u32))) as i32 as i64 as u64);
c.r[12] = (((c.r[15] as u32).wrapping_sub((c.r[12] as u32))) as i32 as i64 as u64);
c.r[1] = (((c.r[10] as u32) << 3) as i32 as i64 as u64);
c.r[4] = (((c.r[11] as u32) >> 3) as i32 as i64 as u64);
c.r[2] = (((c.r[1] as u32).wrapping_add((c.r[11] as u32))) as i32 as i64 as u64);
c.r[5] = (((c.r[12] as u32) >> 3) as i32 as i64 as u64);
c.r[3] = (((c.r[2] as u32).wrapping_add((c.r[12] as u32))) as i32 as i64 as u64);
c.wr16((c.r[25] as u32).wrapping_add(0x00000008), c.r[1] as u16);
c.r[3] = (((c.r[3] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
c.wr16((c.r[25] as u32).wrapping_add(0x0000000c), c.r[2] as u16);
c.r[3] = (((c.r[3] as u32) >> 4) as i32 as i64 as u64);
c.wr64((c.r[25] as u32), 0u64);
c.wr16((c.r[25] as u32).wrapping_add(0x00000006), c.r[3] as u16);
c.r[3] = (((c.r[3] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.wr16((c.r[25] as u32).wrapping_add(0x0000000a), c.r[4] as u16);
c.wr16((c.r[25] as u32).wrapping_add(0x0000000e), c.r[5] as u16);
let t = c.r[4] == 0u64;  c.r[12] = (((c.r[25] as u32).wrapping_add(0x00002800)) as i32 as i64 as u64);
 if t { l = 0x0043b2f8; continue; }
l = 0x0043b2d8; continue; }
0x0043b2d8 => {
c.r[1] = c.rd64((c.r[12] as u32));
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
c.wr64((c.r[13] as u32), c.r[1]);
c.r[13] = (((c.r[13] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
let t = c.r[12] != c.r[14];   if t { l = 0x0043b2d8; continue; }
l = 0x0043b2f8; continue; }
0x0043b2f8 => {
let t = c.r[5] == 0u64;  c.r[12] = (((c.r[25] as u32).wrapping_add(0x00002c00)) as i32 as i64 as u64);
 if t { l = 0x0043b338; continue; }
l = 0x0043b300; continue; }
0x0043b300 => {
c.r[1] = c.rd64((c.r[12] as u32));
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
let t = c.r[1] == 0u64;  c.wr64((c.r[13] as u32), c.r[1]);
 if t { l = 0x0043b328; continue; }
let t = c.r[12] != c.r[15];  c.r[13] = (((c.r[13] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
 if t { l = 0x0043b300; continue; }
 l = 0x0043b338; continue;
}
0x0043b328 => {
c.r[1] = 0u64 | 0x1;
c.wr64((c.r[13] as u32), c.r[1]);
let t = c.r[12] != c.r[15];  c.r[13] = (((c.r[13] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
 if t { l = 0x0043b300; continue; }
l = 0x0043b338; continue; }
0x0043b338 => {
c.r[1] = ((0u32.wrapping_add(0x00000001)) as i32 as i64 as u64);
c.r[10] = 0x0000000010000000;
c.wr64((c.r[13] as u32), c.r[1]);
c.r[10] = c.r[10] | 0xd000;
c.r[24] = c.r[24] & 0xff;
c.r[1] = (((c.r[24] as u32) << 11) as i32 as i64 as u64);
c.wr32((c.r[10] as u32).wrapping_add(0x00000080), 0u64 as u32);
c.r[24] = sx(k.hi(c, 0x0043b354));
c.r[24] = (((c.r[24] as u32).wrapping_add(k.lo(c, 0x0043b358))) as i32 as i64 as u64);
c.wr32((c.r[10] as u32).wrapping_add(0x00000020), c.r[3] as u32);
c.r[24] = (((c.r[24] as u32).wrapping_add((c.r[1] as u32))) as i32 as i64 as u64);
c.wr32((c.r[10] as u32).wrapping_add(0x00000010), c.r[24] as u32);
c.r[1] = 0u64 | 0x100;
c.wr32((c.r[10] as u32).wrapping_add(0x00001010), c.r[1] as u32);
c.r[1] = ((0u32.wrapping_add(0x00000100)) as i32 as i64 as u64);
c.wr32((c.r[10] as u32), c.r[1] as u32);
l = 0x0043b388; continue; }
0x0043b388 => {
let t = !c.cpcond0();   if t { l = 0x0043b388; continue; }
{ let (a, b) = uycpu::mmi::pcpyud((c.r[31], c.rh[31]), (0u64, 0u64)); c.r[31] = a; c.rh[31] = b; }
let t = (c.r[31] as u32);  c.ret = t; return;
}
0x0043b3b8 => {
{ let (a, b) = uycpu::mmi::psraw((c.r[5], c.rh[5]), 16); c.r[5] = a; c.rh[5] = b; }
c.vf[5] = [c.r[5] as u32, (c.r[5] >> 32) as u32, c.rh[5] as u32, (c.rh[5] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((c.r[6], c.rh[6]), (0u64, 0u64)); c.r[6] = a; c.rh[6] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[6], c.rh[6]), 16); c.r[6] = a; c.rh[6] = b; }
c.vf[6] = [c.r[6] as u32, (c.r[6] >> 32) as u32, c.rh[6] as u32, (c.rh[6] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextlh((c.r[8], c.rh[8]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
c.vf[7] = [c.r[7] as u32, (c.r[7] >> 32) as u32, c.rh[7] as u32, (c.rh[7] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((c.r[8], c.rh[8]), (0u64, 0u64)); c.r[8] = a; c.rh[8] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[8], c.rh[8]), 16); c.r[8] = a; c.rh[8] = b; }
c.vf[8] = [c.r[8] as u32, (c.r[8] >> 32) as u32, c.rh[8] as u32, (c.rh[8] >> 32) as u32];
l = 0x0043b3f8; continue; }
0x0043b3f8 => {
uycpu::vu0::itof::<3, 15>(c, 5, 5);
uycpu::vu0::itof::<3, 15>(c, 7, 7);
uycpu::vu0::itof::<3, 15>(c, 6, 6);
uycpu::vu0::itof::<3, 15>(c, 8, 8);
uycpu::vu0::fmac::<6, 15>(c, 5, 5, uycpu::vu0::bcast(c.vf[4][3]));
uycpu::vu0::fmac::<6, 15>(c, 7, 7, uycpu::vu0::bcast(c.vf[4][0]));
uycpu::vu0::fmac::<6, 15>(c, 6, 6, uycpu::vu0::bcast(c.vf[4][3]));
uycpu::vu0::fmac::<6, 15>(c, 8, 8, uycpu::vu0::bcast(c.vf[4][0]));
uycpu::vu0::fmac::<0, 15>(c, 9, 5, c.vf[7]);
uycpu::vu0::fmac::<6, 15>(c, 10, 5, c.vf[7]);
uycpu::vu0::fmac::<0, 15>(c, 11, 6, c.vf[8]);
uycpu::vu0::fmac::<6, 15>(c, 12, 6, c.vf[8]);
uycpu::vu0::fmac::<6, 15>(c, 13, 9, c.vf[9]);
uycpu::vu0::fmac::<0, 4>(c, 32, 10, uycpu::vu0::bcast(c.vf[10][0]));
uycpu::vu0::fmac::<2, 4>(c, 32, 2, uycpu::vu0::bcast(c.vf[10][2]));
uycpu::vu0::fmac::<2, 4>(c, 14, 2, uycpu::vu0::bcast(c.vf[10][3]));
uycpu::vu0::fmac::<0, 8>(c, 32, 13, uycpu::vu0::bcast(c.vf[13][1]));
uycpu::vu0::fmac::<2, 8>(c, 32, 1, uycpu::vu0::bcast(c.vf[13][2]));
uycpu::vu0::fmac::<2, 8>(c, 13, 1, uycpu::vu0::bcast(c.vf[13][3]));
{ let (a, b) = { let v = c.vf[14]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; c.r[5] = a; c.rh[5] = b; }
let t = (c.r[5] as i64) >= 0;   if t { l = 0x0043b468; continue; }
uycpu::vu0::fmac::<1, 15>(c, 9, 5, c.vf[7]);
uycpu::vu0::fmac::<6, 15>(c, 13, 9, c.vf[9]);
uycpu::vu0::fmac::<0, 8>(c, 32, 13, uycpu::vu0::bcast(c.vf[13][1]));
uycpu::vu0::fmac::<2, 8>(c, 32, 1, uycpu::vu0::bcast(c.vf[13][2]));
uycpu::vu0::fmac::<2, 8>(c, 13, 1, uycpu::vu0::bcast(c.vf[13][3]));
l = 0x0043b468; continue; }
0x0043b468 => {
uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[13][0]);
uycpu::vu0::fmac::<6, 15>(c, 15, 11, c.vf[11]);
uycpu::vu0::fmac::<0, 4>(c, 32, 12, uycpu::vu0::bcast(c.vf[12][0]));
uycpu::vu0::fmac::<2, 4>(c, 32, 2, uycpu::vu0::bcast(c.vf[12][2]));
uycpu::vu0::fmac::<2, 4>(c, 16, 2, uycpu::vu0::bcast(c.vf[12][3]));
uycpu::vu0::fmac::<0, 8>(c, 32, 15, uycpu::vu0::bcast(c.vf[15][1]));
uycpu::vu0::fmac::<2, 8>(c, 32, 1, uycpu::vu0::bcast(c.vf[15][2]));
uycpu::vu0::fmac::<2, 8>(c, 15, 1, uycpu::vu0::bcast(c.vf[15][3]));
{ let (a, b) = { let v = c.vf[16]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; c.r[5] = a; c.rh[5] = b; }
let t = (c.r[5] as i64) >= 0;   if t { l = 0x0043b4b8; continue; }
uycpu::vu0::fmac::<1, 15>(c, 11, 6, c.vf[8]);
uycpu::vu0::fmac::<6, 15>(c, 15, 11, c.vf[11]);
uycpu::vu0::fmac::<0, 8>(c, 32, 15, uycpu::vu0::bcast(c.vf[15][1]));
uycpu::vu0::fmac::<2, 8>(c, 32, 1, uycpu::vu0::bcast(c.vf[15][2]));
uycpu::vu0::fmac::<2, 8>(c, 15, 1, uycpu::vu0::bcast(c.vf[15][3]));
l = 0x0043b4b8; continue; }
0x0043b4b8 => {
uycpu::vu0::fmac::<6, 15>(c, 9, 9, uycpu::vu0::bcast(c.vi[22]));
uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[15][0]);
{ let (a, b) = c.rd128((c.r[1] as u32).wrapping_add(0x00000010)); c.r[6] = a; c.rh[6] = b; }
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[2] as u32).wrapping_add(0x00000010)); c.r[8] = a; c.rh[8] = b; }
c.r[2] = (((c.r[2] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pextlh((c.r[6], c.rh[6]), (0u64, 0u64)); c.r[5] = a; c.rh[5] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[5], c.rh[5]), 16); c.r[5] = a; c.rh[5] = b; }
c.vf[5] = [c.r[5] as u32, (c.r[5] >> 32) as u32, c.rh[5] as u32, (c.rh[5] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((c.r[6], c.rh[6]), (0u64, 0u64)); c.r[6] = a; c.rh[6] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[6], c.rh[6]), 16); c.r[6] = a; c.rh[6] = b; }
c.vf[6] = [c.r[6] as u32, (c.r[6] >> 32) as u32, c.rh[6] as u32, (c.rh[6] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextlh((c.r[8], c.rh[8]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
c.vf[7] = [c.r[7] as u32, (c.r[7] >> 32) as u32, c.rh[7] as u32, (c.rh[7] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((c.r[8], c.rh[8]), (0u64, 0u64)); c.r[8] = a; c.rh[8] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[8], c.rh[8]), 16); c.r[8] = a; c.rh[8] = b; }
c.vf[8] = [c.r[8] as u32, (c.r[8] >> 32) as u32, c.rh[8] as u32, (c.rh[8] >> 32) as u32];
uycpu::vu0::fmac::<6, 15>(c, 11, 11, uycpu::vu0::bcast(c.vi[22]));
c.r[4] = (((c.r[4] as u32).wrapping_add(0x00000080)) as i32 as i64 as u64);
{ let v = c.vf[9]; c.wr128((c.r[4] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[3] = (((c.r[3] as u32).wrapping_add(0xfffffffe)) as i32 as i64 as u64);
let t = (c.r[3] as i64) > 0;  { let v = c.vf[11]; c.wr128((c.r[4] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043b3f8; continue; }
 l = 0x0043b160; continue;
}
0x0043b540 => {
let t = c.r[1] != 0u64;   if t { l = 0x0043b6b0; continue; }
l = 0x0043b548; continue; }
0x0043b548 => {
let t = c.r[14] == 0u64;  c.r[9] = 0x0000000010000000;
 if t { l = 0x0043b6b0; continue; }
c.r[13] = c.rd16((c.r[14] as u32).wrapping_add(0x00000002)) as u64;
c.r[9] = c.r[9] | 0xd400;
c.r[12] = c.rd32((c.r[14] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64;
c.r[1] = ((0u32.wrapping_add(0x00002800)) as i32 as i64 as u64);
c.wr32((c.r[9] as u32).wrapping_add(0x00000080), c.r[1] as u32);
c.r[1] = (((c.r[13] as u32).wrapping_add((c.r[13] as u32))) as i32 as i64 as u64);
c.wr32((c.r[9] as u32).wrapping_add(0x00000010), c.r[12] as u32);
c.r[13] = (((c.r[13] as u32).wrapping_add((c.r[1] as u32))) as i32 as i64 as u64);
c.wr32((c.r[9] as u32).wrapping_add(0x00000020), c.r[13] as u32);
c.r[1] = ((0u32.wrapping_add(0x00000100)) as i32 as i64 as u64);
c.wr32((c.r[9] as u32), c.r[1] as u32);
c.r[13] = (((c.r[13] as u32) << 4) as i32 as i64 as u64);
l = 0x0043b580; continue; }
0x0043b580 => {
c.r[1] = c.rd32((c.r[14] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
c.r[12] = (((c.r[25] as u32).wrapping_add(0x00002800)) as i32 as i64 as u64);
c.vf[4] = [c.r[1] as u32, (c.r[1] >> 32) as u32, c.rh[1] as u32, (c.rh[1] >> 32) as u32];
c.r[13] = (((c.r[13] as u32).wrapping_add((c.r[12] as u32))) as i32 as i64 as u64);
c.r[14] = c.rd32((c.r[14] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
c.r[1] = (((c.r[25] as u32).wrapping_add(0x00001e00)) as i32 as i64 as u64);
c.wr32((c.r[13] as u32).wrapping_add(0x0000002c), c.r[1] as u32);
uycpu::vu0::fmac::<1, 1>(c, 4, 0, uycpu::vu0::bcast(c.vf[4][0]));
l = 0x0043b5a0; continue; }
0x0043b5a0 => {
c.r[2] = c.rd32((c.r[9] as u32)) as i32 as i64 as u64;
c.r[2] = c.r[2] & 0x100;
let t = c.r[2] != 0u64;   if t { l = 0x0043b5a0; continue; }
c.r[11] = c.rd32((c.r[12] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
{ let (a, b) = c.rd128((c.r[12] as u32)); c.vf[9] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[12] as u32).wrapping_add(0x00000010)); c.vf[10] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[12] as u32).wrapping_add(0x00000020)); c.vf[11] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[11] as u32)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[11] as u32).wrapping_add(0x00000010)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[11] as u32).wrapping_add(0x00000020)); c.vf[8] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
l = 0x0043b5e0; continue; }
0x0043b5e0 => {
uycpu::vu0::fmac::<6, 15>(c, 6, 6, uycpu::vu0::bcast(c.vf[4][3]));
uycpu::vu0::fmac::<6, 15>(c, 9, 9, uycpu::vu0::bcast(c.vf[4][0]));
uycpu::vu0::fmac::<6, 14>(c, 7, 7, uycpu::vu0::bcast(c.vf[4][3]));
uycpu::vu0::fmac::<6, 14>(c, 10, 10, uycpu::vu0::bcast(c.vf[4][0]));
uycpu::vu0::fmac::<6, 14>(c, 8, 8, uycpu::vu0::bcast(c.vf[4][3]));
uycpu::vu0::fmac::<6, 14>(c, 11, 11, uycpu::vu0::bcast(c.vf[4][0]));
uycpu::vu0::fmac::<0, 15>(c, 12, 6, c.vf[9]);
uycpu::vu0::fmac::<6, 15>(c, 14, 6, c.vf[9]);
uycpu::vu0::fmac::<0, 14>(c, 7, 7, c.vf[10]);
uycpu::vu0::fmac::<0, 14>(c, 8, 8, c.vf[11]);
uycpu::vu0::fmac::<6, 15>(c, 13, 12, c.vf[12]);
uycpu::vu0::fmac::<0, 4>(c, 32, 14, uycpu::vu0::bcast(c.vf[14][0]));
uycpu::vu0::fmac::<2, 4>(c, 32, 2, uycpu::vu0::bcast(c.vf[14][2]));
uycpu::vu0::fmac::<2, 4>(c, 14, 2, uycpu::vu0::bcast(c.vf[14][3]));
uycpu::vu0::fmac::<0, 8>(c, 32, 13, uycpu::vu0::bcast(c.vf[13][1]));
uycpu::vu0::fmac::<2, 8>(c, 32, 1, uycpu::vu0::bcast(c.vf[13][2]));
uycpu::vu0::fmac::<2, 8>(c, 13, 1, uycpu::vu0::bcast(c.vf[13][3]));
{ let (a, b) = { let v = c.vf[14]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; c.r[2] = a; c.rh[2] = b; }
let t = (c.r[2] as i64) >= 0;   if t { l = 0x0043b648; continue; }
uycpu::vu0::fmac::<1, 15>(c, 12, 6, c.vf[9]);
uycpu::vu0::fmac::<6, 15>(c, 13, 12, c.vf[12]);
uycpu::vu0::fmac::<0, 8>(c, 32, 13, uycpu::vu0::bcast(c.vf[13][1]));
uycpu::vu0::fmac::<2, 8>(c, 32, 1, uycpu::vu0::bcast(c.vf[13][2]));
l = 0x0043b640; continue; }
0x0043b640 => {
uycpu::vu0::fmac::<2, 8>(c, 13, 1, uycpu::vu0::bcast(c.vf[13][3]));
l = 0x0043b648; continue; }
0x0043b648 => {
uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[13][0]);
c.r[3] = c.rd32((c.r[12] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
c.r[1] = c.rd32((c.r[12] as u32).wrapping_add(0x0000005c)) as i32 as i64 as u64;
{ let (a, b) = c.rd128((c.r[12] as u32).wrapping_add(0x00000030)); c.vf[9] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[12] as u32).wrapping_add(0x00000040)); c.vf[10] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[12] as u32).wrapping_add(0x00000050)); c.vf[11] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let v = c.vf[7]; c.wr128((c.r[11] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[8]; c.wr128((c.r[11] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let (a, b) = c.rd128((c.r[1] as u32)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[1] as u32).wrapping_add(0x00000010)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[1] as u32).wrapping_add(0x00000020)); c.vf[8] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
c.r[2] = c.rd32((c.r[11] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
let t = c.r[2] != 0u64;   if t { l = 0x0043b690; continue; }
c.wr32((c.r[11] as u32).wrapping_add(0x0000001c), c.r[3] as u32);
l = 0x0043b690; continue; }
0x0043b690 => {
uycpu::vu0::fmac::<6, 15>(c, 12, 12, uycpu::vu0::bcast(c.vi[22]));
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000030)) as i32 as i64 as u64);
{ let v = c.vf[12]; c.wr128((c.r[11] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[11] = (((c.r[1] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
let t = c.r[12] != c.r[13];   if t { l = 0x0043b5e0; continue; }
let t = c.r[14] != 0u64;   if t { l = 0x0043b548; continue; }
l = 0x0043b6b0; continue; }
0x0043b6b0 => {
c.r[15] = c.r[15] >> 32;
c.r[1] = c.r[24] & 0x100;
let t = c.r[1] != 0u64;  uycpu::vu0::fmac::<0, 14>(c, 5, 0, uycpu::vu0::bcast(c.vf[0][3]));
 if t { l = 0x0043b7c8; continue; }
l = 0x0043b6c0; continue; }
0x0043b6c0 => {
let t = c.r[15] == 0u64;  c.r[14] = (((c.r[15] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
 if t { l = 0x0043b7c8; continue; }
c.r[1] = c.rd32((c.r[14] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
c.r[2] = c.rd8((c.r[14] as u32).wrapping_add(0x00000003)) as u64;
{ let (a, b) = c.rd128((c.r[1] as u32)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[1] as u32).wrapping_add(0x00000010)); c.vf[8] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[1] as u32).wrapping_add(0x00000020)); c.vf[10] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[14] as u32).wrapping_add(0x00000010)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[14] as u32).wrapping_add(0x00000020)); c.vf[9] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[14] as u32).wrapping_add(0x00000030)); c.vf[11] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
let t = c.r[2] != 0u64;  c.r[15] = c.rd32((c.r[14] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
 if t { l = 0x0043b728; continue; }
uycpu::vu0::fmac::<6, 1>(c, 12, 7, c.vf[6]);
uycpu::vu0::fmac::<6, 14>(c, 13, 7, c.vf[6]);
uycpu::vu0::fmac::<6, 14>(c, 14, 7, uycpu::vu0::bcast(c.vf[6][3]));
uycpu::vu0::fmac::<6, 14>(c, 15, 6, uycpu::vu0::bcast(c.vf[7][3]));
uycpu::vu0::opmula(c, 6, 7);
uycpu::vu0::opmsub(c, 16, 7, 6);
uycpu::vu0::fmac::<0, 8>(c, 32, 13, uycpu::vu0::bcast(c.vf[13][1]));
uycpu::vu0::fmac::<2, 8>(c, 13, 5, uycpu::vu0::bcast(c.vf[13][2]));
uycpu::vu0::fmac::<0, 14>(c, 6, 14, c.vf[15]);
uycpu::vu0::fmac::<0, 14>(c, 6, 6, c.vf[16]);
uycpu::vu0::fmac::<1, 1>(c, 6, 12, uycpu::vu0::bcast(c.vf[13][0]));
uycpu::vu0::fmac::<6, 14>(c, 8, 8, c.vf[9]);
uycpu::vu0::fmac::<0, 14>(c, 10, 10, c.vf[11]);
 l = 0x0043b7b8; continue;
}
0x0043b728 => {
c.r[3] = c.rd32((c.r[14] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
c.vf[18] = [c.r[3] as u32, (c.r[3] >> 32) as u32, c.rh[3] as u32, (c.rh[3] >> 32) as u32];
uycpu::vu0::fmac::<1, 1>(c, 18, 0, uycpu::vu0::bcast(c.vf[18][0]));
uycpu::vu0::fmac::<6, 15>(c, 6, 6, uycpu::vu0::bcast(c.vf[18][3]));
uycpu::vu0::fmac::<6, 15>(c, 7, 7, uycpu::vu0::bcast(c.vf[18][0]));
uycpu::vu0::fmac::<6, 14>(c, 8, 8, uycpu::vu0::bcast(c.vf[18][3]));
uycpu::vu0::fmac::<6, 14>(c, 9, 9, uycpu::vu0::bcast(c.vf[18][0]));
uycpu::vu0::fmac::<6, 14>(c, 10, 10, uycpu::vu0::bcast(c.vf[18][3]));
uycpu::vu0::fmac::<6, 14>(c, 11, 11, uycpu::vu0::bcast(c.vf[18][0]));
uycpu::vu0::fmac::<0, 15>(c, 14, 6, c.vf[7]);
uycpu::vu0::fmac::<6, 15>(c, 16, 6, c.vf[7]);
uycpu::vu0::fmac::<0, 14>(c, 8, 8, c.vf[9]);
uycpu::vu0::fmac::<0, 14>(c, 10, 10, c.vf[11]);
uycpu::vu0::fmac::<6, 15>(c, 15, 14, c.vf[14]);
uycpu::vu0::fmac::<0, 4>(c, 32, 16, uycpu::vu0::bcast(c.vf[16][0]));
uycpu::vu0::fmac::<2, 4>(c, 32, 2, uycpu::vu0::bcast(c.vf[16][2]));
uycpu::vu0::fmac::<2, 4>(c, 16, 2, uycpu::vu0::bcast(c.vf[16][3]));
uycpu::vu0::fmac::<0, 8>(c, 32, 15, uycpu::vu0::bcast(c.vf[15][1]));
uycpu::vu0::fmac::<2, 8>(c, 32, 1, uycpu::vu0::bcast(c.vf[15][2]));
uycpu::vu0::fmac::<2, 8>(c, 15, 1, uycpu::vu0::bcast(c.vf[15][3]));
{ let (a, b) = { let v = c.vf[16]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; c.r[2] = a; c.rh[2] = b; }
let t = (c.r[2] as i64) >= 0;   if t { l = 0x0043b7a0; continue; }
uycpu::vu0::fmac::<1, 15>(c, 14, 6, c.vf[7]);
uycpu::vu0::fmac::<6, 15>(c, 15, 14, c.vf[14]);
uycpu::vu0::fmac::<0, 8>(c, 32, 15, uycpu::vu0::bcast(c.vf[15][1]));
uycpu::vu0::fmac::<2, 8>(c, 32, 1, uycpu::vu0::bcast(c.vf[15][2]));
uycpu::vu0::fmac::<2, 8>(c, 15, 1, uycpu::vu0::bcast(c.vf[15][3]));
l = 0x0043b7a0; continue; }
0x0043b7a0 => {
uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[15][0]);
uycpu::vu0::fmac::<6, 15>(c, 6, 14, uycpu::vu0::bcast(c.vi[22]));
l = 0x0043b7b8; continue; }
0x0043b7b8 => {
{ let v = c.vf[6]; c.wr128((c.r[1] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[8]; c.wr128((c.r[1] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
let t = c.r[15] != 0u64;  { let v = c.vf[10]; c.wr128((c.r[1] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043b6c0; continue; }
l = 0x0043b7c8; continue; }
0x0043b7c8 => {
 l = 0x0043b178; continue;
}
_ => panic!("the animation unit: no label {l:08x} in 0x0043AAF8"), } }
}

#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn n0043cf90(c: &mut Cpu, k: F) {
let mut l: u32 = 0x0043cf90;
loop { match l {
0x0043cf90 => {
c.r[7] = sx(k.hi(c, 0x0043cf90));
c.r[7] = (((c.r[7] as u32).wrapping_add(k.lo(c, 0x0043cf94))) as i32 as i64 as u64);
c.r[15] = c.rd32((c.r[4] as u32).wrapping_add(0x00000024)) as i32 as i64 as u64;
c.r[12] = c.rd32((c.r[4] as u32).wrapping_add(0x00000058)) as i32 as i64 as u64;
c.r[10] = c.rd8((c.r[15] as u32).wrapping_add(0x00000008)) as u64;
c.r[8] = 0x0000000010000000;
c.r[13] = c.rd32((c.r[4] as u32).wrapping_add(0x0000005c)) as i32 as i64 as u64;
c.r[8] = c.r[8] | 0xd400;
let t = c.r[10] == 0u64;  c.r[9] = c.rd32((c.r[4] as u32).wrapping_add(0x00000044)) as i32 as i64 as u64;
 if t { l = 0x0043d9d0; continue; }
c.r[11] = c.rd32((c.r[15] as u32).wrapping_add(0x00000018)) as i32 as i64 as u64;
c.r[1] = 0u64 | 0x3000;
c.wr16((c.r[7] as u32), c.r[10] as u16);
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
c.wr16((c.r[7] as u32).wrapping_add(0x00000002), c.r[1] as u16);
c.r[13] = (((c.r[13] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
c.wr32((c.r[7] as u32).wrapping_add(0x00000004), c.r[11] as u32);
c.r[24] = c.r[4].wrapping_add(0u64);
c.r[4] = c.rd16((c.r[12] as u32).wrapping_add(0xfffffff6)) as i16 as i64 as u64;
c.r[25] = c.r[5].wrapping_add(0u64);
c.wr16((c.r[7] as u32).wrapping_add(0x00000012), c.r[1] as u16);
c.r[14] = 0x0000000070000000;
c.wr16((c.r[7] as u32).wrapping_add(0x00000010), c.r[4] as u16);
c.r[1] = (((c.r[10] as u32) << 4) as i32 as i64 as u64);
c.wr32((c.r[7] as u32).wrapping_add(0x00000014), c.r[12] as u32);
c.r[1] = (((c.r[1] as u32).wrapping_add((c.r[14] as u32))) as i32 as i64 as u64);
c.r[11] = c.rd16((c.r[13] as u32).wrapping_add(0xfffffff6)) as u64;
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00002000)) as i32 as i64 as u64);
c.wr32((c.r[7] as u32).wrapping_add(0x00000024), c.r[13] as u32);
c.r[4] = (((c.r[4] as u32) << 4) as i32 as i64 as u64);
c.wr32((c.r[7] as u32).wrapping_add(0x00000020), c.r[11] as u32);
c.r[4] = (((c.r[4] as u32).wrapping_add((c.r[1] as u32))) as i32 as i64 as u64);
let t = c.r[9] != 0u64;   if t { l = 0x0043d020; continue; }
c.wr16((c.r[7] as u32).wrapping_add(0x00000012), 0u64 as u16);
l = 0x0043d020; continue; }
0x0043d020 => {
c.r[11] = (((c.r[11] as u32) << 4) as i32 as i64 as u64);
c.r[2] = ((0u32.wrapping_add(0xffffffff)) as i32 as i64 as u64);
c.wr32((c.r[1] as u32).wrapping_add(0xfffffffc), c.r[2] as u32);
c.r[11] = (((c.r[11] as u32).wrapping_add((c.r[4] as u32))) as i32 as i64 as u64);
c.wr32((c.r[4] as u32).wrapping_add(0xfffffffc), c.r[2] as u32);
uycpu::vu0::fmac::<6, 15>(c, 1, 0, uycpu::vu0::bcast(c.vf[0][0]));
c.wr32((c.r[11] as u32).wrapping_add(0xfffffffc), c.r[2] as u32);
uycpu::vu0::fmac::<6, 15>(c, 2, 0, uycpu::vu0::bcast(c.vf[0][0]));
c.wr32((c.r[8] as u32).wrapping_add(0x00000030), c.r[7] as u32);
c.r[7] = c.r[14] | 0x2000;
c.r[2] = c.rd16((c.r[12] as u32).wrapping_add(0xfffffffa)) as i16 as i64 as u64;
{ let v = c.vf[0]; uycpu::vu0::mov::<15>(c, 3, [v[1], v[2], v[3], v[0]]); }
c.r[3] = c.rd16((c.r[12] as u32).wrapping_add(0xfffffffe)) as i16 as i64 as u64;
uycpu::vu0::fmac::<0, 14>(c, 4, 0, uycpu::vu0::bcast(c.vf[0][3]));
c.wr32((c.r[8] as u32).wrapping_add(0x00000080), c.r[7] as u32);
c.r[7] = 0u64 | 0x104;
c.r[5] = c.rd16((c.r[13] as u32).wrapping_add(0xfffffffa)) as i16 as i64 as u64;
uycpu::vu0::fmac::<1, 1>(c, 4, 0, c.vf[0]);
c.r[6] = c.rd16((c.r[13] as u32).wrapping_add(0xfffffffe)) as i16 as i64 as u64;
uycpu::vu0::fmac::<0, 8>(c, 1, 1, uycpu::vu0::bcast(c.vf[0][3]));
c.wr32((c.r[8] as u32), c.r[7] as u32);
uycpu::vu0::fmac::<0, 4>(c, 2, 2, uycpu::vu0::bcast(c.vf[0][3]));
c.vf[7] = [c.r[9] as u32, (c.r[9] >> 32) as u32, c.rh[9] as u32, (c.rh[9] >> 32) as u32];
uycpu::vu0::fmac::<1, 8>(c, 5, 0, uycpu::vu0::bcast(c.vf[0][3]));
uycpu::vu0::fmac::<1, 8>(c, 6, 4, c.vf[7]);
c.r[9] = (((c.r[14] as u32).wrapping_add(0xffffffc0)) as i32 as i64 as u64);
c.r[7] = (((c.r[10] as u32) << 6) as i32 as i64 as u64);
c.r[7] = (((c.r[7] as u32).wrapping_add((c.r[9] as u32))) as i32 as i64 as u64);
l = 0x0043d0a8; continue; }
0x0043d0a8 => {
c.r[9] = (((c.r[9] as u32).wrapping_add(0x00000040)) as i32 as i64 as u64);
{ let v = c.vf[4]; c.wr128((c.r[9] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
let t = c.r[7] != c.r[9];  { let v = c.vf[4]; c.wr128((c.r[9] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043d0a8; continue; }
c.r[7] = c.rd8((c.r[24] as u32).wrapping_add(0x00000040)) as u64;
c.r[8] = c.rd8((c.r[24] as u32).wrapping_add(0x00000041)) as u64;
c.r[9] = c.rd8((c.r[24] as u32).wrapping_add(0x00000042)) as u64;
c.r[8] = (((c.r[8] as u32).wrapping_sub((c.r[7] as u32))) as i32 as i64 as u64);
c.r[7] = c.rd8((c.r[24] as u32).wrapping_add(0x00000043)) as u64;
c.r[8] = c.r[8] & 0xfffe;
c.r[7] = (((c.r[9] as u32).wrapping_sub((c.r[7] as u32))) as i32 as i64 as u64);
c.r[8] = (((c.r[8] as u32).wrapping_add((c.r[7] as u32))) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pceqw((c.r[8], c.rh[8]), (0u64, 0u64)); c.r[8] = a; c.rh[8] = b; }
c.r[8] = c.r[8] & 0x400;
{ let (a, b) = uycpu::mmi::pcpyld((c.r[8], c.rh[8]), (c.r[25], c.rh[25])); c.r[25] = a; c.rh[25] = b; }
l = 0x0043d0f0; continue; }
0x0043d0f0 => {
c.r[7] = c.rd32((c.r[1] as u32).wrapping_add(0xfffffffc)) as i32 as i64 as u64;
c.r[8] = ((0u32.wrapping_add(0xffffffff)) as i32 as i64 as u64);
let t = c.r[7] == c.r[8];   if t { l = 0x0043d0f0; continue; }
c.r[7] = c.r[10].wrapping_add(0u64);
c.r[8] = c.r[14] | 0x2000;
c.r[9] = (((c.r[14] as u32).wrapping_add(0xffffff00)) as i32 as i64 as u64);
l = 0x0043d118; continue; }
0x0043d118 => {
{ let (a, b) = c.rd128((c.r[8] as u32)); c.vf[16] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
c.r[9] = (((c.r[9] as u32).wrapping_add(0x00000100)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[8] as u32).wrapping_add(0x00000010)); c.vf[17] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
c.r[7] = (((c.r[7] as u32).wrapping_add(0xfffffffc)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[8] as u32).wrapping_add(0x00000020)); c.vf[18] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[8] as u32).wrapping_add(0x00000030)); c.vf[19] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let v = c.vf[16]; c.wr128((c.r[9] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
uycpu::vu0::fmac::<6, 1>(c, 16, 16, uycpu::vu0::bcast(c.vf[0][0]));
{ let v = c.vf[17]; c.wr128((c.r[9] as u32).wrapping_add(0x00000060), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
uycpu::vu0::fmac::<6, 1>(c, 17, 17, uycpu::vu0::bcast(c.vf[0][0]));
{ let v = c.vf[18]; c.wr128((c.r[9] as u32).wrapping_add(0x000000a0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
uycpu::vu0::fmac::<6, 1>(c, 18, 18, uycpu::vu0::bcast(c.vf[0][0]));
{ let v = c.vf[19]; c.wr128((c.r[9] as u32).wrapping_add(0x000000e0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
uycpu::vu0::fmac::<6, 1>(c, 19, 19, uycpu::vu0::bcast(c.vf[0][0]));
{ let v = c.vf[16]; c.wr128((c.r[9] as u32).wrapping_add(0x00000030), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[17]; c.wr128((c.r[9] as u32).wrapping_add(0x00000070), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[18]; c.wr128((c.r[9] as u32).wrapping_add(0x000000b0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[8] = (((c.r[8] as u32).wrapping_add(0x00000040)) as i32 as i64 as u64);
let t = (c.r[7] as i64) > 0;  { let v = c.vf[19]; c.wr128((c.r[9] as u32).wrapping_add(0x000000f0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043d118; continue; }
l = 0x0043d168; continue; }
0x0043d168 => {
c.r[7] = c.rd32((c.r[4] as u32).wrapping_add(0xfffffffc)) as i32 as i64 as u64;
c.r[8] = ((0u32.wrapping_add(0xffffffff)) as i32 as i64 as u64);
let t = c.r[7] == c.r[8];   if t { l = 0x0043d168; continue; }
{ let (a, b) = { let v = c.vf[7]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; c.r[7] = a; c.rh[7] = b; }
let t = c.r[7] == 0u64;  c.wr128((c.r[14] as u32).wrapping_add(0x00002000), 0u64, 0u64);
 if t { l = 0x0043d544; continue; }
c.r[12] = (((c.r[10] as u32) << 3) as i32 as i64 as u64);
c.r[13] = (((c.r[14] as u32).wrapping_add(0x00001ff4)) as i32 as i64 as u64);
c.r[12] = (((c.r[12] as u32).wrapping_add((c.r[1] as u32))) as i32 as i64 as u64);
uycpu::vu0::fmac::<6, 15>(c, 9, 0, uycpu::vu0::bcast(c.vf[0][0]));
l = 0x0043d1a0; continue; }
0x0043d1a0 => {
let t = c.r[2] == 0u64;  c.r[7] = c.rd64((c.r[12] as u32));
 if t { l = 0x0043d1f8; continue; }
c.r[8] = c.rd8((c.r[12] as u32).wrapping_add(0x00000006)) as u64;
c.r[2] = (((c.r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[9] = a; c.rh[9] = b; }
{ let (a, b) = uycpu::mmi::psrlw((c.r[9], c.rh[9]), 13); c.r[9] = a; c.rh[9] = b; }
c.vf[8] = [c.r[9] as u32, (c.r[9] >> 32) as u32, c.rh[9] as u32, (c.rh[9] >> 32) as u32];
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
let t = (c.r[7] as i64) >= 0;  c.r[9] = (((c.r[8] as u32) << 6) as i32 as i64 as u64);
 if t { l = 0x0043d1a0; continue; }
uycpu::vu0::itof::<3, 14>(c, 9, 8);
c.r[9] = (((c.r[9] as u32).wrapping_add((c.r[14] as u32))) as i32 as i64 as u64);
c.wr32((c.r[13] as u32).wrapping_add(0x0000000c), c.r[9] as u32);
c.r[13] = c.r[9].wrapping_add(0u64);
c.r[7] = c.r[7] >> 62;
c.r[7] = (((c.r[7] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
c.wr32((c.r[9] as u32).wrapping_add(0x0000001c), c.r[7] as u32);
{ let v = c.vf[9]; c.wr128((c.r[9] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 l = 0x0043d1a0; continue;
}
0x0043d1f8 => {
c.r[9] = (((c.r[14] as u32).wrapping_add(0x00001fc8)) as i32 as i64 as u64);
uycpu::vu0::fmac::<6, 1>(c, 9, 9, uycpu::vu0::bcast(c.vf[0][0]));
l = 0x0043d200; continue; }
0x0043d200 => {
let t = c.r[3] == 0u64;  c.r[7] = c.rd64((c.r[12] as u32));
 if t { l = 0x0043d248; continue; }
c.r[8] = c.rd8((c.r[12] as u32).wrapping_add(0x00000006)) as u64;
c.r[3] = (((c.r[3] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
c.vf[8] = [c.r[7] as u32, (c.r[7] >> 32) as u32, c.rh[7] as u32, (c.rh[7] >> 32) as u32];
c.r[8] = (((c.r[8] as u32) << 6) as i32 as i64 as u64);
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
c.r[8] = (((c.r[8] as u32).wrapping_add((c.r[14] as u32))) as i32 as i64 as u64);
uycpu::vu0::itof::<0, 14>(c, 9, 8);
c.r[7] = c.rd32((c.r[8] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
c.wr32((c.r[9] as u32).wrapping_add(0x0000003c), c.r[8] as u32);
c.r[9] = c.r[8].wrapping_add(0u64);
{ let v = c.vf[9]; c.wr128((c.r[8] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.wr32((c.r[8] as u32).wrapping_add(0x0000002c), c.r[7] as u32);
 l = 0x0043d200; continue;
}
0x0043d248 => {
c.wr32((c.r[9] as u32).wrapping_add(0x0000003c), 0u64 as u32);
l = 0x0043d250; continue; }
0x0043d250 => {
c.r[7] = c.rd32((c.r[11] as u32).wrapping_add(0xfffffffc)) as i32 as i64 as u64;
c.r[8] = ((0u32.wrapping_add(0xffffffff)) as i32 as i64 as u64);
let t = c.r[7] == c.r[8];   if t { l = 0x0043d250; continue; }
c.r[12] = (((c.r[10] as u32) << 3) as i32 as i64 as u64);
c.r[12] = (((c.r[12] as u32).wrapping_add((c.r[4] as u32))) as i32 as i64 as u64);
l = 0x0043d270; continue; }
0x0043d270 => {
let t = c.r[5] == 0u64;  c.r[7] = c.rd64((c.r[12] as u32));
 if t { l = 0x0043d2c8; continue; }
c.r[11] = c.rd8((c.r[12] as u32).wrapping_add(0x00000006)) as u64;
c.r[5] = (((c.r[5] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[8] = a; c.rh[8] = b; }
{ let (a, b) = uycpu::mmi::psrlw((c.r[8], c.rh[8]), 13); c.r[8] = a; c.rh[8] = b; }
c.vf[8] = [c.r[8] as u32, (c.r[8] >> 32) as u32, c.rh[8] as u32, (c.rh[8] >> 32) as u32];
c.r[11] = (((c.r[11] as u32) << 6) as i32 as i64 as u64);
c.r[11] = (((c.r[11] as u32).wrapping_add((c.r[14] as u32))) as i32 as i64 as u64);
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
c.r[8] = c.rd32((c.r[11] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
uycpu::vu0::itof::<3, 14>(c, 8, 8);
let t = (c.r[7] as i64) >= 0;  c.r[7] = c.r[7] >> 62;
 if t { l = 0x0043d270; continue; }
c.r[7] = (((c.r[7] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
{ let v = c.vf[8]; c.wr128((c.r[11] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
let t = c.r[8] != 0u64;  c.wr32((c.r[11] as u32).wrapping_add(0x0000001c), c.r[7] as u32);
 if t { l = 0x0043d270; continue; }
c.wr32((c.r[13] as u32).wrapping_add(0x0000000c), c.r[11] as u32);
c.r[13] = c.r[11].wrapping_add(0u64);
c.wr32((c.r[13] as u32).wrapping_add(0x0000000c), 0u64 as u32);
 l = 0x0043d270; continue;
}
0x0043d2c8 => {
let t = c.r[6] == 0u64;  c.r[7] = c.rd64((c.r[12] as u32));
 if t { l = 0x0043d318; continue; }
c.r[11] = c.rd8((c.r[12] as u32).wrapping_add(0x00000006)) as u64;
c.r[6] = (((c.r[6] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[8] = a; c.rh[8] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[8], c.rh[8]), 16); c.r[8] = a; c.rh[8] = b; }
c.vf[8] = [c.r[8] as u32, (c.r[8] >> 32) as u32, c.rh[8] as u32, (c.rh[8] >> 32) as u32];
c.r[11] = (((c.r[11] as u32) << 6) as i32 as i64 as u64);
c.r[11] = (((c.r[11] as u32).wrapping_add((c.r[14] as u32))) as i32 as i64 as u64);
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
c.r[8] = c.rd32((c.r[11] as u32).wrapping_add(0x0000003c)) as i32 as i64 as u64;
uycpu::vu0::itof::<0, 14>(c, 8, 8);
{ let v = c.vf[8]; c.wr128((c.r[11] as u32).wrapping_add(0x00000030), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
let t = c.r[8] != 0u64;  c.wr32((c.r[11] as u32).wrapping_add(0x0000003c), c.r[8] as u32);
 if t { l = 0x0043d2c8; continue; }
c.wr32((c.r[9] as u32).wrapping_add(0x0000003c), c.r[11] as u32);
c.r[9] = c.r[11].wrapping_add(0u64);
c.wr32((c.r[9] as u32).wrapping_add(0x0000003c), 0u64 as u32);
 l = 0x0043d2c8; continue;
}
0x0043d318 => {
c.r[3] = c.rd32((c.r[14] as u32).wrapping_add(0x00002000)) as i32 as i64 as u64;
l = 0x0043d320; continue; }
0x0043d320 => {
let t = c.r[3] == 0u64;  c.r[2] = c.r[3].wrapping_add(0u64);
 if t { l = 0x0043d348; continue; }
{ let (a, b) = c.rd128((c.r[2] as u32)); c.vf[8] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[2] as u32).wrapping_add(0x00000010)); c.vf[9] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::fmac::<6, 14>(c, 32, 8, uycpu::vu0::bcast(c.vf[6][0]));
uycpu::vu0::fmac::<2, 14>(c, 9, 9, uycpu::vu0::bcast(c.vf[7][0]));
c.r[3] = c.rd32((c.r[2] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
{ let v = c.vf[9]; c.wr128((c.r[2] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 l = 0x0043d320; continue;
}
0x0043d348 => {
c.r[2] = c.rd32((c.r[14] as u32).wrapping_add(0x00002004)) as i32 as i64 as u64;
l = 0x0043d350; continue; }
0x0043d350 => {
let t = c.r[2] == 0u64;  c.r[3] = c.r[2].wrapping_add(0u64);
 if t { l = 0x0043d374; continue; }
{ let (a, b) = c.rd128((c.r[3] as u32).wrapping_add(0x00000020)); c.vf[8] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[3] as u32).wrapping_add(0x00000030)); c.vf[9] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::fmac::<6, 14>(c, 32, 8, uycpu::vu0::bcast(c.vf[6][0]));
uycpu::vu0::fmac::<2, 14>(c, 8, 9, uycpu::vu0::bcast(c.vf[7][0]));
c.r[2] = c.rd32((c.r[2] as u32).wrapping_add(0x0000003c)) as i32 as i64 as u64;
{ let v = c.vf[8]; c.wr128((c.r[3] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 l = 0x0043d350; continue;
}
0x0043d374 => {
{ let (a, b) = uycpu::mmi::pcpyud((c.r[25], c.rh[25]), (0u64, 0u64)); c.r[2] = a; c.rh[2] = b; }
c.r[2] = c.r[2] & 0x400;
let t = c.r[2] != 0u64;   if t { l = 0x0043d4ac; continue; }
c.r[2] = c.rd8((c.r[25] as u32).wrapping_add(0x0000007f)) as u64;
c.r[12] = (((c.r[14] as u32).wrapping_add(0xffffff80)) as i32 as i64 as u64);
l = 0x0043d394; continue; }
0x0043d394 => {
let t = (c.r[2] as i64) <= 0;  { let (a, b) = c.rd128((c.r[1] as u32)); c.r[7] = a; c.rh[7] = b; }
 if t { l = 0x0043d648; continue; }
c.r[2] = (((c.r[2] as u32).wrapping_add(0xfffffffe)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[4] as u32)); c.r[8] = a; c.rh[8] = b; }
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[5] = a; c.rh[5] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[5], c.rh[5]), 16); c.r[5] = a; c.rh[5] = b; }
{ let (a, b) = uycpu::mmi::pextlh((c.r[8], c.rh[8]), (0u64, 0u64)); c.r[6] = a; c.rh[6] = b; }
c.vf[8] = [c.r[5] as u32, (c.r[5] >> 32) as u32, c.rh[5] as u32, (c.rh[5] >> 32) as u32];
{ let (a, b) = uycpu::mmi::psraw((c.r[6], c.rh[6]), 16); c.r[6] = a; c.rh[6] = b; }
{ let (a, b) = uycpu::mmi::pextuh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
c.vf[9] = [c.r[6] as u32, (c.r[6] >> 32) as u32, c.rh[6] as u32, (c.rh[6] >> 32) as u32];
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
{ let (a, b) = uycpu::mmi::pextuh((c.r[8], c.rh[8]), (0u64, 0u64)); c.r[8] = a; c.rh[8] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[8], c.rh[8]), 16); c.r[8] = a; c.rh[8] = b; }
c.vf[10] = [c.r[7] as u32, (c.r[7] >> 32) as u32, c.rh[7] as u32, (c.rh[7] >> 32) as u32];
c.vf[11] = [c.r[8] as u32, (c.r[8] >> 32) as u32, c.rh[8] as u32, (c.rh[8] >> 32) as u32];
uycpu::vu0::itof::<3, 15>(c, 8, 8);
uycpu::vu0::itof::<3, 15>(c, 9, 9);
uycpu::vu0::itof::<3, 15>(c, 10, 10);
uycpu::vu0::itof::<3, 15>(c, 11, 11);
uycpu::vu0::fmac::<6, 15>(c, 12, 8, c.vf[9]);
uycpu::vu0::fmac::<6, 15>(c, 13, 10, c.vf[11]);
uycpu::vu0::fmac::<6, 15>(c, 14, 9, uycpu::vu0::bcast(c.vf[5][0]));
uycpu::vu0::fmac::<6, 15>(c, 15, 11, uycpu::vu0::bcast(c.vf[5][0]));
uycpu::vu0::fmac::<6, 15>(c, 32, 8, uycpu::vu0::bcast(c.vf[6][0]));
uycpu::vu0::fmac::<2, 15>(c, 8, 9, uycpu::vu0::bcast(c.vf[7][0]));
uycpu::vu0::fmac::<2, 15>(c, 9, 14, uycpu::vu0::bcast(c.vf[7][0]));
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
uycpu::vu0::fmac::<0, 4>(c, 32, 12, uycpu::vu0::bcast(c.vf[12][0]));
uycpu::vu0::fmac::<2, 4>(c, 32, 4, uycpu::vu0::bcast(c.vf[12][2]));
uycpu::vu0::fmac::<2, 4>(c, 12, 4, uycpu::vu0::bcast(c.vf[12][3]));
c.r[4] = (((c.r[4] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
uycpu::vu0::fmac::<0, 4>(c, 32, 13, uycpu::vu0::bcast(c.vf[13][0]));
uycpu::vu0::fmac::<2, 4>(c, 32, 4, uycpu::vu0::bcast(c.vf[13][2]));
uycpu::vu0::fmac::<2, 4>(c, 13, 4, uycpu::vu0::bcast(c.vf[13][3]));
uycpu::vu0::fmac::<6, 15>(c, 32, 10, uycpu::vu0::bcast(c.vf[6][0]));
{ let (a, b) = { let v = c.vf[12]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; c.r[7] = a; c.rh[7] = b; }
uycpu::vu0::fmac::<2, 15>(c, 10, 11, uycpu::vu0::bcast(c.vf[7][0]));
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000080)) as i32 as i64 as u64);
uycpu::vu0::fmac::<2, 15>(c, 11, 15, uycpu::vu0::bcast(c.vf[7][0]));
{ let (a, b) = { let v = c.vf[13]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; c.r[8] = a; c.rh[8] = b; }
let t = (c.r[7] as i64) >= 0;  if t { uycpu::vu0::fmac::<6, 15>(c, 9, 8, c.vf[8]);
 l = 0x0043d45c; continue; }
uycpu::vu0::mov::<15>(c, 8, c.vf[9]);
uycpu::vu0::fmac::<6, 15>(c, 9, 9, c.vf[9]);
l = 0x0043d45c; continue; }
0x0043d45c => {
let t = (c.r[8] as i64) >= 0;  if t { uycpu::vu0::fmac::<6, 15>(c, 11, 10, c.vf[10]);
 l = 0x0043d46c; continue; }
uycpu::vu0::mov::<15>(c, 10, c.vf[11]);
uycpu::vu0::fmac::<6, 15>(c, 11, 11, c.vf[11]);
l = 0x0043d46c; continue; }
0x0043d46c => {
uycpu::vu0::fmac::<0, 8>(c, 32, 9, uycpu::vu0::bcast(c.vf[9][1]));
uycpu::vu0::fmac::<2, 8>(c, 32, 4, uycpu::vu0::bcast(c.vf[9][2]));
uycpu::vu0::fmac::<2, 8>(c, 9, 4, uycpu::vu0::bcast(c.vf[9][3]));
uycpu::vu0::fmac::<0, 8>(c, 32, 11, uycpu::vu0::bcast(c.vf[11][1]));
uycpu::vu0::fmac::<2, 8>(c, 32, 4, uycpu::vu0::bcast(c.vf[11][2]));
uycpu::vu0::fmac::<2, 8>(c, 11, 4, uycpu::vu0::bcast(c.vf[11][3]));
uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[9][0]);
uycpu::vu0::fmac::<6, 15>(c, 8, 8, uycpu::vu0::bcast(c.vi[22]));
uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[11][0]);
uycpu::vu0::fmac::<6, 15>(c, 10, 10, uycpu::vu0::bcast(c.vi[22]));
{ let v = c.vf[8]; c.wr128((c.r[12] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[10]; c.wr128((c.r[12] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 l = 0x0043d394; continue;
}
0x0043d4ac => {
c.r[2] = c.rd8((c.r[25] as u32).wrapping_add(0x0000007f)) as u64;
c.r[12] = (((c.r[14] as u32).wrapping_add(0xffffff80)) as i32 as i64 as u64);
l = 0x0043d4b4; continue; }
0x0043d4b4 => {
let t = (c.r[2] as i64) <= 0;  { let (a, b) = c.rd128((c.r[1] as u32)); c.r[7] = a; c.rh[7] = b; }
 if t { l = 0x0043d53c; continue; }
c.r[2] = (((c.r[2] as u32).wrapping_add(0xfffffffe)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[4] as u32)); c.r[8] = a; c.rh[8] = b; }
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[5] = a; c.rh[5] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[5], c.rh[5]), 16); c.r[5] = a; c.rh[5] = b; }
{ let (a, b) = uycpu::mmi::pextlh((c.r[8], c.rh[8]), (0u64, 0u64)); c.r[6] = a; c.rh[6] = b; }
c.vf[8] = [c.r[5] as u32, (c.r[5] >> 32) as u32, c.rh[5] as u32, (c.rh[5] >> 32) as u32];
{ let (a, b) = uycpu::mmi::psraw((c.r[6], c.rh[6]), 16); c.r[6] = a; c.rh[6] = b; }
{ let (a, b) = uycpu::mmi::pextuh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
c.vf[9] = [c.r[6] as u32, (c.r[6] >> 32) as u32, c.rh[6] as u32, (c.rh[6] >> 32) as u32];
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
{ let (a, b) = uycpu::mmi::pextuh((c.r[8], c.rh[8]), (0u64, 0u64)); c.r[8] = a; c.rh[8] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[8], c.rh[8]), 16); c.r[8] = a; c.rh[8] = b; }
c.vf[10] = [c.r[7] as u32, (c.r[7] >> 32) as u32, c.rh[7] as u32, (c.rh[7] >> 32) as u32];
c.vf[11] = [c.r[8] as u32, (c.r[8] >> 32) as u32, c.rh[8] as u32, (c.rh[8] >> 32) as u32];
uycpu::vu0::itof::<3, 15>(c, 8, 8);
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000080)) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 15>(c, 9, 9);
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 15>(c, 10, 10);
uycpu::vu0::itof::<3, 15>(c, 11, 11);
uycpu::vu0::fmac::<6, 15>(c, 32, 8, uycpu::vu0::bcast(c.vf[6][0]));
uycpu::vu0::fmac::<2, 15>(c, 8, 9, uycpu::vu0::bcast(c.vf[7][0]));
uycpu::vu0::fmac::<6, 15>(c, 32, 10, uycpu::vu0::bcast(c.vf[6][0]));
uycpu::vu0::fmac::<2, 15>(c, 10, 11, uycpu::vu0::bcast(c.vf[7][0]));
{ let v = c.vf[8]; c.wr128((c.r[12] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[4] = (((c.r[4] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
{ let v = c.vf[10]; c.wr128((c.r[12] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 l = 0x0043d4b4; continue;
}
0x0043d53c => {
 l = 0x0043d648; continue;
}
0x0043d544 => {
c.r[12] = (((c.r[10] as u32) << 3) as i32 as i64 as u64);
c.r[12] = (((c.r[12] as u32).wrapping_add((c.r[1] as u32))) as i32 as i64 as u64);
uycpu::vu0::fmac::<6, 15>(c, 9, 0, uycpu::vu0::bcast(c.vf[0][0]));
l = 0x0043d550; continue; }
0x0043d550 => {
let t = c.r[2] == 0u64;  c.r[7] = c.rd64((c.r[12] as u32));
 if t { l = 0x0043d5a0; continue; }
c.r[8] = c.rd8((c.r[12] as u32).wrapping_add(0x00000006)) as u64;
c.r[2] = (((c.r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[9] = a; c.rh[9] = b; }
{ let (a, b) = uycpu::mmi::psrlw((c.r[9], c.rh[9]), 13); c.r[9] = a; c.rh[9] = b; }
c.vf[8] = [c.r[9] as u32, (c.r[9] >> 32) as u32, c.rh[9] as u32, (c.rh[9] >> 32) as u32];
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
let t = (c.r[7] as i64) >= 0;  c.r[9] = (((c.r[8] as u32) << 6) as i32 as i64 as u64);
 if t { l = 0x0043d550; continue; }
uycpu::vu0::itof::<3, 14>(c, 9, 8);
c.r[9] = (((c.r[9] as u32).wrapping_add((c.r[14] as u32))) as i32 as i64 as u64);
c.r[7] = c.r[7] >> 62;
c.r[7] = (((c.r[7] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
{ let v = c.vf[9]; c.wr128((c.r[9] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.wr32((c.r[9] as u32).wrapping_add(0x0000001c), c.r[7] as u32);
 l = 0x0043d550; continue;
}
0x0043d5a0 => {
c.r[9] = c.r[14].wrapping_add(0u64);
uycpu::vu0::mov::<1>(c, 9, c.vf[0]);
l = 0x0043d5a8; continue; }
0x0043d5a8 => {
let t = c.r[3] == 0u64;  c.r[7] = c.rd64((c.r[12] as u32));
 if t { l = 0x0043d5f0; continue; }
c.r[8] = c.rd8((c.r[12] as u32).wrapping_add(0x00000006)) as u64;
c.r[3] = (((c.r[3] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
c.vf[8] = [c.r[7] as u32, (c.r[7] >> 32) as u32, c.rh[7] as u32, (c.rh[7] >> 32) as u32];
c.r[8] = (((c.r[8] as u32) << 6) as i32 as i64 as u64);
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
c.r[8] = (((c.r[8] as u32).wrapping_add((c.r[14] as u32))) as i32 as i64 as u64);
uycpu::vu0::itof::<0, 14>(c, 9, 8);
c.r[7] = c.rd32((c.r[8] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
{ let v = c.vf[0]; c.wr128((c.r[8] as u32).wrapping_add(0x00000030), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[9]; c.wr128((c.r[8] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.wr32((c.r[8] as u32).wrapping_add(0x0000002c), c.r[7] as u32);
 l = 0x0043d5a8; continue;
}
0x0043d5f0 => {
c.r[2] = c.rd8((c.r[25] as u32).wrapping_add(0x0000007f)) as u64;
c.r[12] = (((c.r[14] as u32).wrapping_add(0xffffff80)) as i32 as i64 as u64);
l = 0x0043d5f8; continue; }
0x0043d5f8 => {
let t = (c.r[2] as i64) <= 0;  { let (a, b) = c.rd128((c.r[1] as u32)); c.r[7] = a; c.rh[7] = b; }
 if t { l = 0x0043d640; continue; }
c.r[2] = (((c.r[2] as u32).wrapping_add(0xfffffffe)) as i32 as i64 as u64);
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000080)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[5] = a; c.rh[5] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[5], c.rh[5]), 16); c.r[5] = a; c.rh[5] = b; }
{ let (a, b) = uycpu::mmi::pextuh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
c.vf[8] = [c.r[5] as u32, (c.r[5] >> 32) as u32, c.rh[5] as u32, (c.rh[5] >> 32) as u32];
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
c.vf[10] = [c.r[7] as u32, (c.r[7] >> 32) as u32, c.rh[7] as u32, (c.rh[7] >> 32) as u32];
uycpu::vu0::itof::<3, 15>(c, 8, 8);
uycpu::vu0::itof::<3, 15>(c, 10, 10);
{ let v = c.vf[8]; c.wr128((c.r[12] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
{ let v = c.vf[10]; c.wr128((c.r[12] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 l = 0x0043d5f8; continue;
}
0x0043d640 => {
 l = 0x0043d648; continue;
}
0x0043d648 => {
c.r[15] = c.rd32((c.r[24] as u32).wrapping_add(0x00000050)) as i32 as i64 as u64;
l = 0x0043d650; continue; }
0x0043d650 => {
let t = c.r[15] == 0u64;  c.r[1] = 0x0000000010000000;
 if t { l = 0x0043d790; continue; }
c.r[14] = c.rd16((c.r[15] as u32).wrapping_add(0x00000002)) as u64;
c.r[1] = c.r[1] | 0xd400;
c.r[13] = c.rd32((c.r[15] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64;
c.r[2] = (((c.r[14] as u32).wrapping_add((c.r[14] as u32))) as i32 as i64 as u64);
c.wr32((c.r[1] as u32).wrapping_add(0x00000010), c.r[13] as u32);
c.r[14] = (((c.r[14] as u32).wrapping_add((c.r[2] as u32))) as i32 as i64 as u64);
c.wr32((c.r[1] as u32).wrapping_add(0x00000020), c.r[14] as u32);
c.r[2] = ((0u32.wrapping_add(0x00002000)) as i32 as i64 as u64);
c.wr32((c.r[1] as u32).wrapping_add(0x00000080), c.r[2] as u32);
c.r[2] = ((0u32.wrapping_add(0x00000100)) as i32 as i64 as u64);
c.wr32((c.r[1] as u32), c.r[2] as u32);
c.r[14] = (((c.r[14] as u32) << 4) as i32 as i64 as u64);
c.r[2] = c.rd32((c.r[15] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
c.r[12] = 0x0000000070000000;
c.r[13] = (((c.r[12] as u32).wrapping_add(0x00002000)) as i32 as i64 as u64);
c.r[14] = (((c.r[14] as u32).wrapping_add((c.r[13] as u32))) as i32 as i64 as u64);
c.vf[7] = [c.r[2] as u32, (c.r[2] >> 32) as u32, c.rh[2] as u32, (c.rh[2] >> 32) as u32];
c.r[15] = c.rd32((c.r[15] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
uycpu::vu0::fmac::<1, 8>(c, 6, 4, c.vf[7]);
l = 0x0043d6a8; continue; }
0x0043d6a8 => {
c.r[2] = c.rd32((c.r[1] as u32)) as i32 as i64 as u64;
c.r[2] = c.r[2] & 0x100;
let t = c.r[2] != 0u64;   if t { l = 0x0043d6a8; continue; }
l = 0x0043d6c0; continue; }
0x0043d6c0 => {
c.r[1] = c.rd32((c.r[13] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
c.r[13] = (((c.r[13] as u32).wrapping_add(0x00000030)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[13] as u32).wrapping_add(0xffffffd0)); c.vf[13] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[13] as u32).wrapping_add(0xffffffe0)); c.vf[14] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[13] as u32).wrapping_add(0xfffffff0)); c.vf[15] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[1] as u32)); c.vf[10] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[1] as u32).wrapping_add(0x00000010)); c.vf[11] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[1] as u32).wrapping_add(0x00000020)); c.vf[12] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::fmac::<6, 15>(c, 10, 10, uycpu::vu0::bcast(c.vf[6][0]));
uycpu::vu0::fmac::<6, 15>(c, 13, 13, uycpu::vu0::bcast(c.vf[7][0]));
uycpu::vu0::fmac::<6, 14>(c, 11, 11, uycpu::vu0::bcast(c.vf[6][0]));
uycpu::vu0::fmac::<6, 14>(c, 14, 14, uycpu::vu0::bcast(c.vf[7][0]));
uycpu::vu0::fmac::<6, 14>(c, 12, 12, uycpu::vu0::bcast(c.vf[6][0]));
uycpu::vu0::fmac::<6, 14>(c, 15, 15, uycpu::vu0::bcast(c.vf[7][0]));
uycpu::vu0::fmac::<0, 15>(c, 16, 10, c.vf[13]);
uycpu::vu0::fmac::<6, 15>(c, 18, 10, c.vf[13]);
uycpu::vu0::fmac::<0, 14>(c, 11, 11, c.vf[14]);
uycpu::vu0::fmac::<0, 14>(c, 12, 12, c.vf[15]);
uycpu::vu0::fmac::<6, 15>(c, 17, 16, c.vf[16]);
uycpu::vu0::fmac::<0, 4>(c, 32, 18, uycpu::vu0::bcast(c.vf[18][0]));
uycpu::vu0::fmac::<2, 4>(c, 32, 4, uycpu::vu0::bcast(c.vf[18][2]));
uycpu::vu0::fmac::<2, 4>(c, 18, 4, uycpu::vu0::bcast(c.vf[18][3]));
uycpu::vu0::fmac::<0, 8>(c, 32, 17, uycpu::vu0::bcast(c.vf[17][1]));
uycpu::vu0::fmac::<2, 8>(c, 32, 4, uycpu::vu0::bcast(c.vf[17][2]));
uycpu::vu0::fmac::<2, 8>(c, 17, 4, uycpu::vu0::bcast(c.vf[17][3]));
{ let (a, b) = { let v = c.vf[18]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; c.r[2] = a; c.rh[2] = b; }
let t = (c.r[2] as i64) >= 0;   if t { l = 0x0043d748; continue; }
uycpu::vu0::fmac::<1, 15>(c, 16, 10, c.vf[13]);
uycpu::vu0::fmac::<6, 15>(c, 17, 16, c.vf[16]);
uycpu::vu0::fmac::<0, 8>(c, 32, 17, uycpu::vu0::bcast(c.vf[17][1]));
uycpu::vu0::fmac::<2, 8>(c, 32, 4, uycpu::vu0::bcast(c.vf[17][2]));
uycpu::vu0::fmac::<2, 8>(c, 17, 4, uycpu::vu0::bcast(c.vf[17][3]));
l = 0x0043d748; continue; }
0x0043d748 => {
uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[17][0]);
{ let v = c.vf[11]; c.wr128((c.r[1] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[12]; c.wr128((c.r[1] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[2] = c.rd32((c.r[1] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
c.r[3] = c.rd32((c.r[13] as u32).wrapping_add(0xffffffec)) as i32 as i64 as u64;
let t = c.r[2] != 0u64;   if t { l = 0x0043d778; continue; }
c.wr32((c.r[1] as u32).wrapping_add(0x0000001c), c.r[3] as u32);
l = 0x0043d778; continue; }
0x0043d778 => {
uycpu::vu0::fmac::<6, 15>(c, 10, 16, uycpu::vu0::bcast(c.vi[22]));
let t = c.r[13] != c.r[14];  { let v = c.vf[10]; c.wr128((c.r[1] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043d6c0; continue; }
let t = c.r[15] != 0u64;   if t { l = 0x0043d650; continue; }
l = 0x0043d790; continue; }
0x0043d790 => {
c.r[15] = c.rd32((c.r[24] as u32).wrapping_add(0x00000054)) as i32 as i64 as u64;
l = 0x0043d798; continue; }
0x0043d798 => {
let t = c.r[15] == 0u64;  c.r[3] = (((c.r[15] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
 if t { l = 0x0043d8a8; continue; }
c.r[1] = c.rd32((c.r[3] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
c.r[2] = c.rd8((c.r[3] as u32).wrapping_add(0x00000003)) as u64;
{ let (a, b) = c.rd128((c.r[1] as u32)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[1] as u32).wrapping_add(0x00000010)); c.vf[8] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[1] as u32).wrapping_add(0x00000020)); c.vf[10] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[3] as u32).wrapping_add(0x00000010)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[3] as u32).wrapping_add(0x00000020)); c.vf[9] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[3] as u32).wrapping_add(0x00000030)); c.vf[11] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
let t = c.r[2] != 0u64;  c.r[15] = c.rd32((c.r[3] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
 if t { l = 0x0043d800; continue; }
uycpu::vu0::fmac::<6, 1>(c, 12, 7, c.vf[6]);
uycpu::vu0::fmac::<6, 14>(c, 13, 7, c.vf[6]);
uycpu::vu0::fmac::<6, 14>(c, 14, 7, uycpu::vu0::bcast(c.vf[6][3]));
uycpu::vu0::fmac::<6, 14>(c, 15, 6, uycpu::vu0::bcast(c.vf[7][3]));
uycpu::vu0::opmula(c, 6, 7);
uycpu::vu0::opmsub(c, 16, 7, 6);
uycpu::vu0::fmac::<0, 8>(c, 32, 13, uycpu::vu0::bcast(c.vf[13][1]));
uycpu::vu0::fmac::<2, 8>(c, 13, 4, uycpu::vu0::bcast(c.vf[13][2]));
uycpu::vu0::fmac::<0, 14>(c, 6, 14, c.vf[15]);
uycpu::vu0::fmac::<0, 14>(c, 6, 6, c.vf[16]);
uycpu::vu0::fmac::<1, 1>(c, 6, 12, uycpu::vu0::bcast(c.vf[13][0]));
uycpu::vu0::fmac::<6, 14>(c, 8, 8, c.vf[9]);
uycpu::vu0::fmac::<0, 14>(c, 10, 10, c.vf[11]);
 l = 0x0043d890; continue;
}
0x0043d800 => {
c.r[3] = c.rd32((c.r[3] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
c.vf[18] = [c.r[3] as u32, (c.r[3] >> 32) as u32, c.rh[3] as u32, (c.rh[3] >> 32) as u32];
uycpu::vu0::fmac::<1, 1>(c, 18, 0, uycpu::vu0::bcast(c.vf[18][0]));
uycpu::vu0::fmac::<6, 15>(c, 6, 6, uycpu::vu0::bcast(c.vf[18][3]));
uycpu::vu0::fmac::<6, 15>(c, 7, 7, uycpu::vu0::bcast(c.vf[18][0]));
uycpu::vu0::fmac::<6, 14>(c, 8, 8, uycpu::vu0::bcast(c.vf[18][3]));
uycpu::vu0::fmac::<6, 14>(c, 9, 9, uycpu::vu0::bcast(c.vf[18][0]));
uycpu::vu0::fmac::<6, 14>(c, 10, 10, uycpu::vu0::bcast(c.vf[18][3]));
uycpu::vu0::fmac::<6, 14>(c, 11, 11, uycpu::vu0::bcast(c.vf[18][0]));
uycpu::vu0::fmac::<0, 15>(c, 14, 6, c.vf[7]);
uycpu::vu0::fmac::<6, 15>(c, 16, 6, c.vf[7]);
uycpu::vu0::fmac::<0, 14>(c, 8, 8, c.vf[9]);
uycpu::vu0::fmac::<0, 14>(c, 10, 10, c.vf[11]);
uycpu::vu0::fmac::<6, 15>(c, 15, 14, c.vf[14]);
uycpu::vu0::fmac::<0, 4>(c, 32, 16, uycpu::vu0::bcast(c.vf[16][0]));
uycpu::vu0::fmac::<2, 4>(c, 32, 4, uycpu::vu0::bcast(c.vf[16][2]));
uycpu::vu0::fmac::<2, 4>(c, 16, 4, uycpu::vu0::bcast(c.vf[16][3]));
uycpu::vu0::fmac::<0, 8>(c, 32, 15, uycpu::vu0::bcast(c.vf[15][1]));
uycpu::vu0::fmac::<2, 8>(c, 32, 4, uycpu::vu0::bcast(c.vf[15][2]));
uycpu::vu0::fmac::<2, 8>(c, 15, 4, uycpu::vu0::bcast(c.vf[15][3]));
{ let (a, b) = { let v = c.vf[16]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; c.r[2] = a; c.rh[2] = b; }
let t = (c.r[2] as i64) >= 0;   if t { l = 0x0043d878; continue; }
uycpu::vu0::fmac::<1, 15>(c, 14, 6, c.vf[7]);
uycpu::vu0::fmac::<6, 15>(c, 15, 14, c.vf[14]);
uycpu::vu0::fmac::<0, 8>(c, 32, 15, uycpu::vu0::bcast(c.vf[15][1]));
uycpu::vu0::fmac::<2, 8>(c, 32, 4, uycpu::vu0::bcast(c.vf[15][2]));
uycpu::vu0::fmac::<2, 8>(c, 15, 4, uycpu::vu0::bcast(c.vf[15][3]));
l = 0x0043d878; continue; }
0x0043d878 => {
uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[15][0]);
uycpu::vu0::fmac::<6, 15>(c, 6, 14, uycpu::vu0::bcast(c.vi[22]));
l = 0x0043d890; continue; }
0x0043d890 => {
{ let v = c.vf[6]; c.wr128((c.r[1] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[8]; c.wr128((c.r[1] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[10]; c.wr128((c.r[1] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[2] = ((0u32.wrapping_add(0x00000001)) as i32 as i64 as u64);
let t = c.r[15] != 0u64;  c.wr32((c.r[1] as u32).wrapping_add(0x0000001c), c.r[2] as u32);
 if t { l = 0x0043d798; continue; }
l = 0x0043d8a8; continue; }
0x0043d8a8 => {
c.r[12] = c.rd8((c.r[25] as u32).wrapping_add(0x0000007f)) as i8 as i64 as u64;
c.r[14] = 0x0000000070000000;
c.r[12] = (((c.r[12] as u32) << 6) as i32 as i64 as u64);
c.r[7] = c.r[14].wrapping_add(0u64);
c.r[12] = (((c.r[12] as u32).wrapping_add((c.r[14] as u32))) as i32 as i64 as u64);
c.r[5] = c.r[25].wrapping_add(0u64);
l = 0x0043d8c0; continue; }
0x0043d8c0 => {
c.r[6] = c.rd8((c.r[5] as u32)) as i8 as i64 as u64;
c.r[5] = (((c.r[5] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[7] as u32)); c.vf[10] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
c.r[7] = (((c.r[7] as u32).wrapping_add(0x00000040)) as i32 as i64 as u64);
let t = c.r[6] == 0u64;   if t { l = 0x0043d8c0; continue; }
uycpu::vu0::mov::<15>(c, 16, c.vf[1]);
uycpu::vu0::fmac::<0, 15>(c, 11, 10, c.vf[10]);
uycpu::vu0::mov::<15>(c, 17, c.vf[2]);
uycpu::vu0::mov::<15>(c, 18, c.vf[3]);
uycpu::vu0::mov::<15>(c, 19, c.vf[0]);
uycpu::vu0::fmac::<6, 14>(c, 12, 11, uycpu::vu0::bcast(c.vf[10][3]));
uycpu::vu0::fmac::<6, 14>(c, 13, 11, uycpu::vu0::bcast(c.vf[10][0]));
uycpu::vu0::fmac::<6, 6>(c, 14, 11, uycpu::vu0::bcast(c.vf[10][1]));
uycpu::vu0::fmac::<6, 2>(c, 15, 11, uycpu::vu0::bcast(c.vf[10][2]));
uycpu::vu0::fmac::<0, 8>(c, 17, 0, uycpu::vu0::bcast(c.vf[12][2]));
uycpu::vu0::fmac::<1, 8>(c, 18, 0, uycpu::vu0::bcast(c.vf[12][1]));
uycpu::vu0::fmac::<0, 4>(c, 18, 0, uycpu::vu0::bcast(c.vf[12][0]));
uycpu::vu0::fmac::<1, 8>(c, 16, 16, uycpu::vu0::bcast(c.vf[14][1]));
uycpu::vu0::fmac::<1, 4>(c, 17, 17, uycpu::vu0::bcast(c.vf[13][0]));
uycpu::vu0::fmac::<1, 2>(c, 18, 18, uycpu::vu0::bcast(c.vf[13][0]));
uycpu::vu0::fmac::<1, 4>(c, 16, 13, uycpu::vu0::bcast(c.vf[12][2]));
uycpu::vu0::fmac::<0, 2>(c, 16, 13, uycpu::vu0::bcast(c.vf[12][1]));
uycpu::vu0::fmac::<1, 2>(c, 17, 14, uycpu::vu0::bcast(c.vf[12][0]));
uycpu::vu0::fmac::<0, 8>(c, 17, 17, uycpu::vu0::bcast(c.vf[13][1]));
uycpu::vu0::fmac::<0, 8>(c, 18, 18, uycpu::vu0::bcast(c.vf[13][2]));
uycpu::vu0::fmac::<0, 4>(c, 18, 18, uycpu::vu0::bcast(c.vf[14][2]));
uycpu::vu0::fmac::<1, 8>(c, 16, 16, uycpu::vu0::bcast(c.vf[15][2]));
c.r[8] = c.rd32((c.r[7] as u32).wrapping_add(0xffffffdc)) as i32 as i64 as u64;
uycpu::vu0::fmac::<1, 4>(c, 17, 17, uycpu::vu0::bcast(c.vf[15][2]));
c.r[9] = c.rd32((c.r[7] as u32).wrapping_add(0xffffffec)) as i32 as i64 as u64;
uycpu::vu0::fmac::<1, 2>(c, 18, 18, uycpu::vu0::bcast(c.vf[14][1]));
{ let (a, b) = c.rd128((c.r[7] as u32).wrapping_add(0xffffffd0)); c.vf[8] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
let t = c.r[8] == 0u64;  { let (a, b) = c.rd128((c.r[7] as u32).wrapping_add(0xffffffe0)); c.vf[9] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
 if t { l = 0x0043d95c; continue; }
uycpu::vu0::fmac::<6, 14>(c, 16, 16, uycpu::vu0::bcast(c.vf[8][0]));
uycpu::vu0::fmac::<6, 14>(c, 17, 17, uycpu::vu0::bcast(c.vf[8][1]));
uycpu::vu0::fmac::<6, 14>(c, 18, 18, uycpu::vu0::bcast(c.vf[8][2]));
l = 0x0043d95c; continue; }
0x0043d95c => {
let t = c.r[9] != 0u64;  uycpu::vu0::mov::<15>(c, 12, c.vf[1]);
 if t { l = 0x0043d974; continue; }
uycpu::vu0::mov::<15>(c, 13, c.vf[2]);
uycpu::vu0::mov::<15>(c, 14, c.vf[3]);
uycpu::vu0::mov::<15>(c, 15, c.vf[0]);
 l = 0x0043d984; continue;
}
0x0043d974 => {
{ let (a, b) = c.rd128((c.r[9] as u32)); c.vf[12] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[9] as u32).wrapping_add(0x00000010)); c.vf[13] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[9] as u32).wrapping_add(0x00000020)); c.vf[14] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[9] as u32).wrapping_add(0x00000030)); c.vf[15] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
l = 0x0043d984; continue; }
0x0043d984 => {
uycpu::vu0::fmac::<0, 14>(c, 19, 19, c.vf[9]);
uycpu::vu0::fmac::<6, 15>(c, 32, 12, uycpu::vu0::bcast(c.vf[16][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 13, uycpu::vu0::bcast(c.vf[16][1]));
uycpu::vu0::fmac::<2, 15>(c, 20, 14, uycpu::vu0::bcast(c.vf[16][2]));
uycpu::vu0::fmac::<6, 15>(c, 32, 12, uycpu::vu0::bcast(c.vf[17][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 13, uycpu::vu0::bcast(c.vf[17][1]));
uycpu::vu0::fmac::<2, 15>(c, 21, 14, uycpu::vu0::bcast(c.vf[17][2]));
uycpu::vu0::fmac::<6, 15>(c, 32, 12, uycpu::vu0::bcast(c.vf[18][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 13, uycpu::vu0::bcast(c.vf[18][1]));
uycpu::vu0::fmac::<2, 15>(c, 22, 14, uycpu::vu0::bcast(c.vf[18][2]));
uycpu::vu0::fmac::<6, 15>(c, 32, 12, uycpu::vu0::bcast(c.vf[19][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 13, uycpu::vu0::bcast(c.vf[19][1]));
uycpu::vu0::fmac::<2, 15>(c, 32, 14, uycpu::vu0::bcast(c.vf[19][2]));
uycpu::vu0::fmac::<2, 15>(c, 23, 15, uycpu::vu0::bcast(c.vf[0][3]));
{ let v = c.vf[20]; c.wr128((c.r[7] as u32).wrapping_add(0xffffffc0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[21]; c.wr128((c.r[7] as u32).wrapping_add(0xffffffd0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[22]; c.wr128((c.r[7] as u32).wrapping_add(0xffffffe0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
let t = c.r[7] != c.r[12];  { let v = c.vf[23]; c.wr128((c.r[7] as u32).wrapping_add(0xfffffff0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043d8c0; continue; }
l = 0x0043d9d0; continue; }
0x0043d9d0 => {
let t = (c.r[31] as u32);  c.ret = t; return;
}
_ => panic!("the animation unit: no label {l:08x} in 0x0043CF90"), } }
}

#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn n0043d9d8(c: &mut Cpu, k: F) {
let mut l: u32 = 0x0043d9d8;
loop { match l {
0x0043d9d8 => {
{ let (a, b) = uycpu::mmi::pcpyld((c.r[31], c.rh[31]), (0u64, 0u64)); c.r[31] = a; c.rh[31] = b; }
c.r[15] = c.rd32((c.r[4] as u32).wrapping_add(0x00000024)) as i32 as i64 as u64;
c.r[24] = (((c.r[4] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
c.r[12] = c.rd32((c.r[24] as u32).wrapping_add(0x00000058)) as i32 as i64 as u64;
c.r[8] = 0x0000000010000000;
c.r[13] = c.rd32((c.r[24] as u32).wrapping_add(0x0000005c)) as i32 as i64 as u64;
c.r[8] = c.r[8] | 0xd400;
c.r[11] = c.rd32((c.r[15] as u32).wrapping_add(0x00000018)) as i32 as i64 as u64;
uycpu::vu0::fmac::<0, 14>(c, 5, 0, uycpu::vu0::bcast(c.vf[0][3]));
c.r[10] = c.rd8((c.r[15] as u32).wrapping_add(0x00000008)) as u64;
c.r[1] = 0u64 | 0x2000;
c.wr32((c.r[8] as u32).wrapping_add(0x00000080), c.r[1] as u32);
c.r[25] = 0x0000000070000000;
c.wr32((c.r[8] as u32).wrapping_add(0x00000010), c.r[11] as u32);
c.r[15] = (((c.r[10] as u32) << 4) as i32 as i64 as u64);
c.wr32((c.r[8] as u32).wrapping_add(0x00000020), c.r[10] as u32);
c.r[15] = (((c.r[15] as u32).wrapping_add((c.r[25] as u32))) as i32 as i64 as u64);
c.wr64((c.r[15] as u32).wrapping_add(0x00001ff8), 0u64);
c.r[1] = 0u64 | 0x100;
c.wr32((c.r[8] as u32), c.r[1] as u32);
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
c.r[4] = c.rd16((c.r[12] as u32).wrapping_add(0xfffffff6)) as i16 as i64 as u64;
c.r[13] = (((c.r[13] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
c.r[9] = c.rd32((c.r[24] as u32).wrapping_add(0x00000044)) as i32 as i64 as u64;
uycpu::vu0::fmac::<6, 1>(c, 5, 0, uycpu::vu0::bcast(c.vf[0][0]));
c.r[14] = (((c.r[4] as u32) << 4) as i32 as i64 as u64);
uycpu::vu0::fmac::<6, 15>(c, 1, 0, uycpu::vu0::bcast(c.vf[0][0]));
c.r[2] = c.rd16((c.r[12] as u32).wrapping_add(0xfffffffa)) as i16 as i64 as u64;
uycpu::vu0::fmac::<6, 15>(c, 2, 0, uycpu::vu0::bcast(c.vf[0][0]));
c.r[14] = (((c.r[14] as u32).wrapping_add((c.r[25] as u32))) as i32 as i64 as u64);
{ let v = c.vf[0]; uycpu::vu0::mov::<15>(c, 3, [v[1], v[2], v[3], v[0]]); }
c.r[3] = c.rd16((c.r[12] as u32).wrapping_add(0xfffffffe)) as i16 as i64 as u64;
uycpu::vu0::fmac::<0, 8>(c, 1, 1, uycpu::vu0::bcast(c.vf[0][3]));
c.wr128((c.r[14] as u32).wrapping_add(0x00002ff0), 0u64, 0u64);
uycpu::vu0::fmac::<0, 4>(c, 2, 2, uycpu::vu0::bcast(c.vf[0][3]));
c.r[5] = (((c.r[25] as u32).wrapping_add(0xffffff80)) as i32 as i64 as u64);
c.r[6] = (((c.r[10] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
l = 0x0043da70; continue; }
0x0043da70 => {
{ let v = c.vf[5]; c.wr128((c.r[5] as u32).wrapping_add(0x00000080), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[5]; c.wr128((c.r[5] as u32).wrapping_add(0x00000090), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[5] = (((c.r[5] as u32).wrapping_add(0x00000080)) as i32 as i64 as u64);
{ let v = c.vf[5]; c.wr128((c.r[5] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[6] = (((c.r[6] as u32).wrapping_add(0xfffffffe)) as i32 as i64 as u64);
let t = (c.r[6] as i64) > 0;  { let v = c.vf[5]; c.wr128((c.r[5] as u32).wrapping_add(0x00000050), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043da70; continue; }
l = 0x0043da90; continue; }
0x0043da90 => {
c.r[1] = c.rd64((c.r[15] as u32).wrapping_add(0x00001ff8));
c.r[5] = (((c.r[25] as u32).wrapping_add(0x00003000)) as i32 as i64 as u64);
c.r[6] = 0u64 | 0x100;
let t = c.r[1] == 0u64;   if t { l = 0x0043da90; continue; }
c.wr32((c.r[8] as u32).wrapping_add(0x00000080), c.r[5] as u32);
uycpu::vu0::fmac::<6, 1>(c, 9, 0, uycpu::vu0::bcast(c.vf[0][0]));
c.wr32((c.r[8] as u32).wrapping_add(0x00000010), c.r[12] as u32);
uycpu::vu0::fmac::<6, 1>(c, 10, 0, uycpu::vu0::bcast(c.vf[0][0]));
c.wr32((c.r[8] as u32).wrapping_add(0x00000020), c.r[4] as u32);
uycpu::vu0::fmac::<6, 1>(c, 11, 0, uycpu::vu0::bcast(c.vf[0][0]));
c.wr32((c.r[8] as u32), c.r[6] as u32);
uycpu::vu0::fmac::<6, 1>(c, 12, 0, uycpu::vu0::bcast(c.vf[0][0]));
c.r[5] = (((c.r[10] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
c.r[6] = (((c.r[25] as u32).wrapping_add(0x00002000)) as i32 as i64 as u64);
c.r[7] = (((c.r[25] as u32).wrapping_add(0xffffff00)) as i32 as i64 as u64);
l = 0x0043dae0; continue; }
0x0043dae0 => {
{ let (a, b) = c.rd128((c.r[6] as u32)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
c.r[7] = (((c.r[7] as u32).wrapping_add(0x00000100)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[6] as u32).wrapping_add(0x00000010)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
c.r[5] = (((c.r[5] as u32).wrapping_add(0xfffffffc)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[6] as u32).wrapping_add(0x00000020)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::fmac::<6, 14>(c, 9, 5, uycpu::vu0::bcast(c.vf[0][3]));
{ let (a, b) = c.rd128((c.r[6] as u32).wrapping_add(0x00000030)); c.vf[8] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::fmac::<6, 14>(c, 10, 6, uycpu::vu0::bcast(c.vf[0][3]));
{ let v = c.vf[5]; c.wr128((c.r[7] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
uycpu::vu0::fmac::<6, 14>(c, 11, 7, uycpu::vu0::bcast(c.vf[0][3]));
{ let v = c.vf[6]; c.wr128((c.r[7] as u32).wrapping_add(0x00000060), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
uycpu::vu0::fmac::<6, 14>(c, 12, 8, uycpu::vu0::bcast(c.vf[0][3]));
{ let v = c.vf[7]; c.wr128((c.r[7] as u32).wrapping_add(0x000000a0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[8]; c.wr128((c.r[7] as u32).wrapping_add(0x000000e0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[9]; c.wr128((c.r[7] as u32).wrapping_add(0x00000030), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[10]; c.wr128((c.r[7] as u32).wrapping_add(0x00000070), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[11]; c.wr128((c.r[7] as u32).wrapping_add(0x000000b0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[6] = (((c.r[6] as u32).wrapping_add(0x00000040)) as i32 as i64 as u64);
let t = (c.r[5] as i64) > 0;  { let v = c.vf[12]; c.wr128((c.r[7] as u32).wrapping_add(0x000000f0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043dae0; continue; }
let t = c.r[9] == 0u64;   if t { l = 0x0043e068; continue; }
c.r[7] = c.rd8((c.r[24] as u32).wrapping_add(0x00000042)) as u64;
c.r[8] = c.rd8((c.r[24] as u32).wrapping_add(0x00000043)) as u64;
c.r[5] = c.rd8((c.r[24] as u32).wrapping_add(0x00000040)) as u64;
c.r[6] = c.rd8((c.r[24] as u32).wrapping_add(0x00000041)) as u64;
c.r[31] = (((c.r[7] as u32).wrapping_sub((c.r[8] as u32))) as i32 as i64 as u64);
c.r[7] = c.rd16((c.r[13] as u32).wrapping_add(0xfffffff6)) as i16 as i64 as u64;
c.r[5] = (((c.r[5] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.vf[4] = [c.r[9] as u32, (c.r[9] >> 32) as u32, c.rh[9] as u32, (c.rh[9] >> 32) as u32];
c.r[5] = (((c.r[5] as u32).wrapping_sub((c.r[6] as u32))) as i32 as i64 as u64);
c.r[31] = c.r[31] | c.r[5];
c.r[15] = (((c.r[7] as u32) << 4) as i32 as i64 as u64);
c.r[5] = c.rd16((c.r[13] as u32).wrapping_add(0xfffffffa)) as i16 as i64 as u64;
c.r[15] = (((c.r[15] as u32).wrapping_add((c.r[25] as u32))) as i32 as i64 as u64);
c.r[6] = c.rd16((c.r[13] as u32).wrapping_add(0xfffffffe)) as i16 as i64 as u64;
c.wr128((c.r[15] as u32).wrapping_add(0x000035f0), 0u64, 0u64);
uycpu::vu0::fmac::<1, 1>(c, 4, 0, uycpu::vu0::bcast(c.vf[4][0]));
l = 0x0043db80; continue; }
0x0043db80 => {
c.r[1] = c.rd64((c.r[14] as u32).wrapping_add(0x00002ff0));
c.r[8] = 0x0000000010000000;
c.r[8] = c.r[8] | 0xd400;
let t = c.r[1] == 0u64;   if t { l = 0x0043db80; continue; }
c.wr32((c.r[8] as u32).wrapping_add(0x00000010), c.r[13] as u32);
c.r[1] = (((c.r[25] as u32).wrapping_add(0x00003600)) as i32 as i64 as u64);
c.wr32((c.r[8] as u32).wrapping_add(0x00000080), c.r[1] as u32);
c.r[14] = (((c.r[25] as u32).wrapping_add(0x00003d00)) as i32 as i64 as u64);
c.wr32((c.r[8] as u32).wrapping_add(0x00000020), c.r[7] as u32);
c.r[1] = 0u64 | 0x100;
c.wr32((c.r[8] as u32), c.r[1] as u32);
c.r[13] = (((c.r[25] as u32).wrapping_add(0x00003c00)) as i32 as i64 as u64);
c.r[7] = (((c.r[10] as u32) << 3) as i32 as i64 as u64);
c.r[4] = (((c.r[25] as u32).wrapping_add(0x00003000)) as i32 as i64 as u64);
uycpu::vu0::fmac::<6, 15>(c, 5, 0, uycpu::vu0::bcast(c.vf[0][0]));
c.r[4] = (((c.r[4] as u32).wrapping_add((c.r[7] as u32))) as i32 as i64 as u64);
let t = c.r[2] == 0u64;  c.r[7] = c.rd64((c.r[4] as u32));
 if t { l = 0x0043dc38; continue; }
c.r[8] = c.rd8((c.r[4] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[9] = a; c.rh[9] = b; }
{ let (a, b) = uycpu::mmi::psrlw((c.r[9], c.rh[9]), 13); c.r[9] = a; c.rh[9] = b; }
l = 0x0043dbe8; continue; }
0x0043dbe8 => {
let t = (c.r[2] as i64) <= 0;   if t { l = 0x0043dc38; continue; }
c.vf[6] = [c.r[9] as u32, (c.r[9] >> 32) as u32, c.rh[9] as u32, (c.rh[9] >> 32) as u32];
c.r[12] = c.r[7] >> 62;
c.r[11] = (((c.r[8] as u32) << 6) as i32 as i64 as u64);
c.r[7] = c.rd64((c.r[4] as u32).wrapping_add(0x00000008));
uycpu::vu0::itof::<3, 14>(c, 5, 6);
c.r[11] = (((c.r[11] as u32).wrapping_add((c.r[25] as u32))) as i32 as i64 as u64);
c.r[4] = (((c.r[4] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
c.r[2] = (((c.r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
c.r[8] = c.rd8((c.r[4] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[9] = a; c.rh[9] = b; }
let t = c.r[12] == 0u64;  { let (a, b) = uycpu::mmi::psrlw((c.r[9], c.rh[9]), 13); c.r[9] = a; c.rh[9] = b; }
 if t { l = 0x0043dbe8; continue; }
c.wr32((c.r[13] as u32), c.r[11] as u32);
c.r[13] = (((c.r[13] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
{ let v = c.vf[5]; c.wr128((c.r[11] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[1] = ((0u32.wrapping_add(0xfffffffc)) as i32 as i64 as u64);
let t = (c.r[2] as i64) > 0;  c.wr32((c.r[11] as u32).wrapping_add(0x0000000c), c.r[1] as u32);
 if t { l = 0x0043dbe8; continue; }
l = 0x0043dc38; continue; }
0x0043dc38 => {
let t = c.r[3] == 0u64;  c.r[7] = c.rd64((c.r[4] as u32));
 if t { l = 0x0043dc98; continue; }
c.r[8] = c.rd8((c.r[4] as u32).wrapping_add(0x00000006)) as u64;
uycpu::vu0::mov::<1>(c, 5, c.vf[0]);
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
l = 0x0043dc50; continue; }
0x0043dc50 => {
c.vf[6] = [c.r[7] as u32, (c.r[7] >> 32) as u32, c.rh[7] as u32, (c.rh[7] >> 32) as u32];
c.r[12] = (((c.r[8] as u32) << 6) as i32 as i64 as u64);
c.r[12] = (((c.r[12] as u32).wrapping_add((c.r[25] as u32))) as i32 as i64 as u64);
c.r[7] = c.rd64((c.r[4] as u32).wrapping_add(0x00000008));
uycpu::vu0::itof::<0, 14>(c, 5, 6);
c.r[4] = (((c.r[4] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
c.r[8] = c.rd8((c.r[4] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
c.wr32((c.r[14] as u32), c.r[12] as u32);
c.r[14] = (((c.r[14] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
c.r[11] = c.rd32((c.r[12] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
{ let v = c.vf[5]; c.wr128((c.r[12] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[3] = (((c.r[3] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
c.wr32((c.r[12] as u32).wrapping_add(0x0000002c), c.r[11] as u32);
c.r[1] = ((0u32.wrapping_add(0xfffffffc)) as i32 as i64 as u64);
let t = (c.r[3] as i64) > 0;  c.wr32((c.r[12] as u32).wrapping_add(0x0000003c), c.r[1] as u32);
 if t { l = 0x0043dc50; continue; }
l = 0x0043dc98; continue; }
0x0043dc98 => {
c.r[1] = c.rd64((c.r[15] as u32).wrapping_add(0x000035f0));
c.r[7] = (((c.r[10] as u32) << 3) as i32 as i64 as u64);
c.r[4] = (((c.r[25] as u32).wrapping_add(0x00003600)) as i32 as i64 as u64);
let t = c.r[1] == 0u64;  c.r[4] = (((c.r[4] as u32).wrapping_add((c.r[7] as u32))) as i32 as i64 as u64);
 if t { l = 0x0043dc98; continue; }
uycpu::vu0::fmac::<6, 15>(c, 5, 0, uycpu::vu0::bcast(c.vf[0][0]));
let t = c.r[5] == 0u64;  c.r[7] = c.rd64((c.r[4] as u32));
 if t { l = 0x0043dd28; continue; }
c.r[8] = c.rd8((c.r[4] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[9] = a; c.rh[9] = b; }
{ let (a, b) = uycpu::mmi::psrlw((c.r[9], c.rh[9]), 13); c.r[9] = a; c.rh[9] = b; }
l = 0x0043dcd8; continue; }
0x0043dcd8 => {
let t = (c.r[5] as i64) <= 0;   if t { l = 0x0043dd28; continue; }
c.vf[6] = [c.r[9] as u32, (c.r[9] >> 32) as u32, c.rh[9] as u32, (c.rh[9] >> 32) as u32];
c.r[12] = c.r[7] >> 62;
c.r[11] = (((c.r[8] as u32) << 6) as i32 as i64 as u64);
c.r[7] = c.rd64((c.r[4] as u32).wrapping_add(0x00000008));
uycpu::vu0::itof::<3, 14>(c, 5, 6);
c.r[11] = (((c.r[11] as u32).wrapping_add((c.r[25] as u32))) as i32 as i64 as u64);
c.r[8] = c.rd8((c.r[4] as u32).wrapping_add(0x0000000e)) as u64;
c.r[4] = (((c.r[4] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[9] = a; c.rh[9] = b; }
c.wr32((c.r[13] as u32), c.r[11] as u32);
{ let (a, b) = uycpu::mmi::psrlw((c.r[9], c.rh[9]), 13); c.r[9] = a; c.rh[9] = b; }
c.r[1] = c.rd32((c.r[11] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
let t = c.r[12] == 0u64;  c.r[5] = (((c.r[5] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x0043dcd8; continue; }
{ let v = c.vf[5]; c.wr128((c.r[11] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[13] = (((c.r[13] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
let t = c.r[5] != 0u64;  c.r[13] = (((c.r[13] as u32).wrapping_add((c.r[1] as u32))) as i32 as i64 as u64);
 if t { l = 0x0043dcd8; continue; }
l = 0x0043dd28; continue; }
0x0043dd28 => {
let t = c.r[6] == 0u64;  c.r[7] = c.rd64((c.r[4] as u32));
 if t { l = 0x0043dd80; continue; }
c.r[8] = c.rd8((c.r[4] as u32).wrapping_add(0x00000006)) as u64;
uycpu::vu0::mov::<1>(c, 5, c.vf[0]);
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
l = 0x0043dd40; continue; }
0x0043dd40 => {
c.vf[6] = [c.r[7] as u32, (c.r[7] >> 32) as u32, c.rh[7] as u32, (c.rh[7] >> 32) as u32];
c.r[12] = (((c.r[8] as u32) << 6) as i32 as i64 as u64);
c.r[12] = (((c.r[12] as u32).wrapping_add((c.r[25] as u32))) as i32 as i64 as u64);
c.r[7] = c.rd64((c.r[4] as u32).wrapping_add(0x00000008));
uycpu::vu0::itof::<0, 14>(c, 5, 6);
c.r[4] = (((c.r[4] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
c.r[8] = c.rd8((c.r[4] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((c.r[7], c.rh[7]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
c.wr32((c.r[14] as u32), c.r[12] as u32);
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
c.r[1] = c.rd32((c.r[12] as u32).wrapping_add(0x0000003c)) as i32 as i64 as u64;
c.r[14] = (((c.r[14] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
{ let v = c.vf[5]; c.wr128((c.r[12] as u32).wrapping_add(0x00000030), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[6] = (((c.r[6] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
let t = (c.r[6] as i64) > 0;  c.r[14] = (((c.r[14] as u32).wrapping_add((c.r[1] as u32))) as i32 as i64 as u64);
 if t { l = 0x0043dd40; continue; }
l = 0x0043dd80; continue; }
0x0043dd80 => {
c.r[1] = (((c.r[25] as u32).wrapping_add(0x00003c00)) as i32 as i64 as u64);
c.r[2] = c.rd32((c.r[1] as u32)) as i32 as i64 as u64;
c.r[3] = (((c.r[25] as u32).wrapping_add(0x00002000)) as i32 as i64 as u64);
let t = c.r[1] == c.r[13];  c.wr32((c.r[13] as u32), c.r[3] as u32);
 if t { l = 0x0043ddd0; continue; }
{ let (a, b) = c.rd128((c.r[2] as u32)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[2] as u32).wrapping_add(0x00000010)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
l = 0x0043dda0; continue; }
0x0043dda0 => {
uycpu::vu0::fmac::<6, 14>(c, 32, 5, uycpu::vu0::bcast(c.vf[4][3]));
c.r[3] = (((c.r[2] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
uycpu::vu0::fmac::<2, 14>(c, 7, 6, uycpu::vu0::bcast(c.vf[4][0]));
c.r[4] = c.rd32((c.r[2] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
c.r[2] = c.rd32((c.r[1] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[2] as u32)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[2] as u32).wrapping_add(0x00000010)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let v = c.vf[7]; c.wr128((c.r[3] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
let t = c.r[1] != c.r[13];  c.wr32((c.r[3] as u32).wrapping_add(0x0000001c), c.r[4] as u32);
 if t { l = 0x0043dda0; continue; }
l = 0x0043ddd0; continue; }
0x0043ddd0 => {
c.r[1] = (((c.r[25] as u32).wrapping_add(0x00003d00)) as i32 as i64 as u64);
c.r[2] = c.rd32((c.r[1] as u32)) as i32 as i64 as u64;
c.r[3] = (((c.r[25] as u32).wrapping_add(0x00002000)) as i32 as i64 as u64);
let t = c.r[1] == c.r[14];  c.wr32((c.r[14] as u32), c.r[3] as u32);
 if t { l = 0x0043de20; continue; }
{ let (a, b) = c.rd128((c.r[2] as u32).wrapping_add(0x00000020)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[2] as u32).wrapping_add(0x00000030)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
l = 0x0043ddf0; continue; }
0x0043ddf0 => {
uycpu::vu0::fmac::<6, 14>(c, 32, 5, uycpu::vu0::bcast(c.vf[4][3]));
c.r[3] = (((c.r[2] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
uycpu::vu0::fmac::<2, 14>(c, 7, 6, uycpu::vu0::bcast(c.vf[4][0]));
c.r[4] = c.rd32((c.r[2] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
c.r[2] = c.rd32((c.r[1] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[2] as u32).wrapping_add(0x00000020)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[2] as u32).wrapping_add(0x00000030)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let v = c.vf[7]; c.wr128((c.r[3] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
let t = c.r[1] != c.r[14];  c.wr32((c.r[3] as u32).wrapping_add(0x0000002c), c.r[4] as u32);
 if t { l = 0x0043ddf0; continue; }
l = 0x0043de20; continue; }
0x0043de20 => {
c.r[1] = (((c.r[25] as u32).wrapping_add(0x00003000)) as i32 as i64 as u64);
c.r[2] = (((c.r[25] as u32).wrapping_add(0x00003600)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[1] as u32)); c.r[6] = a; c.rh[6] = b; }
c.r[3] = (((c.r[10] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[2] as u32)); c.r[8] = a; c.rh[8] = b; }
c.r[4] = (((c.r[25] as u32).wrapping_add(0xffffff80)) as i32 as i64 as u64);
let t = c.r[31] != 0u64;  { let (a, b) = uycpu::mmi::pextlh((c.r[6], c.rh[6]), (0u64, 0u64)); c.r[5] = a; c.rh[5] = b; }
 if t { l = 0x0043dee0; continue; }
{ let (a, b) = uycpu::mmi::psraw((c.r[5], c.rh[5]), 16); c.r[5] = a; c.rh[5] = b; }
c.vf[5] = [c.r[5] as u32, (c.r[5] >> 32) as u32, c.rh[5] as u32, (c.rh[5] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((c.r[6], c.rh[6]), (0u64, 0u64)); c.r[6] = a; c.rh[6] = b; }
l = 0x0043de50; continue; }
0x0043de50 => {
{ let (a, b) = uycpu::mmi::psraw((c.r[6], c.rh[6]), 16); c.r[6] = a; c.rh[6] = b; }
c.vf[6] = [c.r[6] as u32, (c.r[6] >> 32) as u32, c.rh[6] as u32, (c.rh[6] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextlh((c.r[8], c.rh[8]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
c.vf[7] = [c.r[7] as u32, (c.r[7] >> 32) as u32, c.rh[7] as u32, (c.rh[7] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((c.r[8], c.rh[8]), (0u64, 0u64)); c.r[8] = a; c.rh[8] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[8], c.rh[8]), 16); c.r[8] = a; c.rh[8] = b; }
c.vf[8] = [c.r[8] as u32, (c.r[8] >> 32) as u32, c.rh[8] as u32, (c.rh[8] >> 32) as u32];
c.r[4] = (((c.r[4] as u32).wrapping_add(0x00000080)) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 15>(c, 5, 5);
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 15>(c, 6, 6);
c.r[2] = (((c.r[2] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 15>(c, 7, 7);
uycpu::vu0::itof::<3, 15>(c, 8, 8);
uycpu::vu0::fmac::<6, 15>(c, 32, 5, uycpu::vu0::bcast(c.vf[4][3]));
uycpu::vu0::fmac::<2, 15>(c, 9, 7, uycpu::vu0::bcast(c.vf[4][0]));
uycpu::vu0::fmac::<6, 15>(c, 32, 6, uycpu::vu0::bcast(c.vf[4][3]));
{ let (a, b) = c.rd128((c.r[1] as u32)); c.r[6] = a; c.rh[6] = b; }
uycpu::vu0::fmac::<2, 15>(c, 10, 8, uycpu::vu0::bcast(c.vf[4][0]));
{ let (a, b) = c.rd128((c.r[2] as u32)); c.r[8] = a; c.rh[8] = b; }
{ let (a, b) = uycpu::mmi::pextlh((c.r[6], c.rh[6]), (0u64, 0u64)); c.r[5] = a; c.rh[5] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[5], c.rh[5]), 16); c.r[5] = a; c.rh[5] = b; }
c.vf[5] = [c.r[5] as u32, (c.r[5] >> 32) as u32, c.rh[5] as u32, (c.rh[5] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((c.r[6], c.rh[6]), (0u64, 0u64)); c.r[6] = a; c.rh[6] = b; }
{ let v = c.vf[9]; c.wr128((c.r[4] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[3] = (((c.r[3] as u32).wrapping_add(0xfffffffe)) as i32 as i64 as u64);
let t = (c.r[3] as i64) > 0;  { let v = c.vf[10]; c.wr128((c.r[4] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043de50; continue; }
 l = 0x0043e1c8; continue;
}
0x0043dee0 => {
{ let (a, b) = uycpu::mmi::psraw((c.r[5], c.rh[5]), 16); c.r[5] = a; c.rh[5] = b; }
c.vf[5] = [c.r[5] as u32, (c.r[5] >> 32) as u32, c.rh[5] as u32, (c.rh[5] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((c.r[6], c.rh[6]), (0u64, 0u64)); c.r[6] = a; c.rh[6] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[6], c.rh[6]), 16); c.r[6] = a; c.rh[6] = b; }
c.vf[6] = [c.r[6] as u32, (c.r[6] >> 32) as u32, c.rh[6] as u32, (c.rh[6] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextlh((c.r[8], c.rh[8]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
c.vf[7] = [c.r[7] as u32, (c.r[7] >> 32) as u32, c.rh[7] as u32, (c.rh[7] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((c.r[8], c.rh[8]), (0u64, 0u64)); c.r[8] = a; c.rh[8] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[8], c.rh[8]), 16); c.r[8] = a; c.rh[8] = b; }
c.vf[8] = [c.r[8] as u32, (c.r[8] >> 32) as u32, c.rh[8] as u32, (c.rh[8] >> 32) as u32];
l = 0x0043df20; continue; }
0x0043df20 => {
uycpu::vu0::itof::<3, 15>(c, 5, 5);
uycpu::vu0::itof::<3, 15>(c, 7, 7);
uycpu::vu0::itof::<3, 15>(c, 6, 6);
uycpu::vu0::itof::<3, 15>(c, 8, 8);
uycpu::vu0::fmac::<6, 15>(c, 5, 5, uycpu::vu0::bcast(c.vf[4][3]));
uycpu::vu0::fmac::<6, 15>(c, 7, 7, uycpu::vu0::bcast(c.vf[4][0]));
uycpu::vu0::fmac::<6, 15>(c, 6, 6, uycpu::vu0::bcast(c.vf[4][3]));
uycpu::vu0::fmac::<6, 15>(c, 8, 8, uycpu::vu0::bcast(c.vf[4][0]));
uycpu::vu0::fmac::<0, 15>(c, 9, 5, c.vf[7]);
uycpu::vu0::fmac::<6, 15>(c, 10, 5, c.vf[7]);
uycpu::vu0::fmac::<0, 15>(c, 11, 6, c.vf[8]);
uycpu::vu0::fmac::<6, 15>(c, 12, 6, c.vf[8]);
uycpu::vu0::fmac::<6, 15>(c, 13, 9, c.vf[9]);
uycpu::vu0::fmac::<0, 4>(c, 32, 10, uycpu::vu0::bcast(c.vf[10][0]));
uycpu::vu0::fmac::<2, 4>(c, 32, 2, uycpu::vu0::bcast(c.vf[10][2]));
uycpu::vu0::fmac::<2, 4>(c, 14, 2, uycpu::vu0::bcast(c.vf[10][3]));
uycpu::vu0::fmac::<0, 8>(c, 32, 13, uycpu::vu0::bcast(c.vf[13][1]));
uycpu::vu0::fmac::<2, 8>(c, 32, 1, uycpu::vu0::bcast(c.vf[13][2]));
uycpu::vu0::fmac::<2, 8>(c, 13, 1, uycpu::vu0::bcast(c.vf[13][3]));
{ let (a, b) = { let v = c.vf[14]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; c.r[5] = a; c.rh[5] = b; }
let t = (c.r[5] as i64) >= 0;   if t { l = 0x0043df90; continue; }
uycpu::vu0::fmac::<1, 15>(c, 9, 5, c.vf[7]);
uycpu::vu0::fmac::<6, 15>(c, 13, 9, c.vf[9]);
uycpu::vu0::fmac::<0, 8>(c, 32, 13, uycpu::vu0::bcast(c.vf[13][1]));
uycpu::vu0::fmac::<2, 8>(c, 32, 1, uycpu::vu0::bcast(c.vf[13][2]));
uycpu::vu0::fmac::<2, 8>(c, 13, 1, uycpu::vu0::bcast(c.vf[13][3]));
l = 0x0043df90; continue; }
0x0043df90 => {
uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[13][0]);
uycpu::vu0::fmac::<6, 15>(c, 15, 11, c.vf[11]);
uycpu::vu0::fmac::<0, 4>(c, 32, 12, uycpu::vu0::bcast(c.vf[12][0]));
uycpu::vu0::fmac::<2, 4>(c, 32, 2, uycpu::vu0::bcast(c.vf[12][2]));
uycpu::vu0::fmac::<2, 4>(c, 16, 2, uycpu::vu0::bcast(c.vf[12][3]));
uycpu::vu0::fmac::<0, 8>(c, 32, 15, uycpu::vu0::bcast(c.vf[15][1]));
uycpu::vu0::fmac::<2, 8>(c, 32, 1, uycpu::vu0::bcast(c.vf[15][2]));
uycpu::vu0::fmac::<2, 8>(c, 15, 1, uycpu::vu0::bcast(c.vf[15][3]));
{ let (a, b) = { let v = c.vf[16]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; c.r[5] = a; c.rh[5] = b; }
let t = (c.r[5] as i64) >= 0;   if t { l = 0x0043dfe0; continue; }
uycpu::vu0::fmac::<1, 15>(c, 11, 6, c.vf[8]);
uycpu::vu0::fmac::<6, 15>(c, 15, 11, c.vf[11]);
uycpu::vu0::fmac::<0, 8>(c, 32, 15, uycpu::vu0::bcast(c.vf[15][1]));
uycpu::vu0::fmac::<2, 8>(c, 32, 1, uycpu::vu0::bcast(c.vf[15][2]));
uycpu::vu0::fmac::<2, 8>(c, 15, 1, uycpu::vu0::bcast(c.vf[15][3]));
l = 0x0043dfe0; continue; }
0x0043dfe0 => {
uycpu::vu0::fmac::<6, 15>(c, 9, 9, uycpu::vu0::bcast(c.vi[22]));
uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[15][0]);
{ let (a, b) = c.rd128((c.r[1] as u32).wrapping_add(0x00000010)); c.r[6] = a; c.rh[6] = b; }
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[2] as u32).wrapping_add(0x00000010)); c.r[8] = a; c.rh[8] = b; }
c.r[2] = (((c.r[2] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pextlh((c.r[6], c.rh[6]), (0u64, 0u64)); c.r[5] = a; c.rh[5] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[5], c.rh[5]), 16); c.r[5] = a; c.rh[5] = b; }
c.vf[5] = [c.r[5] as u32, (c.r[5] >> 32) as u32, c.rh[5] as u32, (c.rh[5] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((c.r[6], c.rh[6]), (0u64, 0u64)); c.r[6] = a; c.rh[6] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[6], c.rh[6]), 16); c.r[6] = a; c.rh[6] = b; }
l = 0x0043e020; continue; }
0x0043e020 => {
c.vf[6] = [c.r[6] as u32, (c.r[6] >> 32) as u32, c.rh[6] as u32, (c.rh[6] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextlh((c.r[8], c.rh[8]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
c.vf[7] = [c.r[7] as u32, (c.r[7] >> 32) as u32, c.rh[7] as u32, (c.rh[7] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((c.r[8], c.rh[8]), (0u64, 0u64)); c.r[8] = a; c.rh[8] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[8], c.rh[8]), 16); c.r[8] = a; c.rh[8] = b; }
c.vf[8] = [c.r[8] as u32, (c.r[8] >> 32) as u32, c.rh[8] as u32, (c.rh[8] >> 32) as u32];
uycpu::vu0::fmac::<6, 15>(c, 11, 11, uycpu::vu0::bcast(c.vi[22]));
c.r[4] = (((c.r[4] as u32).wrapping_add(0x00000080)) as i32 as i64 as u64);
{ let v = c.vf[9]; c.wr128((c.r[4] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[3] = (((c.r[3] as u32).wrapping_add(0xfffffffe)) as i32 as i64 as u64);
let t = (c.r[3] as i64) > 0;  { let v = c.vf[11]; c.wr128((c.r[4] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043df20; continue; }
 l = 0x0043e1c8; continue;
}
0x0043e068 => {
c.r[1] = c.rd64((c.r[14] as u32).wrapping_add(0x00002ff0));
c.r[5] = (((c.r[10] as u32) << 3) as i32 as i64 as u64);
c.r[12] = (((c.r[25] as u32).wrapping_add(0x00003000)) as i32 as i64 as u64);
uycpu::vu0::fmac::<6, 15>(c, 5, 0, uycpu::vu0::bcast(c.vf[0][0]));
let t = c.r[1] == 0u64;  c.r[12] = (((c.r[12] as u32).wrapping_add((c.r[5] as u32))) as i32 as i64 as u64);
 if t { l = 0x0043e068; continue; }
let t = c.r[2] == 0u64;  c.r[5] = c.rd64((c.r[12] as u32));
 if t { l = 0x0043e0e0; continue; }
c.r[6] = c.rd8((c.r[12] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((c.r[5], c.rh[5]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
{ let (a, b) = uycpu::mmi::psrlw((c.r[7], c.rh[7]), 13); c.r[7] = a; c.rh[7] = b; }
l = 0x0043e0a0; continue; }
0x0043e0a0 => {
let t = (c.r[2] as i64) <= 0;   if t { l = 0x0043e0e0; continue; }
c.vf[6] = [c.r[7] as u32, (c.r[7] >> 32) as u32, c.rh[7] as u32, (c.rh[7] >> 32) as u32];
c.r[9] = c.r[5] >> 62;
c.r[8] = (((c.r[6] as u32) << 6) as i32 as i64 as u64);
c.r[5] = c.rd64((c.r[12] as u32).wrapping_add(0x00000008));
uycpu::vu0::itof::<3, 14>(c, 5, 6);
c.r[8] = (((c.r[8] as u32).wrapping_add((c.r[25] as u32))) as i32 as i64 as u64);
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
c.r[2] = (((c.r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
c.r[6] = c.rd8((c.r[12] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((c.r[5], c.rh[5]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
let t = c.r[9] == 0u64;  { let (a, b) = uycpu::mmi::psrlw((c.r[7], c.rh[7]), 13); c.r[7] = a; c.rh[7] = b; }
 if t { l = 0x0043e0a0; continue; }
let t = (c.r[2] as i64) > 0;  { let v = c.vf[5]; c.wr128((c.r[8] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043e0a0; continue; }
l = 0x0043e0e0; continue; }
0x0043e0e0 => {
let t = c.r[3] == 0u64;  c.r[5] = c.rd64((c.r[12] as u32));
 if t { l = 0x0043e130; continue; }
c.r[6] = c.rd8((c.r[12] as u32).wrapping_add(0x00000006)) as u64;
uycpu::vu0::mov::<1>(c, 5, c.vf[0]);
{ let (a, b) = uycpu::mmi::pextlh((c.r[5], c.rh[5]), (0u64, 0u64)); c.r[5] = a; c.rh[5] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[5], c.rh[5]), 16); c.r[5] = a; c.rh[5] = b; }
l = 0x0043e0f8; continue; }
0x0043e0f8 => {
c.vf[6] = [c.r[5] as u32, (c.r[5] >> 32) as u32, c.rh[5] as u32, (c.rh[5] >> 32) as u32];
c.r[8] = (((c.r[6] as u32) << 6) as i32 as i64 as u64);
c.r[8] = (((c.r[8] as u32).wrapping_add((c.r[25] as u32))) as i32 as i64 as u64);
c.r[5] = c.rd64((c.r[12] as u32).wrapping_add(0x00000008));
uycpu::vu0::itof::<0, 14>(c, 5, 6);
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
c.r[6] = c.rd8((c.r[12] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((c.r[5], c.rh[5]), (0u64, 0u64)); c.r[5] = a; c.rh[5] = b; }
c.r[7] = c.rd32((c.r[8] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
{ let (a, b) = uycpu::mmi::psraw((c.r[5], c.rh[5]), 16); c.r[5] = a; c.rh[5] = b; }
{ let v = c.vf[5]; c.wr128((c.r[8] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[3] = (((c.r[3] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
let t = (c.r[3] as i64) > 0;  c.wr32((c.r[8] as u32).wrapping_add(0x0000002c), c.r[7] as u32);
 if t { l = 0x0043e0f8; continue; }
l = 0x0043e130; continue; }
0x0043e130 => {
c.r[1] = (((c.r[25] as u32).wrapping_add(0x00003000)) as i32 as i64 as u64);
c.r[2] = (((c.r[25] as u32).wrapping_add(0xffffff00)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[1] as u32)); c.r[6] = a; c.rh[6] = b; }
c.r[3] = (((c.r[10] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((c.r[1] as u32).wrapping_add(0x00000010)); c.r[8] = a; c.rh[8] = b; }
l = 0x0043e148; continue; }
0x0043e148 => {
{ let (a, b) = uycpu::mmi::pextlh((c.r[6], c.rh[6]), (0u64, 0u64)); c.r[5] = a; c.rh[5] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[5], c.rh[5]), 16); c.r[5] = a; c.rh[5] = b; }
c.vf[5] = [c.r[5] as u32, (c.r[5] >> 32) as u32, c.rh[5] as u32, (c.rh[5] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((c.r[6], c.rh[6]), (0u64, 0u64)); c.r[6] = a; c.rh[6] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[6], c.rh[6]), 16); c.r[6] = a; c.rh[6] = b; }
c.vf[6] = [c.r[6] as u32, (c.r[6] >> 32) as u32, c.rh[6] as u32, (c.rh[6] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextlh((c.r[8], c.rh[8]), (0u64, 0u64)); c.r[7] = a; c.rh[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[7], c.rh[7]), 16); c.r[7] = a; c.rh[7] = b; }
c.vf[7] = [c.r[7] as u32, (c.r[7] >> 32) as u32, c.rh[7] as u32, (c.rh[7] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((c.r[8], c.rh[8]), (0u64, 0u64)); c.r[8] = a; c.rh[8] = b; }
{ let (a, b) = uycpu::mmi::psraw((c.r[8], c.rh[8]), 16); c.r[8] = a; c.rh[8] = b; }
c.vf[8] = [c.r[8] as u32, (c.r[8] >> 32) as u32, c.rh[8] as u32, (c.rh[8] >> 32) as u32];
uycpu::vu0::itof::<3, 15>(c, 5, 5);
c.r[1] = (((c.r[1] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 15>(c, 6, 6);
c.r[2] = (((c.r[2] as u32).wrapping_add(0x00000100)) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 15>(c, 7, 7);
{ let (a, b) = c.rd128((c.r[1] as u32)); c.r[6] = a; c.rh[6] = b; }
uycpu::vu0::itof::<3, 15>(c, 8, 8);
{ let (a, b) = c.rd128((c.r[1] as u32).wrapping_add(0x00000010)); c.r[8] = a; c.rh[8] = b; }
{ let v = c.vf[5]; c.wr128((c.r[2] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[6]; c.wr128((c.r[2] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[7]; c.wr128((c.r[2] as u32).wrapping_add(0x00000080), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[3] = (((c.r[3] as u32).wrapping_add(0xfffffffc)) as i32 as i64 as u64);
let t = (c.r[3] as i64) > 0;  { let v = c.vf[8]; c.wr128((c.r[2] as u32).wrapping_add(0x000000c0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043e148; continue; }
l = 0x0043e1c8; continue; }
0x0043e1c8 => {
c.r[14] = c.rd32((c.r[24] as u32).wrapping_add(0x00000050)) as i32 as i64 as u64;
c.r[15] = c.rd32((c.r[24] as u32).wrapping_add(0x00000054)) as i32 as i64 as u64;
l = 0x0043e1d0; continue; }
0x0043e1d0 => {
let t = c.r[14] == 0u64;  c.r[31] = 0x0000000010000000;
 if t { l = 0x0043e338; continue; }
c.r[13] = c.rd16((c.r[14] as u32).wrapping_add(0x00000002)) as u64;
c.r[31] = c.r[31] | 0xd400;
c.r[12] = c.rd32((c.r[14] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64;
c.r[1] = ((0u32.wrapping_add(0x00002000)) as i32 as i64 as u64);
c.wr32((c.r[31] as u32).wrapping_add(0x00000080), c.r[1] as u32);
c.r[1] = (((c.r[13] as u32).wrapping_add((c.r[13] as u32))) as i32 as i64 as u64);
c.wr32((c.r[31] as u32).wrapping_add(0x00000010), c.r[12] as u32);
c.r[13] = (((c.r[13] as u32).wrapping_add((c.r[1] as u32))) as i32 as i64 as u64);
c.wr32((c.r[31] as u32).wrapping_add(0x00000020), c.r[13] as u32);
c.r[1] = ((0u32.wrapping_add(0x00000100)) as i32 as i64 as u64);
c.wr32((c.r[31] as u32), c.r[1] as u32);
c.r[13] = (((c.r[13] as u32) << 4) as i32 as i64 as u64);
c.r[1] = c.rd32((c.r[14] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
c.r[12] = (((c.r[25] as u32).wrapping_add(0x00002000)) as i32 as i64 as u64);
c.vf[4] = [c.r[1] as u32, (c.r[1] >> 32) as u32, c.rh[1] as u32, (c.rh[1] >> 32) as u32];
c.r[13] = (((c.r[13] as u32).wrapping_add((c.r[12] as u32))) as i32 as i64 as u64);
c.r[14] = c.rd32((c.r[14] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
c.r[1] = (((c.r[25] as u32).wrapping_add(0x00002000)) as i32 as i64 as u64);
c.wr32((c.r[13] as u32).wrapping_add(0x0000002c), c.r[1] as u32);
uycpu::vu0::fmac::<1, 1>(c, 4, 0, uycpu::vu0::bcast(c.vf[4][0]));
l = 0x0043e228; continue; }
0x0043e228 => {
c.r[2] = c.rd32((c.r[31] as u32)) as i32 as i64 as u64;
c.r[2] = c.r[2] & 0x100;
let t = c.r[2] != 0u64;   if t { l = 0x0043e228; continue; }
c.r[11] = c.rd32((c.r[12] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
{ let (a, b) = c.rd128((c.r[12] as u32)); c.vf[9] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[12] as u32).wrapping_add(0x00000010)); c.vf[10] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[12] as u32).wrapping_add(0x00000020)); c.vf[11] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[11] as u32)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[11] as u32).wrapping_add(0x00000010)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[11] as u32).wrapping_add(0x00000020)); c.vf[8] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
l = 0x0043e268; continue; }
0x0043e268 => {
uycpu::vu0::fmac::<6, 15>(c, 6, 6, uycpu::vu0::bcast(c.vf[4][3]));
uycpu::vu0::fmac::<6, 15>(c, 9, 9, uycpu::vu0::bcast(c.vf[4][0]));
uycpu::vu0::fmac::<6, 14>(c, 7, 7, uycpu::vu0::bcast(c.vf[4][3]));
uycpu::vu0::fmac::<6, 14>(c, 10, 10, uycpu::vu0::bcast(c.vf[4][0]));
uycpu::vu0::fmac::<6, 14>(c, 8, 8, uycpu::vu0::bcast(c.vf[4][3]));
uycpu::vu0::fmac::<6, 14>(c, 11, 11, uycpu::vu0::bcast(c.vf[4][0]));
uycpu::vu0::fmac::<0, 15>(c, 12, 6, c.vf[9]);
uycpu::vu0::fmac::<6, 15>(c, 14, 6, c.vf[9]);
uycpu::vu0::fmac::<0, 14>(c, 7, 7, c.vf[10]);
uycpu::vu0::fmac::<0, 14>(c, 8, 8, c.vf[11]);
uycpu::vu0::fmac::<6, 15>(c, 13, 12, c.vf[12]);
uycpu::vu0::fmac::<0, 4>(c, 32, 14, uycpu::vu0::bcast(c.vf[14][0]));
uycpu::vu0::fmac::<2, 4>(c, 32, 2, uycpu::vu0::bcast(c.vf[14][2]));
uycpu::vu0::fmac::<2, 4>(c, 14, 2, uycpu::vu0::bcast(c.vf[14][3]));
uycpu::vu0::fmac::<0, 8>(c, 32, 13, uycpu::vu0::bcast(c.vf[13][1]));
uycpu::vu0::fmac::<2, 8>(c, 32, 1, uycpu::vu0::bcast(c.vf[13][2]));
uycpu::vu0::fmac::<2, 8>(c, 13, 1, uycpu::vu0::bcast(c.vf[13][3]));
{ let (a, b) = { let v = c.vf[14]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; c.r[2] = a; c.rh[2] = b; }
let t = (c.r[2] as i64) >= 0;   if t { l = 0x0043e2d0; continue; }
uycpu::vu0::fmac::<1, 15>(c, 12, 6, c.vf[9]);
uycpu::vu0::fmac::<6, 15>(c, 13, 12, c.vf[12]);
uycpu::vu0::fmac::<0, 8>(c, 32, 13, uycpu::vu0::bcast(c.vf[13][1]));
uycpu::vu0::fmac::<2, 8>(c, 32, 1, uycpu::vu0::bcast(c.vf[13][2]));
uycpu::vu0::fmac::<2, 8>(c, 13, 1, uycpu::vu0::bcast(c.vf[13][3]));
l = 0x0043e2d0; continue; }
0x0043e2d0 => {
uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[13][0]);
c.r[3] = c.rd32((c.r[12] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
c.r[1] = c.rd32((c.r[12] as u32).wrapping_add(0x0000005c)) as i32 as i64 as u64;
{ let (a, b) = c.rd128((c.r[12] as u32).wrapping_add(0x00000030)); c.vf[9] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[12] as u32).wrapping_add(0x00000040)); c.vf[10] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[12] as u32).wrapping_add(0x00000050)); c.vf[11] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let v = c.vf[7]; c.wr128((c.r[11] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[8]; c.wr128((c.r[11] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let (a, b) = c.rd128((c.r[1] as u32)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[1] as u32).wrapping_add(0x00000010)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[1] as u32).wrapping_add(0x00000020)); c.vf[8] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
c.r[2] = c.rd32((c.r[11] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
let t = c.r[2] != 0u64;   if t { l = 0x0043e318; continue; }
c.wr32((c.r[11] as u32).wrapping_add(0x0000001c), c.r[3] as u32);
l = 0x0043e318; continue; }
0x0043e318 => {
uycpu::vu0::fmac::<6, 15>(c, 12, 12, uycpu::vu0::bcast(c.vi[22]));
c.r[12] = (((c.r[12] as u32).wrapping_add(0x00000030)) as i32 as i64 as u64);
{ let v = c.vf[12]; c.wr128((c.r[11] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.r[11] = (((c.r[1] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
let t = c.r[12] != c.r[13];   if t { l = 0x0043e268; continue; }
let t = c.r[14] != 0u64;   if t { l = 0x0043e1d0; continue; }
l = 0x0043e338; continue; }
0x0043e338 => {
let t = c.r[15] == 0u64;  c.r[14] = (((c.r[15] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
 if t { l = 0x0043e448; continue; }
uycpu::vu0::fmac::<0, 14>(c, 5, 0, uycpu::vu0::bcast(c.vf[0][3]));
c.r[1] = c.rd32((c.r[14] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
c.r[2] = c.rd8((c.r[14] as u32).wrapping_add(0x00000003)) as u64;
{ let (a, b) = c.rd128((c.r[1] as u32)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[1] as u32).wrapping_add(0x00000010)); c.vf[8] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[1] as u32).wrapping_add(0x00000020)); c.vf[10] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[14] as u32).wrapping_add(0x00000010)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[14] as u32).wrapping_add(0x00000020)); c.vf[9] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((c.r[14] as u32).wrapping_add(0x00000030)); c.vf[11] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
let t = c.r[2] != 0u64;  c.r[15] = c.rd32((c.r[14] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
 if t { l = 0x0043e3a8; continue; }
uycpu::vu0::fmac::<6, 1>(c, 12, 7, c.vf[6]);
uycpu::vu0::fmac::<6, 14>(c, 13, 7, c.vf[6]);
uycpu::vu0::fmac::<6, 14>(c, 14, 7, uycpu::vu0::bcast(c.vf[6][3]));
uycpu::vu0::fmac::<6, 14>(c, 15, 6, uycpu::vu0::bcast(c.vf[7][3]));
uycpu::vu0::opmula(c, 6, 7);
uycpu::vu0::opmsub(c, 16, 7, 6);
uycpu::vu0::fmac::<0, 8>(c, 32, 13, uycpu::vu0::bcast(c.vf[13][1]));
uycpu::vu0::fmac::<2, 8>(c, 13, 5, uycpu::vu0::bcast(c.vf[13][2]));
uycpu::vu0::fmac::<0, 14>(c, 6, 14, c.vf[15]);
uycpu::vu0::fmac::<0, 14>(c, 6, 6, c.vf[16]);
uycpu::vu0::fmac::<1, 1>(c, 6, 12, uycpu::vu0::bcast(c.vf[13][0]));
uycpu::vu0::fmac::<6, 14>(c, 8, 8, c.vf[9]);
uycpu::vu0::fmac::<0, 14>(c, 10, 10, c.vf[11]);
 l = 0x0043e438; continue;
}
0x0043e3a8 => {
c.r[3] = c.rd32((c.r[14] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
c.vf[18] = [c.r[3] as u32, (c.r[3] >> 32) as u32, c.rh[3] as u32, (c.rh[3] >> 32) as u32];
uycpu::vu0::fmac::<1, 1>(c, 18, 0, uycpu::vu0::bcast(c.vf[18][0]));
uycpu::vu0::fmac::<6, 15>(c, 6, 6, uycpu::vu0::bcast(c.vf[18][3]));
uycpu::vu0::fmac::<6, 15>(c, 7, 7, uycpu::vu0::bcast(c.vf[18][0]));
uycpu::vu0::fmac::<6, 14>(c, 8, 8, uycpu::vu0::bcast(c.vf[18][3]));
uycpu::vu0::fmac::<6, 14>(c, 9, 9, uycpu::vu0::bcast(c.vf[18][0]));
uycpu::vu0::fmac::<6, 14>(c, 10, 10, uycpu::vu0::bcast(c.vf[18][3]));
uycpu::vu0::fmac::<6, 14>(c, 11, 11, uycpu::vu0::bcast(c.vf[18][0]));
uycpu::vu0::fmac::<0, 15>(c, 14, 6, c.vf[7]);
uycpu::vu0::fmac::<6, 15>(c, 16, 6, c.vf[7]);
uycpu::vu0::fmac::<0, 14>(c, 8, 8, c.vf[9]);
uycpu::vu0::fmac::<0, 14>(c, 10, 10, c.vf[11]);
uycpu::vu0::fmac::<6, 15>(c, 15, 14, c.vf[14]);
uycpu::vu0::fmac::<0, 4>(c, 32, 16, uycpu::vu0::bcast(c.vf[16][0]));
uycpu::vu0::fmac::<2, 4>(c, 32, 2, uycpu::vu0::bcast(c.vf[16][2]));
uycpu::vu0::fmac::<2, 4>(c, 16, 2, uycpu::vu0::bcast(c.vf[16][3]));
uycpu::vu0::fmac::<0, 8>(c, 32, 15, uycpu::vu0::bcast(c.vf[15][1]));
uycpu::vu0::fmac::<2, 8>(c, 32, 1, uycpu::vu0::bcast(c.vf[15][2]));
uycpu::vu0::fmac::<2, 8>(c, 15, 1, uycpu::vu0::bcast(c.vf[15][3]));
{ let (a, b) = { let v = c.vf[16]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; c.r[2] = a; c.rh[2] = b; }
let t = (c.r[2] as i64) >= 0;   if t { l = 0x0043e420; continue; }
uycpu::vu0::fmac::<1, 15>(c, 14, 6, c.vf[7]);
uycpu::vu0::fmac::<6, 15>(c, 15, 14, c.vf[14]);
uycpu::vu0::fmac::<0, 8>(c, 32, 15, uycpu::vu0::bcast(c.vf[15][1]));
uycpu::vu0::fmac::<2, 8>(c, 32, 1, uycpu::vu0::bcast(c.vf[15][2]));
uycpu::vu0::fmac::<2, 8>(c, 15, 1, uycpu::vu0::bcast(c.vf[15][3]));
l = 0x0043e420; continue; }
0x0043e420 => {
uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[15][0]);
uycpu::vu0::fmac::<6, 15>(c, 6, 14, uycpu::vu0::bcast(c.vi[22]));
l = 0x0043e438; continue; }
0x0043e438 => {
{ let v = c.vf[6]; c.wr128((c.r[1] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[8]; c.wr128((c.r[1] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
let t = c.r[15] != 0u64;  { let v = c.vf[10]; c.wr128((c.r[1] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043e338; continue; }
l = 0x0043e448; continue; }
0x0043e448 => {
{ let (a, b) = uycpu::mmi::pcpyud((c.r[31], c.rh[31]), (0u64, 0u64)); c.r[31] = a; c.rh[31] = b; }
let t = (c.r[31] as u32);  c.ret = t; return;
}
_ => panic!("the animation unit: no label {l:08x} in 0x0043D9D8"), } }
}

#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn n004389d8(c: &mut Cpu, k: F) {
let mut l: u32 = 0x004389d8;
loop { match l {
0x004389d8 => {
c.r[2] = c.rd64((c.r[4] as u32).wrapping_add(0x00000048));
c.r[15] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
c.r[25] = c.rd32((c.r[4] as u32).wrapping_add(0x00000024)) as i32 as i64 as u64;
c.r[24] = (((c.r[4] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
c.r[10] = c.rd8((c.r[24] as u32).wrapping_add(0x00000042)) as u64;
c.f[0] = uycpu::fpu::sub(c.f[0], c.f[0]);
c.r[11] = c.rd8((c.r[24] as u32).wrapping_add(0x00000043)) as u64;
c.r[1] = 0x000000003f800000;
c.r[14] = c.rd32((c.r[24] as u32).wrapping_add(0x0000006c)) as i32 as i64 as u64;
c.r[13] = 0u64 | 0xffff;
c.f[1] = c.rd32((c.r[24] as u32).wrapping_add(0x00000044));
c.r[3] = c.r[2] >> 32;
c.r[14] = (((c.r[14] as u32) << 8) as i32 as i64 as u64);
c.f[2] = (c.r[2] as u32);
let t = c.r[3] == 0u64;  c.r[14] = (((c.r[14] as u32) >> 8) as i32 as i64 as u64);
 if t { l = 0x00438ca0; continue; }
let t = c.r[10] != c.r[11];  c.f[3] = (c.r[3] as u32);
 if t { l = 0x00438a90; continue; }
c.facc = uycpu::fpu::add(c.f[0], c.f[1]);
c.r[3] = 0x000000003f800000;
c.f[1] = uycpu::fpu::add(c.facc, uycpu::fpu::mul(c.f[2], c.f[3]));
c.r[4] = 0x000000003f7f0000;
l = 0x00438a30; continue; }
0x00438a30 => {
c.r[8] = c.rd8((c.r[24] as u32).wrapping_add(0x00000040)) as u64;
c.r[2] = (((c.r[2] as u32) >> 0) as i32 as i64 as u64);
let t = c.r[2] == 0u64;  c.r[9] = c.rd8((c.r[24] as u32).wrapping_add(0x00000041)) as u64;
 if t { l = 0x00438ca0; continue; }
c.f[12] = c.rd32((c.r[24] as u32).wrapping_add(0x00000044));
c.r[12] = (((c.r[8] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
c.r[2] = c.f[1] as i32 as i64 as u64;
c.r[3] = c.r[3] | 0x8000;
c.f[4] = (c.r[1] as u32);
c.r[7] = (((c.r[11] as u32) << 2) as i32 as i64 as u64);
c.r[3] = c.r[3].wrapping_sub(c.r[2]);
c.r[4] = c.r[2].wrapping_sub(c.r[4]);
c.r[3] = c.r[3] | c.r[4];
c.r[4] = c.r[2].wrapping_sub(c.r[1]);
let t = (c.r[3] as i64) > 0;  c.r[7] = (((c.r[7] as u32).wrapping_add((c.r[25] as u32))) as i32 as i64 as u64);
 if t { l = 0x00438aa0; continue; }
let t = (c.r[4] as i64) > 0;  c.r[7] = c.rd32((c.r[7] as u32).wrapping_add(0x00000048)) as i32 as i64 as u64;
 if t { l = 0x00438aa8; continue; }
let t = (c.r[2] as i64) < 0;   if t { l = 0x00438b80; continue; }
let t = c.r[13] != c.r[14];  c.wr32((c.r[24] as u32).wrapping_add(0x00000044), c.f[1]);
 if t { l = 0x00438c10; continue; }
let t = (c.r[31] as u32); c.wr8((c.r[24] as u32).wrapping_add(0x00000060), c.r[15] as u8);
 c.ret = t; return;
}
0x00438a90 => {
c.f[1] = uycpu::fpu::add(c.f[1], c.f[3]);
c.r[3] = 0x000000003f800000;
c.r[4] = 0x000000003f7f0000;
 l = 0x00438a30; continue;
}
0x00438aa0 => {
c.f[1] = (c.r[1] as u32);
c.r[7] = c.rd32((c.r[7] as u32).wrapping_add(0x00000048)) as i32 as i64 as u64;
l = 0x00438aa8; continue; }
0x00438aa8 => {
c.f[1] = uycpu::fpu::sub(c.f[1], c.f[4]);
c.r[8] = (((c.r[9] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
c.r[9] = (((c.r[9] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
let t = c.r[10] != c.r[11];  c.r[5] = c.rd32((c.r[24] as u32).wrapping_add(0x0000005c)) as i32 as i64 as u64;
 if t { l = 0x00438b50; continue; }
l = 0x00438ac0; continue; }
0x00438ac0 => {
c.f[1] = uycpu::fpu::div(c, c.f[1], c.f[3]);
c.r[15] = c.r[15] | 0x1;
c.r[3] = c.rd8((c.r[7] as u32).wrapping_add(0x00000010)) as u64;
c.r[6] = (((c.r[9] as u32) << 2) as i32 as i64 as u64);
c.r[4] = c.rd32((c.r[7] as u32).wrapping_add(0x00000018)) as i32 as i64 as u64;
c.r[6] = (((c.r[6] as u32).wrapping_add((c.r[7] as u32))) as i32 as i64 as u64);
c.r[3] = (((c.r[3] as u32).wrapping_sub((c.r[9] as u32))) as i32 as i64 as u64);
c.r[6] = c.rd32((c.r[6] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
let t = (c.r[3] as i64) > 0;  c.f[3] = (c.r[4] as u32);
 if t { l = 0x00438b00; continue; }
c.r[9] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
c.r[15] = c.r[15] | 0x2;
c.r[6] = c.rd32((c.r[7] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
l = 0x00438b00; continue; }
0x00438b00 => {
let t = c.r[4] != 0u64;   if t { l = 0x00438b10; continue; }
c.f[3] = c.rd32((c.r[5] as u32));
l = 0x00438b10; continue; }
0x00438b10 => {
c.f[1] = uycpu::fpu::mul(c.f[1], c.f[3]);
c.wr8((c.r[24] as u32).wrapping_add(0x00000042), c.r[10] as u8);
c.wr8((c.r[24] as u32).wrapping_add(0x00000040), c.r[8] as u8);
c.wr8((c.r[24] as u32).wrapping_add(0x00000041), c.r[9] as u8);
c.wr32((c.r[24] as u32).wrapping_add(0x00000058), c.r[5] as u32);
c.wr32((c.r[24] as u32).wrapping_add(0x0000005c), c.r[6] as u32);
c.r[2] = c.f[1] as i32 as i64 as u64;
c.r[1] = 0x000000003f800000;
c.wr32((c.r[24] as u32).wrapping_add(0x0000004c), c.f[3]);
c.r[1] = c.r[2].wrapping_sub(c.r[1]);
let t = (c.r[1] as i64) > 0;  c.wr32((c.r[24] as u32).wrapping_add(0x00000044), c.f[1]);
 if t { l = 0x00438aa8; continue; }
let t = c.r[13] != c.r[14];  c.wr8((c.r[24] as u32).wrapping_add(0x00000060), c.r[15] as u8);
 if t { l = 0x00438c10; continue; }
let t = (c.r[31] as u32);  c.ret = t; return;
}
0x00438b50 => {
c.r[12] = c.rd8((c.r[24] as u32).wrapping_add(0x0000007e)) as u64;
c.r[10] = (((c.r[11] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
c.r[1] = c.rd8((c.r[7] as u32).wrapping_add(0x00000012)) as u64;
c.r[12] = c.r[12] & 0x4;
let t = (c.r[12] as i64) > 0;  if t { c.r[1] = (((c.r[1] as u32).wrapping_sub((c.r[1] as u32))) as i32 as i64 as u64);
 l = 0x00438b68; continue; }
l = 0x00438b68; continue; }
0x00438b68 => {
c.f[12] = 0u32;
c.r[14] = (((c.r[1] as u32) << 16) as i32 as i64 as u64);
c.r[12] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
c.r[14] = c.r[14] | c.r[13];
c.wr8((c.r[24] as u32).wrapping_add(0x0000006e), c.r[1] as u8);
 l = 0x00438ac0; continue;
}
0x00438b80 => {
c.f[1] = uycpu::fpu::div(c, c.f[1], c.f[3]);
c.r[9] = (((c.r[8] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
c.r[8] = (((c.r[8] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
c.r[6] = c.rd32((c.r[24] as u32).wrapping_add(0x00000058)) as i32 as i64 as u64;
c.r[15] = c.r[15] | 0x1;
let t = (c.r[8] as i64) >= 0;  c.r[4] = c.rd32((c.r[7] as u32).wrapping_add(0x00000018)) as i32 as i64 as u64;
 if t { l = 0x00438bb8; continue; }
c.r[8] = c.rd8((c.r[7] as u32).wrapping_add(0x00000010)) as u64;
c.r[15] = c.r[15] | 0x2;
c.r[8] = (((c.r[8] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
l = 0x00438bb8; continue; }
0x00438bb8 => {
c.r[5] = (((c.r[8] as u32) << 2) as i32 as i64 as u64);
c.r[5] = (((c.r[5] as u32).wrapping_add((c.r[7] as u32))) as i32 as i64 as u64);
c.f[3] = (c.r[4] as u32);
let t = c.r[4] != 0u64;  c.r[5] = c.rd32((c.r[5] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
 if t { l = 0x00438bd8; continue; }
c.f[3] = c.rd32((c.r[5] as u32));
l = 0x00438bd8; continue; }
0x00438bd8 => {
c.f[1] = uycpu::fpu::mul(c.f[1], c.f[3]);
c.wr32((c.r[24] as u32).wrapping_add(0x00000058), c.r[5] as u32);
c.wr8((c.r[24] as u32).wrapping_add(0x00000040), c.r[8] as u8);
c.wr8((c.r[24] as u32).wrapping_add(0x00000041), c.r[9] as u8);
c.f[1] = uycpu::fpu::add(c.f[1], c.f[4]);
c.wr32((c.r[24] as u32).wrapping_add(0x0000005c), c.r[6] as u32);
c.r[1] = c.f[1] as i32 as i64 as u64;
c.wr32((c.r[24] as u32).wrapping_add(0x0000004c), c.f[3]);
let t = (c.r[1] as i64) < 0;  c.wr32((c.r[24] as u32).wrapping_add(0x00000044), c.f[1]);
 if t { l = 0x00438b80; continue; }
let t = c.r[13] != c.r[14];  c.wr8((c.r[24] as u32).wrapping_add(0x00000060), c.r[15] as u8);
 if t { l = 0x00438c10; continue; }
let t = (c.r[31] as u32);  c.ret = t; return;
}
0x00438c10 => {
let t = c.r[10] != c.r[11];  c.wr8((c.r[24] as u32).wrapping_add(0x00000060), c.r[15] as u8);
 if t { l = 0x00438ca0; continue; }
c.r[6] = (((((c.r[14] as u32) as i32) >> 16) as u32) as i32 as i64 as u64);
c.r[3] = 0x0000000041800000;
let t = (c.r[6] as i64) <= 0;  c.f[3] = (c.r[3] as u32);
 if t { l = 0x00438ca0; continue; }
c.r[1] = (((c.r[8] as u32) << 4) as i32 as i64 as u64);
c.r[2] = (((c.r[12] as u32) << 4) as i32 as i64 as u64);
c.f[1] = uycpu::fpu::mul(c.f[1], c.f[3]);
c.f[2] = uycpu::fpu::mul(c.f[12], c.f[3]);
c.f[1] = uycpu::fpu::cvt_w(c.f[1]);
c.f[2] = uycpu::fpu::cvt_w(c.f[2]);
c.r[5] = c.rd8((c.r[7] as u32).wrapping_add(0x00000010)) as u64;
c.r[3] = c.f[1] as i32 as i64 as u64;
c.r[4] = c.f[2] as i32 as i64 as u64;
c.r[1] = (((c.r[1] as u32).wrapping_add((c.r[3] as u32))) as i32 as i64 as u64);
c.r[2] = (((c.r[2] as u32).wrapping_add((c.r[4] as u32))) as i32 as i64 as u64);
c.r[3] = (((c.r[1] as u32).wrapping_sub((c.r[2] as u32))) as i32 as i64 as u64);
c.r[5] = (((c.r[5] as u32) << 2) as i32 as i64 as u64);
let t = (c.r[3] as i64) <= 0;  c.r[5] = (((c.r[5] as u32).wrapping_add((c.r[7] as u32))) as i32 as i64 as u64);
 if t { l = 0x00438ca0; continue; }
c.r[3] = c.rd16((c.r[5] as u32).wrapping_add(0x0000001e)) as u64;
c.r[2] = (((c.r[2] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
l = 0x00438c70; continue; }
0x00438c70 => {
let t = (c.r[6] as i64) <= 0;  c.r[6] = (((c.r[6] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x00438ca0; continue; }
c.r[4] = (((c.r[3] as u32).wrapping_sub((c.r[2] as u32))) as i32 as i64 as u64);
c.r[3] = (((c.r[1] as u32).wrapping_sub((c.r[3] as u32))) as i32 as i64 as u64);
c.r[3] = c.r[3] | c.r[4];
c.r[5] = (((c.r[5] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
let t = (c.r[3] as i64) < 0;  c.r[3] = c.rd16((c.r[5] as u32).wrapping_add(0x0000001e)) as u64;
 if t { l = 0x00438c70; continue; }
c.r[4] = c.rd16((c.r[5] as u32).wrapping_add(0x00000018)) as u64;
c.r[5] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
c.r[6] = (((c.r[24] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
 c.call(k.jal(c, 0x00438c98), k.pc(0x00438c98)); return;
}
0x00438ca0 => {
c.r[1] = (((c.r[14] as u32) << 16) as i32 as i64 as u64);
c.r[2] = (((c.r[13] as u32) << 16) as i32 as i64 as u64);
let t = c.r[1] == c.r[2];  c.wr8((c.r[24] as u32).wrapping_add(0x00000060), c.r[15] as u8);
 if t { l = 0x00438ce0; continue; }
c.r[1] = (((c.r[1] as u32) >> 24) as i32 as i64 as u64);
c.r[2] = 0u64 | 0xff;
let t = c.r[1] != c.r[2];   if t { l = 0x00438cd8; continue; }
c.r[3] = c.rd32((c.r[28] as u32).wrapping_add(k.lo(c, 0x00438cc0))) as i32 as i64 as u64;
c.r[2] = (((c.r[24] as u32) >> 8) as i32 as i64 as u64);
c.r[3] = c.r[3] & 0x3;
c.r[2] = c.r[2] & 0x3;
let t = c.r[2] != c.r[3];   if t { l = 0x00438ce0; continue; }
l = 0x00438cd8; continue; }
0x00438cd8 => {
c.r[4] = (((c.r[24] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
 c.call(k.jal(c, 0x00438cd8), k.pc(0x00438cd8)); return;
}
0x00438ce0 => {
let t = (c.r[31] as u32);  c.ret = t; return;
}
_ => panic!("the animation unit: no label {l:08x} in 0x004389D8"), } }
}

#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn n00436500(c: &mut Cpu, k: F) {
let mut l: u32 = 0x00436500;
loop { match l {
0x00436500 => {
c.r[2] = c.rd8((c.r[4] as u32).wrapping_add(0x00000042)) as u64;
c.r[3] = ((0u32.wrapping_add(0x000000ff)) as i32 as i64 as u64);
let t = c.r[2] == c.r[3];  if t { c.r[5] = c.rd32((c.r[4] as u32).wrapping_add(0x0000005c)) as i32 as i64 as u64;
 l = 0x00436514; continue; }
c.r[5] = c.rd32((c.r[4] as u32).wrapping_add(0x00000058)) as i32 as i64 as u64;
l = 0x00436514; continue; }
0x00436514 => {
c.f[4] = c.rd32((c.r[4] as u32).wrapping_add(0x00000044));
c.f[0] = 0u32;
c.set_fc(uycpu::fpu::eq(c.f[4], c.f[0]));
let t = c.fcr31 & 0x0080_0000 == 0;  if t { c.r[3] = c.rd8((c.r[4] as u32).wrapping_add(0x00000042)) as u64;
 l = 0x00436550; continue; }
c.r[2] = c.rd16((c.r[5] as u32).wrapping_add(0x00000004)) as i16 as i64 as u64;
c.r[1] = 0x000000003d800000;
c.f[1] = (c.r[1] as u32);
c.f[0] = (c.r[2] as u32);
c.f[0] = uycpu::fpu::cvt_s(c.f[0]);
let t = (c.r[31] as u32); c.f[0] = uycpu::fpu::mul(c.f[0], c.f[1]);
 c.ret = t; return;
}
0x00436550 => {
c.r[2] = c.rd8((c.r[4] as u32).wrapping_add(0x00000043)) as u64;
let t = c.r[3] != c.r[2];  if t { c.r[2] = c.rd16((c.r[5] as u32).wrapping_add(0x00000004)) as i16 as i64 as u64;
 l = 0x00436574; continue; }
c.r[3] = c.rd8((c.r[4] as u32).wrapping_add(0x00000040)) as u64;
c.r[2] = c.rd8((c.r[4] as u32).wrapping_add(0x00000041)) as u64;
c.r[2] = (c.r[2] < c.r[3]) as u64;
let t = c.r[2] == 0u64;  if t { c.r[2] = c.rd32((c.r[4] as u32).wrapping_add(0x0000005c)) as i32 as i64 as u64;
 l = 0x00436594; continue; }
c.r[2] = c.rd16((c.r[5] as u32).wrapping_add(0x00000004)) as i16 as i64 as u64;
l = 0x00436574; continue; }
0x00436574 => {
c.r[1] = 0x000000003d800000;
c.f[1] = (c.r[1] as u32);
c.f[0] = (c.r[2] as u32);
c.f[0] = uycpu::fpu::cvt_s(c.f[0]);
c.f[0] = uycpu::fpu::mul(c.f[0], c.f[1]);
let t = (c.r[31] as u32); c.f[0] = uycpu::fpu::add(c.f[0], c.f[4]);
 c.ret = t; return;
}
0x00436594 => {
c.r[3] = c.rd16((c.r[5] as u32).wrapping_add(0x00000004)) as i16 as i64 as u64;
c.r[4] = c.rd16((c.r[2] as u32).wrapping_add(0x00000004)) as i16 as i64 as u64;
c.r[1] = 0x000000003f800000;
c.f[2] = (c.r[1] as u32);
c.f[0] = (c.r[3] as u32);
c.f[0] = uycpu::fpu::cvt_s(c.f[0]);
c.f[1] = (c.r[4] as u32);
c.f[1] = uycpu::fpu::cvt_s(c.f[1]);
c.r[1] = 0x000000003d800000;
c.f[3] = (c.r[1] as u32);
c.f[2] = uycpu::fpu::sub(c.f[2], c.f[4]);
c.f[1] = uycpu::fpu::mul(c.f[1], c.f[4]);
c.f[0] = uycpu::fpu::mul(c.f[0], c.f[2]);
c.f[0] = uycpu::fpu::add(c.f[0], c.f[1]);
let t = (c.r[31] as u32); c.f[0] = uycpu::fpu::mul(c.f[0], c.f[3]);
 c.ret = t; return;
}
_ => panic!("the animation unit: no label {l:08x} in 0x00436500"), } }
}

#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn n004365e0(c: &mut Cpu, k: F) {
let mut l: u32 = 0x004365e0;
loop { match l {
0x004365e0 => {
c.f[0] = 0u32;
c.f[4] = c.rd32((c.r[4] as u32).wrapping_add(0x00000024));
c.set_fc(uycpu::fpu::eq(c.f[4], c.f[0]));
let t = c.fcr31 & 0x0080_0000 == 0;  c.r[5] = c.rd32((c.r[4] as u32).wrapping_add(0x00000038)) as i32 as i64 as u64;
 if t { l = 0x00436618; continue; }
c.r[2] = c.rd16((c.r[5] as u32).wrapping_add(0x00000004)) as i16 as i64 as u64;
c.r[1] = 0x000000003d800000;
c.f[1] = (c.r[1] as u32);
c.f[0] = (c.r[2] as u32);
c.f[0] = uycpu::fpu::cvt_s(c.f[0]);
let t = (c.r[31] as u32); c.f[0] = uycpu::fpu::mul(c.f[0], c.f[1]);
 c.ret = t; return;
}
0x00436618 => {
c.r[3] = c.rd8((c.r[4] as u32).wrapping_add(0x00000022)) as u64;
c.r[2] = c.rd8((c.r[4] as u32).wrapping_add(0x00000023)) as u64;
let t = c.r[3] != c.r[2];  if t { c.r[2] = c.rd16((c.r[5] as u32).wrapping_add(0x00000004)) as i16 as i64 as u64;
 l = 0x00436640; continue; }
c.r[3] = c.rd8((c.r[4] as u32).wrapping_add(0x00000020)) as u64;
c.r[2] = c.rd8((c.r[4] as u32).wrapping_add(0x00000021)) as u64;
c.r[2] = (c.r[2] < c.r[3]) as u64;
let t = c.r[2] == 0u64;  if t { c.r[2] = c.rd32((c.r[4] as u32).wrapping_add(0x0000003c)) as i32 as i64 as u64;
 l = 0x00436660; continue; }
c.r[2] = c.rd16((c.r[5] as u32).wrapping_add(0x00000004)) as i16 as i64 as u64;
l = 0x00436640; continue; }
0x00436640 => {
c.r[1] = 0x000000003d800000;
c.f[1] = (c.r[1] as u32);
c.f[0] = (c.r[2] as u32);
c.f[0] = uycpu::fpu::cvt_s(c.f[0]);
c.f[0] = uycpu::fpu::mul(c.f[0], c.f[1]);
let t = (c.r[31] as u32); c.f[0] = uycpu::fpu::add(c.f[0], c.f[4]);
 c.ret = t; return;
}
0x00436660 => {
c.r[3] = c.rd16((c.r[5] as u32).wrapping_add(0x00000004)) as i16 as i64 as u64;
c.r[4] = c.rd16((c.r[2] as u32).wrapping_add(0x00000004)) as i16 as i64 as u64;
c.r[1] = 0x000000003f800000;
c.f[2] = (c.r[1] as u32);
c.f[0] = (c.r[3] as u32);
c.f[0] = uycpu::fpu::cvt_s(c.f[0]);
c.f[1] = (c.r[4] as u32);
c.f[1] = uycpu::fpu::cvt_s(c.f[1]);
c.r[1] = 0x000000003d800000;
c.f[3] = (c.r[1] as u32);
c.f[2] = uycpu::fpu::sub(c.f[2], c.f[4]);
c.f[1] = uycpu::fpu::mul(c.f[1], c.f[4]);
c.f[0] = uycpu::fpu::mul(c.f[0], c.f[2]);
c.f[0] = uycpu::fpu::add(c.f[0], c.f[1]);
let t = (c.r[31] as u32); c.f[0] = uycpu::fpu::mul(c.f[0], c.f[3]);
 c.ret = t; return;
}
_ => panic!("the animation unit: no label {l:08x} in 0x004365E0"), } }
}

