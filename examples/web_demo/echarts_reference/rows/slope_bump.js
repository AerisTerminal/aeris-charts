// Slope and bump charts. ECharts 6.1.0 draws both with line series: a slope chart labels the first
// and last points and resolves label overlap with `labelLayout.moveOverlap`; a bump chart plots
// precomputed ranks on an inverted value axis. Ranks here use the dense tie policy (1, 2, 2, 3)
// with rank 1 for the largest value at each period.
export const slope = [
  { title: "Search", color: "#4c8bf5", values: [48, 61] },
  { title: "Social", color: "#28b7a4", values: [44, 39] },
  { title: "Email", color: "#e4a11b", values: [31, 42] },
  { title: "Direct", color: "#e46f61", values: [26, 24] },
];
export const periods = ["2024", "2025"];
export const quarters = ["Q1", "Q2", "Q3", "Q4", "Q5"];
export const bump = [
  { title: "Aeris", color: "#4c8bf5", values: [18, 24, 31, 36, 41] },
  { title: "Nimbus", color: "#28b7a4", values: [26, 27, 25, 29, 33] },
  { title: "Vector", color: "#e4a11b", values: [31, 22, 28, 24, 21] },
  { title: "Orbit", color: "#e46f61", values: [12, 15, 19, 17, 26] },
];

export function dense_ranks(series) {
  return series.map((line) => line.values.map((value, period) => {
    const larger = new Set(series.map((other) => other.values[period]).filter((other) => other > value));
    return larger.size + 1;
  }));
}
export const ranks = dense_ranks(bump);

export default {
  id: "slope_bump",
  title: "Slope and bump",
  charts: [
    {
      id: "slope",
      title: "line series with first/last labels and moveOverlap",
      width: 520,
      height: 300,
      option: {
        grid: { left: 110, right: 110, top: 16, bottom: 28 },
        xAxis: { type: "category", data: periods, boundaryGap: false },
        yAxis: { type: "value", scale: true },
        labelLayout: { moveOverlap: "shiftY" },
        series: slope.map((line) => ({
          type: "line", name: line.title, color: line.color, symbolSize: 8,
          data: line.values.map((value, index) => ({
            value,
            label: {
              show: true, position: index === 0 ? "left" : "right", color: line.color,
              formatter: index === 0 ? `${line.title} ${value}` : `${value} ${line.title}`,
            },
          })),
        })),
      },
    },
    {
      id: "bump",
      title: "line series of dense ranks on an inverted axis",
      width: 520,
      height: 300,
      option: {
        grid: { left: 48, right: 16, top: 16, bottom: 28 },
        xAxis: { type: "category", data: quarters, boundaryGap: false },
        yAxis: { type: "value", inverse: true, min: 0.5, max: bump.length + 0.5, interval: 1 },
        series: bump.map((line, index) => ({
          type: "line", name: line.title, color: line.color, symbolSize: 10, data: ranks[index],
        })),
      },
    },
  ],
};
