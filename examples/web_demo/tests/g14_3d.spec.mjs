import { expect } from "@playwright/test";
import { test } from "./page_ready.mjs";
import { bars, helix, orthographic, quarters, regions, scatter, scatter_size, series_color, surface }
  from "../echarts_reference/rows/chart3d.js";

// G14 3D bar, scatter, line and surface charts are complete-build only: this spec runs in the
// `chromium-complete` project (see playwright.config.mjs). build_variant.spec.mjs proves the
// standard build rejects them with `complete_build_required`.

const bar_data = {
  type: "points_3d",
  points: bars,
  x_axis: { name: "Quarter", categories: quarters },
  y_axis: { name: "Region", categories: regions },
  z_axis: { name: "Revenue" },
};
const scatter_data = { type: "points_3d", points: scatter, size: scatter_size };
const line_data = { type: "points_3d", points: helix };
const surface_data = { type: "surface", ...surface };
const families = [
  { kind: "bar_3d", data: bar_data, element: "point", rows: bars.length },
  { kind: "scatter_3d", data: scatter_data, element: "point", rows: scatter.length },
  { kind: "line_3d", data: line_data, element: "point", rows: helix.length },
  { kind: "surface_3d", data: surface_data, element: "sample", rows: surface.z.length },
];

async function blank_page(page) {
  await page.route("**/g14_blank.html", (route) => route.fulfill({
    contentType: "text/html; charset=utf-8",
    body: "<!doctype html><meta charset=utf-8><title>g14</title><body></body>",
  }));
  await page.goto("/g14_blank.html");
}

// Mounts one 420 by 320 chart and exposes scan helpers on `window.__g14`.
const mount = async ({ backend, fixed }) => {
  const api = await import("/dist/aeris_charts_financial.js");
  const host = document.createElement("div");
  host.style.cssText = fixed
    ? "position:fixed;left:0px;top:0px;width:420px;height:320px;z-index:9999"
    : "position:relative;width:420px;height:320px";
  document.body.append(host);
  const chart = await api.create_chart(host, {
    backend,
    __force_webgpu_fallback_adapter: true,
    autoSize: false,
    theme: "light",
    initialPane: { horizontal_domain: { type: "layout" } },
  });
  if (chart.backend() !== backend) throw new Error(`expected ${backend}, got ${chart.backend()}`);
  chart.resize(420, 320, 1);
  const left = chart.panes()[0].get_geometry().left;
  // Hit regions per row: pixel count and centroid in chart coordinates.
  const scan = (step = 1) => {
    const regions = {};
    for (let y = 0; y < 320; y += step) {
      for (let x = 0; x < 420 - left; x += step) {
        const hit = chart.layout_hit_test(0, x, y);
        if (hit === null) continue;
        const region = (regions[`${hit.element}:${hit.id}`] ??= { x: 0, y: 0, count: 0, hit });
        region.x += x;
        region.y += y;
        region.count += 1;
      }
    }
    return Object.fromEntries(Object.entries(regions).map(([key, region]) =>
      [key, { x: region.x / region.count + left, y: region.y / region.count, count: region.count, hit: region.hit }]));
  };
  const painted = () => {
    chart.render();
    const canvas = chart.take_screenshot();
    const image = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
    const corner = `${image[0]},${image[1]},${image[2]}`;
    let count = 0;
    for (let offset = 0; offset < image.length; offset += 16) {
      if (`${image[offset]},${image[offset + 1]},${image[offset + 2]}` !== corner) count += 1;
    }
    return count;
  };
  window.__g14 = { api, chart, host, left, scan, painted };
  return left;
};

const unmount = () => {
  const { chart, host, extra = [] } = window.__g14;
  for (const item of extra) item.detach?.() ?? item.remove?.();
  chart.remove();
  host.remove();
  delete window.__g14;
};

for (const backend of ["canvas2d", "webgpu"]) test(`3D families paint, hit and describe their items through ${backend}`, async ({ page }) => {
  await blank_page(page);
  await page.evaluate(mount, { backend, fixed: false });
  const report = await page.evaluate(({ families, series_color }) => {
    const { chart, scan, painted } = window.__g14;
    const blank = painted();
    const results = [];
    for (const family of families) {
      const series = chart.add_layout_series(family.kind, { pane: 0, title: family.kind, color: series_color }, family.data);
      const regions = scan(2);
      const snapshot = series.accessibility_snapshot(0, 512);
      results.push({
        kind: family.kind,
        painted: painted(),
        elements: [...new Set(Object.values(regions).map((region) => region.hit.element))],
        series_ids: [...new Set(Object.values(regions).map((region) => region.hit.series))],
        id: series.id,
        regions: Object.keys(regions).length,
        sample: Object.values(regions)[0]?.hit ?? null,
        total: snapshot.total,
        first: snapshot.items[0],
        camera: series.options().camera_3d,
      });
      series.remove();
    }
    return { blank, results };
  }, { families, series_color });
  for (const [index, family] of families.entries()) {
    const result = report.results[index];
    expect(result.painted, `${family.kind} paints its box and mesh`).toBeGreaterThan(report.blank + 400);
    expect(result.elements, family.kind).toEqual([family.element]);
    expect(result.series_ids, family.kind).toEqual([result.id]);
    expect(result.regions, `${family.kind} answers hits`).toBeGreaterThan(family.kind === "line_3d" ? 5 : 3);
    expect(result.sample).toMatchObject({ element: family.element, title: family.kind });
    expect(result.total, family.kind).toBe(family.rows);
    expect(result.camera).toEqual({ alpha: 20, beta: 40, distance: 200, projection: "perspective", orthographic_size: 150 });
  }
  expect(report.results[0].first).toMatchObject({ element: "point", label: "Q1 North", value: 4 });
  expect(report.results[1].first).toMatchObject({ element: "point", label: "zero", value: 0 });
  await page.evaluate(unmount);
});

test("3D scatter projection matches the ECharts GL 2.1.0 grid3D camera", async ({ page }) => {
  await page.goto("/echarts_reference.html?case=chart3d");
  await page.waitForFunction(() => window.__echarts_reference_ready === true);
  const reference = await page.evaluate((scatter) => Object.fromEntries(["scatter_3d", "scatter_3d_orthographic"]
    .map((id) => [id, scatter.map((point) => window.__echarts_gl_project(id, [point.x, point.y, point.z]))])), scatter);

  await blank_page(page);
  await page.evaluate(mount, { backend: "canvas2d", fixed: false });
  const cameras = {
    scatter_3d: {},
    scatter_3d_orthographic: {
      projection: "orthographic", orthographic_size: orthographic.aeris_size,
      alpha: orthographic.alpha, beta: orthographic.beta,
    },
  };
  const aeris = await page.evaluate(({ cameras, scatter_data }) => {
    const { chart, scan } = window.__g14;
    const result = {};
    for (const [id, camera] of Object.entries(cameras)) {
      const series = chart.add_layout_series("scatter_3d", { pane: 0, camera_3d: camera }, scatter_data);
      result[id] = scan(1);
      series.remove();
    }
    return result;
  }, { cameras, scatter_data });
  await page.evaluate(unmount);

  for (const id of Object.keys(cameras)) {
    const projected = reference[id];
    // Only whole symbols give unbiased centroids; a symbol partly behind another is skipped.
    const largest = Math.max(...Object.values(aeris[id]).map((region) => region.count));
    const pairs = scatter.map((_, row) => [projected[row], aeris[id][`point:${row}`]])
      .filter(([, region]) => region !== undefined && region.count >= largest * 0.9);
    expect(pairs.length, `${id} has enough unoccluded symbols`).toBeGreaterThanOrEqual(5);
    // Fit Aeris pixels = center + height * ECharts offset; the plot rectangles differ in size
    // and position, but one camera maps the same points to the same normalized offsets.
    const mean = (values) => values.reduce((sum, value) => sum + value, 0) / values.length;
    const [mu, mv] = [mean(pairs.map(([p]) => p.x)), mean(pairs.map(([p]) => p.y))];
    const [mx, my] = [mean(pairs.map(([, a]) => a.x)), mean(pairs.map(([, a]) => a.y))];
    const numerator = pairs.reduce((sum, [p, a]) => sum + (a.x - mx) * (p.x - mu) + (a.y - my) * (p.y - mv), 0);
    const denominator = pairs.reduce((sum, [p]) => sum + (p.x - mu) ** 2 + (p.y - mv) ** 2, 0);
    const scale = numerator / denominator;
    expect(scale, `${id} plot height`).toBeGreaterThan(200);
    expect(scale, `${id} plot height`).toBeLessThanOrEqual(320);
    for (const [p, a] of pairs) {
      const error = Math.hypot(mx + scale * (p.x - mu) - a.x, my + scale * (p.y - mv) - a.y);
      expect(error, `${id} symbol at ${JSON.stringify(p)}`).toBeLessThan(1.5);
    }
  }
});

for (const backend of ["canvas2d", "webgpu"]) test(`3D orbit drag, wheel, keys, tooltip and focus run through the ${backend} host`, async ({ page }) => {
  await blank_page(page);
  await page.evaluate(mount, { backend, fixed: true });
  const setup = await page.evaluate(({ bar_data, series_color }) => {
    const { api, chart, host, scan } = window.__g14;
    const series = chart.add_layout_series("bar_3d", { pane: 0, title: "Revenue", color: series_color }, bar_data);
    const tooltip = api.create_general_tooltip(chart, host, { pane: 0 });
    const accessibility = api.enable_accessibility(chart, { chart_title: "Quarterly revenue" });
    window.__g14.series = series;
    window.__g14.extra = [tooltip, accessibility];
    chart.render();
    const regions = scan(2);
    const tallest = regions["point:9"];
    return { tallest: tallest && { x: tallest.x, y: tallest.y }, plot: chart.panes()[0].get_geometry() };
  }, { bar_data, series_color });
  expect(setup.tallest, "the Q4 North bar answers hits").toBeDefined();

  // Hovering a bar shows its label and height.
  await page.mouse.move(setup.tallest.x, setup.tallest.y);
  await expect.poll(() => page.evaluate(() =>
    window.__g14.host.querySelector('[role="tooltip"]')?.textContent ?? "")).toContain("Q4 North");
  expect(await page.evaluate(() => window.__g14.host.querySelector('[role="tooltip"]')?.textContent)).toContain("10");

  // A drag right turns the box right; it is not a click.
  const before = await page.evaluate(() => window.__g14.chart.take_screenshot().toDataURL());
  const start = { x: setup.plot.left + 40, y: 280 };
  await page.mouse.move(start.x, start.y);
  await page.mouse.down();
  await page.mouse.move(start.x + 30, start.y - 10, { steps: 4 });
  await page.mouse.move(start.x + 60, start.y - 20, { steps: 4 });
  await page.mouse.up();
  const dragged = await page.evaluate(() => {
    const { chart, series } = window.__g14;
    chart.render();
    return {
      camera: series.options().camera_3d,
      selected: chart.layout_selected_hit(),
      changed: chart.take_screenshot().toDataURL(),
    };
  });
  expect(dragged.camera.beta).toBeLessThan(40);
  expect(dragged.camera.alpha).toBeLessThan(20);
  expect(dragged.selected, "a drag is not a click").toBeNull();
  expect(dragged.changed).not.toBe(before);

  // The wheel moves the camera closer.
  await page.mouse.wheel(0, -100);
  await expect.poll(() => page.evaluate(() => window.__g14.series.options().camera_3d.distance)).toBeLessThan(200);

  // Orbit keys turn the pane under the pointer; Home restores the default view.
  await page.evaluate(() => window.__g14.host.querySelector('canvas[tabindex="-1"]').focus());
  const turned = await page.evaluate(() => window.__g14.series.options().camera_3d);
  await page.keyboard.press("ArrowLeft");
  await expect.poll(() => page.evaluate(() => window.__g14.series.options().camera_3d.beta)).toBeCloseTo(turned.beta + 5, 6);
  await page.keyboard.press("Home");
  await expect.poll(() => page.evaluate(() => window.__g14.series.options().camera_3d))
    .toEqual({ alpha: 20, beta: 40, distance: 200, projection: "perspective", orthographic_size: 150 });

  // Keyboard item focus announces bars in input order.
  await page.mouse.move(415, 315);
  await page.evaluate(() => {
    window.__g14.extra[1].focus(0);
    window.__g14.chart.render();
  });
  await page.keyboard.press("ArrowRight");
  await expect(page.locator(".aeris_charts-a11y-live-region")).toContainText("Q1 North");
  await page.keyboard.press("ArrowRight");
  await expect(page.locator(".aeris_charts-a11y-live-region")).toContainText("Q1 South");
  await page.keyboard.press("Enter");
  expect(await page.evaluate(() => window.__g14.chart.layout_selected_hit()?.label)).toBe("Q1 South");
  await page.keyboard.press("Escape");
  await page.evaluate(unmount);
});

test("3D data validates atomically and cameras survive persistence", async ({ page }) => {
  await blank_page(page);
  await page.evaluate(mount, { backend: "canvas2d", fixed: false });
  const result = await page.evaluate(async ({ bar_data, surface_data }) => {
    const { api, chart } = window.__g14;
    const code = (action) => {
      try {
        action();
        return "ok";
      } catch (error) {
        return error.code;
      }
    };
    const camera = { alpha: 35, beta: -60, distance: 150, projection: "orthographic", orthographic_size: 120 };
    const bars = chart.add_layout_series("bar_3d", { pane: 0, camera_3d: camera }, bar_data);
    const surface = chart.add_layout_series("surface_3d", { pane: 0 }, surface_data);
    const rejected = {
      category: code(() => bars.set_data({ ...bar_data, points: [{ x: 0.5, y: 0, z: 1 }] })),
      surface: code(() => surface.set_data({ ...surface_data, z: surface_data.z.slice(1) })),
      camera: code(() => chart.add_layout_series("bar_3d", { pane: 0, camera_3d: { alpha: 120 } }, bar_data)),
      wrong_kind: code(() => chart.add_layout_series("surface_3d", { pane: 0 }, bar_data)),
    };
    const unchanged = { rows: bars.accessibility_snapshot(0, 64).total, series: chart.layout_series().length };
    chart.render();
    const image = chart.take_screenshot().toDataURL();
    const state = JSON.stringify(chart.export_state());
    const host = document.createElement("div");
    host.style.cssText = "position:relative;width:420px;height:320px";
    document.body.append(host);
    const next = await api.create_chart(host, {
      backend: "canvas2d", autoSize: false, theme: "light",
      initialPane: { horizontal_domain: { type: "layout" } },
    });
    next.resize(420, 320, 1);
    next.import_state(state);
    next.render();
    const restored = {
      same_state: JSON.stringify(next.export_state()) === state,
      same_image: next.take_screenshot().toDataURL() === image,
      cameras: next.layout_series().map((series) => series.options().camera_3d),
    };
    next.remove();
    host.remove();
    return { rejected, unchanged, camera, restored };
  }, { bar_data, surface_data });
  await page.evaluate(unmount);
  expect(result.rejected).toEqual({
    category: "invalid_data", surface: "invalid_data", camera: "invalid_options", wrong_kind: "invalid_data",
  });
  expect(result.unchanged).toEqual({ rows: bars.length, series: 2 });
  expect(result.restored.same_state).toBe(true);
  expect(result.restored.same_image, "the restored camera paints the same frame").toBe(true);
  // The pane's camera is shared by its 3D series.
  expect(result.restored.cameras).toEqual([result.camera, result.camera]);
});
