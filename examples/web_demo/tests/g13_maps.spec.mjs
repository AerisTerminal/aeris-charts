import { expect } from "@playwright/test";
import { test } from "./page_ready.mjs";
import { geojson, series_color, values } from "../echarts_reference/rows/choropleth.js";
import { links, mark_color, points } from "../echarts_reference/rows/geo_marks.js";

// G13 choropleth, bubble and flow maps are complete-build only: this spec runs in the `chromium-complete` project
// (see playwright.config.mjs). The standard build rejects the kind with `complete_build_required`;
// build_variant.spec.mjs proves that gating in both projects.

const map_data = { type: "geo", geojson, values, projection: { type: "equal_earth" } };
const feature_ids = geojson.features.map((feature) => feature.id);

// Two squares sharing one arc, the second reversing it, in planar coordinates.
const topology = {
  type: "Topology",
  objects: {
    regions: {
      type: "GeometryCollection",
      geometries: [
        { type: "Polygon", id: "a", properties: { name: "A" }, arcs: [[0, 1]] },
        { type: "Polygon", id: "b", properties: { name: "B" }, arcs: [[2, -1]] },
      ],
    },
  },
  arcs: [
    [[1, 0], [1, 1]],
    [[1, 1], [0, 1], [0, 0], [1, 0]],
    [[1, 0], [2, 0], [2, 1], [1, 1]],
  ],
};

async function blank_page(page) {
  await page.route("**/g13_blank.html", (route) => route.fulfill({
    contentType: "text/html; charset=utf-8",
    body: "<!doctype html><meta charset=utf-8><title>g13</title><body></body>",
  }));
  await page.goto("/g13_blank.html");
}

for (const backend of ["canvas2d", "webgpu"]) test(`choropleth renders, fills by value and answers hit tests through ${backend}`, async ({ page }) => {
  await blank_page(page);
  const report = await page.evaluate(async ({ backend, map_data, series_color }) => {
    const api = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:relative;width:420px;height:320px";
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
    const series = chart.add_layout_series("choropleth", { pane: 0, title: "Growth", color: series_color }, map_data);
    chart.render();
    const canvas = chart.take_screenshot();
    const image = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
    const key = (offset) => `${image[offset]},${image[offset + 1]},${image[offset + 2]}`;
    const corner = key(0);
    let painted = 0;
    for (let offset = 0; offset < image.length; offset += 16) {
      if (key(offset) !== corner) painted += 1;
    }
    const left = chart.panes()[0].get_geometry().left;
    const hits = {};
    const centers = {};
    for (let y = 0; y < 320; y += 2) {
      for (let x = 0; x < 420; x += 2) {
        const hit = chart.layout_hit_test(0, x, y);
        if (hit === null) continue;
        hits[hit.id] ??= hit;
        const center = (centers[hit.id] ??= { x: 0, y: 0, count: 0, points: [] });
        center.x += x;
        center.y += y;
        center.count += 1;
        center.points.push([x, y]);
      }
    }
    // Sample each feature at its interior hit point nearest the hit centroid: away from borders,
    // strokes and holes, every hit six pixels around agrees.
    const samples = {};
    for (const [id, center] of Object.entries(centers)) {
      const cx = center.x / center.count;
      const cy = center.y / center.count;
      const interior = center.points.filter(([x, y]) => [[-6, 0], [6, 0], [0, -6], [0, 6], [-4, -4], [4, 4], [-4, 4], [4, -4]]
        .every(([dx, dy]) => chart.layout_hit_test(0, x + dx, y + dy)?.id === id));
      const [x, y] = interior.reduce((best, point) =>
        Math.hypot(point[0] - cx, point[1] - cy) < Math.hypot(best[0] - cx, best[1] - cy) ? point : best);
      const offset = (y * canvas.width + Math.round(x + left)) * 4;
      samples[id] = [image[offset], image[offset + 1], image[offset + 2]];
    }
    const snapshot = series.accessibility_snapshot(0, 512);
    chart.remove();
    host.remove();
    return { painted, hits, samples, snapshot };
  }, { backend, map_data, series_color });

  expect(report.painted, "the map paints its features").toBeGreaterThan(400);
  expect(Object.keys(report.hits).sort(), "every feature answers a hit").toEqual([...feature_ids].sort());
  for (const hit of Object.values(report.hits)) expect(hit.element).toBe("feature");
  expect(report.hits.east).toMatchObject({ label: "East", value: 9.5, title: "Growth" });
  expect(report.hits.isles).toMatchObject({ label: "Isles", value: null });
  // A sequential scale darkens toward the series color as the value grows; the valueless
  // feature takes the neutral missing color.
  const luminance = ([r, g, b]) => 0.2126 * r + 0.7152 * g + 0.0722 * b;
  const order = ["south", "west", "north", "central", "east"].map((id) => luminance(report.samples[id]));
  for (let index = 1; index < order.length; index += 1) {
    expect(order[index], `fill ${index} is darker than fill ${index - 1}`).toBeLessThan(order[index - 1]);
  }
  const [r, g, b] = report.samples.isles;
  expect(Math.max(r, g, b) - Math.min(r, g, b), "missing values are neutral gray").toBeLessThanOrEqual(8);
  expect(report.snapshot.total).toBe(feature_ids.length);
  expect(report.snapshot.items.map((item) => item.id)).toEqual(feature_ids);
  expect(report.snapshot.items[0]).toMatchObject({ element: "feature", id: "west", label: "West", value: 2.5 });
  expect(report.snapshot.items.at(-1)).toMatchObject({ id: "isles", value: null });
});

for (const backend of ["canvas2d", "webgpu"]) test(`choropleth hover tooltip, keyboard focus and legend run through the ${backend} host`, async ({ page }) => {
  await blank_page(page);
  const setup = await page.evaluate(async ({ backend, map_data, series_color }) => {
    const api = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    Object.assign(host.style, { width: "420px", height: "320px", position: "fixed", left: "0px", top: "0px", zIndex: "9999" });
    host.id = "g13-host";
    document.body.append(host);
    const chart = await api.create_chart(host, {
      backend,
      __force_webgpu_fallback_adapter: true,
      autoSize: false,
      theme: "light",
      initialPane: { horizontal_domain: { type: "layout" } },
    });
    chart.resize(420, 320, 1);
    const series = chart.add_layout_series("choropleth", { pane: 0, title: "Growth", color: series_color }, map_data);
    const tooltip = api.create_general_tooltip(chart, host, { pane: 0 });
    const legend_host = document.createElement("div");
    legend_host.style.cssText = "position:fixed;left:0px;top:340px";
    document.body.append(legend_host);
    const legend = api.create_general_legend(chart, legend_host, { pane: 0 });
    const accessibility = api.enable_accessibility(chart, { chart_title: "Regional growth" });
    let location = null;
    for (let y = 4; y < 316 && location === null; y += 4) {
      for (let x = 4; x < 416 && location === null; x += 4) {
        if (chart.layout_hit_test(0, x, y)?.id === "east") location = { x, y };
      }
    }
    window.__g13 = { api, chart, series, host, tooltip, legend, legend_host, accessibility, location };
    const bounds = host.getBoundingClientRect();
    const geometry = chart.panes()[0].get_geometry();
    chart.render();
    return {
      id: series.id,
      backend: chart.backend(),
      point: location && { x: bounds.left + geometry.left + location.x, y: bounds.top + location.y },
    };
  }, { backend, map_data, series_color });
  expect(setup.backend).toBe(backend);
  expect(setup.point).not.toBeNull();

  await page.mouse.move(setup.point.x, setup.point.y);
  await expect.poll(() => page.evaluate(() =>
    window.__g13.host.querySelector('[role="tooltip"]')?.textContent ?? "")).toContain("East");
  expect(await page.evaluate(() => window.__g13.host.querySelector('[role="tooltip"]')?.textContent)).toContain("9.5");
  await page.mouse.click(setup.point.x, setup.point.y);
  expect(await page.evaluate(() => window.__g13.chart.layout_selected_hit()?.id)).toBe("east");
  await page.keyboard.press("Escape");
  expect(await page.evaluate(() => window.__g13.chart.layout_selected_hit())).toBeNull();
  await page.mouse.move(415, 315);

  const before_focus = await page.evaluate(() => {
    window.__g13.accessibility.focus(0);
    window.__g13.chart.render();
    return window.__g13.chart.take_screenshot().toDataURL();
  });
  await page.keyboard.press("ArrowRight");
  await expect(page.locator("#g13-host .aeris_charts-a11y-live-region")).toContainText("West");
  await page.keyboard.press("ArrowRight");
  await expect(page.locator("#g13-host .aeris_charts-a11y-live-region")).toContainText("Central");
  const focus_changed = await page.evaluate(async (before) => {
    window.__g13.chart.render();
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    return window.__g13.chart.take_screenshot().toDataURL() !== before;
  }, before_focus);
  expect(focus_changed, "the focus ring paints").toBe(true);
  await page.keyboard.press("Enter");
  expect(await page.evaluate(() => window.__g13.chart.layout_selected_hit()?.id)).toBe("central");
  await page.keyboard.press("Escape");

  const button = page.locator(`[data-series="layout:${setup.id}"]`);
  await expect(button).toHaveAttribute("aria-label", "Hide Growth");
  await button.click();
  const hidden = await page.evaluate(() => {
    const { chart, series, location } = window.__g13;
    return {
      option: series.options().visible,
      persisted: chart.export_state().layout_series[0].visible,
      hit: chart.layout_hit_test(0, location.x, location.y),
    };
  });
  expect(hidden).toEqual({ option: false, persisted: false, hit: null });
  await button.click();
  expect(await page.evaluate(() => window.__g13.series.options().visible)).toBe(true);
  await page.evaluate(() => {
    const { chart, host, tooltip, legend, legend_host, accessibility } = window.__g13;
    accessibility.detach(); tooltip.detach(); legend.detach();
    chart.remove(); host.remove(); legend_host.remove();
    delete window.__g13;
  });
});

test("choropleth data validates atomically, accepts TopoJSON and survives persistence", async ({ page }) => {
  await blank_page(page);
  const result = await page.evaluate(async ({ map_data, topology }) => {
    const api = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:relative;width:420px;height:320px";
    document.body.append(host);
    const options = {
      backend: "canvas2d", autoSize: false, theme: "light",
      initialPane: { horizontal_domain: { type: "layout" } },
    };
    const chart = await api.create_chart(host, options);
    chart.resize(420, 320, 1);
    const code = (action) => {
      try {
        action();
        return "ok";
      } catch (error) {
        return error.code;
      }
    };
    const square = { type: "Polygon", coordinates: [[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]] };
    const unknown_value = code(() => chart.add_layout_series("choropleth", { pane: 0 }, {
      type: "geo", geojson: { type: "Feature", id: "a", geometry: square }, values: { b: 1 },
    }));
    const duplicate_id = code(() => chart.add_layout_series("choropleth", { pane: 0 }, {
      type: "geo", geojson: { type: "FeatureCollection", features: [
        { type: "Feature", id: "a", geometry: square }, { type: "Feature", id: "a", geometry: square },
      ] },
    }));
    const bad_classes = code(() => chart.add_layout_series("choropleth", { pane: 0 }, {
      ...map_data, scale: { type: "quantize", classes: 12 },
    }));
    const flat_polygon = code(() => chart.add_layout_series("choropleth", { pane: 0 }, {
      type: "geo", geojson: { type: "Polygon", coordinates: [[0, 0], [1, 0], [1, 1], [0, 0]] },
    }));
    const both_sources = code(() => chart.add_layout_series("choropleth", { pane: 0 }, {
      ...map_data, topojson: topology,
    }));
    const wrong_data = code(() => chart.add_layout_series("choropleth", { pane: 0 }, {
      type: "words", words: [{ text: "a", weight: 1 }],
    }));
    const wrong_kind = code(() => chart.add_layout_series("treemap", { pane: 0 }, map_data));
    const count_after_errors = chart.layout_series().length;

    const series = chart.add_layout_series("choropleth", { pane: 0, title: "Growth" }, map_data);
    const replace_error = code(() => series.set_data({ ...map_data, values: { nowhere: 1 } }));
    const kept = series.accessibility_snapshot(0, 512).total;
    series.set_data({
      type: "geo", topojson: topology, object: "regions", values: [{ id: "a", value: 1 }, { id: "b", value: 2 }],
      projection: { type: "identity" }, scale: { type: "quantize", classes: 2, method: "quantile" },
    });
    chart.render();
    const topo_snapshot = series.accessibility_snapshot(0, 512);
    const topo_hits = new Set();
    for (let y = 0; y < 320; y += 4) {
      for (let x = 0; x < 420; x += 4) {
        const hit = chart.layout_hit_test(0, x, y);
        if (hit !== null) topo_hits.add(hit.id);
      }
    }
    series.set_data(map_data);
    chart.render();
    const state = JSON.stringify(chart.export_state());
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:relative;width:420px;height:320px";
    document.body.append(restored_host);
    const restored = await api.create_chart(restored_host, options);
    restored.resize(420, 320, 1);
    restored.import_state(state);
    restored.render();
    const restored_state = JSON.stringify(restored.export_state());
    let same_hits = true;
    for (let y = 0; y < 320; y += 6) {
      for (let x = 0; x < 420; x += 6) {
        if (chart.layout_hit_test(0, x, y)?.id !== restored.layout_hit_test(0, x, y)?.id) same_hits = false;
      }
    }
    const kinds = restored.layout_series().map((handle) => handle.kind);
    chart.remove();
    restored.remove();
    host.remove();
    restored_host.remove();
    return {
      unknown_value, duplicate_id, bad_classes, flat_polygon, both_sources, wrong_data, wrong_kind,
      count_after_errors, replace_error, kept,
      topo_ids: topo_snapshot.items.map((item) => item.id),
      topo_values: topo_snapshot.items.map((item) => item.value),
      topo_hits: [...topo_hits].sort(),
      same: state === restored_state, same_hits, kinds,
    };
  }, { map_data, topology });
  expect(result).toEqual({
    unknown_value: "invalid_data",
    duplicate_id: "invalid_data",
    bad_classes: "invalid_data",
    flat_polygon: "invalid_options",
    both_sources: "invalid_options",
    wrong_data: "invalid_data",
    wrong_kind: "invalid_data",
    count_after_errors: 0,
    replace_error: "invalid_data",
    kept: feature_ids.length,
    topo_ids: ["a", "b"],
    topo_values: [1, 2],
    topo_hits: ["a", "b"],
    same: true,
    same_hits: true,
    kinds: ["choropleth"],
  });
});

for (const backend of ["canvas2d", "webgpu"]) test(`bubble and flow maps share the base projection and pan and zoom under the mouse through ${backend}`, async ({ page }) => {
  await blank_page(page);
  const setup = await page.evaluate(async ({ backend, map_data, points, links, mark_color }) => {
    const api = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    Object.assign(host.style, { width: "420px", height: "320px", position: "fixed", left: "0px", top: "0px", zIndex: "9999" });
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
    const base = chart.add_layout_series("choropleth", { pane: 0, title: "Growth" }, map_data);
    chart.render();
    const base_url = chart.take_screenshot().toDataURL();
    // Small radii keep the bubbles apart, so each hit region is a whole symbol.
    const bubbles = chart.add_layout_series("bubble_map", { pane: 0, title: "Desks", color: mark_color },
      { type: "geo_points", points, radius: [3, 10] });
    const flows = chart.add_layout_series("flow_map", { pane: 0, title: "Orders" },
      { type: "geo_links", links, arrows: true });
    chart.render();
    const marks_url = chart.take_screenshot().toDataURL();
    const left = chart.panes()[0].get_geometry().left;
    // Flows end at bubble anchors and draw above them, so each scan shows one mark series.
    const scan = (shown = bubbles, hidden = flows) => {
      hidden.set_visible(false);
      const regions = {};
      for (let y = 0; y < 320; y += 1) {
        for (let x = 0; x < 420 - left; x += 1) {
          const hit = chart.layout_hit_test(0, x, y);
          if (hit === null || hit.element === "feature") continue;
          const region = (regions[`${hit.element}:${hit.id}`] ??= { x: 0, y: 0, count: 0 });
          region.x += x;
          region.y += y;
          region.count += 1;
        }
      }
      hidden.set_visible(true);
      return Object.fromEntries(Object.entries(regions).map(([key, region]) =>
        [key, { x: region.x / region.count + left, y: region.y / region.count, count: region.count }]));
    };
    const before = scan();
    const east = before["bubble:east"];
    flows.set_visible(false);
    const feature_under_east = chart.layout_hit_test(0, east.x - left, east.y + 14)?.id ?? null;
    const hit = chart.layout_hit_test(0, east.x - left, east.y);
    flows.set_visible(true);
    window.__g13_marks = { chart, host, base, bubbles, flows, scan, left };
    return {
      backend: chart.backend(),
      marks_painted: marks_url !== base_url,
      before,
      feature_under_east,
      hit,
      flow_hit: Object.keys(scan(flows, bubbles)).sort(),
    };
  }, { backend, map_data, points, links, mark_color });
  expect(setup.backend).toBe(backend);
  expect(setup.marks_painted, "bubbles and flows paint over the base map").toBe(true);
  expect(Object.keys(setup.before).filter((key) => key.startsWith("bubble:")).sort())
    .toEqual(points.map((point) => `bubble:${point.id}`).sort());
  expect(setup.flow_hit).toEqual(links.map((link) => `link:${link.id}`).sort());
  expect(setup.hit).toMatchObject({ element: "bubble", id: "east", label: "East", value: 75, title: "Desks" });
  // The bubble anchored to the "east" feature sits inside that feature's projected shape.
  expect(setup.feature_under_east).toBe("east");
  // Square-root radii: the largest bubble covers more hit pixels than the smallest.
  expect(setup.before["bubble:central"].count).toBeGreaterThan(setup.before["bubble:north"].count);

  const east = setup.before["bubble:east"];
  const center = { x: 210, y: 160 };
  for (let notch = 0; notch < 5; notch += 1) {
    await page.mouse.move(center.x, center.y);
    await page.mouse.wheel(0, -100);
  }
  await expect.poll(() => page.evaluate(() => window.__g13_marks.bubbles.options().map_view.zoom)).toBeGreaterThan(1);
  const zoomed = await page.evaluate(() => {
    const { chart, base, bubbles, flows, scan } = window.__g13_marks;
    chart.render();
    return {
      views: [base, bubbles, flows].map((series) => series.options().map_view),
      regions: scan(),
    };
  });
  const zoom = zoomed.views[0].zoom;
  expect(zoomed.views[1]).toEqual(zoomed.views[0]);
  expect(zoomed.views[2]).toEqual(zoomed.views[0]);
  // Zoom anchors at the pointer: every mark moves away from it by the zoom factor.
  const scaled = zoomed.regions["bubble:east"];
  expect(Math.abs(scaled.x - (center.x + zoom * (east.x - center.x)))).toBeLessThan(1.5);
  expect(Math.abs(scaled.y - (center.y + zoom * (east.y - center.y)))).toBeLessThan(1.5);

  const grab = { x: 150, y: 250 };
  await page.mouse.move(grab.x, grab.y);
  await page.mouse.down();
  await page.mouse.move(grab.x + 20, grab.y - 15, { steps: 4 });
  await page.mouse.move(grab.x + 40, grab.y - 30, { steps: 4 });
  await page.mouse.up();
  const panned = await page.evaluate(() => {
    const { chart, bubbles, scan } = window.__g13_marks;
    chart.render();
    return { view: bubbles.options().map_view, regions: scan(), selected: chart.layout_selected_hit() };
  });
  expect(panned.view.zoom).toBe(zoom);
  expect(panned.selected, "a drag is not a click").toBeNull();
  for (const key of ["bubble:east", "bubble:west"]) {
    if (zoomed.regions[key] === undefined || panned.regions[key] === undefined) continue;
    expect(Math.abs(panned.regions[key].x - zoomed.regions[key].x - 40)).toBeLessThan(1.5);
    expect(Math.abs(panned.regions[key].y - zoomed.regions[key].y + 30)).toBeLessThan(1.5);
  }
  expect(panned.regions["bubble:east"], "the dragged bubble stays on the map").toBeDefined();

  const restored = await page.evaluate(async () => {
    const { chart, host, scan } = window.__g13_marks;
    const api = await import("/dist/aeris_charts_financial.js");
    const state = JSON.stringify(chart.export_state());
    const own = scan();
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:relative;width:420px;height:320px";
    document.body.append(restored_host);
    const next = await api.create_chart(restored_host, {
      backend: "canvas2d", autoSize: false, theme: "light",
      initialPane: { horizontal_domain: { type: "layout" } },
    });
    next.resize(420, 320, 1);
    next.import_state(state);
    next.render();
    const same_state = JSON.stringify(next.export_state()) === state;
    const left = next.panes()[0].get_geometry().left;
    next.layout_series()[2].set_visible(false);
    let same = true;
    for (const [key, region] of Object.entries(own)) {
      const hit = next.layout_hit_test(0, region.x - left, region.y);
      if (hit === null || `${hit.element}:${hit.id}` !== key) same = false;
    }
    const result = {
      same_state,
      same_hits: same,
      views: next.layout_series().map((series) => series.options().map_view),
    };
    next.remove();
    restored_host.remove();
    chart.remove();
    host.remove();
    delete window.__g13_marks;
    return result;
  });
  expect(restored.same_state).toBe(true);
  expect(restored.same_hits).toBe(true);
  for (const view of restored.views) expect(view).toEqual(panned.view);
});
