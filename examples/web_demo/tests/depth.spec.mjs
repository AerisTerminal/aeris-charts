import { test, expect } from "@playwright/test";
import { PNG } from "pngjs";

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

for (const backend of ["canvas2d", "webgpu"]) {
  test(`depth heatmap paints hard-edged cells on every bar slot through the latest bar (${backend})`, async ({ page }) => {
    await open_chart(page, backend);
    await page.evaluate(() => {
      const chart = window.__chart;
      const n = window.__data.length;
      chart.apply_options({ grid: { vertLines: { visible: false }, horzLines: { visible: false } } });
      chart.time_scale().set_visible_logical_range({ from: n - 30, to: n + 2 });
    });
    const before = await chart_capture(page);
    const probe = await page.evaluate(() => {
      const chart = window.__chart;
      const data = window.__data;
      const interval = data.at(-1).time - data.at(-2).time;
      // A power-of-two tick about 24 CSS px tall keeps prices exact and cells large enough to probe.
      const span = Math.abs(window.__main.coordinate_to_price(30) - window.__main.coordinate_to_price(54));
      const tick = 2 ** Math.round(Math.log2(span));
      const price = Math.round(window.__main.coordinate_to_price(40) / tick) * tick;
      const bars = data.slice(-12);
      const sizes = bars.map((_, i) => (i % 2 === 0 ? 100 : 30));
      const stream = chart.add_depth_stream("BROWSER:DEPTH:CELLS", {
        tick_size: tick,
        history_bucket_micros: interval * 1_000_000,
      });
      chart.set_depth_snapshot_typed(stream, {
        timestamp_micros: bars[0].time * 1_000_000,
        sequence_high: 0,
        sequence_low: 1,
        bids: { prices: new Float64Array([price - 20 * tick]), sizes: new Float64Array([1]), order_counts: new Uint32Array([1]) },
        asks: { prices: new Float64Array([price]), sizes: new Float64Array([sizes[0]]), order_counts: new Uint32Array([1]) },
      });
      const count = bars.length - 1;
      chart.update_depth_typed(stream, {
        timestamps_micros: new Float64Array(bars.slice(1).map((bar) => bar.time * 1_000_000)),
        sequences: { high: new Uint32Array(count), low: new Uint32Array(Array.from({ length: count }, (_, i) => i + 2)) },
        previous_sequences: { high: new Uint32Array(count), low: new Uint32Array(Array.from({ length: count }, (_, i) => i + 1)) },
        sides: new Uint8Array(count).fill(1),
        prices: new Float64Array(count).fill(price),
        sizes: new Float64Array(sizes.slice(1)),
      });
      chart.add_depth_heatmap(stream, { price_min: price - 2 * tick, price_max: price + 2 * tick, maximum_size: 100 });
      const left = chart.wasm.pane_left();
      const x = (time) => left + chart.time_scale().time_to_coordinate(time);
      return {
        dpr: window.devicePixelRatio,
        y: window.__main.price_to_coordinate(price),
        row: Math.abs(window.__main.price_to_coordinate(price + tick) - window.__main.price_to_coordinate(price)),
        spacing: x(bars[1].time) - x(bars[0].time),
        bars: bars.map((bar) => x(bar.time)),
        before_first: x(data.at(-14).time),
        clear_of_candles: bars.every((bar) => price - tick > bar.high),
      };
    });
    expect(probe.clear_of_candles, "the probed row is clear of the candles").toBe(true);
    expect(probe.spacing).toBeGreaterThan(12);
    expect(probe.row).toBeGreaterThan(12);
    const after = await chart_capture(page);
    const at = (png, x, y) => {
      const o = (Math.round(y * probe.dpr) * png.width + Math.round(x * probe.dpr)) * 4;
      return Array.from(png.data.slice(o, o + 4));
    };
    const half = probe.spacing / 2 - 2;
    const half_row = probe.row / 2 - 2;
    const centers = probe.bars.map((x) => at(after, x, probe.y));
    probe.bars.forEach((x, i) => {
      expect(centers[i], `bar ${i} slot is painted`).not.toEqual(at(before, x, probe.y));
      for (const [dx, dy] of [[-half, 0], [half, 0], [0, -half_row], [0, half_row]]) {
        expect(at(after, x + dx, probe.y + dy), `bar ${i} cell interior is flat`).toEqual(centers[i]);
      }
      if (i > 0) expect(centers[i], `bar ${i} differs from its neighbour`).not.toEqual(centers[i - 1]);
    });
    expect(at(after, probe.before_first, probe.y), "nothing paints before the first bucket").toEqual(
      at(before, probe.before_first, probe.y),
    );
  });
}
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

function count_anchor_border(png) {
  // Semantic primary #006edd, the selection anchor border.
  let n = 0;
  for (let o = 0; o < png.data.length; o += 4) {
    if (Math.abs(png.data[o]) <= 30 && Math.abs(png.data[o + 1] - 110) <= 30 && Math.abs(png.data[o + 2] - 221) <= 30) n += 1;
  }
  return n;
}

async function chart_capture(page) {
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  const data_url = await page.evaluate(() => window.__chart.take_screenshot().toDataURL("image/png"));
  return PNG.sync.read(Buffer.from(data_url.split(",")[1], "base64"));
}

test("depth profile is hovered, selected and deselected like other indicators; the heatmap is not selectable", async ({ page }) => {
  await page.goto("/?backend=canvas2d");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");
  const spot = await page.evaluate(() => {
    const chart = window.__chart;
    const last = window.__data.at(-1);
    const center = Math.round(last.close * 4) / 4;
    const stream = chart.add_depth_stream("BROWSER:DEPTH:SELECT", { tick_size: 0.25 });
    chart.set_depth_snapshot_typed(stream, {
      timestamp_micros: last.time * 1_000_000,
      sequence_high: 0,
      sequence_low: 1,
      bids: { prices: new Float64Array([center, center - 0.25]), sizes: new Float64Array([10, 100]), order_counts: new Uint32Array([1, 1]) },
      asks: { prices: new Float64Array([center + 0.25, center + 0.5]), sizes: new Float64Array([10, 12]), order_counts: new Uint32Array([1, 1]) },
    });
    const profile = chart.add_depth_profile(stream, { max_width: 120, show_sizes: false });
    const heatmap = chart.add_depth_heatmap(stream, { price_min: center - 2, price_max: center + 2, maximum_size: 100 });
    window.__depth_select = { stream, profile, heatmap };
    const bounds = document.getElementById("chart_container").getBoundingClientRect();
    return {
      profile,
      heatmap,
      x: bounds.left + chart.wasm.pane_left() + chart.time_scale().width() - 60,
      y: bounds.top + window.__main.price_to_coordinate(center - 0.25),
    };
  });
  const cursor = () => page.evaluate(() => {
    const canvases = document.querySelectorAll("#chart_container canvas");
    return canvases[canvases.length - 1].style.cursor;
  });
  const selected = () => page.evaluate(() => window.__chart.selected_depth_profile());
  expect(await page.evaluate(() => "select_depth_heatmap" in window.__chart || "selected_depth_heatmap" in window.__chart)).toBe(false);

  const before = count_anchor_border(await chart_capture(page));
  await page.mouse.move(spot.x, spot.y);
  await expect.poll(cursor).toBe("pointer");
  await page.mouse.click(spot.x, spot.y);
  expect(await selected()).toBe(spot.profile);
  // At this zoom the four levels merge into one ask row and one bid row: two bar-tip discs.
  expect(count_anchor_border(await chart_capture(page)), "selection anchors paint").toBeGreaterThan(before + 8);

  // Delete leaves a depth profile in place: its lifecycle is host-owned.
  await page.keyboard.press("Delete");
  expect(await selected()).toBe(spot.profile);

  await page.keyboard.press("Escape");
  expect(await selected()).toBe(null);
  expect(count_anchor_border(await chart_capture(page))).toBeLessThanOrEqual(before);

  // Removing the selected profile clears its selection; the Delete above did not remove it.
  await page.waitForTimeout(550);
  await page.mouse.click(spot.x, spot.y);
  expect(await selected()).toBe(spot.profile);
  const removed = await page.evaluate(() => window.__chart.remove_depth_profile(window.__depth_select.profile));
  expect(removed).toBe(true);
  expect(await selected()).toBe(null);

  // A painted heatmap cell above the candles is not an interactive object: hovering keeps the
  // pane cursor and clicking selects nothing.
  const cell = await page.evaluate(() => {
    const chart = window.__chart;
    const data = window.__data;
    const bar = data.at(-30);
    const interval = data.at(-29).time - bar.time;
    const price = Math.round(window.__main.coordinate_to_price(30) * 4) / 4;
    const stream = chart.add_depth_stream("BROWSER:DEPTH:HEAT", { tick_size: 0.25, history_bucket_micros: interval * 1_000_000 });
    chart.set_depth_snapshot_typed(stream, {
      timestamp_micros: bar.time * 1_000_000,
      sequence_high: 0,
      sequence_low: 1,
      bids: { prices: new Float64Array([price - 0.5]), sizes: new Float64Array([100]), order_counts: new Uint32Array([1]) },
      asks: { prices: new Float64Array([price]), sizes: new Float64Array([100]), order_counts: new Uint32Array([1]) },
    });
    chart.add_depth_heatmap(stream, { price_min: price - 1, price_max: price + 1, maximum_size: 100 });
    const x0 = chart.time_scale().time_to_coordinate(bar.time);
    const x1 = chart.time_scale().time_to_coordinate(data.at(-29).time);
    const bounds = document.getElementById("chart_container").getBoundingClientRect();
    return {
      x: bounds.left + chart.wasm.pane_left() + (x0 + x1) / 2,
      y: bounds.top + window.__main.price_to_coordinate(price),
      above_candle: price - 0.25 > bar.high,
    };
  });
  expect(cell.above_candle, "the probed cell is clear of the candle").toBe(true);
  const quiet = count_anchor_border(await chart_capture(page));
  await page.mouse.move(cell.x, cell.y);
  await page.mouse.move(cell.x + 1, cell.y);
  await expect.poll(cursor).not.toBe("pointer");
  await page.mouse.click(cell.x, cell.y);
  expect(await selected()).toBe(null);
  expect(count_anchor_border(await chart_capture(page)), "nothing paints selection anchors").toBeLessThanOrEqual(quiet);
});
