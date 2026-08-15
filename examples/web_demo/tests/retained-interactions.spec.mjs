import { test, expect } from "@playwright/test";
import { PNG } from "pngjs";

async function settle(page) {
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined);
  await page.evaluate(() => new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  }));
  await page.waitForTimeout(600);
}

async function snapshot(page) {
  const data_url = await page.evaluate(() => window.__chart.take_screenshot().toDataURL("image/png"));
  return PNG.sync.read(Buffer.from(data_url.slice(data_url.indexOf(",") + 1), "base64"));
}

function changed_pixels(before, after) {
  expect([after.width, after.height]).toEqual([before.width, before.height]);
  let changed = 0;
  for (let i = 0; i < before.data.length; i += 4) {
    if (before.data[i] !== after.data[i]
      || before.data[i + 1] !== after.data[i + 1]
      || before.data[i + 2] !== after.data[i + 2]
      || before.data[i + 3] !== after.data[i + 3]) changed += 1;
  }
  return changed;
}

function primary_pixels(png) {
  let count = 0;
  for (let i = 0; i < png.data.length; i += 4) {
    if (Math.abs(png.data[i] - 62) <= 30
      && Math.abs(png.data[i + 1] - 99) <= 30
      && Math.abs(png.data[i + 2] - 221) <= 30) count += 1;
  }
  return count;
}

async function chart_geometry(page) {
  return page.evaluate(() => {
    const bounds = document.getElementById("chart_container").getBoundingClientRect();
    return {
      x: bounds.left + window.__chart.wasm.pane_left(),
      y: bounds.top,
      width: window.__chart.wasm.time_scale_width(),
      height: bounds.height - window.__chart.wasm.time_scale_height(),
    };
  });
}

for (const requested_backend of ["auto", "canvas2d"]) {
  test(`real ${requested_backend} pan and zoom rebuild historical pixels coherently`, async ({ page }) => {
    await page.goto(`/?backend=${requested_backend}&forceFallbackAdapter=1`);
    await settle(page);
    const actual_backend = await page.evaluate(() => window.__chart.backend());
    expect(actual_backend).toBe(requested_backend === "canvas2d" ? "canvas2d" : "webgpu");

    const box = await chart_geometry(page);
    const center = { x: box.x + box.width * 0.55, y: box.y + box.height * 0.45 };
    await page.mouse.move(center.x, center.y);
    let previous = await snapshot(page);
    let previous_offset = await page.evaluate(() => window.__chart.wasm.scroll_position());

    await page.mouse.down();
    for (const delta of [35, 70, 105]) {
      await page.mouse.move(center.x - delta, center.y);
      await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(resolve)));
      const offset = await page.evaluate(() => window.__chart.wasm.scroll_position());
      const current = await snapshot(page);
      expect(offset).not.toBeCloseTo(previous_offset, 8);
      expect(changed_pixels(previous, current), `pan move ${delta} changed state but not pixels`).toBeGreaterThan(500);
      previous = current;
      previous_offset = offset;
    }
    await page.mouse.up();

    const spacing_before = await page.evaluate(() => window.__chart.wasm.bar_spacing());
    const before_zoom = await snapshot(page);
    await page.mouse.move(center.x, center.y);
    await page.mouse.wheel(0, -120);
    await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(resolve)));
    const spacing_after = await page.evaluate(() => window.__chart.wasm.bar_spacing());
    const after_zoom = await snapshot(page);
    expect(spacing_after).toBeGreaterThan(spacing_before);
    expect(changed_pixels(before_zoom, after_zoom), "zoom changed state but not historical pixels").toBeGreaterThan(500);

    const spot = await page.evaluate(() => {
      const range = window.__chart.time_scale().get_visible_logical_range();
      const index = Math.floor((range.from + range.to) / 2);
      const bar = window.__main.data_by_index(index);
      const bounds = document.getElementById("chart_container").getBoundingClientRect();
      return {
        x: bounds.left + window.__chart.wasm.pane_left()
          + window.__chart.time_scale().logical_to_coordinate(index),
        y: bounds.top + window.__main.price_to_coordinate(bar.close),
      };
    });
    await page.mouse.move(spot.x, spot.y);
    const cursor = await page.evaluate(() => {
      const canvases = document.querySelectorAll("#chart_container canvas");
      return canvases[canvases.length - 1].style.cursor;
    });
    expect(cursor).toBe("pointer");
    await page.mouse.click(spot.x, spot.y);
    expect(primary_pixels(await snapshot(page)), "series selection after pan/zoom must paint anchors")
      .toBeGreaterThan(50);
  });
}
