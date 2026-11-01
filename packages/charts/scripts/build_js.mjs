import { build } from "esbuild";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));

const shared = {
  bundle: true,
  format: "esm",
  minify: true,
  sourcemap: true,
  logLevel: "warning",
};
const core = { ...shared, loader: { ".svg": "text", ".png": "dataurl" } };
const react = { ...shared, external: ["react"] };

// The complete entries reuse every package source unchanged; only the generated WebAssembly glue
// and, for React, the sibling core module differ.
const complete_glue = {
  name: "aeris-complete-glue",
  setup(context) {
    context.onResolve({ filter: /\/pkg\/aeris_charts_wasm\.js$/ }, () => ({
      path: join(root, "pkg_complete", "aeris_charts_wasm_complete.js"),
    }));
  },
};
const core_module = (target) => ({
  name: `aeris-react-core-${target}`,
  setup(context) {
    context.onResolve({ filter: /^\.\/index\.js$/ }, () => ({ path: target, external: true }));
  },
});

await Promise.all([
  build({ ...core, entryPoints: [join(root, "src", "index.ts")], outfile: join(root, "dist", "index.js") }),
  build({
    ...core,
    entryPoints: [join(root, "src", "complete.ts")],
    outfile: join(root, "dist", "complete.js"),
    plugins: [complete_glue],
  }),
  build({
    ...react,
    entryPoints: [join(root, "src", "react.ts")],
    outfile: join(root, "dist", "react.js"),
    plugins: [core_module("./index.js")],
  }),
  build({
    ...react,
    entryPoints: [join(root, "src", "complete_react.ts")],
    outfile: join(root, "dist", "complete_react.js"),
    plugins: [core_module("./complete.js")],
  }),
]);
