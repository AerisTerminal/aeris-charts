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

## Crate boundaries

### `nucleuscharts_core`

Platform-free chart fundamentals: validated data, plot lists, ranges, options, formatting, price scales, time scales, tick marks, and shared math. Media-space calculations remain `f64`; conversion to backend coordinate formats happens at rendering boundaries.

`nucleuscharts_core` must not depend on a window system, browser, GPU, or host application.

### `nucleuscharts_indicators`

Pure technical-indicator calculations over numeric slices. Warm-up gaps are explicit. This crate does not know about charts, panes, rendering, WebAssembly, or GPUI.

### `nucleuscharts_engine`

The headless owner of chart behavior and mutable chart state. It owns series, panes, scales, workspace layout, drawings, hit testing, interaction models, indicator bindings, price lines, and frame construction.

Hosts send input and data to the engine. The engine returns query results and a prepared `ChartFrame`. Host-specific gesture recognition may translate operating-system events, but zoom, scroll, kinetic motion, snapping, selection, and drawing semantics belong here.

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

The package uses `snake_case` publicly. Data crosses into WebAssembly in typed columns or bounded shared-ring layouts rather than per-point object calls on hot paths. `examples/web_demo` is an integration and parity test host, not part of the library architecture.

## State and frame ownership

Each chart has one engine owner. Mutations invalidate only the state that changed. A frame is a deterministic snapshot of engine state for a viewport and device scale.

The ordered frame contract contains pane backgrounds and grids, series geometry, custom-series contributions, drawings, primitives, crosshair overlays, axes, labels, and text. Backends preserve ordering, clipping, blending, and coordinate conversion. A backend may batch compatible adjacent primitives only when visible output is unchanged.

## Plugins and host extensions

Custom series and primitives are explicit host boundaries. The engine owns their identity, layout participation, hit-test context, autoscale contribution, and built-in chrome integration. A host may execute custom drawing callbacks, then records the values the engine needs for the next canonical frame.

Extensions must not receive unrestricted engine internals or create a second scene graph. Add extension surfaces only for current consumers with a stable semantic need.

## Performance contract

Performance comes from avoiding work:

1. Recompute only invalidated state.
2. Keep hot data columnar and transfers bounded.
3. Reuse GPU, text, image, and geometry resources.
4. Conflate replaceable frame requests while preserving the newest state.
5. Keep rendering and input queues bounded.
6. Measure release builds before changing algorithms or adding caches.

Track CPU frame time, GPU time where available, draw calls, dropped and presented frames, memory, ring overruns, interaction latency, and steady-state allocation. Device loss or unavailable WebGPU must fail over without losing headless chart state.

## Correctness and parity

Chart math must be deterministic for the same state, viewport, and device scale. Validate malformed data at the input boundary. Preserve whitespace rows, time ordering, logical ranges, primitive order, and explicit warm-up gaps.

Changes to geometry, snapping, scales, interactions, or execution require the narrowest relevant combination of unit tests, frame-contract tests, golden images, draw-stream parity, replay stability, browser tests, and release performance evidence. A backend-specific screenshot alone is not proof of shared-engine correctness.

## Dependency direction

Lower layers never import a host API to bypass their boundary. The headless path is `nucleuscharts_core` and `nucleuscharts_indicators` into `nucleuscharts_engine`, then `nucleuscharts_render`; GPUI, WebGPU, native, and WASM/browser code sit at execution boundaries. Avoid new crates, traits, and feature flags unless they enforce a real current dependency or platform boundary.

## Repository documentation

Markdown documentation may live at the root or beside the component it explains when it has a durable repository purpose. Keep the root README focused on product orientation and contributor setup, and keep architectural ownership and data flow in this file. Do not commit transient work notes, generated reports, or duplicate documentation.

## Verification

The standard gates mirror CI:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p nucleuscharts_wasm --target wasm32-unknown-unknown -- -D warnings
cargo test --workspace
cargo run -p nucleuscharts_native --example perf_gate --release

cd packages/charts
npm ci
npm run lint
npm run build
npm run typecheck
npm run test:pack
```

Run Playwright for browser behavior, rendering, interaction, packaging, or parity changes. Run GPUI parity and replay checks for GPUI executor changes.
