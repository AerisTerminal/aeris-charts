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

  const resized = await send(page, { type: "resize", width: 500, height: 300, dpr: 2 });
  expect(resized.size).toEqual([1_000, 600]);
  expect(resized.stats.presented_frames).toBeGreaterThan(keyed.stats.presented_frames);
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
