//! The sea's VU1 program done by the port's own code: the program whose VIF packet is at 0x116070 in the main program (MPG at
//! 0x116074, the microcode from 0x116078: 470 instructions, 3,760 bytes). Levels 2, 4, 5, 9, 12, 14, 17, 19 and 22 run it once a
//! frame from the moby chain: the open sea out to the horizon. `mobies::Pass` hands it the pieces of a pass that upload it.
//!
//! # What the chain sends it
//!
//! The microcode (two MPGs), FLUSHA, two DIRECT packets, STCYCL 4/4, the sea's 43 quadwords unpacked to 0 (V4-32) and MSCAL 0;
//! DIRECT packets after it. The quadwords, all in clip space (the screen is the middle third of the frustum, as for the water):
//!
//! - 0 to 3: two sets of four waves as complex numbers, each lane a wave: 0 and 2 the first set's real and imaginary parts, 1 and 3
//!   the second's (the amplitude is the length); 4 to 7 their turn from a vertex to the next along a row (4 and 6 the first set's
//!   cosine and sine, 5 and 7 the second's), 8 to 11 from a row to the next (8 and 10, 9 and 11); 29 to 32 how the turn along a row
//!   itself turns from a row to the next (29 and 31 the first set's, 30 and 32 the second's).
//! - 12 to 15: what each wave's real part gives the surface's two slopes (12 and 13 the first and second set's to slope A, 14
//!   and 15 to slope B); the imaginary parts summed are the height.
//! - 16, 18, 19.w: the first row's start (16 plus 18 times 19.w); 17.xy the step from a vertex to the next along a row (17.z how
//!   much that step grows from a row to the next); 21 the step of 19 from a row to the next, 35 that of 33.
//! - 33: the eye's direction at the row's start (34.xy its step along a row), 19 the texture coordinates (20.xy their step along
//!   a row); 24 the surface's up (a height of 1 moves a point by it); 22 the colour's scale; 25 and 26 the screen's scale and
//!   offset (their w the fog's), 38.zw the fog's bounds; 34.zw the colour's offset and its slope term.
//! - 27: the vertices of a row (x), the rows of the three stretches (y, z, w); 39 the clip flags that end a row (x on the first
//!   side, y on the second) and the pass (z); 28 the GIF tag the strips start with, 40 to 42 and 37 the A+D packets and the GIF tag
//!   of the reflection's strips, 36 their colour.
//!
//! # What the program does
//!
//! The surface is made row by row away from the eye, each row from a start point along a line to one side, then (the second
//! pass) everything again to the other side: the steps along a row negated and the turns along a row conjugated. Each pass kicks
//! the tag at 28 first, then makes rows in three stretches: 27.y rows with both sets of waves, the second set's amplitude scaled
//! from 1 down to 1 / 27.y; then 27.z rows (the row steps doubled, the first set's turns from a row to the next squared) with the
//! first set alone, scaled the same way; then 27.w flat rows (the steps a hundred times). Each row:
//!
//! - its start raised and lowered by a quarter of the up is judged against the frustum; past the far plane (+z) when raised ends
//!   the pass;
//! - each vertex: from its waves the slopes A and B and the height C (sums of the lanes), the position X = start + up C, the
//!   colour 2^23 + 22 (34.z + 33.w B + 34.w A) (the integer in the float's low bits), the texture coordinates 19 / X.w and the
//!   screen position 26 + 25 X / X.w (FTOI4; the fog from the row's start), and the eye's direction 33 reflected in the normal
//!   (A, B, 1), the reflection's texture coordinates: with D = 33.z + 33.y B + 33.x A and N = 1 + B^2 + A^2, (A D - 33.x N / 2,
//!   B D - 33.y N / 2, 2 (D - 33.z N / 2) + |33 N|). A vertex outside a plane 39 names ends the row after it.
//! - the rows are made in two buffers in turn (0x10A and 0x30A): the surface's strip upward from the buffer's start, the
//!   reflection's (the same positions, its own texture coordinates, the colour 36) downward from it, each pair of vertices the
//!   new row's vertex and the row before's at the same place. A row longer or shorter than the one before is evened out with
//!   the shorter one's last vertex. From the second row on the row is kicked with both strips (the reflection's first);
//!   a row that both starts and ends below the screen (39.z) ends the pass.
//!
//! The arithmetic is the VU's (`vufloat.rs`'s, done as "The arithmetic" below says), and the program's timing decides two things done here as it does them: the clip
//! flags read when a row starts are the lowered start's (the raised start's are not in yet), and the far-plane test is the raised
//! start's.
//!
//! The GIF output is the VU's but for word 3 of the reflection's ST quadwords where a row is made longer to match the row before:
//! the program stores a register whose w it never set there, the port 0. The GS's PACKED ST takes S, T and Q from words 0 to 2.

use crate::vif::{kick, Micro, VuMem};
use crate::vufloat::rz::itof0;

type V = [f32; 4];

// # The arithmetic
//
// The VU's arithmetic (`vufloat::rz`) takes an operand whose exponent is 255 as +-FLT_MAX and one that is denormal as 0, rounds
// every result toward zero and makes one below the smallest normal 0. Here a number is brought to that form once, as it is read
// from VU memory (`f`), and everything the program makes from numbers in that form stays in it: under the rounding the run holds
// (`vif::run`: toward zero, with flush-to-zero and denormals-are-zero) a sum, difference, product, quotient or root of such numbers
// is never infinite or NaN (an overflow is FLT_MAX), and a result below the smallest normal is 0 of its sign, as the VU has it.
// The host's own operations are then the VU's, so nothing is taken apart and put together again around each (that was most of
// the sea's time). Words copied from memory to memory are not numbers: they are moved as they are (`Mem::ld`, `st`).

/// A word of VU memory read as a number: exponent 255 is +-FLT_MAX, a denormal is 0 of its sign.
#[inline(always)]
fn f(w: u32) -> f32 {
    match w & 0x7f80_0000 {
        0x7f80_0000 => f32::from_bits((w & 0x8000_0000) | 0x7f7f_ffff),
        0 => f32::from_bits(w & 0x8000_0000),
        _ => f32::from_bits(w),
    }
}

/// ADD.
#[inline(always)]
fn add(a: f32, b: f32) -> f32 {
    a + b
}

/// SUB.
#[inline(always)]
fn sub(a: f32, b: f32) -> f32 {
    a - b
}

/// MUL.
#[inline(always)]
fn mul(a: f32, b: f32) -> f32 {
    a * b
}

/// DIV: a divisor of 0 gives +-FLT_MAX, the sign the two signs' xor.
#[inline(always)]
fn div(a: f32, b: f32) -> f32 {
    if b.to_bits() & 0x7f80_0000 == 0 {
        return f32::from_bits(((a.to_bits() ^ b.to_bits()) & 0x8000_0000) | 0x7f7f_ffff);
    }
    a / b
}

/// SQRT, of |a|.
#[inline(always)]
fn sqrt(a: f32) -> f32 {
    a.abs().sqrt()
}

/// MAX and MINI.
#[inline(always)]
fn max(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}

#[inline(always)]
fn min(a: f32, b: f32) -> f32 {
    if a < b { a } else { b }
}

/// FTOI0 / FTOI4 (`scale` 1 or 16): the value times the scale, truncated; past the integers' range the largest of its sign.
#[inline(always)]
fn ftoi(v: f32, scale: f32) -> u32 {
    let x = mul(v, scale);
    if x.to_bits() & 0x7f80_0000 <= 0x4e80_0000 {
        x as i32 as u32
    } else if x.is_sign_negative() {
        0x8000_0000
    } else {
        0x7fff_ffff
    }
}

/// CLIP's six judgements of (x, y, z) against |w|: bit 0 +x, 1 -x, 2 +y, 3 -y, 4 +z, 5 -z.
#[inline(always)]
fn clip(p: V) -> u32 {
    let w = p[3].abs();
    let mut c = 0;
    for l in 0..3 {
        let v = p[l];
        c |= ((v > w) as u32) << (2 * l) | ((v < -w) as u32) << (2 * l + 1);
    }
    c
}

/// Four lanes, each `l(lane)`: inlined at every use (an array's `map` with a closure was not, and cost a call a lane).
#[inline(always)]
fn lanes(l: impl Fn(usize) -> f32) -> V {
    [l(0), l(1), l(2), l(3)]
}

/// A complex product, lane by lane: (re, im) turned by (c, s) as the program does it (MULA, MSUB; MULA, MADD).
#[inline(always)]
fn turn(re: V, im: V, c: V, s: V) -> (V, V) {
    (lanes(|l| sub(mul(c[l], re[l]), mul(s[l], im[l]))), lanes(|l| add(mul(c[l], im[l]), mul(s[l], re[l]))))
}

#[inline(always)]
fn scale(v: V, s: f32) -> V {
    lanes(|l| mul(v[l], s))
}

/// The four lanes summed as ESUM does it.
#[inline(always)]
fn esum(v: V) -> f32 {
    add(add(add(v[0], v[1]), v[2]), v[3])
}

/// The two sets of waves at a vertex: the first set's real and imaginary parts, the second's.
#[derive(Clone, Copy)]
struct Waves {
    re1: V,
    im1: V,
    re2: V,
    im2: V,
}

/// What the program keeps in registers through a pass.
struct Regs {
    /// The turns along a row (vf12, vf13 the first set's cosine and sine; vf22, vf23 the second's).
    c1: V,
    s1: V,
    c2: V,
    s2: V,
    /// The steps along a row: of the position (vf26.xy; z how they grow from a row to the next), of the eye's direction (vf27.xy;
    /// zw the colour's offset and slope term) and of the texture coordinates (vf28.xy).
    step: V,
    eye_step: V,
    tex_step: V,
    /// The flags that end a row (vi12).
    row_mask: u32,
}

/// The constants (loaded once).
struct Consts {
    slope_a: [V; 2],
    slope_b: [V; 2],
    colour: V,
    up: V,
    screen_offset: V,
    screen_scale: V,
}

enum Row {
    Next,
    EndPass,
}

/// The program as the VIF starts it (entry 0, its only one).
pub struct Sea;

impl Micro for Sea {
    fn call(&mut self, entry: u32, mem: &mut VuMem, _top: usize, clip: &mut u32, out: &mut Vec<[u32; 4]>) -> Result<(), String> {
        if entry != 0 {
            return Err(format!("sea: a run at entry {entry:#x} (the program has only 0)"));
        }
        Run { clip }.run(mem, out)
    }
}

/// A run, with the VU's clip flags (`vif::Micro::call`).
struct Run<'a> {
    clip: &'a mut u32,
}

/// VU memory as the program addresses it (the address wraps at 1,024 quadwords).
struct Mem<'a>(&'a mut VuMem);

impl Mem<'_> {
    fn ld(&self, a: u32) -> [u32; 4] {
        self.0[(a & 0x3ff) as usize]
    }
    fn v(&self, a: u32) -> V {
        let q = self.ld(a);
        [f(q[0]), f(q[1]), f(q[2]), f(q[3])]
    }
    fn st(&mut self, a: u32, q: [u32; 4]) {
        self.0[(a & 0x3ff) as usize] = q;
    }
    /// A store of x, y and z, word 3 left as it is.
    fn st3(&mut self, a: u32, q: [u32; 4]) {
        self.0[(a & 0x3ff) as usize][..3].copy_from_slice(&q[..3]);
    }
    fn stv(&mut self, a: u32, v: V) {
        self.st(a, [v[0].to_bits(), v[1].to_bits(), v[2].to_bits(), v[3].to_bits()]);
    }
}

impl Run<'_> {
    fn judge(&mut self, p: V) {
        *self.clip = ((*self.clip << 6) | clip(p)) & 0xff_ffff;
    }

    fn run(&mut self, mem: &mut VuMem, out: &mut Vec<[u32; 4]>) -> Result<(), String> {
        let mut m = Mem(mem);
        let len = m.ld(27)[0] & 0xffff;
        if len == 0 {
            return Err("sea: rows of no vertices".into());
        }
        if m.ld(39)[2] & 0xfc0 != 0 {
            return Err(format!("sea: the pass's end tested against flags from before the run (39.z {:#x})", m.ld(39)[2] & 0xffff));
        }
        // The two buffers' heads: the reflection's A+D packet and the strip's tag, copied into both.
        let mut base = 0x10a;
        for (k, a) in [40, 41, 28].into_iter().enumerate() {
            let q = m.ld(a);
            m.st(base + k as u32, q);
            m.st(base + 0x200 + k as u32, q);
        }
        // The reflection's colour (36, alpha 0) in its vertices' colour quadwords, in both buffers.
        let fill = { let mut q = m.ld(36); q[3] = 0; q };
        for k in 0..2 * len {
            m.st(0x108u32.wrapping_sub(3 * k), fill);
            m.st(0x308u32.wrapping_sub(3 * k), fill);
        }
        // What the passes change, saved to be put back after each.
        const SAVED: [(u32, u32); 18] = [(2, 509), (0, 511), (3, 508), (1, 510), (8, 504), (10, 503), (16, 507), (33, 505), (19, 506), (18, 500), (35, 498), (21, 499), (9, 502), (11, 501), (29, 497), (31, 496), (30, 495), (32, 494)];
        for &(a, s) in &SAVED {
            let q = m.ld(a);
            m.st(s, q);
        }
        let k = Consts { slope_a: [m.v(12), m.v(13)], slope_b: [m.v(14), m.v(15)], colour: m.v(22), up: m.v(24), screen_offset: m.v(26), screen_scale: m.v(25) };
        let mut r = Regs { c1: m.v(4), s1: m.v(6), c2: m.v(5), s2: m.v(7), step: m.v(17), eye_step: m.v(34), tex_step: m.v(20), row_mask: m.ld(39)[0] & 0xffff };
        let mut flagged = 0xffdcu32; // vi08: where the head of the reflection's last strip was (none yet)
        for _ in 0..2 {
            kick(m.0, 0x1c, out)?;
            let mut prev = 0u32;
            self.pass(&mut m, &k, &mut r, &mut base, &mut prev, &mut flagged, len, out)?;
            // Put back what the pass changed; the second side's steps and turns.
            for &(a, s) in &SAVED {
                let q = m.ld(s);
                m.st(a, q);
            }
            let neg = |v: V| scale(v, -1.0);
            r.c1 = m.v(4);
            r.s1 = neg(m.v(6));
            r.c2 = m.v(5);
            r.s2 = neg(m.v(7));
            let (step, eye, tex) = (neg(m.v(17)), neg(m.v(34)), neg(m.v(20)));
            r.step = m.v(17);
            r.step[0] = step[0];
            r.step[1] = step[1];
            r.eye_step[0] = eye[0];
            r.eye_step[1] = eye[1];
            r.tex_step[0] = tex[0];
            r.tex_step[1] = tex[1];
            let (t31, t32) = (neg(m.v(31)), neg(m.v(32)));
            for (a, v) in [(17, r.step), (34, r.eye_step), (20, r.tex_step)] {
                let mut q = m.ld(a);
                q[0] = v[0].to_bits();
                q[1] = v[1].to_bits();
                m.st(a, q);
            }
            m.stv(31, t31);
            m.stv(32, t32);
            r.row_mask = m.ld(39)[1] & 0xffff;
        }
        Ok(())
    }

    /// One pass: the three stretches of rows, until the last or a row that ends the pass.
    #[allow(clippy::too_many_arguments)]
    fn pass(&mut self, m: &mut Mem, k: &Consts, r: &mut Regs, base: &mut u32, prev: &mut u32, flagged: &mut u32, len: u32, out: &mut Vec<[u32; 4]>) -> Result<(), String> {
        let zero = [0.0f32; 4];
        // The row's start, eye's direction and texture coordinates, from memory as each row begins.
        let starts = |m: &Mem| -> (V, V, V) {
            let (o, d, t) = (m.v(16), m.v(18), m.v(19));
            (lanes(|l| add(add(o[l], 0.0), mul(d[l], t[3]))), m.v(33), t)
        };
        // The amplitude of a stretch's row: what is left of the stretch over its length (vi06 / 27.y or 27.z).
        let fade = |n: u32, of: u32| div(itof0(n as u16 as i16 as i32 as u32), itof0(of));
        // Both sets of waves.
        let rows = m.ld(27)[1] & 0xffff;
        let mut n = rows;
        loop {
            let mut w = Waves { re1: m.v(0), re2: m.v(1), im1: m.v(2), im2: m.v(3) };
            let (p, e, t) = starts(m);
            let (a, b) = turn(m.v(0), m.v(2), m.v(8), m.v(10));
            m.stv(0, a);
            m.stv(2, b);
            let (a, b) = turn(m.v(1), m.v(3), m.v(9), m.v(11));
            m.stv(1, a);
            m.stv(3, b);
            let s = fade(n, m.ld(27)[1]);
            w.re2 = scale(w.re2, s);
            w.im2 = scale(w.im2, s);
            if let Row::EndPass = self.row(m, k, r, w, p, e, t, base, prev, flagged, len, out)? {
                return Ok(());
            }
            (r.c1, r.s1) = turn(r.c1, r.s1, m.v(29), m.v(31));
            (r.c2, r.s2) = turn(r.c2, r.s2, m.v(30), m.v(32));
            n = (n - 1) & 0xffff;
            if n == 0 {
                break;
            }
        }
        // The first set alone, its rows twice as far apart and its turns from a row to the next doubled.
        let mut q = m.ld(35);
        for l in 0..2 {
            q[l] = add(f(q[l]), f(q[l])).to_bits();
        }
        m.st(35, q);
        let mut q = m.ld(21);
        for l in [0, 1, 3] {
            q[l] = add(f(q[l]), f(q[l])).to_bits();
        }
        m.st(21, q);
        let (a, b) = turn(m.v(8), m.v(10), m.v(8), m.v(10));
        m.stv(8, a);
        m.stv(10, b);
        r.step[2] = add(r.step[2], r.step[2]);
        let (a, b) = turn(m.v(29), m.v(31), m.v(29), m.v(31));
        m.stv(29, a);
        m.stv(31, b);
        let rows = m.ld(27)[2] & 0xffff;
        let mut n = rows;
        loop {
            let mut w = Waves { re1: m.v(0), re2: zero, im1: m.v(2), im2: zero };
            let (p, e, t) = starts(m);
            let (a, b) = turn(m.v(0), m.v(2), m.v(8), m.v(10));
            m.stv(0, a);
            m.stv(2, b);
            let s = fade(n, m.ld(27)[2]);
            w.re1 = scale(w.re1, s);
            w.im1 = scale(w.im1, s);
            if let Row::EndPass = self.row(m, k, r, w, p, e, t, base, prev, flagged, len, out)? {
                return Ok(());
            }
            (r.c1, r.s1) = turn(r.c1, r.s1, m.v(29), m.v(31));
            n = (n - 1) & 0xffff;
            if n == 0 {
                break;
            }
        }
        // Flat rows out to the horizon, a hundred times as far apart.
        let mut q = m.ld(35);
        for l in 0..2 {
            q[l] = mul(f(q[l]), 100.0).to_bits();
        }
        m.st(35, q);
        let mut q = m.ld(21);
        for l in [0, 1, 3] {
            q[l] = mul(f(q[l]), 100.0).to_bits();
        }
        m.st(21, q);
        r.step[2] = mul(r.step[2], 100.0);
        let mut n = m.ld(27)[3] & 0xffff;
        loop {
            let w = Waves { re1: zero, re2: zero, im1: zero, im2: zero };
            let (p, e, t) = starts(m);
            if let Row::EndPass = self.row(m, k, r, w, p, e, t, base, prev, flagged, len, out)? {
                return Ok(());
            }
            n = (n - 1) & 0xffff;
            if n == 0 {
                break;
            }
        }
        Ok(())
    }

    /// A row (the subroutine at 0x8F8): its vertices into the buffer at `base`, both strips kicked when there was a row before.
    #[allow(clippy::too_many_arguments)]
    fn row(&mut self, m: &mut Mem, k: &Consts, r: &mut Regs, w0: Waves, p0: V, e0: V, t0: V, base: &mut u32, prev: &mut u32, flagged: &mut u32, len: u32, out: &mut Vec<[u32; 4]>) -> Result<Row, String> {
        // The next row's eye's direction and texture coordinates.
        let mut q = m.ld(33);
        let s = m.v(35);
        q[0] = add(e0[0], s[0]).to_bits();
        q[1] = add(e0[1], s[1]).to_bits();
        m.st(33, q);
        let mut q = m.ld(19);
        let s = m.v(21);
        for l in [0, 1, 3] {
            q[l] = add(t0[l], s[l]).to_bits();
        }
        m.st(19, q);
        let end_mask = m.ld(39)[2] & 0xffff;
        // The start lowered and raised by a quarter of the up, judged; the flags read are the lowered start's.
        let acc = lanes(|l| add(p0[l], 0.0));
        let lowered = lanes(|l| sub(acc[l], mul(k.up[l], 0.25)));
        let raised = lanes(|l| add(acc[l], mul(k.up[l], 0.25)));
        self.judge(lowered);
        let start_flags = end_mask & *self.clip & 0xfff;
        self.judge(raised);
        if *self.clip & 0x10 != 0 {
            return Ok(Row::EndPass);
        }
        let bounds = m.v(38);
        let fog = ftoi(max(min(add(add(k.screen_offset[3], 0.0), mul(k.screen_scale[3], p0[3])), bounds[2]), bounds[3]), 16.0);
        let big = 8_388_608.0f32;
        let colour_base = [add(0.0, big), add(0.0, big), add(0.0, big), add(1.0, big)];
        let eye_z = add(0.0, e0[2]);
        let half = 0.5f32;

        // The slopes and height of a vertex from its waves.
        let surface = |w: &Waves| -> (f32, f32, f32) {
            let pa = lanes(|l| add(mul(w.re1[l], k.slope_a[0][l]), mul(w.re2[l], k.slope_a[1][l])));
            let a = add(add(pa[0], pa[1]), add(pa[2], pa[3]));
            let pb = lanes(|l| add(mul(w.re1[l], k.slope_b[0][l]), mul(w.re2[l], k.slope_b[1][l])));
            let c = lanes(|l| add(w.im1[l], w.im2[l]));
            (a, esum(pb), esum(c))
        };
        let (mut w, mut p, mut e, mut t) = (w0, p0, e0, t0);
        let (mut a, mut b, mut c) = surface(&w);
        let (mut pp, mut rr) = (*base + 3, base.wrapping_sub(6));
        let mut count = 0u32;
        let stop = (2 * len) & 0xffff;
        let (mut screen, mut refl);
        loop {
            // D: the eye's direction along the normal (A, B, 1); colour term G; N = |(A, B, 1)|^2.
            let d = add(add(mul(eye_z, 1.0), mul(e[1], b)), mul(e[0], a));
            let g = add(add(mul(r.eye_step[2], 1.0), mul(e[3], b)), mul(r.eye_step[3], a));
            let nn = add(add(mul(1.0, 1.0), mul(b, b)), mul(a, a));
            count = (count + 2) & 0xffff;
            let prev_st = m.ld(pp + 0x203);
            let prev_refl = m.ld(rr.wrapping_add(0x200));
            m.st3(pp, prev_st);
            m.st3(rr.wrapping_add(3), prev_refl);
            let prev_colour = m.ld(pp + 0x204);
            let prev_xyzf = m.ld(pp + 0x205);
            m.st(pp + 1, prev_colour);
            let x = lanes(|l| add(add(p[l], 0.0), mul(k.up[l], c)));
            let colour = lanes(|l| add(colour_base[l], mul(k.colour[l], g)));
            m.stv(pp + 4, colour);
            let qx = div(1.0, x[3]);
            self.judge(x);
            m.st(pp + 2, prev_xyzf);
            m.st(rr.wrapping_add(5), prev_xyzf);
            let st = [mul(t[0], qx), mul(t[1], qx), mul(t[2], qx), 0.0];
            let pos_l = |l: usize| add(add(k.screen_offset[l], 0.0), mul(mul(x[l], k.screen_scale[l]), qx));
            let pos = [pos_l(0), pos_l(1), pos_l(2)];
            let ends = *self.clip & 0xfff & r.row_mask != 0;
            // The eye's direction times N, and its length.
            let en = [mul(e[0], nn), mul(e[1], nn), mul(e[2], nn)];
            let len2 = add(add(mul(en[0], en[0]), mul(en[1], en[1])), mul(en[2], en[2]));
            let rf = [mul(a, d), mul(b, d), mul(1.0, d)];
            let rf = [sub(rf[0], mul(en[0], half)), sub(rf[1], mul(en[1], half)), sub(rf[2], mul(en[2], half))];
            m.st3(pp + 3, [st[0].to_bits(), st[1].to_bits(), st[2].to_bits(), st[3].to_bits()]);
            screen = [ftoi(pos[0], 16.0), ftoi(pos[1], 16.0), ftoi(pos[2], 16.0), fog];
            refl = [rf[0], rf[1], add(add(rf[2], rf[2]), mul(1.0, sqrt(len2))), 0.0];
            m.st(pp + 5, screen);
            m.st(rr.wrapping_add(2), screen);
            m.st3(rr, [refl[0].to_bits(), refl[1].to_bits(), refl[2].to_bits(), refl[3].to_bits()]);
            pp += 6;
            rr = rr.wrapping_sub(6);
            if ends || count == stop {
                break;
            }
            // The next vertex.
            p[0] = add(p[0], r.step[0]);
            p[1] = add(p[1], r.step[1]);
            e[0] = add(e[0], r.eye_step[0]);
            e[1] = add(e[1], r.eye_step[1]);
            t[0] = add(t[0], r.tex_step[0]);
            t[1] = add(t[1], r.tex_step[1]);
            (w.re1, w.im1) = turn(w.re1, w.im1, r.c1, r.s1);
            (w.re2, w.im2) = turn(w.re2, w.im2, r.c2, r.s2);
            (a, b, c) = surface(&w);
        }
        // A row longer or shorter than the one before: the pairs past the shorter one get its last vertex.
        if *prev != 0 && count != *prev {
            let d = count.wrapping_sub(*prev) as u16 as i16;
            if d > 0 {
                let back = (3 * d as u32) & 0xffff;
                pp = pp.wrapping_sub(back);
                rr = rr.wrapping_add(back);
                let (v1, v2, v3, v4) = (m.ld(pp + 509), m.ld(pp + 510), m.ld(pp + 511), m.ld(rr.wrapping_add(518)));
                loop {
                    m.st(pp, v1);
                    pp += 6;
                    m.st(pp - 5, v2);
                    rr = rr.wrapping_sub(6);
                    m.st(pp - 4, v3);
                    *prev = (*prev + 2) & 0xffff;
                    m.st(rr.wrapping_add(9), v4);
                    m.st(rr.wrapping_add(11), v3);
                    if *prev == count {
                        break;
                    }
                }
            } else {
                let mut left = d as i32;
                let (v5, v7) = (m.ld(pp - 3), m.ld(pp - 2));
                loop {
                    m.st(pp + 3, v5);
                    m.st(pp + 4, v7);
                    m.st(pp + 5, screen);
                    pp += 6;
                    m.stv(rr, refl);
                    m.st(rr.wrapping_add(2), screen);
                    let v1 = m.ld(pp + 509);
                    rr = rr.wrapping_sub(6);
                    let (v2, v3, v9) = (m.ld(pp + 510), m.ld(pp + 511), m.ld(rr.wrapping_add(518)));
                    m.st(pp - 6, v1);
                    left += 2;
                    m.st(pp - 5, v2);
                    m.st(pp - 4, v3);
                    m.st(rr.wrapping_add(9), v9);
                    m.st(rr.wrapping_add(11), v3);
                    if left == 0 {
                        break;
                    }
                }
            }
        }
        // The reflection's head below its strip; both strips kicked from it when there was a row before.
        let head = rr.wrapping_add(3);
        let (h0, h1, h2) = (m.ld(40), m.ld(42), m.ld(37));
        m.st(head, h0);
        m.st(head.wrapping_add(1), h1);
        m.st(head.wrapping_add(2), h2);
        let refl_colour = m.ld(36);
        if *prev != 0 {
            let mut q = m.ld(head.wrapping_add(2));
            q[0] = *prev;
            m.st(head.wrapping_add(2), q);
            let mut q = m.ld(*base + 2);
            q[0] = (*prev + 0x8000) & 0xffff;
            m.st(*base + 2, q);
            kick(m.0, (head & 0x3ff) as usize, out)?;
        }
        *base ^= 0x200;
        *prev = count;
        // The reflection's colour back where the last head's A+D data went.
        m.st(*flagged, refl_colour);
        *flagged = head.wrapping_add(1);
        if *self.clip & 0xfff & start_flags != 0 {
            return Ok(Row::EndPass);
        }
        // The steps along a row grow for the next.
        let z = r.step[2];
        let (g17, g34, g20) = (m.v(17), m.v(34), m.v(20));
        for l in 0..2 {
            r.step[l] = add(add(r.step[l], 0.0), mul(g17[l], z));
            r.eye_step[l] = add(add(r.eye_step[l], 0.0), mul(g34[l], z));
            r.tex_step[l] = add(add(r.tex_step[l], 0.0), mul(g20[l], z));
        }
        Ok(Row::Next)
    }
}
