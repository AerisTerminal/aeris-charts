import { test, expect } from "@playwright/test";
import { PNG } from "pngjs";

async function open_alert_demo(page) {
  await page.goto("/?feature=trading&backend=canvas2d");
  await page.waitForFunction(() => window.__demo_catalogs?.lab.active_ids().includes("trading-bracket"));
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
}

test("crosshair plus chip emits an exact host request and host alert lines stay authoritative", async ({ page }) => {
  await open_alert_demo(page);
  const probe = await page.evaluate(() => {
    window.__alert_requests = [];
    window.__chart.subscribe_crosshair_action((request) => window.__alert_requests.push(request));
    const overlay = document.querySelector("#chart_container canvas:last-of-type").getBoundingClientRect();
    return {
      x: overlay.left + window.__chart.time_scale().width() - 11.5,
      y: overlay.top + window.__main.price_to_coordinate(104),
    };
  });

  await page.mouse.move(probe.x, probe.y);
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(resolve)));
  expect(await page.locator("#chart_container canvas:last-of-type").evaluate((canvas) => canvas.style.cursor)).toBe("pointer");
  await page.mouse.click(probe.x, probe.y);

  const request = await page.evaluate(() => window.__alert_requests[0]);
  expect(request).toMatchObject({
    pane_index: 0,
    price_scale_id: "right",
  });
  expect(request.price).toBeCloseTo(104, 6);

  const demo_line = await page.evaluate(() => window.__chart.alerts().state().lines.find((line) => line.id.startsWith("demo-alert-")));
  expect(demo_line).toMatchObject({
    price: request.price,
    condition: "crossing",
    frequency: "only_once",
    status: "active",
  });
  expect(demo_line.label).toBeUndefined();

  const state = await page.evaluate((price) => {
    const alerts = window.__chart.alerts();
    alerts.update_line({
      id: "host-alert",
      price,
      condition: "crossing_up",
      frequency: "every_time",
      status: "active",
      label: "Breakout",
    });
    return alerts.state();
  }, request.price);
  expect(state.lines).toEqual(expect.arrayContaining([
    expect.objectContaining({
      id: "host-alert",
      condition: "crossing_up",
      frequency: "every_time",
      status: "active",
      label: "Breakout",
    }),
  ]));
});

test("crosshair action remains visible and hit-testable at every DPR", async ({ page }) => {
  await open_alert_demo(page);
  const box = await page.locator("#chart_container canvas:last-of-type").boundingBox();
  await page.mouse.move(box.x + 200, box.y + box.height * 0.3);
  for (const dpr of [1, 1.25, 1.5, 2, 3]) {
    await page.evaluate(({ dpr }) => {
      const container = document.querySelector("#chart_container").getBoundingClientRect();
      window.__chart.resize(container.width, container.height, dpr);
    }, { dpr });
    await page.mouse.move(box.x + 200, box.y + box.height * 0.3);
    const visible = await page.evaluate(async () => {
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      const width = document.querySelector("#chart_container canvas:last-of-type").clientWidth;
      const height = document.querySelector("#chart_container canvas:last-of-type").clientHeight;
      const plot_width = window.__chart.time_scale().width();
      for (let y = Math.max(0, height * 0.3 - 50); y <= Math.min(height, height * 0.3 + 50); y += 2) {
        for (let x = Math.max(0, plot_width - 30); x <= width; x += 2) {
          if (window.__chart.alert_create_hit_at(x, y)) return true;
        }
      }
      return false;
    });
    expect(visible).toBe(true);
  }
});

for (const backend of ["canvas2d", "webgpu"]) {
  test(`crosshair action visibly matches the circular SVG (${backend})`, async ({ page }) => {
    await page.goto(`/?feature=trading&backend=${backend}&forceFallbackAdapter=1`);
    await page.waitForFunction(() => window.__demo_catalogs?.lab.active_ids().includes("trading-bracket"));
    expect(await page.evaluate(() => window.__chart.backend())).toBe(backend);
    await page.evaluate(() => window.__chart.apply_options({ crosshair: { mode: 0 } }));
    const box = await page.locator("#chart_container canvas:last-of-type").boundingBox();
    const y = box.height * 0.3;
    await page.mouse.move(box.x + 200, box.y + y);
    await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    const plotWidth = await page.evaluate(() => window.__chart.time_scale().width());
    const png = PNG.sync.read(await page.screenshot({ clip: { x: box.x + plotWidth - 19, y: box.y + y - 9.5, width: 19, height: 19 } }));
    const peak = (x, y) => {
      let value = 0;
      const px = Math.round(x / 19 * png.width);
      const py = Math.round(y / 19 * png.height);
      for (let dy = -1; dy <= 1; dy++) for (let dx = -1; dx <= 1; dx++) {
        const offset = ((py + dy) * png.width + px + dx) * 4;
        value = Math.max(value, Math.min(...png.data.subarray(offset, offset + 3)));
      }
      return value;
    };
    // White at the plus center and circle's cardinal points; dark outside the ring.
    for (const [x, y] of [[9.5, 9.5], [9.5, 2.375], [9.5, 16.625], [2.375, 9.5], [16.625, 9.5]]) {
      expect(peak(x, y), `missing circular-plus stroke at ${x},${y}`).toBeGreaterThan(150);
    }
    for (const [x, y] of [[2, 2], [17, 2], [2, 17], [17, 17]]) {
      expect(peak(x, y), `square corner at ${x},${y}`).toBeLessThan(100);
    }
  });
}
