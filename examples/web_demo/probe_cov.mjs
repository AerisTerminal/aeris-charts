import { chromium } from "playwright";
import { PNG } from "pngjs";

const b = await chromium.launch({ channel: "chromium" });
const p = await (await b.newContext({ viewport: { width: 1280, height: 720 }, deviceScaleFactor: 1 })).newPage();
await p.goto("http://127.0.0.1:4174/");
await p.waitForFunction(() => window.__chart?.backend?.() !== undefined);
await p.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
await p.evaluate(() => {
  const now = Math.floor(Date.now() / 1000);
  const last = window.__data[window.__data.length - 1];
  const close = last.close - 2;
  window.__cluster_close = close;
  window.__main.update({ time: now, open: last.close, high: last.close + 0.6, low: close - 0.6, close });
  window.__main.apply_options({ title: "AION", title_visible: true, countdown_visible: true, price_line_visible: false });
});
await p.waitForTimeout(400);
const info = await p.evaluate(() => ({
  pane_w: window.__chart.time_scale().width(),
  y: window.__main.price_to_coordinate(window.__cluster_close),
}));
const data_url = await p.evaluate(() => window.__chart.take_screenshot().toDataURL("image/png"));
const shot = PNG.sync.read(Buffer.from(data_url.split(",")[1], "base64"));
const LABEL = [239, 83, 80];
const near = (a, b, tol = 12) => Math.max(Math.abs(a[0]-b[0]), Math.abs(a[1]-b[1]), Math.abs(a[2]-b[2])) <= tol;
const y = Math.round(info.y) - 8;
console.log("anchor y:", info.y, "scan row band:", y, "pane_w:", info.pane_w);
for (let x = info.pane_w - 120; x < info.pane_w; x += 4) {
  let filled = 0;
  for (let yy = 0; yy < 17; yy++) {
    for (let xx = 0; xx < 4; xx++) {
      const o = ((y + yy) * shot.width + (x + xx)) * 4;
      if (near([shot.data[o], shot.data[o+1], shot.data[o+2]], LABEL)) filled++;
    }
  }
  const cov = (filled / (17 * 4)).toFixed(2);
  if (cov > 0.2) console.log(`x=${x}: coverage ${cov}`);
}
await b.close();
