//! The VU's float arithmetic bit for bit as rac-rs's VU1 (its instrument) does it: an operand whose exponent is 255 is +-FLT_MAX
//! and a denormal one is 0 (its sign kept); every operation rounds toward zero, so nothing overflows (past FLT_MAX is FLT_MAX); a
//! result below the smallest normal is 0. The VU1 programs the port does itself (`vuwater.rs`, `vusea.rs`) use `rz`'s; ADD and
//! SUB here are for a caller outside such a program's run (`tievu.rs`), done exactly with f64: a sum of two f32 is its rounded
//! value plus the error (two-sum), so the cut toward zero is never a second rounding.

const MAXF: u32 = 0x7f7f_ffff;

/// An operand as the VU reads it.
fn operand(x: f32) -> f64 {
    let b = x.to_bits();
    match b & 0x7f80_0000 {
        0x7f80_0000 => f32::from_bits((b & 0x8000_0000) | MAXF) as f64,
        0 => f32::from_bits(b & 0x8000_0000) as f64,
        _ => x as f64,
    }
}

/// The exact value `hi + rest` (`hi` the exact value rounded to f64; only `rest`'s sign is used) cut toward zero to f32.
fn toward_zero(hi: f64, rest: f64) -> f32 {
    let mut f = hi as f32;
    if f.is_infinite() {
        f = f32::MAX.copysign(f);
    }
    let fd = f as f64;
    let beyond = (hi > 0.0 && (fd > hi || (fd == hi && rest < 0.0))) || (hi < 0.0 && (fd < hi || (fd == hi && rest > 0.0)));
    if beyond {
        f = f32::from_bits(f.to_bits() - 1);
    }
    if f.abs() < f32::MIN_POSITIVE { 0.0f32.copysign(f) } else { f }
}

/// ADD.
pub fn add(a: f32, b: f32) -> f32 {
    let (x, y) = (operand(a), operand(b));
    let s = x + y;
    let bb = s - x;
    toward_zero(s, (x - (s - bb)) + (y - bb))
}

/// SUB.
pub fn sub(a: f32, b: f32) -> f32 {
    add(a, -b)
}

/// The VU's operations in the host's single precision, for the programs that run under `rz::Mode` (`vif::run`): the water's and
/// the sea's. ADD and SUB above stay for a caller outside such a run (`tievu.rs`).
pub mod rz {
    //! The VU's float arithmetic, for the VU1 programs the port does itself (`vuwater.rs`, `vusea.rs`), bit for bit as rac-rs's VU1
    //! (its instrument) does it: an operand whose exponent is 255 is +-FLT_MAX and a denormal one is 0 (its sign kept); every
    //! operation rounds toward zero, so nothing overflows (past FLT_MAX is FLT_MAX); a result below the smallest normal is 0.
    //!
    //! Done in the host's own single precision with the rounding toward zero (`Mode`, which a program's run holds: `vif::run`): an
    //! IEEE 754 sum, difference, product, quotient or square root is the exact value rounded in the rounding direction, so in this
    //! mode it is the exact value cut toward zero, as the VU does it. The operands are taken as the VU takes them (`operand`) and a
    //! result below the smallest normal is made 0 (`flush`) here, in plain bit tests, so nothing depends on the denormal modes or on
    //! how the compiler folds an operation on constants (it folds only exact ones: x * 1, x + 0 of an operand already taken).
    //! (The f64 version before it, the exact value by two-sum and a fused multiply-add's remainder, gave the same words and took 3 to
    //! 10 ms of the frame on the sea's levels.)

    const MAXF: u32 = 0x7f7f_ffff;

    /// The x86's rounding toward zero (MXCSR RC 3, every exception masked, flush-to-zero and denormals-are-zero as well), held while
    /// it lives and the caller's mode put back after.
    pub struct Mode(u32);

    impl Mode {
        pub fn toward_zero() -> Mode {
            Mode(uyart::fpu::set_mode(0x7f80 | 0x6000 | 0x0040 | 0x8000))
        }
    }

    impl Drop for Mode {
        fn drop(&mut self) {
            uyart::fpu::set_mode(self.0);
        }
    }

    /// An operand as the VU reads it: exponent 255 is +-FLT_MAX, a denormal is 0 of its sign.
    #[inline(always)]
    fn operand(x: f32) -> f32 {
        let b = x.to_bits();
        match b & 0x7f80_0000 {
            0x7f80_0000 => f32::from_bits((b & 0x8000_0000) | MAXF),
            0 => f32::from_bits(b & 0x8000_0000),
            _ => x,
        }
    }

    /// A result below the smallest normal is 0 of its sign (the rounding toward zero never takes a result to infinity).
    #[inline(always)]
    fn flush(x: f32) -> f32 {
        let b = x.to_bits();
        if b & 0x7f80_0000 == 0 { f32::from_bits(b & 0x8000_0000) } else { x }
    }

    /// ADD.
    #[inline(always)]
    pub fn add(a: f32, b: f32) -> f32 {
        flush(operand(a) + operand(b))
    }

    /// SUB.
    #[inline(always)]
    pub fn sub(a: f32, b: f32) -> f32 {
        flush(operand(a) - operand(b))
    }

    /// MUL.
    #[inline(always)]
    pub fn mul(a: f32, b: f32) -> f32 {
        flush(operand(a) * operand(b))
    }

    /// DIV: a divisor of 0 (or a denormal) gives +-FLT_MAX, the sign the two signs' xor.
    #[inline(always)]
    pub fn div(a: f32, b: f32) -> f32 {
        if b.to_bits() & 0x7f80_0000 == 0 {
            return f32::from_bits(((a.to_bits() ^ b.to_bits()) & 0x8000_0000) | MAXF);
        }
        flush(operand(a) / operand(b))
    }

    /// SQRT, of |a|.
    #[inline(always)]
    pub fn sqrt(a: f32) -> f32 {
        let b = a.to_bits() & 0x7fff_ffff;
        if b & 0x7f80_0000 == 0 {
            return 0.0;
        }
        flush(operand(f32::from_bits(b)).sqrt())
    }

    /// MAX and MINI: compared with denormals as 0, the operand itself given back.
    #[inline(always)]
    pub fn max(a: f32, b: f32) -> f32 {
        if operand(a) > operand(b) { a } else { b }
    }
    #[inline(always)]
    pub fn min(a: f32, b: f32) -> f32 {
        if operand(a) < operand(b) { a } else { b }
    }

    /// FTOI0 / FTOI4 (`scale` 1 or 16): the value times the scale, truncated; past the integers' range the largest of its sign.
    #[inline(always)]
    pub fn ftoi(v: f32, scale: f32) -> u32 {
        let x = mul(v, scale);
        if x.to_bits() & 0x7f80_0000 <= 0x4e80_0000 {
            x as i32 as u32
        } else if x.is_sign_negative() {
            0x8000_0000
        } else {
            0x7fff_ffff
        }
    }

    /// ITOF0: the integer rounded toward zero (an i32 to f32 conversion rounds in the mode held).
    #[inline(always)]
    pub fn itof0(w: u32) -> f32 {
        w as i32 as f32
    }

    /// CLIP's six judgements of (x, y, z) against |w|: bit 0 +x, 1 -x, 2 +y, 3 -y, 4 +z, 5 -z.
    #[inline(always)]
    pub fn clip(p: [f32; 4]) -> u32 {
        let w = operand(p[3]).abs();
        let mut c = 0;
        for l in 0..3 {
            let v = operand(p[l]);
            c |= ((v > w) as u32) << (2 * l) | ((v < -w) as u32) << (2 * l + 1);
        }
        c
    }
}
