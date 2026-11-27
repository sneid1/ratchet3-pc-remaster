//! The emulated machine's EE: rac-rs's interpreter alone, or with the game's code statically recompiled to Rust (`--ee interp|rec`,
//! default rec). The game itself (`uya play`) runs on the port's own translator and runtime instead; this is the reference only.
//!
//! The recompiled crates are written from the player's disc by `scripts/uya-recgen.py` (generated/uya/, never committed) and are a
//! dependency of this tool: a build without them fails. The oracle runs a compiled function only while RAM holds exactly the code
//! it was made from (a hash of each function), the interpreter runs everything else, and both give the same RAM and the same
//! instruction count, so a run is the same run either way; `summary` and `ram_hash` show it.

use ee_oracle::Machine;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Interp,
    Rec,
}

/// `--ee interp|rec` from the command line (default `rec`).
pub fn mode(args: &[String]) -> Result<Mode, String> {
    match args.iter().position(|x| x == "--ee").map(|i| args.get(i + 1).map(String::as_str)) {
        None => Ok(Mode::Rec),
        Some(Some("rec")) => Ok(Mode::Rec),
        Some(Some("interp")) => Ok(Mode::Interp),
        Some(_) => Err("--ee interp|rec".into()),
    }
}

/// Registers the recompiled sets and turns the dispatcher on. The table is empty until the game has unpacked its code into RAM; the
/// oracle rebuilds it when a level loads and when code at a registered function changes (`FlushCache`).
pub fn enable(m: &mut Machine, mode: Mode) {
    if mode == Mode::Rec {
        ee_game_rc3::register();
        m.enable_rec();
    }
}

/// FNV-1a over the 32 MB of EE RAM, 8 bytes at a time.
pub fn ram_hash(m: &Machine) -> u64 {
    m.cpu.mem.ram[..32 << 20].chunks_exact(8).fold(0xcbf2_9ce4_8422_2325u64, |h, c| (h ^ u64::from_le_bytes(c.try_into().unwrap())).wrapping_mul(0x100_0000_01b3))
}

/// One line: the instructions of the run and who ran them. `Cpu::ri` counts every instruction the interpreter executed, `n_skip` the
/// idle vblank polls it skips and the estimates of non-exact hands; the rest of `icount` is compiled code. The share is of the
/// instructions actually run (skipped idle time left out).
pub fn summary(m: &Machine) -> String {
    let c = &m.cpu;
    let total = c.icount;
    let (interp, skipped) = (c.ri as u64, c.n_skip);
    let rec = total - interp - skipped;
    format!(
        "{} M insns of the clock: {} M idle/skipped, {} M run: {} M compiled ({:.1}%), {} M interpreted",
        total / 1_000_000,
        skipped / 1_000_000,
        (rec + interp) / 1_000_000,
        rec / 1_000_000,
        100.0 * rec as f64 / (rec + interp).max(1) as f64,
        interp / 1_000_000
    )
}

/// The field's GS jobs as the emulated DMA sent them (`jobs`, as rac-rs's pipe receives them) with the blocks the oracle's VIF1
/// trace recorded for each VIF1 chain (`blocks`, one list per chain in kick order), as the port's renderer takes a field's jobs.
pub fn frame_jobs(field: u64, jobs: &[ee_oracle::gs::Job], blocks: Vec<Vec<(usize, u32, u32, u32)>>) -> Result<uya::jobs::FrameJobs, String> {
    use uya::jobs::Job;
    let chains = jobs.iter().filter(|j| matches!(j, ee_oracle::gs::Job::Vif1(..))).count();
    if chains != blocks.len() {
        return Err(format!("jobs: field {field}: {chains} VIF1 chains sent, {} traced", blocks.len()));
    }
    let mut blocks = blocks.into_iter();
    let jobs = jobs
        .iter()
        .map(|j| match j {
            ee_oracle::gs::Job::Vif1(w, m, d) => Job::Vif1 { words: w.clone(), marks: m.clone(), blocks: blocks.next().expect("counted above"), dispfb: *d },
            ee_oracle::gs::Job::Gif(q, d) => Job::Gif { qs: q.clone(), dispfb: *d },
        })
        .collect();
    Ok(uya::jobs::FrameJobs { field, jobs, skin: Default::default() })
}
