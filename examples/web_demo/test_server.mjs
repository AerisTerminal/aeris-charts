import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";
import { extname, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL(".", import.meta.url)));
const port = Number.parseInt(process.env.NUCLEUSCHARTS_TEST_PORT ?? "4174", 10);
const mime_types = new Map([
  [".html", "text/html; charset=utf-8"],
  [".css", "text/css; charset=utf-8"],
  [".js", "text/javascript; charset=utf-8"],
  [".mjs", "text/javascript; charset=utf-8"],
  [".map", "application/json; charset=utf-8"],
  [".json", "application/json; charset=utf-8"],
  [".wasm", "application/wasm"],
]);

const server = createServer(async (request, response) => {
  try {
    const url = new URL(request.url ?? "/", `http://${request.headers.host ?? "127.0.0.1"}`);
    if (url.pathname === "/favicon.ico") {
      response.writeHead(204).end();
      return;
    }
    const relative = decodeURIComponent(url.pathname === "/" ? "/index.html" : url.pathname);
    const filename = resolve(root, `.${relative}`);
    if (filename !== root && !filename.startsWith(`${root}${sep}`)) {
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

server.listen(port, "127.0.0.1", () => {
  console.log(`Nucleus browser-test server listening on http://127.0.0.1:${port}`);
});

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => server.close(() => process.exit(0)));
}
