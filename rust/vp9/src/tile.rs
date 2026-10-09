//! Parsing a tile: its partitions, each block's mode and motion vectors, and
//! its coefficients, which are left with the modes for `recon` to make the
//! samples of. Transcribed from libvpx 1.16.0's vp9_decodeframe.c,
//! vp9_decodemv.c, vp9_detokenize.c, vp9_mvref_common.c and vp9_pred_common.c,
//! for 4:4:4 and single references.

use crate::bits::{BoolDecoder, Source, Window};
use crate::error::{Error, Result};
use crate::frame::*;
use crate::header::{FrameHeader, SEG_LVL_REF_FRAME, SEG_LVL_SKIP, SWITCHABLE, Segments, TX_MODE_SELECT};
use crate::probs::*;
use crate::tables::*;
use std::cell::UnsafeCell;

/// The coefficients of one tile's row of 64×64 blocks, in the order its
/// blocks are coded: parsed into, then reconstructed from.
#[derive(Default)]
pub(crate) struct RowBuf {
    /// Per block that is not skipped: how many of its transform blocks have
    /// coefficients, then for each of those in coding order its place (the
    /// plane, the row and the column in 4×4s, as `plane << 8 | row << 4 |
    /// col`), how many coefficients it has in scan order, and its one
    /// coefficient, or, for a 4×4, all sixteen in place, or how many of its
    /// coefficients are not zero and then, for each in scan order, where in
    /// the block it is and its value.
    pub coeffs: Vec<i16>,
}

pub(crate) struct RowCell(pub UnsafeCell<RowBuf>);

// SAFETY: a row is written by the thread that parses it and read only once
// that thread has said it is done.
unsafe impl Sync for RowCell {}

/// Everything of a frame its threads read or write. Tiles are columns of
/// whole 64×64 blocks, and each parses only its own columns of the mode grid
/// and the contexts above, so tiles parse side by side; a row of blocks is
/// reconstructed once it is parsed and the row above it is a block ahead.
pub(crate) struct FrameCtx<'a> {
    pub h: &'a FrameHeader,
    pub tx_mode: u8,
    pub probs: &'a Probs,
    /// Whether the frame's symbols are counted, for the probabilities to
    /// adapt to once it has decoded.
    pub counting: bool,
    /// The size shown, which references are extended from.
    pub width: usize,
    pub height: usize,
    pub mi_cols: usize,
    pub mi_rows: usize,
    pub stride: usize,
    pub cur: [*mut u8; 3],
    /// The three references' planes, at this frame's size and stride.
    pub refs: [[*const u8; 3]; 3],
    pub mi: *mut ModeInfo,
    /// The previous frame's modes, when its motion vectors are candidates.
    pub prev_mi: *const ModeInfo,
    pub seg: Segments,
    /// The segment of each 8×8 in the last frame coded in segments, and in
    /// this one: `mi_rows` rows of `mi_cols`.
    pub seg_last: *const u8,
    pub seg_cur: *mut u8,
    /// Per plane, whether each 4×4 column's last block above had coefficients.
    pub above_nz: [*mut u8; 3],
    pub above_part: *mut u8,
    /// [segment][luma, chroma][DC, AC]
    pub dequant: [[[i16; 2]; 2]; 8],
    /// Where each tile column starts, in 8×8 units, and the frame's width
    /// after the last.
    pub tile_starts: Vec<usize>,
    /// Per tile column, its rows of 64×64 blocks.
    pub rows: &'a [RowCell],
    pub sb_rows: usize,
}

// SAFETY: the pointers are to buffers that outlive the frame's decoding, and
// tiles touch disjoint columns of them; the last segment map is only read.
unsafe impl Sync for FrameCtx<'_> {}

/// The tree a block's segment is coded as: three booleans, the first with
/// the first probability, the second with one of the next two, the third
/// with one of the last four.
const SEGMENT_TREE: [i8; 14] = [2, 4, 6, 8, 10, 12, 0, -1, -2, -3, -4, -5, -6, -7];

/// The transform an intra mode's residual is coded with.
pub(crate) const INTRA_TX_TYPE: [u8; 10] = [0, 1, 2, 0, 3, 1, 2, 2, 1, 3];

type Scan = (&'static [u16], &'static [u16]);
/// [transform size][transform type]: the scan and each position's two
/// neighbours.
static SCANS: [[Scan; 4]; 4] = {
    const D4: Scan = (&DEFAULT_SCAN_4X4, &DEFAULT_SCAN_4X4_NEIGHBORS);
    const D8: Scan = (&DEFAULT_SCAN_8X8, &DEFAULT_SCAN_8X8_NEIGHBORS);
    const D16: Scan = (&DEFAULT_SCAN_16X16, &DEFAULT_SCAN_16X16_NEIGHBORS);
    const D32: Scan = (&DEFAULT_SCAN_32X32, &DEFAULT_SCAN_32X32_NEIGHBORS);
    [
        [D4, (&ROW_SCAN_4X4, &ROW_SCAN_4X4_NEIGHBORS), (&COL_SCAN_4X4, &COL_SCAN_4X4_NEIGHBORS), D4],
        [D8, (&ROW_SCAN_8X8, &ROW_SCAN_8X8_NEIGHBORS), (&COL_SCAN_8X8, &COL_SCAN_8X8_NEIGHBORS), D8],
        [D16, (&ROW_SCAN_16X16, &ROW_SCAN_16X16_NEIGHBORS), (&COL_SCAN_16X16, &COL_SCAN_16X16_NEIGHBORS), D16],
        [D32, D32, D32, D32],
    ]
};

/// Where a block looks for motion vector candidates, by block size: (row,
/// column) in 8×8 units.
static MV_REF_BLOCKS: [[(i8, i8); 8]; 13] = [
    [(-1, 0), (0, -1), (-1, -1), (-2, 0), (0, -2), (-2, -1), (-1, -2), (-2, -2)],
    [(-1, 0), (0, -1), (-1, -1), (-2, 0), (0, -2), (-2, -1), (-1, -2), (-2, -2)],
    [(-1, 0), (0, -1), (-1, -1), (-2, 0), (0, -2), (-2, -1), (-1, -2), (-2, -2)],
    [(-1, 0), (0, -1), (-1, -1), (-2, 0), (0, -2), (-2, -1), (-1, -2), (-2, -2)],
    [(0, -1), (-1, 0), (1, -1), (-1, -1), (0, -2), (-2, 0), (-2, -1), (-1, -2)],
    [(-1, 0), (0, -1), (-1, 1), (-1, -1), (-2, 0), (0, -2), (-1, -2), (-2, -1)],
    [(-1, 0), (0, -1), (-1, 1), (1, -1), (-1, -1), (-3, 0), (0, -3), (-3, -3)],
    [(0, -1), (-1, 0), (2, -1), (-1, -1), (-1, 1), (0, -3), (-3, 0), (-3, -3)],
    [(-1, 0), (0, -1), (-1, 2), (-1, -1), (1, -1), (-3, 0), (0, -3), (-3, -3)],
    [(-1, 1), (1, -1), (-1, 2), (2, -1), (-1, -1), (-3, 0), (0, -3), (-3, -3)],
    [(0, -1), (-1, 0), (4, -1), (-1, 2), (-1, -1), (0, -3), (-3, 0), (2, -1)],
    [(-1, 0), (0, -1), (-1, 4), (2, -1), (-1, -1), (-3, 0), (0, -3), (-1, 2)],
    [(-1, 3), (3, -1), (-1, 4), (4, -1), (-1, -1), (-1, 0), (0, -1), (-1, 6)],
];
/// What a neighbour's mode adds to the count the inter mode's context is of.
const MODE_2_COUNTER: [u8; 14] = [9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 0, 0, 3, 1];
const COUNTER_TO_CONTEXT: [u8; 19] = [2, 3, 4, 1, 3, 9, 0, 9, 9, 5, 5, 9, 5, 9, 9, 9, 9, 9, 6];
const IDX_N_COLUMN_TO_SUBBLOCK: [[usize; 2]; 4] = [[1, 2], [1, 3], [3, 2], [3, 3]];

const CAT1: [u8; 1] = [159];
const CAT2: [u8; 2] = [165, 145];
const CAT3: [u8; 3] = [173, 148, 140];
const CAT4: [u8; 4] = [176, 155, 140, 135];
const CAT5: [u8; 5] = [180, 157, 141, 134, 130];
const CAT6: [u8; 14] = [254, 254, 254, 252, 249, 243, 230, 196, 177, 153, 140, 133, 130, 129];

/// One tile column's parser, over the tile of each tile row in turn.
pub(crate) struct Tile<'a> {
    f: &'a FrameCtx<'a>,
    /// The column's tiles, one per tile row, and the row each starts at.
    data: Vec<(usize, &'a [u8])>,
    r: BoolDecoder<'a>,
    started: bool,
    pub counts: Box<Counts>,
    index: usize,
    col_start: usize,
    col_end: usize,
    left_nz: [[u8; 16]; 3],
    left_part: [u8; 8],
    /// Each coefficient's size class, for the context of those after it.
    token_cache: Box<[u8; 1024]>,
    /// The row being parsed.
    buf: *mut RowBuf,

    // The block being decoded.
    mi_row: usize,
    mi_col: usize,
    have_above: bool,
    have_left: bool,
    above: ModeInfo,
    left: ModeInfo,
    /// The block's distance to each edge of the frame, in eighths of a sample.
    to_left: i32,
    to_right: i32,
    to_top: i32,
    to_bottom: i32,
    /// log2 of a block under 8×8's sub-blocks across and down, less one.
    sub_wl: u8,
    sub_hl: u8,
}

// SAFETY: the row a tile points at is its own until it says it is parsed.
unsafe impl Send for Tile<'_> {}

#[inline]
fn clamp_mv(mv: Mv, min_col: i32, max_col: i32, min_row: i32, max_row: i32) -> Mv {
    Mv { row: (mv.row as i32).clamp(min_row, max_row) as i16, col: (mv.col as i32).clamp(min_col, max_col) as i16 }
}

#[inline]
fn use_hp(mv: Mv) -> bool {
    (mv.row as i32).abs() < 64 && (mv.col as i32).abs() < 64
}

#[inline]
fn lower_precision(mv: &mut Mv, allow_hp: bool) {
    if !(allow_hp && use_hp(*mv)) {
        if mv.row & 1 != 0 {
            mv.row += if mv.row > 0 { -1 } else { 1 };
        }
        if mv.col & 1 != 0 {
            mv.col += if mv.col > 0 { -1 } else { 1 };
        }
    }
}

impl<'a> Tile<'a> {
    /// The parser of tile column `index`, whose tile of each tile row is in
    /// `data` with the row it starts at.
    pub fn new(f: &'a FrameCtx<'a>, index: usize, data: Vec<(usize, &'a [u8])>) -> Self {
        Tile {
            f,
            data,
            r: BoolDecoder::empty(),
            started: false,
            counts: Box::default(),
            index,
            col_start: f.tile_starts[index],
            col_end: f.tile_starts[index + 1],
            left_nz: [[0; 16]; 3],
            left_part: [0; 8],
            token_cache: Box::new([0; 1024]),
            buf: std::ptr::null_mut(),
            mi_row: 0,
            mi_col: 0,
            have_above: false,
            have_left: false,
            above: ModeInfo::default(),
            left: ModeInfo::default(),
            to_left: 0,
            to_right: 0,
            to_top: 0,
            to_bottom: 0,
            sub_wl: 0,
            sub_hl: 0,
        }
    }

    fn overran(&self) -> Result<()> {
        if self.started && self.r.overran() { Err(Error::invalid("a tile is cut short")) } else { Ok(()) }
    }

    /// Parses the tile's row of 64×64 blocks `sb_row`, the rows before it
    /// having been parsed.
    pub fn parse_row(&mut self, sb_row: usize) -> Result<()> {
        let mi_row = sb_row * 8;
        // Every tile that starts here, in turn: a frame of fewer rows than tile
        // rows has tiles of no rows, each a partition all the same, and the
        // last of them is the one this row is in.
        for &(_, data) in self.data.iter().filter(|(start, _)| *start == mi_row) {
            self.overran()?;
            self.r = BoolDecoder::new(data)?;
            self.started = true;
        }
        self.buf = self.f.rows[self.index * self.f.sb_rows + sb_row].0.get();
        // SAFETY: this thread alone has the row until it says it is parsed.
        unsafe {
            (*self.buf).coeffs.clear();
        }
        self.left_nz = [[0; 16]; 3];
        self.left_part = [0; 8];
        for mi_col in (self.col_start..self.col_end).step_by(8) {
            self.partition(mi_row, mi_col, 4)?;
            // Once overrun a tile stays so, and what it reads is zeros: there
            // is no more to parse of it.
            self.overran()?;
        }
        if sb_row + 1 == self.f.sb_rows {
            self.overran()?;
        }
        Ok(())
    }

    /// The block of `1 << n4x4_l2` 4×4s a side at this position, and whatever
    /// it is split into.
    fn partition(&mut self, mi_row: usize, mi_col: usize, n4x4_l2: u32) -> Result<()> {
        let f = self.f;
        if mi_row >= f.mi_rows || mi_col >= f.mi_cols {
            return Ok(());
        }
        let bsl = n4x4_l2 - 1;
        let num_8x8 = 1usize << bsl;
        let hbs = num_8x8 >> 1;
        let has_rows = mi_row + hbs < f.mi_rows;
        let has_cols = mi_col + hbs < f.mi_cols;
        let bsize = 3 + 3 * bsl as u8;

        // SAFETY: mi_col is inside the frame, and the context above is as long
        // as the frame's width rounded up to whole 64×64 blocks.
        let above_part = unsafe { std::slice::from_raw_parts_mut(f.above_part.add(mi_col), num_8x8) };
        let above = (above_part[0] >> bsl) & 1;
        let left = (self.left_part[mi_row & 7] >> bsl) & 1;
        let ctx = (left * 2 + above) as usize + bsl as usize * 4;
        let probs = if f.h.intra() { &KF_PARTITION_PROBS[ctx] } else { &f.probs.partition[ctx] };
        let p = if has_rows && has_cols {
            self.r.tree(&PARTITION_TREE, probs)
        } else if !has_rows && has_cols {
            if self.r.read(probs[1]) { 3 } else { 1 }
        } else if has_rows && !has_cols {
            if self.r.read(probs[2]) { 3 } else { 2 }
        } else {
            3
        };
        self.counts.partition[ctx][p] += 1;
        let subsize = bsize - p as u8;

        if hbs == 0 {
            self.sub_wl = (p & 2 == 0) as u8;
            self.sub_hl = (p & 1 == 0) as u8;
            self.block(mi_row, mi_col, subsize, 1, 1)?;
        } else {
            match p {
                0 => self.block(mi_row, mi_col, subsize, n4x4_l2, n4x4_l2)?,
                1 => {
                    self.block(mi_row, mi_col, subsize, n4x4_l2, bsl)?;
                    if has_rows {
                        self.block(mi_row + hbs, mi_col, subsize, n4x4_l2, bsl)?;
                    }
                }
                2 => {
                    self.block(mi_row, mi_col, subsize, bsl, n4x4_l2)?;
                    if has_cols {
                        self.block(mi_row, mi_col + hbs, subsize, bsl, n4x4_l2)?;
                    }
                }
                _ => {
                    self.partition(mi_row, mi_col, bsl)?;
                    self.partition(mi_row, mi_col + hbs, bsl)?;
                    self.partition(mi_row + hbs, mi_col, bsl)?;
                    self.partition(mi_row + hbs, mi_col + hbs, bsl)?;
                }
            }
        }

        if bsize == BLOCK_8X8 || p != 3 {
            above_part.fill(PARTITION_CONTEXT[subsize as usize][0]);
            self.left_part[mi_row & 7..(mi_row & 7) + num_8x8].fill(PARTITION_CONTEXT[subsize as usize][1]);
        }
        Ok(())
    }

    /// One block of `1 << bwl` by `1 << bhl` 4×4s: its mode, then its samples.
    fn block(&mut self, mi_row: usize, mi_col: usize, bsize: u8, bwl: u32, bhl: u32) -> Result<()> {
        let f = self.f;
        let (bw, bh) = (1usize << (bwl - 1), 1usize << (bhl - 1));
        let x_mis = bw.min(f.mi_cols - mi_col);
        let y_mis = bh.min(f.mi_rows - mi_row);
        self.mi_row = mi_row;
        self.mi_col = mi_col;
        self.to_top = -((mi_row * 64) as i32);
        self.to_bottom = (f.mi_rows as i32 - bh as i32 - mi_row as i32) * 64;
        self.to_left = -((mi_col * 64) as i32);
        self.to_right = (f.mi_cols as i32 - bw as i32 - mi_col as i32) * 64;
        self.have_above = mi_row > 0;
        self.have_left = mi_col > self.col_start;
        // SAFETY: both neighbours are inside the frame when they are had.
        unsafe {
            let at = f.mi.add(mi_row * f.mi_cols + mi_col);
            if self.have_above {
                self.above = *at.sub(f.mi_cols);
            }
            if self.have_left {
                self.left = *at.sub(1);
            }
        }

        let mut mi = ModeInfo { sb_type: bsize, ..ModeInfo::default() };
        if f.h.intra() {
            mi.segment_id = self.intra_segment_id(x_mis, y_mis);
            self.intra_frame_mode_info(&mut mi);
        } else {
            self.inter_segment_id(&mut mi, x_mis, y_mis);
            self.inter_frame_mode_info(&mut mi)?;
        }

        let (n4_w, n4_h) = (bw * 2, bh * 2);
        if mi.skip {
            let x = mi_col * 2;
            let y = (mi_row * 2) & 15;
            for plane in 0..3 {
                // SAFETY: a block's columns are inside the frame's width
                // rounded up to whole 64×64 blocks.
                unsafe { std::slice::from_raw_parts_mut(f.above_nz[plane].add(x), n4_w) }.fill(0);
                self.left_nz[plane][y..y + n4_h].fill(0);
            }
        } else {
            // The 4×4s of the block inside the frame, across and down.
            let max_w = if self.to_right >= 0 { n4_w } else { (n4_w as i32 + (self.to_right >> 5)) as usize };
            let max_h = if self.to_bottom >= 0 { n4_h } else { (n4_h as i32 + (self.to_bottom >> 5)) as usize };
            let inter = mi.is_inter();
            // SAFETY: this thread alone has the row until it says it is parsed.
            let buf = unsafe { &mut *self.buf };
            // Room for all the block can have, asked for here, where it can
            // be refused: the buffers do not grow as they are parsed into.
            if buf.coeffs.try_reserve(n4_w * n4_h * 35 * 3 + 1).is_err() {
                return Err(Error::unsupported("more coefficients than the memory has room for"));
            }
            let first = buf.coeffs.len();
            let eobtotal = match (f.counting, mi.tx_size & 3) {
                (false, 0) => self.tokens::<false, 0>(&mi, max_w, max_h),
                (false, 1) => self.tokens::<false, 1>(&mi, max_w, max_h),
                (false, 2) => self.tokens::<false, 2>(&mi, max_w, max_h),
                (false, _) => self.tokens::<false, 3>(&mi, max_w, max_h),
                (true, 0) => self.tokens::<true, 0>(&mi, max_w, max_h),
                (true, 1) => self.tokens::<true, 1>(&mi, max_w, max_h),
                (true, 2) => self.tokens::<true, 2>(&mi, max_w, max_h),
                (true, _) => self.tokens::<true, 3>(&mi, max_w, max_h),
            };
            // What the loop filter and the blocks after this one see: an inter
            // block with no coefficients is a skipped one.
            if inter && bsize >= BLOCK_8X8 && eobtotal == 0 {
                mi.skip = true;
                // SAFETY: as above.
                unsafe { (*self.buf).coeffs.truncate(first) };
            }
        }

        // SAFETY: x_mis and y_mis keep the block's cells inside the frame.
        unsafe {
            let at = f.mi.add(mi_row * f.mi_cols + mi_col);
            for y in 0..y_mis {
                std::slice::from_raw_parts_mut(at.add(y * f.mi_cols), x_mis).fill(mi);
            }
        }
        Ok(())
    }

    /// The block's segment, coded as the tree.
    fn read_segment_id(&mut self) -> u8 {
        self.r.tree(&SEGMENT_TREE, &self.f.seg.tree_probs) as u8
    }

    /// The segment map's cells of the block, `x_mis` by `y_mis` inside the
    /// frame, at `at`.
    #[inline]
    fn segment_cells(&self, at: *const u8, x_mis: usize, y_mis: usize) -> impl Iterator<Item = *const u8> {
        let (mi_cols, offset) = (self.f.mi_cols, self.mi_row * self.f.mi_cols + self.mi_col);
        // SAFETY: the block's cells are inside the frame, which the maps are
        // the size of.
        (0..y_mis).flat_map(move |y| (0..x_mis).map(move |x| unsafe { at.add(offset + y * mi_cols + x) }))
    }

    /// Writes the block's segment into this frame's map.
    fn set_segment_id(&mut self, x_mis: usize, y_mis: usize, segment_id: u8) {
        for cell in self.segment_cells(self.f.seg_cur, x_mis, y_mis) {
            // SAFETY: inside the map, and in this tile's columns.
            unsafe { *cell.cast_mut() = segment_id };
        }
    }

    /// Carries the block's cells of the last map into this frame's.
    fn copy_segment_id(&mut self, x_mis: usize, y_mis: usize) {
        let (last, cur) = (self.f.seg_last, self.f.seg_cur);
        for (from, to) in self.segment_cells(last, x_mis, y_mis).zip(self.segment_cells(cur, x_mis, y_mis)) {
            // SAFETY: inside the maps, and in this tile's columns.
            unsafe { *to.cast_mut() = *from };
        }
    }

    /// The segment the last map gives the block: the lowest of its cells'.
    fn predicted_segment_id(&self, x_mis: usize, y_mis: usize) -> u8 {
        // SAFETY: inside the map.
        self.segment_cells(self.f.seg_last, x_mis, y_mis).map(|cell| unsafe { *cell }).min().unwrap_or(0)
    }

    /// A block's segment in a frame of only intra blocks, as
    /// `read_intra_segment_id`: coded, or 0 while the map is carried.
    fn intra_segment_id(&mut self, x_mis: usize, y_mis: usize) -> u8 {
        let seg = self.f.seg;
        if !seg.enabled {
            return 0;
        }
        if !seg.update_map {
            self.copy_segment_id(x_mis, y_mis);
            return 0;
        }
        let segment_id = self.read_segment_id();
        self.set_segment_id(x_mis, y_mis, segment_id);
        segment_id
    }

    /// A block's segment in an inter frame, as `read_inter_segment_id`: the
    /// last map's where the map is carried, or where the block says it is
    /// the last map's, and coded otherwise.
    fn inter_segment_id(&mut self, mi: &mut ModeInfo, x_mis: usize, y_mis: usize) {
        let seg = self.f.seg;
        if !seg.enabled {
            return;
        }
        let predicted = self.predicted_segment_id(x_mis, y_mis);
        if !seg.update_map {
            self.copy_segment_id(x_mis, y_mis);
            mi.segment_id = predicted;
            return;
        }
        let segment_id = if seg.temporal_update {
            let ctx = (self.have_above && self.above.seg_id_predicted) as usize + (self.have_left && self.left.seg_id_predicted) as usize;
            mi.seg_id_predicted = self.r.read(seg.pred_probs[ctx]);
            if mi.seg_id_predicted { predicted } else { self.read_segment_id() }
        } else {
            self.read_segment_id()
        };
        self.set_segment_id(x_mis, y_mis, segment_id);
        mi.segment_id = segment_id;
    }

    /// Whether the block has no residual: its segment's say, or coded.
    fn skip_flag(&mut self, segment_id: u8) -> bool {
        if self.f.seg.feature(segment_id, SEG_LVL_SKIP).is_some() {
            return true;
        }
        let ctx = (self.have_above && self.above.skip) as usize + (self.have_left && self.left.skip) as usize;
        let skip = self.r.read(self.f.probs.skip[ctx]);
        self.counts.skip[ctx][skip as usize] += 1;
        skip
    }

    fn tx_size(&mut self, bsize: u8, allow_select: bool) -> u8 {
        let max = MAX_TX_SIZE[bsize as usize];
        if !(allow_select && self.f.tx_mode == TX_MODE_SELECT && bsize >= BLOCK_8X8) {
            return max.min([0, 1, 2, 3, 3][self.f.tx_mode as usize]);
        }
        let mut above = if self.have_above && !self.above.skip { self.above.tx_size } else { max };
        let mut left = if self.have_left && !self.left.skip { self.left.tx_size } else { max };
        if !self.have_left {
            left = above;
        }
        if !self.have_above {
            above = left;
        }
        let ctx = (above + left > max) as usize;
        let p = &self.f.probs;
        let probs: &[u8] = match max {
            1 => &p.tx8[ctx],
            2 => &p.tx16[ctx],
            _ => &p.tx32[ctx],
        };
        let mut tx = self.r.read(probs[0]) as u8;
        if tx != 0 && max >= 2 {
            tx += self.r.read(probs[1]) as u8;
            if tx != 1 && max >= 3 {
                tx += self.r.read(probs[2]) as u8;
            }
        }
        match max {
            1 => self.counts.tx8[ctx][tx as usize] += 1,
            2 => self.counts.tx16[ctx][tx as usize] += 1,
            _ => self.counts.tx32[ctx][tx as usize] += 1,
        }
        tx
    }

    /// The modes of a block of a frame that has only intra blocks, each coded
    /// against the modes above and to the left of it.
    fn intra_frame_mode_info(&mut self, mi: &mut ModeInfo) {
        mi.skip = self.skip_flag(mi.segment_id);
        mi.tx_size = self.tx_size(mi.sb_type, true);
        mi.ref_frame = INTRA_FRAME;
        mi.interp_filter = 3;
        let above = if self.have_above { self.above.sub_mode } else { [0; 4] };
        let left = if self.have_left { self.left.sub_mode } else { [0; 4] };
        let read = |t: &mut Self, sub: &[u8; 4], b: usize| {
            let a = if b < 2 { above[b + 2] } else { sub[b - 2] };
            let l = if b & 1 == 0 { left[b + 1] } else { sub[b - 1] };
            t.r.tree(&INTRA_MODE_TREE, &KF_Y_MODE_PROBS[a as usize][l as usize]) as u8
        };
        let mut sub = [0u8; 4];
        match mi.sb_type {
            0 => {
                for b in 0..4 {
                    sub[b] = read(self, &sub, b);
                }
            }
            1 => {
                sub[0] = read(self, &sub, 0);
                sub[2] = sub[0];
                sub[1] = read(self, &sub, 1);
                sub[3] = sub[1];
            }
            2 => {
                sub[0] = read(self, &sub, 0);
                sub[1] = sub[0];
                sub[2] = read(self, &sub, 2);
                sub[3] = sub[2];
            }
            _ => sub = [read(self, &sub, 0); 4],
        }
        mi.sub_mode = sub;
        mi.mode = sub[3];
        mi.uv_mode = self.r.tree(&INTRA_MODE_TREE, &KF_UV_MODE_PROBS[mi.mode as usize]) as u8;
    }

    fn intra_mode_y(&mut self, group: usize) -> u8 {
        let mode = self.r.tree(&INTRA_MODE_TREE, &self.f.probs.y_mode[group]);
        self.counts.y_mode[group][mode] += 1;
        mode as u8
    }

    fn inter_frame_mode_info(&mut self, mi: &mut ModeInfo) -> Result<()> {
        mi.skip = self.skip_flag(mi.segment_id);
        // Whether the block is predicted from a reference: its segment's say,
        // or coded.
        let inter = match self.f.seg.feature(mi.segment_id, SEG_LVL_REF_FRAME) {
            Some(reference) => reference != INTRA_FRAME as i16,
            None => {
                let ctx = match (self.have_above, self.have_left) {
                    (true, true) => {
                        let (a, l) = (!self.above.is_inter(), !self.left.is_inter());
                        if a && l { 3 } else { (a || l) as usize }
                    }
                    (true, false) => 2 * !self.above.is_inter() as usize,
                    (false, true) => 2 * !self.left.is_inter() as usize,
                    (false, false) => 0,
                };
                let inter = self.r.read(self.f.probs.intra_inter[ctx]);
                self.counts.intra_inter[ctx][inter as usize] += 1;
                inter
            }
        };
        mi.tx_size = self.tx_size(mi.sb_type, !mi.skip || !inter);
        if inter {
            return self.inter_block_mode_info(mi);
        }

        let mut sub = [0u8; 4];
        match mi.sb_type {
            0 => {
                for b in &mut sub {
                    *b = self.intra_mode_y(0);
                }
            }
            1 => {
                sub[0] = self.intra_mode_y(0);
                sub[2] = sub[0];
                sub[1] = self.intra_mode_y(0);
                sub[3] = sub[1];
            }
            2 => {
                sub[0] = self.intra_mode_y(0);
                sub[1] = sub[0];
                sub[2] = self.intra_mode_y(0);
                sub[3] = sub[2];
            }
            b => sub = [self.intra_mode_y(SIZE_GROUP[b as usize] as usize); 4],
        }
        mi.sub_mode = sub;
        mi.mode = sub[3];
        let uv = self.r.tree(&INTRA_MODE_TREE, &self.f.probs.uv_mode[mi.mode as usize]);
        self.counts.uv_mode[mi.mode as usize][uv] += 1;
        mi.uv_mode = uv as u8;
        mi.interp_filter = 3;
        mi.ref_frame = INTRA_FRAME;
        Ok(())
    }

    /// Which reference: last, golden or altref.
    fn ref_frame(&mut self) -> u8 {
        let (a, l) = (self.above.ref_frame, self.left.ref_frame);
        let ctx0 = match (self.have_above, self.have_left) {
            (true, true) => match (a == 0, l == 0) {
                (true, true) => 2,
                (true, false) => 4 * (l == 1) as usize,
                (false, true) => 4 * (a == 1) as usize,
                (false, false) => 2 * (a == 1) as usize + 2 * (l == 1) as usize,
            },
            (true, false) => if a == 0 { 2 } else { 4 * (a == 1) as usize },
            (false, true) => if l == 0 { 2 } else { 4 * (l == 1) as usize },
            (false, false) => 2,
        };
        let bit0 = self.r.read(self.f.probs.single_ref[ctx0][0]);
        self.counts.single_ref[ctx0][0][bit0 as usize] += 1;
        if !bit0 {
            return 1;
        }
        let one = |e: u8| if e == 0 || e == 1 { 2 } else { 4 * (e == 2) as usize };
        let ctx1 = match (self.have_above, self.have_left) {
            (true, true) => match (a == 0, l == 0) {
                (true, true) => 2,
                (true, false) => if l == 1 { 3 } else { 4 * (l == 2) as usize },
                (false, true) => if a == 1 { 3 } else { 4 * (a == 2) as usize },
                (false, false) => {
                    if a == 1 && l == 1 {
                        3
                    } else if a == 1 || l == 1 {
                        4 * ((if a == 1 { l } else { a }) == 2) as usize
                    } else {
                        2 * (a == 2) as usize + 2 * (l == 2) as usize
                    }
                }
            },
            (true, false) => one(a),
            (false, true) => one(l),
            (false, false) => 2,
        };
        let bit1 = self.r.read(self.f.probs.single_ref[ctx1][1]);
        self.counts.single_ref[ctx1][1][bit1 as usize] += 1;
        if bit1 { 3 } else { 2 }
    }

    fn inter_mode(&mut self, ctx: usize) -> u8 {
        let mode = self.r.tree(&INTER_MODE_TREE, &self.f.probs.inter_mode[ctx]);
        self.counts.inter_mode[ctx][mode] += 1;
        NEARESTMV + mode as u8
    }

    /// The cell `pos` away, when it is in the frame and in this tile.
    #[inline]
    fn candidate(&self, pos: (i8, i8)) -> Option<&ModeInfo> {
        let f = self.f;
        let row = self.mi_row as isize + pos.0 as isize;
        let col = self.mi_col as isize + pos.1 as isize;
        if row < 0 || col < self.col_start as isize || row >= f.mi_rows as isize || col >= self.col_end as isize {
            return None;
        }
        // SAFETY: just checked to be inside the frame.
        Some(unsafe { &*f.mi.add(row as usize * f.mi_cols + col as usize) })
    }

    /// The two motion vector candidates for `ref_frame`, the nearest first:
    /// from the blocks around, then the previous frame's block here, then the
    /// same again for the other references. `block` is the sub-block of a block
    /// under 8×8, or -1.
    #[allow(unused_assignments)]
    fn find_mv_refs(&self, bsize: u8, ref_frame: u8, block: i32) -> [Mv; 2] {
        let f = self.f;
        let search = &MV_REF_BLOCKS[bsize as usize];
        let bias = &f.h.sign_bias;
        let mut list = [Mv::default(); 2];
        let mut count = 0;
        let mut different_ref_found = false;
        // SAFETY: the block is inside the frame, which the previous frame's
        // modes are the size of.
        let prev = if f.prev_mi.is_null() { None } else { Some(unsafe { &*f.prev_mi.add(self.mi_row * f.mi_cols + self.mi_col) }) };

        'done: {
            macro_rules! add {
                ($mv:expr) => {{
                    let mv: Mv = $mv;
                    if count != 0 {
                        if mv != list[0] {
                            list[1] = mv;
                            break 'done;
                        }
                    } else {
                        list[0] = mv;
                        count = 1;
                    }
                }};
            }
            for (i, &pos) in search.iter().enumerate() {
                if let Some(c) = self.candidate(pos) {
                    different_ref_found = true;
                    if c.ref_frame == ref_frame {
                        // The two nearest give the sub-block beside this one.
                        if i < 2 && block >= 0 {
                            add!(c.sub_mv[IDX_N_COLUMN_TO_SUBBLOCK[block as usize][(pos.1 == 0) as usize]]);
                        } else {
                            add!(c.mv);
                        }
                    }
                }
            }
            if let Some(p) = prev {
                if p.ref_frame == ref_frame {
                    add!(p.mv);
                }
            }
            if different_ref_found {
                for &pos in search {
                    if let Some(c) = self.candidate(pos) {
                        if c.is_inter() && c.ref_frame != ref_frame {
                            let mut mv = c.mv;
                            if bias[c.ref_frame as usize] != bias[ref_frame as usize] {
                                mv = Mv { row: mv.row.wrapping_neg(), col: mv.col.wrapping_neg() };
                            }
                            add!(mv);
                        }
                    }
                }
            }
            if let Some(p) = prev {
                if p.ref_frame != ref_frame && p.is_inter() {
                    let mut mv = p.mv;
                    if bias[p.ref_frame as usize] != bias[ref_frame as usize] {
                        mv = Mv { row: mv.row.wrapping_neg(), col: mv.col.wrapping_neg() };
                    }
                    add!(mv);
                }
            }
        }
        list.map(|mv| clamp_mv(mv, self.to_left - 128, self.to_right + 128, self.to_top - 128, self.to_bottom + 128))
    }

    fn mv_component(&mut self, comp: usize, usehp: bool) -> i32 {
        let p = &self.f.probs.mv[comp];
        let sign = self.r.read(p.sign);
        let class = self.r.tree(&MV_CLASS_TREE, &p.classes);
        let (d, mut mag) = if class == 0 {
            (self.r.read(p.class0[0]) as i32, 0)
        } else {
            let mut d = 0;
            for i in 0..class {
                d |= (self.r.read(p.bits[i]) as i32) << i;
            }
            (d, 2 << (class + 2))
        };
        let fr = self.r.tree(&MV_FP_TREE, if class == 0 { &p.class0_fp[d as usize] } else { &p.fp }) as i32;
        let hp = if usehp { self.r.read(if class == 0 { p.class0_hp } else { p.hp }) as i32 } else { 1 };
        mag += ((d << 3) | (fr << 1) | hp) + 1;

        // The counts, as `vp9_inc_mv` has them from the value.
        let c = &mut self.counts.mv[comp];
        c.sign[sign as usize] += 1;
        c.classes[class] += 1;
        if class == 0 {
            c.class0[d as usize] += 1;
            c.class0_fp[d as usize][fr as usize] += 1;
            c.class0_hp[hp as usize] += 1;
        } else {
            for i in 0..class {
                c.bits[i][((d >> i) & 1) as usize] += 1;
            }
            c.fp[fr as usize] += 1;
            c.hp[hp as usize] += 1;
        }
        if sign { -mag } else { mag }
    }

    /// A coded motion vector: its difference from `best`.
    fn read_mv(&mut self, best: Mv) -> Result<Mv> {
        let joint = self.r.tree(&MV_JOINT_TREE, &self.f.probs.mv_joints);
        self.counts.mv_joints[joint] += 1;
        let usehp = self.f.h.allow_hp && use_hp(best);
        let (mut row, mut col) = (best.row as i32, best.col as i32);
        if joint == 2 || joint == 3 {
            row += self.mv_component(0, usehp);
        }
        if joint == 1 || joint == 3 {
            col += self.mv_component(1, usehp);
        }
        if row <= -(1 << 14) || row >= (1 << 14) - 1 || col <= -(1 << 14) || col >= (1 << 14) - 1 {
            return Err(Error::invalid("a motion vector is out of range"));
        }
        Ok(Mv { row: row as i16, col: col as i16 })
    }

    fn inter_block_mode_info(&mut self, mi: &mut ModeInfo) -> Result<()> {
        let bsize = mi.sb_type;
        let allow_hp = self.f.h.allow_hp;
        mi.ref_frame = match self.f.seg.feature(mi.segment_id, SEG_LVL_REF_FRAME) {
            Some(reference) => reference as u8,
            None => self.ref_frame(),
        };
        let search = &MV_REF_BLOCKS[bsize as usize];
        let mut counter = 0;
        for &pos in &search[..2] {
            if let Some(c) = self.candidate(pos) {
                counter += MODE_2_COUNTER[c.mode as usize];
            }
        }
        let mode_ctx = COUNTER_TO_CONTEXT[counter as usize] as usize;
        if self.f.seg.feature(mi.segment_id, SEG_LVL_SKIP).is_some() {
            // A skipped segment's block is the reference's samples at its
            // own place, and is 8×8 at least: libvpx refuses a smaller one.
            if bsize < BLOCK_8X8 {
                return Err(Error::invalid("a block under 8×8 in a segment that skips"));
            }
            mi.mode = ZEROMV;
        } else if bsize >= BLOCK_8X8 {
            if mode_ctx > 6 {
                return Err(Error::invalid("an inter mode's context is impossible"));
            }
            mi.mode = self.inter_mode(mode_ctx);
        }

        mi.interp_filter = if self.f.h.interp_filter == SWITCHABLE {
            let left = if self.have_left { self.left.interp_filter } else { 3 };
            let above = if self.have_above { self.above.interp_filter } else { 3 };
            let ctx = if left == above {
                left
            } else if left == 3 {
                above
            } else if above == 3 {
                left
            } else {
                3
            } as usize;
            let filter = self.r.tree(&INTERP_FILTER_TREE, &self.f.probs.interp_filter[ctx]);
            self.counts.interp_filter[ctx][filter] += 1;
            filter as u8
        } else {
            self.f.h.interp_filter
        };

        if bsize >= BLOCK_8X8 {
            mi.mv = match mi.mode {
                ZEROMV => Mv::default(),
                mode => {
                    let list = self.find_mv_refs(bsize, mi.ref_frame, -1);
                    let mut best = list[(mode == NEARMV) as usize];
                    lower_precision(&mut best, allow_hp);
                    if mode == NEWMV { self.read_mv(best)? } else { best }
                }
            };
            mi.sub_mv = [mi.mv; 4];
            return Ok(());
        }

        if mode_ctx > 6 {
            return Err(Error::invalid("an inter mode's context is impossible"));
        }
        let (num_w, num_h) = (1usize << self.sub_wl, 1usize << self.sub_hl);
        let mut best_new = None;
        let mut mode = ZEROMV;
        for idy in (0..2).step_by(num_h) {
            for idx in (0..2).step_by(num_w) {
                let j = idy * 2 + idx;
                mode = self.inter_mode(mode_ctx);
                let mv = match mode {
                    NEARESTMV | NEARMV => {
                        let near = mode == NEARMV;
                        let b = &mi.sub_mv;
                        match j {
                            0 => self.find_mv_refs(bsize, mi.ref_frame, 0)[near as usize],
                            1 | 2 if !near => b[0],
                            1 | 2 => self.find_mv_refs(bsize, mi.ref_frame, j as i32).into_iter().find(|&mv| mv != b[0]).unwrap_or_default(),
                            _ if !near => b[2],
                            _ => {
                                let list = self.find_mv_refs(bsize, mi.ref_frame, 3);
                                [b[1], b[0], list[0], list[1]].into_iter().find(|&mv| mv != b[2]).unwrap_or_default()
                            }
                        }
                    }
                    NEWMV => {
                        let best = *best_new.get_or_insert_with(|| {
                            let mut best = self.find_mv_refs(bsize, mi.ref_frame, -1)[0];
                            lower_precision(&mut best, allow_hp);
                            best
                        });
                        self.read_mv(best)?
                    }
                    _ => Mv::default(),
                };
                mi.sub_mv[j] = mv;
                if num_h == 2 {
                    mi.sub_mv[j + 2] = mv;
                }
                if num_w == 2 {
                    mi.sub_mv[j + 1] = mv;
                }
            }
        }
        mi.mode = mode;
        mi.mv = mi.sub_mv[3];
        Ok(())
    }

    /// The coefficients of a block's transform blocks, dequantized and left
    /// in the row's buffer, which has room for them: how many there are in
    /// scan order, over them all. `max_w` and `max_h` are the 4×4s of the
    /// block inside the frame. The decoder's registers are locals here, and a
    /// transform block with no coefficients, which most are, is one boolean.
    /// `TX` is the block's transform size, so that the widths that hang on it
    /// are constants of the code.
    #[inline(never)]
    fn tokens<const COUNT: bool, const TX: usize>(&mut self, mi: &ModeInfo, max_w: usize, max_h: usize) -> usize {
        let f = self.f;
        let tx = TX;
        let n = 1usize << tx;
        let inter = mi.is_inter();
        let intra_luma = !inter && !f.h.lossless;
        // Where the contexts stop being written, for a block the frame ends in.
        let inside = self.to_right >= 0 && self.to_bottom >= 0;
        let (x0, y0) = (self.mi_col * 2, (self.mi_row * 2) & 15);
        let src = &self.r.src;
        let mut win = self.r.win;
        let mut total = 0;
        // SAFETY: this thread alone has the row until it says it is parsed,
        // and `block` has made room in it for what this block can have. A
        // block's columns are inside the frame's width rounded up to whole
        // 64×64 blocks, which the contexts above are as long as, and its rows
        // inside the 16 of the contexts to the left.
        unsafe {
            let buf = &mut *self.buf;
            let coeffs = buf.coeffs.as_mut_ptr();
            // The count of the transform blocks with coefficients goes first,
            // once they are counted.
            let header = buf.coeffs.len();
            let (mut coded, mut at) = (0, header + 1);
            for plane in 0..3 {
                let ty = (plane != 0) as usize;
                let probs = &f.probs.coef[tx][ty][inter as usize];
                let above = f.above_nz[plane].add(x0);
                let left = self.left_nz[plane].as_mut_ptr().add(y0);
                let mut row = 0;
                while row < max_h {
                    let mut col = 0;
                    while col < max_w {
                        let (a, l) = (above.add(col), left.add(row));
                        let ctx = nz_any(a, tx) + nz_any(l, tx);
                        let more = win.read(src, probs[0][ctx][0]);
                        if COUNT {
                            self.counts.eob_branch[tx][ty][inter as usize][0][ctx] += 1;
                            self.counts.coef[tx][ty][inter as usize][0][ctx][3] += !more as u32;
                        }
                        let mut eob = 0;
                        if more {
                            let tx_type = if intra_luma && plane == 0 {
                                INTRA_TX_TYPE[(if mi.sb_type < BLOCK_8X8 { mi.sub_mode[((row << 1) + col) & 3] } else { mi.mode }).min(9) as usize] as usize
                            } else {
                                0
                            };
                            // A copy, so that the registers themselves are
                            // never in memory.
                            let mut w = win;
                            let counts = (&mut self.counts.coef[tx][ty][inter as usize], &mut self.counts.eob_branch[tx][ty][inter as usize]);
                            let written;
                            (eob, written) = decode_coefs::<COUNT, TX>(&mut w, src, probs, counts, coeffs.add(at + 2), &mut self.token_cache, f.dequant[mi.segment_id as usize & 7][ty], ctx, SCANS[tx][tx_type]);
                            win = w;
                            // The record's place and count, before its
                            // coefficients.
                            *coeffs.add(at) = ((plane << 8) | (row << 4) | col) as i16;
                            *coeffs.add(at + 1) = eob as i16;
                            at += 2 + written;
                            coded += 1;
                        }
                        total += eob;
                        let v = (eob > 0) as u8;
                        if inside {
                            nz_set(a, tx, v);
                            nz_set(l, tx, v);
                        } else {
                            for i in 0..n {
                                *a.add(i) = if col + i >= max_w { 0 } else { v };
                                *l.add(i) = if row + i >= max_h { 0 } else { v };
                            }
                        }
                        col += n;
                    }
                    row += n;
                }
            }
            *coeffs.add(header) = coded as i16;
            buf.coeffs.set_len(at);
        }
        self.r.win = win;
        total
    }
}

/// Whether any of the `1 << tx` contexts at `p` is set.
#[inline(always)]
unsafe fn nz_any(p: *const u8, tx: usize) -> usize {
    unsafe {
        (match tx {
            0 => *p != 0,
            1 => (p as *const u16).read_unaligned() != 0,
            2 => (p as *const u32).read_unaligned() != 0,
            _ => (p as *const u64).read_unaligned() != 0,
        }) as usize
    }
}

/// Sets the `1 << tx` contexts at `p` to `v`, which is 0 or 1.
#[inline(always)]
unsafe fn nz_set(p: *mut u8, tx: usize, v: u8) {
    unsafe {
        match tx {
            0 => *p = v,
            1 => (p as *mut u16).write_unaligned(v as u16 * 0x0101),
            2 => (p as *mut u32).write_unaligned(v as u32 * 0x0101_0101),
            _ => (p as *mut u64).write_unaligned(v as u64 * 0x0101_0101_0101_0101),
        }
    }
}

/// A token's extra bits, most significant first.
#[inline(always)]
fn extra(r: &mut Window, src: &Source, probs: &[u8]) -> i32 {
    probs.iter().fold(0, |v, &p| (v << 1) | r.read(src, p) as i32)
}

/// A token of five or more, after the booleans that said so: its category
/// and its extra bits. Apart from the rest, which is nearly every token, so
/// that the loop over them stays small.
///
/// # Safety
/// `cache` is the token's place in the cache.
#[inline(never)]
unsafe fn large_token(r: &mut Window, src: &Source, p: &[u8; 8], cache: *mut u8) -> i32 {
    if r.read(src, p[3]) {
        unsafe { *cache = 5 };
        if r.read(src, p[5]) {
            if r.read(src, p[7]) { 67 + extra(r, src, &CAT6) } else { 35 + extra(r, src, &CAT5) }
        } else if r.read(src, p[6]) {
            19 + extra(r, src, &CAT4)
        } else {
            11 + extra(r, src, &CAT3)
        }
    } else {
        unsafe { *cache = 4 };
        if r.read(src, p[4]) { 7 + extra(r, src, &CAT2) } else { 5 + extra(r, src, &CAT1) }
    }
}

/// `decode_coefs`: one transform block's tokens after the first, which said
/// that it has any. Returns how many it has in scan order, and how much was
/// written at `out`: the one coefficient of a block of one, which is its
/// first, or, for a 4×4, all sixteen in place, as the transform takes them,
/// or else how many are not zero, and each one's place and value.
///
/// # Safety
/// `out` must have room for one more number than twice the block's
/// coefficients.
#[inline(never)]
unsafe fn decode_coefs<const COUNT: bool, const TX: usize>(win: &mut Window, src: &Source, probs: &[[[u8; 3]; 6]; 6], counts: (&mut [[[u32; 4]; 6]; 6], &mut [[u32; 6]; 6]), out: *mut i16, token_cache: &mut [u8; 1024], dq: [i16; 2], mut ctx: usize, (scan, nb): Scan) -> (usize, usize) {
    let (counts, eob_branch) = counts;
    let tx = TX;
    let max_eob = 16usize << (tx << 1);
    let band_translate: &[u8] = if tx == 0 { &COEFBAND_4X4 } else { &COEFBAND_8X8PLUS };
    let dq_shift = (tx == 3) as u32;
    let mut r = *win;
    let mut dqv = dq[0] as i32;
    let mut c = 0;
    let mut band = 0;
    let mut prob = &probs[0][ctx.min(5)];
    // SAFETY, for every index below that is not checked: `c` is less than
    // `max_eob`, which the scan and the bands are as long as and the
    // neighbours twice as long; a scan's places and its neighbours are inside
    // the block, which the cache is as large as the largest of; and a band
    // and a context are each under 6, the cache holding 5 at most. The tests
    // hold the tables to this.
    // After the count, which is written last.
    let mut pairs = 1;
    if tx == 0 {
        // SAFETY: the caller's: room for sixteen.
        unsafe { out.write_bytes(0, 16) };
    }
    let context = |cache: &[u8; 1024], c: usize| unsafe { (1 + *cache.get_unchecked(*nb.get_unchecked(2 * c) as usize) as usize + *cache.get_unchecked(*nb.get_unchecked(2 * c + 1) as usize) as usize) >> 1 };
    let place = |c: usize| unsafe { *scan.get_unchecked(c) as usize };
    let band_of = |c: usize| unsafe { *band_translate.get_unchecked(c) as usize };
    let prob_of = |band: usize, ctx: usize| unsafe { probs.get_unchecked(band).get_unchecked(ctx) };

    'block: loop {
        while !r.read(src, prob[1]) {
            if COUNT {
                counts[band][ctx][0] += 1;
            }
            dqv = dq[1] as i32;
            unsafe { *token_cache.get_unchecked_mut(place(c)) = 0 };
            c += 1;
            if c >= max_eob {
                break 'block;
            }
            ctx = context(token_cache, c);
            band = band_of(c);
            prob = prob_of(band, ctx);
        }

        let pos = place(c);
        let v = if r.read(src, prob[2]) {
            if COUNT {
                counts[band][ctx][2] += 1;
            }
            let p = &PARETO8_FULL[(prob[2] as usize).saturating_sub(1)];
            if r.read(src, p[0]) {
                // SAFETY: as above.
                let val = unsafe { large_token(&mut r, src, p, token_cache.as_mut_ptr().add(pos)) };
                val.wrapping_mul(dqv) >> dq_shift
            } else if r.read(src, p[1]) {
                unsafe { *token_cache.get_unchecked_mut(pos) = 3 };
                ((3 + r.read(src, p[2]) as i32) * dqv) >> dq_shift
            } else {
                unsafe { *token_cache.get_unchecked_mut(pos) = 2 };
                (2 * dqv) >> dq_shift
            }
        } else {
            if COUNT {
                counts[band][ctx][1] += 1;
            }
            unsafe { *token_cache.get_unchecked_mut(pos) = 1 };
            dqv >> dq_shift
        };
        let v = (if r.bit(src) { -v } else { v }) as i16;
        // SAFETY: the caller's: a pair for each coefficient, after the count,
        // or a 4×4's sixteen.
        unsafe {
            if tx == 0 {
                *out.add(pos) = v;
            } else {
                *out.add(pairs) = pos as i16;
                *out.add(pairs + 1) = v;
            }
        }
        pairs += 2;
        c += 1;
        if c >= max_eob {
            break;
        }
        ctx = context(token_cache, c);
        dqv = dq[1] as i32;
        band = band_of(c);
        prob = prob_of(band, ctx);
        if COUNT {
            eob_branch[band][ctx] += 1;
        }
        if !r.read(src, prob[0]) {
            if COUNT {
                counts[band][ctx][3] += 1;
            }
            break;
        }
    }
    *win = r;
    // SAFETY: as above. A block of one coefficient has it first, which a
    // 4×4's is already.
    unsafe {
        if tx == 0 {
            (c, if c == 1 { 1 } else { 16 })
        } else if c == 1 {
            *out = *out.add(2);
            (1, 1)
        } else {
            *out = (pairs >> 1) as i16;
            (c, pairs)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `decode_coefs` takes on trust of the tables it indexes.
    #[test]
    fn the_scans_and_the_bands_stay_inside_their_blocks() {
        for (tx, scans) in SCANS.iter().enumerate() {
            let n = 16usize << (tx << 1);
            for (scan, nb) in scans {
                assert_eq!(scan.len(), n);
                assert!(nb.len() >= 2 * n);
                assert!(scan.iter().chain(nb.iter()).all(|&v| (v as usize) < n));
            }
        }
        assert!(COEFBAND_4X4.iter().chain(&COEFBAND_8X8PLUS).all(|&b| b < 6));
    }
}
