//! VU0 as the EE's code uses it: the macro instructions (COP2) on VF, ACC and VI with the VU's arithmetic (`fpu::fin`, toward
//! zero) and its MAC and status flags, VU0's data memory (`Cpu::vu0d`), and the operations of the game's VU0 microprograms as
//! `uyatr` translates them (`m_*`). Starting and resuming a microprogram (VCALLMS, CTC2) is the runtime's (`uyart::vu0`).

use crate::Cpu;
use crate::fpu::{add, fin, max, min, mul, sub};

#[inline(always)]
pub fn bcast(x: u32) -> [u32; 4] {
    [x; 4]
}

/// Lane l (0 = x) is in the dest field `d` (bit 3 = x).
#[inline(always)]
const fn on(d: u32, l: usize) -> bool {
    d & (8 >> l) != 0
}

#[inline(always)]
fn flags(c: &mut Cpu, mac: u32) {
    c.vi[17] = mac;
    let st = ((mac & 0xf != 0) as u32) | (((mac & 0xf0 != 0) as u32) << 1);
    c.vi[16] = (c.vi[16] & !0x3) | st | (st << 6);
}

/// The FMAC operations: K = 0 ADD, 1 SUB, 2 MADD, 3 MSUB, 4 MAX, 5 MINI, 6 MUL; `dst` 32 is ACC; MAX and MINI set no flags.
#[inline(always)]
pub fn fmac<const K: u8, const D: u32>(c: &mut Cpu, dst: usize, fs: usize, t: [u32; 4]) {
    let s = c.vf[fs];
    let acc = c.vacc;
    let mut out = if dst == 32 { c.vacc } else { c.vf[dst] };
    let mut mac = 0u32;
    for l in 0..4 {
        if !on(D, l) {
            continue;
        }
        let v = match K {
            0 => add(s[l], t[l]),
            1 => sub(s[l], t[l]),
            2 => add(acc[l], mul(s[l], t[l])),
            3 => sub(acc[l], mul(s[l], t[l])),
            4 => max(s[l], t[l]),
            5 => min(s[l], t[l]),
            _ => mul(s[l], t[l]),
        };
        out[l] = v;
        if v & 0x7fff_ffff == 0 {
            mac |= 1 << (3 - l);
        }
        if v & 0x8000_0000 != 0 {
            mac |= 1 << (7 - l);
        }
    }
    if dst == 32 {
        c.vacc = out;
    } else if dst != 0 {
        c.vf[dst] = out;
    }
    if K != 4 && K != 5 {
        flags(c, mac);
    }
}

/// OPMULA: ACC.xyz = fs.yzx * ft.zxy.
pub fn opmula(c: &mut Cpu, fs: usize, ft: usize) {
    let (s, t) = (c.vf[fs], c.vf[ft]);
    let p = [mul(s[1], t[2]), mul(s[2], t[0]), mul(s[0], t[1])];
    c.vacc[..3].copy_from_slice(&p);
    flags(c, mac3(p));
}

/// OPMSUB: fd.xyz = ACC.xyz - fs.yzx * ft.zxy.
pub fn opmsub(c: &mut Cpu, fd: usize, fs: usize, ft: usize) {
    let (s, t, a) = (c.vf[fs], c.vf[ft], c.vacc);
    let p = [sub(a[0], mul(s[1], t[2])), sub(a[1], mul(s[2], t[0])), sub(a[2], mul(s[0], t[1]))];
    if fd != 0 {
        c.vf[fd][..3].copy_from_slice(&p);
    }
    flags(c, mac3(p));
}

fn mac3(p: [u32; 3]) -> u32 {
    let mut m = 0;
    for (l, v) in p.iter().enumerate() {
        if v & 0x7fff_ffff == 0 {
            m |= 1 << (3 - l);
        }
        if v & 0x8000_0000 != 0 {
            m |= 1 << (7 - l);
        }
    }
    m
}

const ITOF_SCALE: [f32; 4] = [1.0, 1.0 / 16.0, 1.0 / 4096.0, 1.0 / 32768.0];
const FTOI_SCALE: [f32; 4] = [1.0, 16.0, 4096.0, 32768.0];

pub fn itof<const N: usize, const D: u32>(c: &mut Cpu, ft: usize, fs: usize) {
    let s = c.vf[fs];
    if ft == 0 {
        return;
    }
    for l in 0..4 {
        if on(D, l) {
            c.vf[ft][l] = ((s[l] as i32 as f32) * ITOF_SCALE[N]).to_bits();
        }
    }
}

pub fn ftoi<const N: usize, const D: u32>(c: &mut Cpu, ft: usize, fs: usize) {
    let s = c.vf[fs];
    if ft == 0 {
        return;
    }
    for l in 0..4 {
        if on(D, l) {
            let x = fin(s[l]) * FTOI_SCALE[N];
            c.vf[ft][l] = if x >= 2147483648.0 {
                0x7fff_ffff
            } else if x < -2147483648.0 {
                0x8000_0000
            } else {
                x as i32 as u32
            };
        }
    }
}

pub fn abs<const D: u32>(c: &mut Cpu, ft: usize, fs: usize) {
    let s = c.vf[fs];
    if ft == 0 {
        return;
    }
    for l in 0..4 {
        if on(D, l) {
            c.vf[ft][l] = s[l] & 0x7fff_ffff;
        }
    }
}

#[inline(always)]
pub fn mov<const D: u32>(c: &mut Cpu, ft: usize, v: [u32; 4]) {
    if ft == 0 {
        return;
    }
    for l in 0..4 {
        if on(D, l) {
            c.vf[ft][l] = v[l];
        }
    }
}

/// CLIP: fs.xyz against +-|ft.w|; the six bits are shifted into the clip flag's last 24 bits.
pub fn clip(c: &mut Cpu, fs: usize, ft: usize) {
    let (s, w) = (c.vf[fs], fin(c.vf[ft][3]).abs());
    let mut f = (c.vi[18] << 6) & 0xff_ffff;
    for l in 0..3 {
        let v = fin(s[l]);
        if v > w {
            f |= 1 << (2 * l);
        }
        if v < -w {
            f |= 2 << (2 * l);
        }
    }
    c.vi[18] = f;
}

#[inline(always)]
fn dq(_c: &mut Cpu, vi: u32) -> usize {
    ((vi & 0xff) * 16) as usize
}

/// A quadword of VU0 data memory (4 KB: `dq` keeps `a` below 0xFF0).
fn load(c: &mut Cpu, a: usize) -> [u32; 4] {
    m_ld(c.vu0d, a)
}

fn store<const D: u32>(c: &mut Cpu, a: usize, v: [u32; 4]) {
    m_st::<D>(c.vu0d, a, v)
}

pub fn lqi<const D: u32>(c: &mut Cpu, ft: usize, is: usize) {
    let a = dq(c, c.vi[is]);
    let v = load(c, a);
    mov::<D>(c, ft, v);
    c.set_vi(is, c.vi[is].wrapping_add(1));
}
pub fn lqd<const D: u32>(c: &mut Cpu, ft: usize, is: usize) {
    c.set_vi(is, c.vi[is].wrapping_sub(1));
    let a = dq(c, c.vi[is]);
    let v = load(c, a);
    mov::<D>(c, ft, v);
}
pub fn sqi<const D: u32>(c: &mut Cpu, fs: usize, it: usize) {
    let a = dq(c, c.vi[it]);
    let v = c.vf[fs];
    store::<D>(c, a, v);
    c.set_vi(it, c.vi[it].wrapping_add(1));
}
pub fn sqd<const D: u32>(c: &mut Cpu, fs: usize, it: usize) {
    c.set_vi(it, c.vi[it].wrapping_sub(1));
    let a = dq(c, c.vi[it]);
    let v = c.vf[fs];
    store::<D>(c, a, v);
}

/// ILWR.D it, (is): the 16 bits of lane D of the quadword at vi[is].
pub fn ilwr<const D: u32>(c: &mut Cpu, it: usize, is: usize) {
    let a = dq(c, c.vi[is]);
    let v = load(c, a);
    let l = (0..4).find(|&l| on(D, l)).unwrap_or(0);
    c.set_vi(it, v[l] & 0xffff);
}
/// ISWR.D it, (is): vi[it]'s 16 bits into the D lanes of the quadword at vi[is].
pub fn iswr<const D: u32>(c: &mut Cpu, it: usize, is: usize) {
    let a = dq(c, c.vi[is]);
    let v = c.vi[it] & 0xffff;
    store::<D>(c, a, [v; 4]);
}

/// DIV (0), SQRT (1), RSQRT (2) into Q, with the status flags D and I.
pub fn qop<const K: u8>(c: &mut Cpu, a: u32, b: u32) {
    let (x, y) = (fin(a), fin(b));
    let maxv = |s: u32| (s & 0x8000_0000) | 0x7f7f_ffff;
    let q = match K {
        0 => {
            if y == 0.0 {
                c.vi[16] |= if x == 0.0 { 0x410 } else { 0x820 };
                maxv(a ^ b)
            } else {
                (x / y).to_bits()
            }
        }
        1 => {
            if y < 0.0 {
                c.vi[16] |= 0x410;
            }
            if y == 0.0 { b & 0x8000_0000 } else { y.abs().sqrt().to_bits() }
        }
        _ => {
            if y == 0.0 {
                c.vi[16] |= 0x820;
                maxv(a ^ b)
            } else {
                if y < 0.0 {
                    c.vi[16] |= 0x410;
                }
                (x / y.abs().sqrt()).to_bits()
            }
        }
    };
    c.vi[22] = q;
}

fn r_next(r: u32) -> u32 {
    let b = ((r >> 4) ^ (r >> 22)) & 1;
    (((r << 1) | b) & 0x7f_ffff) | 0x3f80_0000
}
pub fn rnext<const D: u32>(c: &mut Cpu, ft: usize) {
    c.vi[20] = r_next(c.vi[20]);
    let r = c.vi[20];
    mov::<D>(c, ft, [r; 4]);
}
pub fn rget<const D: u32>(c: &mut Cpu, ft: usize) {
    let r = c.vi[20];
    mov::<D>(c, ft, [r; 4]);
}
pub fn rinit(c: &mut Cpu, v: u32) {
    c.vi[20] = 0x3f80_0000 | (v & 0x7f_ffff);
}
pub fn rxor(c: &mut Cpu, v: u32) {
    c.vi[20] = 0x3f80_0000 | ((c.vi[20] ^ v) & 0x7f_ffff);
}

// ---- the translated microprograms' operations (`uyatr`'s vu0mp.rs): whole vectors, the lanes put by `m_put` ----------------

/// The FMAC operations on every lane (K as `fmac`); the dest lanes `D` are the ones the caller puts. Four lanes at once on SSE:
/// the game threads run with toward-zero rounding, DAZ and FTZ (`fpu::set_mode`), so the host's arithmetic is the VU's once
/// an operand with exponent 255 has been read as the largest number of its sign (`fin4`); MAX and MINI keep the chosen
/// operand's own bits. The lanes outside `D` are 0, as the caller drops them.
#[inline(always)]
pub fn m_fmac<const K: u8, const D: u32>(s: [u32; 4], t: [u32; 4], acc: [u32; 4]) -> [u32; 4] {
    use std::arch::x86_64::*;
    unsafe {
        let (si, ti) = (ld4(s), ld4(t));
        let (fs, ft) = (fin4(si), fin4(ti));
        let r = match K {
            0 => _mm_castps_si128(_mm_add_ps(fs, ft)),
            1 => _mm_castps_si128(_mm_sub_ps(fs, ft)),
            2 => _mm_castps_si128(_mm_add_ps(fin4(ld4(acc)), _mm_mul_ps(fs, ft))),
            3 => _mm_castps_si128(_mm_sub_ps(fin4(ld4(acc)), _mm_mul_ps(fs, ft))),
            // MAX keeps s where s >= t, MINI where s <= t (the scalar `fpu::max` / `fpu::min`).
            4 => sel(_mm_castps_si128(_mm_cmpge_ps(fs, ft)), si, ti),
            5 => sel(_mm_castps_si128(_mm_cmple_ps(fs, ft)), si, ti),
            _ => _mm_castps_si128(_mm_mul_ps(fs, ft)),
        };
        st4(_mm_and_si128(r, lanes::<D>()))
    }
}

#[inline(always)]
unsafe fn ld4(v: [u32; 4]) -> std::arch::x86_64::__m128i {
    unsafe { std::arch::x86_64::_mm_loadu_si128(v.as_ptr() as *const _) }
}
#[inline(always)]
unsafe fn st4(v: std::arch::x86_64::__m128i) -> [u32; 4] {
    let mut o = [0u32; 4];
    unsafe { std::arch::x86_64::_mm_storeu_si128(o.as_mut_ptr() as *mut _, v) };
    o
}
/// Every lane's bits where `m` is set, else `b`'s.
#[inline(always)]
unsafe fn sel(m: std::arch::x86_64::__m128i, a: std::arch::x86_64::__m128i, b: std::arch::x86_64::__m128i) -> std::arch::x86_64::__m128i {
    use std::arch::x86_64::*;
    unsafe { _mm_or_si128(_mm_and_si128(m, a), _mm_andnot_si128(m, b)) }
}
/// The dest field `D` as a lane mask (bit 3 = x = lane 0).
#[inline(always)]
unsafe fn lanes<const D: u32>() -> std::arch::x86_64::__m128i {
    let m = |l: usize| if on(D, l) { -1i32 } else { 0 };
    unsafe { std::arch::x86_64::_mm_setr_epi32(m(0), m(1), m(2), m(3)) }
}
/// `fpu::fin` on four lanes: exponent 255 becomes the largest number of its sign (denormals are DAZ's).
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

#[inline(always)]
pub fn m_opmula(s: [u32; 4], t: [u32; 4]) -> [u32; 4] {
    [mul(s[1], t[2]), mul(s[2], t[0]), mul(s[0], t[1]), 0]
}
#[inline(always)]
pub fn m_opmsub(s: [u32; 4], t: [u32; 4], a: [u32; 4]) -> [u32; 4] {
    [sub(a[0], mul(s[1], t[2])), sub(a[1], mul(s[2], t[0])), sub(a[2], mul(s[0], t[1])), 0]
}
#[inline(always)]
pub fn m_itof<const N: usize>(s: [u32; 4]) -> [u32; 4] {
    s.map(|x| ((x as i32 as f32) * ITOF_SCALE[N]).to_bits())
}
#[inline(always)]
pub fn m_ftoi<const N: usize>(s: [u32; 4]) -> [u32; 4] {
    s.map(|x| {
        let v = fin(x) * FTOI_SCALE[N];
        if v >= 2147483648.0 {
            0x7fff_ffff
        } else if v < -2147483648.0 {
            0x8000_0000
        } else {
            v as i32 as u32
        }
    })
}
#[inline(always)]
pub fn m_abs(s: [u32; 4]) -> [u32; 4] {
    s.map(|x| x & 0x7fff_ffff)
}

/// The lanes of `M` (bit 3 = x) of `v` into `r`.
#[inline(always)]
pub fn m_put<const M: u32>(r: &mut [u32; 4], v: [u32; 4]) {
    if M == 15 {
        *r = v;
        return;
    }
    unsafe { *r = st4(sel(lanes::<M>(), ld4(v), ld4(*r))) }
}

/// The MAC flags of a result over the lanes of `M`: zero at bit 3 - l, sign at bit 7 - l.
#[inline(always)]
pub fn m_mac<const M: u32>(v: [u32; 4]) -> u32 {
    use std::arch::x86_64::*;
    // A lane mask (lane l at bit l) in the flags' order (lane l at bit 3 - l).
    const REV: [u32; 16] = [0, 8, 4, 12, 2, 10, 6, 14, 1, 9, 5, 13, 3, 11, 7, 15];
    unsafe {
        let x = ld4(v);
        let zero = _mm_movemask_ps(_mm_castsi128_ps(_mm_cmpeq_epi32(_mm_and_si128(x, _mm_set1_epi32(0x7fff_ffff)), _mm_setzero_si128()))) as usize;
        let sign = _mm_movemask_ps(_mm_castsi128_ps(x)) as usize;
        (REV[zero] & M) | ((REV[sign] & M) << 4)
    }
}

/// A quadword of VU0 data memory (`a` a byte address, quadword aligned, below 4 KB).
#[inline(always)]
pub fn m_ld(dm: *mut u8, a: usize) -> [u32; 4] {
    unsafe { std::array::from_fn(|l| (dm.add(a + 4 * l) as *const u32).read_unaligned()) }
}
#[inline(always)]
pub fn m_st<const D: u32>(dm: *mut u8, a: usize, v: [u32; 4]) {
    for l in 0..4 {
        if on(D, l) {
            unsafe { (dm.add(a + 4 * l) as *mut u32).write_unaligned(v[l]) };
        }
    }
}

/// DIV (0), SQRT (1), RSQRT (2): the value Q gets when it lands (the status flags D and I are set now).
#[inline(always)]
pub fn m_q<const K: u8>(c: &mut Cpu, a: u32, b: u32) -> u32 {
    let old = c.vi[22];
    qop::<K>(c, a, b);
    let q = c.vi[22];
    c.vi[22] = old;
    q
}
