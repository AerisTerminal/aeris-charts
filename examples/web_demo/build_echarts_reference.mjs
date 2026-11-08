// Bundles the pinned ECharts reference page. ECharts GL 2.1.0 imports ECharts and zrender
// internals without file extensions, which the `./*` export maps of ECharts 6 and zrender 6
// resolve only with the exact file name, so those deep imports gain their `.js` extension here.
import { build } from "esbuild";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL(".", import.meta.url));

const extensionless_internals = {
  name: "echarts-gl-extensionless-internals",
  setup(context) {
    context.onResolve({ filter: /^(?:echarts|zrender)\/lib\// }, (args) =>
      args.path.endsWith(".js")
        ? undefined
        : context.resolve(`${args.path}.js`, { kind: args.kind, resolveDir: args.resolveDir }),
    );
  },
};

await build({
  entryPoints: [join(root, "echarts_reference", "index.js")],
  bundle: true,
  format: "esm",
  outfile: join(root, "dist", "echarts_reference.js"),
  absWorkingDir: root,
  logLevel: "warning",
  plugins: [extensionless_internals],
});
