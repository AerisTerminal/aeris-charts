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
  return <AerisChart style={{ width: "100%", height: 400 }}>
    <GeneralPane options={{ horizontal_domain: { type: "category", scale: "band" } }}>
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
Framework-neutral callers can use `validate_general_pane_proposal(...)` to dry-run a complete pane
graph. The engine validates proposed axes, packed series data, bindings, and references without
mutating chart state. `GeneralPane` uses this operation before removing dependent objects. Pass `onValidationError`
to report a mapper failure or rejected definition while retaining the last
committed pane; without it, the error reaches the nearest React error boundary.

Row `id` values make focus and selection survive replacement and reorder. A row's `label` is the
portable text for `GeneralDataLabels`; a React formatter cannot run inside native or WebGPU
rendering. `GeneralPane` also accepts keyed `axes`, `series`, and `references` arrays for existing
callers. The complete mixed financial/general example is
[react.tsx](../examples/all_in_one/react.tsx).

## Brush state and linked charts

`GeneralBrushControls` is uncontrolled when `range` is omitted. `defaultRange` sets its initial
semantic range. `onChange` reports a bounded engine snapshot after changes. Supplying
`range` makes it controlled: the component applies that value, reports attempted changes, and
keeps the current parent value until a new value is rendered. `null` clears the range. Numeric,
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

`add_pane`, `add_axis`, `add_series`, `set_data`, `add_general_reference`, and
`set_general_brush_range` own the same engine objects that React declares. The default controls
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
