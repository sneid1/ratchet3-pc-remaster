//! The game's sound calls: what the EE side of the sound library sends the IOP module's two servers, and what the module does with
//! each (`Engine::call`).
//!
//! # The entry
//!
//! ```ignore
//! pub fn call(&mut self, host: &mut dyn Host, sid: u32, fno: u32, send: &[u8]) -> Result<Vec<u8>, String>
//! ```
//!
//! The runtime calls it for every `sceSifCallRpc` whose client is bound to server `0x123456` or `0x123457`: `sid` the server,
//! `fno` the function number, `send` the bytes at the call's send buffer (its length is the call's send size), the return value
//! the bytes to put at the call's receive buffer (the caller copies at most the receive size; the engine's reply is exactly as long
//! as the game's receive size for each call below). The engine reads and writes the rest of the machine through `Host`.
//!
//! # What the game sends and reads back
//!
//! Server `0x123456` (the commands), reply `[0xFFFFFFFF, result 0, result 1, ..., 0xFFFFFFFF]`:
//! - function 0, direct, send 8 bytes (EE address of the status area, flags), receive 12 bytes: the engine starts. The status area
//!   (the EE's 0x1D4980: word 0 the disc channel's busy flag, word 4 its status) is where the module reports the end of a disc
//!   read (command 0x38): both words are cleared when a read is done (989snd FUN_0000A744; the EE polls them, func_0013CDF0).
//! - function 0x4D, the batch the EE sends at the end of a field (func_0013C908): receive `count * 4 + 8` bytes. The send is a
//!   32-bit count, then per command a 16-bit number, a 16-bit length and the arguments padded to 4 bytes. Command j's result is
//!   word j + 1 of the reply; the EE hands it to the callback it queued with the command (func_0013B620).
//! - other functions, direct, are single commands with the same argument layout and a 12-byte reply (the result is word 1): 0x2A
//!   (starts the stream engine: send 16 bytes, the reader count first; result 1 when it started, 0 when it was already started),
//!   0x2C and the other stream commands (stream.rs), 0x34 (ends every stream), 0x36 (waits until the streams are idle: result 1),
//!   0x37 (the streams' state: result 1, idle), 0x18 (stops every sound and stream), 0x08 (settles the banks after a load).
//!
//! Server `0x123457` (banks), reply one word, the bank's handle (0: it did not load); the EE waits until the word is not
//! 0xFFFFFFFF and hands it to the callback it queued (func_0013BAB8, func_0013BC30, func_0013BD58):
//! - function 3, send 8 bytes (the bank's disc sector, a second word the engine does not need): the bank is read from the disc.
//! - function 0x57, send 4 bytes (the bank's address in EE memory): the bank is read from EE memory.
//! - function 0x59, send 4 bytes (the bank's address in IOP memory): the bank the EE has sent to IOP memory (a story level's
//!   gadget banks, func_L01_0042A300: three slots of 0xC800 bytes from the IOP address at EE 0x1D4B38).
//! The disc and EE-memory loads (989snd FUN_00008BAC) refuse a version 1 SBlk (error 0x71: handle 0); the IOP-memory load
//! (FUN_0000898C) takes it with the same reads as version 3 (three of WAD7's gadget banks are version 1).
//!
//! The handle is the engine's own (1, 2, ...); the game keeps it and passes it to the play command, with -1 meaning the first
//! bank loaded of those still loaded (the head of the module's list).
//!
//! What no program on the disc calls is refused with an error (`captures/snd/sites.py` and `wrappers.py` list every call the
//! boot program, the 50 story overlays and both front ends make into the library): the bank server's functions 2 and 0x6A,
//! and the commands outside the list below. The PCM stream's commands (0x3B to 0x41, 0x5A, 0x5B) are called only from the
//! movie player's copies (`pcmtree.py`: front end 0x3AFAA8 and 0x3AA2E8, each level's), which the runtime replaces with
//! `cinema::play_movie`: refused too.
//!
//! # Commands
//!
//! The commands are 989snd's own table (`0x1D044`, numbers 0..=0x6A); `command` has the ones the engine does: 0x00 start, 0x06
//! unload a bank, 0x08 settle, 0x09 a voice group's volume (group 0x10: the master volume), 0x0B the output mode, 0x0D and 0x4E the
//! voices of the groups, 0x11 and 0x12 play a sound (0x11 returns its handle), 0x13 and 0x14 pause and resume a sound, 0x15
//! let a sound go, 0x16 and 0x17 pause and resume the sounds of voice groups, 0x18 stop everything, 0x19 whether a sound plays,
//! 0x1B a sound's volume and pan, 0x21 several of its parameters, 0x38 a disc read into EE memory (the EE's own file reads while
//! the sound library is up: 989snd FUN_00013528), 0x2A..0x37, 0x4F and 0x60 the stream engine and the ducking of voice groups
//! (stream.rs), 0x10, 0x50, 0x51 and 0x64 the reverb (reverb.rs). Every other command is an error.

use super::{Engine, Play};
use uyafmt::sbank;

/// What the runtime gives the engine: the machine's memory the IOP module read and wrote itself, and the disc.
pub trait Host {
    /// `len` bytes of EE memory at `addr` (what the module's SIF transfers read).
    fn read_ee(&mut self, addr: u32, len: usize) -> Result<Vec<u8>, String>;
    /// Writes bytes into EE memory at `addr` (a disc read's data, the status area).
    fn write_ee(&mut self, addr: u32, bytes: &[u8]) -> Result<(), String>;
    /// `sectors` 2048-byte sectors of the disc from sector `lba`.
    fn read_disc(&mut self, lba: u32, sectors: u32) -> Result<Vec<u8>, String>;
    /// The disc's sectors: the CD driver refuses a read past them.
    fn disc_sectors(&mut self) -> u64;
    /// `len` bytes of IOP memory at `addr` (what the EE sent there with sceSifSetDma: a bank for function 0x59).
    fn read_iop(&mut self, addr: u32, len: usize) -> Result<Vec<u8>, String>;
}

fn words(b: &[u8]) -> Vec<u32> {
    b.chunks(4).map(|c| u32::from_le_bytes([c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0), c.get(2).copied().unwrap_or(0), c.get(3).copied().unwrap_or(0)])).collect()
}

fn reply(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// The bytes a bank takes from its wrapper's first 0x18 bytes: the sample data's offset and size.
fn bank_len(wrapper: &[u8]) -> Result<usize, String> {
    let w = words(wrapper);
    if w.len() < 6 || (w[0] != 1 && w[0] != 3) || w[1] != 2 || w[2] != 0x18 {
        return Err(format!("no bank wrapper: {:x?}", &w[..w.len().min(6)]));
    }
    Ok(w[4] as usize + w[5] as usize)
}

impl Engine {
    /// One call of the game's sound servers: see the module doc. A fault the sounds met since the last call fails this one.
    pub fn call(&mut self, host: &mut dyn Host, sid: u32, fno: u32, send: &[u8]) -> Result<Vec<u8>, String> {
        if let Some(f) = self.take_fault() {
            return Err(format!("sound engine: {f}"));
        }
        let w = words(send);
        match sid {
            0x123456 => {
                if fno == 0x4d {
                    let count = *w.first().ok_or("sound batch without a count")? as usize;
                    let mut out = vec![0xffff_ffffu32];
                    let mut at = 4usize;
                    for _ in 0..count {
                        let head = send.get(at..at + 4).ok_or("sound batch ends inside a command")?;
                        let (cid, len) = (u16::from_le_bytes([head[0], head[1]]) as u32, i16::from_le_bytes([head[2], head[3]]) as i32);
                        let n = (len.max(0) as usize + 3) & !3;
                        let args = words(send.get(at + 4..at + 4 + n).ok_or("sound command's arguments run past the batch")?);
                        let r = self.command(host, cid, &args)?;
                        if super::log_on() {
                            println!("snd: tick {}: command {cid:#x} {args:x?} -> {r:#x}", self.ticks);
                        }
                        out.push(r);
                        at += 4 + n;
                    }
                    out.push(0xffff_ffff);
                    Ok(reply(&out))
                } else {
                    let r = self.command(host, fno, &w)?;
                    if super::log_on() {
                        println!("snd: tick {}: command {fno:#x} {w:x?} -> {r:#x}", self.ticks);
                    }
                    Ok(reply(&[0xffff_ffff, r, 0xffff_ffff]))
                }
            }
            0x123457 => {
                let handle = match fno {
                    3 => {
                        let lba = *w.first().ok_or("bank load without a sector")?;
                        let len = bank_len(&host.read_disc(lba, 1)?)?;
                        let bytes = host.read_disc(lba, len.div_ceil(2048) as u32)?;
                        self.add_bank(&bytes[..len], false)?
                    }
                    0x57 => {
                        let addr = *w.first().ok_or("bank load without an address")?;
                        let len = bank_len(&host.read_ee(addr, 0x18)?)?;
                        let bytes = host.read_ee(addr, len)?;
                        self.add_bank(&bytes, false)?
                    }
                    0x59 => {
                        let addr = *w.first().ok_or("bank load without an IOP address")?;
                        let len = bank_len(&host.read_iop(addr, 0x18)?)?;
                        let bytes = host.read_iop(addr, len)?;
                        self.add_bank(&bytes, true)?
                    }
                    f => return Err(format!("sound bank server function {f:#x}: no program on the disc calls it")),
                };
                Ok(reply(&[handle]))
            }
            s => Err(format!("sound call to server {s:#x}")),
        }
    }

    /// Decodes and adds a bank; the handle is what the game passes back to play its sounds. A version 1 bank loads only from IOP
    /// memory (`iop`); the other loads answer 0 for it, as the module's do.
    fn add_bank(&mut self, bytes: &[u8], iop: bool) -> Result<u32, String> {
        let bank = sbank::decode(bytes)?;
        if bank.version == 1 && !iop {
            return Ok(0);
        }
        let (id, n) = (bank.id, bank.sounds.len());
        let h = self.load_bank(bank);
        if super::log_on() {
            println!("snd: tick {}: bank {h} loaded (SBlk id {id:#x}, {n} sounds, {} bytes)", self.ticks, bytes.len());
        }
        Ok(h)
    }

    /// The end of a disc read: the status area's two words are cleared (989snd FUN_0000A744 writes the status first, then the busy
    /// word).
    fn disc_read_done(&mut self, host: &mut dyn Host) -> Result<(), String> {
        if self.status_addr != 0 {
            host.write_ee(self.status_addr + 0x10, &0u32.to_le_bytes())?;
            host.write_ee(self.status_addr, &0u32.to_le_bytes())?;
        }
        Ok(())
    }

    /// One command (989snd's handler for `cid`); its result, 0 for one that has none.
    fn command(&mut self, host: &mut dyn Host, cid: u32, a: &[u32]) -> Result<u32, String> {
        let w = |i: usize| a.get(i).copied().unwrap_or(0);
        let lo = |i: usize| w(i) as u16 as i16 as i32;
        Ok(match cid {
            // 0 starts the engine: the status area's address and flags.
            0x00 => {
                self.status_addr = w(0);
                0
            }
            // 8 settles the banks' references after a load (nothing to settle: the engine resolves them when it plays).
            0x08 => 0,
            // 6 unloads a bank.
            0x06 => {
                self.unload_bank(w(0));
                0
            }
            // 9 sets the volume of a voice group (0..0x400); group 0x10 is the master volume.
            0x09 => {
                self.set_group_volume(w(0) as usize, w(1) as i32)?;
                0
            }
            0x0b => {
                self.out_mode = w(0) as i32;
                0
            }
            // 0xD: whether the sounds use the second core's voices only (1: group 1 the first core's; 989snd 0x18BE4), and 0x4E sets a
            // group's voices.
            0x0d => {
                for g in 0..16 {
                    self.group_voices[g] = if w(0) == 1 { (24, 47) } else { (0, 47) };
                }
                if w(0) == 1 {
                    self.group_voices[1] = (0, 23);
                }
                0
            }
            0x4e => {
                self.group_voices[(w(0) as usize) & 15] = ((w(1) as i32).max(0) as usize, (w(2) as usize).min(super::CHIP_VOICES - 1));
                0
            }
            // 0x11 plays a sound and returns its handle, 0x12 does not return it: the bank, the sound, the master volume (10 bits),
            // a pan angle that replaces the sound's (-1, -2: none), a transposition and a pitch bend.
            0x11 | 0x12 => {
                let bank = if w(0) == 0xffff_ffff { self.first_bank() } else { w(0) };
                let p = Play { volume: None, pan: None, volmult: w(2) as i32, pan_override: Some(w(3) as i32), transpose: lo(4), bend: lo(5) };
                self.play(bank, w(1) as usize, p)?
            }
            0x13 => {
                self.pause(w(0), true);
                0
            }
            0x14 => {
                self.pause(w(0), false);
                0
            }
            // 0x15 lets a sound go (its release runs), 0x16 and 0x17 pause and resume every sound of the voice groups in a mask, 0x18
            // stops everything at once.
            0x15 => {
                self.stop(w(0), false);
                0
            }
            0x16 => {
                self.pause_groups(w(0), true);
                0
            }
            0x17 => {
                self.pause_groups(w(0), false);
                0
            }
            0x18 => {
                self.stop_all();
                0
            }
            // 0x19 answers the handle if the sound still plays, else 0.
            0x19 => {
                if w(0) == 0xffff_ffff || self.playing(w(0)) {
                    w(0)
                } else {
                    0
                }
            }
            // 0x1B sets a sound's master volume and pan.
            0x1b => {
                self.held(w(0));
                self.set_volume_pan(w(0), w(1) as i32, w(2) as i32);
                0
            }
            // 0x21 sets some of a sound's parameters named by a mask: bit 0 the master volume (word 2), bit 1 the pan (word 3), bit 2 a
            // slide of the pan to the angle in word 3 (with bit 1 too, the slide only), bit 3 the transposition (low half of word 4), bit 4 the bend (low half of word 5). It answers the
            // handle, 0 if the sound is gone.
            0x21 => {
                let (h, mask) = (w(0), w(1));
                if !self.playing(h) {
                    return Ok(0);
                }
                self.held(h);
                let mask = if mask & 6 == 6 { mask & !2 } else { mask };
                if mask & 3 != 0 {
                    let vol = if mask & 1 != 0 { w(2) as i32 } else { 0x7fff_ffff };
                    let pan = if mask & 2 != 0 { w(3) as i32 } else { -2 };
                    self.set_volume_pan(h, vol, pan);
                }
                if mask & 4 != 0 {
                    self.pan_slide(h, w(3) as i32, 0, 0x1e);
                }
                if mask & 8 != 0 {
                    self.set_transpose(h, lo(4));
                }
                if mask & 0x10 != 0 {
                    self.set_bend_of(h, lo(5));
                }
                h
            }
            // 0x38 reads sectors of the disc into EE memory: the sector, the count and the address; the status area is cleared when
            // the data is there. (The game reads its files through the sound library's disc channel once it is up.)
            0x38 => {
                let (lba, n, dest) = (w(0), w(1), w(2));
                if n != 0 {
                    let bytes = host.read_disc(lba, n)?;
                    host.write_ee(dest, &bytes)?;
                }
                self.disc_read_done(host)?;
                0
            }
            // The stream engine (stream.rs): 0x2A starts it (its reader count first; 1 if it started, 0 if it was already), 0x2B
            // is a call the module only reports as an error, 0x2C starts a stream or queues a part (the handle, 1 for a part, 0 for
            // nothing), 0x2D holds it, 0x2E lets it go, 0x2F ends it, 0x30 its parts, 0x31 0x32 0x33 the time played, left and
            // total, 0x34 ends every stream, 0x35 shuts the engine down, 0x36 waits for the streams to be idle and 0x37 asks (they
            // are: the port reads a part whole), 0x4F whether it has started, 0x60 a ducking record.
            0x2a => self.stream_engine_start(w(0)),
            0x2b => 0,
            0x2c => self.stream_start(host, a)?,
            0x2d => {
                self.stream_pause(w(0), true);
                0
            }
            0x2e => {
                self.stream_pause(w(0), false);
                0
            }
            0x2f => {
                self.stream_stop(w(0));
                0
            }
            0x30 => self.stream_parts(w(0)),
            0x31 => self.stream_elapsed(w(0)),
            0x32 => self.stream_remaining(w(0)),
            0x33 => self.stream_total(w(0)),
            0x34 => {
                self.stream_end_all();
                0
            }
            0x35 => {
                self.stream_engine_stop();
                0
            }
            0x36 | 0x37 => 1,
            0x4f => self.stream_started(w(0)),
            0x60 => {
                self.stream_duck(w(0), a.get(1..).unwrap_or(&[]));
                0
            }
            // The reverb (reverb.rs): 0x10 moves its depth, 0x50 sets its mode, depth, delay and feedback, 0x51 reserves a mode's work
            // area, 0x64 sends the first core's output into the second core's effect. None of them answers.
            0x10 => {
                self.reverb.fade(w(0), w(1) as i32, w(2) as i32, w(3))?;
                0
            }
            0x50 => {
                self.reverb.set(w(0), w(1), w(2) as i32, w(3), w(4))?;
                println!("sound: tick {}: reverb mode {} on cores {:#x}, depth {:#x}, delay {}, feedback {}", self.ticks, w(1), w(0), w(2) as i16, w(3), w(4));
                0
            }
            0x51 => {
                self.reverb.reserve(w(0), w(1));
                0
            }
            0x64 => {
                self.reverb.enable(w(0) != 0);
                0
            }
            0x3b..=0x41 | 0x5a | 0x5b => return Err(format!("sound command {cid:#x}: the PCM stream, which only the movie player sends (the runtime plays movies with cinema::play_movie)")),
            c => return Err(format!("sound command {c:#x}: no program on the disc sends it")),
        })
    }
}
