//! catalogue_export DISC.iso OUT_DIR TRIG.bin
//!
//! Offline source census for the NPC/enemy catalogue. Reads every story level's LEVEL core (all moby class slots) and every
//! SCENE record's block (the cutscene-only classes, `uyafmt::scene::load`) straight from the disc. No game execution, no RAM
//! dumps, no renderer: geometry is the class's own bind-space mesh, skinned through identity joint matrices with the exact
//! original (cos, sin) normal table (captures/rift-texture-review/source-skin-trig.bin, normal decode revision 2).
//!
//! Writes OUT_DIR/inventory.tsv (one row per LEVEL slot and per SCENE class), OUT_DIR/obj/<fingerprint>.obj (+ .far.obj) for
//! every fingerprint worth identifying (joints >= 2 or >= 300 near triangles, and every SCENE class with a mesh), and the
//! textures those meshes bind as OUT_DIR/tex/<stem>.png (PCSX2 dump names; alpha rescaled from the GS 0..0x80 range).
//! LEVEL fingerprints use exactly the hash of review_inventory.inc.rs, so rows join captures/.../occurrences.tsv.

mod texhash;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use uyafmt::core::Texture;
use uyafmt::moby::{JobLights, MobyClass, PacketB, RigidInputs, SkinInputs, SkinState};

/// How a texture setup's local number resolves for one class.
enum Tex<'a> {
    Image(&'a Texture),
    Runtime(String),
}

fn fingerprint(class: &MobyClass, names: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    let mut feed = |value: &str| {
        for byte in value.bytes() {
            hash = (hash ^ byte as u64).wrapping_mul(0x100000001b3);
        }
    };
    let h = &class.header;
    feed(&format!("{}/{}/{}/{}/{}/{:?}/{:?}/{names}", h.meshes_near, h.meshes_far, h.meshes_third, h.joints, h.joints_far, class.joints, class.bind));
    for mesh in &class.meshes {
        feed(&format!("{:?}{:?}", mesh.packet_a, mesh.packet_b));
    }
    hash
}

fn count(class: &MobyClass, start: usize, n: usize) -> usize {
    class.meshes.iter().skip(start).take(n).filter_map(|m| m.packet_a.as_ref())
        .flat_map(|p| &p.strips).map(|s| s.vertices.iter().skip(2).filter(|v| !v.skip).count()).sum()
}

fn locals(class: &MobyClass) -> BTreeSet<i32> {
    class.meshes.iter().filter_map(|m| m.packet_a.as_ref()).flat_map(|p| &p.textures).map(|t| t.local).collect()
}

fn stem(t: &Texture) -> Result<Option<String>, String> {
    Ok(texhash::name(t)?.map(|n| n.stem()))
}

fn flat(e: &str) -> String {
    e.replace(['\n', '\t'], " ")
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (n, e) in table.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB88320 ^ (c >> 1) } else { c >> 1 };
        }
        *e = c;
    }
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = table[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

/// RGBA8 PNG with stored (uncompressed) deflate blocks: no dependencies, exact pixels.
fn png(w: usize, h: usize, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(h * (1 + 4 * w));
    for y in 0..h {
        raw.push(0);
        raw.extend_from_slice(&rgba[y * 4 * w..(y + 1) * 4 * w]);
    }
    let mut z = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, c) in chunks.iter().enumerate() {
        z.push(if i + 1 == chunks.len() { 1 } else { 0 });
        z.extend_from_slice(&(c.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(c.len() as u16)).to_le_bytes());
        z.extend_from_slice(c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &v in &raw {
        a = (a + v as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut chunk = |kind: &[u8], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}

fn write_texture(dir: &Path, t: &Texture, name: &str) -> Result<(), String> {
    let path = dir.join(format!("{name}.png"));
    if path.exists() {
        return Ok(());
    }
    let level = t.levels.first().ok_or("texture without level 0")?;
    let rgba: Vec<u8> = level.iter().flat_map(|p| [p[0], p[1], p[2], (p[3] as u16 * 255 / 128).min(255) as u8]).collect();
    std::fs::write(&path, png(t.width, t.height, &rgba)).map_err(|e| format!("{}: {e}", path.display()))
}

struct MeshStats {
    vertices: usize,
    triangles: usize,
    bbox: [[f32; 3]; 2],
    env_uv_meshes: usize,
    no_uv_meshes: usize,
    errors: Vec<String>,
}

/// One LOD run as an OBJ: identity joints expose the bind-space mesh (review_source.inc.rs does the same for the native views).
fn export_obj(class: &MobyClass, first: usize, n: usize, trig: &[[f32; 2]], material: &dyn Fn(i32) -> String, header: &str, path: &Path) -> Result<MeshStats, String> {
    let identity: [[f32; 4]; 4] = std::array::from_fn(|r| std::array::from_fn(|c| if r == c { 1.0 } else { 0.0 }));
    let row = |i: usize| -> [f32; 4] { std::array::from_fn(|c| if c == i { 1.0 } else { 0.0 }) };
    let input = SkinInputs {
        joints: vec![identity; 112],
        lights: JobLights { dirs: [[0.0; 4]; 3], colours: [[0.0; 4]; 3], wrap: [0.0; 3], flat: [128; 4], base: [65537.0; 4] },
        rigid: Some(RigidInputs { view: [row(0), row(1), row(2)], alpha: 0x80 }),
        trig: trig.to_vec(),
    };
    let mut state = SkinState::new();
    let mut obj = String::new();
    let _ = writeln!(obj, "{header}");
    let _ = writeln!(obj, "mtllib {}", path.file_name().unwrap().to_string_lossy().replace(".obj", ".mtl"));
    let mut stats = MeshStats { vertices: 0, triangles: 0, bbox: [[f32::MAX; 3], [f32::MIN; 3]], env_uv_meshes: 0, no_uv_meshes: 0, errors: vec![] };
    let mut used = BTreeSet::new();
    let mut base = 1usize;
    let mut current: Option<String> = None;
    for (m, mesh) in class.meshes.iter().enumerate().skip(first).take(n) {
        let Some(a) = mesh.packet_a.as_ref() else { continue };
        let sk = match uyafmt::moby::skin_fast(mesh, &input, &mut state) {
            Ok(s) => s,
            Err(e) => {
                stats.errors.push(format!("mesh {m}: {e}"));
                continue;
            }
        };
        let rigid = matches!(mesh.packet_b, Some(PacketB::Rigid(_)));
        let uvs: Vec<[f32; 2]> = if let Some(u) = &a.uvs {
            u.clone()
        } else if let Some(u) = &sk.uvs {
            stats.env_uv_meshes += 1;
            u.iter().map(|v| [v[0] as f32 / 4096.0, v[1] as f32 / 4096.0]).collect()
        } else {
            stats.no_uv_meshes += 1;
            vec![[0.0; 2]; sk.positions.len()]
        };
        let _ = writeln!(obj, "o mesh{m:02}{}", if rigid { "_rigid" } else { "" });
        for (i, p) in sk.positions.iter().enumerate() {
            let p = p.map(|v| v as f32 / 1024.0);
            for k in 0..3 {
                stats.bbox[0][k] = stats.bbox[0][k].min(p[k]);
                stats.bbox[1][k] = stats.bbox[1][k].max(p[k]);
            }
            let mut nrm = sk.normals.get(i).copied().unwrap_or([0.0, 0.0, 1.0]); // outward as skin_fast gives them (checked at the extremities of classes 87, 6501, 7066)
            let len = nrm.iter().map(|x| x * x).sum::<f32>().sqrt();
            if len.is_finite() && len > 1e-6 {
                nrm = nrm.map(|v| v / len);
            } else {
                nrm = [0.0, 0.0, 1.0];
            }
            let uv = uvs.get(i).copied().unwrap_or([0.0; 2]);
            let _ = writeln!(obj, "v {} {} {}\nvt {} {}\nvn {} {} {}", p[0], p[1], p[2], uv[0], 1.0 - uv[1], nrm[0], nrm[1], nrm[2]);
        }
        for strip in &a.strips {
            if let Some(t) = strip.texture {
                let name = material(a.textures[t].local);
                if current.as_deref() != Some(name.as_str()) {
                    let _ = writeln!(obj, "usemtl {name}");
                    used.insert(name.clone());
                    current = Some(name);
                }
            }
            if current.is_none() {
                let _ = writeln!(obj, "usemtl untextured");
                used.insert("untextured".to_string());
                current = Some("untextured".into());
            }
            for k in 2..strip.vertices.len() {
                if strip.vertices[k].skip {
                    continue;
                }
                let c = if k % 2 == 0 { [k - 2, k - 1, k] } else { [k - 1, k - 2, k] };
                let idx = c.map(|i| strip.vertices[i].index as usize + base);
                let _ = writeln!(obj, "f {0}/{0}/{0} {1}/{1}/{1} {2}/{2}/{2}", idx[0], idx[1], idx[2]);
                stats.triangles += 1;
            }
        }
        base += sk.positions.len();
        stats.vertices += sk.positions.len();
    }
    if stats.triangles == 0 {
        return Ok(stats);
    }
    std::fs::write(path, obj).map_err(|e| e.to_string())?;
    let mut mtl = String::new();
    for name in used {
        let _ = writeln!(mtl, "newmtl {name}\nKd 1 1 1");
        if let Some(stem) = name.strip_prefix("tex_") {
            let _ = writeln!(mtl, "map_Kd ../tex/{stem}.png");
        }
    }
    std::fs::write(path.with_extension("mtl"), mtl).map_err(|e| e.to_string())?;
    Ok(stats)
}

struct Row<'c> {
    source: &'static str,
    level: usize,
    record: String,
    slot: usize,
    class_id: u32,
    mode: String,
    fp: u64,
    class: &'c MobyClass,
    names: String,
}

fn process<'t>(row: &Row, lookup: &dyn Fn(i32) -> Option<Tex<'t>>, out: &Path, trig: &[[f32; 2]], exported: &mut BTreeMap<u64, String>, table: &mut String) -> Result<(), String> {
            let c = row.class;
            let h = &c.header;
            let (near, far, third) = (count(c, 0, h.meshes_near as usize), count(c, h.meshes_near as usize, h.meshes_far as usize),
                count(c, h.meshes_near as usize + h.meshes_far as usize, h.meshes_third as usize));
            let skinned = c.meshes.iter().filter(|m| matches!(m.packet_b, Some(PacketB::Skinned(_)))).count();
            let rigid = c.meshes.iter().filter(|m| matches!(m.packet_b, Some(PacketB::Rigid(_)))).count();
            let joints = h.joints.max(h.joints_far);
            let wanted = (joints >= 2 || near >= 300 || row.source == "SCENE") && (near > 0 || far > 0);
            let key = format!("{:016x}", row.fp);
            let mut cells = (String::new(), 0usize, String::new(), String::new(), 0usize, 0usize, String::new());
            if wanted {
                let tex_dir = out.join("tex");
                let material = |local: i32| -> String {
                    match lookup(local) {
                        Some(Tex::Image(t)) => match stem(t) {
                            Ok(Some(s)) => {
                                let _ = write_texture(&tex_dir, t, &s);
                                format!("tex_{s}")
                            }
                            _ => "untextured".into(),
                        },
                        Some(Tex::Runtime(r)) => r,
                        None => "untextured".into(),
                    }
                };
                let path = out.join("obj").join(format!("{key}.obj"));
                let header = format!("# UYA {} level {} record {} slot {} class {}; bind-space via identity joints; normal decode revision 2 (exact original trig table); Z up; 1 unit = 1024 source units",
                    row.source, row.level, row.record, row.slot, row.class_id);
                let (first, n) = if near > 0 { (0, h.meshes_near as usize) } else { (h.meshes_near as usize, h.meshes_far as usize) };
                if let Some(previous) = exported.get(&row.fp) {
                    cells.0 = previous.clone();
                } else {
                    let s = export_obj(c, first, n, trig, &material, &header, &path)?;
                    if near > 0 && far > 0 {
                        let _ = export_obj(c, h.meshes_near as usize, h.meshes_far as usize, trig, &material, &header, &path.with_extension("far.obj"));
                    }
                    let rel = if s.triangles > 0 { format!("obj/{key}.obj") } else { String::new() };
                    exported.insert(row.fp, rel.clone());
                    cells = (rel, s.vertices, format!("{:.3},{:.3},{:.3}", s.bbox[0][0], s.bbox[0][1], s.bbox[0][2]),
                        format!("{:.3},{:.3},{:.3}", s.bbox[1][0], s.bbox[1][1], s.bbox[1][2]), s.env_uv_meshes, s.no_uv_meshes, s.errors.join("; "));
                }
            }
            let _ = writeln!(*table, "{}\t{}\t{}\t{}\t{}\t{}\t{key}\t{near}\t{far}\t{third}\t{}\t{skinned}\t{rigid}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                row.source, row.level, row.record, row.slot, row.class_id, row.mode, c.meshes.len(), h.joints, h.joints_far, row.names,
                cells.0, cells.1, cells.2, cells.3, cells.4, cells.5, cells.6.replace(['\t', '\n'], " "));
    Ok(())
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: catalogue_export DISC.iso OUT_DIR TRIG.bin".into());
    }
    let (iso, out) = (&args[1], PathBuf::from(&args[2]));
    let trig = SkinInputs::trig_table(&std::fs::read(&args[3]).map_err(|e| e.to_string())?)?;
    for d in ["obj", "tex"] {
        std::fs::create_dir_all(out.join(d)).map_err(|e| e.to_string())?;
    }
    let mut disc = uyafmt::disc::Disc::open(iso)?;
    let toc = uyafmt::toc::read(&mut disc)?;
    let mut table = String::from("source\tlevel\trecord\tslot\tclass_id\tmode\tasset_id\tnear_triangles\tfar_triangles\tthird_triangles\tmeshes\tskinned_meshes\trigid_meshes\tnear_joints\tfar_joints\ttextures\texported\tobj_vertices\tbbox_min\tbbox_max\tenv_uv_meshes\tno_uv_meshes\terrors\n");
    let mut problems = String::from("source\tlevel\trecord\tslot\tclass_id\tproblem\n");
    let mut exported: BTreeMap<u64, String> = BTreeMap::new();
    let levels: BTreeSet<usize> = toc.files.iter().map(|f| f.level).collect();
    for &level in &levels {
        let mut rows = 0usize;
        // LEVEL core.
        let data = match toc.files.iter().any(|f| f.level == level && f.kind == uyafmt::toc::FileKind::Level) {
            true => match uyafmt::walk::level(iso, level) {
                Ok(d) => Some(d),
                Err(e) => {
                    let _ = writeln!(problems, "LEVEL\t{level}\t\t\t\t{}", flat(&e));
                    None
                }
            },
            false => None,
        };
        let (mut ship, mut clank) = (None, None);
        if data.is_some() {
            match uyafmt::levelcode::globals(iso, level) {
                Ok(g) => {
                    for (skin, slot) in [(g.ship_texture, &mut ship), (g.clank_texture, &mut clank)] {
                        match uyafmt::moby::wad_image(iso, skin.wad, skin.pair as usize) {
                            Ok(image) => *slot = Some(image),
                            Err(e) => {
                                let _ = writeln!(problems, "SKIN\t{level}\t\t\t\t{}", flat(&e));
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = writeln!(problems, "SKIN\t{level}\t\t\t\tlevel globals: {}", flat(&e));
                }
            }
        }
        if let Some(core) = data.as_ref().and_then(|d| d.core.as_ref()) {
            for (slot, (class, record)) in core.mobys.iter().zip(&core.moby_classes).enumerate() {
                let Some(class) = class else {
                    let _ = writeln!(problems, "LEVEL\t{level}\t\t{slot}\t{}\tClass has no decoded mesh data", record.id);
                    continue;
                };
                let mut names = BTreeSet::new();
                for local in locals(class) {
                    let index = usize::try_from(local).ok().and_then(|i| record.textures.get(i)).copied();
                    if let Some(index) = index.filter(|&i| i != 255) {
                        let t = core.moby_textures.get(index as usize).ok_or("Moby texture index outside source table")?;
                        if let Some(n) = stem(t)? {
                            names.insert(n);
                        }
                    } else {
                        names.insert(format!("runtime-material-{}-{local}", record.mode));
                    }
                }
                let names = names.into_iter().collect::<Vec<_>>().join(",");
                let fp = fingerprint(class, &names);
                // The fingerprint keeps the inventory's runtime-material names; the export resolves what the loader binds:
                // a 0xFF setup of a mode 1 class gets the ship's skin, of another nonzero mode Clank's (func_L01_00428750;
                // standard skin 0, as the review renderer draws it), and setups -2 / -3 the core's special textures.
                let lookup = |local: i32| -> Option<Tex<'_>> {
                    let index = usize::try_from(local).ok().and_then(|i| record.textures.get(i)).copied();
                    match index {
                        Some(i) if i != 255 => core.moby_textures.get(i as usize).map(Tex::Image),
                        Some(_) if record.mode == 1 => ship.as_ref().map(Tex::Image),
                        Some(_) if record.mode >= 2 => clank.as_ref().map(Tex::Image),
                        None if local == -2 || local == -3 => Some(Tex::Image(&core.special_textures[(-2 - local) as usize])),
                        _ => Some(Tex::Runtime(format!("runtime_mode{}_local{local}", record.mode))),
                    }
                };
                let row = Row { source: "LEVEL", level, record: String::new(), slot, class_id: record.id, mode: record.mode.to_string(), fp, class, names };
                process(&row, &lookup, &out, &trig, &mut exported, &mut table)?;
                rows += 1;
            }
        }
        // SCENE blocks (cutscene-only classes).
        let mut blocks = vec![];
        if let Some(f) = toc.files.iter().find(|f| f.level == level && f.kind == uyafmt::toc::FileKind::Scene) {
            for k in 0..30 {
                match uyafmt::scene::load(&mut disc, f, k) {
                    Ok(Some(b)) => blocks.push((k, b)),
                    Ok(None) => {}
                    Err(e) => {
                        let _ = writeln!(problems, "SCENE\t{level}\t{k}\t\t\t{}", flat(&e));
                    }
                }
            }
        }
        for (k, b) in &blocks {
            for (slot, sc) in b.block.classes.iter().enumerate() {
                let mut names = BTreeSet::new();
                for local in locals(&sc.class) {
                    match sc.image(local) {
                        Ok(t) => {
                            if let Some(n) = stem(t)? {
                                names.insert(n);
                            }
                        }
                        Err(_) => {
                            names.insert(format!("runtime-material-scene-{local}"));
                        }
                    }
                }
                let names = names.into_iter().collect::<Vec<_>>().join(",");
                let fp = fingerprint(&sc.class, &names);
                let lookup = |local: i32| -> Option<Tex<'_>> {
                    match sc.image(local) {
                        Ok(t) => Some(Tex::Image(t)),
                        Err(_) => Some(Tex::Runtime(format!("runtime_scene_local{local}"))),
                    }
                };
                let row = Row { source: "SCENE", level, record: k.to_string(), slot, class_id: sc.id, mode: String::new(), fp, class: &sc.class, names };
                process(&row, &lookup, &out, &trig, &mut exported, &mut table)?;
                rows += 1;
            }
        }
        std::fs::write(out.join("inventory.tsv"), &table).map_err(|e| e.to_string())?;
        std::fs::write(out.join("problems.tsv"), &problems).map_err(|e| e.to_string())?;
        println!("level {level}: {rows} rows ({} SCENE blocks)", blocks.len());
    }
    Ok(())
}
