# General chart API proposal

## Status and purpose

This document began as the Phase 0 API proposal for the all-in-one architecture in `plan.md` and
remains the contract for unfinished chart families. The current package implements the Phase 2 Cartesian
families: category columns and horizontal bars, category-band box plots, category/category plus
numeric/numeric and temporal/numeric heatmap grids, numeric XY scatter/bubble marks, numeric/temporal/category error bars,
and `xy_line`, `xy_area`, `range_area`, grouped/stacked bar, and stacked-area slices. Domain-aware panes, explicit axes, object and
typed bulk replacement/update, bounded retention, row labels, snapshots, hit testing, accessibility,
and V2 persistence are public for these implemented kinds/options. `xy_line`, `xy_area`, and `range_area` support continuous numeric,
temporal epoch-millisecond, and category band/point X domains. `error_bar` supports numeric and temporal X
axes with optional independent bounds on either axis, plus category band/point X with optional Y bounds only.
`box_plot` uses category-band X with numeric Y and requires a complete ordered
`min <= q1 <= median <= q3 <= max` row for visible geometry. All forms require their center/value channels
for a visible mark. `heatmap_grid` supports band X/Y string categories, continuous numeric X with numeric Y
coordinates, and temporal epoch-millisecond X with numeric Y coordinates. Later polar series names and the React surface
below remain proposals until their implementations and release evidence land.

General-series legend metadata is engine-owned. `chart.general_legend_snapshot(pane?)` returns bounded
series metadata in stable engine order, including pane, kind, title, color, and visibility. Hidden series remain
present with `visible: false` so a host-rendered legend can expose them without rebuilding state independently;
pane filtering is read-only and V2 restore reconstructs the same semantic entries.

Cross-series tooltip grouping is also engine-owned. `chart.general_shared_tooltip(series, row)` returns the
visible rows in the same pane whose exact horizontal datum matches the anchor. Ordering is stable series order
then row order; duplicate X values are preserved, hidden series are excluded, and a heatmap may contribute more
than one cell for one X coordinate/category.

General range selection uses `chart.set_general_brush(axis, from_coordinate, to_coordinate)`. The browser sends
CSS-pixel endpoints once; Rust converts them immediately into a numeric, temporal, or category range and owns
the transient selection. `general_brush_snapshot()` returns that semantic range plus a bounded list of visible
row identities on the selected axis. Zoom/resize therefore reprojects the stored range instead of retaining stale
pixels. `clear_general_brush()` removes it.

Reference components are first-class engine state. `chart.add_general_reference(...)` creates a line, dot, or
rectangular region bound to explicit general axes. Each reference declares `extend_domain`; automatic domains
include its coordinates only when that flag is true. References lower into the same backend-neutral frame,
participate in pane/axis lifecycle protection, and round-trip in V2 persistence.

The proposal is additive. Existing financial series, data shapes, pane methods, price-scale
handles, snake-case methods, and persistence V1 keep their current meaning. In particular,
`"line"`, `"area"`, `"bar"`, `"histogram"`, `"baseline"`, `"candlestick"`, and `"footprint"`
remain financial-time series. Nucleus must never guess whether a row belongs to the financial or
general data domain.

The imperative API remains canonical. Phase 3 adds camel-case aliases plus a React adapter that call
the same mutations rather than defining another chart model; snake-case methods remain supported.

## Domain vocabulary

The first implementation binds one horizontal domain to each pane. A future plot-region API may
allow multiple regions inside a pane, but it must preserve these same rules and may not create a
second engine or frame.

```ts
type horizontal_domain_options =
  | { type: "financial_time" }
  | { type: "continuous"; scale?: "linear" | "log" | "symlog" }
  | { type: "temporal" }
  | { type: "category"; scale?: "band" | "point" }
  | { type: "polar" };

interface general_pane_options {
  preserve_empty?: boolean;
  horizontal_domain: horizontal_domain_options;
}
```

`financial_time` remains the default for the initial pane and for every existing call to
`chart.add_pane()` or `chart.add_pane(preserve_empty)`. The additive overload is:

```ts
chart.add_pane(options: general_pane_options): pane_api;
```

The engine always retains one layout slot. Removing an empty preserved final pane retires that
pane's stable identity and installs a fresh, unpreserved financial-time default in the same slot;
the removed handle becomes stale. A populated or unpreserved final pane is rejected. This lets
declarative owners dispose their pane without manufacturing a temporary keeper pane.

A pane's horizontal-domain type is immutable while the pane contains a series, axis, selection,
or persisted general dataset. This avoids silently reinterpreting stored coordinates. An empty
pane may be rebound explicitly in a later API, but remove-and-recreate is sufficient for the first
release.

Numeric continuous values are finite IEEE-754 numbers. Temporal values are `Date` objects or
finite whole epoch-millisecond numbers; they are normalized once to an engine-owned integer
column. ISO strings and unit inference are not accepted because their parsing and units are not
deterministic. Existing financial numeric times remain whole UTC seconds and are unchanged.

Category labels are UTF-8 strings interned by the engine in first-seen order unless an explicit
domain is supplied. Empty strings are valid labels. Category identity is the exact string; display
formatters do not change identity or order.

## Axis vocabulary

General axes are explicit engine objects. IDs are chart-local, case-sensitive UTF-8 strings of
1–128 bytes. The built-in financial price/time handles remain available and retain their current
specialized implementations.

```ts
type axis_dimension = "x" | "y" | "angle" | "radius";
type axis_position = "top" | "bottom" | "left" | "right";
type general_scale_type =
  | "linear"
  | "log"
  | "symlog"
  | "temporal"
  | "band"
  | "point"
  | "radial_linear"
  | "angular_category";

type numeric_domain = "auto" | readonly [number, number];
type temporal_domain = "auto" | readonly [Date | number, Date | number];
type category_domain = "auto" | readonly string[];
type general_axis_tick =
  | { type: "numeric"; value: number; label?: string }
  | { type: "temporal"; value: Date | number; label?: string }
  | { type: "category"; value: string; label?: string };

interface general_axis_options {
  id: string;
  pane: number;
  dimension: axis_dimension;
  position?: axis_position;
  scale: general_scale_type;
  domain?: numeric_domain | temporal_domain | category_domain;
  reverse?: boolean;
  visible?: boolean;
  title?: string;
  tick_count?: number;
  ticks?: readonly general_axis_tick[];
  min_tick_gap?: number;
  band_padding_inner?: number;
  band_padding_outer?: number;
  zero_line?: boolean;
  grid_visible?: boolean;
}
```

Validation is structural and atomic:

- X axes must match the pane's horizontal domain.
- Y axes may be linear, log, or symlog; polar radius axes use radial linear.
- A log axis rejects non-positive explicit bounds and ignores non-positive data for automatic
  domain contribution while retaining those rows as missing geometry.
- Band padding is finite and clamped only when the documented range permits it; invalid values do
  not partially mutate the axis.
- Automatic domains combine only visible series bound to that axis. Reference components declare
  explicitly whether they extend the domain.
- On executable Cartesian axes, `ticks` replaces automatic tick selection with at most 512 typed
  values. Values must match the axis scale and be unique; numeric values are finite, logarithmic values are positive, and temporal
  values are JavaScript-safe epoch milliseconds. A supplied label is retained as portable engine
  state and reaches every backend; an omitted label uses the built-in numeric, UTC temporal, or
  category formatter. Explicit ticks outside the effective domain are clipped. `tick_count` and
  `ticks` are mutually exclusive so accepted options never carry two competing selection policies.
  Polar explicit ticks are rejected until polar tick execution is implemented.
- Tick placement, collision removal, grid contribution, titles, and label anchors are engine-owned.
  A host may preformat an explicit tick label, but it cannot change tick coordinates.
- `grid_visible` projects that axis's tick coordinates into the clipped pane underlay when the
  matching chart-wide grid direction is visible. `zero_line` independently draws a solid rule when
  zero lies inside a numeric domain. Coincident rules are emitted once, with the zero rule taking
  precedence, so multiple axes do not darken shared coordinates.
- Temporal axes select a bounded UTC interval from milliseconds through calendar years. Intraday,
  day, month, and year labels use injected locale month names and the same shared `AxisFrame` as
  numeric and category axes; hosts do not run a parallel date-axis layout.

The first implementation should expose `chart.add_axis(options)`, `chart.axis(id)`,
`chart.axes(pane?)`, and `chart.remove_axis(id)`. Removing a populated axis is rejected. A series
rebind is one atomic operation and fails without mutation when either target axis is absent or
incompatible.

## General series names

General series use names that cannot collide with the existing financial meanings:

```ts
type general_series_kind =
  | "xy_line"
  | "xy_area"
  | "range_area"
  | "column"
  | "horizontal_bar"
  | "scatter"
  | "bubble"
  | "box_plot"
  | "heatmap_grid"
  | "error_bar"
  | "pie"
  | "donut"
  | "radar"
  | "radial_bar"
  | "polar_area";
```

Grouped and stacked charts are options on compatible column, horizontal-bar, and area series, not
separate engines or renderer-specific kinds. A stack ID joins series only when their pane, axes,
orientation, and category/continuous coordinate semantics match.

The currently implemented Phase 2 slice exposes `group_id` on `column` and `horizontal_bar`, and
`stack_id`/`stack_mode` on both bar orientations and `xy_area`. Vertical columns bind a category X axis
to a numeric Y axis; horizontal bars reuse the same category/value rows with a numeric X axis and band Y axis.
Grouped bars subdivide the category band; matching stacked bars occupy one group slot. Stacked areas align members by exact X identity across
numeric, temporal, and category domains and fill between the preceding cumulative boundary and the new
cumulative boundary. `stack_mode: "normal"` uses independent positive/negative accumulation around zero,
while `stack_mode: "percent"` normalizes positive and negative totals independently to `+1`/`-1`.
Both bar orientations reuse ordered `Rect` primitives, exact rectangle hits, bounded labels, snapshots,
typed/object updates, and V2 persistence.

```ts
interface cartesian_series_options {
  pane?: number;
  x_axis_id: string;
  y_axis_id: string;
  visible?: boolean;
  title?: string;
  color?: string;
  /** Draw markers at line, area, or range-area data points; defaults to false. */
  point_markers?: boolean;
  /** Marker shape for scatter and path markers; defaults to circle. */
  point_symbol?: "circle" | "square" | "diamond" | "triangle";
  /** Stroke width for line, area, and range-area paths in CSS pixels; defaults to 2. */
  line_width?: number;
  /** Portable stroke pattern for line, area, and range-area paths; defaults to solid. */
  line_style?: "solid" | "dotted" | "dashed";
  /** Shared path interpolation for line, area, and range-area boundaries; defaults to linear. */
  interpolation?: "linear" | "step" | "curved";
  /** Bridge missing rows in path series; transform-invalid rows remain gaps. Defaults to false. */
  connect_missing?: boolean;
  /** Explicit numeric fill baseline for `xy_area`; omitted uses zero when visible, otherwise the edge. */
  baseline_value?: number;
  group_id?: string;
  stack_id?: string;
  stack_mode?: "normal" | "percent";
  missing?: "gap" | "zero";
}

interface polar_series_options {
  pane?: number;
  angle_axis_id: string;
  radius_axis_id: string;
  visible?: boolean;
  title?: string;
  color?: string;
  stack_id?: string;
}
```

`missing: "zero"` is allowed only for series where zero is a meaningful baseline. Line, scatter,
bubble, box-plot, error-bar, and range data always treat a missing value as absent geometry.

## Object data shapes

Object rows are validated and converted to typed engine columns once at the package boundary.
`null` marks a missing channel; `NaN` and infinities are invalid input rather than alternate missing
sentinels.

```ts
type general_row_id = string | number;
type general_x = number | Date | string;

interface xy_row {
  id?: general_row_id;
  x: general_x;
  y: number | null;
  color?: string;
  label?: string;
}

interface range_row {
  id?: general_row_id;
  x: general_x;
  low: number | null;
  high: number | null;
  color?: string;
  label?: string;
}

interface bubble_row extends xy_row {
  size: number | null;
}

interface heatmap_row {
  id?: general_row_id;
  x: string | number | Date;
  y: string | number;
  value: number | null;
  color?: string;
  label?: string;
}

interface error_bar_row extends xy_row {
  x: number | string | Date; // Number/Date for numeric or temporal X; string for band/point X.
  x_low?: number | Date | null;
  x_high?: number | Date | null;
  y_low?: number | null;
  y_high?: number | null;
}

// The numeric typed form has parallel x/y and x_low/x_high/y_low/y_high Float64Array columns.
// The temporal typed form uses x_epoch_ms plus x_low_epoch_ms/x_high_epoch_ms Float64Array columns;
// every present temporal X value and bound must be a whole JavaScript-safe epoch-millisecond integer.
// The category typed form has categories/category_indices, y, y_low, and y_high columns.
// Each bound has an optional Uint8Array validity mask (0 = absent), independent of y_valid.
// Present numeric/temporal X bounds must not cross X; with valid center Y, present Y bounds must not
// cross Y. Category rows reject X bounds. Absent bounds remain queryable.

interface box_plot_row {
  id?: general_row_id;
  x: general_x;
  min: number | null;
  q1: number | null;
  median: number | null;
  q3: number | null;
  max: number | null;
  color?: string;
  label?: string;
}

interface polar_value_row {
  id?: general_row_id;
  category: string;
  value: number | null;
  color?: string;
  label?: string;
}
```

Rows with required missing channels stay in the dataset and category/domain union but emit no mark.
Range rows require `low <= high`. Box rows require `min <= q1 <= median <= q3 <= max`. Negative
bubble sizes and negative pie/donut values are invalid. Zero-size bubbles and zero-value sectors are
retained for queries and accessibility but emit no visible area.

If `id` is omitted, `set_data()` assigns an internal identity scoped to that installed batch.
Identity-sensitive transitions and update/remove-by-ID operations require explicit unique IDs.
Duplicate explicit IDs reject the complete transaction. The engine never uses a formatted label or
floating-point hash as hidden identity.

## Typed bulk ingestion

The bulk API mirrors the object shapes with one column per channel. It does not accept row objects,
invoke a callback per point, or cross the WASM boundary per point.

```ts
interface numeric_xy_columns {
  ids?: readonly general_row_id[];
  x: Float64Array;
  y: Float64Array;
  y_valid?: Uint8Array;
  colors?: Uint32Array;
}

interface category_xy_columns {
  ids?: readonly general_row_id[];
  categories: readonly string[];
  category_indices: Uint32Array;
  y: Float64Array;
  y_valid?: Uint8Array;
  colors?: Uint32Array;
}

interface temporal_xy_columns extends Omit<numeric_xy_columns, "x"> {
  /** Whole epoch milliseconds, exactly representable as JavaScript numbers. */
  x_epoch_ms: Float64Array;
}

interface category_heatmap_columns {
  ids?: readonly general_row_id[];
  x_categories: readonly string[];
  x_category_indices: Uint32Array;
  y_categories: readonly string[];
  y_category_indices: Uint32Array;
  value: Float64Array;
  value_valid?: Uint8Array;
}

interface numeric_heatmap_columns {
  ids?: readonly general_row_id[];
  x: Float64Array;
  y_coordinate: Float64Array;
  value: Float64Array;
  value_valid?: Uint8Array;
}

interface temporal_heatmap_columns {
  ids?: readonly general_row_id[];
  /** Whole epoch milliseconds, exactly representable as JavaScript numbers. */
  x_epoch_ms: Float64Array;
  y_coordinate: Float64Array;
  value: Float64Array;
  value_valid?: Uint8Array;
}
```

All parallel arrays must have equal row counts. Validity arrays contain only `0` or `1`. Category
indices must be in range. The transaction is validated before the live dataset changes. Bubble extends
numeric XY columns with `size: Float64Array` and optional `size_valid: Uint8Array`. Box plots use category
indices plus five parallel `Float64Array` channels named `min`, `q1`, `median`, `q3`, and `max`, each
with an optional validity mask. Category heatmaps use independent bounded X/Y category dictionaries and aligned
index columns plus one numeric `value` channel; both dictionaries merge and compact atomically during explicit-ID
updates and bounded retention rather than introducing a second data store. Continuous and temporal heatmaps reuse
the ordinary numeric/temporal X column, add one aligned numeric `y_coordinate` column, and keep `value` in the
existing value/validity channel. Cell extents are inferred deterministically from neighboring coordinate centers.

The first release needs `set_data()`, `set_data_typed()`, append/update by explicit row ID, bounded
retention, `data_at()`, hit-test/tooltip snapshots, and capacity telemetry. General storage is
allocated lazily when the first general series or dataset is created; a financial-only chart must
retain zero general-dataset, domain, axis, and geometry capacity.

The current column/horizontal-bar/box-plot/heatmap-grid/scatter/bubble/`xy_line`/`xy_area`/`range_area`/`error_bar` browser slices expose `update_data(rows, { max_rows })` and
`update_data_typed(columns, { max_rows })`. Every updated row needs an explicit string or numeric
`id`; matching IDs replace in place, while new IDs append in input order. `max_rows` is an optional
per-transaction retention limit: after the update, oldest rows are removed until the dataset fits.
Pass it on every streaming update that needs retention. Invalid batches leave the prior dataset
unchanged. Rows removed by retention lose their identity and any hover/selection target; retained
explicit IDs continue to identify the same marks after front trimming.

The current column/scatter/bubble/`xy_line`/`xy_area`/`range_area`/`error_bar` options also accept `data_labels: true` to draw visible numeric Y values
near their marks. The engine places labels in the shared frame, rejects overlapping or out-of-plot
placements, and caps output at 512 labels and 4,096 placement attempts per pane per frame. Missing
rows never produce labels. Object rows may supply `label?: string`; typed columns may supply a
parallel `labels?: readonly (string | null)[]`. Custom text replaces the displayed numeric value
for that row, while an empty string intentionally hides its visible label. An omitted/null label
falls back to the numeric value. Replacing or updating a row without a custom label clears its old
text; untouched IDs keep theirs. Label validation, update, and retention are atomic with the row
transaction. A label is limited to 4,096 UTF-8 bytes, and each dataset to 65,536 custom labels and
1,048,576 label bytes. Tooltip and bounded accessibility snapshots expose the custom text as
`label: string | null` alongside the raw value.

The current legend surface is metadata-only by design: the browser may render DOM legend controls, but the
entry set, order, visibility, title, color, kind, and pane identity come from the engine snapshot. Axis and
series handles expose `set_visible`/`setVisible`; legend controls use the series handle and then read the next
engine snapshot rather than maintaining parallel visibility state.

Axis and series handles also expose `apply_options`/`applyOptions`. Axis updates cover domain, direction,
position, visibility, title, tick spacing/count, band padding, zero line, and grid policy; changing a configured
domain resets only that axis's runtime view. Series updates cover visibility, title, color, point radius, data
labels, grouping, stack identity, stack mode, and compatible pane/axis rebinding. A rebind requires the target
pane to have the same horizontal-domain semantics and the target X/Y axes to retain the current scale types.
Each update validates a complete candidate before commit, preserves handles and data, and leaves prior state
intact on rejection. Kind and dataset changes remain structural.

Numeric, temporal, and category axis handles expose `pan(fraction)`, `zoom(factor, anchor_value)`, and
`reset_view()`/`resetView()`. A temporal zoom anchor is a whole JavaScript-safe epoch millisecond;
a category anchor is its string identity in the current visible window. Category pan shifts by a
rounded fraction of the visible category count and clamps at the base-domain ends. Category views
retain a bounded index window rather than copied labels, so automatic-domain changes cannot leave
stale category identities in the viewport. Runtime views reject invalid anchors or collapsing
transforms atomically and leave the configured or automatic base domain available for reset.

`chart.general_series_order(pane?)` returns live handles in the engine's stable bottom-first order.
`chart.set_general_series_order(handles, pane?)` atomically accepts only an exact permutation of every live
general series in that scope. Pane-local reordering leaves every other pane's relative order unchanged. The
same order drives painting, legend and hit-test traversal, React keyed array order, and V2 persistence.

Reference components are intentionally separate from series data. A reference line binds one X or Y axis and one
compatible numeric/temporal/category value; a dot binds explicit X and Y axes; a region binds two endpoints on
each axis. Styling is bounded engine-owned state. `extend_domain: false` is the default semantic: the reference is
drawn only where its value maps into the current domain. `extend_domain: true` contributes each declared reference
coordinate to automatic domain resolution. Removing an axis used by a live reference is rejected until the
reference is removed.

Brush state is transient and is not serialized in V2. It is interaction state like hover/selection rather than
chart configuration. The snapshot contains at most the engine's bounded brush item limit and preserves
series/row ordering. Shared tooltips are derived snapshots and likewise add no retained host-side registry.

## Pane compatibility matrix

`same region` below means the series can contribute to one coordinate region and ordered frame.
Different Y axes are allowed within that region when their scale types accept the series values.

| Existing or proposed family | Financial time | Continuous X | Temporal X | Category X | Polar |
| --- | --- | --- | --- | --- | --- |
| Candlestick, financial bar, histogram, baseline, footprint, indicators | same region | incompatible | incompatible | incompatible | incompatible |
| `xy_line`, `xy_area`, `range_area` | incompatible | same region | same region | same region with point/band axis | incompatible |
| `column` | incompatible | same region | same region | same region | incompatible |
| `horizontal_bar` (numeric X, category Y) | incompatible | same region | incompatible | incompatible | incompatible |
| `scatter`, `bubble`, `error_bar` | incompatible | same region | same region | same region with point axis | incompatible |
| `heatmap_grid`, `box_plot` | incompatible | same region when both axes agree | same region when axes agree | same region | incompatible |
| Pie, donut, radar, radial bar, polar area | incompatible | incompatible | incompatible | incompatible | same polar region |

Additional rules:

- Financial time and temporal X never share a region. Financial time is logical-index spacing over
  a timestamp union; temporal X is elapsed-time spacing. Equal-looking timestamps do not make the
  semantics compatible.
- Continuous linear, log, and symlog series may share a pane only through explicit compatible X
  axes. Multiple X axes are allowed, but every series still uses the pane's continuous domain kind.
- Band and point axes may share the same ordered category registry. Band geometry uses the registry
  interval; point geometry uses its center. Explicit category orders must be identical or use
  distinct panes/regions.
- Vertical and horizontal bars may share a pane only when their X/Y role reversal resolves to the
  same two axis registries. A category-X vertical column and category-Y horizontal bar are otherwise
  incompatible.
- Stacked series require the same stack ID, axis IDs, category keys, orientation, and baseline.
  Grouped series require the same category registry but may use separate compatible Y axes.
- Polar series share only when their angular category order and radial-domain semantics agree.
- Incompatible additions fail with `invalid_options`; Nucleus never moves the series implicitly or
  creates a hidden chart engine.

## Shared behavior required of every general series

Adding a public series kind is incomplete until the engine owns all of these behaviors:

- input validation and deterministic missing-value semantics;
- automatic-domain contribution and explicit-domain clipping;
- backend-neutral geometry in the ordered `ChartFrame`;
- exact and nearest hit testing with stable tie-breaking;
- tooltip/value snapshots and formatted axis values;
- keyboard focus and accessibility snapshot values;
- selection and hover state;
- lifecycle removal and bounded cache invalidation;
- persistence under the future general-chart schema;
- Canvas2D, WebGPU, GPUI, and native parity evidence;
- financial-only, general-only, and combined performance evidence.

Host callbacks may format labels or present HTML tooltips, but built-in geometry, axis placement,
domain calculation, hit testing, tooltip values, and accessibility values remain Rust-owned.

## Initial vertical-slice acceptance

The category-column slice is ready only when it proves band padding, category ordering, tick
collision, positive/negative baselines, missing rows, labels, tooltip/hit testing, accessibility,
and all executor paths. The XY-scatter slice is ready only when it proves independent continuous X
and Y domains, log/symlog validation where enabled, point-size bounds, dense hit testing, pan/zoom,
missing rows, and all executor paths.

The first `xy_line` slice is accepted only when continuous numeric, temporal, and category X domains
remain explicit; missing rows split stroke runs; bound datasets cannot change X kind; segment hits,
row identity, labels, accessibility, keyboard navigation, incremental updates, V2 persistence, and
Canvas2D/WebGPU/GPUI execution all use the same engine-owned semantics.

The first `xy_area` slice uses that same path and identity contract, fills each contiguous run to an
engine-owned baseline, never bridges missing or transform-invalid rows, and includes the filled region in
hit testing. The shared frame emits `AreaFill` followed by the matching stroke, with direct Canvas2D,
WebGPU, and GPUI executor coverage.

Both slices must use the same chart lifecycle, pane handles, theme, event subscriptions,
screenshot path, and ordered frame as financial series. A financial-only chart must show no
material change in output, steady-state work, retained memory, startup, or package size against the
Phase 0 evidence.
