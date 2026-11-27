//! The ties' materials selected by the GPU, in the game's draw order. `Pipelines::draw_ties` binds a material and draws for each
//! of a frame's draws (a level's ties: a thousand a frame on Veldin); this keeps its runs, stretches and phases as they are and
//! merges the draws that need the same pipeline into one span: the material is a per-corner id (`TieCorner::material`) into a table
//! (the vegetation batch's, `shrub_batch.rs`: storage buffer of materials, binding arrays of the textures), and the span's corners
//! are an index list the frame's triangles that are drawn make (undrawn triangles never reach the vertex shader). The pixels are
//! the same: the same triangles, in the same order, under the same pipeline state and the same material values.

use crate::objects::{Blend, FastMap, Key, Materials, TieDraw};
use crate::shrub_batch::MaterialInput;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use wgpu::util::DeviceExt;

/// The most indices a frame set keeps for the frame's spans (4 bytes each): a range whose spans pass what is left is drawn
/// material by material (`draw` says so once).
const INDICES_MAX: u64 = 4 << 20;

/// A pipeline of the batch: the passing pixels (by blend, `Blend::index`), the failing ones (by AFAIL's frame buffer write, then
/// blend) and `Blend::AddDst`'s colour-only first draw.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Pipe {
    Pass(u8),
    Fail(u8, u8),
    Half,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Material {
    value: crate::native::MatU,
    // Textures 0 to 6 (the texture, the four material maps, the region map, the relief map), samplers 7 and 8, then padding:
    // three vec4<u32> in the shader's `BatchMat`.
    slots: [u32; 12],
}

/// One frame set's index list: the spans of every range's draws, a range's after the other's (`next` counts them).
struct IndexSet {
    buffer: wgpu::Buffer,
    next: AtomicU32,
}

pub(crate) struct TieBatch {
    ids: FastMap<(usize, Key), u32>,
    group: wgpu::BindGroup,
    pass: Vec<Option<wgpu::RenderPipeline>>,
    fail: [Vec<Option<wgpu::RenderPipeline>>; 2],
    half: Option<wgpu::RenderPipeline>,
    sets: Vec<IndexSet>,
    capacity: u32,
    full: AtomicBool,
}

/// A range's room for its frame's spans (`TieBatch::draw`), kept from frame to frame.
#[derive(Default)]
pub(crate) struct Room {
    order: Vec<(Pipe, u32)>,
    found: Vec<(bool, usize)>,
    indices: Vec<u32>,
    /// Per draw: where its triangles' indices were written (`NOT_PLACED` before the first step that draws it).
    placed: Vec<(u32, u32)>,
    spans: Vec<(Pipe, u32, u32)>,
}

const NOT_PLACED: u32 = u32::MAX;

/// The order `Pipelines::draw_ties` draws a frame's draws in, as (pipeline, draw): a stretch of `Blend::AddDst` shines and
/// `Blend::Replace` draws that cannot fail draws all its first parts, then 4 steps of its shines (half, passing, failing RGB, failing
/// frame buffer); every other draw belongs to a run of one TEST_1 that draws all its draws' passing pixels, then all their failing
/// ones. Only the draws' materials are looked up; a missing one, a GS blend and a fade (a bundle cannot set the blend constant) are
/// errors, as in `draw_ties`.
pub(crate) fn order(materials: &[Materials], draws: &[TieDraw], found: &mut Vec<(bool, usize)>, out: &mut Vec<(Pipe, u32)>) -> Result<(), String> {
    out.clear();
    found.clear();
    for dr in draws {
        let m = &materials[dr.test];
        let (_, fails) = m.keys.get(&dr.key).ok_or("native: a tie texture setting made at load is missing")?;
        found.push((*fails, m.fb_only as usize));
    }
    let shine = |dr: &TieDraw| dr.key.5 == Blend::AddDst;
    let opaque = |at: usize| draws[at].key.5 == Blend::Replace && !found[at].0;
    let mut k = 0;
    while k < draws.len() {
        let mut n = 0;
        while k + n < draws.len() && (shine(&draws[k + n]) || opaque(k + n)) {
            n += 1;
        }
        if n > 0 {
            for j in (k..k + n).filter(|&j| !shine(&draws[j])) {
                out.push((Pipe::Pass(Blend::Replace.index() as u8), j as u32));
            }
            let add = Blend::AddDst.index() as u8;
            for step in 0..4 {
                for j in (k..k + n).filter(|&j| shine(&draws[j])) {
                    let (fails, fb) = found[j];
                    if step >= 2 && !fails {
                        continue;
                    }
                    out.push((
                        match step {
                            0 => Pipe::Half,
                            1 => Pipe::Pass(add),
                            2 => Pipe::Fail(0, add),
                            _ => Pipe::Fail(fb as u8, add),
                        },
                        j as u32,
                    ));
                }
            }
            k += n;
            continue;
        }
        // A run of one TEST_1's other draws.
        let t = draws[k].test;
        let mut n = 0;
        while k + n < draws.len() && draws[k + n].test == t && (n == 0 || !(shine(&draws[k + n]) || opaque(k + n))) {
            n += 1;
        }
        let fb = materials[t].fb_only as u8;
        for phase in 0..2 {
            for j in k..k + n {
                let blend = draws[j].key.5;
                if let Blend::Gs(..) | Blend::Fade(_) = blend {
                    return Err(format!("native: a {blend:?} blend of a tie in the batch"));
                }
                if phase == 1 && !found[j].0 {
                    continue;
                }
                out.push((if phase == 0 { Pipe::Pass(blend.index() as u8) } else { Pipe::Fail(fb, blend.index() as u8) }, j as u32));
            }
        }
        k += n;
    }
    Ok(())
}

impl TieBatch {
    /// The batch of a level's tie materials, `(test, key, input, whether its pixels can fail the alpha test, AFAIL FB_ONLY)` each
    /// (`test` selects `Ties::materials`); `Ok(None)` (a line naming why) where the level's tables pass this GPU's binding arrays.
    pub fn new(device: &wgpu::Device, view: &wgpu::BindGroupLayout, tie_layout: &wgpu::BindGroupLayout, inputs: Vec<(usize, Key, MaterialInput, bool, bool)>) -> Result<Option<Self>, String> {
        if inputs.is_empty() {
            return Err("native: tie batch has no materials".into());
        }
        let (mut ids, mut materials, mut textures, mut samplers) = (FastMap::default(), Vec::new(), Vec::new(), Vec::new());
        let mut needs: Vec<(Key, bool, bool)> = Vec::new();
        for (test, key, input, fails, fb) in inputs {
            let mut slots = [0; 12];
            for (i, texture) in input.textures.into_iter().enumerate() {
                slots[i] = match textures.iter().position(|t| *t == texture) {
                    Some(k) => k as u32,
                    None => {
                        textures.push(texture);
                        (textures.len() - 1) as u32
                    }
                };
            }
            for (i, sampler) in input.samplers.into_iter().enumerate() {
                slots[7 + i] = match samplers.iter().position(|s| *s == sampler) {
                    Some(k) => k as u32,
                    None => {
                        samplers.push(sampler);
                        (samplers.len() - 1) as u32
                    }
                };
            }
            ids.insert((test, key), materials.len() as u32);
            materials.push(Material { value: input.uniform, slots });
            needs.push((key, fails, fb));
        }
        // The same limits as the vegetation's tables (`shrub_batch::Batch::new`).
        let limits = device.limits();
        if (textures.len() + samplers.len()) as u32 > limits.max_binding_array_elements_per_shader_stage || samplers.len() as u32 > limits.max_binding_array_sampler_elements_per_shader_stage {
            println!(
                "native: this level's ties drawn material by material: its {} textures and {} samplers pass this GPU's binding arrays ({} elements, {} samplers)",
                textures.len(),
                samplers.len(),
                limits.max_binding_array_elements_per_shader_stage,
                limits.max_binding_array_sampler_elements_per_shader_stage
            );
            return Ok(None);
        }
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tie material table"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: std::num::NonZeroU32::new(textures.len() as u32),
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: std::num::NonZeroU32::new(samplers.len() as u32),
                },
            ],
        });
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("tie materials"), contents: bytemuck::cast_slice(&materials), usage: wgpu::BufferUsages::STORAGE });
        let texture_refs: Vec<_> = textures.iter().collect();
        let sampler_refs: Vec<_> = samplers.iter().collect();
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tie material table"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureViewArray(&texture_refs) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::SamplerArray(&sampler_refs) },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("GPU tie materials"),
            source: wgpu::ShaderSource::Wgsl(crate::shrub_batch::rewrite(None, vertex).into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("GPU ties"), bind_group_layouts: &[Some(view), Some(&layout), Some(tie_layout)], immediate_size: 0 });
        let attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Uint32x2, 2 => Unorm8x4, 3 => Uint32, 4 => Snorm8x4, 5 => Float32x2, 6 => Uint32, 7 => Uint32];
        let blends = crate::objects::blend_states();
        let add_dst = Blend::AddDst.index();
        // `entry`: the fragment entry point; `alpha`: whether the pixel writes its alpha; the passing pixels (depth written) of a
        // blend but `AddDst`'s have a visible-surface twin (`surface_buffer::pipeline`), as `Pipelines`' ties' do.
        let make = |entry: &str, depth_write: bool, b: usize, alpha: bool| {
            crate::surface_buffer::pipeline(device, &wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_tie_batch"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<crate::objects::TieCorner>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs })],
                },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState { format: crate::native::DEPTH, depth_write_enabled: Some(depth_write), depth_compare: Some(wgpu::CompareFunction::GreaterEqual), stencil: Default::default(), bias: Default::default() }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: blends[b], write_mask: if alpha { wgpu::ColorWrites::ALL } else { wgpu::ColorWrites::COLOR } })],
                }),
                multiview_mask: None,
                cache: crate::pipelines::cache(),
            }, (depth_write && b != add_dst).then_some("batch_surface"))
        };
        let (mut pass, mut fail, mut half): (Vec<Option<wgpu::RenderPipeline>>, [Vec<Option<wgpu::RenderPipeline>>; 2], Option<wgpu::RenderPipeline>) = (vec![None; blends.len()], [vec![None; blends.len()], vec![None; blends.len()]], None);
        // The pipelines the level's settings draw with, as `draw_ties` chooses them.
        for (key, fails, fb) in needs {
            let b = key.5.index();
            if pass[b].is_none() {
                pass[b] = Some(make("batch_pass", true, b, true));
            }
            if b == add_dst && half.is_none() {
                half = Some(make("batch_pass", false, b, false));
            }
            if fails {
                if fail[fb as usize][b].is_none() {
                    fail[fb as usize][b] = Some(make("batch_fail", false, b, fb));
                }
                if b == add_dst && fail[0][b].is_none() {
                    fail[0][b] = Some(make("batch_fail", false, b, false));
                }
            }
        }
        println!(
            "native: GPU ties: {} materials, {} shared texture views, {} samplers (this GPU's binding arrays: {} elements, {} samplers)",
            materials.len(),
            textures.len(),
            samplers.len(),
            limits.max_binding_array_elements_per_shader_stage,
            limits.max_binding_array_sampler_elements_per_shader_stage
        );
        Ok(Some(Self { ids, group, pass, fail, half, sets: Vec::new(), capacity: 0, full: AtomicBool::new(false) }))
    }

    /// Each frame set's index list, room for `corners` corners of the level's drawing four times (a corner is drawn at most that
    /// often: `AddDst`'s shine steps) up to `INDICES_MAX`.
    pub fn allocate(&mut self, device: &wgpu::Device, corners: u64, sets: usize) {
        let capacity = (4 * corners).min(INDICES_MAX).min(device.limits().max_buffer_size / 4).max(3);
        self.capacity = capacity as u32;
        self.sets = (0..sets)
            .map(|_| IndexSet {
                buffer: device.create_buffer(&wgpu::BufferDescriptor { label: Some("tie spans"), size: capacity * 4, usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }),
                next: AtomicU32::new(0),
            })
            .collect();
    }

    /// The id of a tie texture setting (the corner's `material`).
    pub fn id(&self, test: usize, key: &Key) -> Result<u32, String> {
        self.ids.get(&(test, *key)).copied().ok_or_else(|| format!("native: tie texture setting {key:?} of test {test} has no batch material"))
    }

    /// A new frame in set `set`: its index list is empty.
    pub fn begin(&self, set: usize) {
        self.sets[set].next.store(0, Ordering::Relaxed);
    }

    fn pipeline(&self, pipe: Pipe) -> Result<&wgpu::RenderPipeline, String> {
        match pipe {
            Pipe::Pass(b) => self.pass[b as usize].as_ref(),
            Pipe::Fail(fb, b) => self.fail[fb as usize][b as usize].as_ref(),
            Pipe::Half => self.half.as_ref(),
        }
        .ok_or_else(|| format!("native: the tie batch has no pipeline {pipe:?}"))
    }

    /// The frame's draws of one range (`draws`, ranges of corners `ranges`; `bits`: the range's drawn bits, a bit a triangle from
    /// triangle `base`) as `draw_ties` draws them, in spans: the triangles drawn of each span's draws written to frame set `set`'s
    /// index list and drawn from it. `Ok(false)` (nothing recorded, a line saying so once) when the set's list has no room left.
    #[allow(clippy::too_many_arguments)]
    pub fn draw<'a, E: crate::replay::Enc<'a>>(&'a self, pass: &mut E, queue: &wgpu::Queue, set: usize, materials: &[Materials], draws: &[TieDraw], ranges: &[(u32, u32)], bits: &[u32], base: u32, room: &mut Room, stats: &mut crate::objects::TieStats) -> Result<bool, String> {
        let t_gather = std::time::Instant::now();
        order(materials, draws, &mut room.found, &mut room.order)?;
        room.indices.clear();
        room.spans.clear();
        room.placed.clear();
        room.placed.resize(draws.len(), (NOT_PLACED, 0));
        // A draw's indices are written once, at the first step that draws it (its passing pixels': the order draws a run's draws
        // one after the other, so a run's failing pixels draw the same stretch of the list again, or the pieces of it whose draws
        // can fail); a span is the stretches of the list its draws were written at that follow one another.
        for &(pipe, at) in &room.order {
            let (start, end) = match room.placed[at as usize] {
                (NOT_PLACED, _) => {
                    let dr = &draws[at as usize];
                    let start = room.indices.len() as u32;
                    for &(first, count) in &ranges[dr.first as usize..(dr.first + dr.ranges) as usize] {
                        push_triangles(&mut room.indices, first, count, bits, base);
                    }
                    let placed = (start, room.indices.len() as u32);
                    room.placed[at as usize] = placed;
                    placed
                }
                placed => placed,
            };
            match room.spans.last_mut() {
                Some((p, _, e)) if *p == pipe && *e == start => *e = end,
                _ => room.spans.push((pipe, start, end)),
            }
        }
        let len = room.indices.len() as u32;
        let list = &self.sets[set];
        let at = list.next.fetch_add(len, Ordering::Relaxed);
        if at as u64 + len as u64 > self.capacity as u64 {
            if !self.full.swap(true, Ordering::Relaxed) {
                println!("native: a frame's tie spans pass the {} indices a frame set keeps: that range is drawn material by material", self.capacity);
            }
            return Ok(false);
        }
        let t_write = std::time::Instant::now();
        stats.gather_ns += (t_write - t_gather).as_nanos() as u64;
        if len > 0 {
            queue.write_buffer(&list.buffer, at as u64 * 4, bytemuck::cast_slice(&room.indices));
        }
        stats.write_ns += t_write.elapsed().as_nanos() as u64;
        pass.set_bind_group(1, &self.group);
        pass.set_index_buffer(&list.buffer);
        let mut spans = 0;
        for &(pipe, from, to) in &room.spans {
            if to > from {
                pass.set_pipeline(self.pipeline(pipe)?);
                pass.draw_indexed(at + from..at + to);
                spans += 1;
            }
        }
        stats.spans += spans;
        stats.indices += len as usize;
        Ok(true)
    }
}

/// The drawn triangles among a range of corners (`first`, `count`), their corners as indices: triangle `t`'s bit is bit `t - base`
/// of `bits` (`Ties::frame` sets it for a first draw's triangle the game's program draws and for every second draw's).
fn push_triangles(out: &mut Vec<u32>, first: u32, count: u32, bits: &[u32], base: u32) {
    let base = base / 3;
    let (mut l, end) = (first / 3 - base, (first + count) / 3 - base);
    while l < end {
        // The rest of this word's triangles that are in the range, as the word's low bits.
        let n = (32 - (l & 31)).min(end - l);
        let word = bits[(l >> 5) as usize] >> (l & 31);
        let mut set = if n == 32 { word } else { word & ((1 << n) - 1) };
        if set == if n == 32 { u32::MAX } else { (1 << n) - 1 } {
            out.extend(3 * (base + l)..3 * (base + l + n));
        } else {
            while set != 0 {
                let t = base + l + set.trailing_zeros();
                out.extend_from_slice(&[3 * t, 3 * t + 1, 3 * t + 2]);
                set &= set - 1;
            }
        }
        l += n;
    }
}

/// The ties' vertex stage for the batch: `vs_tie` with the corner's material id passed to the pixel (`@location(7)`).
fn vertex(mut s: String) -> String {
    s = s.replace("@vertex fn vs_tie(v: VInTie, @builtin(vertex_index) i: u32) -> VOut {", "fn vs_tie(v: VInTie, i: u32) -> VOut {");
    s.push_str(
        r#"
struct VInTieBatch { @location(0) pos: vec3<f32>, @location(1) st: vec2<u32>, @location(2) colour: vec4<f32>, @location(3) slot: u32, @location(4) ln: vec4<f32>, @location(5) ls: vec2<f32>, @location(6) light: u32, @location(7) material: u32 };
@vertex fn vs_tie_batch(v: VInTieBatch, @builtin(vertex_index) i: u32) -> VOut {
    var o = vs_tie(VInTie(v.pos, v.st, v.colour, v.slot, v.ln, v.ls, v.light), i);
    o.material = v.material;
    return o;
}
"#,
    );
    s
}

impl Drop for TieBatch {
    fn drop(&mut self) {
        for p in self.pass.iter().flatten() {
            crate::surface_buffer::unregister(p);
        }
    }
}
