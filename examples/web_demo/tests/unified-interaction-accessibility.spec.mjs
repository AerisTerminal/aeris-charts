import { test, expect } from "@playwright/test";

async function open_chart(page) {
  await page.goto("/?backend=canvas2d");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
}

test("default wheel behavior matches Lightweight Charts axis semantics", async ({ page }) => {
  await open_chart(page);
  const box = await page.locator("#chart_container canvas:last-of-type").boundingBox();
  const state = () => page.evaluate(() => ({
    spacing: window.__chart.wasm.bar_spacing(),
    offset: window.__chart.wasm.scroll_position(),
  }));
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);

  const beforeZoom = await state();
  await page.mouse.wheel(0, -24);
  const afterZoom = await state();
  expect(afterZoom.spacing).toBeGreaterThan(beforeZoom.spacing);

  const beforePan = await state();
  await page.mouse.wheel(24, 0);
  const afterPan = await state();
  expect(afterPan.spacing).toBeCloseTo(beforePan.spacing, 8);
  expect(afterPan.offset).not.toBeCloseTo(beforePan.offset, 8);
});

test("pointer interaction does not move focus into the accessibility application", async ({ page }) => {
  await open_chart(page);
  const overlay = page.locator("#chart_container canvas:last-of-type");
  const box = await overlay.boundingBox();
  await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
  const state = await page.evaluate(() => {
    const layer = window.__chart.chart_element().querySelector(".nucleuscharts-a11y-layer");
    return {
      accessibilityFocused: layer.contains(document.activeElement),
      outline: getComputedStyle(layer).outlineStyle,
    };
  });
  expect(state.accessibilityFocused).toBe(false);
  expect(state.outline).toBe("none");
});

test("Pointer Events pinch around the live centroid and continue with the surviving pointer", async ({ page }) => {
  await open_chart(page);
  const result = await page.evaluate(() => {
    const chart = window.__chart;
    const overlay = chart.chart_element().querySelector("canvas:last-of-type");
    const rect = overlay.getBoundingClientRect();
    const paneLeft = chart.wasm.pane_left();
    const x = rect.left + paneLeft + chart.wasm.time_scale_width() / 2;
    const y = rect.top + chart.wasm.pane_height(0) / 2;
    const send = (type, pointerId, clientX, clientY, buttons = type === "pointerup" ? 0 : 1) => {
      overlay.dispatchEvent(new PointerEvent(type, {
        pointerId, pointerType: "touch", isPrimary: pointerId === 11,
        clientX, clientY, button: 0, buttons, bubbles: true, cancelable: true,
      }));
    };
    const before = { spacing: chart.wasm.bar_spacing(), offset: chart.wasm.scroll_position() };
    send("pointerdown", 11, x - 50, y);
    send("pointerdown", 12, x + 50, y);
    send("pointermove", 12, x + 100, y + 20);
    const pinched = { spacing: chart.wasm.bar_spacing(), offset: chart.wasm.scroll_position() };
    send("pointerup", 12, x + 100, y + 20, 0);
    const rebased = chart.wasm.scroll_position();
    send("pointermove", 11, x - 10, y);
    const continued = chart.wasm.scroll_position();
    send("pointerup", 11, x - 10, y, 0);
    return { before, pinched, rebased, continued, touchAction: overlay.style.touchAction };
  });
  expect(result.touchAction).not.toBe("auto");
  expect(result.pinched.spacing).not.toBeCloseTo(result.before.spacing, 6);
  expect(result.pinched.offset).not.toBeCloseTo(result.before.offset, 6);
  expect(result.continued).not.toBeCloseTo(result.rebased, 6);
  expect(Math.abs(result.continued - result.rebased)).toBeCloseTo(40 / result.pinched.spacing, 5);
});

test("capture loss cancels the canonical gesture and rolls back later samples", async ({ page }) => {
  await open_chart(page);
  const result = await page.evaluate(() => {
    const chart = window.__chart;
    const overlay = chart.chart_element().querySelector("canvas:last-of-type");
    const rect = overlay.getBoundingClientRect();
    const x = rect.left + chart.wasm.pane_left() + chart.wasm.time_scale_width() / 2;
    const y = rect.top + chart.wasm.pane_height(0) / 2;
    const send = (type, clientX, buttons = 1) => overlay.dispatchEvent(new PointerEvent(type, {
      pointerId: 31, pointerType: "touch", isPrimary: true, clientX, clientY: y,
      button: 0, buttons, bubbles: true, cancelable: true,
    }));
    send("pointerdown", x);
    send("pointermove", x - 40);
    const atLoss = chart.wasm.scroll_position();
    send("lostpointercapture", x - 40, 0);
    send("pointermove", x - 140);
    const after = chart.wasm.scroll_position();
    return { atLoss, after };
  });
  expect(result.after).toBeCloseTo(result.atLoss, 8);
});

test("accessibility is default, singleton, bounded, silent for streaming, and keyboard drawing edits roll back", async ({ page }) => {
  await open_chart(page);
  const result = await page.evaluate(async () => {
    const api = await import("/dist/nucleuscharts_financial.js");
    const chart = window.__chart;
    const host = chart.chart_element();
    const first = chart.accessibility();
    const compatible = api.enable_accessibility(chart, { chart_title: "Unified chart" });
    const drawing = chart.add_drawing("trend_line", [
      { logical: 20, price: 100 },
      { logical: 30, price: 105 },
    ]);
    chart.wasm.set_selected_drawing(drawing.id);
    first.focus_target(`drawing:${drawing.id}`);
    const layer = document.activeElement;
    const before = drawing.points();
    for (const key of ["Enter", "ArrowRight", "Escape"]) {
      layer.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
    }
    const after = drawing.points();
    const last = window.__main.data().at(-1);
    window.__main.update({ ...last, close: last.close + 0.25 });
    await new Promise((resolve) => setTimeout(resolve, 220));
    await new Promise((resolve) => requestAnimationFrame(resolve));
    const state = {
      singleton: first === compatible,
      hostRole: host.getAttribute("role"),
      applications: host.querySelectorAll('[role="application"]').length,
      canvasHidden: [...host.querySelectorAll("canvas")].every((canvas) => canvas.getAttribute("aria-hidden") === "true"),
      live: host.querySelector(".nucleuscharts-a11y-shared-status-region")?.textContent ?? "",
      before,
      after,
    };
    chart.apply_options({ accessibility: false });
    state.disabledApplications = host.querySelectorAll('[role="application"]').length;
    chart.apply_options({ accessibility: true });
    state.reenabledApplications = host.querySelectorAll('[role="application"]').length;
    return state;
  });
  expect(result).toMatchObject({
    singleton: true,
    hostRole: "group",
    applications: 1,
    canvasHidden: true,
    live: "",
    disabledApplications: 0,
    reenabledApplications: 1,
  });
  expect(result.after).toEqual(result.before);
});

test("semantic axis and trading targets route keyboard actions through canonical engine paths", async ({ page }) => {
  await open_chart(page);
  const result = await page.evaluate(() => {
    const chart = window.__chart;
    const a11y = chart.accessibility();
    const trading = chart.trading();
    const intents = [];
    trading.subscribe_intents((intent) => intents.push(intent));
    trading.apply_snapshot({
      instrument: { tick_size: 0.25, price_precision: 2 },
      orders: [{
        id: "keyboard-order", pane_index: 0, price_scale: "right", side: "buy",
        kind: "limit", role: "working", status: "working", price: 100,
        quantity: 2, filled_quantity: 0, revision: 3,
      }],
    });
    a11y.refresh();
    a11y.focus_target("order:keyboard-order");
    for (const key of ["Enter", "ArrowUp", "Enter"]) {
      document.activeElement.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
    }
    const price = chart.price_scale("right", 0);
    price.set_visible_range({ from: 90, to: 110 });
    a11y.focus_target("price-axis");
    document.activeElement.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true, cancelable: true }));
    const adjusted = price.get_visible_range();
    document.activeElement.dispatchEvent(new KeyboardEvent("keydown", { key: "Home", bubbles: true, cancelable: true }));
    return {
      intent: intents[0],
      adjusted,
      autoScale: price.options().auto_scale,
      focused: document.activeElement.getAttribute("aria-label"),
    };
  });
  expect(result.intent).toMatchObject({
    action: "create_take_profit",
    order_id: "keyboard-order",
    side: "sell",
    kind: "limit",
    role: "take_profit",
    price: 100.25,
  });
  expect(result.adjusted.to - result.adjusted.from).toBeCloseTo(19, 8);
  expect(result.autoScale).toBe(true);
  expect(result.focused).toContain("price axis");
});

test("DPR-only transitions resize auto-sized bitmap surfaces", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "devicePixelRatio override is only deterministic in Chromium");
  await open_chart(page);
  await page.evaluate(() => window.__chart.apply_options({ autoSize: true }));
  const before = await page.evaluate(() => {
    const canvas = window.__chart.chart_element().querySelector("canvas");
    return { bitmap: canvas.width, css: canvas.getBoundingClientRect().width };
  });
  await page.evaluate(() => {
    Object.defineProperty(window, "devicePixelRatio", { configurable: true, value: 2 });
    window.dispatchEvent(new Event("orientationchange"));
  });
  await page.waitForFunction((oldWidth) => document.querySelector("#chart_container canvas").width !== oldWidth, before.bitmap);
  const after = await page.evaluate(() => {
    const canvas = window.__chart.chart_element().querySelector("canvas");
    return { bitmap: canvas.width, css: canvas.getBoundingClientRect().width };
  });
  expect(after.css).toBeCloseTo(before.css, 1);
  expect(after.bitmap).toBeGreaterThan(before.bitmap);
});
