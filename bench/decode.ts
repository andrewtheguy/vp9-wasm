// bun bench/decode.ts FILE.ivf THREADS [FRAMES]: the module in VP9_WASM_DIR (build/out) decoding a stream, timed per frame
// by its own clock. With CHECK=1 each picture's MD5 is compared with FILE.ivf.framemd5, outside the timing.
import { ivfFrames } from "../test/ivf";
import { decodeFrame, loadModule, pictureMd5 } from "../test/module";

const [file, t = "1", limit = "100000"] = process.argv.slice(2);
if (!file) throw new Error("usage: bun bench/decode.ts FILE.ivf THREADS [FRAMES]");
const threads = Number(t);
const check = process.env.CHECK === "1";
const loaded = await loadModule(Math.max(threads, 1));
const frames = ivfFrames(new Uint8Array(await Bun.file(file).arrayBuffer())).slice(0, Number(limit));
const md5s = check ? (await Bun.file(`${file}.framemd5`).text()).split("\n").filter((l) => l && !l.startsWith("#")).map((l) => l.split(",").pop()!.trim()) : [];
const decoder = new loaded.glue.Decoder(threads);
const times: number[] = [];
let mismatch = -1;
frames.forEach((frame, i) => {
  const started = performance.now();
  const picture = decodeFrame(loaded, decoder, frame);
  times.push(performance.now() - started);
  if (check && mismatch < 0 && (!picture || pictureMd5(loaded, picture) !== md5s[i])) mismatch = i;
});
const total = times.reduce((a, b) => a + b, 0);
times.sort((a, b) => a - b);
const at = (x: number) => times[Math.floor((times.length - 1) * x)]!.toFixed(2);
console.log(`${frames.length} pictures, ${threads} threads: mean ${(total / frames.length).toFixed(2)} ms, median ${at(0.5)}, p95 ${at(0.95)}, max ${at(1)}${check ? `, first mismatch ${mismatch < 0 ? "none" : mismatch}` : ""}`);
decoder.free();
loaded.close();
if (mismatch >= 0) process.exit(1);
