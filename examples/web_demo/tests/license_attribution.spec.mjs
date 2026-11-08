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

test("general-only charts carry no attribution logo; a financial pane brings it back", async ({ page }) => {
  await page.goto("/");
  const result = await page.evaluate(async () => {
    const api = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:relative;width:350px;height:260px";
    document.body.append(host);
    const chart = await api.create_chart(host, {
      backend: "canvas2d", autoSize: false, theme: "light",
      initialPane: { horizontal_domain: { type: "category", scale: "point" } },
    });
    chart.resize(350, 260, 1);
    chart.add_axis({ id: "x", pane: 0, dimension: "x", scale: "point" });
    chart.add_axis({ id: "y", pane: 0, dimension: "y", scale: "linear" });
    chart.add_series("xy_line", { pane: 0, x_axis_id: "x", y_axis_id: "y" }).set_data([
      { x: "Jan", y: 1200 }, { x: "Feb", y: 1800 }, { x: "Mar", y: 2400 },
    ]);
    chart.apply_options({ generalLegend: { visible: true }, theme: "dark" });
    chart.render();
    const general_only = host.querySelectorAll("a").length;

    const financial = chart.add_pane(true);
    const financial_index = financial.pane_index();
    chart.add_series("line", { pane: financial_index }).set_data([
      { time: 1_700_000_000, value: 10 }, { time: 1_700_000_060, value: 12 },
    ]);
    chart.render();
    const mixed_is_financial = chart.pane_is_financial_time(financial_index);
    const mixed = host.querySelectorAll("a").length;
    const href = host.querySelector("a")?.href;

    chart.remove_pane(financial_index);
    chart.render();
    const after_removal = host.querySelectorAll("a").length;
    chart.remove();
    host.remove();
    return { general_only, mixed_is_financial, mixed, href, after_removal };
  });
  expect(result.general_only).toBe(0);
  expect(result.mixed_is_financial).toBe(true);
  expect(result.mixed).toBe(1);
  expect(result.href).toBe("https://aeristerminal.com/");
  expect(result.after_removal).toBe(0);
});

test("attribution logo sits inside a general bottom pane's plot corner, clear of axis labels", async ({ page }) => {
  await page.goto("/");
  const result = await page.evaluate(async () => {
    const api = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:relative;width:350px;height:370px";
    document.body.append(host);
    const chart = await api.create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(350, 370, 1);
    chart.add_series("line").set_data([
      { time: 1_700_000_000, value: 10 }, { time: 1_700_000_060, value: 12 },
    ]);
    const general = chart.add_pane({ preserve_empty: true, horizontal_domain: { type: "category", scale: "point" } });
    const pane = general.pane_index();
    chart.add_axis({ id: "x", pane, dimension: "x", scale: "point" });
    chart.add_axis({ id: "y", pane, dimension: "y", scale: "linear" });
    chart.add_series("xy_line", { pane, x_axis_id: "x", y_axis_id: "y" }).set_data([
      { x: "Jan", y: 1200 }, { x: "Feb", y: 1800 }, { x: "Mar", y: 2400 },
    ]);
    chart.render();
    const measure = () => {
      const link = host.querySelector("a").getBoundingClientRect();
      const box = host.getBoundingClientRect();
      return { left_gap: link.left - box.left, bottom_gap: box.bottom - link.bottom };
    };
    const plain = measure();
    const plot_left = chart.panes()[pane].get_geometry().left;
    chart.apply_options({ generalLegend: { visible: true } });
    chart.render();
    const with_legend = measure();
    chart.remove();
    host.remove();
    return { plain, with_legend, plot_left };
  });
  expect(result.plot_left).toBeGreaterThan(20);
  expect(result.plain.left_gap).toBeCloseTo(result.plot_left + 8, 0);
  // Above the X-axis tick strip, not over the bottom-left tick labels.
  expect(result.plain.bottom_gap).toBeGreaterThan(8 + 15);
  expect(result.with_legend.bottom_gap).toBeGreaterThan(result.plain.bottom_gap + 10);
});
