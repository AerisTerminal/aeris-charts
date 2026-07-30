import { test, expect } from "@playwright/test";
import { readFileSync } from "node:fs";
import pixelmatch from "pixelmatch";
import { PNG } from "pngjs";

const fixture = JSON.parse(readFileSync(new URL("../fixtures/d1/candles.json", import.meta.url), "utf8"));

test.beforeEach(async ({ page }) => {
  page.on("console", (message) => console.log(`[browser:${message.type()}] ${message.text()}`));
  page.on("pageerror", (error) => console.log(`[browser:pageerror] ${error.message}`));
});

async function wait_for_chart(page) {
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined);
  await page.evaluate(() => new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  }));
}

async function settle_frames(page) {
  await page.evaluate(() => new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  }));
}

async function goto_fixture(page, backend, feature, spacing) {
  const feature_param = feature ? `&feature=${feature}` : "";
  const spacing_param = spacing ? `&spacing=${spacing}` : "";
  await page.goto(`/?runtimeTest=presentedFrame&backend=${backend}&forceFallbackAdapter=1${feature_param}${spacing_param}`);
  await wait_for_chart(page);
}

function crop_png(source, x, y, width, height) {
  const output = new PNG({ width, height });
  PNG.bitblt(source, output, x, y, width, height, 0, 0);
  return output;
}

function count_different(a, b) {
  expect([a.width, a.height]).toEqual([b.width, b.height]);
  return pixelmatch(a.data, b.data, new PNG({ width: a.width, height: a.height }).data, a.width, a.height, {
    threshold: 0,
    includeAA: true,
  });
}

function max_channel_delta(a, b) {
  let max_delta = 0;
  for (let i = 0; i < a.data.length; i += 1) {
    max_delta = Math.max(max_delta, Math.abs(a.data[i] - b.data[i]));
  }
  return max_delta;
}

// (a) The custom series (the ported reference rounded-candles plugin example) records the same Prim
// commands once per frame, so WebGPU and Canvas2D present pixel-identical frames with it active.
// Bar spacing 3 puts the example's `radius` rule at 0, so its bodies emit crisp quad-family
// rects — the family both backends rasterize bit-exactly (nonzero radii take the tessellated
// RoundRect path, which this repo deliberately does not assert 0-diff on; see the pane/series
// primitive references).
test("custom series paints identically on both backends with the plugin active", async ({ page }, test_info) => {
  await goto_fixture(page, "canvas2d", null, 3);
  const plain = PNG.sync.read(await page.screenshot({ animations: "disabled", fullPage: false }));
  await goto_fixture(page, "canvas2d", "custom_series", 3);
  expect(await page.evaluate(() => window.__custom_series_active())).toBe(true);
  const canvas_active = PNG.sync.read(await page.screenshot({ animations: "disabled", fullPage: false }));

  // The plugin actually painted: the pane region (rounded candles over the built-in ones) changed.
  const pane_width = Math.round((fixture.css_width - fixture.price_axis_width) * fixture.pixel_ratio);
  const pane_height = Math.round((fixture.css_height - fixture.time_axis_height) * fixture.pixel_ratio);
  const painted = count_different(
    crop_png(plain, 0, 0, pane_width, pane_height),
    crop_png(canvas_active, 0, 0, pane_width, pane_height),
  );
  expect(painted, "the custom series must draw into the pane").toBeGreaterThan(0);

  await goto_fixture(page, "auto", "custom_series", 3);
  expect(await page.evaluate(() => window.__chart.backend())).toBe("webgpu");
  expect(await page.evaluate(() => window.__custom_series_active())).toBe(true);
  const gpu_active = PNG.sync.read(await page.screenshot({ animations: "disabled", fullPage: false }));
  const pane_backend_diff = count_different(
    crop_png(gpu_active, 0, 0, pane_width, pane_height),
    crop_png(canvas_active, 0, 0, pane_width, pane_height),
  );
  expect(pane_backend_diff, "the custom series' owning pane must remain pixel-identical").toBe(0);

  // Shared axis text uses identical geometry and bounded backend-specific AA at fractional DPR.
  const backend_diff = count_different(gpu_active, canvas_active);
  const backend_max_delta = max_channel_delta(gpu_active, canvas_active);
  if (backend_diff > 5_000 || backend_max_delta > 64) {
    await test_info.attach("webgpu.png", { body: PNG.sync.write(gpu_active), contentType: "image/png" });
    await test_info.attach("canvas2d.png", { body: PNG.sync.write(canvas_active), contentType: "image/png" });
  }
  expect(backend_diff, "full-frame differences must stay confined to bounded AA edges").toBeLessThanOrEqual(5_000);
  expect(backend_max_delta).toBeLessThanOrEqual(64);
});

// (b) Autoscale: the custom series' `price_value_builder` values drive its price scale through
// the contribution path — hiding the series drops them back off the scale (public handles only).
test("custom series autoscale contains the items' price extent and drops when hidden", async ({ page }) => {
  await goto_fixture(page, "canvas2d", "custom_series");
  // Shift the custom items far above the candles so only the custom contribution can reach them.
  await page.evaluate(() => {
    window.__custom.set_data(window.__data.map((bar) => ({
      time: bar.time, open: bar.open + 50, high: bar.high + 50, low: bar.low + 50, close: bar.close + 50,
    })));
    window.__custom.apply_options({ visible: false });
  });
  await settle_frames(page);
  const extent = await page.evaluate(() => ({
    min_low: Math.min(...window.__data.map((bar) => bar.low)),
    max_high: Math.max(...window.__data.map((bar) => bar.high)),
  }));

  const plain_range = await page.evaluate(() => window.__chart.price_scale("right").get_visible_range());
  expect(plain_range).not.toBeNull();
  expect(plain_range.to, "without the custom series the scale must stay below its +50 extent")
    .toBeLessThan(extent.max_high + 50);

  await page.evaluate(() => window.__custom.apply_options({ visible: true }));
  await settle_frames(page);
  const expanded_range = await page.evaluate(() => window.__custom.price_scale().get_visible_range());
  expect(expanded_range).not.toBeNull();
  // The public handle returns the raw scale range (margins apply to coordinates, not the range),
  // so exact containment of the custom items' extent is the assertion.
  expect(expanded_range.to, "the scale must contain the custom items' highs")
    .toBeGreaterThanOrEqual(extent.max_high + 50);
  expect(expanded_range.from, "the scale must still contain the candle lows")
    .toBeLessThanOrEqual(extent.min_low);

  await page.evaluate(() => window.__custom.apply_options({ visible: false }));
  await settle_frames(page);
  const restored_range = await page.evaluate(() => window.__chart.price_scale("right").get_visible_range());
  expect(restored_range.to, "hiding the custom series drops its contribution again")
    .toBeLessThan(extent.max_high + 50);
});

// (c) set_data/update/data round-trip the raw items through the same sort/dedupe/update rules
// as the built-ins (alignment between the host items and the engine's time-only rows). The
// custom times share the fixture's merged time axis, so their logical indices follow the
// fixture's 1000 bars.
test("custom series data()/update() round-trip items through sort, dedupe, and streaming update", async ({ page }) => {
  await goto_fixture(page, "canvas2d");
  const result = await page.evaluate(() => {
    const series = window.__chart.add_custom_series({
      price_value_builder: (item) => [item.value],
      render() {},
    });
    const t1 = 1767225600; // the fixture's last bar time (merged index 999)
    const t2 = t1 + 3600;
    const t3 = t2 + 3600;
    // Unsorted, duplicated (last wins), and one non-finite-time row (dropped with a warning).
    series.set_data([
      { time: t2, value: 2, tag: "first-t2" },
      { time: t1, value: 1 },
      { time: t2, value: 3, tag: "last-t2" },
      { time: NaN, value: 9 },
    ]);
    const after_set = series.data().map((item) => ({ ...item }));
    series.update({ time: t3, value: 4 });
    series.update({ time: t1, value: 7 });
    const after_update = series.data().map((item) => ({ ...item }));
    const t2_index = window.__chart.time_scale().time_to_index(t2);
    const by_index = { ...series.data_by_index(t2_index, 0) };
    const last = series.last_value_data(true);
    return { after_set, after_update, by_index, last, series_type: series.series_type(), t1, t2, t3 };
  });
  expect(result.series_type).toBe("custom");
  expect(result.after_set).toEqual([
    { time: result.t1, value: 1 },
    { time: result.t2, value: 3, tag: "last-t2" },
  ]);
  expect(result.after_update).toEqual([
    { time: result.t1, value: 7 },
    { time: result.t2, value: 3, tag: "last-t2" },
    { time: result.t3, value: 4 },
  ]);
  expect(result.by_index).toEqual({ time: result.t2, value: 3, tag: "last-t2" });
  // The last non-whitespace item's LAST price_value_builder element is the last value.
  expect(result.last.value).toBe(4);
  expect(result.last.time).toBe(result.t3);
});

// (d) A whitespace item renders nothing at its slot: the plugin never receives it, and the
// slot's pixels equal the series-hidden frame exactly (scale pinned so the grid cannot shift;
// the built-in last-price chrome is off so toggling visibility changes nothing outside bars).
test("a whitespace item renders nothing at its slot", async ({ page }) => {
  const pixel_ratio = fixture.pixel_ratio;
  const pane_height = Math.round((fixture.css_height - fixture.time_axis_height) * pixel_ratio);
  await goto_fixture(page, "canvas2d", "custom_series");
  await page.evaluate(() => {
    const t0 = window.__data[100].time;
    const items = [];
    for (let i = 0; i < 8; i += 1) {
      items.push({ time: t0 + i * 86400, open: 100 + i, high: 103 + i, low: 97 + i, close: 102 + i });
    }
    items[4] = { time: items[4].time }; // the whitespace slot
    window.__ws_time = items[4].time;
    window.__main.apply_options({ visible: false });
    window.__custom.apply_options({ price_line_visible: false, last_value_visible: false });
    window.__custom.set_data(items);
    // Pin the scale so toggling the custom series cannot shift the grid between captures.
    window.__chart.price_scale("right").set_visible_range({ from: 90, to: 115 });
    window.__chart.time_scale().fit_content();
  });
  await settle_frames(page);

  // The plugin never receives the whitespace item (the engine treats the slot as a gap).
  const rendered = await page.evaluate(() => window.__custom_render_items.map((point) => point.time));
  const ws_time = await page.evaluate(() => window.__ws_time);
  expect(rendered.length).toBe(7);
  expect(rendered).not.toContain(ws_time);

  const with_whitespace = PNG.sync.read(await page.screenshot({ animations: "disabled", fullPage: false }));
  await page.evaluate(() => window.__custom.apply_options({ visible: false }));
  await settle_frames(page);
  const series_hidden = PNG.sync.read(await page.screenshot({ animations: "disabled", fullPage: false }));
  // The whitespace slot's pixels equal the series-hidden frame exactly — nothing renders there.
  const x_css = await page.evaluate((time) => window.__chart.time_scale().time_to_coordinate(time), ws_time);
  expect(x_css).not.toBeNull();
  const center = Math.round(x_css * pixel_ratio);
  const strip = (png) => crop_png(png, center - 8, 0, 16, pane_height);
  expect(count_different(strip(with_whitespace), strip(series_hidden))).toBe(0);

  // A real item at the same slot paints into it (a body inside the pinned range).
  await page.evaluate(() => {
    window.__custom.apply_options({ visible: true });
    const items = window.__custom.data().map((item) => ({ ...item }));
    items[4] = { time: window.__ws_time, open: 101, high: 104, low: 98, close: 103 };
    window.__custom.set_data(items);
  });
  await settle_frames(page);
  const with_item = PNG.sync.read(await page.screenshot({ animations: "disabled", fullPage: false }));
  expect(count_different(strip(with_whitespace), strip(with_item))).toBeGreaterThan(0);
});

// (e) Removing the custom series drops its host entry (firing the view's `destroy` hook) and
// re-adding works — the cycle leaves no page errors.
test("remove_series cleans the custom series up (destroy fires) and re-adding works", async ({ page }) => {
  const page_errors = [];
  page.on("pageerror", (error) => page_errors.push(error.message));
  await goto_fixture(page, "canvas2d", "custom_series");
  const active_before = PNG.sync.read(await page.screenshot({ animations: "disabled", fullPage: false }));

  await page.evaluate(() => window.__set_custom_series(false));
  await settle_frames(page);
  expect(await page.evaluate(() => window.__custom_destroy_count())).toBe(1);
  expect(await page.evaluate(() => window.__custom)).toBeNull();
  expect(await page.evaluate(() => window.__custom_series_active())).toBe(false);
  const removed = PNG.sync.read(await page.screenshot({ animations: "disabled", fullPage: false }));

  await page.evaluate(() => window.__set_custom_series(true));
  await settle_frames(page);
  expect(await page.evaluate(() => window.__custom_series_active())).toBe(true);
  expect(await page.evaluate(() => window.__custom.data().length)).toBe(1000);
  const re_added = PNG.sync.read(await page.screenshot({ animations: "disabled", fullPage: false }));
  expect(count_different(removed, re_added), "the re-added custom series must paint again").toBeGreaterThan(0);
  expect(count_different(active_before, re_added), "same view + same data repaints the same frame").toBe(0);

  expect(page_errors, "no page errors across the remove/re-add cycle").toEqual([]);
});
