# What remains

What the decoder does not do yet, and what has not been measured. What it
does and how it is built are in the [README](../README.md), the numbers
behind this list in [measurements.md](measurements.md), and what was built
and found no faster in [tried.md](tried.md).

## Speed

A busy frame, where a decoder falls behind, is two thirds token parsing,
and there the module is within a twentieth of libvpx's cycles: both sit on
the boolean decoder's chain, which no layout of the loop shortens. So
nothing below is large. In the order the work would go:

- **The loop filter's masks settled as the blocks are parsed**, as
  hevc-wasm's boundary strengths are, instead of the scan of a 64×64
  block's modes at filter time. The masks and their walk are 2% to 3% of a
  quiet frame, and only of a stream that has the filter, which from
  `screen-vp9` 0.0.10 is one under 2048 wide.
- **The token's probability by a table of offsets.** It is found at
  `probs[band][ctx]` by a multiply by 18 that a table of the 36 triples'
  offsets would make a shift. It rides beside the boolean's chain, not on
  it.
- **`ModeInfo` smaller.** The context fills of a few bytes are whole words
  now, as hevc-wasm writes them, and the parse of a 64×64 block in the
  inactive segment is mostly the 34-byte `ModeInfo` written to its 64 cells:
  2 KB a block, 4 MB a 4K frame, half of the parser's block function in a
  native line profile of the quiet 4K sample, where that function is a third
  of the frame. Only a block under 8×8 has four motion vectors and four
  modes, and those could live in a table of their own by cell, `mv` is
  always `sub_mv[3]` and `mode` always `sub_mode[3]`, so the record could be
  16 bytes.
- **The damage to the painter.** What changed reaches the decoder, as the
  active map, and stops there: the page uploads all three planes of every
  picture to WebGL, 25 MB at 4K for a frame of a kilobyte. The decoder now
  knows which 64×64 blocks a frame wrote (`Frame::kept`); `picture()` could
  say so, as a rectangle or as the rows of blocks, and remotex's paint worker
  upload only that with `texSubImage2D`, or nothing when nothing changed.
  Likely worth more on an iPad than the decode of such a frame now costs.
- **The transforms in plain code**: the ADST at 8 and 16 and the DCT at 16
  and 32. A quiet desktop has 184 8×8 ADST blocks a frame, 0.4% of its
  instructions; a busy frame's have not been counted. An exact port needs
  32-bit lanes for the ADST, whose stages libvpx's C keeps at 32 bits.
- **Intra prediction in plain code**, and the scatter of the coefficients
  of a transform block larger than 4×4. Intra blocks are 0.6% to 6% of the
  area.

## Not measured

- **Apple silicon.** Every number is from an x86 workstation. V8 lowers
  SIMD128 to NEON differently there and the memory system differs, so the
  parts of a frame may rank differently.
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
