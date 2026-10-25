import { test, expect } from "@playwright/test";

const BASE = "/?backend=canvas2d&forceFallbackAdapter=1";
const TICK = 0.05;

async function open_demo(page, query = "") {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  await page.goto(`${BASE}${query}`);
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined && window.__demo_catalogs !== undefined);
  return errors;
}

function wait_for_history(page, count) {
  return page.waitForFunction((n) => window.__live_market.stats().histories >= n, count, { timeout: 30_000 });
}

function sample(page) {
  return page.evaluate(() => {
    const live = window.__live_market;
    const footprint = window.__demo_catalogs.series.active_id() === "footprint"
      ? window.__demo_catalogs.series.interaction_series().footprint_bars().at(-1) ?? null
      : null;
    return {
      stats: live.stats(),
      sim_time: live.sim_time(),
      quote: live.quote(),
      bars: live.bars().length,
      data: window.__main.data().length,
      last: window.__main.data().at(-1),
      footprint: footprint === null ? null : { index: footprint.logical_index, volume: footprint.total_volume },
    };
  });
}

test("live feed is off by default and leaves the deterministic demo untouched", async ({ page }) => {
  const errors = await open_demo(page);
  await expect(page.locator("#live_controls")).toBeVisible();
  await expect(page.locator("#live_toggle")).toHaveAttribute("aria-pressed", "false");
  await expect(page.locator("#live_speed")).toHaveValue("1");
  await expect(page.locator("#market_meta")).toHaveText("Synthetic market · 1,000 bars");
  const read = () => page.evaluate(() => ({
    running: window.__live_market.running(),
    stats: window.__live_market.stats(),
    data: window.__main.data().map((point) => [point.time, point.open, point.high, point.low, point.close]),
    fixture: window.__data.map((point) => [point.time, point.open, point.high, point.low, point.close]),
  }));
  const before = await read();
  expect(before.running).toBe(false);
  expect(before.stats).toMatchObject({ histories: 0, batches: 0, trades: 0, trade_stream: null, depth_stream: null });
  expect(before.data).toHaveLength(1000);
  expect(before.data).toEqual(before.fixture);
  await page.waitForTimeout(500);
  const after = await read();
  expect(after.data).toEqual(before.data);
  expect(after.stats.batches).toBe(0);
  expect(errors).toEqual([]);
});

test("runtime fixtures never expose or start the live feed", async ({ page }) => {
  await page.goto("/?runtimeTest=presentedFrame&backend=canvas2d&forceFallbackAdapter=1&live=1");
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined);
  await expect(page.locator("#live_controls")).toBeHidden();
  expect(await page.evaluate(() => window.__live_market === undefined && window.__main.data().length)).toBe(1000);
});

test("Live streams a simulated market into candles, tape, book, footprint, and order-flow layers", async ({ page }) => {
  test.setTimeout(90_000);
  const errors = await open_demo(page, "&liveSeed=7");
  await page.selectOption("#live_speed", "60");
  await page.click("#live_toggle");
  await wait_for_history(page, 1);
  await expect(page.locator("#live_toggle")).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator("#market_meta")).toHaveText("Live simulated market · 1m · 60x");
  await expect(page.locator("#market_symbol")).toHaveText("NCL · 1m");

  const history = await page.evaluate(() => ({
    bars: window.__live_market.bars(),
    data: window.__main.data().length,
    countdown: window.__main.options().countdown_visible,
    stats: window.__live_market.stats(),
  }));
  expect(history.bars.length).toBeGreaterThanOrEqual(720);
  expect(history.data).toBe(history.bars.length);
  // The package countdown follows the wall clock, so it is hidden while the clock runs at 60x.
  expect(history.countdown).toBe(false);
  expect(history.stats.trade_stream).not.toBeNull();
  expect(history.stats.depth_stream).not.toBeNull();
  expect(history.stats.trades).toBeGreaterThan(10_000);
  let traded = 0;
  for (let index = 0; index < history.bars.length; index += 1) {
    const bar = history.bars[index];
    expect(bar.low).toBeLessThanOrEqual(Math.min(bar.open, bar.close));
    expect(bar.high).toBeGreaterThanOrEqual(Math.max(bar.open, bar.close));
    expect(bar.low).toBeGreaterThan(0);
    if (index > 0) expect(bar.time - history.bars[index - 1].time).toBe(60);
    if (bar.volume > 0) traded += 1;
  }
  expect(traded / history.bars.length).toBeGreaterThan(0.95);
  const ranges = history.bars.map((bar) => Math.round((bar.high - bar.low) / TICK)).sort((a, b) => a - b);
  const median = ranges[Math.floor(ranges.length / 2)];
  expect(median).toBeGreaterThanOrEqual(3);
  expect(median).toBeLessThanOrEqual(60);

  await page.evaluate(() => {
    window.__demo_catalogs.series.activate("footprint");
    for (const id of ["depth-heatmap", "depth-profile", "delta-histogram", "cvd"]) window.__demo_catalogs.lab.activate(id);
  });
  expect(await page.evaluate(() => window.__demo_catalogs.series.active_id())).toBe("footprint");
  expect(await page.evaluate(() => window.__demo_catalogs.lab.active_ids()))
    .toEqual(expect.arrayContaining(["depth-heatmap", "depth-profile", "delta-histogram", "cvd"]));
  await page.waitForFunction(() => window.__demo_catalogs.series.interaction_series().footprint_bars().length > 0);

  const first = await sample(page);
  await page.waitForTimeout(2500);
  const second = await sample(page);
  expect(second.stats.batches).toBeGreaterThan(first.stats.batches);
  expect(second.stats.trades).toBeGreaterThan(first.stats.trades);
  expect(second.stats.depth_rows).toBeGreaterThan(first.stats.depth_rows);
  expect(second.stats.bar_updates).toBeGreaterThan(first.stats.bar_updates);
  const advanced = second.sim_time - first.sim_time;
  expect(advanced).toBeGreaterThan(60);
  expect(advanced).toBeLessThan(400);
  expect(second.last.time).toBeGreaterThan(first.last.time);
  expect(second.data).toBe(second.bars);
  expect(second.footprint).not.toBeNull();
  expect(second.footprint).not.toEqual(first.footprint);
  const spread = Math.round((second.quote.ask - second.quote.bid) / TICK);
  expect(spread).toBeGreaterThanOrEqual(1);
  expect(spread).toBeLessThanOrEqual(2);
  expect(errors).toEqual([]);
});

test("stopping keeps the data and restarting regenerates a clean session", async ({ page }) => {
  test.setTimeout(90_000);
  const errors = await open_demo(page, "&live=1&liveSeed=3");
  await wait_for_history(page, 1);
  await expect(page.locator("#live_toggle")).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator("#market_meta")).toHaveText("Live simulated market · 1m");
  expect(await page.evaluate(() => window.__main.options().countdown_visible)).toBe(true);
  await page.selectOption("#live_speed", "60");
  await expect(page.locator("#market_meta")).toHaveText("Live simulated market · 1m · 60x");
  await page.waitForFunction(() => window.__live_market.stats().batches > 5);

  await page.click("#live_toggle");
  await expect(page.locator("#live_toggle")).toHaveAttribute("aria-pressed", "false");
  await expect(page.locator("#market_meta")).toHaveText("Simulated market · 1m · paused");
  expect(await page.evaluate(() => window.__main.options().countdown_visible)).toBe(true);
  const stopped = await sample(page);
  expect(stopped.quote).toBeNull();
  await page.waitForTimeout(600);
  const still = await sample(page);
  expect(still.stats.batches).toBe(stopped.stats.batches);
  expect(still.stats.trades).toBe(stopped.stats.trades);
  expect(still.data).toBe(stopped.data);
  expect(still.last).toEqual(stopped.last);

  await page.click("#live_toggle");
  await wait_for_history(page, 2);
  await expect(page.locator("#live_speed")).toHaveValue("60");
  const restarted = await sample(page);
  expect(restarted.stats.running).toBe(true);
  // The tape stream is reused rather than leaked; the history replaces the previous session.
  expect(restarted.stats.trade_stream).toBe(stopped.stats.trade_stream);
  expect(restarted.bars).toBeLessThanOrEqual(stopped.bars);
  expect(restarted.bars).toBeGreaterThanOrEqual(720);
  expect(restarted.data).toBe(restarted.bars);
  await page.click("#live_toggle");
  expect(errors).toEqual([]);
});
