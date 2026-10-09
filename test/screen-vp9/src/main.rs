//! A 4:4:4 stream of a desktop-like picture, coded as the remotex gateway
//! drives screen-vp9: a keyframe, then frames told where the picture changed,
//! a whole frame, a keyframe forced in the middle, and the dial moved. libvpx
//! codes a frame told where the picture changed through its active map, which
//! is segmentation in the bitstream: a segment that skips and is not loop
//! filtered over the blocks outside the change, a map coded whole or as the
//! last frame's, and segmentation left on with no feature in force by the
//! whole frame after.
//!
//!   screen-vp9-fixtures OUT.ivf WIDTHxHEIGHT THREADS
//!
//! The threads decide the tile columns, as they do in the gateway.

use screen_vp9::{Chroma, Rect, Stream};

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
}

const PATCH: (usize, usize) = (56, 40);

/// A desktop: a flat background, a band of text-like noise every third row
/// of 24, and a coloured patch at `at`.
fn paint(rgb: &mut [u8], w: usize, h: usize, at: (usize, usize)) {
    let mut lcg = Lcg(7);
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 3;
            let band = (y / 24) % 3 == 1;
            let (r, g, b) = if band && lcg.next() % 5 == 0 {
                (lcg.next() as u8, 200, 30)
            } else if band {
                (240, 240, 235)
            } else {
                (30 + (x / 32) as u8 * 4, 40, 60 + (y / 32) as u8 * 3)
            };
            rgb[i..i + 3].copy_from_slice(&[r, g, b]);
        }
    }
    for y in at.1..(at.1 + PATCH.1).min(h) {
        for x in at.0..(at.0 + PATCH.0).min(w) {
            let i = (y * w + x) * 3;
            rgb[i..i + 3].copy_from_slice(&[220, (x + y) as u8, 40]);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, out, size, threads] = args.as_slice() else {
        eprintln!("usage: screen-vp9-fixtures OUT.ivf WIDTHxHEIGHT THREADS");
        std::process::exit(2);
    };
    let (w, h) = size.split_once('x').expect("WIDTHxHEIGHT");
    let (w, h): (u16, u16) = (w.parse().expect("width"), h.parse().expect("height"));
    let threads: usize = threads.parse().expect("threads");
    let (wu, hu) = (usize::from(w), usize::from(h));

    let mut stream = Stream::new(w, h, Chroma::Full, 60, threads).expect("a stream");
    let patch = |i: usize| ((20 + 9 * i) % (wu - PATCH.0), (16 + 7 * i) % (hu - PATCH.1));
    let rect = |p: (usize, usize)| Rect { x: p.0 as u16, y: p.1 as u16, width: PATCH.0 as u16, height: PATCH.1 as u16 };
    let moved = |from: usize, to: usize| Some(vec![rect(patch(from)), rect(patch(to))]);
    // Each frame: where the patch is, whether a keyframe is forced, and where
    // the picture changed, `None` being a whole frame; and the dial before it.
    let mut frames: Vec<(usize, bool, Option<Vec<Rect>>, u8)> = vec![(0, false, Some(vec![]), 60)];
    for i in 1..4 {
        frames.push((i, false, moved(i - 1, i), 60));
    }
    // The same map again, which libvpx may code as the last frame's.
    frames.push((3, false, moved(3, 3), 60));
    frames.push((4, false, None, 60));
    frames.push((5, false, moved(4, 5), 60));
    frames.push((5, true, Some(vec![]), 60));
    for i in 6..9 {
        frames.push((i, false, moved(i - 1, i), 60));
    }
    frames.push((8, false, None, 40));
    frames.push((9, false, moved(8, 9), 40));

    let mut file = Vec::new();
    file.extend_from_slice(b"DKIF");
    file.extend_from_slice(&0u16.to_le_bytes());
    file.extend_from_slice(&32u16.to_le_bytes());
    file.extend_from_slice(b"VP90");
    file.extend_from_slice(&w.to_le_bytes());
    file.extend_from_slice(&h.to_le_bytes());
    file.extend_from_slice(&30u32.to_le_bytes());
    file.extend_from_slice(&1u32.to_le_bytes());
    file.extend_from_slice(&(frames.len() as u32).to_le_bytes());
    file.extend_from_slice(&0u32.to_le_bytes());
    let mut rgb = vec![0u8; wu * hu * 3];
    for (n, (at, keyframe, changed, quality)) in frames.iter().enumerate() {
        if stream.quality() != *quality {
            stream.set_quality(*quality).expect("the dial moved");
        }
        paint(&mut rgb, wu, hu, patch(*at));
        let mut frame = Vec::new();
        stream.encode_rgb(&rgb, changed.as_deref(), *keyframe, &mut frame).expect("a frame").expect("a frame produced");
        file.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        file.extend_from_slice(&(n as u64).to_le_bytes());
        file.extend_from_slice(&frame);
    }
    std::fs::write(out, file).expect("write the stream");
}
