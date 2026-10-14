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
module takes a third to two thirds more cycles a frame than libvpx on the
video and the walk, a quarter to a third more on the browser, two thirds
more or worse on a quiet terminal under 2048 wide, most of it the engine
still starting (warm, the terminal and uncover are level with libvpx), and
half libvpx's on the terminal from 2048 wide; it is level only on the
flood, which is token parsing. On four threads it is ahead of libvpx or
within a tenth everywhere but the browser at 4K and uncover at 1920×1080.
Set beside libvpx's own profile, the module's tokens and modes are level on
the video; what it takes more is below, each part's cost measured by a
build that leaves it out (which changes the picture, so the costs are near
and not exact). In the order the work would go:

- **The loop filter, under 2048 wide.** 17.9 M cycles of a 1920×1080 video
  frame's 67 on x86 under Bun, where libvpx's is 11.1 of 45, and 1.3 of the
  quiet terminal's 5.9 at 1440×900, before its narrow and 8-sample edges
  went two to a call as libvpx's `_dual` kernels take them, which made a
  filtered video frame 5% to 8% faster on an M2 Max under either engine
  ([measurements.md](measurements.md)). The 16-sample edges are still one
  at a time, and on the video nine in ten of them leave early. What the
  filter now costs against libvpx has not been measured on the Mac. The
  masks settled as the blocks are parsed, as hevc-wasm's boundary strengths
  are, remain the smaller part.
- **The transforms.** About 10 M cycles of a 4K video frame where libvpx's
  are 6.7, down from 14.0 now that the 8- and 16-point transforms run in
  vectors, short DCTs over their top-left corner alone and a DC alone as a
  saturating add of bytes ([measurements.md](measurements.md)). A replay of
  the video's blocks gives 97 cycles a block; what is left is mostly the 4×4
  DCT, 28% of the blocks at 73 cycles, of which the kernel is some 150
  instructions: V8 makes each vector constant again at each use, three
  instructions, takes the low halves of 32-bit lanes in four, and gives the
  multiply that rounds a fixup of four for a product no constant here can
  make. The call and the dispatch are some 60 instructions a block. The
  32-point DCT, which the video has none of, is in plain code.
- **Intra prediction in plain code.** 5.1 M cycles of the 4K video frame
  where libvpx's is 3.1, 6.4 of the walk's for 3.8, 10.3 of the browser's
  for 6.1, and 1.0 of the quiet terminal's 5.9 at 1440×900, a sixth of it.
- **`ModeInfo` smaller, and the block's walk.** The modes are 33 M cycles of
  the browser's 4K frame where libvpx's are 25, and `Tile::block` is 31% of
  a quiet 4K frame, where the module already takes half libvpx's cycles. The
  parse of a 64×64 block in the inactive segment is mostly the 34-byte
  `ModeInfo` written to its 64 cells: 2 KB a block, 4 MB a 4K frame. Only a
  block under 8×8 has four motion vectors and four modes, and those could
  live in a table of their own by cell, `mv` is always `sub_mv[3]` and
  `mode` always `sub_mode[3]`, so the record could be 16 bytes. The motion
  vectors' candidates and the mode trees are 6% each of the browser's frame.
- **Sub-sample prediction** is 44 M cycles of the 4K video frame where
  libvpx's is 31, and is near what 128-bit vectors do: a row of eight
  samples is sixteen multiplies and sixteen adds, the same instructions
  whether the eight rows down are loaded or kept in registers (keeping them
  gave 2%), where libvpx's AVX2 multiplies byte pairs and adds them in one
  instruction, thirty-two products at a time. WebAssembly has no such
  instruction, and relaxed SIMD's dot product takes signed 7-bit factors,
  which neither the samples nor the taps are.
- **Token parsing** is 29% to 30% of a video frame and 22% of the walk's,
  where it is two thirds of the flood's. Both decoders sit on the boolean's
  chain there, which no layout of the loop shortens, and the module is
  level. The token's probability by a table of offsets was no faster on an
  M2 ([tried.md](tried.md)).
- **The damage to the painter.** What changed reaches the decoder, as the
  active map, and stops there: the page uploads all three planes of every
  picture to WebGL, 25 MB at 4K for a frame of a kilobyte. The decoder now
  knows which 64×64 blocks a frame wrote (`Frame::kept`); `picture()` could
  say so, as a rectangle or as the rows of blocks, and remotex's paint worker
  upload only that with `texSubImage2D`, or nothing when nothing changed.
  Likely worth more on an iPad than the decode of such a frame now costs.

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
- **The video on Apple silicon against libvpx.** What the M2 above says of
  libvpx is of the flood. The video has been run on an M2 Max only against
  the module itself, for the loop filter's pairs
  ([measurements.md](measurements.md)), where a 1920×1080 frame took 31 M
  cycles on one thread under Bun and 29 under Node, against 56 to 69 on
  x86.
- **Eight threads.** remotex starts a pool of up to eight. Five and six on
  six cores gain nothing on the video and the walk and 12% on the browser at
  4K; eight on eight have not been run.
- **Four columns without the filter from 1440 wide.** On the video they
  would make a frame at 1440×900 or 1920×1080 23% to 24% faster on four
  threads for 2.1% to 2.3% of the bytes and 0.3 dB
  ([measurements.md](measurements.md)); the columns no longer cost what the
  flood of a Windows terminal paid for them, which is a reference and no
  target. It is `screen-vp9`'s to choose, once the text of a quiet terminal
  without the filter has been looked at.
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
