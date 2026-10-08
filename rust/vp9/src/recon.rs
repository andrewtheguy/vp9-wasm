//! Making a parsed block's samples: its prediction, from the frame's own
//! samples or from a reference, and the residual of its coefficients on top.
//! Transcribed from libvpx 1.16.0's vp9_decodeframe.c and vp9_reconintra.c,
//! for 4:4:4 and references of the frame's own size.

use crate::frame::*;
use crate::intra::Pred;
use crate::tile::{FrameCtx, RowBuf, INTRA_TX_TYPE};
use crate::{convolve, intra, itx};

const NEED_LEFT: u8 = 2;
const NEED_ABOVE: u8 = 4;
const NEED_ABOVERIGHT: u8 = 8;
/// Which of its neighbours an intra mode predicts from.
const EXTEND: [u8; 10] = [NEED_ABOVE | NEED_LEFT, NEED_ABOVE, NEED_LEFT, NEED_ABOVERIGHT, NEED_LEFT | NEED_ABOVE, NEED_LEFT | NEED_ABOVE, NEED_LEFT | NEED_ABOVE, NEED_LEFT, NEED_ABOVERIGHT, NEED_LEFT | NEED_ABOVE];

#[repr(align(32))]
struct Scratch([i16; 1024]);

/// One thread's reconstruction of rows of 64×64 blocks.
pub(crate) struct Recon<'a> {
    f: &'a FrameCtx<'a>,
    /// All zero but while a block's coefficients are transformed from it.
    dc: Box<Scratch>,
    /// A reference block with the frame's edge repeated around it.
    mc: Box<[u8; 80 * 80]>,
    /// The row's coefficients, and how far into them the blocks have got.
    buf: *const RowBuf,
    next_coeff: usize,

    // The block being made.
    mi_row: usize,
    mi_col: usize,
    have_above: bool,
    have_left: bool,
    to_right: i32,
    to_bottom: i32,
}

impl<'a> Recon<'a> {
    pub fn new(f: &'a FrameCtx<'a>) -> Self {
        Recon { f, dc: Box::new(Scratch([0; 1024])), mc: Box::new([0; 80 * 80]), buf: std::ptr::null(), next_coeff: 0, mi_row: 0, mi_col: 0, have_above: false, have_left: false, to_right: 0, to_bottom: 0 }
    }

    /// The row of 64×64 blocks `sb_row` of an inter frame starts as the LAST
    /// reference's rows, one copy of the contiguous bytes, so that a still
    /// block from it (`ModeInfo::still`) is in place already. Past the frame's
    /// right and bottom edges, as far as the last 8×8, the samples are the
    /// edge's repeated, which is what a block that crosses an edge is
    /// predicted from (`predict_inter_block`) and what the blocks after it
    /// and the loop filter read there.
    ///
    /// # Safety
    /// Nothing has written the row yet, and no other thread touches it.
    pub unsafe fn start_row(&self, sb_row: usize) {
        let f = self.f;
        let (stride, w, h) = (f.stride, f.width, f.height);
        let (cols, rows) = (f.mi_cols * 8, f.mi_rows * 8);
        let y0 = sb_row * 64;
        // The row has an 8×8 inside the frame, so it starts above the edge.
        let y1 = (y0 + 64).min(h);
        for plane in 0..3 {
            let (src, dst) = (f.refs[0][plane], f.cur[plane]);
            // SAFETY: the planes are whole rows of 64×64 blocks, and the
            // reference is of the frame's size.
            unsafe {
                std::ptr::copy_nonoverlapping(src.add(y0 * stride), dst.add(y0 * stride), (y1 - y0) * stride);
                for y in y0..y1 {
                    let line = dst.add(y * stride);
                    std::ptr::write_bytes(line.add(w), *line.add(w - 1), cols - w);
                }
                for y in h..(y0 + 64).min(rows) {
                    std::ptr::copy_nonoverlapping(dst.add((h - 1) * stride), dst.add(y * stride), cols);
                }
            }
        }
    }

    /// Starts on tile column `tile`'s part of the row of 64×64 blocks
    /// `sb_row`, which has been parsed.
    pub fn start(&mut self, tile: usize, sb_row: usize) {
        self.buf = self.f.rows[tile * self.f.sb_rows + sb_row].0.get();
        self.next_coeff = 0;
    }

    /// The 64×64 block at (`mi_row`, `mi_col`) in 8×8 units, of the tile
    /// column that starts at `tile_start`: the next of the row started.
    ///
    /// # Safety
    /// The block's row has been parsed, the blocks to its left and above it
    /// are made, and no other thread is making this one.
    pub unsafe fn superblock(&mut self, mi_row: usize, mi_col: usize, tile_start: usize) {
        self.partition(mi_row, mi_col, 3, tile_start);
    }

    /// The blocks of the `8 << bsl` square here, as the modes say it was
    /// split.
    fn partition(&mut self, mi_row: usize, mi_col: usize, bsl: u32, tile_start: usize) {
        let f = self.f;
        if mi_row >= f.mi_rows || mi_col >= f.mi_cols {
            return;
        }
        // SAFETY: inside the frame, and parsed.
        let mi = unsafe { *f.mi.add(mi_row * f.mi_cols + mi_col) };
        let (wl, hl) = (B_WIDTH_LOG2[mi.sb_type as usize] as u32, B_HEIGHT_LOG2[mi.sb_type as usize] as u32);
        let hbs = (1usize << bsl) >> 1;
        let n4 = bsl + 1;
        if bsl == 0 || (wl == n4 && hl == n4) {
            self.block(&mi, mi_row, mi_col, tile_start);
        } else if wl == n4 && hl == n4 - 1 {
            self.block(&mi, mi_row, mi_col, tile_start);
            if mi_row + hbs < f.mi_rows {
                // SAFETY: as above.
                let mi = unsafe { *f.mi.add((mi_row + hbs) * f.mi_cols + mi_col) };
                self.block(&mi, mi_row + hbs, mi_col, tile_start);
            }
        } else if wl == n4 - 1 && hl == n4 {
            self.block(&mi, mi_row, mi_col, tile_start);
            if mi_col + hbs < f.mi_cols {
                // SAFETY: as above.
                let mi = unsafe { *f.mi.add(mi_row * f.mi_cols + mi_col + hbs) };
                self.block(&mi, mi_row, mi_col + hbs, tile_start);
            }
        } else {
            self.partition(mi_row, mi_col, bsl - 1, tile_start);
            self.partition(mi_row, mi_col + hbs, bsl - 1, tile_start);
            self.partition(mi_row + hbs, mi_col, bsl - 1, tile_start);
            self.partition(mi_row + hbs, mi_col + hbs, bsl - 1, tile_start);
        }
    }

    fn block(&mut self, mi: &ModeInfo, mi_row: usize, mi_col: usize, tile_start: usize) {
        if mi.still() {
            // The samples its row started as (`start_row`), and no residual.
            return;
        }
        let f = self.f;
        let bsize = mi.sb_type;
        let (bw, bh) = (NUM_8X8_WIDE[bsize as usize] as usize, NUM_8X8_HIGH[bsize as usize] as usize);
        let (n4_w, n4_h) = (bw * 2, bh * 2);
        self.mi_row = mi_row;
        self.mi_col = mi_col;
        self.to_bottom = (f.mi_rows as i32 - bh as i32 - mi_row as i32) * 64;
        self.to_right = (f.mi_cols as i32 - bw as i32 - mi_col as i32) * 64;
        self.have_above = mi_row > 0;
        self.have_left = mi_col > tile_start;
        // The 4×4s of the block inside the frame, across and down.
        let max_w = if self.to_right >= 0 { n4_w } else { (n4_w as i32 + (self.to_right >> 5)) as usize };
        let max_h = if self.to_bottom >= 0 { n4_h } else { (n4_h as i32 + (self.to_bottom >> 5)) as usize };
        let lossless = f.h.lossless;
        let tx = mi.tx_size as usize & 3;
        let step = 1 << tx;

        if !mi.is_inter() {
            // A block under 8×8 is two 4×4s a side.
            let bwl = (B_WIDTH_LOG2[bsize as usize] as u32).max(1);
            // The transform blocks with coefficients come in the order walked
            // here, so the next one's place is compared with each block's.
            let mut coded = if mi.skip { 0 } else { self.coded() };
            for plane in 0..3 {
                for row in (0..max_h).step_by(step) {
                    for col in (0..max_w).step_by(step) {
                        let mode = if plane != 0 {
                            mi.uv_mode
                        } else if bsize < BLOCK_8X8 {
                            mi.sub_mode[(row << 1) + col]
                        } else {
                            mi.mode
                        };
                        let dst = self.dst(plane, col, row);
                        self.predict_intra(dst, tx, mode.min(9), bwl, col, row);
                        if coded > 0 && self.next_place() == Some((plane << 8) | (row << 4) | col) {
                            coded -= 1;
                            let Some(eob) = self.next_eob() else { return };
                            let tx_type = if plane != 0 || lossless { 0 } else { INTRA_TX_TYPE[mode.min(9) as usize] as usize };
                            self.residual(eob, dst, tx, tx_type);
                        }
                    }
                }
            }
        } else {
            self.predict_inter(mi, n4_w, n4_h);
            if !mi.skip {
                // The transform blocks with coefficients alone, each at its
                // place; one that is not a whole transform block of this
                // block inside the frame is nothing the parser wrote.
                for _ in 0..self.coded() {
                    let Some(place) = self.next_place() else { return };
                    let Some(eob) = self.next_eob() else { return };
                    let (plane, row, col) = (place >> 8, (place >> 4) & 15, place & 15);
                    if plane > 2 || row >= max_h || col >= max_w || (row | col) & (step - 1) != 0 {
                        return;
                    }
                    self.residual(eob, self.dst(plane, col, row), tx, 0);
                }
            }
        }
    }

    /// How many transform blocks of the block have coefficients: what the
    /// parser left first.
    #[inline(always)]
    fn coded(&mut self) -> usize {
        // SAFETY: the row is parsed, and nothing writes it until the next frame.
        let n = unsafe { &*self.buf }.coeffs.get(self.next_coeff).copied().unwrap_or(0);
        self.next_coeff += 1;
        n.max(0) as usize
    }

    /// The next coded transform block's place, left where it is.
    #[inline(always)]
    fn next_place(&self) -> Option<usize> {
        // SAFETY: as above.
        unsafe { &*self.buf }.coeffs.get(self.next_coeff).map(|&p| p as u16 as usize)
    }

    /// Past the next coded transform block's place, how many coefficients
    /// it has in scan order.
    #[inline(always)]
    fn next_eob(&mut self) -> Option<u16> {
        // SAFETY: as above.
        let eob = unsafe { &*self.buf }.coeffs.get(self.next_coeff + 1).map(|&e| e as u16);
        self.next_coeff += 2;
        eob
    }

    /// Where the transform block at (`col`, `row`) 4×4s into the block is, in
    /// `plane`.
    #[inline]
    fn dst(&self, plane: usize, col: usize, row: usize) -> *mut u8 {
        // SAFETY: the block starts inside the frame, whose planes are whole
        // 64×64 blocks.
        unsafe { self.f.cur[plane].add((self.mi_row * 8 + row * 4) * self.f.stride + self.mi_col * 8 + col * 4) }
    }

    /// Adds the residual of a transform block of `eob` coefficients, the next
    /// that has any.
    fn residual(&mut self, eob: u16, dst: *mut u8, tx: usize, tx_type: usize) {
        // SAFETY: the row is parsed, and nothing writes it until the next frame.
        let buf = unsafe { &*self.buf };
        let n = 4usize << tx;
        let (lossless, stride) = (self.f.h.lossless, self.f.stride);
        let block = &mut self.dc.0;
        // SAFETY: a transform block that starts inside the frame ends inside
        // its planes.
        unsafe {
            if eob == 1 {
                let Some(&dc) = buf.coeffs.get(self.next_coeff) else { return };
                self.next_coeff += 1;
                block[0] = dc;
                itx::inverse_add(tx, tx_type, lossless, &block[..n * n], 1, dst, stride);
                block[0] = 0;
            } else if tx == 0 {
                // A 4×4 is left whole, as the transform takes it.
                let Some(whole) = buf.coeffs.get(self.next_coeff..self.next_coeff + 16) else { return };
                self.next_coeff += 16;
                itx::inverse_add(0, tx_type, lossless, whole, eob as usize, dst, stride);
            } else {
                let Some(&count) = buf.coeffs.get(self.next_coeff) else { return };
                let Some(pairs) = buf.coeffs.get(self.next_coeff + 1..self.next_coeff + 1 + 2 * count as usize) else { return };
                self.next_coeff += 1 + pairs.len();
                // Each coefficient to its place, and the places zero again
                // once the block is made.
                for pair in pairs.chunks_exact(2) {
                    block[pair[0] as usize & (n * n - 1)] = pair[1];
                }
                itx::inverse_add(tx, tx_type, lossless, &block[..n * n], eob as usize, dst, stride);
                for pair in pairs.chunks_exact(2) {
                    block[pair[0] as usize & (n * n - 1)] = 0;
                }
            }
        }
    }

    /// The intra prediction of one transform block, from the samples above it
    /// and to its left as libvpx's `vp9_predict_intra_block` gathers them.
    fn predict_intra(&mut self, dst: *mut u8, tx: usize, mode: u8, bwl: u32, col: usize, row: usize) {
        let f = self.f;
        let stride = f.stride;
        let bs = 4usize << tx;
        let up = row != 0 || self.have_above;
        let left = col != 0 || self.have_left;
        let right = col + (1 << tx) < (1 << bwl);
        let frame_width = f.mi_cols * 8;
        let frame_height = f.mi_rows * 8;
        let x0 = self.mi_col * 8 + col * 4;
        let y0 = self.mi_row * 8 + row * 4;
        let ext = EXTEND[mode as usize];

        let mut left_col = [0u8; 32];
        let mut above_data = [0u8; 64 + 16];
        // SAFETY: the samples read are of this frame, above and to the left of
        // a block that starts inside it, and no further right or down than the
        // frame's size in whole 8×8s.
        unsafe {
            let above_row = above_data.as_mut_ptr().add(16);
            let mut above: *const u8 = above_row;
            let above_ref = dst.wrapping_sub(stride) as *const u8;

            if ext & NEED_LEFT != 0 {
                if left {
                    let have = if self.to_bottom < 0 && y0 + bs > frame_height { frame_height - y0 } else { bs };
                    for i in 0..bs {
                        left_col[i] = *dst.add(i.min(have - 1) * stride).sub(1);
                    }
                } else {
                    left_col[..bs].fill(129);
                }
            }

            if ext & NEED_ABOVE != 0 {
                if up {
                    if self.to_right < 0 {
                        if x0 + bs <= frame_width {
                            std::ptr::copy_nonoverlapping(above_ref, above_row, bs);
                        } else if x0 <= frame_width {
                            let r = frame_width - x0;
                            std::ptr::copy_nonoverlapping(above_ref, above_row, r);
                            std::ptr::write_bytes(above_row.add(r), *above_row.add(r - 1), x0 + bs - frame_width);
                        }
                    } else if bs == 4 && right && left {
                        above = above_ref;
                    } else {
                        std::ptr::copy_nonoverlapping(above_ref, above_row, bs);
                    }
                    *above_row.sub(1) = if left { *above_ref.sub(1) } else { 129 };
                } else {
                    std::ptr::write_bytes(above_row.sub(1), 127, bs + 1);
                }
            }

            if ext & NEED_ABOVERIGHT != 0 {
                if up {
                    if self.to_right < 0 {
                        if x0 + 2 * bs <= frame_width {
                            if right && bs == 4 {
                                std::ptr::copy_nonoverlapping(above_ref, above_row, 2 * bs);
                            } else {
                                std::ptr::copy_nonoverlapping(above_ref, above_row, bs);
                                std::ptr::write_bytes(above_row.add(bs), *above_row.add(bs - 1), bs);
                            }
                        } else if x0 + bs <= frame_width {
                            let r = frame_width - x0;
                            if right && bs == 4 {
                                std::ptr::copy_nonoverlapping(above_ref, above_row, r);
                                std::ptr::write_bytes(above_row.add(r), *above_row.add(r - 1), x0 + 2 * bs - frame_width);
                            } else {
                                std::ptr::copy_nonoverlapping(above_ref, above_row, bs);
                                std::ptr::write_bytes(above_row.add(bs), *above_row.add(bs - 1), bs);
                            }
                        } else if x0 <= frame_width {
                            let r = frame_width - x0;
                            std::ptr::copy_nonoverlapping(above_ref, above_row, r);
                            std::ptr::write_bytes(above_row.add(r), *above_row.add(r - 1), x0 + 2 * bs - frame_width);
                        }
                    } else if bs == 4 && right && left {
                        above = above_ref;
                    } else {
                        std::ptr::copy_nonoverlapping(above_ref, above_row, bs);
                        if bs == 4 && right {
                            std::ptr::copy_nonoverlapping(above_ref.add(bs), above_row.add(bs), bs);
                        } else {
                            std::ptr::write_bytes(above_row.add(bs), *above_row.add(bs - 1), bs);
                        }
                    }
                    *above_row.sub(1) = if left { *above_ref.sub(1) } else { 129 };
                } else {
                    std::ptr::write_bytes(above_row.sub(1), 127, bs * 2 + 1);
                }
            }

            let pred = match (mode, left, up) {
                (0, true, true) => Pred::Dc,
                (0, true, false) => Pred::DcLeft,
                (0, false, true) => Pred::DcTop,
                (0, false, false) => Pred::Dc128,
                (1, ..) => Pred::V,
                (2, ..) => Pred::H,
                (3, ..) => Pred::D45,
                (4, ..) => Pred::D135,
                (5, ..) => Pred::D117,
                (6, ..) => Pred::D153,
                (7, ..) => Pred::D207,
                (8, ..) => Pred::D63,
                _ => Pred::Tm,
            };
            intra::predict(pred, tx, dst, stride, above, left_col.as_ptr());
        }
    }

    /// The block's prediction from its reference, in all three planes.
    fn predict_inter(&mut self, mi: &ModeInfo, n4_w: usize, n4_h: usize) {
        let refs = self.f.refs[(mi.ref_frame as usize).clamp(1, 3) - 1];
        let (x, y) = (self.mi_col * 8, self.mi_row * 8);
        for plane in 0..3 {
            if mi.sb_type < BLOCK_8X8 {
                for i in 0..4 {
                    self.predict_inter_block(refs[plane], plane, x + 4 * (i & 1), y + 4 * (i >> 1), 4, 4, mi.sub_mv[i], mi.interp_filter);
                }
            } else {
                self.predict_inter_block(refs[plane], plane, x, y, 4 * n4_w, 4 * n4_h, mi.mv, mi.interp_filter);
            }
        }
    }

    /// `dec_build_inter_predictors`, for a reference of the frame's own size.
    fn predict_inter_block(&mut self, reference: *const u8, plane: usize, x: usize, y: usize, w: usize, h: usize, mv: Mv, filter: u8) {
        let f = self.f;
        let stride = f.stride;
        let (fw, fh) = (f.width as i32, f.height as i32);
        // Sixteenths of a sample.
        let (mv_col, mv_row) = (mv.col as i32 * 2, mv.row as i32 * 2);
        let (subpel_x, subpel_y) = ((mv_col & 15) as usize, (mv_row & 15) as usize);
        let mut x0 = x as i32 + (mv_col >> 4);
        let mut y0 = y as i32 + (mv_row >> 4);
        // SAFETY: the destination is a block of this frame. The reference is
        // read where the block lands in it, which is inside the frame or is
        // gathered into `mc` with the frame's edges repeated; a block that
        // does not move reads the reference's own block.
        unsafe {
            let dst = f.cur[plane].add(y * stride + x);
            if mv_col != 0 || mv_row != 0 || (fw & 7) != 0 || (fh & 7) != 0 {
                let (ox, oy) = (x0, y0);
                let mut x1 = x0 + w as i32;
                let mut y1 = y0 + h as i32;
                let (mut x_pad, mut y_pad) = (0, 0);
                if subpel_x != 0 {
                    x0 -= 3;
                    x1 += 4;
                    x_pad = 1;
                }
                if subpel_y != 0 {
                    y0 -= 3;
                    y1 += 4;
                    y_pad = 1;
                }
                if x0 < 0 || x0 > fw - 1 || x1 < 0 || x1 > fw - 1 || y0 < 0 || y0 > fh - 1 || y1 < 0 || y1 > fh - 1 {
                    let b_w = (x1 - x0 + 1) as usize;
                    let b_h = (y1 - y0 + 1) as usize;
                    let left = (-x0).clamp(0, b_w as i32) as usize;
                    let right = (x0 + b_w as i32 - fw).clamp(0, b_w as i32) as usize;
                    let copy = (b_w - left).saturating_sub(right);
                    for j in 0..b_h {
                        let row = reference.add((y0 + j as i32).clamp(0, fh - 1) as usize * stride);
                        let out = self.mc.as_mut_ptr().add(j * b_w);
                        std::ptr::write_bytes(out, *row, left);
                        if copy != 0 {
                            std::ptr::copy_nonoverlapping(row.add((x0 + left as i32) as usize), out.add(left), copy);
                        }
                        std::ptr::write_bytes(out.add(left + copy), *row.add(fw as usize - 1), b_w - left - copy);
                    }
                    let src = self.mc.as_ptr().add(y_pad * 3 * b_w + x_pad * 3);
                    convolve::predict(src, b_w, dst, stride, filter as usize & 3, subpel_x, subpel_y, w, h);
                    return;
                }
                x0 = ox;
                y0 = oy;
            }
            convolve::predict(reference.add(y0 as usize * stride + x0 as usize), stride, dst, stride, filter as usize & 3, subpel_x, subpel_y, w, h);
        }
    }
}
