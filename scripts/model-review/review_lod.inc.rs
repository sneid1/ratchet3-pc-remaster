// Included inside the isolated mobies module. Reuse a captured pose only when
// every required joint exists and the original low mesh reproduces that capture.
impl Mobies {
    fn review_near_runs(&mut self, runs: Vec<Run>, core: u32, skin: &[(u64,std::sync::Arc<uyart::hw::SkinJoints>)]) -> Result<Vec<Run>,String> {
        use uyafmt::moby::{SkinInputs,SkinState,JobLights,PacketB};
        let mut inputs=HashMap::<u32,Vec<SkinInputs>>::new();
        for (_,capture) in skin {
            let trig=SkinInputs::trig_table(&capture.trig)?;
            for &(_,qwc,at) in &capture.records {
                let Some(bytes)=capture.bytes.get(at..at+0x1c00+0x40+16*qwc as usize) else {continue};
                let (joint,d)=bytes.split_at(0x1c00);
                let word=|o:usize|u32::from_le_bytes(d[o..o+4].try_into().unwrap());
                let n=word(0) as usize;
                if n==0 || n>200 || qwc as usize!=9+n/2 || d.len()<0xc0+n*8 {continue;}
                let joints=(0..112).map(|j|std::array::from_fn(|r|std::array::from_fn(|c|f32::from_le_bytes(joint[64*j+16*r+4*c..64*j+16*r+4*c+4].try_into().unwrap())))).collect();
                let input=SkinInputs{joints,lights:JobLights::read(&d[..0xc0])?,rigid:None,trig:trig.clone()};
                for k in 0..n { let pb=(word(0xc0+k*8)&!0xff)>>4;inputs.entry(pb).or_default().push(input.clone()); }
            }
        }
        self.skinned.retain(|at,_|*at<0xf0000000);
        let mut output=Vec::new();
        let mut replaced=std::collections::HashSet::new();
        for run in runs {
            let found=run.vu.packet_a.and_then(|pa|pa.checked_sub(core)).and_then(|a|self.by_packet.get(&a).copied());
            let Some((class_index,mesh_index))=found else {output.push(run);continue;};
            let class=self.classes[class_index].as_ref().unwrap();
            let near=class.header.meshes_near as usize;
            if class.header.meshes_far==0 || mesh_index<near || mesh_index>=near+class.header.meshes_far as usize || class.header.joints!=class.header.joints_far || class.header.joints==0 {
                output.push(run);continue;
            }
            let actor=(class_index,run.vu.rows.map(|r|r.map(f32::to_bits)));
            if replaced.contains(&actor) {continue;}
            let mesh=&class.meshes[mesh_index];
            let base=run.vu.packet_a.unwrap()-mesh.a_at as u32;
            // The first far mesh starts its skin state. Later meshes may depend
            // on earlier staging vertices, so never use them as a pose proof.
            if mesh_index!=near {output.push(run);continue;}
            let Some(candidates)=inputs.get(&(base+mesh.b_at as u32)) else {return Err(format!("review near: no captured input for class {class_index}"));};
            let candidate=candidates.iter().rev().find(|input| {
                let Ok(check)=uyafmt::moby::skin_fast(mesh,input,&mut SkinState::new()) else {return false};
                check.positions.len()==run.vu.positions.len() && check.positions.iter().zip(&run.vu.positions).all(|(a,b)|(0..3).all(|k|(a[k] as i32-b[k] as i32).abs()<=1))
            }).ok_or_else(||format!("review near: captured pose mismatch for class {class_index}"))?;
            let mut state=SkinState::new();
            let mut prepared=Vec::new();
            for high in class.meshes.iter().take(near) {
                if high.packet_a.is_none() {return Err("review near: source mesh has no packet A".into());}
                let sk=uyafmt::moby::skin(high,candidate,&mut state)?;
                let flat=match &high.packet_b {Some(PacketB::Skinned(s))=>sk.positions.len()-s.constant_normals as usize,_=>return Err("review near: rigid group requires its captured transform".into())};
                let l=candidate.lights;
                let terms=sk.normals.iter().map(|v| {
                    let len=(v[0]*v[0]+v[1]*v[1]+v[2]*v[2]).sqrt().max(f32::MIN_POSITIVE);
                    [0,1].map(|k|{let d=(l.dirs[0][k]*v[0]+l.dirs[1][k]*v[1]+l.dirs[2][k]*v[2])/len;d.max(d*l.wrap[k].min(0.0))})
                }).collect();
                let key=0xf0000000+(output.len()+prepared.len()) as u32;
                let mut next=run.clone();
                next.vu.packet_a=Some(base+high.a_at as u32);next.vu.positions_at=Some(key);
                next.vu.positions=sk.positions.clone();next.vu.colours=sk.colours;next.vu.uvs=sk.uvs;
                prepared.push((next,key,std::sync::Arc::new(SkinOut{positions:sk.positions,normals:sk.normals,terms,flat,lights:l})));
            }
            eprintln!("review near: class {class_index}, captured {} joints, {} near meshes",class.header.joints,near);
            for (next,key,sk) in prepared {self.put_skinned(key,sk);output.push(next);}
            replaced.insert(actor);
        }
        Ok(output)
    }
}
