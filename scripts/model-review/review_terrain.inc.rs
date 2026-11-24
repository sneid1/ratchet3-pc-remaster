// Access the decoded original terrain directly; use the captured level light
// sets with its authored vertex normals and ambient/base colours.
impl Scene {
    pub(crate) fn review_source_light(&self,eye:[f32;3])->Result<(u32,[f32;3]),String> {
        let i=self.vertices.iter().enumerate().min_by(|(_,a),(_,b)| {
            let distance=|p:[f32;3]|(0..3).map(|k|(p[k]-eye[k]).powi(2)).sum::<f32>();
            distance(a.pos).total_cmp(&distance(b.pos))
        }).map(|(i,_)|i).ok_or("No terrain lighting reference")?;
        let l=self.lit[i];
        Ok(((l.sets&0xffff)|crate::lighting::LIT|crate::lighting::MOBY,[l.base[0] as f32,l.base[1] as f32,l.base[2] as f32]))
    }
    pub(crate) fn review_terrain(&self,chunk:u32,sets:&crate::lighting::Sets) -> Result<(Vec<crate::objects::ObjVertex>,Vec<crate::lighting::MobyLit>,Vec<crate::objects::Draw>),String> {
        let mut vertices=Vec::new();let mut lighting=Vec::new();let mut draws=Vec::new();
        for dr in self.draws.iter().filter(|d|d.chunk==chunk) {
            let (tex,cs,ct,mm,k)=self.keys[dr.key as usize];
            let first=vertices.len() as u32;
            for &index in &self.indices[dr.first as usize..(dr.first+dr.count) as usize] {
                let v=self.vertices[index as usize];let l=self.lit[index as usize];
                let word=l.sets|crate::lighting::LIT;
                let lights=crate::lighting::lights(sets,word);
                let terms=crate::lighting::terms_of(&lights,word,l.normal);
                let mut colour=l.base;
                for q in 0..3 {colour[q]=(l.base[q] as f32+128.0*(lights[0].0[q]*terms[0]+lights[1].0[q]*terms[1])).floor().clamp(0.0,255.0) as u8;}
                vertices.push(crate::objects::ObjVertex{pos:v.pos,st:[v.uv[0],v.uv[1],1.0],colour,f:256.0});
                lighting.push(crate::lighting::MobyLit{n:crate::lighting::snorm(l.normal,1.0),s:terms,light:word});
            }
            draws.push(crate::objects::Draw{key:(tex,cs,ct,mm,k,crate::objects::Blend::Alpha,0),first,count:vertices.len() as u32-first});
        }
        if vertices.is_empty(){return Err(format!("No original terrain chunk {chunk}"));}
        Ok((vertices,lighting,draws))
    }
    /// Each terrain chunk's textures (`@terrain:N` draws chunk N): chunk, its triangles, a texture's stem and that texture's
    /// triangles in it; then each tie class's textures (`tie` rows: class id, triangles, stem), which `@terrain` cannot draw.
    pub(crate) fn review_terrain_index(&self,path:&std::path::Path) -> Result<(),String> {
        use std::io::Write;
        let mut per=std::collections::BTreeMap::<(u32,String),usize>::new();
        let mut totals=std::collections::BTreeMap::<u32,usize>::new();
        for dr in &self.draws {
            let texture=&self.textures[self.keys[dr.key as usize].0 as usize];
            let stem=crate::texnames::name(texture)?.map_or("-".to_string(),|n|n.stem());
            *per.entry((dr.chunk,stem)).or_default()+=dr.count as usize/3;
            *totals.entry(dr.chunk).or_default()+=dr.count as usize/3;
        }
        let mut out=std::fs::File::create(path).map_err(|e|e.to_string())?;
        writeln!(out,"layer\tid\tid_triangles\tstem\tstem_triangles").map_err(|e|e.to_string())?;
        for ((chunk,stem),n) in &per {writeln!(out,"terrain\t{chunk}\t{}\t{stem}\t{n}",totals[chunk]).map_err(|e|e.to_string())?;}
        let (_,textures,records)=&self.ties;
        for rec in records {
            let mut names=std::collections::BTreeSet::new();
            for &t in &rec.textures {
                if let Some(texture)=textures.get(t as usize) {if let Some(n)=crate::texnames::name(texture)? {names.insert(n.stem());}}
            }
            for stem in names {writeln!(out,"tie\t{}\t0\t{stem}\t0",rec.id).map_err(|e|e.to_string())?;}
        }
        Ok(())
    }
    pub(crate) fn review_terrain_materials(&self,device:&wgpu::Device,queue:&wgpu::Queue) -> Result<crate::objects::Materials,String> {
        crate::objects::Materials::new(device,queue,&self.textures,5.0,0x60)
    }

    pub(crate) fn review_shrub_index(&self,path:&std::path::Path) -> Result<(),String> {
        use std::io::Write;
        let (classes,textures,records,game)=self.shrubs.as_ref().ok_or("No source shrubs")?;
        std::fs::create_dir_all(path).map_err(|e|e.to_string())?;
        let mut out=std::fs::File::create(path.join("instances.tsv")).map_err(|e|e.to_string())?;
        writeln!(out,"instance\tclass_id\ttriangles\tposition\ttextures").map_err(|e|e.to_string())?;
        let mut seen=std::collections::HashSet::new();
        for (i,s) in game.shrubs.iter().enumerate() {
            // One intact kit instance of each source class is enough for the
            // material contact sheet. No duplicate placements inflate coverage.
            if !seen.insert(s.class){continue;}
            let ci=records.iter().position(|c|c.id==s.class).ok_or("Shrub class record")?;
            let class=&classes[ci];let rec=&records[ci];
            let mut names=std::collections::BTreeSet::new();let mut triangles=0;
            for block in &class.blocks {for strip in &block.strips {
                triangles+=strip.vertices.len().saturating_sub(2);
                let slot=block.textures[strip.texture].index as usize;
                let texture=&textures[rec.textures[slot] as usize];
                if let Some(name)=crate::texnames::name(texture)? {
                    names.insert(name.stem());
                    let file=path.join(format!("{}.png",name.stem()));
                    if !file.exists() {
                        std::fs::write(file,crate::pic::png_rgba(texture.width as usize,texture.height as usize,bytemuck::cast_slice(&texture.levels[0]))).map_err(|e|e.to_string())?;
                    }
                }
            }}
            writeln!(out,"{i}\t{}\t{triangles}\t{:?}\t{}",s.class,s.position,names.into_iter().collect::<Vec<_>>().join(",")).map_err(|e|e.to_string())?;
        }
        Ok(())
    }

    pub(crate) fn review_shrub_materials(&self,device:&wgpu::Device,queue:&wgpu::Queue) -> Result<crate::objects::Materials,String> {
        let source=self.shrubs.as_ref().ok_or("No source shrubs")?;
        crate::objects::Materials::new(device,queue,&source.1,-5.0,0x40)
    }

    // Keep the kit mesh, captured transform, vertex colours and source opacity.
    // Shrub packets contain no authored vertex normals/light-set IDs. Normals
    // therefore come from the mesh; the nearest original terrain vertex supplies
    // its actual light-set blend. This explicit spatial lighting reconstruction is
    // for the sandbox, not a claim of recovered original shrub normal data.
    pub(crate) fn review_shrub(&self,id:usize,ram:&[u8],g:&crate::objects::ShrubGlobals,sets:&crate::lighting::Sets) -> Result<(Vec<crate::objects::ObjVertex>,Vec<crate::lighting::MobyLit>,Vec<crate::objects::Draw>),String> {
        use crate::objects::{ObjVertex,Draw,Blend};
        use crate::lighting::{MobyLit,LIT,snorm,lights,terms_of};
        let (classes,_,records,game)=self.shrubs.as_ref().ok_or("No source shrubs")?;
        let s=game.shrubs.get(id).ok_or("Shrub instance outside original kit")?;
        let ci=records.iter().position(|r|r.id==s.class).ok_or("Shrub source class")?;
        let class=&classes[ci];let rec=&records[ci];
        let word=|at:usize| -> Result<u32,String>{Ok(u32::from_le_bytes(ram.get(at..at+4).ok_or("Shrub RAM address")?.try_into().unwrap()))};
        let float=|at:usize| -> Result<f32,String>{Ok(f32::from_bits(word(at)?))};
        if word(g.count as usize)? as usize!=game.shrubs.len(){return Err("Shrub capture count differs from source level".into());}
        let arr=word(g.instances as usize)? as usize+id*32;
        let matrix=word(g.matrices as usize)? as usize+id*64;
        let block_number=(word(arr+0x18)?&0xffff) as usize;
        let colours=word(g.colours as usize)? as usize+block_number*96;
        let mut rows=[[0.0f32;3];4];
        for r in 0..4 {for k in 0..3 {rows[r][k]=float(matrix+16*r+4*k)?;}}
        let scale=float(matrix+0x3c)?;
        let nearest=self.vertices.iter().enumerate().min_by(|(_,a),(_,b)|{
            let d=|p:[f32;3]|(0..3).map(|k|(p[k]-rows[3][k]).powi(2)).sum::<f32>();
            d(a.pos).total_cmp(&d(b.pos))
        }).ok_or("No terrain light anchor")?.0;
        // Bit 18 is sandbox-only: these original uncullled leaf/prop strips
        // require a two-sided physical normal in the material shader.
        let light=self.lit[nearest].sets|LIT|(1<<18);let source_lights=lights(sets,light);
        let mut vertices=Vec::new();let mut lighting=Vec::new();let mut draws=Vec::new();
        let norm=|v:[f32;3]|{let len=v.iter().map(|v|v*v).sum::<f32>().sqrt().max(1e-12);v.map(|v|v/len)};
        let cross=|a:[f32;3],b:[f32;3]|[a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]];
        for bl in &class.blocks {
            let pos:Vec<[f32;3]>=bl.vertices.iter().map(|v|std::array::from_fn(|q|rows[3][q]+(0..3).map(|k|v.pos[k]/1024.0*scale*rows[k][q]).sum::<f32>())).collect();
            let mut normals=vec![[0.0;3];pos.len()];
            let mut triangles=Vec::new();
            for st in &bl.strips {
                let te=&bl.textures[st.texture];
                if te.wms>1 || te.wmt>1{return Err("Unsupported shrub clamp".into());}
                let key=(rec.textures[te.index as usize] as u32,te.wms==1,te.wmt==1,te.mmin,crate::objects::tex1_k(te.k).to_bits(),Blend::Alpha,0);
                for k in 2..st.vertices.len() {
                    let mut tri=[st.vertices[k-2],st.vertices[k-1],st.vertices[k]];
                    if k%2==1 {tri.swap(0,1);}
                    triangles.push((key,tri));
                }
            }
            // These original strips were rendered without normals/culling.
            // Degenerate restarts and inconsistent source face winding must not
            // create opposing normals in one petal. Orient each manifold patch
            // through shared geometric edges, then average coincident corners.
            let mut welded=std::collections::HashMap::new();
            let mut ids=Vec::new();
            for p in &pos {
                let next=welded.len();ids.push(*welded.entry(p.map(f32::to_bits)).or_insert(next));
            }
            let mut edges=std::collections::HashMap::<(usize,usize),Vec<(usize,bool)>>::new();
            for (t,(_,tri)) in triangles.iter().enumerate() {
                let a=std::array::from_fn(|q|pos[tri[1]][q]-pos[tri[0]][q]);
                let b=std::array::from_fn(|q|pos[tri[2]][q]-pos[tri[0]][q]);
                if cross(a,b).iter().map(|v|v*v).sum::<f32>()<1e-14{continue;}
                for k in 0..3 {
                    let (a,b)=(ids[tri[k]],ids[tri[(k+1)%3]]);if a==b{continue;}
                    edges.entry((a.min(b),a.max(b))).or_default().push((t,a<b));
                }
            }
            let mut neighbours=vec![Vec::new();triangles.len()];
            for edge in edges.values().filter(|v|v.len()==2) {
                let (a,b)=(edge[0],edge[1]);
                neighbours[a.0].push((b.0,a.1==b.1));neighbours[b.0].push((a.0,a.1==b.1));
            }
            let mut flip=vec![None;triangles.len()];
            for seed in 0..triangles.len() {
                if flip[seed].is_some(){continue;}
                flip[seed]=Some(false);let mut stack=vec![seed];let mut patch=Vec::new();
                while let Some(t)=stack.pop(){patch.push(t);for &(other,invert) in &neighbours[t] {
                    if flip[other].is_none(){flip[other]=Some(flip[t].unwrap()^invert);stack.push(other);}
                }}
                // The source's predominant face direction wins; preserve that
                // orientation instead of imposing a world-axis normal.
                if patch.iter().filter(|&&t|flip[t]==Some(true)).count()>patch.len()/2 {
                    for &t in &patch{flip[t]=Some(!flip[t].unwrap());}
                }
            }
            let mut shared=vec![[0.0;3];welded.len()];
            for (t,(_,tri)) in triangles.iter_mut().enumerate() {
                if flip[t]==Some(true){tri.swap(0,1);}
                let a=std::array::from_fn(|q|pos[tri[1]][q]-pos[tri[0]][q]);
                let b=std::array::from_fn(|q|pos[tri[2]][q]-pos[tri[0]][q]);let n=cross(a,b);
                for &vi in tri.iter(){for q in 0..3{shared[ids[vi]][q]+=n[q];}}
            }
            for vi in 0..normals.len(){normals[vi]=shared[ids[vi]];}
            for (key,tri) in triangles {
                if draws.last().is_none_or(|d:&Draw|d.key!=key){draws.push(Draw{key,first:vertices.len() as u32,count:0});}
                for vi in tri {
                    let v=&bl.vertices[vi];if v.colour>=24{return Err("Shrub colour index outside capture".into());}
                    let mut colour=word(colours+v.colour as usize*4)?.to_le_bytes();colour[3]=ram[colours+3];
                    let n=norm(normals[vi]);let terms=terms_of(&source_lights,light,n);
                    vertices.push(ObjVertex{pos:pos[vi],st:v.st,colour,f:256.0});
                    lighting.push(MobyLit{n:snorm(n,1.0),s:terms,light});
                }
                draws.last_mut().unwrap().count+=3;
            }
        }
        if vertices.is_empty(){return Err("Empty source shrub".into());}
        eprintln!("review shrub: instance {id}, class {}, terrain light anchor {nearest}, light word {light:#x}; reconstructed mesh normals",s.class);
        Ok((vertices,lighting,draws))
    }

    /// One row per tie instance (`@tie:N` draws it): instance, class id, LOD 0's triangles, position and LOD 0's texture stems.
    pub(crate) fn review_tie_index(&self,path:&std::path::Path) -> Result<(),String> {
        use std::io::Write;
        let (classes,textures,records)=&self.ties;
        let game=&self.shrubs.as_ref().ok_or("No source gameplay block")?.3;
        let mut out=std::fs::File::create(path).map_err(|e|e.to_string())?;
        writeln!(out,"instance\tclass_id\ttriangles\tposition\tstems").map_err(|e|e.to_string())?;
        for (i,t) in game.ties.iter().enumerate() {
            let ci=records.iter().position(|r|r.id==t.class).ok_or("Tie class record")?;
            let class=&classes[ci];let rec=&records[ci].textures;
            let mut names=std::collections::BTreeSet::new();let mut triangles=0;
            for pk in &class.lods[0] {for tri in &pk.triangles {
                triangles+=1;
                let key=Self::review_tie_key(class,pk,tri,rec)?;
                if let Some(n)=crate::texnames::name(textures.get(key.0 as usize).ok_or("Tie texture")?)? {names.insert(n.stem());}
            }}
            writeln!(out,"{i}\t{}\t{triangles}\t{:?}\t{}",t.class,t.position,names.into_iter().collect::<Vec<_>>().join(",")).map_err(|e|e.to_string())?;
        }
        Ok(())
    }
    /// The game's texture setting of a tie triangle (`objects::tie_key`, private there).
    fn review_tie_key(c:&uyafmt::tie::TieClass,pk:&uyafmt::tie::TiePacket,tri:&uyafmt::tie::TieTriangle,rec:&[u8;16])->Result<crate::objects::Key,String> {
        let j=*pk.textures.get(tri.texture).ok_or("tie triangle texture")?;
        let reg=c.textures.get(j).ok_or("tie texture register")?;
        let texture=*rec.get(j).ok_or("tie texture index byte")? as u32;
        if reg.wms>1 || reg.wmt>1 {return Err(format!("tie CLAMP WMS {} WMT {}",reg.wms,reg.wmt));}
        let blend=crate::objects::Blend::of(reg.alpha_a,reg.alpha_b,reg.alpha_c,reg.alpha_d,reg.alpha_fix)?;
        Ok((texture,reg.wms==1,reg.wmt==1,reg.mmin,crate::objects::tex1_k(reg.k_word).to_bits(),blend,0))
    }
    pub(crate) fn review_tie_materials(&self,device:&wgpu::Device,queue:&wgpu::Queue) -> Result<crate::objects::Materials,String> {
        crate::objects::Materials::new(device,queue,&self.ties.1,-5.0,0x40)
    }
    /// A tie instance as the game lit and drew it (`Ties::frame`): LOD 0's first-draw triangles through the instance's matrix and the
    /// class scale, the colour words of its colour buffer in RAM, the light word at instance +0x18 with each colour word's light
    /// positions' normals and terms. Second draws are not made. An instance with no colour buffer, or lit for another LOD, is an error.
    pub(crate) fn review_tie(&self,id:usize,ram:&[u8],g:&crate::objects::TieGlobals,sets:&crate::lighting::Sets) -> Result<(Vec<crate::objects::ObjVertex>,Vec<crate::lighting::MobyLit>,Vec<crate::objects::Draw>),String> {
        use crate::objects::{ObjVertex,Draw};
        use crate::lighting::{MobyLit,LIT,TIE,snorm,lights,terms_of};
        let (classes,_,records)=&self.ties;
        let game=&self.shrubs.as_ref().ok_or("No source gameplay block")?.3;
        let inst=game.ties.get(id).ok_or("Tie instance outside the level")?;
        let ci=records.iter().position(|r|r.id==inst.class).ok_or("Tie class record")?;
        let class=&classes[ci];let rec=&records[ci].textures;
        let word=|at:usize| -> Result<u32,String>{Ok(u32::from_le_bytes(ram.get(at..at+4).ok_or("Tie RAM address")?.try_into().unwrap()))};
        let float=|at:usize| -> Result<f32,String>{Ok(f32::from_bits(word(at)?))};
        if word(g.count as usize)? as usize!=game.ties.len(){return Err("Tie capture count differs from source level".into());}
        let a=word(g.instances as usize)? as usize+32*id;
        let m=word(g.matrices as usize)? as usize+64*id;
        let cols=word(g.colours as usize)? as usize;
        let buffer_index=u16::from_le_bytes(ram.get(a+0x14..a+0x16).ok_or("Tie RAM address")?.try_into().unwrap());
        if buffer_index==0xffff {return Err(format!("Tie instance {id} has no colour buffer in this capture (the game had not lit it); pick an instance the capture lit"));}
        let lod=ram[a+0x1f];
        if lod!=0 {return Err(format!("Tie instance {id} was lit for LOD {lod}, not LOD 0; its colour words are not LOD 0's"));}
        let buffer=cols+16*buffer_index as usize;
        let mut rows=[[0.0f32;3];4];
        for r in 0..4 {for k in 0..3 {rows[r][k]=float(m+16*r+4*k)?;}}
        let scale=float(m+0x3c)?;
        let light=u16::from_le_bytes(ram[a+0x18..a+0x1a].try_into().unwrap()) as u32|TIE|LIT;
        let source_lights=lights(sets,light);
        let unit=|r:[f32;3]|{let l=(r[0]*r[0]+r[1]*r[1]+r[2]*r[2]).sqrt();[r[0]/l,r[1]/l,r[2]/l]};
        let axes=[unit(rows[0]),unit(rows[1]),unit(rows[2])];
        let cs=|k:u16|{let t=std::f64::consts::TAU*(k&0xff) as f64/256.0;(t.cos() as f32,t.sin() as f32)};
        let lit_at:Vec<([f32;3],[f32;2])>=class.light_positions.iter().map(|&(_,w)|{
            let ((ca,sa),(cb,sb))=(cs(w),cs(w>>8));let n=[ca*cb,sa*cb,sb];
            let wn=[0,1,2].map(|q|-(n[0]*axes[0][q]+n[1]*axes[1][q]+n[2]*axes[2][q]));
            (wn,terms_of(&source_lights,light,wn))
        }).collect();
        let mut vertices=Vec::new();let mut lighting=Vec::new();let mut draws:Vec<Draw>=Vec::new();
        for pk in &class.lods[0] {
            let mut verts=Vec::new();
            for v in &pk.vertices {
                let p=v.pos.map(|x|x/1024.0*scale);
                let pos=[0,1,2].map(|q|p[0]*rows[0][q]+p[1]*rows[1][q]+p[2]*rows[2][q]+rows[3][q]);
                let at=buffer+pk.colour_offset as usize+4*v.colour as usize;
                let w=ram.get(at..at+4).ok_or("Tie colour past RAM")?;
                let slot=pk.colour_offset as usize/4+v.colour as usize;
                let (mut n,mut ls,mut share)=([0f32;3],[0f32;2],0f32);
                for &(k,wt) in class.light_slots[0].get(slot).map_or(&[][..],|s|s.as_slice()) {
                    if k==511 {continue;}
                    let (nk,sk)=*lit_at.get(k as usize).ok_or("Tie light position outside class")?;
                    let wt=wt as f32/4.0;
                    n=[0,1,2].map(|q|n[q]+wt*nk[q]);ls=[ls[0]+wt*sk[0],ls[1]+wt*sk[1]];share+=wt;
                }
                let lt=if share==0.0 || n==[0.0;3] {MobyLit{n:[0;4],s:[0.0;2],light:0}} else {MobyLit{n:snorm(n,share),s:ls,light}};
                verts.push((ObjVertex{pos,st:[v.uv[0],v.uv[1],1.0],colour:[w[0],w[1],w[2],w[3]],f:256.0},lt));
            }
            for tri in &pk.triangles {
                let key=Self::review_tie_key(class,pk,tri,rec)?;
                if draws.last().is_none_or(|d|d.key!=key){draws.push(Draw{key,first:vertices.len() as u32,count:0});}
                for &vi in &tri.v {let (v,l)=verts.get(vi as usize).ok_or("Tie triangle vertex")?;vertices.push(*v);lighting.push(*l);}
                draws.last_mut().unwrap().count+=3;
            }
        }
        if vertices.is_empty(){return Err("Empty tie LOD 0".into());}
        eprintln!("review tie: instance {id}, class {}, colour buffer {buffer_index}, light word {light:#x}; second draws not made",inst.class);
        Ok((vertices,lighting,draws))
    }
}
