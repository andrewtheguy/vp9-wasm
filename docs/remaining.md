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

The same profile on the gateway's own streams, the 120-frame samples of
`bench/samples.sh`, says something else of a busy frame, which is where a
decoder falls behind:

| function | quiet 1440×900 | busy 1920×1080 | busy 2560×1600 | busy 3840×2160 |
|---|---|---|---|---|
| the loop filter | 26% | 11% | none coded | none coded |
| `decode_coefs`, with `large_token` | 21% | 52% | 61% | 56% |
| `tokens` | 6% | 5% | 7% | 6% |
| `Recon::block`, with `residual` | 5% | 5% | 8% | 9% |
| `inverse_add`, with `add_8x8` | 3% | 4% | 5% | 5% |
| `convolve::predict` and its filters | 4% | 5% | 5% | 6% |
| the row copy | 3% | 1% | 1% | 4% |

A busy frame is two thirds token parsing, and against libvpx the module is
then within a twentieth of its cycles (433 M a frame for 413 at
2560×1600, 294 for 283 at 3840×2160) where on a quiet frame it is 40%
behind: both sit on the boolean's chain (2 below). So what was tried on the
earlier captures and found no faster, the still blocks' copy, the loop
filter's pairs and the empty transform block, was tried on the parts that
are smallest when the frame is slow, and none of it is worth trying again
on these. What a busy frame has is threads, a tile column each.

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

The larger step is the encoder's, and it is paid for in the picture.
Every frame of the captures has a filter level of 7 to 9 and a sharpness of
0: libvpx at this speed takes the level from the quantizer, never below 4
between keyframes, a light filter that still visits every edge. Its
`VP9E_SET_DISABLE_LOOPFILTER` at 2 makes the level 0, at which a frame is
not filtered at all. Measured on what the remotex gateway itself wrote on
2026-10-08, one desktop of four panes at eight sizes: a Windows desktop
over RDP at five and a virtual Mac at three. The first 300 frames of each
were coded again by `screen-vp9` with the filter and without it, at
quality 90 on four threads as the gateway codes them, and the module
decodes each of the sixteen streams as libvpx does. The bytes are without
the filter over with it, the PSNR libvpx's decoder's against the planes
that went in, the cycles a frame on one thread and the mean milliseconds a
frame on four:

| | tile columns | bytes | luma dB | chroma dB | encode | M cycles | ms on four |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Windows, 1280×800 | 1 | +3.1% | 46.69 → 45.81 | 50.77 → 49.29 | −8% | 36.5 → 25.9 | 7.5 → 7.2 |
| Windows, 1440×900 | 2 | +3.7% | 47.53 → 46.58 | 51.63 → 50.12 | −6% | 39.8 → 27.6 | 5.8 → 5.7 |
| Windows, 1600×1000 | 2 | +3.7% | 47.59 → 46.60 | 51.63 → 50.11 | −6% | 47.3 → 33.0 | 6.4 → 6.1 |
| Windows, 1920×1080 | 2 | +3.6% | 47.35 → 46.49 | 51.99 → 50.53 | −5% | 72.0 → 50.6 | 9.0 → 9.8 |
| Mac, 2560×1600 | 2 | +4.4% | 49.81 → 49.00 | 55.24 → 53.60 | −6% | 87.7 → 55.3 | 10.3 → 9.5 |
| Mac, 2880×1800 | 4 | +3.6% | 49.99 → 48.61 | 53.57 → 51.36 | −7% | 79.1 → 46.8 | 10.4 → 6.4 |
| Windows, 3456×2168 | 4 | +3.3% | 50.41 → 49.78 | 55.44 → 54.12 | −3% | 190.3 → 134.3 | 21.3 → 14.2 |
| Mac, 3840×2160 | 4 | +5.5% | 50.64 → 49.82 | 56.03 → 54.55 | −3% | 154.5 → 100.1 | 17.4 → 12.1 |

One thread decodes in 29% to 41% fewer cycles at every size. Four gain 31%
to 38% of the time where the stream is in four tile columns and nothing to
speak of where it is in one or two, up to 2560 wide, because there the
frame waits on the parsing of its columns and the filter was not what held
it. The line falls between 2560 and 2880, where `screen-vp9` goes from two
columns to four, and the two neighbours show it: 9.5 ms for 10.3 against
6.4 for 10.4. The earlier captures, a wlshare desktop and animation and a
Mac screen recording, said the same, and gave two more figures. A
3456×1804 desktop coded in two columns instead of four is 27.4 ms on four
threads for 29.0, so it is the columns and not the width. And the filter
is worth more to the picture than its level suggests, since every later
frame predicts from what it smoothed: at the same luma PSNR a 1440×900 Mac
capture without the filter is quality 97 for 90, 34% more bytes, and then
31.4 M cycles a frame for 40.1.

So `screen-vp9` 0.0.9 leaves the filter out of a 4:4:4 stream in four tile
columns or more, 2880 wide on four threads, and keeps it elsewhere. Not
measured: more than four decoding threads, which a page with eight cores
gets, and the picture by eye.

The first 300 frames are the quiet end of each capture, 40 to 150 KB a
frame. The 300 that took the most bytes, 290 KB to 1.2 MB a frame, were
coded again the same way from a keyframe, and libvpx's decoder timed beside
the module:

| busiest 300 | tile columns | bytes | luma dB | M cycles | ms on four | libvpx, ms on four |
| --- | --- | --- | --- | --- | --- | --- |
| Windows, 1280×800 | 1 | +0.4% | 43.31 → 42.98 | 142.4 → 126.8 | 34.0 → 35.6 | 39.2 → 38.2 |
| Windows, 1440×900 | 2 | +0.7% | 43.34 → 43.00 | 162.0 → 142.3 | 23.1 → 23.2 | 25.7 → 24.1 |
| Windows, 1600×1000 | 2 | +0.2% | 43.37 → 43.00 | 194.7 → 170.4 | 26.2 → 26.1 | 30.4 → 27.6 |
| Windows, 1920×1080 | 2 | +1.2% | 43.44 → 43.07 | 236.1 → 207.0 | 31.0 → 32.9 | 38.1 → 34.3 |
| Mac, 2560×1600 | 2 | +1.1% | 44.96 → 44.40 | 355.2 → 298.3 | 44.1 → 40.9 | 54.3 → 46.6 |
| Mac, 2560×1600 | 4 | +1.3% | 44.90 → 44.35 | 356.4 → 300.6 | 39.1 → 29.4 | 30.9 → 27.2 |
| Mac, 2880×1800 | 4 | +0.8% | 44.81 → 44.34 | 560.1 → 480.8 | 62.9 → 46.3 | 47.8 → 44.6 |
| Windows, 3456×2168 | 4 | +0.5% | 48.13 → 47.79 | 433.7 → 371.9 | 49.2 → 42.4 | 49.3 → 44.6 |
| Mac, 3840×2160 | 4 | +1.1% | 46.80 → 46.23 | 557.9 → 468.3 | 65.0 → 47.7 | 52.5 → 46.9 |

A busy frame is mostly coefficients, so the filter is less of it and of the
stream: 11% to 16% of one thread's cycles, 0.2% to 1.3% of the bytes, 0.3
to 0.6 dB. The line holds: four threads gain 14% to 27% in four columns and
nothing to speak of in one or two. In four columns without the filter the
module's four threads and libvpx's are within 8% of each other, where with it
libvpx's are ahead by up to a quarter.

Four columns under 2880 wide were tried on the 2560×1600 capture, the same
300 frames, the columns forced in a copy of `screen-vp9`:

| columns, filter | MB | luma dB | encode ms | M cycles | ms on four |
| --- | --- | --- | --- | --- | --- |
| two, on | 15.21 | 49.81 | 42.9 | 84.3 | 9.3 |
| two, off | 15.87 | 49.00 | 40.4 | 54.8 | 8.3 |
| four, on | 15.22 | 49.79 | 38.8 | 85.4 | 9.0 |
| four, off | 15.89 | 48.99 | 37.0 | 55.5 | 6.1 |

The columns themselves cost 0.1% of the bytes, nothing of the picture and
1% of one thread's cycles, and the encoder's four threads code them a
tenth faster. With the filter the decoder's four threads get nothing from
them; without it the frame is 6.1 ms for 9.3, a third less, as from 2880
wide. On the busiest 300, in the table above, four columns cost 1.0% of
the bytes, the encoder codes them 14% faster, and four columns without the
filter are 29.4 ms for the 44.1 of two with it, for 2.3% of the bytes and
0.6 dB.

A 2048×1536 capture of the same Mac says the same, two columns with the
filter against four without it: 8.1 ms on four threads and 5.3 on its quiet
300, for 4.1% of the bytes and 0.9 dB, and 45.1 and 28.1 on its busiest,
for 2.2% and 0.5 dB, of which the columns are 0.1% and 1.4% of the bytes.
libvpx's four threads go from 51.9 ms to 27.0 on the busy frames. The
Windows capture at 1920×1080 does not: its busy frames are 28% faster on
four threads in four columns without the filter, 22.6 ms for 31.3, and 16%
larger, 15% of it the columns alone, whether for their 480 samples or for
what is on that desktop. So `screen-vp9` 0.0.10 codes four columns from
2048 wide, and with them leaves the filter out there.

It is what is on that desktop, and not the width. 120 busy frames coded
with the filter, in MB, by the columns:

| | one | two | four |
|---|---|---|---|
| Windows, 1280×800 | 37.4 | 39.9, +7% | 43.2, +8% more |
| Windows, 1920×1080 | 49.5 | 55.5, +12% | 63.1, +14% more |
| Windows, 3456×2168, its top left 1920×1080 | | 15.3 | 15.5, +1.4% |
| Windows, 3456×2168, its top left 2560×1600 | | 28.1 | 28.5, +1.3% |
| Mac, 2560×1600, its top left 1920×1080 | | 69.3 | 70.1, +1.1% |

The two that pay are a Windows terminal at one sample to the pixel,
scrolling random coloured text over the whole screen; the Mac's busy frames
are the same text in a terminal at two samples to the pixel and pay 1%, as
a browser on Windows does. So a column costs about 1% at 1920 wide as it
does above, except on that terminal, where the two columns `screen-vp9`
codes from 1440 wide already cost 12%. Not known: what in it costs, the
small text or how Windows scrolls it, and so whether a desktop of 2048 wide
or more at one sample to the pixel would pay the same for its four.

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
- **Threads.** Mean ms a frame on the samples, by the pool's threads, two
  to four on four cores and five and six on six:

  | | columns | 1 | 2 | 3 | 4 | 5 | 6 |
  |---|---|---|---|---|---|---|---|
  | quiet 1440×900 | 2 | 13.4 | 6.5 | 5.6 | 5.8 | 6.8 | 6.6 |
  | busy 1280×800 | 1 | 46.6 | 33.0 | 32.9 | 33.8 | 37.7 | 38.1 |
  | busy 1920×1080 | 2 | 76.4 | 41.2 | 34.2 | 31.2 | 34.9 | 35.1 |
  | busy 2560×1600, no filter | 4 | 136.0 | 76.6 | 54.3 | 41.7 | 37.7 | 38.8 |
  | busy 3840×2160, no filter | 4 | 95.0 | 48.3 | 38.8 | 31.9 | 32.4 | 32.7 |

  A frame is bound by its columns' parsing: 1.4× on four threads in one
  column, 2.4× in two, 3.0× to 3.3× in four. Past four threads the 2560
  sample gains a tenth and the rest lose up to a sixth, on a machine with
  nothing else to run, so four it stays. Before this a pool of fewer threads
  than the frame had columns decoded it on one, 136 ms and 135 for the 76.6
  and 54.3 above: the threads now take the columns from a queue.

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
