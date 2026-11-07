# General chart authoring

The root package entry is framework neutral. The optional `/react` entry owns chart and component
lifecycle over the same engine handles. Both use the shared frame, input controller, persistence,
and renderers described in [architecture.md](architecture.md).

## React composition

`GeneralPane` accepts the existing `axes` and `series` arrays, or direct JSX declarations inside
the pane. Fragments and conditional children are supported. `GeneralAxis`, `GeneralSeries`,
`MappedGeneralSeries`, `GeneralDataLabels`, and `GeneralReference` are declarations; they do not
draw React elements. `GeneralLegend`, `GeneralTooltip`, and `GeneralBrushControls` mount the
package's accessible default DOM controls. Every general pane already has an engine-drawn hover
tooltip; `GeneralTooltip` replaces it with a DOM tooltip of the same style while mounted. Arbitrary wrapper components cannot stand in for a
declaration; render their returned declarations as direct pane children.

```tsx
import {
  AerisChart, GeneralAxis, GeneralBrushControls, GeneralDataLabels,
  GeneralLegend, GeneralPane, GeneralReference, GeneralTooltip,
  MappedGeneralSeries,
} from "@aeristerminal/aeris-charts/react";

type Sale = { id: string; month: string; amount: number };
const mapSale = (row: Sale) => ({
  id: row.id, x: row.month, y: row.amount, label: String(row.amount),
});

export function SalesChart({ sales }: { sales: readonly Sale[] }) {
  return <AerisChart options={{ autoSize: true, initialPane: { horizontal_domain: { type: "category", scale: "band" } } }}
    style={{ width: "100%", height: 400 }}>
    <GeneralPane options={{ horizontal_domain: { type: "category", scale: "band" } }} useInitialPane>
      <GeneralAxis id="month" dimension="x" scale="band" position="bottom" />
      <GeneralAxis id="amount" dimension="y" scale="linear" position="left" />
      <MappedGeneralSeries seriesKey="sales" kind="column"
        options={{ x_axis_id: "month", y_axis_id: "amount", title: "Sales" }}
        data={sales} mapRow={mapSale} />
      <GeneralDataLabels seriesKey="sales" />
      <GeneralReference referenceKey="goal"
        options={{ kind: "line", axis_id: "amount", value: 50 }} />
      <GeneralLegend /><GeneralTooltip />
      <GeneralBrushControls options={{ axis: "month" }} />
    </GeneralPane>
  </AerisChart>;
}
```

Use `seriesKey` and `referenceKey` for engine identity. React's reserved `key` is unavailable to
these declarations. Stable keys retain handles across data, presentation, visibility, and
reference option changes. A changed series kind or axis dimension/scale is structural and
replaces its dependent object. `mapRow` runs when the source array or mapper function identity
changes, not in frame or hit loops; keep mapper functions stable when their meaning is stable.
Typed columns remain the imperative bulk-ingestion path for high-rate updates.
Both React and framework-neutral callers use `compose_general_pane(...)` to apply a complete pane
graph in one engine transaction. The operation accepts ordered axes, keyed series with data and
visibility, and keyed references. Pass each prior result handle as `current` on the next call to
retain its identity. The engine validates the complete final graph before applying dependency-ordered
removals and upserts; any failure retains the prior graph and handle counters.
`validate_general_pane_proposal(...)` remains available for an optional non-mutating dry run, but
React does not need a separate validation call. Pass `onValidationError`
to report a mapper failure or rejected composition while retaining the last
committed pane; without it, the error reaches the nearest React error boundary.
`onSeriesReady` and `onReferenceReady` run after the composition commits. If one throws, the pane
keeps the complete new graph, every other new handle still receives its callback, and the failure
is reported once (several failures arrive together as an `AggregateError`).

For a general-only chart, supply the same domain in `AerisChart`'s `initialPane` and the first
`GeneralPane`'s `options`, then set `useInitialPane`. That declaration owns the existing first pane,
so no empty financial pane consumes half the chart. The domain must match and the first pane must
be empty when React claims it. Removing the declaration clears its axes, series, and references but
keeps the preserved pane available for a later declaration. Omit `useInitialPane` for additional
panes or for mixed financial/general charts.

Set `autoSize: true` when the host can resize. Give the host a measurable height; a width-only
percentage without a parent height cannot size a chart. While a host is hidden, Aeris retains the
last usable canvas size and resumes sizing when it becomes visible.

Row `id` values make focus and selection survive replacement and reorder. A row's `label` is the
portable text for `GeneralDataLabels`; a React formatter cannot run inside native or WebGPU
rendering. `GeneralPane` also accepts keyed `axes`, `series`, and `references` arrays for existing
callers. The complete mixed financial/general example is
[react.tsx](../examples/all_in_one/react.tsx).

## Brush state and linked charts

`GeneralBrushControls` is uncontrolled when `range` is omitted. `defaultRange` sets its initial
semantic range. `onChange` reports a bounded engine snapshot after changes. Supplying
`range` makes it controlled: the component applies that value, reports attempted changes, and
keeps the current parent value until a new value is rendered. A chart holds one transient brush at a
time. `null` clears only the component's axis and cannot clear a brush currently owned by another
axis. When a composition removes the brushed axis or replaces it structurally (dimension or scale
change), the engine clears the range and notifies brush subscribers. Controls bound to that axis ID
stay mounted and can start a new range on the replacement axis. `onChange` reports only its own
axis: it receives `null` once when another axis takes the brush, and a controlled control does not
take the brush back. Numeric,
temporal, and category ranges keep their own value types; category endpoints are identities.

```tsx
import { useState } from "react";
import type { general_brush_range } from "@aeristerminal/aeris-charts";

const [range, setRange] = useState<general_brush_range | null>(null);
<GeneralBrushControls options={{ axis: "month" }} range={range}
  onChange={(snapshot) => setRange(snapshot?.range ?? null)} />
```

For independent charts, route each drained `take_sync_events()` envelope to another chart with
a distinct `source` ID. `apply_external_sync_event()` rejects stale revisions and does not echo
the received event. A coordinator owns group membership and disposal; the engine resolves axis
IDs, category identities, and values against each target chart. See
[linked.mjs](../examples/all_in_one/linked.mjs).

## Imperative equivalent

`compose_general_pane` is the framework-neutral equivalent of `GeneralPane`:

```ts
const composed = chart.compose_general_pane({
  pane,
  axes: [
    { id: "month", dimension: "x", scale: "band" },
    { id: "amount", dimension: "y", scale: "linear" },
  ],
  series: [{
    key: "sales",
    kind: "column",
    options: { x_axis_id: "month", y_axis_id: "amount", title: "Sales" },
    data: [{ id: "jan", x: "Jan", y: 42 }],
  }],
  references: [{
    key: "goal",
    options: { kind: "line", axis_id: "amount", value: 50 },
  }],
});
// On the next composition, pass `current: composed.series[0].handle`.
```

The lower-level `add_axis`, `add_series`, `set_data`, and `add_general_reference` operations remain
available for isolated mutations. The default controls
are available through `create_general_legend`, `create_general_tooltip`, and
`create_general_brush_controls`; detach each returned handle during host cleanup. See
[standalone.mjs](../examples/all_in_one/standalone.mjs) and
[vanilla.mjs](../examples/all_in_one/vanilla.mjs).

| Recharts authoring concept | Aeris equivalent |
| --- | --- |
| `ResponsiveContainer` with `ComposedChart` | `AerisChart` with `autoSize: true` and one or more `GeneralPane` declarations in a sized host |
| `XAxis` / `YAxis` | `GeneralAxis` with explicit dimension, scale, and axis ID |
| `XAxis angle={-60} height="auto"` | Horizontal `GeneralAxis` with `tick_angle: -60`; Aeris measures the needed strip up to its pane limit |
| Custom `XAxis` tick with two SVG `tspan` lines | Explicit `ticks[].label` with newline-separated text; Aeris measures and paints up to four lines in the shared frame |
| `dataKey` on a series | `MappedGeneralSeries` with a typed `mapRow`, or canonical `GeneralSeries` data |
| `LabelList` | `GeneralDataLabels` plus portable row `label` values |
| `ReferenceLine` / `ReferenceDot` / `ReferenceArea` | `GeneralReference` line / dot / region |
| `Cell fill` on each `Bar` and a rounded positive top | Row `color` on each `column` and `bar_corner_radius` on the series; Aeris rounds the exposed end of negative bars too |
| `Legend` / `Tooltip` / range selection UI | `GeneralLegend` / `GeneralTooltip` / `GeneralBrushControls` |
| `Bar` series sharing a `stackId` with `stackOffset="sign"` | `column` or `horizontal_bar` series sharing `stack_id`, with `stack_mode: "normal"` to separate positive and negative totals at zero |
| `Bar` stacks with `stackOffset="expand"` | Shared `stack_id` and `stack_mode: "percent"`; each value is divided by its category's signed total over a fixed `0..1` axis |
| `Bar` stacks with `stackOffset="positive"` | Shared `stack_id` and `stack_mode: "positive"`; negative source rows remain queryable but have no painted segment |
| `Bar` stacks with `stackOffset="none"` | Shared `stack_id` and `stack_mode: "cumulative"`; signed values accumulate in series order and can overlap earlier marks |
| `Bar` stacks with `stackOffset="silhouette"` or `"wiggle"` | Shared `stack_id` and the matching `stack_mode`; both retain the ordered baseline transformation |
| `BarChart layout="vertical"` | `horizontal_bar` series on a numeric X axis and a band Y axis; the first category is at the top |
| Several `Bar` series without `stackId` | `column` or `horizontal_bar` series sharing one `group_id`; members subdivide each category band |
| `barSize` / `barGap` | Series `bar_max_width` and `bar_gap` in CSS pixels |
| `Bar` with a `[low, high]` `dataKey` | `range_bar` rows `{ id, x, low, high }` on a category X axis |
| `XAxis`/`YAxis` `scale="log"` or `"symlog"` | Axis `scale: "log"` or `"symlog"` (and the matching pane domain for X) |
| Axis `domain`, `reversed`, `tickCount`, `label` | Axis `domain`, `reverse`, `tick_count`, and `title` |
| `yAxisId` on series and axes | Distinct axis `id` values; each series names its `x_axis_id` and `y_axis_id` |
| `CartesianGrid` | Chart `grid: { vertLines, horzLines }` with per-axis `grid_visible`; rules use the resolved ticks |
| `syncId` across charts | `take_sync_events()` and `apply_external_sync_event()` through a host coordinator (see linked charts above) |

Aeris `stack_mode: "percent"` reproduces Recharts `stackOffset="expand"` for mixed-sign data too.
A category whose values have opposite signs divides by a small signed total, so its segments can
exceed the `0..1` axis and are clipped; in the pinned fixture two base bars fill the plot and their
top segments disappear. Prefer `stack_mode: "normal"` when mixed-sign shares must stay readable.

For an animated Recharts line, call `chart.set_general_transition_duration(400)` after creating
the Aeris chart, then update the retained series handle. Aeris uses one fixed transition clock
for shared frames and hits. It applies the update immediately when the browser requests reduced
motion; the pinned Recharts 3.10.1 line fixture continues animating under that media setting.
The transition duration persists in chart state, while an in-progress transition does not.

A Recharts `<ReferenceArea x1="Feb" x2="Mar" />` spans the plot height when Y bounds are
omitted. An Aeris `region` binds explicit X and Y axes and accepts any nonempty set
of `x_from`, `x_to`, `y_from`, and `y_to`. Omitted endpoints use the corresponding
plot edge, so an X-only band follows Y zoom and a Y-only band follows X zoom.
Like Recharts, regions paint beneath series with fill `#ccc` by default and every fill at 0.5 opacity.
`ReferenceLine y={16}` maps to a Y-axis `line` with `value: 16`.
Both references enter Aeris frame exports; row `label` values or `data_labels: true` keep
numeric labels portable across backends.

For a Recharts `YAxis tickFormatter`, preformat bounded explicit Aeris ticks while keeping
their numeric values. For example, `ticks: [0, 600, 1200].map(value => ({ type: "numeric",
value, label: new Intl.NumberFormat("fr-FR").format(value) }))` carries French grouping into
the shared frame and state without changing coordinates or hits. Update the explicit ticks when
the visible domain changes; a JavaScript formatter is not called during native rendering.

Aeris uses an explicit pane domain and typed axis values. Financial timestamps are UTC seconds;
general temporal axes use epoch milliseconds. Missing rows, duplicate values, and stable row IDs
follow the engine's per-family rules. Translate source records once at the application boundary,
then update the retained series handle; do not run data accessors inside chart rendering.

### Migrating Cartesian paths, points, boxes, and heatmaps

Recharts `Line`, `Area`, and `Scatter` map to `xy_line`, `xy_area`, and `scatter` on
explicit compatible X/Y axes. Translate each source record once into `{ id, x, y }`;
`bubble` adds a nonnegative `size`. Recharts `ZAxis dataKey="size"` maps to that row
field; pre-scale Recharts `range` values in host data when exact bubble areas matter.
Recharts `Area` with `[low, high]` data maps to Aeris `range_area` rows
`{ id, x, low, high }`, with `low <= high`. Recharts Y `ErrorBar`
margins map to Aeris `error_bar` rows `{ id, x, y, y_low, y_high }` with
absolute bounds. Stable IDs preserve focus,
selection, and update identity when rows reorder.

The pinned Recharts reference has no first-class `BoxPlot` or `Heatmap` component;
its custom Scatter shape and Treemap color example are visual analogues. Use Aeris
`box_plot` with category-band X, numeric Y, and ordered
`{ id, x, min, q1, median, q3, max }` rows. Use `heatmap_grid` with
`{ id, x, y, value, color? }` cells on compatible category or continuous axes.
These are distinct data models: migrate source statistics or cells instead of
copying a React SVG shape callback. Check the [row shapes](general_charts_api.md#object-data-shapes)
and [pane compatibility matrix](general_charts_api.md#pane-compatibility-matrix)
before composing them in one pane.

### Migrating a pie or donut chart

Recharts `<Pie dataKey="value" nameKey="name">` maps each source record to an Aeris polar
row with `x` as the category and `y` as the numeric value. Use a polar first pane for a
standalone chart. A donut is a `pie` series with a nonzero normalized `inner_radius`:

```ts
import { create_chart } from "@aeristerminal/aeris-charts";

const container = document.querySelector("#chart");
if (!(container instanceof HTMLElement)) throw new Error("missing #chart container");
const chart = await create_chart(container, {
  initialPane: { horizontal_domain: { type: "polar" } },
});
chart.add_axis({ id: "angle", pane: 0, dimension: "angle", scale: "angular_category", visible: false });
chart.add_axis({ id: "radius", pane: 0, dimension: "radius", scale: "radial_linear", visible: false });
const share = chart.add_series("pie", {
  pane: 0, x_axis_id: "angle", y_axis_id: "radius",
  polar: { inner_radius: 0.35, outer_radius: 0.9 },
});
share.set_data([
  { id: "north", x: "North", y: 42, color: "#4c8bf5" },
  { id: "south", x: "South", y: 28, color: "#28b7a4" },
]);
// Call chart.remove() when the owning view is disposed.
```

Recharts radii can be pixels or percentages; Aeris `inner_radius` and `outer_radius` are
fractions of its measured polar plot radius. Angles use the Recharts convention in radians, so
the default pie starts at three o'clock and sweeps counter-clockwise like `<Pie>`; convert
`startAngle`/`endAngle` degrees with `degrees * Math.PI / 180`. Preserve a stable row `id` for
updates and translate `Cell` fill overrides to row `color`. The same row shape and axes work for
`radar`, `polar_area`, and `radial_bar`, with each family's own series options and layout.
A Recharts `<RadialBarChart innerRadius="20%" outerRadius="90%"><RadialBar dataKey="value" />`
maps to `radial_bar` with `polar: { inner_radius: 0.2, outer_radius: 0.9 }`: each category
draws one concentric ring from the inside out and its value sets the arc length.
The runnable [polar.mjs](../examples/all_in_one/polar.mjs) recipe also enables the portable
engine legend and data labels.

### Migrating hierarchy and flow charts

Use an axis-free `{ type: "layout" }` first pane for a standalone funnel, treemap, Sankey,
or sunburst. These families use `add_layout_series(kind, options, data)` and a typed
input object rather than `add_series` and X/Y rows. For a Recharts Sankey, replace
numeric link indexes with stable node IDs:

```ts
import { create_chart } from "@aeristerminal/aeris-charts";

const container = document.querySelector("#chart");
if (!(container instanceof HTMLElement)) throw new Error("missing #chart container");
const chart = await create_chart(container, {
  initialPane: { horizontal_domain: { type: "layout" } },
});
const flow = chart.add_layout_series("sankey", {
  pane: 0, title: "Conversion flow", data_labels: true,
}, {
  type: "flow",
  nodes: [
    { id: "visit", label: "Visit" },
    { id: "trial", label: "Trial" },
    { id: "paid", label: "Paid" },
  ],
  links: [
    { source: "visit", target: "trial", value: 18 },
    { source: "trial", target: "paid", value: 11 },
  ],
});
// Keep `flow` to call flow.set_data(nextFlow) without replacing the chart.
```

The runnable [flow.mjs](../examples/all_in_one/flow.mjs) recipe bundles this path from the
installed package. `funnel` uses `{ type: "funnel", stages }`; `treemap` and `sunburst`
use `{ type: "hierarchy", nodes }` with stable `id` and `parent` references. Built-in
labels, exact hits, keyboard focus, and frame exports use the same engine geometry.
Treemap, Sankey and sunburst defaults match Recharts' default tiling, curved links, node and
link colors, sunburst hole and value labels, so an unstyled Recharts chart needs no extra
options. A Recharts `node={{ fill }}` override has no Aeris option yet: Sankey nodes keep
Recharts' default node fill while `color` sets the link color.
Custom Recharts SVG node/link components remain host-only presentation and need a
portable data/style mapping to appear in exported or native charts.

### Migrating ECharts Gantt, calendar and Marimekko charts

ECharts has no Gantt or Marimekko series; applications build them from `custom` series.
Aeris replaces that rendering code with typed layout inputs on an axis-free layout pane:

| ECharts authoring concept | Aeris equivalent |
| --- | --- |
| `custom` series `renderItem` task bars on a `time` X axis and `category` Y axis | `add_layout_series("gantt", options, { type: "gantt", tasks })` with `{ id, label, start, end }` tasks in UTC epoch milliseconds |
| Progress overlay rectangle in `renderItem` | Task `progress` in `[0, 1]` |
| Zero-length task drawn as a diamond | Task with `end == start` |
| Dependency polylines drawn in `renderItem` | Task `dependencies: [predecessorId]` (finish-to-start) |
| `markLine` at `Date.now()` | Host-supplied `now`; update it with `set_data` |
| `dataZoom` on the category Y axis for long task lists | Built-in bounded row viewport: wheel and keyboard focus scroll rows |
| `calendar` coordinate system with `heatmap` series | `add_layout_series("calendar_heatmap", options, { type: "calendar", days })` |
| `calendar.range` | Inferred: every whole UTC year that contains a day (up to ten) |
| `calendar.dayLabel.firstDay` | `week_start` (0 = Sunday ... 6 = Saturday) |
| `calendar.dayLabel.nameMap` | `day_labels`, seven names beginning with Sunday |
| `visualMap` continuous `inRange.color` | `low_color` and `high_color` |
| `[dateString, value]` heatmap rows | `{ date, value }` with `date` as epoch milliseconds inside the UTC day |
| `custom` series variable-width stacked rectangles | `add_layout_series("marimekko", options, { type: "mosaic", cells })` with `{ column, segment, value }` cells |
| Series-name legend for mosaic segments | Built-in in-canvas segment legend |

```ts
const plan = chart.add_layout_series("gantt", { pane: 0, title: "Release plan" }, {
  type: "gantt",
  now: Date.UTC(2026, 0, 21),
  tasks: [
    { id: "scope", label: "Scope", start: Date.UTC(2026, 0, 5), end: Date.UTC(2026, 0, 9),
      progress: 1, group: "Plan" },
    { id: "build", label: "Build", start: Date.UTC(2026, 0, 9), end: Date.UTC(2026, 0, 23),
      progress: 0.4, group: "Plan", dependencies: ["scope"] },
    { id: "ship", label: "Ship", start: Date.UTC(2026, 0, 23), end: Date.UTC(2026, 0, 23),
      dependencies: ["build"] },
  ],
});
```

ECharts interprets calendar date strings in the browser's local time zone; Aeris always uses UTC
days, so convert local dates with `Date.UTC(year, monthIndex, day)` before passing them. Custom
`renderItem` shapes beyond bars, progress, milestones and finish-to-start arrows have no Aeris
equivalent yet.

### Migrating ECharts gauge, chord, tree and graph charts (complete build)

These families need `@aeristerminal/aeris-charts/complete`; the standard build throws
`complete_build_required`. Each takes a typed layout input on an axis-free layout pane:

| ECharts authoring concept | Aeris equivalent |
| --- | --- |
| `gauge` series with `data: [{ value, name }]` | `add_layout_series("gauge", options, { type: "gauge", value, label })` |
| `axisLine.lineStyle.color: [[fraction, color], ...]` | `bands: [{ to, color }]` in data units inside `min..max` |
| `progress.show: true` | `progress: true` |
| `detail.formatter: "{value}%"` | `unit: "%"`, optional `decimals` |
| `chord` series with `data` and `links` | `add_layout_series("chord", options, { type: "graph", nodes, edges })` with `source`/`target` node IDs |
| `tree` series with nested `children` | `add_layout_series("tree", options, { type: "hierarchy", nodes })` with flat `{ id, parent }` nodes |
| `tree` `orient: "LR"` / `"TB"`, `layout: "radial"` | `hierarchy_orientation: "horizontal"` / `"vertical"` / `"radial"` |
| `tree` `edgeShape: "polyline"` with leaves aligned | `add_layout_series("dendrogram", ...)` |
| `expandAndCollapse` and `collapsed` node flags | Built-in click and Enter collapse; initial state through `collapsed: [ids]` |
| `graph` series `layout: "force"` / `"circular"` / `"none"` | `add_layout_series("network", { network_layout: "force" \| "circular" \| "fixed" }, { type: "graph", nodes, edges })` |
| `graph` `roam: true`, `draggable: true` | Built-in pan, wheel and pinch zoom, and node drag with pin on release |
| `graph` `categories` | Node `category`; nodes sharing one share a color |

Aeris follows d3-force where ECharts runs its own force model, so force coordinates differ. To
reopen a settled network without re-simulating, keep the exported state or pass its
`options().positions` back at creation. Circle packing, icicle, Venn and word cloud have no
ECharts series: use `"circle_packing"` and `"icicle"` with hierarchy data, `"venn"` with
`{ type: "sets", sets }` exclusive regions (venn.js shape), and `"word_cloud"` with
`{ type: "words", words }`.

```ts
import { create_chart } from "@aeristerminal/aeris-charts/complete";

const chart = await create_chart(container, {
  initialPane: { horizontal_domain: { type: "layout" } },
});
const network = chart.add_layout_series("network", { pane: 0, title: "Services" }, {
  type: "graph",
  nodes: [
    { id: "api", label: "API", category: "edge" },
    { id: "auth", label: "Auth", category: "core" },
    { id: "db", label: "Database", category: "core" },
  ],
  edges: [
    { source: "api", target: "auth" },
    { source: "auth", target: "db", value: 2 },
  ],
});
```

### Migrating ECharts waterfall, bullet, lollipop, dumbbell, slope and bump charts

ECharts builds these from stacked bars, `custom` series, extra scatter series and host-computed
values. Aeris takes the business data directly and computes the derived geometry in the engine:

| ECharts authoring concept | Aeris equivalent |
| --- | --- |
| Transparent placeholder `bar` stacked under the visible step `bar`, with host-computed running totals | `add_series("waterfall", options)` with `{ x, y }` signed steps; the engine keeps the running total |
| Subtotal or total bar whose value the host sums | Row `step: "subtotal"` or `step: "total"` with `y: null` |
| Per-item `itemStyle.color` by sign | `waterfall: { increase_color, decrease_color, subtotal_color, total_color }` |
| `markLine` or custom polylines between steps | Built-in connectors (`waterfall.connectors`, default on) |
| `custom` series painting bands, measure and target rectangles | `add_series("bullet", options)` with `{ x, y, target, comparative?, bands? }` rows |
| 2 px `bar` stem plus a `scatter` head | `column` or `horizontal_bar` with `bar_mark: "lollipop"`, `point_radius` |
| Transparent stacked base, 2 px connector `bar` and two `scatter` series | `range_bar` with `bar_mark: "dumbbell"` and `{ x, low, high }` rows |
| First/last point `label` plus `labelLayout: { moveOverlap: "shiftY" }` | `xy_line` with `end_labels: "both"` (or `"start"`, `"end"`) |
| Host-computed ranks on a `yAxis: { inverse: true }` value axis | `xy_line` with raw values and `rank: "dense"` or `"competition"`; the rank axis is top-down by default |
| `xAxis.boundaryGap: false` with room for end labels | Point X axis with `band_padding_outer` sized for the labels |

```ts
chart.add_axis({ id: "x", pane: 0, dimension: "x", scale: "band" });
chart.add_axis({ id: "y", pane: 0, dimension: "y", scale: "linear" });
const bridge = chart.add_series("waterfall", {
  pane: 0, x_axis_id: "x", y_axis_id: "y", title: "Profit bridge", data_labels: true,
});
bridge.set_data([
  { x: "Revenue", y: 420 }, { x: "COGS", y: -170 }, { x: "Gross", y: null, step: "subtotal" },
  { x: "Opex", y: -120 }, { x: "Net", y: null, step: "total" },
]);
```

The tooltip and accessibility snapshots of these rows carry `detail` with the running totals,
bullet target and bands, or rank, so a host formatter does not recompute them.
### Migrating legend and tooltip content

A Recharts `Legend` or `Tooltip` formatter runs in React's DOM. Use the package's DOM components
when a formatter needs JavaScript, locale data, or application state:

```tsx
<GeneralLegend options={{
  format_title: (title) => title === "Sales" ? "Net sales" : title,
  format_action: (visible, title) => `${visible ? "Hide" : "Show"} ${title}`,
}} />
<GeneralTooltip options={{
  mode: "shared",
  format_value: (value) => new Intl.NumberFormat("en-US", {
    style: "currency", currency: "USD",
  }).format(value),
}} />
```

These components read engine snapshots and keep visibility and hit semantics in the chart. Their
DOM markup is outside `take_screenshot()`. For an exported or native chart, supply portable row
`label` text for data labels and use the engine-drawn tooltip and legend. Enable the latter with
`chart.apply_options({ generalLegend: { visible: true, position: "top" } })` and omit the DOM
`GeneralLegend`; omit the DOM `GeneralTooltip` to keep the engine-drawn hover tooltip. Named legend
positions, side gutters, and `{ x, y }` custom coordinates are described in
[General chart API](general_charts_api.md). A React or DOM formatter cannot become an arbitrary
GPUI, WebGPU, or Canvas2D drawing callback.

| Recharts customization | Portable Aeris mapping |
| --- | --- |
| `stroke`, `strokeWidth`, `strokeDasharray` | Series `color`, `line_width`, and `line_style` (`solid`, `dotted`, `dashed`) |
| `type="step"` on a line or area | Series `interpolation: "step_middle"`; Aeris `"step"` retains its legacy step-after shape |
| `connectNulls` | Series `connect_missing: true`; missing rows remain in state and queries |
| `dot` and scatter `shape` | `point_markers`, `point_radius`, and `point_symbol` (`circle`, `square`, `diamond`, `triangle`) |
| `Cell` fill and symbol overrides | Row `color` and `symbol` data; the same values drive marks, hits, and snapshots |
| SVG fill gradient | Series `fill_gradient` with two CSS colors; `fill_opacity` controls area alpha |
| `LabelList` formatter | Translate the formatter result into each row's portable `label`, then enable `data_labels` |
| Custom HTML tooltip or legend | `GeneralTooltip` and `GeneralLegend` DOM options; use engine snapshots for any other host markup |

Portable choices are validated before engine mutation and persist with series/data state where
applicable. If a custom mark requires arbitrary application code at draw time, keep that work in
the browser host and document that it will not appear in GPUI or frame-image exports.

## Accessibility, state, and export migration

Map Recharts SVG `title`/`desc` to Aeris `accessibility.chart_title` and
a `messages.description: () => "..."` function under `accessibility`, then use the
chart's semantic focus controller.
Set `lang` for localized announcements and keep stable
row IDs so keyboard focus survives updates. Use a retained series handle's bounded
`accessibility_snapshot(offset, limit)` when the host needs a data summary;
`chart.accessibility().focus()` moves focus to the chart controller.

Recharts source rows normally live in React or other host state. Aeris
`chart.export_state()` captures portable chart state and general data in the
revisioned schema; `chart.import_state(state)` restores it atomically into a
compatible fresh chart. Financial OHLC rows and the current viewport are not
serialized, so supply those separately when restoring a mixed chart. Reattach
host callbacks and DOM controls after restore; they are application code.

Use `chart.take_screenshot()` for a synchronous frame image, including
engine-drawn labels and legend. DOM tooltips and custom host markup are outside
that image. With `backend: "auto"`, WebGPU can fall back to Canvas2D while the
same chart state and handles remain live; inspect `chart.backend_status()` for
the active stage and reason. Call `chart.remove()` when the owning view is
removed.

## Browser and server setup

The package exports JavaScript, declarations, design CSS, and a sibling WASM asset. Bundlers
should preserve the package's `import.meta.url` asset resolution or use the explicit `/wasm`
export when configuring asset copying. The package smoke test bundles all six checked-in
consumer examples from an installed npm tarball. The React entry is safe to import during server
rendering: it performs no DOM work until mount. It does not produce a static chart image on the
server; the browser initializes the chart after hydration. Call `chart.remove()` for imperative
owners and detach default DOM component handles when unmounting. React owns those cleanup steps
for its declarations.
