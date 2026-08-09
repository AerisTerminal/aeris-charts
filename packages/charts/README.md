# @tradeaion/charts

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
"@tradeaion" = { token = "$GITHUB_READ_PACKAGES_TOKEN", url = "https://npm.pkg.github.com/" }
```

(Reference an env var; don't hardcode the token.) Then:

```sh
bun add @tradeaion/charts
```

**npm** — add to your project's `.npmrc`:

```
@tradeaion:registry=https://npm.pkg.github.com
//npm.pkg.github.com/:_authToken=${GITHUB_READ_PACKAGES_TOKEN}
```

```sh
npm install @tradeaion/charts
```

No Rust toolchain or native build step is needed — the package ships prebuilt JS + WASM.

## Quick start

```ts
import { create_chart } from "@tradeaion/charts";

// Async: WebGPU backend acquisition (the one deliberate divergence from the reference's sync createChart).
const chart = await create_chart(document.getElementById("chart"), {
  autoSize: true,
});

const series = chart.add_series("candlestick");
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
  export default { optimizeDeps: { exclude: ["@tradeaion/charts"] } };
  ```

  or point the engine at an explicit wasm URL:

  ```ts
  import { create_chart, init_wasm } from "@tradeaion/charts";
  import wasm_url from "@tradeaion/charts/dist/origin_wasm_bg.wasm?url";

  await init_wasm(wasm_url); // call once, before the first create_chart
  ```
- **Plain static hosting / `<script type="module">`**: works as-is.

## Worker rendering with `OffscreenCanvas`

For rendering in a dedicated worker, transfer **two** canvases and call the additive
`create_offscreen_chart` entry point. A canvas cannot switch context type after WebGPU claims it, so
the second surface keeps Canvas2D fallback warm without rebuilding chart state.

```ts
// main.ts
const gpu_element = document.querySelector<HTMLCanvasElement>("#chart-gpu")!;
const fallback_element = document.querySelector<HTMLCanvasElement>("#chart-fallback")!;
const gpu_canvas = gpu_element.transferControlToOffscreen();
const fallback_canvas = fallback_element.transferControlToOffscreen();

const show_backend = (backend: "webgpu" | "canvas2d") => {
  gpu_element.style.visibility = backend === "webgpu" ? "visible" : "hidden";
  fallback_element.style.visibility = backend === "canvas2d" ? "visible" : "hidden";
};
worker.onmessage = ({ data }) => {
  if (data.type === "ready" || data.type === "backend_change") show_backend(data.backend);
};

worker.postMessage(
  { type: "init", gpu_canvas, fallback_canvas, width: 900, height: 500, dpr: devicePixelRatio },
  [gpu_canvas, fallback_canvas],
);
```

```ts
// chart.worker.ts
import { create_offscreen_chart } from "@tradeaion/charts";

self.onmessage = async ({ data }) => {
  if (data.type !== "init") return;
  const chart = await create_offscreen_chart(data.gpu_canvas, data.fallback_canvas, {
    width: data.width,
    height: data.height,
    dpr: data.dpr,
  });
  chart.subscribe_backend_change((backend) => {
    self.postMessage({ type: "backend_change", backend });
  });
  chart.add_series("candlestick");
  self.postMessage({ type: "ready", backend: chart.backend() });
};
```

Keep both HTML canvases stacked. Use the initial `chart.backend()` report and every
`subscribe_backend_change` notification to display the active surface, including a runtime
WebGPU-to-Canvas2D device-loss fallback. The subscription returns an unsubscribe function.

The worker API provides typed data updates, explicit `resize`, options, frame statistics, visible
logical range, and normalized `inject_pointer_event`, `inject_wheel_event`, and `inject_key_event`
methods. Relay coordinates in full-chart CSS pixels. Workers cannot infer element size or DPR, so
send changes from the main thread and call `resize(width, height, dpr)`. Worker options reject
DOM-only settings rather than silently ignoring them: `autoSize`, localization, gesture/kinetic/
tracking options, and `layout.panes.enableResize` remain available only through `create_chart`.

The worker façade deliberately excludes DOM-only facilities such as `ResizeObserver`, accessibility,
HTML/canvas plugins, DOM event listeners, and synchronous screenshots. Use `create_chart` when those
features are required.

## Runtime requirements

Browser runtime with `fetch` for the wasm asset. Use `create_chart` in a DOM `Window`, or
`create_offscreen_chart` in a dedicated worker with `OffscreenCanvas`. WebGPU is optional in both
paths and falls back to the supplied Canvas2D surface.

Importing the module in Node/SSR is safe (side-effect-free), but constructing either chart requires a
browser implementation of the corresponding canvas APIs.

## License

Proprietary — commercial license required. See [LICENSE](../../LICENSE).
