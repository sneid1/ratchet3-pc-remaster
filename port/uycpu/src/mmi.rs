//! The R5900's multimedia instructions on 128-bit registers, (low 64 bits, high 64 bits). rd = op(rs, rt) as the R5900 defines
//! each. HI and LO are 128 bits each: `lo` and `hi` hold their low doublewords, `lo1` and `hi1` their high ones.

use crate::Cpu;

type Q = (u64, u64);

#[inline(always)]
fn w(q: Q) -> [u32; 4] {
    [q.0 as u32, (q.0 >> 32) as u32, q.1 as u32, (q.1 >> 32) as u32]
}
#[inline(always)]
fn fw(v: [u32; 4]) -> Q {
    (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32)
}
#[inline(always)]
fn h(q: Q) -> [u16; 8] {
    let mut o = [0u16; 8];
    for i in 0..4 {
        o[i] = (q.0 >> (16 * i)) as u16;
        o[4 + i] = (q.1 >> (16 * i)) as u16;
    }
    o
}
#[inline(always)]
fn fh(v: [u16; 8]) -> Q {
    let mut a = 0u64;
    let mut b = 0u64;
    for i in 0..4 {
        a |= (v[i] as u64) << (16 * i);
        b |= (v[4 + i] as u64) << (16 * i);
    }
    (a, b)
}
#[inline(always)]
fn b(q: Q) -> [u8; 16] {
    let mut o = [0u8; 16];
    o[..8].copy_from_slice(&q.0.to_le_bytes());
    o[8..].copy_from_slice(&q.1.to_le_bytes());
    o
}
#[inline(always)]
fn fb(v: [u8; 16]) -> Q {
    (u64::from_le_bytes(v[..8].try_into().unwrap()), u64::from_le_bytes(v[8..].try_into().unwrap()))
}

macro_rules! lanes_w {
    ($name:ident, |$x:ident, $y:ident| $e:expr) => {
        pub fn $name(s: Q, t: Q) -> Q {
            let (a, bb) = (w(s), w(t));
            fw(std::array::from_fn(|i| {
                let ($x, $y) = (a[i], bb[i]);
                $e
            }))
        }
    };
}
macro_rules! lanes_h {
    ($name:ident, |$x:ident, $y:ident| $e:expr) => {
        pub fn $name(s: Q, t: Q) -> Q {
            let (a, bb) = (h(s), h(t));
            fh(std::array::from_fn(|i| {
                let ($x, $y) = (a[i], bb[i]);
                $e
            }))
        }
    };
}
macro_rules! lanes_b {
    ($name:ident, |$x:ident, $y:ident| $e:expr) => {
        pub fn $name(s: Q, t: Q) -> Q {
            let (a, bb) = (b(s), b(t));
            fb(std::array::from_fn(|i| {
                let ($x, $y) = (a[i], bb[i]);
                $e
            }))
        }
    };
}

lanes_w!(paddw, |x, y| x.wrapping_add(y));
lanes_w!(psubw, |x, y| x.wrapping_sub(y));
lanes_w!(pcgtw, |x, y| if (x as i32) > (y as i32) { u32::MAX } else { 0 });
lanes_w!(pmaxw, |x, y| if (x as i32) > (y as i32) { x } else { y });
lanes_w!(pminw, |x, y| if (x as i32) < (y as i32) { x } else { y });
lanes_w!(pceqw, |x, y| if x == y { u32::MAX } else { 0 });
lanes_w!(paddsw, |x, y| (x as i32).saturating_add(y as i32) as u32);
lanes_w!(psubsw, |x, y| (x as i32).saturating_sub(y as i32) as u32);
lanes_w!(padduw, |x, y| x.saturating_add(y));
lanes_w!(psubuw, |x, y| x.saturating_sub(y));
lanes_h!(paddh, |x, y| x.wrapping_add(y));
lanes_h!(psubh, |x, y| x.wrapping_sub(y));
lanes_h!(pcgth, |x, y| if (x as i16) > (y as i16) { u16::MAX } else { 0 });
lanes_h!(pmaxh, |x, y| if (x as i16) > (y as i16) { x } else { y });
lanes_h!(pminh, |x, y| if (x as i16) < (y as i16) { x } else { y });
lanes_h!(pceqh, |x, y| if x == y { u16::MAX } else { 0 });
lanes_h!(paddsh, |x, y| (x as i16).saturating_add(y as i16) as u16);
lanes_h!(psubsh, |x, y| (x as i16).saturating_sub(y as i16) as u16);
lanes_h!(padduh, |x, y| x.saturating_add(y));
lanes_h!(psubuh, |x, y| x.saturating_sub(y));
lanes_b!(paddb, |x, y| x.wrapping_add(y));
lanes_b!(psubb, |x, y| x.wrapping_sub(y));
lanes_b!(pcgtb, |x, y| if (x as i8) > (y as i8) { u8::MAX } else { 0 });
lanes_b!(pceqb, |x, y| if x == y { u8::MAX } else { 0 });
lanes_b!(paddsb, |x, y| (x as i8).saturating_add(y as i8) as u8);
lanes_b!(psubsb, |x, y| (x as i8).saturating_sub(y as i8) as u8);
lanes_b!(paddub, |x, y| x.saturating_add(y));
lanes_b!(psubub, |x, y| x.saturating_sub(y));

pub fn padsbh(s: Q, t: Q) -> Q {
    let (a, bb) = (h(s), h(t));
    fh(std::array::from_fn(|i| if i < 4 { a[i].wrapping_sub(bb[i]) } else { a[i].wrapping_add(bb[i]) }))
}

pub fn pand(s: Q, t: Q) -> Q {
    (s.0 & t.0, s.1 & t.1)
}
pub fn por(s: Q, t: Q) -> Q {
    (s.0 | t.0, s.1 | t.1)
}
pub fn pxor(s: Q, t: Q) -> Q {
    (s.0 ^ t.0, s.1 ^ t.1)
}
pub fn pnor(s: Q, t: Q) -> Q {
    (!(s.0 | t.0), !(s.1 | t.1))
}

pub fn pextlw(s: Q, t: Q) -> Q {
    let (a, bb) = (w(s), w(t));
    fw([bb[0], a[0], bb[1], a[1]])
}
pub fn pextuw(s: Q, t: Q) -> Q {
    let (a, bb) = (w(s), w(t));
    fw([bb[2], a[2], bb[3], a[3]])
}
pub fn ppacw(s: Q, t: Q) -> Q {
    let (a, bb) = (w(s), w(t));
    fw([bb[0], bb[2], a[0], a[2]])
}
pub fn pextlh(s: Q, t: Q) -> Q {
    let (a, bb) = (h(s), h(t));
    fh(std::array::from_fn(|i| if i % 2 == 0 { bb[i / 2] } else { a[i / 2] }))
}
pub fn pextuh(s: Q, t: Q) -> Q {
    let (a, bb) = (h(s), h(t));
    fh(std::array::from_fn(|i| if i % 2 == 0 { bb[4 + i / 2] } else { a[4 + i / 2] }))
}
pub fn ppach(s: Q, t: Q) -> Q {
    let (a, bb) = (h(s), h(t));
    fh(std::array::from_fn(|i| if i < 4 { bb[2 * i] } else { a[2 * (i - 4)] }))
}
pub fn pextlb(s: Q, t: Q) -> Q {
    let (a, bb) = (b(s), b(t));
    fb(std::array::from_fn(|i| if i % 2 == 0 { bb[i / 2] } else { a[i / 2] }))
}
pub fn pextub(s: Q, t: Q) -> Q {
    let (a, bb) = (b(s), b(t));
    fb(std::array::from_fn(|i| if i % 2 == 0 { bb[8 + i / 2] } else { a[8 + i / 2] }))
}
pub fn ppacb(s: Q, t: Q) -> Q {
    let (a, bb) = (b(s), b(t));
    fb(std::array::from_fn(|i| if i < 8 { bb[2 * i] } else { a[2 * (i - 8)] }))
}
pub fn pinth(s: Q, t: Q) -> Q {
    let (a, bb) = (h(s), h(t));
    fh(std::array::from_fn(|i| if i % 2 == 0 { bb[i / 2] } else { a[4 + i / 2] }))
}
pub fn pinteh(s: Q, t: Q) -> Q {
    let (a, bb) = (h(s), h(t));
    fh(std::array::from_fn(|i| if i % 2 == 0 { bb[i] } else { a[i - 1] }))
}
pub fn pcpyld(s: Q, t: Q) -> Q {
    (t.0, s.0)
}
pub fn pcpyud(s: Q, t: Q) -> Q {
    (s.1, t.1)
}

pub fn pext5(t: Q) -> Q {
    let a = w(t);
    fw(a.map(|x| ((x & 0x1f) << 3) | (((x >> 5) & 0x1f) << 11) | (((x >> 10) & 0x1f) << 19) | (((x >> 15) & 1) << 31)))
}
pub fn ppac5(t: Q) -> Q {
    let a = w(t);
    fw(a.map(|x| ((x >> 3) & 0x1f) | (((x >> 11) & 0x1f) << 5) | (((x >> 19) & 0x1f) << 10) | (((x >> 31) & 1) << 15)))
}
pub fn pabsw(t: Q) -> Q {
    fw(w(t).map(|x| if x == 0x8000_0000 { 0x7fff_ffff } else { (x as i32).unsigned_abs() }))
}
pub fn pabsh(t: Q) -> Q {
    fh(h(t).map(|x| if x == 0x8000 { 0x7fff } else { (x as i16).unsigned_abs() }))
}
pub fn pexeh(t: Q) -> Q {
    let a = h(t);
    fh([a[2], a[1], a[0], a[3], a[6], a[5], a[4], a[7]])
}
pub fn prevh(t: Q) -> Q {
    let a = h(t);
    fh([a[3], a[2], a[1], a[0], a[7], a[6], a[5], a[4]])
}
pub fn pexch(t: Q) -> Q {
    let a = h(t);
    fh([a[0], a[2], a[1], a[3], a[4], a[6], a[5], a[7]])
}
pub fn pcpyh(t: Q) -> Q {
    let a = h(t);
    fh([a[0], a[0], a[0], a[0], a[4], a[4], a[4], a[4]])
}
pub fn pexew(t: Q) -> Q {
    let a = w(t);
    fw([a[2], a[1], a[0], a[3]])
}
pub fn pexcw(t: Q) -> Q {
    let a = w(t);
    fw([a[0], a[2], a[1], a[3]])
}
pub fn prot3w(t: Q) -> Q {
    let a = w(t);
    fw([a[1], a[2], a[0], a[3]])
}

pub fn psllh(t: Q, sa: u32) -> Q {
    fh(h(t).map(|x| x << (sa & 15)))
}
pub fn psrlh(t: Q, sa: u32) -> Q {
    fh(h(t).map(|x| x >> (sa & 15)))
}
pub fn psrah(t: Q, sa: u32) -> Q {
    fh(h(t).map(|x| ((x as i16) >> (sa & 15)) as u16))
}
pub fn psllw(t: Q, sa: u32) -> Q {
    fw(w(t).map(|x| x << (sa & 31)))
}
pub fn psrlw(t: Q, sa: u32) -> Q {
    fw(w(t).map(|x| x >> (sa & 31)))
}
pub fn psraw(t: Q, sa: u32) -> Q {
    fw(w(t).map(|x| ((x as i32) >> (sa & 31)) as u32))
}

/// PSLLVW and friends: words 0 and 2 of rt shifted by words 0 and 2 of rs, each result sign-extended into its doubleword.
pub fn psllvw(s: Q, t: Q) -> Q {
    let (a, bb) = (w(s), w(t));
    ((bb[0] << (a[0] & 31)) as i32 as i64 as u64, (bb[2] << (a[2] & 31)) as i32 as i64 as u64)
}
pub fn psrlvw(s: Q, t: Q) -> Q {
    let (a, bb) = (w(s), w(t));
    ((bb[0] >> (a[0] & 31)) as i32 as i64 as u64, (bb[2] >> (a[2] & 31)) as i32 as i64 as u64)
}
pub fn psravw(s: Q, t: Q) -> Q {
    let (a, bb) = (w(s), w(t));
    (((bb[0] as i32) >> (a[0] & 31)) as i64 as u64, ((bb[2] as i32) >> (a[2] & 31)) as i64 as u64)
}

/// QFSRV: the 256-bit rs:rt shifted right by SA bits, its low 128 bits.
pub fn qfsrv(s: Q, t: Q, sa: u32) -> Q {
    let v = ((s.1 as u128) << 64 | s.0 as u128, (t.1 as u128) << 64 | t.0 as u128);
    let n = sa & 127;
    let r = if n == 0 { v.1 } else { (v.1 >> n) | (v.0 << (128 - n)) };
    (r as u64, (r >> 64) as u64)
}

/// PLZCW: for each word of rs's low doubleword, the count of leading bits equal to its sign bit, less one.
pub fn plzcw(s: u64) -> u64 {
    let f = |x: u32| -> u64 { (if x & 0x8000_0000 != 0 { (!x).leading_zeros() } else { x.leading_zeros() } - 1) as u64 };
    f(s as u32) | f((s >> 32) as u32) << 32
}

// HI and LO as words: LO.w0, LO.w1 in `lo`, LO.w2, LO.w3 in `lo1` (the same for HI).
fn lo_w(c: &Cpu) -> [u32; 4] {
    w((c.lo, c.lo1))
}
fn hi_w(c: &Cpu) -> [u32; 4] {
    w((c.hi, c.hi1))
}
fn set_lohi(c: &mut Cpu, lo: [u32; 4], hi: [u32; 4]) {
    (c.lo, c.lo1) = fw(lo);
    (c.hi, c.hi1) = fw(hi);
}

pub fn pmfhl_lw(lo: u64, lo1: u64, hi: u64, hi1: u64) -> Q {
    let (l, hh) = (w((lo, lo1)), w((hi, hi1)));
    fw([l[0], hh[0], l[2], hh[2]])
}
pub fn pmfhl_uw(lo: u64, lo1: u64, hi: u64, hi1: u64) -> Q {
    let (l, hh) = (w((lo, lo1)), w((hi, hi1)));
    fw([l[1], hh[1], l[3], hh[3]])
}
pub fn pmfhl_slw(lo: u64, lo1: u64, hi: u64, hi1: u64) -> Q {
    let (l, hh) = (w((lo, lo1)), w((hi, hi1)));
    let sat = |x: i64| x.clamp(i32::MIN as i64, i32::MAX as i64) as u64;
    (sat(((hh[0] as u64) << 32 | l[0] as u64) as i64), sat(((hh[2] as u64) << 32 | l[2] as u64) as i64))
}
pub fn pmfhl_lh(lo: u64, lo1: u64, hi: u64, hi1: u64) -> Q {
    let (l, hh) = (h((lo, lo1)), h((hi, hi1)));
    fh([l[0], l[2], hh[0], hh[2], l[4], l[6], hh[4], hh[6]])
}
pub fn pmfhl_sh(lo: u64, lo1: u64, hi: u64, hi1: u64) -> Q {
    let (l, hh) = (w((lo, lo1)), w((hi, hi1)));
    let s = |x: u32| (x as i32).clamp(i16::MIN as i32, i16::MAX as i32) as u16;
    fh([s(l[0]), s(l[1]), s(hh[0]), s(hh[1]), s(l[2]), s(l[3]), s(hh[2]), s(hh[3])])
}

/// PMULTW / PMADDW / PMSUBW (signed) and PMULTUW / PMADDUW (unsigned) on words 0 and 2: LO/HI doubleword i = the 64-bit result's
/// low and high words, each sign-extended; rd's doubleword i = the result.
fn pmw(c: &mut Cpu, s: Q, t: Q, signed: bool, acc: i32) -> Q {
    let (a, bb) = (w(s), w(t));
    let mut out = [0u64; 2];
    let (mut lo, mut hi) = ([c.lo, c.lo1], [c.hi, c.hi1]);
    for i in 0..2 {
        let p = if signed { (a[2 * i] as i32 as i64).wrapping_mul(bb[2 * i] as i32 as i64) as u64 } else { (a[2 * i] as u64) * (bb[2 * i] as u64) };
        let old = (hi[i] as u32 as u64) << 32 | lo[i] as u32 as u64;
        let v = match acc {
            1 => old.wrapping_add(p),
            -1 => old.wrapping_sub(p),
            _ => p,
        };
        lo[i] = v as u32 as i32 as i64 as u64;
        hi[i] = (v >> 32) as u32 as i32 as i64 as u64;
        out[i] = v;
    }
    (c.lo, c.lo1, c.hi, c.hi1) = (lo[0], lo[1], hi[0], hi[1]);
    (out[0], out[1])
}
pub fn pmultw(c: &mut Cpu, s: Q, t: Q) -> Q {
    pmw(c, s, t, true, 0)
}
pub fn pmaddw(c: &mut Cpu, s: Q, t: Q) -> Q {
    pmw(c, s, t, true, 1)
}
pub fn pmsubw(c: &mut Cpu, s: Q, t: Q) -> Q {
    pmw(c, s, t, true, -1)
}
pub fn pmultuw(c: &mut Cpu, s: Q, t: Q) -> Q {
    pmw(c, s, t, false, 0)
}
pub fn pmadduw(c: &mut Cpu, s: Q, t: Q) -> Q {
    pmw(c, s, t, false, 1)
}

/// PMULTH / PMADDH / PMSUBH: the eight halfword products into LO.w0, LO.w1, HI.w0, HI.w1, LO.w2, LO.w3, HI.w2, HI.w3;
/// rd = (LO.w0, HI.w0, LO.w2, HI.w2).
fn pmh(c: &mut Cpu, s: Q, t: Q, acc: i32) -> Q {
    let (a, bb) = (h(s), h(t));
    let p: [u32; 8] = std::array::from_fn(|i| (a[i] as i16 as i32).wrapping_mul(bb[i] as i16 as i32) as u32);
    let (mut lo, mut hi) = (lo_w(c), hi_w(c));
    let slots = [(0usize, false), (1, false), (0, true), (1, true), (2, false), (3, false), (2, true), (3, true)];
    for (i, &(k, is_hi)) in slots.iter().enumerate() {
        let r = if is_hi { &mut hi[k] } else { &mut lo[k] };
        *r = match acc {
            1 => r.wrapping_add(p[i]),
            -1 => r.wrapping_sub(p[i]),
            _ => p[i],
        };
    }
    set_lohi(c, lo, hi);
    fw([lo[0], hi[0], lo[2], hi[2]])
}
pub fn pmulth(c: &mut Cpu, s: Q, t: Q) -> Q {
    pmh(c, s, t, 0)
}
pub fn pmaddh(c: &mut Cpu, s: Q, t: Q) -> Q {
    pmh(c, s, t, 1)
}
pub fn pmsubh(c: &mut Cpu, s: Q, t: Q) -> Q {
    pmh(c, s, t, -1)
}

/// PHMADH / PHMSBH: pairs of halfword products summed (or the odd minus the even) into LO.w0, HI.w0, LO.w2, HI.w2.
fn phm(c: &mut Cpu, s: Q, t: Q, sub: bool) -> Q {
    let (a, bb) = (h(s), h(t));
    let p = |i: usize| (a[i] as i16 as i32).wrapping_mul(bb[i] as i16 as i32);
    let pair = |k: usize| if sub { p(2 * k + 1).wrapping_sub(p(2 * k)) } else { p(2 * k + 1).wrapping_add(p(2 * k)) } as u32;
    let (mut lo, mut hi) = (lo_w(c), hi_w(c));
    lo[0] = pair(0);
    hi[0] = pair(1);
    lo[2] = pair(2);
    hi[2] = pair(3);
    set_lohi(c, lo, hi);
    fw([lo[0], hi[0], lo[2], hi[2]])
}
pub fn phmadh(c: &mut Cpu, s: Q, t: Q) -> Q {
    phm(c, s, t, false)
}
pub fn phmsbh(c: &mut Cpu, s: Q, t: Q) -> Q {
    phm(c, s, t, true)
}

/// PDIVW / PDIVUW: words 0 and 2 divided (the DIV and DIVU results, zero divisors included) into LO/HI doublewords 0 and 1.
/// PDIVBW: the four words of rs by rt's halfword 0 into LO.w0..3, the remainders into HI.w0..3. They write no GPR: rd keeps
/// its value (the translator passes it in and gets it back).
pub fn pdivw(c: &mut Cpu, s: Q, t: Q) -> Q {
    let (a, bb) = (w(s), w(t));
    for i in 0..2 {
        let (q, m) = crate::div32(a[2 * i] as i32, bb[2 * i] as i32);
        if i == 0 {
            (c.lo, c.hi) = (q as i64 as u64, m as i64 as u64);
        } else {
            (c.lo1, c.hi1) = (q as i64 as u64, m as i64 as u64);
        }
    }
    s
}
pub fn pdivuw(c: &mut Cpu, s: Q, t: Q) -> Q {
    let (a, bb) = (w(s), w(t));
    for i in 0..2 {
        let (q, m) = crate::divu32(a[2 * i], bb[2 * i]);
        if i == 0 {
            (c.lo, c.hi) = (q as i32 as i64 as u64, m as i32 as i64 as u64);
        } else {
            (c.lo1, c.hi1) = (q as i32 as i64 as u64, m as i32 as i64 as u64);
        }
    }
    s
}
pub fn pdivbw(c: &mut Cpu, s: Q, t: Q) -> Q {
    let a = w(s);
    let d = t.0 as u16 as i16 as i32;
    let mut lo = [0u32; 4];
    let mut hi = [0u32; 4];
    for i in 0..4 {
        let (q, m) = crate::div32(a[i] as i32, d);
        lo[i] = q as u32;
        hi[i] = m as i16 as i32 as u32;
    }
    set_lohi(c, lo, hi);
    s
}
