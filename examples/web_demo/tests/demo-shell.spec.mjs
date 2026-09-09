import { test, expect } from "@playwright/test";

async function open_demo(page, url = "/") {
  await page.goto(url);
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined && window.__demo_catalogs !== undefined);
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
}

for (const [backend, url] of [
  ["webgpu", "/"],
  ["canvas2d", "/?backend=canvas2d&forceFallbackAdapter=1"],
]) {
  test(`${backend} interactive demo preserves engine defaults except hidden grid`, async ({ page }) => {
    await open_demo(page, url);
    const state = await page.evaluate(async () => {
      const api = await import("../dist/nucleuscharts_financial.js");
      const host = document.createElement("div");
      Object.assign(host.style, { position: "fixed", width: "320px", height: "180px", left: "-10000px", top: "0" });
      document.body.appendChild(host);
      const scratch = await api.create_chart(host, { autoSize: true, backend: "canvas2d" });
      const scratch_series = scratch.add_series("candlestick");
      const expected_chart = structuredClone(scratch.options());
      expected_chart.grid.vertLines.visible = false;
      expected_chart.grid.horzLines.visible = false;
      const result = {
        actual_chart: window.__chart.options(),
        expected_chart,
        actual_series: window.__main.options(),
        expected_series: scratch_series.options(),
      };
      scratch.remove();
      host.remove();
      return result;
    });
    expect(state.actual_chart).toEqual(state.expected_chart);
    expect(state.actual_series).toEqual(state.expected_series);
    expect(state.actual_chart.grid.vertLines).toMatchObject({ visible: false, style: 2 });
    expect(state.actual_chart.grid.horzLines).toMatchObject({ visible: false, style: 2 });
    expect(state.actual_series).toMatchObject({ title: "", countdown_visible: true });
  });
}

test("attribution logo matches the LWC final-pane placement and surface contrast", async ({ page }) => {
  await open_demo(page);
  const result = await page.evaluate(() => {
    const chart = window.__chart;
    chart.apply_options({
      layout: {
        attributionLogo: true,
        background: { type: "solid", color: "#ffffff" },
        textColor: "#141414",
      },
    });
    const read = () => {
      const logo = document.querySelector(".nucleuscharts-attribution-logo");
      if (!(logo instanceof HTMLDivElement)) return null;
      const pane = chart.panes().at(-1).get_geometry();
      const svg = logo.querySelector("svg");
      const path = svg?.querySelector("path") ?? null;
      return {
        left: Number.parseFloat(logo.style.left),
        top: Number.parseFloat(logo.style.top),
        width: logo.getBoundingClientRect().width,
        height: logo.getBoundingClientRect().height,
        pane,
        tone: logo.dataset.logoTone,
        fill: path?.getAttribute("fill") ?? null,
        stroke: path?.getAttribute("stroke") ?? null,
        stroke_width: path?.getAttribute("stroke-width") ?? null,
        vector_effect: path?.getAttribute("vector-effect") ?? null,
      };
    };

    const light = read();
    chart.apply_options({
      layout: {
        background: { type: "solid", color: "#141414" },
        textColor: "#f0f0f0",
      },
    });
    const dark = read();

    chart.add_pane(true);
    const multi_pane = read();
    chart.remove_pane(chart.panes().length - 1);

    chart.apply_options({ layout: { attributionLogo: false } });
    const hidden_count = document.querySelectorAll(".nucleuscharts-attribution-logo").length;
    chart.apply_options({ layout: { attributionLogo: true } });
    const restored_count = document.querySelectorAll(".nucleuscharts-attribution-logo").length;
    return { light, dark, multi_pane, hidden_count, restored_count };
  });

  for (const state of [result.light, result.dark, result.multi_pane]) {
    expect(state).not.toBeNull();
    expect(state.left).toBeCloseTo(state.pane.left + 10, 5);
    expect(state.top + state.height).toBeCloseTo(state.pane.top + state.pane.height - 10, 5);
    expect(state.height).toBeCloseTo(19, 5);
    // Chromium quantizes fractional CSS widths to 1/64 px.
    expect(state.width).toBeCloseTo((221 / 48) * 19, 1);
    expect(state.stroke_width).toBe("1");
    expect(state.vector_effect).toBe("non-scaling-stroke");
  }
  expect(result.light).toMatchObject({ tone: "dark", fill: "#141414", stroke: "#ffffff" });
  expect(result.dark).toMatchObject({ tone: "light", fill: "#F0F0F0", stroke: "#141414" });
  expect(result.multi_pane.tone).toBe("light");
  expect(result.hidden_count).toBe(0);
  expect(result.restored_count).toBe(1);
});

test("demo controls have readable headings and usable desktop and phone targets", async ({ page }) => {
  await open_demo(page);
  await expect(page.locator("#feature_lab_group h2")).toHaveText("Feature lab");
  for (const selector of ["#theme_toggle", "#inspector_toggle", "#feature_search", "#feature_filters button"]) {
    const sizes = await page.locator(selector).evaluateAll((elements) => elements.map((element) => {
      const rect = element.getBoundingClientRect();
      return { width: rect.width, height: rect.height };
    }));
    for (const size of sizes) {
      expect(size.width).toBeGreaterThanOrEqual(40);
      expect(size.height).toBeGreaterThanOrEqual(40);
    }
  }
  await page.setViewportSize({ width: 360, height: 780 });
  await page.locator("#inspector_toggle").click();
  await expect(page.locator("#inspector")).toHaveAttribute("data-open", "true");
  expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false);
  const target = await page.locator("#theme_toggle").boundingBox();
  expect(target.width).toBeGreaterThanOrEqual(44);
  expect(target.height).toBeGreaterThanOrEqual(44);
  expect(await page.locator("#chart_wrap").evaluate((element) => element.inert)).toBe(true);
  await page.keyboard.press("Escape");
  await expect(page.locator("#inspector")).toHaveAttribute("data-open", "false");
  await expect(page.locator("#inspector_toggle")).toBeFocused();
  expect(await page.locator("#chart_wrap").evaluate((element) => element.inert)).toBe(false);
  expect(await page.locator("#inspector").evaluate((element) => element.inert)).toBe(true);
});

test("section navigation leaves the chart in place and reduced motion removes shell transitions", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await open_demo(page);
  const chart = await page.locator("#chart_wrap").boundingBox();
  await page.locator('#tool_rail [data-scroll-target="drawings_group"]').click();
  await expect(page.locator("#control_jump")).toHaveValue("drawings_group");
  await expect(page.locator("#drawings_group h2")).toBeInViewport();
  await page.locator("#control_jump").selectOption("type_group");
  await expect(page.locator('#tool_rail [data-scroll-target="type_group"]')).toHaveAttribute("aria-current", "true");
  await expect(page.locator("#type_group h2")).toBeInViewport();
  expect(await page.locator("#chart_wrap").boundingBox()).toEqual(chart);
  await page.locator("#theme_toggle").click();
  // The shared stylesheet sets a tiny !important duration for reduced motion;
  // the shell disables transition properties entirely, so no animation is created.
  const animations = await page.locator("#theme_toggle [data-theme-icon]").evaluateAll((icons) =>
    icons.map((icon) => icon.getAnimations().length));
  expect(animations).toEqual([0, 0]);
});

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

test("canonical series stay in Series while feature lab contains only composable scenarios", async ({ page }) => {
  await open_demo(page);
  await expect(page.locator("#series_grid .feature-card")).toHaveCount(11);
  await expect(page.locator("#feature_grid .feature-card")).toHaveCount(9);
  expect(await page.locator('input[name="series"]').evaluateAll((radios) => radios.map((radio) => radio.value))).toEqual([
    "candlestick", "hollow_candlestick", "bar", "line", "area", "histogram", "baseline",
  ]);
  await expect(page.locator('#series_grid [data-series-id="footprint"]')).toBeVisible();
  await expect(page.locator('#series_grid [data-series-id="heatmap-standalone"]')).toBeVisible();
  await expect(page.locator('#series_grid [data-series-id="heatmap-line"]')).toBeVisible();
  await expect(page.locator('#feature_grid [data-feature-id="footprint"]')).toHaveCount(0);
  await expect(page.locator('#feature_grid [data-feature-id="rectangle"]')).toHaveCount(0);
  await expect(page.locator('#feature_grid [data-feature-id="accessibility"]')).toHaveCount(0);
  await expect(page.locator('#feature_grid [data-feature-id="volume-profile"]')).toHaveCount(0);
  await expect(page.locator('#feature_grid [data-feature-id="anchored-text"]')).toHaveCount(0);
  await expect(page.locator('#feature_grid [data-feature-id="image-watermark"]')).toHaveCount(0);
  await expect(page.locator('#feature_grid [data-feature-id="partial-price-line"]')).toHaveCount(0);
  await expect(page.locator("#plugin_watermark_toggle")).toHaveCount(0);
  await expect(page.locator("#watermark_group")).toHaveCount(1);
  await expect(page.locator('#drawings_group [data-tool="rectangle"]')).toHaveCount(1);
  await expect(page.locator("#volume_profile_toggle")).toHaveCount(1);

  await page.locator('#series_grid [data-series-id="hlc-area"]').click();
  await expect(page.locator('#series_grid [data-series-id="hlc-area"]')).toHaveAttribute("aria-pressed", "true");
  expect(await page.evaluate(() => window.__demo_catalogs.series.active_id())).toBe("hlc-area");
  expect(await page.evaluate(() => window.__chart.series_order().filter((item) => item.series_type() === "hlc_area").length)).toBe(1);
  expect(await page.evaluate(() => window.__main.options().visible)).toBe(false);

  await page.locator('[data-feature-id="tooltip"]').click();
  expect(await page.evaluate(() => window.__demo_catalogs.lab.active_ids())).toEqual(["tooltip"]);
  expect(await page.evaluate(() => window.__demo_catalogs.series.active_id())).toBe("hlc-area");
  await page.locator("#feature_clear").click();
  expect(await page.evaluate(() => window.__demo_catalogs.lab.active_ids())).toEqual([]);
  expect(await page.evaluate(() => window.__demo_catalogs.series.active_id())).toBe("hlc-area");
  await page.evaluate(() => window.__demo_catalogs.series.clear());
  expect(await page.evaluate(() => window.__chart.series_order().filter((item) => item.series_type() === "hlc_area").length)).toBe(0);
  expect(await page.evaluate(() => window.__main.options().visible)).toBe(true);
});

test("Series launches a readable tick-driven footprint preview", async ({ page }) => {
  await open_demo(page);
  await page.locator('#series_grid [data-series-id="footprint"]').click();
  await expect(page.locator("#candle_style")).toBeHidden();
  const state = await page.evaluate(() => {
    const footprint = window.__chart.series_order().find((series) => series.series_type() === "footprint");
    const bars = footprint?.footprint_bars() ?? [];
    return {
      active: window.__demo_catalogs.series.active_id(),
      visible: footprint?.options().visible,
      bars: bars.length,
      levels: bars[0]?.levels.length ?? 0,
      has_stacked: bars.some((bar) => bar.levels.some(
        (level) => level.stacked_ask_imbalance || level.stacked_bid_imbalance,
      )),
      spacing: window.__chart.time_scale().options().bar_spacing,
      main_visible: window.__main.options().visible,
      footprint_scale_id: footprint?.price_scale_id(),
      main_scale_id: window.__main.price_scale_id(),
      scale_ids: window.__chart.price_scales().map((scale) => scale.id),
    };
  });
  expect(state).toMatchObject({
    active: "footprint",
    visible: true,
    bars: 12,
    levels: 11,
    has_stacked: true,
    spacing: 72,
    main_visible: false,
    footprint_scale_id: "footprint-dedicated",
    main_scale_id: "right",
  });
  expect(state.scale_ids).toContain("footprint-dedicated");
});

test("every feature-lab card activates through its real public API wiring", async ({ page }) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await open_demo(page);
  const features = await page.locator("#feature_grid .feature-card").evaluateAll((cards) =>
    cards.map((card) => ({ id: card.dataset.featureId, kind: card.dataset.featureKind, disabled: card.disabled })),
  );
  for (const feature of features) {
    if (feature.disabled) continue;
    await page.evaluate((id) => window.__demo_catalogs.lab.activate(id), feature.id);
    expect(await page.evaluate((id) => window.__demo_catalogs.lab.active_ids().includes(id), feature.id)).toBe(true);
    if (feature.kind === "primitive") await page.evaluate((id) => window.__demo_catalogs.lab.activate(id), feature.id);
  }
  await page.evaluate(() => window.__demo_catalogs.lab.clear());
  expect(await page.evaluate(() => window.__demo_catalogs.lab.active_ids())).toEqual([]);
  expect(errors).toEqual([]);
});

test("Delta Tooltip is unavailable on candlesticks and becomes available on area series", async ({ page }) => {
  await open_demo(page);
  const delta = page.locator('[data-feature-id="delta-tooltip"]');

  await expect(delta).toBeDisabled();
  await page.evaluate(() => window.__demo_catalogs.lab.activate("delta-tooltip"));
  expect(await page.evaluate(() => window.__demo_catalogs.lab.active_ids())).not.toContain("delta-tooltip");

  await page.locator('input[name="series"][value="area"]').check();
  await expect(delta).toBeEnabled();
  await page.evaluate(() => window.__demo_catalogs.lab.activate("delta-tooltip"));
  expect(await page.evaluate(() => window.__demo_catalogs.lab.active_ids())).toContain("delta-tooltip");

  await page.locator('input[name="series"][value="candlestick"]').check();
  await expect(delta).toBeDisabled();
  await expect.poll(() => page.evaluate(() => window.__demo_catalogs.lab.active_ids())).not.toContain("delta-tooltip");
});

test("hollow candles are a Series choice backed by the canonical candlestick series", async ({ page }) => {
  await open_demo(page);
  await page.locator('input[name="series"][value="hollow_candlestick"]').check();
  expect(await page.evaluate(() => ({
    kind: window.__main.series_type(),
    up: window.__main.options().up_color,
    down: window.__main.options().down_color,
  }))).toEqual({ kind: "candlestick", up: "transparent", down: "transparent" });

  await page.locator('input[name="series"][value="candlestick"]').check();
  expect(await page.evaluate(() => ({
    kind: window.__main.series_type(),
    up: window.__main.options().up_color,
    down: window.__main.options().down_color,
  }))).toEqual({ kind: "candlestick", up: "#089981", down: "#f7525f" });

  await page.locator('input[name="series"][value="histogram"]').check();
  expect(await page.evaluate(() => window.__main.series_type())).toBe("histogram");
});

test("reported plugin scenarios use full data and official line compositions", async ({ page }) => {
  await open_demo(page);
  const inspect = () => page.evaluate(() => window.__chart.series_order().map((series) => ({
    type: series.series_type(),
    points: series.data().length,
    first_cells: series.series_type() === "heatmap" ? series.data()[0]?.cells.length : null,
  })));

  const initial_lines = (await inspect()).filter((item) => item.type === "line").length;
  await page.evaluate(() => window.__demo_catalogs.series.activate("heatmap-standalone"));
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

  await page.evaluate(() => window.__demo_catalogs.series.activate("heatmap-line"));
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

  await page.evaluate(() => window.__demo_catalogs.series.activate("shaded-background"));
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

test("brushable area compares chronological delta in either Shift-drag direction", async ({ page }) => {
  await open_demo(page);
  await page.locator('#series_grid [data-series-id="brushable-area"]').click();
  const targets = await page.evaluate(() => {
    const feature = window.__demo_catalogs.series.interaction_series();
    if (feature.series_type() !== "area") throw new Error("brushable demo must use an ordinary Area series");
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
  const drag = async (from, to, positive) => {
    await page.mouse.move(from.x, from.y);
    await page.keyboard.down("Shift");
    await page.mouse.down();
    await page.mouse.move(to.x, to.y, { steps: 4 });
    await expect.poll(() => page.evaluate(() => {
      return window.__demo_catalogs.series.interaction_range()?.positive ?? null;
    })).toBe(positive);
    await page.mouse.up();
    await page.keyboard.up("Shift");
    await page.mouse.move((from.x + to.x) * 0.5, from.y + 12);
    return page.evaluate(() => {
      const brush = window.__demo_catalogs.series.interaction_range();
      return {
        positive: brush?.positive ?? null,
        range: brush === null ? null : { from: brush.from, to: brush.to },
        active: window.__demo_catalogs.series.active_id(),
        kind: window.__demo_catalogs.series.interaction_series().series_type(),
      };
    });
  };

  const expect_brush = (result, positive) => {
    expect(result).toMatchObject({ positive, active: "brushable-area", kind: "area" });
    expect(result.range.to).toBeGreaterThan(result.range.from);
  };
  expect_brush(await drag(targets.down_from, targets.down_to, false), false);
  expect_brush(await drag(targets.down_to, targets.down_from, false), false);
  expect_brush(await drag(targets.up_to, targets.up_from, true), true);
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
