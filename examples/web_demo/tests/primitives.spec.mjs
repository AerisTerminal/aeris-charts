import { expect } from "@playwright/test";
import { test, wait_for_chart } from "./page_ready.mjs";
import { readFileSync } from "node:fs";
import pixelmatch from "pixelmatch";
import { PNG } from "pngjs";
import { crop_png, count_different, max_channel_delta } from "./parity_pixels.mjs";

const fixture = JSON.parse(readFileSync(new URL("../fixtures/d1/candles.json", import.meta.url), "utf8"));

test.beforeEach(async ({ page }) => {
  page.on("console", (message) => console.log(`[browser:${message.type()}] ${message.text()}`));
  page.on("pageerror", (error) => console.log(`[browser:pageerror] ${error.message}`));
});

async function settle_frames(page) {
  await page.evaluate(() => new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  }));
}

// Reference pane primitive exercising the three z-order layers plus both axis-label surfaces.
// It deliberately emits quad-family commands only (rect/hline/rect_frame): those paint in the
// same bucket order on both backends, so the WebGPU/Canvas2D identity assertion below isolates
// the primitive pipeline itself rather than the engines' tri/quad bucket split.
function reference_primitive_factory() {
  return {
    pane_views: () => [
      {
        z_order: "bottom",
        renderer(ctx) {
          ctx.rect(ctx.pane_left + 60, ctx.pane_top + 10, 150, ctx.pane_height - 20, "rgba(41, 98, 255, 0.12)");
        },
      },
      {
        z_order: "normal",
        renderer(ctx) {
          ctx.hline(ctx.pane_top + 40, ctx.pane_left, ctx.pane_left + ctx.pane_width, "#e91e63", 2, 0);
          ctx.vline(ctx.pane_left + 260, ctx.pane_top, ctx.pane_top + ctx.pane_height, "#e91e63", 2, 0);
        },
      },
      {
        z_order: "top",
        renderer(ctx) {
          // Opaque on purpose: translucent fills can land on a 0.5 alpha-blend rounding tie
          // that the two rasterizers resolve a channel unit apart (a backend artifact the
          // cross-backend fixture never exercises), which is outside this pipeline's scope.
          ctx.rect_frame(ctx.pane_left + 300, ctx.pane_top + 60, 140, 70, "#e91e63", 2);
        },
      },
    ],
    price_axis_views: () => [{ text: "PBAND", coordinate: 40, color: "#e91e63" }],
    time_axis_views: () => [{ text: "P1", coordinate: 120, color: "#e91e63" }],
  };
}

async function goto_fixture(page, backend) {
  await page.goto(`/?runtimeTest=presentedFrame&backend=${backend}&forceFallbackAdapter=1`);
  await wait_for_chart(page);
}

async function attach_reference_primitive(page) {
  await page.evaluate((factory_source) => {
    // eslint-disable-next-line no-eval
    const factory = eval(`(${factory_source})`);
    window.__reference_primitive_handle = window.__chart.panes()[0].attach_primitive(factory());
  }, reference_primitive_factory.toString());
  await settle_frames(page);
}

async function detach_reference_primitive(page) {
  await page.evaluate(() => {
    window.__reference_primitive_handle.detach();
    window.__reference_primitive_handle = null;
  });
  await settle_frames(page);
}

test("pane primitive paints identically on both backends, changes its regions, and detaches cleanly", async ({ page }, test_info) => {
  const pixel_ratio = fixture.pixel_ratio;
  const pane_width = Math.round((fixture.css_width - fixture.price_axis_width) * pixel_ratio);
  const pane_height = Math.round((fixture.css_height - fixture.time_axis_height) * pixel_ratio);

  // ---- Canvas2D: baseline → attach → detach ----
  await goto_fixture(page, "canvas2d");
  expect(await page.evaluate(() => window.__chart.backend())).toBe("canvas2d");
  const canvas_before = PNG.sync.read(await page.screenshot({ animations: "disabled", fullPage: false }));
  await attach_reference_primitive(page);
  const canvas_attached = PNG.sync.read(await page.screenshot({ animations: "disabled", fullPage: false }));

  // (b) The primitive actually painted: the pane region (bands/lines/frame), the price-axis
  // strip (boxed PBAND label), and the time strip (boxed P1 label) all changed.
  const pane_diff = count_different(
    crop_png(canvas_before, 0, 0, pane_width, pane_height),
    crop_png(canvas_attached, 0, 0, pane_width, pane_height),
  );
  expect(pane_diff, "pane region must change where the primitive draws").toBeGreaterThan(0);
  const price_axis_diff = count_different(
    crop_png(canvas_before, pane_width, 0, canvas_before.width - pane_width, pane_height),
    crop_png(canvas_attached, pane_width, 0, canvas_attached.width - pane_width, pane_height),
  );
  expect(price_axis_diff, "price axis must gain the primitive's boxed label").toBeGreaterThan(0);
  const time_axis_diff = count_different(
    crop_png(canvas_before, 0, pane_height, pane_width, canvas_before.height - pane_height),
    crop_png(canvas_attached, 0, pane_height, pane_width, canvas_attached.height - pane_height),
  );
  expect(time_axis_diff, "time axis must gain the primitive's boxed label").toBeGreaterThan(0);

  // (c) Detach restores the exact prior pixels (same backend, deterministic render).
  await detach_reference_primitive(page);
  const canvas_restored = PNG.sync.read(await page.screenshot({ animations: "disabled", fullPage: false }));
  expect(count_different(canvas_before, canvas_restored)).toBe(0);

  // ---- WebGPU: same chart + same primitive → identical presented frame ----
  await goto_fixture(page, "auto");
  expect(await page.evaluate(() => window.__chart.backend())).toBe("webgpu");
  await attach_reference_primitive(page);
  const gpu_attached = PNG.sync.read(await page.screenshot({ animations: "disabled", fullPage: false }));

  // (a) Primitive pane geometry remains byte-identical; shared axis text may differ only at
  // bounded fractional-DPR antialiasing edges.
  const pane_backend_diff = count_different(
    crop_png(gpu_attached, 0, 0, pane_width, pane_height),
    crop_png(canvas_attached, 0, 0, pane_width, pane_height),
  );
  expect(pane_backend_diff, "pane primitive geometry must remain pixel-identical").toBe(0);
  const backend_diff = count_different(gpu_attached, canvas_attached);
  const backend_max_delta = max_channel_delta(gpu_attached, canvas_attached);
  console.log(`pane-primitive full-frame residual: ${backend_diff} px, max delta ${backend_max_delta}`);
  if (backend_diff > 2_600 || backend_max_delta > 40) {
    await test_info.attach("webgpu.png", { body: PNG.sync.write(gpu_attached), contentType: "image/png" });
    await test_info.attach("canvas2d.png", { body: PNG.sync.write(canvas_attached), contentType: "image/png" });
  }
  // Windows SwiftShader measurement: 2,307 pixels, max delta 32.
  expect(backend_diff, "full-frame differences must stay confined to bounded AA edges").toBeLessThanOrEqual(2_600);
  expect(backend_max_delta).toBeLessThanOrEqual(40);

  // Sanity: the demo's built-in session-bands primitive (z_order "bottom") also toggles.
  await page.evaluate(() => window.__set_day_bands(true));
  await settle_frames(page);
  expect(await page.evaluate(() => window.__day_bands_active())).toBe(true);
  const gpu_bands = PNG.sync.read(await page.screenshot({ animations: "disabled", fullPage: false }));
  expect(count_different(gpu_attached, gpu_bands)).toBeGreaterThan(0);
  await page.evaluate(() => window.__set_day_bands(false));
  await settle_frames(page);
  expect(await page.evaluate(() => window.__day_bands_active())).toBe(false);
});

test("text_views are clipped to their owning pane and cannot cover axis chrome", async ({ page }) => {
  const pixel_ratio = fixture.pixel_ratio;
  const pane_width = Math.round((fixture.css_width - fixture.price_axis_width) * pixel_ratio);
  const pane_height = Math.round((fixture.css_height - fixture.time_axis_height) * pixel_ratio);
  await goto_fixture(page, "canvas2d");
  const before = PNG.sync.read(await page.screenshot({ animations: "disabled", fullPage: false }));

  await page.evaluate(() => {
    window.__edge_text_handle = window.__chart.panes()[0].attach_primitive({
      text_views: (info) => [{
        text: "CLIPPED AT AXIS",
        x: info.pane_left + info.pane_width - 5,
        y: info.pane_top + 80,
        color: "#ff00ff",
        size: 18,
        align: "left",
        baseline: "middle",
      }],
    });
  });
  await settle_frames(page);
  const attached = PNG.sync.read(await page.screenshot({ animations: "disabled", fullPage: false }));

  expect(count_different(
    crop_png(before, 0, 0, pane_width, pane_height),
    crop_png(attached, 0, 0, pane_width, pane_height),
  ), "the in-pane edge of the text must remain visible").toBeGreaterThan(0);
  expect(count_different(
    crop_png(before, pane_width, 0, before.width - pane_width, pane_height),
    crop_png(attached, pane_width, 0, attached.width - pane_width, pane_height),
  ), "plugin text must not alter the price-axis strip").toBe(0);

  await page.evaluate(() => window.__edge_text_handle.detach());
  await settle_frames(page);
  const restored = PNG.sync.read(await page.screenshot({ animations: "disabled", fullPage: false }));
  expect(count_different(before, restored)).toBe(0);
});

test("plugin text_views enter the ordered pane frame used by pane-only screenshots", async ({ page }) => {
  await goto_fixture(page, "canvas2d");
  const result = await page.evaluate(() => {
    const chart = window.__chart;
    const handle = chart.panes()[0].attach_primitive({
      text_views: (info) => [{
        text: "FRAME TEXT", x: info.pane_left + 75, y: info.pane_top + 80,
        font: "italic bold 24px Arial", color: "#ff00ff", baseline: "alphabetic",
      }],
    });
    const count_magenta = () => {
      const screenshot = chart.take_screenshot(false, false);
      const pixels = screenshot.getContext("2d").getImageData(0, 0, screenshot.width, screenshot.height).data;
      let magenta = 0;
      for (let index = 0; index < pixels.length; index += 4) {
        if (pixels[index] > 220 && pixels[index + 1] < 80 && pixels[index + 2] > 220 && pixels[index + 3] > 200) magenta++;
      }
      return magenta;
    };
    const painted = count_magenta();
    const cover = chart.panes()[0].attach_primitive({
      pane_views: () => [{
        z_order: "top",
        renderer(ctx) { ctx.rect(ctx.pane_left + 50, ctx.pane_top + 45, 260, 55, "#000000"); },
      }],
    });
    const covered = count_magenta();
    cover.detach();
    handle.detach();
    return { painted, covered };
  });
  expect(result.painted).toBeGreaterThan(30);
  expect(result.covered).toBe(0);
});

test("plugin text_views paint through the WebGPU presented frame", async ({ page }) => {
  await goto_fixture(page, "auto");
  expect(await page.evaluate(() => window.__chart.backend())).toBe("webgpu");
  await page.evaluate(() => {
    window.__frame_text_handle = window.__chart.panes()[0].attach_primitive({
      text_views: (info) => [{
        text: "GPU FRAME", x: info.pane_left + 75, y: info.pane_top + 80,
        font: "italic bold 24px Arial", color: "#ff00ff", baseline: "alphabetic",
      }],
    });
  });
  await settle_frames(page);
  const image = PNG.sync.read(await page.screenshot({ animations: "disabled", fullPage: false }));
  let magenta = 0;
  for (let index = 0; index < image.data.length; index += 4) {
    if (image.data[index] > 220 && image.data[index + 1] < 80 && image.data[index + 2] > 220 && image.data[index + 3] > 200) magenta++;
  }
  expect(magenta).toBeGreaterThan(30);
});
