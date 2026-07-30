# Origin Charts — GPUI Implementation Report

Deliverable for `docs/GPUI_PLAN.md` §6.4, updated after the DPR/resize, text, axis, price-line, and interaction parity investigation.

**Target consumer:** AxiusFlow. Origin supplies a framework-independent engine, shared host-neutral layout/axis policy, and an optional official-GPUI paint adapter. AxiusFlow remains responsible for application chrome and application-specific bridges.

## Verdict

**Renderer/adapter parity: READY.** Every current `Prim` lowers through official `gpui = "=0.2.2"`; clipping, ordering, DPR conversion, native text shaping, cache bounds, and performance gates pass. GPUI remains an optional leaf dependency and is absent from `origin_core`, `origin_engine`, and `origin_render`.

**Desktop-native chart and demo surface: READY.** The default GPUI demo now exposes the Web demo's application categories: five chart types and granular style cycles; SMA, volume, and RSI; authoritative `Workspace` splits, cap/usage/close/resize, Ctrl/Cmd+H/V shortcuts, and Ctrl/Cmd+click maximize/restore; seven drawing tools with live templates; crosshair, theme, grid, font, series chrome, axes, watermark, scaling, kinetic, reset, legend, and click status controls. Split cells own independent engines and receive distinct deterministic 300-bar assets, while the root keeps the Web app's chart-wide controls and single workspace legend. Exact package light/dark tokens are applied (`#ffffff/#f5f5f5/#0a0a0a` and `#0a0a0a/#16191f/#fafafa`), explicit grid/axis/text/separator choices follow-or-pin across theme changes, and shell dividers resolve from the root chart's axis-border token.

The host also has complete desktop pointer cleanup, modifier-aware OHLC magnet behavior while drawing, native drawing cursors and shaped-text hit widths, root-routed divider release, bounded interactive telemetry, and elapsed-time live updates. The finite benchmark path remains separate and process-validated.

**JavaScript plugin-object and universal application parity: NOT CLAIMED.** Native fixture toggles are explicitly visual approximations, not a JavaScript object bridge: they do not implement browser plugin attachment lifecycle, arbitrary JS callbacks, custom-series replacement contracts, or a JS ABI. The GPUI toolbar now follows the Web demo's always-present wrapping group layout and exposes the same functional setting families through native cycle/toggle controls, but it does not reproduce free-form HTML color, text, number, select, and range widgets. Touch arbitration, an inline text-entry editor, accessibility announcements, and public application callback/subscription bridges remain host work. These limits do not block the native renderer/engine/default desktop demo, but they do block an honest claim of universal one-to-one application parity.

The strict universal-zero pixel verdict also remains **BLOCKED** by the known marker raster residual, pane-only cross-backend capture, and physical capture availability at only DPR 1.5 on this machine.

## 1. Dependency and architecture

| Item | Value |
|---|---|
| Package | `gpui` |
| Version | `=0.2.2` (exact pin) |
| Source | official Zed crates.io publication |
| Features | `default-features = false`, `windows-manifest` |
| Optional feature | `origin_render_gpui/gpui-backend` |
| Default graph | excludes GPUI |

The dependency direction is:

```text
origin_core -> origin_engine -> origin_render::DrawList<Prim> -> origin_render_gpui -> gpui
```

`origin_core`, `origin_engine`, and `origin_render` contain no GPUI imports or lifecycle types. Shared browser/desktop policy now lives in framework-neutral engine modules:

- `origin_engine::host_layout`: time-strip reservation, native-measured left/right axis widths, grow-fast/shrink-on-full behavior, two refinement passes, pane geometry, and time-scale width.
- `origin_engine::axis_primitives`: watermark, axis borders, ticks, pane separators, plain/boxed labels, attached price/countdown rows, rounded corners, and final text primitives.

The browser host delegates to these modules with Canvas text metrics. The GPUI host delegates with `WindowTextSystem` metrics. GPUI-specific event objects, focus, repaint scheduling, native shaping, and scene submission remain at the GPUI boundary.

`PreparedOriginFrame` borrows an already-built `ChartFrame`; painting cannot mutate market data, models, indicators, scales, or layout. One `GpuiChartRenderer` per chart retains bounded scene, geometry, shaped-text, and image caches.

## 2. DPR/resize defect and correction

### Root cause

The original probe updated `css_width`, `css_height`, and DPR on resize but retained positive negotiated `pane_w` and `pane_h`. `ChartEngine::layout_for_frame` intentionally preserves positive host-negotiated dimensions. Maximizing the window therefore expanded the GPUI canvas while candles and price lines remained constrained to the old approximately 1024×640 geometry. Synthetic host text used the new canvas bounds, producing the misleading combination of partial lines, clipped/missing labels, and apparent DPR drift.

Pinned GPUI 0.2.2 source confirms `Bounds<Pixels>` is in logical pixels and GPUI applies the window scale for physical output. The adapter's device-to-logical divide-by-DPR transform was correct and was not changed.

### Fix

On a real bounds change the GPUI host now discards stale negotiated pane dimensions, sets the new CSS size and DPR, and calls shared `ChartEngine::recompute_layout_with_measure`. It then builds a real `AxisFrame`, the pane `ChartFrame`, and shared axis primitives. DPR changes invalidate native text/resource caches. Runtime assertions verify:

- full logical width equals `left axis + pane + right axis`;
- pane scissor origin/width/height match negotiated pane geometry at DPR;
- frame DPR equals GPUI's physical window scale.

A regression test resizes 1024×640 to 1536×864 at DPR 1.5 and verifies pane replacement, frame dimensions, device scissor, and real axis text emission.

### Real GPUI evidence

A corrected release probe was built in an alternate target directory so the old persistent release process remained untouched during validation:

```text
canvas          : 1024.0 × 640.0 logical
pane            : origin (0.0, 0.0), size 968.0 × 612.0 logical
axes            : left 0.0, right 56.0 logical
device scissor  : [0, 0, 1452, 918]
physical DPR    : 1.5
text runs       : 27 painted, 0 dropped
all prim drops  : 0
```

This proves correct propagation and full negotiated geometry for the tested physical scale. It is not a substitute for a full-frame cross-backend pixel diff.

## 3. Primitive mapping and text

The guard test `every_prim_variant_lowers_to_at_least_one_op` fails if a future `Prim` variant is not handled.

| Origin primitive | GPUI lowering |
|---|---|
| `Rect` | solid `paint_quad` |
| `RectFrame` | four ordered edge quads |
| `HLine`, `VLine` | Origin-snapped quad spans; existing dash phase preserved |
| `Background` | quad with a bounds-relative 180° linear gradient |
| `RoundRect` | Origin tessellated polygon by default; native rounded quad is opt-in |
| `Triangle` | one GPUI path triangle |
| `Circle` | tessellated disc plus optional annulus stroke |
| `Polyline` | Origin line tessellation through GPUI path triangles |
| `AreaFill` | Origin mesh with equivalent bounds-relative gradient |
| `BandFill` | two triangles per segment |
| `Text` | GPUI `WindowTextSystem::shape_line` and `ShapedLine::paint` |

Composition remains pane-by-pane (`under`, `main`, `top_prims`) inside each pane mask, followed by the shared unscissored axis/top layer. The adapter never uses `paint_layer`, which would flatten GPUI ordering.

`origin_render_gpui::backend::measure_text` exposes native logical width, ascent, and descent for host negotiation. The browser applies its Canvas ink-box midpoint correction. GPUI supplies zero additional correction because the GPUI text executor already converts the visual center to a baseline from native ascent/descent; applying the browser correction again would double-shift text.

The GPUI shaped-line cache is a bounded 512-entry LRU. Position and alignment remain paint-time inputs, allowing moved labels to reuse shaping. `invalidate_caches()` clears text and resource state when DPR or external font resolution changes.

## 4. Desktop interaction and native application surface

The integration demo forwards GPUI events into engine-owned behavior rather than reimplementing chart math. Its explicit `GestureConfig` matches the browser's resolved desktop defaults: pan, wheel scroll/zoom, axis double-click resets, time/price axis scaling, and pane resizing are enabled; mouse kinetic scrolling is disabled.

Implemented host policy includes:

- pane, axis, separator, drawing-body, drawing-anchor, and directional-resize cursors;
- pointer-exit cleanup for crosshair, hovered series, modifier magnet, scroll/scale sessions, separator state, drawing drags, and brush capture;
- Ctrl/Cmd OHLC crosshair magnet while a drawing tool is armed, plus Shift straightening;
- 5px Manhattan click slop, distinct click selection, persistent drag pan, normalized Windows wheel input, vertical manual-scale price pan, axis drag scaling, double-click resets, and browser-aligned keyboard controls;
- GPUI-shaped drawing-label measurement for hit geometry;
- browser-grid workspace behavior: a 5px drag target consuming 1px of layout, an axis-color hairline absolutely snapped from its painted bounds to an integer physical-pixel span, live ratio preview with engine persistence on release, Ctrl/Cmd+H/V split shortcuts, and Ctrl/Cmd+click maximize/restore;
- complementary flex bases for split children, nested owning-split measurement in the browser grid's total coordinate space, no demo-only active-cell outlines, and root-routed release cleanup where GPUI pointer capture is unavailable;
- one always-visible wrapping toolbar with Web-style bordered caption groups, one workspace-level root legend whose parent observes root-chart notifications for fresh pointer-driven OHLC text, browser-equivalent root-versus-active control routing, and clean distinctly seeded hourly assets (1,000 bars for the root and 300 bars per split); toolbar splits activate the new cell while shortcut splits preserve the current active cell;
- one authoritative engine `WorkspaceLayout` snapshot, primary-cell protection, independent chart engines per split cell, cap/usage reporting, and synchronized ratios/IDs;
- bounded interactive metrics and one live append per elapsed-time epoch; finite mode retains all samples and its 60-frame cadence for benchmark repeatability.

### Native demo support checklist

| Surface | Native GPUI status | Evidence / qualification |
|---|---|---|
| Candlestick, bar, line, area, baseline | Supported | Runtime controls and series conversion paths |
| Candle/line/area styling | Supported cycles | Body/wick/border colors and visibility, reset-parts, line color/width, and area fill; not arbitrary HTML color/range inputs |
| SMA(20), volume overlay, RSI(14) pane | Supported | Independent toggles; engine-owned series/pane behavior |
| Horizontal/vertical splits, active cell, close, cap, usage, resize, maximize | Supported | Browser-grid 1px/5px divider geometry, bounds-aware physical-pixel snapping, live preview/release persistence, toolbar-versus-shortcut activation semantics, Ctrl/Cmd shortcuts, maximize/restore, clean hourly cell assets, and authoritative `Workspace` topology |
| Trend, horizontal line/ray, vertical line, rectangle, text, brush | Supported | Placement, selection, drag, live template patches, clear/delete |
| Drawing color/style/width/text typography | Supported cycles | Pending and selected field-level merge tests preserve unrelated options |
| Crosshair mode/color/width/style/label background/labels | Supported cycles | Web ordering and ranges; modifier-aware magnet and pointer cleanup included |
| Exact light/dark package themes | Supported | Token assertions for background, grid, axes, text, separators |
| Grid/font and follow-or-pin style overrides | Supported cycles | Grid visibility/color/style plus font family/size; theme regression covers pinned and unpinned values |
| Price line, last value, title chip/text, countdown, bid/ask | Supported cycles | Series-specific controls follow the active Web-grid cell |
| Axis borders/text/separators and shell divider color | Supported | Web-equivalent root-chart routing; divider follows the root axis token |
| Watermark text/color/size, axis scaling, kinetic, reset | Supported cycles | Browser-equivalent root-chart routing; mouse kinetic default remains off |
| OHLC legend and click status | Supported | Single root-chart overlay; retained parent observation propagates root pointer notifications. Native pointer-driven text changes are code-reviewed, not automated by an OS input driver |
| Browser plugin fixtures | Visual approximations only | No JavaScript plugin-object ABI, lifecycle, hit views, or callbacks |
| Touch gestures | Not implemented in this desktop demo | Shared engine math exists; native touch arbitration remains host work |
| Inline drawing text editor | Not implemented | Text drawing and template label cycles work; no free-form editor widget |
| Accessibility/callback bridges | Not implemented | Application integration responsibility |

The previously validated persistent process launched at DPR 1.5; a replacement smoke is required after the browser-grid application-parity correction. There is no automated OS screenshot/input driver in this workspace. Divider geometry, routing, shortcut activation semantics, maximize guards, and release persistence are supported by focused code review and pure state/layout tests; the persistent-window check is only an application startup/render smoke, not automated native mouse/keyboard-event evidence. Visual button-by-button inspection is therefore not claimed.

## 5. WebGPU-versus-GPUI pixel evidence

The primary reference is Chromium's presented WebGPU frame. The candidate is an official-GPUI process using the same deterministic engine fixture. Both are captured at physical DPR 1.5. The current matrix crops to the engine-owned pane before price/time axes.

A fresh build after the shared layout/axis extraction reproduced the pinned results:

| Case | Exact differing pixels | Max delta | Perceptual pixels | Result |
|---|---:|---:|---:|---|
| fit-content, light, base | 0 | 0 | 0 | exact |
| spacing 0.5, light, base | 0 | 0 | 0 | exact |
| spacing 6, light, base | 0 | 0 | 0 | exact |
| spacing 50, light, base | 0 | 0 | 0 | exact |
| fit-content, dark, base | 0 | 0 | 0 | exact |
| spacing 6, light, markers | 1,176 (0.0618%) | 217 | 483 (0.0254%) | bounded known residual |

The five exact base cases demonstrate that extracting browser layout/axis policy did not change existing pane WebGPU output. The marker residual is localized to browser text/MSAA versus DirectWrite/path-edge rasterization and remains gated rather than normalized away.

The synthetic cause-isolation harness also reproduced its established DPR 1.5 values:

| Fixture | Differing pixels | Max delta |
|---|---:|---:|
| crisp rectangles | 0 | 0 |
| translucent | 16,920 | 1 |
| opaque AA | 2,523 | 118 |
| tessellated | 2,211 | 69 |
| gradients | 108,135 | 38 |
| text | 2,504 | 255 |

The crisp canonical gate passes at tolerance zero.

## 6. Performance

### Scene construction

Release, 1600×900 at DPR 1.5, 200 iterations per source fixture:

| Source points | Prims | Quads | Paths | p50 | p95 | p99 | Max | Gate |
|---:|---:|---:|---:|---:|---:|---:|---:|---|
| 10,000 | 9,610 | 11,794 | 1 | 0.452 ms | 0.525 ms | 0.601 ms | 0.632 ms | PASS |
| 100,000 | 9,610 | 11,794 | 1 | 0.453 ms | 0.549 ms | 0.698 ms | 0.758 ms | PASS |
| 1,000,000 | 9,610 | 11,794 | 1 | 0.455 ms | 0.536 ms | 0.579 ms | 0.717 ms | PASS |

### Corrected real GPUI probe

`1024×640`, physical DPR 1.5, 120 release frames, 500 bars plus one live append, real axes, price line, and drawing:

```text
prims / ops       : 1534 / 2386
quads / paths     : 2354 / 3
text runs         : 27 painted, 0 dropped
adapter total     : p50 0.392 ms, p99 0.799 ms
plan build        : p50 0.024 ms, p99 0.105 ms
GPUI submission   : p50 0.366 ms, p99 0.755 ms
text shape cache  : 27 hits, 0 misses
```

The combined adapter p99 remains below the 2 ms budget. This adapter metric starts after host prepaint/frame preparation; it does **not** include GPUI layout, host text measurement, or other end-to-end window work. Clean unchanged frames now return before drawing-label measurement, so committed labels are not reshaped every animation prepaint. The post-correction process exited at exactly 120 painted frames, emitted the summary, reported 501 bars (one finite-mode append), dropped zero primitives, and performed no trailing post-budget prepaint, append, or paint.

### Existing native performance gate

```text
build_frame (10 × 50K bars): 0.67 ms / 16.67 ms budget — PASS
1M-bar set_series_data:      72.61 ms / 300 ms budget — PASS
```

## 7. Final validation

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `git diff --check` | PASS; line-ending notices only |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo clippy -p origin_render_gpui --all-targets --features gpui-backend -- -D warnings` | PASS |
| `cargo clippy -p origin_wasm --target wasm32-unknown-unknown --all-targets -- -D warnings` | PASS |
| `cargo test --workspace` | PASS |
| `cargo test -p origin_render_gpui --features gpui-backend` | PASS, 139 library/integration tests |
| `cargo test -p origin_render_gpui --features gpui-backend --example gpui_probe` | PASS, 15 tests including themes, drawing patch safety, telemetry bounds, live cadence, nested browser-grid owning extents, complementary flex ratios, workspace snapshots, and interaction persistence |
| `cargo test -p origin_engine --lib` | PASS, 227 tests including pending/brush live-option and typed-workspace regressions |
| `cargo build --workspace` | PASS; default graph excludes GPUI |
| default and feature-enabled dependency trees | PASS; feature resolves `gpui v0.2.2` only |
| unofficial/git-source scans | PASS; no matches |
| strict native `perf_gate` | PASS |
| release `plan_bench` | PASS at 10K/100K/1M |
| finite corrected 120-frame release GPUI probe | PASS, exact frame-budget exit, p99 0.799 ms, zero drops |
| persistent normal-target interactive GPUI smoke | PASS, corrected replacement running at DPR 1.5 with a 1280×402.7 logical chart region; no startup/layout errors; no automated screenshot/input-driver evidence |
| `pixel_parity` at tolerance zero | PASS crisp gate; diagnostic values reproduced |
| `npm run test:gpui-webgpu` | PASS, fresh wasm/JS/types build plus six-case matrix |
| fractional-DPR resize regression | PASS |
| edited-file diagnostics | clean |

## 8. Known limitations and blockers to universal one-to-one parity

1. **Full-frame differential evidence:** the canonical matrix is pane-only. A matching browser/GPUI fixture that captures real axes, time labels, the LI chip, maximized resize, and interaction states is still required.
2. **Physical DPR matrix:** real GPUI capture is DPR 1.5 on this Windows display. Other DPRs have draw-stream and coordinate coverage but require OS-scale workers for physical captures.
3. **JavaScript plugin-object ABI:** native fixture controls are visual approximations. They do not accept browser plugin objects or reproduce attachment/detachment callbacks, JS hit-test/axis-view contracts, and custom-series lifecycle.
4. **Native widget equivalence:** the GPUI toolbar follows the Web demo's wrapping group layout and exposes its functional setting families, but compact native cycles/toggles do not provide arbitrary HTML color, text, number, select, or range values. An inline drawing text editor remains absent.
5. **Touch/accessibility/application callbacks:** native touch arbitration, accessibility announcements, and public subscription bridges remain application-host responsibilities.
6. **Marker rasterization residual:** the bounded 1,176-pixel residual remains; exact values are retained.
7. **Volume overlay:** host-owned overlay margin policy is not represented in the primary pane matrix.
8. **Circle stroke divergence:** GPUI, Canvas2D, and native honor `Prim::Circle` stroke; the current WebGPU triangle executor drops it. This pre-existing product discrepancy was not copied into GPUI.

## 9. Integration boundary

Add the optional adapter feature:

```toml
origin_render_gpui = { path = "…/crates/origin_render_gpui", features = ["gpui-backend"] }
```

During GPUI prepaint/update:

1. Copy logical bounds and `window.scale_factor()` into the engine.
2. Call `recompute_layout_with_measure`, using `origin_render_gpui::backend::measure_text` for the chart's resolved font.
3. Build `AxisFrame`, then `ChartFrame`, then call `build_axis_primitives_into`.
4. Submit immutable pane and axis data with `PreparedOriginFrame::new(&frame).with_axis(...)`.

During paint:

```rust
let viewport = OriginViewport::from_bounds(
    bounds.origin.x.into(),
    bounds.origin.y.into(),
    bounds.size.width.into(),
    bounds.size.height.into(),
);
renderer.paint_frame(
    &prepared,
    viewport,
    window.scale_factor(),
    window,
    cx,
)?;
```

Contract:

- rebuild whenever logical bounds or scale factor changes;
- require `frame.pixel_ratio == window.scale_factor()`;
- paint shared axis/top primitives after pane content via `with_axis`;
- retain one renderer per chart for stable caches;
- invalidate caches on DPR or external font/resource changes;
- do not hold model/data locks during submission;
- do not wrap the chart in `Window::paint_layer`;
- record `GpuiFrameMetrics` in application telemetry.

## 10. Files involved in the parity correction

```text
crates/origin_engine/src/
    axis_primitives.rs
    drawings.rs
    drawings/tests.rs
    host_layout.rs
    workspace.rs
    lib.rs
crates/origin_wasm/src/
    chart.rs
    chart/inner_api.rs
    chart/inner_render.rs
    lib.rs
    axis_policy.rs (removed; policy promoted to origin_engine)
crates/origin_render_gpui/src/backend.rs
crates/origin_render_gpui/examples/gpui_probe.rs
docs/GPUI_IMPLEMENTATION_REPORT.md
```

The browser still uses Canvas metrics and its existing rendering backends; only duplicated host policy moved into shared engine code. The fresh presented-WebGPU pane matrix remained unchanged.
