import { test, expect } from "@playwright/test";
import waterfall_row, { bars, colors, steps } from "../echarts_reference/rows/waterfall.js";
import bullet_row, { bullets, measure_color } from "../echarts_reference/rows/bullet.js";
import lollipop_row, { dumbbell, dumbbell_color, head_radius, lollipop, lollipop_color } from "../echarts_reference/rows/lollipop_dumbbell.js";
import slope_row, { bump, quarters, periods, ranks, slope } from "../echarts_reference/rows/slope_bump.js";

// G9 business families against pinned Apache ECharts 6.1.0. ECharts geometry comes from
// `convertToPixel` on the recipe each family uses there; Aeris geometry comes from exact-color
// pixel scans of the public Canvas2D screenshot, because every mark here has an explicit color.
// Both sides are normalized against the data extent, so axis nicing and gutters do not matter.

const MARKS = ["increase", "decrease", "subtotal", "total"];

const chart_of = (row, id) => row.charts.find((chart) => chart.id === id);
const save_png = (info, name, data_url) => info.attach(name, {
  body: Buffer.from(data_url.split(",")[1], "base64"), contentType: "image/png",
});

async function echarts_page(page, row, info, ids) {
  await page.goto(`/echarts_reference.html?case=${row.id}`);
  await page.waitForFunction(() => window.__echarts_reference_ready === true);
  for (const id of ids) {
    const shot = await page.locator(`[data-chart=${id}]`).screenshot();
    await save_png(info, `echarts-${id}.png`, `data:image/png;base64,${shot.toString("base64")}`);
  }
}

/** In-page pixel helpers over one `take_screenshot()` capture, in CSS pixels. */
function pixel_library() {
  const rgb = (css) => [1, 3, 5].map((index) => parseInt(css.slice(index, index + 2), 16));
  const near = (pixel, color, tolerance) => pixel.every((channel, index) => Math.abs(channel - color[index]) <= tolerance);
  const clamp = (cap, rect) => ({
    x0: Math.max(0, Math.ceil(rect.left)), x1: Math.min(cap.width, Math.floor(rect.left + rect.width)),
    y0: Math.max(0, Math.ceil(rect.top)), y1: Math.min(cap.height, Math.floor(rect.top + rect.height)),
  });
  window.__g9b_px = {
    rgb,
    capture(chart, width) {
      const canvas = chart.take_screenshot();
      const scale = canvas.width / width;
      const data = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
      return {
        width, height: Math.floor(canvas.height / scale), image: canvas.toDataURL("image/png"),
        at(x, y) {
          const offset = (Math.floor(y * scale) * canvas.width + Math.floor(x * scale)) * 4;
          return [data[offset], data[offset + 1], data[offset + 2]];
        },
      };
    },
    /** Consecutive columns (`major` x) or rows (`major` y) holding pixels of any listed color. */
    runs(cap, rect, colors, major, tolerance = 6) {
      const targets = colors.map(rgb);
      const { x0, x1, y0, y1 } = clamp(cap, rect);
      const [m0, m1, c0, c1] = major === "x" ? [x0, x1, y0, y1] : [y0, y1, x0, x1];
      const out = [];
      let run = null;
      for (let m = m0; m < m1; m++) {
        let any = false;
        for (let c = c0; c < c1; c++) {
          const index = targets.findIndex((target) => near(major === "x" ? cap.at(m, c) : cap.at(c, m), target, tolerance));
          if (index < 0) continue;
          any = true;
          run ??= { from: m, to: m, min: c, max: c, counts: targets.map(() => 0) };
          run.to = m;
          run.min = Math.min(run.min, c);
          run.max = Math.max(run.max, c);
          run.counts[index] += 1;
        }
        if (!any && run !== null) {
          out.push(run);
          run = null;
        }
      }
      if (run !== null) out.push(run);
      return out.map(({ counts, ...entry }) => ({ ...entry, color: counts.indexOf(Math.max(...counts)) }));
    },
    /** Centers of filled marks of one color whose interior holds a solid (2k+1)-pixel square. */
    blobs(cap, rect, color, k, tolerance = 8) {
      const target = rgb(color);
      const { x0, x1, y0, y1 } = clamp(cap, rect);
      const width = x1 - x0;
      const grid = new Uint8Array(width * (y1 - y0));
      for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) grid[(y - y0) * width + x - x0] = near(cap.at(x, y), target, tolerance) ? 1 : 0;
      const clusters = [];
      for (let y = y0 + k; y < y1 - k; y++) {
        for (let x = x0 + k; x < x1 - k; x++) {
          let solid = true;
          for (let dy = -k; dy <= k && solid; dy++) for (let dx = -k; dx <= k && solid; dx++) solid = grid[(y + dy - y0) * width + x + dx - x0] === 1;
          if (!solid) continue;
          const cluster = clusters.find((entry) => Math.abs(entry.sx / entry.n - x) <= 2 * k + 2 && Math.abs(entry.sy / entry.n - y) <= 2 * k + 2);
          if (cluster === undefined) clusters.push({ sx: x, sy: y, n: 1 });
          else { cluster.sx += x; cluster.sy += y; cluster.n += 1; }
        }
      }
      return clusters.map((entry) => ({ x: entry.sx / entry.n + 0.5, y: entry.sy / entry.n + 0.5 }));
    },
    /** Vertical extent of pixels near `color` inside `rect`, or null. */
    extent(cap, rect, color, tolerance) {
      const target = rgb(color);
      const { x0, x1, y0, y1 } = clamp(cap, rect);
      let min = Infinity;
      let max = -Infinity;
      for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) {
        if (!near(cap.at(x, y), target, tolerance)) continue;
        min = Math.min(min, y);
        max = Math.max(max, y);
      }
      return min === Infinity ? null : { min, max };
    },
    /** Pixels in `rect` darker than `threshold` in every channel. */
    dark(cap, rect, threshold = 140) {
      const { x0, x1, y0, y1 } = clamp(cap, rect);
      let count = 0;
      for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) if (Math.max(...cap.at(x, y)) < threshold) count += 1;
      return count;
    },
  };
}

/** One Aeris general chart in a fixed host with one pane and axes `x`/`y`, kept on `window.__g9b`. */
async function aeris_chart(page, config) {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.evaluate(pixel_library);
  return page.evaluate(async ({ width, height, domain, axes, series }) => {
    const api = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.id = "g9b-host";
    host.style.cssText = `position:fixed;left:0;top:0;width:${width}px;height:${height}px;z-index:10000;background:#fff`;
    document.body.appendChild(host);
    const chart = await api.create_chart(host, {
      backend: "canvas2d", autoSize: false, theme: "light", initialPane: { horizontal_domain: domain },
    });
    chart.resize(width, height, 1);
    for (const axis of axes) chart.add_axis({ pane: 0, ...axis });
    const handles = series.map(({ kind, options, data }) => {
      const handle = chart.add_series(kind, { pane: 0, x_axis_id: "x", y_axis_id: "y", ...options });
      handle.set_data(data);
      return handle;
    });
    chart.render();
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    window.__g9b = { api, chart, host, handles, width };
    return chart.panes()[0].get_geometry();
  }, config);
}

const remove_chart = (page) => page.evaluate(() => {
  const { chart, host, accessibility } = window.__g9b;
  accessibility?.detach();
  chart.remove();
  host.remove();
  delete window.__g9b;
});

const category_axes = (horizontal, value_axis = {}) => horizontal
  ? [{ id: "x", dimension: "x", scale: "linear", ...value_axis }, { id: "y", dimension: "y", scale: "band" }]
  : [{ id: "x", dimension: "x", scale: "band" }, { id: "y", dimension: "y", scale: "linear", ...value_axis }];
const category_domain = (horizontal) => (horizontal
  ? { type: "continuous", scale: "linear" } : { type: "category", scale: "band" });

/** ECharts pixels as `[category coordinate, value coordinate]` for either orientation. */
async function echarts_points(page, id, horizontal, pairs) {
  return page.evaluate(({ id, horizontal, pairs }) => {
    const instance = window.__echarts_instances[id];
    return pairs.map(([category, value]) => {
      const point = instance.convertToPixel({ gridIndex: 0 }, horizontal ? [value, category] : [category, value]);
      return horizontal ? [point[1], point[0]] : point;
    });
  }, { id, horizontal, pairs });
}

const peak = Math.max(...bars.map((bar) => bar.end));

for (const horizontal of [false, true]) {
  const id = horizontal ? "waterfall_horizontal" : "waterfall";
  test(`${horizontal ? "horizontal" : "vertical"} waterfall bars, colors and connectors match the ECharts placeholder stack`, async ({ page }, info) => {
    const panel = chart_of(waterfall_row, id);
    await echarts_page(page, waterfall_row, info, [id]);
    const pairs = bars.flatMap((bar, index) => [[index, bar.start], [index, bar.end]]);
    const [zero, top] = await echarts_points(page, id, horizontal, [[0, 0], [0, peak]]);
    const points = await echarts_points(page, id, horizontal, pairs);
    const along = (value) => (value - zero[1]) / (top[1] - zero[1]);
    const reference = bars.map((bar, index) => {
      const [start, end] = [points[index * 2], points[index * 2 + 1]];
      return { center: start[0], low: Math.min(along(start[1]), along(end[1])), high: Math.max(along(start[1]), along(end[1])) };
    });

    const waterfall = {
      increase_color: colors.increase, decrease_color: colors.decrease,
      subtotal_color: colors.subtotal, total_color: colors.total,
    };
    await aeris_chart(page, {
      width: panel.width, height: panel.height, domain: category_domain(horizontal), axes: category_axes(horizontal),
      series: [{ kind: "waterfall", options: { title: "Profit bridge", color: colors.total, data_labels: true, waterfall }, data: steps }],
    });
    const measured = await page.evaluate(async ({ palette, horizontal, waterfall, levels, rows }) => {
      const { chart, handles, width } = window.__g9b;
      const px = window.__g9b_px;
      const plot = chart.panes()[0].get_geometry();
      const on = px.capture(chart, width);
      const runs = px.runs(on, plot, palette, horizontal ? "y" : "x");
      const edges = runs.map((run) => [run.min, run.max + 1]);
      const zero = horizontal ? Math.min(...edges.map((edge) => edge[0])) : Math.max(...edges.map((edge) => edge[1]));
      const top = horizontal ? Math.max(...edges.map((edge) => edge[1])) : Math.min(...edges.map((edge) => edge[0]));
      // Connector probes: the gap between neighbors at the running-total level of the left bar.
      const probes = runs.slice(0, -1).map((run, index) => {
        const gap = Math.floor((run.to + 1 + runs[index + 1].from) / 2);
        const level = Math.round(zero + levels[index] * (top - zero));
        return [-2, -1, 0, 1].map((offset) => (horizontal ? [level + offset, gap] : [gap, level + offset]));
      });
      const before = probes.map((points) => points.map(([x, y]) => on.at(x, y)));
      // Data labels sit beyond the end of increase, subtotal and total bars.
      const labels = runs.map((run) => (horizontal
        ? px.dark(on, { left: run.max + 2, top: run.from, width: 40, height: run.to - run.from + 1 })
        : px.dark(on, { left: run.from, top: run.min - 16, width: run.to - run.from + 1, height: 15 })));
      handles[0].apply_options({ waterfall: { ...waterfall, connectors: false } });
      chart.render();
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      const off = px.capture(chart, width);
      const connectors = probes.map((points, index) => points.some(([x, y], point) => {
        const [a, b] = [before[index][point], off.at(x, y)];
        return Math.abs(a[0] - b[0]) + Math.abs(a[1] - b[1]) + Math.abs(a[2] - b[2]) > 30;
      }));
      return {
        plot, image: on.image, connectors, labels, zero, top,
        runs: runs.map((run, index) => ({
          center: (run.from + run.to + 1) / 2, color: run.color,
          low: Math.min(...edges[index].map((edge) => (edge - zero) / (top - zero))),
          high: Math.max(...edges[index].map((edge) => (edge - zero) / (top - zero))),
        })),
        details: Array.from({ length: rows }, (_, row) => handles[0].data_at(row)?.detail ?? null),
      };
    }, {
      palette: MARKS.map((mark) => colors[mark]), horizontal, waterfall,
      levels: bars.map((bar) => bar.end / peak), rows: steps.length,
    });
    await save_png(info, `aeris-${id}.png`, measured.image);

    expect(measured.runs, JSON.stringify(measured.plot)).toHaveLength(bars.length);
    const span = (entries) => [entries[0].center, entries.at(-1).center];
    const [aeris_first, aeris_last] = span(measured.runs);
    const [echarts_first, echarts_last] = span(reference);
    for (const [index, bar] of bars.entries()) {
      const actual = measured.runs[index];
      const expected = reference[index];
      expect(MARKS[actual.color], `${bar.x} color`).toBe(bar.mark);
      expect(Math.abs((actual.center - aeris_first) / (aeris_last - aeris_first)
        - (expected.center - echarts_first) / (echarts_last - echarts_first)), `${bar.x} category`).toBeLessThan(0.01);
      expect(Math.abs(actual.low - expected.low), `${bar.x} low`).toBeLessThan(0.01);
      expect(Math.abs(actual.high - expected.high), `${bar.x} high`).toBeLessThan(0.01);
      expect(measured.details[index]).toEqual({ type: "waterfall", mark: bar.mark, start: bar.start, end: bar.end });
      if (bar.mark !== "decrease") expect(measured.labels[index], `${bar.x} label`).toBeGreaterThan(0);
    }
    expect(measured.connectors).toEqual(bars.slice(0, -1).map(() => true));

    if (!horizontal) {
      await page.evaluate(() => {
        const state = window.__g9b;
        state.accessibility = state.api.enable_accessibility(state.chart, { chart_title: "Profit bridge", data_scope: "all" });
      });
      const layer = page.locator("#g9b-host .aeris_charts-a11y-layer").last();
      await layer.focus();
      await page.keyboard.press("Home");
      const live = layer.locator(".aeris_charts-a11y-live-region");
      await expect(live).toContainText("increase 420");
      await page.keyboard.press("End");
      await expect(live).toContainText("total 116");
    }
    await remove_chart(page);
  });
}

test("bullet bands, measure, comparative and target match the ECharts custom bullet in both orientations", async ({ page }, info) => {
  const panel = chart_of(bullet_row, "bullet");
  await echarts_page(page, bullet_row, info, ["bullet"]);
  const values = bullets.flatMap((row, index) => [row.y, row.target, row.comparative, ...row.bands].map((value) => [index, value]));
  const [zero, full] = await echarts_points(page, "bullet", true, [[0, 0], [0, 100]]);
  const points = await echarts_points(page, "bullet", true, values);
  const fraction = (point) => (point[1] - zero[1]) / (full[1] - zero[1]);
  const reference = bullets.map((row, index) => {
    const [measure, target, comparative, ...bands] = points.slice(index * 6, index * 6 + 6).map(fraction);
    return { measure, target, comparative, bands };
  });

  for (const horizontal of [true, false]) {
    await aeris_chart(page, {
      width: panel.width, height: panel.height, domain: category_domain(horizontal),
      axes: category_axes(horizontal, { domain: [0, 100] }),
      series: [{ kind: "bullet", options: { title: "Attainment %", color: measure_color }, data: bullets }],
    });
    const measured = await page.evaluate(({ measure_color, horizontal }) => {
      const { chart, handles, width } = window.__g9b;
      const px = window.__g9b_px;
      const plot = chart.panes()[0].get_geometry();
      const cap = px.capture(chart, width);
      const bottom = plot.top + plot.height;
      const length = Math.floor(horizontal ? plot.width : plot.height);
      // `along` counts pixels from the value-zero side of the plot.
      const pixel = (along, cross) => (horizontal ? cap.at(plot.left + along, cross) : cap.at(cross, bottom - 1 - along));
      const background = (color) => color[0] + color[1] + color[2] >= 750;
      const differs = (a, b) => Math.abs(a[0] - b[0]) + Math.abs(a[1] - b[1]) + Math.abs(a[2] - b[2]) > 9;
      const runs = px.runs(cap, plot, [measure_color], horizontal ? "y" : "x");
      return {
        plot, image: cap.image,
        detail: handles[0].data_at(0)?.detail ?? null,
        bullets: runs.map((run) => {
          const thickness = run.to - run.from + 1;
          const center = (run.from + run.to + 1) / 2;
          const line = (offset) => Array.from({ length }, (_, along) => pixel(along, Math.floor(center + offset)));
          const band_line = line(-1.2 * thickness);
          const first = band_line.findIndex((color) => !background(color));
          const last = band_line.findLastIndex((color) => !background(color)) + 1;
          const edges = [first];
          for (let along = first + 1; along < last; along++) {
            if (differs(band_line[along], band_line[along - 1]) && along - edges.at(-1) > 2) edges.push(along);
          }
          edges.push(last);
          const comparative = line(-0.62 * thickness).findLastIndex((color) => color[2] - color[0] > 40) + 1;
          // The axis line at the zero edge is dark too, so only the plot interior counts.
          const dark = line(0).flatMap((color, along) => (along > first + 2 && along < last && Math.max(...color) < 110 ? [along] : []));
          const target = dark.reduce((sum, along) => sum + along, 0) / dark.length + 0.5;
          const inside = (offset) => Math.abs(offset) < 4 * thickness && !background(pixel(last - 2, Math.floor(center + offset)));
          let band = 0;
          for (let offset = 0; inside(offset); offset--) band += 1;
          for (let offset = 1; inside(offset); offset++) band += 1;
          const measure = horizontal ? run.max + 1 - plot.left : bottom - run.min;
          const scale = (along) => (along - first) / (last - first);
          return {
            first, last, thickness, band,
            measure: scale(measure), target: scale(target), comparative: scale(comparative),
            bands: edges.slice(1).map(scale), measure_share: thickness / band,
          };
        }),
      };
    }, { measure_color, horizontal });
    await save_png(info, `aeris-bullet-${horizontal ? "horizontal" : "vertical"}.png`, measured.image);
    expect(measured.bullets, JSON.stringify(measured.plot)).toHaveLength(bullets.length);
    for (const [index, row] of bullets.entries()) {
      const actual = measured.bullets[index];
      const expected = reference[index];
      const name = `${horizontal ? "horizontal" : "vertical"} ${row.x}`;
      expect(Math.abs(actual.measure - expected.measure), `${name} measure`).toBeLessThan(0.012);
      expect(Math.abs(actual.target - expected.target), `${name} target`).toBeLessThan(0.012);
      expect(Math.abs(actual.comparative - expected.comparative), `${name} comparative`).toBeLessThan(0.012);
      expect(actual.bands, `${name} bands`).toHaveLength(3);
      actual.bands.forEach((band, slot) => expect(Math.abs(band - expected.bands[slot]), `${name} band ${slot}`).toBeLessThan(0.012));
      // Measure bar one third of the band thickness, as in the reference.
      expect(Math.abs(actual.measure_share - 1 / 3), `${name} thickness`).toBeLessThan(0.06);
    }
    expect(measured.detail).toEqual({ type: "bullet", target: 90, comparative: 74, bands: [60, 85, 100] });
    await remove_chart(page);
  }
});

test("lollipop heads and dumbbell ends match the ECharts bar-plus-scatter recipe", async ({ page }, info) => {
  const panel = chart_of(lollipop_row, "lollipop");
  await echarts_page(page, lollipop_row, info, ["lollipop", "dumbbell"]);
  const largest = Math.max(...lollipop.map((row) => row.y));
  const [zero, top] = await echarts_points(page, "lollipop", false, [[0, 0], [0, largest]]);
  const heads = await echarts_points(page, "lollipop", false, lollipop.map((row, index) => [index, row.y]));
  const lollipop_reference = heads.map((point) => (point[1] - zero[1]) / (top[1] - zero[1]));
  const ends = await echarts_points(page, "dumbbell", false, dumbbell.flatMap((row, index) => [[index, row.high], [index, row.low]]));
  const end_top = Math.min(...ends.map((point) => point[1]));
  const end_bottom = Math.max(...ends.map((point) => point[1]));
  const dumbbell_reference = ends.map((point) => (point[1] - end_top) / (end_bottom - end_top));

  const scan = (color, horizontal) => page.evaluate(({ color, horizontal, k }) => {
    const { chart, width, handles } = window.__g9b;
    const px = window.__g9b_px;
    const plot = chart.panes()[0].get_geometry();
    const cap = px.capture(chart, width);
    return {
      plot, image: cap.image, heads: px.blobs(cap, plot, color, k),
      runs: px.runs(cap, plot, [color], horizontal ? "y" : "x", 40),
      detail: handles[0].data_at(0), options: handles[0].options(),
    };
  }, { color, horizontal, k: Math.floor(head_radius / 2) });

  for (const horizontal of [false, true]) {
    const name = horizontal ? "horizontal lollipop" : "lollipop";
    await aeris_chart(page, {
      width: panel.width, height: panel.height, domain: category_domain(horizontal), axes: category_axes(horizontal),
      series: [{
        kind: horizontal ? "horizontal_bar" : "column",
        options: { title: "Net promoter score", color: lollipop_color, bar_mark: "lollipop", point_radius: head_radius },
        data: lollipop,
      }],
    });
    const measured = await scan(lollipop_color, horizontal);
    await save_png(info, `aeris-${name.replace(" ", "-")}.png`, measured.image);
    expect(measured.options.bar_mark).toBe("lollipop");
    expect(measured.heads, `${name} ${JSON.stringify(measured.plot)}`).toHaveLength(lollipop.length);
    // `cross` runs across categories, `along` along the value axis.
    const cross = (point) => (horizontal ? point.y : point.x);
    const along = (point) => (horizontal ? point.x : point.y);
    const ordered = [...measured.heads].sort((a, b) => cross(a) - cross(b));
    const zeros = ordered.map((head) => {
      const run = measured.runs.find((entry) => entry.from <= cross(head) && cross(head) <= entry.to + 1);
      expect(run, `${name} stem`).toBeDefined();
      expect(Math.abs(run.to - run.from + 1 - head_radius * 2), `${name} head width`).toBeLessThanOrEqual(2);
      return along(head) - run.min < run.max + 1 - along(head) ? run.max + 1 : run.min;
    });
    const sorted_zeros = [...zeros].sort((a, b) => a - b);
    const aeris_zero = sorted_zeros[Math.floor(sorted_zeros.length / 2)];
    const peak_head = along(ordered[lollipop.findIndex((row) => row.y === largest)]);
    ordered.forEach((head, index) => {
      const actual = (along(head) - aeris_zero) / (peak_head - aeris_zero);
      expect(Math.abs(actual - lollipop_reference[index]), `${name} ${lollipop[index].x}`).toBeLessThan(0.015);
      expect(Math.abs(zeros[index] - aeris_zero), `${name} ${lollipop[index].x} baseline`).toBeLessThanOrEqual(1);
    });
    expect(measured.detail).toMatchObject({ value: lollipop[0].y, detail: null });
    await remove_chart(page);
  }

  await aeris_chart(page, {
    width: panel.width, height: panel.height, domain: category_domain(false),
    axes: category_axes(false, { domain: [40, 80] }),
    series: [{
      kind: "range_bar",
      options: { title: "Life expectancy", color: dumbbell_color, bar_mark: "dumbbell", point_radius: head_radius },
      data: dumbbell,
    }],
  });
  const measured = await scan(dumbbell_color, false);
  await save_png(info, "aeris-dumbbell.png", measured.image);
  expect(measured.heads, JSON.stringify(measured.plot)).toHaveLength(dumbbell.length * 2);
  const by_x = [...measured.heads].sort((a, b) => a.x - b.x);
  const pairs = dumbbell.map((_, index) => by_x.slice(index * 2, index * 2 + 2).sort((a, b) => a.y - b.y));
  pairs.forEach(([high, low]) => expect(Math.abs(high.x - low.x)).toBeLessThan(1.5));
  const ys = pairs.flat().map((head) => head.y);
  const aeris_top = Math.min(...ys);
  const aeris_bottom = Math.max(...ys);
  ys.forEach((y, index) => {
    const label = `${dumbbell[Math.floor(index / 2)].x} ${index % 2 === 0 ? "high" : "low"}`;
    expect(Math.abs((y - aeris_top) / (aeris_bottom - aeris_top) - dumbbell_reference[index]), label).toBeLessThan(0.015);
  });
  // The connector joins both ends: one run per category spanning both heads.
  expect(measured.runs).toHaveLength(dumbbell.length);
  expect(measured.detail).toMatchObject({ low: 66, high: 76 });
  await remove_chart(page);
});

test("slope end labels never overlap and bump ranks match the ECharts inverted rank axis", async ({ page }, info) => {
  const slope_panel = chart_of(slope_row, "slope");
  const bump_panel = chart_of(slope_row, "bump");
  await echarts_page(page, slope_row, info, ["slope", "bump"]);
  const slope_points = await echarts_points(page, "slope", false, slope.flatMap((line) => line.values.map((value, period) => [period, value])));
  const slope_top = Math.min(...slope_points.map((point) => point[1]));
  const slope_bottom = Math.max(...slope_points.map((point) => point[1]));
  const slope_reference = slope_points.map((point) => (point[1] - slope_top) / (slope_bottom - slope_top));
  const bump_points = await echarts_points(page, "bump", false, ranks.flatMap((line) => line.map((rank, quarter) => [quarter, rank])));
  const [rank_one, rank_last] = await echarts_points(page, "bump", false, [[0, 1], [0, bump.length]]);
  const bump_reference = bump_points.map((point) => ({
    x: (point[0] - bump_points[0][0]) / (bump_points[quarters.length - 1][0] - bump_points[0][0]),
    y: (point[1] - rank_one[1]) / (rank_last[1] - rank_one[1]),
  }));

  const markers = (lines) => page.evaluate(({ lines, k }) => {
    const { chart, width, handles } = window.__g9b;
    const px = window.__g9b_px;
    const plot = chart.panes()[0].get_geometry();
    const cap = px.capture(chart, width);
    const heads = lines.map((line) => px.blobs(cap, plot, line.color, k).sort((a, b) => a.x - b.x));
    const all = heads.flat();
    const left = Math.min(...all.map((head) => head.x));
    const right = Math.max(...all.map((head) => head.x));
    // End labels are drawn in the line color outside the first and last markers.
    const labels = (side) => lines.map((line) => px.extent(cap, side === "start"
      ? { left: 0, top: 0, width: left - 9, height: cap.height }
      : { left: right + 9, top: 0, width: cap.width - right - 9, height: cap.height }, line.color, 48));
    return {
      plot, image: cap.image, heads, start: labels("start"), end: labels("end"),
      details: handles.map((handle) => Array.from({ length: lines[0].values.length }, (_, row) => handle.data_at(row)?.detail ?? null)),
    };
  }, { lines, k: 3 });

  const line_series = (lines, options) => lines.map((line) => ({
    kind: "xy_line",
    options: { title: line.title, color: line.color, point_markers: true, point_radius: 6, ...options },
    data: line.values.map((value, index) => ({ x: (lines === slope ? periods : quarters)[index], y: value })),
  }));

  await aeris_chart(page, {
    width: slope_panel.width, height: slope_panel.height, domain: { type: "category", scale: "point" },
    axes: [
      { id: "x", dimension: "x", scale: "point", band_padding_outer: 1 },
      { id: "y", dimension: "y", scale: "linear", zero_line: false },
    ],
    series: line_series(slope, { end_labels: "both" }),
  });
  const slope_measured = await markers(slope);
  await save_png(info, "aeris-slope.png", slope_measured.image);
  const slope_heads = slope_measured.heads.flat();
  expect(slope_heads, JSON.stringify(slope_measured.plot)).toHaveLength(slope.length * 2);
  const heads_top = Math.min(...slope_heads.map((head) => head.y));
  const heads_bottom = Math.max(...slope_heads.map((head) => head.y));
  slope_heads.forEach((head, index) => {
    const label = `${slope[Math.floor(index / 2)].title} ${periods[index % 2]}`;
    expect(Math.abs((head.y - heads_top) / (heads_bottom - heads_top) - slope_reference[index]), label).toBeLessThan(0.015);
  });
  for (const side of ["start", "end"]) {
    const period = side === "start" ? 0 : 1;
    const boxes = slope_measured[side].map((box, index) => ({ ...box, value: slope[index].values[period], title: slope[index].title }));
    boxes.forEach((box) => expect(box.min, `${side} label ${box.title}`).toBeDefined());
    boxes.sort((a, b) => a.min - b.min);
    // Top-to-bottom order follows the values, and neighbors never share a pixel row.
    expect(boxes.map((box) => box.value)).toEqual([...boxes.map((box) => box.value)].sort((a, b) => b - a));
    for (let index = 1; index < boxes.length; index++) {
      expect(boxes[index].min, `${side} ${boxes[index - 1].title}/${boxes[index].title}`).toBeGreaterThan(boxes[index - 1].max);
    }
  }
  await remove_chart(page);

  await aeris_chart(page, {
    width: bump_panel.width, height: bump_panel.height, domain: { type: "category", scale: "point" },
    axes: [
      { id: "x", dimension: "x", scale: "point", band_padding_outer: 0.3 },
      { id: "y", dimension: "y", scale: "linear", zero_line: false },
    ],
    series: line_series(bump, { rank: "dense" }),
  });
  const bump_measured = await markers(bump);
  await save_png(info, "aeris-bump.png", bump_measured.image);
  bump_measured.heads.forEach((heads, line) => expect(heads, `${bump[line].title} markers`).toHaveLength(quarters.length));
  const at_rank = (rank) => bump_measured.heads.flatMap((heads, line) => heads.filter((_, quarter) => ranks[line][quarter] === rank));
  const mean_y = (heads) => heads.reduce((sum, head) => sum + head.y, 0) / heads.length;
  const aeris_rank_one = mean_y(at_rank(1));
  const aeris_rank_last = mean_y(at_rank(bump.length));
  // Rank 1 is at the top of the rank axis.
  expect(aeris_rank_one).toBeLessThan(aeris_rank_last);
  const first_x = bump_measured.heads[0][0].x;
  const last_x = bump_measured.heads[0][quarters.length - 1].x;
  bump_measured.heads.forEach((heads, line) => heads.forEach((head, quarter) => {
    const expected = bump_reference[line * quarters.length + quarter];
    const label = `${bump[line].title} ${quarters[quarter]}`;
    expect(Math.abs((head.x - first_x) / (last_x - first_x) - expected.x), `${label} x`).toBeLessThan(0.015);
    expect(Math.abs((head.y - aeris_rank_one) / (aeris_rank_last - aeris_rank_one) - expected.y), `${label} rank`).toBeLessThan(0.02);
  }));
  expect(bump_measured.details).toEqual(ranks.map((line) => line.map((rank) => ({ type: "rank", rank }))));

  // Ties: dense 1, 1, 2, 3 against competition 1, 1, 3, 4 at the first period.
  const tied = await page.evaluate(({ lines, quarters }) => {
    const { handles } = window.__g9b;
    const firsts = [40, 40, 30, 20];
    handles.forEach((handle, index) => handle.set_data(lines[index].values.map((value, quarter) => ({
      x: quarters[quarter], y: quarter === 0 ? firsts[index] : value,
    }))));
    const read = () => handles.map((handle) => handle.data_at(0)?.detail?.rank);
    const dense = read();
    handles.forEach((handle) => handle.apply_options({ rank: "competition" }));
    return { dense, competition: read() };
  }, { lines: bump, quarters });
  expect(tied).toEqual({ dense: [1, 1, 2, 3], competition: [1, 1, 3, 4] });
  await page.evaluate(() => {
    const state = window.__g9b;
    state.accessibility = state.api.enable_accessibility(state.chart, { chart_title: "Bump", data_scope: "all" });
  });
  const layer = page.locator("#g9b-host .aeris_charts-a11y-layer").last();
  await layer.focus();
  await page.keyboard.press("Home");
  await expect(layer.locator(".aeris_charts-a11y-live-region")).toContainText("rank 1");
  await remove_chart(page);
});
