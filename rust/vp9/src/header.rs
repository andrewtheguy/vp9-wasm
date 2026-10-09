//! A frame's two headers: the uncompressed one, and the probability updates
//! of the compressed one.

use crate::bits::{BitReader, BoolDecoder};
use crate::error::{Error, Result};
use crate::probs::{Probs, diff_update};

/// `interp_filter` of a frame whose blocks each code their own.
pub const SWITCHABLE: u8 = 4;
/// `tx_mode` of a frame whose blocks each code their own transform size.
pub const TX_MODE_SELECT: u8 = 4;

/// The segment features, by their place in a segment's four: an alternate
/// quantizer, an alternate loop filter level, a reference frame, and a skip,
/// which is no residual and no motion.
pub const SEG_LVL_ALT_Q: usize = 0;
pub const SEG_LVL_ALT_LF: usize = 1;
pub const SEG_LVL_REF_FRAME: usize = 2;
pub const SEG_LVL_SKIP: usize = 3;
/// Each feature's data: how many bits it is coded in, and whether a sign
/// follows them.
const SEG_FEATURE_BITS: [(u32, bool); 4] = [(8, true), (6, true), (2, false), (0, false)];

/// The eight segments' features: per segment, per feature, its data where
/// the feature is on.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct SegmentFeatures {
    /// Whether the data is the value itself, rather than a change to the
    /// frame's.
    pub absolute: bool,
    pub data: [[Option<i16>; 4]; 8],
}

/// A frame's segmentation, as its uncompressed header codes it.
#[derive(Clone, Copy, Debug)]
pub struct Segmentation {
    pub enabled: bool,
    /// Whether each block codes its segment, or has the one the last map
    /// gives it.
    pub update_map: bool,
    /// Whether a block may code its segment as the last map's.
    pub temporal_update: bool,
    pub tree_probs: [u8; 7],
    pub pred_probs: [u8; 3],
    /// The features this frame puts in force, where it does; the frame
    /// before's stay in force otherwise.
    pub update_data: Option<SegmentFeatures>,
}

impl Default for Segmentation {
    fn default() -> Self {
        Segmentation { enabled: false, update_map: false, temporal_update: false, tree_probs: [255; 7], pred_probs: [255; 3], update_data: None }
    }
}

/// Segmentation as a frame decodes under it: its header's, with the
/// features in force, its own or carried from a frame before.
#[derive(Clone, Copy)]
pub struct Segments {
    pub enabled: bool,
    pub update_map: bool,
    pub temporal_update: bool,
    pub tree_probs: [u8; 7],
    pub pred_probs: [u8; 3],
    pub features: SegmentFeatures,
}

impl Segments {
    pub fn new(s: &Segmentation, features: SegmentFeatures) -> Self {
        Segments { enabled: s.enabled, update_map: s.update_map, temporal_update: s.temporal_update, tree_probs: s.tree_probs, pred_probs: s.pred_probs, features }
    }

    /// `feature`'s data for `segment`, where segmentation is on and so is
    /// the feature.
    #[inline]
    pub fn feature(&self, segment: u8, feature: usize) -> Option<i16> {
        if self.enabled { self.features.data[segment as usize & 7][feature] } else { None }
    }
}

/// The colour a keyframe states, as VP9 codes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Colour {
    /// `color_space`: 0 unknown, 1 BT.601, 2 BT.709, 3 SMPTE 170, 4 SMPTE 240,
    /// 5 BT.2020, 7 sRGB.
    pub space: u8,
    pub full_range: bool,
}

/// How a frame names its size.
pub enum Size {
    Coded(u32, u32),
    /// As one of its three references, by index.
    OfRef(usize),
}

pub struct FrameHeader {
    pub show_existing_frame: Option<usize>,
    pub keyframe: bool,
    pub show_frame: bool,
    pub error_resilient: bool,
    pub intra_only: bool,
    pub reset_frame_context: u8,
    /// Stated by a keyframe or an intra-only frame.
    pub colour: Option<Colour>,
    pub refresh_frame_flags: u8,
    pub ref_slots: [usize; 3],
    pub sign_bias: [bool; 4],
    pub size: Size,
    /// The size to show the frame at, where it states one.
    pub render: Option<(u32, u32)>,
    pub allow_hp: bool,
    pub interp_filter: u8,
    pub refresh_frame_context: bool,
    pub frame_parallel: bool,
    pub frame_context_idx: usize,
    pub lf_level: u8,
    pub lf_sharpness: u8,
    pub lf_delta_enabled: bool,
    /// The deltas this frame replaces, by reference then by mode.
    pub lf_ref_delta: [Option<i8>; 4],
    pub lf_mode_delta: [Option<i8>; 2],
    pub base_qindex: u8,
    pub y_dc_delta: i32,
    pub uv_dc_delta: i32,
    pub uv_ac_delta: i32,
    pub lossless: bool,
    pub segmentation: Segmentation,
    pub log2_tile_cols: u32,
    pub log2_tile_rows: u32,
    /// Bytes of the uncompressed header and of the compressed one after it.
    pub header_bytes: usize,
    pub compressed_bytes: usize,
}

impl FrameHeader {
    pub fn intra(&self) -> bool {
        self.keyframe || self.intra_only
    }
}

fn sync_code(r: &mut BitReader) -> Result<()> {
    if r.literal(24)? != 0x49_83_42 {
        return Err(Error::invalid("a frame's sync code is wrong"));
    }
    Ok(())
}

/// The only colour layout this decoder takes: 8 bits, 4:4:4.
fn colour_444(r: &mut BitReader, profile: u32) -> Result<Colour> {
    if profile >= 2 {
        return Err(Error::unsupported(format!("profile {profile}: more than 8 bits a sample")));
    }
    if profile != 1 {
        return Err(Error::unsupported("profile 0: 4:2:0 chroma, where only 4:4:4 is decoded"));
    }
    let space = r.literal(3)? as u8;
    if space == 7 {
        if r.flag()? {
            return Err(Error::invalid("a reserved bit is set"));
        }
        return Ok(Colour { space, full_range: true });
    }
    let full_range = r.flag()?;
    let (ss_x, ss_y) = (r.flag()?, r.flag()?);
    if r.flag()? {
        return Err(Error::invalid("a reserved bit is set"));
    }
    if ss_x || ss_y {
        return Err(Error::unsupported(format!("4:{} chroma, where only 4:4:4 is decoded", if ss_y { "2:0" } else { "2:2" })));
    }
    Ok(Colour { space, full_range })
}

fn frame_size(r: &mut BitReader) -> Result<Size> {
    let (w, h) = (r.literal(16)? + 1, r.literal(16)? + 1);
    Ok(Size::Coded(w, h))
}

/// The render size is where a scaled picture would be shown.
fn render_size(r: &mut BitReader) -> Result<Option<(u32, u32)>> {
    if !r.flag()? {
        return Ok(None);
    }
    Ok(Some((r.literal(16)? + 1, r.literal(16)? + 1)))
}

fn delta_q(r: &mut BitReader) -> Result<i32> {
    if r.flag()? { r.signed(4) } else { Ok(0) }
}

/// `segmentation_params`: whether the frame's blocks are in segments, how
/// their segments are coded, and the features that replace those in force.
fn segmentation(r: &mut BitReader) -> Result<Segmentation> {
    let mut s = Segmentation { enabled: r.flag()?, ..Segmentation::default() };
    if !s.enabled {
        return Ok(s);
    }
    s.update_map = r.flag()?;
    if s.update_map {
        for p in &mut s.tree_probs {
            *p = if r.flag()? { r.literal(8)? as u8 } else { 255 };
        }
        s.temporal_update = r.flag()?;
        if s.temporal_update {
            for p in &mut s.pred_probs {
                *p = if r.flag()? { r.literal(8)? as u8 } else { 255 };
            }
        }
    }
    if r.flag()? {
        let mut features = SegmentFeatures { absolute: r.flag()?, data: Default::default() };
        for segment in &mut features.data {
            for (feature, (bits, signed)) in segment.iter_mut().zip(SEG_FEATURE_BITS) {
                if r.flag()? {
                    let v = r.literal(bits)? as i16;
                    *feature = Some(if signed && r.flag()? { -v } else { v });
                }
            }
        }
        s.update_data = Some(features);
    }
    Ok(s)
}

/// The uncompressed header. `mi_cols_of` gives a size's width in 8-sample
/// units once the size is known, which the tile layout is coded against; a
/// size named by reference is resolved through `ref_width`.
pub fn uncompressed(data: &[u8], ref_width: impl Fn(usize) -> Option<u32>) -> Result<FrameHeader> {
    let mut r = BitReader::new(data);
    if r.literal(2)? != 2 {
        return Err(Error::invalid("not a VP9 frame"));
    }
    let mut profile = r.bit()? | r.bit()? << 1;
    if profile > 2 {
        profile += r.bit()?;
    }
    let mut h = FrameHeader {
        show_existing_frame: None,
        keyframe: false,
        show_frame: true,
        error_resilient: false,
        intra_only: false,
        reset_frame_context: 0,
        colour: None,
        refresh_frame_flags: 0,
        ref_slots: [0; 3],
        sign_bias: [false; 4],
        size: Size::OfRef(0),
        render: None,
        allow_hp: false,
        interp_filter: 0,
        refresh_frame_context: false,
        frame_parallel: true,
        frame_context_idx: 0,
        lf_level: 0,
        lf_sharpness: 0,
        lf_delta_enabled: false,
        lf_ref_delta: [None; 4],
        lf_mode_delta: [None; 2],
        base_qindex: 0,
        y_dc_delta: 0,
        uv_dc_delta: 0,
        uv_ac_delta: 0,
        lossless: false,
        segmentation: Segmentation::default(),
        log2_tile_cols: 0,
        log2_tile_rows: 0,
        header_bytes: 0,
        compressed_bytes: 0,
    };
    if r.flag()? {
        h.show_existing_frame = Some(r.literal(3)? as usize);
        return Ok(h);
    }
    h.keyframe = !r.flag()?;
    h.show_frame = r.flag()?;
    h.error_resilient = r.flag()?;
    if h.keyframe {
        sync_code(&mut r)?;
        h.colour = Some(colour_444(&mut r, profile)?);
        h.refresh_frame_flags = 0xff;
        h.size = frame_size(&mut r)?;
        h.render = render_size(&mut r)?;
    } else {
        h.intra_only = !h.show_frame && r.flag()?;
        h.reset_frame_context = if h.error_resilient { 0 } else { r.literal(2)? as u8 };
        if h.intra_only {
            sync_code(&mut r)?;
            h.colour = Some(colour_444(&mut r, profile)?);
            h.refresh_frame_flags = r.literal(8)? as u8;
            h.size = frame_size(&mut r)?;
            h.render = render_size(&mut r)?;
        } else {
            if profile != 1 {
                return Err(Error::unsupported(format!("profile {profile}, where only profile 1 (8 bits, 4:4:4) is decoded")));
            }
            h.refresh_frame_flags = r.literal(8)? as u8;
            for i in 0..3 {
                h.ref_slots[i] = r.literal(3)? as usize;
                h.sign_bias[i + 1] = r.flag()?;
            }
            let mut found = None;
            for i in 0..3 {
                if r.flag()? {
                    found = Some(i);
                    break;
                }
            }
            h.size = match found {
                Some(i) => Size::OfRef(i),
                None => frame_size(&mut r)?,
            };
            h.render = render_size(&mut r)?;
            h.allow_hp = r.flag()?;
            h.interp_filter = if r.flag()? { SWITCHABLE } else { [1, 0, 2, 3][r.literal(2)? as usize] };
        }
    }
    let width = match h.size {
        Size::Coded(w, _) => w,
        Size::OfRef(i) => ref_width(h.ref_slots[i]).ok_or_else(|| Error::invalid("a frame takes its size from a reference the decoder does not have"))?,
    };
    if !h.error_resilient {
        h.refresh_frame_context = r.flag()?;
        h.frame_parallel = r.flag()?;
    }
    h.frame_context_idx = r.literal(2)? as usize;

    h.lf_level = r.literal(6)? as u8;
    h.lf_sharpness = r.literal(3)? as u8;
    h.lf_delta_enabled = r.flag()?;
    if h.lf_delta_enabled && r.flag()? {
        for d in &mut h.lf_ref_delta {
            if r.flag()? {
                *d = Some(r.signed(6)? as i8);
            }
        }
        for d in &mut h.lf_mode_delta {
            if r.flag()? {
                *d = Some(r.signed(6)? as i8);
            }
        }
    }

    h.base_qindex = r.literal(8)? as u8;
    h.y_dc_delta = delta_q(&mut r)?;
    h.uv_dc_delta = delta_q(&mut r)?;
    h.uv_ac_delta = delta_q(&mut r)?;
    h.lossless = h.base_qindex == 0 && h.y_dc_delta == 0 && h.uv_dc_delta == 0 && h.uv_ac_delta == 0;

    h.segmentation = segmentation(&mut r)?;

    let sb_cols = width.div_ceil(64);
    let mut min_log2 = 0;
    while (64 << min_log2) < sb_cols {
        min_log2 += 1;
    }
    let mut max_log2 = 1;
    while (sb_cols >> max_log2) >= 4 {
        max_log2 += 1;
    }
    max_log2 -= 1;
    h.log2_tile_cols = min_log2;
    while h.log2_tile_cols < max_log2 && r.flag()? {
        h.log2_tile_cols += 1;
    }
    h.log2_tile_rows = r.bit()?;
    if h.log2_tile_rows != 0 {
        h.log2_tile_rows += r.bit()?;
    }

    h.compressed_bytes = r.literal(16)? as usize;
    if h.compressed_bytes == 0 {
        return Err(Error::invalid("a frame's compressed header is empty"));
    }
    h.header_bytes = r.bytes_read();
    Ok(h)
}

/// What the compressed header says besides the probabilities it updates.
pub struct Compressed {
    pub tx_mode: u8,
}

/// The compressed header: the frame's transform mode, and the changes to the
/// probabilities `p` it decodes with.
pub fn compressed(data: &[u8], h: &FrameHeader, p: &mut Probs) -> Result<Compressed> {
    let mut r = BoolDecoder::new(data)?;
    let tx_mode = if h.lossless {
        0
    } else {
        let mode = r.literal(2) as u8;
        if mode == 3 { mode + r.bit() as u8 } else { mode }
    };
    if tx_mode == TX_MODE_SELECT {
        for row in &mut p.tx8 {
            diff_update(&mut r, &mut row[0]);
        }
        for q in p.tx16.as_flattened_mut() {
            diff_update(&mut r, q);
        }
        for q in p.tx32.as_flattened_mut() {
            diff_update(&mut r, q);
        }
    }
    let biggest = [0, 1, 2, 3, 3][tx_mode as usize];
    for size in &mut p.coef[..=biggest] {
        if !r.bit() {
            continue;
        }
        for plane in size {
            for inter in plane {
                for (k, band) in inter.iter_mut().enumerate() {
                    for ctx in &mut band[..if k == 0 { 3 } else { 6 }] {
                        for q in ctx {
                            diff_update(&mut r, q);
                        }
                    }
                }
            }
        }
    }
    for q in &mut p.skip {
        diff_update(&mut r, q);
    }
    if !h.intra() {
        for q in p.inter_mode.as_flattened_mut() {
            diff_update(&mut r, q);
        }
        if h.interp_filter == SWITCHABLE {
            for q in p.interp_filter.as_flattened_mut() {
                diff_update(&mut r, q);
            }
        }
        for q in &mut p.intra_inter {
            diff_update(&mut r, q);
        }
        // A second reference is allowed where the references' sign biases
        // differ: only then is the choice coded.
        if (h.sign_bias[2] != h.sign_bias[1] || h.sign_bias[3] != h.sign_bias[1]) && r.bit() {
            return Err(Error::unsupported("compound prediction"));
        }
        for q in p.single_ref.as_flattened_mut() {
            diff_update(&mut r, q);
        }
        for q in p.y_mode.as_flattened_mut() {
            diff_update(&mut r, q);
        }
        for q in p.partition.as_flattened_mut() {
            diff_update(&mut r, q);
        }
        let mv_update = |r: &mut BoolDecoder, q: &mut u8| {
            if r.read(252) {
                *q = ((r.literal(7) << 1) | 1) as u8;
            }
        };
        for q in &mut p.mv_joints {
            mv_update(&mut r, q);
        }
        for comp in &mut p.mv {
            mv_update(&mut r, &mut comp.sign);
            for q in &mut comp.classes {
                mv_update(&mut r, q);
            }
            mv_update(&mut r, &mut comp.class0[0]);
            for q in &mut comp.bits {
                mv_update(&mut r, q);
            }
        }
        for comp in &mut p.mv {
            for q in comp.class0_fp.as_flattened_mut() {
                mv_update(&mut r, q);
            }
            for q in &mut comp.fp {
                mv_update(&mut r, q);
            }
        }
        if h.allow_hp {
            for comp in &mut p.mv {
                mv_update(&mut r, &mut comp.class0_hp);
                mv_update(&mut r, &mut comp.hp);
            }
        }
    }
    if r.overran() {
        return Err(Error::invalid("a frame's compressed header is cut short"));
    }
    Ok(Compressed { tx_mode })
}
