import { afterAll, beforeAll, describe, expect, test } from "bun:test";

import { type Fixture, type FixtureName, loadFixtures } from "./fixtures";
import { decodeFrame, type LoadedModule, loadModule, type ModuleDecoder, type ModulePicture, pictureMd5 } from "./module";

// The pool remotex would start, at most.
const POOL = 4;

let loaded: LoadedModule;
let fixtures: Record<FixtureName, Fixture>;

beforeAll(async () => {
  [loaded, fixtures] = await Promise.all([loadModule(POOL), loadFixtures()]);
});

afterAll(() => loaded?.close());

function withDecoder<T>(threads: number, fn: (decoder: ModuleDecoder) => T): T {
  const decoder = new loaded.glue.Decoder(threads);
  try {
    return fn(decoder);
  } finally {
    decoder.free();
  }
}

/** Decodes frames as the page feeds them, reading each picture before the next frame. */
function decodeAll(decoder: ModuleDecoder, frames: Uint8Array[]): { pictures: ModulePicture[]; md5s: string[] } {
  const pictures: ModulePicture[] = [];
  const md5s: string[] = [];
  for (const frame of frames) {
    const picture = decodeFrame(loaded, decoder, frame);
    if (!picture) continue;
    pictures.push(picture);
    md5s.push(pictureMd5(loaded, picture));
  }
  return { pictures, md5s };
}

const DECODED = [
  "screen-330x194",
  "screen-352x256",
  "tiles-608x130",
  "tiles-1024x66",
  "tile-rows-544x200",
  "tile-rows-62x130",
  "still-352x256",
  "keyframes-330x194",
  "lossless-160x96",
  "thorough-330x194",
  "adapting-330x194",
] as const;

describe("a 4:4:4 stream", () => {
  for (const name of DECODED) {
    for (const threads of [1, 2, 3, POOL]) {
      test(`${name} decodes bit for bit as libvpx does, on ${threads} thread(s)`, () => {
        const fixture = fixtures[name];
        const { pictures, md5s } = withDecoder(threads, (d) => decodeAll(d, fixture.frames));
        expect(md5s).toEqual(fixture.md5s);
        expect(pictures[0]!.keyframe).toBe(true);
        expect(pictures[1]!.keyframe).toBe(false);
        for (const p of pictures) {
          expect([p.width, p.height, p.format]).toEqual([fixture.width, fixture.height, 2]);
        }
      });
    }
  }

  test("the planes are in the module's memory, rows at their stride", () => {
    const fixture = fixtures["screen-330x194"];
    withDecoder(1, (d) => {
      const picture = decodeFrame(loaded, d, fixture.frames[0]!)!;
      for (const plane of picture.planes) {
        expect(plane.stride).toBeGreaterThanOrEqual(fixture.width);
        expect(plane.address + plane.stride * fixture.height).toBeLessThanOrEqual(loaded.memory.buffer.byteLength);
      }
    });
  });

  test("the colour is the stream's: BT.601 at studio swing, or what it states", () => {
    withDecoder(1, (d) => {
      const p = decodeFrame(loaded, d, fixtures["screen-330x194"].frames[0]!)!;
      expect([p.range, p.matrix, p.primaries, p.transfer]).toEqual([1, 6, 6, 6]);
    });
    withDecoder(1, (d) => {
      const frames = fixtures["bt709-full-160x96"].frames;
      const p = decodeFrame(loaded, d, frames[0]!)!;
      expect([p.range, p.matrix, p.primaries, p.transfer]).toEqual([2, 1, 1, 1]);
      // A frame that states none has its keyframe's.
      const q = decodeFrame(loaded, d, frames[1]!)!;
      expect([q.range, q.matrix]).toEqual([2, 1]);
    });
  });

  test("a stream joined before its keyframe is refused until one arrives", () => {
    const fixture = fixtures["keyframes-330x194"];
    withDecoder(1, (d) => {
      for (const frame of fixture.frames.slice(1, 6)) {
        expect(() => decodeFrame(loaded, d, frame)).toThrow();
      }
      expect(decodeAll(d, fixture.frames.slice(6)).md5s).toEqual(fixture.md5s.slice(6));
    });
  });

  test("a superframe's frames decode in turn, and its last is the picture", () => {
    const fixture = fixtures["screen-330x194"];
    const [first, second] = [fixture.frames[0]!, fixture.frames[1]!];
    // The two frames, then an index: a marker, each frame's size in four
    // bytes, and the marker again.
    const marker = 0xc0 | (3 << 3) | 1;
    const unit = new Uint8Array(first.length + second.length + 10);
    unit.set(first);
    unit.set(second, first.length);
    const index = new DataView(unit.buffer, first.length + second.length);
    index.setUint8(0, marker);
    index.setUint32(1, first.length, true);
    index.setUint32(5, second.length, true);
    index.setUint8(9, marker);
    withDecoder(1, (d) => {
      expect(pictureMd5(loaded, decodeFrame(loaded, d, unit)!)).toBe(fixture.md5s[1]!);
      expect(decodeAll(d, fixture.frames.slice(2)).md5s).toEqual(fixture.md5s.slice(2));
    });
  });

  test("two decoders decode side by side", () => {
    const a = fixtures["screen-330x194"];
    const b = fixtures["tiles-608x130"];
    withDecoder(POOL, (da) =>
      withDecoder(1, (db) => {
        const got = { a: [] as string[], b: [] as string[] };
        for (let i = 0; i < Math.max(a.frames.length, b.frames.length); i++) {
          const pa = a.frames[i] && decodeFrame(loaded, da, a.frames[i]!);
          const pb = b.frames[i] && decodeFrame(loaded, db, b.frames[i]!);
          if (pa) got.a.push(pictureMd5(loaded, pa));
          if (pb) got.b.push(pictureMd5(loaded, pb));
        }
        expect(got.a).toEqual(a.md5s);
        expect(got.b).toEqual(b.md5s);
      }),
    );
  });
});

describe("what it refuses", () => {
  test("4:2:0, by name", () => {
    withDecoder(1, (d) => {
      expect(() => decodeFrame(loaded, d, fixtures["yuv420p-330x194"].frames[0]!)).toThrow(/4:4:4/);
    });
  });

  test("garbage, and then goes on from the next keyframe", () => {
    const fixture = fixtures["screen-330x194"];
    withDecoder(1, (d) => {
      expect(() => decodeFrame(loaded, d, new Uint8Array([1, 2, 3, 4, 5]))).toThrow(/not a VP9 frame/);
      expect(() => decodeFrame(loaded, d, new Uint8Array(0))).toThrow();
      // The second frame alone has nothing to be predicted from.
      expect(() => decodeFrame(loaded, d, fixture.frames[1]!)).toThrow();
      expect(decodeAll(d, fixture.frames).md5s).toEqual(fixture.md5s);
    });
  });
});

describe("a frame too large for the memory", () => {
  test("is refused with an error, and the decoder goes on", () => {
    const fixture = fixtures["screen-330x194"];
    withDecoder(1, (d) => {
      let thrown: unknown;
      try {
        // The module's memory grows to a gibibyte at most.
        d.input(1 << 30);
      } catch (error) {
        thrown = error;
      }
      expect(thrown).toBeInstanceOf(Error);
      expect(thrown).not.toBeInstanceOf(WebAssembly.RuntimeError);
      expect(decodeAll(d, fixture.frames).md5s).toEqual(fixture.md5s);
    });
  });
});

describe("a damaged stream", () => {
  // The module's SIMD loops run nowhere else than here: the Rust fuzz target
  // (rust/vp9/fuzz) runs their scalar counterparts. So the fixtures are
  // damaged the ways a stream gets damaged, the same ways every run, and the
  // module must throw an error rather than trap, and decode each frame alike
  // on one thread and on the pool.
  const ROUNDS = 64;

  /** xorshift32, as a draw of `0..n`. */
  function rng(seed: number): (n: number) => number {
    let s = seed >>> 0 || 1;
    return (n) => {
      s ^= s << 13;
      s >>>= 0;
      s ^= s >>> 17;
      s ^= s << 5;
      s >>>= 0;
      return s % n;
    };
  }

  /** The frames with one's bits flipped, a run of its bytes random, its end cut off, or it dropped. */
  function damage(frames: Uint8Array[], draw: (n: number) => number): Uint8Array[] {
    const out = frames.slice();
    const at = draw(out.length);
    const frame = out[at]!.slice();
    switch (draw(4)) {
      case 0:
        for (let n = draw(8) + 1; n > 0; n--) {
          const i = draw(frame.length);
          frame[i] = (frame[i] ?? 0) ^ (1 << draw(8));
        }
        out[at] = frame;
        break;
      case 1: {
        const i = draw(frame.length);
        const end = Math.min(i + draw(64) + 1, frame.length);
        for (let j = i; j < end; j++) frame[j] = draw(256);
        out[at] = frame;
        break;
      }
      case 2:
        out[at] = frame.subarray(0, draw(frame.length));
        break;
      default:
        out.splice(at, 1);
    }
    return out;
  }

  /** The picture's digest, null for none, or "error": what the two decoders must agree on. */
  function outcome(decoder: ModuleDecoder, frame: Uint8Array): string | null {
    try {
      const picture = decodeFrame(loaded, decoder, frame);
      return picture && pictureMd5(loaded, picture);
    } catch (error) {
      expect(error).not.toBeInstanceOf(WebAssembly.RuntimeError);
      return "error";
    }
  }

  for (const name of ["screen-330x194", "tiles-608x130", "thorough-330x194"] as const) {
    test(`${name} never traps, and decodes alike on one thread and ${POOL}`, () => {
      const draw = rng(name.length);
      for (let round = 0; round < ROUNDS; round++) {
        const frames = damage(fixtures[name].frames, draw);
        withDecoder(1, (one) =>
          withDecoder(POOL, (many) => {
            for (const frame of frames) {
              expect(outcome(many, frame)).toBe(outcome(one, frame));
            }
          }),
        );
      }
    });
  }
});
