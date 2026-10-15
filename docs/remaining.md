# What remains

What the decoder does not do yet, and what has not been measured. What it
does and how it is built are in the [README](../README.md), the numbers
behind this list in [measurements.md](measurements.md), and what was built
and found no faster in [tried.md](tried.md).

## Speed

Judged on the terminal and the video of wlshare's captures and on its other
scenarios, as `screen-vp9` 0.0.18 codes them, on an M2 Max, the target,
under Bun and Node, Safari's engine and Chrome's, measured again on
2026-10-15 ([measurements.md](measurements.md)): each part's cost by a build
that leaves it out, beside libvpx's share under macOS's `sample`, and each
function's under `samply` with the engines' JIT dumps. On one thread the
module takes a fifth to two fifths more cycles than libvpx on the video,
the walk and the browser, and the whole of that is reconstruction and the
loop filter: the parse, tokens and modes together, is level with libvpx's
or faster (37.4 M cycles of a 4K video frame against 41.6). In the order
the work would go, the largest first:

- **Prediction from the reference.** 17.5 M cycles of a 4K video frame
  under Bun and 14.7 under Node, where libvpx's is 11.4; 4.3 against 2.0
  at 1920×1080. The 2-D filter at 16 wide alone is a seventh of a 4K
  frame's time under Bun, and 22% more under Bun than under Node: in
  JavaScriptCore's ARM code every tap is a multiply and an add, where V8
  makes one `mla`. JavaScriptCore does make one `smlal` of an add and an
  `extmul` of bytes, which LLVM undoes for a hoisted tap
  ([tried.md](tried.md)); a JavaScriptCore that fused `i16x8.add` with
  `i16x8.mul` would give it for nothing, and is worth raising with WebKit.
- **The transforms.** 4.8 M cycles of the 4K video frame under Bun where
  libvpx's are 1.7: some 100,000 transform blocks a frame, 54,000 of them a
  DC alone and most of the rest 4×4 with a few coefficients, so about 48
  cycles a block against libvpx's 17, mostly what a block costs to reach
  its kernel: the walk's dispatch, the reads of its coefficients and the
  frame each call takes on the shadow stack.
- **The loop filter, under 2048 wide.** 6.7 M cycles of a 1920×1080 video
  frame under Bun and 6.5 under Node, where libvpx's is 3.8; the masks are
  0.8 of it, the walk 0.9, and the vertical kernels twice the horizontal.
- **Intra prediction.** 2.8 M cycles of the 4K video frame and 5.2 of the
  browser's, libvpx's 1.2 of the video's: the predictors themselves are
  cheap (in vectors, no faster), and what costs is each block's chain of
  gathering its edges from the block it follows, predicting and adding the
  residual. The predictors are a function of their own now and the edges
  gathered without `memory.copy`, which took 1% to 3.5% off the frame.
- **The walk over a block's transform blocks.** What reconstruction costs
  beyond its parts, about 4.6 M cycles of the 4K video frame.

On four threads, as the page decodes, every sample waits on its columns'
parsing: leaving all of reconstruction out takes 6% to 9% off a frame's
time and the loop filter nothing. So what reconstruction saves is the
CPU's time, which on an iPhone is its battery, and what would make a frame
come sooner is the parse:

- **The parse.** Level with libvpx's on one thread, and on four threads the
  time a frame takes: `decode_coefs` and `tokens` are three quarters of a
  parsing thread's time, and JavaScriptCore runs them as fast as V8 or
  faster. A tree's symbol read inlined into its caller, the modes counted
  only in a frame that adapts, and `ModeInfo` in 16 bytes for 34 took 1%
  to 3% off between them; the window to `large_token` by value, the split
  by `range - 1` and `256 - prob` from a table, and the first boolean's
  context in a local are each no faster on the M2 again
  ([tried.md](tried.md)). What is left is the boolean's chain itself, and
  `mv_component`, twice as slow under JavaScriptCore as under V8.

## Not measured

- **An iPhone or iPad's speed.** What its Safari is was asked of an iPhone
  with an A17 Pro by the engine page (`bench/page/engine.ts`) on
  2026-10-15: Safari 26.6.1, cross-origin isolated, with a shared memory,
  a worker able to start workers of its own, and a timer of 0.02 ms in the
  page and in a worker. Its WebAssembly takes what Safari's on the Mac
  takes, SIMD, threads, bulk memory, tail calls and exceptions, and no
  relaxed SIMD, multi-memory or 64-bit memory; Safari 27 on the Mac adds
  JSPI, which the module does not use. A memory one thread grows is the
  new size to an instance on another, and `memory.fill` and `memory.copy`
  past the old end work there, as on the Mac: the Simulator's Safari 26.2
  saw the old size and trapped them, and is no stand-in for that. A shared
  memory of 1 GB from the start is given. Its `VideoDecoder` refuses VP9
  profile 1 at 4:4:4, as the Mac's does, and takes AV1, which the Mac's
  does not. On the same Mac Bun, Safari 27 and the Simulator's Safari
  decode within a few percent of one another on one thread and agree on
  what a change gains ([measurements.md](measurements.md)), so the engine
  measured is the engine an iPhone runs. Not measured: the time a frame
  takes on the iPhone's own cores, and whether its JavaScriptCore checks
  each access against the memory's bounds, which on the Mac costs 11% of
  the cycles on one thread and 7% to 10% on four.
- **Threads on an iPhone or iPad.** The iPhone's Safari says it has four
  cores where the A17 Pro has six, two performance and four efficiency, so
  remotex's pool, as many threads as `navigator.hardwareConcurrency` says
  up to eight, is four there. On the M2 Max eight are a little slower than
  six on every sample, and six gain 16% over four on the browser and
  nothing on the video ([measurements.md](measurements.md)).
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
