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
    const beforeInvalid = chart.series_order().length;
    const errors = [
      () => chart.add_swing_points(source, 0, 1),
      () => chart.add_swing_points(source, 1, 51),
    ].map((invoke) => {
      try { invoke(); return null; } catch (error) {
        return { name: error.name, code: error.code };
      }
    });
    const afterInvalid = chart.series_order().length;
    const snapshot = swings.map((output) => ({
      kind: output.indicator_info().kind,
      parameters: output.indicator_info().parameters,
      data: output.data(),
      annotations: chart.study_annotations(output),
    }));
    const schemas = [chart.indicator_schema("swing_points")];
    let plainError = null;
    try { chart.study_annotations(source); } catch (error) { plainError = error.code; }
    chart.remove_series(source);
    let removedError = null;
    try { chart.study_annotations(swings[0]); } catch (error) { removedError = error.code; }
    return { snapshot, schemas, errors, beforeInvalid, afterInvalid, plainError, removedError };
  });
  expect(result.snapshot.map(({ kind }) => kind)).toEqual(["swing_points", "swing_points"]);
  expect(result.snapshot[0].annotations.markers).toContainEqual(expect.objectContaining({
    row: 1, confirm_row: 2, price: 14, kind: "swing_high",
  }));
  expect(result.snapshot[0].annotations.markers).toContainEqual(expect.objectContaining({
    row: 5, confirm_row: 6, price: 8, kind: "swing_low",
  }));
  expect(result.snapshot.every(({ annotations }) => Object.keys(annotations).join() === "markers")).toBe(true);
  expect(result.snapshot[0].annotations.markers.every((marker) => !("from_row" in marker))).toBe(true);
  expect(result.snapshot[0].data[0]).toEqual({ time: 1_700_000_120, value: 14 });
  expect(result.snapshot[0].parameters).toMatchObject({ left: 1, right: 1 });
  expect(result.schemas.map(({ revision }) => revision)).toEqual([2]);
  expect(result.errors).toEqual(Array(2).fill({ name: "AerisChartsError", code: "invalid_options" }));
  expect(result.afterInvalid).toBe(result.beforeInvalid);
  expect(result.plainError).toBe("unsupported_operation");
  expect(result.removedError).toBe("invalid_handle");
});
