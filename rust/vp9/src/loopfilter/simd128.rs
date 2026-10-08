//! The edge kernels in WebAssembly's 128-bit vectors: the arithmetic of the
//! plain ones, the 8 positions of an edge at a time. A line of samples along
//! the edge is the low 8 bytes of a vector, whose other 8 are never looked at;
//! the sums of the flat filters are taken in 16-bit lanes.
//!
//! An edge leaves early where it has nothing to do: when its samples are the
//! same across it at every position, and when its mask lets no position
//! through. The narrow filter, the 7-tap and the 15-tap one are each computed
//! only when a position takes them, and only the lines one of them can have
//! changed are written.

use core::arch::wasm32::*;

/// The low 8 bytes of a vector: zero when none of an edge's positions is set.
#[inline(always)]
fn low(v: v128) -> u64 {
    u64x2_extract_lane::<0>(v)
}

#[inline(always)]
fn abs_diff(a: v128, b: v128) -> v128 {
    v128_or(u8x16_sub_sat(a, b), u8x16_sub_sat(b, a))
}

/// `a` where `mask` is set and `b` elsewhere.
#[inline(always)]
fn select(a: v128, b: v128, mask: v128) -> v128 {
    v128_bitselect(a, b, mask)
}

/// The flat filters, as the plain `smooth`, over the N lines across an edge
/// from the farthest before it to the farthest after: 8 for the 7-tap filter
/// and 16 for the 15-tap one. The first and the last come back as they were.
#[inline(always)]
fn smooth<const N: usize>(lines: [v128; N]) -> [v128; N] {
    let wide = |i: usize| u16x8_extend_low_u8x16(lines[i]);
    let shift = N.trailing_zeros();
    // The window around line 1, with the rounding: the line continued before
    // its start by its first sample.
    let mut sum = u16x8_add(u16x8_splat(N as u16 / 2), u16x8_sub(u16x8_shl(wide(0), shift - 1), wide(0)));
    sum = u16x8_add(sum, u16x8_add(u16x8_add(wide(1), wide(2)), u16x8_add(wide(3), wide(4))));
    if N == 16 {
        sum = u16x8_add(sum, u16x8_add(u16x8_add(wide(5), wide(6)), u16x8_add(wide(7), wide(8))));
    }
    let mut out = lines;
    // Line `i`, then the window moved on to the next.
    let mut step = |i: usize| {
        let v = u16x8_shr(u16x8_add(sum, wide(i)), shift);
        out[i] = u8x16_narrow_i16x8(v, v);
        sum = u16x8_sub(u16x8_add(sum, wide((i + N / 2).min(N - 1))), wide(i.saturating_sub(N / 2 - 1)));
    };
    // Spelled out, so that each line is a value of its own and not an index
    // into memory.
    step(1);
    step(2);
    step(3);
    step(4);
    step(5);
    step(6);
    if N == 16 {
        step(7);
        step(8);
        step(9);
        step(10);
        step(11);
        step(12);
        step(13);
        step(14);
    }
    out
}

/// Filters the 8 positions of an edge. `p` and `q` are the 4 lines on each
/// side of it, nearest first, and `outer` loads the 4 beyond them on each side,
/// called only when a position may take the 15-tap filter. Returns the lines
/// of each side and how many of them, counted from the edge, may have changed.
#[inline(always)]
fn filter<const TAPS: usize>(
    p: [v128; 4],
    q: [v128; 4],
    outer: impl FnOnce() -> ([v128; 4], [v128; 4]),
    blimit: u8,
    limit: u8,
    thresh: u8,
) -> ([v128; 8], [v128; 8], usize) {
    let mut po = [p[0], p[1], p[2], p[3], p[3], p[3], p[3], p[3]];
    let mut qo = [q[0], q[1], q[2], q[3], q[3], q[3], q[3], q[3]];

    // The mask. Its sum across the edge passes 255, so it is taken in 16 bits.
    let (dp1, dq1) = (abs_diff(p[1], p[0]), abs_diff(q[1], q[0]));
    let inner = u8x16_max(dp1, dq1);
    let steps = u8x16_max(
        u8x16_max(abs_diff(p[3], p[2]), abs_diff(p[2], p[1])),
        u8x16_max(abs_diff(q[2], q[1]), abs_diff(q[3], q[2])),
    );
    let d0 = u16x8_extend_low_u8x16(abs_diff(p[0], q[0]));
    let d1 = u16x8_extend_low_u8x16(abs_diff(p[1], q[1]));
    let across = u16x8_le(u16x8_add(u16x8_shl(d0, 1), u16x8_shr(d1, 1)), u16x8_splat(blimit as u16));
    let mask = v128_and(u8x16_le(u8x16_max(steps, inner), u8x16_splat(limit)), i8x16_narrow_i16x8(across, across));
    if low(mask) == 0 {
        return (po, qo, 0);
    }

    let one = u8x16_splat(1);
    let mut flat = u8x16_splat(0);
    if TAPS >= 8 {
        let far = u8x16_max(
            u8x16_max(abs_diff(p[2], p[0]), abs_diff(q[2], q[0])),
            u8x16_max(abs_diff(p[3], p[0]), abs_diff(q[3], q[0])),
        );
        flat = v128_and(mask, u8x16_le(u8x16_max(inner, far), one));
    }

    // The narrow filter, on the samples with their top bit turned: signed
    // bytes, whose saturating sums are the clamps of the plain one.
    if low(v128_andnot(mask, flat)) != 0 {
        let bias = u8x16_splat(0x80);
        let (ps1, ps0) = (v128_xor(p[1], bias), v128_xor(p[0], bias));
        let (qs0, qs1) = (v128_xor(q[0], bias), v128_xor(q[1], bias));
        let hev = u8x16_gt(inner, u8x16_splat(thresh));
        let step = i8x16_sub_sat(qs0, ps0);
        let f = v128_and(i8x16_sub_sat(ps1, qs1), hev);
        let f = i8x16_add_sat(i8x16_add_sat(i8x16_add_sat(f, step), step), step);
        let f1 = i8x16_shr(i8x16_add_sat(f, i8x16_splat(4)), 3);
        let f2 = i8x16_shr(i8x16_add_sat(f, i8x16_splat(3)), 3);
        let f = v128_andnot(i8x16_shr(i8x16_add(f1, i8x16_splat(1)), 1), hev);
        qo[0] = select(v128_xor(i8x16_sub_sat(qs0, f1), bias), q[0], mask);
        po[0] = select(v128_xor(i8x16_add_sat(ps0, f2), bias), p[0], mask);
        qo[1] = select(v128_xor(i8x16_sub_sat(qs1, f), bias), q[1], mask);
        po[1] = select(v128_xor(i8x16_add_sat(ps1, f), bias), p[1], mask);
    }
    if low(flat) == 0 {
        return (po, qo, 2);
    }

    let mut flat2 = u8x16_splat(0);
    if TAPS == 16 {
        let (op, oq) = outer();
        po[4..].copy_from_slice(&op);
        qo[4..].copy_from_slice(&oq);
        let mut far = u8x16_splat(0);
        for i in 0..4 {
            far = u8x16_max(far, u8x16_max(abs_diff(op[i], p[0]), abs_diff(oq[i], q[0])));
        }
        flat2 = v128_and(flat, u8x16_le(far, one));
    }
    // The 7-tap filter where a flat position does not take the 15-tap one.
    if low(v128_andnot(flat, flat2)) != 0 {
        let l = smooth([p[3], p[2], p[1], p[0], q[0], q[1], q[2], q[3]]);
        po[2] = select(l[1], po[2], flat);
        po[1] = select(l[2], po[1], flat);
        po[0] = select(l[3], po[0], flat);
        qo[0] = select(l[4], qo[0], flat);
        qo[1] = select(l[5], qo[1], flat);
        qo[2] = select(l[6], qo[2], flat);
    }
    if low(flat2) == 0 {
        return (po, qo, 3);
    }
    let l = smooth([po[7], po[6], po[5], po[4], p[3], p[2], p[1], p[0], q[0], q[1], q[2], q[3], qo[4], qo[5], qo[6], qo[7]]);
    po[6] = select(l[1], po[6], flat2);
    po[5] = select(l[2], po[5], flat2);
    po[4] = select(l[3], po[4], flat2);
    po[3] = select(l[4], po[3], flat2);
    po[2] = select(l[5], po[2], flat2);
    po[1] = select(l[6], po[1], flat2);
    po[0] = select(l[7], po[0], flat2);
    qo[0] = select(l[8], qo[0], flat2);
    qo[1] = select(l[9], qo[1], flat2);
    qo[2] = select(l[10], qo[2], flat2);
    qo[3] = select(l[11], qo[3], flat2);
    qo[4] = select(l[12], qo[4], flat2);
    qo[5] = select(l[13], qo[5], flat2);
    qo[6] = select(l[14], qo[6], flat2);
    (po, qo, 7)
}

/// Filters 8 samples along a horizontal edge with up to the filter of `TAPS`
/// samples: 4, 8 or 16.
///
/// # Safety
///
/// Columns `s[0..8]` of the `TAPS.max(8) / 2` rows before `s` and as many from
/// it on must be readable and writable.
pub(super) unsafe fn horizontal<const TAPS: usize>(s: *mut u8, pitch: isize, blimit: u8, limit: u8, thresh: u8) {
    // Row `i` from the edge, 0 being q0's and -1 p0's.
    let row = |i: isize| unsafe { v128_load64_zero(s.offset(i * pitch) as *const u64) };
    let side = |from: isize, step: isize| [row(from), row(from + step), row(from + 2 * step), row(from + 3 * step)];
    let outer = || (side(-5, -1), side(4, 1));
    let differs = |p: &[v128; 4], q: &[v128; 4], to: v128| {
        let mut acc = u8x16_splat(0);
        for i in 0..4 {
            acc = v128_or(acc, v128_or(v128_xor(p[i], to), v128_xor(q[i], to)));
        }
        low(acc) != 0
    };

    let (p, q) = (side(-1, -1), side(0, 1));
    // Samples that are the same across the edge stay as they are.
    if !differs(&p, &q, p[0]) {
        if TAPS < 16 {
            return;
        }
        let (op, oq) = outer();
        if !differs(&op, &oq, p[0]) {
            return;
        }
    }
    let (po, qo, reach) = filter::<TAPS>(p, q, outer, blimit, limit, thresh);
    // Line `i` of each side.
    let store = |i: usize| unsafe {
        v128_store64_lane::<0>(po[i], s.offset(-(i as isize + 1) * pitch) as *mut u64);
        v128_store64_lane::<0>(qo[i], s.offset(i as isize * pitch) as *mut u64);
    };
    if reach >= 2 {
        store(0);
        store(1);
    }
    if reach >= 3 {
        store(2);
    }
    if reach == 7 {
        store(3);
        store(4);
        store(5);
        store(6);
    }
}

/// Eight lines of 8 turned about their diagonal, two lines to a vector: line
/// `2i` of the result is the low half of vector `i` and line `2i + 1` its
/// high half.
#[inline(always)]
fn transpose(v: [v128; 8]) -> [v128; 4] {
    let bytes = |a, b| i8x16_shuffle::<0, 16, 1, 17, 2, 18, 3, 19, 4, 20, 5, 21, 6, 22, 7, 23>(a, b);
    let (a0, a1, a2, a3) = (bytes(v[0], v[1]), bytes(v[2], v[3]), bytes(v[4], v[5]), bytes(v[6], v[7]));
    let b0 = i16x8_shuffle::<0, 8, 1, 9, 2, 10, 3, 11>(a0, a1);
    let b1 = i16x8_shuffle::<4, 12, 5, 13, 6, 14, 7, 15>(a0, a1);
    let b2 = i16x8_shuffle::<0, 8, 1, 9, 2, 10, 3, 11>(a2, a3);
    let b3 = i16x8_shuffle::<4, 12, 5, 13, 6, 14, 7, 15>(a2, a3);
    [
        i32x4_shuffle::<0, 4, 1, 5>(b0, b2),
        i32x4_shuffle::<2, 6, 3, 7>(b0, b2),
        i32x4_shuffle::<0, 4, 1, 5>(b1, b3),
        i32x4_shuffle::<2, 6, 3, 7>(b1, b3),
    ]
}

/// The lines of `transpose`, one to a vector.
#[inline(always)]
fn lines(v: [v128; 4]) -> [v128; 8] {
    let high = |v| i64x2_shuffle::<1, 1>(v, v);
    [v[0], high(v[0]), v[1], high(v[1]), v[2], high(v[2]), v[3], high(v[3])]
}

/// Filters 8 rows along a vertical edge with up to the filter of `TAPS`
/// samples: 4, 8 or 16.
///
/// # Safety
///
/// The `TAPS.max(8) / 2` samples before `s` and as many from it on must be
/// readable and writable in each of 8 rows.
pub(super) unsafe fn vertical<const TAPS: usize>(s: *mut u8, pitch: isize, blimit: u8, limit: u8, thresh: u8) {
    // The 8 samples from `at` on in row `r`.
    let word = |r: usize, at: isize| unsafe { (s.offset(r as isize * pitch + at) as *const u64).read_unaligned() };
    // The 4 samples before `s - 4` and the 4 from `s + 4` on in row `r`.
    let outer_row = |r: usize| unsafe {
        let before = (s.offset(r as isize * pitch - 8) as *const u32).read_unaligned();
        let after = (s.offset(r as isize * pitch + 4) as *const u32).read_unaligned();
        u32x4(before, after, 0, 0)
    };
    // Columns p7 p6 p5 p4 q4 q5 q6 q7.
    let outer = || {
        let c = lines(transpose(core::array::from_fn(outer_row)));
        ([c[3], c[2], c[1], c[0]], [c[4], c[5], c[6], c[7]])
    };

    // Rows that are each one value stay as they are.
    let mut differs = 0;
    for r in 0..8 {
        let x = word(r, -4);
        differs |= x ^ x.rotate_left(8);
    }
    if differs == 0 {
        if TAPS < 16 {
            return;
        }
        // The halves of a row share its middle, so they are one value when
        // they are the same.
        for r in 0..8 {
            differs |= word(r, -8) ^ word(r, 0);
        }
        if differs == 0 {
            return;
        }
    }

    // Columns p3 p2 p1 p0 q0 q1 q2 q3.
    let c = lines(transpose(core::array::from_fn(|r| u64x2(word(r, -4), 0))));
    let (po, qo, reach) = filter::<TAPS>([c[3], c[2], c[1], c[0]], [c[4], c[5], c[6], c[7]], outer, blimit, limit, thresh);
    let row = |r: usize, at: isize| unsafe { s.offset(r as isize * pitch + at) };
    match reach {
        0 => {}
        2 => {
            // The four columns about the edge, four rows to a vector.
            let before = i8x16_shuffle::<0, 16, 1, 17, 2, 18, 3, 19, 4, 20, 5, 21, 6, 22, 7, 23>(po[1], po[0]);
            let after = i8x16_shuffle::<0, 16, 1, 17, 2, 18, 3, 19, 4, 20, 5, 21, 6, 22, 7, 23>(qo[0], qo[1]);
            let top = i16x8_shuffle::<0, 8, 1, 9, 2, 10, 3, 11>(before, after);
            let bottom = i16x8_shuffle::<4, 12, 5, 13, 6, 14, 7, 15>(before, after);
            unsafe {
                v128_store32_lane::<0>(top, row(0, -2) as *mut u32);
                v128_store32_lane::<1>(top, row(1, -2) as *mut u32);
                v128_store32_lane::<2>(top, row(2, -2) as *mut u32);
                v128_store32_lane::<3>(top, row(3, -2) as *mut u32);
                v128_store32_lane::<0>(bottom, row(4, -2) as *mut u32);
                v128_store32_lane::<1>(bottom, row(5, -2) as *mut u32);
                v128_store32_lane::<2>(bottom, row(6, -2) as *mut u32);
                v128_store32_lane::<3>(bottom, row(7, -2) as *mut u32);
            }
        }
        _ => {
            // The rows of 8 columns from `at` on.
            let store = |c: [v128; 8], at: isize| {
                let rows = transpose(c);
                for i in 0..4 {
                    unsafe {
                        v128_store64_lane::<0>(rows[i], row(2 * i, at) as *mut u64);
                        v128_store64_lane::<1>(rows[i], row(2 * i + 1, at) as *mut u64);
                    }
                }
            };
            if reach == 3 {
                store([po[3], po[2], po[1], po[0], qo[0], qo[1], qo[2], qo[3]], -4);
            } else {
                store([po[7], po[6], po[5], po[4], po[3], po[2], po[1], po[0]], -8);
                store(qo, 0);
            }
        }
    }
}
