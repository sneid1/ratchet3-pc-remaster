//! A black-background material inspection stage using the game's actual renderer.
//! Compiled only in the isolated snapshot made by scripts/model-review/prepare.py.
use std::{io::{BufRead, Write}, path::Path, sync::Mutex};
use wgpu::util::DeviceExt;
use crate::{lighting, native::Camera, objects::{Draw, ObjVertex}};

#[derive(Clone)]
struct Options {
    prefixes: Vec<String>, yaw: f32, pitch: f32, zoom: f32,
    pan_x: f32, pan_y: f32, instance: isize, channel: u32, whole: bool, output: String, edited: String, pair:[f32;2], emission_policy:String,
}
static OPTIONS: Mutex<Option<Options>> = Mutex::new(None);
static RUNS: Mutex<Vec<(usize, usize, [f32; 3])>> = Mutex::new(Vec::new());
static SOURCE_SCENE:Mutex<Option<crate::native::Scene>>=Mutex::new(None);
// The level's 30 SCENE records (uyafmt::scene::load): cutscene-only classes, drawn by "@scene:<record*1000+entry>".
static SOURCE_SCENE_BLOCKS:Mutex<Option<Vec<Option<uyafmt::scene::LoadedBlock>>>>=Mutex::new(None);
include!("review_inventory.inc.rs");
include!("review_geometry.inc.rs");
include!("review_source.inc.rs");
pub(crate) fn channel() -> f32 { OPTIONS.lock().unwrap().as_ref().map_or(0.0, |o|o.channel as f32) }
pub(crate) fn enabled() -> bool { OPTIONS.lock().unwrap().is_some() }
pub(crate) fn near_source() -> bool { OPTIONS.lock().unwrap().as_ref().is_some_and(|o|o.edited.starts_with("@near")) }
pub(crate) fn record_run(first: usize, end: usize, origin: [f32; 3]) {
    RUNS.lock().unwrap().push((first, end, origin));
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 { (0..3).map(|k| a[k] * b[k]).sum() }
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 { dot(std::array::from_fn(|k| a[k]-b[k]), std::array::from_fn(|k| a[k]-b[k])) }

pub fn serve() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len()==6 && args[1]=="--skin-image" {
        let wad=args[3].parse().map_err(|_|"skin WAD number")?;
        let pair=args[4].parse().map_err(|_|"skin pair number")?;
        let texture=uyafmt::moby::wad_image(&args[2],wad,pair)?;
        let name=crate::texnames::name(&texture)?.ok_or("Unnamed skin texture")?.stem();
        let target=Path::new(&args[5]);std::fs::create_dir_all(target).map_err(|e|e.to_string())?;
        let bytes:Vec<u8>=texture.levels[0].iter().flatten().copied().collect();
        std::fs::write(target.join(format!("{name}.png")),crate::pic::png_rgba(texture.width,texture.height,&bytes)).map_err(|e|e.to_string())?;
        println!("Skin WAD {wad} pair {pair}: {name}, {} x {}",texture.width,texture.height);
        return Ok(());
    }
    if args.len()==5 && args[1]=="--shrub-index" {
        let scene=crate::native::Scene::decode(&args[2],args[3].parse().map_err(|_|"level")?)?;
        return scene.review_shrub_index(Path::new(&args[4]));
    }
    if args.len()==5 && args[1]=="--terrain-index" {
        let scene=crate::native::Scene::decode(&args[2],args[3].parse().map_err(|_|"level")?)?;
        return scene.review_terrain_index(Path::new(&args[4]));
    }
    if args.len()==5 && args[1]=="--tie-index" {
        let scene=crate::native::Scene::decode(&args[2],args[3].parse().map_err(|_|"level")?)?;
        return scene.review_tie_index(Path::new(&args[4]));
    }
    if args.len()==4 && args[1]=="--moby-index" {return moby_inventory(&args[2],Path::new(&args[3]));}
    if args.len()==11 && args[1]=="--probe" {
        let number=|i:usize|args[i].parse::<f32>().map_err(|_|"Probe coordinate".to_string());
        return crate::review_probe::capture(&args[2],args[3].parse().map_err(|_|"Probe level")?,&args[4],&args[5],Path::new(&args[6]),Path::new(&args[7]),[number(8)?,number(9)?,number(10)?]);
    }
    // One existing captured view, for material review in its real surroundings.
    // It does not run gameplay, advance the simulation or measure throughput.
    if args.len()==9 && args[1]=="--world" {
        let mut settings=crate::settings::Settings::default();
        settings.set_pairs("lighting=per_pixel,texture_filtering=16x,rift_apart_surfaces=on")?;
        crate::enhance::set(&settings);
        crate::texnames::open(Path::new(&args[5]),Some(Path::new(&args[6])))?;
        let mut native=crate::native::Native::open(&args[2],args[4].parse().map_err(|_|"height")?)?;
        let ram=std::fs::read(&args[7]).map_err(|e|e.to_string())?;
        let p=Path::new(&args[7]);
        let suffix=p.file_name().and_then(|s|s.to_str()).and_then(|s|s.strip_prefix("ram")).ok_or("RAM capture name")?;
        let jobs=crate::jobs::FrameJobs::read(&p.with_file_name(format!("jobs{suffix}")).to_string_lossy())?;
        let frame=native.draw(&ram,&jobs,args[3].parse().map_err(|_|"level")?)?;
        std::fs::write(&args[8],crate::pic::png(&frame)).map_err(|e|e.to_string())?;
        println!("REVIEW OK");return Ok(());
    }
    if args.len()==5 && args[1]=="--asset-info" {
        let level:usize=args[3].parse().map_err(|_|"level")?;
        let data=uyafmt::walk::level(&args[2],level)?;
        let core=data.core.ok_or("level has no core")?;
        let target=Path::new(&args[4]);std::fs::create_dir_all(target).map_err(|e|e.to_string())?;
        let mut table=String::from("class\tnear_meshes\tfar_meshes\tnear_triangles\tfar_triangles\tjoints\tfar_joints\ttextures\n");
        for (index,(class,record)) in core.mobys.iter().zip(&core.moby_classes).enumerate() {
            let Some(class)=class else {continue};
            let names=record.textures.iter().filter_map(|&i|core.moby_textures.get(i as usize))
                .filter_map(|t|crate::texnames::name(t).transpose()).collect::<Result<Vec<_>,_>>()?;
            let count=|start:usize,n:usize|class.meshes.iter().skip(start).take(n)
                .filter_map(|m|m.packet_a.as_ref()).flat_map(|a|&a.strips)
                .map(|s|s.vertices.iter().skip(2).filter(|v|!v.skip).count()).sum::<usize>();
            let near=class.header.meshes_near as usize;let far=class.header.meshes_far as usize;
            table+=&format!("{index}\t{near}\t{far}\t{}\t{}\t{}\t{}\t{}\n",count(0,near),count(near,far),class.header.joints,class.header.joints_far,names.iter().map(|n|n.stem()).collect::<Vec<_>>().join(","));
            std::fs::write(target.join(format!("class-{index}.txt")),format!("{class:#?}")).map_err(|e|e.to_string())?;
        }
        std::fs::write(target.join("classes.tsv"),table).map_err(|e|e.to_string())?;
        return Ok(());
    }
    if args.len() != 6 { return Err("usage: uya-model-review DISC LEVEL HEIGHT PACKS RIFT; tab-separated render requests on stdin".into()); }
    let level = args[2].parse().map_err(|_| "level")?;
    let height = args[3].parse().map_err(|_| "height")?;
    let mut settings = crate::settings::Settings::default();
    settings.set_pairs("lighting=per_pixel,texture_filtering=16x,rift_apart_surfaces=on")?;
    crate::enhance::set(&settings);
    crate::texnames::open(Path::new(&args[4]), Some(Path::new(&args[5])))?;
    crate::review_probe::file(Some(Path::new(&args[5]).parent().unwrap().parent().unwrap().join(format!("probes/L{level:02}/probe.bin"))));
    let cache = Path::new(&args[4]).parent().ok_or("pack parent")?.join("review-pipeline-cache");
    std::fs::create_dir_all(&cache).map_err(|e|e.to_string())?;
    crate::pipelines::open(&cache);
    let mut native = crate::native::Native::open(&args[1], height)?;
    crate::pipelines::save()?;
    let mut last_path = String::new();
    let mut ram = Vec::new();
    let mut jobs = None;
    println!("REVIEW READY");
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    for line in std::io::stdin().lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line == "quit" { break; }
        let result = (|| -> Result<(), String> {
            let v: Vec<&str> = line.split('\t').collect();
            if v.len() != 14 && v.len()!=15 { return Err("expected 14 review fields and optional source emission policy".into()); }
            let pair:Vec<f32>=v[13].split(',').map(str::parse).collect::<Result<_,_>>().map_err(|_|"attachment pair")?;
            if pair.len()!=2 || pair.iter().any(|x|!x.is_finite() || *x<0.0){return Err("invalid attachment pair".into());}
            let number = |k: usize| -> Result<f32, String> {
                let x: f32 = v[k].parse().map_err(|_| format!("number at {k}"))?;
                if !x.is_finite() { return Err("non-finite option".into()); }
                Ok(x)
            };
            *OPTIONS.lock().unwrap() = Some(Options { prefixes: v[1].split(',').map(str::to_owned).collect(),
                yaw: number(2)?.to_radians(), pitch: number(3)?.clamp(-85.0,85.0).to_radians(),
                zoom: number(4)?.clamp(0.2,8.0), pan_x: number(5)?, pan_y: number(6)?,
                instance: v[7].parse().map_err(|_| "instance")?, channel: v[10].parse().map_err(|_| "channel")?, whole: v[11]=="1", output:v[8].to_owned(), edited:v[12].to_owned(), pair:[pair[0],pair[1]],emission_policy:v.get(14).unwrap_or(&"").to_string() });
            if (v[12].starts_with("@terrain:") || v[12].starts_with("@shrub:") || v[12].starts_with("@tie:") || v[12].starts_with("@source:") || v[12].starts_with("@body:") || v[12].starts_with("@scene:")) && SOURCE_SCENE.lock().unwrap().is_none() {
                *SOURCE_SCENE.lock().unwrap()=Some(crate::native::Scene::decode(&args[1],level)?);
            }
            if (v[12].starts_with("@source:") || v[12].starts_with("@body:") || v[12].starts_with("@scene:")) && SOURCE_MOBIES.lock().unwrap().is_none() {
                *SOURCE_MOBIES.lock().unwrap()=uyafmt::walk::level(&args[1],level as usize)?.core;
            }
            if v[12].starts_with("@scene:") && SOURCE_SCENE_BLOCKS.lock().unwrap().is_none() {
                let mut disc=uyafmt::disc::Disc::open(&args[1])?;let toc=uyafmt::toc::read(&mut disc)?;
                let file=toc.files.iter().find(|f|f.level==level as usize && f.kind==uyafmt::toc::FileKind::Scene).ok_or("Level has no SCENE file")?;
                let mut blocks=Vec::new();
                for k in 0..30 {blocks.push(uyafmt::scene::load(&mut disc,file,k)?);}
                *SOURCE_SCENE_BLOCKS.lock().unwrap()=Some(blocks);
            }
            if v[9] == "1" {
                crate::texnames::open(Path::new(&args[4]), Some(Path::new(&args[5])))?;
                native.reload();
            }
            if last_path != v[0] {
                ram = std::fs::read(v[0]).map_err(|e| e.to_string())?;
                let p = Path::new(v[0]);
                let suffix = p.file_name().and_then(|s| s.to_str()).and_then(|s| s.strip_prefix("ram")).ok_or("expected ramNNNNN.bin")?;
                jobs = Some(crate::jobs::FrameJobs::read(&p.with_file_name(format!("jobs{suffix}")).to_string_lossy())?);
                last_path = v[0].to_owned();
            }
            let jobs = jobs.as_ref().ok_or("no jobs")?;
            if jobs.skin.is_empty() && !v[12].starts_with("@terrain:") && !v[12].starts_with("@shrub:") && !v[12].starts_with("@tie:") && !v[12].starts_with("@source:") && !v[12].starts_with("@body:") && !v[12].starts_with("@scene:") { return Err("pose has no saved skin joints; capture with per-pixel lighting enabled".into()); }
            let frame = native.draw(&ram, jobs, level)?;
            std::fs::write(v[8], crate::pic::png(&frame)).map_err(|e| e.to_string())?;
            println!("REVIEW OK");
            Ok(())
        })();
        if let Err(e) = result { println!("REVIEW ERROR {}", e.replace(['\n','\r'], " ")); }
        std::io::stdout().flush().map_err(|e| e.to_string())?;
    }
    Ok(())
}

// The level's tie globals, read from its own code on the disc (`draw`'s callers pass only the shrubs'); the level is the
// session's (command line argument 2).
fn tie_globals() -> Result<crate::objects::TieGlobals,String> {
    static CACHE: std::sync::Mutex<Option<crate::objects::TieGlobals>> = std::sync::Mutex::new(None);
    let mut cache=CACHE.lock().unwrap();
    if let Some(g)=*cache {return Ok(g);}
    let iso=std::env::args().nth(1).ok_or("Missing disc path")?;
    let level:usize=std::env::args().nth(2).ok_or("Missing level")?.parse().map_err(|_|"level")?;
    let g=uyafmt::levelcode::all(&iso)?.into_iter().find(|g|g.level==level).ok_or("Level has no code globals")?;
    let t=g.ties;
    let globals=crate::objects::TieGlobals{count:t.count,instances:t.instances,matrices:t.matrices,colours:t.colours,list:t.list,list_bytes:t.list_bytes,records:t.records,tests:t.tests,light_sets:g.light_sets};
    *cache=Some(globals);Ok(globals)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw(
    device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout,
    boot: &uyafmt::boot::Boot, mo: &mut crate::mobies::Mobies, ram: &[u8], jobs: &crate::jobs::FrameJobs,
    globals: &crate::mobies::MobyGlobals, shrub_globals: &crate::objects::ShrubGlobals, original_cam: &Camera, fog: &uyafmt::gameplay::Fog,
    pipes: &crate::objects::Pipelines, tv: &wgpu::TextureView, dv: &wgpu::TextureView, render_target: &wgpu::Texture,
    depth: &wgpu::Texture, ao: &crate::ao::Ao,
    view: &wgpu::Buffer, view_bg: &wgpu::BindGroup, size: (u32,u32),
) -> Result<(), String> {
    let options = OPTIONS.lock().unwrap().clone().ok_or("no review request")?;
    RUNS.lock().unwrap().clear();
    let walk = crate::mobies::walk(jobs, boot, true, None)?;
    let (mut vertices, mut prev, mut lit, mut draws, mut targets) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let skipped;
    if let Some(number)=options.edited.strip_prefix("@shrub:") {
        let instance=number.split('|').next().unwrap().parse().map_err(|_|"shrub instance")?;
        let guard=SOURCE_SCENE.lock().unwrap();let scene=guard.as_ref().ok_or("No source shrubs")?;
        let sets=lighting::sets_from_ram(ram,globals.light_sets)?;
        let (v,l,ds)=scene.review_shrub(instance,ram,shrub_globals,&sets)?;
        vertices=v;lit=l;prev=vertices.iter().map(|v|v.pos).collect();
        let set=(0xfffd0000,0);
        if !mo.materials.contains_key(&set) {mo.materials.insert(set,scene.review_shrub_materials(device,queue)?);}
        for dr in ds {mo.materials.get_mut(&set).unwrap().get(device,layout,dr.key)?;draws.push((set,dr,crate::eedraw::MAIN));}
        record_run(0,vertices.len(),vertices[0].pos);skipped=Vec::<(String,usize)>::new();
    } else if let Some(number)=options.edited.strip_prefix("@tie:") {
        let instance=number.split('|').next().unwrap().parse().map_err(|_|"tie instance")?;
        let guard=SOURCE_SCENE.lock().unwrap();let scene=guard.as_ref().ok_or("No source ties")?;
        let sets=lighting::sets_from_ram(ram,globals.light_sets)?;
        let (v,l,ds)=scene.review_tie(instance,ram,&tie_globals()?,&sets)?;
        vertices=v;lit=l;prev=vertices.iter().map(|v|v.pos).collect();
        let set=(0xfffd0001,0);
        if !mo.materials.contains_key(&set) {mo.materials.insert(set,scene.review_tie_materials(device,queue)?);}
        for dr in ds {mo.materials.get_mut(&set).unwrap().get(device,layout,dr.key)?;draws.push((set,dr,crate::eedraw::MAIN));}
        record_run(0,vertices.len(),vertices[0].pos);skipped=Vec::<(String,usize)>::new();
    } else if let Some(number)=options.edited.strip_prefix("@terrain:") {
        let chunk=number.split('|').next().unwrap().parse().map_err(|_|"terrain chunk")?;
        let guard=SOURCE_SCENE.lock().unwrap();let scene=guard.as_ref().ok_or("No source terrain")?;
        let sets=lighting::sets_from_ram(ram,globals.light_sets)?;
        let (v,l,ds)=scene.review_terrain(chunk,&sets)?;
        vertices=v;lit=l;prev=vertices.iter().map(|v|v.pos).collect();
        let set=(0xfffe0000,0);
        if !mo.materials.contains_key(&set) {mo.materials.insert(set,scene.review_terrain_materials(device,queue)?);}
        for dr in ds {mo.materials.get_mut(&set).unwrap().get(device,layout,dr.key)?;draws.push((set,dr,crate::eedraw::MAIN));}
        record_run(0,vertices.len(),vertices[0].pos);skipped=Vec::<(String,usize)>::new();
    } else if let Some(number)=options.edited.strip_prefix("@body:") {
            let selection=number.split('|').next().unwrap();
            let assembly=selection.ends_with(",ship");
            let variant=selection.split(',').next().unwrap().parse::<usize>().map_err(|_|"ship variant")?;
            if variant>2 {return Err("Ship variant must be 0, 1 or 2".into());}
            let mut guard=SOURCE_BODY.lock().unwrap();
            if guard.as_ref().is_none_or(|(v,_)|*v!=variant) {
                let iso=std::env::args().nth(1).ok_or("Missing disc path")?;
                let mut disc=uyafmt::disc::Disc::open(&iso)?;let toc=uyafmt::toc::read(&mut disc)?;
                *guard=Some((variant,uyafmt::scenecode::hero_body(&mut disc,&toc,&globals.scene,variant,0)?));
            }
            let body=&guard.as_ref().unwrap().1;
            let sets=lighting::sets_from_ram(ram,globals.light_sets)?;
            let reference=SOURCE_SCENE.lock().unwrap().as_ref().ok_or("No source lighting")?.review_source_light(original_cam.eye)?;
            let (mut v,mut l,mut ds)=source_mesh(&body.class,&sets,reference,|local|body.image(&globals.scene,local).map(|_|0))?;
            if assembly {
                // Original class-header scales and mirrored instances, verified
                // against the saved L01 field 1001 RAM; see ship-assembly.json.
                // Canonical bind assembly excludes transient projectiles.
                let core_guard=SOURCE_MOBIES.lock().unwrap();let core=core_guard.as_ref().ok_or("No ship kit classes")?;
                let hull_scale=f32::from_le_bytes(body.bytes[0x24..0x28].try_into().unwrap());
                if !hull_scale.is_finite() || hull_scale<=0.0 {return Err("Invalid ship hull scale".into());}
                // Variants use their own original wing class/header scale.
                // The additional scales are recorded by verify_ship_assembly.py.
                let wing_id=4308+variant as u32;
                let wing_scale=[0.14575443,0.14575443,0.14788912][variant];
                let mut parts=vec![format!("{{\"name\":\"Hull and cockpit\",\"first\":0,\"end\":{},\"scale\":{hull_scale}}}",v.len()/3)];
                for (name,id,scale,mirror) in [("Port wing",wing_id,wing_scale,false),("Starboard wing",wing_id,wing_scale,true),("Twin engine assembly",4312,0.116835035,false),("Port weapon",4314,0.20566118,false),("Starboard weapon",4314,0.20566118,true)] {
                    let slot=core.moby_classes.iter().position(|c|c.id==id).ok_or("Missing original ship kit part")?;
                    let class=core.mobys[slot].as_ref().ok_or("Empty ship kit class")?;
                    let record=&core.moby_classes[slot];
                    let (pv,pl,pd)=source_mesh_transform(class,&sets,reference,|local| {
                        if local>=0 && record.mode==1 && record.textures.get(local as usize)==Some(&255) {Ok(0)} else {Err("Unverified ship kit material".into())}
                    },scale/hull_scale,mirror)?;
                    let first=v.len() as u32;v.extend(pv);l.extend(pl);
                    parts.push(format!("{{\"name\":\"{name}\",\"class_id\":{id},\"first\":{},\"end\":{},\"scale\":{scale},\"mirror_y\":{mirror}}}",first/3,v.len()/3));
                    for mut d in pd {d.first+=first;ds.push(d);}
                }
                std::fs::write(Path::new(&options.output).with_extension("assembly.json"),format!("{{\"variant\":{variant},\"parts\":[{}]}}",parts.join(","))).map_err(|e|e.to_string())?;
            }
            vertices=v;lit=l;prev=vertices.iter().map(|v|v.pos).collect();
            let set=(0xfffa0000+variant as u32,0);
            if !mo.materials.contains_key(&set) {mo.materials.insert(set,crate::objects::Materials::new(device,queue,std::slice::from_ref(&body.image),5.0,0x60)?);}
            for dr in ds {mo.materials.get_mut(&set).unwrap().get(device,layout,dr.key)?;draws.push((set,dr,crate::eedraw::MAIN));}
            RUNS.lock().unwrap().clear();record_run(0,vertices.len(),[0.0;3]);skipped=Vec::new();
    } else if let Some(number)=options.edited.strip_prefix("@source:") {
            let slot=number.split('|').next().unwrap().parse().map_err(|_|"source class slot")?;
            let guard=SOURCE_MOBIES.lock().unwrap();let core=guard.as_ref().ok_or("No source classes")?;
            let sets=lighting::sets_from_ram(ram,globals.light_sets)?;
            let reference=SOURCE_SCENE.lock().unwrap().as_ref().ok_or("No source lighting")?.review_source_light(original_cam.eye)?;
            let (v,l,ds)=source_class(core,slot,&sets,reference)?;
            vertices=v;lit=l;prev=vertices.iter().map(|v|v.pos).collect();draws.clear();
            let set=(0xfffc0000,0);
            if !mo.materials.contains_key(&set) {mo.materials.insert(set,crate::objects::Materials::new(device,queue,&core.moby_textures,5.0,0x60)?);}
            let own=(0xfffb0000+core.moby_classes[slot].mode as u32,0);
            if ds.iter().any(|dr|dr.key.0==u32::MAX) && !mo.materials.contains_key(&own) {
                let skin=if core.moby_classes[slot].mode==1 {globals.ship}else{globals.clank};
                let iso=std::env::args().nth(1).ok_or("Missing disc path")?;
                let image=uyafmt::moby::wad_image(&iso,skin.wad,skin.pair as usize)?;
                mo.materials.insert(own,crate::objects::Materials::new(device,queue,&[image],5.0,0x60)?);
            }
            for mut dr in ds {
                let draw_set=if dr.key.0==u32::MAX {dr.key.0=0;own} else if dr.key.0>=core.moby_textures.len() as u32 {(0,0)}else{set};
                mo.materials.get_mut(&draw_set).unwrap().get(device,layout,dr.key)?;draws.push((draw_set,dr,crate::eedraw::MAIN));
            }
            RUNS.lock().unwrap().clear();record_run(0,vertices.len(),[0.0;3]);skipped=Vec::new();
    } else if let Some(number)=options.edited.strip_prefix("@scene:") {
            // A cutscene SCENE class, drawn like "@source:": its own images, the core's special textures for -1..-3.
            // code = costume * 100000 + record * 1000 + entry; the costume number is used by scene class 10 only.
            let code:usize=number.split('|').next().unwrap().parse().map_err(|_|"scene class code")?;
            let (costume,record,entry)=(code/100000,code%100000/1000,code%1000);
            let bguard=SOURCE_SCENE_BLOCKS.lock().unwrap();let blocks=bguard.as_ref().ok_or("No scene blocks")?;
            let sc=blocks.get(record).and_then(|b|b.as_ref()).and_then(|b|b.block.classes.get(entry)).ok_or("Scene class absent")?;
            let guard=SOURCE_MOBIES.lock().unwrap();let core=guard.as_ref().ok_or("No source classes")?;
            if sc.images.len()>=core.moby_textures.len() {return Err("Scene class has more images than the core has moby textures".into());}
            // Scene class 10 has no images of its own: the game binds WAD 5's image `class10_texture` (a RAM variable) to
            // slot 0 through the body remap (mobies.rs, Src::Scene); `costume` stands for that variable here.
            let class10_images;
            let images:&[uyafmt::core::Texture]=if sc.id==10 {
                let iso=std::env::args().nth(1).ok_or("Missing disc path")?;
                let mut disc=uyafmt::disc::Disc::open(&iso)?;let toc=uyafmt::toc::read(&mut disc)?;
                class10_images=vec![uyafmt::scenecode::class10_image(&mut disc,&toc,&globals.scene,costume)?];&class10_images
            } else {&sc.images};
            let sets=lighting::sets_from_ram(ram,globals.light_sets)?;
            let reference=SOURCE_SCENE.lock().unwrap().as_ref().ok_or("No source lighting")?.review_source_light(original_cam.eye)?;
            let (v,l,ds)=source_mesh(&sc.class,&sets,reference,|local| {
                if (-3..=-1).contains(&local) {return Ok(core.moby_textures.len() as u32 + if local == -1 {2} else {(-2-local) as u32});}
                if sc.id==10 {return Ok(0);}
                sc.image(local).map(|_|local as u32)
            })?;
            vertices=v;lit=l;prev=vertices.iter().map(|v|v.pos).collect();draws.clear();
            let set=(0xfff90000+(code%100000) as u32,u8::try_from(costume).map_err(|_|"costume number")?);
            if !mo.materials.contains_key(&set) {mo.materials.insert(set,crate::objects::Materials::new(device,queue,images,5.0,0x60)?);}
            for dr in ds {
                let draw_set=if dr.key.0>=core.moby_textures.len() as u32 {(0,0)} else {set};
                mo.materials.get_mut(&draw_set).unwrap().get(device,layout,dr.key)?;draws.push((draw_set,dr,crate::eedraw::MAIN));
            }
            RUNS.lock().unwrap().clear();record_run(0,vertices.len(),[0.0;3]);skipped=Vec::new();
    } else {
        let (_, errors, _) = mo.frame(device, queue, layout, ram, walk, globals, original_cam, jobs.field,
            &mut vertices, &mut prev, &mut lit, &jobs.skin, false, &mut draws, &mut targets, &mut None)?;
        skipped=errors;
    }
    // "*" selects every draw: for source classes whose only setups are the special -1 set, which has no texture name.
    let select_all = options.prefixes.iter().any(|p| p == "*");
    let matches = |set: &(u32,u8), dr: &Draw| select_all || mo.materials[set].names.get(dr.key.0 as usize).copied().flatten()
        .is_some_and(|n| options.prefixes.iter().any(|p| n.stem().starts_with(p)));
    let mut selected = vec![false; vertices.len()];
    let mut names = std::collections::BTreeSet::new();
    for (set, dr, fb) in &draws {
        if *fb == crate::eedraw::MAIN && matches(set, dr) {
            selected[dr.first as usize..(dr.first+dr.count) as usize].fill(true);
            if let Some(n) = mo.materials[set].names.get(dr.key.0 as usize).copied().flatten() { names.insert(n.stem()); }
        }
    }
    if !selected.iter().any(|v| *v) {
        let all = draws.iter().filter_map(|(set,dr,_)| mo.materials[set].names.get(dr.key.0 as usize).copied().flatten().map(|n| n.stem())).collect::<std::collections::BTreeSet<_>>();
        return Err(format!("no matching model; available {all:?}; skipped {skipped:?}"));
    }
    let runs = RUNS.lock().unwrap();
    let mut origins: Vec<[f32;3]> = Vec::new();
    for &(first,end,p) in runs.iter().filter(|(a,b,_)| selected[*a..*b].iter().any(|v|*v)) {
        let _ = (first,end);
        if origins.iter().all(|q| distance(p,*q)>0.04) { origins.push(p); }
    }
    origins.sort_by(|a,b| distance(*a,original_cam.eye).total_cmp(&distance(*b,original_cam.eye)));
    if options.channel == 0 { eprintln!("review origins: {:?}",origins); }
    // -1 includes attachments: the hero's body and helmet have distinct origins.
    // Catalog entries use this only for the unique player character in the capture.
    if options.instance >= 0 {
        let origin = *origins.get(options.instance as usize).ok_or_else(||format!("instance {} of {}",options.instance,origins.len()))?;
        // Multi-part actors use an explicit, reviewed spatial association from
        // the asset manifest. Other assets retain exact origin selection.
        let same_actor = |p:[f32;3]| distance(p,origin)<=0.04 || (options.pair[0]>0.0 &&
            (p[0]-origin[0]).powi(2)+(p[1]-origin[1]).powi(2)<options.pair[0].powi(2) &&
            (p[2]-origin[2]).abs()<options.pair[1]);
        for &(first,end,p) in runs.iter() {
            if !same_actor(p) { selected[first..end].fill(false); }
            else if options.whole { selected[first..end].fill(true); }
        }
    }
    drop(runs);
    if !options.emission_policy.is_empty() {
        let data=std::fs::read(&options.emission_policy).map_err(|e|e.to_string())?;
        let source:Vec<_>=selected.iter().enumerate().filter_map(|(i,s)|s.then_some(i)).collect();
        if data.len()!=12+source.len()/3 || data.get(..8)!=Some(b"R3EMIT01") || source.len()%3!=0 {
            return Err("Emission policy does not match this source mesh/LOD".into());
        }
        let count=u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
        if count!=source.len() {return Err("Emission policy source vertex count changed".into());}
        for (triangle,ids) in source.chunks_exact(3).enumerate() {
            let flags=match data[12+triangle] {0=>1u32<<19,1=>0,2=>1u32<<20,_=>return Err("Unknown emission face policy".into())};
            for &i in ids {lit[i].light=(lit[i].light & !((1u32<<19)|(1u32<<20)))|flags;}
        }
    }
    let edit_path=if options.edited.starts_with("@terrain:") || options.edited.starts_with("@shrub:") || options.edited.starts_with("@tie:") || options.edited.starts_with("@source:") || options.edited.starts_with("@body:") || options.edited.starts_with("@scene:") {options.edited.split_once('|').map(|p|p.1)}
        else if options.edited=="@near" || options.edited=="@family" || options.edited=="@near:family" || options.edited.is_empty(){None}
        else {Some(options.edited.strip_prefix("@near:").unwrap_or(&options.edited))};
    if let Some(path)=edit_path {
        let mut source_materials=vec![String::new();vertices.len()];
        for (set,dr,_) in &draws {
            let name=mo.materials[set].names[dr.key.0 as usize].map(|n|n.stem()).unwrap_or_else(||"untextured".into());
            source_materials[dr.first as usize..(dr.first+dr.count) as usize].fill(name);
        }
        let edited=crate::mesh_edit::deform(Path::new(path),&vertices,&lit,&selected,&source_materials)?;
        let mut keys=Vec::new();
        for name in &edited.materials {
            let key=draws.iter().find(|(set,dr,fb)|*fb==crate::eedraw::MAIN && selected[dr.first as usize..(dr.first+dr.count) as usize].iter().any(|v|*v) &&
                mo.materials[set].names[dr.key.0 as usize].map(|n|n.stem()).unwrap_or_else(||"untextured".into())==*name)
                .map(|(set,dr,_)|(*set,dr.key)).ok_or_else(||format!("edited mesh material {name} is absent from this capture"))?;
            keys.push(key);
        }
        draws.clear();
        for (triangle,&material) in edited.triangle_materials.iter().enumerate() {
            let (set,key)=keys[material];
            if let Some((previous,draw,_))=draws.last_mut().filter(|(previous,draw,_)|*previous==set && draw.key==key) {
                let _=previous;draw.count+=3;
            } else { draws.push((set,Draw{key,first:(triangle*3) as u32,count:3},crate::eedraw::MAIN)); }
        }
        vertices=edited.vertices;lit=edited.lighting;prev=vertices.iter().map(|v|v.pos).collect();selected=vec![true;vertices.len()];
    }
    if options.edited=="@family" || options.edited=="@near:family" {
        family_geometry::refine(&mut vertices,&mut lit,&mut draws,&mut selected)?;
        prev=vertices.iter().map(|v|v.pos).collect();
    }
    names.clear();
    for (set,dr,fb) in &draws {
        if *fb != crate::eedraw::MAIN { selected[dr.first as usize..(dr.first+dr.count) as usize].fill(false); }
        else if selected[dr.first as usize..(dr.first+dr.count) as usize].iter().any(|v|*v) {
            if let Some(name)=mo.materials[set].names[dr.key.0 as usize] { names.insert(name.stem()); }
        }
    }
    let mut bounds = [[f32::INFINITY;3],[f32::NEG_INFINITY;3]];
    let mut count = 0;
    let mut normals = 0;
    for (i,v) in vertices.iter().enumerate().filter(|(i,_)| selected[*i]) {
        for k in 0..3 { bounds[0][k]=bounds[0][k].min(v.pos[k]); bounds[1][k]=bounds[1][k].max(v.pos[k]); }
        count += 1;
        normals += (lit[i].light & lighting::LIT != 0) as usize;
    }
    if normals == 0 { return Err("selected model has no verified skinned normals; cannot judge its materials".into()); }
    let center: [f32;3] = std::array::from_fn(|k|(bounds[0][k]+bounds[1][k])*0.5);
    if options.channel==0 && std::env::var_os("UYA_REVIEW_EXPORT_OBJ").is_some() {
        let path=Path::new(&options.output).with_extension("obj");
        let mut out=std::io::BufWriter::new(std::fs::File::create(&path).map_err(|e|e.to_string())?);
        let mut ids=vec![0;vertices.len()];
        let mut next=0;
        let mut vertex_lighting=Vec::new();
        writeln!(out,"# UYA {}; Z up; UV and decoded skin normals preserved.\n# world center {} {} {}",if options.edited.starts_with("@source:") || options.edited.starts_with("@body:"){ "original source bind-space mesh (not an animated capture)" }else{ "captured pose" },center[0],center[1],center[2]).map_err(|e|e.to_string())?;
        for (i,v) in vertices.iter().enumerate().filter(|(i,_)|selected[*i]) {
            next+=1;ids[i]=next;
            vertex_lighting.extend_from_slice(bytemuck::bytes_of(&lit[i]));
            vertex_lighting.extend_from_slice(&v.colour);
            writeln!(out,"v {} {} {}\nvt {} {}\nvn {} {} {}",v.pos[0]-center[0],v.pos[1]-center[1],v.pos[2]-center[2],v.st[0]/v.st[2],1.0-v.st[1]/v.st[2],lit[i].n[0] as f32/127.0,lit[i].n[1] as f32/127.0,lit[i].n[2] as f32/127.0).map_err(|e|e.to_string())?;
        }
        let mut light_words=std::collections::BTreeMap::new();
        for (set,dr,_) in &draws {
            let name=mo.materials[set].names[dr.key.0 as usize].map(|n|n.stem()).unwrap_or_else(||"untextured".into());
            if !selected[dr.first as usize..(dr.first+dr.count) as usize].iter().any(|v|*v) {continue;}
            writeln!(out,"g {name}\nusemtl {name}").map_err(|e|e.to_string())?;
            for i in (dr.first as usize..(dr.first+dr.count) as usize).step_by(3) {
                if selected[i] { let a=ids[i];let b=ids[i+1];let c=ids[i+2];
                    writeln!(out,"f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}").map_err(|e|e.to_string())?;
                    *light_words.entry(lit[i].light).or_insert(0usize)+=1;
                }
            }
        }
        out.flush().map_err(|e|e.to_string())?;
        let at=globals.light_sets as usize;
        std::fs::write(path.with_extension("lights.bin"),&ram[at..at+0x400]).map_err(|e|e.to_string())?;
        std::fs::write(path.with_extension("lights.txt"),format!("{light_words:?}")).map_err(|e|e.to_string())?;
        std::fs::write(path.with_extension("vertex-lighting.bin"),vertex_lighting).map_err(|e|e.to_string())?;
    }
    let radius = distance(bounds[0],bounds[1]).sqrt()*0.5;
    let tan_y = 0.32;
    let dist = radius/tan_y*1.12/options.zoom;
    let (sy,cy) = options.yaw.sin_cos();
    let (sp,cp) = options.pitch.sin_cos();
    let outward = [cy*cp,sy*cp,sp];
    let right = [-sy,cy,0.0];
    let up = [-cy*sp,-sy*sp,cp];
    let target: [f32;3] = std::array::from_fn(|k|center[k]+radius*(right[k]*options.pan_x+up[k]*options.pan_y));
    let cam = Camera { eye: std::array::from_fn(|k|target[k]+outward[k]*dist), forward: outward.map(|v|-v), up,
        tan: [tan_y*size.0 as f32/size.1 as f32,tan_y], k: original_cam.k };
    // Retain the captured game's vertex colours, normal shares, light words and
    // light terms. Native::render has already uploaded all 16 real level light
    // sets from this frame's RAM. Only distance fog is removed for inspection.
    for i in 0..vertices.len() {
        if !selected[i] { continue; }
        vertices[i].f = 256.0;
    }
    if options.channel==6 {
        for (i,v) in vertices.iter_mut().enumerate() {let id=i as u32/3+1;v.colour=[id as u8,(id>>8) as u8,(id>>16) as u8,128];}
        let mut info=String::from("first_triangle\tend_triangle\tmaterial\n");
        for (set,dr,_) in &draws {info+=&format!("{}\t{}\t{}\n",dr.first/3+1,(dr.first+dr.count)/3+1,mo.materials[set].names[dr.key.0 as usize].map(|n|n.stem()).unwrap_or_else(||"untextured".into()));}
        std::fs::write(Path::new(&options.output).with_extension("triangles.tsv"),info).map_err(|e|e.to_string())?;
    }
    queue.write_buffer(view,0,bytemuck::bytes_of(&crate::native::view_u(&cam,fog,0.0)));
    let buffer = |label,bytes|device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label:Some(label), contents:bytes, usage:wgpu::BufferUsages::VERTEX });
    let vb=buffer("review vertices",bytemuck::cast_slice::<ObjVertex,u8>(&vertices));
    let pb=buffer("review previous",bytemuck::cast_slice::<[f32;3],u8>(&prev));
    let lb=buffer("review real skin normals",bytemuck::cast_slice::<lighting::MobyLit,u8>(&lit));
    let mut encoder=device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label:Some("model review"),..Default::default() });
    {
        let mut pass=encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label:Some("black room"), color_attachments:&[Some(wgpu::RenderPassColorAttachment {
                view:tv,depth_slice:None,resolve_target:None,ops:wgpu::Operations { load:wgpu::LoadOp::Clear(wgpu::Color::BLACK),store:wgpu::StoreOp::Store } })],
            depth_stencil_attachment:Some(wgpu::RenderPassDepthStencilAttachment { view:dv,
                depth_ops:Some(wgpu::Operations { load:wgpu::LoadOp::Clear(0.0),store:wgpu::StoreOp::Store }),
                stencil_ops:Some(wgpu::Operations { load:wgpu::LoadOp::Clear(0),store:wgpu::StoreOp::Store }) }),
            ..Default::default() });
        pass.set_bind_group(0,view_bg,&[]);
        pass.set_vertex_buffer(0,vb.slice(..)); pass.set_vertex_buffer(1,pb.slice(..)); pass.set_vertex_buffer(2,lb.slice(..));
        for (set,dr,_) in &draws {
            let mut pieces=Vec::new();
            let mut i=dr.first as usize;
            let end=(dr.first+dr.count) as usize;
            while i<end {
                if !selected[i] { i+=3; continue; }
                let start=i;
                while i<end && selected[i] { i+=3; }
                pieces.push(Draw { key:dr.key,first:start as u32,count:(i-start) as u32 });
            }
            if !pieces.is_empty() { pipes.draw_moby(&mut pass,&mo.materials[set],&pieces)?; }
        }
    }
    let emission_source=if options.channel==0 || options.channel==8 {Some(crate::review_emission::copy_frame(device,&mut encoder,render_target,size))} else {None};
    if options.channel == 0 || options.channel == 5 || options.channel == 8 {
        // The same depth-based AO as gameplay, applied once to the selected mesh.
        // No painted-in lighting or inferred cavities from the colour texture.
        let no_fog=uyafmt::gameplay::Fog { colour:[0;3], near:0.0, far:crate::native::FAR, near_f:255.0, far_f:255.0 };
        let mask=crate::ao::mask(device,size);
        ao.begin(device,&mut encoder,depth,&mask,tv,size,&cam,&no_fog,(crate::native::NEAR,crate::native::FAR),2);
    }
    if let Some(source)=emission_source {crate::review_emission::glow(device,&mut encoder,render_target,tv,&source,size);}
    queue.submit([encoder.finish()]);
    eprintln!("review: {} vertices; {}/{} valid normals; instance {}/{}; bounds {:?}; textures {:?}",count,normals,count,options.instance,origins.len(),bounds,names);
    Ok(())
}
