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
and the samples cut from those as `screen-vp9` 0.0.12 codes them. The
last section is what a change is judged on now: the scenarios of wlshare's
captures of 2026-10-13, as `screen-vp9` 0.0.18 codes them. The busy samples
of every section before it are a flood of text, the worst case. The walk's
numbers are of its first 120 frames, the walk down alone, of a capture that
stopped the video and settled on black (the artifacts drive's
`archive/bench-walk-stopped/`), but for the walk's rows of what was kept,
which are of the walk captured again on 2026-10-15, all 174 frames of it:
down, back up and settled on the playing video.

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

## The scenarios, on what `screen-vp9` 0.0.18 codes

The samples of 2026-10-14 are cut from wlshare's captures of 2026-10-13,
eight scenarios at the ten sizes, and are named for the capture each is cut
from. A change is judged on the terminal, which is the quiet desktop, and on
the video, a clip played full screen, which is the busy one; the flood of
coloured text that was the busy sample until now changes nearly every pixel
of every frame, which a desktop almost never does, and is measured for
reference. What was concluded from it above, that a busy frame is two
thirds token parsing and the module within a twentieth of libvpx, is true of
the flood and of nothing else here.

Release 0.0.9 against ffmpeg's libvpx, 120 frames of each and 48 of
uncover, the medians of three rounds of `bench/run.sh`, which counts the
frames after the first: a run of the first frame alone is taken off each
run, since the start of Bun and the module's compiling, or of ffmpeg, is
175 to 350 M cycles, ffmpeg's the larger, as much as 120 quiet frames. Counted
with it, as the first table of this section was, a quiet frame was a third
of the module's cycles too few against libvpx's. The module's milliseconds
are its own clock's mean, libvpx's its wall time; the mean of a quiet sample
is mostly its first frames after the keyframe, its median a quarter of it.

| | columns, filter | M cycles, module | libvpx | | mean ms on four, module | libvpx |
|---|---|---|---|---|---|---|
| terminal 1280×800 | 1, on | 5.3 | 2.1 | +152% | 1.46 | 1.45 |
| terminal 1440×900 | 2, on | 6.0 | 2.9 | +107% | 1.27 | 1.74 |
| terminal 1600×1000 | 2, on | 6.4 | 3.8 | +68% | 1.72 | 2.05 |
| terminal 1920×1080 | 2, on | 7.2 | 5.0 | +44% | 1.61 | 2.35 |
| terminal 2048×1536 | 4, off | 6.9 | 7.2 | -4% | 2.01 | 1.64 |
| terminal 2560×1600 | 4, off | 7.6 | 10.3 | -26% | 2.16 | 2.06 |
| terminal 2880×1800 | 4, off | 8.7 | 13.4 | -35% | 2.41 | 2.72 |
| terminal 3200×2000 | 4, off | 9.7 | 16.8 | -42% | 2.46 | 3.16 |
| terminal 3456×2168 | 4, off | 10.0 | 20.0 | -50% | 2.86 | 4.05 |
| terminal 3840×2160 | 4, off | 10.2 | 22.2 | -54% | 3.12 | 3.91 |
| video 1280×800 | 1, on | 43.2 | 26.4 | +64% | 6.90 | 7.58 |
| video 1440×900 | 2, on | 51.5 | 33.0 | +56% | 6.07 | 6.55 |
| video 1600×1000 | 2, on | 59.8 | 39.0 | +53% | 6.78 | 7.31 |
| video 1920×1080 | 2, on | 67.5 | 45.3 | +49% | 7.62 | 8.74 |
| video 2048×1536 | 4, off | 84.6 | 58.6 | +44% | 8.70 | 8.08 |
| video 2560×1600 | 4, off | 89.1 | 65.8 | +35% | 9.31 | 9.78 |
| video 2880×1800 | 4, off | 110.6 | 80.0 | +38% | 11.48 | 11.48 |
| video 3200×2000 | 4, off | 130.8 | 95.7 | +37% | 13.34 | 14.75 |
| video 3456×2168 | 4, off | 150.5 | 109.3 | +38% | 14.68 | 16.50 |
| video 3840×2160 | 4, off | 147.5 | 107.5 | +37% | 14.95 | 16.64 |
| browser 1920×1080 | 2, on | 37.8 | 30.6 | +24% | 5.29 | 6.29 |
| browser 3840×2160 | 4, off | 109.5 | 83.2 | +32% | 12.88 | 10.40 |
| drag 1920×1080 | 2, on | 21.2 | 14.2 | +49% | 3.22 | 4.05 |
| drag 3840×2160 | 4, off | 37.9 | 37.9 | +0% | 6.04 | 6.20 |
| uncover 1920×1080 | 2, on | 25.4 | 12.5 | +103% | 4.32 | 3.74 |
| uncover 3840×2160 | 4, off | 53.6 | 45.1 | +19% | 8.76 | 8.55 |
| walk 1920×1080 | 2, on | 53.4 | 33.8 | +58% | 6.43 | 6.66 |
| walk 3840×2160 | 4, off | 117.0 | 80.8 | +45% | 12.00 | 12.35 |
| flood 1280×800, for reference | 1, on | 267.6 | 256.8 | +4% | 67.91 | 76.61 |
| flood 1920×1080, for reference | 2, on | 567.8 | 552.7 | +3% | 77.08 | 89.51 |
| flood 2560×1600, for reference | 4, off | 673.9 | 666.8 | +1% | 62.00 | 61.15 |
| flood 3840×2160, for reference | 4, off | 1393.1 | 1374.8 | +1% | 122.68 | 124.58 |

On one thread the module is a third to two thirds behind libvpx on the
video and the walk, at every size, and two thirds behind or more on a quiet
terminal under 2048 wide, where the loop filter is on: 3 M cycles a frame
more, of 6, most of which is the engine still starting (below). From 2048 wide, in four columns without the filter, it takes
half libvpx's cycles on the terminal. Four threads hide the rest: the module
is ahead of libvpx or within a tenth everywhere but the browser at 4K, 12.9
ms a frame for 10.4, and uncover at 1920×1080, 4.3 for 3.7, whose frames
each uncover a window anew.

Uncover's rows are of its sample cut again on 2026-10-14 from its capture
trimmed to begin at the terminal's first move: as captured it began with
two keyframes of the still page and 3.5 s of nothing. Its frames cost the
same a frame either way, to 5% on either decoder. The second
keyframe had seemed to cost 221 M cycles, a run of two frames less a run of
one; but the second frame of the trimmed sample, an inter frame of 2 KB,
costs 107 M cycles the same way, four frames of the run, and the terminal's
53 M, nine of its frames: what a run's second frame costs is mostly the
engine still starting, whatever the frame.

The engine's start is in every sample's frames. Bun's JavaScriptCore runs
the module in an interpreter, then as its quick compiler made it, and
compiles each function again with its optimising compiler on a thread of
its own once the function has run enough. `bench/run.sh` counts the whole
process and takes off a run of the first frame alone, so the frames after
the first count that thread and the slower code that ran before it. Of a
run of the terminal's 120 frames on one thread the compiler's thread takes
417 M cycles, 347 M of them after the first frame, 2.9 M a frame of its
6.2; of the 1920×1080 video's, 873 M, 811 M of them after the first frame,
6.8 M a frame of 68. With the optimising compiler off the terminal is 3.1 M
cycles a frame, and the video 108. Decoded once whole in the process by a
decoder of its own before it is counted, M cycles a frame on one thread,
this branch's module beside libvpx's of the table above:

| | as `bench/run.sh` counts | after one pass | libvpx |
|---|---|---|---|
| terminal 1440×900 | 6.2 | 2.9 | 2.9 |
| uncover 1920×1080 | 28.3 | 12.4 | 12.5 |
| video 1280×800 | 44.1 | 34.4 | 26.4 |
| video 1920×1080 | 68.4 | 58.5 | 45.3 |
| video 3840×2160 | 140.0 | 129.6 | 107.5 |

The video's second pass leaves the compiler's thread 24 M cycles, nothing to
speak of; the terminal's 148 M, since its light frames make a function hot
slowly, so its 2.9 still holds some. After four passes the terminal's
decoding thread alone takes about 1.3 M cycles a frame, and the whole
process from 1.4 to 3.1 after two to eight passes, as the compiling lands
in one pass or another. Warm, then, the module is level with libvpx on the
terminal and uncover and a fifth to a third behind on the video, and the
cold gaps of the table above on the quiet samples are the engine's start.

A page pays it once. remotex keeps the module's worker for the paint
worker's life, so a stream or a resize after the first runs code already
optimised; each function is compiled again once in the page, when it first
runs hot, which on a desktop that turns from text to video is then. It is
0.4 to 0.9 G cycles of the compiler's thread, a tenth to a third of a second
of one core, and the slower code before it, in a session's first seconds.
Not followed further, since a session past those seconds runs at the warm
figure, and builds compared cold each pay the same start: the benchmark
still counts cold. V8, Chrome's engine, starts otherwise and was not
measured.

A picture held while the next frame decodes, as the page holds one while it
uploads it since 0.0.9, costs nothing: the cycles a frame are the same to
1% on the terminal, the video, the browser and drag, whether the last
picture is released before the next frame or after it.

What libvpx and the module spend a frame on, on one thread: the module
profiled under Node (`node --perf-basic-prof`), libvpx as `vpxdec` built
from its 1.16.0 source as Debian configures it, with symbols, whose counts
are ffmpeg's to 3%. Each function is put with the work it does and its share
scaled to the cycles above; M cycles a frame, module and libvpx:

| | video 1920×1080 | video 3840×2160 | walk 3840×2160 | browser 3840×2160 |
|---|---|---|---|---|
| tokens: `decode_coefs`, `tokens`, `large_token`; `decode_coefs`, `vp9_decode_block_tokens` | 19.7, 16.9 | 45.5, 44.5 | 26.5, 24.7 | 19.6, 20.4 |
| modes: `Tile::block` and the trees; `vp9_read_mode_info` and `decode_block` | 6.8, 6.1 | 19.6, 20.1 | 18.1, 17.3 | 33.1, 24.9 |
| prediction from the reference: `convolve`; `vpx_convolve*`, `dec_build_inter_predictors_sb` | 11.4, 7.6 | 43.6, 30.6 | 37.3, 28.4 | 22.9, 26.2 |
| reconstruction: `Recon::block`, `residual`, the transforms, intra prediction | 6.8, 2.7 | 25.6, 9.8 | 21.1, 8.1 | 19.0, 7.3 |
| the loop filter | 16.7, 11.1 | | | |
| the rest, mostly the row copy (libc's `memmove`) | 4.2, 0.8 | 10.5, 1.7 | 11.5, 1.9 | 12.7, 3.8 |

Token parsing and the modes, which every earlier section found the frame
waiting on, are level with libvpx's on the video. The module's 40 M cycles
more on the 4K video frame are its reconstruction, 16 M, its prediction, 13
M, and the rest, 9 M, nearly all of it the libc `memmove` behind the row
copy: every row of an inter frame starts as the last frame's (`start_row`), 25
MB a 4K frame, where on the video every block is then predicted again. libvpx
makes no such copy and predicts a still block by `vpx_convolve_copy`, 10 M of
its frame. Under 2048 wide the loop filter is 6 M more of the 22. On the
browser, a page scrolled, it is the modes, 8 M more, and not the
prediction.

The threads, on the video, the walk and the browser: mean milliseconds a
frame after the first, module and libvpx, one thread and four as above,
two to four on four cores and five and six on six:

| | 1 | 2 | 3 | 4 | 5 | 6 |
|---|---|---|---|---|---|---|
| video 1280×800, one column | 13.8, 8.2 | 7.4, 8.2 | 7.0, 8.5 | 6.9, 7.6 | 7.7, 7.7 | 8.1, 7.6 |
| video 1920×1080, two | 21.3, 14.2 | 12.1, 10.1 | 9.2, 9.8 | 7.5, 8.9 | 7.4, 8.8 | 7.8, 9.0 |
| video 2560×1600, four | 27.9, 20.5 | 15.7, 13.7 | 11.8, 13.8 | 9.3, 9.8 | 9.8, 9.9 | 10.7, 10.0 |
| video 3840×2160, four | 46.1, 33.8 | 26.4, 22.3 | 18.9, 22.1 | 14.9, 16.2 | 14.8, 16.9 | 15.6, 16.7 |
| walk 3840×2160, four | 37.7, 25.6 | 20.9, 16.8 | 13.9, 15.1 | 12.2, 12.4 | 12.1, 12.6 | 12.4, 12.8 |
| browser 3840×2160, four | 34.7, 25.9 | 20.8, 16.6 | 15.1, 13.6 | 12.7, 10.3 | 11.1, 10.3 | 11.4, 10.2 |

What the flood said of the threads holds of the video: the module gains up
to four and nothing past them, but on the browser, 12% at five, so the
measurements stay at four. With one column its pool still makes the frame
twice as fast, filtering and reconstructing beside the parsing, where
libvpx's gains are its loop filter's thread alone.

What each part of the module costs, measured as the cycles a frame a build
saves that leaves the part out, on one thread, M cycles a frame of the base
build's. Leaving the residual out of a filtered stream changes what the loop
filter does too, so the unfiltered 4K samples give the clean numbers:

| left out | video 1920×1080 (67.3) | video 3840×2160 (147.2) | walk 3840×2160 (116.0) | browser 3840×2160 (110.4) | terminal 1440×900 (5.9) |
|---|---|---|---|---|---|
| intra prediction | 2.2 | 5.1 | 6.4 | 10.3 | 1.0 |
| the transforms in plain code | 0.8 | 3.9 | 4.2 | 1.0 | |
| every transform | 13.4 | 14.0 | 10.5 | 2.8 | |
| the row copy | 0.6 | 4.8 | 5.0 | 8.1 | |
| the loop filter | 17.9 | | | | 1.3 |

libvpx's own transforms are 6.7 M cycles of the 4K video frame, 4.3 of the
walk's and 1.2 of the browser's, its intra prediction 3.1, 3.8 and 6.1. A 4K
video frame has about 100,000 transform blocks: 72,000 4×4 DCTs, 41,000 of
them a DC alone, 5,000 4×4 ADSTs, 22,000 8×8 DCTs, 13,000 of them a DC
alone, and some 500 of 16×16; the module's transforms are some 140 cycles a
block, libvpx's 67.

The transforms were then timed alone, replaying the 3,000,000 transform
blocks the native decoder met in the 4K video's first frames through
`inverse_add` as `residual` calls it, in a module of the transforms alone
under Node (`tmp/itx`), cycles a block of each kind, before and after the
8- and 16-point transforms went into vectors, a DCT short enough for its
top-left 4×4 or 8×8 alone, and the DC alone as a saturating add of bytes:

| kind | share of the blocks | before | after |
|---|---|---|---|
| a DC alone | 52% | 67 | 35 |
| 4×4 DCT | 28% | 72 | 73 |
| 4×4 ADST | 10% | 104 | 93 |
| 8×8 DCT, 86% of them a top-left 4×4 | 7.4% | 295 | 233 |
| 8×8 ADST | 1.9% | 923 | 428 |
| 16×16 | 0.6% | 4,649 | 1,218 |
| every block | | 156 | 97 |

Before, the 16×16s and the 8×8 ADSTs, in plain code, were 2.5% of the blocks
and a third of the cycles. Each kernel is exact to the plain code over the
replay and over 1,600,000 random blocks of every kind, their coefficients at
the ends of 16 bits among them. In the decoder, M cycles a frame on one
thread and four, before and after:

| | one thread | | four | |
|---|---|---|---|---|
| terminal 1440×900 | 6.2 | 6.3 | 7.9 | 8.0 |
| video 1280×800 | 44.1 | 44.0 | 42.3 | 41.8 |
| video 1920×1080 | 67.3 | 66.9 | 67.8 | 67.6 |
| video 2560×1600 | 85.6 | 83.7 | 89.4 | 87.2 |
| video 3840×2160 | 141.3 | 137.2 | 146.7 | 142.3 |
| flood 1920×1080 | 570.9 | 574.7 | 583.1 | 588.9 |

The terminal has 6,800 transform blocks in its 120 frames, and the flood's,
nine in ten of them whole 4×4 DCTs, replay faster (109 cycles a block to
94); counted whole with `perf stat`, each is within its own noise, the flood
0.2% fewer instructions.

Two changes came of it, checked bit for bit on every sample on one thread
and four. The row of an inter frame was copied whole from the LAST
reference before its blocks were made, so that a still block was in place
already; on a video, whose every block is predicted again, that was 25 MB a
4K frame copied for nothing. A 64×64 block is now copied only where a still
block of it keeps the samples, decided once its row is parsed. And the
vector sub-sample filters keep the rows the next output row shares in lanes
rather than write an intermediate and load eight rows again. M cycles a
frame, one thread and four, the base, the copy alone and both:

| | one thread | | | four | | |
|---|---|---|---|---|---|---|
| terminal 1440×900 | 6.2 | 5.9 | 6.0 | 8.3 | 7.8 | 8.5 |
| terminal 3840×2160 | 10.2 | 10.4 | 10.2 | 12.1 | 12.4 | 12.2 |
| video 1920×1080 | 67.3 | 67.7 | 67.3 | 68.2 | 68.9 | 68.0 |
| video 3840×2160 | 149.8 | 146.8 | 144.4 | 152.4 | 148.2 | 145.9 |
| walk 3840×2160 | 119.2 | 116.6 | 114.1 | 120.5 | 117.5 | 114.7 |
| browser 3840×2160 | 114.6 | 109.1 | 107.6 | 127.4 | 117.7 | 118.6 |
| drag 3840×2160 | 38.2 | 36.8 | 37.3 | 43.3 | 39.9 | 39.8 |
| uncover 1920×1080 | 25.4 | 25.7 | 26.0 | 23.8 | 23.8 | 24.0 |

The filters gain less than the intermediate's loads would suggest: on x86 a
load widened from memory is one instruction, as the move of a register is,
so the sixteen multiplies and sixteen adds of a row of eight samples are
the filter whichever way its rows arrive.

What `screen-vp9` chooses, on the video. Its rule, four tile columns
without the loop filter from 2048 wide and two or fewer with it below, was
settled on what the remotex gateway wrote and on the flood of a Windows
terminal, which paid 12% to 16% of its bytes for two columns more; the
video does not. The first 120 frames of the video captures at 1440×900,
1920×1080 and 2560×1600, the frames of the benchmark's samples, coded again
by libvpx as `screen-vp9` 0.0.18 codes them on four threads, the columns
and the filter forced in a copy of it; the luma PSNR against the capture,
and M cycles a frame on one thread and mean ms on four, module and libvpx:

| | MB | luma dB | M cycles, one thread | ms on four, module | libvpx |
|---|---|---|---|---|---|
| 1440×900, two columns, filter on (now) | 5.13 | 49.05 | 51.6 | 6.12 | 6.49 |
| two, off | 5.24 | 48.74 | 39.1 | 5.27 | 4.95 |
| four, on | 5.13 | 49.05 | 52.1 | 5.95 | 6.11 |
| four, off | 5.24 | 48.74 | 39.6 | 4.68 | 4.13 |
| 1920×1080, two columns, filter on (now) | 6.46 | 50.16 | 67.6 | 7.60 | 8.87 |
| two, off | 6.61 | 49.84 | 48.4 | 6.57 | 6.67 |
| four, on | 6.47 | 50.16 | 67.5 | 7.25 | 8.13 |
| four, off | 6.61 | 49.83 | 48.7 | 5.84 | 6.11 |
| 2560×1600, four columns, filter off (now) | 11.43 | 50.61 | 85.8 | 9.33 | 9.83 |
| four, on | 11.18 | 51.10 | 121.4 | 12.49 | 13.48 |
| two, on | 11.17 | 51.10 | 121.4 | 12.99 | 16.54 |
| two, off | 11.42 | 50.61 | 88.2 | 10.38 | 12.60 |

On the video the columns cost 0.1% of the bytes and nothing of the
picture, and the filter is a quarter of one thread's cycles, 2.1% to 2.3%
of the bytes and 0.3 dB, 0.5 at 2560×1600. Four columns without the filter
would make a 1440×900 or 1920×1080 video frame 23% to 24% faster on four
threads than the two columns with it that `screen-vp9` codes there now, as
from 2048 wide; what the quiet terminal at those sizes gives for it, a
light filter left off text at one sample to the pixel, has not been looked
at ([remaining.md](remaining.md)).

## The loop filter's edges two at a time, on a Mac and on x86

The edges of a 1920×1080 video frame, counted natively over its 120 frames:
some 63,000 a direction a frame over the three planes, of which 61% come to
the narrow filter, 16% to the 7-tap one, 18% have the same samples across
them and leave early, and few take the 15-tap filter. The narrow filter is
arithmetic in bytes, so sixteen positions in a vector cost what the eight of
one edge did, and libvpx's `_dual` kernels take two edges of the same width
at once, each with its own limits: two side by side along a horizontal edge,
which 96% of the horizontal edges have, and two one above the other along a
vertical one, the walk taking two rows of blocks at a time, which three
quarters of the vertical ones have. The module now does the same for the
narrow and the 8-sample filters, the 16-sample edges staying one at a time;
of two edges, one whose samples are the same across it is left and the other
filtered alone, so an edge that would have left early still does. Each
kernel is a function of its own: inlined into the walk, as they were, the
terminal took 5% more cycles under Bun on x86 for the same video.

`bench/run.sh` under both engines, M cycles a frame, before and after. On an
M2 Max, the target, five rounds on a Mac in use; Bun's is Safari's
JavaScriptCore and Node's Chrome's V8:

| M2 Max | Bun, one thread | Node | Bun, four threads | Node |
|---|---|---|---|---|
| terminal 1440×900 | 3.2, 3.3 | 1.9, 1.9 | 4.6, 4.6 | 3.2, 2.5 |
| video 1280×800 | 21.0, 19.9 | 19.7, 18.1 | 22.4, 21.3 | 22.1, 20.4 |
| video 1920×1080 | 32.7, 30.6 | 31.5, 28.9 | 34.5, 32.7 | 34.3, 32.7 |
| video 2560×1600, no filter | 42.0, 42.0 | 40.0, 40.0 | 46.2, 44.4 | 43.3, 44.4 |
| video 3840×2160, no filter | 68.1, 68.2 | 65.6, 65.6 | 71.3, 71.2 | 68.9, 69.2 |
| flood 1920×1080 | 360, 350 | 395, 383 | 367, 359 | 404, 390 |

On the six-core x86 workstation, three rounds:

| x86 | Bun, one thread | Node | Bun, four threads | Node |
|---|---|---|---|---|
| terminal 1440×900 | 6.1, 5.9 | 3.1, 3.0 | 8.4, 8.0 | 4.2, 4.2 |
| video 1280×800 | 43.3, 43.7 | 34.6, 33.8 | 41.9, 42.5 | 35.9, 34.9 |
| video 1920×1080 | 67.0, 68.7 | 57.3, 55.9 | 68.1, 68.5 | 60.2, 58.4 |
| flood 1920×1080 | 578, 576 | 588, 576 | 593, 590 | 598, 590 |

A filtered video frame takes 5% to 8% fewer cycles on the Mac under either
engine, and the flood 3%; the unfiltered samples and the terminal are the
same within what a Mac in use gives. On x86 V8 gains 2% to 3% and
JavaScriptCore loses up to 3%: on x86 it makes each shuffle of two vectors
that is not of 64-bit halves two `pshufb` and an `or`, where V8 makes it one
`punpck` and JavaScriptCore on ARM one `zip` or `tbl`, and the vertical
pairs take eight shuffles of two vectors more than two single edges, to
turn sixteen rows rather than eight. Disassembled from its jitdump, the
vertical pair's code under Bun on x86 is 86 `vpshufb` and 57 `vorps`.

On x86, under Node, the pairs of one direction alone gave none of it: the
horizontal ones alone 0.8% fewer cycles on the 1920×1080 video, the vertical
ones alone 0.8%, both 2.7%. Loads blocked on a store not yet written
(`ld_blocks.store_forward`), the next edge's rows read across what the last
one wrote, were 25 M over the video's 120 frames before and 8 M after, where
libvpx's are 6 M; the horizontal pairs alone take them off as well, so they
were not what the frame waited on.

## On the target: an M2 Max against libvpx

The module of 0.0.11 on an M2 Max (eight performance cores and four
efficiency ones, macOS 26.7.1) under Bun 1.4.2, whose JavaScriptCore is
Safari's, and Node 24.10, whose V8 is Chrome's, against ffmpeg's libvpx
1.16.0 as Homebrew builds it, which on the M2 runs its NEON kernels and,
for prediction, its `i8mm` ones, whose dot products of bytes WebAssembly
has no equal of. `bench/run.sh`, five rounds on a Mac in use, M cycles a
frame on one thread and mean ms on four, module under Bun and Node and
libvpx:

| | M cycles, Bun | Node | libvpx | ms on four, Bun | Node | libvpx |
|---|---|---|---|---|---|---|
| terminal 1440×900 | 3.2 | 1.9 | 1.1 | 0.25 | 0.23 | 0.24 |
| video 1280×800 | 19.8 | 18.1 | 14.1 | 2.97 | 3.09 | 3.53 |
| video 1920×1080 | 30.5 | 28.7 | 23.2 | 2.83 | 3.04 | 3.94 |
| video 2560×1600, no filter | 42.0 | 40.0 | 34.7 | 3.79 | 3.99 | 4.77 |
| video 3840×2160, no filter | 68.1 | 65.5 | 56.7 | 5.94 | 6.11 | 7.95 |
| walk 3840×2160, no filter | 51.7 | 49.5 | 41.0 | 4.67 | 4.70 | 5.76 |
| browser 3840×2160, no filter | 51.2 | 55.1 | 41.7 | 4.60 | 5.18 | 4.57 |
| flood 1920×1080 | 348 | 381 | 360 | 45.5 | 50.8 | 53.7 |

On one thread the module takes a sixth to two fifths more cycles than
libvpx on the video, the walk and the browser, a little more under Bun than
under Node but on the browser, where Node takes 8% more than Bun; on the
flood it is level. The terminal's are the engine still starting, as the
scenarios above found on x86. On four threads the module is faster than
libvpx on every video and the walk, by a sixth to a quarter, level on the
browser under Bun and an eighth behind under Node.

What each part costs, the module's as the cycles a frame a build saves
that leaves the part out, on one thread, Bun and Node, and libvpx's as its
share of the decoding thread's samples under macOS's `sample` times its
cycles above, the libvpx of Homebrew keeping the names of its functions.
Leaving a part out of a filtered stream changes what the loop filter does,
so the transforms and prediction at 1920×1080 are near and not exact; the
unfiltered 4K samples give the clean numbers. libvpx's prediction takes in
its copy of a still block, which is the module's row copy; M cycles a
frame, module under Bun and Node, then libvpx:

| | video 1920×1080 | video 3840×2160 | walk 3840×2160 | browser 3840×2160 |
|---|---|---|---|---|
| prediction from the reference | 4.3, 3.5; 2.6 | 17.5, 14.4; 12.4 | 14.9, 12.5; 11.6 | 11.1, 11.4; 8.3 |
| the row copy | 0.3, 0.3 | 0.7, 1.0 | 1.1, 1.4 | 1.0, 1.2 |
| the loop filter | 6.8, 6.6; 4.9 | | | |
| the transforms | 3.8, 3.3; 0.7 | 3.4, 2.5; 2.0 | 2.5, 1.9; 1.2 | 0.5, 0.6; 0.3 |
| intra prediction | 0.6, 0.2; 0.2 | 2.4, 1.9; 1.3 | 3.0, 2.6; 1.7 | 4.5, 4.6; 2.8 |
| libvpx's tokens | 11.3 | 26.5 | 16.2 | 13.8 |
| libvpx's modes | 3.1 | 9.6 | 9.2 | 13.0 |

So on the target the most the module takes more is its prediction from the
reference and the copy beside it: 3 to 6 M cycles of a 4K video frame, 2
to 4 of the walk's and the browser's, and 3 M more under Bun than under
Node on the video, where the same module's kernels are compiled by the two
engines for the same ARM. Then the loop filter under 2048 wide, 1.8 M more
than libvpx's at 1920×1080; intra prediction, 1.1 to 1.8 M more at 4K; and
the transforms, 0.5 to 1.4. Measured again below, the parse, tokens and
modes together, is not part of the gap: it is level with libvpx's or less.

## On the target again: where the time goes, and four threads

Measured on 2026-10-15 on the same M2 Max, macOS idle but for the runs,
Bun 1.4.2 and Node 24.18 against ffmpeg 9.0.1's libvpx-vp9, by
`bench/run.sh`: the base, release 0.0.11, against a copy of itself was 0.0%
to 0.6% apart over five rounds, which is the noise. One thread, M cycles a
frame, the base under Bun, Node and libvpx:

| | Bun | Node | libvpx |
|---|---|---|---|
| terminal 1440×900 | 3.2 | 1.9 | 1.1 |
| terminal 3840×2160 | 4.7 | 3.3 | 9.8 |
| video 1280×800 | 19.7 | 17.8 | 14.0 |
| video 1920×1080 | 30.3 | 28.5 | 23.1 |
| video 2560×1600, no filter | 41.7 | 39.6 | 34.3 |
| video 3840×2160, no filter | 67.2 | 64.5 | 56.0 |
| walk 1920×1080 | 23.4 | 21.5 | 16.1 |
| walk 3840×2160, no filter | 51.1 | 48.8 | 40.6 |
| browser 3840×2160, no filter | 50.7 | 54.6 | 41.2 |
| flood 1920×1080 | 348.8 | 381.1 | 360.1 |

What each part costs, as the cycles a frame a build saves that leaves it
out (which changes the picture, so the costs are near and not exact), and
libvpx's as its share of the decoding thread under macOS's `sample` times
its cycles. "The rest" is the frame less reconstruction and the loop filter:
the parse, tokens and modes, and the frame's own work.

| M cycles | video 1920×1080, Bun / Node / libvpx | video 3840×2160, Bun / Node / libvpx | walk 3840×2160, Bun | browser 3840×2160, Bun |
|---|---|---|---|---|
| the frame | 30.3 / 28.5 / 23.1 | 67.4 / 64.8 / 56.0 | 50.9 | 50.6 |
| prediction from the reference, with its copies | 4.3 / 3.5 / 2.0 | 17.8 / 14.7 / 11.4 | 15.0 | 11.6 |
| the row copy | 0.6 / 0.5 | 1.5 / 1.6 | 1.4 | 1.8 |
| the loop filter | 6.7 / 6.5 / 3.8 | | | |
| of it, the masks | 0.8 / 0.7 | | | |
| the transforms | 4.3 / 3.7 / 0.65 | 4.8 / 3.9 / 1.7 | 3.2 | 0.8 |
| intra prediction, with its edges | 0.7 / 0.2 / 0.2 | 2.8 / 2.8 / 1.2 | 3.1 | 5.2 |
| all of reconstruction | 10.9 / 9.3 | 30.0 / 26.5 | 25.6 | 21.6 |
| the rest | 12.7 / 12.7 / 16.4 | 37.4 / 38.3 / 41.6 | 25.3 | 29.0 |

The `ModeInfo` written twice into the grid adds 0.2 M cycles at 1920×1080
and 0.8 to 1.4 at 4K, which is what writing it once costs. On the 4K video
libvpx's tokens alone are 28.4 M of its 41.6.

So on one thread the whole of the gap to libvpx is reconstruction and the
loop filter, and the parse is level with libvpx's or less. On four threads,
as the page decodes, it is the other way: a frame waits on its tile
columns' parsing. Under Bun, leaving all of reconstruction out takes 6% to
9% off a frame's time on four threads (1280×800 2.94 ms to 2.68, 1920×1080
2.82 to 2.65, 3840×2160 5.96 to 5.57) and leaving the loop filter out
nothing; profiled, the two threads that parse the 1920×1080 video's two
columns are busy nine tenths of the time and the two that reconstruct and
filter three quarters.

**A profile of the module under each engine.** JavaScriptCore writes a
jitdump that `samply` reads on macOS, and V8 a perf map:

```sh
BUN_JSC_useJITDump=1 BUN_JSC_jitDumpDirectory=tmp/prof VP9_WASM_DIR=build/out \
  samply record --save-only -o tmp/prof/bun.json.gz -- bun bench/decode.ts FILE 1
VP9_WASM_DIR=build/out samply record --save-only -o tmp/prof/node.json.gz -- \
  node --perf-basic-prof bench/decode.ts FILE 1
```

and `BUN_JSC_useConcurrentJIT=0 BUN_JSC_dumpOMGDisassembly=1` prints each
function's B3 and Air as JavaScriptCore's top tier compiles it, Air being
ARM's instructions but for the registers' last assignment. Samples a
function took on the 4K video, Bun against Node, at the same rate, about
3,050 a run:

| function | Bun | Node |
|---|---|---|
| `decode_coefs` of a 4×4 | 501 | 513 |
| `convolve::simd128::both::<16>` | 327 | 267 |
| `Tile::tokens` of a 4×4 | 174 | 213 |
| `Recon::block` | 156 | 138 |
| `itx::inverse_add` | 81 | 64 |
| `BoolDecoder::tree` | 78 | 115 |
| `Tile::mv_component` | 57 | 29 |
| `Recon::predict_inter_block` | 42 | 26 |

Bun runs fewer instructions than Node, 237 M a 4K video frame against
255 M, and takes more cycles: JavaScriptCore's code issues 3.5 a cycle,
V8's 3.9. The largest difference is the 2-D filter, whose every tap
JavaScriptCore compiles as a multiply and an add where V8 makes one `mla`:
`both::<16>` at 16×16 in cache is 140 ns a call under Bun and 112 under
Node. JavaScriptCore does fuse `i16x8.add` with `i16x8.extmul_*_i8x16_s`
into one `smlal`, but LLVM does not leave the `extmul` of a hoisted tap
([tried.md](tried.md)).

**Threads.** Mean ms a frame of the base, Bun / Node:

| | 1 | 2 | 4 | 6 | 8 |
|---|---|---|---|---|---|
| video 1920×1080 | 7.96 / 7.95 | 4.40 / 4.21 | 2.83 / 3.04 | 2.92 / 3.05 | 3.03 / 3.20 |
| video 2560×1600 | 11.37 / 11.19 | 6.01 / 5.87 | 3.80 / 3.97 | 3.88 / 4.03 | 4.00 / 4.10 |
| video 3840×2160 | 18.91 / 18.66 | 9.96 / 9.67 | 5.97 / 6.20 | 5.95 / 6.18 | 6.13 / 6.22 |
| browser 3840×2160 | 14.23 / 15.65 | 7.79 / 8.50 | 4.65 / 5.33 | 3.91 / 4.45 | 4.08 / 4.33 |

Past four threads the video gains nothing, its four columns parsed; the
browser gains 16% at six. Eight are a little slower than six on every
sample.

**What an iPhone or iPad may do differently.** JavaScriptCore told to check
every access against the memory's bounds (`BUN_JSC_useWasmFastMemory=0`),
as iOS may for a shared memory: 11% more cycles on one thread (the 4K video
67.6 M to 75.0, the 1920×1080 30.4 to 33.8) for 50% more instructions, 7%
to 10% on four threads. The process clamped to utility QoS
(`taskpolicy -c utility`) ran on the performance cores as before, and to
background QoS 6.5 times slower, which throttles the clock as well, so
neither stands in for an iPhone's efficiency cores.

**What was kept.** Intra prediction a function of its own, the transforms'
kernels functions of their own, an intra block's edges gathered without a
`memory.copy` or `memory.fill` of a length the compiler does not know, a
tree's symbol read inlined, the modes counted only in a frame that adapts,
and `ModeInfo` in 16 bytes for 34: the six together against the base, five
rounds (`bench/run.sh`), M cycles a frame on one thread and mean ms on four,
base → now:

| | Bun, one thread | Node, one thread | Bun, four threads | Node, four threads | libvpx, one / four |
|---|---|---|---|---|---|
| terminal 1440×900 | 3.2 → 2.6 | 1.8 → 1.8 | 0.26 → 0.23 | 0.23 → 0.20 | 1.1 / 0.24 |
| video 1280×800 | 19.8 → 18.9 | 17.8 → 17.6 | 3.00 → 2.94 | 3.13 → 3.06 | 14.1 / 3.61 |
| video 1920×1080 | 30.3 → 29.2 | 28.5 → 28.0 | 2.86 → 2.80 | 2.97 → 2.91 | 23.0 / 4.06 |
| video 2560×1600, no filter | 41.8 → 40.3 | 39.7 → 38.6 | 3.90 → 3.85 | 4.04 → 3.95 | 34.3 / 4.87 |
| video 3840×2160, no filter | 67.4 → 65.1 | 64.7 → 62.5 | 6.11 → 6.06 | 6.33 → 6.23 | 56.2 / 8.11 |
| walk 1920×1080, all 174 frames | 20.2 → 19.5 | 19.0 → 18.5 | 1.80 → 1.77 | 1.91 → 1.82 | 13.7 / 2.31 |
| walk 3840×2160, all 174 frames, no filter | 44.6 → 43.0 | 43.1 → 41.2 | 4.08 → 3.98 | 4.02 → 3.93 | 35.3 / 4.71 |
| browser 3840×2160, no filter | 50.6 → 48.0 | 54.4 → 50.4 | 4.85 → 4.61 | 5.12 → 4.92 | 41.2 / 4.74 |
| flood 1920×1080 | 354.2 → 347.9 | 389.7 → 380.9 | 46.08 → 45.91 | 51.52 → 51.24 | 366.2 / 54.71 |

Under Bun a fifth fewer cycles on the quiet terminal, which is the engine's
tier-up of smaller functions, and 3.4% to 5.1% on every busy sample but the
flood's 1.8%; under Node up to 7.4%. On four threads, which wait on the
parse, 1% to 5% faster. Nothing is slower. The walk's rows are of its
settled sample, measured again on 2026-10-15, Node's on one thread from
seven rounds: five, at a load of 8.7, had them level, and the same two
builds on the walk down alone had 51.1 → 48.6 under Bun and 48.8 → 46.1
under Node at 4K.

**The upload a quiet frame saves.** Three R8 planes of 3840×2160 uploaded
with `texSubImage2D` from a buffer of the planes, in Playwright's WebKit
and Chromium on the same Mac, ms a frame with `finish`: the whole picture
1.6 in WebKit and 1.3 in Chromium; a 128×128 rectangle of each plane 0.13
and 0.01 given by the offset of its first sample, but 0.50 in WebKit given
by `UNPACK_SKIP_PIXELS` and `UNPACK_SKIP_ROWS`.

**The same module in Safari.** `bench/page/serve.ts` on the same M2 Max,
the base against what was kept, three rounds, one thread, mean ms a frame
after the first: Safari 27.0 on the Mac and Safari 26.2 in the iOS
Simulator, which runs on the Mac's cores, beside Bun's from `bench/run.sh`
(five rounds).

| | Bun | Safari 27.0 | iOS Simulator's Safari 26.2 |
|---|---|---|---|
| terminal 1440×900 | 0.27 → 0.25 | 0.27 → 0.25 | 0.30 → 0.27 |
| video 1280×800 | 4.82 → 4.71 | 4.90 → 4.80 | 5.08 → 4.92 |
| video 1920×1080 | 7.95 → 7.76 | 7.99 → 7.82 | 8.31 → 8.14 |

The three decode within a few percent of one another and agree on what the
change gains, so Bun measures what Safari runs. An iPhone's Safari 26.6.1
takes the same WebAssembly features as the Simulator's
([remaining.md](remaining.md)).
