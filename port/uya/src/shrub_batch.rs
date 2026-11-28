//! Vegetation materials selected by the GPU, in the original triangle order. A draw spans adjacent material runs, and the
//! vertex shader rejects hidden instances through their fog entry (-1). This avoids thousands of CPU material binds without
//! sorting translucent leaves. A device without texture binding arrays gets the same draws with the base textures in one
//! `texture_2d_array` (`Batch::layered`), and a level that does not fit one is drawn material by material.

use crate::objects::{Draw, Key, ShrubVertex};
use std::collections::HashMap;
use wgpu::util::DeviceExt;

pub(crate) struct MaterialInput {
    pub uniform: crate::native::MatU,
    pub textures: [wgpu::TextureView; 7],
    pub samplers: [wgpu::Sampler; 2],
    /// The base texture's address modes (`true`: clamp to edge, else repeat), u then v: the one texture array's sampler repeats,
    /// so the shader does each axis's own mode (`Batch::layered`).
    pub clamp: [bool; 2],
    pub filter: Filter,
}

/// A base texture sampler's filters; the texture array's one sampler takes them, so every material of a level must agree.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Filter {
    pub min: wgpu::FilterMode,
    pub mip: wgpu::MipmapFilterMode,
    pub aniso: u16,
    pub lod_max: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Material {
    value: crate::native::MatU,
    // Textures 0 to 6 (the texture, the four material maps, the region map, the relief map), samplers 7 and 8, then padding:
    // three vec4<u32> in the shader's `BatchMat`.
    slots: [u32; 12],
}

/// A material of the texture array form: the value, then its layer's place (`geo`: the texture's share of the layer in u and v,
/// then half a texel of the texture in each; `lay`: the layer, then 1 for each axis that clamps, then the sampler's anisotropy).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LayerMaterial {
    value: crate::native::MatU,
    geo: [f32; 4],
    lay: [u32; 4],
}

/// The texels past a repeating (or clamping) axis's edge each layer's mips hold: a few, so that bilinear, trilinear and
/// moderate anisotropy stay inside the layer's copy.
const BORDER: u32 = 8;

/// The array's size limit, in bytes.
const ARRAY_MAX: u64 = 256 << 20;

/// The most hidden corners a draw spans to reach the next run of its chunk (`Batch::draw`): past it the span ends there and a
/// draw of its own begins at the run.
const SPAN_GAP: u32 = 1024;

pub(crate) fn supported(device: &wgpu::Device) -> bool {
    device.features().contains(
        wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )
}

pub(crate) struct Batch {
    ids: HashMap<Key, u32>,
    group: wgpu::BindGroup,
    pass: wgpu::RenderPipeline,
    fail: wgpu::RenderPipeline,
}

impl Batch {
    pub fn new(
        device: &wgpu::Device,
        view: &wgpu::BindGroupLayout,
        fog: &wgpu::BindGroupLayout,
        inputs: impl IntoIterator<Item = (Key, MaterialInput)>,
    ) -> Result<Option<Self>, String> {
        let (mut ids, mut materials, mut textures, mut samplers) =
            (HashMap::new(), Vec::new(), Vec::new(), Vec::new());
        for (key, input) in inputs {
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
            ids.insert(key, materials.len() as u32);
            materials.push(Material {
                value: input.uniform,
                slots,
            });
        }
        if materials.is_empty() {
            return Err("native: vegetation batch has no materials".into());
        }
        // The tables are binding arrays: wgpu counts all their elements against max_binding_array_elements_per_shader_stage and
        // the samplers again against max_binding_array_sampler_elements_per_shader_stage (wgpu-core's
        // BindingTypeMaxCountValidator; on Vulkan the update-after-bind descriptor limits), not against the per-stage sampled
        // texture and sampler limits (58 and 64 on Intel's Windows driver, where a level's vegetation with the updated textures
        // uses up to 95 views). A level whose tables pass them is drawn material by material, the way a GPU without the arrays
        // draws every level (`Shrubs::new`).
        let limits = device.limits();
        if (textures.len() + samplers.len()) as u32 > limits.max_binding_array_elements_per_shader_stage
            || samplers.len() as u32 > limits.max_binding_array_sampler_elements_per_shader_stage
        {
            println!(
                "native: this level's vegetation drawn material by material: its {} textures and {} samplers pass this GPU's binding arrays ({} elements, {} samplers)",
                textures.len(),
                samplers.len(),
                limits.max_binding_array_elements_per_shader_stage,
                limits.max_binding_array_sampler_elements_per_shader_stage
            );
            return Ok(None);
        }
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vegetation material table"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
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
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vegetation materials"),
            contents: bytemuck::cast_slice(&materials),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let texture_refs: Vec<_> = textures.iter().collect();
        let sampler_refs: Vec<_> = samplers.iter().collect();
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vegetation material table"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureViewArray(&texture_refs),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::SamplerArray(&sampler_refs),
                },
            ],
        });
        let (pass, fail) = Self::pipelines(device, view, fog, &layout, shader(None));
        println!(
            "native: GPU vegetation: {} materials, {} shared texture views, {} samplers (this GPU's binding arrays: {} elements, {} samplers)",
            materials.len(),
            textures.len(),
            samplers.len(),
            limits.max_binding_array_elements_per_shader_stage,
            limits.max_binding_array_sampler_elements_per_shader_stage
        );
        Ok(Some(Self { ids, group, pass, fail }))
    }

    /// The passing and failing pipelines of a material table's layout and shader.
    fn pipelines(device: &wgpu::Device, view: &wgpu::BindGroupLayout, fog: &wgpu::BindGroupLayout, layout: &wgpu::BindGroupLayout, source: String) -> (wgpu::RenderPipeline, wgpu::RenderPipeline) {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("GPU vegetation materials"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("GPU vegetation"),
            bind_group_layouts: &[Some(view), Some(layout), Some(fog)],
            immediate_size: 0,
        });
        let attrs =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4, 3 => Uint32];
        let mat_attr = wgpu::vertex_attr_array![4 => Uint32, 5 => Uint32];
        let pipeline = |fail: bool| {
            crate::surface_buffer::pipeline(device, &wgpu::RenderPipelineDescriptor {
                label: Some("GPU vegetation"),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_shrub"),
                    compilation_options: Default::default(),
                    buffers: &[
                        Some(wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<ShrubVertex>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &attrs,
                        }),
                        Some(wgpu::VertexBufferLayout {
                            array_stride: 8,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &mat_attr,
                        }),
                    ],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: crate::native::DEPTH,
                    depth_write_enabled: Some(!fail),
                    depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(if fail { "batch_fail" } else { "batch_pass" }),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        blend: Some(wgpu::BlendState {
                            color: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::Src1Alpha,
                                dst_factor: wgpu::BlendFactor::OneMinusSrc1Alpha,
                                operation: wgpu::BlendOperation::Add,
                            },
                            alpha: wgpu::BlendComponent::REPLACE,
                        }),
                        write_mask: if fail {
                            wgpu::ColorWrites::COLOR
                        } else {
                            wgpu::ColorWrites::ALL
                        },
                    })],
                }),
                multiview_mask: None,
                cache: crate::pipelines::cache(),
            }, (!fail).then_some("batch_surface"))
        };
        (pipeline(false), pipeline(true))
    }

    /// The batch for a GPU without texture binding arrays: the level's base textures in one `texture_2d_array` (a layer for each
    /// texture and its address modes), the other slots single bindings. `Ok(None)` (a line naming why) where the level's vegetation
    /// does not fit that form.
    pub fn layered(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::BindGroupLayout,
        fog: &wgpu::BindGroupLayout,
        inputs: impl IntoIterator<Item = (Key, MaterialInput)>,
    ) -> Result<Option<Self>, String> {
        let inputs: Vec<(Key, MaterialInput)> = inputs.into_iter().collect();
        let Some((_, first)) = inputs.first() else { return Err("native: vegetation batch has no materials".into()) };
        let fallback = |why: String| {
            println!("native: this level's vegetation drawn material by material: {why}");
            Ok(None)
        };
        // One view and one sampler for every slot but the base texture, and one set of base filters.
        for (_, i) in &inputs {
            if i.textures[1..] != first.textures[1..] || i.samplers[1] != first.samplers[1] {
                return fallback("its materials' Rift Apart maps differ".into());
            }
            if i.filter != first.filter {
                return fallback(format!("its base textures' filters differ ({:?} and {:?})", first.filter, i.filter));
            }
        }
        // A layer for each base texture and its address modes (the borders hold what those modes read past an edge).
        let (mut ids, mut materials, mut layers): (HashMap<Key, u32>, Vec<crate::native::MatU>, Vec<(wgpu::TextureView, [bool; 2])>) = (HashMap::new(), Vec::new(), Vec::new());
        let (mut max_w, mut max_h) = (1, 1);
        let mut shapes = Vec::new();
        for (key, input) in &inputs {
            let base = &input.textures[0];
            let size = base.texture().size();
            let (w, h) = (size.width, size.height);
            if !w.is_power_of_two() || !h.is_power_of_two() {
                return fallback(format!("a base texture of {w} x {h} texels is not a power of two"));
            }
            let layer = match layers.iter().position(|(v, c)| v == base && *c == input.clamp) {
                Some(l) => l,
                None => {
                    layers.push((base.clone(), input.clamp));
                    (max_w, max_h) = (max_w.max(w), max_h.max(h));
                    layers.len() - 1
                }
            };
            shapes.push((layer as u32, w, h));
            ids.insert(*key, materials.len() as u32);
            materials.push(input.uniform);
        }
        let (full_w, full_h) = ((2 * max_w).next_power_of_two(), (2 * max_h).next_power_of_two());
        let mips = 32 - full_w.max(full_h).leading_zeros();
        let level = |n: u32, k: u32| (n >> k).max(1);
        let layer_bytes: u64 = (0..mips).map(|k| level(full_w, k) as u64 * level(full_h, k) as u64 * 4).sum();
        let bytes = layer_bytes * layers.len() as u64;
        let limits = device.limits();
        if full_w.max(full_h) > limits.max_texture_dimension_2d || layers.len() as u32 > limits.max_texture_array_layers {
            return fallback(format!("its {} layers of {full_w} x {full_h} texels pass this GPU's texture limits ({} texels, {} layers)", layers.len(), limits.max_texture_dimension_2d, limits.max_texture_array_layers));
        }
        if bytes > ARRAY_MAX {
            return fallback(format!("its {} layers of {full_w} x {full_h} texels are {} MB, past {} MB", layers.len(), bytes >> 20, ARRAY_MAX >> 20));
        }
        let array = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("vegetation base textures"),
            size: wgpu::Extent3d { width: full_w, height: full_h, depth_or_array_layers: layers.len() as u32 },
            mip_level_count: mips,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        Self::fill(device, queue, &array, &layers, (full_w, full_h), mips);
        let materials: Vec<LayerMaterial> = materials
            .into_iter()
            .zip(&shapes)
            .zip(&inputs)
            .map(|((value, &(layer, w, h)), (_, input))| LayerMaterial {
                value,
                geo: [w as f32 / full_w as f32, h as f32 / full_h as f32, 0.5 / w as f32, 0.5 / h as f32],
                lay: [layer, input.clamp[0] as u32, input.clamp[1] as u32, input.filter.aniso as u32],
            })
            .collect();
        // The sampler stops at the deepest level any layer's texture has (the layer's mips past a texture's own last level are not
        // the texture's): where every texture has the same levels that is each one's own limit, as a texture of its own mips has.
        let top = layers.iter().map(|(v, _)| v.texture().mip_level_count() - 1).max().expect("a layer");
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("vegetation base textures"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: first.filter.min,
            mipmap_filter: first.filter.mip,
            anisotropy_clamp: first.filter.aniso,
            lod_max_clamp: first.filter.lod_max.min(top as f32),
            ..Default::default()
        });
        let map = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vegetation material table"),
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                map(3),
                wgpu::BindGroupLayoutEntry { binding: 4, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                map(5),
                map(6),
                map(7),
                map(8),
                map(9),
            ],
        });
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vegetation materials"),
            contents: bytemuck::cast_slice(&materials),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let array_view = array.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let t = &first.textures;
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vegetation material table"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&array_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&t[1]) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&first.samplers[1]) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&t[2]) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&t[3]) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(&t[4]) },
                wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::TextureView(&t[5]) },
                wgpu::BindGroupEntry { binding: 9, resource: wgpu::BindingResource::TextureView(&t[6]) },
            ],
        });
        let (pass, fail) = Self::pipelines(device, view, fog, &layout, shader(Some(top)));
        println!(
            "native: GPU vegetation in one texture array: {} materials, {} layers of {full_w} x {full_h} texels ({mips} mips), {:.1} MB",
            materials.len(),
            layers.len(),
            bytes as f64 / (1 << 20) as f64
        );
        Ok(Some(Self { ids, group, pass, fail }))
    }

    /// Every layer's mips written from its base texture: mip k of the layer holds level k of the texture (the last level for the
    /// mips past its own) at the origin, texel for texel, and past the texture's right and bottom edges, and (the sampler repeats)
    /// at the layer's last columns and rows, the texels that axis's address mode reads there: the texture's own first or last
    /// ones. Drawn with the source's texels loaded unfiltered, so every texel is exact.
    fn fill(device: &wgpu::Device, queue: &wgpu::Queue, array: &wgpu::Texture, layers: &[(wgpu::TextureView, [bool; 2])], full: (u32, u32), mips: u32) {
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct Pass {
            // The source level's width and height, and 1 for each axis that clamps.
            a: [u32; 4],
            // The layer mip's width and height, and the width of its left and top borders.
            b: [u32; 4],
        }
        const SHADER: &str = r#"
struct Pass { a: vec4<u32>, b: vec4<u32> };
@group(0) @binding(0) var<storage, read> passes: array<Pass>;
@group(0) @binding(1) var src: texture_2d<f32>;
struct V { @builtin(position) pos: vec4<f32>, @location(0) @interpolate(flat) job: u32 };
@vertex fn vs(@builtin(vertex_index) v: u32, @builtin(instance_index) job: u32) -> V {
    let xy = vec2<f32>(f32((v << 1u) & 2u), f32(v & 2u));
    return V(vec4<f32>(xy * 2.0 - 1.0, 0.0, 1.0), job);
}
fn wrap(l: i32, n: i32, clamped: bool) -> i32 {
    if (clamped) { return clamp(l, 0, n - 1); }
    return ((l % n) + n) % n;
}
@fragment fn fs(i: V) -> @location(0) vec4<f32> {
    let p = passes[i.job];
    var x = i32(i.pos.x);
    var y = i32(i.pos.y);
    if (x >= i32(p.b.x) - i32(p.b.z)) { x -= i32(p.b.x); }
    if (y >= i32(p.b.y) - i32(p.b.w)) { y -= i32(p.b.y); }
    return textureLoad(src, vec2<i32>(wrap(x, i32(p.a.x), p.a.z != 0u), wrap(y, i32(p.a.y), p.a.w != 0u)), 0);
}
"#;
        let level = |n: u32, k: u32| (n >> k).max(1);
        // Past the texture's edge a side of the layer's room takes up to BORDER texels, the right (first) side the odd one.
        let sides = |layer: u32, texture: u32| {
            let room = layer.saturating_sub(texture);
            (BORDER.min((room + 1) / 2), BORDER.min(room / 2))
        };
        let mut passes = Vec::new();
        let mut jobs = Vec::new();
        for (l, (base, clamp)) in layers.iter().enumerate() {
            let texture = base.texture();
            let (size, n) = (texture.size(), texture.mip_level_count());
            for k in 0..mips {
                let s = k.min(n - 1);
                let (sw, sh) = (level(size.width, s), level(size.height, s));
                let (wk, hk) = (level(full.0, k), level(full.1, k));
                let ((rx, lx), (ry, ly)) = (sides(wk, sw), sides(hk, sh));
                let range = |size: u32, source: u32, right: u32, left: u32| {
                    let mut r = vec![(0, (source + right).min(size))];
                    if left > 0 {
                        r.push((size - left, size));
                    }
                    r
                };
                jobs.push((l as u32, k, s, passes.len() as u32, texture.clone(), range(wk, sw, rx, lx), range(hk, sh, ry, ly)));
                passes.push(Pass { a: [sw, sh, clamp[0] as u32, clamp[1] as u32], b: [wk, hk, lx, ly] });
            }
        }
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("vegetation layer fill"), contents: bytemuck::cast_slice(&passes), usage: wgpu::BufferUsages::STORAGE });
        let group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vegetation layer fill"),
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
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("vegetation layer fill"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("vegetation layer fill"), bind_group_layouts: &[Some(&group_layout)], immediate_size: 0 });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("vegetation layer fill"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("vegetation layer fill") });
        for (layer, k, s, pass, texture, xs, ys) in jobs {
            let source = texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2), base_mip_level: s, mip_level_count: Some(1), ..Default::default() });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("vegetation layer fill"),
                layout: &group_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&source) },
                ],
            });
            let target = array.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_mip_level: k,
                mip_level_count: Some(1),
                base_array_layer: layer,
                array_layer_count: Some(1),
                ..Default::default()
            });
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("vegetation layer fill"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&pipeline);
            rp.set_bind_group(0, &group, &[]);
            for &(y0, y1) in &ys {
                for &(x0, x1) in &xs {
                    if x1 > x0 && y1 > y0 {
                        rp.set_scissor_rect(x0, y0, x1 - x0, y1 - y0);
                        rp.draw(0..3, pass..pass + 1);
                    }
                }
            }
        }
        queue.submit([enc.finish()]);
    }

    /// Each chunk's vertices' material and colour words (`objects::Shrubs::build`: per vertex its strip's texture setting and its
    /// colour's place in the fog table), the batch's second vertex stream.
    pub fn vertex_materials(&self, device: &wgpu::Device, vertices: &[Option<Vec<(Key, u32)>>]) -> Vec<Option<wgpu::Buffer>> {
        vertices
            .iter()
            .map(|vm| {
                vm.as_ref().map(|vm| {
                    let ids: Vec<[u32; 2]> = vm.iter().map(|(key, slot)| [self.ids[key], *slot]).collect();
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("vegetation vertex materials"),
                        contents: bytemuck::cast_slice(&ids),
                        usage: wgpu::BufferUsages::VERTEX,
                    })
                })
            })
            .collect()
    }

    pub fn draw<'a, E: crate::replay::Enc<'a>>(
        &'a self,
        pass: &mut E,
        keys: &crate::objects::Materials,
        vertices: &'a [Option<wgpu::Buffer>],
        indices: &'a [Option<wgpu::Buffer>],
        materials: &'a [Option<wgpu::Buffer>],
        draws: &[(usize, Draw)],
    ) -> Result<(), String> {
        // Keep the caller's passing/failing split and every visible triangle's order. Hidden instances inside a span are
        // clipped on the GPU; material selection no longer needs a CPU bind/draw between neighbouring strips. The failing
        // pixels' pipeline draws only the runs of draws whose material can fail its alpha test (`Materials::keys`' flag, as the
        // material-by-material path takes it): over a material that never fails it draws nothing, and spanning those runs too
        // ran every shrub's vertices a second time (1.2 million in Veldin, 49 MB more read a frame). A span also ends where more
        // than `SPAN_GAP` hidden corners lie before the next run: their vertices are read and clipped for nothing (at Veldin's
        // first checkpoint 190,000 corners drawn spanned 600,000, 400,000 of them in some 40 gaps of over 1,024; a weak GPU pays
        // for each vertex read from the memory it shares, more than for a draw).
        pass.set_bind_group(1, &self.group);
        for fail in [false, true] {
            pass.set_pipeline(if fail { &self.fail } else { &self.pass });
            let mut i = 0;
            while i < draws.len() {
                let chunk = draws[i].0;
                let (mut end, mut span): (u32, Option<(u32, u32)>) = (draws[i].1.first, None);
                let mut bound = false;
                while i < draws.len() && draws[i].0 == chunk {
                    let dr = &draws[i].1;
                    if dr.first < end {
                        return Err("native: vegetation draws are not in vertex order".into());
                    }
                    end = dr.first + dr.count;
                    i += 1;
                    let fails = keys.keys.get(&dr.key).ok_or("native: a shrub texture setting made at load is missing")?.1;
                    if fail && !fails {
                        if let Some((first, last)) = span.take() {
                            self.span(pass, &mut bound, vertices, indices, materials, chunk, first..last)?;
                        }
                        continue;
                    }
                    if let Some((first, last)) = span.filter(|s| dr.first - s.1 > SPAN_GAP) {
                        self.span(pass, &mut bound, vertices, indices, materials, chunk, first..last)?;
                        span = None;
                    }
                    span = Some((span.map_or(dr.first, |s| s.0), end));
                }
                if let Some((first, last)) = span {
                    self.span(pass, &mut bound, vertices, indices, materials, chunk, first..last)?;
                }
            }
        }
        Ok(())
    }

    /// One bundle's draws with the vegetation's cull (`veg_cull`): its lists of the passing pixels, then of the failing pixels
    /// (`lists`: whether failing, the list and its chunk), in the order `draw` draws their spans, each list's corners those of
    /// its pieces the cull keeps, packed in their order, drawn with the count the GPU wrote.
    pub fn draw_culled<'a, E: crate::replay::Enc<'a>>(&'a self, pass: &mut E, cull: &'a crate::veg_cull::FrameCull, vertices: &'a [Option<wgpu::Buffer>], materials: &'a [Option<wgpu::Buffer>], lists: &[(bool, usize, usize)]) -> Result<(), String> {
        pass.set_bind_group(1, &self.group);
        pass.set_index_buffer(&cull.packed);
        for fail in [false, true] {
            pass.set_pipeline(if fail { &self.fail } else { &self.pass });
            for &(_, list, chunk) in lists.iter().filter(|l| l.0 == fail) {
                pass.set_vertex_buffer(0, vertices[chunk].as_ref().ok_or("native: missing vegetation vertices")?);
                pass.set_vertex_buffer(1, materials[chunk].as_ref().ok_or("native: missing vegetation material indices")?);
                pass.draw_indexed_indirect(&cull.state, cull.args_offset(list));
            }
        }
        Ok(())
    }

    /// One span of a chunk's vertices drawn, the chunk's vertex and material streams bound first (`bound`) where not yet.
    fn span<'a, E: crate::replay::Enc<'a>>(&self, pass: &mut E, bound: &mut bool, vertices: &'a [Option<wgpu::Buffer>], indices: &'a [Option<wgpu::Buffer>], materials: &'a [Option<wgpu::Buffer>], chunk: usize, range: std::ops::Range<u32>) -> Result<(), String> {
        if !*bound {
            pass.set_vertex_buffer(0, vertices[chunk].as_ref().ok_or("native: missing vegetation vertices")?);
            pass.set_vertex_buffer(1, materials[chunk].as_ref().ok_or("native: missing vegetation material indices")?);
            pass.set_index_buffer(indices[chunk].as_ref().ok_or("native: missing vegetation corners")?);
            *bound = true;
        }
        pass.draw_indexed(range);
        Ok(())
    }
}

/// The batch's shader, derived from the common one (the pixel authority; only its resource references change): its base textures
/// in one texture array (`layered`) or, with binding arrays, every slot indexed through the material table.
fn shader(layered: Option<u32>) -> String {
    rewrite(layered, shrub_vertex)
}

/// The vegetation's vertex stage for the batch: each vertex's material and colour slot as a second stream.
fn shrub_vertex(mut s: String) -> String {
    s = s.replace(
        "@location(3) instance: u32 };",
        "@location(3) instance: u32, @location(4) material: u32, @location(5) colour_slot: u32 };",
    );
    s.replace("o.f = bitcast<f32>(shrub_f[v.instance]);", "o.f = bitcast<f32>(shrub_f[v.instance]);\n    o.colour = unpack4x8unorm(shrub_f[v.colour_slot]);\n    o.material = v.material;\n    if (o.f < 0.0) { o.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0); }")
}

/// The batch's shader with the vertex stage's changes `vertex` makes (the pixel stage and the tables are the same for every
/// batch: the ties', `tie_batch.rs`, take the vegetation's rewrite and add their own vertex stage).
pub(crate) fn rewrite(layered: Option<u32>, vertex: impl FnOnce(String) -> String) -> String {
    let mut s = format!(
        "{}{}{}{}{}",
        if layered.is_some() { "" } else { "enable wgpu_binding_array;\n" },
        crate::objects::SHADER,
        crate::lighting::MATERIAL, crate::surface_buffer::COMMON, crate::surface_buffer::OBJECTS
    );
    // The material table's own bindings take the place of the per-material ones (the layered form keeps bindings 3 to 9, the
    // Rift Apart maps, as they are).
    let own = |l: &str| if layered.is_some() { ["@group(1) @binding(0)", "@group(1) @binding(1)", "@group(1) @binding(2)"].iter().any(|b| l.starts_with(b)) } else { l.starts_with("@group(1)") };
    s = s.lines().filter(|l| !own(l)).collect::<Vec<_>>().join("\n");
    s = s
        .replace("@fragment fn fs_surface", "fn fs_surface")
        .replace("@fragment fn fs_pass", "fn fs_pass")
        .replace("@fragment fn fs_fail", "fn fs_fail");
    s = s
        .replace(
            "    var t: vec4<f32>;",
            "    let tex_dx = dpdx(uv);\n    let tex_dy = dpdy(uv);\n    var t: vec4<f32>;",
        )
        .replace(
            "textureSampleGrad(tex, smp, uv, dpdx(uv) * view.lerp.z, dpdy(uv) * view.lerp.z)",
            "textureSampleGrad(tex, smp, uv, tex_dx * view.lerp.z, tex_dy * view.lerp.z)",
        );
    s = s.replace("@location(8) @interpolate(flat) shell: f32,", "@location(8) @interpolate(flat) shell: f32,\n    @location(9) @interpolate(flat) material: u32,");
    s = vertex(s);
    if let Some(top) = layered {
        // The base texture read through its layer: the coordinate wrapped by the material's own modes into the layer's share, the
        // gradients (taken from the unwrapped coordinate, so that a seam picks the right level) scaled to it.
        s = s
            .replace("textureSampleGrad(tex, smp, ", "array_grad(")
            .replace("textureSampleLevel(tex, smp, ", "array_level(");
        s.push_str(&format!("\nconst TOP_LEVEL: f32 = {top}.0;"));
        s.push_str(
            r#"
struct BatchMat { value: Mat, geo: vec4<f32>, lay: vec4<u32> };
@group(1) @binding(0) var<storage, read> batch_materials: array<BatchMat>;
@group(1) @binding(1) var tex: texture_2d_array<f32>;
@group(1) @binding(2) var smp: sampler;
var<private> mat: Mat;
var<private> geo: vec4<f32>;
var<private> lay: vec4<u32>;
fn select_material(id: u32) {
    let b = batch_materials[id];
    mat = b.value;
    geo = b.geo;
    lay = b.lay;
}
fn array_uv(uv: vec2<f32>) -> vec2<f32> {
    let held = select(fract(uv), clamp(uv, geo.zw, vec2<f32>(1.0) - geo.zw), vec2<bool>(lay.y != 0u, lay.z != 0u));
    return held * geo.xy;
}
// A texture with fewer levels than the sampler's last (TOP_LEVEL): the level its footprint picks (the sampler's own rule: the
// major axis over the anisotropy ratio) kept to the texture's last, as a texture of its own levels clamps it; the layer's mips
// past them are not the texture's.
fn array_grad(uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    let size = 0.5 / geo.zw;
    let px = length(dx * size);
    let py = length(dy * size);
    let major = max(px, py);
    let ratio = min(ceil(major / max(min(px, py), 1e-30)), f32(lay.w));
    let kept = select(1.0, exp2(min(mat.last - log2(max(major / ratio, 1e-30)), 0.0)), mat.last < TOP_LEVEL);
    return textureSampleGrad(tex, smp, array_uv(uv), lay.x, dx * geo.xy * kept, dy * geo.xy * kept);
}
fn array_level(uv: vec2<f32>, lod: f32) -> vec4<f32> {
    return textureSampleLevel(tex, smp, array_uv(uv), lay.x, lod);
}
@fragment fn batch_surface(i: VOut) -> SurfaceOut { select_material(i.material); return fs_surface(i); }
@fragment fn batch_pass(i: VOut) -> FOut { select_material(i.material); return fs_pass(i); }
@fragment fn batch_fail(i: VOut) -> FOut { select_material(i.material); return fs_fail(i); }
"#,
        );
        return s;
    }
    s.push_str(
        r#"
struct BatchMat { value: Mat, slots0: vec4<u32>, slots1: vec4<u32>, slots2: vec4<u32> };
@group(1) @binding(0) var<storage, read> batch_materials: array<BatchMat>;
@group(1) @binding(1) var batch_textures: binding_array<texture_2d<f32>>;
@group(1) @binding(2) var batch_samplers: binding_array<sampler>;
var<private> mat: Mat;
var<private> slots0: vec4<u32>;
var<private> slots1: vec4<u32>;
var<private> slots2: vec4<u32>;
fn select_material(id: u32) {
    let b = batch_materials[id];
    mat = b.value;
    slots0 = b.slots0;
    slots1 = b.slots1;
    slots2 = b.slots2;
}
@fragment fn batch_surface(i: VOut) -> SurfaceOut { select_material(i.material); return fs_surface(i); }
@fragment fn batch_pass(i: VOut) -> FOut { select_material(i.material); return fs_pass(i); }
@fragment fn batch_fail(i: VOut) -> FOut { select_material(i.material); return fs_fail(i); }
"#,
    );
    // The common shader remains the pixel authority. Only its resource references become GPU-selected table entries.
    for (name, slot) in [
        ("tex", "slots0.x"),
        ("mtl", "slots0.y"),
        ("rgn", "slots0.z"),
        ("mtl1", "slots0.w"),
        ("mtl2", "slots1.x"),
        ("mtl3", "slots1.y"),
        ("rlf", "slots1.z"),
    ] {
        for function in [
            "textureSampleGrad",
            "textureSampleLevel",
            "textureSample",
            "textureLoad",
            "textureDimensions",
        ] {
            s = s.replace(
                &format!("{function}({name},"),
                &format!("{function}(batch_textures[{slot}],"),
            );
            s = s.replace(
                &format!("{function}({name})"),
                &format!("{function}(batch_textures[{slot}])"),
            );
        }
    }
    s.replace(", smp,", ", batch_samplers[slots1.w],")
        .replace(", msmp,", ", batch_samplers[slots2.x],")
}

impl Drop for Batch {
    fn drop(&mut self) { crate::surface_buffer::unregister(&self.pass); }
}
