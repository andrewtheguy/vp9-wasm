//! The two readers of a frame: plain bits, for the uncompressed header, and
//! the boolean decoder everything after it is coded with.

use crate::error::{Error, Result};

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

/// The boolean decoder's registers, apart from the partition they read, so
/// that a loop may keep them in locals of its own.
#[derive(Clone, Copy)]
pub(crate) struct Window {
    value: u64,
    /// Bits in `value` below its top byte; negative when it needs a refill.
    count: i32,
    range: u32,
    /// The bytes taken, counting those past the partition's end, which are 0.
    pos: usize,
}

/// A partition, and its last bytes again with zeros after them: what a refill
/// near the end reads, so that every refill is one whole word.
pub(crate) struct Source<'a> {
    data: &'a [u8],
    tail: [u8; 24],
    /// Where in `data` the tail starts.
    tail_from: usize,
}

impl Window {
    #[inline(always)]
    fn refill(&mut self, src: &Source) {
        let shift = 48 - self.count;
        // Whole bytes: as many as the window has room for.
        let bits = (shift & !7) + 8;
        // SAFETY: eight bytes are in the data from `pos`, or in the tail from
        // no further than 16 into its 24.
        let word = unsafe {
            let from = if self.pos + 8 <= src.data.len() { src.data.as_ptr().add(self.pos) } else { src.tail.as_ptr().add((self.pos - src.tail_from).min(16)) };
            u64::from_be((from as *const u64).read_unaligned())
        };
        self.value |= (word >> (64 - bits)) << (shift & 7);
        self.count += bits;
        self.pos += (bits >> 3) as usize;
    }

    /// One boolean whose probability of being false is `prob` in 256.
    #[inline(always)]
    pub fn read(&mut self, src: &Source, prob: u8) -> bool {
        let split = (self.range * prob as u32 + (256 - prob as u32)) >> 8;
        if self.count < 0 {
            self.refill(src);
        }
        let bigsplit = (split as u64) << 56;
        let bit = self.value >= bigsplit;
        let range = if bit {
            self.value -= bigsplit;
            self.range - split
        } else {
            split
        };
        // A range is 1 to 255, and is brought back to 128 at least.
        let shift = range.leading_zeros() - 24;
        self.range = range << shift;
        self.value <<= shift;
        self.count -= shift as i32;
        bit
    }

    #[inline(always)]
    pub fn bit(&mut self, src: &Source) -> bool {
        self.read(src, 128)
    }
}

/// The boolean decoder: libvpx's, with a 64-bit window.
pub struct BoolDecoder<'a> {
    pub(crate) win: Window,
    pub(crate) src: Source<'a>,
}

impl<'a> BoolDecoder<'a> {
    /// A decoder over one partition, whose first bit is a marker that must be 0.
    pub fn new(data: &'a [u8]) -> Result<Self> {
        if data.is_empty() {
            return Err(Error::invalid("an empty partition"));
        }
        let mut d = BoolDecoder::over(data);
        if d.bit() {
            return Err(Error::invalid("a partition's marker bit is set"));
        }
        Ok(d)
    }

    /// A decoder of nothing, to stand where one will be.
    pub fn empty() -> Self {
        BoolDecoder::over(&[])
    }

    fn over(data: &'a [u8]) -> Self {
        let kept = data.len().min(8);
        let mut tail = [0; 24];
        tail[..kept].copy_from_slice(&data[data.len() - kept..]);
        BoolDecoder { win: Window { value: 0, count: -8, range: 255, pos: 0 }, src: Source { data, tail, tail_from: data.len() - kept } }
    }

    /// One boolean whose probability of being false is `prob` in 256.
    #[inline(always)]
    pub fn read(&mut self, prob: u8) -> bool {
        self.win.read(&self.src, prob)
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
    /// or corrupt. As libvpx has it, that is once a refill has asked for a
    /// byte past the end and the bits of those before it are used up.
    pub fn overran(&self) -> bool {
        let (w, len) = (&self.win, self.src.data.len());
        w.pos > len && (w.count as i64) < 8 * (w.pos - len) as i64
    }
}
