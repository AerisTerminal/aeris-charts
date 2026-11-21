import { test, expect } from "@playwright/test";

test("catalog exposes every categorized built-in indicator with working creation and removal", async ({ page }) => {
  await page.goto("/?backend=canvas2d");
  await page.waitForFunction(() => window.__demo_indicators);
  const ids = [
    "sma", "ema", "vwma",
    "supertrend", "parabolic_sar", "ichimoku", "adx_dmi",
    "bollinger", "keltner", "donchian", "atr",
    "rsi", "macd", "stochastic", "stochastic_rsi", "cci", "squeeze_momentum",
    "volume", "relative_volume", "vwap", "vwap_bands", "twap", "obv", "cmf", "mfi", "volume_profile",
    "session_levels", "previous_period_levels", "opening_range", "adr_levels", "pivot_points", "swing_points", "mtf_ema_4h",
  ];
  await expect(page.locator("#indicator_catalog .indicator-category")).toHaveCount(6);
  await expect(page.locator("#indicator_catalog input")).toHaveCount(ids.length);
  for (const id of ids) {
    await page.locator(`#${id}_toggle`).check();
    await expect(page.locator("#indicator_error")).toBeEmpty();
    const valid = await page.evaluate((id) => {
      const outputs = window.__demo_indicators.get(id).outputs;
      return id === "volume_profile"
        ? outputs[0].snapshot().bar_count > 0
        : outputs.every((output) => output.indicator_info() !== null && output.data().length > 0 &&
          (id !== "swing_points" || Array.isArray(window.__chart.study_annotations(output).markers)) &&
          (id !== "volume" || output.pane_index() > 0) &&
          (id !== "mtf_ema_4h" || output.indicator_info().mtf_interval_seconds === 14400));
    }, id);
    expect(valid, id).toBe(true);
    await page.locator(`#${id}_toggle`).uncheck();
    expect(await page.evaluate(() => window.__demo_indicators.size)).toBe(0);
  }
  await page.locator("#indicator_search").fill("volume profile");
  await expect(page.locator("#indicator_catalog label:visible")).toHaveCount(1);
  await expect(page.locator("#indicator_catalog .indicator-category:visible")).toHaveCount(1);
  await page.locator("#indicator_search").fill("momentum");
  expect(await page.locator("#indicator_catalog label:visible").count()).toBeGreaterThanOrEqual(6);
  await page.locator("#indicator_search").fill("no matching formula");
  await expect(page.locator("#indicator_empty")).toBeVisible();
});
