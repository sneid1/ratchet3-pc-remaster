//! Reading a structure's fields from its bytes while marking them read.

use crate::track::Node;

/// A structure's bytes and its node: every field read through here is marked read.
pub struct Fields<'a, 'n> {
    pub b: &'a [u8],
    pub n: &'n mut Node,
    /// Where `b` starts in the node.
    pub base: u64,
}

impl<'a, 'n> Fields<'a, 'n> {
    pub fn new(b: &'a [u8], n: &'n mut Node) -> Self {
        Fields { b, n, base: 0 }
    }

    pub fn u32(&mut self, at: usize) -> Result<u32, String> {
        let v = crate::disc::u32_at(self.b, at)?;
        self.n.mark(self.base + at as u64, 4);
        Ok(v)
    }

    /// A u32 the game does not read: marked only when it equals `want`.
    pub fn u32_eq(&mut self, at: usize, want: u32, what: &str) -> Result<(), String> {
        let v = crate::disc::u32_at(self.b, at)?;
        if v != want {
            return Err(format!("+{at:#x} {what}: {v:#x}, expected {want:#x}"));
        }
        self.n.mark(self.base + at as u64, 4);
        Ok(())
    }

    /// Bytes that must all be `fill`.
    pub fn fill(&mut self, at: usize, len: usize, fill: u8, what: &str) -> Result<(), String> {
        let s = self.b.get(at..at + len).ok_or_else(|| format!("+{at:#x}+{len:#x} {what}: past the end {:#x}", self.b.len()))?;
        if let Some(k) = s.iter().position(|&v| v != fill) {
            return Err(format!("+{:#x} {what}: byte {:#04x}, expected {fill:#04x}", at + k, s[k]));
        }
        self.n.mark(self.base + at as u64, len as u64);
        Ok(())
    }

    /// Bytes that must equal `want`.
    pub fn same(&mut self, at: usize, want: &[u8], what: &str) -> Result<(), String> {
        let s = self.b.get(at..at + want.len()).ok_or_else(|| format!("+{at:#x} {what}: past the end"))?;
        if let Some(k) = s.iter().zip(want).position(|(a, b)| a != b) {
            return Err(format!("+{:#x} {what}: byte {:#04x}, expected {:#04x}", at + k, s[k], want[k]));
        }
        self.n.mark(self.base + at as u64, want.len() as u64);
        Ok(())
    }

    pub fn mark(&mut self, at: usize, len: usize) {
        self.n.mark(self.base + at as u64, len as u64);
    }

    /// Skips bytes the decoder has shown nothing in the game reads (`Node::skip`).
    pub fn skip(&mut self, at: usize, len: usize) {
        self.n.skip(self.base + at as u64, len as u64);
    }
}
