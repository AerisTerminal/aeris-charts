# Nucleus Charts Architecture

## Purpose

Nucleus Charts is a high-performance financial chart engine. It provides chart state, interaction behavior, drawing tools, indicators, frame construction, and multiple rendering backends for Axiusflow and browser hosts.

The engine is backend-neutral and host-neutral. One canonical state must produce equivalent frames across GPUI, WebGPU, Canvas2D, and native test rendering. Performance, visual parity, deterministic behavior, and bounded resource use are product requirements.

Source code, tests, and measured release behavior are the implementation truth. This file must change in the same commit whenever the architecture changes.

## Data flow

```text
Host API and market data
    -> nucleuscharts_core validation, data, scales, and options
    -> nucleuscharts_engine chart state and interaction
    -> ChartFrame and nucleuscharts_render DrawList
    -> GPUI | WebGPU | Canvas2D | tiny-skia executor
    -> pixels and frame metrics
```

Browser hosts enter through `packages/charts`, which translates the public TypeScript API into typed arrays and WebAssembly calls. Native Rust hosts use `nucleuscharts_engine` directly and select a renderer. Rendering backends consume prepared frame data; they do not own chart semantics.

Canonical series data uses opaque chart-local `u32` identities mapped to reusable storage slots. Identities are never reused, removed identities are classified as stale, and slot-backed vectors remain bounded by peak concurrent series rather than lifetime add/remove count. Each ordinary series owns one timestamp column and either one scalar value column or four OHLC columns. `PlotList` owns only a dense range or sparse logical-index mapping plus its chunked autoscale cache; allocation-free views join that mapping to the canonical values for queries and frame construction. Dense aligned mappings carry no per-row index allocation. Indicator outputs own one scalar value column and alias a contiguous source-time range by identity, so they duplicate neither source timestamps nor plot values. The merged timestamp union remains independently owned because ordinary source series are independently mutable and may diverge or carry whitespace. It carries a generation that changes only when its contents change; time weights use that generation, while value-only current-bar updates retain the O(1) fast path.

Each canonical series also carries a data generation. An ascending typed batch is sanitized once at the host boundary, merged into its source in one data-layer operation, then synchronizes merged time points, tick weights, dependent indicators, and frame generations once. Tail batches append weights incrementally; historical batches merge in `O(n + k)` and reindex once rather than once per input row.

## Crate boundaries

### `nucleuscharts_core`

Platform-free chart fundamentals: validated canonical columnar data, compact plot index/view storage, ranges, options, formatting, price scales, time scales, tick marks, and shared math. It also exposes structure-level payload and capacity attribution for memory evidence; these counters are not allocator, WASM-page, or browser-memory measurements. Media-space calculations remain `f64`; conversion to backend coordinate formats happens at rendering boundaries.

`nucleuscharts_core` must not depend on a window system, browser, GPU, or host application.

### `nucleuscharts_indicators`

Pure technical-indicator calculations over numeric slices. Warm-up gaps are explicit. Alongside clean full-recomputation functions, it owns the explicit per-formula rolling state used for append, current-bar replacement, and rebuild-from-index. Bounded-window formulas retain no source-length state; recursive formulas retain tail state and one checkpoint per 1,024 source rows, then recompute from the nearest prior checkpoint after a historical correction. Derived values use short-lived transfer buffers that move into or update the engine's canonical output series and are capped after partial repairs. This crate does not know about charts, panes, rendering, WebAssembly, or GPUI.

### `nucleuscharts_engine`

The headless owner of chart behavior and mutable chart state. It owns series, panes, scales, workspace layout, drawings, hit testing, interaction models, indicator bindings, price lines, and frame construction.

Hosts send input and data to the engine. The engine returns query results and a prepared `ChartFrame`. Host-specific gesture recognition may translate operating-system events, but zoom, scroll, kinetic motion, snapping, selection, and drawing semantics belong here.

An indicator binding keeps its public definition, compact private runtime, and ordinary canonical output series separate. Sparse runtime checkpoints are tied to source row positions and to the source and optional volume-series generations. A tail mutation advances only bindings that depend on that source and installs only changed output rows; a historical mutation resumes from the nearest valid checkpoint and replaces the affected output suffix, while truncation or complete replacement performs a clean rebuild. Removed source/output series drop the binding and its runtime state together.

### `nucleuscharts_render`

Backend-neutral drawing primitives, colors, geometry, bar-width rules, and the ordered `DrawList`. This is the contract shared by every renderer. Pixel snapping, primitive ordering, clipping intent, and geometry must be decided before backend execution whenever possible.

### `nucleuscharts_render_gpui`

The native GPUI executor. It converts the prepared primitive stream into GPUI scene operations and owns GPUI-specific text, image caches, geometry conversion, backend metrics, and fixtures. It must not fork chart behavior or recalculate engine geometry.

### `nucleuscharts_render_wgpu`

The WebGPU executor. It owns quad, triangle, textured-label, atlas, blend, multisample, scissor, and GPU timing resources. GPU objects are reused across frames and rebuilt only when their actual invalidation inputs change.

### `nucleuscharts_wasm`

The browser boundary. It exposes the engine through `wasm-bindgen`, decodes typed input, selects WebGPU or Canvas2D policy, executes browser frames, handles shared ring input, text measurement, workspace APIs, and browser telemetry.

The browser boundary translates data and platform events. It must not become a second chart engine.

### `nucleuscharts_native`

The headless native executor and verification support. It uses tiny-skia for deterministic raster output, golden comparisons, examples, and release performance gates. It is evidence infrastructure, not a competing product model.

## TypeScript package

`packages/charts` publishes the `@nucleuscharts/financial` browser API. It owns WebAssembly initialization, TypeScript chart handles, DOM canvas lifecycle, resize observation, browser event translation, gesture recognition, host callbacks, themes, shortcuts, offscreen support, and grid helpers.

The package also ships `design.css` and Inter as the portable host design system. Its semantic CSS colors have deterministic sRGB render equivalents in `packages/charts/src/style_tokens.json`; `nucleuscharts_core` compiles that file into the default options used by every engine and backend. Chart foreground, muted foreground, surfaces, borders, and market colors therefore resolve before frame construction rather than through demo or renderer overrides. The demo consumes the published CSS asset and selects the same named theme as the chart. A `v*` tag matching the package version publishes the verified artifact to GitHub Packages.

The package uses `snake_case` publicly. Data crosses into WebAssembly in typed columns or bounded shared-ring layouts rather than per-point object calls on hot paths. Typed update batches transfer their sanitized owned columns to the engine's batch entry point; the browser wrapper never loops through the single-row engine API. `examples/web_demo` is an integration and parity test host, not part of the library architecture.

`chart.remove()` is the single public browser lifecycle operation. It is idempotent and transitions the retained TypeScript handle to a disposed state after cancelling scheduling, detaching browser resources and extensions, releasing per-chart GPU state, explicitly disposing the Rust object, and calling the generated `free()`. Later operations fail with a stable disposed-state error. Offscreen charts use the same explicit dispose-then-free ordering.

## State and frame ownership

Each chart has one engine owner. Mutations invalidate only the state that changed. A frame is a deterministic snapshot of engine state for a viewport and device scale.

Frame invalidation is an engine-owned generation graph. Layout, coordinates/autoscale, grid and underlay, each series, drawings, and interaction overlays have independent generations. Coordinate-range changes fan out to coordinate-dependent layers; a value-only current-bar update stays on its source series when autoscale bounds do not change. Public option and series-style mutation are included in the generation inputs, so direct native callers cannot bypass retention accidentally.

The engine retains semantic pane layers and their ordered primitive/point ranges, then assembles the same canonical `ChartFrame` contract from clean and rebuilt layers. The retained boundaries are underlay/grid, individual series, pane chrome, drawings, and overlay. Retention never gives a backend permission to change ordering or semantics. Host/plugin primitive callbacks use the canonical frame but conservatively rebuild the affected pane stream because their output is not engine-owned. Incremental frames are tested against forced clean rebuilds across data, interaction, scale, drawing, theme, and resize mutations.

Panes expose stable chart-local identities at the browser boundary. A live pane handle resolves its current index after moves or swaps; removal permanently invalidates that handle, so later index reuse cannot retarget it to another pane.

The ordered frame contract contains pane backgrounds and grids, series geometry, custom-series contributions, drawings, primitives, crosshair overlays, axes, labels, and text. Backends preserve ordering, clipping, blending, and coordinate conversion. A backend may batch compatible adjacent primitives only when visible output is unchanged.

## Plugins and host extensions

Custom series and primitives are explicit host boundaries. The engine owns their identity, layout participation, hit-test context, autoscale contribution, and built-in chrome integration. A host may execute custom drawing callbacks, then records the values the engine needs for the next canonical frame.

Extensions must not receive unrestricted engine internals or create a second scene graph. Add extension surfaces only for current consumers with a stable semantic need.

Disposal invokes every registered extension teardown exactly once; one failing JavaScript cleanup hook cannot prevent the remaining hooks from running.

## Performance contract

Performance comes from avoiding work:

1. Recompute only invalidated state.
2. Keep hot data columnar and transfers bounded.
3. Reuse GPU, text, image, and geometry resources.
4. Conflate replaceable frame requests while preserving the newest state.
5. Keep rendering and input queues bounded.
6. Measure release builds before changing algorithms or adding caches.

Track CPU frame time, GPU time where available, draw calls, dropped and presented frames, memory, ring overruns, interaction latency, and steady-state allocation. Device loss or unavailable WebGPU must fail over without losing headless chart state.

Each browser chart owns reusable WebGPU vertex buffers for its retained semantic draw groups. Buffers grow geometrically to a high-water capacity, upload only when the corresponding group revision changes, never shrink during the chart lifetime, and are released with the chart's GPU state. The public last-frame telemetry also reports buffer allocations, buffer writes, uploaded bytes, and retained-layer rebuild counts so stable and localized-update behavior is directly testable.

The shared text atlas treats one render as a transaction: slots referenced or inserted in the current frame cannot be recycled until that frame completes. Atlas pressure after the frame has accepted text defers the reset to the next frame; the browser renders the pressured frame through Canvas2D rather than submit stale UVs. A reset increments the atlas epoch, invalidating retained textured groups and text-cache entries before the next WebGPU submission.

Browser WebGPU shares one page-wide adapter/device/queue and atlas while retaining per-chart surfaces. Device loss is therefore a shared generation event, not ownership of the chart that first created the device: every live chart listener wakes and falls back, while disposed charts have no listener. Headless chart data is preserved through fallback.

## Evidence benchmark subsystem

`benchmarks/` is development and release evidence infrastructure outside every production crate and the published package. Its single Node entry point builds the actual release package, drives the public browser API through the existing Playwright demo host, generates deterministic versioned OHLCV data, validates versioned JSON results, compares explicit baselines, applies centralized budgets, and emits human- and website-readable artifacts. The browser page is served by `examples/web_demo/test_server.mjs` only for automation; it is not part of the npm package.

The subsystem reuses `chart_api.frame_stats()` for bounded CPU, real capability-detected WebGPU timestamp, draw, presentation, dropped-frame, ring-overrun, and WASM-linear-memory observations. It does not add production instrumentation, dependencies, imports, feature flags, logging, or runtime branches. Browser page/heap memory is labeled as whole-page memory, and unsupported presentation or GPU measurements remain unsupported rather than inferred.

Raw local results are ignored and CI results are artifacts. Public summaries and committed release baselines require a clean `release` profile result classified as `official-benchmark-runner`; shared CI timings are smoke/trend evidence only. Scenario, dataset-generator, schema, and baseline versions preserve historical comparability.

Benchmark comparisons enforce only explicitly configured budgets. An empty policy is reported as `NO ENFORCED BUDGET`, and configured keys must match comparable metrics so a typo cannot silently disable a hard threshold. Publication requires the portable browser runtime/parity suite; machine-calibrated pixel, GPU, and wall-clock evidence remains a separate non-blocking result.

## Correctness and parity

Chart math must be deterministic for the same state, viewport, and device scale. Validate malformed data at the input boundary. Preserve whitespace rows, time ordering, logical ranges, primitive order, and explicit warm-up gaps.

OHLC ingestion preserves structurally valid numeric input rather than silently rewriting financial values. Impossible relationships are accepted for compatibility but counted in structured diagnostics alongside accepted, dropped, deduplicated, reordered, non-finite, and out-of-range rows. Clean ingestion returns no diagnostic object on the browser hot path.

Changes to geometry, snapping, scales, interactions, or execution require the narrowest relevant combination of unit tests, frame-contract tests, golden images, draw-stream parity, replay stability, browser tests, and release performance evidence. A backend-specific screenshot alone is not proof of shared-engine correctness.

## Dependency direction

Lower layers never import a host API to bypass their boundary. The headless path is `nucleuscharts_core` and `nucleuscharts_indicators` into `nucleuscharts_engine`, then `nucleuscharts_render`; GPUI, WebGPU, native, and WASM/browser code sit at execution boundaries. Avoid new crates, traits, and feature flags unless they enforce a real current dependency or platform boundary.

## Repository documentation

Markdown documentation may live at the root or beside the component it explains when it has a durable repository purpose. Keep the root README focused on product orientation and contributor setup, and keep architectural ownership and data flow in this file. Do not commit transient work notes, generated reports, or duplicate documentation.

## Verification

The standard gates mirror CI:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy -p nucleuscharts_wasm --target wasm32-unknown-unknown --locked -- -D warnings
cargo test --workspace --locked
cargo run -p nucleuscharts_native --example perf_gate --release

cd packages/charts
npm ci
npm run lint
npm run build
npm run typecheck
npm run test:pack
```

Run Playwright for browser behavior, rendering, interaction, packaging, or parity changes. Run GPUI parity and replay checks for GPUI executor changes.

The evidence harness has one entry point:

```text
node benchmarks/benchmark.mjs test
node benchmarks/benchmark.mjs smoke
node benchmarks/benchmark.mjs release
```
