//! The loop filter over a decoded frame: which edges of each 64×64 block are
//! filtered and how wide, from the blocks' modes. Transcribed from libvpx
//! 1.16.0's vp9_loopfilter.c (`vp9_filter_block_plane_non420`), for 4:4:4,
//! where the three planes share their edges.

use crate::frame::{ModeInfo, B_HEIGHT_LOG2, B_WIDTH_LOG2, NUM_8X8_HIGH, NUM_8X8_WIDE};
use crate::header::FrameHeader;
use crate::loopfilter::*;

pub(crate) struct LoopFilter {
    /// [reference][whether the mode is one with a motion vector]
    lvl: [[u8; 2]; 4],
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
    pub fn new(h: &FrameHeader, ref_deltas: [i8; 4], mode_deltas: [i8; 2]) -> LoopFilter {
        let level = h.lf_level as i32;
        let mut lf = LoopFilter { lvl: [[h.lf_level; 2]; 4], lim: [0; 64], mblim: [0; 64] };
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
        if h.lf_delta_enabled {
            let scale = 1 << (level >> 5);
            lf.lvl[0] = [(level + ref_deltas[0] as i32 * scale).clamp(0, 63) as u8; 2];
            for r in 1..4 {
                for m in 0..2 {
                    lf.lvl[r][m] = (level + ref_deltas[r] as i32 * scale + mode_deltas[m] as i32 * scale).clamp(0, 63) as u8;
                }
            }
        }
        lf
    }

    /// Filters the 64×64 block at (`mi_row`, `mi_col`) in 8×8 units: its
    /// vertical edges, then its horizontal ones, which reach eight samples
    /// into the blocks to its left and above it.
    ///
    /// # Safety
    /// No other thread may be filtering this block, the one to its left, or
    /// the three above it and above to either side.
    pub unsafe fn filter_sb(&self, f: &Filtered, mi_row: usize, mi_col: usize) {
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
                let level = self.lvl[mi.ref_frame as usize & 3][matches!(mi.mode, 10 | 11 | 13) as usize];
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
            return;
        }

        // The frame's left edge is not filtered, nor its top one.
        let border = if mi_col == 0 { !1u32 } else { !0 };
        let pitch = f.stride as isize;
        for plane in f.planes {
            // SAFETY: the block is inside the plane, and an edge is filtered
            // only where there are eight samples on both sides of it.
            unsafe {
                let sb = plane.add(mi_row * 8 * f.stride + mi_col * 8);
                for r in 0..rows {
                    let row = sb.add(r * 8 * f.stride);
                    let (m16, m8, m4, int) = (mask_16x16_c[r] & border, mask_8x8_c[r] & border, mask_4x4_c[r] & border, mask_4x4_int[r]);
                    let mut mask = m16 | m8 | m4 | int;
                    while mask != 0 {
                        let c = mask.trailing_zeros() as usize;
                        mask &= mask - 1;
                        let bit = 1 << c;
                        let l = lfl[r * 8 + c] as usize;
                        let (mblim, lim, hev) = (self.mblim[l], self.lim[l], (l >> 4) as u8);
                        let s = row.add(c * 8);
                        if m16 & bit != 0 {
                            lpf_vertical_16(s, pitch, mblim, lim, hev);
                        } else if m8 & bit != 0 {
                            lpf_vertical_8(s, pitch, mblim, lim, hev);
                        } else if m4 & bit != 0 {
                            lpf_vertical_4(s, pitch, mblim, lim, hev);
                        }
                        if int & bit != 0 {
                            lpf_vertical_4(s.add(4), pitch, mblim, lim, hev);
                        }
                    }
                }
                for r in 0..rows {
                    let row = sb.add(r * 8 * f.stride);
                    let top = mi_row + r == 0;
                    let (m16, m8, m4, int) = if top { (0, 0, 0, mask_4x4_int[r]) } else { (mask_16x16[r], mask_8x8[r], mask_4x4[r], mask_4x4_int[r]) };
                    let mut mask = m16 | m8 | m4 | int;
                    while mask != 0 {
                        let c = mask.trailing_zeros() as usize;
                        mask &= mask - 1;
                        let bit = 1 << c;
                        let l = lfl[r * 8 + c] as usize;
                        let (mblim, lim, hev) = (self.mblim[l], self.lim[l], (l >> 4) as u8);
                        let s = row.add(c * 8);
                        if m16 & bit != 0 {
                            lpf_horizontal_16(s, pitch, mblim, lim, hev);
                        } else if m8 & bit != 0 {
                            lpf_horizontal_8(s, pitch, mblim, lim, hev);
                        } else if m4 & bit != 0 {
                            lpf_horizontal_4(s, pitch, mblim, lim, hev);
                        }
                        if int & bit != 0 {
                            lpf_horizontal_4(s.add(4 * f.stride), pitch, mblim, lim, hev);
                        }
                    }
                }
            }
        }
    }
}

/// Filters the whole frame, its 64×64 blocks in raster order.
pub(crate) fn filter_frame(lf: &LoopFilter, f: &Filtered, _threads: usize) {
    for mi_row in (0..f.mi_rows).step_by(8) {
        for mi_col in (0..f.mi_cols).step_by(8) {
            // SAFETY: one block at a time.
            unsafe { lf.filter_sb(f, mi_row, mi_col) };
        }
    }
}
