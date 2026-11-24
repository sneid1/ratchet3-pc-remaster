//! Blender surface bindings for the isolated material/model workbench.
//! Original captured triangles supply animation and light terms. Edited normals,
//! UVs and surface offsets come from Blender. A changed source LOD is an error.
use crate::{objects::ObjVertex,lighting::MobyLit};
use std::path::Path;

pub struct Edited {
    pub vertices:Vec<ObjVertex>, pub lighting:Vec<MobyLit>,
    pub materials:Vec<String>, pub triangle_materials:Vec<usize>,
}
fn sub(a:[f32;3],b:[f32;3])->[f32;3] { std::array::from_fn(|i|a[i]-b[i]) }
fn dot(a:[f32;3],b:[f32;3])->f32 { (0..3).map(|i|a[i]*b[i]).sum() }
fn cross(a:[f32;3],b:[f32;3])->[f32;3] { [a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]] }
fn normal(a:[f32;3])->Result<[f32;3],String> {
    let length=dot(a,a).sqrt();
    if length<1e-8 {return Err("edited mesh: source triangle collapsed in this pose".into());}
    Ok(a.map(|v|v/length))
}
struct Reader { bytes:Vec<u8>,at:usize }
impl Reader {
    fn take<const N:usize>(&mut self)->Result<[u8;N],String> {
        let b=self.bytes.get(self.at..self.at+N).ok_or("edited mesh: truncated file")?;
        self.at+=N;Ok(b.try_into().unwrap())
    }
    fn u32(&mut self)->Result<usize,String>{Ok(u32::from_le_bytes(self.take()?) as usize)}
    fn f32(&mut self)->Result<f32,String>{let x=f32::from_le_bytes(self.take()?);if !x.is_finite(){return Err("edited mesh: non-finite value".into())}Ok(x)}
    fn v3(&mut self)->Result<[f32;3],String>{Ok([self.f32()?,self.f32()?,self.f32()?])}
}
pub fn deform(path:&Path,source:&[ObjVertex],lit:&[MobyLit],selected:&[bool],source_materials:&[String])->Result<Edited,String>{
    if path.metadata().map_err(|e|e.to_string())?.len()>128*1024*1024 {return Err("edited mesh exceeds 128 MiB".into())}
    let mut r=Reader{bytes:std::fs::read(path).map_err(|e|e.to_string())?,at:0};
    let signature=r.take::<8>()?;
    let version2=&signature==b"R3EDIT02";
    if !version2 && &signature!=b"R3EDIT01" {return Err("edited mesh: unknown format".into())}
    let count=r.u32()?;let material_count=r.u32()?;let vertex_count=r.u32()?;
    let indices:Vec<_>=selected.iter().enumerate().filter_map(|(i,s)|s.then_some(i)).collect();
    if count!=indices.len() {return Err(format!("edited mesh source changed: expected {count} vertices, got {} (pose/LOD mismatch)",indices.len()))}
    if material_count==0 || material_count>128 || vertex_count>1_500_000 || vertex_count%3!=0 {return Err("edited mesh: invalid counts".into())}
    let mut materials=Vec::new();
    let mut animated_uv=Vec::new();
    for _ in 0..material_count {
        let n=u16::from_le_bytes(r.take()?) as usize;
        if n>256 {return Err("edited mesh: material name too long".into())}
        let bytes=r.bytes.get(r.at..r.at+n).ok_or("edited mesh: truncated material")?;
        materials.push(std::str::from_utf8(bytes).map_err(|e|e.to_string())?.to_string());r.at+=n;
        let flag=if version2 {r.u32()?}else{0};
        if flag>1{return Err("edited mesh: unknown UV policy".into())}
        animated_uv.push(flag==1);
    }
    let mut uv_delta=Vec::with_capacity(count);
    for &i in &indices {
        let uv=[r.f32()?,r.f32()?];let s=source[i].st;
        let animated=if version2 {
            let material=r.u32()?;
            if material>=material_count || source_materials[i]!=materials[material] {return Err("edited mesh: source material/order changed; rebind this asset".into())}
            animated_uv[material]
        }else{false};
        let delta=[s[0]/s[2]-uv[0],s[1]/s[2]-uv[1]];
        if !animated && delta.iter().any(|d|d.abs()>1e-4) {return Err("edited mesh: source UV/order changed; rebind this asset".into())}
        uv_delta.push(if animated {delta}else{[0.0;2]});
    }
    let mut vertices=Vec::with_capacity(vertex_count);let mut lighting=Vec::with_capacity(vertex_count);let mut triangle_materials=Vec::new();
    for vertex in 0..vertex_count {
        let ids=[r.u32()?,r.u32()?,r.u32()?];
        if ids.iter().any(|&i|i>=count){return Err("edited mesh: source index outside mesh".into())}
        let weights=r.v3()?;let offset=r.v3()?;let n=r.v3()?;let mut uv=[r.f32()?,r.f32()?];let material=r.u32()?;
        if material>=material_count || (weights.iter().sum::<f32>()-1.0).abs()>0.001 || weights.iter().any(|&w|!(-0.001..=1.001).contains(&w)) {return Err("edited mesh: invalid binding".into())}
        if vertex%3==0 {triangle_materials.push(material)} else if triangle_materials.last()!=Some(&material) {return Err("edited mesh: material changes inside triangle".into())}
        if animated_uv[material] {for k in 0..2 {uv[k]+=(0..3).map(|j|uv_delta[ids[j]][k]*weights[j]).sum::<f32>();}}
        let ids=ids.map(|i|indices[i]);let p=ids.map(|i|source[i].pos);
        let x=normal(sub(p[1],p[0]))?;let z=normal(cross(sub(p[1],p[0]),sub(p[2],p[0])))?;let y=cross(z,x);
        let pos=std::array::from_fn(|k|(0..3).map(|j|p[j][k]*weights[j]).sum::<f32>()+x[k]*offset[0]+y[k]*offset[1]+z[k]*offset[2]);
        let n=normal(std::array::from_fn(|k|x[k]*n[0]+y[k]*n[1]+z[k]*n[2]))?;
        let colour=std::array::from_fn(|k|(0..3).map(|j|source[ids[j]].colour[k] as f32*weights[j]).sum::<f32>().round().clamp(0.0,255.0) as u8);
        let light=lit[ids[0]].light;
        if ids.iter().any(|&i|lit[i].light!=light){return Err("edited mesh: binding crosses incompatible light sets".into())}
        let share=(0..3).map(|j|lit[ids[j]].n[3] as f32*weights[j]).sum::<f32>().round().clamp(-127.0,127.0) as i8;
        lighting.push(MobyLit{n:[(n[0]*127.0).round() as i8,(n[1]*127.0).round() as i8,(n[2]*127.0).round() as i8,share],
            s:std::array::from_fn(|k|(0..3).map(|j|lit[ids[j]].s[k]*weights[j]).sum()),light});
        vertices.push(ObjVertex{pos,st:[uv[0],uv[1],1.0],colour,f:256.0});
    }
    if r.at!=r.bytes.len(){return Err("edited mesh: trailing data".into())}
    Ok(Edited{vertices,lighting,materials,triangle_materials})
}
