// Tree: a three-level market hierarchy with the pinned ECharts 6.1.0 `tree` series defaults
// (orthogonal left-to-right layout, 12% margins, 7px hollow symbols, #ccc 1.5px curved links,
// parents labeled before their node and leaves after it). Aeris lays the same nodes out with the
// d3-hierarchy 3.1.2 `tree()` algorithm; the radial chart is ECharts' `layout: "radial"`.
export const nodes = [
  { id: "markets", parent: null, label: "Markets", value: null },
  { id: "equities", parent: "markets", label: "Equities", value: null },
  { id: "rates", parent: "markets", label: "Rates", value: null },
  { id: "fx", parent: "markets", label: "FX", value: null },
  { id: "us_eq", parent: "equities", label: "US", value: 42 },
  { id: "eu_eq", parent: "equities", label: "Europe", value: 24 },
  { id: "asia_eq", parent: "equities", label: "Asia", value: 18 },
  { id: "ust", parent: "rates", label: "Treasuries", value: 30 },
  { id: "bund", parent: "rates", label: "Bunds", value: 14 },
  { id: "eurusd", parent: "fx", label: "EUR/USD", value: 16 },
  { id: "usdjpy", parent: "fx", label: "USD/JPY", value: 11 },
  { id: "gbpusd", parent: "fx", label: "GBP/USD", value: 7 },
];
export const series_color = "#5070dd";

/** The flat rows as the nested `{ name, value, children }` data ECharts tree series take. */
export function nested(rows) {
  const build = (row) => {
    const children = rows.filter((child) => child.parent === row.id).map(build);
    return { name: row.label, value: row.value ?? undefined, ...(children.length > 0 ? { children } : {}) };
  };
  return build(rows.find((row) => row.parent === null));
}

const tree_series = (layout) => ({
  type: "tree",
  data: [nested(nodes)],
  layout,
  initialTreeDepth: -1,
  itemStyle: { borderColor: series_color },
});

export default {
  id: "tree",
  title: "Tree",
  charts: [
    {
      id: "tree",
      title: "tree series, orthogonal left-to-right",
      width: 360,
      height: 300,
      option: { series: [tree_series("orthogonal")] },
    },
    {
      id: "tree_radial",
      title: "tree series, radial",
      width: 360,
      height: 300,
      option: { series: [tree_series("radial")] },
    },
  ],
};
