//! Light shafts, the `light_shafts` option (PLAN 8.6; `settings::OPTIONS`, `enhance::shafts`): sunbeams through the level's fog,
//! from the level's own sun, as a
//! post-process over the world (after the mobies and the effects, before the 2D layer and the glow the game draws over them).
//!
//! - **The sun** is the level's first light: light set 0's first directional light in the gameplay block (`uyafmt::gameplay::
//!   LightSet`, the set the terrain's light pass uses by default), whose direction the light travels in (the game's shadows fall
//!   along it: Veldin's start, the sun to the right, Ratchet's shadow to the left) and whose colour is the sun's (1.0 = 0x80).
//! - **The beams** are where the air between the eye and a pixel is lit by the sun through gaps in what the frame drew: for each
//!   pixel, the share of the screen's path from it to the sun's place on the screen that shows the sky (the depth buffer's far
//!   plane: the sky writes no depth), after Mitchell, "Volumetric Light Scattering as a Post-Process", GPU Gems 3, 2007: each
//!   point of the path weighed by 0.97 a step from the pixel on high (64 steps), 0.94 on low (32): the same falloff, the far end
//!   about a seventh of the near, the weights' sum dividing (with every point weighed alike, as first made, the beams were a
//!   soft lift of the far haze with no shafts in it; a Fable agent judged the pictures: more structure, not more light). The
//!   beams are the sun's light: its colour, not the sky texture's (taken from the sky pixels, the first version's beams came out
//!   a sixth as bright, under the haze they light).
//! - **How much** is the game's fog: the air between the eye and the pixel scatters light toward the eye as densely as the
//!   frame's fog thickens (its share of the picture, `(255 - F) / 255`, grows by its slope between the fog's near and far
//!   depths), from the eye, up to the fog's thickest; a level with no fog has no beams. Measured from the fog's own start (where
//!   the game's fog begins, 57 units on Florana), the first version left everything nearer, the trees the sun shines through,
//!   with none. Times the cosine of the pixel's view ray to the sun (light scattered forward, toward the eye looking into it)
//!   and the sun's colour. The beams add to the picture; the frame's
//!   alpha byte (which the glow reads) is kept. A sun behind the eye gives none.
//!
//! Low marches 32 points a pixel at half the picture's height, High 64 at the picture's own size. It is a command of the frame's
//! recorded list (`replay::Cmd::Shafts`), so a picture drawn between two frames has its own beams from its own camera.

const SHADER: &str = r#"
// The sun's place on the screen (uv) and direction; the camera's forward, right and up rows and its tangents; the fog's near and
// far depths and F at them; the sun's colour; the target's size, the samples, the depth buffer's near and far planes.
struct U { sun: vec4<f32>, dir: vec4<f32>, f: vec4<f32>, r: vec4<f32>, u: vec4<f32>, tan: vec4<f32>, fog: vec4<f32>, colour: vec4<f32>, size: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(2) var depth: texture_depth_2d;
@group(0) @binding(3) var beams: texture_2d<f32>;
@group(0) @binding(4) var smp: sampler;

@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

fn texel(uv: vec2<f32>) -> vec2<i32> {
    let n = vec2<i32>(textureDimensions(depth));
    return clamp(vec2<i32>(uv * vec2<f32>(n)), vec2<i32>(0), n - vec2<i32>(1));
}

// The depth buffer's projection (`native::Camera::matrix`): reversed depth NEAR / (FAR - NEAR) x (FAR / z - 1), z the view depth.
fn view_z(d: f32) -> f32 {
    return u.size.z * u.size.w / (u.size.z + d * (u.size.w - u.size.z));
}

// The pixel's beams: the share of the screen's path from it to the sun that shows the sky, each point weighed by the decay a step
// (u.tan.w) from the pixel, the march's start turned per pixel (interleaved gradient noise).
@fragment fn fs_beams(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = at.xy / u.size.xy;
    let n = u32(u.tan.z);
    let noise = fract(52.9829189 * fract(dot(at.xy, vec2<f32>(0.06711056, 0.00583715))));
    // Only march the part of the ray whose depth we know. Clamping every off-screen sample to the edge made one tree or
    // rock there darken the entire distant view as the camera moved past it.
    let delta = u.sun.xy - uv;
    let edge = select(uv, vec2<f32>(1.0) - uv, delta > vec2<f32>(0.0)) / max(abs(delta), vec2<f32>(1e-5));
    let end = min(1.0, min(edge.x, edge.y));
    var sky = 0.0;
    var total = 0.0;
    var w = 1.0;
    for (var k = 0u; k < n; k = k + 1u) {
        let q = uv + delta * (end * (f32(k) + noise) / f32(n));
        if (textureLoad(depth, texel(q), 0) <= 0.0) {
            sky = sky + w;
        }
        total = total + w;
        w = w * u.tan.w;
    }
    let lit = sky / total;
    return vec4<f32>(lit, lit, lit, 1.0);
}

// The beams added to the picture: times the fog's share of the pixel, the cosine of its view ray to the sun and the sun's colour.
@fragment fn fs_apply(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = at.xy / vec2<f32>(textureDimensions(depth));
    let d = textureLoad(depth, texel(uv), 0);
    let z = select(view_z(d), u.fog.y, d <= 0.0);
    // The fog's share thickening by its slope from the eye to the view depth, up to its thickest.
    let slope = abs(u.fog.w - u.fog.z) / 255.0 / (u.fog.y - u.fog.x);
    let fogged = min(slope * z, 1.0 - min(u.fog.z, u.fog.w) / 255.0);
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let ray = normalize(u.f.xyz + u.r.xyz * ndc.x * u.tan.x + u.u.xyz * ndc.y * u.tan.y);
    // The sun's projection tends to infinity at 90 degrees. Fade before it passes behind the camera rather than cutting
    // a still-visible beam abruptly at that boundary.
    let facing = max(dot(ray, u.dir.xyz), 0.0) * smoothstep(0.0, 0.15, dot(u.f.xyz, u.dir.xyz));
    let b = textureSampleLevel(beams, smp, uv, 0.0).rgb;
    return vec4<f32>(b * u.colour.rgb * fogged * facing, 0.0);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ShaftU {
    sun: [f32; 4],
    dir: [f32; 4],
    f: [f32; 4],
    r: [f32; 4],
    u: [f32; 4],
    /// The tangents across and up, the samples a pixel and the decay a sample.
    tan: [f32; 4],
    fog: [f32; 4],
    colour: [f32; 4],
    /// The beams' target's width and height, the depth buffer's near and far planes.
    size: [f32; 4],
}

/// The level's sun: the direction its light travels in and its colour (`LightSet`'s, 1.0 = 0x80).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sun {
    pub direction: [f32; 3],
    pub colour: [f32; 3],
}

impl Sun {
    /// The level's sun from its gameplay block: light set 0's first light; None for a level with no light set.
    pub fn of(gameplay: &uyafmt::gameplay::Gameplay) -> Option<Sun> {
        let s = gameplay.light_sets.first()?;
        let d = s.direction[0];
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        (len > 0.0).then(|| Sun { direction: [d[0] / len, d[1] / len, d[2] / len], colour: [s.colour[0][0], s.colour[0][1], s.colour[0][2]] })
    }
}

pub(crate) struct Shafts {
    beams: wgpu::RenderPipeline,
    apply: wgpu::RenderPipeline,
    /// The beams' pass reads the picture's depth; the apply pass the depth and the beams.
    beams_layout: wgpu::BindGroupLayout,
    apply_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The beams' picture, kept from frame to frame (the frame's own thread and the presenter's pictures between frames draw it
    /// in turn, each within one submission).
    target: std::sync::Mutex<Option<((u32, u32), wgpu::Texture)>>,
}

impl Shafts {
    pub fn new(device: &wgpu::Device) -> Shafts {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("light shafts"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let tex = |binding: u32, sample_type: wgpu::TextureSampleType| wgpu::BindGroupLayoutEntry {
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
        let beams_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("light shafts' beams"), entries: &[uniform, tex(2, wgpu::TextureSampleType::Depth)] });
        let apply_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("light shafts, applied"),
            entries: &[
                uniform,
                tex(2, wgpu::TextureSampleType::Depth),
                tex(3, wgpu::TextureSampleType::Float { filterable: true }),
                wgpu::BindGroupLayoutEntry { binding: 4, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let pipeline = |layout: &wgpu::BindGroupLayout, entry: &str, blend: Option<wgpu::BlendState>, write_mask: wgpu::ColorWrites| {
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(entry), bind_group_layouts: &[Some(layout)], immediate_size: 0 });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pl),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some(entry), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend, write_mask })] }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: crate::pipelines::cache(),
            })
        };
        // The beams added to the picture's colour; its alpha kept.
        let add = wgpu::BlendState { color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add }, alpha: wgpu::BlendComponent::REPLACE };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("light shafts"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        Shafts { beams: pipeline(&beams_layout, "fs_beams", None, wgpu::ColorWrites::ALL), apply: pipeline(&apply_layout, "fs_apply", Some(add), wgpu::ColorWrites::COLOR), beams_layout, apply_layout, sampler, target: std::sync::Mutex::new(None) }
    }

    /// Adds the beams of `sun` to `picture` (the native picture, `size` pixels; its depth `depth`) at level `q` (1 low, 2 high), seen
    /// from `cam` in fog `fog`; nothing when the sun is behind the eye.
    #[allow(clippy::too_many_arguments)]
    pub fn run(&self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, picture: &wgpu::Texture, depth: &wgpu::Texture, size: (u32, u32), cam: &crate::native::Camera, fog: &uyafmt::gameplay::Fog, near_far: (f32, f32), sun: Sun, q: u8) {
        use wgpu::util::DeviceExt;
        let (f, up) = (cam.forward, cam.up);
        let r = [f[1] * up[2] - f[2] * up[1], f[2] * up[0] - f[0] * up[2], f[0] * up[1] - f[1] * up[0]];
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        // Toward the sun: against the light's travel.
        let s = sun.direction.map(|c| -c);
        let ahead = dot(s, f);
        if ahead <= 0.0 {
            return;
        }
        let ndc = [dot(s, r) / ahead / cam.tan[0], dot(s, up) / ahead / cam.tan[1]];
        let (beams_size, samples, decay) = match q {
            2 => (size, 64.0, 0.97),
            _ => (((size.0 + 1) / 2, (size.1 + 1) / 2), 32.0, 0.94),
        };
        let mut target = self.target.lock().expect("the light shafts' target");
        if target.as_ref().is_none_or(|t| t.0 != beams_size) {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("light shafts"),
                size: wgpu::Extent3d { width: beams_size.0, height: beams_size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            *target = Some((beams_size, tex));
        }
        let beams = target.as_ref().expect("made above").1.clone();
        drop(target);
        let v4 = |a: [f32; 3], w: f32| [a[0], a[1], a[2], w];
        let u = ShaftU {
            sun: [(ndc[0] + 1.0) / 2.0, (1.0 - ndc[1]) / 2.0, 0.0, 0.0],
            dir: v4(s, 0.0),
            f: v4(f, 0.0),
            r: v4(r, 0.0),
            u: v4(up, 0.0),
            tan: [cam.tan[0], cam.tan[1], samples, decay],
            fog: [fog.near, fog.far, fog.near_f, fog.far_f],
            colour: v4(sun.colour, 0.0),
            size: [beams_size.0 as f32, beams_size.1 as f32, near_far.0, near_far.1],
        };
        let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("light shafts"), contents: bytemuck::bytes_of(&u), usage: wgpu::BufferUsages::UNIFORM });
        let (pv, dv, bv) = (picture.create_view(&Default::default()), depth.create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() }), beams.create_view(&Default::default()));
        let beams_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("light shafts' beams"),
            layout: &self.beams_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&dv) },
            ],
        });
        let apply_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("light shafts, applied"),
            layout: &self.apply_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&dv) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&bv) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        // The beams first (reading the depth), then onto the picture (reading the beams).
        for (view, pipe, bg) in [(&bv, &self.beams, &beams_bg), (&pv, &self.apply, &apply_bg)] {
            let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("light shafts"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            p.set_pipeline(pipe);
            p.set_bind_group(0, bg, &[]);
            p.draw(0..3, 0..1);
        }
    }
}
