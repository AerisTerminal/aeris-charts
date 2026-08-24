import { test, expect } from "@playwright/test";

async function wait_chart(page) {
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined);
}

test("chart value snapshots preserve exact gaps, latest independence, formatting, and freshness", async ({ page }) => {
  await page.goto("/");
  await wait_chart(page);

  const result = await page.evaluate(() => {
    const chart = window.__chart;
    const candles = chart.add_series("candlestick", {
      price_format: { type: "price", precision: 1, min_move: 0.1 },
    });
    candles.set_data([
      { time: 2_000_000_000, open: 10, high: 13, low: 9, close: 11 },
      { time: 2_000_000_010, open: 20, high: 23, low: 19, close: 21 },
      { time: 2_000_000_020 },
    ]);
    const comparison = chart.add_series("line", { pane: 1, price_scale_id: "left" });
    comparison.set_data([
      { time: 2_000_000_000, value: 100 },
      { time: 2_000_000_020, value: 300 },
    ]);
    const converted = chart.add_series("line");
    converted.set_data([{ time: 2_000_000_020, value: 7 }]);
    converted.set_type("bar");
    chart.render();

    const x = chart.time_scale().time_to_coordinate(2_000_000_010);
    const logical = chart.time_scale().coordinate_to_logical(x);
    const exact = chart.value_snapshot(logical);
    const whitespace_x = chart.time_scale().time_to_coordinate(2_000_000_020);
    const whitespace_logical = chart.time_scale().coordinate_to_logical(whitespace_x);
    const exact_whitespace = chart.value_snapshot(whitespace_logical);
    const latest_before = chart.value_snapshot();
    comparison.update({ time: 2_000_000_030, value: 400 });
    const latest_after = chart.value_snapshot();

    let leave = null;
    chart.subscribe_crosshair_move((params) => {
      if (params.point === null) {
        leave = {
          series_data_size: params.series_data.size,
          comparison: params.value_snapshot.find((entry) => entry.series === comparison),
        };
      }
    });
    chart.emit_crosshair_left();

    const pick = (snapshot, series) => {
      const value = snapshot.find((entry) => entry.series === series);
      return { ...value, series: value.series.id };
    };
    return {
      exact_candles: pick(exact, candles),
      exact_comparison: pick(exact, comparison),
      exact_whitespace: pick(exact_whitespace, candles),
      latest_candles: pick(latest_before, candles),
      latest_comparison_before: pick(latest_before, comparison),
      latest_comparison_after: pick(latest_after, comparison),
      converted: pick(latest_after, converted),
      leave: {
        series_data_size: leave.series_data_size,
        comparison: { ...leave.comparison, series: leave.comparison.series.id },
      },
    };
  });

  expect(result.exact_candles).toMatchObject({
    kind: "candlestick", time: 2_000_000_010, open: 20, high: 23, low: 19, close: 21,
    value: null, previous_value: 11, formatted_close: "21.0", formatted_previous_value: "11.0",
  });
  expect(result.exact_comparison).toMatchObject({
    kind: "line", time: 2_000_000_010, value: null, previous_value: null,
    pane_index: 1, price_scale_id: "left",
  });
  expect(result.exact_whitespace).toMatchObject({
    time: 2_000_000_020, open: null, high: null, low: null, close: null, previous_value: null,
  });
  expect(result.latest_candles).toMatchObject({ time: 2_000_000_010, close: 21, previous_value: 11 });
  expect(result.latest_comparison_before).toMatchObject({ time: 2_000_000_020, value: 300, previous_value: 100 });
  expect(result.latest_comparison_after).toMatchObject({ time: 2_000_000_030, value: 400, previous_value: 300 });
  expect(result.converted).toMatchObject({ kind: "bar", open: 7, high: 7, low: 7, close: 7 });
  expect(result.leave.series_data_size).toBe(0);
  expect(result.leave.comparison).toMatchObject({ time: 2_000_000_030, value: 400 });
});

test("indicator metadata exposes stable bindings, complete parameters, values, and output order", async ({ page }) => {
  await page.goto("/");
  await wait_chart(page);

  const result = await page.evaluate(() => {
    const chart = window.__chart;
    const macd = chart.add_macd(window.__main, 12, 26, 9);
    const volume = chart.add_series("histogram");
    const source = window.__main.data();
    volume.set_data(source.map((point) => ({ time: point.time, value: 10 })));
    const vwap = chart.add_vwap(window.__main, volume);
    const snapshot = chart.value_snapshot();
    return {
      macd: macd.map((series) => {
        const info = series.indicator_info();
        const value = snapshot.find((entry) => entry.series === series);
        return {
          ...info,
          source: info.source.id,
          volume_source: info.volume_source?.id ?? null,
          has_value: value.value !== null,
          has_formatted_value: value.formatted_value !== null,
        };
      }),
      vwap: (() => {
        const info = vwap.indicator_info();
        return { ...info, source: info.source.id, volume_source: info.volume_source?.id ?? null };
      })(),
      volume_id: volume.id,
    };
  });

  expect(result.macd.map((info) => info.output_name)).toEqual(["MACD", "Signal", "Histogram"]);
  expect(result.macd.map((info) => info.output_index)).toEqual([0, 1, 2]);
  expect(result.macd.every((info) => info.binding_id === result.macd[0].binding_id)).toBe(true);
  expect(result.macd.every((info) => info.output_count === 3)).toBe(true);
  expect(result.macd.every((info) => info.parameters.fast === 12 && info.parameters.slow === 26 && info.parameters.signal === 9)).toBe(true);
  expect(result.macd.every((info) => info.has_value && info.has_formatted_value)).toBe(true);
  expect(result.vwap.output_name).toBe("VWAP");
  expect(result.vwap.volume_source).toBe(result.volume_id);
});
