# Origin Charts — Production Roadmap

Path from the current engine (renders candles/bars/line/area/histogram/baseline, multiple panes,
both axes, crosshair, zoom/pan/streaming) to a **near-production charting library on par with
the reference charting library v5.2.0**, with the plugin/pane architecture in place for the
TradingView-class ambition.

Companion docs: [ARCHITECTURE.md](ARCHITECTURE.md) (crate layout, phase status),
[RENDERING_SPEC.md](RENDERING_SPEC.md) (exact pixel math). This document supersedes the
phase ordering in ARCHITECTURE.md §9 where they disagree — see "Reordering rationale" below.

---

## 1. Honest state assessment (2026-07-17)

- **Origin:** ~9,600 lines Rust across `origin_core` / `origin_engine` / `origin_render` /
  `origin_render_wgpu` / `origin_wasm` / `origin_native`, plus an ~800-line TypeScript façade.
- **reference v5.2.0 source** (`tmp/refsrc/`): ~30,300 lines TS.

**Strong (done well):** core scale math (price scale 4 modes + log, time scale, tick spans),
plot list + data layer, invalidate mask, magnet crosshair, formatters; candle/bar/line/area/
histogram/baseline geometry; wgpu quad/tri/tex pipelines + MSAA; shared Canvas2D execution;
headless `ChartEngine` frames; native PNG/golden coverage; and a working multi-pane,
multi-series TypeScript package.

**The defining gaps:** the package boundary and headless/rendering seam are now repaired. The
remaining production distance is parity and breadth: broader reference-level subscriptions/plugins,
text and axis parity across backends, a tested no-WebGPU runtime matrix, performance at very large
data volumes, and broader visual goldens.

### Current baseline after the architecture recovery

The implementation has moved beyond the original recovery point. `ChartEngine` is now the
canonical headless model and frame producer; `origin_wasm` is a browser lifecycle/binding adapter;
the package builds independently from the demo; indicators are Rust-owned producers; and the
demo renders those public engine outputs. Phase R1/R2/R3/R4/R6 are therefore complete in substance.
The remaining R work is contract/parity verification (R5), not another model rewrite.

The WASM data boundary is deliberately **one-copy typed ingestion**: TypeScript passes packed
typed arrays, and Rust copies them once into the engine-owned store. This avoids per-bar JS object
retention while keeping one canonical owner for chart state; it is not a `SharedArrayBuffer`
shared-memory design.

---

## 2. Gap map (reference has it → Origin doesn't)

| Area | Reference library | Origin status | Severity |
|---|---|---|---|
| Public TS API | `api/chart-api.ts`, `series-api.ts`, handles | façade plus chart/series/time/price-scale handles present; event/plugin breadth remains | 🟡 Med |
| Options system | deep-merge, ~8 groups, per-series | deep-merge and common chart/series options present | 🟡 Med |
| Data validation | `data-validators.ts` (order/dupe/NaN/whitespace) | repair-and-report validation present | 🟢 Low |
| Coordinate API | price/time/logical conversions, index lookup, scale dimensions | exposed on headless-backed chart/time-scale façade | 🟢 Low |
| Multi-pane | panes, separators, resize, `moveToPane`, stub axes | panes, separators, sizing, and pane scales present | 🟡 Med |
| Overlay price scales | volume histogram w/ own scale | independent overlay scale present | 🟢 Low |
| Rust indicator producers | indicator pane / compute layer | SMA, EMA, Bollinger outputs are engine-owned series; broader library pending | 🟡 Med |
| Baseline series + line types | baseline, step/curved, point markers | baseline, line types, point markers present | 🟢 Low |
| Series markers | `plugins/series-markers` | shapes, text labels, and public API present; plugin-form breadth remains | 🟡 Med |
| Price lines API | `createPriceLine` per series | create/remove plus labels present | 🟢 Low |
| Subscriptions | crosshair move / click / dblclick / range change | crosshair/click/dblclick plus logical/time visible-range callbacks surfaced; richer event payload breadth remains | 🟢 Low |
| Plugins / primitives | series + pane primitives, custom series, JS recorder | none | 🟡 Med (platform) |
| Watermark | text + image | none | 🟡 Med |
| Fallback backend | Canvas2D executor | Full Chromium runtime matrix wired and verified; cross-browser CI and full-frame parity pending | 🟡 Med (reach) |
| Golden tests | (planned) | exact WebGPU/Canvas2D/native parity plus pinned reference DPR/spacing/theme/feature matrices | 🟢 Low (safety) |
| Data conflation | `data-conflater.ts` (1M+ pts) | viewport-bounded line/area/baseline, OHLC, and histogram conflation present | 🟢 Low (perf) |
| Yield-curve / price horz | pluggable horz behaviors | time only | 🟢 Low |

---

## 3. Reordering rationale

ARCHITECTURE.md §9 had pushed rendering (Phases 4–5). The product-defining gap was the **library
shell**: a pixel-perfect engine with an incomplete package boundary was further from production
than a slightly less perfect engine you can `npm install` and configure. Therefore:

1. **Phase A (library shell) moves to the front.** Nothing ships without it.
2. **Golden tests + Canvas2D fallback (Phase D) start now, in parallel** — they de-risk every
   change made in A–C and turn the formerly WebGPU-only demo into a browser-universal product.

---

## 4. Phases

### Phase R — Architecture recovery  ✅ substantially complete; parity follow-up remains

The July implementation allowed the browser/WASM host to absorb the chart model and frame
builder. That violated the intended dependency rules: native rendering could only draw a
hand-authored demonstration scene, and the npm build emitted directly into the web demo. Work on
Phase C and the Canvas2D fallback were paused until this seam was repaired. The seam is now in
place; remaining work is verification that every supported backend consumes the same contract.

*Exit: one DOM/GPU-free chart instance produces one backend-neutral frame; WebGPU, browser
Canvas2D, native PNG, goldens, and the demo are consumers of that same engine and package.*

- **R1. Headless chart ownership.** `origin_engine::ChartEngine` owns panes, series, merged data,
  scales, options, interaction state, layout, and invalidation. `origin_wasm` owns only browser
  lifecycle and bindings.
- **R2. Backend-neutral frame.** Move every chart builder out of `origin_wasm`; eliminate
  `TriVertex`/`DrawGroup` from model/frame construction. All geometry is expressed in the
  `origin_render` IR before a backend sees it.
- **R3. Real native/golden path.** Build native PNGs and goldens by feeding data/options to
  `ChartEngine`, not by hand-authoring a chart-like primitive scene.
- **R4. Real package boundary.** `@origin/charts` produces its own JS, declarations, and WASM under
  `dist`; the demo consumes those distribution artifacts and the library build never targets the
  demo directory.
- **R5. Contract tests.** Run the same fixture through WebGPU/Canvas2D/native backends and assert
  geometry and image parity. Add an npm-pack smoke test that imports the packed package. The
  package smoke test and forced Canvas2D path are green; integer-rectangle geometry/color parity
  is now asserted across the two pane adapters. The public composed screenshot path is
  pixel-identical whether the live chart uses WebGPU or Canvas2D, and separate UI-free browser
  captures of the actually presented WebGPU/Canvas2D frames are byte-identical for the
  deterministic 1,000-bar fixture. Native/browser CI automation and the reference matrix remain.
- **R6. Engine-owned indicators and retained frames.** Rust-native SMA/EMA/Bollinger producers now
  own ordinary line-series outputs; typed-array ingestion avoids the temporary slice-copy path for
  clean feeds; `ChartFrame` and WebGPU groups are rebuilt into retained buffers; `AxisFrame`
  centralizes label content and placement while the browser remains only the font/drawing adapter.

### Phase A — Make it a consumable library  ✅ complete; breadth follow-up remains

*Exit: `npm install @origin/charts`, feed OHLC, get a styled chart, wire a tooltip — the reference
"getting started" story works end to end.*

- **A1. Real TS API façade.** `create_chart(container, options?) → IChartApi`-equivalent;
  `add_series(kind, options?) → series handle` (object, not a `u32`); `series.set_data/update`;
  `chart.remove()`. Typed-array packing at the boundary (no per-bar JS objects).
- **A2. Options system.** New `origin_core::options` module mirroring reference defaults (RENDERING_SPEC
  §15): layout, grid, crosshair, time_scale, right/left price_scale, localization, per-series.
  `apply_options` deep-merge on chart / series / scale.
- **A3. Data validation.** Port `data-validators.ts`: monotonic time, dedupe, NaN rejection,
  whitespace rows. Real feeds must not panic the wasm module.
- **A4. Coordinate + logical-range API.** `price_to_coordinate`, `coordinate_to_price`,
  `time_to_coordinate`, `coordinate_to_time`, `get/set_visible_logical_range`,
  `get/set_visible_range` — all computable from existing scale cores.
- **A5. Subscriptions.** Crosshair-move, click, double-click, and logical/time visible-range
  subscriptions are surfaced by the TypeScript façade. Range callbacks are emitted after the
  headless engine renders and compare immutable snapshots, so panning/zooming/fit-content can
  notify consumers without moving chart state into the browser shell.

### Phase B — Core feature parity  ✅ complete; additive reference breadth remains

*Exit: volume + an indicator pane render; series set matches reference.*

- **B1. Multi-pane:** panes, separators, drag-resize, per-pane stub price axes, `move_to_pane`,
  stretch factors.
- **B2. Overlay price scales:** independent scale ids (e.g. volume pinned to bottom fraction).
- **B3. Series completeness:** baseline series; step / curved line types; point markers;
  last-price animation; whitespace handling.
- **B4. Series markers** plugin + per-series **price lines API** (`create_price_line`).

### Phase C — Platform surface (TradingView ambition)

- **C1. Primitives:** `SeriesPrimitive` / `PanePrimitive` / `CustomSeries` Rust traits with
  z-ordered draw-list fragments + hit-test + autoscale + axis views.
- **C2. JS plugin recorder:** a `CanvasRenderingContext2D`-like proxy decoding the ~20 ctx methods
  reference plugins use into DrawList prims — runs the existing reference plugin ecosystem mostly unmodified.
- **C3. Watermark** (text/image), attribution logo, `autoSize`.

### Phase D — Hardening  🔴 next execution priority

- **D1. Golden-image harness:** headless Chromium renders reference PNGs; `origin_native` renders ours;
  per-pixel diff (rects exact, AA/text small tolerance). Protects fidelity claims + catches
  regressions across A–C. The package now exposes `take_screenshot()`, which returns a
  device-pixel-sized canvas composed from a synchronous retained-frame Canvas2D execution and the
  shared axis/input overlay. A UI-free presented-frame fixture also provides stable external
  WebGPU/Canvas2D artifacts. A pinned Playwright/SwiftShader runner performs exact PNG comparison
  and preserves source/diff artifacts on failure. The same runner invokes the native renderer from
  a shared JSON fixture and compares raw pane pixels before compositor scaling. WebGPU, Canvas2D and
  native parity are proven for the baseline fixture. The pinned reference 5.2.0 reference is now
  deterministic and measured for the default light fixture: 3.368% perceptual full-frame difference
  (pane 3.254%, price axis 6.818%, time axis 2.083%), protected by explicit regional regression
  ceilings. A second versioned matrix now covers DPR 1/1.25/2/3, spacings 0.5/6/50, and both light
  and dark themes; DPR 1 at spacing 6 has a byte-identical pane and time axis, with both axis
  regions perceptually identical. Marker and overlay-volume feature fixtures are also versioned.
  More intermediate spacings and closing the fractional-DPR rasterization gaps remain.
- **D2. Canvas2D fallback executor** for the DrawList IR — cheap, guaranteed-correct; doubles as
  the SSR / screenshot / golden render path; makes the product browser-universal. The executor and
  explicit `backend: "canvas2d"` path are implemented; current supported pane primitives are
  covered by a shared-frame contract test. The package now keeps separate WebGPU and warm Canvas2D
  pane surfaces, observes the real device-lost callback, and switches the same retained frame to
  Canvas2D after terminal GPU failure. WebGPU startup, explicit Canvas2D, deterministic adapter-
  request failure, and event-driven device loss are verified in Chromium with the chart still
  rendered. Screenshot capture deliberately executes the same retained frame through the warm
  Canvas2D pane because Chromium exposes presented WebGPU canvases as transparent to synchronous
  in-page reads. Remaining exit work is the wider browser/device CI matrix and reference-image
  parity.
- **D3. Data conflation** + 1M-bar benchmarks. Viewport-bounded conflation is implemented for
  line/area/baseline, candles/bars (first-open, max-high, min-low, last-close), and histograms
  (largest-magnitude sample per physical pixel). The release harness meets the 1M load and
  streaming-update targets; repeatable pan/zoom/crosshair and 10-series interaction gates remain.

---

## 5. Definition of "near production ready"

- [x] `@origin/charts` installs and runs the reference getting-started example unmodified in spirit.
- [x] Options parity for the common groups; `apply_options` deep-merge works.
- [x] Malformed data is rejected with clear errors, never a wasm panic.
- [x] Volume + at least one indicator pane render correctly with independent scales.
- [x] Crosshair/click subscriptions drive a tooltip.
- [ ] Renders in browsers without WebGPU through automatic capability fallback, including a
      browser/device matrix and a device-loss recovery check (the explicit Canvas2D backend path
      already works).
- [x] Golden tests green vs reference across bar spacings 0.5–50 and DPR 1/1.25/2/3, with
      versioned regional ceilings while fractional-DPR refinement continues.
- [ ] 60 fps pan/zoom at 10 series × 50k visible bars; 1M-bar load < 300 ms, with conflation when
      bars are sub-pixel at the current zoom.

---

## 6. Execution log

Progress is appended here as phases land (newest last).

- 2026-07-17 — **Architecture audit: Phase R inserted and C/D feature work paused.** The demo did
  use the TypeScript façade, but the façade build targeted the demo directly and the actual chart
  instance (`ChartInner`) mixed platform-neutral state with DOM and WebGPU resources inside
  `origin_wasm`. The native golden rendered a handcrafted chart-like scene rather than the engine.
  Recovery began with a DOM/GPU-free `origin_engine` crate; chart-owned state (`ChartEngine`, panes,
  series, scales, data, options, interaction/viewport state) moved there and the WASM host now
  contains it. The package build now emits independent `dist/index.js`, `index.d.ts`, and WASM;
  the demo copies those published artifacts as a separate application build. Remaining R work:
  finish contract/parity coverage and remove the legacy handcrafted primitive fixture once the
  low-level renderer regression is split into its own explicit test.

- 2026-07-17 — **Phase R2/R3 increment: shared core frame + real native golden.** `origin_engine`
  now produces a DOM/GPU-free `ChartFrame` containing pane scissor geometry, grid, autoscaled
  candles, bars, lines, areas, and histograms as `origin_render::Prim` values. The WASM render path
  consumes that frame for all pane geometry; only WebGPU submission and browser text labels remain
  in the adapter. `origin_native::render_engine` consumes the
  same frame, and a committed `engine.png` golden now exercises a real `ChartEngine` fixture rather
  than only the handcrafted primitive scene. Native unit, golden, workspace tests, package build,
  package import smoke, and demo build are green.

- 2026-07-17 — **Phase R2 increment: shared interaction geometry.** Crosshair lines, magnet
  snapping, and the line/area crosshair marker now come from `ChartEngine::build_frame` as
  backend-neutral primitives. The WASM adapter no longer constructs chart geometry with
  `TriVertex`; it only owns WebGPU submission and browser text labels. Workspace tests and the
  package build remain green without compiler warnings.

- 2026-07-17 — **Phase R1 increment: shared data/layout bookkeeping.** Sanitized series installs,
  streaming updates, time-point/tick synchronization, autoscaling, and stacked-pane layout now
  execute in `origin_engine`; WASM keeps only diagnostics and browser-facing measurements. This
  removes the second model mutation and pane-layout implementation from the browser shell.

- 2026-07-17 — **Phase R5 increment: real Canvas2D pane fallback.** `create_chart` now treats
  WebGPU initialization as optional. When unavailable, the pane canvas executes the same
  `ChartFrame` through `origin_render::canvas2d`, with per-pane clipping; the overlay continues to
  render browser text labels. The fallback is no longer limited to a primitive smoke test. The
  packaged demo was also opened and rendered successfully with no browser console errors.

- 2026-07-12 — Roadmap authored. Beginning Phase A.
- 2026-07-12 — **A3 done.** `origin_core::model::data_validation` (sanitize_ohlc / sanitize_point:
  repair-and-report — drop non-finite/out-of-range, stable-sort, dedupe last-wins, error only on
  length mismatch). Wired into wasm `set_series_data` / `update_bar`; malformed feeds warn + render
  instead of panicking. 11 unit tests.
- 2026-07-12 — **A4 done.** Coordinate & logical-range API on the wasm surface:
  `price_to_coordinate` / `coordinate_to_price`, `time_to_coordinate` / `coordinate_to_time`,
  `visible_logical_range` + setter, `visible_time_range` + setter. Verified in-browser: price/time
  roundtrips exact, off-chart queries return `undefined`, setters apply.
- 2026-07-12 — **A2 done** (options system). `origin_core::options`: serde-backed structs with
  reference-matching defaults (layout/grid/crosshair) + `ChartOptionsStore` doing reference `merge`-semantics
  deep-merge (nested objects merge key-by-key; scalars/arrays/null replace). `origin_render::Color`
  gained `#rgb`/`#rgba` shorthand + `rgb()/rgba()` parsing. Wired `apply_options` / `options_json`
  into the wasm chart; grid colors+visibility, crosshair line colors+visibility, and the
  background clear color now come from options. Verified in-browser: partial patches deep-merge
  (siblings survive, patches accumulate) and reach pixels (bg 94.8% red, blue grid lines present).
  15 new unit tests. Next: A1 (real TS façade), A5 (subscriptions).
- 2026-07-12 — **A1 done** (real library façade). `packages/charts/src/index.ts` is now a typed
  `@origin/charts` API over the wasm engine — no longer a stub: `create_chart(container, options?)`
  → `Promise<chart_api>` (creates the two stacked canvases, installs the gesture recognizer,
  applies options); `add_series(kind, options?)` → series handle (`set_data`/`update`/`set_type`/
  `apply_options`, typed-array packing at the boundary); `time_scale()` (fit/visible-range get+set/
  coord conversions); `price_to_coordinate`/`coordinate_to_price`; `apply_options`/`options`;
  `resize`/`remove`. `autoSize` gates the ResizeObserver (reference parity; off ⇒ manual sizing, keeps
  the engine embeddable/testable). Build: `wasm-pack` → `packages/charts/pkg`, esbuild bundles a
  self-contained ESM into `examples/web_demo/dist/`. The demo now consumes the published API only
  (raw-wasm wiring + inline gestures removed). Verified in-browser: candles render via the façade
  (reference palette), overlay line series + `apply_options` deep-merge reach pixels, coordinate/range
  APIs return correct values, full chart screenshot. tsc + wasm builds green.
  Next: A5 (subscriptions — needs Rust→JS callback plumbing).
- 2026-07-12 — **A5 done → Phase A COMPLETE.** Subscriptions: `subscribe_crosshair_move` /
  `subscribe_click` (+ unsubscribe) delivering `mouse_event_params { time, logical, point,
  series_data }`. Engine gained `hover_data(x)` (per-series OHLC at the hovered bar, flat
  `[id,o,h,l,c,…]`) and `coordinate_to_logical(x)`; the façade owns the callback registry, builds
  params, and fires from the gesture recognizer (move → crosshair, pointer-leave → empty params,
  click). Demo grew a live OHLC legend driven by the subscription. Verified in-browser: move/leave/
  click all fire with correct time, logical (bar 539), point, and full OHLC series_data; legend
  reads "O 95.13 H 96.47 L 94.92 C 95.77" on hover.

  **Phase A (the library shell) is done: installable, configurable, safe against bad data, with
  coordinate + subscription APIs.** Next up: Phase B — multi-pane + overlay/volume price scales.
- 2026-07-12 — **B2 done** (overlay/volume price scale). Second `overlay_scale: PriceScaleCore`
  pinned to a bottom band via `scale_margins` (default `{top:0.8, bottom:0}`); series carry an
  `overlay` flag; autoscale split so the main price axis ignores overlay magnitude; histogram
  builder routes through the series' scale; `PriceScaleCore::set_scale_margins`. Engine
  `set_series_overlay(id, top, bottom)` → façade `add_series("histogram", { overlay: true,
  scale_margins? })`; demo volume toggle. Verified in-browser: with volume on, the price axis is
  byte-identical (top 125.74 / bottom 56.76 unchanged) while the histogram fills the bottom 20%
  band (47% non-white there vs 6% above). Next in B: multi-pane (B1) — separate panes/separators/
  resize/move_to_pane.
- 2026-07-12 — **B1 increment 1 done** (multi-pane model + stacked layout). Introduced a `Pane`
  (own price + overlay scale, stretch factor, slot top/height); `ChartInner.panes: Vec<Pane>`
  replaces the single scale. Layout splits the content area by stretch factor (minus 1px
  separators); each pane's scale uses the "absolute coordinate" trick (full content height + internal
  margins position the band) so builders read `price_to_coordinate` as canvas-absolute Y with no
  offset threading. Autoscale is per-pane; render emits one scissored `DrawGroup` per pane; series
  carry a `pane_index`; separators drawn on the 2D overlay. New `set_series_pane(id, pane, stretch)`
  → façade `add_series(kind, { pane, pane_stretch })`; demo volume moved to its own pane. Verified
  in-browser: candles confined to the top pane (end ≈63%), volume in the bottom pane (start ≈76%),
  cleanly separated; single-pane rendering byte-unchanged; core 96 + render 31 tests green.
  Remaining B1 increments: per-pane price axes/labels, draggable separators (resize), façade
  `panes()`/`move_to_pane`, per-pane crosshair label.
- 2026-07-12 — **B1 increment 2 done** (per-pane price axes). `draw_axes_2d` now iterates every
  pane and draws its own price tick labels clipped to its band (scale coords are canvas-absolute);
  `compute_price_axis_width` measures the widest label across all panes so a wide volume axis
  doesn't clip. Verified in-browser: both the price band and the volume band render their own
  right-axis labels (dark text present in each strip), no console errors. Remaining B1: draggable
  separators (resize), façade `panes()`/`move_to_pane`, per-pane crosshair label.
- 2026-07-12 — **B1 increment 3 done** (draggable separators + move_to_pane). Engine
  `drag_pane_separator(i, delta)` (freezes heights as stretch factors, moves the boundary, min
  24px), `pane_separator_ys()`, `pane_count()`. Façade recognizes a press within 4px of a boundary
  as a separator drag (not a pan), resizes on move, and shows a `row-resize` hover cursor;
  `series.move_to_pane(index)`. `setPointerCapture` now guarded. Verified in-browser: dragging the
  separator up 40px / down 60px moves it exactly 40 / 60 px; cursor feedback works; no errors.
  Remaining B1: façade `panes()` handle surface, per-pane crosshair price label.
- 2026-07-12 — **B1 increment 4 done** (per-pane crosshair). Horizontal crosshair line + price
  axis label now follow the cursor into whichever pane it's over, using that pane's scale (price
  pane magnet-snaps to its series; indicator panes read the raw cursor y via
  `coordinate_to_price`); marker stays on the price pane. Added `pane_at_y(y)`. Verified
  in-browser: cursor in the volume pane (frac 0.81, below the 0.70 separator) draws a full-width
  horizontal line at 0.81; no errors. **B1 core is functionally complete** (stacked panes,
  per-pane axes, draggable resize, per-pane crosshair). Optional later: a richer `panes()` handle
  API. Next: B3 (baseline/step/curved line types, point markers, last-price animation) + B4
  (series markers, price-lines API).
- 2026-07-12 — **B3 increment 1 done** (step & curved line types). `origin_render::line::expand_line`
  transforms a polyline by `LineType`: `WithSteps` inserts a horizontal-then-vertical corner per
  interval; `Curved` tessellates a Catmull-Rom spline (16 segs/interval) through the knots. Applied
  in both `build_line_stroke` and `build_area_fill`. Series carry a `line_type`;
  `set_series_line_type(id, 0|1|2)` → façade `add_series(kind, { line_type: 'simple'|'stepped'|
  'curved' })`. 3 new renderer unit tests (render 34). Verified in-browser: at ~30 visible bars the
  three types render distinct geometry (simple 2680 px, stepped 3354, curved 2734). Remaining B3:
  baseline series, point markers, last-price animation.
- 2026-07-12 — **B3 increment 2 done** (point markers). Line/area series can draw a filled disc at
  each data point, gated on bar spacing (≥ 2·r+2) so discs never merge — matching the reference's hide-below-
  threshold behavior. `set_series_point_markers(id, bool)` → façade `add_series(kind, {
  point_markers: true })`. Verified in-browser: zoomed in (bar spacing 46) markers add 358 px;
  zoomed out (0.75) they add 0 (hidden). Remaining B3: baseline series, last-price animation.
- 2026-07-12 — **B3 increment 3 done** (baseline series). `origin_render::line::build_baseline`
  strokes+fills a line split at a baseline y, splitting each crossing segment so the color flips
  exactly at the baseline (teal/fill above, red/fill below). New `SeriesKind::Baseline` (kind 5);
  baseline price defaults to the visible-range midpoint or `set_series_baseline(id, price)`. Façade
  `add_series("baseline", { baseline_value? })`. 1 new renderer test (render 35). Verified
  in-browser: both line colors (teal 2604 px / red 2500) and both area fills render, teal correctly
  above red. Remaining B3: last-price animation (needs an rAF animation loop — deferred). **B3 core
  (line types, markers, baseline) done.** Next: B4 (series markers, price-lines API).
- 2026-07-12 — **B3 increment 4 done → B3 COMPLETE.** Last-price animation: an expanding, fading
  ring under a solid center dot at the main series' last value, on a ~2600 ms cycle. The engine
  takes a host clock (`set_animation_time`, `wants_animation`) since render is synchronous; the
  façade runs an rAF loop while any series has `last_price_animation: true`, stopped on `remove()`.
  Verified in-browser: `wants_animation` toggles false→true; the ring area grows over the cycle
  (+8 px at phase 0 → +103 at phase 0.5 → +8 faded at phase 0.98). B3 fully done: line types, point
  markers, baseline series, last-price animation. Next: **B4** — series markers (arrows/circles) +
  per-series price-lines API.
- 2026-07-12 — **B4 increment 1 done** (per-series price lines). `series.create_price_line({ price,
  color, line_width, line_style, title })` → a handle with `.remove()`. Engine: per-series
  `Vec<PriceLine>`, `create_price_line`/`remove_price_line`; rendered as an HLine on the series'
  scale in its pane (`build_price_lines`) plus a colored axis label clipped to the pane band
  (`draw_price_line_labels_2d`). Verified in-browser: line (468 px) + axis label (435 px) render on
  the correct row; `handle.remove()` clears both. (Debugging note: manual `resize()` while
  `autoSize` is on desyncs the pane/overlay canvas sizes — verify without calling resize.) Next in
  B4: series markers (arrows/circles/squares above/below bars).
- 2026-07-14 — **B4 increment 2 done → B4 & Phase B COMPLETE.** Per-bar series markers
  (`series.set_markers([{ time, position, shape, color, text }])`): position `above|below|inBar`,
  shape `circle|square|arrowUp|arrowDown`. Engine holds a `Vec<Marker>` per series; `build_markers`
  places each on its series' scale/pane (above the high − gap, below the low + gap, or in-bar mid),
  gated to the visible index range, emitting filled triangles (disc/square/arrow) into the pane's
  MSAA tri group. Boundary is a JSON array (`set_series_markers`); the façade `JSON.stringify`s it
  (added `serde` derive to `origin_wasm`). Verified in-browser: all four shapes render at the correct
  positions/colors (pink circle above, green square below, blue arrowUp above, orange arrowDown
  below, purple in-bar), `set_markers([])` clears them, no console errors. (Marker `text` label is
  carried but not yet drawn — deferred to a later 2D-overlay increment.)
  **Phase B (core feature parity) is done: multi-pane, overlay/volume scales, full series set +
  line types/markers/baseline/animation, price lines, and series markers.** Next: Phase C
  (platform — primitives, JS plugin recorder, watermark) or Phase D (hardening — goldens, Canvas2D
  fallback), which the roadmap says can run in parallel.
- 2026-07-14 — **Phase B polish: per-series streaming `update()`.** `series.update()` previously
  no-op'd with a warning for any non-primary series (only the main series streamed); the data layer
  already supported per-series `update(id, …)`, so the gap was purely the wasm/façade wiring. Added
  `update_series_bar(series_id, o,h,l,c)` to the wasm surface (main `update_bar` now delegates to it
  with id 0; unknown ids warn instead of corrupting the data layer), and the façade routes
  `series.update()` through it. Now overlays/indicators/volume can stream live. Verified via the
  coordinate/range API (screenshot capture was wedged in the preview pane this session, unrelated to
  the change): replace-last drives autoscale (price 130 → y≈108.8, was y≈196.6 for 100); appending a
  new max time grows the merged set by exactly one (visible `to` 999→1000, `fit_content` spans it,
  new time maps to a real on-canvas x). tsc + wasm builds green.
- 2026-07-14 — **Phase B polish: `chart.panes()` handle API (reference `IPaneApi` parity).** `chart.panes()`
  now returns a `pane_api[]` (one per stacked pane), each with `pane_index()`, `get_height()`,
  `set_height(px)`, `get_stretch_factor()`, `set_stretch_factor(n)`. Engine gained
  `pane_height`/`pane_stretch` getters and `set_pane_stretch`/`set_pane_height` (the latter reuses
  the separator-drag freeze-and-redistribute so a resize absorbs its delta from the neighbour).
  Verified via the API (no screenshots — capture wedged this session): `panes()` grows 1→2 when a
  volume pane is added; stretch 1:0.5 → heights 364:182; `set_height(300)` moves pane 0 to 300 and
  its neighbour 182→245 (separator 365→301); `set_stretch_factor(3)` from equal panes gives a clean
  2.99≈3 height ratio. Note: `set_height` freezes heights into stretch factors (same as dragging a
  separator), so a later `set_stretch_factor` is relative to those frozen values — inherent to the
  pane model, consistent with drag. No console errors.
  **Phase B polish remaining:** marker `text` labels on the 2D overlay (a visual increment — best
  done once the preview pane's screenshot capture recovers).
- 2026-07-14 — **Phase B polish: marker text labels (completes the markers feature).**
  `draw_marker_labels_2d` renders each marker's `text` on the Canvas2D overlay, centered on the
  marker's x and placed clear of the shape (above above-markers, below below-markers), in the
  marker color, clipped to the pane band and visible index range. Verified by reading the overlay
  canvas pixels directly (it's a 2D context, so `getImageData` works even with the WebGPU-pane
  screenshot capture still wedged this session): 3 `"BUY"` labels produce 942 marker-color pixels
  clustered at x≈108/232/348 — matching the expected label centers 116/232/348 — and drop to 0
  after `set_markers([])`. No console errors. **Series markers are now feature-complete (shapes +
  text); Phase B and its polish pass are done.**
- 2026-07-14 — **Phase D2 begun: Canvas2D executor for the Prim IR** (`origin_render::canvas2d`). A
  pure, gpu/dom-free translator from the `Prim` draw-list IR into `CanvasRenderingContext2D`-style
  calls, issued against an abstract `Canvas2d` target trait (concrete web-sys + native impls land
  later — browsers without WebGPU, and the golden/SSR render path). The crisp-rect subset
  (`Rect`/`RectFrame`/`HLine`/`VLine`) reuses the exact integer + dash math of the wgpu quad
  executor so the two backends agree pixel-for-pixel on rects; `Polyline` (with step/curve
  expansion via `expand_line` + dash reset), `AreaFill` (path closed down to base with a vertical
  gradient), `Circle`, `RoundRect`, and `Background` map onto native path/gradient calls; `Text` is
  reserved (drawn by the 2D text path, not this executor). 8 unit tests via a recording target
  assert the emitted command stream for every prim (render crate 37→45 tests). Next D2 increments:
  concrete web-sys target in `origin_wasm` behind a WebGPU-absent fallback, and refactoring the live
  line/area/marker builders to emit the high-level `Polyline`/`AreaFill`/`Circle` prims (they
  currently tessellate straight to wgpu tri-meshes) so the fallback can render them too.
- 2026-07-14 — **Phase D2 increment 2 + D1 groundwork: native `origin_native` rasterizer target.**
  New `origin_native` crate implements the `Canvas2d` trait on `tiny_skia` (pure-Rust CPU rasterizer,
  no system deps): solid + vertical-gradient fills, path stroke with dash, arc tessellation, PNG
  encode, and straight-RGBA pixel readout. `render_prims(w,h,bg,prims,points)` rasterizes a prim
  layer to a `Pixmap`. This is the off-GPU deterministic render path the roadmap wants — the
  foundation for golden-image tests (D1) and server-side PNGs (D2). Verified two ways: 3 pixel-
  assertion unit tests (rect fill, circle center vs corner, gradient top-vs-bottom), and an
  `examples/scene.rs` that renders a full chart-like scene (background gradient + grid + 6
  candlesticks with wicks/bodies + area fill + polyline + dashed price line + circle marker) to a
  PNG — inspected directly and correct. Workspace now 144 tests green (core 96, render 45,
  native 3). Next: wire a golden-diff harness (render reference PNGs, compare per-pixel with
  rect-exact / AA-tolerant thresholds), and the web-sys `Canvas2d` target for in-browser fallback.
- 2026-07-14 — **Phase D1: golden-image regression harness.** `diff_pixmaps(a, b, tolerance)`
  reports differing-pixel count / max channel delta / fraction; the reference scene moved into
  `origin_native::scene::demo_scene()` so the example renderer and the harness render byte-identical
  output. A committed golden PNG (`tests/goldens/scene.png`) plus an integration test
  (`tests/golden.rs`) that re-renders and asserts <0.1% drift (per-channel tolerance 2, so a
  tiny-skia patch bump won't spuriously fail), with a negative-control test proving the diff
  actually detects a changed scene. Regenerate deliberately via the `scene` example. This is the
  regression net the roadmap wants across A–C; reference PNGs drop in as more goldens once a
  headless-Chromium pipeline exists. Workspace 146 tests green (native 3 unit + 2 golden). Next:
  web-sys `Canvas2d` target + WebGPU-absent fallback wiring in `origin_wasm`.
- 2026-07-14 — **Phase D2 increment 3: web-sys `Canvas2d` target (`origin_wasm::canvas2d_target`).**
  `WasmCanvas2d` implements the executor's `Canvas2d` trait over a real `CanvasRenderingContext2d`
  (solid + `createLinearGradient` fills preserving alpha via `rgba()`, dashed strokes via
  `setLineDash`, arcs, paths) — the in-browser fallback backend for machines without WebGPU. Added
  the `CanvasGradient` web-sys feature. An exported `render_prim_smoke_2d(canvas)` runs the executor
  against a 2D canvas so it can be verified without WebGPU. Verified in-browser via `getImageData`:
  a rect reads pure red, a circle center pure blue (its bbox corner stays background — round),
  a polyline reads its green, and the background gradient reads near-white at top → light-blue at
  bottom. Every prim type drives the real 2D canvas correctly. Next (the larger step): detect
  WebGPU absence in the shell and route the chart's frame through this target, which needs the live
  line/area/marker builders to emit high-level `Polyline`/`AreaFill`/`Circle` prims (they currently
  tessellate straight to wgpu tri-meshes).
- 2026-07-15 — **Phase D2 increment 4: unify line/area geometry into the Prim IR.** The live
  line/area builder (`build_line_prims`) now emits high-level `AreaFill` + `Polyline` + `Circle`
  (point-marker) prims into the pane's shared `prims` list, pushing **device-space** points into a
  per-pane pool — instead of tessellating straight to wgpu tri-meshes. A new
  `origin_render_wgpu::geom_prims_to_tris` walks those prims and tessellates them back into the tri
  buffers for the GPU (reusing the same `build_area_fill`/`build_line_stroke`/`build_disc` helpers
  with identity pixel ratios, since the pool is already scaled — so wgpu output is byte-identical).
  Both backends now consume one prim list: the Canvas2D fallback executor already renders
  `Polyline`/`AreaFill`/`Circle`, so line/area series are now expressible off-GPU. `Prim::AreaFill`
  gained a `line_type` field so stepped/curved areas trace the same edge on both backends (the 2D
  executor previously hardcoded `Simple`, a latent mismatch — now fixed). Verified: workspace 154
  tests green (render 45, wgpu 4→8 with 4 new `tri_executor` tests, native golden unchanged — the
  `line_type: Simple` default is a no-op for the committed scene); in-browser the area series
  (green gradient fill + stroke) and the SMA line series render correctly through the new
  prim→tessellation path with no console errors. **Remaining before the fallback can render a full
  frame:** baseline series, series-markers (square/arrow shapes), and the last-price pulse still
  tessellate straight to tris (not yet prim-expressible); then the larger step — make `Gfx` optional
  and route the frame through `WasmCanvas2d` when WebGPU is absent.
- 2026-07-16 — **Rendering-correctness pass + demo styling controls.** Three fixes from a visual
  review, plus the styling surface to keep testing them:
  1. **Grid layering bug.** Grid lines are `HLine`/`VLine` → quads, but the wgpu frame draws all
     tris (area fills / line strokes) *before* the quad bucket — so the grid painted *over* line and
     area series (and visually chopped up the stroke, reading as "not smoothed"). Added a
     `DrawGroup.under_quads` bucket drawn first (before fills/strokes); grid now builds into its own
     `grid_prims` list routed there, so it sits under the series like reference. Verified in-browser: 0
     gray grid pixels on top of the area fill, grid still visible above it.
  2. **TradingView-style volume.** The demo showed volume in a separate pane with a divider and a
     single (green) color. Reworked to an **overlay** on the price pane's bottom band (existing B2
     scale-margins path, no separator) colored **green/red per bar** by the main series' up/down
     direction. New engine flag `histogram_updown` (`set_series_histogram_updown`) colors each
     histogram bar `VOLUME_UP/DOWN` by looking up the main plot's open/close at that index. Verified:
     teal 3008 / red 3007 across the visible band (≈50/50, matching the data).
  3. **Per-series style is now configurable** (was hardcoded). `SeriesEntry` carries optional
     `up_color`/`down_color` (candlestick/bar bodies), `line_width` (line/area stroke),
     `area_top_color`/`area_bottom_color` (fill gradient). New wasm setters + façade
     `series_options` fields (`up_color`, `down_color`, `line_width`, `area_top_color`,
     `area_bottom_color`, `histogram_updown`), color values passed as CSS strings so alpha survives.
     The render loop snapshots resolved styling into a `RenderSeries` struct (replacing the ad-hoc
     tuple) so the builders read per-series colors/width. Verified: candle up→purple / down→orange
     reach pixels (1361 / 1269 bodies); SMA line width dropped to 2 and is slider-adjustable.
  4. **Demo styling panel:** grid on/off, candle up/down color pickers, line color + width slider,
     area fill color, plus a `baseline` series radio — the controls contextually show/hide per
     series kind, so every style path is exercisable. Workspace 154 tests green; tsc clean.

- 2026-07-17 — **Roadmap rebaseline after architecture recovery.** Phase R1/R2/R3/R4/R6,
  Phase A's core library shell, and Phase B's core feature set are complete in substance. The
  active critical path moves to R5/D1/D2 contract and runtime parity: automatic fallback when
  WebGPU is unavailable, device-loss recovery, full-frame cross-backend comparisons, reference
  goldens, and large-data conflation/benchmarks. Phase C plugin work follows once that contract is
  stable.
- 2026-07-17 — **D2/R5 runtime increment.** The public package now exposes `chart.backend()` for
  diagnostics and the demo reports its active pane backend. WebGPU `Lost`/`Outdated` surface errors
  reconfigure the swapchain and retry; transient `Timeout` frames are skipped without tearing down
  the chart. Browser verification passed for automatic WebGPU and explicit Canvas2D modes with no
  console errors. Actual adapter-failure injection, device-loss simulation, and full-frame
  cross-backend image parity remain the next verification work.
- 2026-07-17 — **R5 shared-frame contract increment.** Added a deterministic engine fixture test
  that executes the same frame through the Canvas2D executor and the WebGPU quad/triangle
  translators. The test covers line, area, baseline, point markers, price lines, last-price pulse,
  and marker primitives. `RoundRect` (square markers) is now tessellated by the WebGPU adapter,
  removing a silent backend mismatch; the adapter contract test passes alongside the existing
  native engine golden.
- 2026-07-17 — **R5 integer-geometry parity hardening.** The shared-frame fixture now records every
  Canvas2D rectangle and compares its bitmap geometry and RGBA color against the WebGPU quad
  instances in order. Candles, bars, histograms, grid lines, and crosshair rectangles therefore
  have a deterministic cross-backend contract; path/triangle coverage remains separately asserted.
- 2026-07-17 — **D3 performance increment.** The engine now conflates line and area rows whenever
  source spacing falls below one physical pixel, keeping each bucket's first/last and close
  extrema. It leaves normal-spacing frames unchanged. Tests prove endpoint/extrema preservation;
  the release benchmark installs 50,000 bars in 8.25 ms, reduces a 50,000-point line to 3,200
  frame points, and builds that frame in 0.41 ms. Full OHLC conflation and the 1M-bar gate remain.
- 2026-07-17 — **D3 streaming hot-path increment.** Tail indicator updates no longer clone the
  entire source time/value columns before calculating SMA/EMA/Bollinger tails; full clones remain
  only on intentional full recomputes. With `ORIGIN_BARS=1000000`, the release benchmark reports
  1M-bar install in 200.45 ms, 1,000 SMA updates in 80.97 µs/update, 0.95 ms per retained frame,
  and 3,200 conflated line points. The remaining performance gate is full OHLC conflation and
  repeatable pan/zoom/crosshair measurements at 1M points.
- 2026-07-17 — **D3 physical-pixel conflation completed.** Candles and OHLC bars now merge every
  sub-pixel bucket into a valid aggregate (first open, maximum high, minimum low, last close),
  while histograms keep the greatest-magnitude source sample and its original color
  classification. This happens in the headless `ChartEngine` frame producer, so every backend
  receives the same bounded frame. Unit tests cover aggregate semantics and the unchanged
  normal-spacing path. With `ORIGIN_BARS=1000000` in an optimized build, install is 204.58 ms,
  1,000 SMA updates average 82.44 µs/update, and retained frames average 0.82 ms. Isolated frames
  contain 3,200 line points, 4,826 candlestick primitives, 1,626 bar primitives, or 1,626
  histogram primitives and build in 3.92 ms / 0.64 ms / 0.27 ms / 0.22 ms respectively. The
  remaining D3 gate is a repeatable interaction benchmark for pan, zoom, and crosshair movement.
- 2026-07-17 — **D3 interaction benchmark increment.** Added the optimized `interaction_perf`
  harness with percentile reporting. At `ORIGIN_BARS=1000000`, a single-series headless frame
  averages 479.5 µs for pan, 514.6 µs for zoom, and 206.3 µs for crosshair movement (p95: 1.60 ms,
  1.72 ms, and 397.7 µs). A forced 10-series × 50,000-visible-bar fixture at 0.08 CSS px/bar
  averages 3.62 ms pan, 3.13 ms zoom, and 3.11 ms crosshair (p95: 4.29 ms, 3.30 ms, 3.33 ms).
  These are Rust headless frame-production measurements; browser Canvas2D/WebGPU executor time,
  presentation, and 60 fps visual parity still require a runtime/browser gate.
- 2026-07-17 — **A5 API-breadth increment.** The public façade now exposes double-click handlers and
  logical/time visible-range change subscriptions. The callbacks are driven by post-render range
  snapshots from the headless WASM adapter; double-click still performs the default fit-content
  action, and callback payloads are cloned before delivery. TypeScript typecheck, package bundle,
  and the browser demo preview are clean. Remaining breadth work is richer scale handles, plugins,
  and other reference compatibility surfaces.
- 2026-07-18 — **D2 live device-loss recovery completed.** Startup fallback alone was insufficient:
  a browser canvas cannot switch from a WebGPU context to Canvas2D after initialization. The public
  package now owns dedicated WebGPU and warm Canvas2D pane canvases plus the axis/input overlay,
  sizes them together, and switches visibility without recreating `ChartEngine` or its retained
  frame. A real wgpu device-lost callback atomically marks the backend unhealthy; terminal surface
  failures also fail over, while `Lost`/`Outdated` retry once and `Timeout` skips one frame. The
  surface policy has unit coverage. Browser loss injection proved `webgpu` → `canvas2d` with the
  complete 1,000-bar frame still visible; explicit Canvas2D startup also passed. Remaining D2 work
  is the wider browser/device CI matrix.
- 2026-07-18 — **D2 Chromium runtime matrix completed.** Added a deterministic adapter-acquisition
  failure injection at the actual `request_adapter` boundary. The package took its ordinary startup
  fallback path, reported `canvas2d`, and rendered the complete 1,000-bar chart. The device-loss
  injection was replayed after removing the demo's manual `render()` call: the WASM device callback
  routed a chart-specific loss event to the package, which scheduled recovery and repainted the warm
  Canvas2D pane automatically. Together with normal WebGPU and explicit Canvas2D startup, all four
  runtime conditions are now proven in Chromium. Cross-browser/device automation remains release-CI
  work rather than an engine-architecture gap.
- 2026-07-18 — **D1/R5 composed screenshot parity increment.** Added public
  `chart.take_screenshot()`, returning a new bitmap-resolution canvas composed from the warm
  Canvas2D execution of the current retained frame plus the shared axis/input overlay. An automated
  in-page gate now creates the ordinary automatic-WebGPU chart and a forced-Canvas2D chart through
  the published package; their public screenshots differ in 0 pixels. Chromium does not permit
  page JavaScript to read the actually presented WebGPU surface (both `drawImage` and
  `createImageBitmap` return transparency), so a separate `runtimeTest=presentedFrame` mode removes
  all non-chart UI for external browser capture. Independent 1280×720 WebGPU and Canvas2D captures
  of that mode were byte-identical. This closes one browser cross-backend parity increment; native/
  reference goldens over the full spacing, DPR, theme, and feature matrix remain.
- 2026-07-18 — **D1/R5 external browser parity runner completed.** The web demo now includes a
  no-cache Node static server and pinned Playwright, PNG, and pixel-diff dependencies. Its Chromium
  project runs full Chrome-for-Testing in new-headless mode, explicitly selects Dawn's SwiftShader
  fallback adapter through a hidden test-only package/WASM option, and fails if the chart selects
  Canvas2D instead of WebGPU. One test proves the synchronous public screenshot contract; the other
  captures the actually presented UI-free WebGPU and Canvas2D frames and attaches both sources plus
  a diff on failure. The deterministic 1280×720, DPR 1.5 suite passes 2/2 locally with exact zero-
  pixel differences. Production adapter policy remains hardware-first and does not expose the test
  option publicly.
- 2026-07-18 — **D1 native/browser shared fixture completed.** Added one versioned JSON contract for
  the deterministic 1,000-bar data generator, 1280×720 CSS viewport, DPR 1.5, 58px price-axis strip
  and 28px time-axis strip. Runtime tests explicitly apply that size instead of relying on headless
  `ResizeObserver` device-pixel reporting; ordinary charts retain auto-size behavior. The native
  example reads the same fixture, configures `ChartEngine` to the identical 1833×1038 pane, and
  emits a PNG through tiny-skia. The Playwright suite invokes that native binary, extracts the raw
  browser Canvas2D pane, and compares all 1,902,654 pixels: 0 differ, maximum/mean channel delta 0.
  Axis text remains outside this comparison because native has no font adapter yet. Browser backend
  plus native parity now passes 3/3; the next D1 gap is the reference library and axis/text matrix.
- 2026-07-18 — **D1 pinned reference begun.** The browser harness now installs Lightweight
  Charts 5.2.0 exactly, renders the same deterministic 1,000-bar fixture through its public API,
  proves two reference captures are byte-stable, and measures Origin against reference after the same Chromium
  compositor. The first honest baseline is 3.41% perceptual difference for the full frame, with
  separate pane (3.25%), price-axis (7.68%), and time-axis (2.16%) results. Versioned ceilings make
  regressions fail without misrepresenting the current result as parity. The next D1 work is to
  reduce those gaps and expand the matrix across DPR, spacing, theme, markers, and overlays.
- 2026-07-18 — **First reference-measured axis correction.** Origin's Canvas2D axis adapter now applies
  the reference's actual-glyph-bounds vertical midpoint correction to price labels and its stable `Apr0`
  sample correction to centered time labels. Price-axis perceptual difference fell from 7.68% to
  6.92% and mean channel error from 7.68 to 5.85; time-axis output held at 2.16%. The full-frame
  result improved from 3.41% to 3.38%, and the versioned ceilings were tightened accordingly.
- 2026-07-18 — **D1 reference spacing/DPR/theme matrix and two engine fixes.** Added public
  `time_scale().apply_options({ bar_spacing, right_offset })` backed by headless `ChartEngine`
  state, then established seven regional reference cases across DPR 1/1.25/2/3, spacing 0.5/6/50, and
  light/dark themes. The matrix exposed that hidden series still contributed to autoscale and that
  Origin shrank its price axis eagerly after a range narrowed. Hidden series are now excluded at the
  authoritative engine autoscale layer, while the browser layout follows the reference's grow-fast/
  shrink-only-on-full-layout rule. In the spacing-50 case, axis width, visible logical range, and
  price extent now match reference; full-frame difference fell from 10.26% to 1.15%, pane difference
  from 10.52% to 0.88%, and price-axis difference from 11.68% to 5.45%. At DPR 1/spacing 6 the pane
  is byte-identical. Every case has a checked-in measured baseline and explicit regression ceiling.
- 2026-07-18 — **D1 reference marker/overlay feature matrix and marker correction.** Added shared marker
  and volume data modules consumed through the public Origin and reference 5.2 APIs, plus a no-feature
  control at DPR 1.5 / spacing 6 so feature cost is measurable independently of existing raster
  differences. The gate exposed five engine defects: fixed-size markers, midpoint-anchored `inBar`
  markers instead of close anchoring, incorrect text offsets, triangle-only arrows, and markers
  surviving a hidden series. The headless frame producer now follows the reference's spacing buckets,
  shape-specific dimensions, close anchoring and label layout, emits arrow heads plus stems, and
  skips invisible series. Marker pane difference is 0.869% versus the 0.827% control—only 0.042
  percentage points of feature-specific divergence with marker autoscale disabled. Overlay volume is 1.627% and is now protected
  by its own versioned ceiling. A diagnostic maximum-volume bar matched its value, x coordinate,
  top/base coordinates and 8-device-pixel width; the excess changed area comes from the reference's layered-
  canvas gap smearing at fractional DPR rather than divergent headless scale geometry.
- 2026-07-18 — **Default marker-autoscale parity completed.** Marker autoscale margins now live in
  `ChartEngine`, use the reference's spacing-dependent `shapeHeight × 1.5 + margin × 2` contract, take the
  maximum per price scale, distinguish above/below/in-bar positions, reset when markers or series
  visibility change, and work on overlay scales and stacked panes. The public
  `set_markers(markers, { auto_scale })` option can disable the default. The reference feature fixture now
  runs with default autoscale enabled and asserts identical visible logical ranges, public price
  extents, and axis widths. Its pane difference is 0.880% versus the 0.827% control. Remaining work
  is fractional-DPR raster refinement, richer scale/API handles, and the Phase C plugin surface.
- 2026-07-18 — **D1 media-coordinate axis-text contract completed.** The headless `AxisFrame` now
  records the semantic midpoint policy and weight of every label: price labels use their actual
  glyph bounds, ordinary time ticks use no midpoint correction and bold only the maximum visible
  weight, marker labels use no correction, and crosshair time uses the reference's stable `Apr0` correction.
  The browser adapter renders these labels with a 12px media-coordinate font under a DPR transform,
  matching the reference's canvas model instead of approximating it with a rounded device-pixel font. At
  DPR 1 / spacing 6 the pane and time axis are byte-identical to reference and both axis regions have zero
  perceptual difference. The pinned DPR 1.5 baseline improved to 3.368% full-frame, 6.818% price-
  axis, and 2.083% time-axis difference. Fractional-DPR compositor/antialiasing refinement remains;
  label selection, placement, emphasis, and midpoint policy are no longer browser-owned behavior.
- 2026-07-18 — **Richer reference scale handles moved behind the headless boundary.** Pure time-scale
  queries and mutations that still lived in the WASM host—time/index/logical conversion, visible
  ranges, scrolling, real-time/reset behavior, and scale dimensions—now live on `ChartEngine`.
  The public time-scale handle adds `scroll_position`, `scroll_to_position`,
  `scroll_to_real_time`, `reset_time_scale`, logical/index conversion, and width/height. New chart-
  and series-scoped price-scale handles expose options, width, manual visible range, and autoscale.
  Price-scale range, inversion, margins, and autoscale state are authoritative engine state; frame
  construction now respects manual ranges instead of overwriting them. A real-browser public-API
  gate verifies the handles and the engine has dedicated host-free tests.
- 2026-07-18 — **All four price-scale modes integrated through the engine.** Normal, logarithmic,
  percentage, and indexed-to-100 modes now transform each series' visible raw range using its
  reference-compatible first-visible base before merging autoscale ranges. Every renderer, marker,
  price line, last-value/crosshair label, and series coordinate conversion uses that stable base.
  Public scale margins are now the authoritative reference defaults (`0.2/0.1` main, `0.8/0` overlay)
  instead of being duplicated as hidden pane padding. Axis label formatting and optimal-width
  negotiation moved from WASM into `ChartEngine`, with the browser supplying glyph metrics only.
  The browser gate matches reference exactly for percentage range, 66px axis width, visible logical
  range, and series coordinate; logarithmic API range/coordinate and indexed range/width/logical-
  window/coordinate also match exactly.
- 2026-07-18 — **Series data/range query surface completed behind the engine boundary.** Public
  `data`, `data_by_index`, `bars_in_logical_range`, `series_type`, and data-change subscriptions
  query the engine-owned merged logical index rather than retaining a duplicate JavaScript data
  model. Sparse-gap and fractional before/after semantics are compared directly with reference in the
  browser gate.
- 2026-07-18 — **True left price scales completed through the shared frame contract.** Each pane
  now owns independent left, right and overlay scale state and series can select the reference-style
  `priceScaleId: "left"`. A visible left strip reserves width before the time scale is laid out;
  the engine records that pane origin, shifts all backend-neutral geometry, scopes the frame
  scissor to the translated pane, and emits right-aligned left-axis labels. WebGPU, Canvas2D,
  screenshots and native consumers therefore receive the same geometry instead of a browser-only
  painted axis. A paired browser fixture matches the reference's left width, raw range, series coordinate,
  round trip and logical window exactly. Remaining API breadth is richer event payloads and the
  lower-priority compatibility tail.
- 2026-07-18 — **Fractional-DPR pane scaling now uses the actual bitmap/media ratio.** Frame
  production derives horizontal and vertical pane ratios from independently rounded bitmap
  dimensions, matching reference/fancy-canvas instead of assuming they always equal the nominal device
  pixel ratio. The existing seven-case matrix remains green and unchanged at its current rounded
  primitive positions, which narrows the remaining measured gap to browser compositor/text/AA
  behavior rather than a pane-scale coordinate error.
- 2026-07-20 — **Engineering-hygiene pass.** (1) Default crosshair mode is now Normal — a
  deliberate divergence from the reference's Magnet default. (2) Production paths no longer carry abortable
  panics: invariant expects/unwraps in the scale cores, plot list, data layer, tick-span
  decomposition, frame conflation, and the wasm render path degrade gracefully instead of killing
  the wasm instance. (3) Lint enforcement: rustfmt normalization, `[workspace.lints]` (unsafe
  forbidden), clippy clean on native + wasm32, eslint on the package. (4) The four oversized
  files were split mechanically — `origin_engine/lib.rs` → indicators/price-scale/series-query/tests
  modules, `frame.rs` → a `frame/` directory (axis, conflation, crosshair, series_geometry),
  `origin_wasm/chart.rs` → `chart/{inner_api,inner_render}`, and the package `index.ts` →
  types/impl/gestures. (5) GitHub Actions CI: fmt + clippy + workspace tests (goldens included),
  package lint/build/typecheck, and the Playwright runtime/parity suite in Chromium. All 190 Rust
  tests and the seven-case browser matrix stayed green throughout.

- 2026-07-20 — **Candlestick per-part colors: wick/border up/down + part visibility.** Candlesticks
  now expose the full reference color surface: `wick_up_color`/`wick_down_color`,
  `border_up_color`/`border_down_color`, and `wick_visible`/`border_visible` join
  `up_color`/`down_color` in `series_options`. `SeriesEntry` carries the six optional overrides and
  the frame resolver applies reference fallback semantics — an unset wick/border color follows its
  direction's body color and both parts default visible — so default rendering is unchanged (the
  DPR-1/spacing-6 pane stays byte-identical to reference 5.2; all seven browser gates green). The
  renderer already had independent body/border/wick channels on `CandleItem`; this change plumbs
  the engine → wasm → TS path (`set_series_wick_colors`, `set_series_border_colors`,
  `set_series_wick_visible`, `set_series_border_visible`) and adds demo pickers that pin an
  explicit color only once touched, preserving the body-color fallback until then.

- 2026-07-20 — **Axis border cosmetics through the options store.** The axis overlay previously
  painted every strip border from a hardcoded `#2B2B43`. `PriceAxisOptions` gains the reference's
  `borderVisible`/`borderColor` and a new `timeScale` group carries the same pair for the time
  axis (behavioral time-scale options stay in `TimeScaleCore` until the Phase B fold-in). The
  host axis pass parses the per-strip colors with a default fallback; pane separators share the
  time-scale border color and stay painted even when the time border is hidden. Defaults are
  unchanged, so the parity matrix holds; browser-verified per-strip red-border probes (right
  only → right+time → hidden → restored) all land on the expected pixel counts. Demo: one
  color picker + visibility checkbox drives all three strips.

- 2026-07-20 — **Package style settings file (`theme.ts`).** Product defaults no longer live in
  hardcoded hex scattered across the stack: `packages/charts/src/theme.ts` holds the light/dark
  token palettes (`background` = chart main bg, `border` = axis border color) and maps them onto
  the options tree. Light: `oklch(1 0 0)` → `#ffffff` / border `#f5f5f5`. Dark: `oklch(0.145 0 0)`
  → `#0a0a0b` / border `#16191f`. `create_chart` applies the selected theme under caller options
  (engine deep-merge), so explicit leaves always win; `theme` is a package-level key stripped
  before forwarding. The engine keeps reference defaults (parity anchor); parity fixtures pin
  `#2B2B43` borders explicitly, and `run_backend_parity` now passes the demo's full chart options
  to its forced-Canvas2D twin instead of relying on defaults accidentally matching. Demo: theme
  select applies a palette live and syncs the axis-border picker. Browser-verified: default light
  borders `#f5f5f5` (1884 px), dark switch repaints bg `#0a0a0b` + borders `#16191f`, switching
  back restores the light state exactly.

- 2026-07-20 — **Theme text token + axis text control.** The platform stylesheet's `--foreground`
  tokens joined `theme.ts` as the `text` token: light `oklch(0.145 0 0)` → `#0a0a0a`, dark
  `oklch(0.985 0 0)` → `#fafafa`, mapped onto `layout.textColor` (the axis tick labels were
  already store-driven, so no engine change was needed). oklch conversions now use the exact CSS
  Color 4 path (OKLab → XYZ → sRGB), correcting the dark background `#0a0a0b` → `#0a0a0a`. Demo:
  an axis-text color picker sits beside the border controls and the theme select syncs both
  pickers. Browser-verified: 342 axis-text pixels track the active token exactly (light `#0a0a0a`
  → dark `#fafafa` → picker `#ff0000` → restored), borders/background unchanged, all seven gates
  green.

- 2026-07-20 — **Grid line styles honored end-to-end.** `build_grid_frame` hardcoded
  `LineStyle::Solid` even though the options store, the `Prim` lines, both executors' identical
  dash-segment expansion, and the native `StrokeDash` path already carried the full reference style
  set. The builder now maps the stored `lineStyle` (0 solid … 4 sparse-dotted) per family, so
  dotted/dashed/large-dashed/sparse-dotted render with the spec's §6 patterns; per-family colors
  were already store-driven. New engine test pins style+color flow; browser probe shows textbook
  duty cycles (solid 27,643 px → dotted/dashed/large ≈50% → sparse ≈20%), total recolor, clean
  hide, exact restore. Demo: grid color picker + style select beside the visibility toggle.
  Defaults unchanged (solid `#D6DCDE`), seven gates green.

- 2026-07-20 — **TypeScript 7 (native) + lint toolchain swap.** The package compiler moved from
  5.9 to `typescript@7.0.2` (native port). Two adaptations were required: TS 7's TS5011 strictness
  needs an explicit `rootDir: "src"` for declaration emit, and `typescript-eslint` is fatally
  incompatible with TS 7's API shim (require-time crash; every 8.x release peer-caps at `<6.1.0`)
  — so the lint gate was ported to **oxlint** (`typescript/no-explicit-any`,
  `typescript/ban-ts-comment`, `typescript/no-unused-vars`, verified firing on probes) with
  `eslint.config.mjs` replaced by `.oxlintrc.json`. Removing typescript-eslint also cleared the
  peer conflict, so plain `npm ci` works with no overrides. Verified: `npm ci`, lint, `tsc
  --noEmit`, declaration build, demo build, and all seven browser gates green on TS 7.

- 2026-07-20 — **Honoredness sweep + Bucket-2 breadth lands; loose ends closed.** A parallel
  session delivered: `layout.fontSize`/`fontFamily` honored by every host text path (was
  hardcoded consts), crosshair per-line `style`/`width` honored (was hardcoded LargeDashed/1px),
  crosshair `labelVisible`/`labelBackgroundColor` honored per line (was hardcoded `#131722`),
  `remove_series` (engine tombstones + derived-indicator cleanup + handle invalidation),
  timeScale breadth (`min_bar_spacing`, `fix_left/right_edge`,
  `lock_visible_time_range_on_resize`, `right_bar_stays_on_scroll`, `seconds_visible`), host
  formatter callbacks (`price_formatter_fn`/`time_formatter_fn`/`tick_mark_formatter_fn`),
  `handle_scroll`/`handle_scale` gesture toggles, business-day times at the boundary, the
  plugin-platform design doc, and cross-browser smoke tests (chromium/firefox/webkit Canvas2D).
  This session verified all of it against the full gate suite (fmt, clippy native+wasm32, 16 test
  suites, oxlint, tsc7, build, 10/10 browser tests) and closed the remaining gaps: `layout.panes.
  separatorColor` is now actually consumed by the axis pass, the candle wick/border **unpin**
  path is complete end-to-end (`undefined` keeps, `""` resets to follow-body), and the demo
  gained controls for crosshair cosmetics (color/width/style/label bg/labels), font family+size,
  separator color, a candle "reset parts" button, and a volume toggle that now exercises the real
  `remove_series`. E2E probes proved every path (font 327→1711 px at size 20, family swap,
  crosshair 858→3456 px at width 4, label bg recolor+hide, separator recolor, wick pin/unpin
  14969→0).

- 2026-07-21 — **Watermark (reference v4 shape).** `watermark` options group (visible/text/color/
  fontSize/fontFamily/fontStyle/horzAlign/vertAlign; default color `rgba(0,0,0,0)` exactly like
  reference) flows through the options store with a camelCase merge test. Rendering is deliberately on
  the shared 2D overlay *above* the series — the one text path that stays pixel-identical across
  the WebGPU and Canvas2D pane backends (behind-series placement needs the GPU glyph-atlas text
  path, tracked under the plugin platform); media-coordinate drawing keeps it crisp at fractional
  DPR, and the CSS color is passed through verbatim so alpha survives. Demo: visibility/text/
  color/size controls (50%-alpha overlay tint). Browser-verified: control wiring (2363 px at
  ~50% alpha), full-alpha paint, alignment bbox center→top-left moves, total recolor, clean
  hide. All ten browser gates green.

- 2026-07-21 — **Performance gate: both production targets PASS.** The roadmap's two perf targets
  are now measured by a dedicated headless harness (`crates/origin_native/examples/perf_gate.rs`,
  deterministic fixtures, warm-up, report-only with `ORIGIN_PERF_STRICT=1` as an opt-in hard gate).
  On the dev machine: **Target A** — `build_frame` at 10 series × 50k bars = **2.37 ms** against
  the 16.67 ms 60fps budget (~7× headroom, viewport conflation doing its job); **Target B** —
  1,000,000-bar `set_series_data` = **245.3 ms** against the 300 ms budget (~18% headroom; the
  number to watch on slower hardware). Wired into CI as a report-only step after the test suite.
  Remaining perf work: browser-side present-time measurement and a hard CI gate once thresholds
  prove stable across runners.

- 2026-07-21 — **Interaction parity fixes, proven against reference source.** The initial axis-drag
  implementation used guessed incremental `exp(dx*K)` models. Ground truth was established two
  ways: the reference 5.2.0 source (local `tmp/refsrc` checkout) and empirical probes of the
  pinned reference build. Findings and fixes: (1) **Keyboard scroll was inverted** — ArrowLeft
  showed newer data; TradingView's Left means back-in-time. The engine's `rightOffset` sign was
  already reference-consistent (larger = newer), so only the key mapping changed: Left = older, Right =
  newer, Ctrl/Shift = 10-bar step, eased over ~160 ms for the platform's smooth keyboard feel
  (mirrors the reference's `scrollToOffsetAnimated`). (2) **Time-axis drag was inverted** — drag right
  zoomed in; reference zooms *out*. Replaced with an exact port of `TimeScale.scaleTo` (start-relative
  ratio of distances-from-right). (3) **Price-axis drag direction was correct but the model was
  wrong** (~5x oversensitive incremental exp); replaced with an exact port of `PriceScale.scaleTo`
  (drag-start snapshot scaled around center, `(startY + (h-1)*0.2)/(currentY + (h-1)*0.2)`, min
  0.1), now also a no-op in percentage/indexed modes like reference. Probes show measured coefficients
  matching the reference formulas to 9–10 significant digits; all ten browser gates, oxlint, and tsc
  stay green. Wheel-over-axis behavior was checked too — reference 5.2 zooms *time* there, which we
  already match.

- 2026-07-21 — **Free 2D dragging: vertical price pan.** The pane drag only scrolled time, so the
  chart felt vertically locked. reference pans both axes in one drag (`pane-widget.ts` →
  `scrollPriceTo` + `scrollTimeTo`), and `PriceScale.scrollTo` shifts the press-time range
  snapshot by `dy * span/(h-1)` (drag down = range up, candles follow the cursor). That formula
  is now ported into the recognizer with a deliberate TradingView-flavored extension: where reference
  no-ops under autoscale, the first drag captures the autoscale-resolved range and turns it
  manual (the TradingView feel the user expects); double-clicking the axis re-enables autoscale
  as before. Touch honors `vert_touch_drag`; inverted scales flip the delta like reference. Probe:
  60 px drag shifts the range by 2.3493337717350755 vs the formula's 2.3493337717350555 (13
  significant digits), the reverse drag restores the exact original range, and a diagonal drag
  pans time and price in one motion. All ten browser gates, oxlint, and tsc green.

- 2026-07-21 — **Kinetic default corrected to reference parity.** The package already defaulted to
  the reference's `kineticScroll: { touch: true, mouse: false }`, but the demo force-enabled mouse coasting
  via a checked-by-default toggle. TradingView's feel is: normal drags stop dead on release, and
  the smooth-scroll sensation comes from the keyboard (Ctrl+←/→, which now eases ~160 ms). The
  demo toggle is now **"mouse kinetic", off by default**, leaving touch coasting on per reference.
  Probe: a mouse flick leaves the scroll position bit-identical 600 ms after release by default,
  coasts when the toggle is on (160.95 → 300.40), and Ctrl+ArrowLeft eases exactly 10 bars
  older.

- 2026-07-21 — **Interaction parity 1:1 vs reference 5.2 (verified against vendored source).** Full
  inventory diff followed by fixes: crosshair clamps into the pane instead of vanishing over
  axes and persists after mouse-up; primary-button-only gestures + middle-click autoscroll
  suppression; wheel deltaMode ×32/×120 + Windows-Chromium DPR correction + horizontal-wheel
  `scrollChart`; pinch ported to the reference's anchored `(scale−prev)×5`; touch path rebuilt on raw touch
  events with direction classification and release-to-page scrolling; 240 ms long-press tracking
  mode with `on_next_tap`/`on_touch_end` exit; double-tap axis reset (30 px/500 ms); kinetic
  ported from `KineticAnimation` (15 px sampling, 50 ms window, 0.2–7 px/ms clamps, 0.997 decay);
  price pan blocked while autoscale is on (strict reference). Playwright 10/10.

- 2026-07-21 — **reference API breadth closed (execution plan Phases 1–2, tracked in
  docs/EXECUTION_PLAN.md).** ~40 items: full per-series styling (lastValueVisible with reference label
  overlap resolution, priceLine family, lineStyle dashes frame-built for backend identity,
  crosshairMarker family, baseline quadrant gradients, histogram base, invertFilledArea, bar
  openVisible/thinBars), per-data-point colors (RGBA channels aligned through dedupe/update/
  insert), per-series priceFormat (price/volume/percent/custom + JS formatter), verbatim CSS
  color round-trips, whitespace data items, shiftVisibleRangeOnNewBar compensation, series.pop/
  lastValueData/priceFormatter, programmatic crosshair, locale+dateFormat, primary-series
  removal + seriesOrder, explicit pane management (add/remove/swap/moveTo/getSeries/
  preserveEmptyPane), full price/time axis cosmetics, vertical gradient background, separator
  hover band. Every phase shipped with a live browser probe; 221 cargo tests + Playwright 10/10
  at each checkpoint.

- 2026-07-21 — **Plugin platform landed (Phase 3).** A-first hybrid per PLUGIN_PLATFORM_DESIGN.md:
  pane + series primitives emit backend-neutral Prim commands (JSON command buffer, one
  marshalling pass per frame), z-ordered under/main/top, lifecycle + series-bound draw contexts,
  axis labels; `autoscale_info` engine contributions; reference `hitTestPane` precedence + per-kind
  series hit tests (`hovered_series`, `hovered_object_id`, hit cursor, `hovered_series_on_top`);
  `add_custom_series` with engine-owned time mapping (reference rounded-candles ported line-for-line);
  markers + watermark re-expressed as plugins with a 0-diff parity proof. Playwright 28/28.

- 2026-07-23 — **Publish readiness validated (Phase 4 prep).** `npm publish --dry-run` runs the
  full prepublish chain (clean → wasm build → bundle → types → pack smoke) and reports a valid
  13-file, 362 kB tarball with public access. Fixed a dry-run leak (`npm_config_dry_run`
  propagated into the pack smoke test's inner `npm pack`). Remaining for v0.1.0: create the
  `@origin` npm org, add `NPM_TOKEN` to repo secrets, push tag `v0.1.0` — the CI publish job does
  the rest; then verify `bun add @origin/charts` (primary) and `npm i` in fresh consumers.

## 11. Revised execution order

The active plan is now different from the original scaffolding sequence. The earlier sequence is
retained in the execution log for history; the next work should be:

1. **D1/R5 — Contract and parity hardening.** Public screenshot, externally presented WebGPU/
   Canvas2D, and raw browser/native pane comparisons are automated and exact for the shared baseline
   fixture. reference baselines now cover representative spacing, DPR, theme, marker, and
   overlay-volume cases, including default marker autoscale with exact public price extents. Add
   deterministic native axis text and reduce the remaining fractional-DPR axis/overlay raster
   gaps. Browser axis semantics and the DPR-1 reference contract are now closed.
2. **D2 — Runtime reach.** WebGPU available, explicit Canvas2D, adapter-request failure, and live
   device-loss failover are verified in Chromium with a complete retained frame. Expand the same
   assertions across the supported browser/device CI matrix.
3. **Performance hardening.** Visible-range data conflation plus repeatable headless 1M and
   10-series × 50k-visible-bars pan/zoom/crosshair evidence is in place. Add the corresponding
   browser executor/presentation gate and verify 60 fps under the same fixtures.
4. **API breadth.** Time-scale scrolling/reset/index/dimension methods, all four price-scale modes,
   series price-coordinate and logical-range/data helpers, and left/right/overlay price-scale
   handles are headless-backed and browser-tested. Fill the remaining high-value reference surface:
   richer mouse/series event payloads and the lower-priority compatibility tail.
5. **Phase C platform surface.** Once the draw-list and backend contract are stable, add Rust
   primitives, the JS plugin recorder, watermark, and custom-series APIs.
6. **Release readiness.** Freeze the public API, add CI for Rust/WASM/package/browser matrices,
   publish reference-style examples and migration docs, and define versioned compatibility guarantees.

### What “no WebGPU coverage” means

It does **not** mean that Canvas2D is missing. The explicit Canvas2D backend, adapter-request
fallback, and WebGPU device-loss failover are proven in Chromium. The remaining WebGPU coverage
gap is running those assertions across the broader supported browser/device matrix. The required
matrix is:

| Runtime condition | Expected behavior |
|---|---|
| WebGPU available | Use the WebGPU executor and render the shared `ChartFrame`. |
| WebGPU unavailable or adapter request fails | Transparently use Canvas2D with the same frame. **Injected and verified in Chromium.** |
| WebGPU device/context is lost | Continue immediately through the warm Canvas2D pane with the retained frame. **Verified in Chromium.** |
| Explicit `backend: "canvas2d"` | Skip WebGPU probing and use Canvas2D deterministically. |

Coverage means automated browser tests for those conditions, plus visual and geometry parity checks,
so the fallback is a real product path rather than a separately maintained demo mode.
