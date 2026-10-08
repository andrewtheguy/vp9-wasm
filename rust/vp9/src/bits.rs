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
    /// The bits read ahead at the top, the top byte being the one the
    /// arithmetic compares, then a set bit marking their end, then zeros. The
    /// marker carries the count of them: it rises as they are used up, and the
    /// word's low half being zero is the sign that fewer than 32 remain.
    value: u64,
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
    /// Nothing read ahead: the marker at the top.
    const EMPTY: u64 = 1 << 63;

    /// The bits read ahead and not yet used, the top byte among them.
    #[inline(always)]
    fn held(&self) -> u32 {
        63 - self.value.trailing_zeros()
    }

    #[inline(always)]
    fn refill(&mut self, src: &Source) {
        // Below the marker there is room for whole bytes.
        let room = self.value.trailing_zeros();
        let bits = room & !7;
        // SAFETY: eight bytes are in the data from `pos`, or in the tail from
        // no further than 16 into its 24.
        let word = unsafe {
            let from = if self.pos + 8 <= src.data.len() { src.data.as_ptr().add(self.pos) } else { src.tail.as_ptr().add((self.pos - src.tail_from).min(16)) };
            u64::from_be((from as *const u64).read_unaligned())
        };
        // The marker gives way to the bytes, and is set again below them.
        self.value ^= 1 << room;
        self.value |= ((word >> (64 - bits)) << (room + 1 - bits)) | (1 << (room - bits));
        self.pos += (bits >> 3) as usize;
    }

    /// One boolean whose probability of being false is `prob` in 256.
    #[inline(always)]
    pub fn read(&mut self, src: &Source, prob: u8) -> bool {
        let split = (self.range * prob as u32 + (256 - prob as u32)) >> 8;
        if self.value as u32 == 0 {
            self.refill(src);
        }
        let bigsplit = (split as u64) << 56;
        // The marker and the zeros under it are below the split's bits, so
        // they never decide this.
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
        BoolDecoder { win: Window { value: Window::EMPTY, range: 255, pos: 0 }, src: Source { data, tail, tail_from: data.len() - kept } }
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
    /// or corrupt. As libvpx has it, that is once the bits used are more than
    /// the partition's less the eight of its last byte, which the arithmetic
    /// holds to the end.
    pub fn overran(&self) -> bool {
        let (w, len) = (&self.win, self.src.data.len());
        8 * w.pos - w.held() as usize + 8 > 8 * len
    }
}
