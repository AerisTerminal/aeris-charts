import { test, expect } from "@playwright/test";
import { PNG } from "pngjs";

// The demo's price-line style select and TradingView-style "reset view" button.

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

/** Colored pixel runs in the last-price line's row band (left half of the pane). */
function price_line_runs(png, y, is_line_pixel) {
  const runs = [];
  let cur = null;
  for (let x = 0; x < Math.floor(png.width / 2); x++) {
    const o = (y * png.width + x) * 4;
    if (is_line_pixel([png.data[o], png.data[o + 1], png.data[o + 2]])) {
      if (!cur) cur = { s: x, e: x };
      cur.e = x;
    } else if (cur) {
      runs.push(cur);
      cur = null;
    }
  }
  if (cur) runs.push(cur);
  return runs;
}

test("price line style select restyles the live price line", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await wait_for_chart(page);
  // Pin a unique line color so candle pixels can never be mistaken for the price line. Keep
  // the RAW coordinate: row = round(y·scale) — rounding y first drifts the target row by one
  // device px at .5 boundaries.
  const probe = await page.evaluate(() => {
    window.__main.apply_options({ price_line_color: "#ff00ff" });
    const last = window.__data[window.__data.length - 1];
    return { y: window.__main.price_to_coordinate(last.close) };
  });
  const is_line = (c) => Math.abs(c[0] - 255) < 40 && Math.abs(c[1] - 0) < 40 && Math.abs(c[2] - 255) < 40;
  const css_w = await page.evaluate(() => document.querySelector("#chart_container").getBoundingClientRect().width);

  await page.selectOption("#price_line_style", "0"); // solid
  await wait_for_chart(page);
  const solid_png = await capture(page);
  const scale = solid_png.width / css_w;
  const row = Math.round(probe.y * scale);
  const solid = price_line_runs(solid_png, row, is_line);
  expect(solid.length, "solid style: one continuous run").toBeLessThanOrEqual(2);
  expect(solid.reduce((n, r) => n + (r.e - r.s), 0)).toBeGreaterThan(100);

  await page.selectOption("#price_line_style", "1"); // dotted
  await wait_for_chart(page);
  const dotted = price_line_runs(await capture(page), row, is_line);
  expect(dotted.length, "dotted style: many short runs").toBeGreaterThan(10);
});

test("reset view button restores time defaults and re-fits a contracted price scale", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await wait_for_chart(page);

  // Zoom the time scale at RUNTIME (like a wheel zoom — the option default stays 6) and
  // contract the price scale to a manual sliver.
  await page.evaluate(() => {
    window.__chart.wasm.set_bar_spacing(20);
    const last = window.__data[window.__data.length - 1];
    window.__chart.price_scale("right").set_visible_range({ from: last.close - 0.05, to: last.close + 0.05 });
  });
  await wait_for_chart(page);
  expect(await page.evaluate(() => window.__chart.wasm.bar_spacing())).toBe(20);

  await page.click("#reset_view_btn");
  await wait_for_chart(page);
  // Time scale restored to the configured defaults…
  expect(await page.evaluate(() => window.__chart.wasm.bar_spacing())).toBe(6);
  expect(await page.evaluate(() => window.__chart.time_scale().options().bar_spacing)).toBe(6);
  // …and the price scale autoscales the data into view again.
  const fits = await page.evaluate(() => {
    let min = Infinity, max = -Infinity;
    const range = window.__chart.time_scale().get_visible_logical_range();
    for (let i = Math.ceil(range.from); i <= Math.floor(range.to); i++) {
      const bar = window.__main.data_by_index(i);
      if (bar) { min = Math.min(min, bar.low); max = Math.max(max, bar.high); }
    }
    const top = window.__main.price_to_coordinate(max);
    const bottom = window.__main.price_to_coordinate(min);
    const pane_h = document.querySelector("#chart_container").getBoundingClientRect().height;
    return top > 0 && bottom < pane_h && bottom - top > pane_h * 0.3;
  });
  expect(fits, "candles fill the pane again after reset").toBe(true);
});
