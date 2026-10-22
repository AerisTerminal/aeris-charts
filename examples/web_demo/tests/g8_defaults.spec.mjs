import { test, expect } from "@playwright/test";
import { PNG } from "pngjs";

test("general-only horizontal bars retain zero, top-first categories and a full-height pane", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:480px;height:300px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "continuous", scale: "linear" } },
    });
    chart.resize(480, 300, 1);
    const pane = chart.panes()[0];
    chart.add_axis({ id: "amount", pane: 0, dimension: "x", scale: "linear" });
    chart.add_axis({ id: "name", pane: 0, dimension: "y", scale: "band" });
    const bars = chart.add_series("horizontal_bar", {
      pane: 0, x_axis_id: "amount", y_axis_id: "name", color: "#d02020",
    });
    bars.set_data([
      { id: "first", x: "First", y: 20 },
      { id: "second", x: "Second", y: 40 },
    ]);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const geometry = pane.get_geometry();
    const hit_bounds = new Map();
    for (let y = geometry.top + 1; y < geometry.top + geometry.height - 1; y += 2) {
      for (let x = 0; x < geometry.width; x += 2) {
        const hit = chart.general_hit_test(0, x, y);
        if (!hit || (hit.row_id !== "first" && hit.row_id !== "second")) continue;
        const bounds = hit_bounds.get(hit.row_id) ?? { min_x: x, max_x: x, min_y: y, max_y: y };
        bounds.min_x = Math.min(bounds.min_x, x);
        bounds.max_x = Math.max(bounds.max_x, x);
        bounds.min_y = Math.min(bounds.min_y, y);
        bounds.max_y = Math.max(bounds.max_y, y);
        hit_bounds.set(hit.row_id, bounds);
      }
    }
    const pixels = chart.take_screenshot().getContext("2d").getImageData(0, 0, 480, 300).data;
    const red_at = (x, y) => {
      const offset = (Math.floor(y) * 480 + Math.floor(x)) * 4;
      return pixels[offset] > pixels[offset + 1] + 70 && pixels[offset] > pixels[offset + 2] + 70;
    };
    const marks = Object.fromEntries(Array.from(hit_bounds, ([id, bounds]) => {
      const x = Math.floor(geometry.left + (bounds.min_x + bounds.max_x) / 2);
      const y = Math.floor((bounds.min_y + bounds.max_y) / 2);
      return [id, { ...bounds, pixel: red_at(x, y) }];
    }));
    const outcome = { geometry, marks, panes: chart.panes().length, series: pane.get_series().length };
    chart.remove();
    host.remove();
    return outcome;
  });

  expect(result.panes).toBe(1);
  expect(result.series).toBe(1);
  expect(result.geometry.height).toBeGreaterThan(250);
  expect(result.marks.first.pixel).toBe(true);
  expect(result.marks.second.pixel).toBe(true);
  expect(result.marks.first.min_y).toBeLessThan(result.marks.second.min_y);
  expect(result.marks.first.min_x).toBe(result.marks.second.min_x);
  expect(result.marks.first.max_x).toBeLessThan(result.marks.second.max_x);
});

test("pinned Recharts 3.10.1 uses the same top-first horizontal-bar and zero-axis defaults", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=scales-axes");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const reference = page.locator('[data-reference-api="default horizontal bars"]');
  await expect(reference.locator("svg.recharts-surface")).toBeVisible();
  const bars = reference.locator(".recharts-bar-rectangle");
  await expect(bars).toHaveCount(2);
  const first = await bars.nth(0).boundingBox();
  const second = await bars.nth(1).boundingBox();
  expect(first).not.toBeNull();
  expect(second).not.toBeNull();
  expect(first.y).toBeLessThan(second.y);
  expect(first.x).toBe(second.x);
  expect(first.width).toBeLessThan(second.width);
  const zero_axis = await reference.locator(".recharts-yAxis .recharts-cartesian-axis-line").boundingBox();
  expect(zero_axis).not.toBeNull();
  expect(Math.abs(first.x - zero_axis.x)).toBeLessThanOrEqual(1);
});

test("pinned ResponsiveContainer follows resize and hidden-container reveal", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=responsive-layout");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const host = page.locator('[data-reference-api="ResponsiveContainer"] [data-resize-host]');
  const svg = host.locator('svg.recharts-surface[role="application"]');
  await expect.poll(async () => [
    Number(await svg.getAttribute("width")), Number(await svg.getAttribute("height")),
  ]).toEqual([700, 350]);
  await host.evaluate((node) => { node.style.width = "420px"; node.style.height = "210px"; });
  await expect.poll(async () => [
    Number(await svg.getAttribute("width")), Number(await svg.getAttribute("height")),
  ]).toEqual([420, 210]);
  await host.evaluate((node) => { node.style.display = "none"; });
  await page.evaluate(() => new Promise((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(resolve))));
  expect(await host.evaluate((node) => node.getBoundingClientRect().width)).toBe(0);
  await host.evaluate((node) => {
    node.style.width = "480px";
    node.style.height = "260px";
    node.style.display = "block";
  });
  await expect.poll(async () => [
    Number(await svg.getAttribute("width")), Number(await svg.getAttribute("height")),
  ]).toEqual([480, 260]);
  await expect(svg.locator(".recharts-line-curve").first()).toBeVisible();
});

test("pinned ResponsiveContainer aspect follows width without explicit host height", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=responsive-layout");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const host = page.locator('[data-reference-api="ResponsiveContainer aspect"] [data-aspect-host]');
  const svg = host.locator('svg.recharts-surface[role="application"]');
  await expect.poll(async () => [
    Number(await svg.getAttribute("width")), Number(await svg.getAttribute("height")),
  ]).toEqual([600, 300]);
  await host.evaluate((node) => { node.style.width = "360px"; });
  await expect.poll(async () => [
    Number(await svg.getAttribute("width")), Number(await svg.getAttribute("height")),
  ]).toEqual([360, 180]);
  await expect(svg.locator(".recharts-line-curve").first()).toBeVisible();
});

test("Aeris general-only auto size retains hits through resize and reveal", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:700px;height:350px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: true,
      initialPane: { horizontal_domain: { type: "category", scale: "point" } },
    });
    chart.add_axis({ id: "month", pane: 0, dimension: "x", scale: "point" });
    chart.add_axis({ id: "value", pane: 0, dimension: "y", scale: "linear" });
    const line = chart.add_series("xy_line", {
      pane: 0, x_axis_id: "month", y_axis_id: "value", point_markers: true, point_radius: 6,
    });
    line.set_data([
      { id: "jan", x: "Jan", y: 12 }, { id: "feb", x: "Feb", y: 18 },
      { id: "mar", x: "Mar", y: 14 }, { id: "apr", x: "Apr", y: 24 },
    ]);
    window.__g8_responsive = { chart, host };
  });
  const size = () => page.evaluate(() => {
    const { chart } = window.__g8_responsive;
    const image = chart.take_screenshot();
    return [Math.round(image.width / devicePixelRatio), Math.round(image.height / devicePixelRatio)];
  });
  await expect.poll(size).toEqual([700, 350]);
  await page.evaluate(() => {
    const { host } = window.__g8_responsive;
    host.style.width = "420px";
    host.style.height = "210px";
  });
  await expect.poll(size).toEqual([420, 210]);
  await page.evaluate(() => {
    const { host } = window.__g8_responsive;
    host.style.display = "none";
  });
  await page.evaluate(() => new Promise((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(resolve))));
  expect(await page.evaluate(() => window.__g8_responsive.host.getBoundingClientRect().width)).toBe(0);
  await page.evaluate(() => {
    const { host } = window.__g8_responsive;
    host.style.width = "480px";
    host.style.height = "260px";
    host.style.display = "block";
  });
  await expect.poll(size).toEqual([480, 260]);
  const result = await page.evaluate(() => {
    const { chart, host } = window.__g8_responsive;
    const geometry = chart.panes()[0].get_geometry();
    let hit = null;
    for (let y = geometry.top + 2; y < geometry.top + geometry.height - 2 && hit === null; y += 2) {
      for (let x = 0; x < geometry.width; x += 2) {
        hit = chart.general_hit_test(0, x, y);
        if (hit !== null) break;
      }
    }
    const result = { pane_count: chart.panes().length, auto_size_active: chart.auto_size_active(), hit };
    chart.remove();
    host.remove();
    delete window.__g8_responsive;
    return result;
  });
  expect(result.pane_count).toBe(1);
  expect(result.auto_size_active).toBe(true);
  expect(result.hit?.row_id).toMatch(/jan|feb|mar|apr/);
});

test("Aeris zero-size and constrained hosts recover to bounded general layout", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:0;height:0;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: true,
      initialPane: { horizontal_domain: { type: "category", scale: "point" } },
    });
    chart.add_axis({ id: "x", pane: 0, dimension: "x", scale: "point" });
    chart.add_axis({ id: "y", pane: 0, dimension: "y", scale: "linear" });
    const line = chart.add_series("xy_line", {
      pane: 0, x_axis_id: "x", y_axis_id: "y", point_markers: true, point_radius: 6,
    });
    line.set_data([{ id: "a", x: "A", y: 2 }, { id: "b", x: "B", y: 4 }]);
    window.__g8_small = { chart, host };
  });
  await page.evaluate(() => new Promise((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(resolve))));
  expect(await page.evaluate(() => window.__g8_small.host.getBoundingClientRect().width)).toBe(0);
  const size = () => page.evaluate(() => {
    const image = window.__g8_small.chart.take_screenshot();
    return [Math.round(image.width / devicePixelRatio), Math.round(image.height / devicePixelRatio)];
  });
  await page.evaluate(() => {
    const host = window.__g8_small.host;
    host.style.width = "96px";
    host.style.height = "64px";
  });
  await expect.poll(size).toEqual([96, 64]);
  const small = await page.evaluate(() => {
    const geometry = window.__g8_small.chart.panes()[0].get_geometry();
    return { top: geometry.top, height: geometry.height, width: geometry.width };
  });
  expect(Object.values(small).every((value) => Number.isFinite(value) && value >= 0)).toBe(true);
  await page.evaluate(() => {
    const host = window.__g8_small.host;
    host.style.width = "360px";
    host.style.height = "220px";
  });
  await expect.poll(size).toEqual([360, 220]);
  const result = await page.evaluate(() => {
    const { chart, host } = window.__g8_small;
    const geometry = chart.panes()[0].get_geometry();
    let hit = null;
    for (let y = geometry.top + 2; y < geometry.top + geometry.height - 2 && hit === null; y += 2) {
      for (let x = 0; x < geometry.width; x += 2) {
        hit = chart.general_hit_test(0, x, y);
        if (hit !== null) break;
      }
    }
    const output = { hit, panes: chart.panes().length, active: chart.auto_size_active() };
    chart.remove();
    host.remove();
    delete window.__g8_small;
    return output;
  });
  expect(["a", "b"]).toContain(result.hit?.row_id);
  expect(result.panes).toBe(1);
  expect(result.active).toBe(true);
  expect(errors).toEqual([]);
});

test.describe("device-scale general layout", () => {
  test.use({ deviceScaleFactor: 2 });

  test("auto-sized category marks retain exact hits after a device-scale resize", async ({ page, browserName }) => {
    await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
    const result = await page.evaluate(async () => {
      const { create_chart } = await import("/dist/aeris_charts_financial.js");
      const host = document.createElement("div");
      host.style.cssText = "position:fixed;left:0;top:0;width:260px;height:160px;z-index:10000";
      document.body.appendChild(host);
      const chart = await create_chart(host, {
        backend: "canvas2d", autoSize: true,
        initialPane: { horizontal_domain: { type: "category", scale: "point" } },
      });
      try {
        chart.add_axis({ id: "x", pane: 0, dimension: "x", scale: "point" });
        chart.add_axis({ id: "y", pane: 0, dimension: "y", scale: "linear" });
        const points = chart.add_series("xy_line", {
          pane: 0, x_axis_id: "x", y_axis_id: "y", point_markers: true, point_radius: 8,
        });
        points.set_data([{ id: "a", x: "A", y: 2 }, { id: "b", x: "B", y: 5 }]);
        await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        const before = chart.take_screenshot();
        host.style.width = "320px";
        host.style.height = "220px";
        await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        const after = chart.take_screenshot();
        const geometry = chart.panes()[0].get_geometry();
        let hit = null;
        for (let y = Math.ceil(geometry.top) + 2; y < geometry.top + geometry.height - 2 && hit === null; y += 2) {
          for (let x = 0; x < geometry.width; x += 2) {
            const candidate = chart.general_hit_test(0, x, y);
            if (candidate?.row_id === "b") { hit = candidate; break; }
          }
        }
        return {
          dpr: devicePixelRatio,
          before: [before.width, before.height],
          after: [after.width, after.height],
          hit: hit?.row_id ?? null,
        };
      } finally {
        chart.remove();
        host.remove();
      }
    });
    if (browserName === "firefox") {
      expect([1, 2]).toContain(result.dpr);
    } else {
      expect(result.dpr).toBe(2);
    }
    expect(result).toEqual({
      dpr: result.dpr,
      before: [260 * result.dpr, 160 * result.dpr],
      after: [320 * result.dpr, 220 * result.dpr],
      hit: "b",
    });
  });
});

test("auto-sized category frame follows a live display DPR change", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "live DPR emulation uses Chromium CDP");
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:260px;height:160px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: true,
      initialPane: { horizontal_domain: { type: "category", scale: "point" } },
    });
    chart.add_axis({ id: "x", pane: 0, dimension: "x", scale: "point" });
    chart.add_axis({ id: "y", pane: 0, dimension: "y", scale: "linear" });
    const line = chart.add_series("xy_line", {
      pane: 0, x_axis_id: "x", y_axis_id: "y", point_markers: true, point_radius: 7,
    });
    line.set_data([{ id: "a", x: "A", y: 2 }, { id: "b", x: "B", y: 5 }]);
    window.__g8_dpr = { chart, host };
  });
  const dimensions = () => page.evaluate(() => {
    const image = window.__g8_dpr.chart.take_screenshot();
    return { dpr: devicePixelRatio, width: image.width, height: image.height };
  });
  const original = await dimensions();
  const cdp = await page.context().newCDPSession(page);
  try {
    await cdp.send("Emulation.setDeviceMetricsOverride", {
      width: 1280, height: 720, deviceScaleFactor: 2, mobile: false,
    });
    // CDP changes devicePixelRatio without delivering a resolution media-query event here.
    // The chart also listens for the platform's orientation/fullscreen DPR signals.
    await page.evaluate(() => window.dispatchEvent(new Event("orientationchange")));
    await expect.poll(dimensions).toEqual({ dpr: 2, width: 520, height: 320 });
    await cdp.send("Emulation.setDeviceMetricsOverride", {
      width: 1280, height: 720, deviceScaleFactor: original.dpr, mobile: false,
    });
    await page.evaluate(() => window.dispatchEvent(new Event("orientationchange")));
    await expect.poll(dimensions).toEqual(original);
    const hit = await page.evaluate(() => {
      const chart = window.__g8_dpr.chart;
      const geometry = chart.panes()[0].get_geometry();
      for (let y = Math.ceil(geometry.top) + 2; y < geometry.top + geometry.height - 2; y += 2) {
        for (let x = 0; x < geometry.width; x += 2) {
          const candidate = chart.general_hit_test(0, x, y);
          if (candidate?.row_id === "b") return candidate.row_id;
        }
      }
      return null;
    });
    expect(hit).toBe("b");
  } finally {
    await cdp.send("Emulation.clearDeviceMetricsOverride");
    await cdp.detach();
    await page.evaluate(() => {
      window.__g8_dpr.chart.remove();
      window.__g8_dpr.host.remove();
      delete window.__g8_dpr;
    });
  }
});

test("pinned Recharts donut paints all four Cell colors in source order", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=polar");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const donut = page.locator('[data-reference-api="PieChart Pie"]');
  const sectors = donut.locator(".recharts-pie-sector path");
  await expect(sectors).toHaveCount(4);
  expect(await sectors.evaluateAll((nodes) => nodes.map((node) => node.getAttribute("fill")))).toEqual([
    "#4c8bf5", "#28b7a4", "#8b6df6", "#e4a11b",
  ]);
  expect(await sectors.evaluateAll((nodes) => nodes.every((node) => (node.getAttribute("d") ?? "").length > 30))).toBe(true);
  const lengths = await sectors.evaluateAll((nodes) => nodes.map((node) => node.getTotalLength()));
  expect(lengths[0]).toBeLessThan(lengths[2]);
  expect(lengths[2]).toBeLessThan(lengths[1]);
  expect(lengths[1]).toBeLessThan(lengths[3]);
});

test("Aeris donut keeps the pinned slice order, colors and exact hit proportions", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:350px;height:185px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "polar" } },
    });
    try {
      chart.resize(350, 185, 1);
      chart.add_axis({ id: "angle", pane: 0, dimension: "angle", scale: "angular_category", visible: false });
      chart.add_axis({ id: "radius", pane: 0, dimension: "radius", scale: "radial_linear", visible: false });
      const pie = chart.add_series("pie", {
        pane: 0, x_axis_id: "angle", y_axis_id: "radius",
        polar: { inner_radius: 0.4, outer_radius: 0.9 },
      });
      pie.set_data([
        { id: "jan", x: "Jan", y: 12, color: "#4c8bf5" },
        { id: "feb", x: "Feb", y: 18, color: "#28b7a4" },
        { id: "mar", x: "Mar", y: 14, color: "#8b6df6" },
        { id: "apr", x: "Apr", y: 24, color: "#e4a11b" },
      ]);
      const counts = { jan: 0, feb: 0, mar: 0, apr: 0 };
      for (let y = 0; y < 185; y += 2) {
        for (let x = 0; x < 350; x += 2) {
          const hit = chart.general_hit_test(0, x, y);
          if (hit?.row_id in counts) counts[hit.row_id]++;
        }
      }
      const state = chart.export_state();
      return {
        counts,
        rows: state.datasets[0].input.Category.y,
        colors: state.datasets[0].colors,
        image: chart.take_screenshot().toDataURL("image/png").length > 100,
      };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  expect(result.rows).toEqual([12, 18, 14, 24]);
  expect(result.colors).toEqual(["#4c8bf5ff", "#28b7a4ff", "#8b6df6ff", "#e4a11bff"]);
  expect(result.image).toBe(true);
  expect(Object.values(result.counts).every((count) => count > 100)).toBe(true);
  expect(result.counts.apr).toBeGreaterThan(result.counts.feb);
  expect(result.counts.feb).toBeGreaterThan(result.counts.mar);
  expect(result.counts.mar).toBeGreaterThan(result.counts.jan);
});

test("pinned Recharts radar and radial bars retain four category marks", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=polar");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const radar = page.locator('[data-reference-api="RadarChart Radar"]');
  const radial = page.locator('[data-reference-api="RadialBarChart RadialBar"]');
  const polygon = radar.locator(".recharts-radar-polygon path");
  await expect(polygon).toHaveCount(1);
  expect(await polygon.getAttribute("fill")).toBe("#4c8bf5");
  expect((await polygon.getAttribute("d"))?.length).toBeGreaterThan(30);
  await expect(radial.locator(".recharts-radial-bar-sector")).toHaveCount(4);
});

test("Aeris radar and radial bars keep the pinned four-category values and hits", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:350px;height:370px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "polar" } },
    });
    try {
      chart.resize(350, 370, 1);
      const values = [12, 18, 14, 24];
      const rows = values.map((y, index) => ({
        id: ["jan", "feb", "mar", "apr"][index],
        x: ["Jan", "Feb", "Mar", "Apr"][index], y,
      }));
      const addAxes = (pane, suffix) => {
        chart.add_axis({ id: `angle-${suffix}`, pane, dimension: "angle",
          scale: "angular_category", visible: false });
        chart.add_axis({ id: `radius-${suffix}`, pane, dimension: "radius",
          scale: "radial_linear", visible: false });
      };
      addAxes(0, "radar");
      const radar = chart.add_series("radar", {
        pane: 0, x_axis_id: "angle-radar", y_axis_id: "radius-radar",
        color: "#4c8bf5", fill_opacity: 0.35, point_markers: true,
      });
      radar.set_data(rows);
      const radialPane = chart.add_pane({ preserve_empty: true,
        horizontal_domain: { type: "polar" } });
      const radialIndex = radialPane.pane_index();
      addAxes(radialIndex, "bars");
      const bars = chart.add_series("radial_bar", {
        pane: radialIndex, x_axis_id: "angle-bars", y_axis_id: "radius-bars",
        color: "#28b7a4", polar: { inner_radius: 0.2, outer_radius: 0.9 },
      });
      bars.set_data(rows);
      const scan = (pane) => {
        const geometry = chart.panes()[pane].get_geometry();
        const ids = new Set();
        for (let y = Math.ceil(geometry.top); y < geometry.top + geometry.height; y += 2) {
          for (let x = 0; x < geometry.width; x += 2) {
            const hit = chart.general_hit_test(pane, x, y);
            if (hit?.row_id) ids.add(hit.row_id);
          }
        }
        return [...ids].sort();
      };
      const state = chart.export_state();
      return {
        radarHits: scan(0), radialHits: scan(radialIndex),
        values: state.datasets.map((dataset) => dataset.input.Category.y),
        kinds: state.series.map((series) => series.kind),
        image: chart.take_screenshot().toDataURL("image/png").length > 100,
      };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  expect(result.radarHits).toEqual(["apr", "feb", "jan", "mar"]);
  expect(result.radialHits).toEqual(result.radarHits);
  expect(result.values).toEqual([[12, 18, 14, 24], [12, 18, 14, 24]]);
  expect(result.kinds).toEqual(["Radar", "RadialBar"]);
  expect(result.image).toBe(true);
});

test("pinned Recharts hierarchy and flow panels expose their rendered items", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=hierarchy-flow");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const funnel = page.locator('[data-reference-api="FunnelChart Funnel"]');
  const treemap = page.locator('[data-reference-api="Treemap"]');
  const sankey = page.locator('[data-reference-api="Sankey"]');
  const sunburst = page.locator('[data-reference-api="SunburstChart"]');
  await expect(funnel.locator(".recharts-funnel-trapezoid")).toHaveCount(4);
  await expect(funnel.locator(".recharts-funnel-trapezoid path")).toHaveCount(4);
  const funnelAreas = await funnel.locator(".recharts-funnel-trapezoid path")
    .evaluateAll((nodes) => nodes.map((node) => {
      const box = node.getBBox();
      let area = 0;
      for (let y = box.y; y < box.y + box.height; y += 2) {
        for (let x = box.x; x < box.x + box.width; x += 2) {
          if (node.isPointInFill(new DOMPoint(x + 1, y + 1))) area++;
        }
      }
      return area;
    }));
  expect(funnelAreas).toHaveLength(4);
  const funnelTotal = funnelAreas.reduce((sum, area) => sum + area, 0);
  expect(funnelTotal).toBeGreaterThan(0);
  for (const [index, share] of [15, 16, 19, 12].entries()) {
    expect(Math.abs(funnelAreas[index] / funnelTotal - share / 62)).toBeLessThan(0.05);
  }
  await expect(treemap.locator(".recharts-treemap-depth-2 .recharts-rectangle")).toHaveCount(4);
  const leafAreas = await treemap.locator(".recharts-treemap-depth-2 .recharts-rectangle")
    .evaluateAll((nodes) => nodes.map((node) => {
      const box = node.getBBox();
      return box.width * box.height;
    }));
  expect(leafAreas[0]).toBeLessThan(leafAreas[2]);
  expect(leafAreas[2]).toBeLessThan(leafAreas[1]);
  expect(leafAreas[1]).toBeLessThan(leafAreas[3]);
  const leafTotal = leafAreas.reduce((sum, area) => sum + area, 0);
  for (const [index, value] of [12, 18, 14, 24].entries()) {
    expect(Math.abs(leafAreas[index] / leafTotal - value / 68)).toBeLessThan(0.05);
  }
  await expect(sankey.locator(".recharts-sankey-node")).toHaveCount(3);
  await expect(sankey.locator(".recharts-sankey-link")).toHaveCount(2);
  const linkWidths = await sankey.locator(".recharts-sankey-link")
    .evaluateAll((nodes) => nodes.map((node) => Number(node.getAttribute("stroke-width"))));
  expect(linkWidths[0]).toBeGreaterThan(linkWidths[1]);
  expect(linkWidths[0] / linkWidths[1]).toBeCloseTo(18 / 11, 2);
  await expect(sunburst.locator(".recharts-sunburst .recharts-sector")).toHaveCount(4);
  const sectorAreas = await sunburst.locator(".recharts-sunburst .recharts-sector")
    .evaluateAll((nodes) => nodes.map((node) => {
      const box = node.getBBox();
      let area = 0;
      for (let y = box.y; y < box.y + box.height; y += 2) {
        for (let x = box.x; x < box.x + box.width; x += 2) {
          if (node.isPointInFill(new DOMPoint(x + 1, y + 1))) area++;
        }
      }
      return area;
    }));
  expect(sectorAreas[0]).toBeLessThan(sectorAreas[2]);
  expect(sectorAreas[2]).toBeLessThan(sectorAreas[1]);
  expect(sectorAreas[1]).toBeLessThan(sectorAreas[3]);
  const sectorTotal = sectorAreas.reduce((sum, area) => sum + area, 0);
  for (const [index, value] of [12, 18, 14, 24].entries()) {
    expect(Math.abs(sectorAreas[index] / sectorTotal - value / 68)).toBeLessThan(0.05);
  }
});

test("Aeris funnel stage hit areas match the pinned trapezoid shares", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:350px;height:185px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "layout" } },
    });
    try {
      chart.resize(350, 185, 1);
      chart.add_layout_series("funnel", { pane: 0, color: "#8b6df6" }, {
        type: "funnel", stages: [
          { id: "jan", label: "Jan", value: 12 },
          { id: "feb", label: "Feb", value: 18 },
          { id: "mar", label: "Mar", value: 14 },
          { id: "apr", label: "Apr", value: 24 },
        ],
      });
      const counts = { jan: 0, feb: 0, mar: 0, apr: 0 };
      for (let y = 0; y < 185; y += 2) {
        for (let x = 0; x < 350; x += 2) {
          const hit = chart.layout_hit_test(0, x, y);
          if (hit?.id in counts) counts[hit.id]++;
        }
      }
      return { counts, image: chart.take_screenshot().toDataURL("image/png").length > 100 };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  expect(result.image).toBe(true);
  const hitTotal = Object.values(result.counts).reduce((sum, count) => sum + count, 0);
  expect(hitTotal).toBeGreaterThan(0);
  for (const [index, id] of ["jan", "feb", "mar", "apr"].entries()) {
    expect(Math.abs(result.counts[id] / hitTotal - [15, 16, 19, 12][index] / 62)).toBeLessThan(0.05);
  }
});
test("Aeris treemap hit regions rank with the pinned leaf areas", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:350px;height:185px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "layout" } },
    });
    try {
      chart.resize(350, 185, 1);
      chart.add_layout_series("treemap", { pane: 0, color: "#4c8bf5" }, {
        type: "hierarchy", nodes: [
          { id: "all", parent: null, label: "All", value: null },
          { id: "jan", parent: "all", label: "Jan", value: 12 },
          { id: "feb", parent: "all", label: "Feb", value: 18 },
          { id: "mar", parent: "all", label: "Mar", value: 14 },
          { id: "apr", parent: "all", label: "Apr", value: 24 },
        ],
      });
      const counts = { jan: 0, feb: 0, mar: 0, apr: 0 };
      for (let y = 0; y < 185; y += 3) {
        for (let x = 0; x < 350; x += 3) {
          const hit = chart.layout_hit_test(0, x, y);
          if (hit?.id in counts) counts[hit.id]++;
        }
      }
      return { counts, image: chart.take_screenshot().toDataURL("image/png").length > 100,
        paneCount: chart.panes().length };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  expect(result.paneCount).toBe(1);
  expect(result.image).toBe(true);
  expect(result.counts.jan).toBeGreaterThan(0);
  expect(result.counts.jan).toBeLessThan(result.counts.mar);
  expect(result.counts.mar).toBeLessThan(result.counts.feb);
  expect(result.counts.feb).toBeLessThan(result.counts.apr);
  const hitTotal = Object.values(result.counts).reduce((sum, count) => sum + count, 0);
  for (const [index, id] of ["jan", "feb", "mar", "apr"].entries()) {
    expect(Math.abs(result.counts[id] / hitTotal - [12, 18, 14, 24][index] / 68)).toBeLessThan(0.05);
  }
});
test("Aeris Sankey link hit widths rank with the pinned flow values", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:350px;height:185px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "layout" } },
    });
    try {
      chart.resize(350, 185, 1);
      const flow = chart.add_layout_series("sankey", { pane: 0, color: "#8b6df6" }, {
        type: "flow",
        nodes: [
          { id: "visit", label: "Visit" }, { id: "trial", label: "Trial" },
          { id: "paid", label: "Paid" },
        ],
        links: [
          { source: "visit", target: "trial", value: 18 },
          { source: "trial", target: "paid", value: 11 },
        ],
      });
      const counts = {};
      for (let y = 0; y < 185; y += 2) {
        for (let x = 0; x < 350; x += 2) {
          const hit = chart.layout_hit_test(0, x, y);
          if (hit?.element === "link") counts[hit.id] = (counts[hit.id] ?? 0) + 1;
        }
      }
      return { counts, links: flow.accessibility_snapshot(0, 10).items
        .filter((item) => item.element === "link").map((item) => item.value),
        image: chart.take_screenshot().toDataURL("image/png").length > 100 };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  expect(result.links).toEqual([18, 11]);
  expect(result.image).toBe(true);
  expect(Object.keys(result.counts)).toEqual(["visit", "trial"]);
  expect(result.counts.visit).toBeGreaterThan(0);
  expect(result.counts.visit).toBeGreaterThan(result.counts.trial);
});
test("Aeris sunburst hit sectors rank with the pinned value areas", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:350px;height:185px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "layout" } },
    });
    try {
      chart.resize(350, 185, 1);
      chart.add_layout_series("sunburst", { pane: 0, color: "#28b7a4" }, {
        type: "hierarchy", nodes: [
          { id: "all", parent: null, label: "All", value: null },
          { id: "jan", parent: "all", label: "Jan", value: 12 },
          { id: "feb", parent: "all", label: "Feb", value: 18 },
          { id: "mar", parent: "all", label: "Mar", value: 14 },
          { id: "apr", parent: "all", label: "Apr", value: 24 },
        ],
      });
      const counts = { jan: 0, feb: 0, mar: 0, apr: 0 };
      for (let y = 0; y < 185; y += 2) {
        for (let x = 0; x < 350; x += 2) {
          const hit = chart.layout_hit_test(0, x, y);
          if (hit?.id in counts) counts[hit.id]++;
        }
      }
      return { counts, image: chart.take_screenshot().toDataURL("image/png").length > 100 };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  expect(result.image).toBe(true);
  expect(result.counts.jan).toBeGreaterThan(0);
  expect(result.counts.jan).toBeLessThan(result.counts.mar);
  expect(result.counts.mar).toBeLessThan(result.counts.feb);
  expect(result.counts.feb).toBeLessThan(result.counts.apr);
  const hitTotal = Object.values(result.counts).reduce((sum, count) => sum + count, 0);
  for (const [index, id] of ["jan", "feb", "mar", "apr"].entries()) {
    expect(Math.abs(result.counts[id] / hitTotal - [12, 18, 14, 24][index] / 68)).toBeLessThan(0.05);
  }
});
test("pinned Recharts brush traveller narrows the visible category range", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=brush-selection-sync");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const brush = page.locator('[data-reference-api="Brush"] .recharts-brush');
  const travellers = brush.locator(".recharts-brush-traveller");
  await expect(travellers).toHaveCount(2);
  const initialSlide = await brush.locator(".recharts-brush-slide").boundingBox();
  const first = await travellers.first().boundingBox();
  expect(initialSlide?.width).toBeGreaterThan(100);
  expect(first).not.toBeNull();
  await page.mouse.move(first.x + first.width / 2, first.y + first.height / 2);
  await page.mouse.down();
  await page.mouse.move(first.x + first.width / 2 + 65, first.y + first.height / 2, { steps: 5 });
  await page.mouse.up();
  const narrowed = await brush.locator(".recharts-brush-slide").boundingBox();
  expect(narrowed?.width).toBeLessThan(initialSlide.width - 10);
  expect(narrowed?.x).toBeGreaterThan(initialSlide.x + 10);
});

test("pinned Recharts syncId promotes the same hovered category in both charts", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=brush-selection-sync");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const source = page.locator('[data-reference-api="Brush"]');
  const target = page.locator('[data-reference-api="syncId"]');
  const dot = source.locator(".recharts-line-dots circle").nth(1);
  await expect(dot).toBeVisible();
  await dot.hover();
  await expect(source.locator(".recharts-tooltip-cursor")).toHaveCount(1);
  await expect(target.locator(".recharts-tooltip-cursor")).toHaveCount(1);
  await expect(source.locator(".recharts-tooltip-label")).toHaveText("Feb");
  await expect(target.locator(".recharts-tooltip-label")).toHaveText("Feb");
});
test("pinned declarative React chart mounts axes, two lines, legend and tooltip", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=react-authoring");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const chart = page.locator('[data-reference-api="declarative chart axis series children"]');
  await expect(chart.locator(".recharts-xAxis")).toHaveCount(1);
  await expect(chart.locator(".recharts-yAxis")).toHaveCount(1);
  await expect(chart.locator(".recharts-line-curve")).toHaveCount(2);
  await expect(chart.locator(".recharts-legend-item")).toHaveCount(2);
  await chart.locator(".recharts-line-dots circle").first().hover();
  await expect(chart.locator(".recharts-tooltip-label")).toHaveText("Jan");
});
test("pinned Recharts line, area and scatter render the four source rows", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=paths-points");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const line = page.locator('[data-reference-api="Line"]');
  const area = page.locator('[data-reference-api="Area"]');
  const scatter = page.locator('[data-reference-api="Scatter"]');
  await expect(line.locator(".recharts-line-curve")).toHaveCount(2);
  await expect(area.locator(".recharts-area-area")).toHaveCount(1);
  await expect(area.locator(".recharts-area-curve")).toHaveCount(1);
  await expect(scatter.locator(".recharts-scatter-symbol")).toHaveCount(4);
  expect(await line.locator(".recharts-line-curve").first().getAttribute("stroke")).toBe("#4c8bf5");
  expect(await area.locator(".recharts-area-area").getAttribute("fill")).toBe("#b7eadf");
});

test("pinned square Scatter renders four equal-sided symbols", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=paths-points");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const chart = page.locator('[data-reference-api="Scatter square"]');
  const boxes = await chart.locator(".recharts-scatter-symbol path").evaluateAll((nodes) =>
    nodes.map((node) => {
      const box = node.getBBox();
      const screen = node.getBoundingClientRect();
      return { width: box.width, height: box.height, x: screen.x, y: screen.y };
    }));
  expect(boxes).toHaveLength(4);
  expect(boxes.every((box) => box.width > 0 && Math.abs(box.width - box.height) < 0.01)).toBe(true);
  expect(new Set(boxes.map((box) => `${box.x},${box.y}`)).size).toBe(4);
});
test("Aeris scatter square symbols change exported pixels and persist", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:350px;height:185px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "continuous", scale: "linear" } },
    });
    try {
      chart.resize(350, 185, 1);
      chart.add_axis({ id: "value", pane: 0, dimension: "x", scale: "linear", domain: [0, 30] });
      chart.add_axis({ id: "alternate", pane: 0, dimension: "y", scale: "linear", domain: [0, 25] });
      const scatter = chart.add_series("scatter", {
        pane: 0, x_axis_id: "value", y_axis_id: "alternate",
        color: "#8b6df6", point_radius: 8,
      });
      scatter.set_data([
        { id: "jan", x: 12, y: 8 }, { id: "feb", x: 18, y: 11 },
        { id: "mar", x: 14, y: 16 }, { id: "apr", x: 24, y: 19 },
      ]);
      const pixels = () => chart.take_screenshot().getContext("2d").getImageData(0, 0, 350, 185).data;
      const round = pixels();
      scatter.apply_options({ point_symbol: "square" });
      const square = pixels();
      let changed = 0;
      for (let i = 0; i < round.length; i += 4) {
        if (round[i] !== square[i] || round[i + 1] !== square[i + 1]
            || round[i + 2] !== square[i + 2]) changed++;
      }
      return { changed, symbol: scatter.options().point_symbol,
        stored: chart.export_state().series[0].point_symbol,
        rows: scatter.accessibility_snapshot(0, 10).items.map((item) => item.row_id) };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  expect(result.changed).toBeGreaterThan(40);
  expect(result.symbol).toBe("square");
  expect(result.stored).toBe("Square");
  expect(result.rows).toEqual(["jan", "feb", "mar", "apr"]);
});
test("pinned ErrorBar paints four capped Y whiskers", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=paths-points");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const chart = page.locator('[data-reference-api="Line ErrorBar"]');
  await expect(chart.locator(".recharts-line-curve")).toHaveCount(1);
  const whiskers = chart.locator(".recharts-errorBar");
  await expect(whiskers).toHaveCount(4);
  await expect(whiskers.locator("line")).toHaveCount(12);
  const spans = await whiskers.evaluateAll((nodes) => nodes.map((node) => {
    const lines = [...node.querySelectorAll("line")];
    return Math.max(...lines.map((line) => Number(line.getAttribute("y2"))))
      - Math.min(...lines.map((line) => Number(line.getAttribute("y1"))));
  }));
  expect(spans.every((span) => span > 0)).toBe(true);
});
test("Aeris error bar exports the pinned four Y intervals", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:350px;height:185px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "category", scale: "point" } },
    });
    try {
      chart.resize(350, 185, 1);
      chart.add_axis({ id: "month", pane: 0, dimension: "x", scale: "point" });
      chart.add_axis({ id: "value", pane: 0, dimension: "y", scale: "linear", domain: [0, 30] });
      const pixels = () => chart.take_screenshot().getContext("2d").getImageData(0, 0, 350, 185).data;
      const before = pixels();
      const errors = chart.add_series("error_bar", {
        pane: 0, x_axis_id: "month", y_axis_id: "value", color: "#e46f61",
      });
      errors.set_data([
        { id: "jan", x: "Jan", y: 12, y_low: 10, y_high: 14 },
        { id: "feb", x: "Feb", y: 18, y_low: 15, y_high: 21 },
        { id: "mar", x: "Mar", y: 14, y_low: 13, y_high: 15 },
        { id: "apr", x: "Apr", y: 24, y_low: 20, y_high: 28 },
      ]);
      const after = pixels();
      let changed = 0;
      for (let i = 0; i < before.length; i += 4) {
        if (before[i] !== after[i] || before[i + 1] !== after[i + 1]
            || before[i + 2] !== after[i + 2]) changed++;
      }
      return { changed, rows: errors.accessibility_snapshot(0, 10).items,
        kind: chart.export_state().series[0].kind };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  expect(result.changed).toBeGreaterThan(50);
  expect(result.rows).toMatchObject([
    { row_id: "jan", low: 10, high: 14 }, { row_id: "feb", low: 15, high: 21 },
    { row_id: "mar", low: 13, high: 15 }, { row_id: "apr", low: 20, high: 28 },
  ]);
  expect(result.kind).toBe("ErrorBar");
});
test("pinned ZAxis bubble symbols follow source size order", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=paths-points");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const chart = page.locator('[data-reference-api="Bubble ZAxis"]');
  const areas = await chart.locator(".recharts-scatter-symbol path").evaluateAll((nodes) =>
    nodes.map((node) => {
      const box = node.getBBox();
      return box.width * box.height;
    }));
  expect(areas).toHaveLength(4);
  expect(areas.every((area) => area > 0)).toBe(true);
  expect(areas[0]).toBeLessThan(areas[2]);
  expect(areas[2]).toBeLessThan(areas[1]);
  expect(areas[1]).toBeLessThan(areas[3]);
});
test("Aeris bubble pixels grow with the pinned size order", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:350px;height:185px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "continuous", scale: "linear" } },
    });
    try {
      chart.resize(350, 185, 1);
      chart.add_axis({ id: "value", pane: 0, dimension: "x", scale: "linear", domain: [0, 30] });
      chart.add_axis({ id: "alternate", pane: 0, dimension: "y", scale: "linear", domain: [0, 25] });
      const bubble = chart.add_series("bubble", {
        pane: 0, x_axis_id: "value", y_axis_id: "alternate", color: "#8b6df6",
      });
      const ink = () => {
        const data = chart.take_screenshot().getContext("2d").getImageData(0, 0, 350, 185).data;
        let count = 0;
        for (let i = 0; i < data.length; i += 4) {
          if (data[i] > 100 && data[i + 1] < 150 && data[i + 2] > 180
              && data[i + 2] > data[i] + 50) count++;
        }
        return count;
      };
      const sizeInk = [];
      for (const size of [90, 140, 110, 180]) {
        bubble.set_data([{ id: "center", x: 15, y: 12, size }]);
        sizeInk.push(ink());
      }
      bubble.set_data([
        { id: "jan", x: 12, y: 8, size: 90 }, { id: "feb", x: 18, y: 11, size: 140 },
        { id: "mar", x: 14, y: 16, size: 110 }, { id: "apr", x: 24, y: 19, size: 180 },
      ]);
      return { sizeInk, values: [0, 1, 2, 3].map((index) => bubble.data_at(index)?.size),
        kind: chart.export_state().series[0].kind, paneCount: chart.panes().length };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  expect(result.sizeInk.every((count) => count > 20)).toBe(true);
  expect(result.sizeInk[0]).toBeLessThan(result.sizeInk[2]);
  expect(result.sizeInk[2]).toBeLessThan(result.sizeInk[1]);
  expect(result.sizeInk[1]).toBeLessThan(result.sizeInk[3]);
  expect(result.values).toEqual([90, 140, 110, 180]);
  expect(result.kind).toBe("Bubble");
  expect(result.paneCount).toBe(1);
});
test("pinned range Area paints one band and two boundaries", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=paths-points");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const chart = page.locator('[data-reference-api="Area range"]');
  const band = chart.locator(".recharts-area-area");
  const boundary = chart.locator(".recharts-area-curve");
  await expect(band).toHaveCount(1);
  await expect(boundary).toHaveCount(2);
  expect((await band.getAttribute("d"))?.length).toBeGreaterThan(30);
  expect(await band.getAttribute("fill")).toBe("#b7eadf");
  expect(await boundary.evaluateAll((nodes) => nodes.map((node) => node.getAttribute("stroke"))))
    .toEqual(["#067a67", "#067a67"]);
});
test("Aeris range area keeps the pinned low and high rows in its shared frame", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:350px;height:185px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "category", scale: "point" } },
    });
    try {
      chart.resize(350, 185, 1);
      chart.add_axis({ id: "month", pane: 0, dimension: "x", scale: "point" });
      chart.add_axis({ id: "value", pane: 0, dimension: "y", scale: "linear", domain: [0, 24] });
      const pixels = () => chart.take_screenshot().getContext("2d").getImageData(0, 0, 350, 185).data;
      const before = pixels();
      const band = chart.add_series("range_area", {
        pane: 0, x_axis_id: "month", y_axis_id: "value", color: "#067a67",
      });
      band.set_data([
        { id: "jan", x: "Jan", low: 8, high: 12 },
        { id: "feb", x: "Feb", low: 11, high: 18 },
        { id: "mar", x: "Mar", low: 10, high: 14 },
        { id: "apr", x: "Apr", low: 19, high: 24 },
      ]);
      const after = pixels();
      let changed = 0;
      for (let i = 0; i < before.length; i += 4) {
        if (before[i] !== after[i] || before[i + 1] !== after[i + 1]
            || before[i + 2] !== after[i + 2]) changed++;
      }
      const state = chart.export_state();
      return { changed, rows: band.accessibility_snapshot(0, 10).items,
        kind: state.series[0].kind, paneCount: chart.panes().length };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  expect(result.changed).toBeGreaterThan(100);
  expect(result.rows).toMatchObject([
    { row_id: "jan", low: 8, high: 12 }, { row_id: "feb", low: 11, high: 18 },
    { row_id: "mar", low: 10, high: 14 }, { row_id: "apr", low: 19, high: 24 },
  ]);
  expect(result.kind).toBe("RangeArea");
  expect(result.paneCount).toBe(1);
});
test("pinned step lines preserve a gap or connect across it", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=paths-points");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const chart = page.locator('[data-reference-api="Line step gaps"]');
  const paths = await chart.locator(".recharts-line-curve").evaluateAll((nodes) =>
    nodes.map((node) => ({ d: node.getAttribute("d"), stroke: node.getAttribute("stroke") })));
  expect(paths.map((path) => path.stroke)).toEqual(["#e46f61", "#4c8bf5"]);
  expect(paths[0].d.match(/M/g)?.length).toBe(2);
  expect(paths[1].d.match(/M/g)?.length).toBe(1);
  expect(paths.every((path) => (path.d.match(/L/g)?.length ?? 0) >= 4)).toBe(true);
});
test("Aeris step line bridges only when connect_missing is enabled", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:350px;height:185px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "category", scale: "point" } },
    });
    try {
      chart.resize(350, 185, 1);
      chart.add_axis({ id: "month", pane: 0, dimension: "x", scale: "point" });
      chart.add_axis({ id: "value", pane: 0, dimension: "y", scale: "linear", domain: [0, 24] });
      const line = chart.add_series("xy_line", {
        pane: 0, x_axis_id: "month", y_axis_id: "value", color: "#4c8bf5",
        interpolation: "step_middle", connect_missing: false,
      });
      line.set_data([
        { id: "jan", x: "Jan", y: 12 }, { id: "feb", x: "Feb", y: 18 },
        { id: "mar", x: "Mar", y: null }, { id: "apr", x: "Apr", y: 14 },
        { id: "may", x: "May", y: 24 },
      ]);
      const pixels = () => chart.take_screenshot().getContext("2d").getImageData(0, 0, 350, 185).data;
      const disconnected = pixels();
      line.apply_options({ connect_missing: true });
      const connected = pixels();
      let changed = 0;
      for (let i = 0; i < disconnected.length; i += 4) {
        if (disconnected[i] !== connected[i] || disconnected[i + 1] !== connected[i + 1]
            || disconnected[i + 2] !== connected[i + 2]) changed++;
      }
      const state = chart.export_state();
      return { changed, options: line.options(), state: state.datasets[0].input.Category,
        series: state.series[0] };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  expect(result.changed).toBeGreaterThan(10);
  expect(result.options).toMatchObject({ interpolation: "step_middle", connect_missing: true });
  expect(result.series.connect_missing).toBe(true);
  expect(result.state.y_valid).toEqual([1, 1, 0, 1, 1]);
});
test("Aeris line, area and scatter keep the pinned rows in ordered panes", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:350px;height:370px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "category", scale: "point" } },
    });
    try {
      chart.resize(350, 370, 1);
      chart.add_axis({ id: "month", pane: 0, dimension: "x", scale: "point" });
      chart.add_axis({ id: "amount", pane: 0, dimension: "y", scale: "linear" });
      const rows = [12, 18, 14, 24].map((y, index) => ({
        id: ["jan", "feb", "mar", "apr"][index],
        x: ["Jan", "Feb", "Mar", "Apr"][index], y,
      }));
      const area = chart.add_series("xy_area", {
        pane: 0, x_axis_id: "month", y_axis_id: "amount", color: "#b7eadf",
      });
      area.set_data(rows);
      const line = chart.add_series("xy_line", {
        pane: 0, x_axis_id: "month", y_axis_id: "amount", color: "#4c8bf5",
        point_markers: true, point_radius: 6,
      });
      line.set_data(rows);
      const numeric = chart.add_pane({ preserve_empty: true,
        horizontal_domain: { type: "continuous", scale: "linear" } });
      const numericIndex = numeric.pane_index();
      chart.add_axis({ id: "value", pane: numericIndex, dimension: "x", scale: "linear" });
      chart.add_axis({ id: "alternate", pane: numericIndex, dimension: "y", scale: "linear" });
      const scatter = chart.add_series("scatter", {
        pane: numericIndex, x_axis_id: "value", y_axis_id: "alternate",
        color: "#8b6df6", point_radius: 8,
      });
      scatter.set_data([12, 18, 14, 24].map((x, index) => ({
        id: ["jan", "feb", "mar", "apr"][index], x,
        y: [8, 11, 16, 19][index],
      })));
      const hits = (pane) => {
        const geometry = chart.panes()[pane].get_geometry();
        const bySeries = new Map();
        for (let y = Math.ceil(geometry.top); y < geometry.top + geometry.height; y += 2) {
          for (let x = 0; x < geometry.width; x += 2) {
            const hit = chart.general_hit_test(pane, x, y);
            if (!hit) continue;
            if (!bySeries.has(hit.series)) bySeries.set(hit.series, new Set());
            bySeries.get(hit.series).add(hit.row_id);
          }
        }
        return Object.fromEntries([...bySeries].map(([series, ids]) => [series, [...ids].sort()]));
      };
      const state = chart.export_state();
      return {
        panes: chart.panes().length, area: area.id, line: line.id, scatter: scatter.id,
        categoryHits: hits(0), numericHits: hits(numericIndex),
        values: state.datasets.map((dataset) => dataset.input.Category?.y ?? dataset.input.Numeric?.y),
        image: chart.take_screenshot().toDataURL("image/png").length > 100,
      };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  const ids = ["apr", "feb", "jan", "mar"];
  expect(result.panes).toBe(2);
  expect(result.categoryHits[result.area]).toEqual(ids);
  expect(result.categoryHits[result.line]).toEqual(ids);
  expect(result.numericHits[result.scatter]).toEqual(ids);
  expect(result.values).toEqual([[12, 18, 14, 24], [12, 18, 14, 24], [8, 11, 16, 19]]);
  expect(result.image).toBe(true);
});

test("pinned Recharts ComposedChart orders area, bars and line over the same rows", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=standalone-composed");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const composed = page.locator('[data-reference-api="ComposedChart"]');
  await expect(composed.locator(".recharts-area-area")).toHaveCount(1);
  await expect(composed.locator(".recharts-bar-rectangle")).toHaveCount(4);
  await expect(composed.locator(".recharts-line-curve")).toHaveCount(1);
  const order = await composed.locator(".recharts-area, .recharts-bar, .recharts-line")
    .evaluateAll((nodes) => nodes.map((node) => node.getAttribute("class")?.match(/recharts-(area|bar|line)(?:\s|$)/)?.[1]));
  expect(order).toEqual(["area", "bar", "line"]);
});

test("Aeris composed area, bars and line share one ordered frame and exact hits", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:350px;height:185px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "category", scale: "band" } },
    });
    try {
      chart.resize(350, 185, 1);
      chart.add_axis({ id: "month", pane: 0, dimension: "x", scale: "band" });
      chart.add_axis({ id: "value", pane: 0, dimension: "y", scale: "linear" });
      const rows = (values) => values.map((y, index) => ({
        id: ["jan", "feb", "mar", "apr"][index],
        x: ["Jan", "Feb", "Mar", "Apr"][index], y,
      }));
      const area = chart.add_series("xy_area", {
        pane: 0, x_axis_id: "month", y_axis_id: "value", color: "#28b7a4",
      });
      area.set_data(rows([8, 11, 16, 19]));
      const bars = chart.add_series("column", {
        pane: 0, x_axis_id: "month", y_axis_id: "value", color: "#8b6df6",
      });
      bars.set_data(rows([12, 18, 14, 24]));
      const line = chart.add_series("xy_line", {
        pane: 0, x_axis_id: "month", y_axis_id: "value", color: "#315fc8",
        point_markers: true, point_radius: 5,
      });
      line.set_data(rows([12, 18, 14, 24]));
      const geometry = chart.panes()[0].get_geometry();
      const seen = new Set();
      for (let y = Math.ceil(geometry.top); y < geometry.top + geometry.height; y += 2) {
        for (let x = 0; x < geometry.width; x += 2) {
          const hit = chart.general_hit_test(0, x, y);
          if (hit) seen.add(hit.series);
        }
      }
      const state = chart.export_state();
      return {
        paneCount: chart.panes().length,
        kinds: chart.panes()[0].get_series().map((series) => series.kind),
        order: state.series.map((series) => series.kind),
        hitSeries: [area.id, bars.id, line.id].map((id) => seen.has(id)),
        values: state.datasets.map((dataset) => dataset.input.Category.y),
        image: chart.take_screenshot().toDataURL("image/png").length > 100,
      };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  expect(result.paneCount).toBe(1);
  expect(result.kinds).toEqual(["xy_area", "column", "xy_line"]);
  expect(result.order).toEqual(["XyArea", "Column", "XyLine"]);
  expect(result.hitSeries).toEqual([true, true, true]);
  expect(result.values).toEqual([[8, 11, 16, 19], [12, 18, 14, 24], [12, 18, 14, 24]]);
  expect(result.image).toBe(true);
});

test("pinned references stay behind area marks while labels and threshold line remain visible", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=references-labels");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const chart = page.locator('[data-reference-api="ReferenceLine ReferenceArea LabelList CartesianGrid"]');
  const area = chart.locator(".recharts-reference-area path");
  const threshold = chart.locator(".recharts-reference-line line");
  const labels = chart.locator(".recharts-label-list text");
  await expect(area).toHaveCount(1);
  await expect(threshold).toHaveCount(1);
  await expect(labels).toHaveCount(4);
  const rectangle = await area.evaluate((node) => Object.fromEntries(
    ["x", "y", "width", "height"].map((name) => [name, Number(node.getAttribute(name))]),
  ));
  const line = await threshold.evaluate((node) => Object.fromEntries(
    ["x1", "x2", "y1", "y2"].map((name) => [name, Number(node.getAttribute(name))]),
  ));
  expect(rectangle.width).toBeGreaterThan(0);
  expect(rectangle.height).toBeGreaterThan(0);
  expect(line.x2).toBeGreaterThan(line.x1);
  expect(line.y1).toBe(line.y2);
  expect(line.y1).toBeGreaterThan(rectangle.y);
  expect(line.y1).toBeLessThan(rectangle.y + rectangle.height);
  expect((await labels.allTextContents()).map(Number)).toEqual([12, 18, 14, 24]);
  const order = await chart.locator("svg.recharts-surface").evaluate((svg) => {
    const rect = svg.querySelector(".recharts-reference-area path");
    const mark = svg.querySelector(".recharts-area");
    const rule = svg.querySelector(".recharts-reference-line line");
    const position = (node) => Array.from(svg.querySelectorAll("*")).indexOf(node);
    return [position(rect), position(mark), position(rule)];
  });
  expect(order[0]).toBeLessThan(order[1]);
  expect(order[1]).toBeLessThan(order[2]);
});

test("Aeris general references and data labels change the exported shared frame", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:500px;height:350px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "category", scale: "band" } },
    });
    try {
      chart.resize(500, 350, 1);
      chart.add_axis({ id: "month", pane: 0, dimension: "x", scale: "band" });
      const valueAxis = chart.add_axis({ id: "value", pane: 0, dimension: "y", scale: "linear", domain: [0, 24] });
      const area = chart.add_series("xy_area", {
        pane: 0, x_axis_id: "month", y_axis_id: "value", color: "#067a67",
      });
      area.set_data([
        { id: "jan", x: "Jan", y: 12 }, { id: "feb", x: "Feb", y: 18 },
        { id: "mar", x: "Mar", y: 14 }, { id: "apr", x: "Apr", y: 24 },
      ]);
      const pixels = () => chart.take_screenshot().getContext("2d").getImageData(0, 0, 500, 350).data;
      const difference = (before, after) => {
        let changed = 0;
        for (let i = 0; i < before.length; i += 4) {
          if (before[i] !== after[i] || before[i + 1] !== after[i + 1]
              || before[i + 2] !== after[i + 2] || before[i + 3] !== after[i + 3]) changed++;
        }
        return changed;
      };
      const base = pixels();
      chart.add_general_reference({
        kind: "region", pane: 0, x_axis_id: "month", y_axis_id: "value",
        x_from: "Feb", x_to: "Mar",
        fill_color: "rgba(251,226,166,0.5)",
      });
      const withRegion = pixels();
      let regionRows = 0;
      for (let y = 0; y < 350; y++) {
        let changed = false;
        for (let x = 0; x < 500 && !changed; x++) {
          const i = (y * 500 + x) * 4;
          changed = base[i] !== withRegion[i] || base[i + 1] !== withRegion[i + 1]
            || base[i + 2] !== withRegion[i + 2] || base[i + 3] !== withRegion[i + 3];
        }
        if (changed) regionRows++;
      }
      valueAxis.zoom(1.4, 12);
      const withZoom = pixels();
      chart.resize(540, 380, 1);
      const resizedState = chart.export_state();
      const resizedReference = chart.general_references()[0].options();
      chart.resize(500, 350, 1);
      chart.add_general_reference({
        kind: "line", pane: 0, axis_id: "value", value: 16,
        color: "#e46f61", line_width: 3,
      });
      const withLine = pixels();
      area.apply_options({ data_labels: true });
      const withLabels = pixels();
      const geometry = chart.panes()[0].get_geometry();
      let hit = null;
      for (let y = geometry.top + 2; y < geometry.top + geometry.height - 2 && hit === null; y += 2) {
        for (let x = 0; x < geometry.width; x += 2) {
          const candidate = chart.general_hit_test(0, x, y);
          if (candidate?.row_id === "feb") { hit = candidate; break; }
        }
      }
      const references = chart.general_references().map((reference) => reference.options());
      const labels = area.options().data_labels;
      const exported = chart.export_state();
      const restoredHost = document.createElement("div");
      restoredHost.style.cssText = host.style.cssText;
      document.body.appendChild(restoredHost);
      const restoredChart = await create_chart(restoredHost, {
        backend: "canvas2d", autoSize: false,
        initialPane: { horizontal_domain: { type: "category", scale: "band" } },
      });
      let restoredRegionDifference;
      let restoredReference;
      try {
        restoredChart.resize(500, 350, 1);
        restoredChart.import_state(exported);
        const restoredRegion = restoredChart.general_references()[0];
        restoredReference = restoredRegion.options();
        const withRestoredRegion = restoredChart.take_screenshot().getContext("2d").getImageData(0, 0, 500, 350).data;
        restoredRegion.remove();
        const withoutRestoredRegion = restoredChart.take_screenshot().getContext("2d").getImageData(0, 0, 500, 350).data;
        restoredRegionDifference = difference(withRestoredRegion, withoutRestoredRegion);
      } finally {
        restoredChart.remove();
        restoredHost.remove();
      }
      return {
        regionDifference: difference(base, withRegion),
        regionRows,
        zoomRegionDifference: difference(withRegion, withZoom),
        resizedReference,
        stateReference: resizedState.references?.[0],
        restoredRegionDifference,
        restoredReference,
        lineDifference: difference(withZoom, withLine),
        labelDifference: difference(withLine, withLabels),
        references,
        labels,
        paneCount: chart.panes().length,
        hit,
      };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  expect(result.regionDifference).toBeGreaterThan(100);
  expect(result.regionRows).toBeGreaterThan(180);
  expect(result.zoomRegionDifference).toBeGreaterThan(100);
  expect(result.resizedReference.y_from).toBeUndefined();
  expect(result.resizedReference.y_to).toBeUndefined();
  expect(result.stateReference?.y_from).toBeUndefined();
  expect(result.stateReference?.y_to).toBeUndefined();
  expect(result.restoredReference.y_from).toBeUndefined();
  expect(result.restoredReference.y_to).toBeUndefined();
  expect(result.restoredRegionDifference).toBeGreaterThan(100);
  expect(result.lineDifference).toBeGreaterThan(50);
  expect(result.labelDifference).toBeGreaterThan(10);
  expect(result.references.map((reference) => reference.kind)).toEqual(["region", "line"]);
  expect(result.labels).toBe(true);
  expect(result.paneCount).toBe(1);
  expect(result.hit).toMatchObject({ row_id: "feb" });
});

test("pinned ReferenceArea optional bounds use plot edges", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=references-labels");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const chart = page.locator('[data-reference-api="ReferenceArea optional bounds"]');
  const rects = await chart.locator(".recharts-reference-area path").evaluateAll((nodes) => nodes.map((node) =>
    Object.fromEntries(["x", "y", "width", "height"].map((name) => [name, Number(node.getAttribute(name))]))));
  expect(rects).toHaveLength(2);
  expect(rects[0].width).toBeGreaterThan(rects[1].width);
  expect(rects[0].height).toBeLessThan(rects[1].height);
  expect(rects.every((rect) => rect.width > 0 && rect.height > 0)).toBe(true);
});

test("Aeris Y-only and one-sided reference regions follow plot edges", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:500px;height:350px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "category", scale: "band" } },
    });
    try {
      chart.resize(500, 350, 1);
      const x = chart.add_axis({ id: "month", pane: 0, dimension: "x", scale: "band" });
      chart.add_axis({ id: "value", pane: 0, dimension: "y", scale: "linear", domain: [0, 24] });
      const area = chart.add_series("xy_area", { pane: 0, x_axis_id: "month", y_axis_id: "value" });
      area.set_data([
        { id: "jan", x: "Jan", y: 12 }, { id: "feb", x: "Feb", y: 18 },
        { id: "mar", x: "Mar", y: 14 }, { id: "apr", x: "Apr", y: 24 },
      ]);
      const pixels = () => chart.take_screenshot().getContext("2d").getImageData(0, 0, 500, 350).data;
      const changed = (a, b) => {
        let count = 0;
        for (let i = 0; i < a.length; i += 4) {
          if (a[i] !== b[i] || a[i + 1] !== b[i + 1] || a[i + 2] !== b[i + 2]) count++;
        }
        return count;
      };
      const base = pixels();
      chart.add_general_reference({ kind: "region", pane: 0, x_axis_id: "month", y_axis_id: "value",
        y_from: 8, y_to: 16, fill_color: "rgba(15,180,120,0.5)" });
      const horizontal = pixels();
      chart.add_general_reference({ kind: "region", pane: 0, x_axis_id: "month", y_axis_id: "value",
        x_from: "Feb", fill_color: "rgba(251,226,166,0.5)" });
      const oneSided = pixels();
      x.zoom(1.2, "Feb");
      const zoomed = pixels();
      let emptyRejection = null;
      try {
        chart.add_general_reference({ kind: "region", pane: 0, x_axis_id: "month", y_axis_id: "value" });
      } catch (error) {
        emptyRejection = error.code;
      }
      const state = chart.export_state();
      return { emptyRejection, horizontalDifference: changed(base, horizontal), oneSidedDifference: changed(horizontal, oneSided),
        zoomDifference: changed(oneSided, zoomed), references: chart.general_references().map((ref) => ref.options()),
        stored: state.references };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  expect(result.emptyRejection).toBe("invalid_options");
  expect(result.horizontalDifference).toBeGreaterThan(100);
  expect(result.oneSidedDifference).toBeGreaterThan(100);
  expect(result.zoomDifference).toBeGreaterThan(100);
  expect(result.references[0]).toMatchObject({ kind: "region", y_from: 8, y_to: 16 });
  expect(result.references[0].x_from).toBeUndefined();
  expect(result.references[1]).toMatchObject({ kind: "region", x_from: "Feb" });
  expect(result.references[1].x_to).toBeUndefined();
  expect(result.stored).toHaveLength(2);
  expect(result.stored[0].x_from).toBeUndefined();
  expect(result.stored[0].x_to).toBeUndefined();
  expect(result.stored[1].x_to).toBeUndefined();
});
test("pinned Cell colors and top bar corners survive Recharts composition", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=customization");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const marks = page.locator('[data-reference-api="Cell formatter style props"] .recharts-bar-rectangle path');
  await expect(marks).toHaveCount(4);
  const styles = await marks.evaluateAll((nodes) => nodes.map((node) => ({
    fill: node.getAttribute("fill"), path: node.getAttribute("d"),
    width: Number(node.getAttribute("width")), height: Number(node.getAttribute("height")),
  })));
  expect(styles.map((style) => style.fill)).toEqual(["#4c8bf5", "#28b7a4", "#8b6df6", "#e4a11b"]);
  expect(styles.every((style) => style.width > 0 && style.height > 0)).toBe(true);
  expect(styles.every((style) => style.path?.includes("A 6,6"))).toBe(true);
});

test("Aeris row colors and exposed bar corners survive public frame export", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:350px;height:185px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "category", scale: "band" } },
    });
    try {
      chart.resize(350, 185, 1);
      chart.add_axis({ id: "month", pane: 0, dimension: "x", scale: "band" });
      chart.add_axis({ id: "value", pane: 0, dimension: "y", scale: "linear" });
      const bars = chart.add_series("column", {
        pane: 0, x_axis_id: "month", y_axis_id: "value", bar_corner_radius: 6,
      });
      const names = ["Jan", "Feb", "Mar", "Apr"];
      const colors = ["#4c8bf5", "#28b7a4", "#8b6df6", "#e4a11b"];
      bars.set_data([12, 18, 14, 24].map((y, i) => ({ id: names[i], x: names[i], y,
        color: colors[i] })));
      const geometry = chart.panes()[0].get_geometry();
      const bounds = new Map();
      for (let y = Math.floor(geometry.top) + 1; y < geometry.top + geometry.height - 1; y++) {
        for (let x = 0; x < geometry.width; x++) {
          const hit = chart.general_hit_test(0, x, y);
          if (hit?.series !== bars.id) continue;
          const mark = bounds.get(hit.row_id) ?? { min_x: x, max_x: x, min_y: y, max_y: y };
          mark.min_x = Math.min(mark.min_x, x);
          mark.max_x = Math.max(mark.max_x, x);
          mark.min_y = Math.min(mark.min_y, y);
          mark.max_y = Math.max(mark.max_y, y);
          bounds.set(hit.row_id, mark);
        }
      }
      const pixels = chart.take_screenshot().getContext("2d").getImageData(0, 0, 350, 185).data;
      const colorAt = (x, y) => {
        const offset = (Math.round(y) * 350 + Math.round(geometry.left + x)) * 4;
        return Array.from(pixels.slice(offset, offset + 3));
      };
      const marks = names.map((name) => {
        const mark = bounds.get(name);
        if (!mark) return null;
        const middle = Math.round((mark.min_x + mark.max_x) / 2);
        return { ...mark, center: colorAt(middle, (mark.min_y + mark.max_y) / 2),
          topMiddle: colorAt(middle, mark.min_y), corner: colorAt(mark.min_x, mark.min_y),
          bottomCorner: colorAt(mark.min_x, mark.max_y - 1) };
      });
      return { paneCount: chart.panes().length, colors: chart.export_state().datasets[0].colors,
        marks };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  expect(result.paneCount).toBe(1);
  expect(result.colors).toEqual(["#4c8bf5ff", "#28b7a4ff", "#8b6df6ff", "#e4a11bff"]);
  expect(result.marks.every(Boolean)).toBe(true);
  const expected = [[76, 139, 245], [40, 183, 164], [139, 109, 246], [228, 161, 27]];
  for (let i = 0; i < 4; i++) {
    expect(result.marks[i].center).toEqual(expected[i]);
    expect(result.marks[i].topMiddle).toEqual(expected[i]);
    expect(result.marks[i].corner).not.toEqual(expected[i]);
    expect(result.marks[i].bottomCorner).toEqual(expected[i]);
  }
});

test("pinned Recharts 3.10.1 stacks positive bars in series order", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=bars-stacks");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const reference = page.locator('[data-reference-api="Bar stackId"]');
  const groups = reference.locator(".recharts-bar");
  await expect(groups).toHaveCount(2);
  const first_marks = groups.nth(0).locator(".recharts-bar-rectangle path, .recharts-bar-rectangle rect");
  const second_marks = groups.nth(1).locator(".recharts-bar-rectangle path, .recharts-bar-rectangle rect");
  await expect(first_marks).toHaveCount(4);
  await expect(second_marks).toHaveCount(4);
  const first = await first_marks.all();
  const second = await second_marks.all();
  const read = (mark) => mark.evaluate((node) => Object.fromEntries(
    ["x", "y", "width", "height"].map((key) => [key, Number(node.getAttribute(key))]),
  ));
  await expect.poll(async () => {
    const bases = await Promise.all(first.map(read));
    const tops = await Promise.all(second.map(read));
    if ([...bases, ...tops].some((mark) => mark.height <= 2)) return Infinity;
    return Math.max(...bases.map((base, index) => Math.abs(base.y - (tops[index].y + tops[index].height))));
  }).toBeLessThan(0.1);
  const bounds = [];
  for (let index = 0; index < 4; index++) {
    const base = await read(first[index]);
    const top = await read(second[index]);
    expect(base.height).toBeGreaterThan(0);
    expect(top.height).toBeGreaterThan(0);
    bounds.push({ base, top });
    expect(Math.abs(base.x - top.x)).toBeLessThan(1);
    expect(Math.abs(base.width - top.width)).toBeLessThan(1);
    expect(Math.abs(base.y - (top.y + top.height))).toBeLessThan(1);
  }
  expect(bounds.map(({ base }) => base.x)).toEqual([...bounds.map(({ base }) => base.x)].sort((a, b) => a - b));
  expect(bounds[1].base.height).toBeGreaterThan(bounds[0].base.height);
  expect(bounds[3].top.height).toBeGreaterThan(bounds[0].top.height);
});

test("Aeris general-only stacked columns share the pinned positive-stack geometry", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:350px;height:185px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "category", scale: "band" } },
    });
    chart.resize(350, 185, 1);
    chart.add_axis({ id: "month", pane: 0, dimension: "x", scale: "band" });
    chart.add_axis({ id: "amount", pane: 0, dimension: "y", scale: "linear" });
    const base = chart.add_series("column", {
      pane: 0, x_axis_id: "month", y_axis_id: "amount", stack_id: "s", color: "#4c8bf5",
    });
    const top = chart.add_series("column", {
      pane: 0, x_axis_id: "month", y_axis_id: "amount", stack_id: "s", color: "#28b7a4",
    });
    const names = ["Jan", "Feb", "Mar", "Apr"];
    base.set_data([12, 18, 14, 24].map((y, i) => ({ id: names[i], x: names[i], y })));
    top.set_data([8, 11, 16, 19].map((y, i) => ({ id: names[i], x: names[i], y })));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const geometry = chart.panes()[0].get_geometry();
    const marks = new Map();
    for (let y = Math.floor(geometry.top) + 1; y < geometry.top + geometry.height - 1; y++) {
      for (let x = 0; x < geometry.width; x++) {
        const hit = chart.general_hit_test(0, x, y);
        if (!hit || (hit.series !== base.id && hit.series !== top.id)) continue;
        const key = `${hit.series}:${hit.row_id}`;
        const bounds = marks.get(key) ?? { min_x: x, max_x: x, min_y: y, max_y: y };
        bounds.min_x = Math.min(bounds.min_x, x);
        bounds.max_x = Math.max(bounds.max_x, x);
        bounds.min_y = Math.min(bounds.min_y, y);
        bounds.max_y = Math.max(bounds.max_y, y);
        marks.set(key, bounds);
      }
    }
    const pixels = chart.take_screenshot().getContext("2d").getImageData(0, 0, 350, 185).data;
    const sampled = names.map((name) => [base.id, top.id].map((series) => {
      const bounds = marks.get(`${series}:${name}`);
      if (!bounds) return null;
      const x = Math.round(geometry.left + (bounds.min_x + bounds.max_x) / 2);
      const y = Math.round((bounds.min_y + bounds.max_y) / 2);
      return { ...bounds, color: Array.from(pixels.slice((y * 350 + x) * 4, (y * 350 + x) * 4 + 3)) };
    }));
    const output = { pane_count: chart.panes().length, series_count: chart.panes()[0].get_series().length, sampled };
    chart.remove();
    host.remove();
    return output;
  });
  expect(result.pane_count).toBe(1);
  expect(result.series_count).toBe(2);
  for (const [base, top] of result.sampled) {
    expect(base).not.toBeNull();
    expect(top).not.toBeNull();
    expect(Math.abs(base.min_x - top.min_x)).toBeLessThanOrEqual(1);
    expect(Math.abs(base.max_x - top.max_x)).toBeLessThanOrEqual(1);
    expect(Math.abs(base.min_y - top.max_y)).toBeLessThanOrEqual(2);
    expect(base.color[2]).toBeGreaterThan(base.color[0]);
    expect(top.color[1]).toBeGreaterThan(top.color[0]);
  }
  expect(result.sampled.map(([base]) => base.min_x)).toEqual(
    [...result.sampled.map(([base]) => base.min_x)].sort((a, b) => a - b),
  );
  expect(result.sampled[1][0].max_y - result.sampled[1][0].min_y).toBeGreaterThan(
    result.sampled[0][0].max_y - result.sampled[0][0].min_y,
  );
  expect(result.sampled[3][1].max_y - result.sampled[3][1].min_y).toBeGreaterThan(
    result.sampled[0][1].max_y - result.sampled[0][1].min_y,
  );
});

test("pinned Recharts signed stacks separate positive and negative values at zero", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=bars-stacks");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const reference = page.locator('[data-reference-api="Bar signed stackOffset"]');
  const groups = reference.locator(".recharts-bar");
  await expect(groups).toHaveCount(2);
  const base_marks = groups.nth(0).locator(".recharts-bar-rectangle path, .recharts-bar-rectangle rect");
  const top_marks = groups.nth(1).locator(".recharts-bar-rectangle path, .recharts-bar-rectangle rect");
  await expect(base_marks).toHaveCount(4);
  await expect(top_marks).toHaveCount(4);
  const read = (mark) => mark.evaluate((node) => {
    const x = Number(node.getAttribute("x"));
    const y = Number(node.getAttribute("y"));
    const width = Number(node.getAttribute("width"));
    const height = Number(node.getAttribute("height"));
    return { x, y: Math.min(y, y + height), width, height: Math.abs(height) };
  });
  const base = await base_marks.all();
  const top = await top_marks.all();
  await expect.poll(async () => {
    const b = await Promise.all(base.map(read));
    const t = await Promise.all(top.map(read));
    if ([...b, ...t].some((mark) => mark.height <= 2)) return Infinity;
    return Math.max(
      Math.abs(b[0].y + b[0].height - t[0].y),
      Math.abs(b[1].y - t[1].y - t[1].height),
      Math.abs(b[2].y + b[2].height - t[2].y),
      Math.abs(b[3].y - t[3].y - t[3].height),
    );
  }).toBeLessThan(0.1);
  const b = await Promise.all(base.map(read));
  const t = await Promise.all(top.map(read));
  for (let i = 0; i < 4; i++) {
    expect(b[i].x).toBe(t[i].x);
    expect(b[i].width).toBe(t[i].width);
  }
  const zero = b[0].y + b[0].height;
  expect(Math.abs(zero - t[0].y)).toBeLessThan(0.1);
  expect(Math.abs(zero - b[1].y)).toBeLessThan(0.1);
  expect(Math.abs(zero - t[1].y - t[1].height)).toBeLessThan(0.1);
  expect(Math.abs(zero - b[2].y)).toBeLessThan(0.1);
  expect(Math.abs(zero - b[3].y - b[3].height)).toBeLessThan(0.1);
  expect(b[2].y + b[2].height).toBeLessThan(t[2].y + t[2].height);
  expect(t[3].y).toBeLessThan(b[3].y);
});

test("Aeris normal stacks share the pinned mixed-sign boundaries", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:350px;height:260px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "category", scale: "band" } },
    });
    chart.resize(350, 260, 1);
    chart.add_axis({ id: "category", pane: 0, dimension: "x", scale: "band" });
    chart.add_axis({ id: "value", pane: 0, dimension: "y", scale: "linear" });
    const base = chart.add_series("column", {
      pane: 0, x_axis_id: "category", y_axis_id: "value", stack_id: "s",
      stack_mode: "normal", color: "#4c8bf5",
    });
    const top = chart.add_series("column", {
      pane: 0, x_axis_id: "category", y_axis_id: "value", stack_id: "s",
      stack_mode: "normal", color: "#28b7a4",
    });
    const names = ["A", "B", "C", "D"];
    base.set_data([12, -18, -14, 24].map((y, i) => ({ id: names[i], x: names[i], y })));
    top.set_data([-8, 11, -16, 19].map((y, i) => ({ id: names[i], x: names[i], y })));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const geometry = chart.panes()[0].get_geometry();
    const marks = new Map();
    for (let y = Math.floor(geometry.top) + 1; y < geometry.top + geometry.height - 1; y++) {
      for (let x = 0; x < geometry.width; x++) {
        const hit = chart.general_hit_test(0, x, y);
        if (!hit || (hit.series !== base.id && hit.series !== top.id)) continue;
        const key = `${hit.series}:${hit.row_id}`;
        const bounds = marks.get(key) ?? { min_x: x, max_x: x, min_y: y, max_y: y };
        bounds.min_x = Math.min(bounds.min_x, x);
        bounds.max_x = Math.max(bounds.max_x, x);
        bounds.min_y = Math.min(bounds.min_y, y);
        bounds.max_y = Math.max(bounds.max_y, y);
        marks.set(key, bounds);
      }
    }
    const pixels = chart.take_screenshot().getContext("2d").getImageData(0, 0, 350, 260).data;
    const sampled = names.map((name) => [base.id, top.id].map((series) => {
      const bounds = marks.get(`${series}:${name}`);
      if (!bounds) return null;
      const x = Math.round(geometry.left + (bounds.min_x + bounds.max_x) / 2);
      const y = Math.round((bounds.min_y + bounds.max_y) / 2);
      return { ...bounds, color: Array.from(pixels.slice((y * 350 + x) * 4, (y * 350 + x) * 4 + 3)) };
    }));
    const output = { pane_count: chart.panes().length, sampled };
    chart.remove();
    host.remove();
    return output;
  });
  expect(result.pane_count).toBe(1);
  for (const [base, top] of result.sampled) {
    expect(base).not.toBeNull();
    expect(top).not.toBeNull();
    expect(Math.abs(base.min_x - top.min_x)).toBeLessThanOrEqual(1);
    expect(Math.abs(base.max_x - top.max_x)).toBeLessThanOrEqual(1);
    expect(base.color[2]).toBeGreaterThan(base.color[0]);
    expect(top.color[1]).toBeGreaterThan(top.color[0]);
  }
  const [[a_base, a_top], [b_base, b_top], [c_base, c_top], [d_base, d_top]] = result.sampled;
  const zero = a_base.max_y;
  expect(Math.abs(zero - a_top.min_y)).toBeLessThanOrEqual(2);
  expect(Math.abs(zero - b_base.min_y)).toBeLessThanOrEqual(2);
  expect(Math.abs(zero - b_top.max_y)).toBeLessThanOrEqual(2);
  expect(Math.abs(zero - c_base.min_y)).toBeLessThanOrEqual(2);
  expect(Math.abs(c_base.max_y - c_top.min_y)).toBeLessThanOrEqual(2);
  expect(Math.abs(zero - d_base.max_y)).toBeLessThanOrEqual(2);
  expect(Math.abs(d_base.min_y - d_top.max_y)).toBeLessThanOrEqual(2);
});

test("pinned Recharts expand stacks normalize positive totals to one height", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=bars-stacks");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const reference = page.locator('[data-reference-api="Bar expanded stackOffset"]');
  const groups = reference.locator(".recharts-bar");
  await expect(groups).toHaveCount(2);
  const base_marks = groups.nth(0).locator(".recharts-bar-rectangle path, .recharts-bar-rectangle rect");
  const top_marks = groups.nth(1).locator(".recharts-bar-rectangle path, .recharts-bar-rectangle rect");
  await expect(base_marks).toHaveCount(4);
  await expect(top_marks).toHaveCount(4);
  const read = (mark) => mark.evaluate((node) => Object.fromEntries(
    ["x", "y", "width", "height"].map((key) => [key, Number(node.getAttribute(key))]),
  ));
  const base = await base_marks.all();
  const top = await top_marks.all();
  await expect.poll(async () => {
    const b = await Promise.all(base.map(read));
    const t = await Promise.all(top.map(read));
    if ([...b, ...t].some((mark) => mark.height <= 2)) return Infinity;
    return Math.max(...b.map((mark, i) => Math.abs(mark.y - t[i].y - t[i].height)));
  }).toBeLessThan(0.1);
  const b = await Promise.all(base.map(read));
  const t = await Promise.all(top.map(read));
  const fractions = [12 / 20, 18 / 29, 14 / 30, 24 / 43];
  for (let i = 0; i < 4; i++) {
    expect(Math.abs(t[i].y - t[0].y)).toBeLessThan(0.1);
    expect(Math.abs(b[i].y + b[i].height - b[0].y - b[0].height)).toBeLessThan(0.1);
    expect(Math.abs(b[i].height / (b[i].height + t[i].height) - fractions[i])).toBeLessThan(0.01);
    expect(b[i].x).toBe(t[i].x);
    expect(b[i].width).toBe(t[i].width);
  }
});

test("Aeris percent stacks share the pinned expanded proportions", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:350px;height:220px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "category", scale: "band" } },
    });
    chart.resize(350, 220, 1);
    chart.add_axis({ id: "category", pane: 0, dimension: "x", scale: "band" });
    chart.add_axis({ id: "share", pane: 0, dimension: "y", scale: "linear" });
    const base = chart.add_series("column", {
      pane: 0, x_axis_id: "category", y_axis_id: "share", stack_id: "s",
      stack_mode: "percent", color: "#4c8bf5",
    });
    const top = chart.add_series("column", {
      pane: 0, x_axis_id: "category", y_axis_id: "share", stack_id: "s",
      stack_mode: "percent", color: "#28b7a4",
    });
    const names = ["Jan", "Feb", "Mar", "Apr"];
    base.set_data([12, 18, 14, 24].map((y, i) => ({ id: names[i], x: names[i], y })));
    top.set_data([8, 11, 16, 19].map((y, i) => ({ id: names[i], x: names[i], y })));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const geometry = chart.panes()[0].get_geometry();
    const marks = new Map();
    for (let y = Math.floor(geometry.top) + 1; y < geometry.top + geometry.height - 1; y++) {
      for (let x = 0; x < geometry.width; x++) {
        const hit = chart.general_hit_test(0, x, y);
        if (!hit || (hit.series !== base.id && hit.series !== top.id)) continue;
        const key = `${hit.series}:${hit.row_id}`;
        const bounds = marks.get(key) ?? { min_x: x, max_x: x, min_y: y, max_y: y };
        bounds.min_x = Math.min(bounds.min_x, x);
        bounds.max_x = Math.max(bounds.max_x, x);
        bounds.min_y = Math.min(bounds.min_y, y);
        bounds.max_y = Math.max(bounds.max_y, y);
        marks.set(key, bounds);
      }
    }
    const pixels = chart.take_screenshot().getContext("2d").getImageData(0, 0, 350, 220).data;
    const sampled = names.map((name) => [base.id, top.id].map((series) => {
      const bounds = marks.get(`${series}:${name}`);
      if (!bounds) return null;
      const x = Math.round(geometry.left + (bounds.min_x + bounds.max_x) / 2);
      const y = Math.round((bounds.min_y + bounds.max_y) / 2);
      return { ...bounds, color: Array.from(pixels.slice((y * 350 + x) * 4, (y * 350 + x) * 4 + 3)) };
    }));
    const output = { pane_count: chart.panes().length, sampled };
    chart.remove();
    host.remove();
    return output;
  });
  expect(result.pane_count).toBe(1);
  const fractions = [12 / 20, 18 / 29, 14 / 30, 24 / 43];
  const first = result.sampled[0];
  for (let i = 0; i < 4; i++) {
    const [base, top] = result.sampled[i];
    expect(base).not.toBeNull();
    expect(top).not.toBeNull();
    expect(Math.abs(base.min_x - top.min_x)).toBeLessThanOrEqual(1);
    expect(Math.abs(base.max_x - top.max_x)).toBeLessThanOrEqual(1);
    expect(Math.abs(base.min_y - top.max_y)).toBeLessThanOrEqual(2);
    expect(Math.abs(base.max_y - first[0].max_y)).toBeLessThanOrEqual(1);
    expect(Math.abs(top.min_y - first[1].min_y)).toBeLessThanOrEqual(1);
    const base_height = base.max_y - base.min_y + 1;
    const total_height = base.max_y - top.min_y + 1;
    expect(Math.abs(base_height / total_height - fractions[i])).toBeLessThan(0.03);
    expect(base.color[2]).toBeGreaterThan(base.color[0]);
    expect(top.color[1]).toBeGreaterThan(top.color[0]);
  }
});

test("pinned Recharts positive offset suppresses negative bar segments", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=bars-stacks");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const reference = page.locator('[data-reference-api="Bar positive stackOffset"]');
  const groups = reference.locator(".recharts-bar");
  await expect(groups).toHaveCount(2);
  const base_marks = groups.nth(0).locator(".recharts-bar-rectangle path, .recharts-bar-rectangle rect");
  const top_marks = groups.nth(1).locator(".recharts-bar-rectangle path, .recharts-bar-rectangle rect");
  await expect(base_marks).toHaveCount(2);
  await expect(top_marks).toHaveCount(2);
  const read = (mark) => mark.evaluate((node) => ({
    name: node.getAttribute("name"),
    ...Object.fromEntries(["x", "y", "width", "height"].map((key) =>
      [key, Number(node.getAttribute(key))])),
  }));
  const base = await base_marks.all();
  const top = await top_marks.all();
  await expect.poll(async () => {
    const b = await Promise.all(base.map(read));
    const t = await Promise.all(top.map(read));
    return Math.min(b[0].height, t[0].height, b[1].height, t[1].height);
  }).toBeGreaterThan(2);
  const b = await Promise.all(base.map(read));
  const t = await Promise.all(top.map(read));
  expect(b.map((mark) => mark.name)).toEqual(["A", "D"]);
  expect(t.map((mark) => mark.name)).toEqual(["B", "D"]);
  expect(Math.abs(b[1].y - t[1].y - t[1].height)).toBeLessThan(0.1);
  expect(Math.abs(b[0].y + b[0].height - t[0].y - t[0].height)).toBeLessThan(0.1);
});

test("Aeris positive stacks keep only the pinned visible segments", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:350px;height:220px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "category", scale: "band" } },
    });
    chart.resize(350, 220, 1);
    chart.add_axis({ id: "category", pane: 0, dimension: "x", scale: "band" });
    chart.add_axis({ id: "value", pane: 0, dimension: "y", scale: "linear" });
    const base = chart.add_series("column", {
      pane: 0, x_axis_id: "category", y_axis_id: "value", stack_id: "s",
      stack_mode: "positive", color: "#4c8bf5",
    });
    const top = chart.add_series("column", {
      pane: 0, x_axis_id: "category", y_axis_id: "value", stack_id: "s",
      stack_mode: "positive", color: "#28b7a4",
    });
    const names = ["A", "B", "C", "D"];
    base.set_data([12, -18, -14, 24].map((y, i) => ({ id: names[i], x: names[i], y })));
    top.set_data([-8, 11, -16, 19].map((y, i) => ({ id: names[i], x: names[i], y })));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const geometry = chart.panes()[0].get_geometry();
    const marks = new Map();
    for (let y = Math.floor(geometry.top) + 1; y < geometry.top + geometry.height - 1; y++) {
      for (let x = 0; x < geometry.width; x++) {
        const hit = chart.general_hit_test(0, x, y);
        if (!hit || (hit.series !== base.id && hit.series !== top.id)) continue;
        const key = `${hit.series}:${hit.row_id}`;
        const bounds = marks.get(key) ?? { min_x: x, max_x: x, min_y: y, max_y: y };
        bounds.min_x = Math.min(bounds.min_x, x);
        bounds.max_x = Math.max(bounds.max_x, x);
        bounds.min_y = Math.min(bounds.min_y, y);
        bounds.max_y = Math.max(bounds.max_y, y);
        marks.set(key, bounds);
      }
    }
    const pixels = chart.take_screenshot().getContext("2d").getImageData(0, 0, 350, 220).data;
    const visible = [...marks].map(([key, bounds]) => {
      const x = Math.round(geometry.left + (bounds.min_x + bounds.max_x) / 2);
      const y = Math.round((bounds.min_y + bounds.max_y) / 2);
      return {
        kind: key.startsWith(`${base.id}:`) ? "base" : "top", row: key.split(":").at(-1),
        ...bounds, color: Array.from(pixels.slice((y * 350 + x) * 4, (y * 350 + x) * 4 + 3)),
      };
    });
    const output = {
      pane_count: chart.panes().length, visible,
      negative_values: [base.data_at(1)?.value, top.data_at(0)?.value],
    };
    chart.remove();
    host.remove();
    return output;
  });
  expect(result.pane_count).toBe(1);
  expect(result.negative_values).toEqual([-18, -8]);
  expect(result.visible.map(({ kind, row }) => `${kind}:${row}`).sort()).toEqual([
    "base:A", "base:D", "top:B", "top:D",
  ]);
  const get = (kind, row) => result.visible.find((mark) => mark.kind === kind && mark.row === row);
  expect(Math.abs(get("base", "D").min_y - get("top", "D").max_y)).toBeLessThanOrEqual(2);
  expect(Math.abs(get("base", "A").max_y - get("top", "B").max_y)).toBeLessThanOrEqual(1);
  for (const mark of result.visible) {
    if (mark.kind === "base") expect(mark.color[2]).toBeGreaterThan(mark.color[0]);
    else expect(mark.color[1]).toBeGreaterThan(mark.color[0]);
  }
});

test("pinned cumulative stacks keep signed series order across zero", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=bars-stacks");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const reference = page.locator('[data-reference-api="Bar cumulative stackOffset"]');
  const groups = reference.locator(".recharts-bar");
  await expect(groups).toHaveCount(2);
  const read = (index) => groups.nth(index).locator(
    ".recharts-bar-rectangle path, .recharts-bar-rectangle rect",
  ).evaluateAll((nodes) => nodes.map((node) => {
    const y = Number(node.getAttribute("y"));
    const height = Number(node.getAttribute("height"));
    return { name: node.getAttribute("name"), top: Math.min(y, y + height),
      bottom: Math.max(y, y + height), x: Number(node.getAttribute("x")),
      width: Number(node.getAttribute("width")) };
  }));
  await expect.poll(async () => {
    const [base, top] = await Promise.all([read(0), read(1)]);
    return base.length === 4 && top.length === 4
      ? Math.min(...[...base, ...top].map((mark) => mark.bottom - mark.top)) : 0;
  }).toBeGreaterThan(2);
  const [base, top] = await Promise.all([read(0), read(1)]);
  expect(base.map((mark) => mark.name)).toEqual(["A", "B", "C", "D"]);
  expect(top.map((mark) => mark.name)).toEqual(["A", "B", "C", "D"]);
  for (let i = 0; i < 4; i++) {
    expect(base[i].x).toBe(top[i].x);
    expect(base[i].width).toBe(top[i].width);
  }
  expect(Math.abs(base[0].top - top[0].top)).toBeLessThan(0.1);
  expect(top[0].bottom).toBeGreaterThan(base[0].bottom);
  expect(Math.abs(base[1].bottom - top[1].bottom)).toBeLessThan(0.1);
  expect(top[1].top).toBeGreaterThan(base[1].top);
  for (const index of [2, 3]) {
    expect(Math.abs(base[index].top - top[index].bottom)).toBeLessThan(0.1);
  }
});

test("Aeris cumulative stacks retain the pinned signed overlap and visible order", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:350px;height:260px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "category", scale: "band" } },
    });
    try {
      chart.resize(350, 260, 1);
      chart.add_axis({ id: "category", pane: 0, dimension: "x", scale: "band" });
      chart.add_axis({ id: "value", pane: 0, dimension: "y", scale: "linear" });
      const base = chart.add_series("column", {
        pane: 0, x_axis_id: "category", y_axis_id: "value", stack_id: "s",
        stack_mode: "cumulative", color: "#4c8bf5",
      });
      const top = chart.add_series("column", {
        pane: 0, x_axis_id: "category", y_axis_id: "value", stack_id: "s",
        stack_mode: "cumulative", color: "#28b7a4",
      });
      const names = ["A", "B", "C", "D"];
      base.set_data([12, -18, 6, 10].map((y, i) => ({ id: names[i], x: names[i], y })));
      top.set_data([-18, 11, 4, 5].map((y, i) => ({ id: names[i], x: names[i], y })));
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      const geometry = chart.panes()[0].get_geometry();
      const marks = new Map();
      for (let y = Math.floor(geometry.top) + 1; y < geometry.top + geometry.height - 1; y++) {
        for (let x = 0; x < geometry.width; x++) {
          const hit = chart.general_hit_test(0, x, y);
          if (!hit || (hit.series !== base.id && hit.series !== top.id)) continue;
          const key = `${hit.series === base.id ? "base" : "top"}:${hit.row_id}`;
          const bounds = marks.get(key) ?? { min_x: x, max_x: x, min_y: y, max_y: y };
          bounds.min_x = Math.min(bounds.min_x, x);
          bounds.max_x = Math.max(bounds.max_x, x);
          bounds.min_y = Math.min(bounds.min_y, y);
          bounds.max_y = Math.max(bounds.max_y, y);
          marks.set(key, bounds);
        }
      }
      const pixels = chart.take_screenshot().getContext("2d").getImageData(0, 0, 350, 260).data;
      const visible = [...marks].map(([key, bounds]) => {
        const x = Math.round(geometry.left + (bounds.min_x + bounds.max_x) / 2);
        const y = Math.round((bounds.min_y + bounds.max_y) / 2);
        return { key, ...bounds,
          color: Array.from(pixels.slice((y * 350 + x) * 4, (y * 350 + x) * 4 + 3)) };
      });
      return { paneCount: chart.panes().length, visible,
        source: [base.data_at(0)?.value, top.data_at(0)?.value] };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  expect(result.paneCount).toBe(1);
  expect(result.source).toEqual([12, -18]);
  expect(result.visible.map(({ key }) => key).sort()).toEqual([
    "base:B", "base:C", "base:D", "top:A", "top:B", "top:C", "top:D",
  ]);
  const get = (key) => result.visible.find((mark) => mark.key === key);
  const zero = get("base:C").max_y;
  expect(get("top:A").max_y).toBeGreaterThan(zero);
  expect(get("base:B").min_y).toBeGreaterThanOrEqual(zero - 1);
  expect(get("top:B").min_y).toBeGreaterThan(zero);
  expect(Math.abs(get("top:B").min_y - get("base:B").max_y)).toBeLessThanOrEqual(1);
  expect(get("top:B").max_y).toBeGreaterThan(get("base:B").max_y);
  for (const mark of result.visible) {
    if (mark.key.startsWith("base:")) expect(mark.color[2]).toBeGreaterThan(mark.color[0]);
    else expect(mark.color[1]).toBeGreaterThan(mark.color[0]);
  }
});

test("Aeris silhouette and wiggle baselines track the pinned stack offsets", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=bars-stacks");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const modes = ["silhouette", "wiggle"];
  const reference = {};
  for (const mode of modes) {
    const groups = page.locator(`[data-reference-api="Bar ${mode} stackOffset"] .recharts-bar`);
    await expect(groups).toHaveCount(2);
    const read = () => groups.evaluateAll((elements) => elements.map((group) =>
      Array.from(group.querySelectorAll(".recharts-bar-rectangle path, .recharts-bar-rectangle rect"), (node) => {
        const y = Number(node.getAttribute("y"));
        const height = Number(node.getAttribute("height"));
        return { top: Math.min(y, y + height), bottom: Math.max(y, y + height),
          name: node.getAttribute("name") };
      })));
    await expect.poll(async () => {
      const rows = await read();
      return rows.length === 2 && rows.every((series) => series.length === 4)
        ? Math.min(...rows.flat().map((mark) => mark.bottom - mark.top)) : 0;
    }).toBeGreaterThan(2);
    reference[mode] = await read();
  }
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const aeris = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const output = {};
    for (const mode of ["silhouette", "wiggle"]) {
      const host = document.createElement("div");
      host.style.cssText = "position:fixed;left:0;top:0;width:350px;height:260px;z-index:10000;background:#fff";
      document.body.appendChild(host);
      const chart = await create_chart(host, {
        backend: "canvas2d", autoSize: false,
        initialPane: { horizontal_domain: { type: "category", scale: "band" } },
      });
      try {
        chart.resize(350, 260, 1);
        chart.add_axis({ id: "category", pane: 0, dimension: "x", scale: "band" });
        chart.add_axis({ id: "value", pane: 0, dimension: "y", scale: "linear" });
        const options = { pane: 0, x_axis_id: "category", y_axis_id: "value",
          stack_id: "s", stack_mode: mode };
        const base = chart.add_series("column", { ...options, color: "#4c8bf5" });
        const top = chart.add_series("column", { ...options, color: "#28b7a4" });
        const names = ["Jan", "Feb", "Mar", "Apr"];
        base.set_data([12, 18, 14, 24].map((y, i) => ({ id: names[i], x: names[i], y })));
        top.set_data([8, 11, 16, 19].map((y, i) => ({ id: names[i], x: names[i], y })));
        await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        const geometry = chart.panes()[0].get_geometry();
        const bounds = new Map();
        for (let y = Math.floor(geometry.top) + 1; y < geometry.top + geometry.height - 1; y++) {
          for (let x = 0; x < geometry.width; x++) {
            const hit = chart.general_hit_test(0, x, y);
            if (!hit || (hit.series !== base.id && hit.series !== top.id)) continue;
            const key = `${hit.series === base.id ? "base" : "top"}:${hit.row_id}`;
            const mark = bounds.get(key) ?? { top: y, bottom: y };
            mark.top = Math.min(mark.top, y);
            mark.bottom = Math.max(mark.bottom, y);
            bounds.set(key, mark);
          }
        }
        output[mode] = [base, top].map((_, index) => names.map((name) =>
          bounds.get(`${index === 0 ? "base" : "top"}:${name}`) ?? null));
      } finally {
        chart.remove();
        host.remove();
      }
    }
    return output;
  });
  const totals = [20, 29, 30, 43];
  const normalizedBaselines = (layers) => {
    const [base, top] = layers;
    const scale = (base[0].bottom - top[0].top) / totals[0];
    return base.map((mark) => (mark.bottom - base[0].bottom) / scale);
  };
  for (const mode of modes) {
    const [referenceBase, referenceTop] = reference[mode];
    const [aerisBase, aerisTop] = aeris[mode];
    expect(referenceBase.map((mark) => mark.name)).toEqual(["Jan", "Feb", "Mar", "Apr"]);
    expect(aerisBase.every(Boolean)).toBe(true);
    expect(aerisTop.every(Boolean)).toBe(true);
    for (let i = 0; i < 4; i++) {
      expect(Math.abs(referenceBase[i].top - referenceTop[i].bottom)).toBeLessThan(0.1);
      expect(Math.abs(aerisBase[i].top - aerisTop[i].bottom)).toBeLessThanOrEqual(2);
      const referenceShare = (referenceBase[i].bottom - referenceBase[i].top)
        / (referenceBase[i].bottom - referenceTop[i].top);
      const aerisShare = (aerisBase[i].bottom - aerisBase[i].top + 1)
        / (aerisBase[i].bottom - aerisTop[i].top + 1);
      expect(Math.abs(referenceShare - aerisShare)).toBeLessThan(0.03);
    }
    const expected = normalizedBaselines(reference[mode]);
    const actual = normalizedBaselines(aeris[mode]);
    for (let i = 0; i < 4; i++) {
      expect(Math.abs(actual[i] - expected[i]), `${mode} baseline ${i}`).toBeLessThan(0.5);
    }
    if (mode === "silhouette") {
      const centers = aerisBase.map((mark, i) => (mark.bottom + aerisTop[i].top) / 2);
      expect(Math.max(...centers) - Math.min(...centers)).toBeLessThanOrEqual(2);
    }
  }
});

test("pinned mixed-sign expand clips overshooting bars and omits top segments", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=bars-stacks");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const groups = page.locator('[data-reference-api="Bar mixed expanded stackOffset"] .recharts-bar');
  await expect(groups).toHaveCount(2);
  const read = () => groups.evaluateAll((elements) => elements.map((group) =>
    Array.from(group.querySelectorAll(".recharts-bar-rectangle path, .recharts-bar-rectangle rect"), (node) => {
      const y = Number(node.getAttribute("y"));
      const height = Number(node.getAttribute("height"));
      return { name: node.getAttribute("name"), top: Math.min(y, y + height),
        bottom: Math.max(y, y + height) };
    })));
  await expect.poll(async () => {
    const rows = await read();
    return rows.length === 2 && rows[0].length === 4 && rows[1].length === 2
      ? Math.min(...rows.flat().map((mark) => mark.bottom - mark.top)) : 0;
  }).toBeGreaterThan(2);
  const [base, top] = await read();
  expect(base.map((mark) => mark.name)).toEqual(["A", "B", "C", "D"]);
  expect(top.map((mark) => mark.name)).toEqual(["C", "D"]);
  expect(Math.abs(base[0].top - base[1].top)).toBeLessThan(0.1);
  expect(Math.abs(base[0].bottom - base[1].bottom)).toBeLessThan(0.1);
  expect(Math.abs(base[2].top - top[0].bottom)).toBeLessThan(0.1);
  expect(Math.abs(base[3].top - top[1].bottom)).toBeLessThan(0.1);
});

test("Aeris mixed-sign percent keeps every positive and negative segment visible", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:350px;height:260px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "category", scale: "band" } },
    });
    try {
      chart.resize(350, 260, 1);
      chart.add_axis({ id: "category", pane: 0, dimension: "x", scale: "band" });
      chart.add_axis({ id: "value", pane: 0, dimension: "y", scale: "linear" });
      const options = { pane: 0, x_axis_id: "category", y_axis_id: "value",
        stack_id: "s", stack_mode: "percent" };
      const base = chart.add_series("column", { ...options, color: "#4c8bf5" });
      const top = chart.add_series("column", { ...options, color: "#28b7a4" });
      const names = ["A", "B", "C", "D"];
      base.set_data([12, -18, -14, 24].map((y, i) => ({ id: names[i], x: names[i], y })));
      top.set_data([-8, 11, -16, 19].map((y, i) => ({ id: names[i], x: names[i], y })));
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      const geometry = chart.panes()[0].get_geometry();
      const marks = new Map();
      for (let y = Math.floor(geometry.top) + 1; y < geometry.top + geometry.height - 1; y++) {
        for (let x = 0; x < geometry.width; x++) {
          const hit = chart.general_hit_test(0, x, y);
          if (!hit || (hit.series !== base.id && hit.series !== top.id)) continue;
          const key = `${hit.series === base.id ? "base" : "top"}:${hit.row_id}`;
          const bounds = marks.get(key) ?? { min_x: x, max_x: x, min_y: y, max_y: y };
          bounds.min_x = Math.min(bounds.min_x, x);
          bounds.max_x = Math.max(bounds.max_x, x);
          bounds.min_y = Math.min(bounds.min_y, y);
          bounds.max_y = Math.max(bounds.max_y, y);
          marks.set(key, bounds);
        }
      }
      const pixels = chart.take_screenshot().getContext("2d").getImageData(0, 0, 350, 260).data;
      const visible = [...marks].map(([key, bounds]) => {
        const x = Math.round(geometry.left + (bounds.min_x + bounds.max_x) / 2);
        const y = Math.round((bounds.min_y + bounds.max_y) / 2);
        return { key, ...bounds,
          color: Array.from(pixels.slice((y * 350 + x) * 4, (y * 350 + x) * 4 + 3)) };
      });
      return { panes: chart.panes().length, visible,
        state: chart.export_state().series.filter((series) => series.stack_id === "s")
          .map((series) => series.stack_mode) };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  expect(result.panes).toBe(1);
  expect(result.state).toEqual(["Percent", "Percent"]);
  expect(result.visible.map(({ key }) => key).sort()).toEqual([
    "base:A", "base:B", "base:C", "base:D",
    "top:A", "top:B", "top:C", "top:D",
  ]);
  const get = (key) => result.visible.find((mark) => mark.key === key);
  const zero = get("base:A").max_y;
  expect(Math.abs(zero - get("top:A").min_y)).toBeLessThanOrEqual(2);
  expect(Math.abs(zero - get("base:B").min_y)).toBeLessThanOrEqual(2);
  expect(Math.abs(zero - get("top:B").max_y)).toBeLessThanOrEqual(2);
  expect(Math.abs(get("base:C").max_y - get("top:C").min_y)).toBeLessThanOrEqual(2);
  for (const mark of result.visible) {
    if (mark.key.startsWith("base:")) expect(mark.color[2]).toBeGreaterThan(mark.color[0]);
    else expect(mark.color[1]).toBeGreaterThan(mark.color[0]);
  }
});

test("shared WebGPU loss keeps financial and general charts alive on Canvas2D", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "the portable WebGPU adapter is Chromium-only");
  await page.goto("/?backend=auto&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "webgpu");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:480px;height:300px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "auto", autoSize: false,
      initialPane: { horizontal_domain: { type: "continuous", scale: "linear" } },
    });
    chart.resize(480, 300, 1);
    chart.add_axis({ id: "x", pane: 0, dimension: "x", scale: "linear" });
    chart.add_axis({ id: "y", pane: 0, dimension: "y", scale: "band" });
    const bars = chart.add_series("horizontal_bar", {
      pane: 0, x_axis_id: "x", y_axis_id: "y", color: "#d02020",
    });
    bars.set_data([{ id: "before", x: "Before", y: 20 }]);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const financial_image_before = window.__chart.take_screenshot().toDataURL("image/png");
    const before = {
      general_backend: chart.backend(),
      financial_backend: window.__chart.backend(),
      financial_rows: window.__main.data().length,
      financial_last: window.__main.data().at(-1),
      pane_count: chart.panes().length,
      state: chart.export_state(),
    };
    chart.wasm.simulate_device_loss_for_test();
    chart.render();
    window.__chart.render();
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    bars.set_data([{ id: "after", x: "After", y: 40 }]);
    const last = window.__main.data().at(-1);
    const close = last.close + 1;
    window.__main.update({
      ...last,
      high: Math.max(last.high, close),
      low: Math.min(last.low, close),
      close,
    });
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const geometry = chart.panes()[0].get_geometry();
    let hit = null;
    for (let y = geometry.top + 2; y < geometry.top + geometry.height - 2 && hit === null; y += 2) {
      for (let x = 0; x < geometry.width; x += 2) {
        hit = chart.general_hit_test(0, x, y);
        if (hit !== null) break;
      }
    }
    const pixels = chart.take_screenshot().getContext("2d").getImageData(0, 0, 480, 300).data;
    let red_pixels = 0;
    for (let i = 0; i < pixels.length; i += 4) {
      if (pixels[i] > pixels[i + 1] + 70 && pixels[i] > pixels[i + 2] + 70) red_pixels++;
    }
    const financial_image_after = window.__chart.take_screenshot().toDataURL("image/png");
    const after = {
      general_status: chart.backend_status(),
      financial_status: window.__chart.backend_status(),
      financial_rows: window.__main.data().length,
      financial_last: window.__main.data().at(-1),
      financial_export_length: financial_image_after.length,
      financial_export_changed: financial_image_after !== financial_image_before,
      pane_count: chart.panes().length,
      series_count: chart.panes()[0].get_series().length,
      hit,
      red_pixels,
      exported_state: chart.export_state(),
    };
    window.__g8_loss_chart = { chart, host };
    return { before, after };
  });
  expect(result.before).toMatchObject({
    general_backend: "webgpu",
    financial_backend: "webgpu",
    pane_count: 1,
  });
  expect(result.after.general_status).toMatchObject({
    active_backend: "canvas2d", reason: "device_lost",
  });
  expect(result.after.financial_status).toMatchObject({
    active_backend: "canvas2d", reason: "device_lost",
  });
  expect(result.after.pane_count).toBe(result.before.pane_count);
  expect(result.after.financial_rows).toBe(result.before.financial_rows);
  expect(result.after.financial_last.close).toBe(result.before.financial_last.close + 1);
  expect(result.after.financial_export_length).toBeGreaterThan(100);
  expect(result.after.financial_export_changed).toBe(true);
  expect(result.after.series_count).toBe(1);
  expect(result.after.hit).toMatchObject({ row_id: "after" });
  expect(result.after.red_pixels).toBeGreaterThan(100);
  expect(JSON.stringify(result.after.exported_state)).toContain("after");
  const presented = PNG.sync.read(await page.screenshot({ clip: { x: 0, y: 0, width: 480, height: 300 } }));
  let presented_red = 0;
  for (let i = 0; i < presented.data.length; i += 4) {
    if (presented.data[i] > presented.data[i + 1] + 70 &&
        presented.data[i] > presented.data[i + 2] + 70) presented_red++;
  }
  expect(presented_red).toBeGreaterThan(100);
  await page.evaluate(() => {
    window.__g8_loss_chart.chart.remove();
    window.__g8_loss_chart.host.remove();
    delete window.__g8_loss_chart;
  });
});

test("combined financial and general panes retain data and export after WebGPU loss", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "the portable WebGPU adapter is Chromium-only");
  await page.goto("/?backend=auto&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "webgpu");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:540px;height:420px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "auto", autoSize: false });
    chart.resize(540, 420, 1);
    const candles = chart.add_series("candlestick");
    const rows = Array.from({ length: 12 }, (_, i) => ({
      time: 1_700_000_000 + 60 * i, open: 100 + i, high: 103 + i,
      low: 99 + i, close: 102 + i,
    }));
    candles.set_data(rows);
    const general_pane = chart.add_pane({
      preserve_empty: true, horizontal_domain: { type: "continuous", scale: "linear" },
    });
    const general_index = general_pane.pane_index();
    chart.add_axis({ id: "combined-x", pane: general_index, dimension: "x", scale: "linear" });
    chart.add_axis({ id: "combined-y", pane: general_index, dimension: "y", scale: "linear" });
    const points = chart.add_series("scatter", {
      pane: general_index, x_axis_id: "combined-x", y_axis_id: "combined-y",
      color: "#d02020", point_radius: 9,
    });
    points.set_data([{ id: "before", x: 1, y: 2 }, { id: "keep", x: 2, y: 4 }]);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const before = {
      backend: chart.backend(), pane_count: chart.panes().length,
      general_index, candle_rows: candles.data().length,
      image: chart.take_screenshot().toDataURL("image/png"),
    };
    chart.wasm.simulate_device_loss_for_test();
    chart.render();
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const survivedImageEqual = chart.take_screenshot().toDataURL("image/png") === before.image;
    const replacement = { ...rows.at(-1), high: 120, close: 119 };
    candles.update(replacement);
    points.set_data([{ id: "after", x: 1, y: 6 }, { id: "keep", x: 2, y: 4 }]);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const geometry = chart.panes()[general_index].get_geometry();
    let hit = null;
    for (let y = geometry.top + 2; y < geometry.top + geometry.height - 2 && hit === null; y += 2) {
      for (let x = 0; x < geometry.width; x += 2) {
        const candidate = chart.general_hit_test(general_index, x, y);
        if (candidate?.row_id === "after") { hit = candidate; break; }
      }
    }
    const pixels = chart.take_screenshot().getContext("2d").getImageData(0, 0, 540, 420).data;
    let general_red = 0;
    for (let y = Math.floor(geometry.top); y < Math.ceil(geometry.top + geometry.height); y++) {
      for (let x = 0; x < 540; x++) {
        const offset = (y * 540 + x) * 4;
        if (pixels[offset] > pixels[offset + 1] + 70 && pixels[offset] > pixels[offset + 2] + 70) general_red++;
      }
    }
    const after = {
      status: chart.backend_status(), pane_count: chart.panes().length,
      general_index: general_pane.pane_index(),
      candle_rows: candles.data().length, last_candle: candles.data().at(-1),
      general_series: chart.panes()[general_index].get_series().length,
      state: chart.export_state(),
      survivedImageEqual,
      image_changed: chart.take_screenshot().toDataURL("image/png") !== before.image,
      general_red, hit,
    };
    chart.remove();
    after.remaining_canvases = host.querySelectorAll("canvas").length;
    host.remove();
    return { before: { ...before, image: undefined }, after };
  });
  expect(result.before).toMatchObject({ backend: "webgpu", pane_count: 2, general_index: 1, candle_rows: 12 });
  expect(result.after.status).toMatchObject({ active_backend: "canvas2d", reason: "device_lost" });
  expect(result.after.pane_count).toBe(2);
  expect(result.after.general_index).toBe(1);
  expect(result.after.candle_rows).toBe(12);
  expect(result.after.last_candle.close).toBe(119);
  expect(result.after.general_series).toBe(1);
  expect(result.after.survivedImageEqual).toBe(true);
  expect(result.after.image_changed).toBe(true);
  expect(result.after.general_red).toBeGreaterThan(100);
  expect(result.after.hit).toMatchObject({ row_id: "after" });
  expect(result.after.remaining_canvases).toBe(0);
  expect(JSON.stringify(result.after.state)).toContain("after");
});

test("polar and layout panes retain exact hits and export after WebGPU loss", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "the portable WebGPU adapter is Chromium-only");
  await page.goto("/?backend=auto&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "webgpu");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:500px;height:500px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "auto", autoSize: false,
      initialPane: { horizontal_domain: { type: "polar" } },
    });
    try {
      chart.resize(500, 500, 1);
      chart.add_axis({ id: "angle", pane: 0, dimension: "angle", scale: "angular_category", visible: false });
      chart.add_axis({ id: "radius", pane: 0, dimension: "radius", scale: "radial_linear", visible: false });
      const pie = chart.add_series("pie", {
        pane: 0, x_axis_id: "angle", y_axis_id: "radius", color: "#d02020",
      });
      pie.set_data([{ id: "before", x: "Before", y: 3 }, { id: "keep", x: "Keep", y: 2 }]);
      const layout = chart.add_pane({ preserve_empty: true, horizontal_domain: { type: "layout" } });
      const layoutIndex = layout.pane_index();
      const funnel = chart.add_layout_series("funnel", {
        pane: layoutIndex, color: "#2050d0",
      }, { type: "funnel", stages: [
        { id: "first", label: "First", value: 3 }, { id: "keep", label: "Keep", value: 2 },
      ] });
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      const before = { backend: chart.backend(), panes: chart.panes().length,
        image: chart.take_screenshot().toDataURL("image/png") };
      chart.wasm.simulate_device_loss_for_test();
      chart.render();
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      const survivedImageEqual = chart.take_screenshot().toDataURL("image/png") === before.image;
      pie.set_data([{ id: "after", x: "After", y: 5 }, { id: "keep", x: "Keep", y: 2 }]);
      funnel.set_data({ type: "funnel", stages: [
        { id: "second", label: "Second", value: 5 }, { id: "keep", label: "Keep", value: 2 },
      ] });
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      const findHit = (index, hitTest, wanted) => {
        const geometry = chart.panes()[index].get_geometry();
        for (let y = Math.ceil(geometry.top) + 2; y < geometry.top + geometry.height - 2; y += 3) {
          for (let x = 2; x < 498; x += 3) {
            const hit = hitTest(index, x, y);
            if (wanted(hit)) return hit;
          }
        }
        return null;
      };
      const polarHit = findHit(0, (pane, x, y) => chart.general_hit_test(pane, x, y),
        (hit) => hit?.row_id === "after");
      const layoutHit = findHit(layoutIndex, (pane, x, y) => chart.layout_hit_test(pane, x, y),
        (hit) => hit?.id === "second");
      const state = chart.export_state();
      return {
        before: { backend: before.backend, panes: before.panes },
        status: chart.backend_status(), panes: chart.panes().length,
        polarHit, layoutHit, polarState: JSON.stringify(state.datasets),
        layoutState: JSON.stringify(state.layout_datasets),
        survivedImageEqual,
        imageChanged: chart.take_screenshot().toDataURL("image/png") !== before.image,
      };
    } finally {
      chart.remove();
      if (host.querySelector("canvas")) throw new Error("fallback chart retained a canvas after remove");
      host.remove();
    }
  });
  expect(result.before).toEqual({ backend: "webgpu", panes: 2 });
  expect(result.status).toMatchObject({ active_backend: "canvas2d", reason: "device_lost" });
  expect(result.panes).toBe(2);
  expect(result.polarHit).toMatchObject({ row_id: "after" });
  expect(result.layoutHit).toMatchObject({ id: "second" });
  expect(result.polarState).toContain("after");
  expect(result.layoutState).toContain("second");
  expect(result.survivedImageEqual).toBe(true);
  expect(result.imageChanged).toBe(true);
});

test("box plots and heatmap cells retain exports and hits after WebGPU loss", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "the portable WebGPU adapter is Chromium-only");
  await page.goto("/?backend=auto&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "webgpu");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:480px;height:480px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "auto", autoSize: false,
      initialPane: { horizontal_domain: { type: "category", scale: "band" } },
    });
    try {
      chart.resize(480, 480, 1);
      chart.add_axis({ id: "box-x", pane: 0, dimension: "x", scale: "band" });
      chart.add_axis({ id: "box-y", pane: 0, dimension: "y", scale: "linear" });
      const boxes = chart.add_series("box_plot", {
        pane: 0, x_axis_id: "box-x", y_axis_id: "box-y", color: "#315fc8",
        box_fill_color: "#8b6df6", box_median_color: "#e4a11b",
      });
      boxes.set_data([{ id: "before-box", x: "North", min: 5, q1: 10,
        median: 15, q3: 20, max: 28 }]);
      const heatPane = chart.add_pane({ preserve_empty: true,
        horizontal_domain: { type: "category", scale: "band" } }).pane_index();
      chart.add_axis({ id: "heat-x", pane: heatPane, dimension: "x", scale: "band" });
      chart.add_axis({ id: "heat-y", pane: heatPane, dimension: "y", scale: "band" });
      const cells = chart.add_series("heatmap_grid", {
        pane: heatPane, x_axis_id: "heat-x", y_axis_id: "heat-y",
        heatmap_value_domain: [0, 100], heatmap_low_color: "#2050d0",
        heatmap_high_color: "#d02020",
      });
      cells.set_data([{ id: "before-cell", x: "Jan", y: "North", value: 25 }]);
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      const before = chart.take_screenshot().toDataURL("image/png");
      const initialBackend = chart.backend();
      chart.wasm.simulate_device_loss_for_test();
      chart.render();
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      const sameImage = chart.take_screenshot().toDataURL("image/png") === before;
      boxes.set_data([{ id: "after-box", x: "South", min: 12, q1: 18,
        median: 24, q3: 30, max: 40 }]);
      cells.set_data([{ id: "after-cell", x: "Feb", y: "South", value: 85 }]);
      const findHit = (pane, wanted) => {
        const geometry = chart.panes()[pane].get_geometry();
        for (let y = Math.ceil(geometry.top) + 2; y < geometry.top + geometry.height - 2; y += 3) {
          for (let x = 2; x < 478; x += 3) {
            const hit = chart.general_hit_test(pane, x, y);
            if (wanted(hit)) return hit;
          }
        }
        return null;
      };
      const boxHit = findHit(0, (hit) => hit?.row_id === "after-box")?.row_id ?? null;
      const heatHit = findHit(heatPane, (hit) => hit?.row_id === "after-cell")?.row_id ?? null;
      const state = JSON.stringify(chart.export_state().datasets);
      return {
        initialBackend, status: chart.backend_status(), sameImage,
        changedImage: chart.take_screenshot().toDataURL("image/png") !== before,
        boxHit, heatHit, state,
      };
    } finally {
      chart.remove();
      if (host.querySelector("canvas")) throw new Error("fallback chart retained a canvas after remove");
      host.remove();
    }
  });
  expect(result.initialBackend).toBe("webgpu");
  expect(result.status).toMatchObject({ active_backend: "canvas2d", reason: "device_lost" });
  expect(result.sameImage).toBe(true);
  expect(result.changedImage).toBe(true);
  expect(result.boxHit).toBe("after-box");
  expect(result.heatHit).toBe("after-cell");
  expect(result.state).toContain("after-box");
  expect(result.state).toContain("after-cell");
});

test("one financial Cartesian polar and layout chart restores its ordered frame", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const candlesData = Array.from({ length: 8 }, (_, index) => ({
      time: 1_700_000_000 + index * 60, open: 100 + index, high: 103 + index,
      low: 99 + index, close: 102 + index,
    }));
    const makeHost = () => {
      const host = document.createElement("div");
      host.style.cssText = "position:fixed;left:0;top:0;width:760px;height:760px;z-index:10000;background:#fff";
      document.body.appendChild(host);
      return host;
    };
    const firstHost = makeHost();
    const first = await create_chart(firstHost, { backend: "canvas2d", autoSize: false });
    let state;
    let beforeImage;
    let beforeHits;
    const findHit = (chart, paneIndex, lookup, wanted) => {
      const geometry = chart.panes()[paneIndex].get_geometry();
      for (let y = Math.ceil(geometry.top) + 2; y < geometry.top + geometry.height - 2; y += 3) {
        for (let x = 1; x < geometry.width - 1; x += 3) {
          const hit = lookup(chart, paneIndex, x, y);
          if (wanted(hit)) return hit;
        }
      }
      return null;
    };
    const hitSnapshot = (chart) => ({
      column: findHit(chart, 1, (target, pane, x, y) => target.general_hit_test(pane, x, y),
        (hit) => hit?.row_id === "feb")?.row_id ?? null,
      pie: findHit(chart, 2, (target, pane, x, y) => target.general_hit_test(pane, x, y),
        (hit) => hit?.row_id === "north")?.row_id ?? null,
      funnel: findHit(chart, 3, (target, pane, x, y) => target.layout_hit_test(pane, x, y),
        (hit) => hit?.id === "trial")?.id ?? null,
    });
    try {
      first.resize(760, 760, 1);
      const candles = first.add_series("candlestick");
      candles.set_data(candlesData);
      const cartesian = first.add_pane({ preserve_empty: true,
        horizontal_domain: { type: "category", scale: "band" } });
      const cartesianIndex = cartesian.pane_index();
      first.add_axis({ id: "month", pane: cartesianIndex, dimension: "x", scale: "band" });
      first.add_axis({ id: "revenue", pane: cartesianIndex, dimension: "y", scale: "linear" });
      const columns = first.add_series("column", {
        pane: cartesianIndex, x_axis_id: "month", y_axis_id: "revenue", color: "#2050d0",
      });
      columns.set_data([{ id: "jan", x: "Jan", y: 12 }, { id: "feb", x: "Feb", y: 18 }]);
      const polar = first.add_pane({ preserve_empty: true, horizontal_domain: { type: "polar" } });
      const polarIndex = polar.pane_index();
      first.add_axis({ id: "angle", pane: polarIndex, dimension: "angle",
        scale: "angular_category", visible: false });
      first.add_axis({ id: "radius", pane: polarIndex, dimension: "radius",
        scale: "radial_linear", visible: false });
      const pie = first.add_series("pie", { pane: polarIndex, x_axis_id: "angle",
        y_axis_id: "radius", polar: { inner_radius: 0.25, outer_radius: 0.9 } });
      pie.set_data([{ id: "north", x: "North", y: 3 }, { id: "south", x: "South", y: 2 }]);
      const layout = first.add_pane({ preserve_empty: true,
        horizontal_domain: { type: "layout" } });
      first.add_layout_series("funnel", { pane: layout.pane_index(), color: "#d05a20" },
        { type: "funnel", stages: [{ id: "visit", label: "Visit", value: 8 },
          { id: "trial", label: "Trial", value: 5 }] });
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      state = first.export_state();
      beforeImage = first.take_screenshot().toDataURL("image/png");
      beforeHits = hitSnapshot(first);
    } finally {
      first.remove();
      if (firstHost.querySelector("canvas")) throw new Error("source chart retained a canvas after remove");
      firstHost.remove();
    }
    const secondHost = makeHost();
    const second = await create_chart(secondHost, { backend: "canvas2d", autoSize: false });
    try {
      second.resize(760, 760, 1);
      second.import_state(state);
      second.panes()[0].get_series().find((series) => series.kind === "candlestick").set_data(candlesData);
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      const restored = second.export_state();
      const restoredHits = hitSnapshot(second);
      const restoredCanvas = second.take_screenshot();
      const restoredImage = restoredCanvas.toDataURL("image/png");
      const pixels = restoredCanvas.getContext("2d").getImageData(0, 0, 760, 760).data;
      const paintedByPane = second.panes().map((pane) => {
        const geometry = pane.get_geometry();
        let vivid = 0;
        for (let y = Math.max(0, Math.ceil(geometry.top)); y < Math.min(760, geometry.top + geometry.height); y++) {
          for (let x = 0; x < 760; x++) {
            const offset = (y * 760 + x) * 4;
            const high = Math.max(pixels[offset], pixels[offset + 1], pixels[offset + 2]);
            const low = Math.min(pixels[offset], pixels[offset + 1], pixels[offset + 2]);
            if (high > 100 && high - low > 50) vivid++;
          }
        }
        return vivid;
      });
      const financial = second.panes()[0].get_series().find((series) => series.kind === "candlestick");
      const column = second.panes()[1].get_series().find((series) => series.kind === "column");
      const polar = second.panes()[2].get_series().find((series) => series.kind === "pie");
      const layout = second.layout_series().find((series) => series.kind === "funnel");
      column.set_data([{ id: "jan", x: "Jan", y: 12 }, { id: "new", x: "Feb", y: 24 }]);
      const changedImage = second.take_screenshot().toDataURL("image/png");
      const newHit = findHit(second, 1,
        (chart, pane, x, y) => chart.general_hit_test(pane, x, y),
        (hit) => hit?.row_id === "new");
      return {
        stateEqual: JSON.stringify(restored) === JSON.stringify(state),
        paintedByPane,
        beforeHits, restoredHits,
        panes: second.panes().length,
        kinds: second.panes().map((pane) => pane.get_series().map((series) => series.kind)),
        financialRows: financial?.data().length,
        polarRows: polar?.accessibility_snapshot(0, 0).total_rows,
        layoutItems: layout?.accessibility_snapshot(0, 0).total,
        changedImage: changedImage !== restoredImage,
        newHit: newHit?.row_id ?? null,
      };
    } finally {
      second.remove();
      if (secondHost.querySelector("canvas")) throw new Error("restored chart retained a canvas after remove");
      secondHost.remove();
    }
  });
  expect(result.stateEqual).toBe(true);
  expect(result.paintedByPane).toHaveLength(4);
  for (const count of result.paintedByPane) expect(count).toBeGreaterThan(40);
  expect(result.beforeHits).toEqual({ column: "feb", pie: "north", funnel: "trial" });
  expect(result.restoredHits).toEqual(result.beforeHits);
  expect(result.panes).toBe(4);
  expect(result.kinds).toEqual([["candlestick"], ["column"], ["pie"], []]);
  expect(result.financialRows).toBe(8);
  expect(result.polarRows).toBe(2);
  expect(result.layoutItems).toBe(2);
  expect(result.changedImage).toBe(true);
  expect(result.newHit).toBe("new");
});

test("pinned Recharts export follows host state after a reordered update", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=persistence-export-recovery");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const update = page.getByRole("button", { name: "Update host state" });
  const serialize = page.getByRole("button", { name: "Serialize state for export" });
  const output = page.locator('[data-reference-api="DOM/SVG export"] output');
  const line = page.locator(".recharts-line-curve");
  await expect(line).toHaveCount(1);
  await serialize.click();
  const before = JSON.parse(await output.innerText());
  const beforePath = await line.getAttribute("d");
  await update.click();
  await serialize.click();
  const after = JSON.parse(await output.innerText());
  await expect.poll(() => line.getAttribute("d")).not.toBe(beforePath);
  expect(before.map((row) => row.name)).toEqual(["Jan", "Feb", "Mar", "Apr"]);
  expect(after.map((row) => row.name)).toEqual(["Apr", "Mar", "Feb", "Jan"]);
  expect(after.map((row) => row.value)).toEqual(before.map((row) => row.value).reverse());
});

test("pinned migration scene replaces a standalone line with composed bars and line", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=packaging-migration");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const source = page.locator('[data-reference-api="package-root-import"] code');
  await expect(source).toContainText('from "recharts"');
  await expect(page.locator(".recharts-line-curve")).toHaveCount(1);
  await expect(page.locator(".recharts-bar-rectangle")).toHaveCount(0);
  await page.getByRole("button", { name: "Migrate to composed chart" }).click();
  await expect(page.locator(".recharts-line-curve")).toHaveCount(1);
  await expect(page.locator(".recharts-bar-rectangle")).toHaveCount(4);
  await expect(page.locator(".recharts-wrapper")).toHaveCount(1);
});

test("pinned Recharts animated line reaches updated geometry after its 400 ms transition", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=animation");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const scene = page.locator('[data-reference-api="isAnimationActive animationDuration animationEasing"]');
  const animated = scene.locator(".recharts-line-curve").first();
  await expect(animated).toHaveAttribute("d", /M/);
  await page.waitForTimeout(500);
  const before = await animated.getAttribute("d");
  await scene.getByRole("button", { name: "Update animated line" }).click();
  await page.waitForTimeout(120);
  const middle = await animated.getAttribute("d");
  await page.waitForTimeout(430);
  const after = await animated.getAttribute("d");
  expect(middle).not.toBe(before);
  expect(middle).not.toBe(after);
  expect(after).not.toBe(before);
  await page.waitForTimeout(120);
  expect(await animated.getAttribute("d")).toBe(after);
  await expect(scene.locator(".recharts-line-curve")).toHaveCount(2);
});

test("pinned line animation remains observable with reduced-motion media enabled", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("/g1_recharts_reference.html?case=animation");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  expect(await page.evaluate(() => matchMedia("(prefers-reduced-motion: reduce)").matches)).toBe(true);
  const scene = page.locator('[data-reference-api="isAnimationActive animationDuration animationEasing"]');
  const animated = scene.locator(".recharts-line-curve").first();
  await expect(animated).toHaveAttribute("d", /M/);
  await page.waitForTimeout(500);
  const before = await animated.getAttribute("d");
  await scene.getByRole("button", { name: "Update animated line" }).click();
  await page.waitForTimeout(120);
  const middle = await animated.getAttribute("d");
  await page.waitForTimeout(430);
  const after = await animated.getAttribute("d");
  expect(middle).not.toBe(before);
  expect(middle).not.toBe(after);
});

test("Aeris four-row line paints an intermediate frame on the same 400 ms update", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:350px;height:185px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "category", scale: "point" } },
    });
    try {
      chart.resize(350, 185, 1);
      chart.add_axis({ id: "month", pane: 0, dimension: "x", scale: "point" });
      chart.add_axis({ id: "value", pane: 0, dimension: "y", scale: "linear", domain: [0, 40] });
      const line = chart.add_series("xy_line", {
        pane: 0, x_axis_id: "month", y_axis_id: "value", color: "#4c8bf5",
        point_markers: true, point_radius: 6,
      });
      const rows = (delta) => [12, 18, 14, 24].map((value, index) => ({
        id: ["jan", "feb", "mar", "apr"][index],
        x: ["Jan", "Feb", "Mar", "Apr"][index],
        y: value + delta[index],
      }));
      line.set_data(rows([0, 0, 0, 0]));
      const before = chart.take_screenshot().toDataURL("image/png");
      chart.set_general_transition_duration(400);
      line.set_data(rows([3, 7, 5, 9]));
      const started = performance.now();
      const active = chart.wasm.wants_animation();
      const initial = chart.take_screenshot().toDataURL("image/png");
      chart.wasm.set_animation_time(started + 160);
      const middle = chart.take_screenshot().toDataURL("image/png");
      chart.wasm.set_animation_time(started + 450);
      const after = chart.take_screenshot().toDataURL("image/png");
      const idle = !chart.wasm.wants_animation();
      const values = chart.export_state().datasets[0].input.Category.y;
      return { active, idle, sameStart: initial === before,
        changedMiddle: middle !== before, changedEnd: after !== middle, values };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  expect(result).toEqual({ active: true, idle: true, sameStart: true,
    changedMiddle: true, changedEnd: true, values: [15, 25, 19, 33] });
});

test("Aeris line applies the same update immediately under reduced motion", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:350px;height:185px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "category", scale: "point" } },
    });
    try {
      chart.resize(350, 185, 1);
      chart.add_axis({ id: "month", pane: 0, dimension: "x", scale: "point" });
      chart.add_axis({ id: "value", pane: 0, dimension: "y", scale: "linear", domain: [0, 40] });
      const line = chart.add_series("xy_line", {
        pane: 0, x_axis_id: "month", y_axis_id: "value", color: "#4c8bf5",
      });
      line.set_data([{ id: "jan", x: "Jan", y: 12 }, { id: "feb", x: "Feb", y: 18 }]);
      const before = chart.take_screenshot().toDataURL("image/png");
      chart.set_general_transition_duration(400);
      line.set_data([{ id: "jan", x: "Jan", y: 15 }, { id: "feb", x: "Feb", y: 25 }]);
      const after = chart.take_screenshot().toDataURL("image/png");
      return {
        media: matchMedia("(prefers-reduced-motion: reduce)").matches,
        changed: before !== after,
        idle: !chart.wasm.wants_animation(),
        values: chart.export_state().datasets[0].input.Category.y,
      };
    } finally {
      chart.remove(); host.remove();
    }
  });
  expect(result).toEqual({ media: true, changed: true, idle: true, values: [15, 25] });
});

test("engine legend accepts a near-edge touch while keeping mouse hits precise", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 900, height: 700 }, hasTouch: true });
  const page = await context.newPage();
  try {
    await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
    const target = await page.evaluate(async () => {
      const { create_chart } = await import("/dist/aeris_charts_financial.js");
      const host = document.createElement("div");
      host.style.cssText = "position:fixed;left:0;top:0;width:480px;height:300px;z-index:10000;background:#fff";
      document.body.appendChild(host);
      const chart = await create_chart(host, {
        backend: "canvas2d", autoSize: false,
        initialPane: { horizontal_domain: { type: "category", scale: "band" } },
        generalLegend: { visible: true, position: { x: 260, y: 120 } },
      });
      chart.resize(480, 300, 1);
      chart.add_axis({ id: "x", pane: 0, dimension: "x", scale: "band" });
      chart.add_axis({ id: "y", pane: 0, dimension: "y", scale: "linear" });
      const series = chart.add_series("column", {
        pane: 0, x_axis_id: "x", y_axis_id: "y", title: "Revenue", color: "#2962ff",
      });
      chart.render();
      const canvas = chart.take_screenshot();
      const { data, width, height } = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height);
      let left = width;
      let top = height;
      let bottom = -1;
      for (let y = 0; y < height; y += 1) {
        for (let x = 0; x < width; x += 1) {
          const i = (y * width + x) * 4;
          if (data[i + 2] > 170 && data[i + 2] > data[i] + 60 && data[i + 2] > data[i + 1] + 40) {
            left = Math.min(left, x);
            top = Math.min(top, y);
            bottom = Math.max(bottom, y);
          }
        }
      }
      if (bottom < top || left < 16) throw new Error("legend pill was not exported");
      window.__g8_touch_legend = { chart, host, series };
      const pixels_per_css = canvas.width / 480;
      return {
        x: Math.round(left / pixels_per_css - 15),
        y: Math.round((top + bottom) / (2 * pixels_per_css)),
      };
    });
    await page.touchscreen.tap(target.x, target.y);
    expect(await page.evaluate(() => window.__g8_touch_legend.series.options().visible)).toBe(false);
    await page.mouse.click(target.x, target.y);
    expect(await page.evaluate(() => window.__g8_touch_legend.series.options().visible)).toBe(false);
  } finally {
    await context.close();
  }
});

test("pinned Recharts legend custom coordinates anchor the lower-right corner", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=legend-tooltip");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const custom = page.locator('[data-reference-api="Legend custom position"]');
  const chart = await custom.locator(".recharts-wrapper").boundingBox();
  const legend = await custom.locator(".recharts-legend-wrapper").boundingBox();
  expect(chart).not.toBeNull();
  expect(legend).not.toBeNull();
  // Recharts positions custom coordinates in the chart area inset by its default 5px margin.
  expect(Math.abs(legend.x + legend.width - chart.x - 125)).toBeLessThanOrEqual(1);
  expect(Math.abs(legend.y + legend.height - chart.y - 105)).toBeLessThanOrEqual(1);
  const top = page.locator('[data-reference-api="Legend position top offset"]');
  const top_chart = await top.locator(".recharts-wrapper").boundingBox();
  const top_legend = await top.locator(".recharts-legend-wrapper").boundingBox();
  expect(top_chart).not.toBeNull();
  expect(top_legend).not.toBeNull();
  expect(Math.abs(top_legend.y - top_chart.y - 5)).toBeLessThanOrEqual(1);
});

test("pinned Sankey surface exposes its chart title and default keyboard focus", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=accessibility-localization");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const surface = page.locator('[data-reference-api="Sankey title desc focus"] svg');
  await expect(surface).toHaveAttribute("role", "application");
  await expect(surface).toHaveAttribute("tabindex", "0");
  await expect(surface.locator("title")).toHaveText("Flow comparison");
  await expect(surface.locator("desc")).toHaveText("Value moving from source to destination");
});

test("pinned box and heatmap analogues paint four scatter points and Treemap cells", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=box-heatmap");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const scatter = page.locator('[data-reference-api="Scatter shape composition"]');
  const treemap = page.locator('[data-reference-api="Treemap color reference"]');
  await expect(scatter.locator(".recharts-scatter-symbol")).toHaveCount(4);
  await expect(treemap.locator(".recharts-treemap-depth-1 path.recharts-rectangle")).toHaveCount(4);
  const points = await scatter.locator(".recharts-scatter-symbol").evaluateAll((nodes) =>
    nodes.map((node) => {
      const rect = node.getBoundingClientRect();
      return { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 };
    }));
  expect(points[0].x).toBeLessThan(points[2].x);
  expect(points[2].x).toBeLessThan(points[1].x);
  expect(points[1].x).toBeLessThan(points[3].x);
  expect(points[0].y).toBeGreaterThan(points[1].y);
  expect(points[1].y).toBeGreaterThan(points[2].y);
  expect(points[2].y).toBeGreaterThan(points[3].y);
  const cells = await treemap.locator(".recharts-treemap-depth-1 path.recharts-rectangle").evaluateAll((nodes) =>
    nodes.map((node) => {
      const rect = node.getBoundingClientRect();
      return { area: rect.width * rect.height, fill: node.getAttribute("fill") };
    }));
  expect(cells.every((cell) => cell.area > 0)).toBe(true);
  expect(cells.every((cell) => cell.fill === "#4c8bf5")).toBe(true);
});

test("pinned Recharts automatic X-axis height preserves large ticks and a title", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=scales-axes");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const fixed = page.locator('[data-reference-api="XAxis fixed height comparison"]');
  const automatic = page.locator('[data-reference-api="XAxis automatic height"]');
  const fixed_chart = await fixed.locator(".recharts-wrapper").boundingBox();
  const auto_chart = await automatic.locator(".recharts-wrapper").boundingBox();
  const fixed_axis = await fixed.locator(".recharts-xAxis .recharts-cartesian-axis-line").boundingBox();
  const auto_axis = await automatic.locator(".recharts-xAxis .recharts-cartesian-axis-line").boundingBox();
  expect(fixed_chart).not.toBeNull();
  expect(auto_chart).not.toBeNull();
  expect(fixed_axis).not.toBeNull();
  expect(auto_axis).not.toBeNull();
  const fixed_y = fixed_axis.y - fixed_chart.y;
  const auto_y = auto_axis.y - auto_chart.y;
  expect(auto_y).toBeLessThan(fixed_y - 10);
  await expect(automatic.locator(".recharts-label")).toContainText("Quarter");
});

test("pinned Recharts rotates long X ticks and reserves an automatic strip", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=scales-axes");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const reference = page.locator('[data-reference-api="XAxis rotated long ticks"]');
  const ticks = reference.locator("text.recharts-cartesian-axis-tick-value")
    .filter({ hasText: "revenue" });
  await expect(ticks).toHaveCount(4);
  const transforms = await ticks.evaluateAll((nodes) => nodes.map((node) => node.getAttribute("transform")));
  expect(transforms.every((value) => value?.includes("rotate(-60"))).toBe(true);
  const wrapper = await reference.locator(".recharts-wrapper").boundingBox();
  const axisLine = await reference.locator(".recharts-xAxis .recharts-cartesian-axis-line").boundingBox();
  expect(wrapper).not.toBeNull();
  expect(axisLine).not.toBeNull();
  expect(axisLine.y - wrapper.y).toBeLessThan(125);
});

test("pinned Recharts custom X ticks paint two lines per category", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=scales-axes");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const reference = page.locator('[data-reference-api="XAxis custom multiline tick"]');
  const labels = await reference.locator("text").evaluateAll((nodes) => nodes
    .map((node) => [...node.querySelectorAll("tspan")].map((line) => line.textContent))
    .filter((lines) => lines.length === 2));
  expect(labels).toEqual([
    ["Jan", "Revenue"], ["Feb", "Revenue"],
    ["Mar", "Revenue"], ["Apr", "Revenue"],
  ]);
  await expect(reference.locator(".recharts-line-curve")).toHaveCount(1);
});

test("Aeris multiline ticks grow the strip and restore through the public API", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const makeHost = () => {
      const host = document.createElement("div");
      host.style.cssText = "position:fixed;left:0;top:0;width:420px;height:300px;z-index:10000;background:#fff";
      document.body.appendChild(host);
      return host;
    };
    const blueBottom = (canvas) => {
      const { data, width, height } = canvas.getContext("2d")
        .getImageData(0, 0, canvas.width, canvas.height);
      let bottom = -1;
      for (let y = 0; y < height; y++) {
        for (let x = 0; x < width; x++) {
          const i = (y * width + x) * 4;
          if (data[i + 2] > 160 && data[i + 2] > data[i] + 60
            && data[i + 2] > data[i + 1] + 40) bottom = y;
        }
      }
      return bottom;
    };
    const names = ["Jan Revenue", "Feb Revenue", "Mar Revenue", "Apr Revenue"];
    const ticks = (multiline) => names.map((value) => ({
      type: "category", value,
      label: multiline ? value.replace(" ", value.startsWith("Feb") ? "\r\n" : "\n") : value,
    }));
    const firstHost = makeHost();
    const first = await create_chart(firstHost, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "category", scale: "point" } },
    });
    let saved;
    let finalImage;
    let plainBottom;
    let multilineBottom;
    let changedAfterRotation;
    let plainLeft;
    let multilineLeft;
    let febHit = false;
    try {
      first.resize(420, 300, 1);
      const xAxis = first.add_axis({ id: "month", pane: 0, dimension: "x", scale: "point",
        ticks: ticks(false) });
      const yAxis = first.add_axis({ id: "value", pane: 0, dimension: "y", scale: "linear" });
      const line = first.add_series("xy_line", {
        pane: 0, x_axis_id: "month", y_axis_id: "value", color: "#2962ff",
        point_markers: true, point_radius: 6,
      });
      line.set_data(names.map((x, index) => ({ id: x, x, y: [12, 18, 14, 24][index] })));
      plainBottom = blueBottom(first.take_screenshot());
      plainLeft = first.panes()[0].get_geometry().left;
      xAxis.apply_options({ ticks: ticks(true) });
      const multilineImage = first.take_screenshot().toDataURL("image/png");
      multilineBottom = blueBottom(first.take_screenshot());
      xAxis.apply_options({ tick_angle: -45 });
      const rotatedCanvas = first.take_screenshot();
      finalImage = rotatedCanvas.toDataURL("image/png");
      changedAfterRotation = finalImage !== multilineImage;
      yAxis.apply_options({ ticks: [
        { type: "numeric", value: 10, label: "Ten\nunits" },
        { type: "numeric", value: 20, label: "Twenty\nunits" },
      ] });
      multilineLeft = first.panes()[0].get_geometry().left;
      finalImage = first.take_screenshot().toDataURL("image/png");
      const geometry = first.panes()[0].get_geometry();
      for (let y = Math.ceil(geometry.top); y < geometry.top + geometry.height && !febHit; y += 2) {
        for (let x = 0; x < geometry.width; x += 2) {
          if (first.general_hit_test(0, x, y)?.row_id === "Feb Revenue") {
            febHit = true;
            break;
          }
        }
      }
      saved = first.export_state();
    } finally {
      first.remove(); firstHost.remove();
    }
    const secondHost = makeHost();
    const second = await create_chart(secondHost, { backend: "canvas2d", autoSize: false });
    try {
      second.resize(420, 300, 1);
      second.import_state(saved);
      return {
        plainBottom, multilineBottom, plainLeft, multilineLeft, changedAfterRotation, febHit,
        angle: saved.axes.find((axis) => axis.id === "month")?.tick_angle,
        labels: saved.axes.find((axis) => axis.id === "month")?.ticks.map((tick) => tick.label),
        yLabels: saved.axes.find((axis) => axis.id === "value")?.ticks.map((tick) => tick.label),
        restoredImageEqual: second.take_screenshot().toDataURL("image/png") === finalImage,
      };
    } finally {
      second.remove(); secondHost.remove();
    }
  });
  expect(result.plainBottom).toBeGreaterThan(100);
  expect(result.multilineBottom).toBeLessThan(result.plainBottom - 5);
  expect(result.multilineLeft).toBeGreaterThan(result.plainLeft + 20);
  expect(result.changedAfterRotation).toBe(true);
  expect(result.febHit).toBe(true);
  expect(result.angle).toBe(-45);
  expect(result.labels).toEqual(["Jan\nRevenue", "Feb\r\nRevenue", "Mar\nRevenue", "Apr\nRevenue"]);
  expect(result.yLabels).toEqual(["Ten\nunits", "Twenty\nunits"]);
  expect(result.restoredImageEqual).toBe(true);
});

test("Aeris rotated X ticks reserve plot space and survive public V2 restore", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const makeHost = () => {
      const host = document.createElement("div");
      host.style.cssText = "position:fixed;left:0;top:0;width:400px;height:300px;z-index:10000;background:#fff";
      document.body.appendChild(host);
      return host;
    };
    const bottomBlue = (canvas) => {
      const { data, width, height } = canvas.getContext("2d")
        .getImageData(0, 0, canvas.width, canvas.height);
      let bottom = -1;
      let count = 0;
      for (let y = 0; y < height; y++) {
        for (let x = 0; x < width; x++) {
          const i = (y * width + x) * 4;
          if (data[i + 2] > 160 && data[i + 2] > data[i] + 60
            && data[i + 2] > data[i + 1] + 40) {
            bottom = y;
            count++;
          }
        }
      }
      return { bottom, count };
    };
    const rows = ["Jan revenue", "Feb revenue", "Mar revenue", "Apr revenue"]
      .map((x, index) => ({ id: x, x, y: [12, 18, 14, 24][index] }));
    const firstHost = makeHost();
    const first = await create_chart(firstHost, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "category", scale: "point" } },
    });
    let state;
    let image;
    let plain;
    let rotated;
    try {
      first.resize(400, 300, 1);
      const xAxis = first.add_axis({ id: "month", pane: 0, dimension: "x", scale: "point" });
      first.add_axis({ id: "value", pane: 0, dimension: "y", scale: "linear" });
      const line = first.add_series("xy_line", {
        pane: 0, x_axis_id: "month", y_axis_id: "value", color: "#2962ff",
        point_markers: true, point_radius: 6,
      });
      line.set_data(rows);
      plain = bottomBlue(first.take_screenshot());
      xAxis.apply_options({ tick_angle: -60 });
      const canvas = first.take_screenshot();
      rotated = bottomBlue(canvas);
      const geometry = first.panes()[0].get_geometry();
      let febHit = false;
      for (let y = Math.ceil(geometry.top); y < geometry.top + geometry.height && !febHit; y += 2) {
        for (let x = 0; x < geometry.width; x += 2) {
          if (first.general_hit_test(0, x, y)?.row_id === "Feb revenue") {
            febHit = true;
            break;
          }
        }
      }
      image = canvas.toDataURL("image/png");
      state = first.export_state();
      rotated.hit = febHit;
    } finally {
      first.remove(); firstHost.remove();
    }
    const secondHost = makeHost();
    const second = await create_chart(secondHost, { backend: "canvas2d", autoSize: false });
    try {
      second.resize(400, 300, 1);
      second.import_state(state);
      return {
        plain, rotated,
        angle: state.axes.find((axis) => axis.id === "month")?.tick_angle,
        restoredAngle: second.axis("month")?.options().tick_angle,
        sameImage: second.take_screenshot().toDataURL("image/png") === image,
        rowCount: second.export_state().datasets[0].input.Category.y.length,
      };
    } finally {
      second.remove(); secondHost.remove();
    }
  });
  expect(result.plain.count).toBeGreaterThan(100);
  expect(result.rotated.count).toBeGreaterThan(100);
  expect(result.rotated.hit).toBe(true);
  expect(result.rotated.bottom).toBeLessThan(result.plain.bottom - 20);
  expect(result.angle).toBe(-60);
  expect(result.restoredAngle).toBe(-60);
  expect(result.sameImage).toBe(true);
  expect(result.rowCount).toBe(4);
});

test("Aeris horizontal axis grows for title and chart text through the public API", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:400px;height:300px;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "category", scale: "point" } },
    });
    chart.resize(400, 300, 1);
    const x_axis = chart.add_axis({ id: "quarter", pane: 0, dimension: "x", scale: "point" });
    chart.add_axis({ id: "value", pane: 0, dimension: "y", scale: "linear" });
    const line = chart.add_series("xy_line", {
      pane: 0, x_axis_id: "quarter", y_axis_id: "value", color: "#2962ff",
    });
    line.set_data([{ x: "Q1", y: 12 }, { x: "Q2", y: 18 }, { x: "Q3", y: 15 }]);
    const blue_bottom = () => {
      chart.render();
      const canvas = chart.take_screenshot();
      const { data, width, height } = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height);
      let bottom = -1;
      let count = 0;
      for (let y = 0; y < height; y += 1) {
        for (let x = 0; x < width; x += 1) {
          const i = (y * width + x) * 4;
          if (data[i + 2] > 160 && data[i + 2] > data[i] + 60 && data[i + 2] > data[i + 1] + 40) {
            bottom = y;
            count += 1;
          }
        }
      }
      return { bottom, count };
    };
    const plain = blue_bottom();
    x_axis.apply_options({ title: "Quarter" });
    chart.apply_options({ layout: { fontSize: 22 } });
    const expanded = blue_bottom();
    chart.remove(); host.remove();
    return { plain, expanded };
  });
  expect(result.plain.count).toBeGreaterThan(100);
  expect(result.expanded.count).toBeGreaterThan(100);
  expect(result.expanded.bottom).toBeLessThan(result.plain.bottom - 10);
});

test("pinned accessibility layer keeps French grouped numeric ticks on the chart surface", async ({ page }) => {
  await page.goto("/g1_recharts_reference.html?case=accessibility-localization");
  await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
  const reference = page.locator('[data-reference-api="accessibilityLayer host formatters"]');
  const svg = reference.locator('svg.recharts-surface[role="application"]');
  await expect(svg).toHaveAttribute("tabindex", "0");
  const ticks = (await svg.locator("text.recharts-cartesian-axis-tick-value")
    .allTextContents()).slice(-5);
  expect(ticks).toEqual([0, 600, 1200, 1800, 2400].map((value) =>
    new Intl.NumberFormat("fr-FR").format(value)));
  await expect(reference.locator(".recharts-line-curve")).toBeVisible();
});

test("Aeris preformatted French ticks enter the portable frame without changing data hits", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:350px;height:185px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "category", scale: "point" } },
    });
    try {
      chart.resize(350, 185, 1);
      chart.add_axis({ id: "month", pane: 0, dimension: "x", scale: "point" });
      const values = [0, 600, 1200, 1800, 2400];
      const yAxis = chart.add_axis({ id: "value", pane: 0, dimension: "y", scale: "linear",
        domain: [0, 2400], ticks: values.map((value) => ({ type: "numeric", value })) });
      const line = chart.add_series("xy_line", {
        pane: 0, x_axis_id: "month", y_axis_id: "value", color: "#4c8bf5",
        point_markers: true, point_radius: 6,
      });
      line.set_data([
        { id: "jan", x: "Jan", y: 1200 }, { id: "feb", x: "Feb", y: 1800 },
        { id: "mar", x: "Mar", y: 1400 }, { id: "apr", x: "Apr", y: 2400 },
      ]);
      const before = chart.take_screenshot().toDataURL();
      const formatter = new Intl.NumberFormat("fr-FR");
      yAxis.apply_options({ ticks: values.map((value) =>
        ({ type: "numeric", value, label: formatter.format(value) })) });
      const after = chart.take_screenshot().toDataURL();
      const geometry = chart.panes()[0].get_geometry();
      let hit = null;
      for (let y = geometry.top + 2; y < geometry.top + geometry.height - 2 && hit === null; y += 2) {
        for (let x = 0; x < geometry.width; x += 2) {
          const candidate = chart.general_hit_test(0, x, y);
          if (candidate?.row_id === "feb") { hit = candidate; break; }
        }
      }
      return { ticks: yAxis.options().ticks, persisted: chart.export_state().axes
        .find((axis) => axis.id === "value")?.ticks,
      imageChanged: before !== after, hit, panes: chart.panes().length };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  const expected = [0, 600, 1200, 1800, 2400].map((value) =>
    ({ type: "numeric", value, label: new Intl.NumberFormat("fr-FR").format(value) }));
  expect(result.ticks).toEqual(expected);
  expect(result.persisted).toEqual(expected);
  expect(result.imageChanged).toBe(true);
  expect(result.hit).toMatchObject({ row_id: "feb" });
  expect(result.panes).toBe(1);
});

async function audit_catalog_exports(page, backend, onlyId = null, loseWebGpu = false) {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  return page.evaluate(async ({ requestedBackend, onlyId, loseWebGpu }) => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    // Charts created after a loss get a recreated shared device, so each chart under test loses
    // its own device to exercise the Canvas2D fallback path.
    const lose_device = (target, label) => {
      if (!loseWebGpu) return;
      if (target.backend() !== "webgpu") throw new Error(`${label} did not start on WebGPU`);
      target.wasm.simulate_device_loss_for_test();
      target.render();
      if (target.backend_status().reason !== "device_lost") {
        throw new Error(`${label} did not switch after device loss`);
      }
    };
    const catalog = await (await fetch("/general_catalog.json")).json();
    await document.fonts.ready;
    const results = [];
    for (const example of catalog.examples.filter((entry) => onlyId === null || (Array.isArray(onlyId)
      ? onlyId.includes(entry.id) : entry.id === onlyId))) {
      const host = document.createElement("div");
      host.style.cssText = "position:fixed;left:0;top:0;width:360px;height:220px;z-index:10000";
      document.body.appendChild(host);
      let chart = null;
      let remainingCanvases = null;
      try {
        chart = await create_chart(host, {
          backend: requestedBackend, autoSize: false,
          initialPane: { horizontal_domain: example.horizontal_domain },
          grid: { vertLines: { visible: example.grid !== false },
            horzLines: { visible: example.grid !== false } },
        });
        chart.resize(360, 220, 1);
        lose_device(chart, example.id);
        for (const axis of example.axes ?? []) chart.add_axis({ ...axis, pane: 0 });
        chart.render();
        const empty = chart.take_screenshot().toDataURL("image/png");
        for (const { kind, data, ...options } of example.series ?? []) {
          const series = chart.add_series(kind, {
            x_axis_id: "x", y_axis_id: "y", ...options, pane: 0,
          });
          series.set_data(data);
        }
        for (const { kind, data, ...options } of example.layout_series ?? []) {
          chart.add_layout_series(kind, { ...options, pane: 0 }, data);
        }
        for (const reference of example.references ?? []) {
          chart.add_general_reference({ ...reference, pane: 0 });
        }
        // Cycle the backing size so both source and fresh-restored canvases use a
        // newly allocated surface before comparing their exported pixels.
        chart.resize(361, 221, 1);
        // A full layout settles grow-only axis strips after incremental data installs.
        chart.resize(360, 220, 1);
        chart.render();
        await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        const image = chart.take_screenshot();
        const imageUrl = image.toDataURL("image/png");
        const changed = imageUrl !== empty;
        const restoreHost = document.createElement("div");
        restoreHost.style.cssText = host.style.cssText;
        document.body.appendChild(restoreHost);
        let restored = null;
        let restoredSame = false;
        let restoredStateSame = false;
        let restoredCanvases = null;
        try {
          restored = await create_chart(restoreHost, {
            backend: requestedBackend, autoSize: false,
            initialPane: { horizontal_domain: example.horizontal_domain },
            grid: { vertLines: { visible: example.grid !== false },
              horzLines: { visible: example.grid !== false } },
          });
          restored.resize(360, 220, 1);
          lose_device(restored, `${example.id} restore`);
          const state = chart.export_state();
          restored.import_state(state);
          restored.resize(360, 220, 1);
          restoredStateSame = JSON.stringify(restored.export_state()) === JSON.stringify(state);
          restored.render();
          await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
          const restoredImage = restored.take_screenshot();
          const restoredUrl = restoredImage.toDataURL("image/png");
          restoredSame = restoredUrl === imageUrl;
        } finally {
          restored?.remove();
          restoredCanvases = restoreHost.querySelectorAll("canvas").length;
          restoreHost.remove();
        }
        results.push({ id: example.id, backend: chart.backend(), changed, restoredSame, restoredStateSame,
          restoredCanvases, width: image.width, height: image.height,
          kinds: [...(example.series ?? []), ...(example.layout_series ?? [])]
            .map((series) => series.kind) });
      } finally {
        chart?.remove();
        remainingCanvases = host.querySelectorAll("canvas").length;
        host.remove();
      }
      results[results.length - 1].remainingCanvases = remainingCanvases;
    }
    return results;
  }, { requestedBackend: backend, onlyId, loseWebGpu });
}

for (const backend of ["canvas2d", "webgpu"]) {
  test(`all catalog families export and dispose through ${backend}`, async ({ page, browserName }) => {
    test.skip(backend === "webgpu" && browserName !== "chromium",
      "WebGPU test adapter is configured in Chromium");
    test.setTimeout(120_000);
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    const results = await audit_catalog_exports(page, backend);
    await page.waitForTimeout(50);
    expect(errors).toEqual([]);
    expect(results).toHaveLength(46);
    expect(new Set(results.flatMap((entry) => entry.kinds))).toEqual(new Set([
      "xy_line", "xy_area", "range_area", "range_bar", "error_bar", "column",
      "histogram", "horizontal_bar", "box_plot", "heatmap_grid", "scatter", "bubble",
      "pie", "radar", "polar_area", "radial_bar", "funnel", "treemap", "sankey", "sunburst",
    ]));
    expect(results.filter((entry) => !entry.restoredSame).map((entry) => entry.id)).toEqual([]);
    for (const entry of results) {
      expect(entry, entry.id).toMatchObject({ backend, changed: true, restoredSame: true,
        restoredStateSame: true,
        width: 360, height: 220, remainingCanvases: 0, restoredCanvases: 0 });
    }
  });
}


test("all catalog families restore and dispose after WebGPU device loss", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "WebGPU test adapter is configured in Chromium");
  test.setTimeout(120_000);
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const results = await audit_catalog_exports(page, "webgpu", null, true);
  expect(errors).toEqual([]);
  expect(results).toHaveLength(46);
  expect(results.filter((entry) => !entry.restoredSame || !entry.restoredStateSame).map((entry) => entry.id)).toEqual([]);
  for (const entry of results) {
    expect(entry, entry.id).toMatchObject({ backend: "canvas2d", changed: true,
      restoredSame: true, restoredStateSame: true, remainingCanvases: 0, restoredCanvases: 0 });
  }
});
