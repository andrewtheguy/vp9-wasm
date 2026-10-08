//! The decoder: the frames it keeps as references, the probabilities it
//! carries from frame to frame, and a frame's passage through its headers,
//! tiles, loop filter and adaptation.

use std::sync::Arc;

use crate::error::{Error, Result};
use crate::frame::Frame;
use crate::header::{self, Colour, FrameHeader, Size, SWITCHABLE, TX_MODE_SELECT};
use crate::lf::{Filtered, LoopFilter};
use crate::probs::{Counts, Probs};
use crate::tables::{AC_QLOOKUP, DC_QLOOKUP};
use crate::recon::Recon;
use crate::tile::{FrameCtx, RowCell, Tile};
use crate::wavefront::Progress;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// The most samples a frame may have: 8192×4320.
const MAX_SAMPLES: usize = 8192 * 4320;

/// The most bytes a decoder's frames may take together, those it keeps and
/// those its caller still holds: three quarters of the gibibyte the page's
/// module can grow to, the rest being for what else a frame is decoded with.
const MAX_FRAME_BYTES: usize = 768 << 20;

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
    #[cfg_attr(not(feature = "threads"), allow(dead_code))]
    threads: usize,
    refs: [Option<Arc<Frame>>; 8],
    /// Every frame made that is still held, for its buffers to be used again
    /// once nothing else holds it.
    pool: Vec<Arc<Frame>>,
    /// What the frames in the pool and one more may take together.
    frame_bytes: usize,
    contexts: [Probs; 4],
    /// The colour last stated, which a frame that states none has.
    colour: Colour,
    lf_ref_deltas: [i8; 4],
    lf_mode_deltas: [i8; 2],
    last: Last,
    prev: Option<Arc<Frame>>,
    above_nz: [Vec<u8>; 3],
    above_part: Vec<u8>,
    /// Each tile column's rows of coefficients, between their parsing and
    /// their reconstruction.
    rows: Vec<RowCell>,
    /// A frame failed: nothing decodes until a keyframe.
    broken: bool,
}

impl Decoder {
    /// A decoder whose frames decode on `threads` of rayon's pool; on the
    /// caller for one.
    pub fn new(threads: usize) -> Decoder {
        Decoder {
            threads: threads.max(1),
            refs: Default::default(),
            pool: Vec::new(),
            frame_bytes: MAX_FRAME_BYTES,
            contexts: Default::default(),
            colour: Colour::default(),
            lf_ref_deltas: [1, 0, -1, -1],
            lf_mode_deltas: [0, 0],
            last: Last::default(),
            prev: None,
            above_nz: Default::default(),
            above_part: Vec::new(),
            rows: Vec::new(),
            broken: false,
        }
    }

    /// Decodes one unit of the stream: a frame, or a superframe of several of
    /// which at most one is shown. Returns the frame shown, if any. After an
    /// error the stream decodes again from its next keyframe.
    pub fn decode(&mut self, data: &[u8]) -> Result<Option<Decoded>> {
        let mut shown = None;
        let frames = match superframe(data) {
            Ok(frames) => frames,
            Err(e) => {
                self.broken = true;
                return Err(e);
            }
        };
        for frame in frames {
            // An empty frame is one the encoder dropped. One byte is a whole
            // header where it shows a frame again.
            if !frame.is_empty() {
                match self.frame(frame) {
                    Ok(Some(d)) => shown = Some(d),
                    Ok(None) => {}
                    Err(e) => {
                        self.broken = true;
                        return Err(e);
                    }
                }
            }
        }
        Ok(shown)
    }

    fn fresh_frame(&mut self, width: usize, height: usize) -> Result<Frame> {
        // A frame of another size goes once nothing else holds it: until
        // then it is memory taken, and counted.
        self.pool.retain(|f| Arc::strong_count(f) > 1 || (f.width == width && f.height == height));
        if let Some(i) = self.pool.iter().position(|f| Arc::strong_count(f) == 1 && f.width == width && f.height == height) {
            if let Ok(frame) = Arc::try_unwrap(self.pool.swap_remove(i)) {
                return Ok(frame);
            }
        }
        let held: usize = self.pool.iter().map(|f| Frame::bytes(f.width, f.height)).sum();
        if held + Frame::bytes(width, height) > self.frame_bytes {
            return Err(Error::unsupported(format!("a {width}x{height} frame, with the {} still held, is more than a decoder keeps", self.pool.len())));
        }
        Frame::new(width, height)
    }

    fn frame(&mut self, data: &[u8]) -> Result<Option<Decoded>> {
        let h = header::uncompressed(data, |slot| self.refs[slot].as_ref().map(|f| f.width as u32))?;
        if self.broken && !h.keyframe {
            return Err(Error::invalid("a frame that follows one that failed, and is not a keyframe"));
        }
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
        if let Some((w, h)) = h.render.filter(|&(w, h)| (w as usize, h as usize) != (width, height)) {
            return Err(Error::unsupported(format!("a {width}x{height} frame to be shown scaled, at {w}x{h}")));
        }
        // Not their product, which wraps where a `usize` is 32 bits.
        if width > MAX_SAMPLES / height {
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

        let mut frame = self.fresh_frame(width, height)?;
        frame.colour = h.colour.unwrap_or(self.colour);
        let lf = (h.lf_level != 0).then(|| LoopFilter::new(&h, ref_deltas, mode_deltas));
        let counts = self.decode_rows(&h, tx_mode, &probs, tiles, &mut frame, &refs_held, prev.as_deref(), lf.as_ref())?;

        if !h.error_resilient && !h.frame_parallel {
            let mut adapted = probs.clone();
            adapted.adapt_coef(&pre, &counts, if !h.intra() && self.last.keyframe { 128 } else { 112 });
            if !h.intra() {
                adapted.adapt_modes(&pre, &counts, h.interp_filter == SWITCHABLE, tx_mode == TX_MODE_SELECT, h.allow_hp);
            }
            probs = adapted;
        }

        // The frame has decoded: what it changes is kept.
        self.broken = false;
        if let Some(c) = contexts {
            self.contexts = c;
        }
        if h.refresh_frame_context {
            self.contexts[context_idx] = probs;
        }
        (self.lf_ref_deltas, self.lf_mode_deltas) = (ref_deltas, mode_deltas);
        self.colour = frame.colour;
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

    /// The frame's samples: its tiles parsed, and its rows of 64×64 blocks
    /// reconstructed and then filtered, each stage as far behind the one
    /// before as what it reads requires.
    fn decode_rows(&mut self, h: &FrameHeader, tx_mode: u8, probs: &Probs, mut data: &[u8], frame: &mut Frame, refs: &[Option<Arc<Frame>>; 3], prev: Option<&Frame>, lf: Option<&LoopFilter>) -> Result<Box<Counts>> {
        let (mi_cols, mi_rows) = (frame.mi_cols, frame.mi_rows);
        let sb_cols = mi_cols.div_ceil(8);
        let sb_rows = mi_rows.div_ceil(8);
        for nz in &mut self.above_nz {
            nz.clear();
            nz.resize(sb_cols * 16, 0);
        }
        self.above_part.clear();
        self.above_part.resize(sb_cols * 8, 0);

        let (tile_cols, tile_rows) = (1usize << h.log2_tile_cols, 1usize << h.log2_tile_rows);
        let offset = |idx: usize, sbs: usize, log2: u32, mis: usize| (((idx * sbs) >> log2) << 3).min(mis);
        // Each tile but the last comes after its size.
        let mut tile_data = vec![Vec::with_capacity(tile_rows); tile_cols];
        for tile_row in 0..tile_rows {
            for (tile_col, of_col) in tile_data.iter_mut().enumerate() {
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
                of_col.push((offset(tile_row, sb_rows, h.log2_tile_rows, mi_rows), tile));
            }
        }
        if self.rows.len() < tile_cols * sb_rows {
            self.rows.resize_with(tile_cols * sb_rows, || RowCell(Default::default()));
        }

        let q = h.base_qindex as i32;
        let dc = |delta: i32| DC_QLOOKUP[(q + delta).clamp(0, 255) as usize];
        let ac = |delta: i32| AC_QLOOKUP[(q + delta).clamp(0, 255) as usize];
        let f = FrameCtx {
            h,
            tx_mode,
            probs,
            counting: !h.error_resilient && !h.frame_parallel,
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
            tile_starts: (0..=tile_cols).map(|i| offset(i, sb_cols, h.log2_tile_cols, mi_cols)).collect(),
            rows: &self.rows,
            sb_rows,
        };
        let filtered = Filtered { planes: f.cur, stride: f.stride, mi: f.mi, mi_cols, mi_rows };
        let mut tiles: Vec<Tile> = tile_data.into_iter().enumerate().map(|(i, data)| Tile::new(&f, i, data)).collect();

        // A tile's rows parsed, and each row's blocks reconstructed and
        // filtered.
        let parsed: Vec<Progress> = (0..tile_cols).map(|_| Progress::default()).collect();
        let made: Vec<Progress> = (0..sb_rows).map(|_| Progress::default()).collect();
        let smooth: Vec<Progress> = (0..sb_rows).map(|_| Progress::default()).collect();
        let first = Mutex::new(None::<Error>);
        let stop = AtomicBool::new(false);

        let parse = |tile: &mut Tile, i: usize| {
            for row in 0..sb_rows {
                if !stop.load(Ordering::Relaxed) {
                    match tile.parse_row(row) {
                        Ok(()) => {
                            parsed[i].advance(row + 1);
                            continue;
                        }
                        Err(e) => {
                            stop.store(true, Ordering::Relaxed);
                            first.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert(e);
                        }
                    }
                }
                return parsed[i].fail();
            }
        };
        // A row is made a block behind the row above it, whose samples its
        // intra blocks predict from. An inter frame's row starts as the LAST
        // reference's, before the waits.
        let make = |recon: &mut Recon, row: usize| {
            if !h.intra() {
                // SAFETY: the row is this thread's, and nothing has written it.
                unsafe { recon.start_row(row) };
            }
            for tile in 0..tile_cols {
                if !parsed[tile].wait_for(row + 1) {
                    return made[row].fail();
                }
                recon.start(tile, row);
                for col in f.tile_starts[tile] / 8..f.tile_starts[tile + 1].div_ceil(8) {
                    if row > 0 && !made[row - 1].wait_for(col + 1) {
                        return made[row].fail();
                    }
                    // SAFETY: the row is parsed, the block above is made, and
                    // this thread alone has the row.
                    unsafe { recon.superblock(row * 8, col * 8, f.tile_starts[tile]) };
                    made[row].advance(col + 1);
                }
            }
        };
        // A block is filtered once nothing is still to be predicted from its
        // samples, to its right and below, and once the blocks whose filtering
        // comes before its own in raster order and shares samples with it are
        // done: the one above and to the right.
        let filter = |lf: &LoopFilter, row: usize| {
            for col in 0..sb_cols {
                let ahead = (col + 2).min(sb_cols);
                if !made[row].wait_for(ahead) || (row + 1 < sb_rows && !made[row + 1].wait_for(ahead)) || (row > 0 && !smooth[row - 1].wait_for(ahead)) {
                    return smooth[row].fail();
                }
                // SAFETY: the waits above are what the block's filtering
                // requires, and this thread alone has the row.
                unsafe { lf.filter_sb(&filtered, row * 8, col * 8) };
                smooth[row].advance(col + 1);
            }
        };

        // The pool's threads parse a tile each, then join those making and
        // filtering rows, which each take the next row that is ready soonest.
        // A tile's thread waits on its neighbours' parsing, so each tile needs
        // a thread the pool really has.
        #[cfg(feature = "threads")]
        let workers = self.threads.min(rayon::current_num_threads());
        #[cfg(feature = "threads")]
        let threaded = workers > 1 && workers >= tile_cols;
        #[cfg(not(feature = "threads"))]
        let threaded = {
            let _ = &parse;
            false
        };
        if threaded {
            #[cfg(feature = "threads")]
            {
                use std::sync::atomic::AtomicUsize;
                let next_make = AtomicUsize::new(0);
                let next_filter = AtomicUsize::new(if lf.is_some() { 0 } else { sb_rows });
                let rows = || {
                    let mut recon = Recon::new(&f);
                    loop {
                        let (m, s) = (next_make.load(Ordering::Relaxed), next_filter.load(Ordering::Relaxed));
                        if s < sb_rows && (m >= sb_rows || s + 2 <= m) {
                            if next_filter.compare_exchange(s, s + 1, Ordering::Relaxed, Ordering::Relaxed).is_ok() {
                                filter(lf.expect("rows to filter"), s);
                            }
                        } else if m < sb_rows {
                            if next_make.compare_exchange(m, m + 1, Ordering::Relaxed, Ordering::Relaxed).is_ok() {
                                make(&mut recon, m);
                            }
                        } else {
                            break;
                        }
                    }
                };
                let (parse, rows) = (&parse, &rows);
                let extra = workers - tile_cols;
                rayon::scope(|s| {
                    for (i, tile) in tiles.iter_mut().enumerate() {
                        s.spawn(move |_| {
                            parse(tile, i);
                            rows();
                        });
                    }
                    for _ in 0..extra {
                        s.spawn(move |_| rows());
                    }
                });
            }
        } else {
            // On the caller: a row parsed, then made, then the row above it
            // filtered.
            let mut recon = Recon::new(&f);
            'rows: for row in 0..sb_rows {
                for (i, tile) in tiles.iter_mut().enumerate() {
                    if let Err(e) = tile.parse_row(row) {
                        first.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert(e);
                        break 'rows;
                    }
                    parsed[i].advance(row + 1);
                }
                make(&mut recon, row);
                if let Some(lf) = lf {
                    if row > 0 {
                        filter(lf, row - 1);
                    }
                    if row + 1 == sb_rows {
                        filter(lf, row);
                    }
                }
            }
        }
        if let Some(e) = first.into_inner().unwrap_or_else(|e| e.into_inner()) {
            return Err(e);
        }

        let mut counts = Box::<Counts>::default();
        for tile in &tiles {
            counts.add(&tile.counts);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The two frames of a fixture: a keyframe that states BT.709, and a frame
    /// predicted from it.
    fn fixture() -> (&'static [u8], &'static [u8]) {
        let file: &[u8] = include_bytes!("../../../test/data/bt709-full-160x96.ivf");
        let frame = |at: usize| &file[at + 12..at + 12 + u32::from_le_bytes(file[at..at + 4].try_into().unwrap()) as usize];
        let key = frame(32);
        (key, frame(32 + 12 + key.len()))
    }

    /// The keyframe with its uncompressed header written again: `bits` is
    /// given the header's bits to make the new one's of.
    fn reheaded(key: &[u8], bits: impl FnOnce(&[u8]) -> Vec<u8>) -> Vec<u8> {
        let old = header::uncompressed(key, |_| None).unwrap().header_bytes;
        let was: Vec<u8> = (0..old * 8).map(|i| key[i >> 3] >> (7 - (i & 7)) & 1).collect();
        let mut bits = bits(&was);
        bits.resize(bits.len().next_multiple_of(8), 0);
        let mut frame: Vec<u8> = bits.chunks(8).map(|byte| byte.iter().fold(0, |v, &b| v << 1 | b)).collect();
        // The old header's padding may have become a byte of its own.
        let new = header::uncompressed(&frame, |_| None).unwrap().header_bytes;
        frame.truncate(new);
        frame.extend_from_slice(&key[old..]);
        frame
    }

    #[test]
    fn a_frame_that_states_no_colour_has_the_last_one_stated() {
        let (key, inter) = fixture();
        // The keyframe again as an intra-only frame that resets the contexts
        // as a keyframe does, states BT.601 and is kept in no slot: the frame
        // after it is predicted from the keyframe still.
        let intra = reheaded(key, |was| {
            let mut bits = vec![1, 0, 1, 0, 0, 1, 0, 0, 1, 1, 1];
            bits.extend_from_slice(&was[8..32]);
            bits.extend_from_slice(&[0, 0, 1]);
            bits.extend_from_slice(&was[35..39]);
            bits.extend_from_slice(&[0; 8]);
            bits.extend_from_slice(&was[39..]);
            bits
        });
        let mut d = Decoder::new(1);
        assert_eq!(d.decode(key).unwrap().unwrap().frame.colour.space, 2);
        assert!(d.decode(&intra).unwrap().is_none());
        assert_eq!(d.decode(inter).unwrap().unwrap().frame.colour.space, 1);
    }

    #[test]
    fn a_frame_to_be_shown_at_another_size_is_refused_by_name() {
        let (key, _) = fixture();
        // The render size follows the frame's, after a flag that it does.
        let shown_at = |size: &[u8]| {
            reheaded(key, |was| {
                assert_eq!(was[71], 0);
                [&was[..71], &[1], size, &was[72..]].concat()
            })
        };
        let bits = |v: u16| (0..16).rev().map(|i| (v >> i & 1) as u8).collect::<Vec<_>>();
        assert!(Decoder::new(1).decode(&shown_at(&[bits(159), bits(95)].concat())).unwrap().is_some());
        let e = Decoder::new(1).decode(&shown_at(&[bits(159), bits(47)].concat())).err().expect("refused");
        assert_eq!(e, Error::unsupported("a 160x96 frame to be shown scaled, at 160x48"));
    }

    #[test]
    fn a_frame_more_than_the_decoder_keeps_is_refused_until_one_is_let_go() {
        let (key, inter) = fixture();
        let mut d = Decoder::new(1);
        d.frame_bytes = Frame::bytes(160, 96);
        // The keyframe is in every slot, so the next frame is a second one.
        let shown = d.decode(key).unwrap().unwrap();
        let e = d.decode(inter).err().expect("refused");
        assert_eq!(e, Error::unsupported("a 160x96 frame, with the 1 still held, is more than a decoder keeps"));
        // A keyframe has no room either while the first is in the slots, and
        // the caller's hold on it is not what keeps it.
        drop(shown);
        assert!(d.decode(key).is_err());
        d.frame_bytes = 2 * Frame::bytes(160, 96);
        assert!(d.decode(key).unwrap().is_some());
        assert!(d.decode(inter).unwrap().is_some());
    }

    #[test]
    fn a_frame_shown_again_in_one_byte_is_refused_by_name() {
        // Frame marker 2, profile 0, show_existing_frame, slot 0.
        let e = Decoder::new(1).decode(&[0b1000_1000]).err().expect("refused");
        assert!(matches!(e, Error::Unsupported(_)), "{e}");
    }

    #[test]
    fn a_superframe_that_fails_to_split_waits_for_a_keyframe() {
        // An index of one frame, one byte a size, that claims more than the body.
        let mut d = Decoder::new(1);
        assert!(d.decode(&[0, 0xc0, 9, 0xc0]).is_err());
        assert!(d.broken);
    }
}
