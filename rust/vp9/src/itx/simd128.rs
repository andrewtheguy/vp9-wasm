//! The transforms of `itx` in WebAssembly vectors, exact to the plain ones
//! for every input: the DCT and ADST at 4, 8 and 16 points, and the add of a
//! block whose only coefficient is the DC.
//!
//! A pass runs over its rows at once, a row to a lane: the block is turned
//! about its diagonal, the pass runs over its vectors, and it is turned back
//! for the next. A rotation's two products are one dot product of a pair of
//! lanes with a pair of constants, exact in 32 bits, rounded there and taken
//! back to 16 as the plain `rs` takes them, the low half of the word. Where
//! one of the pair is known to be zero, past the coefficients a short block
//! has, each product is one multiply that rounds as `rs` does.

use core::arch::wasm32::*;

use super::{
    C1, C2, C3, C4, C5, C6, C7, C8, C9, C10, C11, C12, C13, C14, C15, C16, C17, C18, C19, C20, C21, C22, C23, C24, C25, C26, C27, C28, C29, C30,
    C31, SINPI_1_9, SINPI_2_9, SINPI_3_9, SINPI_4_9,
};

/// `c0, c1` in alternate lanes: what a pair of lanes is multiplied by.
const fn pair(c0: i32, c1: i32) -> v128 {
    let (c0, c1) = (c0 as i16, c1 as i16);
    i16x8(c0, c1, c0, c1, c0, c1, c0, c1)
}

/// Pairs of lanes `(a_i, b_i)`: the low four pairs and the high four.
#[inline(always)]
fn pairs(a: v128, b: v128) -> (v128, v128) {
    (i16x8_shuffle::<0, 8, 1, 9, 2, 10, 3, 11>(a, b), i16x8_shuffle::<4, 12, 5, 13, 6, 14, 7, 15>(a, b))
}

/// `rs` of eight 32-bit sums, four in each: rounded, shifted and taken as 16
/// bits.
#[inline(always)]
fn rs2(lo: v128, hi: v128) -> v128 {
    let round = i32x4_splat(1 << 13);
    let lo = i32x4_shr(i32x4_add(lo, round), 14);
    let hi = i32x4_shr(i32x4_add(hi, round), 14);
    i16x8_shuffle::<0, 2, 4, 6, 8, 10, 12, 14>(lo, hi)
}

/// `rs(a·k[0] + b·k[1])` over the eight lanes, the pairs made already.
#[inline(always)]
fn dot(lo: v128, hi: v128, k: v128) -> v128 {
    rs2(i32x4_dot_i16x8(lo, k), i32x4_dot_i16x8(hi, k))
}

/// The plain `rot`: `(rs(a*c0 - b*c1), rs(a*c1 + b*c0))` over eight lanes.
#[inline(always)]
fn rot(a: v128, b: v128, c0: i32, c1: i32) -> (v128, v128) {
    let (lo, hi) = pairs(a, b);
    (dot(lo, hi, pair(c0, -c1)), dot(lo, hi, pair(c1, c0)))
}

/// The plain `half`: `(rs((a + b)*C16), rs((a - b)*C16))`.
#[inline(always)]
fn half(a: v128, b: v128) -> (v128, v128) {
    let (lo, hi) = pairs(a, b);
    (dot(lo, hi, pair(C16, C16)), dot(lo, hi, pair(C16, -C16)))
}

/// The plain `rot(-a, b, c0, c1)`, `-a` taken as the plain takes it, in 32
/// bits, where a lane's negation of -32768 would be -32768.
#[inline(always)]
fn rot_neg(a: v128, b: v128, c0: i32, c1: i32) -> (v128, v128) {
    let (lo, hi) = pairs(a, b);
    (dot(lo, hi, pair(-c0, -c1)), dot(lo, hi, pair(-c1, c0)))
}

/// An input of a pass: `None` where the block's coefficients stop short of
/// it, so that it is zero.
type In = Option<v128>;

/// The first `K` of `N` inputs, the rest zero. Filled in a loop: the same
/// array made by `core::array::from_fn` crashes LLVM 22 and 23 at -O2 and
/// above once the 16×16's shorter blocks are inlined together.
#[inline(always)]
fn lead<const N: usize, const K: usize>(v: [v128; K]) -> [In; N] {
    let mut o = [None; N];
    let mut k = 0;
    while k < K {
        o[k] = Some(v[k]);
        k += 1;
    }
    o
}

/// `rs(a·c)` over eight lanes, for a constant `c` under 2^14 either way: the
/// rounded high half of `a·2c`, which is `(a·c + 2^13) >> 14`, and which never
/// saturates, `2c` being no -32768.
#[inline(always)]
fn mulc(a: v128, c: i32) -> v128 {
    i16x8_q15mulr_sat(a, i16x8_splat((2 * c) as i16))
}

/// `rot`, a product of an input known to be zero left out.
#[inline(always)]
fn rot_z(a: In, b: In, c0: i32, c1: i32) -> (v128, v128) {
    match (a, b) {
        (Some(a), Some(b)) => rot(a, b, c0, c1),
        (Some(a), None) => (mulc(a, c0), mulc(a, c1)),
        (None, Some(b)) => (mulc(b, -c1), mulc(b, c0)),
        (None, None) => (i16x8_splat(0), i16x8_splat(0)),
    }
}

/// `half`, as `rot_z`.
#[inline(always)]
fn half_z(a: In, b: In) -> (v128, v128) {
    match (a, b) {
        (Some(a), Some(b)) => half(a, b),
        (Some(a), None) => {
            let h = mulc(a, C16);
            (h, h)
        }
        (None, Some(b)) => (mulc(b, C16), mulc(b, -C16)),
        (None, None) => (i16x8_splat(0), i16x8_splat(0)),
    }
}

/// `idct8` over eight lanes, each input a vector.
#[inline(always)]
fn idct8(i: [v128; 8]) -> [v128; 8] {
    idct8_z(i.map(Some))
}

/// `idct8` over eight lanes, of inputs some of which may be zero.
#[inline(always)]
fn idct8_z(i: [In; 8]) -> [v128; 8] {
    // The even half, `idct4` of i0, i2, i4 and i6.
    let (s0, s1) = half_z(i[0], i[4]);
    let (s2, s3) = rot_z(i[2], i[6], C24, C8);
    let e = [i16x8_add(s0, s3), i16x8_add(s1, s2), i16x8_sub(s1, s2), i16x8_sub(s0, s3)];
    let (s4, s7) = rot_z(i[1], i[7], C28, C4);
    let (s5, s6) = rot_z(i[5], i[3], C12, C20);
    let (t4, t5, t6, t7) = (i16x8_add(s4, s5), i16x8_sub(s4, s5), i16x8_sub(s7, s6), i16x8_add(s6, s7));
    let (u6, u5) = half(t6, t5);
    let o = [t4, u5, u6, t7];
    core::array::from_fn(|k| if k < 4 { i16x8_add(e[k], o[3 - k]) } else { i16x8_sub(e[7 - k], o[k - 4]) })
}

/// `idct16` over eight lanes, of inputs some of which may be zero.
#[inline(always)]
fn idct16_z(i: [In; 16]) -> [v128; 16] {
    let e = idct8_z(core::array::from_fn(|k| i[2 * k]));
    let (add, sub) = (i16x8_add, i16x8_sub);
    // stage 2
    let (s8, s15) = rot_z(i[1], i[15], C30, C2);
    let (s9, s14) = rot_z(i[9], i[7], C14, C18);
    let (s10, s13) = rot_z(i[5], i[11], C22, C10);
    let (s11, s12) = rot_z(i[13], i[3], C6, C26);
    // stage 3
    let (t8, t9, t10, t11) = (add(s8, s9), sub(s8, s9), sub(s11, s10), add(s10, s11));
    let (t12, t13, t14, t15) = (add(s12, s13), sub(s12, s13), sub(s15, s14), add(s14, s15));
    // stage 4
    let (u9, u14) = rot(t14, t9, C24, C8);
    let (u10, u13) = rot_neg(t10, t13, C24, C8);
    // stage 5
    let (v8, v9, v10, v11) = (add(t8, t11), add(u9, u10), sub(u9, u10), sub(t8, t11));
    let (v12, v13, v14, v15) = (sub(t15, t12), sub(u14, u13), add(u13, u14), add(t12, t15));
    // stage 6
    let (x13, x10) = half(v13, v10);
    let (x12, x11) = half(v12, v11);
    let o = [v8, v9, x10, x11, x12, x13, v14, v15];
    // stage 7
    core::array::from_fn(|k| if k < 8 { add(e[k], o[7 - k]) } else { sub(e[15 - k], o[k - 8]) })
}

/// A 4×4 block as two vectors of two lines each: the halves of `v0` are
/// lines 0 and 1, of `v1` lines 2 and 3. Turned about its diagonal.
#[inline(always)]
fn transpose4(v0: v128, v1: v128) -> (v128, v128) {
    (i16x8_shuffle::<0, 4, 8, 12, 1, 5, 9, 13>(v0, v1), i16x8_shuffle::<2, 6, 10, 14, 3, 7, 11, 15>(v0, v1))
}

/// The halves of a vector the other way about.
#[inline(always)]
fn swap(v: v128) -> v128 {
    i64x2_shuffle::<1, 0>(v, v)
}

/// `idct4` over four lanes, the inputs two to a vector: `[x0 | x1]` and
/// `[x2 | x3]` to `[o0 | o1]` and `[o2 | o3]`.
#[inline(always)]
fn idct4x2(v0: v128, v1: v128) -> (v128, v128) {
    // Pairs (x0, x2), then (x1, x3).
    let (p, q) = pairs(v0, v1);
    let s01 = rs2(i32x4_dot_i16x8(p, pair(C16, C16)), i32x4_dot_i16x8(p, pair(C16, -C16)));
    let s32 = rs2(i32x4_dot_i16x8(q, pair(C8, C24)), i32x4_dot_i16x8(q, pair(C24, -C8)));
    (i16x8_add(s01, s32), swap(i16x8_sub(s01, s32)))
}

/// `iadst4` over four lanes, laid out as `idct4x2`'s: its sums are 32 bits
/// wide to the end, wrapping as the plain one's do.
#[inline(always)]
fn iadst4x2(v0: v128, v1: v128) -> (v128, v128) {
    let (p, q) = pairs(v0, v1);
    let (c1, c2, c3, c4) = (SINPI_1_9, SINPI_2_9, SINPI_3_9, SINPI_4_9);
    let s0 = i32x4_add(i32x4_dot_i16x8(p, pair(c1, c4)), i32x4_dot_i16x8(q, pair(0, c2)));
    let s1 = i32x4_add(i32x4_dot_i16x8(p, pair(c2, -c1)), i32x4_dot_i16x8(q, pair(0, -c4)));
    let s2 = i32x4_add(i32x4_dot_i16x8(p, pair(c3, -c3)), i32x4_dot_i16x8(q, pair(0, c3)));
    let s3 = i32x4_dot_i16x8(q, pair(c3, 0));
    (rs2(i32x4_add(s0, s3), i32x4_add(s1, s3)), rs2(s2, i32x4_sub(i32x4_add(s0, s1), s3)))
}

/// Rounds the outputs of a pass by `shift` and adds them to the block's rows,
/// eight samples of each, two rows to a vector: narrowed apart, each with
/// zeros, LLVM clamps the sums before the narrowing that clamps them already.
/// The round-up saturates where the plain one's does not, which the clip
/// then hides: a sum that near the top is clipped either way.
#[inline(always)]
unsafe fn add_rows<const N: usize>(out: [v128; N], dst: *mut u8, stride: usize, shift: u32) {
    let round = i16x8_splat(1 << (shift - 1));
    for r in (0..N).step_by(2) {
        let (v0, v1) = (i16x8_shr(i16x8_add_sat(out[r], round), shift), i16x8_shr(i16x8_add_sat(out[r + 1], round), shift));
        unsafe {
            let (d0, d1) = (dst.add(r * stride), dst.add((r + 1) * stride));
            let s = v128_load64_lane::<1>(v128_load64_zero(d0 as *const u64), d1 as *const u64);
            let o = u8x16_narrow_i16x8(i16x8_add(u16x8_extend_low_u8x16(s), v0), i16x8_add(u16x8_extend_high_u8x16(s), v1));
            v128_store64_lane::<0>(o, d0 as *mut u64);
            v128_store64_lane::<1>(o, d1 as *mut u64);
        }
    }
}

/// The 4×4 transforms, `row` over the rows then `col` over the columns, added
/// to the block at `dst`.
///
/// # Safety
/// `coeffs` holds 16 coefficients; the 4×4 block at `dst` is writable.
#[inline(always)]
unsafe fn add4x4(coeffs: *const i16, row: impl Fn(v128, v128) -> (v128, v128), col: impl Fn(v128, v128) -> (v128, v128), dst: *mut u8, stride: usize) {
    unsafe {
        let (v0, v1) = (v128_load(coeffs as *const v128), v128_load(coeffs.add(8) as *const v128));
        let (t0, t1) = transpose4(v0, v1);
        let (r0, r1) = row(t0, t1);
        let (u0, u1) = transpose4(r0, r1);
        let (o0, o1) = col(u0, u1);
        let eight = i16x8_splat(8);
        let (o0, o1) = (i16x8_shr(i16x8_add_sat(o0, eight), 4), i16x8_shr(i16x8_add_sat(o1, eight), 4));
        let mut d = v128_load32_zero(dst as *const u32);
        d = v128_load32_lane::<1>(d, dst.add(stride) as *const u32);
        d = v128_load32_lane::<2>(d, dst.add(2 * stride) as *const u32);
        d = v128_load32_lane::<3>(d, dst.add(3 * stride) as *const u32);
        let lo = i16x8_add(u16x8_extend_low_u8x16(d), o0);
        let hi = i16x8_add(u16x8_extend_high_u8x16(d), o1);
        let o = u8x16_narrow_i16x8(lo, hi);
        v128_store32_lane::<0>(o, dst as *mut u32);
        v128_store32_lane::<1>(o, dst.add(stride) as *mut u32);
        v128_store32_lane::<2>(o, dst.add(2 * stride) as *mut u32);
        v128_store32_lane::<3>(o, dst.add(3 * stride) as *mut u32);
    }
}

/// The 4×4 transform of `tx_type`, added to the block at `dst`.
///
/// # Safety
/// `coeffs` holds 16 coefficients; the 4×4 block at `dst` is writable.
pub unsafe fn add_4x4(tx_type: usize, coeffs: *const i16, dst: *mut u8, stride: usize) {
    unsafe {
        match tx_type {
            0 => add4x4(coeffs, idct4x2, idct4x2, dst, stride),
            1 => add4x4(coeffs, idct4x2, iadst4x2, dst, stride),
            2 => add4x4(coeffs, iadst4x2, idct4x2, dst, stride),
            _ => add4x4(coeffs, iadst4x2, iadst4x2, dst, stride),
        }
    }
}

/// An 8×8 block of eight vectors, a line each, turned about its diagonal.
#[inline(always)]
fn transpose8(v: [v128; 8]) -> [v128; 8] {
    // Lanes paired, then pairs paired, then fours paired.
    let (a0, a1) = pairs(v[0], v[1]);
    let (a2, a3) = pairs(v[2], v[3]);
    let (a4, a5) = pairs(v[4], v[5]);
    let (a6, a7) = pairs(v[6], v[7]);
    let b0 = i32x4_shuffle::<0, 4, 1, 5>(a0, a2);
    let b1 = i32x4_shuffle::<2, 6, 3, 7>(a0, a2);
    let b2 = i32x4_shuffle::<0, 4, 1, 5>(a1, a3);
    let b3 = i32x4_shuffle::<2, 6, 3, 7>(a1, a3);
    let b4 = i32x4_shuffle::<0, 4, 1, 5>(a4, a6);
    let b5 = i32x4_shuffle::<2, 6, 3, 7>(a4, a6);
    let b6 = i32x4_shuffle::<0, 4, 1, 5>(a5, a7);
    let b7 = i32x4_shuffle::<2, 6, 3, 7>(a5, a7);
    [
        i64x2_shuffle::<0, 2>(b0, b4),
        i64x2_shuffle::<1, 3>(b0, b4),
        i64x2_shuffle::<0, 2>(b1, b5),
        i64x2_shuffle::<1, 3>(b1, b5),
        i64x2_shuffle::<0, 2>(b2, b6),
        i64x2_shuffle::<1, 3>(b2, b6),
        i64x2_shuffle::<0, 2>(b3, b7),
        i64x2_shuffle::<1, 3>(b3, b7),
    ]
}

/// `idct16` over eight lanes, each input a vector.
#[inline(always)]
fn idct16(i: [v128; 16]) -> [v128; 16] {
    idct16_z(i.map(Some))
}

/// Eight lanes of 32 bits, four to a vector: the ADST's intermediate, which
/// the plain keeps at 32 bits, wrapping where it does.
#[derive(Clone, Copy)]
struct Wide(v128, v128);

impl core::ops::Add for Wide {
    type Output = Wide;
    #[inline(always)]
    fn add(self, b: Wide) -> Wide {
        Wide(i32x4_add(self.0, b.0), i32x4_add(self.1, b.1))
    }
}

impl core::ops::Sub for Wide {
    type Output = Wide;
    #[inline(always)]
    fn sub(self, b: Wide) -> Wide {
        Wide(i32x4_sub(self.0, b.0), i32x4_sub(self.1, b.1))
    }
}

impl Wide {
    /// Times a constant.
    #[inline(always)]
    fn mul(self, c: i32) -> Wide {
        Wide(i32x4_mul(self.0, i32x4_splat(c)), i32x4_mul(self.1, i32x4_splat(c)))
    }

    /// The plain `rsw`.
    #[inline(always)]
    fn rsw(self) -> Wide {
        let round = i32x4_splat(1 << 13);
        Wide(i32x4_shr(i32x4_add(self.0, round), 14), i32x4_shr(i32x4_add(self.1, round), 14))
    }

    /// The low 16 bits of each lane, as the plain `w` takes them.
    #[inline(always)]
    fn low(self) -> v128 {
        i16x8_shuffle::<0, 2, 4, 6, 8, 10, 12, 14>(self.0, self.1)
    }

    /// `-self`'s low 16 bits.
    #[inline(always)]
    fn neg_low(self) -> v128 {
        i16x8_neg(self.low())
    }
}

/// The plain `bf` of 16-bit inputs, its products exact in a dot product.
#[inline(always)]
fn bf16(a: v128, b: v128, c0: i32, c1: i32) -> (Wide, Wide) {
    let (lo, hi) = pairs(a, b);
    let (k0, k1) = (pair(c0, c1), pair(c1, -c0));
    (Wide(i32x4_dot_i16x8(lo, k0), i32x4_dot_i16x8(hi, k0)), Wide(i32x4_dot_i16x8(lo, k1), i32x4_dot_i16x8(hi, k1)))
}

/// The plain `bf`.
#[inline(always)]
fn bf(a: Wide, b: Wide, c0: i32, c1: i32) -> (Wide, Wide) {
    (a.mul(c0) + b.mul(c1), a.mul(c1) - b.mul(c0))
}

/// The plain `bf2`.
#[inline(always)]
fn bf2(a: Wide, b: Wide, c0: i32, c1: i32) -> (Wide, Wide) {
    (b.mul(c1) - a.mul(c0), a.mul(c1) + b.mul(c0))
}

/// `iadst8` over eight lanes.
#[inline(always)]
fn iadst8(x: [v128; 8]) -> [v128; 8] {
    // stage 1
    let (s0, s1) = bf16(x[7], x[0], C2, C30);
    let (s2, s3) = bf16(x[5], x[2], C10, C22);
    let (s4, s5) = bf16(x[3], x[4], C18, C14);
    let (s6, s7) = bf16(x[1], x[6], C26, C6);
    let (x0, x1, x2, x3) = ((s0 + s4).rsw(), (s1 + s5).rsw(), (s2 + s6).rsw(), (s3 + s7).rsw());
    let (x4, x5, x6, x7) = ((s0 - s4).rsw(), (s1 - s5).rsw(), (s2 - s6).rsw(), (s3 - s7).rsw());
    // stage 2
    let (s4, s5) = bf(x4, x5, C8, C24);
    let (s6, s7) = bf2(x6, x7, C24, C8);
    let (x0, x1, x2, x3) = (x0 + x2, x1 + x3, x0 - x2, x1 - x3);
    let (x4, x5, x6, x7) = ((s4 + s6).rsw(), (s5 + s7).rsw(), (s4 - s6).rsw(), (s5 - s7).rsw());
    // stage 3
    let (y2, y3) = ((x2 + x3).mul(C16).rsw(), (x2 - x3).mul(C16).rsw());
    let (y6, y7) = ((x6 + x7).mul(C16).rsw(), (x6 - x7).mul(C16).rsw());
    [x0.low(), x4.neg_low(), y6.low(), y2.neg_low(), y3.low(), y7.neg_low(), x5.low(), x1.neg_low()]
}

/// `iadst16` over eight lanes.
#[inline(always)]
fn iadst16(x: [v128; 16]) -> [v128; 16] {
    // stage 1
    let (s0, s1) = bf16(x[15], x[0], C1, C31);
    let (s2, s3) = bf16(x[13], x[2], C5, C27);
    let (s4, s5) = bf16(x[11], x[4], C9, C23);
    let (s6, s7) = bf16(x[9], x[6], C13, C19);
    let (s8, s9) = bf16(x[7], x[8], C17, C15);
    let (s10, s11) = bf16(x[5], x[10], C21, C11);
    let (s12, s13) = bf16(x[3], x[12], C25, C7);
    let (s14, s15) = bf16(x[1], x[14], C29, C3);
    let (x0, x1, x2, x3) = ((s0 + s8).rsw(), (s1 + s9).rsw(), (s2 + s10).rsw(), (s3 + s11).rsw());
    let (x4, x5, x6, x7) = ((s4 + s12).rsw(), (s5 + s13).rsw(), (s6 + s14).rsw(), (s7 + s15).rsw());
    let (x8, x9, x10, x11) = ((s0 - s8).rsw(), (s1 - s9).rsw(), (s2 - s10).rsw(), (s3 - s11).rsw());
    let (x12, x13, x14, x15) = ((s4 - s12).rsw(), (s5 - s13).rsw(), (s6 - s14).rsw(), (s7 - s15).rsw());
    // stage 2
    let (s0, s1, s2, s3, s4, s5, s6, s7) = (x0, x1, x2, x3, x4, x5, x6, x7);
    let (s8, s9) = bf(x8, x9, C4, C28);
    let (s10, s11) = bf(x10, x11, C20, C12);
    let (s12, s13) = bf2(x12, x13, C28, C4);
    let (s14, s15) = bf2(x14, x15, C12, C20);
    let (x0, x1, x2, x3) = (s0 + s4, s1 + s5, s2 + s6, s3 + s7);
    let (x4, x5, x6, x7) = (s0 - s4, s1 - s5, s2 - s6, s3 - s7);
    let (x8, x9, x10, x11) = ((s8 + s12).rsw(), (s9 + s13).rsw(), (s10 + s14).rsw(), (s11 + s15).rsw());
    let (x12, x13, x14, x15) = ((s8 - s12).rsw(), (s9 - s13).rsw(), (s10 - s14).rsw(), (s11 - s15).rsw());
    // stage 3
    let (s0, s1, s2, s3) = (x0, x1, x2, x3);
    let (s4, s5) = bf(x4, x5, C8, C24);
    let (s6, s7) = bf2(x6, x7, C24, C8);
    let (s8, s9, s10, s11) = (x8, x9, x10, x11);
    let (s12, s13) = bf(x12, x13, C8, C24);
    let (s14, s15) = bf2(x14, x15, C24, C8);
    let (x0, x1, x2, x3) = (s0 + s2, s1 + s3, s0 - s2, s1 - s3);
    let (x4, x5, x6, x7) = ((s4 + s6).rsw(), (s5 + s7).rsw(), (s4 - s6).rsw(), (s5 - s7).rsw());
    let (x8, x9, x10, x11) = (s8 + s10, s9 + s11, s8 - s10, s9 - s11);
    let (x12, x13, x14, x15) = ((s12 + s14).rsw(), (s13 + s15).rsw(), (s12 - s14).rsw(), (s13 - s15).rsw());
    // stage 4
    let (y2, y3) = ((x2 + x3).mul(-C16).rsw(), (x2 - x3).mul(C16).rsw());
    let (y6, y7) = ((x6 + x7).mul(C16).rsw(), (x7 - x6).mul(C16).rsw());
    let (y10, y11) = ((x10 + x11).mul(C16).rsw(), (x11 - x10).mul(C16).rsw());
    let (y14, y15) = ((x14 + x15).mul(-C16).rsw(), (x14 - x15).mul(C16).rsw());
    [
        x0.low(),
        x8.neg_low(),
        x12.low(),
        x4.neg_low(),
        y6.low(),
        y14.low(),
        y10.low(),
        y2.low(),
        y3.low(),
        y11.low(),
        y15.low(),
        y7.low(),
        x5.low(),
        x13.neg_low(),
        x9.low(),
        x1.neg_low(),
    ]
}

/// The first four lanes of four vectors turned about their diagonal: lane j of
/// the k-th returned is lane k of the j-th given, for j and k under 4. The
/// upper lanes given are not read, and those returned are not to be.
#[inline(always)]
fn transpose4_lo(v: [v128; 4]) -> [v128; 4] {
    let a = i16x8_shuffle::<0, 8, 1, 9, 2, 10, 3, 11>(v[0], v[1]);
    let b = i16x8_shuffle::<0, 8, 1, 9, 2, 10, 3, 11>(v[2], v[3]);
    let (q0, q1) = (i32x4_shuffle::<0, 4, 1, 5>(a, b), i32x4_shuffle::<2, 6, 3, 7>(a, b));
    [q0, swap(q0), q1, swap(q1)]
}

/// The first four lanes of eight vectors turned about their diagonal, to four
/// whole vectors: lane j of the k-th returned is lane k of the j-th given. The
/// upper lanes given are not read.
#[inline(always)]
fn transpose8x4(v: [v128; 8]) -> [v128; 4] {
    let a = i16x8_shuffle::<0, 8, 1, 9, 2, 10, 3, 11>(v[0], v[1]);
    let b = i16x8_shuffle::<0, 8, 1, 9, 2, 10, 3, 11>(v[2], v[3]);
    let c = i16x8_shuffle::<0, 8, 1, 9, 2, 10, 3, 11>(v[4], v[5]);
    let d = i16x8_shuffle::<0, 8, 1, 9, 2, 10, 3, 11>(v[6], v[7]);
    let (x0, x1) = (i32x4_shuffle::<0, 4, 1, 5>(a, b), i32x4_shuffle::<2, 6, 3, 7>(a, b));
    let (y0, y1) = (i32x4_shuffle::<0, 4, 1, 5>(c, d), i32x4_shuffle::<2, 6, 3, 7>(c, d));
    [i64x2_shuffle::<0, 2>(x0, y0), i64x2_shuffle::<1, 3>(x0, y0), i64x2_shuffle::<0, 2>(x1, y1), i64x2_shuffle::<1, 3>(x1, y1)]
}

/// Eight of the 16 vectors of `v`, from `at`.
#[inline(always)]
fn eight(v: &[v128; 16], at: usize) -> [v128; 8] {
    core::array::from_fn(|k| v[at + k])
}

/// Two eights as one sixteen.
#[inline(always)]
fn sixteen(a: [v128; 8], b: [v128; 8]) -> [v128; 16] {
    core::array::from_fn(|k| if k < 8 { a[k] } else { b[k - 8] })
}

/// The 8×8 transform of `tx_type`, added to the block at `dst`; a DCT's of the
/// top-left 4×4 alone at an `eob` of 12 or less.
///
/// # Safety
/// `coeffs` holds 64 coefficients; the 8×8 block at `dst` is writable.
pub unsafe fn add_8x8(tx_type: usize, coeffs: *const i16, eob: usize, dst: *mut u8, stride: usize) {
    unsafe {
        let rows = |r: usize| v128_load(coeffs.add(8 * r) as *const v128);
        let both = |row: fn([v128; 8]) -> [v128; 8], col: fn([v128; 8]) -> [v128; 8]| {
            col(transpose8(row(transpose8(core::array::from_fn(rows)))))
        };
        let out = match tx_type {
            0 if eob <= 12 => idct8_z(lead(transpose8x4(idct8_z(lead(transpose4_lo(core::array::from_fn(rows))))))),
            0 => both(idct8, idct8),
            1 => both(idct8, iadst8),
            2 => both(iadst8, idct8),
            _ => both(iadst8, iadst8),
        };
        add_rows(out, dst, stride, 5);
    }
}

/// The 16×16 transform of `tx_type`, added to the block at `dst`; a DCT's of
/// the top-left 4×4 alone at an `eob` of 10 or less, of the top-left 8×8 at 38
/// or less. The rows' pass gives each column of the rows it runs over as a
/// vector, eight rows to it; the columns' pass runs over eight columns at a
/// time.
///
/// # Safety
/// `coeffs` holds 256 coefficients; the 16×16 block at `dst` is writable.
pub unsafe fn add_16x16(tx_type: usize, coeffs: *const i16, eob: usize, dst: *mut u8, stride: usize) {
    unsafe {
        let rows = |r: usize, x: usize| v128_load(coeffs.add(16 * r + x) as *const v128);
        let add = |h: usize, out: [v128; 16]| add_rows(out, dst.add(8 * h), stride, 6);
        let both = |row: fn([v128; 16]) -> [v128; 16], col: fn([v128; 16]) -> [v128; 16]| {
            let o: [[v128; 16]; 2] = core::array::from_fn(|g| {
                let left = transpose8(core::array::from_fn(|r| rows(8 * g + r, 0)));
                let right = transpose8(core::array::from_fn(|r| rows(8 * g + r, 8)));
                row(sixteen(left, right))
            });
            for h in 0..2 {
                add(h, col(sixteen(transpose8(eight(&o[0], 8 * h)), transpose8(eight(&o[1], 8 * h)))));
            }
        };
        match tx_type {
            0 if eob <= 10 => {
                let o = idct16_z(lead(transpose4_lo(core::array::from_fn(|r| rows(r, 0)))));
                for h in 0..2 {
                    add(h, idct16_z(lead(transpose8x4(eight(&o, 8 * h)))));
                }
            }
            0 if eob <= 38 => {
                let o = idct16_z(lead(transpose8(core::array::from_fn(|r| rows(r, 0)))));
                for h in 0..2 {
                    add(h, idct16_z(lead(transpose8(eight(&o, 8 * h)))));
                }
            }
            0 => both(idct16, idct16),
            1 => both(idct16, iadst16),
            2 => both(iadst16, idct16),
            _ => both(iadst16, iadst16),
        }
    }
}

/// Adds `a1` to every sample of the `N`×`N` block at `dst`, clipped: the plain
/// `add_dc`'s add, as a saturating add of what of `a1` is above zero and a
/// saturating subtraction of what is below, one of which is nothing.
///
/// # Safety
/// The block at `dst` is writable.
pub unsafe fn add_dc<const N: usize>(a1: i32, dst: *mut u8, stride: usize) {
    let (up, down) = (u8x16_splat(a1.clamp(0, 255) as u8), u8x16_splat((-a1).clamp(0, 255) as u8));
    let add = |s: v128| u8x16_sub_sat(u8x16_add_sat(s, up), down);
    unsafe {
        if N == 4 {
            let mut s = v128_load32_zero(dst as *const u32);
            s = v128_load32_lane::<1>(s, dst.add(stride) as *const u32);
            s = v128_load32_lane::<2>(s, dst.add(2 * stride) as *const u32);
            s = v128_load32_lane::<3>(s, dst.add(3 * stride) as *const u32);
            let o = add(s);
            v128_store32_lane::<0>(o, dst as *mut u32);
            v128_store32_lane::<1>(o, dst.add(stride) as *mut u32);
            v128_store32_lane::<2>(o, dst.add(2 * stride) as *mut u32);
            v128_store32_lane::<3>(o, dst.add(3 * stride) as *mut u32);
        } else if N == 8 {
            for r in (0..8).step_by(2) {
                let (d0, d1) = (dst.add(r * stride), dst.add((r + 1) * stride));
                let o = add(v128_load64_lane::<1>(v128_load64_zero(d0 as *const u64), d1 as *const u64));
                v128_store64_lane::<0>(o, d0 as *mut u64);
                v128_store64_lane::<1>(o, d1 as *mut u64);
            }
        } else {
            for r in 0..N {
                for x in (0..N).step_by(16) {
                    let d = dst.add(r * stride + x);
                    v128_store(d as *mut v128, add(v128_load(d as *const v128)));
                }
            }
        }
    }
}
