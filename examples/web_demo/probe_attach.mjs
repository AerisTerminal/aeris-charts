import { chromium } from "playwright";
import { PNG } from "pngjs";

const b = await chromium.launch({ channel: "chromium" });
for (const dpr of [1, 1.35, 1.5, 2]) {
  const p = await (await b.newContext({ viewport: { width: 1280, height: 720 }, deviceScaleFactor: dpr })).newPage();
  await p.goto("http://127.0.0.1:4174/");
  await p.waitForFunction(() => window.__chart?.backend?.() !== undefined);
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
    dpr: devicePixelRatio,
  }));
  const shot = PNG.sync.read(await p.screenshot());
  // scan the strip's interior column for a WHITE gap row between price chip and countdown chip
  const x = Math.round((info.pane_w + 10) * 1); // css px; screenshot is device px at dpr... use device coords
  const dev_x = Math.round((info.pane_w + 10) * dpr);
  const dev_y0 = Math.round((info.y - 20) * dpr);
  let gap_rows = 0, box_rows = 0;
  for (let y = dev_y0; y < dev_y0 + Math.round(60 * dpr); y++) {
    const o = (y * shot.width + dev_x) * 4;
    const r = shot.data[o], g = shot.data[o+1], b2 = shot.data[o+2];
    const is_box = r > 200 && g < 130 && b2 < 130;
    if (is_box) box_rows++;
    else if (box_rows > 0 && r > 240 && g > 240 && b2 > 240) gap_rows++;
  }
  // title chip gap: distance between chip right edge and border (in device px)
  let chip_right = -1;
  for (let x = Math.round((info.pane_w - 1) * dpr); x > Math.round((info.pane_w - 60) * dpr); x--) {
    const y = Math.round(info.y * dpr);
    const o = (y * shot.width + x) * 4;
    const r = shot.data[o], g = shot.data[o+1], b2 = shot.data[o+2];
    if (r > 200 && g < 130 && b2 < 130) { chip_right = x; break; }
  }
  const gap_px = chip_right === -1 ? -1 : Math.round(info.pane_w * dpr) - chip_right - 1;
  console.log(`dpr ${dpr}: box_rows=${box_rows} white_gap_rows_inside_cluster=${gap_rows} title_gap_dev_px=${gap_px}`);
  await p.close();
}
await b.close();
