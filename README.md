# Nucleus Charts

Nucleus Charts is a financial chart engine built in Rust. One deterministic chart model powers WebGPU, Canvas2D, GPUI, and native rendering for browser and desktop hosts.

The project includes professional chart interactions, drawings, technical indicators, multiple panes and scales, custom series, primitives, shared-memory market-data input, and backend parity tooling.

## Browser package

The browser SDK is published as `@nucleuscharts/financial` on GitHub Packages. Configure the
registry and authenticate with a GitHub token that can read packages before installing:

```ini
@nucleuscharts:registry=https://npm.pkg.github.com
//npm.pkg.github.com/:_authToken=${GITHUB_TOKEN}
```

```sh
npm install @nucleuscharts/financial
```

Version tags publish automatically when the tag matches `packages/charts/package.json` exactly
(for example, package version `0.8.13` is released from tag `v0.8.13`).

Create a chart with the asynchronous, snake-case API:

```ts
import { create_chart } from "@nucleuscharts/financial";

const container = document.querySelector<HTMLElement>("#chart");
if (!container) throw new Error("missing chart container");

const chart = await create_chart(container, { autoSize: true });
const candles = chart.add_series("candlestick");

candles.set_data([
  { time: 1735689600, open: 100, high: 108, low: 98, close: 105 },
  { time: 1735776000, open: 105, high: 112, low: 103, close: 110 },
]);

chart.time_scale().fit_content();
```

## Advanced chart features

The advanced financial series are first-class Rust-engine series. Their data, autoscale projection,
geometry, lifecycle, and rendering are shared by every backend; the browser package only translates
public data and options at the WASM boundary:

```ts
import { create_volume_profile } from "@nucleuscharts/financial";

const heatmap = chart.add_series("heatmap", {
  cell_border_width: 1,
  cell_border_color: "rgba(255,255,255,.08)",
  cell_shader: (amount) => `rgba(80,0,255,${Math.min(1, amount / 100)})`,
});
heatmap.set_data(heatmap_data);

const profile = create_volume_profile(candles, {
  time: 1735689600,
  profile: [{ price: 100, vol: 12 }, { price: 101, vol: 28 }],
  width: 10, // time-scale bar slots
});
// profile.set_data(next_time_anchored_profile); profile.detach();
```

The engine-owned feature set includes brushable area, dual-range histogram, grouped bars,
heatmap, HLC area, pretty histogram, lollipop, rounded candles, shaded background, stacked area,
stacked bars, and box-and-whisker series. Primitive helpers include accessibility, anchored text,
official ±10% price bands, delta and ordinary tooltips, expiring/user price alerts, highlighted-bar
crosshair, image watermark, overlay price scale, partial price line, rectangle/trend/vertical
drawings, session highlighting, volume profile, and user-defined price lines.

As in the upstream examples, dual-range histogram is composed beneath a normal baseline series,
while heatmap-around-line and shaded-background examples are composed beneath a normal line series.

Features that Nucleus already owns—drawings, bands, price lines, overlay scales, partial-last-price
lines, session shading, highlighted bar slots, and time-anchored volume profiles—are thin helpers
over those engine APIs. Accessibility keyboard/ARIA state and tooltip elements remain browser DOM
chrome, while their exact data lookup, guides, focus geometry, and rendering primitives remain in
the engine. Every returned feature handle with `detach()` releases its engine and host state.

Give the container an explicit size; the chart canvases fill it.

Import the portable design system once in browser hosts:

```ts
import "@nucleuscharts/financial/design.css";
```

Light is the CSS default. Set `data-theme="dark"` (or class `dark`) on a root element for dark
mode, and apply `theme_options("dark")` to the chart. The package ships Inter for host UI; chart
font choice remains explicit so asynchronous web-font loading cannot shift financial labels.
Chart defaults use the same semantic roles directly: foreground for axes and value text, border
for crosshair lines, and muted for crosshair-label surfaces.

## Repository layout

- `crates/nucleuscharts_core` — validated data, scales, options, formatting, and shared math.
- `crates/nucleuscharts_indicators` — platform-free indicator calculations.
- `crates/nucleuscharts_engine` — chart state, interactions, drawings, panes, and frame construction.
- `crates/nucleuscharts_render` — backend-neutral primitives and the ordered draw list.
- `crates/nucleuscharts_render_wgpu` — WebGPU executor.
- `crates/nucleuscharts_render_gpui` — GPUI executor.
- `crates/nucleuscharts_wasm` — browser and WebAssembly boundary.
- `crates/nucleuscharts_native` — deterministic native rendering and performance verification.
- `packages/charts` — TypeScript browser package.
- `examples/web_demo` — browser integration and parity test host; it is not a published package.

See [ARCHITECTURE.md](ARCHITECTURE.md) for ownership, data flow, and backend boundaries.
See [PUBLIC_API.md](PUBLIC_API.md) for supported/experimental surfaces, persistence, errors, and
version policy. Workspace Rust crates are internal exact-revision components, not crates.io products.

## Development

Prerequisites: stable Rust, the `wasm32-unknown-unknown` target, `wasm-pack`, and Node.js 18 or newer.

```sh
cargo test --workspace

cd packages/charts
npm ci
npm run build
npm run lint
npm run typecheck
npm run test:pack
```

The complete verification gates are documented in [AGENTS.md](AGENTS.md) and enforced by CI.

## Performance evidence

Reproducible release-package benchmarks live in [`benchmarks/`](benchmarks/README.md). The harness records deterministic workloads, raw samples, statistical summaries, build and machine provenance, capability limits, package sizes, browser CPU/GPU timing, memory, lifecycle, scaling, and soak behavior. Shared CI results are diagnostics; only clean runs from the controlled benchmark environment may produce public claims or release baselines.

## License

Nucleus Charts is proprietary software. See [LICENSE](LICENSE) for the permitted use and distribution terms.
