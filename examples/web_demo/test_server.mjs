import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";
import { extname, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL(".", import.meta.url)));
const benchmark_root = resolve(root, "..", "..", "benchmarks");
const port = Number.parseInt(process.env.AERIS_CHARTS_TEST_PORT ?? "4174", 10);
// Loopback by default; set AERIS_CHARTS_TEST_HOST=0.0.0.0 to open the demo from a phone on the same network.
const host = process.env.AERIS_CHARTS_TEST_HOST ?? "127.0.0.1";
// The complete-build server answers the shared bundle path with the complete bundle, so every page
// and spec runs unchanged against `@aeristerminal/aeris-charts/complete`.
const complete_build = process.env.AERIS_CHARTS_BUILD === "complete";
const bundle_aliases = new Map(complete_build
  ? [
    ["/dist/aeris_charts_financial.js", "/dist/aeris_charts_financial_complete.js"],
    ["/dist/react_phase3_fixture.js", "/dist/react_phase3_fixture_complete.js"],
  ]
  : []);
const mime_types = new Map([
  [".html", "text/html; charset=utf-8"],
  [".css", "text/css; charset=utf-8"],
  [".js", "text/javascript; charset=utf-8"],
  [".mjs", "text/javascript; charset=utf-8"],
  [".map", "application/json; charset=utf-8"],
  [".json", "application/json; charset=utf-8"],
  [".ttf", "font/ttf"],
  [".wasm", "application/wasm"],
]);

const server = createServer(async (request, response) => {
  try {
    const url = new URL(request.url ?? "/", `http://${request.headers.host ?? "127.0.0.1"}`);
    if (url.pathname === "/favicon.ico") {
      response.writeHead(204).end();
      return;
    }
    const requested = decodeURIComponent(url.pathname === "/" ? "/index.html" : url.pathname);
    const relative = bundle_aliases.get(requested) ?? requested;
    const is_benchmark = relative.startsWith("/benchmarks/");
    const serving_root = is_benchmark ? benchmark_root : root;
    const serving_relative = is_benchmark ? relative.slice("/benchmarks".length) : relative;
    const filename = resolve(serving_root, `.${serving_relative}`);
    if (filename !== serving_root && !filename.startsWith(`${serving_root}${sep}`)) {
      response.writeHead(403).end("forbidden");
      return;
    }
    if (!(await stat(filename)).isFile()) throw new Error("not a file");
    const body = await readFile(filename);
    response.writeHead(200, {
      "cache-control": "no-store",
      "content-type": mime_types.get(extname(filename)) ?? "application/octet-stream",
      // Cross-origin isolation, which is what makes `SharedArrayBuffer` available at all — the
      // ring-source specs need it. Everything the demo loads is same-origin, so `require-corp`
      // costs nothing here. A consumer serving the engine must set the same pair to use
      // `series_api.set_ring_source`.
      "cross-origin-opener-policy": "same-origin",
      "cross-origin-embedder-policy": "require-corp",
    });
    response.end(body);
  } catch {
    response.writeHead(404).end("not found");
  }
});

server.listen(port, host, () => {
  console.log(`Aeris browser-test server (${complete_build ? "complete" : "standard"} build) listening on http://${host}:${port}`);
});

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => server.close(() => process.exit(0)));
}
