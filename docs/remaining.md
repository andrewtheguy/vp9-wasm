# What remains

What the decoder does, how it is built and what it measures against the
release before are in the [README](../README.md). This is the list of what
it does not do yet, in the order the work would go, with what was measured
on the way and what was tried and found no faster, so that it is not tried
again as it was. Numbers are from the module under Bun or Node on one thread
of a six-core x86 workstation (`tmp/measure.sh`, `tmp/interleave.sh`,
`tmp/jitprof.sh` with `tmp/hot.sh` for a function's instructions), on the
four captures the README's table names, unless stated otherwise.

## Speed

### Where the time goes

Profiled as the module under Node on one thread. Shares of a frame:

| function | desktop | Mac | shader | what it is |
|---|---|---|---|---|
| the loop filter (`decode_rows`'s filter closure) | 31% | 30% | 26% | the masks of a 64×64 block from its modes, and the edge kernels inlined |
| `decode_coefs`, with `large_token` | 20% | 23% | 30% | a transform block's tokens after the first |
| `Recon::block` | 7% | 8% | 8% | the walk over a block's transform blocks, the coefficients into place and out again, intra prediction |
| the row copy (`memory.copy`, libc's `memcpy`) | 6.8% | 2.6% | 2.9% | each row of 64×64 blocks of an inter frame started as the last frame's rows (1 below) |
| `convolve::predict` | 4.7% | 5.5% | 5.5% | the whole-sample copies of the blocks that move or predict from another reference, and the dispatch to the sub-sample filters |
| `tokens` | 4% | 8% | 9% | the first boolean of each transform block, and its contexts |
| `Tile::block` | 4% | 3% | 2.5% | a block's modes and motion vectors, and the mode grid filled |
| `find_mv_refs` | 2.4% | 2% | 1.6% | |
| `BoolDecoder::tree` | 1.8% | 1.7% | 1.4% | the symbols of the mode trees |
| `inverse_add` | 1.6% | 3.6% | 4.4% | the transforms, by vector |
| `memory_fill_wrapper` | 0.8% | | | V8's runtime, for the `memory.fill` behind each small `fill` |

Per frame of the desktop capture (1440×900, 665 frames): 2,200 blocks, 66%
of them skipped; 21K transform blocks, 17K of them 8×8 and nine in ten of
those empty; 59K loop-filter kernel calls, 60% of which find the same
samples on both sides of the edge and leave after their loads. Of the Mac's
(1440×900, 743 frames): 3,300 blocks; 70K transform blocks, nearly all 4×4,
21K of them coded with three coefficients each on average; 103K kernel
calls, 25% leaving early, and 45K of them the narrow four-tap filter. Of
the shader animation (1728×902, 822 frames): 3,550 blocks; 81K 4×4
transform blocks, 31K coded, and 26K 8×8; 144K kernel calls, 37% leaving
early. Every capture is one tile column, so a frame's parsing is one
thread's whatever the pool, and it is what the four-thread time waits on.

### 1. The still blocks, and the copy each one was

Nearly every block of a screen is an inter block with a zero motion vector
from the last frame and no residual: 57% of the desktop capture's area, 51%
of the Mac's, 31% of the shader's. Each such block was a whole-sample copy
from the reference in `convolve::predict`, 2.3 MB a frame on the desktop in
blocks at 2.7 GB/s, the 64-wide stores stalling on lines not in cache. Now
each row of 64×64 blocks of an inter frame starts as the last frame's rows,
copied in one sequential pass before the row waits on the row above
(`Recon::start_row`), and a still block costs nothing in reconstruction: no
dispatch, no strided copy. The captures are 1440×900, whose height is not a
multiple of 8: a still block that crosses the frame's bottom or right edge
is predicted in libvpx from the reference's edge replicated
(`extend_and_predict`), and the samples beyond the edge are read by the
intra prediction and the loop filter of the blocks after it, so the row
replicates the edge row and column over the strip beyond the frame before
its blocks are made. Per frame on one thread, 0.0.3 against this: the
desktop capture 21.6 → 21.2 M cycles, the Mac's 43.3 → 42.5, the shader
animation 58.2 → 58.2, the recording 174.7 → 169.8; the copy is 6.8% of the
desktop's cycles, 2.6% of the Mac's and 2.9% of the shader's, in libc's
`memcpy` under `memory.copy`.

hevc-wasm's second step, **most of that copy not made**, was built and
measured three ways and is slower each way, so it is not to be tried again
as it was (`tmp/logs/buffer-reuse-kernels.patch` and
`tmp/logs/buffer-reuse-settle.patch` hold two of them, on top of the row
copy). A pooled buffer keeps the serial of the frame it was decoded as, each
frame records per 64×64 block whether it left the block as the last frame
had it, and a free buffer holding a frame the last frame descends from is
that frame already wherever no frame on the way changed the block, so a row
copies only the runs of blocks changed. What VP9 adds is that a skipped
block's own left and top edges are deblocked whenever the frame's filter
level is not zero, which it never is on these captures, so whether a still
block came out unchanged is known only after the filter:

- **Each kernel reporting which side it changed a shown sample on**, and the
  block, the one to its left and the one above marked by it. 42% of the
  desktop's blocks, 27% of the Mac's and 23% of the shader's came out kept,
  short of the 57%, 42% and 28% that are identical to the frame before,
  since a vertical kernel changes samples that the horizontal kernel below
  changes back. The
  frame cost more than it saved: the desktop 45.7 M instructions and 22.4 M
  cycles against 42.1 and 21.2, the Mac's 89.6 and 44.9 against 83.0 and
  42.5, the recording 372 and 181 against 340 and 170. The report's own
  instructions were not it: made conditional on a side's block still
  counting, which leaves it out of nearly every call, nothing changed.
- **A wholly still block compared with the reference once** its last kernel
  has run, the kernels as they were. The comparison of a block costs about
  what its copy does, 3.8% of the desktop's instructions, and the frame
  22.6 M cycles against 20.8. Under Node with every function compiled
  optimized up front, the gap is the comparison's alone; the rest of what
  the counts show is tier-up, which both engines start a frame's work in.
- **No tracking, a row copying only its still blocks**, by runs, with
  `memcpy` or by vector: slower than the whole row, 21.1 M cycles against
  20.8 on the desktop, 42.1 against 41.8 on the Mac's, 168.8 against 166.4
  on the recording. The whole row is one sequential pass that also brings
  the reference's lines into cache for the blocks that move.

The copy is the bound on all three, 6.8% of the desktop's cycles and less
elsewhere, and on four threads the frame waits on its parsing anyway.

### 2. Parsing

`decode_coefs` and `tokens` are 25% to 40% of a frame on one thread and
all of it on the critical path with threads. The decoder's two registers are
in locals, the bits read ahead carry their own end marker, the large tokens
are read in a function apart and the token loop is a function per transform
size; V8 now keeps the registers in registers, and the zero-token loop is
about 45 instructions.

What was measured and is not to be tried again as it was: the end marker
alone, which cut 1% of the instructions and no cycles; the large tokens
apart alone, which cost a call per such token and 1% of the cycles; the two
together gain 1% to 2% and are kept.

What remains is the boolean's own chain, from one bin's range to the next's:
the multiply by the probability, the add and shift, the shift of the split
to the top byte, the compare, the select, the leading-zero count and the
two shifts, some thirteen cycles, which no layout of the loop shortens.
Beside it ride the context lookups, two neighbours in the token cache, the
band, and the probability at `probs[band][ctx]` by a multiply by 18 that a
table of the 36 probability triples' offsets would make a shift. The largest
lever is not the decoder's: the captures are one tile column each, and
libvpx codes up to four at 1440 wide (`tile-columns`), which `screen-vp9`
now asks for; the decoder parses tile columns side by side already
(`tiles-608x130`).

**The transform block with no coefficients**, which most are, was measured
and is not to be tried again as it was (`tmp/logs/tokens-empty-path.patch`
holds both forms). `tokens` costs about 45 cycles per transform block on the
Mac capture, 7.5% of its frame, and between two blocks runs a chain from one
block's first boolean through the context byte it stores, which the next
block loads back, adds, and addresses its probability by, into the multiply
that makes the split. The left context kept in a local over the row and the
three first-boolean probabilities in locals, so that no load waits on the
context: no fewer cycles on any capture, the desktop 21.0 M against 20.9,
the Mac's 42.6 against 42.0, the shader animation 60.9 against 60.5, the
recording 169.8 against 168.3, and the function grown from 487 instructions
to 1,384, V8 spilling the window's value word each block. The three splits
computed from the range before the context is known, so that the context's
chain ends in two masks and not a multiply: the same cycles for 1.3 M more
instructions on the Mac's frame. In both, as in `decode_coefs`, the samples
sit on the range recurrence, so the empty block is bounded by the boolean's
chain like every other block and token, and what the other thirty cycles
are is mostly the branch on whether the block has coefficients, which a
third of the Mac's do.

Branch mispredictions are 0.31 M a frame on the Mac capture and 0.12 M on
the desktop's, about a tenth of each frame's cycles at the fifteen to twenty
each costs. On the Mac's, 30% of them are in `decode_coefs`, 23% in the loop
filter's early-outs, 15% in `Recon::block`'s test of each transform block
for coefficients, 8% in `tokens`, 6% in `inverse_add`. The tokens and the
edges are decisions on the data, which no layout makes predictable; the
test in `Recon::block` was not, and is gone (4 below).

### 3. The loop filter

34% to 43% of a frame's cycles on one thread, taken by leaving parts of it
out of the module (which changes the picture, so the parts are near and not
exact). Per frame, in M cycles:

| | desktop | Mac | shader | recording |
|---|---|---|---|---|
| the frame | 20.3 | 39.4 | 53.6 | 159.3 |
| the masks of each 64×64 block, and the walk over them | 0.7 | 0.8 | 0.8 | 4.7 |
| the kernels of the vertical edges | 3.3 | 7.4 | 8.8 | 35.2 |
| the kernels of the horizontal edges | 2.5 | 4.9 | 6.5 | 25.2 |

So it is the kernels, which are vectors already with the eight positions of
an edge in the low eight lanes, and leave early where the samples are the
same across the edge. Per frame and direction, by the widest filter an edge
may take and what it came to:

| | desktop | Mac | shader |
|---|---|---|---|
| narrow edges | 1.7K | 30K | 35K |
| of them filtered | 78% | 86% | 86% |
| 8-sample edges | 21K | 12.5K | 31K |
| of them the same across the edge | 58% | 34% | 51% |
| of them with a position the 7-tap filter takes | 29% | 45% | 35% |
| 16-sample edges | 6.5K | 8.5K | 5.5K |
| of them the same across the edge | 75% | 57% | 67% |

Measured and no faster:

- The six kernels as functions of their own rather than inlined into the
  filter's loop: 4% more instructions and no fewer cycles, although the loop
  is a 5,000-instruction function full of spills.
- Two edges that lie end to end in one vector, whatever their widths
  (`tmp/lf-pair-kept` holds that version): on this tree 17% more
  instructions and 12% more cycles on the Mac capture, 23% and 17% on the
  desktop's. The flat filters sum in 16-bit lanes, so sixteen positions are
  two vectors of them and nothing is saved, and a pair leaves early only
  when both edges would.
- Only the narrow edges paired, whose arithmetic is all in bytes and so
  costs the same for sixteen positions as for eight. Two side by side along
  a horizontal edge, in the walk as it is: 0.8 M fewer instructions a frame
  on the Mac capture and the shader animation, which is what a narrow edge
  costs (about 60 instructions), and 0.2 and 0.1 M fewer cycles of 39.4 and 53.5;
  none on the others. Two one above the other along a vertical edge need
  the walk to take two rows of blocks at a time, since along a row each
  edge reads what the one before it wrote; that walk with the pairs is 3 M
  more instructions a frame on the desktop capture and the shader animation
  and 21 M more on the recording, under Bun, with or without the kernels
  inlined, and without the pairs it is 2 M fewer on the Mac capture and 2 M
  more on the recording: the function's size and what the engine makes of
  it decide more than the arithmetic saved.

What is left in the decoder is the 8- and 16-sample edges that do work,
whose cost is their transposes and their 16-bit sums. Untried: the masks
settled as the blocks are parsed, as hevc-wasm's boundary strengths are,
instead of the scan of the 64 modes of a block at filter time, which is at
most the masks' row above; and libvpx's own `ss00` path for 4:4:4, whose
kernels filter two rows of 8×8 blocks at once, which the pairs above say
is no gain here.

The larger step is not the decoder's. Every frame of the captures has a
filter level of 7 to 9 and a sharpness of 0, a light filter that still
visits every edge, and libvpx's `VP9E_SET_DISABLE_LOOPFILTER` makes the
level 0, at which a frame is not filtered at all: the frame without the
filter is 13.3 M cycles of the desktop capture's 20.3, 25.6 of the Mac's
39.4, 36.9 of the shader animation's 53.6 and 90.8 of the recording's
159.3. What it costs the picture at these qualities is `screen-vp9`'s to
measure.

### 4. The rest

- **Transforms.** The ADST at 8 and 16 and the DCT at 16 and 32 run as
  plain code. The desktop capture has 184 8×8 ADST blocks a frame, 0.4% of
  its instructions; the others have fewer. An exact port needs 32-bit lanes
  for the ADST, whose stages libvpx's C keeps at 32 bits.
- **`Recon::block`**, 7% to 8% before this: the end-of-block count of every
  transform block was read with a bounds check and tested, and the test
  mispredicted 45K times a frame on the Mac capture. Now the parser leaves
  each block's coded transform blocks alone, each with its place, in the
  row's one buffer, and an inter block's residual walks those; an intra
  block, predicted transform block by transform block, compares each with
  the next place. Per frame on one thread: the desktop capture 20.9 → 20.6 M
  cycles, the Mac's 41.9 → 40.7, the shader animation 60.3 → 58.4, the
  recording 167.8 → 164.4; on four threads 22.4 → 22.1, 43.9 → 42.5,
  62.7 → 61.0 and 172.2 → 169.6. Then the 4×4's coefficients, nearly all
  of them, stopped being scattered into a block of zeros and cleared again:
  the parser zeroes sixteen and writes each in its place, and the transform
  loads its two vectors from the row's buffer. Against the step before, on
  one thread: 20.9 → 20.4, 41.0 → 39.5, 55.5 → 53.4, 159.3 → 157.5; on four
  22.7 → 22.4, 42.5 → 41.4, 57.7 → 55.8, 163.2 → 162.2. What remains of it
  is the larger blocks' scatter, few, and the intra predictors, which are
  plain code (intra blocks are 0.6% to 6% of the area).
- **The fills.** Each `fill` of a few bytes of the context arrays, and of the
  32-byte `ModeInfo` over a block's cells, is a `memory.fill` into V8's
  runtime, 0.8% of the desktop capture; whole words as hevc-wasm writes them
  would do, and `ModeInfo` could be half its size, since only a block under
  8×8 has four motion vectors.
- **Four threads.** Against one, by the README table's medians: the desktop
  capture 1.65×, the Mac's 1.8×, the shader animation 1.8×, the Mac
  recording 3.3×, all bound by the one tile's parsing (2 above).

### Elsewhere

- **The target machine.** Every number is from an x86 workstation under Bun
  and Node. The module has not been run on Apple silicon, where V8 lowers
  SIMD128 to NEON differently and the memory system differs; the copies and
  the filter may rank differently there.
- **The native build runs the plain code**: the kernels are written for
  `core::arch::wasm32`. `vp9-bench` is for correctness, not speed.

## Coverage

The six recorded 4:4:4 captures decode bit for bit, and none trips a
refusal. What remotex and wlshare might yet send is a stream of more than
one tile column (decoded, and parsed in parallel), a resize at a keyframe
(decoded), or an intra-only frame (decoded); a reference of another size, a
frame shown again, compound prediction or segmentation would be refused by
name, and nothing is known to produce them.

## Robustness

- **Malformed input must return an error, never trap.** The fuzz target
  (`rust/vp9/fuzz`, see the README) decodes each input on one thread and on
  a pool of three and compares. A few minutes on two workers, about 10,000
  inputs, is the run before a release; the boolean decoder's end marker and
  its overrun check had that before 0.0.3 and no more. A corpus from the
  captures' frames would make the same minutes count for more.
- **The module's vector loops** have only the damaged fixtures of `bun test`
  against them, on one thread and four. The transforms were checked exact
  to the plain code on 120,000 random blocks under Node
  (`tmp/itx-wasm-check`).

## Verification

`bun test` decodes the ten 4:4:4 fixtures on one thread, two and four; the
captures are checked by hand with `tmp/verify.sh` against libvpx's
`framemd5`. A fixture nearer the captures' size, and one of two tile columns
at a desktop's width, belong in `test/`.
