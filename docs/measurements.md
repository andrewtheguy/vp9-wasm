# What was measured

Where a frame's time goes, what threads give, and what the loop filter and
the tile columns cost a stream: the numbers behind what the decoder and
`screen-vp9` now do. What the module measures against its last release and
against libvpx is in the [README](../README.md), what was built and found
no faster in [tried.md](tried.md), and what is still to do in
[remaining.md](remaining.md).

The numbers are from the module under Bun or Node on a six-core x86
workstation. The earlier ones are from four captures made before the
gateway's own: a wlshare desktop, a Mac at 1440×900, a shader animation and
a Mac screen recording, called the desktop, the Mac, the shader and the
recording. The later ones are from what the remotex gateway wrote on
2026-10-08 and the 120-frame samples of `bench/samples.sh` cut from it.
The latest, on the buffers held, are from wlshare's captures of 2026-10-09
and the samples cut from those as `screen-vp9` 0.0.12 codes them.

## Where the time goes

Profiled as the module under Node on one thread. Shares of a frame:

| function | desktop | Mac | shader | what it is |
|---|---|---|---|---|
| the loop filter (`decode_rows`'s filter closure) | 31% | 30% | 26% | the masks of a 64×64 block from its modes, and the edge kernels inlined |
| `decode_coefs`, with `large_token` | 20% | 23% | 30% | a transform block's tokens after the first |
| `Recon::block` | 7% | 8% | 8% | the walk over a block's transform blocks, the coefficients into place and out again, intra prediction |
| the row copy (`memory.copy`, libc's `memcpy`) | 6.8% | 2.6% | 2.9% | each row of 64×64 blocks of an inter frame started as the last frame's rows |
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
behind: both sit on the boolean's chain, some thirteen cycles from one
bin's range to the next's. So what was tried on the earlier captures and
found no faster ([tried.md](tried.md)) was tried on the parts that are
smallest when the frame is slow. What a busy frame has is threads, a tile
column each.

Branch mispredictions are 0.31 M a frame on the Mac capture and 0.12 M on
the desktop's, about a tenth of each frame's cycles at the fifteen to twenty
each costs. On the Mac's, 30% of them are in `decode_coefs`, 23% in the loop
filter's early-outs, 15% in `Recon::block`'s test of each transform block
for coefficients, 8% in `tokens`, 6% in `inverse_add`. The tokens and the
edges are decisions on the data, which no layout makes predictable; the
test in `Recon::block` was not, and is gone.

## The loop filter's parts

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

## Threads

runtime, 0.8% of the desktop capture; whole words as hevc-wasm writes them
would do, and `ModeInfo` could be half its size, since only a block under
8×8 has four motion vectors.
Mean ms a frame on the samples, by the pool's threads, two to four on four
cores and five and six on six:

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
nothing else to run, so the measurements stay at four. Until 0.0.6 a pool
of fewer threads than the frame had columns decoded it on one, 136 ms and
135 for the 76.6 and 54.3 above; the threads now take the columns from a
queue.

## The loop filter and the tile columns, in the encoder

The loop filter is 34% to 43% of a quiet frame, and leaving it out is the
encoder's to do and is paid for in the picture. Every frame of the captures has a filter level of 7 to 9 and a sharpness of
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
columns or more, 2880 wide on four threads, and keeps it elsewhere.

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
codes from 1440 wide already cost 12%.

## The buffers held, on what `screen-vp9` 0.0.12 codes

The samples of 2026-10-09 are cut by wlshare's `scripts/vp9-samples.sh`
from its captures of what a session handed its encoder, ten sizes from
1280×800 to 3840×2160, the six from 2048 wide at two pixels to the point,
coded again by `screen-vp9` 0.0.12 on four threads: told where the picture
changed, which libvpx takes as its active map, in four tile columns without
the loop filter from 2048 wide. The quiet sample of a size is the start of a
terminal session at it, the busy one the 120 frames in a row of a flood of
coloured text whose frames said the most pixels changed. A quiet inter frame
is 550 bytes at 1440×900 and 1.4 KB at 4K; 99.9% of its 8×8s are in the
inactive segment, so still, and it touches one or two of its 345 64×64
blocks at 1440×900, three or four of 2,040 at 4K. The samples and their
libvpx MD5s are `dist/vp9-bench` in the wlshare repository, with a README of
which frames of which capture each is.

Profiled as the module under Node on one thread, a quiet 4K frame was 60%
the row copy, libc's `memcpy` behind `memory.copy`, and 17% `Tile::block`,
most of that the mode grid filled over the inactive blocks. The copy is now
made only where the buffer does not hold the samples already (the README):
120 frames of the quiet 4K sample copied 82 MB where the whole rows were
3.0 GB, and 328 MB when a frame's chain of references was kept eight deep,
since the buffer a frame is given is as often the one the golden slot let
go of, ten frames old, as the one from two frames back. One thread, 120
frames, the medians of three rounds; libvpx's milliseconds are its mean, the
one number ffmpeg gives:

| | M cycles a frame | | | median ms a frame | | |
|---|---|---|---|---|---|---|
| | before | after | libvpx | before | after | libvpx |
| quiet 1440×900, two columns, filtered | 9.6 | 7.5 | 5.3 | 1.29 | 0.77 | 1.36 |
| quiet 1920×1080, two, filtered | 13.4 | 9.2 | 7.6 | 2.83 | 0.70 | 2.17 |
| quiet 2560×1600, four, unfiltered | 15.4 | 9.5 | 12.5 | 2.48 | 0.59 | 3.97 |
| quiet 3840×2160, four, unfiltered | 22.7 | 12.7 | 24.8 | 4.78 | 1.08 | 8.31 |
| busy 1280×800, one, filtered | 268 | 268 | 256 | 84.4 | 84.0 | 79.2 |
| busy 1920×1080, two, filtered | 555 | 555 | 542 | 173.9 | 173.7 | 169.2 |
| busy 2560×1600, four, unfiltered | 678 | 679 | 679 | 211.7 | 210.5 | 212.5 |
| busy 3840×2160, four, unfiltered | 1415 | 1410 | 1412 | 466 | 454 | 455 |

A busy frame writes every block and is as it was, the stamps costing nothing
that shows; and the module is within a fortieth of libvpx's cycles on the
three largest, a twentieth on the smallest, those frames being token parsing. A quiet frame's mean is
well above its median, 4.7 ms for 1.1 at 4K, for the keyframe and the first
frames after it, each buffer of the pool copied whole the first time it is
given a frame. On four threads the quiet frames go from 2.4 ms to 1.4 at
1440×900, 2.9 to 1.7 at 1920×1080, 2.9 to 2.2 at 2560×1600 and 4.5 to 3.5
at 3840×2160, libvpx taking 1.9, 2.7, 2.6 and 4.6; the four-thread frame
is now mostly the pool's hand-off and the parse of one tile column. What
is left of a quiet frame on one thread is `Tile::block`, half of it the
34-byte `ModeInfo` written to the 64 cells of each inactive 64×64 block
([remaining.md](remaining.md)).
