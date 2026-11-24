//! The game's VU1 microprograms, translated ahead of time (`vu0mp::translate` with `vu1`) into the crate `tr-vu1` (OUT/vu1),
//! which the renderer runs on VU1 state of its own (`port/uya/src/effects.rs`: program 6, the effects).
//!
//! The ten programs the VIF1 chain uploads, each a VIF packet in the boot program's data (FLUSH, then MPG codes of 256 pairs
//! or fewer, NOPs between them; the chain sends them from these addresses): the eight of `mobies::PROGRAMS` (programs 0 to 7,
//! their pieces 0x808 bytes apart), the water's (0x10F500) and the sea's (0x116070). Each is the image its MPG codes put in
//! micro memory from address 0; a run's program is the one whose code micro memory holds.

use crate::programs::Program;
use std::collections::BTreeSet;
use std::fmt::Write as _;

/// (name, EE address of the packet's first VIF code, pairs it uploads).
const PROGS: &[(&str, u32, usize)] = &[
    ("p0", 0x11_0790, 1335),
    ("p1", 0x10_3680, 1868),
    ("p2", 0x10_8f30, 1145),
    ("p3", 0x10_b330, 1990),
    ("p4", 0x10_1860, 365),
    ("p5", 0x10_23f0, 589),
    ("p6", 0x10_1150, 222),
    ("p7", 0x10_7130, 954),
    ("water", 0x10_f500, 588),
    ("sea", 0x11_6070, 470),
];

/// The microcode a packet uploads: its VIF codes walked from `at` (NOP, FLUSHE, FLUSH, FLUSHA, MPG) until `pairs` pairs are
/// in micro memory, each MPG at its own micro address.
fn microcode(boot: &Program, name: &str, mut at: u32, pairs: usize) -> Result<Vec<u8>, String> {
    let word = |a: u32| boot.word(a).ok_or_else(|| format!("VU1 {name}: {a:08x} is not in the boot program"));
    let mut img = vec![0u8; 8 * pairs];
    let mut got = 0;
    while got < pairs {
        let c = word(at)?;
        let (cmd, num, imm) = ((c >> 24) & 0x7f, (c >> 16) & 0xff, (c & 0xffff) as usize);
        at += 4;
        match cmd {
            0x00 | 0x10 | 0x11 | 0x13 => {}
            0x4a => {
                let n = if num == 0 { 256 } else { num as usize };
                if imm != got || got + n > pairs {
                    return Err(format!("VU1 {name}: MPG of {n} pairs to {imm:#x} at {:08x}, after {got} pairs of {pairs}", at - 4));
                }
                for k in 0..2 * n {
                    img[8 * imm + 4 * k..8 * imm + 4 * k + 4].copy_from_slice(&word(at)?.to_le_bytes());
                    at += 4;
                }
                got += n;
            }
            _ => return Err(format!("VU1 {name}: VIF code {c:08x} at {:08x} before its {pairs} pairs", at - 4)),
        }
    }
    Ok(img)
}

/// Writes `OUT/vu1` (`tr-vu1`): the VU1 state (`vu1rt.rs`) and every program translated.
pub fn write(boot: &Program, out_dir: &str, cpu_path: &str, put: impl Fn(String, String) -> Result<(), String>) -> Result<(), String> {
    let mut lib = String::from(
        "//! Written by port/uyatr: the game's VU1 microprograms (vu1mp.rs), on the VU1 state of `rt`. Game code, never committed.\n#![allow(clippy::all, unused_parens)]\npub mod rt;\n",
    );
    let mut table = String::from("\n/// Every program, by its microcode.\npub static PROGRAMS: &[rt::Prog] = &[\n");
    for &(name, at, pairs) in PROGS {
        let code = microcode(boot, name, at, pairs)?;
        lib.push_str(&crate::vu0mp::translate(name, 0, &code, &BTreeSet::new(), true).map_err(|e| format!("VU1 {name}: {e}"))?);
        writeln!(table, "    rt::Prog {{ name: \"{name}\", code: &{name}::CODE, entry: {name}::entry, run: {name}::run }},").unwrap();
        println!("VU1 {name}: micro 0x0..{:#x}", code.len());
    }
    table.push_str("];\n");
    lib.push_str(&table);
    let dir = format!("{out_dir}/vu1");
    std::fs::create_dir_all(format!("{dir}/src")).map_err(|e| format!("{dir}: {e}"))?;
    put(
        format!("{dir}/Cargo.toml"),
        format!("# Written by port/uyatr. Game code: never committed.\n[package]\nname = \"tr-vu1\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nuycpu = {{ path = \"{cpu_path}\" }}\n"),
    )?;
    put(format!("{dir}/src/rt.rs"), include_str!("vu1rt.rs").to_string())?;
    put(format!("{dir}/src/lib.rs"), lib)
}
