# General chart API

## Status and purpose

This document records the shipped general-chart API and its stable domain contracts. The current
package implements the Cartesian families: category columns and horizontal bars, category-band box plots, category/category plus
numeric/numeric and temporal/numeric heatmap grids, numeric XY scatter/bubble marks, numeric/temporal/category error bars,
and `xy_line`, `xy_area`, `range_area`, grouped/stacked bar, and stacked-area slices, plus
`waterfall` and `bullet` charts, lollipop and dumbbell bar marks, and slope and bump line options. Domain-aware panes, explicit axes, object and
typed bulk replacement/update, bounded retention, row labels, snapshots, hit testing, accessibility,
and V2 persistence are public for these implemented kinds/options. `xy_line`, `xy_area`, and `range_area` support continuous numeric,
temporal epoch-millisecond, and category band/point X domains. `error_bar` supports numeric and temporal X
axes with optional independent bounds on either axis, plus category band/point X with optional Y bounds only.
`box_plot` uses category-band X with numeric Y and requires a complete ordered
`min <= q1 <= median <= q3 <= max` row for visible geometry. All forms require their center/value channels
for a visible mark. `heatmap_grid` supports band X/Y string categories, continuous numeric X with numeric Y
coordinates, and temporal epoch-millisecond X with numeric Y coordinates. Heatmap cells tile their
whole category step with a 1 CSS px seam that shows the chart background, so axis band padding
never opens gaps between cells. Polar families are
implemented. Funnel, treemap, Sankey and sunburst have a public layout-pane API, validated inputs,
shared-frame geometry and exact hits. Their interaction, persistence, accessibility and transition
contracts shipped in G7; full competitor and manual parity evidence remains G8 work.
Defaults follow pinned Recharts 3.10.1. Treemap uses Recharts squarify tiling, one flat series
fill separated by a 1 CSS px seam that shows the chart background (no white stroke), and 14 px cell
names where they fit. Calendar heatmap days use the same seam, with wider background seams between
months. Treemap leaves may carry a
signed `color_value` (for example a daily percent change) as an opt-in: when any leaf has one,
cells shade from a neutral gray toward green for gains and red for losses by magnitude relative to
the largest change. Sankey uses Recharts' layout defaults (5 px margin, 10 px nodes and padding,
32 iterations, justified columns), curved links with `linkCurvature` 0.5 in the series color at
0.2 opacity, and nodes in Recharts' `#0088fe` at 0.8 opacity. Sunburst keeps Recharts' 50 px
center hole, 2 px ring gaps, counterclockwise sectors from three o'clock, a flat series fill with
a 2 px white outline, and per-sector value text; `data_labels` shows node names instead.

Gantt, calendar heatmap and Marimekko are standard-build layout families whose defaults follow
pinned Apache ECharts 6.1.0 (`examples/web_demo/fixtures/echarts_6_1_0/matrix.json` records the
reference rows and open differences). Each uses an axis-free layout pane and
`add_layout_series(kind, options, data)`:

- `gantt` takes `{ type: "gantt", tasks, now? }`. A task is `{ id, label, start, end, progress?,
  group?, dependencies? }` with UTC epoch-millisecond times and `end >= start`; `end == start`
  draws a milestone diamond. `progress` in `[0, 1]` fills the completed part of the bar.
  Contiguous tasks sharing `group` get one header row. `dependencies` lists finish-to-start
  predecessor IDs (acyclic, at most 32 per task) drawn as orthogonal arrows. `now` is an optional
  host-supplied instant drawn as a dashed ECharts `markLine`-red rule; the engine never reads a
  clock, so the host updates `now` with `set_data` when its notion of the current time changes.
  Rows that do not fit the pane scroll inside the engine: the wheel over the timeline moves whole
  rows (at either end the wheel follows the normal wheel policy), keyboard focus scrolls the focused task into
  view, and only visible rows are painted and hit-tested. The scroll offset is view state and is
  not persisted.
- `calendar_heatmap` takes `{ type: "calendar", days, week_start?, day_labels?, low_color?,
  high_color? }`. Each day is `{ date, value }`; `date` is any instant inside its UTC day, so the
  same input lands in the same cell in every host time zone. The chart covers whole UTC years
  (at most ten), one block of week columns and weekday rows per year, with `week_start` 0
  (Sunday) through 6 in the first row and seven host-supplied `day_labels` beginning with Sunday
  (falling back to the chart language's one-letter labels). Month names come from the chart's
  localized month names. Values
  shade linearly from `low_color` to `high_color` (default the series color from 20% to full
  opacity) with an in-canvas continuous legend. Days without a value draw an empty cell and have
  no hit. Accessibility items are every day of the covered years, labelled like `29 February
  2024`; Left/Right move one week and Up/Down one day.
- `marimekko` takes `{ type: "mosaic", cells }` with `{ column, segment, value }` cells, unique per
  column and segment and non-negative. Column widths are proportional to column totals and
  segment heights to their share of the column, both in first-appearance order. The first segment
  uses the series color and later segments continue through the ECharts default palette; an
  in-canvas legend lists every segment, and column names label the baseline where they fit.

Hits return `element: "task"`, `"day"` or `"cell"` with the task ID, `YYYY-MM-DD` date or
`column/segment` ID. Value updates of these families apply immediately without a transition.

### Relationship and hierarchy layout families (complete build)

Nine more layout kinds ship only in the complete build. The standard build rejects the kinds and
their `gauge`, `graph`, `sets` and `words` inputs with `complete_build_required` before any engine
state changes, including on V2 restore. It rejects the hierarchy, network and word cloud options
below the same way on any layout kind, while V2 restore ignores those keys so saved state that
carries only their defaults still restores. Each uses an axis-free layout pane and
`add_layout_series(kind, options, data)`:

| Kind | Data | Layout and reference |
| --- | --- | --- |
| `gauge` | `{ type: "gauge", value, min?, max?, label?, unit?, decimals?, bands?, progress? }` | ECharts 6.1.0 `gauge` defaults: 270° track from 225° to -45°, up to 32 ascending `{ to, color, label? }` bands, optional progress arc, ten split lines with five ticks, kite pointer; values clamp to `min..max` and animate between updates |
| `chord` | `graph` | ECharts chord layout with d3-chord ribbons; the 3° pad follows only nonempty groups and is capped at half the circle |
| `tree` | `hierarchy` | d3-hierarchy `tree()` (Buchheim) with curved links; collapsible |
| `dendrogram` | `hierarchy` | d3-hierarchy `cluster()`: every leaf on the deepest level, elbow links; collapsible |
| `circle_packing` | `hierarchy` | d3-hierarchy `pack()`: leaf area proportional to value, nested parents, deepest-first hits |
| `icicle` | `hierarchy` | d3-hierarchy `partition()`: one band per depth, breadth by summed value |
| `network` | `graph` | d3-force 3.0 simulation (`force`), nodes on a circle in input order (`circular`), or node `x`/`y` (`fixed`) |
| `venn` | `{ type: "sets", sets: [{ sets, size, label? }] }` | Exclusive venn.js-shaped regions; two and three sets are area-proportional, more sets (up to 8, 255 regions) are a labelled approximation whose overlaps carry no area meaning |
| `word_cloud` | `{ type: "words", words: [{ text, weight, color? }] }` | Up to 500 words placed by descending weight on a deterministic spiral from measured text; words without room are omitted |

The shared `graph` input is `{ type: "graph", nodes, edges? }`. Nodes are
`{ id, label, category?, value?, x?, y? }` (`x` and `y` together or not at all) and edges are
directed `{ source, target, value? }` with `value` defaulting to 1. Cycles and self-links are
allowed; a repeated `(source, target)` pair is rejected. Graphs hold at most 4,096 nodes and
16,384 edges, and nodes sharing a `category` share a color.

Layout options added for these kinds:

- `hierarchy_orientation`: `"horizontal"` (root on the left, the default), `"vertical"` (root on
  top) or `"radial"` (root at the center). Trees and dendrograms accept all three, icicles the
  first two, and other kinds only the default.
- `collapsed`: IDs of collapsed tree or dendrogram nodes with children. A primary click or
  Enter/Space on a focused node folds and unfolds it inside the engine; `options()` reports the
  current set, V2 state persists it, and new data drops IDs that no longer name a node with
  children. Accessibility items of such nodes carry `expanded`.
- `network_layout` (`"force"`, `"circular"`, `"fixed"`; `fixed` needs `x`/`y` on every node),
  `network_link_distance` (1 to 1000 CSS px, default 30), `pinned` (`{ id, x, y }` nodes held in
  place), `network_view` (`{ x, y, zoom }`, zoom 0.1 to 10) and `positions` (settled force
  positions from a state export; passing them back restores the layout without simulating).
  Dragging a node pins it where it is released, Escape cancels a drag, dragging empty space pans,
  and the wheel or a pinch zooms around the pointer ahead of the time-scale policy. The force
  simulation settles in 300 ticks on the chart's animation clock; under reduced motion it settles
  at once and never animates.
- `word_cloud_min_size` and `word_cloud_max_size` (4 to 256 CSS px, defaults 10 and 48),
  `word_cloud_scaling` (`"sqrt"`, the default, keeps area linear in weight; `"linear"` keeps the
  font size linear) and `word_cloud_rotate` (rotate every second word by 90°). Accessibility items
  of words carry `placed`, and a font load re-places every word.

`apply_options` does not accept `collapsed`, the network options or the word cloud options:
collapse, pins and view change through interaction, and the layout options are fixed at creation.
Hits return `element: "node"`, `"link"`, `"band"`, `"progress"`, `"pointer"`, `"region"` or
`"word"`. Gauge values and hierarchy leaf values transition like the other layout families.
`examples/web_demo/fixtures/echarts_6_1_0/matrix.json` records the reference rows and open
differences for every G12 kind.

### Map layout families (complete build)

Three map kinds ship only in the complete build. The standard build rejects the kinds, their
`geo`, `geo_points` and `geo_links` inputs and the `map_view` option with
`complete_build_required` before any engine state changes. Aeris bundles no world geometry:
hosts supply GeoJSON or TopoJSON. Each kind uses an axis-free layout pane and
`add_layout_series(kind, options, data)`:

| Kind | Data | Layout and reference |
| --- | --- | --- |
| `choropleth` | `{ type: "geo", geojson \| topojson, object?, values?, id_property?, label_property?, projection?, scale?, missing_color? }` | Features projected by a d3-geo 3 projection fitted to the plot, triangulated with holes, and filled by value; ECharts 6.1.0 `map` series with a continuous `visualMap` is the reference |
| `bubble_map` | `{ type: "geo_points", points: [{ id, at, label?, value?, color? }], symbol?, radius?, projection? }` | Symbols whose radius scales with the square root of `value` (default 3 to 24 CSS px), larger bubbles painted first |
| `flow_map` | `{ type: "geo_links", links: [{ id?, source, target, source_label?, target_label?, value?, color? }], width?, arrows?, projection? }` | Great-circle arcs (d3-geo `geoInterpolate`) whose width scales linearly with `value` (default 1 to 6 CSS px), with optional target arrowheads |

Map data:

- `geojson` is an RFC 7946 FeatureCollection, Feature or bare geometry (Polygon, MultiPolygon,
  LineString, MultiLineString, Point or MultiPoint). `topojson` is a topology decoded like
  topojson-client 3 `feature()`; `object` names the object and may be omitted when there is only
  one. A dataset holds at most 4,096 features, 262,144 positions and 65,536 rings, and a bubble
  or flow dataset at most 4,096 marks.
- A feature's identity is the `id_property` value, else its GeoJSON `id`, else `#<index>`; ids
  must be unique. Its label is the `label_property` (default `name`), else the id.
- `values` is `{ id: value }` or `[{ id, value }]`. A value naming an unknown feature, a
  duplicate, or a non-finite value rejects the whole dataset. Features without a value use
  `missing_color` (a neutral gray by default).
- `projection` is `{ type?, rotate?, center?, parallels?, padding? }`. `type` is `"mercator"`,
  `"equal_earth"` (the default), `"natural_earth1"`, `"orthographic"` (the visible hemisphere),
  `"conic_equal_area"` (Albers with `parallels`, default `[0, 60]`) or `"identity"` (planar
  coordinates with y up). The geometry always fits the plot inset by `padding` (default 8 CSS
  px, d3 `fitExtent`); `center` then moves a longitude/latitude to the middle at the fitted
  scale. Spherical projections cut at the antimeridian, and resampling is adaptive.
- `scale` is `{ type: "sequential", low_color?, high_color? }` (the default: the series color at
  20% opacity to the series color), `{ type: "diverging", low_color?, mid_color?, high_color?,
  midpoint? }` (blue, light gray and red by default), or `{ type: "quantize", classes?, method?,
  low_color?, high_color? }` with 2 to 9 classes of equal width (`"equal_interval"`, d3
  `scaleQuantize`) or equal count (`"quantile"`, d3 `scaleQuantile`). A value on a class
  threshold belongs to the upper class. The frame draws a continuous or stepped color legend
  under the map.
- A mark anchor `at`, `source` or `target` is a `[longitude, latitude]` position (planar `[x, y]`
  under `identity`) or the id of a feature of the pane's base map, which places it at that
  feature's d3 `geoCentroid`. The base map is the pane's first choropleth series. Bubbles and
  flows project through its fitted projection; a pane without a base map fits the marks' own
  `projection` to their positions. A feature anchor that names no base-map feature hides the
  mark, as does a position on the far side of an orthographic globe.

Every map series of a pane shares one `map_view` (`{ x, y, zoom }`). `zoom` is 1 to 32, and
`x` and `y` offset the map center as fractions of the map width and height, at most
`0.5 - 0.5 / zoom` either way, so a pan never leaves the fitted map. Dragging inside the map
pans it, the wheel or a pinch zooms around the pointer ahead of the time-scale policy, and an
unmoved press is an ordinary layout click. `options()` reports the view of map series, V2 state
persists it, and `apply_options` does not accept it. A view change maps the cached projected
geometry through one transform and clips it to the map; it never reprojects or retriangulates.

Hits return `element: "feature"` (polygons answer even-odd, so a hole has no hit and an island
answers its feature), `"bubble"` or `"link"` with the input row. Tooltips and accessibility
items name the feature, bubble or flow (`source to target`, localized) with its value, and the
keyboard focus ring sits on each feature's interior anchor. Value updates apply immediately
without a transition.

### 3D layout families (complete build)

Four 3D kinds ship only in the complete build. The standard build rejects the kinds, their
`points_3d` and `surface` inputs and the `camera_3d` option with `complete_build_required`
before any engine state changes. Each kind uses an axis-free layout pane and
`add_layout_series(kind, options, data)`; ECharts GL 2.1.0 `grid3D` is the reference:

| Kind | Data | Layout and reference |
| --- | --- | --- |
| `bar_3d` | `{ type: "points_3d", points: [{ x, y, z, label?, color? }], x_axis?, y_axis?, z_axis?, size? }` | Shaded boxes from zero to each `z`; `size` is the footprint in box units (default 70% of a category band, or 60% of the even point spacing); ECharts GL `bar3D` |
| `scatter_3d` | Same `points_3d` input | Screen-sized discs `size` CSS px across (default 10); ECharts GL `scatter3D` |
| `line_3d` | Same `points_3d` input | A polyline through the points in input order, `size` CSS px wide (default 2); ECharts GL `line3D` |
| `surface_3d` | `{ type: "surface", x, y, z, x_axis?, y_axis?, z_axis?, colors?, wireframe? }` | A shaded grid colored by height through up to 9 `colors` (empty shades the series color), outlined unless `wireframe: false`; ECharts GL `surface` |

3D data:

- A `points_3d` dataset holds at most 4,096 points with finite coordinates of magnitude at
  most 1e15. `x_axis` and `y_axis` may list `categories`; values on a category axis are
  category indices placed at band centers. `size` is above 0 through 100.
- A surface has 2 to 128 strictly increasing `x` and `y` positions and one height per sample,
  row by row (`z[row * x.length + column]`); `null` leaves a hole, and cells touching a hole are
  not drawn. A surface paints at most 64 by 64 cells, striding evenly through longer grids;
  every sample stays hittable at the nearest drawn corner.
- Value axes keep zero in their extent and take five nice ticks, as ECharts value axes do.

Every 3D series of a pane shares one 100-unit axis box (data `x` across, `y` from near to far,
`z` up) and one camera. The pane's first 3D series owns the axis names and categories and holds
`camera_3d`: `{ alpha?, beta?, distance?, projection?, orthographic_size? }`, ECharts GL
`viewControl` shaped, defaulting to 20, 40, 200, `"perspective"` and 150. `alpha` is -90 to 90,
`beta` above -180 through 180, `distance` 40 to 400 and `orthographic_size` 20 to 400; out-of-range
values reject the series with `invalid_options`. A later 3D series in the pane joins that camera
and ignores its own `camera_3d`. ECharts GL adds one unit to `orthographicSize`,
so its 149 is Aeris's 150.

Dragging inside the plot orbits the camera (a drag across the plot height turns the box two
radians, as ECharts GL does), the wheel or a pinch moves it closer, and with the pointer over the
pane the arrow keys turn it by 5 degrees (15 with Shift), `+` and `-` zoom one wheel notch, and
Home restores the default angles and distance. Escape or a cancelled drag restores the camera
from before the drag, and an unmoved press is an ordinary layout click. `options()` reports the
camera of 3D series, V2 state persists it bit for bit, and `apply_options` does not accept it.

The engine projects, back-face culls and shades (ambient plus one directional light) the whole
scene, then sorts whole pieces (one bar box, one symbol, one half line segment, one surface cell
with its wireframe) far to near by view depth, with series order then input order breaking
ties. It paints the result as one ordered, opaque mesh of per-triangle colors, so every backend
draws the same picture without a depth buffer. Pieces that intersect, such as two crossing
surfaces or a bar piercing a surface, show the nearer piece whole on top. A pane paints at most
131,072 triangles; the piece that would cross the bound and every piece after it are dropped.
Hits name the front-most item under the pointer across every 3D series of the pane:
`element: "point"` with the point index, or `"sample"` for the corner of the hit cell nearest
the pointer (row `y index * x count + x index`). Tooltips and
accessibility items name the point label (default its `x` and `y`) or the sample position, with
the height as the value.
React `GeneralPane` supports keyed arrays and direct axis, series, reference, label, legend,
tooltip, and brush declarations. `MappedGeneralSeries` maps typed application rows before the
engine transaction; brush controls support controlled and uncontrolled ranges. See
[general chart authoring](general_authoring.md) for composition, lifecycle, and migration examples.

General-series legend metadata is engine-owned. `chart.general_legend_snapshot(pane?)` returns bounded
series metadata in stable engine order, including pane, kind, title, color, and visibility. Hidden series remain
present with `visible: false` so a host-rendered legend can expose them without rebuilding state independently;
pane filtering is read-only and V2 restore reconstructs the same semantic entries.

Cross-series tooltip grouping is also engine-owned. `chart.general_shared_tooltip(series, row)` returns the
visible rows in the same pane whose exact horizontal datum matches the anchor. Ordering is stable series order
then row order; duplicate X values are preserved, hidden series are excluded, and a heatmap may contribute more
than one cell for one X coordinate/category. Every item and shared snapshot carries `color`, the opaque
identity color of the row's mark (sector fill for pie, polar area and radial bar; otherwise the row color,
series color or default line color).

General hover is drawn by the engine by default. Cartesian panes with an index-axis series hover the nearest
X datum or category band, like Recharts' axis tooltip: the chart paints a cursor line in the crosshair
`vertLine` style (a horizontal line for horizontal bars), an active dot on each line and area at that datum,
and a floating tooltip with the date or category header and one colored row per shared item. Other panes
describe the item under the pointer. `chart.apply_options({ generalTooltip: { visible: false } })` hides the
tooltip; `crosshair.vertLine.visible` controls the cursor. Like Recharts, the tooltip sits 10 px from the
anchor, flips to the other side when it would cross the plot edge, and stays inside the chart surface; it
paints above axis labels and the legend. Keyboard focus on a general row or layout item shows the same
tooltip at the focused datum or mark until the pointer hovers, Escape dismisses it, or focus leaves.
Crosshair-free exports omit it. The tooltip is part of the ordered frame, so it matches across Canvas2D,
WebGPU, GPUI and native rendering.

`chart.apply_options({ generalLegend: { visible: true } })` (or the same option in `create_chart`) adds an
engine-drawn legend below the plot by default: a rounded color pill and title per series (per layout series in a layout pane).
Entries follow Recharts' default `itemSorter="value"`: a stable sort by name in JavaScript string order
(case-sensitive), which `create_general_legend` also applies. `general_legend_snapshot` keeps engine order.
Horizontal positions wrap into at most three rows with a "+N more" entry when they do not fit. Clicking an entry toggles that
series' visibility and notifies `subscribe_general_legend_change`; hidden series stay listed dimmed. Like the
tooltip, it is part of the ordered frame and matches across every backend, including GPUI hosts, which enable it
through the same engine option. Touch taps have an expanded 44 CSS-pixel control target;
overlapping targets choose the nearest entry, and `+N more` is descriptive only.
`generalLegend.position: "top"` places it above the plot;
`"insideTop"`, `"insideBottom"`, `"insideTopLeft"`, `"insideTopRight"`, `"insideBottomLeft"`,
`"insideBottomRight"`, `"insideLeft"`, `"insideRight"` and `"center"` overlay the plot.
`"left"` and `"right"` reserve a side gutter beyond the axes and paint in the shared
unscissored axis layer. Side entries stack vertically and fold excess entries into `+N more`.
`generalLegend.position: { x, y }` anchors the legend's lower-right corner at custom coordinates. `x` is relative to the full chart width and `y` to the general pane's height from its top; each value is a CSS-pixel number or a percentage string such as `"75%"`. This placement reserves no plot space, paints in the unscissored axis layer and ignores `offset`. Recharts measures custom coordinates from its margin-inset chart area (5 CSS px by default); add that margin to an Aeris coordinate when migrating the same visual placement. `generalLegend.offset` adds a bounded CSS-pixel inset at the selected edge for named positions. The default position is `"bottom"`.
It is off by default so hosts that render `create_general_legend` do not show two legends.

General range selection uses `chart.set_general_brush(axis, from_coordinate, to_coordinate)`. The browser sends
CSS-pixel endpoints once; Rust converts them immediately into a numeric, temporal, or category range and owns
the transient selection. `general_brush_snapshot()` returns that semantic range plus a bounded list of visible
row identities on the selected axis. Zoom/resize therefore reprojects the stored range instead of retaining stale
pixels. `clear_general_brush()` removes it.

Public coordinate origins are intentionally different by dimension: `chart.general_hit_test(pane, x, y)`
accepts plot-local CSS pixels for `x` (zero at `pane.get_geometry().left`) and chart-top CSS pixels
for `y` (zero at the chart container's top, including panes above the target pane). An exact hit omits
`max_distance`; passing it requests a nearest hit. Convert a pointer's chart-relative X by subtracting
`pane.get_geometry().left`, but do not subtract `pane.get_geometry().top` from Y. The coordinates
passed to `set_general_brush` follow the same X/Y origins for the chosen axis.

Reference components are first-class engine state. `chart.add_general_reference(...)` creates a line, dot, or
rectangular region bound to explicit general axes. Each reference declares `extend_domain`; automatic domains
include its coordinates only when that flag is true. References lower into the same backend-neutral frame,
participate in pane/axis lifecycle protection, and round-trip in V2 persistence.

The general API is additive. Existing financial series, data shapes, pane methods, price-scale
handles, snake-case methods, and persistence V1 keep their current meaning. In particular,
`"line"`, `"area"`, `"bar"`, `"histogram"`, `"baseline"`, `"candlestick"`, and `"footprint"`
remain financial-time series. Aeris must never guess whether a row belongs to the financial or
general data domain.

The imperative API remains canonical. Camel-case aliases and the React adapter call
the same mutations rather than defining another chart model; snake-case methods remain supported.

## Image export and backend recovery

`chart.take_screenshot()` returns an `HTMLCanvasElement` at the chart's current device-pixel
resolution. It composites panes and axes from the ordered frame; the optional
`add_top_layer` and `include_crosshair` arguments default to `true`. Export an image
with `chart.take_screenshot().toBlob(...)` or `toDataURL("image/png")`. DOM content
supplied by a host outside the chart canvases, such as a custom HTML tooltip, is outside
this image. The engine-drawn general tooltip and legend are frame content and are included.

`backend: "auto"` tries WebGPU and falls back to Canvas2D when the adapter, device or
surface is unavailable or is lost. `chart.backend()` reports the active executor;
`chart.backend_status()` gives a structured `stage` and `reason` for diagnostics.
The engine state and series handles survive backend fallback, so the host should keep
the chart mounted. A later data update and export use the surviving chart. Use
`backend: "canvas2d"` to request Canvas2D explicitly. Call `chart.remove()` on host
disposal to detach listeners and release renderer resources.

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
  | { type: "polar" }
  | { type: "layout" };

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

V2 state can be restored atomically into a fresh financial-default chart or into a fresh
general-first chart whose initial domain matches the general-only document. General-only restore
never retains or creates a hidden financial series. Pane domains and `preserve_empty`, axes,
datasets, series ordering, and visibility come from the document. Restore rejects targets with
issued general or drawing handles, data, extra financial series, or a conflicting initial general
domain, and leaves the target unchanged.

Financial OHLC rows are supplied by the host after a mixed V2 restore; the document restores
the financial pane and its default series, not those rows. V2 also does not serialize the
current viewport. Reattach host callbacks and data before comparing the restored presentation.

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
  tick_angle?: number;
  domain_padding?: number;
  band_padding_inner?: number;
  band_padding_outer?: number;
  zero_line?: boolean;
  grid_visible?: boolean;
  line_visible?: boolean;
}
```

Validation is structural and atomic:

- X axes must match the pane's horizontal domain.
- Y axes may be linear, log, or symlog; polar radius axes use radial linear.
- A log axis rejects non-positive explicit bounds and ignores non-positive data for automatic
  domain contribution while retaining those rows as missing geometry.
- Band padding is finite and clamped only when the documented range permits it; invalid values do
  not partially mutate the axis.
- `domain_padding` symmetrically expands a Cartesian numeric base domain by a fraction of its
  transformed span before runtime pan/zoom. It accepts `0..=1`, defaults to `0`, uses
  multiplicative spacing for logarithmic axes, and rejects nonnumeric axes instead of becoming a
  silent option. Runtime reset returns to the padded base domain.
- With `domain_padding` left at `0`, automatic linear, log and symlog X/Y domains of point and
  path marks gain a 4% margin of their transformed span at each end, so markers and line caps
  never sit on an axis line. Axes of bars, stacks, areas and histograms (which grow from zero) keep
  zero on the axis line; a zero end of a value axis also stays put.
- Point category axes default `band_padding_outer` to `0.5` (half a step at each end) so the
  first and last points sit inside the plot; band axes keep `0.1`.
- Symlog axes tick zero and signed 1/2/5 decades outward from the linear threshold, thinning to
  decades and then every n-th decade to honor `tick_count`. A thinned decade too close to zero to
  keep its label clear is skipped.
- `line_visible` (default `true`) draws the axis line along the plot edge; `false` hides the
  line while ticks, labels and titles stay.
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
- Horizontal X axes accept `tick_angle` in clockwise degrees from `-90` to `90` (default `0`).
  Rotated labels reserve measured strip height, share one frame transform across renderers, and
  persist in V2 state. The strip uses at most 45% of the pane height; labels that cannot fit
  are omitted while the axis line and title remain. Other axis dimensions reject nonzero angles.
- Explicit tick labels can contain newlines. The engine measures and paints at most four
  lines per label, including on rotated X axes, and reserves X strip height or Y strip width
  for the visible lines. CRLF line endings are normalized for paint and measurement while
  their input labels stay portable. Additional lines are omitted; tick coordinates and grid rules
  are unchanged.
- `grid_visible` projects that axis's tick coordinates into the clipped pane underlay when the
  matching chart-wide grid direction is visible. `zero_line` independently draws a solid rule when
  zero lies inside a numeric domain. Coincident rules are emitted once, with the zero rule taking
  precedence, so multiple axes do not darken shared coordinates. A rule that would sit beside a
  visible axis line on the plot edge is omitted; the axis line marks that edge.
- Temporal axes select a bounded UTC interval from milliseconds through calendar years. Intraday,
  day, month, and year labels use the chart language's month names (or host-injected locale month
  names) and the same shared `AxisFrame` as numeric and category axes; hosts do not run a
  parallel date-axis layout.

The supported API exposes `chart.add_axis(options)`, `chart.axis(id)`,
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
  | "range_bar"
  | "column"
  | "waterfall"
  | "bullet"
  | "histogram"
  | "horizontal_bar"
  | "scatter"
  | "bubble"
  | "box_plot"
  | "heatmap_grid"
  | "error_bar"
  | "pie"
  | "radar"
  | "radial_bar"
  | "polar_area"
  // Complete build only (statistics families):
  | "violin"
  | "density_curve"
  | "ridgeline"
  | "strip_plot"
  | "beeswarm"
  | "dot_plot"
  | "ecdf"
  | "qq_plot"
  | "hexbin"
  | "contour"
  | "parallel_coordinates"
  | "scatter_matrix";
```

`"histogram"` is the one general name that a financial series kind also uses. `add_series("histogram",
options)` creates the general distribution series when `options` carries `x_axis_id`; without axis
bindings it keeps creating the financial time-series histogram.

Grouped and stacked charts are options on compatible column, horizontal-bar, and area series, not
separate engines or renderer-specific kinds. A stack ID joins series only when their pane, axes,
orientation, and category/continuous coordinate semantics match.

The series API exposes `group_id` on `column` and `horizontal_bar`, and
`stack_id`/`stack_mode` on both bar orientations and `xy_area`. Vertical columns bind a category X axis
to a numeric Y axis; horizontal bars reuse the same category/value rows with a numeric X axis and band Y axis.
Grouped bars subdivide the category band; matching stacked bars occupy one group slot. Stacked areas align members by exact X identity across
numeric, temporal, and category domains and fill between the preceding cumulative boundary and the new
cumulative boundary. `stack_mode: "normal"` uses independent positive/negative accumulation around zero,
while `stack_mode: "percent"` follows d3/Recharts `expand`: each value is divided by its position's
signed total (a zero total keeps raw values), all values stack on one running boundary, and the
automatic value domain is fixed to `0..1`. Mixed-sign positions can overshoot that domain and are clipped.
`stack_mode: "positive"` stacks values above zero and suppresses negative marks; their source
rows remain available to data queries. Every nondefault mode requires a stack ID.
`stack_mode: "cumulative"` adds signed values onto the preceding boundary in series order, so
negative values may overlap earlier marks. The automatic value domain includes every intermediate
boundary. This corresponds to the reference library's `stackOffset="none"`; `"normal"` corresponds
to its diverging `"sign"` offset.
`stack_mode: "silhouette"` adds signed values in the same order but shifts each category or X
position's baseline by half its final signed total, centering the finished stack around zero.
Automatic domains still include every intermediate boundary, including overshoots from mixed signs.
This follows the [D3 silhouette offset](https://d3js.org/d3-shape/stack#stackOffsetSilhouette).
  `stack_mode: "wiggle"` uses cumulative signed layers and shifts each baseline to reduce
  weighted movement between adjacent category or X positions in axis order. Empty or zero-total
  positions retain the preceding baseline. Automatic domains include each shifted boundary.
  This follows the [D3 wiggle offset](https://d3js.org/d3-shape/stack#stackOffsetWiggle).
Unstacked `xy_area` uses `baseline_policy: "zero"` by default. `"domain_min"` and
`"domain_max"` follow the effective Y-axis view, including pan and zoom; they correspond to
the [Recharts Area](https://recharts.github.io/en-US/api/Area/) `baseValue: "dataMin"` and
`"dataMax"` choices, which resolve against the axis domain.
`baseline_value` supplies a finite numeric value instead and cannot be combined with a
nonzero policy. Stacked areas derive their painted baseline from the prior member; a valid
explicit or policy baseline remains saved but dormant while stacked. Removing `stack_id`
restores that baseline without requiring another option update.
`xy_area` and `range_area` accept `fill_gradient: [topColor, bottomColor]` with two supported CSS
colors. The colors and their alpha reach the shared vertical `AreaFill` or gradient-band primitive;
`fill_opacity` multiplies both stop alphas. Without this option, the existing one-color area fade
or solid band remains.
`column`, `horizontal_bar`, and `range_bar` accept the same `fill_gradient` pair. Both colors
run from the top to the bottom of each bar in bitmap space, including horizontal bars; their
CSS alpha is retained. Rounded bar ends clip the same ramp to the rounded shape. A row `color`
override replaces the gradient for that mark.
Both bar orientations accept `bar_gap` (0–64 CSS pixels of empty space per slot) and
`bar_max_width` (1–512 CSS pixels). The engine centers the remaining bar within its group slot;
stack members keep the same slot when configured alike. `range_bar` accepts the same thickness
controls. Columns and horizontal bars also accept `bar_corner_radius` (0–64 CSS pixels). The
engine rounds only the value-end corners (the end away from the baseline), so bars stay square
where they meet the axis; in a stack only the exposed end is rounded and interior joins stay square.
`data_labels` on columns sit above (or below, for negative values) their own column, and on
horizontal bars beside the bar end within that bar's own row, so labels never overlap a
neighboring series. The radius is clamped to half the rendered bar's smaller
dimension. All three bar kinds reuse ordered rectangle primitives, exact rectangle hits, bounded labels, snapshots,
typed/object updates, and V2 persistence.

`range_bar` uses the aligned low/high dataset contract with a category X axis and numeric Y axis.
Each complete row lowers to one category-width rectangle spanning its low/high values; missing or
invalid bounds remain queryable but emit no geometry. Range bars use exact rectangle hits and the
same persistence and update paths as range areas.

`histogram` bins one-dimensional samples in the engine, so every host and backend paints the
same bins. It requires a continuous pane with linear X and Y axes. Rows are `{ value, id? }`
objects or `histogram_columns` (`values: Float64Array`, optional `values_valid`, optional `ids`);
null or invalid samples stay queryable by ID but are not counted, and non-finite samples are
rejected. The `histogram` option controls binning:

```ts
interface general_histogram_options {
  bin_count?: number | null; // 1..1000 equal bins spanning exactly [min, max]
  bin_width?: number | null; // finite width; edges align to multiples of it; overrides bin_count
  normalization?: "count" | "percent" | "density"; // default "count"
  cumulative?: boolean; // running totals; cumulative density is the empirical CDF
}
```

Without either setting, the bin count follows Sturges' rule (`ceil(log2 n) + 1`) and the width
is rounded up to a 1/2/5 multiple of a power of ten, with edges aligned to that width. Identical
samples produce one unit-wide bin centered on the value. Each bin spans `[from, to)`; the last
bin also contains its right edge. `percent` reports each count as a percentage of the counted
samples, and `density` divides that share by the bin width so bar areas sum to one.
A setting, replacement, or update that would need more than 1000 bins is rejected with
`invalid_data` and leaves the previous state unchanged; `bin_count` outside 1..1000 or a
non-positive `bin_width` is `invalid_options`, and other kinds reject non-default
`histogram` options. Bins are cached per series and recomputed only when the dataset generation
or options change.

The X axis domain covers the bin edges (padded, not rounded to zero) and the Y domain includes
zero and the tallest bin. Bars accept `color`, `fill_opacity`, `fill_gradient`, `bar_gap`,
`bar_max_width`, `bar_corner_radius`, and `data_labels` like columns; grouping and stacking
are rejected. Hits, selection, tooltips, brushes, and accessibility rows address bins: `row` is
the bin index and `row_id` is `{ generated: "<bin index>" }`. Tooltip and accessibility
snapshots label each bin as an interval such as `[10, 20)` (`]` for the last bin), report the
bin height as `value`, and expose the edges as `x_low`/`x_high`. An X brush selects bins
whose center lies in the range; a Y brush selects them by height. A shared tooltip anchored on a
histogram reports only that bin, and other series never include histogram bins. Histogram series
appear in the legend like columns and persist in V2 documents with their samples and options.

### Business families: waterfall, bullet, lollipop, dumbbell, slope and bump

Two of these families have their own kind because their rows carry data no existing contract
holds; the other four are presentation options on an existing kind because their data is
already an existing row shape:

| Family | API | Why |
| --- | --- | --- |
| Waterfall | `add_series("waterfall")` | A row carries a step role (relative, subtotal, total), and the bar extents come from an engine-owned running total, not from the row value alone. |
| Bullet | `add_series("bullet")` | A row carries a measure plus a target, an optional comparative measure and up to three band thresholds. |
| Lollipop | `bar_mark: "lollipop"` on `column` / `horizontal_bar` | Same category/value rows and value domain as a bar; only the mark changes. |
| Dumbbell | `bar_mark: "dumbbell"` on `range_bar` | Same low/high rows as a range bar. |
| Slope | `end_labels` on `xy_line` | A line over two (or more) categories; only the labels change. |
| Bump | `rank` on `xy_line` | The engine ranks ordinary line values, so hosts pass raw values and tooltips keep them. |

`waterfall` binds a band category axis and a linear value axis in either orientation (category X
with value Y, or value X with category Y). Rows are `{ x, y, step?, id?, label?, color? }`. A
`relative` step (the default) adds its signed `y` to the running total and draws from the previous
total to the new one; `subtotal` and `total` steps draw the running total from zero and ignore `y`,
which may be null. A relative step with a null `y` draws no bar and leaves the total unchanged.
Increases default to the market up green, decreases to the market down red, totals
to the series color, and subtotals to the total color; `waterfall: { increase_color, decrease_color,
subtotal_color, total_color, connectors }` overrides them, and a row `color` overrides its bar.
Connectors (default on) are thin lines in the theme text color at 45% opacity from each bar's end
level to the next bar. `data_labels` shows each step's magnitude (subtotals and totals show the
running total). Typed ingestion uses `category_waterfall_columns`: the category columns plus
`steps: Uint8Array` with 0 relative, 1 subtotal and 2 total.

`bullet` binds the same axes. Rows are `{ x, y, target, comparative?, bands?, id?, label?, color? }`
where `y` is the measure. Up to three ascending `bands` thresholds each span from the previous
threshold (or zero) to their own value as full-thickness qualitative bands, darkest toward zero, from
the theme text color. The comparative measure is a half-thickness bar in the series color at half
opacity, the measure a third-thickness bar, and the target a 2 CSS px marker two thirds of the slot
thick, following Stephen Few's bullet graph specification. Typed ingestion uses
`category_bullet_columns`: `target`, optional `comparative`, and optional band-major `bands`
(every row's first threshold, then every second, then every third), each with an optional validity
mask. Neither kind supports stacking or grouping.

`bar_mark: "lollipop"` draws a stem from the baseline to the value with a `point_symbol` of
`point_radius` at the value on an unstacked `column` or `horizontal_bar`; `bar_mark: "dumbbell"`
draws a connector between two symbols at the low and high values of a `range_bar`. Hits measure the
distance to the stem and symbols. Other kinds reject both marks, and stacked lollipops are rejected.

`end_labels: "start" | "end" | "both"` on `xy_line` labels the first and/or last plotted point with
`"<title> <value>"` before the first point and `"<value> <title>"` after the last, in the line color.
All labels on one side of a pane are spread vertically to one label height and a quarter apart and
kept inside the plot, so they never overlap. Labels are not clipped to the outer category padding;
use `band_padding_outer` on the X axis to reserve room for them.

`rank: "dense" | "competition"` on `xy_line` plots each row at its rank among the visible ranked
`xy_line` series that share the pane, X axis and Y axis at the same X datum; rank 1 is the largest
value. Dense ties give 1, 2, 2, 3; competition ties give 1, 2, 2, 4. A Y axis carrying only ranked
lines spans `0.5..N + 0.5` for N ranked series without a zero baseline and runs top-down so rank 1
is at the top; `reverse: true` flips it back. Ranks are recomputed from the current values on every
frame, hit test and snapshot, so updates and visibility changes re-rank immediately.

Tooltip, data and accessibility snapshots carry `detail` for these families: `{ type: "waterfall",
mark, start, end }` with the running total before and after the bar, `{ type: "bullet", target,
comparative, bands }`, or `{ type: "rank", rank }` (the snapshot `value` stays the original value).
Other series report `detail: null`. Accessibility announcements include the step, target, bands or
rank. All six persist in V2 documents with their rows and options.

Path interpolation accepts `step_before` (vertical then horizontal), `step_middle` (turn at the
interval midpoint), and `step_after` (horizontal then vertical). The existing `step` name keeps its
step-after behavior for saved charts. Map Recharts `step`, `stepBefore`, and `stepAfter` to
`step_middle`, `step_before`, and `step_after` respectively.
`monotone` uses a shape-preserving cubic curve along X and holds flat plateaus without overshoot.
`natural` uses a cubic spline with zero endpoint curvature. Its knots pass through the source
points, and its tessellation is bounded by the shared per-interval curve limit.

```ts
interface cartesian_series_options {
  pane: number;
  x_axis_id: string;
  y_axis_id: string;
  visible?: boolean;
  title?: string;
  color?: string;
  /** Draw markers at line, area, or range-area data points; defaults to false. */
  point_markers?: boolean;
  /** Marker shape for scatter and path markers; defaults to circle. */
  point_symbol?: "circle" | "square" | "diamond" | "triangle";
  /** Stroke width for paths, error bars, and box plots in CSS pixels; defaults to 2. */
  line_width?: number;
  /** Portable stroke pattern for line, area, and range-area paths; defaults to solid. */
  line_style?: "solid" | "dotted" | "dashed";
  /** Shared path interpolation for line, area, and range-area boundaries; defaults to linear. */
  interpolation?: "linear" | "step" | "step_before" | "step_middle" | "step_after" | "curved" | "monotone" | "natural";
  /** Bridge missing rows in path series; transform-invalid rows remain gaps. Defaults to false. */
  connect_missing?: boolean;
  /** Area fill opacity from 0 through 1; defaults to 72 / 255. */
  fill_opacity?: number;
  fill_gradient?: readonly [string, string];
  /** Explicit numeric fill baseline for an unstacked `xy_area`. */
  baseline_value?: number;
  /** Baseline for an unstacked `xy_area`; domain bounds follow the effective Y-axis view. */
  baseline_policy?: "zero" | "domain_min" | "domain_max";
  bar_gap?: number;
  bar_max_width?: number;
  bar_corner_radius?: number;
  /** Fix heatmap color-intensity scaling; omitted uses the visible cell values. */
  heatmap_value_domain?: readonly [number, number];
  /** Optional CSS endpoints for the heatmap's shared color ramp. */
  heatmap_low_color?: string;
  heatmap_high_color?: string;
  /** Optional box-plot interquartile fill and median stroke CSS colors. */
  box_fill_color?: string;
  box_median_color?: string;
  /** Lollipop (column/horizontal_bar) or dumbbell (range_bar) marks; defaults to bar. */
  bar_mark?: "bar" | "lollipop" | "dumbbell";
  /** Waterfall step colors and connectors; waterfall series only. */
  waterfall?: { increase_color?: string | null; decrease_color?: string | null;
    subtotal_color?: string | null; total_color?: string | null; connectors?: boolean };
  /** Slope-chart labels at the first/last point of an xy_line; defaults to none. */
  end_labels?: "none" | "start" | "end" | "both";
  /** Bump-chart rank transform of an xy_line; defaults to null (plot values). */
  rank?: "dense" | "competition" | null;
  /** Statistics-family tuning (complete build only; the standard build rejects it with `complete_build_required`); other kinds reject non-default values. */
  stats?: general_stats_options;
  group_id?: string;
  stack_id?: string;
  stack_mode?: "normal" | "cumulative" | "silhouette" | "wiggle" | "percent" | "positive";
}

interface polar_series_options {
  pane: number;
  /** The angle-axis ID is supplied through the shared X-axis field. */
  x_axis_id: string;
  /** The radius-axis ID is supplied through the shared Y-axis field. */
  y_axis_id: string;
  visible?: boolean;
  title?: string;
  color?: string;
  polar?: {
    /** Radians, Recharts convention: 0 at three o'clock, positive counter-clockwise. */
    start_angle?: number; // default 0
    end_angle?: number; // default 2π; below start_angle sweeps clockwise
    inner_radius?: number; // default 0, fraction of the polar plot radius
    outer_radius?: number; // default 0.8 for pie and radial_bar (Recharts 80%), 1 for polar_area
    pad_angle?: number; // default 0
  };
}
```

Polar defaults follow pinned Recharts 3.10.1. A `pie` starts its first slice at three o'clock and
sweeps counter-clockwise (`Pie startAngle=0 endAngle=360`); `start_angle: Math.PI, end_angle: 0`
is the Recharts `startAngle={180} endAngle={0}` top half. A `radial_bar` follows
`RadialBarChart`: each angular category owns one concentric ring of the inner..outer band, in
category order from the inside out (reversed by the angle axis `reverse`), each ring inset by the
default 10% `barCategoryGap`, and its value sweeps the ring from `start_angle` toward `end_angle`
by its fraction of the radius-axis domain, from zero or the domain bound nearest zero. Radial bars
have no background track (Recharts `background={false}`) and ignore `pad_angle`. Polar-area
sectors stay on the angular axis category spokes, which start at twelve o'clock and advance
clockwise; their sweep scales and offsets that layout.

These shapes summarize the common public `general_series_options` fields; kind-specific validation
still applies. Use `null` for a missing row value. Line, scatter, bubble, box-plot, error-bar,
and range data retain that row without drawing a mark; pass numeric zero explicitly when zero
is the intended value.

### Statistics families (complete build)

Twelve statistics kinds ship only in the complete build (`@aeristerminal/aeris-charts/complete`);
the standard build rejects them with `complete_build_required` before any engine state changes.
The standard build rejects the `stats` options object and matrix data (`matrix_row` records and
`general_matrix_columns`) the same way, its `options()` snapshots omit `stats`, and restoring V2
state that contains a statistics series or matrix dataset fails with the same error:

| Kind | Rows | Axes (X × Y) | Mark model |
| --- | --- | --- | --- |
| `violin` | category `{ x, y }` samples | band × linear | Per-category KDE silhouette; `violin_side: "left" \| "right"` on two series forms a split violin |
| `density_curve` | `{ value }` samples | linear × linear | KDE curve over the sample range; `area: false` draws the line only |
| `ridgeline` | category `{ x, y }` samples | linear × band | One KDE ridge per category on the band axis; `overlap` is the ridge height in band units |
| `strip_plot` | category `{ x, y }` samples | band × linear | One point per sample, jittered within `jitter` of the slot |
| `beeswarm` | category `{ x, y }` samples | band × linear | Deterministic non-overlapping point packing per category |
| `dot_plot` | category `{ x, y }` samples | band × linear | Wilkinson dot plot: equal values stack into columns |
| `ecdf` | `{ value }` samples | linear × linear | Empirical cumulative distribution step path, 0 to 1 |
| `qq_plot` | `{ value }` samples | linear × linear | Sample quantiles against the theoretical (normal) distribution |
| `hexbin` | numeric `{ x, y }` rows | linear × linear | d3-hexbin axial-offset hexagons with a count color ramp and legend |
| `contour` | numeric `{ x, y }` rows | linear × linear | Marching-squares density bands and iso-lines over a KDE-smoothed grid |
| `parallel_coordinates` | `matrix_row` records | linear × linear | One polyline per row across N per-field axes; engine brushing filters rows per axis |
| `scatter_matrix` | `matrix_row` records | linear × linear | N×N small-multiples scatter grid with linked cross-cell selection |

Matrix rows are plain records: every non-`id` key is a numeric field, field order follows first
appearance, and every row must carry a finite number for every field (2..8 fields, at most 512
rows). Typed bulk ingestion uses `general_matrix_columns` (`matrix_fields` plus one equal-length
`matrix_columns` array per field); incremental `update_data` by row `id` is supported.

All statistics options live under one optional `stats` object; fields apply per kind as the table
notes and the rest are ignored. A patch replaces the whole `stats` object (like `histogram`), and
non-default values on a non-statistics series are rejected with `invalid_options`:

```ts
interface general_stats_options {
  kernel?: "gaussian" | "epanechnikov"; // KDE kernel; default gaussian
  bandwidth?: "scott" | "silverman" | number; // rule or explicit positive width; default scott
  grid?: number; // KDE grid: 0 = 64, else 8..256 (contour clamps to 96)
  violin_overlay?: "none" | "quartiles" | "box"; // default quartiles
  violin_side?: "full" | "left" | "right"; // default full
  area?: boolean; // density_curve fill; default true
  overlap?: number; // ridgeline ridge height in band units, 0.25..8; default 1
  jitter?: number; // strip_plot jitter fraction of the slot, 0..1; default 0.8
  qq_distribution?: "normal"; // Q-Q theoretical distribution
  thresholds?: number; // contour band count; 0 = 8 automatic levels
  contour_fill?: boolean; // filled bands; default true
  contour_lines?: boolean; // iso-lines; default true
  smooth?: boolean; // contour 2D KDE smoothing vs raw bin counts; default true
  hexbin_radius?: number; // hexagon circumradius in CSS px; 0 = auto within 64 columns, else 4..128
}
```

KDE bandwidths follow Scott's or Silverman's rule of thumb (scaled by the Epanechnikov kernel's
canonical constant) or the explicit positive number. ECDF and Q-Q quantiles use the engine's
shared sorted-sample quantile with Acklam's inverse-normal transform for the normal Q-Q
coordinates. Synthetic marks (violin categories, density/ECDF grid points, hexbin cells, contour
levels) are hit-testable rows with generated IDs; tooltip and accessibility snapshots carry
`detail: { type: "stats", count, min, max }` (the hexbin cell minimum is NaN by design, and its
`max` is the cell count). Parallel-coordinates and scatter-matrix marks are the real dataset
rows. Strip/beeswarm/dot, hexbin, and contour geometry resolves in CSS pixel space, so it
recomputes when the viewport extent changes; the rest are pure data-space models.
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

interface waterfall_row {
  id?: general_row_id;
  x: string;
  y: number | null; // ignored by subtotal and total steps
  step?: "relative" | "subtotal" | "total"; // default relative
  color?: string;
  label?: string;
}

interface bullet_row {
  id?: general_row_id;
  x: string;
  y: number | null; // measure
  target: number | null;
  comparative?: number | null;
  bands?: readonly (number | null)[]; // up to three ascending thresholds
  color?: string;
  label?: string;
}

interface polar_value_row {
  id?: general_row_id;
  x: string; // angular category
  y: number | null; // radial value
  color?: string;
  label?: string;
}
```

Rows with required missing channels stay in the dataset and category/domain union but emit no mark.
Range rows require `low <= high`. Box rows require `min <= q1 <= median <= q3 <= max`. Negative
bubble sizes and negative pie values are invalid. Zero-size bubbles and zero-value sectors are
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

interface general_matrix_columns {
  ids?: readonly general_row_id[];
  /** 2..8 nonempty, unique field names, at most 128 bytes each. */
  matrix_fields: readonly string[];
  /** One Float64Array per field, all with the same row count (at most 512 rows). */
  matrix_columns: readonly Float64Array[];
}
```

All parallel arrays must have equal row counts. Validity arrays contain only `0` or `1`. Category
indices must be in range. The transaction is validated before the live dataset changes. Bubble extends
numeric XY columns with `size: Float64Array` and optional `size_valid: Uint8Array`. Box plots use category
indices plus five parallel `Float64Array` channels named `min`, `q1`, `median`, `q3`, and `max`, each
with an optional validity mask. Histograms use one `values: Float64Array` sample column with an
optional `values_valid` mask. Category heatmaps use independent bounded X/Y category dictionaries and aligned
index columns plus one numeric `value` channel; both dictionaries merge and compact atomically during explicit-ID
updates and bounded retention rather than introducing a second data store. Continuous and temporal heatmaps reuse
the ordinary numeric/temporal X column, add one aligned numeric `y_coordinate` column, and keep `value` in the
existing value/validity channel. Cell extents are inferred deterministically from neighboring coordinate centers.
`parallel_coordinates` and `scatter_matrix` use `general_matrix_columns`: the field columns live outside the
X/Y channels (column 0 mirrors into the value channel for row bookkeeping), every value must be finite, and the
whole matrix dataset replaces or updates atomically by row ID like every other family.

The first release needs `set_data()`, `set_data_typed()`, append/update by explicit row ID, bounded
retention, `data_at()`, hit-test/tooltip snapshots, and capacity telemetry. General storage is
allocated lazily when the first general series or dataset is created; a financial-only chart must
retain zero general-dataset, domain, axis, and geometry capacity.

The current column/histogram/horizontal-bar/box-plot/heatmap-grid/scatter/bubble/`xy_line`/`xy_area`/`range_area`/`error_bar` browser slices expose `update_data(rows, { max_rows })` and
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

Object rows may set `color?: string`; typed columns may provide parallel
`colors?: readonly (string | null)[]`. A color overrides that row's bar, point, error bar,
box, or heatmap mark in the shared frame. Line and area strokes keep their series color while
enabled point markers use row colors. Colors accept the same supported CSS syntax as series colors,
are capped at 256 UTF-8 bytes and 65,536 colored rows per dataset, and survive explicit-ID updates,
bounded retention, and V2 persistence. An updated row without a color clears its prior override.
Point-marking rows may also set `symbol?: "circle" | "square" | "diamond" | "triangle"`, or typed
columns may supply a parallel `symbols` array. Scatter and enabled line/area point markers paint
and hit-test the row symbol; bubbles retain their size-scaled circle. A missing symbol falls back
to the series symbol. Invalid symbols reject the full row transaction atomically, and V2 preserves
explicit symbols.

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
For a stack, it also determines which member starts at zero and which member occupies the exposed end.

Reference components are intentionally separate from series data. A reference line binds one X or Y axis and one
compatible numeric/temporal/category value; a dot binds explicit X and Y axes; a region binds
explicit X and Y axes with any nonempty set of four optional endpoints.
Omitted endpoints use the corresponding plot edge through pan, zoom, and resize.
Styling is bounded engine-owned state. `extend_domain: false` is the default semantic: the reference is
drawn only where its value maps into the current domain. `extend_domain: true` contributes each declared reference
coordinate to automatic domain resolution. Removing an axis used by a live reference is rejected until the
reference is removed.

Brush state is transient and is not serialized in V2. It is interaction state like hover/selection rather than
chart configuration. The snapshot contains at most the engine's bounded brush item limit and preserves
series/row ordering. Shared tooltips are derived snapshots and likewise add no retained host-side registry.

## Pane compatibility matrix

`same region` below means the series can contribute to one coordinate region and ordered frame.
Different Y axes are allowed within that region when their scale types accept the series values.

| Family | Financial time | Continuous X | Temporal X | Category X | Polar |
| --- | --- | --- | --- | --- | --- |
| Candlestick, financial bar, histogram, baseline, footprint, indicators | same region | incompatible | incompatible | incompatible | incompatible |
| `xy_line`, `xy_area`, `range_area` | incompatible | same region | same region | same region with point/band axis | incompatible |
| `column` | incompatible | same region | same region | same region | incompatible |
| `histogram` (linear X and Y) | incompatible | same region | incompatible | incompatible | incompatible |
| `horizontal_bar` (numeric X, category Y) | incompatible | same region | incompatible | incompatible | incompatible |
| `scatter`, `bubble`, `error_bar` | incompatible | same region | same region | same region with point axis | incompatible |
| `heatmap_grid`, `box_plot` | incompatible | same region when both axes agree | same region when axes agree | same region | incompatible |
| `violin`, `ridgeline`, `strip_plot`, `beeswarm`, `dot_plot` (band category axis) | incompatible | incompatible | incompatible | same region with band axis | incompatible |
| `density_curve`, `ecdf`, `qq_plot`, `hexbin`, `contour`, `parallel_coordinates`, `scatter_matrix` (linear X and Y) | incompatible | same region | incompatible | incompatible | incompatible |
| Pie (including donut), radar, radial bar, polar area | incompatible | incompatible | incompatible | incompatible | same polar region |

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
- Incompatible additions fail with `invalid_options`; Aeris never moves the series implicitly or
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
- persistence under the general-chart V2 schema;
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
recorded financial baseline.
