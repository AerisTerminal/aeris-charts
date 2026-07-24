import { test, expect } from "@playwright/test";
import { PNG } from "pngjs";

// The demo's RSI(14) toggle exercises the separate-pane API end-to-end: move_to_pane stacks a
// dedicated pane with its own price scale and a draggable separator, cross-pane hover/click
// affordances work there, and removing the series prunes the empty pane.

const PURPLE = [171, 71, 188]; // #ab47bc — the demo's RSI stroke
const BLUE = [41, 98, 255]; // #2962ff — selection anchor border

async function wait_for_chart(page) {
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined);
  await page.evaluate(() => new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  }));
}

async function capture(page) {
  const data_url = await page.evaluate(() => window.__chart.take_screenshot().toDataURL("image/png"));
  return PNG.sync.read(Buffer.from(data_url.split(",")[1], "base64"));
}

function count_color_below(png, target, y_min, tol = 30) {
  let n = 0;
  for (let y = Math.ceil(y_min); y < png.height; y++) {
    for (let x = 0; x < png.width; x++) {
      const o = (y * png.width + x) * 4;
      if (
        Math.abs(png.data[o] - target[0]) <= tol &&
        Math.abs(png.data[o + 1] - target[1]) <= tol &&
        Math.abs(png.data[o + 2] - target[2]) <= tol
      ) n += 1;
    }
  }
  return n;
}

async function overlay_cursor(page) {
  return page.evaluate(() => {
    const canvases = document.querySelectorAll("#chart_container canvas");
    return canvases[canvases.length - 1].style.cursor;
  });
}

test("RSI toggle stacks a separate pane with its own scale; unchecking prunes it", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await wait_for_chart(page);
  await page.waitForFunction(() => performance.now() > 600); // touch-suppression window
  expect(await page.evaluate(() => window.__chart.panes().length)).toBe(1);

  await page.check("#rsi_toggle");
  await wait_for_chart(page);
  expect(await page.evaluate(() => window.__chart.panes().length)).toBe(2);
  const sep = await page.evaluate(() => Array.from(window.__chart.wasm.pane_separator_ys()));
  expect(sep.length).toBe(1);

  // The oscillator's own scale maps mid-range RSI into pane 1 (below the separator)…
  const rsi_y = await page.evaluate(() => window.__rsi.price_to_coordinate(50));
  expect(rsi_y).toBeGreaterThan(sep[0]);
  // …and the purple stroke actually paints there (with the 70/30 guide lines).
  const shot = await capture(page);
  expect(count_color_below(shot, PURPLE, sep[0] + 2)).toBeGreaterThan(50);

  // Cross-pane affordances: hovering the RSI line shows the pointer cursor…
  // (page.mouse works in page coords — offset the chart-relative point by the container rect).
  const spot = await page.evaluate(() => {
    const range = window.__chart.time_scale().get_visible_logical_range();
    const index = Math.floor((range.from + range.to) / 2);
    const point = window.__rsi.data_by_index(index);
    const rect = document.querySelector("#chart_container").getBoundingClientRect();
    return {
      x: rect.left + window.__chart.time_scale().logical_to_coordinate(index),
      y: rect.top + window.__rsi.price_to_coordinate(point.value),
    };
  });
  await page.mouse.move(spot.x, spot.y);
  expect(await overlay_cursor(page)).toBe("pointer");
  // …and clicking selects it: accent-blue anchors paint in pane 1.
  await page.mouse.click(spot.x, spot.y);
  const selected = await capture(page);
  expect(count_color_below(selected, BLUE, sep[0] + 2)).toBeGreaterThan(20);

  // Uncheck: the series is removed and its empty pane pruned.
  await page.uncheck("#rsi_toggle");
  await wait_for_chart(page);
  expect(await page.evaluate(() => window.__chart.panes().length)).toBe(1);
});
