import { test, expect } from "@playwright/test";

async function create_worker_chart(page, backend) {
  await page.goto("/");
  const supported = await page.evaluate(() =>
    typeof OffscreenCanvas !== "undefined"
    && "transferControlToOffscreen" in HTMLCanvasElement.prototype,
  );
  test.skip(!supported, "OffscreenCanvas transfer is unavailable in this browser");

  await page.evaluate((requested_backend) => {
    const host = document.createElement("div");
    host.id = "offscreen-worker-host";
    host.style.cssText = "position:fixed;left:0;top:0;width:640px;height:360px;z-index:1000;background:white";
    const gpu = document.createElement("canvas");
    const fallback = document.createElement("canvas");
    for (const canvas of [gpu, fallback]) {
      canvas.style.cssText = "position:absolute;inset:0;width:640px;height:360px";
      host.appendChild(canvas);
    }
    document.body.appendChild(host);
    const worker = new Worker("/offscreen_chart_worker.js", { type: "module" });
    const messages = [];
    worker.onmessage = (event) => {
      messages.push(event.data);
      if (event.data.backend) {
        gpu.style.visibility = event.data.backend === "webgpu" ? "visible" : "hidden";
        fallback.style.visibility = event.data.backend === "canvas2d" ? "visible" : "hidden";
      }
    };
    worker.onerror = (event) => messages.push({ type: "error", message: event.message });
    const gpu_canvas = gpu.transferControlToOffscreen();
    const fallback_canvas = fallback.transferControlToOffscreen();
    worker.postMessage({
      type: "init",
      gpu_canvas,
      fallback_canvas,
      width: 640,
      height: 360,
      dpr: 1,
      backend: requested_backend,
      bars: 5_000,
      force_fallback_adapter: true,
    }, [gpu_canvas, fallback_canvas]);
    window.__offscreen_worker = { worker, messages };
  }, backend);

  await page.waitForFunction(() => window.__offscreen_worker.messages.some((message) =>
    message.type === "ready" || message.type === "error",
  ));
  const ready = await page.evaluate(() => window.__offscreen_worker.messages.at(-1));
  expect(ready.type, ready.message).toBe("ready");
  return ready;
}

async function send(page, message) {
  const count = await page.evaluate((payload) => {
    const state = window.__offscreen_worker;
    const before = state.messages.length;
    state.worker.postMessage(payload);
    return before;
  }, message);
  await page.waitForFunction((before) => window.__offscreen_worker.messages.length > before, count);
  const result = await page.evaluate(() => window.__offscreen_worker.messages.at(-1));
  expect(result.type, result.message).not.toBe("error");
  return result;
}

test.beforeEach(async ({ page }) => {
  page.on("pageerror", (error) => console.log(`[browser:pageerror] ${error.message}`));
  page.on("console", (message) => console.log(`[browser:${message.type()}] ${message.text()}`));
});

test.afterEach(async ({ page }) => {
  await page.evaluate(() => window.__offscreen_worker?.worker.terminate());
});

test("OffscreenCanvas worker renders, resizes, and accepts relayed pointer/wheel/key input", async ({ page }) => {
  test.setTimeout(120_000);
  const ready = await create_worker_chart(page, undefined);
  expect(ready.backend).toBe("webgpu");
  expect(ready.stats.presented_frames).toBeGreaterThan(0);
  expect(ready.stats.draw_calls).toBeGreaterThan(0);
  expect(ready.stats.canvas2d_ops).toBe(0);
  expect(ready.range).not.toBeNull();
  expect(ready.size).toEqual([640, 360]);

  const wheel = await send(page, {
    type: "wheel",
    event: { x: 320, y: 160, delta_x: 0, delta_y: -120, delta_mode: 0 },
  });
  expect(wheel.range).not.toEqual(ready.range);
  expect(wheel.stats.canvas2d_ops).toBe(0);

  await send(page, { type: "pointer", event: { type: "down", x: 250, y: 150, pointer_id: 7 } });
  // The first move crosses the shared drag threshold; the next sample pans the plot.
  await send(page, {
    type: "pointer",
    event: { type: "move", x: 320, y: 152, pointer_id: 7, buttons: 1 },
  });
  const dragged = await send(page, {
    type: "pointer",
    event: { type: "move", x: 390, y: 155, pointer_id: 7, buttons: 1 },
  });
  await send(page, { type: "pointer", event: { type: "up", x: 390, y: 155, pointer_id: 7 } });
  expect(dragged.range).not.toEqual(wheel.range);

  // A second pointer may be active, but canceling the pointer that owns the drag must end the
  // scroll session; the non-owner cannot inherit it.
  await send(page, { type: "pointer", event: { type: "down", x: 260, y: 150, pointer_id: 8, pointer_type: "touch" } });
  await send(page, { type: "pointer", event: { type: "down", x: 320, y: 150, pointer_id: 9, pointer_type: "touch" } });
  const pinched = await send(page, {
    type: "pointer", event: { type: "move", x: 380, y: 165, pointer_id: 9, pointer_type: "touch", buttons: 1 },
  });
  expect(pinched.range).not.toEqual(dragged.range);
  const canceled = await send(page, {
    type: "pointer", event: { type: "cancel", x: 260, y: 150, pointer_id: 8, pointer_type: "touch" },
  });
  const non_owner_move = await send(page, {
    type: "pointer", event: { type: "move", x: 500, y: 150, pointer_id: 9, pointer_type: "touch", buttons: 1 },
  });
  expect(non_owner_move.range).toEqual(canceled.range);
  await send(page, { type: "pointer", event: { type: "up", x: 500, y: 150, pointer_id: 9, pointer_type: "touch" } });

  const keyed = await send(page, { type: "key", event: { key: "ArrowRight" } });
  expect(keyed.range).not.toEqual(dragged.range);
  await page.waitForTimeout(120);
  const after_discrete_key = await send(page, { type: "key", event: { key: "Unidentified" } });
  expect(after_discrete_key.range).toEqual(keyed.range);

  const resized = await send(page, { type: "resize", width: 500, height: 300, dpr: 2 });
  expect(resized.size).toEqual([1_000, 600]);
  expect(resized.stats.presented_frames).toBeGreaterThan(keyed.stats.presented_frames);
});

test("worker pinch keeps the logical bar under its starting centroid", async ({ page }) => {
  await create_worker_chart(page, "canvas2d");
  await send(page, { type: "pointer", event: { type: "down", x: 260, y: 150, pointer_id: 1, pointer_type: "touch" } });
  const before = await send(page, { type: "pointer", event: { type: "down", x: 380, y: 150, pointer_id: 2, pointer_type: "touch" } });
  const after = await send(page, { type: "pointer", event: { type: "move", x: 420, y: 150, pointer_id: 2, pointer_type: "touch", buttons: 1 } });
  expect(after.range).not.toEqual(before.range);
  expect(after.logical_320).toBeCloseTo(before.logical_320, 5);
});

test("worker auto wheel uses the same opt-in price-axis zoom policy", async ({ page }) => {
  const ready = await create_worker_chart(page, "canvas2d");
  const event = { x: ready.axis_x, y: ready.axis_y, delta_x: 0, delta_y: -100 };
  const default_wheel = await send(page, { type: "wheel", event });
  expect(default_wheel.bar_spacing).not.toBeCloseTo(ready.bar_spacing, 8);
  expect(default_wheel.price_range).toEqual(ready.price_range);
  await send(page, { type: "options", options: { price_axis_wheel_zoom: true } });
  const option_wheel = await send(page, { type: "wheel", event });
  expect(option_wheel.price_range).not.toEqual(default_wheel.price_range);
  expect(option_wheel.bar_spacing).toBeCloseTo(default_wheel.bar_spacing, 8);
});

test("worker Page, End, zoom, and Home keys use the engine bindings", async ({ page }) => {
  const ready = await create_worker_chart(page, "canvas2d");
  const older = await send(page, { type: "key", event: { key: "PageUp" } });
  expect(older.range).not.toEqual(ready.range);
  const latest = await send(page, { type: "key", event: { key: "End" } });
  expect(latest.range).not.toEqual(older.range);
  const zoomed = await send(page, { type: "key", event: { key: "+" } });
  expect(zoomed.bar_spacing).toBeGreaterThan(latest.bar_spacing);
  const reset = await send(page, { type: "key", event: { key: "Home" } });
  expect(reset.bar_spacing).toBe(6);
});

test("worker Delete applies the engine request to remove a selected series", async ({ page }) => {
  const ready = await create_worker_chart(page, "canvas2d");
  expect(ready.series_ids).toContain(0);
  await send(page, { type: "select_series", id: 0 });
  const deleted = await send(page, { type: "key", event: { key: "Delete" } });
  expect(deleted.series_ids).not.toContain(0);
  const replacement = await send(page, { type: "add_series", kind: "line" });
  expect(replacement.series_ids).toHaveLength(1);
  expect(replacement.series_ids).not.toContain(0);
});

test("worker mouse pan waits for the shared drag threshold", async ({ page }) => {
  const ready = await create_worker_chart(page, "canvas2d");
  await send(page, { type: "pointer", event: { type: "down", x: 250, y: 150, pointer_id: 17 } });
  const below = await send(page, { type: "pointer", event: { type: "move", x: 253, y: 150, pointer_id: 17, buttons: 1 } });
  expect(below.range).toEqual(ready.range);
  const crossing = await send(page, { type: "pointer", event: { type: "move", x: 257, y: 150, pointer_id: 17, buttons: 1 } });
  expect(crossing.range).toEqual(ready.range);
  const panned = await send(page, { type: "pointer", event: { type: "move", x: 280, y: 150, pointer_id: 17, buttons: 1 } });
  expect(panned.range).not.toEqual(ready.range);
  await send(page, { type: "pointer", event: { type: "up", x: 280, y: 150, pointer_id: 17 } });
});

test("worker touch pan waits for the controller threshold after pinch continuation", async ({ page }) => {
  const ready = await create_worker_chart(page, "canvas2d");
  const pointer = (type, x, pointer_id) => ({
    type: "pointer", event: { type, x, y: 150, pointer_id, pointer_type: "touch", buttons: type === "move" ? 1 : 0 },
  });
  await send(page, pointer("down", 250, 31));
  const below = await send(page, pointer("move", 253, 31));
  expect(below.range).toEqual(ready.range);
  const crossing = await send(page, pointer("move", 257, 31));
  expect(crossing.range).toEqual(ready.range);
  const panned = await send(page, pointer("move", 280, 31));
  expect(panned.range).not.toEqual(ready.range);
  await send(page, pointer("up", 280, 31));

  await send(page, pointer("down", 260, 32));
  await send(page, pointer("down", 320, 33));
  await send(page, pointer("down", 390, 34));
  await send(page, pointer("cancel", 390, 34));
  const pinched = await send(page, pointer("move", 350, 33));
  expect(pinched.bar_spacing).not.toBeCloseTo(panned.bar_spacing, 8);
  await send(page, pointer("up", 350, 33));
  const rebased = await send(page, pointer("move", 280, 32));
  const continued = await send(page, pointer("move", 300, 32));
  expect(continued.range).not.toEqual(rebased.range);
  await send(page, pointer("up", 300, 32));
});

test("worker touch long press enters engine crosshair tracking and exits on the next tap", async ({ page }) => {
  const ready = await create_worker_chart(page, "canvas2d");
  const pointer = (type, x, pointer_id) => ({
    type: "pointer", event: { type, x, y: 150, pointer_id, pointer_type: "touch", buttons: type === "move" ? 1 : 0 },
  });
  await send(page, pointer("down", 250, 41));
  await page.waitForTimeout(280);
  const held = await send(page, { type: "state" });
  expect(held.crosshair).not.toBeNull();
  await send(page, pointer("move", 280, 41));
  const moved = await send(page, pointer("move", 310, 41));
  expect(moved.range).toEqual(ready.range);
  expect(moved.crosshair[0]).toBeGreaterThan(held.crosshair[0]);
  const released = await send(page, pointer("up", 310, 41));
  expect(released.crosshair).toEqual(moved.crosshair);
  await send(page, pointer("down", 285, 42));
  const exited = await send(page, pointer("up", 285, 42));
  expect(exited.crosshair).toBeNull();
});

test("worker ignores release and cancel from a different pointer", async ({ page }) => {
  const ready = await create_worker_chart(page, "canvas2d");
  await send(page, { type: "pointer", event: { type: "down", x: 250, y: 150, pointer_id: 7 } });
  await send(page, { type: "pointer", event: { type: "up", x: 250, y: 150, pointer_id: 9 } });
  await send(page, { type: "pointer", event: { type: "cancel", x: 250, y: 150, pointer_id: 9 } });
  await send(page, { type: "pointer", event: { type: "move", x: 270, y: 150, pointer_id: 7, buttons: 1 } });
  const moved = await send(page, { type: "pointer", event: { type: "move", x: 300, y: 150, pointer_id: 7, buttons: 1 } });
  expect(moved.range).not.toEqual(ready.range);
  await send(page, { type: "pointer", event: { type: "up", x: 300, y: 150, pointer_id: 7 } });
});

test("worker double-click resets time and price axes through the controller", async ({ page }) => {
  const ready = await create_worker_chart(page, "canvas2d");
  await send(page, { type: "bar_spacing", value: 20 });
  await send(page, { type: "pointer", event: {
    type: "down", x: 320, y: ready.time_axis_y, pointer_id: 21, click_count: 2,
  } });
  const time_reset = await send(page, { type: "pointer", event: {
    type: "up", x: 320, y: ready.time_axis_y, pointer_id: 21,
  } });
  expect(time_reset.bar_spacing).toBe(6);

  const manual = await send(page, { type: "price_range", from: 80, to: 120 });
  await send(page, { type: "pointer", event: {
    type: "down", x: manual.axis_x, y: manual.axis_y, pointer_id: 22, click_count: 2,
  } });
  const price_reset = await send(page, { type: "pointer", event: {
    type: "up", x: manual.axis_x, y: manual.axis_y, pointer_id: 22,
  } });
  expect(price_reset.price_range).not.toEqual(manual.price_range);
});

test("worker second press at the same pane point can become a pan", async ({ page }) => {
  const ready = await create_worker_chart(page, "canvas2d");
  const pointer = (type, x, buttons = 0) => ({ type: "pointer", event: {
    type, x, y: 150, pointer_id: 71, buttons,
  } });
  await send(page, pointer("down", 250));
  await send(page, pointer("up", 250));
  await send(page, pointer("down", 250));
  await send(page, pointer("move", 275, 1));
  const moved = await send(page, pointer("move", 310, 1));
  await send(page, pointer("up", 310));
  expect(moved.range).not.toEqual(ready.range);
});

test("worker frames continue while the main thread is blocked for 500 ms", async ({ page }) => {
  test.setTimeout(120_000);
  const ready = await create_worker_chart(page, undefined);
  const started = await send(page, { type: "start", width: 640, height: 360 });

  await page.evaluate(() => {
    const until = performance.now() + 550;
    while (performance.now() < until) {
      // Deliberately occupy the window event loop; the dedicated worker must keep presenting.
    }
  });

  const stopped = await send(page, { type: "stop" });
  console.log(`offscreen blocked-main result: ${JSON.stringify({
    worker_frames: stopped.frame - started.frame,
    presented_frames: stopped.stats.presented_frames - started.stats.presented_frames,
    cpu_ms: stopped.stats.cpu_ms,
  })}`);
  expect(stopped.frame - started.frame).toBeGreaterThanOrEqual(10);
  expect(stopped.stats.presented_frames - started.stats.presented_frames).toBeGreaterThanOrEqual(10);
  expect(stopped.stats.dropped_frames).toBe(ready.stats.dropped_frames);
});

test("runtime WebGPU loss notifies the owner to reveal the warm Canvas2D surface", async ({ page }) => {
  const ready = await create_worker_chart(page, undefined);
  expect(ready.backend).toBe("webgpu");

  const changed = await send(page, { type: "simulate_loss" });
  expect(changed.type).toBe("backend_change");
  expect(changed.backend).toBe("canvas2d");
  expect(changed.stats.canvas2d_ops).toBeGreaterThan(0);
  const visibility = await page.evaluate(() => {
    const [gpu, fallback] = document.querySelectorAll("#offscreen-worker-host canvas");
    return [gpu.style.visibility, fallback.style.visibility];
  });
  expect(visibility).toEqual(["hidden", "visible"]);
});

test("OffscreenCanvas keeps the Canvas2D fallback path renderable", async ({ page }) => {
  const ready = await create_worker_chart(page, "canvas2d");
  expect(ready.backend).toBe("canvas2d");
  expect(ready.stats.presented_frames).toBeGreaterThan(0);
  expect(ready.stats.canvas2d_ops).toBeGreaterThan(0);
  const moved = await send(page, {
    type: "pointer",
    event: { type: "move", x: 300, y: 140, pointer_id: 1 },
  });
  expect(moved.stats.presented_frames).toBeGreaterThan(ready.stats.presented_frames);
  expect(moved.stats.canvas2d_ops).toBeGreaterThan(0);
});

test("OffscreenCanvas exposes atomic timestamp rejection diagnostics", async ({ page }) => {
  await create_worker_chart(page, "canvas2d");
  const result = await send(page, { type: "invalid_timestamp" });
  expect(result.type).toBe("timestamp_diagnostics");
  expect(result.diagnostics).toMatchObject({ status: "rejected", accepted: 0 });
  expect(result.diagnostics.reason).toContain("milliseconds");
});
