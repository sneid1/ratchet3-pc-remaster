//! The sky in the port's renderer (PLAN item 1.1): the sky the game's pointer names, read from RAM as its loader left it
//! (`uyafmt::coremisc::sky_in_ram`), drawn as func_L01_004A46C8 draws it each frame, before the world: every mesh in order,
//! around the eye (the camera's rotation and projection without its translation, the matrix at 0x1E2A00), each turned by its
//! rotation (func_L01_004A45B0: Rz Ry Rx of its angles, applied to a vertex as a row vector) at the angles the game has advanced
//! it to (read each frame from the sky in RAM).
//!
//! The GS state is the game's packet templates': a coloured mesh (flags bit 0) is Gouraud with ALPHA_1 0x44 ((Cs - Cd) x As / 0x80
//! + Cd) and Z test ALWAYS (template 0x1422E0); a textured mesh is TEX0 MODULATE RGBA with R, G, B 0x80 and the vertex's alpha
//! (func_L01_004A4F38), CLAMP 5 and TEX1 0x260 (bilinear, one level), ALPHA_1 0x44 (template 0x142350) or 0xA8, the colour
//! replacing the frame's (flags bit 1, template 0x1423C0), no Z write; its alpha test (GEQUAL 8, failing pixels keep their colour
//! write) changes nothing on screen. Colours and texture coordinates (ST with Q 1.0) are interpolated linearly across the screen.

use crate::native::Vertex;

const SHADER: &str = r#"
enable dual_source_blending;
struct View { m: mat4x4<f32>, sky: mat4x4<f32>, eye: vec4<f32>, forward: vec4<f32>, fog_colour: vec4<f32>, fog: vec4<f32> };
struct Mesh { r0: vec4<f32>, r1: vec4<f32>, r2: vec4<f32> };
@group(0) @binding(0) var<uniform> view: View;
@group(1) @binding(0) var<uniform> mesh: Mesh;
@group(2) @binding(0) var tex: texture_2d<f32>;
@group(2) @binding(1) var smp: sampler;

struct VIn { @location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) colour: vec4<f32> };
struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(linear) uv: vec2<f32>,
    @location(1) @interpolate(linear) colour: vec4<f32>,
};

@vertex fn vs(v: VIn) -> VOut {
    var o: VOut;
    let d = v.pos.x * mesh.r0.xyz + v.pos.y * mesh.r1.xyz + v.pos.z * mesh.r2.xyz;
    var c = view.sky * vec4<f32>(d, 1.0);
    // Behind everything the world draws (the world's depth test passes over it; the sky writes no depth here).
    c.z = c.w * 0.999;
    o.clip = c;
    o.uv = v.uv;
    o.colour = v.colour;
    return o;
}

// The pixel as the GS writes it: its colour and its alpha As exactly (a byte, as the frame buffer keeps it: the glow reads
// it back), and for the blend unit As / 0x80 as the second source's alpha (`Src1Alpha`; up to 1).
struct FOut { @location(0) @blend_src(0) c: vec4<f32>, @location(0) @blend_src(1) f: vec4<f32> };
// ALPHA_1 0x44.
fn blended(p: vec4<f32>) -> FOut {
    var o: FOut;
    o.c = vec4<f32>(p.rgb / 255.0, p.a / 255.0);
    o.f = vec4<f32>(0.0, 0.0, 0.0, min(p.a / 128.0, 1.0));
    return o;
}

@fragment fn fs_colour(i: VOut) -> FOut {
    return blended(round(i.colour * 255.0));
}

@fragment fn fs_texture(i: VOut) -> FOut {
    // One level (the GS samples level 0); a pack replacement's own levels by their texel density on screen.
    // Truncated, as the GS's filter leaves the filtered texel.
    let t = floor(textureSample(tex, smp, i.uv) * 255.0 + 0.001);
    let v = round(i.colour * 255.0);
    return blended(min(floor(t * v / 128.0), vec4<f32>(255.0)));
}
"#;

/// One run of a mesh's triangles under one texture (None: a coloured mesh): vertices `first .. first + count` of the sky's
/// vertex buffer.
struct Run {
    texture: Option<u32>,
    first: u32,
    count: u32,
}

struct Mesh {
    flags: u16,
    runs: Vec<Run>,
    /// Its rotation, one uniform and group per frame set (`native::SETS`): a frame drawn again keeps its own.
    sets: Vec<(wgpu::Buffer, wgpu::BindGroup)>,
}

/// The sky's pipelines and layouts, made once when the renderer opens (`native::Native::open`, `pipelines.rs`): a level's sky
/// makes none.
pub struct SkyPipelines {
    mesh_layout: wgpu::BindGroupLayout,
    tex_layout: wgpu::BindGroupLayout,
    colour: wgpu::RenderPipeline,
    blend: wgpu::RenderPipeline,
    replace: wgpu::RenderPipeline,
}

impl SkyPipelines {
    pub fn new(device: &wgpu::Device, view_layout: &wgpu::BindGroupLayout) -> SkyPipelines {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("sky"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let mesh_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky mesh"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Unorm8x4];
        let layout_c = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("sky colour"), bind_group_layouts: &[Some(view_layout), Some(&mesh_layout)], immediate_size: 0 });
        let layout_t = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("sky texture"), bind_group_layouts: &[Some(view_layout), Some(&mesh_layout), Some(&tex_layout)], immediate_size: 0 });
        let alpha = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Src1Alpha, dst_factor: wgpu::BlendFactor::OneMinusSrc1Alpha, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        let pipeline = |layout: &wgpu::PipelineLayout, entry: &str, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Vertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs })],
                },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: crate::native::DEPTH,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: crate::pipelines::cache(),
            })
        };
        let colour = pipeline(&layout_c, "fs_colour", Some(alpha));
        let blend = pipeline(&layout_t, "fs_texture", Some(alpha));
        let replace = pipeline(&layout_t, "fs_texture", Some(wgpu::BlendState::REPLACE));
        SkyPipelines { mesh_layout, tex_layout, colour, blend, replace }
    }
}

/// A level's sky on the GPU.
pub struct SkyGpu {
    vertices: wgpu::Buffer,
    meshes: Vec<Mesh>,
    textures: Vec<wgpu::BindGroup>,
    colour: wgpu::RenderPipeline,
    blend: wgpu::RenderPipeline,
    replace: wgpu::RenderPipeline,
}

/// The rotation func_0040C678 builds from the angles (2 pi / 65536 units): Rz Ry Rx, rows.
fn rotation(a: [i16; 3]) -> [[f32; 4]; 3] {
    let k = std::f32::consts::TAU / 65536.0;
    let (sx, cx) = (a[0] as f32 * k).sin_cos();
    let (sy, cy) = (a[1] as f32 * k).sin_cos();
    let (sz, cz) = (a[2] as f32 * k).sin_cos();
    let mul = |m: [[f32; 3]; 3], n: [[f32; 3]; 3]| -> [[f32; 3]; 3] {
        let mut o = [[0f32; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                o[i][j] = (0..3).map(|k| m[i][k] * n[k][j]).sum();
            }
        }
        o
    };
    let rz = [[cz, sz, 0.0], [-sz, cz, 0.0], [0.0, 0.0, 1.0]];
    let ry = [[cy, 0.0, -sy], [0.0, 1.0, 0.0], [sy, 0.0, cy]];
    let rx = [[1.0, 0.0, 0.0], [0.0, cx, sx], [0.0, -sx, cx]];
    let r = mul(mul(rz, ry), rx);
    r.map(|row| [row[0], row[1], row[2], 0.0])
}

impl SkyGpu {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, pipes: &SkyPipelines, sky: &uyafmt::coremisc::Sky) -> Result<SkyGpu, String> {
        use wgpu::util::DeviceExt;
        let mut vertices: Vec<Vertex> = Vec::new();
        let (mesh_layout, tex_layout) = (&pipes.mesh_layout, &pipes.tex_layout);
        let mut meshes = Vec::with_capacity(sky.meshes.len());
        for (mi, m) in sky.meshes.iter().enumerate() {
            let coloured = m.flags & 1 != 0;
            let mut runs: Vec<Run> = Vec::new();
            for part in &m.parts {
                for t in &part.triangles {
                    let texture = if coloured { None } else { Some(t[3] as u32) };
                    if let Some(tx) = texture {
                        if tx as usize >= sky.textures.len() {
                            return Err(format!("native: sky mesh {mi}: texture {tx} of {}", sky.textures.len()));
                        }
                    }
                    if runs.last().is_none_or(|r| r.texture != texture) {
                        runs.push(Run { texture, first: vertices.len() as u32, count: 0 });
                    }
                    for &i in &t[..3] {
                        let (v, w) = (part.vertices[i as usize], part.words[i as usize]);
                        let pos = [v[0] as f32, v[1] as f32, v[2] as f32];
                        let (uv, colour) = if coloured {
                            ([0.0, 0.0], w.to_le_bytes())
                        } else {
                            // Two s16 texture coordinates in 1/4096 (vitof12); R, G, B 0x80 and the vertex's alpha byte.
                            ([(w as u16 as i16) as f32 / 4096.0, ((w >> 16) as u16 as i16) as f32 / 4096.0], [0x80, 0x80, 0x80, v[3] as u8])
                        };
                        vertices.push(Vertex { pos, uv, colour });
                    }
                    runs.last_mut().expect("pushed above").count += 3;
                }
            }
            let sets = (0..crate::native::SETS)
                .map(|_| {
                    let uniform = device.create_buffer(&wgpu::BufferDescriptor { label: Some("sky mesh"), size: 48, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
                    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("sky mesh"), layout: mesh_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }] });
                    (uniform, bind)
                })
                .collect();
            meshes.push(Mesh { flags: m.flags, runs, sets });
        }
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sky"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let hd_sampler = crate::texnames::hd_sampler(device, true, true, false, 16);
        let mut textures = Vec::with_capacity(sky.textures.len());
        for t in &sky.textures {
            // A pack replacement (`texnames`) in its place, trilinear and anisotropic.
            if let Some((view, _)) = crate::texnames::hd_view(device, queue, t)? {
                textures.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("sky pack texture"),
                    layout: tex_layout,
                    entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&hd_sampler) }],
                }));
                continue;
            }
            let size = wgpu::Extent3d { width: t.width as u32, height: t.height as u32, depth_or_array_layers: 1 };
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("sky texture"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                bytemuck::cast_slice(&t.levels[0]),
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * t.width as u32), rows_per_image: Some(t.height as u32) },
                size,
            );
            let view = tex.create_view(&Default::default());
            textures.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sky texture"),
                layout: tex_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) }],
            }));
        }
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("sky vertices"), contents: bytemuck::cast_slice(&vertices), usage: wgpu::BufferUsages::VERTEX });
        Ok(SkyGpu { vertices, meshes, textures, colour: pipes.colour.clone(), blend: pipes.blend.clone(), replace: pipes.replace.clone() })
    }

    /// Sets each mesh's rotation from the angles the game has advanced it to: the sky at `sky` in `ram` it was made from
    /// (`uyafmt::coremisc::sky_in_ram`: mesh addresses at +0x20, each mesh's angles at +4, +6, +8).
    /// Into frame set `set`'s uniforms (`draw` with the same set draws them).
    pub fn update(&self, queue: &wgpu::Queue, ram: &[u8], sky: u32, set: usize) -> Result<(), String> {
        let u32_at = |a: usize| -> Result<u32, String> { ram.get(a..a + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).ok_or_else(|| format!("native: RAM {a:#x}")) };
        let sky = sky as usize;
        for (i, m) in self.meshes.iter().enumerate() {
            let h = u32_at(sky + 0x20 + 4 * i)? as usize;
            let a = |o: usize| i16::from_le_bytes([ram[h + o], ram[h + o + 1]]);
            let r = rotation([a(4), a(6), a(8)]);
            queue.write_buffer(&m.sets[set].0, 0, bytemuck::cast_slice(&r));
        }
        Ok(())
    }

    /// Draws the sky into `pass`, whose group 0 is the frame's view.
    pub(crate) fn draw<'a, E: crate::replay::Enc<'a>>(&'a self, pass: &mut E, set: usize) {
        // A sky of no meshes (`uyafmt::coremisc::sky_in_ram`: the game draws none) draws nothing; its vertex buffer is empty.
        if self.meshes.is_empty() {
            return;
        }
        pass.set_vertex_buffer(0, &self.vertices);
        for m in &self.meshes {
            pass.set_bind_group(1, &m.sets[set].1);
            for r in &m.runs {
                match r.texture {
                    None => pass.set_pipeline(&self.colour),
                    Some(t) => {
                        pass.set_pipeline(if m.flags & 2 != 0 { &self.replace } else { &self.blend });
                        pass.set_bind_group(2, &self.textures[t as usize]);
                    }
                }
                pass.draw(r.first..r.first + r.count);
            }
        }
    }
}
