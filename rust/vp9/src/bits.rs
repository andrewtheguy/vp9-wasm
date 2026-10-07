//! The two readers of a frame: plain bits, for the uncompressed header, and
//! the boolean decoder everything after it is coded with.

use crate::error::{Error, Result};
use crate::tables::NORM;

/// Bits most significant first.
pub struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        BitReader { data, pos: 0 }
    }

    pub fn bit(&mut self) -> Result<u32> {
        let byte = *self.data.get(self.pos >> 3).ok_or_else(|| Error::invalid("the frame header runs past the frame"))?;
        let bit = (byte >> (7 - (self.pos & 7))) & 1;
        self.pos += 1;
        Ok(bit as u32)
    }

    pub fn flag(&mut self) -> Result<bool> {
        Ok(self.bit()? != 0)
    }

    pub fn literal(&mut self, bits: u32) -> Result<u32> {
        let mut v = 0;
        for _ in 0..bits {
            v = (v << 1) | self.bit()?;
        }
        Ok(v)
    }

    /// A magnitude of `bits` bits, then its sign.
    pub fn signed(&mut self, bits: u32) -> Result<i32> {
        let v = self.literal(bits)? as i32;
        Ok(if self.flag()? { -v } else { v })
    }

    /// Whole bytes read so far.
    pub fn bytes_read(&self) -> usize {
        self.pos.div_ceil(8)
    }
}

/// What the window is credited with once the data has run out, so that it is
/// never refilled again: zeros follow the last byte.
const PAST_END: i32 = 0x4000_0000;

/// The boolean decoder: libvpx's, with a 64-bit window.
pub struct BoolDecoder<'a> {
    data: &'a [u8],
    pos: usize,
    value: u64,
    /// Bits in `value` below its top byte; negative when it needs a refill.
    count: i32,
    range: u32,
}

impl<'a> BoolDecoder<'a> {
    /// A decoder over one partition, whose first bit is a marker that must be 0.
    pub fn new(data: &'a [u8]) -> Result<Self> {
        if data.is_empty() {
            return Err(Error::invalid("an empty partition"));
        }
        let mut d = BoolDecoder { data, pos: 0, value: 0, count: -8, range: 255 };
        d.fill();
        if d.bit() {
            return Err(Error::invalid("a partition's marker bit is set"));
        }
        Ok(d)
    }

    /// A decoder of nothing, to stand where one will be.
    pub fn empty() -> Self {
        BoolDecoder { data: &[], pos: 0, value: 0, count: 0, range: 255 }
    }

    #[inline]
    fn fill(&mut self) {
        let mut shift = 64 - 8 - (self.count + 8);
        if self.data.len() - self.pos >= 8 {
            // Whole bytes at once: as many as the window has room for.
            let bits = (shift & !7) + 8;
            let be = u64::from_be_bytes(self.data[self.pos..self.pos + 8].try_into().unwrap());
            self.value |= (be >> (64 - bits)) << (shift & 7);
            self.count += bits;
            self.pos += (bits >> 3) as usize;
            return;
        }
        while shift >= 0 {
            let Some(&byte) = self.data.get(self.pos) else {
                self.count += PAST_END;
                break;
            };
            self.count += 8;
            self.value |= (byte as u64) << shift;
            self.pos += 1;
            shift -= 8;
        }
    }

    /// One boolean whose probability of being false is `prob` in 256.
    #[inline(always)]
    pub fn read(&mut self, prob: u8) -> bool {
        let split = (self.range * prob as u32 + (256 - prob as u32)) >> 8;
        if self.count < 0 {
            self.fill();
        }
        let bigsplit = (split as u64) << 56;
        let bit = self.value >= bigsplit;
        let range = if bit {
            self.value -= bigsplit;
            self.range - split
        } else {
            split
        };
        let shift = NORM[range as usize & 255];
        self.range = range << shift;
        self.value <<= shift;
        self.count -= shift as i32;
        bit
    }

    #[inline]
    pub fn bit(&mut self) -> bool {
        self.read(128)
    }

    pub fn literal(&mut self, bits: u32) -> u32 {
        let mut v = 0;
        for _ in 0..bits {
            v = (v << 1) | self.bit() as u32;
        }
        v
    }

    /// A symbol of a tree whose leaves are zero or negative, as libvpx's
    /// `vpx_read_tree`.
    #[inline]
    pub fn tree(&mut self, tree: &[i8], probs: &[u8]) -> usize {
        let mut i = 0usize;
        loop {
            let next = tree[i + self.read(probs[i >> 1]) as usize];
            if next <= 0 {
                return (-next) as usize;
            }
            i = next as usize;
        }
    }

    /// Whether more was read than the partition held: the stream is cut short
    /// or corrupt.
    pub fn overran(&self) -> bool {
        self.count > 64 && self.count < PAST_END
    }
}
