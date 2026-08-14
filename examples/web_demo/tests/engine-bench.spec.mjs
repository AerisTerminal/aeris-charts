import { test, expect } from "@playwright/test";

// Build-flag benchmark (consumer Item 7). The native `perf_gate` example measures engine CPU on
// the host target, which cannot show a wasm SIMD or wasm-opt effect at all — those only exist in
// the wasm artifact. This spec times the representative math-heavy workloads *through the shipped
// .wasm*, so the same numbers can be taken before and after a build-flag change.
//
// Run it against a build, record the numbers, change the flags, rebuild, run it again:
//   npx playwright test tests/engine-bench.spec.mjs --project=chromium
//
// Report-only by design: these are comparative numbers for a PR, not a gate. Absolute values on a
// software rasterizer mean nothing on their own.
//
// Opt-in via NUCLEUSCHARTS_BENCH=1, and skipped otherwise. Two reasons, both deliberate: it is a measuring
// instrument rather than a regression test, and it installs 1M bars five times over, which grows wasm
// substantial linear memory. Linear memory never shrinks, and that residue was observed starving the
// *next* spec's page init past a 30s timeout when this ran as part of the default suite.

const ENABLED = process.env.NUCLEUSCHARTS_BENCH === "1";

/** Best-of-N so a GC pause or a scheduler hiccup does not become the reported number. */
const REPEATS = 5;

async function wait_chart(page) {
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined);
  await page.evaluate(() => new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  }));
}

test.beforeEach(async ({ page }) => {
  test.skip(!ENABLED, "set NUCLEUSCHARTS_BENCH=1 to run the build-flag benchmark");
  page.on("pageerror", (error) => console.log(`[browser:pageerror] ${error.message}`));
  await page.goto("/");
  await wait_chart(page);
});

test("build-flag benchmark: 1M-bar install, autoscale, hit tests, frame build", async ({ page }) => {
  test.skip(!ENABLED, "set NUCLEUSCHARTS_BENCH=1 to run the build-flag benchmark");
  test.setTimeout(600_000);
  const results = await page.evaluate(async (repeats) => {
    const best = (fn) => {
      let ms = Infinity;
      for (let i = 0; i < repeats; i += 1) ms = Math.min(ms, fn());
      return ms;
    };
    const timed = (fn) => {
      const started = performance.now();
      fn();
      return performance.now() - started;
    };

    const t0 = 1577836800;
    const make_columns = (n) => {
      const times = new Float64Array(n);
      const open = new Float64Array(n);
      const high = new Float64Array(n);
      const low = new Float64Array(n);
      const close = new Float64Array(n);
      let price = 100;
      for (let i = 0; i < n; i += 1) {
        const next = price + Math.sin(i * 0.017) * 0.8;
        times[i] = t0 + i * 60;
        open[i] = price;
        high[i] = Math.max(price, next) + 0.4;
        low[i] = Math.min(price, next) - 0.4;
        close[i] = next;
        price = next;
      }
      return { times, open, high, low, close };
    };

    const out = {};

    // --- 1M-bar install: sanitize + column install + time-axis sync -----------------------------
    const million = make_columns(1_000_000);
    out.install_1m_bars_ms = best(() => timed(() => window.__main.set_data_typed(million)));

    // --- autoscale over a 1M-bar series: fit the whole range, then rebuild the frame -----------
    // `fit_content` + render forces a full autoscale pass over every visible bar.
    out.autoscale_1m_bars_ms = best(() => timed(() => {
      window.__chart.time_scale().fit_content();
      window.__chart.render();
    }));

    // --- frame build over a 50k-bar window (the 60fps target's shape) --------------------------
    window.__main.set_data_typed(make_columns(50_000));
    window.__chart.time_scale().fit_content();
    window.__chart.render();
    const FRAMES = 30;
    out.frame_build_50k_ms = best(() => timed(() => {
      for (let i = 0; i < FRAMES; i += 1) window.__chart.render();
    })) / FRAMES;

    // --- hit-testing 10k drawings ---------------------------------------------------------------
    // Anchors spread across the 50k-bar logical range so the R-tree actually has to discriminate
    // (`drawing_point` is `{logical, price}` — bar indices, not timestamps).
    window.__chart.clear_drawings();
    const DRAWINGS = 10_000;
    const BARS = 50_000;
    for (let i = 0; i < DRAWINGS; i += 1) {
      const start = (i / DRAWINGS) * BARS;
      window.__chart.add_drawing("trend_line", [
        { logical: start, price: 95 + (i % 40) * 0.25 },
        { logical: start + BARS / DRAWINGS, price: 96 + (i % 40) * 0.25 },
      ]);
    }
    const HITS = 2_000;
    out.hit_test_10k_drawings_us = best(() => timed(() => {
      for (let i = 0; i < HITS; i += 1) {
        // Sweep the pane so each probe lands in a different R-tree neighbourhood.
        window.__chart.wasm.hover_at(40 + (i * 13) % 1000, 60 + (i * 7) % 500);
      }
    })) / HITS * 1000;
    window.__chart.clear_drawings();

    out.wasm_memory_bytes = window.__chart.frame_stats().memory_bytes;
    out.backend = window.__chart.backend();
    return out;
  }, REPEATS);

  console.log(`ENGINE BENCH ${JSON.stringify(results, null, 2)}`);

  // Sanity only — the numbers are the deliverable, not the assertion.
  expect(results.install_1m_bars_ms).toBeGreaterThan(0);
  expect(results.autoscale_1m_bars_ms).toBeGreaterThan(0);
  expect(results.frame_build_50k_ms).toBeGreaterThan(0);
  expect(results.hit_test_10k_drawings_us).toBeGreaterThan(0);
});

test("indicator benchmark: 1M RSI current/batch latency and linear-memory delta", async ({ page }) => {
  test.skip(!ENABLED, "set NUCLEUSCHARTS_BENCH=1 to run the build-flag benchmark");
  test.setTimeout(600_000);
  const result = await page.evaluate(() => {
    const rows = 1_000_000;
    const t0 = 2_500_000_000;
    const times = new Float64Array(rows);
    const values = new Float64Array(rows);
    for (let index = 0; index < rows; index += 1) {
      times[index] = t0 + index * 60;
      values[index] = 100 + Math.sin(index * 0.017) * 8;
    }
    const source = window.__chart.add_series("line", { visible: false });
    source.set_data_typed({ times, open: values, high: values, low: values, close: values });
    const before_indicator_bytes = window.__chart.frame_stats().memory_bytes;
    window.__chart.add_rsi(source, 14, { visible: false });
    const after_indicator_bytes = window.__chart.frame_stats().memory_bytes;

    const last_time = times[rows - 1];
    let started = performance.now();
    for (let update = 0; update < 10_000; update += 1) {
      source.update({ time: last_time, value: values[rows - 1] + update * 0.000001 });
    }
    const current_update_us = (performance.now() - started) * 1000 / 10_000;

    const batch = 10_000;
    const batch_times = new Float64Array(batch);
    const batch_values = new Float64Array(batch);
    for (let index = 0; index < batch; index += 1) {
      batch_times[index] = last_time + (index + 1) * 60;
      batch_values[index] = 100 + Math.cos(index * 0.031) * 4;
    }
    started = performance.now();
    source.update_typed({
      times: batch_times,
      open: batch_values,
      high: batch_values,
      low: batch_values,
      close: batch_values,
    });
    const batch_10k_ms = performance.now() - started;
    return {
      before_indicator_bytes,
      after_indicator_bytes,
      indicator_delta_bytes: after_indicator_bytes - before_indicator_bytes,
      current_update_us,
      batch_10k_ms,
    };
  });
  console.log(`INDICATOR BENCH ${JSON.stringify(result, null, 2)}`);
  expect(result.current_update_us).toBeGreaterThan(0);
  expect(result.batch_10k_ms).toBeGreaterThan(0);
});
