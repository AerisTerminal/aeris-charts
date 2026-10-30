import { expect } from "@playwright/test";
import { test, wait_for_chart } from "./page_ready.mjs";

// Chart text measured before a web font loads describes the fallback face; the host must
// re-measure on the real DOM host path once the face arrives, without any caller action.

for (const backend of ["webgpu", "canvas2d"]) {
  test(`a web font that finishes loading re-measures chart text (${backend})`, async ({ page }) => {
    await page.goto("/?backend=canvas2d");
    await wait_for_chart(page);
    const result = await page.evaluate(async (backend) => {
      const { create_chart } = await import("/dist/aeris_charts_financial.js");
      const family = "AerisLateFont";
      const options = {
        backend: backend === "webgpu" ? "auto" : "canvas2d",
        autoSize: false,
        width: 640,
        height: 360,
        layout: { fontFamily: `${family}, monospace`, fontSize: 14 },
      };
      const rows = Array.from({ length: 60 }, (_, i) => ({ time: 1_700_000_000 + i * 60, value: 11_111.11 + i * 13.7 }));
      const host = () => {
        const element = document.createElement("div");
        element.style.cssText = "position:absolute;left:-10000px;width:640px;height:360px";
        document.body.append(element);
        return element;
      };
      const frames = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      const early = await create_chart(host(), options);
      early.add_series("line").set_data(rows);
      await frames();
      const fallback_width = early.price_scale("right").width();

      const face = new FontFace(family, "local('Arial'), local('Liberation Sans'), local('Helvetica')");
      document.fonts.add(face);
      await face.load();
      await document.fonts.ready;
      await frames();
      const reloaded_width = early.price_scale("right").width();

      const fresh = await create_chart(host(), options);
      fresh.add_series("line").set_data(rows);
      await frames();
      return {
        backend: early.backend(),
        fallback_width,
        reloaded_width,
        fresh_width: fresh.price_scale("right").width(),
      };
    }, backend);
    expect(result.backend).toBe(backend);
    // The fixture is only meaningful when the loaded face measures differently from the fallback.
    expect(result.fresh_width).not.toBe(result.fallback_width);
    expect(result.reloaded_width).toBe(result.fresh_width);
  });
}
