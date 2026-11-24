//! shrubcheck DISC.iso REF.txt RAM.bin: compares `uyafmt::shrub::shrub`'s output with what the game's VU1 shrub programs (4 and 5)
//! computed on the traced Veldin frame (field 2400). REF.txt is written by captures/agent-shrubgeo/refgen.py, which replays the
//! trace's exact VIF1 uploads through captures/agent-tieb/replay.py and writes, for every run of program 4 or 5 that kicked a GIF
//! packet, the class block the run drew (found through the DMA source address of its header quadword), the instance records the
//! run read and each XGKICKed packet. RAM.bin is the traced RAM (the word at 0x1A1EF0 is the base func_L01_00424850 adds to GS
//! block numbers).
//!
//! For program 4 every packet is rebuilt from the decoded class and its instance record, quadword by quadword, and compared word
//! for word: the A+D tags and texture registers (rebuilt as func_L01_00424850 builds them), the strips' GIF tags, and per vertex
//! ST = (S, T, third) * Q, RGBAQ from the instance's colour table, XYZF2 from the instance matrix, with Q = 1 / w. A decoder that
//! put the primitives in the wrong order, scaled the UVs wrongly or read the wrong vertex would differ in the vertex's ST/Q
//! words (UV scale: every S, T and Q word), the slots (a tag or vertex quadword elsewhere) or XYZ.

use std::collections::BTreeMap;
use uyafmt::core::TexRecord;
use uyafmt::disc::{u32_at, Disc};
use uyafmt::shrub::{ShrubBlock, ShrubClass};
use uyafmt::toc::{self, FileKind};
use uyafmt::track::{Space, Tracker};

type Q = [u32; 4];

struct Run {
    n: usize,
    prog: u32,
    class: usize,
    block: usize,
    inst: Vec<Vec<Q>>,
    consts: [f32; 24],
    packets: Vec<Vec<Q>>,
}

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).expect("hex word")
}

fn quads(it: &mut std::str::SplitWhitespace, n: usize) -> Vec<Q> {
    (0..n).map(|_| [hex(it.next().unwrap()), hex(it.next().unwrap()), hex(it.next().unwrap()), hex(it.next().unwrap())]).collect()
}

fn read_ref(path: &str) -> Result<Vec<Run>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let mut lines = text.lines();
    let mut runs = Vec::new();
    while let Some(l) = lines.next() {
        if l.is_empty() {
            continue;
        }
        let h: Vec<usize> = l.split_whitespace().skip(1).map(|x| x.parse().unwrap()).collect();
        // R n prog class core_offset block packets
        let (n, prog, class, _cs, block, npk) = (h[0], h[1] as u32, h[2], h[3], h[4], h[5]);
        let v: Vec<usize> = lines.next().ok_or("V line")?.split_whitespace().skip(1).map(|x| x.parse().unwrap()).collect();
        let k = v[0];
        let c = lines.next().ok_or("C line")?;
        let cw: Vec<f32> = c.split_whitespace().skip(1).map(|x| f32::from_bits(hex(x))).collect();
        let mut consts = [0f32; 24];
        consts.copy_from_slice(&cw);
        let mut inst = Vec::new();
        for _ in 0..k.max(1) {
            let mut it = lines.next().ok_or("I line")?.split_whitespace();
            it.next();
            inst.push(quads(&mut it, 32));
        }
        let mut packets = Vec::new();
        for _ in 0..npk {
            let mut it = lines.next().ok_or("P line")?.split_whitespace();
            it.next();
            let len: usize = it.next().unwrap().parse().unwrap();
            packets.push(quads(&mut it, len));
        }
        runs.push(Run { n, prog, class, block, inst, consts, packets });
    }
    Ok(runs)
}

struct Level {
    core: Vec<u8>,
    idx: Vec<u8>,
    textures: Vec<TexRecord>,
    classes: Vec<(usize, u64, u64)>,
}

fn level1(iso: &str) -> Result<Level, String> {
    let mut d = Disc::open(iso)?;
    let tc = toc::read(&mut d)?;
    let f = tc.files.iter().find(|f| f.level == 1 && f.kind == FileKind::Level).ok_or("no LEVEL file for level 1")?;
    let (off, n) = (u32_at(&f.header, 0x10)? as u64, u32_at(&f.header, 0x14)? as u64);
    let b = d.sectors(f.sector + off, n)?;
    let part = |k: usize| -> Result<(usize, usize), String> { Ok((u32_at(&b, 8 * k)? as usize, u32_at(&b, 8 * k + 4)? as usize)) };
    let (io, is) = part(1)?;
    let idx = b[io..io + is].to_vec();
    let (co, cs) = part(9)?;
    let core = uyafmt::lz::unpack(&b[co..co + cs])?.data;
    let mut t = Tracker::new();
    let mut region = t.root("LEVEL level data", "L01", b.len() as u64, Space::Disc);
    let (offsets, textures, _h) = uyafmt::core::index(&mut t, &mut region, io as u64, &idx)?;
    let mut by: BTreeMap<u64, &str> = BTreeMap::new();
    for &(o, k) in &offsets {
        if o < core.len() as u64 {
            by.entry(o).or_insert(k);
        }
    }
    let keys: Vec<u64> = by.keys().copied().collect();
    let mut classes = Vec::new();
    for (i, (&o, &k)) in by.iter().enumerate() {
        if k == "shrub class data" {
            let end = keys.get(i + 1).copied().unwrap_or(core.len() as u64);
            classes.push((0, o, end - o));
        }
    }
    // class id = the class record number: record r's word 0 is its core offset.
    let (nrec, orec) = (u32_at(&idx, 40)? as usize, u32_at(&idx, 44)? as usize);
    for c in classes.iter_mut() {
        c.0 = (0..nrec).find(|&r| u32_at(&idx, orec + 48 * r).map(|v| v as u64 == c.1).unwrap_or(false)).ok_or("class without a record")?;
    }
    Ok(Level { core, idx, textures, classes })
}

fn f(u: u32) -> f32 {
    f32::from_bits(u)
}

fn ftoi4(x: f32) -> u32 {
    ((x as f64) * 16.0).trunc() as i32 as u32
}

/// The A+D data func_L01_00424850 builds for texture entry `e` of a block of class record `rec` (its 16 index bytes at +0x10).
fn texture_regs(e: &uyafmt::shrub::ShrubTexture, indexes: &[u8], recs: &[&TexRecord], base: u32, qw: [Q; 4]) -> Result<[Q; 4], String> {
    let t8 = *indexes.get(e.index as usize).ok_or("texture index past the 16 bytes")? as usize;
    let r = recs.get(t8).ok_or_else(|| format!("shrub texture record {t8} of {}", recs.len()))?;
    let (w, h) = (r.width as u32, r.height as u32);
    let lg = |v: u32| 31 - v.leading_zeros() as u64;
    let b = base as u64;
    let tex1 = (((r.levels as u64).wrapping_sub(1)) << 2) | ((e.mmin as u64) << 6) | 0x20 | ((e.k as u64) << 32);
    let clamp = (e.wms as u64) | ((e.wmt as u64) << 2) | ((t8 as u64) << 24);
    let tbw0 = ((w >> 6).max(1)) as u64;
    let tex0 = (tbw0 << 14) | (lg(w) << 26) | (0x13u64 << 20) | (lg(h) << 30) | (((r.palette as i64 as u64).wrapping_add(b)) << 37) | (1u64 << 34) | (1u64 << 63);
    let tbw1 = ((w >> 7).max(1)) as u64;
    let mip = (tbw1 << 14) | (((r.mip2 as i64 as u64).wrapping_add(b)) << 20) | (1u64 << 34) | (((r.mip3 as i64 as u64).wrapping_add(b)) << 40) | (1u64 << 54);
    let mk = |v: u64, src: Q| -> Q { [v as u32, (v >> 32) as u32, src[2], src[3]] };
    Ok([mk(tex1, qw[0]), mk(clamp, qw[1]), mk(mip, qw[2]), mk(tex0, qw[3])])
}

#[derive(Default)]
struct Stats {
    packets: usize,
    qwords: usize,
    words_differ: BTreeMap<&'static str, usize>,
    shown: usize,
    high_colour: usize,
    adc_skipped: usize,
    xyz_compared: usize,
    vertices: usize,
}

fn compare_packet(s: &mut Stats, p5: bool, run: &Run, k: usize, pk: &[Q], c: &ShrubClass, bl: &ShrubBlock, class_tex: &[Q], ad_tag: Q) {
    let inst = &run.inst[k];
    let m: Vec<[f32; 4]> = (1..=4).map(|i| [f(inst[i][0]), f(inst[i][1]), f(inst[i][2]), f(inst[i][3])]).collect();
    let alpha = inst[5][3];
    let (v29, v30) = (&run.consts[0..4], &run.consts[4..8]);
    // The fog value of the instance: MULAz.w ACC = vf21 * vf30z; MADDw.w vf09 = ACC + vf30 * 1.0; MAXx, MINIy, FTOI4.
    let fog = ftoi4((m[3][3] * v30[2] + v30[3] * 1.0).max(v29[0]).min(v29[1]));
    let mut exp: Vec<Q> = vec![[0; 4]; bl.packet_qwords as usize];
    for (i, t) in bl.textures.iter().enumerate() {
        let s0 = t.slot as usize;
        exp[s0] = ad_tag;
        for q in 0..4 {
            exp[s0 + 1 + q] = class_tex[i * 4 + q];
        }
    }
    for st in &bl.strips {
        let n = st.vertices.len() as u32;
        exp[st.slot as usize] = [n | ((st.eop as u32) << 15), 0x4000 | ((st.prim as u32) << 15) | (3 << 28), 0x412, st.slot];
    }
    for st in &bl.strips {
        for (j, &vi) in st.vertices.iter().enumerate() {
            let v = &bl.vertices[vi];
            let at = st.slot as usize + 1 + 3 * j;
            // ACC = vf21 * 1.0; ACC += vf18 * x; ACC += vf19 * y; vf01 = ACC + vf20 * z.
            let mut r = [0f32; 4];
            for ch in 0..4 {
                let mut acc = m[3][ch] * 1.0;
                acc += m[0][ch] * v.pos[0];
                acc += m[1][ch] * v.pos[1];
                r[ch] = acc + m[2][ch] * v.pos[2];
            }
            let qv = 1.0f32 / r[3];
            let col = inst.get(5 + v.colour as usize).copied().unwrap_or([0; 4]);
            s.vertices += 1;
            if v.colour >= 23 {
                s.high_colour += 1;
            }
            exp[at] = [(v.st[0] * qv).to_bits(), (v.st[1] * qv).to_bits(), (v.st[2] * qv).to_bits(), 0];
            exp[at + 1] = [col[0], col[1], col[2], alpha];
            if p5 {
                // Program 5 clips in clip space and scales to the screen afterwards: xyz = (clip.xyz * (third * Q)) * vf22 + vf23
                // (MULz at micro 0x550, MULA.xyz vf22 and MADDw.xyz vf23 at 0x570), then FTOI4.
                let (v22, v23) = (&run.consts[12..16], &run.consts[16..20]);
                let qq = v.st[2] * qv;
                let sc = |ch: usize| ((r[ch] * qq) * v22[ch]) + v23[ch] * 1.0;
                exp[at + 2] = [ftoi4(sc(0)), ftoi4(sc(1)), ftoi4(sc(2)), fog];
            } else {
                exp[at + 2] = [ftoi4(r[0] * qv), ftoi4(r[1] * qv), ftoi4(r[2] * qv), fog];
            }
        }
    }
    s.packets += 1;
    s.qwords += exp.len();
    if pk.len() != exp.len() {
        *s.words_differ.entry("packet length").or_default() += 1;
        return;
    }
    let mut kind = vec![""; exp.len()];
    for (i, k) in kind.iter_mut().enumerate() {
        *k = "packet qword";
        let _ = i;
    }
    for t in &bl.textures {
        kind[t.slot as usize] = "A+D tag";
        for q in 1..=4 {
            kind[t.slot as usize + q] = ["", "TEX1_1", "CLAMP_1", "MIPTBP1_1", "TEX0_1"][q];
        }
    }
    for st in &bl.strips {
        kind[st.slot as usize] = "strip GIF tag";
        for j in 0..st.vertices.len() {
            let at = st.slot as usize + 1 + 3 * j;
            kind[at] = "vertex ST/Q";
            kind[at + 1] = "vertex RGBA";
            kind[at + 2] = "vertex XYZ F";
        }
    }
    for i in 0..exp.len() {
        let mut g = pk[i];
        let mut e = exp[i];
        // TBP0 (TEX0_1 bits 0 to 13) and TBP1 (MIPTBP1_1 bits 0 to 13) are written at run time by the texture streamer (the
        // shrub texture record index sits in CLAMP_1's MINV for it): the loader leaves them 0.
        if kind[i] == "TEX0_1" || kind[i] == "MIPTBP1_1" {
            g[0] &= !0x3fff;
            e[0] &= !0x3fff;
        }
        if p5 && kind[i] == "vertex ST/Q" {
            // Program 5 leaves the vertex's colour index and end marker in the ST quadword's w (the GS ignores it).
            g[3] = 0;
        }
        if p5 && kind[i] == "vertex XYZ F" {
            // Program 5's clip pass sets ADC (bit 15 of w) on the vertices of the triangles it rejects or clips, and leaves
            // its working values in their XYZ; only the vertices the GS draws as they are can be compared.
            if g[3] & 0x8000 != 0 {
                s.adc_skipped += 1;
                continue;
            }
            s.xyz_compared += 1;
        }
        if g != e {
            *s.words_differ.entry(kind[i]).or_default() += 1;
            if s.shown < 12 || (kind[i] == "vertex RGBA" && s.shown < 40) {
                s.shown += 1;
                println!("run {} prog {} class {} block {} instance {k} qword {i} ({}): game {:08x?}, decoder {:08x?}", run.n, run.prog, run.class, run.block, kind[i], pk[i], exp[i]);
            }
        }
    }
    let _ = c;
}

#[derive(Default)]
struct Fans {
    empty: usize,
    fans: usize,
    tex_ok: usize,
    uv_ok: usize,
    vertices: usize,
}

/// A packet program 5 kicks for a clipped triangle: an A+D tag with the texture registers, then one GIF tag (PRIM 0x7D, a triangle
/// fan) with the clipped polygon's vertices. Its registers must be a block entry's, and its texture coordinates (ST / Q) must
/// lie in the texture-coordinate box of a triangle of the block's strips that is drawn with that entry.
fn check_fan(fans: &mut Fans, run: &Run, pk: &[Q], _c: &ShrubClass, bl: &ShrubBlock, class_tex: &[Q], ad: Q) {
    if pk.len() == 1 {
        fans.empty += 1;
        return;
    }
    fans.fans += 1;
    let nl = (pk[5][0] & 0x7fff) as usize;
    if pk[0] != ad || pk.len() != 6 + 3 * nl || (pk[5][1] >> 15) & 0x7ff != 0x7d || pk[5][2] != 0x412 {
        println!("run {} prog 5: packet of {} quadwords is not an A+D tag and one fan: {:08x?} {:08x?}", run.n, pk.len(), pk[0], pk[5]);
        return;
    }
    let mask = |q: Q| [q[0] & !0x3fff, q[1], q[2], q[3]];
    let tex = (0..bl.textures.len()).find(|&i| (0..4).all(|q| mask(pk[1 + q]) == mask(class_tex[i * 4 + q])));
    let Some(tex) = tex else {
        println!("run {} prog 5: fan packet's texture registers match no entry of the block", run.n);
        return;
    };
    fans.tex_ok += 1;
    let uv: Vec<(f32, f32)> = (0..nl).map(|k| (f32::from_bits(pk[6 + 3 * k][0]) / f32::from_bits(pk[6 + 3 * k][2]), f32::from_bits(pk[6 + 3 * k][1]) / f32::from_bits(pk[6 + 3 * k][2]))).collect();
    let eps = 1.0 / 4096.0;
    let found = bl.strips.iter().filter(|st| st.texture == tex).any(|st| {
        st.vertices.windows(3).any(|w| {
            let t: Vec<&uyafmt::shrub::ShrubVertex> = w.iter().map(|&i| &bl.vertices[i]).collect();
            let (u0, u1) = (t.iter().map(|v| v.st[0]).fold(f32::MAX, f32::min), t.iter().map(|v| v.st[0]).fold(f32::MIN, f32::max));
            let (v0, v1) = (t.iter().map(|v| v.st[1]).fold(f32::MAX, f32::min), t.iter().map(|v| v.st[1]).fold(f32::MIN, f32::max));
            uv.iter().all(|&(u, v)| u >= u0 - eps && u <= u1 + eps && v >= v0 - eps && v <= v1 + eps)
        })
    });
    if found {
        fans.uv_ok += 1;
        fans.vertices += nl;
    } else {
        println!("run {} prog 5: fan of {nl} vertices with UVs {uv:?} lies in no triangle's texture-coordinate box", run.n);
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 4 {
        eprintln!("usage: shrubcheck DISC.iso REF.txt RAM.bin");
        std::process::exit(2);
    }
    if let Err(e) = run(&a[1], &a[2], &a[3]) {
        eprintln!("shrubcheck: {e}");
        std::process::exit(1);
    }
}

fn run(iso: &str, refp: &str, ramp: &str) -> Result<(), String> {
    let lv = level1(iso)?;
    let ram = std::fs::read(ramp).map_err(|e| format!("{ramp}: {e}"))?;
    let base = (u32_at(&ram, 0x1a1ef0)? as i32 >> 8) as u32;
    let runs = read_ref(refp)?;
    let mut t = Tracker::new();
    let mut root = t.root("LEVEL core (unpacked)", "L01", lv.core.len() as u64, Space::Unpacked);
    let mut classes: BTreeMap<usize, ShrubClass> = BTreeMap::new();
    let (mut nblocks, mut nverts, mut nstrips) = (0, 0, 0);
    for &(id, o, sz) in &lv.classes {
        let c = uyafmt::shrub::shrub(&mut t, &mut root, o, &lv.core[o as usize..(o + sz) as usize]).map_err(|e| format!("class {id}: {e}"))?;
        nblocks += c.blocks.len();
        for b in &c.blocks {
            nverts += b.vertices.len();
            nstrips += b.strips.len();
        }
        classes.insert(id, c);
    }
    println!("level 1: {} shrub classes decoded, {nblocks} blocks, {nstrips} strips, {nverts} vertices", classes.len());
    let recs: Vec<&TexRecord> = lv.textures.iter().filter(|r| r.table == 3).collect();
    let (nrec, orec) = (u32_at(&lv.idx, 40)? as usize, u32_at(&lv.idx, 44)? as usize);
    let _ = nrec;
    let mut stats = Stats::default();
    let mut stats5 = Stats::default();
    let mut fans = Fans::default();
    let (mut p4, mut p5, mut p5_packets) = (0, 0, 0);
    for run in &runs {
        let c = classes.get(&run.class).ok_or_else(|| format!("run {}: class {} not decoded", run.n, run.class))?;
        let bl = c.blocks.get(run.block).ok_or_else(|| format!("run {}: block {}", run.n, run.block))?;
        if run.prog == 5 {
            p5 += 1;
            p5_packets += run.packets.len();
        } else {
            p4 += 1;
        }
        let indexes = lv.idx[orec + 48 * run.class + 0x10..orec + 48 * run.class + 0x20].to_vec();
        // The texture quadwords as the packet holds them: the disc's quadwords with the data words rebuilt (words 2 and 3 are
        // the disc's), the A+D tag from the VU1's vf31.
        let core_q = |at: usize| -> Q { [u32_at(&lv.core, at).unwrap(), u32_at(&lv.core, at + 4).unwrap(), u32_at(&lv.core, at + 8).unwrap(), u32_at(&lv.core, at + 12).unwrap()] };
        let (cs, _) = lv.classes.iter().find(|x| x.0 == run.class).map(|x| (x.1 as usize, x.2)).unwrap();
        // The block's header quadword: the class's +0x40 table pointer.
        let blk_at = cs + u32_at(&lv.core, cs + 0x40 + 8 * run.block)? as usize;
        // First unpack: STCYCL, NOP, STMOD, UNPACK word, then the header quadword.
        let mut at = blk_at;
        loop {
            let w = u32_at(&lv.core, at)?;
            if (w >> 24) & 0x7f == 0x6c {
                break;
            }
            at += 4;
        }
        let d0 = at + 4;
        let (mm, _nn) = (u32_at(&lv.core, d0 + 4)? as usize, u32_at(&lv.core, d0)? as usize);
        let te0 = d0 + 16 + 16 * mm;
        let mut class_tex: Vec<Q> = Vec::new();
        for (i, e) in bl.textures.iter().enumerate() {
            let q = te0 + 0x40 * i;
            let regs = texture_regs(e, &indexes, &recs, base, [core_q(q), core_q(q + 0x10), core_q(q + 0x20), core_q(q + 0x30)])?;
            class_tex.extend_from_slice(&regs);
        }
        let ad = [run.consts[8].to_bits(), run.consts[9].to_bits(), run.consts[10].to_bits(), run.consts[11].to_bits()];
        let mut inst_k = 0;
        for (k, pk) in run.packets.iter().enumerate() {
            if run.prog == 4 {
                compare_packet(&mut stats, false, run, k, pk, c, bl, &class_tex, ad);
            } else if pk.len() != bl.packet_qwords as usize {
                check_fan(&mut fans, run, pk, c, bl, &class_tex, ad);
            } else if pk[0] == ad {
                compare_packet(&mut stats5, true, run, inst_k, pk, c, bl, &class_tex, ad);
                inst_k += 1;
            }
        }
    }
    println!("program 4 runs {p4}, program 5 runs {p5} ({p5_packets} packets)");
    println!("program 4: {} vertices compared, {} of them with colour index 23 or more (the quadword after the instance's 23 colours)", stats.vertices, stats.high_colour);
    println!("program 5 full-size packets: {} compared ({} vertices with ADC set not compared in XYZ, {} compared), differing quadwords by kind: {:?}", stats5.packets, stats5.adc_skipped, stats5.xyz_compared, stats5.words_differ);
    println!("program 5 clipped output: {} empty kicks (NLOOP 0), {} fan packets: {} with the texture registers of a block entry, {} whose polygon lies in the texture-coordinate box of a triangle of the block drawn with that entry ({} vertices)", fans.empty, fans.fans, fans.tex_ok, fans.uv_ok, fans.vertices);
    println!("program 4: {} packets compared ({} quadwords), differing quadwords by kind: {:?}", stats.packets, stats.qwords, stats.words_differ);
    Ok(())
}
