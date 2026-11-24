//! The water's VU1 program done by the port's own code: the program whose VIF packet is at 0x10F500 in the main program (588
//! instructions, 4,704 bytes of microcode; levels 4, 6, 8, 14, 17, 19, 27 and 28 run it from the moby chain: the water's, the
//! slime's and the swamp's surfaces). `mobies::Pass` hands its stream here instead of running it on a VU1.
//!
//! # What the chain sends it
//!
//! The pass's VIF stream: the microcode (MPG, three pieces), then per surface BASE 0 and OFFSET 0x1D2 (the two input buffers
//! TOPS 0 and 0x1D2), 12 quadwords of constants unpacked to 0x3A4 and MSCAL 0 (the program loads them: vf18..vf25 at 0x3A4,
//! vf31 at 0x3AC, vf29 0x3AD, vf30 0x3AE, vf07 0x3AF), and per batch of a triangle strip: its GIF tag to TOPS + 0 (NLOOP the
//! vertex count, PRIM a textured, Gouraud, fogged, blended strip, registers ST, RGBAQ, XYZF2), a GIF tag to 0x3AC (the same with
//! PRIM a triangle fan, its NLOOP 0: what clipped triangles are kicked with), and three quadwords a vertex from TOPS + 1 (STCYCL
//! 3/1): the texture coordinates (V2-32, z and w 1.0 from the column register), the colour (V4-8, unsigned) and the position
//! (V3-32, w 1.0), then MSCAL 0x70. GS packets the EE sends directly (DIRECT) come between the batches.
//!
//! # What the program does (entry 0x70, its code at 0x498)
//!
//! Each vertex's position goes through the matrix whose columns are vf22, vf23, vf24 (and vf25, the translation): clip
//! coordinates c. With Q = 1 / c.w the vertex is written over its input: the texture coordinates (s, t, 1) times Q, the colour as
//! it came, and the screen position (c.x, c.y, c.z) times Q times vf29 plus vf30, its w (the fog) c.w times vf29.w plus vf30.w
//! held between vf07.x and vf07.y (MINIy then MAXx for the first vertex, MAXx then MINIy for the others), all four by FTOI4. Each
//! vertex is judged against the frustum (CLIP: outside +x, -x, +y, -y, +z or -z of |c.w|), and from the second vertex on, the
//! triangle the vertex ends (it and the two judged before it: for the second vertex the one before the strip, the VU's clip
//! flags carrying over; the program judges one vertex past each strip's end, the next quadword taken for a position, so that one
//! is the last strip's) is looked at: when any of its three is outside a plane other than +z, the
//! vertex gets ADC (the GS does not draw that triangle of the strip: 2048.0 added to the fog before FTOI4 sets bit 15), and unless
//! all three are outside the same one of -z, -y, +y, -x, +x it is drawn clipped instead: when all three are past one side of the
//! screen (x or y beyond a third of c.w: the matrix makes the screen the middle third of the frustum, the frustum a guard band)
//! nothing is drawn, else the triangle is clipped against -z, -x, +x, -y and +y in turn (Sutherland-Hodgman: an edge from P to C
//! keeps C when inside and the crossing P + (C - P) d_P / (d_P - d_C) where it leaves or enters) and, when three corners or more
//! are left, kicked as a fan: a GIF tag with NLOOP 0 (the PRIM alone), then the tag with NLOOP the corner count and each corner as
//! the strip's vertices are made (texture coordinates and colour by FTOI0, no ADC). The fans go to the GS as they are made, the
//! strip after the last vertex. A corner is read back from the strip: its position, its colour by ITOF0, and its texture
//! coordinates as they are there: for the two older corners the ones already written (times Q) divided again by their third (so
//! (s, t, 1) to within rounding, and word 3 what the strip has), for the newest the input's (s, t, 1, 1).
//!
//! The GIF output is the same stream the VU would give: the DIRECT packets and the program's packets in their order (`vif.rs`
//! walks the stream), the arithmetic the VU's (`vufloat.rs`). Word 3 of the strip's ST quadwords is vf13.w or vf06.w, which the
//! program never sets: what an earlier VU1 program left there, 0 on a VU1 that has run nothing before, which is what the port
//! writes (the clipper interpolates it into the fans' word 3). The GS's PACKED ST takes S, T and Q from words 0 to 2 and ignores
//! word 3.

use crate::vif::{Micro, VuMem};
use crate::vufloat::rz::{add, clip, div, ftoi, itof0, max, min, mul, sub};

/// Four lanes, each `l(lane)`, and three: inlined at every use (an array's `map` with a closure was not, and cost a call a lane).
#[inline(always)]
fn lanes(l: impl Fn(usize) -> f32) -> [f32; 4] {
    [l(0), l(1), l(2), l(3)]
}
#[inline(always)]
fn lanes3(l: impl Fn(usize) -> f32) -> [f32; 3] {
    [l(0), l(1), l(2)]
}

/// The program's constants (entry 0): the matrix columns, the fan's GIF tag, the screen scale and offset, the fog's bounds.
#[derive(Clone, Copy, Default)]
struct Consts {
    m: [[f32; 4]; 4],
    vf29: [f32; 4],
    vf30: [f32; 4],
    vf07: [f32; 4],
}

fn f(w: u32) -> f32 {
    f32::from_bits(w)
}

/// CLIP's six flags of a position (bit 0 +x, 1 -x, 2 +y, 3 -y, 4 +z, 5 -z), against |w| with x and y scaled by nothing or, for
/// the screen's test, by three (MULi.xy by 3.0, then CLIP).
fn clip_flags(c: [f32; 4], xy: f32) -> u32 {
    if xy == 1.0 { clip(c) } else { clip([mul(c[0], xy), mul(c[1], xy), c[2], c[3]]) }
}

/// A clip-space corner of a triangle being clipped: position, texture coordinates and the colour as floats.
#[derive(Clone, Copy)]
struct Corner {
    p: [f32; 4],
    st: [f32; 4],
    c: [f32; 4],
}

/// P + (C - P) t, as the program does it (SUB, MULAq, MADDw).
fn lerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    lanes(|k| add(a[k], mul(sub(b[k], a[k]), t)))
}

/// The five planes the clipper uses, in its order: the axis, whether it is the near side (x >= -w: true) or the far side
/// (x <= w), and the CLIP flag it goes by.
const PLANES: [(usize, bool, u32); 5] = [(2, true, 1 << 5), (0, true, 1 << 1), (0, false, 1 << 0), (1, true, 1 << 3), (1, false, 1 << 2)];

/// Where the edge from P to C crosses a plane, as the program works it out: t = -d_P / (d_C - d_P), each step rounded.
fn crossing(p: [f32; 4], c: [f32; 4], axis: usize, near: bool) -> f32 {
    let da = sub(c[axis], p[axis]);
    let (num, den) = if near {
        (sub(sub(0.0, p[3]), p[axis]), add(sub(c[3], p[3]), da))
    } else {
        (sub(add(0.0, p[3]), p[axis]), add(sub(p[3], c[3]), da))
    };
    div(num, den)
}

/// The program as the VIF starts it: entry 0 loads the constants, entry 0x70 makes a strip.
#[derive(Default)]
pub struct Water {
    k: Consts,
}

impl Micro for Water {
    fn call(&mut self, entry: u32, mem: &mut VuMem, top: usize, clip: &mut u32, out: &mut Vec<[u32; 4]>) -> Result<(), String> {
        match entry {
            0x00 => {
                let q = |a: usize| mem[a].map(f);
                self.k = Consts { m: [q(0x3a8), q(0x3a9), q(0x3aa), q(0x3ab)], vf29: q(0x3ad), vf30: q(0x3ae), vf07: q(0x3af) };
                Ok(())
            }
            0x70 => strip(mem, top, &self.k, clip, out),
            e => Err(format!("water: a run at entry {e:#x} (only 0 and 0x70 are done)")),
        }
    }
}

/// Entry 0x70: the strip at `top` (its GIF tag, then three quadwords a vertex) made into the GS's packet in place, its clipped
/// triangles kicked as fans before it.
fn strip(mem: &mut VuMem, top: usize, k: &Consts, chain: &mut u32, out: &mut Vec<[u32; 4]>) -> Result<(), String> {
    let n = (mem[top][0] & 0x7fff) as usize;
    if n == 0 || top + 1 + 3 * n + 3 > mem.len() {
        return Err(format!("water: a strip of {n} vertices at {top:#x}"));
    }
    let fan_tag = mem[0x3ac];
    // MULAw, MADDAx, MADDAy, MADDz: the translation, then each column times its coordinate added.
    let transform = |x: [f32; 4]| -> [f32; 4] { lanes(|j| add(add(add(k.m[3][j], mul(k.m[0][j], x[0])), mul(k.m[1][j], x[1])), mul(k.m[2][j], x[2]))) };
    // MULq, then MULA by vf29 and MADDw of vf30; the fog (w) clamped by MINIy then MAXx (a fan's corners).
    let screen = move |p: [f32; 4], q: f32| -> [u32; 4] {
        let s = lanes3(|j| add(k.vf30[j], mul(mul(p[j], q), k.vf29[j])));
        let fog = max(min(add(k.vf30[3], mul(p[3], k.vf29[3])), k.vf07[1]), k.vf07[0]);
        [ftoi(s[0], 16.0), ftoi(s[1], 16.0), ftoi(s[2], 16.0), ftoi(fog, 16.0)]
    };
    // Each vertex: its clip position and its flags.
    let mut clip: Vec<[f32; 4]> = Vec::with_capacity(n);
    let mut flags: Vec<u32> = Vec::with_capacity(n);
    for v in 0..n {
        let base = top + 1 + 3 * v;
        let st = mem[base].map(f);
        let pos = mem[base + 2].map(f);
        let c = transform(pos);
        let q = div(1.0, c[3]);
        clip.push(c);
        flags.push(clip_flags(c, 1.0));
        *chain = ((*chain << 6) | flags[v]) & 0xff_ffff;
        let s = lanes3(|j| add(k.vf30[j], mul(mul(c[j], q), k.vf29[j])));
        let raw = add(k.vf30[3], mul(c[3], k.vf29[3]));
        let mut fog = if v == 0 { max(min(raw, k.vf07[1]), k.vf07[0]) } else { min(max(raw, k.vf07[0]), k.vf07[1]) };
        // The triangle this vertex ends: any of its corners outside (+z aside) gives the vertex ADC, and the triangle is drawn
        // clipped instead unless all three are outside the same plane. The test reads the VU's last three judgements (FCAND
        // 0x02FBEF): for the second vertex the oldest is the one before the strip.
        if v >= 1 {
            let (f0, f1, f2) = ((*chain >> 12) & 0x3f, (*chain >> 6) & 0x3f, *chain & 0x3f);
            if *chain & 0x02_fbef != 0 {
                fog = add(fog, 2048.0);
                let rejected = [1u32 << 5, 1 << 3, 1 << 2, 1 << 1, 1].iter().any(|&p| f0 & f1 & f2 & p != 0);
                if !rejected && v >= 2 {
                    // The corners as the clipper reads them back from the strip (the two older ones already written).
                    let corner = |u: usize| -> Corner {
                        let b = top + 1 + 3 * u;
                        let t = mem[b].map(f);
                        let st = if u == v {
                            t
                        } else {
                            let r = div(1.0, t[2]);
                            [mul(t[0], r), mul(t[1], r), mul(t[2], r), t[3]]
                        };
                        Corner { p: clip[u], st, c: mem[b + 1].map(itof0) }
                    };
                    fan(&[corner(v - 2), corner(v - 1), corner(v)], &clip[v - 2..=v], fan_tag, &screen, out);
                }
            }
        }
        mem[base] = [mul(st[0], q).to_bits(), mul(st[1], q).to_bits(), mul(st[2], q).to_bits(), 0];
        mem[base + 2] = [ftoi(s[0], 16.0), ftoi(s[1], 16.0), ftoi(s[2], 16.0), ftoi(fog, 16.0)];
    }
    // The program's pipeline transforms and judges one vertex past the strip (the quadword where a next vertex's position would
    // be): its judgement is the VU's last, the oldest of the three the next strip's second vertex is tested with.
    let past = transform(mem[top + 1 + 3 * n + 2].map(f));
    *chain = ((*chain << 6) | clip_flags(past, 1.0)) & 0xff_ffff;
    out.extend_from_slice(&mem[top..top + 1 + 3 * n]);
    Ok(())
}

/// The triangle of three corners (oldest first) clipped and kicked as a fan (the clipper at 0x838).
fn fan(tri: &[Corner], clip: &[[f32; 4]], tag: [u32; 4], screen: &dyn Fn([f32; 4], f32) -> [u32; 4], out: &mut Vec<[u32; 4]>) {
    // All three past one side of the screen (the middle third of the frustum in x and y): nothing to draw.
    let screen_flags: Vec<u32> = clip.iter().map(|&c| clip_flags(c, 3.0)).collect();
    if [1u32 << 3, 1 << 2, 1 << 1, 1].iter().any(|&p| screen_flags.iter().all(|f| f & p != 0)) {
        return;
    }
    let mut poly: Vec<Corner> = tri.to_vec();
    for &(axis, near, flag) in &PLANES {
        let mut next = Vec::with_capacity(poly.len() + 2);
        for j in 0..poly.len() {
            let (p, c) = (poly[(j + poly.len() - 1) % poly.len()], poly[j]);
            let (p_out, c_out) = (clip_flags(p.p, 1.0) & flag != 0, clip_flags(c.p, 1.0) & flag != 0);
            let cross = || {
                let t = crossing(p.p, c.p, axis, near);
                Corner { p: lerp(p.p, c.p, t), st: lerp(p.st, c.st, t), c: lerp(p.c, c.c, t) }
            };
            match (p_out, c_out) {
                (true, true) => {}
                (true, false) => {
                    next.push(cross());
                    next.push(c);
                }
                (false, true) => next.push(cross()),
                (false, false) => next.push(c),
            }
        }
        if next.len() < 3 {
            return;
        }
        poly = next;
    }
    // The PRIM alone (the tag with NLOOP 0), then the fan.
    out.push(tag);
    // The tag's first word as the program makes it: the low half of its x plus the count, sign-extended (MTIR, IADD, MFIR).
    let mut head = tag;
    head[0] = ((tag[0] as u16).wrapping_add(poly.len() as u16) as i16) as i32 as u32;
    out.push(head);
    for cr in &poly {
        let q = div(1.0, cr.p[3]);
        out.push([mul(cr.st[0], q), mul(cr.st[1], q), mul(cr.st[2], q), cr.st[3]].map(f32::to_bits));
        out.push(cr.c.map(|x| ftoi(x, 1.0)));
        out.push(screen(cr.p, q));
    }
}
