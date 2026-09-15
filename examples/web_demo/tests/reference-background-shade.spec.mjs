import { test, expect } from "@playwright/test";
import { PNG } from "pngjs";

const rgb = (value) => [
  Math.round(50 + 205 * value / 1000),
  50,
  Math.round(255 - 205 * value / 1000),
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
  }
  expect(state.nucleus.logical_range.from).toBeCloseTo(state.reference.logical_range.from, 7);
  expect(state.nucleus.logical_range.to).toBeCloseTo(state.reference.logical_range.to, 7);
}

async function assert_scanline_parity(page) {
  const state = await page.locator("#nucleus").evaluate((element) => element.__shade_parity.metrics());
  const nucleus_locator = page.locator("#nucleus");
  const reference_locator = page.locator("#reference");
  const nucleus_box = await nucleus_locator.boundingBox();
  const reference_box = await reference_locator.boundingBox();
  const nucleus = PNG.sync.read(await nucleus_locator.screenshot({ animations: "disabled" }));
  const reference = PNG.sync.read(await reference_locator.screenshot({ animations: "disabled" }));
  const nucleus_scale = nucleus.width / nucleus_box.width;
  const reference_scale = reference.width / reference_box.width;
  const width = Math.min(
    Math.round(state.nucleus.pane.width * nucleus_scale),
    Math.round(state.reference.pane.width * reference_scale),
  );
  const nucleus_y = Math.round((state.nucleus.pane.top + 3) * nucleus_scale);
  const reference_y = Math.round((state.reference.pane.top + 3) * reference_scale);
  for (let x = 0; x < width; x += 1) {
    expect_color(
      pixel(nucleus, Math.round(state.nucleus.pane.left * nucleus_scale) + x, nucleus_y),
      pixel(reference, Math.round(state.reference.pane.left * reference_scale) + x, reference_y),
      `full public-reference shade scanline at x=${x}`,
    );
  }
}

test("background shade retains behavior learned from the public example", async ({ browser }) => {
  for (const dpr of [1, 2]) {
    const context = await browser.newContext({ viewport: { width: 1280, height: 720 }, deviceScaleFactor: dpr });
    const page = await context.newPage();
    await page.goto("/reference-background-shade.html");
    await page.waitForFunction(() => document.getElementById("nucleus")?.__shade_parity?.metrics !== undefined);

    const initial = await fixture(page);
    expect(initial.empty_ranges.nucleus).toBeNull();
    expect(initial.shade_only_ranges.nucleus).toBeNull();
    expect(initial.shade_only_ranges.reference).toEqual({ from: -0.5, to: 0.5 });
    expect(initial.composed_ranges).toEqual(initial.line_only_ranges);
    expect(initial.nucleus_types).toEqual(["background_shade", "line"]);
    expect(initial.nucleus_lengths).toEqual([500, 500]);
    expect(initial.reference_lengths).toEqual([500, 500]);

    await assert_scanline_parity(page);

    await page.locator("#nucleus").evaluate((element) => element.__shade_parity.set_range(0, 20));
    await assert_strips(page, [2, 10, 18]);
    await assert_scanline_parity(page);
    const clipped = await page.locator("#nucleus").evaluate((element) => element.__shade_parity.metrics());
    expect(clipped.nucleus.coordinates[100]).toBeGreaterThan(clipped.nucleus.pane.width);
    expect(clipped.reference.coordinates[100]).toBeGreaterThan(clipped.reference.pane.width);

    await page.locator("#nucleus").evaluate((element) => element.__shade_parity.set_range(200, 260));
    await assert_strips(page, [205, 230, 255]);
    await assert_scanline_parity(page);

    await page.locator("#nucleus").evaluate((element) => element.__shade_parity.set_range(0, 499));
    await assert_scanline_parity(page);

    await page.locator("#nucleus").evaluate((element) => element.__shade_parity.resize(480, 260));
    await page.locator("#nucleus").evaluate((element) => element.__shade_parity.set_range(200, 260));
    await assert_strips(page, [205, 230, 255]);
    await assert_scanline_parity(page);
    await context.close();
  }
});
