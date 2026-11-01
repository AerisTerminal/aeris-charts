import { test, expect } from "@playwright/test";
import gantt_row, { now, task_rows, tasks } from "../echarts_reference/rows/gantt.js";
import calendar_row, { days, probes, week_start } from "../echarts_reference/rows/calendar_heatmap.js";
import marimekko_row, { cells, rects } from "../echarts_reference/rows/marimekko.js";

// G9 timeline, calendar and mosaic families against pinned Apache ECharts 6.1.0. Each pair reads
// ECharts geometry through `convertToPixel` and Aeris geometry through public exact layout hits,
// then compares the normalized positions both libraries derive from the same data.

const panel = (row) => row.charts[0];

async function echarts_page(page, row) {
  await page.goto(`/echarts_reference.html?case=${row.id}`);
  await page.waitForFunction(() => window.__echarts_reference_ready === true);
}

/** Exact-hit bounding boxes by item id for one Aeris layout chart sized like the ECharts panel. */
async function aeris_bounds(page, { kind, data, color, width, height, step, keep_chart = false }) {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  return page.evaluate(async ({ kind, data, color, width, height, step, keep_chart }) => {
    const api = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.id = "g9-host";
    host.style.cssText = `position:fixed;left:0;top:0;width:${width}px;height:${height}px;z-index:10000;background:#fff`;
    document.body.appendChild(host);
    const chart = await api.create_chart(host, {
      backend: "canvas2d", autoSize: false,
      initialPane: { horizontal_domain: { type: "layout" } },
    });
    chart.resize(width, height, 1);
    const series = chart.add_layout_series(kind, { pane: 0, title: "G9", color }, data);
    chart.render();
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const bounds = {};
    for (let y = 0; y < height; y += step) {
      for (let x = 0; x < width; x += step) {
        const hit = chart.layout_hit_test(0, x, y);
        if (hit === null) continue;
        const box = bounds[hit.id] ??= { min_x: x, max_x: x, min_y: y, max_y: y, element: hit.element };
        box.min_x = Math.min(box.min_x, x);
        box.max_x = Math.max(box.max_x, x);
        box.min_y = Math.min(box.min_y, y);
        box.max_y = Math.max(box.max_y, y);
      }
    }
    const snapshot = series.accessibility_snapshot(0, 512);
    const image = chart.take_screenshot().toDataURL("image/png");
    if (keep_chart) {
      window.__g9 = { api, chart, host, series };
    } else {
      chart.remove();
      host.remove();
    }
    return { bounds, snapshot, image };
  }, { kind, data, color, width, height, step, keep_chart });
}

const center = (box) => [(box.min_x + box.max_x) / 2, (box.min_y + box.max_y) / 2];
const save_png = (info, name, data_url) => info.attach(name, {
  body: Buffer.from(data_url.split(",")[1], "base64"), contentType: "image/png",
});

test("Gantt bars, milestones and rows match the pinned ECharts custom-series schedule", async ({ page }, info) => {
  const chart = panel(gantt_row);
  await echarts_page(page, gantt_row);
  const reference = await page.evaluate(({ tasks, task_rows }) => {
    const instance = window.__echarts_instances.gantt;
    const pixel = (time, row) => instance.convertToPixel({ gridIndex: 0 }, [time, row]);
    return tasks.map((task) => ({
      id: task.id,
      start: pixel(task.start, task_rows[task.id])[0],
      end: pixel(task.end, task_rows[task.id])[0],
      y: pixel(task.start, task_rows[task.id])[1],
    }));
  }, { tasks, task_rows });
  await save_png(info, "echarts-gantt.png", `data:image/png;base64,${(await page.locator("[data-chart=gantt]").screenshot()).toString("base64")}`);

  const aeris = await aeris_bounds(page, {
    kind: "gantt", data: { type: "gantt", tasks, now }, color: "#5470c6",
    width: chart.width, height: chart.height, step: 1,
  });
  await save_png(info, "aeris-gantt.png", aeris.image);
  const milestone = (task) => task.start === task.end;
  const measured = tasks.map((task) => {
    const box = aeris.bounds[task.id];
    expect(box, task.id).toBeDefined();
    const [x, y] = center(box);
    return { id: task.id, start: milestone(task) ? x : box.min_x, end: milestone(task) ? x : box.max_x + 1, y };
  });
  const normalize = (entries) => {
    const left = Math.min(...entries.map((entry) => entry.start));
    const right = Math.max(...entries.map((entry) => entry.end));
    const top = entries[0].y;
    const pitch = (entries[1].y - top) / (task_rows[tasks[1].id] - task_rows[tasks[0].id]);
    return entries.map((entry) => ({
      id: entry.id,
      start: (entry.start - left) / (right - left),
      end: (entry.end - left) / (right - left),
      row: Math.round((entry.y - top) / pitch) + task_rows[tasks[0].id],
    }));
  };
  const expected = normalize(reference);
  const actual = normalize(measured);
  for (const [index, entry] of actual.entries()) {
    expect(entry.row, entry.id).toBe(task_rows[entry.id]);
    expect(Math.abs(entry.start - expected[index].start), `${entry.id} start`).toBeLessThan(0.01);
    expect(Math.abs(entry.end - expected[index].end), `${entry.id} end`).toBeLessThan(0.01);
  }
  expect(aeris.snapshot.total).toBe(tasks.length);
  expect(aeris.snapshot.items[2]).toMatchObject({ element: "task", id: "signoff", label: expect.stringContaining("Milestone") });
  expect(aeris.snapshot.items[1].label).toContain("60% complete");
});

test("bounded Gantt rows scroll by wheel and reveal keyboard-focused tasks", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const many = Array.from({ length: 60 }, (_, index) => ({
    id: `t${index}`, label: `Task ${index}`, start: index * 86_400_000, end: (index + 3) * 86_400_000,
    progress: 0.5, dependencies: index > 0 ? [`t${index - 1}`] : [],
  }));
  const before = await page.evaluate(async (tasks) => {
    const api = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.id = "g9-scroll-host";
    host.style.cssText = "position:fixed;left:0;top:0;width:480px;height:260px;z-index:10000;background:#fff";
    document.body.appendChild(host);
    const chart = await api.create_chart(host, {
      backend: "canvas2d", autoSize: false, initialPane: { horizontal_domain: { type: "layout" } },
    });
    chart.resize(480, 260, 1);
    const series = chart.add_layout_series("gantt", { pane: 0, title: "Plan" }, { type: "gantt", tasks });
    const accessibility = api.enable_accessibility(chart);
    chart.render();
    window.__g9_scroll = { chart, host, series, accessibility };
    const ids = new Set();
    for (let y = 0; y < 260; y += 4) for (let x = 0; x < 480; x += 2) {
      const hit = chart.layout_hit_test(0, x, y);
      if (hit !== null) ids.add(hit.id);
    }
    return [...ids];
  }, many);
  expect(before).toContain("t0");
  expect(before.length).toBeLessThan(many.length);
  const visible = async () => page.evaluate(() => {
    const { chart } = window.__g9_scroll;
    const ids = new Set();
    for (let y = 0; y < 260; y += 4) for (let x = 0; x < 480; x += 2) {
      const hit = chart.layout_hit_test(0, x, y);
      if (hit !== null) ids.add(hit.id);
    }
    return [...ids];
  });
  await page.mouse.move(240, 150);
  await page.mouse.wheel(0, 280);
  await expect.poll(visible).not.toContain("t0");
  await page.evaluate(() => window.__g9_scroll.accessibility.focus(0));
  await page.keyboard.press("End");
  await expect(page.locator("#g9-scroll-host .aeris_charts-a11y-live-region")).toContainText("Task 59");
  await expect.poll(visible).toContain("t59");
  await page.keyboard.press("Home");
  await expect.poll(visible).toContain("t0");
  await page.evaluate(() => {
    const { chart, host, accessibility } = window.__g9_scroll;
    accessibility.detach(); chart.remove(); host.remove(); delete window.__g9_scroll;
  });
});

test("calendar heatmap day cells match the pinned ECharts calendar grid", async ({ page }, info) => {
  const chart = panel(calendar_row);
  await echarts_page(page, calendar_row);
  const reference = await page.evaluate((probes) => {
    const instance = window.__echarts_instances.calendar_heatmap;
    const pixel = (date) => instance.convertToPixel({ calendarIndex: 0 }, date);
    const [x0, y0] = pixel("2024-01-01");
    const cell_w = pixel("2024-01-08")[0] - x0;
    const cell_h = pixel("2024-01-02")[1] - y0;
    return Object.fromEntries(probes.map((date) => {
      const [x, y] = pixel(date);
      return [date, [Math.round((x - x0) / cell_w), Math.round((y - y0) / cell_h)]];
    }));
  }, probes);
  await save_png(info, "echarts-calendar.png", `data:image/png;base64,${(await page.locator("[data-chart=calendar_heatmap]").screenshot()).toString("base64")}`);

  const aeris = await aeris_bounds(page, {
    kind: "calendar_heatmap",
    data: { type: "calendar", days, week_start, low_color: "#d8f2e8", high_color: "#3fbe95" },
    color: "#3fbe95", width: chart.width, height: chart.height, step: 1, keep_chart: true,
  });
  await save_png(info, "aeris-calendar.png", aeris.image);
  // Aeris snaps each cell to whole pixels, so a fractional cell pitch cannot be recovered from
  // one week; the column and row of a day are instead its rank among the distinct cell edges.
  const day_boxes = Object.values(aeris.bounds).filter((box) => box.element === "day");
  const edges = (key) => [...new Set(day_boxes.map((box) => box[key]))].sort((a, b) => a - b);
  const columns = edges("min_x");
  const rows = edges("min_y");
  expect(rows).toHaveLength(7);
  const actual = Object.fromEntries(probes.map((date) => {
    const box = aeris.bounds[date];
    expect(box, date).toBeDefined();
    return [date, [columns.indexOf(box.min_x), rows.indexOf(box.min_y)]];
  }));
  expect(actual).toEqual(reference);
  // Days without a value have no hit region, exactly like ECharts cells without data.
  expect(aeris.bounds["2024-01-06"]).toBeUndefined();
  expect(aeris.snapshot).toMatchObject({ total: 366, grid_stride: 7 });
  expect(aeris.snapshot.items[59]).toMatchObject({ element: "day", id: "2024-02-29", label: "29 February 2024" });

  // Keyboard: Right moves one week (one column), Down moves one day (one row).
  await page.evaluate(() => {
    window.__g9.accessibility = window.__g9.api.enable_accessibility(window.__g9.chart);
    window.__g9.accessibility.focus(0);
  });
  const live = page.locator("#g9-host .aeris_charts-a11y-live-region");
  await page.keyboard.press("ArrowRight");
  await expect(live).toContainText("1 January 2024");
  await page.keyboard.press("ArrowRight");
  await expect(live).toContainText("8 January 2024");
  await page.keyboard.press("ArrowDown");
  await expect(live).toContainText("9 January 2024");
  await page.keyboard.press("ArrowLeft");
  await expect(live).toContainText("2 January 2024");
  await page.evaluate(() => {
    const { chart, host, accessibility } = window.__g9;
    accessibility.detach(); chart.remove(); host.remove(); delete window.__g9;
  });
});

test("Marimekko column widths and segment shares match the pinned ECharts custom series", async ({ page }, info) => {
  const chart = panel(marimekko_row);
  await echarts_page(page, marimekko_row);
  const reference = await page.evaluate((rects) => {
    const instance = window.__echarts_instances.marimekko;
    const pixel = (x, y) => instance.convertToPixel({ gridIndex: 0 }, [x, y]);
    return Object.fromEntries(Object.entries(rects).map(([id, [x0, x1, y0, y1]]) => {
      const [left, bottom] = pixel(x0, y0);
      const [right, top] = pixel(x1, y1);
      return [id, { min_x: left, max_x: right, min_y: top, max_y: bottom }];
    }));
  }, rects);
  await save_png(info, "echarts-marimekko.png", `data:image/png;base64,${(await page.locator("[data-chart=marimekko]").screenshot()).toString("base64")}`);

  const aeris = await aeris_bounds(page, {
    kind: "marimekko", data: { type: "mosaic", cells }, color: "#5070dd",
    width: chart.width, height: chart.height, step: 1,
  });
  await save_png(info, "aeris-marimekko.png", aeris.image);
  const shares = (boxes, inclusive) => {
    const pad = inclusive ? 1 : 0;
    const values = Object.values(boxes);
    const left = Math.min(...values.map((box) => box.min_x));
    const right = Math.max(...values.map((box) => box.max_x + pad));
    const top = Math.min(...values.map((box) => box.min_y));
    const bottom = Math.max(...values.map((box) => box.max_y + pad));
    return Object.fromEntries(Object.entries(boxes).map(([id, box]) => [id, {
      x: (box.min_x - left) / (right - left),
      width: (box.max_x + pad - box.min_x) / (right - left),
      height: (box.max_y + pad - box.min_y) / (bottom - top),
    }]));
  };
  const expected = shares(reference, false);
  const actual = shares(aeris.bounds, true);
  expect(Object.keys(actual).sort()).toEqual(Object.keys(expected).sort());
  for (const [id, entry] of Object.entries(actual)) {
    // Aeris strokes a 1px cell border inside the same rectangle, so hits may lose a pixel edge.
    expect(Math.abs(entry.x - expected[id].x), `${id} x`).toBeLessThan(0.01);
    expect(Math.abs(entry.width - expected[id].width), `${id} width`).toBeLessThan(0.01);
    expect(Math.abs(entry.height - expected[id].height), `${id} height`).toBeLessThan(0.02);
  }
  expect(aeris.snapshot.total).toBe(cells.length);
});
