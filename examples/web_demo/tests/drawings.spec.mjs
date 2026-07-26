import { test, expect } from "@playwright/test";
import { readFileSync } from "node:fs";
import pixelmatch from "pixelmatch";
import { PNG } from "pngjs";

// Engine-owned drawing tools: creation via the armed-tool click flow, click-to-select with
// anchor handles, anchor re-anchor drags, whole-body moves, per-tool text labels with 3×3
// alignment, Delete/Backspace removal, and WebGPU == Canvas2D pixel parity. The engine owns
// all state and drag math (drawings.rs); the specs drive the public API + pointer only.

const fixture = JSON.parse(readFileSync(new URL("../fixtures/d1/candles.json", import.meta.url), "utf8"));
const PR = fixture.pixel_ratio;
const BLUE = [41, 98, 255]; // #2962ff — the anchor-handle border and default drawing color
const PURPLE = [123, 31, 162]; // #7b1fa2 — text label color (collides with no fixture pixel)

test.beforeEach(async ({ page }) => {
  page.on("console", (message) => console.log(`[browser:${message.type()}] ${message.text()}`));
  page.on("pageerror", (error) => console.log(`[browser:pageerror] ${error.message}`));
});

async function wait_for_chart(page) {
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined);
  await page.evaluate(() => new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  }));
}

async function settle_frames(page) {
  await page.evaluate(() => new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  }));
}

async function goto_fixture(page, backend = "canvas2d") {
  await page.goto(`/?runtimeTest=presentedFrame&backend=${backend}&forceFallbackAdapter=1`);
  await wait_for_chart(page);
  // Past the touch-suppression window before driving the pointer (see hit-testing.spec.mjs).
  await page.waitForFunction(() => performance.now() > 600);
}

/** The chart composite (pane + axes) at device-pixel resolution. */
async function capture(page) {
  const data_url = await page.evaluate(() => window.__chart.take_screenshot().toDataURL("image/png"));
  return PNG.sync.read(Buffer.from(data_url.split(",")[1], "base64"));
}

function count_color(png, target, tol = 30) {
  let n = 0;
  for (let o = 0; o < png.data.length; o += 4) {
    if (
      Math.abs(png.data[o] - target[0]) <= tol &&
      Math.abs(png.data[o + 1] - target[1]) <= tol &&
      Math.abs(png.data[o + 2] - target[2]) <= tol
    ) n += 1;
  }
  return n;
}

/** The pixel centroid of `target`-colored pixels (device px), or null when none. */
function color_centroid(png, target, tol = 10) {
  let n = 0;
  let sx = 0;
  let sy = 0;
  for (let y = 0; y < png.height; y += 1) {
    for (let x = 0; x < png.width; x += 1) {
      const o = (y * png.width + x) * 4;
      if (
        Math.abs(png.data[o] - target[0]) <= tol &&
        Math.abs(png.data[o + 1] - target[1]) <= tol &&
        Math.abs(png.data[o + 2] - target[2]) <= tol
      ) {
        n += 1;
        sx += x;
        sy += y;
      }
    }
  }
  return n === 0 ? null : { x: sx / n, y: sy / n, count: n };
}

function pixel_diff(a, b) {
  expect([a.width, a.height]).toEqual([b.width, b.height]);
  return pixelmatch(a.data, b.data, null, a.width, a.height, { threshold: 0, includeAA: true });
}

/** Crop a (w×h) region centered on device px (cx, cy), clamped into the image. */
function crop_around(png, cx, cy, w, h) {
  const x = Math.max(0, Math.min(Math.round(cx - w / 2), png.width - w));
  const y = Math.max(0, Math.min(Math.round(cy - h / 2), png.height - h));
  const out = new PNG({ width: w, height: h });
  PNG.bitblt(png, out, x, y, w, h, 0, 0);
  return out;
}

/** Count dark-ish pixels (muted placeholder text on the light fixture background). */
function count_dark(png, max_lum = 170) {
  let n = 0;
  for (let o = 0; o < png.data.length; o += 4) {
    if (png.data[o] < max_lum && png.data[o + 1] < max_lum && png.data[o + 2] < max_lum) n += 1;
  }
  return n;
}

/** The visible mid-range logical indexes and prices to anchor drawings deterministically. */
async function anchor_spots(page) {
  return page.evaluate(() => {
    const range = window.__chart.time_scale().get_visible_logical_range();
    const l0 = Math.floor(range.from + (range.to - range.from) * 0.25);
    const l1 = Math.floor(range.from + (range.to - range.from) * 0.7);
    const b0 = window.__main.data_by_index(l0);
    const b1 = window.__main.data_by_index(l1);
    const low = Math.min(b0.low, b1.low);
    const high = Math.max(b0.high, b1.high);
    return {
      l0,
      l1,
      p_lo: low,
      p_hi: high,
      p_mid: (low + high) / 2,
      to_x: (l) => window.__chart.time_scale().logical_to_coordinate(l),
      to_y: (p) => window.__main.price_to_coordinate(p),
    };
  });
}

/** CSS-px pane coordinates of a (logical, price) anchor. */
async function spot(page, logical, price) {
  return page.evaluate(
    ({ logical, price }) => ({
      x: window.__chart.time_scale().logical_to_coordinate(logical),
      y: window.__main.price_to_coordinate(price),
    }),
    { logical, price },
  );
}

async function drawings(page) {
  return page.evaluate(() => window.__chart.drawings().map((d) => ({
    id: d.id, kind: d.kind(), pane_index: d.pane_index(), points: d.points(),
  })));
}

async function overlay_cursor(page) {
  return page.evaluate(() => {
    const canvases = document.querySelectorAll("#chart_container canvas");
    return canvases[canvases.length - 1].style.cursor;
  });
}

/** Focus the input overlay so keyboard gestures reach the recognizer. */
async function focus_overlay(page) {
  await page.evaluate(() => document.querySelector("#chart_container canvas:last-of-type").focus());
}

/** The EFFECTIVE bar spacing in CSS px (fit-content adjusts it; `options().bar_spacing` is the configured value). */
async function bar_spacing(page) {
  return page.evaluate(() => {
    const x0 = window.__chart.time_scale().logical_to_coordinate(100);
    const x1 = window.__chart.time_scale().logical_to_coordinate(101);
    return x1 - x0;
  });
}

/**
 * CSS-px x for a possibly fractional logical (the public `logical_to_coordinate` is
 * integer-only — reference parity — so interpolate with the effective bar spacing).
 */
async function x_of(page, logical) {
  const spacing = await bar_spacing(page);
  return page.evaluate(
    ({ logical, spacing }) => {
      const base = Math.floor(logical);
      return window.__chart.time_scale().logical_to_coordinate(base) + spacing * (logical - base);
    },
    { logical, spacing },
  );
}

/** A pane point above every visible candle's high: a certain miss for both drawings and series. */
async function empty_spot(page) {
  return page.evaluate(() => {
    const range = window.__chart.time_scale().get_visible_logical_range();
    let max_high = -Infinity;
    for (let i = Math.ceil(range.from); i <= Math.floor(range.to); i++) {
      const bar = window.__main.data_by_index(i);
      if (bar) max_high = Math.max(max_high, bar.high);
    }
    const x = window.__chart.time_scale().logical_to_coordinate(Math.floor((range.from + range.to) / 2));
    return { x, y: window.__main.price_to_coordinate(max_high) - 20 };
  });
}

test("all six tools create through the armed-tool click flow", async ({ page }) => {
  await goto_fixture(page);
  const s = await anchor_spots(page);
  const clean = await capture(page);

  const cases = [
    { kind: "trend_line", clicks: [[s.l0, s.p_lo], [s.l1, s.p_hi]] },
    { kind: "horizontal_line", clicks: [[s.l0, s.p_mid]] },
    { kind: "horizontal_ray", clicks: [[s.l0, s.p_lo]] },
    { kind: "vertical_line", clicks: [[Math.floor((s.l0 + s.l1) / 2), s.p_mid]] },
    { kind: "rectangle", clicks: [[s.l0, s.p_lo], [s.l1, s.p_hi]] },
    { kind: "text", clicks: [[s.l1, s.p_hi]] },
  ];
  for (const [index, entry] of cases.entries()) {
    await page.evaluate(({ kind }) => {
      window.__chart.set_drawing_tool(kind, { color: "#2962ff", text: "" });
      if (window.__chart.active_drawing_tool() !== kind) throw new Error(`tool ${kind} not armed`);
    }, { kind: entry.kind });
    for (const [logical, price] of entry.clicks) {
      const p = await spot(page, logical, price);
      await page.mouse.click(p.x, p.y);
    }
    await settle_frames(page);
    const list = await drawings(page);
    expect(list, `after ${entry.kind}`).toHaveLength(index + 1);
    expect(list[index].kind).toBe(entry.kind);
    expect(list[index].points).toHaveLength(entry.clicks.length);
    for (const point of list[index].points) {
      expect(Number.isFinite(point.logical)).toBe(true);
      expect(Number.isFinite(point.price)).toBe(true);
    }
    // One-shot: the tool disarmed itself after the commit.
    expect(await page.evaluate(() => window.__chart.active_drawing_tool())).toBeNull();
    const after = await capture(page);
    expect(pixel_diff(clean, after), `${entry.kind} paints`).toBeGreaterThan(20);
  }
});

test("two-anchor creation previews the pending drawing; Escape cancels it", async ({ page }) => {
  await goto_fixture(page);
  const s = await anchor_spots(page);
  const clean = await capture(page);
  await page.evaluate(() => window.__chart.set_drawing_tool("rectangle", { color: "#2962ff" }));

  const first = await spot(page, s.l0, s.p_lo);
  await page.mouse.click(first.x, first.y);
  expect(await drawings(page)).toHaveLength(0);
  // The preview follows the mouse before the second click.
  const hover = await spot(page, s.l1, s.p_hi);
  await page.mouse.move(hover.x, hover.y);
  await settle_frames(page);
  const preview = await capture(page);
  expect(pixel_diff(clean, preview), "preview paints before commit").toBeGreaterThan(20);

  // Escape abandons the pending drawing and disarms the tool.
  await focus_overlay(page);
  await page.keyboard.press("Escape");
  await settle_frames(page);
  expect(pixel_diff(clean, await capture(page)), "cancel restores the clean frame").toBe(0);
  expect(await drawings(page)).toHaveLength(0);
  expect(await page.evaluate(() => window.__chart.active_drawing_tool())).toBeNull();

  // A click afterwards places nothing (the tool is no longer armed).
  await page.mouse.click(first.x, first.y);
  expect(await drawings(page)).toHaveLength(0);
});

test("click selects with anchor handles and part cursors; empty click deselects", async ({ page }) => {
  await goto_fixture(page);
  const s = await anchor_spots(page);
  // Black stroke: the blue handle discs are unambiguous against it.
  await page.evaluate(({ l0, l1, p_lo, p_hi }) => {
    window.__chart.add_drawing("trend_line", [
      { logical: l0, price: p_lo },
      { logical: l1, price: p_hi },
    ], { color: "#000000" });
  }, s);
  await settle_frames(page);
  const clean = await capture(page);
  expect(count_color(clean, BLUE), "no handles while unselected").toBe(0);

  // Hovering the body reports the drawing with the move cursor.
  const mid = await page.evaluate(({ l0, l1, p_lo, p_hi }) => ({
    x: (window.__chart.time_scale().logical_to_coordinate(l0) + window.__chart.time_scale().logical_to_coordinate(l1)) / 2,
    y: (window.__main.price_to_coordinate(p_lo) + window.__main.price_to_coordinate(p_hi)) / 2,
  }), s);
  await page.evaluate(() => {
    window.__hits = [];
    window.__chart.subscribe_crosshair_move((p) => window.__hits.push(p.hovered_object_id));
  });
  await page.mouse.move(mid.x, mid.y);
  const id = (await drawings(page))[0].id;
  expect(await page.evaluate(() => window.__hits[window.__hits.length - 1])).toBe(`drawing:${id}`);
  expect(await overlay_cursor(page)).toBe("move");

  // Click selects: anchor handles (blue discs) appear at both defining points.
  await page.mouse.click(mid.x, mid.y);
  await settle_frames(page);
  expect(await page.evaluate(() => window.__chart.selected_drawing()?.id ?? null)).toBe(id);
  const selected = await capture(page);
  expect(count_color(selected, BLUE), "anchor handles after selection").toBeGreaterThan(50);

  // Hovering an anchor handle switches to the pointer cursor.
  const anchor = await spot(page, s.l0, s.p_lo);
  await page.mouse.move(anchor.x, anchor.y);
  expect(await overlay_cursor(page)).toBe("pointer");

  // Clicking empty pane space deselects (handles gone; nothing else selects).
  const empty = await empty_spot(page);
  await page.mouse.click(empty.x, empty.y);
  await settle_frames(page);
  expect(await page.evaluate(() => window.__chart.selected_drawing())).toBeNull();
  expect(count_color(await capture(page), BLUE), "handles gone after deselect").toBe(0);
});

test("anchor drag re-anchors one point; body drag moves the whole drawing", async ({ page }) => {
  await goto_fixture(page);
  const s = await anchor_spots(page);
  await page.evaluate(({ l0, l1, p_lo, p_hi }) => {
    window.__chart.add_drawing("trend_line", [
      { logical: l0, price: p_lo },
      { logical: l1, price: p_hi },
    ]);
  }, s);
  await settle_frames(page);
  const before = (await drawings(page))[0];

  // Select with a body click, then drag anchor 0 by a known pixel delta.
  const mid = await page.evaluate(({ l0, l1, p_lo, p_hi }) => ({
    x: (window.__chart.time_scale().logical_to_coordinate(l0) + window.__chart.time_scale().logical_to_coordinate(l1)) / 2,
    y: (window.__main.price_to_coordinate(p_lo) + window.__main.price_to_coordinate(p_hi)) / 2,
  }), s);
  await page.mouse.click(mid.x, mid.y);
  const anchor = await spot(page, s.l0, s.p_lo);
  const spacing = await bar_spacing(page);
  const drop = { x: anchor.x + 4 * spacing, y: anchor.y - 40 };
  const expected_price = await page.evaluate((y) => window.__main.coordinate_to_price(y), drop.y);
  await page.mouse.move(anchor.x, anchor.y);
  await page.mouse.down();
  await page.mouse.move(drop.x, drop.y, { steps: 5 });
  await page.mouse.up();
  await settle_frames(page);

  const reanchored = (await drawings(page))[0];
  expect(reanchored.points[0].logical).toBeCloseTo(before.points[0].logical + 4, 1);
  expect(reanchored.points[0].price).toBeCloseTo(expected_price, 6);
  // Anchor 1 untouched.
  expect(reanchored.points[1].logical).toBeCloseTo(before.points[1].logical, 6);
  expect(reanchored.points[1].price).toBeCloseTo(before.points[1].price, 6);

  // Body drag: grab the segment's middle, move it — both anchors translate together.
  const seg = {
    x: (await x_of(page, reanchored.points[0].logical) + await x_of(page, reanchored.points[1].logical)) / 2,
    y: await page.evaluate(({ points }) => (
      window.__main.price_to_coordinate(points[0].price) +
      window.__main.price_to_coordinate(points[1].price)
    ) / 2, { points: reanchored.points }),
  };
  const body_drop = { x: seg.x - 2 * spacing, y: seg.y + 24 };
  const expected_body_price0 = await page.evaluate(
    ({ points, dy }) => window.__main.coordinate_to_price(window.__main.price_to_coordinate(points[0].price) + dy),
    { points: reanchored.points, dy: 24 },
  );
  await page.mouse.move(seg.x, seg.y);
  await page.mouse.down();
  await page.mouse.move(body_drop.x, body_drop.y, { steps: 5 });
  await page.mouse.up();
  await settle_frames(page);

  const moved = (await drawings(page))[0];
  expect(moved.points[0].logical).toBeCloseTo(reanchored.points[0].logical - 2, 1);
  expect(moved.points[1].logical).toBeCloseTo(reanchored.points[1].logical - 2, 1);
  expect(moved.points[0].price).toBeCloseTo(expected_body_price0, 6);
  // The shape (anchor spacing) is preserved exactly by the coordinate-space translation.
  expect(moved.points[1].logical - moved.points[0].logical)
    .toBeCloseTo(reanchored.points[1].logical - reanchored.points[0].logical, 6);
  // The drawing stayed selected through the drags.
  expect(await page.evaluate(() => window.__chart.selected_drawing()?.id ?? null)).toBe(moved.id);
});

test("full-span kinds freeze the unused axis in body drags", async ({ page }) => {
  await goto_fixture(page);
  const s = await anchor_spots(page);
  await page.evaluate(({ l0, p_mid, l1 }) => {
    window.__chart.add_drawing("horizontal_line", [{ logical: l0, price: p_mid }]);
    window.__chart.add_drawing("vertical_line", [{ logical: l1, price: 0 }]);
  }, s);
  await settle_frames(page);
  const [hline, vline] = await drawings(page);

  // Diagonal drag on the horizontal line: only the price changes.
  const h = await spot(page, s.l0, s.p_mid);
  await page.mouse.move(h.x, h.y);
  await page.mouse.down();
  await page.mouse.move(h.x + 60, h.y - 30, { steps: 4 });
  await page.mouse.up();
  const [hline_after] = await drawings(page);
  expect(hline_after.points[0].logical).toBeCloseTo(hline.points[0].logical, 6);
  expect(hline_after.points[0].price).not.toBeCloseTo(hline.points[0].price, 3);

  // Diagonal drag on the vertical line: only the logical changes.
  const v = await spot(page, s.l1, s.p_mid);
  await page.mouse.move(v.x, v.y);
  await page.mouse.down();
  await page.mouse.move(v.x + 40, v.y - 30, { steps: 4 });
  await page.mouse.up();
  const list = await drawings(page);
  const vline_after = list.find((d) => d.id === vline.id);
  expect(vline_after.points[0].price).toBeCloseTo(vline.points[0].price, 6);
  expect(vline_after.points[0].logical).not.toBeCloseTo(vline.points[0].logical, 3);
});

test("the 3×3 text alignment places labels around the tool's anchor", async ({ page }) => {
  await goto_fixture(page);
  const s = await anchor_spots(page);
  // No crosshair pixels near the probes.
  await page.evaluate(() => window.__chart.apply_options({ crosshair: { mode: 2 } }));
  const anchor_logical = Math.floor((s.l0 + s.l1) / 2);
  const create = (h, v) => page.evaluate(
    ({ logical, price, h, v }) => window.__chart.add_drawing("text", [{ logical, price }], {
      text: "aligned", text_color: "#7b1fa2", text_size: 20, text_h_align: h, text_v_align: v,
    }).id,
    { logical: anchor_logical, price: s.p_mid, h, v },
  );

  const anchor = await spot(page, anchor_logical, s.p_mid);
  const ax = anchor.x * PR;
  const ay = anchor.y * PR;

  const h_id = await create("center", "middle");
  await settle_frames(page);
  const centered = color_centroid(await capture(page), PURPLE);
  expect(centered, "label paints").not.toBeNull();
  expect(Math.abs(centered.x - ax), "center-aligned centroid on the anchor x").toBeLessThan(3 * PR);
  expect(Math.abs(centered.y - ay), "middle-aligned centroid on the anchor y").toBeLessThan(4 * PR);

  await page.evaluate(({ id, h }) => {
    const d = window.__chart.drawings().find((x) => x.id === id);
    d.apply_options({ text_h_align: h });
  }, { id: h_id, h: "left" });
  await settle_frames(page);
  const left = color_centroid(await capture(page), PURPLE);
  expect(left.x, "left-aligned label extends right of the anchor").toBeGreaterThan(ax + 8 * PR);

  await page.evaluate(({ id, h }) => {
    const d = window.__chart.drawings().find((x) => x.id === id);
    d.apply_options({ text_h_align: h });
  }, { id: h_id, h: "right" });
  await settle_frames(page);
  const right = color_centroid(await capture(page), PURPLE);
  expect(right.x, "right-aligned label extends left of the anchor").toBeLessThan(ax - 8 * PR);

  await page.evaluate(({ id, h, v }) => {
    const d = window.__chart.drawings().find((x) => x.id === id);
    d.apply_options({ text_h_align: h, text_v_align: v });
  }, { id: h_id, h: "center", v: "top" });
  await settle_frames(page);
  const top = color_centroid(await capture(page), PURPLE);
  expect(top.y, "top-aligned label sits above the anchor").toBeLessThan(ay - 8 * PR);

  await page.evaluate(({ id, v }) => {
    const d = window.__chart.drawings().find((x) => x.id === id);
    d.apply_options({ text_v_align: v });
  }, { id: h_id, v: "bottom" });
  await settle_frames(page);
  const bottom = color_centroid(await capture(page), PURPLE);
  expect(bottom.y, "bottom-aligned label sits below the anchor").toBeGreaterThan(ay + 8 * PR);

  // The options round-trip reports the applied alignment.
  const options = await page.evaluate(({ id }) => window.__chart.drawings().find((x) => x.id === id).options(), { id: h_id });
  expect(options.text_h_align).toBe("center");
  expect(options.text_v_align).toBe("bottom");
});

test("a trend line carries an aligned text label", async ({ page }) => {
  await goto_fixture(page);
  const s = await anchor_spots(page);
  await page.evaluate(() => window.__chart.apply_options({ crosshair: { mode: 2 } }));
  await page.evaluate(({ l0, l1, p_lo, p_hi }) => {
    window.__chart.add_drawing("trend_line", [
      { logical: l0, price: p_lo },
      { logical: l1, price: p_hi },
    ], { text: "trend label", text_color: "#7b1fa2", text_v_align: "top" });
  }, s);
  await settle_frames(page);
  const with_label = color_centroid(await capture(page), PURPLE);
  expect(with_label, "label paints above the segment").not.toBeNull();
  const box_top = await page.evaluate(({ l0, l1, p_lo, p_hi }) => {
    const ys = [window.__main.price_to_coordinate(p_lo), window.__main.price_to_coordinate(p_hi)];
    return Math.min(...ys);
  }, s);
  // Above the bounding box: the run's center lands at box_top − (pad + size/2), so the glyph
  // ink's centroid is several device px above it.
  expect(with_label.y, "above the segment's bounding box").toBeLessThan(box_top * PR - 8);
});

test("Delete and Backspace remove the selected drawing", async ({ page }) => {
  await goto_fixture(page);
  const s = await anchor_spots(page);
  const add = () => page.evaluate(({ l0, l1, p_lo, p_hi }) => {
    window.__chart.add_drawing("trend_line", [
      { logical: l0, price: p_lo },
      { logical: l1, price: p_hi },
    ]);
  }, s);
  const mid = async () => {
    const points = (await drawings(page))[0].points;
    return {
      x: (await x_of(page, points[0].logical) + await x_of(page, points[1].logical)) / 2,
      y: await page.evaluate(({ points }) => (
        window.__main.price_to_coordinate(points[0].price) +
        window.__main.price_to_coordinate(points[1].price)
      ) / 2, { points }),
    };
  };
  await focus_overlay(page);

  await add();
  let grab = await mid();
  await page.mouse.click(grab.x, grab.y);
  expect(await page.evaluate(() => window.__chart.selected_drawing())).not.toBeNull();
  await page.keyboard.press("Delete");
  expect(await drawings(page)).toHaveLength(0);

  // Backspace works the same way; with nothing selected both keys are no-ops.
  await add();
  grab = await mid();
  await page.mouse.click(grab.x, grab.y);
  await page.keyboard.press("Backspace");
  expect(await drawings(page)).toHaveLength(0);
  await page.keyboard.press("Delete");
  await page.keyboard.press("Backspace");
  expect(await drawings(page)).toHaveLength(0);
});

test("drawing tools render pixel-identical on WebGPU and Canvas2D (AA coverage steps aside)", async ({ page }, test_info) => {
  const run_scenario = async (backend) => {
    await goto_fixture(page, backend);
    await page.evaluate(() => window.__chart.apply_options({ crosshair: { mode: 2 } }));
    await page.evaluate(() => {
      const chart = window.__chart;
      const range = chart.time_scale().get_visible_logical_range();
      const l0 = Math.floor(range.from + (range.to - range.from) * 0.2);
      const l1 = Math.floor(range.from + (range.to - range.from) * 0.75);
      const b0 = window.__main.data_by_index(l0);
      const b1 = window.__main.data_by_index(l1);
      const lo = Math.min(b0.low, b1.low);
      const hi = Math.max(b0.high, b1.high);
      chart.add_drawing("trend_line", [
        { logical: l0, price: lo },
        { logical: l1, price: hi },
      ], { text: "trend", text_v_align: "top", width: 3 });
      chart.add_drawing("horizontal_line", [{ logical: l0, price: (lo + hi) / 2 }], {
        style: "dashed", text: "h-line", text_h_align: "left",
      });
      chart.add_drawing("horizontal_ray", [{ logical: Math.floor((l0 + l1) / 2), price: lo - 1 }], { width: 2 });
      chart.add_drawing("vertical_line", [{ logical: Math.floor((l0 + l1) / 2), price: 0 }], { style: "dotted" });
      chart.add_drawing("rectangle", [
        { logical: l0 + 2, price: lo },
        { logical: l0 + 14, price: lo + (hi - lo) / 2 },
      ], { color: "#e91e63" });
      chart.add_drawing("text", [{ logical: l1 - 4, price: hi + 0.5 }], {
        text: "parity label", text_size: 16, text_bold: true, text_h_align: "right", text_v_align: "bottom",
        text_color: "#7b1fa2",
      });
      // A boxed text (container background + border) and an empty text (muted placeholder).
      chart.add_drawing("text", [{ logical: l0 + 8, price: hi }], {
        text: "boxed", box_color: "rgba(41, 98, 255, 0.85)", box_border_color: "#e91e63",
        box_border_width: 2, text_color: "#ffffff",
      });
      chart.add_drawing("text", [{ logical: l0 + 16, price: lo + (hi - lo) / 4 }], {});
      // A styled label (heavy weight + italic) — its own atlas/font-spec path.
      chart.add_drawing("text", [{ logical: Math.floor((l0 + l1) / 2), price: lo - 2 }], {
        text: "styled 800 italic", text_weight: 800, text_italic: true, text_color: "#e91e63",
      });
      // A smooth brush stroke (curved polyline through a simplified path).
      chart.add_drawing("brush", [
        { logical: l0 + 4, price: lo - 0.5 },
        { logical: Math.floor((l0 + l1) / 2), price: hi + 0.8 },
        { logical: l1 - 2, price: lo - 0.5 },
      ], { color: "#7b1fa2", width: 3 });
      // One interactive creation through the click flow shares the engine path too.
      chart.set_drawing_tool("trend_line", { color: "#26a69a" });
    });
    const s = await anchor_spots(page);
    const p1 = await spot(page, s.l0, s.p_mid);
    const p2 = await spot(page, s.l1, s.p_lo);
    await page.mouse.click(p1.x, p1.y);
    await page.mouse.click(p2.x, p2.y);
    // Anchor handles: select the first drawing.
    await page.evaluate(() => {
      const first = window.__chart.drawings()[0];
      window.__chart.wasm.set_selected_drawing(first.id);
      window.__chart.render();
    });
    await settle_frames(page);
    return {
      backend: await page.evaluate(() => window.__chart.backend()),
      png: PNG.sync.read(await page.screenshot({ animations: "disabled", fullPage: false })),
    };
  };

  const canvas = await run_scenario("canvas2d");
  expect(canvas.backend).toBe("canvas2d");
  const gpu = await run_scenario("auto");
  expect(gpu.backend).toBe("webgpu");
  expect([canvas.png.width, canvas.png.height]).toEqual([gpu.png.width, gpu.png.height]);

  // Vacuousness guard: the scenario paints substantially on EACH backend (all seven drawings).
  const clean_probe = await (async () => {
    await goto_fixture(page, "canvas2d");
    await page.evaluate(() => window.__chart.apply_options({ crosshair: { mode: 2 } }));
    await settle_frames(page);
    return PNG.sync.read(await page.screenshot({ animations: "disabled", fullPage: false }));
  })();
  expect(pixel_diff(clean_probe, canvas.png), "drawings paint on Canvas2D").toBeGreaterThan(1000);
  expect(pixel_diff(clean_probe, gpu.png), "drawings paint on WebGPU").toBeGreaterThan(1000);

  // The repo's ordering contract (backend-parity.spec.mjs markers gate): zero pixels may differ
  // by more than an AA coverage step — a diagonal stroke's anti-aliased edge legitimately
  // differs between SwiftShader's 4xMSAA and Canvas2D's analytic coverage; anything larger is a
  // paint-order/geometry mismatch.
  let ordering_diff = 0;
  let edge_diff = 0;
  let maximum_channel_delta = 0;
  for (let offset = 0; offset < canvas.png.data.length; offset += 4) {
    let pixel_delta = 0;
    for (let channel = 0; channel < 4; channel += 1) {
      pixel_delta = Math.max(pixel_delta, Math.abs(canvas.png.data[offset + channel] - gpu.png.data[offset + channel]));
    }
    maximum_channel_delta = Math.max(maximum_channel_delta, pixel_delta);
    if (pixel_delta > 96) ordering_diff += 1;
    else if (pixel_delta !== 0) edge_diff += 1;
  }
  console.log(`drawings parity: ${edge_diff} AA-edge pixels (max step ${maximum_channel_delta}), ${ordering_diff} ordering pixels`);
  if (ordering_diff !== 0) {
    const visual = new PNG({ width: canvas.png.width, height: canvas.png.height });
    pixelmatch(canvas.png.data, gpu.png.data, visual.data, canvas.png.width, canvas.png.height, { threshold: 0, includeAA: true });
    await test_info.attach("canvas2d.png", { body: PNG.sync.write(canvas.png), contentType: "image/png" });
    await test_info.attach("webgpu.png", { body: PNG.sync.write(gpu.png), contentType: "image/png" });
    await test_info.attach("diff.png", { body: PNG.sync.write(visual), contentType: "image/png" });
  }
  expect(ordering_diff, "drawing geometry/paint order must match (only AA coverage steps may differ)").toBe(0);
});

test("the demo toolbar arms tools, creates, and clears all", async ({ page }) => {  // The full demo (toolbar visible, grid-native chart) — the drawing group drives the same API.
  await page.goto("/");
  await wait_for_chart(page);
  await page.waitForFunction(() => performance.now() > 600);
  const offset = await page.evaluate(() => {
    const r = document.getElementById("chart_container").getBoundingClientRect();
    return { left: r.left, top: r.top };
  });
  const spots = await anchor_spots(page);
  const at = async (logical, price) => {
    const p = await spot(page, logical, price);
    return { x: p.x + offset.left, y: p.y + offset.top };
  };

  await page.click("#drawings_group [data-tool='trend_line']");
  expect(await page.evaluate(() => window.__chart.active_drawing_tool())).toBe("trend_line");
  const armed = await page.evaluate(() => window.__chart.active_drawing_tool());
  expect(armed).toBe("trend_line");
  const a = await at(spots.l0, spots.p_lo);
  const b = await at(spots.l1, spots.p_hi);
  await page.mouse.click(a.x, a.y);
  await page.mouse.click(b.x, b.y);
  expect(await drawings(page)).toHaveLength(1);
  expect(await page.evaluate(() => window.__chart.active_drawing_tool())).toBeNull();

  // Clicking the button again without placing toggles the tool off.
  await page.click("#drawings_group [data-tool='horizontal_line']");
  await page.click("#drawings_group [data-tool='horizontal_line']");
  expect(await page.evaluate(() => window.__chart.active_drawing_tool())).toBeNull();

  // The label/color inputs template the next drawing; clear all empties the store.
  await page.fill("#drawing_text", "toolbar note");
  await page.click("#drawings_group [data-tool='text']");
  const t = await at(spots.l1, spots.p_mid);
  await page.mouse.click(t.x, t.y);
  const list = await drawings(page);
  expect(list).toHaveLength(2);
  const text_options = await page.evaluate(() => window.__chart.drawings()[1].options());
  expect(text_options.text).toBe("toolbar note");
  await page.click("#clear_drawings");
  expect(await drawings(page)).toHaveLength(0);
});

test("Ctrl magnet snaps placement to the nearest bar's OHLC", async ({ page }) => {
  await goto_fixture(page);
  await page.evaluate(() => window.__chart.set_drawing_tool("trend_line"));
  // Click between two bars, nearer to one bar's close, with Ctrl held: the anchor snaps to
  // that bar's center and to the closest of its open/high/low/close.
  const probe = await page.evaluate(() => {
    const range = window.__chart.time_scale().get_visible_logical_range();
    const index = Math.floor(range.from + (range.to - range.from) * 0.4);
    const bar = window.__main.data_by_index(index + 1);
    // 70% of the way from bar `index` to bar `index + 1`: clearly inside the +1 bar's slot.
    const x = window.__chart.time_scale().logical_to_coordinate(index) +
      (window.__chart.time_scale().logical_to_coordinate(index + 1) -
        window.__chart.time_scale().logical_to_coordinate(index)) * 0.7;
    // Aim just off the close so the snap target is unambiguous among the four prices.
    const y = window.__main.price_to_coordinate(bar.close) + 3;
    return { index, x, y, prices: [bar.open, bar.high, bar.low, bar.close] };
  });
  await page.keyboard.down("Control");
  await page.mouse.click(probe.x, probe.y);
  await page.keyboard.up("Control");
  // Second click WITHOUT Ctrl: stays raw (fractional logical).
  const free = await page.evaluate(() => {
    const range = window.__chart.time_scale().get_visible_logical_range();
    const index = Math.floor(range.from + (range.to - range.from) * 0.75);
    const bar = window.__main.data_by_index(index);
    return {
      x: (window.__chart.time_scale().logical_to_coordinate(index) +
        window.__chart.time_scale().logical_to_coordinate(index + 1)) / 2,
      y: window.__main.price_to_coordinate(bar.close) + 3,
    };
  });
  await page.mouse.click(free.x, free.y);
  const list = await drawings(page);
  expect(list).toHaveLength(1);
  const [first, second] = list[0].points;
  expect(first.logical).toBeCloseTo(probe.index + 1, 6);
  expect(probe.prices).toContainEqual(first.price);
  expect(Number.isInteger(second.logical)).toBe(false);
  expect(probe.prices).not.toContainEqual(second.price);
});

test("Ctrl magnets an anchor drag to a bar's OHLC (and never straightens)", async ({ page }) => {
  await goto_fixture(page);
  const s = await anchor_spots(page);
  await page.evaluate(({ l0, l1, p_lo, p_hi }) => {
    window.__chart.add_drawing("trend_line", [
      { logical: l0, price: p_lo },
      { logical: l1, price: p_hi },
    ]);
  }, s);
  await settle_frames(page);
  // Select with a body click, then drag anchor 1 near a different bar with Ctrl held: the
  // anchor snaps to that bar's center and to the closest of its OHLC — it does NOT straighten
  // to an angle.
  const mid = await page.evaluate(({ l0, l1, p_lo, p_hi }) => ({
    x: (window.__chart.time_scale().logical_to_coordinate(l0) + window.__chart.time_scale().logical_to_coordinate(l1)) / 2,
    y: (window.__main.price_to_coordinate(p_lo) + window.__main.price_to_coordinate(p_hi)) / 2,
  }), s);
  await page.mouse.click(mid.x, mid.y);
  const anchor = await spot(page, s.l1, s.p_hi);
  const probe = await page.evaluate(({ l0 }) => {
    const index = l0 + 3;
    const bar = window.__main.data_by_index(index);
    return {
      index,
      prices: [bar.open, bar.high, bar.low, bar.close],
      x: window.__chart.time_scale().logical_to_coordinate(index),
      y: window.__main.price_to_coordinate(bar.close) + 2,
    };
  }, s);
  await page.mouse.move(anchor.x, anchor.y);
  await page.keyboard.down("Control");
  await page.mouse.down();
  await page.mouse.move(probe.x, probe.y, { steps: 4 });
  await page.mouse.up();
  await page.keyboard.up("Control");
  const points = (await drawings(page))[0].points;
  // Magnet proof: snapped to the bar's center and to one of its OHLC prices.
  expect(points[1].logical).toBeCloseTo(probe.index, 6);
  expect(probe.prices).toContainEqual(points[1].price);
  // Straighten would have forced the dragged anchor onto a 0°/45°/90° ray from anchor 0 —
  // the magnet snap must not be overridden by it (Ctrl never straightens).
  expect(points[1].price).not.toBeCloseTo(points[0].price, 6);
});

test("Shift straightens a trend anchor drag to 45°", async ({ page }) => {
  await goto_fixture(page);
  const s = await anchor_spots(page);
  await page.evaluate(({ l0, l1, p_lo, p_hi }) => {
    window.__chart.add_drawing("trend_line", [
      { logical: l0, price: p_lo },
      { logical: l1, price: p_hi },
    ]);
  }, s);
  await settle_frames(page);
  let points = (await drawings(page))[0].points;
  // Select with a body click at the segment midpoint.
  const mid = await page.evaluate(({ l0, l1, p_lo, p_hi }) => ({
    x: (window.__chart.time_scale().logical_to_coordinate(l0) + window.__chart.time_scale().logical_to_coordinate(l1)) / 2,
    y: (window.__main.price_to_coordinate(p_lo) + window.__main.price_to_coordinate(p_hi)) / 2,
  }), s);
  await page.mouse.click(mid.x, mid.y);
  // Near-diagonal anchor drag with Shift: the segment straightens to 45° (|dx| == |dy| in px).
  const current = {
    x: await x_of(page, points[1].logical),
    y: await page.evaluate((p) => window.__main.price_to_coordinate(p), points[1].price),
  };
  const fixed = {
    x: await x_of(page, points[0].logical),
    y: await page.evaluate((p) => window.__main.price_to_coordinate(p), points[0].price),
  };
  await page.mouse.move(current.x, current.y);
  await page.keyboard.down("Shift");
  await page.mouse.down();
  await page.mouse.move(fixed.x + 55, fixed.y - 48, { steps: 4 });
  await page.mouse.up();
  await page.keyboard.up("Shift");
  points = (await drawings(page))[0].points;
  const pa = {
    x: await x_of(page, points[0].logical),
    y: await page.evaluate((p) => window.__main.price_to_coordinate(p), points[0].price),
  };
  const pb = {
    x: await x_of(page, points[1].logical),
    y: await page.evaluate((p) => window.__main.price_to_coordinate(p), points[1].price),
  };
  expect(Math.abs(Math.abs(pb.x - pa.x) - Math.abs(pb.y - pa.y)), "45°: |dx| == |dy|").toBeLessThan(2);
});

test("Shift constrains a body drag to the dominant axis", async ({ page }) => {
  await goto_fixture(page);
  const s = await anchor_spots(page);
  await page.evaluate(({ l0, l1, p_lo, p_hi }) => {
    window.__chart.add_drawing("trend_line", [
      { logical: l0, price: p_lo },
      { logical: l1, price: p_hi },
    ]);
  }, s);
  await settle_frames(page);
  const before = (await drawings(page))[0];
  const mid = await page.evaluate(({ l0, l1, p_lo, p_hi }) => ({
    x: (window.__chart.time_scale().logical_to_coordinate(l0) + window.__chart.time_scale().logical_to_coordinate(l1)) / 2,
    y: (window.__main.price_to_coordinate(p_lo) + window.__main.price_to_coordinate(p_hi)) / 2,
  }), s);
  // Mostly-horizontal body drag with Shift: prices frozen, logicals moved.
  await page.mouse.move(mid.x, mid.y);
  await page.keyboard.down("Shift");
  await page.mouse.down();
  await page.mouse.move(mid.x + 50, mid.y + 8, { steps: 4 });
  await page.mouse.up();
  await page.keyboard.up("Shift");
  const after = (await drawings(page))[0];
  expect(after.points[0].logical).not.toBeCloseTo(before.points[0].logical, 3);
  expect(after.points[0].price).toBeCloseTo(before.points[0].price, 6);
  expect(after.points[1].price).toBeCloseTo(before.points[1].price, 6);
});

test("Shift squares a rectangle on the second placement click", async ({ page }) => {
  await goto_fixture(page);
  const s = await anchor_spots(page);
  await page.evaluate(() => window.__chart.set_drawing_tool("rectangle"));
  const first = await spot(page, s.l0, s.p_lo);
  await page.mouse.click(first.x, first.y);
  const spacing = await bar_spacing(page);
  await page.keyboard.down("Shift");
  await page.mouse.click(first.x + 3 * spacing, first.y - 2.1 * spacing);
  await page.keyboard.up("Shift");
  const list = await drawings(page);
  expect(list).toHaveLength(1);
  const [a, b] = list[0].points;
  const pa = {
    x: await x_of(page, a.logical),
    y: await page.evaluate((p) => window.__main.price_to_coordinate(p), a.price),
  };
  const pb = {
    x: await x_of(page, b.logical),
    y: await page.evaluate((p) => window.__main.price_to_coordinate(p), b.price),
  };
  expect(Math.abs(Math.abs(pb.x - pa.x) - Math.abs(pb.y - pa.y)), "square: |dx| == |dy|").toBeLessThan(2);
});

test("Ctrl magnets the crosshair to the hovered bar's OHLC", async ({ page }) => {
  await goto_fixture(page);
  // The crosshair's horizontal line is the default crosshair gray — find the pane row with
  // the most of it (the dashed line covers the pane width).
  const CROSS = [149, 152, 161]; // #9598a1
  const crosshair_row = async () => {
    const png = await capture(page);
    const pane_bottom = Math.round((fixture.css_height - fixture.time_axis_height) * PR);
    let best_row = null;
    let best_count = 0;
    for (let y = 0; y < pane_bottom; y += 1) {
      let count = 0;
      for (let x = 0; x < png.width; x += 1) {
        const o = (y * png.width + x) * 4;
        if (
          Math.abs(png.data[o] - CROSS[0]) <= 20 &&
          Math.abs(png.data[o + 1] - CROSS[1]) <= 20 &&
          Math.abs(png.data[o + 2] - CROSS[2]) <= 20
        ) count += 1;
      }
      if (count > best_count) {
        best_count = count;
        best_row = y;
      }
    }
    return best_row;
  };
  // Hover a spot on a bar with a real high/close spread, 30% of the way from the high (an
  // unambiguous nearest-OHLC target): Normal mode leaves the crosshair at the cursor y.
  const probe = await page.evaluate(() => {
    const range = window.__chart.time_scale().get_visible_logical_range();
    let index = Math.floor(range.from + (range.to - range.from) * 0.45);
    let bar = null;
    for (let i = index; i < Math.floor(range.to) - 1; i += 1) {
      const candidate = window.__main.data_by_index(i);
      const span = window.__main.price_to_coordinate(candidate.close) - window.__main.price_to_coordinate(candidate.high);
      if (span > 10) {
        index = i;
        bar = candidate;
        break;
      }
    }
    const high_y = window.__main.price_to_coordinate(bar.high);
    const y = high_y + (window.__main.price_to_coordinate(bar.close) - high_y) * 0.3;
    return { index, bar, x: window.__chart.time_scale().logical_to_coordinate(index), y };
  });
  await page.mouse.move(probe.x, probe.y);
  await settle_frames(page);
  const free_row = await crosshair_row();
  expect(free_row).not.toBeNull();
  expect(Math.abs(free_row - probe.y * PR), "Normal mode: crosshair at the cursor").toBeLessThan(2 * PR);

  // Ctrl held: the line snaps to the bar's high (the nearest of its OHLC in px).
  await page.keyboard.down("Control");
  await settle_frames(page);
  const snapped_row = await crosshair_row();
  const high_y = await page.evaluate((h) => window.__main.price_to_coordinate(h), probe.bar.high);
  expect(Math.abs(snapped_row - high_y * PR), "Ctrl: snapped to the bar's high").toBeLessThan(2 * PR);
  expect(Math.abs(snapped_row - probe.y * PR)).toBeGreaterThan(2 * PR);

  // Released: back to the raw cursor position (the configured mode is untouched).
  await page.keyboard.up("Control");
  await settle_frames(page);
  const released_row = await crosshair_row();
  expect(Math.abs(released_row - probe.y * PR), "released: raw again").toBeLessThan(2 * PR);
});

test("brush: freehand drag draws a simplified smooth stroke with end anchors", async ({ page }) => {
  await goto_fixture(page);
  const s = await anchor_spots(page);
  await page.evaluate(() => window.__chart.set_drawing_tool("brush", { color: "#000000", width: 2 }));
  // Drag an arc across the pane in small steps (the engine decimates + simplifies it).
  const from = await spot(page, s.l0, s.p_lo);
  const bend = await spot(page, Math.floor((s.l0 + s.l1) / 2), s.p_hi);
  const to = await spot(page, s.l1, s.p_lo);
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  const steps = 24;
  for (let i = 1; i <= steps; i += 1) {
    const t = i / steps;
    // Quadratic bezier through the bend for a smooth arc gesture.
    const x = (1 - t) * (1 - t) * from.x + 2 * (1 - t) * t * bend.x + t * t * to.x;
    const y = (1 - t) * (1 - t) * from.y + 2 * (1 - t) * t * bend.y + t * t * to.y;
    await page.mouse.move(x, y);
  }
  await page.mouse.up();
  await settle_frames(page);
  // Committed, selected, simplified: far fewer stored points than raw move steps.
  const list = await drawings(page);
  expect(list).toHaveLength(1);
  expect(list[0].kind).toBe("brush");
  const point_count = list[0].points.length;
  expect(point_count).toBeGreaterThanOrEqual(2);
  expect(point_count).toBeLessThan(steps);
  expect(await page.evaluate(() => window.__chart.selected_drawing()?.id ?? null)).toBe(list[0].id);
  expect(await page.evaluate(() => window.__chart.active_drawing_tool())).toBeNull();

  // The selection shows anchor handles at the TWO ENDS only — not one per path point. A
  // two-anchor trend line's handles give the same blue footprint.
  const with_handles = await capture(page);
  const brush_blue = count_color(with_handles, BLUE);
  expect(brush_blue, "end anchors visible").toBeGreaterThan(40);
  expect(brush_blue, "not one handle per path point").toBeLessThan(200);
  await focus_overlay(page);
  await page.keyboard.press("Escape"); // deselect

  // Body hit: clicking the stroke selects it again; dragging an END anchor re-anchors only it.
  const end = (await drawings(page))[0].points.at(-1);
  const end_px = { x: await x_of(page, end.logical), y: await page.evaluate((p) => window.__main.price_to_coordinate(p), end.price) };
  await page.mouse.click(end_px.x, end_px.y);
  expect(await page.evaluate(() => window.__chart.selected_drawing()?.id ?? null)).toBe(list[0].id);
  const before = (await drawings(page))[0].points;
  await page.mouse.move(end_px.x, end_px.y);
  await page.mouse.down();
  await page.mouse.move(end_px.x + 30, end_px.y - 20, { steps: 4 });
  await page.mouse.up();
  await settle_frames(page);
  const after = (await drawings(page))[0].points;
  expect(after.length).toBe(before.length);
  // Only the last endpoint moved; the first is untouched.
  expect(after[0].logical).toBeCloseTo(before[0].logical, 6);
  expect(after[0].price).toBeCloseTo(before[0].price, 6);
  const moved = after.at(-1);
  expect(Math.hypot(moved.logical - before.at(-1).logical, moved.price - before.at(-1).price)).toBeGreaterThan(0.01);
});

test("brush: a click without a drag discards the stroke", async ({ page }) => {
  await goto_fixture(page);
  const s = await anchor_spots(page);
  await page.evaluate(() => window.__chart.set_drawing_tool("brush"));
  const p = await spot(page, s.l0, s.p_lo);
  await page.mouse.click(p.x, p.y);
  await settle_frames(page);
  expect(await drawings(page)).toHaveLength(0);
  expect(await page.evaluate(() => window.__chart.active_drawing_tool())).toBe("brush");
  // Escape disarms cleanly (no pending state leaks).
  await focus_overlay(page);
  await page.keyboard.press("Escape");
  expect(await page.evaluate(() => window.__chart.active_drawing_tool())).toBeNull();
});

test("changing style mid-edit never wipes the typed text; font size control applies", async ({ page }) => {
  // The full demo (toolbar visible) — the regression: typing in the editor, then changing a
  // style input, used to write the toolbar's EMPTY label field into the drawing, wiping the
  // typed text back to the placeholder.
  await page.goto("/");
  await wait_for_chart(page);
  await page.waitForFunction(() => performance.now() > 600);
  const offset = await page.evaluate(() => {
    const r = document.getElementById("chart_container").getBoundingClientRect();
    return { left: r.left, top: r.top };
  });
  const spots = await anchor_spots(page);
  const p = await spot(page, spots.l0, spots.p_mid);

  await page.click("#drawings_group [data-tool='text']");
  await page.mouse.click(p.x + offset.left, p.y + offset.top);
  const editor = page.locator("#chart_container #aion-text-input");
  await expect(editor).toBeVisible();
  await editor.fill("keep me");
  // Change the weight like a real user (focusing the toolbar input blurs the editor, which
  // commits the text first; the change then applies the style) — the text must survive.
  await page.evaluate(() => document.getElementById("drawing_weight").focus());
  await page.selectOption("#drawing_weight", "700");
  await expect(page.locator("#chart_container #aion-text-editor")).toHaveCount(0);
  const options = await page.evaluate(() => window.__chart.drawings()[0].options());
  expect(options.text).toBe("keep me");
  expect(options.text_weight).toBe(700);

  // The font-size control templates/applies too.
  await page.evaluate(() => {
    const el = document.getElementById("drawing_text_size");
    el.value = "24";
    el.dispatchEvent(new Event("change"));
  });
  const sized = await page.evaluate(() => window.__chart.drawings()[0].options());
  expect(sized.text_size).toBe(24);
  expect(sized.text).toBe("keep me");

  // Selecting the drawing syncs its style into the toolbar inputs.
  const at = { x: p.x + offset.left, y: p.y + offset.top };
  await page.mouse.click(at.x, at.y);
  await page.keyboard.press("Escape"); // close the typing-mode editor the click opened
  const synced = await page.evaluate(() => ({
    weight: document.getElementById("drawing_weight").value,
    size: document.getElementById("drawing_text_size").value,
    text: document.getElementById("drawing_text").value,
  }));
  expect(synced.weight).toBe("700");
  expect(synced.size).toBe("24");
  expect(synced.text).toBe("keep me");
});

test("tool customization templates new drawings and applies live to the selected one", async ({ page }) => {  // The full demo (toolbar visible) — style/width/color are the drawing customization settings.
  await page.goto("/");
  await wait_for_chart(page);
  await page.waitForFunction(() => performance.now() > 600);
  const offset = await page.evaluate(() => {
    const r = document.getElementById("chart_container").getBoundingClientRect();
    return { left: r.left, top: r.top };
  });
  const spots = await anchor_spots(page);
  const at = async (logical, price) => {
    const p = await spot(page, logical, price);
    return { x: p.x + offset.left, y: p.y + offset.top };
  };

  // Template: dotted, width 5, magenta — the next drawing carries them.
  await page.selectOption("#drawing_style", "dotted");
  await page.evaluate(() => {
    const width = document.getElementById("drawing_width");
    width.value = "5";
    width.dispatchEvent(new Event("change"));
    const color = document.getElementById("drawing_color");
    color.value = "#ff00ff";
    color.dispatchEvent(new Event("change"));
  });
  await page.click("#drawings_group [data-tool='trend_line']");
  const a = await at(spots.l0, spots.p_lo);
  const b = await at(spots.l1, spots.p_hi);
  await page.mouse.click(a.x, a.y);
  await page.mouse.click(b.x, b.y);
  const created = await page.evaluate(() => window.__chart.drawings()[0].options());
  expect(created.style).toBe("dotted");
  expect(created.width).toBe(5);
  expect(created.color).toBe("#ff00ff");

  // Live-apply: with the drawing selected, changing the settings updates it in place.
  await page.mouse.click(a.x, a.y);
  expect(await page.evaluate(() => window.__chart.selected_drawing())).not.toBeNull();
  await page.selectOption("#drawing_style", "dashed");
  await page.evaluate(() => {
    const el = document.getElementById("drawing_width");
    el.value = "3";
    el.dispatchEvent(new Event("change"));
  });
  const updated = await page.evaluate(() => window.__chart.drawings()[0].options());
  expect(updated.style).toBe("dashed");
  expect(updated.width).toBe(3);
});

test("text tool: press places and opens typing mode; typing replaces the preview", async ({ page }) => {
  await goto_fixture(page);
  // No crosshair pixels near the probes.
  await page.evaluate(() => window.__chart.apply_options({ crosshair: { mode: 2 } }));
  const s = await anchor_spots(page);
  const p = await spot(page, s.l0, s.p_mid);
  const clean = await capture(page);
  await page.evaluate(() => window.__chart.set_drawing_tool("text"));
  await page.mouse.click(p.x, p.y);
  // Typing mode: the editing chrome is a square, thick blue-bordered box with a focused input
  // and the bold muted "Add text" preview (and nothing else — the engine's own placeholder is
  // suppressed while editing).
  const wrap = page.locator("#chart_container #aion-text-editor");
  await expect(wrap).toBeVisible();
  const editor = wrap.locator("#aion-text-input");
  await expect(editor).toBeFocused();
  const preview = wrap.locator("#aion-text-preview");
  await expect(preview).toHaveText("Add text");
  expect(await wrap.evaluate((el) => getComputedStyle(el).borderRadius)).toBe("0px");
  expect(await wrap.evaluate((el) => getComputedStyle(el).border)).toContain("2px");
  const border_color = await wrap.evaluate((el) => getComputedStyle(el).borderColor);
  expect(border_color).toBe("rgb(41, 98, 255)");
  // One visual only: the canvas's muted-text pixels at the anchor stay at the clean baseline —
  // the engine's own placeholder prim is suppressed while editing (the selected drawing's
  // blue anchor handle stays, correctly).
  const dark_clean = count_dark(crop_around(clean, p.x * PR, p.y * PR, 130, 26));
  const dark_editing = count_dark(crop_around(await capture(page), p.x * PR, p.y * PR, 130, 26));
  expect(dark_editing, "engine placeholder suppressed while editing").toBeLessThanOrEqual(dark_clean);

  // Typing hides the "Add text" preview immediately; the box hugs the typed text.
  await editor.fill("engine label");
  await expect(preview).toBeHidden();
  await page.keyboard.press("Enter");
  await settle_frames(page);
  // Committed: the chrome is gone, the drawing carries the text.
  await expect(page.locator("#chart_container #aion-text-editor")).toHaveCount(0);
  const list = await drawings(page);
  expect(list).toHaveLength(1);
  expect((await page.evaluate(() => window.__chart.drawings()[0].options())).text).toBe("engine label");
});

test("text tool: Escape cancels the edit; clicking the label reopens typing mode", async ({ page }) => {
  await goto_fixture(page);
  const s = await anchor_spots(page);
  await page.evaluate(({ l0, p_mid }) => {
    window.__chart.add_drawing("text", [{ logical: l0, price: p_mid }], { text: "original" });
  }, s);
  await settle_frames(page);
  // Click the label: typing mode opens, prefilled.
  const p = await spot(page, s.l0, s.p_mid);
  await page.mouse.click(p.x, p.y);
  const editor = page.locator("#chart_container #aion-text-input");
  await expect(editor).toBeVisible();
  await expect(editor).toHaveText("original");
  // The preview is hidden for a non-empty edit.
  await expect(page.locator("#aion-text-preview")).toBeHidden();
  // Escape discards the edit.
  await editor.fill("discarded");
  await page.keyboard.press("Escape");
  await settle_frames(page);
  await expect(page.locator("#chart_container #aion-text-editor")).toHaveCount(0);
  expect((await page.evaluate(() => window.__chart.drawings()[0].options())).text).toBe("original");
});

test("empty text shows the muted placeholder, and clicking it opens typing mode", async ({ page }) => {
  await goto_fixture(page);
  // No crosshair pixels near the probes.
  await page.evaluate(() => window.__chart.apply_options({ crosshair: { mode: 2 } }));
  const s = await anchor_spots(page);
  const clean = await capture(page);
  await page.evaluate(({ l0, p_mid }) => {
    window.__chart.add_drawing("text", [{ logical: l0, price: p_mid }]);
  }, s);
  await settle_frames(page);
  // The "Add text" placeholder paints at the anchor (muted) and is a click target.
  const p = await spot(page, s.l0, s.p_mid);
  const with_placeholder = await capture(page);
  expect(
    pixel_diff(crop_around(clean, p.x * PR, p.y * PR, 130, 26), crop_around(with_placeholder, p.x * PR, p.y * PR, 130, 26)),
    "placeholder paints",
  ).toBeGreaterThan(30);
  await page.mouse.click(p.x, p.y);
  const editor = page.locator("#chart_container #aion-text-input");
  await expect(editor).toBeVisible();
  await expect(editor).toHaveText("");
  // While editing, the engine's placeholder is suppressed: the region's muted-text pixels drop
  // back to the clean baseline (the selected drawing's blue anchor handle stays, correctly).
  const dark_clean = count_dark(crop_around(clean, p.x * PR, p.y * PR, 130, 26));
  const dark_placeholder = count_dark(crop_around(with_placeholder, p.x * PR, p.y * PR, 130, 26));
  expect(dark_placeholder, "placeholder text paints").toBeGreaterThan(dark_clean + 20);
  const dark_editing = count_dark(crop_around(await capture(page), p.x * PR, p.y * PR, 130, 26));
  expect(dark_editing, "engine placeholder suppressed while editing").toBeLessThanOrEqual(dark_clean);
  await editor.fill("from placeholder");
  await page.keyboard.press("Enter");
  await settle_frames(page);
  expect((await page.evaluate(() => window.__chart.drawings()[0].options())).text).toBe("from placeholder");
});

test("text drawing moves freely in both directions with a body drag", async ({ page }) => {
  await goto_fixture(page);
  const s = await anchor_spots(page);
  await page.evaluate(({ l0, p_mid }) => {
    window.__chart.add_drawing("text", [{ logical: l0, price: p_mid }], { text: "movable" });
  }, s);
  await settle_frames(page);
  const before = (await drawings(page))[0].points[0];
  const p = await spot(page, s.l0, s.p_mid);
  // Drag the label diagonally: both the logical and the price follow.
  await page.mouse.move(p.x, p.y);
  await page.mouse.down();
  await page.mouse.move(p.x + 45, p.y - 25, { steps: 4 });
  await page.mouse.up();
  await settle_frames(page);
  const after = (await drawings(page))[0].points[0];
  expect(after.logical).not.toBeCloseTo(before.logical, 3);
  expect(after.price).not.toBeCloseTo(before.price, 3);
  // Clicking the label afterwards still opens typing mode (movement does not eat the click).
  const moved_p = { x: p.x + 45, y: p.y - 25 };
  await page.mouse.click(moved_p.x, moved_p.y);
  await expect(page.locator("#chart_container #aion-text-input")).toBeVisible();
  await page.keyboard.press("Escape");
});

test("the editor tracks its anchor through wheel zoom and scroll (no displacement)", async ({ page }) => {
  await goto_fixture(page);
  const s = await anchor_spots(page);
  await page.evaluate(({ l0, p_mid }) => {
    window.__chart.add_drawing("text", [{ logical: l0, price: p_mid }], { text: "anchored" });
  }, s);
  await settle_frames(page);
  const p = await spot(page, s.l0, s.p_mid);
  await page.mouse.click(p.x, p.y);
  const editor = page.locator("#chart_container #aion-text-input");
  await expect(editor).toBeVisible();
  const text_center = () => page.evaluate(() => {
    const editor = document.querySelector("#aion-text-input");
    const range = document.createRange();
    range.selectNodeContents(editor);
    const r = range.getBoundingClientRect();
    return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
  });
  const anchor_of = () => page.evaluate(() => {
    const c = window.__chart.wasm.drawing_point_to_coordinate(window.__chart.drawings()[0].id, 0);
    return { x: c[0], y: c[1] };
  });
  // Wheel-zoom over the pane (does NOT blur the editor): the visible range and the anchor's
  // pixel position change — the editor must follow.
  await page.mouse.move(p.x + 100, p.y);
  await page.mouse.wheel(0, -240);
  await settle_frames(page);
  await expect(editor).toBeVisible();
  let a = await anchor_of();
  let c = await text_center();
  expect(Math.abs(c.x - a.x), "editor follows the anchor through zoom").toBeLessThanOrEqual(2);
  expect(Math.abs(c.y - a.y)).toBeLessThanOrEqual(2);

  // Wheel-scroll pans the chart: same tracking contract.
  await page.mouse.wheel(240, 0);
  await settle_frames(page);
  await expect(editor).toBeVisible();
  a = await anchor_of();
  c = await text_center();
  expect(Math.abs(c.x - a.x), "editor follows the anchor through scroll").toBeLessThanOrEqual(2);
  expect(Math.abs(c.y - a.y)).toBeLessThanOrEqual(2);
  await page.keyboard.press("Escape");
});

test("typing mode keeps the text pixel-anchored (no shift, same size) as it grows", async ({ page }) => {  await goto_fixture(page);
  // No crosshair pixels near the probes.
  await page.evaluate(() => window.__chart.apply_options({ crosshair: { mode: 2 } }));
  const s = await anchor_spots(page);
  await page.evaluate(({ l0, p_mid }) => {
    window.__chart.add_drawing("text", [{ logical: l0, price: p_mid }], { text: "devraj" });
  }, s);
  await settle_frames(page);
  const p = await spot(page, s.l0, s.p_mid);
  await page.mouse.click(p.x, p.y);
  const editor = page.locator("#chart_container #aion-text-input");
  await expect(editor).toBeVisible();
  // The editable's REAL text rect (Range) vs the engine anchor — the calibration's contract.
  const metrics = () => page.evaluate(() => {
    const editor = document.querySelector("#aion-text-input");
    const range = document.createRange();
    range.selectNodeContents(editor);
    const r = range.getBoundingClientRect();
    const cs = getComputedStyle(editor);
    return {
      center: { x: r.left + r.width / 2, y: r.top + r.height / 2 },
      font_size: cs.fontSize,
      font_style: cs.fontStyle,
      font_weight: cs.fontWeight,
    };
  });
  const expected_size = await page.evaluate(() => Math.max(window.__chart.options().layout.fontSize, 12));
  let m = await metrics();
  expect(Math.abs(m.center.x - p.x), "text x on the anchor").toBeLessThanOrEqual(1.5);
  expect(Math.abs(m.center.y - p.y), "text y on the anchor").toBeLessThanOrEqual(1.5);
  // Same font size/style/weight as the engine's committed text.
  expect(m.font_size).toBe(`${expected_size}px`);
  expect(m.font_style).toBe("normal");
  expect(m.font_weight).toBe("400");
  // Growing the text keeps the anchor (no drifting/lifting while typing).
  await editor.fill("devraj the great king of everything");
  m = await metrics();
  expect(Math.abs(m.center.x - p.x), "grown text x still on the anchor").toBeLessThanOrEqual(1.5);
  expect(Math.abs(m.center.y - p.y), "grown text y still on the anchor").toBeLessThanOrEqual(1.5);
  await page.keyboard.press("Escape");
});

test("text styling: weight, italic, and color flow through options and pixels", async ({ page }) => {  await goto_fixture(page);
  // No crosshair pixels near the probes.
  await page.evaluate(() => window.__chart.apply_options({ crosshair: { mode: 2 } }));
  const s = await anchor_spots(page);
  await page.evaluate(({ l0, p_mid }) => {
    window.__chart.add_drawing("text", [{ logical: l0, price: p_mid }], {
      text: "styled",
      text_weight: 800,
      text_italic: true,
      text_color: "#7b1fa2",
    });
  }, s);
  await settle_frames(page);
  const options = await page.evaluate(() => window.__chart.drawings()[0].options());
  expect(options.text_weight).toBe(800);
  expect(options.text_italic).toBe(true);
  expect(options.text_color).toBe("#7b1fa2");
  // Derived legacy boolean (semibold and up reads bold).
  expect(options.text_bold).toBe(true);
  // The styled label paints (purple pixels at the anchor).
  const p = await spot(page, s.l0, s.p_mid);
  expect(color_centroid(await capture(page), PURPLE), "styled label paints").not.toBeNull();

  // Patching down to normal weight + non-italic changes the footprint measurably.
  const heavy = await capture(page);
  await page.evaluate(() => window.__chart.drawings()[0].apply_options({ text_weight: 400, text_italic: false }));
  await settle_frames(page);
  const normal = await capture(page);
  expect(pixel_diff(
    crop_around(heavy, p.x * PR, p.y * PR, 160, 30),
    crop_around(normal, p.x * PR, p.y * PR, 160, 30),
  ), "weight/italic change restyles the label").toBeGreaterThan(50);
  const after = await page.evaluate(() => window.__chart.drawings()[0].options());
  expect(after.text_weight).toBe(400);
  expect(after.text_italic).toBe(false);
  expect(after.text_bold).toBe(false);
});

test("text tool container: background and border make it a box", async ({ page }) => {  await goto_fixture(page);
  const s = await anchor_spots(page);
  await page.evaluate(({ l0, p_mid }) => {
    window.__chart.add_drawing("text", [{ logical: l0, price: p_mid }], {
      text: "boxed",
      box_color: "rgba(41, 98, 255, 0.85)",
      box_border_color: "#e91e63",
      box_border_width: 2,
      text_color: "#ffffff",
    });
  }, s);
  await settle_frames(page);
  const p = await spot(page, s.l0, s.p_mid);
  const crop = crop_around(await capture(page), p.x * PR, p.y * PR, 120, 40);
  expect(count_color(crop, [41, 98, 255], 60), "box background paints").toBeGreaterThan(50);
  expect(count_color(crop, [233, 30, 99], 60), "box border paints").toBeGreaterThan(10);
  // The options round-trip exposes the container settings.
  const options = await page.evaluate(() => window.__chart.drawings()[0].options());
  expect(options.box_color).toBe("rgba(41, 98, 255, 0.85)");
  expect(options.box_border_color).toBe("#e91e63");
  expect(options.box_border_width).toBe(2);
});

test("rectangle: middle pans unselected, drags selected, 8 anchors from the first click", async ({ page }) => {
  await goto_fixture(page);
  const s = await anchor_spots(page);
  await page.evaluate(({ l0, l1, p_lo, p_hi }) => {
    window.__chart.add_drawing("rectangle", [
      { logical: l0, price: p_lo },
      { logical: l1, price: p_hi },
    ], { color: "#2962ff" });
  }, s);
  await settle_frames(page);
  const before = (await drawings(page))[0];
  const center_of = (points) => page.evaluate(({ points }) => ({
    x: (window.__chart.time_scale().logical_to_coordinate(points[0].logical) + window.__chart.time_scale().logical_to_coordinate(points[1].logical)) / 2,
    y: (window.__main.price_to_coordinate(points[0].price) + window.__main.price_to_coordinate(points[1].price)) / 2,
  }), { points });
  const range = () => page.evaluate(() => window.__chart.time_scale().get_visible_logical_range());

  // Unselected: a middle grab pans the chart � the rectangle does not move.
  const r0 = await range();
  const center = await center_of(before.points);
  await page.mouse.move(center.x, center.y);
  await page.mouse.down();
  await page.mouse.move(center.x + 120, center.y, { steps: 5 });
  await page.mouse.up();
  await settle_frames(page);
  const r1 = await range();
  expect(r1.from, "the middle grab panned the chart").not.toBeCloseTo(r0.from, 3);
  expect((await drawings(page))[0].points[0].logical).toBeCloseTo(before.points[0].logical, 6);
  await page.evaluate((r) => window.__chart.time_scale().set_visible_logical_range(r), r0);
  await settle_frames(page);

  // Select with a border click: all 8 anchors paint.
  const border = await page.evaluate(({ points }) => ({
    x: (window.__chart.time_scale().logical_to_coordinate(points[0].logical) + window.__chart.time_scale().logical_to_coordinate(points[1].logical)) / 2,
    y: window.__main.price_to_coordinate(points[1].price),
  }), { points: before.points });
  await page.mouse.click(border.x, border.y);
  await settle_frames(page);
  expect(count_color(await capture(page), BLUE), "8 anchors once selected").toBeGreaterThan(40);

  // Selected: the middle now drags the whole rectangle.
  const spacing = await bar_spacing(page);
  const center2 = await center_of(before.points);
  await page.mouse.move(center2.x, center2.y);
  await page.mouse.down();
  await page.mouse.move(center2.x + 4 * spacing, center2.y, { steps: 5 });
  await page.mouse.up();
  await settle_frames(page);
  const moved = (await drawings(page))[0];
  expect(moved.points[0].logical).toBeCloseTo(before.points[0].logical + 4, 1);
  expect(moved.points[1].logical).toBeCloseTo(before.points[1].logical + 4, 1);

  // Creation: the 8 anchors show from the first click (before the second commits).
  await page.keyboard.press("Delete");
  await page.evaluate(() => window.__chart.set_drawing_tool("rectangle", { color: "#2962ff" }));
  const c1 = await spot(page, s.l0, s.p_lo);
  await page.mouse.click(c1.x, c1.y);
  const c2 = await spot(page, s.l1, s.p_hi);
  await page.mouse.move(c2.x, c2.y);
  await settle_frames(page);
  expect(count_color(await capture(page), BLUE), "8 anchors during the draw").toBeGreaterThan(40);
  await page.keyboard.press("Escape");
});
