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
await p.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
const anchor = await p.evaluate(() => ({
  pane_w: window.__chart.time_scale().width(),
  y: window.__main.price_to_coordinate(window.__cluster_close),
}));
const data_url = await p.evaluate(() => window.__chart.take_screenshot().toDataURL("image/png"));
const shot = PNG.sync.read(Buffer.from(data_url.split(",")[1], "base64"));
const LABEL = [239, 83, 80];
const near = (a, b, tol = 12) => Math.max(Math.abs(a[0]-b[0]), Math.abs(a[1]-b[1]), Math.abs(a[2]-b[2])) <= tol;
const is_label = (c) => near(c, LABEL);
const y = Math.round(anchor.y) - Math.floor(17 / 2);
console.log("anchor:", anchor, "band:", y);
const hits_at = (x) => {
  let hit = 0;
  for (let yy = 0; yy < 17; yy++) {
    const o = ((y + yy) * shot.width + x) * 4;
    if (is_label([shot.data[o], shot.data[o+1], shot.data[o+2]])) hit++;
  }
  return hit;
};
const cols = [];
for (let x = Math.max(0, anchor.pane_w - 120); x < anchor.pane_w - 1; x++) {
  const h = hits_at(x);
  if (h > 0) cols.push([x, h]);
}
console.log(JSON.stringify(cols));
await b.close();
