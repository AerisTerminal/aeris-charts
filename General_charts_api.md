# General chart API proposal

## Status and purpose

This document began as the Phase 0 API proposal for the all-in-one architecture in `plan.md` and
remains the contract for unfinished chart families. The current package now implements the first
category-column and numeric XY-scatter subset: domain-aware panes, explicit axes, object and typed
bulk replacement, snapshots, hit testing, and lifecycle removal are public in
`packages/charts/src/types.ts` and tracked by the public API manifest. Later series names and the
incremental, selection, label, persistence, and React surfaces below remain proposals until their
implementations and release evidence land.

The proposal is additive. Existing financial series, data shapes, pane methods, price-scale
handles, snake-case methods, and persistence V1 keep their current meaning. In particular,
`"line"`, `"area"`, `"bar"`, `"histogram"`, `"baseline"`, `"candlestick"`, and `"footprint"`
remain financial-time series. Nucleus must never guess whether a row belongs to the financial or
general data domain.

The imperative API remains canonical. Camel-case aliases and the React adapter come later and
must call the same mutations rather than define another chart model.

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
- Tick placement, collision removal, grid contribution, titles, and label anchors are engine-owned.
  A host formatter may supply text, but it cannot change tick coordinates.

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

```ts
interface cartesian_series_options {
  pane?: number;
  x_axis_id: string;
  y_axis_id: string;
  visible?: boolean;
  title?: string;
  color?: string;
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
  x: general_x;
  y: number | string;
  value: number | null;
  color?: string;
  label?: string;
}

interface error_bar_row extends xy_row {
  x_low?: number | null;
  x_high?: number | null;
  y_low?: number | null;
  y_high?: number | null;
}

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
```

All parallel arrays must have equal row counts. Validity arrays contain only `0` or `1`. Category
indices must be in range. The transaction is validated before the live dataset changes. Future
range, bubble, error, heatmap, and box column types extend this same convention rather than adding a
generic dynamically typed channel map to the frame hot path.

The first release needs `set_data()`, `set_data_typed()`, append/update by explicit row ID, bounded
retention, `data_at()`, hit-test/tooltip snapshots, and capacity telemetry. General storage is
allocated lazily when the first general series or dataset is created; a financial-only chart must
retain zero general-dataset, domain, axis, and geometry capacity.

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

Both slices must use the same chart lifecycle, pane handles, theme, event subscriptions,
screenshot path, and ordered frame as financial series. A financial-only chart must show no
material change in output, steady-state work, retained memory, startup, or package size against the
Phase 0 evidence.
