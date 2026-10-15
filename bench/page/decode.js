// A run of bench/page: the build's module loaded on a memory of its own, its pool's threads seated as workers when
// the run has more than one (remotex's softwareDecoder.worker.ts does the same), the sample decoded frame by frame
// and each frame timed, and, where asked, each picture checked against libvpx's MD5s, outside the timing.
import { md5 } from "./md5.js";

self.onmessage = async ({ data: run }) => {
  try {
    self.postMessage(await decode(run));
  } catch (error) {
    self.postMessage({ error: error instanceof Error ? `${error.message}\n${error.stack}` : String(error) });
  }
};

async function decode(run) {
  const js = `/build/${run.build}/vp9.js`;
  const glue = await import(js);
  const { memory } = await glue.default();
  self.postMessage({ progress: `loaded, memory ${memory.buffer.byteLength >> 20} MB` });
  if (run.threads > 1) {
    await Promise.all(
      Array.from({ length: run.threads }, () => new Promise((resolve, reject) => {
        const thread = new Worker("pool.js", { type: "module" });
        thread.onmessage = ({ data }) => (data === null ? resolve() : reject(new Error(data)));
        // A thread that fails once seated would leave the decode waiting on it for ever: its error is the run's.
        thread.onerror = (ev) => {
          const message = `a pool thread failed: ${ev.message || "no message"}`;
          reject(new Error(message));
          self.postMessage({ error: message });
        };
        thread.postMessage({ js, module: glue.module(), memory });
      })),
    );
    self.postMessage({ progress: `${run.threads} threads seated` });
    glue.startPool(run.threads);
    self.postMessage({ progress: "pool started" });
  }
  const file = new Uint8Array(await (await fetch(`/sample/${run.sample}.ivf`)).arrayBuffer());
  const frames = ivfFrames(file).slice(0, run.frames || undefined);
  const md5s = run.check ? (await (await fetch(`/sample/${run.sample}.ivf.framemd5`)).text()).split("\n").filter((l) => l && !l.startsWith("#")).map((l) => l.split(",").pop().trim()) : [];
  const decoder = new glue.Decoder(run.threads);
  const times = [];
  let mismatch = -1;
  let held = 0;
  for (const [i, frame] of frames.entries()) {
    if (held) decoder.release(held);
    if (i % 10 === 0) self.postMessage({ progress: `frame ${i}` });
    const started = performance.now();
    const at = decoder.input(frame.length);
    new Uint8Array(memory.buffer, at, frame.length).set(frame);
    const shown = decoder.decode();
    times.push(performance.now() - started);
    held = 0;
    if (!shown) continue;
    const p = new Int32Array(memory.buffer, decoder.picture(), 16);
    held = p[14];
    if (run.check && mismatch < 0 && pictureMd5(memory, p) !== md5s[i]) mismatch = i;
  }
  decoder.free();
  return { frames: frames.length, times, mismatch, resolution: resolution() };
}

/** `ffmpeg -f framemd5`'s digest of a picture: its three planes, rows packed. */
function pictureMd5(memory, p) {
  const heap = new Uint8Array(memory.buffer);
  const [w, h] = [p[0], p[1]];
  const packed = new Uint8Array(w * h * 3);
  for (let plane = 0; plane < 3; plane++) {
    for (let y = 0; y < h; y++) {
      const from = p[7 + plane] + y * p[10 + plane];
      packed.set(heap.subarray(from, from + w), (plane * h + y) * w);
    }
  }
  return md5(packed);
}

/** The smallest step of `performance.now()` here, in ms. */
function resolution() {
  let least = Infinity;
  for (let i = 0; i < 50; i++) {
    const a = performance.now();
    let b = a;
    while (b === a) b = performance.now();
    least = Math.min(least, b - a);
  }
  return least;
}

function ivfFrames(file) {
  const view = new DataView(file.buffer, file.byteOffset, file.byteLength);
  const frames = [];
  for (let at = 32; at + 12 <= file.length; ) {
    const length = view.getUint32(at, true);
    frames.push(file.subarray(at + 12, at + 12 + length));
    at += 12 + length;
  }
  return frames;
}
