import { test, expect } from "@playwright/test";

async function open_pair(page) {
  await page.goto("/?backend=canvas2d");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");
  await page.evaluate(() => {
    const iframe = document.createElement("iframe");
    iframe.id = "reference-oracle";
    iframe.src = "/reference.html?spacing=6";
    iframe.style.cssText =
      "position:fixed;left:0;top:0;width:960px;height:540px;z-index:1000;border:0";
    document.body.append(iframe);
  });
  const oracle = page.frameLocator("#reference-oracle");
  await oracle.locator("html[data-ready=true]").waitFor();
  await page.evaluate(() => {
    window.__chart.time_scale().apply_options({
      bar_spacing: 6,
      right_offset: 0,
      right_bar_stays_on_scroll: false,
    });
  });
}

async function reset_pair(page) {
  await page.evaluate(() => {
    window.__chart.time_scale().apply_options({
      bar_spacing: 6,
      right_offset: 0,
      right_bar_stays_on_scroll: false,
    });
  });
  const oracle = page.frames().find((frame) => frame.url().includes("/reference.html"));
  await oracle.evaluate(() => {
    window.__reference.chart.timeScale().applyOptions({
      barSpacing: 6,
      rightOffset: 0,
      rightBarStaysOnScroll: false,
    });
  });
  // Both implementations invalidate asynchronously. Do not let a pending reset from the previous
  // trace land after the next trace's `before` snapshot on slower CI runners.
  await page.waitForFunction(() =>
    Math.abs(window.__chart.wasm.bar_spacing() - 6) < 1e-9
    && Math.abs(window.__chart.wasm.scroll_position()) < 1e-9,
  );
  await oracle.waitForFunction(() =>
    Math.abs(window.__reference.chart.timeScale().options().barSpacing - 6) < 1e-9
    && Math.abs(window.__reference.chart.timeScale().scrollPosition()) < 1e-9,
  );
}

async function states(page) {
  const Aeris = await page.evaluate(() => ({
    spacing: window.__chart.wasm.bar_spacing(),
    offset: window.__chart.wasm.scroll_position(),
    logical: window.__chart.time_scale().coordinate_to_logical(
      window.__chart.wasm.time_scale_width() / 2,
    ),
  }));
  const oracle = page.frames().find((frame) => frame.url().includes("/reference.html"));
  const reference = await oracle.evaluate(() => ({
    spacing: window.__reference.chart.timeScale().options().barSpacing,
    offset: window.__reference.chart.timeScale().scrollPosition(),
    logical: window.__reference.chart.timeScale().coordinateToLogical(
      window.__reference.chart.timeScale().width() / 2,
    ),
  }));
  return { Aeris, reference };
}

async function dispatch_wheel_pair(page, trace, axis = false) {
  await page.evaluate(({ trace, axis }) => {
    const chart = window.__chart;
    const overlay = chart.chart_element().querySelector("canvas:last-of-type");
    const rect = overlay.getBoundingClientRect();
    const paneLeft = chart.wasm.pane_left();
    const x = axis ? paneLeft + chart.wasm.time_scale_width() + 6
      : paneLeft + chart.wasm.time_scale_width() / 2;
    overlay.dispatchEvent(new WheelEvent("wheel", {
      ...trace,
      clientX: rect.left + x,
      clientY: rect.top + chart.wasm.pane_height(0) / 2,
      bubbles: true,
      cancelable: true,
    }));
  }, { trace, axis });
  const oracle = page.frames().find((frame) => frame.url().includes("/reference.html"));
  await oracle.evaluate(({ trace, axis }) => {
    const root = document.querySelector("#chart > div");
    const rect = root.getBoundingClientRect();
    root.dispatchEvent(new WheelEvent("wheel", {
      ...trace,
      clientX: axis ? rect.right - 6 : rect.left + rect.width / 2,
      clientY: rect.top + rect.height / 2,
      bubbles: true,
      cancelable: true,
    }));
  }, { trace, axis });
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(resolve)));
}

test("normalized wheel traces retain behavior learned from the public reference fixture", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "synthetic line/page WheelEvent scaling differs across browser test drivers");
  await open_pair(page);
  for (const trace of [
    { deltaX: 0, deltaY: -24, deltaMode: 0 },
    { deltaX: 0, deltaY: -1, deltaMode: 1 },
    { deltaX: 0, deltaY: -1, deltaMode: 2 },
    { deltaX: 18, deltaY: 0, deltaMode: 0 },
    { deltaX: 18, deltaY: -24, deltaMode: 0 },
    { deltaX: 0, deltaY: -24, deltaMode: 0, ctrlKey: true },
    { deltaX: 0, deltaY: -24, deltaMode: 0, shiftKey: true },
  ]) {
    await reset_pair(page);
    const before = await states(page);
    await dispatch_wheel_pair(page, trace);
    const after = await states(page);
    expect(after.Aeris.spacing / before.Aeris.spacing, JSON.stringify(trace))
      .toBeCloseTo(after.reference.spacing / before.reference.spacing, 8);
    if (trace.deltaY === 0) {
      expect(after.Aeris.offset - before.Aeris.offset, JSON.stringify(trace))
        .toBeCloseTo(after.reference.offset - before.reference.offset, 6);
    } else if (trace.deltaX === 0) {
      expect(after.Aeris.logical - before.Aeris.logical, JSON.stringify(trace)).toBe(0);
      expect(after.reference.logical - before.reference.logical, JSON.stringify(trace)).toBe(0);
    }
  }

  await reset_pair(page);
  const before_axis = await states(page);
  await dispatch_wheel_pair(page, { deltaX: 0, deltaY: -24, deltaMode: 0 }, true);
  const after_axis = await states(page);
  expect(after_axis.Aeris.spacing / before_axis.Aeris.spacing)
    .toBeCloseTo(after_axis.reference.spacing / before_axis.reference.spacing, 8);
});

test("pane drag slop and click/double-click callback counts match the oracle", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "iframe double-click delivery is not portable in Playwright");
  await open_pair(page);
  await page.evaluate(() => {
    window.__AerisClicks = { single: 0, double: 0 };
    window.__chart.subscribe_click(() => window.__AerisClicks.single++);
    window.__chart.subscribe_dbl_click(() => window.__AerisClicks.double++);
  });
  const oracle = page.frames().find((frame) => frame.url().includes("/reference.html"));
  await oracle.evaluate(() => {
    window.__oracleClicks = { single: 0, double: 0 };
    window.__reference.chart.subscribeClick(() => window.__oracleClicks.single++);
    window.__reference.chart.subscribeDblClick(() => window.__oracleClicks.double++);
  });

  const Aeris_box = await page.locator("#chart_container canvas:last-of-type").boundingBox();
  const Aeris_geometry = await page.evaluate(() => ({
    left: window.__chart.wasm.pane_left(),
    width: window.__chart.wasm.time_scale_width(),
    y: window.__chart.wasm.pane_height(0) / 2,
  }));
  const iframe_box = await page.locator("#reference-oracle").boundingBox();
  const oracle_root = page.frameLocator("#reference-oracle").locator("#chart > div");
  const oracle_box = await oracle_root.boundingBox();
  const Aeris_x = Aeris_box.x + Aeris_geometry.left + Aeris_geometry.width / 2;
  const Aeris_y = Aeris_box.y + Aeris_geometry.y;
  const oracle_x = iframe_box.x + oracle_box.x + oracle_box.width / 2;
  const oracle_y = iframe_box.y + oracle_box.y + oracle_box.height / 2;

  await page.locator("#reference-oracle").evaluate((node) => { node.style.pointerEvents = "none"; });
  const Aeris_offsets = [];
  await page.mouse.move(Aeris_x, Aeris_y);
  await page.mouse.down();
  for (const dx of [1, 4, 5, 6, 20]) {
    await page.mouse.move(Aeris_x + dx, Aeris_y);
    Aeris_offsets.push(await page.evaluate(() => window.__chart.wasm.scroll_position()));
  }
  await page.mouse.up();

  await page.locator("#reference-oracle").evaluate((node) => { node.style.pointerEvents = "auto"; });
  const oracle_offsets = [];
  await page.mouse.move(oracle_x, oracle_y);
  await page.mouse.down();
  for (const dx of [1, 4, 5, 6, 20]) {
    await page.mouse.move(oracle_x + dx, oracle_y);
    oracle_offsets.push(await oracle.evaluate(() => window.__reference.chart.timeScale().scrollPosition()));
  }
  await page.mouse.up();

  expect(Aeris_offsets.slice(0, 3)).toEqual(oracle_offsets.slice(0, 3));
  expect(Aeris_offsets[4]).not.toBeCloseTo(Aeris_offsets[0], 8);
  expect(oracle_offsets[4]).not.toBeCloseTo(oracle_offsets[0], 8);

  await page.locator("#reference-oracle").evaluate((node) => { node.style.pointerEvents = "none"; });
  await page.mouse.dblclick(Aeris_x, Aeris_y);
  await page.locator("#reference-oracle").evaluate((node) => { node.style.pointerEvents = "auto"; });
  await page.mouse.dblclick(oracle_x, oracle_y);
  const counts = {
    Aeris: await page.evaluate(() => window.__AerisClicks),
    reference: await oracle.evaluate(() => window.__oracleClicks),
  };
  expect(counts.Aeris).toEqual(counts.reference);
  expect(counts.Aeris).toEqual({ single: 1, double: 1 });
});
