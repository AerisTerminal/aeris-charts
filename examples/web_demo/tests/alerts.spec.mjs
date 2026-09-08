import { test, expect } from "@playwright/test";
import { PNG } from "pngjs";
import { readFileSync } from "node:fs";

async function open_alert_demo(page) {
  await page.goto("/?feature=trading&backend=canvas2d");
  await page.waitForFunction(() => window.__feature_lab?.active_ids().includes("trading-bracket"));
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

test("crosshair uses the original SVG pixels at every DPR", async ({ page }) => {
  await page.addInitScript(() => {
    window.__action_images = [];
    const draw = CanvasRenderingContext2D.prototype.drawImage;
    CanvasRenderingContext2D.prototype.drawImage = function (source, ...rest) {
      if (source.width <= 96 && source.height <= 96 && source.getContext) {
        const pixels = source.getContext("2d").getImageData(0, 0, source.width, source.height).data;
        window.__action_images.push({ size: source.width, alpha: Array.from(pixels).filter((_, i) => i % 4 === 3) });
      }
      return draw.call(this, source, ...rest);
    };
  });
  await open_alert_demo(page);
  const box = await page.locator("#chart_container canvas:last-of-type").boundingBox();
  await page.mouse.move(box.x + 200, box.y + box.height * 0.3);
  const svg = readFileSync(new URL("../../../packages/charts/src/assets/icons/add.svg", import.meta.url), "utf8");
  for (const dpr of [1, 1.25, 1.5, 2, 3]) {
    const result = await page.evaluate(({ dpr, svg }) => {
      window.__action_images = [];
      const container = document.querySelector("#chart_container").getBoundingClientRect();
      window.__chart.resize(container.width, container.height, dpr);
      const size = Math.round(19 * 0.9 * dpr);
      const c = document.createElement("canvas"); c.width = c.height = size;
      const ctx = c.getContext("2d", { willReadFrequently: true }); ctx.scale(size / 24, size / 24);
      ctx.strokeStyle = "#ffffff"; ctx.lineWidth = 1.5; ctx.lineCap = ctx.lineJoin = "round";
      const root = new DOMParser().parseFromString(svg, "image/svg+xml");
      for (const p of root.querySelectorAll("path")) ctx.stroke(new Path2D(p.getAttribute("d")));
      for (const p of root.querySelectorAll("circle")) {
        const path = new Path2D(); path.arc(+p.getAttribute("cx"), +p.getAttribute("cy"), +p.getAttribute("r"), 0, Math.PI * 2); ctx.stroke(path);
      }
      const expected = Array.from(ctx.getImageData(0, 0, size, size).data).filter((_, i) => i % 4 === 3);
      return { actual: window.__action_images.find((image) => image.size === size)?.alpha, expected };
    }, { dpr, svg });
    expect(result.actual).toEqual(result.expected);
  }
});

for (const backend of ["canvas2d", "webgpu"]) {
  test(`crosshair action visibly matches the circular SVG (${backend})`, async ({ page }) => {
    await page.goto(`/?feature=trading&backend=${backend}&forceFallbackAdapter=1`);
    await page.waitForFunction(() => window.__feature_lab?.active_ids().includes("trading-bracket"));
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
