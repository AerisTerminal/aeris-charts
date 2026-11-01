// Marimekko (variable-width 100% stacked columns). ECharts 6.1.0 has no Marimekko series; the
// documented recipe is one `custom` series per segment whose `renderItem` paints rectangles in a
// 0-100 by 0-100 value grid, which also gives the per-segment legend.
export const cells = [
  { column: "North America", segment: "Equities", value: 52 }, { column: "North America", segment: "Bonds", value: 31 },
  { column: "North America", segment: "Cash", value: 9 },
  { column: "Europe", segment: "Equities", value: 28 }, { column: "Europe", segment: "Bonds", value: 30 },
  { column: "Europe", segment: "Cash", value: 7 },
  { column: "Asia", segment: "Equities", value: 34 }, { column: "Asia", segment: "Bonds", value: 12 },
  { column: "Asia", segment: "Cash", value: 6 },
  { column: "Latin America", segment: "Equities", value: 9 }, { column: "Latin America", segment: "Bonds", value: 6 },
  { column: "Latin America", segment: "Cash", value: 3 },
];
const columns = [...new Set(cells.map((cell) => cell.column))];
const segments = [...new Set(cells.map((cell) => cell.segment))];
const total = cells.reduce((sum, cell) => sum + cell.value, 0);
const column_total = (column) => cells.filter((cell) => cell.column === column)
  .reduce((sum, cell) => sum + cell.value, 0);

// [x0, x1, y0, y1] in percent for every cell, segments stacked upward in first-seen order.
export const rects = {};
let x = 0;
for (const column of columns) {
  const width = column_total(column) / total * 100;
  let y = 0;
  for (const segment of segments) {
    const cell = cells.find((entry) => entry.column === column && entry.segment === segment);
    if (cell === undefined) continue;
    const height = cell.value / column_total(column) * 100;
    rects[`${column}/${segment}`] = [x, x + width, y, y + height];
    y += height;
  }
  x += width;
}

const render = (params, api) => {
  const [x0, y0] = api.coord([api.value(0), api.value(2)]);
  const [x1, y1] = api.coord([api.value(1), api.value(3)]);
  return { type: "rect", shape: { x: x0, y: y1, width: x1 - x0, height: y0 - y1 },
    style: { fill: api.visual("color"), stroke: "#ffffff", lineWidth: 1 } };
};

export default {
  id: "marimekko",
  title: "Marimekko",
  charts: [{
    id: "marimekko",
    title: "custom series per segment with a segment legend",
    width: 760,
    height: 320,
    option: {
      legend: { top: 0, data: segments },
      grid: { left: 8, right: 8, top: 32, bottom: 24 },
      xAxis: { type: "value", min: 0, max: 100, show: false },
      yAxis: { type: "value", min: 0, max: 100, show: false },
      series: segments.map((segment) => ({
        type: "custom", name: segment, renderItem: render, encode: { x: [0, 1], y: [2, 3] },
        data: columns.flatMap((column) => {
          const rect = rects[`${column}/${segment}`];
          return rect === undefined ? [] : [{ name: column, value: rect }];
        }),
      })),
    },
  }],
};
