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

test("general columns group and stack through the browser API and V2 persistence", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/nucleuscharts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:720px;height:640px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(720, 640, 1);

    const grouped_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "category", scale: "band" },
    });
    chart.add_axis({ id: "grouped-x", pane: grouped_pane.pane_index(), dimension: "x", scale: "band" });
    chart.add_axis({ id: "grouped-y", pane: grouped_pane.pane_index(), dimension: "y", scale: "linear" });
    const grouped_a = chart.add_series("column", {
      pane: grouped_pane.pane_index(), x_axis_id: "grouped-x", y_axis_id: "grouped-y",
      group_id: "sales", title: "North",
    });
    const grouped_b = chart.add_series("column", {
      pane: grouped_pane.pane_index(), x_axis_id: "grouped-x", y_axis_id: "grouped-y",
      group_id: "sales", title: "South",
    });
    grouped_a.set_data([{ id: "north-jan", x: "Jan", y: 10 }, { id: "north-feb", x: "Feb", y: 14 }]);
    grouped_b.set_data([{ id: "south-jan", x: "Jan", y: 18 }, { id: "south-feb", x: "Feb", y: 7 }]);

    const stacked_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "category", scale: "band" },
    });
    chart.add_axis({ id: "stacked-x", pane: stacked_pane.pane_index(), dimension: "x", scale: "band" });
    chart.add_axis({ id: "stacked-y", pane: stacked_pane.pane_index(), dimension: "y", scale: "linear" });
    const stacked_a = chart.add_series("column", {
      pane: stacked_pane.pane_index(), x_axis_id: "stacked-x", y_axis_id: "stacked-y",
      group_id: "totals", stack_id: "combined", stack_mode: "normal", title: "Base",
    });
    const stacked_b = chart.add_series("column", {
      pane: stacked_pane.pane_index(), x_axis_id: "stacked-x", y_axis_id: "stacked-y",
      group_id: "totals", stack_id: "combined", stack_mode: "normal", title: "Top",
    });
    stacked_a.set_data([{ id: "base-up", x: "Up", y: 10 }, { id: "base-down", x: "Down", y: -4 }]);
    stacked_b.set_data([{ id: "top-up", x: "Up", y: 5 }, { id: "top-down", x: "Down", y: -2 }]);

    let invalid_stack_mode = null;
    try {
      chart.add_series("column", {
        pane: grouped_pane.pane_index(), x_axis_id: "grouped-x", y_axis_id: "grouped-y",
        stack_id: "bad", stack_mode: "invalid",
      });
    } catch (error) {
      invalid_stack_mode = error.code;
    }

    chart.resize(720, 640, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));

    const scan_hits = (pane, seriesIds) => {
      const found = new Map();
      const geometry = pane.get_geometry();
      for (let y = geometry.top + 2; y < geometry.top + geometry.height - 2 && found.size < seriesIds.length; y += 2) {
        for (let x = 0; x < geometry.width; x += 2) {
          const hit = chart.general_hit_test(pane.pane_index(), x, y);
          if (hit && seriesIds.includes(hit.series) && !found.has(hit.series)) found.set(hit.series, hit);
        }
      }
      return Array.from(found.values());
    };
    const grouped_hits = scan_hits(grouped_pane, [grouped_a.id, grouped_b.id]);
    const stacked_hits = scan_hits(stacked_pane, [stacked_a.id, stacked_b.id]);

    const state = chart.export_state();
    const persisted_columns = state.series.filter((series) => series.kind === "Column");
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:fixed;left:-10000px;top:0;width:720px;height:640px";
    document.body.appendChild(restored_host);
    const restored = await create_chart(restored_host, { backend: "canvas2d", autoSize: false });
    restored.resize(720, 640, 1);
    const restore_result = restored.import_state(state);
    const restored_columns = restored.panes()
      .flatMap((pane) => pane.get_series())
      .filter((series) => series.kind === "column").length;

    const snapshot = {
      invalid_stack_mode,
      grouped_hits,
      stacked_hits,
      persisted_columns: persisted_columns.map(({ group_id, stack_id, stack_mode }) => ({ group_id, stack_id, stack_mode })),
      restored_columns,
      restore_version: restore_result.schema_version,
      grouped_values: [grouped_a.data_at(0)?.value, grouped_b.data_at(0)?.value],
      stacked_values: [stacked_a.data_at(0)?.value, stacked_b.data_at(0)?.value],
    };
    restored.remove();
    restored_host.remove();
    chart.remove();
    host.remove();
    return snapshot;
  });

  expect(result.invalid_stack_mode).toBe("invalid_options");
  expect(result.grouped_hits).toHaveLength(2);
  expect(new Set(result.grouped_hits.map((hit) => hit.series)).size).toBe(2);
  expect(result.stacked_hits).toHaveLength(2);
  expect(new Set(result.stacked_hits.map((hit) => hit.series)).size).toBe(2);
  expect(result.grouped_values).toEqual([10, 18]);
  expect(result.stacked_values).toEqual([10, 5]);
  expect(result.persisted_columns).toEqual([
    { group_id: "sales", stack_id: null, stack_mode: "Normal" },
    { group_id: "sales", stack_id: null, stack_mode: "Normal" },
    { group_id: "totals", stack_id: "combined", stack_mode: "Normal" },
    { group_id: "totals", stack_id: "combined", stack_mode: "Normal" },
  ]);
  expect(result.restore_version).toBe(2);
  expect(result.restored_columns).toBe(4);
});

test("bubble size data drives marks, hits, updates, accessibility, and V2 restore", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/nucleuscharts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:720px;height:520px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(720, 520, 1);

    const pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" },
    });
    chart.add_axis({ id: "bubble-x", pane: pane.pane_index(), dimension: "x", scale: "linear" });
    chart.add_axis({ id: "bubble-y", pane: pane.pane_index(), dimension: "y", scale: "linear" });
    const bubble = chart.add_series("bubble", {
      pane: pane.pane_index(),
      x_axis_id: "bubble-x",
      y_axis_id: "bubble-y",
      title: "Population",
      color: "#336699",
      data_labels: true,
    });
    bubble.set_data([
      { id: "small", x: 1, y: 1, size: 4 },
      { id: "large", x: 2, y: 2, size: 100 },
      { id: "zero", x: 3, y: 3, size: 0 },
      { id: "missing", x: 4, y: 4, size: null },
    ]);

    let negative_size_rejection = null;
    try {
      bubble.update_data([{ id: "large", x: 2, y: 2, size: -1 }]);
    } catch (error) {
      negative_size_rejection = error.code;
    }
    const after_rejection = bubble.data_at(1);

    bubble.update_data_typed({
      ids: ["large", "new"],
      x: new Float64Array([2.5, 5]),
      y: new Float64Array([2.5, 5]),
      size: new Float64Array([144, 25]),
    });
    chart.resize(720, 520, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));

    let hit = null;
    const geometry = pane.get_geometry();
    for (let y = geometry.top + 2; y < geometry.top + geometry.height - 2 && hit === null; y += 2) {
      for (let x = 0; x < geometry.width; x += 2) {
        const candidate = chart.general_hit_test(pane.pane_index(), x, y);
        if (candidate?.series === bubble.id) {
          hit = candidate;
          break;
        }
      }
    }

    const before_restore = bubble.accessibility_snapshot(0, 10);
    const state = chart.export_state();
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:fixed;left:-10000px;top:0;width:720px;height:520px";
    document.body.appendChild(restored_host);
    const restored = await create_chart(restored_host, { backend: "canvas2d", autoSize: false });
    restored.resize(720, 520, 1);
    const restore_result = restored.import_state(state);
    const restored_bubble = restored.panes()
      .flatMap((candidate) => candidate.get_series())
      .find((series) => series.kind === "bubble");
    const restored_snapshot = restored_bubble?.accessibility_snapshot(0, 10) ?? null;

    const snapshot = {
      negative_size_rejection,
      after_rejection,
      before_restore,
      restored_snapshot,
      restored_kind: restored_bubble?.kind ?? null,
      restore_version: restore_result.schema_version,
      hit,
      screenshot: chart.take_screenshot().toDataURL().length,
    };
    restored.remove();
    restored_host.remove();
    chart.remove();
    host.remove();
    return snapshot;
  });

  expect(result.negative_size_rejection).toBe("invalid_data");
  expect(result.after_rejection).toMatchObject({ row_id: "large", value: 2, size: 100 });
  expect(result.before_restore.items).toMatchObject([
    { row_id: "small", size: 4 },
    { row_id: "large", x_label: "2.5", value: 2.5, size: 144 },
    { row_id: "zero", size: 0 },
    { row_id: "missing", size: null },
    { row_id: "new", x_label: "5", value: 5, size: 25 },
  ]);
  expect(result.hit).toMatchObject({ series: expect.any(Number), row_id: expect.anything() });
  expect(result.restore_version).toBe(2);
  expect(result.restored_kind).toBe("bubble");
  expect(result.restored_snapshot).toEqual(result.before_restore);
  expect(result.screenshot).toBeGreaterThan(1000);
});

test("range_area spans numeric, temporal, and category domains through the browser API", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/nucleuscharts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:720px;height:760px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(720, 760, 1);

    const numeric_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" },
    });
    chart.add_axis({ id: "range-x", pane: numeric_pane.pane_index(), dimension: "x", scale: "linear" });
    chart.add_axis({ id: "range-y", pane: numeric_pane.pane_index(), dimension: "y", scale: "linear" });
    const numeric = chart.add_series("range_area", {
      pane: numeric_pane.pane_index(), x_axis_id: "range-x", y_axis_id: "range-y", title: "Forecast",
    });
    numeric.set_data([
      { id: "a", x: 1, low: 2, high: 5, label: "A" },
      { id: "b", x: 2, low: 3, high: 7, label: "B" },
      { id: "gap", x: 3, low: null, high: 8 },
    ]);

    let invalid_range = null;
    try {
      numeric.update_data([{ id: "b", x: 2, low: 9, high: 4 }]);
    } catch (error) {
      invalid_range = error.code;
    }
    const after_invalid = numeric.data_at(1);
    numeric.update_data_typed({
      ids: ["b", "c"],
      labels: ["B2", "C"],
      x: new Float64Array([2.5, 4]),
      low: new Float64Array([4, 5]),
      high: new Float64Array([8, 9]),
    }, { max_rows: 4 });

    const temporal_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "temporal" },
    });
    chart.add_axis({ id: "range-time", pane: temporal_pane.pane_index(), dimension: "x", scale: "temporal" });
    chart.add_axis({ id: "range-time-y", pane: temporal_pane.pane_index(), dimension: "y", scale: "linear" });
    const temporal = chart.add_series("range_area", {
      pane: temporal_pane.pane_index(), x_axis_id: "range-time", y_axis_id: "range-time-y",
    });
    temporal.set_data_typed({
      ids: ["t1", "t2"],
      x_epoch_ms: new Float64Array([Date.UTC(2026, 0, 1), Date.UTC(2026, 0, 2)]),
      low: new Float64Array([10, 12]),
      high: new Float64Array([14, 16]),
    });

    const category_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "category", scale: "point" },
    });
    chart.add_axis({ id: "range-category", pane: category_pane.pane_index(), dimension: "x", scale: "point" });
    chart.add_axis({ id: "range-category-y", pane: category_pane.pane_index(), dimension: "y", scale: "linear" });
    const category = chart.add_series("range_area", {
      pane: category_pane.pane_index(), x_axis_id: "range-category", y_axis_id: "range-category-y",
    });
    category.set_data_typed({
      ids: ["q1", "q2"],
      categories: ["Q1", "Q2"],
      category_indices: new Uint32Array([0, 1]),
      low: new Float64Array([20, 22]),
      high: new Float64Array([28, 30]),
    });

    chart.resize(720, 760, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    let hit = null;
    const geometry = numeric_pane.get_geometry();
    for (let y = geometry.top + 2; y < geometry.top + geometry.height - 2 && hit === null; y += 2) {
      for (let x = 0; x < geometry.width; x += 2) {
        const candidate = chart.general_hit_test(numeric_pane.pane_index(), x, y);
        if (candidate?.series === numeric.id) {
          hit = candidate;
          break;
        }
      }
    }

    const before_restore = [numeric, temporal, category].map((series) => series.accessibility_snapshot(0, 10));
    const state = chart.export_state();
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:fixed;left:-10000px;top:0;width:720px;height:760px";
    document.body.appendChild(restored_host);
    const restored = await create_chart(restored_host, { backend: "canvas2d", autoSize: false });
    restored.resize(720, 760, 1);
    const restore_result = restored.import_state(state);
    const restored_ranges = restored.panes()
      .flatMap((pane) => pane.get_series())
      .filter((series) => series.kind === "range_area")
      .map((series) => series.accessibility_snapshot(0, 10));

    const snapshot = {
      invalid_range,
      after_invalid,
      hit,
      before_restore,
      restored_ranges,
      restore_version: restore_result.schema_version,
      screenshot: chart.take_screenshot().toDataURL().length,
    };
    restored.remove();
    restored_host.remove();
    chart.remove();
    host.remove();
    return snapshot;
  });

  expect(result.invalid_range).toBe("invalid_data");
  expect(result.after_invalid).toMatchObject({ row_id: "b", low: 3, high: 7, value: 7 });
  expect(result.hit).toMatchObject({ series: expect.any(Number), row_id: expect.anything() });
  expect(result.before_restore[0].items).toMatchObject([
    { row_id: "a", x_label: "1", label: "A", low: 2, high: 5 },
    { row_id: "b", x_label: "2.5", label: "B2", low: 4, high: 8 },
    { row_id: "gap", low: null, high: 8 },
    { row_id: "c", x_label: "4", label: "C", low: 5, high: 9 },
  ]);
  expect(result.before_restore[1].items).toMatchObject([
    { row_id: "t1", low: 10, high: 14 },
    { row_id: "t2", low: 12, high: 16 },
  ]);
  expect(result.before_restore[2].items).toMatchObject([
    { row_id: "q1", x_label: "Q1", low: 20, high: 28 },
    { row_id: "q2", x_label: "Q2", low: 22, high: 30 },
  ]);
  expect(result.restore_version).toBe(2);
  expect(result.restored_ranges).toEqual(result.before_restore);
  expect(result.screenshot).toBeGreaterThan(1000);
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

test("general columns, points, lines, and ranges participate in the shared keyboard accessibility controller", async ({ page }) => {
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

    const range_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" },
    });
    chart.add_axis({ id: "kbd-range-x", pane: range_pane.pane_index(), dimension: "x", scale: "linear" });
    chart.add_axis({ id: "kbd-range-y", pane: range_pane.pane_index(), dimension: "y", scale: "linear" });
    const range = chart.add_series("range_area", {
      pane: range_pane.pane_index(),
      x_axis_id: "kbd-range-x",
      y_axis_id: "kbd-range-y",
      title: "Keyboard forecast",
    });
    range.set_data([
      { id: "r1", x: -2, low: 1, high: 3 },
      { id: "r2", x: 0, low: 2, high: 5, label: "Expected" },
      { id: "r3", x: 2, low: 3, high: 6 },
    ]);

    const accessibility = api.enable_accessibility(chart, {
      chart_title: (pane) => pane === category_pane.pane_index()
        ? "Category keyboard pane"
        : pane === scatter_pane.pane_index()
          ? "Scatter keyboard pane"
          : pane === line_pane.pane_index()
            ? "Line keyboard pane"
            : pane === range_pane.pane_index()
              ? "Range keyboard pane"
              : "Financial pane",
      data_scope: "all",
      show_shortcuts: true,
    });
    window.__general_keyboard = { chart, host, columns, scatter, line, range, accessibility };
  });

  const layers = page.locator("#general-keyboard-host .nucleuscharts-a11y-layer");
  await expect(layers).toHaveCount(5);

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

  await layers.nth(4).focus();
  await page.keyboard.press("Home");
  await page.keyboard.press("ArrowRight");
  const range_region = layers.nth(4).locator(".nucleuscharts-a11y-live-region");
  await expect(range_region).toContainText("Keyboard forecast");
  const range_text = await range_region.textContent();
  expect(range_text).toContain("Expected");
  expect(range_text).toContain("2 to 5");
  expect(range_text).toContain("Point 2 of 3");
  const range_before_zoom = await page.evaluate(() => window.__general_keyboard.chart.take_screenshot().toDataURL());
  await page.keyboard.press("+");
  await page.waitForTimeout(30);
  const range_after_zoom = await page.evaluate(() => window.__general_keyboard.chart.take_screenshot().toDataURL());
  expect(range_after_zoom).not.toBe(range_before_zoom);

  await page.evaluate(() => {
    window.__general_keyboard.accessibility.detach();
    window.__general_keyboard.chart.remove();
    window.__general_keyboard.host.remove();
    delete window.__general_keyboard;
  });
});
