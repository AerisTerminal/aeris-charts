import { test, expect } from "@playwright/test";

test("forced adapter failure is classified per chart and warns once", async ({ page }) => {
  const warnings = [];
  page.on("console", (message) => {
    if (message.type() === "warning" && message.text().startsWith("nucleuscharts: WebGPU fallback")) {
      warnings.push(message.text());
    }
  });

  await page.goto("/?backend=canvas2d");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  const statuses = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/nucleuscharts_financial.js");
    const entries = await Promise.all(Array.from({ length: 4 }, async (_, index) => {
      const host = document.createElement("div");
      host.style.cssText = `width:160px;height:100px;position:absolute;left:${-10000 - index * 200}px`;
      document.body.append(host);
      const chart = await create_chart(host, {
        autoSize: false,
        accessibility: false,
        __simulate_webgpu_adapter_failure: true,
      });
      return { chart, host };
    }));
    const result = entries.map(({ chart }) => {
      const status = chart.backend_status();
      return {
        backend: chart.backend(),
        status,
        frozen: Object.isFrozen(status),
      };
    });
    for (const { chart, host } of entries) {
      chart.remove();
      host.remove();
    }
    return result;
  });

  expect(statuses).toHaveLength(4);
  for (const { backend, status, frozen } of statuses) {
    expect(backend).toBe("canvas2d");
    expect(frozen).toBe(true);
    expect(status).toMatchObject({
      requested_backend: "auto",
      active_backend: "canvas2d",
      stage: "adapter_acquisition",
      reason: "adapter_unavailable",
      secure_context: true,
      navigator_gpu: expect.any(Boolean),
      detail: "webgpu found no adapters",
    });
  }
  expect(warnings).toHaveLength(1);
  expect(warnings[0]).toContain("stage=adapter_acquisition reason=adapter_unavailable");
});

test("explicit Canvas2D status does not request or warn about WebGPU", async ({ page }) => {
  const warnings = [];
  page.on("console", (message) => {
    if (message.type() === "warning" && message.text().startsWith("nucleuscharts: WebGPU fallback")) {
      warnings.push(message.text());
    }
  });

  await page.goto("/?backend=canvas2d");
  await page.waitForFunction(() => window.__chart?.backend_status?.() !== undefined);
  const status = await page.evaluate(() => window.__chart.backend_status());

  expect(status).toMatchObject({
    requested_backend: "canvas2d",
    active_backend: "canvas2d",
    stage: "backend_selection",
    reason: "canvas2d_requested",
  });
  expect(status.detail).toBeUndefined();
  expect(warnings).toHaveLength(0);
});

test("forced adapter failure remains chart-local after shared WebGPU is ready", async ({ page }) => {
  await page.goto("/");
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined);
  test.skip(await page.evaluate(() => window.__chart.backend() !== "webgpu"), "WebGPU unavailable");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/nucleuscharts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "width:160px;height:100px;position:absolute;left:-10000px";
    document.body.append(host);
    const chart = await create_chart(host, {
      autoSize: false,
      accessibility: false,
      __simulate_webgpu_adapter_failure: true,
    });
    const status = chart.backend_status();
    const primary_backend = window.__chart.backend();
    chart.remove();
    host.remove();
    return { primary_backend, backend: status.active_backend, reason: status.reason };
  });

  expect(result).toEqual({
    primary_backend: "webgpu",
    backend: "canvas2d",
    reason: "adapter_unavailable",
  });
});

test("device loss updates backend diagnostics before Canvas2D continues", async ({ page }) => {
  await page.goto("/");
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined);
  test.skip(await page.evaluate(() => window.__chart.backend() !== "webgpu"), "WebGPU unavailable");

  const status = await page.evaluate(async () => {
    window.__chart.wasm.simulate_device_loss_for_test();
    window.__chart.render();
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    return window.__chart.backend_status();
  });

  expect(status).toMatchObject({
    requested_backend: "auto",
    active_backend: "canvas2d",
    stage: "runtime",
    reason: "device_lost",
    detail: "WebGPU device was lost",
  });
});
