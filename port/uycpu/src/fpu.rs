//! The R5900 FPU's arithmetic and the VU's (they share it): single precision without infinities, NaNs or denormals. An operand
//! whose exponent is 255 is a number (read here as the largest finite one of its sign), a denormal operand is zero, results
//! round toward zero, an overflow gives the largest finite number and an underflow zero, a division by zero the largest number
//! with the quotient's sign. The game threads run with the x86's rounding set to toward-zero, denormals-are-zero and
//! flush-to-zero (`set_mode`), so the host's SSE arithmetic is the PS2's on the operands `fin` hands it.

/// FCR31's flags: I (invalid), D (division by zero) and their sticky copies SI, SD.
const FLAG_I: u32 = 1 << 17;
const FLAG_D: u32 = 1 << 16;
const STICKY_I: u32 = 1 << 6;
const STICKY_D: u32 = 1 << 5;

/// MXCSR for the game: round toward zero, DAZ, FTZ, every exception masked.
pub const MXCSR: u32 = 0x7f80 | 0x6000 | 0x0040 | 0x8000;

/// Sets the calling thread's MXCSR, returning the old one.
pub fn set_mode(m: u32) -> u32 {
    let mut old: u32 = 0;
    unsafe {
        std::arch::asm!("stmxcsr [{p}]", p = in(reg) &mut old, options(nostack));
        std::arch::asm!("ldmxcsr [{p}]", p = in(reg) &m, options(nostack, readonly));
    }
    old
}

/// An operand: exponent 255 is the largest finite number of its sign; a denormal is zero of its sign.
#[inline(always)]
pub fn fin(x: u32) -> f32 {
    let e = x & 0x7f80_0000;
    if e == 0x7f80_0000 {
        f32::from_bits((x & 0x8000_0000) | 0x7f7f_ffff)
    } else if e == 0 {
        f32::from_bits(x & 0x8000_0000)
    } else {
        f32::from_bits(x)
    }
}

/// A result: toward-zero rounding never makes an infinity from finite operands, and FTZ makes underflows zero; the bits are
/// kept as they come.
#[inline(always)]
fn fout(x: f32) -> u32 {
    x.to_bits()
}

#[inline(always)]
pub fn add(a: u32, b: u32) -> u32 {
    fout(fin(a) + fin(b))
}
#[inline(always)]
pub fn sub(a: u32, b: u32) -> u32 {
    fout(fin(a) - fin(b))
}
#[inline(always)]
pub fn mul(a: u32, b: u32) -> u32 {
    fout(fin(a) * fin(b))
}

/// The largest finite number with the sign of a / b.
#[inline(always)]
fn max_of(a: u32, b: u32) -> u32 {
    ((a ^ b) & 0x8000_0000) | 0x7f7f_ffff
}

pub fn div(c: &mut crate::Cpu, a: u32, b: u32) -> u32 {
    let (x, y) = (fin(a), fin(b));
    if y == 0.0 {
        c.fcr31 |= if x == 0.0 { FLAG_I | STICKY_I } else { FLAG_D | STICKY_D };
        return max_of(a, b);
    }
    fout(x / y)
}

pub fn sqrt(c: &mut crate::Cpu, b: u32) -> u32 {
    let y = fin(b);
    if y < 0.0 {
        c.fcr31 |= FLAG_I | STICKY_I;
    }
    if y == 0.0 {
        return b & 0x8000_0000;
    }
    fout(y.abs().sqrt())
}

pub fn rsqrt(c: &mut crate::Cpu, a: u32, b: u32) -> u32 {
    let (x, y) = (fin(a), fin(b));
    if y == 0.0 {
        c.fcr31 |= FLAG_D | STICKY_D;
        return max_of(a, b);
    }
    if y < 0.0 {
        c.fcr31 |= FLAG_I | STICKY_I;
    }
    fout(x / y.abs().sqrt())
}

#[inline(always)]
pub fn max(a: u32, b: u32) -> u32 {
    if fin(a) >= fin(b) { a } else { b }
}
#[inline(always)]
pub fn min(a: u32, b: u32) -> u32 {
    if fin(a) <= fin(b) { a } else { b }
}

/// CVT.W.S: truncation, saturating at the 32-bit limits.
#[inline(always)]
pub fn cvt_w(a: u32) -> u32 {
    let x = fin(a);
    if x >= 2147483648.0 {
        0x7fff_ffff
    } else if x < -2147483648.0 {
        0x8000_0000
    } else {
        x as i32 as u32
    }
}

#[inline(always)]
pub fn cvt_s(a: u32) -> u32 {
    fout(a as i32 as f32)
}

#[inline(always)]
pub fn eq(a: u32, b: u32) -> bool {
    fin(a) == fin(b)
}
#[inline(always)]
pub fn lt(a: u32, b: u32) -> bool {
    fin(a) < fin(b)
}
#[inline(always)]
pub fn le(a: u32, b: u32) -> bool {
    fin(a) <= fin(b)
}
