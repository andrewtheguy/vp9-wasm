// Regenerates test/data from fixtures.ts's SPECS: each stream encoded by the
// host's ffmpeg with libvpx, or written by test/screen-vp9 where its spec says
// so, and libvpx's decode of it as the reference. Run when a spec changes, and
// commit what it writes.
//
//   bun run fixtures [NAME...]
//
// Named, it writes those streams alone and keeps the rest and the reference's
// ffmpeg line: for a stream added or changed on a host whose ffmpeg is not the
// one the rest were coded by. The reference decode is the same from any.

import { mkdirSync, readdirSync, rmSync } from "node:fs";
import { join } from "node:path";

import { DATA_DIR, type FixtureSpec, REFERENCE, type Reference, type ReferenceFile, SCREEN, SPECS, streamPath } from "./fixtures";

async function run(cmd: string[]): Promise<string> {
  const proc = Bun.spawn(cmd, { stdout: "pipe", stderr: "pipe" });
  const [stdout, stderr, code] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
    proc.exited,
  ]);
  if (code !== 0) throw new Error(`${cmd.join(" ")} exited ${code}\n${stderr}`);
  return stdout;
}

const SCREEN_VP9 = join(import.meta.dir, "screen-vp9");

async function make(name: string, spec: FixtureSpec): Promise<Reference> {
  const path = streamPath(name);
  if (spec.screenVp9) {
    await run(["cargo", "run", "--release", "--quiet", "--manifest-path", join(SCREEN_VP9, "Cargo.toml"), "--", path, spec.size, String(spec.screenVp9.threads)]);
  } else {
    await run([
      "ffmpeg", "-hide_banner", "-loglevel", "error", "-y",
      "-f", "lavfi", "-i", spec.source ?? `testsrc2=size=${spec.size}:rate=30`,
      "-frames:v", String(spec.frames ?? 12),
      "-pix_fmt", spec.pixFmt,
      "-c:v", "libvpx-vp9", "-threads", "2", ...(spec.encoder ?? SCREEN), ...(spec.extra ?? []),
      "-fflags", "+bitexact", "-flags:v", "+bitexact", "-f", "ivf", path,
    ]);
  }
  const probe = JSON.parse(
    await run(["ffprobe", "-v", "error", "-show_entries", "stream=width,height,pix_fmt", "-of", "json", path]),
  ) as { streams: { width: number; height: number; pix_fmt: string }[] };
  const stream = probe.streams[0]!;
  const framemd5 = await run(["ffmpeg", "-hide_banner", "-c:v", "libvpx-vp9", "-i", path, "-fps_mode", "passthrough", "-f", "framemd5", "-"]);
  const md5s = framemd5
    .split("\n")
    .filter((line) => line && !line.startsWith("#"))
    .map((line) => line.split(",").at(-1)!.trim());
  const bytes = new Uint8Array(await Bun.file(path).arrayBuffer());
  return {
    sha256: new Bun.CryptoHasher("sha256").update(bytes).digest("hex"),
    width: stream.width,
    height: stream.height,
    pixFmt: stream.pix_fmt,
    md5s,
  };
}

if (!Bun.which("ffmpeg") || !Bun.which("ffprobe")) {
  throw new Error("generating the fixtures takes ffmpeg with libvpx");
}
const only = process.argv.slice(2);
for (const name of only) {
  if (!(name in SPECS)) throw new Error(`no fixture ${name} in test/fixtures.ts`);
}
const specs: [string, FixtureSpec][] = Object.entries(SPECS).filter(([name]) => only.length === 0 || only.includes(name));
if (specs.some(([, spec]) => spec.screenVp9) && !Bun.which("cargo")) {
  throw new Error("generating the screen-vp9 fixtures takes cargo");
}
mkdirSync(DATA_DIR, { recursive: true });
let reference: ReferenceFile;
if (only.length === 0) {
  for (const file of readdirSync(DATA_DIR)) rmSync(join(DATA_DIR, file));
  reference = { ffmpeg: (await run(["ffmpeg", "-version"])).split("\n")[0]!, fixtures: {} };
} else {
  reference = (await Bun.file(REFERENCE).json()) as ReferenceFile;
}

const references = await Promise.all(specs.map(async ([name, spec]) => [name, await make(name, spec)] as const));
for (const [name, ref] of references) reference.fixtures[name] = ref;
// In the specs' order, whichever were written.
const ordered: Record<string, Reference> = {};
for (const name of Object.keys(SPECS)) {
  const ref = reference.fixtures[name];
  if (ref) ordered[name] = ref;
}
reference.fixtures = ordered;
await Bun.write(REFERENCE, `${JSON.stringify(reference, null, 2)}\n`);
console.log(`wrote ${references.length} streams and reference.json to ${DATA_DIR}`);
