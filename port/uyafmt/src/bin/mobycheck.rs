//! mobycheck DISC.iso REF.txt: compares `uyafmt::moby::class`'s packet A data and the formula of the module doc ("VU1 program 0") with what
//! the game itself drew on the traced Veldin frame (field 2400). REF.txt is written by captures/agent-mobygeo/refgen.py: for each of the
//! 375 runs of VU1 program 0 (MSCAL 0x50 and 0x70) the inputs the run's uploads left in VU1 memory (the constants and matrix at TOPS + 0x123 to
//! 0x12C, the position, colour and texture-coordinate buffers), where the run's packet A came from (a class of the core: class offset
//! and mesh record; or RAM: the packet's bytes) and the GS quadwords the game really produced for the run (the trace's GS stream).
//!
//! mobycheck DISC.iso stats: decodes every moby class of the 34 story levels' cores and prints the totals the module doc quotes.
//!
//! What each comparison would show if the decoder or the formula were wrong: strip assembly (strips, restarts, ADC skips, the groups
//! between zero entries, the dropped look-ahead entries) wrong: the groups' vertex counts or the vertices' ADC bits or XYZF words
//! (word 3 carries ADC as bit 15) differ from the GS stream; UV scale wrong: every ST word (u * Q, v * Q) differs; vertex indexing wrong:
//! the vertex's ST, RGBA and XYZF come from another vertex and differ; formula wrong: XYZF, ST or Q words differ in their low bits or
//! wholly. The GS stream is the game's real output (the VU1 interpreter of captures/agent-tieb rounds to nearest and differs from the
//! hardware in the last bit of some ST words, so it is used only to say whether a run executed the clipping code).

use std::collections::BTreeMap;
use uyafmt::disc::{u32_at, Disc};
use uyafmt::moby::{self, MobyClass, PacketA};
use uyafmt::toc::{self, FileKind};
use uyafmt::track::{Space, Tracker};

type Q = [u32; 4];

struct Run {
    n: usize,
    micro: u32,
    kind: String,
    class_off: usize,
    rec: usize,
    tail: usize,
    uv_in_packet: bool,
    clipped: bool,
    c: Vec<u32>,
    p: Vec<i32>,
    k: Vec<i32>,
    u: Vec<i32>,
    a: Vec<u8>,
    g: Vec<Q>,
}

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).expect("hex word")
}

fn ints(l: &str, tag: &str) -> Result<Vec<i32>, String> {
    let mut it = l.split_whitespace();
    if it.next() != Some(tag) {
        return Err(format!("expected a {tag} line"));
    }
    it.map(|x| x.parse::<i32>().map_err(|e| format!("{tag}: {e}"))).collect()
}

fn read_ref(path: &str) -> Result<Vec<Run>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let mut lines = text.lines();
    let mut runs = Vec::new();
    while let Some(l) = lines.next() {
        if l.is_empty() {
            continue;
        }
        let h: Vec<&str> = l.split_whitespace().collect();
        if h[0] != "R" {
            return Err(format!("expected an R line, got {}", h[0]));
        }
        let num = |i: usize| h[i].parse::<i64>().map_err(|e| format!("R line: {e}"));
        let mut next = || lines.next().ok_or("ref ends inside a run".to_string());
        let c: Vec<u32> = next()?.split_whitespace().skip(1).map(hex).collect();
        let p = ints(next()?, "P")?;
        let k = ints(next()?, "K")?;
        let u = ints(next()?, "U")?;
        let al = next()?;
        let a = match al.split_whitespace().nth(1) {
            Some(x) => (0..x.len() / 2).map(|i| u8::from_str_radix(&x[2 * i..2 * i + 2], 16).unwrap()).collect(),
            None => Vec::new(),
        };
        let gl: Vec<&str> = next()?.split_whitespace().collect();
        let nq: usize = gl[1].parse().map_err(|e| format!("G line: {e}"))?;
        let g = (0..nq).map(|i| [hex(gl[2 + 4 * i]), hex(gl[3 + 4 * i]), hex(gl[4 + 4 * i]), hex(gl[5 + 4 * i])]).collect();
        runs.push(Run { n: num(1)? as usize, micro: num(2)? as u32, kind: h[4].to_string(), class_off: num(5)? as usize, rec: num(6)? as usize, tail: num(7)? as usize, uv_in_packet: num(8)? == 1, clipped: num(9)? == 1, c, p, k, u, a, g });
    }
    Ok(runs)
}

// VU1's floating point: the products and sums are rounded toward zero (the hardware's FMAC; the trace's GS words show it).
fn rz(x: f64) -> f32 {
    let f = x as f32;
    if (f as f64).abs() > x.abs() { f32::from_bits(f.to_bits() - 1) } else { f }
}
fn mul(a: f32, b: f32) -> f32 {
    rz(a as f64 * b as f64)
}
fn add(a: f32, b: f32) -> f32 {
    rz(a as f64 + b as f64)
}
fn div(a: f32, b: f32) -> f32 {
    rz(a as f64 / b as f64)
}
fn ftoi4(x: f32) -> u32 {
    ((x as f64) * 16.0).trunc().clamp(i32::MIN as f64, i32::MAX as f64) as i32 as u32
}

/// The per-run constants of the module doc's table: TOPS + 0x123 (vf24), + 0x124 (vf23), + 0x128 (vf22), + 0x129 to 0x12B (vf17 to vf19) and + 0x12C (vf20).
struct Consts {
    fog_min: f32,
    q_num: f32,
    offset: [f32; 4],
    rows: [[f32; 4]; 3],
    t4: [f32; 4],
}

fn consts(c: &[u32]) -> Consts {
    let f = |q: usize, l: usize| f32::from_bits(c[4 * q + l]);
    let qv = |q: usize| [f(q, 0), f(q, 1), f(q, 2), f(q, 3)];
    Consts { fog_min: f(0, 0), q_num: f(1, 0), offset: qv(5), rows: [qv(6), qv(7), qv(8)], t4: qv(9) }
}

/// The GS words of one vertex ((S, T, Q) of ST, then the XYZF words) from the formula of the module doc. `pos` is the vertex's position
/// in the mesh's stream (0, 1, 2 ...); `quirk` is set when the stream's fourth vertex has ADC (entry 3 of the list negative).
fn vertex(c: &Consts, micro: u32, pos: usize, quirk: bool, p: [i32; 3], uv: [f32; 2], adc: bool) -> ([u32; 3], [u32; 4]) {
    let pf = [p[0] as f32, p[1] as f32, p[2] as f32];
    let mut acc = [0f32; 4];
    if pos < 2 || micro == 0x70 {
        acc = c.t4;
        let order: [usize; 3] = if pos < 2 { [0, 1, 2] } else { [2, 1, 0] };
        for k in order {
            for l in 0..4 {
                acc[l] = add(acc[l], mul(c.rows[k][l], pf[k]));
            }
        }
    } else {
        for l in 0..4 {
            acc[l] = mul(c.rows[0][l], pf[0]);
            acc[l] = add(acc[l], mul(c.rows[1][l], pf[1]));
            acc[l] = add(acc[l], mul(c.rows[2][l], pf[2]));
            acc[l] = add(acc[l], c.t4[l]);
        }
    }
    let q = div(c.q_num, acc[3]);
    let xyz = [add(mul(acc[0], q), c.offset[0]), add(mul(acc[1], q), c.offset[1]), add(mul(acc[2], q), c.offset[2])];
    let mut fog = if micro == 0x70 { add(c.t4[3], c.offset[3]) } else { add(acc[3], c.offset[3]) };
    if fog < c.fog_min {
        fog = c.fog_min;
    }
    if fog > 255.0 {
        fog = 255.0;
    }
    if adc {
        fog = add(fog, 3072.0);
    }
    let mut st = [mul(uv[0], q), mul(uv[1], q), q];
    if quirk && micro == 0x70 && pos == 1 {
        st = [mul(st[0], q), mul(st[1], q), mul(q, q)];
    }
    ([st[0].to_bits(), st[1].to_bits(), st[2].to_bits()], [ftoi4(xyz[0]), ftoi4(xyz[1]), ftoi4(xyz[2]), ftoi4(fog)])
}

struct Group {
    ad: Option<Vec<Q>>,
    verts: Vec<(Q, Q, Q)>,
}

/// The run's GS quadwords as groups: an A+D packet (the texture registers) starts a group, the strip packets after it are its
/// vertices; a strip packet after another one is the program's continuation and begins with a copy of the previous packet's last two
/// vertices, which are removed (counted in `copies`, checked equal).
fn parse_gs(g: &[Q], copies: &mut (usize, usize)) -> Result<Vec<Group>, String> {
    let mut groups: Vec<Group> = Vec::new();
    let mut cur: Option<usize> = None;
    let mut prev: Option<Vec<(Q, Q, Q)>> = None;
    let mut i = 0;
    while i < g.len() {
        let t = g[i];
        let nl = (t[0] & 0x7fff) as usize;
        let nr = match (t[1] >> 28) & 15 {
            0 => 16,
            v => v as usize,
        };
        if t[2] == 0xe {
            groups.push(Group { ad: Some(g[i + 1..i + 1 + nl * nr].to_vec()), verts: Vec::new() });
            cur = Some(groups.len() - 1);
            prev = None;
        } else if t[2] == 0x412 {
            let ci = match cur {
                Some(c) => c,
                None => {
                    groups.push(Group { ad: None, verts: Vec::new() });
                    cur = Some(groups.len() - 1);
                    groups.len() - 1
                }
            };
            let mut vs: Vec<(Q, Q, Q)> = (0..nl).map(|j| (g[i + 1 + 3 * j], g[i + 2 + 3 * j], g[i + 3 + 3 * j])).collect();
            if nl > 0 {
                if let Some(pv) = &prev {
                    if vs.len() < 2 {
                        return Err("continuation packet of fewer than two vertices".to_string());
                    }
                    // The copy repeats the vertex's ST/Q words, the colour's low bytes and the XYZF words (the w word of ST and the
                    // upper bytes of the colour words are lanes the GS ignores).
                    let same = |a: &(Q, Q, Q), b: &(Q, Q, Q)| a.0[..3] == b.0[..3] && (0..4).all(|j| a.1[j] & 0xff == b.1[j] & 0xff) && a.2 == b.2;
                    for (a, b) in vs[..2].iter().zip(&pv[pv.len() - 2..]) {
                        if same(a, b) {
                            copies.0 += 1;
                        } else {
                            copies.1 += 1;
                        }
                    }
                    vs.drain(..2);
                }
                groups[ci].verts.extend(vs.iter().copied());
                // The packet's own last two vertices (the continuation repeats them).
                let all: Vec<(Q, Q, Q)> = (0..nl).map(|j| (g[i + 1 + 3 * j], g[i + 2 + 3 * j], g[i + 3 + 3 * j])).collect();
                prev = Some(all[all.len() - 2.min(all.len())..].to_vec());
            }
        } else {
            return Err(format!("GIF tag with registers {:#x}", t[2]));
        }
        i += 1 + nl * nr;
    }
    groups.retain(|g| g.ad.is_some() || !g.verts.is_empty());
    Ok(groups)
}

struct Level {
    core: Vec<u8>,
    idx: Vec<u8>,
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
    Ok(Level { core, idx, classes })
}

#[derive(Default)]
struct Stats {
    runs: usize,
    runs_core: usize,
    runs_ram: usize,
    runs_clipped: usize,
    groups: usize,
    group_mismatch: usize,
    vertices: usize,
    st_ok: usize,
    xyzf_ok: usize,
    rgba_ok: usize,
    rgba_exact: usize,
    adc_ok: usize,
    triangles: usize,
    uv_checked: usize,
    uv_ok: usize,
    tex_checked: usize,
    tex_ok: usize,
    tex_unmapped: usize,
    ad_regs_ok: usize,
    copies: (usize, usize),
    runs_ok: usize,
    quirk_runs: usize,
    micro: BTreeMap<u32, usize>,
}

fn check_run(s: &mut Stats, run: &Run, lv: &Level, classes: &BTreeMap<usize, MobyClass>, shown: &mut usize) -> Result<(), String> {
    s.runs += 1;
    *s.micro.entry(run.micro).or_default() += 1;
    if run.clipped {
        s.runs_clipped += 1;
    }
    let owned;
    let (pa, record): (&PacketA, Option<usize>) = if run.kind == "core" {
        s.runs_core += 1;
        let c = classes.get(&run.class_off).ok_or_else(|| format!("run {}: class at {:#x} not decoded", run.n, run.class_off))?;
        let m = c.meshes.get(run.rec).ok_or_else(|| format!("run {}: no mesh record {}", run.n, run.rec))?;
        (m.packet_a.as_ref().ok_or_else(|| format!("run {}: mesh record {} has no packet A", run.n, run.rec))?, Some(run.rec))
    } else {
        s.runs_ram += 1;
        owned = moby::decode_packet_a(&run.a, run.tail).map_err(|e| format!("run {}: RAM packet: {e}", run.n))?;
        (&owned, None)
    };
    let _ = record;
    let c = consts(&run.c);
    // Class texture index bytes (the class record's +0x10, 16 bytes) for the texture mapping check.
    let (nrec, orec) = (u32_at(&lv.idx, 24)? as usize, u32_at(&lv.idx, 28)? as usize);
    let tex_bytes: Option<Vec<u8>> = if run.kind == "core" {
        let r = (0..nrec).find(|&r| u32_at(&lv.idx, orec + 32 * r).map(|v| v as usize == run.class_off).unwrap_or(false)).ok_or("class without a record")?;
        Some(lv.idx[orec + 32 * r + 0x10..orec + 32 * r + 0x20].to_vec())
    } else {
        None
    };
    // Texture coordinates: the packet's, compared with the buffer the run drew with; a run whose UV buffer did not come from its
    // packet (uv_in_packet false) is drawn with the buffer's values.
    let uv_of = |i: usize| -> [f32; 2] { [run.u[2 * i] as f32 / 4096.0, run.u[2 * i + 1] as f32 / 4096.0] };
    if run.uv_in_packet {
        let uvs = pa.uvs.as_ref().ok_or_else(|| format!("run {}: the UV buffer came from the packet but the decoder found no UVs", run.n))?;
        for (i, uv) in uvs.iter().enumerate() {
            s.uv_checked += 1;
            if *uv == uv_of(i) {
                s.uv_ok += 1;
            } else if *shown < 20 {
                *shown += 1;
                println!("run {} UV {i}: decoder {uv:?}, buffer {:?}", run.n, uv_of(i));
            }
        }
    }
    let mut copies = (0, 0);
    let groups = parse_gs(&run.g, &mut copies)?;
    s.copies.0 += copies.0;
    s.copies.1 += copies.1;
    let mut ok = true;
    if groups.len() != pa.strips.len() {
        s.group_mismatch += 1;
        println!("run {} ({}): the GS stream has {} groups, the decoder {} strips", run.n, run.kind, groups.len(), pa.strips.len());
        return Ok(());
    }
    let e3_neg = pa.strips.first().is_some_and(|st| st.vertices.len() >= 4 && st.vertices[3].skip);
    if e3_neg && run.micro == 0x70 {
        s.quirk_runs += 1;
    }
    let mut pos = 0usize;
    for (gi, (g, st)) in groups.iter().zip(&pa.strips).enumerate() {
        s.groups += 1;
        // The texture registers of the group: its A+D packet (TEX1_1, CLAMP_1, TEX0_1, MIPTBP1_1 with their register numbers).
        match (&g.ad, st.texture) {
            (Some(ad), Some(k)) => {
                let ts = pa.textures[k];
                let regs_ok = ad.len() == 4 && ad[0][2] == 0x14 && ad[1][2] == 8 && ad[2][2] == 6 && ad[3][2] == 0x34;
                s.ad_regs_ok += regs_ok as usize;
                if let (Some(tb), true) = (&tex_bytes, ts.local >= 0) {
                    let r = *tb.get(ts.local as usize).ok_or_else(|| format!("run {}: local texture {} of 16", run.n, ts.local))? as u32;
                    if r == 0xff {
                        // The class's index byte is 0xFF: no texture record; the game builds the registers from whatever
                        // follows the table (func_L01_00423700 reads the record at table + 16 * 255).
                        s.tex_unmapped += 1;
                        println!("run {} group {gi}: local texture {} has index byte 0xFF (no record); the game's TEX1_1 {:08x?} CLAMP_1 {:08x?}", run.n, ts.local, ad[0], ad[1]);
                    } else {
                        s.tex_checked += 1;
                        let tex1_ok = (ad[0][0] >> 6) & 7 == ts.mmin & 7 && ad[0][0] & 0x20 != 0 && ad[0][1] == ts.k;
                        let clamp_ok = ad[1][0] == (ts.wms | (ts.wmt << 2) | (r << 24)) && ad[1][1] == 0;
                        if tex1_ok && clamp_ok {
                            s.tex_ok += 1;
                        } else if *shown < 20 {
                            *shown += 1;
                            println!("run {} group {gi}: texture {} -> record {r}: TEX1 {:08x?} CLAMP {:08x?}, setup k {:#x} mmin {} wms {} wmt {}", run.n, ts.local, ad[0], ad[1], ts.k, ts.mmin, ts.wms, ts.wmt);
                        }
                    }
                }
            }
            (None, None) => {}
            _ => {
                println!("run {} group {gi}: A+D packet {} but the strip's texture is {:?}", run.n, g.ad.is_some(), st.texture);
                ok = false;
            }
        }
        if g.verts.len() != st.vertices.len() {
            println!("run {} group {gi}: {} vertices in the GS stream, {} decoded", run.n, g.verts.len(), st.vertices.len());
            ok = false;
            pos += st.vertices.len();
            continue;
        }
        for (k, (gv, sv)) in g.verts.iter().zip(&st.vertices).enumerate() {
            let i = sv.index as usize;
            let p = [run.p[3 * i], run.p[3 * i + 1], run.p[3 * i + 2]];
            let uv = if run.uv_in_packet { pa.uvs.as_ref().unwrap()[i] } else { uv_of(i) };
            let (stw, xyzf) = vertex(&c, run.micro, pos, e3_neg, p, uv, sv.skip);
            let col = [run.k[4 * i], run.k[4 * i + 1], run.k[4 * i + 2], run.k[4 * i + 3]];
            s.vertices += 1;
            let st_ok = gv.0[..3] == stw;
            let xyzf_ok = gv.2 == xyzf;
            let rgba_ok = (0..4).all(|j| gv.1[j] & 0xff == col[j] as u32 & 0xff);
            let adc_ok = (gv.2[3] >> 15) & 1 == (sv.skip) as u32;
            s.st_ok += st_ok as usize;
            s.xyzf_ok += xyzf_ok as usize;
            s.rgba_ok += rgba_ok as usize;
            s.rgba_exact += (0..4).all(|j| gv.1[j] == col[j] as u32) as usize;
            s.adc_ok += adc_ok as usize;
            if k >= 2 && !sv.skip {
                s.triangles += 1;
            }
            if !(st_ok && xyzf_ok && rgba_ok && adc_ok) {
                ok = false;
                if *shown < 20 {
                    *shown += 1;
                    println!("run {} ({}) group {gi} vertex {k} (index {i}, position {pos}): game ST {:08x?} XYZF {:08x?}; decoder ST {stw:08x?} XYZF {xyzf:08x?}", run.n, run.kind, &gv.0[..3], gv.2);
                }
            }
            pos += 1;
        }
    }
    s.runs_ok += ok as usize;
    Ok(())
}

fn story_levels() -> Vec<usize> {
    (1..=14).chain(16..=24).chain(26..=36).collect()
}

fn stats(iso: &str) -> Result<(), String> {
    let mut d = Disc::open(iso)?;
    let tc = toc::read(&mut d)?;
    let (mut classes, mut meshes, mut with_a, mut no_uv, mut strips, mut verts, mut tris, mut setups) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut cull: BTreeMap<i8, usize> = BTreeMap::new();
    let mut prepass: BTreeMap<i8, usize> = BTreeMap::new();
    let (mut local_neg, mut local_big, mut run_starts, mut run_starts_untextured, mut lod_runs_past, mut uvless_with_tex) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let mut a_counts: BTreeMap<i16, usize> = BTreeMap::new();
    let mut b_skinned = 0usize;
    let mut b_rigid = 0usize;
    for level in story_levels() {
        let lv = match load_level(&mut d, &tc, level) {
            Ok(l) => l,
            Err(e) => return Err(format!("level {level}: {e}")),
        };
        let mut t = Tracker::new();
        let mut root = t.root("LEVEL core (unpacked)", "L", lv.core.len() as u64, Space::Unpacked);
        for &(o, sz) in &lv.classes {
            let c = moby::class(&mut t, &mut root, "moby", o, &lv.core[o as usize..(o + sz) as usize]).map_err(|e| format!("level {level} class at {o:#x}: {e}"))?;
            classes += 1;
            let mut runs: Vec<(usize, usize)> = vec![(0, c.header.meshes_near as usize), (c.header.meshes_near as usize, c.header.meshes_far as usize)];
            if let Some(l) = &c.lod {
                for e in &l.entries {
                    runs.push((e.run.0 as usize, e.run.1 as usize));
                    if e.alternate.0 != 0 {
                        runs.push((e.alternate.0 as usize, e.alternate.1 as usize));
                    }
                }
            }
            for (start, count) in runs {
                if count == 0 {
                    continue;
                }
                match c.meshes.get(start).and_then(|m| m.packet_a.as_ref()) {
                    Some(pa) => {
                        run_starts += 1;
                        if pa.strips.first().is_none_or(|s| s.texture.is_none()) {
                            run_starts_untextured += 1;
                        }
                    }
                    None => lod_runs_past += 1,
                }
            }
            for m in &c.meshes {
                meshes += 1;
                match &m.packet_b {
                    Some(moby::PacketB::Skinned(_)) => b_skinned += 1,
                    Some(moby::PacketB::Rigid(_)) => b_rigid += 1,
                    None => {}
                }
                if let Some(pa) = &m.packet_a {
                    with_a += 1;
                    if pa.uvs.is_none() {
                        no_uv += 1;
                        if !pa.textures.is_empty() {
                            uvless_with_tex += 1;
                        }
                    }
                    *cull.entry(pa.cull).or_default() += 1;
                    *prepass.entry(pa.prepass_count).or_default() += 1;
                    strips += pa.strips.len();
                    for st in &pa.strips {
                        verts += st.vertices.len();
                        tris += st.vertices.iter().enumerate().filter(|(k, v)| *k >= 2 && !v.skip).count();
                    }
                    setups += pa.textures.len();
                    for ts in &pa.textures {
                        *a_counts.entry(ts.a_count).or_default() += 1;
                        if ts.local < 0 {
                            local_neg += 1;
                        } else if ts.local >= 16 {
                            local_big += 1;
                        }
                    }
                }
            }
        }
        println!("level {level}: {} classes decoded so far", classes);
    }
    println!("{classes} moby classes, {meshes} mesh records ({with_a} with a packet A: {no_uv} without texture coordinates, {uvless_with_tex} of those with texture setups), {strips} strips, {verts} strip vertices, {tris} triangles, {setups} texture setups ({local_neg} negative numbers, {local_big} numbers of 16 or more)");
    println!("packet B: {b_skinned} skinned, {b_rigid} rigid");
    println!("strip list header byte +0 (cull): {cull:?}; byte +3: {prepass:?}; texture setup word 2 low half: {a_counts:?}");
    println!("LOD runs: {run_starts} start at a mesh with a packet A ({run_starts_untextured} of them without a texture setup in their first strip), {lod_runs_past} start at a record without one");
    Ok(())
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let r = if a.len() == 3 && a[2] == "stats" {
        stats(&a[1])
    } else if a.len() == 3 {
        run(&a[1], &a[2])
    } else {
        eprintln!("usage: mobycheck DISC.iso REF.txt | mobycheck DISC.iso stats");
        std::process::exit(2);
    };
    if let Err(e) = r {
        eprintln!("mobycheck: {e}");
        std::process::exit(1);
    }
}

fn run(iso: &str, refp: &str) -> Result<(), String> {
    let mut d = Disc::open(iso)?;
    let tc = toc::read(&mut d)?;
    let lv = load_level(&mut d, &tc, 1)?;
    let runs = read_ref(refp)?;
    let mut t = Tracker::new();
    let mut root = t.root("LEVEL core (unpacked)", "L01", lv.core.len() as u64, Space::Unpacked);
    let mut classes: BTreeMap<usize, MobyClass> = BTreeMap::new();
    for &(o, sz) in &lv.classes {
        let c = moby::class(&mut t, &mut root, "moby", o, &lv.core[o as usize..(o + sz) as usize]).map_err(|e| format!("class at {o:#x}: {e}"))?;
        classes.insert(o as usize, c);
    }
    println!("level 1: {} moby classes decoded, {} mesh records", classes.len(), classes.values().map(|c| c.meshes.len()).sum::<usize>());
    let mut s = Stats::default();
    let mut shown = 0;
    for r in &runs {
        check_run(&mut s, r, &lv, &classes, &mut shown)?;
    }
    println!("{} runs of program 0 compared (MSCAL entries {:?}): {} with their packet A from a class of the core (decoded from the disc), {} from RAM (the packet's bytes decoded by decode_packet_a); {} executed the clipping code", s.runs, s.micro, s.runs_core, s.runs_ram, s.runs_clipped);
    println!("{} groups (an A+D packet and its strips), {} runs whose group count differed", s.groups, s.group_mismatch);
    println!("{} vertices compared: ST/Q words equal {}, XYZF words equal {}, RGBA equal in the low 8 bits {} (all 32 bits {}), ADC bit equal {}; {} triangles", s.vertices, s.st_ok, s.xyzf_ok, s.rgba_ok, s.rgba_exact, s.adc_ok, s.triangles);
    println!("continuation packets: the repeated vertices equal the previous packet's last two: {} equal, {} different", s.copies.0, s.copies.1);
    println!("{} texture coordinates compared with the run's UV buffer: {} equal", s.uv_checked, s.uv_ok);
    println!("{} groups have an A+D packet of TEX1_1, CLAMP_1, TEX0_1 and MIPTBP1_1 (regs 0x14, 8, 6, 0x34); for the core runs' groups TEX1_1 and CLAMP_1 were checked against the setup's words and the class's texture index bytes (record = byte[local number], CLAMP_1 bits 24 up): {} checked, {} equal, {} with index byte 0xFF (no record)", s.ad_regs_ok, s.tex_checked, s.tex_ok, s.tex_unmapped);
    println!("{} runs equal in every group, vertex and field; {} MSCAL 0x70 runs have the fourth vertex ADC ('quirk' of the second vertex's ST)", s.runs_ok, s.quirk_runs);
    Ok(())
}
