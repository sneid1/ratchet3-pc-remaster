//! A game frame drawn again between two of the game's own frames (PLAN 8.8, the frame rate above 60): the game's logic stays at
//! its 59.94 fields a second and `play` presents at the display's refresh rate, each picture between two of the game's frames.
//!
//! A frame's picture is made of what its build left on the GPU, so that drawing it again rebuilds nothing on the CPU (no tie,
//! shrub or terrain vertex, no chain walk):
//! - the static world (the sky and the terrain, the ties, the shrubs) as render bundles, recorded once on their threads
//!   (`Native::render`) and executed by every picture of the frame: the frame's own and each one drawn after it;
//! - the rest (the mobies, the effects, the EE's primitives, their passes into other frame buffers and the copies they read) as a
//!   list of commands (`Cmd`), recorded once and played into each picture's encoder: the frame's own picture is that list played
//!   once, so it is the picture the frame drew before this list existed;
//! - the frame's view (`native::ViewU`) in a uniform buffer every pass of the frame binds: a picture between frames writes its
//!   own view there before it is drawn.
//!
//! Between frame N-1 and frame N at a share `alpha` of the way, the picture is frame N's with:
//! - the camera between the two cameras (the eye along the line, the rows turned between them, the tangents between theirs):
//!   the terrain, the ties, the shrubs, the effects and every EE primitive the game put in the world (shadows, its own triangles)
//!   move with it, each where frame N put it;
//! - each moby vertex between where frame N-1 drew it and where frame N does, for the runs matched to their own run in frame
//!   N-1 (`mobies`: the same mesh, its origin the nearest): `vs_moby` takes the vertex `(1 - alpha)` of the way back;
//! - what frame N alone has (a moby run new in it, an effect, the HUD and every 2D primitive, the glow) as frame N drew it, and
//!   what frame N no longer draws not drawn.
//!
//! A picture is drawn between two frames only when both drew the world on the native picture of one level, in consecutive
//! fields, with no cut of the camera between them (`between`); else the frame's own picture is shown. Nothing in it is a guess of
//! what the game would draw later: every primitive is one of frame N's, at a place between two the game gave it.

use crate::native::Camera;
use std::ops::Range;

/// What draws a frame's triangles: a render pass, a render bundle (the static world) or the recorded list (`Rec`). The calls that
/// set the pass's own state (blend constant, stencil reference, scissor) are not a bundle's: they fail there.
pub(crate) trait Enc<'a> {
    fn set_pipeline(&mut self, p: &'a wgpu::RenderPipeline);
    fn set_bind_group(&mut self, index: u32, g: &'a wgpu::BindGroup);
    fn set_vertex_buffer(&mut self, slot: u32, b: &'a wgpu::Buffer);
    fn set_index_buffer(&mut self, b: &'a wgpu::Buffer);
    fn draw(&mut self, vertices: Range<u32>);
    fn draw_indexed(&mut self, indices: Range<u32>);
    fn draw_instanced(&mut self, vertices: Range<u32>, instances: Range<u32>);
    /// An indexed draw whose count and first corner the GPU wrote at `offset` of `args` (`veg_cull`).
    fn draw_indexed_indirect(&mut self, args: &'a wgpu::Buffer, offset: u64);
    fn set_blend_constant(&mut self, c: wgpu::Color) -> Result<(), String>;
    fn set_stencil_reference(&mut self, r: u32) -> Result<(), String>;
    fn set_scissor_rect(&mut self, r: [u32; 4]) -> Result<(), String>;
}

impl<'a> Enc<'a> for wgpu::RenderPass<'_> {
    fn set_pipeline(&mut self, p: &'a wgpu::RenderPipeline) {
        wgpu::RenderPass::set_pipeline(self, p);
    }
    fn set_bind_group(&mut self, index: u32, g: &'a wgpu::BindGroup) {
        wgpu::RenderPass::set_bind_group(self, index, g, &[]);
    }
    fn set_vertex_buffer(&mut self, slot: u32, b: &'a wgpu::Buffer) {
        wgpu::RenderPass::set_vertex_buffer(self, slot, b.slice(..));
    }
    fn set_index_buffer(&mut self, b: &'a wgpu::Buffer) {
        wgpu::RenderPass::set_index_buffer(self, b.slice(..), wgpu::IndexFormat::Uint32);
    }
    fn draw(&mut self, vertices: Range<u32>) {
        wgpu::RenderPass::draw(self, vertices, 0..1);
    }
    fn draw_indexed(&mut self, indices: Range<u32>) {
        wgpu::RenderPass::draw_indexed(self, indices, 0, 0..1);
    }
    fn draw_instanced(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        wgpu::RenderPass::draw(self, vertices, instances);
    }
    fn draw_indexed_indirect(&mut self, args: &'a wgpu::Buffer, offset: u64) {
        wgpu::RenderPass::draw_indexed_indirect(self, args, offset);
    }
    fn set_blend_constant(&mut self, c: wgpu::Color) -> Result<(), String> {
        wgpu::RenderPass::set_blend_constant(self, c);
        Ok(())
    }
    fn set_stencil_reference(&mut self, r: u32) -> Result<(), String> {
        wgpu::RenderPass::set_stencil_reference(self, r);
        Ok(())
    }
    fn set_scissor_rect(&mut self, r: [u32; 4]) -> Result<(), String> {
        wgpu::RenderPass::set_scissor_rect(self, r[0], r[1], r[2], r[3]);
        Ok(())
    }
}

impl<'a> Enc<'a> for wgpu::RenderBundleEncoder<'a> {
    fn set_pipeline(&mut self, p: &'a wgpu::RenderPipeline) {
        wgpu::RenderBundleEncoder::set_pipeline(self, p);
    }
    fn set_bind_group(&mut self, index: u32, g: &'a wgpu::BindGroup) {
        wgpu::RenderBundleEncoder::set_bind_group(self, index, g, &[]);
    }
    fn set_vertex_buffer(&mut self, slot: u32, b: &'a wgpu::Buffer) {
        wgpu::RenderBundleEncoder::set_vertex_buffer(self, slot, b.slice(..));
    }
    fn set_index_buffer(&mut self, b: &'a wgpu::Buffer) {
        wgpu::RenderBundleEncoder::set_index_buffer(self, b.slice(..), wgpu::IndexFormat::Uint32);
    }
    fn draw(&mut self, vertices: Range<u32>) {
        wgpu::RenderBundleEncoder::draw(self, vertices, 0..1);
    }
    fn draw_indexed(&mut self, indices: Range<u32>) {
        wgpu::RenderBundleEncoder::draw_indexed(self, indices, 0, 0..1);
    }
    fn draw_instanced(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        wgpu::RenderBundleEncoder::draw(self, vertices, instances);
    }
    fn draw_indexed_indirect(&mut self, args: &'a wgpu::Buffer, offset: u64) {
        wgpu::RenderBundleEncoder::draw_indexed_indirect(self, args, offset);
    }
    fn set_blend_constant(&mut self, _: wgpu::Color) -> Result<(), String> {
        Err("native: a blend constant in the static world's render bundle".into())
    }
    fn set_stencil_reference(&mut self, _: u32) -> Result<(), String> {
        Err("native: a stencil reference in the static world's render bundle".into())
    }
    fn set_scissor_rect(&mut self, _: [u32; 4]) -> Result<(), String> {
        Err("native: a scissor in the static world's render bundle".into())
    }
}

/// One recorded command of a frame's picture after its static world (`Rec`).
pub(crate) enum Cmd {
    Pipeline(wgpu::RenderPipeline),
    Group(u32, wgpu::BindGroup),
    Vertices(u32, wgpu::Buffer),
    Indices(wgpu::Buffer),
    Draw(Range<u32>),
    DrawIndexed(Range<u32>),
    DrawInstanced(Range<u32>, Range<u32>),
    /// An indexed draw the GPU wrote at an offset of a buffer (`Enc::draw_indexed_indirect`).
    DrawIndexedIndirect(wgpu::Buffer, u64),
    Blend(wgpu::Color),
    Stencil(u32),
    Scissor([u32; 4]),
    /// A pass of the picture keeping what it holds, its group 0 the frame's view.
    Picture,
    /// The same, its GPU time named apart (`Rec::picture_as`, with UYA_GPU_PASSES only).
    PictureAs(&'static str),
    /// A pass of another frame buffer (its colour and depth), keeping what it holds, its group 0 the frame's view; with `clear` its
    /// depth and stencil cleared (each frame starts so) and nothing drawn in it.
    Target { view: wgpu::TextureView, depth: wgpu::TextureView, clear: bool },
    /// The pass begun last ends.
    End,
    /// The picture copied into a texture of its size.
    CopyPicture(wgpu::Texture, Option<wgpu::Texture>),
    /// A texture copied into another (`size` texels).
    Copy(wgpu::Texture, wgpu::Texture, (u32, u32)),
    /// The picture at the GS's size into the texture (`eedraw::Pipelines::downsample`, with its map).
    Downsample(wgpu::Texture, [f32; 4]),
    /// A pass of the ambient occlusion's occluders (`ao.rs`): the mask, cleared to 1, and the picture's depth, its group 0 the
    /// frame's view; the mobies' opaque draws follow.
    Occluders(wgpu::TextureView),
    /// The ambient occlusion at its level over the picture (`ao::Ao::begin`), the occluders' mask beside it.
    Ao(u8, wgpu::TextureView),
    /// The same occlusion over the mobies' pixels (`ao::Ao::mobies`).
    AoMobies,
    /// A pass of the mobies' shadow map (`shadows::Shadows::casters_pass`): the picture's view, the map's light; the mobies' opaque
    /// draws follow (`shadows::Shadows::draw_mobies`).
    ShadowCasters(std::sync::Arc<crate::shadows::ShadowFrame>),
    /// The key light's shade over the picture (`shadows::Shadows::apply`) with the occluders' mask: over every pixel, or (true)
    /// over the mobies'.
    Shadows(std::sync::Arc<crate::shadows::ShadowFrame>, wgpu::TextureView, bool),
    /// The mobies' ray-traced casters built where the picture draws them (`raytrace::RtFrame::casters`): in every picture when
    /// the flag is set (the ray-traced shadows trace them in each), else in the frame's own picture only.
    RtCasters(std::sync::Arc<crate::raytrace::RtFrame>, bool),
    Surfaces(std::sync::Arc<crate::surface_buffer::Buffers>, Vec<Cmd>),
    /// The key light's ray-traced shade over the picture (`raytrace::RtFrame::apply`) with the occluders' mask: over every pixel,
    /// or (true) over the mobies'.
    RtShadows(std::sync::Arc<crate::raytrace::RtFrame>, wgpu::TextureView, bool),
    /// The ray-traced lighting (`rtlight::light`) at its level, with the ambient occlusion's level (0: none), over the picture
    /// (its rays traced first) or (true) over the mobies' pixels of the occluders' mask.
    RtLight(std::sync::Arc<crate::raytrace::RtFrame>, wgpu::TextureView, u8, u8, bool),
    /// The ray-traced reflections on the game's shiny surfaces at their level (`rtlight::gloss`).
    RtGloss(std::sync::Arc<crate::raytrace::RtFrame>, u8),
    /// The water's reflection traced into the reflections' picture at its level, after the mirror (`rtlight::water`).
    RtWater(std::sync::Arc<crate::raytrace::RtFrame>, crate::reflections::Plane, u8),
    /// The water reflections' mirror of the picture so far in the water's plane, at its level (`reflections.rs`).
    Mirror(crate::reflections::Plane, u8),
    /// The light shafts of the level's sun over the picture so far, at their level (`shafts.rs`).
    Shafts(crate::shafts::Sun, u8),
    /// The picture's depth into its alpha (`eedraw::DepthAlpha`, `eedraw::Pipelines::depth_alpha`): program 6's o.z and k, the
    /// threshold's Z' and alpha, NEAR and FAR, the view's tangents, Z's coefficients (`effects::Projection::gs_depth`).
    DepthAlpha([f32; 16]),
    /// DLSS (`dlss.rs`): the world drawn so far at its render size made the picture by DLSS, with the mobies' motion from these
    /// draws (`objects::Pipelines::draw_motion`); the commands after it draw into the picture at its own size.
    Upscale(Vec<Cmd>),
}

/// What the open pass has bound and set, as far as the recording knows: a command that sets what is already set changes nothing,
/// so it is not recorded (wgpu passes every set on to the driver, and every picture shown between two frames plays the list
/// again). A pass begun is a pass with nothing known: its own group 0 is not in it either.
#[derive(Default)]
struct Held {
    pipeline: Option<wgpu::RenderPipeline>,
    groups: [Option<wgpu::BindGroup>; 4],
    vertices: [Option<wgpu::Buffer>; 4],
    indices: Option<wgpu::Buffer>,
    blend: Option<wgpu::Color>,
    stencil: Option<u32>,
    scissor: Option<[u32; 4]>,
}

/// The commands as they are recorded, with whether a pass is open.
#[derive(Default)]
pub(crate) struct Rec {
    pub cmds: Vec<Cmd>,
    open: bool,
    held: Held,
    /// The open picture pass's name (`picture_as`).
    named: Option<&'static str>,
}

impl Rec {
    /// A pass of the picture, begun unless one is open.
    pub fn picture(&mut self) {
        if !self.open {
            self.begin(Cmd::Picture);
        }
    }
    /// With UYA_GPU_PASSES, a pass of the picture whose GPU time is named `name` (the open one ends first unless it is so named):
    /// what the picture pass draws, timed apart. Without it, `picture`.
    pub fn picture_as(&mut self, name: &'static str) {
        if !crate::gpu_profile::wanted() {
            return self.picture();
        }
        if self.open && self.named == Some(name) {
            return;
        }
        self.end();
        self.begin(Cmd::PictureAs(name));
        self.named = Some(name);
    }
    /// A pass of another frame buffer (the one open ends first).
    pub fn target(&mut self, view: &wgpu::TextureView, depth: &wgpu::TextureView, clear: bool) {
        self.end();
        self.begin(Cmd::Target { view: view.clone(), depth: depth.clone(), clear });
        self.open = !clear;
        if clear {
            self.cmds.push(Cmd::End);
        }
    }
    /// The open pass ends.
    pub fn end(&mut self) {
        if self.open {
            self.cmds.push(Cmd::End);
            self.open = false;
        }
    }
    /// The ambient occlusion's occluders' pass (the open one ends first; `Cmd::Occluders`).
    pub fn occluders(&mut self, mask: &wgpu::TextureView) {
        self.end();
        self.begin(Cmd::Occluders(mask.clone()));
    }
    /// The mobies' shadow map's pass (the open one ends first; `Cmd::ShadowCasters`).
    pub fn shadow_casters(&mut self, f: &std::sync::Arc<crate::shadows::ShadowFrame>) {
        self.end();
        self.begin(Cmd::ShadowCasters(f.clone()));
    }
    /// A command outside every pass (the open one ends first).
    pub fn outside(&mut self, c: Cmd) {
        self.end();
        self.cmds.push(c);
    }
    /// A pass begins with `c`: nothing is bound in it yet.
    fn begin(&mut self, c: Cmd) {
        self.cmds.push(c);
        self.open = true;
        self.held = Held::default();
        self.named = None;
    }
    /// A draw of vertices `v`: the same draw as the last command when that continues it (`Cmd::Draw` after `Cmd::Draw`, nothing set
    /// between them; every pipeline draws triangle lists, so a run of whole triangles and the run after it are one run).
    fn extend(&mut self, v: &Range<u32>) -> bool {
        match self.cmds.last_mut() {
            Some(Cmd::Draw(last)) if last.end == v.start && (last.end - last.start) % 3 == 0 => {
                last.end = v.end;
                true
            }
            _ => false,
        }
    }
    /// The same for an indexed draw (`Cmd::DrawIndexed`).
    fn extend_indexed(&mut self, v: &Range<u32>) -> bool {
        match self.cmds.last_mut() {
            Some(Cmd::DrawIndexed(last)) if last.end == v.start && (last.end - last.start) % 3 == 0 => {
                last.end = v.end;
                true
            }
            _ => false,
        }
    }
}

impl<'a> Enc<'a> for Rec {
    fn set_pipeline(&mut self, p: &'a wgpu::RenderPipeline) {
        if self.held.pipeline.as_ref() != Some(p) {
            self.held.pipeline = Some(p.clone());
            self.cmds.push(Cmd::Pipeline(p.clone()));
        }
    }
    fn set_bind_group(&mut self, index: u32, g: &'a wgpu::BindGroup) {
        if self.held.groups[index as usize].as_ref() != Some(g) {
            self.held.groups[index as usize] = Some(g.clone());
            self.cmds.push(Cmd::Group(index, g.clone()));
        }
    }
    fn set_vertex_buffer(&mut self, slot: u32, b: &'a wgpu::Buffer) {
        if self.held.vertices[slot as usize].as_ref() != Some(b) {
            self.held.vertices[slot as usize] = Some(b.clone());
            self.cmds.push(Cmd::Vertices(slot, b.clone()));
        }
    }
    fn set_index_buffer(&mut self, b: &'a wgpu::Buffer) {
        if self.held.indices.as_ref() != Some(b) {
            self.held.indices = Some(b.clone());
            self.cmds.push(Cmd::Indices(b.clone()));
        }
    }
    fn draw(&mut self, vertices: Range<u32>) {
        if !self.extend(&vertices) {
            self.cmds.push(Cmd::Draw(vertices));
        }
    }
    fn draw_indexed(&mut self, indices: Range<u32>) {
        if !self.extend_indexed(&indices) {
            self.cmds.push(Cmd::DrawIndexed(indices));
        }
    }
    fn draw_instanced(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        self.cmds.push(Cmd::DrawInstanced(vertices, instances));
    }
    fn draw_indexed_indirect(&mut self, args: &'a wgpu::Buffer, offset: u64) {
        self.cmds.push(Cmd::DrawIndexedIndirect(args.clone(), offset));
    }
    fn set_blend_constant(&mut self, c: wgpu::Color) -> Result<(), String> {
        if self.held.blend != Some(c) {
            self.held.blend = Some(c);
            self.cmds.push(Cmd::Blend(c));
        }
        Ok(())
    }
    fn set_stencil_reference(&mut self, r: u32) -> Result<(), String> {
        if self.held.stencil != Some(r) {
            self.held.stencil = Some(r);
            self.cmds.push(Cmd::Stencil(r));
        }
        Ok(())
    }
    fn set_scissor_rect(&mut self, r: [u32; 4]) -> Result<(), String> {
        if self.held.scissor != Some(r) {
            self.held.scissor = Some(r);
            self.cmds.push(Cmd::Scissor(r));
        }
        Ok(())
    }
}

/// The picture a frame is drawn into: its texture, its depth and their views, at the frame's size.
pub(crate) struct Target<'t> {
    pub tex: &'t wgpu::Texture,
    pub depth: &'t wgpu::Texture,
    pub tv: &'t wgpu::TextureView,
    pub dv: &'t wgpu::TextureView,
    pub size: (u32, u32),
}

/// What playing a frame's commands needs besides them: the device, the frame's view (group 0), the steps' pipelines (the
/// downsampling), the ambient occlusion setting's passes and the camera and fog they take.
pub(crate) struct Ctx<'c> {
    pub device: &'c wgpu::Device,
    pub view_bg: &'c wgpu::BindGroup,
    pub shadows: &'c crate::eedraw::Pipelines,
    pub ao: &'c crate::ao::Ao,
    pub light_shadows: &'c crate::shadows::Shadows,
    pub reflections: &'c crate::reflections::Reflections,
    pub shafts: &'c crate::shafts::Shafts,
    pub cam: &'c Camera,
    pub fog: &'c uyafmt::gameplay::Fog,
    /// Whether this is the frame's own picture (which traces the ray-traced lighting and reflections; a picture between frames
    /// takes what it traced: `rtlight::Traced`).
    pub fresh: bool,
    pub profile: Option<&'c crate::gpu_profile::Recording>,
    /// DLSS and the picture's inputs, when the list has its `Cmd::Upscale` (the commands before it draw at its render size).
    pub upscale: Option<(&'c crate::dlss::Upscaler, &'c crate::dlss::Shot)>,
}

/// The static world's bundles in one pass of `t` that clears it first (the frame's clear), in order. A first bundle named
/// "cover" (the terrain's cover, `native::Cover`) is a pass of the depth and stencil alone (cleared), the sky's bundle after it
/// a pass of the colour (cleared) keeping them, and the rest the pass that clears the depth and stencil again. The vegetation's
/// cull (`veg_cull`, its bundles' draws written by it) runs before the static world, against the cover's depth after its pass.
#[allow(clippy::too_many_arguments)]
pub(crate) fn statics(device: &wgpu::Device, queue: &wgpu::Queue, enc: &mut wgpu::CommandEncoder, t: &Target, bundles: &[wgpu::RenderBundle], labels: &[(&'static str, u32)], cull: Option<(&crate::veg_cull::Cull, &crate::veg_cull::FrameCull)>, profile: Option<&crate::gpu_profile::Recording>) -> Result<(), String> {
    let colour = |load| Some(wgpu::RenderPassColorAttachment { view: t.tv, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load, store: wgpu::StoreOp::Store } });
    let depth = |load: Option<(f32, u32)>, store| wgpu::RenderPassDepthStencilAttachment {
        view: t.dv,
        depth_ops: Some(wgpu::Operations { load: load.map_or(wgpu::LoadOp::Load, |l| wgpu::LoadOp::Clear(l.0)), store }),
        stencil_ops: Some(wgpu::Operations { load: load.map_or(wgpu::LoadOp::Load, |l| wgpu::LoadOp::Clear(l.1)), store }),
    };
    let pass = |enc: &mut wgpu::CommandEncoder, label, colour: Option<wgpu::RenderPassColorAttachment>, depth| {
        let colours = [colour];
        enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some(label), color_attachments: if colours[0].is_some() { &colours } else { &[] }, depth_stencil_attachment: Some(depth), timestamp_writes: None, occlusion_query_set: None, multiview_mask: None }).forget_lifetime()
    };
    let cover = labels.first().is_some_and(|l| l.0 == "cover");
    let (bundles, labels) = if cover {
        assert!(labels.get(1).is_some_and(|l| l.0 == "colour_sky"), "native: the terrain's cover with no sky after it");
        let mut p = pass(enc, "native terrain cover", None, depth(Some((0.0, 0)), wgpu::StoreOp::Store));
        execute(&mut p, &bundles[..1], &labels[..1], profile);
        drop(p);
        if let Some((c, f)) = cull {
            c.run(device, queue, enc, f, Some((t.depth, t.size)), profile)?;
        }
        let mut p = pass(enc, "native sky", colour(wgpu::LoadOp::Clear(wgpu::Color::BLACK)), depth(None, wgpu::StoreOp::Discard));
        execute(&mut p, &bundles[1..2], &labels[1..2], profile);
        drop(p);
        (&bundles[2..], &labels[2..])
    } else {
        if let Some((c, f)) = cull {
            c.run(device, queue, enc, f, None, profile)?;
        }
        (bundles, labels)
    };
    let mut p = pass(enc, "native static world", colour(if cover { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(wgpu::Color::BLACK) }), depth(Some((0.0, 0)), wgpu::StoreOp::Store));
    execute(&mut p, bundles, labels, profile);
    Ok(())
}

/// `bundles` executed in `p`, each timed and counted apart with UYA_GPU_PASSES.
fn execute(p: &mut wgpu::RenderPass<'_>, bundles: &[wgpu::RenderBundle], labels: &[(&'static str, u32)], profile: Option<&crate::gpu_profile::Recording>) {
    if profile.is_some() {
        for (bundle, &(label, draws)) in bundles.iter().zip(labels) {
            let timing = crate::gpu_profile::Span::begin_render(profile, p, label);
            if let Some(r) = profile {
                r.count_begin(p, label);
                r.count_draws(label, draws as u64);
            }
            p.execute_bundles(std::iter::once(bundle));
            if let Some(r) = profile {
                r.count_end(p);
            }
            timing.end_render(p);
        }
    } else {
        p.execute_bundles(bundles.iter());
    }
}

/// The world's own target with DLSS (`dlss::Upscaler`): its scene's colour and depth at the render size.
pub(crate) fn scene_target(u: &crate::dlss::Upscaler) -> Target<'_> {
    Target { tex: &u.colour, depth: &u.depth, tv: &u.colour_view, dv: &u.depth_view, size: u.render }
}

/// The threads that finish a picture's encoders (`Chunks::encoder`). wgpu encodes a command buffer's commands, and the driver its
/// Vulkan commands, when its encoder is finished: on the player's Crash Site save (23 November 2026) that was 95 % of the
/// recorder's time for a picture, 2.6 ms on average and 4.4 ms for one in twenty, on one thread; a picture split into several
/// encoders has them finished at once here. Each encoder only locks itself (wgpu-core takes the device's snatch lock to read)
/// and each has its own Vulkan command pool.
const ENCODERS: usize = 3;

/// The fewest drawing commands a part of a picture's list has (`play`): a part costs an encoder and, inside a pass, a pass ended
/// and begun again.
const MIN_PART: usize = 300;

type Finish = (wgpu::CommandEncoder, std::sync::mpsc::Sender<wgpu::CommandBuffer>);

fn finisher() -> &'static std::sync::mpsc::Sender<Finish> {
    static JOBS: std::sync::OnceLock<std::sync::mpsc::Sender<Finish>> = std::sync::OnceLock::new();
    JOBS.get_or_init(|| {
        let (jobs, asked) = std::sync::mpsc::channel::<Finish>();
        let asked = std::sync::Arc::new(std::sync::Mutex::new(asked));
        for i in 0..ENCODERS {
            let asked = asked.clone();
            std::thread::Builder::new()
                .name(format!("uya encode {i}"))
                .spawn(move || loop {
                    let job = asked.lock().expect("replay: the encoders' queue is poisoned").recv();
                    let Ok((enc, back)) = job else { return };
                    // The picture waiting for it is gone when its recorder stopped: nothing to hand back to.
                    let _ = back.send(enc.finish());
                })
                .expect("replay: an encoder's thread");
        }
        jobs
    })
}

/// A picture's command buffers in the order they are submitted, the ones still being finished on the encoders' threads
/// (`finisher`) waited for by `finish`.
#[derive(Default)]
pub(crate) struct Chunks {
    parts: Vec<Result<wgpu::CommandBuffer, std::sync::mpsc::Receiver<wgpu::CommandBuffer>>>,
}

impl Chunks {
    /// `enc`'s commands next, finished on an encoder's thread.
    pub fn encoder(&mut self, enc: wgpu::CommandEncoder) -> Result<(), String> {
        let (back, wait) = std::sync::mpsc::channel();
        finisher().send((enc, back)).map_err(|_| "replay: the encoders' threads stopped")?;
        self.parts.push(Err(wait));
        Ok(())
    }

    /// A command buffer finished already, next.
    pub fn buffer(&mut self, cb: wgpu::CommandBuffer) {
        self.parts.push(Ok(cb));
    }

    /// Every command buffer, in order, once all are finished.
    pub fn finish(self) -> Result<Vec<wgpu::CommandBuffer>, String> {
        self.parts.into_iter().map(|p| p.or_else(|wait| wait.recv().map_err(|_| "replay: an encoder's thread stopped".to_string()))).collect()
    }
}

/// The pass `play` has open, as it is begun again after a split (`Bound::restore`): what it draws into, kept.
#[derive(Clone, Copy)]
enum Open<'c> {
    Picture,
    Target(&'c wgpu::TextureView, &'c wgpu::TextureView),
    Occluders(&'c wgpu::TextureView),
    Casters(&'c std::sync::Arc<crate::shadows::ShadowFrame>),
}

/// A command a pass draws with (a render bundle could hold it), as against one of the pass's own settings or a pass.
fn is_raw(c: &Cmd) -> bool {
    matches!(c, Cmd::Pipeline(_) | Cmd::Group(..) | Cmd::Vertices(..) | Cmd::Indices(_) | Cmd::Draw(_) | Cmd::DrawIndexed(_) | Cmd::DrawInstanced(..) | Cmd::DrawIndexedIndirect(..))
}

/// The pass `open` into `t` begun with what it binds itself (the frame's view as group 0, the casters' light as group 1): the
/// first time (`first`) as the list asks (the occluders' mask cleared to white, the casters' map to the far plane, a target's
/// depth and stencil when `clear`), after a split keeping all it holds.
fn begin(enc: &mut wgpu::CommandEncoder, open: Open, t: &Target, ctx: &Ctx, first: bool, clear: bool) -> wgpu::RenderPass<'static> {
    fn pass(enc: &mut wgpu::CommandEncoder, label: &str, colour: Option<wgpu::RenderPassColorAttachment>, depth: wgpu::RenderPassDepthStencilAttachment) -> wgpu::RenderPass<'static> {
        enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some(label), color_attachments: &[colour], depth_stencil_attachment: Some(depth), timestamp_writes: None, occlusion_query_set: None, multiview_mask: None }).forget_lifetime()
    }
    let keep = || wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store };
    let mut p = match open {
        Open::Picture => pass(enc, "native picture", Some(wgpu::RenderPassColorAttachment { view: t.tv, depth_slice: None, resolve_target: None, ops: keep() }), wgpu::RenderPassDepthStencilAttachment { view: t.dv, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }), stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }) }),
        Open::Target(view, depth) => {
            let (z, s) = if first && clear { (wgpu::LoadOp::Clear(0.0), wgpu::LoadOp::Clear(0)) } else { (wgpu::LoadOp::Load, wgpu::LoadOp::Load) };
            pass(enc, if clear { "EE target depth" } else { "EE target" }, Some(wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: keep() }), wgpu::RenderPassDepthStencilAttachment { view: depth, depth_ops: Some(wgpu::Operations { load: z, store: wgpu::StoreOp::Store }), stencil_ops: Some(wgpu::Operations { load: s, store: wgpu::StoreOp::Store }) })
        }
        Open::Occluders(mask) => {
            let load = if first { wgpu::LoadOp::Clear(wgpu::Color::WHITE) } else { wgpu::LoadOp::Load };
            pass(enc, "moby occluders", Some(wgpu::RenderPassColorAttachment { view: mask, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load, store: wgpu::StoreOp::Store } }), wgpu::RenderPassDepthStencilAttachment { view: t.dv, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }), stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }) })
        }
        Open::Casters(f) => return ctx.light_shadows.casters_pass(enc, f, ctx.view_bg, first),
    };
    if !clear {
        p.set_bind_group(0, ctx.view_bg, &[]);
    }
    p
}

/// What the open pass has bound and set (the list sets nothing twice: `Rec`), set again on the pass that continues it.
#[derive(Default)]
struct Bound<'c> {
    pipeline: Option<&'c wgpu::RenderPipeline>,
    groups: [Option<&'c wgpu::BindGroup>; 4],
    vertices: [Option<&'c wgpu::Buffer>; 4],
    indices: Option<&'c wgpu::Buffer>,
    blend: Option<wgpu::Color>,
    stencil: Option<u32>,
    scissor: Option<[u32; 4]>,
}

impl Bound<'_> {
    fn restore(&self, p: &mut wgpu::RenderPass<'static>) {
        if let Some(x) = self.pipeline {
            p.set_pipeline(x);
        }
        for (i, g) in self.groups.iter().enumerate() {
            if let Some(g) = g {
                p.set_bind_group(i as u32, *g, &[]);
            }
        }
        for (i, b) in self.vertices.iter().enumerate() {
            if let Some(b) = b {
                p.set_vertex_buffer(i as u32, b.slice(..));
            }
        }
        if let Some(b) = self.indices {
            p.set_index_buffer(b.slice(..), wgpu::IndexFormat::Uint32);
        }
        if let Some(c) = self.blend {
            p.set_blend_constant(c);
        }
        if let Some(r) = self.stencil {
            p.set_stencil_reference(r);
        }
        if let Some(r) = self.scissor {
            p.set_scissor_rect(r[0], r[1], r[2], r[3]);
        }
    }
}

/// Plays `cmds` into `enc` for picture `t`. With DLSS (`ctx.upscale`) the commands before `Cmd::Upscale` draw into its scene
/// (`scene_target`), their scissors taken to its size, and its own commands are a command buffer of their own: `enc`'s before
/// them go to `done`, then DLSS's, and `enc` holds the rest (submitted after `done`). The draws before DLSS's step (all of them
/// without it) go to `done` in `ENCODERS` parts of about as many commands each, to be finished at once: a part that ends in a pass
/// ends that pass, and the next part begins it again where it was, keeping what it drew (`Open`) and binding what it had bound
/// (`Bound`).
pub(crate) fn play(cmds: &[Cmd], enc: &mut wgpu::CommandEncoder, t_out: &Target, ctx: &Ctx, done: &mut Chunks) -> Result<(), String> {
    if ctx.upscale.is_some() != cmds.iter().any(|c| matches!(c, Cmd::Upscale(_))) {
        return Err("native: DLSS's step and its upscaler do not go together".into());
    }
    let scene = ctx.upscale.map(|(u, _)| scene_target(u));
    let mut t = scene.as_ref().unwrap_or(t_out);
    // The scissors' scale in the world's passes with DLSS (the game's are the picture's), and whether the open pass is one.
    let scale = ctx.upscale.map(|(u, _)| (u.render.0 as f32 / u.out.0 as f32, u.render.1 as f32 / u.out.1 as f32));
    let mut scaled = false;
    let mut pass: Option<wgpu::RenderPass<'static>> = None;
    // UYA_FRAME_TIMES: the recording's CPU time by kind of command (ms), printed once the list is played.
    const KINDS: [&str; 12] = ["draws", "passes", "rt_casters", "rt_shadows", "rt_light", "rt_gloss", "rt_water", "shadows", "ao", "mirror", "shafts", "copies"];
    let timed = crate::play::frame_times();
    let mut cpu = [0.0f64; KINDS.len()];
    // The parts' length in drawing commands (`ENCODERS` parts before DLSS's step), and how far the part being recorded is; the
    // open pass and what it has bound. A list too short to be worth parts is one.
    let mut part = cmds.iter().take_while(|c| !matches!(c, Cmd::Upscale(_))).filter(|c| is_raw(c)).count().div_ceil(ENCODERS).max(MIN_PART);
    let mut drawn = 0usize;
    let mut open: Option<Open> = None;
    let mut bound = Bound::default();
    // The picture's ambient occlusion from `Cmd::Ao` to `Cmd::AoMobies`.
    let mut ao = None;
    let mut timing = crate::gpu_profile::Span::none();
    // UYA_GPU_COUNTS: the name the open pass's counts go to.
    let mut counting: Option<&'static str> = None;
    for c in cmds {
        let label = match c {
            Cmd::Picture => Some("colour_dynamic"),
            Cmd::PictureAs(name) => Some(*name),
            Cmd::Target { .. } => Some("effects_target"),
            Cmd::Occluders(_) => Some("occluders"),
            Cmd::ShadowCasters(_) => Some("casters"),
            Cmd::RtCasters(..) => Some("rt_casters"),
            Cmd::Surfaces(..) => Some("surface_dynamic"),
            Cmd::Shadows(..) | Cmd::RtShadows(..) => Some("shadows"),
            Cmd::Ao(..) | Cmd::AoMobies => Some("ao"),
            Cmd::CopyPicture(..) => Some("copies"),
            Cmd::Copy(..) => Some("copy_targets"),
            Cmd::Downsample(..) => Some("shadow_downsample"),
            Cmd::RtWater(..) => Some("water_trace"),
            Cmd::Mirror(..) => Some("water_mirror"),
            Cmd::Shafts(..) => Some("shafts"),
            Cmd::Upscale(_) => Some("upscale"),
            _ => None,
        };
        if let Some(name) = label { timing = crate::gpu_profile::Span::begin(ctx.profile, enc, name); }
        let started = timed.then(std::time::Instant::now);
        let kind = match c {
            Cmd::Picture | Cmd::PictureAs(_) | Cmd::Target { .. } | Cmd::End | Cmd::Occluders(_) | Cmd::Surfaces(..) => 1,
            Cmd::RtCasters(..) => 2,
            Cmd::RtShadows(..) => 3,
            Cmd::RtLight(..) => 4,
            Cmd::RtGloss(..) => 5,
            Cmd::RtWater(..) => 6,
            Cmd::ShadowCasters(_) | Cmd::Shadows(..) => 7,
            Cmd::Ao(..) | Cmd::AoMobies => 8,
            Cmd::Mirror(..) => 9,
            Cmd::Shafts(..) => 10,
            Cmd::CopyPicture(..) | Cmd::Copy(..) | Cmd::Downsample(..) => 11,
            _ => 0,
        };
        match c {
            Cmd::Picture | Cmd::PictureAs(_) | Cmd::Occluders(_) => scaled = scale.is_some() && !std::ptr::eq(t, t_out),
            Cmd::Target { .. } => scaled = false,
            _ => {}
        }
        match c {
            Cmd::Upscale(moby) => {
                if pass.is_some() {
                    return Err("native: DLSS's step inside a pass".into());
                }
                let (u, shot) = ctx.upscale.expect("checked above");
                u.run(ctx.device, enc, done, shot, ctx.view_bg, moby, t_out.tv, t_out.dv)?;
                t = t_out;
                part = usize::MAX;
            }
            Cmd::Picture | Cmd::PictureAs(_) | Cmd::Target { .. } | Cmd::Occluders(_) | Cmd::ShadowCasters(_) => {
                let (o, clear) = match c {
                    Cmd::Picture | Cmd::PictureAs(_) => (Open::Picture, false),
                    Cmd::Target { view, depth, clear } => (Open::Target(view, depth), *clear),
                    Cmd::Occluders(mask) => (Open::Occluders(mask), false),
                    Cmd::ShadowCasters(f) => (Open::Casters(f), false),
                    _ => unreachable!("matched above"),
                };
                pass = Some(begin(enc, o, t, ctx, true, clear));
                if let (Some(r), Some(name), Some(p)) = (ctx.profile, label, pass.as_mut()) {
                    r.count_begin(p, name);
                    counting = Some(name);
                }
                open = Some(o);
                bound = Bound::default();
                if !clear {
                    bound.groups[0] = Some(ctx.view_bg);
                }
                if let Cmd::ShadowCasters(f) = c {
                    bound.groups[1] = Some(&f.mobies_light);
                }
            }
            Cmd::End => {
                if let (Some(r), Some(_), Some(p)) = (ctx.profile, counting.take(), pass.as_mut()) {
                    r.count_end(p);
                }
                pass.take().ok_or("native: a recorded pass ends twice")?;
                open = None;
                std::mem::replace(&mut timing, crate::gpu_profile::Span::none()).end(enc);
                if drawn >= part {
                    done.encoder(std::mem::replace(enc, ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("native picture part") })))?;
                    drawn = 0;
                }
            }
            // The picture as drawn so far, texel for texel into `dst` (a GS blend's destination, read at the pixel's own place),
            // and over `snap` (a draw sampling the frame by its shares): the same copy, but with DLSS before its step, where the
            // world is the scene at its render size (`scene_target`), the scene stretched over `snap`
            // (`dlss::Upscaler::scaled_copy`).
            // On a GPU that cannot run DLSS the two are the same picture, and `snap` alone is copied: the GS blend reads it as its
            // destination (`native::ee_draw`), one copy of the whole picture a step fewer.
            Cmd::CopyPicture(snap, dst) => {
                if let Some(r) = ctx.profile {
                    r.count_draws("copies", 1 + dst.is_some() as u64);
                }
                let copy = |to: &wgpu::Texture, enc: &mut wgpu::CommandEncoder| enc.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo { texture: t.tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    wgpu::TexelCopyTextureInfo { texture: to, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    wgpu::Extent3d { width: t.size.0, height: t.size.1, depth_or_array_layers: 1 },
                );
                if let Some(dst) = dst {
                    copy(dst, enc);
                }
                if std::ptr::eq(t, t_out) {
                    copy(snap, enc);
                } else {
                    ctx.upscale.expect("a scene target only with DLSS").0.scaled_copy(ctx.device, enc, t.tex, snap);
                }
            }
            Cmd::Copy(from, to, size) => {
                if let Some(r) = ctx.profile {
                    r.count_draws("copy_targets", 1);
                }
                enc.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo { texture: from, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::TexelCopyTextureInfo { texture: to, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                )
            }
            // The map is in the picture's pixels; from the scene at DLSS's render size, in its pixels.
            Cmd::Downsample(to, map) => {
                if let Some(r) = ctx.profile {
                    r.count_draws("shadow_downsample", 1);
                }
                let map = match scale.filter(|_| !std::ptr::eq(t, t_out)) {
                    Some((sx, sy)) => [map[0] * sx, map[1] * sx, map[2] * sy, map[3] * sy],
                    None => *map,
                };
                ctx.shadows.downsample(ctx.device, enc, t.tex, to, map)
            }
            Cmd::DepthAlpha(params) => {
                if let Some(r) = ctx.profile {
                    r.count_draws("depth_alpha", 1);
                }
                ctx.shadows.depth_alpha(ctx.device, enc, t.depth, t.tv, t.size, params)
            }
            Cmd::Ao(level, mask) => ao = Some(ctx.ao.begin(ctx.device, enc, t.depth, mask, t.tv, t.size, ctx.cam, ctx.fog, (crate::native::NEAR, crate::native::FAR), *level)),
            Cmd::AoMobies => ctx.ao.mobies(enc, t.tv, ao.as_ref().ok_or("native: the mobies' ambient occlusion before the picture's")?),
            Cmd::Shadows(f, mask, mobies) => ctx.light_shadows.apply(ctx.device, enc, f, t.depth, mask, t.tv, t.size, ctx.cam, ctx.fog, (crate::native::NEAR, crate::native::FAR), *mobies),
            Cmd::RtCasters(f, every) => f.casters(enc, ctx.fresh, *every),
            Cmd::Surfaces(buffers, commands) => buffers.draw(enc, t.dv, false, &[], commands),
            Cmd::RtShadows(f, mask, mobies) => f.apply(ctx.device, enc, t.depth, mask, t.tv, t.size, ctx.cam, ctx.fog, (crate::native::NEAR, crate::native::FAR), *mobies),
            Cmd::RtLight(f, mask, level, ao, mobies) => crate::rtlight::light(f, ctx.device, enc, t.depth, mask, t.tv, t.size, ctx.cam, ctx.fog, *level, *ao, *mobies, ctx.fresh, ctx.profile)?,
            Cmd::RtGloss(f, level) => crate::rtlight::gloss(f, ctx.device, enc, t.tex, t.depth, t.tv, t.size, ctx.cam, ctx.fog, *level, ctx.fresh, ctx.profile)?,
            Cmd::RtWater(f, plane, level) => crate::rtlight::water(f, ctx.device, enc, ctx.reflections, t.depth, t.size, ctx.cam, ctx.fog, *plane, *level),
            Cmd::Mirror(plane, level) => ctx.reflections.mirror(ctx.device, enc, t.tex, t.depth, t.size, ctx.cam, *plane, *level),
            Cmd::Shafts(sun, level) => ctx.shafts.run(ctx.device, enc, t.tex, t.depth, t.size, ctx.cam, ctx.fog, (crate::native::NEAR, crate::native::FAR), *sun, *level),
            c => {
                let p = pass.as_mut().ok_or("native: a recorded draw outside every pass")?;
                match c {
                    Cmd::Pipeline(x) => {
                        p.set_pipeline(x);
                        bound.pipeline = Some(x);
                    }
                    Cmd::Group(i, g) => {
                        p.set_bind_group(*i, g, &[]);
                        bound.groups[*i as usize] = Some(g);
                    }
                    Cmd::Vertices(s, b) => {
                        p.set_vertex_buffer(*s, b.slice(..));
                        bound.vertices[*s as usize] = Some(b);
                    }
                    Cmd::Indices(b) => {
                        p.set_index_buffer(b.slice(..), wgpu::IndexFormat::Uint32);
                        bound.indices = Some(b);
                    }
                    Cmd::Draw(_) | Cmd::DrawIndexed(_) | Cmd::DrawInstanced(..) | Cmd::DrawIndexedIndirect(..) => {
                        if let (Some(r), Some(name)) = (ctx.profile, counting) {
                            r.count_draws(name, 1);
                        }
                        match c {
                            Cmd::Draw(r) => p.draw(r.clone(), 0..1),
                            Cmd::DrawIndexed(r) => p.draw_indexed(r.clone(), 0, 0..1),
                            Cmd::DrawInstanced(r, n) => p.draw(r.clone(), n.clone()),
                            Cmd::DrawIndexedIndirect(b, o) => p.draw_indexed_indirect(b, *o),
                            _ => unreachable!("matched above"),
                        }
                    }
                    Cmd::Blend(c) => {
                        p.set_blend_constant(*c);
                        bound.blend = Some(*c);
                    }
                    Cmd::Stencil(r) => {
                        p.set_stencil_reference(*r);
                        bound.stencil = Some(*r);
                    }
                    Cmd::Scissor(r) => {
                        let r = match scale.filter(|_| scaled) {
                            Some((sx, sy)) => {
                                let (x0, y0) = ((r[0] as f32 * sx).floor() as u32, (r[1] as f32 * sy).floor() as u32);
                                let (x1, y1) = ((((r[0] + r[2]) as f32 * sx).ceil() as u32).min(t.size.0), (((r[1] + r[3]) as f32 * sy).ceil() as u32).min(t.size.1));
                                [x0.min(x1), y0.min(y1), x1.saturating_sub(x0), y1.saturating_sub(y0)]
                            }
                            None => *r,
                        };
                        p.set_scissor_rect(r[0], r[1], r[2], r[3]);
                        bound.scissor = Some(r);
                    }
                    _ => unreachable!("matched above"),
                }
                // A part long enough ends after a draw: its pass ends there and goes on in the next part's encoder.
                if is_raw(c) {
                    drawn += 1;
                }
                if drawn >= part && matches!(c, Cmd::Draw(_) | Cmd::DrawIndexed(_) | Cmd::DrawInstanced(..) | Cmd::DrawIndexedIndirect(..)) {
                    let o = open.ok_or("native: a recorded draw outside every pass")?;
                    if let (Some(r), Some(_), Some(p)) = (ctx.profile, counting, pass.as_mut()) {
                        r.count_end(p);
                    }
                    drop(pass.take()); // the pass ends before its encoder is handed over
                    done.encoder(std::mem::replace(enc, ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("native picture part") })))?;
                    let mut p = begin(enc, o, t, ctx, false, false);
                    bound.restore(&mut p);
                    if let (Some(r), Some(name)) = (ctx.profile, counting) {
                        r.count_begin(&mut p, name);
                    }
                    pass = Some(p);
                    drawn = 0;
                }
            }
        }
        if label.is_some() && pass.is_none() {
            std::mem::replace(&mut timing, crate::gpu_profile::Span::none()).end(enc);
        }
        if let Some(s) = started {
            cpu[kind] += s.elapsed().as_secs_f64() * 1e3;
        }
    }
    if pass.is_some() {
        return Err("native: a recorded pass is left open".into());
    }
    if timed {
        let parts: Vec<String> = KINDS.iter().zip(cpu).filter(|(_, ms)| *ms > 0.0).map(|(k, ms)| format!("{k} {ms:.3}")).collect();
        println!("frame stages: commands recorded {}: {}", cmds.len(), parts.join(", "));
    }
    Ok(())
}

/// What a frame left to be drawn again (`Native::render` with `replays` set): its static world's bundles, its other commands,
/// its view's buffer and group, its camera and the one before it (None: no picture between the two), its fog, its size.
pub struct Replay {
    /// A scene snapshot whose first rasterization is owned by the presenter.
    pub(crate) prepared: bool,
    pub(crate) rendered: std::sync::atomic::AtomicBool,
    pub(crate) output_depth: wgpu::Texture,
    pub(crate) output_view: wgpu::TextureView,
    pub(crate) output_depth_view: wgpu::TextureView,
    /// The display buffer, and its copy at the GS's size (`eedraw::EeTextures::display_gs`).
    pub(crate) display: Option<(wgpu::Texture, wgpu::Texture)>,
    pub field: u64,
    /// The field of the world frame before it, which `prev` is the camera of (field - 1 without one): a picture between the two
    /// spans the fields from it.
    pub from: u64,
    /// Whether the frame begins a shot (another level or shot than the world frame before, or none before): its first picture
    /// starts the ray-traced lighting's history again. A field passed over in one shot is not a cut.
    pub(crate) cut: bool,
    pub level: u32,
    pub size: (u32, u32),
    pub(crate) bundles: Vec<wgpu::RenderBundle>,
    /// Each bundle's GPU time's name and its draw calls.
    pub(crate) bundle_labels: Vec<(&'static str, u32)>,
    /// The vegetation's cull, which writes its bundles' draws (`veg_cull`).
    pub(crate) cull: Option<(std::sync::Arc<crate::veg_cull::Cull>, std::sync::Arc<crate::veg_cull::FrameCull>)>,
    pub(crate) surface_bundles: Vec<wgpu::RenderBundle>,
    pub(crate) raster: Option<std::sync::Arc<crate::surface_buffer::Buffers>>,
    pub(crate) cmds: Vec<Cmd>,
    pub(crate) view: wgpu::Buffer,
    pub(crate) view_bg: wgpu::BindGroup,
    pub(crate) cam: Camera,
    pub(crate) prev: Option<Camera>,
    pub(crate) fog: uyafmt::gameplay::Fog,
    pub(crate) shadows: std::sync::Arc<crate::eedraw::Pipelines>,
    pub(crate) ao: std::sync::Arc<crate::ao::Ao>,
    /// The shadows option's pipelines (`shadows.rs`).
    pub(crate) light_shadows: std::sync::Arc<crate::shadows::Shadows>,
    pub(crate) reflections: std::sync::Arc<crate::reflections::Reflections>,
    pub(crate) shafts: std::sync::Arc<crate::shafts::Shafts>,
    /// Set (under its lock) when a later frame lays an instance's tie corners out anew (another LOD: `objects::Ties::frame`),
    /// before it writes them: the frame is then not drawn again. `play` draws a picture between frames under the lock.
    pub(crate) stale: std::sync::Arc<std::sync::Mutex<bool>>,
    /// The frame's buffers (`native::FrameSet`): the frame builds into a set no replay holds.
    pub(crate) _set: std::sync::Arc<()>,
    /// DLSS when the frame's commands have its step (`Cmd::Upscale`), and the layout of the mobies' motion uniform
    /// (`objects::Pipelines::motion_layout`).
    pub(crate) upscale: Option<std::sync::Arc<crate::dlss::Upscaler>>,
    pub(crate) motion_layout: wgpu::BindGroupLayout,
}

impl Replay {
    /// Whether a picture between the frame before and this one can be drawn (both drew the world, one field apart, no cut).
    pub fn between(&self) -> bool {
        self.prev.is_some()
    }

    /// The flag a later frame sets when this frame's ties are no longer on the GPU as it drew them (`stale`).
    pub(crate) fn stale(&self) -> &std::sync::Mutex<bool> {
        &self.stale
    }

    /// The picture `alpha` of the way from the frame before to this one, into `t` (the replay's own size), recorded into `enc`;
    /// its view written into the frame's view buffer now (`queue`), so `enc` is submitted before the frame is drawn again. With
    /// DLSS the commands before its own go to `done` (`play`), submitted before `enc`, and the pictures are drawn in the order
    /// they are shown (DLSS's history is theirs).
    pub(crate) fn draw(&self, device: &wgpu::Device, queue: &wgpu::Queue, enc: &mut wgpu::CommandEncoder, t: &Target, alpha: f32, profile: Option<&crate::gpu_profile::Recording>, done: &mut Chunks) -> Result<(), String> {
        let alpha = if self.prev.is_some() { alpha } else { 1.0 };
        let cam = self.prev.as_ref().map_or(self.cam, |prev| between(prev, &self.cam, alpha));
        let fresh = !self.rendered.load(std::sync::atomic::Ordering::Acquire);
        if fresh && self.cut {
            // Loading can overlap a final old-level snapshot. Reset at execution of the new shot, not only at preparation.
            if let Some(frame) = self.cmds.iter().find_map(|c| match c { Cmd::RtLight(f, ..) | Cmd::RtGloss(f, ..) => Some(f), _ => None }) {
                frame.rt.passes.reset_history();
                if crate::play::frame_times() {
                    println!("frame stages: history reset {}", self.field);
                }
            }
        }
        // DLSS's inputs for this picture: its time (fields), its mobies' share back toward frame N - 1, the frame's span.
        let span = (self.field - self.from) as f64;
        let shot = self.upscale.as_ref().map(|u| u.shot(device, &self.motion_layout, &cam, self.from as f64 + alpha as f64 * span, 1.0 - alpha, span, fresh && self.cut));
        let (ndc, lod) = self.upscale.as_ref().zip(shot.as_ref()).map_or(([0.0; 2], 1.0), |(u, s)| (u.jitter_ndc(s.jitter), u.lod_scale()));
        queue.write_buffer(&self.view, 0, bytemuck::bytes_of(&crate::native::view_u_at(&cam, &self.fog, 1.0 - alpha, ndc, lod)));
        let scene = self.upscale.as_deref().map(scene_target);
        let world = scene.as_ref().unwrap_or(t);
        let total = crate::gpu_profile::Span::begin(profile, enc, "total");
        let colour = crate::gpu_profile::Span::begin(profile, enc, "colour_static");
        statics(device, queue, enc, world, &self.bundles, &self.bundle_labels, self.cull.as_ref().map(|(c, f)| (&**c, &**f)), profile)?;
        colour.end(enc);
        let surfaces = crate::gpu_profile::Span::begin(profile, enc, "surface_static");
        if let Some(raster) = &self.raster { raster.draw(enc, world.dv, true, &self.surface_bundles, &[]); }
        surfaces.end(enc);
        // The static world is a part of its own, finished while the rest is recorded and finished (`Chunks`).
        done.encoder(std::mem::replace(enc, device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("native picture part") })))?;
        play(&self.cmds, enc, t, &Ctx { device, view_bg: &self.view_bg, shadows: &self.shadows, ao: &self.ao, light_shadows: &self.light_shadows, reflections: &self.reflections, shafts: &self.shafts, cam: &cam, fog: &self.fog, fresh, profile, upscale: self.upscale.as_deref().zip(shot.as_ref()) }, done)?;
        // The display buffer holds the game's own frame (what a later frame's grab and a pause take), never a picture between
        // two frames.
        if let Some((display, display_gs)) = self.display.as_ref().filter(|_| alpha >= 0.9995) {
            enc.copy_texture_to_texture(t.tex.as_image_copy(), display.as_image_copy(), wgpu::Extent3d { width: t.size.0, height: t.size.1, depth_or_array_layers: 1 });
            self.shadows.downsample(device, enc, display, display_gs, crate::eedraw::display_gs_map(t.size));
        }
        total.end(enc);
        if let Some(p) = profile { p.resolve(enc); }
        self.rendered.store(true, std::sync::atomic::Ordering::Release);
        Ok(())
    }
}

/// The most a camera moves in one field that is still the same shot: farther, or turned more, is a cut (a cutscene's next shot,
/// a respawn), and no picture is drawn between the two.
const CUT_UNITS: f32 = 3.0;
const CUT_COS: f32 = 0.94;

/// The most fields between two world frames that a picture between them spans: the fields between were not drawn (the game
/// thread never waits for a busy renderer: `rtgame.rs`).
pub(crate) const MAX_GAP: u64 = 4;

/// Whether `b`, `fields` fields after `a`, follows it in one shot (`CUT_UNITS`, `CUT_COS`: each field's most).
pub(crate) fn same_shot(a: &Camera, b: &Camera, fields: u64) -> bool {
    let d = [b.eye[0] - a.eye[0], b.eye[1] - a.eye[1], b.eye[2] - a.eye[2]];
    let dot = |x: [f32; 3], y: [f32; 3]| x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
    let k = fields.max(1) as f32;
    let cos = (CUT_COS.acos() * k).min(std::f32::consts::PI).cos();
    dot(d, d) <= (CUT_UNITS * k).powi(2) && dot(a.forward, b.forward) >= cos && dot(a.up, b.up) >= cos && (a.tan[0] - b.tan[0]).abs() < 0.2 * a.tan[0] && (a.tan[1] - b.tan[1]).abs() < 0.2 * a.tan[1]
}

/// The camera `t` of the way from `a` to `b`: the eye on the line between them, the forward and up rows turned between theirs
/// (each the normalised blend, up made square to forward), the tangents between theirs. At 1 it is `b` exactly.
pub(crate) fn between(a: &Camera, b: &Camera, t: f32) -> Camera {
    if t >= 1.0 {
        return *b;
    }
    let mix = |x: f32, y: f32| x + (y - x) * t;
    let mix3 = |x: [f32; 3], y: [f32; 3]| [mix(x[0], y[0]), mix(x[1], y[1]), mix(x[2], y[2])];
    let norm = |v: [f32; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        [v[0] / l, v[1] / l, v[2] / l]
    };
    let forward = norm(mix3(a.forward, b.forward));
    let up = mix3(a.up, b.up);
    let d = up[0] * forward[0] + up[1] * forward[1] + up[2] * forward[2];
    let up = norm([up[0] - d * forward[0], up[1] - d * forward[1], up[2] - d * forward[2]]);
    Camera { eye: mix3(a.eye, b.eye), forward, up, tan: [mix(a.tan[0], b.tan[0]), mix(a.tan[1], b.tan[1])], k: b.k }
}
