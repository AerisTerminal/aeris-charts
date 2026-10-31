// Automatable half of the G8 screen-reader phase-closure item: what assistive technology
// receives from a Cartesian, polar, layout and mixed financial+general chart. Real NVDA, JAWS
// and VoiceOver listening remains a manual check.
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { expect } from "@playwright/test";
import { test } from "./page_ready.mjs";

const axe_path = createRequire(import.meta.url).resolve("axe-core/axe.min.js");
const catalog = JSON.parse(readFileSync(new URL("../general_catalog.json", import.meta.url), "utf8"));
const example = (id) => catalog.examples.find((candidate) => candidate.id === id);
const HELP_PREFIX = "Keyboard controls. Left and right arrows move between data points.";

async function open_blank(page) {
  // An unknown path stays on the test server without loading the demo shell, so tab order, live
  // regions and axe results belong to the fixture alone.
  await page.goto("/__accessibility_audit__");
  await page.setContent(`<!doctype html><html lang="en"><head><title>Accessibility audit</title></head>
    <body><main id="root"><button id="before" type="button">Before chart</button></main></body></html>`);
}

async function mount(page, id, accessibility) {
  await page.evaluate(async ({ id, entry, accessibility }) => {
    const api = await import("/dist/aeris_charts_financial.js");
    const section = document.createElement("section");
    section.id = `fx-${id}`;
    section.setAttribute("aria-label", `${id} fixture`);
    const host = document.createElement("div");
    host.className = "host";
    host.style.cssText = "position:relative;width:640px;height:360px";
    const legend_host = document.createElement("div");
    const brush_host = document.createElement("div");
    section.append(host, legend_host, brush_host);
    document.querySelector("#root").appendChild(section);
    let chart;
    if (id === "mixed") {
      chart = await api.create_chart(host, { backend: "canvas2d", autoSize: false });
      chart.resize(640, 360, 1);
      chart.panes()[0].get_series()[0].set_data([1, 2, 3, 4].map((day) => ({
        time: 1735689600 + day * 86400, open: 10 + day, high: 12 + day, low: 9 + day, close: 11 + day,
      })));
      const pane = chart.add_pane({ preserve_empty: true, horizontal_domain: { type: "category", scale: "band" } });
      const index = pane.pane_index();
      chart.add_axis({ id: "mixed-x", pane: index, dimension: "x", scale: "band" });
      chart.add_axis({ id: "mixed-y", pane: index, dimension: "y", scale: "linear" });
      chart.add_series("column", { pane: index, x_axis_id: "mixed-x", y_axis_id: "mixed-y", title: "Revenue" })
        .set_data([{ x: "North", y: 7 }, { x: "South", y: 11 }]);
    } else {
      chart = await api.create_chart(host, {
        backend: "canvas2d", autoSize: false, initialPane: { horizontal_domain: entry.horizontal_domain },
      });
      chart.resize(640, 360, 1);
      for (const axis of entry.axes ?? []) chart.add_axis({ ...axis, pane: 0 });
      for (const { kind, data, ...options } of entry.series ?? []) {
        chart.add_series(kind, { x_axis_id: "x", y_axis_id: "y", ...options, pane: 0 }).set_data(data);
      }
      for (const { kind, data, ...options } of entry.layout_series ?? []) {
        chart.add_layout_series(kind, { ...options, pane: 0 }, data);
      }
      api.create_general_legend(chart, legend_host, { pane: 0 });
      if (entry.horizontal_domain.type === "category") api.create_general_brush_controls(chart, brush_host, { axis: "x" });
    }
    if (accessibility !== undefined) {
      const { description, ...options } = accessibility;
      chart.accessibility().apply_options({
        ...options,
        ...(description === undefined ? {} : { messages: { description: () => description } }),
      });
    }
    chart.render();
    (window.__audit ??= {})[id] = chart;
  }, { id, entry: id === "mixed" ? null : example(id), accessibility });
  return page.locator(`#fx-${id}`);
}

async function expect_canvases_hidden(fixture) {
  const states = await fixture.locator("canvas").evaluateAll((canvases) =>
    canvases.map((canvas) => canvas.getAttribute("aria-hidden")));
  expect(states.length).toBeGreaterThan(0);
  expect(states.every((state) => state === "true"), JSON.stringify(states)).toBe(true);
}

async function expect_focus_visible(locator) {
  await expect(locator).toBeFocused();
  const style = await locator.evaluate((element) => {
    const computed = getComputedStyle(element);
    return { style: computed.outlineStyle, width: Number.parseFloat(computed.outlineWidth) };
  });
  expect(style.style, "focused control must draw an outline").not.toBe("none");
  expect(style.width).toBeGreaterThanOrEqual(1);
}

// Chromium's own accessibility tree is what platform screen readers receive. Playwright's
// computed description does not follow aria-describedby into <output>, so read the tree directly.
async function ax_nodes(page) {
  const session = await page.context().newCDPSession(page);
  try {
    const { nodes } = await session.send("Accessibility.getFullAXTree");
    return nodes.filter((node) => !node.ignored).map((node) => ({
      role: node.role?.value ?? "",
      name: node.name?.value ?? "",
      description: node.description?.value ?? "",
    }));
  } finally {
    await session.detach();
  }
}

async function expect_ax_description(page, role, name, description) {
  await expect.poll(async () => (await ax_nodes(page))
    .filter((node) => node.role === role && node.name === name)
    .map((node) => node.description)).toEqual([description]);
}

async function expect_no_exposed_graphics(page) {
  const graphics = (await ax_nodes(page)).filter((node) => ["Canvas", "canvas", "image", "img"].includes(node.role));
  expect(graphics, "chart canvases must stay out of the accessibility tree").toEqual([]);
}

async function press(page, live, key, text) {
  await page.keyboard.press(key);
  if (text instanceof RegExp) await expect(live).toHaveText(text);
  else await expect(live).toHaveText(text);
}

async function expect_no_axe_violations(page) {
  await page.addScriptTag({ path: axe_path });
  const violations = await page.evaluate(async () => {
    const result = await window.axe.run("#root", {
      runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"] },
    });
    return result.violations.map(({ id, nodes }) => ({ id, targets: nodes.map(({ target }) => target) }));
  });
  expect(violations, JSON.stringify(violations, null, 2)).toEqual([]);
}

test("Cartesian chart: focus entry, series/row navigation, legend toggles and brush value", async ({ page }) => {
  await open_blank(page);
  const fixture = await mount(page, "grouped_columns");
  const rows = example("grouped_columns").series.map((series) => ({ title: series.title, data: series.data }));
  const [north, south] = rows;
  const chart = fixture.getByRole("group", { name: "General chart" });
  const surface = chart.getByRole("application");
  const live = fixture.locator(".aeris_charts-a11y-live-region");

  await expect(surface).toHaveAccessibleName("Interactive chart. 3 series. Press H for keyboard help.");
  await expect(surface).toHaveAttribute("aria-roledescription", "Interactive chart pane");
  await expect(surface).toHaveAccessibleDescription(/up and down arrows to switch series/);
  await expect(live).toHaveAttribute("aria-live", "assertive");
  // A general pane has no financial price or time axis, so no inert axis controls are exposed.
  await expect(chart.getByRole("button")).toHaveCount(0);
  await expect_canvases_hidden(fixture);

  await page.locator("#before").focus();
  await page.keyboard.press("Tab");
  await expect_focus_visible(surface);

  await press(page, live, "ArrowRight", `North ${north.data[0].y}, ${north.data[0].x}. Point 1 of ${north.data.length}.`);
  await press(page, live, "ArrowRight", `North ${north.data[1].y}, ${north.data[1].x}. Point 2 of ${north.data.length}.`);
  await press(page, live, "ArrowDown",
    `South, series 2 of 3. South ${south.data[1].y}, ${south.data[1].x}. Point 2 of ${south.data.length}.`);
  await press(page, live, "End", `South ${south.data.at(-1).y}, ${south.data.at(-1).x}. Point ${south.data.length} of ${south.data.length}.`);
  await press(page, live, "Home", `South ${south.data[0].y}, ${south.data[0].x}. Point 1 of ${south.data.length}.`);
  await press(page, live, "Enter", `Selected South ${south.data[0].y}, ${south.data[0].x}. Point 1 of ${south.data.length}.`);
  await press(page, live, "Escape", "Selection cleared.");
  await press(page, live, "h", new RegExp(`^${HELP_PREFIX} Up and down arrows switch between series\\.`));

  // Tab leaves the chart surface (no focus trap) and reaches the legend in document order.
  const legend = fixture.getByRole("group", { name: "Chart legend" });
  await page.keyboard.press("Tab");
  const hide_north = legend.getByRole("button", { name: "Hide North" });
  await expect_focus_visible(hide_north);
  for (const { title } of rows) {
    await expect(legend.getByRole("button", { name: `Hide ${title}` })).toHaveAttribute("aria-pressed", "true");
  }
  await page.keyboard.press("Space");
  const show_north = legend.getByRole("button", { name: "Show North" });
  await expect(show_north).toHaveAttribute("aria-pressed", "false");
  await expect(show_north).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(legend.getByRole("button", { name: "Hide North" })).toHaveAttribute("aria-pressed", "true");

  const brush = fixture.getByRole("group", { name: "Chart range brush" });
  const status = brush.getByRole("status");
  await expect(status).toHaveText("No range selected");
  for (const name of ["Start −", "Start +", "End −", "End +", "Clear"]) {
    await expect(brush.getByRole("button", { name })).toBeDisabled();
  }
  await page.keyboard.press("Tab");
  await page.keyboard.press("Tab");
  await page.keyboard.press("Tab");
  const select_all = brush.getByRole("button", { name: "Select all" });
  await expect_focus_visible(select_all);
  await page.keyboard.press("Enter");
  const categories = north.data.map((row) => row.x);
  await expect(status).toHaveText(`${categories[0]} – ${categories.at(-1)}`);
  const start_later = brush.getByRole("button", { name: "Start +" });
  await expect(start_later).toBeEnabled();
  await page.keyboard.press("Tab");
  await page.keyboard.press("Tab");
  await expect(start_later).toBeFocused();
  // The focused nudge control is described by the current range, so it is read on focus.
  await expect_ax_description(page, "button", "Start +", `${categories[0]} – ${categories.at(-1)}`);
  await page.keyboard.press("Space");
  await expect(status).toHaveText(`${categories[1]} – ${categories.at(-1)}`);
  await expect_ax_description(page, "button", "Start +", `${categories[1]} – ${categories.at(-1)}`);
  await expect_ax_description(page, "application", "Interactive chart. 3 series. Press H for keyboard help.",
    "Use the left and right arrow keys to move between data points, and the up and down arrows to switch series.");
  await expect_no_exposed_graphics(page);

  await expect_no_axe_violations(page);
});

test("polar charts: slice and multi-series navigation, legend slice list", async ({ page }) => {
  await open_blank(page);
  const pie = await mount(page, "pie");
  const radar = await mount(page, "radar");
  const slices = example("pie").series[0].data;
  const [aeris, baseline] = example("radar").series;

  const pie_surface = pie.getByRole("application");
  await expect(pie_surface).toHaveAccessibleName("Interactive chart. Traffic source. Press H for keyboard help.");
  await expect(pie.getByRole("group", { name: "General chart" }).getByRole("button")).toHaveCount(0);
  await expect_canvases_hidden(pie);
  const pie_live = pie.locator(".aeris_charts-a11y-live-region");
  await page.locator("#before").focus();
  await page.keyboard.press("Tab");
  await expect_focus_visible(pie_surface);
  await press(page, pie_live, "ArrowRight", `Traffic source ${slices[0].y}, ${slices[0].x}. Point 1 of ${slices.length}.`);
  // A single-series pane ignores series switching rather than announcing a phantom series.
  await press(page, pie_live, "ArrowDown", `Traffic source ${slices[0].y}, ${slices[0].x}. Point 1 of ${slices.length}.`);
  await press(page, pie_live, "End", `Traffic source ${slices.at(-1).y}, ${slices.at(-1).x}. Point ${slices.length} of ${slices.length}.`);
  await press(page, pie_live, "h", new RegExp(`^${HELP_PREFIX} Page Up`));

  const legend = pie.getByRole("group", { name: "Chart legend" });
  await expect(legend.getByRole("button", { name: "Hide Traffic source" })).toHaveAttribute("aria-pressed", "true");
  const list = legend.getByRole("list", { name: "Traffic source slices" });
  // Recharts' default Legend itemSorter="value" lists slices by name in JavaScript string order.
  const by_name = [...slices].sort((a, b) => (a.x < b.x ? -1 : a.x > b.x ? 1 : 0));
  await expect(list.getByRole("listitem")).toHaveText(by_name.map((slice) => `${slice.x}: ${slice.y}`));

  const radar_surface = radar.getByRole("application");
  await expect(radar_surface).toHaveAccessibleName("Interactive chart. 2 series. Press H for keyboard help.");
  const radar_live = radar.locator(".aeris_charts-a11y-live-region");
  await radar_surface.focus();
  await press(page, radar_live, "ArrowRight", `Aeris ${aeris.data[0].y}, ${aeris.data[0].x}. Point 1 of ${aeris.data.length}.`);
  await press(page, radar_live, "ArrowDown",
    `Baseline, series 2 of 2. Baseline ${baseline.data[0].y}, ${baseline.data[0].x}. Point 1 of ${baseline.data.length}.`);
  await press(page, radar_live, "ArrowUp",
    `Aeris, series 1 of 2. Aeris ${aeris.data[0].y}, ${aeris.data[0].x}. Point 1 of ${aeris.data.length}.`);
  await expect(radar.getByRole("group", { name: "Chart legend" }).getByRole("button")).toHaveText(["Aeris", "Baseline"]);
  await expect_no_exposed_graphics(page);

  await expect_no_axe_violations(page);
});

test("layout charts: Sankey title and description, node/link navigation, exact treemap values", async ({ page }) => {
  await open_blank(page);
  const description = "Visitors moving from acquisition channels to won deals.";
  const sankey = await mount(page, "sankey", { chart_title: "Acquisition flow", description });
  const treemap = await mount(page, "treemap");
  const flow = example("sankey").layout_series[0].data;
  const total = flow.nodes.length + flow.links.length;
  const visitors = flow.links.filter((link) => link.source === "Visitors").reduce((sum, link) => sum + link.value, 0);
  const last_link = flow.links.at(-1);

  const surface = sankey.getByRole("application");
  // Recharts SVG title/desc map to chart_title and messages.description.
  await expect(surface).toHaveAccessibleName("Acquisition flow. Acquisition. Press H for keyboard help.");
  await expect(surface).toHaveAccessibleDescription(description);
  await expect(sankey.getByRole("group", { name: "General chart" }).getByRole("button")).toHaveCount(0);
  await expect_canvases_hidden(sankey);
  const live = sankey.locator(".aeris_charts-a11y-live-region");
  await page.locator("#before").focus();
  await page.keyboard.press("Tab");
  await expect_focus_visible(surface);
  // Before any item is focused, Space reads the series summary; afterwards it selects the item.
  await press(page, live, " ", `Acquisition with ${total} layout items.`);
  await press(page, live, "ArrowRight", `Acquisition, node Visitors, ${visitors}. Item 1 of ${total}.`);
  await press(page, live, "End",
    `Acquisition, link ${last_link.source} to ${last_link.target}, ${last_link.value}. Item ${total} of ${total}.`);
  await press(page, live, "Home", `Acquisition, node Visitors, ${visitors}. Item 1 of ${total}.`);
  await press(page, live, "Enter", `Selected Acquisition, node Visitors, ${visitors}. Item 1 of ${total}, selected.`);
  await press(page, live, "Escape", "Selection cleared.");
  await expect_ax_description(page, "application", "Acquisition flow. Acquisition. Press H for keyboard help.", description);
  await expect_no_exposed_graphics(page);
  await expect(sankey.getByRole("group", { name: "Chart legend" }).getByRole("button", { name: "Hide Acquisition" }))
    .toHaveAttribute("aria-pressed", "true");

  const treemap_surface = treemap.getByRole("application");
  const series = example("treemap").layout_series[0];
  await expect(treemap_surface).toHaveAccessibleName(`Interactive chart. ${series.title}. Press H for keyboard help.`);
  await treemap_surface.focus();
  const treemap_live = treemap.locator(".aeris_charts-a11y-live-region");
  // The root is the engine's floating-point sum of its leaves; it is announced without noise.
  const leaves = series.data.nodes.filter((node) => node.value !== null);
  const root_total = Number(leaves.reduce((sum, node) => sum + node.value, 0).toPrecision(15));
  await press(page, treemap_live, "ArrowRight",
    `${series.title}, node ${series.data.nodes[0].label}, ${root_total}. Item 1 of ${series.data.nodes.length}.`);

  await expect_no_axe_violations(page);
});

test("mixed financial and general chart: per-pane surfaces, axis targets and announcements", async ({ page }) => {
  await open_blank(page);
  const fixture = await mount(page, "mixed");
  const chart = fixture.getByRole("group", { name: "Financial chart" });
  const surfaces = chart.getByRole("application");
  await expect(surfaces).toHaveCount(2);
  const financial = surfaces.nth(0);
  const general = surfaces.nth(1);
  await expect(financial).toHaveAccessibleName("Interactive financial chart. Series 1. Press H for keyboard help.");
  await expect(general).toHaveAccessibleName("Interactive chart. Revenue. Press H for keyboard help.");
  await expect(financial.getByRole("button").evaluateAll((buttons) => buttons.map((button) => button.getAttribute("aria-label"))))
    .resolves.toEqual(["Pane 1 price axis", "Separator after pane 1"]);
  // The general pane owns no price axis; the shared financial time axis stays reachable last.
  await expect(general.getByRole("button").evaluateAll((buttons) => buttons.map((button) => button.getAttribute("aria-label"))))
    .resolves.toEqual(["Time axis"]);
  await expect_canvases_hidden(fixture);

  await page.locator("#before").focus();
  const order = [];
  for (let step = 0; step < 6; step++) {
    await page.keyboard.press("Tab");
    order.push(await page.evaluate(() => document.activeElement?.getAttribute("aria-label") ?? document.activeElement?.tagName));
  }
  expect(order).toEqual([
    "Interactive financial chart. Series 1. Press H for keyboard help.",
    "Pane 1 price axis",
    "Separator after pane 1",
    "Interactive chart. Revenue. Press H for keyboard help.",
    "Time axis",
    "BODY",
  ]);

  const [financial_live, general_live] = [0, 1].map((index) => fixture.locator(".aeris_charts-a11y-live-region").nth(index));
  await financial.focus();
  await expect_focus_visible(financial);
  await press(page, financial_live, "ArrowRight",
    /^Series 1 open 11\.00, high 13\.00, low 10\.00, close 12\.00, .+\. Point 1 of 4\.$/);
  await press(page, financial_live, "End", /^Series 1 open 14\.00, high 16\.00, low 13\.00, close 15\.00, .+\. Point 4 of 4\.$/);
  await press(page, financial_live, "Enter",
    /^Series 1 with 4 data points in view\. From 12\.00 on .+ to 15\.00 on .+\. Overall up by 3\.00, 25\.00 percent\./);
  await general.focus();
  await expect_focus_visible(general);
  await press(page, general_live, "ArrowRight", "Revenue 7, North. Point 1 of 2.");
  await press(page, general_live, "ArrowRight", "Revenue 11, South. Point 2 of 2.");
  await press(page, general_live, "Enter", "Selected Revenue 11, South. Point 2 of 2.");
  await expect_no_exposed_graphics(page);

  await expect_no_axe_violations(page);
});
