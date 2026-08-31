import { test, expect } from "@playwright/test";

async function open_demo(page) {
  await page.goto("/");
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined && window.__feature_lab !== undefined);
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
}

test("demo shell is responsive, icon-led, and has no horizontal control ribbon", async ({ page }) => {
  await open_demo(page);
  const layout = await page.evaluate(() => ({
    page_overflow: document.documentElement.scrollWidth > document.documentElement.clientWidth,
    controls_overflow: document.getElementById("controls_strip").scrollWidth > document.getElementById("controls_strip").clientWidth,
    chart_width: document.getElementById("chart_wrap").getBoundingClientRect().width,
    inspector_width: document.getElementById("inspector").getBoundingClientRect().width,
    icons: document.querySelectorAll("[data-icon] svg").length,
  }));
  expect(layout.page_overflow).toBe(false);
  expect(layout.controls_overflow).toBe(false);
  expect(layout.chart_width).toBeGreaterThan(700);
  expect(layout.inspector_width).toBeGreaterThanOrEqual(300);
  expect(layout.icons).toBeGreaterThan(20);
  await expect(page.locator("#theme_toggle")).toBeVisible();
  await expect(page.locator("#inspector_toggle")).toBeVisible();
});

test("feature lab exposes every first-class helper and manages series lifecycle", async ({ page }) => {
  await open_demo(page);
  // Every supported helper scenario remains represented after retiring three obsolete cards.
  await expect(page.locator("#feature_grid .feature-card")).toHaveCount(26);
  await expect(page.locator('[data-feature-id="footprint"]')).toBeVisible();
  await expect(page.locator('[data-feature-id="heatmap-standalone"]')).toBeVisible();
  await expect(page.locator('[data-feature-id="heatmap-line"]')).toBeVisible();

  await page.locator('[data-feature-id="hlc-area"]').click();
  await expect(page.locator('[data-feature-id="hlc-area"]')).toHaveAttribute("aria-pressed", "true");
  expect(await page.evaluate(() => window.__feature_lab.active_ids())).toEqual(["hlc-area"]);
  expect(await page.evaluate(() => window.__chart.series_order().filter((item) => item.series_type() === "hlc_area").length)).toBe(1);
  expect(await page.evaluate(() => window.__main.options().visible)).toBe(false);

  await page.locator('[data-feature-id="volume-profile"]').click();
  expect(await page.evaluate(() => window.__feature_lab.active_ids().sort())).toEqual(["hlc-area", "volume-profile"]);
  await page.locator("#feature_clear").click();
  expect(await page.evaluate(() => window.__feature_lab.active_ids())).toEqual([]);
  expect(await page.evaluate(() => window.__chart.series_order().filter((item) => item.series_type() === "hlc_area").length)).toBe(0);
  expect(await page.evaluate(() => window.__main.options().visible)).toBe(true);
});

test("feature lab launches a readable tick-driven footprint preview", async ({ page }) => {
  await open_demo(page);
  await page.locator('[data-feature-id="footprint"]').click();
  const state = await page.evaluate(() => {
    const footprint = window.__chart.series_order().find((series) => series.series_type() === "footprint");
    const bars = footprint?.footprint_bars() ?? [];
    return {
      active: window.__feature_lab.active_ids(),
      visible: footprint?.options().visible,
      bars: bars.length,
      levels: bars[0]?.levels.length ?? 0,
      has_stacked: bars.some((bar) => bar.levels.some(
        (level) => level.stacked_ask_imbalance || level.stacked_bid_imbalance,
      )),
      spacing: window.__chart.time_scale().options().bar_spacing,
      main_visible: window.__main.options().visible,
    };
  });
  expect(state).toMatchObject({
    active: ["footprint"],
    visible: true,
    bars: 8,
    levels: 5,
    has_stacked: true,
    spacing: 96,
    main_visible: false,
  });
});

test("every feature-lab card activates through its real public API wiring", async ({ page }) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await open_demo(page);
  const features = await page.locator("#feature_grid .feature-card").evaluateAll((cards) =>
    cards.map((card) => ({ id: card.dataset.featureId, kind: card.dataset.featureKind })),
  );
  for (const feature of features) {
    await page.evaluate((id) => window.__feature_lab.activate(id), feature.id);
    expect(await page.evaluate((id) => window.__feature_lab.active_ids().includes(id), feature.id)).toBe(true);
    if (feature.kind === "primitive") await page.evaluate((id) => window.__feature_lab.activate(id), feature.id);
  }
  await page.evaluate(() => window.__feature_lab.clear());
  expect(await page.evaluate(() => window.__feature_lab.active_ids())).toEqual([]);
  expect(errors).toEqual([]);
});

test("reported plugin scenarios use full data and official line compositions", async ({ page }) => {
  await open_demo(page);
  const inspect = () => page.evaluate(() => window.__chart.series_order().map((series) => ({
    type: series.series_type(),
    points: series.data().length,
    first_cells: series.series_type() === "heatmap" ? series.data()[0]?.cells.length : null,
  })));

  const initial_lines = (await inspect()).filter((item) => item.type === "line").length;
  await page.evaluate(() => window.__feature_lab.activate("heatmap-standalone"));
  let series = await inspect();
  expect(series.find((item) => item.type === "heatmap")).toMatchObject({
    points: await page.evaluate(() => window.__data.length),
    first_cells: 10,
  });
  expect(series.filter((item) => item.type === "line")).toHaveLength(initial_lines);
  expect(await page.evaluate(() => window.__chart.time_scale().options())).toMatchObject({
    min_bar_spacing: 4,
    bar_spacing: 21,
  });

  await page.evaluate(() => window.__feature_lab.activate("heatmap-line"));
  series = await inspect();
  expect(series.find((item) => item.type === "heatmap")).toMatchObject({
    points: await page.evaluate(() => window.__data.length),
    first_cells: 13,
  });
  expect(series.filter((item) => item.type === "line")).toHaveLength(initial_lines + 1);
  expect(await page.evaluate(async () => {
    const { default_theme_name, theme_palette } = await import("/dist/nucleuscharts_financial.js");
    return window.__chart.series_order().at(-1).options().color === theme_palette(default_theme_name).primary;
  })).toBe(true);

  await page.evaluate(() => window.__feature_lab.activate("shaded-background"));
  series = await inspect();
  expect(series.find((item) => item.type === "background_shade")?.points).toBe(await page.evaluate(() => window.__data.length));
  expect(series.filter((item) => item.type === "line")).toHaveLength(initial_lines + 1);
  const shade_state = await page.evaluate(() => {
    const shade = window.__chart.series_order().find((item) => item.series_type() === "background_shade");
    const line = window.__chart.series_order().at(-1);
    const values = shade.data().map((point) => point.value);
    return {
      shade_options: shade.options(),
      line_options: line.options(),
      same_data: JSON.stringify(shade.data()) === JSON.stringify(line.data()),
      value_range: [Math.min(...values), Math.max(...values)],
    };
  });
  const primary = await page.evaluate(async () => {
    const { default_theme_name, theme_palette } = await import("/dist/nucleuscharts_financial.js");
    return theme_palette(default_theme_name).primary;
  });
  expect(shade_state).toMatchObject({
    shade_options: { low_value: 0, high_value: 1000 },
    line_options: { color: primary, line_width: 3, price_line_visible: true },
    same_data: true,
  });
  expect(shade_state.value_range[0]).toBeGreaterThan(150);
  expect(shade_state.value_range[1]).toBeLessThan(900);
});

test("brushable area writes a logical range whose color follows chronological delta in either drag direction", async ({ page }) => {
  await open_demo(page);
  await page.locator('[data-feature-id="brushable-area"]').click();
  const targets = await page.evaluate(() => {
    const feature = window.__chart.series_order().find((item) => item.series_type() === "brushable_area");
    const data = feature.data();
    const pane = window.__chart.panes()[0].get_geometry();
    const visible = window.__chart.time_scale().get_visible_logical_range();
    const pair = (descending) => {
      for (let gap = 20; gap < Math.min(160, data.length); gap += 10) {
        for (let index = Math.max(gap, Math.ceil(visible.from) + gap);
          index <= Math.min(data.length - 1, Math.floor(visible.to)); index += 1) {
          const first_index = index - gap;
          const first = data[first_index];
          const second = data[index];
          const x1 = window.__chart.time_scale().logical_to_coordinate(first_index);
          const x2 = window.__chart.time_scale().logical_to_coordinate(index);
          if (x1 !== null && x2 !== null && Math.abs(x2 - x1) >= 24
            && x1 >= 0 && x1 <= pane.width && x2 >= 0 && x2 <= pane.width
            && ((second.value < first.value) === descending)) {
            return [first_index, index];
          }
        }
      }
      throw new Error(`No ${descending ? "descending" : "ascending"} pair in brush data`);
    };
    const canvases = window.__chart.chart_element().querySelectorAll("canvas");
    const bounds = canvases[canvases.length - 1].getBoundingClientRect();
    const point = (index) => ({
      x: bounds.left + pane.left + window.__chart.time_scale().logical_to_coordinate(index),
      y: bounds.top + pane.top + pane.height * 0.5,
    });
    const [down_from, down_to] = pair(true);
    const [up_from, up_to] = pair(false);
    return {
      down_from: point(down_from),
      down_to: point(down_to),
      up_from: point(up_from),
      up_to: point(up_to),
    };
  });
  const drag = async (from, to, color) => {
    await page.mouse.move(from.x, from.y);
    await page.mouse.down();
    await page.mouse.move(to.x, to.y, { steps: 4 });
    await expect.poll(() => page.evaluate(() => {
      const feature = window.__chart.series_order().find((item) => item.series_type() === "brushable_area");
      return feature.options().brush_ranges[0]?.style.line_color ?? null;
    })).toBe(color);
    await page.mouse.up();
    await page.mouse.move((from.x + to.x) * 0.5, from.y + 12);
    return page.evaluate(() => {
      const feature = window.__chart.series_order().find((item) => item.series_type() === "brushable_area");
      const brush = feature.options().brush_ranges[0];
      return {
        color: brush?.style.line_color ?? null,
        range: brush?.range ?? null,
        active: window.__feature_lab.active_ids(),
      };
    });
  };

  const expect_brush = (result, color) => {
    expect(result).toMatchObject({ color, active: expect.arrayContaining(["brushable-area"]) });
    expect(result.range.to).toBeGreaterThan(result.range.from);
  };
  expect_brush(await drag(targets.down_from, targets.down_to, "#ef5350"), "#ef5350");
  expect_brush(await drag(targets.down_to, targets.down_from, "#ef5350"), "#ef5350");
  expect_brush(await drag(targets.up_to, targets.up_from, "#049981"), "#049981");
});

test("theme action and compact inspector remain directly usable", async ({ page }) => {
  await page.setViewportSize({ width: 760, height: 820 });
  await open_demo(page);
  const initial = await page.locator("html").getAttribute("data-theme");
  await page.locator("#theme_toggle").click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", initial === "light" ? "dark" : "light");
  await expect(page.locator("#theme_select")).toHaveValue(initial === "light" ? "dark" : "light");

  await expect(page.locator("#inspector")).not.toHaveAttribute("data-open", "true");
  await page.locator("#inspector_toggle").click();
  await expect(page.locator("#inspector")).toHaveAttribute("data-open", "true");
  expect(await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth)).toBe(false);
});
