//! The sky's draw and the sprites' cull and draw (level 1 0x4A46C8 and 0x405A58, with everything under each), done natively in
//! front of their translations (port/uyatr `JOBS`). The translated wrapper at 0x4A46C8 asks `start` (`Rt::native` 32), the
//! one at 0x405A58 asks with 36; both answer `after` (33, 37) once the translation ran when `start` let it run. `c.ret` brings
//! the function's address with 1 in its low bit (and 2 when the program's view is widened, which neither unit reads: neither
//! loads the view's tangent, view +0xB0, itself, so neither has a `VIEW_LOADS` entry).
//!
//! - 0x4A46C8 (the sky's frame, called by 0x405768 and its copy 0x4D36B8 between the chain's start 0x4A4768 and end
//!   0x4A47D8): the fields since the last frame (0x1DFBC0 - 0x1E0ED0, at most 2), then for each layer of the sky at
//!   *0x1E0E70 (+6 the count, +0x20 the layers): 0x4A45B0 steps its three angles (+4..+8) by its speeds (+0xA..+0xE) once a
//!   field and builds its rotation at 0x1E0E90 (0x40C678, from the angles times 2 pi / 65536), and 0x4A48F8 draws it,
//!   0x4A4B00 for a layer with bit 0 of +2 set, else 0x4A4958:
//!   - 0x4A5220 culls the layer's clusters (32 bytes each from +0x10) against the view's sides: each cluster's centre in view
//!     space (the view matrix 0x1E7880 times the layer's rotation), z h - (|x| - r k) for x and y, negative outside; h and v
//!     from 0x1E78C0 and k from 0x1E78E0, view_projection_setup's copies (at +0x278) of the view's +0xB0 quadword (its
//!     widened load, `VIEW_LOADS`) and of its frustum quadword +0x1F0 (made from the widened +0xF4 load), so in a story level
//!     the sky is culled with the widened view already; one byte a cluster at scratchpad 0x3F00 (1: outside);
//!   - 0x4A4C98 loads the camera's matrix at 0x1E2A00 times the rotation, the view's 0x1E7790, 0x1E77A0 and 0x1E77D0, and
//!     the four quadwords at 0x1D97A0 the packets start with;
//!   - a REF tag (0x30000007, DIRECT 7) of the layer's GS setup (boot's 0x142350 or 0x1423C0, 0x1422E0 for the second kind)
//!     at the chain (gp+0x4838);
//!   - each cluster not culled: 0x4A4D40 projects its vertices to scratchpad 0x0000 with their clip flags (the AND of them
//!     all returned: a cluster with every vertex outside one plane is dropped), then 0x4A4F38 (with the texture uploads it
//!     queues at 0x1E7900, gp+0x314C, for 0x433A28 in the chain's end) or 0x4A4E38 puts its triangles in the chain, a
//!     triangle with its three vertices outside one plane left out;
//!   - the second kind ends with two GS registers (0x4C1FE8: TEST_1 and ZBUF_1); the first sets bit 2 of gp-0xBD0's word at
//!     0x1DBCE0 when it drew with bit 1 of +2 set.
//!   The frame count is kept at 0x1E0ED0.
//! - 0x405A58 (hand-written; called by the game's code wherever it draws a sprite, 231 calls in level 1): a0 the sprite (its
//!   four corners, +0x40..+0x60 their colours and texture coordinates, +0x70..+0x88 two A+D pairs of GS registers), a1 a
//!   matrix or 0. The corners relative to the camera (0x1E2A40), scaled by gp-0x22F0's z, into the camera's matrix at
//!   0x1E2940, then scaled by the view's 0x1E7780 (x and y also by gp-0x22F0's y, the guard band) and CLIPped: a sprite with
//!   a corner past the box's +z side (any corner: mask 0x410410; the sky's clip mask 0x2F leaves +z out, so +z is the far
//!   side) or all four corners outside one side is culled, nothing written. The rest are CLIPped scaled by 0x1E7780 alone: all inside, the sprite is drawn as its four
//!   vertices (0x406098 opens the GIF packet at the chain gp+0x4838); any corner outside, 0x4060D8
//!   clips the polygon against the box's planes (0x4057C0 each crossing edge's new vertex, then 0x4059F8 and 0x405938 on the
//!   polygon, in scratchpad 0x1C40..0x1FF0) and draws it, with its A+D registers (0x406018) before it when s1 is 0, after it
//!   when not. Its callee-saved registers keep their values in their high halves while it runs (`pcpyld` / `pcpyud`; ra in
//!   s7's), as the translation leaves them.
//!
//! The functions are the same code in every program (uyatr finds each entry by size and shape: one in each story level and
//! each front end, 36 programs, and every function under it has the same size and shape in all of them); what relocation
//! changes in them, the addresses of the globals and of boot data, the gp offsets, the call targets and 0x4060D8's own code
//! addresses, is read from the code in RAM (`F`), named by level 1's address of the instruction holding it. Each is its
//! translation's statements in order on the thread's registers, so what the unit leaves is what the translation leaves: every
//! GPR (both halves), the FPU's registers and FCR31, VU0's registers, ACC, Q, the MAC, status and clip flags, RAM and the
//! scratchpad. What goes away is the code's dispatch: the table's calls, the labels, the loops' safe points (each unit is one
//! block of the thread's time, as the moby unit is).
//!
//! With UYA_NATIVE_CHECK=<job>:N (job `sky`: the calls of 0x4A46C8; `sprite`: the calls of 0x405A58; any other job is another
//! unit's and leaves these alone) the first N calls run twice from the same state: the translation, then the native unit on
//! the state as it was before; every difference in the machine's state they leave (`differences`) is reported with what the
//! calls covered, and the game goes on from the translation's state. Both runs go with the thread's interrupts held
//! (`Check::ints`), as the moby unit's check does.

use crate::{Cpu, Ext};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use uycpu::mmi;
use uycpu::vu0::{m_fmac, m_mac, m_put};

type Q = (u64, u64);

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
        let a = self.at.wrapping_add(pc.wrapping_sub(self.l1));
        assert!(a < uycpu::RAM_SIZE - 4, "the sky and sprite units' code at {a:08x}: not in RAM");
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
    /// gp plus the offset of the instruction at `pc`.
    #[inline(always)]
    fn gp(self, c: &Cpu, gp: u32, pc: u32) -> u32 {
        gp.wrapping_add(self.lo(c, pc))
    }
    /// The function the JAL at `pc` calls, named by level 1's address `l1`.
    #[inline(always)]
    fn call(self, c: &Cpu, pc: u32, l1: u32) -> F {
        F { at: (self.w(c, pc) & 0x03ff_ffff) << 2, l1 }
    }
    /// This program's address of level 1's `pc` (a return address).
    #[inline(always)]
    fn pc(self, pc: u32) -> u32 {
        self.at.wrapping_add(pc.wrapping_sub(self.l1))
    }
}

// ---- registers, 32-bit arithmetic as the R5900 keeps it, VU0's macro instructions -----------------------------------------

const AT: usize = 1;
const V0: usize = 2;
const V1: usize = 3;
const A0: usize = 4;
const A1: usize = 5;
const A2: usize = 6;
const A3: usize = 7;
const T0: usize = 8;
const T1: usize = 9;
const T2: usize = 10;
const T3: usize = 11;
const T4: usize = 12;
const T5: usize = 13;
const T6: usize = 14;
const T7: usize = 15;
const S0: usize = 16;
const S1: usize = 17;
const S2: usize = 18;
const S3: usize = 19;
const S4: usize = 20;
const S5: usize = 21;
const S6: usize = 22;
const S7: usize = 23;
const T8: usize = 24;
const T9: usize = 25;
const GP: usize = 28;
const SP: usize = 29;
const FP: usize = 30;
const RA: usize = 31;

/// A 32-bit result as the R5900 keeps it in 64 bits.
#[inline(always)]
fn sx(v: u32) -> u64 {
    v as i32 as i64 as u64
}
/// ADD / ADDU / ADDI / ADDIU.
#[inline(always)]
fn add32(a: u64, b: u64) -> u64 {
    sx((a as u32).wrapping_add(b as u32))
}
/// SUB / SUBU.
#[inline(always)]
fn sub32(a: u64, b: u64) -> u64 {
    sx((a as u32).wrapping_sub(b as u32))
}
/// SLL.
#[inline(always)]
fn sll(a: u64, s: u32) -> u64 {
    sx((a as u32) << s)
}
/// SRL.
#[inline(always)]
fn srl(a: u64, s: u32) -> u64 {
    sx((a as u32) >> s)
}

#[inline(always)]
fn w4(q: Q) -> [u32; 4] {
    [q.0 as u32, (q.0 >> 32) as u32, q.1 as u32, (q.1 >> 32) as u32]
}
#[inline(always)]
fn q4(v: [u32; 4]) -> Q {
    (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32)
}
/// A lane in every lane (the BC forms).
#[inline(always)]
fn b4(x: u32) -> [u32; 4] {
    [x; 4]
}
/// LQC2 (VF0 keeps its constant).
#[inline(always)]
fn lqc2(c: &mut Cpu, v: usize, a: u32) {
    let q = c.rd128(a);
    if v != 0 {
        c.vf[v] = w4(q);
    }
}
/// SQC2.
#[inline(always)]
fn sqc2(c: &mut Cpu, v: usize, a: u32) {
    let (lo, hi) = q4(c.vf[v]);
    c.wr128(a, lo, hi);
}
/// QMTC2.
#[inline(always)]
fn qmtc2(c: &mut Cpu, v: usize, x: Q) {
    if v != 0 {
        c.vf[v] = w4(x);
    }
}

const ACC: usize = 32;

/// An FMAC macro instruction (K: 0 ADD, 1 SUB, 2 MADD, 3 MSUB, 4 MAX, 5 MINI, 6 MUL) on lanes D into VF `fd` or ACC (`ACC`;
/// `fd` 0 writes nothing), with the MAC and status flags it makes: `uycpu::vu0::fmac`'s result on four lanes at once
/// (`m_fmac` under the game's MXCSR, `uycpu::fpu::MXCSR`, is the VU's arithmetic, `fin` on each operand by DAZ and the
/// exponent-255 clamp).
#[inline(always)]
fn fm<const K: u8, const D: u32>(c: &mut Cpu, fd: usize, fs: usize, t: [u32; 4]) {
    let r = m_fmac::<K, D>(c.vf[fs], t, c.vacc);
    if K != 4 && K != 5 {
        flags(c, m_mac::<D>(r));
    }
    if fd == ACC {
        m_put::<D>(&mut c.vacc, r);
    } else if fd != 0 {
        m_put::<D>(&mut c.vf[fd], r);
    }
}

/// A macro instruction's MAC flags (VI17) and the status flag's zero and sign bits with their sticky copies (VI16), as
/// `uycpu::vu0` sets them.
#[inline(always)]
fn flags(c: &mut Cpu, mac: u32) {
    c.vi[17] = mac;
    let st = ((mac & 0xf != 0) as u32) | (((mac & 0xf0 != 0) as u32) << 1);
    c.vi[16] = (c.vi[16] & !0x3) | st | (st << 6);
}

// ---- what the calls covered (the check's report) ---------------------------------------------------------------------------

/// Counts of the paths the native calls took: the sky's layers drawn, its clusters, the clusters drawn (not culled by the
/// view's sides or their clip flags), the textures they queued for upload; the sprites, the ones culled, the ones drawn
/// whole, the ones clipped at the near plane, the clipped polygons drawn.
const N: usize = 9;
static DONE: [AtomicU64; N] = [const { AtomicU64::new(0) }; N];
const NAMES: [&str; N] = [
    "sky layers",
    "sky clusters",
    "sky clusters drawn",
    "sky textures queued",
    "sprites",
    "sprites culled",
    "sprites drawn whole",
    "sprites clipped",
    "clipped polygons drawn",
];

// ---- the check -------------------------------------------------------------------------------------------------------------

const SPR: u32 = 0x7000_0000;

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

/// What differs between the translation's state `want` and the native unit's `got`. Nothing is left out: the unit is its
/// translation's statements on the same state (the module's notes), so every register, both halves, and every byte of the
/// memories, the frames below sp among them, is what the translation leaves. Neither unit touches a DMA channel or VU0's
/// programs; their registers and VU0's memories are compared all the same.
fn differences(want: &State, got: &State) -> Vec<String> {
    let mut d = Vec::new();
    for (what, a, b, base) in [
        ("RAM", &want.ram, &got.ram, 0u32),
        ("scratchpad", &want.spr, &got.spr, SPR),
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
    let fregs: Vec<String> =
        (0..32).filter(|&k| want.f[k] != got.f[k]).map(|k| format!("f{k} {:x}/{:x}", want.f[k], got.f[k])).collect();
    if !fregs.is_empty() || want.facc != got.facc || want.fcr31 != got.fcr31 {
        d.push(format!("FPU: {} ACC {:x}/{:x} FCR31 {:x}/{:x}", fregs.join(" "), want.facc, got.facc, want.fcr31, got.fcr31));
    }
    let vf: Vec<String> =
        (0..32).filter(|&k| want.vf[k] != got.vf[k]).map(|k| format!("vf{k} {:08x?}/{:08x?}", want.vf[k], got.vf[k])).collect();
    let vi: Vec<String> =
        (0..32).filter(|&k| want.vi[k] != got.vi[k]).map(|k| format!("vi{k} {:x}/{:x}", want.vi[k], got.vi[k])).collect();
    if !vf.is_empty() || !vi.is_empty() || want.vacc != got.vacc || want.parked != got.parked {
        d.push(format!(
            "VU0: {} {} ACC {:08x?}/{:08x?} waiting {:?}/{:?}",
            vf.join(" "),
            vi.join(" "),
            want.vacc,
            got.vacc,
            want.parked,
            got.parked
        ));
    }
    if want.view_h != got.view_h {
        d.push(format!("view_h {:#x}/{:#x}", want.view_h, got.view_h));
    }
    let ch: Vec<String> =
        (0..10).filter(|&k| want.ch[k] != got.ch[k]).map(|k| format!("channel {k} {:x?}/{:x?}", want.ch[k], got.ch[k])).collect();
    if !ch.is_empty() || want.d_stat != got.d_stat || want.intc_stat != got.intc_stat {
        d.push(format!(
            "DMA: {} D_STAT {:#x}/{:#x} INTC {:#x}/{:#x}",
            ch.join(" "),
            want.d_stat,
            got.d_stat,
            want.intc_stat,
            got.intc_stat
        ));
    }
    d
}

/// The two entries: 0x4A46C8 (`sky`) and 0x405A58 (`sprite`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Job {
    Sky,
    Sprite,
}

struct Check {
    job: Job,
    left: u64,
    /// Calls of the checked job still to run natively, unchecked, before the first checked one (`<job>:N:SKIP`): the sprites'
    /// first thousands of calls are the boot's and front end's, all drawn whole.
    skip: u64,
    /// After the skip, every EVERY-th call of the job is checked (`<job>:N:SKIP:EVERY`, 1 if left out), the rest run natively:
    /// a sample spread over the whole run reaches the sprites' bursts of culled and clipped calls, which a run of consecutive
    /// calls misses.
    every: u64,
    seen: u64,
    /// The paths the checked calls' native runs took (`DONE`'s counts over those runs only), for the summary: `DONE` itself also
    /// counts the unchecked calls.
    covered: [u64; N],
    done: u64,
    differ: u64,
    kinds: std::collections::BTreeSet<String>,
    before: Option<State>,
    /// Entries of this unit reached inside the translation being checked: they run their translations too.
    inner: u32,
    /// Whether the thread had its interrupts on at the checked call: both runs go with them held (DI), and EI after the
    /// comparison takes what waited (`mobydraw`'s `Check::ints`: a loop's back edge in the translation past the field's time
    /// gives the field up; held, each interrupt lands once, after both runs).
    ints: bool,
}

static CHECK: std::sync::Mutex<Option<Check>> = std::sync::Mutex::new(None);

/// UYA_NATIVE_CHECK=<job>:N[:SKIP[:EVERY]] for this unit, read once: N calls checked, after the job's first SKIP calls (0 if left
/// out), every EVERY-th one (1 if left out); the others run natively. Another unit's job (`moby`, `moby2`, `sweep`, `shrub`,
/// `shrub2`, any other) leaves this one unchecked.
fn checked() -> Option<(Job, u64, u64, u64)> {
    static C: std::sync::OnceLock<Option<(Job, u64, u64, u64)>> = std::sync::OnceLock::new();
    *C.get_or_init(|| {
        let v = std::env::var("UYA_NATIVE_CHECK").ok()?;
        let (job, rest) = v.split_once(':').unwrap_or_else(|| panic!("UYA_NATIVE_CHECK={v}: <job>:N[:SKIP]"));
        let job = match job {
            "sky" => Job::Sky,
            "sprite" => Job::Sprite,
            _ => return None,
        };
        let mut f = rest.split(':');
        let number = |x: Option<&str>, or: u64| {
            x.map_or(or, |x| x.parse().unwrap_or_else(|_| panic!("UYA_NATIVE_CHECK={v}: N, SKIP and EVERY are numbers of calls")))
        };
        let (n, skip, every) = (number(f.next(), 0), number(f.next(), 0), number(f.next(), 1));
        assert!(every > 0 && f.next().is_none(), "UYA_NATIVE_CHECK={v}: <job>:N[:SKIP[:EVERY]], EVERY at least 1");
        Some((job, n, skip, every))
    })
}

thread_local! {
    /// The function's address of the call being checked, for its native run after the translation's.
    static CALL: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// `Rt::native` 32 and 36: the unit natively (`c.ret` the return address), or (checking) the state saved and `c.ret` 0 so
/// the translation runs.
pub(crate) fn start(c: &mut Cpu, k: u32) {
    let job = if k == 32 { Job::Sky } else { Job::Sprite };
    let at = c.ret & !3;
    if let Some((cj, n, skip, every)) = checked() {
        let mut g = CHECK.lock().expect("the sky and sprite check");
        let ck = g.get_or_insert(Check {
            job: cj,
            left: n,
            skip,
            every,
            seen: 0,
            covered: [0; N],
            done: 0,
            differ: 0,
            kinds: Default::default(),
            before: None,
            inner: 0,
            ints: false,
        });
        if ck.before.is_some() {
            ck.inner += 1;
            c.ret = 0;
            return;
        }
        let sampled = ck.job == job && ck.skip == 0 && {
            ck.seen += 1;
            ck.seen % ck.every == 0
        };
        if ck.job == job && ck.skip > 0 {
            ck.skip -= 1;
        } else if sampled && ck.left > 0 {
            CALL.with(|w| w.set(at));
            // Interrupts on as the kernel sees them (`kernel::K::ints_on`): EIE and IE, not in an exception.
            ck.ints = c.status & 0x1_0001 == 0x1_0001 && c.status & 6 == 0;
            if ck.ints {
                c.di();
            }
            ck.before = Some(State::take(c));
            c.ret = 0;
            return;
        }
    }
    run(c, job, at);
    c.ret = c.r[RA] as u32;
}

/// `Rt::native` 33 and 37: the translation ran; the unit runs natively from the state before it and the two are compared.
pub(crate) fn after(c: &mut Cpu, k: u32) {
    let job = if k == 33 { Job::Sky } else { Job::Sprite };
    let (Some(before), ints) = ({
        let mut g = CHECK.lock().expect("the sky and sprite check");
        let ck = g.as_mut().expect("checking");
        if ck.inner > 0 {
            ck.inner -= 1;
            return;
        }
        (ck.before.take(), ck.ints)
    }) else {
        return;
    };
    let ret = c.ret;
    let want = State::take(c);
    before.put(c);
    let at = CALL.with(|w| w.get());
    let sp = c.r[SP] as u32;
    let pre: [u64; N] = std::array::from_fn(|i| DONE[i].load(Relaxed));
    run(c, job, at);
    let took: [u64; N] = std::array::from_fn(|i| DONE[i].load(Relaxed) - pre[i]);
    c.ret = c.r[RA] as u32;
    let got = State::take(c);
    let d = differences(&want, &got);
    want.put(c);
    c.ret = ret;
    let mut g = CHECK.lock().expect("the sky and sprite check");
    let ck = g.as_mut().expect("checking");
    ck.left -= 1;
    ck.done += 1;
    for (a, t) in ck.covered.iter_mut().zip(took) {
        *a += t;
    }
    if !d.is_empty() {
        ck.differ += 1;
        // The first calls that differ, and the first of each other kind of difference (what differs, without the values).
        let kind: String = d.iter().map(|x| x.split(':').next().unwrap_or(x)).collect::<Vec<_>>().join("+");
        if ck.differ <= 6 || ck.kinds.insert(kind) {
            println!("UYA_NATIVE_CHECK: {job:?} call {} (at {at:08x}, sp {sp:08x}, ra {:08x}): {}", ck.done, ret, d.join("; "));
        }
    }
    if ck.left == 0 || ck.done % 100 == 0 {
        let n: Vec<String> = ck.covered.iter().zip(NAMES).map(|(a, name)| format!("{name} {a}")).collect();
        println!("UYA_NATIVE_CHECK: {job:?}: {} calls checked, {} differ (in the checked calls: {})", ck.done, ck.differ, n.join(", "));
    }
    drop(g);
    // The interrupts held for the two runs (`Check::ints`): what came due runs now, as at the game's own EI.
    if ints {
        c.ei();
        c.ret = ret;
    }
}

/// The unit from either entry, natively.
fn run(c: &mut Cpu, job: Job, at: u32) {
    let mut n = [0u64; N];
    match job {
        Job::Sky => f4a46c8(c, F { at, l1: 0x4a_46c8 }, &mut n),
        Job::Sprite => f405a58(c, F { at, l1: 0x40_5a58 }, &mut n),
    }
    for (a, v) in DONE.iter().zip(n) {
        if v != 0 {
            a.fetch_add(v, Relaxed);
        }
    }
}

// ---- the sky: 0x4A46C8 and everything under it --------------------------------------------------------------------------------
//
// Compiled C (0x4A46C8, 0x4A45B0, 0x4A48F8, 0x4A4958, 0x4A4B00, 0x4C1FE8) and hand-written code (0x40C678, 0x4A5220, 0x4A4C98,
// 0x4A4D40, 0x4A4F38, 0x4A4E38): each its translation's statements in order on the thread's registers, the blocks by level 1's
// labels.

fn f4a46c8(c: &mut Cpu, k: F, n: &mut [u64; N]) {
    let mut l: u32 = 0x004a46c8;
    loop {
        match l {
            0x004a46c8 => {
                c.r[SP] = add32(c.r[SP], 0xffffffe0);
                c.r[V0] = sx(k.hi(c, 0x004a46cc));
                c.r[V0] = sx(c.rd32((c.r[V0] as u32).wrapping_add(k.lo(c, 0x004a46d0))));
                c.r[A1] = sx(k.hi(c, 0x004a46d4));
                c.r[A1] = sx(c.rd32((c.r[A1] as u32).wrapping_add(k.lo(c, 0x004a46d8))));
                c.r[A2] = sx(0x00000002);
                c.wr64(c.r[SP] as u32, c.r[S0]);
                c.wr64((c.r[SP] as u32).wrapping_add(0x00000008), c.r[S1]);
                c.r[S0] = 0;
                c.wr64((c.r[SP] as u32).wrapping_add(0x00000010), c.r[RA]);
                c.r[S1] = sub32(c.r[A1], c.r[V0]);
                c.r[V1] = sx(k.hi(c, 0x004a46f4));
                c.r[V1] = sx(c.rd32((c.r[V1] as u32).wrapping_add(k.lo(c, 0x004a46f8))));
                c.r[V0] = ((c.r[S1] as i64) < 3) as u64;
                c.r[A0] = c.rd16((c.r[V1] as u32).wrapping_add(0x00000006)) as i16 as i64 as u64;
                let t = (c.r[A0] as i64) <= 0;
                if c.r[V0] == 0 {
                    c.r[S1] = c.r[A2];
                }
                if t {
                    l = 0x004a473c;
                    continue;
                }
                c.r[A0] = c.r[S0];
                l = 0x004a4710;
                continue;
            }
            0x004a4710 => {
                c.r[RA] = k.pc(0x004a4718) as u64;
                c.r[A1] = c.r[S1];
                f4a45b0(c, k.call(c, 0x004a4710, 0x4a_45b0));
                c.r[RA] = k.pc(0x004a4720) as u64;
                c.r[A0] = c.r[S0];
                f4a48f8(c, k.call(c, 0x004a4718, 0x4a_48f8), n);
                c.r[S0] = add32(c.r[S0], 0x00000001);
                c.r[V1] = sx(k.hi(c, 0x004a4724));
                c.r[V1] = sx(c.rd32((c.r[V1] as u32).wrapping_add(k.lo(c, 0x004a4728))));
                c.r[V0] = c.rd16((c.r[V1] as u32).wrapping_add(0x00000006)) as i16 as i64 as u64;
                c.r[V0] = ((c.r[S0] as i64) < (c.r[V0] as i64)) as u64;
                let t = c.r[V0] != 0u64;
                if t {
                    c.r[A0] = c.r[S0];
                    l = 0x004a4710;
                    continue;
                }
                l = 0x004a473c;
                continue;
            }
            0x004a473c => {
                c.r[V0] = sx(k.hi(c, 0x004a473c));
                c.r[V0] = sx(c.rd32((c.r[V0] as u32).wrapping_add(k.lo(c, 0x004a4740))));
                c.r[S0] = c.rd64(c.r[SP] as u32);
                c.r[S1] = c.rd64((c.r[SP] as u32).wrapping_add(0x00000008));
                c.r[RA] = c.rd64((c.r[SP] as u32).wrapping_add(0x00000010));
                c.r[AT] = sx(k.hi(c, 0x004a4750));
                c.wr32((c.r[AT] as u32).wrapping_add(k.lo(c, 0x004a4754)), c.r[V0] as u32);
                c.r[SP] = add32(c.r[SP], 0x00000020);
                return;
            }
            _ => unreachable!("0x4A46C8: no block at {l:08x}"),
        }
    }
}

fn f4a45b0(c: &mut Cpu, k: F) {
    let mut l: u32 = 0x004a45b0;
    loop {
        match l {
            0x004a45b0 => {
                c.r[SP] = add32(c.r[SP], 0xffffffe0);
                c.r[V0] = sx(k.hi(c, 0x004a45b4));
                c.r[V0] = sx(c.rd32((c.r[V0] as u32).wrapping_add(k.lo(c, 0x004a45b8))));
                c.wr64((c.r[SP] as u32).wrapping_add(0x00000010), c.r[RA]);
                c.r[A0] = sll(c.r[A0], 2);
                c.r[V0] = add32(c.r[V0], c.r[A0]);
                let t = (c.r[A1] as i64) <= 0;
                c.r[A2] = sx(c.rd32((c.r[V0] as u32).wrapping_add(0x00000020)));
                if t {
                    l = 0x004a4614;
                    continue;
                }
                c.r[T4] = c.rd16((c.r[A2] as u32).wrapping_add(0x0000000a)) as u64;
                c.r[T3] = c.rd16((c.r[A2] as u32).wrapping_add(0x0000000c)) as u64;
                c.r[T2] = c.rd16((c.r[A2] as u32).wrapping_add(0x0000000e)) as u64;
                c.r[T0] = c.rd16((c.r[A2] as u32).wrapping_add(0x00000004)) as u64;
                c.r[A0] = c.rd16((c.r[A2] as u32).wrapping_add(0x00000006)) as u64;
                c.r[V1] = c.rd16((c.r[A2] as u32).wrapping_add(0x00000008)) as u64;
                l = 0x004a45e8;
                continue;
            }
            0x004a45e8 => {
                c.r[T1] = add32(c.r[T0], c.r[T4]);
                c.r[A3] = add32(c.r[A0], c.r[T3]);
                c.r[V0] = add32(c.r[V1], c.r[T2]);
                c.r[T0] = c.r[T1];
                c.r[A0] = c.r[A3];
                c.r[A1] = add32(c.r[A1], 0xffffffff);
                let t = c.r[A1] != 0u64;
                c.r[V1] = c.r[V0];
                if t {
                    l = 0x004a45e8;
                    continue;
                }
                c.wr16((c.r[A2] as u32).wrapping_add(0x00000008), c.r[V0] as u16);
                c.wr16((c.r[A2] as u32).wrapping_add(0x00000006), c.r[A3] as u16);
                c.wr16((c.r[A2] as u32).wrapping_add(0x00000004), c.r[T1] as u16);
                l = 0x004a4614;
                continue;
            }
            0x004a4614 => {
                c.wr128(c.r[SP] as u32, 0u64, 0u64);
                c.r[V0] = c.rd16((c.r[A2] as u32).wrapping_add(0x00000004)) as i16 as i64 as u64;
                let t = c.r[V0] == 0u64;
                if t {
                    c.r[V0] = c.rd16((c.r[A2] as u32).wrapping_add(0x00000006)) as i16 as i64 as u64;
                    l = 0x004a464c;
                    continue;
                }
                c.f[0] = c.r[V0] as u32;
                c.f[0] = uycpu::fpu::cvt_s(c.f[0]);
                c.r[AT] = 0x0000000038c90000;
                c.r[AT] = c.r[AT] | 0xfdb;
                c.f[1] = c.r[AT] as u32;
                c.f[0] = uycpu::fpu::mul(c.f[0], c.f[1]);
                c.wr32(c.r[SP] as u32, c.f[0]);
                c.r[V0] = c.rd16((c.r[A2] as u32).wrapping_add(0x00000006)) as i16 as i64 as u64;
                l = 0x004a464c;
                continue;
            }
            0x004a464c => {
                let t = c.r[V0] == 0u64;
                if t {
                    c.r[V0] = c.rd16((c.r[A2] as u32).wrapping_add(0x00000008)) as i16 as i64 as u64;
                    l = 0x004a467c;
                    continue;
                }
                c.f[0] = c.r[V0] as u32;
                c.f[0] = uycpu::fpu::cvt_s(c.f[0]);
                c.r[AT] = 0x0000000038c90000;
                c.r[AT] = c.r[AT] | 0xfdb;
                c.f[1] = c.r[AT] as u32;
                c.f[0] = uycpu::fpu::mul(c.f[0], c.f[1]);
                c.wr32((c.r[SP] as u32).wrapping_add(0x00000004), c.f[0]);
                c.r[V0] = c.rd16((c.r[A2] as u32).wrapping_add(0x00000008)) as i16 as i64 as u64;
                l = 0x004a467c;
                continue;
            }
            0x004a467c => {
                let t = c.r[V0] == 0u64;
                if t {
                    l = 0x004a46a8;
                    continue;
                }
                c.f[0] = c.r[V0] as u32;
                c.f[0] = uycpu::fpu::cvt_s(c.f[0]);
                c.r[AT] = 0x0000000038c90000;
                c.r[AT] = c.r[AT] | 0xfdb;
                c.f[1] = c.r[AT] as u32;
                c.f[0] = uycpu::fpu::mul(c.f[0], c.f[1]);
                c.wr32((c.r[SP] as u32).wrapping_add(0x00000008), c.f[0]);
                l = 0x004a46a8;
                continue;
            }
            0x004a46a8 => {
                c.r[A0] = sx(k.hi(c, 0x004a46a8));
                c.r[A0] = add32(c.r[A0], k.lo(c, 0x004a46ac) as u64);
                c.r[RA] = k.pc(0x004a46b8) as u64;
                c.r[A1] = c.r[SP];
                f40c678(c);
                c.r[RA] = c.rd64((c.r[SP] as u32).wrapping_add(0x00000010));
                c.r[SP] = add32(c.r[SP], 0x00000020);
                return;
            }
            _ => unreachable!("0x4A45B0: no block at {l:08x}"),
        }
    }
}

fn f40c678(c: &mut Cpu) {
    let mut l: u32 = 0x0040c678;
    loop {
        match l {
            0x0040c678 => {
                c.r[AT] = 0x0000000040490000;
                c.r[V0] = 0x000000003fc90000;
                c.r[AT] = c.r[AT] | 0xfdb;
                c.r[V0] = c.r[V0] | 0xfdb;
                c.f[11] = c.r[V0] as u32;
                c.f[9] = c.r[AT] as u32;
                c.f[12] = c.f[11] ^ 0x8000_0000;
                c.f[10] = c.f[9] ^ 0x8000_0000;
                fm::<6, 15>(c, 1, 0, b4(c.vf[0][0]));
                fm::<6, 15>(c, 2, 0, b4(c.vf[0][0]));
                {
                    let v = c.vf[0];
                    uycpu::vu0::mov::<15>(c, 3, [v[1], v[2], v[3], v[0]]);
                }
                uycpu::vu0::mov::<15>(c, 4, c.vf[0]);
                fm::<0, 8>(c, 1, 1, b4(c.vf[0][3]));
                fm::<0, 4>(c, 2, 2, b4(c.vf[0][3]));
                c.r[AT] = sx(c.rd32((c.r[A1] as u32).wrapping_add(0x00000008)));
                c.r[T7] = sx(0x00000000);
                let t = c.r[AT] == 0u64;
                c.f[13] = c.r[AT] as u32;
                if t {
                    l = 0x0040c6d8;
                    continue;
                }
                c.f[14] = uycpu::fpu::add(c.f[13], c.f[11]);
                l = 0x0040c7b0;
                continue;
            }
            0x0040c6c8 => {
                fm::<0, 8>(c, 1, 0, b4(c.vf[15][0]));
                fm::<0, 4>(c, 1, 0, b4(c.vf[14][0]));
                fm::<0, 4>(c, 2, 0, b4(c.vf[15][0]));
                fm::<1, 8>(c, 2, 0, b4(c.vf[14][0]));
                l = 0x0040c6d8;
                continue;
            }
            0x0040c6d8 => {
                c.r[AT] = sx(c.rd32((c.r[A1] as u32).wrapping_add(0x00000004)));
                c.r[T7] = sx(0x00000001);
                let t = c.r[AT] == 0u64;
                c.f[13] = c.r[AT] as u32;
                if t {
                    l = 0x0040c738;
                    continue;
                }
                c.f[14] = uycpu::fpu::add(c.f[13], c.f[11]);
                l = 0x0040c7b0;
                continue;
            }
            0x0040c6f0 => {
                fm::<6, 15>(c, 5, 0, b4(c.vf[0][0]));
                fm::<6, 15>(c, 6, 0, b4(c.vf[0][0]));
                fm::<6, 15>(c, 7, 0, b4(c.vf[0][0]));
                fm::<0, 8>(c, 5, 0, b4(c.vf[15][0]));
                fm::<1, 2>(c, 5, 0, b4(c.vf[14][0]));
                fm::<0, 4>(c, 6, 6, b4(c.vf[0][3]));
                fm::<0, 2>(c, 7, 0, b4(c.vf[15][0]));
                fm::<0, 8>(c, 7, 0, b4(c.vf[14][0]));
                fm::<6, 15>(c, ACC, 5, b4(c.vf[1][0]));
                fm::<2, 15>(c, ACC, 6, b4(c.vf[1][1]));
                fm::<2, 15>(c, 1, 7, b4(c.vf[1][2]));
                fm::<6, 15>(c, ACC, 5, b4(c.vf[2][0]));
                fm::<2, 15>(c, ACC, 6, b4(c.vf[2][1]));
                fm::<2, 15>(c, 2, 7, b4(c.vf[2][2]));
                fm::<6, 15>(c, ACC, 5, b4(c.vf[3][0]));
                fm::<2, 15>(c, ACC, 6, b4(c.vf[3][1]));
                fm::<2, 15>(c, 3, 7, b4(c.vf[3][2]));
                l = 0x0040c738;
                continue;
            }
            0x0040c738 => {
                c.r[AT] = sx(c.rd32(c.r[A1] as u32));
                c.r[T7] = sx(0x00000002);
                let t = c.r[AT] == 0u64;
                c.f[13] = c.r[AT] as u32;
                if t {
                    l = 0x0040c798;
                    continue;
                }
                c.f[14] = uycpu::fpu::add(c.f[13], c.f[11]);
                l = 0x0040c7b0;
                continue;
            }
            0x0040c750 => {
                fm::<6, 15>(c, 5, 0, b4(c.vf[0][0]));
                fm::<6, 15>(c, 6, 0, b4(c.vf[0][0]));
                fm::<6, 15>(c, 7, 0, b4(c.vf[0][0]));
                fm::<0, 8>(c, 5, 5, b4(c.vf[0][3]));
                fm::<0, 4>(c, 6, 0, b4(c.vf[15][0]));
                fm::<0, 2>(c, 6, 0, b4(c.vf[14][0]));
                fm::<0, 2>(c, 7, 0, b4(c.vf[15][0]));
                fm::<1, 4>(c, 7, 0, b4(c.vf[14][0]));
                fm::<6, 15>(c, ACC, 5, b4(c.vf[1][0]));
                fm::<2, 15>(c, ACC, 6, b4(c.vf[1][1]));
                fm::<2, 15>(c, 1, 7, b4(c.vf[1][2]));
                fm::<6, 15>(c, ACC, 5, b4(c.vf[2][0]));
                fm::<2, 15>(c, ACC, 6, b4(c.vf[2][1]));
                fm::<2, 15>(c, 2, 7, b4(c.vf[2][2]));
                fm::<6, 15>(c, ACC, 5, b4(c.vf[3][0]));
                fm::<2, 15>(c, ACC, 6, b4(c.vf[3][1]));
                fm::<2, 15>(c, 3, 7, b4(c.vf[3][2]));
                l = 0x0040c798;
                continue;
            }
            0x0040c798 => {
                sqc2(c, 1, c.r[A0] as u32);
                sqc2(c, 2, (c.r[A0] as u32).wrapping_add(0x00000010));
                sqc2(c, 3, (c.r[A0] as u32).wrapping_add(0x00000020));
                sqc2(c, 4, (c.r[A0] as u32).wrapping_add(0x00000030));
                return;
            }
            0x0040c7b0 => {
                c.set_fc(uycpu::fpu::lt(c.f[13], c.f[11]));
                let t = c.fcr31 & 0x0080_0000 != 0;
                c.set_fc(uycpu::fpu::lt(c.f[13], c.f[12]));
                if t {
                    l = 0x0040c7c4;
                    continue;
                }
                c.f[13] = uycpu::fpu::sub(c.f[9], c.f[13]);
                l = 0x0040c7c4;
                continue;
            }
            0x0040c7c4 => {
                let t = c.fcr31 & 0x0080_0000 == 0;
                c.f[15] = 0u32;
                if t {
                    l = 0x0040c7d4;
                    continue;
                }
                c.f[13] = uycpu::fpu::sub(c.f[10], c.f[13]);
                l = 0x0040c7d4;
                continue;
            }
            0x0040c7d4 => {
                c.f[0] = uycpu::fpu::mul(c.f[13], c.f[13]);
                c.facc = uycpu::fpu::add(c.f[13], c.f[15]);
                c.f[1] = uycpu::fpu::mul(c.f[13], c.f[0]);
                c.r[T3] = 0xffffffffbe2a0000;
                c.r[T4] = 0x000000003c080000;
                c.r[T5] = 0xffffffffb94f0000;
                c.r[T6] = 0x00000000362e0000;
                c.f[2] = uycpu::fpu::mul(c.f[1], c.f[0]);
                c.r[T3] = c.r[T3] | 0xaaa4;
                c.r[T4] = c.r[T4] | 0x873e;
                c.r[T5] = c.r[T5] | 0xb21f;
                c.r[T6] = c.r[T6] | 0x9c14;
                c.f[3] = uycpu::fpu::mul(c.f[2], c.f[0]);
                c.f[5] = c.r[T3] as u32;
                c.f[6] = c.r[T4] as u32;
                c.f[7] = c.r[T5] as u32;
                c.f[8] = c.r[T6] as u32;
                c.f[4] = uycpu::fpu::mul(c.f[3], c.f[0]);
                c.facc = uycpu::fpu::add(c.facc, uycpu::fpu::mul(c.f[1], c.f[5]));
                c.facc = uycpu::fpu::add(c.facc, uycpu::fpu::mul(c.f[2], c.f[6]));
                c.facc = uycpu::fpu::add(c.facc, uycpu::fpu::mul(c.f[3], c.f[7]));
                c.f[0] = uycpu::fpu::add(c.facc, uycpu::fpu::mul(c.f[4], c.f[8]));
                c.set_fc(uycpu::fpu::lt(c.f[14], c.f[11]));
                let t = c.fcr31 & 0x0080_0000 != 0;
                c.set_fc(uycpu::fpu::lt(c.f[14], c.f[12]));
                if t {
                    l = 0x0040c840;
                    continue;
                }
                c.f[14] = uycpu::fpu::sub(c.f[9], c.f[14]);
                l = 0x0040c840;
                continue;
            }
            0x0040c840 => {
                let t = c.fcr31 & 0x0080_0000 == 0;
                c.r[AT] = c.f[0] as i32 as i64 as u64;
                if t {
                    l = 0x0040c850;
                    continue;
                }
                c.f[14] = uycpu::fpu::sub(c.f[10], c.f[14]);
                l = 0x0040c850;
                continue;
            }
            0x0040c850 => {
                c.f[0] = uycpu::fpu::mul(c.f[14], c.f[14]);
                c.facc = uycpu::fpu::add(c.f[14], c.f[15]);
                c.f[1] = uycpu::fpu::mul(c.f[14], c.f[0]);
                c.f[2] = uycpu::fpu::mul(c.f[1], c.f[0]);
                c.f[3] = uycpu::fpu::mul(c.f[2], c.f[0]);
                c.f[4] = uycpu::fpu::mul(c.f[3], c.f[0]);
                c.facc = uycpu::fpu::add(c.facc, uycpu::fpu::mul(c.f[1], c.f[5]));
                c.facc = uycpu::fpu::add(c.facc, uycpu::fpu::mul(c.f[2], c.f[6]));
                c.facc = uycpu::fpu::add(c.facc, uycpu::fpu::mul(c.f[3], c.f[7]));
                c.f[0] = uycpu::fpu::add(c.facc, uycpu::fpu::mul(c.f[4], c.f[8]));
                c.r[V0] = c.f[0] as i32 as i64 as u64;
                qmtc2(c, 14, (c.r[AT], c.rh[AT]));
                qmtc2(c, 15, (c.r[V0], c.rh[V0]));
                let t = c.r[T7] == 0u64;
                c.r[T7] = add32(c.r[T7], 0xffffffff);
                if t {
                    l = 0x0040c6c8;
                    continue;
                }
                let t = c.r[T7] == 0u64;
                c.r[T7] = add32(c.r[T7], 0xffffffff);
                if t {
                    l = 0x0040c6f0;
                    continue;
                }
                l = 0x0040c750;
                continue;
            }
            _ => unreachable!("0x40C678: no block at {l:08x}"),
        }
    }
}

fn f4a48f8(c: &mut Cpu, k: F, n: &mut [u64; N]) {
    let mut l: u32 = 0x004a48f8;
    loop {
        match l {
            0x004a48f8 => {
                c.r[SP] = add32(c.r[SP], 0xfffffff0);
                c.r[V1] = sx(k.hi(c, 0x004a48fc));
                c.r[V1] = sx(c.rd32((c.r[V1] as u32).wrapping_add(k.lo(c, 0x004a4900))));
                c.wr64(c.r[SP] as u32, c.r[RA]);
                c.r[V0] = c.rd16((c.r[V1] as u32).wrapping_add(0x00000006)) as i16 as i64 as u64;
                c.r[V0] = ((c.r[A0] as i64) < (c.r[V0] as i64)) as u64;
                let t = c.r[V0] == 0u64;
                c.r[V0] = sll(c.r[A0], 2);
                if t {
                    l = 0x004a4948;
                    continue;
                }
                c.r[V0] = add32(c.r[V1], c.r[V0]);
                c.r[A0] = sx(c.rd32((c.r[V0] as u32).wrapping_add(0x00000020)));
                c.r[V1] = c.rd16((c.r[A0] as u32).wrapping_add(0x00000002)) as u64;
                c.r[V1] = c.r[V1] & 0x1;
                n[0] += 1;
                let t = c.r[V1] == 0u64;
                if t {
                    l = 0x004a4940;
                    continue;
                }
                c.r[RA] = k.pc(0x004a4938) as u64;
                f4a4b00(c, k.call(c, 0x004a4930, 0x4a_4b00), n);
                let t = 0u64 == 0u64;
                c.r[RA] = c.rd64(c.r[SP] as u32);
                if t {
                    l = 0x004a494c;
                    continue;
                }
                l = 0x004a4940;
                continue;
            }
            0x004a4940 => {
                c.r[RA] = k.pc(0x004a4948) as u64;
                f4a4958(c, k.call(c, 0x004a4940, 0x4a_4958), n);
                l = 0x004a4948;
                continue;
            }
            0x004a4948 => {
                c.r[RA] = c.rd64(c.r[SP] as u32);
                l = 0x004a494c;
                continue;
            }
            0x004a494c => {
                c.r[SP] = add32(c.r[SP], 0x00000010);
                return;
            }
            _ => unreachable!("0x4A48F8: no block at {l:08x}"),
        }
    }
}

fn f4a4958(c: &mut Cpu, k: F, n: &mut [u64; N]) {
    let gp = c.r[GP] as u32;
    let mut l: u32 = 0x004a4958;
    loop {
        match l {
            0x004a4958 => {
                c.r[SP] = add32(c.r[SP], 0xffffffb0);
                c.wr64((c.r[SP] as u32).wrapping_add(0x00000028), c.r[S5]);
                c.wr64(c.r[SP] as u32, c.r[S0]);
                c.r[S5] = c.r[A0];
                c.wr64((c.r[SP] as u32).wrapping_add(0x00000008), c.r[S1]);
                c.wr64((c.r[SP] as u32).wrapping_add(0x00000010), c.r[S2]);
                c.wr64((c.r[SP] as u32).wrapping_add(0x00000018), c.r[S3]);
                c.wr64((c.r[SP] as u32).wrapping_add(0x00000020), c.r[S4]);
                c.wr64((c.r[SP] as u32).wrapping_add(0x00000030), c.r[S6]);
                c.wr64((c.r[SP] as u32).wrapping_add(0x00000038), c.r[S7]);
                c.wr64((c.r[SP] as u32).wrapping_add(0x00000040), c.r[FP]);
                c.wr64((c.r[SP] as u32).wrapping_add(0x00000048), c.r[RA]);
                c.r[A1] = c.rd16(c.r[S5] as u32) as i16 as i64 as u64;
                let t = c.r[A1] == 0u64;
                c.r[S0] = add32(c.r[S5], 0x00000010);
                if t {
                    l = 0x004a4ad0;
                    continue;
                }
                c.r[A2] = 0x0000000070000000;
                c.r[A0] = c.r[S0];
                c.r[RA] = k.pc(0x004a49a4) as u64;
                c.r[A2] = c.r[A2] | 0x3f00;
                f4a5220(c, k.call(c, 0x004a499c, 0x4a_5220));
                c.r[RA] = k.pc(0x004a49ac) as u64;
                c.r[S4] = 0x0000000070000000;
                f4a4c98(c, k.call(c, 0x004a49a4, 0x4a_4c98));
                c.r[S4] = c.r[S4] | 0x3f00;
                c.r[A1] = sx(k.hi(c, 0x004a49b0));
                c.r[A1] = sx(c.rd32((c.r[A1] as u32).wrapping_add(k.lo(c, 0x004a49b4))));
                c.r[V0] = 0x0000000030000000;
                c.r[V1] = c.rd16((c.r[S5] as u32).wrapping_add(0x00000002)) as u64;
                c.r[V0] = c.r[V0] | 0x7;
                c.wr32(c.r[A1] as u32, c.r[V0] as u32);
                c.r[A0] = 0x0000000050000000;
                c.r[V1] = srl(c.r[V1], 1);
                c.r[A0] = c.r[A0] | 0x7;
                c.r[V0] = sx(k.hi(c, 0x004a49d4));
                c.r[V0] = sx(c.rd32((c.r[V0] as u32).wrapping_add(k.lo(c, 0x004a49d8))));
                c.r[FP] = c.r[V1] & 0x1;
                c.r[S7] = 0;
                c.wr32((c.r[V0] as u32).wrapping_add(0x00000008), 0u64 as u32);
                c.r[V1] = sx(k.hi(c, 0x004a49e8));
                c.r[V1] = sx(c.rd32((c.r[V1] as u32).wrapping_add(k.lo(c, 0x004a49ec))));
                let t = c.r[FP] == 0u64;
                c.wr32((c.r[V1] as u32).wrapping_add(0x0000000c), c.r[A0] as u32);
                if t {
                    l = 0x004a4a0c;
                    continue;
                }
                c.r[V1] = sx(k.hi(c, 0x004a49f8));
                c.r[V1] = sx(c.rd32((c.r[V1] as u32).wrapping_add(k.lo(c, 0x004a49fc))));
                c.r[V0] = sx(k.hi(c, 0x004a4a00));
                let t = 0u64 == 0u64;
                c.r[V0] = add32(c.r[V0], k.lo(c, 0x004a4a08) as u64);
                if t {
                    l = 0x004a4a1c;
                    continue;
                }
                l = 0x004a4a0c;
                continue;
            }
            0x004a4a0c => {
                c.r[V1] = sx(k.hi(c, 0x004a4a0c));
                c.r[V1] = sx(c.rd32((c.r[V1] as u32).wrapping_add(k.lo(c, 0x004a4a10))));
                c.r[V0] = sx(k.hi(c, 0x004a4a14));
                c.r[V0] = add32(c.r[V0], k.lo(c, 0x004a4a18) as u64);
                l = 0x004a4a1c;
                continue;
            }
            0x004a4a1c => {
                c.wr32((c.r[V1] as u32).wrapping_add(0x00000004), c.r[V0] as u32);
                c.r[V0] = sx(k.hi(c, 0x004a4a20));
                c.r[V0] = sx(c.rd32((c.r[V0] as u32).wrapping_add(k.lo(c, 0x004a4a24))));
                c.r[S6] = sx(0xffffffff);
                c.r[V1] = c.rd16(c.r[S5] as u32) as i16 as i64 as u64;
                c.r[S3] = 0;
                c.r[V0] = add32(c.r[V0], 0x00000010);
                let t = (c.r[V1] as i64) <= 0;
                c.wr32(k.gp(c, gp, 0x004a4a3c), c.r[V0] as u32);
                if t {
                    l = 0x004a4ab4;
                    continue;
                }
                l = 0x004a4a40;
                continue;
            }
            0x004a4a40 => {
                n[1] += 1;
                c.r[V0] = c.rd8(c.r[S4] as u32) as u64;
                let t = c.r[V0] != 0u64;
                if t {
                    c.r[V0] = c.rd16(c.r[S5] as u32) as i16 as i64 as u64;
                    l = 0x004a4aa0;
                    continue;
                }
                c.r[V1] = sx(c.rd32((c.r[S0] as u32).wrapping_add(0x00000010)));
                c.r[A1] = 0x0000000070000000;
                c.r[V0] = c.rd16((c.r[S0] as u32).wrapping_add(0x0000001c)) as i16 as i64 as u64;
                c.r[A0] = c.rd16((c.r[S0] as u32).wrapping_add(0x00000018)) as i16 as i64 as u64;
                c.r[A3] = c.rd16((c.r[S0] as u32).wrapping_add(0x0000001a)) as i16 as i64 as u64;
                c.r[S2] = add32(c.r[V1], c.r[V0]);
                c.r[A2] = c.rd16((c.r[S0] as u32).wrapping_add(0x00000014)) as i16 as i64 as u64;
                c.r[A0] = add32(c.r[V1], c.r[A0]);
                c.r[RA] = k.pc(0x004a4a74) as u64;
                c.r[S1] = add32(c.r[V1], c.r[A3]);
                f4a4d40(c);
                let t = c.r[V0] != 0u64;
                if t {
                    c.r[V0] = c.rd16(c.r[S5] as u32) as i16 as i64 as u64;
                    l = 0x004a4aa0;
                    continue;
                }
                c.r[A0] = c.rd16((c.r[S0] as u32).wrapping_add(0x00000016)) as i16 as i64 as u64;
                c.r[T0] = c.r[S6];
                c.r[A1] = c.r[S2];
                c.r[A2] = c.r[S1];
                c.r[A3] = 0x0000000070000000;
                c.r[RA] = k.pc(0x004a4a98) as u64;
                c.r[S7] = sx(0x00000001);
                n[2] += 1;
                f4a4f38(c, k.call(c, 0x004a4a90, 0x4a_4f38), n);
                c.r[S6] = c.r[V0];
                c.r[V0] = c.rd16(c.r[S5] as u32) as i16 as i64 as u64;
                l = 0x004a4aa0;
                continue;
            }
            0x004a4aa0 => {
                c.r[S3] = add32(c.r[S3], 0x00000001);
                c.r[S0] = add32(c.r[S0], 0x00000020);
                c.r[V0] = ((c.r[S3] as i64) < (c.r[V0] as i64)) as u64;
                let t = c.r[V0] != 0u64;
                c.r[S4] = add32(c.r[S4], 0x00000001);
                if t {
                    l = 0x004a4a40;
                    continue;
                }
                l = 0x004a4ab4;
                continue;
            }
            0x004a4ab4 => {
                let t = c.r[FP] == 0u64;
                c.r[S0] = c.rd64(c.r[SP] as u32);
                if t {
                    l = 0x004a4ad4;
                    continue;
                }
                let t = c.r[S7] == 0u64;
                c.r[V0] = sx(c.rd32(k.gp(c, gp, 0x004a4ac0)));
                if t {
                    l = 0x004a4ad4;
                    continue;
                }
                c.r[V0] = c.r[V0] | 0x4;
                c.r[AT] = sx(k.hi(c, 0x004a4ac8));
                c.wr32((c.r[AT] as u32).wrapping_add(k.lo(c, 0x004a4acc)), c.r[V0] as u32);
                l = 0x004a4ad0;
                continue;
            }
            0x004a4ad0 => {
                c.r[S0] = c.rd64(c.r[SP] as u32);
                l = 0x004a4ad4;
                continue;
            }
            0x004a4ad4 => {
                c.r[S1] = c.rd64((c.r[SP] as u32).wrapping_add(0x00000008));
                c.r[S2] = c.rd64((c.r[SP] as u32).wrapping_add(0x00000010));
                c.r[S3] = c.rd64((c.r[SP] as u32).wrapping_add(0x00000018));
                c.r[S4] = c.rd64((c.r[SP] as u32).wrapping_add(0x00000020));
                c.r[S5] = c.rd64((c.r[SP] as u32).wrapping_add(0x00000028));
                c.r[S6] = c.rd64((c.r[SP] as u32).wrapping_add(0x00000030));
                c.r[S7] = c.rd64((c.r[SP] as u32).wrapping_add(0x00000038));
                c.r[FP] = c.rd64((c.r[SP] as u32).wrapping_add(0x00000040));
                c.r[RA] = c.rd64((c.r[SP] as u32).wrapping_add(0x00000048));
                c.r[SP] = add32(c.r[SP], 0x00000050);
                return;
            }
            _ => unreachable!("0x4A4958: no block at {l:08x}"),
        }
    }
}

fn f4a4b00(c: &mut Cpu, k: F, n: &mut [u64; N]) {
    let gp = c.r[GP] as u32;
    let mut l: u32 = 0x004a4b00;
    loop {
        match l {
            0x004a4b00 => {
                c.r[SP] = add32(c.r[SP], 0xffffffc0);
                c.wr64((c.r[SP] as u32).wrapping_add(0x00000030), c.r[S6]);
                c.wr64(c.r[SP] as u32, c.r[S0]);
                c.r[S6] = c.r[A0];
                c.wr64((c.r[SP] as u32).wrapping_add(0x00000008), c.r[S1]);
                c.wr64((c.r[SP] as u32).wrapping_add(0x00000010), c.r[S2]);
                c.wr64((c.r[SP] as u32).wrapping_add(0x00000018), c.r[S3]);
                c.wr64((c.r[SP] as u32).wrapping_add(0x00000020), c.r[S4]);
                c.wr64((c.r[SP] as u32).wrapping_add(0x00000028), c.r[S5]);
                c.wr64((c.r[SP] as u32).wrapping_add(0x00000038), c.r[RA]);
                c.r[A1] = c.rd16(c.r[S6] as u32) as i16 as i64 as u64;
                let t = c.r[A1] == 0u64;
                c.r[S0] = add32(c.r[S6], 0x00000010);
                if t {
                    l = 0x004a4c68;
                    continue;
                }
                c.r[A2] = 0x0000000070000000;
                c.r[A0] = c.r[S0];
                c.r[RA] = k.pc(0x004a4b44) as u64;
                c.r[A2] = c.r[A2] | 0x3f00;
                f4a5220(c, k.call(c, 0x004a4b3c, 0x4a_5220));
                c.r[RA] = k.pc(0x004a4b4c) as u64;
                c.r[S4] = 0x0000000070000000;
                f4a4c98(c, k.call(c, 0x004a4b44, 0x4a_4c98));
                c.r[S2] = c.r[S0];
                c.r[A0] = sx(k.hi(c, 0x004a4b50));
                c.r[A0] = sx(c.rd32((c.r[A0] as u32).wrapping_add(k.lo(c, 0x004a4b54))));
                c.r[V0] = 0x0000000030000000;
                c.r[V0] = c.r[V0] | 0x7;
                c.r[V1] = sx(k.hi(c, 0x004a4b60));
                c.wr32(c.r[A0] as u32, c.r[V0] as u32);
                c.r[V1] = add32(c.r[V1], k.lo(c, 0x004a4b68) as u64);
                c.r[A0] = 0x0000000050000000;
                c.r[S4] = c.r[S4] | 0x3f00;
                c.r[A1] = sx(k.hi(c, 0x004a4b74));
                c.r[A1] = sx(c.rd32((c.r[A1] as u32).wrapping_add(k.lo(c, 0x004a4b78))));
                c.r[A0] = c.r[A0] | 0x7;
                c.r[S5] = 0;
                c.wr32((c.r[A1] as u32).wrapping_add(0x00000004), c.r[V1] as u32);
                c.r[V0] = sx(k.hi(c, 0x004a4b88));
                c.r[V0] = sx(c.rd32((c.r[V0] as u32).wrapping_add(k.lo(c, 0x004a4b8c))));
                c.wr32((c.r[V0] as u32).wrapping_add(0x00000008), 0u64 as u32);
                c.r[V1] = sx(k.hi(c, 0x004a4b94));
                c.r[V1] = sx(c.rd32((c.r[V1] as u32).wrapping_add(k.lo(c, 0x004a4b98))));
                c.wr32((c.r[V1] as u32).wrapping_add(0x0000000c), c.r[A0] as u32);
                c.r[V0] = sx(k.hi(c, 0x004a4ba0));
                c.r[V0] = sx(c.rd32((c.r[V0] as u32).wrapping_add(k.lo(c, 0x004a4ba4))));
                c.r[V1] = c.rd16(c.r[S6] as u32) as i16 as i64 as u64;
                c.r[V0] = add32(c.r[V0], 0x00000010);
                let t = (c.r[V1] as i64) <= 0;
                c.wr32(k.gp(c, gp, 0x004a4bb4), c.r[V0] as u32);
                if t {
                    l = 0x004a4c2c;
                    continue;
                }
                c.r[S3] = c.r[S6];
                l = 0x004a4bc0;
                continue;
            }
            0x004a4bc0 => {
                n[1] += 1;
                c.r[V0] = c.rd8(c.r[S4] as u32) as u64;
                let t = c.r[V0] != 0u64;
                if t {
                    c.r[V0] = c.rd16(c.r[S6] as u32) as i16 as i64 as u64;
                    l = 0x004a4c14;
                    continue;
                }
                c.r[V1] = sx(c.rd32((c.r[S2] as u32).wrapping_add(0x00000010)));
                c.r[A1] = 0x0000000070000000;
                c.r[V0] = c.rd16((c.r[S2] as u32).wrapping_add(0x0000001c)) as i16 as i64 as u64;
                c.r[A0] = c.rd16((c.r[S2] as u32).wrapping_add(0x00000018)) as i16 as i64 as u64;
                c.r[A3] = c.rd16((c.r[S2] as u32).wrapping_add(0x0000001a)) as i16 as i64 as u64;
                c.r[S1] = add32(c.r[V1], c.r[V0]);
                c.r[A2] = c.rd16((c.r[S2] as u32).wrapping_add(0x00000014)) as i16 as i64 as u64;
                c.r[A0] = add32(c.r[V1], c.r[A0]);
                c.r[RA] = k.pc(0x004a4bf4) as u64;
                c.r[S0] = add32(c.r[V1], c.r[A3]);
                f4a4d40(c);
                let t = c.r[V0] != 0u64;
                if t {
                    c.r[V0] = c.rd16(c.r[S6] as u32) as i16 as i64 as u64;
                    l = 0x004a4c14;
                    continue;
                }
                c.r[A0] = c.rd16((c.r[S3] as u32).wrapping_add(0x00000026)) as i16 as i64 as u64;
                c.r[A1] = c.r[S1];
                c.r[A2] = c.r[S0];
                c.r[RA] = k.pc(0x004a4c10) as u64;
                c.r[A3] = 0x0000000070000000;
                n[2] += 1;
                f4a4e38(c, k.call(c, 0x004a4c08, 0x4a_4e38));
                c.r[V0] = c.rd16(c.r[S6] as u32) as i16 as i64 as u64;
                l = 0x004a4c14;
                continue;
            }
            0x004a4c14 => {
                c.r[S5] = add32(c.r[S5], 0x00000001);
                c.r[S3] = add32(c.r[S3], 0x00000020);
                c.r[S2] = add32(c.r[S2], 0x00000020);
                c.r[V0] = ((c.r[S5] as i64) < (c.r[V0] as i64)) as u64;
                let t = c.r[V0] != 0u64;
                c.r[S4] = add32(c.r[S4], 0x00000001);
                if t {
                    l = 0x004a4bc0;
                    continue;
                }
                l = 0x004a4c2c;
                continue;
            }
            0x004a4c2c => {
                c.r[A1] = 0x0000000000030000;
                c.r[A1] = c.r[A1] | 0x180b;
                c.r[RA] = k.pc(0x004a4c3c) as u64;
                c.r[A0] = sx(0x00000047);
                f4c1fe8(c, k.call(c, 0x004a4c34, 0x4c_1fe8));
                c.r[V0] = sx(k.hi(c, 0x004a4c3c));
                c.r[A2] = 0x0000000001000000;
                c.r[V0] = add32(c.r[V0], k.lo(c, 0x004a4c44) as u64);
                c.r[V1] = 0u64 | 0x8000;
                c.r[V1] = c.r[V1] << 17;
                c.r[A1] = sx(c.rd32((c.r[V0] as u32).wrapping_add(0x00000008)));
                c.r[A0] = sx(0x0000004e);
                c.r[A1] = sx(((c.r[A1] as u32 as i32) >> 13) as u32);
                c.r[A1] = c.r[A1] | c.r[A2];
                c.r[RA] = k.pc(0x004a4c68) as u64;
                c.r[A1] = c.r[A1] | c.r[V1];
                f4c1fe8(c, k.call(c, 0x004a4c60, 0x4c_1fe8));
                l = 0x004a4c68;
                continue;
            }
            0x004a4c68 => {
                c.r[S0] = c.rd64(c.r[SP] as u32);
                c.r[S1] = c.rd64((c.r[SP] as u32).wrapping_add(0x00000008));
                c.r[S2] = c.rd64((c.r[SP] as u32).wrapping_add(0x00000010));
                c.r[S3] = c.rd64((c.r[SP] as u32).wrapping_add(0x00000018));
                c.r[S4] = c.rd64((c.r[SP] as u32).wrapping_add(0x00000020));
                c.r[S5] = c.rd64((c.r[SP] as u32).wrapping_add(0x00000028));
                c.r[S6] = c.rd64((c.r[SP] as u32).wrapping_add(0x00000030));
                c.r[RA] = c.rd64((c.r[SP] as u32).wrapping_add(0x00000038));
                c.r[SP] = add32(c.r[SP], 0x00000040);
                return;
            }
            _ => unreachable!("0x4A4B00: no block at {l:08x}"),
        }
    }
}

fn f4a5220(c: &mut Cpu, k: F) {
    let mut l: u32 = 0x004a5220;
    loop {
        match l {
            0x004a5220 => {
                c.r[AT] = sx(k.hi(c, 0x004a5220));
                c.r[AT] = add32(c.r[AT], k.lo(c, 0x004a5224) as u64);
                lqc2(c, 24, (c.r[AT] as u32).wrapping_add(0x00000030));
                lqc2(c, 6, c.r[AT] as u32);
                lqc2(c, 7, (c.r[AT] as u32).wrapping_add(0x00000010));
                lqc2(c, 8, (c.r[AT] as u32).wrapping_add(0x00000020));
                lqc2(c, 20, (c.r[AT] as u32).wrapping_add(0x00000040));
                lqc2(c, 22, (c.r[AT] as u32).wrapping_add(0x00000060));
                c.r[AT] = sx(k.hi(c, 0x004a5240));
                c.r[AT] = add32(c.r[AT], k.lo(c, 0x004a5244) as u64);
                lqc2(c, 11, c.r[AT] as u32);
                lqc2(c, 12, (c.r[AT] as u32).wrapping_add(0x00000010));
                lqc2(c, 13, (c.r[AT] as u32).wrapping_add(0x00000020));
                lqc2(c, 14, (c.r[AT] as u32).wrapping_add(0x00000030));
                fm::<6, 14>(c, ACC, 6, b4(c.vf[11][0]));
                fm::<2, 14>(c, ACC, 7, b4(c.vf[11][1]));
                fm::<2, 14>(c, 25, 8, b4(c.vf[11][2]));
                fm::<6, 14>(c, ACC, 6, b4(c.vf[12][0]));
                fm::<2, 14>(c, ACC, 7, b4(c.vf[12][1]));
                fm::<2, 14>(c, 26, 8, b4(c.vf[12][2]));
                fm::<6, 14>(c, ACC, 6, b4(c.vf[13][0]));
                fm::<2, 14>(c, ACC, 7, b4(c.vf[13][1]));
                fm::<2, 14>(c, 27, 8, b4(c.vf[13][2]));
                fm::<6, 14>(c, ACC, 6, b4(c.vf[14][0]));
                fm::<2, 14>(c, ACC, 7, b4(c.vf[14][1]));
                fm::<2, 14>(c, 28, 8, b4(c.vf[14][2]));
                lqc2(c, 10, c.r[A0] as u32);
                c.r[A2] = add32(c.r[A2], 0xffffffff);
                fm::<6, 14>(c, ACC, 25, b4(c.vf[10][0]));
                fm::<2, 14>(c, ACC, 26, b4(c.vf[10][1]));
                fm::<2, 14>(c, ACC, 27, b4(c.vf[10][2]));
                fm::<2, 14>(c, 11, 28, b4(c.vf[0][3]));
                fm::<6, 12>(c, 12, 22, b4(c.vf[10][3]));
                fm::<6, 12>(c, 13, 20, b4(c.vf[11][2]));
                uycpu::vu0::abs::<12>(c, 14, 11);
                l = 0x004a52b0;
                continue;
            }
            0x004a52b0 => {
                lqc2(c, 10, (c.r[A0] as u32).wrapping_add(0x00000020));
                c.r[A0] = add32(c.r[A0], 0x00000020);
                fm::<1, 12>(c, 3, 14, c.vf[12]);
                fm::<6, 14>(c, ACC, 25, b4(c.vf[10][0]));
                fm::<2, 14>(c, ACC, 26, b4(c.vf[10][1]));
                fm::<2, 14>(c, ACC, 27, b4(c.vf[10][2]));
                fm::<2, 14>(c, 11, 28, b4(c.vf[0][3]));
                fm::<1, 12>(c, 3, 13, c.vf[3]);
                fm::<6, 12>(c, 12, 22, b4(c.vf[10][3]));
                fm::<6, 12>(c, 13, 20, b4(c.vf[11][2]));
                (c.r[AT], c.rh[AT]) = q4(c.vf[3]);
                c.r[A1] = add32(c.r[A1], 0xffffffff);
                uycpu::vu0::abs::<12>(c, 14, 11);
                c.r[V0] = c.r[AT] >> 63;
                c.r[AT] = srl(c.r[AT], 31);
                c.r[AT] = c.r[AT] | c.r[V0];
                c.r[A2] = add32(c.r[A2], 0x00000001);
                let t = c.r[A1] != 0u64;
                c.wr8(c.r[A2] as u32, c.r[AT] as u8);
                if t {
                    l = 0x004a52b0;
                    continue;
                }
                return;
            }
            _ => unreachable!("0x4A5220: no block at {l:08x}"),
        }
    }
}

fn f4a4c98(c: &mut Cpu, k: F) {
    let l: u32 = 0x004a4c98;
    loop {
        match l {
            0x004a4c98 => {
                c.r[AT] = sx(k.hi(c, 0x004a4c98));
                c.r[AT] = add32(c.r[AT], k.lo(c, 0x004a4c9c) as u64);
                c.r[V0] = sx(k.hi(c, 0x004a4ca0));
                c.r[V0] = add32(c.r[V0], k.lo(c, 0x004a4ca4) as u64);
                lqc2(c, 10, (c.r[AT] as u32).wrapping_add(0x00000100));
                lqc2(c, 11, (c.r[AT] as u32).wrapping_add(0x00000110));
                lqc2(c, 12, (c.r[AT] as u32).wrapping_add(0x00000120));
                lqc2(c, 13, (c.r[AT] as u32).wrapping_add(0x00000130));
                c.r[AT] = sx(k.hi(c, 0x004a4cb8));
                c.r[AT] = add32(c.r[AT], k.lo(c, 0x004a4cbc) as u64);
                lqc2(c, 20, c.r[AT] as u32);
                lqc2(c, 21, (c.r[AT] as u32).wrapping_add(0x00000010));
                lqc2(c, 22, (c.r[AT] as u32).wrapping_add(0x00000020));
                lqc2(c, 23, (c.r[AT] as u32).wrapping_add(0x00000030));
                fm::<6, 15>(c, ACC, 10, b4(c.vf[20][0]));
                fm::<2, 15>(c, ACC, 11, b4(c.vf[20][1]));
                fm::<2, 15>(c, ACC, 12, b4(c.vf[20][2]));
                fm::<2, 15>(c, 20, 13, b4(c.vf[20][3]));
                fm::<6, 15>(c, ACC, 10, b4(c.vf[21][0]));
                fm::<2, 15>(c, ACC, 11, b4(c.vf[21][1]));
                fm::<2, 15>(c, ACC, 12, b4(c.vf[21][2]));
                fm::<2, 15>(c, 21, 13, b4(c.vf[21][3]));
                fm::<6, 15>(c, ACC, 10, b4(c.vf[22][0]));
                fm::<2, 15>(c, ACC, 11, b4(c.vf[22][1]));
                fm::<2, 15>(c, ACC, 12, b4(c.vf[22][2]));
                fm::<2, 15>(c, 22, 13, b4(c.vf[22][3]));
                fm::<6, 15>(c, ACC, 10, b4(c.vf[23][0]));
                fm::<2, 15>(c, ACC, 11, b4(c.vf[23][1]));
                fm::<2, 15>(c, ACC, 12, b4(c.vf[23][2]));
                fm::<2, 15>(c, 23, 13, b4(c.vf[23][3]));
                lqc2(c, 1, (c.r[V0] as u32).wrapping_add(0x000001d0));
                lqc2(c, 26, (c.r[V0] as u32).wrapping_add(0x00000190));
                lqc2(c, 27, (c.r[V0] as u32).wrapping_add(0x000001a0));
                fm::<6, 12>(c, 26, 26, c.vf[1]);
                c.r[AT] = sx(k.hi(c, 0x004a4d20));
                c.r[AT] = add32(c.r[AT], k.lo(c, 0x004a4d24) as u64);
                lqc2(c, 28, c.r[AT] as u32);
                lqc2(c, 29, (c.r[AT] as u32).wrapping_add(0x00000010));
                lqc2(c, 30, (c.r[AT] as u32).wrapping_add(0x00000020));
                lqc2(c, 31, (c.r[AT] as u32).wrapping_add(0x00000030));
                c.vf[24] = [0u64 as u32, (0u64 >> 32) as u32, 0u64 as u32, (0u64 >> 32) as u32];
                return;
            }
            _ => unreachable!("0x4A4C98: no block at {l:08x}"),
        }
    }
}

fn f4a4d40(c: &mut Cpu) {
    let mut l: u32 = 0x004a4d40;
    loop {
        match l {
            0x004a4d40 => {
                c.r[V1] = c.rd64(c.r[A0] as u32);
                c.r[A1] = add32(c.r[A1], 0xfffffff0);
                (c.r[V0], c.rh[V0]) = mmi::pextlh((c.r[V1], c.rh[V1]), (0u64, 0u64));
                c.r[V1] = c.rd64((c.r[A0] as u32).wrapping_add(0x00000008));
                (c.r[V0], c.rh[V0]) = mmi::psraw((c.r[V0], c.rh[V0]), 16);
                qmtc2(c, 10, (c.r[V0], c.rh[V0]));
                c.r[A0] = add32(c.r[A0], 0x00000008);
                (c.r[AT], c.rh[AT]) = mmi::pcpyud((c.r[V0], c.rh[V0]), (0u64, 0u64));
                uycpu::vu0::itof::<0, 14>(c, 10, 10);
                c.r[T6] = sx(0x0000002f);
                fm::<6, 15>(c, ACC, 20, b4(c.vf[10][0]));
                fm::<2, 15>(c, ACC, 21, b4(c.vf[10][1]));
                fm::<2, 15>(c, ACC, 22, b4(c.vf[10][2]));
                fm::<2, 15>(c, 11, 23, b4(c.vf[0][3]));
                uycpu::vu0::qop::<0>(c, c.vf[0][3], c.vf[11][3]);
                uycpu::vu0::clip(c, 11, 11);
                l = 0x004a4d90;
                continue;
            }
            0x004a4d90 => {
                (c.r[V0], c.rh[V0]) = mmi::pextlh((c.r[V1], c.rh[V1]), (0u64, 0u64));
                c.r[V1] = c.rd64((c.r[A0] as u32).wrapping_add(0x00000008));
                (c.r[V0], c.rh[V0]) = mmi::psraw((c.r[V0], c.rh[V0]), 16);
                qmtc2(c, 10, (c.r[V0], c.rh[V0]));
                c.r[A0] = add32(c.r[A0], 0x00000008);
                fm::<6, 14>(c, 12, 11, b4(c.vi[22]));
                c.r[A2] = add32(c.r[A2], 0xffffffff);
                uycpu::vu0::itof::<0, 14>(c, 10, 10);
                c.r[A1] = add32(c.r[A1], 0x00000010);
                c.r[A3] = sx(c.vi[18]);
                fm::<6, 15>(c, ACC, 12, c.vf[26]);
                c.r[A3] = c.r[A3] & 0x2f;
                fm::<2, 15>(c, 12, 27, b4(c.vf[0][3]));
                c.r[T6] = c.r[T6] & c.r[A3];
                fm::<6, 15>(c, ACC, 20, b4(c.vf[10][0]));
                c.r[A3] = sll(c.r[A3], 24);
                fm::<2, 15>(c, ACC, 21, b4(c.vf[10][1]));
                c.r[AT] = c.r[AT] >> 32;
                fm::<2, 15>(c, ACC, 22, b4(c.vf[10][2]));
                c.r[T0] = c.r[A3] | c.r[AT];
                fm::<2, 15>(c, 11, 23, b4(c.vf[0][3]));
                uycpu::vu0::ftoi::<1, 14>(c, 12, 12);
                (c.r[AT], c.rh[AT]) = mmi::pcpyud((c.r[V0], c.rh[V0]), (0u64, 0u64));
                uycpu::vu0::qop::<0>(c, c.vf[0][3], c.vf[11][3]);
                uycpu::vu0::clip(c, 11, 11);
                sqc2(c, 12, c.r[A1] as u32);
                let t = (c.r[A2] as i64) > 0;
                c.wr32((c.r[A1] as u32).wrapping_add(0x0000000c), c.r[T0] as u32);
                if t {
                    l = 0x004a4d90;
                    continue;
                }
                c.r[V0] = c.r[T6];
                return;
            }
            _ => unreachable!("0x4A4D40: no block at {l:08x}"),
        }
    }
}

fn f4a4f38(c: &mut Cpu, k: F, n: &mut [u64; N]) {
    let gp = c.r[GP] as u32;
    let mut l: u32 = 0x004a4f38;
    loop {
        match l {
            0x004a4f38 => {
                c.r[T8] = sx(c.rd32(k.gp(c, gp, 0x004a4f38)));
                (c.r[S0], c.rh[S0]) = mmi::pcpyld((c.r[S0], c.rh[S0]), (0u64, 0u64));
                c.r[T1] = sx(c.rd32(c.r[A1] as u32));
                (c.r[T5], c.rh[T5]) = mmi::pcpyld((0u64, 0u64), (0u64, 0u64));
                sqc2(c, 28, c.r[T8] as u32);
                (c.r[T6], c.rh[T6]) = mmi::pcpyld((0u64, 0u64), (0u64, 0u64));
                sqc2(c, 30, (c.r[T8] as u32).wrapping_add(0x00000010));
                (c.r[T7], c.rh[T7]) = mmi::pcpyld((0u64, 0u64), (0u64, 0u64));
                (c.r[AT], c.rh[AT]) = q4(c.vf[31]);
                c.r[T9] = sx(0x00000000);
                qmtc2(c, 25, (c.r[T8], c.rh[T8]));
                c.r[T8] = add32(c.r[T8], 0x00000020);
                c.r[S0] = c.r[AT];
                l = 0x004a4f70;
                continue;
            }
            0x004a4f70 => {
                let t = (c.r[A0] as i64) <= 0;
                c.r[T2] = sll(c.r[T1], 4);
                if t {
                    l = 0x004a50a8;
                    continue;
                }
                c.r[T3] = srl(c.r[T1], 4);
                c.r[T4] = srl(c.r[T1], 12);
                c.r[T2] = c.r[T2] & 0xff0;
                c.r[T3] = c.r[T3] & 0xff0;
                c.r[T4] = c.r[T4] & 0xff0;
                c.r[T5] = add32(c.r[T2], c.r[A3]);
                c.r[AT] = c.rd8((c.r[T5] as u32).wrapping_add(0x0000000f)) as u64;
                c.r[T6] = add32(c.r[T3], c.r[A3]);
                c.r[V0] = c.rd8((c.r[T6] as u32).wrapping_add(0x0000000f)) as u64;
                c.r[T7] = add32(c.r[T4], c.r[A3]);
                c.r[V1] = c.rd8((c.r[T7] as u32).wrapping_add(0x0000000f)) as u64;
                c.r[A0] = add32(c.r[A0], 0xffffffff);
                c.r[V0] = c.r[V0] & c.r[AT];
                c.r[AT] = srl(c.r[T1], 24);
                c.r[V0] = c.r[V0] & c.r[V1];
                c.r[T1] = sx(c.rd32((c.r[A1] as u32).wrapping_add(0x00000004)));
                let t = (c.r[V0] as i64) > 0;
                c.r[A1] = add32(c.r[A1], 0x00000004);
                if t {
                    l = 0x004a4f70;
                    continue;
                }
                (c.r[V0], c.rh[V0]) = q4(c.vf[24]);
                let t = c.r[AT] != c.r[T0];
                c.r[T0] = c.r[AT];
                if t {
                    l = 0x004a50e0;
                    continue;
                }
                l = 0x004a4fd0;
                continue;
            }
            0x004a4fd0 => {
                c.wr64(c.r[T8] as u32, c.r[V0]);
                c.r[T9] = add32(c.r[T9], 0x00000001);
                c.r[T2] = srl(c.r[T2], 2);
                c.r[AT] = c.rd8((c.r[T5] as u32).wrapping_add(0x0000000c)) as u64;
                c.r[T3] = srl(c.r[T3], 2);
                c.r[V0] = c.rd8((c.r[T6] as u32).wrapping_add(0x0000000c)) as u64;
                c.r[T4] = srl(c.r[T4], 2);
                c.r[V1] = c.rd8((c.r[T7] as u32).wrapping_add(0x0000000c)) as u64;
                c.r[T2] = add32(c.r[T2], c.r[A2]);
                c.r[T5] = c.rd64(c.r[T5] as u32);
                c.r[T3] = add32(c.r[T3], c.r[A2]);
                c.r[T6] = c.rd64(c.r[T6] as u32);
                c.r[T4] = add32(c.r[T4], c.r[A2]);
                c.r[T7] = c.rd64(c.r[T7] as u32);
                c.r[T2] = c.rd32(c.r[T2] as u32) as u64;
                (c.r[T5], c.rh[T5]) = mmi::ppach((0u64, 0u64), (c.r[T5], c.rh[T5]));
                c.r[T3] = c.rd32(c.r[T3] as u32) as u64;
                (c.r[T6], c.rh[T6]) = mmi::ppach((0u64, 0u64), (c.r[T6], c.rh[T6]));
                c.r[T4] = c.rd32(c.r[T4] as u32) as u64;
                (c.r[T7], c.rh[T7]) = mmi::ppach((0u64, 0u64), (c.r[T7], c.rh[T7]));
                (c.r[T2], c.rh[T2]) = mmi::pextlh((0u64, 0u64), (c.r[T2], c.rh[T2]));
                c.wr64((c.r[T8] as u32).wrapping_add(0x00000018), c.r[T5]);
                (c.r[T3], c.rh[T3]) = mmi::pextlh((0u64, 0u64), (c.r[T3], c.rh[T3]));
                c.wr64((c.r[T8] as u32).wrapping_add(0x00000030), c.r[T6]);
                (c.r[T4], c.rh[T4]) = mmi::pextlh((0u64, 0u64), (c.r[T4], c.rh[T4]));
                c.wr64((c.r[T8] as u32).wrapping_add(0x00000048), c.r[T7]);
                qmtc2(c, 1, (c.r[T2], c.rh[T2]));
                c.r[AT] = c.r[AT] << 24;
                qmtc2(c, 2, (c.r[T3], c.rh[T3]));
                c.r[V0] = c.r[V0] << 24;
                qmtc2(c, 3, (c.r[T4], c.rh[T4]));
                c.r[V1] = c.r[V1] << 24;
                uycpu::vu0::itof::<2, 12>(c, 1, 1);
                c.r[AT] = c.r[AT] | c.r[S0];
                uycpu::vu0::itof::<2, 12>(c, 2, 2);
                c.wr64((c.r[T8] as u32).wrapping_add(0x00000008), c.r[AT]);
                uycpu::vu0::itof::<2, 12>(c, 3, 3);
                c.r[V0] = c.r[V0] | c.r[S0];
                c.r[V1] = c.r[V1] | c.r[S0];
                c.wr64((c.r[T8] as u32).wrapping_add(0x00000020), c.r[V0]);
                c.wr64((c.r[T8] as u32).wrapping_add(0x00000038), c.r[V1]);
                (c.r[T2], c.rh[T2]) = q4(c.vf[1]);
                (c.r[T3], c.rh[T3]) = q4(c.vf[2]);
                (c.r[T4], c.rh[T4]) = q4(c.vf[3]);
                c.wr64((c.r[T8] as u32).wrapping_add(0x00000010), c.r[T2]);
                c.wr64((c.r[T8] as u32).wrapping_add(0x00000028), c.r[T3]);
                c.r[T8] = add32(c.r[T8], 0x00000050);
                let t = (c.r[A0] as i64) > 0;
                c.wr64((c.r[T8] as u32).wrapping_add(0xfffffff0), c.r[T4]);
                if t {
                    l = 0x004a4f70;
                    continue;
                }
                l = 0x004a50a8;
                continue;
            }
            0x004a50a8 => {
                let t = c.r[T9] == 0u64;
                (c.r[AT], c.rh[AT]) = q4(c.vf[25]);
                if t {
                    l = 0x004a50d0;
                    continue;
                }
                c.r[T9] = c.r[T9] | 0x8000;
                c.r[V1] = sub32(c.r[T8], c.r[AT]);
                c.r[V1] = srl(c.r[V1], 4);
                c.wr32(k.gp(c, gp, 0x004a50bc), c.r[T8] as u32);
                c.wr32((c.r[AT] as u32).wrapping_add(0x00000010), c.r[T9] as u32);
                c.r[V1] = add32(c.r[V1], 0xffffffff);
                c.wr16(c.r[AT] as u32, c.r[V1] as u16);
                c.wr16((c.r[AT] as u32).wrapping_add(0x0000000c), c.r[V1] as u16);
                l = 0x004a50d0;
                continue;
            }
            0x004a50d0 => {
                (c.r[S0], c.rh[S0]) = mmi::pcpyud((c.r[S0], c.rh[S0]), (0u64, 0u64));
                c.r[V0] = c.r[T0];
                return;
            }
            0x004a50e0 => {
                c.r[V1] = sx(c.rd32(k.gp(c, gp, 0x004a50e0)));
                c.r[AT] = sll(c.r[AT], 4);
                c.r[V0] = sx(c.rd32((c.r[V1] as u32).wrapping_add(0x00000010)));
                c.r[AT] = add32(c.r[V0], c.r[AT]);
                c.r[V0] = c.rd64(c.r[AT] as u32);
                (c.r[T6], c.rh[T6]) = mmi::pcpyld((c.r[T6], c.rh[T6]), (c.r[T6], c.rh[T6]));
                let t = c.r[V0] != 0u64;
                qmtc2(c, 24, (c.r[V0], c.rh[V0]));
                if t {
                    l = 0x004a4fd0;
                    continue;
                }
                n[3] += 1;
                (c.r[T2], c.rh[T2]) = mmi::pcpyld((c.r[T2], c.rh[T2]), (c.r[T2], c.rh[T2]));
                c.r[T2] = sx(c.rd32(k.gp(c, gp, 0x004a5108)));
                (c.r[T3], c.rh[T3]) = mmi::pcpyld((c.r[T3], c.rh[T3]), (c.r[T3], c.rh[T3]));
                c.r[T3] = c.rd16((c.r[AT] as u32).wrapping_add(0x0000000e)) as i16 as i64 as u64;
                (c.r[T4], c.rh[T4]) = mmi::pcpyld((c.r[T4], c.rh[T4]), (c.r[T4], c.rh[T4]));
                c.r[T4] = c.rd16((c.r[AT] as u32).wrapping_add(0x0000000c)) as i16 as i64 as u64;
                (c.r[T5], c.rh[T5]) = mmi::pcpyld((c.r[T5], c.rh[T5]), (c.r[T5], c.rh[T5]));
                c.r[T5] = srl(c.r[T2], 8);
                c.r[T6] = sx(0x00000001);
                c.r[T2] = add32(c.r[T2], 0x00000400);
                c.r[T3] = add32(c.r[T4], c.r[T3]);
                c.r[T3] = sx((c.r[T6] as u32).wrapping_shl(c.r[T3] as u32 & 31));
                c.r[T6] = srl(c.r[T2], 8);
                c.r[T2] = add32(c.r[T2], c.r[T3]);
                c.r[V0] = add32(c.r[T4], 0xfffffffa);
                (c.r[V0], c.rh[V0]) = mmi::pmaxw((c.r[V0], c.rh[V0]), (0u64, 0u64));
                c.wr32(k.gp(c, gp, 0x004a5144), c.r[T2] as u32);
                c.r[T2] = sx(0x00000001);
                c.r[T4] = sx((c.r[T2] as u32).wrapping_shl(c.r[V0] as u32 & 31));
                c.r[T4] = c.r[T4] << 14;
                c.r[T2] = c.rd16((c.r[AT] as u32).wrapping_add(0x0000000c)) as i16 as i64 as u64;
                c.r[V0] = c.r[T6].wrapping_add(c.r[T4]);
                c.r[T3] = c.rd16((c.r[AT] as u32).wrapping_add(0x0000000e)) as i16 as i64 as u64;
                c.r[T4] = sx(0x00000013);
                c.r[T2] = c.r[T2] << 26;
                c.r[T3] = c.r[T3] << 30;
                c.r[T4] = c.r[T4] << 20;
                c.r[V0] = c.r[V0].wrapping_add(c.r[T2]);
                c.r[T2] = sx(0x00000001);
                c.r[V0] = c.r[V0].wrapping_add(c.r[T3]);
                c.r[T2] = c.r[T2] << 34;
                c.r[V0] = c.r[V0].wrapping_add(c.r[T4]);
                c.r[T3] = c.r[T5] << 37;
                c.r[V0] = c.r[V0].wrapping_add(c.r[T2]);
                c.r[T2] = sx(0x00000004);
                c.r[V0] = c.r[V0].wrapping_add(c.r[T3]);
                c.r[T2] = c.r[T2] << 61;
                c.r[V0] = c.r[V0].wrapping_add(c.r[T2]);
                c.wr64(c.r[AT] as u32, c.r[V0]);
                qmtc2(c, 24, (c.r[V0], c.rh[V0]));
                c.r[T3] = sx(c.rd32(k.gp(c, gp, 0x004a51a8)));
                c.r[T2] = sx(k.hi(c, 0x004a51ac));
                c.r[T2] = add32(c.r[T2], k.lo(c, 0x004a51b0) as u64);
                c.r[V1] = sx(c.rd32((c.r[V1] as u32).wrapping_add(0x00000014)));
                c.r[T4] = sll(c.r[T3], 4);
                c.r[T3] = add32(c.r[T3], 0x00000001);
                c.wr32(k.gp(c, gp, 0x004a51c0), c.r[T3] as u32);
                c.r[T4] = add32(c.r[T4], c.r[T2]);
                c.r[T2] = c.rd16((c.r[AT] as u32).wrapping_add(0x0000000a)) as i16 as i64 as u64;
                c.r[T3] = c.rd16((c.r[AT] as u32).wrapping_add(0x00000008)) as i16 as i64 as u64;
                c.wr16((c.r[T4] as u32).wrapping_add(0x00000006), c.r[T5] as u16);
                c.r[T2] = sll(c.r[T2], 4);
                c.wr16((c.r[T4] as u32).wrapping_add(0x0000000e), c.r[T6] as u16);
                c.r[T2] = add32(c.r[T2], c.r[V1]);
                c.wr32(c.r[T4] as u32, c.r[T2] as u32);
                c.r[T3] = sll(c.r[T3], 4);
                c.wr16((c.r[T4] as u32).wrapping_add(0x00000004), 0u64 as u16);
                c.r[T3] = add32(c.r[T3], c.r[V1]);
                c.wr32((c.r[T4] as u32).wrapping_add(0x00000008), c.r[T3] as u32);
                c.r[T2] = c.rd8((c.r[AT] as u32).wrapping_add(0x0000000c)) as u64;
                (c.r[T6], c.rh[T6]) = mmi::pcpyud((c.r[T6], c.rh[T6]), (0u64, 0u64));
                c.r[T3] = c.rd8((c.r[AT] as u32).wrapping_add(0x0000000e)) as u64;
                (c.r[T5], c.rh[T5]) = mmi::pcpyud((c.r[T5], c.rh[T5]), (0u64, 0u64));
                c.wr8((c.r[T4] as u32).wrapping_add(0x0000000c), c.r[T2] as u8);
                (c.r[T2], c.rh[T2]) = mmi::pcpyud((c.r[T2], c.rh[T2]), (0u64, 0u64));
                c.wr8((c.r[T4] as u32).wrapping_add(0x0000000d), c.r[T3] as u8);
                (c.r[T3], c.rh[T3]) = mmi::pcpyud((c.r[T3], c.rh[T3]), (0u64, 0u64));
                (c.r[T4], c.rh[T4]) = mmi::pcpyud((c.r[T4], c.rh[T4]), (0u64, 0u64));
                l = 0x004a4fd0;
                continue;
            }
            _ => unreachable!("0x4A4F38: no block at {l:08x}"),
        }
    }
}

fn f4a4e38(c: &mut Cpu, k: F) {
    let gp = c.r[GP] as u32;
    let mut l: u32 = 0x004a4e38;
    loop {
        match l {
            0x004a4e38 => {
                c.r[T9] = sx(c.rd32(k.gp(c, gp, 0x004a4e38)));
                c.r[T0] = sx(c.rd32(c.r[A1] as u32));
                (c.r[T5], c.rh[T5]) = mmi::pcpyld((0u64, 0u64), (0u64, 0u64));
                sqc2(c, 28, c.r[T9] as u32);
                (c.r[T6], c.rh[T6]) = mmi::pcpyld((0u64, 0u64), (0u64, 0u64));
                sqc2(c, 29, (c.r[T9] as u32).wrapping_add(0x00000010));
                (c.r[T7], c.rh[T7]) = mmi::pcpyld((0u64, 0u64), (0u64, 0u64));
                c.r[T1] = sx(0x00000000);
                c.r[T8] = add32(c.r[T9], 0x00000020);
                l = 0x004a4e60;
                continue;
            }
            0x004a4e60 => {
                let t = (c.r[A0] as i64) <= 0;
                c.r[T2] = sll(c.r[T0], 4);
                if t {
                    l = 0x004a4f10;
                    continue;
                }
                c.r[T3] = srl(c.r[T0], 4);
                c.r[T4] = srl(c.r[T0], 12);
                c.r[T2] = c.r[T2] & 0xff0;
                c.r[T3] = c.r[T3] & 0xff0;
                c.r[T4] = c.r[T4] & 0xff0;
                c.r[T5] = add32(c.r[T2], c.r[A3]);
                c.r[AT] = c.rd8((c.r[T5] as u32).wrapping_add(0x0000000f)) as u64;
                c.r[T6] = add32(c.r[T3], c.r[A3]);
                c.r[V0] = c.rd8((c.r[T6] as u32).wrapping_add(0x0000000f)) as u64;
                c.r[T7] = add32(c.r[T4], c.r[A3]);
                c.r[V1] = c.rd8((c.r[T7] as u32).wrapping_add(0x0000000f)) as u64;
                c.r[A0] = add32(c.r[A0], 0xffffffff);
                c.r[AT] = c.r[AT] & c.r[V0];
                c.r[A1] = add32(c.r[A1], 0x00000004);
                c.r[AT] = c.r[AT] & c.r[V1];
                c.r[T0] = sx(c.rd32(c.r[A1] as u32));
                let t = (c.r[AT] as i64) > 0;
                c.r[T2] = srl(c.r[T2], 2);
                if t {
                    l = 0x004a4e60;
                    continue;
                }
                c.r[T3] = srl(c.r[T3], 2);
                c.r[T4] = srl(c.r[T4], 2);
                c.r[T2] = add32(c.r[T2], c.r[A2]);
                c.r[T2] = c.rd32(c.r[T2] as u32) as u64;
                c.r[T3] = add32(c.r[T3], c.r[A2]);
                c.r[T3] = c.rd32(c.r[T3] as u32) as u64;
                c.r[T4] = add32(c.r[T4], c.r[A2]);
                c.r[T4] = c.rd32(c.r[T4] as u32) as u64;
                c.r[T5] = c.rd64(c.r[T5] as u32);
                c.r[T6] = c.rd64(c.r[T6] as u32);
                c.r[T7] = c.rd64(c.r[T7] as u32);
                c.wr64(c.r[T8] as u32, c.r[T2]);
                (c.r[T5], c.rh[T5]) = mmi::ppach((0u64, 0u64), (c.r[T5], c.rh[T5]));
                c.wr64((c.r[T8] as u32).wrapping_add(0x00000010), c.r[T3]);
                (c.r[T6], c.rh[T6]) = mmi::ppach((0u64, 0u64), (c.r[T6], c.rh[T6]));
                c.wr64((c.r[T8] as u32).wrapping_add(0x00000020), c.r[T4]);
                (c.r[T7], c.rh[T7]) = mmi::ppach((0u64, 0u64), (c.r[T7], c.rh[T7]));
                c.wr64((c.r[T8] as u32).wrapping_add(0x00000008), c.r[T5]);
                c.wr64((c.r[T8] as u32).wrapping_add(0x00000018), c.r[T6]);
                c.wr64((c.r[T8] as u32).wrapping_add(0x00000028), c.r[T7]);
                c.r[T8] = add32(c.r[T8], 0x00000030);
                let t = (c.r[A0] as i64) > 0;
                c.r[T1] = add32(c.r[T1], 0x00000003);
                if t {
                    l = 0x004a4e60;
                    continue;
                }
                l = 0x004a4f10;
                continue;
            }
            0x004a4f10 => {
                let t = c.r[T1] == 0u64;
                c.r[AT] = add32(c.r[T1], 0x00000001);
                if t {
                    l = 0x004a4f30;
                    continue;
                }
                c.wr16(c.r[T9] as u32, c.r[AT] as u16);
                c.wr16((c.r[T9] as u32).wrapping_add(0x0000000c), c.r[AT] as u16);
                c.r[T1] = c.r[T1] | 0x8000;
                c.wr32((c.r[T9] as u32).wrapping_add(0x00000010), c.r[T1] as u32);
                c.wr32(k.gp(c, gp, 0x004a4f28), c.r[T8] as u32);
                l = 0x004a4f30;
                continue;
            }
            0x004a4f30 => {
                return;
            }
            _ => unreachable!("0x4A4E38: no block at {l:08x}"),
        }
    }
}

fn f4c1fe8(c: &mut Cpu, k: F) {
    let gp = c.r[GP] as u32;
    let mut l: u32 = 0x004c1fe8;
    loop {
        match l {
            0x004c1fe8 => {
                c.r[V1] = sx(k.hi(c, 0x004c1fe8));
                c.r[V1] = sx(c.rd32((c.r[V1] as u32).wrapping_add(k.lo(c, 0x004c1fec))));
                c.r[V0] = 0x0000000010000000;
                c.r[V0] = c.r[V0] | 0x2;
                c.r[A2] = 0x0000000050000000;
                c.wr32(c.r[V1] as u32, c.r[V0] as u32);
                l = 0x004c2000;
                continue;
            }
            0x004c2000 => {
                c.r[A2] = c.r[A2] | 0x2;
                c.r[A3] = 0u64 | 0x8001;
                c.r[T0] = 0x0000000010000000;
                c.r[V1] = sx(k.hi(c, 0x004c200c));
                c.r[V1] = sx(c.rd32((c.r[V1] as u32).wrapping_add(k.lo(c, 0x004c2010))));
                c.r[T1] = sx(0x0000000e);
                c.wr32((c.r[V1] as u32).wrapping_add(0x00000004), 0u64 as u32);
                c.r[V0] = sx(k.hi(c, 0x004c201c));
                c.r[V0] = sx(c.rd32((c.r[V0] as u32).wrapping_add(k.lo(c, 0x004c2020))));
                l = 0x004c2024;
                continue;
            }
            0x004c2024 => {
                c.wr32((c.r[V0] as u32).wrapping_add(0x00000008), 0u64 as u32);
                c.r[V1] = sx(k.hi(c, 0x004c2028));
                c.r[V1] = sx(c.rd32((c.r[V1] as u32).wrapping_add(k.lo(c, 0x004c202c))));
                c.wr32((c.r[V1] as u32).wrapping_add(0x0000000c), c.r[A2] as u32);
                c.r[V0] = sx(k.hi(c, 0x004c2034));
                c.r[V0] = sx(c.rd32((c.r[V0] as u32).wrapping_add(k.lo(c, 0x004c2038))));
                c.wr32((c.r[V0] as u32).wrapping_add(0x00000010), c.r[A3] as u32);
                c.r[V1] = sx(k.hi(c, 0x004c2040));
                c.r[V1] = sx(c.rd32((c.r[V1] as u32).wrapping_add(k.lo(c, 0x004c2044))));
                c.wr32((c.r[V1] as u32).wrapping_add(0x00000014), c.r[T0] as u32);
                c.r[V0] = sx(k.hi(c, 0x004c204c));
                c.r[V0] = sx(c.rd32((c.r[V0] as u32).wrapping_add(k.lo(c, 0x004c2050))));
                c.wr32((c.r[V0] as u32).wrapping_add(0x00000018), c.r[T1] as u32);
                c.r[V1] = sx(k.hi(c, 0x004c2058));
                c.r[V1] = sx(c.rd32((c.r[V1] as u32).wrapping_add(k.lo(c, 0x004c205c))));
                c.wr32((c.r[V1] as u32).wrapping_add(0x0000001c), 0u64 as u32);
                c.r[V0] = sx(k.hi(c, 0x004c2064));
                c.r[V0] = sx(c.rd32((c.r[V0] as u32).wrapping_add(k.lo(c, 0x004c2068))));
                c.wr64((c.r[V0] as u32).wrapping_add(0x00000020), c.r[A1]);
                c.r[V1] = sx(k.hi(c, 0x004c2070));
                c.r[V1] = sx(c.rd32((c.r[V1] as u32).wrapping_add(k.lo(c, 0x004c2074))));
                c.wr32((c.r[V1] as u32).wrapping_add(0x00000028), c.r[A0] as u32);
                c.r[A0] = sx(k.hi(c, 0x004c207c));
                c.r[A0] = sx(c.rd32((c.r[A0] as u32).wrapping_add(k.lo(c, 0x004c2080))));
                c.wr32((c.r[A0] as u32).wrapping_add(0x0000002c), 0u64 as u32);
                c.r[V0] = sx(k.hi(c, 0x004c2088));
                c.r[V0] = sx(c.rd32((c.r[V0] as u32).wrapping_add(k.lo(c, 0x004c208c))));
                c.r[V0] = add32(c.r[V0], 0x00000030);
                c.wr32(k.gp(c, gp, 0x004c2098), c.r[V0] as u32);
                return;
            }
            _ => unreachable!("0x4C1FE8: no block at {l:08x}"),
        }
    }
}

// ---- the sprites: 0x405A58 and everything under it ---------------------------------------------------------------------------
//
// Hand-written (0x405A58, 0x406018, 0x406098, 0x4060D8, 0x4057C0, 0x405938, 0x4059F8; `at` is 0x406098's packet pointer): each
// its translation's statements in order on the thread's registers, the blocks by level 1's labels. 0x4060D8's `jr` into its
// unrolled runs (0x406208, 0x406240, 0x4062AC, 0x4062E4, 0x40634C) goes to level 1's label of the instruction its target
// register holds, an address in this program's code; a target that is not one of its labels stops the game, as the
// translation's call of it would.

fn f405a58(c: &mut Cpu, k: F, n: &mut [u64; N]) {
    let gp = c.r[GP] as u32;
    let mut l: u32 = 0x00405a58;
    loop {
        match l {
            0x00405a58 => {
                n[4] += 1;
                lqc2(c, 10, c.r[A0] as u32);
                (c.r[S0], c.rh[S0]) = mmi::pcpyld((c.r[S0], c.rh[S0]), (0u64, 0u64));
                lqc2(c, 11, (c.r[A0] as u32).wrapping_add(0x00000010));
                c.r[T8] = sx(k.hi(c, 0x00405a64));
                c.r[T8] = add32(c.r[T8], k.lo(c, 0x00405a68) as u64);
                lqc2(c, 12, (c.r[A0] as u32).wrapping_add(0x00000020));
                lqc2(c, 13, (c.r[A0] as u32).wrapping_add(0x00000030));
                c.r[S0] = sx(k.hi(c, 0x00405a74));
                c.r[S0] = add32(c.r[S0], k.lo(c, 0x00405a78) as u64);
                lqc2(c, 25, (c.r[T8] as u32).wrapping_add(0x00000140));
                let t = 0u64 == c.r[A1];
                lqc2(c, 30, k.gp(c, gp, 0x00405a84));
                if t {
                    l = 0x00405ad8;
                    continue;
                }
                lqc2(c, 1, c.r[A1] as u32);
                lqc2(c, 2, (c.r[A1] as u32).wrapping_add(0x00000010));
                lqc2(c, 3, (c.r[A1] as u32).wrapping_add(0x00000020));
                lqc2(c, 4, (c.r[A1] as u32).wrapping_add(0x00000030));
                fm::<6, 15>(c, ACC, 1, b4(c.vf[10][0]));
                fm::<2, 15>(c, ACC, 2, b4(c.vf[10][1]));
                fm::<2, 15>(c, ACC, 3, b4(c.vf[10][2]));
                fm::<2, 15>(c, 10, 4, b4(c.vf[10][3]));
                fm::<6, 15>(c, ACC, 1, b4(c.vf[11][0]));
                fm::<2, 15>(c, ACC, 2, b4(c.vf[11][1]));
                fm::<2, 15>(c, ACC, 3, b4(c.vf[11][2]));
                fm::<2, 15>(c, 11, 4, b4(c.vf[11][3]));
                fm::<6, 15>(c, ACC, 1, b4(c.vf[12][0]));
                fm::<2, 15>(c, ACC, 2, b4(c.vf[12][1]));
                fm::<2, 15>(c, ACC, 3, b4(c.vf[12][2]));
                fm::<2, 15>(c, 12, 4, b4(c.vf[12][3]));
                fm::<6, 15>(c, ACC, 1, b4(c.vf[13][0]));
                fm::<2, 15>(c, ACC, 2, b4(c.vf[13][1]));
                fm::<2, 15>(c, ACC, 3, b4(c.vf[13][2]));
                fm::<2, 15>(c, 13, 4, b4(c.vf[13][3]));
                l = 0x00405ad8;
                continue;
            }
            0x00405ad8 => {
                fm::<1, 14>(c, 10, 10, c.vf[25]);
                fm::<1, 14>(c, 11, 11, c.vf[25]);
                fm::<1, 14>(c, 12, 12, c.vf[25]);
                fm::<1, 14>(c, 13, 13, c.vf[25]);
                fm::<6, 14>(c, 10, 10, b4(c.vf[30][2]));
                fm::<6, 14>(c, 11, 11, b4(c.vf[30][2]));
                fm::<6, 14>(c, 12, 12, b4(c.vf[30][2]));
                fm::<6, 14>(c, 13, 13, b4(c.vf[30][2]));
                lqc2(c, 1, (c.r[T8] as u32).wrapping_add(0x00000040));
                (c.r[S1], c.rh[S1]) = mmi::pcpyld((c.r[S1], c.rh[S1]), (0u64, 0u64));
                lqc2(c, 2, (c.r[T8] as u32).wrapping_add(0x00000050));
                (c.r[S2], c.rh[S2]) = mmi::pcpyld((c.r[S2], c.rh[S2]), (c.r[A0], c.rh[A0]));
                lqc2(c, 3, (c.r[T8] as u32).wrapping_add(0x00000060));
                (c.r[S3], c.rh[S3]) = mmi::pcpyld((c.r[S3], c.rh[S3]), (0u64, 0u64));
                lqc2(c, 4, (c.r[T8] as u32).wrapping_add(0x00000070));
                (c.r[S7], c.rh[S7]) = mmi::pcpyld((c.r[RA], c.rh[RA]), (c.r[S7], c.rh[S7]));
                lqc2(c, 26, (c.r[S0] as u32).wrapping_add(0x00000180));
                fm::<6, 15>(c, ACC, 1, b4(c.vf[13][0]));
                fm::<2, 15>(c, ACC, 2, b4(c.vf[13][1]));
                fm::<2, 15>(c, ACC, 3, b4(c.vf[13][2]));
                fm::<2, 15>(c, 13, 4, b4(c.vf[0][3]));
                fm::<6, 12>(c, 19, 26, b4(c.vf[30][1]));
                uycpu::vu0::mov::<3>(c, 19, c.vf[26]);
                fm::<6, 15>(c, ACC, 1, b4(c.vf[12][0]));
                fm::<2, 15>(c, ACC, 2, b4(c.vf[12][1]));
                fm::<2, 15>(c, ACC, 3, b4(c.vf[12][2]));
                fm::<2, 15>(c, 12, 4, b4(c.vf[0][3]));
                fm::<6, 15>(c, ACC, 1, b4(c.vf[11][0]));
                fm::<2, 15>(c, ACC, 2, b4(c.vf[11][1]));
                fm::<2, 15>(c, ACC, 3, b4(c.vf[11][2]));
                fm::<2, 15>(c, 11, 4, b4(c.vf[0][3]));
                fm::<6, 15>(c, ACC, 1, b4(c.vf[10][0]));
                fm::<2, 15>(c, ACC, 2, b4(c.vf[10][1]));
                fm::<2, 15>(c, ACC, 3, b4(c.vf[10][2]));
                fm::<2, 15>(c, 10, 4, b4(c.vf[0][3]));
                fm::<6, 15>(c, 14, 10, c.vf[19]);
                fm::<6, 15>(c, 15, 11, c.vf[19]);
                fm::<6, 15>(c, 16, 12, c.vf[19]);
                fm::<6, 15>(c, 17, 13, c.vf[19]);
                uycpu::vu0::clip(c, 17, 17);
                uycpu::vu0::clip(c, 16, 16);
                uycpu::vu0::clip(c, 15, 15);
                uycpu::vu0::clip(c, 14, 14);
                (c.r[T0], c.rh[T0]) = q4(c.vf[14]);
                c.r[T6] = 0x0000000000410000;
                (c.r[T1], c.rh[T1]) = q4(c.vf[15]);
                c.r[T6] = c.r[T6] | 0x410;
                (c.r[T2], c.rh[T2]) = q4(c.vf[16]);
                (c.r[T0], c.rh[T0]) = mmi::pcpyud((c.r[T0], c.rh[T0]), (0u64, 0u64));
                (c.r[T3], c.rh[T3]) = q4(c.vf[17]);
                (c.r[T1], c.rh[T1]) = mmi::pcpyud((c.r[T1], c.rh[T1]), (0u64, 0u64));
                c.r[T9] = sx(c.vi[18]);
                (c.r[T2], c.rh[T2]) = mmi::pcpyud((c.r[T2], c.rh[T2]), (0u64, 0u64));
                let t = (c.r[T0] as i64) >= 0;
                (c.r[T3], c.rh[T3]) = mmi::pcpyud((c.r[T3], c.rh[T3]), (0u64, 0u64));
                if t {
                    l = 0x00405be0;
                    continue;
                }
                c.r[T4] = c.r[T9] | 0x3;
                c.r[T5] = c.r[T9] | 0xc;
                let t = c.r[T4] != 0u64;
                if t {
                    l = 0x00405bd0;
                    continue;
                }
                c.r[T9] = c.r[T9] | 0x3;
                l = 0x00405bd0;
                continue;
            }
            0x00405bd0 => {
                let t = c.r[T5] != 0u64;
                if t {
                    l = 0x00405be0;
                    continue;
                }
                c.r[T9] = c.r[T9] | 0xc;
                l = 0x00405be0;
                continue;
            }
            0x00405be0 => {
                c.r[T6] = c.r[T6] & c.r[T9];
                c.r[T7] = c.r[T9] & 0x3f;
                let t = c.r[T6] != 0u64;
                c.r[T9] = srl(c.r[T9], 6);
                if t {
                    l = 0x00405ca0;
                    continue;
                }
                let t = c.r[T7] == 0u64;
                c.r[T4] = c.r[T9] & 0x3;
                if t {
                    l = 0x00405cb8;
                    continue;
                }
                let t = (c.r[T1] as i64) >= 0;
                if t {
                    l = 0x00405c20;
                    continue;
                }
                let t = c.r[T4] != 0u64;
                c.r[T5] = c.r[T9] & 0xc;
                if t {
                    l = 0x00405c10;
                    continue;
                }
                c.r[T9] = c.r[T9] | 0x3;
                l = 0x00405c10;
                continue;
            }
            0x00405c10 => {
                let t = c.r[T5] != 0u64;
                if t {
                    l = 0x00405c20;
                    continue;
                }
                c.r[T9] = c.r[T9] | 0xc;
                l = 0x00405c20;
                continue;
            }
            0x00405c20 => {
                c.r[T7] = c.r[T7] & c.r[T9];
                c.r[T9] = srl(c.r[T9], 6);
                let t = c.r[T7] == 0u64;
                c.r[T4] = c.r[T9] & 0x3;
                if t {
                    l = 0x00405cb8;
                    continue;
                }
                let t = (c.r[T2] as i64) >= 0;
                if t {
                    l = 0x00405c58;
                    continue;
                }
                let t = c.r[T4] != 0u64;
                c.r[T5] = c.r[T9] & 0xc;
                if t {
                    l = 0x00405c48;
                    continue;
                }
                c.r[T9] = c.r[T9] | 0x3;
                l = 0x00405c48;
                continue;
            }
            0x00405c48 => {
                let t = c.r[T5] != 0u64;
                if t {
                    l = 0x00405c58;
                    continue;
                }
                c.r[T9] = c.r[T9] | 0xc;
                l = 0x00405c58;
                continue;
            }
            0x00405c58 => {
                c.r[T7] = c.r[T7] & c.r[T9];
                c.r[T9] = srl(c.r[T9], 6);
                let t = c.r[T7] == 0u64;
                c.r[T4] = c.r[T9] & 0x3;
                if t {
                    l = 0x00405cb8;
                    continue;
                }
                let t = (c.r[T3] as i64) >= 0;
                if t {
                    l = 0x00405c90;
                    continue;
                }
                let t = c.r[T4] != 0u64;
                c.r[T5] = c.r[T9] & 0xc;
                if t {
                    l = 0x00405c80;
                    continue;
                }
                c.r[T9] = c.r[T9] | 0x3;
                l = 0x00405c80;
                continue;
            }
            0x00405c80 => {
                let t = c.r[T5] != 0u64;
                if t {
                    l = 0x00405c90;
                    continue;
                }
                c.r[T9] = c.r[T9] | 0xc;
                l = 0x00405c90;
                continue;
            }
            0x00405c90 => {
                c.r[T7] = c.r[T7] & c.r[T9];
                let t = c.r[T7] == 0u64;
                if t {
                    l = 0x00405cb8;
                    continue;
                }
                l = 0x00405ca0;
                continue;
            }
            0x00405ca0 => {
                n[5] += 1;
                (c.r[S0], c.rh[S0]) = mmi::pcpyud((c.r[S0], c.rh[S0]), (0u64, 0u64));
                (c.r[RA], c.rh[RA]) = mmi::pcpyud((c.r[S7], c.rh[S7]), (0u64, 0u64));
                (c.r[S1], c.rh[S1]) = mmi::pcpyud((c.r[S1], c.rh[S1]), (0u64, 0u64));
                (c.r[S2], c.rh[S2]) = mmi::pcpyud((c.r[S2], c.rh[S2]), (0u64, 0u64));
                (c.r[S3], c.rh[S3]) = mmi::pcpyud((c.r[S3], c.rh[S3]), (0u64, 0u64));
                return;
            }
            0x00405cb8 => {
                fm::<6, 15>(c, 14, 10, c.vf[26]);
                fm::<0, 2>(c, ACC, 14, b4(c.vf[15][2]));
                fm::<2, 2>(c, ACC, 16, b4(c.vf[0][3]));
                fm::<2, 2>(c, 1, 17, b4(c.vf[0][3]));
                fm::<6, 15>(c, 15, 11, c.vf[26]);
                fm::<6, 15>(c, 16, 12, c.vf[26]);
                fm::<6, 15>(c, 17, 13, c.vf[26]);
                uycpu::vu0::ftoi::<0, 15>(c, 1, 1);
                uycpu::vu0::clip(c, 14, 14);
                uycpu::vu0::clip(c, 15, 15);
                uycpu::vu0::clip(c, 16, 16);
                uycpu::vu0::clip(c, 17, 17);
                (c.r[T0], c.rh[T0]) = q4(c.vf[1]);
                c.r[T1] = 0u64 | 0x3ff;
                lqc2(c, 31, k.gp(c, gp, 0x00405cf0));
                c.r[A0] = sx(0x0000000c);
                (c.r[T0], c.rh[T0]) = mmi::pexew((c.r[T0], c.rh[T0]));
                lqc2(c, 27, (c.r[S0] as u32).wrapping_add(0x00000190));
                c.r[T0] = sx(((c.r[T0] as u32 as i32) >> 5) as u32);
                uycpu::vu0::qop::<0>(c, c.vf[31][0], c.vf[10][3]);
                c.r[V0] = sx(c.vi[18]);
                (c.r[T0], c.rh[T0]) = mmi::pminw((c.r[T0], c.rh[T0]), (c.r[T1], c.rh[T1]));
                lqc2(c, 28, (c.r[S0] as u32).wrapping_add(0x000001a0));
                (c.r[T0], c.rh[T0]) = mmi::pmaxw((c.r[T0], c.rh[T0]), (0u64, 0u64));
                let t = c.r[V0] != 0u64;
                c.r[S3] = sll(c.r[T0], 1);
                if t {
                    l = 0x00405f38;
                    continue;
                }
                n[6] += 1;
                c.r[RA] = k.pc(0x00405d28) as u64;
                lqc2(c, 14, (c.r[S2] as u32).wrapping_add(0x00000050));
                f406098(c, k.call(c, 0x00405d20, 0x40_6098));
                c.r[T0] = 0x0000000020000000;
                c.r[T1] = 0u64 | 0xee;
                c.r[T0] = c.r[T0] << 32;
                c.r[T2] = c.rd64((c.r[S2] as u32).wrapping_add(0x00000080));
                c.r[T0] = c.r[T0] | 0x1;
                c.r[T3] = 0u64 | 0x14;
                fm::<6, 15>(c, ACC, 28, b4(c.vf[0][3]));
                fm::<2, 15>(c, 10, 10, b4(c.vi[22]));
                fm::<6, 12>(c, 14, 14, b4(c.vi[22]));
                fm::<6, 1>(c, 1, 0, b4(c.vi[22]));
                (c.r[S0], c.rh[S0]) = mmi::pcpyud((c.r[S0], c.rh[S0]), (0u64, 0u64));
                uycpu::vu0::ftoi::<1, 15>(c, 10, 10);
                (c.r[T4], c.rh[T4]) = mmi::pcpyld((c.r[T1], c.rh[T1]), (c.r[T0], c.rh[T0]));
                (c.r[T5], c.rh[T5]) = mmi::pcpyld((c.r[T3], c.rh[T3]), (c.r[T2], c.rh[T2]));
                uycpu::vu0::qop::<0>(c, c.vf[31][0], c.vf[11][3]);
                (c.r[T0], c.rh[T0]) = q4(c.vf[14]);
                (c.r[T1], c.rh[T1]) = q4(c.vf[10]);
                c.wr64((c.r[AT] as u32).wrapping_add(0x00000058), c.r[T0]);
                (c.r[T2], c.rh[T2]) = mmi::ppach((c.r[T1], c.rh[T1]), (c.r[T1], c.rh[T1]));
                c.wr128(c.r[AT] as u32, c.r[T4], c.rh[T4]);
                (c.r[T1], c.rh[T1]) = mmi::pcpyud((c.r[T1], c.rh[T1]), (0u64, 0u64));
                c.wr128((c.r[AT] as u32).wrapping_add(0x00000010), c.r[T5], c.rh[T5]);
                (c.r[T1], c.rh[T1]) = mmi::psraw((c.r[T1], c.rh[T1]), 4);
                c.wr32((c.r[AT] as u32).wrapping_add(0x00000068), c.r[T2] as u32);
                c.wr32((c.r[AT] as u32).wrapping_add(0x0000006c), c.r[T1] as u32);
                c.r[T1] = ((c.r[T1] as i64) >> 32) as u64;
                c.r[T3] = sx(0x000000ff);
                (c.r[T1], c.rh[T1]) = mmi::pminw((c.r[T1], c.rh[T1]), (c.r[T3], c.rh[T3]));
                (c.r[T1], c.rh[T1]) = mmi::pmaxw((c.r[T1], c.rh[T1]), (0u64, 0u64));
                c.wr8((c.r[AT] as u32).wrapping_add(0x0000006f), c.r[T1] as u8);
                c.r[T0] = 0xfffffffff4000000;
                c.r[T1] = 0x0000000004120000;
                c.r[T0] = c.r[T0] << 32;
                c.r[T1] = c.r[T1] | 0x4124;
                c.r[T0] = c.r[T0] | 0x8001;
                c.r[T2] = 0x0000000012410000;
                c.r[T2] = c.r[T2] | 0x2068;
                c.wr64((c.r[AT] as u32).wrapping_add(0x000000b8), 0u64);
                (c.r[T1], c.rh[T1]) = mmi::pextlw((c.r[T1], c.rh[T1]), (c.r[T2], c.rh[T2]));
                (c.r[T0], c.rh[T0]) = mmi::pcpyld((c.r[T1], c.rh[T1]), (c.r[T0], c.rh[T0]));
                c.wr128((c.r[AT] as u32).wrapping_add(0x00000030), c.r[T0], c.rh[T0]);
                {
                    let v = c.vf[1];
                    uycpu::vu0::mov::<15>(c, 1, [v[1], v[2], v[3], v[0]]);
                }
                fm::<2, 15>(c, 11, 11, b4(c.vi[22]));
                fm::<6, 3>(c, 14, 14, b4(c.vi[22]));
                fm::<6, 1>(c, 1, 0, b4(c.vi[22]));
                uycpu::vu0::qop::<0>(c, c.vf[31][0], c.vf[12][3]);
                uycpu::vu0::ftoi::<1, 15>(c, 11, 11);
                (c.r[T0], c.rh[T0]) = q4(c.vf[14]);
                (c.r[T1], c.rh[T1]) = q4(c.vf[11]);
                c.wr64((c.r[AT] as u32).wrapping_add(0x00000070), c.r[T0]);
                (c.r[T2], c.rh[T2]) = mmi::ppach((c.r[T1], c.rh[T1]), (c.r[T1], c.rh[T1]));
                (c.r[T1], c.rh[T1]) = mmi::pcpyud((c.r[T1], c.rh[T1]), (0u64, 0u64));
                (c.r[T1], c.rh[T1]) = mmi::psraw((c.r[T1], c.rh[T1]), 4);
                c.wr32((c.r[AT] as u32).wrapping_add(0x00000080), c.r[T2] as u32);
                c.wr32((c.r[AT] as u32).wrapping_add(0x00000084), c.r[T1] as u32);
                c.r[T1] = ((c.r[T1] as i64) >> 32) as u64;
                c.r[T3] = sx(0x000000ff);
                (c.r[T1], c.rh[T1]) = mmi::pminw((c.r[T1], c.rh[T1]), (c.r[T3], c.rh[T3]));
                (c.r[T1], c.rh[T1]) = mmi::pmaxw((c.r[T1], c.rh[T1]), (0u64, 0u64));
                c.wr8((c.r[AT] as u32).wrapping_add(0x00000087), c.r[T1] as u8);
                (c.r[T1], c.rh[T1]) = q4(c.vf[14]);
                c.wr64((c.r[AT] as u32).wrapping_add(0x00000058), c.r[T1]);
                (c.r[T1], c.rh[T1]) = mmi::pcpyud((c.r[T1], c.rh[T1]), (c.r[T1], c.rh[T1]));
                c.wr64((c.r[AT] as u32).wrapping_add(0x00000070), c.r[T1]);
                lqc2(c, 15, (c.r[S2] as u32).wrapping_add(0x00000060));
                {
                    let v = c.vf[1];
                    uycpu::vu0::mov::<15>(c, 1, [v[1], v[2], v[3], v[0]]);
                }
                fm::<2, 15>(c, 12, 12, b4(c.vi[22]));
                fm::<6, 12>(c, 15, 15, b4(c.vi[22]));
                fm::<6, 1>(c, 1, 0, b4(c.vi[22]));
                uycpu::vu0::qop::<0>(c, c.vf[31][0], c.vf[13][3]);
                uycpu::vu0::ftoi::<1, 15>(c, 12, 12);
                (c.r[T0], c.rh[T0]) = q4(c.vf[14]);
                (c.r[T1], c.rh[T1]) = q4(c.vf[12]);
                c.wr64((c.r[AT] as u32).wrapping_add(0x00000088), c.r[T0]);
                (c.r[T2], c.rh[T2]) = mmi::ppach((c.r[T1], c.rh[T1]), (c.r[T1], c.rh[T1]));
                (c.r[T1], c.rh[T1]) = mmi::pcpyud((c.r[T1], c.rh[T1]), (0u64, 0u64));
                (c.r[T1], c.rh[T1]) = mmi::psraw((c.r[T1], c.rh[T1]), 4);
                c.wr32((c.r[AT] as u32).wrapping_add(0x00000098), c.r[T2] as u32);
                c.wr32((c.r[AT] as u32).wrapping_add(0x0000009c), c.r[T1] as u32);
                c.r[T1] = ((c.r[T1] as i64) >> 32) as u64;
                c.r[T3] = sx(0x000000ff);
                (c.r[T1], c.rh[T1]) = mmi::pminw((c.r[T1], c.rh[T1]), (c.r[T3], c.rh[T3]));
                (c.r[T1], c.rh[T1]) = mmi::pmaxw((c.r[T1], c.rh[T1]), (0u64, 0u64));
                c.wr8((c.r[AT] as u32).wrapping_add(0x0000009f), c.r[T1] as u8);
                c.r[T0] = c.rd64((c.r[S2] as u32).wrapping_add(0x00000088));
                c.r[T1] = 0u64 | 0x42;
                c.r[T2] = c.rd64((c.r[S2] as u32).wrapping_add(0x00000070));
                c.r[T3] = 0u64 | 0x7c;
                (c.r[T0], c.rh[T0]) = mmi::pcpyld((c.r[T1], c.rh[T1]), (c.r[T0], c.rh[T0]));
                c.wr64((c.r[AT] as u32).wrapping_add(0x00000050), c.r[T3]);
                c.r[T3] = c.rd64((c.r[S2] as u32).wrapping_add(0x00000078));
                c.wr128((c.r[AT] as u32).wrapping_add(0x00000020), c.r[T0], c.rh[T0]);
                (c.r[T2], c.rh[T2]) = mmi::pcpyld((c.r[T3], c.rh[T3]), (c.r[T2], c.rh[T2]));
                c.wr128((c.r[AT] as u32).wrapping_add(0x00000040), c.r[T2], c.rh[T2]);
                {
                    let v = c.vf[1];
                    uycpu::vu0::mov::<15>(c, 1, [v[1], v[2], v[3], v[0]]);
                }
                fm::<2, 15>(c, 13, 13, b4(c.vi[22]));
                fm::<6, 3>(c, 15, 15, b4(c.vi[22]));
                fm::<6, 1>(c, 1, 0, b4(c.vi[22]));
                uycpu::vu0::ftoi::<1, 15>(c, 13, 13);
                (c.r[T0], c.rh[T0]) = q4(c.vf[14]);
                (c.r[T1], c.rh[T1]) = q4(c.vf[13]);
                c.wr64((c.r[AT] as u32).wrapping_add(0x000000a0), c.r[T0]);
                (c.r[T2], c.rh[T2]) = mmi::ppach((c.r[T1], c.rh[T1]), (c.r[T1], c.rh[T1]));
                (c.r[T1], c.rh[T1]) = mmi::pcpyud((c.r[T1], c.rh[T1]), (0u64, 0u64));
                (c.r[T1], c.rh[T1]) = mmi::psraw((c.r[T1], c.rh[T1]), 4);
                c.wr32((c.r[AT] as u32).wrapping_add(0x000000b0), c.r[T2] as u32);
                c.wr32((c.r[AT] as u32).wrapping_add(0x000000b4), c.r[T1] as u32);
                c.r[T1] = ((c.r[T1] as i64) >> 32) as u64;
                c.r[T3] = sx(0x000000ff);
                (c.r[T1], c.rh[T1]) = mmi::pminw((c.r[T1], c.rh[T1]), (c.r[T3], c.rh[T3]));
                (c.r[T1], c.rh[T1]) = mmi::pmaxw((c.r[T1], c.rh[T1]), (0u64, 0u64));
                c.wr8((c.r[AT] as u32).wrapping_add(0x000000b7), c.r[T1] as u8);
                (c.r[T1], c.rh[T1]) = q4(c.vf[15]);
                c.wr64((c.r[AT] as u32).wrapping_add(0x00000088), c.r[T1]);
                (c.r[T1], c.rh[T1]) = mmi::pcpyud((c.r[T1], c.rh[T1]), (c.r[T1], c.rh[T1]));
                c.wr64((c.r[AT] as u32).wrapping_add(0x000000a0), c.r[T1]);
                (c.r[RA], c.rh[RA]) = mmi::pcpyud((c.r[S7], c.rh[S7]), (0u64, 0u64));
                (c.r[T0], c.rh[T0]) = q4(c.vf[1]);
                (c.r[S1], c.rh[S1]) = mmi::pcpyud((c.r[S1], c.rh[S1]), (0u64, 0u64));
                (c.r[T1], c.rh[T1]) = c.rd128((c.r[S2] as u32).wrapping_add(0x00000040));
                (c.r[T2], c.rh[T2]) = mmi::pextlw((c.r[T0], c.rh[T0]), (c.r[T1], c.rh[T1]));
                c.wr64((c.r[AT] as u32).wrapping_add(0x00000060), c.r[T2]);
                (c.r[T2], c.rh[T2]) = mmi::pcpyud((c.r[T2], c.rh[T2]), (c.r[T2], c.rh[T2]));
                c.wr64((c.r[AT] as u32).wrapping_add(0x00000078), c.r[T2]);
                (c.r[T2], c.rh[T2]) = mmi::pextuw((c.r[T0], c.rh[T0]), (c.r[T1], c.rh[T1]));
                c.wr64((c.r[AT] as u32).wrapping_add(0x00000090), c.r[T2]);
                (c.r[T2], c.rh[T2]) = mmi::pcpyud((c.r[T2], c.rh[T2]), (c.r[T2], c.rh[T2]));
                c.wr64((c.r[AT] as u32).wrapping_add(0x000000a8), c.r[T2]);
                (c.r[S2], c.rh[S2]) = mmi::pcpyud((c.r[S2], c.rh[S2]), (0u64, 0u64));
                (c.r[S3], c.rh[S3]) = mmi::pcpyud((c.r[S3], c.rh[S3]), (0u64, 0u64));
                return;
            }
            0x00405f38 => {
                n[7] += 1;
                let t = c.r[S1] != 0u64;
                lqc2(c, 29, (c.r[S0] as u32).wrapping_add(0x000001b0));
                if t {
                    l = 0x00405f48;
                    continue;
                }
                c.r[RA] = k.pc(0x00405f48) as u64;
                f406018(c, k.call(c, 0x00405f40, 0x40_6018));
                l = 0x00405f48;
                continue;
            }
            0x00405f48 => {
                (c.r[T3], c.rh[T3]) = c.rd128((c.r[S2] as u32).wrapping_add(0x00000040));
                c.r[T8] = 0x0000000070000000;
                (c.r[T0], c.rh[T0]) = c.rd128((c.r[S2] as u32).wrapping_add(0x00000050));
                c.r[T8] = c.r[T8] | 0x1c40;
                c.r[T1] = c.rd64((c.r[S2] as u32).wrapping_add(0x00000060));
                (c.r[T2], c.rh[T2]) = mmi::pextlw((0u64, 0u64), (c.r[T3], c.rh[T3]));
                sqc2(c, 14, c.r[T8] as u32);
                (c.r[T3], c.rh[T3]) = mmi::pextuw((0u64, 0u64), (c.r[T3], c.rh[T3]));
                sqc2(c, 14, (c.r[T8] as u32).wrapping_add(0x00000060));
                (c.r[T4], c.rh[T4]) = mmi::pcpyld((c.r[T2], c.rh[T2]), (c.r[T0], c.rh[T0]));
                c.wr128((c.r[T8] as u32).wrapping_add(0x00000010), c.r[T4], c.rh[T4]);
                c.r[T9] = add32(c.r[T8], 0x00000120);
                c.wr128((c.r[T8] as u32).wrapping_add(0x00000070), c.r[T4], c.rh[T4]);
                (c.r[T8], c.rh[T8]) = mmi::pcpyld((c.r[T9], c.rh[T9]), (c.r[T8], c.rh[T8]));
                sqc2(c, 15, (c.r[T8] as u32).wrapping_add(0x00000020));
                (c.r[T4], c.rh[T4]) = mmi::pcpyud((c.r[T0], c.rh[T0]), (c.r[T2], c.rh[T2]));
                c.wr128((c.r[T8] as u32).wrapping_add(0x00000030), c.r[T4], c.rh[T4]);
                fm::<6, 15>(c, 23, 14, c.vf[29]);
                sqc2(c, 16, (c.r[T8] as u32).wrapping_add(0x00000040));
                (c.r[T4], c.rh[T4]) = mmi::pcpyld((c.r[T3], c.rh[T3]), (c.r[T1], c.rh[T1]));
                c.r[RA] = k.pc(0x00405fa0) as u64;
                c.wr128((c.r[T8] as u32).wrapping_add(0x00000050), c.r[T4], c.rh[T4]);
                f4060d8(c, k.call(c, 0x00405f98, 0x40_60d8), n);
                (c.r[T3], c.rh[T3]) = c.rd128((c.r[S2] as u32).wrapping_add(0x00000040));
                (c.r[T0], c.rh[T0]) = c.rd128((c.r[S2] as u32).wrapping_add(0x00000050));
                (c.r[T1], c.rh[T1]) = c.rd128((c.r[S2] as u32).wrapping_add(0x00000060));
                (c.r[T2], c.rh[T2]) = mmi::pextlw((0u64, 0u64), (c.r[T3], c.rh[T3]));
                sqc2(c, 15, c.r[T8] as u32);
                (c.r[T3], c.rh[T3]) = mmi::pextuw((0u64, 0u64), (c.r[T3], c.rh[T3]));
                sqc2(c, 15, (c.r[T8] as u32).wrapping_add(0x00000060));
                (c.r[T4], c.rh[T4]) = mmi::pcpyud((c.r[T0], c.rh[T0]), (c.r[T2], c.rh[T2]));
                c.wr128((c.r[T8] as u32).wrapping_add(0x00000010), c.r[T4], c.rh[T4]);
                c.wr128((c.r[T8] as u32).wrapping_add(0x00000070), c.r[T4], c.rh[T4]);
                sqc2(c, 16, (c.r[T8] as u32).wrapping_add(0x00000020));
                (c.r[T4], c.rh[T4]) = mmi::pcpyld((c.r[T3], c.rh[T3]), (c.r[T1], c.rh[T1]));
                c.wr128((c.r[T8] as u32).wrapping_add(0x00000030), c.r[T4], c.rh[T4]);
                fm::<6, 15>(c, 23, 15, c.vf[29]);
                sqc2(c, 17, (c.r[T8] as u32).wrapping_add(0x00000040));
                (c.r[T4], c.rh[T4]) = mmi::pcpyud((c.r[T1], c.rh[T1]), (c.r[T3], c.rh[T3]));
                c.r[RA] = k.pc(0x00405fe8) as u64;
                c.wr128((c.r[T8] as u32).wrapping_add(0x00000050), c.r[T4], c.rh[T4]);
                f4060d8(c, k.call(c, 0x00405fe0, 0x40_60d8), n);
                let t = c.r[S1] == 0u64;
                (c.r[S0], c.rh[S0]) = mmi::pcpyud((c.r[S0], c.rh[S0]), (0u64, 0u64));
                if t {
                    l = 0x00405ff8;
                    continue;
                }
                c.r[RA] = k.pc(0x00405ff8) as u64;
                f406018(c, k.call(c, 0x00405ff0, 0x40_6018));
                l = 0x00405ff8;
                continue;
            }
            0x00405ff8 => {
                (c.r[RA], c.rh[RA]) = mmi::pcpyud((c.r[S7], c.rh[S7]), (0u64, 0u64));
                (c.r[S1], c.rh[S1]) = mmi::pcpyud((c.r[S1], c.rh[S1]), (0u64, 0u64));
                (c.r[S2], c.rh[S2]) = mmi::pcpyud((c.r[S2], c.rh[S2]), (0u64, 0u64));
                (c.r[S3], c.rh[S3]) = mmi::pcpyud((c.r[S3], c.rh[S3]), (0u64, 0u64));
                return;
            }
            _ => unreachable!("0x405A58: no block at {l:08x}"),
        }
    }
}

fn f406018(c: &mut Cpu, k: F) {
    let mut l: u32 = 0x00406018;
    loop {
        match l {
            0x00406018 => {
                (c.r[RA], c.rh[RA]) = mmi::pexew((c.r[RA], c.rh[RA]));
                c.r[RA] = k.pc(0x00406028) as u64;
                c.r[A0] = sx(0x00000005);
                f406098(c, k.call(c, 0x00406020, 0x40_6098));
                c.r[T0] = 0x0000000024000000;
                c.r[T2] = c.rd64((c.r[S2] as u32).wrapping_add(0x00000070));
                c.r[T0] = c.r[T0] << 32;
                c.r[T3] = c.rd64((c.r[S2] as u32).wrapping_add(0x00000078));
                c.r[T0] = c.r[T0] | 0x1;
                c.r[T1] = 0u64 | 0x68;
                (c.r[T2], c.rh[T2]) = mmi::pcpyld((c.r[T3], c.rh[T3]), (c.r[T2], c.rh[T2]));
                c.r[T4] = c.rd64((c.r[S2] as u32).wrapping_add(0x00000080));
                (c.r[T0], c.rh[T0]) = mmi::pcpyld((c.r[T1], c.rh[T1]), (c.r[T0], c.rh[T0]));
                c.wr128((c.r[AT] as u32).wrapping_add(0x00000010), c.r[T2], c.rh[T2]);
                c.wr128(c.r[AT] as u32, c.r[T0], c.rh[T0]);
                c.r[T3] = 0u64 | 0x14;
                (c.r[T4], c.rh[T4]) = mmi::pcpyld((c.r[T3], c.rh[T3]), (c.r[T4], c.rh[T4]));
                c.r[T2] = c.rd64((c.r[S2] as u32).wrapping_add(0x00000088));
                c.r[T0] = 0x0000000020000000;
                c.r[T1] = 0u64 | 0xee;
                c.r[T0] = c.r[T0] << 32;
                c.r[T3] = 0u64 | 0x42;
                c.r[T0] = c.r[T0] | 0x8001;
                (c.r[T0], c.rh[T0]) = mmi::pcpyld((c.r[T1], c.rh[T1]), (c.r[T0], c.rh[T0]));
                c.wr128((c.r[AT] as u32).wrapping_add(0x00000030), c.r[T4], c.rh[T4]);
                l = 0x00406080;
                continue;
            }
            0x00406080 => {
                (c.r[T2], c.rh[T2]) = mmi::pcpyld((c.r[T3], c.rh[T3]), (c.r[T2], c.rh[T2]));
                c.wr128((c.r[AT] as u32).wrapping_add(0x00000020), c.r[T0], c.rh[T0]);
                (c.r[RA], c.rh[RA]) = mmi::pexew((c.r[RA], c.rh[RA]));
                c.wr128((c.r[AT] as u32).wrapping_add(0x00000040), c.r[T2], c.rh[T2]);
                return;
            }
            _ => unreachable!("0x406018: no block at {l:08x}"),
        }
    }
}

fn f406098(c: &mut Cpu, k: F) {
    let gp = c.r[GP] as u32;
    let l: u32 = 0x00406098;
    loop {
        match l {
            0x00406098 => {
                c.r[T9] = sll(c.r[A0], 4);
                c.r[AT] = sx(c.rd32(k.gp(c, gp, 0x004060a0)));
                c.r[T0] = 0x0000000010000000;
                c.r[T9] = add32(c.r[T9], 0x00000010);
                c.r[T0] = c.r[T0] | c.r[A0];
                c.wr128(c.r[AT] as u32, 0u64, 0u64);
                c.r[T1] = 0x0000000040000000;
                c.wr32(c.r[AT] as u32, c.r[T0] as u32);
                c.r[T0] = c.r[T0] | c.r[T1];
                c.wr32((c.r[AT] as u32).wrapping_add(0x0000000c), c.r[T0] as u32);
                c.r[T2] = add32(c.r[AT], c.r[T9]);
                c.r[AT] = add32(c.r[AT], 0x00000010);
                c.wr32(k.gp(c, gp, 0x004060d4), c.r[T2] as u32);
                return;
            }
            _ => unreachable!("0x406098: no block at {l:08x}"),
        }
    }
}

fn f4060d8(c: &mut Cpu, k: F, n: &mut [u64; N]) {
    let mut l: u32 = 0x004060d8;
    loop {
        match l {
            0x004060d8 => {
                c.r[T9] = sx(0x00000005);
                (c.r[RA], c.rh[RA]) = mmi::pexew((c.r[RA], c.rh[RA]));
                c.r[T7] = sx(0x00000003);
                fm::<0, 14>(c, 24, 0, b4(c.vf[0][3]));
                l = 0x004060e8;
                continue;
            }
            0x004060e8 => {
                c.r[T6] = 0u64 | 0u64;
                (c.r[T3], c.rh[T3]) = mmi::pexew((c.r[T8], c.rh[T8]));
                l = 0x004060f0;
                continue;
            }
            0x004060f0 => {
                c.r[T4] = sll(c.r[T6], 5);
                c.r[T5] = add32(c.r[T6], 0x00000001);
                c.r[T4] = add32(c.r[T4], c.r[T8]);
                c.r[T5] = sll(c.r[T5], 5);
                lqc2(c, 1, c.r[T4] as u32);
                c.r[T5] = add32(c.r[T5], c.r[T8]);
                lqc2(c, 3, c.r[T5] as u32);
                uycpu::vu0::clip(c, 1, 1);
                uycpu::vu0::clip(c, 3, 3);
                lqc2(c, 2, (c.r[T4] as u32).wrapping_add(0x00000010));
                sqc2(c, 1, c.r[T3] as u32);
                sqc2(c, 2, (c.r[T3] as u32).wrapping_add(0x00000010));
                c.r[T0] = sx(c.vi[18]);
                c.r[T0] = sx((c.r[T0] as u32).wrapping_shr(c.r[T9] as u32 & 31));
                c.r[T1] = c.r[T0] & 0x40;
                c.r[T2] = sll(c.r[T0], 6);
                let t = c.r[T1] == 0u64;
                if t {
                    c.r[T3] = add32(c.r[T3], 0x00000020);
                    l = 0x00406150;
                    continue;
                }
                l = 0x00406150;
                continue;
            }
            0x00406150 => {
                c.r[T0] = c.r[T1] ^ c.r[T2];
                c.r[T6] = add32(c.r[T6], 0x00000001);
                c.r[T0] = c.r[T0] & 0x40;
                let t = c.r[T0] == 0u64;
                c.r[A0] = c.r[T3];
                if t {
                    l = 0x00406180;
                    continue;
                }
                c.r[A1] = c.r[T4];
                c.r[A2] = c.r[T5];
                c.r[RA] = k.pc(0x00406178) as u64;
                c.r[A3] = c.r[T9];
                f4057c0(c);
                c.r[T3] = add32(c.r[T3], 0x00000020);
                l = 0x00406180;
                continue;
            }
            0x00406180 => {
                let t = c.r[T6] != c.r[T7];
                if t {
                    l = 0x004060f0;
                    continue;
                }
                (c.r[T8], c.rh[T8]) = mmi::pexew((c.r[T8], c.rh[T8]));
                (c.r[T0], c.rh[T0]) = c.rd128(c.r[T8] as u32);
                (c.r[T1], c.rh[T1]) = c.rd128((c.r[T8] as u32).wrapping_add(0x00000010));
                c.wr128(c.r[T3] as u32, c.r[T0], c.rh[T0]);
                c.wr128((c.r[T3] as u32).wrapping_add(0x00000010), c.r[T1], c.rh[T1]);
                c.r[T7] = sub32(c.r[T3], c.r[T8]);
                let t = c.r[T7] == 0u64;
                c.r[T7] = srl(c.r[T7], 5);
                if t {
                    l = 0x004066c4;
                    continue;
                }
                let t = c.r[T9] != 0u64;
                c.r[T9] = add32(c.r[T9], 0xffffffff);
                if t {
                    l = 0x004060e8;
                    continue;
                }
                c.r[T9] = add32(c.r[T7], 0xfffffffd);
                let t = (c.r[T9] as i64) <= 0;
                if t {
                    l = 0x004065f0;
                    continue;
                }
                lqc2(c, 9, (c.r[T8] as u32).wrapping_add(0x00000100));
                c.r[T9] = 0u64 | 0x9;
                lqc2(c, 8, (c.r[T8] as u32).wrapping_add(0x000000e0));
                c.r[T9] = sub32(c.r[T9], c.r[T7]);
                lqc2(c, 7, (c.r[T8] as u32).wrapping_add(0x000000c0));
                c.r[T9] = sll(c.r[T9], 2);
                lqc2(c, 6, (c.r[T8] as u32).wrapping_add(0x000000a0));
                c.r[T0] = sx(k.hi(c, 0x004061e4));
                c.r[T0] = add32(c.r[T0], k.lo(c, 0x004061e8) as u64);
                lqc2(c, 5, (c.r[T8] as u32).wrapping_add(0x00000080));
                lqc2(c, 4, (c.r[T8] as u32).wrapping_add(0x00000060));
                c.r[T1] = add32(c.r[T0], c.r[T9]);
                lqc2(c, 3, (c.r[T8] as u32).wrapping_add(0x00000040));
                c.r[T6] = sll(c.r[T9], 1);
                lqc2(c, 2, (c.r[T8] as u32).wrapping_add(0x00000020));
                c.r[T5] = 0x0000000070000000;
                let t = c.r[T1] as u32;
                lqc2(c, 1, c.r[T8] as u32);
                l = t.wrapping_sub(k.at).wrapping_add(k.l1);
                continue;
            }
            0x00406210 => {
                fm::<6, 14>(c, 9, 9, c.vf[29]);
                l = 0x00406214;
                continue;
            }
            0x00406214 => {
                fm::<6, 14>(c, 8, 8, c.vf[29]);
                l = 0x00406218;
                continue;
            }
            0x00406218 => {
                fm::<6, 14>(c, 7, 7, c.vf[29]);
                l = 0x0040621c;
                continue;
            }
            0x0040621c => {
                fm::<6, 14>(c, 6, 6, c.vf[29]);
                l = 0x00406220;
                continue;
            }
            0x00406220 => {
                fm::<6, 14>(c, 5, 5, c.vf[29]);
                l = 0x00406224;
                continue;
            }
            0x00406224 => {
                c.r[T5] = c.r[T5] | 0x1ea0;
                l = 0x00406228;
                continue;
            }
            0x00406228 => {
                fm::<6, 14>(c, 4, 4, c.vf[29]);
                l = 0x0040622c;
                continue;
            }
            0x0040622c => {
                c.r[T0] = sx(k.hi(c, 0x0040622c));
                l = 0x00406230;
                continue;
            }
            0x00406230 => {
                c.r[T0] = add32(c.r[T0], k.lo(c, 0x00406230) as u64);
                l = 0x00406234;
                continue;
            }
            0x00406234 => {
                fm::<6, 14>(c, 3, 3, c.vf[29]);
                l = 0x00406238;
                continue;
            }
            0x00406238 => {
                fm::<6, 14>(c, 2, 2, c.vf[29]);
                l = 0x0040623c;
                continue;
            }
            0x0040623c => {
                c.r[T1] = add32(c.r[T0], c.r[T6]);
                l = 0x00406240;
                continue;
            }
            0x00406240 => {
                let t = c.r[T1] as u32;
                fm::<6, 14>(c, 1, 1, c.vf[29]);
                l = t.wrapping_sub(k.at).wrapping_add(k.l1);
                continue;
            }
            0x00406248 => {
                l = 0x0040624c;
                continue;
            }
            0x0040624c => {
                sqc2(c, 9, (c.r[T5] as u32).wrapping_add(0x00000100));
                l = 0x00406250;
                continue;
            }
            0x00406250 => {
                fm::<1, 14>(c, 9, 9, c.vf[23]);
                l = 0x00406254;
                continue;
            }
            0x00406254 => {
                sqc2(c, 8, (c.r[T5] as u32).wrapping_add(0x000000e0));
                l = 0x00406258;
                continue;
            }
            0x00406258 => {
                fm::<1, 14>(c, 8, 8, c.vf[23]);
                l = 0x0040625c;
                continue;
            }
            0x0040625c => {
                sqc2(c, 7, (c.r[T5] as u32).wrapping_add(0x000000c0));
                l = 0x00406260;
                continue;
            }
            0x00406260 => {
                fm::<1, 14>(c, 7, 7, c.vf[23]);
                l = 0x00406264;
                continue;
            }
            0x00406264 => {
                sqc2(c, 6, (c.r[T5] as u32).wrapping_add(0x000000a0));
                l = 0x00406268;
                continue;
            }
            0x00406268 => {
                fm::<1, 14>(c, 6, 6, c.vf[23]);
                l = 0x0040626c;
                continue;
            }
            0x0040626c => {
                sqc2(c, 5, (c.r[T5] as u32).wrapping_add(0x00000080));
                l = 0x00406270;
                continue;
            }
            0x00406270 => {
                fm::<1, 14>(c, 5, 5, c.vf[23]);
                l = 0x00406274;
                continue;
            }
            0x00406274 => {
                sqc2(c, 4, (c.r[T5] as u32).wrapping_add(0x00000060));
                l = 0x00406278;
                continue;
            }
            0x00406278 => {
                c.r[T0] = sll(c.r[T7], 5);
                l = 0x0040627c;
                continue;
            }
            0x0040627c => {
                sqc2(c, 3, (c.r[T5] as u32).wrapping_add(0x00000040));
                l = 0x00406280;
                continue;
            }
            0x00406280 => {
                c.r[T0] = add32(c.r[T0], c.r[T5]);
                l = 0x00406284;
                continue;
            }
            0x00406284 => {
                sqc2(c, 2, (c.r[T5] as u32).wrapping_add(0x00000020));
                l = 0x00406288;
                continue;
            }
            0x00406288 => {
                l = 0x0040628c;
                continue;
            }
            0x0040628c => {
                sqc2(c, 1, c.r[T5] as u32);
                l = 0x00406290;
                continue;
            }
            0x00406290 => {
                sqc2(c, 1, c.r[T0] as u32);
                l = 0x00406294;
                continue;
            }
            0x00406294 => {
                fm::<1, 14>(c, 4, 4, c.vf[23]);
                l = 0x00406298;
                continue;
            }
            0x00406298 => {
                c.r[T0] = sx(k.hi(c, 0x00406298));
                l = 0x0040629c;
                continue;
            }
            0x0040629c => {
                c.r[T0] = add32(c.r[T0], k.lo(c, 0x0040629c) as u64);
                l = 0x004062a0;
                continue;
            }
            0x004062a0 => {
                fm::<1, 14>(c, 3, 3, c.vf[23]);
                l = 0x004062a4;
                continue;
            }
            0x004062a4 => {
                c.r[T1] = add32(c.r[T0], c.r[T9]);
                l = 0x004062a8;
                continue;
            }
            0x004062a8 => {
                fm::<1, 14>(c, 2, 2, c.vf[23]);
                l = 0x004062ac;
                continue;
            }
            0x004062ac => {
                let t = c.r[T1] as u32;
                fm::<1, 14>(c, 1, 1, c.vf[23]);
                l = t.wrapping_sub(k.at).wrapping_add(k.l1);
                continue;
            }
            0x004062b4 => {
                fm::<6, 14>(c, 9, 9, c.vf[9]);
                l = 0x004062b8;
                continue;
            }
            0x004062b8 => {
                fm::<6, 14>(c, 8, 8, c.vf[8]);
                l = 0x004062bc;
                continue;
            }
            0x004062bc => {
                fm::<6, 14>(c, 7, 7, c.vf[7]);
                l = 0x004062c0;
                continue;
            }
            0x004062c0 => {
                fm::<6, 14>(c, 6, 6, c.vf[6]);
                l = 0x004062c4;
                continue;
            }
            0x004062c4 => {
                fm::<6, 14>(c, 5, 5, c.vf[5]);
                l = 0x004062c8;
                continue;
            }
            0x004062c8 => {
                c.r[T9] = sll(c.r[T9], 1);
                l = 0x004062cc;
                continue;
            }
            0x004062cc => {
                fm::<6, 14>(c, 4, 4, c.vf[4]);
                l = 0x004062d0;
                continue;
            }
            0x004062d0 => {
                c.r[T0] = sx(k.hi(c, 0x004062d0));
                l = 0x004062d4;
                continue;
            }
            0x004062d4 => {
                c.r[T0] = add32(c.r[T0], k.lo(c, 0x004062d4) as u64);
                l = 0x004062d8;
                continue;
            }
            0x004062d8 => {
                fm::<6, 14>(c, 3, 3, c.vf[3]);
                l = 0x004062dc;
                continue;
            }
            0x004062dc => {
                fm::<6, 14>(c, 2, 2, c.vf[2]);
                l = 0x004062e0;
                continue;
            }
            0x004062e0 => {
                c.r[T1] = add32(c.r[T0], c.r[T9]);
                l = 0x004062e4;
                continue;
            }
            0x004062e4 => {
                let t = c.r[T1] as u32;
                fm::<6, 14>(c, 1, 1, c.vf[1]);
                l = t.wrapping_sub(k.at).wrapping_add(k.l1);
                continue;
            }
            0x004062ec => {
                fm::<0, 4>(c, ACC, 9, b4(c.vf[9][0]));
                l = 0x004062f0;
                continue;
            }
            0x004062f0 => {
                fm::<2, 4>(c, 9, 24, b4(c.vf[9][2]));
                l = 0x004062f4;
                continue;
            }
            0x004062f4 => {
                fm::<0, 4>(c, ACC, 8, b4(c.vf[8][0]));
                l = 0x004062f8;
                continue;
            }
            0x004062f8 => {
                fm::<2, 4>(c, 8, 24, b4(c.vf[8][2]));
                l = 0x004062fc;
                continue;
            }
            0x004062fc => {
                fm::<0, 4>(c, ACC, 7, b4(c.vf[7][0]));
                l = 0x00406300;
                continue;
            }
            0x00406300 => {
                fm::<2, 4>(c, 7, 24, b4(c.vf[7][2]));
                l = 0x00406304;
                continue;
            }
            0x00406304 => {
                fm::<0, 4>(c, ACC, 6, b4(c.vf[6][0]));
                l = 0x00406308;
                continue;
            }
            0x00406308 => {
                fm::<2, 4>(c, 6, 24, b4(c.vf[6][2]));
                l = 0x0040630c;
                continue;
            }
            0x0040630c => {
                fm::<0, 4>(c, ACC, 5, b4(c.vf[5][0]));
                l = 0x00406310;
                continue;
            }
            0x00406310 => {
                fm::<2, 4>(c, 5, 24, b4(c.vf[5][2]));
                l = 0x00406314;
                continue;
            }
            0x00406314 => {
                fm::<0, 4>(c, ACC, 4, b4(c.vf[4][0]));
                l = 0x00406318;
                continue;
            }
            0x00406318 => {
                fm::<2, 4>(c, 4, 24, b4(c.vf[4][2]));
                l = 0x0040631c;
                continue;
            }
            0x0040631c => {
                fm::<0, 4>(c, ACC, 3, b4(c.vf[3][0]));
                l = 0x00406320;
                continue;
            }
            0x00406320 => {
                fm::<2, 4>(c, 3, 24, b4(c.vf[3][2]));
                l = 0x00406324;
                continue;
            }
            0x00406324 => {
                fm::<0, 4>(c, ACC, 2, b4(c.vf[2][0]));
                l = 0x00406328;
                continue;
            }
            0x00406328 => {
                fm::<2, 4>(c, 2, 24, b4(c.vf[2][2]));
                l = 0x0040632c;
                continue;
            }
            0x0040632c => {
                fm::<0, 4>(c, ACC, 1, b4(c.vf[1][0]));
                l = 0x00406330;
                continue;
            }
            0x00406330 => {
                fm::<2, 4>(c, 1, 24, b4(c.vf[1][2]));
                l = 0x00406334;
                continue;
            }
            0x00406334 => {
                c.r[T1] = add32(c.r[T9], c.r[T9]);
                l = 0x00406338;
                continue;
            }
            0x00406338 => {
                c.r[T0] = sx(k.hi(c, 0x00406338));
                l = 0x0040633c;
                continue;
            }
            0x0040633c => {
                c.r[T0] = add32(c.r[T0], k.lo(c, 0x0040633c) as u64);
                l = 0x00406340;
                continue;
            }
            0x00406340 => {
                c.r[T9] = add32(c.r[T9], c.r[T1]);
                l = 0x00406344;
                continue;
            }
            0x00406344 => {
                c.r[T0] = add32(c.r[T0], c.r[T9]);
                l = 0x00406348;
                continue;
            }
            0x00406348 => {
                uycpu::vu0::mov::<15>(c, 10, c.vf[1]);
                l = 0x0040634c;
                continue;
            }
            0x0040634c => {
                let t = c.r[T0] as u32;
                c.r[T9] = 0;
                l = t.wrapping_sub(k.at).wrapping_add(k.l1);
                continue;
            }
            0x00406354 => {
                fm::<1, 4>(c, 11, 9, c.vf[10]);
                l = 0x00406358;
                continue;
            }
            0x00406358 => {
                (c.r[T0], c.rh[T0]) = q4(c.vf[11]);
                l = 0x0040635c;
                continue;
            }
            0x0040635c => {
                let t = (c.r[T0] as i64) >= 0;
                if t {
                    l = 0x0040636c;
                    continue;
                }
                c.r[T9] = 0u64 | 0x8;
                uycpu::vu0::mov::<15>(c, 10, c.vf[9]);
                l = 0x0040636c;
                continue;
            }
            0x0040636c => {
                fm::<1, 4>(c, 11, 8, c.vf[10]);
                (c.r[T0], c.rh[T0]) = q4(c.vf[11]);
                let t = (c.r[T0] as i64) >= 0;
                if t {
                    l = 0x00406384;
                    continue;
                }
                c.r[T9] = 0u64 | 0x7;
                uycpu::vu0::mov::<15>(c, 10, c.vf[8]);
                l = 0x00406384;
                continue;
            }
            0x00406384 => {
                fm::<1, 4>(c, 11, 7, c.vf[10]);
                (c.r[T0], c.rh[T0]) = q4(c.vf[11]);
                let t = (c.r[T0] as i64) >= 0;
                if t {
                    l = 0x0040639c;
                    continue;
                }
                c.r[T9] = 0u64 | 0x6;
                uycpu::vu0::mov::<15>(c, 10, c.vf[7]);
                l = 0x0040639c;
                continue;
            }
            0x0040639c => {
                fm::<1, 4>(c, 11, 6, c.vf[10]);
                (c.r[T0], c.rh[T0]) = q4(c.vf[11]);
                let t = (c.r[T0] as i64) >= 0;
                if t {
                    l = 0x004063b4;
                    continue;
                }
                c.r[T9] = 0u64 | 0x5;
                uycpu::vu0::mov::<15>(c, 10, c.vf[6]);
                l = 0x004063b4;
                continue;
            }
            0x004063b4 => {
                fm::<1, 4>(c, 11, 5, c.vf[10]);
                (c.r[T0], c.rh[T0]) = q4(c.vf[11]);
                let t = (c.r[T0] as i64) >= 0;
                if t {
                    l = 0x004063cc;
                    continue;
                }
                c.r[T9] = 0u64 | 0x4;
                uycpu::vu0::mov::<15>(c, 10, c.vf[5]);
                l = 0x004063cc;
                continue;
            }
            0x004063cc => {
                fm::<1, 4>(c, 11, 4, c.vf[10]);
                (c.r[T0], c.rh[T0]) = q4(c.vf[11]);
                let t = (c.r[T0] as i64) >= 0;
                if t {
                    l = 0x004063e4;
                    continue;
                }
                c.r[T9] = 0u64 | 0x3;
                uycpu::vu0::mov::<15>(c, 10, c.vf[4]);
                l = 0x004063e4;
                continue;
            }
            0x004063e4 => {
                fm::<1, 4>(c, 11, 3, c.vf[10]);
                (c.r[T0], c.rh[T0]) = q4(c.vf[11]);
                let t = (c.r[T0] as i64) >= 0;
                if t {
                    l = 0x004063fc;
                    continue;
                }
                c.r[T9] = 0u64 | 0x2;
                uycpu::vu0::mov::<15>(c, 10, c.vf[3]);
                l = 0x004063fc;
                continue;
            }
            0x004063fc => {
                fm::<1, 4>(c, 11, 2, c.vf[10]);
                (c.r[T0], c.rh[T0]) = q4(c.vf[11]);
                let t = (c.r[T0] as i64) >= 0;
                if t {
                    l = 0x00406414;
                    continue;
                }
                c.r[T9] = 0u64 | 0x1;
                uycpu::vu0::mov::<15>(c, 10, c.vf[2]);
                l = 0x00406414;
                continue;
            }
            0x00406414 => {
                c.r[A0] = 0x0000000070000000;
                c.r[A0] = c.r[A0] | 0x1fe0;
                c.r[A1] = sll(c.r[T9], 5);
                c.r[A1] = add32(c.r[A1], c.r[T5]);
                c.r[A2] = add32(c.r[A1], 0x00000020);
                c.r[A3] = add32(c.r[T5], 0xffffffe0);
                c.r[RA] = k.pc(0x00406434) as u64;
                sqc2(c, 23, (c.r[T5] as u32).wrapping_add(0xffffffe0));
                f4059f8(c);
                let t = c.r[T9] != 0u64;
                c.r[A2] = add32(c.r[A2], 0xffffffc0);
                if t {
                    l = 0x00406448;
                    continue;
                }
                c.r[A2] = add32(c.r[T7], 0xffffffff);
                c.r[A2] = sll(c.r[A2], 5);
                c.r[A2] = add32(c.r[A2], c.r[T5]);
                l = 0x00406448;
                continue;
            }
            0x00406448 => {
                lqc2(c, 21, c.r[A0] as u32);
                c.r[A0] = add32(c.r[A0], 0x00000010);
                c.f[1] = c.f[0];
                fm::<0, 4>(c, 8, 0, b4(c.vf[8][0]));
                c.r[RA] = k.pc(0x00406460) as u64;
                f4059f8(c);
                lqc2(c, 22, c.r[A0] as u32);
                fm::<1, 14>(c, 21, 21, c.vf[23]);
                fm::<6, 14>(c, 21, 21, c.vf[21]);
                fm::<0, 8>(c, ACC, 21, b4(c.vf[21][1]));
                fm::<2, 8>(c, 21, 24, b4(c.vf[21][2]));
                uycpu::vu0::qop::<1>(c, c.vf[0][0], c.vf[21][0]);
                fm::<0, 4>(c, 21, 0, b4(c.vi[22]));
                fm::<1, 14>(c, 22, 22, c.vf[23]);
                fm::<6, 14>(c, 22, 22, c.vf[22]);
                fm::<0, 8>(c, ACC, 22, b4(c.vf[22][1]));
                fm::<2, 8>(c, 22, 24, b4(c.vf[22][2]));
                uycpu::vu0::qop::<1>(c, c.vf[0][0], c.vf[22][0]);
                fm::<1, 12>(c, 7, 8, b4(c.vf[30][0]));
                fm::<1, 12>(c, 6, 8, b4(c.vf[0][3]));
                (c.r[T1], c.rh[T1]) = q4(c.vf[8]);
                (c.r[T4], c.rh[T4]) = q4(c.vf[7]);
                (c.r[T3], c.rh[T3]) = q4(c.vf[6]);
                fm::<1, 4>(c, 22, 21, b4(c.vi[22]));
                let t = (c.r[T4] as i64) > 0;
                (c.r[T5], c.rh[T5]) = q4(c.vf[22]);
                if t {
                    l = 0x004064d0;
                    continue;
                }
                c.r[T0] = c.r[T4] << 32;
                c.r[A2] = c.r[T7];
                let t = (c.r[T0] as i64) > 0;
                (c.r[A0], c.rh[A0]) = mmi::pexew((c.r[T8], c.rh[T8]));
                if t {
                    l = 0x004064d0;
                    continue;
                }
                c.r[A1] = c.r[T9];
                l = 0x004065a8;
                continue;
            }
            0x004064d0 => {
                let t = (c.r[T5] as i64) > 0;
                c.r[A1] = add32(c.r[T9], 0x00000001);
                if t {
                    l = 0x004064f0;
                    continue;
                }
                let t = (c.r[T3] as i64) < 0;
                (c.r[A0], c.rh[A0]) = mmi::pexew((c.r[T8], c.rh[T8]));
                if t {
                    l = 0x004064f0;
                    continue;
                }
                let t = c.r[A1] != c.r[T7];
                c.r[A2] = c.r[T7];
                if t {
                    l = 0x004065a8;
                    continue;
                }
                c.r[A1] = 0;
                l = 0x004065a8;
                continue;
            }
            0x004064f0 => {
                let t = (c.r[T5] as i64) < 0;
                c.r[T0] = c.r[T3] << 32;
                if t {
                    l = 0x00406518;
                    continue;
                }
                let t = (c.r[T0] as i64) < 0;
                (c.r[A0], c.rh[A0]) = mmi::pexew((c.r[T8], c.rh[T8]));
                if t {
                    l = 0x00406518;
                    continue;
                }
                let t = c.r[T9] != 0u64;
                c.r[A1] = add32(c.r[T9], 0xffffffff);
                if t {
                    l = 0x004065a8;
                    continue;
                }
                c.r[A2] = c.r[T7];
                c.r[A1] = add32(c.r[T7], 0xffffffff);
                l = 0x004065a8;
                continue;
            }
            0x00406518 => {
                let t = (c.r[T5] as i64) > 0;
                (c.r[A0], c.rh[A0]) = mmi::pexew((c.r[T8], c.rh[T8]));
                if t {
                    l = 0x00406558;
                    continue;
                }
                let t = (c.r[T1] as i64) <= 0;
                c.r[A1] = sll(c.r[T9], 5);
                if t {
                    l = 0x00406558;
                    continue;
                }
                c.r[A1] = add32(c.r[A1], c.r[T8]);
                c.f[12] = c.f[1];
                c.r[RA] = k.pc(0x00406538) as u64;
                c.r[A2] = add32(c.r[A1], 0x00000020);
                f405938(c);
                c.r[A0] = add32(c.r[A0], 0x00000020);
                c.r[A1] = add32(c.r[T9], 0x00000001);
                let t = c.r[A1] == c.r[T7];
                if t {
                    c.r[A1] = 0;
                    l = 0x00406548;
                    continue;
                }
                l = 0x00406548;
                continue;
            }
            0x00406548 => {
                c.r[A2] = c.r[T7];
                c.r[T7] = add32(c.r[T7], 0x00000001);
                l = 0x004065a8;
                continue;
            }
            0x00406558 => {
                let t = (c.r[T5] as i64) < 0;
                (c.r[A0], c.rh[A0]) = mmi::pexew((c.r[T8], c.rh[T8]));
                if t {
                    l = 0x004065f0;
                    continue;
                }
                c.r[T0] = c.r[T1] << 32;
                let t = (c.r[T0] as i64) <= 0;
                c.r[A1] = sll(c.r[T9], 5);
                if t {
                    l = 0x004065f0;
                    continue;
                }
                c.f[12] = c.f[0];
                c.r[A1] = add32(c.r[A1], c.r[T8]);
                let t = c.r[T9] != 0u64;
                c.r[A2] = add32(c.r[A1], 0xffffffe0);
                if t {
                    l = 0x00406590;
                    continue;
                }
                c.r[A2] = add32(c.r[T7], 0xffffffff);
                c.r[A2] = sll(c.r[A2], 5);
                c.r[A2] = add32(c.r[A2], c.r[T8]);
                l = 0x00406590;
                continue;
            }
            0x00406590 => {
                c.r[RA] = k.pc(0x00406598) as u64;
                c.r[T7] = add32(c.r[T7], 0x00000001);
                f405938(c);
                c.r[A2] = add32(c.r[T7], 0xffffffff);
                c.r[A0] = add32(c.r[A0], 0x00000020);
                c.r[A1] = c.r[T9];
                l = 0x004065a8;
                continue;
            }
            0x004065a8 => {
                c.r[T9] = c.r[A2];
                c.r[T1] = sll(c.r[A1], 5);
                c.r[T1] = add32(c.r[T1], c.r[T8]);
                c.r[T2] = c.r[A0];
                l = 0x004065b8;
                continue;
            }
            0x004065b8 => {
                (c.r[T3], c.rh[T3]) = c.rd128(c.r[T1] as u32);
                (c.r[T4], c.rh[T4]) = c.rd128((c.r[T1] as u32).wrapping_add(0x00000010));
                c.wr128(c.r[T2] as u32, c.r[T3], c.rh[T3]);
                c.wr128((c.r[T2] as u32).wrapping_add(0x00000010), c.r[T4], c.rh[T4]);
                c.r[T1] = add32(c.r[T1], 0x00000020);
                c.r[T2] = add32(c.r[T2], 0x00000020);
                c.r[T9] = add32(c.r[T9], 0xffffffff);
                let t = c.r[T9] == 0u64;
                if t {
                    (c.r[T8], c.rh[T8]) = mmi::pexew((c.r[T8], c.rh[T8]));
                    l = 0x004065f0;
                    continue;
                }
                let t = c.r[T9] != c.r[A1];
                if t {
                    l = 0x004065b8;
                    continue;
                }
                c.r[T1] = c.r[T8];
                l = 0x004065b8;
                continue;
            }
            0x004065f0 => {
                n[8] += 1;
                c.r[A0] = add32(c.r[T7], c.r[T7]);
                c.r[A0] = add32(c.r[A0], c.r[T7]);
                c.r[A0] = srl(c.r[A0], 1);
                c.r[RA] = k.pc(0x00406604) as u64;
                c.r[A0] = add32(c.r[A0], 0x00000003);
                f406098(c, k.call(c, 0x004065fc, 0x40_6098));
                c.r[T0] = 0x0000000044000000;
                c.r[T1] = 0u64 | 0x1;
                c.wr32((c.r[AT] as u32).wrapping_add(0x00000004), c.r[T0] as u32);
                c.r[T2] = 0u64 | 0x4120;
                c.wr32(c.r[AT] as u32, c.r[T1] as u32);
                c.r[T0] = 0u64 | 0x7d;
                c.wr64((c.r[AT] as u32).wrapping_add(0x00000008), c.r[T2]);
                c.r[T1] = 0u64 | 0x412;
                c.wr64((c.r[AT] as u32).wrapping_add(0x00000010), c.r[T0]);
                c.r[T2] = 0x0000000034000000;
                c.wr64((c.r[AT] as u32).wrapping_add(0x00000038), c.r[T1]);
                c.r[T0] = c.r[T7] | 0x8000;
                c.wr32((c.r[AT] as u32).wrapping_add(0x00000034), c.r[T2] as u32);
                c.r[T0] = add32(c.r[T0], 0xffffffff);
                c.wr32((c.r[AT] as u32).wrapping_add(0x00000030), c.r[T0] as u32);
                c.r[AT] = add32(c.r[AT], 0x00000018);
                c.r[T9] = sx(0x00000028);
                c.r[T4] = c.r[T8];
                l = 0x0040664c;
                continue;
            }
            0x0040664c => {
                lqc2(c, 1, c.r[T4] as u32);
                fm::<6, 15>(c, 1, 1, c.vf[27]);
                uycpu::vu0::qop::<0>(c, c.vf[31][0], c.vf[1][3]);
                lqc2(c, 2, (c.r[T4] as u32).wrapping_add(0x00000010));
                fm::<6, 1>(c, 2, 0, b4(c.vf[0][3]));
                c.r[T4] = add32(c.r[T4], 0x00000020);
                fm::<6, 15>(c, 1, 1, b4(c.vi[22]));
                fm::<6, 13>(c, 2, 2, b4(c.vi[22]));
                fm::<0, 15>(c, 1, 1, c.vf[28]);
                uycpu::vu0::ftoi::<1, 15>(c, 1, 1);
                (c.r[T0], c.rh[T0]) = q4(c.vf[2]);
                c.wr64(c.r[AT] as u32, c.r[T0]);
                (c.r[T1], c.rh[T1]) = mmi::pcpyud((c.r[T0], c.rh[T0]), (0u64, 0u64));
                c.wr64((c.r[AT] as u32).wrapping_add(0x00000008), c.r[T1]);
                (c.r[T1], c.rh[T1]) = q4(c.vf[1]);
                (c.r[T2], c.rh[T2]) = mmi::ppach((c.r[T1], c.rh[T1]), (c.r[T1], c.rh[T1]));
                (c.r[T1], c.rh[T1]) = mmi::pcpyud((c.r[T1], c.rh[T1]), (0u64, 0u64));
                (c.r[T1], c.rh[T1]) = mmi::psraw((c.r[T1], c.rh[T1]), 4);
                c.wr32((c.r[AT] as u32).wrapping_add(0x00000010), c.r[T2] as u32);
                c.wr32((c.r[AT] as u32).wrapping_add(0x00000014), c.r[T1] as u32);
                c.r[T1] = ((c.r[T1] as i64) >> 32) as u64;
                c.r[T3] = sx(0x000000ff);
                (c.r[T1], c.rh[T1]) = mmi::pminw((c.r[T1], c.rh[T1]), (c.r[T3], c.rh[T3]));
                (c.r[T1], c.rh[T1]) = mmi::pmaxw((c.r[T1], c.rh[T1]), (0u64, 0u64));
                c.wr8((c.r[AT] as u32).wrapping_add(0x00000017), c.r[T1] as u8);
                c.r[AT] = add32(c.r[AT], c.r[T9]);
                c.r[T7] = add32(c.r[T7], 0xffffffff);
                let t = c.r[T7] != 0u64;
                c.r[T9] = sx(0x00000018);
                if t {
                    l = 0x0040664c;
                    continue;
                }
                l = 0x004066c4;
                continue;
            }
            0x004066c4 => {
                (c.r[RA], c.rh[RA]) = mmi::pexew((c.r[RA], c.rh[RA]));
                return;
            }
            _ => panic!("the sprite unit's 0x4060D8: a jump to level 1's {l:08x}, which is not one of its labels"),
        }
    }
}

fn f4057c0(c: &mut Cpu) {
    let l: u32 = 0x004057c0;
    loop {
        match l {
            0x004057c0 => {
                c.r[T1] = srl(c.r[A3], 1);
                c.f[0] = c.rd32((c.r[A1] as u32).wrapping_add(0x0000000c));
                c.r[T1] = sll(c.r[T1], 2);
                c.f[1] = c.rd32((c.r[A2] as u32).wrapping_add(0x0000000c));
                c.r[T0] = add32(c.r[A1], c.r[T1]);
                c.f[5] = c.rd32((c.r[A1] as u32).wrapping_add(0x00000010));
                c.f[1] = uycpu::fpu::sub(c.f[1], c.f[0]);
                c.r[T0] = sx(c.rd32(c.r[T0] as u32));
                c.r[T1] = add32(c.r[A2], c.r[T1]);
                c.f[4] = c.rd32((c.r[A2] as u32).wrapping_add(0x00000010));
                c.r[A3] = sll(c.r[A3], 31);
                c.r[T1] = sx(c.rd32(c.r[T1] as u32));
                c.r[T0] = c.r[T0] ^ c.r[A3];
                c.f[4] = uycpu::fpu::sub(c.f[4], c.f[5]);
                c.f[2] = c.r[T0] as u32;
                c.r[T1] = c.r[T1] ^ c.r[A3];
                c.f[3] = c.r[T1] as u32;
                c.f[0] = uycpu::fpu::sub(c.f[2], c.f[0]);
                c.r[T1] = sx(c.rd32((c.r[A1] as u32).wrapping_add(0x00000018)));
                c.f[3] = uycpu::fpu::sub(c.f[2], c.f[3]);
                (c.r[T1], c.rh[T1]) = mmi::pextlb((0u64, 0u64), (c.r[T1], c.rh[T1]));
                c.r[T0] = sx(c.rd32((c.r[A2] as u32).wrapping_add(0x00000018)));
                (c.r[T1], c.rh[T1]) = mmi::pextlh((0u64, 0u64), (c.r[T1], c.rh[T1]));
                c.f[2] = c.rd32((c.r[A1] as u32).wrapping_add(0x00000014));
                (c.r[T0], c.rh[T0]) = mmi::pextlb((0u64, 0u64), (c.r[T0], c.rh[T0]));
                qmtc2(c, 3, (c.r[T1], c.rh[T1]));
                c.f[1] = uycpu::fpu::add(c.f[1], c.f[3]);
                (c.r[T0], c.rh[T0]) = mmi::pextlh((0u64, 0u64), (c.r[T0], c.rh[T0]));

                uycpu::vu0::itof::<0, 15>(c, 3, 3);
                c.f[0] = uycpu::fpu::div(c, c.f[0], c.f[1]);
                qmtc2(c, 4, (c.r[T0], c.rh[T0]));
                c.f[3] = c.rd32((c.r[A2] as u32).wrapping_add(0x00000014));
                uycpu::vu0::itof::<0, 15>(c, 4, 4);
                c.f[3] = uycpu::fpu::sub(c.f[3], c.f[2]);
                lqc2(c, 2, c.r[A2] as u32);
                c.f[4] = uycpu::fpu::mul(c.f[4], c.f[0]);
                lqc2(c, 1, c.r[A1] as u32);
                c.r[T0] = c.f[0] as i32 as i64 as u64;
                qmtc2(c, 5, (c.r[T0], c.rh[T0]));
                c.f[3] = uycpu::fpu::mul(c.f[3], c.f[0]);
                fm::<6, 15>(c, ACC, 3, b4(c.vf[0][3]));
                fm::<2, 15>(c, ACC, 4, b4(c.vf[5][0]));
                fm::<3, 15>(c, 3, 3, b4(c.vf[5][0]));
                fm::<6, 15>(c, ACC, 1, b4(c.vf[0][3]));
                fm::<2, 15>(c, ACC, 2, b4(c.vf[5][0]));
                c.f[4] = uycpu::fpu::add(c.f[4], c.f[5]);
                uycpu::vu0::ftoi::<1, 15>(c, 3, 3);
                c.f[3] = uycpu::fpu::add(c.f[3], c.f[2]);
                fm::<3, 15>(c, 2, 1, b4(c.vf[5][0]));
                c.wr32((c.r[A0] as u32).wrapping_add(0x00000010), c.f[4]);
                (c.r[T1], c.rh[T1]) = q4(c.vf[3]);
                c.r[T0] = 0u64 | 0x2;
                c.wr32((c.r[A0] as u32).wrapping_add(0x00000014), c.f[3]);
                (c.r[T0], c.rh[T0]) = mmi::pcpyh((c.r[T0], c.rh[T0]));
                (c.r[T1], c.rh[T1]) = mmi::ppach((0u64, 0u64), (c.r[T1], c.rh[T1]));
                sqc2(c, 2, c.r[A0] as u32);
                (c.r[T1], c.rh[T1]) = mmi::paddh((c.r[T1], c.rh[T1]), (c.r[T0], c.rh[T0]));
                (c.r[T1], c.rh[T1]) = mmi::psrlh((c.r[T1], c.rh[T1]), 4);
                (c.r[T1], c.rh[T1]) = mmi::ppacb((0u64, 0u64), (c.r[T1], c.rh[T1]));
                c.wr32((c.r[A0] as u32).wrapping_add(0x00000018), c.r[T1] as u32);
                return;
            }
            _ => unreachable!("0x4057C0: no block at {l:08x}"),
        }
    }
}

fn f405938(c: &mut Cpu) {
    let l: u32 = 0x00405938;
    loop {
        match l {
            0x00405938 => {
                c.r[T0] = sx(c.rd32((c.r[A1] as u32).wrapping_add(0x00000018)));
                c.r[T2] = c.f[12] as i32 as i64 as u64;
                (c.r[T0], c.rh[T0]) = mmi::pextlb((0u64, 0u64), (c.r[T0], c.rh[T0]));
                c.r[T1] = sx(c.rd32((c.r[A2] as u32).wrapping_add(0x00000018)));
                (c.r[T0], c.rh[T0]) = mmi::pextlh((0u64, 0u64), (c.r[T0], c.rh[T0]));
                qmtc2(c, 1, (c.r[T2], c.rh[T2]));
                qmtc2(c, 5, (c.r[T0], c.rh[T0]));
                (c.r[T1], c.rh[T1]) = mmi::pextlb((0u64, 0u64), (c.r[T1], c.rh[T1]));
                (c.r[T1], c.rh[T1]) = mmi::pextlh((0u64, 0u64), (c.r[T1], c.rh[T1]));
                qmtc2(c, 3, (c.r[T1], c.rh[T1]));
                uycpu::vu0::itof::<0, 15>(c, 5, 5);
                fm::<1, 1>(c, 6, 0, b4(c.vf[1][0]));
                uycpu::vu0::itof::<0, 15>(c, 3, 3);
                lqc2(c, 2, c.r[A1] as u32);
                fm::<6, 15>(c, ACC, 5, b4(c.vf[6][3]));
                fm::<2, 15>(c, 5, 3, b4(c.vf[1][0]));
                lqc2(c, 3, c.r[A2] as u32);
                fm::<6, 15>(c, ACC, 2, b4(c.vf[6][3]));
                fm::<2, 15>(c, 2, 3, b4(c.vf[1][0]));
                uycpu::vu0::ftoi::<1, 15>(c, 5, 5);
                lqc2(c, 3, (c.r[A1] as u32).wrapping_add(0x00000010));
                lqc2(c, 4, (c.r[A2] as u32).wrapping_add(0x00000010));
                fm::<6, 12>(c, ACC, 3, b4(c.vf[6][3]));
                c.r[T0] = 0u64 | 0x2;
                fm::<2, 12>(c, 3, 4, b4(c.vf[1][0]));
                (c.r[T0], c.rh[T0]) = mmi::pcpyh((c.r[T0], c.rh[T0]));
                (c.r[T1], c.rh[T1]) = q4(c.vf[5]);
                (c.r[T1], c.rh[T1]) = mmi::ppach((0u64, 0u64), (c.r[T1], c.rh[T1]));
                (c.r[T1], c.rh[T1]) = mmi::paddh((c.r[T1], c.rh[T1]), (c.r[T0], c.rh[T0]));
                (c.r[T1], c.rh[T1]) = mmi::psrlh((c.r[T1], c.rh[T1]), 4);
                sqc2(c, 3, (c.r[A0] as u32).wrapping_add(0x00000010));
                (c.r[T1], c.rh[T1]) = mmi::ppacb((0u64, 0u64), (c.r[T1], c.rh[T1]));
                sqc2(c, 2, c.r[A0] as u32);
                c.wr32((c.r[A0] as u32).wrapping_add(0x00000018), c.r[T1] as u32);
                return;
            }
            _ => unreachable!("0x405938: no block at {l:08x}"),
        }
    }
}

fn f4059f8(c: &mut Cpu) {
    let l: u32 = 0x004059f8;
    loop {
        match l {
            0x004059f8 => {
                fm::<0, 14>(c, 24, 0, b4(c.vf[0][3]));
                lqc2(c, 1, c.r[A1] as u32);
                lqc2(c, 2, c.r[A2] as u32);
                lqc2(c, 3, c.r[A3] as u32);
                fm::<1, 14>(c, 4, 2, c.vf[1]);
                fm::<1, 14>(c, 5, 3, c.vf[1]);
                fm::<6, 14>(c, 6, 4, c.vf[4]);
                fm::<6, 14>(c, 7, 4, c.vf[5]);
                fm::<0, 8>(c, ACC, 6, b4(c.vf[6][1]));
                fm::<2, 8>(c, 6, 24, b4(c.vf[6][2]));
                fm::<0, 8>(c, ACC, 7, b4(c.vf[7][1]));
                fm::<2, 8>(c, 7, 24, b4(c.vf[7][2]));
                uycpu::vu0::qop::<0>(c, c.vf[7][0], c.vf[6][0]);
                fm::<0, 8>(c, 8, 0, b4(c.vi[22]));
                fm::<5, 8>(c, 7, 8, b4(c.vf[0][3]));
                (c.r[T0], c.rh[T0]) = q4(c.vf[8]);
                fm::<4, 8>(c, 7, 7, b4(c.vf[0][0]));
                c.f[0] = c.r[T0] as u32;
                fm::<6, 14>(c, ACC, 4, b4(c.vf[7][0]));
                fm::<2, 14>(c, 1, 1, b4(c.vf[0][3]));
                sqc2(c, 1, c.r[A0] as u32);
                return;
            }
            _ => unreachable!("0x4059F8: no block at {l:08x}"),
        }
    }
}
