//! Reflection of an actual saved level's static surroundings. No studio image.
use crate::native::Camera;
use std::{path::Path,sync::Mutex};
static CAMERA:Mutex<Option<Camera>>=Mutex::new(None);
static FILE:Mutex<Option<std::path::PathBuf>>=Mutex::new(None);
pub(crate) fn set_camera(value:Option<Camera>){*CAMERA.lock().unwrap()=value;}
pub(crate) fn camera()->Option<Camera>{CAMERA.lock().unwrap().clone()}
pub(crate) fn capturing()->bool{CAMERA.lock().unwrap().is_some()}
pub(crate) fn file(value:Option<std::path::PathBuf>){*FILE.lock().unwrap()=value;}
pub(crate) struct Probe {pub view:wgpu::TextureView,pub sampler:wgpu::Sampler}
impl Probe {
    pub fn open(device:&wgpu::Device,queue:&wgpu::Queue)->Result<Self,String>{
        let path=FILE.lock().unwrap().clone();
        let bytes=match path.filter(|p|p.is_file()) {Some(p)=>std::fs::read(&p).map_err(|e|format!("{}: {e}",p.display()))?,None=>Vec::new()};
        let (width,mips)=if bytes.is_empty(){(1,1)}else{
            if bytes.len()<16 || &bytes[..8]!=b"R3PRB01\0"{return Err("Invalid level probe header".into());}
            (u32::from_le_bytes(bytes[8..12].try_into().unwrap()),u32::from_le_bytes(bytes[12..16].try_into().unwrap()))
        };
        if !width.is_power_of_two() || width>512 || mips!=width.ilog2()+1 {return Err("Invalid level probe dimensions".into());}
        let tex=device.create_texture(&wgpu::TextureDescriptor{label:Some("captured level reflection"),size:wgpu::Extent3d{width,height:width,depth_or_array_layers:6},mip_level_count:mips,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Rgba16Float,usage:wgpu::TextureUsages::TEXTURE_BINDING|wgpu::TextureUsages::COPY_DST,view_formats:&[]});
        let mut at=16;
        for mip in 0..mips {let size=width>>mip;let count=(size*size*6*8) as usize;
            let black=vec![0;count];
            let px=if bytes.is_empty(){&black[..]}else{let end=at+count;let p=bytes.get(at..end).ok_or("Incomplete level probe")?;at=end;p};
            queue.write_texture(wgpu::TexelCopyTextureInfo{texture:&tex,mip_level:mip,origin:wgpu::Origin3d::ZERO,aspect:wgpu::TextureAspect::All},px,wgpu::TexelCopyBufferLayout{offset:0,bytes_per_row:Some(size*8),rows_per_image:Some(size)},wgpu::Extent3d{width:size,height:size,depth_or_array_layers:6});
        }
        if !bytes.is_empty() && at!=bytes.len(){return Err("Unexpected trailing level probe bytes".into());}
        Ok(Self{view:tex.create_view(&wgpu::TextureViewDescriptor{dimension:Some(wgpu::TextureViewDimension::Cube),..Default::default()}),sampler:device.create_sampler(&wgpu::SamplerDescriptor{label:Some("roughness-filtered level probe"),mag_filter:wgpu::FilterMode::Linear,min_filter:wgpu::FilterMode::Linear,mipmap_filter:wgpu::MipmapFilterMode::Linear,..Default::default()})})
    }
}
pub(crate) fn capture(disc:&str,level:u32,pack:&str,rift:&str,ram_path:&Path,target:&Path,eye:[f32;3])->Result<(),String>{
    let mut settings=crate::settings::Settings::default();settings.set_pairs("lighting=per_pixel,texture_filtering=16x,rift_apart_surfaces=on")?;crate::enhance::set(&settings);
    crate::texnames::open(Path::new(pack),Some(Path::new(rift)))?;
    file(None);std::fs::create_dir_all(target).map_err(|e|e.to_string())?;
    let mut native=crate::native::Native::open(disc,256)?;
    let ram=std::fs::read(ram_path).map_err(|e|e.to_string())?;
    let suffix=ram_path.file_name().and_then(|s|s.to_str()).and_then(|s|s.strip_prefix("ram")).ok_or("Expected RAM capture filename")?;
    let jobs=crate::jobs::FrameJobs::read(&ram_path.with_file_name(format!("jobs{suffix}")).to_string_lossy())?;
    // GPU cubemap orientation. Captures are vertically flipped by the prefilter.
    let axes=[([1.,0.,0.],[0.,-1.,0.]),([-1.,0.,0.],[0.,-1.,0.]),([0.,1.,0.],[0.,0.,1.]),([0.,-1.,0.],[0.,0.,-1.]),([0.,0.,1.],[0.,-1.,0.]),([0.,0.,-1.],[0.,-1.,0.])];
    for (face,(forward,up)) in axes.into_iter().enumerate(){
        set_camera(Some(Camera{eye,forward,up,tan:[1.,1.],k:0.4805/0.62}));
        let im=native.draw(&ram,&jobs,level)?;
        if im.w!=256 || im.h!=256{return Err("Probe capture must be square".into());}
        std::fs::write(target.join(format!("face-{face}.png")),crate::pic::png(&im)).map_err(|e|e.to_string())?;
    }
    set_camera(None);
    std::fs::write(target.join("source.txt"),format!("LEVEL {level}\nSaved capture {}\nWorld position {eye:?}\nActual static terrain, sky, ties and shrubs; no actor, HUD or simulation. LDR source, not reconstructed HDR.\n",ram_path.display())).map_err(|e|e.to_string())?;
    Ok(())
}
