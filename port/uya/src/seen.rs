//! The cutscenes the player has seen, remembered in the profile (`seen.txt`), for CUTSCENE SKIP (PLAN 10, `settings.rs`): with
//! AFTER FIRST VIEWING a cutscene plays through the first time and can be skipped as the game lets it be (any button, or Start
//! while the game asks for it) every time after; ANY TIME is the game's own (any press skips at once).
//!
//! A cutscene is the game's in-engine one, its level and its SCENE record (the level's scene state + 0x30,
//! `uyafmt::scenecode::SceneGlobals::scene_index`), written `scene LEVEL RECORD`; or a movie, its first disc sector (the movie
//! player's a0), written `movie SECTOR`. One a line; a cutscene is added when it ends, played through or skipped.

use std::collections::BTreeSet;
use std::sync::Mutex;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Cutscene {
    Scene { level: u32, record: u32 },
    Movie { sector: u32 },
}

struct Seen {
    path: String,
    set: BTreeSet<Cutscene>,
    /// Whether a cutscene not yet seen plays through (the setting at AFTER FIRST VIEWING).
    first_through: bool,
}

static SEEN: Mutex<Option<Seen>> = Mutex::new(None);

/// Reads the profile's record (none yet: nothing seen); a line it cannot read stops the game, naming it.
pub fn open(profile: &str) -> Result<(), String> {
    let path = format!("{profile}/seen.txt");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{path}: {e}")),
    };
    let mut set = BTreeSet::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let w: Vec<&str> = line.split_whitespace().collect();
        let num = |s: &str| s.parse::<u32>().map_err(|_| format!("{path}: \"{line}\" is not a cutscene"));
        let c = match w[..] {
            ["scene", l, r] => Cutscene::Scene { level: num(l)?, record: num(r)? },
            ["movie", s] => Cutscene::Movie { sector: num(s)? },
            _ => return Err(format!("{path}: \"{line}\" is not a cutscene")),
        };
        set.insert(c);
    }
    *SEEN.lock().expect("seen lock") = Some(Seen { path, set, first_through: false });
    Ok(())
}

/// The setting: whether a cutscene not yet seen plays through.
pub fn set_first_through(on: bool) {
    if let Some(s) = SEEN.lock().expect("seen lock").as_mut() {
        s.first_through = on;
    }
}

/// Whether the player may skip `c` now: always with ANY TIME; with AFTER FIRST VIEWING once it has been seen. Outside `uya play`
/// (no profile opened: the emulated reference's movies) the game's own rule, any time.
pub fn may_skip(c: Cutscene) -> bool {
    match SEEN.lock().expect("seen lock").as_ref() {
        Some(s) => !s.first_through || s.set.contains(&c),
        None => true,
    }
}

/// `c` has been seen (it ended): remembered in the profile.
pub fn saw(c: Cutscene) -> Result<(), String> {
    let mut g = SEEN.lock().expect("seen lock");
    let Some(s) = g.as_mut() else { return Ok(()) };
    if !s.set.insert(c) {
        return Ok(());
    }
    let text: String = s
        .set
        .iter()
        .map(|c| match c {
            Cutscene::Scene { level, record } => format!("scene {level} {record}\n"),
            Cutscene::Movie { sector } => format!("movie {sector}\n"),
        })
        .collect();
    std::fs::write(&s.path, text).map_err(|e| format!("{}: {e}", s.path))
}
