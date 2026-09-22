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
      data_labels: true,
    });
    columns.set_data([
      { id: "jan", x: "Jan", y: 42, label: "January" },
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
      data_labels: true,
    });
    scatter.set_data_typed({
      ids: [101, 102, 103],
      labels: [null, "Center", null],
      x: new Float64Array([-10, 0, 10]),
      y: new Float64Array([-5, 0, 5]),
    });

    chart.resize(640, 480, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));

    let hit = null;
    let hit_point = null;
    const geometry = scatter_pane.get_geometry();
    for (let y = geometry.top + 6; y <= geometry.top + geometry.height - 6 && hit === null; y += 2) {
      for (let x = 0; x <= geometry.width; x += 2) {
        const candidate = chart.general_hit_test(scatter_pane.pane_index(), x, y);
        if (candidate?.series === scatter.id) {
          hit = candidate;
          hit_point = { x, y };
          break;
        }
      }
    }

    let hover_event = null;
    let click_event = null;
    chart.subscribe_crosshair_move((params) => {
      if (params.general_hit !== null) {
        hover_event = { hit: params.general_hit, kind: params.hovered_series?.kind ?? null };
      }
    });
    chart.subscribe_click((params) => {
      click_event = params.general_hit;
    });
    const overlay = host.querySelectorAll("canvas")[3];
    const browser_x = hit_point.x + geometry.left;
    overlay.dispatchEvent(new PointerEvent("pointermove", {
      clientX: browser_x,
      clientY: hit_point.y,
      pointerId: 1,
      pointerType: "mouse",
      isPrimary: true,
      buttons: 0,
      bubbles: true,
    }));
    overlay.dispatchEvent(new MouseEvent("click", {
      clientX: browser_x,
      clientY: hit_point.y,
      button: 0,
      bubbles: true,
    }));
    await new Promise((resolve) => requestAnimationFrame(resolve));

    x_axis.zoom(2, 0);
    x_axis.pan(0.25);
    x_axis.reset_view();
    const blocked_axis_remove = chart.remove_axis("sample-x");
    const before_remove = {
      panes: chart.panes().length,
      axes: chart.axes().map((axis) => axis.id),
      column_row: columns.data_at(0),
      column_missing: columns.data_at(1),
      scatter_row: scatter.data_at(2),
      scatter_accessibility: scatter.accessibility_snapshot(1, 2),
      hit,
      hover_event,
      click_event,
      selected_hit: scatter.selected_hit(),
      chart_selected_hit: chart.general_selected_hit(),
      category_series: category_pane.get_series().map((series) => series.kind ?? series.series_type()),
      scatter_series: scatter_pane.get_series().map((series) => series.kind ?? series.series_type()),
      screenshot: chart.take_screenshot().toDataURL().length,
      blocked_axis_remove,
    };

    const before_custom_label = chart.take_screenshot().toDataURL();
    scatter.update_data_typed({
      ids: [102],
      labels: ["Zero marker"],
      x: new Float64Array([0]),
      y: new Float64Array([0]),
    });
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const custom_label_changed = chart.take_screenshot().toDataURL() !== before_custom_label;
    const updated_custom_label = scatter.data_at(1)?.label;
    let rejected_label = null;
    try {
      scatter.update_data_typed({
        ids: [102],
        labels: ["x".repeat(4097)],
        x: new Float64Array([0]),
        y: new Float64Array([99]),
      });
    } catch (error) {
      rejected_label = error.code;
    }
    const after_rejected_label = scatter.data_at(1);

    let rejected_update = null;
    try {
      scatter.update_data([
        { id: 105, x: 12, y: 7 },
        { id: 105, x: 13, y: 8 },
      ]);
    } catch (error) {
      rejected_update = error.code;
    }
    const after_rejected_update = scatter.accessibility_snapshot(0, 10);
    scatter.update_data_typed({
      ids: [102, 104],
      x: new Float64Array([0, 11]),
      y: new Float64Array([7, 6]),
    }, { max_rows: 3 });
    columns.update_data([
      { id: "jan", x: "Jan", y: 43 },
      { id: "apr", x: "Apr", y: 9, label: "April" },
    ], { max_rows: 3 });
    const after_update = {
      scatter: scatter.accessibility_snapshot(0, 10),
      columns: columns.accessibility_snapshot(0, 10),
      selected: scatter.selected_hit(),
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
    return { before_remove, custom_label_changed, updated_custom_label, rejected_label, after_rejected_label, rejected_update, after_rejected_update, after_update, removed_axis, remaining_panes, lifecycle };
  });

  expect(result.before_remove.panes).toBe(3);
  expect(result.before_remove.axes).toEqual(["month", "revenue", "sample-x", "sample-y"]);
  expect(result.before_remove.column_row).toMatchObject({ row_id: "jan", label: "January", value: 42 });
  expect(result.before_remove.column_missing).toMatchObject({ row_id: { generated: "1" }, x_label: "Feb", value: null });
  expect(result.before_remove.scatter_row).toMatchObject({ row_id: 103, x_label: "10", value: 5 });
  expect(result.before_remove.scatter_accessibility).toMatchObject({
    total_rows: 3,
    offset: 1,
    items: [
      { row: 1, row_id: 102, x_label: "0", label: "Center", value: 0 },
      { row: 2, row_id: 103, x_label: "10", value: 5 },
    ],
  });
  expect(result.before_remove.hit).toMatchObject({ series: expect.any(Number), row_id: expect.any(Number) });
  expect(result.before_remove.hover_event).toMatchObject({
    hit: { series: expect.any(Number), row_id: expect.any(Number) },
    kind: "scatter",
  });
  expect(result.before_remove.click_event).toMatchObject({ series: expect.any(Number), row_id: expect.any(Number) });
  expect(result.before_remove.selected_hit).toEqual(result.before_remove.chart_selected_hit);
  expect(result.before_remove.selected_hit).toMatchObject({ series: expect.any(Number), row_id: expect.any(Number) });
  expect(result.before_remove.category_series).toEqual(["column"]);
  expect(result.before_remove.scatter_series).toEqual(["scatter"]);
  expect(result.before_remove.screenshot).toBeGreaterThan(1000);
  expect(result.before_remove.blocked_axis_remove).toBe(false);
  expect(result.custom_label_changed).toBe(true);
  expect(result.updated_custom_label).toBe("Zero marker");
  expect(result.rejected_label).toBe("resource_limit");
  expect(result.after_rejected_label).toMatchObject({ row_id: 102, value: 0, label: "Zero marker" });
  expect(result.rejected_update).toBe("invalid_data");
  expect(result.after_rejected_update.total_rows).toBe(3);
  expect(result.after_rejected_update.items.map((item) => item.row_id)).toEqual([101, 102, 103]);
  expect(result.after_update.scatter.items).toMatchObject([
    { row_id: 102, x_label: "0", label: null, value: 7 },
    { row_id: 103, x_label: "10", value: 5 },
    { row_id: 104, x_label: "11", value: 6 },
  ]);
  if (result.before_remove.selected_hit.row_id === 101) {
    expect(result.after_update.selected).toBeNull();
  } else {
    expect(result.after_update.selected).toMatchObject({
      row_id: result.before_remove.selected_hit.row_id,
      row: result.before_remove.selected_hit.row - 1,
    });
  }
  expect(result.after_update.columns.items).toMatchObject([
    { x_label: "Feb", value: null },
    { row_id: "mar", x_label: "Mar", value: -17 },
    { row_id: "apr", x_label: "Apr", label: "April", value: 9 },
  ]);
  expect(result.removed_axis).toBe(true);
  expect(result.remaining_panes).toBe(1);
  expect(result.lifecycle).toEqual([
    ["added", "column", 1],
    ["added", "scatter", 2],
    ["removed", "scatter", 2],
    ["removed", "column", 1],
  ]);
});
