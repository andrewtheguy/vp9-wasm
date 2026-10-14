# What remains

What the decoder does not do yet, and what has not been measured. What it
does and how it is built are in the [README](../README.md), the numbers
behind this list in [measurements.md](measurements.md), and what was built
and found no faster in [tried.md](tried.md).

## Speed

Judged on the terminal and the video of wlshare's captures and on its other
scenarios, as `screen-vp9` 0.0.18 codes them, on an M2 Max, the target,
under Bun and Node, Safari's engine and Chrome's
([measurements.md](measurements.md)); the flood of text is the worst case a
desktop can be, measured for reference and no target, and x86 is the next
target. On one thread the module takes a sixth to two fifths more cycles a
frame than libvpx on the video, the walk and the browser, and is level on
the flood; on four threads, as the page decodes, it is faster than libvpx
by a sixth to a quarter on every video and the walk, and level on the
browser under Bun and an eighth behind under Node. What it takes more is
below, each part's cost measured by a build that leaves it out (which
changes the picture, so the costs are near and not exact) and set beside
libvpx's share of its samples under macOS's `sample`. In the order the work
would go, the largest first:

- **Prediction from the reference.** 17.5 M cycles of a 4K video frame
  under Bun and 14.4 under Node, where libvpx's is 12.4, and the row copy
  beside it 0.7 to 1.4 M, which libvpx's figure takes in; 2 to 4 M more
  than libvpx's on the walk and the browser. The 3 M a 4K video frame more
  under Bun than under Node is the same kernels compiled by the two engines
  for the same ARM, and Safari's is the target: JavaScriptCore's ARM code
  for `convolve` is the first thing to read, as its x86 code was for the
  loop filter. libvpx on the M2 runs its `i8mm` kernels, whose dot products
  of bytes WebAssembly has no equal of, and relaxed SIMD's dot product
  takes signed 7-bit factors, which neither the samples nor the taps are;
  on x86 a row of eight samples is sixteen multiplies and sixteen adds
  either way, whether the eight rows down are loaded or kept in registers
  (keeping them gave 2%).
- **`ModeInfo` smaller, and the block's walk.** What the parts measured
  leave of the gap is the tokens and the modes: on the 4K video 3 M cycles
  under Bun and 5 under Node, on the browser 4 under Bun and 7 under Node,
  where Node takes 8% more of the whole frame than Bun. On x86 the modes
  were 33 M cycles of the browser's 4K frame where libvpx's were 25, and
  `Tile::block` 31% of a quiet 4K frame. The parse of a 64×64 block in the
  inactive segment is mostly the 34-byte `ModeInfo` written to its 64
  cells: 2 KB a block, 4 MB a 4K frame. Only a block under 8×8 has four
  motion vectors and four modes, and those could live in a table of their
  own by cell, `mv` is always `sub_mv[3]` and `mode` always `sub_mode[3]`,
  so the record could be 16 bytes. The motion vectors' candidates and the
  mode trees were 6% each of the browser's frame on x86.
- **The loop filter, under 2048 wide.** 6.8 M cycles of a 1920×1080 video
  frame under Bun and 6.6 under Node, where libvpx's is 4.9, since its
  narrow and 8-sample edges went two to a call as libvpx's `_dual` kernels
  take them, which made a filtered video frame 5% to 8% faster on the M2
  Max. The 16-sample edges are still one at a time, and on the video nine
  in ten of them leave early. The masks settled as the blocks are parsed,
  as hevc-wasm's boundary strengths are, remain the smaller part.
- **Intra prediction in plain code.** 2.4 M cycles of the 4K video frame
  under Bun and 1.9 under Node where libvpx's is 1.3, 3.0 and 2.6 of the
  walk's for 1.7, 4.5 and 4.6 of the browser's for 2.8.
- **The transforms.** 3.4 M cycles of a 4K video frame under Bun and 2.5
  under Node where libvpx's are 2.0, now that the 8- and 16-point
  transforms run in vectors, short DCTs over their top-left corner alone
  and a DC alone as a saturating add of bytes. On x86 a replay of the
  video's blocks gave 97 cycles a block, mostly the 4×4 DCT, 28% of the
  blocks at 73 cycles, of which the kernel is some 150 instructions: V8 there
  makes each vector constant again at each use, takes the low halves of
  32-bit lanes in four instructions, and gives the multiply that rounds a
  fixup of four; the call and the dispatch are some 60 instructions a
  block. The 32-point DCT, which the video has none of, is in plain code.
- **Token parsing** is 47% to 49% of libvpx's video frame on the M2 Max and
  33% to 39% of the walk's and the browser's; on x86 it was 29% to 30% of
  the module's video frame, where it is two thirds of the flood's. Both
  decoders sit on the boolean's chain there, which no layout of the loop
  shortens, and on x86 the module was level; on the M2 no build can leave
  it out, and with the modes it is what the parts leave of the gap, above.
  The token's probability by a table of offsets was no faster on an M2
  ([tried.md](tried.md)).
- **The damage to the painter.** What changed reaches the decoder, as the
  active map, and stops there: the page uploads all three planes of every
  picture to WebGL, 25 MB at 4K for a frame of a kilobyte. The decoder now
  knows which 64×64 blocks a frame wrote (`Frame::kept`); `picture()` could
  say so, as a rectangle or as the rows of blocks, and remotex's paint worker
  upload only that with `texSubImage2D`, or nothing when nothing changed.
  Likely worth more on an iPad than the decode of such a frame now costs.

## Not measured

- **An iPhone or iPad.** The Mac on Apple silicon is measured
  ([measurements.md](measurements.md)): on an M2 under Bun, whose
  JavaScriptCore is Safari's, a frame of the flood is three quarters token
  parsing, waiting on the boolean's chain, and the module is level with
  libvpx's NEON build on one thread (`bench/run.sh`, and what was tried
  there in [tried.md](tried.md)). The memory is fast memory, a load being one
  instruction with no bounds check; the shuffles are each one NEON
  instruction, `zip`, `uzp`, `dup` or `ext` for seven in ten and `tbl` for the
  rest; nothing uses `bitmask`. Not measured: iOS, whose Safari may give a
  shared memory bounds checks, and its two performance cores beside four
  efficiency ones, onto which the thread that parses may be put.
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
