//! Up Your Arsenal on the PC: the game's own code from the player's disc, translated ahead of time by the port's own translator onto
//! the port's own runtime, drawn, heard and played by the port's own code (the library, src/lib.rs). No emulation: the emulated
//! reference is port/uyaref, a tool of its own.
//!
//!   uya play <disc.iso> [options]
//!       the game in a window (src/play.rs says what each option does): boot, the front end, NEW GAME and every story level.
//!
//!   uya native <disc.iso> <ram.bin>[,<ram.bin>...] <level> <out.png> [--height H] [--hd DIR [--rift DIR]] [--preset NAME] [--set KEY=VALUE,...] [--prompts DEVICE [--profile DIR]] [--frames-dir DIR]
//!       draws the native picture (896 pixels high) of a RAM dump that `play` (UYA_RAM_AT) or the reference's `uyaref frames
//!       --ram-dir` wrote while level `level` was played, with the jobs file beside it; several dumps (consecutive fields) are drawn
//!       in turn by one renderer, so what a frame keeps for the next (the display buffer, a grabbed frame) carries on, and the last
//!       picture is saved. With --frames-dir, --replay-alpha A[,A...] also saves the presenter's between-frame pictures.
//!       --hd DIR draws the PCSX2 pack files under DIR (the profile's texture_replacements) in place of the textures they name,
//!       at their own resolution (src/texnames.rs); --height the picture's height (896 by default); --preset
//!       (original, low, medium, high, ultra) and --set (src/settings.rs's keys and values, e.g. texture_filtering=16x) the graphics
//!       options of the settings page (src/enhance.rs), every one off without them; --prompts the button prompts and the
//!       HUD fonts' letters as `play` draws them for that device (the atlas of the profile folder, default the player's).
//!
//!   uya scene <disc.iso> <level>
//!       decodes the level's native scene from the disc (src/native.rs `Scene::decode`) and prints what it holds, or the first
//!       decoder error; no game run, no window.
//!
//!   uya texnames <disc.iso> <level>[,<level>...] <dumps dir> [--write]
//!       names every texture of each level's native scene as PCSX2 does (src/texnames.rs) and counts, per kind, the names the
//!       dump folder holds a file for; --write dumps the others there.
//!
//!   uya sounds <disc.iso> <level|fe> <out dir> [--sound LABEL:N,...], uya sndreplay <disc.iso> [--wav FILE], uya sndbanks <disc.iso>
//!       the port's own sound engine on its own: a bank's sounds into WAVs, the boot's recorded sound calls replayed and checked,
//!       every bank of the disc decoded (src/snd/tool.rs).

use std::process::ExitCode;

fn main() -> ExitCode {
    // The screen recorders' Vulkan layers (OBS Studio's game capture, VK_LAYER_OBS_HOOK; Bandicam's, VK_LAYER_bandicam_helper) are
    // registered for every Vulkan program on the PC and load whether or not the recorder runs, hooking the commands: on the
    // player's Crash Site profile (23 November 2026) they took 4 % of the game's CPU, inside the presenter's recording. Each
    // layer's manifest names the variable that keeps it out of a program; the Vulkan loader reads it when the instance is made.
    // SAFETY: set before any other thread of the program starts.
    unsafe {
        std::env::set_var("DISABLE_VULKAN_OBS_CAPTURE", "1");
        std::env::set_var("VK_LAYER_bandicam_helper_DEBUG_1", "1");
    }
    let a: Vec<String> = std::env::args().skip(1).collect();
    // The batch commands run below normal priority (`uya play` sets its own back to normal).
    #[cfg(windows)]
    uya::lower_priority();
    let r = match a.first().map(String::as_str) {
        Some("native") if a.len() >= 5 => uya::redraw(&a),
        Some("texnames") if a.len() >= 4 => match a[2].split(',').map(str::parse::<u32>).collect::<Result<Vec<_>, _>>() {
            Ok(levels) => uya::texnames::report(&a[1], &levels, &a[3], a.iter().any(|x| x == "--write")),
            Err(_) => Err("uya texnames <disc.iso> <level>[,<level>...] <dumps dir> [--write]: levels are numbers".into()),
        },
        // `--ee` chose between rac-rs's emulated machine and the port's runtime until PLAN 0.6: the game runs on the runtime only.
        Some("play") if a.iter().any(|x| x == "--ee") => Err("uya play: --ee is gone; the game runs on the port's own runtime only (the emulated machine is port/uyaref's `uyaref frames`)".into()),
        // uya play <disc.iso> [--height H] [--monitor N] [--seconds S] [--press ...] [--click ...] [--hover FIELD:X:Y,...] [--mouse FIELD:DX:DY:FIELDS,...] [--fast [--movies]] [--start LEVEL] [--control] [--write-icon] [--warp LEVEL@FIELD,...] [--prompts DEVICE] [--profile DIR] [--rift DIR] [--mute] [--wav FILE]: src/play.rs.
        Some("play") if a.len() >= 2 => {
            let text = |name: &str| a.iter().position(|x| x == name).map(|i| a[i + 1].clone());
            let profile = text("--profile").unwrap_or_else(|| format!("{}/Games/Ratchet and Clank 3", std::env::var("USERPROFILE").expect("USERPROFILE")));
            #[cfg(windows)]
            let logged = uya::keep_log(&profile, &a);
            #[cfg(not(windows))]
            let logged: Result<(), String> = Ok(());
            match logged.and_then(|()| text("--prompts").map(|d| uya::input::Device::parse(&d)).transpose()) {
                Ok(prompts) => uya::play::play(&a[1], uya::play::Options { height: text("--height").map(|h| h.parse().expect("height")), seconds: text("--seconds").map(|s| s.parse().expect("seconds")), monitor: text("--monitor").map(|m| m.parse().expect("monitor")), aspect: text("--aspect").map(|v| { let (w, h) = v.split_once(':').expect("--aspect W:H"); (w.parse().expect("--aspect W"), h.parse().expect("--aspect H")) }), presses: uya::presses(&a), write_icon: a.iter().any(|x| x == "--write-icon"), warps: text("--warp").map(|v| v.split(',').map(|w| { let (l, at) = w.split_once('@').expect("--warp LEVEL@FIELD,..."); (l.parse().expect("level"), at.parse().expect("field")) }).collect()).unwrap_or_default(), clicks: text("--click").map(|c| c.split(',').map(|p| { let v: Vec<f32> = p.split(':').map(|x| x.parse().expect("--click FIELD:X:Y")).collect(); (v[0] as u64, v[1], v[2]) }).collect()).unwrap_or_default(), fast: a.iter().any(|x| x == "--fast"), start: text("--start").map(|l| l.parse().expect("--start LEVEL")), control: a.iter().any(|x| x == "--control"), movies: a.iter().any(|x| x == "--movies"), mice: text("--mouse").map(|c| c.split(',').map(|p| { let v: Vec<f64> = p.split(':').map(|x| x.parse().expect("--mouse FIELD:DX:DY:FIELDS")).collect(); (v[0] as u64, v[1], v[2], v[3] as u64) }).collect()).unwrap_or_default(), hovers: text("--hover").map(|c| c.split(',').map(|p| { let v: Vec<f32> = p.split(':').map(|x| x.parse().expect("--hover FIELD:X:Y")).collect(); (v[0] as u64, v[1], v[2]) }).collect()).unwrap_or_default(), prompts, profile, rift: text("--rift"), audio: uya::audio::Options { mute: a.iter().any(|x| x == "--mute"), wav: text("--wav"), device_rate: false } }),
                Err(e) => Err(e),
            }
        }
        Some("scene") if a.len() >= 3 => match a[2].parse::<u32>() {
            Ok(level) => uya::native::Scene::decode(&a[1], level).map(|_| ()),
            Err(_) => Err("uya scene <disc.iso> <level>: the level is a number".into()),
        },
        // uya sounds <disc.iso> <level|fe> <out dir> [--sound LABEL:SOUND,...]: the port's own sound engine plays a bank's sounds into WAVs (src/snd/tool.rs).
        Some("sounds") if a.len() >= 4 => uya::snd::tool::sounds(&a),
        Some("sndreplay") if a.len() >= 2 => uya::snd::tool::replay(&a),
        Some("sndrev") if a.len() >= 5 => uya::snd::tool::sndrev(&a),
        Some("sndbanks") if a.len() >= 2 => uya::snd::tool::bank_census(&a),
        Some("sndlog") if a.len() >= 5 => uya::snd::tool::sndlog(&a),
        _ => Err("usage: uya play <disc.iso> [--height H] [--monitor N] [--seconds S] [--press FIELD:BUTTONS:FIELDS,...] [--start LEVEL] [--warp LEVEL@FIELD,...] [--prompts keyboard|xbox|dualsense|dualshock4] [--profile DIR] [--rift DIR] [--mute] [--wav FILE]
       uya native <disc.iso> <ram.bin>[,<ram.bin>...] <level> <out.png> [--height H] [--hd DIR [--rift DIR]] [--preset NAME] [--set KEY=VALUE,...] [--prompts DEVICE [--profile DIR]] [--frames-dir DIR]
       uya scene <disc.iso> <level>
       uya texnames <disc.iso> <level>[,<level>...] <dumps dir> [--write]
       uya sounds <disc.iso> <level|fe> <out dir> [--sound LABEL:SOUND,...]
       uya sndreplay <disc.iso> [--wav FILE]
       uya sndbanks <disc.iso>
       uya sndlog <play.log> <disc.iso> <from tick> <to tick>".into()),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
