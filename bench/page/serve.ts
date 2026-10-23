// bun bench/page/serve.ts BUILD...: the benchmark in a browser, Safari's engine as Safari runs it, where
// bench/run.sh runs it in Bun. Each BUILD is a directory holding a build of the module (vp9.js, vp9_bg.wasm). The
// page decodes each of bench/samples.sh's samples in a worker as zDesk's page does, its pool's threads workers of
// that worker, the builds alternating round by round, one run a page load so that each starts with an engine that has
// compiled nothing; the first round of each build checks every picture against libvpx's MD5s. The page reports each
// frame's time, and the server prints the medians over the rounds of the mean ms a frame after the first, and of the
// median frame's, then exits.
//   TARGET=sim (the iOS Simulator), safari (Safari on this Mac), or none (print the address only)
//   SIMULATOR="iPhone 17 Pro" ROUNDS=3 THREADS="1 4" FRAMES=all SAMPLES="name ..." PORT=8642
// The page is served from http://localhost, which a browser takes as secure, with the headers that make it
// cross-origin isolated, as a shared memory needs: the Simulator shares the Mac's network.
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { basename, join, resolve } from "node:path";

const root = resolve(import.meta.dir, "../..");
const builds = process.argv.slice(2).map((b) => resolve(b));
if (builds.length === 0) throw new Error("usage: bun bench/page/serve.ts BUILD...");
for (const b of builds) if (!existsSync(join(b, "vp9_bg.wasm"))) throw new Error(`no vp9_bg.wasm in ${b}`);
const samples = (process.env.SAMPLES ?? "terminal-1440x900-2col-lf video-1280x800-1col-lf video-1920x1080-2col-lf video-3840x2160-4col-nolf walk-3840x2160-4col-nolf browser-3840x2160-4col-nolf").split(/\s+/).filter(Boolean);
const sampleDir = spawnSync("bench/samples.sh", samples, { cwd: root, encoding: "utf8" }).stdout.trim();
if (!sampleDir) throw new Error("bench/samples.sh found no samples");
const threads = (process.env.THREADS ?? "1 4").split(/\s+/).map(Number);
const rounds = Number(process.env.ROUNDS ?? 3);
const frames = process.env.FRAMES && process.env.FRAMES !== "all" ? Number(process.env.FRAMES) : 0;
const port = Number(process.env.PORT ?? 8642);
// The session the opened page is of: a page left open from an earlier one is told it is done.
const session = crypto.randomUUID();
const target = process.env.TARGET ?? "sim";

interface Run { index: number; of: number; build: number; sample: string; threads: number; round: number; check: boolean; frames: number }
interface Result { index: number; frames?: number; times?: number[]; mismatch?: number; error?: string; resolution?: number; userAgent: string; isolated: boolean; cores: number }

// As bench/run.sh: by threads, then sample, then round, the builds alternating within a round.
const runs: Run[] = [];
for (const t of threads) for (const sample of samples) for (let round = 0; round < rounds; round++) for (let build = 0; build < builds.length; build++) {
  runs.push({ index: runs.length, of: 0, build, sample, threads: t, round, check: round === 0, frames });
}
for (const r of runs) r.of = runs.length;
const results: Result[] = [];
let next = 0;

const headers = { "Cross-Origin-Opener-Policy": "same-origin", "Cross-Origin-Embedder-Policy": "require-corp", "Cross-Origin-Resource-Policy": "same-origin", "Cache-Control": "no-store" };
const types: Record<string, string> = { html: "text/html", js: "text/javascript", wasm: "application/wasm", ivf: "application/octet-stream", framemd5: "text/plain" };
const file = (path: string) => {
  if (!existsSync(path)) return new Response("not found", { status: 404, headers });
  return new Response(Bun.file(path), { headers: { ...headers, "Content-Type": types[path.split(".").pop()!] ?? "application/octet-stream" } });
};

let finish: () => void;
const finished = new Promise<void>((r) => (finish = r));
const server = Bun.serve({
  port,
  async fetch(req) {
    const url = new URL(req.url);
    const path = decodeURIComponent(url.pathname);
    const ours = url.searchParams.get("session") === session;
    if (path === "/next") return Response.json(ours && next < runs.length ? runs[next] : { done: true, runs: runs.length }, { headers });
    if (path === "/result" && req.method === "POST") {
      if (!ours) return new Response("not this session", { status: 409, headers });
      const result = (await req.json()) as Result;
      if (result.index === next) {
        results.push(result);
        const run = runs[next++]!;
        const mean = result.times ? average(result.times.slice(1)) : NaN;
        console.error(`${next}/${runs.length} build ${run.build} ${run.sample} ${run.threads}t round ${run.round}: ${result.error ?? `${mean.toFixed(2)} ms${run.check ? `, first mismatch ${result.mismatch! < 0 ? "none" : result.mismatch}` : ""}`}`);
        if (result.error || (run.check && result.mismatch! >= 0)) next = runs.length;
        if (next === runs.length) setTimeout(finish, 200);
      }
      return new Response("ok", { headers });
    }
    const m = path.match(/^\/build\/(\d+)\/([\w.]+)$/);
    if (m) return file(join(builds[Number(m[1])] ?? "", m[2]!));
    const s = path.match(/^\/sample\/([\w.-]+)$/);
    if (s) return file(join(sampleDir, s[1]!));
    return file(join(import.meta.dir, path === "/" ? "index.html" : path.slice(1).replace(/\.\.+/g, "")));
  },
});
const address = `http://localhost:${server.port}/?session=${session}`;
console.error(`serving ${address}: ${runs.length} runs of ${builds.map((b) => basename(b)).join(", ")}`);

if (target === "sim") {
  const device = process.env.SIMULATOR ?? "iPhone 17 Pro";
  spawnSync("xcrun", ["simctl", "boot", device]);
  // A Safari of its own each session: its pages' memories add up over many reloads in one.
  spawnSync("xcrun", ["simctl", "terminate", device, "com.apple.mobilesafari"]);
  spawnSync("open", ["-a", "Simulator"]);
  spawnSync("xcrun", ["simctl", "openurl", device, address], { stdio: "inherit" });
} else if (target === "safari") {
  spawnSync("open", ["-a", "Safari", address], { stdio: "inherit" });
}

await finished;
server.stop(true);

function average(v: number[]) {
  return v.reduce((a, b) => a + b, 0) / Math.max(v.length, 1);
}
function median(v: number[]) {
  const s = [...v].sort((a, b) => a - b);
  return s.length ? s[(s.length - 1) >> 1]! : NaN;
}

const first = results[0];
if (first) console.log(`# ${first.userAgent}\n# cross-origin isolated ${first.isolated}, ${first.cores} cores, timer step ${first.resolution} ms`);
const failed = results.find((r) => r.error || (r.mismatch ?? -1) >= 0);
if (failed) {
  console.log(`!! run ${failed.index + 1}: ${failed.error ?? `first mismatch at frame ${failed.mismatch}`}`);
  process.exit(1);
}
for (const t of threads) for (const sample of samples) {
  console.log(`-- ${sample}, ${t} thread(s), medians of ${rounds} rounds`);
  for (let b = 0; b < builds.length; b++) {
    const mine = results.filter((r) => { const run = runs[r.index]!; return run.build === b && run.sample === sample && run.threads === t; });
    const means = mine.map((r) => average(r.times!.slice(1)));
    const medians = mine.map((r) => median(r.times!.slice(1)));
    console.log(`${basename(builds[b]!).padEnd(24)} mean ${median(means).toFixed(2).padStart(7)} ms  median ${median(medians).toFixed(2).padStart(7)} ms`);
  }
}
