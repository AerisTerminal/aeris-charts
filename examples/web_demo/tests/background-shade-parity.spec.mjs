import { test, expect } from "@playwright/test";
import { PNG } from "pngjs";

const rgb = (value) => [
  Math.round(50 + 205 * value / 100),
  50,
  Math.round(255 - 205 * value / 100),
];

function pixel(image, x, y) {
  const offset = (y * image.width + x) * 4;
  return [...image.data.subarray(offset, offset + 3)];
}

function expect_color(actual, expected, message) {
  for (let channel = 0; channel < 3; channel += 1) {
    expect(
      Math.abs(actual[channel] - expected[channel]),
      `${message}: actual ${actual.join(",")} expected ${expected.join(",")}`,
    ).toBeLessThanOrEqual(1);
  }
}

async function fixture(page) {
  return page.locator("#nucleus").evaluate((element) => {
    const fixture = element.__shade_parity;
    return {
      empty_ranges: fixture.empty_ranges,
      shade_only_ranges: fixture.shade_only_ranges,
      composed_ranges: fixture.composed_ranges,
      line_only_ranges: fixture.line_only_ranges,
      nucleus_types: [fixture.series.nucleus_shade.series_type(), fixture.series.nucleus_line.series_type()],
      nucleus_lengths: [fixture.series.nucleus_shade.data().length, fixture.series.nucleus_line.data().length],
      reference_lengths: [fixture.series.reference_shade.data().length, fixture.series.reference_line.data().length],
    };
  });
}

async function assert_strips(page, indices) {
  const state = await page.locator("#nucleus").evaluate((element) => element.__shade_parity.metrics());
  for (const name of ["nucleus", "reference"]) {
    const locator = page.locator(`#${name}`);
    const box = await locator.boundingBox();
    const image = PNG.sync.read(await locator.screenshot({ animations: "disabled" }));
    const scale = image.width / box.width;
    const pane = state[name].pane;
    for (const index of indices) {
      const x = Math.round((pane.left + state[name].coordinates[index]) * scale);
      expect(x, `${name} index ${index} must be visible`).toBeGreaterThanOrEqual(0);
      expect(x, `${name} index ${index} must be visible`).toBeLessThan(image.width);
      const expected = rgb(state.values[index]);
      for (const y of [pane.top + 3, pane.top + pane.height - 4]) {
        expect_color(pixel(image, x, Math.round(y * scale)), expected, `${name} strip ${index} at x=${x} y=${y}`);
      }
    }
    for (const index of [9, 31]) {
      const coordinate = state[name].coordinates[index];
      if (coordinate === null || coordinate < 0 || coordinate >= pane.width) continue;
      const x = Math.round((pane.left + coordinate) * scale);
      expect_color(
        pixel(image, x, Math.round((pane.top + 3) * scale)),
        [255, 255, 255],
        `${name} whitespace ${index}`,
      );
    }
  }
  expect(state.nucleus.logical_range.from).toBeCloseTo(state.reference.logical_range.from, 7);
  expect(state.nucleus.logical_range.to).toBeCloseTo(state.reference.logical_range.to, 7);
}

test("background shade matches the official full-height per-bar renderer", async ({ browser }) => {
  for (const dpr of [1, 2]) {
    const context = await browser.newContext({ viewport: { width: 1280, height: 720 }, deviceScaleFactor: dpr });
    const page = await context.newPage();
    await page.goto("/background-shade-parity.html");
    await page.waitForFunction(() => document.getElementById("nucleus")?.__shade_parity?.metrics !== undefined);

    const initial = await fixture(page);
    expect(initial.empty_ranges.nucleus).toBeNull();
    expect(initial.shade_only_ranges.nucleus).toBeNull();
    expect(initial.shade_only_ranges.reference).toEqual({ from: -0.5, to: 0.5 });
    expect(initial.composed_ranges).toEqual(initial.line_only_ranges);
    expect(initial.nucleus_types).toEqual(["background_shade", "line"]);
    expect(initial.nucleus_lengths).toEqual([60, 60]);
    expect(initial.reference_lengths).toEqual([58, 60]);

    await assert_strips(page, [0, 1, 2, 3]);

    await page.locator("#nucleus").evaluate((element) => element.__shade_parity.set_range(0, 10));
    await assert_strips(page, [0, 1, 2, 3]);
    const clipped = await page.locator("#nucleus").evaluate((element) => element.__shade_parity.metrics());
    expect(clipped.nucleus.coordinates[20]).toBeGreaterThan(clipped.nucleus.pane.width);
    expect(clipped.reference.coordinates[20]).toBeGreaterThan(clipped.reference.pane.width);

    await page.locator("#nucleus").evaluate((element) => element.__shade_parity.set_range(20, 35));
    await assert_strips(page, [22, 23, 24]);

    await page.locator("#nucleus").evaluate((element) => element.__shade_parity.set_range(0, 59));
    await assert_strips(page, [0, 1, 2, 3]);

    await page.locator("#nucleus").evaluate((element) => element.__shade_parity.resize(480, 260));
    await page.locator("#nucleus").evaluate((element) => element.__shade_parity.set_range(20, 35));
    await assert_strips(page, [22, 23, 24]);
    await context.close();
  }
});
