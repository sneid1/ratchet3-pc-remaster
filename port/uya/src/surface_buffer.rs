//! Visible surface data for the ray-traced passes. A depth-equal raster pass uses
//! the same vertex shaders, draw order and alpha tests as the picture. The colour
//! pass keeps its dual-source blend, which cannot share an MRT pipeline.
//! Their vertex position outputs must be `@invariant`: a separate compilation
//! must not round the surface's depth differently and fail the Equal test.

use crate::replay::{Cmd, Enc};
use std::{collections::HashMap, sync::{Arc, RwLock, OnceLock}};

pub(crate) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
static PAIRS: OnceLock<RwLock<HashMap<wgpu::RenderPipeline, wgpu::RenderPipeline>>> = OnceLock::new();

pub(crate) fn pipeline(device: &wgpu::Device, desc: &wgpu::RenderPipelineDescriptor<'_>, entry: Option<&str>) -> wgpu::RenderPipeline {
    let picture = device.create_render_pipeline(desc);
    if let Some(entry) = entry {
        let mut d = desc.clone();
        let targets = std::array::from_fn::<_, 3, _>(|_| Some(wgpu::ColorTargetState { format: FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }));
        let fragment = d.fragment.as_mut().expect("surface fragment");
        fragment.entry_point = Some(entry);
        fragment.targets = &targets;
        let depth = d.depth_stencil.as_mut().expect("surface depth");
        depth.depth_write_enabled = Some(false);
        depth.depth_compare = Some(wgpu::CompareFunction::Equal);
        let surface = device.create_render_pipeline(&d);
        PAIRS.get_or_init(Default::default).write().expect("surface pipelines").insert(picture.clone(), surface);
    }
    picture
}

/// Drop level-specific pipeline handles when their texture-array layout goes away.
pub(crate) fn unregister(pipeline: &wgpu::RenderPipeline) {
    if let Some(pairs) = PAIRS.get() { pairs.write().expect("surface pipelines").remove(pipeline); }
}

/// Record only draws with a visible-surface counterpart. Failed alpha pixels,
/// additive effects, UI and sky never supply an opaque surface.
pub(crate) fn capture(cmds: &[Cmd]) -> Vec<Cmd> {
    let pairs = PAIRS.get_or_init(Default::default).read().expect("surface pipelines");
    let mut out = Vec::new();
    let mut active = false;
    for c in cmds {
        match c {
            Cmd::Pipeline(p) => { active = pairs.contains_key(p); if active { out.push(Cmd::Pipeline(pairs[p].clone())); } }
            Cmd::Group(i, g) => out.push(Cmd::Group(*i, g.clone())),
            Cmd::Vertices(i, b) => out.push(Cmd::Vertices(*i, b.clone())),
            Cmd::Indices(b) => out.push(Cmd::Indices(b.clone())),
            Cmd::Draw(r) if active => out.push(Cmd::Draw(r.clone())),
            Cmd::DrawIndexed(r) if active => out.push(Cmd::DrawIndexed(r.clone())),
            Cmd::DrawInstanced(r, n) if active => out.push(Cmd::DrawInstanced(r.clone(), n.clone())),
            Cmd::DrawIndexedIndirect(b, o) if active => out.push(Cmd::DrawIndexedIndirect(b.clone(), *o)),
            _ => {}
        }
    }
    out
}

pub(crate) fn commands<'a, E: Enc<'a>>(p: &mut E, cmds: &'a [Cmd]) {
    for c in cmds {
        match c {
            Cmd::Pipeline(v) => p.set_pipeline(v), Cmd::Group(i, v) => p.set_bind_group(*i, v),
            Cmd::Vertices(i, v) => p.set_vertex_buffer(*i, v), Cmd::Indices(v) => p.set_index_buffer(v),
            Cmd::Draw(r) => p.draw(r.clone()), Cmd::DrawIndexed(r) => p.draw_indexed(r.clone()),
            Cmd::DrawInstanced(r, n) => p.draw_instanced(r.clone(), n.clone()),
            Cmd::DrawIndexedIndirect(b, o) => p.draw_indexed_indirect(b, *o),
            _ => panic!("surface bundle contains a non-draw command"),
        }
    }
}

pub(crate) fn bundle(device: &wgpu::Device, cmds: &[Cmd], surface: bool) -> wgpu::RenderBundle {
    let colours = if surface { vec![Some(FORMAT); 3] } else { vec![Some(wgpu::TextureFormat::Rgba8Unorm)] };
    let mut p = device.create_render_bundle_encoder(&wgpu::RenderBundleEncoderDescriptor {
        label: Some("world surface bundle"), color_formats: &colours,
        depth_stencil: Some(wgpu::RenderBundleDepthStencil { format: crate::native::DEPTH, depth_read_only: surface, stencil_read_only: surface }),
        sample_count: 1, multiview: None,
    });
    commands(&mut p, cmds);
    p.finish(&wgpu::RenderBundleDescriptor { label: Some("world surface bundle") })
}

pub(crate) struct Buffers {
    pub views: [wgpu::TextureView; 3],
}

impl Buffers {
    pub fn new(device: &wgpu::Device, size: (u32, u32)) -> Arc<Self> {
        let views = std::array::from_fn(|_| device.create_texture(&wgpu::TextureDescriptor {
            label: Some("visible surface"), size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[],
        }).create_view(&Default::default()));
        Arc::new(Self { views })
    }

    pub fn draw(&self, enc: &mut wgpu::CommandEncoder, depth: &wgpu::TextureView, clear: bool, bundles: &[wgpu::RenderBundle], cmds: &[Cmd]) {
        let attachments = self.views.each_ref().map(|view| Some(wgpu::RenderPassColorAttachment {
            view, depth_slice: None, resolve_target: None,
            ops: wgpu::Operations { load: if clear { wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT) } else { wgpu::LoadOp::Load }, store: wgpu::StoreOp::Store },
        }));
        let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("visible surfaces"), color_attachments: &attachments,
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: depth, depth_ops: None, stencil_ops: None }),
            timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
        });
        p.execute_bundles(bundles);
        commands(&mut p, cmds);
    }
}

pub(crate) const COMMON: &str = r#"
struct SurfaceOut { @location(0) normal: vec4<f32>, @location(1) bounce: vec4<f32>, @location(2) hue: vec4<f32> };
// Match raytrace::RIFT_GLOSS_LOD: broad reflections must not inherit subpixel gloss noise.
fn surface_lod(size: vec2<u32>, tile: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> f32 {
    let scale = vec2<f32>(size) * abs(tile);
    return max(3.0, log2(max(max(length(dx * scale), length(dy * scale)), 1.0)));
}
fn surface_data(uv: vec2<f32>, depth: f32, pos: vec3<f32>, normal: vec3<f32>, vertex: vec4<f32>, opaque: bool, chrome: bool) -> SurfaceOut {
    let dx = dpdx(uv); let dy = dpdy(uv);
    let face = cross(dpdx(pos), dpdy(pos));
    var n = material_renormal(select(face, normal, dot(normal, normal) > 0.0));
    if (dot(n, pos - view.eye.xyz) > 0.0) { n = -n; }
    var t: vec4<f32>;
    if (mat.extra.x > 0.5) { t = round(textureSampleGrad(tex, smp, uv, dx, dy) * 255.0); }
    else { t = floor(textureSampleLevel(tex, smp, uv, min(log2(depth) + mat.qlog + mat.k, mat.last)) * 255.0 + 0.001); }
    let alpha = min(floor(t.a * round(vertex.a * 255.0) / 128.0), 255.0);

    if (alpha < max(mat.aref, select(0.0, 128.0, opaque))) { discard; }
    let coverage = clamp(alpha / 128.0, 0.0, 1.0);
    var shine = select(0.0, coverage, chrome);
    var metal = 1.0;
    var f0 = -1.0;
    if (mat.rift.w > 2.5) {
        // A fitted atlas (`pbr.tsv`): its gloss is 1 - the ORM map's roughness (green), its metalness the ORM's blue, as
        // `lighting::MATERIAL` lights it and `raytrace`'s hits take it. Read as the other surfaces' map blue, its first map is the
        // normal map (blue a flat normal's z, 0.985 to 1.0) and its region weights the flat stand-in's, so every one of the 575
        // was at least 0.5 glossy whatever its roughness: dirt and grass mirrored the scene.
        let orm = textureSampleLevel(mtl1, msmp, uv * mat.tiles.xy, surface_lod(textureDimensions(mtl1), mat.tiles.xy, dx, dy));
        shine = max(shine, 1.0 - orm.g);
        metal = clamp(orm.b, 0.0, 1.0);
        f0 = mix(0.04, 0.6, metal);
    } else if (mat.rift.w > 0.5) {
        var weights = vec4<f32>(1.0, 0.0, 0.0, 0.0);
        if (mat.rift.w > 1.5) { weights = textureSampleGrad(rgn, msmp, uv, dx, dy); }
        var gloss = 0.0;
        if (weights.x > 0.0) { gloss += weights.x * textureSampleLevel(mtl, msmp, uv * mat.rift.xy, surface_lod(textureDimensions(mtl), mat.rift.xy, dx, dy)).b; }
        if (weights.y > 0.0) { gloss += weights.y * textureSampleLevel(mtl1, msmp, uv * mat.tiles.xy, surface_lod(textureDimensions(mtl1), mat.tiles.xy, dx, dy)).b; }
        if (weights.z > 0.0) { gloss += weights.z * textureSampleLevel(mtl2, msmp, uv * mat.tiles.zw, surface_lod(textureDimensions(mtl2), mat.tiles.zw, dx, dy)).b; }
        if (weights.w > 0.0) { gloss += weights.w * textureSampleLevel(mtl3, msmp, uv * mat.tiles3.xy, surface_lod(textureDimensions(mtl3), mat.tiles3.xy, dx, dy)).b; }
        shine = max(shine, gloss);
        metal = dot(weights, mat.metals);
        f0 = dot(weights, mat.refls * mat.refls);
    }
    let texel = t.rgb / 255.0;
    let tint = vertex.rgb / max(max(vertex.r, max(vertex.g, vertex.b)), 1e-3);
    var o: SurfaceOut;
    o.normal = vec4<f32>(n, shine);
    o.bounce = vec4<f32>(texel * tint * coverage * (1.0 - shine * metal), f0);
    // Depth is the validity tag used by rtlight::raster_valid. Never mark a missing/degenerate normal as a usable surface:
    // the lighting pass must retain its depth-derived normal and reflections must use their camera-ray fallback there.
    let normal_length2 = dot(n, n);
    let valid = normal_length2 > 0.5 && normal_length2 < 1.5;
    o.hue = vec4<f32>(mix(vec3<f32>(1.0), texel / max(max(texel.r, max(texel.g, texel.b)), 1e-3), metal), select(0.0, depth, valid));
    return o;
}
"#;

pub(crate) const OBJECTS: &str = r#"
@fragment fn fs_surface(i: VOut) -> SurfaceOut { return surface_data(clamp(i.st.xy / i.st.z, mat.region.xz, mat.region.yw), i.depth, i.wpos, i.ln.xyz, i.colour, false, (i.light & 0x40000000u) != 0u); }
@fragment fn fs_surface_opaque(i: VOut) -> SurfaceOut { return surface_data(clamp(i.st.xy / i.st.z, mat.region.xz, mat.region.yw), i.depth, i.wpos, i.ln.xyz, i.colour, true, false); }
"#;

pub(crate) const TERRAIN: &str = r#"
@fragment fn fs_surface_terrain(i: VOut) -> SurfaceOut { return surface_data(i.uv, i.depth, i.wpos, vec3<f32>(0.0), i.colour, false, false); }
"#;

pub(crate) const LIT: &str = r#"
@fragment fn fs_surface_lit(i: VOutLit) -> SurfaceOut { return surface_data(i.uv, i.depth, i.wpos, i.normal, lit(i, renormal(i.normal)).colour, false, false); }
"#;
