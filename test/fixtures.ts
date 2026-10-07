// Synthetic VP9 streams in test/data, with libvpx's decode of each as the
// reference: every frame's MD5 from `ffmpeg -f framemd5`, which the module
// must match bit for bit. Both are committed, so a run needs no ffmpeg, and an
// encoder upgrade cannot change what is tested. `bun run fixtures`
// regenerates them from SPECS (test/generate-fixtures.ts).

import { existsSync } from "node:fs";
import { join } from "node:path";

export interface FixtureSpec {
  size: string;
  pixFmt: string;
  frames?: number;
  // A lavfi graph of the size to encode, in place of testsrc2 at that size.
  source?: string;
  // libvpx-vp9 options in place of SCREEN's.
  encoder?: string[];
  // Options after the encoder's.
  extra?: string[];
}

export interface Fixture {
  name: string;
  /** The stream's frames, as the page is sent them. */
  frames: Uint8Array[];
  width: number;
  height: number;
  pixFmt: string;
  // The reference decode's MD5 of each frame shown, in order.
  md5s: string[];
}

// What test/data/reference.json holds for each stream.
export interface Reference {
  sha256: string;
  width: number;
  height: number;
  pixFmt: string;
  md5s: string[];
}

export interface ReferenceFile {
  // `ffmpeg -version`'s first line, for the encoder and the reference decoder.
  ffmpeg: string;
  fixtures: Record<string, Reference>;
}

export const DATA_DIR = join(import.meta.dir, "data");
export const REFERENCE = join(DATA_DIR, "reference.json");

// libvpx as remotex and wlshare configure it (the `screen-vp9` crate), as near
// as ffmpeg's options come: one pass, real time, no frames held back, screen
// content, a constant quantizer, no periodic keyframe, BT.601 at studio swing.
export const SCREEN = [
  "-deadline", "realtime", "-cpu-used", "6", "-lag-in-frames", "0", "-tune-content", "screen",
  "-aq-mode", "0", "-crf", "24", "-b:v", "0", "-g", "9999", "-error-resilient", "0", "-row-mt", "1",
  "-colorspace", "smpte170m", "-color_range", "tv",
];

export const SPECS = {
  // Sizes off the 8-pixel grid, and off the 64-pixel one.
  "screen-330x194": { size: "330x194", pixFmt: "yuv444p" },
  "screen-352x256": { size: "352x256", pixFmt: "yuv444p" },
  // Two tile columns, which parse side by side, and two tile rows.
  "tiles-608x130": { size: "608x130", pixFmt: "yuv444p", extra: ["-tile-columns", "1"] },
  "tile-rows-544x200": { size: "544x200", pixFmt: "yuv444p", extra: ["-tile-columns", "1", "-tile-rows", "1"] },
  // A screen mostly still: a patch moves across one frozen picture, and the
  // rest is predicted from the frame before without a residual.
  "still-352x256": {
    size: "352x256",
    pixFmt: "yuv444p",
    frames: 18,
    source:
      "testsrc2=size=352x256:rate=30,trim=end_frame=1,loop=loop=-1:size=1[bg];testsrc2=size=44x36:rate=30[fg];[bg][fg]overlay=" +
      "x='if(lt(n,6),37+9*n,32*(floor(n/3)-1))':y='if(lt(n,6),50+5*n,64*(floor(n/3)-1))'",
  },
  // A keyframe every six frames, for a stream joined in its middle.
  "keyframes-330x194": { size: "330x194", pixFmt: "yuv444p", frames: 14, extra: ["-g", "6", "-keyint_min", "6"] },
  // The quantizer at zero: the Walsh-Hadamard transform.
  "lossless-160x96": {
    size: "160x96",
    pixFmt: "yuv444p",
    frames: 4,
    encoder: ["-deadline", "realtime", "-cpu-used", "6", "-lag-in-frames", "0", "-lossless", "1", "-g", "9999"],
  },
  // The best quality the encoder has, slowly: every transform size and
  // prediction mode it will pick with one reference frame at a time.
  "thorough-330x194": {
    size: "330x194",
    pixFmt: "yuv444p",
    frames: 8,
    encoder: ["-deadline", "good", "-cpu-used", "0", "-lag-in-frames", "0", "-auto-alt-ref", "0", "-crf", "30", "-b:v", "0", "-g", "9999"],
  },
  "bt709-full-160x96": { size: "160x96", pixFmt: "yuv444p", frames: 2, extra: ["-colorspace", "bt709", "-color_range", "pc"] },
  // What the decoder refuses: 4:2:0, which a browser decodes itself.
  "yuv420p-330x194": { size: "330x194", pixFmt: "yuv420p", frames: 4 },
} satisfies Record<string, FixtureSpec>;

export type FixtureName = keyof typeof SPECS;

export const streamPath = (name: string) => join(DATA_DIR, `${name}.ivf`);

let fixtures: Promise<Record<FixtureName, Fixture>> | null = null;

export function loadFixtures(): Promise<Record<FixtureName, Fixture>> {
  fixtures ??= (async () => {
    const { ivfFrames } = await import("./ivf");
    if (!existsSync(REFERENCE)) throw new Error(`no ${REFERENCE}: run \`bun run fixtures\``);
    const reference = (await Bun.file(REFERENCE).json()) as ReferenceFile;
    const entries = await Promise.all(
      Object.keys(SPECS).map(async (name) => {
        const ref = reference.fixtures[name];
        const path = streamPath(name);
        if (!ref || !existsSync(path)) throw new Error(`no fixture ${name}: run \`bun run fixtures\``);
        const stream = new Uint8Array(await Bun.file(path).arrayBuffer());
        // A stream regenerated without its reference, or the other way round.
        const sha256 = new Bun.CryptoHasher("sha256").update(stream).digest("hex");
        if (sha256 !== ref.sha256) throw new Error(`${path} does not match reference.json: run \`bun run fixtures\``);
        const fixture: Fixture = { name, frames: ivfFrames(stream), width: ref.width, height: ref.height, pixFmt: ref.pixFmt, md5s: ref.md5s };
        return [name, fixture] as const;
      }),
    );
    return Object.fromEntries(entries) as Record<FixtureName, Fixture>;
  })();
  return fixtures;
}
