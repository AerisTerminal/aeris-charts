import { createChart } from "@aeristerminal/aeris-charts";

const leftHost = document.querySelector("#left-chart");
const rightHost = document.querySelector("#right-chart");
if (!(leftHost instanceof HTMLElement) || !(rightHost instanceof HTMLElement)) {
  throw new Error("missing #left-chart or #right-chart container");
}

async function categoryChart(host, categories) {
  const chart = await createChart(host, {
    autoSize: true,
    initialPane: { horizontal_domain: { type: "category", scale: "band" } },
  });
  chart.add_axis({ id: "period", pane: 0, dimension: "x", scale: "band", domain: categories });
  chart.add_axis({ id: "value", pane: 0, dimension: "y", scale: "linear", domain: [0, 10] });
  const series = chart.add_series("column", {
    pane: 0, x_axis_id: "period", y_axis_id: "value", title: "Value",
  });
  series.set_data(categories.map((x, index) => ({ id: x, x, y: index + 1 })));
  return chart;
}

const left = await categoryChart(leftHost, ["A", "B", "C", "D"]);
const right = await categoryChart(rightHost, ["A", "X", "B", "C", "Y", "D"]);

// Cross-chart transport is host-owned. Each receiving engine resolves the semantic values
// against its own axes and suppresses echoes and stale source revisions.
const relay = (from, to, source) => {
  for (const event of from.take_sync_events()) {
    to.apply_external_sync_event({ ...event, source });
  }
};
const leftMoved = () => relay(left, right, "left-chart");
const rightMoved = () => relay(right, left, "right-chart");
left.subscribe_crosshair_move(leftMoved);
right.subscribe_crosshair_move(rightMoved);

export function zoomLeft(factor, anchorCategory) {
  left.axis("period").zoom(factor, anchorCategory);
  relay(left, right, "left-chart");
}

export function dispose() {
  left.unsubscribe_crosshair_move(leftMoved);
  right.unsubscribe_crosshair_move(rightMoved);
  left.remove();
  right.remove();
}
