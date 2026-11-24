//! spectexcheck DISC.iso OUTDIR [RAM.bin]: decodes `core::special_textures` (the moby textures -2 and -3) of all 34 story levels and
//! writes each as OUTDIR/LNN_m2.png and LNN_m3.png (RGBA, alpha 0x80 = 1.0 scaled to 255). With RAM.bin (captures/trace/ram02400.bin,
//! Veldin's EE RAM at field 2400) it searches the RAM for the decoded palettes (the 0x400 bytes of part 2) and the texels.
//!
//! What it would show if the decoder were wrong: a wrong palette order, texel offset or size gives noise or a wrong image instead
//! of a recognisable texture; a wrong palette or texel offset also finds no match in RAM.
use uyafmt::disc::{u32_at, Disc};
use uyafmt::toc::{self, FileKind};
use uyafmt::track::{Space, Tracker};

fn crc32(d: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in d {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

fn png(path: &str, w: usize, h: usize, px: &[[u8; 4]]) {
    let mut raw = Vec::new();
    for y in 0..h {
        raw.push(0u8);
        for x in 0..w {
            let p = px[y * w + x];
            raw.extend_from_slice(&[p[0], p[1], p[2], (p[3] as u32 * 255 / 0x80).min(255) as u8]);
        }
    }
    let mut z = vec![0x78, 1];
    let mut chunks = raw.chunks(65535).peekable();
    while let Some(c) = chunks.next() {
        z.push(if chunks.peek().is_none() { 1 } else { 0 });
        z.extend_from_slice(&(c.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(c.len() as u16)).to_le_bytes());
        z.extend_from_slice(c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10];
    let mut chunk = |t: &[u8; 4], d: &[u8]| {
        out.extend_from_slice(&(d.len() as u32).to_be_bytes());
        let mut td = t.to_vec();
        td.extend_from_slice(d);
        out.extend_from_slice(&td);
        out.extend_from_slice(&crc32(&td).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    std::fs::write(path, out).expect("write png");
}

fn find(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    let mut v = Vec::new();
    let mut i = 0;
    while let Some(p) = hay[i..].windows(needle.len()).position(|w| w == needle) {
        v.push(i + p);
        i += p + 1;
    }
    v
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: spectexcheck DISC.iso OUTDIR [RAM.bin]");
        std::process::exit(2);
    }
    if let Err(e) = run(&a[1], &a[2], a.get(3)) {
        eprintln!("spectexcheck: {e}");
        std::process::exit(1);
    }
}

fn run(iso: &str, out: &str, ram: Option<&String>) -> Result<(), String> {
    let mut d = Disc::open(iso)?;
    let tc = toc::read(&mut d)?;
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    for level in (1..=14).chain(16..=24).chain(26..=36) {
        let f = tc.files.iter().find(|f| f.level == level && f.kind == FileKind::Level).ok_or(format!("no LEVEL file for level {level}"))?;
        let (off, n) = (u32_at(&f.header, 0x10)? as u64, u32_at(&f.header, 0x14)? as u64);
        let b = d.sectors(f.sector + off, n)?;
        let part = |k: usize| -> Result<(usize, usize), String> { Ok((u32_at(&b, 8 * k)? as usize, u32_at(&b, 8 * k + 4)? as usize)) };
        let (io, is) = part(1)?;
        let (go, gsz) = part(2)?;
        let (idx, gs) = (&b[io..io + is], &b[go..go + gsz]);
        let mut t = Tracker::new();
        let mut region = t.root("LEVEL level data", "L", b.len() as u64, Space::Disc);
        let images = uyafmt::hud::gs_ram(&mut t, &mut region, go as u64, gs, idx)?;
        let tex = uyafmt::core::special_textures(idx, gs, &images).map_err(|e| format!("L{level:02}: {e}"))?;
        for (k, x) in tex.iter().enumerate() {
            let name = format!("{out}/L{level:02}_m{}.png", k + 2);
            png(&name, x.width, x.height, &x.levels[0]);
            let opaque = x.levels[0].iter().filter(|p| p[3] == 0x80).count();
            println!("L{level:02} -{}: {} x {} -> {name} ({} of {} texels alpha 0x80)", k + 2, x.width, x.height, opaque, x.levels[0].len());
        }
        if level == 1 {
            if let Some(r) = ram {
                let ram = std::fs::read(r).map_err(|e| format!("{r}: {e}"))?;
                let w = |i: usize| u32_at(idx, 4 * i).map(|v| v as usize);
                let tab = w(1)?;
                let n0 = w(0)?;
                for (k, (tw, cw)) in [(w(0x24)?, w(0x25)?), (w(0x26)?, w(0x27)?)].into_iter().enumerate() {
                    let pal = &gs[cw..cw + 0x400];
                    let e = (n0..n0 + w(0x21)?).map(|e| tab + 16 * e).find(|&e| u32_at(idx, e + 0xc).map(|v| v as usize) == Ok(tw)).ok_or("image")?;
                    let wh = u32_at(idx, e + 4)?;
                    let (iw, ih, o8) = ((wh & 0xffff) as usize, (wh >> 16) as usize, u32_at(idx, e + 8)? as usize);
                    let texels = &gs[o8..o8 + iw * ih];
                    println!("RAM: -{} palette (0x400 bytes, part 2 +{cw:#x}) at {:x?}; texels ({iw} x {ih}, part 2 +{o8:#x}) full at {:x?}, first 256 bytes at {:x?}, last 256 bytes at {:x?}", k + 2, find(&ram, pal), find(&ram, texels), find(&ram, &texels[..256]), find(&ram, &texels[texels.len() - 256..]));
                }
                println!("RAM: GS RAM image's first 0x1000 bytes of part 2 at {:x?}", find(&ram, &gs[..0x1000]));
            }
        }
    }
    Ok(())
}
