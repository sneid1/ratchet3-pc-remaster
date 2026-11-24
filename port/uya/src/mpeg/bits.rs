//! A bit reader over an MPEG-2 video elementary stream: bits come most significant first, and past the end of the data they read as
//! zeros (a slice ends at a start code, so the reader must be able to look a few bits past the last byte).

pub struct Bits<'a> {
    d: &'a [u8],
    /// The position in bits.
    pub pos: usize,
}

impl<'a> Bits<'a> {
    pub fn new(d: &'a [u8]) -> Bits<'a> {
        Bits { d, pos: 0 }
    }

    /// The next `n` bits (0..=32) without taking them.
    #[inline]
    pub fn peek(&self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let byte = self.pos >> 3;
        let v = match self.d.get(byte..byte + 8) {
            Some(w) => u64::from_be_bytes(w.try_into().expect("8 bytes")),
            None => (0..8).fold(0u64, |v, i| v << 8 | *self.d.get(byte + i).unwrap_or(&0) as u64),
        };
        ((v << (self.pos & 7)) >> (64 - n)) as u32
    }

    #[inline]
    pub fn skip(&mut self, n: u32) {
        self.pos += n as usize;
    }

    #[inline]
    pub fn get(&mut self, n: u32) -> u32 {
        let v = self.peek(n);
        self.pos += n as usize;
        v
    }

    #[inline]
    pub fn get1(&mut self) -> bool {
        self.get(1) != 0
    }

    /// Whether the position is past the data's end.
    pub fn over(&self) -> bool {
        self.pos > self.d.len() * 8
    }

    /// Whether the next 23 bits are zero: a start code's prefix follows, perhaps after stuffing zeros (the end of a slice).
    pub fn at_start_code(&self) -> bool {
        self.peek(23) == 0
    }
}
