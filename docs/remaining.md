# What remains

What the decoder does not do yet, and what has not been measured. What it
does and how it is built are in the [README](../README.md), the numbers
behind this list in [measurements.md](measurements.md), and what was built
and found no faster in [tried.md](tried.md).

## Speed

Judged on the terminal and the video of wlshare's captures and on its other
scenarios, as `screen-vp9` 0.0.18 codes them
([measurements.md](measurements.md)); the flood of text is the worst case a
desktop can be, measured for reference and no target. On one thread the
module takes a third to a half more cycles a frame than libvpx on the video
and the walk, a quarter more on the browser, and is level only on the flood,
which is token parsing. On four threads it is ahead of libvpx or level
everywhere but the browser at 4K, 13.6 ms a frame for 10.8, and uncover,
whose mean is its first frames. In the order the work would go:

- **Sub-sample prediction.** 28% of a 4K video frame and 30% of the walk's,
  16% at 1920×1080: `convolve::simd128::both` at 16 wide is the largest
  function of both 4K samples, 17%, and `convolve::predict`, the
  whole-sample copies and the dispatch, 6% to 8%, 14% on the browser. Not
  yet known: how much of the module's half more instructions than libvpx is
  here, and how much of that is libvpx's AVX2, which 128-bit vectors cannot
  follow.
- **The loop filter, under 2048 wide.** 24% of a 1920×1080 video frame, its
  largest function. What was tried on the kernels
  ([tried.md](tried.md)) was measured on the earlier captures and not on a
  video; libvpx's `ss00` path is untried. The masks settled as the blocks
  are parsed, as hevc-wasm's boundary strengths are, remain the smaller
  part.
- **`ModeInfo` smaller, and the block's walk.** `Tile::block` is 31% of a
  quiet 4K frame, 12% of the browser's and 5% of the video's, and
  `Recon::block` 13% of the browser's, 4% to 9% of the video's and the
  walk's. The parse of a 64×64 block in the inactive segment is mostly the
  34-byte `ModeInfo` written to its 64 cells: 2 KB a block, 4 MB a 4K
  frame. Only a block under 8×8 has four motion vectors and four modes, and
  those could live in a table of their own by cell, `mv` is always
  `sub_mv[3]` and `mode` always `sub_mode[3]`, so the record could be 16
  bytes.
- **The motion vectors' candidates and the mode trees** on a page scrolled:
  `find_mv_refs` and `BoolDecoder::tree` are 6% each of the browser's 4K
  frame, 1% to 4% elsewhere.
- **Token parsing** is 29% to 30% of a video frame and 22% of the walk's,
  where it is two thirds of the flood's. Both decoders sit on the boolean's
  chain there, which no layout of the loop shortens, so this is where the
  module is level already. The token's probability by a table of offsets
  was no faster on an M2 ([tried.md](tried.md)).
- **The damage to the painter.** What changed reaches the decoder, as the
  active map, and stops there: the page uploads all three planes of every
  picture to WebGL, 25 MB at 4K for a frame of a kilobyte. The decoder now
  knows which 64×64 blocks a frame wrote (`Frame::kept`); `picture()` could
  say so, as a rectangle or as the rows of blocks, and remotex's paint worker
  upload only that with `texSubImage2D`, or nothing when nothing changed.
  Likely worth more on an iPad than the decode of such a frame now costs.
- **The transforms**, `inverse_add` and what it calls, are 4% to 7% of a
  video frame and 1% of the browser's. The ADST at 8 and 16 and the DCT at
  16 and 32 are in plain code; an exact port needs 32-bit lanes for the
  ADST, whose stages libvpx's C keeps at 32 bits.
- **Intra prediction in plain code**, and the scatter of the coefficients
  of a transform block larger than 4×4. Not counted on these samples.

## Not measured

- **Apple silicon, on an iPhone or iPad.** On an M2 under Bun, whose
  JavaScriptCore is Safari's, a busy frame is three quarters token parsing,
  waiting on the boolean's chain, and the module takes 1% to 4% fewer cycles
  than libvpx's NEON build on one thread (`bench/run.sh`, and what was tried
  there in [tried.md](tried.md)). The memory is fast memory, a load being one
  instruction with no bounds check; the shuffles are each one NEON
  instruction, `zip`, `uzp`, `dup` or `ext` for seven in ten and `tbl` for the
  rest; nothing uses `bitmask`. Not measured: iOS, whose Safari may give a
  shared memory bounds checks, and its two performance cores beside four
  efficiency ones, onto which the thread that parses may be put.
- **The video on Apple silicon, and the threads on it.** What the M2 above
  and the table of threads in [measurements.md](measurements.md) say is of
  the flood; neither has been run on the video.
- **Eight threads.** remotex starts a pool of up to eight. Six on six cores
  gain a tenth on one busy sample and lose on the rest; eight on eight have
  not been run.
- **The picture without the loop filter, by eye.** It is 0.3 to 0.9 dB by
  PSNR. A desktop of 2048 wide or more at one sample to the pixel, where
  an edge left unsmoothed is largest on the screen, has not been looked at.
- **What a Windows terminal of scrolling text pays for tile columns.** Each
  doubling costs it 7% to 14% of the bytes where everything else pays 1%.
  Not known: whether it is the small text or how Windows scrolls it, and so
  whether such a desktop at 2048 wide or more pays the same for its four.
- **The README's tables on what `screen-vp9` 0.0.10 codes.** Their captures
  were coded by 0.0.8: the loop filter on at every size, and 2560 wide in
  two tile columns.

## Coverage

The gateway's nine captures decode bit for bit, and none trips a refusal.
A reference of another size, a frame shown again or compound prediction
would be refused by name, and nothing is known to produce them. Segmentation
is decoded, since screen-vp9 0.0.12 codes a frame told where the picture
changed through libvpx's active map; the two `active-map` fixtures are what
that writes, checked against libvpx's decode, and the thirteen frames of the
gateway's sequence at three sizes were compared with libvpx plane by plane
when it was added.

## Robustness

- **A corpus for the fuzz target from the captures' frames.** The target
  (`rust/vp9/fuzz`, see the README) decodes each input on one thread and on
  a pool of three and compares. A few minutes on two workers, about 10,000
  inputs, is the run before a release, and a corpus would make the same
  minutes count for more.
- **The module's vector loops** have only the damaged fixtures of `bun test`
  against them, on one thread and four.

## Verification

`bun test` decodes the eleven 4:4:4 fixtures on one thread to four, and
`bench/run.sh` checks the module against libvpx's MD5s on its samples; the
whole captures are checked by hand with `tmp/verify.sh`. A fixture nearer
the captures' size belongs in `test/`.
