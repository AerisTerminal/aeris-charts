import { test, expect } from "@playwright/test";

// Browser 60fps gate (roadmap O2 item O1): the native headless gates (crates/nucleuscharts_native/
// examples/perf_gate.rs, interaction_perf.rs) measure engine-CPU cost only. This spec measures
// the full browser path — engine + executor + presentation — as rAF frame deltas during
// scripted pan/zoom/crosshair on a large dataset.
//
// Report-only by default: CI runs WebGPU on SwiftShader (software rasterizer), so absolute
// frame times are not comparable to real GPUs. Set NUCLEUSCHARTS_PERF_STRICT=1 to enforce the
// budgets as hard assertions on machines with a real GPU.

const STRICT = process.env.NUCLEUSCHARTS_PERF_STRICT === "1";
const FRAME_BUDGET_MS = 1000 / 60;
const P95_BUDGET_MS = FRAME_BUDGET_MS * 2;
const BARS = 100_000;
const STEPS = 240;

async function wait_chart(page) {
  await page.waitForFunction(() => window.__chart !== undefined && window.__chart?.backend?.() !== undefined);
  await page.evaluate(() => new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  }));
}

/** Deterministic OHLC data (same LCG family as fixture_data.js) installed via the public API. */
async function install_bars(page, bars) {
  await page.evaluate((n) => {
    let seed = 0x2f6e2b1;
    const rand = () => {
      seed = (seed * 1103515245 + 12345) & 0x7fffffff;
      return seed / 0x7fffffff;
    };
    const data = [];
    let price = 100;
    const t0 = 1577836800; // 2020-01-01
    for (let i = 0; i < n; i += 1) {
      const drift = Math.sin(i * 0.017) * 0.8;
      const next = price + drift + (rand() - 0.5) * 0.4;
      data.push({
        time: t0 + i * 86400,
        open: price,
        high: Math.max(price, next) + 0.4,
        low: Math.min(price, next) - 0.4,
        close: next,
      });
      price = next;
    }
    const started = performance.now();
    window.__main.set_data(data);
    window.__load_ms = performance.now() - started;
  }, bars);
  await page.evaluate(() => new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  }));
  return page.evaluate(() => window.__load_ms);
}

/** Record rAF deltas while `drive` performs an interaction; returns timing stats. */
async function measure_interaction(page, label, drive) {
  await page.evaluate(() => {
    window.__raf = [];
    let last = performance.now();
    const tick = (t) => {
      window.__raf.push(t - last);
      last = t;
      if (window.__raf_recording) requestAnimationFrame(tick);
    };
    window.__raf_recording = true;
    requestAnimationFrame(tick);
  });
  await drive();
  const samples = await page.evaluate(() => {
    window.__raf_recording = false;
    return window.__raf.slice(2); // drop recorder warmup frames
  });
  expect(samples.length).toBeGreaterThan(10);
  samples.sort((a, b) => a - b);
  const mean = samples.reduce((s, v) => s + v, 0) / samples.length;
  const p95 = samples[Math.floor((samples.length - 1) * 0.95)];
  const max = samples[samples.length - 1];
  const over_budget = samples.filter((v) => v > FRAME_BUDGET_MS).length / samples.length;
  const result = { label, frames: samples.length, mean: +mean.toFixed(3), p95: +p95.toFixed(3), max: +max.toFixed(3), over_budget_pct: +(over_budget * 100).toFixed(1) };
  console.log("perf:", JSON.stringify(result));
  if (STRICT) {
    expect(mean, `${label} mean frame`).toBeLessThan(FRAME_BUDGET_MS);
    expect(p95, `${label} p95 frame`).toBeLessThan(P95_BUDGET_MS);
  }
  return result;
}

test.describe("browser perf gate", () => {
  test.setTimeout(120_000);

  test("pan/zoom/crosshair frame deltas at 100k bars", async ({ page }) => {
    await page.goto("/index.html");
    await wait_chart(page);

    const load_ms = await install_bars(page, BARS);
    console.log(`perf: ${JSON.stringify({ label: "set_data", bars: BARS, ms: +load_ms.toFixed(1) })}`);
    if (STRICT) expect(load_ms).toBeLessThan(1000);

    const canvas = page.locator("#chart_container canvas").first();
    const box = await canvas.boundingBox();
    const cx = box.x + box.width / 2;
    const cy = box.y + box.height / 2;

    await measure_interaction(page, "pan", async () => {
      await page.mouse.move(cx - 300, cy);
      await page.mouse.down();
      await page.mouse.move(cx + 300, cy, { steps: STEPS });
      await page.mouse.up();
    });

    await measure_interaction(page, "zoom", async () => {
      await page.mouse.move(cx, cy);
      for (let i = 0; i < STEPS / 4; i += 1) {
        await page.mouse.wheel(0, i % 2 === 0 ? -120 : 120);
        await page.waitForTimeout(8);
      }
    });

    await measure_interaction(page, "crosshair", async () => {
      await page.mouse.move(box.x + 20, cy);
      await page.mouse.move(box.x + box.width - 20, cy, { steps: STEPS });
    });
  });
});
