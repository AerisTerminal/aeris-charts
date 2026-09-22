import { test, expect } from "@playwright/test";

test("public category-column and XY-scatter slices share the chart lifecycle", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/nucleuscharts_financial.js");
    const host = document.createElement("div");
    Object.assign(host.style, {
      position: "fixed",
      width: "640px",
      height: "480px",
      left: "0",
      top: "0",
      zIndex: "10000",
    });
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(640, 480, 1);
    const lifecycle = [];
    chart.subscribe_series_added(({ series, pane_index }) => lifecycle.push(["added", series.kind ?? series.series_type(), pane_index]));
    chart.subscribe_series_removed(({ series, pane_index }) => lifecycle.push(["removed", series.kind ?? series.series_type(), pane_index]));

    const category_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "category", scale: "band" },
    });
    chart.add_axis({ id: "month", pane: category_pane.pane_index(), dimension: "x", scale: "band" });
    chart.add_axis({ id: "revenue", pane: category_pane.pane_index(), dimension: "y", scale: "linear" });
    const columns = chart.add_series("column", {
      pane: category_pane.pane_index(),
      x_axis_id: "month",
      y_axis_id: "revenue",
      title: "Revenue",
      color: "#267f99",
    });
    columns.set_data([
      { id: "jan", x: "Jan", y: 42 },
      { x: "Feb", y: null },
      { id: "mar", x: "Mar", y: -17 },
    ]);

    const scatter_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" },
    });
    const x_axis = chart.add_axis({ id: "sample-x", pane: scatter_pane.pane_index(), dimension: "x", scale: "linear" });
    chart.add_axis({ id: "sample-y", pane: scatter_pane.pane_index(), dimension: "y", scale: "symlog" });
    const scatter = chart.add_series("scatter", {
      pane: scatter_pane.pane_index(),
      x_axis_id: "sample-x",
      y_axis_id: "sample-y",
      point_radius: 5,
      title: "Samples",
      color: "#7d52f4",
    });
    scatter.set_data_typed({
      ids: [101, 102, 103],
      x: new Float64Array([-10, 0, 10]),
      y: new Float64Array([-5, 0, 5]),
    });

    chart.resize(640, 480, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));

    let hit = null;
    const geometry = scatter_pane.get_geometry();
    for (let y = geometry.top; y <= geometry.top + geometry.height && hit === null; y += 2) {
      for (let x = geometry.left; x <= geometry.left + geometry.width; x += 2) {
        const candidate = chart.general_hit_test(scatter_pane.pane_index(), x, y, 2);
        if (candidate?.series === scatter.id) {
          hit = candidate;
          break;
        }
      }
    }

    x_axis.zoom(2, 0);
    x_axis.pan(0.25);
    x_axis.reset_view();
    const blocked_axis_remove = chart.remove_axis("sample-x");
    const before_remove = {
      panes: chart.panes().length,
      axes: chart.axes().map((axis) => axis.id),
      column_missing: columns.data_at(1),
      scatter_row: scatter.data_at(2),
      scatter_accessibility: scatter.accessibility_snapshot(1, 2),
      hit,
      category_series: category_pane.get_series().map((series) => series.kind ?? series.series_type()),
      scatter_series: scatter_pane.get_series().map((series) => series.kind ?? series.series_type()),
      screenshot: chart.take_screenshot().toDataURL().length,
      blocked_axis_remove,
    };

    scatter.remove();
    const removed_axis = x_axis.remove();
    chart.remove_axis("sample-y");
    chart.remove_pane(scatter_pane.pane_index());
    columns.remove();
    chart.remove_axis("month");
    chart.remove_axis("revenue");
    chart.remove_pane(category_pane.pane_index());
    const remaining_panes = chart.panes().length;
    chart.remove();
    host.remove();
    return { before_remove, removed_axis, remaining_panes, lifecycle };
  });

  expect(result.before_remove.panes).toBe(3);
  expect(result.before_remove.axes).toEqual(["month", "revenue", "sample-x", "sample-y"]);
  expect(result.before_remove.column_missing).toMatchObject({ row_id: { generated: "1" }, x_label: "Feb", value: null });
  expect(result.before_remove.scatter_row).toMatchObject({ row_id: 103, x_label: "10", value: 5 });
  expect(result.before_remove.scatter_accessibility).toMatchObject({
    total_rows: 3,
    offset: 1,
    items: [
      { row: 1, row_id: 102, x_label: "0", value: 0 },
      { row: 2, row_id: 103, x_label: "10", value: 5 },
    ],
  });
  expect(result.before_remove.hit).toMatchObject({ series: expect.any(Number), row_id: expect.any(Number) });
  expect(result.before_remove.category_series).toEqual(["column"]);
  expect(result.before_remove.scatter_series).toEqual(["scatter"]);
  expect(result.before_remove.screenshot).toBeGreaterThan(1000);
  expect(result.before_remove.blocked_axis_remove).toBe(false);
  expect(result.removed_axis).toBe(true);
  expect(result.remaining_panes).toBe(1);
  expect(result.lifecycle).toEqual([
    ["added", "column", 1],
    ["added", "scatter", 2],
    ["removed", "scatter", 2],
    ["removed", "column", 1],
  ]);
});
