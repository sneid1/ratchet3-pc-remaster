// Source-class material inspection in the existing native blackroom. No EE
// execution: identity skin matrices expose the original bind-space mesh. The
// saved frame supplies actual level light sets and the nearest terrain ambient.
static SOURCE_MOBIES: Mutex<Option<uyafmt::core::Core>> = Mutex::new(None);
static SOURCE_BODY: Mutex<Option<(usize,uyafmt::scenecode::HeroBody)>> = Mutex::new(None);

fn source_class(core:&uyafmt::core::Core,slot:usize,sets:&lighting::Sets,
    reference:(u32,[f32;3]))
    ->Result<(Vec<ObjVertex>,Vec<lighting::MobyLit>,Vec<Draw>),String> {
    let class=core.mobys.get(slot).and_then(|c|c.as_ref()).ok_or("Source class absent")?;
    let record=&core.moby_classes[slot];
    source_mesh(class,sets,reference,|local| {
        if (-3..=-1).contains(&local) {return Ok(core.moby_textures.len() as u32 + if local == -1 {2} else {(-2-local) as u32});}
        let tex=usize::try_from(local).ok().and_then(|i|record.textures.get(i)).copied().ok_or("Invalid source texture number")?;
        if tex==255 && record.mode>=1 {Ok(u32::MAX)} else if tex==255 {Err("Source mesh needs an unbound runtime material".into())} else {Ok(tex as u32)}
    })
}

fn source_mesh(class:&uyafmt::moby::MobyClass,sets:&lighting::Sets,reference:(u32,[f32;3]),
    texture:impl Fn(i32)->Result<u32,String>) ->Result<(Vec<ObjVertex>,Vec<lighting::MobyLit>,Vec<Draw>),String> {
    source_mesh_transform(class,sets,reference,texture,1.0,false)
}

fn source_mesh_transform(class:&uyafmt::moby::MobyClass,sets:&lighting::Sets,reference:(u32,[f32;3]),
    texture:impl Fn(i32)->Result<u32,String>,scale:f32,mirror:bool) ->Result<(Vec<ObjVertex>,Vec<lighting::MobyLit>,Vec<Draw>),String> {
    use uyafmt::moby::{SkinInputs,SkinState,JobLights};
    use crate::objects::{Blend,tex1_k};
    let (word,base)=reference;
    let lights=lighting::lights(sets,word);
    let identity=std::array::from_fn(|r|std::array::from_fn(|c|if r==c {1.0}else{0.0}));
    // Saved gameplay RAM is sparse: this immutable boot range may contain
    // zeros or stale data. Use the exact table from saved original skin jobs,
    // verified identical in three levels by source_skin_reference.py.
    let trig=SkinInputs::trig_table(include_bytes!("../../source-skin-trig.bin"))?;
    if trig.iter().any(|v| !v.iter().all(|x|x.is_finite()) || (v[0]*v[0]+v[1]*v[1]-1.0).abs()>0.001) {
        return Err("Source skin normal lookup is not a unit-circle table".into());
    }
    let input=SkinInputs{joints:vec![identity;112],
        lights:JobLights{dirs:[[0.0;4];3],colours:[[0.0;4];3],wrap:[0.0;3],flat:[128;4],base:[65537.0;4]},
        rigid:None,trig};
    let mut state=SkinState::new();let mut vertices=Vec::new();let mut lit=Vec::new();let mut draws:Vec<Draw>=Vec::new();
    let mut key=None;
    for mesh in class.meshes.iter().take(class.header.meshes_near as usize) {
        let a=mesh.packet_a.as_ref().ok_or("Source mesh has no packet A")?;
        let uv=a.uvs.as_ref().ok_or("Source mesh requires runtime rigid UVs; needs captured inspection")?;
        let sk=uyafmt::moby::skin_fast(mesh,&input,&mut state)?;
        for strip in &a.strips {
            if let Some(t)=strip.texture {
                let t=&a.textures[t];
                let tex=texture(t.local)?;
                if t.wms>1 || t.wmt>1 {return Err("Unsupported source texture clamp".into());}
                key=Some((tex as u32,t.wms==1,t.wmt==1,t.mmin,tex1_k(t.k).to_bits(),Blend::Alpha,0));
            }
            let key=key.ok_or("Source strip has no preceding texture")?;
            for k in 2..strip.vertices.len() {
                if strip.vertices[k].skip {continue;}
                let corners=if (k%2==0)!=mirror {[k-2,k-1,k]} else {[k-1,k-2,k]};
                let first=vertices.len() as u32;
                for i in corners {
                    let i=strip.vertices[i].index as usize;
                    let p=sk.positions.get(i).ok_or("Source strip position")?;
                    let mut n=sk.normals.get(i).ok_or("Source strip normal")?.map(|v|-v);
                    if mirror {n[1] = -n[1];}
                    if !n.iter().all(|x|x.is_finite()) || n.iter().map(|x|x*x).sum::<f32>()<0.1 {
                        return Err(format!("Source mesh has invalid decoded skin normal at vertex {i}"));
                    }
                    let s=lighting::terms_of(&lights,word,n);
                    let colour=std::array::from_fn(|k|if k==3 {128} else {(base[k]+128.0*(lights[0].0[k]*s[0]+lights[1].0[k]*s[1])).round().clamp(0.0,255.0) as u8});
                    let uv=uv.get(i).ok_or("Source strip UV")?;
                    let mut pos=p.map(|v|v as f32*scale/1024.0);
                    if mirror {pos[1] = -pos[1];}
                    vertices.push(ObjVertex{pos,st:[uv[0],uv[1],1.0],colour,f:256.0});
                    lit.push(lighting::MobyLit{n:lighting::snorm(n,1.0),s,light:word});
                }
                if let Some(d)=draws.last_mut().filter(|d|d.key==key){d.count+=3;}
                else {draws.push(Draw{key,first,count:3});}
            }
        }
    }
    if vertices.is_empty(){return Err("Source class has no near mesh".into());}
    eprintln!("Source mesh: {} bind-space triangles; captured level light word {word:#x}, nearest source terrain base {base:?}. Pose/rig approval separate.",vertices.len()/3);
    Ok((vertices,lit,draws))
}
