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
(for example, package version `0.9.0` is released from tag `v0.9.0`).

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

Numeric times are finite whole UTC seconds in the exact inclusive range
`-62167219200..253402300799` (years 0000..9999). Nucleus never auto-converts numeric timestamps;
rejections include a likely milliseconds, microseconds, or nanoseconds hint when applicable.
Direct set/update batches reject atomically on any invalid timestamp, and invalid single updates
leave existing data unchanged. Inspect `series.last_ingestion_diagnostics()` for the reason.

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

The engine-owned feature set includes brushable area, grouped bars, heatmap, HLC area,
pretty histogram, shaded background, stacked area,
stacked bars, and box-and-whisker series. Primitive helpers include accessibility, anchored text,
official ±10% price bands, delta and ordinary tooltips, highlighted-bar
crosshair, image watermark, overlay price scale, partial price line, rectangle/trend/vertical
drawings, session highlighting, volume profile, and user-defined price lines.

Heatmap-around-line and shaded-background examples are composed beneath a normal line series.

Features that Nucleus already owns—drawings (including Long Position and Short Position tools), bands, price lines, overlay scales, partial-last-price
lines, session shading, highlighted bar slots, and time-anchored volume profiles—are thin helpers
over those engine APIs. Accessibility is enabled by default; `chart.accessibility()` returns its
singleton controller and `enable_accessibility(chart, options)` configures the same instance for
compatibility. Keyboard/ARIA nodes and announcements remain browser DOM chrome, while bounded data
queries, focus geometry, drawing edits, and rendering primitives use the shared engine. Streaming
market updates are silent unless `announce_data_updates` is enabled. Every returned feature handle
with `detach()` releases its engine and host state.

Browser input uses Pointer Events for mouse and pen, plus cancellable Touch Events for dynamic
page-scroll arbitration. The engine owns the bounded gesture state, 5 px drag threshold,
fixed-start-centroid cumulative pinch behavior, primary-touch continuation, cancellation, and
device-aware hit tolerances. Wheel policy is configurable with
`wheel_behavior: "auto" | "pan" | "zoom"`; matching Lightweight Charts 5.2.1, auto zooms time from
vertical deltas and pans time from horizontal deltas independently on the pane or either axis, with
no Ctrl/Shift special case. The explicit `pan` and `zoom` values retain Nucleus extension routing.

## Trading and order management

Trading objects are a separate first-party engine domain. The application supplies authoritative
positions, working orders, bracket/OCO relationships, executions, and instrument metadata; Nucleus
owns their deterministic visualization, native axis labels, hit testing, risk/reward regions, and
local interaction previews. A drag never rewrites confirmed broker state. Instant mode emits one
typed, broker-neutral intent on release; manual mode holds the preview behind inline Confirm and
Discard controls. The host reconciles a confirmed preview with an accepted state update or rejects
it explicitly. Risk/reward fills belong only to active previews, never confirmed orders.

```ts
const trading = chart.trading();
trading.set_confirmation_mode("manual"); // Optional; the default is "instant".
trading.apply_snapshot({
  instrument: { tick_size: 0.25, price_precision: 2, point_value: 50, currency: "USD" },
  positions: [{ id: "position-1", side: "long", average_price: 5230, quantity: 2 }],
  orders: [{
    id: "target-1", side: "sell", kind: "limit", role: "take_profit", status: "working",
    price: 5240, quantity: 2, position_id: "position-1", oco_group_id: "bracket-1", revision: 4,
  }],
});

trading.subscribe_intents(async (intent) => {
  const accepted = await route_to_broker(intent);
  trading.resolve_intent(intent.sequence, accepted);
  // On acceptance, push the resulting authoritative order/position update through this API.
});

// Convert a completed Long/Short Position drawing into one atomic bracket request. Quantity is
// deliberately host-owned; the intent carries the drawing's tick-snapped entry, TP, and SL.
const plan = chart.selected_drawing();
if (plan && (plan.kind() === "long_position" || plan.kind() === "short_position")) {
  trading.place_bracket_order(plan.id, quantity_from_host);
}
```

Live trading objects, previews, and intent queues are chart-local runtime state and are deliberately
excluded from `chart.export_state()`.

Give the container an explicit size; the chart canvases fill it.

Import the portable design system once in browser hosts:

```ts
import "@nucleuscharts/financial/design.css";
```

Light is the CSS default. Set `data-theme="dark"` (or class `dark`) on a root element for dark
mode, and apply `theme_options("dark")` to the chart. Host chrome and chart labels default to the
system UI font stack. Chart font remains an explicit layout option so a host webfont cannot shift
financial labels until the host sets it.
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

See [Architecture.md](Architecture.md) for ownership, data flow, and backend boundaries.
See [Public_api.md](Public_api.md) for supported/experimental surfaces, persistence, errors, and
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
