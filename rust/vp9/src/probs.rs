//! The probabilities a frame is decoded with, the counts it leaves, and the
//! adaptation of the one by the other.

use crate::bits::BoolDecoder;
use crate::tables::*;

pub const INTRA_MODE_TREE: [i8; 18] = [0, 2, -9, 4, -1, 6, 8, 12, -2, 10, -4, -5, -3, 14, -8, 16, -6, -7];
/// Leaves are the mode less NEARESTMV: 0 nearest, 1 near, 2 zero, 3 new.
pub const INTER_MODE_TREE: [i8; 6] = [-2, 2, 0, 4, -1, -3];
pub const PARTITION_TREE: [i8; 6] = [0, 2, -1, 4, -2, -3];
pub const INTERP_FILTER_TREE: [i8; 4] = [0, 2, -1, -2];
pub const MV_JOINT_TREE: [i8; 6] = [0, 2, -1, 4, -2, -3];
pub const MV_CLASS_TREE: [i8; 20] = [0, 2, -1, 4, 6, 8, -2, -3, 10, 12, -4, -5, -6, 14, 16, 18, -7, -8, -9, -10];
pub const MV_CLASS0_TREE: [i8; 2] = [0, -1];
pub const MV_FP_TREE: [i8; 6] = [0, 2, -1, 4, -2, -3];

/// [tx size][plane type][inter][band][context][node]
pub type CoefProbs = [[[[[[u8; 3]; 6]; 6]; 2]; 2]; 4];

#[derive(Clone, Copy)]
pub struct MvCompProbs {
    pub sign: u8,
    pub classes: [u8; 10],
    pub class0: [u8; 1],
    pub bits: [u8; 10],
    pub class0_fp: [[u8; 3]; 2],
    pub fp: [u8; 3],
    pub class0_hp: u8,
    pub hp: u8,
}

/// libvpx's FRAME_CONTEXT.
#[derive(Clone)]
pub struct Probs {
    pub y_mode: [[u8; 9]; 4],
    pub uv_mode: [[u8; 9]; 10],
    pub partition: [[u8; 3]; 16],
    pub coef: CoefProbs,
    pub interp_filter: [[u8; 2]; 4],
    pub inter_mode: [[u8; 3]; 7],
    pub intra_inter: [u8; 4],
    pub comp_inter: [u8; 5],
    pub single_ref: [[u8; 2]; 5],
    pub comp_ref: [u8; 5],
    pub tx8: [[u8; 1]; 2],
    pub tx16: [[u8; 2]; 2],
    pub tx32: [[u8; 3]; 2],
    pub skip: [u8; 3],
    pub mv_joints: [u8; 3],
    pub mv: [MvCompProbs; 2],
}

impl Default for Probs {
    fn default() -> Self {
        let comp = |classes, class0| MvCompProbs {
            sign: 128,
            classes,
            class0: [class0],
            bits: [136, 140, 148, 160, 176, 192, 224, 234, 234, 240],
            class0_fp: [[128, 128, 64], [96, 112, 64]],
            fp: [64, 96, 64],
            class0_hp: 160,
            hp: 128,
        };
        Probs {
            y_mode: DEFAULT_Y_MODE_PROBS,
            uv_mode: DEFAULT_UV_MODE_PROBS,
            partition: DEFAULT_PARTITION_PROBS,
            coef: [DEFAULT_COEF_PROBS_4X4, DEFAULT_COEF_PROBS_8X8, DEFAULT_COEF_PROBS_16X16, DEFAULT_COEF_PROBS_32X32],
            interp_filter: DEFAULT_INTERP_FILTER_PROBS,
            inter_mode: DEFAULT_INTER_MODE_PROBS,
            intra_inter: DEFAULT_INTRA_INTER_PROBS,
            comp_inter: DEFAULT_COMP_INTER_PROBS,
            single_ref: DEFAULT_SINGLE_REF_PROBS,
            comp_ref: DEFAULT_COMP_REF_PROBS,
            tx8: [[100], [66]],
            tx16: [[20, 152], [15, 101]],
            tx32: [[3, 136, 37], [5, 52, 13]],
            skip: DEFAULT_SKIP_PROBS,
            mv_joints: [32, 64, 96],
            mv: [comp([224, 144, 192, 168, 192, 176, 192, 198, 198, 245], 216), comp([216, 128, 176, 160, 176, 176, 192, 198, 198, 208], 208)],
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct MvCompCounts {
    pub sign: [u32; 2],
    pub classes: [u32; 11],
    pub class0: [u32; 2],
    pub bits: [[u32; 2]; 10],
    pub class0_fp: [[u32; 4]; 2],
    pub fp: [u32; 4],
    pub class0_hp: [u32; 2],
    pub hp: [u32; 2],
}

/// libvpx's FRAME_COUNTS: what a frame's symbols were, for the next frame's
/// probabilities.
#[derive(Clone, Default)]
pub struct Counts {
    pub y_mode: [[u32; 10]; 4],
    pub uv_mode: [[u32; 10]; 10],
    pub partition: [[u32; 4]; 16],
    /// Per token class: zero, one, two or more, end of block.
    pub coef: [[[[[[u32; 4]; 6]; 6]; 2]; 2]; 4],
    pub eob_branch: [[[[[u32; 6]; 6]; 2]; 2]; 4],
    pub interp_filter: [[u32; 3]; 4],
    pub inter_mode: [[u32; 4]; 7],
    pub intra_inter: [[u32; 2]; 4],
    pub single_ref: [[[u32; 2]; 2]; 5],
    pub tx8: [[u32; 2]; 2],
    pub tx16: [[u32; 3]; 2],
    pub tx32: [[u32; 4]; 2],
    pub skip: [[u32; 2]; 3],
    pub mv_joints: [u32; 4],
    pub mv: [MvCompCounts; 2],
}

impl Counts {
    /// Adds another tile's counts.
    pub fn add(&mut self, o: &Counts) {
        // Every field is u32s and nothing else, so the two are summed as such.
        let n = size_of::<Counts>() / size_of::<u32>();
        // SAFETY: Counts is arrays of u32 all the way down, with no padding.
        let (a, b) = unsafe { (std::slice::from_raw_parts_mut(self as *mut Counts as *mut u32, n), std::slice::from_raw_parts(o as *const Counts as *const u32, n)) };
        for (a, b) in a.iter_mut().zip(b) {
            *a = a.wrapping_add(*b);
        }
    }
}

fn inv_recenter_nonneg(v: i32, m: i32) -> i32 {
    if v > 2 * m {
        v
    } else if v & 1 != 0 {
        m - ((v + 1) >> 1)
    } else {
        m + (v >> 1)
    }
}

/// A probability's update in the compressed header: `vp9_diff_update_prob`.
pub fn diff_update(r: &mut BoolDecoder, p: &mut u8) {
    if !r.read(252) {
        return;
    }
    let delta = if !r.bit() {
        r.literal(4)
    } else if !r.bit() {
        r.literal(4) + 16
    } else if !r.bit() {
        r.literal(5) + 32
    } else {
        let v = r.literal(7);
        if v < 65 { v + 64 } else { (v << 1) - 1 + r.bit() as u32 }
    };
    let v = INV_MAP_TABLE[(delta as usize).min(INV_MAP_TABLE.len() - 1)] as i32;
    let m = *p as i32 - 1;
    *p = if (m << 1) <= 255 { 1 + inv_recenter_nonneg(v, m) } else { 255 - inv_recenter_nonneg(v, 254 - m) } as u8;
}

fn get_prob(num: u32, den: u32) -> u8 {
    (((num as u64 * 256 + (den as u64 >> 1)) / den as u64) as u32).clamp(1, 255) as u8
}

fn weighted(pre: u8, prob: u8, factor: u32) -> u8 {
    ((pre as u32 * (256 - factor) + prob as u32 * factor + 128) >> 8) as u8
}

/// A coefficient probability's adaptation.
fn merge(pre: u8, ct: [u32; 2], count_sat: u32, max_update: u32) -> u8 {
    let den = ct[0].wrapping_add(ct[1]);
    let prob = if den == 0 { 128 } else { get_prob(ct[0], den) };
    weighted(pre, prob, max_update * den.min(count_sat) / count_sat)
}

/// Every other probability's.
fn mode_merge(pre: u8, ct: [u32; 2]) -> u8 {
    const FACTOR: [u32; 21] = [0, 6, 12, 19, 25, 32, 38, 44, 51, 57, 64, 70, 76, 83, 89, 96, 102, 108, 115, 121, 128];
    let den = ct[0].wrapping_add(ct[1]);
    if den == 0 { pre } else { weighted(pre, get_prob(ct[0], den), FACTOR[den.min(20) as usize]) }
}

fn tree_merge(tree: &[i8], pre: &[u8], counts: &[u32], probs: &mut [u8]) {
    fn walk(i: usize, tree: &[i8], pre: &[u8], counts: &[u32], probs: &mut [u8]) -> u32 {
        let side = |t: i8, probs: &mut [u8]| if t <= 0 { counts[(-t) as usize] } else { walk(t as usize, tree, pre, counts, probs) };
        let left = side(tree[i], probs);
        let right = side(tree[i + 1], probs);
        probs[i >> 1] = mode_merge(pre[i >> 1], [left, right]);
        left.wrapping_add(right)
    }
    walk(0, tree, pre, counts, probs);
}

impl Probs {
    /// `vp9_adapt_coef_probs`.
    pub fn adapt_coef(&mut self, pre: &Probs, counts: &Counts, update_factor: u32) {
        for t in 0..4 {
            for i in 0..2 {
                for j in 0..2 {
                    for k in 0..6 {
                        for l in 0..if k == 0 { 3 } else { 6 } {
                            let c = &counts.coef[t][i][j][k][l];
                            let neob = c[3];
                            let branch = [[neob, counts.eob_branch[t][i][j][k][l].wrapping_sub(neob)], [c[0], c[1].wrapping_add(c[2])], [c[1], c[2]]];
                            for m in 0..3 {
                                self.coef[t][i][j][k][l][m] = merge(pre.coef[t][i][j][k][l][m], branch[m], 24, update_factor);
                            }
                        }
                    }
                }
            }
        }
    }

    /// `vp9_adapt_mode_probs` and `vp9_adapt_mv_probs`, for an inter frame.
    pub fn adapt_modes(&mut self, pre: &Probs, counts: &Counts, switchable_interp: bool, tx_select: bool, allow_hp: bool) {
        for i in 0..4 {
            self.intra_inter[i] = mode_merge(pre.intra_inter[i], counts.intra_inter[i]);
        }
        for i in 0..5 {
            for j in 0..2 {
                self.single_ref[i][j] = mode_merge(pre.single_ref[i][j], counts.single_ref[i][j]);
            }
        }
        for i in 0..7 {
            tree_merge(&INTER_MODE_TREE, &pre.inter_mode[i], &counts.inter_mode[i], &mut self.inter_mode[i]);
        }
        for i in 0..4 {
            tree_merge(&INTRA_MODE_TREE, &pre.y_mode[i], &counts.y_mode[i], &mut self.y_mode[i]);
        }
        for i in 0..10 {
            tree_merge(&INTRA_MODE_TREE, &pre.uv_mode[i], &counts.uv_mode[i], &mut self.uv_mode[i]);
        }
        for i in 0..16 {
            tree_merge(&PARTITION_TREE, &pre.partition[i], &counts.partition[i], &mut self.partition[i]);
        }
        if switchable_interp {
            for i in 0..4 {
                tree_merge(&INTERP_FILTER_TREE, &pre.interp_filter[i], &counts.interp_filter[i], &mut self.interp_filter[i]);
            }
        }
        if tx_select {
            for i in 0..2 {
                let c = counts.tx8[i];
                self.tx8[i][0] = mode_merge(pre.tx8[i][0], [c[0], c[1]]);
                let c = counts.tx16[i];
                self.tx16[i][0] = mode_merge(pre.tx16[i][0], [c[0], c[1].wrapping_add(c[2])]);
                self.tx16[i][1] = mode_merge(pre.tx16[i][1], [c[1], c[2]]);
                let c = counts.tx32[i];
                self.tx32[i][0] = mode_merge(pre.tx32[i][0], [c[0], c[1].wrapping_add(c[2]).wrapping_add(c[3])]);
                self.tx32[i][1] = mode_merge(pre.tx32[i][1], [c[1], c[2].wrapping_add(c[3])]);
                self.tx32[i][2] = mode_merge(pre.tx32[i][2], [c[2], c[3]]);
            }
        }
        for i in 0..3 {
            self.skip[i] = mode_merge(pre.skip[i], counts.skip[i]);
        }

        tree_merge(&MV_JOINT_TREE, &pre.mv_joints, &counts.mv_joints, &mut self.mv_joints);
        for i in 0..2 {
            let (comp, pre, c) = (&mut self.mv[i], &pre.mv[i], &counts.mv[i]);
            comp.sign = mode_merge(pre.sign, c.sign);
            tree_merge(&MV_CLASS_TREE, &pre.classes, &c.classes, &mut comp.classes);
            tree_merge(&MV_CLASS0_TREE, &pre.class0, &c.class0, &mut comp.class0);
            for j in 0..10 {
                comp.bits[j] = mode_merge(pre.bits[j], c.bits[j]);
            }
            for j in 0..2 {
                tree_merge(&MV_FP_TREE, &pre.class0_fp[j], &c.class0_fp[j], &mut comp.class0_fp[j]);
            }
            tree_merge(&MV_FP_TREE, &pre.fp, &c.fp, &mut comp.fp);
            if allow_hp {
                comp.class0_hp = mode_merge(pre.class0_hp, c.class0_hp);
                comp.hp = mode_merge(pre.hp, c.hp);
            }
        }
    }
}
