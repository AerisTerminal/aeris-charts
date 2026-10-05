import { test, expect } from "@playwright/test";

const start = 1_700_000_000;
const candles = [
  { time: start, open: 9, high: 10, low: 8, close: 9 },
  { time: start + 60, open: 12, high: 14, low: 9, close: 13 },
  { time: start + 120, open: 12, high: 12, low: 10, close: 11 },
  { time: start + 180, open: 16, high: 18, low: 15, close: 17 },
  { time: start + 240, open: 16, high: 16, low: 15, close: 15 },
  { time: start + 300, open: 18, high: 20, low: 8, close: 19 },
  { time: start + 360, open: 19, high: 21, low: 16, close: 20 },
];

test("replay clock masks future structure annotations and session levels, and backward seek matches fresh load", async ({ page }) => {
  await page.goto("/?backend=canvas2d");
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined);
  const replay = await page.evaluate(({ candles, cutoff }) => {
    const chart = window.__chart;
    const source = chart.add_series("candlestick");
    source.set_data(candles);
    const market = chart.add_market_structure(source, 1, 1);
    const gaps = chart.add_fair_value_gaps(source);
    const blocks = chart.add_order_blocks(source, { left: 1, right: 1 });
    const levels = chart.add_session_levels(source);
    const snapshot = () => ({
      market: chart.study_annotations(market),
      gaps: chart.study_annotations(gaps),
      blocks: chart.study_annotations(blocks),
      levels: levels.map((output) => output.data().map(({ time, value }) => ({ time, value: value ?? null }))),
    });
    const complete = snapshot();
    chart.set_replay_clock_micros(cutoff);
    const masked = snapshot();
    chart.set_replay_clock_micros(null);
    const resumed = snapshot();
    chart.set_replay_clock_micros(cutoff);
    return { complete, masked, resumed, soughtBack: snapshot(), clock: chart.replay_clock_micros() };
  }, { candles, cutoff: (start + 180) * 1_000_000 });

  expect(replay.clock).toBe((start + 180) * 1_000_000);
  expect(replay.complete.market.markers.some(({ row }) => row > 3)).toBe(true);
  expect(replay.complete.blocks.zones.some(({ confirm_row }) => confirm_row > 3)).toBe(true);
  expect(replay.complete.levels[0]).toHaveLength(candles.length);
  expect(replay.complete.levels[0].at(-1).value).toBe(21);
  for (const { markers, zones } of [replay.masked.market, replay.masked.gaps, replay.masked.blocks]) {
    expect(markers.every(({ row, confirm_row }) => row <= 3 && confirm_row <= 3)).toBe(true);
    expect(zones.every(({ start_row, confirm_row, end_row }) =>
      start_row <= 3 && confirm_row <= 3 && (end_row === null || end_row <= 3))).toBe(true);
  }
  expect(replay.masked.levels.every((output) => output.length === 4)).toBe(true);
  expect(replay.masked.levels[0].at(-1).value).toBe(18);
  expect(replay.resumed).toEqual(replay.complete);
  expect(replay.soughtBack).toEqual(replay.masked);

  await page.reload();
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined);
  const fresh = await page.evaluate(({ candles, cutoff }) => {
    const chart = window.__chart;
    chart.set_replay_clock_micros(cutoff);
    const source = chart.add_series("candlestick");
    source.set_data(candles);
    const market = chart.add_market_structure(source, 1, 1);
    const gaps = chart.add_fair_value_gaps(source);
    const blocks = chart.add_order_blocks(source, { left: 1, right: 1 });
    const levels = chart.add_session_levels(source);
    return {
      market: chart.study_annotations(market),
      gaps: chart.study_annotations(gaps),
      blocks: chart.study_annotations(blocks),
      levels: levels.map((output) => output.data().map(({ time, value }) => ({ time, value: value ?? null }))),
    };
  }, { candles, cutoff: (start + 180) * 1_000_000 });
  expect(replay.soughtBack).toEqual(fresh);
});
