//! Intra prediction at 8 bits.
//!
//! Transcribed from libvpx 1.16.0: the predictors of `vpx_dsp/intrapred.c`,
//! bound to sizes as `vp9_init_intra_predictors_internal` of
//! `vp9/common/vp9_reconintra.c` binds them. D45 and D63 have a 4×4 form of
//! their own that reads further right than the larger sizes do; the other 4×4
//! functions libvpx names give what the sized ones give at 4.

use core::ptr::{copy_nonoverlapping, write_bytes};

/// A predictor: libvpx's intra `PREDICTION_MODE`s in order, then the three DC
/// forms used when an edge is missing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Pred {
    Dc = 0,
    V = 1,
    H = 2,
    D45 = 3,
    D135 = 4,
    D117 = 5,
    D153 = 6,
    D207 = 7,
    D63 = 8,
    Tm = 9,
    DcLeft = 10,
    DcTop = 11,
    Dc128 = 12,
}

#[inline(always)]
fn avg2(a: u8, b: u8) -> u8 {
    ((a as u32 + b as u32 + 1) >> 1) as u8
}

#[inline(always)]
fn avg3(a: u8, b: u8, c: u8) -> u8 {
    ((a as u32 + 2 * b as u32 + c as u32 + 2) >> 2) as u8
}

/// Fills the N×N block with one value.
#[inline(always)]
unsafe fn fill<const N: usize>(dst: *mut u8, stride: usize, v: u8) {
    for r in 0..N {
        unsafe { write_bytes(dst.add(r * stride), v, N) };
    }
}

/// The sum of N samples.
#[inline(always)]
unsafe fn sum<const N: usize>(p: *const u8) -> u32 {
    unsafe { &*(p as *const [u8; N]) }.iter().map(|&v| v as u32).sum()
}

unsafe fn dc<const N: usize>(dst: *mut u8, stride: usize, above: *const u8, left: *const u8) {
    unsafe {
        let total = sum::<N>(above) + sum::<N>(left);
        fill::<N>(dst, stride, ((total + N as u32) / (2 * N as u32)) as u8);
    }
}

/// DC from one edge alone: the row above or the column to the left.
unsafe fn dc_edge<const N: usize>(dst: *mut u8, stride: usize, edge: *const u8) {
    unsafe {
        let total = sum::<N>(edge);
        fill::<N>(dst, stride, ((total + (N as u32 >> 1)) / N as u32) as u8);
    }
}

unsafe fn v<const N: usize>(dst: *mut u8, stride: usize, above: *const u8) {
    unsafe {
        for r in 0..N {
            copy_nonoverlapping(above, dst.add(r * stride), N);
        }
    }
}

unsafe fn h<const N: usize>(dst: *mut u8, stride: usize, left: *const u8) {
    unsafe {
        let l = &*(left as *const [u8; N]);
        for r in 0..N {
            write_bytes(dst.add(r * stride), l[r], N);
        }
    }
}

unsafe fn tm<const N: usize>(dst: *mut u8, stride: usize, above: *const u8, left: *const u8) {
    unsafe {
        let a = &*(above as *const [u8; N]);
        let l = &*(left as *const [u8; N]);
        let top_left = *above.sub(1) as i32;
        for r in 0..N {
            let base = l[r] as i32 - top_left;
            let mut row = [0u8; N];
            for c in 0..N {
                row[c] = (base + a[c] as i32).clamp(0, 255) as u8;
            }
            copy_nonoverlapping(row.as_ptr(), dst.add(r * stride), N);
        }
    }
}

unsafe fn d207<const N: usize>(dst: *mut u8, stride: usize, left: *const u8) {
    unsafe {
        let l = &*(left as *const [u8; N]);
        let last = l[N - 1];
        // First column.
        for r in 0..N - 1 {
            *dst.add(r * stride) = avg2(l[r], l[r + 1]);
        }
        *dst.add((N - 1) * stride) = last;
        // Second column.
        for r in 0..N - 2 {
            *dst.add(r * stride + 1) = avg3(l[r], l[r + 1], l[r + 2]);
        }
        *dst.add((N - 2) * stride + 1) = avg3(l[N - 2], last, last);
        *dst.add((N - 1) * stride + 1) = last;
        // The rest of the last row, then each row from the one below it.
        write_bytes(dst.add((N - 1) * stride + 2), last, N - 2);
        for r in (0..N - 1).rev() {
            copy_nonoverlapping(dst.add((r + 1) * stride), dst.add(r * stride + 2), N - 2);
        }
    }
}

unsafe fn d63<const N: usize>(dst: *mut u8, stride: usize, above: *const u8) {
    unsafe {
        let a = |i: usize| *above.add(i);
        for c in 0..N {
            *dst.add(c) = avg2(a(c), a(c + 1));
            *dst.add(stride + c) = avg3(a(c), a(c + 1), a(c + 2));
        }
        let pad = a(N - 1);
        for i in 1..N / 2 {
            let size = N - 1 - i;
            for k in 0..2 {
                let row = dst.add((2 * i + k) * stride);
                copy_nonoverlapping(dst.add(k * stride + i), row, size);
                write_bytes(row.add(size), pad, N - size);
            }
        }
    }
}

/// libvpx's `vpx_d63_predictor_4x4_c`.
unsafe fn d63_4x4(dst: *mut u8, stride: usize, above: *const u8) {
    unsafe {
        let a = &*(above as *const [u8; 7]);
        for y in 0..4 {
            for x in 0..4 {
                let i = x + y / 2;
                *dst.add(x + y * stride) =
                    if y & 1 == 0 { avg2(a[i], a[i + 1]) } else { avg3(a[i], a[i + 1], a[i + 2]) };
            }
        }
    }
}

unsafe fn d45<const N: usize>(dst: *mut u8, stride: usize, above: *const u8) {
    unsafe {
        let a = |i: usize| *above.add(i);
        let above_right = a(N - 1);
        for x in 0..N - 1 {
            *dst.add(x) = avg3(a(x), a(x + 1), a(x + 2));
        }
        *dst.add(N - 1) = above_right;
        for x in 1..N {
            let row = dst.add(x * stride);
            let size = N - 1 - x;
            copy_nonoverlapping(dst.add(x), row, size);
            write_bytes(row.add(size), above_right, x + 1);
        }
    }
}

/// libvpx's `vpx_d45_predictor_4x4_c`.
unsafe fn d45_4x4(dst: *mut u8, stride: usize, above: *const u8) {
    unsafe {
        let a = &*(above as *const [u8; 8]);
        let mut diagonal = [a[7]; 7];
        for i in 0..6 {
            diagonal[i] = avg3(a[i], a[i + 1], a[i + 2]);
        }
        for y in 0..4 {
            copy_nonoverlapping(diagonal.as_ptr().add(y), dst.add(y * stride), 4);
        }
    }
}

unsafe fn d117<const N: usize>(dst: *mut u8, stride: usize, above: *const u8, left: *const u8) {
    unsafe {
        let l = &*(left as *const [u8; N]);
        // t(0) is the top-left sample, t(1 + c) the one above column c.
        let t = |i: usize| *above.sub(1).add(i);
        // First row.
        for c in 0..N {
            *dst.add(c) = avg2(t(c), t(c + 1));
        }
        // Second row.
        *dst.add(stride) = avg3(l[0], t(0), t(1));
        for c in 1..N {
            *dst.add(stride + c) = avg3(t(c - 1), t(c), t(c + 1));
        }
        // The rest of the first column.
        *dst.add(2 * stride) = avg3(t(0), l[0], l[1]);
        for r in 3..N {
            *dst.add(r * stride) = avg3(l[r - 3], l[r - 2], l[r - 1]);
        }
        // The rest of the block.
        for r in 2..N {
            copy_nonoverlapping(dst.add((r - 2) * stride), dst.add(r * stride + 1), N - 1);
        }
    }
}

unsafe fn d135<const N: usize>(dst: *mut u8, stride: usize, above: *const u8, left: *const u8) {
    unsafe {
        let l = &*(left as *const [u8; N]);
        let t = |i: usize| *above.sub(1).add(i);
        // The outer border from the bottom-left corner to the top-right one.
        let mut border = [0u8; 64];
        for i in 0..N - 2 {
            border[i] = avg3(l[N - 3 - i], l[N - 2 - i], l[N - 1 - i]);
        }
        border[N - 2] = avg3(t(0), l[0], l[1]);
        border[N - 1] = avg3(l[0], t(0), t(1));
        border[N] = avg3(t(0), t(1), t(2));
        for i in 0..N - 2 {
            border[N + 1 + i] = avg3(t(i + 1), t(i + 2), t(i + 3));
        }
        for r in 0..N {
            copy_nonoverlapping(border[N - 1 - r..][..N].as_ptr(), dst.add(r * stride), N);
        }
    }
}

unsafe fn d153<const N: usize>(dst: *mut u8, stride: usize, above: *const u8, left: *const u8) {
    unsafe {
        let l = &*(left as *const [u8; N]);
        let t = |i: usize| *above.sub(1).add(i);
        // First column.
        *dst = avg2(t(0), l[0]);
        for r in 1..N {
            *dst.add(r * stride) = avg2(l[r - 1], l[r]);
        }
        // Second column.
        *dst.add(1) = avg3(l[0], t(0), t(1));
        *dst.add(stride + 1) = avg3(t(0), l[0], l[1]);
        for r in 2..N {
            *dst.add(r * stride + 1) = avg3(l[r - 2], l[r - 1], l[r]);
        }
        // The rest of the first row, then each row from the one above it.
        for c in 0..N - 2 {
            *dst.add(2 + c) = avg3(t(c), t(c + 1), t(c + 2));
        }
        for r in 1..N {
            copy_nonoverlapping(dst.add((r - 1) * stride), dst.add(r * stride + 2), N - 2);
        }
    }
}

/// Predicts the bs×bs block at `dst` (bs = 4 << tx_size, tx_size 0..=3; any
/// other size writes nothing).
///
/// `above` points at the sample above the block's first column: `above[-1]` is
/// the top-left sample, `above[0..2 * bs]` the row above and above-right;
/// `left[0..bs]` is the column to the left. Exactly the arguments of libvpx's
/// `intra_pred_fn(dst, stride, above, left)`.
///
/// # Safety
///
/// Those ranges must be readable, `dst..dst + (bs - 1) * stride + bs` writable
/// and apart from them, and `stride` at least bs.
pub unsafe fn predict(
    pred: Pred,
    tx_size: usize,
    dst: *mut u8,
    stride: usize,
    above: *const u8,
    left: *const u8,
) {
    macro_rules! sized {
        ($n:literal) => {
            match pred {
                Pred::Dc => dc::<$n>(dst, stride, above, left),
                Pred::V => v::<$n>(dst, stride, above),
                Pred::H => h::<$n>(dst, stride, left),
                Pred::D45 => d45::<$n>(dst, stride, above),
                Pred::D135 => d135::<$n>(dst, stride, above, left),
                Pred::D117 => d117::<$n>(dst, stride, above, left),
                Pred::D153 => d153::<$n>(dst, stride, above, left),
                Pred::D207 => d207::<$n>(dst, stride, left),
                Pred::D63 => d63::<$n>(dst, stride, above),
                Pred::Tm => tm::<$n>(dst, stride, above, left),
                Pred::DcLeft => dc_edge::<$n>(dst, stride, left),
                Pred::DcTop => dc_edge::<$n>(dst, stride, above),
                Pred::Dc128 => fill::<$n>(dst, stride, 128),
            }
        };
    }
    unsafe {
        match tx_size {
            0 => match pred {
                Pred::D45 => d45_4x4(dst, stride, above),
                Pred::D63 => d63_4x4(dst, stride, above),
                _ => sized!(4),
            },
            1 => sized!(8),
            2 => sized!(16),
            3 => sized!(32),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Pred; 13] = [
        Pred::Dc,
        Pred::V,
        Pred::H,
        Pred::D45,
        Pred::D135,
        Pred::D117,
        Pred::D153,
        Pred::D207,
        Pred::D63,
        Pred::Tm,
        Pred::DcLeft,
        Pred::DcTop,
        Pred::Dc128,
    ];

    fn lcg(state: &mut u64) -> u8 {
        *state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (*state >> 56) as u8
    }

    /// Every predictor at every size over one pseudo-random edge, hashed: the
    /// value libvpx's C gives for the same input.
    #[test]
    fn matches_libvpx_known_answer() {
        let (mut state, mut hash) = (1, 0xcbf29ce484222325u64);
        let (mut edge, mut left) = ([0u8; 65], [0u8; 32]);
        edge.iter_mut().chain(&mut left).for_each(|v| *v = lcg(&mut state));
        for (i, pred) in ALL.into_iter().enumerate() {
            assert_eq!(pred as usize, i);
            for tx_size in 0..4 {
                let mut dst = [0u8; 32 * 32];
                unsafe { predict(pred, tx_size, dst.as_mut_ptr(), 32, edge[1..].as_ptr(), left.as_ptr()) };
                for b in dst {
                    hash = (hash ^ b as u64).wrapping_mul(0x100000001b3);
                }
            }
        }
        assert_eq!(hash, 0xd7f6f8ee4390ef3b);
    }

    /// At 4×4, D45 runs on into the above-right samples; from 8×8 up it repeats
    /// the last sample above the block instead.
    #[test]
    fn d45_reads_above_right_only_at_4x4() {
        let mut edge = [0u8; 17];
        edge.iter_mut().enumerate().for_each(|(i, v)| *v = i as u8 * 10);
        let above = edge[1..].as_ptr();
        // above[i] is 10 * (i + 1).
        let mut dst = [0u8; 8 * 8];
        unsafe { predict(Pred::D45, 0, dst.as_mut_ptr(), 8, above, above) };
        assert_eq!([dst[0], dst[3], dst[2 * 8 + 3], dst[3 * 8 + 3]], [20, 50, 70, 80]);
        unsafe { predict(Pred::D45, 1, dst.as_mut_ptr(), 8, above, above) };
        assert_eq!([dst[0], dst[6], dst[7], dst[7 * 8], dst[7 * 8 + 7]], [20, 80, 80, 80, 80]);
    }

    #[test]
    fn dc_forms() {
        let edge = [9u8, 10, 20, 30, 41, 0, 0, 0, 0];
        let left = [50u8, 60, 70, 81];
        let above = edge[1..].as_ptr();
        let run = |pred| {
            let mut dst = [0u8; 16];
            unsafe { predict(pred, 0, dst.as_mut_ptr(), 4, above, left.as_ptr()) };
            assert!(dst.iter().all(|&v| v == dst[0]));
            dst[0]
        };
        // (101 + 2) / 4, (261 + 2) / 4, (362 + 4) / 8.
        assert_eq!([run(Pred::DcTop), run(Pred::DcLeft), run(Pred::Dc), run(Pred::Dc128)], [25, 65, 45, 128]);
    }
}
