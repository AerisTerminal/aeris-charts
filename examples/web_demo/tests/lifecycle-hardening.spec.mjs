import { test, expect } from "@playwright/test";

async function wait_for_chart(page) {
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined);
}

test("retained chart disposal is idempotent and destroys every extension", async ({ page }) => {
  await page.goto("/?backend=canvas2d");
  await wait_for_chart(page);

  const result = await page.evaluate(() => {
    const chart = window.__chart;
    const series = window.__main;
    const destroyed = [];
    const detached = [];
    for (let index = 0; index < 3; index += 1) {
      chart.add_custom_series({
        price_value_builder: () => [1],
        render() {},
        destroy() {
          destroyed.push(index);
          if (index === 0) throw new Error("expected cleanup failure");
        },
      });
    }
    for (let index = 0; index < 3; index += 1) {
      chart.panes()[0].attach_canvas_primitive({
        detached() {
          detached.push(index);
          if (index === 0) throw new Error("expected cleanup failure");
        },
      });
    }
    const canvases_before = document.querySelectorAll("#chart_container canvas").length;
    chart.remove();
    chart.remove();
    let chart_error = "";
    let series_error = "";
    try { chart.backend(); } catch (error) { chart_error = String(error); }
    try { series.update({ time: 1, value: 1 }); } catch (error) { series_error = String(error); }
    return {
      destroyed,
      detached,
      canvases_before,
      canvases_after: document.querySelectorAll("#chart_container canvas").length,
      chart_error,
      series_error,
      loss_count: chart.backend_loss_count_for_test(),
    };
  });

  expect(result.canvases_before).toBeGreaterThan(0);
  expect(result.canvases_after).toBe(0);
  expect(result.destroyed).toEqual([0, 1, 2]);
  expect(result.detached).toEqual([0, 1, 2]);
  expect(result.chart_error).toContain("chart has been disposed");
  expect(result.series_error).toContain("series has been removed");
  expect(result.loss_count).toBe(0);
});

test("ingestion exposes structured repair and impossible-OHLC diagnostics", async ({ page }) => {
  await page.goto("/?backend=canvas2d");
  await wait_for_chart(page);
  const result = await page.evaluate(() => {
    const series = window.__main;
    series.set_data([{ time: 1, open: 10, high: 12, low: 8, close: 11 }]);
    const clean = series.last_ingestion_diagnostics();
    series.set_data([{ time: 1, open: 10, high: 9, low: 8, close: 11 }]);
    const semantic = series.last_ingestion_diagnostics();
    const values_after_semantic = series.data();
    series.set_data_typed({
      times: new Float64Array([2, 1, 2, 3, 4]),
      open: new Float64Array([2, 1, 22, Number.NaN, 1e20]),
      high: new Float64Array([3, 2, 23, 4, 1e20]),
      low: new Float64Array([1, 0, 21, 2, 1e20]),
      close: new Float64Array([2, 1, 22, 3, 1e20]),
    });
    return { clean, semantic, repaired: series.last_ingestion_diagnostics(), values_after_semantic };
  });
  expect(result.clean).toBeNull();
  expect(result.semantic).toMatchObject({
    status: "accepted_with_diagnostics",
    accepted: 1,
    semantic_anomalies: 1,
  });
  expect(result.values_after_semantic[0]).toMatchObject({ open: 10, high: 9, low: 8, close: 11 });
  expect(result.repaired).toMatchObject({
    accepted: 2,
    dropped_invalid: 2,
    dropped_non_finite: 1,
    dropped_out_of_range: 1,
    deduplicated: 1,
    reordered: true,
  });
});

test("removed pane handles never retarget a replacement pane", async ({ page }) => {
  await page.goto("/?backend=canvas2d");
  await wait_for_chart(page);
  const result = await page.evaluate(() => {
    const chart = window.__chart;
    const removed = chart.add_pane(true);
    const removed_index = removed.pane_index();
    chart.remove_pane(removed_index);
    const replacement = chart.add_pane(true);
    let error = "";
    try { removed.set_stretch_factor(7); } catch (value) { error = String(value); }
    return {
      removed_index,
      replacement_index: replacement.pane_index(),
      replacement_stretch: replacement.get_stretch_factor(),
      error,
    };
  });
  expect(result.replacement_index).toBe(result.removed_index);
  expect(result.replacement_stretch).toBe(1);
  expect(result.error).toContain("pane has been removed");
});

test("shared WebGPU loss wakes 1, 2, 8, and 16 live charts", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "WebGPU test hook is Chromium-only");
  await page.goto("/?backend=auto&forceFallbackAdapter=1");
  await wait_for_chart(page);
  test.skip(await page.evaluate(() => window.__chart.backend() !== "webgpu"), "WebGPU unavailable");

  for (const count of [1, 2, 8, 16]) {
    const result = await page.evaluate(async (count) => {
      const { create_chart } = await import("/dist/nucleuscharts_financial.js");
      const original = window.__chart;
      const charts = [];
      for (let index = 0; index < count; index += 1) {
        const host = document.createElement("div");
        host.style.cssText = "width:160px;height:100px;position:absolute;left:-10000px";
        document.body.append(host);
        const chart = await create_chart(host, {
          autoSize: false,
          __force_webgpu_fallback_adapter: true,
        });
        chart.add_series("line").set_data([{ time: 1, value: index + 1 }]);
        charts.push({ chart, host });
      }
      // The chart that first created the shared device may disappear without owning notification.
      original.remove();
      charts[0].chart.wasm.simulate_device_loss_for_test();
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      const counts = charts.map(({ chart }) => chart.backend_loss_count_for_test());
      const backends = charts.map(({ chart }) => chart.backend());
      const disposed = charts.pop();
      disposed.chart.remove();
      globalThis.dispatchEvent(new CustomEvent("nucleuscharts-chart-backend-lost", { detail: 999 }));
      await Promise.resolve();
      const disposed_count = disposed.chart.backend_loss_count_for_test();
      for (const { chart, host } of charts) {
        chart.remove();
        host.remove();
      }
      disposed.host.remove();
      return { counts, backends, disposed_count };
    }, count);
    expect(result.counts).toEqual(Array(count).fill(1));
    expect(result.backends).toEqual(Array(count).fill("canvas2d"));
    expect(result.disposed_count).toBe(1);
    await page.reload();
    await wait_for_chart(page);
    if (count !== 16 && await page.evaluate(() => window.__chart.backend() !== "webgpu")) test.skip();
  }
});
