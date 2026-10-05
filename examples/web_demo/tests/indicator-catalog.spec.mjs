import { test, expect } from "@playwright/test";

test("catalog exposes every built-in indicator with working creation and removal", async ({ page }) => {
  await page.goto("/?backend=canvas2d");
  await page.waitForFunction(() => window.__demo_indicators);
  const ids = ["sma", "ema", "wma", "ema_ribbon", "bollinger", "rsi", "macd", "stochastic", "atr", "vwap", "volume_profile", "swing_points", "market_structure", "fair_value_gaps", "order_blocks", "session_levels", "previous_period_levels", "opening_range"];
  await expect(page.locator("#indicator_catalog input")).toHaveCount(ids.length);
  for (const id of ids) {
    await page.locator(`#${id}_toggle`).check();
    await expect(page.locator("#indicator_error")).toBeEmpty();
    const valid = await page.evaluate((id) => {
      const outputs = window.__demo_indicators.get(id).outputs;
      return id === "volume_profile"
        ? outputs[0].snapshot().bar_count > 0
        : outputs.every((output) => output.indicator_info() !== null && output.data().length > 0 &&
          (!["swing_points", "market_structure", "fair_value_gaps", "order_blocks"].includes(id) ||
            (Array.isArray(window.__chart.study_annotations(output).markers) &&
              Array.isArray(window.__chart.study_annotations(output).zones))));
    }, id);
    expect(valid, id).toBe(true);
    await page.locator(`#${id}_toggle`).uncheck();
    expect(await page.evaluate(() => window.__demo_indicators.size)).toBe(0);
  }
  await page.locator("#indicator_search").fill("volume profile");
  await expect(page.locator("#indicator_catalog label:visible")).toHaveCount(1);
  await page.locator("#indicator_search").fill("no matching formula");
  await expect(page.locator("#indicator_empty")).toBeVisible();
});
