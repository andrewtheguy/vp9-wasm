//! Unscaled motion-compensated prediction at 8 bits.
//!
//! Transcribed from libvpx 1.16.0: the convolutions of
//! `vpx_dsp/vpx_convolve.c` at a step of one whole sample, and the kernels of
//! `vp9/common/vp9_filter.c`.

use core::mem::MaybeUninit;
use core::ptr::copy_nonoverlapping;

/// The eight-tap kernels in libvpx's `INTERP_FILTER` order: 0 `EIGHTTAP`
/// (regular), 1 `EIGHTTAP_SMOOTH`, 2 `EIGHTTAP_SHARP`, 3 `BILINEAR`;
/// `[subpel position 0..16][tap]`.
pub static FILTERS: [[[i16; 8]; 16]; 4] = [
    // EIGHTTAP: sub_pel_filters_8, the regular one.
    [
        [0, 0, 0, 128, 0, 0, 0, 0],
        [0, 1, -5, 126, 8, -3, 1, 0],
        [-1, 3, -10, 122, 18, -6, 2, 0],
        [-1, 4, -13, 118, 27, -9, 3, -1],
        [-1, 4, -16, 112, 37, -11, 4, -1],
        [-1, 5, -18, 105, 48, -14, 4, -1],
        [-1, 5, -19, 97, 58, -16, 5, -1],
        [-1, 6, -19, 88, 68, -18, 5, -1],
        [-1, 6, -19, 78, 78, -19, 6, -1],
        [-1, 5, -18, 68, 88, -19, 6, -1],
        [-1, 5, -16, 58, 97, -19, 5, -1],
        [-1, 4, -14, 48, 105, -18, 5, -1],
        [-1, 4, -11, 37, 112, -16, 4, -1],
        [-1, 3, -9, 27, 118, -13, 4, -1],
        [0, 2, -6, 18, 122, -10, 3, -1],
        [0, 1, -3, 8, 126, -5, 1, 0],
    ],
    // EIGHTTAP_SMOOTH: sub_pel_filters_8lp.
    [
        [0, 0, 0, 128, 0, 0, 0, 0],
        [-3, -1, 32, 64, 38, 1, -3, 0],
        [-2, -2, 29, 63, 41, 2, -3, 0],
        [-2, -2, 26, 63, 43, 4, -4, 0],
        [-2, -3, 24, 62, 46, 5, -4, 0],
        [-2, -3, 21, 60, 49, 7, -4, 0],
        [-1, -4, 18, 59, 51, 9, -4, 0],
        [-1, -4, 16, 57, 53, 12, -4, -1],
        [-1, -4, 14, 55, 55, 14, -4, -1],
        [-1, -4, 12, 53, 57, 16, -4, -1],
        [0, -4, 9, 51, 59, 18, -4, -1],
        [0, -4, 7, 49, 60, 21, -3, -2],
        [0, -4, 5, 46, 62, 24, -3, -2],
        [0, -4, 4, 43, 63, 26, -2, -2],
        [0, -3, 2, 41, 63, 29, -2, -2],
        [0, -3, 1, 38, 64, 32, -1, -3],
    ],
    // EIGHTTAP_SHARP: sub_pel_filters_8s.
    [
        [0, 0, 0, 128, 0, 0, 0, 0],
        [-1, 3, -7, 127, 8, -3, 1, 0],
        [-2, 5, -13, 125, 17, -6, 3, -1],
        [-3, 7, -17, 121, 27, -10, 5, -2],
        [-4, 9, -20, 115, 37, -13, 6, -2],
        [-4, 10, -23, 108, 48, -16, 8, -3],
        [-4, 10, -24, 100, 59, -19, 9, -3],
        [-4, 11, -24, 90, 70, -21, 10, -4],
        [-4, 11, -23, 80, 80, -23, 11, -4],
        [-4, 10, -21, 70, 90, -24, 11, -4],
        [-3, 9, -19, 59, 100, -24, 10, -4],
        [-3, 8, -16, 48, 108, -23, 10, -4],
        [-2, 6, -13, 37, 115, -20, 9, -4],
        [-2, 5, -10, 27, 121, -17, 7, -3],
        [-1, 3, -6, 17, 125, -13, 5, -2],
        [0, 1, -3, 8, 127, -7, 3, -1],
    ],
    // BILINEAR: bilinear_filters.
    [
        [0, 0, 0, 128, 0, 0, 0, 0],
        [0, 0, 0, 120, 8, 0, 0, 0],
        [0, 0, 0, 112, 16, 0, 0, 0],
        [0, 0, 0, 104, 24, 0, 0, 0],
        [0, 0, 0, 96, 32, 0, 0, 0],
        [0, 0, 0, 88, 40, 0, 0, 0],
        [0, 0, 0, 80, 48, 0, 0, 0],
        [0, 0, 0, 72, 56, 0, 0, 0],
        [0, 0, 0, 64, 64, 0, 0, 0],
        [0, 0, 0, 56, 72, 0, 0, 0],
        [0, 0, 0, 48, 80, 0, 0, 0],
        [0, 0, 0, 40, 88, 0, 0, 0],
        [0, 0, 0, 32, 96, 0, 0, 0],
        [0, 0, 0, 24, 104, 0, 0, 0],
        [0, 0, 0, 16, 112, 0, 0, 0],
        [0, 0, 0, 8, 120, 0, 0, 0],
    ],
];

/// The taps reach 3 samples before the one predicted and 4 after it.
const BEFORE: usize = 3;
/// The rows the horizontal pass makes for the vertical one, past the block's.
const EXTRA: usize = 7;

/// One sample: eight taps a `step` apart starting at `p`, rounded to 7 bits
/// and clipped.
#[inline(always)]
unsafe fn tap(p: *const u8, step: usize, f: &[i16; 8]) -> u8 {
    let mut sum = 64;
    for k in 0..8 {
        sum += unsafe { *p.add(k * step) } as i32 * f[k] as i32;
    }
    (sum >> 7).clamp(0, 255) as u8
}

/// libvpx's `vpx_convolve_copy`.
unsafe fn copy<const W: usize>(src: *const u8, src_stride: usize, dst: *mut u8, dst_stride: usize, h: usize) {
    for y in 0..h {
        unsafe { copy_nonoverlapping(src.add(y * src_stride), dst.add(y * dst_stride), W) };
    }
}

/// libvpx's `vpx_convolve8_horiz`.
unsafe fn horiz<const W: usize>(
    src: *const u8,
    src_stride: usize,
    dst: *mut u8,
    dst_stride: usize,
    f: &[i16; 8],
    h: usize,
) {
    unsafe {
        let src = src.sub(BEFORE);
        for y in 0..h {
            let from = src.add(y * src_stride);
            let mut row = [0u8; W];
            for x in 0..W {
                row[x] = tap(from.add(x), 1, f);
            }
            copy_nonoverlapping(row.as_ptr(), dst.add(y * dst_stride), W);
        }
    }
}

/// libvpx's `vpx_convolve8_vert`.
unsafe fn vert<const W: usize>(
    src: *const u8,
    src_stride: usize,
    dst: *mut u8,
    dst_stride: usize,
    f: &[i16; 8],
    h: usize,
) {
    unsafe {
        let src = src.sub(BEFORE * src_stride);
        for y in 0..h {
            let from = src.add(y * src_stride);
            let mut row = [0u8; W];
            for x in 0..W {
                row[x] = tap(from.add(x), src_stride, f);
            }
            copy_nonoverlapping(row.as_ptr(), dst.add(y * dst_stride), W);
        }
    }
}

/// libvpx's `vpx_convolve8`: across into an intermediate of 8-bit samples,
/// then down from it. `h` is at most 64.
unsafe fn both<const W: usize>(
    src: *const u8,
    src_stride: usize,
    dst: *mut u8,
    dst_stride: usize,
    fx: &[i16; 8],
    fy: &[i16; 8],
    h: usize,
) {
    let mut temp = [MaybeUninit::<u8>::uninit(); 64 * (64 + EXTRA)];
    let temp = temp.as_mut_ptr() as *mut u8;
    unsafe {
        horiz::<W>(src.sub(BEFORE * src_stride), src_stride, temp, 64, fx, h + EXTRA);
        vert::<W>(temp.add(BEFORE * 64), 64, dst, dst_stride, fy, h);
    }
}

/// Writes the w×h block predicted from `src` to `dst`.
///
/// `src` points at the block's whole-sample position in the reference; `mx`
/// and `my` in 0..16 are the sixteenth-sample fractions, and `filter` indexes
/// [`FILTERS`]. `w` and `h` are each one of 4, 8, 16, 32 and 64; any other size
/// writes nothing.
///
/// The result is what libvpx's `sf->predict[mx != 0][my != 0][0]` gives for an
/// unscaled reference: `vpx_convolve_copy`, `vpx_convolve8_horiz`,
/// `vpx_convolve8_vert`, or `vpx_convolve8`, whose intermediate is rounded and
/// clipped to 8 bits between the passes.
///
/// # Safety
///
/// The block must be readable at `src`, with the 3 columns before it and the 4
/// after when `mx` is not 0, and the 3 rows above it and the 4 below when `my`
/// is not 0; the block at `dst` must be writable and apart from them.
pub unsafe fn predict(
    src: *const u8,
    src_stride: usize,
    dst: *mut u8,
    dst_stride: usize,
    filter: usize,
    mx: usize,
    my: usize,
    w: usize,
    h: usize,
) {
    if !matches!(h, 4 | 8 | 16 | 32 | 64) {
        return;
    }
    let kernels = &FILTERS[filter & 3];
    let (fx, fy) = (&kernels[mx & 15], &kernels[my & 15]);
    macro_rules! sized {
        ($w:literal) => {
            match (mx & 15 != 0, my & 15 != 0) {
                (false, false) => copy::<$w>(src, src_stride, dst, dst_stride, h),
                (true, false) => horiz::<$w>(src, src_stride, dst, dst_stride, fx, h),
                (false, true) => vert::<$w>(src, src_stride, dst, dst_stride, fy, h),
                (true, true) => both::<$w>(src, src_stride, dst, dst_stride, fx, fy, h),
            }
        };
    }
    unsafe {
        match w {
            4 => sized!(4),
            8 => sized!(8),
            16 => sized!(16),
            32 => sized!(32),
            64 => sized!(64),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lcg(state: &mut u64) -> u8 {
        *state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (*state >> 56) as u8
    }

    /// Each filter in each of the four directions at a spread of sizes over
    /// noise, hashed: the value libvpx's C gives for the same input.
    #[test]
    fn matches_libvpx_known_answer() {
        let (mut state, mut hash) = (1, 0xcbf29ce484222325u64);
        let mut src = [0u8; 80 * 80];
        src.iter_mut().for_each(|v| *v = lcg(&mut state));
        for filter in 0..4 {
            for (mx, my) in [(0, 0), (5, 0), (0, 11), (3, 14), (15, 1)] {
                for (w, h) in [(4, 4), (8, 16), (64, 64), (32, 8), (16, 64)] {
                    let mut dst = [0u8; 64 * 64];
                    let from = src[8 * 80 + 8..].as_ptr();
                    unsafe { predict(from, 80, dst.as_mut_ptr(), 64, filter, mx, my, w, h) };
                    for b in dst {
                        hash = (hash ^ b as u64).wrapping_mul(0x100000001b3);
                    }
                }
            }
        }
        assert_eq!(hash, 0xf5de7a112cd86b24);
    }

    #[test]
    fn kernels_sum_to_one() {
        for kernel in FILTERS.iter().flatten() {
            assert_eq!(kernel.iter().sum::<i16>(), 128);
        }
        assert!(FILTERS.iter().all(|f| f[0] == [0, 0, 0, 128, 0, 0, 0, 0]));
    }

    /// Halfway between two samples with the bilinear kernel is their rounded
    /// mean, across, down, and both with the first pass rounded on its own.
    #[test]
    fn bilinear_half_sample() {
        let mut src = [0u8; 16 * 16];
        // The block's four corners: 10 11 / 13 20, the rest of its samples 0.
        src[4 * 16 + 4..][..2].copy_from_slice(&[10, 11]);
        src[5 * 16 + 4..][..2].copy_from_slice(&[13, 20]);
        let run = |mx, my| {
            let mut dst = [0u8; 16];
            unsafe { predict(src[4 * 16 + 4..].as_ptr(), 16, dst.as_mut_ptr(), 4, 3, mx, my, 4, 4) };
            dst[0]
        };
        // (10 + 11 + 1) / 2 = 11 and (13 + 20 + 1) / 2 = 17, then (11 + 17) / 2.
        assert_eq!([run(0, 0), run(8, 0), run(0, 8), run(8, 8)], [10, 11, 12, 14]);
    }
}
