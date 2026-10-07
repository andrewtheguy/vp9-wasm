//! The decoder: the frames it keeps as references, the probabilities it
//! carries from frame to frame, and a frame's passage through its headers,
//! tiles, loop filter and adaptation.

use std::sync::Arc;

use crate::error::{Error, Result};
use crate::frame::Frame;
use crate::header::{self, FrameHeader, Size, SWITCHABLE, TX_MODE_SELECT};
use crate::lf::{Filtered, LoopFilter};
use crate::probs::{Counts, Probs};
use crate::tables::{AC_QLOOKUP, DC_QLOOKUP};
use crate::tile::{FrameCtx, Tile};

/// The most samples a frame may have: 8192×4320.
const MAX_SAMPLES: usize = 8192 * 4320;

/// A frame that is shown.
pub struct Decoded {
    pub frame: Arc<Frame>,
    pub keyframe: bool,
}

/// What of the frame before decides how the next is decoded.
#[derive(Clone, Copy, Default)]
struct Last {
    width: usize,
    height: usize,
    shown: bool,
    keyframe: bool,
    intra_only: bool,
}

pub struct Decoder {
    threads: usize,
    refs: [Option<Arc<Frame>>; 8],
    /// Every frame made, for its buffers to be used again once nothing else
    /// holds it.
    pool: Vec<Arc<Frame>>,
    contexts: [Probs; 4],
    lf_ref_deltas: [i8; 4],
    lf_mode_deltas: [i8; 2],
    last: Last,
    prev: Option<Arc<Frame>>,
    above_nz: [Vec<u8>; 3],
    above_part: Vec<u8>,
}

impl Decoder {
    /// A decoder whose frames' tiles decode, and rows filter, on `threads` of
    /// rayon's pool; on the caller for one.
    pub fn new(threads: usize) -> Decoder {
        Decoder {
            threads: threads.max(1),
            refs: Default::default(),
            pool: Vec::new(),
            contexts: Default::default(),
            lf_ref_deltas: [1, 0, -1, -1],
            lf_mode_deltas: [0, 0],
            last: Last::default(),
            prev: None,
            above_nz: Default::default(),
            above_part: Vec::new(),
        }
    }

    /// Decodes one unit of the stream: a frame, or a superframe of several of
    /// which at most one is shown. Returns the frame shown, if any. After an
    /// error the stream decodes again from its next keyframe.
    pub fn decode(&mut self, data: &[u8]) -> Result<Option<Decoded>> {
        let mut shown = None;
        for frame in superframe(data)? {
            // A frame of one byte or none is one the encoder dropped.
            if frame.len() > 1 {
                if let Some(d) = self.frame(frame)? {
                    shown = Some(d);
                }
            }
        }
        Ok(shown)
    }

    fn fresh_frame(&mut self, width: usize, height: usize) -> Frame {
        self.pool.retain(|f| f.width == width && f.height == height);
        if let Some(i) = self.pool.iter().position(|f| Arc::strong_count(f) == 1) {
            if let Ok(frame) = Arc::try_unwrap(self.pool.swap_remove(i)) {
                return frame;
            }
        }
        Frame::new(width, height)
    }

    fn frame(&mut self, data: &[u8]) -> Result<Option<Decoded>> {
        let h = header::uncompressed(data, |slot| self.refs[slot].as_ref().map(|f| f.width as u32))?;
        if h.show_existing_frame.is_some() {
            return Err(Error::unsupported("a frame shown again"));
        }
        let (width, height) = match h.size {
            Size::Coded(w, h) => (w as usize, h as usize),
            Size::OfRef(i) => {
                let f = self.refs[h.ref_slots[i]].as_ref().expect("the header found its width");
                (f.width, f.height)
            }
        };
        if width * height > MAX_SAMPLES {
            return Err(Error::unsupported(format!("a {width}x{height} frame")));
        }
        let mut refs: [Option<&Frame>; 3] = [None; 3];
        if !h.intra() {
            for (r, &slot) in refs.iter_mut().zip(&h.ref_slots) {
                let f = self.refs[slot].as_deref().ok_or_else(|| Error::invalid("a frame refers to one the decoder does not have"))?;
                if f.width != width || f.height != height {
                    return Err(Error::unsupported("a reference of another size than the frame"));
                }
                *r = Some(f);
            }
        }
        let rest = &data[h.header_bytes.min(data.len())..];
        if h.compressed_bytes > rest.len() {
            return Err(Error::invalid("a frame's compressed header is cut short"));
        }
        let (compressed, tiles) = rest.split_at(h.compressed_bytes);

        // What the frame leaves for the next is worked out apart, and kept
        // only once the frame has decoded.
        let mut contexts = None;
        let mut context_idx = h.frame_context_idx;
        let (mut ref_deltas, mut mode_deltas) = (self.lf_ref_deltas, self.lf_mode_deltas);
        if h.intra() || h.error_resilient {
            (ref_deltas, mode_deltas) = ([1, 0, -1, -1], [0, 0]);
            let mut c = self.contexts.clone();
            if h.keyframe || h.error_resilient || h.reset_frame_context == 3 {
                c = Default::default();
            } else if h.reset_frame_context == 2 {
                c[context_idx] = Probs::default();
            }
            contexts = Some(c);
            context_idx = 0;
        }
        for (d, new) in ref_deltas.iter_mut().zip(h.lf_ref_delta) {
            *d = new.unwrap_or(*d);
        }
        for (d, new) in mode_deltas.iter_mut().zip(h.lf_mode_delta) {
            *d = new.unwrap_or(*d);
        }
        let pre = contexts.as_ref().unwrap_or(&self.contexts)[context_idx].clone();
        let mut probs = pre.clone();
        let tx_mode = header::compressed(compressed, &h, &mut probs)?.tx_mode;

        let use_prev_mvs = !h.error_resilient && width == self.last.width && height == self.last.height && !self.last.intra_only && self.last.shown && !self.last.keyframe;
        let prev = self.prev.clone().filter(|_| use_prev_mvs && !h.intra());
        let refs_held: [Option<Arc<Frame>>; 3] = std::array::from_fn(|i| if h.intra() { None } else { self.refs[h.ref_slots[i]].clone() });

        let mut frame = self.fresh_frame(width, height);
        if let Some(colour) = h.colour {
            frame.colour = colour;
        } else if let Some(r) = &refs_held[0] {
            frame.colour = r.colour;
        }
        let counts = self.decode_tiles(&h, tx_mode, &probs, tiles, &mut frame, &refs_held, prev.as_deref())?;

        if h.lf_level != 0 {
            let lf = LoopFilter::new(&h, ref_deltas, mode_deltas);
            let f = Filtered { planes: std::array::from_fn(|p| frame.planes[p].data.as_mut_ptr()), stride: frame.planes[0].stride, mi: frame.mi.as_ptr(), mi_cols: frame.mi_cols, mi_rows: frame.mi_rows };
            crate::lf::filter_frame(&lf, &f, self.threads);
        }

        if !h.error_resilient && !h.frame_parallel {
            let mut adapted = probs.clone();
            adapted.adapt_coef(&pre, &counts, if !h.intra() && self.last.keyframe { 128 } else { 112 });
            if !h.intra() {
                adapted.adapt_modes(&pre, &counts, h.interp_filter == SWITCHABLE, tx_mode == TX_MODE_SELECT, h.allow_hp);
            }
            probs = adapted;
        }

        // The frame has decoded: what it changes is kept.
        if let Some(c) = contexts {
            self.contexts = c;
        }
        if h.refresh_frame_context {
            self.contexts[context_idx] = probs;
        }
        (self.lf_ref_deltas, self.lf_mode_deltas) = (ref_deltas, mode_deltas);
        let frame = Arc::new(frame);
        for (i, slot) in self.refs.iter_mut().enumerate() {
            if h.refresh_frame_flags & (1 << i) != 0 {
                *slot = Some(frame.clone());
            }
        }
        self.pool.push(frame.clone());
        self.prev = Some(frame.clone());
        self.last = Last { width, height, shown: h.show_frame, keyframe: h.keyframe, intra_only: h.intra_only };
        Ok(h.show_frame.then_some(Decoded { frame, keyframe: h.keyframe }))
    }

    fn decode_tiles(&mut self, h: &FrameHeader, tx_mode: u8, probs: &Probs, mut data: &[u8], frame: &mut Frame, refs: &[Option<Arc<Frame>>; 3], prev: Option<&Frame>) -> Result<Box<Counts>> {
        let (mi_cols, mi_rows) = (frame.mi_cols, frame.mi_rows);
        let sb_cols = mi_cols.div_ceil(8);
        let sb_rows = mi_rows.div_ceil(8);
        for nz in &mut self.above_nz {
            nz.clear();
            nz.resize(sb_cols * 16, 0);
        }
        self.above_part.clear();
        self.above_part.resize(sb_cols * 8, 0);

        let q = h.base_qindex as i32;
        let dc = |delta: i32| DC_QLOOKUP[(q + delta).clamp(0, 255) as usize];
        let ac = |delta: i32| AC_QLOOKUP[(q + delta).clamp(0, 255) as usize];
        let f = FrameCtx {
            h,
            tx_mode,
            probs,
            width: frame.width,
            height: frame.height,
            mi_cols,
            mi_rows,
            stride: frame.planes[0].stride,
            cur: std::array::from_fn(|p| frame.planes[p].data.as_mut_ptr()),
            refs: std::array::from_fn(|r| std::array::from_fn(|p| refs[r].as_ref().map_or(std::ptr::null(), |f| f.planes[p].data.as_ptr()))),
            mi: frame.mi.as_mut_ptr(),
            prev_mi: prev.map_or(std::ptr::null(), |p| p.mi.as_ptr()),
            above_nz: std::array::from_fn(|p| self.above_nz[p].as_mut_ptr()),
            above_part: self.above_part.as_mut_ptr(),
            dequant: [[dc(h.y_dc_delta), ac(0)], [dc(h.uv_dc_delta), ac(h.uv_ac_delta)]],
        };

        let (tile_cols, tile_rows) = (1usize << h.log2_tile_cols, 1usize << h.log2_tile_rows);
        let offset = |idx: usize, sbs: usize, log2: u32, mis: usize| (((idx * sbs) >> log2) << 3).min(mis);
        let mut counts = Box::<Counts>::default();
        for tile_row in 0..tile_rows {
            let mut tiles = Vec::with_capacity(tile_cols);
            for tile_col in 0..tile_cols {
                let size = if tile_row == tile_rows - 1 && tile_col == tile_cols - 1 {
                    data.len()
                } else {
                    let (size, rest) = data.split_first_chunk::<4>().ok_or_else(|| Error::invalid("a tile's size is cut short"))?;
                    data = rest;
                    u32::from_be_bytes(*size) as usize
                };
                if size > data.len() {
                    return Err(Error::invalid("a tile is cut short"));
                }
                let (tile, rest) = data.split_at(size);
                data = rest;
                tiles.push(Tile::new(&f, tile, offset(tile_col, sb_cols, h.log2_tile_cols, mi_cols), offset(tile_col + 1, sb_cols, h.log2_tile_cols, mi_cols))?);
            }
            let (row_start, row_end) = (offset(tile_row, sb_rows, h.log2_tile_rows, mi_rows), offset(tile_row + 1, sb_rows, h.log2_tile_rows, mi_rows));
            crate::threads::each(&mut tiles, self.threads, |tile| tile.decode(row_start, row_end))?;
            for tile in &tiles {
                counts.add(&tile.counts);
            }
        }
        Ok(counts)
    }
}

/// The frames of a unit: one, or those a superframe's index at its end lists.
fn superframe(data: &[u8]) -> Result<Vec<&[u8]>> {
    let Some(&marker) = data.last() else {
        return Err(Error::invalid("an empty frame"));
    };
    if marker & 0xe0 == 0xc0 {
        let frames = (marker & 7) as usize + 1;
        let mag = ((marker >> 3) & 3) as usize + 1;
        let index = 2 + mag * frames;
        if data.len() >= index && data[data.len() - index] == marker {
            let mut sizes = &data[data.len() - index + 1..data.len() - 1];
            let mut body = &data[..data.len() - index];
            let mut out = Vec::with_capacity(frames);
            for _ in 0..frames {
                let size = sizes[..mag].iter().rev().fold(0usize, |v, &b| (v << 8) | b as usize);
                sizes = &sizes[mag..];
                if size > body.len() {
                    return Err(Error::invalid("a superframe's index runs past it"));
                }
                let (frame, rest) = body.split_at(size);
                out.push(frame);
                body = rest;
            }
            return Ok(out);
        }
    }
    Ok(vec![data])
}
