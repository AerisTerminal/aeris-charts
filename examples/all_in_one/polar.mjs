import { createChart } from "@aeristerminal/aeris-charts";

const container = document.querySelector("#chart");
if (!(container instanceof HTMLElement)) throw new Error("missing #chart container");

// A donut is a pie series with a nonzero inner radius. The first pane is polar, so no
// empty financial pane consumes space in this standalone chart.
const chart = await createChart(container, {
  autoSize: true,
  initialPane: { horizontal_domain: { type: "polar" } },
  generalLegend: { visible: true, position: "bottom" },
});
chart.add_axis({ id: "angle", pane: 0, dimension: "angle", scale: "angular_category", visible: false });
chart.add_axis({ id: "radius", pane: 0, dimension: "radius", scale: "radial_linear", visible: false });
const share = chart.add_series("pie", {
  pane: 0,
  x_axis_id: "angle",
  y_axis_id: "radius",
  title: "Regional share",
  polar: { inner_radius: 0.35, outer_radius: 0.9 },
  data_labels: true,
});
share.set_data([
  { id: "north", x: "North", y: 42, color: "#4c8bf5", label: "42" },
  { id: "south", x: "South", y: 28, color: "#28b7a4", label: "28" },
  { id: "east", x: "East", y: 19, color: "#8b6df6", label: "19" },
  { id: "west", x: "West", y: 11, color: "#e4a11b", label: "11" },
]);
