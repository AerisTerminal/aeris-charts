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

Browser hosts enter through `packages/charts`, which translates the supported public TypeScript API into typed arrays and WebAssembly calls. Repository-native Rust hosts use `nucleuscharts_engine` directly and select a renderer, but the Rust crates are internal exact-revision components rather than crates.io/semver products. Rendering backends consume prepared frame data; they do not own chart semantics.

Canonical series data uses opaque chart-local `u32` identities mapped to reusable storage slots. Identities are never reused, removed identities are classified as stale, and slot-backed vectors remain bounded by peak concurrent series rather than lifetime add/remove count. Each ordinary series owns one timestamp column and either one scalar value column or four OHLC columns. `PlotList` owns only a dense range or sparse logical-index mapping plus its chunked autoscale cache; allocation-free views join that mapping to the canonical values for queries and frame construction. Dense aligned mappings carry no per-row index allocation. Indicator outputs own one scalar value column and alias a contiguous source-time range by identity, so they duplicate neither source timestamps nor plot values. The merged timestamp union remains independently owned because ordinary source series are independently mutable and may diverge or carry whitespace. It carries a generation that changes only when its contents change; time weights use that generation, while value-only current-bar updates retain the O(1) fast path.

Each canonical built-in series also owns an eager fanout-16 row-summary pyramid. A summary stores only six `u32` source-row identities: chronological endpoints, OHLC low/high, and close minimum/maximum. Values are always dereferenced from the canonical columns, so the hierarchy adds no second value owner and preserves whitespace. Point and tail mutations repair one node per affected level; typed batches repair the affected range once; historical insertion, replacement, and retention rebuild or repair the exact affected hierarchy before the mutation is visible. The same endpoint summaries bound latest and predecessor lookup to at most fanout work per hierarchy level even across pathological whitespace; no parallel predecessor index or value history is retained. Series removal releases the hierarchy with the canonical storage slot. Custom-series host geometry is not summarized because its semantics are not engine-owned.

Each canonical series also carries a data generation. An ascending typed batch is sanitized once at the host boundary, merged into its source in one data-layer operation, then synchronizes merged time points, tick weights, dependent indicators, and frame generations once. Tail batches append weights incrementally; historical batches merge in `O(n + k)` and reindex once rather than once per input row. A data-layer transaction that actually rebuilds the timestamp union temporarily captures its prior union and emits one old-to-final logical mapping through common timestamps when the engine synchronizes. Current-bar replacements and pure tail appends capture and map nothing, and no second merged timeline survives the synchronization boundary.

One core validator defines canonical numeric time: a finite, integral count of whole UTC seconds in
the inclusive range `-62167219200..253402300799` (years 0000..9999). Hosts do not auto-convert
numeric timestamps. Out-of-range errors include a likely milliseconds, microseconds, or nanoseconds
hint when dividing by that unit would enter the supported range. Direct set/update batches validate
all timestamps before repair or mutation and reject atomically; single updates likewise preserve
state. Shared-ring drains may reject individual rows because producer drains cannot be rolled back.

## Crate boundaries

### `nucleuscharts_core`

Platform-free chart fundamentals: validated canonical columnar data, compact plot index/view storage, ranges, options, formatting, price scales, time scales, tick marks, and shared math. It also exposes structure-level payload and capacity attribution for memory evidence; these counters are not allocator, WASM-page, or browser-memory measurements. Media-space calculations remain `f64`; conversion to backend coordinate formats happens at rendering boundaries.

`ChartOptionsStore` keeps typed options canonical for engine and frame reads and retains the raw JSON
object only for boundary-compatible deep merges and serialization. An option patch is merged and
validated once at mutation time; frame construction borrows the typed value without cloning or
deserializing JSON.

`nucleuscharts_core` must not depend on a window system, browser, GPU, or host application.

### `nucleuscharts_indicators`

Pure technical-indicator calculations over numeric slices. Warm-up gaps are explicit. Alongside clean full-recomputation functions, it owns the explicit per-formula rolling state used for append, current-bar replacement, and rebuild-from-index. Bounded-window formulas retain no source-length state; recursive formulas retain tail state and one checkpoint per 1,024 source rows, then recompute from the nearest prior checkpoint after a historical correction. Derived values use short-lived transfer buffers that move into or update the engine's canonical output series and are capped after partial repairs. This crate does not know about charts, panes, rendering, WebAssembly, or GPUI.

### `nucleuscharts_engine`

The headless owner of chart behavior and mutable chart state. It owns series, panes, scales, workspace layout, drawings, hit testing, interaction models, indicator bindings, price lines, and frame construction.

Each pane owns one unified price-scale collection: reserved `left`, `right`, and overlay (`""`)
scales plus at most sixteen host-created named scales. Named IDs are case-sensitive and pane-local;
each visible side scale retains its own options, range, formatter source, autoscale, inversion,
dense plot-outward order, measured width, and gesture state. Layout reserves the sum of visible
strips on each side while keeping every pane and the shared time scale aligned. Axis ticks,
last-value and crosshair labels, primitives, coordinates, and gestures resolve through the exact
owning scale. Width negotiation measures every axis-side row in a live-value cluster, including the
scaled countdown row; the pane-side title chip is fitted to the available pane instead of inflating
the strip. The horizontal grid uses only the innermost visible populated scale, preferring the right
side when equal orders meet. Hidden and empty named scales retain state without consuming layout or
receiving labels and input.

Axis chrome is engine-owned and compact: axis-attached text resolves to 11/12 of `layout.fontSize`
(11 CSS px at the 12 px default) with the configured family, countdown text to 10/12 of layout
(10 px), scaling proportionally with larger fonts. The price strip is the widest required text plus
1 px border, 3 px tick, and 4 px padding on each side; the time strip is the axis text plus border,
tick, and vertical padding, snapped to an even CSS-pixel height (22 px by default). Price tags are
axis text plus 2 px padding above and below (15 px), while the crosshair Y-axis tag alone adds 2 px
per side (19 px); countdown rows are countdown text plus 2 px padding per side (14 px), and time tags
fit the strip height with 6 px horizontal padding per side. Axis-attached price, time, drawing,
alert, and live-value chips share a 1.5 CSS-pixel corner radius. Tick
density, collision spacing, drag bounds, and crosshair placement derive from the same metrics, and
hosts measure axis strings at the axis size and countdown strings at the countdown size with matching
weight. Font, DPR, formatter, and minimum-dimension changes invalidate measurements, retained labels,
and layout together.

Pane scale geometry is pane-local. Every scale a pane owns is laid out against that pane's own slot
height and carries the pane's top edge as its single explicit transform into chart-content space, so
autoscale, margins, internal height, tick marks, hit testing, and axis gestures resolve inside the
owning pane alone and never against the stacked content height. Resizing one pane therefore cannot
move another pane's range or coordinates. The pane divider is structural rather than plot chrome: its
resting line, hover band, hit test, and drag geometry all describe the same boundary spanning the
full chart width, including every visible left and right price-scale strip.

Series pane/scale rebinding is one validated engine mutation. An unknown destination leaves pane,
scale, data, type, style, visibility, streaming state, and handle identity unchanged. Percentage
and indexed geometry always uses each series' own first visible value, including when several
comparison series share one scale.

Hosts send input and data to the engine. The engine returns query results and a prepared `ChartFrame`. Browser and GPUI adapters normalize native events into CSS-logical `PointerSample`/`WheelSample` values and feed the same fixed-capacity `GestureResolver`. The resolver owns pointer membership, explicit gesture state, live pinch centroid/distance, survivor rebasing, and cancellation; it retains at most two pointers and performs no move-sample allocation. Hosts still own platform capture, cursor application, event-default policy, and frame/timer scheduling. Normalization includes pointer cadence: browsers already coalesce pointer motion to display frames, and a native host must do the same for brush capture — Wayland delivers per-HID-report motion (~1000 Hz, often one axis per event), and feeding every sample to `brush_create_add` records that axis-alternating staircase as stroke knots. The GPUI host therefore retains only the newest brush sample per painted frame and flushes it once during prepaint (and on pointer-up before commit), so stroke knots sample the drag trajectory at display cadence on every host. Zoom, scroll, kinetic motion, snapping, selection, drawing/trading preview semantics, and rollback belong here.

Secondary clicks use the engine-owned Chart Context query. It resolves chart-space coordinates,
pane, time, logical index, hit series, and price on that series' exact scale (or the pane's canonical
default scale on empty space) without running primary-click selection or activation. Browser and
native hosts may use the payload to build menus, clipboard actions, or order UI, but those side
effects remain outside the engine.

Chart-wide value snapshots are assembled once by the engine from canonical plots, current series kind, pane/scale placement, and each series formatter. Exact logical mode retains every live series and leaves gaps or whitespace null; latest mode independently selects each series' own last non-whitespace logical index and time. The snapshot also carries the previous same-series non-whitespace close/value. WASM only serializes this bounded result, while the TypeScript package maps opaque series IDs to existing handles. Crosshair compatibility `series_data` is filtered from the same snapshot, and crosshair leave exposes a rich latest snapshot while retaining an empty compatibility map.

All interaction hit tests use an engine `HitProfile`. Mouse and pen retain precision tolerances; touch expands semantic anchors and actionable trading controls to an effective 44 CSS-pixel target without changing visual geometry. Cancellation from pointer cancellation/capture loss, host focus or visibility loss, resize, backend loss, or disposal closes scale/scroll sessions without inertia and restores drawing/trading previews rather than committing them.

Built-in frame geometry and series hit testing share one viewport-density query. Resolvable spacing uses the raw rows unchanged. Below one physical pixel per row, the query chooses the deepest summary level whose group fits the average pixel density, uses aligned summary nodes for pixel-bucket interiors, and refines partial boundaries through lower levels or raw rows. The existing per-kind conflation then preserves chronological line endpoints and close extrema, candle first-open/high/low/last-close semantics, and histogram greatest absolute value. The resulting ordered `ChartFrame` remains the only backend contract. Crosshair data lookup and autoscale remain exact raw/cached canonical queries rather than LOD approximations.

The official advanced-series examples are engine-owned feature series, not browser drawing callbacks. Each retains its complete validated payload beside an OHLC-shaped canonical projection used by the shared time/price-scale and query machinery. Brushable area, grouped bars, heatmap, HLC area, pretty histogram, background shade, stacked area/bars, and whisker boxes all construct backend-neutral primitives in the same ordered series layer as built-in geometry. Their official defaults, visible-range rules, pixel snapping, autoscale semantics, and source-data lifecycle are therefore identical in browser and native hosts.

Professional footprint / numbers-bar data has a separate tick-truth owner described in
`Footprint.md`. The engine retains canonical microsecond trade events with explicit or deterministically
classified aggressor side and derives integer tick-grid levels, bid/ask/unknown/total volume, POC,
final/session delta, running Max/Min Delta, and diagonal stacked imbalances. Live tip events update
only the active derived bar; a late-event or provider-correction batch merges atomically into the
final canonical tape, validates its final session/bar projection, and reconstructs exactly once.
The configured tick size owns the series min-move/formatter and the shared autoscale, frame, and hit
paths use complete half-tick outer cell bounds on the series' ordinary pane-local price scale.
Footprint bars ultimately emit the same ordered `ChartFrame` as every other series, and no backend
may infer order flow from OHLC or recalculate footprint math.

The upstream heatmap-around-line and background-shade examples are compositions: the specialized engine series is ordered beneath an ordinary line series rather than duplicating that base-series geometry. Heatmap `cell_shader` callbacks are the one styling boundary in this group; the browser evaluates the callback while normalizing input, and Rust retains the resolved color with each bounded cell so every renderer executes the same prepared frame.

Trading is a first-party engine domain, not a drawing, series, primitive, or plugin. Each chart owns host-supplied typed position, order, group, and execution identities; broker relationships and instrument metadata; semantic trading style; dedicated hit state; and a bounded intent queue. The host remains authoritative for broker state. Pointer movement changes only a chart-local snapped preview. Release emits one broker-neutral typed intent directly, and the chart offers no inline confirmation step of its own: a host that gates modifications runs its own confirmation around the intent before answering it, which keeps that policy where the host's instant-order-placement setting already lives. The chart also APPLIES the change as it emits — a closed order or position leaves the chart, a dragged line stays where it was dropped — and keeps only a rollback, so rejecting the intent restores the object exactly as it was. Nothing is parked in a pending tint waiting on an answer, because closing means the object is gone and moving means it has moved. Confirmed objects change only through a subsequent host snapshot or incremental update. Accepted previews remain visibly dotted and pending until that authoritative update arrives; rejected or discarded previews disappear without mutating the confirmed object. Trading state, previews, intents, and executions are runtime-only and never enter drawing persistence.

Price alerts use the same host-authoritative boundary. The engine retains at most 4,096 typed alert-line indicators and paints them through the canonical pane/axis frame; it does not evaluate conditions, persist alerts, enforce account limits, run background timers, or deliver notifications. An alert line's price tag shows nothing but its price, exactly like every other axis tag; what names it is a badge chip attached to the tag's pane-facing edge, carrying a bell drawn from prims rather than a font glyph, rounded on its outer edge and square against the tag so the pair reads as one control. The crosshair price label exposes one engine-rendered multipurpose action chip on its primary price scale: an attached button, rounded on its outer edge and square against the tag with no radius on the tag side, carrying a PlusSignSquare-style icon. The chip stays visible whenever the crosshair is; hovering it lifts the fill a step with no blue fill. Activating that exact hit zone emits a bounded chart-level action request carrying pane, scale, and price; the browser package forwards it to host subscribers so the host can offer alert, limit-order, horizontal-line, or other context-appropriate actions. The request does not choose an action or carry alert defaults. Alert metadata represents the regular-price `crossing`, directional crossing, greater/less operators and the `only_once`/`every_time` frequencies, plus interval-dependent per-bar, bar-close, and per-minute frequencies. These values are display/configuration metadata only until the host returns an authoritative line snapshot or update. Alert lines and pending action requests are runtime-only and never enter chart persistence.

Official primitives with chart semantics are likewise retained by the engine. Series primitives follow the source across panes and own their bounded data, hit state, autoscale contribution, and pane/axis views; pane-only primitives retain a stable `PaneId`. A browser may decode an image or evaluate a user-supplied color/format callback at the platform boundary, but it sends the bounded result back to Rust. The engine stores image watermarks as RGBA8 `RasterImage` values and emits one shared `Image` primitive; Canvas2D, WebGPU, GPUI, and native executors only upload/cache and paint that prepared image.

An indicator binding keeps its public definition, compact private runtime, and ordinary canonical output series separate. Sparse runtime checkpoints are tied to source row positions and to the source and optional volume-series generations. A tail mutation advances only bindings that depend on that source and installs only changed output rows; a historical mutation resumes from the nearest valid checkpoint and replaces the affected output suffix, while truncation or complete replacement performs a clean rebuild. The five-output EMA ribbon is one binding with one independently checkpointed recursive EMA state per configured period. Its atomic period update retains all output identities and presentation, rebuilds the five value columns once, and propagates the resulting changes through dependent indicators. Removed source/output series drop the binding and its runtime state together.

Indicator output metadata is additive and binding-complete: the first output's monotonic series identity is the stable binding identity; every output reports the full structured parameters, source and optional VWAP volume source, stable output name, index, and count. Native hosts may also enumerate one typed definition per live binding in deterministic creation/dependency order and recreate it through the generic `IndicatorKind` entry point, remapping source, volume-source, and ordered output identities as they go. This definition snapshot excludes runtime calculation state and host-owned output presentation. The host groups and renders legends from this metadata; indicator values still come through the ordinary chart value snapshot.

Drawing anchors, kinds, styles, pane association, stable z-order, and metadata remain the only authoritative drawing state (temporary hover/selection/drag/edit promotion never rewrites it). A chart-local derived runtime maps the existing monotonic `DrawingId` values to conservative logical/price bounds, coordinate-keyed media-space anchor geometry, and pane-local z-ordered candidate lists. Candidate queries first reject drawings in semantic space, then test cached conservative screen bounds; only viewport or pointer candidates rebuild coordinate geometry and reach canonical primitive emission or precise hit testing. Full-span horizontal lines, full-height vertical lines, and half-infinite horizontal rays retain explicit unbounded dimensions rather than fake finite extents. The multi-click Path stores two to 100,000 vertices, emits one straight polyline with an open terminal chevron, exposes every vertex for editing, treats the two arrowhead wings as body-movement targets, and commits one history command only when double-click or Enter finishes it; Backspace removes the latest pending vertex and Escape discards the pending path. Brush remains a separate press-drag freehand tool: its bounds are computed once on semantic mutation, padded for curved interpolation, and remain conservatively unbounded during an active brush drag before one exact pointer-up rebuild. Brush capture decimates pointer samples by distance and leaves the frame untouched when a sample is rejected, so a rapid drag invalidates the drawings layer only per captured point. Runtime bounds, geometry, counters, and index entries are never serialized.

When a historical insertion, removal, series replacement, or retention trim changes merged logical indices, the engine rebases every drawing-semantic logical snapshot through the data layer's common-timestamp mapping: committed anchors, pending creation anchors and preview, active brush points, active drag start and current snapshots, and both drawing-history stacks. Common timestamps map exactly, fractional positions interpolate between them, positions outside their extent extrapolate with slope one, and a replacement with no common timestamp is identity. The transient mapping retains only slope-change breakpoints. Pixel drag/brush baselines refresh immediately for input continuity and once more after the next frame settles layout and autoscale. This maintenance mutation creates no drawing-history command.

Each chart also owns a bounded runtime-only drawing history of the last 100 committed semantic
create, delete, anchor, style, and clear operations. Pointer-move samples mutate the active drag
snapshot without adding commands; pointer-up records one start-to-end update. Undo/redo rebuilds
only the affected drawing runtime state, a new mutation clears the redo branch, and persistence
never contains either history stack.

Versioned persistence is an engine-owned semantic DTO boundary, never serialization of live engine
structs. V1 contains ordered pane topology and built-in drawings only. Pane persistence identity is
separate from live `PaneId`: import preserves document references while issuing fresh monotonic live
IDs, so pre-import pane and price-scale handles become stale. The complete document is size-bounded,
parsed, and validated before one transactional install; drawing bounds, candidates, and geometry are
rebuilt once from anchors. Market history, series/indicator definitions, chart options, extensions,
callbacks, and every runtime cache remain host-owned or derived. Unknown versions and semantic kinds
fail structurally without mutation.

Named price-scale descriptors and series bindings remain host-owned configuration and are not added
to chart-state V1. A restoring host recreates pane-local named scales before reinstalling or rebinding
its series.

The browser split grid is the active-chart router. Its stable workspace cell ID decides which
independent chart receives a global drawing tool, document shortcut, or view reset; the receiving
chart retains all drawing selection, hit testing, mutation semantics, and history. An armed toolbar
tool migrates between active cells, but drawing selection never does. The grid's host-facing
workspace state is a small composition of the validated generic split layout, optional active/stable
cell identity, and one unchanged chart persistence V1 document per cell. Optional instrument identities are opaque
host strings. The host stores this composition and restores market history, subscriptions, and
host-owned series/indicator definitions after the grid restores each Nucleus chart document.

### `nucleuscharts_render`

Backend-neutral drawing primitives, colors, geometry, bar-width rules, and the ordered `DrawList`. This is the contract shared by every renderer. Pixel snapping, primitive ordering, clipping intent, and geometry must be decided before backend execution whenever possible. Curved polylines expand their Catmull-Rom spline adaptively by device-px interval length — intervals already a few pixels long render as their chord (dense freehand brush samples), long sparse intervals keep up to 16 segments — and round joins are emitted only where a turn opens a visible wedge, so tessellation volume stays proportional to what the pixels can show on every backend.

### `nucleuscharts_render_gpui`

The native GPUI executor. It converts the prepared primitive stream into GPUI scene operations and owns GPUI-specific text, image caches, geometry conversion, backend metrics, and fixtures. It must not fork chart behavior or recalculate engine geometry. The repository's interactive Linux probes enable GPUI's Wayland and X11 platforms; macOS and Windows continue through GPUI's native platform selection. CI compiles and tests the GPUI backend on all three operating systems.

GPUI's path pass cannot rely on MSAA — its sample count is picked from the surface and can fall back to 1x on Linux — so stroke, disc, and ring meshes carry a per-vertex Loop-Blinn signed-distance encoding in the path shader's `st` coordinates, which the shader resolves into a 1 px coverage fade. That is the edge smoothing the WebGPU backend gets from its 4x MSAA target. The encoding is only exact across a triangle whose `st` spans the 1 px exterior fade band (there `t ≡ 0`, so the shader's `s² - t` field is the exact squared distance and its coverage ramp is linear); wide triangles spanning the whole stroke half-width would interpolate `t` along the quadratic's secant and collapse the fade into a displaced hard edge. Geometry is therefore solid (`st = (0, 1)`) out to each nominal edge and only the exterior 1 px band carries the distance encoding. A mesh larger than a bounded chunk is split into multiple GPUI paths so one stroke cannot overflow GPUI's fixed path instance buffer and trigger its grow-and-redraw retry loop; the mesh is a triangle soup, so coverage and paint order are unchanged.

An interactive GPUI host requests another animation frame only for active engine animation or an explicit finite measurement run. Idle charts stop scheduling frames. The executor retains its lowered `ScenePlan`; a host presentation that does not change the canonical engine frame can repaint that plan without lowering every primitive again.

### `nucleuscharts_render_wgpu`

The WebGPU executor. It owns quad, triangle, textured-label, atlas, blend, multisample, scissor, and GPU timing resources. GPU objects are reused across frames and rebuilt only when their actual invalidation inputs change.

### `nucleuscharts_wasm`

The browser boundary. It exposes the engine through `wasm-bindgen`, decodes typed input, selects WebGPU or Canvas2D policy, executes browser frames, handles shared ring input, text measurement, workspace APIs, and browser telemetry.

The browser boundary translates data and platform events and serializes engine-owned value snapshots. It must not become a second chart engine.

### `nucleuscharts_native`

The headless native executor and verification support. It uses tiny-skia for deterministic raster output, golden comparisons, examples, and release performance gates. Text follows the host system UI sans-serif face. It is evidence infrastructure, not a competing product model.

## TypeScript package

`packages/charts` publishes the `@nucleuscharts/financial` browser API. It owns WebAssembly initialization, TypeScript chart handles, DOM canvas lifecycle, resize observation, Pointer Event translation, platform capture/default policy, host callbacks, themes, shortcuts, offscreen support, and grid helpers. It does not retain a parallel Touch Event recognizer. Option-derived `touch-action` is installed before a gesture begins. Wheel samples retain floating-point deltas; matching Lightweight Charts, `wheel_behavior: "auto"` independently maps vertical deltas to zoom and horizontal deltas to time-scale pan without a modifier. Explicit `"pan"`/`"zoom"` modes remain host overrides.

Browser accessibility is chart-owned, enabled by default, and represented by one singleton controller exposed through `chart.accessibility()`. `enable_accessibility(chart, options)` configures that same controller for compatibility. The chart container is a named group; canvases are hidden from assistive technology and each pane has one complete application-style keyboard surface. Default streaming announcements are off, user-driven navigation/actions remain announced, visible data queries are capped at 512 on-demand logical points, and only the active series owns one engine-rendered focus primitive. Accessibility focus/edit state is runtime-only and is never persisted. Pointer interaction updates ordinary chart selection and hover without moving DOM focus into the application surface; keyboard traversal and explicit accessibility API calls own its visible focus. Forced colors, higher contrast, reduced motion, locale, host names, and visible focus are resolved at the host boundary; the shared engine retains exact focus geometry and keyboard drawing mutations use the same drawing history/rollback path as pointer input.

Auto-size keeps `ResizeObserver`'s exact device-pixel path. A resolution media-query watcher plus orientation/fullscreen fallbacks re-run sizing when DPR changes without a CSS-bounds change; resize reprojects semantic state and does not create new object identities.

The package also ships `design.css` as the portable host design system. Its complete surface, control, interaction, icon, action, focus, radius, and market palette remains host-owned CSS; host chrome uses the system UI font stack and may use `color-mix` and `oklch`. The published package does not include a webfont. Native CPU text uses the host system UI sans-serif face; scene goldens that contain no text stay machine-independent. Chart-facing roles — surface, axis text, axis and grid border, muted text, separator interaction, focus/primary interaction, and market up/down — have deterministic opaque sRGB projections in `packages/charts/src/style_tokens.json`, composited over the theme surface. `nucleuscharts_core` compiles that file into the default options used by every engine and backend. Those colors therefore resolve before frame construction rather than through demo or renderer overrides. The demo consumes the published CSS asset and selects the same named theme as the chart. A `v*` tag matching the package version publishes the verified artifact to GitHub Packages.

The package uses `snake_case` publicly. Data crosses into WebAssembly in typed columns or bounded shared-ring layouts rather than per-point object calls on hot paths. Typed update batches transfer their sanitized owned columns to the engine's batch entry point; the browser wrapper never loops through the single-row engine API. `examples/web_demo` is an integration and parity test host, not part of the library architecture.

`chart.value_snapshot(logical_index?)` crosses WebAssembly once and returns all live series. The package adds live handles to the engine records and derives legacy crosshair `series_data` by retaining only valued entries. Engine-owned feature series expose their scalar scale projection and retain the legacy scalar event shape. Arbitrary custom-series callbacks remain host-owned: exact snapshots are null, while latest snapshots can expose only the last value recorded during a visible frame and are explicitly render-state-dependent. Symbol/exchange metadata, volume association outside VWAP bindings, bar/day change math, session calendars, visibility settings, and legend DOM remain host-owned.

The supported, experimental, internal-but-exposed, and legacy surfaces are classified in
`Public_api.md`. Predictable browser failures use `nucleuscharts_error` with stable category codes;
clean ingestion retains a null diagnostics fast path. The generated WASM surface and benchmark/test
hooks are internal even when visible to developer tools. A deterministic declaration manifest makes
supported TypeScript surface changes explicit in CI.

`chart.remove()` is the single public browser lifecycle operation. It is idempotent and transitions the retained TypeScript handle to a disposed state after cancelling scheduling, detaching browser resources and extensions, releasing per-chart GPU state, explicitly disposing the Rust object, and calling the generated `free()`. Later operations fail with a stable disposed-state error. Offscreen charts use the same explicit dispose-then-free ordering.

## State and frame ownership

Each chart has one engine owner. Mutations invalidate only the state that changed. A frame is a deterministic snapshot of engine state for a viewport and device scale.

Coordinate-authoritative scale objects advance canonical revisions inside their mutating methods. Browser and GPUI hosts express gestures through `ChartEngine` commands; legacy direct Rust access remains coherent because it cannot bypass the scale-owned revision. `SeriesStore` likewise advances its canonical presentation revision whenever a Rust host takes mutable access, replacing read-side hashing of every style field. Retained coordinate-dependent layers are derived caches stamped with the engine's current coordinate revision. Frame assembly asserts that the grid, visible series, chrome, drawings, and interaction overlay all carry that same revision, so a frame cannot mix transforms.

Frame invalidation is an engine-owned generation graph. Layout, coordinates/autoscale, grid and underlay, each series, drawings (with per-drawing prim/point segments plus a trailing brush/pending preview block), and interaction overlays have independent generations. Coordinate-range changes fan out to coordinate-dependent layers; a value-only current-bar update stays on its source series when autoscale bounds do not change. Ordering-only promotion (hover/selection/drag/edit) reassembles retained series layers and drawing segments without rebuilding geometry; drawing drag rebuilds the drawings layer with fresh segments while reusing the runtime per-entry cache. Public option and series-style mutation are included in the generation inputs, so direct native callers cannot bypass retention accidentally.

Series and indicator selection owns one transient engine snapshot of at most twelve canonical output timestamps, sampled from the selected output's full canonical start-to-end extent only on the unselected-to-selected transition. Selection-time projection determines sparse density, while endpoint-inclusive logical spacing prevents a partial-series selection treatment. Overlay rebuilds resolve those identities against current canonical values and coordinates, place candlestick handles at the current body midpoint, clip offscreen handles without replacement, and discard the snapshot on deselection; LOD geometry, screen coordinates, and persistence never own selection-anchor membership.

Drawing semantic mutations reuse this graph: add/remove/style/anchor changes invalidate the drawing layer and update only the affected derived entry, while selection/hover/drag/edit promotion reassembles retained drawing segments without rebuilding geometry and selection changes additionally invalidate the overlay and axis frame for handles. Drawing selection handles are assembled at the beginning of the overlay, preserving their prior canonical order immediately after drawing bodies and before crosshair/series overlays without rebuilding unrelated drawing geometry. Temporary promotion (dragging/editing → hovered → selected → idle, hover gated by `hoveredSeriesOnTop`) never rewrites saved drawing z-order; deselection, hover leave, cancellation, or removal restores it. A selected price-spanning rectangle also emits primary-colored extent tags and a territory band on its bound price scale; those axis views follow creation, drag, and resize coordinates and disappear on deselection unless the drawing explicitly requests persistent axis views. The text tool is an exception: it emits no anchor discs — selection and hover paint the same focus border box (hover at reduced opacity), empty text paints nothing on the chart, and leaving the host editor without typed text removes the drawing. While typing, the host wrap is borderless with transparent glyphs; the engine keeps painting both the label and the focus border underneath, so edit entry cannot lift the text or shift the outline. Crosshair movement and unchanged-coordinate market-data updates do not invalidate drawing geometry. Pane add/remove/swap/move rebuilds pane membership because pane ownership itself changed; ordinary drawing drag updates one entry, and structural removal repairs the canonical vector's id-to-position map.

The engine-owned crosshair overlay can paint the same configurable hover marker for every visible line, area, baseline, and line-shaped indicator output at the snapped logical index. Markers ship disabled and hosts opt in per series or indicator output; when enabled, marker coordinates, per-series colors, borders, pane ownership, and scale conversion are resolved before the shared frame reaches any backend. Crosshair and drawing magnets share one pixel-space candidate path: candle, bar, and footprint series expose their rendered OHLC fields, while line, area, histogram, baseline, and other scalar projections expose only the close/value they paint, so hidden storage columns cannot attract an anchor. Bar-slot highlights and tooltip guides resolve their default tint from the current chart surface, using a light lift on dark surfaces and a dark tint on light surfaces; overlay price-scale text follows the current layout foreground. Explicit host colors remain authoritative, while implicit colors retokenize with chart options. Chrome that stands for a bar itself — the built-in live price line, its last-value axis chip, and the crosshair marker — follows one shared bar-color resolution. For candlesticks that resolution walks the parts in paint order, body then border then wick, skipping any part that is transparent or switched off, so a hollow candle (a transparent body over a visible border frame, TradingView-style) keeps its bullish or bearish color instead of resolving to an invisible fill.

The engine retains semantic pane layers and their ordered primitive/point ranges, then assembles the same canonical `ChartFrame` contract from clean and rebuilt layers. The retained boundaries are underlay/grid, individual series, per-drawing segments plus a trailing creation-preview block, pane chrome, transient trading risk/reward preview regions, financial-action lines/controls (trading and alerts), and overlay. Frame assembly is the single pane-local ordering owner: grid/background → idle indicators → idle drawings → ordinary price series → active objects (dragging/editing → hovered → selected, series before drawings within a tier, previews trailing active) → chrome → trading regions → trading/alerts → overlay → top. Indicator outputs move as one visual group with internal ordering preserved (bindings own grouping, never series type or title); explicit `set_series_order` overrides default idle series grouping while idle drawings stay below price series and indicator-only panes keep stable internal order. Axes, crosshair, and financial-action controls keep their protected layers above all chart content; active chart content stays clipped to its owning pane. No public z-index API or renderer-specific policy exists. Preview regions sit above chart content with financially actionable lines, alert indicators, and exact control hit zones above them and below crosshair transients. A series' live-price cluster stays filled while its value is live, and is outlined — chart-surface fill, semantic color as an inside border and text — once the series' final bar scrolls out of view, so a stale value never reads as the current one. Colliding series clusters are spaced by the overlap pass rather than restyled. Otherwise-solid trading tags that meet the primary cluster's raw axis region take that same outlined treatment, but financial-action tags stay at their exact price coordinate instead of being collision-shifted away from the line they identify. They emit before the primary cluster so the live price remains visually authoritative if exact coordinates overlap. Confirmed orders never manufacture persistent risk/reward fills. Positions and orders use a bounded marker beside the price scale instead of a full-pane line, and only that visible marker span is interactive. Each marker is a readout chip — a solid quantity cell and then P&L or order type inside ONE outline, with no inner border or divider, so the quantity block's own edge is the seam — followed after a gap by the detached close/cancel chip. The two chips round only their outer corners and face each other with square edges, and a destructive control never shares an edge with the readout it would destroy. Outline, quantity fill, and close mark all carry the object's direction color, so the whole marker reads as one color from the line to the price tag; only the P&L text keeps its own profit/loss tint. The close mark is stroked geometry rather than a font glyph, because a host `font_family` is not guaranteed to carry the multiplication sign. A resting limit order is an intention parked at a price, not a directional fill, so it takes the neutral `working_order` accent for as long as it rests; stops and stop-limits are directional triggers and read by side already, and a filled order of any kind reads by side. Buy and long take the up-bar color, sell and short the down-bar color; rejected, cancelled, and expired markers use the rejected color, while pending broker operations use the pending color. A price tag is solid only once its order is actually filled — a resting order stays outlined, so a working intention never reads as an executed one. Every marker chip is square and outlined with a hairline resolved on the chart's own device-pixel convention (`floor(dpr)`, the same rule the axis border uses), so a hollow chip reads as a rule rather than a heavy frame — on the pane labels and on the axis price tag alike. Hover and press tint the addressed control, and the close chip answers hits across the chip's full height rather than the line tolerance. Those tints are pre-blended against the chip surface and stay OPAQUE: a control sits on top of its own marker line, and a translucent fill would let that line read through the button the pointer is on. For the same reason the host suppresses the crosshair lines while the pointer is over a trading control — the control is not a price to read. Action tooltips are host-timed: the engine owns no clock, so it reveals one only once the host arms it after a hover dwell, and a changed hover disarms it. Sweeping across stacked markers therefore never flashes a tooltip per marker. An action tooltip is chart chrome rather than part of the object it describes: it takes the active theme's surface, border, and text tokens, never the order's buy/sell color, so it reads identically on every line and in both themes. Confirmed TP and SL orders remain ordinary host-authoritative order markers with quantity and projected P&L, endpoint nodes, and action tooltips. Trading, alerts, and axis labels share the canonical pane price transforms; WebGPU, Canvas2D, GPUI, screenshots, and native/headless consumers receive no separate financial-action geometry. Retention never gives a backend permission to change ordering or semantics. Host/plugin primitive callbacks use the canonical frame but conservatively rebuild the affected pane stream because their output is not engine-owned. Incremental frames are tested against forced clean rebuilds across data, interaction, scale, drawing, theme, and resize mutations.

Trading interaction is one engine-owned state machine: idle, hovering, dragging an authoritative order through a local preview, or awaiting a host answer while holding that change's rollback. These states are mutually exclusive. Bounded hover and pressed hits are retained separately as visual feedback, so actionable buttons continue to respond while the semantic state is awaiting confirmation without those visuals becoming broker state. Pointer movement changes only the local preview; release applies it, emits one semantic intent, and holds a rollback until the host answers. Position markers do not start drags, and the engine no longer creates protection orders from TP/SL controls; hosts supply confirmed protection orders through the same authoritative snapshot/update path as every other order. Existing protection-order markers remain draggable even when a broker snapshot omits a local position or parent relationship. Modify intents preserve the authoritative order kind and stop-limit trigger price. Escape discards a live drag. Rejecting an emitted intent runs its rollback — reinserting a closed order or position at its original index, or moving a dragged line back — while acceptance simply releases the rollback, since the chart already shows the change and the host's own snapshot remains the last word. A separate active/inactive trading-group visual state owns confirmed bracket connector chrome. Successful protection acknowledgement activates the related bracket, position, or parent-order group; an empty-canvas press deactivates only that visual state, leaving broker relationships and confirmed entry, TP, and SL objects intact.

Panes expose opaque, monotonic chart-local identities at the browser boundary. A live pane or
price-scale handle resolves its current index after moves or swaps; removal permanently invalidates
that handle, so later index reuse cannot retarget it to another pane or scale. Persistence uses a
separate stable pane identity and intentionally issues fresh live IDs during restore.

The ordered frame contract contains pane backgrounds and grids, idle indicator geometry, idle drawings, ordinary series geometry, active series/drawings/previews, custom-series contributions spliced at their paint marks, pane chrome, trading regions, trading/alerts, crosshair overlays, axes, labels, and text, plus per-series and per-drawing segment ranges for retained backend groups. `series_order`/`drawings` stay the stable saved orders; the frame derives the effective paint order without rewriting them, and series/drawing hit tests tie-break on stable order so promotion cannot oscillate hover. Backends preserve ordering, clipping, blending, and coordinate conversion. A backend may batch compatible adjacent primitives only when visible output is unchanged.

## Plugins and host extensions

User-defined custom series and primitives remain explicit host boundaries. The engine owns their identity, layout participation, hit-test context, autoscale contribution, and built-in chrome integration. A host may execute an arbitrary user callback, then records the values the engine needs for the next canonical frame. The official plugin implementations above do not use that callback path.

Extensions must not receive unrestricted engine internals or create a second scene graph. Add extension surfaces only for current consumers with a stable semantic need.

Disposal invokes every registered extension teardown exactly once; one failing JavaScript cleanup hook cannot prevent the remaining hooks from running.

Extension rendering is host-timed, non-reentrant with chart mutation, and error-contained at the
host boundary. Extension runtime objects and callbacks are never persisted by the engine; hosts own
their configuration and restoration. The current custom-series and primitive APIs are experimental,
not a second plugin framework.

The browser package's official-feature modules are thin lifecycle and platform adapters over these
engine owners. They normalize public data/options, translate pointer or keyboard events, decode
browser images, and create optional DOM chrome; they do not simulate financial geometry. Tooltip
guides/value lookup, accessibility focus geometry, drawings, bands, price lines, overlay
labels, image placement, and every specialized series frame are constructed in Rust. Feature
handles release their engine primitive plus any host subscription, timer, or DOM node exactly once;
none of that runtime state enters engine persistence.

## Performance contract

Performance comes from avoiding work:

1. Recompute only invalidated state.
2. Keep hot data columnar and transfers bounded.
3. Reuse GPU, text, image, and geometry resources.
4. Conflate replaceable frame requests while preserving the newest state.
5. Keep rendering and input queues bounded.
6. Measure release builds before changing algorithms or adding caches.

Large-history geometry and hit testing are bounded by physical viewport density plus hierarchy-boundary refinement rather than visible source-row count. The hierarchy is a compact canonical-data auxiliary index, not a renderer cache: GPUI, WebGPU, Canvas2D, native rendering, retained rebuilds, and forced clean rebuilds all consume the same selected geometry. Native evidence records selected level, summary-node operations, raw boundary rows, and candidate rows; browser evidence records the resulting frame CPU, backend work, allocations, and upload bytes without exposing LOD controls through the public chart API.

Drawing work is independently bounded before frame emission. Charts with at most twenty drawings use the direct stable z-ordered render path to avoid index overhead. Larger charts scan only the hovered pane's compact bounds entries, use semantic-domain rejection before coordinate work, preserve canonical pane stable z-order in the candidate list, and run exact per-tool hit tests only for pointer candidates. Retained per-drawing segments reassemble idle-below / active-above without rebuilding geometry on promotion; hit tests stay on stable order so promotion cannot oscillate hover. This intentionally small local structure has no spatial-tree dependency: its cheap bounds pass is linear in drawings owned by the pane; text extents are measured once per semantic/font generation; and coordinate conversion, brush traversal, primitive emission, and precise hit testing follow the candidate count. Pathological complete overlap therefore remains an explicit linear candidate worst case. Cache memory is bounded by live drawing entries, retained path-point capacity, pane membership, and one reusable candidate scratch vector; removal releases the entry and no historical geometry is retained.

Trading state is capped at 4,096 live positions, orders, and executions per chart, and pending intent delivery is capped at 256 entries. Expected terminal workloads (10, 50, 100, and 500 trading objects) use a direct topmost-first pane scan for hits and one retained trading rebuild per semantic/preview mutation; unchanged frames reuse both trading layers. This deliberately avoids a spatial tree until measurements justify one. Engine memory telemetry includes trading vector, intent-queue, and retained string capacity.

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

OHLC ingestion preserves structurally valid numeric input rather than silently rewriting financial values. Impossible relationships are accepted for compatibility but counted in structured diagnostics alongside accepted, dropped, deduplicated, reordered, non-finite, and out-of-range rows. Invalid timestamps reject a direct transaction before value-row repair; accepted batches retain the existing value repair semantics. Clean ingestion returns no diagnostic object on the browser hot path. Predictable boundary failures carry stable error categories rather than relying on console text.

The generic `Workspace` engine type owns only split-tree topology, stable cell identities, ratios,
and bounded validation of a restored layout. Subscription caps, billing-tier vetoes, cumulative
split usage, storage, provider identity, and cell-age metering live in the browser grid host; the
shared engine has no commercial-policy or account knowledge.
Workspace divider mutations reject non-finite deltas without changing the layout, and splits
reject exhausted `u32` cell identities before mutation so browser handles remain addressable.

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

Tag publication requires the Rust, package, and portable Chromium/Firefox/WebKit jobs. Public
declaration and release-policy guards, V1 fixtures, Node import, and pack smoke are portable blocking
checks. Configured `perf_gate` budgets run strictly. Machine-calibrated screenshots, GPU timings,
heap sampling, and wall-clock evidence stay in separate non-blocking diagnostic steps; approved hashes
are never changed merely to satisfy a different host.
