//! Instanced world objects in the port's renderer (PLAN item 1.1): the shrubs, as func_L01_004A2200 and VU1 programs 4 and 5
//! draw them, and the ties, as func_L01_004AFA28, func_L01_004B0990 and VU1 program 2 draw them (see `Ties`). Each frame the game's own shrub arrays in RAM say which instances are drawn, where (the matrix array) and in
//! which colours (the per-instance colour blocks its light pass fills); the classes' geometry and textures come from `uyafmt`.
//!
//! - Which instances: func_L01_004A2200's cull loop (0x4A2538): an instance whose +0x16 bit 0 is set is skipped; otherwise it is
//!   drawn when its centre's view depth is below the smaller of its draw distance (+0x10) and the level's limit (gp+0x45A8), and
//!   its sphere (+0 centre, +0xC radius) is not wholly behind the eye. The view itself needs no test: the GPU clips.
//! - Where: the instance's matrix (64 bytes from [gp+0x45A0]) applied to the vertex's position in 1/1024 units times the class's
//!   scale in row 3's w (x row 0 + y row 1 + z row 2 + row 3; program 4's per-frame matrix carries the 1/1024 and the scale).
//! - Colours: the instance's 24 RGBA words at [gp+0x45A4] + 96 x its +0x18 index; a vertex's colour index picks one, and every
//!   vertex takes the alpha of word 0 (the record's +5 quadword w that program 4 sends as A).
//! - The pixel: each strip's texture entry (TEX0 from the class record's texture byte, TEX1 K and MMIN, CLAMP), PRIM 0x7C (Gouraud,
//!   textured, fogged, blended), ALPHA_1 0x44 and TEST_1 0x5340B (alpha test GEQUAL 0x40, a failing pixel writes its colour but
//!   not its depth). The mip level: program 4's Q is 1 / w, w a 32nd of the view depth. The fog value F is the instance's, from
//!   its origin's view depth (vf21.w), as the terrain's.

use crate::native::{Camera, MatU};
use crate::replay::Enc;
use std::collections::HashMap;

/// A hash map whose keys are hashed by `FastHash`: the per-draw and per-frame maps (std's SipHash of a texture setting cost
/// every one of the shrubs' 20,000 draws a frame a lookup's worth of hashing).
pub(crate) type FastMap<K, V> = HashMap<K, V, std::hash::BuildHasherDefault<FastHash>>;

/// The multiply-rotate hash rustc's own maps use (FxHash): fast on the small integers and tuples of them these maps key by.
#[derive(Default, Clone, Copy)]
pub(crate) struct FastHash(u64);

impl std::hash::Hasher for FastHash {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }
    fn write_u8(&mut self, n: u8) {
        self.write_u64(n as u64);
    }
    fn write_u32(&mut self, n: u32) {
        self.write_u64(n as u64);
    }
    fn write_usize(&mut self, n: usize) {
        self.write_u64(n as u64);
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x51_7cc1_b727_220a_95);
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ObjVertex {
    pub pos: [f32; 3],
    /// S, T and Q as the program sends them before its 1/w (the GS samples at S/Q, T/Q).
    pub st: [f32; 3],
    pub colour: [u8; 4],
    /// The GS fog value F; above 255 the primitive has FGE off (no fog).
    pub f: f32,
}

/// A shrub's vertex in the level's built shrubs (`Shrubs::frame`): an `ObjVertex` whose fog value is its instance's, by number
/// (`vs_shrub` reads it from the frame's values).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ShrubVertex {
    pub pos: [f32; 3],
    pub st: [f32; 3],
    pub colour: [u8; 4],
    pub instance: u32,
}

pub(crate) const SHADER: &str = r#"
enable dual_source_blending;
// `lerp.x`: how far back toward the frame before a moby vertex is drawn (`vs_moby`; 0 in the frame's own picture); `lerp.y`: 1
// with the per-pixel lighting (`lit`). `sets`: the level's light sets as the game has them this frame (16 of 4 rows: colour 0,
// direction 0, colour 1, direction 1; `native::Native::render` writes them after the view).
struct View { m: mat4x4<f32>, sky: mat4x4<f32>, eye: vec4<f32>, forward: vec4<f32>, fog_colour: vec4<f32>, fog: vec4<f32>, lerp: vec4<f32>, sets: array<vec4<f32>, 64> };
struct Mat { k: f32, last: f32, qlog: f32, aref: f32, region: vec4<f32>, extra: vec4<f32>, rift: vec4<f32>, tiles: vec4<f32>, tiles3: vec4<f32>, grains: vec4<f32>, specs: vec4<f32>, refls: vec4<f32>, furs: vec4<f32>, catches: vec4<f32>, metals: vec4<f32>, emit: vec4<f32>, relief: vec4<f32> , lodq: vec4<f32> };
// The destination of a GS blend no blend state can do (`Blend::Gs`): a copy of the buffer drawn into.
@group(2) @binding(0) var dst: texture_2d<f32>;
// One picture's motion and jitter (`dlss::MotionU`): the mobies' motion pass reads the cameras without the jitter, the share each
// picture draws a moby vertex back toward frame N - 1 at, and the render size.
struct Motion { cur: mat4x4<f32>, prev: mat4x4<f32>, r: vec4<f32>, u: vec4<f32>, f: vec4<f32>, eye: vec4<f32>, tan: vec4<f32>, jit: vec4<f32>, back: vec4<f32> };
@group(2) @binding(6) var<uniform> motion: Motion;
@group(0) @binding(0) var<uniform> view: View;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var smp: sampler;
@group(1) @binding(2) var<uniform> mat: Mat;
// A Rift Apart surface's material map and its sampler (`lighting::MATERIAL`), read when `mat.rift.w` is 1 or more; a surface of
// regions' (`mat.rift.w` 2: a character's atlas) region map and other materials' maps.
@group(1) @binding(3) var mtl: texture_2d<f32>;
@group(1) @binding(4) var msmp: sampler;
@group(1) @binding(5) var rgn: texture_2d<f32>;
@group(1) @binding(6) var mtl1: texture_2d<f32>;
@group(1) @binding(7) var mtl2: texture_2d<f32>;
@group(1) @binding(8) var mtl3: texture_2d<f32>;
// The texture's relief map (`texnames` `relief.tsv`, `lighting::MATERIAL` `material_relief`), read when `mat.relief.x` is above 0.
@group(1) @binding(9) var rlf: texture_2d<f32>;

struct VIn { @location(0) pos: vec3<f32>, @location(1) st: vec3<f32>, @location(2) colour: vec4<f32>, @location(3) f: f32 };
struct VOut {
    // Colour, occluder and surface pipelines share an exact-equal depth test.
    @builtin(position) @invariant clip: vec4<f32>,
    @location(0) st: vec3<f32>,
    @location(1) @interpolate(linear) colour: vec4<f32>,
    @location(2) depth: f32,
    @location(3) @interpolate(linear) f: f32,
    // The per-pixel lighting's inputs (`lit`): the light terms at the vertex, interpolated as the colour is, the normal and the
    // light word (`lighting::LIT` clear: none).
    @location(4) @interpolate(linear) ls: vec2<f32>,
    @location(5) @interpolate(linear) ln: vec4<f32>,
    @location(6) @interpolate(flat) light: u32,
    // The world position (a Rift Apart material's frame at the pixel, `lighting::MATERIAL`; the ties' and the mobies').
    @location(7) wpos: vec3<f32>,
    // A fur shell's height, a share of the longest fur (`vs_moby_fur`; 0 for every other draw).
    @location(8) @interpolate(flat) shell: f32,
    // The mobies' motion pass (`vs_moby_motion`, `dlss.rs`): the vertex where this picture's camera and the picture before's see
    // it, without the jitter (0 in every other draw; location 9 is the vegetation batch's material, `shrub_batch.rs`).
    @location(10) mc: vec4<f32>,
    @location(11) mp: vec4<f32>,
};

@vertex fn vs(v: VIn) -> VOut {
    var o: VOut;
    o.clip = view.m * vec4<f32>(v.pos, 1.0);
    o.st = v.st;
    o.colour = v.colour;
    o.depth = dot(v.pos - view.eye.xyz, view.forward.xyz);
    o.f = v.f;
    o.wpos = v.pos;
    return o;
}

// The mobies of the native picture: each vertex with where the frame before drew it (`prev`; itself for a run that frame did
// not have), drawn `view.lerp.x` of the way back there (`replay.rs`). At 0 it is `vs` exactly.
struct VInMoby {
    @location(0) pos: vec3<f32>, @location(1) st: vec3<f32>, @location(2) colour: vec4<f32>, @location(3) f: f32, @location(4) prev: vec3<f32>,
    @location(5) ln: vec4<f32>, @location(6) ls: vec2<f32>, @location(7) light: u32,
};
@vertex fn vs_moby(v: VInMoby) -> VOut {
    return moby_out(v);
}

// The same vertex for the mobies' motion pass (`dlss.rs`): at the same place in the picture (its depth equal to the picture's),
// with where this picture and the picture before put it, by their cameras without the jitter.
@vertex fn vs_moby_motion(v: VInMoby) -> VOut {
    var o = moby_out(v);
    o.mc = motion.cur * vec4<f32>(v.pos + (v.prev - v.pos) * motion.back.x, 1.0);
    o.mp = motion.prev * vec4<f32>(v.pos + (v.prev - v.pos) * motion.back.y, 1.0);
    return o;
}

fn moby_out(v: VInMoby) -> VOut {
    var o: VOut;
    let p = v.pos + (v.prev - v.pos) * view.lerp.x;
    o.clip = view.m * vec4<f32>(p, 1.0);
    o.st = v.st;
    o.colour = v.colour;
    o.depth = dot(p - view.eye.xyz, view.forward.xyz);
    o.f = v.f;
    o.ls = v.ls;
    o.ln = v.ln;
    o.light = v.light;
    o.wpos = p;
    return o;
}

// The ties, kept on the GPU (`Ties::frame`): each triangle's corners as the instance draws them, each with its vertex's slot of the
// frame's colour values (bit 31: a second draw's corner, its coordinates the frame's at `st2`). Fog is evaluated on the GPU.
// A first-draw triangle the frame does
// not draw (its bit clear) has its corners at one point outside the picture: no pixel. Every other value is the vertex's as it was
// made each frame before, so `vs` gave the same.
@group(2) @binding(3) var<storage, read> tie_drawn: array<u32>;
@group(2) @binding(4) var<storage, read> tie_st: array<vec2<f32>>;
@group(2) @binding(5) var<storage, read> tie_colours: array<u32>;
// `st`: a first-draw corner's texture coordinates as their bits, a second-draw corner's slot of the frame's coordinates in x.
struct VInTie { @location(0) pos: vec3<f32>, @location(1) st: vec2<u32>, @location(2) colour: vec4<f32>, @location(3) slot: u32, @location(4) ln: vec4<f32>, @location(5) ls: vec2<f32>, @location(6) light: u32 };
@vertex fn vs_tie(v: VInTie, @builtin(vertex_index) i: u32) -> VOut {
    var o: VOut;
    let second = (v.slot >> 31u) != 0u;
    let tri = i / 3u;
    if (!second && ((tie_drawn[tri >> 5u] >> (tri & 31u)) & 1u) == 0u) {
        o.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        o.st = vec3<f32>(0.0, 0.0, 1.0);
        o.colour = vec4<f32>(0.0);
        o.depth = 1.0;
        o.f = 0.0;
        return o;
    }
    let slot = v.slot & 0x7fffffffu;
    o.clip = view.m * vec4<f32>(v.pos, 1.0);
    o.st = select(vec3<f32>(bitcast<vec2<f32>>(v.st), 1.0), vec3<f32>(tie_st[v.st.x], 1.0), second);
    o.colour = select(unpack4x8unorm(tie_colours[slot]), v.colour, second);
    o.depth = dot(v.pos - view.eye.xyz, view.forward.xyz);
    let fog = view.fog.z + (view.fog.w - view.fog.z) * (o.depth - view.fog.x) / (view.fog.y - view.fog.x);
    o.f = floor(clamp(fog, min(view.fog.z, view.fog.w), max(view.fog.z, view.fog.w)));
    o.ls = v.ls;
    o.ln = v.ln;
    o.light = v.light;
    o.wpos = v.pos;
    return o;
}

// The shrubs, built once a level (`Shrubs::frame`): each vertex's fog value is its instance's, from the frame's values.
@group(2) @binding(1) var<storage, read> shrub_f: array<u32>;
struct VInShrub { @location(0) pos: vec3<f32>, @location(1) st: vec3<f32>, @location(2) colour: vec4<f32>, @location(3) instance: u32 };
@vertex fn vs_shrub(v: VInShrub) -> VOut {
    var o: VOut;
    o.clip = view.m * vec4<f32>(v.pos, 1.0);
    o.st = v.st;
    o.colour = v.colour;
    o.depth = dot(v.pos - view.eye.xyz, view.forward.xyz);
    o.f = bitcast<f32>(shrub_f[v.instance]);
    // The surface pass derives this mesh's normal from position derivatives. Shrubs include ground decals and rocks as
    // well as plants; leaving this at zero makes their ray-traced lighting/reflection normals undefined.
    o.wpos = v.pos;
    return o;
}

// Another frame buffer's moby runs: the position in its normalised device coordinates, z the view depth (their rows are
// another camera's; `mobies::Mobies::frame`), drawn with the depth test.
@vertex fn vs_clip(v: VIn) -> VOut {
    var o: VOut;
    o.clip = vec4<f32>(v.pos.xy, 1.0 / (v.pos.z + 1.0), 1.0);
    o.st = v.st;
    o.colour = v.colour;
    o.depth = v.pos.z;
    o.f = v.f;
    return o;
}

// The EE's triangles in the world with a vertex the world cannot hold (`eedraw`: one behind the eye, its GS Z wrapped to the
// largest, which program 0's variant B gives a moby with one; the native picture would cut the triangle at its near plane): as
// the GS draws them, a position already in the picture's normalised device coordinates and z the depth of the GS's Z (1 at NEAR,
// 0 at FAR, as `native::Camera::matrix` gives it), with S, T, Q and Z interpolated across the picture as the GS interpolates
// them, drawn with the depth test.
@vertex fn vs_flat(v: VIn) -> VOut {
    var o: VOut;
    o.clip = vec4<f32>(v.pos, 1.0);
    o.st = v.st;
    o.colour = v.colour;
    // The texture's level from the pixel's Q alone (`fs_pass`: depth 1, as 2D's).
    o.depth = 1.0;
    o.f = v.f;
    return o;
}

// 2D: a position already in the picture's normalised device coordinates (the GS's Z test off or ALWAYS).
@vertex fn vs_screen(v: VIn) -> VOut {
    var o: VOut;
    o.clip = vec4<f32>(v.pos.xy, 0.0, 1.0);
    o.st = v.st;
    o.colour = v.colour;
    o.depth = 1.0;
    o.f = v.f;
    return o;
}

fn renormal(d: vec3<f32>) -> vec3<f32> {
    let l = dot(d, d);
    return select(vec3<f32>(0.0), d * inverseSqrt(l), l > 0.0);
}

// Per-pixel lighting (`lighting.rs`, the `lighting` option, `view.lerp.y`): the vertex's colour is its light pass's base plus each
// light's colour times its term at the vertex, which the GS interpolates across the triangle; here the terms at the pixel take
// the place of the interpolated ones, from the normal across the triangle and the vertex's lights (its light word: two sets of
// the frame's and B's weight, blended as the light passes blend them). At a vertex the colour is the game's own.
// The two lights of a light word (`lighting::lights`): colour (w its wrap) and direction each.
struct Lights { c0: vec4<f32>, d0: vec3<f32>, c1: vec4<f32>, d1: vec3<f32> };
@group(0) @binding(1) var<storage, read> actor_lights: array<Lights>;
fn lights_of(light: u32) -> Lights {
    if (((light >> 28u) & 3u) >= 2u) {
        return actor_lights[light & 0x0fffffffu];
    }
    let a = (light & 15u) * 4u;
    let b = ((light >> 4u) & 15u) * 4u;
    let w = f32((light >> 8u) & 255u) / 256.0;
    var l = Lights(view.sets[a], view.sets[a + 1u].xyz, view.sets[a + 2u], view.sets[a + 3u].xyz);
    if (w > 0.0) {
        l.c0 = view.sets[a] * (1.0 - w) + view.sets[b] * w;
        l.d0 = renormal(view.sets[a + 1u].xyz * (1.0 - w) + view.sets[b + 1u].xyz * w);
        l.c1 = view.sets[a + 2u] * (1.0 - w) + view.sets[b + 2u] * w;
        l.d1 = renormal(view.sets[a + 3u].xyz * (1.0 - w) + view.sets[b + 3u].xyz * w);
    }
    return l;
}

// `n`: the normal at the pixel (the one across the triangle, or a Rift Apart material's, `gs`).
fn lit(i: VOut, v: vec4<f32>, n: vec3<f32>) -> vec4<f32> {
    if (view.lerp.y < 0.5 || (i.light >> 31u) == 0u) {
        return v;
    }
    let l = lights_of(i.light);
    var c0 = l.c0;
    let d0 = l.d0;
    var c1 = l.c1;
    let d1 = l.d1;
    let e0 = dot(n, renormal(d0));
    let e1 = dot(n, renormal(d1));
    let mode = (i.light >> 28u) & 3u;
    var s = vec2<f32>(max(e0, e0 * c0.w), max(e1, e1 * c1.w));
    var top = 243.0;
    var k = 1.0;
    if (mode == 2u) {
        s = vec2<f32>(max(e0, e0 * min(c0.w, 0.0)), max(e1, e1 * min(c1.w, 0.0)));
        c0 = max(c0, vec4<f32>(0.0));
        c1 = max(c1, vec4<f32>(0.0));
        top = 255.0;
    } else if (mode == 3u) {
        s = vec2<f32>(max(e0, 0.0), max(e1, 0.0));
        c0 = max(c0, vec4<f32>(0.0));
        c1 = max(c1, vec4<f32>(0.0));
        top = 255.0;
        k = 0.25;
    }
    // The light terms at the pixel, times the share of the colour the lights make there (`lighting::snorm`).
    s = s * i.ln.w;
    let rgb = clamp(v.rgb + 128.0 * k * (c0.rgb * (s.x - i.ls.x) + c1.rgb * (s.y - i.ls.y)), vec3<f32>(0.0), vec3<f32>(top));
    return vec4<f32>(rgb, v.a);
}

fn gs(i: VOut) -> vec4<f32> { return gs_test(i, 0); }

// `test`: 0 keeps every pixel (fur), 1 passes, -1 fails, 2 is the opaque occluder. Compute all derivatives before rejecting
// coverage, then shade only pixels this draw can write. Alpha depends on the sampled texel and vertex alpha, never lighting.
fn gs_test(i: VOut, test: i32) -> vec4<f32> {
    // CLAMP REGION_CLAMP: the coordinate held inside the region (unbounded when off).
    let uv = clamp(i.st.xy / i.st.z, mat.region.xz, mat.region.yw);
    // A Rift Apart surface's material at the pixel (`lighting::MATERIAL`), its frame taken where every pixel of the draw runs.
    let m = material_at(uv);
    let duv1 = dpdx(uv) * mat.rift.xy;
    let duv2 = dpdy(uv) * mat.rift.xy;
    let dp1 = dpdx(i.wpos);
    let dp2 = dpdy(i.wpos);
    let relief_g1 = dpdx(uv);
    let relief_g2 = dpdy(uv);
    var t: vec4<f32>;
    if (mat.extra.x > 0.5) {
        // A pack replacement (`texnames`): the level its own texel density on screen picks.
        t = round(textureSampleGrad(tex, smp, uv, dpdx(uv) * view.lerp.z, dpdy(uv) * view.lerp.z) * 255.0);
    } else {
        // TEX1 LCM 0: LOD = log2(1/Q) + K: 1/Q the view depth times 2^qlog, or (`mat.lodq.x`, the EE's triangles) the pixel's own
        // (st.z its Q times the depth `depth`; 2D, and the GS's positions with its depth, `vs_flat`, depth 1).
        let q = select(log2(i.depth) + mat.qlog, log2(i.depth) - log2(max(abs(i.st.z), 1e-30)), mat.lodq.x > 0.5);
        let lod = min(q + mat.k, mat.last);
        // The filtered texel truncated, as the GS's filter leaves it (the sum of the weighted texels' integer part; a sample
        // on a texel is that texel: the small amount takes up the float's error).
        t = floor(textureSampleLevel(tex, smp, uv, lod) * 255.0 + 0.001);
    }
    // A Rift Apart surface: the texel times its material's grain, its alpha the texture's; lit per pixel, the light pass with
    // the material's normal and then each light's specular.
    let rift = mat.rift.w > 0.5;
    let per_pixel = view.lerp.y > 0.5 && (i.light >> 31u) != 0u;
    let shaded = rift && per_pixel;
    let vertex = round(i.colour * 255.0);
    // TEX0's texture function (`Materials::texture_function`, `mat.emit.z` and `w`): TCC 0, the texel's colour alone; HIGHLIGHT
    // and HIGHLIGHT2, the colour MODULATE gives plus the vertex's alpha, the alpha the texel's plus the vertex's (HIGHLIGHT) or
    // the texel's (HIGHLIGHT2), the vertex's with TCC 0.
    let rgb_only = mat.emit.z > 0.5;
    let tfx = u32(mat.emit.w);
    if (rgb_only) {
        t = vec4<f32>(t.rgb, 128.0);
    }
    var alpha = min(floor(t.a * vertex.a / 128.0), 255.0);
    if (tfx >= 2u) {
        alpha = select(select(t.a, min(t.a + vertex.a, 255.0), tfx == 2u), vertex.a, rgb_only);
    }
    // The alpha test before the normals: a pixel it rejects makes neither (the material's and the relief's sample with the
    // gradients taken above, so the pixels kept shade exactly as before).
    if ((test == 1 && alpha < mat.aref) || (test == -1 && alpha >= mat.aref) || (test == 2 && alpha < max(mat.aref, 128.0))) { discard; }
    // Each normal made only where it is used (`select` makes both sides: the boss fight's effects and fog sheets, no
    // material and no relief, sampled both maps and made both normals at every pixel).
    var n0 = renormal(i.ln.xyz);
    if (shaded) {
        n0 = material_normal(i.ln.xyz, dp1, dp2, duv1, duv2, m);
    }
    // The texture's own relief over that, wherever the pixel is lit per pixel (`lighting::MATERIAL` `material_relief`).
    var n = n0;
    if (per_pixel && mat.relief.x > 0.0) {
        n = material_relief(n0, dp1, dp2, uv, relief_g1, relief_g2);
    }
    let normal_variance = material_normal_variance(n);
    if (rift) {
        // Fur's root, under its strands (`fs_fur`), in their shade.
        let g = material_grain(m) * (1.0 - FUR_ROOT * m.furw);
        t = vec4<f32>(min(round(t.rgb * g), vec3<f32>(255.0)), t.a);
    }
    let v = lit(i, vertex, n);
    var c = min(floor(t * v / 128.0), vec4<f32>(255.0));
    if (tfx >= 2u) {
        c = vec4<f32>(min(c.rgb + vertex.a, vec3<f32>(255.0)), alpha);
    }
    if (shaded) {
        let l = lights_of(i.light);
        let e = renormal(view.eye.xyz - i.wpos);
        let spec = material_highlight(material_specular_filtered(n, e, l.c0, l.d0, m, normal_variance) + material_specular_filtered(n, e, l.c1, l.d1, m, normal_variance) + material_catchlight_filtered(n, e, m, normal_variance), m);
        // A metal reflects what is around it (`lighting::MATERIAL` `material_reflection`), its painted shading less.
        let sky = 127.5 * (max(l.c0.rgb, vec3<f32>(0.0)) + max(l.c1.rgb, vec3<f32>(0.0)));
        let refl = material_reflection(n, e, sky, t.rgb, m);
        let diffuse = 1.0 - MATERIAL_METAL_DIFFUSE * material_metal(m);
        c = vec4<f32>(min(floor(c.rgb * diffuse) + floor((spec + refl) * i.ln.w), vec3<f32>(255.0)), c.a);
    }
    // A fitted atlas (`pbr.tsv`) is lit by the light pass above like any Rift Apart surface: its own normal, gloss and
    // metalness through the game's per-pixel lighting, so it keeps the game's brightness. Its emission map self-illuminates
    // and glows through the game's glow pass by alpha.
    if (m.glow > 0.0) {
        c = vec4<f32>(max(c.rgb, 255.0 * min(material_to_srgb(m.emission), vec3<f32>(1.0))), max(c.a, 128.0 + 127.0 * m.glow));
    }
    // A light (`mat.emit`): at least its own colour times its boost whatever falls on it. Glow changes the frame buffer's
    // alpha only; the original source alpha still controls coverage, the depth test and colour blending.
    if (mat.emit.x > 0.0) {
        c = vec4<f32>(max(c.rgb, min(floor(t.rgb * mat.emit.x), vec3<f32>(255.0))), c.a);
    }
    let rgb = select(floor((c.rgb * i.f + view.fog_colour.rgb * (255.0 - i.f)) / 256.0), c.rgb, i.f > 255.5);
    return vec4<f32>(rgb, c.a);
}

// The pixel as the GS writes it: its colour and its alpha As exactly (a byte, as the frame buffer keeps it: the glow reads
// it back), and for the blend unit As / 0x80 as the second source's alpha (`Src1Alpha`; up to 1).
struct FOut { @location(0) @blend_src(0) c: vec4<f32>, @location(0) @blend_src(1) f: vec4<f32> };
// For a blend that adds its product to the destination, subtracts it or replaces it (`mat.extra.w`: 1 Add and Scale, C FIX;
// 2 AddAlpha, C As; 3 Sub), the product as the GS's blend unit makes it, (Cs x C) >> 7 (for Sub, Cd + ((0 - Cs) x FIX >> 7):
// rounded up), which the blend state adds, subtracts or writes.
fn blended(p: vec4<f32>) -> FOut {
    var o: FOut;
    var rgb = p.rgb;
    let mode = u32(mat.extra.w);
    if (mode == 1u) {
        rgb = floor(p.rgb * mat.extra.z / 128.0);
    } else if (mode == 2u) {
        rgb = floor(p.rgb * p.a / 128.0);
    } else if (mode == 3u) {
        rgb = ceil(p.rgb * mat.extra.z / 128.0);
    }
    o.c = vec4<f32>(rgb / 255.0, material_glow_alpha(p.a) / 255.0);
    o.f = vec4<f32>(0.0, 0.0, 0.0, min(p.a / 128.0, 1.0));
    return o;
}

// Fur (`texnames::RiftSlot`: a surface of regions' fur, its length in the world in `mat.furs`): the moby draw's triangles drawn
// again as FUR_SHELLS shells, root to tip, shell k raised (k + 1) / FUR_SHELLS of the longest fur along the vertex's normal and
// laid over by its square times FUR_LIE down the surface (the world's z is up: fur lies under its own weight; `vs_moby_fur`).
// A strand stands in each cell of the fur's tiles times FUR_CELLS (`fur_strands`: a streak lying the way the fur lies). A
// shell's pixel covers what of it the strands there cover at that height, filtered over the pixel, and where the strands are
// finer than a pixel their share of it (the mean over strands of every length), so fine fur is soft, not noise. Lit as the surface is (`gs`), in
// shade at the root (FUR_ROOT, the surface under the strands too) to a third lighter than the surface at the tip, and at the
// silhouette brighter still (FUR_SHEEN: the tips catch the light edge-on).
const FUR_SHELLS: u32 = 16u;
const FUR_CELLS: f32 = 32.0;
const FUR_ROOT: f32 = 0.2;
const FUR_LIE: f32 = 0.3;
const FUR_STREAK: f32 = 3.0;
const FUR_SHEEN: f32 = 0.6;

@vertex fn vs_moby_fur(v: VInMoby, @builtin(instance_index) k: u32) -> VOut {
    let h = f32(k + 1u) / f32(FUR_SHELLS);
    let top = max(max(mat.furs.x, mat.furs.y), max(mat.furs.z, mat.furs.w));
    let n = renormal(v.ln.xyz);
    let down = vec3<f32>(0.0, 0.0, -1.0) - n * (-n.z);
    var o: VOut;
    let p = v.pos + (v.prev - v.pos) * view.lerp.x + n * (top * h) + down * (top * h * h * FUR_LIE);
    o.clip = view.m * vec4<f32>(p, 1.0);
    o.st = v.st;
    o.colour = v.colour;
    o.depth = dot(p - view.eye.xyz, view.forward.xyz);
    o.f = v.f;
    o.ls = v.ls;
    o.ln = v.ln;
    o.light = v.light;
    o.wpos = p;
    o.shell = h;
    return o;
}

fn fur_hash(c: vec2<f32>) -> vec2<f32> {
    let q = vec2<f32>(dot(c, vec2<f32>(127.1, 311.7)), dot(c, vec2<f32>(269.5, 183.3)));
    return fract(sin(q) * 43758.5453);
}

// A shell's strands at the pixel: each cell's strand (its neighbours' too: a strand reaches over its cell) a streak FUR_STREAK
// times as long as it is wide, lying along `dir` (the fur's way down the surface, in cells), at a random place, 0.65 to 1 of
// the fur's length, a cell's 0.7 wide at its root to nothing at its tip; `span` the cells a pixel spans (the edge filtered over
// it). x: what the strands cover there, y: the covering strand's height in its length.
fn fur_strands(g: vec2<f32>, dir: vec2<f32>, height: f32, span: f32) -> vec2<f32> {
    let perp = vec2<f32>(-dir.y, dir.x);
    var best = vec2<f32>(0.0, 0.0);
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let c = floor(g) + vec2<f32>(f32(dx), f32(dy));
            let r = fur_hash(c);
            let q = g - (c + 0.5 + (fur_hash(c + vec2<f32>(17.0, 31.0)) - 0.5) * 0.9);
            let rise = height / (0.65 + 0.35 * r.x);
            let half = 0.35 * max(1.0 - rise, 0.0);
            let e = length(vec2<f32>(dot(q, dir) / FUR_STREAK, dot(q, perp)));
            let cover = (1.0 - smoothstep(half - 0.5 * span, half + 0.5 * span, e)) * step(rise, 1.0);
            if (cover > best.x) {
                best = vec2<f32>(cover, rise);
            }
        }
    }
    return best;
}

// The share of a cell the strands cover at `rise` of their length.
fn fur_area(rise: f32) -> f32 {
    let half = 0.35 * max(1.0 - rise, 0.0);
    return min(MATERIAL_PI * half * half * FUR_STREAK, 1.0);
}

@fragment fn fs_fur(i: VOut) -> FOut {
    let p = gs(i);
    let uv = clamp(i.st.xy / i.st.z, mat.region.xz, mat.region.yw);
    let m = material_at(uv);
    let top = max(max(mat.furs.x, mat.furs.y), max(mat.furs.z, mat.furs.w));
    // The fur's tiles: its material's (a surface has one fur).
    let f = step(vec4<f32>(1e-6), mat.furs);
    let region_tiles = mat.rift.xy * f.x + mat.tiles.xy * f.y + mat.tiles.zw * f.z + mat.tiles3.xy * f.w;
    // A fitted atlas's fur keeps its original strand tiling (`mat.tiles3.zw`), not the maps' one atlas repeat.
    let tiles = select(region_tiles, mat.tiles3.zw, mat.rift.w > 2.5);
    let g = uv * tiles * FUR_CELLS;
    // How many cells the pixel spans.
    let span = max(length(dpdx(g)), length(dpdy(g)));
    // The way the fur lies, down the surface, in the texture's coordinates: its change of u and v along it (the gradients of u
    // and v in the world from the pixel's frame, as `material_normal` takes them), in cells.
    let n = renormal(i.ln.xyz);
    let down = vec3<f32>(0.0, 0.0, -1.0) + n * n.z;
    let dp1 = dpdx(i.wpos);
    let dp2 = dpdy(i.wpos);
    let duv1 = dpdx(uv);
    let duv2 = dpdy(uv);
    let p2 = cross(dp2, n);
    let p1 = cross(n, dp1);
    let gu = p2 * duv1.x + p1 * duv2.x;
    let gv = p2 * duv1.y + p1 * duv2.y;
    let d = vec2<f32>(dot(down, gu), dot(down, gv)) * tiles;
    let dir = select(vec2<f32>(0.0, 1.0), d * inverseSqrt(max(dot(d, d), 1e-30)), dot(d, d) > 1e-24);
    let height = i.shell * top / max(m.fur, 1e-6);
    let s = fur_strands(g, dir, height, span);
    // The mean over strands of 0.65 to 1 of the length (five of them).
    var mean = 0.0;
    for (var j = 0; j < 5; j++) {
        mean += fur_area(height / (0.65 + 0.0875 * f32(j))) * 0.2;
    }
    let fine = smoothstep(0.3, 1.0, span);
    let cover = mix(s.x, mean, fine);
    if (p.a < mat.aref || cover < 0.004) {
        discard;
    }
    // Relative to the surface, which `gs` darkened at the root already.
    let rise = mix(s.y, min(height, 1.0), fine);
    // Lighter toward the tip, and a sheen where the fur is seen edge-on (its tips catching the light at the silhouette).
    let edge_on = pow(1.0 - abs(dot(n, renormal(view.eye.xyz - i.wpos))), 2.0);
    let shade = ((1.0 - FUR_ROOT) + (FUR_ROOT + 0.3) * min(rise, 1.0)) * (1.0 + FUR_SHEEN * edge_on * min(rise, 1.0)) / (1.0 - FUR_ROOT * m.furw);
    var o: FOut;
    o.c = vec4<f32>(min(floor(p.rgb * shade), vec3<f32>(255.0)) / 255.0, material_glow_alpha(p.a) / 255.0);
    o.f = vec4<f32>(0.0, 0.0, 0.0, cover);
    return o;
}

@fragment fn fs_pass(i: VOut) -> FOut {
    let p = gs_test(i, 1);
    if (p.a < mat.aref) { discard; }
    return blended(p);
}

// The occluders' pass of an alpha-blended draw (`Pipelines::mask_opaque`): only its opaque pixels, As at least 0x80 (and past
// its alpha test); its see-through ones take no depth, so what the frame draws behind them later still shows through.
@fragment fn fs_opaque(i: VOut) -> FOut {
    let p = gs_test(i, 2);
    if (p.a < max(mat.aref, 128.0)) { discard; }
    return blended(p);
}

// The mobies' motion (`dlss.rs`): render pixels from where this picture puts the pixel's surface to where the picture before did,
// for the pixels the occluders' pass puts in (`fs_pass`'s, `fs_opaque`'s).
// A vertex at or behind the picture before's camera (w at or below 0: the camera passed a moby, or stood in one) had no place
// in that picture: divided by its w it was an infinity or a NaN, which DLSS kept and spread over the whole picture (the player,
// 18 November 2026: a spot on Veldin where "the screen gets completely" broken). Such a pixel came from off the picture, so its
// motion puts it twice the picture's width away (DLSS takes nothing of its history there); every motion stays within twice
// the picture's size either way, inside the half floats the motion is kept in.
fn motion_px(c: vec4<f32>, p: vec4<f32>) -> vec4<f32> {
    let most = 2.0 * motion.tan.zw;
    if (p.w <= 1e-6 || c.w <= 1e-6) {
        return vec4<f32>(most.x, 0.0, 0.0, 0.0);
    }
    return vec4<f32>(clamp((p.xy / p.w - c.xy / c.w) * vec2<f32>(0.5, -0.5) * motion.tan.zw, -most, most), 0.0, 0.0);
}

@fragment fn fs_motion(i: VOut) -> @location(0) vec4<f32> {
    let p = gs_test(i, 1);
    if (p.a < mat.aref) { discard; }
    return motion_px(i.mc, i.mp);
}

@fragment fn fs_motion_opaque(i: VOut) -> @location(0) vec4<f32> {
    let p = gs_test(i, 2);
    if (p.a < max(mat.aref, 128.0)) { discard; }
    return motion_px(i.mc, i.mp);
}

// An effect's motion (`Pipelines::draw_fx_motion`), over the surface's behind it where the effect is most of what the pixel shows:
// an alpha-blended effect's pixels at least half covering (As 0x40 of 0x80), an added one's at least a quarter bright (a channel
// 64 of 255). Without it DLSS and frame generation carried a spark or a trail with the wall's motion behind it and smeared it.
@fragment fn fs_motion_fx(i: VOut) -> @location(0) vec4<f32> {
    let p = gs_test(i, 1);
    if (p.a < 64.0) { discard; }
    return motion_px(i.mc, i.mp);
}

@fragment fn fs_motion_fx_add(i: VOut) -> @location(0) vec4<f32> {
    let p = gs_test(i, 1);
    if (max(p.r, max(p.g, p.b)) < 64.0) { discard; }
    return motion_px(i.mc, i.mp);
}

@fragment fn fs_fail(i: VOut) -> FOut {
    let p = gs_test(i, -1);
    if (p.a >= mat.aref) { discard; }
    return blended(p);
}

// The GS blend unit: ((A - B) x C >> 7) + D, A, B and D each Cs, Cd or 0 (selectors 0, 1, 2), C As, Ad or FIX (0, 1, 2), the
// colour clamped to 0..255; the alpha written is As. `mat.extra.y` holds A | B << 2 | C << 4 | D << 6, `extra.z` FIX; Cd and
// Ad are the copy's pixel (its alpha over 0x80).
fn gs_blend(p: vec4<f32>, at: vec4<f32>) -> vec4<f32> {
    let d = textureLoad(dst, vec2<i32>(at.xy), 0);
    let cd = round(d.rgb * 255.0);
    let ad = round(d.a * 255.0);
    let sel = u32(mat.extra.y);
    let pick = function_pick(sel & 3u, p.rgb, cd);
    let b = function_pick((sel >> 2u) & 3u, p.rgb, cd);
    let dd = function_pick((sel >> 6u) & 3u, p.rgb, cd);
    let cs = (sel >> 4u) & 3u;
    let c = select(select(mat.extra.z, ad, cs == 1u), p.a, cs == 0u);
    let rgb = clamp(floor((pick - b) * c / 128.0) + dd, vec3<f32>(0.0), vec3<f32>(255.0));
    return vec4<f32>(rgb / 255.0, material_glow_alpha(p.a) / 255.0);
}

fn function_pick(s: u32, cs: vec3<f32>, cd: vec3<f32>) -> vec3<f32> {
    return select(select(vec3<f32>(0.0), cd, s == 1u), cs, s == 0u);
}

@fragment fn fs_gs_pass(i: VOut) -> @location(0) vec4<f32> {
    let p = gs_test(i, 1);
    if (p.a < mat.aref) { discard; }
    return gs_blend(p, i.clip);
}

@fragment fn fs_gs_fail(i: VOut) -> @location(0) vec4<f32> {
    let p = gs_test(i, -1);
    if (p.a >= mat.aref) { discard; }
    return gs_blend(p, i.clip);
}
"#;

/// The GS blend (ALPHA_1: (A - B) x C / 0x80 + D) of a texture setting, as the blend unit does it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Blend {
    /// 0x44: (Cs - Cd) x As + Cd.
    Alpha,
    /// A = B (the colour minus itself) and D = Cs: the colour replaces the frame's.
    Replace,
    /// A Cs, B 0, C FIX, D Cd: Cs x FIX / 0x80 + Cd.
    Add(u8),
    /// A Cs, B 0, C As, D Cd: Cs x As / 0x80 + Cd.
    AddAlpha,
    /// A Cs, B Cd, C FIX, D Cd: (Cs - Cd) x FIX / 0x80 + Cd (the moby chain fades an instance this way).
    Fade(u8),
    /// A = B, D Cd: the colour stays and only As is written (a two-pass draw's first pass writes the alpha its second pass's
    /// AddDst reads).
    Keep,
    /// A Cs, B 0, C FIX, D 0: Cs x FIX / 0x80 (the glow's passes scale the frame into a smaller buffer).
    Scale(u8),
    /// A 0, B Cs, C FIX, D Cd: Cd - Cs x FIX / 0x80 (a subtractive shade).
    Sub(u8),
    /// Any other (A | B << 2 | C << 4 | D << 6, FIX), and those above with a FIX over 0x80: done by the shader on a copy of the
    /// destination (`fs_gs_pass`).
    Gs(u8, u8),
    /// A Cs, B 0, C Ad, D Cd: Cs x Ad / 0x80 + Cd, Ad the frame's alpha byte. A blend factor gives Ad / 0xFF: the draw is made
    /// twice, the first time writing the colour alone (`half`), so that both add Cs x Ad / 0xFF to the colour the alpha byte
    /// leaves (a tie's second part adds its shine where the first part's alpha says).
    AddDst,
}

impl Blend {
    /// The blend of ALPHA_1 fields A, B, C, D and FIX; a combination no texture setting of this game's data uses stops the draw.
    /// `of` for the EE's own primitives: the shader's GS blend (`Gs`) for any other combination and for `Fade` with a FIX over
    /// 0x80 (up to 2, beyond a blend constant's 1). Add, Scale and Sub take any FIX: the shader makes Cs x FIX / 0x80 (`blended`)
    /// and the 8-bit picture clamps it at 0xFF as the GS clamps the sum (Koros draws 24 such glows a frame, each a GS blend on
    /// two copies of the whole picture before).
    pub fn of_any(a: u8, b: u8, c: u8, d: u8, fix: u8) -> Result<Blend, String> {
        if a == 3 || b == 3 || c == 3 || d == 3 {
            return Err(format!("native: ALPHA A {a} B {b} C {c} D {d}: selector 3 is reserved"));
        }
        let gs = Blend::Gs(a | b << 2 | c << 4 | d << 6, fix);
        Ok(match Blend::of(a, b, c, d, fix) {
            Ok(Blend::Fade(f)) if f > 0x80 => gs,
            Ok(blend) => blend,
            Err(_) => gs,
        })
    }

    pub fn of(a: u8, b: u8, c: u8, d: u8, fix: u8) -> Result<Blend, String> {
        match (a, b, c, d) {
            (0, 1, 0, 1) => Ok(Blend::Alpha),
            (x, y, _, 0) if x == y => Ok(Blend::Replace),
            (0, 2, 2, 1) => Ok(Blend::Add(fix)),
            (0, 1, 2, 1) => Ok(Blend::Fade(fix)),
            (0, 2, 0, 1) => Ok(Blend::AddAlpha),
            (0, 2, 2, 2) => Ok(Blend::Scale(fix)),
            (2, 0, 2, 1) => Ok(Blend::Sub(fix)),
            (0, 2, 1, 1) => Ok(Blend::AddDst),
            (x, y, _, 1) if x == y => Ok(Blend::Keep),
            _ => Err(format!("native: ALPHA_1 A {a} B {b} C {c} D {d} FIX {fix:#x}")),
        }
    }

    fn index(self) -> usize {
        match self {
            Blend::Alpha => 0,
            Blend::Replace => 1,
            Blend::Add(_) => 2,
            Blend::Fade(_) => 3,
            Blend::AddAlpha => 4,
            Blend::Keep => 5,
            Blend::Scale(_) => 6,
            Blend::Sub(_) => 7,
            Blend::Gs(..) => 8,
            Blend::AddDst => 9,
        }
    }
}

/// The pipelines of the object pass, per blend: pixels passing the alpha test (colour and depth) and failing it (colour only).
pub(crate) struct Pipelines {
    pass: Vec<wgpu::RenderPipeline>,
    /// The failing pixels' (colour only, no depth): AFAIL RGB_ONLY (the frame's alpha kept) and FB_ONLY (the alpha written).
    fail: [Vec<wgpu::RenderPipeline>; 2],
    /// The same for 2D (screen positions, no depth test or write).
    screen_pass: Vec<wgpu::RenderPipeline>,
    screen_fail: [Vec<wgpu::RenderPipeline>; 2],
    /// The same as `pass` and `fail` for the EE's triangles the world cannot hold (`vs_flat`: screen positions and the GS's depth).
    flat_pass: Vec<wgpu::RenderPipeline>,
    flat_fail: [Vec<wgpu::RenderPipeline>; 2],
    /// `Blend::Gs`'s: the destination's copy at group 2 (`dst_layout`), no blend state: world pass and fail, 2D pass and fail
    /// (RGB_ONLY), world and 2D fail (FB_ONLY), and `vs_flat`'s pass, fail (RGB_ONLY) and fail (FB_ONLY).
    gs: [wgpu::RenderPipeline; 9],
    /// The same as `pass` and `fail` for another frame buffer's moby runs (`vs_clip`).
    clip_pass: Vec<wgpu::RenderPipeline>,
    clip_fail: [Vec<wgpu::RenderPipeline>; 2],
    /// `Blend::AddDst`'s first draw (colour only, no depth write): world, 2D, another frame buffer's moby runs, `vs_flat`'s.
    half: [wgpu::RenderPipeline; 4],
    /// The same as `pass`, `fail`, `gs` (world pass, fail RGB_ONLY, fail FB_ONLY) and `half` for the native picture's moby runs
    /// (`vs_moby`: their vertices and, in buffer 1, where the frame before drew each).
    moby_pass: Vec<wgpu::RenderPipeline>,
    moby_fail: [Vec<wgpu::RenderPipeline>; 2],
    moby_gs: [wgpu::RenderPipeline; 3],
    moby_half: wgpu::RenderPipeline,
    /// Fur's shells (`vs_moby_fur`, `fs_fur`): blended by their strands' cover, no depth written, drawn after a moby run's
    /// draws with fur, root to tip (`draw_moby`).
    moby_fur: wgpu::RenderPipeline,
    /// The same as `pass`, `fail` and `half` for the ties (`vs_tie`: the level's corners, the frame's values at group 2,
    /// `tie_layout`).
    tie_pass: Vec<wgpu::RenderPipeline>,
    tie_fail: [Vec<wgpu::RenderPipeline>; 2],
    tie_half: wgpu::RenderPipeline,
    pub tie_layout: wgpu::BindGroupLayout,
    pub dst_layout: wgpu::BindGroupLayout,
    /// The shrubs' (`vs_shrub`, `Blend::Alpha`, the only blend they draw with): the passing pixels, the failing ones (RGB_ONLY,
    /// FB_ONLY); their instances' fog values at group 2 (`shrub_layout`).
    shrub_pass: wgpu::RenderPipeline,
    shrub_fail: [wgpu::RenderPipeline; 2],
    pub shrub_layout: wgpu::BindGroupLayout,
    /// The ambient occlusion's moby pass (`ao.rs`): an opaque draw's passing pixels into the depth, and 0 into an R8 mask
    /// cleared to 1 (the blend makes every written pixel 0).
    mask: wgpu::RenderPipeline,
    /// The same for an alpha-blended draw: only its opaque pixels (As at least 0x80, `fs_opaque`). Its see-through pixels (a
    /// helmet's visor glass) take no depth in this pass, which runs before the moby runs draw: the face behind the glass, drawn
    /// after it, would fail the depth test there (the Phoenix arrival's close-up drew no upper face with the ambient occlusion,
    /// the shadows or the ray tracing on).
    mask_opaque: wgpu::RenderPipeline,
    /// The mobies' motion for DLSS (`dlss.rs`, `vs_moby_motion`): the occluders' pixels, each its motion, into the motion
    /// target with the picture's depth (read only), and the layout of its uniform (group 2, `dlss::MotionU`).
    motion: wgpu::RenderPipeline,
    motion_opaque: wgpu::RenderPipeline,
    /// The effects' motion (`draw_fx_motion`): an alpha-blended effect's covering pixels, an added one's bright pixels.
    motion_fx: wgpu::RenderPipeline,
    motion_fx_add: wgpu::RenderPipeline,
    pub motion_layout: wgpu::BindGroupLayout,
}

impl Pipelines {
    pub fn new(device: &wgpu::Device, view_layout: &wgpu::BindGroupLayout, mat_layout: &wgpu::BindGroupLayout) -> Pipelines {
        let shader = format!("{SHADER}{}{}{}", crate::lighting::MATERIAL, crate::surface_buffer::COMMON, crate::surface_buffer::OBJECTS);
        let source = if crate::surfaces::on() { crate::surfaces::shader(&shader, "t = round(textureSampleGrad(tex, smp, uv, dpdx(uv) * view.lerp.z, dpdy(uv) * view.lerp.z) * 255.0);", "uv").unwrap_or_else(|e| panic!("{e}")) } else { shader };
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("objects"), source: wgpu::ShaderSource::Wgsl(source.into()) });
        let dst_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("GS blend destination"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("objects"), bind_group_layouts: &[Some(view_layout), Some(mat_layout)], immediate_size: 0 });
        let gs_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("objects, GS blend"), bind_group_layouts: &[Some(view_layout), Some(mat_layout), Some(&dst_layout)], immediate_size: 0 });
        let attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4, 3 => Float32];
        let add_dst = Some(wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::DstAlpha, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent::REPLACE,
        });
        let blends = [
            Some(wgpu::BlendState {
                color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Src1Alpha, dst_factor: wgpu::BlendFactor::OneMinusSrc1Alpha, operation: wgpu::BlendOperation::Add },
                alpha: wgpu::BlendComponent::REPLACE,
            }),
            Some(wgpu::BlendState::REPLACE),
            // Add: the shader's product (`blended`) plus Cd.
            Some(wgpu::BlendState {
                color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
                alpha: wgpu::BlendComponent::REPLACE,
            }),
            Some(wgpu::BlendState {
                color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Constant, dst_factor: wgpu::BlendFactor::OneMinusConstant, operation: wgpu::BlendOperation::Add },
                alpha: wgpu::BlendComponent::REPLACE,
            }),
            // AddAlpha: the shader's product plus Cd.
            Some(wgpu::BlendState {
                color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
                alpha: wgpu::BlendComponent::REPLACE,
            }),
            Some(wgpu::BlendState {
                color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
                alpha: wgpu::BlendComponent::REPLACE,
            }),
            // Scale: the shader's product.
            Some(wgpu::BlendState::REPLACE),
            // Sub: Cd less the shader's product.
            Some(wgpu::BlendState {
                color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::ReverseSubtract },
                alpha: wgpu::BlendComponent::REPLACE,
            }),
            // Gs (index 8) draws with `gs`; this entry is not used.
            Some(wgpu::BlendState::REPLACE),
            add_dst,
        ];
        // A moby vertex's place in the frame before (`vs_moby`), buffer 1.
        let prev_attrs = wgpu::vertex_attr_array![4 => Float32x3];
        // A moby vertex's per-pixel lighting (`lighting::MobyLit`), buffer 2.
        let lit_attrs = wgpu::vertex_attr_array![5 => Snorm8x4, 6 => Float32x2, 7 => Uint32];
        let lit_buffer = wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<crate::lighting::MobyLit>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &lit_attrs };
        let obj_buffer = wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<ObjVertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs };
        let prev_buffer = wgpu::VertexBufferLayout { array_stride: 12, step_mode: wgpu::VertexStepMode::Vertex, attributes: &prev_attrs };
        // `alpha`: whether the pixel writes its alpha (a passing pixel and AFAIL FB_ONLY do; AFAIL RGB_ONLY writes the colour alone).
        let pipeline_in = |layout: &wgpu::PipelineLayout, vs: &str, entry: &str, depth_write: bool, compare: wgpu::CompareFunction, blend: Option<wgpu::BlendState>, alpha: bool| {
            let buffers = [Some(obj_buffer.clone()), Some(prev_buffer.clone()), Some(lit_buffer.clone())];
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: if vs.starts_with("vs_moby") { &buffers[..] } else { &buffers[..1] },
                },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: crate::native::DEPTH,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(compare),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend, write_mask: if alpha { wgpu::ColorWrites::ALL } else { wgpu::ColorWrites::COLOR } })],
                }),
                multiview_mask: None,
                cache: crate::pipelines::cache(),
            })
        };
        let pipeline = |vs: &str, entry: &str, depth_write: bool, compare: wgpu::CompareFunction, blend: Option<wgpu::BlendState>, alpha: bool| pipeline_in(&layout, vs, entry, depth_write, compare, blend, alpha);
        let (near, always) = (wgpu::CompareFunction::GreaterEqual, wgpu::CompareFunction::Always);
        // Per blend: [RGB_ONLY, FB_ONLY] for the failing pixels.
        let fail = |vs: &str| -> [Vec<wgpu::RenderPipeline>; 2] { [false, true].map(|a| blends.iter().map(|&b| pipeline(vs, "fs_fail", false, if vs == "vs_screen" { always } else { near }, b, a)).collect()) };
        // The shrubs': `pass` and `fail`'s `Blend::Alpha` pipelines with the built shrubs' vertices and their fog values.
        let shrub_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shrub fog values"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let shrub_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("shrubs"), bind_group_layouts: &[Some(view_layout), Some(mat_layout), Some(&shrub_layout)], immediate_size: 0 });
        // The ties': the frame's fog values, drawn bits and second-draw coordinates at group 2.
        let tie_entry = |binding: u32| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::VERTEX, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None }, count: None };
        let tie_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("ties' frame values"), entries: &[tie_entry(3), tie_entry(4), tie_entry(5)] });
        let tie_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("ties"), bind_group_layouts: &[Some(view_layout), Some(mat_layout), Some(&tie_layout)], immediate_size: 0 });
        let tie_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Uint32x2, 2 => Unorm8x4, 3 => Uint32, 4 => Snorm8x4, 5 => Float32x2, 6 => Uint32];
        let tie_pipeline = |entry: &str, depth_write: bool, blend: Option<wgpu::BlendState>, alpha: bool| {
            crate::surface_buffer::pipeline(device, &wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&tie_pl),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_tie"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<TieCorner>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &tie_attrs })],
                },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState { format: crate::native::DEPTH, depth_write_enabled: Some(depth_write), depth_compare: Some(near), stencil: Default::default(), bias: Default::default() }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend, write_mask: if alpha { wgpu::ColorWrites::ALL } else { wgpu::ColorWrites::COLOR } })],
                }),
                multiview_mask: None,
                cache: crate::pipelines::cache(),
            }, (depth_write && blend != add_dst).then_some("fs_surface"))
        };
        let shrub_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4, 3 => Uint32];
        let shrub_pipeline = |entry: &str, depth_write: bool, alpha: bool| {
            crate::surface_buffer::pipeline(device, &wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&shrub_pl),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_shrub"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<ShrubVertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &shrub_attrs })],
                },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState { format: crate::native::DEPTH, depth_write_enabled: Some(depth_write), depth_compare: Some(near), stencil: Default::default(), bias: Default::default() }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: blends[Blend::Alpha.index()], write_mask: if alpha { wgpu::ColorWrites::ALL } else { wgpu::ColorWrites::COLOR } })],
                }),
                multiview_mask: None,
                cache: crate::pipelines::cache(),
            }, depth_write.then_some("fs_surface"))
        };
        let mask = |entry: &str| {
            crate::surface_buffer::pipeline(device, &wgpu::RenderPipelineDescriptor {
                label: Some("moby occluders"),
                layout: Some(&layout),
                // As the moby runs draw (`vs_moby`): where a picture between two frames puts them too.
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_moby"), compilation_options: Default::default(), buffers: &[Some(obj_buffer.clone()), Some(prev_buffer.clone()), Some(lit_buffer.clone())] },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState { format: crate::native::DEPTH, depth_write_enabled: Some(true), depth_compare: Some(near), stencil: Default::default(), bias: Default::default() }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::R8Unorm,
                        blend: Some(wgpu::BlendState { color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::Zero, operation: wgpu::BlendOperation::Add }, alpha: wgpu::BlendComponent::REPLACE }),
                        write_mask: wgpu::ColorWrites::RED,
                    })],
                }),
                multiview_mask: None,
                cache: crate::pipelines::cache(),
            }, Some(if entry == "fs_opaque" { "fs_surface_opaque" } else { "fs_surface" }))
        };
        let motion_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("dlss moby motion"),
            entries: &[wgpu::BindGroupLayoutEntry { binding: 6, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }],
        });
        let motion_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("objects, motion"), bind_group_layouts: &[Some(view_layout), Some(mat_layout), Some(&motion_layout)], immediate_size: 0 });
        let motion = |entry: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("dlss moby motion"),
                layout: Some(&motion_pl),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_moby_motion"), compilation_options: Default::default(), buffers: &[Some(obj_buffer.clone()), Some(prev_buffer.clone()), Some(lit_buffer.clone())] },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState { format: crate::native::DEPTH, depth_write_enabled: Some(false), depth_compare: Some(near), stencil: Default::default(), bias: Default::default() }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some(entry), compilation_options: Default::default(), targets: &[Some(wgpu::TextureFormat::Rg16Float.into())] }),
                multiview_mask: None,
                cache: crate::pipelines::cache(),
            })
        };
        Pipelines {
            motion: motion("fs_motion"),
            motion_opaque: motion("fs_motion_opaque"),
            motion_fx: motion("fs_motion_fx"),
            motion_fx_add: motion("fs_motion_fx_add"),
            motion_layout,
            mask: mask("fs_pass"),
            mask_opaque: mask("fs_opaque"),
            shrub_pass: shrub_pipeline("fs_pass", true, true),
            shrub_fail: [shrub_pipeline("fs_fail", false, false), shrub_pipeline("fs_fail", false, true)],
            shrub_layout,
            pass: blends.iter().map(|&b| pipeline("vs", "fs_pass", true, near, b, true)).collect(),
            fail: fail("vs"),
            screen_pass: blends.iter().map(|&b| pipeline("vs_screen", "fs_pass", false, always, b, true)).collect(),
            screen_fail: fail("vs_screen"),
            clip_pass: blends.iter().map(|&b| pipeline("vs_clip", "fs_pass", true, near, b, true)).collect(),
            clip_fail: fail("vs_clip"),
            flat_pass: blends.iter().map(|&b| pipeline("vs_flat", "fs_pass", true, near, b, true)).collect(),
            flat_fail: fail("vs_flat"),
            gs: [
                pipeline_in(&gs_layout, "vs", "fs_gs_pass", true, near, None, true),
                pipeline_in(&gs_layout, "vs", "fs_gs_fail", false, near, None, false),
                pipeline_in(&gs_layout, "vs_screen", "fs_gs_pass", false, always, None, true),
                pipeline_in(&gs_layout, "vs_screen", "fs_gs_fail", false, always, None, false),
                pipeline_in(&gs_layout, "vs", "fs_gs_fail", false, near, None, true),
                pipeline_in(&gs_layout, "vs_screen", "fs_gs_fail", false, always, None, true),
                pipeline_in(&gs_layout, "vs_flat", "fs_gs_pass", true, near, None, true),
                pipeline_in(&gs_layout, "vs_flat", "fs_gs_fail", false, near, None, false),
                pipeline_in(&gs_layout, "vs_flat", "fs_gs_fail", false, near, None, true),
            ],
            half: [
                pipeline("vs", "fs_pass", false, near, add_dst, false),
                pipeline("vs_screen", "fs_pass", false, always, add_dst, false),
                pipeline("vs_clip", "fs_pass", false, near, add_dst, false),
                pipeline("vs_flat", "fs_pass", false, near, add_dst, false),
            ],
            moby_pass: blends.iter().map(|&b| pipeline("vs_moby", "fs_pass", true, near, b, true)).collect(),
            moby_fail: fail("vs_moby"),
            moby_gs: [
                pipeline_in(&gs_layout, "vs_moby", "fs_gs_pass", true, near, None, true),
                pipeline_in(&gs_layout, "vs_moby", "fs_gs_fail", false, near, None, false),
                pipeline_in(&gs_layout, "vs_moby", "fs_gs_fail", false, near, None, true),
            ],
            moby_half: pipeline("vs_moby", "fs_pass", false, near, add_dst, false),
            moby_fur: pipeline("vs_moby_fur", "fs_fur", false, near, blends[Blend::Alpha.index()], false),
            tie_pass: blends.iter().map(|&b| tie_pipeline("fs_pass", true, b, true)).collect(),
            tie_fail: [false, true].map(|a| blends.iter().map(|&b| tie_pipeline("fs_fail", false, b, a)).collect()),
            tie_half: tie_pipeline("fs_pass", false, add_dst, false),
            tie_layout,
            dst_layout,
        }
    }

    /// The opaque draws of `draws` (`Blend::Alpha` and `Blend::Replace`: what a glow, a fade or an added shine is not) as the
    /// ambient occlusion's occluders (`ao.rs`): their passing pixels into the depth and the mask (`mask`).
    pub fn draw_occluders<'a, E: Enc<'a>>(&'a self, pass: &mut E, materials: &'a Materials, draws: &[Draw]) -> Result<(), String> {
        let mut b = Bound::default();
        for dr in draws.iter().filter(|d| matches!(d.key.5, Blend::Alpha | Blend::Replace)) {
            let (bg, _) = materials.keys.get(&dr.key).ok_or("native: an object texture setting made at load is missing")?;
            // An alpha-blended draw puts only its opaque pixels in (`mask_opaque`); a replacing one all that pass its test.
            b.pipeline(pass, if dr.key.5 == Blend::Alpha { &self.mask_opaque } else { &self.mask });
            b.group(pass, bg);
            pass.draw(dr.first..dr.first + dr.count);
        }
        Ok(())
    }

    /// The mobies' motion for DLSS (`dlss.rs`): the draws `draw_occluders` puts in, their motion into the motion target.
    pub fn draw_motion<'a, E: Enc<'a>>(&'a self, pass: &mut E, materials: &'a Materials, draws: &[Draw]) -> Result<(), String> {
        let mut b = Bound::default();
        for dr in draws.iter().filter(|d| matches!(d.key.5, Blend::Alpha | Blend::Replace)) {
            let (bg, _) = materials.keys.get(&dr.key).ok_or("native: an object texture setting made at load is missing")?;
            b.pipeline(pass, if dr.key.5 == Blend::Alpha { &self.motion_opaque } else { &self.motion });
            b.group(pass, bg);
            pass.draw(dr.first..dr.first + dr.count);
        }
        Ok(())
    }

    /// The effects' motion for DLSS (`native.rs`, after the mobies' `draw_motion`, the effects' vertices and their places in the
    /// frame before bound as `draw_moby`'s): a replacing effect's pixels, an alpha-blended one's covering pixels and an added
    /// one's bright pixels (`fs_motion_fx`, `fs_motion_fx_add`); an effect that keeps the colour behind it moves nothing seen.
    pub fn draw_fx_motion<'a, E: Enc<'a>>(&'a self, pass: &mut E, materials: &'a Materials, draws: &[Draw]) -> Result<(), String> {
        let mut b = Bound::default();
        for dr in draws.iter().filter(|d| d.count > 0) {
            let pipe = match dr.key.5 {
                Blend::Alpha => &self.motion_fx,
                Blend::Replace => &self.motion,
                Blend::Keep => continue,
                _ => &self.motion_fx_add,
            };
            let (bg, _) = materials.keys.get(&dr.key).ok_or("native: an effect's texture setting made at load is missing")?;
            b.pipeline(pass, pipe);
            b.group(pass, bg);
            pass.draw(dr.first..dr.first + dr.count);
        }
        Ok(())
    }

    /// The native picture's moby runs (`vs_moby`: buffer 1 holds where the frame before drew each vertex), as `draw` draws;
    /// then the shells of the opaque draws whose texture has fur (`fs_fur`).
    pub fn draw_moby<'a, E: Enc<'a>>(&'a self, pass: &mut E, materials: &'a Materials, draws: &[Draw]) -> Result<(), String> {
        self.draw_with(pass, materials, draws, Kind::Moby, None, false)?;
        let mut b = Bound::default();
        for dr in draws.iter().filter(|d| matches!(d.key.5, Blend::Alpha | Blend::Replace) && crate::texnames::RiftMat::has_fur(materials.rift[d.key.0 as usize].as_ref())) {
            let (bg, _) = materials.keys.get(&dr.key).ok_or("native: an object texture setting made at load is missing")?;
            b.group(pass, bg);
            b.pipeline(pass, &self.moby_fur);
            pass.draw_instanced(dr.first..dr.first + dr.count, 0..FUR_SHELLS);
        }
        Ok(())
    }

    /// The shrubs' `draws` (`Shrubs::frame`: ranges of the level's built shrub vertices, each with the chunk of `vertices` it
    /// draws from, bound with the instances' fog values at group 2), as `draw` draws a `Blend::Alpha` setting.
    pub fn draw_shrubs<'a, E: Enc<'a>>(&'a self, pass: &mut E, materials: &'a Materials, vertices: &'a [Option<wgpu::Buffer>], draws: &[(usize, Draw)]) -> Result<(), String> {
        // Every draw's passing pixels, then every draw's failing ones: two pipeline binds, where a draw's own passing and failing
        // pixels in turn bound one each (level 1's 10,149 draws took 11.4 ms to record that way). A shrub's below-AREF pixels (its
        // fringe, colour only) come after the passing pixels of the shrubs after it, as within one draw they come after its later
        // triangles'; the GS draws each pixel once, passing or failing.
        let mut b = Bound::default();
        for fail in [false, true] {
            b.pipeline(pass, if fail { &self.shrub_fail[materials.fb_only as usize] } else { &self.shrub_pass });
            let mut chunk = usize::MAX;
            for (c, dr) in draws {
                let (bg, fails) = materials.keys.get(&dr.key).ok_or("native: a shrub texture setting made at load is missing")?;
                if fail && !*fails {
                    continue;
                }
                if *c != chunk {
                    let vb = vertices[*c].as_ref().ok_or("native: a shrub draw from a chunk with no vertices")?;
                    pass.set_vertex_buffer(0, vb);
                    chunk = *c;
                }
                b.group(pass, bg);
                pass.draw(dr.first..dr.first + dr.count);
            }
        }
        Ok(())
    }

    /// Another frame buffer's moby runs (`vs_clip`); a GS blend no blend state does is not among them (the moby pass's ALPHA_1).
    pub fn draw_clip<'a, E: Enc<'a>>(&'a self, pass: &mut E, materials: &'a Materials, draws: &[Draw]) -> Result<(), String> {
        let mut b = Bound::default();
        for dr in draws {
            let (bg, fails) = materials.keys.get(&dr.key).ok_or("native: an object texture setting made at load is missing")?;
            let blend = dr.key.5;
            if let Blend::Gs(..) = blend {
                return Err("native: a GS blend into another frame buffer's moby run".into());
            }
            if let Blend::Fade(fix) = blend {
                let f = (fix as f64 / 128.0).min(1.0);
                pass.set_blend_constant(wgpu::Color { r: f, g: f, b: f, a: f })?;
            }
            b.group(pass, bg);
            if blend == Blend::AddDst {
                b.pipeline(pass, &self.half[2]);
                pass.draw(dr.first..dr.first + dr.count);
            }
            b.pipeline(pass, &self.clip_pass[blend.index()]);
            pass.draw(dr.first..dr.first + dr.count);
            if *fails {
                if blend == Blend::AddDst {
                    b.pipeline(pass, &self.clip_fail[0][blend.index()]);
                    pass.draw(dr.first..dr.first + dr.count);
                }
                b.pipeline(pass, &self.clip_fail[materials.fb_only as usize][blend.index()]);
                pass.draw(dr.first..dr.first + dr.count);
            }
        }
        Ok(())
    }

    /// The same, in the picture's own coordinates when `screen` is set (2D), or in them with the GS's depth when `flat` is
    /// (`vs_flat`); `dst` is the destination's copy a `Blend::Gs` draw reads (`dst_layout`).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_in<'a, E: Enc<'a>>(&'a self, pass: &mut E, materials: &'a Materials, draws: &[Draw], screen: bool, flat: bool, dst: Option<&'a wgpu::BindGroup>) -> Result<(), String> {
        self.draw_with(pass, materials, draws, if screen { Kind::Screen } else if flat { Kind::Flat } else { Kind::World }, dst, false)
    }

    /// The ties' draws (`Ties::frame`, `vs_tie`: ranges of the level's tie corners), each draw's ranges together: every draw's
    /// passing pixels in the game's order, then every draw's failing ones. The GS draws a draw's failing pixels (colour, no
    /// depth) before the next draw's; drawn so here, the pipeline changed twice a draw, and the presenter executes the ties'
    /// bundle for every picture: on Daxx the switches were 1.55 of its 3.61 ms a picture. The one difference is where a draw's
    /// failing pixel lies in front of a later draw's passing one: the GS let the later draw cover it, here it blends over it
    /// (a one-pixel fringe: 835 of Daxx's 1.4 million pixels at 1593 x 896, none on Phoenix, 9 and 27 on Zeldrin and Veldin).
    /// So within each run of one TEST_1's draws (`materials`, by `TieDraw::test`).
    ///
    /// A stretch of opaque first parts (Replace, no failing pixels) and second parts (AddDst: the shine the first part's alpha
    /// gives) is drawn its first parts first, then each step of its second parts for the whole stretch: their colour-only draws,
    /// their full draws, their failing pixels. The depth test keeps the picture: a second part lies on its first part, and an
    /// opaque first part in front of it hides it as before; only surfaces at exactly one depth can differ. The final boss's
    /// level alternates the two a draw at a time, their TEST_1s apart: five pipeline changes a pair, 5,300 a frame.
    pub fn draw_ties<'a, E: Enc<'a>>(&'a self, pass: &mut E, materials: &'a [Materials], draws: &[TieDraw], ranges: &[(u32, u32)]) -> Result<(), String> {
        let mut b = Bound::default();
        let go = |pass: &mut E, dr: &TieDraw| {
            for &(first, count) in &ranges[dr.first as usize..(dr.first + dr.ranges) as usize] {
                pass.draw(first..first + count);
            }
        };
        // Each draw's material, looked up once (a missing one is the error where the draw first needs it).
        let found: Vec<Option<(&'a wgpu::BindGroup, bool, usize)>> = draws.iter().map(|dr| {
            let m = &materials[dr.test];
            m.keys.get(&dr.key).map(|(bg, fails)| (bg, *fails, m.fb_only as usize))
        }).collect();
        let entry = |at: usize| -> Result<(&'a wgpu::BindGroup, bool, usize), String> {
            found[at].ok_or("native: a tie texture setting made at load is missing".to_string())
        };
        let shine = |dr: &TieDraw| dr.key.5 == Blend::AddDst;
        let mut k = 0;
        while k < draws.len() {
            let mut n = 0;
            while let Some(dr) = draws.get(k + n) {
                if !(shine(dr) || (dr.key.5 == Blend::Replace && !entry(k + n)?.1)) {
                    break;
                }
                n += 1;
            }
            if n > 0 {
                let stretch = &draws[k..k + n];
                for (j, dr) in stretch.iter().enumerate().filter(|(_, d)| !shine(d)) {
                    b.group(pass, entry(k + j)?.0);
                    b.pipeline(pass, &self.tie_pass[Blend::Replace.index()]);
                    go(pass, dr);
                }
                for step in 0..4 {
                    for (j, dr) in stretch.iter().enumerate().filter(|(_, d)| shine(d)) {
                        let (bg, fails, fb) = entry(k + j)?;
                        if step >= 2 && !fails {
                            continue;
                        }
                        b.group(pass, bg);
                        b.pipeline(pass, match step {
                            0 => &self.tie_half,
                            1 => &self.tie_pass[Blend::AddDst.index()],
                            2 => &self.tie_fail[0][Blend::AddDst.index()],
                            _ => &self.tie_fail[fb][Blend::AddDst.index()],
                        });
                        go(pass, dr);
                    }
                }
                k += n;
                continue;
            }
            // A run of one TEST_1's other draws.
            let t = draws[k].test;
            let mut n = 0;
            while let Some(dr) = draws.get(k + n) {
                if dr.test != t || (n > 0 && (shine(dr) || (dr.key.5 == Blend::Replace && !entry(k + n)?.1))) {
                    break;
                }
                n += 1;
            }
            let run = &draws[k..k + n];
            let first_of_run = k;
            k += n;
            let fb = materials[t].fb_only as usize;
            for phase in 0..2 {
                for (j, dr) in run.iter().enumerate() {
                    let (bg, fails, _) = entry(first_of_run + j)?;
                    let blend = dr.key.5;
                    if let Blend::Gs(..) = blend {
                        return Err("native: a GS blend of a tie".into());
                    }
                    if phase == 1 && !fails {
                        continue;
                    }
                    if let Blend::Fade(fix) = blend {
                        let f = (fix as f64 / 128.0).min(1.0);
                        pass.set_blend_constant(wgpu::Color { r: f, g: f, b: f, a: f })?;
                    }
                    b.group(pass, bg);
                    b.pipeline(pass, if phase == 0 { &self.tie_pass[blend.index()] } else { &self.tie_fail[fb][blend.index()] });
                    go(pass, dr);
                }
            }
        }
        Ok(())
    }

    fn draw_with<'a, E: Enc<'a>>(&'a self, pass: &mut E, materials: &'a Materials, draws: &[Draw], kind: Kind, dst: Option<&'a wgpu::BindGroup>, indexed: bool) -> Result<(), String> {
        let go = |pass: &mut E, dr: &Draw| {
            if indexed {
                pass.draw_indexed(dr.first..dr.first + dr.count);
            } else {
                pass.draw(dr.first..dr.first + dr.count);
            }
        };
        let fb = materials.fb_only as usize;
        // Per kind: the passing pixels' pipelines, the failing ones' (this AFAIL's, and RGB_ONLY's for AddDst), the GS blend's
        // (pass, fail RGB_ONLY, fail FB_ONLY) and AddDst's first draw.
        let (pipes_pass, pipes_fail, rgb_fail, gs, half) = match kind {
            Kind::World => (&self.pass, &self.fail[fb], &self.fail[0], [&self.gs[0], &self.gs[1], &self.gs[4]], &self.half[0]),
            Kind::Screen => (&self.screen_pass, &self.screen_fail[fb], &self.screen_fail[0], [&self.gs[2], &self.gs[3], &self.gs[5]], &self.half[1]),
            Kind::Flat => (&self.flat_pass, &self.flat_fail[fb], &self.flat_fail[0], [&self.gs[6], &self.gs[7], &self.gs[8]], &self.half[3]),
            Kind::Moby => (&self.moby_pass, &self.moby_fail[fb], &self.moby_fail[0], [&self.moby_gs[0], &self.moby_gs[1], &self.moby_gs[2]], &self.moby_half),
        };
        let mut b = Bound::default();
        for dr in draws {
            let (bg, fails) = materials.keys.get(&dr.key).ok_or("native: an object texture setting made at load is missing")?;
            let blend = dr.key.5;
            if let Blend::Gs(..) = blend {
                let d = dst.ok_or("native: a GS blend drawn without its destination's copy")?;
                b.group(pass, bg);
                pass.set_bind_group(2, d);
                b.pipeline(pass, gs[0]);
                go(pass, dr);
                if *fails {
                    b.pipeline(pass, if materials.fb_only { gs[2] } else { gs[1] });
                    go(pass, dr);
                }
                continue;
            }
            if let Blend::Fade(fix) = blend {
                let f = (fix as f64 / 128.0).min(1.0);
                pass.set_blend_constant(wgpu::Color { r: f, g: f, b: f, a: f })?;
            }
            b.group(pass, bg);
            if blend == Blend::AddDst {
                b.pipeline(pass, half);
                go(pass, dr);
            }
            b.pipeline(pass, &pipes_pass[blend.index()]);
            go(pass, dr);
            if *fails {
                if blend == Blend::AddDst {
                    b.pipeline(pass, &rgb_fail[blend.index()]);
                    go(pass, dr);
                }
                b.pipeline(pass, &pipes_fail[blend.index()]);
                go(pass, dr);
            }
        }
        Ok(())
    }
}

/// Which vertices an object draw takes: in the world (`vs`), in the picture's own coordinates (`vs_screen`), the native
/// picture's mobies with their places in the frame before (`vs_moby`).
#[derive(Clone, Copy)]
enum Kind {
    World,
    Screen,
    Flat,
    Moby,
}

/// What a run of draws last bound in its pass (its pipeline, its group 1), so a draw binding the same again binds nothing: wgpu
/// passes every set on to the driver. Made fresh for each run of draws (whatever was bound before it is bound again).
#[derive(Default)]
pub(crate) struct Bound {
    pipeline: Option<*const wgpu::RenderPipeline>,
    group: Option<*const wgpu::BindGroup>,
}

impl Bound {
    pub(crate) fn pipeline<'a, E: Enc<'a>>(&mut self, pass: &mut E, p: &'a wgpu::RenderPipeline) {
        if self.pipeline != Some(p as *const _) {
            pass.set_pipeline(p);
            self.pipeline = Some(p as *const _);
        }
    }
    pub(crate) fn group<'a, E: Enc<'a>>(&mut self, pass: &mut E, g: &'a wgpu::BindGroup) {
        if self.group != Some(g as *const _) {
            pass.set_bind_group(1, g);
            self.group = Some(g as *const _);
        }
    }
}

/// A GS texture setting: the texture (index into the layer's textures), CLAMP WMS and WMT (true = clamp), TEX1 MMIN and K
/// (bits), the blend, and the CLAMP register when an axis has REGION_CLAMP (WMS or WMT 2; 0 otherwise).
pub(crate) type Key = (u32, bool, bool, u32, u32, Blend, u64);

/// `MatU::region` with no region.
pub(crate) const NO_REGION: [f32; 4] = [-1e30, 1e30, -1e30, 1e30];

/// Fur's shells a draw with fur takes (the shader's FUR_SHELLS).
const FUR_SHELLS: u32 = 16;

/// The sampler keys' filters for a texture sampled at the level its density on screen picks (`texnames::hd_sampler`; TEX1 MMIN
/// is 0 to 5): every level, and level 0 alone.
pub(crate) const HD_MMIN: u32 = 6;
pub(crate) const HD_MMIN0: u32 = 7;
/// The same for the game's own textures under the texture filtering option (`enhance::aniso`: its anisotropy, where a pack
/// replacement's is 16).
pub(crate) const ANISO_MMIN: u32 = 8;
pub(crate) const ANISO_MMIN0: u32 = 9;

/// TEX1 K (the low 12 bits of the register word: a signed count of 1/16 mip levels).
pub(crate) fn tex1_k(k: u32) -> f32 {
    let k12 = (k & 0xfff) as i32;
    (if k12 >= 0x800 { k12 - 0x1000 } else { k12 }) as f32 / 16.0
}

/// A layer's textures on the GPU and its materials (one bind group per GS texture setting, made when first used).
pub(crate) struct Materials {
    views: Vec<wgpu::TextureView>,
    levels: Vec<usize>,
    /// Minimum alpha across every mip, for the original texture and its replacement.
    alpha_min: Vec<[u8; 2]>,
    /// Per texture: its width and height.
    sizes: Vec<(usize, usize)>,
    /// Per texture: its pack replacement on the GPU and its level count (`texnames`), which every setting without a clamp region
    /// draws.
    hd: Vec<Option<(wgpu::TextureView, usize)>>,
    /// Per texture: its Rift Apart material (`texnames::rift`), the map of those without one (`texnames::flat_material`) and the
    /// materials' sampler (repeating).
    rift: Vec<Option<crate::texnames::RiftMat>>,
    /// Each texture's light (`texnames::light`: boost, glow).
    lights: Vec<[f32; 2]>,
    flat: wgpu::TextureView,
    rift_sampler: std::sync::Arc<wgpu::Sampler>,
    /// Per texture: its PCSX2 name (`texnames::name`; None for one with no GS form), which `uya native --surfaces` counts an
    /// object's surface by (`surfaces::object_triangle`).
    pub names: Vec<Option<crate::texnames::TexName>>,
    samplers: FastMap<(bool, bool, u32), wgpu::Sampler>,
    pub keys: FastMap<Key, (wgpu::BindGroup, bool)>,
    batch_inputs: Option<FastMap<Key, crate::shrub_batch::MaterialInput>>,
    qlog: f32,
    /// TEST_1 AREF; 256 for ATST NEVER (every pixel fails).
    aref: u16,
    /// TEST_1 AFAIL KEEP: a failing pixel writes nothing (no second draw).
    keep: bool,
    /// TEST_1 AFAIL FB_ONLY: a failing pixel writes its colour and alpha (RGB_ONLY, the default: its colour alone).
    pub fb_only: bool,
    /// TEX0's texture function: TCC 0 (the texel's colour alone, its alpha 0x80: the vertex's alpha is the pixel's), and TFX
    /// (0 MODULATE, 1 DECAL: the vertex colour 0x80, `eedraw`; 2 HIGHLIGHT, 3 HIGHLIGHT2: the vertex's alpha added to the colour).
    rgb_only: bool,
    tfx: u8,
    /// The level from the pixel's own Q (`native::MatU::lodq`): the EE's triangles' (`eedraw`).
    pixel_q: bool,
}

impl Materials {
    /// The same with each level from the pixel's own Q (`pixel_q`: the EE's triangles, whose Q is the GS's).
    pub fn pixel_q(self) -> Materials {
        Materials { pixel_q: true, ..self }
    }

    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, textures: &[uyafmt::core::Texture], qlog: f32, aref: u8) -> Result<Materials, String> {
        let views = textures.iter().map(|t| Materials::view(device, queue, t)).collect();
        let levels = textures.iter().map(|t| t.levels.len()).collect();
        let sizes = textures.iter().map(|t| (t.width, t.height)).collect();
        let hd: Vec<_> = textures.iter().map(|t| crate::texnames::hd(device, queue, t)).collect::<Result<_, String>>()?;
        let alpha_min = textures.iter().zip(&hd).map(|(t, h)| [Self::min_alpha(t), h.as_ref().map_or(0, |h| h.min_alpha)]).collect();
        let hd = hd.into_iter().map(|h| h.map(|h| (h.view, h.levels))).collect();
        let rift = textures.iter().map(|t| crate::texnames::rift(device, queue, t)).collect::<Result<_, String>>()?;
        let lights = textures.iter().map(crate::texnames::light).collect::<Result<_, String>>()?;
        let flat = crate::texnames::flat_material(device, queue)?;
        let rift_sampler = std::sync::Arc::new(crate::texnames::hd_sampler(device, false, false, false, 16));
        // A texture that cannot be named (`texnames::name`'s error, which `texnames::hd` counts) is unnamed here.
        let names = textures.iter().map(|t| crate::texnames::name(t).ok().flatten()).collect();
        Ok(Materials { views, levels, alpha_min, sizes, hd, rift, lights, flat, rift_sampler, names, samplers: FastMap::default(), keys: FastMap::default(), batch_inputs: None, qlog, aref: aref as u16, keep: false, fb_only: false, rgb_only: false, tfx: 0, pixel_q: false })
    }

    /// The textures and their texels' bytes (every mip level, four bytes a texel).
    pub fn census(&self) -> (usize, u64) {
        let bytes = self.sizes.iter().zip(&self.levels).map(|(&(w, h), &n)| (0..n).map(|l| ((w >> l).max(1) * (h >> l).max(1) * 4) as u64).sum::<u64>()).sum();
        (self.views.len(), bytes)
    }

    /// Adds a texture (shared with `other`, the same textures' other material sets); returns its index.
    pub fn push(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, t: &uyafmt::core::Texture, others: &mut [&mut Materials]) -> Result<u32, String> {
        let hd = crate::texnames::hd(device, queue, t)?;
        let rift = crate::texnames::rift(device, queue, t)?;
        let light = crate::texnames::light(t)?;
        Ok(self.add(device, queue, t, hd, rift, light, others))
    }

    /// Adds a texture the port made (no GS texels, so no pack name: the prompt icons' atlas, the vector font's), shared with
    /// `others`; returns its index.
    pub fn push_made(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, t: &uyafmt::core::Texture, others: &mut [&mut Materials]) -> u32 {
        self.add(device, queue, t, None, None, [0.0; 2], others)
    }

    fn add(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, t: &uyafmt::core::Texture, hd: Option<crate::texnames::HdView>, rift: Option<crate::texnames::RiftMat>, light: [f32; 2], others: &mut [&mut Materials]) -> u32 {
        let view = Materials::view(device, queue, t);
        let alpha = [Self::min_alpha(t), hd.as_ref().map_or(0, |h| h.min_alpha)];
        let hd = hd.map(|h| (h.view, h.levels));
        // A texture that cannot be named (`texnames::name`'s error, which `texnames::hd` counts) is unnamed here.
        let name = crate::texnames::name(t).ok().flatten();
        for m in others.iter_mut() {
            m.views.push(view.clone());
            m.levels.push(t.levels.len());
            m.alpha_min.push(alpha);
            m.sizes.push((t.width, t.height));
            m.hd.push(hd.clone());
            m.rift.push(rift.clone());
            m.lights.push(light);
            m.names.push(name);
        }
        self.views.push(view);
        self.levels.push(t.levels.len());
        self.alpha_min.push(alpha);
        self.sizes.push((t.width, t.height));
        self.hd.push(hd);
        self.rift.push(rift);
        self.lights.push(light);
        self.names.push(name);
        self.views.len() as u32 - 1
    }

    /// Adds a texture already on the GPU (a frame buffer of one level, `size` its pixels), shared with `others`; returns its
    /// index. Its alpha channel holds the GS alpha as a byte, as every pipeline writes it.
    pub fn push_view(&mut self, view: wgpu::TextureView, size: (u32, u32), others: &mut [&mut Materials]) -> u32 {
        for m in others.iter_mut() {
            m.views.push(view.clone());
            m.levels.push(1);
            m.alpha_min.push([0; 2]);
            m.sizes.push((size.0 as usize, size.1 as usize));
            m.hd.push(None);
            m.rift.push(None);
            m.lights.push([0.0; 2]);
            m.names.push(None);
        }
        self.views.push(view);
        self.levels.push(1);
        self.alpha_min.push([0; 2]);
        self.sizes.push((size.0 as usize, size.1 as usize));
        self.hd.push(None);
        self.rift.push(None);
        self.lights.push([0.0; 2]);
        self.names.push(None);
        self.views.len() as u32 - 1
    }

    fn min_alpha(t: &uyafmt::core::Texture) -> u8 {
        t.levels.iter().flat_map(|l| l.iter()).map(|p| p[3]).min().expect("a nonempty texture")
    }

    /// The ray shader uses this only as a lower bound; a GPU-produced frame texture has the conservative bound zero.
    pub fn minimum_alpha(&self, key: &Key) -> u8 {
        let t = key.0 as usize;
        let replacement = self.hd[t].is_some() && crate::texnames::region_free(self.sizes[t].0, self.sizes[t].1, key.6);
        self.alpha_min[t][replacement as usize]
    }

    fn view(device: &wgpu::Device, queue: &wgpu::Queue, t: &uyafmt::core::Texture) -> wgpu::TextureView {
        {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("object texture"),
                size: wgpu::Extent3d { width: t.width as u32, height: t.height as u32, depth_or_array_layers: 1 },
                mip_level_count: t.levels.len() as u32,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            for (l, px) in t.levels.iter().enumerate() {
                let (w, h) = ((t.width >> l) as u32, (t.height >> l) as u32);
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: l as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    bytemuck::cast_slice(px),
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
                    wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                );
            }
            tex.create_view(&Default::default())
        }
    }

    /// The number of textures.
    pub fn len(&self) -> usize {
        self.views.len()
    }

    /// The same textures (shared on the GPU) for another TEST_1 AREF.
    pub fn with_aref(&self, aref: u16) -> Materials {
        Materials { views: self.views.clone(), levels: self.levels.clone(), alpha_min: self.alpha_min.clone(), sizes: self.sizes.clone(), hd: self.hd.clone(), rift: self.rift.clone(), lights: self.lights.clone(), flat: self.flat.clone(), rift_sampler: self.rift_sampler.clone(), names: self.names.clone(), samplers: FastMap::default(), keys: FastMap::default(), batch_inputs: None, qlog: self.qlog, aref, keep: false, fb_only: false, rgb_only: false, tfx: 0, pixel_q: self.pixel_q }
    }

    /// The same textures with TEST_1 `test`'s alpha test (ATE, ATST ALWAYS, GEQUAL or GREATER, AREF, AFAIL); its depth test
    /// must be the world's (ZTE on, ZTST GEQUAL, DATE off).
    pub fn with_test(&self, test: u64) -> Result<Materials, String> {
        let (ate, atst, aref, afail) = (test & 1, test >> 1 & 7, (test >> 4 & 0xff) as u16, test >> 12 & 3);
        if test >> 14 != 0x14 {
            return Err(format!("native: TEST_1 {test:#x} for the world"));
        }
        let m = match (ate, atst) {
            (0, _) | (1, 1) => self.with_aref(0),
            (1, 5) => self.with_aref(aref),
            (1, 6) => self.with_aref(aref + 1),
            _ => return Err(format!("native: TEST_1 {test:#x} for the world")),
        };
        match afail {
            0 => Ok(m.keeping()),
            1 => Ok(m.fb_only()),
            3 => Ok(m),
            _ => Err(format!("native: TEST_1 {test:#x} for the world")),
        }
    }

    /// The same materials with TEST_1 ATST NEVER: every pixel fails the alpha test (writes colour only).
    pub fn never_passing(self) -> Materials {
        Materials { aref: 256, ..self }
    }

    /// The same materials with TEX0's texture function: TCC (`rgba`: the texel's alpha taken) and TFX.
    pub fn texture_function(self, rgba: bool, tfx: u8) -> Materials {
        Materials { rgb_only: !rgba, tfx, ..self }
    }

    /// The same materials with TEST_1 AFAIL KEEP: a pixel failing the alpha test writes nothing.
    pub fn keeping(self) -> Materials {
        Materials { keep: true, ..self }
    }

    /// The same materials with TEST_1 AFAIL FB_ONLY: a pixel failing the alpha test writes its colour and alpha.
    pub fn fb_only(self) -> Materials {
        Materials { fb_only: true, ..self }
    }

    /// Whether a pixel failing the alpha test writes nothing (AFAIL KEEP; `raytrace.rs`'s EE layers).
    pub fn keeps(&self) -> bool {
        self.keep
    }

    /// What the ray-traced shadows' alpha test reads of setting `key` (made by `get` at load): the texture its bind group samples
    /// (the pack replacement where `get` takes it), whether it clamps in S and in T, the TEST_1 AREF a texel's alpha passes at
    /// (256: none passes) and whether a pixel can fail (`raytrace.rs`).
    pub fn alpha_test(&self, key: &Key) -> Result<(wgpu::TextureView, bool, bool, u16, bool, Option<crate::texnames::RiftMat>), String> {
        self.keys.get(key).ok_or("native: a texture setting made at load is missing")?;
        // keys' boolean asks the rasterizer to DRAW failed pixels (AFAIL != KEEP), not whether any pixel can FAIL. A KEEP
        // material still needs ray alpha testing; marking it opaque turned cut-out leaves into solid shadow-casting quads.
        let fails = self.aref > 0;
        let (texture, cs, ct, _, _, _, region) = *key;
        let t = texture as usize;
        let view = self.views.get(t).ok_or_else(|| format!("native: object texture {texture} of {}", self.views.len()))?;
        let hd = self.hd[t].as_ref().filter(|_| crate::texnames::region_free(self.sizes[t].0, self.sizes[t].1, region));
        Ok((hd.map_or(view, |h| &h.0).clone(), cs, ct, self.aref, fails, self.rift[t].clone()))
    }

    /// The bind group of setting `key` (made now if new) and whether its pixels can fail the alpha test.
    pub fn get(&mut self, device: &wgpu::Device, layout: &wgpu::BindGroupLayout, key: Key) -> Result<&(wgpu::BindGroup, bool), String> {
        use wgpu::util::DeviceExt;
        if !self.keys.contains_key(&key) {
            let (texture, cs, ct, mmin, k, _, region) = key;
            let t = texture as usize;
            let view = self.views.get(t).ok_or_else(|| format!("native: object texture {texture} of {}", self.views.len()))?;
            // A pack replacement, for a setting without a clamp region (a region names another rectangle of the texture).
            let hd = self.hd[t].as_ref().filter(|_| crate::texnames::region_free(self.sizes[t].0, self.sizes[t].1, region));
            let (view, mmin) = match hd {
                Some((v, _)) => (v, HD_MMIN),
                // The texture filtering option (`enhance`): the original's own levels the same way (level 0 alone for MMIN 0 and 1);
                // but an EE triangle's mipmapped texture keeps the level the GS picks with the game's own K (`pixel_q`: the lights
                // of the fall down level 27's pipe bias theirs to the white smallest levels, where level 0 about them is another
                // texture's; picked by their size on screen they showed level 0, black).
                None if crate::enhance::aniso().is_some() && !(self.pixel_q && mmin >= 2) => (view, if mmin < 2 { ANISO_MMIN0 } else { ANISO_MMIN }),
                None => (view, mmin),
            };
            let (min, mip, levels) = match mmin {
                0 => (wgpu::FilterMode::Nearest, wgpu::MipmapFilterMode::Nearest, 1),
                1 => (wgpu::FilterMode::Linear, wgpu::MipmapFilterMode::Nearest, 1),
                2 => (wgpu::FilterMode::Nearest, wgpu::MipmapFilterMode::Nearest, self.levels[t]),
                3 => (wgpu::FilterMode::Nearest, wgpu::MipmapFilterMode::Linear, self.levels[t]),
                4 => (wgpu::FilterMode::Linear, wgpu::MipmapFilterMode::Nearest, self.levels[t]),
                5 => (wgpu::FilterMode::Linear, wgpu::MipmapFilterMode::Linear, self.levels[t]),
                HD_MMIN => (wgpu::FilterMode::Linear, wgpu::MipmapFilterMode::Linear, hd.map_or(self.levels[t], |h| h.1)),
                HD_MMIN0 | ANISO_MMIN0 => (wgpu::FilterMode::Linear, wgpu::MipmapFilterMode::Linear, 1),
                ANISO_MMIN => (wgpu::FilterMode::Linear, wgpu::MipmapFilterMode::Linear, self.levels[t]),
                m => return Err(format!("native: TEX1 MMIN {m}")),
            };
            if mmin >= HD_MMIN {
                let aniso = if mmin >= ANISO_MMIN { crate::enhance::aniso().expect("the option is on") } else { 16 };
                self.samplers.entry((cs, ct, mmin)).or_insert_with(|| crate::texnames::hd_sampler(device, cs, ct, mmin == HD_MMIN0 || mmin == ANISO_MMIN0, aniso));
            }
            let sampler = self.samplers.entry((cs, ct, mmin)).or_insert_with(|| {
                let mode = |c: bool| if c { wgpu::AddressMode::ClampToEdge } else { wgpu::AddressMode::Repeat };
                device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some("object"),
                    address_mode_u: mode(cs),
                    address_mode_v: mode(ct),
                    mag_filter: wgpu::FilterMode::Linear,
                    min_filter: min,
                    mipmap_filter: mip,
                    ..Default::default()
                })
            });
            // REGION_CLAMP (MINU, MAXU, MINV, MAXV in texels): the texel centres at the region's edges, so that filtering stays in it.
            let (w, h) = (self.sizes[t].0 as f32, self.sizes[t].1 as f32);
            let axis = |mode: u64, min: u64, max: u64, n: f32| if mode == 2 { [(min as f32 + 0.5) / n, (max as f32 + 0.5) / n] } else { [-1e30, 1e30] };
            let (ru, rv) = (axis(region & 3, region >> 4 & 0x3ff, region >> 14 & 0x3ff, w), axis(region >> 2 & 3, region >> 24 & 0x3ff, region >> 34 & 0x3ff, h));
            // The product's form for the shader (`blended`), a GS blend's selectors, and FIX.
            let (mode, sel, fix) = match key.5 {
                Blend::Gs(s, f) => (0.0, s as f32, f as f32),
                Blend::Add(f) | Blend::Scale(f) => (1.0, 0.0, f as f32),
                Blend::AddAlpha => (2.0, 0.0, 0.0),
                Blend::Sub(f) => (3.0, 0.0, f as f32),
                _ => (0.0, 0.0, 0.0),
            };
            let r = self.rift[t].as_ref();
            let [rift, tiles, tiles3, grains, specs, refls, furs, catches, metals] = crate::texnames::RiftMat::uniform(r);
            let u = MatU {
                k: f32::from_bits(k),
                last: (levels - 1) as f32,
                qlog: self.qlog,
                aref: self.aref as f32,
                region: [ru[0], ru[1], rv[0], rv[1]],
                extra: [(mmin >= HD_MMIN) as u32 as f32, sel, fix, mode],
                rift,
                tiles,
                tiles3,
                grains,
                specs,
                refls,
                furs,
                catches,
                metals,
                emit: [self.lights[t][0], self.lights[t][1], self.rgb_only as u32 as f32, self.tfx as f32],
                relief: crate::texnames::RiftMat::relief_uniform(r),
                lodq: [self.pixel_q as u32 as f32, 0.0, 0.0, 0.0],
            };
            let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("material"), contents: bytemuck::bytes_of(&u), usage: wgpu::BufferUsages::UNIFORM });
            let [e3, e4, e5, e6, e7, e8, e9] = crate::texnames::RiftMat::entries(r, &self.flat, &self.rift_sampler);
            if let Some(inputs) = &mut self.batch_inputs {
                let tex = |e: &wgpu::BindGroupEntry<'_>| match &e.resource {
                    wgpu::BindingResource::TextureView(v) => (*v).clone(),
                    _ => unreachable!("Rift material texture binding"),
                };
                inputs.insert(key, crate::shrub_batch::MaterialInput {
                    uniform: u,
                    textures: [view.clone(), tex(&e3), tex(&e5), tex(&e6), tex(&e7), tex(&e8), tex(&e9)],
                    samplers: [sampler.clone(), (*self.rift_sampler).clone()],
                });
            }
            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("material"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: ub.as_entire_binding() },
                    e3,
                    e4,
                    e5,
                    e6,
                    e7,
                    e8,
                    e9,
                ],
            });
            // A pixel's alpha is the texel's times the vertex's: any pixel can fail a test with an AREF (the cockpit glass's vertex
            // alpha is 60), so the failing pixels are drawn whenever there is one (but for KEEP).
            self.keys.insert(key, (bg, !self.keep && self.aref > 0));
        }
        Ok(&self.keys[&key])
    }
}

/// The shrubs' globals of a level: the instance count, the instance array (32 bytes each), the matrix array (64 bytes each), the
/// colour blocks (96 bytes each) and the draw distance limit (f32).
#[derive(Clone, Copy)]
pub(crate) struct ShrubGlobals {
    pub count: u32,
    pub instances: u32,
    pub matrices: u32,
    pub colours: u32,
    pub limit: u32,
}

/// A level's shrubs: the decoded classes, each instance's class (the gameplay block's instance order, the game's array order),
/// the class records' texture bytes, and the materials of the shrub textures.
pub(crate) struct Shrubs {
    classes: Vec<uyafmt::shrub::ShrubClass>,
    records: Vec<[u8; 16]>,
    instance_class: Vec<usize>,
    /// Each class's reach: the farthest of its vertices from its bounding sphere's centre, in class units (`visible`).
    class_reach: Vec<f32>,
    pub materials: Materials,
    /// The instances' chunks (`build`): runs of consecutive instances of at most `SHRUB_CHUNK` vertices (their classes'
    /// triangles, three vertices each), each built into a GPU buffer of its own (a buffer holds at most 256 MB: level 28's shrubs
    /// are 11.9 million vertices).
    chunks: Vec<std::ops::Range<usize>>,
    /// Every instance built on the GPU (`frame`), from the game's arrays as they stood at the level's first frame, built again
    /// when the game changes them.
    built: std::sync::Mutex<Option<ShrubsBuilt>>,
    pub batch: Option<crate::shrub_batch::Batch>,
}

/// The most vertices a chunk of built shrubs holds (32 MB of `ShrubVertex`).
const SHRUB_CHUNK: usize = 1 << 20;

/// The threads a level's shrubs are built on (`Shrubs::build`), a chunk at a time each.
const SHRUB_BUILDERS: usize = 4;

/// The level's shrub instances on the GPU (`Shrubs::frame`): the triangles of each, in the game's array order, and what they
/// were built from.
struct ShrubsBuilt {
    /// The game's matrix array, the instances' colour block numbers (+0x18) and the colour blocks, as the build read them.
    matrices: Vec<u8>,
    blocks: Vec<u16>,
    colours: Vec<u8>,
    /// Per instance its chunk and its runs of one texture setting: (setting, first vertex in the chunk, vertices).
    runs: Vec<(usize, Vec<(Key, u32, u32)>)>,
    /// Each chunk's vertices (None: no triangles).
    vertices: Vec<Option<wgpu::Buffer>>,
    /// Each instance's fog value as float bits, followed by the batched path's current packed colours, and per frame set
    /// (`native::SETS`) its buffer and bind group
    /// (`Pipelines::shrub_layout`; empty when no instance has a triangle).
    fog: Vec<u32>,
    fog_gpu: Vec<(wgpu::Buffer, wgpu::BindGroup)>,
    batch_vertices: Vec<Option<wgpu::Buffer>>,
}

/// A shadow caster (`shadows.rs`): its bounding sphere and its draws (with the test set each takes: `Ties::materials`).
pub(crate) struct Caster {
    pub centre: [f32; 3],
    pub radius: f32,
    pub draws: Vec<(usize, Draw)>,
}

/// One class's triangles for the ray tracing (`raytrace.rs`): its vertices in the class's own units (the raw positions over
/// 1024; an instance's transform, `Ties::rt_instances`, `Shrubs::rt_instances`, puts them in the world), each vertex's S/Q and
/// T/Q and the word of an instance's colours it takes (a tie: from the first its LOD reads, `Ties::rt_instances`; a shrub: of
/// its instance's colour block), and its triangles' corners (indices into the vertices, three a triangle) by material: the
/// material set (a tie packet's first test, `Ties::materials`; 0 for the shrubs), the texture setting and whether the game
/// draws a shine over them (a tie packet's second part, its texture coordinates from the eye's reflection or the direction
/// to a light: `uyafmt::tie`), in the class's order. `normal`: each vertex's normal where the game gives one (a tie packet whose
/// second part is the eye's reflection: its packed normals, one a vertex, `uyafmt::tie::TieSecond::normals`) as four signed bytes,
/// the fourth 127; 0 where it gives none.
pub(crate) struct RtClass {
    pub pos: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    pub colour: Vec<u32>,
    pub normal: Vec<u32>,
    pub runs: Vec<(usize, Key, bool, Vec<u32>)>,
}

/// A normal (any length) as four signed bytes, the fourth 127 (`RtClass::normal`); 0 for a zero normal.
fn pack_normal(n: [i32; 3]) -> u32 {
    let l = ((n[0] as f32).powi(2) + (n[1] as f32).powi(2) + (n[2] as f32).powi(2)).sqrt();
    if l == 0.0 {
        return 0;
    }
    let b = n.map(|c| (c as f32 / l * 127.0).round() as i8 as u8);
    u32::from_le_bytes([b[0], b[1], b[2], 127])
}

/// The 3 x 4 row-major transform of an instance's matrix at `m` in `ram` from its class's units into the world, as `Ties::casters`
/// and `Shrubs::build` place a vertex: the rows times row 3's w (the class's scale), row 3 the place.
fn rt_transform(ram: &[u8], m: usize) -> Result<[f32; 12], String> {
    let row = |r: usize| -> Result<[f32; 3], String> { Ok([rd_f32(ram, m + 16 * r)?, rd_f32(ram, m + 16 * r + 4)?, rd_f32(ram, m + 16 * r + 8)?]) };
    let rows = [row(0)?, row(1)?, row(2)?, row(3)?];
    let s = rd_f32(ram, m + 0x3c)?;
    Ok([0, 1, 2].map(|k| [s * rows[0][k], s * rows[1][k], s * rows[2][k], rows[3][k]]).concat().try_into().expect("12 values"))
}

/// One run of triangles under one material: vertices `first .. first + count` of the frame's object vertex buffer.
pub(crate) struct Draw {
    pub key: Key,
    pub first: u32,
    pub count: u32,
}

/// A shrub strip's texture setting: its block entry's texture (the class record's byte), CLAMP, MMIN and K.
fn strip_key(bl: &uyafmt::shrub::ShrubBlock, st: &uyafmt::shrub::ShrubStrip, rec: &[u8; 16]) -> Result<Key, String> {
    let te = bl.textures.get(st.texture).ok_or("native: shrub strip texture entry")?;
    let texture = *rec.get(te.index as usize).ok_or("native: shrub texture index byte")? as u32;
    if te.wms > 1 || te.wmt > 1 {
        return Err(format!("native: shrub CLAMP WMS {} WMT {}", te.wms, te.wmt));
    }
    // ALPHA_1 0x44 before the shrubs (Veldin field 2400, the GS stream); the block's entries send no ALPHA_1.
    Ok((texture, te.wms == 1, te.wmt == 1, te.mmin, tex1_k(te.k).to_bits(), Blend::Alpha, 0))
}

fn rd_u32(ram: &[u8], a: usize) -> Result<u32, String> {
    ram.get(a..a + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).ok_or_else(|| format!("native: RAM {a:#x}"))
}

fn rd_f32(ram: &[u8], a: usize) -> Result<f32, String> {
    rd_u32(ram, a).map(f32::from_bits)
}

impl Shrubs {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout, view_layout: &wgpu::BindGroupLayout, fog_layout: &wgpu::BindGroupLayout, classes: Vec<uyafmt::shrub::ShrubClass>, textures: &[uyafmt::core::Texture], records: &[uyafmt::core::ClassRecord], gameplay: &uyafmt::gameplay::Gameplay) -> Result<Shrubs, String> {
        let instance_class = gameplay
            .shrubs
            .iter()
            .enumerate()
            .map(|(i, s)| records.iter().position(|c| c.id == s.class).ok_or_else(|| format!("native: shrub instance {i}: class {} has no record", s.class)))
            .collect::<Result<Vec<_>, String>>()?;
        let records: Vec<[u8; 16]> = records.iter().map(|c| c.textures).collect();
        // TEST_1 0x5340B before the shrubs (Veldin field 2400, the GS stream): AREF 0x40. Program 4's 1/Q is a 32nd of the depth.
        let mut materials = Materials::new(device, queue, textures, -5.0, 0x40)?;
        if crate::shrub_batch::supported(device) && !crate::surfaces::on() {
            materials.batch_inputs = Some(FastMap::default());
        }
        // Every texture setting the classes use, made now (the frame only reads them).
        for (c, rec) in classes.iter().zip(&records) {
            for bl in &c.blocks {
                for st in &bl.strips {
                    materials.get(device, layout, strip_key(bl, st, rec)?)?;
                }
            }
        }
        let class_reach = classes
            .iter()
            .map(|c| {
                let far = |v: &uyafmt::shrub::ShrubVertex| (0..3).map(|k| (v.pos[k] / 1024.0 - c.centre[k]).powi(2)).sum::<f32>().sqrt();
                c.blocks.iter().flat_map(|b| &b.vertices).map(far).fold(0.0, f32::max)
            })
            .collect();
        // The chunks: consecutive instances while their vertices fit in one.
        let class_vertices: Vec<usize> = classes.iter().map(|c| c.blocks.iter().flat_map(|b| &b.strips).map(|s| 3 * s.vertices.len().saturating_sub(2)).sum()).collect();
        let mut chunks: Vec<std::ops::Range<usize>> = Vec::new();
        let (mut from, mut held) = (0, 0);
        for (i, &c) in instance_class.iter().enumerate() {
            let n = class_vertices[c];
            if held + n > SHRUB_CHUNK && i > from {
                chunks.push(from..i);
                (from, held) = (i, 0);
            }
            held += n;
        }
        chunks.push(from..instance_class.len());
        let batch = materials.batch_inputs.take().filter(|inputs| !inputs.is_empty()).map(|inputs| crate::shrub_batch::Batch::new(device, view_layout, fog_layout, inputs)).transpose()?;
        Ok(Shrubs { classes, records, instance_class, class_reach, materials, chunks, built: std::sync::Mutex::new(None), batch })
    }

    /// Every built instance not hidden (+0x16 bit 0), as the shadows' casters (`shadows.rs`): `f(its chunk's vertices, a run's
    /// setting, its vertices, the instance's sphere centre and radius)` for each run, in the game's order (`visible`'s sphere).
    /// Nothing before the shrubs' first frame has built them.
    pub fn casters(&self, ram: &[u8], g: &ShrubGlobals, mut f: impl FnMut(&wgpu::Buffer, Key, std::ops::Range<u32>, [f32; 3], f32)) -> Result<(), String> {
        let built = self.built.lock().map_err(|_| "native: the built shrubs' lock is poisoned")?;
        let Some(b) = built.as_ref() else { return Ok(()) };
        let (arr, mats) = (rd_u32(ram, g.instances as usize)? as usize, rd_u32(ram, g.matrices as usize)? as usize);
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        for (i, (chunk, runs)) in b.runs.iter().enumerate() {
            if *ram.get(arr + 32 * i + 0x16).ok_or("native: a shrub instance past RAM")? & 1 != 0 || runs.is_empty() {
                continue;
            }
            let Some(vb) = &b.vertices[*chunk] else { continue };
            let m = mats + 64 * i;
            let row = |r: usize| -> Result<[f32; 3], String> { Ok([rd_f32(ram, m + 16 * r)?, rd_f32(ram, m + 16 * r + 4)?, rd_f32(ram, m + 16 * r + 8)?]) };
            let rows = [row(0)?, row(1)?, row(2)?, row(3)?];
            let scale = rd_f32(ram, m + 0x3c)?;
            let c = &self.classes[self.instance_class[i]].centre;
            let mid = [0, 1, 2].map(|k| scale * (c[0] * rows[0][k] + c[1] * rows[1][k] + c[2] * rows[2][k]) + rows[3][k]);
            let norm = (dot(rows[0], rows[0]) + dot(rows[1], rows[1]) + dot(rows[2], rows[2])).sqrt();
            let reach = self.class_reach[self.instance_class[i]] * scale.abs() * norm * 1.001 + 0.01;
            for &(key, first, count) in runs {
                f(vb, key, first..first + count, mid, reach);
            }
        }
        Ok(())
    }

    /// Each class's triangles in its own units, by strip texture setting (`RtClass`, material set 0), as `build` makes an
    /// instance's: the ray-traced shadows' meshes.
    pub fn rt_classes(&self) -> Result<Vec<RtClass>, String> {
        let mut out = Vec::with_capacity(self.classes.len());
        for (c, class) in self.classes.iter().enumerate() {
            let rec = self.records.get(c).ok_or_else(|| format!("native: shrub class {c} has no record"))?;
            let mut m = RtClass { pos: Vec::new(), uv: Vec::new(), colour: Vec::new(), normal: Vec::new(), runs: Vec::new() };
            for bl in &class.blocks {
                let base = m.pos.len() as u32;
                for v in &bl.vertices {
                    if v.colour >= 24 {
                        return Err(format!("native: shrub class {c}: colour index {} of 24", v.colour));
                    }
                    m.pos.push(v.pos.map(|x| x / 1024.0));
                    m.uv.push([v.st[0] / v.st[2], v.st[1] / v.st[2]]);
                    m.colour.push(v.colour as u32);
                    m.normal.push(0);
                }
                for st in &bl.strips {
                    let key = strip_key(bl, st, rec)?;
                    if m.runs.last().is_none_or(|r| r.1 != key) {
                        m.runs.push((0, key, false, Vec::new()));
                    }
                    let run = &mut m.runs.last_mut().expect("pushed above").3;
                    for k in 2..st.vertices.len() {
                        run.extend([st.vertices[k - 2], st.vertices[k - 1], st.vertices[k]].map(|vi| base + vi as u32));
                    }
                }
            }
            out.push(m);
        }
        Ok(out)
    }

    /// Every instance not hidden (+0x16 bit 0), as `casters` takes them: its class, its transform into the world and its colour
    /// block (+0x18: the block's 24 words, `rt_colours`).
    /// What `rt_instances` reads: the instance array and the matrix array as RAM holds them (`raytrace::LevelRt::frame` reads the
    /// instances again only when these change).
    pub fn rt_state<'r>(&self, ram: &'r [u8], g: &ShrubGlobals) -> Result<[&'r [u8]; 2], String> {
        let n = self.instance_class.len();
        let (arr, mats) = (rd_u32(ram, g.instances as usize)? as usize, rd_u32(ram, g.matrices as usize)? as usize);
        Ok([ram.get(arr..arr + 32 * n).ok_or("native: the shrub instances lie past RAM")?, ram.get(mats..mats + 64 * n).ok_or("native: the shrub matrices lie past RAM")?])
    }

    pub fn rt_instances(&self, ram: &[u8], g: &ShrubGlobals) -> Result<Vec<(usize, [f32; 12], u32)>, String> {
        let n = rd_u32(ram, g.count as usize)? as usize;
        if n != self.instance_class.len() {
            return Err(format!("native: the game has {n} shrub instances, the gameplay block {}", self.instance_class.len()));
        }
        let (arr, mats) = (rd_u32(ram, g.instances as usize)? as usize, rd_u32(ram, g.matrices as usize)? as usize);
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            if *ram.get(arr + 32 * i + 0x16).ok_or("native: a shrub instance past RAM")? & 1 != 0 {
                continue;
            }
            let a = arr + 32 * i + 0x18;
            out.push((self.instance_class[i], rt_transform(ram, mats + 64 * i)?, u16::from_le_bytes([ram[a], ram[a + 1]]) as u32));
        }
        Ok(out)
    }

    /// The first `blocks` colour blocks (`rt_instances`), each word as `build` colours a vertex with it: its own colour, the
    /// block's first alpha.
    pub fn rt_colours(&self, ram: &[u8], g: &ShrubGlobals, blocks: u32, out: &mut Vec<u32>) -> Result<(), String> {
        let cols = rd_u32(ram, g.colours as usize)? as usize;
        let all = ram.get(cols..cols + 96 * blocks as usize).ok_or("native: the shrub colour blocks lie past RAM")?;
        for b in all.chunks_exact(96) {
            out.extend(b.chunks_exact(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], b[3]])));
        }
        Ok(())
    }

    /// The instances the frame draws, in the game's array order: not hidden (+0x16 bit 0), nearer than their own distance (+0x10)
    /// and the level's limit (at least `reach`, DRAW DISTANCE FAR's: `range.rs`), not wholly behind the eye, and not wholly
    /// outside one of the picture's side planes.
    pub fn visible(&self, ram: &[u8], g: &ShrubGlobals, cam: &Camera, reach: Option<f32>) -> Result<Vec<usize>, String> {
        let n = rd_u32(ram, g.count as usize)? as usize;
        if n != self.instance_class.len() {
            return Err(format!("native: the game has {n} shrub instances, the gameplay block {}", self.instance_class.len()));
        }
        let (arr, mats) = (rd_u32(ram, g.instances as usize)? as usize, rd_u32(ram, g.matrices as usize)? as usize);
        let limit = rd_f32(ram, g.limit as usize)?;
        let limit = match reach {
            Some(r) if r > limit => r,
            _ => limit,
        };
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        // The picture's side planes, as `Camera::matrix` projects (x the right axis, forward x up, over tan[0]; y the up axis over
        // tan[1]; w the view depth): inside is w - x, w + x, w - y and w + y at least 0, each the dot of a world direction with the
        // offset from the eye. A triangle wholly outside one is clipped whole and draws no pixel.
        let (f, u) = (cam.forward, cam.up);
        let r = [f[1] * u[2] - f[2] * u[1], f[2] * u[0] - f[0] * u[2], f[0] * u[1] - f[1] * u[0]];
        let planes = [(1.0, r, cam.tan[0]), (-1.0, r, cam.tan[0]), (1.0, u, cam.tan[1]), (-1.0, u, cam.tan[1])].map(|(s, axis, t)| {
            let n = [0, 1, 2].map(|k| f[k] - s * axis[k] / t);
            (n, dot(n, n).sqrt())
        });
        let mut v = Vec::new();
        for i in 0..n {
            let a = arr + 32 * i;
            if ram[a + 0x16] & 1 != 0 {
                continue;
            }
            let centre = [rd_f32(ram, a)?, rd_f32(ram, a + 4)?, rd_f32(ram, a + 8)?];
            let radius = rd_f32(ram, a + 0xc)?;
            let depth = dot([centre[0] - cam.eye[0], centre[1] - cam.eye[1], centre[2] - cam.eye[2]], cam.forward);
            // Every shrub out to the level's shrub limit: the game's own cull draws each only within its own distance (+0x10: 32
            // or 64 units for most of level 28's, the pipe's lights and bolts among them), and they came into sight a few steps
            // away (the player: "the blue looking bulb things near the pipe entrance, the pipe lights and bolts").
            if depth >= limit || depth + radius <= 0.0 {
                continue;
            }
            // The side planes test the port's own sphere around the instance's vertices, not the game's (+0xC, the class's radius
            // times the matrix's longest axis): the class's centre carried through the rows as `build` carries a vertex, and
            // the class's reach (its farthest vertex from that centre) times the scale and the rows' norm, so every vertex the
            // instance draws lies in it; widened by a thousandth and 0.01 for the rounding of the vertices' own sums.
            let m = mats + 64 * i;
            let row = |r: usize| -> Result<[f32; 3], String> { Ok([rd_f32(ram, m + 16 * r)?, rd_f32(ram, m + 16 * r + 4)?, rd_f32(ram, m + 16 * r + 8)?]) };
            let rows = [row(0)?, row(1)?, row(2)?, row(3)?];
            let scale = rd_f32(ram, m + 0x3c)?;
            let c = &self.classes[self.instance_class[i]].centre;
            let mid = [0, 1, 2].map(|k| scale * (c[0] * rows[0][k] + c[1] * rows[1][k] + c[2] * rows[2][k]) + rows[3][k] - cam.eye[k]);
            let norm = (dot(rows[0], rows[0]) + dot(rows[1], rows[1]) + dot(rows[2], rows[2])).sqrt();
            let reach = self.class_reach[self.instance_class[i]] * scale.abs() * norm * 1.001 + 0.01;
            if planes.iter().any(|&(p, len)| dot(p, mid) < -reach * len) {
                continue;
            }
            v.push(i);
        }
        Ok(v)
    }

    /// The frame's shrubs: `instances` (from `visible`, in the game's order) as runs of every instance's triangles on the GPU
    /// (`build`: from the game's arrays at the level's first frame, and again whenever the game changes an instance's matrix,
    /// colour block or alpha; batched RGB lighting updates are uploaded separately), each instance's fog value from its
    /// origin's view depth, written into frame set `set`'s buffer; `draws` with the
    /// chunk each draws from (vertices `first .. first + count` of it: an instance's runs, and the next instance's when its
    /// triangles follow in the chunk under the same setting), and the chunks' vertices and the fog values' group to draw them with
    /// (`Pipelines::draw_shrubs`), None when no instance has a triangle. The triangles drawn and their order are those the
    /// vertices made each frame gave before (level 1's shrubs out to the level's limit took 9 to 17 ms a frame on four threads,
    /// building 1.2 million vertices a frame), and then a frame's indices into them (an index for every vertex drawn); now only
    /// the fog values are written each frame.
    #[allow(clippy::too_many_arguments)]
    ///
    /// `groups` gets where each run of one chunk and setting begins in `draws` (the draws the frame's indices gave before: the
    /// shrubs' threads split their bundles between them as they split those draws).
    pub fn frame(&self, device: &wgpu::Device, queue: &wgpu::Queue, fog_layout: &wgpu::BindGroupLayout, ram: &[u8], g: &ShrubGlobals, cam: &Camera, fog: &uyafmt::gameplay::Fog, instances: &[usize], set: usize, draws: &mut Vec<(usize, Draw)>, groups: &mut Vec<usize>) -> Result<Option<(Vec<Option<wgpu::Buffer>>, Vec<Option<wgpu::Buffer>>, wgpu::BindGroup)>, String> {
        let n = self.instance_class.len();
        let (arr, mats, cols) = (rd_u32(ram, g.instances as usize)? as usize, rd_u32(ram, g.matrices as usize)? as usize, rd_u32(ram, g.colours as usize)? as usize);
        let mut built = self.built.lock().map_err(|_| "native: the built shrubs' lock is poisoned")?;
        let same = built.as_ref().is_some_and(|b| {
            ram.get(mats..mats + 64 * n) == Some(&b.matrices[..])
                && (0..n).all(|i| ram.get(arr + 32 * i + 0x18..arr + 32 * i + 0x1a).is_some_and(|w| u16::from_le_bytes([w[0], w[1]]) == b.blocks[i]))
                && ram.get(cols..cols + b.colours.len()).is_some_and(|colours| {
                    if self.batch.is_some() {
                        // RGB lighting changes go straight to the GPU. The shadow caster still reads the baked first alpha
                        // of each block, so rebuild if that changes (or if the transforms/block assignments above change).
                        colours.chunks_exact(96).zip(b.colours.chunks_exact(96)).all(|(a, b)| a[3] == b[3])
                    } else { colours == &b.colours[..] }
                })
        });
        if !same {
            *built = Some(self.build(device, queue, fog_layout, ram, g, crate::surfaces::Pick::new(cam).as_ref())?);
        }
        let b = built.as_mut().expect("built above");
        let Some((fog_buffer, fog_group)) = b.fog_gpu.get(set) else { return Ok(None) };
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let fog_f = |depth: f32| -> f32 {
            let f = fog.near_f + (fog.far_f - fog.near_f) * (depth - fog.near) / (fog.far - fog.near);
            f.clamp(fog.near_f.min(fog.far_f), fog.near_f.max(fog.far_f)).floor()
        };
        if self.batch.is_some() {
            b.fog[..n].fill((-1.0f32).to_bits());
            for (dst, block) in b.fog[n..].chunks_exact_mut(24).zip(ram[cols..cols + b.colours.len()].chunks_exact(96)) {
                for (dst, c) in dst.iter_mut().zip(block.chunks_exact(4)) {
                    *dst = u32::from_le_bytes([c[0], c[1], c[2], block[3]]);
                }
            }
        }
        for &i in instances {
            // The fog value F of the instance's origin (row 3 of its matrix), as the terrain's.
            let m = mats + 64 * i;
            let origin = [rd_f32(ram, m + 48)?, rd_f32(ram, m + 52)?, rd_f32(ram, m + 56)?];
            b.fog[i] = fog_f(dot([origin[0] - cam.eye[0], origin[1] - cam.eye[1], origin[2] - cam.eye[2]], cam.forward)).to_bits();
            let (chunk, runs) = &b.runs[i];
            for &(key, first, count) in runs {
                match draws.last_mut() {
                    Some((c, d)) if c == chunk && d.key == key && d.first + d.count == first => d.count += count,
                    Some((c, d)) if c == chunk && d.key == key => draws.push((*chunk, Draw { key, first, count })),
                    _ => {
                        groups.push(draws.len());
                        draws.push((*chunk, Draw { key, first, count }));
                    }
                }
            }
        }
        queue.write_buffer(fog_buffer, 0, bytemuck::cast_slice(&b.fog));
        Ok(Some((b.vertices.clone(), b.batch_vertices.clone(), fog_group.clone())))
    }

    /// Every instance's triangles from the game's arrays in `ram`, in its order, each vertex carrying its instance's number for
    /// its fog value, on the GPU: each chunk built and copied into its buffer as it is mapped at creation.
    fn build(&self, device: &wgpu::Device, queue: &wgpu::Queue, fog_layout: &wgpu::BindGroupLayout, ram: &[u8], g: &ShrubGlobals, pick: Option<&crate::surfaces::Pick>) -> Result<ShrubsBuilt, String> {
        use wgpu::util::DeviceExt;
        let t = std::time::Instant::now();
        let n = self.instance_class.len();
        let (arr, mats, cols) = (rd_u32(ram, g.instances as usize)? as usize, rd_u32(ram, g.matrices as usize)? as usize, rd_u32(ram, g.colours as usize)? as usize);
        let matrices = ram.get(mats..mats + 64 * n).ok_or("native: the shrub matrices lie past RAM")?.to_vec();
        let blocks: Vec<u16> = (0..n).map(|i| ram.get(arr + 32 * i + 0x18..arr + 32 * i + 0x1a).map(|w| u16::from_le_bytes([w[0], w[1]])).ok_or_else(|| format!("native: shrub instance {i} lies past RAM"))).collect::<Result<_, _>>()?;
        let colours_len = blocks.iter().map(|&k| 96 * (k as usize + 1)).fold(0, usize::max);
        let colours = ram.get(cols..cols + colours_len).ok_or("native: the shrub colour blocks lie past RAM")?.to_vec();
        // One chunk: its instances' runs, and its vertices' buffer, written as they are made.
        let chunk = |c: usize| -> Result<(Vec<Vec<(Key, u32, u32)>>, Option<wgpu::Buffer>), String> {
            let range = self.chunks[c].clone();
            // The chunk's runs first (the vertices each takes), then its vertices into the buffer.
            let mut runs = Vec::with_capacity(range.len());
            let mut total = 0u32;
            for i in range.clone() {
                let (class, rec) = (&self.classes[self.instance_class[i]], &self.records[self.instance_class[i]]);
                let mut own: Vec<(Key, u32, u32)> = Vec::new();
                for bl in &class.blocks {
                    for st in &bl.strips {
                        let key = strip_key(bl, st, rec)?;
                        if own.last().is_none_or(|r| r.0 != key) {
                            own.push((key, total, 0));
                        }
                        total += 3 * st.vertices.len().saturating_sub(2) as u32;
                        let r = own.last_mut().expect("pushed above");
                        r.2 = total - r.1;
                    }
                }
                runs.push(own);
            }
            if total == 0 {
                return Ok((runs, None));
            }
            let buffer = device.create_buffer(&wgpu::BufferDescriptor { label: Some("shrubs"), size: total as u64 * std::mem::size_of::<ShrubVertex>() as u64, usage: wgpu::BufferUsages::VERTEX, mapped_at_creation: true });
            {
                let mut out: Vec<ShrubVertex> = Vec::with_capacity(total as usize);
                // A block's vertices as this instance draws them, each made the first time a strip asks for it: a strip's vertex
                // is in up to three of its triangles.
                let mut made: Vec<Option<ShrubVertex>> = Vec::new();
                for i in range {
                    let a = arr + 32 * i;
                    let m = mats + 64 * i;
                    let row = |r: usize| -> Result<[f32; 3], String> { Ok([rd_f32(ram, m + 16 * r)?, rd_f32(ram, m + 16 * r + 4)?, rd_f32(ram, m + 16 * r + 8)?]) };
                    let rows = [row(0)?, row(1)?, row(2)?, row(3)?];
                    // Row 3's w: the class's scale the loader writes (the instance's centre, +0, is the class's centre times it,
                    // carried through the rows: Veldin field 2400, every shrub).
                    let scale = rd_f32(ram, m + 0x3c)?;
                    let block = cols + 96 * u16::from_le_bytes([ram[a + 0x18], ram[a + 0x19]]) as usize;
                    let colour = |c: u16| -> Result<[u8; 4], String> {
                        if c >= 24 {
                            return Err(format!("native: shrub colour index {c} of 24"));
                        }
                        let w = ram.get(block + 4 * c as usize..block + 4 * c as usize + 4).ok_or("native: shrub colour block past RAM")?;
                        Ok([w[0], w[1], w[2], ram[block + 3]])
                    };
                    for bl in &self.classes[self.instance_class[i]].blocks {
                        made.clear();
                        made.resize(bl.vertices.len(), None);
                        for st in &bl.strips {
                            // `uya native --surfaces`: the strip's texture's name, for its world size per repeat.
                            let surface = match crate::surfaces::on() {
                                true => self.materials.names.get(strip_key(bl, st, &self.records[self.instance_class[i]])?.0 as usize).copied().flatten(),
                                false => None,
                            };
                            let mut vert = |vi: usize| -> Result<ShrubVertex, String> {
                                if let Some(o) = made[vi] {
                                    return Ok(o);
                                }
                                let v = &bl.vertices[vi];
                                // The raw s16 position in 1/1024 class units (every vertex then lies in the class's bounding
                                // sphere, +0xC), times the class's scale.
                                let p = v.pos.map(|c| c / 1024.0 * scale);
                                let pos = [0, 1, 2].map(|k| p[0] * rows[0][k] + p[1] * rows[1][k] + p[2] * rows[2][k] + rows[3][k]);
                                let o = ShrubVertex { pos, st: v.st, colour: colour(v.colour)?, instance: i as u32 };
                                made[vi] = Some(o);
                                Ok(o)
                            };
                            for k in 2..st.vertices.len() {
                                for vi in [st.vertices[k - 2], st.vertices[k - 1], st.vertices[k]] {
                                    out.push(vert(vi)?);
                                }
                                if let Some(name) = surface {
                                    let n = out.len();
                                    let t = [out[n - 3], out[n - 2], out[n - 1]];
                                    crate::surfaces::object_triangle(name, t.map(|v| v.pos), t.map(|v| [v.st[0], v.st[1]]));
                                }
                                if let Some(pick) = pick {
                                    let n = out.len();
                                    let t = [out[n - 3], out[n - 2], out[n - 1]];
                                    if let Some(distance) = pick.hit(t.map(|v| v.pos)) {
                                        let key = strip_key(bl, st, &self.records[self.instance_class[i]])?;
                                        let name = self.materials.names[key.0 as usize].map(|n| n.stem());
                                        println!("pick: shrub {i} class {} distance {distance:.4} texture {name:?} key {key:?} colours {:?}", self.instance_class[i], t.map(|v| v.colour));
                                    }
                                }
                            }
                        }
                    }
                }
                buffer.slice(..).get_mapped_range_mut().map_err(|e| format!("native: the shrubs' buffer, mapped at creation: {e}"))?.copy_from_slice(bytemuck::cast_slice(&out));
            }
            buffer.unmap();
            Ok((runs, Some(buffer)))
        };
        // `SHRUB_BUILDERS` chunks at a time, each on a thread; each batch's vertices copied to the GPU and their staging freed
        // before the next batch is built (level 28's 12 chunks, 380 MB, staged at once took its peak memory from 1.5 to 1.8 GB).
        let mut done: Vec<(usize, Vec<Vec<(Key, u32, u32)>>, Option<wgpu::Buffer>)> = Vec::with_capacity(self.chunks.len());
        let all: Vec<usize> = (0..self.chunks.len()).collect();
        let chunk = &chunk;
        for batch in all.chunks(SHRUB_BUILDERS) {
            let built = std::thread::scope(|s| -> Result<Vec<_>, String> {
                let workers: Vec<_> = batch.iter().map(|&c| s.spawn(move || chunk(c).map(|(runs, buffer)| (c, runs, buffer)))).collect();
                workers.into_iter().map(|w| w.join().unwrap_or_else(|p| std::panic::resume_unwind(p))).collect()
            })?;
            done.extend(built);
            queue.submit([]);
            device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("native: GPU wait: {e}"))?;
        }
        done.sort_by_key(|d| d.0);
        let mut runs = Vec::with_capacity(n);
        let mut vertices = Vec::with_capacity(done.len());
        for (c, r, buffer) in done {
            runs.extend(r.into_iter().map(|own| (c, own)));
            vertices.push(buffer);
        }
        // Batched vertices fetch the current lighting colour after the per-instance fog words. Keep both in the frame set
        // so replays retain the original frame's colours while another frame is prepared.
        let fog = vec![0u32; n + if self.batch.is_some() { colours.len() / 4 } else { 0 }];
        let sets = if vertices.iter().any(Option::is_some) { crate::native::SETS } else { 0 };
        let fog_gpu = (0..sets)
            .map(|_| {
                let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("shrub fog values"), contents: bytemuck::cast_slice(&fog), usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST });
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("shrub fog values"), layout: fog_layout, entries: &[wgpu::BindGroupEntry { binding: 1, resource: buffer.as_entire_binding() }] });
                (buffer, group)
            })
            .collect();
        let total: u64 = vertices.iter().flatten().map(|b| b.size()).sum::<u64>() / std::mem::size_of::<ShrubVertex>() as u64;
        println!("native: shrubs built: {n} instances, {total} vertices in {} chunks, {:.1} ms", vertices.len(), t.elapsed().as_secs_f64() * 1e3);
        let batch_vertices = match &self.batch {
            Some(batch) => batch.vertex_materials(device, &vertices, &runs, |chunk| {
                let mut slots = Vec::new();
                for i in self.chunks[chunk].clone() {
                    let base = n as u32 + 24 * blocks[i] as u32;
                    for bl in &self.classes[self.instance_class[i]].blocks {
                        for st in &bl.strips {
                            for k in 2..st.vertices.len() {
                                slots.extend([st.vertices[k - 2], st.vertices[k - 1], st.vertices[k]].map(|v| base + bl.vertices[v].colour as u32));
                            }
                        }
                    }
                }
                slots
            }),
            None => Vec::new(),
        };
        Ok(ShrubsBuilt { matrices, blocks, colours, runs, vertices, fog, fog_gpu, batch_vertices })
    }
}

/// The ties' globals of a level: the instance count, the instance array (32 bytes each), the matrix array (64 bytes each) and
/// the colour buffers' base.
#[derive(Clone, Copy)]
pub(crate) struct TieGlobals {
    pub count: u32,
    pub instances: u32,
    pub matrices: u32,
    pub colours: u32,
    /// The frame's list of ties drawn, its size in bytes, and the variable holding the record table (uyafmt::levelcode::TieAddrs).
    pub list: u32,
    pub list_bytes: u32,
    pub records: u32,
    /// The TEST_1 values of a packet's first draw: [0] when its entry flags have bit 3, [1] otherwise and for second parts
    /// (uyafmt::levelcode::TieAddrs::tests).
    pub tests: [u64; 2],
    /// The level's light sets (`lighting::sets_from_ram`).
    pub light_sets: u32,
}

/// The tie light pass's source of a colour word holding the class's constant colour (`uyafmt::tie::TieClass::light_slots`).
const CONSTANT_SOURCE: u16 = 511;

/// A level's ties: the decoded classes, each instance's class, the class records' texture bytes and the tie materials.
///
/// Each frame, from the game's tie arrays in RAM (func_L01_004AFA28's cull and func_L01_004B0990's DMA chain):
/// - an instance is drawn when the game's list of the frame's ties names it and it has a colour buffer (+0x14 not 0xFFFF: the
///   light pass has lit it); the game's cull lists it when its +0x1E bit 2 is clear, its +0x10 mask ANDed with its +0x11 zone's
///   byte of the frame's zone record is not zero, its class's +0x3C has neither bit 0 nor 3, and its centre's view depth (+0,
///   radius +0xC) is below 16 x min(+0x1C, the limit) and its sphere is not wholly behind the eye;
/// - its LOD is the one the game drew it at (+0x1F, the LOD its colours were lit for: the cull picks it by the view depth and
///   the class's distances as its cull reads them, the port's, `range.rs`) and its morph factor k the one
///   the record the chain sent carries (quadword 4's x);
/// - a vertex is (position, plus k x its morph offset) / 1024 x the class's scale (row 3's w), through the instance's matrix rows
///   (the instance's centre is the class's centre carried the same way: Veldin field 2400, every tie);
/// - its colour is word `colour` of the packet's colours at the instance's colour buffer ([colours] + 16 x +0x14) plus the
///   packet's `colour_offset`;
/// - the pixel: the packet texture's register (TEX0 from the class record's byte, TEX1 K and MMIN, CLAMP, ALPHA_1), the TEST_1
///   the chain sends before the packet (`TieGlobals::tests`: no alpha test when the entry flags have bit 3, a part whose alpha
///   its second part's AddDst reads; else GEQUAL 0x40), fog per vertex as the terrain's; program 2's Q is 1 / w, w a 32nd of
///   the depth;
/// - a packet with a second part (entry flags 2 or 4) is drawn a second time over the same triangles: the second texture list
///   (a triangle's texture slot names its register there), every vertex the second part's colour, and the texture coordinates
///   the EE computed and the chain sends before the second draw (`second_coordinates`).
pub(crate) struct Ties {
    classes: Vec<uyafmt::tie::TieClass>,
    instance_class: Vec<usize>,
    /// The materials of each of `TieGlobals::tests`.
    pub materials: [Materials; 2],
    /// Per class, LOD and packet: what of its triangles no frame changes (`frame`).
    packets: Vec<Vec<Vec<PacketTriangles>>>,
    /// Per class and LOD: its packets' triangles, the second draws' with them (`split` weighs a range's work by them).
    weights: Vec<Vec<usize>>,
    /// Per class and LOD: the bytes of an instance's colour buffer its packets read (from, to).
    colour_extent: Vec<Vec<(usize, usize)>>,
    /// Per instance (and one past the last), where its blocks begin in the level's tie buffers (`Ties::new`).
    blocks: Vec<TieBlock>,
    /// The level's tie corners on the GPU (`TieCorner`): each instance's, built when it is first drawn at its LOD and again when
    /// what they are built from changes (`frame`).
    corners: wgpu::Buffer,
    /// Per frame set (`native::SETS`): the frame's values.
    sets: Vec<TieFrame>,
    /// Per instance: what its corners were built from.
    built: Vec<std::sync::Mutex<TieBuilt>>,
    /// Per class, each light position's normal as the tie light pass makes it from the position's two angles (func_L01_004B3368:
    /// (cos a cos b, sin a cos b, sin b), class space).
    normals: Vec<Vec<[f32; 3]>>,
}

/// What building a range of ties took (`Ties::frame`, `UYA_NATIVE_TIMES`): the tie programs' decisions (`tievu::decide`), the
/// instances' corners built, the writes and all of it (ns), the packets and those whose run has the back-face test or is program
/// 3's, the vertices made (those of the instances built), those of them swaying, the second draws' triangle corners, and the
/// instances built.
#[derive(Default, Clone, Copy)]
pub(crate) struct TieStats {
    pub decide_ns: u64,
    pub build_ns: u64,
    pub upload_ns: u64,
    pub all_ns: u64,
    pub packets: usize,
    pub decided: usize,
    pub made: usize,
    pub sway: usize,
    pub second: usize,
    pub rebuilt: usize,
}

impl TieStats {
    pub fn add(&mut self, o: &TieStats) {
        self.decide_ns += o.decide_ns;
        self.build_ns += o.build_ns;
        self.upload_ns += o.upload_ns;
        self.all_ns += o.all_ns;
        self.packets += o.packets;
        self.decided += o.decided;
        self.made += o.made;
        self.sway += o.sway;
        self.second += o.second;
        self.rebuilt += o.rebuilt;
    }
}

/// A tie packet's triangles as every instance of its class draws them: each one's texture setting in the packet's first draw and
/// in its second (none without second-draw coordinates), and where the ADC flag at its third vertex lies in the packet's flags
/// (`tievu::decide`: each strip's in turn).
struct PacketTriangles {
    first: Vec<Key>,
    second: Vec<Key>,
    adc: Vec<usize>,
}

/// Which of `TieGlobals::tests` a packet's first draw takes (the first when its entry flags have bit 3).
fn first_test(pk: &uyafmt::tie::TiePacket) -> usize {
    if pk.flags & 8 != 0 { 0 } else { 1 }
}

/// A tie triangle's texture setting in a packet's second draw: its texture slot names a register of the second texture list.
fn tie_second_key(c: &uyafmt::tie::TieClass, second: &uyafmt::tie::TieSecond, tri: &uyafmt::tie::TieTriangle, rec: &[u8; 16]) -> Result<Key, String> {
    let j = *second.registers.get(tri.texture).ok_or_else(|| format!("native: tie triangle texture slot {} past the second part's {} registers", tri.texture, second.registers.len()))?;
    let reg = c.textures.get(j).ok_or("native: tie second-part texture register")?;
    let texture = *rec.get(j).ok_or("native: tie texture index byte")? as u32;
    if reg.wms > 1 || reg.wmt > 1 {
        return Err(format!("native: tie CLAMP WMS {} WMT {}", reg.wms, reg.wmt));
    }
    let blend = Blend::of(reg.alpha_a, reg.alpha_b, reg.alpha_c, reg.alpha_d, reg.alpha_fix)?;
    Ok((texture, reg.wms == 1, reg.wmt == 1, reg.mmin, tex1_k(reg.k_word).to_bits(), blend, 0))
}

/// Whether a packet has a second draw with coordinates (entry flags 2 or 4).
fn has_second_uvs(pk: &uyafmt::tie::TiePacket) -> bool {
    pk.second.is_some() && pk.flags & 6 != 0
}

/// A tie packet's second-draw texture coordinates as the game's chain sends them: the data of the UNPACK V2-32 to 0xDC of the
/// record buffer (TOPS-relative, where the first draw had the colours) before the second draw, two words a vector, as the EE
/// computed them (func_L01_004B13C8 for entry flag 2, func_L01_004B17E8 for flag 4, with the tie VU0 program). A second pass
/// set up at entry 0x70 and drawn at 0x0 or 0x10 takes them as integers (ITOF12); one drawn at 0x90 or 0xA0 as floats.
pub(crate) struct SecondUv<'a> {
    words: &'a [u32],
    itof12: bool,
}

impl SecondUv<'_> {
    /// Vertex `v`'s coordinates in a packet of `n2` and `n3` vertices of its two kinds (`TieSecond::uv_counts`, in the packet's
    /// order): entry 0x70's two copy loops count down before they test, so with no vertex of the first kind the second kind's
    /// start one vector on; entry 0x90's start where the first kind's end.
    fn at(&self, v: usize, (n2, n3): (usize, usize)) -> Result<[f32; 2], String> {
        let first = if self.itof12 { n2.max(1) } else { n2 };
        let k = match v {
            v if v < n2 => v,
            v if v < n2 + n3 => first + v - n2,
            v => return Err(format!("native: tie second-part vertex {v} past its {n2} + {n3} coordinates")),
        };
        let w = self.words.get(2 * k..2 * k + 2).ok_or_else(|| format!("native: tie second-part vertex {v}: its coordinates past the {} the chain sent", self.words.len() / 2))?;
        Ok(if self.itof12 { [w[0] as i32 as f32 / 4096.0, w[1] as i32 as f32 / 4096.0] } else { [f32::from_bits(w[0]), f32::from_bits(w[1])] })
    }
}

/// Every tie second draw's coordinates in the frame's VIF1 chain (`SecondUv`), by the instance record the chain sends before the
/// draw, in the chain's order: one per packet of the instance that has a second part with coordinates, in its packets' order.
/// The chain draws a second pass after its packet's first draw (level 4, tour field 6300) or the first draws of a run of
/// instances and then their second passes, each after its record again (level 20, field 1555). Read from the chain's own data,
/// not from a model of VU1 memory.
pub(crate) fn second_coordinates(jobs: &crate::jobs::FrameJobs) -> Result<FastMap<u32, Vec<SecondUv<'_>>>, String> {
    let mut by: FastMap<u32, Vec<SecondUv<'_>>> = FastMap::default();
    let mut resident: Option<u8> = None;
    for job in &jobs.jobs {
        let crate::jobs::Job::Vif1 { words, marks, blocks, .. } = job else { continue };
        let source = |at: usize| -> Option<u32> {
            let k = blocks.partition_point(|b| b.0 <= at).checked_sub(1)?;
            Some(blocks[k].1 + 4 * (at - blocks[k].0) as u32)
        };
        let phase = |at: usize| -> usize {
            match marks.partition_point(|m| m.0 <= at).checked_sub(1) {
                Some(k) if marks[k].1 == 2 => 2 + (at - marks[k].0),
                Some(k) => (at - marks[k].0) % 4,
                None => at % 4,
            }
        };
        let mut record: Option<u32> = None;
        // The colours unpacked since the last run (a first draw takes them), the coordinates unpacked since the last run, and
        // those an entry 0x70 setup took for the draw after it.
        let mut colours = false;
        let mut sent: Option<&[u32]> = None;
        let mut setup: Option<&[u32]> = None;
        let (mut cl, mut wl, mut mode) = (1usize, 1usize, 0u32);
        let mut i = 0;
        while i < words.len() {
            let code = words[i];
            let (cmd, imm, num) = ((code >> 24) & 0x7f, code & 0xffff, ((code >> 16) & 0xff) as usize);
            i += 1;
            match cmd {
                0x00 | 0x02..=0x04 | 0x06 | 0x07 | 0x10 | 0x11 | 0x13 => {}
                0x05 => mode = imm & 3,
                0x01 => (cl, wl) = ((imm & 0xff) as usize, (imm >> 8 & 0xff) as usize),
                0x20 => i += 1,
                0x30 | 0x31 => i += 4,
                0x14 | 0x15 => {
                    let entry = imm * 8;
                    if matches!(resident, Some(2 | 3)) {
                        let drawn = match entry {
                            0x0 | 0x10 if colours => {
                                setup = None;
                                None
                            }
                            0x70 => {
                                setup = sent;
                                None
                            }
                            0x0 | 0x10 => setup.take().map(|w| (w, true)),
                            0x90 | 0xa0 => sent.map(|w| (w, false)),
                            _ => None,
                        };
                        if let Some((words, itof12)) = drawn {
                            let rec = record.ok_or("native: a tie second draw with no instance record unpacked before it")?;
                            by.entry(rec).or_default().push(SecondUv { words, itof12 });
                        }
                    }
                    colours = false;
                    sent = None;
                }
                0x17 => {
                    colours = false;
                    sent = None;
                }
                // MPG to micro address 0: the program it uploads.
                0x4a => {
                    if imm == 0 {
                        let at = source(i);
                        resident = crate::mobies::PROGRAMS.iter().find(|(_, pieces)| Some(pieces[0]) == at).map(|&(p, _)| p);
                    }
                    i += 2 * if num == 0 { 256 } else { num };
                }
                // DIRECT: its GIF data starts at the next quadword.
                0x50 | 0x51 => {
                    i += (4 - phase(i)) % 4;
                    i += 4 * if imm == 0 { 0x10000 } else { imm as usize };
                }
                0x60..=0x7f => {
                    let (vn, vl) = ((cmd >> 2) & 3, cmd & 3);
                    let n = if num == 0 { 256 } else { num };
                    let items = if wl <= cl || wl == 0 { n } else { (n / wl) * cl + (n % wl).min(cl) };
                    let bits = if vl == 3 { 16 } else { (vn as usize + 1) * (32 >> vl) };
                    let len = (items * bits).div_ceil(32);
                    match cmd & 0x6f {
                        0x6e => colours = true,
                        0x6c if num == 6 => record = Some(source(i).ok_or("native: a tie record UNPACK outside the job's blocks")?),
                        // V2-32 to 0xDC of the record buffer: the second draw's coordinates, each vector one quadword's x and y.
                        0x64 if imm & 0x3ff == 0xdc && imm & 0x8000 != 0 && matches!(resident, Some(2 | 3)) => {
                            if cmd & 0x10 != 0 || mode != 0 || cl != wl {
                                return Err(format!("native: tie second-draw coordinates unpacked masked ({}), in mode {mode} or with CL {cl} WL {wl}", cmd & 0x10 != 0));
                            }
                            sent = Some(words.get(i..i + len).ok_or("native: a tie coordinates UNPACK past its job")?);
                        }
                        _ => {}
                    }
                    i += len;
                }
                c => return Err(format!("native: VIF code {c:#x} in the tie chain")),
            }
        }
    }
    Ok(by)
}

/// A tie triangle's texture setting.
fn tie_key(c: &uyafmt::tie::TieClass, pk: &uyafmt::tie::TiePacket, tri: &uyafmt::tie::TieTriangle, rec: &[u8; 16]) -> Result<Key, String> {
    let j = *pk.textures.get(tri.texture).ok_or("native: tie triangle texture")?;
    let reg = c.textures.get(j).ok_or("native: tie texture register")?;
    let texture = *rec.get(j).ok_or("native: tie texture index byte")? as u32;
    if reg.wms > 1 || reg.wmt > 1 {
        return Err(format!("native: tie CLAMP WMS {} WMT {}", reg.wms, reg.wmt));
    }
    let blend = Blend::of(reg.alpha_a, reg.alpha_b, reg.alpha_c, reg.alpha_d, reg.alpha_fix)?;
    Ok((texture, reg.wms == 1, reg.wmt == 1, reg.mmin, tex1_k(reg.k_word).to_bits(), blend, 0))
}

impl Ties {
    #[allow(clippy::too_many_arguments)]
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout, tie_layout: &wgpu::BindGroupLayout, classes: Vec<uyafmt::tie::TieClass>, textures: &[uyafmt::core::Texture], records: &[uyafmt::core::ClassRecord], gameplay: &uyafmt::gameplay::Gameplay, tests: [u64; 2]) -> Result<Ties, String> {
        let instance_class = gameplay
            .ties
            .iter()
            .enumerate()
            .map(|(i, t)| records.iter().position(|c| c.id == t.class).ok_or_else(|| format!("native: tie instance {i}: class {} has no record", t.class)))
            .collect::<Result<Vec<_>, String>>()?;
        let records: Vec<[u8; 16]> = records.iter().map(|c| c.textures).collect();
        // Program 2's 1/Q is a 32nd of the depth.
        let base = Materials::new(device, queue, textures, -5.0, 0)?;
        let mut materials = [base.with_test(tests[0])?, base.with_test(tests[1])?];
        for (c, rec) in classes.iter().zip(&records) {
            for lod in &c.lods {
                for pk in lod {
                    for tri in &pk.triangles {
                        materials[first_test(pk)].get(device, layout, tie_key(c, pk, tri, rec)?)?;
                        if has_second_uvs(pk) {
                            materials[1].get(device, layout, tie_second_key(c, pk.second.as_ref().expect("has_second_uvs"), tri, rec)?)?;
                        }
                    }
                }
            }
        }
        let packets = classes
            .iter()
            .zip(&records)
            .map(|(c, rec)| {
                c.lods
                    .iter()
                    .map(|lod| {
                        lod.iter()
                            .map(|pk| -> Result<PacketTriangles, String> {
                                let first = pk.triangles.iter().map(|tri| tie_key(c, pk, tri, rec)).collect::<Result<_, _>>()?;
                                let second = match pk.second.as_ref().filter(|_| has_second_uvs(pk)) {
                                    Some(second) => pk.triangles.iter().map(|tri| tie_second_key(c, second, tri, rec)).collect::<Result<_, _>>()?,
                                    None => Vec::new(),
                                };
                                // Each triangle's place in its strip, and its third vertex's flag after the strips before it.
                                let starts: Vec<usize> = pk.strips.iter().scan(0, |at, st| Some(std::mem::replace(at, *at + st.vertices.len()))).collect();
                                let mut places: Vec<usize> = Vec::with_capacity(pk.triangles.len());
                                let mut adc = Vec::with_capacity(pk.triangles.len());
                                for (t, tri) in pk.triangles.iter().enumerate() {
                                    places.push(if t > 0 && pk.triangles[t - 1].strip == tri.strip { places[t - 1] + 1 } else { 0 });
                                    let st = pk.strips.get(tri.strip).ok_or_else(|| format!("native: tie triangle {t} of strip {} of {}", tri.strip, pk.strips.len()))?;
                                    if places[t] + 2 >= st.vertices.len() {
                                        return Err(format!("native: tie triangle {t}: place {} in a strip of {} vertices", places[t], st.vertices.len()));
                                    }
                                    adc.push(starts[tri.strip] + places[t] + 2);
                                }
                                Ok(PacketTriangles { first, second, adc })
                            })
                            .collect::<Result<Vec<_>, String>>()
                    })
                    .collect::<Result<Vec<_>, String>>()
            })
            .collect::<Result<Vec<_>, String>>()?;
        let weights = classes.iter().map(|c| c.lods.iter().map(|lod| lod.iter().map(|pk| pk.triangles.len() * if has_second_uvs(pk) { 2 } else { 1 }).sum()).collect()).collect();
        // Each class and LOD's colours in an instance's colour buffer: from the first a packet reads to the end of the last.
        let colour_extent: Vec<Vec<(usize, usize)>> = classes
            .iter()
            .map(|c| {
                c.lods
                    .iter()
                    .map(|lod| {
                        let words = lod.iter().flat_map(|pk| pk.vertices.iter().map(move |v| pk.colour_offset as usize + 4 * v.colour as usize));
                        let (lo, hi) = words.fold((usize::MAX, 0), |(lo, hi), w| (lo.min(w), hi.max(w + 4)));
                        if lo == usize::MAX { (0, 0) } else { (lo, hi) }
                    })
                    .collect()
            })
            .collect();
        // Each instance's blocks, in instance order, as large as its class's largest LOD needs: its corners (a whole number of 96,
        // so that each instance's triangles have whole words of the frame's drawn bits), its vertices' colour slots (padded to
        // 4, retaining the resident layout) and its second draws' coordinates.
        let mut blocks = Vec::with_capacity(instance_class.len() + 1);
        let mut at = TieBlock { corners: 0, slots: 0, seconds: 0 };
        for &ci in &instance_class {
            blocks.push(at);
            let (mut corners, mut slots, mut seconds) = (0usize, 0usize, 0usize);
            for lod in &classes[ci].lods {
                corners = corners.max(lod.iter().map(|pk| 3 * pk.triangles.len() * if has_second_uvs(pk) { 2 } else { 1 }).sum());
                slots = slots.max(lod.iter().map(|pk| pk.vertices.len()).sum());
                seconds = seconds.max(lod.iter().filter(|pk| has_second_uvs(pk)).map(|pk| pk.vertices.len()).sum());
            }
            at = TieBlock { corners: at.corners + corners.next_multiple_of(96) as u32, slots: at.slots + slots.next_multiple_of(4) as u32, seconds: at.seconds + seconds as u32 };
        }
        blocks.push(at);
        let corners_size = (at.corners as u64 * std::mem::size_of::<TieCorner>() as u64).max(32);
        if corners_size > device.limits().max_buffer_size {
            return Err(format!("native: the level's tie corners ({} of them, {corners_size} bytes) pass the GPU's largest buffer ({})", at.corners, device.limits().max_buffer_size));
        }
        let corners = device.create_buffer(&wgpu::BufferDescriptor { label: Some("tie corners"), size: corners_size, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
        let storage = |label: &str, size: u64| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size.max(16).next_multiple_of(4), usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let sets = (0..crate::native::SETS)
            .map(|_| {
                let (drawn, st, colours) = (storage("tie triangles drawn", at.corners as u64 / 96 * 4), storage("tie second-draw coordinates", at.seconds as u64 * 8), storage("tie current colours", at.slots as u64 * 4));
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("ties' frame values"),
                    layout: tie_layout,
                    entries: &[wgpu::BindGroupEntry { binding: 3, resource: drawn.as_entire_binding() }, wgpu::BindGroupEntry { binding: 4, resource: st.as_entire_binding() }, wgpu::BindGroupEntry { binding: 5, resource: colours.as_entire_binding() }],
                });
                TieFrame { drawn, st, colours, group }
            })
            .collect();
        println!("native: ties: {} instances, room for {} corners ({:.1} MB), {} vertices and {} second-draw vertices on the GPU", instance_class.len(), at.corners, corners_size as f64 / 1e6, at.slots, at.seconds);
        let built = (0..instance_class.len()).map(|_| std::sync::Mutex::new(TieBuilt::default())).collect();
        // Each class's light positions' normals, from the (cos, sin) table of 2 pi k / 256 (class space).
        let cs = |k: u16| {
            let t = std::f64::consts::TAU * (k & 0xff) as f64 / 256.0;
            (t.cos() as f32, t.sin() as f32)
        };
        let normals = classes
            .iter()
            .map(|c| {
                c.light_positions
                    .iter()
                    .map(|&(_, w)| {
                        let ((ca, sa), (cb, sb)) = (cs(w), cs(w >> 8));
                        [ca * cb, sa * cb, sb]
                    })
                    .collect()
            })
            .collect();
        Ok(Ties { classes, instance_class, materials, packets, weights, colour_extent, blocks, corners, sets, built, normals })
    }

    /// The level's tie corners (`TieCorner`, vertex buffer 0 of `vs_tie`) and frame set `set`'s values' group (group 2).
    pub fn buffers(&self, set: usize) -> (&wgpu::Buffer, &wgpu::BindGroup) {
        (&self.corners, &self.sets[set].group)
    }

    /// Every tie instance the level shows (its colour buffer set, +0x14, and its distance byte, +0x1C, not 0: `range.rs`) at its
    /// finest LOD, in world space as `frame` puts a vertex there (the matrices in `ram`, no sway), each triangle of every packet
    /// with its first draw's texture setting: the shadows' casters (`shadows.rs`), built once a level. Each instance's draws index
    /// the returned indices; its sphere holds its vertices.
    pub fn casters(&self, ram: &[u8], g: &TieGlobals) -> Result<(Vec<ObjVertex>, Vec<u32>, Vec<Caster>), String> {
        let n = rd_u32(ram, g.count as usize)? as usize;
        if n != self.instance_class.len() {
            return Err(format!("native: the game has {n} tie instances, the gameplay block {}", self.instance_class.len()));
        }
        let (arr, mats) = (rd_u32(ram, g.instances as usize)? as usize, rd_u32(ram, g.matrices as usize)? as usize);
        let (mut out, mut indices, mut casters) = (Vec::new(), Vec::new(), Vec::new());
        for i in 0..n {
            let a = arr + 32 * i;
            let shown = ram.get(a..a + 32).ok_or("native: a tie instance past RAM")?;
            if u16::from_le_bytes([shown[0x14], shown[0x15]]) == 0xffff || shown[0x1c] == 0 {
                continue;
            }
            let c = &self.classes[self.instance_class[i]];
            let Some(lod) = c.lods.first() else { continue };
            let m = mats + 64 * i;
            let row = |r: usize| -> Result<[f32; 3], String> { Ok([rd_f32(ram, m + 16 * r)?, rd_f32(ram, m + 16 * r + 4)?, rd_f32(ram, m + 16 * r + 8)?]) };
            let rows = [row(0)?, row(1)?, row(2)?, row(3)?];
            let scale = rd_f32(ram, m + 0x3c)?;
            let first_vertex = out.len();
            let mut draws: Vec<(usize, Draw)> = Vec::new();
            for (dn, pk) in lod.iter().enumerate() {
                let base = out.len() as u32;
                for v in &pk.vertices {
                    let p = v.pos.map(|x| x / 1024.0 * scale);
                    let pos = [0, 1, 2].map(|q| p[0] * rows[0][q] + p[1] * rows[1][q] + p[2] * rows[2][q] + rows[3][q]);
                    out.push(ObjVertex { pos, st: [v.uv[0], v.uv[1], 1.0], colour: [0x80; 4], f: 256.0 });
                }
                let test = first_test(pk);
                let pt = &self.packets[self.instance_class[i]][0][dn];
                for (t, tri) in pk.triangles.iter().enumerate() {
                    let key = pt.first[t];
                    if draws.last().is_none_or(|(t, dr)| dr.key != key || *t != test) {
                        draws.push((test, Draw { key, first: indices.len() as u32, count: 0 }));
                    }
                    indices.extend(tri.v.iter().map(|&vi| base + vi));
                    let dr = &mut draws.last_mut().expect("pushed above").1;
                    dr.count = indices.len() as u32 - dr.first;
                }
            }
            let verts = &out[first_vertex..];
            if verts.is_empty() {
                continue;
            }
            let (lo, hi) = verts.iter().fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), v| ([0, 1, 2].map(|k| lo[k].min(v.pos[k])), [0, 1, 2].map(|k| hi[k].max(v.pos[k]))));
            let centre = [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2.0);
            let radius = verts.iter().map(|v| (0..3).map(|k| (v.pos[k] - centre[k]).powi(2)).sum::<f32>().sqrt()).fold(0.0, f32::max);
            casters.push(Caster { centre, radius, draws });
        }
        Ok((out, indices, casters))
    }

    /// Each class's LODs in its own units, each triangle under its packet's first draw's test and texture setting (`RtClass`), as
    /// `frame` draws an instance's first draw (the rest positions: no sway): the ray-traced shadows' meshes.
    pub fn rt_classes(&self) -> Result<Vec<Vec<RtClass>>, String> {
        let mut out = Vec::with_capacity(self.classes.len());
        for (ci, c) in self.classes.iter().enumerate() {
            let mut lods = Vec::with_capacity(c.lods.len());
            for (l, lod) in c.lods.iter().enumerate() {
                let mut m = RtClass { pos: Vec::new(), uv: Vec::new(), colour: Vec::new(), normal: Vec::new(), runs: Vec::new() };
                let lo = self.colour_extent[ci][l].0;
                for (dn, pk) in lod.iter().enumerate() {
                    let base = m.pos.len() as u32;
                    // A second part of the eye's reflection (entry flag 2) carries the game's normal of each vertex, in vertex order.
                    let normals = pk.second.as_ref().filter(|s| pk.flags & 2 != 0 && s.normals.len() >= pk.vertices.len()).map(|s| &s.normals);
                    for (k, v) in pk.vertices.iter().enumerate() {
                        m.pos.push(v.pos.map(|x| x / 1024.0));
                        m.uv.push(v.uv);
                        m.colour.push(((pk.colour_offset as usize + 4 * v.colour as usize - lo) / 4) as u32);
                        m.normal.push(normals.map_or(0, |n| pack_normal(n[k])));
                    }
                    let test = first_test(pk);
                    let shine = has_second_uvs(pk);
                    let pt = &self.packets[ci][l][dn];
                    for (t, tri) in pk.triangles.iter().enumerate() {
                        let key = pt.first[t];
                        if m.runs.last().is_none_or(|r| r.0 != test || r.1 != key || r.2 != shine) {
                            m.runs.push((test, key, shine, Vec::new()));
                        }
                        m.runs.last_mut().expect("pushed above").3.extend(tri.v.iter().map(|&vi| base + vi));
                    }
                }
                lods.push(m);
            }
            out.push(lods);
        }
        Ok(out)
    }

    /// Every instance the level shows, as `casters` takes them (its colour buffer set, +0x14, and its distance byte, +0x1C, not
    /// 0): its class, the LOD the game draws it at (+0x1F, as `split` reads it), its transform into the world, and where in RAM
    /// the colours its LOD reads start and how many words they run (`frame`'s colour buffer from the first word its LOD reads
    /// to the last: the words `RtClass::colour` counts from).
    /// What `rt_instances` reads: the instance array and the matrix array as RAM holds them (`raytrace::LevelRt::frame` reads the
    /// instances again only when these change).
    pub fn rt_state<'r>(&self, ram: &'r [u8], g: &TieGlobals) -> Result<[&'r [u8]; 2], String> {
        let n = self.instance_class.len();
        let (arr, mats) = (rd_u32(ram, g.instances as usize)? as usize, rd_u32(ram, g.matrices as usize)? as usize);
        Ok([ram.get(arr..arr + 32 * n).ok_or("native: the tie instances lie past RAM")?, ram.get(mats..mats + 64 * n).ok_or("native: the tie matrices lie past RAM")?])
    }

    pub fn rt_instances(&self, ram: &[u8], g: &TieGlobals) -> Result<Vec<(usize, u8, [f32; 12], u32, u32)>, String> {
        let n = rd_u32(ram, g.count as usize)? as usize;
        if n != self.instance_class.len() {
            return Err(format!("native: the game has {n} tie instances, the gameplay block {}", self.instance_class.len()));
        }
        let (arr, mats, cols) = (rd_u32(ram, g.instances as usize)? as usize, rd_u32(ram, g.matrices as usize)? as usize, rd_u32(ram, g.colours as usize)? as usize);
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let a = arr + 32 * i;
            let shown = ram.get(a..a + 32).ok_or("native: a tie instance past RAM")?;
            if u16::from_le_bytes([shown[0x14], shown[0x15]]) == 0xffff || shown[0x1c] == 0 {
                continue;
            }
            let lod = shown[0x1f];
            if lod as usize >= self.classes[self.instance_class[i]].lods.len() {
                return Err(format!("native: tie {i} drawn at LOD {lod} of its class's {}", self.classes[self.instance_class[i]].lods.len()));
            }
            let (lo, hi) = self.colour_extent[self.instance_class[i]][lod as usize];
            let at = cols + 16 * u16::from_le_bytes([shown[0x14], shown[0x15]]) as usize + lo;
            out.push((self.instance_class[i], lod, rt_transform(ram, mats + 64 * i)?, at as u32, ((hi - lo) / 4) as u32));
        }
        Ok(out)
    }

    /// The instances in up to `parts` ranges, in order, together all of them, each holding about as many triangles of the ties
    /// the game's list names this frame, at the LOD it draws each at (`frame` builds each; ranges of as many ties left one thread
    /// most of level 28's arrival).
    /// One range a `per` triangles, `parts` at most.
    pub fn split(&self, ram: &[u8], g: &TieGlobals, per: usize, parts: usize) -> Result<Vec<std::ops::Range<usize>>, String> {
        let n = self.instance_class.len();
        let arr = rd_u32(ram, g.instances as usize)? as usize;
        let list_n = rd_u32(ram, g.list_bytes as usize)? as usize / 4;
        let mut listed: Vec<(usize, usize)> = Vec::with_capacity(list_n);
        for k in 0..list_n {
            let i = (rd_u32(ram, g.list as usize + 4 * k)? & 0xfff) as usize;
            if i >= n {
                continue;
            }
            let at = arr + 32 * i;
            let instance = ram.get(at..at + 32).ok_or("native: a tie instance past RAM")?;
            // The cull can list a tie before it has colours or a LOD. The DMA builder
            // skips that entry (0x4B0BE4 on Veldin), as does `frame`.
            if u16::from_le_bytes([instance[0x14], instance[0x15]]) == 0xffff {
                continue;
            }
            let lod = instance[0x1f] as usize;
            let w = *self.weights[self.instance_class[i]].get(lod).ok_or_else(|| format!("native: tie {i} drawn at LOD {lod} of its class's {}", self.weights[self.instance_class[i]].len()))?;
            listed.push((i, w));
        }
        let total: usize = listed.iter().map(|l| l.1).sum();
        let parts = total.div_ceil(per).clamp(1, parts);
        if parts == 1 {
            return Ok(vec![0..n]);
        }
        listed.sort_unstable();
        let mut ranges = Vec::with_capacity(parts);
        let (mut from, mut acc, mut part) = (0, 0, 1);
        for &(i, w) in &listed {
            if part < parts && acc * parts >= total * part && i > from {
                ranges.push(from..i);
                from = i;
                part += 1;
            }
            acc += w;
        }
        ranges.push(from..n);
        Ok(ranges)
    }

    /// The frame's tie triangles, as runs of the level's corners on the GPU appended to `draws` (each with the index of its
    /// TEST_1, `TieGlobals::tests`), for the instances of `range` (`split`), with this frame's values written into frame set
    /// `set`'s buffers. Returns the ties drawn, those the game lists and its chain sends no run for, and those whose runs do not
    /// match their LOD's packets (`tievu`).
    ///
    /// A packet's first draw takes the triangles the game's program draws (`tievu::decide`: no ADC at the triangle's third
    /// vertex, for the record the chain sent with the run); its second draw (MSCAL 0x12, no test) all of them. A triangle
    /// program 3 sends clipped to its guard band is drawn whole (the GPU clips it).
    ///
    /// What no frame of an instance changes while its matrix, morph factor, light inputs and LOD stay as they are is its
    /// corners: each corner of each of its LOD's triangles, its first draw's and its second draw's, with the vertex as the
    /// instance draws it (where, its texture coordinates, its colour), built into the instance's block of the level's corners the
    /// first time the instance is drawn so and again whenever one of those changes. Each frame then writes only what the camera
    /// and the chain change: each vertex's colour, whether each first-draw triangle is drawn (one bit: `vs_tie`
    /// puts the corners of a triangle the program does not draw at one point outside the picture), and the second draws'
    /// coordinates; level 28's arrival wrote 6.4 MB of vertices and 3.7 MB of indices a frame before. The pixels are the same:
    /// each corner's values are those the vertex had, worked out by the same arithmetic, the triangles are drawn in the same
    /// order and those not drawn draw nothing.
    #[allow(clippy::too_many_arguments)]
    ///
    /// An instance laid out anew (drawn at another LOD than its corners were built for) sets `stale`, the frame before's replay's
    /// flag (`replay::Replay`), before its corners are written: that frame is not drawn again from them.
    ///
    /// The corners laid out anew go to the level's corner buffer, which every frame drawing the ties reads: written now, or with
    /// `corners_out` collected (byte offset, bytes) for whoever submits the frames in order (`native::Outbox`), so that a frame
    /// still to be drawn keeps the corners it was built with.
    pub fn frame(&self, queue: &wgpu::Queue, ram: &[u8], g: &TieGlobals, cam: &Camera, tie_draws: &crate::tievu::TieDraws, coords: &FastMap<u32, Vec<SecondUv>>, range: std::ops::Range<usize>, set: usize, stale: Option<&std::sync::Mutex<bool>>, corners_out: Option<&std::sync::Mutex<Vec<(u64, Vec<u8>)>>>, room: &mut TieRoom, stats: &mut TieStats) -> Result<(usize, usize, usize), String> {
        let t_all = std::time::Instant::now();
        let pick = crate::surfaces::Pick::new(cam);
        let n = rd_u32(ram, g.count as usize)? as usize;
        if n != self.instance_class.len() {
            return Err(format!("native: the game has {n} tie instances, the gameplay block {}", self.instance_class.len()));
        }
        let (arr, mats, cols) = (rd_u32(ram, g.instances as usize)? as usize, rd_u32(ram, g.matrices as usize)? as usize, rd_u32(ram, g.colours as usize)? as usize);
        // The game's list of the frame's ties: each drawn instance's record (quadword index into the record table).
        let (list_n, table) = (rd_u32(ram, g.list_bytes as usize)? as usize / 4, rd_u32(ram, g.records as usize)? as usize);
        // By instance (the word's 12 bits): its record.
        // The table is kept from frame to frame: only the entries the last list set are cleared.
        let mut record_of_table = std::mem::take(&mut room.record_of);
        record_of_table.resize(0x1000, None);
        for &i in &room.listed {
            record_of_table[i] = None;
        }
        room.listed.clear();
        for k in 0..list_n {
            let w = rd_u32(ram, g.list as usize + 4 * k)?;
            let i = (w & 0xfff) as usize;
            if record_of_table[i].is_none() {
                room.listed.push(i);
            }
            record_of_table[i] = Some(table + 16 * ((w >> 12) & 0x1fff) as usize);
        }
        let record_of = |i: usize| record_of_table.get(i).copied().flatten();
        // The light sets the per-pixel lighting's terms are taken with (`lighting.rs`).
        let light_sets = &crate::lighting::sets_from_ram(ram, g.light_sets)?;
        // Whether instance i is drawn: the game's list decides (its cull: the zones, the class's flags, the distance, the picture),
        // at the LOD the game drew it at (+0x1F, the LOD its colours are lit for) with the morph factor of the record it sent
        // (quadword 4's x); else None, or which count it goes to (unsent: listed, no run in the chain; unmatched: not one run per
        // packet of that LOD). The LOD distances and the instances' own distances the cull reads are the port's (`range.rs`).
        enum Instance<'c> {
            Drawn(&'c uyafmt::tie::TieClass, usize, f32, &'c [crate::tievu::Call]),
            Unsent,
            Unmatched,
        }
        let instance = |i: usize| -> Result<Option<Instance>, String> {
            let Some(at) = record_of(i) else { return Ok(None) };
            let a = arr + 32 * i;
            // The chain's builder skips an instance with no colour buffer (func_L01_004B0990, 0x4B0BE4).
            if u16::from_le_bytes([ram[a + 0x14], ram[a + 0x15]]) == 0xffff {
                return Ok(None);
            }
            let c = &self.classes[self.instance_class[i]];
            let Some(runs) = tie_draws.by_record.get(&(at as u32)) else { return Ok(Some(Instance::Unsent)) };
            let lod = ram[a + 0x1f] as usize;
            let packets = c.lods.get(lod).ok_or_else(|| format!("native: tie {i} drawn at LOD {lod} of its class's {}", c.lods.len()))?;
            if runs.len() != packets.len() {
                return Ok(Some(Instance::Unmatched));
            }
            Ok(Some(Instance::Drawn(c, lod, f32::from_bits(runs[0].record[4][0]), runs)))
        };
        let (mut drawn, mut unsent, mut unmatched) = (0, 0, 0);
        // The range's share of the frame's values: colour words, drawn-bit words and second-draw coordinates.
        let (b0, b1) = (self.blocks[range.start], self.blocks[range.end]);
        room.colours.resize((b1.slots - b0.slots) as usize, 0);
        room.colour_spans.clear();
        room.bits.clear();
        room.bits.resize(((b1.corners - b0.corners) / 96) as usize, 0);
        room.st.clear();
        room.st.resize((b1.seconds - b0.seconds) as usize, [0.0; 2]);
        room.up.clear();
        room.spans.clear();
        room.rebuilt.clear();
        // Each packet's ADC flags (`tievu::decide`) and the room it works in.
        let (mut adc, mut decide_room) = (std::mem::take(&mut room.adc), std::mem::take(&mut room.decide));
        let mut draws = std::mem::take(&mut room.draws);
        draws.draws.clear();
        draws.ranges.clear();
        for i in range.clone() {
            let (c, lod, k, runs) = match instance(i)? {
                None => continue,
                Some(Instance::Unsent) => {
                    unsent += 1;
                    continue;
                }
                Some(Instance::Unmatched) => {
                    unmatched += 1;
                    continue;
                }
                Some(Instance::Drawn(c, lod, k, runs)) => (c, lod, k, runs),
            };
            drawn += 1;
            let ci_class = self.instance_class[i];
            let blk = self.blocks[i];
            let a = arr + 32 * i;
            let ci = u16::from_le_bytes([ram[a + 0x14], ram[a + 0x15]]);
            let m = mats + 64 * i;
            let buffer = cols + 16 * ci as usize;
            let (lo, hi) = self.colour_extent[ci_class][lod];
            // Geometry and lighting terms depend on the transform, morph, light choice and light rows, not the current
            // colour bytes. Weapon flashes and pickups update those through the per-frame GPU colour table below.
            let rows_bytes = ram.get(m..m + 64).ok_or("native: a tie matrix past RAM")?;
            let colours = ram.get(buffer + lo..buffer + hi).ok_or("native: tie colours past RAM")?;
            let word = u16::from_le_bytes([ram[a + 0x18], ram[a + 0x19]]) as u32 | crate::lighting::TIE | crate::lighting::LIT;
            let lights = crate::lighting::lights(light_sets, word);
            let mut built = self.built[i].lock().map_err(|_| "native: a tie instance's lock is poisoned")?;
            if built.lod != Some(lod) || built.colours != colours {
                built.colours.clear();
                built.colours.extend_from_slice(colours);
                built.colour_version += 1;
            }
            let colour_changed = built.colour_sets[set] != built.colour_version;
            let same = built.lod == Some(lod) && built.from.len() == 68 && built.from[..64] == *rows_bytes && built.from[64..68] == k.to_bits().to_le_bytes() && built.lights == Some((word, lights));
            if !same {
                let t_build = std::time::Instant::now();
                let row = |r: usize| -> Result<[f32; 3], String> { Ok([rd_f32(ram, m + 16 * r)?, rd_f32(ram, m + 16 * r + 4)?, rd_f32(ram, m + 16 * r + 8)?]) };
                let rows = [row(0)?, row(1)?, row(2)?, row(3)?];
                let scale = rd_f32(ram, m + 0x3c)?;
                // The per-pixel lighting's: the instance's light word (+0x18: sets A and B, B's weight) and the light pass's normal of
                // a light position in the world: the class normal through the matrix's rows (each to unit length), negated (the
                // pass dots the light directions turned into the class's space and negated with it: d = -dir . n).
                let unit = |r: [f32; 3]| {
                    let l = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();
                    [r[0] / l, r[1] / l, r[2] / l]
                };
                let axes = [unit(rows[0]), unit(rows[1]), unit(rows[2])];
                let world_normal = |n: [f32; 3]| [0, 1, 2].map(|q| -(n[0] * axes[0][q] + n[1] * axes[1][q] + n[2] * axes[2][q]));
                // Each light position's normal in the world and its light terms there.
                let lit_at: Vec<([f32; 3], [f32; 2])> = self.normals[ci_class]
                    .iter()
                    .map(|&n| {
                        let w = world_normal(n);
                        (w, crate::lighting::terms_of(&lights, word, w))
                    })
                    .collect();
                let b = &mut *built;
                if b.lod.is_some_and(|l| l != lod)
                    && let Some(s) = stale
                {
                    *s.lock().map_err(|_| "native: a replay's flag is poisoned")? = true;
                }
                b.lod = Some(lod);
                b.from.clear();
                b.from.extend_from_slice(rows_bytes);
                b.from.extend_from_slice(&k.to_bits().to_le_bytes());
                b.lights = Some((word, lights));
                b.pos.clear();
                b.runs.clear();
                // The block's corners, written at its place (the rest of the block left zero: drawn by no run).
                let first_up = room.up.len();
                room.up.resize(first_up + (self.blocks[i + 1].corners - blk.corners) as usize, bytemuck::Zeroable::zeroed());
                let (mut vbase, mut cbase, mut sbase) = (0u32, 0u32, 0u32);
                for (dn, pk) in c.lods[lod].iter().enumerate() {
                    let pt = &self.packets[ci_class][lod][dn];
                    let test = first_test(pk);
                    // Each vertex as this instance draws it (a vertex is (position, plus k x its morph offset) / 1024 x the class's
                    // scale through the instance's matrix rows; its colour word of the packet's colours).
                    let mut verts: Vec<([f32; 3], [u8; 4])> = Vec::with_capacity(pk.vertices.len());
                    // Each vertex's colour word's sources (`uyafmt::tie::TieClass::light_slots`): its normal the lit sources' normals
                    // as the pass blends their colours, its light terms theirs blended so, and the share of its colour they make; source
                    // 511 is the word the pass writes the class's constant colour into (scratchpad 0x27FC: +0x4C, 0x4B3628 and
                    // 0x4B3E80), unlit. No lit source: not lit per pixel.
                    let lit: Vec<([i8; 4], [f32; 2], u32)> = pk
                        .vertices
                        .iter()
                        .map(|v| -> Result<([i8; 4], [f32; 2], u32), String> {
                            let slot = pk.colour_offset as usize / 4 + v.colour as usize;
                            let (mut n, mut ls, mut share) = ([0f32; 3], [0f32; 2], 0f32);
                            for &(k, w) in c.light_slots[lod].get(slot).map_or(&[][..], |s| s.as_slice()) {
                                if k == CONSTANT_SOURCE {
                                    continue;
                                }
                                let (nk, sk) = *lit_at.get(k as usize).ok_or_else(|| format!("native: tie {i}: a colour word lit from light position {k} of the class's {}", lit_at.len()))?;
                                let w = w as f32 / 4.0;
                                n = [0, 1, 2].map(|q| n[q] + w * nk[q]);
                                ls = [ls[0] + w * sk[0], ls[1] + w * sk[1]];
                                share += w;
                            }
                            Ok(if share == 0.0 || n == [0.0; 3] { ([0; 4], [0.0; 2], 0) } else { (crate::lighting::snorm(n, share), ls, word) })
                        })
                        .collect::<Result<_, _>>()?;
                    for v in &pk.vertices {
                        let mut p = v.pos;
                        if let Some(s) = v.sway {
                            p = [p[0] + s[0] * k, p[1] + s[1] * k, p[2] + s[2] * k];
                        }
                        let p = p.map(|x| x / 1024.0 * scale);
                        let pos = [0, 1, 2].map(|q| p[0] * rows[0][q] + p[1] * rows[1][q] + p[2] * rows[2][q] + rows[3][q]);
                        let at = buffer + pk.colour_offset as usize + 4 * v.colour as usize;
                        let w = ram.get(at..at + 4).ok_or("native: tie colour past RAM")?;
                        verts.push((pos, [w[0], w[1], w[2], w[3]]));
                        stats.made += 1;
                        stats.sway += v.sway.is_some() as usize;
                    }
                    let run = |b: &mut TieBuilt, test: usize, key: Key, corner: u32, second: bool| match b.runs.last_mut() {
                        Some((t, kk, first, count, sec)) if *t == test && *kk == key && *first + *count == corner => {
                            *count += 3;
                            *sec |= second;
                        }
                        _ => b.runs.push((test, key, corner, 3, second)),
                    };
                    for (t, tri) in pk.triangles.iter().enumerate() {
                        let corner = blk.corners + cbase + 3 * t as u32;
                        for (q, &vi) in tri.v.iter().enumerate() {
                            let (pos, colour) = *verts.get(vi as usize).ok_or("native: tie triangle vertex")?;
                            let uv = pk.vertices[vi as usize].uv;
                            let (n, s, light) = lit[vi as usize];
                            let light = light | ((has_second_uvs(pk) as u32) << 30);
                            room.up[first_up + (cbase + 3 * t as u32) as usize + q] = TieCorner { pos, st: uv.map(f32::to_bits), colour, slot: blk.slots + vbase + vi, n, s, light };
                        }
                        run(b, test, pt.first[t], corner, false);
                    }
                    cbase += 3 * pk.triangles.len() as u32;
                    if has_second_uvs(pk) {
                        let colour = pk.second.as_ref().expect("has_second_uvs").colour.ok_or("native: tie second part without a colour")?;
                        for (t, tri) in pk.triangles.iter().enumerate() {
                            let corner = blk.corners + cbase + 3 * t as u32;
                            for (q, &vi) in tri.v.iter().enumerate() {
                                let pos = verts.get(vi as usize).ok_or("native: tie triangle vertex")?.0;
                                room.up[first_up + (cbase + 3 * t as u32) as usize + q] = TieCorner { pos, st: [blk.seconds + sbase + vi, 0], colour, slot: (blk.slots + vbase + vi) | 1 << 31, n: [0; 4], s: [0.0; 2], light: 0 };
                            }
                            run(b, 1, pt.second[t], corner, true);
                        }
                        cbase += 3 * pk.triangles.len() as u32;
                        sbase += pk.vertices.len() as u32;
                    }
                    b.pos.extend(verts.iter().map(|v| v.0));
                    vbase += pk.vertices.len() as u32;
                }
                room.rebuilt.push(i);
                stats.build_ns += t_build.elapsed().as_nanos() as u64;
            }
            // This frame's values: each first-draw triangle the program draws, changed colour words, the second draws'
            // coordinates as the chain sent them (one per packet with a second part, in order).
            let sent_uvs = record_of(i).and_then(|at| coords.get(&(at as u32))).map_or(&[][..], |v| v.as_slice());
            let mut nth = 0;
            let (mut vbase, mut cbase, mut sbase) = (0u32, 0u32, 0u32);
            for (dn, (pk, call)) in c.lods[lod].iter().zip(runs).enumerate() {
                let t_decide = std::time::Instant::now();
                crate::tievu::decide(pk, call, &mut adc, &mut decide_room);
                stats.decide_ns += t_decide.elapsed().as_nanos() as u64;
                stats.packets += 1;
                stats.decided += (call.program == 3 || (call.program == 2 && call.test)) as usize;
                let pt = &self.packets[ci_class][lod][dn];
                for t in 0..pk.triangles.len() {
                    if let Some(pick) = &pick {
                        let tri = &pk.triangles[t];
                        if let Some(distance) = pick.hit(tri.v.map(|v| built.pos[(vbase + v) as usize])) {
                            let key = pt.first[t];
                            let name = self.materials[first_test(pk)].names[key.0 as usize].map(|n| n.stem());
                            println!("pick: tie {i} class {ci_class} lod {lod} packet {dn} distance {distance:.4} morph {k} drawn {} texture {name:?} key {key:?} second {:?}", !adc[pt.adc[t]], pk.second.as_ref().map(|s| (&s.registers, s.colour)));
                        }
                    }
                    // ADC at the triangle's third vertex: the game's program draws no triangle there.
                    if !adc[pt.adc[t]] {
                        let tri = (blk.corners + cbase) / 3 + t as u32 - b0.corners / 3;
                        room.bits[(tri / 32) as usize] |= 1 << (tri % 32);
                    }
                }
                if colour_changed {
                    for vi in 0..pk.vertices.len() as u32 {
                        let slot = blk.slots + vbase + vi - b0.slots;
                        let at = pk.colour_offset as usize + 4 * pk.vertices[vi as usize].colour as usize - lo;
                        room.colours[slot as usize] = u32::from_le_bytes(colours[at..at + 4].try_into().expect("validated colour extent"));
                    }
                }
                cbase += 3 * pk.triangles.len() as u32;
                if has_second_uvs(pk) {
                    let second = pk.second.as_ref().expect("has_second_uvs");
                    let at = record_of(i).ok_or_else(|| format!("native: tie {i} is drawn and not in the game's list of ties drawn"))?;
                    let sent = sent_uvs.get(nth).ok_or_else(|| format!("native: tie {i} (record {at:#x}) run {dn}: its second part {nth} with coordinates, the chain sent {}", sent_uvs.len()))?;
                    nth += 1;
                    let counts = second.uv_counts.ok_or("native: a tie second part with texture coordinates and no counts of them")?;
                    let counts = (counts.0 as usize, counts.1 as usize);
                    for v in 0..pk.vertices.len() {
                        room.st[(blk.seconds + sbase - b0.seconds) as usize + v] = sent.at(v, counts)?;
                    }
                    stats.second += 3 * pk.triangles.len();
                    cbase += 3 * pk.triangles.len() as u32;
                    sbase += pk.vertices.len() as u32;
                }
                vbase += pk.vertices.len() as u32;
            }
            if nth != sent_uvs.len() {
                return Err(format!("native: tie {i}: {nth} packets with second-part coordinates, the chain sent {} for its record", sent_uvs.len()));
            }
            // The draws as the triangles made each frame gave them: each run of one setting's triangles the frame draws, its corners'
            // ranges (a run whose triangles the program all left out draws nothing and parts no draw).
            for &(test, key, first, count, second) in &built.runs {
                let t0 = (first - b0.corners) / 3;
                let kept = second || (t0..t0 + count / 3).any(|t| room.bits[(t / 32) as usize] >> (t % 32) & 1 != 0);
                if !kept {
                    continue;
                }
                match draws.draws.last_mut() {
                    Some(d) if d.test == test && d.key == key => match draws.ranges.last_mut() {
                        Some(r) if r.0 + r.1 == first => r.1 += count,
                        _ => {
                            draws.ranges.push((first, count));
                            d.ranges += 1;
                        }
                    },
                    _ => {
                        draws.ranges.push((first, count));
                        draws.draws.push(TieDraw { test, key, first: draws.ranges.len() as u32 - 1, ranges: 1 });
                    }
                }
            }
            room.spans.push(i);
            if colour_changed {
                room.colour_spans.push(i);
                built.colour_sets[set] = built.colour_version;
            }
        }
        // The rebuilt instances' corners, each run of consecutive ones in one write; this frame's values of the drawn instances,
        // each run of them (with the undrawn between them when few) in one write.
        let t_up = std::time::Instant::now();
        let corner_bytes = std::mem::size_of::<TieCorner>() as u64;
        let mut k = 0;
        let mut up_at = 0;
        while k < room.rebuilt.len() {
            let start = room.rebuilt[k];
            let mut end = start + 1;
            while k + 1 < room.rebuilt.len() && room.rebuilt[k + 1] == end {
                k += 1;
                end += 1;
            }
            let len = (self.blocks[end].corners - self.blocks[start].corners) as usize;
            let (offset, bytes) = (self.blocks[start].corners as u64 * corner_bytes, bytemuck::cast_slice(&room.up[up_at..up_at + len]));
            match corners_out {
                Some(out) => out.lock().map_err(|_| "native: the tie corner writes' lock is poisoned")?.push((offset, bytes.to_vec())),
                None => queue.write_buffer(&self.corners, offset, bytes),
            }
            up_at += len;
            k += 1;
        }
        // A room grown by a level's first frames (hundreds of instances built at once) is not kept for the frames after, which
        // build a few at most.
        if room.up.capacity() > ROOM_KEPT {
            room.up = Vec::new();
        }
        let fs = &self.sets[set];
        let mut k = 0;
        while k < room.spans.len() {
            let start = room.spans[k];
            let mut end = start + 1;
            // An undrawn instance between two drawn ones costs its words in the write; past 32 of them a write of its own.
            while k + 1 < room.spans.len() && room.spans[k + 1] - end <= 32 {
                k += 1;
                end = room.spans[k] + 1;
            }
            let (s, e) = (self.blocks[start], self.blocks[end]);
            let bits = |corners: u32| ((corners - b0.corners) / 96) as usize;
            queue.write_buffer(&fs.drawn, s.corners as u64 / 96 * 4, bytemuck::cast_slice(&room.bits[bits(s.corners)..bits(e.corners)]));
            if e.seconds > s.seconds {
                queue.write_buffer(&fs.st, s.seconds as u64 * 8, bytemuck::cast_slice(&room.st[(s.seconds - b0.seconds) as usize..(e.seconds - b0.seconds) as usize]));
            }
            k += 1;
        }
        // Colour-only changes update the frame set's table without rebuilding or re-uploading corners. Do not bridge
        // unchanged instances here: their table entries already belong to this set and must retain their current values.
        let mut k = 0;
        while k < room.colour_spans.len() {
            let start = room.colour_spans[k];
            let mut end = start + 1;
            while k + 1 < room.colour_spans.len() && room.colour_spans[k + 1] == end {
                k += 1;
                end += 1;
            }
            let (s, e) = (self.blocks[start], self.blocks[end]);
            queue.write_buffer(&fs.colours, s.slots as u64 * 4, bytemuck::cast_slice(&room.colours[(s.slots - b0.slots) as usize..(e.slots - b0.slots) as usize]));
            k += 1;
        }
        stats.upload_ns += t_up.elapsed().as_nanos() as u64;
        stats.rebuilt += room.rebuilt.len();
        (room.adc, room.decide, room.draws, room.record_of) = (adc, decide_room, draws, record_of_table);
        stats.all_ns += t_all.elapsed().as_nanos() as u64;
        Ok((drawn, unsent, unmatched))
    }
}

/// Where an instance's blocks begin in the level's tie buffers (`Ties::new`): its corners, its vertices' colour slots, its second
/// draws' coordinates.
#[derive(Clone, Copy, Default)]
struct TieBlock {
    corners: u32,
    slots: u32,
    seconds: u32,
}

/// A frame set's tie values (`Ties::frame`): each triangle's drawn bit (32 a word), current colour words, second draws'
/// coordinates, and their group (`Pipelines::tie_layout`).
struct TieFrame {
    colours: wgpu::Buffer,
    drawn: wgpu::Buffer,
    st: wgpu::Buffer,
    group: wgpu::BindGroup,
}

/// What an instance's corners were built from (its LOD, matrix, morph factor and lighting inputs), its LOD's first-draw
/// vertices' positions (the optional material probe), and its runs of corners by TEST_1 and texture setting (test, key, first, count).
/// The runs: (test, key, first corner, corners, whether a second draw's triangle is among them).
#[derive(Default)]
struct TieBuilt {
    lod: Option<usize>,
    from: Vec<u8>,
    lights: Option<(u32, [([f32; 4], [f32; 3]); 2])>,
    colours: Vec<u8>,
    colour_version: u64,
    colour_sets: [u64; crate::native::SETS],
    pos: Vec<[f32; 3]>,
    runs: Vec<(usize, Key, u32, u32, bool)>,
}

/// The most corners a range keeps room for between frames (`Ties::frame`): 1 MB of them.
const ROOM_KEPT: usize = (1 << 20) / std::mem::size_of::<TieCorner>();

/// One tie draw of the frame (`Ties::frame`, `Pipelines::draw_ties`): one TEST_1 and texture setting's triangles in a row, as the
/// frame's triangles made each frame gave them (each draw's passing pixels, then its failing ones), as `ranges` ranges of the
/// level's corners from `first` of `TieDraws::ranges`.
pub(crate) struct TieDraw {
    pub test: usize,
    pub key: Key,
    pub first: u32,
    pub ranges: u32,
}

/// The frame's tie draws and their ranges of corners (first, count).
#[derive(Default)]
pub(crate) struct TieDraws {
    pub draws: Vec<TieDraw>,
    pub ranges: Vec<(u32, u32)>,
}

/// One corner of a tie triangle as the level keeps it on the GPU (`Ties::frame`, vertex buffer 0 of `vs_tie`): the vertex's
/// position, texture coordinates (S and T as their bits; Q is 1) and colour as the instance draws it; its vertex's slot of the
/// frame's colour values (bit 31: a second draw's corner, which every frame draws, its coordinates the frame's at the slot in
/// `st[0]`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct TieCorner {
    pub pos: [f32; 3],
    pub st: [u32; 2],
    pub colour: [u8; 4],
    pub slot: u32,
    /// The per-pixel lighting's (`lighting.rs`, `objects` `lit`): the light pass's normal at the corner's vertex in the world (the
    /// one it dots with the light directions), its light terms there and the instance's light word (0 for a second draw).
    pub n: [i8; 4],
    pub s: [f32; 2],
    pub light: u32,
}

/// A range's room for its frame (`Ties::frame`), kept from frame to frame: its values before they are written, the corners of
/// the instances rebuilt, the instances drawn and rebuilt.
#[derive(Default)]
pub(crate) struct TieRoom {
    colours: Vec<u32>,
    colour_spans: Vec<usize>,
    bits: Vec<u32>,
    st: Vec<[f32; 2]>,
    up: Vec<TieCorner>,
    spans: Vec<usize>,
    rebuilt: Vec<usize>,
    /// The game's list by instance (`Ties::frame`), the instances it set, `tievu::decide`'s flags and room, and the frame's draws.
    record_of: Vec<Option<usize>>,
    listed: Vec<usize>,
    adc: Vec<bool>,
    decide: crate::tievu::DecideRoom,
    pub(crate) draws: TieDraws,
}
