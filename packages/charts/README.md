# @origin/charts

A trading-chart engine in **Rust + WebGPU + WASM**, pixel-faithful to
the reference charting library v5, with a plain
TypeScript API. Canvas2D fallback with automatic device-loss failover included.

**Private package** — hosted on GitHub Packages (`TradeAion` org). Not on the public npm registry.

## Install

GitHub Packages requires authentication for all installs. Create a personal access token (classic)
with the `read:packages` scope (in GitHub Actions, use the built-in `GITHUB_TOKEN`).

**Bun** (primary) — add to `bunfig.toml` in your project root:

```toml
[install.scopes]
"@origin" = { token = "$GITHUB_READ_PACKAGES_TOKEN", url = "https://npm.pkg.github.com/" }
```

(Reference an env var; don't hardcode the token.) Then:

```sh
bun add @origin/charts
```

**npm** — add to your project's `.npmrc`:

```
@origin:registry=https://npm.pkg.github.com
//npm.pkg.github.com/:_authToken=${GITHUB_READ_PACKAGES_TOKEN}
```

```sh
npm install @origin/charts
```

No Rust toolchain or native build step is needed — the package ships prebuilt JS + WASM.

## Quick start

```ts
import { create_chart } from "@origin/charts";

// Async: WebGPU backend acquisition (the one deliberate divergence from the reference's sync createChart).
const chart = await create_chart(document.getElementById("chart"), {
  layout: { background: { type: "solid", color: "#ffffff" }, textColor: "#191919" },
});

const series = chart.add_series("candlestick", { up_color: "#26a69a", down_color: "#ef5350" });
series.set_data([
  { time: "2026-01-01", open: 100, high: 104, low: 99, close: 103 },
  { time: "2026-01-02", open: 103, high: 106, low: 102, close: 105 },
]);

chart.time_scale().fit_content();
```

API semantics mirror the reference charting library v5 (options, series handles, time/price scale handles,
events), with snake_case naming. See the
[repository](https://github.com/TradeAion/Origin_charts) for the full docs.

## Bundler notes

The package is ESM-only and ships two artifacts side by side in `dist/`: `index.js` and
`origin_wasm_bg.wasm`. The wasm is fetched relative to the bundle
(`new URL("origin_wasm_bg.wasm", import.meta.url)`).

- **webpack 5 / Next.js / Vite production builds**: works out of the box (the wasm is emitted as
  an asset).
- **Vite dev server**: the dep optimizer rebundles `node_modules`, breaking the co-location.
  Either exclude the package from pre-bundling:

  ```ts
  // vite.config.ts
  export default { optimizeDeps: { exclude: ["@origin/charts"] } };
  ```

  or point the engine at an explicit wasm URL:

  ```ts
  import { create_chart, init_wasm } from "@origin/charts";
  import wasm_url from "@origin/charts/dist/origin_wasm_bg.wasm?url";

  await init_wasm(wasm_url); // call once, before the first create_chart
  ```
- **Plain static hosting / `<script type="module">`**: works as-is.

## Runtime requirements

Browser only (DOM + `fetch` of the wasm asset; WebGPU optional — falls back to Canvas2D).
Importing the module in Node/SSR is safe (side-effect-free); calling `create_chart` requires a
browser environment.

## License

Proprietary — commercial license required. See [LICENSE](../../LICENSE).
