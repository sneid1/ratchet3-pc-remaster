//! The vegetation's occlusion cull on the GPU (`Cull`). The port draws every shrub out to the level's shrub limit (the pop-in fix
//! for level 28, `objects::Shrubs::visible`), far beyond the game's own distances, and most of those far shrubs lie behind the
//! terrain: at Veldin's start a build with the game's own culls drew 66,000 vegetation vertices against the port's 991,000 for
//! the same pixels (12.71 against 12.75 million). Each picture, between the terrain's cover (`native::Cover`, the terrain's depth
//! alone) and the sky, the cover's depth is reduced to a pyramid of its farthest depths (`HiZ`), each drawn instance's bounding
//! sphere is tested against it, and the corners of the instances it does not hide are packed, in their order, into one index
//! list per draw the vegetation's bundles make (`shrub_batch::Batch::draw_culled`), drawn indirectly with the count the GPU
//! wrote. An instance is hidden only where every pixel of it would fail the depth test (GEQUAL) against the terrain's depth
//! alone, which the ties drawn before the vegetation only bring nearer: none of its pixels is ever drawn, and the picture is the
//! one drawn without the cull, byte for byte. Without the cover (the terrain lit per pixel, or no terrain drawn), and where the
//! drawn shrubs are too few for the pyramid to pay (`Cull::run`), every drawn instance is kept, and the lists hold the drawn
//! instances' corners only (no hidden one's within a span).

use wgpu::util::DeviceExt;

/// A drawn instance's bounding sphere (its centre in the world and its reach, `objects::Shrubs::sphere`), as the cull reads it.
pub(crate) type Sphere = [f32; 4];

/// One instance's run of corners in a list: its slot among the frame's drawn instances, its first corner in its chunk's corners
/// and their count; the list it is in follows from `List`.
#[derive(Clone, Copy)]
pub(crate) struct Piece {
    pub slot: u32,
    pub first: u32,
    pub count: u32,
}

/// The frame's lists: each the pieces of one draw (one chunk's, in the bundles' order), and the chunk it draws from.
#[derive(Default)]
pub(crate) struct Lists {
    pub pieces: Vec<Piece>,
    /// (first piece, end, chunk).
    pub lists: Vec<(u32, u32, u32)>,
}

impl Lists {
    /// A list begun for chunk `chunk` (the pieces pushed after it are its own).
    pub fn begin(&mut self, chunk: usize) {
        let at = self.pieces.len() as u32;
        self.lists.push((at, at, chunk as u32));
    }

    pub fn push(&mut self, p: Piece) {
        self.pieces.push(p);
        self.lists.last_mut().expect("a list begun").1 += 1;
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    slots: u32,
    items: u32,
    lists: u32,
    blocks: u32,
    at_items: u32,
    at_lists: u32,
    at_chunks: u32,
    at_offsets: u32,
    at_blocks: u32,
    at_args: u32,
    pad: [u32; 2],
}

/// Per frame set (`native::SETS`) the cull's buffers, grown as a frame needs: what the CPU writes (`input`: the spheres, the
/// pieces, the lists, each chunk's pieces), what the GPU works in (`state`: each slot's verdict, each piece's place, the scan's
/// blocks, each list's indirect draw), the packed corners, and each chunk's range of pieces.
struct Buffers {
    params: wgpu::Buffer,
    input: wgpu::Buffer,
    state: wgpu::Buffer,
    packed: wgpu::Buffer,
    sizes: (u64, u64, u64),
}

/// A frame's cull, as its pictures run it (`Cull::run`): its bind groups, its sizes, and the buffers its bundles draw from.
pub(crate) struct FrameCull {
    group: wgpu::BindGroup,
    /// Each chunk's group (its corners and its range of pieces) and its piece count.
    chunks: Vec<(wgpu::BindGroup, u32)>,
    slots: u32,
    lists: u32,
    blocks: u32,
    pub packed: wgpu::Buffer,
    pub state: wgpu::Buffer,
    /// Where list `l`'s indirect draw is in `state` (bytes).
    at_args: u64,
    /// The corners the lists hold before the cull (`run`'s cost rule).
    corners: u64,
}

impl FrameCull {
    pub fn args_offset(&self, list: usize) -> u64 {
        self.at_args + 20 * list as u64
    }
}

/// The pyramid of the cover's farthest depths at one picture size: level k's texel the smallest depth (the farthest: depth is
/// 1 at NEAR, 0 at FAR) of the 2^(k+1) by 2^(k+1) pixels it covers.
struct HiZ {
    size: (u32, u32),
    levels: u32,
    /// Level k as a storage target (the first level's is the depth's group's, made each picture), the groups that make each next
    /// level from the one before, and the test's group (the whole pyramid and the picture's size).
    writes: Vec<wgpu::TextureView>,
    next: Vec<wgpu::BindGroup>,
    cull: wgpu::BindGroup,
}

pub(crate) struct Cull {
    first: wgpu::ComputePipeline,
    next: wgpu::ComputePipeline,
    cull: wgpu::ComputePipeline,
    keep: wgpu::ComputePipeline,
    scan: wgpu::ComputePipeline,
    blocks: wgpu::ComputePipeline,
    finish: wgpu::ComputePipeline,
    lists: wgpu::ComputePipeline,
    scatter: wgpu::ComputePipeline,
    frame_layout: wgpu::BindGroupLayout,
    first_layout: wgpu::BindGroupLayout,
    next_layout: wgpu::BindGroupLayout,
    hiz_layout: wgpu::BindGroupLayout,
    chunk_layout: wgpu::BindGroupLayout,
    hiz: std::sync::Mutex<Option<HiZ>>,
    sets: std::sync::Mutex<Vec<Option<Buffers>>>,
}

const SHADER: &str = r#"
struct Params { slots: u32, items: u32, lists: u32, blocks: u32, at_items: u32, at_lists: u32, at_chunks: u32, at_offsets: u32, at_blocks: u32, at_args: u32, pad0: u32, pad1: u32 };
struct View { m: mat4x4<f32> };
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<uniform> view: View;
@group(0) @binding(2) var<storage, read> input: array<vec4<u32>>;
@group(0) @binding(3) var<storage, read_write> state: array<u32>;
@group(0) @binding(4) var<storage, read_write> packed: array<u32>;

// The pyramid's first level from the depth; each next from the one before. A texel past the picture's edge holds 1 (the nearest:
// no pixel there to keep).
@group(1) @binding(0) var depth: texture_depth_2d;
@group(1) @binding(1) var first_out: texture_storage_2d<r32float, write>;
@compute @workgroup_size(8, 8) fn hiz_first(@builtin(global_invocation_id) g: vec3<u32>) {
    let out = textureDimensions(first_out);
    if (g.x >= out.x || g.y >= out.y) { return; }
    let size = textureDimensions(depth);
    var d = 1.0;
    for (var k = 0u; k < 4u; k++) {
        let c = vec2<u32>(2u * g.x + (k & 1u), 2u * g.y + (k >> 1u));
        if (c.x < size.x && c.y < size.y) { d = min(d, textureLoad(depth, c, 0)); }
    }
    textureStore(first_out, g.xy, vec4<f32>(d, 0.0, 0.0, 0.0));
}

@group(1) @binding(0) var below: texture_2d<f32>;
@group(1) @binding(1) var next_out: texture_storage_2d<r32float, write>;
@compute @workgroup_size(8, 8) fn hiz_next(@builtin(global_invocation_id) g: vec3<u32>) {
    let out = textureDimensions(next_out);
    if (g.x >= out.x || g.y >= out.y) { return; }
    let size = textureDimensions(below);
    var d = 1.0;
    for (var k = 0u; k < 4u; k++) {
        let c = vec2<u32>(2u * g.x + (k & 1u), 2u * g.y + (k >> 1u));
        if (c.x < size.x && c.y < size.y) { d = min(d, textureLoad(below, c, 0).r); }
    }
    textureStore(next_out, g.xy, vec4<f32>(d, 0.0, 0.0, 0.0));
}

// Each drawn instance's verdict: 1 drawn, 0 hidden. Hidden only when the nearest depth any of its pixels can have (the nearest
// corner of the box around its sphere, with a ten-thousandth to spare) is farther than the farthest terrain depth over every
// pixel the box can cover (a pixel more each side); never with a corner at or behind the eye.
@group(1) @binding(0) var hiz: texture_2d<f32>;
@group(1) @binding(1) var<uniform> picture: vec4<u32>;
fn hidden(s: vec4<f32>) -> bool {
    var lo = vec2<f32>(1e30);
    var hi = vec2<f32>(-1e30);
    var near = -1e30;
    for (var k = 0u; k < 8u; k++) {
        let o = vec3<f32>(select(-1.0, 1.0, (k & 1u) != 0u), select(-1.0, 1.0, (k & 2u) != 0u), select(-1.0, 1.0, (k & 4u) != 0u));
        let c = view.m * vec4<f32>(s.xyz + s.w * o, 1.0);
        if (c.w <= 1e-3) { return false; }
        let n = c.xyz / c.w;
        lo = min(lo, n.xy);
        hi = max(hi, n.xy);
        near = max(near, n.z);
    }
    let w = f32(picture.x);
    let h = f32(picture.y);
    let x0 = i32(clamp(floor((lo.x * 0.5 + 0.5) * w) - 1.0, 0.0, w - 1.0));
    let x1 = i32(clamp(ceil((hi.x * 0.5 + 0.5) * w) + 1.0, 0.0, w - 1.0));
    let y0 = i32(clamp(floor((0.5 - hi.y * 0.5) * h) - 1.0, 0.0, h - 1.0));
    let y1 = i32(clamp(ceil((0.5 - lo.y * 0.5) * h) + 1.0, 0.0, h - 1.0));
    // The level whose texels span at most four across the box each way (level k's texel 2^(k+1) pixels).
    var k = 0u;
    loop {
        let sh = k + 1u;
        if (((x1 >> sh) - (x0 >> sh)) < 4 && ((y1 >> sh) - (y0 >> sh)) < 4) { break; }
        if (k + 1u >= picture.z) { break; }
        k++;
    }
    let sh = k + 1u;
    var far = 1.0;
    for (var y = y0 >> sh; y <= (y1 >> sh); y++) {
        for (var x = x0 >> sh; x <= (x1 >> sh); x++) {
            far = min(far, textureLoad(hiz, vec2<i32>(x, y), i32(k)).r);
        }
    }
    return near * 1.0001 < far;
}
@compute @workgroup_size(64) fn cull(@builtin(global_invocation_id) g: vec3<u32>) {
    if (g.x >= p.slots) { return; }
    state[g.x] = select(1u, 0u, hidden(bitcast<vec4<f32>>(input[g.x])));
}
// Without the cover: every drawn instance kept.
@compute @workgroup_size(64) fn keep(@builtin(global_invocation_id) g: vec3<u32>) {
    if (g.x >= p.slots) { return; }
    state[g.x] = 1u;
}

// The pieces' places in the packed corners: each piece's count where its instance is drawn, scanned in its order (each block of
// 256 here, the blocks' totals in `scan_blocks`, both added in `finish`).
var<workgroup> sh: array<u32, 256>;
fn kept(i: u32) -> u32 {
    let it = input[p.at_items + i];
    return select(0u, it.z, state[it.x] != 0u);
}
fn scan_shared(l: u32) {
    for (var o = 1u; o < 256u; o = o << 1u) {
        var v = 0u;
        if (l >= o) { v = sh[l - o]; }
        workgroupBarrier();
        sh[l] += v;
        workgroupBarrier();
    }
}
@compute @workgroup_size(256) fn scan(@builtin(global_invocation_id) g: vec3<u32>, @builtin(local_invocation_id) l: vec3<u32>, @builtin(workgroup_id) w: vec3<u32>) {
    var c = 0u;
    if (g.x < p.items) { c = kept(g.x); }
    sh[l.x] = c;
    workgroupBarrier();
    scan_shared(l.x);
    if (g.x < p.items) { state[p.at_offsets + g.x] = sh[l.x] - c; }
    if (l.x == 255u) { state[p.at_blocks + w.x] = sh[255]; }
}
@compute @workgroup_size(256) fn scan_blocks(@builtin(local_invocation_id) l: vec3<u32>) {
    var carry = 0u;
    for (var base = 0u; base < p.blocks; base += 256u) {
        let i = base + l.x;
        var v = 0u;
        if (i < p.blocks) { v = state[p.at_blocks + i]; }
        sh[l.x] = v;
        workgroupBarrier();
        scan_shared(l.x);
        if (i < p.blocks) { state[p.at_blocks + i] = carry + sh[l.x] - v; }
        carry += sh[255];
        workgroupBarrier();
    }
}
@compute @workgroup_size(256) fn finish(@builtin(global_invocation_id) g: vec3<u32>, @builtin(workgroup_id) w: vec3<u32>) {
    if (g.x < p.items) { state[p.at_offsets + g.x] += state[p.at_blocks + w.x]; }
}
// Each list's indirect draw: its pieces' packed corners, from the first's place.
@compute @workgroup_size(64) fn lists(@builtin(global_invocation_id) g: vec3<u32>) {
    if (g.x >= p.lists) { return; }
    let r = input[p.at_lists + g.x];
    var first = 0u;
    var count = 0u;
    if (r.y > r.x) {
        first = state[p.at_offsets + r.x];
        count = state[p.at_offsets + r.y - 1u] + kept(r.y - 1u) - first;
    }
    let a = p.at_args + 5u * g.x;
    state[a] = count;
    state[a + 1u] = 1u;
    state[a + 2u] = first;
    state[a + 3u] = 0u;
    state[a + 4u] = 0u;
}

// One chunk's kept pieces' corners copied into their places, a workgroup a piece.
@group(1) @binding(0) var<storage, read> corners: array<u32>;
@group(1) @binding(1) var<uniform> range: vec4<u32>;
@compute @workgroup_size(64) fn scatter(@builtin(workgroup_id) w: vec3<u32>, @builtin(num_workgroups) n: vec3<u32>, @builtin(local_invocation_id) l: vec3<u32>) {
    let k = w.x + w.y * n.x;
    if (k >= range.y) { return; }
    let i = input[p.at_chunks + range.x + k].x;
    let it = input[p.at_items + i];
    if (state[it.x] == 0u) { return; }
    let at = state[p.at_offsets + i];
    for (var j = l.x; j < it.z; j += 64u) {
        packed[at + j] = corners[it.y + j];
    }
}
"#;

/// The most workgroups a dispatch's x holds; more go into y.
const MOST_X: u32 = 65535;

fn dispatch(pass: &mut wgpu::ComputePass<'_>, groups: u32) {
    if groups > 0 {
        pass.dispatch_workgroups(groups.min(MOST_X), groups.div_ceil(MOST_X), 1);
    }
}

impl Cull {
    pub fn new(device: &wgpu::Device) -> Cull {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("vegetation cull"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let buffer = |binding: u32, ty: wgpu::BufferBindingType| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None }, count: None };
        let storage = |read_only| wgpu::BufferBindingType::Storage { read_only };
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vegetation cull frame"),
            entries: &[buffer(0, wgpu::BufferBindingType::Uniform), buffer(1, wgpu::BufferBindingType::Uniform), buffer(2, storage(true)), buffer(3, storage(false)), buffer(4, storage(false))],
        });
        let texture = |binding: u32, sample_type| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Texture { sample_type, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None };
        let target = wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: wgpu::TextureFormat::R32Float, view_dimension: wgpu::TextureViewDimension::D2 }, count: None };
        let unfiltered = wgpu::TextureSampleType::Float { filterable: false };
        let first_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("vegetation cull depth"), entries: &[texture(0, wgpu::TextureSampleType::Depth), target] });
        let next_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("vegetation cull pyramid"), entries: &[texture(0, unfiltered), target] });
        let hiz_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("vegetation cull test"), entries: &[texture(0, unfiltered), buffer(1, wgpu::BufferBindingType::Uniform)] });
        let chunk_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("vegetation cull chunk"), entries: &[buffer(0, storage(true)), buffer(1, wgpu::BufferBindingType::Uniform)] });
        let pipeline = |entry: &str, second: Option<&wgpu::BindGroupLayout>| {
            let layouts = [Some(&frame_layout), second];
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("vegetation cull"), bind_group_layouts: if second.is_some() { &layouts } else { &layouts[..1] }, immediate_size: 0 });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some("vegetation cull"), layout: Some(&layout), module: &module, entry_point: Some(entry), compilation_options: Default::default(), cache: crate::pipelines::cache() })
        };
        Cull {
            first: pipeline("hiz_first", Some(&first_layout)),
            next: pipeline("hiz_next", Some(&next_layout)),
            cull: pipeline("cull", Some(&hiz_layout)),
            keep: pipeline("keep", None),
            scan: pipeline("scan", None),
            blocks: pipeline("scan_blocks", None),
            finish: pipeline("finish", None),
            lists: pipeline("lists", None),
            scatter: pipeline("scatter", Some(&chunk_layout)),
            frame_layout,
            first_layout,
            next_layout,
            hiz_layout,
            chunk_layout,
            hiz: std::sync::Mutex::new(None),
            sets: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Frame set `set`'s cull for the frame's drawn instances (`spheres`, by slot) and lists, its corners from each chunk's
    /// `corners`; the view the frame's pictures write (`view`) is the one the cull projects with.
    pub fn frame(&self, device: &wgpu::Device, queue: &wgpu::Queue, set: usize, view: &wgpu::Buffer, spheres: &[Sphere], lists: &Lists, corners: &[Option<wgpu::Buffer>]) -> Result<FrameCull, String> {
        let (slots, items, nlists) = (spheres.len() as u32, lists.pieces.len() as u32, lists.lists.len() as u32);
        let blocks = items.div_ceil(256).max(1);
        // Each chunk's pieces, by chunk, in their order.
        let mut by_chunk: Vec<Vec<u32>> = vec![Vec::new(); corners.len()];
        for &(start, end, chunk) in &lists.lists {
            let c = by_chunk.get_mut(chunk as usize).ok_or("native: a vegetation list names a chunk past the level's")?;
            c.extend(start..end);
        }
        let mut input: Vec<[u32; 4]> = Vec::with_capacity(spheres.len() + lists.pieces.len() * 2 + lists.lists.len());
        input.extend(spheres.iter().map(|s| s.map(f32::to_bits)));
        let at_items = input.len() as u32;
        input.extend(lists.pieces.iter().map(|p| [p.slot, p.first, p.count, 0]));
        let at_lists = input.len() as u32;
        input.extend(lists.lists.iter().map(|&(s, e, c)| [s, e, c, 0]));
        let at_chunks = input.len() as u32;
        let mut ranges = Vec::with_capacity(by_chunk.len());
        for c in &by_chunk {
            ranges.push((input.len() as u32 - at_chunks, c.len() as u32));
            input.extend(c.iter().map(|&i| [i, 0, 0, 0]));
        }
        let (at_offsets, at_blocks) = (slots, slots + items);
        let at_args = at_blocks + blocks;
        let state_words = at_args + 5 * nlists;
        let packed_words: u64 = lists.pieces.iter().map(|p| p.count as u64).sum::<u64>().max(1);
        let params = Params { slots, items, lists: nlists, blocks, at_items, at_lists, at_chunks, at_offsets, at_blocks, at_args, pad: [0; 2] };
        let mut sets = self.sets.lock().map_err(|_| "native: the vegetation cull's buffers' lock is poisoned")?;
        if sets.len() <= set {
            sets.resize_with(set + 1, || None);
        }
        let need = ((input.len() as u64 * 16).max(16), state_words as u64 * 4, packed_words * 4);
        let b = match sets[set].take() {
            Some(b) if b.sizes.0 >= need.0 && b.sizes.1 >= need.1 && b.sizes.2 >= need.2 => b,
            old => {
                // A half more than this frame needs, so a few more instances drawn do not make them again.
                let grow = |n: u64, had: u64| (n + n / 2).max(had).next_multiple_of(256);
                let had = old.as_ref().map_or((0, 0, 0), |b| b.sizes);
                let sizes = (grow(need.0, had.0), grow(need.1, had.1), grow(need.2, had.2));
                let buffer = |label, size, usage| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: false });
                Buffers {
                    params: buffer("vegetation cull params", std::mem::size_of::<Params>() as u64, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST),
                    input: buffer("vegetation cull input", sizes.0, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST),
                    state: buffer("vegetation cull state", sizes.1, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::INDIRECT),
                    packed: buffer("vegetation packed corners", sizes.2, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::INDEX),
                    sizes,
                }
            }
        };
        queue.write_buffer(&b.params, 0, bytemuck::bytes_of(&params));
        if !input.is_empty() {
            queue.write_buffer(&b.input, 0, bytemuck::cast_slice(&input));
        }
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vegetation cull frame"),
            layout: &self.frame_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: b.params.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: view.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: b.input.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: b.state.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: b.packed.as_entire_binding() },
            ],
        });
        let mut chunks = Vec::new();
        for (c, (&(start, n), source)) in ranges.iter().zip(corners).enumerate() {
            if n == 0 {
                continue;
            }
            let source = source.as_ref().ok_or_else(|| format!("native: vegetation chunk {c} is listed and has no corners"))?;
            let range = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("vegetation cull chunk"), contents: bytemuck::cast_slice(&[start, n, 0, 0]), usage: wgpu::BufferUsages::UNIFORM });
            let g = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("vegetation cull chunk"),
                layout: &self.chunk_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: source.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: range.as_entire_binding() }],
            });
            chunks.push((g, n));
        }
        let fc = FrameCull { group, chunks, slots, lists: nlists, blocks, packed: b.packed.clone(), state: b.state.clone(), at_args: at_args as u64 * 4, corners: packed_words };
        sets[set] = Some(b);
        Ok(fc)
    }

    /// The pyramid at `size`, made again when the size changes.
    fn pyramid<'h>(&self, device: &wgpu::Device, queue: &wgpu::Queue, held: &'h mut Option<HiZ>, size: (u32, u32)) -> &'h HiZ {
        if held.as_ref().is_none_or(|h| h.size != size) {
            let base = (size.0.div_ceil(2).max(1), size.1.div_ceil(2).max(1));
            let levels = 32 - base.0.max(base.1).leading_zeros();
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("vegetation cull pyramid"),
                size: wgpu::Extent3d { width: base.0, height: base.1, depth_or_array_layers: 1 },
                mip_level_count: levels,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R32Float,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let level = |k: u32| tex.create_view(&wgpu::TextureViewDescriptor { label: Some("vegetation cull level"), base_mip_level: k, mip_level_count: Some(1), ..Default::default() });
            let writes: Vec<wgpu::TextureView> = (0..levels).map(level).collect();
            let reads: Vec<wgpu::TextureView> = (0..levels).map(level).collect();
            let next = (1..levels as usize)
                .map(|k| {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("vegetation cull pyramid"),
                        layout: &self.next_layout,
                        entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&reads[k - 1]) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&writes[k]) }],
                    })
                })
                .collect();
            let all = tex.create_view(&Default::default());
            let picture = device.create_buffer(&wgpu::BufferDescriptor { label: Some("vegetation cull picture"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            queue.write_buffer(&picture, 0, bytemuck::cast_slice(&[size.0, size.1, levels, 0]));
            let cull = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("vegetation cull test"),
                layout: &self.hiz_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&all) }, wgpu::BindGroupEntry { binding: 1, resource: picture.as_entire_binding() }],
            });
            *held = Some(HiZ { size, levels, writes, next, cull });
        }
        held.as_ref().expect("made above")
    }

    /// The frame's cull for one picture, into `enc`: with `depth` (the terrain's cover, the depth of the picture of `size`) its
    /// pyramid and each instance's verdict, else every instance kept; then the lists' corners packed and their draws written.
    /// The pyramid reads the whole picture's depth and writes a third of it again (some 5 bytes a pixel), where a corner drawn
    /// reads its vertex's 40 (its position, coordinates, colour, instance and material words) and runs its program: with fewer
    /// corners than a sixth of the picture's pixels, hiding every one would not pay for it, and the drawn ones are kept (at
    /// Veldin's first checkpoint 78,000 corners of 1.47 million pixels; at its start 981,000, of which the cull hides 79 %).
    pub fn run(&self, device: &wgpu::Device, queue: &wgpu::Queue, enc: &mut wgpu::CommandEncoder, frame: &FrameCull, depth: Option<(&wgpu::Texture, (u32, u32))>, profile: Option<&crate::gpu_profile::Recording>) -> Result<(), String> {
        let depth = depth.filter(|(_, (w, h))| 6 * frame.corners >= *w as u64 * *h as u64);
        let mut held = self.hiz.lock().map_err(|_| "native: the vegetation cull's pyramid lock is poisoned")?;
        let hiz = depth.map(|(d, size)| {
            let h = self.pyramid(device, queue, &mut held, size);
            let view = d.create_view(&wgpu::TextureViewDescriptor { label: Some("vegetation cull depth"), aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() });
            let first = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("vegetation cull depth"),
                layout: &self.first_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&h.writes[0]) }],
            });
            (h, first)
        });
        let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("vegetation cull"), timestamp_writes: None });
        let timing = crate::gpu_profile::Span::begin_compute(profile, &mut pass, "vegetation_cull");
        pass.set_bind_group(0, &frame.group, &[]);
        match &hiz {
            Some((h, first)) => {
                let level = |k: u32| ((h.size.0.div_ceil(2) >> k).max(1), (h.size.1.div_ceil(2) >> k).max(1));
                pass.set_pipeline(&self.first);
                pass.set_bind_group(1, first, &[]);
                let (w, hh) = level(0);
                pass.dispatch_workgroups(w.div_ceil(8), hh.div_ceil(8), 1);
                pass.set_pipeline(&self.next);
                for k in 1..h.levels {
                    pass.set_bind_group(1, &h.next[k as usize - 1], &[]);
                    let (w, hh) = level(k);
                    pass.dispatch_workgroups(w.div_ceil(8), hh.div_ceil(8), 1);
                }
                pass.set_pipeline(&self.cull);
                pass.set_bind_group(1, &h.cull, &[]);
                dispatch(&mut pass, frame.slots.div_ceil(64));
            }
            None => {
                pass.set_pipeline(&self.keep);
                dispatch(&mut pass, frame.slots.div_ceil(64));
            }
        }
        pass.set_pipeline(&self.scan);
        dispatch(&mut pass, frame.blocks);
        pass.set_pipeline(&self.blocks);
        pass.dispatch_workgroups(1, 1, 1);
        pass.set_pipeline(&self.finish);
        dispatch(&mut pass, frame.blocks);
        pass.set_pipeline(&self.lists);
        dispatch(&mut pass, frame.lists.div_ceil(64));
        pass.set_pipeline(&self.scatter);
        for (g, n) in &frame.chunks {
            pass.set_bind_group(1, g, &[]);
            dispatch(&mut pass, *n);
        }
        timing.end_compute(&mut pass);
        Ok(())
    }
}
