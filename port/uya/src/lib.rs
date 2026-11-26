//! Up Your Arsenal on the PC, the port's own code: the game's EE code as the port's translator (port/uyatr) wrote it, on the port's
//! own runtime (port/uyart), drawn by the port's own renderer (`native`, wgpu), heard through its own sound engine (`snd`) and movie
//! decoder (`mpeg`), in its own window (`play`). No emulation and no rac-rs crate (PLAN item 0). `src/main.rs` is the game's
//! command line (`uya play`, `uya native`, ...); the emulated reference, port/uyaref, takes the renderer, the sound engine and the
//! movie decoder from this library to set them against rac-rs's emulated PS2.

use std::path::Path;

pub mod ao;
pub mod audio;
pub mod bindings;
pub mod cinema;
pub mod control;
#[cfg(windows)]
pub mod crash;
mod crew;
mod dlss;
mod streamline;
pub mod eedraw;
pub mod effects;
pub mod enhance;
pub mod gsmem;
mod gpu_profile;
pub mod icon;
pub mod input;
pub mod jobs;
pub mod lighting;
pub mod menus;
pub mod mobies;
pub mod mpeg;
pub mod native;
pub mod objects;
mod shrub_batch;
pub mod pic;
pub mod pipelines;
pub mod play;
pub mod prompts;
pub mod range;
pub mod scenespace;
pub mod raytrace;
pub mod rtlight;
pub mod reflections;
pub mod replay;
#[cfg(windows)]
pub mod sampler;
pub mod seen;
pub mod settings;
#[cfg(windows)]
pub mod setup;
pub mod shadows;
pub mod shafts;
pub mod sky;
pub mod surfaces;
pub mod snd;
pub mod texnames;
pub mod text;
pub mod tievu;
pub mod vif;
pub mod vufloat;
pub mod vusea;
pub(crate) mod vu1;
mod vu1p0;
mod vu1p5;
mod vu1p6;
pub mod vuwater;

/// rac3-decomp's `level_current` (D_001D545C): the level being played, 0 or -1 in the front end.
pub const LEVEL_CURRENT: u32 = 0x1D_545C;

/// The level being left during a level change (request_level_change stores it).
pub const LEVEL_LEFT: u32 = 0x1D_4CF0;

/// `level_current` in RAM `ram`: for a front end's frame, the level being entered (0 or -1: none).
pub fn level_current(ram: &[u8]) -> u32 {
    u32::from_le_bytes(ram[LEVEL_CURRENT as usize..][..4].try_into().expect("4 bytes"))
}

/// The level loader's step, 0 to 5: the loader (the front end's function at 0x39A4A8, Veldin's at 0x428518, one copy in each
/// program) reads the next level a part at a time, indexing its jump table (the front end's at 0x31C1A0) by this word and
/// advancing it as each part is read. The front end names the level it enters (`level_current`) and puts the step back to 0 in
/// one place (0x3BD610); its space scene of the level's arrival, the load screen, shows from step 2 on (its main loop keeps
/// `step >= 2` in the byte at gp - 0x3BB0, 0x3BD870, for the screen it draws the field after). NEW GAME into Veldin: step 0
/// from the save slot written (field 767) through the slot screen's fade to black, 2 from the load screen's first picture (823),
/// 5 from 1119 until Veldin's own picture (1268).
pub const LOAD_STEP: u32 = 0x1D_4B48;

/// `LOAD_STEP` in RAM `ram`.
pub fn load_step(ram: &[u8]) -> i32 {
    i32::from_le_bytes(ram[LOAD_STEP as usize..][..4].try_into().expect("4 bytes"))
}

/// The pad buttons of every press whose fields include field `f` (`--press FIELD:BUTTONS:FIELDS`).
pub fn pad_at(presses: &[(u64, u16, u64)], f: u64) -> u16 {
    presses.iter().filter(|&&(at, _, n)| f >= at && f < at + n).fold(0u16, |b, &(_, x, _)| b | x)
}

/// --press FIELD:BUTTONS:FIELDS,... (buttons in hex, the pad word's bits: 0x800 Start, 0x40 Cross, ...).
pub fn presses(a: &[String]) -> Vec<(u64, u16, u64)> {
    a.iter()
        .position(|x| x == "--press")
        .map(|i| {
            a[i + 1]
                .split(',')
                .map(|p| {
                    let v: Vec<&str> = p.split(':').collect();
                    (v[0].parse().expect("field"), u16::from_str_radix(v[1].trim_start_matches("0x"), 16).expect("buttons"), v[2].parse().expect("fields"))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The batch tools run below normal priority: a long redraw or capture never takes the desktop's CPU time (`uya play` sets its own
/// back to normal).
#[cfg(windows)]
pub fn lower_priority() {
    use windows::Win32::System::Threading::{GetCurrentProcess, SetPriorityClass, BELOW_NORMAL_PRIORITY_CLASS};
    unsafe { SetPriorityClass(GetCurrentProcess(), BELOW_NORMAL_PRIORITY_CLASS) }.expect("SetPriorityClass");
}

/// `uya play` given a console of its own (started from Explorer or the desktop shortcut; not from a terminal, and not by a script
/// that reads its output through a pipe) writes what it prints, its errors and a panic's message with the rest, to the
/// profile's `play.log`, and the run before's is kept as `play-previous.log`: the console closes with the game, and what it
/// printed as it stopped went with it (the player, 16 November 2026: "game crashed check logs").
#[cfg(windows)]
pub fn keep_log(profile: &str, args: &[String]) -> Result<(), String> {
    use windows::Win32::Storage::FileSystem::{GetFileType, FILE_TYPE_CHAR};
    use windows::Win32::System::Console::{FreeConsole, GetConsoleProcessList, GetStdHandle, SetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE};
    let out = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) }.map_err(|e| format!("play: the console: {e}"))?;
    let mut attached = [0u32; 2];
    // Under RecompLauncher (it sets RECOMPLAUNCHER) the output is its pipe and its console is attached, so the checks
    // below would skip the log; but the launcher's crash report shows the newest .log in the game's folder, and the
    // game's own last lines are what names a crash: keep the log there anyway.
    let launcher = std::env::var_os("RECOMPLAUNCHER").is_some();
    if !launcher && (unsafe { GetFileType(out) } != FILE_TYPE_CHAR || unsafe { GetConsoleProcessList(&mut attached) } != 1) {
        return Ok(());
    }
    let dir = std::path::Path::new(profile);
    std::fs::create_dir_all(dir).map_err(|e| format!("play: {profile}: {e}"))?;
    let log = dir.join("play.log");
    if log.exists() {
        std::fs::rename(&log, dir.join("play-previous.log")).map_err(|e| format!("play: {}: {e}", log.display()))?;
    }
    let file = std::fs::File::create(&log).map_err(|e| format!("play: {}: {e}", log.display()))?;
    let handle = windows::Win32::Foundation::HANDLE(std::os::windows::io::IntoRawHandle::into_raw_handle(file));
    unsafe {
        // Explorer allocated this console solely for the game. Once the log is open, the game window
        // should own the desktop and taskbar entry. Interactive control still reads its console input.
        // Under the launcher the console is its: leave it attached.
        if !args.iter().any(|arg| arg == "--control") && std::env::var_os("RECOMPLAUNCHER").is_none() {
            FreeConsole().map_err(|e| format!("play: detach launcher console: {e}"))?;
        }
        SetStdHandle(STD_OUTPUT_HANDLE, handle).map_err(|e| format!("play: the log: {e}"))?;
        SetStdHandle(STD_ERROR_HANDLE, handle).map_err(|e| format!("play: the log: {e}"))?;
    }
    let exe = std::env::current_exe().map_err(|e| format!("play: the executable: {e}"))?;
    println!("play: {} {}", exe.display(), args.join(" "));
    Ok(())
}

/// `uya native <disc.iso> <ram.bin>[,<ram.bin>...] <level>[,<level>...] <out.png> [--height H] [--hd DIR [--rift DIR]] [--preset NAME]
/// [--set KEY=VALUE,...] [--prompts DEVICE [--profile DIR]] [--frames-dir DIR] [--screen]` (`a[0]` is the command's name): the native picture of a RAM dump of that level (`uya play`
/// with UYA_RAM_AT, or the reference's `uyaref frames --ram-dir`), with the frame's jobs saved beside it (ramNNNNN.bin,
/// jobsNNNNN.bin); several dumps are drawn in turn by one renderer (what a frame keeps for the next, the display buffer and a
/// grabbed frame, carries on) and the last is saved. `--frames-dir` also saves every frame in sequence. `--preset` (original, low, medium, high, ultra) and `--set` (keys and values
/// as `settings.txt` writes them, after the preset) choose the graphics options (`settings::OPTIONS`); without them each is at
/// its default: off, the game's picture, but for UPDATED TEXTURES, the standard game, on (`--set rift_apart_surfaces=off`
/// draws the picture without it). `--screen` draws the dumps as front end frames (below), each frame's count of what it could not
/// draw printed.
pub fn redraw(a: &[String]) -> Result<(), String> {
    // UYA_PROFILE=START,SECONDS: the threads' time by function (`sampler.rs`, an instrument).
    #[cfg(windows)]
    crate::sampler::start();
    // One level for every dump, or one per dump in turn: the level each frame is drawn as, as `play` draws a level change (the
    // dumps of the level left, then the next level's: the next level's GPU state made as the renderer meets it).
    let levels: Vec<u32> = a[3].split(',').map(|l| l.parse().map_err(|_| "uya native: the level is a number")).collect::<Result<_, _>>()?;
    let dumps = a[2].split(',').count();
    if levels.len() != 1 && levels.len() != dumps {
        return Err(format!("uya native: {} levels for {dumps} dumps (one, or one a dump)", levels.len()));
    }
    let level_of = |i: usize| levels[if levels.len() == 1 { 0 } else { i }];
    let level = level_of(dumps - 1);
    let flag = |name: &str, default: u64| -> u64 { a.iter().position(|x| x == name).and_then(|i| a.get(i + 1)).map(|v| v.parse().expect("number")).unwrap_or(default) };
    let mut options = settings::Settings::default();
    if let Some(i) = a.iter().position(|x| x == "--preset") {
        let name = a.get(i + 1).ok_or("uya native: --preset NAME")?;
        options.set_preset(settings::PRESETS.iter().position(|p| p.eq_ignore_ascii_case(name)).ok_or_else(|| format!("uya native: --preset {name} (original, low, medium, high, ultra)"))?);
    }
    if let Some(i) = a.iter().position(|x| x == "--set") {
        options.set_pairs(a.get(i + 1).ok_or("uya native: --set KEY=VALUE,...")?)?;
    }
    enhance::set(&options);
    // --surfaces OUT.tsv: each terrain and tie texture's pixels in the last dump's picture (`surfaces`), the picture drawn in the
    // textures' id colours; the game's picture alone (no pack, no option), so that each colour is a texture's.
    let surfaces = a.iter().position(|x| x == "--surfaces").map(|i| a.get(i + 1).ok_or("uya native: --surfaces OUT.tsv")).transpose()?;
    if surfaces.is_some() {
        if ["--hd", "--preset", "--set"].iter().any(|f| a.iter().any(|x| x == f)) {
            return Err("uya native: --surfaces draws the game's own picture: no --hd, --preset or --set with it".into());
        }
        surfaces::turn_on();
    }
    // --rift DIR: the Rift Apart surfaces of DIR (a test's folder, kept apart from the player's profile) in place of the pack
    // folder's own `0-riftapart` (`texnames::open`).
    let rift = a.iter().position(|x| x == "--rift").map(|i| a.get(i + 1).ok_or("uya native: --rift DIR")).transpose()?;
    // --dump-textures DIR: every texture named while drawing (the scene's, the HUD's, the menus') written to DIR, as PCSX2
    // dumps it (`texnames`).
    if let Some(i) = a.iter().position(|x| x == "--dump-textures") {
        texnames::dump_to(Path::new(a.get(i + 1).ok_or("uya native: --dump-textures DIR")?))?;
    }
    // --dump-ui DIR: the 2D layer's textures (the HUD's and the menus') alone, the same way.
    if let Some(i) = a.iter().position(|x| x == "--dump-ui") {
        texnames::dump_ui_to(Path::new(a.get(i + 1).ok_or("uya native: --dump-ui DIR")?))?;
    }
    if let Some(i) = a.iter().position(|x| x == "--hd") {
        let dir = a.get(i + 1).ok_or("uya native: --hd DIR")?;
        texnames::open(Path::new(&dir), rift.map(Path::new))?;
    } else if rift.is_some() {
        return Err("uya native: --rift DIR goes with --hd DIR (the Rift Apart surfaces are drawn on the packs' textures)".into());
    }
    let mut n = native::Native::open(&a[1], flag("--height", 896) as u32)?;
    let replay_alphas: Vec<f32> = a.iter().position(|x| x == "--replay-alpha").map(|i| {
        a.get(i + 1).ok_or("uya native: --replay-alpha A[,A...]".to_string())?.split(',').map(|s| {
            s.parse::<f32>().ok().filter(|v| v.is_finite() && (0.0..=1.0).contains(v)).ok_or("uya native: replay alpha must be 0 to 1".to_string())
        }).collect::<Result<Vec<_>, _>>()
    }).transpose()?.unwrap_or_default();
    n.replays = !replay_alphas.is_empty();
    if n.replays && !a.iter().any(|x| x == "--frames-dir") {
        return Err("uya native: --replay-alpha requires --frames-dir".into());
    }
    // --prompts DEVICE: the button prompts and the HUD fonts' letters as `uya play` draws them for that device (`prompts.rs`;
    // the atlas `prompts.bin` of --profile DIR, default the player's profile folder).
    if let Some(i) = a.iter().position(|x| x == "--prompts") {
        let device = input::Device::parse(a.get(i + 1).ok_or("uya native: --prompts DEVICE")?)?;
        let profile = match a.iter().position(|x| x == "--profile") {
            Some(j) => a.get(j + 1).ok_or("uya native: --profile DIR")?.clone(),
            None => format!("{}/Games/Ratchet and Clank 3", std::env::var("USERPROFILE").map_err(|e| format!("USERPROFILE: {e}"))?),
        };
        // The profile's bindings: what the icons show (`bindings.rs`).
        let bindings = bindings::Bindings::load(&format!("{profile}/bindings.txt"))?;
        let atlas = std::sync::Arc::new(prompts::Atlas::read(&format!("{profile}/prompts.bin"))?.with_moves(bindings.moves()));
        let text = std::sync::Arc::new(text::Text::new(&uyafmt::boot::boot(&a[1])?)?);
        n.prompts = Some(prompts::Look { device, atlas, text, clicks: false, lines: std::sync::Arc::new(Vec::new()), pointer: None, answers: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())), bindings, pictures: std::sync::Arc::new(Vec::new()), slots: Vec::new() });
    }
    // Save every redraw when comparing consecutive fields (flicker cannot be judged from the last picture alone).
    let frames = a.iter().position(|x| x == "--frames-dir").map(|i| a.get(i + 1).ok_or("uya native: --frames-dir DIR")).transpose()?;
    if let Some(dir) = frames {
        std::fs::create_dir_all(dir).map_err(|e| format!("{dir}: {e}"))?;
    }
    // --screen: each dump drawn as a front end's frame (the menus, a trip's or an arrival's space scene: `Native::render_screen`,
    // across the picture's width while a level is entered, as `play` draws it), after GS memory as the field before left it: the
    // `gsmNNNNN.bin` (UYA_GSM_DUMP_AT) of the field before the dump's, beside it. A front end's frame samples what the GS holds
    // from earlier fields' transfers (a level's load sends the space scene's textures).
    let screen = a.iter().any(|x| x == "--screen");
    let mut pic = None;
    for (index, dump) in a[2].split(',').enumerate() {
        let ram = std::fs::read(dump).map_err(|e| format!("{dump}: {e}"))?;
        let p = Path::new(dump);
        let name = p.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_prefix("ram")).ok_or("uya native: the RAM dump's name is ramNNNNN.bin")?;
        let jobs = jobs::FrameJobs::read(&p.with_file_name(format!("jobs{name}")).to_string_lossy())?;
        let frame = if screen {
            let field: u64 = name.strip_suffix(".bin").and_then(|f| f.parse().ok()).ok_or("uya native: the RAM dump's name is ramNNNNN.bin")?;
            n.load_gsm(&p.with_file_name(format!("gsm{:05}.bin", field - 1)).to_string_lossy())?;
            // The load screen's fill as `play` decides it: a level named and the front end showing its arrival (`LOAD_STEP`).
            let fill = !matches!(level_current(&ram), 0 | u32::MAX) && load_step(&ram) >= 2;
            n.draw_screen(&ram, &jobs, fill)?
        } else {
            n.draw(&ram, &jobs, level_of(index))?
        };
        if let Some(dir) = frames {
            let out = Path::new(dir).join(format!("{index:05}.png"));
            std::fs::write(&out, pic::png(&frame)).map_err(|e| format!("{}: {e}", out.display()))?;
            for (j, &alpha) in replay_alphas.iter().enumerate() {
                if let Some(between) = n.draw_replay(alpha)? {
                    let out = Path::new(dir).join(format!("{index:05}-between{j:02}.png"));
                    std::fs::write(&out, pic::png(&between)).map_err(|e| format!("{}: {e}", out.display()))?;
                }
            }
        }
        pic = Some(frame);
    }
    let pic = pic.ok_or("uya native: no RAM dump named")?;
    if let Some(out) = surfaces {
        surfaces::write(&pic, level, out)?;
    }
    std::fs::write(&a[4], pic::png(&pic)).map_err(|e| format!("{}: {e}", a[4]))
}

mod surface_buffer;
