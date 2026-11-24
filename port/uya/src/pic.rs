//! A picture as the port saves it (8-bit RGB rows, top first) and its PNG file: zlib stored blocks, no compression. Also the PNG
//! files the port reads (a texture pack's images, `texnames.rs`): 8-bit RGB, RGBA or palette, not interlaced, inflated here.

pub struct Image {
    pub w: usize,
    pub h: usize,
    pub rgb: Vec<u8>,
}

fn crc32(table: &[u32; 256], bytes: &[u8], mut c: u32) -> u32 {
    for &b in bytes {
        c = table[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    c
}

/// The PNG file of `img`.
pub fn png(img: &Image) -> Vec<u8> {
    png_of(img.w, img.h, 3, &img.rgb)
}

/// The PNG file of `w` x `h` RGBA8 pixels, rows top first.
pub fn png_rgba(w: usize, h: usize, rgba: &[u8]) -> Vec<u8> {
    png_of(w, h, 4, rgba)
}

/// A PNG of 8-bit rows of `channels` (3: RGB, 4: RGBA).
fn png_of(w: usize, h: usize, channels: usize, px: &[u8]) -> Vec<u8> {
    let mut table = [0u32; 256];
    for (n, t) in table.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    // The filtered rows: filter type 0, then the row.
    let mut raw = Vec::with_capacity((w * channels + 1) * h);
    for row in px.chunks_exact(w * channels) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    // zlib: header, stored blocks of at most 65535 bytes, Adler-32.
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push((i + 1 == blocks.len()) as u8);
        let n = b.len() as u16;
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(b);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let c = crc32(&table, data, crc32(&table, kind, 0xffff_ffff)) ^ 0xffff_ffff;
        out.extend_from_slice(&c.to_be_bytes());
    };
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, if channels == 4 { 6 } else { 2 }, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut s) = (1u32, 0u32);
    for chunk in bytes.chunks(5552) {
        for &v in chunk {
            a += v as u32;
            s += a;
        }
        a %= 65521;
        s %= 65521;
    }
    (s << 16) | a
}

/// A PNG file's pixels as RGBA8 (`w`, `h`, rows top first): 8-bit RGB (alpha 255), RGBA, or palette (tRNS alpha, else 255), not
/// interlaced. Any other kind is refused with its reason.
pub fn read_png(file: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    if file.get(..8) != Some(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Err("not a PNG file".into());
    }
    let be32 = |b: &[u8]| u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
    let (mut at, mut head, mut plte, mut trns, mut idat) = (8, None, Vec::new(), Vec::new(), Vec::new());
    while at + 8 <= file.len() {
        let len = be32(&file[at..]) as usize;
        let body = file.get(at + 8..at + 8 + len).ok_or("a chunk runs past the end of the file")?;
        match &file[at + 4..at + 8] {
            b"IHDR" if len == 13 => head = Some((be32(body), be32(&body[4..]), body[8], body[9], body[12])),
            b"PLTE" => plte = body.to_vec(),
            b"tRNS" => trns = body.to_vec(),
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        at += 12 + len;
    }
    let (w, h, depth, kind, interlace) = head.ok_or("no IHDR")?;
    let channels = match (depth, kind, interlace) {
        (8, 2, 0) => 3,
        (8, 3, 0) => 1,
        (8, 6, 0) => 4,
        _ => return Err(format!("bit depth {depth}, colour type {kind}, interlace {interlace}: only 8-bit RGB, RGBA and palette files, not interlaced, are read")),
    };
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        return Err(format!("{w} x {h} is outside 1 to 8192"));
    }
    let (wu, hu) = (w as usize, h as usize);
    let row = wu * channels;
    let mut raw = inflate(&idat, (row + 1) * hu)?;
    // The filters, each row on the one above (bytes `channels` to the left).
    for y in 0..hu {
        let (done, rest) = raw.split_at_mut(y * (row + 1));
        let up = (y > 0).then(|| &done[(y - 1) * (row + 1) + 1..]);
        let (filter, cur) = rest[..row + 1].split_first_mut().expect("a row");
        for x in 0..row {
            let a = if x >= channels { cur[x - channels] } else { 0 };
            let b = up.map_or(0, |u| u[x]);
            let c = if x >= channels { up.map_or(0, |u| u[x - channels]) } else { 0 };
            cur[x] = cur[x].wrapping_add(match *filter {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((a as u16 + b as u16) / 2) as u8,
                4 => {
                    let p = a as i16 + b as i16 - c as i16;
                    let (pa, pb, pc) = ((p - a as i16).abs(), (p - b as i16).abs(), (p - c as i16).abs());
                    if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    }
                }
                f => return Err(format!("row {y}: filter {f}")),
            });
        }
    }
    let mut rgba = Vec::with_capacity(wu * hu * 4);
    for r in raw.chunks_exact(row + 1).map(|r| &r[1..]) {
        match channels {
            4 => rgba.extend_from_slice(r),
            3 => r.chunks_exact(3).for_each(|p| rgba.extend_from_slice(&[p[0], p[1], p[2], 255])),
            _ => {
                for &i in r {
                    let i = i as usize;
                    let c = plte.get(3 * i..3 * i + 3).ok_or_else(|| format!("palette index {i} past the palette of {}", plte.len() / 3))?;
                    rgba.extend_from_slice(&[c[0], c[1], c[2], trns.get(i).copied().unwrap_or(255)]);
                }
            }
        }
    }
    Ok((w, h, rgba))
}

/// One Huffman code of deflate as a table of the next `bits` input bits (least significant first): each entry is the symbol << 4
/// and the code's length.
struct Huffman {
    bits: u32,
    table: Vec<u16>,
}

impl Huffman {
    /// The canonical code of these code lengths (0: the symbol is not used).
    fn new(lengths: &[u8]) -> Result<Huffman, String> {
        let bits = lengths.iter().copied().max().unwrap_or(0).max(1) as u32;
        let mut count = [0u16; 16];
        for &l in lengths {
            count[l as usize] += 1;
        }
        count[0] = 0;
        let mut next = [0u32; 16];
        let mut code = 0u32;
        for l in 1..16 {
            code = (code + count[l - 1] as u32) << 1;
            next[l] = code;
        }
        let mut table = vec![0u16; 1 << bits];
        for (sym, &l) in lengths.iter().enumerate().filter(|&(_, &l)| l > 0) {
            let l = l as u32;
            let c = next[l as usize];
            next[l as usize] += 1;
            if c >= 1 << l {
                return Err("an over-full Huffman code".into());
            }
            let rev = c.reverse_bits() >> (32 - l);
            for k in 0..1u32 << (bits - l) {
                table[(rev | k << l) as usize] = (sym as u16) << 4 | l as u16;
            }
        }
        Ok(Huffman { bits, table })
    }
}

/// Bits of a deflate stream, least significant first.
struct Bits<'a> {
    input: &'a [u8],
    at: usize,
    buf: u64,
    n: u32,
}

impl Bits<'_> {
    /// At least 32 bits in `buf` (zeros past the end of the input; `end` tells whether they were used).
    fn fill(&mut self) {
        while self.n <= 56 {
            let b = self.input.get(self.at).copied().unwrap_or(0);
            self.at += 1;
            self.buf |= (b as u64) << self.n;
            self.n += 8;
        }
    }

    fn take(&mut self, k: u32) -> u32 {
        self.fill();
        let v = (self.buf & ((1u64 << k) - 1)) as u32;
        self.buf >>= k;
        self.n -= k;
        v
    }

    fn symbol(&mut self, h: &Huffman) -> Result<u16, String> {
        self.fill();
        let e = h.table[(self.buf & ((1u64 << h.bits) - 1)) as usize];
        if e & 15 == 0 {
            return Err("a code not in the Huffman table".into());
        }
        self.buf >>= e & 15;
        self.n -= (e & 15) as u32;
        Ok(e >> 4)
    }

    /// Whether more bits were read than the input has.
    fn past_end(&self) -> bool {
        self.at * 8 > self.input.len() * 8 + self.n as usize
    }
}

const LENGTH_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LENGTH_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

/// zlib's data (RFC 1950 around RFC 1951's deflate), which must come to `size` bytes, with its Adler-32 checked.
fn inflate(z: &[u8], size: usize) -> Result<Vec<u8>, String> {
    if z.len() < 6 || z[0] & 0x0f != 8 || (u16::from(z[0]) << 8 | u16::from(z[1])) % 31 != 0 || z[1] & 0x20 != 0 {
        return Err("not zlib data".into());
    }
    let mut out = Vec::with_capacity(size);
    let mut b = Bits { input: &z[2..], at: 0, buf: 0, n: 0 };
    loop {
        let last = b.take(1);
        match b.take(2) {
            0 => {
                // Stored: from the next byte, LEN and its complement, then LEN bytes.
                let k = b.n % 8;
                b.take(k);
                let (len, nlen) = (b.take(16), b.take(16));
                if len != !nlen & 0xffff {
                    return Err("a stored block's length does not match its complement".into());
                }
                for _ in 0..len {
                    out.push(b.take(8) as u8);
                }
            }
            kind @ (1 | 2) => {
                let (lit, dist) = if kind == 1 {
                    let mut l = [8u8; 288];
                    l[144..256].fill(9);
                    l[256..280].fill(7);
                    (Huffman::new(&l)?, Huffman::new(&[5u8; 30])?)
                } else {
                    let (hlit, hdist, hclen) = (b.take(5) as usize + 257, b.take(5) as usize + 1, b.take(4) as usize + 4);
                    let mut cl = [0u8; 19];
                    for &k in [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15].iter().take(hclen) {
                        cl[k] = b.take(3) as u8;
                    }
                    let clh = Huffman::new(&cl)?;
                    let mut lengths = Vec::with_capacity(hlit + hdist);
                    while lengths.len() < hlit + hdist {
                        match b.symbol(&clh)? {
                            s @ 0..=15 => lengths.push(s as u8),
                            16 => {
                                let prev = *lengths.last().ok_or("a repeat with no length before it")?;
                                let n = 3 + b.take(2);
                                lengths.extend(std::iter::repeat_n(prev, n as usize));
                            }
                            17 => lengths.extend(std::iter::repeat_n(0, 3 + b.take(3) as usize)),
                            _ => lengths.extend(std::iter::repeat_n(0, 11 + b.take(7) as usize)),
                        }
                    }
                    if lengths.len() != hlit + hdist {
                        return Err("code lengths run past the codes".into());
                    }
                    (Huffman::new(&lengths[..hlit])?, Huffman::new(&lengths[hlit..])?)
                };
                loop {
                    let s = b.symbol(&lit)? as usize;
                    if s < 256 {
                        out.push(s as u8);
                        continue;
                    }
                    if s == 256 {
                        break;
                    }
                    let k = s - 257;
                    if k >= 29 {
                        return Err(format!("length symbol {s}"));
                    }
                    let len = LENGTH_BASE[k] as usize + b.take(LENGTH_EXTRA[k] as u32) as usize;
                    let d = b.symbol(&dist)? as usize;
                    if d >= 30 {
                        return Err(format!("distance symbol {d}"));
                    }
                    let back = DIST_BASE[d] as usize + b.take(DIST_EXTRA[d] as u32) as usize;
                    if back > out.len() {
                        return Err("a distance before the start".into());
                    }
                    let from = out.len() - back;
                    for i in 0..len {
                        out.push(out[from + i]);
                    }
                }
            }
            _ => return Err("block type 3".into()),
        }
        if out.len() > size {
            return Err(format!("more than the {size} bytes of the image"));
        }
        if last == 1 {
            break;
        }
    }
    if b.past_end() {
        return Err("the data ends inside a block".into());
    }
    if out.len() != size {
        return Err(format!("{} bytes, not the image's {size}", out.len()));
    }
    // The Adler-32 after the deflate data, from the next byte.
    let k = b.n % 8;
    b.take(k);
    let sum = (0..4).fold(0u32, |s, _| s << 8 | b.take(8));
    if b.past_end() || sum != adler32(&out) {
        return Err("the Adler-32 does not match".into());
    }
    Ok(out)
}
