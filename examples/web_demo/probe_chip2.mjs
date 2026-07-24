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
await p.waitForTimeout(300);
const info = await p.evaluate(() => ({
  pane_w: window.__chart.time_scale().width(),
  y: Math.round(window.__main.price_to_coordinate(window.__cluster_close)),
}));
const data_url = await p.evaluate(() => window.__chart.take_screenshot().toDataURL("image/png"));
const shot = PNG.sync.read(Buffer.from(data_url.split(",")[1], "base64"));
const { pane_w, y } = info;
console.log("pane_w", pane_w, "y", y);
for (let dy = -10; dy <= 25; dy += 3) {
  const row = [];
  for (let dx = -70; dx <= 15; dx += 3) {
    const o = ((y + dy) * shot.width + (pane_w + dx)) * 4;
    const r = shot.data[o], g = shot.data[o+1], b2 = shot.data[o+2];
    row.push(`${r}/${g}/${b2}`.padEnd(13));
  }
  console.log(`y${String(dy).padStart(3)}:`, row.join(" "));
}
await b.close();
