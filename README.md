# vp9-wasm (experimental)

A software decoder for VP9 at 4:4:4, for
[remotex](https://github.com/andrewtheguy/remotex)'s browser client where the
browser's `VideoDecoder` does not take VP9 profile 1: Safari on iOS and
iPadOS, for one, which decodes VP9 in hardware alone and so at 4:2:0 alone. It
is written in Rust for the one shape of stream remotex and
[wlshare](https://github.com/andrewtheguy/wlshare) send and nothing else of
VP9, compiled to WebAssembly with SIMD128 and threads, and decodes bit for bit
as libvpx does.

A release is `vp9-wasm-vX.Y.Z.tar.gz`, attached to this repository's release
`vX.Y.Z` with its `SHA256SUMS`, holding the module as wasm-pack names it:
`vp9.js`, wasm-bindgen's ES module glue, `vp9_bg.wasm`, and `vp9.d.ts`, the
glue's types. remotex pins one by version and SHA-256, and its frontend build
downloads that archive and bundles the module with the page.

## Building

```sh
bun install
./build.sh
```

wasm-pack comes from `bun install`, and the compiler is the nightly
`rust/vp9-web/rust-toolchain.toml` names, which rustup installs on the first
build. A nightly because the module's threads share a memory, and the standard
library as shipped for `wasm32-unknown-unknown` is built without `atomics`: it
links, but its locks are single-thread stubs, and only a nightly Cargo builds
it with them (`build-std`, in `rust/vp9-web/.cargo/config.toml`). The pin is
the channel, not a date: the decoder itself builds on stable. The script writes
the three files to `build/out`, and the release archive to `dist/`.

The archive is built with fixed names, owners and times, so on the same
toolchain one build's archive is byte for byte the next's.

## Testing

```sh
cargo build --release --manifest-path rust/Cargo.toml -p vp9-bench
rust/target/release/vp9-bench test/data/screen-330x194.ivf 4
bun test
bun run typecheck
```

`vp9-bench FILE [THREADS] [REPEATS]` decodes an IVF file natively and prints
each frame's MD5 as `ffmpeg -f framemd5` does, with the time per frame on
stderr. Natively the decoder runs its scalar loops, since the SIMD ones are
written for wasm32.

`bun test` loads `build/out` (or `$VP9_WASM_DIR`) under Bun as the page loads
it: the module on a shared memory, its pool's threads as workers that each run
an instance of it. Every 4:4:4 stream in `test/data`, libvpx's output under
settings near remotex's, must decode to libvpx's `-f framemd5`, recorded in
`test/data/reference.json`, on one thread, on two and on four; the 4:2:0 one
must be refused by name. The tests also cover where the planes are in the
memory, the colour a stream states, joining a stream before its keyframe, a
superframe, two decoders side by side, and garbage.

A run needs no ffmpeg. `bun run fixtures` regenerates the streams and their
reference with the host's ffmpeg and libvpx from the specs in
`test/fixtures.ts`; commit what it writes. The two `active-map` streams are
written by `test/screen-vp9`, a program over the `screen-vp9` crate that codes
a picture as the remotex gateway does, told where it changed: ffmpeg has no
way to ask libvpx for its active map. `bun run fixtures NAME...` writes the
streams named and keeps the rest, for a host whose ffmpeg is not the one they
were coded by; the reference decode is the same from any.

The decoder must return an error on any input and never trap, since a trap
takes the module down for every decoder in it. `rust/vp9/fuzz` holds a
[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) target that feeds a
stream frame by frame to two decoders, one on the calling thread and one on a
pool of three, and requires the same picture or the same failure of both. It
is built natively, with the sanitizer and the debug assertions on, so that it
catches a bound the shared buffers' accessors take on trust and a race between
the stages as well as a panic. The SIMD loops are the module's alone, so
`bun test` also feeds damaged copies of the fixtures to the module, on one
thread and on four, and requires that nothing traps and that the two agree.

```sh
cd rust/vp9
mkdir -p fuzz/corpus/decode && cp ../../test/data/*.ivf fuzz/corpus/decode/
cargo +nightly fuzz run decode -- -max_len=65536 -jobs=3 -workers=3
```

An input that fails lands in `fuzz/artifacts/decode/`; `cargo +nightly fuzz
run decode fuzz/artifacts/decode/<file>` replays it.

## Releasing

Bump the version in `rust/vp9-web/Cargo.toml` (the module's crate: the archive
and the tag take their number from it), commit, push, and run the
`Release vp9-wasm` workflow (`.github/workflows/release.yml`) on that branch.
It tests the decoder, builds the module, tests it, and creates the release
`vX.Y.Z` at that commit with the archive and its `SHA256SUMS`; from a branch
other than `main` it is a prerelease. remotex then takes it as a new version
and checksum.

## The decoder

`rust/` holds the decoder, written for the VP9 that libvpx emits as the
[`screen-vp9`](https://github.com/andrewtheguy/screen-vp9) crate configures
it: profile 1, 4:4:4 at 8 bits, every frame the size of the frames it refers
to, each block predicted from one reference. Any other stream is refused by
name: 4:2:0 or 4:2:2 chroma, more bits a sample, a reference of another size,
compound prediction, a frame shown again. Segmentation is decoded, all four
features: it is how libvpx codes a frame told where the picture changed,
which `screen-vp9` asks of it for every frame the gateway or wlshare knows
the damage of. The blocks outside the change are a segment that skips and is
not loop filtered, each the reference's samples at its own place, so such a
frame costs the decoder its changed blocks and the parse. Its arithmetic and
its tables were transcribed from libvpx 1.16.0 (BSD-3-Clause,
`rust/vp9/LICENSE` and `PATENTS`).

- `rust/vp9` is the decoder: one frame in, its picture out as three planes of
  bytes, with the size to show and the colour the stream states. A frame goes
  through three stages, each as far behind the one before as what it reads
  requires. A tile is parsed, its blocks' modes into the frame's grid and its
  coefficients into a buffer per row of 64×64 blocks; tiles are columns, and
  parse side by side. A row of blocks is reconstructed once it is parsed, a
  block behind the row above it, whose samples its intra blocks predict from.
  A block is loop-filtered once the blocks to its right and below it are
  reconstructed, since they predict from its samples as they were, and once
  the block above and to its right is filtered. With threads the stages run
  on rayon's pool, which is what a frame of one tile, a desktop under 1920
  wide, decodes in parallel by; on one thread a row is parsed, reconstructed,
  and the row above it filtered, in turn. The sample loops that carry the time
  are SIMD128 (`core::arch::wasm32`): the loop filter's edges, the
  interpolation filters, and the inverse transforms these streams are made
  of, the 4×4 ones and the 8×8 DCT, a row to a lane and each rotation a dot
  product, exact to the plain code for every input. Parsing, the one stage
  that a tile's bytes keep in order, is what a frame waits on with threads,
  and is written for V8: a block's coefficients are read with the boolean
  decoder's two registers in locals, in a function of their own for each
  transform size, so that the widths that hang on the size are constants of
  the code; the bits read ahead carry a marker of their end rather than a
  count, so the test for a refill is the word's low half being zero, and
  every refill is one whole word, since the partition's last bytes are kept
  again with zeros after them; a token of five or more, with its category and
  extra bits, is read in a function apart, so the loop over the rest stays
  small enough that V8 keeps the registers in registers; a transform block
  with no coefficients, which most are, costs one boolean and leaves nothing
  behind; the parser leaves each block's coded transform blocks alone, each
  with its place, so an inter block's residual walks those and not every
  transform block; a 4×4's coefficients are left whole, in place, as the
  transform loads them, and a larger block's as their places and values,
  which reconstruction puts into a block of zeros and takes out again; and
  symbols are counted
  only in a frame whose probabilities adapt to them, which no frame of
  remotex's or wlshare's does.
- `rust/vp9-web` is the page's module, in the shape of hevc-wasm's:
  wasm-bindgen, a pool whose threads are seats the page's workers take
  (`runPoolThread`, `startPool`), and a `Decoder` with `input`, `decode` and
  `picture`. `picture` describes the picture in the sixteen numbers hevc-wasm's
  does: its size, the chroma layout, the colour the stream states, each
  plane's address and stride in the module's memory, and whether it is a
  keyframe, so the paint worker uploads the planes to WebGL from the module's
  shared memory.
- `rust/vp9-bench` is the native harness above. To profile the module itself,
  run it under `node --perf-prof` and `perf record`: the build keeps the
  function names. Profiles and logs go under `tmp/`.

The comparison is on what the remotex gateway itself wrote on 2026-10-08,
one desktop of four panes, an animation, a page scrolled, a terminal
printing and a page of text, at eight sizes: a Windows desktop over RDP at
five and a virtual Mac at three, at quality 90 with the loop filter on
every frame, each capture whole. It ran on a six-core x86 workstation, the
builds alternately on the same cores, one for one thread and four for four,
once the host was quiet, each once. The cycle counts are `perf stat`'s for
the whole process and do not depend on the load.

The module against its own earlier release, both under Bun, per frame,
lower better:

| Capture | Threads | M cycles, 0.0.3 | M cycles, 0.0.5 | Mean ms, 0.0.3 | Mean ms, 0.0.5 |
|---|---|---|---|---|---|
| Windows, 1280×800, 1898 frames | 1 | 69.0 | 66.0 | 21.4 | 20.5 |
| | 4 | 72.4 | 68.9 | 16.0 | 15.7 |
| Windows, 1440×900, 1797 frames | 1 | 85.1 | 81.2 | 26.4 | 25.1 |
| | 4 | 89.4 | 84.6 | 12.3 | 12.0 |
| Windows, 1600×1000, 2333 frames | 1 | 104.8 | 100.0 | 32.4 | 31.1 |
| | 4 | 108.5 | 103.3 | 13.7 | 13.4 |
| Windows, 1920×1080, 1943 frames | 1 | 101.4 | 96.2 | 31.9 | 29.8 |
| | 4 | 105.3 | 100.8 | 13.3 | 13.7 |
| a Mac, 2560×1600, 1986 frames | 1 | 154.3 | 147.6 | 48.7 | 46.5 |
| | 4 | 160.6 | 152.5 | 19.6 | 18.9 |
| a Mac, 2880×1800, 2245 frames | 1 | 200.1 | 190.4 | 61.9 | 59.6 |
| | 4 | 213.1 | 205.7 | 23.5 | 22.9 |
| Windows, 3456×2168, 887 frames | 1 | 272.2 | 256.2 | 98.3 | 87.6 |
| | 4 | 280.0 | 266.9 | 30.6 | 30.8 |
| a Mac, 3840×2160, 1443 frames | 1 | 234.5 | 220.3 | 73.1 | 68.6 |
| | 4 | 249.4 | 234.2 | 28.4 | 27.8 |

The module against libvpx, per frame, lower better: release 0.0.5 in the
same runs and ffmpeg 7.1's
`libvpx-vp9` decoder (libvpx 1.15.0) alternated with it; the cycles are each
whole process's, the module's ms its own clock's mean over the frames and
libvpx's its process's wall time over them. The command is
`ffmpeg -threads N -c:v libvpx-vp9 -i FILE -benchmark -f null -`, and the
build Debian 13's: ffmpeg 7.1.5-0+deb13u1, by gcc 14 with
`--toolchain=hardened --enable-shared --enable-libvpx`, over its libvpx9
1.15.0-2.1+deb13u1, configured `--target=x86_64-linux-gcc --enable-pic
--enable-shared --enable-vp9-highbitdepth --enable-postproc
--enable-vp9-postproc`, which picks its vector code as it runs, AVX2 on this
i5-8500T, and whose high bit depth build widens the coefficients of an 8-bit
stream to 32 bits:

| Capture | Threads | M cycles, libvpx | M cycles, module | Mean ms, libvpx | Mean ms, module |
|---|---|---|---|---|---|
| Windows, 1280×800 | 1 | 59.7 | 66.0 | 18.6 | 20.5 |
| | 4 | 60.3 | 68.9 | 17.8 | 15.7 |
| Windows, 1440×900 | 1 | 74.0 | 81.2 | 23.3 | 25.1 |
| | 4 | 75.4 | 84.6 | 14.7 | 12.0 |
| Windows, 1600×1000 | 1 | 89.8 | 100.0 | 28.2 | 31.1 |
| | 4 | 91.2 | 103.3 | 16.7 | 13.4 |
| Windows, 1920×1080 | 1 | 86.4 | 96.2 | 26.9 | 29.8 |
| | 4 | 87.9 | 100.8 | 16.2 | 13.7 |
| a Mac, 2560×1600 | 1 | 130.4 | 147.6 | 40.6 | 46.5 |
| | 4 | 132.5 | 152.5 | 25.0 | 18.9 |
| a Mac, 2880×1800 | 1 | 164.2 | 190.4 | 52.4 | 59.6 |
| | 4 | 173.1 | 205.7 | 21.7 | 22.9 |
| Windows, 3456×2168 | 1 | 228.7 | 256.2 | 77.5 | 87.6 |
| | 4 | 236.3 | 266.9 | 29.5 | 30.8 |
| a Mac, 3840×2160 | 1 | 198.9 | 220.3 | 62.5 | 68.6 |
| | 4 | 209.2 | 234.2 | 26.7 | 27.8 |

On one thread libvpx, in AVX2, takes 9% to 14% fewer cycles a frame than
the module in 128-bit vectors. With four threads libvpx decodes a tile
column on each and filters on a worker of its own, so a stream of one
column or two leaves it little to split: up to 2560 wide the module, which
parses a column on a thread and reconstructs and filters by rows on the
others, takes 12% to 24% less time for more cycles. From 2880 wide the
stream is in four columns, libvpx's four threads each have one, and its
frame takes 4% to 6% less time than the module's.

On the captures measured before these, a wlshare desktop and animation and
a Mac's, a frame is some hundred thousand tokens in tens of thousands
of transform blocks, most of them 4×4 and six to nine in ten with no
coefficient at all, so what a transform block costs before its first token
counts for as much as the tokens do. On one thread the loop filter is a
quarter to a third of the time, parsing a quarter to two fifths, the inverse
transforms a thirtieth, and the copy each row of 64×64 blocks of an inter
frame starts as, the last frame's rows, 3% to 7%. A still block, a zero
motion vector from the last frame and no residual, which is most of a
screen's area, is then no reconstruction at all, where its own whole-sample
copy had been a tenth of the time. With threads a frame of one tile column
waits on its parsing, which is why the four-thread times move little.
Measured and not done: a row copying only its still blocks, and a pooled
buffer keeping track of which blocks are the last frame's still so that a
row copies less. The whole row is one sequential pass that also brings the
reference's lines into cache for the blocks that move, and telling a block
unchanged costs about what copying it does. Before 0.0.3, filtering two
edges that lie end to end in one vector and the edge kernels as functions of
their own were measured and are not done either: those are in
[docs/tried.md](docs/tried.md). Where a frame's time goes, what threads
give and what the loop filter and the tile columns cost a stream are in
[docs/measurements.md](docs/measurements.md), and what the decoder does not
do yet in [docs/remaining.md](docs/remaining.md).

The routine measurement of a change is `bench/run.sh [BUILD...]`, some
minutes: each BUILD is a directory holding a build of the module, `build/out`
when none is named, or the word `libvpx` for ffmpeg's decoder. It decodes
five samples of 120 frames, cut from those captures as `screen-vp9` 0.0.10
codes each size, one from a quiet stretch and four from the busiest, of one
tile column, two and four. `bench/samples.sh` copies them once from the
`bench` folder of the artifacts drive to `tmp/bench`, and no run reads the
drive. That folder has a quiet and a busy sample of every size captured, and
`SAMPLES` names others to decode instead. The script does what a busy host needs: it pins the builds to the same
cores (`taskset`), alternates them round by round, waits for the load to fall
before each stream, and prints `perf stat -e instructions:u,cycles:u` beside
the times, with the medians of three rounds.

## Requirements

The page must be cross-origin isolated for `SharedArrayBuffer`, the threads'
shared memory: remotex's gateway serves every file with
`Cross-Origin-Opener-Policy: same-origin` and
`Cross-Origin-Embedder-Policy: require-corp`. The browser must run WebAssembly
with SIMD128 and threads.
