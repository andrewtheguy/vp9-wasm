// The engine page's worker: its timer, whether it can start a worker of its own (remotex's decode worker starts its
// pool's threads so), and an instance on the page's shared memory that grows it, for the page's thread to use.
self.onmessage = async ({ data }) => {
  const nested = await new Promise((resolve) => {
    try {
      const inner = new Worker(new URL("engine-worker.js?inner", import.meta.url), { type: "module" });
      inner.onmessage = () => (inner.terminate(), resolve(true));
      inner.onerror = (ev) => resolve(`error: ${ev.message || "failed"}`);
      inner.postMessage({ ping: true });
    } catch (error) {
      resolve(`error: ${error.message}`);
    }
  });
  const instance = new WebAssembly.Instance(data.module, { env: { mem: data.mem } }).exports;
  instance.grow(10);
  let least = Infinity;
  for (let i = 0; i < 50; i++) {
    const a = performance.now();
    let b = a;
    while (b === a) b = performance.now();
    least = Math.min(least, b - a);
  }
  self.postMessage({ workerTimerStepMs: least, nestedWorker: nested, sizeWorkerSeesAfterGrowing: instance.size() });
};
if (location.search === "?inner") self.onmessage = () => self.postMessage(true);
