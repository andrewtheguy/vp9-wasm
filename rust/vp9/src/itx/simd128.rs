//! The transforms of `itx` in WebAssembly vectors, exact to the plain ones
//! for every input: the 4×4 DCT and ADST, the 8×8 DCT, and the add of a
//! block whose only coefficient is the DC.
//!
//! A pass runs over its rows at once, a row to a lane: the block is turned
//! about its diagonal, the pass runs over its vectors, and it is turned back
//! for the next. A rotation's two products are one dot product of a pair of
//! lanes with a pair of constants, exact in 32 bits, rounded there and taken
//! back to 16 as the plain `rs` takes them, the low half of the word.

use core::arch::wasm32::*;

use super::{C12, C16, C20, C24, C28, C4, C8, SINPI_1_9, SINPI_2_9, SINPI_3_9, SINPI_4_9};

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

/// `rot(-a, b, c0, c1)`: the negation folded into the constants, since `-a`
/// may not fit a lane.
#[inline(always)]
fn rot_neg(a: v128, b: v128, c0: i32, c1: i32) -> (v128, v128) {
    let (lo, hi) = pairs(a, b);
    (dot(lo, hi, pair(-c0, -c1)), dot(lo, hi, pair(-c1, c0)))
}

/// The plain `half`: `(rs((a + b)*C16), rs((a - b)*C16))`.
#[inline(always)]
fn half(a: v128, b: v128) -> (v128, v128) {
    let (lo, hi) = pairs(a, b);
    (dot(lo, hi, pair(C16, C16)), dot(lo, hi, pair(C16, -C16)))
}

/// `idct4` over eight lanes, each input a vector.
#[inline(always)]
fn idct4(i: [v128; 4]) -> [v128; 4] {
    let (s0, s1) = half(i[0], i[2]);
    let (s2, s3) = rot(i[1], i[3], C24, C8);
    [i16x8_add(s0, s3), i16x8_add(s1, s2), i16x8_sub(s1, s2), i16x8_sub(s0, s3)]
}

/// `idct8` over eight lanes.
#[inline(always)]
fn idct8(i: [v128; 8]) -> [v128; 8] {
    let e = idct4([i[0], i[2], i[4], i[6]]);
    let (s4, s7) = rot(i[1], i[7], C28, C4);
    let (s5, s6) = rot(i[5], i[3], C12, C20);
    let (t4, t5, t6, t7) = (i16x8_add(s4, s5), i16x8_sub(s4, s5), i16x8_sub(s7, s6), i16x8_add(s6, s7));
    let (u6, u5) = half(t6, t5);
    let o = [t4, u5, u6, t7];
    [
        i16x8_add(e[0], o[3]),
        i16x8_add(e[1], o[2]),
        i16x8_add(e[2], o[1]),
        i16x8_add(e[3], o[0]),
        i16x8_sub(e[3], o[0]),
        i16x8_sub(e[2], o[1]),
        i16x8_sub(e[1], o[2]),
        i16x8_sub(e[0], o[3]),
    ]
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

/// Rounds a pass's output by `shift` and adds it to eight samples at `d`.
/// The round-up saturates where the plain one's does not, which the clip
/// then hides: a sum that near the top is clipped either way.
#[inline(always)]
unsafe fn add8(d: *mut u8, v: v128, shift: u32) {
    let v = i16x8_shr(i16x8_add_sat(v, i16x8_splat(1 << (shift - 1))), shift);
    unsafe {
        let s = u16x8_extend_low_u8x16(v128_load64_zero(d as *const u64));
        let o = u8x16_narrow_i16x8(i16x8_add(s, v), i16x8_splat(0));
        v128_store64_lane::<0>(o, d as *mut u64);
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

/// The 8×8 DCT, added to the block at `dst`.
///
/// # Safety
/// `coeffs` holds 64 coefficients; the 8×8 block at `dst` is writable.
pub unsafe fn add_8x8(coeffs: *const i16, dst: *mut u8, stride: usize) {
    unsafe {
        let rows: [v128; 8] = core::array::from_fn(|r| v128_load(coeffs.add(8 * r) as *const v128));
        let out = idct8(transpose8(idct8(transpose8(rows))));
        for (r, &o) in out.iter().enumerate() {
            add8(dst.add(r * stride), o, 5);
        }
    }
}

/// Adds `a1` to every sample of the `N`×`N` block at `dst`: the plain
/// `add_dc`'s add.
///
/// # Safety
/// The block at `dst` is writable.
pub unsafe fn add_dc<const N: usize>(a1: i32, dst: *mut u8, stride: usize) {
    let v = i16x8_splat(a1 as i16);
    unsafe {
        for r in 0..N {
            let d = dst.add(r * stride);
            if N == 4 {
                let s = u16x8_extend_low_u8x16(v128_load32_zero(d as *const u32));
                let o = u8x16_narrow_i16x8(i16x8_add(s, v), i16x8_splat(0));
                v128_store32_lane::<0>(o, d as *mut u32);
            } else if N == 8 {
                let s = u16x8_extend_low_u8x16(v128_load64_zero(d as *const u64));
                let o = u8x16_narrow_i16x8(i16x8_add(s, v), i16x8_splat(0));
                v128_store64_lane::<0>(o, d as *mut u64);
            } else {
                for x in (0..N).step_by(16) {
                    let s = v128_load(d.add(x) as *const v128);
                    let o = u8x16_narrow_i16x8(i16x8_add(u16x8_extend_low_u8x16(s), v), i16x8_add(u16x8_extend_high_u8x16(s), v));
                    v128_store(d.add(x) as *mut v128, o);
                }
            }
        }
    }
}
