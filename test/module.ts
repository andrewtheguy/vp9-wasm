// Loads the decoder's module as released (build/out, built by ./build.sh) as
// zDesk's decode worker loads it: the module instantiated on a shared memory,
// its pool's threads started as workers that each run an instance of it, and a
// picture read from where the decoder left its planes. It runs under Bun, whose
// engine is Safari's, and under Node, whose engine is Chrome's.

import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { Worker } from "node:worker_threads";

export const OUT_DIR = resolve(process.env.VP9_WASM_DIR ?? `${import.meta.dirname}/../build/out`);

/** What rust/vp9-web/src/lib.rs exports, as wasm-bindgen's glue presents it. */
interface Glue {
  default(options: { module_or_path: Uint8Array | WebAssembly.Module; memory?: WebAssembly.Memory }): Promise<{ memory: WebAssembly.Memory }>;
  module(): WebAssembly.Module;
  startPool(threads: number): void;
  Decoder: new (threads: number) => ModuleDecoder;
}

export interface ModuleDecoder {
  input(size: number): number;
  decode(): boolean;
  picture(): number;
  /**
   * Where the last picture may differ from picture `since`, by its serial, as four numbers: left, top, width and
   * height, the whole picture where it does not come down from that one, and a width of 0 where nothing changed.
   */
  changed(since: number): number;
  release(serial: number): void;
  free(): void;
}

/** A picture as the module describes it: the planes are in `memory`. */
export interface ModulePicture {
  width: number;
  height: number;
  format: number;
  range: number;
  matrix: number;
  primaries: number;
  transfer: number;
  planes: { address: number; stride: number }[];
  keyframe: boolean;
  /** Which of the decoder's pictures it is, for `release`. */
  serial: number;
}

export interface LoadedModule {
  glue: Glue;
  memory: WebAssembly.Memory;
  /** Stops the pool's workers, which would otherwise keep the process alive. */
  close(): void;
}

/** What each pool worker is told: the glue to import, the compiled module, and the one memory. */
export interface PoolSeat {
  js: string;
  module: WebAssembly.Module;
  memory: WebAssembly.Memory;
}

/** Instantiates the module and starts a pool of `threads` workers. Once per process. */
export async function loadModule(threads: number): Promise<LoadedModule> {
  const js = `${OUT_DIR}/vp9.js`;
  const wasm = `${OUT_DIR}/vp9_bg.wasm`;
  if (!existsSync(js) || !existsSync(wasm)) {
    throw new Error(`no vp9.js and vp9_bg.wasm in ${OUT_DIR}: run ./build.sh, or set VP9_WASM_DIR`);
  }
  const glue = (await import(js)) as Glue;
  const bytes = new Uint8Array(readFileSync(wasm));
  const { memory } = await glue.default({ module_or_path: bytes });
  const seat: PoolSeat = { js, module: glue.module(), memory };
  const workers: Worker[] = [];
  try {
    await Promise.all(
      Array.from({ length: threads }, () => new Promise<void>((resolve, reject) => {
        const worker = new Worker(new URL("./pool.worker.ts", import.meta.url));
        workers.push(worker);
        worker.on("message", (data: string | null) => (data === null ? resolve() : reject(new Error(data))));
        worker.on("error", (error: Error) => reject(new Error(error.message || "a pool worker did not start")));
        worker.postMessage(seat);
      })),
    );
  } catch (error) {
    // A seat not taken: the others would keep the process alive.
    for (const worker of workers) worker.terminate();
    throw error;
  }
  glue.startPool(threads);
  return {
    glue,
    memory,
    close() {
      for (const worker of workers) worker.terminate();
    },
  };
}

// The picture each decoder last gave `decodeFrame`, which the next call releases.
const last = new WeakMap<ModuleDecoder, number>();

/**
 * Feeds `unit`, one frame of the stream, to `decoder` and reads the picture it decoded to, if any: good until the next
 * call for the decoder, which releases it, as a page done with each picture before the next would.
 */
export function decodeFrame(loaded: LoadedModule, decoder: ModuleDecoder, unit: Uint8Array): ModulePicture | null {
  const before = last.get(decoder);
  if (before !== undefined) decoder.release(before);
  last.delete(decoder);
  const picture = decodeHeld(loaded, decoder, unit);
  if (picture) last.set(decoder, picture.serial);
  return picture;
}

/** Feeds `unit` to `decoder` and reads the picture it decoded to, if any, which is held until released. */
export function decodeHeld(loaded: LoadedModule, decoder: ModuleDecoder, unit: Uint8Array): ModulePicture | null {
  const at = decoder.input(unit.length);
  new Uint8Array(loaded.memory.buffer, at, unit.length).set(unit);
  if (!decoder.decode()) return null;
  const p = new Int32Array(loaded.memory.buffer, decoder.picture(), 16);
  return {
    width: p[0]!,
    height: p[1]!,
    format: p[2]!,
    range: p[3]!,
    matrix: p[4]!,
    primaries: p[5]!,
    transfer: p[6]!,
    planes: [0, 1, 2].map((i) => ({ address: p[7 + i]!, stride: p[10 + i]! })),
    keyframe: p[13] === 1,
    serial: p[14]!,
  };
}

/** `ffmpeg -f framemd5`'s digest of the picture: its three planes, rows packed. */
export function pictureMd5(loaded: LoadedModule, picture: ModulePicture): string {
  const heap = new Uint8Array(loaded.memory.buffer);
  const hasher = createHash("md5");
  for (const plane of picture.planes) {
    for (let y = 0; y < picture.height; y++) {
      hasher.update(heap.subarray(plane.address + y * plane.stride, plane.address + y * plane.stride + picture.width));
    }
  }
  return hasher.digest("hex");
}
