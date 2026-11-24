//! The effect layer: what VU1 program 6 draws (the game's depth-sorted effect pool: sprites such as Veldin's sea of sand
//! clouds, smoke and sparks, ground-plane quads, lines, ribbons), placed in the world and drawn by the native renderer.
//!
//! Each frame `uyafmt::prog6::draw` runs the frame's effect pool through the game's own cull, sort and item builder
//! (func_L01_0045E810) and program 6 (the port's own code of the microcode from the main program on the disc: `vu1p6`, run here
//! on VU1 state of the frame's own), from the inputs the level's own code names (`uyafmt::fxcode::addresses`),
//! and returns each GIF packet program 6 kicks. The packets are read here as the GS reads PACKED data:
//! - RGBAQ sets the vertex colour, and its Q is the Q that the last ST write held back (a PACKED ST stores S and T and keeps its
//!   Q for the next RGBAQ write), so a packet that writes RGBAQ before its STs (kind 0's sprites) draws with the Q of an earlier
//!   packet's last ST. The first packet of a frame takes the Q the GIF held when program 6 started (`mobies::HeldQ`: the EE's
//!   last PACKED ST before it, a pass's output, or the last moby run's last vertex); without one it is not drawn and is counted.
//! - XYZF2 kicks a vertex: X and Y in 1/16 pixel, Z bits 4 to 27 (FTOI4 leaves 4 fraction bits under them), F bits 4 to 11.
//!
//! A vertex is put back in the world with the program's own matrix M (the frame's `vu_matrix`, rows 0 to 2 times 1024, row 3
//! the translation) and camera position: the program outputs (X, Y, Z) = vf26.xyz + clip.xyz x Q with Q = vf25.x / clip.w and
//! clip = (world - camera, 1) . M (vf25 and vf26 are the program's constant quadwords 4 and 5, `vu_q4` and `vu_q5`: -0.0239 and
//! 2048, 2048, 8388112 on Veldin), so clip = (w / vf25.x) (X - vf26.x, Y - vf26.y, Z - vf26.z, vf25.x), and (world - camera, 1)
//! is that times M^-1: the w scale cancels when the result is divided by its fourth component. The GS interpolates S, T, Q,
//! colour and F linearly across the screen and samples at S / Q, T / Q; the native vertex carries (S d, T d, Q d), d the native
//! view depth, so the GPU's perspective-correct interpolation leaves exactly the screen-linear S, T and Q to divide.
//!
//! The pixel: the effect texture of the entry (`Core::effect_textures`, by its table index) or none (white), TEX1_1
//! 0xFF9000000120 (MXL 0: one level, linear) and CLAMP_1 5 (both axes clamped): the GS state when program 6 starts in the traced
//! frame; the packet's ALPHA_1 and PRIM (IIP flat or Gouraud, TME, FGE, ABE); TEST_1 0x53001 (func_L01_004010D0 sets it before
//! the call): the alpha test never passes and a failing pixel writes only its colour, so no effect writes depth; Z test GEQUAL.

use crate::native::Camera;
use crate::objects::{Blend, Draw, Key, Materials, ObjVertex};

pub(crate) struct Effects {
    /// The level's effect textures, then a white one (untextured primitives), with the alpha test that never passes.
    pub materials: Materials,
    white: u32,
    /// Cull/build/decode, translated program, vertex conversion, and material lookup (ms), when requested by the renderer.
    pub stages: [f64; 4],
    /// The last frame's drawn entries, by pool index: what each was (`Mark`) and its two points (+0x10, +0x20), for where each
    /// vertex was the frame before (`frame`'s `prev`).
    last: std::collections::HashMap<usize, (Mark, [f32; 3], [f32; 3])>,
}

/// What a pool entry is, to tell the same effect in the same slot the frame before from a new one there: its type (+0), its kind
/// (+1, bits 0-1) and its texture (+2).
type Mark = [u8; 3];

/// Pool entry `e`'s point at `at` (+0x10 the position, +0x20 a line's or a ribbon's second point).
fn point(e: &[u8; 64], at: usize) -> [f32; 3] {
    let f = |k: usize| f32::from_le_bytes(e[at + 4 * k..at + 4 * k + 4].try_into().expect("4 bytes"));
    [f(0), f(1), f(2)]
}

/// One GS vertex as a packet kicks it: ST and Q, RGBA, X and Y in pixels, Z, F.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GsVertex {
    pub s: f32,
    pub t: f32,
    pub q: f32,
    pub rgba: [u8; 4],
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub f: u32,
    /// The UV register (texels), for primitives with PRIM FST.
    pub u: f32,
    pub v: f32,
}

/// The frame's projection from the world to the GS screen (program 6's: the camera 4 x 4 M with rows 0 to 2 times 1024, vf25.x
/// and vf26 from its constant quadwords 4 and 5, the camera position). The EE's own primitives in the moby chain (the shadows)
/// use the same screen and depth (Veldin field 2400: Ratchet's shadow volume comes out on the ground under his feet).
pub(crate) struct Projection {
    m: [[f64; 4]; 4],
    mi: [[f64; 4]; 4],
    o: [f64; 4],
    k: f64,
    cam: [f64; 3],
    /// `ndc`'s view edges for the camera last asked (its eye, forward, up and tangents' bits): the same for every point of the
    /// frame, and two products with M each. A lock: the EE's triangles are put in the world on the crew's threads (`eedraw`).
    edges: std::sync::Mutex<Option<([u32; 11], f64, f64)>>,
    /// How many times the view's height the frame's is (`at_rest`'s frame across the picture's width; 1 otherwise): a primitive
    /// over the frame's whole width is placed up and down by it.
    pub fill: f64,
}

impl Projection {
    pub fn of(frame: &uyafmt::prog6::Frame) -> Result<Projection, String> {
        let m: [[f64; 4]; 4] = frame.vu_matrix.map(|r| r.map(|x| x as f64));
        let mi = crate::mobies::invert(m).ok_or("native: program 6's matrix has no inverse")?;
        let o = frame.vu_q5.map(|x| f32::from_bits(x) as f64);
        Ok(Projection { m, mi, o, k: f32::from_bits(frame.vu_q4[0]) as f64, cam: [0, 1, 2].map(|j| frame.cam[j] as f64), edges: std::sync::Mutex::new(None), fill: 1.0 })
    }

    /// The projection of a frame with no level's view (the front end's), for the camera at rest `cam` (its eye at the origin): the
    /// GS screen of the game's 512 x 416 frame about (2048, 2048), as the world programs' (`ndc` maps the frame across the picture,
    /// its 2D placed in the middle at its own scale by `eedraw`), and GS Z the inverse of the view depth d, Z = o.z + B / d, GS Z 0
    /// at FAR and the Z24 buffer's largest (2^24, `mobies::screen_walk` puts every Z in its range) at 0.99 NEAR: the HUD's and the
    /// menus' Z near the largest is nearer than NEAR (2D, as in a level's frames), the front end's VU1 programs' 3D is in the
    /// world at its depth. `frame`: the view's tangents across and up at the frame's own edges (pixels 0 and 512, 0 and 416): a
    /// screen point is put back in the world there (the inverse of the projection of those tangents), so the 3D is placed in the
    /// picture as the 2D about it is: in the middle at the frame's own scale (`frame` x `cam.k` times narrower than the view,
    /// `eedraw::unstretched` placing the 2D so), or the frame across the picture's width, its top and bottom past the picture's
    /// (`frame` up `fill` times the view's, `cam.k` 1: `ndc` places the 2D so).
    pub fn at_rest(cam: &Camera, frame: [f32; 2]) -> Projection {
        let (hx, hy) = (256.0, 208.0);
        let (f, u) = (cam.forward.map(|v| v as f64), cam.up.map(|v| v as f64));
        let r = [f[1] * u[2] - f[2] * u[1], f[2] * u[0] - f[0] * u[2], f[0] * u[1] - f[1] * u[0]];
        let (t0, f0, f1) = (cam.tan[0] as f64, frame[0] as f64, frame[1] as f64);
        let (near, far) = (0.99 * crate::native::NEAR as f64, crate::native::FAR as f64);
        let b = 16_777_216.0 / (1.0 / near - 1.0 / far);
        // Rows: the view vector's x, y, z, then 1; columns: X and Y before the division, B, and the view depth.
        let m = |sx: f64| -> [[f64; 4]; 4] { std::array::from_fn(|i| if i < 3 { [r[i] * sx, -u[i] * hy / f1, 0.0, f[i]] } else { [0.0, 0.0, b, 0.0] }) };
        let mi = crate::mobies::invert(m(hx / f0)).expect("the screen projection has an inverse: B is not 0 and right, up and forward are a basis");
        Projection { m: m(hx / t0), mi, o: [2048.0, 2048.0, -b / far, 0.0], k: 1.0, cam: cam.eye.map(|v| v as f64), edges: std::sync::Mutex::new(None), fill: f1 / cam.tan[1] as f64 }
    }

    /// The world position of the screen point (X, Y, Z): clip = (w / vf25.x) (X - vf26.x, Y - vf26.y, Z - vf26.z, vf25.x), and
    /// (world - camera, 1) = clip . M^-1, the w scale cancelling.
    #[inline]
    pub fn world(&self, x: f64, y: f64, z: f64) -> [f32; 3] {
        let c = [x - self.o[0], y - self.o[1], z - self.o[2], self.k];
        // Each column's sum in the order the iterator's sum takes it (it starts from -0.0, which adds nothing).
        let mi = &self.mi;
        let u = |j: usize| c[0] * mi[0][j] + c[1] * mi[1][j] + c[2] * mi[2][j] + c[3] * mi[3][j];
        let w = u(3);
        [(u(0) / w + self.cam[0]) as f32, (u(1) / w + self.cam[1]) as f32, (u(2) / w + self.cam[2]) as f32]
    }

    /// The screen X and Y of a world position.
    pub fn screen(&self, p: [f64; 3]) -> [f64; 2] {
        let r = [p[0] - self.cam[0], p[1] - self.cam[1], p[2] - self.cam[2], 1.0];
        let c: [f64; 4] = std::array::from_fn(|j| (0..4).map(|i| r[i] * self.m[i][j]).sum());
        let q = self.k / c[3];
        [self.o[0] + c[0] * q, self.o[1] + c[1] * q]
    }

    /// The native picture's normalised device coordinates of screen point (X, Y): the picture shows the camera's view, whose edges
    /// (the tangents across and up at view depth 1) are where the GS screen puts them.
    pub fn ndc(&self, cam: &Camera, x: f64, y: f64) -> [f32; 2] {
        let (ex, ey) = self.edges(cam);
        [((x - self.o[0]) / ex) as f32, ((y - self.o[1]) / ey) as f32]
    }

    /// The view's edges on the screen less the screen's offset, across (x) and up (y), for `ndc`: kept for the camera last asked.
    #[inline]
    fn edges(&self, cam: &Camera) -> (f64, f64) {
        let b = |v: f32| v.to_bits();
        let key = [b(cam.eye[0]), b(cam.eye[1]), b(cam.eye[2]), b(cam.forward[0]), b(cam.forward[1]), b(cam.forward[2]), b(cam.up[0]), b(cam.up[1]), b(cam.up[2]), b(cam.tan[0]), b(cam.tan[1])];
        if let Some((k, ex, ey)) = *self.edges.lock().expect("the projection's edges")
            && k == key
        {
            return (ex, ey);
        }
        let (f, u) = (cam.forward.map(|v| v as f64), cam.up.map(|v| v as f64));
        let r = [f[1] * u[2] - f[2] * u[1], f[2] * u[0] - f[0] * u[2], f[0] * u[1] - f[1] * u[0]];
        let e = cam.eye.map(|v| v as f64);
        let edge = |a: [f64; 3], t: f32| self.screen(std::array::from_fn(|j| e[j] + f[j] + a[j] * t as f64));
        let (ex, ey) = (edge(r, cam.tan[0]), edge(u, cam.tan[1]));
        let (ex, ey) = (ex[0] - self.o[0], ey[1] - self.o[1]);
        *self.edges.lock().expect("the projection's edges") = Some((key, ex, ey));
        (ex, ey)
    }
}

impl Effects {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, mut textures: Vec<uyafmt::core::Texture>, micro: Vec<u8>) -> Result<Effects, String> {
        if !crate::vu1::Prog::P6.is(&micro) {
            return Err("native: program 6's microcode on the disc is not the one the port's code of it was written from".into());
        }
        let white = textures.len() as u32;
        textures.push(uyafmt::core::Texture { width: 1, height: 1, levels: vec![vec![[0x80; 4]]], gs: None });
        let materials = Materials::new(device, queue, &textures, 0.0, 0)?.never_passing();
        Ok(Effects { materials, white, stages: [0.0; 4], last: Default::default() })
    }

    /// The frame's effect triangles, appended to `out` and `draws`. Returns the primitives drawn and, per reason, those not drawn.
    /// `held_q` is the GIF's held Q when the pass starts (`mobies::HeldQ`): the first packet's first RGBAQ takes it.
    #[allow(clippy::too_many_arguments)]
    /// `prev`: beside each vertex of `out`, where it was the frame before (`objects`' `vs_moby`, buffer 1: the pictures between
    /// frames draw it on its way, and DLSS's motion pass gives its motion): moved back by its entry's own move since the last
    /// frame (a line's or a ribbon's vertex by its nearer point's), itself for an entry new in its slot.
    pub fn frame(&mut self, device: &wgpu::Device, layout: &wgpu::BindGroupLayout, frame: &uyafmt::prog6::Frame, proj: &Projection, cam: &Camera, held_q: Option<f32>, out: &mut Vec<ObjVertex>, prev: &mut Vec<[f32; 3]>, draws: &mut Vec<Draw>, timed: bool) -> Result<(usize, Vec<(String, usize)>), String> {
        self.stages = [0.0; 4];
        let start = timed.then(std::time::Instant::now);
        let mut program_ms = 0.0;
        // Program 6 on VU1 state of the frame's own (`vu1::run` sets the game's MXCSR while it runs).
        let mut vu = crate::vu1::Vu1::new();
        // Program 6 fills its kicked buffer during the run; its consumer reads the finished packet below. Capturing at the
        // kick copies incomplete data (sometimes walks the whole memory looking for EOP), and that capture is never used.
        vu.capture_packets = false;
        let drawn = uyafmt::prog6::draw(frame, &mut |data: &[[u32; 4]], at: usize, entry: usize| {
            let start = timed.then(std::time::Instant::now);
            vu.mem[at..at + data.len()].copy_from_slice(data);
            vu.kicks.clear();
            vu.sent.clear();
            crate::vu1::run(&mut vu, crate::vu1::Prog::P6, entry).map_err(|e| format!("native: program 6 from {entry:#x}: {e}"))?;
            let packet = vu.kicks.last().map_or(Ok(Vec::new()), |&a| vu.packet(a));
            if let Some(start) = start {
                program_ms += start.elapsed().as_secs_f64() * 1000.0;
            }
            packet
        })?;
        if let Some(start) = start {
            self.stages[0] = start.elapsed().as_secs_f64() * 1000.0 - program_ms;
            self.stages[1] = program_ms;
        }
        let start = timed.then(std::time::Instant::now);
        let mut skipped: Vec<(String, usize)> = Vec::new();
        let mut skip = |why: String| match skipped.iter_mut().find(|(w, _)| *w == why) {
            Some((_, n)) => *n += 1,
            None => skipped.push((why, 1)),
        };
        // The GS's held-back Q: unknown until program 6's first ST.
        let mut held_q = held_q;
        let mut count = 0;
        // The packet's vertices and, for each, its world position and view depth: kept across the packets.
        let mut verts: Vec<GsVertex> = Vec::new();
        let mut placed: Vec<([f32; 3], f32)> = Vec::new();
        // The key of the material last got, which a packet of the same key does not need got again.
        let mut got: Option<Key> = None;
        // This frame's drawn entries (`last` for the next frame).
        let mut seen: std::collections::HashMap<usize, (Mark, [f32; 3], [f32; 3])> = std::collections::HashMap::new();
        for (run, p) in drawn.runs.iter().flat_map(|r| r.prims.iter().map(move |p| (r, p))) {
            // The packet as the GS reads it.
            let raw = run.raw_of(p);
            let tag = raw[0];
            let nloop = (tag[0] & 0x7fff) as usize;
            let nreg = match tag[1] >> 28 {
                0 => 16,
                n => n as usize,
            };
            verts.clear();
            let mut no_q = false;
            let (mut s, mut t, mut q, mut rgba) = (0f32, 0f32, None, [0u8; 4]);
            for l in 0..nloop {
                for r in 0..nreg {
                    let reg = if r < 8 { tag[2] >> (4 * r) & 15 } else { tag[3] >> (4 * (r - 8)) & 15 };
                    let d = raw[1 + l * nreg + r];
                    match reg {
                        1 => {
                            rgba = [d[0] as u8, d[1] as u8, d[2] as u8, d[3] as u8];
                            q = held_q;
                        }
                        2 => {
                            (s, t) = (f32::from_bits(d[0]), f32::from_bits(d[1]));
                            held_q = Some(f32::from_bits(d[2]));
                        }
                        4 => {
                            no_q |= q.is_none();
                            verts.push(GsVertex { s, t, q: q.unwrap_or(0.0), rgba, x: (d[0] & 0xffff) as f64 / 16.0, y: (d[1] & 0xffff) as f64 / 16.0, z: d[2] as f64 / 16.0, f: d[3] >> 4 & 0xff, u: 0.0, v: 0.0 });
                        }
                        _ => {}
                    }
                }
            }
            if no_q {
                skip("its GS Q is from before program 6".into());
                continue;
            }
            let prim = p.prim;
            // GS PRIM: type (bits 0-2; 4 a triangle strip), IIP (3), TME (4), FGE (5), ABE (6), FST (8).
            if prim & 7 != 4 {
                skip(format!("GS primitive type {} (PRIM {prim:#x})", prim & 7));
                continue;
            }
            if prim & 0x100 != 0 {
                skip(format!("PRIM {prim:#x} uses UV"));
                continue;
            }
            let blend = if prim & 0x40 == 0 {
                Blend::Replace
            } else {
                let a = |sh: u32| ((p.alpha >> sh) & 3) as u8;
                match Blend::of(a(0), a(2), a(4), a(6), (p.alpha >> 32) as u8) {
                    Ok(b) => b,
                    Err(e) => {
                        skip(e);
                        continue;
                    }
                }
            };
            let texture = if prim & 0x10 == 0 {
                self.white
            } else {
                let (Some(tex0), Some(tx)) = (p.tex0, &p.texture) else {
                    skip(format!("PRIM {prim:#x} is textured and the packet sets no TEX0_1"));
                    continue;
                };
                // TEX0 TCC (bit 34) RGBA and TFX (bits 35-36) MODULATE, as the shader samples.
                if tex0 >> 34 & 7 != 1 {
                    skip(format!("TEX0_1 TCC {} TFX {}", tex0 >> 34 & 1, tex0 >> 35 & 3));
                    continue;
                }
                if tx.index as u32 >= self.white {
                    skip(format!("effect texture {} of the level's {}", tx.index, self.white));
                    continue;
                }
                tx.index as u32
            };
            // CLAMP_1 5 (clamp both), TEX1_1 MXL 0 (level 0, linear: MMIN 1 samples the same).
            let key: Key = (texture, true, true, 1, 0f32.to_bits(), blend, 0);
            if got != Some(key) {
                let start = timed.then(std::time::Instant::now);
                self.materials.get(device, layout, key)?;
                if let Some(start) = start {
                    self.stages[3] += start.elapsed().as_secs_f64() * 1000.0;
                }
                got = Some(key);
            }
            if draws.last().is_none_or(|dr| dr.key != key) {
                draws.push(Draw { key, first: out.len() as u32, count: 0 });
            }
            // The entry's move since the last frame: each point's, where the slot holds the same effect.
            let e = frame.pool.get(p.entry).ok_or_else(|| format!("native: program 6 drew pool entry {} of {}", p.entry, frame.pool.len()))?;
            let mark: Mark = [e[0], e[1] & 3, e[2]];
            let (a, b) = (point(e, 0x10), point(e, 0x20));
            seen.insert(p.entry, (mark, a, b));
            let moved = |now: [f32; 3], was: [f32; 3]| [now[0] - was[0], now[1] - was[1], now[2] - was[2]];
            let (da, db) = match self.last.get(&p.entry) {
                Some(&(m, la, lb)) if m == mark => (moved(a, la), moved(b, lb)),
                _ => ([0.0; 3], [0.0; 3]),
            };
            let line = mark[1] >= 2;
            let near = |pos: [f32; 3], q: [f32; 3]| (0..3).map(|k| (pos[k] - q[k]).powi(2)).sum::<f32>();
            // Each vertex is in up to three triangles of the strip and is put in the world once.
            placed.clear();
            placed.extend(verts.iter().map(|v| {
                let pos = proj.world(v.x, v.y, v.z);
                (pos, (0..3).map(|a| (pos[a] - cam.eye[a]) * cam.forward[a]).sum::<f32>())
            }));
            out.reserve(3 * verts.len().saturating_sub(2));
            for i in 2..verts.len() {
                // IIP 0 (flat): the triangle takes the colour of the vertex that kicks it.
                for j in [i - 2, i - 1, i] {
                    let v = &verts[j];
                    let (pos, d) = placed[j];
                    let colour = if prim & 8 == 0 { verts[i].rgba } else { v.rgba };
                    // FGE 0: no fog (F above 255 tells the shader so).
                    let f = if prim & 0x20 == 0 { 256.0 } else { v.f as f32 };
                    out.push(ObjVertex { pos, st: [v.s * d, v.t * d, v.q * d], colour, f });
                    let dm = if line && near(pos, b) < near(pos, a) { db } else { da };
                    prev.push([pos[0] - dm[0], pos[1] - dm[1], pos[2] - dm[2]]);
                }
            }
            let dr = draws.last_mut().expect("pushed above");
            dr.count = out.len() as u32 - dr.first;
            count += 1;
        }
        if let Some(start) = start {
            self.stages[2] = start.elapsed().as_secs_f64() * 1000.0 - self.stages[3];
        }
        self.last = seen;
        Ok((count, skipped))
    }
}
