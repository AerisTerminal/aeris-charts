import { test, expect } from "@playwright/test";
import { PNG } from "pngjs";

// TradingView-style last-value cluster: title chip + price text + candle-close countdown row,
// held together with side-specific corner radius. These specs drive the live demo page (hourly
// bars ending at the current hour) through the public API only.

const LABEL = [239, 83,80]; // #ef5350 — the deterministic final DOWN bar's label color
const CHIP = LABEL; // the title chip shares the main label color by default
const ROW = 17; // 12px font + 2*2.5 padding

const test_port = Number.parseInt(process.env.NUCLEUSCHARTS_TEST_PORT ?? "4174", 10);
const test_base_url = `http://127.0.0.1:${test_port}`;

async function wait_for_chart(page) {
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined);
  await page.evaluate(() => new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  }));
}

// Pixel probes run at dpr 1 (2px radius = 2 bitmap px; CSS px = bitmap px).
async function open_cluster_page(browser, options) {
  const context = await browser.newContext({
    viewport: { width: 1280, height: 720 },
    deviceScaleFactor: 1,
    colorScheme: "light",
  });
  const page = await context.newPage();
  await page.goto(`${test_base_url}/`);
  await wait_for_chart(page);
  await page.evaluate((opts) => {
    // A deterministic final DOWN bar at the current second: the label color is pinned to
    // #ef5350 and the countdown always has ~1h left (no hour-boundary flake). The bar is a
    // real `update` through the public series API.
    const now = Math.floor(Date.now() / 1000);
    const last = window.__data[window.__data.length - 1];
    const close = last.close - 2;
    window.__cluster_close = close;
    window.__main.update({ time: now, open: last.close, high: last.close + 0.6, low: close - 0.6, close });
    window.__main.apply_options({ price_line_visible: false, ...opts });
  }, options);
  await page.evaluate(() => new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  }));
  return { context, page };
}

async function capture(page) {
  const data_url = await page.evaluate(() => window.__chart.take_screenshot().toDataURL("image/png"));
  return PNG.sync.read(Buffer.from(data_url.split(",")[1], "base64"));
}

async function cluster_anchor(page) {
  return page.evaluate(() => ({
    pane_w: window.__chart.time_scale().width(),
    y: window.__main.price_to_coordinate(window.__cluster_close),
  }));
}

function px(png, x, y) {
  const o = (y * png.width + x) * 4;
  return [png.data[o], png.data[o + 1], png.data[o + 2]];
}

function dist(a, b) {
  return Math.max(Math.abs(a[0] - b[0]), Math.abs(a[1] - b[1]), Math.abs(a[2] - b[2]));
}

function near(a, b, tol = 12) {
  return dist(a, b) <= tol;
}

const is_box = (c) => near(c, LABEL) || near(c, CHIP);

// Locate the OUTSIDE title chip (TradingView geometry: it sits on the pane side, a ~2px gap
// before the axis border). Since the chip shares the label color (candle bodies/wicks match
// too), detection is by box coverage: a chip is a solid rectangle (≥ 70% LABEL pixels over a
// 20px window on the row band); wicks/bodies never fill a window like that.
const is_label = (c) => near(c, LABEL);

function chip_boxes_at(png, y, x0, x1, row_h = 17) {
  // A chip box is a run of columns with ≥ 1 LABEL pixel (allowing ≤ 8px text/AA dips), ≥ 12px
  // wide, averaging ≥ 0.5 coverage. Wicks/bodies are too narrow or too sparse to qualify.
  const hits_at = (x) => {
    let hit = 0;
    for (let yy = 0; yy < row_h; yy += 1) {
      const o = ((y + yy) * png.width + x) * 4;
      if (is_label([png.data[o], png.data[o + 1], png.data[o + 2]])) hit += 1;
    }
    return hit;
  };
  const boxes = [];
  let cur = null;
  let gap = 0;
  for (let x = x0; x < x1; x += 1) {
    const hit = hits_at(x);
    if (hit >= 1) {
      if (!cur) cur = { s: x, e: x, filled: 0 };
      cur.e = x;
      cur.filled += hit;
      gap = 0;
    } else if (cur) {
      gap += 1;
      if (gap > 8) { boxes.push(cur); cur = null; gap = 0; }
    }
  }
  if (cur) boxes.push(cur);
  return boxes.filter((b) => b.e - b.s + 1 >= 12 && b.filled / ((b.e - b.s + 1) * row_h) >= 0.5);
}

function chip_run_near(png, pane_w, anchor_y) {
  const y = Math.round(anchor_y) - Math.floor(17 / 2);
  const boxes = chip_boxes_at(png, y, Math.max(0, pane_w - 120), pane_w - 1);
  if (boxes.length === 0) return { left: -1, right: -1, top: -1, bottom: -1, found: false };
  const last = boxes[boxes.length - 1]; // the border-most box
  return { left: last.s, right: last.e, top: y, bottom: y + 17, found: true };
}

function chip_extent(png, pane_w, anchor_y) {
  const run = chip_run_near(png, pane_w, anchor_y);
  return { left: run.left, right: run.right, top: run.top, bottom: run.bottom, found: run.found };
}

function find_chip(png, pane_w, anchor_y) {
  return chip_run_near(png, pane_w, anchor_y);
}

function count_chip_left(png, pane_w, anchor_y) {
  const y = Math.round(anchor_y) - Math.floor(17 / 2);
  let n = 0;
  for (const box of chip_boxes_at(png, y, Math.max(0, pane_w - 120), pane_w - 1)) n += box.e - box.s + 1;
  return n;
}

// Locate the cluster's painted bounding box: column pane_w+2 (inside the chip, left of its
// centered text) brackets the vertical extent; the right edge is the widest box-colored run
// across the rows (rounded corners only shrink the outer 2 rows).
function find_cluster(png, pane_w) {
  let top = -1;
  let bottom = -1;
  for (let y = 0; y < png.height; y += 1) {
    if (is_box(px(png, pane_w + 2, y))) {
      if (top === -1) top = y;
      bottom = y;
    }
  }
  let right = -1;
  for (let y = top; y <= bottom; y += 1) {
    for (let x = png.width - 1; x >= pane_w; x -= 1) {
      if (is_box(px(png, x, y))) {
        right = Math.max(right, x + 1);
        break;
      }
    }
  }
  return { left: pane_w, top, right, bottom: bottom + 1 };
}

function count_where(png, box, predicate) {
  let n = 0;
  for (let y = box.top; y < box.bottom; y += 1) {
    for (let x = box.left; x < box.right; x += 1) {
      if (predicate(px(png, x, y))) n += 1;
    }
  }
  return n;
}

const count_color = (png, box, color) => count_where(png, box, (c) => near(c, color));
const is_white = (c) => c[0] > 240 && c[1] > 240 && c[2] > 240;

function region_diff(a, b, box) {
  let diff = 0;
  for (let y = box.top; y < box.bottom; y += 1) {
    for (let x = box.left; x < box.right; x += 1) {
      if (dist(px(a, x, y), px(b, x, y)) > 10) diff += 1;
    }
  }
  return diff;
}

test("countdown_timer_needed gates on visibility and data (pure timer logic)", async () => {
  const { countdown_timer_needed } = await import("../dist/nucleuscharts_financial.js");
  expect(countdown_timer_needed([])).toBe(false);
  expect(countdown_timer_needed([{ countdown_visible: true, has_data: true }])).toBe(true);
  expect(countdown_timer_needed([{ countdown_visible: true, has_data: false }])).toBe(false);
  expect(countdown_timer_needed([{ countdown_visible: false, has_data: true }])).toBe(false);
  expect(countdown_timer_needed([{ has_data: true }, { countdown_visible: true, has_data: true }])).toBe(true);
});

test("last-value cluster paints chip, price, and countdown rows; the chip matches the label color", async ({ browser }) => {
  const { context, page } = await open_cluster_page(browser, {
    title: "NUCLEUS",
    title_visible: true,
    countdown_visible: true,
  });
  const anchor = await cluster_anchor(page);
  const on = await capture(page);
  const box = find_cluster(on, anchor.pane_w);
  expect(box.top, "cluster box should be located").toBeGreaterThanOrEqual(0);
  // One connected two-row box (~34px at the default 12px font).
  expect(box.bottom - box.top).toBeGreaterThanOrEqual(ROW * 2 - 4);
  expect(box.bottom - box.top).toBeLessThanOrEqual(ROW * 2 + 4);
  // The title chip sits OUTSIDE the axis strip (pane side, ~2px gap before the border), in the
  // SAME color as the price/countdown chips by default.
  const chip = find_chip(on, anchor.pane_w, anchor.y);
  expect(chip.found, "title chip outside the strip").toBe(true);
  const gap = anchor.pane_w - chip.right - 1;
  expect(Math.abs(gap - 2)).toBeLessThanOrEqual(2);
  const chip_pixel = px(on, chip.left + 3, box.top + Math.floor(ROW / 2));
  const price_pixel = px(on, box.left + 3, box.top + Math.floor(ROW / 2));
  expect(near(chip_pixel, CHIP), `chip pixel ${chip_pixel}`).toBe(true);
  expect(near(price_pixel, LABEL), `price pixel ${price_pixel}`).toBe(true);
  expect(dist(chip_pixel, price_pixel)).toBeLessThanOrEqual(12); // matching colors by default
  // The countdown row sits below the top row, in the main label color, spanning the full width.
  expect(near(px(on, box.left + 3, box.bottom - 3), LABEL)).toBe(true);
  expect(near(px(on, box.right - 4, box.bottom - 3), LABEL)).toBe(true);

  // Disabled (title chip + countdown off, price label on): the cluster rows vanish (diff > 0
  // in the cluster region) and the remaining plain label is a single row.
  await page.evaluate(() => window.__main.apply_options({ title_visible: false, countdown_visible: false }));
  const off = await capture(page);
  expect(region_diff(on, off, box)).toBeGreaterThan(0);
  const plain = find_cluster(off, anchor.pane_w);
  expect(plain.top).toBeGreaterThanOrEqual(0);
  expect(plain.bottom - plain.top).toBeLessThanOrEqual(ROW + 3);
  await context.close();
});

test("cluster parts toggle independently", async ({ browser }) => {
  const { context, page } = await open_cluster_page(browser, {
    title: "NUCLEUS",
    title_visible: true,
    countdown_visible: true,
  });
  const anchor = await cluster_anchor(page);

  // Title chip off: no chip-colored pixels anywhere (inside or outside the strip), price +
  // countdown rows remain.
  await page.evaluate(() => window.__main.apply_options({ title_visible: false }));
  let shot = await capture(page);
  let box = find_cluster(shot, anchor.pane_w);
  expect(box.bottom - box.top).toBeGreaterThanOrEqual(ROW * 2 - 4);
  expect(count_chip_left(shot, anchor.pane_w, anchor.y)).toBe(0);
  expect(count_color(shot, box, LABEL)).toBeGreaterThan(100);
  // Price text (white glyphs on the box) is still painted in the top row.
  expect(count_where(shot, { ...box, bottom: box.top + ROW }, is_white)).toBeGreaterThan(5);

  // Price off (chip + countdown on): the outside title chip returns, NO empty price box inside
  // the strip's top row, and the countdown row keeps its text.
  await page.evaluate(() => window.__main.apply_options({ title_visible: true, last_value_visible: false }));
  shot = await capture(page);
  box = find_cluster(shot, anchor.pane_w);
  const extent = chip_extent(shot, anchor.pane_w, anchor.y);
  expect(extent.found, "outside title chip present").toBe(true);
  // Gap between the chip's right edge and the axis border (~2px, ±2 for AA).
  expect(Math.abs(anchor.pane_w - extent.right - 1 - 2)).toBeLessThanOrEqual(2);
  // Inside the strip the top row is empty above the countdown; the countdown row has text in
  // the MUTED text color (full-contrast white at reduced alpha over the chip → pinkish).
  const is_muted_text = (c) => c[0] > 220 && c[1] > 150 && c[2] > 150 && !is_white(c);
  expect(count_where(shot, { ...box, top: box.top }, is_muted_text)).toBeGreaterThan(5);

  // Countdown off (chip + price on): one inside row, nothing painted below it, price text present.
  await page.evaluate(() => window.__main.apply_options({ last_value_visible: true, countdown_visible: false }));
  shot = await capture(page);
  box = find_cluster(shot, anchor.pane_w);
  expect(box.bottom - box.top).toBeLessThanOrEqual(ROW + 3);
  expect(count_chip_left(shot, anchor.pane_w, anchor.y)).toBeGreaterThan(20);
  expect(count_where(shot, { ...box, bottom: box.top + ROW }, is_white)).toBeGreaterThan(5);

  // Everything off: no cluster at all, no outside chip either.
  await page.evaluate(() => window.__main.apply_options({ last_value_visible: false, title_visible: false }));
  shot = await capture(page);
  expect(find_cluster(shot, anchor.pane_w).top).toBe(-1);
  expect(count_chip_left(shot, anchor.pane_w, anchor.y)).toBe(0);
  await context.close();
});

test("countdown row ticks with the 1s interval timer", async ({ browser }) => {
  const { context, page } = await open_cluster_page(browser, {
    title: "NUCLEUS",
    title_visible: true,
    countdown_visible: true,
  });
  const anchor = await cluster_anchor(page);
  // Let the first tick settle (the timer starts on apply; the first capture must be past the
  // initial pin so both captures read distinct remaining seconds).
  await page.waitForTimeout(1100);
  const first = await capture(page);
  const box = find_cluster(first, anchor.pane_w);
  expect(box.bottom - box.top).toBeGreaterThanOrEqual(ROW * 2 - 4);
  await page.waitForTimeout(1300);
  const second = await capture(page);
  const countdown_row = { left: box.left, right: box.right, top: box.bottom - ROW, bottom: box.bottom };
  expect(region_diff(first, second, countdown_row)).toBeGreaterThan(0);
  await context.close();
});

test("price and countdown chips share an exact edge at any DPR (no attachment gap)", async ({ browser }) => {
  for (const dpr of [1, 1.35, 2]) {
    const context = await browser.newContext({ viewport: { width: 1280, height: 720 }, deviceScaleFactor: dpr, colorScheme: "light" });
    const page = await context.newPage();
    await page.goto(`${test_base_url}/`);
    await wait_for_chart(page);
    await page.evaluate(() => {
      const now = Math.floor(Date.now() / 1000);
      const last = window.__data[window.__data.length - 1];
      const close = last.close - 2;
      window.__cluster_close = close;
      window.__main.update({ time: now, open: last.close, high: last.close + 0.6, low: close - 0.6, close });
      window.__main.apply_options({ title: "NUCLEUS", title_visible: true, countdown_visible: true, price_line_visible: false });
    });
    await page.waitForTimeout(300);
    const shot = PNG.sync.read(await page.screenshot());
    // Boundary column inside the strip (a few device px right of the border): walking down from
    // above the cluster, once inside the boxes there must be NO white row until the boxes end.
    const border_dev_x = Math.round((await page.evaluate(() => window.__chart.wasm.pane_left() + window.__chart.time_scale().width())) * dpr) + Math.round(4 * dpr);
    const y0 = Math.round((await page.evaluate(() => window.__main.price_to_coordinate(window.__cluster_close))) * dpr);
    let entered = false;
    let gap_rows = 0;
    for (let y = y0 - Math.round(30 * dpr); y < y0 + Math.round(45 * dpr); y++) {
      const o = (y * shot.width + border_dev_x) * 4;
      const r = shot.data[o], g = shot.data[o + 1], b2 = shot.data[o + 2];
      const in_box = r > 200 && g < 130 && b2 < 130;
      const white = r > 240 && g > 240 && b2 > 240;
      if (in_box) entered = true;
      else if (entered && white) gap_rows += 1;
    }
    expect(gap_rows, `dpr ${dpr}: white rows between attached chips`).toBe(0);
    await context.close();
  }
});

test("cluster rounds its axis-facing corners and keeps the chart-facing side sharp", async ({ browser }) => {
  const { context, page } = await open_cluster_page(browser, {
    title: "NUCLEUS",
    title_visible: true,
    countdown_visible: true,
  });
  const anchor = await cluster_anchor(page);
  const shot = await capture(page);
  const box = find_cluster(shot, anchor.pane_w);
  expect(box.top).toBeGreaterThanOrEqual(0);
  const strip_bg = px(shot, box.right + 3, box.top + 3);

  // Axis-facing top-right corner (2px radius): the extreme corner pixel is clipped (blended
  // toward the strip background), while the same column a few px down is fully filled.
  const corner_tr = px(shot, box.right - 1, box.top);
  expect(dist(corner_tr, LABEL)).toBeGreaterThan(40);
  expect(dist(corner_tr, strip_bg)).toBeLessThan(dist(corner_tr, LABEL));
  expect(near(px(shot, box.right - 1, box.top + 3), LABEL)).toBe(true);
  // Chart-facing top-left corner of the inside price chip: sharp, fully filled.
  expect(near(px(shot, box.left, box.top), LABEL)).toBe(true);
  // Axis-facing bottom-right corner of the countdown row: clipped the same way.
  const corner_br = px(shot, box.right - 1, box.bottom - 1);
  expect(dist(corner_br, LABEL)).toBeGreaterThan(40);
  expect(near(px(shot, box.right - 1, box.bottom - 4), LABEL)).toBe(true);
  // Chart-facing bottom-left corner: sharp.
  expect(near(px(shot, box.left, box.bottom - 1), LABEL)).toBe(true);
  // The OUTSIDE title chip rounds only its OUTER side: the corner pixel itself is clipped
  // (white, not chip), while the interior and the axis-facing edge are fully filled (sharp).
  const extent = chip_extent(shot, anchor.pane_w, anchor.y);
  expect(extent.found).toBe(true);
  expect(dist(px(shot, extent.left - 1, extent.top), CHIP)).toBeGreaterThan(12); // clipped corner
  expect(near(px(shot, extent.left + 2, extent.top + 2), CHIP)).toBe(true); // interior is full
  expect(near(px(shot, extent.right, extent.top), CHIP)).toBe(true); // axis-facing corner: sharp
  await context.close();
});

test("two clustered series never chain into one box; the volume shows title + volume value", async ({ page }) => {
  await page.goto("/");
  await wait_for_chart(page);
  await page.evaluate(() => {
    const now = Math.floor(Date.now() / 1000);
    const last = window.__data[window.__data.length - 1];
    const close = last.close - 2;
    window.__cluster_close = close;
    window.__main.update({ time: now, open: last.close, high: last.close + 0.6, low: close - 0.6, close });
  });
  await page.check("#vol_toggle");
  await page.evaluate(() => new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  }));

  // The volume cluster: outside title chip "Volume" + volume-formatted value (K/M suffix).
  const shot = await capture(page);
  const anchor = await cluster_anchor(page);
  const axis_x0 = Math.round(anchor.pane_w);
  // The strip between the main cluster's bottom and the volume band must NOT be a continuous
  // red column (the attach-group chaining bug merged every cluster on the strip into one box).
  const main_bottom = Math.round(anchor.y + ROW);
  const strip = new PNG({ width: shot.width - axis_x0, height: Math.max(1, shot.height - main_bottom - 90) });
  PNG.bitblt(shot, strip, axis_x0, main_bottom, strip.width, strip.height, 0, 0);
  let red_rows = 0;
  for (let y = 0; y < strip.height; y++) {
    let red_in_row = 0;
    for (let x = 0; x < strip.width; x++) {
      const o = (y * strip.width + x) * 4;
      if (Math.abs(strip.data[o] - 239) <= 25 && Math.abs(strip.data[o + 1] - 83) <= 25 && Math.abs(strip.data[o + 2] - 80) <= 25) red_in_row++;
    }
    // A chain box would paint red across the whole strip on EVERY row between the clusters.
    if (red_in_row > strip.width * 0.8) red_rows++;
  }
  expect(red_rows, "no merged red column between the two clusters").toBeLessThan(4);
});
