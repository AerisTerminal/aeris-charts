import { test, expect } from "@playwright/test";

async function open_demo(page) {
  await page.goto("/");
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined && window.__feature_lab !== undefined);
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
}

test("demo shell is responsive, icon-led, and has no horizontal control ribbon", async ({ page }) => {
  await open_demo(page);
  const layout = await page.evaluate(() => ({
    page_overflow: document.documentElement.scrollWidth > document.documentElement.clientWidth,
    controls_overflow: document.getElementById("controls_strip").scrollWidth > document.getElementById("controls_strip").clientWidth,
    chart_width: document.getElementById("chart_wrap").getBoundingClientRect().width,
    inspector_width: document.getElementById("inspector").getBoundingClientRect().width,
    icons: document.querySelectorAll("[data-icon] svg").length,
  }));
  expect(layout.page_overflow).toBe(false);
  expect(layout.controls_overflow).toBe(false);
  expect(layout.chart_width).toBeGreaterThan(700);
  expect(layout.inspector_width).toBeGreaterThanOrEqual(300);
  expect(layout.icons).toBeGreaterThan(20);
  await expect(page.locator("#theme_toggle")).toBeVisible();
  await expect(page.locator("#inspector_toggle")).toBeVisible();
});

test("feature lab exposes every first-class helper and manages series lifecycle", async ({ page }) => {
  await open_demo(page);
  await expect(page.locator("#feature_grid .feature-card")).toHaveCount(29);

  await page.locator('[data-feature-id="rounded-candles"]').click();
  await expect(page.locator('[data-feature-id="rounded-candles"]')).toHaveAttribute("aria-pressed", "true");
  expect(await page.evaluate(() => window.__feature_lab.active_ids())).toEqual(["rounded-candles"]);
  expect(await page.evaluate(() => window.__chart.series_order().filter((item) => item.series_type() === "rounded_candles").length)).toBe(1);
  expect(await page.evaluate(() => window.__main.options().visible)).toBe(false);

  await page.locator('[data-feature-id="volume-profile"]').click();
  expect(await page.evaluate(() => window.__feature_lab.active_ids().sort())).toEqual(["rounded-candles", "volume-profile"]);
  await page.locator("#feature_clear").click();
  expect(await page.evaluate(() => window.__feature_lab.active_ids())).toEqual([]);
  expect(await page.evaluate(() => window.__chart.series_order().filter((item) => item.series_type() === "rounded_candles").length)).toBe(0);
  expect(await page.evaluate(() => window.__main.options().visible)).toBe(true);
});

test("every feature-lab card activates through its real public API wiring", async ({ page }) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await open_demo(page);
  const features = await page.locator("#feature_grid .feature-card").evaluateAll((cards) =>
    cards.map((card) => ({ id: card.dataset.featureId, kind: card.dataset.featureKind })),
  );
  for (const feature of features) {
    await page.evaluate((id) => window.__feature_lab.activate(id), feature.id);
    expect(await page.evaluate((id) => window.__feature_lab.active_ids().includes(id), feature.id)).toBe(true);
    if (feature.kind === "primitive") await page.evaluate((id) => window.__feature_lab.activate(id), feature.id);
  }
  await page.evaluate(() => window.__feature_lab.clear());
  expect(await page.evaluate(() => window.__feature_lab.active_ids())).toEqual([]);
  expect(errors).toEqual([]);
});

test("brushable area drag writes a logical range into the Rust series", async ({ page }) => {
  await open_demo(page);
  await page.locator('[data-feature-id="brushable-area"]').click();
  const overlay = page.locator("#chart_container canvas").last();
  const bounds = await overlay.boundingBox();
  expect(bounds).not.toBeNull();
  const y = bounds.y + bounds.height * 0.45;
  await page.mouse.move(bounds.x + bounds.width * 0.3, y);
  await page.mouse.down();
  await page.mouse.move(bounds.x + bounds.width * 0.65, y, { steps: 8 });
  await page.mouse.up();
  const state = await page.evaluate(() => {
    const feature = window.__chart.series_order().find((item) => item.series_type() === "brushable_area");
    return {
      ranges: feature?.options().brush_ranges ?? [],
      active: window.__feature_lab.active_ids(),
    };
  });
  expect(state.active).toContain("brushable-area");
  expect(state.ranges).toHaveLength(1);
  expect(state.ranges[0].range.to).toBeGreaterThan(state.ranges[0].range.from);
  expect(state.ranges[0].style.line_color).toBe("#049981");
});

test("theme action and compact inspector remain directly usable", async ({ page }) => {
  await page.setViewportSize({ width: 760, height: 820 });
  await open_demo(page);
  const initial = await page.locator("html").getAttribute("data-theme");
  await page.locator("#theme_toggle").click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", initial === "light" ? "dark" : "light");
  await expect(page.locator("#theme_select")).toHaveValue(initial === "light" ? "dark" : "light");

  await expect(page.locator("#inspector")).not.toHaveAttribute("data-open", "true");
  await page.locator("#inspector_toggle").click();
  await expect(page.locator("#inspector")).toHaveAttribute("data-open", "true");
  expect(await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth)).toBe(false);
});
