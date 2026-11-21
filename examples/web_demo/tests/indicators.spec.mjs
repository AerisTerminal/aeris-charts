import { expect } from "@playwright/test";
import { PNG } from "pngjs";
import { test, wait_for_chart } from "./page_ready.mjs";

// Engine-native indicators: Bollinger band fill, oscillator separate panes with channel strips,
// MACD four-state histogram colors, and the full native set's placement/lineage.

async function wait_grid(page) {
  await wait_for_chart(page, { grid: true });
}

test("page readiness reports a failed static import instead of timing out", async ({ page }) => {
  await page.route("**/fixture_features.js", (route) => route.abort("internetdisconnected"));
  await page.goto("/");
  await expect(wait_grid(page)).rejects.toThrow(/fixture_features\.js.*ERR_INTERNET_DISCONNECTED/);
});

test("page readiness reports a module error instead of timing out", async ({ page }) => {
  await page.route("**/fixture_features.js", (route) =>
    route.fulfill({ contentType: "text/javascript", body: "export const marker_fixture = null; export const volume_fixture = null; throw new Error('fixture module crashed')" }));
  await page.goto("/");
  await expect(wait_grid(page)).rejects.toThrow(/fixture module crashed/);
});

function count_color(png, target, tol = 10) {
  let n = 0;
  for (let o = 0; o < png.data.length; o += 4) {
    if (
      Math.abs(png.data[o] - target[0]) <= tol &&
      Math.abs(png.data[o + 1] - target[1]) <= tol &&
      Math.abs(png.data[o + 2] - target[2]) <= tol
    ) n += 1;
  }
  return n;
}

test("volume studies align sparse volume, repair history, and place volume in a separate pane", async ({ page }) => {
  await page.goto("/");
  await wait_grid(page);
  const result = await page.evaluate(() => {
    const chart = window.__chart;
    const source = chart.add_series("candlestick", { visible: false });
    const volume = chart.add_series("histogram", { visible: false });
    const start = 1_700_000_000;
    source.set_data([
      { time: start, open: 10, high: 12, low: 8, close: 11 },
      { time: start + 60, open: 11, high: 14, low: 10, close: 13 },
      { time: start + 120, open: 13, high: 15, low: 11, close: 15 },
    ]);
    volume.set_data([{ time: start, value: 10 }, { time: start + 120, value: 30 }]);
    const obv = chart.add_obv(source, volume);
    const relative = chart.add_relative_volume(source, 2, volume);
    const cmf = chart.add_cmf(source, 2, volume);
    const mfi = chart.add_mfi(source, 2, volume);
    const [vol_hist, vol_ma] = chart.add_volume(source, 2, volume);
    const studies = [obv, relative, cmf, mfi, vol_hist, vol_ma];
    const before = studies.map((output) => output.data().at(-1)?.value);
    volume.update({ time: start + 60, value: 20 });
    return {
      before,
      after: studies.map((output) => output.data().at(-1)?.value),
      kinds: studies.map((output) => output.indicator_info().kind),
      volume_ids: studies.map((output) => output.indicator_info().volume_source.id),
      volume_panes: [vol_hist.pane_index(), vol_ma.pane_index()],
      source_pane: source.pane_index(),
      volume_id: volume.id,
    };
  });
  expect(result.kinds).toEqual(["obv", "relative_volume", "cmf", "mfi", "volume", "volume"]);
  expect(result.volume_ids).toEqual(Array(6).fill(result.volume_id));
  expect(result.volume_panes[0]).toBeGreaterThan(result.source_pane);
  expect(result.volume_panes[1]).toBe(result.volume_panes[0]);
  expect(result.after[0] - result.before[0]).toBeCloseTo(20, 8);
  expect(result.before[1]).toBeCloseTo(6, 8);
  expect(result.after[1]).toBeCloseTo(2, 8);
  expect(result.before[2]).toBeCloseTo(1, 8);
  expect(result.after[2]).toBeCloseTo(0.8, 8);
  expect(result.before[3]).toBeCloseTo(100, 8);
  expect(result.after[3]).toBeCloseTo(100, 8);
  expect(result.before[4]).toBe(30);
  expect(result.after[4]).toBe(30);
  expect(result.before[5]).toBeCloseTo(15, 8);
  expect(result.after[5]).toBeCloseTo(25, 8);
});

test("indicator sources recover from empty, short, and retention-trimmed warm-up", async ({ page }) => {
  await page.goto("/");
  await wait_grid(page);
  const result = await page.evaluate(() => {
    const rows = Array.from({ length: 20 }, (_, index) => ({
      time: 1_700_000_000 + index * 60,
      value: 100 + index,
    }));
    const repopulate = (replacement_rows) => {
      const source = window.__chart.add_series("line", { visible: false });
      source.set_data(rows);
      const sma = window.__chart.add_sma(source, 20);
      source.set_data(rows.slice(0, replacement_rows));
      const empty_rows = sma.data().length;
      for (const row of rows.slice(replacement_rows)) source.update(row);
      const output = sma.data();
      window.__chart.remove_series(source);
      return { empty_rows, output };
    };

    const retained = window.__chart.add_series("line", { visible: false });
    retained.set_data(rows);
    const retained_sma = window.__chart.add_sma(retained, 20);
    retained.apply_options({ max_points: 1 });
    for (let index = 20; index < 40; index += 1) {
      retained.update({ time: 1_700_000_000 + index * 60, value: 100 + index });
    }
    return {
      empty: repopulate(0),
      short: repopulate(10),
      retained_rows: retained_sma.data().length,
    };
  });

  for (const replacement of [result.empty, result.short]) {
    expect(replacement.empty_rows).toBe(0);
    expect(replacement.output).toEqual([{ time: 1_700_001_140, value: 109.5 }]);
  }
  expect(result.retained_rows).toBe(0);
});

test("RSI and EMA continue through an interior whitespace item across append and correction", async ({ page }) => {
  await page.goto("/?backend=canvas2d");
  await wait_grid(page);
  const result = await page.evaluate(() => {
    const chart = window.__chart;
    const source = chart.add_series("line", { visible: false });
    const rows = Array.from({ length: 25 }, (_, index) => ({
      time: 1_701_000_000 + index * 60,
      ...(index === 9 ? {} : { value: 100 + index * 0.15 + Math.sin(index * 0.7) * 4 }),
    }));
    source.set_data(rows);
    const rsi = chart.add_rsi(source, 4);
    const ema = chart.add_ema(source, 4);
    const check = () => ({ rows: rows.map((row) => ({ ...row })), rsi: rsi.data(), ema: ema.data() });
    const stages = [check()];
    const append = { time: 1_701_000_000 + 25 * 60, value: 112.5 };
    rows.push(append);
    source.update(append);
    stages.push(check());
    const correction = { ...rows[5], value: 95.25 };
    rows[5] = correction;
    source.update(correction);
    stages.push(check());
    return stages;
  });
  for (const { rows, rsi, ema } of result) {
    const valid = rows.filter((row) => row.value !== undefined);
    const expected = { rsi: new Map(), ema: new Map() };
    let avg, gain = 0, loss = 0;
    for (let i = 0; i < valid.length; i++) {
      if (i === 3) avg = valid.slice(0, 4).reduce((sum, row) => sum + row.value, 0) / 4;
      else if (i > 3) avg += (valid[i].value - avg) * 0.4;
      if (i >= 3) expected.ema.set(valid[i].time, avg);
      if (i > 0) {
        const change = valid[i].value - valid[i - 1].value;
        if (i <= 4) {
          gain += Math.max(change, 0) / 4;
          loss += Math.max(-change, 0) / 4;
        } else {
          gain = (gain * 3 + Math.max(change, 0)) / 4;
          loss = (loss * 3 + Math.max(-change, 0)) / 4;
        }
        if (i >= 4) expected.rsi.set(valid[i].time,
          loss === 0 ? (gain === 0 ? 50 : 100) : 100 - 100 / (1 + gain / loss));
      }
    }
    for (const [kind, actual] of [["rsi", rsi], ["ema", ema]]) {
      expect(actual.find((row) => row.time === rows[9].time)?.value).toBeUndefined();
      for (const row of actual) {
        const value = expected[kind].get(row.time);
        if (value === undefined) expect(row.value).toBeUndefined();
        else expect(Math.abs(row.value - value)).toBeLessThan(1e-9 * Math.max(1, Math.abs(value)));
      }
      expect(actual.at(-1).value).toBeDefined();
    }
  }
});

/** Chart screenshot decoded to a PNG plus a row-crop counter (geometry is CSS px, shots are device px). */
async function shot(page) {
  const { url, chart_width } = await page.evaluate(() => ({
    url: window.__chart.take_screenshot().toDataURL("image/png"),
    chart_width: document.getElementById("chart_container").getBoundingClientRect().width,
  }));
  const png = PNG.sync.read(Buffer.from(url.split(",")[1], "base64"));
  // The demo has a dockable inspector, so the chart bitmap no longer necessarily spans the
  // viewport. Pane geometry is in chart-local CSS px; derive its scale from the actual chart.
  const dsf = png.width / chart_width;
  const crop = (top_css, bottom_css) => {
    const top = Math.max(0, Math.floor(top_css * dsf));
    const bottom = Math.min(png.height, Math.ceil(bottom_css * dsf));
    const sub = new PNG({ width: png.width, height: bottom - top });
    PNG.bitblt(png, sub, 0, top, png.width, bottom - top, 0, 0);
    return sub;
  };
  return { png, dsf, crop };
}

test("bollinger bands paint their background fill between upper and lower", async ({ page }) => {
  // Pin the light theme: the expected fill is #2196f3 at 0.2 alpha over white.
  await page.goto("/?theme=light");
  await wait_grid(page);
  await page.evaluate(() => {
    window.__bands = window.__chart.add_bollinger(window.__main, 20, 2);
  });
  await wait_grid(page);
  const { png } = await shot(page);
  // Band color #2196f3 at 0.2 alpha over the white background blends to ~(211, 234, 253).
  expect(count_color(png, [211, 234, 253], 8), "band fill pixels").toBeGreaterThan(200);
});

test("rsi and macd stack their own panes with channel strip and four-state histogram", async ({ page }) => {
  await page.goto("/");
  await wait_grid(page);

  const added = await page.evaluate(() => {
    const events = [];
    window.__chart.subscribe_series_added((e) => events.push({ id: e.series.id, pane: e.pane_index }));
    window.__rsi = window.__chart.add_rsi(window.__main, 14);
    window.__macd = window.__chart.add_macd(window.__main, 12, 26, 9);
    return {
      events: events.length,
      rsi_pane: events[0].pane,
      macd_panes: events.slice(1).map((e) => e.pane),
      rsi_kind: window.__rsi.indicator_info().kind,
      hist_type: window.__macd[2].series_type(),
      hist_slot: window.__macd[2].indicator_info().output_index,
      panes: window.__chart.panes().length,
    };
  });
  expect(added.events).toBe(4); // 1 rsi + 3 macd outputs
  expect(added.rsi_pane).toBe(1);
  expect(added.macd_panes).toEqual([2, 2, 2]);
  expect(added.rsi_kind).toBe("rsi");
  expect(added.hist_type).toBe("histogram");
  expect(added.hist_slot).toBe(2);
  expect(added.panes).toBe(3);

  await wait_grid(page);
  const geo = await page.evaluate(() => window.__chart.panes().map((p) => p.get_geometry()));
  const { crop } = await shot(page);
  // RSI channel strip rgba(120,123,134,0.2) over the canonical #171717 dark surface.
  const rsi_strip = count_color(crop(geo[1].top, geo[1].top + geo[1].height), [42, 43, 45], 6);
  expect(rsi_strip, "rsi 30/70 channel strip pixels").toBeGreaterThan(500);
  // MACD histogram strong-up columns paint the opaque candle green.
  const macd_green = count_color(crop(geo[2].top, geo[2].top + geo[2].height), [8, 153, 129], 10);
  expect(macd_green, "macd histogram strong-state pixels").toBeGreaterThan(20);
});

test("indicator chips: auto-name on, no countdown, 2px default, style overrides", async ({ page }) => {
  await page.goto("/");
  await wait_grid(page);
  const out = await page.evaluate(() => {
    const rsi = window.__chart.add_rsi(window.__main, 14);
    const before = rsi.options();
    // The platform surfaces its own chip: custom name, chip visible, dotted 1px override.
    rsi.apply_options({ title: "RSI(14) 1h", title_visible: true, line_style: 1, line_width: 1 });
    const after = rsi.options();
    return {
      before: {
        title: before.title,
        title_visible: before.title_visible,
        countdown: before.countdown_visible,
        width: before.line_width,
        price_line_visible: before.price_line_visible,
        price_line_extent: before.price_line_extent,
        price_line_width: before.price_line_width,
        price_line_style: before.price_line_style,
      },
      after: {
        title: after.title,
        title_visible: after.title_visible,
        line_style: after.line_style,
        line_width: after.line_width,
      },
    };
  });
  expect(out.before).toEqual({
    title: "RSI 14",
    title_visible: true,
    countdown: false,
    width: 2,
    price_line_visible: false,
    price_line_extent: "partial",
    price_line_width: 1,
    price_line_style: 1,
  });
  expect(out.after).toEqual({ title: "RSI(14) 1h", title_visible: true, line_style: 1, line_width: 1 });
});

test("demo-created indicators and feature companions never enable candle countdowns", async ({ page }) => {
  await page.goto("/");
  await wait_grid(page);
  const outputs = await page.evaluate(() => {
    window.__demo_catalogs.series.activate("shaded-background");
    window.__demo_catalogs.lab.activate("overlay-scale");
    document.getElementById("sma_toggle").click(); // Catalog indicators are created on demand.
    document.getElementById("rsi_toggle").click();
    return window.__chart.series_order()
      .filter((series) => series !== window.__main)
      .map((series) => ({
        type: series.series_type(),
        title: series.options().title,
        countdown_visible: series.options().countdown_visible,
      }));
  });
  expect(outputs.length).toBeGreaterThanOrEqual(5);
  expect(outputs.every((series) => series.countdown_visible === false)).toBe(true);
});

test("crosshair hover marker paints on an engine-created indicator line and remains configurable", async ({ page }) => {
  await page.goto("/");
  await wait_grid(page);
  const probe = await page.evaluate(() => {
    const default_sma = window.__chart.add_sma(window.__main, 5);
    const default_visible = default_sma.options().crosshair_marker_visible;
    window.__chart.remove_series(default_sma);
    window.__hover_sma = window.__chart.add_sma(window.__main, 5, {
      color: "#ff9900",
      crosshair_marker_visible: true,
      crosshair_marker_radius: 6,
      crosshair_marker_border_width: 3,
      crosshair_marker_border_color: "#010203",
      crosshair_marker_background_color: "#040506",
    });
    const range = window.__chart.time_scale().get_visible_logical_range();
    const index = Math.floor((range.from + range.to) / 2);
    const point = window.__hover_sma.data_by_index(index);
    window.__chart.set_crosshair_position(point.value, point.time, window.__hover_sma);
    const options = window.__hover_sma.options();
    return {
      x: window.__chart.time_scale().logical_to_coordinate(index),
      y: window.__hover_sma.price_to_coordinate(point.value),
      default_visible,
      options: {
        visible: options.crosshair_marker_visible,
        radius: options.crosshair_marker_radius,
        border_width: options.crosshair_marker_border_width,
        border: options.crosshair_marker_border_color,
        fill: options.crosshair_marker_background_color,
      },
    };
  });
  expect(probe.default_visible).toBe(false);
  expect(probe.options).toEqual({
    visible: true,
    radius: 6,
    border_width: 3,
    border: "#010203",
    fill: "#040506",
  });
  await wait_grid(page);

  const local_color_count = async (target) => {
    const { png, dsf } = await shot(page);
    const cx = Math.round(probe.x * dsf);
    const cy = Math.round(probe.y * dsf);
    const reach = Math.ceil(11 * dsf);
    let count = 0;
    for (let y = cy - reach; y <= cy + reach; y += 1) {
      for (let x = cx - reach; x <= cx + reach; x += 1) {
        const offset = (y * png.width + x) * 4;
        if (
          png.data[offset] === target[0]
          && png.data[offset + 1] === target[1]
          && png.data[offset + 2] === target[2]
        ) count += 1;
      }
    }
    return count;
  };
  expect(await local_color_count([1, 2, 3]), "marker border pixels").toBeGreaterThan(10);
  expect(await local_color_count([4, 5, 6]), "marker fill pixels").toBeGreaterThan(10);

  await page.evaluate(() => window.__hover_sma.apply_options({ crosshair_marker_visible: false }));
  await wait_grid(page);
  expect(await local_color_count([1, 2, 3]), "disabled marker border").toBe(0);
  expect(await local_color_count([4, 5, 6]), "disabled marker fill").toBe(0);
});

test("indicator values inherit source precision at creation", async ({ page }) => {
  await page.goto("/");
  await wait_grid(page);
  const values = await page.evaluate(() => {
    window.__main.apply_options({ price_format: { type: "price", precision: 0, min_move: 1 } });
    const [upper] = window.__chart.add_bollinger(window.__main, 20, 2);
    const inherited = upper.price_formatter()(65475.46);
    upper.apply_options({ price_format: { type: "price", precision: 4, min_move: 0.0001 } });
    return { inherited, overridden: upper.price_formatter()(65475.46) };
  });
  expect(values).toEqual({ inherited: "65,475", overridden: "65,475.4600" });
});

for (const backend of ["canvas2d", "webgpu"]) {
  test(`${backend}: hiding the sole SMA preserves scale precision and spacing`, async ({ page }) => {
    await page.goto(`/?backend=${backend}&forceFallbackAdapter=1`);
    await wait_grid(page);
    const result = await page.evaluate(async () => {
      const chart = window.__chart;
      window.__main.apply_options({
        price_format: { type: "price", precision: 0, min_move: 1 },
      });
      const scale = chart.add_price_scale({ id: "sma-only", side: "right", order: 0 });
      const sma = chart.add_sma(window.__main, 20);
      sma.apply_options({
        price_scale_id: "sma-only",
        last_value_visible: false,
        price_line_visible: false,
        title_visible: false,
      });
      chart.time_scale().fit_content();
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      const range = chart.time_scale().get_visible_logical_range();
      const logical = Math.floor((range.from + range.to) / 2);
      const point = sma.data_by_index(logical);
      const snapshot = () => ({
        pane_width: chart.time_scale().width(),
        scale_width: scale.width(),
        range: scale.get_visible_range(),
        coordinate: sma.price_to_coordinate(point.value),
      });
      const initial = snapshot();

      sma.apply_options({ visible: false });
      await new Promise((resolve) => requestAnimationFrame(resolve));
      const hidden = snapshot();

      sma.apply_options({ visible: true });
      await new Promise((resolve) => requestAnimationFrame(resolve));
      const shown = snapshot();
      return { actual_backend: chart.backend(), initial, hidden, shown };
    });

    expect(result.actual_backend).toBe(backend);
    expect(result.initial.scale_width).toBeGreaterThan(0);
    expect(result.hidden).toEqual(result.initial);
    expect(result.shown).toEqual(result.initial);
  });
}

test("stochastic, atr, vwap, and vwma register with lineage and placement", async ({ page }) => {
  await page.goto("/");
  await wait_grid(page);
  const out = await page.evaluate(() => {
    const panes_before = window.__chart.panes().length;
    const stoch = window.__chart.add_stochastic(window.__main, 14, 3);
    const atr = window.__chart.add_atr(window.__main, 14);
    const vwap = window.__chart.add_vwap(window.__main, null);
    const vwma = window.__chart.add_vwma(window.__main, 20);
    const info = (s) => {
      const i = s.indicator_info();
      return { kind: i.kind, period: i.period, deviation: i.deviation, output_index: i.output_index };
    };
    return {
      panes_before,
      panes_after: window.__chart.panes().length,
      stoch: [info(stoch[0]), info(stoch[1])],
      atr: info(atr),
      vwap: info(vwap),
      vwma: info(vwma),
      overlays_on_price_pane: [vwap, vwma].map((s) =>
        window.__chart.panes()[0].get_series().some((p) => p.id === s.id),
      ),
      stoch_same_pane: window.__chart
        .panes()
        .map((p) => p.get_series().some((s) => s.id === stoch[0].id || s.id === stoch[1].id)),
    };
  });
  expect(out.panes_after).toBe(out.panes_before + 2); // stochastic + atr each own a pane
  expect(out.stoch[0]).toEqual({ kind: "stochastic", period: 14, deviation: 3, output_index: 0 });
  expect(out.stoch[1].output_index).toBe(1);
  expect(out.atr.kind).toBe("atr");
  expect(out.vwap).toEqual({ kind: "vwap", period: 0, deviation: null, output_index: 0 });
  expect(out.vwma).toEqual({ kind: "vwma", period: 20, deviation: null, output_index: 0 });
  expect(out.overlays_on_price_pane).toEqual([true, true]);
  // Both stochastic lines share one pane (and it is not the price pane).
  const stoch_pane = out.stoch_same_pane.findIndex(Boolean);
  expect(stoch_pane).toBeGreaterThan(0);
});

test("browser schemas retain every multi-parameter Rust canonical default", async ({ page }) => {
  await page.goto("/");
  await wait_grid(page);
  // Explicit browser contract, independent of the engine's schema builder and indicator functions.
  // Includes volume-source descriptors when they are the second parameter.
  const defaults = {
    swing_points: { left: 5, right: 5 },
    previous_period_levels: { period: "day", calendar: "utc" },
    opening_range: { duration_seconds: 1800, calendar: "utc" },
    stochastic_rsi: { rsi_period: 14, stochastic_period: 14 },
    keltner: { period: 14, multiplier: 2 },
    supertrend: { period: 14, multiplier: 3 },
    bollinger: { period: 14, deviation: 2 },
    macd: { fast: 12, slow: 26, signal: 9 },
    stochastic: { k_period: 14, d_period: 3 },
    relative_volume: { period: 14, volume_source: null },
    cmf: { period: 14, volume_source: null },
    mfi: { period: 14, volume_source: null },
    volume: { period: 14, exponential: false, volume_source: null },
    vwma: { period: 14, volume_source: null },
    vwap_bands: { reset: "session", standard_deviation: 1, percent: 10, volume_source: null },
  };
  const actual = await page.evaluate((names) => Object.fromEntries(names.map((name) => {
    const schema = window.__chart.indicator_schema(name);
    return [name, {
      kind: schema.kind,
      parameters: Object.fromEntries(schema.parameters
        .filter(({ name: parameter }) => parameter !== "source")
        .map(({ name: parameter, default: value }) => [parameter, value])),
    }];
  })), Object.keys(defaults));
  for (const [kind, expected] of Object.entries(defaults)) {
    expect(actual[kind].kind, kind).toBe(kind);
    expect(actual[kind].parameters, kind).toEqual(expected);
  }
});
