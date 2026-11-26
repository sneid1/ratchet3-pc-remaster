//! Vegetation materials selected by the GPU, in the original triangle order. A draw spans adjacent material runs, and the
//! vertex shader rejects hidden instances through their fog entry (-1). This avoids thousands of CPU material binds without
//! sorting translucent leaves. The existing per-material path serves devices without texture binding arrays.

use crate::objects::{Draw, Key, ShrubVertex};
use std::collections::HashMap;
use wgpu::util::DeviceExt;

pub(crate) struct MaterialInput {
    pub uniform: crate::native::MatU,
    pub textures: [wgpu::TextureView; 7],
    pub samplers: [wgpu::Sampler; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Material {
    value: crate::native::MatU,
    // Textures 0 to 6 (the texture, the four material maps, the region map, the relief map), samplers 7 and 8, then padding:
    // three vec4<u32> in the shader's `BatchMat`.
    slots: [u32; 12],
}

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
        let source = shader();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("GPU vegetation materials"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("GPU vegetation"),
            bind_group_layouts: &[Some(view), Some(&layout), Some(fog)],
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
        println!(
            "native: GPU vegetation: {} materials, {} shared texture views, {} samplers (this GPU's binding arrays: {} elements, {} samplers)",
            materials.len(),
            textures.len(),
            samplers.len(),
            limits.max_binding_array_elements_per_shader_stage,
            limits.max_binding_array_sampler_elements_per_shader_stage
        );
        Ok(Some(Self {
            ids,
            group,
            pass: pipeline(false),
            fail: pipeline(true),
        }))
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
        // ran every shrub's vertices a second time (1.2 million in Veldin, 49 MB more read a frame).
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
                    span = Some((span.map_or(dr.first, |s| s.0), end));
                }
                if let Some((first, last)) = span {
                    self.span(pass, &mut bound, vertices, indices, materials, chunk, first..last)?;
                }
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

fn shader() -> String {
    let mut s = format!(
        "enable wgpu_binding_array;\n{}{}{}{}",
        crate::objects::SHADER,
        crate::lighting::MATERIAL, crate::surface_buffer::COMMON, crate::surface_buffer::OBJECTS
    );
    s = s
        .lines()
        .filter(|l| !l.starts_with("@group(1)"))
        .collect::<Vec<_>>()
        .join("\n");
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
    s = s.replace(
        "@location(3) instance: u32 };",
        "@location(3) instance: u32, @location(4) material: u32, @location(5) colour_slot: u32 };",
    );
    s = s.replace("o.f = bitcast<f32>(shrub_f[v.instance]);", "o.f = bitcast<f32>(shrub_f[v.instance]);\n    o.colour = unpack4x8unorm(shrub_f[v.colour_slot]);\n    o.material = v.material;\n    if (o.f < 0.0) { o.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0); }");
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
