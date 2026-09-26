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
      options: footprint.options(),
      first,
      final,
      bars: footprint.footprint_bars().length,
      tip,
      historical,
      generic_set_error,
    };
  });

  expect(result.type).toBe("footprint");
  expect(result.options.ask_color).toMatch(/^rgba\(8,153,129,/);
  expect(result.options.positive_delta_color).toMatch(/^rgba\(8,153,129,/);
  expect(result.options.stacked_ask_color).toBe("#089981");
  expect(result.bars).toBe(1);
  expect(result.first).toMatchObject({
    logical_index: 0,
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
  expect(result.bar).toMatchObject({ logical_index: 0, total_volume: 13, delta: 5, max_delta: 9, min_delta: 0 });
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

test("footprint validation rejects malformed aggressors and infinities atomically", async ({ page }) => {
  await open_chart(page);
  const result = await page.evaluate(() => {
    const chart = window.__chart;
    const footprint = chart.add_series("footprint", { tick_size: 0.25, interval_seconds: 60 });
    const second = Math.floor(window.__data[0].time / 60) * 60;
    const columns = (aggressor = 1, bid = Number.NaN) => ({
      timestamps_micros: new Float64Array([second * 1_000_000 + 1]),
      prices: new Float64Array([100.25]),
      volumes: new Float64Array([4]),
      aggressors: new Uint8Array([aggressor]),
      bids: new Float64Array([bid]),
      asks: new Float64Array([Number.NaN]),
      sequences: new Float64Array([Number.NaN]),
      trade_ids: new Float64Array([1]),
      conditions: new Uint32Array([0]),
      session_ids: new Float64Array([1]),
    });
    footprint.set_trades_typed(columns());
    const before = footprint.footprint_bars();
    const errors = [];
    for (const invalid of [columns(9), columns(1, Number.POSITIVE_INFINITY)]) {
      try {
        footprint.set_trades_typed(invalid);
      } catch (error) {
        errors.push({ code: error.code, message: error.message });
      }
    }
    try {
      footprint.update_trade({
        timestamp_micros: second * 1_000_000 + 2,
        price: 100.25,
        volume: 1,
        aggressor: "crossed",
      });
    } catch (error) {
      errors.push({ code: error.code, message: error.message });
    }
    return { before, after: footprint.footprint_bars(), errors };
  });
  expect(result.errors).toHaveLength(3);
  expect(result.errors.every((error) => error.code === "invalid_data")).toBe(true);
  expect(result.errors[0].message).toContain("index 0");
  expect(result.errors[0].message).toContain("aggressor");
  expect(result.errors[1].message).toContain("infinity");
  expect(result.errors[2].message).toContain("aggressor");
  expect(result.after).toEqual(result.before);
});

test("historical correction batches and session replacements use final canonical truth", async ({ page }) => {
  await open_chart(page);
  const result = await page.evaluate(() => {
    const chart = window.__chart;
    const footprint = chart.add_series("footprint", { tick_size: 1, interval_seconds: 60 });
    const second = Math.floor(window.__data[0].time / 60) * 60;
    const micros = second * 1_000_000;
    footprint.set_trades([
      { timestamp_micros: micros + 1, price: 100, volume: 4, aggressor: "buy", trade_id: 1, session_id: 1 },
      { timestamp_micros: micros + 2, price: 101, volume: 2, aggressor: "sell", trade_id: 2, session_id: 1 },
    ]);
    const batch = footprint.update_trades([
      { timestamp_micros: micros + 1, price: 100, volume: 6, aggressor: "sell", trade_id: 1, session_id: 2 },
      { timestamp_micros: micros + 2, price: 101, volume: 3, aggressor: "buy", trade_id: 2, session_id: 2 },
    ]);
    return { batch, bars: footprint.footprint_bars() };
  });
  expect(result.batch).toBe("historical");
  expect(result.bars).toHaveLength(1);
  expect(result.bars[0]).toMatchObject({ session_id: 2, bid_volume: 6, ask_volume: 3, delta: -3 });
});

test("failed footprint creation leaves engine order, handles, scale membership, and notifications unchanged", async ({ page }) => {
  await open_chart(page);
  const result = await page.evaluate(() => {
    const chart = window.__chart;
    const before = {
      handles: chart.series_order().map((series) => series.id),
      snapshots: chart.value_snapshot().map((entry) => entry.series_id),
    };
    const added = [];
    chart.subscribe_series_added((event) => added.push(event.series.id));
    let failure = null;
    try {
      chart.add_series("footprint", { price_scale_id: "footprint-dedicated", tick_size: 0.25 });
    } catch (error) {
      failure = { code: error.code, message: error.message };
    }
    const rejected = {
      handles: chart.series_order().map((series) => series.id),
      snapshots: chart.value_snapshot().map((entry) => entry.series_id),
      added: [...added],
      scales: chart.price_scales().map((scale) => ({ id: scale.id, series_ids: scale.series_ids })),
    };
    chart.add_price_scale({ id: "footprint-dedicated", side: "right" });
    const footprint = chart.add_series("footprint", {
      price_scale_id: "footprint-dedicated",
      tick_size: 0.25,
    });
    return {
      before,
      failure,
      rejected,
      accepted: {
        id: footprint.id,
        scale_id: footprint.price_scale_id(),
        added,
        members: chart.price_scales().find((scale) => scale.id === "footprint-dedicated")?.series_ids,
      },
    };
  });
  expect(result.failure).toMatchObject({ code: "invalid_options" });
  expect(result.rejected.handles).toEqual(result.before.handles);
  expect(result.rejected.snapshots).toEqual(result.before.snapshots);
  expect(result.rejected.added).toEqual([]);
  expect(result.rejected.scales.some((scale) => scale.id === "footprint-dedicated")).toBe(false);
  expect(result.accepted.scale_id).toBe("footprint-dedicated");
  expect(result.accepted.added).toEqual([result.accepted.id]);
  expect(result.accepted.members).toEqual([result.accepted.id]);
});
