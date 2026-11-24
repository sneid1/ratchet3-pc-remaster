//! Isolated renderer glow from the explicit GS emission-alpha contract.
//! Read native pixels before AO to keep self-illumination out of cavity shading.
const SHADER:&str=r#"
@group(0) @binding(0) var before_ao:texture_2d<f32>;
@group(0) @binding(1) var after_ao:texture_2d<f32>;
@vertex fn vs(@builtin(vertex_index) i:u32)->@builtin(position) vec4<f32>{
    let p=vec2<f32>(f32((i<<1u)&2u),f32(i&2u));return vec4<f32>(p*2.0-1.0,0.0,1.0);
}
fn linear(c:vec3<f32>)->vec3<f32>{return select(c/12.92,pow((c+0.055)/1.055,vec3<f32>(2.4)),c>vec3<f32>(0.04045));}
fn srgb(c:vec3<f32>)->vec3<f32>{let v=max(c,vec3<f32>(0.0));return select(v*12.92,1.055*pow(v,vec3<f32>(1.0/2.4))-0.055,v>vec3<f32>(0.0031308));}
fn mask(p:vec4<f32>)->f32{return clamp((p.a*255.0-128.0)/127.0,0.0,1.0);}
fn load(p:vec2<f32>)->vec4<f32>{return textureLoad(before_ao,clamp(vec2<i32>(p),vec2<i32>(0),vec2<i32>(textureDimensions(before_ao))-vec2<i32>(1)),0);}
fn emission(p:vec2<f32>)->vec3<f32>{let v=load(p);return linear(v.rgb)*mask(v);}
@fragment fn fs(@builtin(position) pos:vec4<f32>)->@location(0) vec4<f32>{
    let old=load(pos.xy);let shaded=textureLoad(after_ao,vec2<i32>(pos.xy),0);
    let base=mix(linear(shaded.rgb),linear(old.rgb),clamp(mask(old)*12.0,0.0,1.0));
    let directions=array<vec2<f32>,8>(vec2<f32>(1.0,0.0),vec2<f32>(0.707,0.707),vec2<f32>(0.0,1.0),vec2<f32>(-0.707,0.707),vec2<f32>(-1.0,0.0),vec2<f32>(-0.707,-0.707),vec2<f32>(0.0,-1.0),vec2<f32>(0.707,-0.707));
    let scale=f32(textureDimensions(before_ao).y)/900.0;
    var halo=vec3<f32>(0.0);
    for(var i=0u;i<8u;i++){
        let d=directions[i]*scale;
        halo+=(emission(pos.xy+d*2.0)*0.5+emission(pos.xy+d*5.0)*0.35+emission(pos.xy+d*10.0)*0.15)/8.0;
    }
    return vec4<f32>(min(srgb(base+halo*0.27),vec3<f32>(1.0)),shaded.a);
}
"#;

pub(crate) fn copy_frame(device:&wgpu::Device,enc:&mut wgpu::CommandEncoder,source:&wgpu::Texture,size:(u32,u32))->wgpu::Texture{
    let extent=wgpu::Extent3d{width:size.0,height:size.1,depth_or_array_layers:1};
    let copy=device.create_texture(&wgpu::TextureDescriptor{label:Some("review emission source"),size:extent,mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Rgba8Unorm,usage:wgpu::TextureUsages::TEXTURE_BINDING|wgpu::TextureUsages::COPY_DST,view_formats:&[]});
    enc.copy_texture_to_texture(wgpu::TexelCopyTextureInfo{texture:source,mip_level:0,origin:wgpu::Origin3d::ZERO,aspect:wgpu::TextureAspect::All},wgpu::TexelCopyTextureInfo{texture:&copy,mip_level:0,origin:wgpu::Origin3d::ZERO,aspect:wgpu::TextureAspect::All},extent);
    copy
}
pub(crate) fn glow(device:&wgpu::Device,enc:&mut wgpu::CommandEncoder,target:&wgpu::Texture,tv:&wgpu::TextureView,source:&wgpu::Texture,size:(u32,u32)){
    let shaded=copy_frame(device,enc,target,size);
    let a=source.create_view(&Default::default());let b=shaded.create_view(&Default::default());
    let layout=device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor{label:Some("review emission"),entries:&[0,1].map(|binding|wgpu::BindGroupLayoutEntry{binding,visibility:wgpu::ShaderStages::FRAGMENT,ty:wgpu::BindingType::Texture{sample_type:wgpu::TextureSampleType::Float{filterable:false},view_dimension:wgpu::TextureViewDimension::D2,multisampled:false},count:None})});
    let group=device.create_bind_group(&wgpu::BindGroupDescriptor{label:Some("review emission"),layout:&layout,entries:&[wgpu::BindGroupEntry{binding:0,resource:wgpu::BindingResource::TextureView(&a)},wgpu::BindGroupEntry{binding:1,resource:wgpu::BindingResource::TextureView(&b)}]});
    let pl=device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor{label:Some("review emission"),bind_group_layouts:&[Some(&layout)],immediate_size:0});
    let shader=device.create_shader_module(wgpu::ShaderModuleDescriptor{label:Some("review emission"),source:wgpu::ShaderSource::Wgsl(SHADER.into())});
    let pipeline=device.create_render_pipeline(&wgpu::RenderPipelineDescriptor{label:Some("review emission"),layout:Some(&pl),vertex:wgpu::VertexState{module:&shader,entry_point:Some("vs"),compilation_options:Default::default(),buffers:&[]},fragment:Some(wgpu::FragmentState{module:&shader,entry_point:Some("fs"),compilation_options:Default::default(),targets:&[Some(wgpu::ColorTargetState{format:wgpu::TextureFormat::Rgba8Unorm,blend:None,write_mask:wgpu::ColorWrites::ALL})]}),primitive:Default::default(),depth_stencil:None,multisample:Default::default(),multiview_mask:None,cache:crate::pipelines::cache()});
    let mut pass=enc.begin_render_pass(&wgpu::RenderPassDescriptor{label:Some("review emission"),color_attachments:&[Some(wgpu::RenderPassColorAttachment{view:tv,depth_slice:None,resolve_target:None,ops:wgpu::Operations{load:wgpu::LoadOp::Load,store:wgpu::StoreOp::Store}})],..Default::default()});
    pass.set_pipeline(&pipeline);pass.set_bind_group(0,&group,&[]);pass.draw(0..3,0..1);
}
