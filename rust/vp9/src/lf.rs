//! The loop filter over a decoded frame: which edges of each 64×64 block are
//! filtered and how wide, from the blocks' modes. Transcribed from libvpx
//! 1.16.0's vp9_loopfilter.c (`vp9_filter_block_plane_non420`), for 4:4:4,
//! where the three planes share their edges.

use crate::frame::{ModeInfo, B_HEIGHT_LOG2, B_WIDTH_LOG2, NUM_8X8_HIGH, NUM_8X8_WIDE};
use crate::header::{FrameHeader, SEG_LVL_ALT_LF, Segments};
use crate::loopfilter::*;

pub(crate) struct LoopFilter {
    /// [segment][reference][whether the mode is one with a motion vector]
    lvl: [[[u8; 2]; 4]; 8],
    lim: [u8; 64],
    mblim: [u8; 64],
}

/// The frame the filter runs over. Blocks are filtered in raster order, or
/// side by side where no two touch the same samples.
pub(crate) struct Filtered {
    pub planes: [*mut u8; 3],
    pub stride: usize,
    pub mi: *const ModeInfo,
    pub mi_cols: usize,
    pub mi_rows: usize,
}

// SAFETY: see `LoopFilter::filter_sb` for what one call touches.
unsafe impl Sync for Filtered {}

impl LoopFilter {
    pub fn new(h: &FrameHeader, seg: &Segments, ref_deltas: [i8; 4], mode_deltas: [i8; 2]) -> LoopFilter {
        let level = h.lf_level as i32;
        let mut lf = LoopFilter { lvl: [[[0; 2]; 4]; 8], lim: [0; 64], mblim: [0; 64] };
        for lvl in 0..64 {
            let sharp = h.lf_sharpness as i32;
            let mut inside = lvl as i32 >> ((sharp > 0) as i32 + (sharp > 4) as i32);
            if sharp > 0 {
                inside = inside.min(9 - sharp);
            }
            inside = inside.max(1);
            lf.lim[lvl] = inside as u8;
            lf.mblim[lvl] = (2 * (lvl as i32 + 2) + inside) as u8;
        }
        // The deltas are scaled by the frame's level, whatever a segment's.
        let scale = 1 << (level >> 5);
        for (segment, lvl) in lf.lvl.iter_mut().enumerate() {
            let level = match seg.feature(segment as u8, SEG_LVL_ALT_LF) {
                Some(d) => (if seg.features.absolute { d as i32 } else { level + d as i32 }).clamp(0, 63),
                None => level,
            };
            *lvl = [[level as u8; 2]; 4];
            if h.lf_delta_enabled {
                lvl[0] = [(level + ref_deltas[0] as i32 * scale).clamp(0, 63) as u8; 2];
                for r in 1..4 {
                    for m in 0..2 {
                        lvl[r][m] = (level + ref_deltas[r] as i32 * scale + mode_deltas[m] as i32 * scale).clamp(0, 63) as u8;
                    }
                }
            }
        }
        lf
    }

    /// Filters the 64×64 block at (`mi_row`, `mi_col`) in 8×8 units: its
    /// vertical edges, then its horizontal ones, which reach eight samples
    /// into the blocks to its left and above it. Returns whether any of its
    /// blocks has a filter level at all: when none does, nothing is written.
    ///
    /// # Safety
    /// No other thread may be filtering this block, the one to its left, or
    /// the three above it and above to either side.
    pub unsafe fn filter_sb(&self, f: &Filtered, mi_row: usize, mi_col: usize) -> bool {
        let mut mask_16x16 = [0u32; 8];
        let mut mask_8x8 = [0u32; 8];
        let mut mask_4x4 = [0u32; 8];
        let mut mask_4x4_int = [0u32; 8];
        let mut mask_16x16_c = [0u32; 8];
        let mut mask_8x8_c = [0u32; 8];
        let mut mask_4x4_c = [0u32; 8];
        let mut lfl = [0u8; 64];
        let rows = 8.min(f.mi_rows - mi_row);
        let cols = 8.min(f.mi_cols - mi_col);
        let mut any = false;

        for r in 0..rows {
            for c in 0..cols {
                // SAFETY: inside the frame.
                let mi = unsafe { &*f.mi.add((mi_row + r) * f.mi_cols + mi_col + c) };
                let sb = mi.sb_type as usize;
                let skip_this = mi.skip && mi.is_inter();
                let edge_left = if B_WIDTH_LOG2[sb] > 0 { c & (NUM_8X8_WIDE[sb] as usize - 1) == 0 } else { true };
                let skip_c = skip_this && !edge_left;
                let edge_above = if B_HEIGHT_LOG2[sb] > 0 { r & (NUM_8X8_HIGH[sb] as usize - 1) == 0 } else { true };
                let skip_r = skip_this && !edge_above;
                let level = self.lvl[mi.segment_id as usize & 7][mi.ref_frame as usize & 3][matches!(mi.mode, 10 | 11 | 13) as usize];
                lfl[r * 8 + c] = level;
                if level == 0 {
                    continue;
                }
                any = true;
                let bit = 1 << c;
                match mi.tx_size {
                    3 => {
                        if !skip_c && c & 3 == 0 {
                            mask_16x16_c[r] |= bit;
                        }
                        if !skip_r && r & 3 == 0 {
                            mask_16x16[r] |= bit;
                        }
                    }
                    2 => {
                        if !skip_c && c & 1 == 0 {
                            mask_16x16_c[r] |= bit;
                        }
                        if !skip_r && r & 1 == 0 {
                            mask_16x16[r] |= bit;
                        }
                    }
                    tx => {
                        // At least the eight-sample filter on a 32×32 edge.
                        if !skip_c {
                            if tx == 1 || c & 3 == 0 { mask_8x8_c[r] |= bit } else { mask_4x4_c[r] |= bit }
                        }
                        if !skip_r {
                            if tx == 1 || r & 3 == 0 { mask_8x8[r] |= bit } else { mask_4x4[r] |= bit }
                        }
                        if !skip_this && tx == 0 {
                            mask_4x4_int[r] |= bit;
                        }
                    }
                }
            }
        }
        if !any {
            return false;
        }

        // The frame's left edge is not filtered, nor its top one.
        let border = if mi_col == 0 { !1u32 } else { !0 };
        // The widest filter of each edge of a row, 16, 8 or 4, by column: none
        // where the bit of each mask is clear.
        let width = |masks: [u32; 3], bit: u32| {
            if masks[0] & bit != 0 {
                16
            } else if masks[1] & bit != 0 {
                8
            } else if masks[2] & bit != 0 {
                4
            } else {
                0
            }
        };
        let limits = |r: usize, c: usize| {
            let l = lfl[r * 8 + c] as usize;
            (self.mblim[l], self.lim[l], (l >> 4) as u8)
        };
        let pitch = f.stride as isize;
        for plane in f.planes {
            // SAFETY: the block is inside the plane, and an edge is filtered
            // only where there are eight samples on both sides of it.
            unsafe {
                let sb = plane.add(mi_row * 8 * f.stride + mi_col * 8);
                // The vertical edges of two rows at a time, column by column,
                // since each reads what the one before it in its row wrote: two
                // of the same width, one above the other, as one.
                for r in (0..rows).step_by(2) {
                    let row = sb.add(r * 8 * f.stride);
                    let masks = |r: usize| [mask_16x16_c[r] & border, mask_8x8_c[r] & border, mask_4x4_c[r] & border];
                    let (a, ia) = (masks(r), mask_4x4_int[r]);
                    let (b, ib) = if r + 1 < rows { (masks(r + 1), mask_4x4_int[r + 1]) } else { ([0; 3], 0) };
                    let mut mask = a[0] | a[1] | a[2] | ia | b[0] | b[1] | b[2] | ib;
                    while mask != 0 {
                        let c = mask.trailing_zeros() as usize;
                        mask &= mask - 1;
                        let bit = 1 << c;
                        let (s, below) = (row.add(c * 8), row.add(8 * f.stride + c * 8));
                        let (wa, wb) = (width(a, bit), width(b, bit));
                        let (b0, l0, t0) = limits(r, c);
                        // Of a last row alone, the row below's limits are not read.
                        let (b1, l1, t1) = if wb != 0 || ib & bit != 0 { limits(r + 1, c) } else { (0, 0, 0) };
                        match (wa, wb) {
                            (4, 4) => lpf_vertical_4_dual(s, pitch, b0, l0, t0, b1, l1, t1),
                            (8, 8) => lpf_vertical_8_dual(s, pitch, b0, l0, t0, b1, l1, t1),
                            _ => {
                                vertical(wa, s, pitch, b0, l0, t0);
                                vertical(wb, below, pitch, b1, l1, t1);
                            }
                        }
                        match (ia & bit != 0, ib & bit != 0) {
                            (true, true) => lpf_vertical_4_dual(s.add(4), pitch, b0, l0, t0, b1, l1, t1),
                            (true, false) => lpf_vertical_4(s.add(4), pitch, b0, l0, t0),
                            (false, true) => lpf_vertical_4(below.add(4), pitch, b1, l1, t1),
                            (false, false) => {}
                        }
                    }
                }
                // The horizontal edges row by row: two of the same width side
                // by side as one.
                for r in 0..rows {
                    let row = sb.add(r * 8 * f.stride);
                    let top = mi_row + r == 0;
                    let masks = if top { [0; 3] } else { [mask_16x16[r], mask_8x8[r], mask_4x4[r]] };
                    let int = mask_4x4_int[r];
                    let mut mask = masks[0] | masks[1] | masks[2] | int;
                    while mask != 0 {
                        let c = mask.trailing_zeros() as usize;
                        let bit = 1 << c;
                        // The edge after it, taken with it.
                        let next = mask & bit << 1;
                        mask &= !(bit | next);
                        let s = row.add(c * 8);
                        let wa = width(masks, bit);
                        let wb = width(masks, next);
                        let (b0, l0, t0) = limits(r, c);
                        let (b1, l1, t1) = if next != 0 { limits(r, c + 1) } else { (0, 0, 0) };
                        match (wa, wb) {
                            (4, 4) => lpf_horizontal_4_dual(s, pitch, b0, l0, t0, b1, l1, t1),
                            (8, 8) => lpf_horizontal_8_dual(s, pitch, b0, l0, t0, b1, l1, t1),
                            _ => {
                                horizontal(wa, s, pitch, b0, l0, t0);
                                horizontal(wb, s.add(8), pitch, b1, l1, t1);
                            }
                        }
                        let inner = s.add(4 * f.stride);
                        match (int & bit != 0, int & next != 0) {
                            (true, true) => lpf_horizontal_4_dual(inner, pitch, b0, l0, t0, b1, l1, t1),
                            (true, false) => lpf_horizontal_4(inner, pitch, b0, l0, t0),
                            (false, true) => lpf_horizontal_4(inner.add(8), pitch, b1, l1, t1),
                            (false, false) => {}
                        }
                    }
                }
            }
        }
        true
    }
}

/// Filters 8 rows along a vertical edge with the filter of `width` samples, or
/// none when it is 0.
///
/// # Safety
/// As `lpf_vertical_16` for the samples the filter reaches.
#[inline(always)]
unsafe fn vertical(width: u32, s: *mut u8, pitch: isize, blimit: u8, limit: u8, thresh: u8) {
    unsafe {
        match width {
            16 => lpf_vertical_16(s, pitch, blimit, limit, thresh),
            8 => lpf_vertical_8(s, pitch, blimit, limit, thresh),
            4 => lpf_vertical_4(s, pitch, blimit, limit, thresh),
            _ => {}
        }
    }
}

/// Filters 8 samples along a horizontal edge with the filter of `width`
/// samples, or none when it is 0.
///
/// # Safety
/// As `lpf_horizontal_16` for the samples the filter reaches.
#[inline(always)]
unsafe fn horizontal(width: u32, s: *mut u8, pitch: isize, blimit: u8, limit: u8, thresh: u8) {
    unsafe {
        match width {
            16 => lpf_horizontal_16(s, pitch, blimit, limit, thresh),
            8 => lpf_horizontal_8(s, pitch, blimit, limit, thresh),
            4 => lpf_horizontal_4(s, pitch, blimit, limit, thresh),
            _ => {}
        }
    }
}
