# Tried and no faster

What was built, measured and not kept, so that it is not tried again as it
was. Each was measured on the four earlier captures that
[measurements.md](measurements.md) names, per frame on one thread, and each
is on a part of the frame that is smaller still on the gateway's busy
frames, where two thirds of the time is token parsing.

## Most of the still blocks' copy not made

What this section measured is done another way now that `screen-vp9`
0.0.12 coded a frame told where the picture changed through libvpx's active
map: see the README. The blocks outside the change are a segment that skips
and is not loop filtered, so whether a 64×64 block came out as the reference
had it is known for nothing from the modes and the filter's own report, with
no comparison and no kernel reporting a side; and a quiet frame's rows are
nearly all held already, so the runs copied are one or two blocks and not
the fragments these captures' half-still frames gave. On the captures below,
which have the filter on every block, every block is written and the whole
row is copied as before. What follows is the record of the three ways that
were no faster.

Each row of 64×64 blocks of an inter frame starts as the last frame's rows,
copied in one sequential pass (`Recon::start_row`), so that a block with a
zero motion vector and no residual, half a screen's area, costs nothing in
reconstruction. That copy is 6.8% of the desktop capture's cycles, 2.6% of
the Mac's and 2.9% of the shader's.

hevc-wasm's second step, most of that copy not made, was built and
measured three ways and is slower each way (`tmp/logs/buffer-reuse-kernels.patch`
and `tmp/logs/buffer-reuse-settle.patch` hold two of them, on top of the row
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

## Parsing

In the token loop: the end marker
alone, which cut 1% of the instructions and no cycles; the large tokens
apart alone, which cost a call per such token and 1% of the cycles; the two
together gain 1% to 2% and are kept.

A path for the transform block with no coefficients, which most are, was
measured in two forms (`tmp/logs/tokens-empty-path.patch` holds both). `tokens` costs about 45 cycles per transform block on the
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

## The loop filter's kernels

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

libvpx's own `ss00` path for 4:4:4, whose kernels filter two rows of 8×8
blocks at once, is untried, and the pairs above say it is no gain here.
