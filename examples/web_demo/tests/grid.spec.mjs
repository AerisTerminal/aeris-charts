import { test, expect } from "@playwright/test";
import { PNG } from "pngjs";

// Multi-chart split grid in the MAIN demo: the primary chart is the first cell, splits are
// independent, dividers drag, the usage signal meters, the cap enforces, and closes collapse.

async function wait_grid(page) {
  await page.waitForFunction(() => window.__grid !== undefined && window.__chart?.backend?.() !== undefined);
  await page.evaluate(() => new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  }));
}

/** Every cell's chart handle has resolved (a split rebuilds the layout once the chart is in). */
async function wait_cell_charts(page) {
  await page.waitForFunction(() => window.__grid.cells().every((c) => !!c.chart));
  await wait_grid(page);
}

const canvas_count = (page) => page.evaluate(() => document.querySelectorAll("#chart_container canvas").length);

/** Per-cell screenshots as PNGs (public take_screenshot per chart). */
async function cell_shots(page) {
  const urls = await page.evaluate(() =>
    window.__grid.cells().map((c) => c.chart.take_screenshot().toDataURL("image/png")),
  );
  return urls.map((u) => PNG.sync.read(Buffer.from(u.split(",")[1], "base64")));
}

function count_color(png, target, tol = 40) {
  let n = 0;
  for (let o = 0; o < png.data.length; o += 4) {
    if (
      Math.abs(png.data[o] - target[0]) <= tol &&
      Math.abs(png.data[o + 1] - target[1]) <= tol &&
      Math.abs(png.data[o + 2] - target[2]) <= tol
    ) n += 1;
  }
  return n;
}

/** Click a cell's center to make it the active one (toolbar acts on it). */
async function activate_cell(page, index) {
  const point = await page.evaluate((i) => {
    const rect = window.__grid.cells()[i].element.getBoundingClientRect();
    return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
  }, index);
  await page.mouse.click(point.x, point.y);
}

test("main demo splits into independent charts, drags dividers, meters, caps, and collapses", async ({ page }) => {
  await page.goto("/");
  await wait_grid(page);
  expect(await canvas_count(page)).toBe(4); // the primary chart alone: four stacked canvases
  expect((await page.evaluate(() => window.__grid.usage())).chart_count).toBe(1);

  // Split horizontally: two independent charts, both rendering candles.
  await page.click("#split_h");
  await page.waitForFunction(() => document.querySelectorAll("#chart_container canvas").length === 8);
  await wait_cell_charts(page);
  let shots = await cell_shots(page);
  for (const shot of shots) {
    const green = count_color(shot, [38, 166, 154]);
    const red = count_color(shot, [239, 83, 80]);
    expect(green + red, "each cell renders its own candles").toBeGreaterThan(200);
  }

  // The divider is visible and draggable (col-resize), and the drag resizes the cells.
  const divider = page.locator(".nucleuscharts-grid-divider >> nth=0");
  await expect(divider).toHaveCSS("cursor", "col-resize");
  const widths = () => page.evaluate(() => window.__grid.cells().map((c) => c.element.getBoundingClientRect().width));
  const before = await widths();
  const box = await divider.boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + 120, box.y + box.height / 2, { steps: 5 });
  await page.mouse.up();
  const after = await widths();
  expect(after[0], "dragging grows the left cell").toBeGreaterThan(before[0] + 60);
  expect(after[1], "and shrinks the right cell").toBeLessThan(before[1] - 60);

  // Independence: the primary cell (the main demo chart) recolors alone.
  await page.evaluate(() => window.__main.apply_options({ up_color: "#0000ff" }));
  shots = await cell_shots(page);
  expect(count_color(shots[0], [0, 0, 255], 40), "primary cell recolored").toBeGreaterThan(50);
  expect(count_color(shots[1], [0, 0, 255], 40), "second cell untouched").toBe(0);

  // Recursive split (vertical on the second cell) → three charts, row-resize divider inside.
  await activate_cell(page, 1);
  await page.click("#split_v");
  await page.waitForFunction(() => document.querySelectorAll("#chart_container canvas").length === 12);
  await wait_cell_charts(page);
  await expect(page.locator(".nucleuscharts-grid-divider >> nth=1")).toHaveCSS("cursor", "row-resize");
  let usage = await page.evaluate(() => window.__grid.usage());
  expect(usage.chart_count).toBe(3);
  expect(usage.split_count).toBe(2);
  expect(usage.cells.length).toBe(3);

  // Paywall cap: at max 3 the next split is rejected, the grid unchanged.
  await page.selectOption("#max_charts", "3");
  await activate_cell(page, 0);
  await page.click("#split_h");
  await page.waitForTimeout(400);
  expect(await canvas_count(page)).toBe(12);
  expect((await page.evaluate(() => window.__grid.usage())).chart_count).toBe(3);

  // Close the active (non-primary) cell: the sibling absorbs the space.
  await activate_cell(page, 1);
  await page.click("#close_cell");
  await page.waitForFunction(() => document.querySelectorAll("#chart_container canvas").length === 8);
  usage = await page.evaluate(() => window.__grid.usage());
  expect(usage.chart_count).toBe(2);

  // The primary cell cannot be closed from the toolbar (all wiring lives on it).
  await activate_cell(page, 0);
  await page.click("#close_cell");
  await page.waitForTimeout(300);
  expect(await canvas_count(page), "primary cell refuses to close").toBe(8);

  // …but closing the remaining secondary cell returns to the single primary chart.
  await activate_cell(page, 1);
  await page.click("#close_cell");
  await page.waitForFunction(() => document.querySelectorAll("#chart_container canvas").length === 4);
  expect((await page.evaluate(() => window.__grid.usage())).chart_count).toBe(1);
});

test("the toolbar's series type and style act on the ACTIVE cell, not always the primary", async ({ page }) => {
  await page.goto("/");
  await wait_grid(page);
  await page.click("#split_h");
  await page.waitForFunction(() => document.querySelectorAll("#chart_container canvas").length === 8);
  await wait_cell_charts(page);
  const type_of = (i) => page.evaluate((idx) => window.__grid.cells()[idx].chart.__seed_series.series_type(), i);

  // Activate the second chart and switch it to area: only it changes.
  await activate_cell(page, 1);
  await expect(page.locator('input[name="series"][value="candlestick"]')).toBeChecked();
  await page.locator('input[name="series"][value="area"]').check();
  expect(await type_of(1), "active cell switched to area").toBe("area");
  expect(await type_of(0), "primary cell untouched").toBe("candlestick");
  // The area fill style group appears for the area-typed active chart.
  await expect(page.locator("#area_style")).toBeVisible();

  // The toolbar tracks the active cell: back on the primary, its type is selected again and
  // changes land there instead.
  await activate_cell(page, 0);
  await expect(page.locator('input[name="series"][value="candlestick"]')).toBeChecked();
  await expect(page.locator("#area_style")).toBeHidden();
  await page.locator('input[name="series"][value="line"]').check();
  expect(await type_of(0), "primary switched to line").toBe("line");
  expect(await type_of(1), "second cell keeps its area type").toBe("area");

  // Style options follow the active cell too: recolor the primary's line only.
  await page.evaluate(() => {
    const el = document.getElementById("line_color");
    el.value = "#ff00ff";
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
  const color_of = (i) => page.evaluate((idx) => window.__grid.cells()[idx].chart.__seed_series.options().color, i);
  expect(await color_of(0)).toBe("#ff00ff");
  expect(await color_of(1)).not.toBe("#ff00ff");
});

test("divider drags never disturb a cell's candle spacing (even with interactions off)", async ({ page }) => {
  await page.goto("/");
  await wait_grid(page);
  await page.click("#split_h");
  await page.waitForFunction(() => document.querySelectorAll("#chart_container canvas").length === 8);
  await wait_grid(page);

  // Dashboard-style embeds: all scroll/scale gestures off. The engine's all-interactions-off
  // aggregate feeds label alignment only (reference time-scale.ts:975-986) — it must not force
  // fix-edge semantics, so a resize leaves bar spacing and the right range edge untouched.
  await page.evaluate(() => {
    for (const cell of window.__grid.cells()) {
      cell.chart.apply_options({ handle_scroll: false, handle_scale: false });
      cell.chart.time_scale().fit_content();
    }
  });
  const probe = (i) =>
    page.evaluate((idx) => {
      const chart = window.__grid.cells()[idx].chart;
      const r = chart.wasm.visible_logical_range();
      return { spacing: chart.wasm.bar_spacing(), right: r.length === 2 ? r[1] : null };
    }, i);
  const left_before = await probe(0);
  const right_before = await probe(1);

  const divider = page.locator(".nucleuscharts-grid-divider >> nth=0");
  const box = await divider.boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 - 120, box.y + box.height / 2, { steps: 8 });
  await page.mouse.up();
  await wait_grid(page);

  const left_after = await probe(0);
  const right_after = await probe(1);
  expect(right_after.spacing, "growing cell keeps its bar spacing").toBe(right_before.spacing);
  expect(right_after.right, "growing cell keeps its right range edge").toBe(right_before.right);
  expect(left_after.spacing, "shrinking cell keeps its bar spacing").toBe(left_before.spacing);
  expect(left_after.right, "shrinking cell keeps its right range edge").toBe(left_before.right);
});

test("the divider keeps its 1px line under a global border-box reset", async ({ page }) => {
  await page.goto("/");
  await wait_grid(page);
  // Platform pages commonly reset `box-sizing: border-box` globally (even `!important`); the
  // divider paints its line as a gradient stop across a fixed 5px box, so no content box can
  // collapse under it.
  await page.addStyleTag({ content: "*, *::before, *::after { box-sizing: border-box !important; }" });
  await page.click("#split_h");
  await page.waitForFunction(() => document.querySelectorAll("#chart_container canvas").length === 8);
  await wait_cell_charts(page);
  const divider = page.locator(".nucleuscharts-grid-divider >> nth=0");
  const box = await divider.boundingBox();
  expect(box.width).toBeGreaterThanOrEqual(5); // the full hit area

  // The 1px line actually paints: screenshot the divider strip and count border-color pixels.
  const png = PNG.sync.read(await page.screenshot({ clip: box }));
  const border_hex = await page.evaluate(() => window.__chart.options().rightPriceScale.borderColor);
  const target = [1, 3, 5].map((i) => parseInt(border_hex.slice(i, i + 2), 16));
  expect(count_color(png, target, 10), "the line paints under the reset").toBeGreaterThan(100);

  // The drag hit area survives too.
  const widths = () => page.evaluate(() => window.__grid.cells().map((c) => c.element.getBoundingClientRect().width));
  const before = await widths();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + 100, box.y + box.height / 2, { steps: 5 });
  await page.mouse.up();
  const after = await widths();
  expect(after[0], "drag still resizes under the reset").toBeGreaterThan(before[0] + 40);
});

test("split dividers follow the axis border token (theme and explicit changes)", async ({ page }) => {
  await page.goto("/");
  await wait_grid(page);
  await page.click("#split_h");
  await page.waitForFunction(() => document.querySelectorAll("#chart_container canvas").length === 8);
  await wait_cell_charts(page);

  const divider_rgb = () =>
    page.locator(".nucleuscharts-grid-divider >> nth=0").evaluate((el) => {
      // The line color is the solid inner strip's background.
      const inner = el.firstElementChild;
      return inner ? getComputedStyle(inner).backgroundColor : "";
    });
  const border_hex = () => page.evaluate(() => window.__chart.options().rightPriceScale.borderColor);
  const to_rgb = (hex) => `rgb(${[1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16)).join(", ")})`;

  // Default: the divider paints in the axis border color (light theme border).
  expect(await divider_rgb()).toBe(to_rgb(await border_hex()));

  // Theme switch: the border token changes and the divider tracks it.
  await page.selectOption("#theme_select", "dark");
  await wait_grid(page);
  const dark_border = await border_hex();
  expect(dark_border.toLowerCase()).toBe("#16191f");
  expect(await divider_rgb()).toBe(to_rgb(dark_border));

  // An explicit axis border change re-resolves the divider too.
  await page.locator("#axis_border_color").evaluate((el) => {
    el.value = "#ff8800";
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await wait_grid(page);
  expect(await divider_rgb()).toBe("rgb(255, 136, 0)");
});

test("Ctrl+H/V split the active cell; typing fields and the cap still hold", async ({ page }) => {
  await page.goto("/");
  await wait_grid(page);
  expect(await canvas_count(page)).toBe(4);

  // Ctrl+H splits the (initially active) primary cell horizontally.
  await page.keyboard.press("Control+h");
  await page.waitForFunction(() => document.querySelectorAll("#chart_container canvas").length === 8);
  await wait_cell_charts(page);
  expect((await page.evaluate(() => window.__grid.usage())).chart_count).toBe(2);

  // Ctrl+V splits the SAME active cell again (still the primary) vertically.
  await page.keyboard.press("Control+v");
  await page.waitForFunction(() => document.querySelectorAll("#chart_container canvas").length === 12);
  await wait_cell_charts(page);
  expect((await page.evaluate(() => window.__grid.usage())).chart_count).toBe(3);

  // Pressing another cell retargets the shortcut: it splits, the primary stays put.
  await activate_cell(page, 2);
  await page.keyboard.press("Control+h");
  await page.waitForFunction(() => document.querySelectorAll("#chart_container canvas").length === 16);
  await wait_cell_charts(page);
  expect((await page.evaluate(() => window.__grid.usage())).chart_count).toBe(4);

  // Typing in an input does NOT split.
  await page.locator("#drawing_text").click();
  await page.keyboard.press("Control+v");
  await page.waitForTimeout(300);
  expect(await canvas_count(page)).toBe(16);

  // The paywall cap still gates the shortcut (max 4): the next Ctrl+H is a no-op.
  await page.selectOption("#max_charts", "4");
  await page.keyboard.press("Control+h");
  await page.waitForTimeout(300);
  expect(await canvas_count(page)).toBe(16);
  expect((await page.evaluate(() => window.__grid.usage())).chart_count).toBe(4);
});

test("shortcut splits come up seeded (the on_cell_added hook fires)", async ({ page }) => {
  await page.goto("/");
  await wait_grid(page);
  await page.keyboard.press("Control+h");
  await page.waitForFunction(() => document.querySelectorAll("#chart_container canvas").length === 8);
  await wait_cell_charts(page);
  // Both cells render candles � the shortcut path runs the same seeding hook as the button.
  const shots = await cell_shots(page);
  for (const shot of shots) {
    const green = count_color(shot, [38, 166, 154]);
    const red = count_color(shot, [239, 83, 80]);
    expect(green + red, "shortcut-created cell renders its seeded candles").toBeGreaterThan(200);
  }
});

test("the shortcut registry accepts combo overrides", async ({ page }) => {
  await page.goto("/");
  await wait_grid(page);
  // A scratch grid with the vertical split re-keyed: ctrl+shift+x, default ctrl+v disabled.
  await page.evaluate(async () => {
    const mod = await import("/dist/nucleuscharts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:640px;height:400px;z-index:50;background:white;";
    host.id = "scratch_grid";
    document.body.appendChild(host);
    window.__scratch = await mod.create_chart_grid(host, {
      shortcuts: { "grid.split_vertical": "ctrl+shift+x" },
      on_cell_added: (cell) => {
        const s = cell.chart.add_series("line");
        s.set_data([{ time: 1, value: 1 }, { time: 2, value: 2 }]);
      },
    });
  });
  // The overridden combo splits; the default combo does not.
  await page.keyboard.press("Control+Shift+x");
  await page.waitForFunction(() => window.__scratch.chart_count() === 2);
  await page.keyboard.press("Control+v");
  await page.waitForTimeout(300);
  expect(await page.evaluate(() => window.__scratch.chart_count())).toBe(2);
  await page.evaluate(() => { window.__scratch.destroy(); document.getElementById("scratch_grid").remove(); });
});

test("Ctrl+click maximizes a cell to full container and restores", async ({ page }) => {
  await page.goto("/");
  await wait_grid(page);
  await page.click("#split_h");
  await page.waitForFunction(() => document.querySelectorAll("#chart_container canvas").length === 8);
  await wait_cell_charts(page);

  const widths = () => page.evaluate(() => window.__grid.cells().map((c) => {
    const r = c.element.getBoundingClientRect();
    // Hidden = detached from the container (the grid mounts only the maximized slot; a
    // detached slot never collapses its chart through a 0-size resize).
    return { attached: c.element.isConnected, width: r.width };
  }));
  const before = await widths();
  expect(before[1].width).toBeGreaterThan(100);

  // Ctrl+click the second cell: it takes the container, the first cell and dividers hide.
  const point = await page.evaluate(() => {
    const rect = window.__grid.cells()[1].element.getBoundingClientRect();
    return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
  });
  await page.keyboard.down("Control");
  await page.mouse.click(point.x, point.y);
  await page.keyboard.up("Control");
  const maximized = await widths();
  expect(maximized[0].attached).toBe(false);
  expect(maximized[1].attached).toBe(true);
  expect(maximized[1].width).toBeGreaterThan(before[1].width + before[0].width - 20);
  expect(await page.evaluate(() => window.__grid.maximized_cell()?.id ?? null)).toBe(2);

  // Ctrl+click again: everything reappears.
  await page.keyboard.down("Control");
  await page.mouse.click(point.x, point.y);
  await page.keyboard.up("Control");
  const restored = await widths();
  expect(restored[0].attached).toBe(true);
  expect(restored[1].attached).toBe(true);
  expect(restored[1].width).toBeLessThan(restored[0].width + 20);
  expect(await page.evaluate(() => window.__grid.maximized_cell())).toBeNull();
});
