// Lollipop and dumbbell. ECharts 6.1.0 composes a lollipop from a 2 px bar stem and a scatter
// head, and a dumbbell from a transparent stacked base, a 2 px connector bar and scatter ends.
export const lollipop = [
  { x: "Search", y: 42 }, { x: "Billing", y: 18 }, { x: "Exports", y: -6 },
  { x: "Alerts", y: 31 }, { x: "Sharing", y: 24 }, { x: "Mobile", y: 12 },
];
export const dumbbell = [
  { x: "Brazil", low: 66, high: 76 }, { x: "China", low: 69, high: 78 },
  { x: "India", low: 58, high: 70 }, { x: "Nigeria", low: 46, high: 55 }, { x: "Peru", low: 66, high: 77 },
];
export const lollipop_color = "#28b7a4";
export const dumbbell_color = "#e4a11b";
export const head_radius = 6;

export default {
  id: "lollipop_dumbbell",
  title: "Lollipop and dumbbell",
  charts: [
    {
      id: "lollipop",
      title: "bar stem plus scatter head",
      width: 520,
      height: 300,
      option: {
        grid: { left: 48, right: 16, top: 16, bottom: 28 },
        xAxis: { type: "category", data: lollipop.map((row) => row.x) },
        yAxis: { type: "value" },
        color: [lollipop_color],
        series: [
          { type: "bar", barWidth: 2, data: lollipop.map((row) => row.y) },
          { type: "scatter", symbolSize: head_radius * 2, data: lollipop.map((row) => row.y) },
        ],
      },
    },
    {
      id: "dumbbell",
      title: "stacked connector plus scatter ends",
      width: 520,
      height: 300,
      option: {
        grid: { left: 48, right: 16, top: 16, bottom: 28 },
        xAxis: { type: "category", data: dumbbell.map((row) => row.x) },
        yAxis: { type: "value", min: 40, max: 80 },
        color: [dumbbell_color],
        series: [
          {
            type: "bar", stack: "range", silent: true, barWidth: 2,
            itemStyle: { color: "transparent" }, data: dumbbell.map((row) => row.low),
          },
          { type: "bar", stack: "range", barWidth: 2, data: dumbbell.map((row) => row.high - row.low) },
          { type: "scatter", symbolSize: head_radius * 2, data: dumbbell.map((row) => row.low) },
          { type: "scatter", symbolSize: head_radius * 2, data: dumbbell.map((row) => row.high) },
        ],
      },
    },
  ],
};
