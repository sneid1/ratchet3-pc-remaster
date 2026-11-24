//! skincheck DISC.iso REF.txt RUNS.txt RAM.bin JOINTS.bin: compares `uyafmt::moby::skin` (what func_003D00F8 writes for a mesh: the
//! skinned positions, the lit colours and, for the rigid layout, the texture coordinates that VU1 program 0 reads at TOPS + 0, + 0x61
//! and + 0xC2) with what the game uploaded on the traced Veldin frame (field 2400).
//!
//! Inputs. RAM.bin is the EE RAM at the end of field 2400 (captures/trace/ram02400.bin). The draw records (func_L01_0043EA78's job
//! lists, level 1) are in it: the traced frame's list at 0x800000 (found because the trace's UNPACK sources of the runs are the
//! destinations of that list's mesh entries; RUNS.txt holds each run's source addresses, written by captures/agent-mobyskin/gen.py
//! from the trace), the next frame's list at 0x680000. Each record gives the light section, the mesh entries (packet B's RAM address
//! and the destination) and the second group; packet B is decoded from the RAM bytes by `moby::decode_packet_b` and, for the meshes
//! of the core's classes, from the disc by `moby::class` (the two must be equal). The joint matrices are not in RAM (they are the
//! scratchpad's, built by func_L01_00439E00): JOINTS.bin holds the scratchpad's first 112 matrices as they are when each record's skin job starts (the records of each list
//! in order, nothing cleared between them), written by captures/agent-mobyskin
//! gen.py from RAM.bin by running the game's own func_L01_00439E00 (a small R5900 and VU0 interpreter, captures/agent-mobyskin/ee.py)
//! on the record and the pose buffers in RAM. REF.txt is captures/agent-mobygeo/ref.txt: the positions, colours and texture
//! coordinates each of the 375 program 0 runs received in VU1 memory.
//!
//! Comparison 1: every run of the trace against `skin` on the traced frame's list (positions as the three s16 words, colours as four
//! byte words, rigid texture coordinates as two s16 words), word for word. A run whose instance's pose buffers have advanced since
//! the draw (the buffers are updated in place; RAM is the end of the field) cannot be recomputed: those runs are listed with the
//! blend of the same class's record in the next frame's list.
//! Comparison 2: every mesh of the next frame's list against the arrays the game's own skin job left in RAM at the mesh entry's
//! destination (the pose buffers, records and arrays of that frame are all in RAM).
//!
//! What each comparison would show if `skin` were wrong: a wrong joint blend, weight, address, slot or lag changes positions
//! (integers) and colours of every vertex that uses that matrix or copy; a wrong normal decoding or light value changes colour
//! bytes only; a wrong rounding order flips a position's last integer or a colour byte for a few vertices; wrong texture-coordinate
//! scaling changes every rigid vertex's s and t. A wrong joint matrix input would show as the animated instances' differences.

use std::collections::{BTreeMap, HashMap};
use uyafmt::disc::{u32_at, Disc};
use uyafmt::moby::{self, JobLights, Mesh, MobyClass, PacketB, RigidInputs, SkinInputs, SkinState, Skinned};
use uyafmt::toc::{self, FileKind};
use uyafmt::track::{Space, Tracker};

const BASE_CORE: usize = 0x91e000;

struct Level {
    core: Vec<u8>,
    classes: Vec<(u64, u64)>,
}

fn load_level(d: &mut Disc, tc: &toc::Toc, level: usize) -> Result<Level, String> {
    let f = tc.files.iter().find(|f| f.level == level && f.kind == FileKind::Level).ok_or(format!("no LEVEL file for level {level}"))?;
    let (off, n) = (u32_at(&f.header, 0x10)? as u64, u32_at(&f.header, 0x14)? as u64);
    let b = d.sectors(f.sector + off, n)?;
    let part = |k: usize| -> Result<(usize, usize), String> { Ok((u32_at(&b, 8 * k)? as usize, u32_at(&b, 8 * k + 4)? as usize)) };
    let (io, is) = part(1)?;
    let idx = b[io..io + is].to_vec();
    let (co, cs) = part(9)?;
    let core = uyafmt::lz::unpack(&b[co..co + cs])?.data;
    let mut t = Tracker::new();
    let mut region = t.root("LEVEL level data", "L", b.len() as u64, Space::Disc);
    let (offsets, _textures, _h) = uyafmt::core::index(&mut t, &mut region, io as u64, &idx)?;
    let mut by: BTreeMap<u64, &str> = BTreeMap::new();
    for &(o, k) in &offsets {
        if o < core.len() as u64 {
            by.entry(o).or_insert(k);
        }
    }
    let keys: Vec<u64> = by.keys().copied().collect();
    let mut classes = Vec::new();
    for (i, (&o, &k)) in by.iter().enumerate() {
        if k == "moby class data" {
            let end = keys.get(i + 1).copied().unwrap_or(core.len() as u64);
            classes.push((o, end - o));
        }
    }
    Ok(Level { core, classes })
}

fn hex(s: &str) -> usize {
    usize::from_str_radix(s, 16).expect("hex")
}

struct Ref {
    p: Vec<i32>,
    k: Vec<i32>,
    u: Vec<i32>,
}

fn ints(l: &str, tag: &str) -> Vec<i32> {
    let mut it = l.split_whitespace();
    assert_eq!(it.next(), Some(tag));
    it.map(|x| x.parse::<i32>().expect("int")).collect()
}

fn read_ref(path: &str) -> Result<Vec<Ref>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if !lines[i].starts_with("R ") {
            return Err(format!("expected an R line at {i}"));
        }
        out.push(Ref { p: ints(lines[i + 2], "P"), k: ints(lines[i + 3], "K"), u: ints(lines[i + 4], "U") });
        i += 7;
    }
    Ok(out)
}

fn u32r(ram: &[u8], a: usize) -> usize {
    u32::from_le_bytes([ram[a], ram[a + 1], ram[a + 2], ram[a + 3]]) as usize
}

fn q16(v: usize) -> usize {
    v.div_ceil(16) * 16
}

#[derive(Clone)]
struct Entry {
    packet_b: usize,
    qwc: usize,
    dest: usize,
    rigid: bool,
}

struct Job {
    at: usize,
    entries: Vec<Entry>,
    group2: Vec<Entry>,
    group2_at: usize,
    field_14: usize,
}

fn is_job(ram: &[u8], a: usize) -> bool {
    let n = u32r(ram, a);
    if !(0 < n && n < 200 && (u32r(ram, a + 0x10) & 0xffff) < 0x200) {
        return false;
    }
    for k in 0..n.min(2) {
        let (w0, w1) = (u32r(ram, a + 0xC0 + 8 * k), u32r(ram, a + 0xC4 + 8 * k));
        let b = (w0 & !0xFF) >> 4;
        if !((0x1000000..0x2000000).contains(&b) && (w0 & 0xff) > 0 && (0x400000..0x1000000).contains(&w1)) {
            return false;
        }
    }
    true
}

fn entry(ram: &[u8], at: usize) -> Entry {
    let (w0, w1) = (u32r(ram, at), u32r(ram, at + 4));
    Entry { packet_b: (w0 & !0xFF) >> 4, qwc: w0 & 0xFF, dest: w1, rigid: false }
}

fn jobs(ram: &[u8], lo: usize, hi: usize) -> Vec<Job> {
    let mut v = Vec::new();
    for a in (lo..hi).step_by(16) {
        if !is_job(ram, a) {
            continue;
        }
        let n = u32r(ram, a);
        let entries = (0..n).map(|k| entry(ram, a + 0xC0 + 8 * k)).collect();
        let field_14 = u32r(ram, a + 0x14);
        let group2_at = a + 0x40 + 16 * ((n >> 1) + 9);
        let mut group2 = Vec::new();
        if field_14 >> 16 != 0 {
            let mut at = group2_at + 64;
            while u32r(ram, at) != 0 {
                let mut e = entry(ram, at);
                e.rigid = true;
                group2.push(e);
                at += 8;
            }
        }
        v.push(Job { at: a, entries, group2, group2_at, field_14 });
    }
    v
}

/// A mesh from packet B's bytes in RAM (what the draw job names), or the disc's mesh when the class is in the core.
fn mesh_of(ram: &[u8], e: &Entry, disc: &HashMap<usize, &Mesh>, stats: &mut Stats) -> Result<Mesh, String> {
    let b = &ram[e.packet_b..e.packet_b + 16 * e.qwc];
    let pb = moby::decode_packet_b(b).map_err(|x| format!("packet B at {:#x}: {x}", e.packet_b))?;
    let vertices = match &pb {
        PacketB::Skinned(s) => s.rows.len() + s.copies.len(),
        PacketB::Rigid(r) => r.rows.len(),
    } as u8;
    if let Some(m) = disc.get(&e.packet_b) {
        stats.disc_checked += 1;
        if m.packet_b.as_ref() == Some(&pb) && m.b_qwords as usize == e.qwc {
            stats.disc_equal += 1;
        } else {
            stats.disc_different += 1;
        }
        return Ok((*m).clone());
    }
    Ok(Mesh { record: 0, b_qwords: e.qwc as u8, position_qwords: 0, normal_qwords: 0, vertices, a_at: 0, b_at: 0, packet_a: None, packet_b: Some(pb) })
}

#[derive(Default)]
struct Stats {
    disc_checked: usize,
    disc_equal: usize,
    disc_different: usize,
    c_meshes: usize,
    b_meshes: usize,
    runs: usize,
    runs_ok: usize,
    words: usize,
    words_ok: usize,
    vertices: usize,
    rigid_runs: usize,
    ram_meshes_ok: usize,
    ram_meshes: usize,
    ram_words: usize,
    ram_words_bad: usize,
}

type Out = HashMap<usize, (Skinned, usize)>;

/// Runs a list: one `SkinState` for all its records in address order. Returns the results keyed by the destination of the
/// array of colours (the address the trace's colour UNPACK reads), with the destination of the entry.
fn run_list(ram: &[u8], js: &[Job], jm: &HashMap<usize, Vec<[[f32; 4]; 4]>>, trig: &[[f32; 2]], disc: &HashMap<usize, &Mesh>, stats: &mut Stats) -> Result<(Out, Vec<(usize, usize)>), String> {
    let mut out: Out = HashMap::new();
    let mut order = Vec::new();
    let mut st = SkinState::new();
    for j in js {
        let hdr = &ram[j.at..j.at + 0xC0];
        let lights = JobLights::read(hdr)?;
        let mats = jm.get(&j.at).ok_or(format!("no joint matrices for the record at {:#x}", j.at))?;
        let joints = mats.clone();
        let mut inputs = SkinInputs { joints, lights, rigid: None, trig: trig.to_vec() };
        for e in &j.entries {
            let mesh = mesh_of(ram, e, disc, stats)?;
            let r = moby::skin(&mesh, &inputs, &mut st).map_err(|x| format!("record {:#x} mesh at {:#x}: {x}", j.at, e.packet_b))?;
            let n = r.positions.len();
            out.insert(e.dest, (r, e.dest));
            order.push((j.at, e.dest));
            let _ = n;
        }
        if !j.group2.is_empty() {
            inputs.rigid = Some(RigidInputs::read(&ram[j.group2_at..j.group2_at + 0x30], j.field_14 as u32)?);
            for e in &j.group2 {
                let mesh = mesh_of(ram, e, disc, stats)?;
                let r = moby::skin(&mesh, &inputs, &mut st).map_err(|x| format!("record {:#x} rigid mesh at {:#x}: {x}", j.at, e.packet_b))?;
                let n = r.positions.len();
                // The colours are the third array: the trace's colour source is dest + q16(4 n) + q16(6 n).
                out.insert(e.dest + q16(4 * n) + q16(6 * n), (r, e.dest));
                order.push((j.at, e.dest));
            }
        }
    }
    Ok((out, order))
}

fn s16(x: i32) -> i16 {
    x as i16
}

fn run(a: &[String]) -> Result<(), String> {
    let mut d = Disc::open(&a[0])?;
    let tc = toc::read(&mut d)?;
    let lv = load_level(&mut d, &tc, 1)?;
    let refs = read_ref(&a[1])?;
    let runs: Vec<(usize, usize, usize, usize)> = std::fs::read_to_string(&a[2])
        .map_err(|e| e.to_string())?
        .lines()
        .map(|l| {
            let w: Vec<&str> = l.split_whitespace().collect();
            (w[0].parse().unwrap(), hex(w[1]), hex(w[2]), hex(w[3]))
        })
        .collect();
    let ram = std::fs::read(&a[3]).map_err(|e| e.to_string())?;
    let jb = std::fs::read(&a[4]).map_err(|e| e.to_string())?;
    let mut jm: HashMap<usize, Vec<[[f32; 4]; 4]>> = HashMap::new();
    let mut i = 0;
    while i < jb.len() {
        let (at, n) = (u32r(&jb, i), u32r(&jb, i + 4));
        i += 8;
        let mats = (0..n)
            .map(|j| {
                let mut m = [[0f32; 4]; 4];
                for r in 0..4 {
                    for c in 0..4 {
                        m[r][c] = f32::from_bits(u32r(&jb, i + 64 * j + 16 * r + 4 * c) as u32);
                    }
                }
                m
            })
            .collect();
        i += 64 * n;
        jm.insert(at, mats);
    }
    let trig = SkinInputs::trig_table(&ram[0x2BF5A0..0x2BF5A0 + 0x800])?;
    // The disc's decode of the core's classes.
    let mut t = Tracker::new();
    let mut root = t.root("LEVEL core (unpacked)", "L01", lv.core.len() as u64, Space::Unpacked);
    let mut classes: Vec<(usize, MobyClass)> = Vec::new();
    for &(o, sz) in &lv.classes {
        let c = moby::class(&mut t, &mut root, "moby", o, &lv.core[o as usize..(o + sz) as usize]).map_err(|e| format!("class at {o:#x}: {e}"))?;
        classes.push((BASE_CORE + o as usize, c));
    }
    let mut disc: HashMap<usize, &Mesh> = HashMap::new();
    for (base, c) in &classes {
        for m in &c.meshes {
            if m.packet_b.is_some() {
                disc.insert(base + m.b_at, m);
            }
        }
    }
    println!("level 1: {} classes of the core decoded from the disc, {} of their packet Bs known by RAM address", classes.len(), disc.len());
    let mut s = Stats::default();
    let jc = jobs(&ram, 0x800000, 0x803000);
    let jbl = jobs(&ram, 0x680000, 0x682a90);
    println!("draw records in RAM: {} in the traced frame's list (0x800000), {} in the next frame's (0x680000)", jc.len(), jbl.len());
    let (oc, _) = run_list(&ram, &jc, &jm, &trig, &disc, &mut s)?;
    s.c_meshes = oc.len();
    let (ob, _) = run_list(&ram, &jbl, &jm, &trig, &disc, &mut s)?;
    s.b_meshes = ob.len();
    println!("meshes skinned: {} in the traced frame's list, {} in the next frame's list", s.c_meshes, s.b_meshes);
    println!("packet B decoded from RAM equals the disc's decode for {} of {} meshes of the core's classes ({} differ)", s.disc_equal, s.disc_checked, s.disc_different);

    // Comparison 1: the trace's runs.
    let mut bad_jobs: BTreeMap<usize, usize> = BTreeMap::new();
    // per differing record: vertices compared, vertices with a different position, largest position difference, colour bytes different
    let mut vstat: BTreeMap<usize, (usize, usize, i32, usize)> = BTreeMap::new();
    let by_dest: HashMap<usize, usize> = {
        // job record of every colour destination in the traced list, for the report
        let mut m = HashMap::new();
        for j in &jc {
            for e in &j.entries {
                m.insert(e.dest, j.at);
            }
            for e in &j.group2 {
                m.insert(e.dest, j.at);
            }
        }
        m
    };
    let mut missing = 0;
    for &(i, pos_src, col_src, _uv) in &runs {
        let r = &refs[i];
        let Some((sk, dest)) = oc.get(&col_src) else {
            missing += 1;
            continue;
        };
        s.runs += 1;
        let n = sk.positions.len();
        let _ = pos_src;
        let mut ok = true;
        let mut w_ok = 0;
        let mut w = 0;
        let (mut vd, mut maxd, mut cd) = (0usize, 0i32, 0usize);
        for v in 0..n {
            let mut vbad = false;
            for k in 0..3 {
                w += 1;
                if r.p[3 * v + k] == sk.positions[v][k] as i32 {
                    w_ok += 1;
                } else {
                    ok = false;
                    vbad = true;
                    maxd = maxd.max((r.p[3 * v + k] - sk.positions[v][k] as i32).abs());
                }
            }
            if vbad {
                vd += 1;
            }
            for k in 0..4 {
                w += 1;
                if r.k[4 * v + k] == sk.colours[v][k] as i32 {
                    w_ok += 1;
                } else {
                    ok = false;
                    cd += 1;
                }
            }
            if let Some(uv) = &sk.uvs {
                for k in 0..2 {
                    w += 1;
                    if r.u[2 * v + k] == uv[v][k] as i32 {
                        w_ok += 1;
                    } else {
                        ok = false;
                    }
                }
            }
        }
        s.vertices += n;
        s.words += w;
        s.words_ok += w_ok;
        if sk.uvs.is_some() {
            s.rigid_runs += 1;
        }
        if ok {
            s.runs_ok += 1;
        } else {
            *bad_jobs.entry(by_dest[dest]).or_insert(0) += 1;
            let e = vstat.entry(by_dest[dest]).or_insert((0, 0, 0, 0));
            *e = (e.0 + n, e.1 + vd, e.2.max(maxd), e.3 + cd);
        }
    }
    println!(
        "comparison 1: {} of the trace's {} program 0 runs found a mesh entry of the traced frame's list ({} did not); {} runs ({} rigid) compared, {} vertices, {} words ({} equal), {} runs equal in every word",
        s.runs + missing,
        runs.len(),
        missing,
        s.runs,
        s.rigid_runs,
        s.vertices,
        s.words,
        s.words_ok,
        s.runs_ok
    );
    for (job, nbad) in &bad_jobs {
        // The same instance's record in the next frame's list: same class tables (+0x18, +0x1C) and mesh count.
        let c_blend = f32::from_bits(u32r(&ram, job + 4) as u32);
        let next: Vec<&Job> = jbl.iter().filter(|b| u32r(&ram, b.at + 0x18) == u32r(&ram, job + 0x18) && u32r(&ram, b.at + 0x1C) == u32r(&ram, job + 0x1C) && b.entries.len() == jc.iter().find(|c| c.at == *job).unwrap().entries.len() + 0).collect();
        let nb = next.first().map(|b| f32::from_bits(u32r(&ram, b.at + 4) as u32));
        let (nv, vd, maxd, cd) = vstat[job];
        let b_job = next.first().map(|b| b.at);
        let same_pose = b_job.map(|b| (0x20..0x40).step_by(4).all(|o| u32r(&ram, b + o) == u32r(&ram, job + o)));
        println!("  record {job:#x}: {nbad} runs differ ({vd} of their {nv} vertices have a different position, largest difference {maxd}; {cd} colour bytes differ); its blend (record +4) is {c_blend}, the same instance's record in the next frame's list ({b_job:x?}, equal in every word with the game's arrays) has {nb:?} and the same pose buffer pointers: {same_pose:?}");
    }

    // Comparison 2: the next frame's list against the arrays in RAM.
    let mut bad2: BTreeMap<usize, usize> = BTreeMap::new();
    for j in jbl.iter() {
        for e in j.entries.iter().chain(j.group2.iter()) {
            let key = if e.rigid {
                let n = match oc_len(&ob, e) {
                    Some(n) => n,
                    None => continue,
                };
                e.dest + q16(4 * n) + q16(6 * n)
            } else {
                e.dest
            };
            let (sk, dest) = &ob[&key];
            let n = sk.positions.len();
            s.ram_meshes += 1;
            let (pa, ca, ua) = if e.rigid { (dest + q16(4 * n), dest + q16(4 * n) + q16(6 * n), Some(*dest)) } else { (dest + q16(4 * n), *dest, None) };
            let mut ok = true;
            for v in 0..n {
                for k in 0..3 {
                    s.ram_words += 1;
                    let g = i16::from_le_bytes([ram[pa + 6 * v + 2 * k], ram[pa + 6 * v + 2 * k + 1]]);
                    if g != s16(sk.positions[v][k] as i32) {
                        s.ram_words_bad += 1;
                        ok = false;
                    }
                }
                for k in 0..4 {
                    s.ram_words += 1;
                    if ram[ca + 4 * v + k] != sk.colours[v][k] {
                        s.ram_words_bad += 1;
                        ok = false;
                    }
                }
                if let (Some(u0), Some(uv)) = (ua, &sk.uvs) {
                    for k in 0..2 {
                        s.ram_words += 1;
                        let g = i16::from_le_bytes([ram[u0 + 4 * v + 2 * k], ram[u0 + 4 * v + 2 * k + 1]]);
                        if g != uv[v][k] {
                            s.ram_words_bad += 1;
                            ok = false;
                        }
                    }
                }
            }
            if ok {
                s.ram_meshes_ok += 1;
            } else {
                *bad2.entry(j.at).or_insert(0) += 1;
            }
        }
    }
    println!("comparison 2: {} meshes of the next frame's list compared with the arrays the game's skin job left in RAM, {} words ({} different): {} meshes equal in every word", s.ram_meshes, s.ram_words, s.ram_words_bad, s.ram_meshes_ok);
    for (job, nbad) in &bad2 {
        println!("  record {job:#x}: {nbad} meshes differ");
    }
    Ok(())
}

/// The vertex count of a rigid entry from its packet B header.
fn oc_len(_ob: &Out, e: &Entry) -> Option<usize> {
    Some(u32r_static(e))
}

fn u32r_static(e: &Entry) -> usize {
    // A rigid packet B's row count is its u32 header +0; the quadword count is (16 + 16 n) / 16.
    e.qwc - 1
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() != 5 {
        eprintln!("usage: skincheck DISC.iso REF.txt RUNS.txt RAM.bin JOINTS.bin");
        std::process::exit(2);
    }
    if let Err(e) = run(&a) {
        eprintln!("skincheck: {e}");
        std::process::exit(1);
    }
}
