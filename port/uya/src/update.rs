//! The update notice (the player, 27 November 2026: "Set up the update notice for the next release", for the players who installed
//! the release zip by hand; RecompLauncher keeps its players' game up to date itself). As the game starts (`start`, from
//! `App::start` with the profile's settings) a thread of its own asks GitHub for the remaster's latest release, at most once in
//! `RECHECK` a profile (the answer and its time kept in the profile's update.txt), unless RecompLauncher started the game or the
//! GAMEPLAY tab's UPDATE CHECK is off; what it found goes to the log. A newer release than this build's puts a plate on the main
//! menu (`plate`, each field from `rtgame.rs`, drawn by the window: `play.rs` `NoticePlate`) in the style of the settings page's
//! tabs: UPDATE AVAILABLE, the release and this one, and how to get it; a click on it or Select (named on the plate: the key the
//! player's bindings give it with the keyboard, SELECT with a controller; no F key, which some keyboards lack: the player, 27
//! November 2026) opens the release's page in the player's browser (`open`). No address is shown (the player: "I dont want to show a link address in a message that is
//! amateur stuff"); the rest of the design was Fable's, but for its place: at the original picture's top right, clear of the main
//! menu's panel, not at the settings tabs' height, which meets the panel's top.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// This build's release: port/uya/Cargo.toml's version (`scripts/package-release.ps1` refuses a zip whose uya.exe says another).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The repository whose releases are the remaster's.
const REPO: &str = "sneid1/ratchet3-pc-remaster";

/// How long a profile's last answer stands before GitHub is asked again (its API answers 60 asks an hour from one address).
const RECHECK_SECS: u64 = 6 * 3600;

/// The plate's place on the main menu, in shares of the original picture (its 2D layer, where the menus' pointer is: x 0 to 1 over
/// the original picture's width in the middle of the window, y 0 to 1 down its height): at the top right, right of the menu's
/// panel (x 0.25 to 0.75, y from 0.21) and below the FPS counter's corner.
pub const PLATE: [f32; 4] = [0.74, 0.045, 0.975, 0.126];

/// The newer release the check found (its tag, "v0.1.11").
static NEWER: OnceLock<String> = OnceLock::new();

/// Whether its page has been opened in this run.
static OPENED: AtomicBool = AtomicBool::new(false);

/// The plate as the main menu shows it in a field: lit under the pointer, and its three lines.
#[derive(Clone)]
pub struct Shown {
    pub lit: bool,
    pub lines: [String; 3],
}

/// The release numbers of a tag "vA.B.C"; None for any other.
fn numbers(tag: &str) -> Option<[u32; 3]> {
    let mut it = tag.strip_prefix('v')?.split('.');
    let n = [it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?];
    it.next().is_none().then_some(n)
}

/// Starts the check: `on` is the profile's UPDATE CHECK.
pub fn start(profile: &str, on: bool) {
    if std::env::var_os("RECOMPLAUNCHER").is_some() {
        println!("update: not checked: RecompLauncher keeps the game up to date");
        return;
    }
    if !on {
        println!("update: not checked: UPDATE CHECK is off");
        return;
    }
    let file = format!("{profile}/update.txt");
    std::thread::Builder::new()
        .name("update check".into())
        .spawn(move || match latest(&file) {
            Ok((tag, when)) => compare(&tag, &when),
            Err(e) => println!("update: no answer from GitHub: {e}"),
        })
        .expect("the update check's thread");
}

/// The latest release `tag` against this build's.
fn compare(tag: &str, when: &str) {
    let ours = numbers(&format!("v{VERSION}")).expect("port/uya/Cargo.toml's version is A.B.C");
    match numbers(tag) {
        Some(n) if n > ours => {
            println!("update: {tag} is out, this is v{VERSION} ({when}): the main menu says so");
            NEWER.set(tag.to_string()).expect("the check runs once");
        }
        Some(_) => println!("update: v{VERSION} is up to date: the latest release is {tag} ({when})"),
        None => println!("update: the latest release is named {tag:?}, not vA.B.C ({when}): not compared"),
    }
}

/// The latest release's tag and when it was asked: the profile's record while it is under `RECHECK_SECS` old, else GitHub's
/// answer, recorded.
fn latest(file: &str) -> Result<(String, String), String> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| format!("the clock: {e}"))?.as_secs();
    match std::fs::read_to_string(file) {
        Ok(text) => match record(&text) {
            Some((at, tag)) if now.saturating_sub(at) < RECHECK_SECS => return Ok((tag, format!("asked {} min ago", now.saturating_sub(at) / 60))),
            Some(_) => {}
            None => println!("update: {file} holds no check's record: GitHub is asked again"),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{file}: {e}")),
    }
    let tag = ask()?;
    if let Err(e) = std::fs::write(file, format!("checked {now}\nlatest {tag}\n")) {
        println!("update: {file}: {e}: the answer is not kept, and the next start asks again");
    }
    Ok((tag, "asked now".to_string()))
}

/// update.txt's record: the time it was asked (seconds since 1970) and the tag.
fn record(text: &str) -> Option<(u64, String)> {
    let mut lines = text.lines();
    let at = lines.next()?.strip_prefix("checked ")?.parse().ok()?;
    let tag = lines.next()?.strip_prefix("latest ")?.to_string();
    Some((at, tag))
}

/// GitHub's latest release's tag (its REST API's releases/latest, over WinHTTP: the system's TLS and proxy).
#[cfg(windows)]
fn ask() -> Result<String, String> {
    use windows::Win32::Networking::WinHttp::*;
    use windows::core::{HSTRING, PCWSTR, w};
    /// A WinHTTP handle, closed when dropped.
    struct Handle(*mut std::ffi::c_void);
    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: a handle WinHTTP gave, closed once.
            let _ = unsafe { WinHttpCloseHandle(self.0) };
        }
    }
    let made = |h: *mut std::ffi::c_void, what: &str| if h.is_null() { Err(format!("{what}: {}", windows::core::Error::from_thread())) } else { Ok(Handle(h)) };
    let agent = HSTRING::from(format!("ratchet3-pc-remaster/{VERSION}"));
    let path = HSTRING::from(format!("/repos/{REPO}/releases/latest"));
    let headers: Vec<u16> = "Accept: application/vnd.github+json\r\n".encode_utf16().collect();
    // SAFETY: WinHTTP's calls on handles made here and dropped in reverse order; the buffers outlive the calls that fill them.
    unsafe {
        let session = made(WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0), "WinHttpOpen")?;
        WinHttpSetTimeouts(session.0, 5000, 5000, 5000, 10000).map_err(|e| format!("WinHttpSetTimeouts: {e}"))?;
        let connect = made(WinHttpConnect(session.0, w!("api.github.com"), INTERNET_DEFAULT_HTTPS_PORT, 0), "WinHttpConnect")?;
        let request = made(WinHttpOpenRequest(connect.0, w!("GET"), &path, PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE), "WinHttpOpenRequest")?;
        WinHttpSendRequest(request.0, Some(&headers), None, 0, 0, 0).map_err(|e| format!("WinHttpSendRequest: {e}"))?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|e| format!("WinHttpReceiveResponse: {e}"))?;
        let (mut status, mut len) = (0u32, 4u32);
        WinHttpQueryHeaders(request.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, PCWSTR::null(), Some(&mut status as *mut u32 as *mut std::ffi::c_void), &mut len, std::ptr::null_mut())
            .map_err(|e| format!("the status: {e}"))?;
        let mut body = Vec::new();
        loop {
            let mut ready = 0u32;
            WinHttpQueryDataAvailable(request.0, &mut ready).map_err(|e| format!("WinHttpQueryDataAvailable: {e}"))?;
            if ready == 0 {
                break;
            }
            let at = body.len();
            body.resize(at + ready as usize, 0u8);
            let mut read = 0u32;
            WinHttpReadData(request.0, body[at..].as_mut_ptr() as *mut std::ffi::c_void, ready, &mut read).map_err(|e| format!("WinHttpReadData: {e}"))?;
            body.truncate(at + read as usize);
            if body.len() > 1 << 20 {
                return Err("GitHub's answer is over 1 MB".into());
            }
        }
        if status != 200 {
            return Err(format!("GitHub answered {status}: {}", String::from_utf8_lossy(&body[..body.len().min(200)])));
        }
        tag_name(&String::from_utf8_lossy(&body))
    }
}

#[cfg(not(windows))]
fn ask() -> Result<String, String> {
    Err("the check is made on Windows only".into())
}

/// The release JSON's "tag_name" string (a tag has no escapes).
fn tag_name(json: &str) -> Result<String, String> {
    let at = json.find("\"tag_name\"").ok_or("no tag_name in GitHub's answer")?;
    let rest = json[at + "\"tag_name\"".len()..].trim_start().strip_prefix(':').ok_or("tag_name without its value")?.trim_start();
    let rest = rest.strip_prefix('"').ok_or("tag_name is not a string")?;
    Ok(rest[..rest.find('"').ok_or("tag_name's string does not end")?].to_string())
}

/// The newer release's tag, once the check found one.
pub fn newer() -> Option<&'static str> {
    NEWER.get().map(String::as_str)
}

/// Whether the pointer at `at` (shares of the original picture) is on the plate.
pub fn on_plate(at: (f32, f32)) -> bool {
    (PLATE[0]..=PLATE[2]).contains(&at.0) && (PLATE[1]..=PLATE[3]).contains(&at.1)
}

/// The plate in a field: shown on the main menu (`main_menu`) while a newer release is known, lit under the pointer (`at`); `press`
/// says how to open the page (`rtgame.rs`: the keyboard's key for Select, or SELECT), until it has been opened.
pub fn plate(main_menu: bool, at: Option<(f32, f32)>, press: String) -> Option<Shown> {
    let tag = newer().filter(|_| main_menu)?;
    let how = if OPENED.load(Ordering::Relaxed) { "OPENED IN YOUR BROWSER".to_string() } else { press };
    Some(Shown { lit: at.is_some_and(on_plate), lines: ["UPDATE AVAILABLE".to_string(), format!("{tag} IS OUT - YOU HAVE v{VERSION}"), how] })
}

/// The newer release's page opened in the player's browser (a thread of its own: the shell may take a moment to answer).
pub fn open() {
    let Some(tag) = newer() else { return };
    let url = format!("https://github.com/{REPO}/releases/tag/{tag}");
    std::thread::Builder::new()
        .name("update page".into())
        .spawn(move || {
            #[cfg(windows)]
            {
                use windows::Win32::UI::Shell::ShellExecuteW;
                use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
                use windows::core::{HSTRING, PCWSTR, w};
                // SAFETY: the shell's open of a URL with strings that outlive the call; it answers above 32 when it started it.
                let r = unsafe { ShellExecuteW(None, w!("open"), &HSTRING::from(url.as_str()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
                if r.0 as usize > 32 {
                    OPENED.store(true, Ordering::Relaxed);
                    println!("update: {url} opened in the browser");
                } else {
                    println!("update: the shell could not open {url} (its answer {})", r.0 as usize);
                }
            }
        })
        .expect("the update page's thread");
}
