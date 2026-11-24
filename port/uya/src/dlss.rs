//! DLSS, NVIDIA's Deep Learning Super Sampling, on the world (the DLSS option: `settings::OPTIONS`, `enhance::dlss`), through
//! `dlss_wgpu` on wgpu's Vulkan backend.
//!
//! - **Where it runs in a picture** (`replay::Cmd::Upscale`): the world (the sky, the terrain, the ties, the shrubs, the mobies,
//!   the effects, the EE's own triangles in the world, the ray-traced passes, the occlusion, the shade, the water's reflection and
//!   the light shafts) is drawn at DLSS's render size into the scene's own colour and depth (`Upscaler`), DLSS makes the output
//!   picture from it, and what the EE's packets draw after the effect pass (the glow's passes and the 2D layer: the HUD, the
//!   menus over the world) is drawn at the output's size onto DLSS's picture, so the HUD stays as sharp as before. The glow reads
//!   the picture's alpha (the pixels above 0x80), so DLSS upscales the alpha too (`AlphaUpscaling`). The output's depth is the
//!   scene's taken to its size (`fs_depth`), for any world triangle the EE draws after the effects.
//! - **The camera's jitter**: each picture's projection is moved by a sub-pixel offset (DLSS's Halton sequence over its phases),
//!   which DLSS takes out again; the screen-space 2D (`vs_screen`) is not moved.
//! - **Motion** (`Shot`): for every pixel where its surface was in the picture shown before, in render pixels. The static world,
//!   the effects and the EE's triangles move with the camera only: from the depth, the point the pixel shows, put where the
//!   camera of the picture before saw it (`fs_camera`; the sky's pixels by direction alone). The mobies move themselves: each
//!   vertex carries where frame N - 1 drew it (`objects` `VInMoby::prev`, the pictures between frames' own data), so a picture
//!   `back` of the way toward it draws the vertex there, and the picture before at its own share (`back_prev`, past 1 across a
//!   frame: the frame's motion carried on). Their opaque draws are drawn again into the motion with the frame's depth (equal
//!   where they show), each pixel's motion from its two places (`objects` `vs_moby_motion`).
//! - **History**: a cut (`replay::same_shot`), a level change, a picture whose time is before the last one's or more than
//!   `replay::MAX_GAP` fields after it, or a new size restarts DLSS's history.

use crate::native::Camera;
use dlss_wgpu::super_resolution::{DlssSuperResolution, DlssSuperResolutionExposure, DlssSuperResolutionRenderParameters};
use dlss_wgpu::{DlssFeatureFlags, DlssPerfQualityMode, DlssSdk};
use std::sync::{Arc, Mutex};
use wgpu::util::DeviceExt;

/// The port's DLSS project id (NGX names an application by it).
pub(crate) const PROJECT: uuid::Uuid = uuid::Uuid::from_u128(0x7a3c_51e2_9d04_4b8f_a6e1_3f2b_c80d_5e19);

/// The motion's format (render pixels toward the picture before).
const MOTION: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;

/// The DLSS option's levels (`settings` `dlss`): 0 off, then DLAA (the output's own size, anti-aliasing only), QUALITY, BALANCED
/// and PERFORMANCE.
pub(crate) fn quality(level: u8) -> Option<DlssPerfQualityMode> {
    match level {
        0 => None,
        1 => Some(DlssPerfQualityMode::Dlaa),
        2 => Some(DlssPerfQualityMode::Quality),
        3 => Some(DlssPerfQualityMode::Balanced),
        4 => Some(DlssPerfQualityMode::Performance),
        l => panic!("dlss: level {l}"),
    }
}

/// A wgpu instance with the Vulkan instance extensions NGX asks for (Vulkan only: DLSS's backend), and whether this PC's driver
/// offers DLSS.
pub(crate) fn instance() -> Result<(wgpu::Instance, dlss_wgpu::FeatureSupport), String> {
    // Streamline first (`streamline.rs`): it becomes the Vulkan loader of this instance, for FRAME GENERATION at any time.
    crate::streamline::load();
    let mut support = dlss_wgpu::FeatureSupport::default();
    let instance = dlss_wgpu::create_instance(PROJECT, &wgpu::InstanceDescriptor::new_without_display_handle(), &mut support).map_err(|e| format!("native: the Vulkan instance with DLSS's extensions: {e}"))?;
    Ok((instance, support))
}

/// The device of `adapter` with the device extensions NGX asks for.
pub(crate) fn device(adapter: &wgpu::Adapter, desc: &wgpu::DeviceDescriptor, support: &mut dlss_wgpu::FeatureSupport) -> Result<(wgpu::Device, wgpu::Queue), String> {
    dlss_wgpu::request_device(PROJECT, adapter, desc, support, Some(desc.required_limits.clone())).map_err(|e| format!("native: GPU device with DLSS's extensions: {e}"))
}

/// NGX on `device` (once a run, when the option is first on): it loads `nvngx_dlss.dll` from beside the executable.
pub(crate) fn sdk(device: &wgpu::Device) -> Result<Arc<Mutex<DlssSdk>>, String> {
    DlssSdk::new(PROJECT, device.clone()).map_err(|e| format!("native: DLSS (NGX, nvngx_dlss.dll beside uya.exe): {e}"))
}

/// One picture's motion and jitter (`MotionU`), as its passes read it.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct MotionU {
    /// The picture's view and projection without the jitter, and the picture before's.
    cur: [[f32; 4]; 4],
    prev: [[f32; 4]; 4],
    /// The camera's right, up, forward rows and eye.
    r: [f32; 4],
    u: [f32; 4],
    f: [f32; 4],
    eye: [f32; 4],
    /// Its tangents, and the render size.
    tan: [f32; 4],
    /// The jitter: how far the picture is moved (render pixels, right and down); the output's size.
    jit: [f32; 4],
    /// How far back toward frame N - 1 a moby vertex is drawn in this picture and in the picture before (`objects`
    /// `vs_moby_motion`).
    back: [f32; 4],
}

const SHADER: &str = r#"
struct M { cur: mat4x4<f32>, prev: mat4x4<f32>, r: vec4<f32>, u: vec4<f32>, f: vec4<f32>, eye: vec4<f32>, tan: vec4<f32>, jit: vec4<f32>, back: vec4<f32> };
@group(0) @binding(0) var<uniform> m: M;
@group(0) @binding(1) var depth: texture_depth_2d;

@vertex fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

// The pixel's motion with the camera: the point its depth puts it at (at the pixel's place without the jitter), where the camera
// of the picture before saw it, less where it is now.
@fragment fn fs_camera(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let d = textureLoad(depth, vec2<i32>(at.xy), 0);
    let size = m.tan.zw;
    let c = at.xy - m.jit.xy;
    let s = vec2<f32>(c.x / size.x * 2.0 - 1.0, 1.0 - c.y / size.y * 2.0);
    let dir = m.f.xyz + s.x * m.tan.x * m.r.xyz + s.y * m.tan.y * m.u.xyz;
    var q: vec4<f32>;
    if (d <= 0.0) {
        // The sky (the depth's clear): a direction only.
        q = m.prev * vec4<f32>(dir, 0.0);
    } else {
        let z = NEAR * FAR / (NEAR + d * (FAR - NEAR));
        q = m.prev * vec4<f32>(m.eye.xyz + dir * z, 1.0);
    }
    // A point at or behind the picture before's camera had no place in it (`objects` `motion_px`): it came from off the picture.
    // A point just in front of that camera's plane divides into a motion past the half floats' range, an infinity DLSS spreads
    // over the whole picture: every motion stays within twice the picture's size.
    if (q.w <= 1e-6) { return vec4<f32>(2.0 * size.x, 0.0, 0.0, 0.0); }
    let n = q.xy / q.w;
    let p = vec2<f32>((n.x + 1.0) * 0.5 * size.x, (1.0 - n.y) * 0.5 * size.y);
    return vec4<f32>(clamp(p - c, -2.0 * size, 2.0 * size), 0.0, 0.0);
}

// The scene's depth at the output's size: each output pixel the render pixel under it.
@fragment fn fs_depth(@builtin(position) at: vec4<f32>) -> @builtin(frag_depth) f32 {
    let k = m.tan.zw / m.jit.zw;
    return textureLoad(depth, vec2<i32>(floor(at.xy * k)), 0);
}
"#;

/// The device's DLSS passes (made once: `Native::open`).
pub(crate) struct Pipes {
    layout: wgpu::BindGroupLayout,
    camera: wgpu::RenderPipeline,
    depth: wgpu::RenderPipeline,
    /// The scene at its render size drawn into a texture of the picture's size (`Upscaler::scaled_copy`), and its sampler.
    copy: wgpu::RenderPipeline,
    linear: wgpu::Sampler,
}

/// A picture copy of the scene at another size (`Upscaler::scaled_copy`): the source sampled across the target.
const COPY: &str = r#"
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
struct VOut { @builtin(position) p: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> VOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VOut;
    o.p = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    return o;
}
@fragment fn fs(i: VOut) -> @location(0) vec4<f32> {
    return textureSampleLevel(src, smp, i.uv, 0.0);
}
"#;

impl Pipes {
    pub fn new(device: &wgpu::Device) -> Pipes {
        let source = SHADER.replace("NEAR", &format!("{:?}", crate::native::NEAR)).replace("FAR", &format!("{:?}", crate::native::FAR));
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("dlss"), source: wgpu::ShaderSource::Wgsl(source.into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("dlss"),
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("dlss"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let vertex = wgpu::VertexState { module: &module, entry_point: Some("vs_full"), compilation_options: Default::default(), buffers: &[] };
        let camera = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("dlss camera motion"),
            layout: Some(&pl),
            vertex: vertex.clone(),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs_camera"), compilation_options: Default::default(), targets: &[Some(MOTION.into())] }),
            multiview_mask: None,
            cache: None,
        });
        let copy_module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("dlss scaled copy"), source: wgpu::ShaderSource::Wgsl(COPY.into()) });
        let copy = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("dlss scaled copy"),
            layout: None,
            vertex: wgpu::VertexState { module: &copy_module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &copy_module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())] }),
            multiview_mask: None,
            cache: None,
        });
        let linear = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("dlss scaled copy"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let depth = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("dlss output depth"),
            layout: Some(&pl),
            vertex,
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState { format: crate::native::DEPTH, depth_write_enabled: Some(true), depth_compare: Some(wgpu::CompareFunction::Always), stencil: Default::default(), bias: Default::default() }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs_depth"), compilation_options: Default::default(), targets: &[] }),
            multiview_mask: None,
            cache: None,
        });
        Pipes { layout, camera, depth, copy, linear }
    }
}

/// What the pictures before left for the next: the jitter's place in its sequence, and the last picture's camera and time.
struct History {
    index: u32,
    last: Option<(Camera, f64)>,
}

/// DLSS at one mode and output size: its context, its render size, and the scene's own targets at that size (shared by every
/// picture: the GPU runs the pictures in order, each drawing them whole).
pub(crate) struct Upscaler {
    ctx: Mutex<DlssSuperResolution>,
    adapter: wgpu::Adapter,
    pipes: Arc<Pipes>,
    pub level: u8,
    pub render: (u32, u32),
    pub out: (u32, u32),
    pub colour: wgpu::Texture,
    pub colour_view: wgpu::TextureView,
    pub depth: wgpu::Texture,
    pub depth_view: wgpu::TextureView,
    depth_only: wgpu::TextureView,
    motion_view: wgpu::TextureView,
    history: Mutex<History>,
}

/// One picture's DLSS inputs: its jitter (render pixels the picture is moved by), DLSS's offset for it, whether DLSS's history
/// restarts, and its motion's uniforms for the camera's pass and the mobies' (`objects` group 2).
pub(crate) struct Shot {
    pub jitter: [f32; 2],
    offset: [f32; 2],
    reset: bool,
    /// The picture's frame for DLSS Frame Generation (`streamline::begin`), when it runs.
    fg: Option<u32>,
    camera_bg: wgpu::BindGroup,
    pub moby_bg: wgpu::BindGroup,
}

impl Upscaler {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, adapter: &wgpu::Adapter, sdk: &Arc<Mutex<DlssSdk>>, pipes: &Arc<Pipes>, level: u8, out: (u32, u32)) -> Result<Upscaler, String> {
        let mode = quality(level).ok_or("dlss: an upscaler with DLSS off")?;
        let flags = DlssFeatureFlags::LowResolutionMotionVectors | DlssFeatureFlags::InvertedDepth | DlssFeatureFlags::AlphaUpscaling;
        let ctx = DlssSuperResolution::new([out.0, out.1], mode, flags, sdk.clone(), device, queue).map_err(|e| format!("dlss: {mode:?} at {}x{}: {e}", out.0, out.1))?;
        // The mode's share of the output's size each way (NVIDIA's: DLAA 1, QUALITY 1 / 1.5, BALANCED 0.58, PERFORMANCE 0.5),
        // within the sizes the context takes: NGX's own optimum came back at PERFORMANCE's for QUALITY on this PC (1281 x 720
        // for 2562 x 1440).
        let share = [1.0, 1.0 / 1.5, 0.58, 0.5][level as usize - 1];
        let range = ctx.render_resolution_range();
        let (lo, hi) = (range.start(), range.end());
        let render = (((out.0 as f64 * share).round() as u32).clamp(lo[0], hi[0]), ((out.1 as f64 * share).round() as u32).clamp(lo[1], hi[1]));
        println!("dlss: NGX's optimum {:?}, sizes {:?} to {:?}", ctx.render_resolution(), lo, hi);
        let texture = |label, size: (u32, u32), format, usage| device.create_texture(&wgpu::TextureDescriptor { label: Some(label), size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format, usage, view_formats: &[] });
        let colour = texture("dlss scene", render, wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST);
        let depth = texture("dlss scene depth", render, crate::native::DEPTH, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC);
        // COPY_SRC: DLSS Frame Generation's copy of the motion (`streamline::tag`).
        let motion = texture("dlss motion", render, MOTION, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC);
        let depth_only = depth.create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() });
        println!("dlss: {mode:?}, {}x{} drawn, {}x{} shown", render.0, render.1, out.0, out.1);
        Ok(Upscaler {
            ctx: Mutex::new(ctx),
            adapter: adapter.clone(),
            pipes: pipes.clone(),
            level,
            render,
            out,
            colour_view: colour.create_view(&Default::default()),
            colour,
            depth_view: depth.create_view(&Default::default()),
            depth_only,
            depth,
            motion_view: motion.create_view(&Default::default()),
            history: Mutex::new(History { index: 0, last: None }),
        })
    }

    /// The next picture's DLSS inputs: camera `cam` at the game's time `time` (fields), its mobies `back` of the way toward frame
    /// N - 1 over the frame's `span` fields, `cut` when it begins a shot. Pictures are asked for in the order they are shown.
    pub fn shot(&self, device: &wgpu::Device, motion_layout: &wgpu::BindGroupLayout, cam: &Camera, time: f64, back: f32, span: f64, cut: bool) -> Shot {
        let mut h = self.history.lock().expect("dlss history lock");
        let ctx = self.ctx.lock().expect("dlss context lock");
        let offset = ctx.suggested_jitter(h.index, [self.render.0, self.render.1]);
        drop(ctx);
        h.index = h.index.wrapping_add(1);
        // DLSS's offset is the shift applied to the picture, in render pixels right and down (its programming guide, 3.7.3: the
        // jitter applied to the projection, in the motion vectors' coordinates).
        let jitter = offset;
        let (prev, back_prev, reset) = match h.last {
            Some((pc, pt)) if !cut && time >= pt && time - pt <= crate::replay::MAX_GAP as f64 && crate::replay::same_shot(&pc, cam, (time - pt).ceil().max(1.0) as u64) => (pc, back + ((time - pt) / span) as f32, false),
            _ => (*cam, back, true),
        };
        h.last = Some((*cam, time));
        drop(h);
        // DLSS Frame Generation's frame (Reflex's sleep waits here, on the thread that records the picture) and the camera it takes.
        let fg = crate::streamline::begin();
        if let Some(i) = fg {
            crate::streamline::constants(i, cam, &prev, offset, self.render, reset);
        }
        let (f, u) = (cam.forward, cam.up);
        let r = [f[1] * u[2] - f[2] * u[1], f[2] * u[0] - f[0] * u[2], f[0] * u[1] - f[1] * u[0]];
        let m = MotionU {
            cur: cam.matrix(),
            prev: prev.matrix(),
            r: [r[0], r[1], r[2], 0.0],
            u: [u[0], u[1], u[2], 0.0],
            f: [f[0], f[1], f[2], 0.0],
            eye: [cam.eye[0], cam.eye[1], cam.eye[2], 0.0],
            tan: [cam.tan[0], cam.tan[1], self.render.0 as f32, self.render.1 as f32],
            jit: [jitter[0], jitter[1], self.out.0 as f32, self.out.1 as f32],
            back: [back, back_prev, 0.0, 0.0],
        };
        let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("dlss motion"), contents: bytemuck::bytes_of(&m), usage: wgpu::BufferUsages::UNIFORM });
        let camera_bg = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("dlss camera motion"), layout: &self.pipes.layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.depth_only) }] });
        let moby_bg = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("dlss moby motion"), layout: motion_layout, entries: &[wgpu::BindGroupEntry { binding: 6, resource: ub.as_entire_binding() }] });
        Shot { jitter, offset, reset, fg, camera_bg, moby_bg }
    }

    /// The scene `from` (its render size) drawn into `to`, a texture of the picture's size (RGBA8, renderable), stretched over it:
    /// a copy of the picture taken while the world is drawn at the render size (`replay::Cmd::CopyPicture`). Copied texel for
    /// texel it filled only `to`'s top left and left the rest a picture before, and an effect that draws the copy over the screen
    /// showed both (the player, 17 November 2026: "Glitch", the whole scene in the top left two thirds).
    pub fn scaled_copy(&self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, from: &wgpu::Texture, to: &wgpu::Texture) {
        let src = from.create_view(&Default::default());
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("dlss scaled copy"), layout: &self.pipes.copy.get_bind_group_layout(0), entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&src) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.pipes.linear) }] });
        let view = to.create_view(&Default::default());
        let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("dlss scaled copy"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        p.set_pipeline(&self.pipes.copy);
        p.set_bind_group(0, &bg, &[]);
        p.draw(0..3, 0..1);
    }

    /// The textures' gradient scale with DLSS (`native::ViewU::lerp` z): the render width over the output's, so a texture is
    /// sampled at the level the output's own size would pick (DLSS's guide, 3.5: no more than log2 of it as the bias).
    pub fn lod_scale(&self) -> f32 {
        self.render.0 as f32 / self.out.0 as f32
    }

    /// `jitter` (render pixels, right and down) as the change to a view and projection's clip x and y: x by 2 / width of w, y by
    /// -2 / height of w.
    pub fn jitter_ndc(&self, jitter: [f32; 2]) -> [f32; 2] {
        [2.0 * jitter[0] / self.render.0 as f32, -2.0 * jitter[1] / self.render.1 as f32]
    }

    /// The scene drawn so far (`colour`, `depth`) made into `output` (the output's size) by DLSS, with the motion first (the
    /// camera's, then the mobies' `moby` draws over it with the frame's view `view_bg`), and the scene's depth into `out_depth`
    /// after. DLSS's commands are a command buffer of their own: `enc`'s so far go to `done` (finished on an encoder's thread),
    /// then DLSS's, and `enc` starts again.
    #[allow(clippy::too_many_arguments)]
    pub fn run(&self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, done: &mut crate::replay::Chunks, shot: &Shot, view_bg: &wgpu::BindGroup, moby: &[crate::replay::Cmd], output: &wgpu::TextureView, out_depth: &wgpu::TextureView) -> Result<(), String> {
        {
            let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("dlss camera motion"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &self.motion_view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            p.set_pipeline(&self.pipes.camera);
            p.set_bind_group(0, &shot.camera_bg, &[]);
            p.draw(0..3, 0..1);
        }
        if !moby.is_empty() {
            let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("dlss moby motion"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &self.motion_view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &self.depth_view, depth_ops: None, stencil_ops: None }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            p.set_bind_group(0, view_bg, &[]);
            p.set_bind_group(2, &shot.moby_bg, &[]);
            crate::surface_buffer::commands(&mut p, moby);
        }
        let params = DlssSuperResolutionRenderParameters {
            color: &self.colour_view,
            depth: &self.depth_only,
            motion_vectors: &self.motion_view,
            exposure: DlssSuperResolutionExposure::Automatic,
            bias: None,
            dlss_output: output,
            reset: shot.reset,
            jitter_offset: shot.offset,
            // The scene's size (the crate takes the context's largest without it).
            partial_texture_size: Some([self.render.0, self.render.1]),
            motion_vector_scale: None,
        };
        let dlss = self.ctx.lock().map_err(|_| "dlss: the context's lock is poisoned")?.render(params, enc, &self.adapter).map_err(|e| format!("dlss: {e}"))?;
        let before = std::mem::replace(enc, device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("after dlss") }));
        done.encoder(before)?;
        done.buffer(dlss);
        // DLSS Frame Generation's inputs, tagged after the picture's world and DLSS (`streamline::tag`): the picture before the HUD,
        // which the steps after this one draw.
        if let Some(i) = shot.fg {
            match crate::streamline::tag(device, &self.adapter, i, &self.depth_only, &self.motion_view, output) {
                Ok(tags) => tags.into_iter().for_each(|t| done.buffer(t)),
                Err(e) => crate::streamline::fail(e),
            }
        }
        let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("dlss output depth"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: out_depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }), stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Store }) }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        p.set_pipeline(&self.pipes.depth);
        p.set_bind_group(0, &shot.camera_bg, &[]);
        p.draw(0..3, 0..1);
        Ok(())
    }
}
