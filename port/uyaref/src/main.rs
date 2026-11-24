//! The emulated reference (plan item 0.6): rac-rs's PS2 core (ee-oracle, rac-engine's GS-full pipeline and SPU2 mixer) running the
//! game's code as `scripts/uya-recgen.py` recompiled it (generated/uya), and the instruments that set the port against it. The game
//! (`uya play`, port/uya) has none of this: it runs on the port's own translator and runtime. This tool is built only when asked
//! for (`scripts\build.ps1 -Package uyaref`; port/Cargo.toml's default-members leave it out) and is deleted once nothing needs it.
//!
//!   uyaref frames <disc.iso> [--fields N] [--every K] [--save-from F] [--out DIR] [--press FIELD:BUTTONS:FIELDS,...]
//!       cold-boots the disc on the emulated machine, draws every field with rac-rs's GS-full pipeline and saves the picture every K
//!       fields (from field F, default 0);
//!       --press holds pad buttons (hex, Machine::set_pad's bits: 0x800 Start, 0x40 Cross) from FIELD for FIELDS fields;
//!       --poke FIELD:ADDR:BYTES,... writes the bytes (hex, in RAM order) at EE address ADDR before FIELD runs (the hero put
//!       somewhere: the player's position, 0x1A4C60 on level 28);
//!       --catch-from FIELD single-steps from that field until a memory fault and prints the last 64 instructions;
//!       --wav FILE also records the sound (as `sound` does); --movies plays the game's movies into the capture (every
//!       `every`-th picture as DIR/mNN_fFFFFF.png, their sound in the WAV; the press script then runs on the run's clock,
//!       fields plus movie time, and Start during a movie skips it); without it every movie counts as skipped with Start.
//!       --warp LEVEL@FIELD,...: after each such field, asks the game for level LEVEL through its own level change (ee-oracle
//!       rc3/warp.rs); every saved picture then also prints the level being played.
//!       --tour LEVEL,...@FIELD:STAY (with --native): at FIELD asks for the first level, and asks for each next one once the level
//!       being played is the one asked for, the game's camera is set and the level has been played that way for STAY fields
//!       (a level's intro scene runs as long as it runs: the next level is asked for only when the level's own code is in RAM,
//!       retried every field until it is); the run ends STAY fields after the last level is reached, or with an error when a
//!       level asked for is not reached within 20000 fields. One run over every level.
//!       Textures (PCSX2's names; rac-rs texpack/live.rs, gsfull/gs/texhook.rs):
//!       --dump DIR writes every texture the game draws once, as DIR/<PCSX2 name>.png (an existing file is kept);
//!       --replace DIR draws the replacement images under DIR (a PCSX2 replacements folder, searched recursively) in place of
//!       the textures they name; --areas NAME:FROM-TO,... samples which textures the shown picture is made of, every
//!       --cover-every N fields (default 4) from field FROM to TO of each area. Any of them writes OUT/textures.tsv (every name
//!       drawn), with --areas OUT/coverage.tsv (per area: each texture's share of the picture) and with --replace
//!       OUT/replacements.tsv (each replacement looked up: used, or why it was refused). With --areas, every saved picture inside
//!       an area also gets OUT/ids-fNNNNN.png: each texture in its own colour, untextured black, unnamed render targets magenta,
//!       textures drawn with a replacement in green.
//!       --native H also draws the world with the port's own renderer (`uya::native`): at every saved field of a level whose camera
//!       is known, from the field's GS jobs and the EE RAM they were built from (the field before's), offscreen, H pixels high;
//!       saved as DIR/fNNNNN-native.png, and DIR/fNNNNN-sheet.png puts the emulated picture of the field that shows that frame (the
//!       next one, stretched to the same size), the native one and the two blended half and half side by side.
//!       --trace FIELD writes that field's VIF1 traffic to DIR/trace-fNNNNN.bin (`write_trace`): every VIF1 job's words with the
//!       EE address of each DMA block, each UNPACK into VU1 memory, each VU1 program run and DIRECT transfer, and the field's GS
//!       qwords; the ground truth of how the game reads its level data for drawing (scripts/uya_trace.py reads it).
//!       --ee interp|rec runs the game's EE code on rac-rs's interpreter alone or statically recompiled (default rec; src/ee.rs);
//!       every saved picture also prints a hash of the 32 MB of EE RAM and the instructions run compiled and interpreted;
//!       --vu1 interp|aot|diff runs the eight VU1 programs on rac-rs's interpreter, compiled ahead of time (default) or both;
//!       --ram-dir DIR also writes, per saved field, its GS jobs to DIR/jobsNNNNN.bin and the EE RAM they were built from to
//!       DIR/ramNNNNN.bin (what `uya native` redraws).
//!
//!   uyaref sound <disc.iso> [--fields N] [--wav FILE] [--press FIELD:BUTTONS:FIELDS,...]
//!       cold-boots the disc without drawing and records the game's sound: the SPU2 writes of the oracle's 989snd driver,
//!       mixed offline on the 48 SPU2 voices (rac-engine's spumix) into a 48 kHz stereo WAV (default sound.wav).
//!
//!   uyaref vucheck <disc.iso> <ram.bin>[,<ram.bin>...] <level> <out.png> [uya native's options]
//!       `uya native`'s redraw with each water and sea pass also run on rac-rs's VIF1 and VU1 and the two outputs compared
//!       (src/vucheck.rs).
//!
//!   uyaref sndtrace | sndemu | sndcheck | sndlfo | sndrev | sndtour ...: the emulated sound beside the port's engine (src/snd.rs,
//!       src/sndtour.rs; each says what it takes).
//!
//!   uyaref mpegcmp <disc.iso> <sector> <bytes> [--out DIR] [--every N] [--lang L] | uyaref mpegcmp <disc.iso> all: the port's
//!       movie decoder beside rac-engine's (src/mpegcmp.rs).

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;

mod ee;
mod frame_control;
mod movie;
mod mpegcmp;
mod record;
mod snd;
mod sndtour;
mod vucheck;

/// The texture options of `frames`.
#[derive(Default)]
struct TexArgs {
    dump: Option<String>,
    replace: Option<String>,
    /// (name, first field, last field).
    areas: Vec<(String, u64, u64)>,
    cover_every: u64,
}

impl TexArgs {
    fn on(&self) -> bool {
        self.dump.is_some() || self.replace.is_some() || !self.areas.is_empty()
    }
}

/// Coverage of one area: samples taken, pixels sampled, and per texture id the pixels and the samples it was in.
#[derive(Default)]
struct Area {
    samples: u64,
    pixels: u64,
    size: (u32, u32),
    ids: HashMap<u32, (u64, u64)>,
}

/// Writes OUT/textures.tsv, OUT/coverage.tsv (with areas) and OUT/replacements.tsv (with a replacement folder).
fn write_tex_reports(gs: &rac_engine::gsfull::gs::Gs, out: &str, t: &TexArgs, areas: &[Area]) -> Result<(), String> {
    use rac_engine::gsfull::gs::RT;
    let names = gs.tex_names();
    let repl_of = |s: &rac_engine::gsfull::gs::Seen| s.repl.as_ref().map_or("-".to_string(), |r| r.file.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned()));
    let mut seen_in: HashMap<u32, Vec<&str>> = HashMap::new();
    for ((name, _, _), a) in t.areas.iter().zip(areas) {
        for &id in a.ids.keys() {
            seen_in.entry(id).or_default().push(name);
        }
    }
    let mut s = String::from("id\tstem\tpsm\tw\th\tprims\treplacement\tareas\n");
    for (i, n) in names.iter().enumerate() {
        let ar = seen_in.get(&(i as u32 + 1)).map_or("-".to_string(), |v| v.join(","));
        let _ = writeln!(s, "{}\t{}\t{:#x}\t{}\t{}\t{}\t{}\t{ar}", i + 1, n.stem, n.psm, n.w, n.h, n.prims, repl_of(n));
    }
    let path = format!("{out}/textures.tsv");
    std::fs::write(&path, s).map_err(|e| format!("{path}: {e}"))?;
    if !t.areas.is_empty() {
        let mut s = String::from("# share: mean share of the shown picture over the area's samples (%); seen: samples it was in (%)\n");
        for ((name, from, to), a) in t.areas.iter().zip(areas) {
            let _ = writeln!(s, "# area {name}: fields {from}-{to}, {} samples of {}x{}", a.samples, a.size.0, a.size.1);
        }
        s.push_str("area\tstem\tpsm\tw\th\tshare\tseen\treplacement\n");
        for ((name, _, _), a) in t.areas.iter().zip(areas) {
            let mut v: Vec<(&u32, &(u64, u64))> = a.ids.iter().collect();
            v.sort_by(|x, y| y.1 .0.cmp(&x.1 .0).then(x.0.cmp(y.0)));
            for (&id, &(px, n)) in v {
                let share = 100.0 * px as f64 / a.pixels.max(1) as f64;
                let seen = 100.0 * n as f64 / a.samples.max(1) as f64;
                let (stem, psm, w, h, r) = match id {
                    0 => ("(untextured)".to_string(), "-".to_string(), 0, 0, "-".to_string()),
                    RT => ("(render target)".to_string(), "-".to_string(), 0, 0, "-".to_string()),
                    _ => {
                        let n = &names[id as usize - 1];
                        (n.stem.clone(), format!("{:#x}", n.psm), n.w, n.h, repl_of(n))
                    }
                };
                let _ = writeln!(s, "{name}\t{stem}\t{psm}\t{w}\t{h}\t{share:.4}\t{seen:.1}\t{r}");
            }
        }
        let path = format!("{out}/coverage.tsv");
        std::fs::write(&path, s).map_err(|e| format!("{path}: {e}"))?;
    }
    if let Some(live) = gs.tex_live() {
        let mut s = String::from("stem\tstate\tfile_w\tfile_h\torig_w\torig_h\tfile\n");
        for o in live.outcomes() {
            let _ = writeln!(s, "{}\t{}\t{}\t{}\t{}\t{}\t{}", o.stem, o.state, o.file_size.0, o.file_size.1, o.rect_size.0, o.rect_size.1, o.file.display());
        }
        let path = format!("{out}/replacements.tsv");
        std::fs::write(&path, s).map_err(|e| format!("{path}: {e}"))?;
    }
    Ok(())
}

/// `--tour LEVEL,...@FIELD:STAY`.
struct Tour {
    levels: Vec<u32>,
    first: u64,
    stay: u64,
}

/// The level whose world a field's RAM shows, as this emulator's run can tell without the port's table of the programs in RAM
/// (the game itself takes the level whose code is in RAM: `uyart::RamCopy::level`): the level being played; while a level
/// change fades the old one out (the new one's camera not set yet), the level being left.
fn world_level(n: &uya::native::Native, ram: &[u8]) -> u32 {
    let word = |a: u32| u32::from_le_bytes(ram[a as usize..][..4].try_into().expect("4 bytes"));
    match word(uya::LEVEL_CURRENT) {
        l if n.camera(ram, l).is_err() && n.camera(ram, word(uya::LEVEL_LEFT)).is_ok() => word(uya::LEVEL_LEFT),
        l => l,
    }
}

/// Cold-boots the disc and draws every field with rac-rs's GS-full pipeline (VIF1 + VU1 + software GS); every `every`
/// fields the displayed picture goes to `out/fNNNNN.png`.
#[allow(clippy::too_many_arguments)]
fn frames(iso: &str, fields: u64, every: u64, save_from: u64, out: &str, presses: &[(u64, u16, u64)], catch_from: Option<u64>, wav: Option<&str>, movies: bool, tex: &TexArgs, vu1: rac_engine::gsfull::vu1::AotMode, hash_from: Option<u64>, ee: ee::Mode, ram_dir: Option<&str>, warps: &[(u32, u64)], native_h: Option<u32>, trace: Option<u64>, tour: Option<&Tour>) -> Result<(), String> {
    use rac_engine::gsfull::{crtc::{present, Crtc}, gs::{Gs, TexOpts}, pipe};
    use rac_engine::texpack::live::{Dumper, Live};
    let mut m = ee_oracle::Machine::cold(iso, "")?;
    ee::enable(&mut m, ee);
    // --stick FIELD:LX,LY:FIELDS,...: the left stick held at the pad report's bytes LX, LY (hex: x 0 left to FF right, y 0
    // forward to FF back; 80 centred) from FIELD for FIELDS fields, so a run can walk the hero where a press cannot.
    let sticks: Vec<(u64, u8, u8, u64)> = std::env::args().skip_while(|x| x != "--stick").nth(1).map(|s| {
        s.split(',').collect::<Vec<_>>().chunks(2).map(|c| {
            let (a, b) = (c[0].split(':').collect::<Vec<_>>(), c[1].split(':').collect::<Vec<_>>());
            (a[0].parse().expect("--stick FIELD"), u8::from_str_radix(a[1], 16).expect("--stick LX"), u8::from_str_radix(b[0], 16).expect("--stick LY"), b[1].parse().expect("--stick FIELDS"))
        }).collect()
    }).unwrap_or_default();
    let stick_at = |f: u64| sticks.iter().find(|&&(at, _, _, n)| f >= at && f < at + n).map_or([0x80, 0x80, 0x80, 0x80], |&(_, x, y, _)| [0x80, 0x80, x, y]);
    // --poke FIELD:ADDR:BYTES,...: the bytes (hex, in RAM order) written at EE address ADDR (hex) before FIELD runs, as the port's
    // control channel's `write` does (the hero put where a press cannot take him: the player's position, 0x1A4C60 on level 28).
    let pokes: Vec<(u64, usize, Vec<u8>)> = std::env::args().skip_while(|x| x != "--poke").nth(1).map(|s| {
        s.split(',').map(|p| {
            let v: Vec<&str> = p.split(':').collect();
            let hex = v[2];
            assert!(hex.len() % 2 == 0, "--poke BYTES: an even number of hex digits");
            (v[0].parse().expect("--poke FIELD"), usize::from_str_radix(v[1], 16).expect("--poke ADDR"), (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("--poke BYTES")).collect())
        }).collect()
    }).unwrap_or_default();
    if movies {
        movie::capture(iso, out, every, presses);
    }
    let mut rec = wav.map(|_| record::Recorder::new(&mut m));
    let (jtx, jrx) = std::sync::mpsc::channel::<ee_oracle::gs::Job>();
    m.cpu.mem.gsq = Some(jtx);
    let mut p = pipe::Pipe::new("").ok_or("GS-full pipe: no VU1 state")?;
    // At most 4 raster workers: a headless capture shares the PC with everything else.
    let mut gs = Gs::new(vec![0u8; 4 << 20], pipe::threads().min(4));
    std::fs::create_dir_all(out).map_err(|e| format!("{out}: {e}"))?;
    if tex.on() {
        // Replacements are kept from the level at most twice the original's size (the software GS draws at the original
        // resolution), within 1 GB of decoded images.
        let live = tex.replace.as_deref().map(|d| Live::new(Path::new(d), Some(2.0), 1 << 30)).transpose()?;
        let dump = tex.dump.as_deref().map(|d| Dumper::new(Path::new(d))).transpose()?;
        gs.textures(TexOpts { dump, live, cover: !tex.areas.is_empty() });
    }
    p.vu.aot_mode = vu1;
    let mut native = native_h.map(|h| uya::native::Native::open(iso, h)).transpose()?;
    if tour.is_some() && native.is_none() {
        return Err("--tour needs --native (a level counts as reached when its camera is set)".into());
    }
    // The tour: how many of its levels were asked for, the field the level asked for last was reached at, and whether a
    // refused request for the next one was already printed.
    let (mut toured, mut reached, mut refused, mut asked) = (0usize, None::<u64>, false, 0u64);
    let mut areas: Vec<Area> = tex.areas.iter().map(|_| Area::default()).collect();
    let mut vu_errors_shown = 0u64;
    let t0 = std::time::Instant::now();
    // The EE RAM at the end of a field whose next field is saved, and a native picture waiting for the emulated picture of the
    // field after its own (its field and level, the picture and its line).
    let mut ram_next: Option<Vec<u8>> = None;
    // The fields saved: every `every`-th from `save_from`.
    let saved = |f: u64| f >= save_from && (f - save_from) % every == 0;
    let mut pending: Option<(u64, uya::pic::Image, String)> = None;
    // With --native, the field before's jobs, VIF1 blocks and the RAM they were built from.
    let mut prev: Option<(u64, uya::jobs::FrameJobs, Option<Vec<u8>>)> = None;
    // Time per stage since the last saved picture: the EE field, VIF1 + VU1, the GS (raster finished).
    // --step-from F stops before field F and reads run/press, ram/write, shot
    // and quit on stdin. Shots use the emulated GS at the last completed field.
    let mut control = std::env::args().skip_while(|arg| arg != "--step-from").nth(1)
        .map(|value| value.parse::<u64>()).transpose().map_err(|e| e.to_string())?.map(frame_control::Control::new);
    let mut ns = [0u64; 3];
    for f in 1..=fields {
        // Scripted pad: the buttons of every press whose fields include this one, the left stick as --stick holds it.
        let mut buttons = uya::pad_at(presses, movie::at_field(f));
        if let Some(ctl) = &mut control {
            let crtc = Crtc::from_regs(&m.cpu.mem.gs);
            let Some(held) = ctl.step(f, &mut m.cpu.mem.ram, |path| {
                let pic = present(&mut gs, &crtc);
                let rgb = pic.rgba.chunks_exact(4).flat_map(|c| [c[0], c[1], c[2]]).collect();
                let img = uya::pic::Image { w: pic.w as usize, h: pic.h as usize, rgb };
                std::fs::write(path, uya::pic::png(&img)).map_err(|e| format!("{path}: {e}"))
            })? else { break };
            buttons |= held;
        }
        m.set_pad(buttons, stick_at(f));
        for (_, at, bytes) in pokes.iter().filter(|p| p.0 == f) {
            m.cpu.mem.ram[*at..*at + bytes.len()].copy_from_slice(bytes);
            println!("field {f}: poke {at:#x} {}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>());
        }
        if catch_from == Some(f) {
            // From here the host steps the CPU itself, one instruction at a time, until a memory fault or a stop, and prints
            // the last 64 instructions. The game thread stays parked in its vblank callback for good (the run ends here).
            // Nothing drains the GS jobs from here on: the game must stop producing them, or every field's ~1 MB of jobs
            // piles up in the channel for as long as the stepping runs.
            m.cpu.mem.gsq = None;
            drop(jrx);
            let c = &mut **m.cpu;
            let mut ring = std::collections::VecDeque::new();
            // UYA_RING=N instructions kept (default 64, at most 1,000,000: about 56 MB).
            let ring_n: usize = std::env::var("UYA_RING").ok().and_then(|v| v.parse().ok()).unwrap_or(64).min(1_000_000);
            // UYA_WATCH=ADDR (hex): every change of that word, with the instruction that made it.
            let watch = std::env::var("UYA_WATCH").ok().map(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).expect("hex address"));
            for _ in 0..40_000_000_000u64 {
                ring.push_back((c.pc, c.mem.rd32(c.pc), [2, 3, 4, 5, 6, 16, 17, 18, 19, 29, 31].map(|r| c.g(r) as u32)));
                if ring.len() > ring_n {
                    ring.pop_front();
                }
                let watched = watch.map(|a| c.mem.rd32(a));
                let (wpc, wra) = (c.pc, c.g(31) as u32);
                c.step();
                if let (Some(a), Some(old)) = (watch, watched) {
                    let now = c.mem.rd32(a);
                    if now != old {
                        println!("watch {a:08x}: {old:08x} -> {now:08x} by pc {wpc:08x} (ra {wra:08x}, icount {})", c.icount);
                    }
                }
                if c.mem.fault.is_some() || c.stop.is_some() {
                    // UYA_RING_CALLS=1: only the calls and returns (jal, jalr, jr) of the ring.
                    let calls_only = std::env::var("UYA_RING_CALLS").is_ok();
                    for (pc, w, r) in ring.iter().filter(|(_, w, _)| !calls_only || w >> 26 == 3 || (w >> 26 == 0 && (w & 0x3f == 8 || w & 0x3f == 9))) {
                        println!("{pc:08x} {w:08x} v0 {:x} v1 {:x} a0 {:x} a1 {:x} a2 {:x} s0 {:x} s1 {:x} s2 {:x} s3 {:x} sp {:x} ra {:x}", r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7], r[8], r[9], r[10]);
                    }
                    return Err(format!("{:?} {:?}", c.mem.fault, c.stop));
                }
            }
            return Err("no fault within 4e10 instructions".into());
        }
        // A saved field keeps its GS jobs as the DMA sent them (`jobs.rs`), with each VIF1 chain's blocks from the oracle's trace,
        // and the EE RAM they were built from: the EE builds a frame's chains in one field and the DMA sends them in the next
        // (the moby chain the record list in RAM names at a field's end is in the next field's jobs), so the RAM is the field
        // before's. The game shows that frame one field later still (each frame's first job copies the frame drawn before it
        // to the display buffer), so the sheet takes the emulated picture of the field after.
        let keep_jobs = saved(f) && (ram_dir.is_some() || native.is_some());
        // With --native every field's VIF1 blocks are traced: a field that reads the frame back is drawn natively with the one
        // before it (below).
        if trace == Some(f) || keep_jobs || native.is_some() {
            ee_oracle::gs::trace_start();
        }
        if trace == Some(f) {
            p.trace = true;
        }
        let t = std::time::Instant::now();
        if let Err(e) = m.frame() {
            // The game stopped: where, the registers, and (env UYA_RAM_OUT=FILE) the 32 MB of EE RAM.
            let c = &m.cpu;
            let regs: Vec<String> = (1..32).map(|r| format!("r{r}={:x}", c.g(r) as u32)).collect();
            eprintln!("field {f}: pc {:08x}
{}", c.pc, regs.join(" "));
            if let Ok(path) = std::env::var("UYA_RAM_OUT") {
                std::fs::write(&path, &c.mem.ram[..32 << 20]).map_err(|e| format!("{path}: {e}"))?;
            }
            return Err(e);
        }
        ns[0] += t.elapsed().as_nanos() as u64;
        // The RAM the next field's jobs are built from.
        let ram_built = std::mem::take(&mut ram_next);
        if native.is_some() || (saved(f + 1) && ram_dir.is_some()) {
            ram_next = Some(m.cpu.mem.ram[..32 << 20].to_vec());
        }
        if let Some(r) = &mut rec {
            r.field(&mut m);
        }
        if let Some(&(level, _)) = warps.iter().find(|&&(_, at)| at == f) {
            let w = ee_oracle::rc3::warp::warp(&mut m.cpu, level)?;
            println!("field {f}: warp from level {} to {level}{}", w.from, if w.mapped != level { format!(" (the story state mapped it to {})", w.mapped) } else { String::new() });
        }
        if let (Some(t), Some(n)) = (tour, &native) {
            let current = m.cpu.mem.rd32(ee_oracle::rc3::warp::LEVEL_CURRENT);
            if toured > 0 && reached.is_none() && current == t.levels[toured - 1] && n.camera(&m.cpu.mem.ram, current).is_ok() {
                reached = Some(f);
                println!("field {f}: tour: level {current} reached");
            }
            if toured > 0 && reached.is_none() && f >= asked + 20_000 {
                return Err(format!("field {f}: tour: level {} asked for at field {asked} is not reached (the game's camera is not set)", t.levels[toured - 1]));
            }
            let due = if toured == 0 { f >= t.first } else { reached.is_some_and(|r| f >= r + t.stay) };
            if due && toured == t.levels.len() {
                println!("field {f}: tour: every level reached");
                break;
            }
            if due {
                let level = t.levels[toured];
                match ee_oracle::rc3::warp::warp(&mut m.cpu, level) {
                    Ok(w) => {
                        println!("field {f}: tour: warp from level {} to {level}{}", w.from, if w.mapped != level { format!(" (the story state mapped it to {})", w.mapped) } else { String::new() });
                        (toured, reached, refused, asked) = (toured + 1, None, false, f);
                    }
                    Err(e) if !refused => {
                        println!("field {f}: tour: {e}; asking again every field");
                        refused = true;
                    }
                    Err(_) => {}
                }
            }
        }
        let crtc = Crtc::from_regs(&m.cpu.mem.gs);
        let t = std::time::Instant::now();
        let field_jobs: Vec<ee_oracle::gs::Job> = jrx.try_iter().collect();
        let blocks = (trace == Some(f) || keep_jobs || native.is_some()).then(ee_oracle::gs::trace_take);
        let frame_jobs = if keep_jobs { Some(ee::frame_jobs(f, &field_jobs, blocks.clone().expect("traced above"))?) } else { None };
        // A field whose jobs read the frame back from the GS (the battle-missions menu, as it opens, keeps the picture it opens over
        // and puts it back each frame as its background): the native renderer draws the field before and this one, so that it
        // grabs the picture the game read (a saved field is drawn below anyway).
        let this_field = if native.is_some() { Some(ee::frame_jobs(f, &field_jobs, blocks.clone().expect("traced with --native"))?) } else { None };
        if let (Some(n), Some(ram), Some(now)) = (&mut native, ram_built.as_deref(), &this_field) {
            let reads = n.reads_frame_back(now);
            if let Err(e) = &reads {
                println!("field {f}: whether the jobs read the frame back: {e}");
            }
            if reads == Ok(true) {
                match &prev {
                    Some((pf, before, Some(pram))) if pf + 1 == f => {
                        let r = n.draw(pram, before, world_level(n, pram)).and_then(|_| {
                            if saved(f) {
                                return Ok(());
                            }
                            n.draw(ram, now, world_level(n, ram)).map(|_| ())
                        });
                        match r {
                            Ok(()) => println!("field {f}: the jobs read the frame back: fields {pf} and {f} drawn natively"),
                            Err(e) => println!("field {f}: the jobs read the frame back; the native draw: {e}"),
                        }
                    }
                    _ => println!("field {f}: the jobs read the frame back and the field before was not kept"),
                }
            }
        }
        let field = p.process(field_jobs, crtc);
        ns[1] += t.elapsed().as_nanos() as u64;
        if trace == Some(f) {
            let path = format!("{out}/trace-f{f:05}.bin");
            write_trace(&path, f, blocks.expect("traced above"), &field)?;
            println!("field {f}: trace -> {path} ({} VIF1 jobs, {} GS qwords)", field.trace.len(), field.qs.len());
        }
        if hash_from.is_some_and(|h| f >= h) {
            // FNV-1a over the field's GS packet stream, to compare the interpreter with the native VU1 programs.
            let h = field.qs.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, q| (((h ^ *q as u64).wrapping_mul(0x100_0000_01b3)) ^ (*q >> 64) as u64).wrapping_mul(0x100_0000_01b3));
            println!("qs {f} {h:016x} {}", field.qs.len());
        }
        let t = std::time::Instant::now();
        pipe::raster(&mut gs, &field);
        gs.finish();
        ns[2] += t.elapsed().as_nanos() as u64;
        for (i, &(_, from, to)) in tex.areas.iter().enumerate() {
            if f >= from && f <= to && (f - from) % tex.cover_every == 0 {
                if let Some((w, h, ids)) = gs.cover_ids(&crtc) {
                    let a = &mut areas[i];
                    a.samples += 1;
                    a.pixels += ids.len() as u64;
                    a.size = (w, h);
                    let mut count: HashMap<u32, u64> = HashMap::new();
                    for id in ids {
                        *count.entry(id).or_default() += 1;
                    }
                    for (id, n) in count {
                        let e = a.ids.entry(id).or_default();
                        e.0 += n;
                        e.1 += 1;
                    }
                }
            }
        }
        if saved(f) && tex.areas.iter().any(|&(_, from, to)| f >= from && f <= to) {
            if let Some((w, h, ids)) = gs.cover_ids(&crtc) {
                let names = gs.tex_names();
                let rgb: Vec<u8> = ids
                    .iter()
                    .flat_map(|&id| match id {
                        0 => [0, 0, 0],
                        rac_engine::gsfull::gs::RT => [255, 0, 255],
                        _ if names[id as usize - 1].repl.is_some() => [40, 220, 60],
                        _ => {
                            let x = id.wrapping_mul(0x9e37_79b9);
                            [(x >> 24) as u8 | 0x40, (x >> 16) as u8 | 0x40, (x >> 8) as u8 & 0x7f]
                        }
                    })
                    .collect();
                let img = uya::pic::Image { w: w as usize, h: h as usize, rgb };
                let path = format!("{out}/ids-f{f:05}.png");
                std::fs::write(&path, uya::pic::png(&img)).map_err(|e| format!("{path}: {e}"))?;
            }
        }
        if let Some((at, npic, line)) = pending.take() {
            let pic = present(&mut gs, &crtc);
            let img = uya::pic::Image { w: pic.w as usize, h: pic.h as usize, rgb: pic.rgba.chunks_exact(4).flat_map(|c| [c[0], c[1], c[2]]).collect() };
            let path = format!("{out}/f{at:05}-sheet.png");
            std::fs::write(&path, uya::pic::png(&uya::native::sheet(&img, &npic))).map_err(|e| format!("{path}: {e}"))?;
            println!("{line}, {path} (the game's picture of field {f})");
        }
        if saved(f) {
            let pic = present(&mut gs, &crtc);
            let rgb: Vec<u8> = pic.rgba.chunks_exact(4).flat_map(|c| [c[0], c[1], c[2]]).collect();
            let img = uya::pic::Image { w: pic.w as usize, h: pic.h as usize, rgb };
            let path = format!("{out}/f{f:05}.png");
            std::fs::write(&path, uya::pic::png(&img)).map_err(|e| format!("{path}: {e}"))?;
            println!(
                "field {f}: {}x{} -> {path} ({} GS qwords, VU1 errors {}, {:.1}s, {} M insns; ms per field: EE {:.1}, VIF1+VU1 {:.1}, GS {:.1})",
                pic.w,
                pic.h,
                field.qs.len(),
                field.vu_errors,
                t0.elapsed().as_secs_f64(),
                m.cpu.icount / 1_000_000,
                ns[0] as f64 / 1e6 / every as f64,
                ns[1] as f64 / 1e6 / every as f64,
                ns[2] as f64 / 1e6 / every as f64
            );
            ns = [0; 3];
            println!("field {f}: EE RAM {:016x}; {}", ee::ram_hash(&m), ee::summary(&m));
            // The RAM this field's jobs were built from (none when the run starts at this field).
            let built = ram_built.as_deref();
            if let (Some(n), Some(ram)) = (&mut native, built) {
                let level = world_level(n, ram);
                // A level whose native draw fails prints why and the run goes on, so one run over every level shows every failure.
                let fj = frame_jobs.as_ref().expect("kept at every saved field with --native");
                match n.camera(ram, level).and_then(|cam| Ok((cam, std::time::Instant::now(), n.draw(ram, fj, level)?))) {
                    Ok((cam, t, npic)) => {
                        let path = format!("{out}/f{f:05}-native.png");
                        std::fs::write(&path, uya::pic::png(&npic)).map_err(|e| format!("{path}: {e}"))?;
                        let line = format!(
                            "field {f}: native level {level}: {}x{} -> {path} ({:.1} ms; camera eye {:?} forward {:?} up {:?} tangents {:?})",
                            npic.w,
                            npic.h,
                            t.elapsed().as_secs_f64() * 1e3,
                            cam.eye,
                            cam.forward,
                            cam.up,
                            cam.tan
                        );
                        pending = Some((f, npic, line));
                    }
                    Err(e) => println!("field {f}: {e}"),
                }
            }
            if let (Some(dir), Some(ram)) = (ram_dir, built) {
                std::fs::create_dir_all(dir).map_err(|e| format!("{dir}: {e}"))?;
                let path = format!("{dir}/ram{f:05}.bin");
                std::fs::write(&path, ram).map_err(|e| format!("{path}: {e}"))?;
                frame_jobs.as_ref().expect("kept at every saved field with --ram-dir").write(&format!("{dir}/jobs{f:05}.bin"))?;
            }
            if tex.on() {
                write_tex_reports(&gs, out, tex, &areas)?;
            }
            if !warps.is_empty() {
                println!("field {f}: level {}", m.cpu.mem.rd32(ee_oracle::rc3::warp::LEVEL_CURRENT));
            }
            if field.vu_errors > vu_errors_shown {
                vu_errors_shown = field.vu_errors;
                println!("field {f}: last VU1 error: {}", p.vu.last_err.as_deref().unwrap_or("-"));
            }
        }
        if let Some(jobs) = this_field {
            prev = Some((f, jobs, ram_built));
        }
    }
    let a = &p.vu.aot;
    println!(
        "VU1 runs ({:?}): {} native ({} of them bailed to the interpreter), {} not covered, {} compared, {} differed",
        vu1,
        a.native,
        a.bails,
        a.interp,
        a.compared,
        a.differed
    );
    for ((id, pc), n) in &a.bail_at {
        println!("  bail: program {id} at pc {pc:04x}: {n}");
    }
    for l in &p.vu.diff_log {
        println!("  {l}");
    }
    if tex.on() {
        write_tex_reports(&gs, out, tex, &areas)?;
        let repl = gs.tex_live().map_or(String::new(), |l| {
            let ok = l.outcomes().iter().filter(|o| o.state == "ok").count();
            format!(", replacements: {ok} drawn, {} refused, {} MB decoded", l.outcomes().len() - ok, l.used() >> 20)
        });
        let dumps = gs.tex_dumper().map_or(String::new(), |d| format!(", dumps: {} written, {} already there", d.written, d.kept));
        println!("textures: {} names drawn{dumps}{repl}", gs.tex_names().len());
        for e in gs.tex_errors().iter().take(10) {
            eprintln!("textures: {e}");
        }
    }
    if let (Some(r), Some(path)) = (rec, wav) {
        r.write(path)?;
    }
    Ok(())
}

/// The trace file of `--trace` (little-endian u32 words): "UYAT", version 1, the field, the job count; per VIF1 job its word count
/// and words, its DMA block count and blocks (first data word, EE source address, qwords, tag id; 99 a normal transfer), its event
/// count and events (8 words each, `Out::trace`); then the field's GS qword count and qwords (4 words each) and its segment count
/// and segments (first qword, path, VU1 program). Each job's blocks are the oracle's record of the chain the pipe ran as that job.
fn write_trace(path: &str, f: u64, blocks: Vec<Vec<(usize, u32, u32, u32)>>, field: &rac_engine::gsfull::pipe::Field) -> Result<(), String> {
    if blocks.len() != field.trace.len() {
        return Err(format!("trace: {} VIF1 chains recorded by the oracle, {} jobs run by the pipe", blocks.len(), field.trace.len()));
    }
    let mut w: Vec<u32> = vec![u32::from_le_bytes(*b"UYAT"), 1, f as u32, blocks.len() as u32];
    for (b, (words, ev)) in blocks.iter().zip(&field.trace) {
        w.push(words.len() as u32);
        w.extend_from_slice(words);
        w.push(b.len() as u32);
        for &(at, src, q, id) in b {
            w.extend([at as u32, src, q, id]);
        }
        w.push(ev.len() as u32);
        for e in ev {
            w.extend_from_slice(e);
        }
    }
    w.push(field.qs.len() as u32);
    for q in &field.qs {
        w.extend([*q as u32, (*q >> 32) as u32, (*q >> 64) as u32, (*q >> 96) as u32]);
    }
    w.push(field.segs.len() as u32);
    for &(s, path, prog) in &field.segs {
        w.extend([s, path as u32, prog as u32]);
    }
    let bytes: Vec<u8> = w.iter().flat_map(|x| x.to_le_bytes()).collect();
    std::fs::write(path, bytes).map_err(|e| format!("{path}: {e}"))
}

/// Cold-boots the disc without drawing (no GS jobs are made) and records the sound of `fields` fields into `wav`.
fn sound(iso: &str, fields: u64, wav: &str, presses: &[(u64, u16, u64)]) -> Result<(), String> {
    let mut m = ee_oracle::Machine::cold(iso, "")?;
    let mut rec = record::Recorder::new(&mut m);
    let t0 = std::time::Instant::now();
    for f in 1..=fields {
        m.set_pad(uya::pad_at(presses, f), [0x80; 4]);
        if let Err(e) = m.frame() {
            rec.write(wav)?;
            return Err(format!("field {f}: pc {:08x}: {e}", m.cpu.pc));
        }
        rec.field(&mut m);
        if f % 600 == 0 {
            println!("field {f}: {:.1}s of sound, {:.1}s, {} M insns", rec.seconds(), t0.elapsed().as_secs_f64(), m.cpu.icount / 1_000_000);
        }
    }
    rec.write(wav)
}


fn main() -> ExitCode {
    #[cfg(windows)]
    uya::lower_priority();
    let a: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str, default: u64| -> u64 {
        a.iter().position(|x| x == name).and_then(|i| a.get(i + 1)).map(|v| v.parse().expect("number")).unwrap_or(default)
    };
    let r = match a.first().map(String::as_str) {
        Some("frames") if a.len() >= 2 => {
            let out = a.iter().position(|x| x == "--out").map(|i| a[i + 1].clone()).unwrap_or_else(|| "frames".into());
            let catch_from = a.iter().any(|x| x == "--catch-from").then(|| flag("--catch-from", 0));
            let wav = a.iter().position(|x| x == "--wav").map(|i| a[i + 1].as_str());
            let text = |name: &str| a.iter().position(|x| x == name).map(|i| a[i + 1].clone());
            // --areas NAME:FROM-TO,...
            let areas = text("--areas")
                .map(|v| {
                    v.split(',')
                        .map(|p| {
                            let (name, r) = p.split_once(':').expect("--areas NAME:FROM-TO");
                            let (from, to) = r.split_once('-').expect("--areas NAME:FROM-TO");
                            (name.to_string(), from.parse().expect("field"), to.parse().expect("field"))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let vu1 = match text("--vu1").as_deref() {
                None | Some("aot") => rac_engine::gsfull::vu1::AotMode::On,
                Some("interp") => rac_engine::gsfull::vu1::AotMode::Off,
                Some("diff") => rac_engine::gsfull::vu1::AotMode::Diff,
                Some(v) => panic!("--vu1 interp|aot|diff, not {v}"),
            };
            let tex = TexArgs { dump: text("--dump"), replace: text("--replace"), areas, cover_every: flag("--cover-every", 4).max(1) };
            // --warp LEVEL@FIELD,...
            let warps: Vec<(u32, u64)> = text("--warp")
                .map(|v| {
                    v.split(',')
                        .map(|w| {
                            let (l, at) = w.split_once('@').expect("--warp LEVEL@FIELD,...");
                            (l.parse().expect("level"), at.parse().expect("field"))
                        })
                        .collect()
                })
                .unwrap_or_default();
            // --tour LEVEL,...@FIELD:STAY
            let tour = text("--tour").map(|v| {
                let (levels, rest) = v.split_once('@').expect("--tour LEVEL,...@FIELD:STAY");
                let (first, stay) = rest.split_once(':').expect("--tour LEVEL,...@FIELD:STAY");
                Tour { levels: levels.split(',').map(|l| l.parse().expect("level")).collect(), first: first.parse().expect("field"), stay: stay.parse().expect("fields") }
            });
            match ee::mode(&a) {
                Ok(mode) => frames(&a[1], flag("--fields", 600), flag("--every", 60), flag("--save-from", 0), &out, &uya::presses(&a), catch_from, wav, a.iter().any(|x| x == "--movies"), &tex, vu1, a.iter().any(|x| x == "--hash-from").then(|| flag("--hash-from", 0)), mode, text("--ram-dir").as_deref(), &warps, a.iter().any(|x| x == "--native").then(|| flag("--native", 896) as u32), a.iter().any(|x| x == "--trace").then(|| flag("--trace", 0)), tour.as_ref()),
                Err(e) => Err(e),
            }
        }
        Some("sound") if a.len() >= 2 => {
            let wav = a.iter().position(|x| x == "--wav").map(|i| a[i + 1].as_str()).unwrap_or("sound.wav");
            sound(&a[1], flag("--fields", 600), wav, &uya::presses(&a))
        }
        // uyaref vucheck: `uya native`'s redraw (`uya::redraw`) with the passes' check on.
        Some("vucheck") if a.len() >= 5 => {
            vucheck::install();
            uya::redraw(&a)
        }
        Some("sndtrace") if a.len() >= 2 => snd::trace(&a),
        Some("sndcheck") if a.len() >= 3 => snd::check(&a),
        Some("sndemu") if a.len() >= 3 => snd::emu_streams(&a),
        Some("sndlfo") if a.len() >= 4 => snd::lfo_compare(&a),
        Some("sndrev") if a.len() >= 2 => snd::reverb_compare(&a),
        Some("sndtour") if a.len() >= 2 => sndtour::run(&a),
        Some("mpegcmp") if a.len() >= 3 => mpegcmp::compare(&a),
        _ => Err("usage: uyaref frames <disc.iso> [--fields N] [--every K] [--out DIR] [--press FIELD:BUTTONS:FIELDS,...] [--catch-from FIELD] [--wav FILE] [--movies]
         [--warp LEVEL@FIELD,...] [--tour LEVEL,...@FIELD:STAY] [--dump DIR] [--replace DIR] [--areas NAME:FROM-TO,... [--cover-every N]]
         [--ee interp|rec] [--vu1 interp|aot|diff] [--ram-dir DIR] [--native H] [--trace FIELD]
       uyaref sound <disc.iso> [--fields N] [--wav FILE] [--press FIELD:BUTTONS:FIELDS,...]
       uyaref vucheck <disc.iso> <ram.bin>[,<ram.bin>...] <level> <out.png> [--height H] [--hd DIR] [--aniso] [--ao]
       uyaref sndtrace|sndemu|sndcheck|sndlfo|sndrev|sndtour <disc.iso> ...
       uyaref mpegcmp <disc.iso> <sector> <bytes> [--out DIR] [--every N] [--lang L] | uyaref mpegcmp <disc.iso> all [--lang L]".into()),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
