import { test, expect } from "@playwright/test";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import pixelmatch from "pixelmatch";
import { PNG } from "pngjs";

const fixture = JSON.parse(readFileSync(new URL("../fixtures/d1/candles.json", import.meta.url), "utf8"));
const reference_baseline = JSON.parse(readFileSync(new URL("../fixtures/d1/reference-baseline.json", import.meta.url), "utf8"));
const reference_matrix = JSON.parse(readFileSync(new URL("../fixtures/d1/reference-matrix.json", import.meta.url), "utf8"));
const reference_features = JSON.parse(readFileSync(new URL("../fixtures/d1/reference-features.json", import.meta.url), "utf8"));
const repository_root = fileURLToPath(new URL("../../..", import.meta.url));
const test_port = Number.parseInt(process.env.ORIGIN_TEST_PORT ?? "4174", 10);
const test_base_url = `http://127.0.0.1:${test_port}`;

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

async function capture_presented_frame(page, backend, extra_query = "") {
  await page.goto(`/?runtimeTest=presentedFrame&backend=${backend}&forceFallbackAdapter=1${extra_query}`);
  await wait_for_chart(page);
  return {
    backend: await page.evaluate(() => window.__chart.backend()),
    png: await page.screenshot({ animations: "disabled", fullPage: false }),
  };
}

function render_native_fixture(output) {
  const args = process.platform === "win32"
    ? ["+stable-x86_64-pc-windows-msvc", "run", "-p", "origin_native", "--example", "parity_fixture", "--", output]
    : ["run", "-p", "origin_native", "--example", "parity_fixture", "--", output];
  const result = spawnSync("cargo", args, { cwd: repository_root, encoding: "utf8" });
  expect(result.status, `native fixture failed\n${result.stdout}\n${result.stderr}`).toBe(0);
}

function rgba_diff(a, b, tolerance) {
  let different_pixels = 0;
  let maximum_channel_delta = 0;
  let absolute_channel_delta = 0;
  for (let offset = 0; offset < a.length; offset += 4) {
    let pixel_delta = 0;
    for (let channel = 0; channel < 4; channel += 1) {
      const delta = Math.abs(a[offset + channel] - b[offset + channel]);
      pixel_delta = Math.max(pixel_delta, delta);
      maximum_channel_delta = Math.max(maximum_channel_delta, delta);
      absolute_channel_delta += delta;
    }
    if (pixel_delta > tolerance) different_pixels += 1;
  }
  return {
    different_pixels,
    maximum_channel_delta,
    mean_absolute_channel_delta: absolute_channel_delta / a.length,
  };
}

function crop_png(source, x, y, width, height) {
  const output = new PNG({ width, height });
  PNG.bitblt(source, output, x, y, width, height, 0, 0);
  return output;
}

function image_stats(a, b) {
  expect([a.width, a.height]).toEqual([b.width, b.height]);
  const exact = rgba_diff(a.data, b.data, 0);
  const visual = new PNG({ width: a.width, height: a.height });
  const perceptual_pixels = pixelmatch(a.data, b.data, visual.data, a.width, a.height, {
    threshold: 0.1,
    includeAA: false,
  });
  return {
    ...exact,
    total_pixels: a.width * a.height,
    different_fraction: exact.different_pixels / (a.width * a.height),
    perceptual_pixels,
    perceptual_fraction: perceptual_pixels / (a.width * a.height),
    visual,
  };
}

function changed_footprint(base, feature) {
  let pixels = 0;
  let min_x = base.width;
  let min_y = base.height;
  let max_x = -1;
  let max_y = -1;
  for (let y = 0; y < base.height; y += 1) {
    for (let x = 0; x < base.width; x += 1) {
      const offset = (y * base.width + x) * 4;
      if (base.data[offset] === feature.data[offset]
        && base.data[offset + 1] === feature.data[offset + 1]
        && base.data[offset + 2] === feature.data[offset + 2]
        && base.data[offset + 3] === feature.data[offset + 3]) continue;
      pixels += 1;
      min_x = Math.min(min_x, x);
      min_y = Math.min(min_y, y);
      max_x = Math.max(max_x, x);
      max_y = Math.max(max_y, y);
    }
  }
  return { pixels, bounds: pixels === 0 ? null : { min_x, min_y, max_x, max_y } };
}

function regional_fidelity_report(origin, reference, pixel_ratio, price_axis_width = fixture.price_axis_width) {
  expect([origin.width, origin.height]).toEqual([reference.width, reference.height]);
  const pane_width = Math.round((fixture.css_width - price_axis_width) * pixel_ratio);
  const pane_height = Math.round((fixture.css_height - fixture.time_axis_height) * pixel_ratio);
  const regions = {
    full: [origin, reference],
    pane: [crop_png(origin, 0, 0, pane_width, pane_height), crop_png(reference, 0, 0, pane_width, pane_height)],
    price_axis: [
      crop_png(origin, pane_width, 0, origin.width - pane_width, pane_height),
      crop_png(reference, pane_width, 0, reference.width - pane_width, pane_height),
    ],
    time_axis: [
      crop_png(origin, 0, pane_height, pane_width, origin.height - pane_height),
      crop_png(reference, 0, pane_height, pane_width, reference.height - pane_height),
    ],
  };
  const report = {};
  const visuals = {};
  for (const [name, [origin_region, reference_region]] of Object.entries(regions)) {
    const stats = image_stats(origin_region, reference_region);
    report[name] = {
      total_pixels: stats.total_pixels,
      different_pixels: stats.different_pixels,
      different_percent: stats.different_fraction * 100,
      perceptual_pixels: stats.perceptual_pixels,
      perceptual_percent: stats.perceptual_fraction * 100,
      maximum_channel_delta: stats.maximum_channel_delta,
      mean_absolute_channel_delta: stats.mean_absolute_channel_delta,
    };
    visuals[name] = stats.visual;
  }
  return { report, visuals };
}

test("public screenshot is deterministic across live backends", async ({ page }) => {
  await page.goto("/?runtimeTest=backendParity&forceFallbackAdapter=1");
  await page.waitForFunction(() => document.documentElement.dataset.backendParity !== undefined);
  const result = await page.evaluate(() => window.__backend_parity_result);

  expect(result.status).toBe("passed");
  expect(result.screenshot_api.status).toBe("passed");
  expect(result.screenshot_api.different_pixels).toBe(0);
});

test("presented WebGPU and Canvas2D frames are pixel-identical", async ({ page }, test_info) => {
  const gpu = await capture_presented_frame(page, "auto");
  expect(gpu.backend, "This project is the WebGPU coverage gate; fallback is tested separately").toBe("webgpu");
  const canvas = await capture_presented_frame(page, "canvas2d");
  expect(canvas.backend).toBe("canvas2d");

  const gpu_image = PNG.sync.read(gpu.png);
  const canvas_image = PNG.sync.read(canvas.png);
  expect([gpu_image.width, gpu_image.height]).toEqual([canvas_image.width, canvas_image.height]);

  const diff = new PNG({ width: gpu_image.width, height: gpu_image.height });
  const different_pixels = pixelmatch(
    gpu_image.data,
    canvas_image.data,
    diff.data,
    gpu_image.width,
    gpu_image.height,
    { threshold: 0, includeAA: true },
  );

  if (different_pixels !== 0) {
    await test_info.attach("webgpu.png", { body: gpu.png, contentType: "image/png" });
    await test_info.attach("canvas2d.png", { body: canvas.png, contentType: "image/png" });
    await test_info.attach("diff.png", { body: PNG.sync.write(diff), contentType: "image/png" });
  }
  expect(different_pixels).toBe(0);

  // The same presented-frame gate with engine markers visible (?feature=markers) — the state
  // that exposed the WebGPU paint-order bug: markers are tri-family shapes emitted after the
  // quad-family candles, so both backends must paint them over the wicks/bodies.
  const marker_gpu = await capture_presented_frame(page, "auto", "&feature=markers");
  expect(marker_gpu.backend, "markers gate: WebGPU must stay active").toBe("webgpu");
  const marker_canvas = await capture_presented_frame(page, "canvas2d", "&feature=markers");
  expect(marker_canvas.backend).toBe("canvas2d");
  const gpu_markers = PNG.sync.read(marker_gpu.png);
  const canvas_markers = PNG.sync.read(marker_canvas.png);
  expect([gpu_markers.width, gpu_markers.height]).toEqual([canvas_markers.width, canvas_markers.height]);

  // The markers must actually paint on both backends, or the gate below is vacuous.
  const marker_pixels = { threshold: 0, includeAA: true };
  expect(
    pixelmatch(gpu_image.data, gpu_markers.data, null, gpu_image.width, gpu_image.height, marker_pixels),
    "engine markers must change the presented WebGPU frame",
  ).toBeGreaterThan(0);
  expect(
    pixelmatch(canvas_image.data, canvas_markers.data, null, canvas_image.width, canvas_image.height, marker_pixels),
    "engine markers must change the presented Canvas2D frame",
  ).toBeGreaterThan(0);

  // Ordering contract: zero pixels may differ by more than an AA coverage step. The shapes'
  // anti-aliased edges legitimately differ by 1-2 steps between SwiftShader's 4xMSAA and
  // Canvas2D's analytic coverage (measured max channel delta 66 on this fixture — not the
  // ordering contract's concern); a paint-order mismatch swaps whole marker/wick/body colors
  // (pre-fix: 67 pixels above this bound, up to 201). This keeps the strict 0-diff gate
  // above untouched while pinning the marker paint order exactly.
  let ordering_diff = 0;
  let edge_diff = 0;
  let maximum_channel_delta = 0;
  for (let offset = 0; offset < gpu_markers.data.length; offset += 4) {
    let pixel_delta = 0;
    for (let channel = 0; channel < 4; channel += 1) {
      pixel_delta = Math.max(pixel_delta, Math.abs(gpu_markers.data[offset + channel] - canvas_markers.data[offset + channel]));
    }
    maximum_channel_delta = Math.max(maximum_channel_delta, pixel_delta);
    if (pixel_delta > 96) ordering_diff += 1;
    else if (pixel_delta !== 0) edge_diff += 1;
  }
  console.log(`markers gate: ${edge_diff} AA-edge pixels (max step ${maximum_channel_delta}), ${ordering_diff} ordering pixels`);
  if (ordering_diff !== 0) {
    const marker_visual = new PNG({ width: gpu_markers.width, height: gpu_markers.height });
    pixelmatch(gpu_markers.data, canvas_markers.data, marker_visual.data, gpu_markers.width, gpu_markers.height, marker_pixels);
    await test_info.attach("webgpu-markers.png", { body: marker_gpu.png, contentType: "image/png" });
    await test_info.attach("canvas2d-markers.png", { body: marker_canvas.png, contentType: "image/png" });
    await test_info.attach("markers-diff.png", { body: PNG.sync.write(marker_visual), contentType: "image/png" });
  }
  expect(ordering_diff, "marker paint order must match Canvas2D (only AA coverage steps may differ)").toBe(0);
});

test("native and browser Canvas2D panes consume the same fixture", async ({ page }, test_info) => {
  await page.goto("/?runtimeTest=presentedFrame&backend=canvas2d");
  await wait_for_chart(page);
  const browser_data_url = await page.evaluate(() => window.__chart.take_screenshot().toDataURL("image/png"));
  const browser_full = PNG.sync.read(Buffer.from(browser_data_url.split(",")[1], "base64"));
  const pane_width = Math.round((fixture.css_width - fixture.price_axis_width) * fixture.pixel_ratio);
  const pane_height = Math.round((fixture.css_height - fixture.time_axis_height) * fixture.pixel_ratio);
  expect([browser_full.width, browser_full.height]).toEqual([
    Math.round(fixture.css_width * fixture.pixel_ratio),
    Math.round(fixture.css_height * fixture.pixel_ratio),
  ]);

  const browser_pane = new PNG({ width: pane_width, height: pane_height });
  PNG.bitblt(browser_full, browser_pane, 0, 0, pane_width, pane_height, 0, 0);
  const native_path = test_info.outputPath("native-pane.png");
  render_native_fixture(native_path);
  const native_pane = PNG.sync.read(readFileSync(native_path));
  expect([native_pane.width, native_pane.height]).toEqual([pane_width, pane_height]);

  // This is raw executor output on both sides, before browser compositor scaling, so the shared
  // Canvas2D command stream must be byte-exact even though the rasterizer implementations differ.
  const stats = rgba_diff(native_pane.data, browser_pane.data, 0);
  const fraction = stats.different_pixels / (pane_width * pane_height);
  console.log(`native/browser pane diff: ${stats.different_pixels}/${pane_width * pane_height} (${(fraction * 100).toFixed(4)}%), max ${stats.maximum_channel_delta}, mean ${stats.mean_absolute_channel_delta.toFixed(4)}`);

  if (stats.different_pixels !== 0) {
    const visual_diff = new PNG({ width: pane_width, height: pane_height });
    pixelmatch(native_pane.data, browser_pane.data, visual_diff.data, pane_width, pane_height, {
      threshold: 0.02,
      includeAA: true,
    });
    await test_info.attach("native-pane.png", { body: PNG.sync.write(native_pane), contentType: "image/png" });
    await test_info.attach("browser-pane.png", { body: PNG.sync.write(browser_pane), contentType: "image/png" });
    await test_info.attach("native-browser-diff.png", { body: PNG.sync.write(visual_diff), contentType: "image/png" });
  }
  expect(stats.different_pixels).toBe(0);
});

test("public time and price scale handles are engine-owned and reference-compatible", async ({ page }) => {
  await page.goto("/?runtimeTest=presentedFrame&backend=canvas2d");
  await wait_for_chart(page);
  const result = await page.evaluate(async () => {
    const chart = window.__chart;
    const main = window.__main;
    const data = window.__data;
    const time = chart.time_scale();
    const exact_time = data[100].time;
    const x = time.logical_to_coordinate(100);
    const time_result = {
      width: time.width(),
      height: time.height(),
      exact_index: time.time_to_index(exact_time),
      missing_index: time.time_to_index(exact_time + 1),
      nearest_index: time.time_to_index(exact_time + 1, true),
      logical_roundtrip: x === null ? null : time.coordinate_to_logical(x),
      time_coordinate_matches: x === time.time_to_coordinate(exact_time),
    };

    time.apply_options({ bar_spacing: 12, right_offset: 0 });
    time.scroll_to_position(0, false);
    const scrolled = time.scroll_position();
    time.reset_time_scale();
    const reset_options = time.options();
    // With reference `restoreDefault` semantics the applied bar_spacing 12 persists through the reset;
    // restore the fixture spacing for the downstream reference comparisons.
    time.apply_options({ bar_spacing: 6 });
    const query_logical_range = time.get_visible_logical_range();
    const queried_data = main.data();
    const data_scopes = [];
    const on_data_changed = (scope) => data_scopes.push(scope);
    main.subscribe_data_changed(on_data_changed);
    main.set_data(data);
    main.update(data[data.length - 1]);
    main.unsubscribe_data_changed(on_data_changed);
    const series_queries = {
      logical_range: query_logical_range,
      bars: query_logical_range === null ? null : main.bars_in_logical_range(query_logical_range),
      exact: main.data_by_index(100),
      missing: main.data_by_index(-1),
      nearest_right: main.data_by_index(-1, 1),
      length: queried_data.length,
      first: queried_data[0],
      last: queried_data[queried_data.length - 1],
      type: main.series_type(),
      data_scopes,
    };

    const price = main.price_scale();
    const chart_price = chart.price_scale("right");
    const initial_range = price.get_visible_range();
    price.set_visible_range({ from: 90, to: 140 });
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const manual_range = price.get_visible_range();
    const manual_options = price.options();
    price.apply_options({ invert_scale: true, scale_margins: { top: 0.25, bottom: 0.15 } });
    const changed_options = price.options();
    price.set_auto_scale(true);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const auto_range = price.get_visible_range();
    price.apply_options({ mode: 2 });
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const percentage_options = price.options();
    const source_price = data[900].close;
    const percentage_coordinate = main.price_to_coordinate(source_price);
    const percentage_roundtrip = percentage_coordinate === null
      ? null
      : main.coordinate_to_price(percentage_coordinate);
    const percentage_range = price.get_visible_range();
    const percentage_width = price.width();
    const percentage_logical_range = time.get_visible_logical_range();
    price.apply_options({ mode: 1 });
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const log_coordinate = main.price_to_coordinate(source_price);
    const log_roundtrip = log_coordinate === null ? null : main.coordinate_to_price(log_coordinate);
    const log_range = price.get_visible_range();
    price.apply_options({ mode: 3 });
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const indexed_coordinate = main.price_to_coordinate(source_price);
    const indexed_roundtrip = indexed_coordinate === null
      ? null
      : main.coordinate_to_price(indexed_coordinate);
    const indexed_range = price.get_visible_range();
    const indexed_logical_range = time.get_visible_logical_range();

    return {
      time_result,
      scrolled,
      reset_options,
      series_queries,
      initial_range,
      manual_range,
      manual_options,
      changed_options,
      auto_range,
      percentage_options,
      percentage_range,
      percentage_width,
      percentage_logical_range,
      percentage_coordinate,
      source_price,
      percentage_roundtrip,
      log_coordinate,
      log_roundtrip,
      log_range,
      indexed_coordinate,
      indexed_roundtrip,
      indexed_range,
      indexed_logical_range,
      price_width: price.width(),
      chart_price_width: chart_price.width(),
      overlay_width: chart.price_scale("").width(),
    };
  });

  expect(result.time_result.width).toBeGreaterThan(0);
  expect(result.time_result.height).toBe(fixture.time_axis_height);
  expect(result.time_result.exact_index).toBe(100);
  expect(result.time_result.missing_index).toBeNull();
  expect(result.time_result.nearest_index).toBe(101);
  expect(result.time_result.logical_roundtrip).toBe(100);
  expect(result.time_result.time_coordinate_matches).toBe(true);
  expect(result.scrolled).toBe(0);
  // reference `restoreDefault` restores from the *configured* options (time-scale.ts), so the
  // bar_spacing applied above (12) survives reset_time_scale — options() reports it back.
  expect(result.reset_options).toEqual({
    bar_spacing: 12,
    right_offset: 0,
    min_bar_spacing: 0.5,
    max_bar_spacing: 0,
    right_offset_pixels: null,
    time_visible: true,
    seconds_visible: false,
    fix_left_edge: false,
    fix_right_edge: false,
    lock_visible_time_range_on_resize: false,
    right_bar_stays_on_scroll: false,
    shift_visible_range_on_new_bar: true,
    allow_shift_visible_range_on_whitespace_replacement: false,
    allow_bold_labels: true,
    ticks_visible: false,
    minimum_height: 0,
    tick_mark_max_character_length: 8,
    visible: true,
  });
  expect(result.series_queries.length).toBe(fixture.bar_count);
  expect(result.series_queries.type).toBe("candlestick");
  expect(result.series_queries.missing).toBeNull();
  expect(result.series_queries.nearest_right).toEqual(result.series_queries.first);
  expect(result.series_queries.data_scopes).toEqual(["full", "update"]);
  expect(result.initial_range).not.toBeNull();
  expect(result.manual_range).toEqual({ from: 90, to: 140 });
  expect(result.manual_options.auto_scale).toBe(false);
  expect(result.manual_options.scale_margins).toEqual({ top: 0.2, bottom: 0.1 });
  expect(result.changed_options.invert_scale).toBe(true);
  expect(result.changed_options.scale_margins).toEqual({ top: 0.25, bottom: 0.15 });
  expect(result.auto_range).not.toEqual({ from: 90, to: 140 });
  expect(result.percentage_options.mode).toBe(2);
  expect(result.percentage_options.auto_scale).toBe(true);
  expect(result.percentage_range).not.toBeNull();
  expect(result.percentage_roundtrip).toBeCloseTo(result.source_price, 9);
  expect(result.log_roundtrip).toBeCloseTo(result.source_price, 8);
  expect(result.indexed_roundtrip).toBeCloseTo(result.source_price, 9);
  expect(result.price_width).toBeGreaterThan(0);
  expect(result.chart_price_width).toBe(result.price_width);
  expect(result.overlay_width).toBe(0);

  await page.goto("/reference.html");
  await page.waitForFunction(() => document.documentElement.dataset.ready === "true");
  const reference_modes = await page.evaluate(async (source_price) => {
    const { chart, series } = window.__reference;
    const scale = chart.priceScale("right");
    chart.timeScale().applyOptions({ barSpacing: 6, rightOffset: 0 });
    scale.applyOptions({
      mode: 2,
      invertScale: true,
      scaleMargins: { top: 0.25, bottom: 0.15 },
    });
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const percentage = {
      range: scale.getVisibleRange(),
      coordinate: series.priceToCoordinate(source_price),
      width: scale.width(),
      logical_range: chart.timeScale().getVisibleLogicalRange(),
    };
    scale.applyOptions({ mode: 1 });
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const log = {
      range: scale.getVisibleRange(),
      coordinate: series.priceToCoordinate(source_price),
    };
    scale.applyOptions({ mode: 3 });
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    return {
      percentage,
      log,
      indexed: {
        range: scale.getVisibleRange(),
        coordinate: series.priceToCoordinate(source_price),
        width: scale.width(),
        logical_range: chart.timeScale().getVisibleLogicalRange(),
      },
    };
  }, result.source_price);
  const reference_series_queries = await page.evaluate((logical_range) => {
    const { series } = window.__reference;
    const values = series.data();
    const bars = series.barsInLogicalRange(logical_range);
    return {
      bars: bars === null ? null : {
        bars_before: bars.barsBefore,
        bars_after: bars.barsAfter,
        ...(bars.from === undefined ? {} : { from: bars.from, to: bars.to }),
      },
      exact: series.dataByIndex(100),
      missing: series.dataByIndex(-1),
      nearest_right: series.dataByIndex(-1, 1),
      length: values.length,
      first: values[0],
      last: values[values.length - 1],
      type: series.seriesType().toLowerCase(),
    };
  }, result.series_queries.logical_range);
  expect(result.series_queries.bars).toEqual(reference_series_queries.bars);
  expect(result.series_queries.exact).toEqual(reference_series_queries.exact);
  expect(result.series_queries.missing).toEqual(reference_series_queries.missing);
  expect(result.series_queries.nearest_right).toEqual(reference_series_queries.nearest_right);
  expect(result.series_queries.length).toBe(reference_series_queries.length);
  expect(result.series_queries.first).toEqual(reference_series_queries.first);
  expect(result.series_queries.last).toEqual(reference_series_queries.last);
  expect(result.series_queries.type).toBe(reference_series_queries.type);
  expect(result.percentage_range.from).toBeCloseTo(reference_modes.percentage.range.from, 9);
  expect(result.percentage_range.to).toBeCloseTo(reference_modes.percentage.range.to, 9);
  expect(result.percentage_coordinate).toBeCloseTo(reference_modes.percentage.coordinate, 7);
  expect(result.percentage_width).toBe(reference_modes.percentage.width);
  expect(result.percentage_logical_range).toEqual(reference_modes.percentage.logical_range);
  expect(result.log_coordinate).toBeCloseTo(reference_modes.log.coordinate, 7);
  expect(result.log_range).toEqual(reference_modes.log.range);
  expect(result.indexed_range.from).toBeCloseTo(reference_modes.indexed.range.from, 9);
  expect(result.indexed_range.to).toBeCloseTo(reference_modes.indexed.range.to, 9);
  expect(result.indexed_coordinate).toBeCloseTo(reference_modes.indexed.coordinate, 7);
  expect(result.price_width).toBe(reference_modes.indexed.width);
  expect(result.indexed_logical_range).toEqual(reference_modes.indexed.logical_range);

  await page.goto("/?runtimeTest=presentedFrame&backend=canvas2d&leftScale=1&dpr=1");
  await wait_for_chart(page);
  const origin_left = await page.evaluate(() => {
    const chart = window.__chart;
    const series = window.__main;
    const source_price = window.__data[900].close;
    return {
      left_width: chart.price_scale("left").width(),
      right_width: chart.price_scale("right").width(),
      pane_width: chart.time_scale().width(),
      range: chart.price_scale("left").get_visible_range(),
      coordinate: series.price_to_coordinate(source_price),
      roundtrip: series.coordinate_to_price(series.price_to_coordinate(source_price)),
      logical_range: chart.time_scale().get_visible_logical_range(),
      source_price,
    };
  });
  expect(origin_left.left_width).toBeGreaterThan(0);
  expect(origin_left.right_width).toBe(0);
  expect(origin_left.pane_width + origin_left.left_width).toBe(fixture.css_width);
  expect(origin_left.roundtrip).toBeCloseTo(origin_left.source_price, 9);

  await page.goto("/reference.html?leftScale=1");
  await page.waitForFunction(() => document.documentElement.dataset.ready === "true");
  const reference_left = await page.evaluate((source_price) => {
    const { chart, series } = window.__reference;
    return {
      left_width: chart.priceScale("left").width(),
      right_width: chart.priceScale("right").width(),
      pane_width: chart.timeScale().width(),
      range: chart.priceScale("left").getVisibleRange(),
      coordinate: series.priceToCoordinate(source_price),
      logical_range: chart.timeScale().getVisibleLogicalRange(),
    };
  }, origin_left.source_price);
  expect(origin_left.left_width).toBe(reference_left.left_width);
  expect(origin_left.right_width).toBe(reference_left.right_width);
  expect(origin_left.pane_width).toBe(reference_left.pane_width);
  expect(origin_left.range).toEqual(reference_left.range);
  expect(origin_left.coordinate).toBeCloseTo(reference_left.coordinate, 7);
  expect(origin_left.logical_range).toEqual(reference_left.logical_range);
});

test("reference 5.2 reference is deterministic and reports Origin fidelity", async ({ page }, test_info) => {
  await page.goto("/?runtimeTest=presentedFrame&backend=canvas2d");
  await wait_for_chart(page);
  const origin = PNG.sync.read(await page.screenshot({ animations: "disabled", fullPage: false }));

  await page.goto("/reference.html");
  await page.waitForFunction(() => document.documentElement.dataset.ready === "true");
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  const capture_reference = async () => page.screenshot({ animations: "disabled", fullPage: false });
  const reference_first = await capture_reference();
  const reference_second = await capture_reference();
  expect(reference_second.equals(reference_first), "the pinned reference fixture must itself be deterministic").toBe(true);
  const reference = PNG.sync.read(reference_first);

  // Comparing presented pages puts both libraries through the same Chromium compositor and avoids
  // the unequal public screenshot resolutions (Origin is device-pixel-sized; reference is CSS-sized).
  const expected_size = [
    Math.round(fixture.css_width * fixture.pixel_ratio),
    Math.round(fixture.css_height * fixture.pixel_ratio),
  ];
  expect([origin.width, origin.height]).toEqual(expected_size);
  expect([reference.width, reference.height]).toEqual(expected_size);

  const { report, visuals } = regional_fidelity_report(origin, reference, fixture.pixel_ratio);
  await test_info.attach("origin.png", { body: PNG.sync.write(origin), contentType: "image/png" });
  await test_info.attach("reference-5.2.0.png", { body: PNG.sync.write(reference), contentType: "image/png" });
  await test_info.attach("origin-reference-diff.png", { body: PNG.sync.write(visuals.full), contentType: "image/png" });
  console.log(`Origin/reference 5.2 fidelity report: ${JSON.stringify(report)}`);
  await test_info.attach("origin-reference-report.json", {
    body: Buffer.from(JSON.stringify({ fixture: fixture.name, ref_version: "5.2.0", regions: report }, null, 2)),
    contentType: "application/json",
  });

  // This gate currently establishes a reproducible upstream reference and makes divergence
  // visible. The explicit ceilings prevent fidelity regressions and are lowered region-by-region
  // as Origin closes each measured gap; they are intentionally not represented as pixel parity.
  expect(reference_baseline.fixture).toBe(fixture.name);
  expect(reference_baseline.ref_version).toBe("5.2.0");
  for (const [name, ceiling] of Object.entries(reference_baseline.maximum_perceptual_difference)) {
    expect(report[name].perceptual_percent / 100, `${name} exceeded its recorded reference fidelity ceiling`).toBeLessThanOrEqual(ceiling);
  }
});

test("reference spacing, DPR, and theme matrix reports regional fidelity", async ({ browser }, test_info) => {
  expect(reference_matrix.fixture).toBe(fixture.name);
  expect(reference_matrix.ref_version).toBe("5.2.0");
  const cases = reference_matrix.cases;
  const matrix = {};
  for (const entry of cases) {
    const context = await browser.newContext({
      viewport: { width: fixture.css_width, height: fixture.css_height },
      deviceScaleFactor: entry.dpr,
      colorScheme: entry.theme,
    });
    const matrix_page = await context.newPage();
    const query = new URLSearchParams({
      runtimeTest: "presentedFrame",
      backend: "canvas2d",
      dpr: String(entry.dpr),
      spacing: String(entry.spacing),
      theme: entry.theme,
    });
    await matrix_page.goto(`${test_base_url}/?${query}`);
    await wait_for_chart(matrix_page);
    const origin_spacing = Number(await matrix_page.getAttribute("html", "data-bar-spacing"));
    expect(origin_spacing).toBeCloseTo(entry.spacing, 9);
    const origin_range = JSON.parse(await matrix_page.getAttribute("html", "data-visible-logical-range"));
    const origin_axis_width = Number(await matrix_page.getAttribute("html", "data-price-axis-width"));
    const origin_price_extent = await matrix_page.evaluate(() => [
      window.__chart.coordinate_to_price(0),
      window.__chart.coordinate_to_price(window.__chart.wasm.pane_height(0) - 1),
    ]);
    const origin = PNG.sync.read(await matrix_page.screenshot({ animations: "disabled", fullPage: false }));

    await matrix_page.goto(`${test_base_url}/reference.html?${query}`);
    await matrix_page.waitForFunction(() => document.documentElement.dataset.ready === "true");
    await matrix_page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    const reference_spacing = Number(await matrix_page.getAttribute("html", "data-bar-spacing"));
    expect(reference_spacing).toBeCloseTo(entry.spacing, 9);
    const reference_range = JSON.parse(await matrix_page.getAttribute("html", "data-visible-logical-range"));
    const reference_axis_width = Number(await matrix_page.getAttribute("html", "data-price-axis-width"));
    expect(origin_axis_width).toBe(reference_axis_width);
    const reference_price_extent = await matrix_page.evaluate(() => [
      window.__reference.series.coordinateToPrice(0),
      window.__reference.series.coordinateToPrice(window.__reference.chart.panes()[0].getHeight() - 1),
    ]);
    const reference = PNG.sync.read(await matrix_page.screenshot({ animations: "disabled", fullPage: false }));
    expect([origin.width, origin.height]).toEqual([
      Math.round(fixture.css_width * entry.dpr),
      Math.round(fixture.css_height * entry.dpr),
    ]);
    const { report, visuals } = regional_fidelity_report(origin, reference, entry.dpr, origin_axis_width);
    matrix[entry.name] = report;
    console.log(`${entry.name}: axis ${origin_axis_width}px, ranges Origin ${JSON.stringify(origin_range)} reference ${JSON.stringify(reference_range)}, price extents Origin ${JSON.stringify(origin_price_extent)} reference ${JSON.stringify(reference_price_extent)}; ${JSON.stringify(report)}`);
    if (entry.spacing === 50) {
      await test_info.attach(`${entry.name}-origin.png`, { body: PNG.sync.write(origin), contentType: "image/png" });
      await test_info.attach(`${entry.name}-reference.png`, { body: PNG.sync.write(reference), contentType: "image/png" });
      await test_info.attach(`${entry.name}-diff.png`, { body: PNG.sync.write(visuals.full), contentType: "image/png" });
    }
    await context.close();
  }
  await test_info.attach("origin-reference-matrix.json", {
    body: Buffer.from(JSON.stringify({ ref_version: "5.2.0", cases: matrix }, null, 2)),
    contentType: "application/json",
  });
  for (const entry of cases) {
    const report = matrix[entry.name];
    for (const [region, ceiling] of Object.entries(entry.maximum)) {
      expect(report[region].perceptual_percent / 100, `${entry.name}/${region} exceeded its reference ceiling`).toBeLessThanOrEqual(ceiling);
    }
  }
});

test("reference marker and overlay-volume fixtures report regional fidelity", async ({ browser }, test_info) => {
  expect(reference_features.fixture).toBe(fixture.name);
  expect(reference_features.ref_version).toBe("5.2.0");
  const feature_reports = {};
  const captures = {};
  for (const feature of ["base", "markers", "volume"]) {
    const context = await browser.newContext({
      viewport: { width: fixture.css_width, height: fixture.css_height },
      deviceScaleFactor: fixture.pixel_ratio,
      colorScheme: "light",
    });
    const feature_page = await context.newPage();
    const query = new URLSearchParams({
      runtimeTest: "presentedFrame",
      backend: "canvas2d",
      dpr: String(fixture.pixel_ratio),
      spacing: "6",
      theme: "light",
      feature,
    });
    await feature_page.goto(`${test_base_url}/?${query}`);
    await wait_for_chart(feature_page);
    const axis_width = Number(await feature_page.getAttribute("html", "data-price-axis-width"));
    const origin_range = JSON.parse(await feature_page.getAttribute("html", "data-visible-logical-range"));
    const origin_price_extent = await feature_page.evaluate(() => [
      window.__chart.coordinate_to_price(0),
      window.__chart.coordinate_to_price(window.__chart.wasm.pane_height(0) - 1),
    ]);
    const origin = PNG.sync.read(await feature_page.screenshot({ animations: "disabled", fullPage: false }));

    await feature_page.goto(`${test_base_url}/reference.html?${query}`);
    await feature_page.waitForFunction(() => document.documentElement.dataset.ready === "true");
    const reference_axis_width = Number(await feature_page.getAttribute("html", "data-price-axis-width"));
    const reference_range = JSON.parse(await feature_page.getAttribute("html", "data-visible-logical-range"));
    const reference_price_extent = await feature_page.evaluate(() => [
      window.__reference.series.coordinateToPrice(0),
      window.__reference.series.coordinateToPrice(window.__reference.chart.panes()[0].getHeight() - 1),
    ]);
    expect(axis_width).toBe(reference_axis_width);
    expect(origin_range).toEqual(reference_range);
    expect(origin_price_extent[0]).toBeCloseTo(reference_price_extent[0], 9);
    expect(origin_price_extent[1]).toBeCloseTo(reference_price_extent[1], 9);
    const reference = PNG.sync.read(await feature_page.screenshot({ animations: "disabled", fullPage: false }));
    const { report, visuals } = regional_fidelity_report(origin, reference, fixture.pixel_ratio, axis_width);
    captures[feature] = { origin, reference };
    console.log(`${feature} ranges: Origin ${JSON.stringify(origin_range)} reference ${JSON.stringify(reference_range)}, price extents Origin ${JSON.stringify(origin_price_extent)} reference ${JSON.stringify(reference_price_extent)}`);
    feature_reports[feature] = report;
    console.log(`${feature}: ${JSON.stringify(report)}`);
    await test_info.attach(`${feature}-origin.png`, { body: PNG.sync.write(origin), contentType: "image/png" });
    await test_info.attach(`${feature}-reference.png`, { body: PNG.sync.write(reference), contentType: "image/png" });
    await test_info.attach(`${feature}-diff.png`, { body: PNG.sync.write(visuals.full), contentType: "image/png" });
    await context.close();
  }
  const footprints = {};
  for (const feature of ["markers", "volume"]) {
    footprints[feature] = {
      origin: changed_footprint(captures.base.origin, captures[feature].origin),
      reference: changed_footprint(captures.base.reference, captures[feature].reference),
    };
  }
  console.log(`feature footprints: ${JSON.stringify(footprints)}`);
  await test_info.attach("origin-reference-features.json", {
    body: Buffer.from(JSON.stringify({ ref_version: "5.2.0", features: feature_reports, footprints }, null, 2)),
    contentType: "application/json",
  });
  for (const entry of reference_features.cases) {
    const report = feature_reports[entry.name];
    for (const [region, ceiling] of Object.entries(entry.maximum)) {
      expect(report[region].perceptual_percent / 100, `${entry.name}/${region} exceeded its reference ceiling`).toBeLessThanOrEqual(ceiling);
    }
  }
});
