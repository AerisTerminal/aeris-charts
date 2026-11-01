import { expect } from "@playwright/test";
import { test, wait_for_chart } from "./page_ready.mjs";
import { PNG } from "pngjs";

const UP = [8, 153, 129];
const DOWN = [247, 82, 95];

function count_near(png, rgb) {
  let count = 0;
  for (let i = 0; i < png.data.length; i += 4) {
    if (Math.abs(png.data[i] - rgb[0]) + Math.abs(png.data[i + 1] - rgb[1]) + Math.abs(png.data[i + 2] - rgb[2]) <= 24) count += 1;
  }
  return count;
}

for (const backend of ["canvas2d", "webgpu"]) {
  test(`financial chart styles derive, update, and render through ${backend}`, async ({ page }) => {
    await page.goto(`/?runtimeTest=presentedFrame&backend=${backend}&forceFallbackAdapter=1`);
    await page.waitForFunction((expected) => window.__chart?.backend?.() === expected, backend);
    await wait_for_chart(page, { settle: false });

    const result = await page.evaluate(async () => {
      const chart = window.__chart;
      const series = window.__main;
      const start = Math.floor(window.__data[0].time);
      const row = (index, open, high, low, close) => ({ time: start + index, open, high, low, close });
      const ohlc = () => series.data().map(({ open, high, low, close }) => ({ open, high, low, close }));

      // Renko: high/low source, wicks, hand-computed brick and wick values.
      const renko_options = { kind: "renko_fixed", box_size: 1, source: "high_low", wicks: true };
      chart.configure_synthetic_bar_series(series, renko_options);
      chart.set_synthetic_bar_source(series, [
        row(0, 100, 100, 100, 100),
        row(1, 100, 102.3, 99.5, 100.5),
        row(2, 100.5, 101, 99.8, 100),
      ]);
      const renko = ohlc();
      const second_brick = chart.synthetic_bar_at(series, 2);
      const out_of_range = chart.synthetic_bar_at(series, 3);

      // The crosshair resolves a derived bar to its source open time; several bricks formed by
      // one source row share that time and resolve to the run's first brick.
      chart.time_scale().fit_content();
      const time_at_first_shared = chart.time_scale().coordinate_to_time(chart.time_scale().logical_to_coordinate(1));
      const time_at_second_shared = chart.time_scale().coordinate_to_time(chart.time_scale().logical_to_coordinate(2));
      // The crosshair accepts the shared source time and lands on the run's first brick; value
      // snapshots report each derived bar's source open time.
      chart.set_crosshair_position(101, start + 1, series);
      chart.clear_crosshair_position();
      const first_shared = chart.value_snapshot(1).find((entry) => entry.series_id === series.id);
      const second_shared = chart.value_snapshot(2).find((entry) => entry.series_id === series.id);

      // A no-change tip rewrite keeps the sequence; a newer row appends only its own bricks.
      chart.update_synthetic_bar_source(series, row(2, 100.5, 101, 99.8, 100));
      const after_rewrite = ohlc();
      chart.update_synthetic_bar_source(series, row(3, 100, 105.4, 100, 105.4));
      const after_append = ohlc();

      // Indicators read derived closes through the same series handle.
      const sma = chart.add_sma(series, 2);
      const sma_values = sma.data().map((point) => point.value);

      // The transform definition round-trips through state persistence; a fresh chart restores
      // the owner, and installing the same source reproduces the projection.
      const state = chart.export_state();
      const { create_chart } = await import("/dist/aeris_charts_financial.js");
      const host = document.createElement("div");
      host.style.cssText = "position:absolute;left:-10000px;width:400px;height:300px";
      document.body.append(host);
      const restored_chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
      const restored_series = restored_chart.add_series("candlestick");
      const restore = restored_chart.import_state(state);
      restored_chart.set_synthetic_bar_source(restored_series, [
        row(0, 100, 100, 100, 100),
        row(1, 100, 102.3, 99.5, 100.5),
        row(2, 100.5, 101, 99.8, 100),
        row(3, 100, 105.4, 100, 105.4),
      ]);
      const restored = restored_series.data().map(({ open, high, low, close }) => ({ open, high, low, close }));
      const restored_brick = restored_chart.synthetic_bar_at(restored_series, 2);
      const canonical = restored_chart.export_state();
      restored_chart.remove();
      host.remove();

      // Kagi: absolute and percent reversals, yang/yin switching at the prior shoulder/waist.
      const closes = (values) => values.map((close, index) => row(index, close, close, close, close));
      chart.configure_synthetic_bar_series(series, { kind: "kagi", reversal_size: 2 });
      chart.set_synthetic_bar_source(series, closes([100, 104, 101, 99, 102, 105]));
      const kagi = [0, 1, 2].map((index) => {
        const bar = chart.synthetic_bar_at(series, index);
        return { open: bar.open, close: bar.close, kagi: bar.kagi };
      });
      chart.configure_synthetic_bar_series(series, { kind: "kagi", reversal_size: 2, reversal_mode: "percent" });
      chart.set_synthetic_bar_source(series, closes([100, 103, 104, 101.95]));
      const percent_kagi_lines = series.data().length;

      // Point & Figure on high/low: intrabar extremes build and reverse columns.
      chart.configure_synthetic_bar_series(series, { kind: "point_and_figure", box_size: 1, source: "high_low" });
      chart.set_synthetic_bar_source(series, [
        row(0, 100, 100, 100, 100),
        row(1, 100, 103.4, 99.6, 102),
        row(2, 102, 102.5, 99.9, 100.2),
      ]);
      const point_and_figure = series.data().map(({ open, close }) => ({ open, close }));

      // Line Break defaults to three lines; a reversal line opens at the previous line's open.
      chart.configure_synthetic_bar_series(series, { kind: "line_break" });
      chart.set_synthetic_bar_source(series, closes([100, 101, 102, 101.5, 99]));
      const line_break = series.data().map(({ open, close }) => ({ open, close }));

      return {
        renko, second_brick, out_of_range,
        time_at_first_shared, time_at_second_shared, first_shared, second_shared,
        after_rewrite, after_append, sma_values,
        state, restore, restored, restored_brick, canonical,
        kagi, percent_kagi_lines, point_and_figure, line_break,
        start,
      };
    });

    expect(result.renko).toEqual([
      { open: 100, high: 101, low: 99.5, close: 101 },
      // The first brick already carries the row's 99.5 wick; the second sees only the same row.
      { open: 101, high: 102, low: 101, close: 102 },
      { open: 101, high: 101, low: 100, close: 100 },
    ]);
    expect(result.second_brick).toMatchObject({
      logical_index: 2,
      open: 101, high: 101, low: 100, close: 100,
      open_timestamp_micros: (result.start + 1) * 1_000_000,
      close_timestamp_micros: (result.start + 2) * 1_000_000,
      kagi: null,
    });
    expect(result.out_of_range).toBeNull();

    expect(result.time_at_first_shared).toBe(result.start + 1);
    expect(result.time_at_second_shared).toBe(result.start + 1);
    expect(result.first_shared).toMatchObject({ close: 102, logical_index: 1, time: result.start + 1 });
    expect(result.second_shared).toMatchObject({ close: 100, logical_index: 2, time: result.start + 1 });

    expect(result.after_rewrite).toEqual(result.renko);
    expect(result.after_append.slice(0, 3)).toEqual(result.renko);
    expect(result.after_append.slice(3).map(({ open, close }) => ({ open, close }))).toEqual([
      { open: 101, close: 102 },
      { open: 102, close: 103 },
      { open: 103, close: 104 },
      { open: 104, close: 105 },
    ]);
    expect(result.sma_values).toEqual([101.5, 101, 101, 102.5, 103.5, 104.5]);

    expect(result.state.schema_version).toBe(3);
    expect(result.state.price_transforms).toEqual([{ series_id: 0, options: { kind: "renko_fixed", box_size: 1, source: "high_low", wicks: true } }]);
    expect(result.restore.schema_version).toBe(3);
    expect(result.restored).toEqual(result.after_append);
    expect(result.restored_brick).toMatchObject({ open: 101, close: 100 });
    expect(result.canonical).toEqual(result.state);

    expect(result.kagi).toEqual([
      { open: 100, close: 104, kagi: { yang_at_open: true, reference: null } },
      { open: 104, close: 99, kagi: { yang_at_open: true, reference: 100 } },
      { open: 99, close: 105, kagi: { yang_at_open: false, reference: 104 } },
    ]);
    expect(result.percent_kagi_lines).toBe(1);
    expect(result.point_and_figure).toEqual([
      { open: 100, close: 103 },
      { open: 102, close: 100 },
    ]);
    expect(result.line_break).toEqual([
      { open: 100, close: 100 },
      { open: 100, close: 101 },
      { open: 101, close: 102 },
      { open: 101, close: 99 },
    ]);

    // Rendered evidence: Kagi draws both yang (up color) and yin (down color) segments, and
    // Point & Figure draws both X and O columns, on every backend.
    await page.evaluate(() => {
      const chart = window.__chart;
      const start = Math.floor(window.__data[0].time);
      const closes = (values) => values.map((close, index) => ({ time: start + index, open: close, high: close, low: close, close }));
      chart.configure_synthetic_bar_series(window.__main, { kind: "kagi", reversal_size: 2 });
      chart.set_synthetic_bar_source(window.__main, closes([100, 104, 101, 99, 102, 105]));
      chart.time_scale().fit_content();
    });
    await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    const kagi_png = PNG.sync.read(await page.locator("#chart_wrap").screenshot());
    expect(count_near(kagi_png, UP)).toBeGreaterThan(50);
    expect(count_near(kagi_png, DOWN)).toBeGreaterThan(50);

    await page.evaluate(() => {
      const chart = window.__chart;
      const start = Math.floor(window.__data[0].time);
      chart.configure_synthetic_bar_series(window.__main, { kind: "point_and_figure", box_size: 1, reversal_boxes: 3 });
      chart.set_synthetic_bar_source(window.__main, [100, 104.2, 102, 100.5, 103.8].map((close, index) => ({ time: start + index, open: close, high: close, low: close, close })));
      chart.time_scale().fit_content();
      chart.time_scale().apply_options({ bar_spacing: 40 });
    });
    await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    const pnf_png = PNG.sync.read(await page.locator("#chart_wrap").screenshot());
    expect(count_near(pnf_png, UP)).toBeGreaterThan(50);
    expect(count_near(pnf_png, DOWN)).toBeGreaterThan(50);
  });
}
