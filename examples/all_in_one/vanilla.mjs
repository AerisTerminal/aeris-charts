import { createChart } from "aeris-charts";

const container = document.querySelector("#chart");
if (!(container instanceof HTMLElement)) throw new Error("missing #chart container");

const chart = await createChart(container, { autoSize: true });

const candles = chart.addSeries("candlestick", { title: "NCL" });
candles.setData([
  { time: 1735689600, open: 100, high: 108, low: 98, close: 105 },
  { time: 1735776000, open: 105, high: 112, low: 103, close: 110 },
]);

const summaryPane = chart.addPane({
  preserve_empty: true,
  horizontal_domain: { type: "category", scale: "band" },
});
const summaryPaneIndex = summaryPane.paneIndex();
chart.addAxis({ id: "month", pane: summaryPaneIndex, dimension: "x", scale: "band" });
chart.addAxis({ id: "revenue", pane: summaryPaneIndex, dimension: "y", scale: "linear" });

const revenue = chart.addSeries("column", {
  pane: summaryPaneIndex,
  x_axis_id: "month",
  y_axis_id: "revenue",
  title: "Revenue",
});
revenue.setData([
  { id: "jan", x: "Jan", y: 42 },
  { id: "feb", x: "Feb", y: 57 },
  { id: "mar", x: "Mar", y: 51 },
]);

chart.timeScale().fitContent();
