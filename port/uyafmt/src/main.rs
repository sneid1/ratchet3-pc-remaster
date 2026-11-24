//! uyafmt disc DISC.iso [--out the project’s decode notes]: walks the whole disc and writes the decode tracker.

const HEADER: &str = "Written by `uyafmt disc disc/SCUS_973.53.iso` (port/uyafmt); never edited by hand. One line per structure kind, \
grouped by file kind. Bytes are disc bytes, except lines marked `unpacked`, which count bytes of a block unpacked from an LZ \
stream; an LZ stream's disc bytes count as mapped only when everything unpacked from it is mapped too.

A line is `mapped` when a decoder in port/uyafmt walked every instance of it on the disc without error, every byte of it is a \
field the decoder reads or padding it requires to be zero, and the line names the game function its fields come from (NTSC-U \
addresses: `func_XXXXXXXX` in boot_elf, `func_LNN_XXXXXXXX` in level NN's overlay). Mapped is a map: where a structure is and \
what reads it. A mapped line is `loaded` when its decoder returns the structure's data and the port has used it (drawn, played \
or shown). A mapped line is `claimed` when its read or skipped marks rest on the partial evidence the project’s plan lists under \
\"Unproven claims in the map\"; its bytes are counted as claimed, not proven. Anything less than mapped is `not mapped`, with \
what is missing. `unknown bytes` lines hold the bytes of a file that no structure line claims yet. Code lines (program images) are \
counted apart and read `recompiled` or `not recompiled`.";

/// uyafmt colours DISC.iso LEVEL RAM.bin: the terrain's lit colours against the colours the game wrote into its copy of the core
/// (RAM.bin: 32 MB of EE RAM from address 0, with the level loaded; the core is at the core index's load address, the terrain
/// at core offset 0, each chunk's data at its absolute +0x10 and its colours at +0x1E).
fn colours(iso: &str, level: usize, ram: &str) -> Result<(), String> {
    let data = uyafmt::walk::level(iso, level)?;
    let core = data.core.ok_or("no core")?;
    let ram = std::fs::read(ram).map_err(|e| format!("{ram}: {e}"))?;
    let rd = |a: usize, n: usize| ram.get(a..a + n).ok_or_else(|| format!("RAM {a:#x}+{n:#x} past the dump"));
    let u32_ = |a: usize| -> Result<usize, String> { Ok(u32::from_le_bytes(rd(a, 4)?.try_into().unwrap()) as usize) };
    // The terrain sits at 0x91E000 on Veldin (the core's load address); its chunk table at +0x40.
    let base = 0x91e000;
    if u32_(base + 4)? != core.terrain.chunks.len() {
        return Err(format!("RAM {base:#x}: {} chunks, the disc's terrain has {}", u32_(base + 4)?, core.terrain.chunks.len()));
    }
    let (mut same, mut off1, mut worse, mut shown) = (0usize, 0usize, 0usize, 0);
    let mut keys_differ = 0;
    for (k, c) in core.terrain.chunks.iter().enumerate() {
        let e = base + 0x40 + 0x40 * k;
        let ram_key = (rd(e + 0x3a, 1)?[0], rd(e + 0x3b, 1)?[0]);
        if ram_key != c.visibility {
            keys_differ += 1;
            if keys_differ <= 5 {
                println!("chunk {k}: game's mask and zone {ram_key:02x?}, decoder {:02x?}", c.visibility);
            }
        }
        let at = u32_(e + 0x10)? + u16::from_le_bytes(rd(e + 0x1e, 2)?.try_into().unwrap()) as usize;
        for (i, col) in c.colours.iter().enumerate() {
            let g = rd(at + 4 * i, 4)?;
            let d = (0..4).map(|j| (g[j] as i32 - col[j] as i32).abs()).max().unwrap();
            match d {
                0 => same += 1,
                1 => off1 += 1,
                _ => {
                    worse += 1;
                    if shown < 10 {
                        shown += 1;
                        println!("chunk {k} colour {i}: game {g:02x?}, decoder {col:02x?}");
                    }
                }
            }
        }
    }
    println!("level {level}: {} chunks; colours equal to the game's {same}, off by 1 {off1}, off by more {worse}; visibility keys differing from the game's {keys_differ}", core.terrain.chunks.len());
    // The tie instances: the instance array at [0x1E0F54] (32 bytes: +0x10 the visibility key) and the matrix array at
    // [0x1E0F5C] (64 bytes: rows 0 to 3, x y z).
    let g = data.gameplay.ok_or("no gameplay block")?;
    let (ta, ma, n) = (u32_(0x1e0f54)?, u32_(0x1e0f5c)?, u32_(0x1e0f60)?);
    if n != g.ties.len() {
        return Err(format!("RAM: {n} tie instances, the gameplay block has {}", g.ties.len()));
    }
    let keys = g.tie_keys(core.has_grid);
    let f = |a: usize| -> Result<f32, String> { Ok(f32::from_bits(u32_(a)? as u32)) };
    let (mut kd, mut md) = (0, 0);
    for (i, t) in g.ties.iter().enumerate() {
        let k = (rd(ta + 32 * i + 0x10, 1)?[0], rd(ta + 32 * i + 0x11, 1)?[0]);
        if k != keys[i] {
            kd += 1;
            if kd <= 6 {
                println!("tie {i} (id {}, class {}): game's key {k:02x?}, decoder {:02x?}", t.id, t.class, keys[i]);
            }
        }
        let rows = [t.axes[0], t.axes[1], t.axes[2], t.position];
        for (r, row) in rows.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                let game = f(ma + 64 * i + 16 * r + 4 * c)?;
                if game != *v {
                    md += 1;
                    if md <= 8 {
                        println!("tie {i} (id {}, class {}) row {r} column {c}: game {game}, decoder {v}", t.id, t.class);
                    }
                }
            }
        }
    }
    println!("level {level}: {n} tie instances; visibility keys differing from the game's {kd}; matrix values differing {md}");
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 5 && args[1] == "colours" {
        if let Err(e) = colours(&args[2], args[3].parse().expect("level"), &args[4]) {
            eprintln!("uyafmt colours: {e}");
            std::process::exit(1);
        }
        return;
    }
    if args.len() < 3 || args[1] != "disc" {
        eprintln!("usage: uyafmt disc DISC.iso [--out the project’s decode notes]
       uyafmt colours DISC.iso LEVEL RAM.bin");
        std::process::exit(2);
    }
    let iso = &args[2];
    let out = args.iter().position(|a| a == "--out").and_then(|i| args.get(i + 1)).cloned().unwrap_or("the project’s decode notes".into());
    let t0 = std::time::Instant::now();
    match uyafmt::walk::walk(iso) {
        Ok(w) => {
            let report = w.tracker.report(w.disc_bytes, w.overlays, w.overlays_recompiled, HEADER);
            if let Err(e) = std::fs::write(&out, &report) {
                eprintln!("{out}: {e}");
                std::process::exit(1);
            }
            let top: Vec<&str> = report.lines().filter(|l| l.starts_with("Data:") || l.starts_with("Code:")).collect();
            println!("{}\n{out} written in {:.1} s", top.join("\n"), t0.elapsed().as_secs_f64());
        }
        Err(e) => {
            eprintln!("uyafmt disc: {e}");
            std::process::exit(1);
        }
    }
}
