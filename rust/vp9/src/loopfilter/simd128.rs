//! The edge kernels in WebAssembly's 128-bit vectors: the arithmetic of the
//! plain ones, the 8 positions of an edge at a time, or of two edges at once
//! as libvpx's `_dual` kernels take them. `W` is the lanes an edge's lines
//! fill: 8 for one edge, whose line is the low 8 bytes of a vector and whose
//! other 8 are never looked at, and 16 for two, the first's positions in the
//! low 8 and the second's in the high 8, each with its own limits. The sums of
//! the flat filters are taken in 16-bit lanes.
//!
//! An edge leaves early where it has nothing to do: when its samples are the
//! same across it at every position, and when its mask lets no position
//! through. Of two edges, the one whose samples are the same across it is left
//! and the other filtered alone. The narrow filter, the 7-tap and the 15-tap
//! one are each computed only when a position takes them, and only the lines
//! one of them can have changed are written.

use core::arch::wasm32::*;

/// Whether no lane an edge fills is set.
#[inline(always)]
fn none<const W: usize>(v: v128) -> bool {
    if W == 8 { u64x2_extract_lane::<0>(v) == 0 } else { !v128_any_true(v) }
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

/// A limit of the first edge in the low 8 lanes and of the second in the high.
#[inline(always)]
fn limits(first: u8, second: u8) -> v128 {
    i64x2_shuffle::<0, 3>(u8x16_splat(first), u8x16_splat(second))
}

/// The flat filters' sums, as the plain `smooth`, over the N lines across an
/// edge in 16-bit lanes, from the farthest before it to the farthest after: 8
/// for the 7-tap filter and 16 for the 15-tap one. Line `i` of the result is
/// line `i` filtered, for each but the first and the last.
#[inline(always)]
fn sums<const N: usize>(wide: [v128; N]) -> [v128; N] {
    let shift = N.trailing_zeros();
    // The window around line 1, with the rounding: the line continued before
    // its start by its first sample.
    let mut sum = u16x8_add(u16x8_splat(N as u16 / 2), u16x8_sub(u16x8_shl(wide[0], shift - 1), wide[0]));
    sum = u16x8_add(sum, u16x8_add(u16x8_add(wide[1], wide[2]), u16x8_add(wide[3], wide[4])));
    if N == 16 {
        sum = u16x8_add(sum, u16x8_add(u16x8_add(wide[5], wide[6]), u16x8_add(wide[7], wide[8])));
    }
    let mut out = wide;
    // Line `i`, then the window moved on to the next.
    let mut step = |i: usize| {
        out[i] = u16x8_shr(u16x8_add(sum, wide[i]), shift);
        sum = u16x8_sub(u16x8_add(sum, wide[(i + N / 2).min(N - 1)]), wide[i.saturating_sub(N / 2 - 1)]);
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

/// The flat filters over the N lines of `sums`, in bytes: the first and the
/// last line come back as they were.
#[inline(always)]
fn smooth<const N: usize, const W: usize>(lines: [v128; N]) -> [v128; N] {
    let low = sums(lines.map(|v| u16x8_extend_low_u8x16(v)));
    let high = if W == 16 { sums(lines.map(|v| u16x8_extend_high_u8x16(v))) } else { low };
    let mut out = lines;
    let mut narrow = |i: usize| out[i] = u8x16_narrow_i16x8(low[i], high[i]);
    narrow(1);
    narrow(2);
    narrow(3);
    narrow(4);
    narrow(5);
    narrow(6);
    if N == 16 {
        narrow(7);
        narrow(8);
        narrow(9);
        narrow(10);
        narrow(11);
        narrow(12);
        narrow(13);
        narrow(14);
    }
    out
}

/// Filters the positions of an edge, or of two. `p` and `q` are the 4 lines on
/// each side of it, nearest first, and `outer` loads the 4 beyond them on each
/// side, called only when a position may take the 15-tap filter. Returns the
/// lines of each side and how many of them, counted from the edge, may have
/// changed.
#[inline(always)]
fn filter<const TAPS: usize, const W: usize>(
    p: [v128; 4],
    q: [v128; 4],
    outer: impl FnOnce() -> ([v128; 4], [v128; 4]),
    blimit: v128,
    limit: v128,
    thresh: v128,
) -> ([v128; 8], [v128; 8], usize) {
    let mut po = [p[0], p[1], p[2], p[3], p[3], p[3], p[3], p[3]];
    let mut qo = [q[0], q[1], q[2], q[3], q[3], q[3], q[3], q[3]];

    // The mask. Its sum across the edge saturates past 255, which is past
    // every limit.
    let (dp1, dq1) = (abs_diff(p[1], p[0]), abs_diff(q[1], q[0]));
    let inner = u8x16_max(dp1, dq1);
    let steps = u8x16_max(
        u8x16_max(abs_diff(p[3], p[2]), abs_diff(p[2], p[1])),
        u8x16_max(abs_diff(q[2], q[1]), abs_diff(q[3], q[2])),
    );
    let d0 = abs_diff(p[0], q[0]);
    let across = u8x16_add_sat(u8x16_add_sat(d0, d0), u8x16_shr(abs_diff(p[1], q[1]), 1));
    let mask = v128_and(u8x16_le(u8x16_max(steps, inner), limit), u8x16_le(across, blimit));
    if none::<W>(mask) {
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
    if !none::<W>(v128_andnot(mask, flat)) {
        let bias = u8x16_splat(0x80);
        let (ps1, ps0) = (v128_xor(p[1], bias), v128_xor(p[0], bias));
        let (qs0, qs1) = (v128_xor(q[0], bias), v128_xor(q[1], bias));
        let hev = u8x16_gt(inner, thresh);
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
    if none::<W>(flat) {
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
    if !none::<W>(v128_andnot(flat, flat2)) {
        let l = smooth::<8, W>([p[3], p[2], p[1], p[0], q[0], q[1], q[2], q[3]]);
        po[2] = select(l[1], po[2], flat);
        po[1] = select(l[2], po[1], flat);
        po[0] = select(l[3], po[0], flat);
        qo[0] = select(l[4], qo[0], flat);
        qo[1] = select(l[5], qo[1], flat);
        qo[2] = select(l[6], qo[2], flat);
    }
    if none::<W>(flat2) {
        return (po, qo, 3);
    }
    let l = smooth::<16, W>([po[7], po[6], po[5], po[4], p[3], p[2], p[1], p[0], q[0], q[1], q[2], q[3], qo[4], qo[5], qo[6], qo[7]]);
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

/// The lines beyond the 4 on each side, which an edge of the narrow or the
/// 8-sample filter never asks for.
#[inline(always)]
fn no_outer() -> ([v128; 4], [v128; 4]) {
    unreachable!()
}

/// Row `i` of a horizontal edge, 0 being q0's and -1 p0's: its first W
/// samples.
#[inline(always)]
unsafe fn row<const W: usize>(s: *mut u8, pitch: isize, i: isize) -> v128 {
    let at = unsafe { s.offset(i * pitch) };
    unsafe { if W == 8 { v128_load64_zero(at as *const u64) } else { v128_load(at as *const v128) } }
}

/// The 4 rows of a horizontal edge from `from` on, `step` apart.
#[inline(always)]
unsafe fn side<const W: usize>(s: *mut u8, pitch: isize, from: isize, step: isize) -> [v128; 4] {
    unsafe { [0, 1, 2, 3].map(|i| row::<W>(s, pitch, from + i * step)) }
}

/// Which positions of the lines `p` and `q` differ from `to`, one to a byte.
#[inline(always)]
fn differs(p: &[v128; 4], q: &[v128; 4], to: v128) -> v128 {
    let mut acc = u8x16_splat(0);
    for i in 0..4 {
        acc = v128_or(acc, v128_or(v128_xor(p[i], to), v128_xor(q[i], to)));
    }
    acc
}

/// Filters W samples along a horizontal edge whose 4 rows on each side are `p`
/// and `q`, with up to the filter of `TAPS` samples: 4, 8 or 16. Of two edges,
/// the second is the one from `s + 8`.
///
/// # Safety
///
/// Columns `s[0..W]` of the `TAPS.max(8) / 2` rows before `s` and as many from
/// it on must be readable and writable.
#[inline(always)]
unsafe fn horizontal_lanes<const TAPS: usize, const W: usize>(
    s: *mut u8,
    pitch: isize,
    p: [v128; 4],
    q: [v128; 4],
    blimit: v128,
    limit: v128,
    thresh: v128,
) {
    let outer = || unsafe { (side::<W>(s, pitch, -5, -1), side::<W>(s, pitch, 4, 1)) };
    let (po, qo, reach) = filter::<TAPS, W>(p, q, outer, blimit, limit, thresh);
    // Line `i` of each side.
    let store = |i: usize| unsafe {
        let (before, after) = (s.offset(-(i as isize + 1) * pitch), s.offset(i as isize * pitch));
        if W == 8 {
            v128_store64_lane::<0>(po[i], before as *mut u64);
            v128_store64_lane::<0>(qo[i], after as *mut u64);
        } else {
            v128_store(before as *mut v128, po[i]);
            v128_store(after as *mut v128, qo[i]);
        }
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

/// Filters 8 samples along a horizontal edge with up to the filter of `TAPS`
/// samples: 4, 8 or 16.
///
/// # Safety
///
/// Columns `s[0..8]` of the `TAPS.max(8) / 2` rows before `s` and as many from
/// it on must be readable and writable.
pub(super) unsafe fn horizontal<const TAPS: usize>(s: *mut u8, pitch: isize, blimit: u8, limit: u8, thresh: u8) {
    let (p, q) = unsafe { (side::<8>(s, pitch, -1, -1), side::<8>(s, pitch, 0, 1)) };
    // Samples that are the same across the edge stay as they are.
    if none::<8>(differs(&p, &q, p[0])) {
        if TAPS < 16 {
            return;
        }
        let (op, oq) = unsafe { (side::<8>(s, pitch, -5, -1), side::<8>(s, pitch, 4, 1)) };
        if none::<8>(differs(&op, &oq, p[0])) {
            return;
        }
    }
    let (b, l, t) = (u8x16_splat(blimit), u8x16_splat(limit), u8x16_splat(thresh));
    unsafe { horizontal_lanes::<TAPS, 8>(s, pitch, p, q, b, l, t) }
}

/// Filters two edges of 8 samples side by side along a horizontal edge, the
/// second from `s + 8`, each with its own limits, with up to the filter of
/// `TAPS` samples: 4 or 8.
///
/// # Safety
///
/// Columns `s[0..16]` of the 4 rows before `s` and the 4 from it on must be
/// readable and writable.
pub(super) unsafe fn horizontal_dual<const TAPS: usize>(
    s: *mut u8,
    pitch: isize,
    blimit0: u8,
    limit0: u8,
    thresh0: u8,
    blimit1: u8,
    limit1: u8,
    thresh1: u8,
) {
    let (p, q) = unsafe { (side::<16>(s, pitch, -1, -1), side::<16>(s, pitch, 0, 1)) };
    let d = differs(&p, &q, p[0]);
    match (u64x2_extract_lane::<0>(d) != 0, u64x2_extract_lane::<1>(d) != 0) {
        (false, false) => {}
        (true, false) => unsafe { horizontal::<TAPS>(s, pitch, blimit0, limit0, thresh0) },
        (false, true) => unsafe { horizontal::<TAPS>(s.add(8), pitch, blimit1, limit1, thresh1) },
        (true, true) => unsafe {
            let (b, l, t) = (limits(blimit0, blimit1), limits(limit0, limit1), limits(thresh0, thresh1));
            horizontal_lanes::<TAPS, 16>(s, pitch, p, q, b, l, t)
        },
    }
}

/// Eight lines of 8 turned about their diagonal, two lines to a vector: line
/// `2i` of the result is the low half of vector `i` and line `2i + 1` its
/// high half. The lines are the low 8 bytes of each vector, or with `HIGH`
/// the high 8.
#[inline(always)]
fn transpose<const HIGH: bool>(v: [v128; 8]) -> [v128; 4] {
    let bytes = |a, b| {
        if HIGH {
            i8x16_shuffle::<8, 24, 9, 25, 10, 26, 11, 27, 12, 28, 13, 29, 14, 30, 15, 31>(a, b)
        } else {
            i8x16_shuffle::<0, 16, 1, 17, 2, 18, 3, 19, 4, 20, 5, 21, 6, 22, 7, 23>(a, b)
        }
    };
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

/// The lines of two `transpose`s side by side, the first's in the low halves
/// and the second's in the high.
#[inline(always)]
fn lines2(a: [v128; 4], b: [v128; 4]) -> [v128; 8] {
    let even = |i: usize| i64x2_shuffle::<0, 2>(a[i], b[i]);
    let odd = |i: usize| i64x2_shuffle::<1, 3>(a[i], b[i]);
    [even(0), odd(0), even(1), odd(1), even(2), odd(2), even(3), odd(3)]
}

/// Writes rows of the 4 columns about a vertical edge from their lines `p1 p0
/// q0 q1` across it, the rows of the low 8 lanes from `s`, or with `HIGH` of
/// the high 8.
#[inline(always)]
unsafe fn store_inner<const HIGH: bool>(s: *mut u8, pitch: isize, po: &[v128; 8], qo: &[v128; 8]) {
    let bytes = |a, b| {
        if HIGH {
            i8x16_shuffle::<8, 24, 9, 25, 10, 26, 11, 27, 12, 28, 13, 29, 14, 30, 15, 31>(a, b)
        } else {
            i8x16_shuffle::<0, 16, 1, 17, 2, 18, 3, 19, 4, 20, 5, 21, 6, 22, 7, 23>(a, b)
        }
    };
    let (before, after) = (bytes(po[1], po[0]), bytes(qo[0], qo[1]));
    let top = i16x8_shuffle::<0, 8, 1, 9, 2, 10, 3, 11>(before, after);
    let bottom = i16x8_shuffle::<4, 12, 5, 13, 6, 14, 7, 15>(before, after);
    let row = |r: isize| unsafe { s.offset(r * pitch - 2) as *mut u32 };
    unsafe {
        v128_store32_lane::<0>(top, row(0));
        v128_store32_lane::<1>(top, row(1));
        v128_store32_lane::<2>(top, row(2));
        v128_store32_lane::<3>(top, row(3));
        v128_store32_lane::<0>(bottom, row(4));
        v128_store32_lane::<1>(bottom, row(5));
        v128_store32_lane::<2>(bottom, row(6));
        v128_store32_lane::<3>(bottom, row(7));
    }
}

/// Writes 8 rows of 8 columns from `at` on from the columns' lines, the rows
/// of the low 8 lanes from `s`, or with `HIGH` of the high 8.
#[inline(always)]
unsafe fn store_rows<const HIGH: bool>(s: *mut u8, pitch: isize, at: isize, c: [v128; 8]) {
    let rows = transpose::<HIGH>(c);
    for i in 0..4 {
        unsafe {
            v128_store64_lane::<0>(rows[i], s.offset(2 * i as isize * pitch + at) as *mut u64);
            v128_store64_lane::<1>(rows[i], s.offset((2 * i as isize + 1) * pitch + at) as *mut u64);
        }
    }
}

/// The 8 samples from `at` on in row `r` of a vertical edge.
#[inline(always)]
unsafe fn word(s: *mut u8, pitch: isize, r: usize, at: isize) -> u64 {
    unsafe { (s.offset(r as isize * pitch + at) as *const u64).read_unaligned() }
}

/// Whether one of the `rows` rows from `s` is not one value in the 8 samples
/// about a vertical edge.
#[inline(always)]
unsafe fn vertical_differs(s: *mut u8, pitch: isize, rows: core::ops::Range<usize>) -> bool {
    let mut differs = 0;
    for r in rows {
        let x = unsafe { word(s, pitch, r, -4) };
        differs |= x ^ x.rotate_left(8);
    }
    differs != 0
}

/// Filters 8 rows along a vertical edge with up to the filter of `TAPS`
/// samples: 4, 8 or 16.
///
/// # Safety
///
/// The `TAPS.max(8) / 2` samples before `s` and as many from it on must be
/// readable and writable in each of 8 rows.
pub(super) unsafe fn vertical<const TAPS: usize>(s: *mut u8, pitch: isize, blimit: u8, limit: u8, thresh: u8) {
    let word = |r: usize, at: isize| unsafe { word(s, pitch, r, at) };
    // The 4 samples before `s - 4` and the 4 from `s + 4` on in row `r`.
    let outer_row = |r: usize| unsafe {
        let before = (s.offset(r as isize * pitch - 8) as *const u32).read_unaligned();
        let after = (s.offset(r as isize * pitch + 4) as *const u32).read_unaligned();
        u32x4(before, after, 0, 0)
    };
    // Columns p7 p6 p5 p4 q4 q5 q6 q7.
    let outer = || {
        let c = lines(transpose::<false>(core::array::from_fn(outer_row)));
        ([c[3], c[2], c[1], c[0]], [c[4], c[5], c[6], c[7]])
    };

    // Rows that are each one value stay as they are.
    if !unsafe { vertical_differs(s, pitch, 0..8) } {
        if TAPS < 16 {
            return;
        }
        // The halves of a row share its middle, so they are one value when
        // they are the same.
        let mut differs = 0;
        for r in 0..8 {
            differs |= word(r, -8) ^ word(r, 0);
        }
        if differs == 0 {
            return;
        }
    }

    // Columns p3 p2 p1 p0 q0 q1 q2 q3.
    let c = lines(transpose::<false>(core::array::from_fn(|r| u64x2(word(r, -4), 0))));
    let (b, l, t) = (u8x16_splat(blimit), u8x16_splat(limit), u8x16_splat(thresh));
    let (po, qo, reach) = filter::<TAPS, 8>([c[3], c[2], c[1], c[0]], [c[4], c[5], c[6], c[7]], outer, b, l, t);
    unsafe {
        match reach {
            0 => {}
            2 => store_inner::<false>(s, pitch, &po, &qo),
            3 => store_rows::<false>(s, pitch, -4, [po[3], po[2], po[1], po[0], qo[0], qo[1], qo[2], qo[3]]),
            _ => {
                store_rows::<false>(s, pitch, -8, [po[7], po[6], po[5], po[4], po[3], po[2], po[1], po[0]]);
                store_rows::<false>(s, pitch, 0, qo);
            }
        }
    }
}

/// Filters two edges of 8 rows one above the other along a vertical edge, the
/// second from row 8, each with its own limits, with up to the filter of
/// `TAPS` samples: 4 or 8.
///
/// # Safety
///
/// The 4 samples before `s` and the 4 from it on must be readable and writable
/// in each of 16 rows.
pub(super) unsafe fn vertical_dual<const TAPS: usize>(
    s: *mut u8,
    pitch: isize,
    blimit0: u8,
    limit0: u8,
    thresh0: u8,
    blimit1: u8,
    limit1: u8,
    thresh1: u8,
) {
    let second = unsafe { s.offset(8 * pitch) };
    match unsafe { (vertical_differs(s, pitch, 0..8), vertical_differs(s, pitch, 8..16)) } {
        (false, false) => return,
        (true, false) => return unsafe { vertical::<TAPS>(s, pitch, blimit0, limit0, thresh0) },
        (false, true) => return unsafe { vertical::<TAPS>(second, pitch, blimit1, limit1, thresh1) },
        (true, true) => {}
    }

    // Columns p3 p2 p1 p0 q0 q1 q2 q3, rows 0 to 7 in the low halves and 8 to
    // 15 in the high.
    let rows = |from: usize| transpose::<false>(core::array::from_fn(|r| u64x2(unsafe { word(s, pitch, from + r, -4) }, 0)));
    let c = lines2(rows(0), rows(8));
    let (b, l, t) = (limits(blimit0, blimit1), limits(limit0, limit1), limits(thresh0, thresh1));
    let (po, qo, reach) = filter::<TAPS, 16>([c[3], c[2], c[1], c[0]], [c[4], c[5], c[6], c[7]], no_outer, b, l, t);
    unsafe {
        match reach {
            0 => {}
            2 => {
                store_inner::<false>(s, pitch, &po, &qo);
                store_inner::<true>(second, pitch, &po, &qo);
            }
            _ => {
                let c = [po[3], po[2], po[1], po[0], qo[0], qo[1], qo[2], qo[3]];
                store_rows::<false>(s, pitch, -4, c);
                store_rows::<true>(second, pitch, -4, c);
            }
        }
    }
}
