//! Decode an IVF file, one frame at a time, and print per frame the MD5
//! `ffmpeg -f framemd5` prints for its planar output, so a run can be checked
//! against libvpx; the timing goes to stderr.
//!
//!   vp9-bench FILE [THREADS] [REPEATS]
//!
//! With VP9_STATS set, each frame also gets a line on stderr of what its blocks
//! were coded as: the share of its 8×8s that are the last frame's samples in
//! place (`still`), in libvpx's inactive segment, intra, and not skipped (their
//! residual is coded, though each transform block of it may hold no
//! coefficient), and how many of its 64×64 blocks hold anything but still ones.

mod md5;

use std::time::Instant;

/// The frames of an IVF file: a 32-byte header, then each frame after a
/// 12-byte header of its length and time.
fn ivf_frames(data: &[u8]) -> Vec<&[u8]> {
    let mut frames = Vec::new();
    let mut rest = data.get(32..).unwrap_or_default();
    while rest.len() >= 12 {
        let len = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
        let Some(frame) = rest.get(12..12 + len) else { break };
        frames.push(frame);
        rest = &rest[12 + len..];
    }
    frames
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: vp9-bench FILE [THREADS] [REPEATS]");
    let threads: usize = args.next().map_or(1, |s| s.parse().expect("THREADS"));
    let repeats: usize = args.next().map_or(1, |s| s.parse().expect("REPEATS"));
    let data = std::fs::read(&path).expect("read input");
    let stats = std::env::var_os("VP9_STATS").is_some();
    let units = ivf_frames(&data);
    if units.is_empty() {
        eprintln!("{path}: no frames");
        std::process::exit(1);
    }
    if threads > 1 {
        rayon::ThreadPoolBuilder::new().num_threads(threads).build_global().expect("pool");
    }
    for r in 0..repeats {
        let mut dec = vp9::Decoder::new(threads);
        let mut n = 0;
        let mut times = Vec::with_capacity(units.len());
        for unit in &units {
            let t0 = Instant::now();
            let decoded = dec.decode(unit);
            times.push(t0.elapsed().as_secs_f64() * 1000.0);
            match decoded {
                Ok(Some(d)) => {
                    n += 1;
                    if r == 0 {
                        let f = &d.frame;
                        let mut m = md5::Md5::new();
                        let mut raw = Vec::new();
                        for plane in &f.planes {
                            for row in 0..f.height {
                                let line = &plane.data[row * plane.stride..row * plane.stride + f.width];
                                m.update(line);
                                raw.extend_from_slice(line);
                            }
                        }
                        println!("{}", m.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>());
                        if stats {
                            let (mi, cols) = f.blocks();
                            let rows = mi.len() / cols;
                            let (mut still, mut inactive, mut intra, mut unskipped) = (0, 0, 0, 0);
                            for m in mi {
                                still += m.still() as usize;
                                inactive += (m.segment_id == 7) as usize;
                                intra += !m.is_inter() as usize;
                                unskipped += !m.skip as usize;
                            }
                            let (sb_cols, sb_rows) = (cols.div_ceil(8), rows.div_ceil(8));
                            let touched = (0..sb_rows * sb_cols)
                                .filter(|&s| {
                                    let (r0, c0) = (s / sb_cols * 8, s % sb_cols * 8);
                                    (r0..(r0 + 8).min(rows)).any(|r| (c0..(c0 + 8).min(cols)).any(|c| !mi[r * cols + c].still()))
                                })
                                .count();
                            let pct = |n: usize| 100.0 * n as f64 / mi.len() as f64;
                            eprintln!(
                                "frame {}: {} bytes{}, still {:.1}%, inactive {:.1}%, intra {:.1}%, not skipped {:.1}%, 64x64 blocks touched {touched}/{}",
                                times.len() - 1,
                                unit.len(),
                                if d.keyframe { ", keyframe" } else { "" },
                                pct(still),
                                pct(inactive),
                                pct(intra),
                                pct(unskipped),
                                sb_rows * sb_cols
                            );
                        }
                        // VP9_DUMP=path:index writes frame `index` as raw planar 4:4:4.
                        if let Some((path, idx)) = std::env::var("VP9_DUMP").ok().and_then(|v| v.split_once(':').map(|(p, i)| (p.to_string(), i.parse::<usize>().unwrap_or(0))))
                            && idx + 1 == n
                        {
                            std::fs::write(path, &raw).expect("dump");
                        }
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("frame {}: {e}", times.len() - 1);
                    std::process::exit(1);
                }
            }
        }
        // The decode calls alone: the digests are not timed.
        let total: f64 = times.iter().sum();
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
        eprintln!("{n} frames, {total:.1} ms decoding: {:.2} ms/frame, median {:.2}, p95 {:.2}, max {:.2}", total / n.max(1) as f64, p(0.5), p(0.95), p(1.0));
    }
}
