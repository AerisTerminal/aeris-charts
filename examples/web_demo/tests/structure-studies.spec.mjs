import { test, expect } from "@playwright/test";

test("browser structure studies expose values, typed annotations and parameter validation", async ({ page }) => {
  await page.goto("/?backend=canvas2d");
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined);
  const result = await page.evaluate(() => {
    const chart = window.__chart;
    const source = chart.add_series("candlestick", { visible: false });
    const time = 1_700_000_000;
    source.set_data([
      { time, open: 9, high: 10, low: 8, close: 9 },
      { time: time + 60, open: 12, high: 14, low: 9, close: 13 },
      { time: time + 120, open: 12, high: 12, low: 10, close: 11 },
      { time: time + 180, open: 16, high: 18, low: 15, close: 17 },
      { time: time + 240, open: 16, high: 16, low: 15, close: 15 },
      { time: time + 300, open: 18, high: 20, low: 8, close: 19 },
      { time: time + 360, open: 19, high: 21, low: 16, close: 20 },
    ]);
    const swings = chart.add_swing_points(source, 1, 1);
    const market = chart.add_market_structure(source, 1, 1, "close");
    const gaps = chart.add_fair_value_gaps(source, { min_size: 0.5, show_mitigated: true });
    const blocks = chart.add_order_blocks(source, { left: 1, right: 1, zone: "body", show_mitigated: true });
    const studies = [...swings, market, gaps, blocks];
    const errors = [
      () => chart.add_swing_points(source, 0, 1),
      () => chart.add_swing_points(source, 1, 51),
      () => chart.add_market_structure(source, 1, 1, "invalid"),
      () => chart.add_fair_value_gaps(source, { min_size: -1 }),
      () => chart.add_fair_value_gaps(source, { mitigation: "invalid" }),
      () => chart.add_fair_value_gaps(source, { max_active: 65 }),
      () => chart.add_fair_value_gaps(source, { mitigation: null }),
      () => chart.add_fair_value_gaps(source, { show_mitigated: null }),
      () => chart.add_fair_value_gaps(source, { max_active: 1.5 }),
      () => chart.add_order_blocks(source, { zone: "invalid" }),
      () => chart.add_order_blocks(source, { mitigation_price: "invalid" }),
    ].map((invoke) => {
      try { invoke(); return null; } catch (error) { return error.code; }
    });
    const snapshot = studies.map((output) => ({
      kind: output.indicator_info().kind,
      data: output.data(),
      annotations: chart.study_annotations(output),
    }));
    const schemas = ["swing_points", "market_structure", "fair_value_gaps", "order_blocks"]
      .map((kind) => chart.indicator_schema(kind));
    let plainError = null;
    try { chart.study_annotations(source); } catch (error) { plainError = error.code; }
    chart.remove_series(source);
    let removedError = null;
    try { chart.study_annotations(market); } catch (error) { removedError = error.code; }
    return { snapshot, schemas, errors, plainError, removedError };
  });
  expect(result.snapshot.map(({ kind }) => kind)).toEqual([
    "swing_points", "swing_points", "market_structure", "fair_value_gaps", "order_blocks",
  ]);
  expect(result.snapshot[0].annotations.markers).toContainEqual(expect.objectContaining({
    row: 1, confirm_row: 2, price: 14, kind: "swing_high",
  }));
  expect(result.snapshot[0].data[2].value).toBe(14);
  expect(result.snapshot[2].annotations.markers.some(({ kind }) => typeof kind === "object" && kind.bos?.up)).toBe(true);
  expect(result.snapshot[3].annotations.zones).toContainEqual(expect.objectContaining({
    start_row: 2, confirm_row: 3, top: 15, bottom: 14, bullish: true,
  }));
  expect(result.snapshot[4].annotations.zones.length).toBeGreaterThan(0);
  expect(result.snapshot.slice(2).every(({ data }) => data.every(({ value }) => value === undefined))).toBe(true);
  expect(result.schemas.map(({ revision }) => revision)).toEqual([2, 2, 2, 2]);
  expect(result.schemas[2].parameters.find(({ name }) => name === "mitigation")).toMatchObject({
    parameter_type: "choice", default: "touch", options: ["touch", "half", "full"],
  });
  expect(result.errors).toEqual(Array(11).fill("invalid_options"));
  expect(result.plainError).toBe("unsupported_operation");
  expect(result.removedError).toBe("invalid_handle");
});

test("max active retires zones without deleting their browser history", async ({ page }) => {
  await page.goto("/?backend=canvas2d");
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined);
  const zones = await page.evaluate(() => {
    const chart = window.__chart;
    const source = chart.add_series("candlestick", { visible: false });
    source.set_data(Array.from({ length: 30 }, (_, row) => ({
      time: 1_700_100_000 + row * 60,
      open: row * 3 + 10, high: row * 3 + 11,
      low: row * 3 + 9, close: row * 3 + 10,
    })));
    const gap = chart.add_fair_value_gaps(source, {
      max_active: 1, mitigation: "full", mitigation_price: "close", show_mitigated: true,
    });
    return chart.study_annotations(gap).zones;
  });
  expect(zones).toHaveLength(28);
  expect(zones[0]).toMatchObject({
    start_row: 1, confirm_row: 2, end_row: 3, retired: true,
  });
  expect(zones.at(-1)).toMatchObject({ end_row: null, retired: false });
});
