import { test, expect } from "@playwright/test";

async function open_chart(page, backend = "canvas2d") {
  await page.goto(`/?runtimeTest=presentedFrame&backend=${backend}&forceFallbackAdapter=1`);
  await page.waitForFunction((expected) => window.__chart?.backend?.() === expected, backend);
}

test("tick-driven footprint API preserves delta path, POC, and stacked imbalances", async ({ page }) => {
  await open_chart(page);
  const result = await page.evaluate(() => {
    const chart = window.__chart;
    chart.remove_series(window.__main);
    const footprint = chart.add_series("footprint", {
      tick_size: 1,
      interval_seconds: 60,
      imbalance_ratio: 3,
      imbalance_minimum_volume: 20,
      stacked_imbalance_levels: 2,
      font_size: 9,
      cell_mode: "bid_ask",
      price_line_visible: false,
      last_value_visible: false,
    });
    const second = Math.floor(window.__data[0].time / 60) * 60;
    const micros = second * 1_000_000;
    footprint.set_trades([
      { timestamp_micros: micros + 1, price: 100, volume: 10, aggressor: "sell", session_id: 1 },
      { timestamp_micros: micros + 2, price: 101, volume: 40, aggressor: "buy", session_id: 1 },
      { timestamp_micros: micros + 3, price: 101, volume: 10, aggressor: "sell", session_id: 1 },
      { timestamp_micros: micros + 4, price: 102, volume: 50, aggressor: "buy", session_id: 1 },
      // Mean reversion proves extrema come from the tick path, not final delta.
      { timestamp_micros: micros + 5, price: 102, volume: 80, aggressor: "sell", session_id: 1 },
    ]);
    chart.time_scale().fit_content();
    chart.time_scale().apply_options({ bar_spacing: 100 });
    const first = footprint.footprint_bar(0);
    const generic_set_error = (() => {
      try {
        footprint.set_data([{ time: second, open: 1, high: 2, low: 0, close: 1 }]);
        return null;
      } catch (error) {
        return { code: error.code, message: error.message };
      }
    })();
    const tip = footprint.update_trade({
      timestamp_micros: micros + 6,
      price: 101,
      volume: 5,
      aggressor: "buy",
      session_id: 1,
    });
    const historical = footprint.update_trade({
      timestamp_micros: micros + 3,
      price: 101,
      volume: 2,
      aggressor: "buy",
      sequence: 1,
      session_id: 1,
    });
    const final = footprint.footprint_bar(0);
    return {
      type: footprint.series_type(),
      first,
      final,
      bars: footprint.footprint_bars().length,
      tip,
      historical,
      generic_set_error,
    };
  });

  expect(result.type).toBe("footprint");
  expect(result.bars).toBe(1);
  expect(result.first).toMatchObject({
    bid_volume: 100,
    ask_volume: 90,
    total_volume: 190,
    delta: -10,
    max_delta: 70,
    min_delta: -10,
    poc_price: 102,
  });
  expect(result.first.levels.filter((level) => level.stacked_ask_imbalance).map((level) => level.price))
    .toEqual([101, 102]);
  expect(result.tip).toBe("tip");
  expect(result.historical).toBe("historical");
  expect(result.final.delta).toBe(-3);
  expect(result.final.max_delta).toBe(72);
  expect(result.final.min_delta).toBe(-10);
  expect(result.generic_set_error).toMatchObject({ code: "unsupported_operation" });

  const screenshot = await page.screenshot();
  expect(screenshot.byteLength).toBeGreaterThan(10_000);
});

test("footprint shared-frame semantics execute through WebGPU", async ({ page }) => {
  await open_chart(page, "webgpu");
  const result = await page.evaluate(() => {
    const chart = window.__chart;
    chart.remove_series(window.__main);
    const footprint = chart.add_series("footprint", {
      tick_size: 0.25,
      interval_seconds: 60,
      cell_mode: "delta",
    });
    const second = Math.floor(window.__data[0].time / 60) * 60;
    const micros = second * 1_000_000;
    footprint.set_trades([
      { timestamp_micros: micros + 1, price: 100, volume: 9, aggressor: "buy" },
      { timestamp_micros: micros + 2, price: 100.25, volume: 4, aggressor: "sell" },
    ]);
    chart.time_scale().fit_content();
    chart.time_scale().apply_options({ bar_spacing: 100 });
    return { backend: chart.backend(), bar: footprint.footprint_bar(0) };
  });
  expect(result.backend).toBe("webgpu");
  expect(result.bar).toMatchObject({ total_volume: 13, delta: 5, max_delta: 9, min_delta: 0 });
  const screenshot = await page.screenshot();
  expect(screenshot.byteLength).toBeGreaterThan(10_000);
});

test("typed footprint columns reject off-grid data without replacing accepted bars", async ({ page }) => {
  await open_chart(page);
  const result = await page.evaluate(() => {
    const chart = window.__chart;
    const footprint = chart.add_series("footprint", { tick_size: 0.25, interval_seconds: 60 });
    const second = Math.floor(window.__data[0].time / 60) * 60;
    const columns = (price) => ({
      timestamps_micros: new Float64Array([second * 1_000_000 + 1]),
      prices: new Float64Array([price]),
      volumes: new Float64Array([4]),
      aggressors: new Uint8Array([1]),
      bids: new Float64Array([NaN]),
      asks: new Float64Array([NaN]),
      sequences: new Float64Array([NaN]),
      trade_ids: new Float64Array([NaN]),
      conditions: new Uint32Array([0]),
      session_ids: new Float64Array([1]),
    });
    footprint.set_trades_typed(columns(100.25));
    const before = footprint.footprint_bars();
    let error = null;
    try {
      footprint.set_trades_typed(columns(100.30));
    } catch (caught) {
      error = { code: caught.code, message: caught.message };
    }
    return { before, after: footprint.footprint_bars(), error };
  });
  expect(result.error).toMatchObject({ code: "invalid_data" });
  expect(result.error.message).toContain("tick_size");
  expect(result.after).toEqual(result.before);
});
