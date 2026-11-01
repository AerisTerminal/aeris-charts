import { expect, test } from "@playwright/test";

// Runs on `chromium` (standard bundle) and `chromium-complete` (the complete bundle served at the
// same path), so each build proves which WebAssembly module it loaded.
test("the engine reports the loaded build and both builds create shared families", async ({ page }, test_info) => {
  const expected = test_info.project.name === "chromium-complete" ? "complete" : "standard";
  // A page that loads no chart, so the module is still uninitialized when the spec imports it.
  await page.route("**/build_variant_blank.html", (route) => route.fulfill({
    contentType: "text/html; charset=utf-8",
    body: "<!doctype html><meta charset=utf-8><title>build variant</title><body></body>",
  }));
  await page.goto("/build_variant_blank.html");
  const result = await page.evaluate(async () => {
    const api = await import("/dist/aeris_charts_financial.js");
    let before = null;
    try {
      api.build_variant();
    } catch (error) {
      before = { name: error.name, code: error.code };
    }
    const host = document.createElement("div");
    host.style.cssText = "position:relative;width:320px;height:200px";
    document.body.append(host);
    const chart = await api.create_chart(host, {
      backend: "canvas2d",
      initialPane: { horizontal_domain: { type: "category", scale: "band" } },
    });
    chart.add_axis({ id: "x", pane: 0, dimension: "x", scale: "band" });
    chart.add_axis({ id: "y", pane: 0, dimension: "y", scale: "linear" });
    const column = chart.add_series("column", { pane: 0, x_axis_id: "x", y_axis_id: "y" });
    column.set_data([{ id: "a", x: "A", y: 1 }]);
    // The G11 statistics families exist only in the complete build; creation must fail
    // atomically in the standard build and succeed in the complete one.
    let violin = null;
    try {
      const handle = chart.add_series("violin", { pane: 0, x_axis_id: "x", y_axis_id: "y" });
      handle.set_data([{ x: "A", y: 1 }, { x: "A", y: 3 }, { x: "A", y: 2 }]);
      violin = "created";
    } catch (error) {
      violin = error.code;
    }
    // Statistics options are complete-only too: the standard build rejects them before the
    // engine sees them, and the complete build rejects them on a non-statistics family.
    let stats_options = null;
    try {
      column.apply_options({ stats: { kernel: "epanechnikov" } });
      stats_options = "applied";
    } catch (error) {
      stats_options = error.code;
    }
    chart.render();
    const variant = api.build_variant();
    const camel = api.buildVariant();
    chart.remove();
    host.remove();
    const wasm = performance.getEntriesByType("resource")
      .map((entry) => new URL(entry.name).pathname)
      .filter((path) => path.endsWith(".wasm"));
    return { before, variant, camel, wasm, violin, stats_options };
  });
  expect(result).toEqual({
    before: { name: "AerisChartsError", code: "unsupported_operation" },
    variant: expected,
    camel: expected,
    wasm: [expected === "complete" ? "/dist/aeris_charts_wasm_complete_bg.wasm" : "/dist/aeris_charts_wasm_bg.wasm"],
    violin: expected === "complete" ? "created" : "complete_build_required",
    stats_options: expected === "complete" ? "invalid_options" : "complete_build_required",
  });
});
