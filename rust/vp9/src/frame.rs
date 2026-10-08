//! A decoded frame: its three planes, and what each of its 8×8 blocks was
//! coded as, which the loop filter and the next frame's motion vectors read.

use crate::error::{Error, Result};
use crate::header::Colour;

/// A motion vector in eighths of a sample.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(C)]
pub struct Mv {
    pub row: i16,
    pub col: i16,
}

pub const INTRA_FRAME: u8 = 0;
pub const LAST_FRAME: u8 = 1;

pub const NEARESTMV: u8 = 10;
pub const NEARMV: u8 = 11;
pub const ZEROMV: u8 = 12;
pub const NEWMV: u8 = 13;

pub const BLOCK_8X8: u8 = 3;

/// One block's mode, copied to every 8×8 cell the block covers.
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct ModeInfo {
    /// libvpx's BLOCK_SIZE: 0 is 4×4, 12 is 64×64.
    pub sb_type: u8,
    /// The luma mode: 0..=9 intra, then nearest, near, zero, new. For a block
    /// under 8×8, its last sub-block's.
    pub mode: u8,
    pub uv_mode: u8,
    pub tx_size: u8,
    pub skip: bool,
    /// 0 intra, then last, golden, altref.
    pub ref_frame: u8,
    /// 3 for an intra block.
    pub interp_filter: u8,
    pub mv: Mv,
    /// A block under 8×8 has four of each; the one that applies is set.
    pub sub_mv: [Mv; 4],
    pub sub_mode: [u8; 4],
}

impl ModeInfo {
    #[inline]
    pub fn is_inter(&self) -> bool {
        self.ref_frame > INTRA_FRAME
    }

    /// Whether the block is the last frame's samples at its own place: a
    /// skipped block with no motion from the LAST reference, which on a
    /// screen is nearly every block. Its row started as those samples
    /// (`Recon::start_row`), so it costs nothing to make.
    #[inline]
    pub fn still(&self) -> bool {
        self.ref_frame == LAST_FRAME && self.skip && self.sub_mv == [Mv::default(); 4]
    }
}

/// One plane, at the frame's size rounded up to whole 64×64 blocks: the
/// decoder writes blocks whole.
pub struct Plane {
    pub data: Vec<u8>,
    pub stride: usize,
}

pub struct Frame {
    /// The size to show.
    pub width: usize,
    pub height: usize,
    pub colour: Colour,
    pub planes: [Plane; 3],
    pub(crate) mi: Vec<ModeInfo>,
    pub(crate) mi_cols: usize,
    pub(crate) mi_rows: usize,
}

impl Frame {
    /// The bytes a frame of this size takes: its planes and its blocks' modes.
    pub(crate) fn bytes(width: usize, height: usize) -> usize {
        3 * width.next_multiple_of(64) * height.next_multiple_of(64) + width.div_ceil(8) * height.div_ceil(8) * size_of::<ModeInfo>()
    }

    /// A frame of zeros, or an error where the memory has no room for it.
    pub(crate) fn new(width: usize, height: usize) -> Result<Frame> {
        fn zeroed<T: Clone>(len: usize, zero: T) -> Option<Vec<T>> {
            let mut v = Vec::new();
            v.try_reserve_exact(len).ok()?;
            v.resize(len, zero);
            Some(v)
        }
        let stride = width.next_multiple_of(64);
        let rows = height.next_multiple_of(64);
        let (mi_cols, mi_rows) = (width.div_ceil(8), height.div_ceil(8));
        let plane = || Some(Plane { data: zeroed(stride * rows, 0)?, stride });
        (|| Some(Frame {
            width,
            height,
            colour: Colour::default(),
            planes: [plane()?, plane()?, plane()?],
            mi: zeroed(mi_cols * mi_rows, ModeInfo::default())?,
            mi_cols,
            mi_rows,
        }))()
        .ok_or_else(|| Error::unsupported(format!("a {width}x{height} frame, which the memory has no room for")))
    }
}

// Block geometry, by libvpx's BLOCK_SIZE.
pub const B_WIDTH_LOG2: [u8; 13] = [0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4];
pub const B_HEIGHT_LOG2: [u8; 13] = [0, 1, 0, 1, 2, 1, 2, 3, 2, 3, 4, 3, 4];
pub const NUM_8X8_WIDE: [u8; 13] = [1, 1, 1, 1, 1, 2, 2, 2, 4, 4, 4, 8, 8];
pub const NUM_8X8_HIGH: [u8; 13] = [1, 1, 1, 1, 2, 1, 2, 4, 2, 4, 8, 4, 8];
pub const SIZE_GROUP: [u8; 13] = [0, 0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 3, 3];
pub const MAX_TX_SIZE: [u8; 13] = [0, 0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 3, 3];
/// What a block leaves in the partition contexts above it and to its left.
pub const PARTITION_CONTEXT: [[u8; 2]; 13] = [[15, 15], [15, 14], [14, 15], [14, 14], [14, 12], [12, 14], [12, 12], [12, 8], [8, 12], [8, 8], [8, 0], [0, 8], [0, 0]];
