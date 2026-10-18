# General chart authoring

The root package entry is framework neutral. The optional `/react` entry owns chart and component
lifecycle over the same engine handles. Both use the shared frame, input controller, persistence,
and renderers described in [architecture.md](architecture.md).

## React composition

`GeneralPane` accepts the existing `axes` and `series` arrays, or direct JSX declarations inside
the pane. Fragments and conditional children are supported. `GeneralAxis`, `GeneralSeries`,
`MappedGeneralSeries`, `GeneralDataLabels`, and `GeneralReference` are declarations; they do not
draw React elements. `GeneralLegend`, `GeneralTooltip`, and `GeneralBrushControls` mount the
package's accessible default DOM controls. Arbitrary wrapper components cannot stand in for a
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
  return <AerisChart options={{ initialPane: { horizontal_domain: { type: "category", scale: "band" } } }}
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
| `ResponsiveContainer` with `ComposedChart` | `AerisChart` with one or more `GeneralPane` declarations |
| `XAxis` / `YAxis` | `GeneralAxis` with explicit dimension, scale, and axis ID |
| `dataKey` on a series | `MappedGeneralSeries` with a typed `mapRow`, or canonical `GeneralSeries` data |
| `LabelList` | `GeneralDataLabels` plus portable row `label` values |
| `ReferenceLine` / `ReferenceDot` / `ReferenceArea` | `GeneralReference` line / dot / region |
| `Legend` / `Tooltip` / range selection UI | `GeneralLegend` / `GeneralTooltip` / `GeneralBrushControls` |

Aeris uses an explicit pane domain and typed axis values. Financial timestamps are UTC seconds;
general temporal axes use epoch milliseconds. Missing rows, duplicate values, and stable row IDs
follow the engine's per-family rules. Translate source records once at the application boundary,
then update the retained series handle; do not run data accessors inside chart rendering.

## Browser and server setup

The package exports JavaScript, declarations, design CSS, and a sibling WASM asset. Bundlers
should preserve the package's `import.meta.url` asset resolution or use the explicit `/wasm`
export when configuring asset copying. The package smoke test bundles all four checked-in
consumer examples from an installed npm tarball. The React entry is safe to import during server
rendering: it performs no DOM work until mount. It does not produce a static chart image on the
server; the browser initializes the chart after hydration. Call `chart.remove()` for imperative
owners and detach default DOM component handles when unmounting. React owns those cleanup steps
for its declarations.
