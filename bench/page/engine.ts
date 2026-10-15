// bun bench/page/engine.ts [PORT]: serves the engine page (engine.js), which shows what the browser it is opened in
// says of itself and what its WebAssembly takes, cross-origin isolated as a shared memory needs. Port 8644 by default.
import { join } from "node:path";

const port = Number(process.argv[2] ?? 8644);
const headers = { "Cross-Origin-Opener-Policy": "same-origin", "Cross-Origin-Embedder-Policy": "require-corp", "Cache-Control": "no-store" };
const page = `<!doctype html>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Engine</title>
<pre id="out" style="white-space: pre-wrap; font-size: 12px">Checking…</pre>
<script type="module" src="engine.js"></script>
`;

Bun.serve({
  port,
  fetch(req) {
    const name = new URL(req.url).pathname.slice(1);
    if (name === "") return new Response(page, { headers: { ...headers, "Content-Type": "text/html" } });
    if (name === "engine.js" || name === "engine-worker.js") {
      return new Response(Bun.file(join(import.meta.dir, name)), { headers: { ...headers, "Content-Type": "text/javascript" } });
    }
    return new Response("not found", { status: 404 });
  },
});
console.log(`http://localhost:${port}/`);
