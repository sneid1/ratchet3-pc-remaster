//! The game thread of `uya play`, on the port's own runtime (PLAN item 0; the only one since 0.6): the game's code as `port/uyatr`
//! translated it (`translated/uya`, never committed), on `uyart`'s machine, with no rac-rs crate between. Each field it reads the
//! input, runs the field (`Machine::field`), and sends the renderer a `Frame`: the field's GS jobs as the DMA kicks handed them over,
//! with each VIF1 chain's blocks, and the EE RAM they were built from. The game's sound calls go to the port's sound engine
//! (`snd::Engine`, `SndModule`), which the live audio (`audio.rs`) pulls after each field.

use super::{Event, Frame, Msg, FIELD, LEVEL_AUTOSAVE, LEVEL_SAVING, MC_CONTINUE, MC_CONTINUE_YES, MC_CREATE, MC_CREATE_YES, MC_HIDDEN, MC_MESSAGE, RAM, SAVE_CARD};
use crate::input::Input;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use winit::event_loop::EventLoopProxy;

fn rd32(ram: &[u8], a: u32) -> u32 {
    u32::from_le_bytes(ram[a as usize..a as usize + 4].try_into().expect("4 bytes"))
}

/// The sound module's two servers (0x123456, 0x123457) on the runtime: each call goes to the port's sound engine, which the
/// live audio's mixer also pulls (`Audio::engine`).
struct SndModule(Arc<Mutex<crate::snd::Engine>>);

/// The runtime's EE memory and disc as the sound engine reaches them.
struct SndHost<'a>(&'a mut dyn uyart::iop::SoundHost);

impl crate::snd::rpc::Host for SndHost<'_> {
    fn read_ee(&mut self, addr: u32, len: usize) -> Result<Vec<u8>, String> {
        self.0.read_ee(addr, len)
    }
    fn write_ee(&mut self, addr: u32, bytes: &[u8]) -> Result<(), String> {
        self.0.write_ee(addr, bytes)
    }
    fn read_disc(&mut self, lba: u32, sectors: u32) -> Result<Vec<u8>, String> {
        self.0.read_disc(lba, sectors)
    }
    fn disc_sectors(&mut self) -> u64 {
        self.0.disc_sectors()
    }
    fn read_iop(&mut self, addr: u32, len: usize) -> Result<Vec<u8>, String> {
        self.0.read_iop(addr, len)
    }
}

impl uyart::iop::Sound for SndModule {
    fn call(&mut self, host: &mut dyn uyart::iop::SoundHost, sid: u32, fno: u32, send: &[u8]) -> Result<Vec<u8>, String> {
        let mut e = self.0.lock().map_err(|_| "the sound engine's lock is poisoned (the mixer thread panicked)")?;
        e.call(&mut SndHost(host), sid, fno, send)
    }
}

/// The game's movie player (`uyart::machine::movie_player`: a0 the PSS's sector, a1 its bytes, a3 the language) to the port's
/// movie decoder; true when the movie was skipped.
fn movie(_c: &mut uyart::Cpu, a: [u32; 4]) -> bool {
    crate::cinema::play_movie(a[0], a[1], a[2])
}

/// The front end's own `request_level_change` (rac3-decomp func_0038E5B8; in this disc's main program at 0x393110), the copy the
/// front end's NEW GAME uses.
const FRONT_REQUEST: u32 = 0x39_3110;
/// Its first four code words, the same in every copy (the front end's and each level program's).
const REQUEST_SIG: [u32; 4] = [0x27bd_ffe0, 0xffb0_0010, 0xafa4_0000, 0x00a0_802d];
/// The game's own 16:9 option (option_widescreen, a byte of the options at 0x143950 + 0xAD): held at 0 (`game`).
const WIDESCREEN_BYTE: usize = 0x14_39FD;
/// The game's options (0x143950, which a save keeps; `WIDESCREEN_BYTE` is one of them) hold the camera options of each camera
/// setting, a row 0x10 apart (third person, lock-strafe, first person): the vertical invert (its Options' UP/DOWN MOVEMENT) at
/// +0x14, the horizontal (LEFT/RIGHT MOVEMENT) at +0x44 and the speed at +0x74, 1 INVERTED and 0 NORMAL as the Options show
/// them. A level's start copies them into the camera's own rows, player 0's (hero_level_start, Veldin 0x4DBB60:
/// camera_invert_v_options 0x143A10, camera_invert_h_options 0x143A70, camera_speed_options 0x143AD0). The main program's own
/// data, which a new game keeps, has every vertical INVERTED and the third person's horizontal INVERTED (the other two NORMAL):
/// the stick turns the camera the other way from a PC game's. The player (16 November 2026): "the default should be not
/// inverted". They are set NORMAL once, at the first field the main program's own options are in RAM (`new_game_camera`), before
/// a save can be loaded; a loaded save brings its own.
const CAMERA_INVERT_V: usize = 0x14_3964;
const CAMERA_INVERT_H: usize = 0x14_3994;
/// The step from one camera setting's row of the options to the next.
const CAMERA_ROW: usize = 0x10;

/// Whether RAM holds the main program's own camera invert options (`CAMERA_INVERT_V`): every vertical 1, the horizontal 1 in
/// the third person row and 0 in the other two.
fn new_game_camera(ram: &[u8]) -> bool {
    let w = |a: usize| u32::from_le_bytes(ram[a..a + 4].try_into().expect("4 bytes"));
    (0..3).all(|k| w(CAMERA_INVERT_V + CAMERA_ROW * k) == 1 && w(CAMERA_INVERT_H + CAMERA_ROW * k) == u32::from(k == 0))
}
/// The game's current mode while the quick-select ring is held open in play (`menus::Menus::game_mode`).
const QUICK_RING: u32 = 4;

/// The port's runtime as the control channel drives it (`control::Game`): EE RAM, and the game's own level request; the warps
/// waiting for the game to ask for levels (`--warp`'s queue), the menus that say when it does, and the field about to run.
struct RtGame<'a>(&'a mut uyart::Machine, &'a mut Warps, &'a mut crate::menus::Menus, u64);

/// The warps not yet asked for: the level and the field after which it is.
type Warps = std::collections::VecDeque<(u32, u64)>;

impl crate::control::Game for RtGame<'_> {
    fn rd8(&mut self, a: u32) -> u8 {
        self.0.ram[a as usize & (RAM - 1)]
    }
    fn wr8(&mut self, a: u32, v: u8) {
        self.0.ram[a as usize & (RAM - 1)] = v;
    }
    fn rd32(&mut self, a: u32) -> u32 {
        rd32(&self.0.ram, a)
    }
    /// Now when the game itself asks for levels in the state it is in (`Menus::asks_levels`), else queued as a `--warp` of this
    /// field, asked for at the first field it does.
    fn request_level(&mut self, level: u32) -> Result<String, String> {
        if self.2.asks_levels(&self.0.ram[..RAM]) {
            return request_level(self.0, level);
        }
        self.1.push_back((level, self.3 - 1));
        Ok(format!("warp to {level} when the game asks for levels (its mode is {:?}: not in play)", self.2.game_mode(&self.0.ram[..RAM])))
    }
    fn request_scene(&mut self, record: u32) -> Result<String, String> {
        request_scene(self.0, record)
    }
    fn request_mode(&mut self, mode: u32, arg: u32) -> Result<String, String> {
        let f = level_function(self.0, "game_mode_request", |m, a| (0..3).all(|k| rd32(&m.ram, a + 4 * k) == MODE_REQUEST_SIG[k as usize]))?;
        match self.0.call_between(f, &[mode, 1, arg, 0, 0])? as i32 {
            0 => Ok(format!("level {}'s mode {mode} with {arg} asked for", rd32(&self.0.ram, crate::LEVEL_CURRENT))),
            r => Err(format!("ask {mode} {arg}: game_mode_request at {f:#x} returned {r}")),
        }
    }
    fn request_chunk(&mut self, loader: u32, chunk: u32) -> Result<(), String> {
        if self.game_mode() != Some(0) { return Err("chunk: the level must be in play".into()); }
        // The LEVEL header in the game's table of contents. Blocks 4..6 hold the three chunks.
        let header = 0x16_0c40 + 0x79a0;
        if chunk > 2 || rd32(&self.0.ram, header + 0x34 + chunk * 8) == 0 {
            return Err(format!("level {} has no chunk {chunk}", rd32(&self.0.ram, crate::LEVEL_CURRENT)));
        }
        self.0.queue_chunk(loader, chunk)
    }
    fn loading_chunk(&self) -> bool { self.0.loading_chunk() }
    fn game_mode(&mut self) -> Option<u32> {
        self.2.game_mode(&self.0.ram[..RAM])
    }
    fn set(&mut self, pairs: &str) -> Result<(), String> {
        self.2.settings.set_pairs(pairs)
    }
}

/// The first words of the level program's game_mode_request (Veldin's func_L01_00457288, level 9's 0x470CF8: `beqz $t0`,
/// `daddu $t3, $a0, $zero`, `sb $zero, 0($t0)`). It stores the cutscene it asks for with `sw $a2, imm($gp)` in its first 40 words, at
/// the level's own small data (Veldin gp - 0x4AF0, level 9 gp - 0x4B30); the mode switch passes it to the cutscene's start
/// (func_L01_004B5C78).
const MODE_REQUEST_SIG: [u32; 3] = [0x1100_0002, 0x0080_582d, 0xa100_0000];

/// Plays the level's in-engine cutscene `record` as the game asks for one: its own game_mode_request(2, 1, record, 0, 0), called
/// between fields (mode 2, the cutscene; 1, the mode in play kept to come back to; an id below 100 is the level's own SCENE record,
/// func_L01_004AD7B0). The game's mode switch fades out and starts it over the next fields.
fn request_scene(m: &mut uyart::Machine, record: u32) -> Result<String, String> {
    let gp = m.main_gp()?;
    let f = level_function(m, "game_mode_request", |m, a| (0..3).all(|k| rd32(&m.ram, a + 4 * k) == MODE_REQUEST_SIG[k as usize])).map_err(|e| format!("scene: {e}"))?;
    let asked_at = (0..40).map(|k| rd32(&m.ram, f + 4 * k)).find(|w| w >> 16 == 0xaf86).map(|w| gp.wrapping_add(w as u16 as i16 as i32 as u32)).ok_or_else(|| format!("scene: game_mode_request at {f:#x} stores no $a2 at $gp"))?;
    if record >= 100 {
        return Err(format!("scene {record}: a level's own records are below 100"));
    }
    let result = m.call_between(f, &[CUTSCENE_MODE, 1, record, 0, 0])? as i32;
    if result != 0 {
        return Err(format!("scene {record}: game_mode_request at {f:#x} returned {result}"));
    }
    let asked = rd32(&m.ram, asked_at);
    if asked != record {
        return Err(format!("scene {record}: game_mode_request at {f:#x} did not take it (the cutscene asked for is {asked})"));
    }
    Ok(format!("level {}'s scene {record} asked for", rd32(&m.ram, crate::LEVEL_CURRENT)))
}

/// The game's current mode while a cutscene plays (`menus::Menus::game_mode`), its in-engine cutscenes' SCENE records.
const CUTSCENE_MODE: u32 = 2;

/// level_change_request (gp - 0x623C, the same word for the front ends and every level: request_level_change sets it, the level
/// change clears it once the new level runs): 1 from the request (a travel, the front end's NEW GAME or LOAD GAME, a warp) to the
/// new level's first field, about 400 fields from Veldin to Florana, in which the game's code takes 1.6 ms a field. With FAST
/// LOADING on those fields run unpaced (PLAN 10): the game does the same, sooner; a picture is drawn when the renderer is free
/// and the sound made meanwhile is not played (`audio.rs`: what does not fit the ring is dropped).
const LEVEL_CHANGE_REQUEST: u32 = 0x1D_6674;

/// The mouse aim (PLAN 10; always on with the keyboard and mouse, the player, 15 November 2026: "Aiming fixes shouldnt be
/// optional"). The game has a lock-strafe camera of its own (its Options' CAMERA, camera_setting 0x1439F4: 0 third person, 1
/// lock-strafe, 2 first person): the hero's free-look camera (fps_cam, 0x1A6120) turned by the right stick, the hero facing
/// where it looks, his guns bent towards the point its ray hits (aim_bend_to_point, Veldin 0x4C7010; fps_aim_pos 0x1A6230).
/// While the keyboard and mouse are the device in use and a level runs (not a menu: the options there are the player's own),
/// the hero's code reads the option as lock-strafe (`uyart::set_aim`, uyatr's `CAMERA_SETTING`), unless the player's option is
/// first person, which is the free-look camera too (hero_camera_mode 2, with the same buttons: hero_pad_button_for and
/// pad_button_for remap for every option but 0); while that camera is the hero's (hero_camera_mode 1 or 2) the mouse turns it
/// itself: its yaw and pitch moved by the mouse before the field, the right stick at rest (`Input::direct`), so the turn is the
/// mouse's own, without the stick's ramp. A pad plays with the player's own option, as the game does. When the device changes
/// in play the hero's camera is set up for the other (`hero_mode`, hero_camera_set, as the game's options and quick select
/// do). Where the game's follow camera runs instead (hero_camera_mode 0) the mouse turns that by angle too (`follow_turn`).
///
/// The crosshair (the player, 16 November 2026: "set up the full correct crosshair per gun and view mode") is each gun's own:
/// its update draws it (crosshair_colour, Veldin 0x4C7588, then reticle_marker_add 0x3F4D78) by its own test, so each gun shows
/// it in the view modes the game does: the Shock Blaster and its Shock Cannon and the Plasma Storm while the hero's camera is the
/// free-look one (hero_camera_mode not 0) or the hero aims (hero_state 0x1A71A4 is 29); the Plasma Coil and the Disc Blade only
/// with the free-look camera; the N60 and its N90, the Suck Cannon, the Rift Inducer and the Annihilator by tests of their own,
/// whatever the camera. With the view mode the player's (a pad's option; the mouse's lock-strafe or first person), the game's
/// own code does the rest.
const HERO_CAMERA_MODE: u32 = 0x1A_6290;
/// Player 0's camera option (camera_setting), as `uycpu::CAMERA_SETTING`.
const CAMERA_SETTING: u32 = 0x14_39F4;
/// fps_cam's yaw and pitch (radians; the pitch positive looking down).
const FPS_YAW: u32 = 0x1A_6160;
const FPS_PITCH: u32 = 0x1A_6180;
/// fps_cam's target tracking gains, yaw then pitch (+0x74, +0x78: 0.666 and 0.85, set when the camera starts). fps_cam_update's
/// strafe tracking (Veldin 0x411110-0x4112C4, every story level's copy alike) turns the camera after a target the hero moves
/// across, by the movement across the view over the target's distance times the target's weight times these, up to 0.026
/// radians a field: the pull a mouse turn away from a target fights. LOCK-ON CAMERA PULL writes them each field in a level as
/// the game's own times its factor (`Settings::lock_pull`); the game sets them only as its camera starts (a value written in
/// Veldin stayed 300 fields), so a value that is not the one written last is the game's own, taken again.
const FPS_TRACK: u32 = 0x1A_6194;
/// The turn per mouse count at the mouse's camera speed 100 (radians: 0.11 degrees; 800 counts an inch turn 92 degrees).
const MOUSE_RAD: f64 = 0.002;
/// hero_camera_set(mode), every story level's own copy (Veldin 0x510530), by its first words.
const HERO_CAMERA_SET_SIG: [u32; 8] = [0x27bd_ffe0, 0xffb1_0008, 0xffb0_0000, 0x0080_882d, 0x1620_0023, 0xffbf_0010, 0x3c02_001a, 0x2403_0001];

/// The follow camera (hero_camera_mode 0: swimming, a ledge, a vehicle) is turned by the mouse the same way, by angle, by moving
/// its own state: the arm from the look point to the camera (`FOLLOW_ARM`, follow block +0x180; camera_manual_input, Veldin
/// 0x3E7668, rotates it by the right stick's turn each frame about the camera's up axis, and its tilt about the horizontal one).
/// Where it lives comes from the level's own code, as every level's globals do: GameCamera (`LevelGlobals::camera`, found by
/// `uyafmt::levelcode`) holds the current camera mode's object at +0x50 (camera_select, Veldin 0x3F1BE0, writes it through
/// camera_switch), whose +0x8C is the mode (0 the follow camera) and +0x70 the follow block; GameCamera +0x80 is the up axis
/// camera_manual_input turns the arm about (its table at +0x60, the same +0x60 in every level's copy). No right stick is pushed
/// and no virtual stick (+0x1FC) is set: the arm itself moves, and the game's own springs (camera_place) follow it.
const GAME_CAMERA_OBJECT: u32 = 0x50;
const GAME_CAMERA_UP: u32 = 0x80;
const CAMERA_OBJECT_MODE: u32 = 0x8C;
const CAMERA_OBJECT_FOLLOW: u32 = 0x70;
const FOLLOW_ARM: u32 = 0x180;
const FOLLOW_ARM_REACHED: u32 = 0x190;
const FOLLOW_CAMERA: u32 = 0x00;
const FOLLOW_CAMERA_LAST: u32 = 0x230;
const FOLLOW_LOOK: u32 = 0xB0;
/// How far the mouse tilts the follow camera's arm up or down (radians): the right stick's own limit, 40 degrees
/// (camera_manual_input's 0x3F32B8C2).
const FOLLOW_TILT: f64 = 0.698_131_7;

/// The follow block of the level's follow camera while it is the camera in use (GameCamera's current mode object is mode 0), or
/// None while another camera is (a fixed one, a cutscene's, the look or lock-strafe cameras) or none yet.
fn follow_block(ram: &[u8], gc: u32) -> Result<Option<u32>, String> {
    let inside = |a: u32| (0x10_0000..RAM as u32 - 0x400).contains(&a);
    let obj = rd32(ram, gc + GAME_CAMERA_OBJECT);
    // Until the level's camera manager has run its first frame no mode is current.
    if obj == 0 {
        return Ok(None);
    }
    if !inside(obj) {
        return Err(format!("the mouse turns the follow camera: GameCamera {gc:#x} holds no camera object ({obj:#x})"));
    }
    if u16::from_le_bytes([ram[(obj + CAMERA_OBJECT_MODE) as usize], ram[(obj + CAMERA_OBJECT_MODE) as usize + 1]]) != 0 {
        return Ok(None);
    }
    let block = rd32(ram, obj + CAMERA_OBJECT_FOLLOW);
    if !inside(block) {
        return Err(format!("the mouse turns the follow camera: the follow camera's object {obj:#x} holds no block ({block:#x})"));
    }
    Ok(Some(block))
}

/// Turns the follow camera about its look point by the mouse's motion `look` (counts at the mouse's speed, inversion applied: x
/// right, y down): right turns the view right about the camera's up axis; down tilts it down, the camera rising, within
/// `FOLLOW_TILT`. The camera's whole orbit moves together: the arm (+0x180), the arm the springs have reached (+0x190), the
/// camera (+0x00) and last frame's (+0x230), each about the look point (+0xB0): camera_arm_drag (Veldin 0x3E7A70) draws the arm
/// back 85% a frame towards the camera's last position, so an arm turned alone is undone by the next frame.
fn follow_turn(ram: &mut [u8], gc: u32, block: u32, look: (f64, f64)) {
    let v3 = |ram: &[u8], a: u32| [0, 4, 8].map(|k| f32::from_bits(rd32(ram, a + k)) as f64);
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    // v turned by `angle` about the unit axis `k` (Rodrigues).
    let turn = |v: [f64; 3], k: [f64; 3], angle: f64| {
        let (s, c) = angle.sin_cos();
        let (x, d) = (cross(k, v), dot(k, v));
        [0, 1, 2].map(|i| v[i] * c + x[i] * s + k[i] * d * (1.0 - c))
    };
    let unit = |a: [f64; 3]| a.map(|x| x / dot(a, a).sqrt());
    let (arm, up, centre) = (v3(ram, block + FOLLOW_ARM), unit(v3(ram, gc + GAME_CAMERA_UP)), v3(ram, block + FOLLOW_LOOK));
    // The yaw about the up axis: a positive angle turns the view left (counter-clockwise seen from above), so the mouse turns by
    // minus its motion.
    let yaw = -look.0 * MOUSE_RAD;
    let turned = turn(arm, up, yaw);
    // The tilt: the arm's elevation above the plane normal to the up axis, raised about the horizontal axis (flat x up).
    let elevation = (dot(turned, up) / dot(turned, turned).sqrt()).asin();
    let tilt = (elevation + look.1 * MOUSE_RAD).clamp(-FOLLOW_TILT, FOLLOW_TILT) - elevation;
    let flat = [0, 1, 2].map(|i| turned[i] - up[i] * dot(turned, up));
    let axis = unit(cross(flat, up));
    let moved = |v: [f64; 3]| turn(turn(v, up, yaw), axis, tilt);
    let write = |ram: &mut [u8], a: u32, v: [f64; 3]| {
        for (i, x) in v.iter().enumerate() {
            let at = a as usize + 4 * i;
            ram[at..at + 4].copy_from_slice(&(*x as f32).to_le_bytes());
        }
    };
    for a in [FOLLOW_ARM, FOLLOW_ARM_REACHED] {
        let v = v3(ram, block + a);
        write(ram, block + a, moved(v));
    }
    for a in [FOLLOW_CAMERA, FOLLOW_CAMERA_LAST] {
        let p = v3(ram, block + a);
        let r = moved([0, 1, 2].map(|i| p[i] - centre[i]));
        write(ram, block + a, [0, 1, 2].map(|i| centre[i] + r[i]));
    }
}

/// Whether the mouse aims: the keyboard and mouse in use, a story level running (not the front end, nor a vid-comic, levels 30
/// to 36, framed by their own camera), no menu up.
fn aims(ram: &[u8], menus: &crate::menus::Menus, kbm: bool) -> bool {
    let level = rd32(ram, crate::LEVEL_CURRENT);
    kbm && !matches!(level, 0 | u32::MAX) && !(30..=36).contains(&level) && !menus.menu(ram)
}

/// The hero's camera mode a device plays: the keyboard and mouse the free-look camera, lock-strafe (1) or the player's first
/// person (2); a pad the player's own option.
fn hero_mode(ram: &[u8], kbm: bool) -> u32 {
    match ram[CAMERA_SETTING as usize] as u32 {
        2 => 2,
        _ if kbm => 1,
        option => option,
    }
}

/// The level's hero_camera_set, found in RAM by its first words.
fn hero_camera_set(m: &uyart::Machine) -> Result<u32, String> {
    level_function(m, "hero_camera_set", |m, a| (0..8).all(|k| rd32(&m.ram, a + 4 * k) == HERO_CAMERA_SET_SIG[k as usize])).map_err(|e| format!("the mouse aim: {e}"))
}

/// The one address in the level overlays' range of the code in RAM (`uyart::Machine::resident_text`) where `is` finds the level
/// program's function `name`. Not every word of RAM: a level loaded over another leaves the other's code past its own end, and
/// with it a second copy (Florana loaded from Veldin: "level 2's hero_camera_set: 2 copies in RAM", which ended the game; the
/// player, 16 November 2026: "It almost always crashes when I load into my veldin save, play a bit, then try to load the
/// florana save").
fn level_function(m: &uyart::Machine, name: &str, is: impl Fn(&uyart::Machine, u32) -> bool) -> Result<u32, String> {
    let found: Vec<u32> = m.resident_text().flat_map(|(lo, hi)| (lo.max(0x0020_0000)..hi.min(0x0060_0000)).step_by(4)).filter(|&a| is(m, a)).collect();
    match found[..] {
        [f] => Ok(f),
        _ => Err(format!("level {}'s {name}: {} copies in the code in RAM", rd32(&m.ram, crate::LEVEL_CURRENT), found.len())),
    }
}

/// Asks the game for level `level` through its own `request_level_change(level, 0)`, called between fields on the runtime: the
/// front end's copy while the front end runs, else the level program's (found in RAM by its first words). It stores the level being
/// left, sets `level_next` and `level_change_request`; when the story state mapped the level to another, `level_next` is set back to
/// the level asked for.
fn request_level(m: &mut uyart::Machine, level: u32) -> Result<String, String> {
    let from = rd32(&m.ram, crate::LEVEL_CURRENT);
    let front = matches!(from, 0 | u32::MAX);
    // The main thread's gp is the main program's once its startup's SetupThread has run: before that it is 0 or the loader's
    // (the loader runs at 0x800000), and the request's gp-relative stores land outside RAM (0 - 0x6D3C: 0x1FFF92C4, seen on a busy
    // PC at field 2). level_current reads -1 as soon as the loader has unpacked the main program (its data's initial value).
    let gp = m.main_gp()?;
    if !(0x10_0000..0x80_0000).contains(&gp) {
        return Err(format!("start: the main program has not started yet (the main thread's gp is {gp:#x})"));
    }
    let sig = |m: &uyart::Machine, f: u32| (0..4).all(|k| rd32(&m.ram, f + 4 * k) == REQUEST_SIG[k as usize]);
    let f = if front {
        if !sig(m, FRONT_REQUEST) {
            return Err(format!("start: the front end's request_level_change at {FRONT_REQUEST:#x} is not in RAM"));
        }
        FRONT_REQUEST
    } else {
        // The level program's copy: the one place in its code with the signature and the two stores.
        level_function(m, "request_level_change", |m, a| sig(m, a) && stores(m, a).is_some()).map_err(|e| format!("warp: {e}"))?
    };
    let (req_off, next_off) = stores(m, f).ok_or_else(|| format!("warp: request_level_change at {f:#x} has no store of level_next and level_change_request"))?;
    let (change_request, level_next) = (gp.wrapping_add(req_off as u32), gp.wrapping_add(next_off as u32));
    m.call_between(f, &[level])?;
    if rd32(&m.ram, change_request) != 1 {
        return Err(format!("warp: request_level_change returned without setting level_change_request ({change_request:#x})"));
    }
    let mapped = rd32(&m.ram, level_next);
    if mapped != level {
        m.ram[level_next as usize..level_next as usize + 4].copy_from_slice(&level.to_le_bytes());
    }
    let mapped = if mapped != level { format!(" (the story state mapped it to {mapped})") } else { String::new() };
    Ok(if front { format!("level {level} asked for from the front end{mapped}") } else { format!("warp from level {from} to {level}{mapped}") })
}

/// request_level_change's stores of level_change_request (v0) and level_next (a1) relative to $gp, in its first 24 words.
fn stores(m: &uyart::Machine, f: u32) -> Option<(i32, i32)> {
    let (mut req, mut next) = (None, None);
    for k in 0..24 {
        let w = rd32(&m.ram, f + 4 * k);
        if w >> 26 == 0x2b && (w >> 21) & 31 == 28 {
            match (w >> 16) & 31 {
                2 => req = Some(w as u16 as i16 as i32),
                5 => next = Some(w as u16 as i16 as i32),
                _ => {}
            }
        }
    }
    Some((req?, next?))
}

/// A disconnected renderer reports its own result to the window.
pub(super) enum GameEnd {
    Quit,
    RendererStopped,
}

/// The game thread: input, the game's field, the frame to the renderer, paced to the console's field rate.
#[allow(clippy::too_many_arguments)]
pub(super) fn game(iso: &str, (presses, clicks, warps, mice): (&[(u64, u16, u64)], &[(u64, f32, f32, bool)], &[(u32, u64)], &[(u64, f64, f64, u64)]), (fast, mut start, control): (bool, Option<u32>, bool), input: Arc<Mutex<Input>>, tx: &SyncSender<Msg>, back: &Receiver<uyart::RamCopy>, quit: &AtomicBool, field: &AtomicU64, settings: crate::settings::Settings, profile: &str, events: &EventLoopProxy<Event>, audio: &crate::audio::Options) -> Result<GameEnd, String> {
    // The menus as a PC game's, and the settings and bindings set in them (the profile's `bindings.txt`: `bindings.rs`); every
    // level's menu and save-message addresses (`uyafmt::menucode`).
    let bindings_file = format!("{profile}/bindings.txt");
    let mut menus = crate::menus::Menus::new(settings, crate::bindings::Bindings::load(&bindings_file)?, uyafmt::menucode::all(iso)?);
    // Every level's globals: the ties drawn farther and finer by the game's own cull (`range.rs`), and the control channel's;
    // every level's scene state (its SCENE record's word and its frame packet buffers' words: cutscene skip, below, and the tie
    // ranges' room).
    let levels = uyafmt::levelcode::all(iso)?;
    let scenes = uyafmt::scenecode::all(iso)?;
    let range = crate::range::Range::new(levels.clone(), &scenes);
    let mut warps = warps.iter().copied().collect::<Warps>();
    // A warp due while the game does not ask for levels: said once.
    let mut warp_waits = false;
    let mut menu = true;
    // The skin job's joint matrices of the last fields (`uyart::hw::SkinJoints`): a field's job skins the frame the next field
    // sends, a later one when the game misses fields, or, in a field that runs two frames, the field's own second: a frame's jobs
    // carry the last `mobies::SKIN_FIELDS` fields' (`mobies::Mobies::skin_pass` skins each field's once, those of fields the
    // renderer drew no frame of too).
    let mut skin_before: std::collections::VecDeque<(u64, std::sync::Arc<uyart::hw::SkinJoints>)> = std::collections::VecDeque::new();
    let mut m = uyart::Machine::boot(iso, uya_game::PROGRAMS, uya_game::VU0)?;
    m.iop.memcard = Some(format!("{profile}/memcard"));
    // The sound device and the engine the game's sound calls go to, before the first field.
    let mut audio = crate::audio::Audio::open(audio, iso)?;
    audio.set_hq(menus.settings.get("audio_mixing") == 1);
    audio.set_clean_reverb(menus.settings.get("reverb") == 1);
    m.iop.sound = Some(Box::new(SndModule(audio.engine())));
    // The game's movies into the window when `play` attached the movie player (not with --fast unless --movies): else the game's
    // player skips each movie as it starts.
    if crate::cinema::attached() {
        m.movie = Some(movie);
    }
    let mut before = uyart::RamCopy::new();
    // Whether `before` holds the RAM at the field's start (copied at the last field's end). A field that kicks a frame is drawn
    // with the RAM at the kick (`Machine::frame_ram`), so the field before it does not copy the RAM at its end: in play every
    // field kicks one. A field that kicks none after one that did (the game opening a menu, whose picture is the 2D layer over
    // the last) is drawn with the RAM at its own end. Each copy (`uyart::Ram::copy_to`) takes only the pages the game wrote since
    // that buffer was last filled.
    let mut before_set = true;
    let (t0, mut next) = (Instant::now(), Instant::now());
    // The memory card message up, and the fields it has been up (its answer is pressed for the player, two fields down, two up).
    let (mut message, mut prompt) = (0u32, 0u64);
    let ram_at: Vec<u64> = std::env::var("UYA_RAM_AT").map(|v| v.split(',').map(|f| f.parse().expect("UYA_RAM_AT: fields")).collect()).unwrap_or_default();
    // Time per stage since the last line: input, the game's field, the jobs and RAM handed over; and when the last line was.
    let mut ns = [0u64; 3];
    let mut last_line = Instant::now();
    // The fields since the last line that ended with a thread still running, by where it was (`FieldOut::preempted`).
    let mut preempted: std::collections::HashMap<(u32, u32, u32), u64> = std::collections::HashMap::new();
    // The level's save message word as last seen (logged when it changes).
    let mut level_word = 0u32;
    // The GS's PMODE as last seen (logged when it changes: the display's own blend of the picture with the background).
    let mut pmode_was = u64::MAX;
    // A RAM buffer kept back from a frame the renderer was not ready for (the field not drawn), and the fields drawn.
    let (mut spare, mut drawn): (Vec<uyart::RamCopy>, u64) = (Vec::new(), 0);
    // The image transfers into GS memory of the fields and frames the renderer did not get since the last it did, oldest first
    // (`Frame::uploads_before`): a level's load sends the GS what the space scene samples while the renderer takes few of its
    // fields (fast loading runs it unpaced).
    let mut pending: Vec<uyart::hw::GsUpload> = Vec::new();
    // --control: the channel, and the renderer's own picture written to a file (its `shot`).
    let mut control = match control {
        true => Some(crate::control::Control::stdin(levels.clone())),
        false => None,
    };
    // The main program (the moby walk's), for the control channel's `until pause`.
    // The main program, for walking jobs the renderer does not get (`read_back`) and for the control channel's `until pause`.
    let boot = Arc::new(uyafmt::boot::boot(iso)?);
    // A frame the renderer was too busy to take is walked for its read-back (`main_read_back`) on a thread of its own, not on the
    // game's core: the walk of a frame (several ms on this PC with the other cores busy) made a field
    // with a dropped picture overrun its 16.7 ms, so a renderer that fell behind slowed the game as well (the v0.1.4 player's
    // i7-13700K: a fifth to a third of the pictures dropped and 53 to 57 fields a second). It takes the frame's jobs and RAM
    // buffer and gives back the pages and the buffer; the game thread takes what it gave back each field, and all of it before it
    // hands the renderer its next frame, which carries those pages (`read_back`).
    let (to_read_back, read_back_rx) = std::sync::mpsc::channel::<(crate::jobs::Kicked, uyart::RamCopy)>();
    let (read_back_tx, read_backs) = std::sync::mpsc::channel::<(Result<Vec<u32>, String>, uyart::RamCopy)>();
    let walker_boot = boot.clone();
    std::thread::Builder::new()
        .name("uya read-back".into())
        .spawn(move || {
            for (jobs, ram) in read_back_rx {
                let pages = jobs.into_frame(&ram).map(|fj| main_read_back(&fj, &walker_boot));
                if read_back_tx.send((pages, ram)).is_err() {
                    break;
                }
            }
        })
        .map_err(|e| format!("play: the read-back thread: {e}"))?;
    // The frames with the read-back thread.
    let mut reading_back = 0usize;
    let ctl_boot = match control {
        Some(_) => Some(&*boot),
        None => None,
    };
    // Cutscene skip (PLAN 10): every level's scene state (`scenes`, above), the profile's seen cutscenes, and the in-engine
    // cutscene playing.
    // A cutscene's SCENE block, unpacked out of the frame packets' reach (`scenespace`).
    crate::scenespace::install(&scenes);
    crate::seen::open(profile)?;
    let mut scene: Option<crate::seen::Cutscene> = None;
    // Fast loading: the field and the time a level change started, while it runs unpaced.
    let mut loading: Option<(u64, Instant)> = None;
    // The mouse aim: the device in use at the last field (the keyboard and mouse or a pad) and the player's camera option, and a
    // change of either in play waiting for the hero's camera to be set up for it (the options menu sets the hero's camera to the
    // option as written, which the keyboard and mouse play as `hero_mode` says); the level's hero_camera_set once found.
    let mut kbm_was = true;
    let mut option_was: Option<u8> = None;
    let mut aim_pending = false;
    let mut camera_set: Option<(u32, u32)> = None;
    // The tracking gains as the game set them, and as last written (`FPS_TRACK`).
    let mut track: Option<([u32; 2], [u32; 2])> = None;
    // The quick-select ring: a click took its slot, Triangle held off until Q is let go, Circle until the button is.
    let (mut ring_taken, mut ring_circle) = (false, false);
    // The quick-select ring's second ring turned on by the mouse wheel (below).
    let mut ring_two = false;
    // The main frame's pages read back in jobs the renderer did not get (`main_read_back`), for the next frame it draws.
    let mut read_back: Vec<u32> = Vec::new();
    // Whether a new game's camera invert options have been set NORMAL (`CAMERA_INVERT_V`).
    let mut camera_normal = false;
    let shot_tx = tx.clone();
    let mut shot = move |path: &str| -> Result<(), String> {
        let (rtx, rrx) = std::sync::mpsc::channel();
        shot_tx.send(Msg::Shot(path.into(), rtx)).map_err(|_| "play: the renderer stopped")?;
        rrx.recv().map_err(|_| "play: the renderer stopped")?
    };
    for f in 1u64.. {
        if quit.load(Ordering::SeqCst) {
            return Ok(GameEnd::Quit);
        }
        field.store(f, Ordering::SeqCst);
        // The field's start: where the presenter places its picture on the display's timeline (`play.rs`).
        let at = Instant::now();
        // --control: commands until one runs this field.
        let mut ctl = None;
        if let Some(c) = control.as_mut() {
            let before = menus.settings;
            match c.field(&mut crate::control::State { field: f, cpu: &mut RtGame(&mut m, &mut warps, &mut menus, f), menu, world: true, shot: &mut shot }) {
                crate::control::Step::Quit => return Ok(GameEnd::Quit),
                crate::control::Step::Run(cf) => ctl = Some(cf),
            }
            // A `set` reaches the renderer as a change on the settings page does (not saved: the profile's file stays as it was).
            if menus.settings != before && tx.send(Msg::Settings(menus.settings)).is_err() {
                return Ok(GameEnd::RendererStopped);
            }
        }
        // --start: from the front end as soon as the main program runs it, with its request_level_change in RAM (the first fields
        // are the boot's: `request_level` says when).
        if let Some(level) = start.filter(|_| matches!(rd32(&m.ram, crate::LEVEL_CURRENT), 0 | u32::MAX)) {
            match request_level(&mut m, level) {
                Ok(done) => {
                    println!("play: field {f}: {done}, {:.1} s", t0.elapsed().as_secs_f64());
                    start = None;
                }
                Err(e) if f % 60 == 0 => println!("play: field {f}: {e}"),
                Err(_) => {}
            }
        }
        // --warp: between two fields, through the game's own level change, at the first field from the one asked for in which the
        // game itself asks for levels (`Menus::asks_levels`: in play, not in a cutscene, a menu or the ship).
        while let Some(&(level, _)) = warps.front().filter(|w| w.1 < f) {
            if !menus.asks_levels(&m.ram[..RAM]) {
                if !warp_waits {
                    println!("play: field {f}: the warp to {level} waits for play (the game's mode is {:?})", menus.game_mode(&m.ram[..RAM]));
                    warp_waits = true;
                }
                break;
            }
            let done = request_level(&mut m, level)?;
            println!("play: field {f}: {done}");
            warps.pop_front();
            warp_waits = false;
        }
        let t = Instant::now();
        let answer = match message {
            MC_CREATE if prompt / 2 % 2 == 1 => MC_CREATE_YES,
            MC_CONTINUE if prompt / 2 % 2 == 1 => MC_CONTINUE_YES,
            LEVEL_SAVING if prompt / 2 % 2 == 1 => crate::input::CROSS,
            _ => 0,
        };
        let (mut pointer, mut pad, presses, pad_kind) = {
            let mut i = input.lock().expect("input lock");
            i.bindings = menus.bindings;
            i.controller = ctl.as_ref().map_or(0, |c| c.controller);
            i.controller_sticks = ctl.as_ref().map_or([0x80; 4], |c| c.controller_sticks);
            // --mouse: the scripted motion, as the window's raw mouse motion of the field.
            for &(_, dx, dy, _) in mice.iter().filter(|m| (m.0..m.0 + m.3).contains(&f)) {
                let mut k = i.kbm.lock().expect("input lock");
                k.motion.0 += dx;
                k.motion.1 += dy;
            }
            if let Some(cf) = &ctl {
                let mut k = i.kbm.lock().expect("input lock");
                k.motion.0 += cf.mouse.0;
                k.motion.1 += cf.mouse.1;
            }
            i.camera = menus.settings.camera();
            // The mouse aim: the control channel's `look` is a pad's right stick.
            let kbm = i.last == crate::input::Device::Keyboard && !ctl.as_ref().is_some_and(|c| c.look.is_some());
            let aim = aims(&m.ram[..RAM], &menus, kbm);
            // The quick-select ring held open: the mouse points at its slots (`input::RING_REACH`), not at the camera.
            i.ring = kbm && menus.game_mode(&m.ram[..RAM]) == Some(QUICK_RING);
            let (mode, mut follow) = (rd32(&m.ram, HERO_CAMERA_MODE), None);
            let steers = aim && !i.ring && menus.in_play(&m.ram[..RAM]);
            if steers && mode == 0 {
                let level = rd32(&m.ram, crate::LEVEL_CURRENT) as usize;
                let g = levels.iter().find(|g| g.level == level).ok_or_else(|| format!("the mouse turns the follow camera: no globals for level {level}"))?;
                follow = follow_block(&m.ram[..RAM], g.camera)?.map(|b| (g.camera, b));
            }
            i.direct = steers && (matches!(mode, 1 | 2) || follow.is_some());
            // The option read as lock-strafe for the mouse's free-look camera, unless the player's is first person (read as written).
            uyart::set_aim(aim && hero_mode(&m.ram[..RAM], true) == 1);
            let option = m.ram[CAMERA_SETTING as usize];
            if kbm != kbm_was || option_was.is_some_and(|o| o != option) {
                aim_pending = true;
                kbm_was = kbm;
            }
            option_was = Some(option);
            let at = (i.pointer(), i.field(crate::pad_at(presses, f) | answer, menu));
            // The right stick's camera speed on the game's own option; the keyboard and mouse turn by the mouse's (`Camera`).
            uyart::set_camera_speed(if i.last == crate::input::Device::Keyboard { 1.0 } else { menus.settings.stick_speed() });
            // The mouse aim: the mouse turns the hero's camera itself (its yaw to the right for a move right, its pitch down for
            // a move down); the game's own turn then keeps the pitch within its limits.
            if let (true, Some((gc, block))) = (i.look != (0.0, 0.0), follow.filter(|_| i.direct)) {
                follow_turn(&mut m.ram[..RAM], gc, block, i.look);
            } else if i.direct && i.look != (0.0, 0.0) {
                let f = |a: u32| f32::from_bits(rd32(&m.ram, a)) as f64;
                let yaw = (f(FPS_YAW) - i.look.0 * MOUSE_RAD + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
                let pitch = f(FPS_PITCH) + i.look.1 * MOUSE_RAD;
                m.ram[FPS_YAW as usize..FPS_YAW as usize + 4].copy_from_slice(&(yaw as f32).to_le_bytes());
                m.ram[FPS_PITCH as usize..FPS_PITCH as usize + 4].copy_from_slice(&(pitch as f32).to_le_bytes());
            }
            (at.0, at.1, i.presses.clone(), i.pad_kind())
        };
        // The mouse aim: the device changed in play, so the hero's camera is set up for it (`hero_mode`: the free-look camera for
        // the keyboard and mouse, the player's own option for a pad), as the game's options do.
        // LOCK-ON CAMERA PULL: the free-look camera's tracking gains, the game's own times the setting (`FPS_TRACK`); at 100%
        // what is written is what the game set.
        if !m.front_end() {
            let now = [rd32(&m.ram, FPS_TRACK), rd32(&m.ram, FPS_TRACK + 4)];
            let own = match track {
                Some((own, written)) if written == now => own,
                _ => now,
            };
            let pull = menus.settings.lock_pull();
            let written = own.map(|g| (f32::from_bits(g) * pull).to_bits());
            for (k, w) in written.iter().enumerate() {
                let a = FPS_TRACK as usize + 4 * k;
                m.ram[a..a + 4].copy_from_slice(&w.to_le_bytes());
            }
            track = Some((own, written));
        } else {
            track = None;
        }
        if aim_pending && menus.in_play(&m.ram[..RAM]) {
            let level = rd32(&m.ram, crate::LEVEL_CURRENT);
            let set = match camera_set {
                Some((l, a)) if l == level => a,
                _ => {
                    let a = hero_camera_set(&m)?;
                    camera_set = Some((level, a));
                    a
                }
            };
            let mode = hero_mode(&m.ram[..RAM], kbm_was);
            m.call_between(set, &[mode])?;
            println!("play: field {f}: the {} plays: the hero's camera set to {mode}", if kbm_was { "keyboard and mouse" } else { "pad" });
            aim_pending = false;
        }
        // The --click and --hover script: the cursor where it was last put, the left button (Cross in the menus) pressed for the
        // field of a click.
        if let Some(&(at, u, v, click)) = clicks.iter().rev().find(|c| c.0 <= f) {
            pointer = crate::menus::Pointer { at: Some((u, v)), moved: at == f, click: click && at == f, down: click && at == f, wheel: 0 };
            if click && at == f {
                pad.buttons |= crate::input::CROSS;
            }
        }
        // --control: its buttons and pointer for this field.
        if let Some(cf) = &ctl {
            pad.buttons |= cf.buttons;
            if let Some([x, y]) = cf.stick {
                pad.sticks[2] = x;
                pad.sticks[3] = y;
            }
            // `look`: the right stick as a pad's, with the right stick's camera settings.
            if let Some([x, y]) = cf.look {
                let p = crate::input::Pad { sticks: [x, y, 0x80, 0x80], ..crate::input::CENTRE }.invert_right(menus.settings.camera().stick_invert);
                pad.sticks[0] = p.sticks[0];
                pad.sticks[1] = p.sticks[1];
                uyart::set_camera_speed(menus.settings.stick_speed());
            }
            if let Some(at) = cf.pointer {
                pointer = crate::menus::Pointer { at: Some(at), moved: cf.moved, click: cf.click, down: cf.click, wheel: cf.wheel };
                if cf.click {
                    pad.buttons |= crate::input::CROSS;
                }
            }
        }
        // The quick-select ring: a click takes the slot pointed at, as letting go of Q does: the game sees Triangle let go until Q
        // is, and no Circle (no shot) until the button is (the ring closes at once and the button may still be down).
        if menus.game_mode(&m.ram[..RAM]) == Some(QUICK_RING) && pointer.click && pad.buttons & crate::input::TRIANGLE != 0 {
            (ring_taken, ring_circle) = (true, true);
        }
        if ring_taken {
            ring_taken = pad.buttons & crate::input::TRIANGLE != 0;
            pad.buttons &= !crate::input::TRIANGLE;
        }
        if ring_circle {
            ring_circle = pad.buttons & crate::input::CIRCLE != 0;
            pad.buttons &= !crate::input::CIRCLE;
        }
        // The ring's second ring by the mouse wheel: a notch turns it on, another off again (the player, 19 November 2026: "there
        // is no way to switch the the secondary quick select screen on keyboard and mouse"). The game shows the second ring while
        // R2 is held (quick_select_ring_update reads pad_button_for(8), R1 moved to R2 in the lock-strafe layout the keyboard and
        // mouse play), so while it is on R2 is held for the game; the R2 key (CROUCH) still does it held. Off when the ring closes.
        if menus.game_mode(&m.ram[..RAM]) == Some(QUICK_RING) {
            if pointer.wheel != 0 {
                ring_two = !ring_two;
            }
            if ring_two {
                pad.buttons |= crate::input::R2;
            }
        } else {
            ring_two = false;
        }
        // The answers the last frame drawn offers (boxes: `front.rs`), unless that frame is old.
        let answers = match input.lock().expect("input lock").answers() {
            (at, a) if f.saturating_sub(at) <= 4 => a,
            _ => Vec::new(),
        };
        menus.main_menu_items(&mut m.ram[..RAM], m.iop.has_save());
        // A new profile on an integrated GPU: RESOLUTION 720P, as the player's change would be (`play::INTEGRATED_720`).
        let integrated = crate::play::INTEGRATED_720.swap(false, Ordering::Relaxed).then(|| {
            let mut s = menus.settings;
            s.values[crate::settings::index("resolution")] = crate::settings::HEIGHTS.iter().position(|&h| h == 720).expect("720 among the heights") as u8;
            menus.settings = s;
            s
        });
        if let Some(s) = menus.before(&mut m.ram[..RAM], pointer, &mut pad, &answers, &presses, pad_kind).or(integrated) {
            println!("play: field {f}: settings {s}");
            s.save(&format!("{profile}/settings.txt"))?;
            audio.set_hq(s.get("audio_mixing") == 1);
            audio.set_clean_reverb(s.get("reverb") == 1);
            let _ = events.send_event(Event::Settings(s));
            if tx.send(Msg::Settings(s)).is_err() {
                return Ok(GameEnd::RendererStopped);
            }
        }
        // The bindings the player changed, applied from the next field (`Input::bindings`) and saved.
        if let Some(b) = menus.take_bindings() {
            b.save(&bindings_file)?;
            println!("play: field {f}: bindings saved");
        }
        // The menus' sound for a highlight the pointer moved, or a choice on the port's own rows (`Menus::take_sound`): the game's
        // own play_ui_sound(id, 0, 0), between fields (it only fills a sound slot, which the game's sound code plays).
        if let (Some(id), Some(f)) = (menus.take_sound(), menus.ui_sound(&m.ram[..RAM])) {
            m.call_between(f, &[id])?;
        }
        let playing = menus.in_play(&m.ram[..RAM]);
        let cutscene = menus.game_mode(&m.ram[..RAM]) == Some(CUTSCENE_MODE);
        // The port's ranges this field's culls apply as they read (`range.rs`).
        range.publish(&m.ram[..RAM], playing, cutscene).map_err(|e| format!("play: field {f}: {e}"))?;
        if let Some(s) = crate::scenespace::before() {
            println!("play: field {f}: {s}");
        }
        if m.hw.gs[0] != pmode_was {
            pmode_was = m.hw.gs[0];
            println!("play: field {f}: PMODE {pmode_was:#x} (EN {}, ALP {:#x}), BGCOLOR {:#x}", pmode_was & 3, pmode_was >> 8 & 0xff, m.hw.gs[14]);
        }
        // A level's menu keeps its lists where the game lays them out (`uycpu::Host::menu`).
        uyart::set_menu(menus.menu(&m.ram[..RAM]));
        // The field of view setting in play, the game's own view elsewhere (`Menus::in_play`), from this field's first view on. A
        // level's menu keeps the factor the field before it had: the pause and the screens of its family (the galactic map) hold
        // the frame's projection (program 6's matrix, by which the renderer places the 2D layer: `effects::Projection::ndc`) at
        // the last field's while the game still makes the view's tangents each field (Veldin's 0x1E78C0, the native camera's), and
        // the frame's projection follows the tangents a field late, so a factor changed by a menu's mode put the two apart: at
        // FIELD OF VIEW 110 the pause menu and the galactic map were drawn 1.67 times their size, past the screen's edges, the
        // world behind the pause at the game's own view (report 155269, v0.1.7: "Menu size is tied to FOV: normal at default,
        // bigger at 90, massive at 110"), and the field after the pause or a vendor the 2D layer was drawn at 0.6 times its size.
        if !menus.level_menu(&m.ram[..RAM]) {
            uyart::set_view_fov(if menus.in_play(&m.ram[..RAM]) { menus.settings.fov_factor() } else { 1.0 });
        }
        ns[0] += t.elapsed().as_nanos() as u64;
        // Cutscene skip: an in-engine cutscene (the game's mode 2) not yet seen, with AFTER FIRST VIEWING, hears no button, so
        // nothing skips it (the game's own test, func_L01_004B66E0 +0x1C0: any of 0x9FF newly pressed, or Start); it is seen
        // when the mode leaves 2.
        crate::seen::set_first_through(menus.settings.get("cutscene_skip") == 1);
        let level = rd32(&m.ram, crate::LEVEL_CURRENT);
        let now_scene = match (menus.game_mode(&m.ram[..RAM]), scenes.iter().find(|g| g.level as u32 == level)) {
            (Some(CUTSCENE_MODE), Some(g)) => Some(crate::seen::Cutscene::Scene { level, record: rd32(&m.ram, g.scene_index) }),
            _ => None,
        };
        if now_scene != scene {
            if let Some(c) = scene {
                crate::seen::saw(c)?;
                println!("play: field {f}: cutscene {c:?} ended");
            }
            if let Some(c) = now_scene {
                println!("play: field {f}: cutscene {c:?}, {}", if crate::seen::may_skip(c) { "may be skipped" } else { "plays through (not seen yet)" });
            }
            scene = now_scene;
        }
        if scene.is_some_and(|c| !crate::seen::may_skip(c)) {
            pad.buttons = 0;
        }
        // Fast loading: a level change runs unpaced (`LEVEL_CHANGE_REQUEST`).
        let changing = menus.settings.get("fast_loading") == 1 && rd32(&m.ram, LEVEL_CHANGE_REQUEST) != 0;
        match (changing, loading) {
            (true, None) => loading = Some((f, Instant::now())),
            (false, Some((f0, t0))) => {
                println!("play: field {f}: a level change ran unpaced: {} fields in {:.2} s (at the game's pace {:.2} s)", f - f0, t0.elapsed().as_secs_f64(), (f - f0) as f64 / 59.94);
                loading = None;
            }
            _ => {}
        }
        let unpaced = fast || loading.is_some();
        // The game's own 16:9 option is held off: it crops the picture top and bottom (view_projection_setup divides the vertical
        // view by 4/3), while the port draws the picture at the screen's shape itself (WIDESCREEN, `settings.rs`), and the
        // option's row shows the port's setting instead (`menus.rs`). A save made with it on loads it on.
        if m.ram[WIDESCREEN_BYTE] != 0 {
            m.ram[WIDESCREEN_BYTE] = 0;
        }
        // A new game's camera NORMAL, not inverted (`CAMERA_INVERT_V`): once, when the main program's own options are in RAM.
        if !camera_normal && new_game_camera(&m.ram) {
            for k in 0..3 {
                for a in [CAMERA_INVERT_V + CAMERA_ROW * k, CAMERA_INVERT_H + CAMERA_ROW * k] {
                    m.ram[a..a + 4].fill(0);
                }
            }
            camera_normal = true;
            println!("play: field {f}: a new game's camera NORMAL (UP/DOWN and LEFT/RIGHT MOVEMENT not inverted)");
        }
        let t = Instant::now();
        let out = m.field(pad.report()).map_err(|e| format!("play: field {f}: {e}"))?;
        audio.field()?;
        if let Some(p) = out.preempted {
            *preempted.entry(p).or_default() += 1;
        }
        // The field's last frame is drawn with the RAM as it stood at its first kick, the state its lists were built from
        // (`uyart::hw`'s kick); a field with no frame kicked keeps the RAM at its start.
        if out.kicked {
            std::mem::swap(&mut before, &mut m.frame_ram);
        } else if !before_set {
            m.ram.copy_to(&mut before)?;
        }
        if out.earlier > 0 {
            println!("play: field {f}: {} jobs of an earlier frame in the field, its last frame drawn", out.earlier);
        }
        // A main-frame read-back in the frames before the field's last (the game saving the picture shown: a pause's first
        // frame), which the renderer does not get: given with the next frame it draws (`read_back`).
        if out.earlier > 0 {
            read_back.extend(main_read_back(&crate::jobs::Kicked::new(f, out.earlier_jobs, Vec::new()).into_frame(&before)?, &boot));
        }
        // The transfers of the frames before the field's last, and the last's own.
        let mut uploads = out.uploads;
        let own = uploads.split_off(uploads.partition_point(|u| u.job < out.earlier));
        pending.extend(uploads);
        let ee_ms = t.elapsed().as_secs_f64() * 1e3;
        ns[1] += t.elapsed().as_nanos() as u64;
        let lines = menus.lines(&m.ram[..RAM]);
        let tab_bar = menus.tab_bar(&m.ram[..RAM]);
        input.lock().expect("input lock").kbm.lock().expect("input lock").tab_bar = tab_bar;
        let now = menus.menu(&m.ram[..RAM]);
        if now != menu {
            menu = now;
            let _ = events.send_event(Event::Menu(menu));
        }
        let t = Instant::now();
        skin_before.push_back((f, std::sync::Arc::new(out.skin)));
        if skin_before.len() > crate::mobies::SKIN_FIELDS as usize {
            skin_before.pop_front();
        }
        // The field's jobs, its frame's draw lists still in the frame's RAM copy (`before`): built from it where they are walked.
        let jobs = crate::jobs::Kicked::new(f, out.jobs, skin_before.iter().cloned().collect());
        // --control: whether this field ends the running command (then its frame is drawn before the answer). An `until pause`
        // also asks whether the field's jobs carry a world frame, as the renderer does.
        let settle = match control.as_mut() {
            Some(c) => {
                let world = match (c.wants_world(), &ctl_boot) {
                    (true, Some(boot)) => {
                        let jobs = jobs.frame(&before)?;
                        crate::mobies::walk(&jobs, boot, true, None).or_else(|_| crate::mobies::walk(&jobs, boot, false, None)).is_ok_and(|w| w.has_runs() || w.ran.iter().any(|&r| r))
                    }
                    _ => true,
                };
                c.ran(&mut crate::control::State { field: f, cpu: &mut RtGame(&mut m, &mut warps, &mut menus, f), menu, world, shot: &mut shot })
            }
            None => false,
        };
        // The memory card messages, the front end's and a pause's SAVE screen's while it checks the card: answered for the player
        // and never shown (`play::MC_MESSAGE`). A message hidden in the front end stays the front end's until it goes, while the
        // front end's program is in RAM: LOAD GAME's "Loading Data" (23) is still up when the save it reads names its level
        // (level_current, some 110 fields before the box goes), and those fields' frames were shown, the box over the slots, in
        // the place and shape of the arrival's space scene that a level being entered gives a front end's frame (`play.rs`,
        // `render_screen_walked`'s fill): the whole screen zoomed in for some two seconds after a save was clicked.
        let front_end = matches!(rd32(&m.ram, crate::LEVEL_CURRENT), 0 | u32::MAX) || (m.front_end() && MC_HIDDEN.contains(&message));
        let now = if front_end || menus.save_state(&m.ram[..RAM]).is_some_and(|s| s >= SAVE_CARD) {
            rd32(&m.ram, MC_MESSAGE)
        } else {
            match menus.save_words(&m.ram[..RAM]) {
                Some((message, frames)) => {
                    let word = rd32(&m.ram, message);
                    if word != level_word {
                        println!("play: field {f}: the level's save message word {word}");
                        level_word = word;
                    }
                    if word == LEVEL_AUTOSAVE {
                        let n = rd32(&m.ram, frames).max(121);
                        m.ram[frames as usize..frames as usize + 4].copy_from_slice(&n.to_le_bytes());
                        LEVEL_SAVING
                    } else {
                        0
                    }
                }
                None => 0,
            }
        };
        if now != message {
            println!("play: field {f}: memory card message {now}");
        }
        prompt = if now == message { prompt + 1 } else { 0 };
        message = now;
        // The control channel's `dump DIR`: this field's RAM and jobs into DIR.
        let dump_dir = control.as_mut().and_then(|c| c.take_dump());
        if ram_at.contains(&f) || dump_dir.is_some() {
            let dir = dump_dir.map_or(String::new(), |d| format!("{d}/"));
            // The RAM the renderer draws the field's jobs with (the frame below): as it stood when the field began, the state
            // the EE built the chain from (RAM after the field is a field on, and its tie list names records the chain did not send);
            // in a field of more than one frame, as it stood when the last frame's lists were kicked.
            std::fs::write(format!("{dir}ram{f:05}.bin"), &before[..]).map_err(|e| format!("play: RAM dump: {e}"))?;
            // The field's jobs beside it, as `frames --ram-dir` writes them: `uya native` redraws the field from the two.
            jobs.frame(&before)?.write(&format!("{dir}jobs{f:05}.bin"))?;
            println!("play: field {f}: RAM written to {dir}, {:.1} s", t0.elapsed().as_secs_f64());
        }
        if MC_HIDDEN.contains(&message) || message == LEVEL_SAVING {
            // The memory card message is not shown: the window keeps the picture before it.
            pending.extend(own);
            m.ram.copy_to(&mut before)?;
            before_set = true;
            next = Instant::now();
            continue;
        }
        // The read-back thread's finished walks: their pages for the next frame drawn, their buffers spare.
        while let Ok((pages, ram)) = read_backs.try_recv() {
            read_back.extend(pages?);
            spare.push(ram);
            reading_back -= 1;
        }
        // A free RAM buffer for the frame: only one at hand, paced or not (the renderer busy: the field is not drawn, and the game
        // does not wait for a renderer a frame late: it drops a picture, not game time). The game waits for a renderer held on one
        // frame for more than two fields (a level's load, a long build: `play::renderer_held`), as on a loading screen, and a
        // control command's last field waits: its answer comes once its frame is drawn.
        let buffer = match spare.pop() {
            Some(b) => Some(b),
            None if settle || crate::play::renderer_held() => match back.recv() {
                Ok(b) => Some(b),
                Err(_) => return Ok(GameEnd::RendererStopped),
            },
            None => match back.try_recv() {
                Ok(b) => Some(b),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return Ok(GameEnd::RendererStopped),
            },
        };
        match buffer {
            None => {
                pending.extend(own);
                read_back.extend(main_read_back(&jobs.into_frame(&before)?, &boot));
                m.ram.copy_to(&mut before)?;
                before_set = true;
            }
            Some(mut now) => {
                // The pages of the frames the read-back thread still holds go with this one.
                while reading_back > 0 {
                    let (pages, ram) = read_backs.recv().map_err(|_| "play: the read-back thread stopped")?;
                    read_back.extend(pages?);
                    spare.push(ram);
                    reading_back -= 1;
                }
                // The next field's start, needed only if it kicks no frame (`before_set`).
                before_set = !out.kicked;
                if before_set {
                    m.ram.copy_to(&mut now)?;
                }
                let ram = std::mem::replace(&mut before, now);
                let prompts = input.lock().expect("input lock").prompts();
                let msg = Msg::Field(Frame { field: f, at, ram, jobs, read_back: std::mem::take(&mut read_back), uploads_before: std::mem::take(&mut pending), uploads: own, front_end: m.front_end(), prompts, menu, lines, pointer: pointer.at, bindings: menus.bindings });
                if !settle {
                    match tx.try_send(msg) {
                        Ok(()) => drawn += 1,
                        Err(std::sync::mpsc::TrySendError::Full(msg)) if crate::play::renderer_held() => {
                            if tx.send(msg).is_err() {
                                return Ok(GameEnd::RendererStopped);
                            }
                            drawn += 1;
                        }
                        Err(std::sync::mpsc::TrySendError::Full(Msg::Field(fr))) => {
                            read_back = fr.read_back;
                            to_read_back.send((fr.jobs, fr.ram)).map_err(|_| "play: the read-back thread stopped")?;
                            reading_back += 1;
                            pending = fr.uploads_before;
                            pending.extend(fr.uploads);
                        }
                        Err(_) => return Ok(GameEnd::RendererStopped),
                    }
                } else if tx.send(msg).is_err() {
                    return Ok(GameEnd::RendererStopped);
                }
                // A control command's last field: answered once its frame is drawn.
                if settle {
                    let (rtx, rrx) = std::sync::mpsc::channel();
                    if tx.send(Msg::Drawn(rtx)).is_err() || rrx.recv().is_err() {
                        return Ok(GameEnd::RendererStopped);
                    }
                }
            }
        }
        if crate::play::frame_times() { println!("frame stages: game {f} EE {ee_ms:.3} handover {:.3}{}", t.elapsed().as_secs_f64() * 1e3, if settle { " settle" } else { "" }); }
        ns[2] += t.elapsed().as_nanos() as u64;
        if f % 300 == 0 {
            let ms = |n: u64| n as f64 / 1e6 / 300.0;
            let busy: u64 = preempted.values().sum();
            let p = m.take_prof();
            let (vu0, dma, copy) = (p.vu0, p.dma, p.copy);
            // The shares as ms a field (of the EE's), and counts a field.
            let (ee, field) = (ms(ns[1]), |n: u64| n as f64 / 300.0);
            const CHANNELS: [&str; 10] = ["VIF0", "VIF1", "GIF", "from IPU", "to IPU", "SIF0", "SIF1", "SIF2", "from SPR", "to SPR"];
            let kicks: Vec<String> = p.kicks.iter().zip(CHANNELS).filter(|(k, _)| k.0 > 0).map(|((n, s), name)| format!("{name} {:.1} ({:.2} ms)", field(*n), s * ee)).collect();
            let progs: Vec<String> = p.vu0_progs.iter().map(|(name, n, s)| format!("{name} {:.1} ({:.2} ms)", field(*n), s * ee)).collect();
            // Each kick's own work once (a kick an end handler makes inside another is not in that one's), and the end handlers' game
            // code apart (`uyart::hw::kick_timed`).
            let costs = format!("; a field: DMA kicks {}, the DMA end handlers' game code in them {:.2} ms; VU0 microprogram runs {}; turns passed between the game's threads and the host {:.1} ({:.2} ms from a turn given to its thread running)", if kicks.is_empty() { "none".into() } else { kicks.join(", ") }, p.handlers * ee, if progs.is_empty() { "none".into() } else { progs.join(", ") }, field(p.turns.0), p.turns.1 * ee);
            let costs = match uyart::hw::kick_check() {
                Some([chains, differ, streams, events]) => format!("{costs}; UYA_KICK_CHECK: {chains} chains built both ways, {differ} differ; the GS events of {streams} VIF1 kicks found both ways, {events} differ"),
                None => costs,
            };
            let copied = m.ram.take_copied() as f64 / 300.0 / (1 << 20) as f64;
            let top = preempted.iter().max_by_key(|e| e.1).map(|((t, pc, ra), n)| format!(", most at thread {t} {pc:08x} (ra {ra:08x}) {n}")).unwrap_or_default();
            let shown = format!(", {drawn} of the last 300 drawn");
            let recent = 300.0 / last_line.elapsed().as_secs_f64();
            last_line = Instant::now();
            println!("play: field {f}, {:.1} s: {:.1} fields a second, the last 300 at {recent:.2}; ms per field: input {:.2}, EE {:.1}, hand-over {:.1}{shown} (the port's runtime); of the EE's time VU0 microprograms {:.0} %, DMA kicks {:.0} % (of them the frames' RAM copies {:.1} %, {copied:.1} MB a field copied in all); fields that ended with a thread running {busy}{top}; {}{costs}", t0.elapsed().as_secs_f64(), f as f64 / t0.elapsed().as_secs_f64(), ms(ns[0]), ms(ns[1]), ms(ns[2]), vu0 * 100.0, dma * 100.0, copy * 100.0, m.k.summary());
            ns = [0; 3];
            drawn = 0;
            preempted.clear();
        }
        if unpaced {
            next = Instant::now();
            continue;
        }
        // At most one field per 1/59.94 s; a field that runs late is not made up by running the next ones early.
        next += FIELD;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else {
            next = now;
        }
    }
    unreachable!("the field loop returns")
}

/// The main frame's pages that `jobs` read back before their first primitive (`mobies::Transfer`, TRXDIR 1 at index 0: the game
/// keeping the picture shown, which it puts back under a pause's menus). The renderer grabs its picture shown for them
/// (`native::Native::render`); jobs it does not get (an earlier frame in a field, a field it could not take) carry them to the
/// next frame it draws. Jobs the walk cannot read read back nothing.
fn main_read_back(jobs: &crate::jobs::FrameJobs, boot: &uyafmt::boot::Boot) -> Vec<u32> {
    let Ok(w) = crate::mobies::walk(jobs, boot, false, None) else { return Vec::new() };
    w.transfers.iter().filter(|t| t.dir == 1 && t.index == 0).filter_map(|t| t.main_pages().ok()).flatten().collect()
}
