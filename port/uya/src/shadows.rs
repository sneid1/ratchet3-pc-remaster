//! Real-time shadows, the `shadows` option (PLAN 8.4): what stands between a surface and the level's key light shades it, from
//! shadow maps of the light.
//!
//! **The light.** The game lights its world with light sets of two directional lights each (`uyafmt::gameplay::LightSet`; the
//! terrain light pass, `lighting.rs`, and the mobies' skin job light by them). The key light is the brighter light of the set the
//! most terrain vertices are lit by (`key_light`: their sets' nibbles, weighted as the light pass blends them): the sun of an
//! outdoor level. A light set's direction is the way its light travels (the light pass dots it with the inward normal), so the
//! maps look along it.
//!
//! **How dark.** The game's own shadows (the mobies' shadow volumes, `eedraw.rs`) keep three quarters of a pixel's colour (the
//! darken sprites' FIX 0x20: (0 - Cd) x 0x20 / 0x80 + Cd). A pixel in the key light's shadow keeps the same three quarters, where
//! its surface faces the light (a surface facing away gets none of the light already, and is not darkened); with the option the
//! game's own darken steps are drawn with FIX 0 (`Step::Darken`: the counting and the alpha as the game's, the colour kept), so
//! a moby's shadow is the map's and never darkens twice.
//!
//! **The maps.** Three layers of one depth texture, orthographic along the light (`Cascade`): two of the static world (the
//! terrain loaded, every tie the level shows at its finest LOD, `objects::Ties::casters`, built once a level, and every shrub,
//! `objects::Shrubs::casters`), `EXTENT[0]` and `EXTENT[1]` units either side of a centre near the camera, drawn again only when
//! the camera has moved a quarter of that from it (the static world does not move, and a centre on the maps' texel grid keeps
//! their edges still), on a submission of their own before the frame's (`LevelShadows::frame`); and one of the mobies (their
//! opaque draws, as the ambient occlusion's occluders), `EXTENT[2]` either side of the frame's eye, a command of the frame's
//! recorded list (`replay::Cmd::ShadowCasters`), so a picture between two frames draws it with the mobies where it draws them
//! (`vs_mobies`: `view.lerp.x`). Every caster is alpha-tested with its own texture and TEST_1 AREF, as it draws. The maps'
//! matrices are one uniform (`ShadowFrame::cascades`) the shade reads when it is drawn.
//!
//! **Applied** as the ambient occlusion is (`ao.rs`), commands of the frame's recorded list (`replay::Cmd::Shadows`): after the
//! static world, over every pixel (`fs_static`), and after the moby runs over the mobies' pixels (`fs_mobies`, the occluders'
//! mask), each pixel put back in the world from the depth buffer with the picture's own camera (a picture between frames its
//! own, the field of view's too) and looked up in the nearest static map that holds it (3 x 3 texels, each casting only within `REACH`) and in the mobies'
//! map; faded at the outer tenth of the far maps and with the frame's fog, as the occlusion is. The effects, the EE's
//! primitives after the effect pass and the 2D layer draw after it, untouched.

/// The maps' half widths in world units: the near and far static maps, the mobies'.
const EXTENT: [f32; 3] = [24.0, 120.0, 32.0];
/// The depth the maps hold either side of their centre along the light, in world units.
const DEPTH_RANGE: f32 = 2048.0;
/// How far a shadow reaches from what casts it, in world units along the light (`lit_in`: fading from half of it): Ratchet's
/// shadow, a rock's or a wall's foot are cast; a roof or a canopy high over the ground is not, so an interior lit by the level's
/// sun keeps the game's light (level 10's hall, level 28's tunnels went a quarter darker whole without it).
pub(crate) const REACH: f32 = 8.0;
/// How much of a pixel's colour the shadow takes: the game's darken sprites' FIX 0x20 of 0x80 (`eedraw.rs`, "The shadows").
pub(crate) const DARKNESS: f32 = 0x20 as f32 / 128.0;
/// The map size (texels a side) at the option's levels low, medium and high.
pub(crate) const SIZES: [u32; 3] = [1024, 2048, 4096];

const CASTER_SHADER: &str = r#"
struct Light { m: mat4x4<f32> };
struct Mat { k: f32, last: f32, qlog: f32, aref: f32, region: vec4<f32>, extra: vec4<f32>, rift: vec4<f32>, tiles: vec4<f32>, tiles3: vec4<f32>, grains: vec4<f32>, specs: vec4<f32>, refls: vec4<f32>, furs: vec4<f32>, catches: vec4<f32>, metals: vec4<f32>, emit: vec4<f32> };
@group(0) @binding(0) var<uniform> light: Light;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var smp: sampler;
@group(1) @binding(2) var<uniform> mat: Mat;

struct Out { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32>, @location(1) alpha: f32 };

// The ties' casters, the shrubs and the mobies (`objects::ObjVertex`, `ShrubVertex`: S, T, Q).
@vertex fn vs_obj(@location(0) pos: vec3<f32>, @location(1) st: vec3<f32>, @location(2) colour: vec4<f32>) -> Out {
    var o: Out;
    o.clip = light.m * vec4<f32>(pos, 1.0);
    o.uv = st.xy / st.z;
    o.alpha = colour.a * 255.0;
    return o;
}

// The terrain (`native::Vertex`).
@vertex fn vs_terrain(@location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) colour: vec4<f32>) -> Out {
    var o: Out;
    o.clip = light.m * vec4<f32>(pos, 1.0);
    o.uv = uv;
    o.alpha = colour.a * 255.0;
    return o;
}

// The caster's filtered, modulated alpha. Mip zero aliases fine cut-outs as they move across the shadow map.
@fragment fn fs(i: Out) {
    let uv = clamp(i.uv, mat.region.xz, mat.region.yw);
    let a = floor(round(textureSample(tex, smp, uv).a * 255.0) * round(i.alpha) / 128.0);
    if (a < mat.aref) { discard; }
}
"#;

const MOBY_CASTER_SHADER: &str = r#"
struct View { m: mat4x4<f32>, sky: mat4x4<f32>, eye: vec4<f32>, forward: vec4<f32>, fog_colour: vec4<f32>, fog: vec4<f32>, lerp: vec4<f32> };
struct Light { m: mat4x4<f32> };
struct Mat { k: f32, last: f32, qlog: f32, aref: f32, region: vec4<f32>, extra: vec4<f32>, rift: vec4<f32>, tiles: vec4<f32>, tiles3: vec4<f32>, grains: vec4<f32>, specs: vec4<f32>, refls: vec4<f32>, furs: vec4<f32>, catches: vec4<f32>, metals: vec4<f32>, emit: vec4<f32> };
@group(0) @binding(0) var<uniform> view: View;
@group(1) @binding(0) var<uniform> light: Light;
@group(2) @binding(0) var tex: texture_2d<f32>;
@group(2) @binding(1) var smp: sampler;
@group(2) @binding(2) var<uniform> mat: Mat;

struct Out { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32>, @location(1) alpha: f32 };

// The moby runs' opaque draws where the picture draws them (`objects` `vs_moby`: `view.lerp.x` of the way back to the frame before).
@vertex fn vs_mobies(@location(0) pos: vec3<f32>, @location(1) st: vec3<f32>, @location(2) colour: vec4<f32>, @location(4) prev: vec3<f32>) -> Out {
    var o: Out;
    let p = pos + (prev - pos) * view.lerp.x;
    o.clip = light.m * vec4<f32>(p, 1.0);
    o.uv = st.xy / st.z;
    o.alpha = colour.a * 255.0;
    return o;
}

fn alpha(i: Out) -> f32 {
    let uv = clamp(i.uv, mat.region.xz, mat.region.yw);
    return floor(round(textureSample(tex, smp, uv).a * 255.0) * round(i.alpha) / 128.0);
}

@fragment fn fs(i: Out) {
    if (alpha(i) < mat.aref) { discard; }
}

// Match the main picture's opaque-only depth pass for alpha-blended characters, including vertex fades.
@fragment fn fs_opaque(i: Out) {
    if (alpha(i) < max(mat.aref, 128.0)) { discard; }
}
"#;

const APPLY_SHADER: &str = r#"
struct U {
    r: vec4<f32>, u: vec4<f32>, f: vec4<f32>, eye: vec4<f32>,
    // The camera's tangents and the picture's size.
    tan: vec4<f32>,
    // The depth buffer's near and far planes, the darkness, unused.
    nf: vec4<f32>,
    // The fog's near and far view depths and F at them.
    fog: vec4<f32>,
};
// The maps (`ShadowFrame::cascades`).
struct C {
    // The direction towards the key light.
    to_light: vec4<f32>,
    m: array<mat4x4<f32>, 3>,
    // Each map's half width in world units (0: not drawn); w: the shadows' reach (`REACH`).
    ext: vec4<f32>,
    // Each map's texel in world units; w: a map's depth range in world units (2 `DEPTH_RANGE`).
    texel: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(5) var<uniform> cs: C;
@group(0) @binding(1) var depth: texture_depth_2d;
@group(0) @binding(2) var maps: texture_depth_2d_array;
@group(0) @binding(3) var cmp: sampler_comparison;
@group(0) @binding(4) var mask: texture_2d<f32>;

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

// A moby's own features no nearer the point than this along the light (world units) shade it from the mobies' map: a face, a
// hand or a helmet within a character's own depth casts nothing onto it. Their low-poly outlines and the map's texels at a
// cutscene's close-up shaded faces in hard triangles and hatching the game never had (the player, 22 November 2026: "weird
// texture triangle/shading on characters faces sometimes in cutscenes"); another character, a crate or the world beyond it
// still shade a moby.
const SELF_GAP: f32 = 0.35;
// The facing a moby's pixel takes the whole shadow from (a world surface's: 0.15). A character's normals here are its flat
// facets' (the depth buffer's), and its own lighting already darkens it toward the terminator: the shadow comes in over a wider
// turn there, so neighbouring facets do not switch between shaded and not.
const MOBY_FACING: f32 = 0.5;

// How much of the key light reaches world point `w` in map `k`: 1 lit, 0 in shadow, -1 outside the map. An occluder nearer the
// point than `gap_min` along the light casts nothing (fading in from half of it); 0 lets every one cast.
fn lit_in(k: u32, w: vec3<f32>, gap_min: f32) -> f32 {
    let c = cs.m[k] * vec4<f32>(w, 1.0);
    if (abs(c.x) > 1.0 || abs(c.y) > 1.0 || c.z < 0.0 || c.z > 1.0) {
        return -1.0;
    }
    let uv = vec2<f32>(c.x * 0.5 + 0.5, 0.5 - c.y * 0.5);
    let n = i32(textureDimensions(maps).x);
    let texel = uv * f32(n) - vec2<f32>(0.5);
    let at = vec2<i32>(floor(texel));
    let fr = fract(texel);
    var sum = 0.0;
    var weights = 0.0;
    // A continuous tent filter over the comparisons. The old equal-weight 3x3 box jumped to a different set of samples
    // whenever the camera moved a receiver across a map texel, making even a stationary shadow crawl.
    for (var y = -1; y <= 2; y = y + 1) {
        for (var x = -1; x <= 2; x = x + 1) {
            let o = textureLoad(maps, clamp(at + vec2<i32>(x, y), vec2<i32>(0), vec2<i32>(n - 1)), i32(k), 0);
            // How far before the point the occluder lies along the light, in world units: a shadow reaches `REACH` from what
            // casts it, fading over its second half (a roof high over a floor casts none).
            let gap = (c.z - o) * cs.texel.w;
            let w = (2.0 - abs(f32(x) - fr.x)) * (2.0 - abs(f32(y) - fr.y));
            let near = select(1.0, smoothstep(0.5 * gap_min, gap_min, gap), gap_min > 0.0);
            sum = sum + w * (1.0 - select(0.0, (1.0 - smoothstep(0.5 * cs.ext.w, cs.ext.w, gap)) * near, gap > 0.0));
            weights += w;
        }
    }
    let lit = sum / weights;
    // The far maps fade out over their outer tenth.
    let edge = smoothstep(0.9, 1.0, max(abs(c.x), abs(c.y)));
    return select(mix(lit, 1.0, edge), lit, k == 0u);
}

fn shade(at: vec2<f32>, moby: bool) -> f32 {
    let q = vec2<i32>(at.xy);
    let d = load_d(q);
    if (d <= 0.0) {
        return 1.0;
    }
    let p = world(at.xy, d);
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
    let facing = dot(n, cs.to_light.xyz);
    if (facing <= 0.0) {
        return 1.0;
    }
    // Each map looked up a texel and a half off the surface along its normal (no surface shades itself).
    var lit = 1.0;
    let near = lit_in(0u, p + n * cs.texel.x * 1.5, 0.0);
    if (near >= 0.0) {
        lit = near;
        // Blend the near map into the far map across its edge instead of abruptly switching resolutions there.
        let c = cs.m[0] * vec4<f32>(p + n * cs.texel.x * 1.5, 1.0);
        let edge = smoothstep(0.9, 1.0, max(abs(c.x), abs(c.y)));
        if (edge > 0.0) {
            let far = lit_in(1u, p + n * cs.texel.y * 1.5, 0.0);
            lit = mix(near, select(1.0, far, far >= 0.0), edge);
        }
    } else {
        let far = lit_in(1u, p + n * cs.texel.y * 1.5, 0.0);
        if (far >= 0.0) {
            lit = far;
        }
    }
    let m = lit_in(2u, p + n * cs.texel.z * 1.5, select(0.0, SELF_GAP, moby));
    if (m >= 0.0) {
        lit = min(lit, m);
    }
    // The share of the pixel that is not fog, as the occlusion's.
    let vz = dot(p - u.eye.xyz, u.f.xyz);
    let f = u.fog.z + (u.fog.w - u.fog.z) * (vz - u.fog.x) / (u.fog.y - u.fog.x);
    let clear = clamp(f, min(u.fog.z, u.fog.w), max(u.fog.z, u.fog.w)) / 255.0;
    return 1.0 - u.nf.z * (1.0 - lit) * smoothstep(0.0, select(0.15, MOBY_FACING, moby), facing) * clear;
}

@fragment fn fs_static(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let s = shade(at.xy, false);
    return vec4<f32>(s, s, s, 1.0);
}

@fragment fn fs_mobies(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    if (textureLoad(mask, vec2<i32>(at.xy), 0).r > 0.5) {
        return vec4<f32>(1.0);
    }
    let s = shade(at.xy, true);
    return vec4<f32>(s, s, s, 1.0);
}
"#;

/// The shade's per-picture uniform (`U` of `APPLY_SHADER`): the picture's camera, size, planes and fog.
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
}

/// The maps' uniform (`C` of `APPLY_SHADER`, `ShadowFrame::cascades`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CascadesU {
    to_light: [f32; 4],
    m: [[[f32; 4]; 4]; 3],
    ext: [f32; 4],
    texel: [f32; 4],
}

/// The pipelines (`Native::open`).
pub(crate) struct Shadows {
    obj: wgpu::RenderPipeline,
    terrain: wgpu::RenderPipeline,
    mobies: [wgpu::RenderPipeline; 2],
    light_layout: wgpu::BindGroupLayout,
    apply_static: wgpu::RenderPipeline,
    apply_mobies: wgpu::RenderPipeline,
    apply_layout: wgpu::BindGroupLayout,
    compare: wgpu::Sampler,
}

/// One map: its centre along the light's x and y and its matrix (world to the map's clip space).
#[derive(Clone, Copy)]
struct Cascade {
    centre: [f32; 2],
    matrix: [[f32; 4]; 4],
}

/// What the frame's shadow commands draw with (`replay::Cmd::ShadowCasters`, `Cmd::Shadows`): the maps, the mobies' map's layer
/// and its light, the maps' uniform.
pub(crate) struct ShadowFrame {
    array: wgpu::TextureView,
    mobies_layer: wgpu::TextureView,
    pub(crate) mobies_light: wgpu::BindGroup,
    cascades: wgpu::Buffer,
}

/// A level's shadows (`native::Gpu`): the key light, the ties' casters (built the first time the option draws), the maps.
pub(crate) struct LevelShadows {
    /// The key light's direction of travel and the light's axes (x, y across it, z along it); None: the level's sets light
    /// nothing.
    key: Option<[[f32; 3]; 3]>,
    ties: Option<(Option<wgpu::Buffer>, Option<wgpu::Buffer>, Vec<crate::objects::Caster>)>,
    maps: Option<Maps>,
}

struct Maps {
    size: u32,
    terrain: Option<u32>,
    layers: Vec<wgpu::TextureView>,
    cascades: [Option<Cascade>; 3],
    lights: Vec<(wgpu::Buffer, wgpu::BindGroup)>,
    frame: std::sync::Arc<ShadowFrame>,
}

/// The key light of a level: the brighter light of the set its terrain vertices are most lit by (`lighting::LitVertex::sets`:
/// set A, and set B by its weight), from the light sets as the light pass reads them (`lighting::Sets`).
pub(crate) fn key_light(vertices: &[crate::lighting::LitVertex], sets: &crate::lighting::Sets) -> Option<[f32; 3]> {
    let mut weight = [0f64; 16];
    for v in vertices {
        let (a, b, w) = ((v.sets & 15) as usize, ((v.sets >> 4) & 15) as usize, (v.sets >> 8) as f64 / 256.0);
        if w == 0.0 {
            weight[a] += 1.0;
        } else {
            weight[a] += 1.0 - w;
            weight[b] += w;
        }
    }
    let set = (0..16).max_by(|&a, &b| weight[a].total_cmp(&weight[b]))?;
    let rows = &sets.rows[4 * set..4 * set + 4];
    let luma = |c: [f32; 4]| 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
    let (colour, dir) = if luma(rows[0]) >= luma(rows[2]) { (rows[0], rows[1]) } else { (rows[2], rows[3]) };
    let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    (luma(colour) > 0.05 && len > 0.5).then(|| [dir[0] / len, dir[1] / len, dir[2] / len])
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn normalize(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt();
    [a[0] / l, a[1] / l, a[2] / l]
}

/// A depth-only caster pipeline: both faces cast (the GS draws both), a slope bias against acne.
#[allow(clippy::too_many_arguments)]
fn caster(device: &wgpu::Device, layout: &wgpu::PipelineLayout, module: &wgpu::ShaderModule, vs: &str, fs: &str, buffers: &[Option<wgpu::VertexBufferLayout<'_>>]) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(vs),
        layout: Some(layout),
        vertex: wgpu::VertexState { module, entry_point: Some(vs), compilation_options: Default::default(), buffers },
        primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: wgpu::DepthBiasState { constant: 0, slope_scale: 1.5, clamp: 0.0 },
        }),
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState { module, entry_point: Some(fs), compilation_options: Default::default(), targets: &[] }),
        multiview_mask: None,
        cache: crate::pipelines::cache(),
    })
}

impl Shadows {
    pub fn new(device: &wgpu::Device, view_layout: &wgpu::BindGroupLayout, mat_layout: &wgpu::BindGroupLayout) -> Shadows {
        let casters = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("shadow casters"), source: wgpu::ShaderSource::Wgsl(CASTER_SHADER.into()) });
        let moby_casters = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("moby shadow casters"), source: wgpu::ShaderSource::Wgsl(MOBY_CASTER_SHADER.into()) });
        let uniform = |binding: u32, visibility: wgpu::ShaderStages| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let light_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("shadow light"), entries: &[uniform(0, wgpu::ShaderStages::VERTEX)] });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("shadow casters"), bind_group_layouts: &[Some(&light_layout), Some(mat_layout)], immediate_size: 0 });
        let mpl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("moby shadow casters"), bind_group_layouts: &[Some(view_layout), Some(&light_layout), Some(mat_layout)], immediate_size: 0 });
        let obj_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4];
        let prev_attrs = wgpu::vertex_attr_array![4 => Float32x3];
        let terrain_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Unorm8x4];
        let obj_buffer = wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<crate::objects::ObjVertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &obj_attrs };
        let obj = caster(device, &pl, &casters, "vs_obj", "fs", &[Some(obj_buffer.clone())]);
        let terrain = caster(device, &pl, &casters, "vs_terrain", "fs", &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<crate::native::Vertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &terrain_attrs })]);
        let mobies = ["fs", "fs_opaque"].map(|fs| caster(device, &mpl, &moby_casters, "vs_mobies", fs, &[Some(obj_buffer.clone()), Some(wgpu::VertexBufferLayout { array_stride: 12, step_mode: wgpu::VertexStepMode::Vertex, attributes: &prev_attrs })]));
        let apply = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("shadows applied"), source: wgpu::ShaderSource::Wgsl(APPLY_SHADER.into()) });
        let texture = |binding: u32, sample_type: wgpu::TextureSampleType, view_dimension: wgpu::TextureViewDimension| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type, view_dimension, multisampled: false },
            count: None,
        };
        let apply_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadows applied"),
            entries: &[
                uniform(0, wgpu::ShaderStages::FRAGMENT),
                texture(1, wgpu::TextureSampleType::Depth, wgpu::TextureViewDimension::D2),
                texture(2, wgpu::TextureSampleType::Depth, wgpu::TextureViewDimension::D2Array),
                wgpu::BindGroupLayoutEntry { binding: 3, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison), count: None },
                texture(4, wgpu::TextureSampleType::Float { filterable: false }, wgpu::TextureViewDimension::D2),
                uniform(5, wgpu::ShaderStages::FRAGMENT),
            ],
        });
        let apl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("shadows applied"), bind_group_layouts: &[Some(&apply_layout)], immediate_size: 0 });
        // The picture's colour times the shade; its alpha kept (the frame's alpha, which the glow reads).
        let multiply = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::Src, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
        };
        let applied = |entry: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&apl),
                vertex: wgpu::VertexState { module: &apply, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                fragment: Some(wgpu::FragmentState { module: &apply, entry_point: Some(entry), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: Some(multiply), write_mask: wgpu::ColorWrites::ALL })] }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: crate::pipelines::cache(),
            })
        };
        let compare = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow maps"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        Shadows { obj, terrain, mobies, light_layout, apply_static: applied("fs_static"), apply_mobies: applied("fs_mobies"), apply_layout, compare }
    }

    /// The mobies' opaque draws (`Blend::Alpha`, `Blend::Replace`, as the ambient occlusion's occluders) into the mobies' map, after
    /// `replay::Rec::shadow_casters` (vertex buffers 0 and 1: the frame's moby vertices and their places in the frame before).
    pub fn draw_mobies<'a, E: crate::replay::Enc<'a>>(&'a self, e: &mut E, materials: &'a crate::objects::Materials, draws: &[crate::objects::Draw]) -> Result<(), String> {
        for dr in draws.iter().filter(|d| matches!(d.key.5, crate::objects::Blend::Alpha | crate::objects::Blend::Replace)) {
            e.set_pipeline(&self.mobies[usize::from(dr.key.5 == crate::objects::Blend::Alpha)]);
            let (bg, _) = materials.keys.get(&dr.key).ok_or("native: a moby texture setting made at load is missing")?;
            e.set_bind_group(2, bg);
            e.draw(dr.first..dr.first + dr.count);
        }
        Ok(())
    }

    /// The mobies' map's pass (`replay::Cmd::ShadowCasters`), cleared (`clear`; else kept, a pass begun again after a part of the
    /// picture's list: `replay::play`), with the picture's view (group 0) and the map's light.
    pub fn casters_pass(&self, enc: &mut wgpu::CommandEncoder, f: &ShadowFrame, view_bg: &wgpu::BindGroup, clear: bool) -> wgpu::RenderPass<'static> {
        let mut p = enc
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("moby shadow map"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &f.mobies_layer, depth_ops: Some(wgpu::Operations { load: if clear { wgpu::LoadOp::Clear(1.0) } else { wgpu::LoadOp::Load }, store: wgpu::StoreOp::Store }), stencil_ops: None }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            })
            .forget_lifetime();
        p.set_bind_group(0, view_bg, &[]);
        p.set_bind_group(1, &f.mobies_light, &[]);
        p
    }

    /// The shade over `target` (the picture, `size` pixels, its depth `depth`), seen from `cam` (`replay::Cmd::Shadows`): after the
    /// static world every pixel (`mobies` false), after the moby runs the mask's (the occluders' pass, `ao::mask`).
    #[allow(clippy::too_many_arguments)]
    pub fn apply(&self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, f: &ShadowFrame, depth: &wgpu::Texture, mask: &wgpu::TextureView, target: &wgpu::TextureView, size: (u32, u32), cam: &crate::native::Camera, fog: &uyafmt::gameplay::Fog, near_far: (f32, f32), mobies: bool) {
        use wgpu::util::DeviceExt;
        let w4 = |a: [f32; 3]| [a[0], a[1], a[2], 0.0];
        let u = ApplyU {
            r: w4(cross(cam.forward, cam.up)),
            u: w4(cam.up),
            f: w4(cam.forward),
            eye: w4(cam.eye),
            tan: [cam.tan[0], cam.tan[1], size.0 as f32, size.1 as f32],
            nf: [near_far.0, near_far.1, DARKNESS, 0.0],
            fog: [fog.near, fog.far, fog.near_f, fog.far_f],
        };
        let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("shadows applied"), contents: bytemuck::bytes_of(&u), usage: wgpu::BufferUsages::UNIFORM });
        let dv = depth.create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadows applied"),
            layout: &self.apply_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&dv) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&f.array) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.compare) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(mask) },
                wgpu::BindGroupEntry { binding: 5, resource: f.cascades.as_entire_binding() },
            ],
        });
        let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shadows applied"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: target, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        p.set_pipeline(if mobies { &self.apply_mobies } else { &self.apply_static });
        p.set_bind_group(0, &group, &[]);
        p.draw(0..3, 0..1);
    }
}

/// The static world's casters for one map (`LevelShadows::frame`): the terrain's vertices, indices and draws (with each draw's
/// material group), the shrubs (`objects::Shrubs::casters`, with their materials) and the ties'.
pub(crate) struct Statics<'a> {
    pub terrain_id: u32,
    pub terrain: (&'a wgpu::Buffer, &'a wgpu::Buffer, Vec<(&'a wgpu::BindGroup, std::ops::Range<u32>)>),
    pub shrubs: Option<(&'a crate::objects::Shrubs, &'a crate::objects::ShrubGlobals)>,
    pub ties: Option<(&'a crate::objects::Ties, &'a crate::objects::TieGlobals)>,
}

/// A map's matrix (world to its clip space: x and y across the light over `r` either side of `centre`, z along it over
/// `DEPTH_RANGE` either side of `z0`), column-major.
fn cascade_matrix(axes: &[[f32; 3]; 3], centre: [f32; 2], z0: f32, r: f32) -> [[f32; 4]; 4] {
    let (x, y, f) = (axes[0], axes[1], axes[2]);
    let rows = [[x[0] / r, x[1] / r, x[2] / r, -centre[0] / r], [y[0] / r, y[1] / r, y[2] / r, -centre[1] / r], [f[0] / (2.0 * DEPTH_RANGE), f[1] / (2.0 * DEPTH_RANGE), f[2] / (2.0 * DEPTH_RANGE), 0.5 - z0 / (2.0 * DEPTH_RANGE)], [0.0, 0.0, 0.0, 1.0]];
    std::array::from_fn(|c| std::array::from_fn(|j| rows[j][c]))
}

impl LevelShadows {
    pub fn new(key: Option<[f32; 3]>) -> LevelShadows {
        let key = key.map(|f| {
            let up = if f[2].abs() > 0.99 { [0.0, 1.0, 0.0] } else { [0.0, 0.0, 1.0] };
            let x = normalize(cross(up, f));
            let y = cross(f, x);
            [x, y, f]
        });
        LevelShadows { key, ties: None, maps: None }
    }

    /// The maps for the frame seen from `eye` (`level` the option's: 1 low, 2 medium, 3 high): the static maps drawn again where
    /// the camera has left them (`statics`, recorded on commands of their own), the mobies' map placed about the eye (its casters
    /// are the frame's recorded commands), the maps' uniform written. None when the level has no key light. The maps, their
    /// matrices and the uniform are the level's, shared by every frame: their writes and the static maps' commands go into
    /// `outbox`, which is applied in the frames' order before the frame's first picture.
    #[allow(clippy::too_many_arguments)]
    pub fn frame(&mut self, s: &Shadows, device: &wgpu::Device, outbox: &mut crate::native::Outbox, ram: &[u8], level: u8, eye: [f32; 3], statics: Statics<'_>) -> Result<Option<std::sync::Arc<ShadowFrame>>, String> {
        let Some(axes) = self.key else { return Ok(None) };
        let size = SIZES[level as usize - 1];
        if self.maps.as_ref().is_none_or(|m| m.size != size) {
            self.maps = Some(Maps::new(device, &s.light_layout, size));
        }
        // The ties' casters, once a level.
        if self.ties.is_none()
            && let Some((ties, g)) = statics.ties
        {
            use wgpu::util::DeviceExt;
            let (v, i, c) = ties.casters(ram, g)?;
            let vb = (!v.is_empty()).then(|| device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("tie casters"), contents: bytemuck::cast_slice(&v), usage: wgpu::BufferUsages::VERTEX }));
            let ib = (!i.is_empty()).then(|| device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("tie caster indices"), contents: bytemuck::cast_slice(&i), usage: wgpu::BufferUsages::INDEX }));
            self.ties = Some((vb, ib, c));
        }
        let maps = self.maps.as_mut().expect("made above");
        // A streamed terrain chunk can change while the eye remains inside the cached cascades. Redraw those maps even
        // without camera movement; otherwise the previous chunk keeps casting shadows until the eye crosses a cascade.
        if maps.terrain != Some(statics.terrain_id) {
            maps.cascades = [None; 3];
            maps.terrain = Some(statics.terrain_id);
        }
        let light = [dot(eye, axes[0]), dot(eye, axes[1])];
        let z0 = dot(eye, axes[2]);
        let mut enc: Option<wgpu::CommandEncoder> = None;
        for k in 0..3 {
            let texel = 2.0 * EXTENT[k] / size as f32;
            let moved = maps.cascades[k].is_none_or(|c| (light[0] - c.centre[0]).abs().max((light[1] - c.centre[1]).abs()) > EXTENT[k] / 4.0);
            if k < 2 && !moved {
                continue;
            }
            // The centre on the map's texel grid.
            let centre = [(light[0] / texel).round() * texel, (light[1] / texel).round() * texel];
            let matrix = cascade_matrix(&axes, centre, z0, EXTENT[k]);
            maps.cascades[k] = Some(Cascade { centre, matrix });
            outbox.write(&maps.lights[k].0, 0, bytemuck::cast_slice(&matrix));
            if k == 2 {
                break;
            }
            let enc = enc.get_or_insert_with(|| device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("static shadow maps") }));
            let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("static shadow map"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &maps.layers[k], depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            p.set_bind_group(0, &maps.lights[k].1, &[]);
            let (x, y, r) = (axes[0], axes[1], EXTENT[k]);
            // Whether a caster's sphere reaches the map (across the light: along it the depth range holds the level).
            let reaches = |c: [f32; 3], rad: f32| (dot(c, x) - centre[0]).abs() <= r + rad && (dot(c, y) - centre[1]).abs() <= r + rad;
            p.set_pipeline(&s.terrain);
            p.set_vertex_buffer(0, statics.terrain.0.slice(..));
            p.set_index_buffer(statics.terrain.1.slice(..), wgpu::IndexFormat::Uint32);
            for (bg, range) in &statics.terrain.2 {
                p.set_bind_group(1, *bg, &[]);
                p.draw_indexed(range.clone(), 0, 0..1);
            }
            p.set_pipeline(&s.obj);
            if let (Some((ties, _)), Some((Some(vb), Some(ib), casters))) = (statics.ties, &self.ties) {
                p.set_vertex_buffer(0, vb.slice(..));
                p.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                for c in casters.iter().filter(|c| reaches(c.centre, c.radius)) {
                    for (test, d) in &c.draws {
                        let (bg, _) = ties.materials[*test].keys.get(&d.key).ok_or("shadows: a tie texture setting made at load is missing")?;
                        p.set_bind_group(1, bg, &[]);
                        p.draw_indexed(d.first..d.first + d.count, 0, 0..1);
                    }
                }
            }
            if let Some((shrubs, g)) = statics.shrubs {
                let mut bound: Option<*const wgpu::Buffer> = None;
                let mut err = None;
                shrubs.casters(ram, g, |vb, key, range, centre, radius| {
                    if err.is_some() || !reaches(centre, radius) {
                        return;
                    }
                    let Some((bg, _)) = shrubs.materials.keys.get(&key) else {
                        err = Some("shadows: a shrub texture setting made at load is missing".to_string());
                        return;
                    };
                    if bound != Some(vb as *const _) {
                        p.set_vertex_buffer(0, vb.slice(..));
                        bound = Some(vb as *const _);
                    }
                    p.set_bind_group(1, bg, &[]);
                    p.draw(range, 0..1);
                })?;
                if let Some(e) = err {
                    return Err(e);
                }
            }
        }
        if let Some(enc) = enc {
            outbox.commands.push(enc.finish());
        }
        let mut u = CascadesU { to_light: [-axes[2][0], -axes[2][1], -axes[2][2], 0.0], m: [[[0.0; 4]; 4]; 3], ext: [0.0, 0.0, 0.0, REACH], texel: [0.0, 0.0, 0.0, 2.0 * DEPTH_RANGE] };
        for k in 0..3 {
            if let Some(c) = maps.cascades[k] {
                u.m[k] = c.matrix;
                u.ext[k] = EXTENT[k];
            }
            u.texel[k] = 2.0 * EXTENT[k] / size as f32;
        }
        outbox.write(&maps.frame.cascades, 0, bytemuck::bytes_of(&u));
        Ok(Some(maps.frame.clone()))
    }
}

impl Maps {
    fn new(device: &wgpu::Device, light_layout: &wgpu::BindGroupLayout, size: u32) -> Maps {
        use wgpu::util::DeviceExt;
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow maps"),
            size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 3 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let layers: Vec<wgpu::TextureView> = (0..3)
            .map(|k| tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2), base_array_layer: k, array_layer_count: Some(1), ..Default::default() }))
            .collect();
        let array = tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let lights: Vec<(wgpu::Buffer, wgpu::BindGroup)> = (0..3)
            .map(|_| {
                let b = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("shadow light"), contents: bytemuck::cast_slice(&[[0f32; 4]; 4]), usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST });
                let g = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("shadow light"), layout: light_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: b.as_entire_binding() }] });
                (b, g)
            })
            .collect();
        let cascades = device.create_buffer(&wgpu::BufferDescriptor { label: Some("shadow maps' matrices"), size: std::mem::size_of::<CascadesU>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let frame = std::sync::Arc::new(ShadowFrame { array, mobies_layer: layers[2].clone(), mobies_light: lights[2].1.clone(), cascades });
        Maps { size, terrain: None, layers, cascades: [None; 3], lights, frame }
    }
}
