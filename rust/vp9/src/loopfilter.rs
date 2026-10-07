//! The loop filter's edge kernels at 8 bits.
//!
//! Transcribed from libvpx 1.16.0's `vpx_dsp/loopfilter.c`. Each function is
//! libvpx's of the same name without the `vpx_` prefix and `_c` suffix, with the
//! thresholds taken by value. `s` points at the first sample after the edge and
//! `pitch` is the stride: a horizontal edge has its samples `pitch` apart across
//! it, a vertical one has them 1 apart.
//!
//! The plain kernels in `scalar` are the reference, and what every target runs
//! but WebAssembly with SIMD, which runs the same arithmetic from `simd128`.

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
mod simd128;

#[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
use scalar::{horizontal, vertical};
#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
use simd128::{horizontal, vertical};

#[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
mod scalar {
    #[inline(always)]
    fn signed_char_clamp(t: i32) -> i32 {
        t.clamp(-128, 127)
    }

    /// The K samples on each side of the edge, nearest first: `p[i]` is libvpx's
    /// p_i and `q[i]` its q_i. `across` is the distance between them.
    #[inline(always)]
    unsafe fn load<const K: usize>(s: *const u8, across: isize) -> ([i32; K], [i32; K]) {
        let (mut p, mut q) = ([0; K], [0; K]);
        for i in 0..K {
            unsafe {
                p[i] = *s.offset(-(i as isize + 1) * across) as i32;
                q[i] = *s.offset(i as isize * across) as i32;
            }
        }
        (p, q)
    }

    /// Writes sample `i` of the line across the edge, 0 being q0 and -1 p0.
    #[inline(always)]
    unsafe fn store(s: *mut u8, across: isize, i: isize, v: i32) {
        unsafe { *s.offset(i * across) = v as u8 };
    }

    /// Whether the edge is filtered at all.
    #[inline(always)]
    fn filter_mask(limit: i32, blimit: i32, p: &[i32; 4], q: &[i32; 4]) -> bool {
        (p[3] - p[2]).abs() <= limit
            && (p[2] - p[1]).abs() <= limit
            && (p[1] - p[0]).abs() <= limit
            && (q[1] - q[0]).abs() <= limit
            && (q[2] - q[1]).abs() <= limit
            && (q[3] - q[2]).abs() <= limit
            && (p[0] - q[0]).abs() * 2 + (p[1] - q[1]).abs() / 2 <= blimit
    }

    /// Whether samples `from..K` of each side are within 1 of the side's nearest:
    /// libvpx's `flat_mask4` from 1 over four samples, and what `flat_mask5` adds
    /// to it from 4 over eight.
    #[inline(always)]
    fn flat<const K: usize>(p: &[i32; K], q: &[i32; K], from: usize) -> bool {
        (from..K).all(|i| (p[i] - p[0]).abs() <= 1 && (q[i] - q[0]).abs() <= 1)
    }

    /// The narrow filter, for an edge its mask lets through: it changes the two
    /// samples on each side.
    #[inline(always)]
    unsafe fn filter4(s: *mut u8, across: isize, thresh: i32, p: &[i32; 4], q: &[i32; 4]) {
        let (ps1, ps0) = (p[1] - 128, p[0] - 128);
        let (qs0, qs1) = (q[0] - 128, q[1] - 128);
        // High edge variance.
        let hev = (p[1] - p[0]).abs() > thresh || (q[1] - q[0]).abs() > thresh;

        // The outer taps only with high edge variance, then the inner ones.
        let filter = if hev { signed_char_clamp(ps1 - qs1) } else { 0 };
        let filter = signed_char_clamp(filter + 3 * (qs0 - ps0));

        // One side rounds by 4 and the other by 3.
        let filter1 = signed_char_clamp(filter + 4) >> 3;
        let filter2 = signed_char_clamp(filter + 3) >> 3;

        // The outer tap adjustment.
        let filter = if hev { 0 } else { (filter1 + 1) >> 1 };

        unsafe {
            store(s, across, 0, signed_char_clamp(qs0 - filter1) + 128);
            store(s, across, -1, signed_char_clamp(ps0 + filter2) + 128);
            store(s, across, 1, signed_char_clamp(qs1 - filter) + 128);
            store(s, across, -2, signed_char_clamp(ps1 + filter) + 128);
        }
    }

    /// The flat filters: every sample but the outermost of each side becomes the
    /// rounded mean of the 2K - 1 samples around it and itself once more, the line
    /// continued past its ends by its end samples. K = 4 is the 7-tap filter of
    /// libvpx's `filter8`, K = 8 the 15-tap one of its `filter16`.
    #[inline(always)]
    unsafe fn smooth<const K: usize>(s: *mut u8, across: isize, p: &[i32; K], q: &[i32; K]) {
        let k = K as isize;
        let at = |j: isize| if j < 0 { p[((-j - 1) as usize).min(K - 1)] } else { q[(j as usize).min(K - 1)] };
        let shift = K.trailing_zeros() + 1;
        // The window around the first sample written, with the rounding.
        let mut sum = K as i32;
        for j in 2 - 2 * k..=0 {
            sum += at(j);
        }
        for i in 1 - k..k - 1 {
            unsafe { store(s, across, i, (sum + at(i)) >> shift) };
            sum += at(i + k) - at(i + 1 - k);
        }
    }

    /// Filters 8 positions along an edge, each `along` from the last. `TAPS` is
    /// the widest filter the edge may use: 4, 8 or 16 samples across.
    #[inline(always)]
    unsafe fn edge<const TAPS: usize>(
        mut s: *mut u8,
        across: isize,
        along: isize,
        blimit: u8,
        limit: u8,
        thresh: u8,
    ) {
        for _ in 0..8 {
            unsafe {
                let (p, q) = load::<4>(s, across);
                if filter_mask(limit as i32, blimit as i32, &p, &q) {
                    if TAPS >= 8 && flat(&p, &q, 1) {
                        let (p8, q8) = if TAPS == 16 { load::<8>(s, across) } else { ([0; 8], [0; 8]) };
                        if TAPS == 16 && flat(&p8, &q8, 4) {
                            smooth::<8>(s, across, &p8, &q8);
                        } else {
                            smooth::<4>(s, across, &p, &q);
                        }
                    } else {
                        filter4(s, across, thresh as i32, &p, &q);
                    }
                }
                s = s.offset(along);
            }
        }
    }

    /// Filters 8 samples along a horizontal edge.
    pub(super) unsafe fn horizontal<const TAPS: usize>(s: *mut u8, pitch: isize, blimit: u8, limit: u8, thresh: u8) {
        unsafe { edge::<TAPS>(s, pitch, 1, blimit, limit, thresh) }
    }

    /// Filters 8 rows along a vertical edge.
    pub(super) unsafe fn vertical<const TAPS: usize>(s: *mut u8, pitch: isize, blimit: u8, limit: u8, thresh: u8) {
        unsafe { edge::<TAPS>(s, 1, pitch, blimit, limit, thresh) }
    }
}

/// The narrow and the 8-sample filters.
macro_rules! lpf {
    ($taps:literal, $h:ident, $v:ident) => {
        /// Filters 8 samples along a horizontal edge.
        ///
        /// # Safety
        ///
        /// Columns `s[0..8]` of the 4 rows before `s` and the 4 from it on
        /// must be readable and writable.
        pub unsafe fn $h(s: *mut u8, pitch: isize, blimit: u8, limit: u8, thresh: u8) {
            unsafe { horizontal::<$taps>(s, pitch, blimit, limit, thresh) }
        }

        /// Filters 8 rows along a vertical edge.
        ///
        /// # Safety
        ///
        /// The 4 samples before `s` and the 4 from it on must be readable and
        /// writable in each of 8 rows.
        pub unsafe fn $v(s: *mut u8, pitch: isize, blimit: u8, limit: u8, thresh: u8) {
            unsafe { vertical::<$taps>(s, pitch, blimit, limit, thresh) }
        }
    };
}

lpf!(4, lpf_horizontal_4, lpf_vertical_4);
lpf!(8, lpf_horizontal_8, lpf_vertical_8);

/// Filters 8 samples along a horizontal edge with up to the 16-sample filter.
///
/// # Safety
///
/// Columns `s[0..8]` of the 8 rows before `s` and the 8 from it on must be
/// readable and writable.
pub unsafe fn lpf_horizontal_16(s: *mut u8, pitch: isize, blimit: u8, limit: u8, thresh: u8) {
    unsafe { horizontal::<16>(s, pitch, blimit, limit, thresh) }
}

/// Filters 8 rows along a vertical edge with up to the 16-sample filter.
///
/// # Safety
///
/// The 8 samples before `s` and the 8 from it on must be readable and writable
/// in each of 8 rows.
pub unsafe fn lpf_vertical_16(s: *mut u8, pitch: isize, blimit: u8, limit: u8, thresh: u8) {
    unsafe { vertical::<16>(s, pitch, blimit, limit, thresh) }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Lpf = unsafe fn(*mut u8, isize, u8, u8, u8);

    /// The six functions: the horizontal ones by width, then the vertical.
    const ALL: [Lpf; 6] = [lpf_horizontal_4, lpf_horizontal_8, lpf_horizontal_16, lpf_vertical_4, lpf_vertical_8, lpf_vertical_16];

    fn lcg(state: &mut u64) -> u8 {
        *state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (*state >> 56) as u8
    }

    /// Every function over noise, flat, ramped and partly flat blocks, hashed:
    /// the value libvpx's C gives for the same input.
    #[test]
    fn matches_libvpx_known_answer() {
        let (mut state, mut hash) = (1, 0xcbf29ce484222325u64);
        for round in 0..60 {
            let mut buf = [0u8; 32 * 32];
            for (i, v) in buf.iter_mut().enumerate() {
                let (x, y, r) = (i % 32, i / 32, lcg(&mut state));
                *v = match round % 5 {
                    0 => r,
                    1 => 100 + (r & 1),
                    2 => 60 + (x + y) as u8 + (r & 1),
                    3 => 128 + r % 9,
                    _ if (12..20).contains(&y) || (4..12).contains(&x) => 100 + (r & 1),
                    _ => 100 + (r & 7),
                };
            }
            let t = [255, 63, 15].map(|most| lcg(&mut state) & most);
            for lpf in ALL {
                let mut out = buf;
                unsafe { lpf(out[16 * 32 + 8..].as_mut_ptr(), 32, t[0], t[1], t[2]) };
                for b in out {
                    hash = (hash ^ b as u64).wrapping_mul(0x100000001b3);
                }
            }
        }
        assert_eq!(hash, 0x0828709cc2aa353f);
    }

    /// Flatness is judged on each side by itself, so a step between two flat
    /// sides is spread over the widest filter the edge has, unless the edge
    /// limit holds it back.
    #[test]
    fn a_step_between_flat_sides() {
        let step = || {
            let mut buf = [100u8; 16 * 8];
            buf.chunks_exact_mut(16).for_each(|row| row[8..].fill(200));
            buf
        };
        let mut buf = step();
        unsafe { lpf_vertical_16(buf[8..].as_mut_ptr(), 16, 249, 63, 0) };
        assert_eq!(buf, step());

        unsafe { lpf_vertical_8(buf[8..].as_mut_ptr(), 16, 250, 0, 0) };
        let seven_tap = [100, 100, 100, 100, 100, 113, 125, 138, 163, 175, 188, 200, 200, 200, 200, 200];
        assert!(buf.chunks_exact(16).all(|row| row == seven_tap));

        let mut buf = step();
        unsafe { lpf_vertical_16(buf[8..].as_mut_ptr(), 16, 250, 0, 0) };
        let fifteen_tap = [100, 106, 113, 119, 125, 131, 138, 144, 156, 163, 169, 175, 181, 188, 194, 200];
        assert!(buf.chunks_exact(16).all(|row| row == fifteen_tap));
    }
}
