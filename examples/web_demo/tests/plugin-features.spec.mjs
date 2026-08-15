import { test, expect } from "@playwright/test";

async function open_chart(page) {
  await page.goto("/?runtimeTest=presentedFrame&backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
}

test("all advanced series render through the shared Rust engine", async ({ page }) => {
  const warnings = [];
  page.on("console", (message) => {
    if (message.type() === "warning") warnings.push(message.text());
  });
  await open_chart(page);
  const before = await page.screenshot();
  const result = await page.evaluate(async () => {
    const api = await import("/dist/nucleuscharts_financial.js");
    const bars = window.__data.slice(0, 20);
    const definitions = [
      ["brushable_area", { brush_ranges: [{ range: { from: 5, to: 10 }, style: { line_color: "#f23645", top_color: "#f2364555", bottom_color: "#f2364500", line_width: 2 } }] }, bars.map((bar) => ({ time: bar.time, value: bar.close }))],
      ["dual_range_histogram", {}, bars.map((bar, index) => ({ time: bar.time, values: [index + 1, index / 2 + 1, index / 3 + 1, index / 4 + 1] }))],
      ["grouped_bars", {}, bars.map((bar, index) => ({ time: bar.time, values: [index + 1, index + 3, index + 2] }))],
      ["heatmap", {}, bars.map((bar, index) => ({ time: bar.time, cells: [{ low: bar.low, high: bar.close, amount: index * 5 }, { low: bar.close, high: bar.high, amount: 100 - index * 5 }] }))],
      ["hlc_area", {}, bars.map((bar) => ({ time: bar.time, high: bar.high, low: bar.low, close: bar.close }))],
      ["pretty_histogram", { base_price: 90 }, bars.map((bar) => ({ time: bar.time, value: bar.close }))],
      ["rounded_candles", {}, bars.map(({ time, open, high, low, close }) => ({ time, open, high, low, close }))],
      ["background_shade", { low_value: 90, high_value: 120 }, bars.map((bar) => ({ time: bar.time, value: bar.close }))],
      ["stacked_area", {}, bars.map((bar, index) => ({ time: bar.time, values: [10 + index, 5 + index / 2, 3] }))],
      ["stacked_bars", {}, bars.map((bar, index) => ({ time: bar.time, values: [10 + index, 5, 3] }))],
      ["whisker_box", {}, bars.map((bar) => ({ time: bar.time, quartiles: [bar.low - 1, bar.low, bar.close, bar.high, bar.high + 1], outliers: [bar.high + 2] }))],
    ];
    const series = definitions.map(([kind, options, data]) => {
      const feature = window.__chart.add_series(kind, { ...options, price_line_visible: false, last_value_visible: false });
      feature.set_data(data);
      return feature;
    });
    window.__chart.time_scale().fit_content();
    window.__chart.render();
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    return { count: series.length, kinds: series.map((item) => item.series_type()) };
  });
  const after = await page.screenshot();
  expect(result.count).toBe(11);
  expect(new Set(result.kinds)).toEqual(new Set([
    "brushable_area", "dual_range_histogram", "grouped_bars", "heatmap", "hlc_area",
    "pretty_histogram", "rounded_candles", "background_shade", "stacked_area",
    "stacked_bars", "whisker_box",
  ]));
  expect(after.equals(before)).toBe(false);
  expect(warnings.filter((warning) => warning.includes("advanced series") || warning.includes("skipped"))).toEqual([]);
});

test("advanced-series data, updates, options, and diagnostics round-trip from Rust", async ({ page }) => {
  await open_chart(page);
  const result = await page.evaluate(() => {
    const feature = window.__chart.add_series("grouped_bars", {
      colors: ["#2962ff", "#e1575a"],
      price_line_visible: false,
    });
    feature.set_data([
      { time: 3, values: [3, 6] },
      { time: 1, values: [1, 2] },
      { time: 3, values: [30, 60] },
      { time: 2 },
    ]);
    const replace_diagnostics = feature.last_ingestion_diagnostics();
    feature.update({ time: 2, values: [20, 40] });
    const after_update = feature.data();
    const nearest = feature.data_by_index(1, 1);
    feature.apply_options({ colors: ["#089981", "#f23645"] });
    const options = feature.options();
    feature.update({ time: Number.NaN, values: [1, 2] });
    const invalid = feature.last_ingestion_diagnostics();
    return {
      type: feature.series_type(),
      replace_diagnostics,
      after_update,
      nearest,
      colors: options.colors,
      invalid,
    };
  });
  expect(result.type).toBe("grouped_bars");
  expect(result.replace_diagnostics).toMatchObject({ accepted: 3, deduplicated: 1, reordered: true });
  expect(result.after_update).toEqual([
    { time: 1, values: [1, 2] },
    { time: 2, values: [20, 40] },
    { time: 3, values: [30, 60] },
  ]);
  expect(result.nearest).toEqual({ time: 2, values: [20, 40] });
  expect(result.colors).toEqual(["#089981", "#f23645"]);
  expect(result.invalid).toMatchObject({ status: "rejected", accepted: 0, dropped_invalid: 1 });
});

test("heatmap cell_shader resolves colors at the host boundary and keeps data engine-owned", async ({ page }) => {
  await open_chart(page);
  const result = await page.evaluate(() => {
    let initial_calls = 0;
    let replacement_calls = 0;
    const heatmap = window.__chart.add_series("heatmap", {
      cell_shader: () => {
        initial_calls += 1;
        return "#0c2238";
      },
    });
    heatmap.set_data([
      { time: 1, cells: [{ low: 10, high: 11, amount: 25 }, { low: 11, high: 12, amount: 75 }] },
    ]);
    const data = heatmap.data();
    heatmap.apply_options({
      cell_shader: () => {
        replacement_calls += 1;
        return "rgba(155,0,255,.8)";
      },
    });
    return {
      initial_calls,
      replacement_calls,
      has_shader: typeof heatmap.options().cell_shader === "function",
      data,
    };
  });
  expect(result).toMatchObject({ initial_calls: 2, replacement_calls: 2, has_shader: true });
  expect(result.data).toEqual([
    { time: 1, cells: [{ low: 10, high: 11, amount: 25 }, { low: 11, high: 12, amount: 75 }] },
  ]);
});

test("primitive feature helpers compose existing engine and host boundaries", async ({ page }) => {
  const page_errors = [];
  page.on("pageerror", (error) => page_errors.push(error.message));
  await open_chart(page);
  const result = await page.evaluate(async () => {
    const api = await import("/dist/nucleuscharts_financial.js");
    const chart = window.__chart;
    const series = window.__main;
    const bars = window.__data;
    const start = bars[10];
    const end = bars[30];
    const handles = [];

    const anchored = api.create_anchored_text(series, { text: "Anchor", font: "bold 20px Arial" });
    anchored.apply_options({ text: "Updated anchor", horz_align: "right", vert_align: "bottom" });
    handles.push(anchored);
    api.create_rectangle_drawing(chart, [{ logical: 14, price: start.low }, { logical: 20, price: end.high }]);
    handles.push(api.create_trend_line(series, [{ time: start.time, price: start.low }, { time: end.time, price: end.high }]));
    handles.push(api.create_vertical_line(series, bars[24].time, { label_text: "Event", show_label: true }));
    api.create_user_price_line(series, { price: start.close, title: "Level" });
    handles.push(api.create_user_price_lines(chart, series));
    handles.push(api.create_partial_price_line(series));
    handles.push(api.create_session_highlighting(series, { start_hour_utc: 0, end_hour_utc: 24 }));
    handles.push(api.create_highlight_bar_crosshair(chart, series));
    const profile = bars.slice(0, 15).map((bar, index) => ({ price: bar.close, vol: index + 1 }));
    const volume_profile = api.create_volume_profile(series, { time: start.time, profile, width: 10 });
    volume_profile.set_data({ time: start.time, profile: profile.map((point) => ({ ...point, vol: point.vol + 1 })), width: 12 });
    handles.push(volume_profile);
    handles.push(api.create_bands_indicator(series));

    const overlay = chart.add_series("line", { price_scale_id: "" });
    overlay.set_data(bars.slice(0, 20).map((bar) => ({ time: bar.time, value: bar.close })));
    const overlay_labels = api.create_overlay_price_scale(overlay);
    const overlay_scale = overlay_labels.price_scale();
    handles.push(overlay_labels);

    const image = document.createElement("canvas");
    image.width = 16;
    image.height = 16;
    image.getContext("2d").fillRect(0, 0, 16, 16);
    handles.push(api.create_image_watermark(series, image.toDataURL(), { maxWidth: 32, maxHeight: 32, alpha: 0.4 }));
    handles.push(api.create_tooltip(chart));
    handles.push(api.create_delta_tooltip(chart, { series }));

    const alerts = api.create_expiring_price_alerts(series);
    alerts.add(start.close, start.time, end.time + 86_400, { title: "Crossing up", crossing_direction: "up" });
    alerts.add(end.close, start.time, end.time, { title: "Crossing down", crossing_direction: "down" });
    const user_alerts = api.create_user_price_alerts(chart, series);
    user_alerts.add(start.close);

    const a11y = api.enable_accessibility(chart, { chart_title: "Test financial chart" });
    a11y.focus();
    document.activeElement.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true }));
    chart.set_crosshair_position(start.close, start.time, series);
    chart.render();
    await new Promise((resolve) => setTimeout(resolve, 40));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));

    const state = {
      drawings: chart.drawings().map((drawing) => drawing.kind()),
      overlay_has_range: overlay_scale.get_visible_range() !== null,
      tooltips: chart.chart_element().querySelectorAll(".nucleuscharts-tooltip").length,
      role: chart.chart_element().querySelector(".nucleuscharts-a11y-layer")?.getAttribute("role"),
      label: chart.chart_element().querySelector(".nucleuscharts-a11y-layer")?.getAttribute("aria-label"),
      announcement: chart.chart_element().querySelector(".nucleuscharts-a11y-live-region")?.textContent ?? "",
      alerts: alerts.alerts().length,
      user_alerts: user_alerts.alerts().length,
    };
    a11y.detach();
    alerts.detach();
    user_alerts.detach();
    handles.forEach((handle) => handle.detach());
    return state;
  });
  expect(result.drawings).toEqual(expect.arrayContaining(["rectangle"]));
  expect(result.overlay_has_range).toBe(true);
  expect(result.tooltips).toBe(1);
  expect(result.role).toBe("application");
  expect(result.label).toContain("Test financial chart");
  expect(result.announcement).toContain("Point");
  expect(result.alerts).toBe(2);
  expect(result.user_alerts).toBe(1);
  expect(page_errors).toEqual([]);
});

test("rectangle tool uses official two-click preview, data-time snapping, and engine axis views", async ({ page }) => {
  await open_chart(page);
  const setup = await page.evaluate(async () => {
    const api = await import("/dist/nucleuscharts_financial.js");
    const chart = window.__chart;
    const series = window.__main;
    series.apply_options({ price_scale_id: "left" });
    chart.apply_options({
      leftPriceScale: { visible: true },
      rightPriceScale: { visible: false },
    });
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const toolbar = document.createElement("div");
    toolbar.id = "rectangle-test-toolbar";
    document.body.appendChild(toolbar);
    window.__rectangle_tool = api.create_rectangle_drawing_tool(chart, series, toolbar);
    window.__rectangle_tool.start_drawing();
    const range = chart.time_scale().get_visible_logical_range();
    const first_logical = Math.ceil(range.from + (range.to - range.from) * 0.3);
    const second_logical = Math.floor(range.from + (range.to - range.from) * 0.7);
    const spacing = chart.time_scale().logical_to_coordinate(first_logical + 1) - chart.time_scale().logical_to_coordinate(first_logical);
    const first = series.data_by_index(first_logical);
    const second = series.data_by_index(second_logical);
    const pane_offset = chart.price_scale("left").width();
    return {
      first: {
        x: pane_offset + chart.time_scale().logical_to_coordinate(first_logical) + spacing * 0.36,
        y: series.price_to_coordinate(first.low),
      },
      second: {
        x: pane_offset + chart.time_scale().logical_to_coordinate(second_logical) + spacing * 0.41,
        y: series.price_to_coordinate(second.high),
      },
      toolbar_children: toolbar.childElementCount,
      logicals: [first_logical, second_logical],
    };
  });
  expect(setup.toolbar_children).toBe(2);
  await page.mouse.click(setup.first.x, setup.first.y);
  await page.mouse.move(setup.second.x, setup.second.y);
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));

  const preview = await page.evaluate(() => {
    const chart = window.__chart;
    const canvas = chart.take_screenshot();
    const ctx = canvas.getContext("2d");
    let band_pixels = 0;
    const pixels = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
    for (let index = 0; index < pixels.length; index += 4) {
      if (Math.abs(pixels[index] - 248) <= 3 && Math.abs(pixels[index + 1] - 229) <= 3 && Math.abs(pixels[index + 2] - 236) <= 3 && pixels[index + 3] === 255) {
        band_pixels += 1;
      }
    }
    return {
      drawing_count: chart.drawings().length,
      active: window.__rectangle_tool.is_drawing(),
      pending: chart.creation_active(),
      band_pixels,
    };
  });
  expect(preview.drawing_count).toBe(0);
  expect(preview.active).toBe(true);
  expect(preview.pending).toBe(true);
  expect(preview.band_pixels).toBeGreaterThan(20);

  await page.mouse.click(setup.second.x, setup.second.y);
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  const committed = await page.evaluate(() => {
    const chart = window.__chart;
    const drawing = chart.drawings()[0];
    const canvas = chart.take_screenshot();
    const ctx = canvas.getContext("2d");
    const options = drawing.options();
    const points = drawing.points();
    let label_pixels = 0;
    let band_pixels = 0;
    const pixels = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
    for (let index = 0; index < pixels.length; index += 4) {
      if (Math.abs(pixels[index] - 200) <= 3 && Math.abs(pixels[index + 1] - 50) <= 3 && Math.abs(pixels[index + 2] - 100) <= 3 && pixels[index + 3] === 255) {
        label_pixels += 1;
      }
      if (Math.abs(pixels[index] - 234) <= 3 && Math.abs(pixels[index + 1] - 178) <= 3 && Math.abs(pixels[index + 2] - 197) <= 3 && pixels[index + 3] === 255) {
        band_pixels += 1;
      }
    }
    return {
      count: chart.drawings().length,
      active: window.__rectangle_tool.is_drawing(),
      points,
      options,
      label_pixels,
      band_pixels,
    };
  });
  expect(committed.count).toBe(1);
  expect(committed.active).toBe(false);
  expect(committed.points.map((point) => point.logical)).toEqual(setup.logicals);
  expect(committed.options).toMatchObject({
    price_scale_id: "left",
    fill_color: "rgba(200, 50, 100, 0.75)",
    preview_fill_color: "rgba(200, 50, 100, 0.25)",
    border_visible: false,
    show_labels: true,
    axis_bands_visible: true,
    snap_time_to_data: true,
  });
  expect(committed.band_pixels).toBeGreaterThan(20);
  expect(committed.label_pixels).toBeGreaterThan(20);

  const removed = await page.evaluate(() => {
    window.__rectangle_tool.remove();
    return {
      drawings: window.__chart.drawings().length,
      toolbar: document.querySelector("#rectangle-test-toolbar").childElementCount,
    };
  });
  expect(removed).toEqual({ drawings: 0, toolbar: 0 });
});

test("session highlighting follows the official callback contract and refreshes on source data", async ({ page }) => {
  await open_chart(page);
  const result = await page.evaluate(async () => {
    const api = await import("/dist/nucleuscharts_financial.js");
    const series = window.__main;
    const source_count = series.data().length;
    let calls = 0;
    let last_type = "";
    const handle = api.create_session_highlighting(series, (time) => {
      calls += 1;
      last_type = typeof time;
      return calls % 2 === 0 ? "rgba(1, 2, 3, 0.2)" : "rgba(4, 5, 6, 0.3)";
    });
    const initial_calls = calls;
    const last = series.data().at(-1);
    series.update({
      time: last.time + 86_400,
      open: last.close,
      high: last.close + 2,
      low: last.close - 2,
      close: last.close + 1,
    });
    await new Promise((resolve) => requestAnimationFrame(resolve));
    const update_calls = calls;
    handle.detach();
    series.update({
      time: last.time + 2 * 86_400,
      open: last.close + 1,
      high: last.close + 3,
      low: last.close - 1,
      close: last.close + 2,
    });
    return { source_count, initial_calls, update_calls, detached_calls: calls, last_type };
  });
  expect(result.initial_calls).toBe(result.source_count);
  expect(result.update_calls).toBe(result.initial_calls + result.source_count + 1);
  expect(result.detached_calls).toBe(result.update_calls);
  expect(result.last_type).toBe("number");
});

test("accessibility provides per-pane semantics, official keyboard help, summaries, updates, and engine focus", async ({ page }) => {
  await open_chart(page);
  const result = await page.evaluate(async () => {
    const api = await import("/dist/nucleuscharts_financial.js");
    const chart = window.__chart;
    const series = window.__main;
    const before_canvas = [...chart.chart_element().querySelectorAll("canvas")].map((canvas) => canvas.getAttribute("aria-hidden"));
    const accessibility = api.enable_accessibility(chart, {
      chart_title: "Accessible price history",
      data_scope: "all",
      show_shortcuts: true,
    });
    accessibility.focus(0);
    const layer = document.activeElement;
    layer.dispatchEvent(new KeyboardEvent("keydown", { key: "End", bubbles: true }));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const point_text = chart.chart_element().querySelector(".nucleuscharts-a11y-live-region")?.textContent ?? "";
    const last = series.data().at(-1);
    const x = chart.time_scale().time_to_coordinate(last.time);
    const y = series.price_to_coordinate(last.close);
    const screenshot = chart.take_screenshot(true, false);
    const context = screenshot.getContext("2d");
    const ratio = screenshot.width / chart.chart_element().clientWidth;
    const pixels = context.getImageData(
      Math.max(0, Math.floor((x - 16) * ratio)),
      Math.max(0, Math.floor((y - 16) * ratio)),
      Math.ceil(32 * ratio),
      Math.ceil(32 * ratio),
    ).data;
    let focus_pixels = 0;
    for (let index = 0; index < pixels.length; index += 4) {
      if (pixels[index] < 90 && pixels[index + 1] > 60 && pixels[index + 1] < 145 && pixels[index + 2] > 180 && pixels[index + 3] > 180) focus_pixels++;
    }
    layer.dispatchEvent(new KeyboardEvent("keydown", { key: "H", bubbles: true }));
    const panel_visible = getComputedStyle(chart.chart_element().querySelector(".nucleuscharts-a11y-shortcuts-panel")).display !== "none";
    const before_zoom = chart.time_scale().get_visible_logical_range();
    layer.dispatchEvent(new KeyboardEvent("keydown", { key: "+", bubbles: true }));
    const after_zoom = chart.time_scale().get_visible_logical_range();
    layer.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const summary = chart.chart_element().querySelector(".nucleuscharts-a11y-live-region")?.textContent ?? "";
    series.update({ ...last, close: last.close + 1 });
    await new Promise((resolve) => setTimeout(resolve, 220));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const update = chart.chart_element().querySelector(".nucleuscharts-a11y-shared-status-region")?.textContent ?? "";
    const semantic = {
      layers: chart.chart_element().querySelectorAll(".nucleuscharts-a11y-layer").length,
      role: layer.getAttribute("role"),
      role_description: layer.getAttribute("aria-roledescription"),
      label: layer.getAttribute("aria-label"),
      canvases_hidden: [...chart.chart_element().querySelectorAll("canvas")].every((canvas) => canvas.getAttribute("aria-hidden") === "true"),
    };
    accessibility.detach();
    const after_canvas = [...chart.chart_element().querySelectorAll("canvas")].map((canvas) => canvas.getAttribute("aria-hidden"));
    return { semantic, point_text, focus_pixels, panel_visible, before_zoom, after_zoom, summary, update, before_canvas, after_canvas };
  });
  expect(result.semantic).toMatchObject({
    layers: 1,
    role: "application",
    role_description: "Interactive chart pane",
    canvases_hidden: true,
  });
  expect(result.semantic.label).toContain("Accessible price history");
  expect(result.point_text).toContain("Point");
  expect(result.point_text).toContain("open");
  expect(result.focus_pixels).toBeGreaterThan(0);
  expect(result.panel_visible).toBe(true);
  expect(result.after_zoom.to - result.after_zoom.from).toBeLessThan(result.before_zoom.to - result.before_zoom.from);
  expect(result.summary).toContain("data points");
  expect(result.update).toContain("Chart data updated");
  expect(result.after_canvas).toEqual(result.before_canvas);
});

test("accessibility creates an independently named focus target for every live pane", async ({ page }) => {
  await open_chart(page);
  const result = await page.evaluate(async () => {
    const api = await import("/dist/nucleuscharts_financial.js");
    const chart = window.__chart;
    const accessibility = api.enable_accessibility(chart, {
      chart_title: (pane_index) => pane_index === 0 ? "Price pane" : "Volume pane",
    });
    const volume = chart.add_series("histogram", { pane: 1, title: "Volume" });
    volume.set_data(window.__data.map((bar, index) => ({ time: bar.time, value: 1_000 + index })));
    await new Promise((resolve) => queueMicrotask(resolve));
    await new Promise((resolve) => requestAnimationFrame(resolve));
    accessibility.focus(1);
    const layers = [...chart.chart_element().querySelectorAll(".nucleuscharts-a11y-layer")];
    const state = {
      count: layers.length,
      labels: layers.map((layer) => layer.getAttribute("aria-label")),
      focused_label: document.activeElement?.getAttribute("aria-label"),
    };
    accessibility.detach();
    return state;
  });
  expect(result.count).toBe(2);
  expect(result.labels[0]).toContain("Price pane");
  expect(result.labels[1]).toContain("Volume pane");
  expect(result.focused_label).toContain("Volume pane");
});

test("user price alerts use the engine button and centered remove hit", async ({ page }) => {
  await open_chart(page);
  const geometry = await page.evaluate(async () => {
    const api = await import("/dist/nucleuscharts_financial.js");
    const chart = window.__chart;
    const series = chart.panes()[0].get_series()[0];
    window.__user_price_alerts = api.create_user_price_alerts(chart, series, { symbol_name: "AAPL" });
    const pane = chart.panes()[0].get_geometry();
    const bounds = chart.chart_element().getBoundingClientRect();
    return { left: bounds.left + pane.left, top: bounds.top + pane.top, width: pane.width, height: pane.height };
  });
  const before = await page.screenshot();
  await page.waitForFunction(() => performance.now() > 600);
  await page.mouse.move(
    geometry.left + geometry.width - 10,
    geometry.top + geometry.height * 0.5,
  );
  expect(await page.evaluate(() => getComputedStyle(document.elementFromPoint(
    window.__chart.chart_element().getBoundingClientRect().left
      + window.__chart.panes()[0].get_geometry().left
      + window.__chart.panes()[0].get_geometry().width - 10,
    window.__chart.chart_element().getBoundingClientRect().top
      + window.__chart.panes()[0].get_geometry().top
      + window.__chart.panes()[0].get_geometry().height * 0.5,
  )).cursor)).toBe("pointer");
  await page.mouse.click(
    geometry.left + geometry.width - 10,
    geometry.top + geometry.height * 0.5,
  );
  await expect.poll(() => page.evaluate(() => window.__user_price_alerts.alerts().length)).toBe(1);
  const after = await page.screenshot();
  expect(after.equals(before)).toBe(false);

  const remove = await page.evaluate(() => {
    const chart = window.__chart;
    const series = chart.panes()[0].get_series()[0];
    const pane = chart.panes()[0].get_geometry();
    const bounds = chart.chart_element().getBoundingClientRect();
    const alert = window.__user_price_alerts.alerts()[0];
    const text = `AAPL crossing ${series.price_formatter()(alert.price)}`;
    const width = 18 + 26 + text.length * 5.81;
    return {
      x: bounds.left + pane.left + (pane.width - width) * 0.5 + width - 13,
      y: bounds.top + series.price_to_coordinate(alert.price),
    };
  });
  await page.mouse.move(remove.x, remove.y);
  await page.mouse.click(remove.x, remove.y);
  await expect.poll(() => page.evaluate(() => window.__user_price_alerts.alerts().length)).toBe(0);
  await page.evaluate(() => window.__user_price_alerts.detach());
});

test("user price lines create through the chart's engine click route", async ({ page }) => {
  await open_chart(page);
  const geometry = await page.evaluate(async () => {
    const api = await import("/dist/nucleuscharts_financial.js");
    const chart = window.__chart;
    window.__user_price_lines = api.create_user_price_lines(chart, window.__main, {
      color: "#ff00aa",
      hover_color: "#550033",
    });
    const pane = chart.panes()[0].get_geometry();
    const bounds = chart.chart_element().getBoundingClientRect();
    return {
      x: bounds.left + pane.left + pane.width - 10,
      y: bounds.top + pane.top + pane.height * 0.5,
    };
  });
  await page.waitForFunction(() => performance.now() > 600);
  await page.mouse.move(geometry.x, geometry.y);
  const button = await page.screenshot();
  await page.mouse.click(geometry.x, geometry.y);
  await page.waitForTimeout(40);
  const line = await page.screenshot();
  expect(line.equals(button)).toBe(false);
  await page.evaluate(() => window.__user_price_lines.detach());
});

test("tooltip matches the official structured chrome over an engine source snapshot", async ({ page }) => {
  await open_chart(page);
  const target = await page.evaluate(async () => {
    const api = await import("/dist/nucleuscharts_financial.js");
    const chart = window.__chart;
    window.__official_tooltip = api.create_tooltip(chart, {
      series: window.__main,
      title: "AAPL",
      follow_mode: "top",
      top_offset: 20,
    });
    const range = chart.time_scale().get_visible_logical_range();
    const logical = Math.round((range.from + range.to) * 0.5);
    const pane = chart.panes()[0].get_geometry();
    const bounds = chart.chart_element().getBoundingClientRect();
    return {
      x: bounds.left + pane.left + chart.time_scale().logical_to_coordinate(logical),
      y: bounds.top + pane.top + pane.height * 0.5,
    };
  });
  await page.waitForFunction(() => performance.now() > 600);
  const before = await page.screenshot();
  await page.mouse.move(target.x, target.y);
  await expect.poll(() => page.locator(".nucleuscharts-tooltip").evaluate((element) => element.style.opacity)).toBe("1");
  const content = await page.locator(".nucleuscharts-tooltip").evaluate((element) => ({
    rows: [...element.children].map((row) => row.textContent),
    background: getComputedStyle(element).backgroundColor,
    shadow: getComputedStyle(element).boxShadow,
    transform: element.style.transform,
  }));
  expect(content.rows[0]).toBe("AAPL");
  expect(content.rows[1]).toMatch(/^-?\d+\.\d{2}$/);
  expect(content.rows[2]).toMatch(/^\d{2} .+ \d{4}$/);
  expect(content.rows[3]).toMatch(/^\d{2}:\d{2}$/);
  expect(content.background).toBe("rgb(255, 255, 255)");
  expect(content.shadow).not.toBe("none");
  expect(content.transform).toContain("20px");
  const after = await page.screenshot();
  expect(after.equals(before)).toBe(false);
  await page.evaluate(() => window.__official_tooltip.detach());
  await expect(page.locator(".nucleuscharts-tooltip")).toHaveCount(0);
});

test("delta tooltip uses standard gestures and Rust-owned one/two-point state", async ({ page }) => {
  await open_chart(page);
  const geometry = await page.evaluate(async () => {
    const api = await import("/dist/nucleuscharts_financial.js");
    const chart = window.__chart;
    chart.apply_options({ handle_scroll: false, handle_scale: false });
    window.__delta_ranges = [];
    window.__delta_tooltip = api.create_delta_tooltip(chart, {
      series: window.__main,
      on_active_range_change: (range) => window.__delta_ranges.push(range),
    });
    const pane = chart.panes()[0].get_geometry();
    const bounds = chart.chart_element().getBoundingClientRect();
    const range = chart.time_scale().get_visible_logical_range();
    const span = range.to - range.from;
    const first = Math.ceil(range.from + span * 0.25);
    const second = Math.floor(range.from + span * 0.75);
    const middle = Math.round((first + second) * 0.5);
    return {
      left: bounds.left + pane.left,
      top: bounds.top + pane.top,
      y: bounds.top + pane.top + pane.height * 0.5,
      first,
      second,
      x_first: chart.time_scale().logical_to_coordinate(first),
      x_second: chart.time_scale().logical_to_coordinate(second),
      x_middle: chart.time_scale().logical_to_coordinate(middle),
    };
  });
  await page.waitForFunction(() => performance.now() > 600);
  const before = await page.screenshot();
  await page.mouse.move(geometry.left + geometry.x_first, geometry.y);
  await expect.poll(() => page.evaluate(() => window.__delta_tooltip.active_range())).toBe(null);
  const hover = await page.screenshot();
  expect(hover.equals(before)).toBe(false);

  await page.mouse.down();
  await page.mouse.move(geometry.left + geometry.x_second, geometry.y, { steps: 4 });
  await expect.poll(() => page.evaluate(() => window.__delta_tooltip.active_range())).toEqual({
    from: geometry.first + 1,
    to: geometry.second + 1,
    positive: expect.any(Boolean),
  });
  const comparison = await page.screenshot();
  expect(comparison.equals(hover)).toBe(false);
  expect(await page.evaluate(({ from, to }) => window.__delta_ranges.some((range) => range?.from === from && range?.to === to), {
    from: geometry.first + 1,
    to: geometry.second + 1,
  })).toBe(true);
  expect(await page.evaluate(() => window.__chart.chart_element().querySelectorAll(".nucleuscharts-delta-tooltip").length)).toBe(0);

  await page.mouse.up();
  expect(await page.evaluate(() => window.__delta_tooltip.active_range())).toMatchObject({
    from: geometry.first + 1,
    to: geometry.second + 1,
  });
  await page.mouse.move(geometry.left + geometry.x_middle, geometry.y);
  await expect.poll(() => page.evaluate(() => window.__delta_tooltip.active_range())).toBe(null);
  await page.evaluate(() => window.__delta_tooltip.detach());
});
