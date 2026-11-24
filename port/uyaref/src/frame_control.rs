//! Step the emulated reference between fields. This lets a capture wait for
//! the actual game state and take both pictures before a short scene ends.
use std::io::{BufRead, Write};

pub struct Control {
    next: u64,
    buttons: u16,
}

impl Control {
    pub fn new(first: u64) -> Self { Self { next: first, buttons: 0 } }

    pub fn step(&mut self, field: u64, ram: &mut [u8], mut shot: impl FnMut(&str) -> Result<(), String>) -> Result<Option<u16>, String> {
        if field < self.next { return Ok(Some(self.buttons)); }
        println!("ref: field {}", field - 1);
        let stdin = std::io::stdin();
        let mut input = stdin.lock();
        loop {
            std::io::stdout().flush().map_err(|e| e.to_string())?;
            let mut line = String::new();
            if input.read_line(&mut line).map_err(|e| e.to_string())? == 0 { return Ok(None); }
            let cmd = line.trim();
            if let Some(path) = cmd.strip_prefix("shot ") {
                shot(path)?;
                println!("ref: ok");
                continue;
            }
            let words: Vec<_> = cmd.split_whitespace().collect();
            let hex = |v: &str| usize::from_str_radix(v, 16).map_err(|_| format!("invalid hex number {v}"));
            match words.as_slice() {
                ["quit"] => return Ok(None),
                ["run", count] | ["press", _, count] => {
                    self.buttons = if words.len() == 3 { u16::try_from(hex(words[1])?).map_err(|e| e.to_string())? } else { 0 };
                    let count: u64 = count.parse().map_err(|_| format!("invalid field count {count}"))?;
                    if count == 0 { return Err("field count must be positive".into()); }
                    self.next = field.checked_add(count).ok_or("field count overflow")?;
                    return Ok(Some(self.buttons));
                }
                ["ram", at, count] => {
                    let at = hex(at)?;
                    let count: usize = count.parse().map_err(|_| format!("invalid byte count {count}"))?;
                    let bytes = ram.get(at..at.checked_add(count).ok_or("RAM range overflow")?).ok_or("read past EE RAM")?;
                    let value: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                    println!("ref: {value}");
                }
                ["write", at, data] => {
                    let at = hex(at)?;
                    if data.len() % 2 != 0 || !data.bytes().all(|v| v.is_ascii_hexdigit()) { return Err("invalid hex bytes".into()); }
                    let bytes = ram.get_mut(at..at.checked_add(data.len() / 2).ok_or("RAM range overflow")?).ok_or("write past EE RAM")?;
                    for (i, byte) in bytes.iter_mut().enumerate() { *byte = hex(&data[2*i..2*i+2])? as u8; }
                    println!("ref: ok");
                }
                _ => return Err(format!("unknown reference command {cmd}")),
            }
        }
    }
}
