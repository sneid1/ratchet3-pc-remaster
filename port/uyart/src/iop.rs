//! The IOP's side, native: the port does not run the IOP's processor or its modules; what the game's EE code asks of them is
//! answered here, at the SIF, the boundary the EE's own libraries talk through.
//!
//! - **Transfers into IOP memory** (SifSetDma): kept in a 2 MB IOP RAM (sound banks, the IOP cache's blobs, RPC arguments).
//! - **SIF commands**: a transfer to the IOP's command buffer (`CMDBUF`, what SIF_REG_SUBADDR reports) is a command packet
//!   (header: +0 its size, bytes 1..3 the size of the data sent with it, +4 where that data went, +8 the command id, +0xC an
//!   option; the EE's sceSifSendCmd, 0x121c38). This side answers the ones the EE's libsif sends: its receive address
//!   (0x80000000, or 0x80000002 with option 0 on first contact), the RPC init handshake (0x80000002 with option 1: answered by
//!   setting software register 0, which sceSifInitRpc waits on at 0x122140), an IOP reset (0x80000003), and the RPC requests: bind (0x80000009), call (0x8000000A), read data
//!   (0x8000000C). An answer goes back as the IOP's would: a packet written into the EE's receive buffer and the SIF0 DMA's
//!   end (channel 5), whose handler (libsif's 0x121df0, translated) dispatches it: the RPC end packet (0x80000008: +0x14 the
//!   request packet, +0x18 its rpc id, +0x1C the client, +0x20 the request's command id, +0x24 the server, +0x28 its buffer)
//!   signals the caller's semaphore, runs its end function and frees the client (0x1222e8).
//! - **The servers**, by server id, each from the game's own library code that calls it (`serve`): libcdvd's (reads from the
//!   disc image), the IOP heap's, loadfile's, file I/O's, mtapman's and padman's (and padman's frame of every open pad each
//!   field, `pad_frames`), the memory card's (`mc`, `memcard.bin` in the player's profile), the IOP cache's, and the sound
//!   module's (`Sound`, the port's own sound engine). A call this side does not answer stops the game with the call.

use crate::{Cpu, Ext};
use std::collections::VecDeque;
use uyafmt::disc::{Disc, SECTOR};

pub const IOP_RAM: usize = 2 << 20;
/// The IOP's SIF command buffer (SIF_REG_SUBADDR).
pub const CMDBUF: u32 = 0x0001_f000;
/// The IOP cache server's region (`IOP_CACHE` fno 2): the PS2's own answer, base 0x40000 and 0xc0000 bytes. The game parks
/// blobs there with SifSetDma and fetches them back: level 28's map art (func_L28_003F5680, 0x41000 bytes after a 0xd680-byte
/// blob) needs that size; with less the park fails and the pause menu's map is never sent again once other textures overwrite it.
const CACHE_BASE: u32 = 0x0004_0000;
const CACHE_SIZE: u32 = 0x000c_0000;
/// The servers' receive buffers (the RPC arguments arrive there): `SERVER_BUF` bytes each from here, up to the heap.
const SERVER_BUFS: u32 = CACHE_BASE + CACHE_SIZE;
const SERVER_BUF: u32 = 0x8000;
/// sceSifAllocSysMemory's blocks, from here to the end of IOP RAM.
const HEAP_BASE: u32 = 0x0018_0000;

const SND_SID: u32 = 0x12_3456;
const SND_LOAD_SID: u32 = 0x12_3457;

/// The sound module's servers (989snd's command server 0x123456 and bank loader 0x123457), answered by the port's own sound
/// engine (`port/uya/src/snd`), which the host attaches (`Iop::sound`). There is no other answer: a sound call with no engine
/// attached stops the game with the call.
pub trait Sound: Send {
    /// One call: the server, the function, the bytes the EE sent; the reply the EE reads back (its receive size).
    fn call(&mut self, host: &mut dyn SoundHost, sid: u32, fno: u32, send: &[u8]) -> Result<Vec<u8>, String>;
}

/// What the sound engine reaches through the runtime, as the IOP module reaches it: EE memory (its SIF transfers) and the disc.
pub trait SoundHost {
    fn read_ee(&mut self, addr: u32, len: usize) -> Result<Vec<u8>, String>;
    fn write_ee(&mut self, addr: u32, bytes: &[u8]) -> Result<(), String>;
    /// EE memory written at once: a change the port itself makes to the game's state inside a sound call. `write_ee` is the
    /// module's own write, which lands at the next vertical blank: too late for what the game's code reads right after the
    /// call returns.
    fn poke_ee(&mut self, addr: u32, bytes: &[u8]);
    fn read_disc(&mut self, lba: u32, sectors: u32) -> Result<Vec<u8>, String>;
    /// The disc's sectors: the CD driver refuses a read past them.
    fn disc_sectors(&mut self) -> u64;
    /// `len` bytes of IOP RAM at `addr`: what the EE sent there with SifSetDma (a bank the sound module loads from IOP memory).
    fn read_iop(&mut self, addr: u32, len: usize) -> Result<Vec<u8>, String>;
}

pub struct Pad {
    /// The field's report: buttons (bit set = released, as the pad sends them), sticks, pressures.
    pub report: [u8; 18],
}

impl Pad {
    pub fn set(&mut self, r: [u8; 18]) {
        self.report = r;
    }
}

pub struct Iop {
    disc: Disc,
    files: Vec<uyafmt::iso::Extent>,
    pub ram: Vec<u8>,
    pub pad: Pad,
    /// The memory card's file in the player's profile.
    pub memcard: Option<String>,
    pub sound: Option<Box<dyn Sound>>,
    /// The EE's SIF0 receive buffer (command 0x80000000), and the packets waiting to go into it.
    pub(crate) ee_rbuf: Option<u32>,
    pub(crate) to_ee: VecDeque<Vec<u8>>,
    /// Command packets that arrived in this SifSetDma batch (the data sent with them arrives in the same batch, before).
    arrived: Vec<Vec<u8>>,
    /// Bound servers by token - 1: (server id, its buffer in IOP RAM).
    servers: Vec<(u32, u32)>,
    heap: u32,
    /// padman's open ports: (port, slot, EE address of the DMA buffer).
    pad_ports: Vec<(u32, u32, u32)>,
    pub(crate) mc: Option<crate::mc::Mc>,
    /// The sound module's transfers into EE memory (a disc read's data, then its status area), in order: they land at the next
    /// vertical blank (`land_sound_writes`).
    sound_writes: Vec<(u32, Vec<u8>)>,
}

impl Iop {
    pub fn new(iso: &str) -> Result<Iop, String> {
        let mut disc = Disc::open(iso)?;
        let v = uyafmt::iso::volume(&mut disc)?;
        let mut idle = [0xffu8; 18];
        idle[2..6].copy_from_slice(&[0x80; 4]);
        idle[6..].fill(0);
        Ok(Iop {
            disc,
            files: v.files,
            ram: vec![0u8; IOP_RAM],
            pad: Pad { report: idle },
            memcard: None,
            sound: None,
            ee_rbuf: None,
            to_ee: VecDeque::new(),
            arrived: Vec::new(),
            servers: Vec::new(),
            heap: HEAP_BASE,
            pad_ports: Vec::new(),
            mc: None,
            sound_writes: Vec::new(),
        })
    }

    /// A file of the disc's file system, whole.
    pub fn disc_file(&mut self, path: &str) -> Result<Vec<u8>, String> {
        let f = self.files.iter().find(|f| f.path.eq_ignore_ascii_case(path)).ok_or_else(|| format!("{path}: not on the disc"))?;
        let (s, n) = (f.sector, f.size);
        self.disc.bytes_at(s * SECTOR, n)
    }

    /// `n` sectors from sector `lsn`.
    pub fn sectors(&mut self, lsn: u64, n: u64) -> Result<Vec<u8>, String> {
        self.disc.sectors(lsn, n)
    }
}

fn w(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap())).unwrap_or(0)
}

fn put(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}

/// EE memory as the IOP's DMA reaches it (any of RAM's windows).
pub fn ee_write(c: &mut Cpu, a: u32, bytes: &[u8]) {
    let p = (a & 0x1fff_ffff) as usize;
    let m = c.mach();
    if p + bytes.len() > crate::RAM_SIZE as usize {
        panic!("the IOP writes {} bytes to EE {a:08x}: past RAM", bytes.len());
    }
    m.ram[p..p + bytes.len()].copy_from_slice(bytes);
}

pub fn ee_read(c: &mut Cpu, a: u32, n: usize) -> Vec<u8> {
    let p = (a & 0x1fff_ffff) as usize;
    let m = c.mach();
    if p + n > crate::RAM_SIZE as usize {
        panic!("the IOP reads {n} bytes from EE {a:08x}: past RAM");
    }
    m.ram[p..p + n].to_vec()
}

/// SifSetDma's transfer of `size` bytes from EE `src` to IOP `dst`. A transfer to the command buffer is a command.
pub fn sif_dma(c: &mut Cpu, src: u32, dst: u32, size: u32, mode: u32) {
    let n = size.div_ceil(16) * 16;
    let d = (dst & 0x1f_ffff) as usize;
    if d + n as usize > IOP_RAM {
        panic!("SifSetDma of {size:#x} bytes from {src:08x} to IOP {dst:08x} (mode {mode:#x}): past the IOP's 2 MB");
    }
    let b = ee_read(c, src, n as usize);
    let iop = &mut c.mach().iop;
    iop.ram[d..d + n as usize].copy_from_slice(&b);
    if dst & 0x1f_ffff == CMDBUF {
        iop.arrived.push(b[..size as usize].to_vec());
    }
}

/// After a SifSetDma batch: the commands it carried, then their answers to the EE.
pub fn sif_dma_done(c: &mut Cpu) {
    let cmds = std::mem::take(&mut c.mach().iop.arrived);
    for p in cmds {
        command(c, &p);
    }
    pump(c);
}

/// The answers waiting for the EE, one at a time into its receive buffer while the buffer is free (libsif's handler takes
/// a packet and clears its first byte), each with the SIF0 DMA's end.
pub fn pump(c: &mut Cpu) {
    loop {
        let m = c.mach();
        let Some(rbuf) = m.iop.ee_rbuf else { return };
        if m.iop.to_ee.is_empty() {
            return;
        }
        let p = (rbuf & 0x1fff_ffff) as usize;
        if m.ram[p] != 0 {
            return;
        }
        let pkt = m.iop.to_ee.pop_front().expect("checked");
        ee_write(c, rbuf, &pkt);
        let m = c.mach();
        m.hw.d_stat |= 1 << 5;
        let mp = c.mp();
        crate::kernel::raise(mp, Some(c), crate::kernel::Int::Dmac(5));
        let m = c.mach();
        if m.ram[p] != 0 {
            // The handler waits (interrupts off, or a handler running): it pumps again when it has run.
            return;
        }
    }
}

/// A packet for the EE: the header (size, no extra data, command id) and the payload words from +0x10.
fn packet(cid: u32, payload: &[u32]) -> Vec<u8> {
    let size = 0x10 + 4 * payload.len();
    let mut p = vec![0u8; size.div_ceil(16) * 16];
    put(&mut p, 0, size as u32);
    put(&mut p, 8, cid);
    for (k, &v) in payload.iter().enumerate() {
        put(&mut p, 0x10 + 4 * k, v);
    }
    p
}

/// One SIF command from the EE.
fn command(c: &mut Cpu, p: &[u8]) {
    let cid = w(p, 8);
    match cid {
        // The EE's receive buffer, once the IOP's command layer is up (sceSifInitCmd at 0x121a6c).
        0x8000_0000 => c.mach().iop.ee_rbuf = Some(w(p, 0x10)),
        // A software register the EE sets on the IOP: nothing here reads them.
        0x8000_0001 => {}
        // The init handshake, by its option (+0xC). 0: sceSifInitCmd's first contact (0x121b2c), the EE's receive buffer at
        // +0x10. 1: sceSifInitRpc's (0x122120): the IOP's RPC layer is up, software register 0 = 1.
        0x8000_0002 => match w(p, 0xc) {
            0 => c.mach().iop.ee_rbuf = Some(w(p, 0x10)),
            1 => {
                let reply = packet(0x8000_0001, &[0, 1]);
                c.mach().iop.to_ee.push_back(reply);
            }
            o => panic!("SIF init command with option {o}, which the game's libsif does not send"),
        },
        // An IOP reset (sceSifRebootIop): the modules the EE binds are this side's own and are there again at once.
        0x8000_0003 => c.mach().iop.servers.clear(),
        0x8000_0009 => {
            let sid = w(p, 0x20);
            if !known(sid) {
                panic!("the game binds IOP server {sid:#x}, which the port's IOP side does not answer");
            }
            // A server id is one server on the IOP: binding it again gives the same server and buffer.
            let iop = &mut c.mach().iop;
            let k = match iop.servers.iter().position(|s| s.0 == sid) {
                Some(k) => k,
                None => {
                    let buf = SERVER_BUFS + SERVER_BUF * iop.servers.len() as u32;
                    if buf + SERVER_BUF > HEAP_BASE {
                        panic!("the game binds a {}th IOP server ({sid:#x}): more than the port has buffers for", iop.servers.len() + 1);
                    }
                    iop.servers.push((sid, buf));
                    iop.servers.len() - 1
                }
            };
            let (token, buf) = (k as u32 + 1, iop.servers[k].1);
            let reply = packet(0x8000_0008, &[w(p, 0x10), w(p, 0x14), w(p, 0x18), w(p, 0x1c), 0x8000_0009, token, buf, buf]);
            iop.to_ee.push_back(reply);
        }
        0x8000_000a => {
            let (client, fno, ssize, recv, rsize, server) = (w(p, 0x1c), w(p, 0x20), w(p, 0x24), w(p, 0x28), w(p, 0x2c), w(p, 0x34));
            let iop = &c.mach().iop;
            let Some(&(sid, buf)) = iop.servers.get((server as usize).wrapping_sub(1)) else {
                panic!("an RPC call (function {fno:#x}) on client {client:08x} to server token {server:#x}, which was never bound");
            };
            if ssize > SERVER_BUF {
                panic!("an RPC call to IOP server {sid:#x} (function {fno:#x}) sends {ssize:#x} bytes, more than its {SERVER_BUF:#x}-byte buffer");
            }
            let send = iop.ram[buf as usize..buf as usize + ssize as usize].to_vec();
            let reply = serve(c, sid, fno, &send, rsize as usize, recv);
            if recv != 0 && rsize != 0 {
                if reply.len() != rsize as usize {
                    panic!("IOP server {sid:#x} function {fno:#x}: a reply of {} bytes for a receive size of {rsize:#x}", reply.len());
                }
                ee_write(c, recv, &reply);
            }
            let end = packet(0x8000_0008, &[w(p, 0x10), w(p, 0x14), w(p, 0x18), client, 0x8000_000a, server, buf, buf]);
            c.mach().iop.to_ee.push_back(end);
        }
        // Read data: IOP memory into the EE.
        0x8000_000c => {
            let (client, src, dst, size) = (w(p, 0x1c), w(p, 0x20), w(p, 0x24), w(p, 0x28));
            let b = c.mach().iop.ram[(src & 0x1f_ffff) as usize..][..size as usize].to_vec();
            ee_write(c, dst, &b);
            let end = packet(0x8000_0008, &[w(p, 0x10), w(p, 0x14), w(p, 0x18), client, 0x8000_000c, 0, 0, 0]);
            c.mach().iop.to_ee.push_back(end);
        }
        _ => panic!("SIF command {cid:#x} from the EE ({} bytes): not one the port's IOP side answers", p.len()),
    }
}

fn known(sid: u32) -> bool {
    matches!(sid, 0x8000_0001 | 0x8000_0003 | 0x8000_0006 | 0x8000_0100 | 0x8000_0101 | 0x8000_0400 | 0x8000_0592 | 0x8000_0593 | 0x8000_0595 | 0x8000_059a | 0x8000_059c | 0x8000_0900 | 0x11 | SND_SID | SND_LOAD_SID)
}

/// The last transfers of disc data into EE memory, for a stop's message (`reads_note`): sceCdRead's (field, thread, sector,
/// sectors, EE buffer) and the sound module's writes as each is queued (field, thread, address, bytes) and as it lands at a
/// vertical blank (field, address, bytes). A trap in code that reads what a read brought (level 35's LZ unpacker) is told apart
/// from one whose stream a later transfer overwrote.
static READS: std::sync::Mutex<std::collections::VecDeque<String>> = std::sync::Mutex::new(std::collections::VecDeque::new());

fn note_read(field: u64, thread: u32, lsn: u32, n: u32, buf: u32) {
    note(format!("field {field} thread {thread} sceCdRead {n} sectors from {lsn} into {buf:08x}..{:08x}", buf.wrapping_add(n * 2048)));
}

fn note(s: String) {
    let mut r = READS.lock().expect("the disc reads");
    if r.len() == 48 {
        r.pop_front();
    }
    r.push_back(s);
}

/// The last transfers, oldest first (`note`).
pub(crate) fn reads_note() -> String {
    READS.lock().expect("the disc reads").iter().cloned().collect::<Vec<_>>().join("; ")
}

/// The reply of `rsize` bytes the EE reads back from its receive buffer.
fn reply_words(rsize: usize, words: &[(usize, u32)]) -> Vec<u8> {
    let mut r = vec![0u8; rsize];
    for &(k, v) in words {
        put(&mut r, 4 * k, v);
    }
    r
}

/// One RPC call to an IOP server: what the EE's library reads back (each case names the library function and what it reads).
fn serve(c: &mut Cpu, sid: u32, fno: u32, send: &[u8], rsize: usize, recv: u32) -> Vec<u8> {
    let stop = || -> ! { panic!("IOP server {sid:#x} function {fno:#x} ({} bytes sent, {rsize:#x} read back to {recv:08x}): not answered by the port's IOP side", send.len()) };
    match (sid, fno) {
        // libcdvd's init (sceCdInit 0x12b300): of the 16 bytes it checks the version at +8 ((v + 0xff) >> 8 < 2, 0x12b4f8).
        (0x8000_0592, 0) => reply_words(rsize, &[]),
        // sceCdDiskReady (0x12b800): the reply word, 2 = SCECdComplete (6 would be not ready).
        (0x8000_059c, 0) => reply_words(rsize, &[(0, 2)]),
        // S commands: 0x22 sceCdMmode (1 = done), 4 sceCdGetError (0 = none), 1 sceCdReadClock (word 0 = 1, the clock at +4).
        (0x8000_0593, 0x22) => reply_words(rsize, &[(0, 1)]),
        (0x8000_0593, 4) => reply_words(rsize, &[(0, 0)]),
        (0x8000_0593, 1) => {
            let mut r = reply_words(rsize, &[(0, 1)]);
            r[4..12].copy_from_slice(&clock());
            r
        }
        // N commands: 14 sceCdGetDiskType (0x14 = PS2 DVD); 1 sceCdRead {lsn, sectors, buf, try, spindle, pattern, bounce}: the
        // sectors straight into the EE buffer, the bounce block's head and tail counts (+0, +4) zero, so its end callback
        // (0x12ab88) has nothing to copy.
        (0x8000_0595, 14) => reply_words(rsize, &[(0, 0x14)]),
        (0x8000_0595, 1) => {
            let (lsn, n, buf, pattern, bounce) = (w(send, 0), w(send, 4), w(send, 8), send[0xe], w(send, 0x10));
            if pattern != 0 {
                panic!("sceCdRead of {n} sectors from {lsn} with data pattern {pattern}: the port reads 2048-byte sectors");
            }
            let data = c.mach().iop.sectors(lsn as u64, n as u64).unwrap_or_else(|e| panic!("sceCdRead {lsn}+{n}: {e}"));
            let (field, thread) = (c.mach().field, c.thread);
            crate::kernel::guard_note(|| format!("field {field} thread {thread} sceCdRead {n} sectors from {lsn} into {buf:08x}"));
            note_read(field, thread, lsn, n, buf);
            ee_write(c, buf, &data);
            ee_write(c, bounce, &[0u8; 8]);
            reply_words(rsize, &[])
        }
        // The IOP heap: 4 sceSifAllocSysMemory {size, type, addr} (0x123b98: the reply word is the block, 0 = none; type 2
        // asks for addr itself), 6 / 7 sceSifQueryTotalFreeMemSize / sceSifQueryMaxFreeMemSize (the reply word).
        (0x8000_0003, 4) => {
            let (size, ty, addr) = (w(send, 0), w(send, 4), w(send, 8));
            let iop = &mut c.mach().iop;
            let a = if ty == 2 {
                addr
            } else {
                let a = iop.heap;
                let next = a + size.div_ceil(256) * 256;
                if next as usize > IOP_RAM {
                    0
                } else {
                    iop.heap = next;
                    a
                }
            };
            reply_words(rsize, &[(0, a)])
        }
        (0x8000_0003, 6 | 7) => {
            let free = IOP_RAM as u32 - c.mach().iop.heap;
            reply_words(rsize, &[(0, free)])
        }
        // loadfile's version query (0x123d60) and file I/O's init (0x1232a0): no code of the game reads their words back.
        (0x8000_0006, 0xff) | (0x8000_0001, 0xff) => reply_words(rsize, &[]),
        // mtapman: every call sends and reads back the 128-byte buffer. 5 its version (0x12eab4 needs version >> 4 == 0x31),
        // 6 its status buffer (the reply is not read), 1 sceMtapPortOpen (word 1 = 1, opened), 3 sceMtapGetConnection (word 1 =
        // 0: no multitap, the pad is plugged in directly).
        (0x8000_0900, 5 | 6 | 1 | 3) => {
            let mut r = send.to_vec();
            r.resize(rsize, 0);
            match fno {
                5 => put(&mut r, 0, 0x316),
                6 => put(&mut r, 0, 0),
                1 => put(&mut r, 4, 1),
                _ => put(&mut r, 4, 0),
            }
            r
        }
        // padman: function 1, the command in word 0 of the 128-byte buffer that comes back.
        (0x8000_0100 | 0x8000_0101, 1) => {
            let mut r = send.to_vec();
            r.resize(rsize, 0);
            match w(send, 0) {
                // Version (0x12fce8 returns word 3; libpad needs major 4, 0x12ed6c).
                0x12 => put(&mut r, 0xc, 0x400),
                // Init (0x12ee18) and end (0x12ef10): word 3, 1 = done.
                0x10 | 0x0f => put(&mut r, 0xc, 1),
                // scePadPortOpen {1, port, slot, -, EE DMA buffer} (0x12f0b8): word 3 = 1 opened, word 5 padman's area for it.
                1 => {
                    let (port, slot, buf) = (w(send, 4), w(send, 8), w(send, 0x10));
                    let iop = &mut c.mach().iop;
                    iop.pad_ports.retain(|&(p, s, _)| (p, s) != (port, slot));
                    iop.pad_ports.push((port, slot, buf));
                    put(&mut r, 0xc, 1);
                    put(&mut r, 0x14, 0x3_0000 + 0x100 * (2 * port + slot));
                }
                // scePadSetMainMode (0x12f860), scePadSetActAlign (0x12f9e0): word 5 = 1, accepted.
                6 | 8 => put(&mut r, 0x14, 1),
                // The pressure-sensitive buttons on (0x12fb68): word 4 = 1.
                10 => put(&mut r, 0x10, 1),
                // 0x18 (0x12fd50): the caller reads nothing back.
                0x18 => {}
                k => panic!("padman command {k:#x}: not answered by the port's IOP side"),
            }
            r
        }
        (0x8000_0400, _) => {
            let r = crate::mc::serve(c, fno, send, rsize);
            // UYA_MCLOG=1: every memory card call, its first words and name, and the reply (an instrument).
            if std::env::var_os("UYA_MCLOG").is_some() {
                let words = |b: &[u8]| b.chunks(4).take(8).map(|x| format!("{:x}", w(x, 0))).collect::<Vec<_>>().join(" ");
                let name = if send.len() > 0x14 { String::from_utf8_lossy(&send[0x14..send.len().min(0x54)]).trim_end_matches('\0').to_string() } else { String::new() };
                println!("mc: field {} fno {fno:#x} send [{}] {name:?} reply [{}]", c.mach().field, words(send), words(&r));
            }
            r
        }
        // The IOP cache: 2 its region {base, size}; 1 {IOP address}: rsize bytes of IOP RAM back to the EE (what it parked
        // there with SifSetDma).
        (0x11, 2) => reply_words(rsize, &[(0, CACHE_BASE), (1, CACHE_SIZE)]),
        (0x11, 1) => {
            let a = (w(send, 0) & 0x1f_ffff) as usize;
            c.mach().iop.ram[a..a + rsize].to_vec()
        }
        (SND_SID | SND_LOAD_SID, _) => {
            let Some(mut snd) = c.mach().iop.sound.take() else {
                panic!("sound call: server {sid:#x} function {fno:#x}, {} bytes sent, {rsize:#x} read back: the port's sound engine (port/uya/src/snd) is not attached to the runtime yet", send.len());
            };
            let r = {
                let mut host = Host { c };
                snd.call(&mut host, sid, fno, send)
            };
            c.mach().iop.sound = Some(snd);
            match r {
                // The engine's reply; the IOP copies at most the receive size.
                Ok(mut r) => {
                    if r.len() < rsize {
                        panic!("sound call: server {sid:#x} function {fno:#x}: a reply of {} bytes for a receive size of {rsize:#x}", r.len());
                    }
                    r.truncate(rsize);
                    // UYA_SNDLOG=1: every sound call, the words sent and the words read back (an instrument).
                    if std::env::var_os("UYA_SNDLOG").is_some() {
                        let words = |b: &[u8]| b.chunks(4).take(12).map(|x| format!("{:x}", w(x, 0))).collect::<Vec<_>>().join(" ");
                        println!("snd: field {} sid {sid:#x} fno {fno:#x} send [{}] ({} bytes) reply [{}]", c.mach().field, words(send), send.len(), words(&r));
                    }
                    r
                }
                Err(e) => panic!("sound call: server {sid:#x} function {fno:#x}: {e}"),
            }
        }
        _ => stop(),
    }
}

/// The sound module's transfers into EE memory, at the vertical blank after the calls that made them.
pub fn land_sound_writes(c: &mut Cpu) {
    let field = c.mach().field;
    for (addr, bytes) in std::mem::take(&mut c.mach().iop.sound_writes) {
        note(format!("field {field} the sound module's write lands at {addr:08x}..{:08x}", addr.wrapping_add(bytes.len() as u32)));
        ee_write(c, addr, &bytes);
    }
}

struct Host<'a> {
    c: &'a mut Cpu,
}

impl SoundHost for Host<'_> {
    fn read_ee(&mut self, addr: u32, len: usize) -> Result<Vec<u8>, String> {
        Ok(ee_read(self.c, addr, len))
    }
    /// Queued: the module's disc read lands, and its status area says so, at the next vertical blank (`land_sound_writes`).
    /// On the console the read takes the drive's time and ends long after the call that asked for it; the game counts on that:
    /// the front end marks its request as in flight (0x3a28b8) only after the sound library's read call (0x13cd28) returns, and
    /// the completion callback (0x3a2ab8, run by 0x13b620 once the busy word 0x1D4980 is 0) finishes only a request in flight.
    fn write_ee(&mut self, addr: u32, bytes: &[u8]) -> Result<(), String> {
        let (field, thread) = (self.c.mach().field, self.c.thread);
        note(format!("field {field} thread {thread} the sound module's write queued for {addr:08x}..{:08x}", addr.wrapping_add(bytes.len() as u32)));
        self.c.mach().iop.sound_writes.push((addr, bytes.to_vec()));
        Ok(())
    }
    fn poke_ee(&mut self, addr: u32, bytes: &[u8]) {
        ee_write(self.c, addr, bytes);
    }
    fn read_disc(&mut self, lba: u32, sectors: u32) -> Result<Vec<u8>, String> {
        let (field, thread) = (self.c.mach().field, self.c.thread);
        note(format!("field {field} thread {thread} the sound module reads {sectors} sectors from {lba}"));
        self.c.mach().iop.sectors(lba as u64, sectors as u64)
    }
    fn disc_sectors(&mut self) -> u64 {
        self.c.mach().iop.disc.sectors
    }
    fn read_iop(&mut self, addr: u32, len: usize) -> Result<Vec<u8>, String> {
        let a = addr as usize;
        self.c.mach().iop.ram.get(a..a + len).map(|b| b.to_vec()).ok_or_else(|| format!("IOP RAM {addr:#x}+{len:#x}: past the IOP's 2 MB"))
    }
}

/// The console's clock as sceCdReadClock gives it: status, second, minute, hour, pad, day, month, year, in BCD, Japan time.
pub(crate) fn clock() -> [u8; 8] {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) + 9 * 3600;
    let (days, s) = (t / 86400, t % 86400);
    // Days to a civil date (proleptic Gregorian).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    let bcd = |v: u64| (((v / 10) << 4) | (v % 10)) as u8;
    [0, bcd(s % 60), bcd(s / 60 % 60), bcd(s / 3600), 0, bcd(d as u64), bcd(m as u64), bcd((y % 100) as u64)]
}

/// padman's frame of every open port, as it DMAs them each field (libpad 4.0 reads them: 0x12f2a0 takes the half of the
/// 0x100-byte buffer with the larger frame number at +0x58). Port 0 slot 0 is a DualShock 2 in analog mode with pressure (mode
/// id 0x79), the field's report; every other port and slot has no pad.
pub fn pad_frames(c: &mut Cpu) {
    let ports = c.mach().iop.pad_ports.clone();
    let report = c.mach().iop.pad.report;
    for (port, slot, buf) in ports {
        let n0 = u32::from_le_bytes(ee_read(c, buf + 0x58, 4).try_into().unwrap());
        let n1 = u32::from_le_bytes(ee_read(c, buf + 0x80 + 0x58, 4).try_into().unwrap());
        let half = if n0 <= n1 { buf } else { buf + 0x80 };
        let mut f = [0u8; 0x80];
        put(&mut f, 0x58, n0.max(n1).wrapping_add(1));
        if port == 0 && slot == 0 {
            // The data scePadRead copies (+0x60 bytes of it, 0x12f3f8): +0 ok, +1 the mode id (0x79: nine halfwords follow),
            // the buttons, sticks and pressures.
            f[0] = 0;
            f[1] = 0x79;
            f[2..20].copy_from_slice(&report);
            put(&mut f, 0x60, 20);
            // What libpad's info functions read (0x12f600, 0x12f720, 0x12fab8): +0x72 1 (the frame is current), +0x71 the request
            // state (0, none in progress; 2 would be failed), +0x64 2 (modes and actuators can be asked about: below 2 they
            // answer nothing), +0x65 the current mode id (scePadInfoMode CURID gives it >> 4, 7: analog), +0x68 / +0x69 / +0x50
            // the mode table (two terminal ids, digital 4 and analog 7, the current one at offset 1), +0x6A two actuators,
            // +0x66 2 (the pressure information is there); +0x70 the state, 6 stable.
            f[0x50..0x54].copy_from_slice(&[4, 0, 7, 0]);
            f[0x64] = 2;
            f[0x65] = 0x79;
            f[0x66] = 2;
            f[0x68] = 2;
            f[0x69] = 1;
            f[0x6a] = 2;
            f[0x70] = 6;
            f[0x71] = 0;
            f[0x72] = 1;
        }
        ee_write(c, half, &f);
    }
}
