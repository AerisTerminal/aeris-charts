import { expect } from "@playwright/test";
import { test, wait_for_chart } from "./page_ready.mjs";
import { PNG } from "pngjs";

// Bar replay through the real DOM host: the engine's trim cursor (no platform cursor, a thick
// primary line, the dimmed future), a click that trims, playback on the controller wake, and the
// Shift+arrow replay keys.

const PRIMARY = [0, 110, 221]; // semantic primary #006edd
const START = 1_700_000_000;
const BARS = 60;

test.beforeEach(async ({ page }) => {
  page.on("pageerror", (error) => console.log(`[browser:pageerror] ${error.message}`));
});

async function load(page) {
  await page.goto("/?runtimeTest=presentedFrame&backend=canvas2d&forceFallbackAdapter=1");
  await wait_for_chart(page);
  await page.evaluate(({ start, bars }) => {
    const candles = Array.from({ length: bars }, (_, i) => ({
      time: start + i * 60, open: 100 + i, high: 103 + i, low: 98 + i, close: 101 + i,
    }));
    window.__main.set_data(candles);
    window.__chart.time_scale().fit_content();
    window.__replay_events = [];
    window.__chart.subscribe_replay_change((state) => window.__replay_events.push(state));
  }, { start: START, bars: BARS });
  await next_frame(page);
}

async function next_frame(page) {
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
}

/** Page coordinates of bar `index`, halfway down the main pane. */
async function bar_point(page, index) {
  return page.evaluate((index) => {
    const bounds = document.getElementById("chart_container").getBoundingClientRect();
    const pane_height = bounds.height - window.__chart.wasm.time_scale_height();
    return {
      x: bounds.left + window.__chart.wasm.pane_left() + window.__chart.time_scale().logical_to_coordinate(index),
      y: bounds.top + pane_height / 2,
      pane_x: window.__chart.time_scale().logical_to_coordinate(index) + window.__chart.wasm.pane_left(),
      pane_height,
      chart_width: bounds.width,
    };
  }, index);
}

async function overlay_cursor(page) {
  return page.evaluate(() => {
    const canvases = document.querySelectorAll("#chart_container canvas");
    return canvases[canvases.length - 1].style.cursor;
  });
}

async function capture(page) {
  const data_url = await page.evaluate(() => window.__chart.take_screenshot().toDataURL("image/png"));
  return PNG.sync.read(Buffer.from(data_url.split(",")[1], "base64"));
}

/** Share of pane rows whose pixel at column `x` is the primary color. */
function primary_column_share(png, x, height) {
  let hits = 0;
  for (let y = 0; y < height; y++) {
    const o = (y * png.width + Math.round(x)) * 4;
    if (PRIMARY.every((channel, i) => Math.abs(png.data[o + i] - channel) <= 30)) hits += 1;
  }
  return hits / height;
}

const close_of = (index) => (START + (index + 1) * 60) * 1_000_000 - 1;

test("the trim cursor replaces the platform cursor and a click trims at the bar", async ({ page }) => {
  await load(page);
  const point = await bar_point(page, 20);
  await page.mouse.move(point.x, point.y);
  expect(await overlay_cursor(page)).toBe("crosshair");

  const armed = await page.evaluate(() => window.__chart.replay_select_start());
  expect(armed.phase).toBe("selecting");
  await page.mouse.move(point.x + 1, point.y);
  await page.mouse.move(point.x, point.y);
  expect(await overlay_cursor(page)).toBe("none");
  await next_frame(page);
  const trim = await capture(page);
  // The screenshot is in device pixels. The 3 CSS px primary line spans the pane, so one CSS
  // pixel either side of its center still belongs to it.
  const scale = trim.width / point.chart_width;
  for (const dx of [-1, 0, 1]) {
    expect(primary_column_share(trim, (point.pane_x + dx) * scale, Math.floor(point.pane_height * scale)))
      .toBeGreaterThan(0.6);
  }

  await page.mouse.click(point.x, point.y);
  const state = await page.evaluate(() => window.__chart.replay_state());
  expect(state).toMatchObject({ phase: "paused", clock_micros: close_of(20), can_step_back: true, can_step_forward: true });
  expect(await overlay_cursor(page)).toBe("crosshair");
  const events = await page.evaluate(() => window.__replay_events.map((event) => event.phase));
  expect(events).toEqual(["selecting", "paused"]);
});

test("playback advances on the controller wake and the replay keys step and pause", async ({ page }) => {
  await load(page);
  await page.evaluate(({ time }) => {
    window.__chart.replay_select_time(time);
    window.__chart.set_replay_speed(10);
    window.__chart.replay_play();
  }, { time: (START + 10 * 60) * 1_000_000 });
  await page.waitForFunction((clock) => window.__chart.replay_state().clock_micros >= clock, close_of(13));

  const overlay = page.locator("#chart_container canvas:last-of-type");
  await overlay.focus();
  await page.keyboard.press("Shift+ArrowDown");
  const paused = await page.evaluate(() => window.__chart.replay_state());
  expect(paused.phase).toBe("paused");
  await page.waitForTimeout(300);
  expect((await page.evaluate(() => window.__chart.replay_state())).clock_micros).toBe(paused.clock_micros);

  await page.keyboard.press("Shift+ArrowRight");
  const stepped = await page.evaluate(() => window.__chart.replay_state().clock_micros);
  expect(stepped).toBe(paused.clock_micros + 60_000_000);
  await page.keyboard.press("Shift+ArrowLeft");
  expect(await page.evaluate(() => window.__chart.replay_state().clock_micros)).toBe(paused.clock_micros);

  const done = await page.evaluate(() => window.__chart.replay_jump_to_realtime());
  expect(done).toMatchObject({ phase: "off", clock_micros: null });
  await expect(page.evaluate(() => window.__chart.replay_step_forward())).rejects.toThrow(/start bar/);
});

test("the demo replay toolbar drives the active chart through the public API", async ({ page }) => {
  await page.goto("/");
  await wait_for_chart(page, { grid: true });
  const bar = page.locator("#replay_bar");
  await expect(bar).toBeHidden();

  await page.locator("#replay_toggle").click();
  await expect(bar).toHaveAttribute("data-phase", "selecting");
  await expect(page.locator("#replay_hint")).toBeVisible();
  await expect(page.locator("#replay_play")).toBeHidden();

  const point = await page.evaluate(() => {
    const bounds = document.getElementById("chart_container").getBoundingClientRect();
    return { x: bounds.left + bounds.width * 0.4, y: bounds.top + bounds.height * 0.4 };
  });
  await page.mouse.move(point.x, point.y);
  await page.mouse.click(point.x, point.y);
  await expect(bar).toHaveAttribute("data-phase", "paused");
  await expect(page.locator("#replay_toggle")).toHaveAttribute("aria-pressed", "true");
  const start = await page.evaluate(() => window.__chart.replay_state().clock_micros);
  expect(start).not.toBeNull();
  await expect(page.locator("#replay_clock")).toContainText("UTC");

  await page.locator("#replay_forward").click();
  const stepped = await page.evaluate(() => window.__chart.replay_state().clock_micros);
  expect(stepped).toBeGreaterThan(start);
  await page.locator("#replay_back").click();
  expect(await page.evaluate(() => window.__chart.replay_state().clock_micros)).toBe(start);

  await page.locator("#replay_speed").selectOption("5");
  await page.locator("#replay_step").selectOption("900");
  expect(await page.evaluate(() => window.__chart.replay_state())).toMatchObject({
    speed: 5, step: { kind: "interval", micros: 900_000_000 },
  });

  await page.locator("#replay_play").click();
  await expect(bar).toHaveAttribute("data-phase", "playing");
  await page.locator("#replay_play").click();
  await expect(bar).toHaveAttribute("data-phase", "paused");

  await page.locator("#replay_exit").click();
  await expect(bar).toBeHidden();
  await expect(page.locator("#replay_toggle")).toHaveAttribute("aria-pressed", "false");
  expect((await page.evaluate(() => window.__chart.replay_state())).phase).toBe("off");
});
