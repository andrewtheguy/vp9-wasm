//! A stream as the decoder gets one, frame by frame, to two decoders: one on
//! the calling thread and one on the pool. The input is an IVF file's frames,
//! or one frame where it is not an IVF file. Every outcome is allowed but a
//! panic, and a frame the two do not decode alike.

#![no_main]

use std::sync::Once;

use libfuzzer_sys::fuzz_target;

/// The pool's threads: enough for the stages to wait on one another.
const THREADS: usize = 3;

fn pool() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| rayon::ThreadPoolBuilder::new().num_threads(THREADS).build_global().expect("the pool"));
}

/// The frames of an IVF file: a 32-byte header, then each frame after its
/// length and its time.
fn frames(data: &[u8]) -> Vec<&[u8]> {
    let mut frames = Vec::new();
    if !data.starts_with(b"DKIF") {
        return vec![data];
    }
    let mut rest = data.get(32..).unwrap_or_default();
    while rest.len() >= 12 {
        let len = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
        let frame = rest.get(12..12 + len).unwrap_or(&rest[12..]);
        frames.push(frame);
        rest = &rest[12 + frame.len()..];
    }
    frames
}

/// What a frame decoded to, as far as the two decoders must agree: an error,
/// no picture, or the picture's samples.
fn outcome(dec: &mut vp9::Decoder, frame: &[u8]) -> Option<Option<Vec<u8>>> {
    match dec.decode(frame) {
        Err(_) => None,
        Ok(None) => Some(None),
        Ok(Some(d)) => {
            let f = &d.frame;
            let mut v = Vec::new();
            for p in &f.planes {
                for y in 0..f.height {
                    v.extend_from_slice(&p.data[y * p.stride..y * p.stride + f.width]);
                }
            }
            Some(Some(v))
        }
    }
}

fuzz_target!(|data: &[u8]| {
    pool();
    let mut one = vp9::Decoder::new(1);
    let mut many = vp9::Decoder::new(THREADS);
    for frame in frames(data) {
        assert!(outcome(&mut one, frame) == outcome(&mut many, frame), "one thread and {THREADS} decoded a frame differently");
    }
});
