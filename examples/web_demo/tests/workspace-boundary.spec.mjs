import { test, expect } from "@playwright/test";
import { readFile } from "node:fs/promises";

test("WASM workspace rejects invalid mutations without losing addressable cells", async ({ page }) => {
  // Exercise the generated wasm-bindgen boundary, including its u32 argument conversion.
  const package_root = new URL("../../../packages/charts/pkg/", import.meta.url);
  await page.route("**/__workspace_test/**", async (route) => {
    const relative = new URL(route.request().url()).pathname.split("/__workspace_test/")[1];
    const file = new URL(relative, package_root);
    if (!file.href.startsWith(package_root.href)) return route.abort();
    await route.fulfill({
      body: await readFile(file),
      contentType: relative.endsWith(".wasm") ? "application/wasm" : "text/javascript",
    });
  });
  await page.goto("/");
  const result = await page.evaluate(async () => {
    const { default: init, AerisWorkspace } = await import("/__workspace_test/aeris_charts_wasm.js");
    await init();
    const workspace = new AerisWorkspace(0);
    try {
      workspace.split(1, "horizontal", 0);
      const before = workspace.layout_json();
      const invalid_resizes = [NaN, Infinity, -Infinity].map((delta) => ({
        accepted: workspace.resize_between(1, 2, delta),
        unchanged: workspace.layout_json() === before,
      }));
      const restored = workspace.restore_layout_json('{"kind":"cell","id":4294967294}');
      const last_id = Number(workspace.split(4294967294, "horizontal", 0));
      const full = workspace.layout_json();
      const rejected = Number(workspace.split(last_id, "vertical", 0));
      const unchanged = workspace.layout_json() === full;
      const removed = workspace.remove(last_id);
      return { invalid_resizes, restored, last_id, rejected, unchanged, removed, count: workspace.chart_count() };
    } finally {
      workspace.free();
    }
  });
  expect(result).toEqual({
    invalid_resizes: Array(3).fill({ accepted: false, unchanged: true }),
    restored: true, last_id: 4294967295, rejected: -1,
    unchanged: true, removed: true, count: 1,
  });
});
