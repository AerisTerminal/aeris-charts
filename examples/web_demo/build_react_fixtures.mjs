// Bundles the React fixture once per package build. The complete fixture resolves the same
// fixture source against `complete_react.js` and `complete.js`, so the complete-build server can
// run the React specs on the complete WebAssembly module (see test_server.mjs).
import { build } from "esbuild";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL(".", import.meta.url));
const package_dist = resolve(root, "..", "..", "packages", "charts", "dist");

const complete_entries = {
  name: "complete-package-entries",
  setup(context) {
    context.onResolve({ filter: /(?:^|[\\/])(?:index|react)\.js$/ }, (args) => {
      const target = resolve(args.resolveDir, args.path);
      if (target === join(package_dist, "index.js")) return { path: join(package_dist, "complete.js") };
      if (target === join(package_dist, "react.js")) return { path: join(package_dist, "complete_react.js") };
      return undefined;
    });
  },
};

const shared = {
  entryPoints: [join(root, "react_phase3_fixture.tsx")],
  bundle: true,
  format: "esm",
  jsx: "automatic",
  alias: { react: "./node_modules/react", "react-dom": "./node_modules/react-dom" },
  absWorkingDir: root,
  logLevel: "warning",
};

await Promise.all([
  build({ ...shared, outfile: join(root, "dist", "react_phase3_fixture.js") }),
  build({ ...shared, outfile: join(root, "dist", "react_phase3_fixture_complete.js"), plugins: [complete_entries] }),
]);
