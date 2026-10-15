// The engine page of bench/page/engine.ts: what the browser it is opened in says of itself and what its WebAssembly
// takes, without decoding anything: the version, cross-origin isolation, the features the module needs and others,
// how large a shared memory it gives, its timer, its codecs, and whether a thread's instance sees the memory another
// thread grew. Shown on the page.
const out = document.getElementById("out");
const report = {};
const show = () => (out.textContent = JSON.stringify(report, null, 2));
const attempt = (f) => {
  try {
    return f();
  } catch (error) {
    return `error: ${error instanceof Error ? error.message : String(error)}`;
  }
};

// Each a module of a few bytes that uses the feature, built by wat2wasm.
const MODULES = {
  simd: "AGFzbQEAAAABBQFgAAF7AwIBAAorASkA/QwBAAAAAgAAAAMAAAAEAAAA/QwBAAAAAgAAAAMAAAAEAAAA/a4BCw==",
  threads: "AGFzbQEAAAABBQFgAAF/AwIBAAUEAQMBAQoMAQoAQQBBAf4eAgAL",
  bulkMemory: "AGFzbQEAAAABBAFgAAADAgEABQMBAAEKFwEVAEEAQQBBCPwLAEEIQQBBCPwKAAAL",
  relaxedSimd: "AGFzbQEAAAABBQFgAAF7AwIBAAoZARcA/QwAAIA/AAAAQAAAQEAAAIBA/YECCw==",
  relaxedDot: "AGFzbQEAAAABBQFgAAF7AwIBAAorASkA/QwBAAAAAgAAAAMAAAAEAAAA/QwBAAAAAgAAAAMAAAAEAAAA/ZICCw==",
  tailCall: "AGFzbQEAAAABBQFgAAF/AwMCAAAKCwIEAEEBCwQAEgAL",
  exceptionsLegacy: "AGFzbQEAAAABBAFgAAADAgEADQMBAAAKCwEJAAZACAAHAAsL",
  multiMemory: "AGFzbQEAAAABBQFgAAF/AwIBAAUFAgABAAEKCgEIAEEAKEIBAAs=",
  memory64: "AGFzbQEAAAABBQFgAAF/AwIBAAUDAQQBCgkBBwBCACgCAAs=",
  extendedConst: "AGFzbQEAAAAGDgJ/AEEBC38AQQFBAmoL",
};
const GROW = "AGFzbQEAAAABFQRgAX8Bf2AAAX9gAn9/AGADf39/AAIOAQNlbnYDbWVtAgMB6AcDBgUAAQACAwckBQRncm93AAAEc2l6ZQABBGxvYWQAAgRmaWxsAAMEY29weQAECi4FBgAgAEAACwQAPwALBwAgACgCAAsLACAAQQcgAfwLAAsMACAAIAEgAvwKAAAL";
const bytes = (b64) => Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));

/** The smallest step of `performance.now()`, in ms. */
function timerStep() {
  let least = Infinity;
  for (let i = 0; i < 50; i++) {
    const a = performance.now();
    let b = a;
    while (b === a) b = performance.now();
    least = Math.min(least, b - a);
  }
  return least;
}

report.browser = {
  userAgent: navigator.userAgent,
  platform: navigator.platform,
  vendor: navigator.vendor,
  cores: navigator.hardwareConcurrency,
  deviceMemory: navigator.deviceMemory ?? null,
  touchPoints: navigator.maxTouchPoints,
  screen: `${screen.width}x${screen.height} @${devicePixelRatio}`,
};
report.context = {
  secure: isSecureContext,
  crossOriginIsolated,
  sharedArrayBuffer: typeof SharedArrayBuffer === "function",
  atomicsWaitAsync: typeof Atomics?.waitAsync === "function",
  timerStepMs: timerStep(),
};
show();

report.wasm = Object.fromEntries(Object.entries(MODULES).map(([name, b64]) => [name, attempt(() => WebAssembly.validate(bytes(b64)))]));
report.wasm.jspi = "Suspending" in WebAssembly;
report.wasm.exceptionsApi = typeof WebAssembly.Tag === "function";
show();

// How large a memory the engine gives: the module's own, which starts at 18 pages and may grow to 1 GB; one that
// starts at 1 GB, so that it never grows; and larger maxima. Each dropped before the next.
const PAGE = 65536;
const memory = (desc) => attempt(() => {
  const m = new WebAssembly.Memory(desc);
  return `ok, ${m.buffer.byteLength / PAGE} pages`;
});
report.memory = {
  sharedAsModule_18_to_16384: memory({ initial: 18, maximum: 16384, shared: true }),
  sharedFrom1GB_16385: memory({ initial: 16385, maximum: 16385, shared: true }),
  sharedMax2GB: memory({ initial: 18, maximum: 32768, shared: true }),
  sharedMax4GB: memory({ initial: 18, maximum: 65536, shared: true }),
  unsharedFrom1GB: memory({ initial: 16384, maximum: 16384 }),
};
show();

report.video = await (async () => {
  if (typeof VideoDecoder !== "function") return "no VideoDecoder";
  const codecs = {
    "vp9 profile 0 (4:2:0)": "vp09.00.10.08",
    "vp9 profile 1 (4:4:4), as the gateway sends": "vp09.01.10.08.03",
    "hevc main": "hvc1.1.6.L120.90",
    "h264 high": "avc1.640028",
    "av1 main": "av01.0.08M.08",
  };
  const results = {};
  for (const [name, codec] of Object.entries(codecs)) {
    results[name] = await VideoDecoder.isConfigSupported({ codec, codedWidth: 1920, codedHeight: 1080 }).then((r) => r.supported, (e) => `error: ${e.message}`);
  }
  return results;
})();
report.gpu = attempt(() => {
  const gl = new OffscreenCanvas(1, 1).getContext("webgl2");
  if (!gl) return "no webgl2";
  const info = gl.getExtension("WEBGL_debug_renderer_info");
  return {
    renderer: info ? gl.getParameter(info.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER),
    vendor: info ? gl.getParameter(info.UNMASKED_VENDOR_WEBGL) : gl.getParameter(gl.VENDOR),
    maxTextureSize: gl.getParameter(gl.MAX_TEXTURE_SIZE),
    webgpu: "gpu" in navigator,
  };
});
show();

// A shared memory grown by one thread and used by another, as the module's pool does: the Simulator's Safari 26.2
// reported the old size to an instance on another thread, and trapped its memory.fill and memory.copy past it.
report.threads = await (async () => {
  if (!crossOriginIsolated) return "not cross-origin isolated: no shared memory";
  const module = await WebAssembly.compile(bytes(GROW));
  const mem = new WebAssembly.Memory({ initial: 1, maximum: 1000, shared: true });
  const main = new WebAssembly.Instance(module, { env: { mem } }).exports;
  const worker = new Worker("engine-worker.js", { type: "module" });
  const fromWorker = await new Promise((resolve) => {
    worker.onmessage = ({ data }) => resolve(data);
    worker.onerror = (ev) => resolve({ error: ev.message || "the worker failed" });
    worker.postMessage({ module, mem });
  });
  worker.terminate();
  const t = (f) => attempt(() => (f(), "ok"));
  return {
    ...fromWorker,
    afterWorkerGrewTo11Pages: {
      sizeThisThreadSees: main.size(),
      loadPastOldEnd: t(() => main.load(5 * PAGE)),
      fillPastOldEnd: t(() => main.fill(5 * PAGE, 4096)),
      copyPastOldEnd: t(() => main.copy(6 * PAGE, 5 * PAGE, 4096)),
      bufferPages: mem.buffer.byteLength / PAGE,
    },
  };
})();
show();
