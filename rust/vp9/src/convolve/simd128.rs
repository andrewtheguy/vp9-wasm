//! The filters of `convolve` in WebAssembly vectors, sample for sample what
//! the scalar ones give.
//!
//! A pass sums in 16-bit lanes that wrap. With a kernel's taps summing to 128
//! and its negative ones to no less than −64, the sum over 8-bit samples lies
//! in −16320..=48960, wider than a signed lane, but with the rounding 64 added
//! and 128·128 taken off it lies in −32640..=32640: started from [`BIAS`],
//! the lanes end exact however they wrapped on the way. Shifted down 7 bits
//! that is the rounded sample less 128, which is added back before the
//! saturating narrow clips to 8 bits.

use core::arch::wasm32::*;
use core::mem::MaybeUninit;

use super::{BEFORE, EXTRA};

/// The rounding, less the 128·128 that keeps a sum inside a signed lane.
const BIAS: i16 = 64 - 128 * 128;

/// Eight samples at `p` as lanes; the upper four are 0 for a `W` of 4, whose
/// four bytes are all that is read.
#[inline(always)]
unsafe fn lanes<const W: usize>(p: *const u8) -> v128 {
    u16x8_extend_low_u8x16(unsafe {
        if W == 4 { v128_load32_zero(p as *const u32) } else { v128_load64_zero(p as *const u64) }
    })
}

/// Eight samples filtered, not yet clipped: `lanes(k)` gives what tap `k`
/// multiplies.
#[inline(always)]
fn sums(lanes: impl Fn(usize) -> v128, taps: &[v128; 8]) -> v128 {
    let mut sum = i16x8_splat(BIAS);
    for k in 0..8 {
        sum = i16x8_add(sum, i16x8_mul(lanes(k), taps[k]));
    }
    i16x8_add(i16x8_shr(sum, 7), i16x8_splat(128))
}

/// Writes eight sums at `p` as samples, clipped; four for a `W` of 4.
#[inline(always)]
unsafe fn store<const W: usize>(p: *mut u8, sums: v128) {
    let samples = u8x16_narrow_i16x8(sums, sums);
    unsafe {
        if W == 4 {
            v128_store32_lane::<0>(samples, p as *mut u32)
        } else {
            v128_store64_lane::<0>(samples, p as *mut u64)
        }
    }
}

/// `h` rows of `W` samples, each from the eight a `step` apart starting at its
/// place in `src`. No load reaches past the last tap's `W` samples.
#[inline(always)]
unsafe fn filter<const W: usize>(
    src: *const u8,
    src_stride: usize,
    dst: *mut u8,
    dst_stride: usize,
    f: &[i16; 8],
    step: usize,
    h: usize,
) {
    let taps = f.map(|tap| i16x8_splat(tap));
    for y in 0..h {
        for x in (0..W).step_by(8) {
            unsafe {
                let from = src.add(y * src_stride + x);
                store::<W>(dst.add(y * dst_stride + x), sums(|k| lanes::<W>(from.add(k * step)), &taps));
            }
        }
    }
}

/// `vpx_convolve8_horiz`.
pub unsafe fn horiz<const W: usize>(
    src: *const u8,
    src_stride: usize,
    dst: *mut u8,
    dst_stride: usize,
    f: &[i16; 8],
    h: usize,
) {
    unsafe { filter::<W>(src.sub(BEFORE), src_stride, dst, dst_stride, f, 1, h) }
}

/// `vpx_convolve8_vert`.
pub unsafe fn vert<const W: usize>(
    src: *const u8,
    src_stride: usize,
    dst: *mut u8,
    dst_stride: usize,
    f: &[i16; 8],
    h: usize,
) {
    unsafe { filter::<W>(src.sub(BEFORE * src_stride), src_stride, dst, dst_stride, f, src_stride, h) }
}

/// `vpx_convolve8`: across into an intermediate of 8-bit samples, then down
/// from it. `h` is at most 64.
pub unsafe fn both<const W: usize>(
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
