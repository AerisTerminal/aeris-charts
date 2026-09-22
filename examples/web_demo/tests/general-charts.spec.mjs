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
    const replacement_x_axis = chart.add_axis({
      id: "sample-x",
      pane: scatter_pane.pane_index(),
      dimension: "x",
      scale: "linear",
      title: "Replacement X",
    });
    let stale_axis_code = null;
    try { x_axis.options(); } catch (error) { stale_axis_code = error.code; }
    const replacement_axis_title = replacement_x_axis.options().title;
    replacement_x_axis.remove();
    chart.remove_axis("sample-y");
    chart.remove_pane(scatter_pane.pane_index());
    columns.remove();
    chart.remove_axis("month");
    chart.remove_axis("revenue");
    chart.remove_pane(category_pane.pane_index());
    const remaining_panes = chart.panes().length;
    chart.remove();
    host.remove();
    return {
      before_remove,
      custom_label_changed,
      updated_custom_label,
      rejected_label,
      after_rejected_label,
      rejected_update,
      after_rejected_update,
      after_update,
      removed_axis,
      stale_axis_code,
      replacement_axis_title,
      remaining_panes,
      lifecycle,
    };
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
  expect(result.stale_axis_code).toBe("stale_handle");
  expect(result.replacement_axis_title).toBe("Replacement X");
  expect(result.remaining_panes).toBe(1);
  expect(result.lifecycle).toEqual([
    ["added", "column", 1],
    ["added", "scatter", 2],
    ["removed", "scatter", 2],
    ["removed", "column", 1],
  ]);
});

test("xy_line and xy_area span general domains and restore through V2", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  const result = await page.evaluate(async () => {
    const api = await import("/dist/nucleuscharts_financial.js");
    const host = document.createElement("div");
    Object.assign(host.style, {
      position: "fixed",
      width: "720px",
      height: "640px",
      left: "0",
      top: "0",
      zIndex: "10000",
    });
    document.body.appendChild(host);
    const chart = await api.create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(720, 640, 1);

    const numeric_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" },
    });
    chart.add_axis({ id: "line-x", pane: numeric_pane.pane_index(), dimension: "x", scale: "linear" });
    chart.add_axis({ id: "line-y", pane: numeric_pane.pane_index(), dimension: "y", scale: "linear" });
    const numeric_line = chart.add_series("xy_line", {
      pane: numeric_pane.pane_index(),
      x_axis_id: "line-x",
      y_axis_id: "line-y",
      title: "Numeric trend",
      color: "#336699",
      data_labels: true,
    });
    numeric_line.set_data([
      { id: "n0", x: 0, y: 0 },
      { id: "n1", x: 1, y: 1 },
      { id: "gap", x: 2, y: null },
      { id: "n3", x: 3, y: 3 },
      { id: "n4", x: 4, y: 4 },
    ]);

    const temporal_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "temporal" },
    });
    chart.add_axis({ id: "time-x", pane: temporal_pane.pane_index(), dimension: "x", scale: "temporal" });
    chart.add_axis({ id: "time-y", pane: temporal_pane.pane_index(), dimension: "y", scale: "linear" });
    const temporal_line = chart.add_series("xy_line", {
      pane: temporal_pane.pane_index(),
      x_axis_id: "time-x",
      y_axis_id: "time-y",
      title: "Temporal trend",
    });
    temporal_line.set_data_typed({
      ids: [1, 2, 3],
      x_epoch_ms: new Float64Array([1_700_000_000_000, 1_700_000_060_000, 1_700_000_120_000]),
      y: new Float64Array([10, 12, 11]),
    });
    temporal_line.update_data([
      { id: 2, x: new Date(1_700_000_060_000), y: 13 },
    ]);
    let temporal_rejection = null;
    try {
      temporal_line.set_data_typed({
        ids: [9],
        x_epoch_ms: new Float64Array([1_700_000_000_000.5]),
        y: new Float64Array([99]),
      });
    } catch (error) {
      temporal_rejection = error.code;
    }
    const temporal_after_rejection = temporal_line.accessibility_snapshot(0, 10);

    const category_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "category", scale: "point" },
    });
    chart.add_axis({ id: "point-x", pane: category_pane.pane_index(), dimension: "x", scale: "point" });
    chart.add_axis({ id: "point-y", pane: category_pane.pane_index(), dimension: "y", scale: "linear" });
    const category_line = chart.add_series("xy_line", {
      pane: category_pane.pane_index(),
      x_axis_id: "point-x",
      y_axis_id: "point-y",
      title: "Category trend",
    });
    category_line.set_data([
      { id: "a", x: "A", y: 3 },
      { id: "b", x: "B", y: 1 },
      { id: "c", x: "C", y: 2 },
    ]);
    const category_area = chart.add_series("xy_area", {
      pane: category_pane.pane_index(),
      x_axis_id: "point-x",
      y_axis_id: "point-y",
      title: "Category area",
      color: "#7e57c2",
    });
    category_area.set_data([
      { id: "aa", x: "A", y: 1 },
      { id: "ab", x: "B", y: 2 },
      { id: "ac", x: "C", y: 1.5 },
    ]);

    chart.resize(720, 640, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const geometry = numeric_pane.get_geometry();
    let hit = null;
    for (let y = geometry.top + 4; y < geometry.top + geometry.height - 4 && hit === null; y += 2) {
      for (let x = 0; x < geometry.width; x += 2) {
        const candidate = chart.general_hit_test(numeric_pane.pane_index(), x, y);
        if (candidate?.series === numeric_line.id) {
          hit = candidate;
          break;
        }
      }
    }

    const state = chart.export_state();
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:fixed;left:-10000px;top:0;width:720px;height:640px";
    document.body.appendChild(restored_host);
    const restored = await api.create_chart(restored_host, { backend: "canvas2d", autoSize: false });
    restored.resize(720, 640, 1);
    const restore_result = restored.import_state(state);
    const restored_kinds = restored.panes()
      .flatMap((pane) => pane.get_series())
      .map((series) => series.kind ?? series.series_type());

    const snapshot = {
      state_version: state.schema_version,
      restore_version: restore_result.schema_version,
      restored_kinds,
      numeric_gap: numeric_line.data_at(2),
      numeric_accessibility: numeric_line.accessibility_snapshot(0, 10),
      temporal_accessibility: temporal_after_rejection,
      category_accessibility: category_line.accessibility_snapshot(0, 10),
      area_accessibility: category_area.accessibility_snapshot(0, 10),
      temporal_rejection,
      hit,
      screenshot: chart.take_screenshot().toDataURL().length,
    };
    restored.remove();
    restored_host.remove();
    chart.remove();
    host.remove();
    return snapshot;
  });

  expect(result.state_version).toBe(2);
  expect(result.restore_version).toBe(2);
  expect(result.restored_kinds.filter((kind) => kind === "xy_line")).toHaveLength(3);
  expect(result.restored_kinds.filter((kind) => kind === "xy_area")).toHaveLength(1);
  expect(result.numeric_gap).toMatchObject({ row_id: "gap", x_label: "2", value: null });
  expect(result.numeric_accessibility.items.map((item) => item.value)).toEqual([0, 1, null, 3, 4]);
  expect(result.temporal_rejection).toBe("invalid_data");
  expect(result.temporal_accessibility.items).toMatchObject([
    { row_id: 1, x_label: "1700000000000", value: 10 },
    { row_id: 2, x_label: "1700000060000", value: 13 },
    { row_id: 3, x_label: "1700000120000", value: 11 },
  ]);
  expect(result.category_accessibility.items.map((item) => item.x_label)).toEqual(["A", "B", "C"]);
  expect(result.area_accessibility.items.map((item) => item.value)).toEqual([1, 2, 1.5]);
  expect(result.hit).toMatchObject({ series: expect.any(Number), row_id: expect.anything() });
  expect(result.screenshot).toBeGreaterThan(1000);
});

test("general columns, scatter, and XY lines participate in the shared keyboard accessibility controller", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  await page.evaluate(async () => {
    const api = await import("/dist/nucleuscharts_financial.js");
    const host = document.createElement("div");
    Object.assign(host.style, {
      position: "fixed",
      width: "640px",
      height: "480px",
      left: "0",
      top: "0",
      zIndex: "10000",
    });
    host.id = "general-keyboard-host";
    document.body.appendChild(host);
    const chart = await api.create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(640, 480, 1);

    const category_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "category", scale: "band" },
    });
    chart.add_axis({ id: "kbd-month", pane: category_pane.pane_index(), dimension: "x", scale: "band" });
    chart.add_axis({ id: "kbd-revenue", pane: category_pane.pane_index(), dimension: "y", scale: "linear" });
    const columns = chart.add_series("column", {
      pane: category_pane.pane_index(),
      x_axis_id: "kbd-month",
      y_axis_id: "kbd-revenue",
      title: "Keyboard revenue",
    });
    columns.set_data([
      { id: "jan", x: "Jan", y: 10 },
      { id: "feb", x: "Feb", y: 20 },
      { id: "mar", x: "Mar", y: 30 },
    ]);

    const scatter_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" },
    });
    chart.add_axis({ id: "kbd-x", pane: scatter_pane.pane_index(), dimension: "x", scale: "linear" });
    chart.add_axis({ id: "kbd-y", pane: scatter_pane.pane_index(), dimension: "y", scale: "linear" });
    const scatter = chart.add_series("scatter", {
      pane: scatter_pane.pane_index(),
      x_axis_id: "kbd-x",
      y_axis_id: "kbd-y",
      title: "Keyboard samples",
    });
    scatter.set_data([
      { id: 1, x: -10, y: -5 },
      { id: 2, x: 0, y: 0, label: "Center" },
      { id: 3, x: 10, y: 5 },
    ]);

    const line_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" },
    });
    chart.add_axis({ id: "kbd-line-x", pane: line_pane.pane_index(), dimension: "x", scale: "linear" });
    chart.add_axis({ id: "kbd-line-y", pane: line_pane.pane_index(), dimension: "y", scale: "linear" });
    const line = chart.add_series("xy_line", {
      pane: line_pane.pane_index(),
      x_axis_id: "kbd-line-x",
      y_axis_id: "kbd-line-y",
      title: "Keyboard trend",
    });
    line.set_data([
      { id: "l1", x: -2, y: 1 },
      { id: "l2", x: 0, y: 3 },
      { id: "l3", x: 2, y: 2 },
    ]);

    const accessibility = api.enable_accessibility(chart, {
      chart_title: (pane) => pane === category_pane.pane_index()
        ? "Category keyboard pane"
        : pane === scatter_pane.pane_index()
          ? "Scatter keyboard pane"
          : pane === line_pane.pane_index()
            ? "Line keyboard pane"
            : "Financial pane",
      data_scope: "all",
      show_shortcuts: true,
    });
    window.__general_keyboard = { chart, host, columns, scatter, line, accessibility };
  });

  const layers = page.locator("#general-keyboard-host .nucleuscharts-a11y-layer");
  await expect(layers).toHaveCount(4);

  await layers.nth(1).focus();
  await page.keyboard.press("Home");
  const category_region = layers.nth(1).locator(".nucleuscharts-a11y-live-region");
  await expect(category_region).toContainText("Jan");

  await page.evaluate(() => {
    window.__general_keyboard.columns.set_data([
      { id: "feb", x: "Feb", y: 20 },
      { id: "jan", x: "Jan", y: 11 },
      { id: "mar", x: "Mar", y: 30 },
    ]);
  });
  await page.keyboard.press("ArrowRight");
  await expect(category_region).toContainText("Mar");
  const category_text = await category_region.textContent();
  expect(category_text).toContain("Mar");
  expect(category_text).toContain("Point 3 of 3");

  await layers.nth(2).focus();
  await page.keyboard.press("End");
  const scatter_region = layers.nth(2).locator(".nucleuscharts-a11y-live-region");
  await expect(scatter_region).toContainText("Keyboard samples");
  const scatter_text = await scatter_region.textContent();
  expect(scatter_text).toContain("Keyboard samples");
  expect(scatter_text).toContain("10");
  expect(scatter_text).toContain("Point 3 of 3");

  const before_zoom = await page.evaluate(() => window.__general_keyboard.chart.take_screenshot().toDataURL());
  await page.keyboard.press("+");
  await page.waitForTimeout(30);
  const after_zoom = await page.evaluate(() => window.__general_keyboard.chart.take_screenshot().toDataURL());
  expect(after_zoom).not.toBe(before_zoom);

  await page.keyboard.press("Enter");
  await expect(scatter_region).toContainText("data points");

  await layers.nth(3).focus();
  await page.keyboard.press("Home");
  await page.keyboard.press("ArrowRight");
  const line_region = layers.nth(3).locator(".nucleuscharts-a11y-live-region");
  await expect(line_region).toContainText("Keyboard trend");
  const line_text = await line_region.textContent();
  expect(line_text).toContain("Keyboard trend");
  expect(line_text).toContain("Point 2 of 3");
  const line_before_zoom = await page.evaluate(() => window.__general_keyboard.chart.take_screenshot().toDataURL());
  await page.keyboard.press("+");
  await page.waitForTimeout(30);
  const line_after_zoom = await page.evaluate(() => window.__general_keyboard.chart.take_screenshot().toDataURL());
  expect(line_after_zoom).not.toBe(line_before_zoom);

  await page.evaluate(() => {
    window.__general_keyboard.accessibility.detach();
    window.__general_keyboard.chart.remove();
    window.__general_keyboard.host.remove();
    delete window.__general_keyboard;
  });
});
