import { test, expect } from "@playwright/test";
import { PNG } from "pngjs";

for (const backend of ["canvas2d", "webgpu"]) {
  test(`volume profile computes, renders and updates through ${backend}`, async ({ page }) => {
    await page.goto(`/?backend=${backend}`);
    await page.waitForFunction((expected) => window.__chart?.backend?.() === expected, backend);
    const initial = await page.evaluate(() => {
      const chart = window.__chart;
      window.__main.apply_options({ visible: false });
      const prices = chart.add_series("candlestick");
      const times = window.__data.slice(-3).map((bar) => bar.time);
      prices.set_data(times.map((time, index) => ({
        time, open: index === 2 ? 103 : 101, high: 104, low: 100, close: 102,
      })));
      const volume = chart.add_series("histogram", { visible: false });
      volume.set_data([{ time: times[0], value: 40 }, { time: times[2], value: 80 }]);
      chart.time_scale().set_visible_range({ from: times[0], to: times[2] });
      const profile = chart.add_volume_profile(prices, volume, {
        rows: 4,
        up_color: "#089981",
        down_color: "#f7525f",
        value_area_up_color: "#089981",
        value_area_down_color: "#f7525f",
        poc_color: "#f5a623",
      });
      window.__profile_test = { chart, prices, volume, profile, times };
      return profile.snapshot();
    });
    expect(initial.total_volume).toBe(120);
    expect(initial.bar_count).toBe(2);
    expect(initial.rows.map((row) => row.volume)).toEqual([30, 30, 30, 30]);
    expect(initial.rows.map((row) => row.up_volume)).toEqual([10, 10, 10, 10]);
    expect(initial.rows.map((row) => row.down_volume)).toEqual([20, 20, 20, 20]);
    expect(initial.poc).toBe(100.5);
    expect(initial.value_area_low).toBe(100);
    expect(initial.value_area_high).toBe(103);
    await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    const pixels = PNG.sync.read(await page.locator("#chart_wrap").screenshot());
    let green = 0;
    let red = 0;
    for (let offset = 0; offset < pixels.data.length; offset += 4) {
      if (Math.abs(pixels.data[offset] - 8) <= 2 && Math.abs(pixels.data[offset + 1] - 153) <= 2 && Math.abs(pixels.data[offset + 2] - 129) <= 2) green++;
      if (Math.abs(pixels.data[offset] - 247) <= 2 && Math.abs(pixels.data[offset + 1] - 82) <= 2 && Math.abs(pixels.data[offset + 2] - 95) <= 2) red++;
    }
    expect(green).toBeGreaterThan(100);
    expect(red).toBeGreaterThan(100);
    const updates = await page.evaluate(() => {
      const { chart, volume, profile, times } = window.__profile_test;
      const cached = profile.snapshot().calculation_revision;
      profile.apply_options({ width_percent: 30 });
      const styled = profile.snapshot().calculation_revision;
      volume.update({ time: times[2], value: 160 });
      const updated = profile.snapshot();
      chart.time_scale().set_visible_range({ from: times[0], to: times[1] });
      const ranged = profile.snapshot();
      let invalid;
      try { profile.apply_options({ rows: 513 }); } catch (error) { invalid = error.code; }
      const rows = profile.options().rows;
      chart.remove_series(volume);
      let stale;
      try { profile.snapshot(); } catch (error) { stale = error.code; }
      profile.remove();
      profile.remove();
      return { cached, styled, updated, ranged, invalid, rows, stale };
    });
    expect(updates.styled).toBe(updates.cached);
    expect(updates.updated.total_volume).toBe(200);
    expect(updates.ranged.total_volume).toBe(40);
    expect(updates.invalid).toBe("invalid_options");
    expect(updates.rows).toBe(4);
    expect(updates.stale).toBe("stale_handle");
  });
}

test("demo volume profile uses the built-in calculation", async ({ page }) => {
  await page.goto("/?backend=canvas2d");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const button = page.locator('[data-feature-id="volume-profile"]');
  await button.click();
  await expect(button).toHaveAttribute("aria-pressed", "true");
  await button.click();
  await expect(button).toHaveAttribute("aria-pressed", "false");
  expect(errors).toEqual([]);
});
