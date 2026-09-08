// Rebuild the shared icon masks from the original SVG using the pinned browser rasterizer.
// Run from any directory: node examples/web_demo/build_crosshair_icon.mjs [--check]
import { chromium } from "@playwright/test";
import { readFileSync, writeFileSync } from "node:fs";

const source = new URL("../../packages/charts/src/assets/icons/add.svg", import.meta.url);
const target = new URL("../../crates/nucleuscharts_render/src/crosshair_add.alpha", import.meta.url);
const browser = await chromium.launch();
try {
  const page = await browser.newPage();
  const masks = await page.evaluate((svg) => {
    const root = new DOMParser().parseFromString(svg, "image/svg+xml").documentElement;
    const paths = Array.from(root.querySelectorAll("path[d]"), (p) => new Path2D(p.getAttribute("d")));
    for (const circle of root.querySelectorAll("circle")) {
      const path = new Path2D();
      path.arc(Number(circle.getAttribute("cx")), Number(circle.getAttribute("cy")), Number(circle.getAttribute("r")), 0, Math.PI * 2);
      paths.push(path);
    }
    return Array.from({ length: 96 }, (_, index) => {
      const size = index + 1;
      const canvas = document.createElement("canvas");
      canvas.width = canvas.height = size;
      // Software coverage is independent of the machine GPU and canvas MSAA settings.
      const ctx = canvas.getContext("2d", { alpha: true, willReadFrequently: true });
      ctx.scale(size / 24, size / 24);
      ctx.strokeStyle = "#ffffff";
      ctx.lineWidth = Number(root.getAttribute("stroke-width"));
      ctx.lineCap = ctx.lineJoin = "round";
      for (const path of paths) ctx.stroke(path);
      return Array.from(ctx.getImageData(0, 0, size, size).data).filter((_, i) => i % 4 === 3);
    });
  }, readFileSync(source, "utf8"));
  // 97 little-endian offsets, followed by (run length, alpha) byte pairs.
  const offsets = Buffer.alloc(97 * 4);
  const runs = [];
  for (const [index, mask] of masks.entries()) {
    offsets.writeUInt32LE(offsets.length + runs.length, index * 4);
    for (let i = 0; i < mask.length;) {
      let count = 1;
      while (count < 255 && i + count < mask.length && mask[i + count] === mask[i]) count++;
      runs.push(count, mask[i]);
      i += count;
    }
  }
  offsets.writeUInt32LE(offsets.length + runs.length, 96 * 4);
  const bytes = Buffer.concat([offsets, Buffer.from(runs)]);
  if (process.argv.includes("--check")) {
    if (!readFileSync(target).equals(bytes)) throw new Error("Crosshair icon masks differ from the pinned SVG rasterization");
  } else {
    writeFileSync(target, bytes);
  }
  console.log(`Crosshair icon: ${bytes.length} bytes for all 96 pixel sizes`);
} finally {
  await browser.close();
}
