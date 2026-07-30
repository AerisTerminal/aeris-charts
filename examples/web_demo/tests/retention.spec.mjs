import { test, expect } from "@playwright/test";

// `series_options.max_points` retention + `frame_stats().memory_bytes` (consumer Item 6). Wasm
// linear memory never shrinks, so an uncapped series in an 8-hour live session grows monotonically.
// These specs pin both halves of the answer: the default really is unbounded (documented, not
// silently capped), and a capped series reaches a memory plateau while keeping the newest window.

async function wait_chart(page) {
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined);
  await page.evaluate(() => new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  }));
}

test.beforeEach(async ({ page }) => {
  page.on("pageerror", (error) => console.log(`[browser:pageerror] ${error.message}`));
  await page.goto("/");
  await wait_chart(page);
});

test("series retention is unbounded by default", async ({ page }) => {
  const result = await page.evaluate(() => {
    const series = window.__chart.add_series("line", { visible: false });
    const t0 = 4_000_000_000;
    const N = 20_000;
    const times = new Float64Array(N);
    const values = new Float64Array(N);
    for (let i = 0; i < N; i += 1) { times[i] = t0 + i * 60; values[i] = 100 + (i % 17); }
    series.set_data_typed({ times, open: values, high: values, low: values, close: values });
    return { rows: series.data().length, requested: N };
  });
  expect(result.rows).toBe(result.requested);
});

test("max_points is a hard ceiling that keeps the newest window", async ({ page }) => {
  const result = await page.evaluate(() => {
    const CAP = 1_000;
    const series = window.__chart.add_series("line", { max_points: CAP, visible: false });
    const t0 = 4_000_000_000;
    const N = 25_000;
    const times = new Float64Array(N);
    const values = new Float64Array(N);
    for (let i = 0; i < N; i += 1) { times[i] = t0 + i * 60; values[i] = i; }
    // A full install is trimmed to the ceiling too, not just streaming appends.
    series.set_data_typed({ times, open: values, high: values, low: values, close: values });
    const after_install = series.data();

    // Now stream past it and confirm the ceiling holds every step of the way.
    let over_ceiling = 0;
    let under_floor = 0;
    const floor = CAP - Math.floor(CAP / 32);
    const one = new Float64Array(1);
    const one_value = new Float64Array(1);
    for (let i = 0; i < 3_000; i += 1) {
      one[0] = t0 + (N + i) * 60;
      one_value[0] = N + i;
      series.update_typed({ times: one, open: one_value, high: one_value, low: one_value, close: one_value });
      const rows = series.data().length;
      if (rows > CAP) over_ceiling += 1;
      if (rows < floor) under_floor += 1;
    }
    const final = series.data();
    return {
      cap: CAP,
      floor,
      install_rows: after_install.length,
      install_last_value: after_install[after_install.length - 1].value,
      over_ceiling,
      under_floor,
      final_rows: final.length,
      final_last_value: final[final.length - 1].value,
      final_first_value: final[0].value,
      expected_last_value: N + 2_999,
    };
  });

  expect(result.install_rows).toBeLessThanOrEqual(result.cap);
  expect(result.install_rows).toBeGreaterThanOrEqual(result.floor);
  // Oldest-first eviction: the survivors are the tail of the installed data.
  expect(result.install_last_value).toBe(24_999);
  expect(result.over_ceiling, "row count exceeded max_points").toBe(0);
  expect(result.under_floor, "row count fell below the documented hysteresis floor").toBe(0);
  expect(result.final_last_value).toBe(result.expected_last_value);
  expect(result.final_first_value).toBe(result.expected_last_value - result.final_rows + 1);
});

test("a capped series plateaus in memory while an uncapped one keeps growing", async ({ page }) => {
  test.setTimeout(600_000);
  // The consumer's criterion is that an 8-hour, one-bar-per-second run reaches a stable
  // `memory_bytes` plateau rather than growing monotonically. A single 8-hour pass cannot show
  // that: memory legitimately rises while the retention window fills. So run four such passes
  // (32 simulated hours, 115,200 points) and compare the growth in the **first quarter** against
  // the growth in the **second half**. A plateau means the late growth is ~zero; monotonic growth
  // means it keeps pace.
  const result = await page.evaluate(async () => {
    const HOUR_POINTS = 3_600;
    const POINTS = 32 * HOUR_POINTS; // 32 h at 1 bar/s
    const BATCH = 60;
    const t0 = 4_000_000_000;

    const run = (max_points) => {
      const series = window.__chart.add_series(
        "line",
        max_points ? { max_points, visible: false } : { visible: false },
      );
      const times = new Float64Array(BATCH);
      const values = new Float64Array(BATCH);
      const columns = { times, open: values, high: values, low: values, close: values };
      const curve = [];
      let t = t0;
      for (let written = 0; written < POINTS; written += BATCH) {
        for (let i = 0; i < BATCH; i += 1) {
          times[i] = t;
          t += 1;
          values[i] = 100 + Math.sin(t * 1e-3) * 3;
        }
        series.update_typed(columns);
        curve.push({ written, bytes: window.__chart.frame_stats().memory_bytes });
      }
      const at = (fraction) => curve[Math.floor((curve.length - 1) * fraction)].bytes;
      return {
        rows: series.data().length,
        // Growth while the window fills vs growth long after it is full.
        early_growth: at(0.25) - at(0),
        late_growth: at(1) - at(0.5),
        peak: Math.max(...curve.map((s) => s.bytes)),
      };
    };

    // Capped first, so a plateau cannot be an artifact of the uncapped run having already grown
    // linear memory past what the capped run would need.
    const capped = run(8 * HOUR_POINTS); // an 8-hour retention window
    const uncapped = run(0);
    return { points: POINTS, capped, uncapped };
  });

  console.log(`retention memory curve: ${JSON.stringify(result)}`);

  expect(result.capped.rows).toBeLessThanOrEqual(8 * 3_600);
  // The plateau: once the retention window is full, memory stops growing outright. Linear memory
  // comes in whole 64 KiB pages, so allow a couple of pages of allocator noise but no more.
  expect(result.capped.late_growth, `capped memory still grew ${result.capped.late_growth} B in the second half`)
    .toBeLessThanOrEqual(256 * 1024);
  // The uncapped series is the control: it keeps every point (the documented default) and its
  // memory keeps climbing in the same stretch where the capped one has flattened.
  expect(result.uncapped.rows).toBe(result.points);
  expect(result.uncapped.late_growth, "uncapped memory did not keep growing — control invalid")
    .toBeGreaterThan(result.capped.late_growth);
});

test("clearing max_points restores unbounded growth", async ({ page }) => {
  const result = await page.evaluate(() => {
    const series = window.__chart.add_series("line", { max_points: 100, visible: false });
    const t0 = 4_000_000_000;
    const N = 500;
    const times = new Float64Array(N);
    const values = new Float64Array(N);
    for (let i = 0; i < N; i += 1) { times[i] = t0 + i * 60; values[i] = i; }
    series.set_data_typed({ times, open: values, high: values, low: values, close: values });
    const capped_rows = series.data().length;

    series.apply_options({ max_points: 0 }); // 0 clears the cap
    series.set_data_typed({ times, open: values, high: values, low: values, close: values });
    return { capped_rows, uncapped_rows: series.data().length, requested: N };
  });
  expect(result.capped_rows).toBeLessThanOrEqual(100);
  expect(result.uncapped_rows).toBe(result.requested);
});

test("eviction only drops shared timestamps no other series holds", async ({ page }) => {
  const result = await page.evaluate(() => {
    const t0 = 4_000_000_000;
    const N = 400;
    const times = new Float64Array(N);
    const values = new Float64Array(N);
    for (let i = 0; i < N; i += 1) { times[i] = t0 + i * 60; values[i] = i; }
    const columns = { times, open: values, high: values, low: values, close: values };

    const capped = window.__chart.add_series("line", { max_points: 50, visible: false });
    const uncapped = window.__chart.add_series("line", { visible: false });
    capped.set_data_typed(columns);
    uncapped.set_data_typed(columns);
    // The capped series is trimmed again after both are installed, to prove the trim does not
    // disturb the other series' rows.
    capped.apply_options({ max_points: 25 });
    return { capped_rows: capped.data().length, uncapped_rows: uncapped.data().length, requested: N };
  });
  expect(result.capped_rows).toBeLessThanOrEqual(25);
  expect(result.uncapped_rows).toBe(result.requested);
});
