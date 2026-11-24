//! `uyaref vucheck DISC RAM.bin[,RAM.bin...] LEVEL OUT.png [uya native's options]`: the redraw of `uya native`, with every pass of
//! the moby chain that runs a VU1 program the port does in its own code (the water's, `uya::vuwater`, and the sea's, `uya::vusea`)
//! also run on rac-rs's VIF1 and VU1 (the game's own microcode, uploaded by the pass's MPG packets from the disc's main program),
//! and the port's GIF output set against the VU1's word by word (equal; within rounding: floats within a few units in the last
//! place, integers within 1; or different), sorted by what each word is to the GS (a vertex's ST, RGBAQ or XYZF2, or a GIF tag), on
//! stderr. The renderer hands each pass to `check` through `uya::mobies::check_passes`, and each run of program 0 it replays on
//! its translated VU1 to `check_run` (`uya::mobies::check_runs`: the run again on rac-rs's VU1 from the same memory and registers,
//! one line a run, the words that differ printed); the game sets no check.

/// Turns the check on for the redraw that follows.
pub fn install() {
    uya::mobies::check_passes(check);
    uya::mobies::check_runs(check_run);
}

/// One run of program 0 the port replays on its translated VU1 (`uya::mobies::check_runs`), run again on rac-rs's VU1 from the
/// same start (program 0's microcode, the port's VU1 data memory and VF and VI registers), its GIF output set against the port's;
/// a run whose output differs beyond rounding is printed (`compare`), the others counted.
fn check_run(mem: &[[u32; 4]], vf: &[[u32; 4]; 32], vi: &[u32; 32], start: usize, top: usize, port: &[[u32; 4]], code: &[u8]) {
    use rac_engine::gsfull::vu1::{gif_packet, Path1, Vu1};
    struct Sink(Vec<[u32; 4]>);
    impl Path1 for Sink {
        fn xgkick(&mut self, data: &[[u32; 4]], addr: usize) {
            self.0.extend(gif_packet(data, addr));
        }
    }
    let mut vu = Vu1::new(&[], &[]);
    let pairs: Vec<u64> = code.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().expect("8 bytes"))).collect();
    vu.mpg(0, &pairs);
    vu.data = mem.to_vec();
    vu.vf = *vf;
    vu.vi[..16].copy_from_slice(&vi[..16]);
    let mut sink = Sink(Vec::new());
    if let Err(e) = vu.run(start, top as u32, 0, &mut sink) {
        eprintln!("run check: program 0 from {start:#x}, TOP {top:#x}: rac-rs's VU1: {e}");
        return;
    }
    let off = port.iter().zip(&sink.0).flat_map(|(a, b)| (0..4).map(move |j| (a[j], b[j]))).filter(|&(x, y)| x != y && (x as i32).abs_diff(y as i32) > 1 && (f32::from_bits(x) - f32::from_bits(y)).abs() > 1e-5 * f32::from_bits(y).abs().max(1.0)).count();
    eprintln!("run check: program 0 from {start:#x}, TOP {top:#x}: {} quadwords the port's, {} rac-rs's, {off} words different", port.len(), sink.0.len());
    if off > 0 || port.len() != sink.0.len() {
        compare(&format!("run of program 0 from {start:#x}, TOP {top:#x}"), port, &sink.0);
    }
}

/// One pass: its name (the programs it runs), its VIF stream and marks, and the port's GIF output.
fn check(name: &str, stream: &[u32], marks: &[(usize, u32)], port: &[[u32; 4]]) -> Result<(), String> {
    let vu1 = on_vu1(stream, marks)?;
    compare(name, port, &vu1);
    Ok(())
}

/// The pass run on rac-rs's VIF1 and VU1, its GIF output.
fn on_vu1(stream: &[u32], marks: &[(usize, u32)]) -> Result<Vec<[u32; 4]>, String> {
    use rac_engine::gsfull::{vif::{Out, Vif1}, vu1::Vu1};
    let (mut vif, mut vu, mut out) = (Vif1::new(), Vu1::new(&[], &[]), Out::default());
    vif.feed(stream, marks, &mut vu, &mut out);
    vif.flush_now(&mut vu);
    if let Some(e) = &vu.last_err {
        return Err(format!("vucheck: the VU1 pass at the chain's end: {e}"));
    }
    Ok(out.qs.iter().map(|&q| [q as u32, (q >> 32) as u32, (q >> 64) as u32, (q >> 96) as u32]).collect())
}

/// The port's output of a pass set against the VU1's, word by word, on stderr.
fn compare(name: &str, mine: &[[u32; 4]], theirs: &[[u32; 4]]) {
    // What each quadword of the VU1's stream is: the GIF tags and, in PACKED packets, the register each quadword writes.
    let mut role = vec!["A+D or IMAGE"; theirs.len()];
    // Where each quadword is: its packet's tag and the loop (the vertex) within it.
    let mut place = vec![(0usize, 0usize); theirs.len()];
    let mut k = 0;
    while k < theirs.len() {
        let t = theirs[k];
        let (lo, hi) = (t[0] as u64 | (t[1] as u64) << 32, t[2] as u64 | (t[3] as u64) << 32);
        let (nloop, flg, nreg) = ((lo & 0x7fff) as usize, (lo >> 58) & 3, match (lo >> 60) & 0xf { 0 => 16, r => r as usize });
        role[k] = "GIF tag";
        k += 1;
        let n = match flg {
            0 => nloop * nreg,
            1 => (nloop * nreg).div_ceil(2),
            _ => nloop,
        };
        for j in 0..n.min(theirs.len().saturating_sub(k)) {
            place[k + j] = (k - 1, j / nreg);
            if flg == 0 {
                role[k + j] = match (hi >> (4 * (j % nreg))) & 0xf {
                    1 => "RGBAQ",
                    2 => "ST",
                    4 => "XYZF2",
                    0xe => "A+D",
                    _ => "other register",
                };
            }
        }
        k += n;
    }
    let mut tally: std::collections::BTreeMap<(&str, usize), (usize, usize, usize)> = std::collections::BTreeMap::new();
    let mut shown = 0;
    for (q, (a, b)) in mine.iter().zip(theirs).enumerate() {
        for j in 0..4 {
            let (x, y) = (a[j], b[j]);
            let e = tally.entry((role[q], j)).or_default();
            if x == y {
                e.0 += 1;
                continue;
            }
            let near = (x as i32).abs_diff(y as i32) <= 1 || (f32::from_bits(x) - f32::from_bits(y)).abs() <= 1e-5 * f32::from_bits(y).abs().max(1.0);
            if near {
                e.1 += 1;
            } else {
                e.2 += 1;
            }
            if shown < 12 && !(role[q] == "ST" && j == 3) {
                eprintln!("{name} check:   quadword {q} ({} of loop {} of the packet at {}) word {j}: {x:#010x} against {y:#010x}", role[q], place[q].1, place[q].0);
                shown += 1;
            }
        }
    }
    eprintln!("{name} check: {} quadwords the port's, {} the VU1's", mine.len(), theirs.len());
    for ((r, j), (equal, near, off)) in tally {
        if near + off > 0 {
            eprintln!("{name} check:   {r} word {j}: {equal} equal, {near} within rounding, {off} different");
        }
    }
}
