//! Compares uyafmt's tie class decoder with what the game's VU1 programs 2 and 3 computed for every packet drawn in the traced
//! Veldin frame (field 2400): `tiecheck <dir> <ram02400.bin>`, `dir` holding the files captures/agent-tiegeo/ref.py wrote
//! (core01.bin: level 1's unpacked core; classes.txt: offset and size of each tie class; ref.txt: per draw the packet's core
//! offset, the instance record the program read (matrix and quads 4, 5), the RAM address of its first colour quadword and the GS
//! packets it kicked, from the game's own program run on the traced input).
//!
//! Per draw: the output's strips must equal the decoder's strips (count, length, texture), the sequence of drawn vertices' S/Q,
//! T/Q must equal the decoder's UVs vertex for vertex, the triangles the GS draws from the output (a TRISTRIP kick per vertex
//! from the third on, none where ADC is set) must equal the decoder's triangles for the strip, the RGBA must equal the RAM bytes
//! at the colour address + 4 x the vertex's colour index (the instance's colour buffer is checked against the instance array in
//! RAM), the screen position (x, y within 2/16 pixel, from the GS packets the real game sent in the trace, as the program emulation
//! mis-models the DIV latency for a few vertices) must equal the decoder's class-local position put through the instance matrix as
//! program 2 does (program 2 draws only; program 3 reads its instance record elsewhere),
//! and the A+D registers the packet sends must equal the decoder's TEX1, CLAMP and ALPHA of the packet's registers.
use std::collections::HashMap;
use uyafmt::tie::{tie, TieClass, TiePacket};
use uyafmt::track::{Space, Tracker};

#[derive(Default)]
struct V {
    s: f32,
    t: f32,
    q: f32,
    rgba: [u64; 4],
    xyz: [u64; 3],
    w: u64,
}

#[derive(Default)]
struct Run {
    n: u64,
    prog: u64,
    micro: u64,
    pk: u64,
    caddr: u64,
    m: Vec<[u32; 4]>,
    groups: Vec<Vec<(u64, u64)>>,
    strips: Vec<(usize, Vec<V>, Vec<[u64; 3]>)>,
}

fn f(x: u32) -> f32 {
    f32::from_bits(x)
}

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let dir = a.get(1).ok_or("usage: tiecheck <dir> <ram02400.bin>")?;
    let ram = std::fs::read(a.get(2).ok_or("usage: tiecheck <dir> <ram02400.bin>")?).map_err(|e| e.to_string())?;
    let core = std::fs::read(format!("{dir}/core01.bin")).map_err(|e| e.to_string())?;
    let idx = std::fs::read(format!("{dir}/index01.bin")).map_err(|e| e.to_string())?;
    let w = |i: usize| u32::from_le_bytes(idx[4 * i..4 * i + 4].try_into().unwrap()) as usize;
    let tex_tab = idx[w(17)..w(17) + 16 * w(16)].to_vec();
    let mut classes: Vec<(u64, TieClass)> = Vec::new();
    for l in std::fs::read_to_string(format!("{dir}/classes.txt")).map_err(|e| e.to_string())?.lines() {
        let mut it = l.split(' ').map(|x| x.parse::<u64>().unwrap());
        let (off, size) = (it.next().unwrap(), it.next().unwrap());
        let mut t = Tracker::new();
        let mut root = t.root("LEVEL core (unpacked)", "L01", core.len() as u64, Space::Unpacked);
        let c = tie(&mut t, &mut root, off, &core[off as usize..(off + size) as usize]).map_err(|e| format!("class at {off:#x}: {e}"))?;
        classes.push((off, c));
    }
    // The runs.
    let mut runs: Vec<Run> = Vec::new();
    for l in std::fs::read_to_string(format!("{dir}/ref.txt")).map_err(|e| e.to_string())?.lines() {
        let w: Vec<&str> = l.split(' ').collect();
        let p = |i: usize| -> u64 { let s = w[i]; if let Some(h) = s.strip_prefix("0x") { u64::from_str_radix(h, 16).unwrap() } else { s.parse().unwrap() } };
        match w[0] {
            "R" => runs.push(Run { n: p(1), prog: p(2), micro: p(3), pk: p(4), caddr: p(6), ..Default::default() }),
            "M" => runs.last_mut().unwrap().m.push([p(1) as u32, p(2) as u32, p(3) as u32, p(4) as u32]),
            "A" => {
                let r = runs.last_mut().unwrap();
                let (lo, hi, reg) = (p(1), p(2), p(3));
                if reg == 6 {
                    r.groups.push(Vec::new());
                }
                r.groups.last_mut().unwrap().push((lo | hi << 32, reg));
            }
            "S" => {
                let r = runs.last_mut().unwrap();
                let g = r.groups.len();
                r.strips.push((g - 1, Vec::new(), Vec::new()));
            }
            "W" => runs.last_mut().unwrap().strips.last_mut().unwrap().2.push([p(1), p(2), p(3)]),
            "V" => {
                let v = V { s: f(p(1) as u32), t: f(p(2) as u32), q: f(p(3) as u32), rgba: [p(4), p(5), p(6), p(7)], xyz: [p(8), p(9), p(10)], w: p(11) };
                runs.last_mut().unwrap().strips.last_mut().unwrap().1.push(v);
            }
            _ => return Err(format!("ref.txt: {l}")),
        }
    }
    let ru32 = |a: usize| u32::from_le_bytes(ram[a..a + 4].try_into().unwrap()) as usize;
    let ru16 = |a: usize| u16::from_le_bytes(ram[a..a + 2].try_into().unwrap()) as usize;
    let (inst_base, col_base) = (ru32(0x1E0F54), ru32(0x1E0F64));
    // Every instance's colour buffer address.
    let mut buffers: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut k = 0;
    while inst_base + 32 * k + 0x16 <= ram.len() && k < 100000 {
        let c = ru16(inst_base + 32 * k + 0x14);
        if c != 0xffff {
            buffers.insert(col_base + 16 * c);
        }
        k += 1;
    }
    let mut cnt: HashMap<&str, u64> = HashMap::new();
    let mut bad: Vec<String> = Vec::new();
    let mut packets = std::collections::BTreeSet::new();
    let note = |bad: &mut Vec<String>, what: String| {
        // The first 6 of each kind (the text after "run N" up to the first digit run).
        let kind: String = what.split(':').nth(1).unwrap_or("").chars().filter(|c| c.is_alphabetic()).take(14).collect();
        if bad.iter().filter(|b| b.split(':').nth(1).unwrap_or("").chars().filter(|c| c.is_alphabetic()).take(14).collect::<String>() == kind).count() < 6 {
            bad.push(what);
        }
    };
    for r in &runs {
        let base = 0x91e000u64;
        let _ = base;
        let (ci, c) = classes.iter().rev().find(|(o, _)| *o <= r.pk).ok_or("no class")?;
        let rel = (r.pk - ci) as u32;
        let pkt: &TiePacket = c.lods.iter().flatten().find(|p| p.offset == rel).ok_or_else(|| format!("run {}: no packet at class {ci:#x}+{rel:#x}", r.n))?;
        packets.insert((r.prog, r.pk));
        *cnt.entry("draws").or_default() += 1;
        let second = r.micro == 0x90;
        let tex_list: &Vec<usize> = if second { &pkt.second.as_ref().ok_or("second draw of a packet without a second part")?.registers } else { &pkt.textures };
        // A+D registers.
        if r.groups.len() != tex_list.len() {
            note(&mut bad, format!("run {}: {} texture groups, the packet has {}", r.n, r.groups.len(), tex_list.len()));
            *cnt.entry("tex groups differ").or_default() += 1;
            continue;
        }
        for (g, &j) in r.groups.iter().zip(tex_list) {
            let t = &c.textures[j];
            let rec = class_rec(&idx, *ci, j)?;
            let levels = u16::from_le_bytes([tex_tab[16 * rec + 8], tex_tab[16 * rec + 9]]) as u64;
            let tex1 = (levels - 1) << 2 | 0x20 | (t.mmin as u64) << 6 | (t.k_word as u64) << 32;
            let clamp = t.wms as u64 | (t.wmt as u64) << 2 | (rec as u64) << 24;
            let want = [(0x14, tex1), (8, clamp), (0x42, t.alpha)];
            for (reg, val) in want {
                *cnt.entry("registers compared").or_default() += 1;
                if g.iter().find(|(_, rg)| *rg == reg).map(|x| x.0) != Some(val) {
                    *cnt.entry("registers differ").or_default() += 1;
                    note(&mut bad, format!("run {}: register {reg:#x} of texture {j}: game {:x?}, decoder {val:#x}", r.n, g.iter().find(|(_, rg)| *rg == reg)));
                }
            }
        }
        if r.strips.len() != pkt.strips.len() {
            *cnt.entry("strip counts differ").or_default() += 1;
            note(&mut bad, format!("run {}: {} strips, decoder {}", r.n, r.strips.len(), pkt.strips.len()));
            continue;
        }
        let mat: Vec<[f32; 4]> = r.m.iter().map(|q| [f(q[0]), f(q[1]), f(q[2]), f(q[3])]).collect();
        let alpha_flag = r.m[4][1];
        // The colour buffer of the instance.
        let have_colour = !second && r.caddr != 0;
        if have_colour {
            let buf = r.caddr as usize - pkt.colour_offset as usize;
            *cnt.entry("colour buffers checked").or_default() += 1;
            if !buffers.contains(&buf) {
                *cnt.entry("colour buffers not an instance's").or_default() += 1;
                note(&mut bad, format!("run {}: colour address {:#x} - offset {:#x} is no instance's buffer", r.n, r.caddr, pkt.colour_offset));
            }
        }
        for (si, ((tg, gv, tw), ds)) in r.strips.iter().zip(&pkt.strips).enumerate() {
            *cnt.entry("strips").or_default() += 1;
            if gv.len() != ds.vertices.len() || (*tg != ds.texture) {
                *cnt.entry("strips differ").or_default() += 1;
                note(&mut bad, format!("run {} strip {si}: {} vertices texture group {tg}, decoder {} and {}", r.n, gv.len(), ds.vertices.len(), ds.texture));
                continue;
            }
            for (i, (g, &vi)) in gv.iter().zip(&ds.vertices).enumerate() {
                let d = &pkt.vertices[vi as usize];
                *cnt.entry("vertices").or_default() += 1;
                if !second {
                    let (u, v) = (g.s / g.q, g.t / g.q);
                    let ok = |x: f32, y: f32| (x - y).abs() <= 1e-3 * y.abs().max(1.0);
                    if !ok(u, d.uv[0]) || !ok(v, d.uv[1]) {
                        *cnt.entry("uv differ").or_default() += 1;
                        note(&mut bad, format!("run {} strip {si} vertex {i}: game uv ({u}, {v}), decoder {:?}", r.n, d.uv));
                    }
                }
                // Colour.
                let want: Option<[u64; 4]> = if second {
                    pkt.second.as_ref().and_then(|s| s.colour).map(|c| [c[0] as u64, c[1] as u64, c[2] as u64, c[3] as u64])
                } else if have_colour {
                    let at = r.caddr as usize + 4 * d.colour as usize;
                    let b = &ram[at..at + 4];
                    Some([b[0] as u64, b[1] as u64, b[2] as u64, if alpha_flag == 0x80 { b[3] as u64 } else { alpha_flag as u64 }])
                } else {
                    None
                };
                if let Some(w) = want {
                    *cnt.entry("colours").or_default() += 1;
                    if w != g.rgba {
                        *cnt.entry("colours differ").or_default() += 1;
                        note(&mut bad, format!("run {} strip {si} vertex {i}: game rgba {:?}, expected {w:?} (alpha flag {alpha_flag:#x})", r.n, g.rgba));
                    }
                }
                // Screen position.
                let mut p = d.pos;
                if let Some(sw) = d.sway {
                    let k = mat_k(&r.m);
                    for a in 0..3 {
                        p[a] = sw[a] * k + p[a];
                    }
                }
                let mut acc = mat[3];
                for (row, x) in [(0usize, p[0]), (1, p[1]), (2, p[2])] {
                    for a in 0..4 {
                        acc[a] += mat[row][a] * x;
                    }
                }
                let q = 1.0f32 / acc[3];
                let sx = (acc[0] * q * 16.0) as i32;
                let sy = (acc[1] * q * 16.0) as i32;
                if r.prog != 2 {
                    continue;
                }
                *cnt.entry(if d.sway.is_some() { "positions (sway vertices)" } else { "positions" }).or_default() += 1;
                let (gx, gy) = (tw[i][0] as i32, tw[i][1] as i32);
                if (sx - gx).abs() > 2 || (sy - gy).abs() > 2 {
                    *cnt.entry(if d.sway.is_some() { "positions differ (sway vertices)" } else { "positions differ" }).or_default() += 1;
                    note(&mut bad, format!("run {} strip {si} vertex {i}: game xy ({gx:#x}, {gy:#x}), decoder ({sx:#x}, {sy:#x}) sway {:?} k {} pos {:?} w {}", r.n, d.sway, mat_k(&r.m), d.pos, acc[3]));
                }
            }
            // Triangles as the GS draws them: a kick per vertex from the third on unless ADC is set.
            for i in 2..gv.len() {
                let t = &pkt.triangles.iter().filter(|t| t.strip == si).nth(i - 2).ok_or("triangle")?;
                *cnt.entry("triangles").or_default() += 1;
                if gv[i].w & 0x8000 != 0 {
                    *cnt.entry("triangles skipped by ADC (culling pass, entry 0x10)").or_default() += 1;
                    continue;
                }
                *cnt.entry("triangles drawn").or_default() += 1;
                if t.v != [ds.vertices[i - 2], ds.vertices[i - 1], ds.vertices[i]] || t.texture != ds.texture {
                    *cnt.entry("triangles differ").or_default() += 1;
                    note(&mut bad, format!("run {} strip {si} triangle {}: decoder {:?}", r.n, i - 2, t.v));
                }
                if !second {
                    for (k, gk) in [i - 2, i - 1, i].into_iter().enumerate() {
                        let d = &pkt.vertices[t.v[k] as usize];
                        let (u, v) = (gv[gk].s / gv[gk].q, gv[gk].t / gv[gk].q);
                        let ok = |x: f32, y: f32| (x - y).abs() <= 1e-3 * y.abs().max(1.0);
                        if !ok(u, d.uv[0]) || !ok(v, d.uv[1]) {
                            *cnt.entry("triangle uvs differ").or_default() += 1;
                            note(&mut bad, format!("run {} strip {si} triangle {}: vertex {k} uv differs", r.n, i - 2));
                        }
                    }
                }
            }
        }
    }
    println!("tiecheck: {} classes decoded, {} draws (programs 2 and 3) of {} distinct packets", classes.len(), runs.len(), packets.len());
    let mut keys: Vec<_> = cnt.iter().collect();
    keys.sort();
    for (k, v) in keys {
        println!("  {k}: {v}");
    }
    let fails: u64 = cnt.iter().filter(|(k, _)| k.contains("differ") || k.contains("not an instance")).map(|(_, v)| *v).sum();
    for b in &bad {
        println!("  MISMATCH {b}");
    }
    if fails != 0 {
        return Err(format!("{fails} differences"));
    }
    println!("tiecheck: no differences");
    Ok(())
}

/// The x of the instance record's quad 4 (vf27x as program 2 loads it at micro 0xA68).
fn mat_k(m: &[[u32; 4]]) -> f32 {
    f32::from_bits(m[4][0])
}

/// The tie texture record index of register `j` of the class at core offset `off`: byte j of the 16 bytes at +0x10 of the class's
/// record in the core index (words 8/9, 32 bytes each, +0 the core offset).
fn class_rec(idx: &[u8], off: u64, j: usize) -> Result<usize, String> {
    let w = |i: usize| u32::from_le_bytes(idx[4 * i..4 * i + 4].try_into().unwrap()) as usize;
    for r in 0..w(8) {
        let at = w(9) + 32 * r;
        if u32::from_le_bytes(idx[at..at + 4].try_into().unwrap()) as u64 == off {
            return Ok(idx[at + 0x10 + j] as usize);
        }
    }
    Err(format!("no class record for the class at {off:#x}"))
}
