import { expect, test } from "@playwright/test";

test("attribution logo links to Aeris Terminal, follows the theme, and hides for license keys", async ({ page }) => {
  await page.goto("/");
  const result = await page.evaluate(async () => {
    const api = await import("/dist/aeris_charts_financial.js");
    const make_host = () => {
      const host = document.createElement("div");
      host.style.cssText = "position:relative;width:320px;height:200px";
      document.body.append(host);
      return host;
    };
    const host = make_host();
    const chart = await api.create_chart(host, { theme: "light" });
    const link = host.querySelector("a");
    const light_src = link?.querySelector("img")?.src ?? "";
    const time_axis = chart.time_scale().height();
    const link_rect = link.getBoundingClientRect();
    const host_rect = host.getBoundingClientRect();
    chart.apply_options({ theme: "dark" });
    const dark_src = link?.querySelector("img")?.src ?? "";
    chart.remove();
    const removed = host.querySelector("a") === null;

    const licensed_host = make_host();
    const licensed = await api.create_chart(licensed_host, { licenseKey: "commercial-test-key" });
    const licensed_link = licensed_host.querySelector("a");
    licensed.remove();
    return {
      href: link?.href,
      target: link?.target,
      rel: link?.rel,
      label: link?.getAttribute("aria-label"),
      light_src,
      dark_src,
      bottom_gap: host_rect.bottom - link_rect.bottom,
      left_gap: link_rect.left - host_rect.left,
      time_axis,
      removed,
      licensed_link: licensed_link === null,
    };
  });
  expect(result.href).toBe("https://aeristerminal.com/");
  expect(result.target).toBe("_blank");
  expect(result.rel).toBe("noopener noreferrer");
  expect(result.label).toBe("Charts by Aeris Terminal");
  expect(result.light_src.startsWith("data:image/png;base64,")).toBe(true);
  expect(result.dark_src.startsWith("data:image/png;base64,")).toBe(true);
  expect(result.dark_src).not.toBe(result.light_src);
  expect(result.bottom_gap).toBeCloseTo(result.time_axis + 8, 0);
  expect(result.left_gap).toBeCloseTo(8, 0);
  expect(result.removed).toBe(true);
  expect(result.licensed_link).toBe(true);
});
