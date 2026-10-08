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
`test/fixtures.ts`; commit what it writes.

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
compound prediction, segmentation, a frame shown again. Its arithmetic and its
tables were transcribed from libvpx 1.16.0 (BSD-3-Clause, `rust/vp9/LICENSE`
and `PATENTS`).

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
  are SIMD128 (`core::arch::wasm32`): the loop filter's edges and the
  interpolation filters.
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

## Requirements

The page must be cross-origin isolated for `SharedArrayBuffer`, the threads'
shared memory: remotex's gateway serves every file with
`Cross-Origin-Opener-Policy: same-origin` and
`Cross-Origin-Embedder-Policy: require-corp`. The browser must run WebAssembly
with SIMD128 and threads.
