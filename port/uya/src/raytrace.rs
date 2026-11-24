//! Ray tracing (PLAN 8.10): the level's acceleration structures, which the GPU's ray tracing hardware searches (Vulkan's ray
//! queries, `VK_KHR_ray_query`, which wgpu 30 offers as `Features::EXPERIMENTAL_RAY_QUERY`), with what each surface in them is
//! drawn with, and the ray-traced shadows: the `shadows` option's RAY TRACED value. The ray-traced lighting and reflections
//! (`rtlight.rs`) search the same structures and shade what they meet with this module's scene library (`scene`).
//!
//! **The maps' shadow, traced.** The key light, how dark, where it shades and how far a shadow reaches are `shadows.rs`'s: the
//! brighter light of the light set most terrain is lit by (`shadows::key_light`); three quarters of the colour kept
//! (`shadows::DARKNESS`) where the surface faces the light, faded by the frame's fog; a shadow reaches `shadows::REACH` from what
//! casts it, fading over its second half, so an interior lit by the level's sun keeps the game's light. Every caster is
//! alpha-tested with its own texture and TEST_1 AREF, as it draws, and the game's own darken steps darken nothing (`native.rs`,
//! FIX 0), so nothing is darkened twice.
//!
//! **What the rays give that the maps do not.** The nearest caster along the light, where a map keeps the one nearest the light
//! (a crate's shadow under a canopy: the map holds the canopy, out of reach, and the crate's shadow is lost); a shadow at every
//! distance the picture shows, where the far map ends 120 units from the camera; no texels: an edge as sharp far off as near,
//! no acne, and no gap between a caster's foot and its shadow.
//!
//! **The acceleration structures.** The static world (`World`): bottom levels built at the level's first ray-traced frame, and
//! again when the terrain buffer holds another chunk: one of the terrain in the terrain buffer (each of its draws a geometry,
//! from the level's own vertex and index buffers), one per tie class and LOD (`objects::Ties::rt_classes`) and one per shrub
//! class (`objects::Shrubs::rt_classes`); one top level placing every tie the level shows at the LOD the game draws it at and
//! every shrub (`objects::Ties::rt_instances`, `objects::Shrubs::rt_instances`), made again when either changes. Checked
//! against the picture (`UYA_RT_CHECK`: the eye's ray through each pixel against its depth): on Veldin's play dump 99.7% of
//! the pixels meet the surface the picture shows, the rest the edges of alpha-tested leaves and glows. The mobies (`Actors`, each
//! frame): the frame's opaque moby draws into the main frame (the ambient occlusion's occluders, the moby shadow map's
//! casters) in one bottom level with a top level of its own, both built again in each picture from where that picture draws
//! them (`replay::Cmd::RtCasters`: a compute pass puts each vertex `view.lerp.x` of the way back to the frame before, as
//! `vs_moby` does), so a picture between two frames (PLAN 8.8) is shaded by its own mobies.
//!
//! **What a ray meets is drawn as the game draws it** (`scene`'s `scene_hit`): every geometry (`Geom`) has its triangles'
//! corners in its group's index buffer, their vertices' places, texture coordinates and colours in its group's vertex buffers,
//! and its material (`MatRt`: its texture, its AREF and clamps, and whether the game draws a shine over it). The static world's
//! vertex buffers hold the terrain's vertices (copied from the level's own by the gather pass) and every tie and shrub class's;
//! a tie's or shrub's colour is its instance's, as the game lights it: the vertex names a word of its instance's colours
//! (`objects::RtClass::colour`), which the top level's instance table (`Placed::inst`, by the hit's instance) finds in the
//! frame's colours (the ties' colour buffers and the shrubs' colour blocks as RAM holds them, uploaded again when they change).
//! The mobies' are the frame's moby vertices themselves (`objects::ObjVertex`). The EE's layers (`Layers`, made once a frame
//! for the ray-traced lighting and reflections from `EeIn`): the EE's triangles drawn into the main frame in the world (the
//! water's and the sea's programs' output, the slime, the EE's own triangles) and program 6's effects, each draw's material
//! lying in what a ray sees as its blend draws it (`MODE_*`): over what is behind it by its alpha or a fixed share, or added to
//! it; a ray sees every one of them before the surface it ends on (`scene_hit`). At a hit: the texel at the hit's texture
//! coordinates, at the level of detail the ray's width there picks (from the triangle's texels over its area), times the
//! vertex colours across the triangle as the GS modulates (Ct x Cv / 0x80); the triangle's own normal, turned to the ray.
//!
//! **Built for the RTX's tracing**: the static world's bottom levels are compacted after their build (`ALLOW_COMPACTION`,
//! `Queue::compact_blas`: NVIDIA's advice for every static structure on its RTX cards; Florana's 147 in 44 ms at the level's
//! first ray-traced frame): the driver's copy at the built size, less of the GPU's memory (which this PC's driver charges to the
//! process) and fewer bytes fetched while tracing. The top level is placed again only when the ties' or shrubs' instance or
//! matrix arrays change in RAM (`LevelRt::state`: compared as bytes; Florana's instances 0.44 ms a frame to 0.03).
//!
//! **Alpha.** A geometry whose texture has no texel below its AREF is opaque: the hardware commits its hits. The others are
//! tested at each candidate hit: the texel's alpha at the hit's texture coordinates against the AREF, and the hit kept only when
//! it passes.
//!
//! **The shadows applied** as the maps' shade is: commands of the frame's recorded list, after the static world over every pixel
//! and after the moby runs over the mobies' pixels (`replay::Cmd::RtShadows`), each pixel put back in the world from the depth
//! buffer with the picture's own camera, then one ray from just off its surface toward the light, `REACH` long.

use std::collections::HashMap;
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// The `shadows` option's ray-traced value (1 to 3 are the shadow maps' sizes, `shadows::SIZES`).
pub(crate) const RAY_TRACED: u8 = 4;

/// The most textures a level's static world, or a frame's mobies, may have: the size of the scene groups' texture arrays.
const TEXTURES: u32 = 4096;

/// The 3 x 4 transform that leaves a point where it is.
const IDENTITY: [f32; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];

/// The most moby vertices one dispatch of the places' pass reaches (65,535 groups of 64).
const MOST_VERTICES: usize = 65535 * 64;

/// A geometry's flags (`Geom::flags`): its colours are its instance's (a word of the frame's colours, `Placed::inst`); its
/// vertices are the effects' (the EE layers' binding 8, `Layers`).
const G_INSTANCE: u32 = 1;
const G_EFFECTS: u32 = 2;

/// How a material's pixels lie in what a ray sees (`MatRt::mode`): a surface that ends the ray (the static world, the mobies);
/// or one of the EE's layers drawn over what is behind it (the water, the sea, the slime, the effects, the EE's own triangles:
/// `EeIn`), as its blend draws it: over it by the pixel's alpha (ALPHA_1 (Cs - Cd) x As + Cd), over it by a fixed share (FIX,
/// or a colour that replaces: `factor`), or added to it by the pixel's alpha or a fixed share.
pub(crate) const MODE_SURFACE: u32 = 0;
const MODE_OVER_ALPHA: u32 = 1;
const MODE_OVER_FIX: u32 = 2;
const MODE_ADD_ALPHA: u32 = 3;
const MODE_ADD_FIX: u32 = 4;

/// Which group a scene group's WGSL is (`group_wgsl`): the static world (instance colours, four words a vertex), the mobies
/// (their places apart from the frame's moby vertices) or the EE's layers (the frame's EE vertices, places included).
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    World,
    Mobies,
    Ee,
}

/// The scene library (WGSL) every ray-traced pass shades with: the static world's group at `@group(1)`, the mobies' at
/// `@group(2)` when the frame has mobies to trace (`mobies`), the EE's layers' at the next group when the frame has them and
/// the pass sees them (`ee`). It gives:
/// - `scene_t(o, d, tmax)`: how far from `o` along `d` the nearest surface lies within `tmax` (`MISS`: none);
///   `scene_t_from(o, d, tmin, tmax)` the same from `tmin` on;
/// - `scene_any(o, d, tmax)`: whether any surface lies within `tmax` (the hardware stops at the first it confirms: a shadow
///   ray's test, `RAY_FLAG_TERMINATE_ON_FIRST_HIT`);
/// - `scene_surface(o, d, tmin, tmax, w0, spread)`: the nearest surface between `tmin` and `tmax` (`Surface`; `hit` 0: none),
///   drawn as the game draws it, its texture at the level of detail of a ray `w0 + spread x t` wide where it meets it (a pixel's
///   own surface is looked for only about its depth: the hardware then searches only the boxes there);
/// - `scene_hit(o, d, tmax, w0, spread, near)`: what the ray sees: that surface with the EE's layers before it from `near` on
///   (the water and slime over it, the effects' light added: `Surface::add`); where no surface ends the ray, the nearest layer.
/// Each group: the top level (0), its geometries (1), materials (2), corners (3: indices of vertices), the vertices' places (4:
/// three floats each; the EE's the vertices themselves), their texture coordinates and colours (5: the static world's four words
/// each, U and V as float bits, the colour and the game's normal, `objects::RtClass::normal`; the mobies' and the EE's their
/// frame's vertices, `objects::ObjVertex`, eight words each), the textures (6) and their sampler (7); the static world's also its
/// instances' first colour words (8) and the frame's colours (9).
pub(crate) fn scene(mobies: bool, ee: bool) -> String {
    let mut s = String::from(SCENE_COMMON);
    s += &group_wgsl("world", 1, Kind::World);
    if mobies {
        s += &group_wgsl("mobies", 2, Kind::Mobies);
    }
    if ee {
        s += &group_wgsl("ee", 2 + mobies as u32, Kind::Ee);
    }
    let (t, any, surface) = if mobies {
        (
            "    let a = world_t(o, d, tmin, tmax);\n    return min(a, mobies_t(o, d, tmin, min(a, tmax)));\n",
            "    return world_any(o, d, tmax) || mobies_any(o, d, tmax);\n",
            r#"    let a = world_trace(o, d, tmin, tmax, RAY_FLAG_NONE, w0, spread);
    var far = tmax;
    if (a.kind != RAY_QUERY_INTERSECTION_NONE) {
        far = a.t;
    }
    let b = mobies_trace(o, d, tmin, far, RAY_FLAG_NONE, w0, spread);
    if (b.kind != RAY_QUERY_INTERSECTION_NONE) {
        return mobies_surface(b, o, d, w0, spread);
    }
    if (a.kind != RAY_QUERY_INTERSECTION_NONE) {
        return world_surface(a, o, d, w0, spread);
    }
    var none: Surface;
    return none;
"#,
        )
    } else {
        (
            "    return world_t(o, d, tmin, tmax);\n",
            "    return world_any(o, d, tmax);\n",
            r#"    let a = world_trace(o, d, tmin, tmax, RAY_FLAG_NONE, w0, spread);
    if (a.kind != RAY_QUERY_INTERSECTION_NONE) {
        return world_surface(a, o, d, w0, spread);
    }
    var none: Surface;
    return none;
"#,
        )
    };
    let hit = if ee {
        r#"    var s = scene_surface(o, d, 0.0, tmax, w0, spread);
    let l = ee_through(o, d, near, select(tmax, s.t, s.hit != 0u), w0, spread);
    s.add = l.add;
    if (l.t < MISS) {
        if (s.hit != 0u) {
            s.colour = mix(s.colour, l.layer.rgb, l.layer.a);
        } else {
            s.hit = 1u;
            s.layer = 1u;
            s.t = l.t;
            s.p = o + d * l.t;
            s.n = -d;
            s.colour = l.layer.rgb * l.layer.a;
            s.texel = l.layer.rgb;
            s.alpha = 1.0;
            s.vertex = vec3<f32>(1.0);
        }
    }
    return s;
"#
    } else {
        "    return scene_surface(o, d, 0.0, tmax, w0, spread);\n"
    };
    s += &format!(
        "\nfn scene_t_from(o: vec3<f32>, d: vec3<f32>, tmin: f32, tmax: f32) -> f32 {{\n{t}}}\n\nfn scene_t(o: vec3<f32>, d: vec3<f32>, tmax: f32) -> f32 {{\n    return scene_t_from(o, d, 0.0, tmax);\n}}\n\nfn scene_any(o: vec3<f32>, d: vec3<f32>, tmax: f32) -> bool {{\n{any}}}\n\nfn scene_surface(o: vec3<f32>, d: vec3<f32>, tmin: f32, tmax: f32, w0: f32, spread: f32) -> Surface {{\n{surface}}}\n\nfn scene_hit(o: vec3<f32>, d: vec3<f32>, tmax: f32, w0: f32, spread: f32, near: f32) -> Surface {{\n{hit}}}\n"
    );
    s
}

const SCENE_COMMON: &str = r#"
// A geometry: its first corner in its group's corners, the vertex its corners count from, its material, its flags (bit 0: its
// colours are its instance's). A material: its texture in the group's array, its TEST_1 AREF, its clamps (bit 0 S, bit 1 T),
// the shine the game draws over it (1: a tie packet's second part), how its pixels lie in what a ray sees (`raytrace::MODE_*`:
// 0 a surface; 1 and 2 a layer over what is behind by the pixel's alpha or by `factor`; 3 and 4 a layer added by the pixel's
// alpha or by `factor`).
struct Geom { first: u32, base: u32, mat: u32, flags: u32 };
struct Mat { tex: u32, aref: f32, clamp: u32, shine: f32, mode: u32, factor: f32, min_alpha: u32, pad1: u32, rift: u32, region: u32, pad2: u32, pad3: u32, maps: vec4<u32>, tiles01: vec4<f32>, tiles23: vec4<f32>, f0: vec4<f32>, metal: vec4<f32> };
// A material's Rift Apart surface (`texnames::RiftMat`, `Shade::mat`): `rift` 0 none, 1 one material, 2 a surface of regions
// (its region map at `region` of the group's textures); per material its map (`NO_MAP`: none, fur), its tiles, its F0 and its
// metalness.
const NO_MAP: u32 = 0xFFFFFFFFu;
// The level of a Rift Apart map its gloss is read at for a reflection's weight (its gloss over a few texels of the material).
const RIFT_GLOSS_LOD: f32 = 3.0;
// What a ray met (`hit` 1), as the game draws it: where, how far, its normal turned to the ray (the game's own across the
// triangle where it gives one, `objects::RtClass::normal`, else the triangle's), its colour (the texel times the
// vertex colours, as the GS modulates; with the EE's layers before it over it), its texel alone, its vertex colours, the shine
// the game draws over it there (its material's times the pixel's alpha), the light the EE's added layers before it add, and
// whether what the ray met is one of the EE's layers with no surface behind it (`layer` 1).
// With a Rift Apart surface (`f0`, `metal`): its reflectance at normal incidence (below 0: the game's own chrome, the pass's
// metal F0) and its metalness; its `shine` its material's gloss there.
struct Surface { hit: u32, t: f32, p: vec3<f32>, n: vec3<f32>, colour: vec3<f32>, texel: vec3<f32>, vertex: vec3<f32>, shine: f32, add: vec3<f32>, layer: u32, f0: f32, metal: f32, alpha: f32 };
// A triangle's pixel where a ray meets it: the triangle's normal in the world (not turned), the game's own normal there (0:
// none), the texel and the vertex colours (bytes, the alpha on the GS scale), the pixel the GS modulates of them.
struct Shaded { n: vec3<f32>, game_n: vec3<f32>, texel: vec4<f32>, vertex: vec4<f32>, px: vec4<f32>, uv: vec2<f32>, footprint: vec2<f32> };
// The EE's layers along a ray: the light the added ones add, and the nearest one drawn over what is behind it (its colour, and
// how much of what is behind it it covers) and how far it is (`MISS`: none).
struct Through { add: vec3<f32>, layer: vec4<f32>, t: f32 };

// No surface within the ray's length.
const MISS: f32 = 1e30;
const G_INSTANCE: u32 = 1u;
const G_EFFECTS: u32 = 2u;

fn scene_texel_uv(uv0: vec2<f32>, clamps: u32, dims: vec2<f32>) -> vec2<f32> {
    var uv = uv0;
    let half = 0.5 / dims;
    if ((clamps & 1u) != 0u) {
        uv.x = clamp(uv.x, half.x, 1.0 - half.x);
    }
    if ((clamps & 2u) != 0u) {
        uv.y = clamp(uv.y, half.y, 1.0 - half.y);
    }
    return uv;
}
"#;

/// One scene group's WGSL (`scene`): its bindings at `@group(group)`, `NAME_shaded` (a triangle's pixel where a ray meets it),
/// and for a group of surfaces `NAME_trace` (the nearest surface, each candidate of an alpha-tested geometry kept only where its
/// texel passes), `NAME_t` and `NAME_surface`; for the EE's layers `NAME_through`.
fn group_wgsl(name: &str, group: u32, kind: Kind) -> String {
    let (extra, place, uv, colour, normal) = match kind {
        Kind::World => (
            format!("@group({group}) @binding(8) var<storage, read> {name}_inst: array<u32>;\n@group({group}) @binding(9) var<storage, read> {name}_cols: array<u32>;\n"),
            format!("    return vec3<f32>({name}_pos[3u * v], {name}_pos[3u * v + 1u], {name}_pos[3u * v + 2u]);"),
            format!("    return vec2<f32>(bitcast<f32>({name}_attr[4u * v]), bitcast<f32>({name}_attr[4u * v + 1u]));"),
            format!("    var c = {name}_attr[4u * v + 2u];\n    if ((g.flags & G_INSTANCE) != 0u) {{\n        c = {name}_cols[{name}_inst[inst] + c];\n    }}\n    return unpack4x8unorm(c);"),
            format!("    return {name}_attr[4u * v + 3u];"),
        ),
        Kind::Mobies => (
            String::new(),
            format!("    return vec3<f32>({name}_pos[3u * v], {name}_pos[3u * v + 1u], {name}_pos[3u * v + 2u]);"),
            format!("    return vec2<f32>(bitcast<f32>({name}_attr[8u * v + 3u]), bitcast<f32>({name}_attr[8u * v + 4u])) / bitcast<f32>({name}_attr[8u * v + 5u]);"),
            format!("    return unpack4x8unorm({name}_attr[8u * v + 6u]);"),
            "    return 0u;".to_string(),
        ),
        Kind::Ee => (
            format!("@group({group}) @binding(8) var<storage, read> {name}_fx: array<u32>;\n\n// Word `i` of the vertices of geometry `g`: the EE's, or the effects'.\nfn {name}_word(g: Geom, i: u32) -> u32 {{\n    if ((g.flags & G_EFFECTS) != 0u) {{\n        return {name}_fx[i];\n    }}\n    return {name}_attr[i];\n}}\n"),
            format!("    return vec3<f32>(bitcast<f32>({name}_word(g, 8u * v)), bitcast<f32>({name}_word(g, 8u * v + 1u)), bitcast<f32>({name}_word(g, 8u * v + 2u)));"),
            format!("    return vec2<f32>(bitcast<f32>({name}_word(g, 8u * v + 3u)), bitcast<f32>({name}_word(g, 8u * v + 4u))) / bitcast<f32>({name}_word(g, 8u * v + 5u));"),
            format!("    return unpack4x8unorm({name}_word(g, 8u * v + 6u));"),
            "    return 0u;".to_string(),
        ),
    };
    let mut s = format!(
        r#"
@group({group}) @binding(0) var {name}_as: acceleration_structure;
@group({group}) @binding(1) var<storage, read> {name}_geoms: array<Geom>;
@group({group}) @binding(2) var<storage, read> {name}_mats: array<Mat>;
@group({group}) @binding(3) var<storage, read> {name}_idx: array<u32>;
@group({group}) @binding(4) var<storage, read> {name}_pos: array<f32>;
@group({group}) @binding(5) var<storage, read> {name}_attr: array<u32>;
@group({group}) @binding(6) var {name}_tex: binding_array<texture_2d<f32>>;
@group({group}) @binding(7) var {name}_smp: sampler;
{extra}
fn {name}_vertex(g: Geom, prim: u32, k: u32) -> u32 {{
    return {name}_idx[g.first + 3u * prim + k] + g.base;
}}

fn {name}_place(g: Geom, v: u32) -> vec3<f32> {{
{place}
}}

fn {name}_uv(g: Geom, v: u32) -> vec2<f32> {{
{uv}
}}

fn {name}_colour(v: u32, g: Geom, inst: u32) -> vec4<f32> {{
{colour}
}}

// The game's normal of vertex `v` as four signed bytes (0: none).
fn {name}_normal(v: u32) -> u32 {{
{normal}
}}

fn {name}_texel(m: Mat, uv: vec2<f32>, lod: f32) -> vec4<f32> {{
    let dims = vec2<f32>(textureDimensions({name}_tex[m.tex]));
    return textureSampleLevel({name}_tex[m.tex], {name}_smp, scene_texel_uv(uv, m.clamp, dims), lod);
}}

// The pixel of geometry `g` (material `m`), triangle `prim`, at barycentrics `b` of instance `inst` placed by `o2w`, its texture
// at the level of a ray `width` wide there.
fn {name}_shaded(g: Geom, m: Mat, prim: u32, b: vec2<f32>, inst: u32, o2w: mat4x3<f32>, width: f32) -> Shaded {{
    let v0 = {name}_vertex(g, prim, 0u);
    let v1 = {name}_vertex(g, prim, 1u);
    let v2 = {name}_vertex(g, prim, 2u);
    let w = vec3<f32>(1.0 - b.x - b.y, b.x, b.y);
    let p0 = o2w * vec4<f32>({name}_place(g, v0), 1.0);
    let p1 = o2w * vec4<f32>({name}_place(g, v1), 1.0);
    let p2 = o2w * vec4<f32>({name}_place(g, v2), 1.0);
    let c = cross(p1 - p0, p2 - p0);
    let area = max(length(c), 1e-12);
    var o: Shaded;
    o.n = c / area;
    let g0 = {name}_normal(v0);
    let g1 = {name}_normal(v1);
    let g2 = {name}_normal(v2);
    if (g0 != 0u && g1 != 0u && g2 != 0u) {{
        let gn = unpack4x8snorm(g0).xyz * w.x + unpack4x8snorm(g1).xyz * w.y + unpack4x8snorm(g2).xyz * w.z;
        // Normals transform by the inverse transpose. Applying the position matrix tilts them on nonuniformly scaled
        // instances. Its cofactors suffice here: normalization removes the determinant's magnitude and surface faces the ray.
        let wn = cross(o2w[1], o2w[2]) * gn.x + cross(o2w[2], o2w[0]) * gn.y + cross(o2w[0], o2w[1]) * gn.z;
        if (dot(wn, wn) > 0.0) {{
            o.game_n = normalize(wn);
        }}
    }}
    let t0 = {name}_uv(g, v0);
    let t1 = {name}_uv(g, v1);
    let t2 = {name}_uv(g, v2);
    let uv = t0 * w.x + t1 * w.y + t2 * w.z;
    let dims = vec2<f32>(textureDimensions({name}_tex[m.tex]));
    // Each coordinate's gradient along the world triangle. An area ratio alone underfilters stretched UVs: one axis can
    // cross many texels even when the other is almost constant. Keep this footprint for the material and region maps too.
    let edge1 = cross(p2 - p0, o.n) / area;
    let edge2 = cross(o.n, p1 - p0) / area;
    let du = edge1 * (t1.x - t0.x) + edge2 * (t2.x - t0.x);
    let dv = edge1 * (t1.y - t0.y) + edge2 * (t2.y - t0.y);
    o.footprint = width * vec2<f32>(length(du), length(dv));
    let texels = o.footprint * dims;
    o.texel = {name}_texel(m, uv, log2(max(max(texels.x, texels.y), 1.0))) * 255.0;
    o.vertex = ({name}_colour(v0, g, inst) * w.x + {name}_colour(v1, g, inst) * w.y + {name}_colour(v2, g, inst) * w.z) * 255.0;
    // The GS's modulate: Ct x Cv / 0x80 (bytes), the alpha on the GS scale (0x80 opaque).
    o.px = min(o.texel * o.vertex / 128.0, vec4<f32>(255.0));
    o.uv = uv;
    return o;
}}
"#
    );
    if kind == Kind::Ee {
        s += &format!(
            r#"
// The EE's layers along the ray from `o` along `d` between `near` and `tmax` (`Through`), each pixel as its blend draws it;
// a pixel its alpha test removes (a material whose failing pixels write nothing) is none.
fn {name}_through(o: vec3<f32>, d: vec3<f32>, near: f32, tmax: f32, w0: f32, spread: f32) -> Through {{
    var r: Through;
    r.t = MISS;
    if (near >= tmax) {{
        return r;
    }}
    var rq: ray_query;
    rayQueryInitialize(&rq, {name}_as, RayDesc(RAY_FLAG_NONE, 0xFFu, near, tmax, o, d));
    while (rayQueryProceed(&rq)) {{
        let c = rayQueryGetCandidateIntersection(&rq);
        let g = {name}_geoms[c.instance_custom_data + c.geometry_index];
        let m = {name}_mats[g.mat];
        let p = {name}_shaded(g, m, c.primitive_index, c.barycentrics, c.instance_index, c.object_to_world, w0 + spread * c.t);
        // The GS tests the modulated pixel alpha, including the vertex's fade, not the texture alpha on its own.
        if (floor(p.px.a) < m.aref) {{
            continue;
        }}
        let a = clamp(p.px.a / 128.0, 0.0, 1.0);
        if (m.mode >= 3u) {{
            r.add = r.add + p.px.rgb / 255.0 * select(m.factor, a, m.mode == 3u);
        }} else if (c.t < r.t) {{
            r.t = c.t;
            r.layer = vec4<f32>(p.px.rgb / 255.0, select(m.factor, a, m.mode == 1u));
        }}
    }}
    return r;
}}
"#
        );
    } else {
        s += &format!(
            r#"
// The nearest surface from `o` along `d` between `tmin` and `tmax` (with `flags`: the first confirmed), each alpha-tested
// candidate kept only where its filtered alpha passes its AREF. Testing level 0 while shading a coarser mip made the eye's
// rays find holes inside minified leaves the rasterizer draws solid (and solid edges where it draws holes).
fn {name}_trace(o: vec3<f32>, d: vec3<f32>, tmin: f32, tmax: f32, flags: u32, w0: f32, spread: f32) -> RayIntersection {{
    var rq: ray_query;
    rayQueryInitialize(&rq, {name}_as, RayDesc(flags, 0xFFu, tmin, tmax, o, d));
    while (rayQueryProceed(&rq)) {{
        let c = rayQueryGetCandidateIntersection(&rq);
        let g = {name}_geoms[c.instance_custom_data + c.geometry_index];
        let m = {name}_mats[g.mat];
        // Filtering cannot make alpha smaller than the minimum of all mip texels. The triangle's actual vertex colours
        // include the instance/frame's fade. Leave a whole alpha byte of margin for floating-point interpolation.
        // Ambiguous coverage (including cut-outs and fades at the threshold) still takes the original filtered test.
        if (m.min_alpha > 0u) {{
            let a0 = {name}_colour({name}_vertex(g, c.primitive_index, 0u), g, c.instance_index).a;
            let a1 = {name}_colour({name}_vertex(g, c.primitive_index, 1u), g, c.instance_index).a;
            let a2 = {name}_colour({name}_vertex(g, c.primitive_index, 2u), g, c.instance_index).a;
            let lower = f32(m.min_alpha) * (min(a0, min(a1, a2)) * 255.0) / 128.0;
            if (floor(lower) >= m.aref + 1.0) {{
                rayQueryConfirmIntersection(&rq);
                continue;
            }}
        }}
        let p = {name}_shaded(g, m, c.primitive_index, c.barycentrics, c.instance_index, c.object_to_world, w0 + spread * c.t);
        if (floor(p.px.a) >= m.aref) {{
            rayQueryConfirmIntersection(&rq);
        }}
    }}
    return rayQueryGetCommittedIntersection(&rq);
}}

fn {name}_t(o: vec3<f32>, d: vec3<f32>, tmin: f32, tmax: f32) -> f32 {{
    let h = {name}_trace(o, d, tmin, tmax, RAY_FLAG_NONE, 0.0, 0.0);
    return select(MISS, h.t, h.kind != RAY_QUERY_INTERSECTION_NONE);
}}

fn {name}_any(o: vec3<f32>, d: vec3<f32>, tmax: f32) -> bool {{
    return {name}_trace(o, d, 0.0, tmax, RAY_FLAG_TERMINATE_ON_FIRST_HIT, 0.0, 0.0).kind != RAY_QUERY_INTERSECTION_NONE;
}}

// The surface `h` (from `o` along `d`) as the game draws it, its texture at the level of a ray `w0 + spread x t` wide there.
fn {name}_surface(h: RayIntersection, o: vec3<f32>, d: vec3<f32>, w0: f32, spread: f32) -> Surface {{
    let g = {name}_geoms[h.instance_custom_data + h.geometry_index];
    let m = {name}_mats[g.mat];
    let p = {name}_shaded(g, m, h.primitive_index, h.barycentrics, h.instance_index, h.object_to_world, w0 + spread * h.t);
    var n = select(p.n, p.game_n, dot(p.game_n, p.game_n) > 0.0);
    if (dot(n, d) > 0.0) {{
        n = -n;
    }}
    var s: Surface;
    s.hit = 1u;
    s.t = h.t;
    s.p = o + d * h.t;
    s.n = n;
    s.colour = p.px.rgb / 255.0;
    s.texel = p.texel.rgb / 255.0;
    s.alpha = clamp(p.px.a / 128.0, 0.0, 1.0);
    s.vertex = p.vertex.rgb / 255.0;
    s.shine = m.shine * clamp(p.px.a / 128.0, 0.0, 1.0);
    // The game's own chrome reflects at the pass's metal F0; a Rift Apart surface by its material's gloss there (its map's
    // blue; a surface of regions' materials by its region map), its F0 and its metalness.
    s.f0 = -1.0;
    s.metal = 1.0;
    if (m.rift != 0u) {{
        var w = vec4<f32>(1.0, 0.0, 0.0, 0.0);
        if (m.rift == 2u) {{
            let texels = p.footprint * vec2<f32>(textureDimensions({name}_tex[m.region]));
            w = textureSampleLevel({name}_tex[m.region], {name}_smp, p.uv, log2(max(max(texels.x, texels.y), 1.0)));
        }}
        let tiles = array<vec2<f32>, 4>(m.tiles01.xy, m.tiles01.zw, m.tiles23.xy, m.tiles23.zw);
        var gloss = 0.0;
        for (var k = 0u; k < 4u; k++) {{
            if (w[k] > 0.0 && m.maps[k] != NO_MAP) {{
                let texels = p.footprint * abs(tiles[k]) * vec2<f32>(textureDimensions({name}_tex[m.maps[k]]));
                let lod = max(RIFT_GLOSS_LOD, log2(max(max(texels.x, texels.y), 1.0)));
                gloss += w[k] * textureSampleLevel({name}_tex[m.maps[k]], {name}_smp, p.uv * tiles[k], lod).b;
            }}
        }}
        s.shine = max(s.shine, gloss);
        s.f0 = dot(w, m.f0);
        s.metal = dot(w, m.metal);
        if (m.rift == 3u) {{
            let texels=p.footprint*vec2<f32>(textureDimensions({name}_tex[m.maps.y]));
            let orm=textureSampleLevel({name}_tex[m.maps.y],{name}_smp,p.uv,log2(max(max(texels.x,texels.y),1.0)));
            s.shine=max(m.shine,1.0-orm.g);
            s.metal=orm.b;
            s.f0=mix(0.04,0.6,orm.b);
        }}
    }}
    return s;
}}
"#
        );
    }
    s
}

/// The ray-traced shadows' shade over the picture (`RtFrame::apply`), after the scene library (`scene`).
const SHADE: &str = r#"
struct U {
    r: vec4<f32>, u: vec4<f32>, f: vec4<f32>, eye: vec4<f32>,
    // The camera's tangents and the picture's size.
    tan: vec4<f32>,
    // The depth buffer's near and far planes, the darkness, the shadows' reach.
    nf: vec4<f32>,
    // The fog's near and far view depths and F at them.
    fog: vec4<f32>,
    // The direction towards the key light.
    to_light: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var depth: texture_depth_2d;
@group(0) @binding(2) var mask: texture_2d<f32>;

@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

fn view_z(d: f32) -> f32 {
    return u.nf.x * u.nf.y / (u.nf.x + d * (u.nf.y - u.nf.x));
}

fn load_d(p: vec2<i32>) -> f32 {
    return textureLoad(depth, clamp(p, vec2<i32>(0), vec2<i32>(u.tan.zw) - vec2<i32>(1)), 0);
}

// The world position of the pixel at `px` with depth `d` (`native::Camera::matrix` inverted).
fn world(px: vec2<f32>, d: f32) -> vec3<f32> {
    let z = view_z(d);
    let ndc = vec2<f32>(px.x / u.tan.z * 2.0 - 1.0, 1.0 - px.y / u.tan.w * 2.0);
    return u.eye.xyz + u.r.xyz * (ndc.x * u.tan.x * z) + u.u.xyz * (ndc.y * u.tan.y * z) + u.f.xyz * z;
}

fn world_at(q: vec2<i32>) -> vec3<f32> {
    return world(vec2<f32>(q) + vec2<f32>(0.5), load_d(q));
}

fn shade(at: vec2<f32>) -> f32 {
    let q = vec2<i32>(at.xy);
    let d = load_d(q);
    if (d <= 0.0) {
        return 1.0;
    }
    let p = world(at.xy, d);
    // UYA_RT_CHECK: the structures against the picture. The eye's ray through the pixel meets what the depth buffer holds
    // (1), something nearer that the picture does not show (0.5), or nothing: a surface the structures lack (0).
    if (u.to_light.w > 0.5) {
        let dist = length(p - u.eye.xyz);
        let dir = (p - u.eye.xyz) / dist;
        let foot = 3.0 * dist * 2.0 * u.tan.y / u.tan.w + 0.02;
        let t = scene_t(u.eye.xyz, dir, dist + foot);
        return select(select(1.0, 0.5, t < dist - foot), 0.0, t >= MISS);
    }
    // The surface's normal from the nearer neighbour on each axis, turned to the eye.
    let pl = world_at(q - vec2<i32>(1, 0));
    let pr = world_at(q + vec2<i32>(1, 0));
    let pu = world_at(q - vec2<i32>(0, 1));
    let pd = world_at(q + vec2<i32>(0, 1));
    let dx = select(pr - p, p - pl, length(pl - p) < length(pr - p));
    let dy = select(pd - p, p - pu, length(pu - p) < length(pd - p));
    var n = normalize(cross(dx, dy));
    if (dot(n, u.eye.xyz - p) < 0.0) {
        n = -n;
    }
    let facing = dot(n, u.to_light.xyz);
    if (facing <= 0.0) {
        return 1.0;
    }
    // The ray starts off the surface by a pixel and a half's width at the pixel's distance (the point and its normal come from
    // the depth of it and its neighbours, good to about a pixel's width: from 0.02 units off, rays from creases and from faces
    // the light grazes struck their own surface in lines and dots), so no surface shades itself; the maps' shade is held off
    // the same way by a texel and a half and its slope bias.
    let vz = dot(p - u.eye.xyz, u.f.xyz);
    let o = p + n * (0.01 + 1.5 * vz * 2.0 * u.tan.y / u.tan.w);
    let reach = u.nf.w;
    // The nearest caster: a shadow reaches `REACH` from what casts it, fading over its second half (a roof high over a floor
    // casts none).
    var lit = 0.0;
    if (!scene_any(o, u.to_light.xyz, 0.5 * reach)) {
        lit = smoothstep(0.5 * reach, reach, scene_t_from(o, u.to_light.xyz, 0.5 * reach, reach));
    }
    // The share of the pixel that is not fog, as the occlusion's.
    let f = u.fog.z + (u.fog.w - u.fog.z) * (vz - u.fog.x) / (u.fog.y - u.fog.x);
    let clear = clamp(f, min(u.fog.z, u.fog.w), max(u.fog.z, u.fog.w)) / 255.0;
    // A face the light barely reaches gets little of it to lose, and on the game's faceted rocks a quad the light grazes is
    // struck by the next facet of its own rock (the shadow terminator: a hard-edged dark quad where the game's smooth lighting
    // is already dim): the shade grows with the face's turn to the light up to 20 degrees (the maps' slope bias hides it).
    return 1.0 - u.nf.z * (1.0 - lit) * smoothstep(0.0, 0.35, facing) * clear;
}

@fragment fn fs_static(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let s = shade(at.xy);
    return vec4<f32>(s, s, s, 1.0);
}

@fragment fn fs_mobies(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    if (textureLoad(mask, vec2<i32>(at.xy), 0).r > 0.5) {
        return vec4<f32>(1.0);
    }
    let s = shade(at.xy);
    return vec4<f32>(s, s, s, 1.0);
}
"#;

/// The enables every ray-traced pass's module starts with.
pub(crate) const ENABLES: &str = "enable wgpu_ray_query;\nenable wgpu_binding_array;\n";

/// The places the mobies' bottom level is built from: each moby vertex (`objects::ObjVertex`, 8 words) `view.lerp.x` of the way
/// back to where the frame before drew it (3 words), as `vs_moby` draws it.
const LERP_SHADER: &str = r#"
struct View { m: mat4x4<f32>, sky: mat4x4<f32>, eye: vec4<f32>, forward: vec4<f32>, fog_colour: vec4<f32>, fog: vec4<f32>, lerp: vec4<f32> };
@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> now: array<f32>;
@group(0) @binding(2) var<storage, read> before: array<f32>;
@group(0) @binding(3) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64) fn places(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if (3u * i >= arrayLength(&out)) {
        return;
    }
    for (var k = 0u; k < 3u; k = k + 1u) {
        let p = now[8u * i + k];
        out[3u * i + k] = p + (before[3u * i + k] - p) * view.lerp.x;
    }
}
"#;

/// The terrain's part of the static world's vertex buffers (`World::build`): each terrain vertex's place (3 words) and its texture
/// coordinates and colour (3 words, then no normal of the game's: 0) from the level's terrain vertices (`native::Vertex`, 6 words: the place, U, V, the colour),
/// and the terrain's corners copied from the level's terrain indices.
const GATHER_SHADER: &str = r#"
@group(0) @binding(0) var<storage, read> vtx: array<u32>;
@group(0) @binding(1) var<storage, read> idx: array<u32>;
@group(0) @binding(2) var<storage, read_write> pos: array<u32>;
@group(0) @binding(3) var<storage, read_write> attr: array<u32>;
@group(0) @binding(4) var<storage, read_write> corners: array<u32>;

@compute @workgroup_size(64) fn gather(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) n: vec3<u32>) {
    let k = id.x + id.y * n.x * 64u;
    if (k < arrayLength(&vtx) / 6u) {
        for (var j = 0u; j < 3u; j = j + 1u) {
            pos[3u * k + j] = vtx[6u * k + j];
            attr[4u * k + j] = vtx[6u * k + 3u + j];
        }
        attr[4u * k + 3u] = 0u;
    }
    if (k < arrayLength(&idx)) {
        corners[k] = idx[k];
    }
}
"#;

/// The shade's per-picture uniform (`U` of `SHADE`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ApplyU {
    r: [f32; 4],
    u: [f32; 4],
    f: [f32; 4],
    eye: [f32; 4],
    tan: [f32; 4],
    nf: [f32; 4],
    fog: [f32; 4],
    to_light: [f32; 4],
}

/// A geometry (`Geom` of the scene library): its first corner, the vertex its corners count from, its material, its flags.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Geom {
    first: u32,
    base: u32,
    mat: u32,
    flags: u32,
}

/// A material (`Mat` of the scene library).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MatRt {
    tex: u32,
    aref: f32,
    clamp: u32,
    shine: f32,
    mode: u32,
    factor: f32,
    pad: [u32; 2],
    /// Its Rift Apart surface (the scene library's `Mat`): 0 none, 1 one material, 2 regions; the region map's texture; per
    /// material its map's texture (`NO_MAP`: none), its tiles (u, v), its F0 and its metalness.
    rift: u32,
    region: u32,
    pad2: [u32; 2],
    maps: [u32; 4],
    tiles: [f32; 8],
    f0: [f32; 4],
    metal: [f32; 4],
}

/// No Rift Apart map (the scene library's `NO_MAP`).
const NO_MAP: u32 = u32::MAX;

/// The device features the ray tracing takes: ray queries, and arrays of textures indexed per hit.
pub(crate) fn features() -> wgpu::Features {
    wgpu::Features::EXPERIMENTAL_RAY_QUERY | wgpu::Features::TEXTURE_BINDING_ARRAY | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING | wgpu::Features::PARTIALLY_BOUND_BINDING_ARRAY
}

/// Whether `adapter` traces rays: the features, and room for two top levels and two texture arrays in a pass.
pub(crate) fn supported(adapter: &wgpu::Adapter) -> bool {
    let l = adapter.limits();
    adapter.features().contains(features()) && l.max_acceleration_structures_per_shader_stage >= 2 && l.max_binding_array_elements_per_shader_stage >= 2 * TEXTURES && l.max_sampled_textures_per_shader_stage >= 2 * TEXTURES + 4 && l.max_tlas_instance_count > 0 && l.max_storage_buffers_per_shader_stage >= 16
}

/// `base` with the limits the ray tracing takes, at what `adapter` offers.
pub(crate) fn limits(adapter: &wgpu::Adapter, base: wgpu::Limits) -> wgpu::Limits {
    let a = adapter.limits();
    wgpu::Limits {
        max_blas_primitive_count: a.max_blas_primitive_count,
        max_blas_geometry_count: a.max_blas_geometry_count,
        max_tlas_instance_count: a.max_tlas_instance_count,
        max_acceleration_structures_per_shader_stage: a.max_acceleration_structures_per_shader_stage,
        max_binding_array_elements_per_shader_stage: a.max_binding_array_elements_per_shader_stage,
        max_binding_array_sampler_elements_per_shader_stage: a.max_binding_array_sampler_elements_per_shader_stage,
        max_samplers_per_shader_stage: a.max_samplers_per_shader_stage,
        max_sampled_textures_per_shader_stage: a.max_sampled_textures_per_shader_stage,
        max_storage_buffers_per_shader_stage: a.max_storage_buffers_per_shader_stage.max(base.max_storage_buffers_per_shader_stage),
        ..base
    }
}

/// Whether `uya native` times the frame on the GPU (`UYA_RT_TIMES`, `GpuTimes`): the device then asks for timestamps.
pub(crate) fn times_wanted() -> bool {
    std::env::var_os("UYA_RT_TIMES").is_some()
}

/// The device features the GPU's timestamps take (`GpuTimes`).
pub(crate) fn times_features() -> wgpu::Features {
    wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS
}

/// The GPU's timestamps of a frame (`UYA_RT_TIMES`), by pair: the mobies' casters (`RtFrame::casters`), the shade over the
/// picture and over the mobies' pixels (`RtFrame::apply`), the frame's first and last command (`native.rs`), the ray-traced
/// lighting over the picture and over the mobies' pixels, the reflections on the shiny surfaces and on the water
/// (`rtlight.rs`); read after the frame is submitted (`report`).
pub(crate) struct GpuTimes {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    /// Nanoseconds a tick.
    period: f32,
}

/// The timestamp pairs (`GpuTimes`).
pub(crate) const T_CASTERS: u32 = 0;
pub(crate) const T_SHADE: u32 = 2;
pub(crate) const T_SHADE_MOBIES: u32 = 4;
pub(crate) const T_FRAME: u32 = 6;
pub(crate) const T_LIGHT: u32 = 8;
pub(crate) const T_LIGHT_MOBIES: u32 = 10;
pub(crate) const T_GLOSS: u32 = 12;
pub(crate) const T_WATER: u32 = 14;
const STAMPS: u32 = 16;

impl GpuTimes {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> GpuTimes {
        let set = device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("GPU times"), ty: wgpu::QueryType::Timestamp, count: STAMPS });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor { label: Some("GPU times"), size: 8 * STAMPS as u64, usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
        let read = device.create_buffer(&wgpu::BufferDescriptor { label: Some("GPU times read"), size: 8 * STAMPS as u64, usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        GpuTimes { set, resolve, read, period: queue.get_timestamp_period() }
    }

    /// Timestamp `k` written by `enc` where it stands.
    pub fn stamp(&self, enc: &mut wgpu::CommandEncoder, k: u32) {
        enc.write_timestamp(&self.set, k);
    }

    /// A pass's timestamps `pair` and `pair + 1` at its beginning and end.
    pub fn pass(&self, pair: u32) -> wgpu::RenderPassTimestampWrites<'_> {
        wgpu::RenderPassTimestampWrites { query_set: &self.set, beginning_of_pass_write_index: Some(pair), end_of_pass_write_index: Some(pair + 1) }
    }

    /// The frame's GPU times, after its submission: the whole frame and each ray-traced step it ran (`ran`, by pair).
    pub fn report(&self, device: &wgpu::Device, queue: &wgpu::Queue, ran: &[(u32, &str)]) -> Result<(), String> {
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("GPU times") });
        enc.resolve_query_set(&self.set, 0..STAMPS, &self.resolve, 0);
        enc.copy_buffer_to_buffer(&self.resolve, 0, &self.read, 0, 8 * STAMPS as u64);
        queue.submit([enc.finish()]);
        let slice = self.read.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| ());
        device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("native: GPU wait: {e}"))?;
        let t: Vec<u64> = bytemuck::cast_slice(&slice.get_mapped_range().map_err(|e| format!("native: GPU times: {e}"))?).to_vec();
        self.read.unmap();
        let ms = |a: u32| t[a as usize + 1].wrapping_sub(t[a as usize]) as f64 * self.period as f64 / 1e6;
        let mut line = format!("native: GPU times: the frame {:.3} ms", ms(T_FRAME));
        for (pair, what) in ran {
            line += &format!(", {what} {:.3} ms", ms(*pair));
        }
        println!("{line}");
        Ok(())
    }
}

/// The ray tracing's pipelines (`Native::open`, when the device traces rays): the shadows' (`RtFrame::apply`), the mobies'
/// places, the terrain's gather, and the ray-traced lighting's and reflections' (`rtlight::Passes`).
pub(crate) struct RayTracing {
    frame_layout: wgpu::BindGroupLayout,
    world_layout: wgpu::BindGroupLayout,
    mobies_layout: wgpu::BindGroupLayout,
    ee_layout: wgpu::BindGroupLayout,
    /// At `mobies` (0: none to trace, 1: traced): the shade over every pixel and over the mobies' pixels.
    apply: Vec<[wgpu::RenderPipeline; 2]>,
    places: wgpu::ComputePipeline,
    places_layout: wgpu::BindGroupLayout,
    gather: wgpu::ComputePipeline,
    gather_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pub(crate) passes: crate::rtlight::Passes,
    /// With `UYA_RT_TIMES`: the GPU's times of each frame.
    pub times: Option<GpuTimes>,
}

pub(crate) fn storage(binding: u32, visibility: wgpu::ShaderStages, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None }, count: None }
}

pub(crate) fn uniform(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }
}

/// A geometry of `corners` indices into `vertices` vertices, opaque or alpha-tested.
fn tris(vertices: u32, corners: u32, opaque: bool) -> wgpu::BlasTriangleGeometrySizeDescriptor {
    wgpu::BlasTriangleGeometrySizeDescriptor {
        vertex_format: wgpu::VertexFormat::Float32x3,
        vertex_count: vertices,
        index_format: Some(wgpu::IndexFormat::Uint32),
        index_count: Some(corners),
        flags: if opaque { wgpu::AccelerationStructureGeometryFlags::OPAQUE } else { wgpu::AccelerationStructureGeometryFlags::empty() },
    }
}

/// A scene group's layout (`scene`): the static world's (with its instances' colours), the mobies' or the EE layers' (with
/// the effects' vertices).
fn scene_layout(device: &wgpu::Device, kind: Kind) -> wgpu::BindGroupLayout {
    let v = wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE;
    let mut entries = vec![
        wgpu::BindGroupLayoutEntry { binding: 0, visibility: v, ty: wgpu::BindingType::AccelerationStructure { vertex_return: false }, count: None },
        storage(1, v, true),
        storage(2, v, true),
        storage(3, v, true),
        storage(4, v, true),
        storage(5, v, true),
        wgpu::BindGroupLayoutEntry {
            binding: 6,
            visibility: v,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: std::num::NonZeroU32::new(TEXTURES),
        },
        wgpu::BindGroupLayoutEntry { binding: 7, visibility: v, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
    ];
    match kind {
        Kind::World => entries.extend([storage(8, v, true), storage(9, v, true)]),
        Kind::Ee => entries.push(storage(8, v, true)),
        Kind::Mobies => (),
    }
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("ray-traced scene"), entries: &entries })
}

impl RayTracing {
    pub fn new(device: &wgpu::Device, times: Option<GpuTimes>) -> RayTracing {
        let f = wgpu::ShaderStages::FRAGMENT;
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ray-traced shadows"),
            entries: &[
                uniform(0, f),
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: f, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: f, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
            ],
        });
        let world_layout = scene_layout(device, Kind::World);
        let mobies_layout = scene_layout(device, Kind::Mobies);
        let ee_layout = scene_layout(device, Kind::Ee);
        // The picture's colour times the shade; its alpha kept (the frame's alpha, which the glow reads), as the maps' shade.
        let multiply = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::Src, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
        };
        let mut apply = Vec::with_capacity(2);
        for mobies in [false, true] {
            let source = format!("{ENABLES}{}{SHADE}", scene(mobies, false));
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("ray-traced shadows"), source: wgpu::ShaderSource::Wgsl(source.into()) });
            let mut groups = vec![Some(&frame_layout), Some(&world_layout)];
            if mobies {
                groups.push(Some(&mobies_layout));
            }
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("ray-traced shadows"), bind_group_layouts: &groups, immediate_size: 0 });
            let make = |entry: &str| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(entry),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                    fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some(entry), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: Some(multiply), write_mask: wgpu::ColorWrites::ALL })] }),
                    primitive: Default::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: crate::pipelines::cache(),
                })
            };
            apply.push([make("fs_static"), make("fs_mobies")]);
        }
        let c = wgpu::ShaderStages::COMPUTE;
        let compute = |label: &str, source: &str, entry: &str, layout: &wgpu::BindGroupLayout| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(source.into()) });
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(label), bind_group_layouts: &[Some(layout)], immediate_size: 0 });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some(label), layout: Some(&pl), module: &module, entry_point: Some(entry), compilation_options: Default::default(), cache: crate::pipelines::cache() })
        };
        let places_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("moby places"), entries: &[uniform(0, c), storage(1, c, true), storage(2, c, true), storage(3, c, false)] });
        let gather_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("terrain vertices"), entries: &[storage(0, c, true), storage(1, c, true), storage(2, c, false), storage(3, c, false), storage(4, c, false)] });
        let places = compute("moby places", LERP_SHADER, "places", &places_layout);
        let gather = compute("terrain vertices", GATHER_SHADER, "gather", &gather_layout);
        // The textures' sampler: bilinear, between levels too (a hit's level from the ray's width); the alpha test reads level 0 as
        // the casters' (`shadows.rs`); the clamps made in the shader.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ray-traced textures"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let passes = crate::rtlight::Passes::new(device, &world_layout, &mobies_layout, &ee_layout);
        RayTracing { frame_layout, world_layout, mobies_layout, ee_layout, apply, places, places_layout, gather, gather_layout, sampler, passes, times }
    }
}

/// What a group's surfaces are drawn with: each geometry (`Geom`), each material once (`MatRt`) and the materials' textures.
#[derive(Default)]
struct Shade {
    geoms: Vec<Geom>,
    mats: Vec<MatRt>,
    views: Vec<wgpu::TextureView>,
    /// The Rift Apart maps already among `views`, by their key (`texnames::material_path`'s).
    rift_views: HashMap<String, u32>,
}

impl Shade {
    /// A material of `view` with AREF `aref`, clamps `cs`, `ct`, the game's shine over it, how its pixels lie in what a ray
    /// sees (`mode`, `factor`: `MODE_SURFACE` and 1 for the static world and the mobies), and its Rift Apart surface (`rift`:
    /// its maps, region map, F0s and metalness, which its reflections take); its index.
    #[allow(clippy::too_many_arguments)]
    fn mat(&mut self, view: wgpu::TextureView, cs: bool, ct: bool, aref: u16, shine: bool, mode: u32, factor: f32, min_alpha: u8, rift: Option<&crate::texnames::RiftMat>) -> Result<u32, String> {
        let tex = self.view(view)?;
        let mut m = MatRt { tex, aref: aref as f32, clamp: cs as u32 | (ct as u32) << 1, shine: shine as u32 as f32, mode, factor, pad: [min_alpha as u32, 0], rift: 0, region: NO_MAP, pad2: [0; 2], maps: [NO_MAP; 4], tiles: [0.0; 8], f0: [0.0; 4], metal: [0.0; 4] };
        if let Some(r) = rift.filter(|r| r.has_material()) {
            let (slots, region) = r.rt();
            for (k, (map, tiles, f0, metal)) in slots.into_iter().enumerate().take(4) {
                if let Some((key, v)) = map {
                    m.maps[k] = self.rift_view(key, v)?;
                }
                m.tiles[2 * k] = tiles[0];
                m.tiles[2 * k + 1] = tiles[1];
                m.f0[k] = f0;
                m.metal[k] = metal;
            }
            m.rift = 1;
            if let Some((key, v)) = region {
                m.region = self.rift_view(key, v)?;
                m.rift = 2;
            }
            if r.is_pbr() { m.rift=3; }
        }
        self.mats.push(m);
        Ok(self.mats.len() as u32 - 1)
    }

    /// `view` added to the group's textures; its index.
    fn view(&mut self, view: wgpu::TextureView) -> Result<u32, String> {
        if self.views.len() as u32 >= TEXTURES {
            return Err(format!("native: ray tracing: more than {TEXTURES} textures"));
        }
        self.views.push(view);
        Ok(self.views.len() as u32 - 1)
    }

    /// A Rift Apart map's index among the group's textures, added the first time its key comes.
    fn rift_view(&mut self, key: String, view: wgpu::TextureView) -> Result<u32, String> {
        if let Some(&i) = self.rift_views.get(&key) {
            return Ok(i);
        }
        let i = self.view(view)?;
        self.rift_views.insert(key, i);
        Ok(i)
    }

    /// The geometries and materials on the GPU.
    fn upload(&self, device: &wgpu::Device) -> Result<(wgpu::Buffer, wgpu::Buffer), String> {
        if self.geoms.is_empty() || self.mats.is_empty() {
            return Err("native: ray tracing: a group with no geometry".into());
        }
        let geoms = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("ray-traced geometries"), contents: bytemuck::cast_slice(&self.geoms), usage: wgpu::BufferUsages::STORAGE });
        let mats = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("ray-traced materials"), contents: bytemuck::cast_slice(&self.mats), usage: wgpu::BufferUsages::STORAGE });
        Ok((geoms, mats))
    }
}

/// What the static world's structures are built from (`native.rs`): the terrain in the terrain buffer (its number, the level's
/// vertex and index buffers, each of its draws' setting, first index and indices), each terrain setting's bind group's "can
/// fail" and the texture it samples with its clamps, the terrain's AREF, the ties and the shrubs.
pub(crate) struct WorldIn<'a> {
    pub terrain: u32,
    pub vertices: &'a wgpu::Buffer,
    pub indices: &'a wgpu::Buffer,
    pub draws: Vec<(usize, u32, u32)>,
    pub keys: &'a [(wgpu::BindGroup, bool)],
    pub views: &'a [(wgpu::TextureView, bool, bool, Option<crate::texnames::RiftMat>)],
    pub aref: u8,
    pub ties: Option<(&'a crate::objects::Ties, &'a crate::objects::TieGlobals)>,
    pub shrubs: Option<(&'a crate::objects::Shrubs, &'a crate::objects::ShrubGlobals)>,
    /// Whether the frame's tie programs and shrub programs ran (`native::render_frame`): a layer that did not run is not read from
    /// RAM, which may hold anything then (a loading screen's frames, the next level's data arriving over the instances).
    pub ties_ran: bool,
    pub shrubs_ran: bool,
}

/// The frame's mobies the rays meet (`native.rs`): its moby vertices and their buffer, where the frame before drew each, the
/// picture's view buffer (its `lerp.x`), and the opaque draws into the main frame (their materials, setting, first vertex and
/// vertices).
pub(crate) struct MobiesIn<'a> {
    pub vertices: &'a [crate::objects::ObjVertex],
    pub now: Option<&'a wgpu::Buffer>,
    pub before: Option<&'a wgpu::Buffer>,
    pub view: &'a wgpu::Buffer,
    pub runs: Vec<(&'a crate::objects::Materials, crate::objects::Key, u32, u32)>,
}

/// The frame's EE layers the rays see (`native.rs`, only with the ray-traced lighting or reflections): the frame's EE vertex
/// buffer and effect vertex buffer as it draws them (`objects::ObjVertex`, in the world) with their vertex counts, and the draws
/// (their materials, setting, whether from the effects' buffer, first vertex and vertices: the EE's triangles drawn into the main
/// frame in the world, not its 2D, and program 6's effects).
pub(crate) struct EeIn<'a> {
    pub ee: Option<&'a wgpu::Buffer>,
    pub ee_vertices: u32,
    pub fx: Option<&'a wgpu::Buffer>,
    pub fx_vertices: u32,
    pub runs: Vec<(&'a crate::objects::Materials, crate::objects::Key, bool, u32, u32)>,
}

/// A level's ray tracing (`native::Gpu`): the key light and the static world's structures.
pub(crate) struct LevelRt {
    /// The key light's direction of travel; None: the level's sets light nothing (`shadows::key_light`).
    key: Option<[f32; 3]>,
    world: Option<World>,
    /// The ties' and the shrubs' instance and matrix arrays as the top level was last placed from (`objects::Ties::rt_state`):
    /// a layer's instances are read again only when its bytes change (most frames they do not).
    tie_state: Vec<u8>,
    shrub_state: Vec<u8>,
    /// The ties' and the shrubs' colours as last read (`frame`), each layer's from the last frame its programs ran.
    tie_colours: Vec<u32>,
    shrub_colours: Vec<u32>,
}

/// The static world's bottom levels and what its surfaces are drawn with, built once a terrain chunk: the terrain's, each tie
/// class's at each LOD and each shrub class's (with its first geometry; None: no triangle), the geometries, materials and
/// textures, and the vertex buffers (`scene`'s bindings 3 to 5: the terrain's vertices and corners first, then the classes');
/// and the top level placing them with its group, made again when a tie's LOD or a shrub's place changes (with the instances it
/// was made from), and its colours.
struct World {
    terrain: u32,
    terrain_blas: Option<wgpu::Blas>,
    ties: Vec<Vec<Option<(wgpu::Blas, u32)>>>,
    shrubs: Vec<Option<(wgpu::Blas, u32)>>,
    geoms: wgpu::Buffer,
    mats: wgpu::Buffer,
    views: Vec<wgpu::TextureView>,
    idx: wgpu::Buffer,
    pos: wgpu::Buffer,
    attr: wgpu::Buffer,
    placed: Option<Placed>,
}

/// The top level as placed (`World::place`): the ties and shrubs it was made from, each instance's first word in the frame's
/// colours (`inst`), the shrubs' blocks' first word there and how many blocks, the colours as last uploaded (`cols`), and the
/// group.
struct Placed {
    ties: Vec<(usize, u8, [f32; 12], u32, u32)>,
    shrubs: Vec<(usize, [f32; 12], u32)>,
    tlas: wgpu::Tlas,
    inst: wgpu::Buffer,
    blocks: u32,
    colours: Vec<u32>,
    cols: wgpu::Buffer,
    group: wgpu::BindGroup,
}

/// The frame's mobies' structures: built in each picture (`RtFrame::casters`) from the picture's places of the frame's vertices
/// (`pos`, written by the places pass), each geometry's first index in `idx`.
struct Actors {
    blas: wgpu::Blas,
    tlas: wgpu::Tlas,
    sizes: Vec<wgpu::BlasTriangleGeometrySizeDescriptor>,
    firsts: Vec<u32>,
    pos: wgpu::Buffer,
    idx: wgpu::Buffer,
    vertices: u32,
    places: wgpu::BindGroup,
    group: wgpu::BindGroup,
}

/// The frame's EE layers' structures (`Layers::new`): built once for the frame on a submission of its own (they lie where the
/// frame put them in the world, so every picture of the frame sees them as they are), with their group (`scene`'s EE group).
struct Layers {
    blas: wgpu::Blas,
    tlas: wgpu::Tlas,
    sizes: Vec<wgpu::BlasTriangleGeometrySizeDescriptor>,
    firsts: Vec<u32>,
    /// Per geometry: whether its vertices are the effects'.
    sources: Vec<bool>,
    ee: wgpu::Buffer,
    fx: wgpu::Buffer,
    idx: wgpu::Buffer,
    group: wgpu::BindGroup,
}

/// What the frame's ray-traced commands draw with (`replay::Cmd::RtCasters`, `Cmd::RtShadows`, and `rtlight.rs`'s): the
/// pipelines, the direction toward the key light (None: the level has none), whether what the rays meet is shaded by it (the
/// shadows option on: the picture's surfaces are), the static world's group, the mobies' structures and the EE's layers'.
pub(crate) struct RtFrame {
    pub(crate) raster: Option<Arc<crate::surface_buffer::Buffers>>,
    pub(crate) rt: Arc<RayTracing>,
    pub(crate) to_light: Option<[f32; 3]>,
    pub(crate) sun: bool,
    pub(crate) world: wgpu::BindGroup,
    actors: Option<Actors>,
    layers: Option<Layers>,
    /// The CPU's ms making the frame's structures (`LevelRt::frame`): the static world's instances (and its top level when they
    /// changed), its colours, the mobies', the EE layers' (`native.rs`'s times line).
    pub(crate) times: [f64; 4],
    /// What the frame's own picture traced of the lighting and the reflections, which the pictures between frames take
    /// (`rtlight::Traced`).
    pub(crate) traced: std::sync::Mutex<crate::rtlight::Traced>,
}

impl Drop for RtFrame {
    /// The frame's traced textures back for the next frames' (`rtlight::Passes::give_back`).
    fn drop(&mut self) {
        if let Ok(t) = self.traced.get_mut() {
            self.rt.passes.give_back(std::mem::take(t));
        }
    }
}

impl LevelRt {
    pub fn new(key: Option<[f32; 3]>) -> LevelRt {
        LevelRt { key, world: None, tie_state: Vec::new(), shrub_state: Vec::new(), tie_colours: Vec::new(), shrub_colours: Vec::new() }
    }

    /// The frame's structures: the static world's bottom levels built at the level's first ray-traced frame and when the terrain
    /// buffer holds another chunk, its top level made again when a tie's LOD or a shrub's place changed and its colours uploaded
    /// again when they changed, the mobies' made for the frame. `sun`: whether what the rays meet is shaded by the key light.
    /// A new top level's build goes into `outbox` (applied in the frames' order before the frame's first picture).
    #[allow(clippy::too_many_arguments)]
    pub fn frame(&mut self, rt: &Arc<RayTracing>, device: &wgpu::Device, queue: &wgpu::Queue, outbox: &mut crate::native::Outbox, ram: &[u8], w: WorldIn<'_>, m: MobiesIn<'_>, e: EeIn<'_>, sun: bool, raster: Option<Arc<crate::surface_buffer::Buffers>>) -> Result<Arc<RtFrame>, String> {
        if self.world.as_ref().is_none_or(|x| x.terrain != w.terrain) {
            self.world = Some(World::build(rt, device, queue, &w)?);
        }
        let t0 = std::time::Instant::now();
        let world = self.world.as_mut().expect("built above");
        // Each layer's instances: read from RAM when its programs ran this frame and its arrays changed since they were placed;
        // the ones placed before when it did not run (as the frame's own tie and shrub passes drop a layer that did not run: the
        // frames of a loading screen leaving level 28 left the tie instances to the next level's data, one tie at LOD 23 of 3).
        let placed_before = world.placed.as_ref();
        let ties = match w.ties {
            Some((t, g)) if w.ties_ran => {
                let state = t.rt_state(ram, g)?.concat();
                match placed_before {
                    Some(p) if state == self.tie_state => p.ties.clone(),
                    _ => {
                        let ties = t.rt_instances(ram, g)?;
                        self.tie_state = state;
                        ties
                    }
                }
            }
            Some(_) => placed_before.map(|p| p.ties.clone()).unwrap_or_default(),
            None => Vec::new(),
        };
        let shrubs = match w.shrubs {
            Some((s, g)) if w.shrubs_ran => {
                let state = s.rt_state(ram, g)?.concat();
                match placed_before {
                    Some(p) if state == self.shrub_state => p.shrubs.clone(),
                    _ => {
                        let shrubs = s.rt_instances(ram, g)?;
                        self.shrub_state = state;
                        shrubs
                    }
                }
            }
            Some(_) => placed_before.map(|p| p.shrubs.clone()).unwrap_or_default(),
            None => Vec::new(),
        };
        if world.placed.as_ref().is_none_or(|p| p.ties != ties || p.shrubs != shrubs) {
            world.place(rt, device, outbox, ties, shrubs)?;
        }
        let t1 = std::time::Instant::now();
        // The frame's colours: each tie's colour buffer as RAM holds it, then the shrubs' colour blocks; a layer that did not run
        // keeps the colours it last read (its instances, and so its words, are the ones placed then).
        let placed = world.placed.as_ref().expect("placed above");
        if w.ties_ran {
            self.tie_colours.clear();
            for &(_, _, _, at, words) in &placed.ties {
                let b = ram.get(at as usize..at as usize + 4 * words as usize).ok_or("native: ray tracing: tie colours past RAM")?;
                self.tie_colours.extend(b.chunks_exact(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])));
            }
        }
        if let Some((s, g)) = w.shrubs
            && w.shrubs_ran
        {
            self.shrub_colours.clear();
            s.rt_colours(ram, g, placed.blocks, &mut self.shrub_colours)?;
        }
        let colours = [self.tie_colours.as_slice(), self.shrub_colours.as_slice()].concat();
        if colours != placed.colours {
            world.colour(rt, device, colours)?;
        }
        let group = world.placed.as_ref().expect("placed above").group.clone();
        let t2 = std::time::Instant::now();
        let actors = Actors::new(rt, device, m)?;
        let t3 = std::time::Instant::now();
        let layers = Layers::new(rt, device, e)?;
        let ms = |a: std::time::Instant, b: std::time::Instant| (b - a).as_secs_f64() * 1e3;
        let times = [ms(t0, t1), ms(t1, t2), ms(t2, t3), ms(t3, std::time::Instant::now())];
        Ok(Arc::new(RtFrame { raster, rt: rt.clone(), to_light: self.key.map(|d| [-d[0], -d[1], -d[2]]), sun, world: group, actors, layers, times, traced: Default::default() }))
    }
}

/// One class's bottom level (`World::build`): its geometries' sizes and first corners, its first vertex, its first geometry in
/// the world's.
struct Mesh {
    sizes: Vec<wgpu::BlasTriangleGeometrySizeDescriptor>,
    firsts: Vec<u32>,
    base: u32,
    geom: u32,
}

/// The tie and shrub classes' vertices (place, then U and V as bits, the colour word and the game's normal) and corners, each material made once by
/// its source (1 ties, 2 shrubs), material set, setting and shine, and where the classes' vertices and corners start in the
/// world's (after the terrain's).
struct Classes {
    pos: Vec<[f32; 3]>,
    attr: Vec<[u32; 4]>,
    idx: Vec<u32>,
    mats: HashMap<(u8, usize, crate::objects::Key, bool), u32>,
    vertex_from: u32,
    corner_from: u32,
}

impl Classes {
    /// `class`'s bottom level (None: no triangle): its opaque runs of each material one geometry, then each alpha-tested run one,
    /// with the materials of its sets (`materials`).
    fn mesh<'m>(&mut self, shade: &mut Shade, class: &crate::objects::RtClass, source: u8, materials: impl Fn(usize) -> &'m crate::objects::Materials) -> Result<Option<Mesh>, String> {
        // The class's geometries: (material, opaque, corners).
        let mut geoms: Vec<(u32, bool, Vec<u32>)> = Vec::new();
        for (set, key, shine, corners) in &class.runs {
            if corners.is_empty() {
                continue;
            }
            let (view, cs, ct, aref, fails, rift) = materials(*set).alpha_test(key)?;
            let m = match self.mats.get(&(source, *set, *key, *shine)) {
                Some(&m) => m,
                None => {
                    let m = shade.mat(view, cs, ct, aref, *shine, MODE_SURFACE, 1.0, materials(*set).minimum_alpha(key), rift.as_ref())?;
                    self.mats.insert((source, *set, *key, *shine), m);
                    m
                }
            };
            match geoms.iter_mut().find(|g| g.0 == m && !fails) {
                Some(g) => g.2.extend(corners),
                None => geoms.push((m, !fails, corners.clone())),
            }
        }
        if geoms.is_empty() {
            return Ok(None);
        }
        let n = class.pos.len() as u32;
        let base = self.pos.len() as u32;
        let mut mesh = Mesh { sizes: Vec::new(), firsts: Vec::new(), base, geom: shade.geoms.len() as u32 };
        self.pos.extend(&class.pos);
        self.attr.extend(class.uv.iter().zip(&class.colour).zip(&class.normal).map(|((uv, &c), &n)| [uv[0].to_bits(), uv[1].to_bits(), c, n]));
        for (mat, opaque, corners) in geoms {
            mesh.firsts.push(self.idx.len() as u32);
            mesh.sizes.push(tris(n, corners.len() as u32, opaque));
            shade.geoms.push(Geom { first: self.corner_from + self.idx.len() as u32, base: self.vertex_from + base, mat, flags: G_INSTANCE });
            self.idx.extend(corners);
        }
        Ok(Some(mesh))
    }
}

impl World {
    /// The bottom levels and what is drawn on them, of the terrain in the terrain buffer and of every tie class (at each LOD) and
    /// shrub class, built on a submission of their own.
    fn build(rt: &RayTracing, device: &wgpu::Device, queue: &wgpu::Queue, w: &WorldIn<'_>) -> Result<World, String> {
        let t0 = std::time::Instant::now();
        let mut shade = Shade::default();
        // The terrain: each draw a geometry of the terrain buffer's vertices and corners, at the same places in the world's.
        let tv = (w.vertices.size() / std::mem::size_of::<crate::native::Vertex>() as u64) as u32;
        let ti = (w.indices.size() / 4) as u32;
        let mut terrain_mats: HashMap<usize, u32> = HashMap::new();
        let mut terrain_sizes = Vec::with_capacity(w.draws.len());
        for &(key, first, count) in &w.draws {
            let fails = w.keys.get(key).ok_or_else(|| format!("native: ray tracing: terrain setting {key} of {}", w.keys.len()))?.1;
            let mat = match terrain_mats.get(&key) {
                Some(&m) => m,
                None => {
                    let (v, cs, ct, rift) = w.views.get(key).ok_or_else(|| format!("native: ray tracing: terrain setting {key} has no texture"))?;
                    let m = shade.mat(v.clone(), *cs, *ct, w.aref as u16, false, MODE_SURFACE, 1.0, 0, rift.as_ref())?;
                    terrain_mats.insert(key, m);
                    m
                }
            };
            shade.geoms.push(Geom { first, base: 0, mat, flags: 0 });
            terrain_sizes.push(tris(tv, count, !fails));
        }
        // The tie and shrub classes: their vertices and corners after the terrain's (`Classes`).
        let mut o = Classes { pos: Vec::new(), attr: Vec::new(), idx: Vec::new(), mats: HashMap::new(), vertex_from: tv, corner_from: ti };
        let mut tie_meshes: Vec<Vec<Option<Mesh>>> = Vec::new();
        if let Some((t, _)) = w.ties {
            for lods in t.rt_classes()? {
                tie_meshes.push(lods.iter().map(|c| o.mesh(&mut shade, c, 1, |set| &t.materials[set])).collect::<Result<_, String>>()?);
            }
        }
        let mut shrub_meshes: Vec<Option<Mesh>> = Vec::new();
        if let Some((s, _)) = w.shrubs {
            for c in s.rt_classes()? {
                shrub_meshes.push(o.mesh(&mut shade, &c, 2, |_| &s.materials)?);
            }
        }
        if shade.geoms.len() >= 1 << 24 {
            return Err(format!("native: ray tracing: {} geometries, more than an instance's 24-bit index reaches", shade.geoms.len()));
        }
        // The vertex buffers: the terrain's part gathered from the level's on the GPU, the classes' written after it.
        let (nv, ni) = (tv as u64 + o.pos.len() as u64, ti as u64 + o.idx.len() as u64);
        let buffer = |label: &str, size: u64, blas: bool| {
            let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | if blas { wgpu::BufferUsages::BLAS_INPUT } else { wgpu::BufferUsages::empty() };
            device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size.max(4), usage, mapped_at_creation: false })
        };
        let pos = buffer("ray-traced places", 12 * nv, true);
        let attr = buffer("ray-traced coordinates, colours and normals", 16 * nv, false);
        let idx = buffer("ray-traced corners", 4 * ni, true);
        queue.write_buffer(&pos, 12 * tv as u64, bytemuck::cast_slice(&o.pos));
        queue.write_buffer(&attr, 16 * tv as u64, bytemuck::cast_slice(&o.attr));
        queue.write_buffer(&idx, 4 * ti as u64, bytemuck::cast_slice(&o.idx));
        // The bottom levels: the terrain's, then each class's.
        // Built for tracing, and compacted after (below).
        let flags = wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE | wgpu::AccelerationStructureFlags::ALLOW_COMPACTION;
        let make = |label: &str, sizes: &[wgpu::BlasTriangleGeometrySizeDescriptor]| device.create_blas(&wgpu::CreateBlasDescriptor { label: Some(label), flags, update_mode: wgpu::AccelerationStructureUpdateMode::Build }, wgpu::BlasGeometrySizeDescriptors::Triangles { descriptors: sizes.to_vec() });
        let terrain_blas = (!terrain_sizes.is_empty()).then(|| make("ray-traced terrain", &terrain_sizes));
        let class_blas = |m: &Option<Mesh>, label: &str| m.as_ref().map(|m| (make(label, &m.sizes), m.geom));
        let ties: Vec<Vec<Option<(wgpu::Blas, u32)>>> = tie_meshes.iter().map(|lods| lods.iter().map(|m| class_blas(m, "ray-traced tie class")).collect()).collect();
        let shrubs: Vec<Option<(wgpu::Blas, u32)>> = shrub_meshes.iter().map(|m| class_blas(m, "ray-traced shrub class")).collect();
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("ray-traced world") });
        {
            let g = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("terrain vertices"),
                layout: &rt.gather_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: w.vertices.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: w.indices.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: pos.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: attr.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 4, resource: idx.as_entire_binding() },
                ],
            });
            let groups = tv.max(ti).div_ceil(64);
            let mut p = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("terrain vertices"), timestamp_writes: None });
            p.set_pipeline(&rt.gather);
            p.set_bind_group(0, &g, &[]);
            p.dispatch_workgroups(groups.min(65535), groups.div_ceil(65535), 1);
        }
        let mut entries = Vec::new();
        if let Some(b) = &terrain_blas {
            let geometry = terrain_sizes.iter().zip(&w.draws).map(|(size, &(_, first, _))| wgpu::BlasTriangleGeometry { size, vertex_buffer: w.vertices, first_vertex: 0, vertex_stride: std::mem::size_of::<crate::native::Vertex>() as u64, index_buffer: Some(w.indices), first_index: Some(first), transform_buffer: None, transform_buffer_offset: None }).collect();
            entries.push(wgpu::BlasBuildEntry { blas: b, geometry: wgpu::BlasGeometries::TriangleGeometries(geometry) });
        }
        let classes = tie_meshes.iter().flatten().zip(ties.iter().flatten()).chain(shrub_meshes.iter().zip(shrubs.iter()));
        for (m, b) in classes {
            if let (Some(m), Some((b, _))) = (m, b) {
                let geometry = m.sizes.iter().zip(&m.firsts).map(|(size, &first)| wgpu::BlasTriangleGeometry { size, vertex_buffer: &pos, first_vertex: tv + m.base, vertex_stride: 12, index_buffer: Some(&idx), first_index: Some(ti + first), transform_buffer: None, transform_buffer_offset: None }).collect();
                entries.push(wgpu::BlasBuildEntry { blas: b, geometry: wgpu::BlasGeometries::TriangleGeometries(geometry) });
            }
        }
        enc.build_acceleration_structures(entries.iter(), std::iter::empty::<&wgpu::Tlas>());
        drop(entries);
        queue.submit([enc.finish()]);
        // Every bottom level compacted, as NVIDIA advises for every static one on its RTX cards: the build is made with room for
        // its worst case, and the copy the driver makes once it knows the built size is smaller (less of the GPU's memory, which
        // this PC's driver charges to the process, and fewer bytes to fetch while tracing).
        let tc = std::time::Instant::now();
        let failed = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let all: Vec<&wgpu::Blas> = terrain_blas.iter().chain(ties.iter().flatten().flatten().map(|(b, _)| b)).chain(shrubs.iter().flatten().map(|(b, _)| b)).collect();
        for b in &all {
            let failed = failed.clone();
            b.prepare_compaction_async(move |r| {
                if let Err(e) = r {
                    failed.lock().expect("the compaction's errors").push(e.to_string());
                }
            });
        }
        device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("native: GPU wait: {e}"))?;
        let compacted = all.len();
        drop(all);
        if let Some(e) = failed.lock().map_err(|_| "native: ray tracing: the compaction's errors' lock is poisoned")?.first() {
            return Err(format!("native: ray tracing: a bottom level cannot be compacted: {e}"));
        }
        let compact = |b: wgpu::Blas| -> Result<wgpu::Blas, String> {
            match b.ready_for_compaction() {
                true => Ok(queue.compact_blas(&b)),
                false => Err("native: ray tracing: a bottom level not ready for compaction after the GPU finished".into()),
            }
        };
        let terrain_blas = terrain_blas.map(compact).transpose()?;
        let ties = ties.into_iter().map(|lods| lods.into_iter().map(|m| m.map(|(b, g)| compact(b).map(|b| (b, g))).transpose()).collect::<Result<Vec<_>, String>>()).collect::<Result<Vec<_>, String>>()?;
        let shrubs = shrubs.into_iter().map(|m| m.map(|(b, g)| compact(b).map(|b| (b, g))).transpose()).collect::<Result<Vec<_>, String>>()?;
        let compact_ms = tc.elapsed().as_secs_f64() * 1e3;
        let triangles = terrain_sizes.iter().chain(tie_meshes.iter().flatten().chain(&shrub_meshes).flatten().flat_map(|m| &m.sizes)).map(|s| s.index_count.unwrap_or(0) as u64 / 3).sum::<u64>();
        let shiny = shade.mats.iter().filter(|m| m.shine > 0.0).count();
        println!(
            "native: ray tracing: world built: terrain {} ({} draws), {} tie classes at each LOD, {} shrub classes, {triangles} triangles, {} materials ({} under the game's shine), {:.1} ms ({compacted} bottom levels compacted in {compact_ms:.1})",
            w.terrain,
            w.draws.len(),
            tie_meshes.len(),
            shrub_meshes.iter().flatten().count(),
            shade.mats.len(),
            shiny,
            t0.elapsed().as_secs_f64() * 1e3
        );
        let (geoms, mats) = shade.upload(device)?;
        Ok(World { terrain: w.terrain, terrain_blas, ties, shrubs, geoms, mats, views: shade.views, idx, pos, attr, placed: None })
    }

    /// The top level: the terrain where it lies, every shown tie at the LOD the game draws it at and every shrub where its matrix
    /// puts its class, each with its first word in the frame's colours; its build recorded on commands of its own, into `outbox`.
    /// The colours and the group follow (`colour`).
    /// Level 28 in play: made again on 280 of 1,556 frames (a tie changing LOD), 1.2 to 1.8 ms of the renderer's thread each.
    fn place(&mut self, rt: &RayTracing, device: &wgpu::Device, outbox: &mut crate::native::Outbox, ties: Vec<(usize, u8, [f32; 12], u32, u32)>, shrubs: Vec<(usize, [f32; 12], u32)>) -> Result<(), String> {
        let mut instances: Vec<wgpu::TlasInstance> = self.terrain_blas.iter().map(|b| wgpu::TlasInstance::new(b, IDENTITY, 0, 0xff)).collect();
        let mut inst: Vec<u32> = vec![0; instances.len()];
        let mut word = 0u32;
        for &(c, lod, transform, _, words) in &ties {
            let lods = self.ties.get(c).ok_or_else(|| format!("native: ray tracing: a tie of class {c} of {}", self.ties.len()))?;
            if let Some(Some((b, geom))) = lods.get(lod as usize) {
                instances.push(wgpu::TlasInstance::new(b, transform, *geom, 0xff));
                inst.push(word);
            }
            word += words;
        }
        let blocks = shrubs.iter().map(|s| s.2 + 1).max().unwrap_or(0);
        for &(c, transform, block) in &shrubs {
            if let Some((b, geom)) = self.shrubs.get(c).ok_or_else(|| format!("native: ray tracing: a shrub of class {c} of {}", self.shrubs.len()))? {
                instances.push(wgpu::TlasInstance::new(b, transform, *geom, 0xff));
                inst.push(word + 24 * block);
            }
        }
        if instances.is_empty() {
            return Err("native: ray tracing: the static world has no surface".into());
        }
        let mut tlas = device.create_tlas(&wgpu::CreateTlasDescriptor { label: Some("ray-traced world"), max_instances: instances.len() as u32, flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE, update_mode: wgpu::AccelerationStructureUpdateMode::Build });
        for (k, i) in instances.into_iter().enumerate() {
            tlas[k] = Some(i);
        }
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("ray-traced world's places") });
        enc.build_acceleration_structures(std::iter::empty::<&wgpu::BlasBuildEntry<'_>>(), std::iter::once(&tlas));
        outbox.commands.push(enc.finish());
        let inst = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("ray-traced instances' colours"), contents: bytemuck::cast_slice(&inst), usage: wgpu::BufferUsages::STORAGE });
        let cols = device.create_buffer(&wgpu::BufferDescriptor { label: Some("ray-traced colours"), size: 4, usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false });
        let group = self.group(rt, device, &tlas, &inst, &cols);
        // No colours yet: the frame's are uploaded next (`LevelRt::frame`), and differ from these.
        self.placed = Some(Placed { ties, shrubs, tlas, inst, blocks, colours: Vec::new(), cols, group });
        Ok(())
    }

    /// The frame's colours (`LevelRt::frame`) uploaded, and the group with them.
    fn colour(&mut self, rt: &RayTracing, device: &wgpu::Device, colours: Vec<u32>) -> Result<(), String> {
        let contents: &[u8] = if colours.is_empty() { &[0; 4] } else { bytemuck::cast_slice(&colours) };
        let cols = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("ray-traced colours"), contents, usage: wgpu::BufferUsages::STORAGE });
        let p = self.placed.as_ref().ok_or("native: ray tracing: colours before the top level")?;
        let group = self.group(rt, device, &p.tlas, &p.inst, &cols);
        let p = self.placed.as_mut().expect("checked above");
        p.cols = cols;
        p.group = group;
        p.colours = colours;
        Ok(())
    }

    /// The static world's scene group (`scene`, `scene_layout`) of top level `tlas` with instance table `inst` and colours `cols`.
    fn group(&self, rt: &RayTracing, device: &wgpu::Device, tlas: &wgpu::Tlas, inst: &wgpu::Buffer, cols: &wgpu::Buffer) -> wgpu::BindGroup {
        let views: Vec<&wgpu::TextureView> = self.views.iter().collect();
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ray-traced world"),
            layout: &rt.world_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: tlas.as_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: self.geoms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: self.mats.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: self.idx.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: self.pos.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: self.attr.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureViewArray(&views) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::Sampler(&rt.sampler) },
                wgpu::BindGroupEntry { binding: 8, resource: inst.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 9, resource: cols.as_entire_binding() },
            ],
        })
    }
}

impl Actors {
    /// The frame's mobies' structures (not built yet: `RtFrame::casters`), None when it has no opaque moby draw.
    fn new(rt: &RayTracing, device: &wgpu::Device, m: MobiesIn<'_>) -> Result<Option<Actors>, String> {
        let (Some(now), Some(before)) = (m.now, m.before) else { return Ok(None) };
        if m.runs.is_empty() {
            return Ok(None);
        }
        let n = m.vertices.len();
        if n > MOST_VERTICES {
            return Err(format!("native: ray tracing: {n} moby vertices, more than one dispatch of the places' pass reaches"));
        }
        let mut shade = Shade::default();
        // Each material's corners, opaque or alpha-tested, in the order first met.
        let mut geoms: Vec<(u32, bool, Vec<u32>)> = Vec::new();
        let mut mats: HashMap<(*const crate::objects::Materials, crate::objects::Key), usize> = HashMap::new();
        for (materials, key, first, count) in &m.runs {
            if (first + count) as usize > n {
                return Err(format!("native: ray tracing: a moby draw past the frame's {n} vertices"));
            }
            let at = match mats.get(&(*materials as *const _, *key)) {
                Some(&k) => k,
                None => {
                    let (view, cs, ct, aref, fails, rift) = materials.alpha_test(key)?;
                    // Every alpha-blended draw stops a ray only at opaque pixels, as `mask_opaque` does. Even an opaque
                    // texture can fade through vertex alpha; looking only for see-through texels missed those fades.
                    let (aref, fails) = match key.5 == crate::objects::Blend::Alpha {
                        true => (aref.max(0x80), true),
                        false => (aref, fails),
                    };
                    let mat = shade.mat(view, cs, ct, aref, false, MODE_SURFACE, 1.0, materials.minimum_alpha(key), rift.as_ref())?;
                    geoms.push((mat, !fails, Vec::new()));
                    mats.insert((*materials as *const _, *key), geoms.len() - 1);
                    geoms.len() - 1
                }
            };
            geoms[at].2.extend(*first..first + count);
        }
        let (mut sizes, mut firsts, mut idx): (Vec<wgpu::BlasTriangleGeometrySizeDescriptor>, Vec<u32>, Vec<u32>) = (Vec::new(), Vec::new(), Vec::new());
        for (mat, opaque, corners) in &geoms {
            firsts.push(idx.len() as u32);
            sizes.push(tris(n as u32, corners.len() as u32, *opaque));
            shade.geoms.push(Geom { first: idx.len() as u32, base: 0, mat: *mat, flags: 0 });
            idx.extend(corners);
        }
        let idx_b = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("ray-traced moby corners"), contents: bytemuck::cast_slice(&idx), usage: wgpu::BufferUsages::BLAS_INPUT | wgpu::BufferUsages::STORAGE });
        let pos = device.create_buffer(&wgpu::BufferDescriptor { label: Some("ray-traced moby places"), size: 12 * n as u64, usage: wgpu::BufferUsages::BLAS_INPUT | wgpu::BufferUsages::STORAGE, mapped_at_creation: false });
        let flags = wgpu::AccelerationStructureFlags::PREFER_FAST_BUILD;
        let blas = device.create_blas(&wgpu::CreateBlasDescriptor { label: Some("ray-traced mobies"), flags, update_mode: wgpu::AccelerationStructureUpdateMode::Build }, wgpu::BlasGeometrySizeDescriptors::Triangles { descriptors: sizes.clone() });
        let mut tlas = device.create_tlas(&wgpu::CreateTlasDescriptor { label: Some("ray-traced mobies"), max_instances: 1, flags, update_mode: wgpu::AccelerationStructureUpdateMode::Build });
        tlas[0] = Some(wgpu::TlasInstance::new(&blas, IDENTITY, 0, 0xff));
        let vertices = std::num::NonZeroU64::new(std::mem::size_of::<crate::objects::ObjVertex>() as u64 * n as u64);
        let places = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("moby places"),
            layout: &rt.places_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: m.view, offset: 0, size: std::num::NonZeroU64::new(crate::native::SETS_AT) }) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: now, offset: 0, size: vertices }) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: before, offset: 0, size: std::num::NonZeroU64::new(12 * n as u64) }) },
                wgpu::BindGroupEntry { binding: 3, resource: pos.as_entire_binding() },
            ],
        });
        let (geoms_b, mats_b) = shade.upload(device)?;
        let views: Vec<&wgpu::TextureView> = shade.views.iter().collect();
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ray-traced mobies"),
            layout: &rt.mobies_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: tlas.as_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: geoms_b.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: mats_b.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: idx_b.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: pos.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: now, offset: 0, size: vertices }) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureViewArray(&views) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::Sampler(&rt.sampler) },
            ],
        });
        Ok(Some(Actors { blas, tlas, sizes, firsts, pos, idx: idx_b, vertices: n as u32, places, group }))
    }
}

impl Layers {
    /// The frame's EE layers' structures (not built yet: `RtFrame::casters` builds them in the frame's own picture), None when the
    /// frame draws none. Each draw's material lies in what a ray sees as its blend draws it (`MODE_*`): ALPHA_1 (Cs - Cd) x As +
    /// Cd over what is behind by the pixel's alpha; a colour that replaces, or (Cs - Cd) x FIX + Cd, over it by that share; Cs x
    /// As + Cd or Cs x FIX + Cd added. A draw that writes alpha alone, scales the frame into another buffer (the glow's passes),
    /// takes the frame's alpha (a tie's second part), subtracts or blends some other way (`objects::Blend::Gs`) is not a layer. A
    /// pixel its alpha test fails is drawn all the same but where the material's failing pixels write nothing (AFAIL KEEP:
    /// `Materials::keeps`). Their vertices are the frame's own EE and effect vertex buffers, as the frame draws them (no copy).
    fn new(rt: &RayTracing, device: &wgpu::Device, e: EeIn<'_>) -> Result<Option<Layers>, String> {
        use crate::objects::Blend;
        let mut shade = Shade::default();
        // Each material's corners, by the buffer they lie in (false the EE's, true the effects').
        let mut geoms: Vec<(u32, bool, Vec<u32>)> = Vec::new();
        let mut mats: HashMap<(*const crate::objects::Materials, crate::objects::Key, bool), usize> = HashMap::new();
        for &(materials, key, fx, first, count) in &e.runs {
            let n = if fx { e.fx_vertices } else { e.ee_vertices };
            if first + count > n || (if fx { e.fx } else { e.ee }).is_none() {
                return Err(format!("native: ray tracing: an EE draw past the frame's {n} {} vertices", if fx { "effect" } else { "EE" }));
            }
            let (mode, factor) = match key.5 {
                Blend::Alpha => (MODE_OVER_ALPHA, 1.0),
                Blend::Replace => (MODE_OVER_FIX, 1.0),
                Blend::Fade(fix) => (MODE_OVER_FIX, (fix as f32 / 128.0).min(1.0)),
                Blend::AddAlpha => (MODE_ADD_ALPHA, 1.0),
                Blend::Add(fix) => (MODE_ADD_FIX, fix as f32 / 128.0),
                Blend::Keep | Blend::Scale(_) | Blend::AddDst | Blend::Sub(_) | Blend::Gs(..) => continue,
            };
            let at = match mats.get(&(materials as *const _, key, fx)) {
                Some(&k) => k,
                None => {
                    let (view, cs, ct, aref, _, _) = materials.alpha_test(&key)?;
                    let mat = shade.mat(view, cs, ct, if materials.keeps() { aref } else { 0 }, false, mode, factor, 0, None)?;
                    geoms.push((mat, fx, Vec::new()));
                    mats.insert((materials as *const _, key, fx), geoms.len() - 1);
                    geoms.len() - 1
                }
            };
            geoms[at].2.extend(first..first + count);
        }
        if geoms.is_empty() {
            return Ok(None);
        }
        let (mut sizes, mut firsts, mut sources, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::<u32>::new());
        for (mat, fx, corners) in &geoms {
            firsts.push(idx.len() as u32);
            sizes.push(tris(if *fx { e.fx_vertices } else { e.ee_vertices }, corners.len() as u32, false));
            sources.push(*fx);
            shade.geoms.push(Geom { first: idx.len() as u32, base: 0, mat: *mat, flags: if *fx { G_EFFECTS } else { 0 } });
            idx.extend(corners);
        }
        let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("ray-traced EE corners"), contents: bytemuck::cast_slice(&idx), usage: wgpu::BufferUsages::BLAS_INPUT | wgpu::BufferUsages::STORAGE });
        let flags = wgpu::AccelerationStructureFlags::PREFER_FAST_BUILD;
        let blas = device.create_blas(&wgpu::CreateBlasDescriptor { label: Some("ray-traced EE layers"), flags, update_mode: wgpu::AccelerationStructureUpdateMode::Build }, wgpu::BlasGeometrySizeDescriptors::Triangles { descriptors: sizes.clone() });
        let mut tlas = device.create_tlas(&wgpu::CreateTlasDescriptor { label: Some("ray-traced EE layers"), max_instances: 1, flags, update_mode: wgpu::AccelerationStructureUpdateMode::Build });
        tlas[0] = Some(wgpu::TlasInstance::new(&blas, IDENTITY, 0, 0xff));
        let (geoms_b, mats_b) = shade.upload(device)?;
        let views: Vec<&wgpu::TextureView> = shade.views.iter().collect();
        // The EE's group (`scene`'s, `ee_layout`): its EE vertices are its places too (bindings 4 and 5), the effects' at 8; a
        // frame with one of the two binds the other's where it has none (no geometry reads it).
        let ee = e.ee.or(e.fx).expect("a layer's buffer is checked above");
        let fx = e.fx.or(e.ee).expect("a layer's buffer is checked above");
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ray-traced EE layers"),
            layout: &rt.ee_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: tlas.as_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: geoms_b.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: mats_b.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: ib.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: ee.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: ee.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureViewArray(&views) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::Sampler(&rt.sampler) },
                wgpu::BindGroupEntry { binding: 8, resource: fx.as_entire_binding() },
            ],
        });
        Ok(Some(Layers { blas, tlas, sizes, firsts, sources, ee: ee.clone(), fx: fx.clone(), idx: ib, group }))
    }

    /// The build of the structures, recorded into the frame's own picture's commands (`RtFrame::casters`).
    fn build(&self, enc: &mut wgpu::CommandEncoder) {
        let stride = std::mem::size_of::<crate::objects::ObjVertex>() as u64;
        let geometry = self.sizes.iter().zip(&self.firsts).zip(&self.sources).map(|((size, &first), &fx)| wgpu::BlasTriangleGeometry { size, vertex_buffer: if fx { &self.fx } else { &self.ee }, first_vertex: 0, vertex_stride: stride, index_buffer: Some(&self.idx), first_index: Some(first), transform_buffer: None, transform_buffer_offset: None }).collect();
        let entry = wgpu::BlasBuildEntry { blas: &self.blas, geometry: wgpu::BlasGeometries::TriangleGeometries(geometry) };
        enc.build_acceleration_structures(std::iter::once(&entry), std::iter::once(&self.tlas));
    }
}

impl RtFrame {
    /// The mobies' structures built where the picture draws them (`replay::Cmd::RtCasters`): the places pass (the picture's view
    /// buffer's `lerp.x`), then the bottom level and its top level.
    /// In the frame's own picture (`fresh`) the EE layers' structures are built first (they lie where the frame put them: every
    /// picture of the frame sees them so). The mobies' are built in every picture with `every` (the ray-traced shadows trace them
    /// in each), else in the frame's own picture only, and the pictures after it trace against those.
    pub fn casters(&self, enc: &mut wgpu::CommandEncoder, fresh: bool, every: bool) {
        if let (true, Some(l)) = (fresh, &self.layers) {
            l.build(enc);
        }
        if !fresh && !every {
            return;
        }
        let Some(a) = &self.actors else { return };
        if let Some(t) = &self.rt.times {
            t.stamp(enc, T_CASTERS);
        }
        {
            let mut p = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("moby places"), timestamp_writes: None });
            p.set_pipeline(&self.rt.places);
            p.set_bind_group(0, &a.places, &[]);
            p.dispatch_workgroups(a.vertices.div_ceil(64), 1, 1);
        }
        let geometry = a.sizes.iter().zip(&a.firsts).map(|(size, &first)| wgpu::BlasTriangleGeometry { size, vertex_buffer: &a.pos, first_vertex: 0, vertex_stride: 12, index_buffer: Some(&a.idx), first_index: Some(first), transform_buffer: None, transform_buffer_offset: None }).collect();
        let entry = wgpu::BlasBuildEntry { blas: &a.blas, geometry: wgpu::BlasGeometries::TriangleGeometries(geometry) };
        enc.build_acceleration_structures(std::iter::once(&entry), std::iter::once(&a.tlas));
        if let Some(t) = &self.rt.times {
            t.stamp(enc, T_CASTERS + 1);
        }
    }

    /// Whether the frame traces mobies (`GpuTimes::report`, the scene library's variant).
    pub fn has_mobies(&self) -> bool {
        self.actors.is_some()
    }

    /// The mobies' scene group, when the frame traces mobies.
    pub(crate) fn mobies_group(&self) -> Option<&wgpu::BindGroup> {
        self.actors.as_ref().map(|a| &a.group)
    }

    /// The EE's layers' scene group, when the frame has any.
    pub(crate) fn layers_group(&self) -> Option<&wgpu::BindGroup> {
        self.layers.as_ref().map(|l| &l.group)
    }

    /// The shade over `target` (the picture, `size` pixels, its depth `depth`), seen from `cam` (`replay::Cmd::RtShadows`): after
    /// the static world every pixel (`mobies` false), after the moby runs the mask's (the occluders' pass, `ao::mask`). Nothing
    /// on a level with no key light.
    #[allow(clippy::too_many_arguments)]
    pub fn apply(&self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, depth: &wgpu::Texture, mask: &wgpu::TextureView, target: &wgpu::TextureView, size: (u32, u32), cam: &crate::native::Camera, fog: &uyafmt::gameplay::Fog, near_far: (f32, f32), mobies: bool) {
        let Some(to_light) = self.to_light else { return };
        let w4 = |a: [f32; 3]| [a[0], a[1], a[2], 0.0];
        let (f, up) = (cam.forward, cam.up);
        let right = [f[1] * up[2] - f[2] * up[1], f[2] * up[0] - f[0] * up[2], f[0] * up[1] - f[1] * up[0]];
        let u = ApplyU {
            r: w4(right),
            u: w4(up),
            f: w4(f),
            eye: w4(cam.eye),
            tan: [cam.tan[0], cam.tan[1], size.0 as f32, size.1 as f32],
            nf: [near_far.0, near_far.1, crate::shadows::DARKNESS, crate::shadows::REACH],
            fog: [fog.near, fog.far, fog.near_f, fog.far_f],
            to_light: [to_light[0], to_light[1], to_light[2], std::env::var_os("UYA_RT_CHECK").is_some() as u32 as f32],
        };
        let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("ray-traced shadows"), contents: bytemuck::bytes_of(&u), usage: wgpu::BufferUsages::UNIFORM });
        let dv = depth.create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ray-traced shadows"),
            layout: &self.rt.frame_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&dv) }, wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(mask) }],
        });
        let pipeline = &self.rt.apply[self.actors.is_some() as usize][mobies as usize];
        let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ray-traced shadows"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: target, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: self.rt.times.as_ref().map(|t| t.pass(if mobies { T_SHADE_MOBIES } else { T_SHADE })),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        p.set_pipeline(pipeline);
        p.set_bind_group(0, &group, &[]);
        p.set_bind_group(1, &self.world, &[]);
        if let Some(a) = &self.actors {
            p.set_bind_group(2, &a.group, &[]);
        }
        p.draw(0..3, 0..1);
    }
}
