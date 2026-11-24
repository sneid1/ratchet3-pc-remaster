//! Per-pixel lighting, the `lighting` option (PLAN 8.3): the terrain, the ties and the mobies lit at every pixel by the game's own
//! light passes instead of at their vertices. The game's three passes are one formula: a vertex's colour is its base colour plus,
//! for the two directional lights of its light choice (a light set of the level's 16, or the blend of two: sets A and B and B's
//! weight in 256ths, the directions renormalised), each light's colour times its term s_k, d_k the vertex's normal dotted with
//! the light's direction:
//! - the terrain, once at the level's load (func_L01_004AEC50, `uyafmt::terrain`): s = max(d, d w), w the light's wrap, red, green
//!   and blue held at 255;
//! - the ties, per instance when it is lit (func_L01_004B3368 and its VU0 program): the same at each of the class's light positions
//!   (`uyafmt::tie::TieClass::light_positions`: a place and two angles of the (cos, sin) table, turned by the instance's rows), the
//!   base the instance's own, held at 243; the patch lists then make each colour word of the instance's buffer a copy of one
//!   position's colour or a blend of two to four (`TieClass::light_slots`; checked against Veldin's RAM: 40,650 copies and 4,462
//!   blends of 198 drawn ties, every one equal);
//! - the mobies, every frame in the skin job (func_003D00F8, `uyafmt::moby` "The skin job"): the normal turned by the vertex's
//!   joint matrices, s = max(d, d min(w, 0)) (the rigid layout: max(d, 0) and a quarter of the colours), the base the instance's.
//!
//! The GS then interpolates those colours across each triangle, so a large triangle shows its three corners' light and a
//! straight blend between them: a low-polygon surface is lit in facets. With the option the terrain's vertices carry the light
//! pass's inputs instead (`LitVertex`: the normal, the base colour, the light sets; the gameplay block's sets bound: `sets`) and
//! the pixel shader applies the light pass's formula with the normal interpolated across the triangle. The ties' and the mobies'
//! vertices keep the game's colours and carry, beside them, the normal the pass lit them with in the world, its light terms there
//! and the light choice (`objects::TieCorner`, `MobyLit`, the word of `TIE`, `MOBY`, `MOBY_RIGID`); the pixel takes the
//! interpolated colour plus each light's colour times the change of its term from the interpolated vertex terms to the pixel's
//! own (`objects` `lit`, using the frame's light sets for ties and the skin job's two lights for each moby run). At a vertex every surface's
//! colour is the game's own; between them it follows the surface's curve. A tie vertex's normal and terms are its colour word's
//! sources' blended as the patch lists blend their colours; a moby's are the skin job's own, run again from each draw record's
//! joint matrices (`uyart::hw::SkinJoints`, `mobies::Mobies::skin_pass`) and used for a run only when they give the positions the
//! run uploaded. Everything after the colour (the texture, the fog, the alpha test, the blend) is the surfaces' own shaders', and
//! a picture between two frames (`replay.rs`) draws it with the frame's own inputs.

/// The light pass's inputs for one terrain vertex (vertex buffer slot 1 of the lit pipelines).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LitVertex {
    /// The position's normal (`uyafmt::terrain::TerrainChunk::normals`).
    pub normal: [f32; 3],
    /// Its base colour (0x80 = 1.0) and light sets (`uyafmt::terrain::TerrainLight`).
    pub base: [u8; 4],
    pub sets: u32,
}

/// The light sets as the light pass reads them: 16 slots of 0x40 bytes (colour 0, direction 0, colour 1, direction 1), the
/// level's sets in the first and the rest zero (the loader clears the 16 slots before it copies the level's sets in).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Sets {
    pub rows: [[f32; 4]; 64],
}

/// The light sets as the game has them now: the 16 slots of 0x40 bytes at the level's light sets (`uyafmt::levelcode::LevelGlobals::
/// light_sets`, 0x1F17C0 on Veldin), the level's sets and the runtime lights the game writes into slots 13 and 14 for ties.
pub(crate) fn sets_from_ram(ram: &[u8], at: u32) -> Result<Sets, String> {
    let b = ram.get(at as usize..at as usize + 0x400).ok_or_else(|| format!("lighting: the light sets at {at:#x} past RAM"))?;
    let mut rows = [[0.0; 4]; 64];
    for (k, r) in rows.iter_mut().enumerate() {
        for (l, v) in r.iter_mut().enumerate() {
            *v = f32::from_le_bytes(b[16 * k + 4 * l..16 * k + 4 * l + 4].try_into().expect("4 bytes"));
        }
    }
    Ok(Sets { rows })
}

/// A light word (`objects` `lit`, a tie corner's or a moby vertex's): set A (bits 0 to 3), set B (4 to 7) and B's weight in 256ths
/// (8 to 15: 0, set A alone), as a terrain light table entry's +6 and a tie instance's +0x18 name them; the light pass (bits 28
/// and 29: `TIE`, `MOBY`, `MOBY_RIGID`); `LIT` (bit 31) when the vertex is lit at all.
/// For mobies the low 28 bits index the pair of lights captured for the run.
pub(crate) const TIE: u32 = 1 << 28;
pub(crate) const MOBY: u32 = 2 << 28;
pub(crate) const MOBY_RIGID: u32 = 3 << 28;
pub(crate) const LIT: u32 = 1 << 31;

pub(crate) fn renormal(d: [f32; 3]) -> [f32; 3] {
    let l = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    if l > 0.0 { [d[0] / l.sqrt(), d[1] / l.sqrt(), d[2] / l.sqrt()] } else { [0.0; 3] }
}

/// The two lights of `word`: colour (w its wrap) and direction each, blended as the light passes blend two sets (row by row,
/// the directions renormalised).
pub(crate) fn lights(sets: &Sets, word: u32) -> [([f32; 4], [f32; 3]); 2] {
    let (a, b, w) = (4 * (word & 15) as usize, 4 * ((word >> 4) & 15) as usize, ((word >> 8) & 255) as f32 / 256.0);
    let dir = |r: [f32; 4]| [r[0], r[1], r[2]];
    let mix = |x: [f32; 4], y: [f32; 4]| [0, 1, 2, 3].map(|k| x[k] * (1.0 - w) + y[k] * w);
    [0, 1].map(|l| {
        let (c, d) = if w == 0.0 { (sets.rows[a + 2 * l], dir(sets.rows[a + 2 * l + 1])) } else { (mix(sets.rows[a + 2 * l], sets.rows[b + 2 * l]), renormal(dir(mix(sets.rows[a + 2 * l + 1], sets.rows[b + 2 * l + 1])))) };
        (c, d)
    })
}

/// The light terms the light pass of `word` (its lights `l`, `lights`) gives normal `n` (the one it dots with the light
/// directions, unit length): s_k =
/// max(d, d w) for the terrain's and the ties' (`TIE`), max(d, d min(w, 0)) for the mobies' skin job (`MOBY`), max(d, 0) for its
/// rigid meshes (`MOBY_RIGID`); d the normal dotted with light k's direction (renormalised), w its colour's w.
pub(crate) fn terms_of(l: &[([f32; 4], [f32; 3]); 2], word: u32, n: [f32; 3]) -> [f32; 2] {
    let n = renormal(n);
    [0, 1].map(|k| {
        let (c, d) = l[k];
        let d = renormal(d);
        let e = n[0] * d[0] + n[1] * d[1] + n[2] * d[2];
        match word & (3 << 28) {
            TIE => e.max(e * c[3]),
            MOBY => e.max(e * c[3].min(0.0)),
            _ => e.max(0.0),
        }
    })
}

/// A unit normal and the share `lit` (0 to 1) of the vertex's colour that its lights make, as four signed bytes (the shaders'
/// `Snorm8x4`: `objects` `lit` takes the light terms at the pixel times the share, which a vertex of none (an unlit corner of a lit
/// triangle) takes down to zero across the triangle).
pub(crate) fn snorm(n: [f32; 3], lit: f32) -> [i8; 4] {
    let n = renormal(n);
    [(n[0] * 127.0).round() as i8, (n[1] * 127.0).round() as i8, (n[2] * 127.0).round() as i8, (lit * 127.0).round() as i8]
}

/// The vertex data of a moby's per-pixel lighting (`objects` `vs_moby`, vertex buffer 2): its normal, its light terms and light word.
#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct MobyLit {
    pub n: [i8; 4],
    pub s: [f32; 2],
    pub light: u32,
}

pub(crate) fn sets(level: &[uyafmt::gameplay::LightSet]) -> Result<Sets, String> {
    if level.len() > 16 {
        return Err(format!("lighting: {} light sets, the light pass has 16 slots", level.len()));
    }
    let mut rows = [[0.0; 4]; 64];
    for (k, s) in level.iter().enumerate() {
        rows[4 * k..4 * k + 4].copy_from_slice(&[s.colour[0], s.direction[0], s.colour[1], s.direction[1]]);
    }
    Ok(Sets { rows })
}

const SHADER: &str = r#"
struct Sets { rows: array<vec4<f32>, 64> };
@group(2) @binding(0) var<uniform> sets: Sets;

struct VInLit {
    @location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) colour: vec4<f32>,
    @location(3) normal: vec3<f32>, @location(4) base: vec4<f32>, @location(5) sets: u32,
};
struct VOutLit {
    // The colour and surface pipelines must agree exactly, as must lit and unlit terrain.
    @builtin(position) @invariant clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(2) depth: f32,
    @location(3) @interpolate(linear) f: f32,
    // The light pass's inputs, across the triangle: the base colour (bytes), the normal, each light's colour (w its wrap) and
    // direction.
    @location(4) @interpolate(linear) base: vec4<f32>,
    @location(5) normal: vec3<f32>,
    @location(6) col0: vec4<f32>,
    @location(7) dir0: vec3<f32>,
    @location(8) col1: vec4<f32>,
    @location(9) dir1: vec3<f32>,
    // The world position (a Rift Apart material's frame at the pixel, `MATERIAL`).
    @location(10) wpos: vec3<f32>,
};

// The vertex's lights as the light pass blends them: set A, or A x (1 - w) + B x w row by row with the directions renormalised
// (a zero direction stays zero).
fn renormal(d: vec3<f32>) -> vec3<f32> {
    let l = dot(d, d);
    return select(vec3<f32>(0.0), d * inverseSqrt(l), l > 0.0);
}

@vertex fn vs_lit(v: VInLit) -> VOutLit {
    // As the terrain's `vs`.
    var o: VOutLit;
    o.clip = view.m * vec4<f32>(v.pos, 1.0);
    o.uv = v.uv;
    o.depth = dot(v.pos - view.eye.xyz, view.forward.xyz);
    let f = view.fog.z + (view.fog.w - view.fog.z) * (o.depth - view.fog.x) / (view.fog.y - view.fog.x);
    o.f = floor(clamp(f, min(view.fog.z, view.fog.w), max(view.fog.z, view.fog.w)));
    o.base = v.base * 255.0;
    o.normal = v.normal;
    o.wpos = v.pos;
    let a = (v.sets & 15u) * 4u;
    let b = ((v.sets >> 4u) & 15u) * 4u;
    let w = f32(v.sets >> 8u) / 256.0;
    if (w == 0.0) {
        o.col0 = sets.rows[a];
        o.dir0 = sets.rows[a + 1u].xyz;
        o.col1 = sets.rows[a + 2u];
        o.dir1 = sets.rows[a + 3u].xyz;
    } else {
        o.col0 = sets.rows[a] * (1.0 - w) + sets.rows[b] * w;
        o.dir0 = renormal(sets.rows[a + 1u].xyz * (1.0 - w) + sets.rows[b + 1u].xyz * w);
        o.col1 = sets.rows[a + 2u] * (1.0 - w) + sets.rows[b + 2u] * w;
        o.dir1 = renormal(sets.rows[a + 3u].xyz * (1.0 - w) + sets.rows[b + 3u].xyz * w);
    }
    return o;
}

// The light pass's colour at the pixel: base + colour0 max(d0, d0 w0) + colour1 max(d1, d1 w1), in bytes (0x80 = 1.0), red,
// green and blue held at 255, alpha the base's; `n` the normal at the pixel (the one across the triangle, or a Rift Apart
// material's).
fn lit(i: VOutLit, n: vec3<f32>) -> VOut {
    let d0 = dot(n, renormal(i.dir0));
    let d1 = dot(n, renormal(i.dir1));
    let s0 = max(d0, d0 * i.col0.w);
    let s1 = max(d1, d1 * i.col1.w);
    let rgb = min(floor(i.base.rgb + 128.0 * (i.col0.rgb * s0 + i.col1.rgb * s1)), vec3<f32>(255.0));
    var o: VOut;
    o.clip = i.clip;
    o.uv = i.uv;
    o.colour = vec4<f32>(rgb, i.base.a) / 255.0;
    o.depth = i.depth;
    o.f = i.f;
    return o;
}

// The pixel: the light pass, and on a Rift Apart surface (`mat.rift.w` 1) with its material's normal (its grain is on the
// texel, the terrain's `gs`), then each light's specular, fogged as the colour is.
fn gs_lit(i: VOutLit) -> vec4<f32> {
    let m = material_at(i.uv);
    let duv1 = dpdx(i.uv) * mat.rift.xy;
    let duv2 = dpdy(i.uv) * mat.rift.xy;
    let dp1 = dpdx(i.wpos);
    let dp2 = dpdy(i.wpos);
    let rift = mat.rift.w > 0.5;
    // Each normal made only where it is used (`select` makes both sides).
    var n0 = renormal(i.normal);
    if (rift) {
        n0 = material_normal(i.normal, dp1, dp2, duv1, duv2, m);
    }
    // The texture's own relief over that (`material_relief`).
    let g1 = dpdx(i.uv);
    let g2 = dpdy(i.uv);
    var n = n0;
    if (mat.relief.x > 0.0) {
        n = material_relief(n0, dp1, dp2, i.uv, g1, g2);
    }
    var p = gs(lit(i, n));
    if (rift) {
        let e = renormal(view.eye.xyz - i.wpos);
        let spec = material_highlight(material_specular(n, e, i.col0, i.dir0, m) + material_specular(n, e, i.col1, i.dir1, m), m);
        // A metal reflects the light around it (`material_reflection`, the lit pixel its colour), its painted shading less.
        let sky = 127.5 * (max(i.col0.rgb, vec3<f32>(0.0)) + max(i.col1.rgb, vec3<f32>(0.0)));
        let refl = material_reflection(n, e, sky, p.rgb, m);
        let diffuse = 1.0 - MATERIAL_METAL_DIFFUSE * material_metal(m);
        p = vec4<f32>(min(floor(p.rgb * diffuse) + floor((spec + refl) * i.f / 256.0), vec3<f32>(255.0)), p.a);
    }
    return p;
}

@fragment fn fs_pass_lit(i: VOutLit) -> FOut {
    let p = gs_lit(i);
    if (p.a < mat.aref) { discard; }
    return blended(p);
}

@fragment fn fs_fail_lit(i: VOutLit) -> FOut {
    let p = gs_lit(i);
    if (p.a >= mat.aref) { discard; }
    return blended(p);
}
"#;

/// The Rift Apart materials (PLAN 9.2, `texnames::rift`), for the terrain's lit pipelines (`SHADER`) and the objects' shader: the
/// method the player approved (Veldin's cliff, 15 November 2026). A paired surface keeps its own texture (the pack's), colours
/// and all, and takes its material, tiled at the material's own size in the world (the material uniform's `rift`: tiles per 1.0
/// of u and v), from the material's map at Rift Apart's own resolution (binding 3, sampled repeating at binding 4: the normal's
/// x and y in red and green, the gloss in blue, the albedo's luminance times its cavities over its mean in alpha, 1.0 at 102;
/// `scripts/riftapart-pair.py` writes them from Rift Apart's own maps): the texel times that grain (`material_grain`), and with
/// the per-pixel lighting the map's normal turned into the world by the frame the surface's position and texture coordinates
/// make at the pixel (the game's meshes have no tangents), lit by the light pass's own lights, with each light's specular (GGX,
/// Smith and Schlick's Fresnel, at the material's gloss and reflectance) added in bytes, a light's colour of 1.0 to 255. The
/// look is data, from the surface's row (`texnames::RiftSurface`, `RiftSlot`): per material (the uniform's `grains`, `specs`
/// and `refls`; a surface of regions blends its materials' by the region map, `material_at`) the grain's power (1, Rift
/// Apart's own contrast, as approved), the specular's strength (`material_highlight`) and the reflectance's root, and the
/// surface's normal strength (`rift.z`: 1, the map's own).
pub(crate) const MATERIAL: &str = r#"
const MATERIAL_PI: f32 = 3.14159265;
const MATERIAL_REFLECT: f32 = 1.0;
const MATERIAL_METAL_DIFFUSE: f32 = 0.2;

// A remaster light's glow belongs to the frame buffer, whose alpha the game's bloom pass reads. Keep it separate from the
// source alpha used for the GS alpha/depth tests and colour blending. Scale the added glow by coverage so a transparent
// texel stays transparent and a fading light does not suddenly become opaque.
fn material_glow_alpha(a: f32) -> f32 {
    return select(a, max(a, (128.0 + mat.emit.y * 127.0) * min(a / 128.0, 1.0)), mat.emit.x > 0.0);
}

fn material_renormal(d: vec3<f32>) -> vec3<f32> {
    let l = dot(d, d);
    return select(vec3<f32>(0.0), d * inverseSqrt(l), l > 0.0);
}

// A Rift Apart material at a pixel: `map` its map's texel (the normal's x and y, the gloss, the grain), `look` x the grain's
// power, y the specular's strength, z the root of the reflectance at normal incidence; `fur` the fur's length there in the
// world, `furw` the fur's weight there (`objects` `fs_fur`), `catchlight` an eye's catchlight's strength, `metal` its metalness.
struct MaterialPx { map: vec4<f32>, look: vec3<f32>, fur: f32, furw: f32, catchlight: f32, metal: f32, ao: f32, emission: vec3<f32>, glow: f32 };

// The material at the pixel (any pixel: its gradients are the coordinates' own, taken here in uniform control flow). A
// surface of regions (`mat.rift.w` 2) blends its four materials, maps and looks, by the region map's weights at the texel
// (red the first material, green the second, blue the third, alpha the fourth; what they leave is no material: a flat normal,
// no gloss, a grain of 1, no specular), each tiled at its own size (`mat.rift.xy`, `mat.tiles`, `mat.tiles3.xy`).
fn material_at(uv: vec2<f32>) -> MaterialPx {
    // No Rift Apart surface (`mat.rift.w` 0): every caller reads its material only on one, but for its glow (none); no sample.
    if (mat.rift.w < 0.5) {
        return MaterialPx(vec4<f32>(0.5, 0.5, 0.0, 0.4), vec3<f32>(1.0, 0.0, 0.0), 0.0, 0.0, 0.0, 0.0, 1.0, vec3<f32>(0.0), 0.0);
    }
    // Scaled as the textures' are (`native::ViewU::lerp` z: sampled as at the picture's own size with DLSS).
    let g1 = dpdx(uv) * view.lerp.z;
    let g2 = dpdy(uv) * view.lerp.z;
    // Explicit gradients are taken above all material/weight branches, including the vegetation table's nonuniform
    // material selection. A single-material surface never uses its region map or the other three material maps.
    if (mat.rift.w <= 1.5) {
        let m = textureSampleGrad(mtl, msmp, uv * mat.rift.xy, g1 * mat.rift.xy, g2 * mat.rift.xy);
        return MaterialPx(m, vec3<f32>(mat.grains.x, mat.specs.x, mat.refls.x), mat.furs.x, step(1e-6, mat.furs.x), mat.catches.x, mat.metals.x, 1.0, vec3<f32>(0.0), 0.0);
    }
    // A fitted atlas (`pbr.tsv`, `mat.rift.w` 3, or 4 with an emission map): the maps at the atlas's own coordinates,
    // normal XY (green up) with the fur weight in A, occlusion/roughness/metalness (glTF order) with the lens weight in A,
    // emission RGB with glow in A. Colour stays the replacement atlas (sRGB); these maps are linear.
    if (mat.rift.w > 2.5) {
        let p0 = textureSampleGrad(mtl, msmp, uv * mat.rift.xy, g1 * mat.rift.xy, g2 * mat.rift.xy);
        let p1 = textureSampleGrad(mtl1, msmp, uv * mat.tiles.xy, g1 * mat.tiles.xy, g2 * mat.tiles.xy);
        var p2 = vec4<f32>(0.0);
        if (mat.rift.w > 3.5) { p2 = textureSampleGrad(mtl2, msmp, uv * mat.tiles.zw, g1 * mat.tiles.zw, g2 * mat.tiles.zw); }
        let metal = clamp(p1.b, 0.0, 1.0);
        return MaterialPx(vec4<f32>(p0.xy, 1.0 - p1.g, 0.4), vec3<f32>(0.0, 1.0, sqrt(mix(0.04, 0.6, metal))), mat.furs.x * p0.a,
            select(0.0, p0.a, mat.furs.x > 0.0), mat.catches.x * p1.a, metal, p1.r, p2.rgb * 4.0, p2.a);
    }
    let w = textureSampleGrad(rgn, msmp, uv, g1, g2);
    // Zero-weight materials contribute exactly zero. Preserve the original blend's arithmetic and sampling footprint
    // for every contributing map; do not threshold small weights or change the sampler/LOD.
    var m0 = vec4<f32>(0.0);
    var m1 = vec4<f32>(0.0);
    var m2 = vec4<f32>(0.0);
    var m3 = vec4<f32>(0.0);
    if (w.r != 0.0) { m0 = textureSampleGrad(mtl, msmp, uv * mat.rift.xy, g1 * mat.rift.xy, g2 * mat.rift.xy); }
    if (w.g != 0.0) { m1 = textureSampleGrad(mtl1, msmp, uv * mat.tiles.xy, g1 * mat.tiles.xy, g2 * mat.tiles.xy); }
    if (w.b != 0.0) { m2 = textureSampleGrad(mtl2, msmp, uv * mat.tiles.zw, g1 * mat.tiles.zw, g2 * mat.tiles.zw); }
    if (w.a != 0.0) { m3 = textureSampleGrad(mtl3, msmp, uv * mat.tiles3.xy, g1 * mat.tiles3.xy, g2 * mat.tiles3.xy); }
    let rest = max(1.0 - (w.r + w.g + w.b + w.a), 0.0);
    let blend = m0 * w.r + m1 * w.g + m2 * w.b + m3 * w.a + vec4<f32>(0.5, 0.5, 0.0, 0.4) * rest;
    let looks = vec3<f32>(dot(w, mat.grains) + rest, dot(w, mat.specs), dot(w, mat.refls));
    return MaterialPx(blend, looks, dot(w, mat.furs), dot(w, step(vec4<f32>(1e-6), mat.furs)), dot(w, mat.catches), dot(w, mat.metals), 1.0, vec3<f32>(0.0), 0.0);
}

// The material's normal in the world at the pixel: `n0` the surface's own, `dp1` and `dp2` the position's change across the
// pixel, `duv1` and `duv2` the material's coordinates'; Rift Apart's green is up in its images, against v. Its slope is the
// map's times `mat.rift.z`.
fn material_normal(n0: vec3<f32>, dp1: vec3<f32>, dp2: vec3<f32>, duv1: vec2<f32>, duv2: vec2<f32>, m: MaterialPx) -> vec3<f32> {
    let n = material_renormal(n0);
    let p2 = cross(dp2, n);
    let p1 = cross(n, dp1);
    let t = p2 * duv1.x + p1 * duv2.x;
    let b = p2 * duv1.y + p1 * duv2.y;
    let s = max(dot(t, t), dot(b, b));
    if (s <= 0.0) {
        return n;
    }
    let xy = m.map.xy * 2.0 - 1.0;
    let z = sqrt(max(1.0 - dot(xy, xy), 0.0));
    return material_renormal((t * xy.x - b * xy.y) * (mat.rift.z * inverseSqrt(s)) + n * z);
}

// A texture's relief (`texnames` `relief.tsv`; `mat.relief.x` its strength, 1 the map's own slope): the normal made from the
// texture's own generated height (x and y in red and green, green up against v, as a fitted atlas's), at the texture's own
// coordinates, one repeat, tilting `n` (the surface's normal, its material's already on it) in the frame the position and the
// texture's coordinates make at the pixel, as `material_normal` takes it. `g1` and `g2` are the coordinates' change across the
// pixel, taken by the caller where every pixel of the draw runs.
fn material_relief(n: vec3<f32>, dp1: vec3<f32>, dp2: vec3<f32>, uv: vec2<f32>, g1: vec2<f32>, g2: vec2<f32>) -> vec3<f32> {
    let r = textureSampleGrad(rlf, msmp, uv, g1 * view.lerp.z, g2 * view.lerp.z);
    let p2 = cross(dp2, n);
    let p1 = cross(n, dp1);
    let t = p2 * g1.x + p1 * g2.x;
    let b = p2 * g1.y + p1 * g2.y;
    let s = max(dot(t, t), dot(b, b));
    if (s <= 0.0) {
        return n;
    }
    let xy = r.xy * 2.0 - 1.0;
    let z = sqrt(max(1.0 - dot(xy, xy), 0.0));
    return material_renormal((t * xy.x - b * xy.y) * (mat.relief.x * inverseSqrt(s)) + n * z);
}

// A Rift Apart surface's grain on its texel: the map's alpha (1.0 at 102) to the grain's power.
fn material_grain(m: MaterialPx) -> f32 {
    return pow(m.map.a * 2.5, m.look.x);
}

// The lights' specular `s` (bytes, `material_specular`) as the surface shows it: times the material's specular strength
// (`m.look.y`), rolled off toward 255 (1 - e^-x: a faint highlight as it is, a strong one no longer adding past white: a
// glossy paint under the game's sparse lights has no reflections or tone mapping to balance its peak, as in Rift Apart).
fn material_highlight(s: vec3<f32>, m: MaterialPx) -> vec3<f32> {
    return 255.0 * (vec3<f32>(1.0) - exp(-s * m.look.y / 255.0));
}

// An eye's catchlight in bytes (`m.catchlight`; none elsewhere): a white light of that strength from just above the eye's own
// view (the world's z is up), as a film's eye light, its highlight on the eye's gloss wherever the eye looks.
fn material_catchlight_filtered(n: vec3<f32>, e: vec3<f32>, m: MaterialPx, normal_variance: f32) -> vec3<f32> {
    // With no catchlight the old expression selected zero after doing a full extra GGX light.
    if (m.catchlight <= 0.0) { return vec3<f32>(0.0); }
    return material_specular_filtered(n, e, vec4<f32>(m.catchlight), e + vec3<f32>(0.0, 0.0, 0.35), m, normal_variance);
}

// A metal's reflection of what is around it, in bytes (`objects` `gs`, the terrain's `gs_lit`; with the ray-traced reflections
// on, `rtlight` traces the true one over it): the light around it as the game's own lights give it there (`sky`, bytes: the
// two lights' colours, averaged) above the horizon in the reflected direction, a fifth of it below (the ground), the horizon
// softened by the material's roughness (its gloss); times Schlick's Fresnel at its reflectance (`texnames::RiftSlot::f0`),
// times MATERIAL_REFLECT, tinted three quarters of the way to its own colour over a mid grey (`t`, bytes: a metal's reflection
// takes its colour, a dark paint stays dark); times its metalness (`material_metal`: a paint, a stone, a cloth reflects none
// here).
fn material_reflection(n: vec3<f32>, e: vec3<f32>, sky: vec3<f32>, t: vec3<f32>, m: MaterialPx) -> vec3<f32> {
    let f0 = m.look.z * m.look.z;
    let r = reflect(-e, n);
    let rough = 1.0 - m.map.z;
    let above = smoothstep(-0.1 - rough, 0.1 + rough, r.z);
    let fres = f0 + (1.0 - f0) * pow(1.0 - max(dot(n, e), 0.0), 5.0);
    let tint = mix(vec3<f32>(1.0), min(t / 128.0, vec3<f32>(1.5)), 0.75);
    return sky * (0.2 + 0.8 * above) * fres * material_metal(m) * tint * MATERIAL_REFLECT;
}

// How much of a metal the material is (`texnames::RiftSlot` metal): a metal's own colour is mostly what it reflects, so its
// painted shading is drawn at 1 - MATERIAL_METAL_DIFFUSE x its metalness (`objects` `gs`, `gs_lit`).
fn material_metal(m: MaterialPx) -> f32 {
    return m.metal;
}

fn material_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(c/12.92,pow((c+0.055)/1.055,vec3<f32>(2.4)),c>vec3<f32>(0.04045));
}
fn material_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let v=max(c,vec3<f32>(0.0));
    return select(v*12.92,1.055*pow(v,vec3<f32>(1.0/2.4))-0.055,v>vec3<f32>(0.0031308));
}

// Energy-conserving GGX metallic/roughness BRDF for authored character atlases.
// n/l/e are world-space; colours and ambient come from the captured game light
// sets, not a viewer studio rig. AO only attenuates indirect illumination.
fn material_pbr_light(base:vec3<f32>,n:vec3<f32>,e:vec3<f32>,d:vec3<f32>,radiance:vec3<f32>,metal:f32,a2:f32) -> vec3<f32> {
    let l=material_renormal(d);
    let nl=max(dot(n,l),0.0);
    let nv=max(dot(n,e),0.001);
    let h=material_renormal(l+e);
    let nh=max(dot(n,h),0.0);
    let vh=max(dot(e,h),0.0);
    let f0=mix(vec3<f32>(0.04),base,metal);
    let f=f0+(1.0-f0)*pow(1.0-vh,5.0);
    let q=nh*nh*(a2-1.0)+1.0;
    let distribution=a2/(MATERIAL_PI*q*q);
    let gv=nl*sqrt(nv*nv*(1.0-a2)+a2);
    let gl=nv*sqrt(nl*nl*(1.0-a2)+a2);
    let visibility=0.5/max(gv+gl,0.00001);
    let diffuse=(1.0-f)*(1.0-metal)*base/MATERIAL_PI;
    return (diffuse+f*distribution*visibility)*max(radiance,vec3<f32>(0.0))*nl*MATERIAL_PI;
}
fn material_pbr(base_srgb:vec3<f32>,n:vec3<f32>,e:vec3<f32>,c0:vec3<f32>,d0:vec3<f32>,c1:vec3<f32>,d1:vec3<f32>,ambient:vec3<f32>,m:MaterialPx,variance:f32) -> vec3<f32> {
    let base=material_to_linear(clamp(base_srgb,vec3<f32>(0.0),vec3<f32>(1.0)));
    let rough=clamp(1.0-m.map.z,0.045,1.0);
    let a2=clamp(rough*rough*rough*rough+variance,0.0009,1.0);
    let direct=material_pbr_light(base,n,e,d0,c0,m.metal,a2)+material_pbr_light(base,n,e,d1,c1,m.metal,a2);
    let f0=mix(vec3<f32>(0.04),base,m.metal);
    // A diffuse irradiance approximation from the level's ambient. Traced
    // environment reflections remain the runtime reflection pass's job.
    let indirect=max(ambient,vec3<f32>(0.0))*((1.0-f0)*(1.0-m.metal)*base+f0*(1.0-0.5*rough))*m.ao;
    return 255.0*min(material_to_srgb(direct+indirect+m.emission),vec3<f32>(1.0));
}

// A light's specular in bytes: `n` the material's normal, `e` toward the eye, `c` the light's colour, `d` its direction.
fn material_specular(n: vec3<f32>, e: vec3<f32>, c: vec4<f32>, d: vec3<f32>, m: MaterialPx) -> vec3<f32> {
    // Integrate the highlight over the normal's pixel footprint. Without this, a normal map's subpixel peaks can switch
    // between black and white as a leaf or the camera moves. Derivatives must precede the per-pixel facing branch.
    return material_specular_filtered(n, e, c, d, m, material_normal_variance(n));
}

fn material_normal_variance(n: vec3<f32>) -> f32 {
    let nx = dpdx(n);
    let ny = dpdy(n);
    return 0.5 * (dot(nx, nx) + dot(ny, ny));
}

// The same formula with the derivatives supplied before an alpha discard (`objects::gs_test`).
fn material_specular_filtered(n: vec3<f32>, e: vec3<f32>, c: vec4<f32>, d: vec3<f32>, m: MaterialPx, normal_variance: f32) -> vec3<f32> {
    let l = material_renormal(d);
    let nl = dot(n, l);
    let nv = dot(n, e);
    if (nl <= 0.0 || nv <= 0.0) {
        return vec3<f32>(0.0);
    }
    let h = material_renormal(l + e);
    let nh = max(dot(n, h), 0.0);
    let vh = max(dot(e, h), 0.0);
    let r = 1.0 - m.map.z;
    let a2 = clamp(r * r * r * r + normal_variance, 0.0009, 1.0);
    let a = sqrt(a2);
    let q = nh * nh * (a2 - 1.0) + 1.0;
    let dist = a2 / (MATERIAL_PI * q * q);
    let k = a * 0.5;
    let vis = 1.0 / (4.0 * (nl * (1.0 - k) + k) * (nv * (1.0 - k) + k));
    let f0 = m.look.z * m.look.z;
    let fres = f0 + (1.0 - f0) * pow(1.0 - vh, 5.0);
    return 255.0 * max(c.rgb, vec3<f32>(0.0)) * (dist * vis * fres * nl * MATERIAL_PI);
}
"#;

/// The lit terrain's pipelines (passing and failing pixels, as the terrain's own) and the light sets' group layout.
pub(crate) struct Lighting {
    pub pass: wgpu::RenderPipeline,
    pub fail: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

/// A level's terrain lighting on the GPU: the vertices' light pass inputs and the light sets' group.
pub(crate) struct TerrainLit {
    pub vertices: wgpu::Buffer,
    pub group: wgpu::BindGroup,
}

impl Lighting {
    /// `terrain` is the terrain's own shader (`native::SHADER` with `MATERIAL`), `view_layout` and `mat_layout` its groups 0 and 1, `blend` and
    /// `depth` its pass pipeline's blend and depth format.
    pub fn new(device: &wgpu::Device, terrain: &str, view_layout: &wgpu::BindGroupLayout, mat_layout: &wgpu::BindGroupLayout, blend: wgpu::BlendState, depth: wgpu::TextureFormat) -> Lighting {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("terrain, lit per pixel"), source: wgpu::ShaderSource::Wgsl(format!("{terrain}{SHADER}{}", crate::surface_buffer::LIT).into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("light sets"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("terrain, lit per pixel"), bind_group_layouts: &[Some(view_layout), Some(mat_layout), Some(&layout)], immediate_size: 0 });
        let attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Unorm8x4];
        let lit_attrs = wgpu::vertex_attr_array![3 => Float32x3, 4 => Unorm8x4, 5 => Uint32];
        let pipeline = |entry: &str, depth_write: bool| {
            crate::surface_buffer::pipeline(device, &wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_lit"),
                    compilation_options: Default::default(),
                    buffers: &[
                        Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<crate::native::Vertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs }),
                        Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<LitVertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &lit_attrs }),
                    ],
                },
                // As the terrain's own pipelines (`Native::open`).
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState { format: depth, depth_write_enabled: Some(depth_write), depth_compare: Some(wgpu::CompareFunction::GreaterEqual), stencil: Default::default(), bias: Default::default() }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: Some(blend), write_mask: if depth_write { wgpu::ColorWrites::ALL } else { wgpu::ColorWrites::COLOR } })],
                }),
                multiview_mask: None,
                cache: crate::pipelines::cache(),
            }, depth_write.then_some("fs_surface_lit"))
        };
        Lighting { pass: pipeline("fs_pass_lit", true), fail: pipeline("fs_fail_lit", false), layout }
    }

    /// A level's terrain lighting: each terrain vertex's inputs (in the order of the terrain's vertices) and the level's sets.
    pub fn terrain(&self, device: &wgpu::Device, vertices: &[LitVertex], sets: &Sets) -> TerrainLit {
        use wgpu::util::DeviceExt;
        let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("terrain light pass inputs"), contents: bytemuck::cast_slice(vertices), usage: wgpu::BufferUsages::VERTEX });
        let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("light sets"), contents: bytemuck::bytes_of(sets), usage: wgpu::BufferUsages::UNIFORM });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("light sets"), layout: &self.layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() }] });
        TerrainLit { vertices: vb, group }
    }
}
