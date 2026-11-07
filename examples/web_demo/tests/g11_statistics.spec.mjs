import { expect } from "@playwright/test";
import { test } from "./page_ready.mjs";

// G11 statistics families are complete-build only: this spec runs in the `chromium-complete`
// project (see playwright.config.mjs). The standard build rejects these kinds with
// `complete_build_required`; build_variant.spec.mjs proves that gating in both projects.

const BAND_AXES = [
  { id: "x", dimension: "x", scale: "band" },
  { id: "y", dimension: "y", scale: "linear" },
];
const LINEAR_AXES = [
  { id: "x", dimension: "x", scale: "linear" },
  { id: "y", dimension: "y", scale: "linear" },
];

// Deterministic sample sets shared by the in-page scenarios.
const CATEGORY_ROWS = [
  { id: "n1", x: "North", y: 31 }, { id: "n2", x: "North", y: 44 }, { id: "n3", x: "North", y: 38 },
  { id: "n4", x: "North", y: 51 }, { id: "n5", x: "North", y: 40 }, { id: "n6", x: "North", y: 46 },
  { id: "s1", x: "South", y: 58 }, { id: "s2", x: "South", y: 63 }, { id: "s3", x: "South", y: 49 },
  { id: "s4", x: "South", y: 71 }, { id: "s5", x: "South", y: 55 }, { id: "s6", x: "South", y: 66 },
  { id: "e1", x: "East", y: 22 }, { id: "e2", x: "East", y: 35 }, { id: "e3", x: "East", y: 28 },
  { id: "e4", x: "East", y: 41 }, { id: "e5", x: "East", y: 30 }, { id: "e6", x: "East", y: 37 },
];
const VALUE_ROWS = [12, 18, 22, 25, 27, 29, 31, 33, 34, 36, 38, 41, 44, 47, 52, 58]
  .map((value, index) => ({ id: `v${index}`, value }));
const XY_ROWS = [
  ...Array.from({ length: 16 }, (_, index) => ({ id: `a${index}`, x: 18 + (index % 4) * 2.5, y: 24 + Math.floor(index / 4) * 3 })),
  ...Array.from({ length: 16 }, (_, index) => ({ id: `b${index}`, x: 58 + (index % 4) * 2.5, y: 56 + Math.floor(index / 4) * 3 })),
];
const MATRIX_ROWS = Array.from({ length: 10 }, (_, index) => ({
  id: `m${index}`,
  speed: 20 + index * 4,
  quality: 90 - index * 3,
  cost: 40 + (index % 3) * 9,
}));

async function blank_page(page) {
  await page.route("**/g11_blank.html", (route) => route.fulfill({
    contentType: "text/html; charset=utf-8",
    body: "<!doctype html><meta charset=utf-8><title>g11</title><body></body>",
  }));
  await page.goto("/g11_blank.html");
}

for (const backend of ["canvas2d", "webgpu"]) test(`statistics families render and answer hit tests through ${backend}`, async ({ page }) => {
  await blank_page(page);
  const report = await page.evaluate(async ({ backend, BAND_AXES, LINEAR_AXES, CATEGORY_ROWS, VALUE_ROWS, XY_ROWS, MATRIX_ROWS }) => {
    const api = await import("/dist/aeris_charts_financial.js");
    const scenario = async (axes, specs, domain) => {
      const host = document.createElement("div");
      host.style.cssText = "position:relative;width:640px;height:420px";
      document.body.append(host);
      const chart = await api.create_chart(host, {
        backend,
        __force_webgpu_fallback_adapter: true,
        autoSize: false,
        theme: "light",
        initialPane: { horizontal_domain: domain },
      });
      if (chart.backend() !== backend) throw new Error(`expected ${backend}, got ${chart.backend()}`);
      for (const axis of axes) chart.add_axis({ ...axis, pane: 0 });
      const handles = specs.map(({ kind, data, ...options }) => {
        const handle = chart.add_series(kind, { pane: 0, x_axis_id: "x", y_axis_id: "y", ...options });
        handle.set_data(data);
        return handle;
      });
      chart.render();
      const canvas = chart.take_screenshot();
      const image = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
      const key = (offset) => `${image[offset]},${image[offset + 1]},${image[offset + 2]}`;
      const corner = key(0);
      let painted = 0;
      for (let offset = 0; offset < image.length; offset += 16) {
        if (key(offset) !== corner) painted += 1;
      }
      const geometry = chart.panes()[0].get_geometry();
      // Scan a grid so every series of multi-series scenarios collects its own nearest hit.
      const found = new Map();
      for (let y = 12; y < geometry.height && found.size < handles.length; y += 10) {
        for (let x = 12; x < geometry.width && found.size < handles.length; x += 10) {
          const hit = chart.general_hit_test(0, x, y, 6);
          if (hit === null || found.has(hit.series)) continue;
          const handle = handles.find((candidate) => candidate.id === hit.series);
          if (handle !== undefined) {
            found.set(hit.series, { row: hit.row, detail: handle.data_at(hit.row)?.detail ?? null });
          }
        }
      }
      const hits = handles.map((handle) => found.get(handle.id) ?? null);
      const result = { painted, hits, options: handles.map((handle) => handle.options()) };
      chart.remove();
      host.remove();
      return result;
    };
    const band_domain = { type: "category", scale: "band" };
    const linear_domain = { type: "continuous", scale: "linear" };
    const out = {};
    out.violin = await scenario(BAND_AXES, [
      { kind: "violin", title: "v", stats: { violin_overlay: "box" }, data: CATEGORY_ROWS },
    ], band_domain);
    out.split_violin = await scenario(BAND_AXES, [
      { kind: "violin", title: "left", stats: { violin_side: "left" }, data: CATEGORY_ROWS },
      { kind: "violin", title: "right", stats: { violin_side: "right" }, data: CATEGORY_ROWS.map((row) => ({ ...row, y: row.y + 6 })) },
    ], band_domain);
    out.density_curve = await scenario(LINEAR_AXES, [
      { kind: "density_curve", title: "d", data: VALUE_ROWS },
      { kind: "density_curve", title: "d2", stats: { area: false, kernel: "epanechnikov", bandwidth: 4 }, data: VALUE_ROWS.map((row) => ({ ...row, value: row.value + 12 })) },
    ], linear_domain);
    out.ridgeline = await scenario(
      [{ id: "x", dimension: "x", scale: "linear" }, { id: "y", dimension: "y", scale: "band" }],
      [{ kind: "ridgeline", title: "r", stats: { overlap: 1.6 }, data: CATEGORY_ROWS }],
      linear_domain,
    );
    out.strip_plot = await scenario(BAND_AXES, [
      { kind: "strip_plot", title: "s", stats: { jitter: 0.9 }, data: CATEGORY_ROWS },
    ], band_domain);
    out.beeswarm = await scenario(BAND_AXES, [
      { kind: "beeswarm", title: "b", data: CATEGORY_ROWS },
    ], band_domain);
    out.dot_plot = await scenario(BAND_AXES, [
      { kind: "dot_plot", title: "w", data: CATEGORY_ROWS.map((row) => ({ ...row, y: Math.round(row.y / 8) })) },
    ], band_domain);
    out.ecdf = await scenario(LINEAR_AXES, [{ kind: "ecdf", title: "e", data: VALUE_ROWS }], linear_domain);
    out.qq_plot = await scenario(LINEAR_AXES, [{ kind: "qq_plot", title: "q", data: VALUE_ROWS }], linear_domain);
    out.hexbin = await scenario(LINEAR_AXES, [
      { kind: "hexbin", title: "h", stats: { hexbin_radius: 18 }, data: XY_ROWS },
    ], linear_domain);
    out.contour = await scenario(LINEAR_AXES, [
      { kind: "contour", title: "c", stats: { thresholds: 5 }, data: XY_ROWS },
    ], linear_domain);
    out.parallel_coordinates = await scenario(LINEAR_AXES, [
      { kind: "parallel_coordinates", title: "p", data: MATRIX_ROWS },
    ], linear_domain);
    out.scatter_matrix = await scenario(LINEAR_AXES, [
      { kind: "scatter_matrix", title: "m", data: MATRIX_ROWS },
    ], linear_domain);
    return out;
  }, { backend, BAND_AXES, LINEAR_AXES, CATEGORY_ROWS, VALUE_ROWS, XY_ROWS, MATRIX_ROWS });

  const kinds = [
    "violin", "split_violin", "density_curve", "ridgeline", "strip_plot", "beeswarm",
    "dot_plot", "ecdf", "qq_plot", "hexbin", "contour", "parallel_coordinates", "scatter_matrix",
  ];
  for (const kind of kinds) {
    const scenario = report[kind];
    expect(scenario.painted, `${kind} paints marks`).toBeGreaterThan(200);
    for (const [index, hit] of scenario.hits.entries()) {
      expect(hit, `${kind} series ${index} answers a hit test`).not.toBeNull();
    }
  }
  // Statistics marks carry the shared stats row detail: sample count and range.
  expect(report.violin.hits[0].detail).toMatchObject({ type: "stats" });
  expect(report.violin.hits[0].detail.count).toBeGreaterThan(0);
  expect(report.hexbin.hits[0].detail).toMatchObject({ type: "stats" });
  expect(report.hexbin.hits[0].detail.count).toBeGreaterThan(0);
  expect(report.violin.options[0].stats).toMatchObject({ violin_overlay: "box", kernel: "gaussian", bandwidth: "scott" });
  expect(report.density_curve.options[1].stats).toMatchObject({ area: false, kernel: "epanechnikov", bandwidth: 4 });
});

test("statistics options validate, update, and survive persistence", async ({ page }) => {
  await blank_page(page);
  const result = await page.evaluate(async ({ BAND_AXES, CATEGORY_ROWS, MATRIX_ROWS }) => {
    const api = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:relative;width:640px;height:420px";
    document.body.append(host);
    const chart = await api.create_chart(host, {
      backend: "canvas2d",
      autoSize: false,
      theme: "light",
      initialPane: { horizontal_domain: { type: "category", scale: "band" } },
    });
    for (const axis of BAND_AXES) chart.add_axis({ ...axis, pane: 0 });
    const violin = chart.add_series("violin", { pane: 0, x_axis_id: "x", y_axis_id: "y", stats: { violin_overlay: "quartiles", grid: 48 } });
    violin.set_data(CATEGORY_ROWS);
    chart.render();

    const errors = {};
    for (const [name, stats] of [
      ["kernel", { kernel: "triangle" }],
      ["bandwidth_rule", { bandwidth: "sturges" }],
      ["bandwidth_value", { bandwidth: -2 }],
      ["violin_side", { violin_side: "middle" }],
      ["qq_distribution", { qq_distribution: "lognormal" }],
    ]) {
      try {
        violin.apply_options({ stats });
        errors[name] = null;
      } catch (error) {
        errors[name] = error.code;
      }
    }
    // A stats patch replaces the stats object wholesale, like the histogram options.
    violin.apply_options({ stats: { violin_overlay: "none" } });
    const patched = violin.options().stats;

    // Non-statistics kinds reject non-default statistics options.
    let misplaced = null;
    const column = chart.add_series("column", { pane: 0, x_axis_id: "x", y_axis_id: "y" });
    column.set_data([{ x: "A", y: 3 }]);
    try {
      column.apply_options({ stats: { violin_overlay: "box" } });
    } catch (error) {
      misplaced = error.code;
    }

    // Matrix datasets update incrementally by row id.
    const matrix_host = document.createElement("div");
    matrix_host.style.cssText = "position:relative;width:640px;height:420px";
    document.body.append(matrix_host);
    const matrix_chart = await api.create_chart(matrix_host, {
      backend: "canvas2d",
      autoSize: false,
      theme: "light",
      initialPane: { horizontal_domain: { type: "continuous", scale: "linear" } },
    });
    matrix_chart.add_axis({ id: "x", pane: 0, dimension: "x", scale: "linear" });
    matrix_chart.add_axis({ id: "y", pane: 0, dimension: "y", scale: "linear" });
    const parallel = matrix_chart.add_series("parallel_coordinates", { pane: 0, x_axis_id: "x", y_axis_id: "y" });
    parallel.set_data(MATRIX_ROWS);
    let matrix_update = "ok";
    try {
      parallel.update_data([
        { id: "m0", speed: 90, quality: 12, cost: 70 },
        { id: "m10", speed: 12, quality: 88, cost: 22 },
      ]);
    } catch (error) {
      matrix_update = error.code;
    }
    matrix_chart.render();

    // Statistics options and kinds persist through export/import.
    const state = chart.export_state();
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:relative;width:640px;height:420px";
    document.body.append(restored_host);
    const restored = await api.create_chart(restored_host, {
      backend: "canvas2d",
      autoSize: false,
      theme: "light",
    });
    let imported = "ok";
    try {
      restored.import_state(state);
    } catch (error) {
      imported = error.code ?? String(error);
    }
    restored.render();
    const canvas = restored.take_screenshot();
    const image = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
    const key = (offset) => `${image[offset]},${image[offset + 1]},${image[offset + 2]}`;
    const corner = key(0);
    let restored_painted = 0;
    for (let offset = 0; offset < image.length; offset += 16) {
      if (key(offset) !== corner) restored_painted += 1;
    }
    const state_text = typeof state === "string" ? state : JSON.stringify(state);
    matrix_chart.remove();
    matrix_host.remove();
    restored.remove();
    restored_host.remove();
    chart.remove();
    host.remove();
    return {
      errors,
      patched: { violin_overlay: patched.violin_overlay, grid: patched.grid, bandwidth: patched.bandwidth },
      misplaced,
      matrix_update,
      imported,
      state_has_violin: state_text.includes('"Violin"'),
      state_has_overlay: state_text.includes('"None"'),
      restored_painted,
    };
  }, { BAND_AXES, CATEGORY_ROWS, MATRIX_ROWS });

  expect(result.errors).toEqual({
    kernel: "invalid_options",
    bandwidth_rule: "invalid_options",
    bandwidth_value: "invalid_options",
    violin_side: "invalid_options",
    qq_distribution: "invalid_options",
  });
  expect(result.patched).toEqual({ violin_overlay: "none", grid: 0, bandwidth: "scott" });
  expect(result.misplaced).toBe("invalid_options");
  expect(result.matrix_update).toBe("ok");
  expect(result.imported).toBe("ok");
  expect(result.state_has_violin).toBe(true);
  expect(result.state_has_overlay).toBe(true);
  expect(result.restored_painted).toBeGreaterThan(200);
});

test("the general dashboard mounts every complete-only card on the complete build", async ({ page }) => {
  await page.goto("/?theme=dark&demo=general");
  await page.waitForFunction(() => window.__generalDashboard?.ready === true);
  const cards = page.locator('#general_workspace .general-chart-card[data-complete-only="true"]');
  await expect(cards).toHaveCount(22);
  for (const card of await cards.all()) {
    await card.scrollIntoViewIfNeeded();
    await expect(card).toHaveAttribute("data-mounted", "true");
  }
  expect(await page.evaluate(() => window.__generalDashboard.errors)).toEqual([]);
});
