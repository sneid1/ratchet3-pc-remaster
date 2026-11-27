//! The port's PC settings (PLAN items 4, 8 and 10): the settings a PC game has and the PS2 game does not, saved in the player's
//! profile (`settings.txt`) as soon as one changes and applied by `play.rs` at once. They are set in the game's own Options
//! screen, on the settings page `menus.rs` makes of the game's own option list: a tab per section (`TABS`), a row per
//! option, the highlighted option's description under the list, a quality preset at the top of GRAPHICS and a reset at the
//! bottom of each tab. The picture takes the shape of what it is shown on, the monitor in fullscreen or the window (PLAN 4.2,
//! `uyart::set_view_k`), unless WIDESCREEN is off (the original 4:3 picture).
//!
//! **An option is one entry of `OPTIONS`**: its key in `settings.txt`, its tab, its label, its values (as the file writes them
//! and as the row shows them), its default, its value at each quality preset (GRAPHICS only) and its description. The page,
//! the file and the presets are made from the table; the code an option drives reads its value by key (`Settings::get`, and
//! `enhance::level` on the renderer's side). Every GRAPHICS option's first value is off, and off is its default: the Original
//! preset is the game as it was (PLAN 8: every feature off by default).

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Display {
    Windowed,
    Fullscreen,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Display,
    Graphics,
    /// The camera's options (CAMERA).
    Controls,
    /// The bindings (`bindings.rs`): the keyboard and mouse's, a controller's.
    Keyboard,
    Controller,
    Gameplay,
    Audio,
}

/// The tabs in the order the page shows them, with their titles.
pub const TABS: [(Tab, &str); 7] = [
    (Tab::Display, "DISPLAY"),
    (Tab::Graphics, "GRAPHICS"),
    (Tab::Controls, "CAMERA"),
    (Tab::Keyboard, "KEYBOARD"),
    (Tab::Controller, "CONTROLLER"),
    (Tab::Gameplay, "GAMEPLAY"),
    (Tab::Audio, "AUDIO"),
];

pub struct Opt {
    /// The key in `settings.txt`.
    pub key: &'static str,
    pub tab: Tab,
    /// The row's label.
    pub name: &'static str,
    /// Each value as `settings.txt` writes it and as the row shows it.
    pub values: &'static [(&'static str, &'static str)],
    /// The value of a profile whose file does not name the option (and after a reset).
    pub default: u8,
    /// The value at the presets Low, Medium, High and Ultra (Original is every option's first value); None: no preset sets it.
    pub presets: Option<[u8; 4]>,
    /// The description shown while the row is highlighted: what the option does and what it costs.
    pub about: &'static str,
}

/// Every option, in the order of its tab's rows.
pub const OPTIONS: &[Opt] = &[
    Opt {
        key: "display",
        tab: Tab::Display,
        name: "DISPLAY MODE",
        values: &[("windowed", "WINDOWED"), ("fullscreen", "FULLSCREEN")],
        default: 0,
        presets: None,
        about: "Windowed, or fullscreen borderless on the window's monitor. The picture takes the shape of the window or monitor.",
    },
    // The heights below 1080 for integrated and older GPUs (a v0.1.3 player on a Ryzen 5 4600G's Radeon graphics: 20 to 22
    // pictures a second in Veldin at 1080, "nothing I do changes the fact that it stays at 20 fps": no setting was cheaper than
    // the Original preset at 1080). A picture smaller than the window is brought up to it edge-directed and sharpened
    // (`play.rs` `Screen::window_copy`), as the movies are.
    Opt {
        key: "resolution",
        tab: Tab::Display,
        name: "RESOLUTION",
        values: &[("540", "540P"), ("720", "720P"), ("900", "900P"), ("1080", "1080P"), ("1440", "1440P"), ("2160", "2160P (4K)")],
        default: 3,
        presets: None,
        about: "The height the world is drawn at, scaled to the window. Lower is faster on integrated and older GPUs; higher is sharper.",
    },
    // PLAN 4.2: the picture at the screen's shape (Hor+: more of the world at the sides, the game's own view in the middle), or the
    // game's original 4:3 picture with bars at the sides. The game's own "16:9 (WIDESCREEN)" row (text 0x508, in the front end's
    // OPTIONS and a pause's AUDIO / VISUAL) shows and changes this setting (`menus.rs`); the game's own 16:9 option, which crops the
    // picture top and bottom (view_projection_setup divides the vertical view by 4/3), is held off (`rtgame.rs`, WIDESCREEN_BYTE).
    Opt {
        key: "widescreen",
        tab: Tab::Display,
        name: "WIDESCREEN",
        values: &[("on", "ON"), ("off", "OFF")],
        default: 0,
        presets: None,
        about: "On: the picture fills the screen's shape, more of the world at the sides. Off: the original 4:3 picture.",
    },
    // The FPS counter (`play.rs` `Fps`): the pictures the window showed in the last second, in its top right corner (the player,
    // 17 November 2026: "Add an FPS counter and FPS counter option in settings").
    Opt {
        key: "fps_counter",
        tab: Tab::Display,
        name: "FPS COUNTER",
        values: &[("off", "OFF"), ("on", "ON")],
        default: 0,
        presets: None,
        about: "The pictures shown each second, in the top right corner of the screen.",
    },
    // FRAME PACING (`play.rs` `Timeline`; the player, 24 November 2026: "do the uncapped mode like other games"): UNCAPPED draws
    // the pictures between the game's frames back to back, as many as the GPU keeps up with, each drawn for the refresh it is
    // presented to and shown there (a picture the GPU cannot finish in time shows a refresh late, and the next is drawn for the
    // refresh it reaches). EVEN keeps a fixed pace, a picture every N refreshes (360, 180, 120 at 360 Hz), and steps to a slower
    // one only after the GPU has failed to keep it for a second.
    Opt {
        key: "frame_pacing",
        tab: Tab::Display,
        name: "FRAME PACING",
        values: &[("uncapped", "UNCAPPED"), ("even", "EVEN")],
        default: 0,
        presets: None,
        about: "Uncapped: as many pictures as the PC can draw, like other PC games. Even: a fixed pace that stays on one rate until the PC cannot keep it.",
    },
    Opt {
        key: "texture_filtering",
        tab: Tab::Graphics,
        name: "TEXTURE FILTERING",
        values: &[("off", "OFF"), ("2x", "2X"), ("4x", "4X"), ("8x", "8X"), ("16x", "16X")],
        default: 0,
        presets: Some([1, 2, 4, 4]),
        about: "Anisotropic: the game's own textures stay sharp on ground seen at a shallow angle. Off is the PS2's filtering. Very low cost.",
    },
    Opt {
        key: "ambient_occlusion",
        tab: Tab::Graphics,
        name: "AMBIENT OCCLUSION",
        values: &[("off", "OFF"), ("low", "LOW"), ("high", "HIGH")],
        default: 0,
        presets: Some([0, 1, 2, 2]),
        about: "Soft shade in corners, under props and where characters stand, faded by the fog. Low cost; High is smoother.",
    },
    Opt {
        key: "water_reflections",
        tab: Tab::Graphics,
        name: "WATER REFLECTIONS",
        values: &[("off", "OFF"), ("low", "LOW"), ("high", "HIGH")],
        default: 0,
        presets: Some([0, 1, 2, 2]),
        about: "Water, swamp and slime mirror what stands above them, most toward the horizon. Low cost; High is sharper.",
    },
    Opt {
        key: "light_shafts",
        tab: Tab::Graphics,
        name: "LIGHT SHAFTS",
        values: &[("off", "OFF"), ("low", "LOW"), ("high", "HIGH")],
        default: 0,
        presets: Some([0, 1, 2, 2]),
        about: "Sunbeams through the level's haze where the sun shines past trees and buildings. Low cost; High is finer.",
    },
    // PLAN 9.2. The player, 15 November 2026: the Rift Apart version "isnt a different game version its the updated version. Its
    // the default and standard": on in a profile that has not set it and at every preset but Original (the game's own picture). It
    // draws the profile's `texture_replacements\0-riftapart` (`texnames::open`); on with no set there, the game stops at the first
    // texture it looks up, naming the folder. Shown as UPDATED TEXTURES since 18 November 2026 (the player: "just be updated
    // textures"); the key stays, so a profile's settings.txt keeps its choice.
    Opt {
        key: "rift_apart_surfaces",
        tab: Tab::Graphics,
        name: "UPDATED TEXTURES",
        values: &[("off", "OFF"), ("on", "ON")],
        default: 1,
        presets: Some([1, 1, 1, 1]),
        about: "The remaster's updated textures: new detail and colour on characters and the world, with surface grain, relief and gloss, lit per pixel. Some GPU memory.",
    },
    Opt {
        key: "lighting",
        tab: Tab::Graphics,
        name: "LIGHTING",
        values: &[("vertex", "VERTEX"), ("per_pixel", "PER-PIXEL")],
        default: 0,
        presets: Some([0, 1, 1, 1]),
        about: "Per-pixel: the ground, buildings, rocks and characters lit at every pixel by the game's own lights. Low cost.",
    },
    Opt {
        key: "shadows",
        tab: Tab::Graphics,
        name: "SHADOWS",
        // RAY TRACED (`raytrace.rs`): the same shadows found by the GPU's ray tracing hardware instead of shadow maps.
        values: &[("off", "OFF"), ("low", "LOW"), ("medium", "MEDIUM"), ("high", "HIGH"), ("ray_traced", "RAY TRACED")],
        default: 0,
        presets: Some([0, 1, 2, 3]),
        about: "Shadows cast by the level's sun from buildings, rocks, plants and characters. Ray traced (with DLSS on): sharp at every distance.",
    },
    // DLSS (`dlss.rs`): the world drawn at a lower size and made the picture's by NVIDIA's DLSS, which also smooths its edges; DLAA
    // keeps the picture's size and smooths only. The HUD and the menus are drawn after it at the picture's size. No preset takes
    // it: it needs an NVIDIA RTX GPU. The ray-traced options need it on (`Settings::constrain`).
    Opt {
        key: "dlss",
        tab: Tab::Graphics,
        name: "DLSS",
        values: &[("off", "OFF"), ("dlaa", "DLAA"), ("quality", "QUALITY"), ("balanced", "BALANCED"), ("performance", "PERFORMANCE")],
        default: 0,
        presets: None,
        about: "NVIDIA's upscaler: the world drawn smaller and rebuilt sharp, with smooth edges. DLAA smooths only. Ray tracing needs it on.",
    },
    // FRAME GENERATION (the player, 24 November 2026: "download it and add frame generation"): NVIDIA DLSS Frame Generation
    // through Streamline, generating 1, 2 or 3 frames between the pictures the port draws (`enhance::frame_generation`); the
    // presenter draws its own pictures that many refreshes apart (`play::FG_GENERATED`). It needs DLSS on (`constrain`).
    Opt {
        key: "frame_generation",
        tab: Tab::Graphics,
        name: "FRAME GENERATION",
        values: &[("off", "OFF"), ("2x", "2X"), ("3x", "3X"), ("4x", "4X")],
        default: 0,
        presets: None,
        about: "NVIDIA's frame generation: frames made between the drawn ones, up to the screen's rate. Needs DLSS and an RTX 40 or 50 card.",
    },
    // PLAN 8.10 (`rtlight.rs`): the game's light bounced once off what surrounds each surface, and with AMBIENT OCCLUSION on the
    // occlusion traced instead of read from the depth buffer. No preset takes it: it needs ray tracing hardware.
    Opt {
        key: "ray_traced_lighting",
        tab: Tab::Graphics,
        name: "RAY-TRACED LIGHTING",
        values: &[("off", "OFF"), ("low", "LOW"), ("high", "HIGH")],
        default: 0,
        presets: None,
        about: "Light bounced off sunlit walls and floors into shade, and ray-traced occlusion. Needs DLSS on; High is smoother.",
    },
    // PLAN 8.10 (`rtlight.rs`): water and the game's shiny surfaces reflect the level by rays, off screen too.
    Opt {
        key: "ray_traced_reflections",
        tab: Tab::Graphics,
        name: "RAY-TRACED REFLECTIONS",
        values: &[("off", "OFF"), ("low", "LOW"), ("high", "HIGH")],
        default: 0,
        presets: None,
        about: "Water and shiny metal reflect the whole level, even what is off screen. Needs DLSS on; High is sharper.",
    },
    // PLAN 10.1: the horizontal angle at 16:9 in degrees (`FOV_DEGREES`; the game's own view is 81 there). In play on the story
    // levels the game's view is made at that angle at 16:9, and at the same vertical angle at any other shape (Hor+), both of its
    // tangents scaled alike at the loads that make the view and decide what is seen (`uycpu::Host::fov`, uyatr's `VIEW_LOADS`
    // and `FOV_LOADS`): its culls, level of detail and targeting follow. Cutscenes, menus, the ship and the vid-comics keep the
    // game's own (`Settings::fov_factor`'s caller, `rtgame.rs`).
    Opt {
        key: "field_of_view",
        tab: Tab::Controls,
        name: "FIELD OF VIEW",
        values: &[("original", "ORIGINAL"), ("70", "70"), ("90", "90"), ("100", "100"), ("110", "110")],
        default: 0,
        presets: None,
        about: "How wide the camera sees in play, in degrees across a 16:9 screen; the game's own is 81. Cutscenes keep theirs.",
    },
    // PLAN 10: the camera's speed and inversion by the right stick and by the mouse, each on top of the game's own camera options
    // (its Options' CAMERA SPEED and LEFT/RIGHT, UP/DOWN MOVEMENT). The right stick's speed is a factor on the game's own option
    // where its cameras turn by it (`uycpu::Host::cam_speed`, uyatr's `CAMERA_SPEED`), so the stick turns faster or slower at
    // every push, its top speed too; the mouse's is a factor on the mouse's turn; the inversions flip the device's input before
    // the game reads it (`input::Camera`). 100% and OFF are the game's own.
    Opt {
        key: "stick_camera_speed",
        tab: Tab::Controls,
        name: "STICK SPEED",
        values: SPEED_VALUES,
        default: SPEED_GAME,
        presets: None,
        about: "How fast the right stick turns the camera, on top of the game's own camera speed option. 100% is the game's.",
    },
    Opt {
        key: "stick_camera_invert",
        tab: Tab::Controls,
        name: "INVERT STICK",
        values: INVERT_VALUES,
        default: 0,
        presets: None,
        about: "Turns the camera the other way with the right stick: up and down, left and right, or both. Off is the game's.",
    },
    Opt {
        key: "mouse_camera_speed",
        tab: Tab::Controls,
        name: "MOUSE SPEED",
        values: SPEED_VALUES,
        default: SPEED_GAME,
        presets: None,
        about: "How far the camera turns for a move of the mouse. 100% is the port's own.",
    },
    Opt {
        key: "mouse_camera_invert",
        tab: Tab::Controls,
        name: "INVERT MOUSE",
        values: INVERT_VALUES,
        default: 0,
        presets: None,
        about: "Turns the camera the other way with the mouse: up and down, left and right, or both.",
    },
    // The free-look camera (the mouse aim's lock-strafe camera, and a pad's in that camera option) turns by itself to keep a
    // target in view while the hero moves: fps_cam_update's strafe tracking, by its yaw and pitch gains (`rtgame.rs`
    // `FPS_TRACK`), which this scales; the target, the aim point and the crosshair are the game's own at any value (a v0.1.3
    // player: "lock-on camera magnetism pulls back toward targets and fights mouse/free-look", "a toggle/slider to reduce or
    // disable lock-on camera pull").
    Opt {
        key: "lock_on_camera_pull",
        tab: Tab::Controls,
        name: "LOCK-ON CAMERA PULL",
        values: &[("0", "OFF"), ("25", "25%"), ("50", "50%"), ("75", "75%"), ("100", "100%")],
        default: 4,
        presets: None,
        about: "How strongly the camera turns by itself to keep a target in view while you move. 100% is the game's own; off stops it. Aiming is not changed.",
    },
    // The controllers' sticks' round dead zones (`input::shape`), in place of the game's own square one (38 % of each axis, the
    // DualShock 2's), which a pad's stick never reached past on a diagonal. No zero: the game turns any push of the left stick
    // into the d-pad in its menus, so a stick that drifts with no zone would move them.
    Opt {
        key: "left_stick_dead_zone",
        tab: Tab::Controller,
        name: "MOVE STICK DEAD ZONE",
        values: DEAD_VALUES,
        default: 2,
        presets: None,
        about: "How far the left stick is pushed before Ratchet moves. Raise it if he creeps with the stick let go. The stick's edge always runs at full speed.",
    },
    Opt {
        key: "right_stick_dead_zone",
        tab: Tab::Controller,
        name: "CAMERA STICK DEAD ZONE",
        values: DEAD_VALUES,
        default: 3,
        presets: None,
        about: "How far the right stick is pushed before the camera turns. Raise it if the camera drifts with the stick let go.",
    },
    // PLAN 10: a level change (the travel between planets, a level's start from the front end) runs as fast as the PC goes
    // instead of at the game's 59.94 fields a second (`rtgame.rs`, `LEVEL_CHANGE_REQUEST`): the game does the same, sooner. The
    // memory card's saves already run so (their messages are answered for the player and not shown).
    Opt {
        key: "fast_loading",
        tab: Tab::Gameplay,
        name: "FAST LOADING",
        values: &[("off", "OFF"), ("on", "ON")],
        default: 0,
        presets: None,
        about: "Level loads and travel between planets run as fast as the PC can instead of at the PS2's pace. The game does the same, sooner.",
    },
    // PLAN 10: the game lets any button skip a cutscene at once (the movies too, here); AFTER FIRST VIEWING plays each through
    // the first time, remembered per cutscene in the profile (`seen.rs`), and lets it be skipped as the game does after that.
    Opt {
        key: "cutscene_skip",
        tab: Tab::Gameplay,
        name: "CUTSCENE SKIP",
        values: &[("any_time", "ANY TIME"), ("after_first_viewing", "AFTER FIRST VIEWING")],
        default: 0,
        presets: None,
        about: "After first viewing: each cutscene and movie plays through once, then skips. Any time: the game's own.",
    },
    // PLAN 10: HIGH QUALITY mixes the game's sound at the output device's own rate (from the next start: `audio.rs`), each voice
    // reading its sample with a windowed sinc (`snd::sinc`) instead of the 4-point Hermite, the device's read and the movies'
    // sound the same; ORIGINAL is the 48 kHz mix.
    Opt {
        key: "audio_mixing",
        tab: Tab::Audio,
        name: "MIXING",
        values: &[("original", "ORIGINAL"), ("high_quality", "HIGH QUALITY")],
        default: 0,
        presets: None,
        about: "High quality: the mix at your device's own rate (from the next start), with a sinc resampler. Original: 48 kHz.",
    },
    // PLAN 10: CLEAN runs the PS2 sound chip's reverb network from the game's own reverb settings in floating point at the mix's
    // rate (`snd/reverb.rs`, `Clean`): the same rooms, without the chip's 24 kHz band limit and 16-bit grain.
    Opt {
        key: "reverb",
        tab: Tab::Audio,
        name: "REVERB",
        values: &[("original", "ORIGINAL"), ("clean", "CLEAN")],
        default: 0,
        presets: None,
        about: "Clean: the game's own reverb rooms at full bandwidth, without the PS2 chip's 11 kHz limit and grain.",
    },
];

/// The camera speeds (percent) and their factors.
const SPEED_VALUES: &[(&str, &str)] = &[("25", "25%"), ("50", "50%"), ("75", "75%"), ("100", "100%"), ("150", "150%"), ("200", "200%"), ("300", "300%")];
const SPEEDS: [f32; 7] = [0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0];
const SPEED_GAME: u8 = 3;

/// The camera inversions: (left and right flipped, up and down flipped).
const INVERT_VALUES: &[(&str, &str)] = &[("off", "OFF"), ("vertical", "UP / DOWN"), ("horizontal", "LEFT / RIGHT"), ("both", "BOTH")];
const INVERTS: [(bool, bool); 4] = [(false, false), (false, true), (true, false), (true, true)];

/// The sticks' dead zones (percent of a stick's travel) and their shares. 15 % and 20 % (left, right) lie above common stick
/// drift (up to about 8 %) and under XInput's own 24 % and 27 %.
const DEAD_VALUES: &[(&str, &str)] = &[("5", "5%"), ("10", "10%"), ("15", "15%"), ("20", "20%"), ("25", "25%"), ("30", "30%"), ("40", "40%")];
const DEADS: [f32; 7] = [0.05, 0.10, 0.15, 0.20, 0.25, 0.30, 0.40];

/// LOCK-ON CAMERA PULL's factors on the game's own pull.
const PULLS: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];

/// The field of view's values in degrees across at 16:9 (0: the game's own view, 81 degrees there: its vertical half-angle
/// tangent 0.4805 at the gameplay camera's horizontal 0.62).
const FOV_DEGREES: [f32; 5] = [0.0, 70.0, 90.0, 100.0, 110.0];

/// The game's vertical half-angle tangent in play (0.62 x 0.775) times 16/9: the horizontal tangent at 16:9 of its own view.
const TAN_16_9: f32 = 0.4805 * 16.0 / 9.0;

/// The quality presets at the top of GRAPHICS; a page whose values are none of them shows CUSTOM.
pub const PRESETS: [&str; 5] = ["ORIGINAL", "LOW", "MEDIUM", "HIGH", "ULTRA"];
pub const CUSTOM: &str = "CUSTOM";

const N: usize = OPTIONS.len();

/// The option with key `key`; a key the table does not have is a mistake in the code that asks.
pub fn index(key: &str) -> usize {
    OPTIONS.iter().position(|o| o.key == key).unwrap_or_else(|| panic!("settings: no option \"{key}\""))
}

/// The resolutions offered: the native picture's height (the resolution option's values).
pub const HEIGHTS: [u32; 6] = [540, 720, 900, 1080, 1440, 2160];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Settings {
    /// Each option's value, by its place in `OPTIONS`.
    pub values: [u8; N],
}

/// The options as `settings.txt` names them, for the play log (a player's report names what was on).
impl std::fmt::Display for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for (k, (o, v)) in OPTIONS.iter().zip(self.values).enumerate() {
            write!(f, "{}{} {}", if k == 0 { "" } else { ", " }, o.key, o.values[v as usize].0)?;
        }
        Ok(())
    }
}

impl Default for Settings {
    /// Every option at its default: a profile with no settings file yet plays windowed at 1080p, the graphics as the game's.
    fn default() -> Settings {
        let mut values = [0u8; N];
        for (v, o) in values.iter_mut().zip(OPTIONS) {
            *v = o.default;
        }
        Settings { values }
    }
}

impl Settings {
    pub fn get(&self, key: &str) -> u8 {
        self.values[index(key)]
    }

    pub fn display(&self) -> Display {
        match self.get("display") {
            0 => Display::Windowed,
            _ => Display::Fullscreen,
        }
    }

    pub fn height(&self) -> u32 {
        HEIGHTS[self.get("resolution") as usize]
    }

    /// Whether the picture takes the screen's shape (WIDESCREEN ON) or keeps the original 4:3 one.
    pub fn wide(&self) -> bool {
        self.get("widescreen") == 0
    }

    /// The right stick's camera speed: the factor on the game's own camera speed option (`uyart::set_camera_speed`).
    pub fn stick_speed(&self) -> f32 {
        SPEEDS[self.get("stick_camera_speed") as usize]
    }

    /// The camera settings the input applies (`input::Camera`).
    pub fn camera(&self) -> crate::input::Camera {
        crate::input::Camera {
            mouse_speed: SPEEDS[self.get("mouse_camera_speed") as usize],
            mouse_invert: INVERTS[self.get("mouse_camera_invert") as usize],
            stick_invert: INVERTS[self.get("stick_camera_invert") as usize],
            dead: (DEADS[self.get("left_stick_dead_zone") as usize], DEADS[self.get("right_stick_dead_zone") as usize]),
        }
    }

    /// LOCK-ON CAMERA PULL's factor on the free-look camera's own target tracking (`rtgame.rs`): 1.0 for the game's own.
    pub fn lock_pull(&self) -> f32 {
        PULLS[self.get("lock_on_camera_pull") as usize]
    }

    /// The field of view's factor on the game's view tangents (`uyart::set_view_fov`): 1.0 for the game's own.
    pub fn fov_factor(&self) -> f32 {
        let deg = FOV_DEGREES[self.get("field_of_view") as usize];
        if deg == 0.0 { 1.0 } else { (deg.to_radians() / 2.0).tan() / TAN_16_9 }
    }

    /// The preset the GRAPHICS options are at, if they are at one (None: CUSTOM).
    pub fn preset(&self) -> Option<usize> {
        (0..PRESETS.len()).find(|&p| OPTIONS.iter().zip(self.values).all(|(o, v)| o.presets.is_none() || v == Settings::at_preset(o, p)))
    }

    fn at_preset(o: &Opt, p: usize) -> u8 {
        match (p, o.presets) {
            (0, _) | (_, None) => 0,
            (p, Some(l)) => l[p - 1],
        }
    }

    /// Every GRAPHICS option to preset `p`'s value.
    pub fn set_preset(&mut self, p: usize) {
        for (v, o) in self.values.iter_mut().zip(OPTIONS) {
            if o.presets.is_some() {
                *v = Settings::at_preset(o, p);
            }
        }
    }

    /// Tab `tab`'s options back to their defaults.
    pub fn reset(&mut self, tab: Tab) {
        for (v, o) in self.values.iter_mut().zip(OPTIONS) {
            if o.tab == tab {
                *v = o.default;
            }
        }
    }

    /// The ray tracing only with DLSS on (the player, 16 November 2026: "change the settings so ray tracing can only be used when
    /// DLSS is on"): with DLSS off, RAY-TRACED LIGHTING and REFLECTIONS are off and RAY TRACED shadows are HIGH. Every change of
    /// the settings ends here (the page's, a preset, a reset, the file's, `--set`), so a ray-traced option chosen with DLSS off
    /// stays off.
    pub fn constrain(&mut self) {
        // DLSS off where this PC cannot run it (`dlss::unavailable`, known once the renderer has made its device).
        if crate::dlss::unavailable().is_some() {
            self.values[index("dlss")] = 0;
        }
        if self.get("dlss") == 0 {
            self.values[index("frame_generation")] = 0;
            for key in ["ray_traced_lighting", "ray_traced_reflections"] {
                self.values[index(key)] = 0;
            }
            let shadows = index("shadows");
            if self.values[shadows] == crate::raytrace::RAY_TRACED {
                self.values[shadows] = crate::raytrace::RAY_TRACED - 1;
            }
        }
    }

    /// `KEY=VALUE` pairs (as `settings.txt` writes them, separated by commas) over these settings (`uya native --set`).
    pub fn set_pairs(&mut self, pairs: &str) -> Result<(), String> {
        for p in pairs.split(',').filter(|p| !p.is_empty()) {
            let (k, v) = p.split_once('=').ok_or_else(|| format!("settings: \"{p}\" is not KEY=VALUE"))?;
            self.set_named(k.trim(), v.trim())?;
        }
        self.constrain();
        Ok(())
    }

    fn set_named(&mut self, key: &str, value: &str) -> Result<(), String> {
        let i = OPTIONS.iter().position(|o| o.key == key).ok_or_else(|| format!("settings: unknown setting \"{key}\""))?;
        let o = &OPTIONS[i];
        let v = o.values.iter().position(|(f, _)| *f == value).ok_or_else(|| format!("settings: {key} = {value} ({})", o.values.iter().map(|(f, _)| *f).collect::<Vec<_>>().join(", ")))?;
        self.values[i] = v as u8;
        Ok(())
    }

    /// The profile's settings (`key = value` lines); a profile without the file has not set any yet, and an option the file does
    /// not name is at its default.
    pub fn load(path: &str) -> Result<Settings, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::default()),
            Err(e) => return Err(format!("{path}: {e}")),
        };
        let mut s = Settings::default();
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let (k, v) = line.split_once('=').map(|(k, v)| (k.trim(), v.trim())).ok_or_else(|| format!("{path}: \"{line}\" is not key = value"))?;
            if OPTIONS.iter().all(|o| o.key != k) {
                // A key a previous build wrote and this one no longer has (a removed setting): left at its default, and
                // gone from the file the next time it is saved.
                println!("settings: unknown setting \"{k}\" ({path}); left at its default");
                continue;
            }
            s.set_named(k, v).map_err(|e| format!("{path}: {e}"))?;
        }
        s.constrain();
        Ok(s)
    }

    pub fn save(&self, path: &str) -> Result<(), String> {
        let text: String = OPTIONS.iter().zip(self.values).map(|(o, v)| format!("{} = {}\n", o.key, o.values[v as usize].0)).collect();
        std::fs::write(path, text).map_err(|e| format!("{path}: {e}"))
    }
}
