//! Water reflections, the `water_reflections` option (PLAN 8.5; `settings::OPTIONS`, `enhance::reflections`): the water, slime, swamp and sea the game draws reflect the
//! world above them, mirrored in the water's own plane from the picture the frame has drawn (screen-space planar reflections).
//!
//! - **The plane** is the game's own water of the frame: the triangles the water's and the sea's VU1 programs give
//!   (`mobies::Walk::passes`, `vuwater.rs`, `vusea.rs`), put back in the world as every EE vertex is (`effects::Projection`). Of
//!   their level ones (a normal within 25 degrees of the world's up, +z) in a translucent layer (`opacity`), those of one height
//!   (heights less than `GAP` apart) are a surface; of the surfaces below the eye (water seen from under it mirrors nothing above
//!   it: Aquatos's hangar, field 12600 of the tour, has its pool's surface over the camera), the one that covers the most of the
//!   screen is the frame's water: its height is its triangles' mean, weighted by each one's area in the world. Every
//!   `STRIDE`th triangle of a pass is looked at (the sea gives 20,000 a frame; their heights run together all the same), and the
//!   water's reach about its height is twice the farthest corner seen (the sea's waves).
//! - **The reflection** (`Reflections::mirror`, compute passes after the water's pass, so after everything the frame drew before
//!   the water): every pixel of the picture whose point (the depth buffer's, the camera's ray at its view depth; the sky's, which
//!   writes no depth, at infinity along its ray) is above the water is mirrored in the plane and put at the place the camera sees
//!   its mirror image, in a reflection picture (half the native picture's size on low, its size on high). Where several land on
//!   one place, the one the eye sees first through the water wins: the lowest on the screen (the cameras are upright: what is
//!   nearer the water along the reflected ray is lower on the screen), by an atomic maximum of (row + 1, column). A
//!   place no pixel lands on takes its nearest neighbour's within a texel, else it reflects nothing there (what the picture does
//!   not show, above its top, is not invented).
//! - **The water** takes it right after each pass of the water's programs is drawn, over the pass's triangles of the plane in
//!   its translucent layer (`opacity`: the layer blended over what it covers; the layers that add light or write alpha alone are
//!   left, so a surface drawn in several layers takes it once), as a sheen screened over the water as the game drew it (the
//!   picture + S x (1 - the picture), S = the reflection x R x the level's `STRENGTH`: the water stays whole underneath; the
//!   first version mixed the reflection in, R x it + (1 - R) x the water, and the player found it too much), R
//!   water's Fresnel reflectance for the eye's angle to the surface (Schlick: F0 + (1 - F0)(1 - cos)^5, F0 = ((n - 1) / (n +
//!   1))^2 for water's index n = 1.333) of what the layer lets through (1 - its opacity, the game's own blend: Florana's sea
//!   0.37 of it, level 19's water 0.5, level 28's glowing slime about a quarter). Looking down, the water keeps its own look;
//!   toward the horizon it mirrors the shore and the sky; slime keeps its glow under a sheen. The frame's alpha byte (which
//!   the glow reads) is kept.
//!
//! - **Rift Apart's ripples** (PLAN 9.2, with UPDATED TEXTURES on: `texnames::water_normal`, Rift Apart's own tiling water
//!   normal): two layers of it drifting across each other over the water's plane turn the surface, so the reflection is looked
//!   up where the ripples move it, its Fresnel taken at their angle, and where a ripple turns the level's sun to the eye it
//!   glints (`RIPPLE_TILE`, `RIPPLE_SHIFT`, `GLINT`).
//!
//! Both are commands of the frame's recorded list (`replay::Cmd::Mirror`, the overlay's draw), so a picture drawn between two
//! frames mirrors its own picture from its own camera.

use crate::effects::Projection;
use crate::mobies::EePrim;
use crate::native::Camera;

/// Heights of level water triangles closer than this (world units) are one surface.
const GAP: f32 = 0.5;
/// The cosine of the largest angle between a water triangle's normal and the world's up that is still level water (25 degrees).
const LEVEL: f32 = 0.906;
/// The plane is found from every STRIDEth triangle of a pass.
const STRIDE: usize = 4;
/// The sheen's strength at the option's levels (off, low, high): the player, 15 November 2026, "Too much reflection. Keep the
/// original water underneath and lighten the reflection": low a hint, high clearly visible, the game's water the main thing
/// seen at both.
const STRENGTH: [f32; 3] = [0.0, 0.25, 0.6];
/// Rift Apart's ripples (PLAN 9.2, UPDATED TEXTURES on): the water normal's tile in world units (its cells about a metre
/// and a half), how far the reflection moves per unit of the ripples' slope (a share of the picture), and the sun's glint's
/// strength over its colour.
const RIPPLE_TILE: f32 = 6.0;
const RIPPLE_SHIFT: f32 = 0.02;
const GLINT: f32 = 2.0;
/// Water's index of refraction, and its reflectance looking straight down.
const N: f32 = 1.333;
const F0: f32 = ((N - 1.0) / (N + 1.0)) * ((N - 1.0) / (N + 1.0));

/// The frame's water plane: its height and how far its triangles reach from it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Plane {
    pub h: f32,
    pub tol: f32,
}

impl Plane {
    /// Whether the world triangle `t` of a water layer is of this plane: within its reach, however its waves tilt it (a test of
    /// its tilt here, as the plane's search has, left the triangles a wave turned past 25 degrees without the reflection, dark
    /// patches moving with the waves: the player, 16 November 2026, "When water reflections are turned on water flickers").
    pub fn holds(&self, t: [[f32; 3]; 3]) -> bool {
        t.iter().all(|v| (v[2] - self.h).abs() <= self.tol)
    }
}

/// Whether world triangle `t` is level water: its normal within 25 degrees of +z (either face).
fn level(t: [[f32; 3]; 3]) -> bool {
    let (a, b) = (sub(t[1], t[0]), sub(t[2], t[0]));
    let n = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    len > 0.0 && n[2].abs() >= LEVEL * len
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// The opacity of a water layer's vertex, for the layers that blend over what they cover: PRIM ABE with ALPHA_1 (Cs - Cd) x C
/// + Cd, C the vertex's alpha (As: the vertex's A over 0x80, its texels' as the vertex's) or FIX (FIX over 0x80). Florana's sea
/// is FIX 0x50 (0.63 opaque), level 19's water FIX 0x40, level 28's slime As 0x59 to 0x73 under the layers that add its glow;
/// None for any other layer (one that adds light or writes alpha alone: the reflection is laid over a surface once, after its
/// translucent layer's pass).
pub(crate) fn opacity(p: &EePrim) -> Option<Opacity> {
    let al = |s: u32| (p.alpha >> s & 3) as u8;
    if p.prim >> 6 & 1 == 0 || (al(0), al(2), al(6)) != (0, 1, 1) {
        return None;
    }
    match al(4) {
        0 => Some(Opacity::Vertex),
        2 => Some(Opacity::Fix(((p.alpha >> 32) as u8 as f32 / 128.0).min(1.0))),
        _ => None,
    }
}

/// A translucent water layer's opacity (`opacity`): each vertex's alpha, or one FIX.
#[derive(Clone, Copy)]
pub(crate) enum Opacity {
    Vertex,
    Fix(f32),
}

impl Opacity {
    pub fn of(self, v: &crate::effects::GsVertex) -> f32 {
        match self {
            Opacity::Vertex => (v.rgba[3] as f32 / 128.0).min(1.0),
            Opacity::Fix(f) => f,
        }
    }
}

/// Whether `p` is a triangle of the world drawn into the main frame (the GS's Z test on, GEQUAL or GREATER), as the water's are.
pub(crate) fn world_triangle(p: &EePrim) -> bool {
    let (ty, zte, ztst) = (p.prim & 7, p.test >> 16 & 1, p.test >> 17 & 3);
    (3..=5).contains(&ty) && p.verts.len() == 3 && zte == 1 && matches!(ztst, 2 | 3) && p.frame & 0x1ff == 0xF4 && p.frame >> 16 & 0x3f == 8
}

/// The frame's water plane from the passes' output (`passes`: ranges of `prims`), or None: no level water in the frame, or the
/// eye at or under it.
pub(crate) fn plane(prims: &[(usize, EePrim)], passes: &[std::ops::Range<usize>], proj: &Projection, cam: &Camera) -> Option<Plane> {
    // Every STRIDEth level triangle of a translucent layer (the layers the reflection is laid over, `opacity`): its mean height,
    // its area on the screen, its lowest and highest corner, its area in the world (seen from above).
    let mut tris: Vec<(f32, f64, f32, f32, f64)> = Vec::new();
    for r in passes {
        for (_, p) in prims[r.clone()].iter().step_by(STRIDE) {
            if !world_triangle(p) || opacity(p).is_none() {
                continue;
            }
            let v = &p.verts;
            let t = [proj.world(v[0].x, v[0].y, v[0].z), proj.world(v[1].x, v[1].y, v[1].z), proj.world(v[2].x, v[2].y, v[2].z)];
            if !t.iter().flatten().all(|c| c.is_finite()) || !level(t) {
                continue;
            }
            let area = ((v[1].x - v[0].x) * (v[2].y - v[0].y) - (v[2].x - v[0].x) * (v[1].y - v[0].y)).abs() / 2.0;
            let zs = [t[0][2], t[1][2], t[2][2]];
            let world = (((t[1][0] - t[0][0]) * (t[2][1] - t[0][1]) - (t[2][0] - t[0][0]) * (t[1][1] - t[0][1])).abs() / 2.0) as f64;
            tris.push(((zs[0] + zs[1] + zs[2]) / 3.0, area, zs.iter().copied().fold(f32::MAX, f32::min), zs.iter().copied().fold(f32::MIN, f32::max), world));
        }
    }
    tris.sort_by(|a, b| a.0.total_cmp(&b.0));
    // The surfaces: runs of heights with no gap of GAP; of those below the eye (water seen from under it mirrors nothing above
    // it), the one covering the most of the screen.
    let mut best: Option<(f64, Plane)> = None;
    let mut i = 0;
    while i < tris.len() {
        let mut j = i + 1;
        while j < tris.len() && tris[j].0 - tris[j - 1].0 < GAP {
            j += 1;
        }
        let s = &tris[i..j];
        let area: f64 = s.iter().map(|t| t.1).sum();
        let world: f64 = s.iter().map(|t| t.4).sum();
        if area > 0.0 && world > 0.0 {
            // Its height the mean of its triangles' by their area in the world, not on the screen: by the screen's, the waves
            // nearest the camera moved the plane, and the mirror image with it, from frame to frame.
            let h = (s.iter().map(|t| t.0 as f64 * t.4).sum::<f64>() / world) as f32;
            let tol = 2.0 * s.iter().map(|t| (t.2 - h).abs().max((t.3 - h).abs())).fold(0.0, f32::max) + 0.01;
            if cam.eye[2] > h + tol && best.is_none_or(|b| area > b.0) {
                best = Some((area, Plane { h, tol }));
            }
        }
        i = j;
    }
    best.map(|b| b.1)
}

const MIRROR: &str = r#"
// The camera's eye, forward, right and up; its tangents across and up, the plane's height and reach; the picture's and the
// reflection's sizes; the depth buffer's near and far planes.
struct U { eye: vec4<f32>, f: vec4<f32>, r: vec4<f32>, u: vec4<f32>, tan: vec4<f32>, size: vec4<f32>, planes: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var picture: texture_2d<f32>;
@group(0) @binding(2) var depth: texture_depth_2d;
@group(0) @binding(3) var<storage, read_write> hash: array<atomic<u32>>;
@group(0) @binding(4) var refl: texture_storage_2d<rgba8unorm, write>;

// The depth buffer's projection (`native::Camera::matrix`): reversed depth NEAR / (FAR - NEAR) x (FAR / z - 1), z the view depth.
fn view_z(d: f32) -> f32 {
    return u.planes.x * u.planes.y / (u.planes.x + d * (u.planes.y - u.planes.x));
}

// Each pixel above the water put where the camera sees its mirror image, the lowest on the screen winning.
@compute @workgroup_size(8, 8) fn project(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(u.size.xy);
    if (id.x >= size.x || id.y >= size.y) {
        return;
    }
    let d = textureLoad(depth, vec2<i32>(id.xy), 0);
    let ndc = vec2<f32>((f32(id.x) + 0.5) / u.size.x * 2.0 - 1.0, 1.0 - (f32(id.y) + 0.5) / u.size.y * 2.0);
    // The camera's ray through the pixel, one unit forward.
    let ray = u.f.xyz + u.r.xyz * ndc.x * u.tan.x + u.u.xyz * ndc.y * u.tan.y;
    var m: vec3<f32>;
    if (d <= 0.0) {
        // The sky: a direction; its mirror image the direction turned down.
        if (ray.z <= 0.0) {
            return;
        }
        m = vec3<f32>(ray.x, ray.y, -ray.z);
    } else {
        let q = u.eye.xyz + ray * view_z(d);
        if (q.z <= u.tan.z + u.tan.w) {
            return;
        }
        m = vec3<f32>(q.x, q.y, 2.0 * u.tan.z - q.z) - u.eye.xyz;
    }
    let ahead = dot(m, u.f.xyz);
    if (ahead <= 0.0) {
        return;
    }
    let s = vec2<f32>(dot(m, u.r.xyz) / (ahead * u.tan.x), dot(m, u.u.xyz) / (ahead * u.tan.y));
    let t = vec2<f32>((s.x + 1.0) * 0.5 * u.size.z, (1.0 - s.y) * 0.5 * u.size.w);
    if (t.x < 0.0 || t.y < 0.0 || t.x >= u.size.z || t.y >= u.size.w) {
        return;
    }
    let at = u32(t.y) * u32(u.size.z) + u32(t.x);
    atomicMax(&hash[at], ((id.y + 1u) << 16u) | id.x);
}

fn source(t: vec2<i32>) -> u32 {
    let w = i32(u.size.z);
    let h = i32(u.size.w);
    if (t.x < 0 || t.y < 0 || t.x >= w || t.y >= h) {
        return 0u;
    }
    return atomicLoad(&hash[u32(t.y * w + t.x)]);
}

// Each place of the reflection: the colour of the pixel that won it (or of its nearest neighbour's within a texel); nothing
// (alpha 0) where none landed.
@compute @workgroup_size(8, 8) fn resolve(@builtin(global_invocation_id) id: vec3<u32>) {
    if (f32(id.x) >= u.size.z || f32(id.y) >= u.size.w) {
        return;
    }
    let t = vec2<i32>(id.xy);
    var k = source(t);
    let near = array<vec2<i32>, 8>(vec2<i32>(0, -1), vec2<i32>(0, 1), vec2<i32>(-1, 0), vec2<i32>(1, 0), vec2<i32>(-1, -1), vec2<i32>(1, -1), vec2<i32>(-1, 1), vec2<i32>(1, 1));
    for (var n = 0; n < 8 && k == 0u; n = n + 1) {
        k = source(t + near[n]);
    }
    if (k == 0u) {
        textureStore(refl, t, vec4<f32>(0.0));
        return;
    }
    let p = vec2<i32>(i32(k & 0xffffu), i32(k >> 16u) - 1);
    // A pitched camera projects the source viewport's four edges onto the water as a trapezoid.
    // Let its contribution go smoothly to zero there, in source pixels so LOW and HIGH agree.
    let edge = min(vec2<f32>(p), u.size.xy - vec2<f32>(1.0) - vec2<f32>(p));
    let confidence = smoothstep(0.0, 0.08 * min(u.size.x, u.size.y), min(edge.x, edge.y));
    textureStore(refl, t, vec4<f32>(textureLoad(picture, p, 0).rgb, confidence));
}
"#;

const WATER: &str = r#"
struct View { m: mat4x4<f32>, sky: mat4x4<f32>, eye: vec4<f32>, forward: vec4<f32>, fog_colour: vec4<f32>, fog: vec4<f32>, lerp: vec4<f32> };
// The picture's width and height, water's reflectance looking straight down, and the sheen's strength at the option's level;
// the ripples (UPDATED TEXTURES: `ripple.w` 1): the time in seconds, the water normal's tile in world units, how far the
// reflection is moved per unit of the ripples' slope (a share of the picture); the level's sun: the direction its light
// travels in and its glint's strength (0: no sun).
struct Water { size: vec4<f32>, ripple: vec4<f32>, sun: vec4<f32> };
@group(0) @binding(0) var<uniform> view: View;
@group(1) @binding(0) var refl: texture_2d<f32>;
@group(1) @binding(1) var smp: sampler;
@group(1) @binding(2) var<uniform> water: Water;
// Rift Apart's tiling water normal (`texnames::water_normal`; a flat one while the option is off) and its sampler (repeating).
@group(1) @binding(3) var wnorm: texture_2d<f32>;
@group(1) @binding(4) var wsmp: sampler;

struct VIn { @location(0) pos: vec3<f32>, @location(1) st: vec3<f32>, @location(2) colour: vec4<f32>, @location(3) f: f32 };
// The vertex's opacity rides in its fog value's place (`reflections::opacity`). The position is `@invariant`, as the world's
// pipelines' (`objects.rs` VOut): the sheen passes its depth test over the water's own triangles only at exactly their depth,
// and without it a separate compilation rounded each triangle's depth its own way, so whole triangles of the water lost the
// sheen in one picture and had it in the next (the player, 16 November 2026: "When water reflections are turned on water
// flickers").
struct VOut { @builtin(position) @invariant clip: vec4<f32>, @location(0) world: vec3<f32>, @location(1) opacity: f32 };

@vertex fn vs(v: VIn) -> VOut {
    var o: VOut;
    o.clip = view.m * vec4<f32>(v.pos, 1.0);
    o.world = v.pos;
    o.opacity = v.f;
    return o;
}

// The reflection at the pixel as a sheen: times F (Schlick's Fresnel term for the eye's angle to the surface), what the
// surface lets through (1 - its opacity) and the level's strength, where the reflection holds something; screened over the
// water as it was drawn (the blend), which stays whole underneath. With Rift Apart's ripples: the surface's normal turned by
// two layers of its water normal drifting across each other over the plane, the reflection looked up where the ripples move
// it and its Fresnel at their angle, and the sun's glint where a ripple turns its light to the eye.
@fragment fn fs(i: VOut) -> @location(0) vec4<f32> {
    let e = normalize(view.eye.xyz - i.world);
    var n = vec3<f32>(0.0, 0.0, 1.0);
    var at = i.clip.xy / water.size.xy;
    let p = i.world.xy / max(water.ripple.y, 1e-3);
    let t = water.ripple.x;
    let a = textureSample(wnorm, wsmp, p + vec2<f32>(t * 0.011, t * 0.007)).xy * 2.0 - 1.0;
    let b = textureSample(wnorm, wsmp, p * 1.7 + vec2<f32>(-t * 0.008, t * 0.012)).xy * 2.0 - 1.0;
    let rippling = water.ripple.w > 0.5;
    if (rippling) {
        let slope = (a + b) * 0.5;
        n = normalize(vec3<f32>(slope, 1.0));
        at = clamp(at + slope * water.ripple.z, vec2<f32>(0.0), vec2<f32>(1.0));
    }
    let c = 1.0 - clamp(abs(dot(e, n)), 0.0, 1.0);
    let f = water.size.z + (1.0 - water.size.z) * c * c * c * c * c;
    let r = textureSampleLevel(refl, smp, at, 0.0);
    var s = r.rgb * (f * (1.0 - i.opacity) * r.a * water.size.w);
    if (rippling && water.sun.w > 0.0) {
        let h = normalize(e - water.sun.xyz);
        s = s + vec3<f32>(pow(max(dot(n, h), 0.0), 400.0) * water.sun.w * (1.0 - i.opacity));
    }
    return vec4<f32>(min(s, vec3<f32>(1.0)), 1.0);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MirrorU {
    eye: [f32; 4],
    f: [f32; 4],
    r: [f32; 4],
    u: [f32; 4],
    /// The tangents across and up, the plane's height and reach.
    tan: [f32; 4],
    /// The picture's width and height, the reflection's.
    size: [f32; 4],
    /// The depth buffer's near and far planes.
    planes: [f32; 4],
}

/// The reflection's picture and the places' winners, kept from frame to frame at `size` (the frame's own thread and the
/// presenter's pictures between frames draw them in turn, each mirroring and reading within one submission).
struct Target {
    size: (u32, u32),
    tex: wgpu::Texture,
    hash: wgpu::Buffer,
}

pub(crate) struct Reflections {
    /// Rift Apart's water normal for the ripples (`texnames::water_normal`, set with each level's load; `flat` while none),
    /// its repeating sampler, and a flat normal.
    ripple_normal: std::sync::Mutex<Option<wgpu::TextureView>>,
    flat: wgpu::TextureView,
    repeat: wgpu::Sampler,
    project: wgpu::ComputePipeline,
    resolve: wgpu::ComputePipeline,
    mirror_layout: wgpu::BindGroupLayout,
    water: wgpu::RenderPipeline,
    water_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    target: std::sync::Mutex<Option<Target>>,
}

impl Reflections {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, view_layout: &wgpu::BindGroupLayout) -> Reflections {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("water reflections, mirrored"), source: wgpu::ShaderSource::Wgsl(MIRROR.into()) });
        let entry = |binding: u32, ty: wgpu::BindingType| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::COMPUTE, ty, count: None };
        let mirror_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("water reflection, mirrored"),
            entries: &[
                entry(0, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
                entry(1, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }),
                entry(2, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }),
                entry(3, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None }),
                entry(4, wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: wgpu::TextureFormat::Rgba8Unorm, view_dimension: wgpu::TextureViewDimension::D2 }),
            ],
        });
        let cpl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("water reflection, mirrored"), bind_group_layouts: &[Some(&mirror_layout)], immediate_size: 0 });
        let compute = |entry: &str| device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some(entry), layout: Some(&cpl), module: &module, entry_point: Some(entry), compilation_options: Default::default(), cache: crate::pipelines::cache() });
        let (project, resolve) = (compute("project"), compute("resolve"));
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("water reflections"), source: wgpu::ShaderSource::Wgsl(WATER.into()) });
        let water_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("water reflection"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 4, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("water reflections"), bind_group_layouts: &[Some(view_layout), Some(&water_layout)], immediate_size: 0 });
        let attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4, 3 => Float32];
        let water = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("water reflections"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<crate::objects::ObjVertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs })],
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            // Where the water itself was drawn: its own depth or nearer than what it covers; no depth written.
            depth_stencil: Some(wgpu::DepthStencilState { format: crate::native::DEPTH, depth_write_enabled: Some(false), depth_compare: Some(wgpu::CompareFunction::GreaterEqual), stencil: Default::default(), bias: Default::default() }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                // A screen of the sheen over the water: the picture + the sheen x (1 - the picture), so nothing of the water is
                // replaced and the light added never passes white; the frame's alpha kept.
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(wgpu::BlendState { color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::OneMinusDst, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add }, alpha: wgpu::BlendComponent::REPLACE }),
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
            }),
            multiview_mask: None,
            cache: crate::pipelines::cache(),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("water reflection"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        use wgpu::util::DeviceExt;
        let flat = device
            .create_texture_with_data(queue, &wgpu::TextureDescriptor { label: Some("flat water normal"), size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm, usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] }, wgpu::util::TextureDataOrder::LayerMajor, &[128, 128, 255, 255])
            .create_view(&Default::default());
        let repeat = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("water normal"), address_mode_u: wgpu::AddressMode::Repeat, address_mode_v: wgpu::AddressMode::Repeat, mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, mipmap_filter: wgpu::MipmapFilterMode::Linear, anisotropy_clamp: 16, ..Default::default() });
        Reflections { ripple_normal: std::sync::Mutex::new(None), flat, repeat, project, resolve, mirror_layout, water, water_layout, sampler, target: std::sync::Mutex::new(None) }
    }

    /// The reflection's size for a native picture of `picture` pixels at level `level` (2 high: the picture's own; 1 low: half).
    fn size(picture: (u32, u32), level: u8) -> (u32, u32) {
        match level {
            2 => picture,
            _ => (picture.0.div_ceil(2), picture.1.div_ceil(2)),
        }
    }

    /// The reflection's picture and places at `size`, made again when the size changes.
    fn target(&self, device: &wgpu::Device, size: (u32, u32)) -> (wgpu::Texture, wgpu::Buffer) {
        let mut t = self.target.lock().expect("the water reflection's target");
        if t.as_ref().is_none_or(|t| t.size != size) {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("water reflection"),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let hash = device.create_buffer(&wgpu::BufferDescriptor { label: Some("water reflection places"), size: 4 * size.0 as u64 * size.1 as u64, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            *t = Some(Target { size, tex, hash });
        }
        let t = t.as_ref().expect("made above");
        (t.tex.clone(), t.hash.clone())
    }

    /// Mirrors `picture` (the native picture as drawn so far, `size` pixels, its depth `depth`) seen from `cam` in `plane` into
    /// the reflection at level `q` (`MIRROR`: its places cleared, every pixel above the water put at its mirror image's place,
    /// each place's winner's colour read).
    #[allow(clippy::too_many_arguments)]
    pub fn mirror(&self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, picture: &wgpu::Texture, depth: &wgpu::Texture, size: (u32, u32), cam: &Camera, plane: Plane, q: u8) {
        use wgpu::util::DeviceExt;
        let rs = Reflections::size(size, q);
        let (tex, hash) = self.target(device, rs);
        let (f, up) = (cam.forward, cam.up);
        let r = [f[1] * up[2] - f[2] * up[1], f[2] * up[0] - f[0] * up[2], f[0] * up[1] - f[1] * up[0]];
        let v4 = |a: [f32; 3]| [a[0], a[1], a[2], 0.0];
        let u = MirrorU { eye: v4(cam.eye), f: v4(f), r: v4(r), u: v4(up), tan: [cam.tan[0], cam.tan[1], plane.h, plane.tol], size: [size.0 as f32, size.1 as f32, rs.0 as f32, rs.1 as f32], planes: [crate::native::NEAR, crate::native::FAR, 0.0, 0.0] };
        let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("water reflection, mirrored"), contents: bytemuck::bytes_of(&u), usage: wgpu::BufferUsages::UNIFORM });
        let (pv, dv, tv) = (picture.create_view(&Default::default()), depth.create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() }), tex.create_view(&Default::default()));
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("water reflection, mirrored"),
            layout: &self.mirror_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&pv) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&dv) },
                wgpu::BindGroupEntry { binding: 3, resource: hash.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&tv) },
            ],
        });
        // No place won yet: 0, below every key.
        enc.clear_buffer(&hash, 0, None);
        let mut p = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("water reflection, mirrored"), timestamp_writes: None });
        p.set_bind_group(0, &bg, &[]);
        p.set_pipeline(&self.project);
        p.dispatch_workgroups(size.0.div_ceil(8), size.1.div_ceil(8), 1);
        p.set_pipeline(&self.resolve);
        p.dispatch_workgroups(rs.0.div_ceil(8), rs.1.div_ceil(8), 1);
    }

    /// The reflection's picture at level `q` for a native picture of `picture` pixels, and its size: the ray-traced reflections
    /// (`rtlight::water`) put what the mirrored rays meet into it after `mirror`.
    pub fn traced(&self, device: &wgpu::Device, picture: (u32, u32), q: u8) -> (wgpu::Texture, (u32, u32)) {
        let rs = Reflections::size(picture, q);
        (self.target(device, rs).0, rs)
    }

    /// The level's water normal for the ripples (`texnames::water_normal`: Rift Apart's, with UPDATED TEXTURES on; None
    /// off), set as each level loads.
    pub fn set_ripple_normal(&self, view: Option<wgpu::TextureView>) {
        *self.ripple_normal.lock().expect("the ripples' normal") = view;
    }

    /// The water's group 1 for a native picture of `picture` pixels: the reflection at level `q`, its sampler, the picture's size,
    /// F0 and the sheen's strength (`STRENGTH`); with a ripple normal (`set_ripple_normal`), the ripples at field `field` and
    /// the level's sun's glint (`sun`: None, no glint).
    pub fn water_group(&self, device: &wgpu::Device, picture: (u32, u32), q: u8, field: u64, sun: Option<crate::shafts::Sun>) -> wgpu::BindGroup {
        use wgpu::util::DeviceExt;
        let (tex, _) = self.target(device, Reflections::size(picture, q));
        let normal = self.ripple_normal.lock().expect("the ripples' normal").clone();
        let ripple = [field as f32 / 60.0, RIPPLE_TILE, RIPPLE_SHIFT, if normal.is_some() { 1.0 } else { 0.0 }];
        let glint = sun.map_or([0.0; 4], |s| [s.direction[0], s.direction[1], s.direction[2], GLINT * s.colour.iter().cloned().fold(0.0, f32::max)]);
        let u: [f32; 12] = [picture.0 as f32, picture.1 as f32, F0, STRENGTH[q.min(2) as usize], ripple[0], ripple[1], ripple[2], ripple[3], glint[0], glint[1], glint[2], glint[3]];
        let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("water reflection"), contents: bytemuck::cast_slice(&u), usage: wgpu::BufferUsages::UNIFORM });
        let view = tex.create_view(&Default::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("water reflection"),
            layout: &self.water_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: ub.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(normal.as_ref().unwrap_or(&self.flat)) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.repeat) },
            ],
        })
    }

    /// Draws the water's triangles `range` of `vb` (world positions) with the reflection (`group`, `water_group`) over the
    /// picture's pass `pass` (its view bound at group 0).
    pub fn draw<'a, E: crate::replay::Enc<'a>>(&'a self, pass: &mut E, vb: &'a wgpu::Buffer, range: std::ops::Range<u32>, group: &'a wgpu::BindGroup) {
        pass.set_pipeline(&self.water);
        pass.set_bind_group(1, group);
        pass.set_vertex_buffer(0, vb);
        pass.draw(range);
    }
}
