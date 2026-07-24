import { test, expect } from "@playwright/test";
import { PNG } from "pngjs";

// TradingView-style series selection: hovering a series shows the pointer (click affordance)
// cursor; clicking selects it and paints anchor points on its drawn data points — theme-derived
// fill (white on light backgrounds, black on dark) with the accent-blue border.

const BLUE = [41, 98, 255]; // #2962ff — the anchor border

test.beforeEach(async ({ page }) => {
  page.on("pageerror", (error) => console.log(`[browser:pageerror] ${error.message}`));
});

async function wait_for_chart(page) {
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined);
  await page.evaluate(() => new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  }));
}

async function goto_fixture(page) {
  await page.goto("/?runtimeTest=presentedFrame&backend=canvas2d&forceFallbackAdapter=1");
  await wait_for_chart(page);
  // Past the touch-suppression window before driving the pointer (see hit-testing.spec.mjs).
  await page.waitForFunction(() => performance.now() > 600);
}

async function overlay_cursor(page) {
  return page.evaluate(() => {
    const canvases = document.querySelectorAll("#chart_container canvas");
    return canvases[canvases.length - 1].style.cursor;
  });
}

/** A visible bar's center x plus its close y (a certain series hit). */
async function bar_close_spot(page) {
  return page.evaluate(() => {
    const range = window.__chart.time_scale().get_visible_logical_range();
    const index = Math.floor((range.from + range.to) / 2);
    const bar = window.__main.data_by_index(index);
    return { x: window.__chart.time_scale().logical_to_coordinate(index), y: window.__main.price_to_coordinate(bar.close) };
  });
}

/** A pane point above every visible candle's high (plus the hit tolerance): a certain miss. */
async function empty_spot(page) {
  return page.evaluate(() => {
    const range = window.__chart.time_scale().get_visible_logical_range();
    let max_high = -Infinity;
    for (let i = Math.ceil(range.from); i <= Math.floor(range.to); i++) {
      const bar = window.__main.data_by_index(i);
      if (bar) max_high = Math.max(max_high, bar.high);
    }
    const x = window.__chart.time_scale().logical_to_coordinate(Math.floor((range.from + range.to) / 2));
    return { x, y: window.__main.price_to_coordinate(max_high) - 20 };
  });
}

async function capture(page) {
  const data_url = await page.evaluate(() => window.__chart.take_screenshot().toDataURL("image/png"));
  return PNG.sync.read(Buffer.from(data_url.split(",")[1], "base64"));
}

function count_color(png, target, tol = 30) {
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

/** The anchor disc's fill pixel at a bar close (screenshot px == CSS px here). */
function fill_pixel_at(png, x, y) {
  const o = (Math.round(y) * png.width + Math.round(x)) * 4;
  return [png.data[o], png.data[o + 1], png.data[o + 2]];
}

test("series hover shows the pointer cursor; off the geometry it stays crosshair", async ({ page }) => {
  await goto_fixture(page);
  const spot = await bar_close_spot(page);
  await page.mouse.move(spot.x, spot.y);
  expect(await overlay_cursor(page)).toBe("pointer");

  const empty = await empty_spot(page);
  await page.mouse.move(empty.x, empty.y);
  expect(await overlay_cursor(page)).toBe("crosshair");
});

test("click selects a series with theme-derived anchors; empty click deselects", async ({ page }) => {
  await goto_fixture(page);
  const spot = await bar_close_spot(page);
  const before = await capture(page);
  expect(count_color(before, BLUE), "no anchors before selection").toBe(0);

  // Click the bar: accent-blue anchor borders appear, with WHITE fills on the light chart.
  await page.mouse.click(spot.x, spot.y);
  const light = await capture(page);
  expect(count_color(light, BLUE), "anchor borders after click").toBeGreaterThan(50);
  expect(fill_pixel_at(light, spot.x, spot.y), "light-theme anchor fill").toEqual([255, 255, 255]);

  // Dark background: fills track the background luminance to black, blue borders stay.
  await page.evaluate(() => window.__chart.apply_options({ layout: { background: { color: "#0d0d0d" } } }));
  await page.waitForTimeout(100);
  const dark = await capture(page);
  expect(count_color(dark, BLUE), "anchor borders persist on dark").toBeGreaterThan(50);
  const fill = fill_pixel_at(dark, spot.x, spot.y);
  expect(fill[0] < 60 && fill[1] < 60 && fill[2] < 60, `dark-theme anchor fill ${fill}`).toBe(true);

  // Click empty pane space: the selection (and its anchors) clears.
  await page.evaluate(() => window.__chart.apply_options({ layout: { background: { color: "#ffffff" } } }));
  const empty = await empty_spot(page);
  await page.mouse.click(empty.x, empty.y);
  const cleared = await capture(page);
  expect(count_color(cleared, BLUE), "anchors gone after empty click").toBe(0);
});
