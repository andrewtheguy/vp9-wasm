// Regenerates test/data from fixtures.ts's SPECS: each stream encoded by the
// host's ffmpeg with libvpx, and libvpx's decode of it as the reference. Run
// when a spec changes, and commit what it writes.
//
//   bun run fixtures

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

async function make(name: string, spec: FixtureSpec): Promise<Reference> {
  const path = streamPath(name);
  await run([
    "ffmpeg", "-hide_banner", "-loglevel", "error", "-y",
    "-f", "lavfi", "-i", spec.source ?? `testsrc2=size=${spec.size}:rate=30`,
    "-frames:v", String(spec.frames ?? 12),
    "-pix_fmt", spec.pixFmt,
    "-c:v", "libvpx-vp9", "-threads", "2", ...(spec.encoder ?? SCREEN), ...(spec.extra ?? []),
    "-fflags", "+bitexact", "-flags:v", "+bitexact", "-f", "ivf", path,
  ]);
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
mkdirSync(DATA_DIR, { recursive: true });
for (const file of readdirSync(DATA_DIR)) rmSync(join(DATA_DIR, file));

const references = await Promise.all(
  Object.entries(SPECS).map(async ([name, spec]) => [name, await make(name, spec)] as const),
);
const reference: ReferenceFile = {
  ffmpeg: (await run(["ffmpeg", "-version"])).split("\n")[0]!,
  fixtures: Object.fromEntries(references),
};
await Bun.write(REFERENCE, `${JSON.stringify(reference, null, 2)}\n`);
console.log(`wrote ${references.length} streams and reference.json to ${DATA_DIR}`);
