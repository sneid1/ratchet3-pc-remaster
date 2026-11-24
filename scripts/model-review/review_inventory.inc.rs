// Offline source inventory: all LODs and variants, deduplicated by model data.
// This performs no rendering or gameplay and collects no timing measurements.
fn moby_inventory(disc:&str,target:&Path)->Result<(),String> {
    use uyafmt::{disc::Disc,toc::{self,FileKind}};
    let mut source=Disc::open(disc)?;
    let toc=toc::read(&mut source)?;
    std::fs::create_dir_all(target).map_err(|e|e.to_string())?;
    let mut table=std::io::BufWriter::new(std::fs::File::create(target.join("occurrences.tsv")).map_err(|e|e.to_string())?);
    writeln!(table,"level\tslot\tclass_id\tasset_id\tnear_triangles\tfar_triangles\tall_meshes\tskinned_meshes\trigid_meshes\tnear_joints\tfar_joints\ttextures").map_err(|e|e.to_string())?;
    let mut unresolved=String::from("level\tsource\treason\n");
    for file in toc.files.iter().filter(|f|f.kind==FileKind::Scene) {
        unresolved+=&format!("{}\tSCENE\tCutscene-only classes require SCENE decoding; not covered by LEVEL core inventory\n",file.level);
    }
    let mut occurrences=0;
    let mut assets=std::collections::BTreeSet::new();
    for file in toc.files.iter().filter(|f|f.kind==FileKind::Level) {
        let data=match uyafmt::walk::level(disc,file.level) {
            Ok(data)=>data,
            Err(error)=>{unresolved+=&format!("{}\tLEVEL\t{}\n",file.level,error.replace(['\n','\t']," "));continue;}
        };
        let core=data.core.ok_or("Decoded level has no core")?;
        for (slot,(class,record)) in core.mobys.iter().zip(&core.moby_classes).enumerate() {
            let Some(class)=class else {unresolved+=&format!("{}\tclass {}\tClass has no decoded mesh data\n",file.level,record.id);continue;};
            let locals:std::collections::BTreeSet<_>=class.meshes.iter().filter_map(|m|m.packet_a.as_ref())
                .flat_map(|p|&p.textures).map(|t|t.local).collect();
            let mut names=std::collections::BTreeSet::new();
            for local in locals {
                let index=usize::try_from(local).ok().and_then(|i|record.textures.get(i)).copied();
                if let Some(index)=index.filter(|&i|i!=255) {
                    let texture=core.moby_textures.get(index as usize).ok_or("Moby texture index outside source table")?;
                    if let Some(name)=crate::texnames::name(texture)? {names.insert(name.stem());}
                }else{names.insert(format!("runtime-material-{}-{local}",record.mode));}
            }
            let names=names.into_iter().collect::<Vec<_>>().join(",");
            let mut hash=0xcbf29ce484222325u64;
            let mut feed=|value:&str|{for byte in value.bytes(){hash=(hash^byte as u64).wrapping_mul(0x100000001b3);}};
            feed(&format!("{}/{}/{}/{}/{}/{:?}/{:?}/{names}",class.header.meshes_near,class.header.meshes_far,class.header.meshes_third,class.header.joints,class.header.joints_far,class.joints,class.bind));
            for mesh in &class.meshes {feed(&format!("{:?}{:?}",mesh.packet_a,mesh.packet_b));}
            let count=|start:usize,n:usize|class.meshes.iter().skip(start).take(n).filter_map(|m|m.packet_a.as_ref())
                .flat_map(|p|&p.strips).map(|s|s.vertices.iter().skip(2).filter(|v|!v.skip).count()).sum::<usize>();
            let near=class.header.meshes_near as usize;
            let skinned=class.meshes.iter().filter(|m|matches!(m.packet_b,Some(uyafmt::moby::PacketB::Skinned(_)))).count();
            let rigid=class.meshes.iter().filter(|m|matches!(m.packet_b,Some(uyafmt::moby::PacketB::Rigid(_)))).count();
            writeln!(table,"{}\t{slot}\t{}\t{hash:016x}\t{}\t{}\t{}\t{skinned}\t{rigid}\t{}\t{}\t{names}",
                file.level,record.id,count(0,near),count(near,class.header.meshes_far as usize),class.meshes.len(),class.header.joints,class.header.joints_far).map_err(|e|e.to_string())?;
            assets.insert(hash);occurrences+=1;
        }
        table.flush().map_err(|e|e.to_string())?;
        println!("Indexed source LEVEL {}",file.level);
    }
    std::fs::write(target.join("unresolved-sources.tsv"),unresolved).map_err(|e|e.to_string())?;
    std::fs::write(target.join("scope.txt"),format!("{occurrences} decoded LEVEL class occurrences; {} distinct source fingerprints. All LODs/variants included. This inventory is not visual approval or art completion. See unresolved-sources.tsv for sources not decoded.\n",assets.len())).map_err(|e|e.to_string())?;
    Ok(())
}
