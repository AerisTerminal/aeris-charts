import { test, expect } from "@playwright/test";

// `chart_api.frame_stats()` (consumer Item 1). The engine had no frame-timing surface at all, so
// a frame-time budget was unfalsifiable. These specs are the acceptance criteria for the new
// record: plausible values on both backends, `gpu_ms` present exactly where `timestamp-query` is,
// and — the one that matters most — that polling the record every frame does not itself cost
// anything measurable.

const STRICT = process.env.NUCLEUSCHARTS_PERF_STRICT === "1";

async function wait_chart(page) {
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined);
  await page.evaluate(() => new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  }));
}

/** Let `n` frames present, each with a repaint driven through the public API. */
async function present_frames(page, n) {
  await page.evaluate(async (count) => {
    for (let i = 0; i < count; i += 1) {
      window.__chart.render();
      await new Promise((resolve) => requestAnimationFrame(resolve));
    }
  }, n);
}

test.beforeEach(async ({ page }) => {
  page.on("pageerror", (error) => console.log(`[browser:pageerror] ${error.message}`));
});

test("reports plausible last-frame telemetry on the WebGPU backend", async ({ page }) => {
  await page.goto("/");
  await wait_chart(page);
  expect(await page.evaluate(() => window.__chart.backend())).toBe("webgpu");

  await present_frames(page, 5);
  const stats = await page.evaluate(() => window.__chart.frame_stats());

  expect(stats.cpu_ms).toBeGreaterThan(0);
  expect(Number.isFinite(stats.cpu_ms)).toBe(true);
  expect(stats.presented_frames).toBeGreaterThan(0);
  expect(stats.draw_calls).toBeGreaterThan(0);
  expect(stats.memory_bytes).toBeGreaterThan(0);
  // Linear memory is whole 64 KiB pages.
  expect(stats.memory_bytes % 65536).toBe(0);
  expect(stats.dropped_frames).toBeGreaterThanOrEqual(0);
  expect(stats.ring_overruns).toBe(0);
  // Engine-owned chrome, watermark, and crosshair labels are part of the shared render frame.
  // With no plugin `text_views` escape hatch attached, a WebGPU frame must not paint either
  // visible Canvas2D surface.
  expect(stats.canvas2d_ops, "WebGPU must issue zero visible Canvas2D paint operations").toBe(0);

  // Exercise changing price/time crosshair labels rather than accepting a static warmed frame.
  // Every move produces a distinct axis frame and must retain the zero-op guarantee.
  const moving_crosshair_ops = await page.evaluate(async () => {
    let maximum = 0;
    for (let i = 0; i < 24; i += 1) {
      const row = window.__data[500 + i * 7];
      window.__chart.set_crosshair_position(row.close, row.time, window.__main);
      await new Promise((resolve) => requestAnimationFrame(resolve));
      maximum = Math.max(maximum, window.__chart.frame_stats().canvas2d_ops);
    }
    window.__chart.clear_crosshair_position();
    return maximum;
  });
  expect(moving_crosshair_ops, "moving crosshair chrome must stay on WebGPU").toBe(0);
});

test("reports plausible last-frame telemetry on the Canvas2D fallback", async ({ page }) => {
  await page.goto("/?backend=canvas2d");
  await wait_chart(page);
  expect(await page.evaluate(() => window.__chart.backend())).toBe("canvas2d");

  await present_frames(page, 5);
  const stats = await page.evaluate(() => window.__chart.frame_stats());

  expect(stats.cpu_ms).toBeGreaterThan(0);
  expect(stats.presented_frames).toBeGreaterThan(0);
  expect(stats.memory_bytes).toBeGreaterThan(0);
  // The Canvas2D pane paints through the 2D context, so the engine's own op counter is non-zero
  // here by construction. (On WebGPU it is the axis overlay only — see the Item 4 spec.)
  expect(stats.canvas2d_ops).toBeGreaterThan(0);
});

test("gpu_ms is null on canvas2d and never throws", async ({ page }) => {
  await page.goto("/?backend=canvas2d");
  await wait_chart(page);
  // Arm collection, then present enough frames that a readback would have landed if one existed.
  await page.evaluate(() => window.__chart.frame_stats());
  await present_frames(page, 10);
  expect(await page.evaluate(() => window.__chart.frame_stats().gpu_ms)).toBeNull();
});

test("gpu_ms resolves on a WebGPU device that supports timestamp-query", async ({ page }) => {
  await page.goto("/");
  await wait_chart(page);

  const supported = await page.evaluate(async () => {
    const adapter = await navigator.gpu?.requestAdapter();
    return adapter?.features?.has("timestamp-query") ?? false;
  });

  // First read arms collection; the readback resolves a frame or two later.
  await page.evaluate(() => window.__chart.frame_stats());
  await present_frames(page, 20);
  const gpu_ms = await page.evaluate(() => window.__chart.frame_stats().gpu_ms);

  if (!supported) {
    // The documented degradation: no feature, no number, no error.
    expect(gpu_ms).toBeNull();
    test.info().annotations.push({ type: "note", description: "timestamp-query unsupported here" });
    return;
  }
  expect(gpu_ms).not.toBeNull();
  expect(gpu_ms).toBeGreaterThanOrEqual(0);
  expect(Number.isFinite(gpu_ms)).toBe(true);
});

test("reading frame_stats costs a negligible fraction of a frame", async ({ page }) => {
  test.setTimeout(120_000);
  await page.goto("/");
  await wait_chart(page);

  // The consumer's bar is "reading frame_stats() every frame for 60s does not measurably raise
  // cpu_ms". Timed A/B against `render()` cannot actually resolve that here: a SwiftShader frame
  // is ~25 ms with ~0.4 ms of pass-to-pass spread, which is far coarser than the cost being
  // claimed. So measure the read itself in a tight loop — high resolution, and it is the exact
  // quantity in question — and separately assert the engine's own record is untouched by it.
  const read = await page.evaluate(() => {
    const ITERATIONS = 50_000;
    const once = () => {
      const started = performance.now();
      for (let i = 0; i < ITERATIONS; i += 1) window.__chart.frame_stats();
      return (performance.now() - started) / ITERATIONS;
    };
    once(); // warm the JIT and the wasm-bindgen glue
    return Math.min(once(), once(), once());
  });
  console.log(`frame_stats() read cost: ${(read * 1000).toFixed(3)} µs/call`);
  // 0.02 ms is 0.25% of the consumer's 8 ms budget at 120 Hz, and still well above the real
  // cost of a fixed 64-byte copy plus an object literal.
  expect(read, `frame_stats() cost ${read} ms/call`).toBeLessThan(0.02);

  // And the engine-side record must not become self-referential: polling for a sustained stretch
  // leaves `cpu_ms` in the same range as a chart nobody instruments, and does not drift upward.
  const drift = await page.evaluate(async () => {
    const sample = async (poll) => {
      const seen = [];
      for (let i = 0; i < 120; i += 1) {
        window.__chart.render();
        if (poll) window.__chart.frame_stats();
        seen.push(window.__chart.frame_stats().cpu_ms);
        if (i % 30 === 29) await new Promise((resolve) => setTimeout(resolve, 0));
      }
      seen.sort((a, b) => a - b);
      return { median: seen[60], max: seen[seen.length - 1] };
    };
    await sample(true); // warm
    return { unpolled: await sample(false), polled: await sample(true) };
  });
  const detail = JSON.stringify(drift);
  console.log(`engine cpu_ms with/without polling: ${detail}`);
  // Both arms read the record once per frame to observe it at all, so this is a sanity band, not
  // a fine-grained A/B: an accidental O(frames) cost inside the record would blow well past it.
  expect(drift.polled.median, detail).toBeLessThan(Math.max(drift.unpolled.median * 3, 1));
  if (STRICT) {
    expect(drift.polled.median / drift.unpolled.median, detail).toBeLessThan(1.25);
  }
});


test("continuous crosshair stays within the 8 ms CPU budget on 50k bars", async ({ page }) => {
  test.setTimeout(120_000);
  await page.goto("/");
  await wait_chart(page);
  expect(await page.evaluate(() => window.__chart.backend())).toBe("webgpu");

  const result = await page.evaluate(() => {
    const bars = 50_000;
    const times = new Float64Array(bars);
    const open = new Float64Array(bars);
    const high = new Float64Array(bars);
    const low = new Float64Array(bars);
    const close = new Float64Array(bars);
    let price = 100;
    for (let i = 0; i < bars; i += 1) {
      const next = price + Math.sin(i * 0.017) * 0.8;
      times[i] = 1_577_836_800 + i * 60;
      open[i] = price;
      high[i] = Math.max(price, next) + 0.4;
      low[i] = Math.min(price, next) - 0.4;
      close[i] = next;
      price = next;
    }
    window.__main.set_data_typed({ times, open, high, low, close });
    window.__chart.time_scale().fit_content();
    window.__chart.render();

    const cpu = [];
    const wall = [];
    const before = JSON.parse(window.__chart.wasm.text_cache_debug()).rasterizations;
    let maximum_canvas2d_ops = 0;
    for (let i = 0; i < 180; i += 1) {
      const index = 15_000 + ((i * 137) % 20_000);
      const started = performance.now();
      window.__chart.set_crosshair_position(close[index], times[index], window.__main);
      wall.push(performance.now() - started);
      const stats = window.__chart.frame_stats();
      cpu.push(stats.cpu_ms);
      maximum_canvas2d_ops = Math.max(maximum_canvas2d_ops, stats.canvas2d_ops);
    }
    const percentile = (values, p) => {
      values.sort((a, b) => a - b);
      return values[Math.floor((values.length - 1) * p)];
    };
    return {
      cpu_p99_ms: percentile(cpu, 0.99),
      wall_p99_ms: percentile(wall, 0.99),
      maximum_canvas2d_ops,
      text_rasterizations: JSON.parse(window.__chart.wasm.text_cache_debug()).rasterizations - before,
    };
  });

  console.log(`50k continuous crosshair: ${JSON.stringify(result)}`);
  expect(result.maximum_canvas2d_ops).toBe(0);
  expect(result.cpu_p99_ms, JSON.stringify(result)).toBeLessThan(8);
});