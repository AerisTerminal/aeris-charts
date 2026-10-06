# Aeris Charts Architecture

## Purpose

Aeris Charts is a high-performance financial chart engine. It provides chart state, interaction behavior, drawing tools, indicators, frame construction, and multiple rendering backends for [Aeris Terminal](https://aeristerminal.com) and browser hosts.

The engine is backend-neutral and host-neutral. One canonical state must produce equivalent frames across GPUI, WebGPU, Canvas2D, and native test rendering. Performance, visual parity, deterministic behavior, and bounded resource use are product requirements.

Source code, tests, and measured release behavior are the implementation truth. This file must change in the same commit whenever the architecture changes.

## Data flow

```text
Host API and market data
    -> aeris_charts_core validation, data, scales, and options
    -> aeris_charts_engine chart state and interaction
    -> ChartFrame and aeris_charts_render DrawList
    -> GPUI | WebGPU | Canvas2D | tiny-skia executor
    -> pixels and frame metrics
```

Browser hosts enter through `packages/charts`, which translates the supported public TypeScript API into typed arrays and WebAssembly calls. Rust hosts use the `aeris_charts_engine` crate directly and select a renderer crate. Every Rust crate is repository-only (`publish = false`): hosts such as Aeris Terminal consume them through pinned Git revisions or local paths, and nothing is published to crates.io. The workspace builds as Rust edition 2024 with the toolchain pinned in `rust-toolchain.toml`; its `rust-version` is the minimum Rust a consuming host must use. The GPUI executor tracks a reviewed Zed commit. Rendering backends consume prepared frame data; they do not own chart semantics.

The web demo exposes all built-in calculation APIs in a searchable Indicators catalog. Entries create their engine bindings on demand and remove all owned outputs and synthetic volume dependencies when cleared. RSI uses the same engine calculation and oscillator pane as package consumers; no separate demo formula is maintained.

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

### `aeris_charts_core`

Platform-free chart fundamentals: validated canonical columnar data, compact plot index/view storage, ranges, options, formatting, price scales, time scales, tick marks, and shared math. It also exposes structure-level payload and capacity attribution for memory evidence; these counters are not allocator, WASM-page, or browser-memory measurements. Media-space calculations remain `f64`; conversion to backend coordinate formats happens at rendering boundaries.

General Cartesian scale foundations live beside, rather than inside, the financial scales. `LinearScale`,
`LogScale`, and `SymLogScale` map continuous numeric domains and emit bounded deterministic ticks;
`BandScale` and `PointScale` map caller-owned category indices without retaining labels or allocating
category state. All five keep their math in `f64`, accept reversed ranges, and have no host or renderer
dependency. Linear normalization, interpolation, and tick selection remain finite for every pair of
distinct finite domain endpoints, including spans whose direct subtraction overflows. General axes with
explicit numeric, temporal, band, or point domains use these scales during
shared layout and axis-frame construction; temporal coordinates reuse the linear transform over validated
JavaScript-safe epoch milliseconds while the engine owns UTC calendar interval selection and formatting.
The financial coordinate path does not dispatch through them.
Existing financial charts therefore continue to instantiate only `TimeScaleCore` and `PriceScaleCore`
and pay no retained-memory cost for these foundations.

Each pane has one immutable horizontal-domain binding. Absence of a general binding means
`financial_time` and continues to use the chart's established `TimeScaleCore`; this is the initial
pane and every legacy `add_pane` call. Non-financial continuous, temporal, category, and polar
declarations live in a chart-owned registry that allocates only on first use, is capped at 64 live
entries, uses monotonic internal identities, and releases entries with their panes. Pane moves and
swaps carry the binding. Until compatible general series and axes are installed, financial series
cannot move into a general pane, and V1 persistence rejects rather than silently reinterprets a
general pane. The existing financial frame path never dispatches through the registry.

General axes are chart-owned objects with unique case-sensitive UTF-8 IDs and monotonic internal
handles. Their options retain dimension, resolved placement, scale type, automatic or explicit
domain, direction, visibility, title, bounded tick policy, band padding, zero-line policy, and grid
policy. Validation is atomic: Cartesian X axes must match the pane domain; Cartesian Y axes are
numeric; polar panes accept only angular-category and radial-linear axes; explicit domains must
match the scale and category labels must be unique. Axis count, identity/title bytes, tick count,
category count, and category bytes are bounded and included in engine memory attribution. Pane
moves preserve axis ownership through stable pane IDs. Explicit temporal bounds are ascending epoch
milliseconds within JavaScript's exactly representable integer range. Pane removal releases its
axes. Visible Cartesian axes reserve engine-owned top/bottom plot space and measured left/right strips;
multiple axes stack in insertion order, vertical widths use the existing grow-fast/shrink-on-full-layout
policy, and the resulting rules, titles, and collision-filtered ticks are emitted through the common
`AxisFrame`. Each side may reserve at most 45% of the space remaining after financial axes; complete
strips that do not fit are omitted, preserving a nonzero plot and keeping unscissored axis chrome inside
the chart. Category selection and numeric tick generation are capped at 512 candidates. Explicit
Cartesian numeric, temporal, and category ticks share that cap, reject duplicate or scale-incompatible values,
and retain optional preformatted labels as portable engine state; out-of-view ticks are clipped by
the same transform that places generated ticks and grid rules. Automatic band
and numeric domains now resolve from visible bound general series without rewriting configured axis
options; hidden series stop contributing immediately. Each series' O(rows) numeric scan is memoized in
the general-series registry per axis dimension and scale, keyed by dataset identity and generation, series
kind, and stacking, bounded per series and dropped with the series, so hit tests and frames resolve auto
domains without rescanning unchanged data. A single extreme numeric value expands inward
when outward padding would overflow; logarithmic domains use the adjacent positive value when a
percentage expansion rounds back to the same endpoint. Continuous X/Y axes execute linear, logarithmic,
or symmetric-log transforms consistently for ticks, geometry, hit testing, and runtime pan/zoom; a
runtime view is independent of the configured/automatic base domain and can be reset without rewriting
axis options. Temporal axes use that same runtime-view contract with whole epoch-millisecond anchors and
emit bounded UTC millisecond-through-calendar-year ticks through the shared `AxisFrame`; locale injection
supplies month names without moving date math into a host or backend. Polar tick execution remains deferred
until its owning transform slice is implemented. Cartesian grid and numeric zero-line policies execute
from the same effective domains into the retained pane underlay, below references and series. Axis-local
grid visibility combines with the chart-wide direction style, coincident device-pixel rules are deduplicated,
and an enabled zero line replaces a coincident ordinary grid rule. Financial panes allocate no general
axis storage and retain their established price/time axis output unchanged.

Category runtime views are bounded index windows over the current configured or automatic registry.
Zoom anchors use a category identity in the visible window, pan advances by a rounded visible-window
fraction, and registry changes clamp the window without retaining stale category strings.

General Cartesian data has a separate engine-owned typed-column store beside `DataLayer`. The first
storage slice accepts numeric, epoch-millisecond temporal, and interned-category X columns plus numeric Y
values, explicit validity, and stable generated or caller-provided row identities. Installation and
replacement validate the complete batch before mutation; NaN/infinity, duplicate explicit IDs, invalid
category indices, mismatched columns, and over-limit dictionaries are rejected atomically. Dataset and
row counts, category bytes, and ID bytes are bounded, and retained capacity is attributed separately in
engine memory evidence. A financial-only chart keeps the store absent and therefore retains zero general
dataset capacity.

The released Phase 2 Cartesian bindings are category-band columns, horizontal bars, box plots, category/category
plus numeric/numeric and temporal/numeric heatmaps, numeric XY scatter/bubble marks, numeric/temporal/category
error bars, and `xy_line`, `xy_area`, `range_area`, and category-band `range_bar`. General
series have monotonic chart-local identities, stable pane/axis/dataset ownership, bounded title/color
state, and lazy registry allocation. Populated axes and datasets cannot be removed out from under a
series, and a pane containing a general series cannot be removed until that series is detached. Visible
column series contribute their category union and finite valid Y values to automatic domains; the zero baseline participates in the Y
domain. Horizontal bars reuse the same category/value dataset but bind the numeric value scale to X and the
category band scale to Y; category Y autoscale is engine-owned and the numeric X zero baseline participates in
autoscale. Phase 2 bar layout options add bounded `group_id`/`stack_id` state without creating renderer-specific
primitives. Visible members of one group subdivide each category band, while one stack consumes one group
slot. Normal stacks accumulate positive and negative values independently from zero and contribute their
summed category extents to the oriented numeric-axis autoscale; percent stacks normalize each category independently to `+1` and
`-1`. Horizontal stacks apply the same rules on numeric X; vertical stacks apply them on numeric Y.
Stack membership requires the same pane, group, axes, orientation, and stack mode. Missing rows remain queryable
and accessible but emit no mark or stack contribution. Bar geometry is computed once
in shared CSS-space semantics, reused by frame painting and exact/nearest hit testing, then lowered to
ordinary ordered `Rect` primitives. Bounded tooltip and accessibility snapshots come from the same rows.
Scatter binds independent continuous numeric axes, validates logarithmic positivity, clips geometry to
the runtime view, and lowers persisted circle, square, diamond, or triangle symbols to existing ordered
primitives. Path point markers share the same symbol contract; bubbles remain area-scaled circles. Exact
hits follow each symbol boundary. The lazily rebuilt scatter screen-space grid is keyed by dataset
generation, plot geometry, axis domains/transforms, direction, and point radius; grid
cell count is capped, retained capacity is attributed to engine memory, and exact/nearest hits inspect
only intersecting cells while preserving stable series/row tie-breaking. Bubble reuses that point/index
contract with a required typed size channel, square-root area-to-radius mapping clamped to the shared
point-radius bound, and queryable zero/missing sizes that emit no mark. The size channel participates in
atomic replacement, explicit-ID updates, bounded retention, accessibility, tooltip snapshots, memory
accounting, and V2 persistence. `xy_line` and `xy_area` reuse the same
general dataset/axis ownership across continuous numeric, temporal epoch-millisecond, and category
band/point X domains. Missing rows split path runs by default, while persisted `connect_missing`
can bridge them without removing their queryable identity. Transform-invalid rows always remain hard gaps.
Their persisted `linear`, horizontal-then-vertical `step`, and Catmull-Rom `curved` interpolation policy
travels on the ordered frame primitive and drives both shared lowering and exact/nearest hit geometry.
`xy_line` lowers each run to the shared point pool plus ordered `Polyline` primitives. `xy_area` adds an
ordered `AreaFill` before the matching stroke; its zero baseline is clamped into linear/symlog plots and
falls back to the lower-domain plot edge when a logarithmic Y axis has no zero coordinate. A persisted,
bounded fill opacity preserves the shared 3:1 top-to-baseline gradient and scales stacked/range bands from
the same value. Line hits use
segment distance, while area hits include the filled trapezoid and both preserve the closest endpoint row
identity. When `xy_area` has a `stack_id`, visible members with the same pane, X/Y axes, stack ID, and stack
mode, interpolation, and missing-row connection policy align by exact numeric, epoch-millisecond, or
category X identity rather than row position. Positive and negative values accumulate independently;
percent mode normalizes each sign independently to `+1`/`-1`.
Cumulative extents participate in Y autoscale, and each layer becomes a variable-bound `BandFill` between
the preceding stack boundary and the new cumulative boundary while retaining the upper area stroke and row
interaction identity. `range_area` adds bounded typed low/high columns beside the same X domains, rejects inverted
complete bounds atomically, treats either missing or transform-invalid bound as a run break, and emits one
ordered `BandFill` plus its two boundary polylines from shared geometry. Band hit testing returns the nearest
contributing row, while tooltip/accessibility snapshots expose both bounds. Replacement, explicit-ID updates,
retention, memory accounting, and V2 persistence keep both channels aligned. `BandFill` carries the same
interpolation policy as its boundary strokes; shared coupled expansion chooses one bounded subdivision sequence
for both edges so Canvas2D, WebGPU, GPUI, native painting, and band hit testing cannot open seams or disagree.
Numeric and temporal
`error_bar` support four independent optional bound channels around center XY values; temporal X
centers and bounds are validated whole JavaScript-safe epoch milliseconds and contribute to temporal
autoscale. Category band/point error bars center on a category and keep only the two Y-bound channels.
The engine validates bound ordering atomically, excludes absent-center rows from marks and bound autoscale,
and computes stems, caps, center circles, hits, labels, snapshots, and accessibility from one shared geometry
path. Ordered `HLine`, `VLine`, and `Circle` frame primitives keep executor semantics identical; bound
validity, explicit-ID updates, retention, memory accounting, and V2 persistence remain aligned.
Category-band `box_plot` reuses the aligned general dataset with five ordered numeric statistics:
`min`, `q1`, `median`, `q3`, and `max`. Complete rows validate that order atomically; incomplete rows
remain queryable/accessibility-visible but emit no mark and do not affect autoscale. The outer whiskers drive
numeric-Y autoscale, including logarithmic positivity checks across all present statistics. One shared CSS-space
geometry path computes the IQR rectangle, median, whiskers, caps, labels, and exact/nearest hits, then lowers them
to existing `Rect`, `HLine`, and `VLine` primitives for every backend. Tooltip/accessibility snapshots expose
quartiles separately, explicit-ID updates and bounded retention preserve aligned statistics, and V2 persistence
round-trips the complete five-number summary without reinterpretation.
`heatmap_grid` keeps one aligned dataset across all supported Cartesian coordinate variants. Category/category
heatmaps add a second engine-owned category dictionary/index column for Y; the existing category X column remains
the X registry and the ordinary numeric value/validity column remains cell intensity data. Continuous numeric and
temporal X heatmaps instead add one aligned numeric Y-coordinate column while reusing the ordinary numeric/temporal
X column. Category registries validate, merge, remap, trim, and compact inside the same atomic update transaction.
Automatic domains union category registries or numeric/temporal coordinate extents as appropriate. One shared cell
geometry path maps band grids directly and infers numeric/temporal cell boundaries from neighboring coordinate
centers, derives deterministic normalized value intensity from visible valid cells, drives exact/nearest rectangle
hits and labels, and lowers every cell to the ordered `Rect` primitive. Missing values remain queryable and
accessible but emit no cell. Tooltip/accessibility snapshots expose both X and Y labels, explicit-ID updates and
retention keep coordinate/value channels aligned, and V2 persistence round-trips every heatmap coordinate shape.

General interaction/configuration state stays in the same engine registry. Shared tooltip snapshots group visible
rows by the anchor row's exact horizontal datum in stable series/row order, including duplicate-X heatmap cells.
The transient general brush converts host CSS-pixel endpoints immediately into semantic numeric, temporal, or
category ranges, returns bounded selected row identities, and reprojects from semantic values after view changes.
General reference lines, dots, and regions bind explicit axes and lower into existing shared primitives; each
reference independently declares whether its values extend automatic domains. Reference lifecycle and V2
persistence are engine-owned, while brush/hover/selection remain transient and are not serialized.
The release perf harness
includes a 100k-point general-only line target with a 16.67 ms frame budget and 8 ms nearest-hit budget.
It also keeps the current line, area, range, scatter, and bubble paths in one 100k-row mixed-general
target with the same frame/hit budgets and a 12 MiB retained-memory ceiling, then measures one engine
containing 50k financial bars plus a 50k-point general range pane against the frame budget and a 16 MiB
retained-memory ceiling. A separate 100k-row numeric error-bar target enforces the same frame/hit budgets
and a 16 MiB retained-memory ceiling. Financial-only, general-only, and combined execution therefore have separate
enforced evidence rather than unbudgeted performance claims. The official release browser benchmark also includes
a five-series/100k-row representative Phase 2 general dashboard. Its first-frame host startup has an absolute
2,000 ms p50 ceiling and its first-frame WebGPU vertex upload volume has a 96 MiB p50 ceiling; the release workflow
evaluates these through the same versioned `budgets.json` policy as artifact-size ceilings.
Dataset replacement remains atomic against every bound series and cannot change a bound path/scatter/bubble
X kind or drop a bound bubble size or range low channel. Canvas2D, retained
WebGPU, GPUI, and the native tiny-skia rasterizer consume the same frame contract; grouped/stacked columns
reuse the already-covered ordered `Rect` executor path, bubble reuses scatter's already-covered ordered
`Circle` executor path with per-row radii, stacked area reuses the range-band `BandFill` path, and column,
scatter, line/area, and range-band paths have direct executor
coverage.

The browser package exposes these general slices through the common chart lifecycle. Domain-aware pane and
axis handles remain thin mutations over engine state. General-axis browser handles carry the engine's
monotonic handle token as well as the user-visible axis ID, so removing and recreating an axis with the
same ID stales the old handle instead of retargeting it; V2 restore likewise rejects charts that already
issued a general-axis handle rather than recycling that identity. Object rows are normalized once into numeric or
epoch-millisecond temporal, or interned-category columns; typed input crosses the WASM boundary as bulk arrays, while optional string or
numeric identities cross as one bounded JSON vector. A general-series handle owns one engine dataset and
removes it transactionally after detaching the series. Pane enumeration and chart series-lifecycle events
include general handles without making financial primitive helpers reinterpret them. Tooltip, shared-tooltip,
bounded accessibility, exact/nearest hit, brush, reference-component, and legend state come back from Rust. The legend snapshot is derived
directly from the live series registry in stable engine order, optionally filtered by pane, and retains hidden
series with their visibility state rather than maintaining a parallel host registry. Shared tooltip grouping,
semantic brush selection, and reference domain extension likewise do not create browser-owned semantic mirrors.
Axis and series handles mutate visibility in place through the engine registry, preserving handle, data, view,
selection, and ordering identity while shared invalidation updates domains, hits, legends, persistence, and frames.
Their browser `apply_options` transactions also update mutable axis configuration and series presentation in
place after validating the complete candidate. General-series rebinding commits through that same engine
transaction only when the target pane has equivalent horizontal-domain semantics and its X/Y scale types match;
kind and dataset identity remain structural. Invalid candidates leave the live object unchanged. The general
registry is also the single ordering owner: exact global or pane-local permutations update paint, legend,
hit-test, React keyed-array, and persistence order while leaving other panes' relative order intact. React uses
these mutations for ordinary prop and order changes and releases a new pane or series if initial data installation
or a readiness callback throws.
The browser structured tooltip and accessibility date strings call the chart's shared crosshair
time formatter. That engine path owns the configured IANA time zone, date pattern, locale month
names, and optional host formatter callback; the tooltip's data lookup remains engine-owned.
Financial accessibility summaries still traverse their host series data for min/max and
change, while general series use bounded engine accessibility snapshots.
The shared browser accessibility
controller recognizes financial and general handles but keeps their navigation math separate: financial
series continue to query the time scale, while general series page through at most 512 Rust-owned
accessibility rows at a time. General keyboard focus is a distinct engine interaction target rather than
an alias for hover or primary selection; explicit row identities follow reordered replacement batches,
generated batch-local identities clear, and the shared frame paints the same focus chrome for every
executor. Scatter and continuous-numeric `xy_line`/`xy_area` keyboard zoom mutate their bound general X
axis rather than the financial time scale; category/temporal path navigation remains row-oriented without
inventing an unsupported axis zoom.
Generated row
identities are encoded as decimal strings at the JavaScript boundary so their full `u64` identity is not
rounded. The ordinary browser pointer path feeds exact general hits back into engine-owned transient
hover and primary selection. Interaction targets retain row identity rather than formatted coordinates:
explicit identities follow reordered replacement batches, while generated batch-local identities clear.
Explicit-ID incremental batches update existing rows and append new rows in the shared dataset store;
an optional per-call row limit trims the oldest rows and prunes unused category labels. Validation
precedes mutation, and interaction targets reconcile against the retained identities.
The shared frame emits the corresponding mark chrome, so Canvas2D, WebGPU, GPUI, and native executors
receive the same presentation without host overlays. Opt-in numeric value labels are placed in
the shared frame with deterministic collision rejection and per-pane emission/work ceilings; executors
receive ordinary ordered text primitives. Sparse, bounded custom row labels live in the dataset
store and participate in the same validated replacement/upsert/retention transaction as X/Y data;
tooltip and accessibility snapshots expose their text without replacing raw numeric values. General
chart persistence uses schema V2 for pane domains, axes, datasets, labels, series bindings, and chart
options while financial-only exports remain V1-compatible; restore rehydrates browser general-series
handles without persisting transient hover, selection, or keyboard focus.

`ChartOptionsStore` keeps typed options canonical for engine and frame reads and retains the raw JSON
object only for boundary-compatible deep merges and serialization. An option patch is merged and
validated once at mutation time; frame construction borrows the typed value without cloning or
deserializing JSON.

Style reset is also owned at this shared boundary. `ChartEngine::reset_style_to_defaults()` restores
canonical chart and live-series presentation in place, including semantic unset/follow values, while
preserving data, pane/scale topology, drawings, indicator bindings, price formatting, and all
time/price view state. Price-scale mode, ranges, margins and layout constraints are not style reset.
Advanced-series semantic geometry and footprint aggregation/representation likewise survive while
their colors, strokes, fills and other visual styling return to Aeris defaults. The browser passes
its selected light/dark theme through the WASM boundary because theme selection is package state.

`aeris_charts_core` must not depend on a window system, browser, GPU, or host application.

### `aeris_charts_indicators`

Pure technical-indicator calculations over numeric slices. Warm-up gaps are explicit. Alongside clean full-recomputation functions, it owns the explicit per-formula rolling state used for append, current-bar replacement, and rebuild-from-index. Bounded-window formulas retain no source-length state; recursive formulas retain tail state and one checkpoint per 1,024 source rows, then recompute from the nearest prior checkpoint after a historical correction. Sparse checkpoint vectors are copy-on-write so hosts can transactionally clone recursive state without deep-copying retained history during ordinary tail work. Host-neutral indexed EMA, ATR, session-VWAP, RSI, MACD, and Stochastic states accept callback-provided optional samples so non-chart Rust hosts can lazily convert only the canonical rows replayed for a dirty suffix. `None` is a hard reset; recursive replay may begin at an earlier sparse checkpoint while writers receive only the requested suffix. Stochastic additionally retains only bounded tail `%K` windows needed for `%D` tail replacement; its windowed high/low scan remains bounded by the configured `%K` period rather than source-history length. Derived values use short-lived transfer buffers that move into or update the engine's canonical output series and are capped after partial repairs. This crate does not know about charts, panes, rendering, WebAssembly, or GPUI.

Visible-range volume profiles use a pure two-pass OHLCV bin calculation in this crate: uniform high/low overlap, bullish/bearish volume split by candle direction, deterministic point of control and contiguous value area, `O(visible bars + rows)` work and at most 512 rows. It does not claim tick-at-price accuracy.

### `aeris_charts_engine`

The headless owner of chart behavior and mutable chart state. It owns series, panes, scales, workspace layout, drawings, hit testing, interaction models, indicator bindings, price lines, and frame construction.

Volume-profile indicators bind an OHLC price series to a separate scalar volume series by exact timestamp. The engine owns at most 16 distribution handles, their options and bounded bin caches in native primitive state. Before frame construction (or an explicit snapshot read), it refreshes only profiles whose source generations, visible source-row interval, bin parameters or minimum price move changed. Removing either dependency removes the handle. Shared frame geometry stacks bullish and bearish volume within each row, anchors every row flush to the source pane's right edge, uses stronger row colors for the value area, and draws only the solid POC marker without extending autoscale; every executor consumes those same ordered primitives. These price distributions have no synthetic time-series output and are runtime-only, outside V1 scalar-indicator workspace persistence. Hosts recreate them after restoring data.

The B7 profile query path reads either the canonical classified tape or OHLCV candles through the
engine. A tape profile retains bid, ask, unknown and delta volume by tick; a candle profile marks
its approximation. Periodic profiles and TPO consume ordered, disjoint UTC boundaries supplied by
the host. Adjacent periodic boundaries with the same session identity form one composite profile;
gaps contribute no rows or developing points. Naked tape levels stop at the first later trade on
the same tick; candle levels stop when a later OHLC range contains the exact price.

The browser package exposes these engine queries through WASM and installs bounded periodic
profile presentations on a price series. The engine computes each host-defined period from
canonical candles or tape, retains the source binding in native primitive state, and lowers
bid/ask, delta or total bars with POC and value-area markers into that series' shared frame layer.
Presentations can extend POC and value-area levels from the period end to the first later touch.
The shared query resolves all requested levels in one ordered source sweep. Candle and tape
corrections invalidate the bound series layer, and removing a candle dependency releases its
presentation handles. Presentation handles reserve at most 32,768 profile rows in total, using the
full 2,048-row tape ceiling for each tape period even when the current tape has fewer levels. Optional
tape and candle developing paths sample at most 2,048 POC/value-area points across a presentation
and lower them as shared polylines. The indicators crate accumulates candle OHLCV on the period's
final fixed price grid; this keeps historical levels comparable as the candle range grows, and
remains an OHLCV approximation. Period queries retain detailed developing history within their own
aggregate limit; ordinary frame construction omits that history when the paths are hidden.

Fixed-range profile, anchored profile and anchored VWAP drawing kinds bind a validated source and
lower their geometry into the common ordered frame; the browser drawing-kind map uses the same
wire IDs as the engine tool catalog.
Consecutive TPO boundaries with the same host session identity merge into one profile with
continuous period indices; distinct identities split profiles, and gaps between supplied
segments add no synthetic periods. Initial balance counts periods from the first segment.
The TPO presentation is a bounded native primitive attached to its OHLC source series. Its
letter and block modes, value-area and single-print colors, POC and initial-balance lines are
lowered into the shared ordered frame, so every executor paints the same cells. Query output is
bounded across the whole request as well as per profile; the frame switches to compact blocks
when individual cells exceed the detailed presentation budget.
Price and volume corrections invalidate only drawings bound to that series; tape updates and
replay seeks invalidate drawings bound to that stream even when no footprint series is attached.
A stream remains in use while a profile drawing refers to it, so removal cannot leave a live
drawing with a missing tape.

Each pane owns one unified price-scale collection: reserved `left`, `right`, and overlay (`""`)
scales plus at most sixteen host-created named scales. Named IDs are case-sensitive and pane-local;
each visible side scale retains its own options, range, formatter source, autoscale, inversion,
dense plot-outward order, measured width, and gesture state. Layout reserves the sum of visible
strips on each side while keeping every pane and the shared time scale aligned. Axis ticks,
last-value and crosshair labels, primitives, coordinates, and gestures resolve through the exact
owning scale. Width negotiation measures every axis-side row in a live-value cluster, including the
scaled countdown row; the pane-side title chip is fitted to the available pane instead of inflating
the strip. The countdown row exists only while the host has pinned a clock and the market is trading;
a host reports a closed session with `set_bar_countdown_active(false)`, which hides every countdown row
without changing any series' `countdown_visible` preference. The horizontal grid uses only the innermost visible populated scale, preferring the right
side when equal orders meet. Hidden and empty named scales retain state without consuming layout or
receiving labels and input.

Axis chrome is engine-owned and compact: axis-attached text resolves to 11/12 of `layout.fontSize`
(11 CSS px at the 12 px default) with the configured family, countdown text to 10/12 of layout
(10 px), scaling proportionally with larger fonts. The price strip keeps a stable 1 px border slot,
3 px tick, and 4 px padding on each side while the visible border inside that slot uses the canonical
design-system width; the time strip likewise keeps its existing border slot, text, tick, and vertical padding,
tick, and vertical padding, snapped to an even CSS-pixel height (22 px by default). Price tags are
axis text plus 2 px padding above and below (15 px), while the crosshair Y-axis tag alone adds 2 px
per side (19 px); countdown rows are countdown text plus 2 px padding per side (14 px), and time tags
fit the strip height with 6 px horizontal padding per side. Axis-attached price, time, drawing,
alert, and live-value chips share a 1 CSS-pixel corner radius. Tick
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
full chart width, including every visible left and right price-scale strip. Tick labels reserve their full line-box height plus clearance at internal pane edges even when `entireTextOnly` is disabled; an off-edge label is omitted rather than allowed to paint into its neighbor.

Series pane/scale rebinding is one validated engine mutation. An unknown destination leaves pane,
scale, data, type, style, visibility, streaming state, and handle identity unchanged. Percentage
and indexed geometry uses each series' own first visible value by default; a chart-level comparison
anchor may replace that base for every overlay without copying rows. The same anchor resolution
feeds the bounded engine-owned comparison legend snapshot, so browser and native hosts present
per-symbol values from one canonical time identity.

Hosts send input and data to the engine. The engine returns query results and a prepared `ChartFrame`. Browser and GPUI adapters normalize native events into CSS-logical pointer and wheel samples. Browser, worker, and GPUI pointer and wheel samples feed the engine input controller. The resolver owns pointer membership, the 5 px Manhattan drag threshold, explicit gesture state, fixed starting pinch centroid/distance, cumulative pinch scale, primary-touch continuation/termination, and cancellation; it retains at most two pointers and performs no move-sample allocation. Pinch moves zoom only around the starting centroid and cannot begin after a one-finger move or long press. Hosts still own platform capture, cursor application, event-default policy, and frame/timer scheduling. GPUI reverses its horizontal wheel delta at the adapter so `WheelSample` uses the browser deltaX pan direction and wheel-up positive y on every host. The engine normalizes raw wheel axes once: pixel deltas use the source pixel ratio, DOM line units retain the reference 32 px rule, and page units retain the 120 px convention. GPUI converts its native line units at the adapter so the default three-line notch resolves to the same 100 px browser notch. Browser and offscreen wheel events enter `ChartEngine::input_wheel` through one WASM operation with their raw axes, mode, platform pixel ratio, resolved wheel options, modifiers, and timestamp; the controller owns zoom/pan routing, motion cancellation, hover, and cursor refresh. GPUI feeds the same controller from its adapter. Normalization includes pointer cadence: browsers already coalesce pointer motion to display frames, and a native host must do the same for brush capture — Wayland delivers per-HID-report motion (~1000 Hz, often one axis per event), and feeding every sample to `brush_create_add` records that axis-alternating staircase as stroke knots. The engine input controller therefore retains only the newest captured sample; native hosts call `flush_coalesced_input` once per prepaint (pointer-up flushes before commit), so stroke knots sample the drag trajectory at display cadence on every host. Horizontal kinetic scroll follows the reference domain exactly: drag samples are the time scale's logical `rightOffset`, while the reference 0.2/7 px-per-ms speed limits and 15 px minimum move are divided by the bar spacing captured when the drag starts. The resulting coast is therefore zoom-invariant instead of being tuned in raw pointer pixels. Browser and offscreen keyboard events enter the engine controller through WASM; Left/Right pan is velocity-owned rather than destination-owned: key-down gives an immediate bounded velocity kick, the engine adds further low-friction kicks at a fixed cadence while the key remains held, and host key-repeat is ignored except when it changes the requested Ctrl/Shift speed. Key-up cancels the kinetic state immediately. A worker key event without an explicit down/up phase is a discrete press. With host-requested reduced motion, each arrow event applies one discrete step and mouse-pan release never coasts. Ctrl/Shift retain the existing 10x strength relationship to plain arrows. The controller exposes drawing undo and redo through the engine history; GPUI binds those shortcuts here, and browser and offscreen adapters now route those shortcuts through the same controller. Escape queues a bounded host event so browser crosshair subscribers receive their cursor-left callback after the engine clears hover. Zoom, scroll, kinetic motion, snapping, selection, drawing/trading preview semantics, and rollback belong here.

Browser and worker mouse, pen, and touch samples enter the engine input controller through WASM.
Both adapters use the same touch pinch membership, zoom, cancellation, and primary-finger
continuation. The browser keeps only DOM touch capture and page-scroll direction arbitration;
yielding to page scroll cancels the controller press immediately.
The controller also schedules a stationary pane touch at 240 ms through its wake deadline.
On that wake it enters crosshair tracking, keeps the crosshair anchored through one-finger
motion without panning, and applies the configured next-tap or touch-end exit rule. Browser and
worker hosts schedule this engine deadline and call `input_tick` when it arrives.
For consumed wheel input, the controller uses the wheel event's position to promote hover and
resolve the cursor; the browser applies that cursor after the event even without pointer motion.
The hosts supply CSS-logical coordinates, pointer identity and device, and configured interaction
switches; the controller owns capture identity, the 5 px press threshold (`CLICK_SLOP_MANHATTAN`,
which the browser also reads for page-scroll direction arbitration), click recognition, pan,
axis gestures, hover, and cursor priority. A foreign pointer cannot move, release, or cancel the
captured press. Browser and worker touch membership and continuation follow this controller rule.
The controller recognizes a repeated press by native click count or its bounded 500 ms/5 px
history. It commits double-click on a stationary second release; motion instead retains the
gesture, so selecting a drawing anchor and immediately dragging it works on every host.
The browser and worker pointer adapters pack device, modifier, and configured option bits into
one allocation-free WASM argument. After a controller press completes, the browser dispatches
engine trading intents and alert requests to host subscribers. Window modifier events reach the
controller even when the chart canvas lacks keyboard focus. The engine owns trading tooltip dwell
deadlines; the browser schedules a timer for the exposed deadline and calls `input_tick` when it
expires for mouse, pen, and touch input.

The optional browser brushable Area helper installs one engine-owned composition of an ordinary
Area series and its Delta Tooltip. It sends only explicit style overrides through WASM; the
engine retains the selected range, applies its default and overridden brush styles to the frame,
and clears the range on Escape or double-click through the shared input controller. Mouse and
touch comparison gestures (one touch previews, two touches commit a range) run inside the
controller; each change queues one `DeltaTooltipChanged` event per drain, which the browser turns
into its range-listener notification. The helper queries the active range and detaches the
composition without adding host input listeners.
GPUI's input adapter can take `App::reduce_motion()` at prepaint and forward that preference to
the engine's motion policy before advancing input animations. GPUI has no four-way move cursor, so
its adapter maps `ChartCursor::Move` to the open hand (the pointing hand on Windows, where GPUI
draws hand cursors as the arrow).
Hover arbitration is engine-owned (`ChartEngine::resolve_pointer_hover`, read back through
`input_hover`): browser plugin primitives are JS objects, so WASM gathers their best hit as a
`HostPrimitiveHit` (series, z layer, whether it supplied a cursor) and the engine ranks it against
drawings, series, and general-series items with the reference `hitTestPane` order. The controller
decides whether a primitive cursor wins against active presses, chart tools, and built-in objects;
the browser applies the callback's CSS cursor only when the controller selects it, writes the
cursor only when it changes, and repaints after input only while `frame_pending()` reports a
stale prepared frame, so identical hover samples do no frame work.
Volume-profile indicators are native series primitives rather than output series, so the
controller hit-tests their painted rows itself (after drawings, before series). Hovering a row
shows the pointer cursor and reports `volume_profile:{id}` as the hover object; a click selects the
profile (clearing series, drawing, and general selection) and paints selection anchors on its
first and last rows, point of control, and value-area bounds. Escape deselects it and Delete
removes it, exactly like an indicator series.
Keyboard focus targets exposed by the browser accessibility layer (price axis, time axis, pane
separator, drawing) route through `input_target_key_down` with a `ChartFocusTarget`. The engine
owns their bindings: price-axis Home/ArrowUp/ArrowDown, time-axis Home/ArrowLeft/ArrowRight,
separator Home/ArrowUp/ArrowDown, and the keyboard drawing edit session (Enter opens or commits,
arrows nudge, Tab cycles anchors, Escape restores the start, Delete removes). A committed
keyboard drawing edit is one undo entry; the browser only announces the outcome.

Browser, worker, and GPUI hosts route all pointer, wheel, and keyboard input through one engine input controller
(`chart_input.rs`). A host translates platform events into `PointerInput`, `WheelSample`, and
`ChartKey` values and calls `ChartEngine::input_*`; the controller owns the complete interaction
policy: chart-region resolution, press arbitration (live measure, trading controls, crosshair action
chip, armed drawing tool, drawing drag, delta tooltip, Shift measure, pan), the shared 5 px threshold
through the `GestureResolver`, axis and separator drags, kinetic coasting, click-to-select and
text-edit activation, double-click resets, keyboard bindings, wheel routing, hover promotion, the
trading-tooltip dwell deadline, and a semantic `ChartCursor`. Motion that arrives with the primary
button already up (a release the host never saw), Escape, and `input_cancel` abandon the open gesture
without committing it, exactly like browser pointer-capture loss. Host-configurable switches live in
`InteractionOptions` (the reference `handleScroll`/`handleScale` family plus Aeris's
`price_axis_wheel_zoom`). Work only a host can perform arrives as a bounded queue of
`ChartInputEvent`s (context menu, click and double-click notification, text-editor opening, created
drawing, removal of a host-owned series, crosshair leave, and Delta Tooltip range change). A
double-click ends whatever press it lands in (measure, trading drag, pan, price pan, capture
creation) before applying its reset. Browser and offscreen adapters, and
the GPUI probe, drain host-owned series removals after controller input; the browser also forwards
click, double-click, and crosshair events to subscribers and opens the DOM editor for engine-owned
editing sessions. Hosts keep event
translation, pointer capture, applying the cursor, timer and frame scheduling, menus, clipboard, and
product persistence; they never re-implement routing, cursor priority, or key bindings. A new
interaction is therefore added to the controller once and every native host inherits it. Typed scale
commands resolve the effective series, propagate price format across a scale, toggle series/axis
chrome, and move every attached series between price axes as one operation; hosts do not walk engine
series to reproduce these transactions. Committed drawing edits advance `drawing_revision`, so hosts
persist on that revision instead of tracking gestures.
The temporary Ctrl/Cmd OHLC magnet affects a Normal-mode crosshair only while a drawing tool is
armed, a drawing is being created, or an existing drawing is being dragged. Free browsing retains
the raw cursor price even if a host has not yet cleared the modifier flag; explicitly configured
Magnet and MagnetOhlc crosshair modes remain independent of this drawing interaction.

Native financial-frame preparation is also one engine operation. A host supplies the viewport and
native glyph measurement callbacks; the engine installs CSS dimensions and DPR, decides whether
layout/axis work is required, performs optional initial fit, negotiates axes, owns maximum-label
policy, and builds the chart frame plus axis primitives. It rebuilds whenever any frame layer was
invalidated (one invalidation clock covers every layer, including hover-promotion assembly order) or
input changed state since the last prepared frame, and relayouts after an input-driven pane resize.
The host retains renderer-cache invalidation and paint scheduling, but neither reproduces the
preparation sequence nor clears frames by hand to force a rebuild.

Linked-chart ingress is source-aware. Local mutations publish into the bounded synchronization queue;
`apply_external_sync_event` applies host-supplied crosshair and visible-range state without
echoing it and without draining unrelated local events already awaiting delivery. Hosts coordinate
chart groups and transport events, but they never clear the engine queue to manufacture no-echo
behavior.

Secondary clicks use the engine-owned Chart Context query. It resolves chart-space coordinates,
pane, time, logical index, hit series, and price on that series' exact scale (or the pane's canonical
default scale on empty space) without running primary-click selection or activation. Browser and
native hosts may use the payload to build menus, clipboard actions, or order UI, but those side
effects remain outside the engine.

Chart-wide value snapshots are assembled once by the engine from canonical plots, current series kind, pane/scale placement, and each series formatter. Exact logical mode retains every live series and leaves gaps or whitespace null; latest mode independently selects each series' own last non-whitespace logical index and time. The snapshot also carries the previous same-series non-whitespace close/value. WASM only serializes this bounded result, while the TypeScript package maps opaque series IDs to existing handles. Crosshair compatibility `series_data` is filtered from the same snapshot, and crosshair leave exposes a rich latest snapshot while retaining an empty compatibility map.

Sparse host fundamentals use the existing general-series contract: temporal release rows remain sparse, `GeneralInterpolation::Step` lowers to `LineType::WithSteps` (step-after at the release timestamp), and `GeneralSeriesKind::Column` provides the independent-pane histogram form. Row labels are host-supplied as-of release text; the engine neither fetches nor interprets fundamentals, and no value is projected before its release row. The same timestamp/label rows are retained through history gaps and resampling, so replay can mask future releases without a second fundamental-data model.

All interaction hit tests use an engine `HitProfile`. Mouse and pen retain precision tolerances; touch expands semantic anchors and actionable trading controls to an effective 44 CSS-pixel target without changing visual geometry. Cancellation from pointer cancellation/capture loss, host focus or visibility loss, resize, backend loss, or disposal closes scale/scroll sessions without inertia and restores drawing/trading previews rather than committing them.

Built-in frame geometry and series hit testing share one viewport-density query. Resolvable spacing uses the raw rows unchanged. Below one physical pixel per row, the query chooses the deepest summary level whose group fits the average pixel density, uses aligned summary nodes for pixel-bucket interiors, and refines partial boundaries through lower levels or raw rows. The existing per-kind conflation then preserves chronological line endpoints and close extrema, candle first-open/high/low/last-close semantics, and histogram greatest absolute value. The resulting ordered `ChartFrame` remains the only backend contract. Crosshair and trading data lookup remain exact raw/cached canonical queries rather than LOD approximations. Heikin Ashi candlesticks use a generation-keyed engine presentation cache over canonical OHLC: frame geometry, autoscale, and candle chrome may consume the derived values, while `series_data`, crosshair, and trading paths continue to expose raw OHLC.

The official advanced-series examples are engine-owned feature series, not browser drawing callbacks. Each retains its complete validated payload beside an OHLC-shaped canonical projection used by the shared time/price-scale and query machinery. Grouped bars, heatmap, HLC area, pretty histogram, background shade, stacked area/bars, and whisker boxes construct backend-neutral primitives in the same ordered series layer as built-in geometry. Their official defaults, visible-range rules, pixel snapping, autoscale semantics, and source-data lifecycle are therefore identical in browser and native hosts. Brushable Area is deliberately not an advanced-series data type: it is an ordinary built-in Area series plus transient engine-owned range styling, so data ingestion, retention, LOD, hit testing, price-scale ownership, and all ordinary Area APIs remain on the canonical Area path. The legacy browser input name `brushable_area` is only a compatibility alias and normalizes to `area` immediately. Area-like fills share one design token (`market.area_fill_strong_alpha` → `area_fill_faint_alpha`): an unset Area fill, both unset baseline halves, and the brushable range defaults all derive their gradient from their own stroke color at that strength, strong at the series extreme and faint at its base. Brush default styles are engine-owned (`area_brush_defaults`); hosts send only the fields they override plus each range's positive/negative tone. Native hosts compose the whole interaction with one call, `set_brushable_area(series, Some(options))`: the engine attaches the Delta Tooltip, restyles the area from its active range with those defaults after every gesture, clears the range on pane double-click and Escape, and drops the composition when the series stops being an Area series.

Professional footprint / numbers-bar data has a chart-level tick-truth owner described in
`Footprint.md`. `ChartEngine::add_trade_stream` retains one bounded keyed canonical microsecond tape;
footprints, CVD, delta histograms, big-trades indicators and auction markers hold dependent handles, not
provider-event copies. The stream derives integer tick-grid levels, bid/ask/unknown/total volume, POC,
final/session delta, delta percentage, running Max/Min Delta, and diagonal stacked imbalances. CVD
supports session, continuous, and anchored resets, and every dependent carries the stream revision
through tip, correction, and retention updates. Stream telemetry attributes retained tape capacity
and dependent rebuild work.
Live tip events update only the active derived bar; a late-event or provider-correction batch is
validated against its final session/bar projection, then merges in place into the canonical tape:
only the tail from the earliest touched canonical position is re-sorted and re-indexed, and the
derived bars reconstruct exactly once from the newest replay checkpoint preceding that position.
Dependents re-project from the first rebuilt bar, so a late print near the tip costs at most one
checkpoint interval of replay instead of the retained tape; tape-replaying big trades still replay.
Each derived bar also carries an engine-owned logical index plus its full-resolution open and close
microsecond times. `FootprintAggregator::bar_sequence` exposes those bounds without collapsing them
to whole-second labels, so several non-time bars in one second and long gaps remain distinct.
Chart-integrated trade-count, volume, and range footprint projections use chart-local row keys plus
an engine-owned sequence sidecar; axis labels, crosshair lookup, and visible ranges resolve against
the sidecar's full-resolution open times, never synthetic UTC timestamps. `BarSequenceMapping`
matches ordered full-resolution bounds and rebases logical anchors across prepend/rebuild operations
without collapsing duplicate second labels. Ordinary candlestick and OHLC-bar presentations bind
to that same chart-level stream and consume the aggregator's canonical OHLC bars; stream-identity
replacement and live batches update footprint, ordinary bars, studies, and big trades together without
copying or reclassifying the tape. Bound ordinary bars reject independent retention caps because all
presentations in a non-time domain must retain the same logical rows. Non-time tip updates now
replace only the affected suffix (falling back to a full projection when retention can shift the
prefix), and derived delta studies and big-trades orders use the same logical row keys. Big-trades
order grouping compares the original microsecond trade times. Big trades (`big_trades.rs`) rebuilds
aggressive orders from the classified tape before filtering them (automatic rolling percentile or
fixed minimum), advances incrementally on tip appends and replays on any other tape change, and
draws its bounded bubbles as pane chrome above every series of the host price series' pane; value snapshots and series queries resolve their time labels
through the same sidecar. Trading executions, host events, and round-trip geometry resolve their
timestamp anchors through the same index helper. The sidecar is retired when the last live
non-time footprint, candle/bar, or study dependent leaves the chart,
preventing stale sequence labels from affecting later time series.
Auction markers (`auction_markers.rs`) are a separate runtime-only stream dependent capped at 16
handles. Their per-bar marks use canonical footprint levels, update from the earliest changed bar,
follow stream retention and replay, and paint visible-only triangles, circles, framed labels and
dashed revisitation rays in the same pane chrome beside big trades. They do not add frame primitives
or backend-specific rendering.

Level-two depth uses the parallel chart-side projection boundary documented in `Depth.md`.
`ChartEngine::add_depth_stream` owns one keyed, bounded book per host instrument publication. A
validated full snapshot establishes sequence and tick-grid identity; incremental updates are
atomic, and a gap fences all later deltas behind a typed resync request until the host supplies a
new snapshot. The current bid/ask maps, optional per-level order counts, time-bucketed history,
host-detected microstructure events, replay tape, and checkpoints have explicit independent caps.
DOM ladder rows, cumulative curves, imbalance, and time-and-sales are disposable read models over
the canonical depth or classified trade stream, never additional mutable books or tapes.

Liquidity heatmaps retain immutable 32-column RGBA chunks plus one replaceable one-column live
edge. Fixed absolute bucket alignment lets history eviction rebuild only affected edge chunks while
stable image keys preserve executor caches. The ordered frame emits the same `Prim::Image` contract
to Canvas2D, WebGPU, native, and GPUI, followed by ordinary trade/series geometry and explicitly
bound, capped microstructure markers. Hosts own provider decoding, recovery, event detection,
tooltips/panels, and the Terminal's authoritative market book; this engine state is a bounded chart
projection of those publications. The typed WASM boundary uses parallel numeric arrays, split
high/low words for exact `u64` sequences, an optional aligned label vector, and decimal strings for
exact sequence/trade identifiers returned to JavaScript.

The chart owns one host-supplied replay clock in microseconds and applies it to every canonical
trade stream, depth projection, and the ordinary time-domain data layer. Source rows and future
events remain retained once while series queries, studies, footprint cells, heatmap buckets,
depth markers, sparse stepped releases, trading
executions, round trips, and host-event geometry expose only the eligible prefix. A host window
crossing the clock is clipped; a wholly future window is omitted. The ordered frame draws one
engine-owned dashed replay cursor in every pane, so Canvas2D, WebGPU, native, and GPUI executors do
not reconstruct replay state. Non-time dependents use their shared sequence projection rather than
interpreting logical row keys as UTC seconds; arbitrary independently timed series are not valid on
that domain.

Moving the clock forward applies newly revealed canonical events through the ordinary live path.
Backward trade seeks restore the nearest retained aggregation checkpoint, replay only the reported
suffix, and produce the same bars as a fresh load to that clock. Checkpoints are recorded every
1,024 eligible trades and capped at 64 per stream; an older seek starts from the retained tape's
rebuild seed. Depth uses the same 1,024-event interval and 64-checkpoint cap, restores the nearest
book snapshot, and replays only the reported suffix; its ladder, studies, heatmap, and marker
queries all read the replay projection. Retention evicts whole bars and the exact trades they
counted without re-aggregating the survivors: retained bars are renumbered and surviving checkpoints
are rebased onto the new tape start. Ingest wholly beyond
the clock changes only source truth and performs no dependent work. The existing columnar
`update_typed` path is the bulk ordered bar boundary, while trade batches cross as parallel typed
arrays and update all stream dependents once. The release `perf_gate` advances a shared
footprint/candle chart through 6,000 recorded seconds at 100×, builds every frame, and requires
steady-state retained memory not to grow across complete passes. It also streams a sustained
order-flow tape across the retention ceiling and injects a late print one bar behind the tip,
requiring the per-update p99, the worst retention crossing, and the late print to fit a 60 Hz frame.

Renko, Line Break, Kagi, and Point & Figure are engine-owned price-action transforms over one
canonical host OHLC source. Fixed-box Renko requires a two-box reversal; ATR Renko uses Wilder true
range and begins only after its configured warm-up; Line Break compares a source close with the
high/low of the last configured lines; Kagi reverses only by its configured absolute amount; Point
& Figure uses fixed boxes and a configured reversal count. Ordered tip input updates only the
active transform state, while current-source replacement rebuilds deterministically and is tested
against the incremental result. Source and output are each capped at 1,000,000 rows/bars and reject
overflow atomically. Their output installs through the same full-resolution bar-sequence sidecar as
trade-count/volume/range charts, so replay, labels, crosshair lookup, drawing rebasing, indicators,
and every backend share one logical identity. A chart permits only one independent non-time source;
derived indicators may share it, but another synthetic transform, arbitrary independently timed
series, or a non-time trade stream must use another chart. Renko and Line Break use canonical candle
or OHLC-bar geometry, Kagi lowers to shared horizontal/vertical line primitives, and Point & Figure
lowers bounded X/O text (with a dense-column line fallback), so executors contain no transform math.
Synthetic market source remains host-owned and is intentionally excluded from chart-state
persistence, consistent with every financial series definition and market-history payload.
The configured tick size owns the series min-move/formatter and the shared autoscale, frame, and hit
paths use complete row bounds (whole `ticks_per_row` rows padded half a tick) on the series' ordinary
pane-local price scale. Any series may carry a `render_before_time` cutoff: rows at or after it keep
their data, scale participation, and last-value chrome but are not drawn. An order-flow
presentation never hides its primary: a host showing a footprint installs the primary as
whitespace (times only), so it supplies the bar grid and time axis while the footprint series owns
autoscale, the last-value label, price line, and countdown. Bars the tape does not cover stay empty
rather than falling back to OHLC. Automatic rows (`ticks_per_row == 0`) keep levels at the
instrument tick and merge them per frame in 1-2-5 steps into legible display rows, recomputing
imbalances and POC on the merged rows; text stays at the configured size. `fit_footprint_viewport`
opens the cluster zoom (`FOOTPRINT_BAR_SPACING`) at the real-time edge. Appended host suffixes accumulate on the
presentation's tape, so footprint bars outlive a host's shorter sliding trade window; the tape is
capped at `ORDER_FLOW_MAX_RETAINED_TRADES` by evicting whole oldest bars (with the shared retention
hysteresis) from the footprint, its studies, and the stream together.
Footprint bars ultimately emit the same ordered `ChartFrame` as every other series, and no backend
may infer order flow from OHLC or recalculate footprint math.

The upstream heatmap-around-line and background-shade examples are compositions: the specialized engine series is ordered beneath an ordinary line series rather than duplicating that base-series geometry. Heatmap `cell_shader` callbacks are the one styling boundary in this group; the browser evaluates the callback while normalizing input, and Rust retains the resolved color with each bounded cell so every renderer executes the same prepared frame.

Trading is a first-party engine domain, not a drawing, series, primitive, or plugin. Each chart owns host-supplied typed position, order, group, and execution identities; broker relationships and instrument metadata; semantic trading style; dedicated hit state; and a bounded intent queue. The host remains authoritative for broker state. Pointer movement changes only a chart-local snapped preview. Release emits one broker-neutral typed intent directly, and the chart offers no inline confirmation step of its own: a host that gates modifications runs its own confirmation around the intent before answering it, which keeps that policy where the host's instant-order-placement setting already lives. The chart also APPLIES the change as it emits — a closed order or position leaves the chart, a dragged line stays where it was dropped — and keeps only a rollback, so rejecting the intent restores the object exactly as it was. Nothing is parked in a pending tint waiting on an answer, because closing means the object is gone and moving means it has moved. Confirmed objects change only through a subsequent host snapshot or incremental update. Accepted previews remain visibly dotted and pending until that authoritative update arrives; rejected or discarded previews disappear without mutating the confirmed object. Trading state, previews, intents, and executions are runtime-only and never enter drawing persistence.

The trading contract is explicitly multi-account and host-authoritative: every runtime object may carry a bounded validated account ID, and one engine-owned visible-account filter gates both rendering and hit-testing without removing hidden objects from the snapshot. Host annotations on positions and orders are capped, validated atomically, and rendered as shared chip geometry with deterministic overflow; their tone and tooltip are presentation metadata only. Trailing and break-even trigger lines use host-supplied prices, while price-bearing intents also carry an exact integer tick index when instrument tick metadata permits it. Execution markers resolve each fill to the bar that contains its time (the last bar opening at or before it, shared with host events and the comparison anchor), and every visible fill of one side on one bar shares one mark placed outside what the scale's primary series (`primary_series_on_price_scale`) paints there, so overlays on the same scale such as host studies or compare lines never displace it: buys below the bar's rendered low, sells above its rendered high, using the Heikin-Ashi wick when shown, the column top for histograms, and for line, area, and baseline series the stroked line across the mark's full width (its slope toward each neighbor, or a stepped line's riser, padded by half the line width), so a mark never floats off the series or touches its line. One engine layout (`trading_execution_layout`, bounded to visible bars) feeds both the frame and hit testing; hovering or pressing a mark draws a tick at every fill's exact price on the bar, a dotted lead, and a fill tooltip on the mark's outer side. The default mark is an open `Polyline` stroke in its own themable `execution_buy`/`execution_sell` colors, kept apart from the green/red order chrome: one fill draws a shaft with one chevron, and each further fill on one side of one bar stacks one identical tailless chevron nearer the bar (at most five), so the mark counts the fills, only the outermost (newest) chevron keeps the shaft, and the hit box and outward placement follow that height. Hovering over a mark answers with the pointer cursor. Exact-fill ticks mark the true fill price on every series type, even where a line-type series draws nothing at that price. Marks also accept circle/triangle and quantity sizing. Bounded host round trips add outcome-colored connectors and labels. Host event markers and risk windows use a separate transient overlay layer with bounded IDs, deterministic pixel-column LOD collapse, and dedicated host hit results; they never enter drawings, undo history, or persistence. Linked charts use semantic crosshair and visible-time-range events carrying source and monotonic revision; external application resolves values against local data without re-emitting, preventing echo loops.

Price alerts use the same host-authoritative boundary. The engine retains at most 4,096 typed alert-line indicators and paints them through the canonical pane/axis frame; it does not evaluate conditions, persist alerts, enforce account limits, run background timers, or deliver notifications. An alert line's price tag always shows its formatted price, exactly like every other axis tag; an optional host label remains metadata and never replaces that price. What names the line visually is a badge chip attached to the tag's pane-facing edge, carrying a bell drawn from prims rather than a font glyph, rounded on its outer edge and square against the tag so the pair reads as one control. Active alert chrome derives from the theme-aware muted-text token rather than the primary blue accent; triggered and expired states retain warning and darker-neutral colors. The crosshair price label exposes one engine-rendered multipurpose action chip on its primary price scale: an attached button, rounded on its outer edge and square against the tag with no radius on the tag side, carrying the original circular-plus SVG from `packages/charts/src/assets/icons/add.svg`. Its alpha masks for integer sizes 1 through 96 are generated by the pinned browser rasterizer (`node examples/web_demo/build_crosshair_icon.mjs`, with `--check` for verification) and embedded in `aeris_charts_render` as a bounded run-length asset. Each engine retains only the current size as immutable RGBA pixels; an axis frame shares those pixels and the shared converter emits an integer-aligned image primitive for every backend, including workers. Font/DPR changes select the matching mask; device recovery reuses the retained image. No runtime SVG parser or renderer-specific icon shape is involved. The chip stays visible whenever the crosshair is; hovering it lifts the fill a step with no blue fill. Activating that exact hit zone emits a bounded chart-level action request carrying pane, scale, and price; the browser package forwards it to host subscribers so the host can offer alert, limit-order, horizontal-line, or other context-appropriate actions. The request does not choose an action or carry alert defaults. Alert metadata represents the regular-price `crossing`, directional crossing, greater/less operators and the `only_once`/`every_time` frequencies, plus interval-dependent per-bar, bar-close, and per-minute frequencies. These values are display/configuration metadata only until the host returns an authoritative line snapshot or update. Alert lines and pending action requests are runtime-only and never enter chart persistence.

Official primitives with chart semantics are likewise retained by the engine. Series primitives follow the source across panes and own their bounded data, hit state, autoscale contribution, and pane/axis views; pane-only primitives retain a stable `PaneId`. Delta Tooltip is a non-candlestick interaction: the engine rejects attachment to candlestick series and removes an attached Delta Tooltip if a convertible built-in series later becomes candlesticks, while the ordinary Tooltip remains available for candle inspection. The ordinary Tooltip snapshot is a structured bar inspector rather than a one-value DOM guess: Rust resolves the exact hovered source row and returns its retained Open/High/Low/Close for candlestick, bar, area, line, baseline, histogram, and other ordinary presentations. Scalar host rows already normalize the same value through all four canonical columns, so scalar area/line data has coherent OHLC while an area/line presentation over retained OHLC can inspect the complete bar even though it paints Close. Optional volume is explicitly host-associated through a timestamp-aligned `volume_series`; the engine never guesses which independent histogram means volume. Tooltip chrome reads the chart's resolved surface, foreground, muted text, border, and font at the browser boundary so light/dark theme changes cannot drift from the chart. Brushable Area composes an ordinary Area series with Delta Tooltip and transient `SeriesEntry.area_brush` presentation state. While that helper is attached, primary mouse/pen pane-drag belongs to the comparison gesture instead of starting a competing canvas pan; price/time-axis drags and manual scale unlocks remain the ordinary Area behavior, and the helper never globally changes `handle_scroll`, `handle_scale`, or chart crosshair options. One-finger touch can still pan normally, while the Delta Tooltip's existing two-point touch interaction remains available. Delta Tooltip owns its own comparison guides, and clearing/detaching the interaction simply drops the transient brush state so the untouched Area renderer is restored. Setting or clearing brush state invalidates only that series' retained geometry; it does not advance the series-store revision, so a drag step never rebuilds layout, autoscale, grid, axes, drawings, overlays, or other series. A browser may decode an image or evaluate a user-supplied color/format callback at the platform boundary, but it sends the bounded result back to Rust. The engine stores image watermarks as RGBA8 `RasterImage` values and emits one shared `Image` primitive; Canvas2D, WebGPU, GPUI, and native executors only upload/cache and paint that prepared image. The image contract snaps each destination edge to a device pixel and requests bilinear sampling; ordinary text keeps its separate nearest/rotated sampling rules. Canvas2D requests low-quality smoothing because the HTML standard leaves the exact smoothing algorithm to the browser. WebGPU clamps its image-atlas sampling to the image slot and uploads premultiplied colors. GPUI pads its cached image with repeated edge pixels to prevent its fixed sprite sampler from reading neighboring tiles. GPUI's public image call accepts only byte alpha, so opacity is quantized at cache insertion; the scaled-image pixel gate allows at most one channel value of resulting rounding difference.

An indicator binding keeps its public definition, compact private runtime, and ordinary canonical output series separate. Sparse runtime checkpoints are tied to source row positions and to the source and optional volume-series generations. A tail mutation advances only bindings that depend on that source and installs only changed output rows; a historical mutation resumes from the nearest valid checkpoint and replaces the affected output suffix, while truncation or complete replacement performs a clean rebuild. The five-output EMA ribbon is one binding with one independently checkpointed recursive EMA state per configured period. Its atomic period update retains all output identities and presentation, rebuilds the five value columns once, and propagates the resulting changes through dependent indicators. Removed source/output series drop the binding and its runtime state together. VWAP bands use the same sparse checkpoint boundary for weighted basis, population deviation, and percentage bands, with an engine-owned session/weekly/monthly reset key.

Indicator output metadata is additive and binding-complete: the first output's monotonic series identity is the stable binding identity; every output reports the full structured parameters, source and optional VWAP/VWMA volume source, stable output name, index, and count. Native hosts may also enumerate one typed definition per live binding in deterministic creation/dependency order and recreate it through the generic `IndicatorKind` entry point, remapping source, volume-source, and ordered output identities as they go. This definition snapshot excludes runtime calculation state. Scalar series can also carry renderer-neutral semantic presentation owned by the engine: fixed threshold regions lower into the canonical translucent oscillator channel plus dotted boundary lines, and momentum histograms reuse the canonical four-state market palette while treating whitespace as a reset. Hosts declare those semantics but never receive or retain render primitives or palette logic. The host groups and renders legends from binding metadata; indicator values still come through the ordinary chart value snapshot. Study bindings additionally carry a typed scalar input (`open`, `high`, `low`, `close`, `hl2`, `hlc3`, `ohlc4`, or `hlcc4`) selected at the engine boundary; aggregate inputs are materialized only for the bounded rebuild and never become duplicate canonical series storage. Multi-input bindings align VWAP and VWMA volume inputs by exact timestamp rather than row position, using the documented unit-weight fallback for missing timestamps. Each output also exposes a compact engine-owned style snapshot (visibility, line, marker, area, and directional colors) and accepts an atomic validated style replacement, so per-output styling survives host persistence without kind-specific reconstruction. WASM hosts can query the same bounded parameter/output schema by indicator kind, so property panels do not duplicate engine definitions.

Chart-local custom studies use the same indicator binding, aligned scalar output, chaining, style, legend and value paths as built-in studies, but their computations run through a non-cloneable `CustomStudyRuntime` supplied by a registered factory. This differs from external studies, where the host pushes already-computed values. A binding's runtime is either built-in incremental state or custom state; engine-owned scheduling passes a tail update only after contiguous successful coverage, otherwise the changed suffix or a full rebuild. Registration accepts at most 64 typed definitions with 1–5 outputs each, and a chart owns at most 32 custom bindings. Parameter descriptors and output lengths/numbers are validated at the engine boundary; NaN represents whitespace. Each custom output's leading NaN rows are omitted from its aligned time range, just like built-in indicator warm-up; interior NaN rows remain whitespace within that range, while wholly blank pending or faulted outputs alias an empty source suffix. Thus chained built-ins never consume leading or faulted blanks as numeric prices. A failed callback or invalid output faults that binding, clears every output, rebuilds dependents, queues one of at most 64 retained fault events and awaits explicit retry or a source/parameter change. Per-binding call and row counts expose work without imposing a nondeterministic wall-clock deadline. Custom bindings persist their type, exact version, normalized parameters, output count and dedicated-pane placement with V3 definitions and styles, not their runtime. On import an unregistered or version-mismatched implementation remains pending with aligned whitespace outputs on their restored panes; its pane placement consumes the restore cursor, leaving later studies in place. Matching registration applies plots and output titles while retaining restored styles and panes, then rebuilds the binding and dependents. Pending and previously faulted source updates write only the changed whitespace suffix; a newly faulted binding first clears its formerly valid values. Marker-plot output values use the shared study-marker geometry helper at their exact prices instead of a new frame primitive. The engine and rendering crates retain no browser or GPUI dependency. The main-thread WASM adapter runs `init`/`update`/`rebuild` synchronously, preserves capacity-doubling typed-array mirrors while copying only changed input suffixes, and copies the callback's suffix outputs back into the engine. The TypeScript chart guards callback re-entry with a typed error and delivers queued faults after every guarded WASM path that can compute studies, including ring drains and series pops; offscreen worker charts reject custom-study registration and binding.

Structure-study foundations use schema revision 2: `Choice` descriptors retain ordered options and defaults; ordinary descriptors omit options. The engine accepts an optional runtime-only, host-supplied UTC study calendar of at most 20,000 ordered, disjoint spans. Adjacent spans sharing a session identity merge; uncovered rows have no host session. UTC day, Monday-start week, and month spans require no host timezone. Replacing a calendar rebuilds bindings that opt into host sessions and their dependent indicators, without persisting calendar data. Session high/low develop per UTC day or host session; previous day/week/month levels become available only on the first row of the following period, with a host session's trading day determined by the UTC date of its last included second. Opening-range high, low and midpoint develop for the configured number of seconds from the session start, then freeze until its end. The indicators crate retains sparse session-state checkpoints every 1,024 rows so tip updates process one row and historical repairs replay only the affected suffix plus one checkpoint interval. The indicators crate owns confirmation-ordered structural markers and zones, bounded by retained source rows rather than a global event count: swing points and market structure each emit at most two markers per confirmation row, and fair value gaps and order blocks each emit at most two zones per confirmation row. A source retention trim rebuilds structure bindings cleanly from retained rows, so no annotation retains context from dropped rows. At most 64 zones per side are active; exceeding a study's `max_active` retires the oldest (recorded at the retirement row) without removing it from history. Only active zone indices and scanner state are checkpointed every 1,024 rows; corrections replay no more than the suffix plus one checkpoint interval and bounded pivot/order-block lookback, never the full history as an eviction fallback. Annotation interval trees clear only truncated leaves and reversed zone ends during a repair, update their affected ancestors, and reuse their allocations; tip repairs do not rebuild indexes across retained history. Annotation capacity is included in indicator memory telemetry. Bindings retain those annotations apart from their ordinary output series. All seven structure/session studies place their outputs on the source pane and scale. Swing points, market structure, fair value gaps, and order blocks are typed engine bindings: swing levels are aligned stepped scalar series, while the other three expose one all-whitespace anchor series. The binding owns a pure OHLC scanner, derives annotations during source rebuild and propagates aligned output changes to chained indicators; it consumes complete OHLC candles and therefore rejects scalar input overrides. Only definitions and output styles are persisted; annotations are rebuilt from source data. The shared frame emits clipped zone fills/borders, structure segments/labels, and swing markers in the owning output layer using existing primitives, querying interval-indexed visible annotations instead of scanning retained history; BOS/CHoCH segments read the anchor's line style (dashed by default). Annotations have no hit target. Rust and browser hosts can query a typed annotation snapshot by binding identity.

The breadth tier uses these same bindings and canonical output series. Aroon keeps bounded monotone high/low deques and repairs from the previous lookback window, returning two oscillator outputs with the newest equal extreme winning ties. Awesome Oscillator derives median-price SMA(5) minus SMA(34), warms up after 34 bars, and uses the engine's histogram palette. DPO subtracts the current rolling SMA from the price displaced by half its period plus one bar. Chande Momentum uses a bounded window of signed and absolute close movement. Bollinger metrics share one period/deviation binding but put %B and BandWidth in separate oscillator panes because they have different units. Envelopes retain either a bounded SMA window or the existing sparse EMA checkpoint state; their upper and lower outputs use the shared band-fill frame path. ALMA retains normalized Gaussian weights for its configured period, offset, and sigma so updates read only the affected window. Accumulation/Distribution and Price Volume Trend reuse the engine's timestamp-aligned volume source and sparse cumulative checkpoints; missing volume contributes zero. Chaikin Oscillator derives two checkpointed EMAs from that same cumulative A/D flow. Relative Volume divides the current non-negative volume by the mean of the previous configured bars, excluding the current bar; zero baselines remain non-finite gaps in the ordered output. Volume Oscillator keeps checkpointed fast and slow EMAs of non-negative aligned volume, outputs their percentage gap and an EMA signal, and colors the line-minus-signal histogram through the shared palette. Elder Force applies a checkpointed EMA to close-to-close change weighted by non-negative aligned volume, with the first bar excluded from its seed. Ease of Movement averages midpoint distance times bar range divided by normalized volume over a bounded period, preserving a non-finite gap while a zero-volume bar remains in the window. Historical Volatility retains sparse rolling sum and squared-sum checkpoints for sample deviation of positive-price log returns, scales by the configured bars-per-year factor, and reports percent; invalid-price windows remain gaps. TRIX checkpoints three successive EMAs, their one-bar percentage change, and the signal EMA; a zero triple-EMA baseline leaves a gap and resets the signal seed. Coppock Curve directly weights the sum of configurable long and short percent changes over a bounded smoothing window. Fisher Transform keeps rolling high/low extrema in bounded monotone deques and checkpoints its clamped median-price recurrence; its second output is the previous Fisher line. Ultimate Oscillator computes 4:2:1 weighted buying-pressure to true-range ratios over three configured windows; its bounded row calculation repairs historical OHLC changes and leaves zero-range windows as gaps. KST combines four configurable percentage ROC averages with 1:2:3:4 weights and an SMA signal; TSI uses two successive momentum and absolute-momentum EMAs with an EMA signal; Mass Index sums two range-EMA ratios; Vortex compares rolling cross-bar movement to true range. Klinger uses signed volume force with two EMAs and a signal; KAMA uses an efficiency-ratio adaptive smoothing constant; McGinley uses a guarded dynamic divisor; Linear Regression returns the least-squares endpoint and residual-deviation channel. Choppiness returns a clamped 0–100 oscillator from summed true range relative to the high/low span, with Chop Zone presented as thresholds over that one value. ATR bands return close plus/minus a configured multiplier of Wilder ATR, with close as their middle output. All 29 breadth kinds use the same source invalidation, metadata, style, and V3 persistence paths. Their fixed reference fixture asserts each output against independently derived values; the WASM schema query resolves canonical definitions through the engine rather than duplicating parameter defaults in the browser adapter.

Klinger computes fast-minus-slow EMAs of signed volume force and an EMA signal from timestamp-aligned volume (missing rows contribute zero). KAMA uses an efficiency ratio over its configured lookback to blend fast and slow squared smoothing constants, seeded by a simple average. McGinley Dynamic recursively adapts its previous value to the current positive price and treats invalid or nonpositive prices as gaps. Linear Regression evaluates the rolling least-squares line at the current row and places its upper and lower channels at the configured multiple of the population standard deviation of fitting residuals. KAMA, McGinley, and all three regression outputs share the source price pane; Klinger uses an oscillator pane. All four use the normal binding, schema, persistence and checkpoint or bounded-window repair paths.

Drawing anchors, kinds, styles, pane association, stable z-order, and metadata remain the only authoritative committed drawing state (temporary hover/selection/drag/edit promotion never rewrites it). Built-in tool semantics are described by one compile-time engine catalog: stable wire/name identity, placement class, point-count rule, handle policy, movement-axis restriction, straighten behavior, semantic bounds extent, defaults, and platform-edit requests. Trend-line labels resolve their 3×3 left/center/right and top/middle/bottom positions against the actual segment rather than its bounding box; an inline middle label splits the shared stroke around its measured text extent so no backend paints through the glyphs. The catalog includes Long Position and Short Position as single-click preset tools whose committed semantic points are entry, target/width, and stop. Target and stop are normalized to opposite sides of entry, stop shares the origin edge, and editing uses four dedicated controls (target, entry/origin, width, stop) rather than generic anchor behavior. Their one-click presets open asymmetrically at 2:1 reward/risk, with fill-only entry→target profit and entry→stop loss zones, a thin neutral-gray center entry line, target/stop statistic labels, a central P&L/Qty and risk/reward summary, and filled neutral/green/red owning-scale Y-axis price tags. Position run progress is a derived three-state model bounded by the position's horizontal lifetime. Pending positions emit no progress until the first post-placement candle reaches or crosses entry; that candle becomes the progress origin at the exact entry price. While filled and active, the endpoint is the latest in-box candle's Close/current value, so the active reward/risk side follows current position state rather than a historical or current wick extreme. The first candle after fill to touch target or stop completes the run: target-first freezes at the exact target price and stop-first at the exact stop price; a same-candle target+stop touch is conservatively stop-first because OHLC cannot determine intrabar ordering. The stronger opacity covers only the traveled x/y rectangle from first-fill entry to current/terminal price, never the whole TP/SL zone or untouched empty area, and neither overlay nor connector projects beyond the position rectangle. Reward and risk use the same explicit progress-emphasis opacity, stronger than the untouched base-zone opacity, so an SL run is emphasized exactly like a TP run rather than depending on subtle repeated alpha compositing. The connector remains dashed neutral gray. LOD extrema summaries plus prefix binary search keep entry-cross and first-boundary discovery bounded for long-lived positions; an OHLC gap across entry is deterministically treated as a cross and drawn at the semantic entry level because no exact intrabar path is available. The run overlay lowers through pane chrome so series updates do not rebuild retained drawing geometry, and all statistic labels are emitted after it so the dashed trend never paints over their text. The catalog is not a runtime plugin registry; adding a built-in tool extends this deterministic engine-owned definition rather than teaching each host or renderer how the tool behaves. One chart-local `DrawingController` owns the armed tool/template plus pending anchored placement, captured freehand state, and the transient Shift-click measure. Browser and GPUI hosts forward generic press/move/release/activation/finish/cancel actions and retain only platform duties such as pointer capture, event coalescing, editor surfaces, and repaint scheduling; hosts do not branch on concrete drawing kinds to decide creation behavior.

A chart-local derived runtime maps the existing monotonic `DrawingId` values to conservative logical/price bounds, coordinate-keyed media-space anchor geometry, and pane-local z-ordered candidate lists. Candidate queries first reject drawings in semantic space, then test cached conservative screen bounds; only viewport or pointer candidates rebuild coordinate geometry and reach canonical primitive emission or precise hit testing. Tool anchors resolve into one backend-neutral drawing-geometry vocabulary before either body hit-testing or `Prim` emission, so the interactive body and the rendered body share the same segment/ray/full-line/rectangle/polyline geometry and terminal decorations. Frame construction alone lowers that resolved geometry into the shared render IR; Canvas2D, WebGPU, GPUI, and native/headless executors never receive a drawing kind and cannot fork tool semantics. Full-span horizontal lines, full-height vertical lines, and half-infinite horizontal rays retain explicit unbounded dimensions rather than fake finite extents. The multi-click Path stores two to 100,000 vertices, emits one straight polyline with an open terminal chevron, exposes every vertex for editing, treats the two arrowhead wings as body-movement targets, and commits one history command only when double-click or Enter finishes it; Backspace removes the latest pending vertex and Escape discards the pending path. Brush remains a press-drag freehand placement class: its bounds are computed once on semantic mutation, padded for curved interpolation, and remain conservatively unbounded during an active capture before one exact pointer-up rebuild. Capture decimates pointer samples by distance and leaves the frame untouched when a sample is rejected, so rapid input invalidates the drawings layer only per accepted point. Runtime bounds, resolved geometry, controller state, counters, and index entries are never serialized.

The line catalog also includes directed rays, extended lines, info and angle lines, cross lines, and arrow lines. Ray bounds retain the direction from the first anchor for viewport culling; the shared resolver projects rays and extended lines to the pane edge. Cross lines resolve one anchor into full horizontal and vertical arms. Arrow caps and info or angle labels are emitted in the ordered frame, so backends do not interpret these kinds. Parallel, flat-boundary, and disjoint channels resolve their boundaries once for shared hit testing and fill plus stroke emission; their defining anchors remain the editable, persisted geometry. Regression trend selects a logical window from two anchors, fits the primary or explicitly selected same-scale source's finite closes, and derives residual standard-deviation bands. Its source identity, deviation setting, and derived media geometry are engine-owned; series changes invalidate the retained drawing frame and source generations invalidate the drawing geometry cache.

Fibonacci retracement resolves seven default horizontal levels from two anchors; extension projects a price and time swing from a third anchor, channel offsets a sloped baseline, time zones and trend time project vertical levels from a measured interval, the speed fan projects sloped levels from a shared pivot, speed arcs and circles sample concentric levels around the second anchor, the spiral samples bounded logarithmic turns from the first anchor, and the wedge samples concentric arcs between its two anchor-defined side rays. Each persisted level controls visibility, color, stroke style, optional fill to the prior level, and a label. Projected logical bounds keep future time levels in viewport candidates without expanding every drawing to the full pane. Andrews, Schiff, modified Schiff, and inside pitchforks derive distinct median origins and parallel level tines from three anchors; pitchfan uses the same level and frame contract for rays through the outer anchors.

Gann box resolves two anchors into a price and time grid with styled levels. Gann square adds independently styled and bounded angle rays and quarter arcs to that grid; Gann square fixed resolves a square media-space box from the anchors. The three level families have separate schema, patch, persistence, and fill-between controls. Gann fan projects nine proportional angle rays from its pivot to the pane edge. The shared geometry is used by frame construction and precise hit testing.

Fibonacci, pitchfork, and Gann level tools expose reverse, price/value/percent label visibility, and label alignment through their typed drawing schema and persisted options. Retracement, extension, and channel levels can interpolate positive prices geometrically on request; their frame and hit paths use the same result. Reversal mirrors normalized levels, sends time-zone levels to the opposite side of their pivot, and reciprocates positive Gann fan ratios. Drawing bounds include visible projected levels so a level can enter the viewport even when its anchors are outside it. Position drawings retain their dedicated target, entry, and stop statistics instead of exposing these generic level controls.

Projection uses two anchors for a pivot and a future target; the target's time and price set the horizontal horizon and projected height together. The engine resolves its filled sector to a shared triangle for rendering and hit testing.

Forecast uses two anchors for entry and a future target. The engine checks the primary same-scale source's high or low through the target logical slot; the frame labels a reached target, an expired horizon, or a pending forecast. This status is derived from current chart data and is not persisted separately from the editable anchors.

Bars pattern captures at most 512 finite OHLC rows from a source window into the drawing when placement commits. A third anchor moves the frozen copy without reading the source again. Horizontal and vertical mirroring and bar or selected-price line modes are schema-backed style options. The bounded snapshot is persisted with the drawing and carried through clipboard and cross-cell sync payloads, so a destination chart need not hold the source data. Frame construction and precise hit testing project that same snapshot through the active price scale. Moving a source-window anchor recaptures it when the edit commits; moving the target anchor leaves the snapshot intact.

Cyclic lines and time cycles derive repeated vertical marks from two anchors, limit visible marks to 256, and share the same generation for frame output and hit testing. The sine line samples a viewport-clipped quarter-cycle interval into at most 512 segments; hit testing probes those same segments. Harmonic and chart patterns and Elliott waves are ordered, editable anchor paths with engine-owned vertex labels. Elliott degree is validated with the drawing patch, exposed in its schema, and persisted with the drawing; every executor paints the resulting text from the frame. Polyline and highlighter reuse the variable-point placement classes; the highlighter's alpha and width are lowered in the shared frame. Rotated rectangles, ellipses, circles, triangles, arcs, and quadratic or cubic curves also resolve into shared media-space geometry before either frame lowering or precise hit testing.

Anchored text stores normalized pane coordinates captured from its placement click, so time and price scale changes do not move it; drag, undo, coordinate patches, hit testing, and persistence use that same screen position. Notes, comments, callouts, and price notes use the engine text editor and shared text box/frame path; callouts add a leader and price notes add a pane-wide guide. Price labels emit and hit a single pane-edge badge with engine-formatted price text unless overridden. Directional arrow markers, flags, and signposts resolve filled marker geometry and optional stems in the engine, including their hit areas and viewport padding. Icon stamps store a bounded name and display size; a chart-local RGBA8 registry holds at most 32 images of up to 96 by 96 pixels. Persistence keeps the name, while hosts register image bytes again after load. The shared frame emits one image primitive, which GPUI, WebGPU, Canvas2D, and native executors paint in order; missing images use a colored placeholder. GPUI and Canvas2D retain up to 64 decoded raster resources each, covering all chart-local stamps plus other chart images without steady-state stamp churn.

When a historical insertion, removal, series replacement, or retention trim changes merged logical indices, the engine rebases every drawing-semantic logical snapshot through the data layer's common-timestamp mapping: committed anchors, pending creation anchors and preview, active brush points, active drag start and current snapshots, and both drawing-history stacks. Non-time footprint rebuilds use the bar sequence's full-resolution open/close microsecond identity mapping instead of row keys or truncated seconds, including fractional anchors between bars. Common timestamps map exactly, fractional positions interpolate between them, positions outside their extent extrapolate with slope one, and a replacement with no common timestamp is identity. The transient mapping retains only slope-change breakpoints. Pixel drag/brush baselines refresh immediately for input continuity and once more after the next frame settles layout and autoscale. This maintenance mutation creates no drawing-history command. Non-time drawings persist an optional bounded anchor-time sidecar (open/close microseconds) alongside their logical/price anchors; legacy documents omit it and retain their existing behavior, while restored sidecars resolve when the host installs the matching sequence.

Each chart also owns a bounded runtime-only drawing history of the last 100 committed semantic
create, delete, anchor, style, and clear operations. Pointer-move samples mutate the active drag
snapshot without adding commands; pointer-up records one start-to-end update. Undo/redo rebuilds
only the affected drawing runtime state, a new mutation clears the redo branch, and persistence
never contains either history stack.

B2 extends that owner boundary with a versioned typed drawing contract. `drawing_contract.rs`
defines bounded property descriptors, interval visibility, line caps, magnet modes, labels,
levels, templates, clipboard payloads, and revisioned sync payloads. The live `Drawing` remains
the sole source of truth; its common snapshot and discriminated kind-option projection are
computed views, so a property panel cannot create a second state model. Patches validate all
bounded contract fields on a clone before installation and record one undo entry per semantic
change. The shared resolved-geometry path applies line extensions and is consumed by both frame
emission and hit testing. Hidden or interval-ineligible drawings remain in persistence and the
object tree but are excluded from rendering and hit testing; locked drawings remain selectable
but cannot be edited. Selection, clone/copy/paste, z-order, group operations, bulk removal, and
sync are chart-owned and bounded, with sync IDs/revisions preventing stale or echoed updates.
Named templates are validated data rather than host-side drawing copies. V1/V2 persistence keeps
these fields optional for lossless migration of existing layouts, while browser/WASM exposes the
same schema, template, object-tree, and payload operations as the native engine.

Versioned persistence is an engine-owned semantic DTO boundary, never serialization of live engine
structs. Financial-only charts continue to export V1 with ordered pane topology and built-in
drawings only. V2 adds pane horizontal domains, general axes, typed general datasets (including
row identities and labels), general series bindings, and chart options. V1 restoration and its
fixtures remain unchanged; V2 validates on a detached engine before committing general state and
rebuilds browser series handles from an engine catalog. Pane persistence identity is
separate from live `PaneId`: import preserves document references while issuing fresh monotonic live
IDs, so pre-import pane and price-scale handles become stale. The complete document is size-bounded,
parsed, and validated before one transactional install; drawing bounds, candidates, and geometry are
rebuilt once from anchors. Financial market history and series/indicator definitions, extensions,
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
host-owned series/indicator definitions after the grid restores each Aeris chart document.
Native and browser hosts restore that generic layout through the same typed workspace transaction.
Hosts may issue nonzero stable `u64` cell identities when creating or splitting a workspace; the
engine validates uniqueness and overflow before mutation, owns boundary lookup and absolute resize,
and projects normalized legacy basis-point weights. A host must not replay splits or maintain a
parallel engine-cell-to-host-pane identity map.

### `aeris_charts_render`

Backend-neutral drawing primitives, colors, geometry, bar-width rules, and the ordered `DrawList`. This is the contract shared by every renderer. Pixel snapping, primitive ordering, clipping intent, and geometry must be decided before backend execution whenever possible. Curved polylines expand their Catmull-Rom spline adaptively by device-px interval length — intervals already a few pixels long render as their chord (dense freehand brush samples), long sparse intervals keep up to 16 segments — and round joins are emitted only where a turn opens a visible wedge, so tessellation volume stays proportional to what the pixels can show on every backend. Area fills split segments where their line crosses the base, and band fills use shared crossing triangles where their upper and lower lines swap; neither GPU paints overlapping bow-tie triangles. `line::build_area_fill` records which segments cross, and GPUI traces each resulting lobe separately for its edge fringe from that list rather than by comparing coordinates. Polyline strokes for the GPU backends come from one shared anti-aliased stroker (`line::stroke_aa`): a solid core ending half a device pixel inside the nominal edge, a centered one-pixel coverage transition, faded butt caps, and round joins that fill only the outer wedge of a turn. Neighboring segment triangles are clipped at each joint bisector so translucent inner turns cannot blend twice. A joint clips only when both neighbors extend past the inner geometry each must cover for the other, decided from a fixed four-segment window without allocation; sharp turns between short segments (dense zig-zags, wide strokes) keep both segments whole, accepting a doubly blended inner turn instead of an uncovered notch. It emits each vertex with a signed edge distance, and each executor chooses the encoding — WebGPU multiplies vertex alpha by coverage on top of MSAA, GPUI writes the path shader `st` channel — so both backends tessellate identical geometry. Line points are never snapped to the pixel grid: sub-pixel positions plus coverage are what keep diagonals smooth. The shared draw-list admission rule drops circles with non-finite or non-positive radii and polylines with non-finite or non-positive widths before any executor changes paint state. The Canvas2D contract strokes with round joins and butt caps, matching the reference line renderer. `line::round_rect_polygon` is likewise the single rounded-rectangle tessellation for WebGPU and GPUI, with corner chords scaled to the device radius. For an inside border, `line::round_rect_border` returns the ring triangles together with the outer and inner contours; both executors fan the inner fill over exactly the ring's inner vertices, so fill and ring share their boundary at rounded corners. Shared `line::circle_segments` bounds full-circle chord error to 0.1 device pixel through radius 200, with a 256-segment work cap, and supplies disc, ring, and native arc tessellation.

`Prim::Polyline` dash and dot spans follow the expanded simple, stepped, or curved path through
the shared `line::dash_split` routine; GPUI and WebGPU do not invent separate phase rules. The
walk is capped at `MAX_DASH_STEPS`: a pattern too fine for the path strokes solid, and a
non-finite path length strokes nothing, so host-supplied widths and coordinates cannot stall a
frame.
`Prim::Image` carries immutable straight-alpha RGBA8 pixels. Its destination edges snap with the
shared device-pixel rule and scaled pixels use bilinear sampling. GPUI converts cached RGBA bytes
to its BGRA image-upload order once, while WebGPU and native/Canvas2D keep their respective
platform encodings behind the same frame contract.

### `aeris_charts_render_gpui`

The native GPUI executor. It converts the prepared primitive stream into GPUI scene operations and owns GPUI-specific text, image caches, geometry conversion, backend metrics, and fixtures. It must not fork chart behavior or recalculate engine geometry.

Its `input` module is the one GPUI adapter for the engine input controller, shared by every GPUI host (the `gpui_probe` example and Aeris Terminal). `GpuiChartInput` converts GPUI mouse, wheel (native line units mapped to the shared DOM-equivalent wheel scale), trackpad pinch, modifier, and key events into engine input against the chart canvas's top-left window position (`set_canvas_bounds`) and a monotonic clock; `cursor_style` is the single `ChartCursor` → `CursorStyle` mapping (on Windows, whose GPUI backend draws hand cursors as the arrow, vertically dragged trading lines use the vertical-resize cursor); `text_edit_key` applies platform text-editing conventions to the engine typing session, with clipboard shortcuts layered on in `key_down`; and `install_text_metrics` installs the native text measurer and cap-height metric. A host binds each GPUI listener with one adapter call and never routes chart input itself. The repository's interactive Linux probes enable GPUI's Wayland and X11 platforms; macOS and Windows continue through GPUI's native platform selection. CI compiles and tests the GPUI backend on all three operating systems.

GPUI's path pass cannot rely on MSAA — its sample count is picked from the surface and can fall back to 1x on Linux — so stroke, disc, ring, and filled-mesh boundary geometry, including both edges of inside rounded-rectangle borders, carry a per-vertex Loop-Blinn signed-distance encoding in the path shader's `st` coordinates. Area fills paint their exact-bounds gradient core followed immediately by a separate coverage fringe with remapped stops; this keeps GPUI's bounds-relative gradient from shifting when the edge expands by one device pixel. Polyline geometry comes from the shared `line::stroke_aa` stroker; GPUI only maps its signed distances onto `st`. Polyline strokes keep `s` constant and encode signed device-pixel distance in `t`, which is compatible with GPUI's Windows solid-triangle branch; their one-pixel coverage transition is centered on the nominal edge so integrated coverage remains the requested width. Ring strokes use the same constant-s, centered coverage encoding as polylines, preventing Windows from treating the antialiasing fringe as solid stroke. Filled discs use the same constant-s, centered coverage transition, so their integrated area matches the requested radius even when path MSAA is unavailable. A mesh larger than a bounded chunk is split into multiple GPUI paths so one stroke cannot overflow GPUI's fixed path instance buffer and trigger its grow-and-redraw retry loop; the mesh is a triangle soup, so coverage and paint order are unchanged.

An interactive GPUI host requests another animation frame only for active engine animation or an explicit finite measurement run. Idle charts stop scheduling frames. The executor retains its lowered `ScenePlan`; a host presentation that does not change the canonical engine frame can repaint that plan without lowering every primitive again.
The `gpui_probe` and browser render paths prepare their viewport, base axis labels, and retained
pane frame through `ChartEngine::prepare_financial_frame_with_measure`; each supplies native glyph
widths. The engine caps time-axis labels using the resolved painted axis font size. GPUI lowers the
axis primitives in that operation; the browser inserts host plugin labels into the returned base
axis frame first, then lowers the final axis layer. Browser public API mutations use that same
operation's layout-only phase when synchronous getters need settled geometry before render;
incremental render layout grows axes without shrinking them.
Axis label text and trading readouts share the engine's `set_text_cap_center` metric. The engine
requests it at each painted run's exact size, family, and weight and applies the correction before
device-pixel encoding. Browser Canvas2D supplies alphabetic font bounds and cap ink; GPUI supplies
the native shaper's ascent, descent, and cap height. Neither host selects a separate per-label
baseline correction.

The interactive `gpui_probe` example and `examples/web_demo` keep demo controls in a separate,
scrolling inspector so adding control groups does not reduce chart height. Section navigation,
inspector visibility, and responsive shell layout belong to these example hosts. GPUI uses its
native scroll and keyboard-focus facilities; the browser uses semantic headings, labeled controls,
and a dismissible compact inspector. These shells retain the existing engine/API action paths;
the finite GPUI probe and browser runtime fixtures keep their dedicated measurement layouts.
GPUI paints rotated text through transformed SVG sprites because its shaped-line painter has no
rotation parameter. Each sprite leaves a one-em transparent margin around the measured run while
keeping the same anchor to accommodate font fallback and glyph overhang without clipping the
trend-label ink.

### `aeris_charts_render_wgpu`

The WebGPU executor. It owns quad, triangle, textured-label, atlas, blend, multisample, scissor, and GPU timing resources. GPU objects are reused across frames and rebuilt only when their actual invalidation inputs change.

### `aeris_charts_wasm`

The browser boundary. It exposes the engine through `wasm-bindgen`, decodes typed input, selects WebGPU or Canvas2D policy, executes browser frames, handles shared ring input, text measurement, workspace APIs, and browser telemetry.

The browser boundary translates data and platform events and serializes engine-owned value snapshots. It must not become a second chart engine.

### `aeris_charts_native`

The headless native executor and verification support. It uses tiny-skia for deterministic raster output, golden comparisons, examples, and release performance gates. Native rendering executes each pane's under/main/top layers against that pane's own point pool, inside its integer frame scissor; its `TinySkiaCanvas` applies the same clip to paths, rectangles, images, and glyph coverage. No flattened cross-pane point remapping is needed. Native rendering resolves the requested family, weight, and italic style from installed system fonts plus any faces a host registers with `register_font_data` (so a host with bundled fonts exports with the faces it shows on screen), falling back to sans-serif, then a fixed list of common sans-serif families, then the closest installed face ordered by style, weight, and name rather than OS enumeration order; painting, engine label hit geometry, and the public `measure_text` share the selected face and glyph advances. With no face at all, text is skipped and image export returns an error rather than panicking. Image export goes through the engine's `capture_export_frame`, which builds the pane frame and the axis/top layer directly at the requested output DPR and dimensions (a full measured layout only when the size differs from the live view), restores the live viewport and layout through a drop guard (so a panicking host measure callback cannot leave the chart at the export viewport), and invalidates the retained frame so the live host rebuilds with its own measurements; native export executes the frame background including gradients and paints the axis layer above the panes. Export is split in two: `prepare_engine_image` captures the selected frame layers on the thread that owns the chart, and `PreparedChartImage::render` (or `render_png` for encoded file bytes) rasterizes that owned data, plus host decoration prims drawn in image pixels above every pane, on any thread; `render_engine_rgba` runs both with no decoration. Host decorations such as a product legend or branding are host presentation and stay out of the engine frame. Output is bounded to 32 million pixels. It is evidence infrastructure and the native image-export path, not a competing product model.

## TypeScript package

`packages/charts` publishes the `@aeristerminal/aeris-charts` browser API through GitHub Packages. It owns WebAssembly initialization, TypeScript chart handles, DOM canvas lifecycle, resize observation, Pointer Event translation for mouse/pen, cancellable Touch Event translation for direction-dependent page-scroll arbitration, platform capture/default policy, host callbacks, themes, shortcuts, offscreen support, and grid helpers. The root entry remains framework-neutral. The optional `@aeristerminal/aeris-charts/react` entry is a thin lifecycle/reconciliation adapter over those same public chart handles: React mounts one ordinary chart, applies option/data changes to retained engine objects, and disposes through `chart.remove()`; it owns no scale, geometry, hit-test, persistence, or rendering semantics. Its module performs no DOM work at import time, so SSR can import it without constructing a browser chart. Drawing-tool arming and pointer events cross the browser boundary through the generic engine drawing controller; the package does not classify a tool as single-point, multi-point, sequence, or freehand, nor duplicate tool-specific placement state. Touch Events normalize into the same engine resolver rather than a parallel gesture state machine; static `touch-action` stays `auto`, and the host applies the reference-informed vertical-priority direction rule after the shared slop. Wheel samples retain floating-point deltas; `wheel_behavior: "auto"` independently maps vertical deltas to time zoom and horizontal deltas to time pan on every chart surface. The zoom anchor is engine-owned (`ChartEngine::wheel_zoom_time_scale`, reached through the input controller by GPUI, browser DOM, and offscreen wheel events): measured against TradingView, an ordinary notch changes bar spacing by exactly 10% and, because `right_bar_stays_on_scroll` defaults to `true`, preserves the right offset in bars so the latest bars stay put; Ctrl/Cmd + wheel (including macOS trackpad pinch) zooms around the pointer. Two-touch pinch in browser and offscreen workers is recognized by the controller's touch resolver from ordinary pointer input, and native trackpad pinch calls `ChartEngine::input_pinch`; both are direct manipulation and always zoom around the fixed starting centroid. Pinch enablement is independent of wheel zoom. Explicit `"pan"`/`"zoom"` modes remain host overrides. Auto mode zooms the time scale over a price axis by default; `price_axis_wheel_zoom` opts both browser and offscreen hosts into axis zoom through the same engine controller.

Browser accessibility is chart-owned, enabled by default, and represented by one singleton controller exposed through `chart.accessibility()`. `enable_accessibility(chart, options)` configures that same controller for compatibility. The chart container is a named group; canvases are hidden from assistive technology and each pane has one complete application-style keyboard surface. Default streaming announcements are off, user-driven navigation/actions remain announced, visible data queries are capped at 512 on-demand logical points, and only the active series owns one engine-rendered focus primitive. Accessibility focus/edit state is runtime-only and is never persisted. Pointer interaction updates ordinary chart selection and hover without moving DOM focus into the application surface; keyboard traversal and explicit accessibility API calls own its visible focus. Forced colors, higher contrast, reduced motion, locale, host names, and visible focus are resolved at the host boundary; the shared engine retains exact focus geometry and keyboard drawing mutations use the same drawing history/rollback path as pointer input.

Auto-size keeps `ResizeObserver`'s exact device-pixel path. A resolution media-query watcher plus orientation/fullscreen fallbacks re-run sizing when DPR changes without a CSS-bounds change; resize reprojects semantic state and does not create new object identities. While auto-size is active, manual `resize` calls are ignored. Disabling it disconnects the engine-owned observer and returns authority to manual sizing; re-enabling immediately adopts the current container. Hidden or detached containers retain the last usable size and adopt their new bounds when revealed.

The package also ships `aeris_charts.css` as the portable host design system. Its complete brand token contract remains intact even when a token is currently consumed only by Terminal or the website; Charts consumes the applicable surface, border, text, status, control, interaction, icon, action, focus, radius, shadow, and market roles without renaming them. Host chrome uses the system UI font stack and may use `color-mix`. The published package does not include a webfont. Native CPU text selects installed faces for the requested family, weight, and italic style; the native golden scene masks its text region for the exact bitmap comparison and separately requires visible glyph ink across system fonts. Backend-facing roles — the primary surface for chart panes, primary text for axes, border for axis rules and pane separators, canonical border width, muted text, separator interaction, focus/primary interaction, positive/negative market semantics, and shared radii — have deterministic opaque-sRGB projections in `crates/aeris_charts_core/style_tokens.json`. `aeris_charts_core` owns and compiles that file into the defaults used by every engine and backend. Axis borders and visible pane separators both project the shared border-width token onto the device-pixel grid in the engine; the separator hover target stays independently expanded for interaction. The TypeScript package imports the same source at build time for host theming and workspace divider projection. The core crate therefore remains independently packageable without reaching into a browser-package directory, and the tokens resolve before frame construction rather than through demo or renderer overrides. Canonical engine grid lines retain their dashed style and border color but ship disabled; hosts and deterministic parity fixtures may opt either family in explicitly. A `v*` tag matching the package version publishes the verified artifact to GitHub Packages.

The package preserves its complete `snake_case` surface and adds camel-case aliases for the common JavaScript chart/series/scale lifecycle without creating parallel state or handles. Financial and general series use the same chart object and ordered frame. Data crosses into WebAssembly in typed columns or bounded shared-ring layouts rather than per-point object calls on hot paths. Typed update batches transfer their sanitized owned columns to the engine's batch entry point; the browser wrapper never loops through the single-row engine API. The published artifact exports the optimized WASM asset explicitly and the generated glue also resolves that sibling asset by `import.meta.url`; source-tree `pkg/`, crate, benchmark, and demo paths are not runtime dependencies. `examples/web_demo` remains an integration and parity test host, while `examples/all_in_one` contains consumer-facing framework-neutral and React compositions.

Financial appearance keeps theme provenance typed in the engine. Grid, crosshair, bullish, bearish,
wick, and border colors are either semantic theme followers or explicit custom colors; theme changes
retokenize only followers. Native hosts consume and apply the typed financial appearance transaction
and must not infer provenance by comparing resolved CSS strings, create dummy engines for defaults,
or send empty-string color sentinels to clear series overrides.

The engine also projects the ordered financial legend model from its canonical value snapshot. It
owns primary OHLC formatting and tone, native indicator output grouping, external-study grouping,
visibility, pane placement, output labels and colors. Hosts may describe a bounded set of genuinely
product-owned series roles (for example Terminal's reusable volume series) and then map the returned
typed identities to their UI controls; they must not rebuild engine-owned groups by walking series.

`chart.value_snapshot(logical_index?)` crosses WebAssembly once and returns all live series. The package adds live handles to the engine records and derives legacy crosshair `series_data` by retaining only valued entries. Engine-owned feature series expose their scalar scale projection and retain the legacy scalar event shape. Arbitrary custom-series callbacks remain host-owned: exact snapshots are null, while latest snapshots can expose only the last value recorded during a visible frame and are explicitly render-state-dependent. Symbol/exchange metadata, volume association outside VWAP bindings, bar/day change math, session calendars, visibility settings, and legend DOM remain host-owned.

The supported, experimental, internal-but-exposed, and legacy surfaces are classified in
`Public_api.md`. Predictable browser failures use `AerisChartsError` with stable category codes;
clean ingestion retains a null diagnostics fast path. The generated WASM surface and benchmark/test
hooks are internal even when visible to developer tools. A deterministic declaration manifest makes
supported TypeScript surface changes explicit in CI.

`chart.remove()` is the single public browser lifecycle operation. It is idempotent and transitions the retained TypeScript handle to a disposed state after cancelling scheduling, detaching browser resources and extensions, releasing per-chart GPU state, explicitly disposing the Rust object, and calling the generated `free()`. Later operations fail with a stable disposed-state error. Offscreen charts use the same explicit dispose-then-free ordering.

## State and frame ownership

Each chart has one engine owner. Mutations invalidate only the state that changed. A frame is a deterministic snapshot of engine state for a viewport and device scale.

Coordinate-authoritative scale objects advance canonical revisions inside their mutating methods. Browser and GPUI hosts express gestures through `ChartEngine` commands; legacy direct Rust access remains coherent because it cannot bypass the scale-owned revision. `SeriesStore` likewise advances its canonical presentation revision whenever a Rust host takes mutable access, replacing read-side hashing of every style field. Retained coordinate-dependent layers are derived caches stamped with the engine's current coordinate revision. Frame assembly asserts that the grid, visible series, chrome, drawings, and interaction overlay all carry that same revision, so a frame cannot mix transforms.

On every host the engine input controller owns the complete pane/time-axis/price-axis/separator
drag state machine, separator and axis target resolution, crosshair exclusion at dividers, and wheel
pan/zoom routing. A platform adapter translates OS events and maps `ChartCursor` to one platform
cursor, and schedules repaint; it must not reproduce the gesture lifecycle or retain parallel press,
drag, hover, or cursor state.

Frame invalidation is an engine-owned generation graph. Layout, coordinates/autoscale, grid and underlay, each series, drawings (with per-drawing prim/point segments plus a trailing controller-owned creation-preview block), and interaction overlays have independent generations. Coordinate-range changes fan out to coordinate-dependent layers; a value-only current-bar update stays on its source series when autoscale bounds do not change. Ordering-only promotion (hover/selection/drag/edit) reassembles retained series layers and drawing segments without rebuilding geometry; drawing drag rebuilds the drawings layer with fresh segments while reusing the runtime per-entry cache. Public option and series-style mutation are included in the generation inputs, so direct native callers cannot bypass retention accidentally.

Series and indicator selection owns one transient engine snapshot with a single primary command target and at most 64 related output members. Engine-owned indicator bindings expand automatically; hosts may supply the bounded member identities for study groups they author outside the built-in indicator registry. Each member retains at most 128 canonical output timestamps sampled from its own full canonical start-to-end extent only on the unselected-to-selected transition. Selection-time projection determines sparse density, while endpoint-inclusive logical spacing prevents a partial-series selection treatment. Overlay rebuilds resolve every member's identities against its current canonical values and coordinates, place candlestick handles at the current body midpoint, clip offscreen handles without replacement, and discard the snapshot on deselection; LOD geometry, screen coordinates, and persistence never own selection-anchor membership.

Drawing semantic mutations reuse this graph: add/remove/style/anchor changes invalidate the drawing layer and update only the affected derived entry, while selection/hover/drag/edit promotion reassembles retained drawing segments without rebuilding geometry and selection changes additionally invalidate the overlay and axis frame for handles. Drawing selection handles are assembled at the beginning of the overlay, preserving their prior canonical order immediately after drawing bodies and before crosshair/series overlays without rebuilding unrelated drawing geometry. Temporary promotion (dragging/editing → hovered → selected → idle, hover gated by `hoveredSeriesOnTop`) never rewrites saved drawing z-order; deselection, hover leave, cancellation, or removal restores it. A selected price-spanning rectangle also emits primary-colored extent tags and a territory band on its bound price scale; those axis views follow creation, drag, and resize coordinates and disappear on deselection unless the drawing explicitly requests persistent axis views. The text tool is an exception: it emits no anchor discs — selection and hover paint the same focus border box (hover at reduced opacity), empty text paints nothing on the chart, and leaving the editor without typed text removes the drawing. Typing is an engine-owned session (`drawing_text_edit.rs`): it holds the live text and char-based caret, applies label changes through the ordinary drawing-option path, and owns Enter/blur commit, Escape restore, and the per-kind empty lifecycle. Hosts only forward input. The browser keeps a borderless content-editable surface with transparent glyphs for IME, clipboard, and accessibility, mirrors its value and caret into the session, and paints its own caret; native hosts forward committed characters and editing keys, and the session paints the caret as a label-rotated polyline in the drawing's own frame segment. Either way the engine keeps painting both the label and the focus border, so edit entry cannot lift the text or shift the outline. Crosshair movement and unchanged-coordinate market-data updates do not invalidate drawing geometry. Pane add/remove/swap/move rebuilds pane membership because pane ownership itself changed; ordinary drawing drag updates one entry, and structural removal repairs the canonical vector's id-to-position map.

The browser text editor reads its resolved font, aligned run edge, rotated anchor, measured
advance, and caret position from ChartEngine::drawing_text_edit_layout through WASM. Its
transparent content-editable element supplies IME, selection, and clipboard input; it does not
measure the run or derive a separate baseline. The engine layout query uses the same text
placement and measurement functions as the drawing frame.

The engine-owned crosshair overlay can paint the same configurable hover marker for every visible line, area, baseline, and line-shaped indicator output at the snapped logical index. Markers ship disabled and hosts opt in per series or indicator output; when enabled, marker coordinates, per-series colors, borders, pane ownership, and scale conversion are resolved before the shared frame reaches any backend. Crosshair and drawing magnets share one pixel-space candidate path: candle, bar, and footprint series expose their rendered OHLC fields, while line, area, histogram, baseline, and other scalar projections expose only the close/value they paint, so hidden storage columns cannot attract an anchor. An empty hovered trend line emits a low-opacity, borderless `+ Add text` run at its configured segment-relative slot; the engine owns its measured hit box, exact caret anchor, and middle-slot stroke gap, and the shared typing session above owns the edit itself. Clicking either this affordance or existing trend text enters inline editing, and leaving an empty trend edit preserves the drawing. Bar-slot highlights and tooltip guides resolve their default tint from the current chart surface, using a light lift on dark surfaces and a dark tint on light surfaces; overlay price-scale text follows the current layout foreground. Explicit host colors remain authoritative, while implicit colors retokenize with chart options. Chrome that stands for a bar itself — the built-in live price line, its last-value axis chip, and the crosshair marker — follows one shared bar-color resolution. The built-in live price line is one canonical series feature: its default `partial` extent starts at the tracked bar/value and reaches the pane's right edge, while `full` is an explicit per-series option. Both extents use the same source, color, width, and solid/dotted/dashed line-style state, so ordinary series and engine indicator outputs cannot drift in thickness or dash semantics. Explicit user-created horizontal price-line objects remain full-width independent chart objects. For candlesticks that resolution walks the parts in paint order, body then border then wick, skipping any part that is transparent or switched off, so a hollow candle (a transparent body over a visible border frame, industry-standard) keeps its bullish or bearish color instead of resolving to an invisible fill. Bar presentation remains engine-owned as well: OHLC bars keep one vertical high/low body and independently gate the open and close ticks, so setting both visibility flags false produces an explicit high-low bar without a host-side geometry fork.

The engine retains semantic pane layers and their ordered primitive/point ranges, then assembles the same canonical `ChartFrame` contract from clean and rebuilt layers. The retained boundaries are underlay/grid, individual series, per-drawing segments plus a trailing creation-preview block, pane chrome, transient trading risk/reward preview regions, financial-action lines/controls (trading and alerts), and overlay. Frame assembly is the single pane-local ordering owner: grid/background → idle indicators → idle drawings → ordinary price series → active objects (dragging/editing → hovered → selected, series before drawings within a tier, previews trailing active) → chrome → trading regions → trading/alerts → overlay → top. Indicator outputs move as one visual group with internal ordering preserved (bindings own grouping, never series type or title); explicit `set_series_order` overrides default idle series grouping while idle drawings stay below price series and indicator-only panes keep stable internal order. Axes, crosshair, and financial-action controls keep their protected layers above all chart content; active chart content stays clipped to its owning pane. No public z-index API or renderer-specific policy exists. Preview regions sit above chart content with financially actionable lines, alert indicators, and exact control hit zones above them and below crosshair transients. A series' live-price cluster stays filled while its value is live, and is outlined — chart-surface fill, semantic color as an inside border and text — once the series' final bar scrolls out of view, so a stale value never reads as the current one. Colliding series clusters are spaced by the overlap pass using each cluster's full height rather than restyled. Boxed price-line and drawing tags take the nearest free vertical slot around those clusters and earlier tags on the same axis. Placement starts from each tag's price coordinate every frame, so it returns when the obstruction clears; a tag with no free slot is omitted until space returns. Otherwise-solid trading tags that meet the primary cluster's raw axis region take that same outlined treatment, but financial-action tags stay at their exact price coordinate instead of being collision-shifted away from the line they identify. They emit before the primary cluster so the live price remains visually authoritative if exact coordinates overlap. Confirmed orders never manufacture persistent risk/reward fills. Positions and orders use a 304 CSS-pixel bounded marker beside the price scale. Each order and position rule extends by default from the pane's left edge to the marker end and passes beneath the opaque marker container. Working entries and positions expose separate compact `TP` and `SL` drag handles immediately before the marker for each missing protection; their visible rule is a readout, not an implicit protection handle. Keyboard adjustment of a working entry has no handle to aim at, so the protection it creates takes its role from the side of the entry it moves to (toward profit is `TP`, toward loss `SL`); pointer drags from the dedicated handles keep their fixed role. Confirmed TP/SL order rules remain directly draggable at line tolerance, while every dedicated action and close cell retains its larger control-height target. Each marker is a taller readout chip — a solid quantity cell and then P&L or order type inside ONE outline, with no inner border or divider, so the quantity block's own edge is the seam — followed immediately by an integrated close/cancel cell on the P&L/readout's right. The single main container and its edge cells use the shared large `999px` radius token, which the engine resolves with CSS clamping semantics to form one pill. Outline and quantity fill carry the object's semantic color, so the marker reads as one color from the line to the price tag; only the P&L text keeps its own profit/loss tint. Every close icon uses its marker's semantic line color in idle, hover, and pressed states, so line, outline, quantity fill, and close icon read as one color. The close icon is two anti-aliased `Polyline` diagonal strokes from the trading layer's own point pool rather than a rotated font glyph, skewed filled bars, or triangles with cap discs: a host `font_family` is not guaranteed to carry a suitable symbol, and `Polyline` is the one stroke primitive every executor antialiases identically. Protection role takes precedence over broker side and kind: TP is positive green and SL is warning yellow. Every ordinary buy order, including a resting buy limit, uses the positive token; every ordinary sell order, including a sell limit, uses the negative token. Positive and negative P&L text uses those same semantic roles. Rejected, cancelled, and expired markers use the rejected color, while pending broker operations use the pending color. A price tag is solid only once its order is actually filled — a resting order stays outlined, so a working intention never reads as an executed one. Every main marker pill uses a solid semantic outline at the design-system `--border-width`, snapped like a browser border and emitted as the container's `RoundRect` border — every executor (WebGPU, GPUI, and the shared Canvas2D executor behind browser Canvas2D and native CPU output) paints `RoundRect` borders inside the rect, never centred across its edge — with a deliberate chip-surface gap between it and the edge cell fills, while the integrated close cell keeps its full-height hit target but paints no resting surface of its own; its icon sits directly on the container surface. Hover and press fill the addressed control (for the close cell, a smaller inset, fully rounded pill) with the brand `--hover-bg` / `--active-bg` surfaces rather than the order color, and the close cell answers hits across the pill's full height rather than the line tolerance. `ChartEngine::trading_cursor_at` is the one grab/pointer affordance every host uses: a draggable line reads as draggable exactly when a drag would start there, while the close cell and the `TP`/`SL` protection buttons read as buttons with the click cursor even though a protection button also accepts a press-and-drag. Those fills stay OPAQUE: a control sits on top of its own marker line, and a translucent fill would let that line read through the button the pointer is on. For the same reason the engine suppresses the crosshair lines while the pointer is over a trading control — the control is not a price to read. Action tooltips are host-timed: the engine owns no clock, so it reveals one only once the host arms it after a hover dwell, and a changed hover disarms it. Sweeping across stacked markers therefore never flashes a tooltip per marker. An action tooltip is chart chrome rather than part of the object it describes: it takes the active theme's surface, border, text, and radius tokens, never the order's buy/sell color, so it reads identically on every line and in both themes. Because `RoundRect` borders paint inside the box, every bordered chrome box (action tooltips, annotation chips, the Delta Tooltip and its delta band) snaps each edge independently to whole device pixels through the frame's one `DeviceBox` rule; a fractional edge would smear the one-pixel border across two rows on some sides only. The browser's DOM bar-inspector tooltip applies the same rule by translating to device-pixel-snapped coordinates against its containing block instead of a `-50%` translate. Confirmed TP and SL orders remain ordinary host-authoritative order markers with quantity and projected P&L, endpoint nodes, and action tooltips. Trading, alerts, and axis labels share the canonical pane price transforms; WebGPU, Canvas2D, GPUI, screenshots, and native/headless consumers receive no separate financial-action geometry. Retention never gives a backend permission to change ordering or semantics. Host/plugin primitive callbacks use the canonical frame but conservatively rebuild the affected pane stream because their output is not engine-owned. Incremental frames are tested against forced clean rebuilds across data, interaction, scale, drawing, theme, and resize mutations.

Trading interaction is one engine-owned state machine: idle, hovering, dragging through a local preview, or awaiting a host answer while holding that change's rollback. These states are mutually exclusive. Bounded hover and pressed hits are retained separately as visual feedback, so actionable buttons continue to respond while the semantic state is awaiting confirmation without those visuals becoming broker state. A working entry or position starts protection creation only from its dedicated missing-role `TP` or `SL` handle. The chosen role stays fixed for the drag, and validity is checked against the entry side on release (buy/long: lower SL and higher TP; sell/short: higher SL and lower TP); the engine emits `create_stop_loss` or `create_take_profit` and leaves identity assignment and the authoritative child order to the host. Limit and market entries share this behavior, including filled market entries. Once supplied by the host, an SL or TP is an ordinary protection order whose role stays fixed even when dragged across its entry; its modify intent preserves the authoritative kind and stop-limit trigger price. Pointer movement changes only the local preview; release applies an existing-order move or emits a protection creation intent and holds the appropriate rollback until the host answers. A position or entry that already carries one protection omits that role's handle, and one carrying both exposes neither. Escape discards a live drag. Rejecting an emitted intent runs its rollback — reinserting a closed order or position, moving a modified order back, or doing nothing for an unmaterialized protection request — while acceptance releases it and the host's snapshot remains the last word. The vertical entry-to-preview connector exists only during the active placement transaction. Host acknowledgement clears the group visual, so confirmed entry, TP, and SL objects never leave persistent connector chrome behind.

Trading quantity cells use the host's canonical text measurement plus bounded horizontal padding, so the visible cell and close-control hit geometry respond to the formatted quantity without a fixed empty allotment. Every trading control's text (TP/SL buttons, quantity, P&L or order type, annotations, and tooltips) is optically centered in its box rather than left on the `Prim::Text` em-box middle, which sits capitals and figures visibly high in a padded control. The host supplies one vertical glyph metric through `ChartEngine::set_text_cap_center`: the browser host derives it from `measureText` ink bounds of a figure, and the GPUI host installs `text_cap_centerer`, which derives it from the font's cap height. Every cell of a marker shares that one offset so adjacent readouts keep one baseline, and marker text anchors on the snapped pill's own center. A host without the metric keeps the geometric center.

Panes expose opaque, monotonic chart-local identities at the browser boundary. A live pane or
price-scale handle resolves its current index after moves or swaps; removal permanently invalidates
that handle, so later index reuse cannot retarget it to another pane or scale. Persistence uses a
separate stable pane identity and intentionally issues fresh live IDs during restore.

The ordered frame contract contains pane backgrounds and grids, idle indicator geometry, idle drawings, ordinary series geometry, active series/drawings/previews, custom-series contributions spliced at their paint marks, pane chrome, trading regions, trading/alerts, crosshair overlays, axes, labels, and text, plus per-series and per-drawing segment ranges for retained backend groups. `series_order`/`drawings` stay the stable saved orders; the frame derives the effective paint order without rewriting them, and series/drawing hit tests tie-break on stable order so promotion cannot oscillate hover. Backends preserve ordering, clipping, blending, and coordinate conversion. A backend may batch compatible adjacent primitives only when visible output is unchanged.

Trend-line labels are owned by the trend-line feature rather than by `DrawingKind::Text`: the
engine owns their text state, dedicated hover affordance and hit region, edit-session identity,
segment-local transform, and middle-stroke cutout. New trend labels default to the top-right slot;
their 3×3 slots resolve along and perpendicular to the actual segment. The direction is normalized
into the readable half-plane, including a
deterministic vertical orientation, so endpoint crossing preserves visual left/right and never
turns glyphs upside down. Top and bottom slots clear the stroke by the 1.2em line box's half-height
plus the text padding. Pointer hits are inverse-transformed into the measured local text
rectangle. An unset trend-label text color follows the drawing stroke dynamically; an explicit text
color remains independent. Empty labels use that same resolved RGB at reduced alpha for
`+ Add text`; entering or
leaving the dedicated trend-label editor never converts or deletes the trend line. Middle labels
split the stroke in segment-parameter space using measured advance plus padding. Hover reserves the
prompt advance; editing starts with a compact one-em caret opening and expands from shaped text
advance as the user types. Top and bottom slots never cut the stroke. The browser uses a fully
transparent borderless editing surface (including native caret and IME composition paint) plus one
explicit colored caret at the engine's exact anchor and angle, leaving the frame as the sole glyph
owner. Its selection pseudo-element is transparent as well, preventing browser selection/IME paint
from leaking theme-colored duplicate glyphs during live transforms.
Standalone Text retains its separate create/remove lifecycle and explicit toolbar text input.

Segment-following text is an explicit `RotatedText` frame primitive carrying the final aligned
anchor, clockwise angle, font, weight, italics, size, color, and text; no executor reconstructs
trend geometry or silently ignores the angle. Canvas2D translates and rotates around the anchor
before `fillText`; WebGPU sends only rotated runs through a dedicated vertex pipeline that rotates
and bilinearly reconstructs premultiplied coverage from the cached glyph-atlas quad while
preserving the ordinary-text instance/shader contract;
tiny-skia resamples a local glyph-coverage raster around the same pivot; GPUI uses its transformed
monochrome-sprite path backed by its atlas. Browser and GPUI caches key glyph-dependent inputs and
subpixel phase but deliberately exclude angle, so endpoint motion reuses glyph coverage. Their
fixed-capacity/LRU or atlas budgets bound retained entries, and font, DPR, device, or atlas-generation
invalidation drops stale resources.
The browser WebGPU presentation surface uses premultiplied alpha, and the chart background clear
premultiplies its configured color so translucent backgrounds composite with the same host surface
as Canvas2D. A non-empty text run that cannot enter the WebGPU atlas, including an oversized run or
an unavailable text rasterizer, routes the complete ordered frame through the warm Canvas2D pane
for that presentation. The failed GPU group is rebuilt on the next frame, allowing WebGPU to resume
after the run is removed; no executor silently omits its glyphs.
GPUI's rotated sprite uses the same measured middle baseline as ordinary GPUI text. The SVG
rasterizer resolves the requested CSS font family independently of GPUI's text shaper, so a missing
family may produce different glyph shapes or advances even though the baseline aligns.

One-click bracket placement crosses the drawing/trading boundary only through an explicit engine command. A host passes a Long/Short Position drawing identity plus its own quantity; the engine reads the drawing's semantic entry, target, stop, pane, and price scale, snaps all prices to instrument ticks, and emits one atomic `place_bracket_order` intent. It creates no speculative order or position. The broker host owns submission, venue-specific entry interpretation, generated order/bracket/OCO identities, acceptance or rejection, and the authoritative snapshot that materializes the resulting lines. The web demo's quantity input and intent handler are an example host, not account-sizing or broker policy inside Aeris.

Long/Short Position creation, body movement, and entry/target/stop handle drags resolve prices to
the instrument `tick_size`, falling back to the bound price scale's display `min_move`. The engine
converts the original `f64` pointer coordinates directly to price before rounding to ticks, so
ticks smaller than one device pixel remain reachable. Horizontal creation and entry/extent handles
use the vertical crosshair's shared time-slot resolver, including its visible-range and hidden-series
rules. Body movement applies the difference between the pointer's starting and current crosshair
slots to every anchor, preserving width and grab offset while holding between slot changes. Future
empty slots remain editable unless an explicit data-time constraint applies. Square position controls
emit one opaque, theme-filled `RoundRect` with rounded corners and a device-snapped inside border;
all executors receive that same fill and border geometry.

Position statistics are engine-owned pane chrome: target/stop distance, percentage, price ticks,
projected account Amount, and a two-line Open/Closed P&L, Qty, and risk/reward block. Persisted,
validated drawing options `position_account_size` (default 1,000) and `position_risk_percent`
(default 25) define a hypothetical risk budget. Qty divides that budget by stop distance and the
instrument point value; quantity display uses instrument precision (default three decimals).
These estimates do not submit orders or change host-authoritative broker quantities. Closed P&L
uses the same first-boundary, stop-first-on-ambiguous-OHLC run resolver as the progress overlay;
open/unfilled/future drawings use the right-edge or latest available close. Zero-risk or unavailable
values render as an em dash. Statistics emit opaque rounded containers with one device-snapped
inside border. Border black/white is selected by maximum sRGB contrast against the canvas;
text contrast is resolved against the opaque container. Every executor consumes those same shapes.

Measuring tools are catalog entries rather than a separate subsystem. Price range, date range,
and date-and-price range are two-anchor `ClickAnchors` tools whose anchors, like Long/Short
Position, carry the catalog's `grid_snap` flag: creation, anchor drags, and body moves resolve x to
the crosshair's time slot and price to the instrument tick (falling back to the bound scale's
`min_move`), so statistics read whole bars and ticks. The shared resolved geometry gives them one
oriented `Measure` body (start → end) that both hit testing (the measured area is the drag surface)
and frame lowering consume. Lowering emits a translucent device-snapped `Rect` area, crisp `HLine`
price-level or `VLine` time-boundary rules, crisp arrow shafts through the area center with an
open-chevron `Polyline` tip on the shaft's exact pixel center, and the shared opaque, borderless statistics
label beyond the end level (below the area for the date tool). Labels show the signed price change,
percentage, and ticks, and the signed bar count with elapsed time from the time axis's canonical or
display-projected timestamps; elapsed time is omitted where no timestamp exists. Rising or forward
measurements use the drawing color and falling or backward ones the market-down token, which also
colors the active axis tags. Because elapsed time can change without any coordinate change (a bar
arriving in a projected slot, a projection change), frame preparation invalidates the drawing layer
when the timestamp-union generation or a projection revision changes and a measuring tool or
measure exists. Viewport culling reserves the label's reach beyond the area so a visible label is
never culled with an off-screen area.

The Shift-click quick measure is transient state owned by the `DrawingController`: a
date-and-price range bound to the pressed pane's default price scale. A live measure consumes the
next pane press before object hit tests (freezing a following measure, dismissing a frozen one);
otherwise the engine input controller starts it from a Shift press after trading, alert,
armed-tool, drawing-drag, and Delta Tooltip arbitration declines the press. The end anchor follows pointer
movement with or without a held button, clamped into the pane; a release beyond the shared 5 px
click slop freezes a press-drag measure, while a click leaves it following until the next press.
Arming a tool, Escape (`cancel_drawing_tool`), host cancellation, and persistence restore clear it.
It is painted after creation previews in the trailing drawing preview block, keeps the crosshair
visible and its cursor while following, rebases with drawing logicals, and never enters drawings,
history, sync, or persistence. Browser and GPUI adapters forward normalized input to that
controller, so all hosts share one measuring state machine.

## Plugins and host extensions

User-defined custom series and primitives remain explicit host boundaries. The engine owns their identity, layout participation, hit-test context, autoscale contribution, and built-in chrome integration. A host may execute an arbitrary user callback, then records the values the engine needs for the next canonical frame. The official plugin implementations above do not use that callback path.

Extensions must not receive unrestricted engine internals or create a second scene graph. Add extension surfaces only for current consumers with a stable semantic need.

Browser pane and series primitive `text_views` lower into ordered `Prim::Text` commands in each
owning pane's top layer. The shared pane scissor clips them before the axis layer, and Canvas2D,
WebGPU, GPUI, and native executors consume the same font, color, baseline-adjusted anchor, and
paint order. The transparent browser overlay is reserved for input and DOM effects.

Disposal invokes every registered extension teardown exactly once; one failing JavaScript cleanup hook cannot prevent the remaining hooks from running. The WASM chart releases its retained resources synchronously; the browser frees its wasm-bindgen wrapper in the next microtask, after any device-loss or frame callback borrow unwinds.

Extension rendering is host-timed, non-reentrant with chart mutation, and error-contained at the
host boundary. Extension runtime objects and callbacks are never persisted by the engine; hosts own
their configuration and restoration. The current custom-series and primitive APIs are experimental,
not a second plugin framework.

The browser package's official-feature modules are thin lifecycle and platform adapters over these
engine owners. They normalize public data/options, translate pointer or keyboard events, decode
browser images, and create optional DOM chrome; they do not simulate financial geometry. Tooltip
guides/value lookup, accessibility focus geometry, drawings, bands, price lines, overlay
labels, image placement, and every specialized series frame are constructed in Rust. The engine and
its browser/native hosts do not inject product attribution or branding into chart surfaces. Feature
handles release their engine primitive
plus any host subscription,
timer, or DOM node exactly once; none of that runtime state enters engine persistence.

The engine input controller owns the crosshair-action chip as a press target: hovering it resolves the pointer cursor, its press prevents chart pan and selection underneath, and an unmoved release within the hit area emits the shared action request. Pointer cancellation discards the pending press. The GPUI demo shows the request's pane and price in its status line.

Interactive chart objects own pointer feedback: the shared frame suppresses the complete visual crosshair (lines, markers, and axis labels) while any trading object or drawing is hovered, created, or dragged. A following Shift-click measure is the exception: it reads the pointer through the crosshair, so the crosshair stays visible even over other objects. The engine retains the crosshair position for snapping and host callbacks, while each host continues to show the object's pointer, click, grab, or drag cursor.

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

The shared text atlas treats one render as a transaction: slots referenced or inserted in the current frame cannot be recycled until that frame completes. Atlas pressure after the frame has accepted text defers the reset to the next frame; the browser renders the pressured frame through Canvas2D rather than submit stale UVs. A reset increments the atlas epoch, invalidating retained textured groups and text-cache entries before the next WebGPU submission. A visible text run that cannot enter the atlas at all (larger than the atlas, or unmeasurable) marks only the retained group that holds it. While that group's source revision is unchanged, later frames go straight to Canvas2D without WebGPU tessellation; every other retained group keeps its geometry, so the first WebGPU frame after the run changes or disappears rebuilds only what changed.

Browser WebGPU shares one page-wide adapter/device/queue and atlas while retaining per-chart surfaces. Device loss is therefore a shared generation event, not ownership of the chart that first created the device: every live chart listener wakes and falls back, while disposed charts have no listener. Headless chart data is preserved through fallback.

The browser package's default `auto` backend prefers WebGPU but keeps Canvas2D available when the browser exposes no usable adapter; `navigator.gpu` alone is not proof of adapter availability. A failed adapter request is cached for that page session so independently mounted charts and viewport remounts do not repeatedly probe an unavailable adapter. Device-initialization failures remain retryable, explicit fallback-adapter diagnostics are isolated from the ordinary adapter result, and a reload permits a new adapter probe after browser or driver settings change. The General dashboard reports the actual backend and fallback reason rather than rejecting charts when WebGPU is unavailable.

Chart construction accepts an explicit first-pane horizontal domain. The engine creates either the
compatible financial pane plus primary candlestick series or one preserved general pane with no
financial series; browser hosts do not add a temporary financial pane and remove it afterward.
Rejected construction removes the canvases installed by that attempt before control returns to the
caller. The engine retains one layout slot at all times. Explicit removal of an empty preserved final
pane retires its stable and persistence identities, releases its general-domain/axis state, and installs
a fresh unpreserved financial-time pane in the same slot. The removed handle therefore stales normally,
and declarative cleanup never needs a temporary keeper pane.

## Evidence benchmark subsystem

`benchmarks/` is development and release evidence infrastructure outside every production crate and the published package. Its single Node entry point builds the actual release package, drives the public browser API through the existing Playwright demo host, generates deterministic versioned OHLCV data, validates versioned JSON results, compares explicit baselines, applies centralized budgets, and emits human- and website-readable artifacts. The browser page is served by `examples/web_demo/test_server.mjs` only for automation; it is not part of the npm package.

The subsystem reuses `chart_api.frame_stats()` for bounded CPU, real capability-detected WebGPU timestamp, draw, presentation, dropped-frame, ring-overrun, and WASM-linear-memory observations. It does not add production instrumentation, dependencies, imports, feature flags, logging, or runtime branches. Browser page/heap memory is labeled as whole-page memory, and unsupported presentation or GPU measurements remain unsupported rather than inferred.

Raw local results are ignored and CI results are artifacts. Public summaries and committed release baselines require a clean `release` profile result classified as `official-benchmark-runner`; shared CI timings are smoke/trend evidence only. Scenario, dataset-generator, schema, and baseline versions preserve historical comparability.

Benchmark comparisons enforce only explicitly configured budgets. An empty policy is reported as `NO ENFORCED BUDGET`, and configured keys must match comparable metrics so a typo cannot silently disable a hard threshold. Publication requires the portable browser runtime/parity suite, including deterministic SwiftShader WebGPU/Canvas2D and tiny-skia/Canvas2D pixel comparisons. Reference fidelity reports, GPU measurements, and wall-clock evidence remain separate non-blocking results.

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

Lower layers never import a host API to bypass their boundary. The headless path is `aeris_charts_core` and `aeris_charts_indicators` into `aeris_charts_engine`, then `aeris_charts_render`; GPUI, WebGPU, native, and WASM/browser code sit at execution boundaries. Avoid new crates, traits, and feature flags unless they enforce a real current dependency or platform boundary.

## Repository documentation

Markdown documentation may live at the root or beside the component it explains when it has a durable repository purpose. Keep the root README focused on product orientation and contributor setup, and keep architectural ownership and data flow in this file. Do not commit transient work notes, generated reports, or duplicate documentation.

## Verification

The standard gates mirror CI:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy -p aeris_charts_wasm --target wasm32-unknown-unknown --locked -- -D warnings
cargo test --workspace --locked
cargo run -p aeris_charts_native --example perf_gate --release

cd packages/charts
npm ci
npm run lint
npm run build
npm run typecheck
npm run test:pack
```

The release performance gate also measures a 100,000-visible-bar volume-profile refresh through frame construction and verifies that unchanged frames retain the calculation revision.

Run Playwright for browser behavior, rendering, interaction, packaging, or parity changes. Run GPUI parity and replay checks for GPUI executor changes. The Windows native GPUI CI job runs the official-window pixel-parity harness against tiny-skia and fails on a missing capture or any crisp-rect mismatch; the same harness enforces the crosshair icon image against native rendering with at most one channel value of blending-rounding difference. Linux CI selects GPUI's X11 compositor and Mesa's software EGL path under Xvfb, then captures the real 1× window pixels for crisp rectangles and filled meshes. Its filled-scene gate bounds the measured edge residual against tiny-skia while mesh tests verify a coverage ramp on AreaFill, BandFill, Triangle, and RoundRect. Changes to the icon source or masks also run `node examples/web_demo/build_crosshair_icon.mjs --check`.

The evidence harness has one entry point:

```text
node benchmarks/benchmark.mjs test
node benchmarks/benchmark.mjs smoke
node benchmarks/benchmark.mjs release
```

Tag publication requires the Rust, package, and portable Chromium/Firefox/WebKit jobs. Public
declaration and release-policy guards, V1 fixtures, Node import, and pack smoke are portable blocking
checks. Deterministic browser/backend pixel comparisons and the native/browser fixture are blocking
portable Chromium checks. Configured `perf_gate` budgets run strictly. Reference-fidelity screenshots, GPU timings,
heap sampling, and wall-clock evidence stay in separate non-blocking diagnostic steps; approved hashes
are never changed merely to satisfy a different host.

Indicator multi-input validation is engine-owned: VWAP and VWAP-band bindings require a distinct
live scalar volume series, while missing volume remains the explicit unit-weight fallback. Financial
indicator bindings are also the visibility, removal, and chrome ownership unit. One engine operation
shows, hides, or removes every output in a binding, and the retained chart-wide indicator chrome
policy applies name labels, value labels, and price lines to current and later outputs. Hosts choose
that policy and render controls; they do not walk output series or predict output counts.
OHLCV resampling is an engine-owned dependency from a host-fed candlestick/bar series to a
host-created derived series. The host supplies disjoint UTC boundaries and an interval in seconds;
the engine restarts buckets at each boundary, excludes rows outside them, and refreshes the derived
bars and their indicator dependents after source corrections. Browser hosts configure the same
binding through WASM and may read its current aggregate rows. A separate histogram can supply
volume and receive derived volume, but the engine rejects a binding that would overwrite its own
volume input. Higher-timeframe candles and studies use ordinary series and indicator frame paths.
The engine gives each newly created dedicated indicator pane the same 0.3 stretch, including
financial oscillators, external studies, CVD, and delta. A study placed into an explicitly selected
existing pane keeps that pane's user-selected height; every backend renders the shared layout.
Host-computed scalar studies cross the same boundary through the external-study transaction. The
host supplies a stable study/output identity, generation, semantic presentation, stream requirement
metadata, timestamps, and nullable values. The engine validates the complete publication before
mutation and owns its bounded output registry, generation fence, series and dedicated-pane lifecycle,
price-format inheritance, retained chrome, group visibility, and group removal. Provider sessions and
the computation of those values remain host-runtime responsibilities.
Order-flow presentation is likewise installed and removed as one engine transaction. The engine
owns the shared trade-stream graph, footprint/CVD/delta series and panes, candle-to-footprint
cutover, retained indicator chrome, an optional big-trades indicator on the primary series, and
automatic 1-2-5 row-size policy. A product host supplies instrument/provider generation fencing, canonical bounded trades,
bar aggregation intent, current price metadata, and presentation preferences; it does not assemble
or tear down the dependent chart graph itself.
Financial
study persistence V3 stores binding definitions, dependency references, scalar inputs, volume inputs,
and output styles while leaving market history and ordinary series data host-owned. Trade, quote, and
depth study inputs remain owned by the host market runtime: it supplies typed stream requirements and
generation-fenced borrowed views, while Charts receives only bounded scalar study output publications.
Charts must not retain a second tape/book or infer provider stream state from a rendered series. The
Terminal bridge now carries the transitive stream requirements as bounded output metadata and persists
only the validated host binding; this keeps the runtime-to-chart boundary explicit while allowing
downstream output presentation to retain its typed input contract.
