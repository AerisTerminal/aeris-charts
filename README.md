# Nucleus Charts

Nucleus Charts is a financial chart engine built in Rust. One deterministic chart model powers WebGPU, Canvas2D, GPUI, and native rendering for browser and desktop hosts.

The project includes professional chart interactions, drawings, technical indicators, multiple panes and scales, custom series, primitives, shared-memory market-data input, and backend parity tooling.

## Browser package

The browser SDK is configured as `@nucleuscharts/financial` for GitHub Packages. Configure the registry before installing a release:

```ini
@nucleuscharts:registry=https://npm.pkg.github.com
```

```sh
npm install @nucleuscharts/financial
```

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

Give the container an explicit size; the chart canvases fill it.

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
