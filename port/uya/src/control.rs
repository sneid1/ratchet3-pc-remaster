//! Driving a running game for checks: the game's own level request from anywhere (the front end included), and the control
//! channel of `uya play --control` (one command a line on stdin, one answer on stdout; the game advances only when told).
//!
//! The channel's commands (each answered by one line starting `ctl: ok` or `ctl: error`, the log lines around it untouched):
//!   run N                      N fields
//!   press BUTTONS N            N fields with BUTTONS (hex, the pad bits `--press` takes: 40 Cross, 10 Triangle, 800 Start, 1000 Up, 4000 Down) held
//!   mouse DX DY N [BUTTONS]    N fields with the mouse moved DX, DY counts in each (raw motion: x right, y down), and BUTTONS
//!                              (hex, as `press`) held with it
//!   stick X Y N [BUTTONS]      N fields with the left stick held at X, Y (-1 to 1: x right, y forward; 0 0 centred), and
//!                              BUTTONS (hex, as `press`) held with it: the character walks and runs as with a pad
//!   controller BUTTONS N [LX LY RX RY]
//!                              N fields with an Xbox controller's buttons held (hex, each by the PS2 button it presses
//!                              unbound, as `press`): read as a plugged-in controller's are, through the bindings (the
//!                              CONTROLLER tab's swaps, its waiting row) and as the device in use; with its sticks held at
//!                              LX LY RX RY (-1 to 1: x right, y up, as XInput reports them), read as a controller's sticks
//!                              are (`input::shape`: the dead zones, the left stick's circle onto the square)
//!   look X Y N                 N fields with the right stick held at X, Y (-1 to 1: x right, y up), as a pad's: the camera
//!                              turns with the right stick's camera settings (PLAN 10)
//!   pointer X Y [click]        the pointer at X, Y (shares of the picture) from the next field, and the left button clicked
//!   wheel N                    the mouse wheel turned N notches (up positive) on the next field
//!   scene RECORD               the level's in-engine cutscene RECORD (its SCENE record), asked for as the game asks: its own
//!                              game_mode_request(2, 1, RECORD, 0, 0) between fields; the game fades out and plays it (mode 2),
//!                              then goes back to play (`until mode 0`)
//!   ask MODE ARG               the game's mode MODE with ARG, asked for as the level's own code asks: its game_mode_request(MODE,
//!                              1, ARG, 0, 0) between fields (3 with a screen of the pause's family: the Phoenix's consoles ask
//!                              for 15, QWARK COMICS, and 17 and 19; 5 with a vendor's page: 0 weapons, 2 ship, 3 armor)
//!   chunk N                    load the level's terrain and collision chunk N through the game, for a warp within a level
//!   warp LEVEL                 the game's own level request (the front end's or the level program's): now when the game
//!                              itself asks for levels (its front end, or a level in play), else at the first field it does
//!                              (a cutscene, a menu or the ship ends first; the answer says it waits, `until level` waits)
//!   until COND [max N]         fields until COND holds after one (pause: a level's pause screens up, drawn over its picture:
//!                              the game shows a menu and two fields running have sent no world, as the first such field
//!                              draws nothing over the picture yet and the next the dimmed picture and the pause list
//!                              (Veldin, fields 1005 and 1006 after Start at 1001); menu: any menu; level L: level L drawn,
//!                              its camera set, not loading (level 5 from --start: the level number is set at field 20, the
//!                              picture comes at 544; Al's message box then takes no Start until it closes); field F;
//!                              mode M: the game's current mode is M (0 in play, 2 a cutscene)),
//!                              at most N (default 20000); answered `ctl: ok field F` or `ctl: error ... after N fields`
//!   ram ADDR LEN               LEN bytes of EE RAM from ADDR (hex), answered in hex
//!   write ADDR HEX             bytes written to EE RAM at ADDR
//!   set KEY=VALUE[,...]        settings changed from the next field on, as settings.txt names them (not saved): one run shows
//!                              the same moment at two settings (DRAW DISTANCE ORIGINAL, then FAR a field later)
//!   shot FILE                  the picture the window shows, as the renderer drew it (not a window grab), to FILE as a PNG
//!   dump DIR                   the next field's RAM and jobs written to DIR (ramFFFFF.bin, jobsFFFFF.bin), as UYA_RAM_AT writes
//!                              them, for `uya native` to redraw (`uya native --surfaces`: PLAN 9.2's counts in gameplay)
//!   quit
//! A command that runs fields hands the last one's frame to the renderer before it is answered, so a `shot` after it is that
//! field's picture. The game thread calls `Control::field` before each field and `Control::ran` after it; `play.rs` and any other
//! game thread can drive the same channel.

use crate::LEVEL_CURRENT;
use uyafmt::levelcode::LevelGlobals;

/// The machine the channel drives: the port's runtime (`rtgame.rs`).
pub trait Game {
    fn rd8(&mut self, a: u32) -> u8;
    fn wr8(&mut self, a: u32, v: u8);
    fn rd32(&mut self, a: u32) -> u32;
    /// The game's own level request (`rtgame::request_level`).
    fn request_level(&mut self, level: u32) -> Result<String, String>;
    /// The game's own cutscene request (`rtgame::request_scene`).
    fn request_scene(&mut self, record: u32) -> Result<String, String>;
    fn request_mode(&mut self, mode: u32, arg: u32) -> Result<String, String>;
    fn request_chunk(&mut self, loader: u32, chunk: u32) -> Result<(), String>;
    fn loading_chunk(&self) -> bool;
    /// The game's current mode (0 in play, 2 a cutscene; `menus::Menus::game_mode`).
    fn game_mode(&mut self) -> Option<u32>;
    /// Settings changed from the next field on, as `KEY=VALUE` pairs (`settings::Settings::set_pairs`); not saved.
    fn set(&mut self, pairs: &str) -> Result<(), String>;
}

/// What the channel sees of the game at a field's edge.
pub struct State<'a> {
    /// The field about to run (`field`) or just run (`ran`).
    pub field: u64,
    pub cpu: &'a mut dyn Game,
    /// Whether the game showed a menu at the end of the last field.
    pub menu: bool,
    /// Whether the last field's jobs carry a world frame (the field in which the game pauses still sends the last world frame,
    /// with no pause screen yet); computed only while `Control::wants_world`.
    pub world: bool,
    /// Writes the picture the window shows to a PNG file (the renderer's own picture).
    pub shot: &'a mut dyn FnMut(&str) -> Result<(), String>,
}

/// What the field about to run gets from the channel.
pub struct Field {
    /// Pad buttons held this field (PS2 bits).
    pub buttons: u16,
    /// Raw mouse motion this field (counts: x right, y down).
    pub mouse: (f64, f64),
    /// The left stick this field (the pad report's bytes: x 0 left to 0xFF right, y 0 up to 0xFF down), when a `stick` runs.
    pub stick: Option<[u8; 2]>,
    /// The right stick this field (the same bytes), when a `look` runs.
    pub look: Option<[u8; 2]>,
    /// The pointer (shares of the picture), whether it moved this field, and whether the left button is clicked this field.
    pub pointer: Option<(f32, f32)>,
    pub moved: bool,
    pub click: bool,
    /// The mouse wheel turned this field (notches, up positive).
    pub wheel: i32,
    /// An Xbox controller's buttons held this field (`controller`: by the PS2 button each presses unbound), and its sticks (the
    /// bytes a controller reports before `input::shape`: right x, y, left x, y; 0x80 centred).
    pub controller: u16,
    pub controller_sticks: [u8; 4],
}

pub enum Step {
    Run(Field),
    Quit,
}

enum Until {
    Pause,
    Menu,
    Level(u32),
    Field(u64),
    /// The game's current mode (`Game::game_mode`).
    Mode(u32),
    Chunk(u32, u32),
}

enum Task {
    /// Fields left, the buttons held, the mouse motion of each field, the left stick held, the right stick held.
    Run(u64, u16, (f64, f64), Option<[u8; 2]>, Option<[u8; 2]>, u16, [u8; 4]),
    /// The condition, the fields run and the most to run.
    Until(Until, u64, u64),
}

pub struct Control {
    lines: std::sync::mpsc::Receiver<String>,
    task: Option<Task>,
    /// The answer of the task that ended with the last field.
    done: Option<String>,
    pointer: Option<(f32, f32)>,
    moved: bool,
    click: bool,
    wheel: i32,
    /// Fields in a row whose jobs carried no world (while an `until pause` runs).
    worldless: u32,
    /// Every story level's globals (`uyafmt::levelcode::all`): `until level`'s camera.
    levels: Vec<LevelGlobals>,
    /// `dump DIR`: the folder the next field's RAM and jobs go to.
    dump: Option<String>,
}

impl Control {
    /// The channel on this process's stdin (read on a thread of its own); `levels`: `uyafmt::levelcode::all`.
    pub fn stdin(levels: Vec<LevelGlobals>) -> Control {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("uya control".into())
            .spawn(move || {
                for line in std::io::stdin().lines() {
                    let Ok(line) = line else { break };
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            })
            .expect("the control thread starts");
        Control { lines: rx, task: None, done: None, pointer: None, moved: false, click: false, wheel: 0, worldless: 0, levels, dump: None }
    }

    /// Before field `s.field`: answers the commands that run no field and waits for one that does. `Quit` at `quit` or when stdin
    /// closes.
    pub fn field(&mut self, s: &mut State) -> Step {
        if let Some(a) = self.done.take() {
            println!("ctl: {a}");
        }
        loop {
            if let Some(task) = &mut self.task {
                let (buttons, mouse, stick, look, controller, controller_sticks) = match task {
                    Task::Run(left, b, m, st, lk, c, cs) => {
                        *left -= 1;
                        (*b, *m, *st, *lk, *c, *cs)
                    }
                    Task::Until(_, ran, _) => {
                        *ran += 1;
                        (0, (0.0, 0.0), None, None, 0, [0x80; 4])
                    }
                };
                let f = Field { buttons, mouse, stick, look, pointer: self.pointer, moved: std::mem::take(&mut self.moved), click: std::mem::take(&mut self.click), wheel: std::mem::take(&mut self.wheel), controller, controller_sticks };
                return Step::Run(f);
            }
            let Ok(line) = self.lines.recv() else { return Step::Quit };
            let w: Vec<&str> = line.split_whitespace().collect();
            let num = |k: usize| -> Result<u64, String> { w.get(k).ok_or_else(|| format!("{line}: a number is missing"))?.parse().map_err(|_| format!("{line}: not a number")) };
            let hex = |k: usize| -> Result<u32, String> { u32::from_str_radix(w.get(k).ok_or_else(|| format!("{line}: a hex number is missing"))?.trim_start_matches("0x"), 16).map_err(|_| format!("{line}: not hex")) };
            let r: Result<Option<String>, String> = (|| match w.first().copied() {
                None => Ok(None),
                Some("quit") => Err("quit".into()),
                Some("run") => {
                    let n = num(1)?;
                    self.task = (n > 0).then_some(Task::Run(n, 0, (0.0, 0.0), None, None, 0, [0x80; 4]));
                    Ok((n == 0).then(|| format!("ok field {}", s.field - 1)))
                }
                Some("press") => {
                    let (b, n) = (hex(1)?, num(2)?);
                    self.task = (n > 0).then_some(Task::Run(n, b as u16, (0.0, 0.0), None, None, 0, [0x80; 4]));
                    Ok((n == 0).then(|| format!("ok field {}", s.field - 1)))
                }
                Some("controller") => {
                    let (b, n) = (hex(1)?, num(2)?);
                    // The sticks as XInput reports them (-32768..32767), to the bytes `input::xinput` makes of them (y flipped:
                    // up 0).
                    let axis = |k: usize, flip: bool| -> Result<u8, String> {
                        let v: f32 = w.get(k).ok_or_else(|| format!("{line}: controller BUTTONS N [LX LY RX RY]"))?.parse().ok().filter(|v: &f32| (-1.0..=1.0).contains(v)).ok_or_else(|| format!("{line}: a stick from -1 to 1"))?;
                        let u = (((v * 32767.0).round() as i32 + 32768) >> 8) as u8;
                        Ok(if flip { 255 - u } else { u })
                    };
                    let sticks = if w.len() > 3 { [axis(5, false)?, axis(6, true)?, axis(3, false)?, axis(4, true)?] } else { [0x80; 4] };
                    self.task = (n > 0).then_some(Task::Run(n, 0, (0.0, 0.0), None, None, b as u16, sticks));
                    Ok((n == 0).then(|| format!("ok field {}", s.field - 1)))
                }
                Some("mouse") => {
                    let d = |k: usize| -> Result<f64, String> { w.get(k).and_then(|v| v.parse().ok()).ok_or_else(|| "mouse DX DY N".to_string()) };
                    let (dx, dy, n) = (d(1)?, d(2)?, num(3)?);
                    let b = if w.len() > 4 { hex(4)? as u16 } else { 0 };
                    self.task = (n > 0).then_some(Task::Run(n, b, (dx, dy), None, None, 0, [0x80; 4]));
                    Ok((n == 0).then(|| format!("ok field {}", s.field - 1)))
                }
                Some("stick") => {
                    let d = |k: usize| -> Result<f32, String> { w.get(k).and_then(|v| v.parse().ok()).filter(|v: &f32| (-1.0..=1.0).contains(v)).ok_or_else(|| "stick X Y N [BUTTONS]: X and Y from -1 to 1".to_string()) };
                    let (x, y, n) = (d(1)?, d(2)?, num(3)?);
                    let b = if w.len() > 4 { hex(4)? as u16 } else { 0 };
                    let byte = |v: f32| (128.0 + v * 127.0).round() as u8;
                    self.task = (n > 0).then_some(Task::Run(n, b, (0.0, 0.0), Some([byte(x), byte(-y)]), None, 0, [0x80; 4]));
                    Ok((n == 0).then(|| format!("ok field {}", s.field - 1)))
                }
                Some("look") => {
                    let d = |k: usize| -> Result<f32, String> { w.get(k).and_then(|v| v.parse().ok()).filter(|v: &f32| (-1.0..=1.0).contains(v)).ok_or_else(|| "look X Y N: X and Y from -1 to 1".to_string()) };
                    let (x, y, n) = (d(1)?, d(2)?, num(3)?);
                    let byte = |v: f32| (128.0 + v * 127.0).round() as u8;
                    self.task = (n > 0).then_some(Task::Run(n, 0, (0.0, 0.0), None, Some([byte(x), byte(-y)]), 0, [0x80; 4]));
                    Ok((n == 0).then(|| format!("ok field {}", s.field - 1)))
                }
                Some("pointer") => {
                    let (x, y): (f32, f32) = (w.get(1).and_then(|v| v.parse().ok()).ok_or("pointer X Y [click]")?, w.get(2).and_then(|v| v.parse().ok()).ok_or("pointer X Y [click]")?);
                    self.pointer = Some((x, y));
                    self.moved = true;
                    self.click = w.get(3) == Some(&"click");
                    Ok(Some(format!("ok pointer {x} {y}{}", if self.click { ", clicked on the next field" } else { "" })))
                }
                Some("wheel") => {
                    let n: i32 = w.get(1).and_then(|v| v.parse().ok()).ok_or("wheel N")?;
                    self.wheel = n;
                    Ok(Some(format!("ok wheel {n} on the next field")))
                }
                Some("warp") => s.cpu.request_level(num(1)? as u32).map(|d| Some(format!("ok {d}"))),
                Some("scene") => s.cpu.request_scene(num(1)? as u32).map(|d| Some(format!("ok {d}"))),
                Some("ask") => s.cpu.request_mode(num(1)? as u32, num(2)? as u32).map(|d| Some(format!("ok {d}"))),
                Some("chunk") => {
                    let chunk = num(1)? as u32;
                    let level = s.cpu.rd32(LEVEL_CURRENT);
                    let g = self.levels.iter().find(|g| g.level as u32 == level)
                        .ok_or_else(|| format!("chunk: level {level} has no world"))?;
                    s.cpu.request_chunk(g.load_chunk, chunk)?;
                    self.task = Some(Task::Until(Until::Chunk(g.level_chunk, chunk), 0, 2000));
                    Ok(None)
                }
                Some("until") => {
                    let cond = match (w.get(1).copied(), w.get(2)) {
                        (Some("pause"), _) => Until::Pause,
                        (Some("menu"), _) => Until::Menu,
                        (Some("level"), _) => Until::Level(num(2)? as u32),
                        (Some("field"), _) => Until::Field(num(2)?),
                        (Some("mode"), _) => Until::Mode(num(2)? as u32),
                        _ => return Err(format!("{line}: until pause|menu|level L|field F|mode M [max N]")),
                    };
                    let max = match w.iter().position(|&x| x == "max") {
                        Some(k) => num(k + 1)?,
                        None => 20000,
                    };
                    if !matches!(cond, Until::Pause) && holds(&cond, s, &self.levels) {
                        return Ok(Some(format!("ok field {}", s.field - 1)));
                    }
                    self.task = Some(Task::Until(cond, 0, max));
                    self.worldless = 0;
                    Ok(None)
                }
                Some("ram") => {
                    let (a, n) = (hex(1)?, num(2)?);
                    let bytes: Vec<u8> = (0..n as u32).map(|k| s.cpu.rd8(a + k)).collect();
                    Ok(Some(format!("ok {}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>())))
                }
                Some("write") => {
                    let a = hex(1)?;
                    let h = w.get(2).ok_or("write ADDR HEX")?;
                    if h.len() % 2 != 0 {
                        return Err(format!("{line}: an odd number of hex digits"));
                    }
                    for k in 0..h.len() / 2 {
                        let b = u8::from_str_radix(&h[2 * k..2 * k + 2], 16).map_err(|_| format!("{line}: not hex"))?;
                        s.cpu.wr8(a + k as u32, b);
                    }
                    Ok(Some(format!("ok {} bytes", h.len() / 2)))
                }
                Some("set") => {
                    let pairs = w.get(1).ok_or("set KEY=VALUE[,...]")?;
                    s.cpu.set(pairs)?;
                    Ok(Some(format!("ok {pairs} from field {}", s.field + 1)))
                }
                Some("shot") => {
                    let path = w.get(1).ok_or("shot FILE")?;
                    (s.shot)(path)?;
                    Ok(Some(format!("ok {path}")))
                }
                Some("dump") => {
                    let dir = w.get(1).ok_or("dump DIR")?;
                    std::fs::create_dir_all(dir).map_err(|e| format!("{dir}: {e}"))?;
                    self.dump = Some(dir.to_string());
                    Ok(Some(format!("ok field {}'s RAM and jobs go to {dir}", s.field)))
                }
                Some(c) => Err(format!("{c}: not a command (run, press, mouse, stick, pointer, wheel, warp, until, ram, write, set, shot, dump, quit)")),
            })();
            match r {
                Err(e) if e == "quit" => {
                    println!("ctl: ok quit");
                    return Step::Quit;
                }
                Err(e) => println!("ctl: error {e}"),
                Ok(Some(a)) => println!("ctl: {a}"),
                Ok(None) => {}
            }
        }
    }

    /// The folder a `dump` asked the field about to run to write its RAM and jobs to (taken).
    pub fn take_dump(&mut self) -> Option<String> {
        self.dump.take()
    }

    /// Whether the command running needs `State::world` (an `until pause`).
    pub fn wants_world(&self) -> bool {
        matches!(self.task, Some(Task::Until(Until::Pause, ..)))
    }

    /// After field `s.field` ran: whether it ends the running command (its frame then goes to the renderer before the answer).
    pub fn ran(&mut self, s: &mut State) -> bool {
        self.worldless = if s.world { 0 } else { self.worldless + 1 };
        let worldless = self.worldless;
        let end = match &self.task {
            None => return false,
            Some(Task::Run(left, ..)) => (*left == 0).then(|| format!("ok field {}", s.field)),
            Some(Task::Until(cond, ran, max)) => {
                if holds(cond, s, &self.levels) && (!matches!(cond, Until::Pause) || worldless >= 2) {
                    Some(format!("ok field {}", s.field))
                } else if ran >= max {
                    Some(format!("error the condition did not hold after {ran} fields (field {})", s.field))
                } else {
                    None
                }
            }
        };
        let Some(a) = end else { return false };
        self.task = None;
        self.done = Some(a);
        true
    }
}

/// Whether level `g`'s view camera is set, the native renderer's test (`native::camera`): the level is drawn, not loading.
fn camera_set(c: &mut dyn Game, g: &LevelGlobals) -> bool {
    let f = |c: &mut dyn Game, a: u32| f32::from_bits(c.rd32(a));
    let unit = |c: &mut dyn Game, a: u32| {
        let v = [f(c, a), f(c, a + 4), f(c, a + 8)];
        v.iter().all(|x| x.is_finite()) && ((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt() - 1.0).abs() < 0.01
    };
    unit(c, g.view_camera) && unit(c, g.view_camera + 0x20) && f(c, g.tangents) > 0.0 && f(c, g.tangents + 4) > 0.0
}

fn holds(c: &Until, s: &mut State, levels: &[LevelGlobals]) -> bool {
    match c {
        Until::Pause => s.menu && !s.world && !in_front_end(s.cpu),
        Until::Menu => s.menu,
        Until::Level(l) => s.cpu.rd32(LEVEL_CURRENT) == *l && levels.iter().find(|g| g.level as u32 == *l).is_some_and(|g| camera_set(s.cpu, g)),
        Until::Field(f) => s.field >= *f,
        Until::Mode(m) => s.cpu.game_mode() == Some(*m),
        Until::Chunk(at, n) => !s.cpu.loading_chunk() && s.cpu.rd32(*at) == *n,
    }
}

/// Whether the game is in the front end (no level program loaded).
pub fn in_front_end(c: &mut dyn Game) -> bool {
    matches!(c.rd32(LEVEL_CURRENT), 0 | u32::MAX)
}
