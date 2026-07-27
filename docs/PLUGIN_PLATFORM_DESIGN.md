# Origin Charts — Plugin & Primitives Platform (Design)

Status: **design for review — no code yet.** Companion to [ARCHITECTURE.md](ARCHITECTURE.md) and
[RENDERING_SPEC.md](RENDERING_SPEC.md). Tracks roadmap Phase C (extensibility).

Goal: match the reference charting library v5's headline extensibility — **series primitives**, **pane
primitives**, and **custom series** — so third-party drawings (trend lines, position tools,
annotations, custom bar styles) can be attached to a chart, while preserving Origin's dual-backend
(WebGPU + Canvas2D) rendering and headless engine.

---

## 1. What reference v5 exposes (the surface to match)

| reference concept | What it does |
|---|---|
| `ISeriesPrimitive` (`series.attachPrimitive`) | Draw extra content bound to a series' price scale (trend lines, markers, position tools). Provides `paneViews()`, `priceAxisViews()`, `timeAxisViews()`, `autoscaleInfo()`. |
| Pane primitive (`IPanePrimitive`) | Same, but bound to a pane rather than a series (background bands, session shading). |
| `ICustomSeriesPaneView` (`addCustomSeries`) | A user-defined **series type**: the engine owns its data/time-mapping/autoscale, the plugin renders each bar. |
| Renderer `draw(target)` / `drawBackground(target)` | The plugin paints into a `CanvasRenderingTarget2D` (a wrapper over the 2D context with media- and bitmap-space helpers). |
| `priceAxisViews` / `timeAxisViews` | Plugin-owned axis labels (e.g. a price tag on a trend line). |
| `autoscaleInfo()` | Lets a primitive expand the price range so its drawing stays in view. |
| `hitTest(x, y)` | Cursor interaction / custom cursors. |

reference primitives paint by calling arbitrary Canvas2D methods on `target`. **That is the crux of the
port** (see §3): Origin does not paint from a retained 2D context — it builds a backend-neutral
[`Prim`](../crates/origin_render/src/draw_list.rs) IR consumed by *either* the WebGPU or the Canvas2D
backend.

---

## 2. What we already have to build on

- **Backend-neutral draw IR** — `Prim` (`Rect`, `RectFrame`, `HLine`, `VLine`, `Polyline`,
  `AreaFill`, `RoundRect`, `Circle`, `Triangle`, `Background`, `Text`) with a shared `points` pool.
  A `FramePane` carries `under` / `main` layers + a `scissor`; the engine emits it and both backends
  consume it. This is exactly the vocabulary a "primitive" would need to emit.
- **Axis-label IR** — `AxisLabel { text, x, y, color, align, midpoint, bold, background }`.
  Plugin `priceAxisViews`/`timeAxisViews` map straight onto appended `AxisLabel`s.
- **Coordinate conversion, headless** — `time_scale.index_to_coordinate` / `coordinate_to_index`,
  `price_scale.price_to_coordinate` / `coordinate_to_price`, and the public series/chart
  `*_to_coordinate` methods already exposed through wasm + TS.
- **Autoscale hook point** — `autoscale_visible` already walks series to build the pane range; a
  primitive's `autoscaleInfo` is another contributor to that walk.
- **A JS→Rust callback pattern** — established for formatter callbacks (`js_sys::Function` stored
  host-side, called from the engine via a boxed closure). The same mechanism carries plugin draw
  callbacks.

---

## 3. The core decision: how does a JS plugin paint?

Three options. This is the decision that shapes everything else and needs sign-off.

### Option A — Prim-emitting plugins (cross-backend, GPU-capable)
The plugin returns a list of draw commands from the `Prim` vocabulary (plus a coordinate helper).
The engine folds them into the pane's `under`/`main` layer at a chosen z-index.

- **+** Works identically on WebGPU and Canvas2D; GPU-accelerated; z-orders *between* engine layers
  (e.g. a band behind the series) for free; no third canvas.
- **+** Deterministic, testable headlessly (assert emitted `Prim`s), like every existing feature.
- **−** Not drop-in reference-compatible: plugin authors emit structured commands, not raw canvas calls.
- **−** Limited to the `Prim` vocabulary until we extend it (e.g. dashed arcs, text runs, images).

### Option B — Canvas2D overlay plugins (reference-compatible, full 2D power)
Add a dedicated Canvas2D **plugin overlay** canvas above the pane; hand plugins a
`CanvasRenderingTarget2D`-shaped wrapper so reference renderers port almost verbatim.

- **+** Near drop-in reference parity; full arbitrary 2D drawing.
- **−** Always Canvas2D (no GPU) for plugin content; a second compositing surface.
- **−** Z-ordering is *whole-layer*: plugin content sits above the pane, so "behind the series"
  needs an extra under-overlay canvas (2 more surfaces) or is unsupported.
- **−** Per-frame JS drawing calls cross the wasm/JS boundary heavily; harder to test headlessly.

### Option C — Hybrid (recommended)
Ship **A first** as the native, cross-backend, testable primitive API (covers the 80%: lines,
rects, bands, markers, axis tags), and add **B as an opt-in escape hatch** ("canvas primitive")
later for plugins that need arbitrary 2D. Custom series (§4.3) build on A.

**Recommendation: C, starting with A.** It fits the existing architecture, keeps everything
headless-testable and GPU-fast, and defers the heavyweight overlay/compositing work until a real
plugin needs raw canvas. The main cost is that early plugins use a structured command API rather
than raw `ctx` calls — acceptable for a first platform cut.

> Decision needed from you: **A-first hybrid (C)**, or full reference-compatible **B** even at the cost of
> a Canvas2D-only, above-pane-only plugin layer?

---

## 4. Plugin surfaces (assuming Option A/C)

### 4.1 Series primitives
```
series.attach_primitive(primitive)   // returns a handle with .detach()
```
A primitive is a JS object the host adapts to an engine-side descriptor:
- `update_all_views()` — recompute cached geometry (called on data/scale/size change).
- `pane_views()` → renderer that, given a `PrimitiveDrawContext`, pushes `Prim`s at a `z_order`
  (`Bottom` = under series, `Normal`, `Top` = above crosshair).
- `price_axis_views()` / `time_axis_views()` → `AxisLabel`s.
- `autoscale_info(from, to)` → optional `{ min, max }` merged into the owning scale's range.
- `hit_test(x, y)` → optional cursor/interaction result (Phase C2).

`PrimitiveDrawContext` (passed to the renderer) exposes the headless converters the engine already
has: `price_to_y(price)`, `time_to_x(time)`, `logical_to_x`, pane width/height, DPR, and the shared
`points` pool builder.

### 4.2 Pane primitives
Identical, attached to a pane (`chart.panes()[i].attach_primitive`), not bound to a series scale;
used for session shading, watermarks (subsumes roadmap C3), grid overlays.

### 4.3 Custom series
```
chart.add_custom_series(pane_view, options)   // returns a series_api
```
The engine owns the series' data, time mapping, and autoscale (reusing `SeriesEntry` +
`DataLayer`). At frame build, instead of the built-in per-kind geometry, it calls the plugin's
`render_bars(bars, ctx)` which emits `Prim`s. `price_value_builder`/`is_whitespace` mirror reference so
the engine knows how to autoscale arbitrary custom data shapes.

---

## 5. Z-order, layers, and invalidation

- Extend `FramePane` consumption so primitive `Prim`s slot into `under` (Bottom), `main` (Normal),
  and a `top` layer (Top, above crosshair — the split already exists in `PaneLayers`).
- Reuse the existing invalidation tiers: primitive geometry rebuilds on Light/Full; `top`-layer
  primitives (and hit-test state) refresh on Cursor moves.
- Scissoring already clips to the pane; primitives inherit it.

## 6. JS boundary mechanics

- One retained `js_sys::Function`/object per attached primitive (host-side), called during
  `build_frame`, exactly like the formatter closures. The renderer returns a compact command buffer
  (typed array of tagged `Prim` records + a parallel `points` array) that Rust decodes into `Prim`s
  — one marshalling pass per primitive per frame, not per shape.
- Detaching drops the retained function (no leak; mirrors `remove_series`/formatter clear paths).

## 7. Testing

- Headless engine tests assert the `Prim`s / `AxisLabel`s a primitive contributes to `ChartFrame`
  (same style as the crosshair/grid/removal tests).
- A backend-parity Playwright fixture renders a reference primitive and checks WebGPU == Canvas2D.

## 8. Phased plan

| Phase | Scope | Rough size |
|---|---|---|
| **C-a** | Pane primitives, Option A: attach/detach, `pane_views`→`Prim` at z-order, headless tests. Ship one built-in (session-shading or watermark) as the reference. | M |
| **C-b** | Series primitives: bind to a series scale, `autoscale_info`, `price/time_axis_views`. | M |
| **C-c** | Custom series: engine-owned data + plugin `render_bars`. | L |
| **C-d** | Hit-testing + interaction (`hit_test`, cursor, click routing to primitives). | M |
| **C-e** | (Optional, Option B) Canvas2D escape-hatch primitive for raw-`ctx` reference ports. | L |

## 9. Open questions for review

1. **§3 decision** — A-first hybrid (recommended) vs full Canvas2D-overlay parity?
2. **Command-buffer vs per-shape calls** — accept the structured command-buffer marshalling (fast,
   testable) as the primitive API, or prioritize a raw-`ctx`-shaped API (reference-familiar) from day one?
3. **Custom-series priority** — is `addCustomSeries` needed early, or are primitives (annotations/
   drawings) the real near-term demand? This reorders C-b/C-c.
4. **`Prim` vocabulary gaps** — dashed/gradient strokes on arcs, image/bitmap prims, rich text runs:
   extend the IR as plugins need, or bound the first release to the current vocabulary?
