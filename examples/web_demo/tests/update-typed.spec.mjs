import { test, expect } from "@playwright/test";

// `series_api.update_typed(columns)` (consumer Item 2). The streaming path was one JS object per
// point, called twice per bar; at the consumer's 50k ticks/sec target that is 100k short-lived
// allocations per second entering wasm. These specs pin the contract that replaces it: a one-row
// batch must be indistinguishable from `update`, a 500-row batch must be one call and one
// notification, and a long run of batches must not accumulate JS objects.

async function wait_chart(page) {
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined);
  await page.evaluate(() => new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  }));
}

/** Summarize CDP's sampled allocation tree, attributing descendants to package call stacks. */
function sampled_allocations(profile) {
  let total_bytes = 0;
  let package_bytes = 0;
  const by_function = new Map();
  const visit = (node, package_stack = false) => {
    const frame = node.callFrame ?? {};
    const in_package = package_stack || frame.url?.includes("/dist/nucleuscharts_financial.js") === true;
    const bytes = node.selfSize ?? 0;
    total_bytes += bytes;
    if (in_package) {
      package_bytes += bytes;
      const name = frame.functionName || "(anonymous)";
      by_function.set(name, (by_function.get(name) ?? 0) + bytes);
    }
    for (const child of node.children ?? []) visit(child, in_package);
  };
  visit(profile.head);
  return {
    total_bytes,
    package_bytes,
    top_package_functions: [...by_function.entries()]
      .sort((a, b) => b[1] - a[1])
      .slice(0, 8),
  };
}

test.beforeEach(async ({ page }) => {
  page.on("pageerror", (error) => console.log(`[browser:pageerror] ${error.message}`));
  await page.goto("/");
  await wait_chart(page);
});

test("a one-row batch is identical to update() with the same point", async ({ page }) => {
  const result = await page.evaluate(async () => {
    // Two candlestick series with identical starting data on the same chart, so they share the
    // time axis, the layout, and the frame — any divergence is the append path's own.
    const base = [];
    let price = 100;
    const t0 = 1577836800;
    for (let i = 0; i < 200; i += 1) {
      const next = price + Math.sin(i * 0.11) * 0.7;
      base.push({ time: t0 + i * 60, open: price, high: Math.max(price, next) + 0.3, low: Math.min(price, next) - 0.3, close: next });
      price = next;
    }
    const a = window.__chart.add_series("candlestick", { visible: false });
    const b = window.__chart.add_series("candlestick", { visible: false });
    a.set_data(base);
    b.set_data(base);

    const scopes = { a: [], b: [] };
    a.subscribe_data_changed((scope) => scopes.a.push(scope));
    b.subscribe_data_changed((scope) => scopes.b.push(scope));

    const point = { time: t0 + 200 * 60, open: 111.5, high: 112.25, low: 110.75, close: 111.0 };
    a.update(point);
    // Same point as one-row columns.
    b.update_typed({
      times: new Float64Array([point.time]),
      open: new Float64Array([point.open]),
      high: new Float64Array([point.high]),
      low: new Float64Array([point.low]),
      close: new Float64Array([point.close]),
    });

    // And a replace-last of that same bar, through both paths.
    const revised = { time: point.time, open: 111.5, high: 113.0, low: 110.5, close: 112.75 };
    a.update(revised);
    b.update_typed({
      times: new Float64Array([revised.time]),
      open: new Float64Array([revised.open]),
      high: new Float64Array([revised.high]),
      low: new Float64Array([revised.low]),
      close: new Float64Array([revised.close]),
    });

    return {
      data_a: a.data(),
      data_b: b.data(),
      scopes,
      last_a: a.last_value_data(true),
      last_b: b.last_value_data(true),
    };
  });

  // Same `data()` result, row for row.
  expect(result.data_b).toEqual(result.data_a);
  // Same `data_changed` scope sequence: one "update" per call on both paths.
  expect(result.scopes.b).toEqual(result.scopes.a);
  expect(result.scopes.a).toEqual(["update", "update"]);
  expect(result.last_b).toEqual(result.last_a);
});

test("a one-row batch renders identically to update()", async ({ page }) => {
  // Same series, same appended point, driven through each path in turn — the presented frames
  // must match pixel for pixel.
  const shots = await page.evaluate(async () => {
    const build = (n) => {
      const out = [];
      let price = 100;
      for (let i = 0; i < n; i += 1) {
        const next = price + Math.sin(i * 0.13) * 0.6;
        out.push({ time: 1577836800 + i * 60, open: price, high: Math.max(price, next) + 0.3, low: Math.min(price, next) - 0.3, close: next });
        price = next;
      }
      return out;
    };
    const base = build(300);
    const point = { time: 1577836800 + 300 * 60, open: 105, high: 106.5, low: 104.25, close: 106.0 };
    const capture = (apply) => {
      window.__main.set_data(base);
      apply();
      window.__chart.render();
      return window.__chart.take_screenshot().toDataURL();
    };
    const via_update = capture(() => window.__main.update(point));
    const via_typed = capture(() => window.__main.update_typed({
      times: new Float64Array([point.time]),
      open: new Float64Array([point.open]),
      high: new Float64Array([point.high]),
      low: new Float64Array([point.low]),
      close: new Float64Array([point.close]),
    }));
    return { via_update, via_typed };
  });
  expect(shots.via_typed).toBe(shots.via_update);
});

test("a 500-row batch is one call and one data_changed notification", async ({ page }) => {
  const result = await page.evaluate(async () => {
    const series = window.__chart.add_series("line", { visible: false });
    const t0 = 1577836800;
    series.set_data([{ time: t0, value: 100 }]);

    const scopes = [];
    series.subscribe_data_changed((scope) => scopes.push(scope));

    const ROWS = 500;
    const times = new Float64Array(ROWS);
    const values = new Float64Array(ROWS);
    for (let i = 0; i < ROWS; i += 1) {
      times[i] = t0 + (i + 1) * 60;
      values[i] = 100 + Math.sin(i * 0.05) * 5;
    }
    // Single-value series: the same view as all four price channels, per the documented
    // "repeat their value in all four price channels" contract. This is also the aliasing case —
    // if the engine sorted or sanitized in place, this would corrupt.
    const before = Array.from(values.slice(0, 8));
    series.update_typed({ times, open: values, high: values, low: values, close: values });
    const after = Array.from(values.slice(0, 8));

    const data = series.data();
    return {
      scopes,
      row_count: data.length,
      first: data[0],
      last: data[data.length - 1],
      expected_last_time: times[ROWS - 1],
      expected_last_value: values[ROWS - 1],
      input_unchanged: before.every((v, i) => v === after[i]),
      times_unchanged: times[0] === t0 + 60,
    };
  });

  expect(result.scopes).toEqual(["update"]);
  expect(result.row_count).toBe(501);
  expect(result.last.time).toBe(result.expected_last_time);
  expect(result.last.value).toBeCloseTo(result.expected_last_value, 10);
  // The aliasing guarantee documented on `ohlc_columns` / `set_data_typed`.
  expect(result.input_unchanged).toBe(true);
  expect(result.times_unchanged).toBe(true);
});

test("the batch is repaired like set_data_typed: sorted, deduped last-wins, non-finite dropped", async ({ page }) => {
  const result = await page.evaluate(() => {
    const series = window.__chart.add_series("line", { visible: false });
    const t0 = 1577836800;
    series.set_data([{ time: t0, value: 1 }]);
    // Out of order, one duplicate time (last wins), one non-finite row (dropped).
    const times = new Float64Array([t0 + 180, t0 + 60, t0 + 120, t0 + 60, t0 + 240]);
    const values = new Float64Array([30, 10, 20, 11, Number.NaN]);
    series.update_typed({ times, open: values, high: values, low: values, close: values });
    return series.data();
  });
  // t0 kept; +60 collapsed last-wins to 11; +120 and +180 in ascending order; the NaN-valued row
  // at +240 survives as whitespace (an all-NaN row is a whitespace point, not an invalid one).
  expect(result.map((r) => r.time)).toEqual([
    1577836800, 1577836860, 1577836920, 1577836980, 1577837040,
  ]);
  expect(result[1].value).toBe(11);
  expect(result[2].value).toBe(20);
  expect(result[3].value).toBe(30);
  expect(result[4].value).toBeUndefined();
});

test("appending 1M points in batches allocates no per-point JS objects", async ({ page }) => {
  test.setTimeout(300_000);
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("HeapProfiler.enable");
  await cdp.send("HeapProfiler.startSampling", {
    samplingInterval: 4096,
    includeObjectsCollectedByMajorGC: true,
    includeObjectsCollectedByMinorGC: true,
  });
  const result = await page.evaluate(async () => {
    const series = window.__chart.add_series("line", { visible: false });
    // Start past every other series' last timestamp. Appending at the *global* tip is the
    // streaming shape, and the one the engine's single-append fast path is built for; a row that
    // lands mid-history costs a reindex, exactly as it does through `update`.
    const t0 = 4_000_000_000;
    series.set_data([{ time: t0, value: 100 }]);

    const BATCH = 1000;
    const BATCHES = 1000; // 1M points
    // The whole point of the API: these views are allocated once and refilled in place. `values`
    // is passed as all four price channels (the single-value convention).
    const times = new Float64Array(BATCH);
    const values = new Float64Array(BATCH);
    const columns = { times, open: values, high: values, low: values, close: values };

    const heap = () => performance.memory?.usedJSHeapSize ?? 0;
    let next_time = t0 + 60;
    let last_value = 0;
    const fill = () => {
      for (let i = 0; i < BATCH; i += 1) {
        times[i] = next_time;
        next_time += 60;
        last_value = 100 + Math.sin(next_time * 1e-4) * 5;
        values[i] = last_value;
      }
    };
    // Warm, then let the GC settle so the baseline is not read mid-collection.
    fill();
    series.update_typed(columns);
    await new Promise((resolve) => setTimeout(resolve, 250));

    const heap_before = heap();
    const started = performance.now();
    for (let b = 0; b < BATCHES; b += 1) {
      fill();
      series.update_typed(columns);
    }
    const elapsed_ms = performance.now() - started;
    const heap_after = heap();

    return {
      elapsed_ms,
      points: BATCH * BATCHES,
      heap_growth_bytes: heap_after - heap_before,
      heap_available: heap_before > 0,
      engine_memory_bytes: window.__chart.frame_stats().memory_bytes,
      // Cheap correctness check that avoids materializing 1M JS objects via `data()`.
      last: series.last_value_data(true),
      expected_last_time: next_time - 60,
      expected_last_value: last_value,
    };
  });
  const { profile } = await cdp.send("HeapProfiler.stopSampling");
  const allocations = sampled_allocations(profile);
  await cdp.send("HeapProfiler.disable");

  expect(result.last.time).toBe(result.expected_last_time);
  expect(result.last.value).toBeCloseTo(result.expected_last_value, 10);
  console.log(
    `update_typed: ${result.points} points in ${result.elapsed_ms.toFixed(0)} ms `
    + `(${(result.points / (result.elapsed_ms / 1000) / 1e6).toFixed(2)} M pts/s), `
    + `JS heap growth ${result.heap_growth_bytes} B, engine memory ${result.engine_memory_bytes} B`,
  );
  console.log(`update_typed CDP sampled allocations: ${JSON.stringify(allocations)}`);
  // Sampling records allocations even when GC frees them before the end-of-run heap snapshot.
  // A per-point wrapper would allocate tens of MB across 1M points; package-attributed churn must
  // remain bounded by batches/calls instead.
  expect(allocations.package_bytes).toBeLessThan(4 * 1024 * 1024);
  if (result.heap_available) {
    // 1M per-point JS objects would be tens of megabytes. A few hundred KB of incidental churn
    // (the loop's own bookkeeping, GC timing) is expected; 8 MB is a generous ceiling that a
    // per-point allocation could not fit under.
    expect(result.heap_growth_bytes).toBeLessThan(8 * 1024 * 1024);
  }
});
