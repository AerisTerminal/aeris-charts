import { test, expect } from "@playwright/test";

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

test("crosshair plus icon raster follows browser zoom DPR", async ({ page }) => {
  await open_alert_demo(page);

  const sizes = await page.evaluate(() => {
    const container = document.querySelector("#chart_container").getBoundingClientRect();
    const chart = window.__chart;
    chart.resize(container.width, container.height, 1);
    const dpr1 = chart.alert_icon_size_for_test();
    chart.resize(container.width, container.height, 2);
    const dpr2 = chart.alert_icon_size_for_test();
    return { dpr1, dpr2 };
  });

  // Compact axes: default font 12 => 11px axis text => 19px crosshair chip. The SVG
  // viewport occupies 90% of that box, leaving a narrow optical inset without
  // making its stroke subpixel at 1x.
  // Its intrinsic bitmap must match that destination at each DPR instead of
  // remaining a fixed 3x raster.
  expect(sizes).toEqual({ dpr1: 17, dpr2: 34 });
});
