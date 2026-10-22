import { createChart } from "@aeristerminal/aeris-charts";

const container = document.querySelector("#chart");
if (!(container instanceof HTMLElement)) throw new Error("missing #chart container");

// Recharts Sankey uses node indexes in links; Aeris uses stable node IDs so a data update
// can reorder nodes without silently redirecting a link.
const chart = await createChart(container, {
  autoSize: true,
  initialPane: { horizontal_domain: { type: "layout" } },
});
const flow = chart.add_layout_series("sankey", {
  pane: 0,
  title: "Conversion flow",
  color: "#4c8bf5",
  data_labels: true,
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

// Retain the series handle for flow.set_data(nextFlow) and dispose the chart with its view.
