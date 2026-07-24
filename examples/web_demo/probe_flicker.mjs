import { chromium } from "playwright";
import { PNG } from "pngjs";

const b = await chromium.launch({ channel: "chromium" });
const p = await (await b.newContext({ viewport: { width: 1280, height: 720 }, deviceScaleFactor: 1 })).newPage();
await p.goto("http://127.0.0.1:4174/");
await p.waitForFunction(() => window.__chart?.backend?.() !== undefined);
await p.evaluate(() => {
  window.__main.apply_options({ title: "AION", title_visible: true, countdown_visible: true, price_line_visible: false });
});
await p.waitForTimeout(300);
const g = await p.evaluate(() => {
  const r = document.querySelectorAll("canvas")[3].getBoundingClientRect();
  return { left: r.left, top: r.top, pane_left: window.__chart.wasm.pane_left() };
});
// probe the price/countdown attachment edge: find the inside box's row boundary
const probe_edge = async () => {
  const info = await p.evaluate(() => {
    const c = window.__chart;
    const y = Math.round(c.price_to_coordinate(window.__data[window.__data.length - 1].close));
    return { y, pane_w: c.time_scale().width() };
  });
  const shot = PNG.sync.read(await p.screenshot());
  // scan the boundary column inside the strip for LABEL-colored box pixels and find gaps
  const x = g.left + g.pane_left + info.pane_w + 8;
  const rows = [];
  for (let y = info.y - 20; y < info.y + 30; y++) {
    const o = (y * shot.width + x) * 4;
    const r = shot.data[o], gg = shot.data[o + 1], b2 = shot.data[o + 2];
    rows.push(r > 200 && gg < 130 ? "#" : ".");
  }
  return { y: info.y, rows: rows.join("") };
};
const before = await probe_edge();
// drag the chart down slowly, re-probe at a few positions
const cx = g.left + g.pane_left + 400, cy = g.top + 250;
await p.mouse.move(cx, cy);
await p.mouse.down();
const edges = [before];
for (let i = 0; i < 6; i++) {
  await p.mouse.move(cx, cy + (i + 1) * 20, { steps: 3 });
  edges.push(await probe_edge());
}
await p.mouse.up();
console.log("attachment rows over drag (#=filled, .=gap):");
edges.forEach((e, i) => console.log(`step ${i} (y=${e.y}): ${e.rows}`));
// lag measurement: time per pointermove frame with countdown on vs off
const measure = async () => {
  return p.evaluate(() => {
    return new Promise((resolve) => {
      const chart = window.__chart;
      const t0 = performance.now();
      let frames = 0;
      const done = () => resolve((performance.now() - t0) / frames);
      const tick = () => {
        frames++;
        if (frames >= 30) done();
        else requestAnimationFrame(tick);
      };
      chart.render();
      requestAnimationFrame(tick);
      setInterval(() => chart.render(), 0);
    });
  });
};
await p.waitForTimeout(200);
console.log("done");
await b.close();
