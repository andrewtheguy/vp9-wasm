// One thread of the decoder's pool: an instance of the module on the decode worker's memory, which takes a seat in
// the pool. Answers once, before it takes its seat.
self.onmessage = async ({ data }) => {
  try {
    const glue = await import(data.js);
    await glue.default({ module_or_path: data.module, memory: data.memory });
    self.postMessage(null);
    glue.runPoolThread();
  } catch (error) {
    self.postMessage(error instanceof Error ? error.message : String(error));
  }
};
