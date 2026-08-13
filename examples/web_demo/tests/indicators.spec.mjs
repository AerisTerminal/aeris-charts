import { test, expect } from "@playwright/test";
import { PNG } from "pngjs";

// Engine-native indicators: Bollinger band fill, oscillator separate panes with channel strips,
// MACD four-state histogram colors, and the full native set's placement/lineage.

async function wait_grid(page) {
  await page.waitForFunction(() => window.__grid !== undefined && window.__chart?.backend?.() !== undefined);
  await page.evaluate(() => new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  }));
}

function count_color(png, target, tol = 10) {
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

/** Chart screenshot decoded to a PNG plus a row-crop counter (geometry is CSS px, shots are device px). */
async function shot(page) {
  const url = await page.evaluate(() => window.__chart.take_screenshot().toDataURL("image/png"));
  const png = PNG.sync.read(Buffer.from(url.split(",")[1], "base64"));
  const dsf = png.width / 1280; // viewport width
  const crop = (top_css, bottom_css) => {
    const top = Math.max(0, Math.floor(top_css * dsf));
    const bottom = Math.min(png.height, Math.ceil(bottom_css * dsf));
    const sub = new PNG({ width: png.width, height: bottom - top });
    PNG.bitblt(png, sub, 0, top, png.width, bottom - top, 0, 0);
    return sub;
  };
  return { png, dsf, crop };
}

test("bollinger bands paint their background fill between upper and lower", async ({ page }) => {
  await page.goto("/");
  await wait_grid(page);
  await page.evaluate(() => {
    window.__bands = window.__chart.add_bollinger(window.__main, 20, 2);
  });
  await wait_grid(page);
  const { png } = await shot(page);
  // Band color #2196f3 at 0.2 alpha over the white background blends to ~(211, 234, 253).
  expect(count_color(png, [211, 234, 253], 8), "band fill pixels").toBeGreaterThan(200);
});

test("rsi and macd stack their own panes with channel strip and four-state histogram", async ({ page }) => {
  await page.goto("/");
  await wait_grid(page);

  const added = await page.evaluate(() => {
    const events = [];
    window.__chart.subscribe_series_added((e) => events.push({ id: e.series.id, pane: e.pane_index }));
    window.__rsi = window.__chart.add_rsi(window.__main, 14);
    window.__macd = window.__chart.add_macd(window.__main, 12, 26, 9);
    return {
      events: events.length,
      rsi_pane: events[0].pane,
      macd_panes: events.slice(1).map((e) => e.pane),
      rsi_kind: window.__rsi.indicator_info().kind,
      hist_type: window.__macd[2].series_type(),
      hist_slot: window.__macd[2].indicator_info().output_index,
      panes: window.__chart.panes().length,
    };
  });
  expect(added.events).toBe(4); // 1 rsi + 3 macd outputs
  expect(added.rsi_pane).toBe(1);
  expect(added.macd_panes).toEqual([2, 2, 2]);
  expect(added.rsi_kind).toBe("rsi");
  expect(added.hist_type).toBe("histogram");
  expect(added.hist_slot).toBe(2);
  expect(added.panes).toBe(3);

  await wait_grid(page);
  const geo = await page.evaluate(() => window.__chart.panes().map((p) => p.get_geometry()));
  const { crop } = await shot(page);
  // RSI channel strip rgba(120,123,134,0.2) over the canonical dark surface blends to ~#1e2127.
  const rsi_strip = count_color(crop(geo[1].top, geo[1].top + geo[1].height), [30, 33, 39], 6);
  expect(rsi_strip, "rsi 30/70 channel strip pixels").toBeGreaterThan(500);
  // MACD histogram strong-up columns paint the opaque candle green.
  const macd_green = count_color(crop(geo[2].top, geo[2].top + geo[2].height), [8, 153, 129], 10);
  expect(macd_green, "macd histogram strong-state pixels").toBeGreaterThan(20);
});

test("indicator chips: auto-name on, no countdown, 1px default, style overrides", async ({ page }) => {
  await page.goto("/");
  await wait_grid(page);
  const out = await page.evaluate(() => {
    const rsi = window.__chart.add_rsi(window.__main, 14);
    const before = rsi.options();
    // The platform surfaces its own chip: custom name, chip visible, dotted 2px line.
    rsi.apply_options({ title: "RSI(14) 1h", title_visible: true, line_style: 1, line_width: 2 });
    const after = rsi.options();
    return {
      before: {
        title: before.title,
        title_visible: before.title_visible,
        countdown: before.countdown_visible,
        width: before.line_width,
      },
      after: {
        title: after.title,
        title_visible: after.title_visible,
        line_style: after.line_style,
        line_width: after.line_width,
      },
    };
  });
  expect(out.before).toEqual({ title: "RSI 14", title_visible: true, countdown: false, width: 1 });
  expect(out.after).toEqual({ title: "RSI(14) 1h", title_visible: true, line_style: 1, line_width: 2 });
});

test("indicator values inherit source precision at creation", async ({ page }) => {
  await page.goto("/");
  await wait_grid(page);
  const values = await page.evaluate(() => {
    window.__main.apply_options({ price_format: { type: "price", precision: 0, min_move: 1 } });
    const [upper] = window.__chart.add_bollinger(window.__main, 20, 2);
    const inherited = upper.price_formatter()(65475.46);
    upper.apply_options({ price_format: { type: "price", precision: 4, min_move: 0.0001 } });
    return { inherited, overridden: upper.price_formatter()(65475.46) };
  });
  expect(values).toEqual({ inherited: "65,475", overridden: "65,475.4600" });
});

test("stochastic, atr, vwap, and wma register with lineage and placement", async ({ page }) => {
  await page.goto("/");
  await wait_grid(page);
  const out = await page.evaluate(() => {
    const panes_before = window.__chart.panes().length;
    const stoch = window.__chart.add_stochastic(window.__main, 14, 3);
    const atr = window.__chart.add_atr(window.__main, 14);
    const vwap = window.__chart.add_vwap(window.__main, null);
    const wma = window.__chart.add_wma(window.__main, 20);
    const info = (s) => {
      const i = s.indicator_info();
      return { kind: i.kind, period: i.period, deviation: i.deviation, output_index: i.output_index };
    };
    return {
      panes_before,
      panes_after: window.__chart.panes().length,
      stoch: [info(stoch[0]), info(stoch[1])],
      atr: info(atr),
      vwap: info(vwap),
      wma: info(wma),
      overlays_on_price_pane: [vwap, wma].map((s) =>
        window.__chart.panes()[0].get_series().some((p) => p.id === s.id),
      ),
      stoch_same_pane: window.__chart
        .panes()
        .map((p) => p.get_series().some((s) => s.id === stoch[0].id || s.id === stoch[1].id)),
    };
  });
  expect(out.panes_after).toBe(out.panes_before + 2); // stochastic + atr each own a pane
  expect(out.stoch[0]).toEqual({ kind: "stochastic", period: 14, deviation: 3, output_index: 0 });
  expect(out.stoch[1].output_index).toBe(1);
  expect(out.atr.kind).toBe("atr");
  expect(out.vwap).toEqual({ kind: "vwap", period: 0, deviation: null, output_index: 0 });
  expect(out.wma).toEqual({ kind: "wma", period: 20, deviation: null, output_index: 0 });
  expect(out.overlays_on_price_pane).toEqual([true, true]);
  // Both stochastic lines share one pane (and it is not the price pane).
  const stoch_pane = out.stoch_same_pane.findIndex(Boolean);
  expect(stoch_pane).toBeGreaterThan(0);
});
