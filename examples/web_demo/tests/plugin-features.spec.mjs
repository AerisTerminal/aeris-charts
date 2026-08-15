import { test, expect } from "@playwright/test";

async function open_chart(page) {
  await page.goto("/?runtimeTest=presentedFrame&backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
}

test("all first-class custom series features render through the public package", async ({ page }) => {
  const warnings = [];
  page.on("console", (message) => {
    if (message.type() === "warning") warnings.push(message.text());
  });
  await open_chart(page);
  const before = await page.screenshot();
  const result = await page.evaluate(async () => {
    const api = await import("/dist/nucleuscharts_financial.js");
    const bars = window.__data.slice(0, 20);
    const times = bars.map((bar) => bar.time);
    const definitions = [
      [api.create_brushable_area_series({ brush_ranges: [{ from_time: times[5], to_time: times[10], style: { line_color: "#f23645" } }] }), bars.map((bar) => ({ time: bar.time, value: bar.close }))],
      [api.create_dual_range_histogram_series(), bars.map((bar, index) => ({ time: bar.time, values: [index + 1, index / 2 + 1, index / 3 + 1, index / 4 + 1] }))],
      [api.create_grouped_bars_series(), bars.map((bar, index) => ({ time: bar.time, values: [index + 1, index + 3, index + 2] }))],
      [api.create_heatmap_series(), bars.map((bar, index) => ({ time: bar.time, cells: [{ low: bar.low, high: bar.close, amount: index * 5 }, { low: bar.close, high: bar.high, amount: 100 - index * 5 }] }))],
      [api.create_hlc_area_series(), bars.map((bar) => ({ time: bar.time, high: bar.high, low: bar.low, close: bar.close }))],
      [api.create_pretty_histogram_series({ base_price: 90 }), bars.map((bar) => ({ time: bar.time, value: bar.close }))],
      [api.create_lollipop_series({ base_price: 90 }), bars.map((bar) => ({ time: bar.time, value: bar.close }))],
      [api.create_rounded_candle_series(), bars.map((bar) => ({ ...bar, rounded: true }))],
      [api.create_shaded_background_series({ low_value: 90, high_value: 120 }), bars.map((bar) => ({ time: bar.time, value: bar.close }))],
      [api.create_stacked_area_series(), bars.map((bar, index) => ({ time: bar.time, values: [10 + index, 5 + index / 2, 3] }))],
      [api.create_stacked_bars_series(), bars.map((bar, index) => ({ time: bar.time, values: [10 + index, 5, 3] }))],
      [api.create_whisker_box_series(), bars.map((bar) => ({ time: bar.time, quartiles: [bar.low - 1, bar.low, bar.close, bar.high, bar.high + 1], outliers: [bar.high + 2] }))],
    ];
    const series = definitions.map(([view, data]) => {
      const custom = window.__chart.add_custom_series(view, { price_line_visible: false, last_value_visible: false });
      custom.set_data(data);
      return custom;
    });
    window.__chart.time_scale().fit_content();
    window.__chart.render();
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    return { count: series.length, kinds: series.map((item) => item.series_type()) };
  });
  const after = await page.screenshot();
  expect(result.count).toBe(12);
  expect(new Set(result.kinds)).toEqual(new Set(["custom"]));
  expect(after.equals(before)).toBe(false);
  expect(warnings.filter((warning) => warning.includes("custom series") || warning.includes("skipped"))).toEqual([]);
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

    api.create_anchored_text(chart, { logical: 12, price: start.close }, { text: "Anchor" });
    api.create_rectangle_drawing(chart, [{ logical: 14, price: start.low }, { logical: 20, price: end.high }]);
    api.create_trend_line(chart, [{ logical: 10, price: start.low }, { logical: 30, price: end.high }]);
    api.create_vertical_line(chart, { logical: 24, price: 0 });
    api.create_user_price_line(series, { price: start.close, title: "Level" });
    handles.push(api.create_partial_price_line(series, { price: end.close, from_time: start.time, to_time: end.time }));
    handles.push(api.create_session_highlighting(chart, series, { start_hour_utc: 0, end_hour_utc: 24 }));
    handles.push(api.create_highlight_bar_crosshair(chart, series));
    handles.push(api.create_volume_profile(series, bars.slice(0, 50).map((bar, index) => ({ price: bar.close, volume: index + 1 }))));
    api.create_bands_indicator(chart, series, 20, 2, { last_value_visible: false });

    const overlay = chart.add_series("line", { price_scale_id: "" });
    overlay.set_data(bars.slice(0, 20).map((bar) => ({ time: bar.time, value: bar.close })));
    const overlay_scale = api.use_overlay_price_scale(overlay);

    const image = document.createElement("canvas");
    image.width = 16;
    image.height = 16;
    image.getContext("2d").fillRect(0, 0, 16, 16);
    handles.push(api.create_image_watermark(chart.panes()[0], image, { width: 32, height: 32 }));
    handles.push(api.create_tooltip(chart));
    handles.push(api.create_delta_tooltip(chart, { series, anchor_on_click: false }));

    const alerts = api.create_expiring_price_alerts(series);
    alerts.add(start.close, "Persistent");
    alerts.add(end.close, "Expiring", Date.now() + 20);
    const user_alerts = api.create_user_price_alerts(chart, series);
    chart.chart_element().dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 20, clientY: 120 }));
    chart.chart_element().querySelector(".nucleuscharts-price-alert-menu button").click();

    const a11y = api.enable_accessibility(chart, { chart_title: "Test financial chart" });
    a11y.focus();
    chart.chart_element().dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true }));
    chart.set_crosshair_position(start.close, start.time, series);
    chart.render();
    await new Promise((resolve) => setTimeout(resolve, 40));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));

    const state = {
      drawings: chart.drawings().map((drawing) => drawing.kind()),
      overlay_has_range: overlay_scale.get_visible_range() !== null,
      tooltips: chart.chart_element().querySelectorAll(".nucleuscharts-tooltip,.nucleuscharts-delta-tooltip").length,
      role: chart.chart_element().getAttribute("role"),
      label: chart.chart_element().getAttribute("aria-label"),
      announcement: chart.chart_element().querySelector("[aria-live=assertive]")?.textContent ?? "",
      alerts: alerts.alerts().length,
      user_alerts: user_alerts.alerts().length,
    };
    a11y.detach();
    alerts.detach();
    user_alerts.detach();
    handles.forEach((handle) => handle.detach());
    return state;
  });
  expect(result.drawings).toEqual(expect.arrayContaining(["text", "rectangle", "trend_line", "vertical_line"]));
  expect(result.overlay_has_range).toBe(true);
  expect(result.tooltips).toBe(2);
  expect(result.role).toBe("application");
  expect(result.label).toBe("Test financial chart");
  expect(result.announcement).toContain("Point");
  expect(result.alerts).toBe(1);
  expect(result.user_alerts).toBe(1);
  expect(page_errors).toEqual([]);
});
