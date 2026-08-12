import { test, expect } from "@playwright/test";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import pixelmatch from "pixelmatch";
import { PNG } from "pngjs";

const enabled = process.env.NUCLEUSCHARTS_RUN_GPUI_WEBGPU_MATRIX === "1";
const fixture = JSON.parse(readFileSync(new URL("../fixtures/d1/candles.json", import.meta.url), "utf8"));
const repository_root = fileURLToPath(new URL("../../..", import.meta.url));
const matrix_cases = [
  {
    name: "dpr-1_5-spacing-fit-light-base",
    theme: "light",
    spacing: null,
    feature: "base",
    exact: true,
    webgpu_rgba_sha256: "a8d1a0343f76fb44d0ca99403dfc6e0204598300346d3e3ef67e4220afaaeecc",
  },
  {
    name: "dpr-1_5-spacing-0_5-light-base",
    theme: "light",
    spacing: 0.5,
    feature: "base",
    exact: true,
    webgpu_rgba_sha256: "04e6d8d3dc66e213d1b97f35721292a3869c15e345cf88b860e5738f352520f3",
  },
  {
    name: "dpr-1_5-spacing-6-light-base",
    theme: "light",
    spacing: 6,
    feature: "base",
    exact: true,
    webgpu_rgba_sha256: "de10d01e7634124b12ce2006b18393a5270805fefabed2849fbe4f727bc69a11",
  },
  {
    name: "dpr-1_5-spacing-50-light-base",
    theme: "light",
    spacing: 50,
    feature: "base",
    exact: true,
    webgpu_rgba_sha256: "bad3dbf54adb6bf35d281f77bf0feeaee5d9b70914f2af1d6a505cf89d29fe30",
  },
  {
    name: "dpr-1_5-spacing-fit-dark-base",
    theme: "dark",
    spacing: null,
    feature: "base",
    exact: true,
    webgpu_rgba_sha256: "c01daa07be767633caef5993f1a3ec2b3c4a8c7acbd8644748ae663fe76f534d",
  },
  {
    name: "dpr-1_5-spacing-6-light-markers",
    theme: "light",
    spacing: 6,
    feature: "markers",
    exact: false,
    webgpu_rgba_sha256: "48221c47757b6ffd7b3d2fa6d629015b3d9550343326f501d3abd4d32136078c",
  },
];

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function crop_png(source, width, height) {
  const output = new PNG({ width, height });
  PNG.bitblt(source, output, 0, 0, width, height, 0, 0);
  return output;
}

function compare_images(webgpu, gpui) {
  expect([gpui.width, gpui.height]).toEqual([webgpu.width, webgpu.height]);
  let differing_pixels = 0;
  let maximum_channel_delta = 0;
  let absolute_channel_delta = 0;
  for (let offset = 0; offset < webgpu.data.length; offset += 4) {
    let pixel_delta = 0;
    for (let channel = 0; channel < 4; channel += 1) {
      const delta = Math.abs(webgpu.data[offset + channel] - gpui.data[offset + channel]);
      pixel_delta = Math.max(pixel_delta, delta);
      maximum_channel_delta = Math.max(maximum_channel_delta, delta);
      absolute_channel_delta += delta;
    }
    if (pixel_delta !== 0) differing_pixels += 1;
  }

  const visual = new PNG({ width: webgpu.width, height: webgpu.height });
  const perceptual_pixels = pixelmatch(
    webgpu.data,
    gpui.data,
    visual.data,
    webgpu.width,
    webgpu.height,
    { threshold: 0.1, includeAA: false },
  );
  const total_pixels = webgpu.width * webgpu.height;
  return {
    differing_pixels,
    different_fraction: differing_pixels / total_pixels,
    maximum_channel_delta,
    mean_absolute_channel_delta: absolute_channel_delta / webgpu.data.length,
    perceptual_pixels,
    perceptual_fraction: perceptual_pixels / total_pixels,
    total_pixels,
    visual,
  };
}

function capture_gpui(output, metadata, matrix_case) {
  const args = [
    "run",
    "--release",
    "-p",
    "nucleuscharts_render_gpui",
    "--features",
    "gpui-backend",
    "--example",
    "gpui_pane_capture",
  ];
  const env = {
    ...process.env,
    NUCLEUSCHARTS_GPUI_CAPTURE_OUT: output,
    NUCLEUSCHARTS_GPUI_CAPTURE_METADATA: metadata,
    NUCLEUSCHARTS_GPUI_THEME: matrix_case.theme,
    NUCLEUSCHARTS_GPUI_FEATURE: matrix_case.feature,
  };
  delete env.NUCLEUSCHARTS_GPUI_BAR_SPACING;
  if (matrix_case.spacing !== null) env.NUCLEUSCHARTS_GPUI_BAR_SPACING = String(matrix_case.spacing);
  const result = spawnSync("cargo", args, {
    cwd: repository_root,
    encoding: "utf8",
    env,
    timeout: 600_000,
  });
  expect(
    result.status,
    `finite GPUI capture failed for ${matrix_case.name}\nstdout:\n${result.stdout}\nstderr:\n${result.stderr}\nerror: ${result.error ?? "none"}`,
  ).toBe(0);
  expect(result.stdout).toContain("GPUI_PANE_CAPTURE_OK");
}

async function capture_webgpu(page, matrix_case) {
  const query = new URLSearchParams({
    runtimeTest: "presentedFrame",
    backend: "auto",
    forceFallbackAdapter: "1",
    dpr: String(fixture.pixel_ratio),
    theme: matrix_case.theme,
  });
  if (matrix_case.spacing !== null) query.set("spacing", String(matrix_case.spacing));
  if (matrix_case.feature !== "base") query.set("feature", matrix_case.feature);
  await page.goto(`/?${query}`);
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined);
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  expect(await page.evaluate(() => window.__chart.backend()), "primary reference must be WebGPU").toBe("webgpu");
  expect(await page.evaluate(() => window.devicePixelRatio)).toBe(fixture.pixel_ratio);
  return page.screenshot({ animations: "disabled", fullPage: false });
}

test.describe("GPUI versus presented WebGPU matrix", () => {
  test.skip(!enabled, "run explicitly with `npm run test:gpui-webgpu`");
  test.skip(process.platform !== "win32", "the current GPUI capture helper uses Windows DWM");

  test("D1 pane matrix at physical DPR 1.5", async ({ page }, test_info) => {
    test.setTimeout(900_000);
    page.on("console", (message) => console.log(`[browser:${message.type()}] ${message.text()}`));
    page.on("pageerror", (error) => console.log(`[browser:pageerror] ${error.message}`));

    const pane_width = Math.round((fixture.css_width - fixture.price_axis_width) * fixture.pixel_ratio);
    const pane_height = Math.round((fixture.css_height - fixture.time_axis_height) * fixture.pixel_ratio);
    const expected_page_size = [
      Math.round(fixture.css_width * fixture.pixel_ratio),
      Math.round(fixture.css_height * fixture.pixel_ratio),
    ];
    const matrix_report = [];

    for (const matrix_case of matrix_cases) {
      await test.step(matrix_case.name, async () => {
        const presented_png = await capture_webgpu(page, matrix_case);
        const presented = PNG.sync.read(presented_png);
        expect([presented.width, presented.height]).toEqual(expected_page_size);
        const webgpu = crop_png(presented, pane_width, pane_height);
        const webgpu_rgba_sha256 = sha256(webgpu.data);
        expect(
          webgpu_rgba_sha256,
          `${matrix_case.name} presented WebGPU output must match its approved canonical RGBA`,
        ).toBe(matrix_case.webgpu_rgba_sha256);

        const artifact_dir = test_info.outputPath(matrix_case.name);
        mkdirSync(artifact_dir, { recursive: true });
        const gpui_path = `${artifact_dir}/gpui.png`;
        const metadata_path = `${artifact_dir}/gpui.json`;
        capture_gpui(gpui_path, metadata_path, matrix_case);
        const metadata = JSON.parse(readFileSync(metadata_path, "utf8"));
        expect(metadata).toMatchObject({
          schema: 1,
          fixture: fixture.name,
          case: matrix_case.name,
          scope: "pane",
          theme: matrix_case.theme,
          bar_spacing: matrix_case.spacing,
          feature: matrix_case.feature,
          scale_factor: fixture.pixel_ratio,
          pixel_width: pane_width,
          pixel_height: pane_height,
        });

        const gpui_png = readFileSync(gpui_path);
        const gpui = PNG.sync.read(gpui_png);
        const comparison = compare_images(webgpu, gpui);
        const webgpu_png = PNG.sync.write(webgpu);
        const diff_png = PNG.sync.write(comparison.visual);
        writeFileSync(`${artifact_dir}/webgpu.png`, webgpu_png);
        writeFileSync(`${artifact_dir}/diff.png`, diff_png);

        const report = {
          schema: 1,
          fixture: fixture.name,
          case: matrix_case.name,
          scope: "pane",
          theme: matrix_case.theme,
          bar_spacing: matrix_case.spacing,
          feature: matrix_case.feature,
          reference: "Chromium presented WebGPU frame (page.screenshot), cropped before axes",
          candidate: "official GPUI 0.2.2 presented client area (DWM PrintWindow)",
          tolerance: 0,
          width: pane_width,
          height: pane_height,
          ...Object.fromEntries(Object.entries(comparison).filter(([key]) => key !== "visual")),
          webgpu_rgba_sha256,
          gpui_rgba_sha256: sha256(gpui.data),
          webgpu_png_sha256: sha256(webgpu_png),
          gpui_png_sha256: sha256(gpui_png),
          diff_png_sha256: sha256(diff_png),
        };
        matrix_report.push(report);
        const report_json = Buffer.from(`${JSON.stringify(report, null, 2)}\n`);
        writeFileSync(`${artifact_dir}/results.json`, report_json);

        await test_info.attach(`${matrix_case.name}-webgpu.png`, { body: webgpu_png, contentType: "image/png" });
        await test_info.attach(`${matrix_case.name}-gpui.png`, { body: gpui_png, contentType: "image/png" });
        await test_info.attach(`${matrix_case.name}-diff.png`, { body: diff_png, contentType: "image/png" });
        await test_info.attach(`${matrix_case.name}-results.json`, { body: report_json, contentType: "application/json" });
        console.log(
          `${matrix_case.name}: ${comparison.differing_pixels}/${comparison.total_pixels} `
          + `(${(comparison.different_fraction * 100).toFixed(4)}%), max ${comparison.maximum_channel_delta}, `
          + `mean ${comparison.mean_absolute_channel_delta.toFixed(4)}, perceptual `
          + `${comparison.perceptual_pixels} (${(comparison.perceptual_fraction * 100).toFixed(4)}%)`,
        );

        if (matrix_case.exact) {
          expect(comparison.differing_pixels, `${matrix_case.name} must remain byte-exact`).toBe(0);
          expect(comparison.maximum_channel_delta).toBe(0);
        } else {
          // Marker labels and circle/triangle edges use browser text/MSAA versus DirectWrite/GPUI
          // path rasterization. Keep the tolerance-zero metrics and a narrow measured envelope:
          // the July 2026 SwiftShader/Windows baseline is 1,176 exact and 483 perceptual pixels,
          // max channel delta 217. These limits retain small platform headroom while rejecting
          // pane-level mapping, clipping, paint-order, or solid-interior regressions.
          expect(comparison.differing_pixels).toBeLessThanOrEqual(1_300);
          expect(comparison.perceptual_pixels).toBeLessThanOrEqual(550);
          expect(comparison.maximum_channel_delta).toBeLessThanOrEqual(224);
        }
      });
    }

    const matrix_json = Buffer.from(`${JSON.stringify(matrix_report, null, 2)}\n`);
    writeFileSync(test_info.outputPath("gpui-webgpu-matrix-results.json"), matrix_json);
    await test_info.attach("gpui-webgpu-matrix-results.json", {
      body: matrix_json,
      contentType: "application/json",
    });
  });
});
