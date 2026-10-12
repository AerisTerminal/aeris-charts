import { createChart, create_general_legend, create_general_tooltip } from "@aeristerminal/aeris-charts";

const container = document.querySelector("#chart");
if (!(container instanceof HTMLElement)) throw new Error("missing #chart container");

const chart = await createChart(container, {
  autoSize: true,
  initialPane: { horizontal_domain: { type: "category", scale: "band" } },
});
chart.add_axis({ id: "quarter", pane: 0, dimension: "x", scale: "band", title: "Quarter" });
chart.add_axis({ id: "sales", pane: 0, dimension: "y", scale: "linear", title: "Sales" });
const sales = chart.add_series("column", {
  pane: 0,
  x_axis_id: "quarter",
  y_axis_id: "sales",
  title: "Sales",
  data_labels: true,
});
sales.set_data([
  { id: "q1", x: "Q1", y: 12 },
  { id: "q2", x: "Q2", y: 18 },
  { id: "q3", x: "Q3", y: 15 },
]);

const legendHost = document.createElement("div");
legendHost.style.cssText = "position:absolute;top:8px;left:8px;z-index:6";
container.appendChild(legendHost);
create_general_legend(chart, legendHost, { pane: 0 });
create_general_tooltip(chart, container, { pane: 0 });
