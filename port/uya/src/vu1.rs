//! VU1 as the port's own VU1 programs run on it: programs 0 (the mobies' and the stars', `vu1p0.rs`), 5 (`vu1p5.rs`) and 6
//! (the effects', `vu1p6.rs`), written from the game's microcode instruction by instruction. The renderer runs them on VU1
//! state of its own (the moby chain's runs it replays, the front end's chains, the effect pass) through `run`.
//!
//! # The state and the arithmetic
//!
//! VU1's registers as the game's programs leave them from one run to the next: VF (vf00 1.0 in w), ACC, VI (vi00 0, vi01 to
//! vi15 16 bits; 16 the status flag, 17 the MAC flags, 18 the clip flag, 21 I, 22 Q, 23 P), its 16 KB of data memory (addresses
//! wrap at 0x400 quadwords) and the TOP the running program reads (XTOP). An XGKICK is kept (`kicks`) with the packet the GIF
//! got from it (`sent`, read at the kick, or with `defer_packets` at the next kick or `finish`: program 6 kicks a packet and then
//! writes its end).
//!
//! The arithmetic is the host's single precision under the game's MXCSR (toward zero, DAZ, FTZ; `run` sets it): an operand whose
//! exponent is 255 is read as the largest number of its sign, then the SSE operation is the VU's. MAX and MINI keep the chosen
//! operand's bits and set no flags. Every other FMAC result's MAC flags (zero and sign of each lane written) are kept as the run
//! goes (`mac`: the last result's, `sticky`: their union): as a run ends the MAC flag register holds the last result's and the
//! status flag its zero and sign (bits 0 and 1) and, sticky, their union's (bits 6 and 7), as on the VU, which lands a result's
//! flags four cycles after its instruction (nothing the programs read before then is changed by it: where a program reads the
//! MAC flags, FSAND, its code names the result it sees).
//!
//! # The pipeline
//!
//! Each program is written as the VU runs it once its register interlocks have done their work: in program order, the upper and
//! lower instruction of a pair both reading the registers as they were before the pair (the upper's result is put last, and wins
//! a write both make), a branch's delay slot run either way. Where the VU's timing decides a value the code says so: Q (a DIV
//! lands 7 cycles after it issues; every read of Q in these programs comes after the DIV it reads has landed and before the next
//! has, so Q is written as the DIV issues, after the pair's upper has read the old one), the clip flag (a CLIP's judgement is
//! shifted in four cycles after the CLIP; every read of it comes after the judgements it reads), the MAC flags FSAND reads, and
//! the branch hazard (a branch or jump after a pair that changed its VI register reads the value from before that pair).
//!
//! # The check
//!
//! `UYA_VU1_CHECK=N`: the first N runs of each program are run again on the translation of the same microcode the port's
//! translator made (`tr_vu1`), from the same VU1 state and memory, and everything each leaves (data memory, VF, ACC, VI, TOP,
//! the kicks and their packets, where the run ended) is compared: one line per difference, and a summary of the runs checked
//! and differing per program when N of a program have been checked and as the process ends. Without it only the port's code
//! runs.

use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) type V = [u32; 4];

/// ACC as an FMAC's destination.
pub(crate) const ACC: usize = 32;
/// Dest fields (bit 3 = x).
pub(crate) const X: u32 = 8;
pub(crate) const Y: u32 = 4;
pub(crate) const Z: u32 = 2;
pub(crate) const W: u32 = 1;
pub(crate) const XY: u32 = 12;
pub(crate) const XZ: u32 = 10;
pub(crate) const XW: u32 = 9;
pub(crate) const YW: u32 = 5;
pub(crate) const ZW: u32 = 3;
pub(crate) const XYZ: u32 = 14;
pub(crate) const XYW: u32 = 13;
pub(crate) const XYZW: u32 = 15;

/// The special registers' places in `vi`.
pub(crate) const STATUS: usize = 16;
pub(crate) const MACF: usize = 17;
pub(crate) const CLIPF: usize = 18;
pub(crate) const I: usize = 21;
pub(crate) const Q: usize = 22;

/// A word on all four lanes (an FMAC's broadcast field, Q or I).
#[inline(always)]
pub(crate) fn bc(x: u32) -> V {
    [x; 4]
}

/// A VI register as a branch compares it: its 16 bits, signed.
#[inline(always)]
pub(crate) fn s16(x: u32) -> i16 {
    x as u16 as i16
}

/// MFIR's value: a VI register sign-extended to 32 bits.
#[inline(always)]
pub(crate) fn sx(x: u32) -> u32 {
    x as u16 as i16 as i32 as u32
}

pub(crate) struct Vu1 {
    pub vf: [V; 32],
    pub acc: V,
    pub vi: [u32; 32],
    pub mem: Box<[V; 1024]>,
    pub top: u32,
    pub kicks: Vec<u32>,
    pub sent: Vec<Result<Vec<V>, String>>,
    /// Capture each kicked packet (program 6's consumer reads its completed buffer after the run instead).
    pub capture_packets: bool,
    /// With `capture_packets`: a kicked packet is read at the next XGKICK or `finish`, not at its own.
    pub defer_packets: bool,
    pending: Option<u32>,
    /// This run's MAC flags: the last FMAC result's, the union of all of them, whether there was one.
    mac: u32,
    sticky: u32,
    macs: bool,
}

impl Default for Vu1 {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for Vu1 {
    fn clone(&self) -> Self {
        Vu1 { vf: self.vf, acc: self.acc, vi: self.vi, mem: self.mem.clone(), top: self.top, kicks: self.kicks.clone(), sent: self.sent.clone(), capture_packets: self.capture_packets, defer_packets: self.defer_packets, pending: self.pending, mac: self.mac, sticky: self.sticky, macs: self.macs }
    }
}

impl Vu1 {
    /// A VU1 that has run nothing: every register 0 but vf00's w (1.0), memory 0.
    pub fn new() -> Vu1 {
        let mut vf = [[0u32; 4]; 32];
        vf[0] = [0, 0, 0, 0x3f80_0000];
        Vu1 { vf, acc: [0; 4], vi: [0; 32], mem: Box::new([[0; 4]; 1024]), top: 0, kicks: Vec::new(), sent: Vec::new(), capture_packets: true, defer_packets: false, pending: None, mac: 0, sticky: 0, macs: false }
    }

    /// XGKICK at `a`: the address onto `kicks`, the packet as data memory holds it now onto `sent`.
    pub fn kick(&mut self, a: u32) {
        let a = a & 0x3ff;
        self.kicks.push(a);
        if self.capture_packets && self.defer_packets {
            if let Some(b) = self.pending.replace(a) {
                let p = self.packet(b);
                self.sent.push(p);
            }
        } else if self.capture_packets {
            let p = self.packet(a);
            self.sent.push(p);
        }
    }

    /// The run's end with `defer_packets`: its last kicked packet onto `sent`, as data memory holds it now.
    pub fn finish(&mut self) {
        if let Some(b) = self.pending.take() {
            let p = self.packet(b);
            self.sent.push(p);
        }
    }

    /// The GIF packet an XGKICK at `a` sends, as the GIF reads it from data memory: each tag and its data (PACKED NLOOP x NREG
    /// quadwords, REGLIST half that rounded up, IMAGE NLOOP) up to the tag with EOP, addresses wrapping at the end of memory.
    pub fn packet(&self, a: u32) -> Result<Vec<V>, String> {
        let mut a = a as usize & 0x3ff;
        let mut out = Vec::new();
        loop {
            let t = self.mem[a];
            let nloop = (t[0] & 0x7fff) as usize;
            let nreg = match (t[1] >> 28) & 15 {
                0 => 16,
                n => n as usize,
            };
            let len = match (t[1] >> 26) & 3 {
                0 => nloop * nreg,
                1 => (nloop * nreg).div_ceil(2),
                _ => nloop,
            };
            out.extend((0..=len).map(|k| self.mem[(a + k) & 0x3ff]));
            if out.len() > 1024 {
                return Err(format!("VU1: the GIF packet an XGKICK sent from {a:#x} reads past all of data memory without a tag with EOP"));
            }
            a = (a + len + 1) & 0x3ff;
            if t[0] >> 15 & 1 != 0 {
                return Ok(out);
            }
        }
    }

    /// A VI write: vi00 stays 0, vi01 to vi15 keep 16 bits.
    #[inline(always)]
    pub fn set_vi(&mut self, i: usize, v: u32) {
        if i != 0 {
            self.vi[i] = if i < 16 { v & 0xffff } else { v };
        }
    }

    /// The quadword at `a` (VI plus the offset; the address wraps at 0x400 quadwords).
    #[inline(always)]
    pub fn ld(&self, a: u32) -> V {
        self.mem[(a & 0x3ff) as usize]
    }

    /// LQ into the lanes `D` of vf`r`.
    #[inline(always)]
    pub fn lq<const D: u32>(&mut self, r: usize, a: u32) {
        let q = self.ld(a);
        if r != 0 {
            put::<D>(&mut self.vf[r], q);
        }
    }

    /// SQ of the lanes `D` of `v` to `a`.
    #[inline(always)]
    pub fn st<const D: u32>(&mut self, a: u32, v: V) {
        let m = &mut self.mem[(a & 0x3ff) as usize];
        for (l, w) in m.iter_mut().enumerate() {
            if D & (8 >> l) != 0 {
                *w = v[l];
            }
        }
    }

    /// ILW of lane `l` at `a` into vi`r`.
    #[inline(always)]
    pub fn ilw(&mut self, r: usize, a: u32, l: usize) {
        let w = self.ld(a)[l];
        self.set_vi(r, w & 0xffff);
    }

    /// ISW of vi`r` (16 bits) to the lanes `D` at `a`.
    #[inline(always)]
    pub fn isw<const D: u32>(&mut self, r: usize, a: u32) {
        let v = self.vi[r] & 0xffff;
        self.st::<D>(a, bc(v));
    }

    /// An FMAC result's flags kept for the run's end.
    #[inline(always)]
    fn flags<const D: u32>(&mut self, u: V) {
        let f = mac::<D>(u);
        self.mac = f;
        self.sticky |= f;
        self.macs = true;
    }

    /// The MAC flags of this run's last FMAC result (what FSAND reads when that result is the last to have landed).
    #[inline(always)]
    pub fn last_mac(&self) -> u32 {
        self.mac
    }

    /// FSAND vi`r`, 2 on the MAC flags `m` (the status flag's S: any lane negative).
    #[inline(always)]
    pub fn fsand_s(&mut self, r: usize, m: u32) {
        self.set_vi(r, (((m & 0xf0) != 0) as u32) << 1);
    }

    /// An FMAC result (K: 0 ADD, 1 SUB, 2 MADD, 3 MSUB, 4 MAX, 5 MINI, 6 MUL), its flags kept (MAX and MINI set none); the caller
    /// puts it (`put_to`).
    #[inline(always)]
    pub fn fm<const K: u8, const D: u32>(&mut self, s: V, t: V) -> V {
        let u = fmac::<K, D>(s, t, self.acc);
        if K != 4 && K != 5 {
            self.flags::<D>(u);
        }
        u
    }

    /// An upper instruction's result into the lanes `D` of vf`d` (ACC for `ACC`; vf00 is not written).
    #[inline(always)]
    pub fn put_to<const D: u32>(&mut self, d: usize, u: V) {
        if d == ACC {
            put::<D>(&mut self.acc, u);
        } else if d != 0 {
            put::<D>(&mut self.vf[d], u);
        }
    }

    #[inline(always)]
    pub fn add<const D: u32>(&mut self, d: usize, s: V, t: V) {
        let u = self.fm::<0, D>(s, t);
        self.put_to::<D>(d, u);
    }
    #[inline(always)]
    pub fn sub<const D: u32>(&mut self, d: usize, s: V, t: V) {
        let u = self.fm::<1, D>(s, t);
        self.put_to::<D>(d, u);
    }
    /// MADD: ACC + s t.
    #[inline(always)]
    pub fn madd<const D: u32>(&mut self, d: usize, s: V, t: V) {
        let u = self.fm::<2, D>(s, t);
        self.put_to::<D>(d, u);
    }
    /// MSUB: ACC - s t.
    #[inline(always)]
    pub fn msub<const D: u32>(&mut self, d: usize, s: V, t: V) {
        let u = self.fm::<3, D>(s, t);
        self.put_to::<D>(d, u);
    }
    #[inline(always)]
    pub fn max<const D: u32>(&mut self, d: usize, s: V, t: V) {
        let u = self.fm::<4, D>(s, t);
        self.put_to::<D>(d, u);
    }
    #[inline(always)]
    pub fn mini<const D: u32>(&mut self, d: usize, s: V, t: V) {
        let u = self.fm::<5, D>(s, t);
        self.put_to::<D>(d, u);
    }
    #[inline(always)]
    pub fn mul<const D: u32>(&mut self, d: usize, s: V, t: V) {
        let u = self.fm::<6, D>(s, t);
        self.put_to::<D>(d, u);
    }

    /// OPMULA: ACC.xyz = s.yzx t.zxy (its flags kept).
    #[inline(always)]
    pub fn opmula(&mut self, s: V, t: V) {
        use uyart::fpu::mul;
        let u = [mul(s[1], t[2]), mul(s[2], t[0]), mul(s[0], t[1]), 0];
        self.flags::<XYZ>(u);
        put::<XYZ>(&mut self.acc, u);
    }

    /// OPMSUB: vf`d`.xyz = ACC - s.yzx t.zxy.
    #[inline(always)]
    pub fn opmsub(&mut self, d: usize, s: V, t: V) {
        use uyart::fpu::{mul, sub};
        let a = self.acc;
        let u = [sub(a[0], mul(s[1], t[2])), sub(a[1], mul(s[2], t[0])), sub(a[2], mul(s[0], t[1])), 0];
        self.flags::<XYZ>(u);
        self.put_to::<XYZ>(d, u);
    }

    /// ITOF0, 4, 12, 15 (`N` 0 to 3) of `s` into the lanes `D` of vf`d`.
    #[inline(always)]
    pub fn itof<const N: usize, const D: u32>(&mut self, d: usize, s: V) {
        let u = itof::<N>(s);
        self.put_to::<D>(d, u);
    }

    /// FTOI0, 4, 12, 15 (`N` 0 to 3).
    #[inline(always)]
    pub fn ftoi<const N: usize, const D: u32>(&mut self, d: usize, s: V) {
        let u = ftoi::<N>(s);
        self.put_to::<D>(d, u);
    }

    /// DIV Q, a / b: Q from now (the status flag's D and I, and their sticky bits, set now).
    #[inline(always)]
    pub fn div(&mut self, a: u32, b: u32) {
        let (x, y) = (uyart::fpu::fin(a), uyart::fpu::fin(b));
        self.vi[Q] = if y == 0.0 {
            self.vi[STATUS] |= if x == 0.0 { 0x410 } else { 0x820 };
            ((a ^ b) & 0x8000_0000) | 0x7f7f_ffff
        } else {
            (x / y).to_bits()
        };
    }

    /// Q.
    #[inline(always)]
    pub fn q(&self) -> V {
        bc(self.vi[Q])
    }

    /// I.
    #[inline(always)]
    pub fn i(&self) -> V {
        bc(self.vi[I])
    }

    /// A CLIP's judgement shifted into the clip flag.
    #[inline(always)]
    pub fn clip_in(&mut self, j: u32) {
        self.vi[CLIPF] = ((self.vi[CLIPF] << 6) | j) & 0xff_ffff;
    }

    /// FCAND: whether the clip flag has any of `m`.
    #[inline(always)]
    pub fn fcand(&self, m: u32) -> u32 {
        ((self.vi[CLIPF] & m) != 0) as u32
    }

    /// FCOR: whether the clip flag has every bit `m` has not.
    #[inline(always)]
    pub fn fcor(&self, m: u32) -> u32 {
        (((self.vi[CLIPF] | m) & 0xff_ffff) == 0xff_ffff) as u32
    }

    /// A run's start: no MAC flags yet.
    fn begin(&mut self) {
        self.mac = 0;
        self.sticky = 0;
        self.macs = false;
    }

    /// A run's end: the MAC flag register and the status flag's zero and sign as the run's last result left them, sticky
    /// their union's.
    fn end(&mut self) {
        if self.macs {
            self.vi[MACF] = self.mac;
            let st = ((self.mac & 0xf != 0) as u32) | (((self.mac & 0xf0 != 0) as u32) << 1);
            let so = ((self.sticky & 0xf != 0) as u32) | (((self.sticky & 0xf0 != 0) as u32) << 1);
            self.vi[STATUS] = (self.vi[STATUS] & !0xf) | st | (so << 6);
        }
    }
}

/// The lanes of `D` of `v` into `r`.
#[inline(always)]
fn put<const D: u32>(r: &mut V, v: V) {
    for l in 0..4 {
        if D & (8 >> l) != 0 {
            r[l] = v[l];
        }
    }
}

/// The MAC flags of a result over the lanes of `D`: zero at bit 3 - l, sign at bit 7 - l.
#[inline(always)]
fn mac<const D: u32>(v: V) -> u32 {
    let mut f = 0;
    for l in 0..4 {
        if D & (8 >> l) != 0 {
            if v[l] & 0x7fff_ffff == 0 {
                f |= 8 >> l;
            }
            if v[l] & 0x8000_0000 != 0 {
                f |= 0x80 >> l;
            }
        }
    }
    f
}

/// The FMAC operations on every lane, SSE under the game's MXCSR; the lanes outside `D` are 0.
#[inline(always)]
fn fmac<const K: u8, const D: u32>(s: V, t: V, acc: V) -> V {
    use std::arch::x86_64::*;
    // SAFETY: SSE2 is the x86-64 baseline; the loads and stores are of four-word arrays.
    unsafe {
        let ld = |v: V| _mm_loadu_si128(v.as_ptr() as *const __m128i);
        let (si, ti) = (ld(s), ld(t));
        let (fs, ft) = (fin4(si), fin4(ti));
        let r = match K {
            0 => _mm_castps_si128(_mm_add_ps(fs, ft)),
            1 => _mm_castps_si128(_mm_sub_ps(fs, ft)),
            2 => _mm_castps_si128(_mm_add_ps(fin4(ld(acc)), _mm_mul_ps(fs, ft))),
            3 => _mm_castps_si128(_mm_sub_ps(fin4(ld(acc)), _mm_mul_ps(fs, ft))),
            4 => sel(_mm_castps_si128(_mm_cmpge_ps(fs, ft)), si, ti),
            5 => sel(_mm_castps_si128(_mm_cmple_ps(fs, ft)), si, ti),
            _ => _mm_castps_si128(_mm_mul_ps(fs, ft)),
        };
        let m = |l: usize| if D & (8 >> l) != 0 { -1i32 } else { 0 };
        let r = _mm_and_si128(r, _mm_setr_epi32(m(0), m(1), m(2), m(3)));
        let mut o = [0u32; 4];
        _mm_storeu_si128(o.as_mut_ptr() as *mut __m128i, r);
        o
    }
}

/// Every lane's bits where `m` is set, else `b`'s.
#[inline(always)]
unsafe fn sel(m: std::arch::x86_64::__m128i, a: std::arch::x86_64::__m128i, b: std::arch::x86_64::__m128i) -> std::arch::x86_64::__m128i {
    use std::arch::x86_64::*;
    unsafe { _mm_or_si128(_mm_and_si128(m, a), _mm_andnot_si128(m, b)) }
}

/// Four operands as the VU reads them: exponent 255 is the largest number of its sign (denormals are DAZ's).
#[inline(always)]
unsafe fn fin4(v: std::arch::x86_64::__m128i) -> std::arch::x86_64::__m128 {
    use std::arch::x86_64::*;
    unsafe {
        let e = _mm_and_si128(v, _mm_set1_epi32(0x7f80_0000));
        let big = _mm_cmpeq_epi32(e, _mm_set1_epi32(0x7f80_0000));
        if _mm_movemask_epi8(big) == 0 {
            return _mm_castsi128_ps(v);
        }
        let largest = _mm_or_si128(_mm_and_si128(v, _mm_set1_epi32(0x8000_0000u32 as i32)), _mm_set1_epi32(0x7f7f_ffff));
        _mm_castsi128_ps(sel(big, largest, v))
    }
}

const ITOF_SCALE: [f32; 4] = [1.0, 1.0 / 16.0, 1.0 / 4096.0, 1.0 / 32768.0];
const FTOI_SCALE: [f32; 4] = [1.0, 16.0, 4096.0, 32768.0];

#[inline(always)]
fn itof<const N: usize>(s: V) -> V {
    s.map(|x| ((x as i32 as f32) * ITOF_SCALE[N]).to_bits())
}

#[inline(always)]
fn ftoi<const N: usize>(s: V) -> V {
    s.map(|x| {
        let v = uyart::fpu::fin(x) * FTOI_SCALE[N];
        if v >= 2147483648.0 {
            0x7fff_ffff
        } else if v < -2147483648.0 {
            0x8000_0000
        } else {
            v as i32 as u32
        }
    })
}

/// CLIP s.xyz, w: the six judgements (+x, -x, +y, -y, +z, -z in bits 0 to 5) of s against |w|.
#[inline(always)]
pub(crate) fn clip(s: V, w: u32) -> u32 {
    let w = uyart::fpu::fin(w).abs();
    let mut f = 0;
    for l in 0..3 {
        let v = uyart::fpu::fin(s[l]);
        if v > w {
            f |= 1 << (2 * l);
        }
        if v < -w {
            f |= 2 << (2 * l);
        }
    }
    f
}

/// The programs the port does itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Prog {
    P0,
    P5,
    P6,
}

impl Prog {
    pub fn name(self) -> &'static str {
        match self {
            Prog::P0 => "p0",
            Prog::P5 => "p5",
            Prog::P6 => "p6",
        }
    }

    /// Its microcode's size in bytes and the FNV-1a hash of those bytes (the program the port's code was written from).
    const fn code(self) -> (usize, u64) {
        match self {
            Prog::P0 => (10680, P0_HASH),
            Prog::P5 => (4712, P5_HASH),
            Prog::P6 => (1776, P6_HASH),
        }
    }

    /// The program micro memory (from address 0) holds, if it is one the port does itself: the first whose code it starts with
    /// (0, 5, 6, as `tr_vu1::PROGRAMS` has them).
    pub fn of_micro(micro: &[u8]) -> Option<Prog> {
        [Prog::P0, Prog::P5, Prog::P6].into_iter().find(|p| {
            let (n, h) = p.code();
            micro.get(..n).is_some_and(|c| fnv(c) == h)
        })
    }

    /// Whether `code` is the whole microcode of this program.
    pub fn is(self, code: &[u8]) -> bool {
        let (n, h) = self.code();
        code.len() == n && fnv(code) == h
    }
}

const P0_HASH: u64 = 0x0e6e_21c3_99bc_24bb;
const P5_HASH: u64 = 0x202c_265b_0530_0479;
const P6_HASH: u64 = 0xfe30_67f8_6e1f_ab03;

fn fnv(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &x| (h ^ x as u64).wrapping_mul(0x100_0000_01b3))
}

/// A run of program `p` from micro address `entry` (an MSCAL's immediate times 8) on `vu` with TOP as `vu.top` holds it, under
/// the game's MXCSR: the micro address of its E bit's delay slot, or why it stopped (an entry it has not, or a run that would
/// not end: data it loops on).
pub(crate) fn run(vu: &mut Vu1, p: Prog, entry: usize) -> Result<usize, String> {
    let check = check_count().filter(|&n| CHECKED[p as usize].load(Ordering::Relaxed) < n).map(|_| vu.clone());
    let old = uyart::fpu::set_mode(uyart::fpu::MXCSR);
    vu.begin();
    let r = match p {
        Prog::P0 => crate::vu1p0::run(vu, entry),
        Prog::P5 => crate::vu1p5::run(vu, entry),
        Prog::P6 => crate::vu1p6::run(vu, entry),
    };
    vu.end();
    if let Some(before) = check {
        compare(p, entry, before, vu, &r);
    }
    uyart::fpu::set_mode(old);
    r
}

/// A run of a program the port has no code of its own for (the front end's chains may upload any of the game's), as the port's
/// translator translated it, on `vu`'s state; logged the first time each one runs.
pub(crate) fn run_translated(vu: &mut Vu1, prog: &'static tr_vu1::rt::Prog, entry: usize) -> Result<(), String> {
    static SEEN: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    {
        let mut seen = SEEN.lock().expect("the translated programs' log");
        if !seen.contains(&prog.name) {
            seen.push(prog.name);
            println!("native: VU1 program {} runs as the port's translator translated it (the port has no code of its own for it)", prog.name);
        }
    }
    let node = (prog.entry)(entry).ok_or_else(|| format!("started at {entry:#x}, not one of its entries"))?;
    let mut t = translated(vu);
    let old = uyart::fpu::set_mode(uyart::fpu::MXCSR);
    let ran = crate::mobies::run_contained(|| {
        (prog.run)(&mut t, node);
    });
    uyart::fpu::set_mode(old);
    t.finish();
    (vu.vf, vu.acc, vu.vi, vu.top) = (t.vf, t.vacc, t.vi, t.top);
    vu.mem[..].copy_from_slice(&t.mem[..]);
    vu.kicks = t.kicks;
    vu.sent = t.sent;
    vu.pending = None;
    ran
}

/// The translation's VU1 with `vu`'s state (a kick whose packet is still to be read made again: it is the last of `kicks`).
fn translated(vu: &Vu1) -> tr_vu1::rt::Vu1 {
    let mut t = tr_vu1::rt::Vu1::new();
    t.vf = vu.vf;
    t.vacc = vu.acc;
    t.vi = vu.vi;
    t.mem[..].copy_from_slice(&vu.mem[..]);
    t.top = vu.top;
    t.capture_packets = vu.capture_packets;
    t.defer_packets = vu.defer_packets;
    t.sent = vu.sent.clone();
    t.kicks = vu.kicks.clone();
    if let Some(a) = vu.pending {
        t.kicks.pop();
        t.kick(a);
    }
    t
}

static CHECKED: [AtomicU64; 3] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];
static DIFFER: [AtomicU64; 3] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];

/// UYA_VU1_CHECK's N, read once (and the summary at the process's end registered then).
fn check_count() -> Option<u64> {
    static N: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
    *N.get_or_init(|| {
        let n = std::env::var("UYA_VU1_CHECK").ok().map(|v| v.parse::<u64>().expect("UYA_VU1_CHECK is a number of runs"));
        if n.is_some() {
            unsafe extern "C" {
                fn atexit(f: extern "C" fn()) -> i32;
            }
            // SAFETY: the C runtime's atexit with a function that takes and returns nothing.
            unsafe {
                atexit(summary_at_exit);
            }
        }
        n
    })
}

extern "C" fn summary_at_exit() {
    for p in [Prog::P0, Prog::P5, Prog::P6] {
        summary(p);
    }
}

fn summary(p: Prog) {
    let (c, d) = (CHECKED[p as usize].load(Ordering::Relaxed), DIFFER[p as usize].load(Ordering::Relaxed));
    println!("UYA_VU1_CHECK: {}: {c} runs checked, {d} differ", p.name());
}

/// The translation's run of `p` from `entry` on the state `before`, beside the port's (`after`, `r`).
fn compare(p: Prog, entry: usize, before: Vu1, after: &Vu1, r: &Result<usize, String>) {
    let prog = tr_vu1::PROGRAMS.iter().find(|t| t.name == p.name()).expect("tr_vu1 has programs 0, 5 and 6");
    let mut t = translated(&before);
    let tr: Result<usize, String> = match (prog.entry)(entry) {
        None => Err(format!("not one of its entries ({entry:#x})")),
        Some(node) => {
            let mut end = 0;
            crate::mobies::run_contained(|| end = (prog.run)(&mut t, node)).map(|_| end)
        }
    };
    let k = CHECKED[p as usize].fetch_add(1, Ordering::Relaxed) + 1;
    let at = format!("UYA_VU1_CHECK: {} run {k} (entry {entry:#x}, TOP {:#x})", p.name(), before.top);
    let mut lines: Vec<String> = Vec::new();
    match (r, &tr) {
        (Ok(a), Ok(b)) => {
            if a != b {
                lines.push(format!("{at}: ended at {a:#x}, translated at {b:#x}"));
            }
            for i in 0..32 {
                if after.vf[i] != t.vf[i] {
                    lines.push(format!("{at}: vf{i:02} {:08x?}, translated {:08x?}", after.vf[i], t.vf[i]));
                }
            }
            if after.acc != t.vacc {
                lines.push(format!("{at}: ACC {:08x?}, translated {:08x?}", after.acc, t.vacc));
            }
            for i in 0..32 {
                if after.vi[i] != t.vi[i] {
                    lines.push(format!("{at}: vi{i:02} {:#x}, translated {:#x}", after.vi[i], t.vi[i]));
                }
            }
            for a in 0..1024 {
                if after.mem[a] != t.mem[a] {
                    lines.push(format!("{at}: memory {a:#05x} {:08x?}, translated {:08x?}", after.mem[a], t.mem[a]));
                }
            }
            if after.top != t.top {
                lines.push(format!("{at}: TOP {:#x}, translated {:#x}", after.top, t.top));
            }
            if after.kicks != t.kicks {
                lines.push(format!("{at}: kicks {:#x?}, translated {:#x?}", after.kicks, t.kicks));
            }
            // The packets, the one still to be read included.
            let mut sent = after.sent.clone();
            if let Some(a) = after.pending {
                sent.push(after.packet(a));
            }
            t.finish();
            for (n, (a, b)) in sent.iter().zip(&t.sent).enumerate() {
                if a != b {
                    lines.push(format!("{at}: packet {n} differs ({} quadwords, translated {})", a.as_ref().map_or(0, |v| v.len()), b.as_ref().map_or(0, |v| v.len())));
                }
            }
            if sent.len() != t.sent.len() {
                lines.push(format!("{at}: {} packets, translated {}", sent.len(), t.sent.len()));
            }
        }
        // Both stopped: neither run is used.
        (Err(_), Err(_)) => {}
        (Ok(a), Err(e)) => lines.push(format!("{at}: ended at {a:#x}, the translation stopped: {e}")),
        (Err(e), Ok(b)) => lines.push(format!("{at}: stopped ({e}), the translation ended at {b:#x}")),
    }
    let n = check_count().expect("checking");
    if !lines.is_empty() {
        DIFFER[p as usize].fetch_add(1, Ordering::Relaxed);
        let shown = lines.len().min(64);
        for l in &lines[..shown] {
            println!("{l}");
        }
        if lines.len() > shown {
            println!("{at}: {} more differences", lines.len() - shown);
        }
    }
    if k == n {
        summary(p);
    }
}
