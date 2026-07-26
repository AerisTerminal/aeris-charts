import { chromium } from "@playwright/test";
import { PNG } from "pngjs";
import { writeFileSync } from "node:fs";

const browser = await chromium.launch({ channel: "chromium" });
const page = await browser.newPage({ viewport: { width: 1280, height: 720 }, deviceScaleFactor: 1.5 });
await page.goto("http://127.0.0.1:4174/");
await page.waitForFunction(() => window.__grid !== undefined && window.__chart?.backend?.() !== undefined);
await page.check("#vol_toggle");
await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
await page.waitForTimeout(400);
const png = PNG.sync.read(await page.screenshot());
writeFileSync("vol_label.png", PNG.sync.write(png));
console.log("saved");
await browser.close();
