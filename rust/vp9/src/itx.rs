//! The inverse transforms: DCT and ADST at 4, 8 and 16 points, the DCT at 32,
//! and the 4x4 Walsh-Hadamard of lossless frames.
//!
//! The arithmetic is transcribed from libvpx 1.16.0 (`vpx_dsp/inv_txfm.c`,
//! `vp9/common/vp9_idct.c`), as built without high bit depth: an intermediate
//! is 32 bits wide and wraps to 16 wherever the C stores it in an `int16_t`.
//! That makes the result the C's for every `i16` input, a valid stream's or
//! not. The C's own 32-bit sums can overflow in the ADST for coefficients no
//! valid stream holds; they wrap here, as they do in a C built with `-fwrapv`.
//!
//! Each DCT is its half-size DCT over the even inputs plus a stage for the odd
//! ones, which is what the C's flat stages compute.

use core::num::Wrapping;

const C1: i32 = 16364;
const C2: i32 = 16305;
const C3: i32 = 16207;
const C4: i32 = 16069;
const C5: i32 = 15893;
const C6: i32 = 15679;
const C7: i32 = 15426;
const C8: i32 = 15137;
const C9: i32 = 14811;
const C10: i32 = 14449;
const C11: i32 = 14053;
const C12: i32 = 13623;
const C13: i32 = 13160;
const C14: i32 = 12665;
const C15: i32 = 12140;
const C16: i32 = 11585;
const C17: i32 = 11003;
const C18: i32 = 10394;
const C19: i32 = 9760;
const C20: i32 = 9102;
const C21: i32 = 8423;
const C22: i32 = 7723;
const C23: i32 = 7005;
const C24: i32 = 6270;
const C25: i32 = 5520;
const C26: i32 = 4756;
const C27: i32 = 3981;
const C28: i32 = 3196;
const C29: i32 = 2404;
const C30: i32 = 1606;
const C31: i32 = 804;

const SINPI_1_9: i32 = 5283;
const SINPI_2_9: i32 = 9929;
const SINPI_3_9: i32 = 13377;
const SINPI_4_9: i32 = 15212;

/// A value as the C keeps it in an `int16_t`.
#[inline(always)]
fn w(x: i32) -> i32 {
    x as i16 as i32
}

/// libvpx's `dct_const_round_shift`, stored in an `int16_t`.
#[inline(always)]
fn rs(x: i32) -> i32 {
    w((x + (1 << 13)) >> 14)
}

/// `(a*c0 - b*c1, a*c1 + b*c0)`, rounded. `a` and `b` are within 16 bits (or
/// the negation of such a value), so neither sum leaves 32.
#[inline(always)]
fn rot(a: i32, b: i32, c0: i32, c1: i32) -> (i32, i32) {
    (rs(a * c0 - b * c1), rs(a * c1 + b * c0))
}

/// `((a + b) * cospi_16_64, (a - b) * cospi_16_64)`, rounded.
#[inline(always)]
fn half(a: i32, b: i32) -> (i32, i32) {
    (rs((a + b) * C16), rs((a - b) * C16))
}

/// `idct4_c`.
#[inline(always)]
fn idct4(i: &[i32; 4]) -> [i32; 4] {
    let (s0, s1) = half(i[0], i[2]);
    let (s2, s3) = rot(i[1], i[3], C24, C8);
    [w(s0 + s3), w(s1 + s2), w(s1 - s2), w(s0 - s3)]
}

/// `idct8_c`.
#[inline(always)]
fn idct8(i: &[i32; 8]) -> [i32; 8] {
    let e = idct4(&[i[0], i[2], i[4], i[6]]);
    let (s4, s7) = rot(i[1], i[7], C28, C4);
    let (s5, s6) = rot(i[5], i[3], C12, C20);
    let (t4, t5, t6, t7) = (w(s4 + s5), w(s4 - s5), w(s7 - s6), w(s6 + s7));
    let (u6, u5) = half(t6, t5);
    let o = [t4, u5, u6, t7];
    core::array::from_fn(|k| if k < 4 { w(e[k] + o[3 - k]) } else { w(e[7 - k] - o[k - 4]) })
}

/// `idct16_c`.
#[inline(always)]
fn idct16(i: &[i32; 16]) -> [i32; 16] {
    let e = idct8(&core::array::from_fn(|k| i[2 * k]));
    // stage 2
    let (s8, s15) = rot(i[1], i[15], C30, C2);
    let (s9, s14) = rot(i[9], i[7], C14, C18);
    let (s10, s13) = rot(i[5], i[11], C22, C10);
    let (s11, s12) = rot(i[13], i[3], C6, C26);
    // stage 3
    let (t8, t9, t10, t11) = (w(s8 + s9), w(s8 - s9), w(s11 - s10), w(s10 + s11));
    let (t12, t13, t14, t15) = (w(s12 + s13), w(s12 - s13), w(s15 - s14), w(s14 + s15));
    // stage 4
    let (u9, u14) = rot(t14, t9, C24, C8);
    let (u10, u13) = rot(-t10, t13, C24, C8);
    // stage 5
    let (v8, v9, v10, v11) = (w(t8 + t11), w(u9 + u10), w(u9 - u10), w(t8 - t11));
    let (v12, v13, v14, v15) = (w(t15 - t12), w(u14 - u13), w(u13 + u14), w(t12 + t15));
    // stage 6
    let (x13, x10) = half(v13, v10);
    let (x12, x11) = half(v12, v11);
    let o = [v8, v9, x10, x11, x12, x13, v14, v15];
    // stage 7
    core::array::from_fn(|k| if k < 8 { w(e[k] + o[7 - k]) } else { w(e[15 - k] - o[k - 8]) })
}

/// `idct32_c`.
#[inline(always)]
fn idct32(i: &[i32; 32]) -> [i32; 32] {
    let e = idct16(&core::array::from_fn(|k| i[2 * k]));
    // stage 1
    let (a16, a31) = rot(i[1], i[31], C31, C1);
    let (a17, a30) = rot(i[17], i[15], C15, C17);
    let (a18, a29) = rot(i[9], i[23], C23, C9);
    let (a19, a28) = rot(i[25], i[7], C7, C25);
    let (a20, a27) = rot(i[5], i[27], C27, C5);
    let (a21, a26) = rot(i[21], i[11], C11, C21);
    let (a22, a25) = rot(i[13], i[19], C19, C13);
    let (a23, a24) = rot(i[29], i[3], C3, C29);
    // stage 2
    let (b16, b17, b18, b19) = (w(a16 + a17), w(a16 - a17), w(a19 - a18), w(a18 + a19));
    let (b20, b21, b22, b23) = (w(a20 + a21), w(a20 - a21), w(a23 - a22), w(a22 + a23));
    let (b24, b25, b26, b27) = (w(a24 + a25), w(a24 - a25), w(a27 - a26), w(a26 + a27));
    let (b28, b29, b30, b31) = (w(a28 + a29), w(a28 - a29), w(a31 - a30), w(a30 + a31));
    // stage 3
    let (c17, c30) = rot(b30, b17, C28, C4);
    let (c18, c29) = rot(-b18, b29, C28, C4);
    let (c21, c26) = rot(b26, b21, C12, C20);
    let (c22, c25) = rot(-b22, b25, C12, C20);
    // stage 4
    let (d16, d17, d18, d19) = (w(b16 + b19), w(c17 + c18), w(c17 - c18), w(b16 - b19));
    let (d20, d21, d22, d23) = (w(b23 - b20), w(c22 - c21), w(c21 + c22), w(b20 + b23));
    let (d24, d25, d26, d27) = (w(b24 + b27), w(c25 + c26), w(c25 - c26), w(b24 - b27));
    let (d28, d29, d30, d31) = (w(b31 - b28), w(c30 - c29), w(c29 + c30), w(b28 + b31));
    // stage 5
    let (e18, e29) = rot(d29, d18, C24, C8);
    let (e19, e28) = rot(d28, d19, C24, C8);
    let (e20, e27) = rot(-d20, d27, C24, C8);
    let (e21, e26) = rot(-d21, d26, C24, C8);
    // stage 6
    let (f16, f17, f18, f19) = (w(d16 + d23), w(d17 + d22), w(e18 + e21), w(e19 + e20));
    let (f20, f21, f22, f23) = (w(e19 - e20), w(e18 - e21), w(d17 - d22), w(d16 - d23));
    let (f24, f25, f26, f27) = (w(d31 - d24), w(d30 - d25), w(e29 - e26), w(e28 - e27));
    let (f28, f29, f30, f31) = (w(e27 + e28), w(e26 + e29), w(d25 + d30), w(d24 + d31));
    // stage 7
    let (g27, g20) = half(f27, f20);
    let (g26, g21) = half(f26, f21);
    let (g25, g22) = half(f25, f22);
    let (g24, g23) = half(f24, f23);
    let o = [f16, f17, f18, f19, g20, g21, g22, g23, g24, g25, g26, g27, f28, f29, f30, f31];
    // final stage
    core::array::from_fn(|k| if k < 16 { w(e[k] + o[15 - k]) } else { w(e[31 - k] - o[k - 16]) })
}

/// The ADST's 32-bit intermediate. Unlike the DCT's, it is not stored in 16
/// bits between stages, so coefficients no valid stream holds can overflow it.
type W = Wrapping<i32>;

/// `dct_const_round_shift`, kept at 32 bits.
#[inline(always)]
fn rsw(x: W) -> W {
    (x + Wrapping(1 << 13)) >> 14
}

/// `(a*c0 + b*c1, a*c1 - b*c0)`.
#[inline(always)]
fn bf(a: W, b: W, c0: i32, c1: i32) -> (W, W) {
    (a * Wrapping(c0) + b * Wrapping(c1), a * Wrapping(c1) - b * Wrapping(c0))
}

/// `(-a*c0 + b*c1, a*c1 + b*c0)`.
#[inline(always)]
fn bf2(a: W, b: W, c0: i32, c1: i32) -> (W, W) {
    (b * Wrapping(c1) - a * Wrapping(c0), a * Wrapping(c1) + b * Wrapping(c0))
}

/// `iadst4_c`.
#[inline(always)]
fn iadst4(i: &[i32; 4]) -> [i32; 4] {
    let [x0, x1, x2, x3] = i.map(Wrapping);
    let s0 = Wrapping(SINPI_1_9) * x0 + Wrapping(SINPI_4_9) * x2 + Wrapping(SINPI_2_9) * x3;
    let s1 = Wrapping(SINPI_2_9) * x0 - Wrapping(SINPI_1_9) * x2 - Wrapping(SINPI_4_9) * x3;
    let s3 = Wrapping(SINPI_3_9) * x1;
    let s2 = Wrapping(SINPI_3_9) * (x0 - x2 + x3);
    [s0 + s3, s1 + s3, s2, s0 + s1 - s3].map(|s| w(rsw(s).0))
}

/// `iadst8_c`.
#[inline(always)]
fn iadst8(i: &[i32; 8]) -> [i32; 8] {
    let x = i.map(Wrapping);
    let c16 = Wrapping(C16);
    // stage 1
    let (s0, s1) = bf(x[7], x[0], C2, C30);
    let (s2, s3) = bf(x[5], x[2], C10, C22);
    let (s4, s5) = bf(x[3], x[4], C18, C14);
    let (s6, s7) = bf(x[1], x[6], C26, C6);
    let (x0, x1, x2, x3) = (rsw(s0 + s4), rsw(s1 + s5), rsw(s2 + s6), rsw(s3 + s7));
    let (x4, x5, x6, x7) = (rsw(s0 - s4), rsw(s1 - s5), rsw(s2 - s6), rsw(s3 - s7));
    // stage 2
    let (s0, s1, s2, s3) = (x0, x1, x2, x3);
    let (s4, s5) = bf(x4, x5, C8, C24);
    let (s6, s7) = bf2(x6, x7, C24, C8);
    let (x0, x1, x2, x3) = (s0 + s2, s1 + s3, s0 - s2, s1 - s3);
    let (x4, x5, x6, x7) = (rsw(s4 + s6), rsw(s5 + s7), rsw(s4 - s6), rsw(s5 - s7));
    // stage 3
    let (y2, y3) = (rsw(c16 * (x2 + x3)), rsw(c16 * (x2 - x3)));
    let (y6, y7) = (rsw(c16 * (x6 + x7)), rsw(c16 * (x6 - x7)));
    [x0, -x4, y6, -y2, y3, -y7, x5, -x1].map(|v| w(v.0))
}

/// `iadst16_c`.
#[inline(always)]
fn iadst16(i: &[i32; 16]) -> [i32; 16] {
    let x = i.map(Wrapping);
    let c16 = Wrapping(C16);
    // stage 1
    let (s0, s1) = bf(x[15], x[0], C1, C31);
    let (s2, s3) = bf(x[13], x[2], C5, C27);
    let (s4, s5) = bf(x[11], x[4], C9, C23);
    let (s6, s7) = bf(x[9], x[6], C13, C19);
    let (s8, s9) = bf(x[7], x[8], C17, C15);
    let (s10, s11) = bf(x[5], x[10], C21, C11);
    let (s12, s13) = bf(x[3], x[12], C25, C7);
    let (s14, s15) = bf(x[1], x[14], C29, C3);
    let (x0, x1, x2, x3) = (rsw(s0 + s8), rsw(s1 + s9), rsw(s2 + s10), rsw(s3 + s11));
    let (x4, x5, x6, x7) = (rsw(s4 + s12), rsw(s5 + s13), rsw(s6 + s14), rsw(s7 + s15));
    let (x8, x9, x10, x11) = (rsw(s0 - s8), rsw(s1 - s9), rsw(s2 - s10), rsw(s3 - s11));
    let (x12, x13, x14, x15) = (rsw(s4 - s12), rsw(s5 - s13), rsw(s6 - s14), rsw(s7 - s15));
    // stage 2
    let (s0, s1, s2, s3, s4, s5, s6, s7) = (x0, x1, x2, x3, x4, x5, x6, x7);
    let (s8, s9) = bf(x8, x9, C4, C28);
    let (s10, s11) = bf(x10, x11, C20, C12);
    let (s12, s13) = bf2(x12, x13, C28, C4);
    let (s14, s15) = bf2(x14, x15, C12, C20);
    let (x0, x1, x2, x3) = (s0 + s4, s1 + s5, s2 + s6, s3 + s7);
    let (x4, x5, x6, x7) = (s0 - s4, s1 - s5, s2 - s6, s3 - s7);
    let (x8, x9, x10, x11) = (rsw(s8 + s12), rsw(s9 + s13), rsw(s10 + s14), rsw(s11 + s15));
    let (x12, x13, x14, x15) = (rsw(s8 - s12), rsw(s9 - s13), rsw(s10 - s14), rsw(s11 - s15));
    // stage 3
    let (s0, s1, s2, s3) = (x0, x1, x2, x3);
    let (s4, s5) = bf(x4, x5, C8, C24);
    let (s6, s7) = bf2(x6, x7, C24, C8);
    let (s8, s9, s10, s11) = (x8, x9, x10, x11);
    let (s12, s13) = bf(x12, x13, C8, C24);
    let (s14, s15) = bf2(x14, x15, C24, C8);
    let (x0, x1, x2, x3) = (s0 + s2, s1 + s3, s0 - s2, s1 - s3);
    let (x4, x5, x6, x7) = (rsw(s4 + s6), rsw(s5 + s7), rsw(s4 - s6), rsw(s5 - s7));
    let (x8, x9, x10, x11) = (s8 + s10, s9 + s11, s8 - s10, s9 - s11);
    let (x12, x13, x14, x15) = (rsw(s12 + s14), rsw(s13 + s15), rsw(s12 - s14), rsw(s13 - s15));
    // stage 4
    let (y2, y3) = (rsw(-c16 * (x2 + x3)), rsw(c16 * (x2 - x3)));
    let (y6, y7) = (rsw(c16 * (x6 + x7)), rsw(c16 * (x7 - x6)));
    let (y10, y11) = (rsw(c16 * (x10 + x11)), rsw(c16 * (x11 - x10)));
    let (y14, y15) = (rsw(-c16 * (x14 + x15)), rsw(c16 * (x14 - x15)));
    [x0, -x8, x12, -x4, y6, y14, y10, y2, y3, y11, y15, y7, x5, -x13, x9, -x1].map(|v| w(v.0))
}

#[inline(always)]
fn clip_add(dest: u8, trans: i32) -> u8 {
    (dest as i32 + trans).clamp(0, 255) as u8
}

/// The two passes of libvpx's `*_add_c`: `row` over the first `rows` rows of
/// `coeffs` (the rest taken as zero), then `col` over every column, rounded by
/// `shift` and added to `dst`. A row of zeros transforms to zeros and is
/// skipped.
#[inline(always)]
unsafe fn add_2d<const N: usize>(
    coeffs: &[i16],
    rows: usize,
    shift: u32,
    row: impl Fn(&[i32; N]) -> [i32; N],
    col: impl Fn(&[i32; N]) -> [i32; N],
    dst: *mut u8,
    stride: usize,
) {
    // Column-major, so that the second pass reads each column in one piece.
    let mut t = [[0i32; N]; N];
    for (y, r) in coeffs.chunks_exact(N).take(rows.min(N)).enumerate() {
        if r.iter().all(|&c| c == 0) {
            continue;
        }
        let o = row(&core::array::from_fn(|x| r[x] as i32));
        for x in 0..N {
            t[x][y] = o[x];
        }
    }
    for c in t.iter_mut() {
        *c = col(c);
    }
    let round = 1 << (shift - 1);
    for y in 0..N {
        // SAFETY: the caller's: row y of the block, N samples wide.
        unsafe {
            let d = dst.add(y * stride);
            for x in 0..N {
                let p = d.add(x);
                *p = clip_add(*p, (t[x][y] + round) >> shift);
            }
        }
    }
}

/// libvpx's `vpx_idct{4x4,8x8,16x16,32x32}_1_add_c`: a block whose only
/// coefficient is the DC adds one value to every sample.
#[inline(always)]
unsafe fn add_dc(n: usize, shift: u32, dc: i16, dst: *mut u8, stride: usize) {
    let a1 = (rs(rs(dc as i32 * C16) * C16) + (1 << (shift - 1))) >> shift;
    for y in 0..n {
        // SAFETY: the caller's: row y of the block, n samples wide.
        unsafe {
            let d = dst.add(y * stride);
            for x in 0..n {
                let p = d.add(x);
                *p = clip_add(*p, a1);
            }
        }
    }
}

/// One pass of the Walsh-Hadamard over `[a, c, d, b]`, giving `[a, b, c, d]`.
#[inline(always)]
fn iwht4(mut a1: i32, mut c1: i32, mut d1: i32, mut b1: i32) -> [i32; 4] {
    a1 += c1;
    d1 -= b1;
    let e1 = (a1 - d1) >> 1;
    b1 = e1 - b1;
    c1 = e1 - c1;
    a1 -= b1;
    d1 += c1;
    [a1, b1, c1, d1]
}

/// `vpx_iwht4x4_16_add_c`.
unsafe fn iwht4x4_16_add(c: &[i16; 16], dst: *mut u8, stride: usize) {
    let mut t = [[0i32; 4]; 4];
    for y in 0..4 {
        let i: [i32; 4] = core::array::from_fn(|x| (c[4 * y + x] >> 2) as i32);
        t[y] = iwht4(i[0], i[1], i[2], i[3]).map(w);
    }
    for x in 0..4 {
        let o = iwht4(t[0][x], t[1][x], t[2][x], t[3][x]);
        for y in 0..4 {
            // SAFETY: the caller's: sample (x, y) of a 4x4 block.
            unsafe {
                let p = dst.add(y * stride + x);
                *p = clip_add(*p, o[y]);
            }
        }
    }
}

/// `vpx_iwht4x4_1_add_c`.
unsafe fn iwht4x4_1_add(dc: i16, dst: *mut u8, stride: usize) {
    let a1 = (dc >> 2) as i32;
    let e1 = a1 >> 1;
    let t = [a1 - e1, e1, e1, e1];
    for x in 0..4 {
        let e1 = t[x] >> 1;
        let a1 = t[x] - e1;
        for y in 0..4 {
            // SAFETY: the caller's: sample (x, y) of a 4x4 block.
            unsafe {
                let p = dst.add(y * stride + x);
                *p = clip_add(*p, if y == 0 { a1 } else { e1 });
            }
        }
    }
}

/// Adds the inverse transform of a block's dequantized coefficients to the
/// `n`×`n` samples at `dst` (n = 4 << tx_size), clipping to 8 bits.
/// tx_size: 0 = 4x4, 1 = 8x8, 2 = 16x16, 3 = 32x32.
/// tx_type: libvpx's TX_TYPE: 0 DCT_DCT, 1 ADST_DCT, 2 DCT_ADST, 3 ADST_ADST (32x32 is always DCT_DCT).
/// lossless: the 4x4 Walsh-Hadamard (vpx_iwht4x4_16_add_c / _1_add_c), tx_size is then 0.
/// coeffs: n*n coefficients, row-major exactly as libvpx's dqcoeff. Not modified.
/// eob: number of coefficients in scan order that may be non-zero (>= 1). It
/// picks what libvpx's `vp9_idct*_add` picks: for DCT_DCT, the DC alone at 1,
/// and the first 4 rows at up to 12 (8x8) or 10 (16x16), the first 8 at up to
/// 38 (16x16) or 34 (32x32), the first 16 at up to 135 (32x32), which is where
/// the default scan keeps that many coefficients. The other types, whose scans
/// differ, read every row.
///
/// Nothing is added when `coeffs` is shorter than the block or `tx_size` is past 3.
///
/// # Safety
/// dst..dst + (n-1)*stride + n must be writable.
pub unsafe fn inverse_add(
    tx_size: usize,
    tx_type: usize,
    lossless: bool,
    coeffs: &[i16],
    eob: usize,
    dst: *mut u8,
    stride: usize,
) {
    if tx_size > 3 {
        return;
    }
    let n = 4usize << tx_size;
    let Some(coeffs) = coeffs.get(..n * n) else { return };
    // SAFETY: the caller's, for each: the block is n samples square.
    unsafe {
        if lossless {
            let Some(c) = coeffs.first_chunk::<16>() else { return };
            return if eob > 1 { iwht4x4_16_add(c, dst, stride) } else { iwht4x4_1_add(c[0], dst, stride) };
        }
        let dct = tx_type == 0 || tx_size == 3;
        if dct && eob <= 1 {
            return add_dc(n, [4, 5, 6, 6][tx_size], coeffs[0], dst, stride);
        }
        match (tx_size, tx_type) {
            (0, 0) => add_2d(coeffs, 4, 4, idct4, idct4, dst, stride),
            (0, 1) => add_2d(coeffs, 4, 4, idct4, iadst4, dst, stride),
            (0, 2) => add_2d(coeffs, 4, 4, iadst4, idct4, dst, stride),
            (0, _) => add_2d(coeffs, 4, 4, iadst4, iadst4, dst, stride),
            (1, 0) => add_2d(coeffs, if eob <= 12 { 4 } else { 8 }, 5, idct8, idct8, dst, stride),
            (1, 1) => add_2d(coeffs, 8, 5, idct8, iadst8, dst, stride),
            (1, 2) => add_2d(coeffs, 8, 5, iadst8, idct8, dst, stride),
            (1, _) => add_2d(coeffs, 8, 5, iadst8, iadst8, dst, stride),
            (2, 0) => {
                let rows = if eob <= 10 { 4 } else if eob <= 38 { 8 } else { 16 };
                add_2d(coeffs, rows, 6, idct16, idct16, dst, stride)
            }
            (2, 1) => add_2d(coeffs, 16, 6, idct16, iadst16, dst, stride),
            (2, 2) => add_2d(coeffs, 16, 6, iadst16, idct16, dst, stride),
            (2, _) => add_2d(coeffs, 16, 6, iadst16, iadst16, dst, stride),
            _ => {
                let rows = if eob <= 34 { 8 } else if eob <= 135 { 16 } else { 32 };
                add_2d(coeffs, rows, 6, idct32, idct32, dst, stride)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::inverse_add;

    /// Every (tx_size, tx_type, lossless) there is.
    fn kinds() -> impl Iterator<Item = (usize, usize, bool)> {
        let sized = (0..4).flat_map(|s| (0..if s == 3 { 1 } else { 4 }).map(move |t| (s, t, false)));
        core::iter::once((0, 0, true)).chain(sized)
    }

    fn add(tx_size: usize, tx_type: usize, lossless: bool, coeffs: &[i16], eob: usize, dst: &mut [u8]) {
        let n = 4 << tx_size;
        assert_eq!(dst.len(), n * n);
        // SAFETY: dst is the n by n block.
        unsafe { inverse_add(tx_size, tx_type, lossless, coeffs, eob, dst.as_mut_ptr(), n) }
    }

    fn ramp(n: usize) -> Vec<u8> {
        (0..n * n).map(|i| (i * 7 + 3) as u8).collect()
    }

    fn noise(n: usize, seed: u32, shift: u32) -> Vec<i16> {
        let mut s = seed;
        (0..n * n)
            .map(|_| {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                ((s >> 16) as i16) >> shift
            })
            .collect()
    }

    fn fnv(b: &[u8]) -> u32 {
        b.iter().fold(0x811c9dc5u32, |h, &v| (h ^ v as u32).wrapping_mul(0x01000193))
    }

    #[test]
    fn zero_coefficients_change_nothing() {
        for (s, t, l) in kinds() {
            let n = 4 << s;
            let mut d = ramp(n);
            add(s, t, l, &vec![0; n * n], n * n, &mut d);
            assert_eq!(d, ramp(n), "{s} {t} {l}");
        }
    }

    #[test]
    fn dc_alone_equals_the_full_transform() {
        for (s, l) in [(0, true), (0, false), (1, false), (2, false), (3, false)] {
            let n = 4 << s;
            for dc in [1, -1, 7, -64, 500, -1023, 4095, i16::MAX, i16::MIN] {
                let mut c = vec![0i16; n * n];
                c[0] = dc;
                let (mut a, mut b) = (ramp(n), ramp(n));
                add(s, 0, l, &c, 1, &mut a);
                add(s, 0, l, &c, n * n, &mut b);
                assert_eq!(a, b, "{s} {l} {dc}");
            }
        }
    }

    #[test]
    fn leading_rows_alone_equal_the_full_transform() {
        // (tx_size, eob, rows that eob leaves to read)
        for (s, eob, rows) in [(1, 12, 4), (2, 10, 4), (2, 38, 8), (3, 34, 8), (3, 135, 16)] {
            let n = 4 << s;
            let mut c = noise(n, 99 + eob as u32, 6);
            c[rows * n..].fill(0);
            let (mut a, mut b) = (ramp(n), ramp(n));
            add(s, 0, false, &c, eob, &mut a);
            add(s, 0, false, &c, n * n, &mut b);
            assert_eq!(a, b, "{s} {eob}");
        }
    }

    /// What libvpx 1.16.0's C gives for `noise(n, 1 + 4 * tx_size + tx_type + 16 * lossless, 7)`
    /// over `ramp(n)`: the samples at 4x4, their FNV-1a hash above it.
    #[test]
    fn known_answers() {
        let small: [(usize, bool, [u8; 16]); 5] = [
            (0, true, [33, 0, 92, 0, 48, 28, 0, 55, 120, 43, 60, 132, 83, 62, 136, 168]),
            (0, false, [2, 31, 12, 15, 41, 99, 47, 30, 51, 79, 59, 67, 90, 83, 127, 115]),
            (1, false, [0, 0, 14, 7, 57, 65, 42, 44, 96, 57, 57, 89, 84, 132, 92, 110]),
            (2, false, [0, 2, 38, 45, 42, 47, 71, 69, 31, 60, 65, 72, 82, 101, 72, 131]),
            (3, false, [0, 0, 37, 8, 27, 59, 56, 84, 43, 67, 74, 96, 62, 109, 114, 100]),
        ];
        for (t, l, want) in small {
            let mut d = ramp(4);
            add(0, t, l, &noise(4, 1 + t as u32 + l as u32 * 16, 7), 16, &mut d);
            assert_eq!(d, want, "{t} {l}");
        }
        let large: [(usize, usize, u32); 9] = [
            (1, 0, 0x94b72a60),
            (1, 1, 0xfa284832),
            (1, 2, 0xa39ec677),
            (1, 3, 0x638ebf1e),
            (2, 0, 0x3c6ba17f),
            (2, 1, 0xe1f0bd92),
            (2, 2, 0xc4cb4922),
            (2, 3, 0x85428f25),
            (3, 0, 0x242377b1),
        ];
        for (s, t, want) in large {
            let n = 4 << s;
            let mut d = ramp(n);
            add(s, t, false, &noise(n, 1 + s as u32 * 4 + t as u32, 7), n * n, &mut d);
            assert_eq!(fnv(&d), want, "{s} {t}");
        }
    }

    #[test]
    fn coefficients_no_stream_holds_do_not_overflow() {
        for (s, t, l) in kinds() {
            let n = 4 << s;
            for fill in [vec![i16::MIN; n * n], vec![i16::MAX; n * n], noise(n, 5, 0)] {
                for eob in [1, 2, n * n] {
                    add(s, t, l, &fill, eob, &mut ramp(n));
                }
            }
            let checker: Vec<i16> = (0..n * n).map(|i| if (i / n + i % n) % 2 == 0 { i16::MIN } else { i16::MAX }).collect();
            add(s, t, l, &checker, n * n, &mut ramp(n));
        }
    }

    #[test]
    fn a_short_block_or_an_unknown_size_adds_nothing() {
        let mut d = ramp(8);
        add(1, 0, false, &[100; 63], 64, &mut d);
        assert_eq!(d, ramp(8));
        // SAFETY: nothing is written for a size past 32x32.
        unsafe { inverse_add(4, 0, false, &[100; 4096], 1, d.as_mut_ptr(), 8) };
        assert_eq!(d, ramp(8));
    }
}
