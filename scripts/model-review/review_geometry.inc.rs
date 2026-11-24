// Conservative refinement of the original animated mesh, shared by all mobies.
// No class replacement, UV projection, new silhouette design or inferred rig.
// Edge agreement is checked before moving a midpoint: holes, hard edges,
// overlays and non-manifold joins retain their original straight boundary.
mod family_geometry {
    use super::*;
    use std::collections::HashMap;
    type Point=[i64;3];
    type Edge=(Point,Point);
    type Record=([f32;3],[f32;3],usize,bool);
    fn point(p:[f32;3])->Point {p.map(|x|(x as f64*100000.0).round() as i64)}
    fn unit(n:[f32;3])->[f32;3] {let l=dot(n,n).sqrt();if l>1e-8 {n.map(|x|x/l)}else{[0.0;3]}}
    fn normal(l:lighting::MobyLit)->[f32;3] {unit([l.n[0] as f32,l.n[1] as f32,l.n[2] as f32])}
    fn key(a:[f32;3],b:[f32;3])->(Edge,bool) {let (a,b)=(point(a),point(b));if a<b {((a,b),false)}else{((b,a),true)}}
    pub fn refine(vertices:&mut Vec<ObjVertex>,lit:&mut Vec<lighting::MobyLit>,draws:&mut Vec<((u32,u8),Draw,u32)>,selected:&mut Vec<bool>)->Result<(),String> {
        let mut edges=HashMap::<Edge,Record>::new();
        let eligible=|i:usize| (0..3).all(|k|lit[i+k].light==lit[i].light && lit[i+k].light&lighting::LIT!=0 && lit[i+k].n[3]>0 && dot(normal(lit[i+k]),normal(lit[i+k]))>0.9);
        let mut source=0usize;
        for (_,dr,fb) in draws.iter().filter(|(_,_,fb)|*fb==crate::eedraw::MAIN) {
            let _=fb;
            for i in (dr.first as usize..(dr.first+dr.count) as usize).step_by(3).filter(|&i|selected[i]) {
                source+=1;
                for (a,b) in [(i,i+1),(i+1,i+2),(i+2,i)] {
                    let (key,reverse)=key(vertices[a].pos,vertices[b].pos);
                    let (na,nb)=if reverse {(normal(lit[b]),normal(lit[a]))}else{(normal(lit[a]),normal(lit[b]))};
                    let record=edges.entry(key).or_insert((na,nb,0,true));
                    record.2+=1;
                    record.3 &= eligible(i) && dot(record.0,na)>0.999 && dot(record.1,nb)>0.999;
                }
            }
        }
        let mut out=Vec::new();let mut lights=Vec::new();let mut output_draws:Vec<((u32,u8),Draw,u32)>=Vec::new();
        let mut moved=0usize;let mut max_offset=0.0f32;let mut preserved=0usize;
        for (set,dr,fb) in draws.iter().filter(|(_,_,fb)|*fb==crate::eedraw::MAIN) {
            let first=out.len();
            for i in (dr.first as usize..(dr.first+dr.count) as usize).step_by(3).filter(|&i|selected[i]) {
                let mut vs=[vertices[i];6];let mut ls=[lit[i];6];
                vs[..3].copy_from_slice(&vertices[i..i+3]);ls[..3].copy_from_slice(&lit[i..i+3]);
                if !eligible(i) {out.extend_from_slice(&vs[..3]);lights.extend_from_slice(&ls[..3]);preserved+=1;continue;}
                let mut curved=false;
                for (slot,a,b) in [(3,0,1),(4,1,2),(5,2,0)] {
                    let e:[f32;3]=std::array::from_fn(|k|vs[b].pos[k]-vs[a].pos[k]);
                    let na=normal(ls[a]);let nb=normal(ls[b]);
                    let edge=edges[&key(vs[a].pos,vs[b].pos).0];
                    let mut offset:[f32;3]=if edge.2==2 && edge.3 {std::array::from_fn(|k|-0.125*(na[k]*dot(e,na)-nb[k]*dot(e,nb)))}else{[0.0;3]};
                    // At most 4% of the edge length. Flat original faces stay flat.
                    let length=dot(offset,offset).sqrt();let cap=dot(e,e).sqrt()*0.04;
                    if length>cap {offset=offset.map(|x|x*cap/length);}
                    let shift=dot(offset,offset).sqrt();curved|=shift>1e-7;
                    moved+=(shift>1e-7) as usize;max_offset=max_offset.max(shift);
                    vs[slot]=ObjVertex{pos:std::array::from_fn(|k|(vs[a].pos[k]+vs[b].pos[k])*0.5+offset[k]),
                        st:std::array::from_fn(|k|(vs[a].st[k]+vs[b].st[k])*0.5),
                        colour:std::array::from_fn(|k|((vs[a].colour[k] as u16+vs[b].colour[k] as u16+1)/2) as u8),f:(vs[a].f+vs[b].f)*0.5};
                    let n=unit(std::array::from_fn(|k|na[k]+nb[k]));
                    ls[slot]=lighting::MobyLit{n:[(n[0]*127.0).round() as i8,(n[1]*127.0).round() as i8,(n[2]*127.0).round() as i8,((ls[a].n[3] as i16+ls[b].n[3] as i16)/2) as i8],
                        s:std::array::from_fn(|k|(ls[a].s[k]+ls[b].s[k])*0.5),light:ls[a].light};
                }
                // Adjacent triangles also split a shared edge; flat ones must
                // split when their curved neighbour moves that midpoint.
                if curved {for v in [0,3,5,3,1,4,5,4,2,3,4,5] {out.push(vs[v]);lights.push(ls[v]);}}
                else {out.extend_from_slice(&vs[..3]);lights.extend_from_slice(&ls[..3]);preserved+=1;}
            }
            if out.len()>first {output_draws.push((*set,Draw{key:dr.key,first:first as u32,count:(out.len()-first) as u32},*fb));}
        }
        if out.iter().any(|v|v.pos.iter().chain(&v.st).any(|x|!x.is_finite())) {return Err("Family refinement produced a non-finite vertex".into());}
        eprintln!("review family geometry: {source} source triangles, {} output, {preserved} flat/unlit preserved, {moved} curved edge samples, max offset {max_offset}",out.len()/3);
        *selected=vec![true;out.len()];*vertices=out;*lit=lights;*draws=output_draws;Ok(())
    }
}
