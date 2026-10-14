// One thread of the decoder's pool: an instance of the module on the memory the
// test's instance has, which takes a seat in the pool and decodes what the pool
// hands it. Answers once, before it takes its seat.

import { parentPort } from "node:worker_threads";
import type { PoolSeat } from "./module";

parentPort!.once("message", async (data: PoolSeat) => {
  try {
    const glue = await import(data.js);
    await glue.default({ module_or_path: data.module, memory: data.memory });
    parentPort!.postMessage(null);
    glue.runPoolThread();
  } catch (error) {
    parentPort!.postMessage(error instanceof Error ? error.message : String(error));
  }
});
