//! fxcheck DISC.iso OUTDIR: prints `fxcode::addresses` (program 6's input addresses) for the 34 story levels, checks level 1's
//! against `prog6::veldin_addrs`, and writes each level's effect textures (`Core::effect_textures`) as OUTDIR/LNN_fxMMM.png.
//!
//! What it would show if the decoder or the addresses were wrong: a wrong address function fails to find the function, a base
//! register or a field offset (an error naming the level and the offset), or gives level 1 addresses that differ from Veldin's;
//! a wrong palette order, texel offset or size in the texture decoder gives noise instead of recognisable pictures (sand and
//! smoke clouds, sparks, rings).

use uyafmt::disc::Disc;
use uyafmt::levelcode::{self, STORY_LEVELS};
use uyafmt::{fxcode, prog6, toc, walk};

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

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 3 {
        return Err("usage: fxcheck DISC.iso OUTDIR".into());
    }
    std::fs::create_dir_all(&a[2]).map_err(|e| e.to_string())?;
    let mut d = Disc::open(&a[1])?;
    let tc = toc::read(&mut d)?;
    let reference = levelcode::overlay(&mut d, &tc, 1)?;
    println!("level gp       pool     maxidx   limit    cam      rot      clip     vf24     vf26     vf27     vf25     tex0     textures sine     vram     vumat    vuq4     vuq5     staticptr");
    for &l in STORY_LEVELS.iter() {
        let ov = levelcode::overlay(&mut d, &tc, l)?;
        let x = fxcode::addresses(&reference, &ov).map_err(|e| format!("L{l:02}: {e}"))?;
        println!(
            "L{l:02}   {:06x} {:06x}   {:06x}   {:06x}   {:06x}   {:06x}   {:06x}   {:06x}   {:06x}   {:06x}   {:06x}   {:06x}   {:06x}   {:06x}   {:06x}   {:06x}   {:06x}   {:06x}   {:06x}",
            x.gp, x.pool, x.max_index, x.limit, x.cam, x.rot, x.clip, x.vf24, x.vf26, x.vf27, x.vf25, x.tex0_template, x.textures, x.sine, x.vram, x.vu_matrix, x.vu_q4, x.vu_q5, x.static_ptr
        );
        if l == 1 {
            if x != prog6::veldin_addrs() {
                return Err(format!("level 1's addresses {x:x?} differ from Veldin's {:x?}", prog6::veldin_addrs()));
            }
            println!("  level 1 equals prog6::veldin_addrs");
        }
        let data = walk::level(&a[1], l)?;
        let core = data.core.ok_or("no core")?;
        let mut line = format!("  L{l:02}: {} effect textures:", core.effect_textures.len());
        for (k, t) in core.effect_textures.iter().enumerate() {
            png(&format!("{}/L{l:02}_fx{k:03}.png", a[2]), t.width, t.height, &t.levels[0]);
            line += &format!(" {k}:{}x{}", t.width, t.height);
        }
        println!("{line}");
    }
    Ok(())
}
