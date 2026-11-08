import { defineConfig, devices } from "@playwright/test";

const port = Number.parseInt(process.env.AERIS_CHARTS_TEST_PORT ?? "4174", 10);
const portable_browser = process.env.AERIS_CHARTS_PORTABLE_BROWSER === "1";
// A second server answers the shared bundle path with the complete build (see test_server.mjs).
const complete_port = Number.parseInt(process.env.AERIS_CHARTS_COMPLETE_TEST_PORT ?? String(port + 1), 10);
const chromium_webgpu = {
  channel: "chromium",
  launchOptions: {
    args: [
      "--enable-unsafe-webgpu",
      "--enable-unsafe-swiftshader",
      "--use-webgpu-adapter=swiftshader",
      "--enable-dawn-features=allow_unsafe_apis",
      "--disable-dawn-features=use_dxc",
      "--enable-webgpu-developer-features",
      "--use-gpu-in-tests",
      "--enable-accelerated-2d-canvas",
    ],
  },
};

export default defineConfig({
  testDir: "./tests",
  globalSetup: "./global_setup.mjs",
  // GitHub's shared Windows runners are substantially slower than release developer machines.
  // Trace evidence from shared runners showed 13s key presses and 28s visibility checks.
  // Keep local feedback tight while allowing the same assertions to finish in CI.
  timeout: process.env.CI ? 120_000 : 30_000,
  expect: { timeout: process.env.CI ? 30_000 : 5_000 },
  // Shared Windows CI runners can intermittently stall browser startup or GPU work.
  // Retry once in a fresh worker; every assertion must still pass.
  retries: process.env.CI ? 1 : 0,
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
      // Timing budgets and the native GPUI matrix remain machine evidence. Deterministic
      // browser/backend pixel comparisons run in the portable publication gate.
      // g11_statistics, g12_relationships, g13_maps and g14_3d need the complete build; they run in
      // `chromium-complete`.
      testIgnore: portable_browser
        ? /(engine_bench|gpui_webgpu_matrix|perf_gate|g11_statistics|g12_relationships|g13_maps|g14_3d)\.spec\.mjs/
        : /(g11_statistics|g12_relationships|g13_maps|g14_3d)\.spec\.mjs/,
      grepInvert: portable_browser ? /@machine/ : undefined,
      use: chromium_webgpu,
    },
    {
      // The complete build (`@aeristerminal/aeris-charts/complete`) runs a representative slice of
      // the shared suite: families both builds contain must behave identically in it.
      name: "chromium-complete",
      testMatch: /[\\/](build_variant|general_charts|g8_defaults|accessibility|license_attribution|g11_statistics|g12_relationships|g13_maps|g14_3d)\.spec\.mjs$/,
      grepInvert: portable_browser ? /@machine/ : undefined,
      use: { ...chromium_webgpu, baseURL: `http://127.0.0.1:${complete_port}` },
    },
    {
      // Firefox and WebKit have no headless WebGPU here, so they run only the Canvas2D-fallback
      // smoke — confirming the library loads and renders on those engines.
      name: "firefox",
      use: { ...devices["Desktop Firefox"] },
      testMatch: /(cross_browser|financial_compatibility|general_charts|g1_foundations|g8_defaults|unified_interaction_accessibility|public_reference_interaction|g9_timeline_calendar|g9_business)\.spec\.mjs/,
    },
    {
      name: "webkit",
      use: { ...devices["Desktop Safari"] },
      testMatch: /(cross_browser|financial_compatibility|general_charts|g1_foundations|g8_defaults|unified_interaction_accessibility|public_reference_interaction|g9_timeline_calendar|g9_business)\.spec\.mjs/,
    },
  ],
  webServer: [
    {
      command: "node test_server.mjs",
      url: `http://127.0.0.1:${port}`,
      reuseExistingServer: !process.env.CI,
      timeout: 30_000,
    },
    {
      command: "node test_server.mjs",
      url: `http://127.0.0.1:${complete_port}`,
      env: { ...process.env, AERIS_CHARTS_TEST_PORT: String(complete_port), AERIS_CHARTS_BUILD: "complete" },
      reuseExistingServer: !process.env.CI,
      timeout: 30_000,
    },
  ],
});
