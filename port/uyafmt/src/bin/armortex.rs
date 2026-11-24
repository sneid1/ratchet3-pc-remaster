//! armortex DISC.iso ENTRY OUTDIR: writes armor WAD entry ENTRY's images as `moby::armor_entry` decodes them (OUTDIR/aENTRY_N.png).

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

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 4 {
        return Err("usage: armortex DISC.iso ENTRY OUTDIR".into());
    }
    let entry: usize = a[2].parse().map_err(|_| "ENTRY is a number")?;
    std::fs::create_dir_all(&a[3]).map_err(|e| e.to_string())?;
    let (armor, textures) = uyafmt::moby::armor_entry(&a[1], entry)?;
    println!("entry {entry}: {} meshes, {} images", armor.meshes.len(), textures.len());
    for (k, t) in textures.iter().enumerate() {
        let path = format!("{}/a{entry}_{k}.png", a[3]);
        png(&path, t.width, t.height, &t.levels[0]);
        println!("  {path}: {} x {}", t.width, t.height);
    }
    Ok(())
}
