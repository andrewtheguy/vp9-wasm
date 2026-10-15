// The page of bench/page/serve.ts: asks the server for the next run, decodes it in a worker as remotex's page does,
// sends the result back, and reloads, so that each run starts with an engine that has compiled nothing yet.
const status = document.getElementById("status");
const say = (text) => (status.textContent = text);

// The server's session, which a page left open from an earlier one does not have.
const session = new URLSearchParams(location.search).get("session") ?? "";
const next = await (await fetch(`/next?session=${session}`)).json();
if (next.done) {
  say(`done: ${next.runs} runs`);
} else {
  say(`run ${next.index + 1} of ${next.of}: build ${next.build}, ${next.sample}, ${next.threads} thread(s)`);
  const worker = new Worker("decode.js", { type: "module" });
  const result = await new Promise((resolve) => {
    worker.onmessage = ({ data }) => (data.progress ? say(`${status.textContent.split("\n")[0]}\n${data.progress}`) : resolve(data));
    worker.onerror = (ev) => resolve({ error: ev.message || "the decode worker failed" });
    worker.postMessage(next);
    setTimeout(() => resolve({ error: "no result in 600 s" }), 600_000);
  });
  worker.terminate();
  await fetch(`/result?session=${session}`, {
    method: "POST",
    body: JSON.stringify({
      ...result,
      index: next.index,
      userAgent: navigator.userAgent,
      isolated: crossOriginIsolated,
      cores: navigator.hardwareConcurrency,
    }),
  });
  location.reload();
}
