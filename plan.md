# Nucleus Charts All-in-One Architecture Plan

## Decision

Nucleus Charts will become one all-in-one charting library for financial charts and general
application visualization. It will not become a collection of separate products, and the existing
financial engine will not be removed, replaced, or reduced to a compatibility layer.

The financial chart is the primary core of Nucleus. Its deterministic timestamp union, compact
scalar/OHLC storage, price and time scales, panes, interactions, indicators, drawings, trading
objects, footprint data, LOD, and streaming update paths remain authoritative for financial series.
General charting will be added as new engine-owned capabilities alongside that path. All chart
families will use the same chart lifecycle, ordered frame contract, rendering backends, interaction
normalization, accessibility model, styling system, and public library.

This is an additive architecture:

```text
One Nucleus public library and chart API
                    |
             one ChartEngine
                    |
    +---------------+----------------+
    |                                |
existing financial domain       general data domains
time/OHLC/indicators/trading     category/numeric/XY/polar
    |                                |
    +---------- shared layout -------+
                    |
       one ordered ChartFrame contract
                    |
     WebGPU | Canvas2D | GPUI | native
```

Internal modules may have distinct responsibilities, but they must not become divergent products,
rendering models, or user-facing libraries.

## Product target

Nucleus should cover the combined practical territory of Lightweight Charts and Recharts while
retaining the capabilities that distinguish it from both:

- professional real-time financial charts;
- common dashboard and application charts;
- composed charts mixing compatible series;
- deterministic browser, native, and headless output;
- high-density and streaming data;
- one framework-neutral core with first-party TypeScript and React ergonomics;
- Rust-owned chart semantics and geometry rather than a Rust wrapper around a JavaScript renderer.

The target is not API-level emulation of either library. Recharts is React/SVG-oriented and
Lightweight Charts is a specialized imperative Canvas library. Nucleus should provide familiar
capabilities and a straightforward migration path while retaining its own API, renderer model, and
performance guarantees.

## Non-negotiable invariants

### Preserve the financial hot path

- Existing financial series continue to use the canonical timestamp-union `DataLayer`.
- Existing scalar and OHLC columns are not replaced with dynamically typed row objects.
- Current-bar replacement, append, historical merge, whitespace, LOD, autoscale, indicator, and
  query behavior must not acquire general-chart dispatch in their row-level hot loops.
- A chart containing only existing financial features must not allocate general-chart datasets,
  scales, layout state, or geometry caches.
- Existing browser and Rust APIs remain compatible through the pre-1.0 compatibility policy.
- Existing frame ordering, clipping, snapping, scale behavior, and backend parity remain intact.

### Keep one semantic owner and one frame

- Chart semantics remain in `nucleuscharts_engine`; backends only execute prepared frames.
- A chart type may not be implemented separately in WebGPU, Canvas2D, GPUI, or native code.
- Every built-in series owns its validation, scale contribution, geometry, hit testing, tooltip
  values, accessibility values, and invalidation rules in shared Rust code.
- Host callbacks may extend the library, but no built-in chart may depend on a browser callback for
  its canonical geometry.
- Financial and general series ultimately contribute to the same ordered `ChartFrame`.

### Keep the product unified

- `@axiusflowhq/financial` evolves into the all-in-one Nucleus browser SDK rather than leaving a
  financial package behind and creating an unrelated general chart package.
- Rust consumers continue to enter through the coordinated Nucleus crates and `ChartEngine`.
- `chart.add_series(...)`, panes, scale handles, event subscriptions, screenshots, themes, and
  lifecycle operations remain the common concepts.
- React support is an adapter over the same imperative engine. React never owns scales, geometry,
  hit testing, or animation state.
- Different internal data domains must be composable within one chart where their coordinate
  semantics are compatible. Incompatible coordinate systems use distinct panes or plot regions,
  not separate chart engines.

## Target engine architecture

### 1. Domain-aware panes without changing the financial default

Today every pane participates in the chart's shared financial time domain. The target is for a pane
or plot region to bind to an engine-owned horizontal domain:

- `financial_time`: the existing logical-index and timestamp-union behavior;
- `continuous`: numeric X values with a linear, logarithmic, or symmetric-log transform;
- `temporal`: continuous timestamps with elapsed-time spacing;
- `category`: ordered labels with a band or point scale;
- `polar`: angle and radius domains.

The default remains `financial_time`. Existing constructors and restored V1 workspaces therefore
retain their current behavior without new configuration.

Domain binding belongs to the pane or plot region rather than to a renderer. Series may share a
region only when their coordinate requirements are compatible. For example, a financial
candlestick, moving-average line, and volume histogram continue to share financial time. A
category bar and category line may share a category region. A pie chart uses a polar region and
cannot silently join a financial time pane.

The first implementation should add a domain registry and optional general-domain state around the
existing financial fields. It must not rewrite `TimeScaleCore` into a universal scale or add a
trait-object call to every financial coordinate conversion.

### 2. General columnar data beside `DataLayer`

General series need an engine-owned columnar store supporting:

- numeric columns;
- temporal columns;
- interned category columns;
- optional color, size, label, group, low/high, and baseline channels;
- explicit missing values;
- stable row identity for updates, hit testing, transitions, and selections;
- typed bulk ingestion from browser typed arrays;
- bounded caches and capacity reporting.

This store is not a replacement for `DataLayer`. Financial series continue to use their existing
storage. The general store is created lazily when the first general series or dataset is added.

The public API should remain simple for common cases:

```ts
const bars = chart.add_series("column", { x_axis_id: "month", y_axis_id: "revenue" });
bars.set_data([
  { x: "Jan", y: 42 },
  { x: "Feb", y: 57 },
]);

const points = chart.add_series("scatter");
points.set_data([
  { x: 12.5, y: 8.2 },
  { x: 18.0, y: 13.4 },
]);
```

Internally, object input is validated and converted once at the host boundary. Frame construction
must read typed columns rather than JavaScript-shaped rows.

### 3. Shared scale and axis vocabulary

Add general scale implementations without weakening financial price and time scales:

- linear;
- logarithmic;
- symmetric logarithmic;
- continuous temporal;
- band/category;
- point/category;
- radial linear and angular category scales.

Axes become explicit engine-owned objects with stable IDs, orientation, scale binding, tick policy,
formatter, title, grid contribution, visibility, and layout order. Existing financial price-scale
and time-scale handles remain supported and map to their established specialized implementations.

General axes must support:

- top, bottom, left, and right placement;
- multiple X and Y axes;
- vertical and horizontal series orientation;
- independent and shared domains;
- explicit domain bounds and automatic domain calculation;
- tick and label collision handling;
- categorical bands and padding;
- zero lines and reference values.

Axis layout remains a shared engine responsibility. Hosts may measure text through the existing
boundary, but they may not decide tick placement or data geometry.

### 4. Marks and series

Common public series remain concrete, typed series rather than exposing a grammar-of-graphics DSL as
the only API. Internally they may share mark builders and channel resolution.

#### Cartesian foundation

- line;
- area and range area;
- column and horizontal bar;
- grouped and stacked bars;
- stacked area;
- scatter;
- bubble;
- box-and-whisker;
- heatmap;
- error bars;
- composed charts using compatible axes.

Existing financial `line`, `area`, `histogram`, `bar`, and advanced series keep their current data
meaning. New general names or an explicit data-domain option must prevent ambiguous behavior. The
API must never infer whether `{ time, value }` is financial or whether `{ x, y }` is general.

#### Polar foundation

- pie;
- donut;
- radar;
- radial bar;
- polar area.

#### Later specialized layouts

- funnel;
- gauge;
- treemap;
- sunburst;
- Sankey;
- graph/network.

Treemap, sunburst, Sankey, and graph layouts are not ordinary Cartesian series. Each requires a
bounded, deterministic shared layout algorithm and should be added only after the Cartesian and
polar contracts are stable. Geographic and 3D visualization are outside the initial target.

### 5. Extend the ordered frame contract

The existing primitive set is sufficient for most financial geometry but not the complete target.
Add primitives only in response to an implemented built-in series:

- filled and stroked paths;
- arbitrary polygons with an explicit fill rule;
- arcs and sectors;
- reusable symbol instances;
- generalized linear gradients;
- explicit transform or clip scopes if pane-local coordinates cannot express a required layout.

Primitive order, clipping, alpha blending, snapping, and text placement remain fully specified by
the frame. Tessellation must be shared or produce an equally canonical intermediate representation;
individual executors must not independently interpret high-level chart types.

Every new primitive requires Canvas2D, WebGPU, GPUI, and native support plus a cross-backend frame
fixture before a chart type depending on it is considered complete.

### 6. Shared chart components

The following are first-class engine components rather than React or DOM features:

- legends;
- titles and subtitles;
- axis titles;
- tooltips and shared/cross-series tooltip snapshots;
- data labels;
- reference lines, dots, and regions;
- brushes and range selection;
- zoom and pan appropriate to each domain;
- selection and hover state;
- transitions between compatible data states;
- accessibility snapshots and keyboard navigation.

Browser hosts own DOM accessibility nodes, optional HTML tooltip presentation, cursors, clipboard,
and scheduling. Their content and coordinates come from bounded engine snapshots.

### 7. One public API with two authoring styles

The framework-neutral imperative API remains canonical. It should gain conventional camel-case
aliases or a camel-case facade for broad JavaScript adoption while preserving the current snake-case
surface.

A first-party React adapter should reconcile declarative components into incremental mutations of
the same chart instance:

```tsx
<NucleusChart data={data}>
  <XAxis dataKey="month" type="category" />
  <YAxis id="revenue" />
  <Column dataKey="revenue" yAxisId="revenue" />
  <Line dataKey="margin" yAxisId="percent" />
  <Tooltip />
  <Legend />
</NucleusChart>
```

The adapter must diff configuration and data identities. A React render must not automatically
destroy the chart, replace all series data, rebuild all scales, or cross the WASM boundary once per
point.

The initial all-in-one release should remain usable without React in vanilla JavaScript, Vue,
Svelte, Solid, web workers where supported, native Rust, and GPUI.

## Delivery plan

### Phase 0: Freeze evidence and compatibility

Before architectural implementation:

1. Record release measurements for financial startup, package size, 100k and 1m pan/zoom,
   current-bar replacement, append streaming, frame construction, memory, and lifecycle retention.
2. Convert the relevant benchmark observations into enforced budgets.
3. Add a compact compatibility fixture covering every existing financial series, pane, named scale,
   drawing, indicator, and interaction path affected by domain-aware panes.
4. Define the public general-series data shapes and axis vocabulary in an API proposal. The
   concrete proposal is maintained in [`General_charts_api.md`](General_charts_api.md).
5. Define which chart combinations may share a pane and which require separate plot regions. The
   compatibility matrix and rejection rules live in that same proposal.

Exit gate: the project can prove that later work has not regressed the existing financial product.

### Phase 1: Frame and scale foundations

1. Add only the path, polygon, sector, and symbol primitives needed by the first vertical slices.
2. Implement executor parity and native golden fixtures for those primitives.
3. Add general linear and category scales as siblings of the existing scales.
4. Introduce the pane-domain registry with `financial_time` as the unchanged default.
5. Add explicit general X/Y axis state and layout without changing existing financial axis output.

Vertical slices:

- a category column chart, which validates bands, categorical ticks, labels, tooltips, and bars;
- a true XY scatter chart, which validates independent continuous X/Y domains, point hit testing,
  zoom, and dense geometry.

Current implementation progress (2026-09-22):

- the linear/category scale, pane-domain, explicit-axis, shared-layout, and axis-frame foundations are in
  place without adding general dispatch to the financial row path;
- the category-column engine slice now owns bounded typed category/Y storage, stable row identity,
  automatic category/linear domains, positive/negative baseline geometry, missing-row semantics,
  exact/nearest hit testing, tooltip/accessibility snapshots, lifecycle guards, and chart-level
  Canvas2D/WebGPU/GPUI/native parity evidence;
- the true XY scatter engine slice now owns numeric X/Y storage, independent linear/log/symlog domains,
  runtime pan/zoom views, bounded point sizing, missing-row semantics, ordered circle geometry,
  generation-keyed screen-space hit indexing, exact/nearest hits, tooltip/accessibility snapshots, and
  Canvas2D/WebGPU/GPUI/native parity evidence;
- the browser package now exposes domain-aware panes, general axes, category columns, and XY scatter
  through the common chart lifecycle, with object-row conversion, typed bulk ingestion, pane enumeration,
  lifecycle events, data/accessibility snapshots, hit testing, screenshots, and Chromium/Firefox/WebKit
  runtime evidence;
- exact browser hover and primary selection now retain engine-owned row identities, follow reordered
  explicit-ID replacement batches, drop generated batch-local identities, and emit shared frame chrome;
- explicit-ID incremental update/append transactions and per-call bounded retention now run through the
  shared dataset store, with browser object/typed APIs, category pruning, and interaction reconciliation;
- opt-in numeric value labels for column/scatter now use bounded, collision-aware shared-frame text;
- bounded custom row-label channels now follow replacement, explicit-ID updates, retention, shared
  frame placement, and tooltip/accessibility snapshots for both browser general series;
- general-schema persistence now round-trips pane domains, axes, datasets, series, labels, and chart
  options under V2 while keeping V1 financial restoration;
- category columns and XY scatter now participate in the shared browser keyboard/accessibility
  controller using bounded Rust snapshots and a separate engine-owned focus target; explicit row IDs
  retain focus through reordered replacement, generated identities clear, scatter keyboard zoom targets
  its general X axis, and Chromium/Firefox/WebKit runtime coverage exercises the shared controller.

The Phase 1 feature checklist and exit gate are validated. The final gate passed the complete Rust
format/Clippy/workspace-test suite, the release performance gate, package install/lint/build/typecheck/
pack smoke (21 files; 2352 kB WASM), and the complete browser matrix with 278 passed and 13 expected
skips across Chromium/Firefox/WebKit. Financial-only parity, memory-retention, ring-ingest, and browser
performance gates remained green with the general slices enabled.

Exit gate: both slices render equivalently in every backend and a financial-only chart shows no
material regression in output, work, memory, or package size.

### Phase 2: General Cartesian release

**Status: complete (2026-09-22).**

1. Add line, area, grouped/stacked bars, bubble, range, error-bar, heatmap, and box-plot semantics over
   the general store. XY scatter is already supplied by the validated Phase 1 foundation.
2. Add multiple X/Y axes, orientation, domain control, reference components, legend, tooltip, data
   labels, brush, and selection.
3. Extend the Phase 1 typed bulk browser-ingestion and incremental-update path to each new series kind.
4. Extend schema V2 persistence to each new series/configuration shape without changing V1 restoration.
5. Extend the shared Phase 1 accessibility and keyboard controller to each new series kind.
6. Add general-chart benchmark scenarios and enforced budgets.

Phase 2 closure evidence (2026-09-22):

- the first `xy_line` and `xy_area` slices now bind the shared store to continuous numeric, temporal, and
  category band/point X domains with numeric Y axes; missing/transform-invalid rows split shared-frame
  path runs, line/filled-area hits preserve row identity, and bound replacement cannot reinterpret the X
  column kind;
- browser object/typed ingestion includes epoch-millisecond temporal columns, explicit-ID incremental
  updates and retention reuse the Phase 1 transaction path, and invalid temporal values reject atomically;
- both path kinds reuse bounded labels, tooltip/accessibility snapshots, hover/selection/focus, V2
  persistence, and the shared keyboard controller;
- grouped/stacked bars now reuse the Phase 1 category/value store path through bounded `group_id` and
  `stack_id` options. Vertical `column` binds category X/numeric Y, while `horizontal_bar` binds numeric
  X/category Y without adding a renderer-specific primitive. Visible grouped members split each category band, stack members share one group slot,
  positive/negative normal stacks accumulate independently around zero, percent stacks normalize per category,
  stack-aware summed extents feed the oriented numeric axis, and exact/nearest hits retain the contributing row identity.
  The options cross the WASM/TypeScript boundary, survive V2 persistence, and reuse the existing ordered `Rect`
  backend contract. Horizontal bars reuse object/typed category ingestion, explicit-ID updates, bounded labels,
  accessibility/tooltip snapshots, keyboard interaction state, and category-Y axis layout;
- stacked `xy_area` now reuses bounded `stack_id`/`stack_mode` options across numeric, temporal, and
  category X domains. Members align by exact X identity rather than row index, positive/negative normal stacks
  accumulate independently, percent stacks normalize each sign independently to `+1`/`-1`, cumulative
  extents drive Y autoscale, and variable lower/upper boundaries reuse the shared `BandFill` renderer path.
  Filled-region hits preserve the contributing series/row identity, incompatible stack modes are rejected, and
  V2 persistence restores area stack configuration;
- bubble series now reuse the numeric XY store and shared point geometry with a required typed size channel,
  square-root area-to-radius mapping, bounded radii and hit-index work, missing/zero-size semantics, explicit-ID
  updates and retention, tooltip/accessibility snapshots, shared keyboard focus, and V2 persistence;
- `range_area` now owns aligned typed low/high channels over numeric, temporal, and category X domains. Shared
  band geometry splits on missing/transform-invalid bounds, drives exact/nearest hits and bounded labels, exposes
  both values to tooltip/accessibility snapshots, and participates in object/typed replacement, explicit-ID
  updates, retention, memory accounting, keyboard focus, and V2 persistence;
- `error_bar` owns optional independent X/Y lower and upper bounds for numeric and temporal XY, and
  Y-only bounds for category band/point X in the aligned general store. Temporal centers and X bounds
  use whole JavaScript-safe epoch milliseconds and participate in temporal autoscale.
  Shared geometry drives autoscale, ordered stems/caps/center marks, exact hits, labels, snapshots, and
  accessibility. Object and typed updates validate atomically; explicit-ID retention and V2 persistence
  preserve each bound's missingness;
- `box_plot` now implements category-band five-number summaries over the aligned store. Rows enforce
  `min <= q1 <= median <= q3 <= max` atomically, complete outer whiskers drive numeric-Y autoscale, missing
  statistics remain queryable but emit no mark, and shared geometry lowers the IQR box, median, whiskers, and
  caps to existing `Rect`/`HLine`/`VLine` primitives. Object/typed replacement and explicit-ID updates,
  bounded retention, exact hits, labels, accessibility/keyboard focus, and V2 persistence all preserve the five
  channels without a parallel storage path;
- `heatmap_grid` now spans category-X/category-Y, continuous numeric-X/numeric-Y, and temporal-X/numeric-Y
  coordinates over the same aligned store. Category heatmaps retain the bounded second category registry/index
  column for Y; continuous/temporal heatmaps retain one aligned numeric Y-coordinate column while reusing the
  existing numeric/temporal X column and ordinary numeric value/validity channel. Category registries merge and
  compact atomically under explicit-ID updates and `max_rows`; numeric/temporal cells infer deterministic
  boundaries from neighboring coordinate centers. Shared `Rect` geometry drives intensity, exact/nearest hits,
  labels, tooltip/accessibility X/Y labels, keyboard focus, backend parity, and V2 persistence. Missing cell
  values remain queryable but emit no geometry;
- general legends now have a bounded engine-owned metadata snapshot in stable series order, with optional pane
  filtering and explicit hidden-series visibility. Browser hosts can render legend UI without recreating title,
  color, kind, pane, or visibility state; removal updates immediately and V2 restore reconstructs the same
  semantic entries;
- shared cross-series tooltip snapshots are engine-owned and group visible rows in stable series/row order by the
  anchor row's exact horizontal datum. Duplicate X rows are preserved, heatmaps can contribute multiple cells at
  one X coordinate/category, and hidden/other-pane series cannot leak into the snapshot;
- brush/range selection is transient engine state. Hosts provide CSS-pixel endpoints once; Rust converts them to
  semantic numeric, temporal, or category ranges, returns a bounded visible-row snapshot, and reprojects the same
  range after resize/zoom. X and Y brushes cover ordinary series, horizontal bars, and heatmap coordinate axes;
- reference lines, dots, and rectangular regions are first-class bounded engine state bound to explicit axes.
  Each reference independently declares `extend_domain`; automatic domains include it only when requested.
  References lower through the shared frame, block stale pane/axis removal, survive V2 persistence, and have
  Canvas2D/GPUI parity coverage;
- the focused general-chart browser suite is **54/54** across Chromium/Firefox/WebKit, covering legend,
  shared-tooltip, brush, reference components, grouped/stacked vertical and horizontal bars, stacked area,
  bubble, range-area, box-plot, all heatmap coordinate variants, numeric/temporal/category error bars, object/typed
  ingestion, hit testing, updates, accessibility snapshots, keyboard control, and V2 restoration;
- the exact required portable browser suite is green with **315 passed and 10 expected skips** across
  Chromium/Firefox/WebKit. The production pack smoke is green with 21 published files and the optimized
  2,812,727-byte WASM artifact;
- the release perf harness enforces a 100k `xy_line` density gate, a five-series/100k-row mixed-general
  dashboard gate, one combined engine with 50k financial bars plus a 50k-point general range pane,
  and a separate 100k-row numeric error-bar gate.
  General frame construction stays within 16.67 ms, nearest-hit interaction within 8 ms, mixed-general
  retained memory within 12 MiB, and combined retained memory within 16 MiB. These remain separate from
  the existing financial-only targets so regressions cannot hide in an aggregate result. The error-bar
  gate keeps frame/hit work within the same budgets and retained memory within 16 MiB. The strict release run
  passes all targets: mixed-general frame 4.13 ms / hit 5.14 ms / retained 5.51 MiB, combined frame 1.14 ms /
  retained 5.68 MiB, and 100k error bars frame 11.67 ms / hit 7.26 ms / retained 6.87 MiB;
- the browser release benchmark now hard-gates a five-series/100k-row general dashboard. Local closure evidence
  measured 401.09 ms p50 startup against a 2,000 ms ceiling and 87,144,240 first-frame uploaded bytes against a
  96 MiB ceiling. Budget policy v3 also re-baselines the deliberate Phase 2 package growth after minifying the
  shipped ESM: tarball 1,197,880 <= 1,300,000 bytes, unpacked 3,435,987 <= 3,700,000, JavaScript raw
  343,161 <= 620,000, JavaScript Brotli 64,038 <= 95,000, WASM raw 2,812,727 <= 3,000,000, and WASM Brotli
  761,514 <= 810,000;
- full workspace validation is green: workspace Clippy with `-D warnings`, the complete Rust workspace test
  matrix (including 588 engine tests, 25 GPUI parity tests, and 53 WASM tests), benchmark harness tests, package
  lint/typecheck/API checks, formatting, and diff hygiene all pass.

Exit gate: **PASS.** Nucleus builds the representative Recharts-style Cartesian dashboards with engine-owned
semantics, typed updates, persistence, interaction, accessibility, backend parity, and enforced release budgets
while the existing financial gates remain green. Phase 3 is the next open phase.

### Phase 3: All-in-one browser experience

1. Present financial and general series through one documented package and chart lifecycle.
2. Add the camel-case JavaScript facade without removing snake-case APIs.
3. Ship the React adapter with reconciliation, lifecycle, SSR-safe import, and strict cleanup tests.
4. Publish framework-neutral and React examples that combine a financial pane with general summary
   panes in one chart/workspace.
5. Make browser installation possible without repository-specific knowledge or accidental loading
   of development-only assets.
6. Measure whether the full WASM artifact still meets the package-size and startup budgets.

If size evidence shows that one artifact materially harms users, produce optimized build editions
from the same source and public API contract. Such editions are distribution optimization, not
separate products or semantic forks. Do not introduce them speculatively.

Status: **COMPLETE on 2026-09-23.**

- `@axiusflowhq/financial` remains the one framework-neutral package and chart lifecycle for financial,
  general, and combined visualization. The existing snake-case surface remains supported, while the
  common JavaScript lifecycle now also exposes camel-case aliases on the same chart/series/scale handles;
- the optional `@axiusflowhq/financial/react` subpath adds `NucleusChart`, `FinancialSeries`,
  `GeneralPane`, and `useNucleusChart` as a thin authoring layer over the imperative engine. React is an
  optional peer, SSR import performs no DOM work, ordinary rerenders retain engine identities, and
  Strict Mode coverage proves child cleanup plus final chart disposal across Chromium, Firefox, and WebKit;
- `examples/all_in_one/vanilla.mjs` and `examples/all_in_one/react.tsx` demonstrate a financial pane and
  general summary pane in one workspace without a second chart model or framework-specific engine;
- the production package now exports the optimized WASM asset explicitly, pack smoke installs the actual
  tarball into an empty consumer, verifies both naming styles and the React/WASM exports, and rejects
  repository-only runtime paths such as crate, demo, benchmark, or intermediate `pkg/` paths;
- the final artifact remains inside all enforced package budgets, so no speculative split edition is
  warranted: tarball **1,215,014 <= 1,300,000 bytes**, unpacked **3,539,407 <= 3,700,000**, JavaScript
  raw **344,783 <= 620,000**, JavaScript Brotli **64,328 <= 95,000**, WASM raw
  **2,812,727 <= 3,000,000**, and WASM Brotli **761,514 <= 810,000**;
- the 100k general-dashboard startup gate remains comfortably inside budget after the Phase 3 packaging
  work: **336.76 ms p50 <= 2,000 ms**, with **87,144,240 <= 100,663,296** first-frame uploaded bytes;
- the focused general/React browser matrix is **60/60** across Chromium, Firefox, and WebKit, and the exact
  required portable CI browser suite is green with **321 passed and 10 expected skips**. Package build,
  typecheck, lint, API snapshot, namespace policy, release-gate simulation, SSR import, clean-install, and
  packed-consumer checks are part of the closure evidence and the release workflow now gates the new
  React/namespace contracts as well.

Exit gate: **PASS.** A consumer can choose financial, general, or combined visualization through one
library, and React is an authoring option over the same engine rather than a separate implementation.
Phase 4 is the next open phase.

### Phase 4: Polar charts

1. Add angular and radial scales.
2. Add shared sector and polar polygon geometry.
3. Implement pie, donut, radar, radial bar, and polar area.
4. Add polar hit testing, labels, legends, accessibility, animation, and backend parity.

Exit gate: polar charts meet the same deterministic frame, accessibility, and executor standards as
Cartesian and financial charts.

### Phase 5: Demand-driven specialized layouts

Prioritize funnel, gauge, treemap, sunburst, Sankey, and graph layouts using demonstrated customer
demand. Each layout needs its own measurable invariants, complexity bounds, deterministic fixtures,
accessibility representation, and backend-parity evidence before implementation begins.

## Verification requirements

Every phase retains the repository's complete standard gates. In addition:

- financial golden and frame fixtures must remain unchanged unless an intentional visual change is
  separately approved;
- every general chart type needs scale, autoscale, whitespace/missing-value, hit-test, tooltip,
  selection, accessibility, persistence, and lifecycle coverage where applicable;
- every new primitive needs Canvas2D, WebGPU, GPUI, and native parity evidence;
- browser-facing work needs Chromium, Firefox, and WebKit behavior checks;
- React reconciliation needs tests proving stable engine and series identities across rerenders;
- performance suites must separate financial-only, general-only, and combined workloads;
- package size, WASM initialization, first frame, steady-state frame cost, GPU uploads, retained
  memory, and high-density interaction must have explicit budgets;
- no performance claim may be made from an unbudgeted or uncontrolled result.

## Architectural rejection criteria

An implementation must be rejected if it:

- replaces financial timestamp/OHLC storage with a generic row model;
- makes existing financial coordinate or frame loops dynamically dispatch through general chart
  abstractions without measured necessity;
- implements a built-in chart only in a browser or React layer;
- introduces renderer-specific chart semantics;
- forks state or geometry between Canvas2D, WebGPU, GPUI, and native;
- treats custom-series callbacks as the implementation of a promised built-in chart;
- rebuilds the complete chart for routine React property or data changes;
- adds an unbounded dataset, cache, label set, transition queue, or layout iteration;
- adds chart types without corresponding hit testing, accessibility, lifecycle, and parity behavior;
- weakens financial tests or budgets to accommodate general-chart work;
- creates a separately branded or independently behaving general chart product.

## Initial definition of success

The first all-in-one milestone is successful when one Nucleus chart/workspace can, without semantic
or rendering forks:

1. retain all current financial behavior and performance within enforced budgets;
2. render a streaming candlestick pane with indicators and drawings;
3. render category bar/line summary panes and a true XY scatter pane;
4. share themes, events, screenshots, accessibility, persistence, and backend selection;
5. produce equivalent ordered frames across browser, GPUI, WebGPU, and native paths;
6. expose the result through both the framework-neutral API and the React adapter;
7. remain one library whose financial core is still the primary optimized path.

Only after this milestone should breadth become the priority. The durable advantage is not the raw
number of chart names; it is that financial and general visualization share one Rust-owned,
deterministic, high-performance platform without sacrificing the financial engine that established
Nucleus.
