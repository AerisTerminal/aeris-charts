import { test, expect } from "@playwright/test";

async function open_chart(page, backend = "canvas2d") {
  await page.goto(`/?runtimeTest=presentedFrame&backend=${backend}&forceFallbackAdapter=1`);
  await page.waitForFunction((expected) => window.__chart?.backend?.() === expected, backend);
}

test("typed depth ingest, studies, gap recovery, replay, heatmap, and markers share one model", async ({ page }) => {
  await open_chart(page, "canvas2d");
  const result = await page.evaluate(() => {
    const chart = window.__chart;
    const level_columns = (prices, sizes, counts) => ({
      prices: new Float64Array(prices),
      sizes: new Float64Array(sizes),
      order_counts: new Uint32Array(counts),
    });
    const last = window.__data.at(-1);
    const second = last.time;
    const micros = second * 1_000_000;
    const center = Math.round(last.close * 4) / 4;
    const stream = chart.add_depth_stream("BROWSER:DEPTH", {
      tick_size: 0.25,
      history_bucket_micros: 100_000,
      max_history_buckets: 64,
      max_history_cells: 2_048,
    });
    chart.set_depth_snapshot_typed(stream, {
      timestamp_micros: micros,
      sequence_high: 0,
      sequence_low: 1,
      bids: {
        prices: new Float64Array([center, center - 0.25]),
        sizes: new Float64Array([10, 5]),
        order_counts: new Uint32Array([2, 1]),
      },
      asks: {
        prices: new Float64Array([center + 0.25, center + 0.5]),
        sizes: new Float64Array([8, 6]),
        order_counts: new Uint32Array([3, 2]),
      },
    });
    chart.update_depth_typed(stream, {
      timestamps_micros: new Float64Array([micros + 110_000, micros + 210_000]),
      sequences: { high: new Uint32Array([0, 0]), low: new Uint32Array([2, 3]) },
      previous_sequences: { high: new Uint32Array([0, 0]), low: new Uint32Array([1, 2]) },
      sides: new Uint8Array([0, 1]),
      prices: new Float64Array([center, center + 0.25]),
      sizes: new Float64Array([12, 9]),
      order_counts: new Uint32Array([4, 5]),
    });
    const ladder = chart.depth_ladder(stream, 4);
    const study = chart.depth_study(stream, 4);
    let gap = null;
    try {
      chart.update_depth_typed(stream, {
        timestamps_micros: new Float64Array([micros + 220_000]),
        sequences: { high: new Uint32Array([0]), low: new Uint32Array([5]) },
        previous_sequences: { high: new Uint32Array([0]), low: new Uint32Array([4]) },
        sides: new Uint8Array([0]),
        prices: new Float64Array([center]),
        sizes: new Float64Array([13]),
      });
    } catch (error) {
      gap = { code: error.code, message: error.message };
    }
    chart.set_depth_snapshot_typed(stream, {
      timestamp_micros: micros + 300_000,
      sequence_high: 0,
      sequence_low: 10,
      bids: level_columns([center], [14], [6]),
      asks: level_columns([center + 0.25], [11], [4]),
    });
    chart.update_depth_typed(stream, {
      timestamps_micros: new Float64Array([micros + 310_000, micros + 410_000]),
      sequences: { high: new Uint32Array([0, 0]), low: new Uint32Array([11, 12]) },
      previous_sequences: { high: new Uint32Array([0, 0]), low: new Uint32Array([10, 11]) },
      sides: new Uint8Array([0, 1]),
      prices: new Float64Array([center, center + 0.25]),
      sizes: new Float64Array([15, 12]),
    });
    const heatmap = chart.add_depth_heatmap(stream, {
      price_min: center - 1,
      price_max: center + 1,
      maximum_size: 20,
    });
    const event_layer = chart.add_depth_event_layer(stream, { max_markers: 64 });
    chart.set_depth_events_typed(stream, {
      timestamps_micros: new Float64Array([micros + 320_000]),
      prices: new Float64Array([center + 0.25]),
      sizes: new Float64Array([12]),
      sides: new Int8Array([1]),
      kinds: new Uint8Array([3]),
      labels: ["ask sweep"],
    });
    const replay = chart.set_replay_clock_micros(micros + 350_000);
    const replay_sequence = chart.depth_study(stream, 2).sequence;
    chart.set_replay_clock_micros(null);
    const live_sequence = chart.depth_study(stream, 2).sequence;
    chart.time_scale().fit_content();
    return {
      stream_id: chart.depth_stream_id("BROWSER:DEPTH"),
      ladder,
      study,
      gap,
      replay,
      replay_sequence,
      live_sequence,
      removed: chart.remove_depth_event_layer(event_layer) && chart.remove_depth_heatmap(heatmap),
    };
  });

  expect(result.stream_id).toBeGreaterThan(0);
  expect(result.ladder[0]).toMatchObject({ price: expect.any(Number) });
  expect(result.study).toMatchObject({ sequence: "3", imbalance: expect.any(Number) });
  expect(result.gap).toMatchObject({ code: "invalid_data" });
  expect(result.gap.message).toContain("sequence gap");
  expect(result.replay.depth_stream_count).toBe(1);
  expect(result.replay_sequence).toBe("11");
  expect(result.live_sequence).toBe("12");
  expect(result.removed).toBe(true);
});

test("depth heatmap and host markers render through the WebGPU shared frame", async ({ page }) => {
  await open_chart(page, "webgpu");
  const result = await page.evaluate(() => {
    const chart = window.__chart;
    const level_columns = (prices, sizes, counts) => ({
      prices: new Float64Array(prices),
      sizes: new Float64Array(sizes),
      order_counts: new Uint32Array(counts),
    });
    const last = window.__data.at(-1);
    const micros = last.time * 1_000_000;
    const center = Math.round(last.close * 4) / 4;
    const stream = chart.add_depth_stream("BROWSER:DEPTH:GPU", { tick_size: 0.25 });
    chart.set_depth_snapshot_typed(stream, {
      timestamp_micros: micros,
      sequence_high: 0,
      sequence_low: 1,
      bids: level_columns([center], [40], [8]),
      asks: level_columns([center + 0.25], [35], [7]),
    });
    chart.add_depth_heatmap(stream, {
      price_min: center - 1,
      price_max: center + 1,
      maximum_size: 50,
    });
    chart.add_depth_event_layer(stream);
    chart.set_depth_events_typed(stream, {
      timestamps_micros: new Float64Array([micros]),
      prices: new Float64Array([center]),
      sizes: new Float64Array([40]),
      sides: new Int8Array([0]),
      kinds: new Uint8Array([2]),
      labels: ["bid cluster"],
    });
    chart.time_scale().fit_content();
    return { backend: chart.backend(), ladder: chart.depth_ladder(stream, 1) };
  });
  expect(result.backend).toBe("webgpu");
  expect(result.ladder).toHaveLength(2);
  const screenshot = await page.locator("#chart_container").screenshot({ animations: "disabled" });
  expect(screenshot.byteLength).toBeGreaterThan(10_000);
});

test("time and sales is bounded, classified, filtered, and newest first", async ({ page }) => {
  await open_chart(page);
  const result = await page.evaluate(() => {
    const chart = window.__chart;
    const micros = window.__data.at(-1).time * 1_000_000;
    const stream = chart.add_trade_stream("BROWSER:TAPE", { tick_size: 0.25 });
    chart.set_trade_stream_trades(stream, [
      { timestamp_micros: micros + 1, price: 100, volume: 1, aggressor: "sell", trade_id: 1 },
      { timestamp_micros: micros + 2, price: 100.25, volume: 3, aggressor: "unknown", bid: 100, ask: 100.25, trade_id: 2 },
      { timestamp_micros: micros + 3, price: 100.5, volume: 5, aggressor: "buy", trade_id: 3 },
    ]);
    return chart.time_and_sales(stream, { minimum_volume: 2, side: "buy", max_rows: 2 });
  });
  expect(result).toEqual([
    expect.objectContaining({ volume: 5, aggressor: "buy", trade_id: "3" }),
    expect.objectContaining({ volume: 3, aggressor: "buy", trade_id: "2" }),
  ]);
});
