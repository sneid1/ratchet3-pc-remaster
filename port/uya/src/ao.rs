//! Ambient occlusion, the `ambient_occlusion` option (`enhance::ao`): each pixel of the level's static world (sky, terrain, ties,
//! shrubs) and of its mobies is darkened by how much of the hemisphere above it the depth buffer shows closed in (corners, the
//! foot of walls, crevices, the ground under a crate or where Ratchet stands). The effects, the EE's own primitives drawn after
//! the effect pass and the 2D layer draw after it, untouched.
//!
//! The frame's steps (`Native::render_frame`), when the static world was drawn:
//! - the occluders (`replay::Cmd::Occluders`, into `mask`), after the static world's passes and before the mobies: the frame's
//!   opaque moby draws (`objects::Pipelines::draw_occluders`, at the places the moby runs draw them, between two frames too)
//!   into the depth buffer, each writing 0 into a mask cleared to 1, so the occlusion sees the mobies;
//! - `begin`: the occlusion (`fs_ao`) from that depth, smoothed over 5 x 5 pixels of about the same depth, multiplies the
//!   picture (`fs_apply`): the static world takes the shade the mobies and itself cast;
//! - the mobies draw as they do without the option (at the depths their own occluder pass wrote: the same vertices through the
//!   same vertex shader, so their test passes);
//! - `mobies`, after the moby runs and before the effects: the same occlusion multiplies the pixels the mask marks (`fs_mobies`),
//!   the mobies' own shade. A glow, a fade or an added shine is no occluder and is not marked, so what is drawn over the world
//!   is not darkened (one over a moby's pixel is, with the moby).
//!
//! The occlusion: each pixel's view-space position (the depth buffer's projection, `native::NEAR` and `FAR`, and the camera's
//! tangents), its normal from the nearer neighbour on each axis, and 8 (low) or 16 (high) points of the hemisphere within
//! `RADIUS` world units, each turned per pixel, compared with the depth the buffer holds where each projects; it fades out with
//! the frame's fog (F / 255 at the pixel's depth: a fogged pixel is mostly fog colour, which nothing occludes). The multiply
//! leaves the picture's alpha (the frame's alpha, which the glow pass reads) as it was.

/// The hemisphere's radius in world units and how much of the occlusion darkens the picture.
const RADIUS: f32 = 1.0;
pub(crate) const STRENGTH: f32 = 0.7;

const SHADER: &str = r#"
struct U { tan: vec4<f32>, fog: vec4<f32>, size: vec4<f32>, opt: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var depth: texture_depth_2d;
@group(0) @binding(2) var occl: texture_2d<f32>;
@group(0) @binding(3) var mask: texture_2d<f32>;

@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

// The depth buffer's projection (`native::Camera::matrix`): reversed depth NEAR / (FAR - NEAR) x (FAR / z - 1), z the view depth.
fn view_z(d: f32) -> f32 {
    return u.size.z * u.size.w / (u.size.z + d * (u.size.w - u.size.z));
}

fn load_d(p: vec2<i32>) -> f32 {
    return textureLoad(depth, clamp(p, vec2<i32>(0), vec2<i32>(u.size.xy) - vec2<i32>(1)), 0);
}

// View space: x right, y up, z the view depth.
fn view_pos(px: vec2<f32>, d: f32) -> vec3<f32> {
    let z = view_z(d);
    let ndc = vec2<f32>(px.x / u.size.x * 2.0 - 1.0, 1.0 - px.y / u.size.y * 2.0);
    return vec3<f32>(ndc.x * u.tan.x * z, ndc.y * u.tan.y * z, z);
}

fn pos_at(q: vec2<i32>) -> vec3<f32> {
    return view_pos(vec2<f32>(q) + vec2<f32>(0.5), load_d(q));
}

@fragment fn fs_ao(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let q = vec2<i32>(at.xy);
    let d = load_d(q);
    if (d <= 0.0) {
        return vec4<f32>(1.0);
    }
    let p = view_pos(at.xy, d);
    let pl = pos_at(q - vec2<i32>(1, 0));
    let pr = pos_at(q + vec2<i32>(1, 0));
    let pu = pos_at(q - vec2<i32>(0, 1));
    let pd = pos_at(q + vec2<i32>(0, 1));
    let dx = select(pr - p, p - pl, abs(pl.z - p.z) < abs(pr.z - p.z));
    let dy = select(pd - p, p - pu, abs(pu.z - p.z) < abs(pd.z - p.z));
    var n = normalize(cross(dx, dy));
    if (dot(n, p) > 0.0) {
        n = -n;
    }
    let a = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), abs(n.x) > 0.9);
    let t0 = normalize(a - n * dot(a, n));
    let t1 = cross(n, t0);
    // Interleaved gradient noise turns each pixel's samples.
    let noise = fract(52.9829189 * fract(dot(at.xy, vec2<f32>(0.06711056, 0.00583715))));
    let r = u.tan.z;
    var occ = 0.0;
    let count = u32(u.opt.x);
    for (var k = 0u; k < count; k = k + 1u) {
        let fk = f32(k);
        let ang = noise * 6.2831853 + fk * 2.3999632;
        let rad = sqrt((fk + 0.5) / u.opt.x);
        let up = sqrt(1.0 - rad * rad);
        let reach = mix(0.15, 1.0, fract(fk * 0.618034 + noise));
        let s = p + (t0 * cos(ang) * rad + t1 * sin(ang) * rad + n * up) * r * reach * reach;
        if (s.z <= u.size.z) {
            continue;
        }
        let ndc = vec2<f32>(s.x / (s.z * u.tan.x), s.y / (s.z * u.tan.y));
        let sp = vec2<f32>((ndc.x + 1.0) * 0.5 * u.size.x, (1.0 - ndc.y) * 0.5 * u.size.y);
        if (sp.x < 0.0 || sp.y < 0.0 || sp.x >= u.size.x || sp.y >= u.size.y) {
            continue;
        }
        let sz = view_z(load_d(vec2<i32>(sp)));
        // The scene in front of the sample point closes it in, when it is within reach of the pixel.
        let near = smoothstep(0.0, 1.0, r / max(abs(p.z - sz), 0.0001));
        occ = occ + select(0.0, near, sz < s.z - 0.02 * r);
    }
    // The share of the pixel that is not fog: F / 255 at its depth (the terrain program's linear F, held beyond its ends).
    let f = u.fog.z + (u.fog.w - u.fog.z) * (p.z - u.fog.x) / (u.fog.y - u.fog.x);
    let clear = clamp(f, min(u.fog.z, u.fog.w), max(u.fog.z, u.fog.w)) / 255.0;
    let ao = 1.0 - occ / u.opt.x * u.tan.w * clear;
    return vec4<f32>(ao, ao, ao, 1.0);
}

// The occlusion smoothed over the 5 x 5 pixels around of about the same depth.
fn smoothed(q: vec2<i32>) -> f32 {
    let z0 = view_z(load_d(q));
    var sum = 0.0;
    var wsum = 0.0;
    for (var y = -2; y <= 2; y = y + 1) {
        for (var x = -2; x <= 2; x = x + 1) {
            let o = clamp(q + vec2<i32>(x, y), vec2<i32>(0), vec2<i32>(u.size.xy) - vec2<i32>(1));
            let w = select(0.0, 1.0, abs(view_z(load_d(o)) - z0) < 0.05 * z0);
            sum = sum + textureLoad(occl, o, 0).r * w;
            wsum = wsum + w;
        }
    }
    return sum / max(wsum, 1.0);
}

@fragment fn fs_apply(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let q = vec2<i32>(at.xy);
    if (load_d(q) <= 0.0) {
        return vec4<f32>(1.0);
    }
    let ao = smoothed(q);
    return vec4<f32>(ao, ao, ao, 1.0);
}

// The mobies' own pixels (the mask's 0s).
@fragment fn fs_mobies(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let q = vec2<i32>(at.xy);
    if (textureLoad(mask, q, 0).r > 0.5) {
        return vec4<f32>(1.0);
    }
    let ao = smoothed(q);
    return vec4<f32>(ao, ao, ao, 1.0);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct AoU {
    /// The camera's tangents across and up, the radius and the strength.
    tan: [f32; 4],
    /// The fog's near and far view depths and F at them.
    fog: [f32; 4],
    /// The picture's width and height, the depth buffer's near and far planes.
    size: [f32; 4],
    /// The samples a pixel (the option's level) and three unused.
    opt: [f32; 4],
}

/// The samples a pixel at the option's levels low and high.
const SAMPLES: [f32; 2] = [8.0, 16.0];

pub(crate) struct Ao {
    occlude: wgpu::RenderPipeline,
    apply: wgpu::RenderPipeline,
    mobies: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

/// A frame's occlusion between `begin` and `mobies`: the group the mobies' multiply reads (the occlusion, the depth, the mask).
pub(crate) struct AoFrame {
    group: wgpu::BindGroup,
}

impl Ao {
    pub fn new(device: &wgpu::Device) -> Ao {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("ambient occlusion"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let texture = |binding: u32, sample_type: wgpu::TextureSampleType| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let uniform = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let float = wgpu::TextureSampleType::Float { filterable: false };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("ambient occlusion"), entries: &[uniform, texture(1, wgpu::TextureSampleType::Depth), texture(2, float), texture(3, float)] });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("ambient occlusion"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = |entry: &str, format: wgpu::TextureFormat, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pl),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some(entry), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })] }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: crate::pipelines::cache(),
            })
        };
        // The picture's colour times the occlusion; its alpha kept.
        let multiply = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::Src, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
        };
        Ao {
            occlude: pipeline("fs_ao", wgpu::TextureFormat::R8Unorm, None),
            apply: pipeline("fs_apply", wgpu::TextureFormat::Rgba8Unorm, Some(multiply)),
            mobies: pipeline("fs_mobies", wgpu::TextureFormat::Rgba8Unorm, Some(multiply)),
            layout,
        }
    }

    /// After the static world and the mobies' occluders (`mask`): the occlusion of the depth `depth` (Depth32FloatStencil8,
    /// made with TEXTURE_BINDING) from camera `cam` in fog `fog`, at the option's level `level` (1 low, 2 high), multiplies
    /// `target` (the native picture, `size` pixels). Returns what `mobies` needs.
    #[allow(clippy::too_many_arguments)]
    pub fn begin(&self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, depth: &wgpu::Texture, mask: &wgpu::TextureView, target: &wgpu::TextureView, size: (u32, u32), cam: &crate::native::Camera, fog: &uyafmt::gameplay::Fog, near_far: (f32, f32), level: u8) -> AoFrame {
        use wgpu::util::DeviceExt;
        let u = AoU { tan: [cam.tan[0], cam.tan[1], RADIUS, STRENGTH], fog: [fog.near, fog.far, fog.near_f, fog.far_f], size: [size.0 as f32, size.1 as f32, near_far.0, near_far.1], opt: [SAMPLES[level as usize - 1], 0.0, 0.0, 0.0] };
        let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("ambient occlusion"), contents: bytemuck::bytes_of(&u), usage: wgpu::BufferUsages::UNIFORM });
        let ov = r8(device, "occlusion", size);
        let dv = depth.create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ambient occlusion"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&dv) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&ov) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(mask) },
            ],
        });
        // The occlusion pass reads the depth and writes `ov`, which its group must not bind: a group of its own with a blank in
        // the occlusion's and the mask's places.
        let blank = r8(device, "ambient occlusion: unused", size);
        let group_occlude = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ambient occlusion, occlusion pass"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&dv) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&blank) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&blank) },
            ],
        });
        full_pass(enc, &ov, &self.occlude, &group_occlude);
        full_pass(enc, target, &self.apply, &group);
        AoFrame { group }
    }

    /// After the moby runs: the occlusion multiplies the mobies' own pixels of `target`.
    pub fn mobies(&self, enc: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, f: &AoFrame) {
        full_pass(enc, target, &self.mobies, &f.group);
    }
}

/// An R8 texture of the picture's `size` to render into and read.
fn r8(device: &wgpu::Device, label: &str, size: (u32, u32)) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

/// The mask of the mobies' occluders for pictures of `size`: the moby runs' opaque draws (`objects::Pipelines::draw_occluders`,
/// as `Cmd::Occluders` records them after the static world) write 0 into it, cleared to 1, and their depths into the picture's,
/// so the occlusion sees the mobies and knows their pixels.
pub(crate) fn mask(device: &wgpu::Device, size: (u32, u32)) -> wgpu::TextureView {
    r8(device, "moby occluders", size)
}

/// One full-screen triangle of `pipeline` into `view`, keeping what it holds.
fn full_pass(enc: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, pipeline: &wgpu::RenderPipeline, group: &wgpu::BindGroup) {
    let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("ambient occlusion"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    p.set_pipeline(pipeline);
    p.set_bind_group(0, group, &[]);
    p.draw(0..3, 0..1);
}
