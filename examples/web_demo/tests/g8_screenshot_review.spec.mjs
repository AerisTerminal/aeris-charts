import { test, expect } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { PNG } from "pngjs";

// Maintainer-run phase-closure evidence: saves side-by-side Aeris and pinned Recharts 3.10.1 PNGs
// for every matrix row. It writes into the ignored target/ directory and is opt-in so the normal
// suite never produces review artifacts.
test.skip(!process.env.AERIS_G8_SCREENSHOTS, "set AERIS_G8_SCREENSHOTS=1 to capture review screenshots");
test.use({ deviceScaleFactor: 1, viewport: { width: 1600, height: 1200 } });
test.setTimeout(240_000);

const matrix = JSON.parse(fs.readFileSync(
  new URL("../fixtures/g1/recharts_3_10_1/matrix.json", import.meta.url), "utf8"));
const out_root = fileURLToPath(new URL("../../../target/g8_screenshots/", import.meta.url));

const variants = [
  { id: "default", w: 350, h: 185, theme: "light" },
  { id: "small", w: 220, h: 130, theme: "light" },
  { id: "large", w: 700, h: 370, theme: "light" },
  { id: "long-labels", w: 350, h: 185, theme: "light", labels: "long" },
  { id: "dark", w: 350, h: 185, theme: "dark", aeris_only: true },
  { id: "dark-small", w: 220, h: 130, theme: "dark", aeris_only: true },
  { id: "hover", w: 350, h: 185, theme: "light", active: "hover" },
  { id: "focus", w: 350, h: 185, theme: "light", active: "focus" },
];

// Aeris scenes mirror each Recharts Evidence block in fixture order.
const row_scenes = {
  "standalone-composed": ["standard", "composed"],
  "paths-points": ["standard", "area", "error_bar", "range_area", "step_gaps", "scatter", "bubble",
    "scatter_square"],
  "bars-stacks": ["stack_legend", "stack_sign", "stack_expand", "stack_positive", "stack_cumulative",
    "stack_silhouette", "stack_wiggle", "stack_mixed_expand"],
  "box-heatmap": ["box_plot", "heatmap"],
  "scales-axes": ["standard", "axis_font_title", "axis_font_title", "rotated", "multiline",
    "horizontal_bars"],
  polar: ["pie", "radar", "radial_bar"],
  "hierarchy-flow": ["funnel", "treemap", "sankey", "sunburst"],
  "legend-tooltip": ["standard", "legend_top", "legend_custom"],
  "references-labels": ["references", "reference_bounds"],
  "brush-selection-sync": ["standard", "standard"],
  "responsive-layout": ["standard_700x350", "standard_600x300"],
  "react-authoring": ["standard"],
  customization: ["cells"],
  animation: ["standard"],
  "accessibility-localization": ["sankey", "french"],
  "persistence-export-recovery": ["plain_line"],
  "packaging-migration": ["plain_line"],
};

function tile(buffers, max_width = 760, gap = 8, background = [255, 255, 255, 255]) {
  const images = buffers.map((buffer) => PNG.sync.read(buffer));
  const placed = [];
  let x = 0;
  let y = 0;
  let line = 0;
  let width = 0;
  for (const image of images) {
    if (x > 0 && x + image.width > max_width) {
      x = 0;
      y += line + gap;
      line = 0;
    }
    placed.push({ image, x, y });
    x += image.width + gap;
    line = Math.max(line, image.height);
    width = Math.max(width, x - gap);
  }
  const out = new PNG({ width: Math.max(1, width), height: Math.max(1, y + line) });
  for (let i = 0; i < out.data.length; i += 4) out.data.set(background, i);
  for (const { image, x: left, y: top } of placed) {
    PNG.bitblt(image, out, 0, 0, image.width, image.height, left, top);
  }
  return PNG.sync.write(out);
}

function stack_pair(top, bottom) {
  const a = PNG.sync.read(top);
  const b = PNG.sync.read(bottom);
  const out = new PNG({ width: Math.max(a.width, b.width), height: a.height + b.height + 12 });
  for (let i = 0; i < out.data.length; i += 4) out.data.set([255, 0, 255, 255], i);
  PNG.bitblt(a, out, 0, 0, a.width, a.height, 0, 0);
  PNG.bitblt(b, out, 0, 0, b.width, b.height, 0, a.height + 12);
  return PNG.sync.write(out);
}

async function build_aeris({ scenes, w, h, theme, labels }) {
  const api = await import("/dist/aeris_charts_financial.js");
  window.__g8_shots?.cleanup();
  await document.fonts.ready;
  const dark = theme === "dark";
  const overlay = document.createElement("div");
  overlay.style.cssText = `position:fixed;inset:0;z-index:10000;overflow:auto;padding:20px;`
    + `background:${dark ? "#0b0f17" : "#ffffff"}`;
  const container = document.createElement("div");
  container.className = "g8-shot-chart";
  container.style.cssText = "display:flex;flex-wrap:wrap;gap:8px;width:760px;align-items:flex-start";
  overlay.appendChild(container);
  document.body.appendChild(overlay);
  const month = labels === "long"
    ? ["January revenue — 一月", "Février Ünïcödé ✓", "März 北京 Übersicht", "Abril — نيسان"]
    : ["Jan", "Feb", "Mar", "Apr"];
  const ids = ["jan", "feb", "mar", "apr"];
  const value = [12, 18, 14, 24];
  const alternate = [8, 11, 16, 19];
  const size = [90, 140, 110, 180];
  const signed = [[12, -8], [-18, 11], [-14, -16], [24, 19]];
  const xy = (ys, names = month) => ys.map((y, i) => ({ id: ids[i], x: names[i], y }));
  const charts = [];
  const make = async (width, height, domain, options = {}) => {
    const host = document.createElement("div");
    host.className = "g8-shot-host";
    host.style.cssText = `position:relative;width:${width}px;height:${height}px;flex:none`;
    container.appendChild(host);
    const chart = await api.create_chart(host, {
      backend: "canvas2d", autoSize: false, theme,
      initialPane: { horizontal_domain: domain },
      grid: { vertLines: { visible: false }, horzLines: { visible: false } },
      ...options,
    });
    chart.resize(width, height, 1);
    const accessibility = api.enable_accessibility(chart);
    charts.push({ chart, host, accessibility });
    return chart;
  };
  const cartesian = async (scale, options, width = w, height = h) => {
    const chart = await make(width, height, { type: "category", scale }, options);
    chart.add_axis({ id: "x", pane: 0, dimension: "x", scale });
    chart.add_axis({ id: "y", pane: 0, dimension: "y", scale: "linear" });
    return chart;
  };
  const numeric = async () => {
    const chart = await make(w, h, { type: "continuous", scale: "linear" });
    chart.add_axis({ id: "x", pane: 0, dimension: "x", scale: "linear" });
    chart.add_axis({ id: "y", pane: 0, dimension: "y", scale: "linear" });
    return chart;
  };
  const series = (chart, kind, options, data) => {
    const handle = chart.add_series(kind, { pane: 0, x_axis_id: "x", y_axis_id: "y", ...options });
    handle.set_data(data);
    return handle;
  };
  const standard = async (width = w, height = h) => {
    const chart = await cartesian("point", {
      grid: { vertLines: { visible: true }, horzLines: { visible: true } },
      generalLegend: { visible: true },
    }, width, height);
    series(chart, "xy_line", { title: "value", color: "#4c8bf5", point_markers: true, point_radius: 3 },
      xy(value));
    series(chart, "xy_line", { title: "alternate", color: "#28b7a4", point_markers: true,
      point_radius: 3 }, xy(alternate));
  };
  const stack = async (mode, rows, legend = false) => {
    const chart = await cartesian("band", legend ? {
      grid: { vertLines: { visible: true }, horzLines: { visible: true } },
      generalLegend: { visible: true },
    } : {});
    const names = legend ? month : ["A", "B", "C", "D"];
    series(chart, "column", { title: "value", color: "#4c8bf5", stack_id: "s", stack_mode: mode },
      rows.map((r, i) => ({ id: ids[i], x: names[i], y: r[0] })));
    series(chart, "column", { title: "alternate", color: "#28b7a4", stack_id: "s", stack_mode: mode },
      rows.map((r, i) => ({ id: ids[i], x: names[i], y: r[1] })));
  };
  const positive = value.map((v, i) => [v, alternate[i]]);
  const hierarchy = month.map((label, i) => ({ id: ids[i], parent: "all", label, value: value[i] }));
  const flow = {
    type: "flow",
    nodes: [{ id: "visit", label: "Visit" }, { id: "trial", label: "Trial" }, { id: "paid", label: "Paid" }],
    links: [{ source: "visit", target: "trial", value: 18 }, { source: "trial", target: "paid", value: 11 }],
  };
  const polar = async (axes_visible) => {
    const chart = await make(w, h, { type: "polar" }, axes_visible ? {
      grid: { vertLines: { visible: true }, horzLines: { visible: true } },
    } : {});
    chart.add_axis({ id: "x", pane: 0, dimension: "angle", scale: "angular_category", visible: axes_visible });
    chart.add_axis({ id: "y", pane: 0, dimension: "radius", scale: "radial_linear", visible: axes_visible });
    return chart;
  };
  const builders = {
    standard: () => standard(),
    standard_700x350: () => standard(700, 350),
    standard_600x300: () => standard(600, 300),
    composed: async () => {
      const chart = await cartesian("band");
      series(chart, "xy_area", { title: "alternate", color: "#28b7a4" }, xy(alternate));
      series(chart, "column", { title: "value", color: "#8b6df6" }, xy(value));
      series(chart, "xy_line", { title: "value", color: "#315fc8", point_markers: true, point_radius: 3 },
        xy(value));
    },
    area: async () => {
      const chart = await cartesian("point");
      series(chart, "xy_area", { color: "#067a67", point_markers: true, point_radius: 3 }, xy(value));
    },
    error_bar: async () => {
      const chart = await cartesian("point");
      series(chart, "xy_line", { color: "#4c8bf5", point_markers: true, point_radius: 3 }, xy(value));
      const error = [2, 3, 1, 4];
      series(chart, "error_bar", { color: "#e46f61" }, value.map((y, i) =>
        ({ id: ids[i], x: month[i], y, y_low: y - error[i], y_high: y + error[i] })));
    },
    range_area: async () => {
      const chart = await cartesian("point");
      const ranges = [[8, 12], [11, 18], [10, 14], [19, 24]];
      series(chart, "range_area", { color: "#067a67" }, ranges.map(([low, high], i) =>
        ({ id: ids[i], x: month[i], low, high })));
    },
    step_gaps: async () => {
      const chart = await cartesian("point");
      const names = ["Jan", "Feb", "Mar", "Apr", "May"];
      const rows = [12, 18, null, 14, 24].map((y, i) => ({ id: names[i], x: names[i], y }));
      series(chart, "xy_line", { color: "#e46f61", interpolation: "step_middle" }, rows);
      series(chart, "xy_line", { color: "#4c8bf5", interpolation: "step_middle", connect_missing: true },
        rows);
    },
    scatter: async () => {
      const chart = await numeric();
      series(chart, "scatter", { color: "#8b6df6" }, value.map((x, i) => ({ id: ids[i], x, y: alternate[i] })));
    },
    bubble: async () => {
      const chart = await numeric();
      series(chart, "bubble", { color: "#8b6df6" }, value.map((x, i) =>
        ({ id: ids[i], x, y: alternate[i], size: size[i] })));
    },
    scatter_square: async () => {
      const chart = await numeric();
      series(chart, "scatter", { color: "#8b6df6", point_symbol: "square" }, value.map((x, i) =>
        ({ id: ids[i], x, y: alternate[i] })));
    },
    stack_legend: () => stack("normal", positive, true),
    stack_sign: () => stack("normal", signed),
    stack_expand: () => stack("percent", positive),
    stack_positive: () => stack("positive", signed),
    stack_cumulative: () => stack("cumulative", [[12, -18], [-18, 11], [6, 4], [10, 5]]),
    stack_silhouette: () => stack("silhouette", positive),
    stack_wiggle: () => stack("wiggle", positive),
    stack_mixed_expand: () => stack("percent", signed),
    box_plot: async () => {
      const chart = await cartesian("band");
      series(chart, "box_plot", { color: "#315fc8", box_fill_color: "#8b6df6" }, value.map((m, i) =>
        ({ id: ids[i], x: month[i], min: m - 6, q1: m - 3, median: m, q3: m + 2, max: m + 5 })));
    },
    heatmap: async () => {
      const chart = await make(w, h, { type: "category", scale: "band" });
      chart.add_axis({ id: "x", pane: 0, dimension: "x", scale: "band" });
      chart.add_axis({ id: "y", pane: 0, dimension: "y", scale: "band" });
      const regions = ["North", "South", "West"];
      series(chart, "heatmap_grid", { heatmap_low_color: "#dbe7ff", heatmap_high_color: "#4c8bf5" },
        month.flatMap((x, i) => regions.map((y, j) => ({ id: `${i}-${j}`, x, y, value: value[i] * (j + 1) }))));
    },
    axis_font_title: async () => {
      const chart = await cartesian("point");
      chart.axis("x").apply_options({ title: "Quarter" });
      chart.apply_options({ layout: { fontSize: 22 } });
      series(chart, "xy_line", { color: "#4c8bf5", point_markers: true, point_radius: 3 }, xy(value));
    },
    rotated: async () => {
      const chart = await cartesian("point");
      chart.axis("x").apply_options({ tick_angle: -60 });
      series(chart, "xy_line", { color: "#4c8bf5", point_markers: true, point_radius: 3 },
        xy(value, month.map((m) => `${m} revenue`)));
    },
    multiline: async () => {
      const chart = await cartesian("point");
      const names = month.map((m) => `${m} Revenue`);
      chart.axis("x").apply_options({ ticks: names.map((n, i) =>
        ({ type: "category", value: n, label: `${month[i]}\nRevenue` })) });
      series(chart, "xy_line", { color: "#4c8bf5", point_markers: true, point_radius: 3 }, xy(value, names));
    },
    horizontal_bars: async () => {
      const chart = await make(480, 300, { type: "continuous", scale: "linear" });
      chart.add_axis({ id: "x", pane: 0, dimension: "x", scale: "linear" });
      chart.add_axis({ id: "y", pane: 0, dimension: "y", scale: "band" });
      series(chart, "horizontal_bar", { color: "#d02020" },
        [{ id: "first", x: "First", y: 20 }, { id: "second", x: "Second", y: 40 }]);
    },
    pie: async () => {
      const chart = await polar(false);
      const colors = ["#4c8bf5", "#28b7a4", "#8b6df6", "#e4a11b"];
      series(chart, "pie", { polar: { inner_radius: 0.33, outer_radius: 0.7 } },
        value.map((y, i) => ({ id: ids[i], x: month[i], y, color: colors[i] })));
    },
    radar: async () => {
      const chart = await polar(true);
      series(chart, "radar", { color: "#4c8bf5", fill_opacity: 0.35 }, xy(value));
    },
    radial_bar: async () => {
      const chart = await polar(false);
      series(chart, "radial_bar", { color: "#28b7a4", polar: { inner_radius: 0.2, outer_radius: 0.9 } },
        xy(value));
    },
    funnel: async () => {
      const chart = await make(w, h, { type: "layout" });
      chart.add_layout_series("funnel", { pane: 0, color: "#8b6df6" }, {
        type: "funnel", stages: month.map((label, i) => ({ id: ids[i], label, value: value[i] })),
      });
    },
    treemap: async () => {
      const chart = await make(w, h, { type: "layout" });
      chart.add_layout_series("treemap", { pane: 0, color: "#4c8bf5" }, {
        type: "hierarchy", nodes: [{ id: "all", parent: null, label: "All", value: null }, ...hierarchy],
      });
    },
    sankey: async () => {
      const chart = await make(w, h, { type: "layout" });
      chart.add_layout_series("sankey", { pane: 0, color: "#8b6df6" }, flow);
    },
    sunburst: async () => {
      const chart = await make(w, h, { type: "layout" });
      chart.add_layout_series("sunburst", { pane: 0, color: "#28b7a4" }, {
        type: "hierarchy", nodes: [{ id: "all", parent: null, label: "All", value: null }, ...hierarchy],
      });
    },
    legend_top: async () => {
      const chart = await cartesian("point", { generalLegend: { visible: true, position: "top", offset: 8 } });
      series(chart, "xy_line", { title: "value", color: "#4c8bf5", point_markers: true, point_radius: 3 },
        xy(value));
    },
    legend_custom: async () => {
      const chart = await cartesian("point", { generalLegend: { visible: true, position: { x: 125, y: 105 } } });
      series(chart, "xy_line", { title: "value", color: "#4c8bf5", point_markers: true, point_radius: 3 },
        xy(value));
    },
    references: async () => {
      const chart = await cartesian("band", {
        grid: { vertLines: { visible: true }, horzLines: { visible: true } },
      });
      chart.add_general_reference({ kind: "region", pane: 0, x_axis_id: "x", y_axis_id: "y",
        x_from: month[1], x_to: month[2], fill_color: "#fbe2a6" });
      chart.add_general_reference({ kind: "line", pane: 0, axis_id: "y", value: 16, color: "#e46f61" });
      series(chart, "xy_area", { color: "#067a67", data_labels: true }, xy(value));
    },
    reference_bounds: async () => {
      const chart = await cartesian("band");
      chart.add_general_reference({ kind: "region", pane: 0, x_axis_id: "x", y_axis_id: "y",
        y_from: 8, y_to: 16, fill_color: "#b7eadf" });
      chart.add_general_reference({ kind: "region", pane: 0, x_axis_id: "x", y_axis_id: "y",
        x_from: month[1], fill_color: "#fbe2a6" });
      series(chart, "xy_area", { color: "#4c8bf5" }, xy(value));
    },
    cells: async () => {
      const chart = await cartesian("band");
      const colors = ["#4c8bf5", "#28b7a4", "#8b6df6", "#e4a11b"];
      series(chart, "column", { title: "value", bar_corner_radius: 6 },
        value.map((y, i) => ({ id: ids[i], x: month[i], y, color: colors[i] })));
    },
    french: async () => {
      const chart = await make(w, h, { type: "category", scale: "point" });
      chart.add_axis({ id: "x", pane: 0, dimension: "x", scale: "point" });
      const formatter = new Intl.NumberFormat("fr-FR");
      chart.add_axis({ id: "y", pane: 0, dimension: "y", scale: "linear", domain: [0, 2400],
        ticks: [0, 600, 1200, 1800, 2400].map((v) => ({ type: "numeric", value: v, label: formatter.format(v) })) });
      series(chart, "xy_line", { color: "#4c8bf5", point_markers: true, point_radius: 3 },
        xy(value.map((v) => v * 100)));
    },
    plain_line: async () => {
      const chart = await cartesian("point");
      series(chart, "xy_line", { color: "#4c8bf5", point_markers: true, point_radius: 3 }, xy(value));
    },
  };
  const errors = [];
  for (const scene of scenes) {
    try {
      await builders[scene]();
    } catch (error) {
      errors.push(`${scene}: ${error?.message ?? error}`);
    }
  }
  for (const { chart } of charts) chart.render();
  await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  window.__g8_shots = {
    charts,
    cleanup() {
      for (const { chart, accessibility } of charts) {
        accessibility?.dispose?.();
        chart.remove();
      }
      overlay.remove();
      delete window.__g8_shots;
    },
  };
  return { errors, count: charts.length };
}

async function settle(page) {
  await page.evaluate(() => new Promise((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(resolve))));
}

for (const row of matrix.rows) {
  test(`G8 screenshot review: ${row.id}`, async ({ page, browserName }) => {
    test.skip(browserName !== "chromium", "review captures are Chromium-only");
    const directory = path.join(out_root, row.id);
    fs.mkdirSync(directory, { recursive: true });
    const scenes = row_scenes[row.id];
    expect(scenes, row.id).toBeTruthy();
    const report = [];
    for (const variant of variants) {
      const per_variant = path.join(directory, variant.id);
      fs.mkdirSync(per_variant, { recursive: true });
      let recharts_png = null;
      if (!variant.aeris_only) {
        const query = new URLSearchParams({ case: row.id, w: String(variant.w), h: String(variant.h) });
        if (variant.labels) query.set("labels", variant.labels);
        await page.goto(`/g1_recharts_reference.html?${query}`);
        await expect(page.locator("body")).toHaveAttribute("data-ready", "true");
        // Default Recharts entry animations take 1.5 s.
        await page.waitForTimeout(1700);
        const chart = page.locator("section .chart");
        if (!variant.active) {
          recharts_png = await chart.screenshot();
        } else {
          const wrappers = chart.locator(".recharts-wrapper");
          const shots = [];
          for (let i = 0; i < await wrappers.count(); i++) {
            const wrapper = wrappers.nth(i);
            const box = await wrapper.boundingBox();
            if (variant.active === "hover") {
              await page.mouse.move(box.x + box.width * 0.55, box.y + box.height * 0.45);
            } else {
              const surface = wrapper.locator("> svg.recharts-surface");
              await surface.focus().catch(() => {});
              await page.keyboard.press("ArrowRight");
            }
            await page.waitForTimeout(250);
            shots.push(await wrapper.screenshot());
            fs.writeFileSync(path.join(per_variant, `${i}-recharts.png`), shots.at(-1));
            await page.mouse.move(0, 0);
            await page.evaluate(() => document.activeElement?.blur?.());
          }
          recharts_png = tile(shots);
        }
        fs.writeFileSync(path.join(directory, `${variant.id}-recharts.png`), recharts_png);
      }
      await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
      await page.waitForFunction(() => window.__chart?.backend?.() !== undefined);
      const built = await page.evaluate(build_aeris,
        { scenes, w: variant.w, h: variant.h, theme: variant.theme, labels: variant.labels ?? null });
      report.push({ variant: variant.id, ...built });
      const background = variant.theme === "dark" ? [11, 15, 23, 255] : [255, 255, 255, 255];
      let aeris_png;
      if (!variant.active) {
        aeris_png = await page.locator(".g8-shot-chart").screenshot();
      } else {
        const hosts = page.locator(".g8-shot-host");
        const shots = [];
        for (let i = 0; i < await hosts.count(); i++) {
          const host = hosts.nth(i);
          await host.scrollIntoViewIfNeeded();
          const box = await host.boundingBox();
          if (variant.active === "hover") {
            await page.mouse.move(box.x + box.width * 0.55, box.y + box.height * 0.45);
          } else {
            await page.evaluate((index) => window.__g8_shots.charts[index].accessibility.focus(0), i);
            await page.keyboard.press("ArrowRight");
          }
          await settle(page);
          await page.waitForTimeout(120);
          shots.push(await host.screenshot());
          fs.writeFileSync(path.join(per_variant, `${i}-aeris.png`), shots.at(-1));
          await page.mouse.move(0, 0);
          await page.evaluate(() => document.activeElement?.blur?.());
        }
        aeris_png = tile(shots, 760, 8, background);
      }
      fs.writeFileSync(path.join(directory, `${variant.id}-aeris.png`), aeris_png);
      if (recharts_png) {
        fs.writeFileSync(path.join(directory, `${variant.id}-pair.png`), stack_pair(aeris_png, recharts_png));
      }
      await page.evaluate(() => window.__g8_shots?.cleanup());
    }
    fs.writeFileSync(path.join(directory, "report.json"), JSON.stringify(report, null, 2));
    expect(report.flatMap(({ errors }) => errors)).toEqual([]);
  });
}
