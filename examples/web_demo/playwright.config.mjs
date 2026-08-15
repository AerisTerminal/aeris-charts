import { defineConfig, devices } from "@playwright/test";

const port = Number.parseInt(process.env.NUCLEUSCHARTS_TEST_PORT ?? "4174", 10);
const portable_browser = process.env.NUCLEUSCHARTS_PORTABLE_BROWSER === "1";
const linux_webgpu_args = process.platform === "linux"
  ? ["--use-angle=vulkan", "--enable-features=Vulkan", "--disable-vulkan-surface"]
  : [];

export default defineConfig({
  testDir: "./tests",
  fullyParallel: false,
  workers: 1,
  reporter: [["list"], ["html", { open: "never" }]],
  outputDir: "test-results",
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    viewport: { width: 1280, height: 720 },
    deviceScaleFactor: 1.5,
    colorScheme: "light",
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
  },
  projects: [
    {
      // Chromium runs the full suite: the WebGPU backend (SwiftShader adapter) plus the shared
      // Canvas2D-fallback smoke. The WebGPU launch flags are Chromium-specific.
      name: "chromium",
      // These suites intentionally depend on a calibrated machine/GPU, exact raster output, or
      // wall-clock budgets. CI runs them separately as evidence; all other runtime/parity tests
      // are the portable publication gate.
      testIgnore: portable_browser
        ? /(backend-parity|engine-bench|gpui-webgpu-matrix|perf-gate)\.spec\.mjs/
        : undefined,
      grepInvert: portable_browser ? /@machine/ : undefined,
      use: {
        channel: "chromium",
        launchOptions: {
          args: [
            "--enable-unsafe-webgpu",
            "--enable-unsafe-swiftshader",
            "--use-webgpu-adapter=swiftshader",
            ...linux_webgpu_args,
            "--enable-dawn-features=allow_unsafe_apis",
            "--disable-dawn-features=use_dxc",
            "--enable-webgpu-developer-features",
            "--use-gpu-in-tests",
            "--enable-accelerated-2d-canvas",
          ],
        },
      },
    },
    {
      // Firefox and WebKit have no headless WebGPU here, so they run only the Canvas2D-fallback
      // smoke — confirming the library loads and renders on those engines.
      name: "firefox",
      use: { ...devices["Desktop Firefox"] },
      testMatch: /cross-browser\.spec\.mjs/,
    },
    {
      name: "webkit",
      use: { ...devices["Desktop Safari"] },
      testMatch: /cross-browser\.spec\.mjs/,
    },
  ],
  webServer: {
    command: "node test_server.mjs",
    url: `http://127.0.0.1:${port}`,
    reuseExistingServer: !process.env.CI,
    timeout: 30_000,
  },
});
