import { expect, test, chromium } from "@playwright/test";
import { PNG } from "pngjs";

// The chart's own frame is sharp at every ratio; what the screen shows also depends on how the
// browser composites the canvases. Playwright's deviceScaleFactor emulation resamples even a plain
// canvas, so these checks launch Chromium at a real device scale, clear the emulation, and read the
// composited output.
const test_port = Number.parseInt(process.env.AERIS_CHARTS_TEST_PORT ?? "4174", 10);
const base_url = `http://127.0.0.1:${test_port}`;
const SCALES = [1, 0.9, 1.1, 1.25, 1.333, 1.5, 1.75];
const WEBGPU_ARGS = [
  "--enable-unsafe-webgpu",
  "--enable-unsafe-swiftshader",
  "--use-webgpu-adapter=swiftshader",
  "--enable-dawn-features=allow_unsafe_apis",
  "--disable-dawn-features=use_dxc",
  "--use-gpu-in-tests",
];

// Share of non-background pixels that are neither of the two dominant ink colors. Candle bodies
// and wicks are opaque solids, so a 1:1 bitmap has none; a resampled bitmap blends every edge.
function blended_ink_share(png) {
  const counts = new Map();
  for (let i = 0; i < png.data.length; i += 4) {
    const key = (png.data[i] << 16) | (png.data[i + 1] << 8) | png.data[i + 2];
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  const sorted = [...counts.values()].sort((a, b) => b - a);
  const ink = sorted.slice(1).reduce((sum, n) => sum + n, 0);
  return ink === 0 ? 1 : (ink - (sorted[1] ?? 0) - (sorted[2] ?? 0)) / ink;
}

async function open_page(scale) {
  const browser = await chromium.launch({
    args: [...WEBGPU_ARGS, `--force-device-scale-factor=${scale}`, "--window-size=1280,860"],
  });
  // The suite's default emulation must match the forced scale, or Chromium resamples the page.
  const page = await browser.newPage({ viewport: { width: 1200, height: 780 }, deviceScaleFactor: scale });
  const cdp = await page.context().newCDPSession(page);
  return { browser, page, cdp };
}

async function composited(cdp, clip) {
  const shot = await cdp.send("Page.captureScreenshot", { format: "png", clip: { ...clip, scale: 1 } });
  return PNG.sync.read(Buffer.from(shot.data, "base64"));
}

const settle = (page) => page.evaluate(() => new Promise((resolve) => {
  requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(resolve, 30)));
}));

test("candles stay one device pixel per bitmap pixel at real fractional device scales", async ({ browserName }) => {
  test.skip(browserName !== "chromium", "device scale is forced through a Chromium launch flag");
  test.setTimeout(240_000);
  const results = [];
  for (const scale of SCALES) {
    const { browser, page, cdp } = await open_page(scale);
    try {
      expect(await page.evaluate(() => devicePixelRatio)).toBeCloseTo(scale, 3);
      // Control: a plain canvas sized from its own device-pixel box must composite 1:1 here.
      await page.setContent(`<body style="margin:0;background:#fff"><div style="position:absolute;left:3.3px;top:2.7px;width:400px;height:300px"><canvas id="k" style="width:100%;height:100%;display:block"></canvas></div></body>`);
      await page.evaluate(() => new Promise((resolve) => {
        const canvas = document.getElementById("k");
        const observer = new ResizeObserver((entries) => {
          const size = entries[0].devicePixelContentBoxSize[0];
          canvas.width = size.inlineSize;
          canvas.height = size.blockSize;
          const ctx = canvas.getContext("2d");
          ctx.fillStyle = "#fff";
          ctx.fillRect(0, 0, canvas.width, canvas.height);
          ctx.fillStyle = "#089981";
          for (let x = 5; x < canvas.width; x += 13) ctx.fillRect(x, 0, 1, canvas.height);
          ctx.fillStyle = "#f7525f";
          for (let y = 7; y < canvas.height; y += 17) ctx.fillRect(0, y, canvas.width, 1);
          observer.disconnect();
          requestAnimationFrame(() => requestAnimationFrame(resolve));
        });
        observer.observe(canvas, { box: "device-pixel-content-box" });
      }));
      results.push({ scale, case: "control canvas", share: blended_ink_share(await composited(cdp, { x: 20, y: 20, width: 300, height: 200 })) });
      for (const backend of ["webgpu", "canvas2d"]) {
        await page.goto(`${base_url}/?theme=light${backend === "canvas2d" ? "&backend=canvas2d" : ""}`);
        await page.waitForFunction(() => window.__chart?.backend?.() !== undefined && window.__main);
        if (await page.locator("#sma_toggle").isChecked()) await page.uncheck("#sma_toggle");
        // A fractional page offset: the browser, not the chart, must keep the canvases on the grid.
        await page.evaluate(() => {
          document.body.style.paddingLeft = "0.37px";
          document.body.style.paddingTop = "0.61px";
        });
        await settle(page);
        const g = await page.evaluate(() => {
          const rect = window.__chart.chart_element().getBoundingClientRect();
          return {
            left: rect.left,
            top: rect.top,
            pane_left: window.__chart.wasm.pane_left(),
            pane_width: window.__chart.time_scale().width(),
            backend: window.__chart.backend(),
          };
        });
        const clip = { x: g.left + g.pane_left + 40, y: g.top + 60, width: Math.min(500, g.pane_width - 80), height: 350 };
        results.push({ scale, case: `page ${g.backend}`, share: blended_ink_share(await composited(cdp, clip)) });
      }

      // A chart inside a scrolled container at fractional offsets.
      await page.goto(`${base_url}/?theme=light&backend=canvas2d`);
      await page.waitForFunction(() => window.__chart?.backend?.() !== undefined);
      const box = await page.evaluate(async () => {
        const { create_chart } = await import("/dist/aeris_charts_financial.js");
        const scroller = document.createElement("div");
        scroller.style.cssText = "position:fixed;left:10.37px;top:20.61px;width:700px;height:330px;overflow:auto;background:#fff;z-index:100000";
        const spacer = document.createElement("div");
        spacer.style.height = "13.3px";
        const host = document.createElement("div");
        host.style.cssText = "position:relative;width:640px;height:300px";
        const tail = document.createElement("div");
        tail.style.height = "400px";
        scroller.append(spacer, host, tail);
        document.body.appendChild(scroller);
        const chart = await create_chart(host, { backend: "canvas2d", autoSize: true });
        const series = chart.add_series("candlestick", { up_color: "#089981", down_color: "#f7525f", wick_up_color: "#089981", wick_down_color: "#f7525f", border_visible: false, price_line_visible: false, last_value_visible: false });
        const rows = [];
        let price = 100;
        for (let i = 0; i < 60; i++) {
          const open = price;
          const close = open + Math.sin(i * 0.7) * 3 + (i % 3 === 0 ? -2 : 1.5);
          rows.push({ time: 1_700_000_000 + i * 3600, open, high: Math.max(open, close) + 1.5, low: Math.min(open, close) - 1.5, close });
          price = close;
        }
        series.set_data(rows);
        scroller.scrollTop = 7;
        await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        window.__scroll_chart = chart;
        const rect = host.getBoundingClientRect();
        return { left: rect.left, top: rect.top, pane_left: chart.wasm.pane_left() };
      });
      await settle(page);
      const clip = { x: box.left + box.pane_left + 30, y: box.top + 30, width: 420, height: 220 };
      results.push({ scale, case: "scrolled container", share: blended_ink_share(await composited(cdp, clip)) });
    } finally {
      await browser.close();
    }
  }
  const blurry = results.filter((result) => result.share > 0.02);
  expect(blurry, JSON.stringify(results)).toEqual([]);
});
