"""Install real-level probe hooks into the isolated renderer snapshot."""
from pathlib import Path
import shutil
HERE=Path(__file__).resolve().parent;SRC=HERE.parents[1]/'captures/rift-texture-review/viewer-build/src'
def once(text,old,new):
    if new in text:return text
    if text.count(old)!=1:raise ValueError('Probe renderer hook changed: '+old[:65])
    return text.replace(old,new,1)
def apply():
    shutil.copy2(HERE/'review_probe.rs',SRC/'review_probe.rs')
    path=SRC/'lib.rs';s=path.read_text()
    if 'mod review_probe;' not in s:path.write_text(s+'\nmod review_probe;\n')
    path=SRC/'native.rs';s=path.read_text()
    s=once(s,'    sky_pipes: crate::sky::SkyPipelines,','    review_probe: crate::review_probe::Probe,\n    sky_pipes: crate::sky::SkyPipelines,')
    s=once(s,'let cam = self.camera(ram, level)?;','let cam = crate::review_probe::camera().unwrap_or(self.camera(ram, level)?);')
    s=once(s,'entries: &[uniform(0, wgpu::ShaderStages::VERTEX_FRAGMENT)]','''entries: &[uniform(0, wgpu::ShaderStages::VERTEX_FRAGMENT),
        wgpu::BindGroupLayoutEntry{binding:1,visibility:wgpu::ShaderStages::FRAGMENT,ty:wgpu::BindingType::Texture{sample_type:wgpu::TextureSampleType::Float{filterable:true},view_dimension:wgpu::TextureViewDimension::Cube,multisampled:false},count:None},
        wgpu::BindGroupLayoutEntry{binding:2,visibility:wgpu::ShaderStages::FRAGMENT,ty:wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),count:None}]''')
    s=once(s,'Ok(Native { space_scene: false,','Ok(Native { review_probe: crate::review_probe::Probe::open(&device,&queue)?, space_scene: false,')
    s=once(s,'entries: &[wgpu::BindGroupEntry { binding: 0, resource: view.as_entire_binding() }]','''entries: &[wgpu::BindGroupEntry { binding: 0, resource: view.as_entire_binding() },
        wgpu::BindGroupEntry{binding:1,resource:wgpu::BindingResource::TextureView(&self.review_probe.view)},
        wgpu::BindGroupEntry{binding:2,resource:wgpu::BindingResource::Sampler(&self.review_probe.sampler)}]''')
    old='self.queue.submit(std::iter::once(clear.finish()).chain(passes).chain(std::iter::once(main_cb)));'
    new='''if crate::review_probe::capturing() {
            // Static source passes only: no captured actors, UI or effects.
            self.queue.submit(std::iter::once(clear.finish()).chain(passes));
            return Ok(pic.tex.clone());
        }
        '''+old
    s=once(s,old,new);path.write_text(s)
    path=SRC/'lighting.rs';s=path.read_text()
    s=once(s,'const MATERIAL_PI: f32 = 3.14159265;','''@group(0) @binding(1) var review_environment: texture_cube<f32>;
@group(0) @binding(2) var review_environment_sampler: sampler;
const MATERIAL_PI: f32 = 3.14159265;''')
    old='let indirect=max(ambient,vec3<f32>(0.0))*((1.0-f0)*(1.0-m.metal)*base+f0*(1.0-0.5*rough))*m.ao;'
    new='''let env=textureSampleLevel(review_environment,review_environment_sampler,reflect(-e,n),rough*f32(textureNumLevels(review_environment)-1u));
    let fres=f0+(max(vec3<f32>(1.0-rough),f0)-f0)*pow(1.0-max(dot(n,e),0.0),5.0);
    let reflected=select(max(ambient,vec3<f32>(0.0))*f0*(1.0-0.5*rough),env.rgb*fres,env.a>0.5);
    let indirect=max(ambient,vec3<f32>(0.0))*(1.0-f0)*(1.0-m.metal)*base*m.ao+reflected*mix(1.0,m.ao,rough);'''
    s=once(s,old,new);path.write_text(s)
if __name__=='__main__':apply()
