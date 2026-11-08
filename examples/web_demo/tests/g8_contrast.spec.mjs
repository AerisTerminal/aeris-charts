import { test, expect } from "@playwright/test";

// WCAG 2.x thresholds: 4.5:1 for normal-size text, 3:1 for focus indicators (non-text contrast).
const TEXT_AA = 4.5;
const NON_TEXT_AA = 3;

// Mounts a themed host with a column chart, the DOM legend (one visible and one hidden entry), and
// the DOM tooltip, then returns every rendered color pair resolved from computed styles.
async function mount(page, theme) {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  return page.evaluate(async (theme) => {
    const { create_chart, create_general_legend, create_general_tooltip } = await import("/dist/aeris_charts_financial.js");
    const scope = document.createElement("div");
    scope.dataset.theme = theme;
    scope.style.cssText = "position:fixed;left:0;top:0;width:640px;height:440px;z-index:10000;background:var(--surface);color:var(--text-default)";
    const host = document.createElement("div");
    host.style.cssText = "position:relative;width:640px;height:400px";
    const legend_host = document.createElement("div");
    legend_host.style.cssText = "padding:8px;background:var(--surface);color:var(--text-default)";
    // Keyboard entry point directly before the legend, so Tab walks its buttons in name order.
    const sentinel = document.createElement("button");
    sentinel.id = "contrast_sentinel";
    sentinel.textContent = "before legend";
    scope.append(host, sentinel, legend_host);
    document.body.appendChild(scope);
    const chart = await create_chart(host, {
      backend: "canvas2d", autoSize: false, theme,
      initialPane: { horizontal_domain: { type: "category", scale: "band" } },
    });
    chart.apply_options({ theme });
    chart.resize(640, 400, 1);
    chart.add_axis({ id: "x", pane: 0, dimension: "x", scale: "band" });
    chart.add_axis({ id: "y", pane: 0, dimension: "y", scale: "linear" });
    const first = chart.add_series("column", { pane: 0, x_axis_id: "x", y_axis_id: "y", title: "Revenue" });
    const second = chart.add_series("column", { pane: 0, x_axis_id: "x", y_axis_id: "y", title: "Margin" });
    first.set_data([{ x: "A", y: 42 }, { x: "B", y: 55 }]);
    second.set_data([{ x: "A", y: 21 }, { x: "B", y: 28 }]);
    chart.render();
    const legend = create_general_legend(chart, legend_host, { pane: 0 });
    const tooltip = create_general_tooltip(chart, host, { pane: 0 });
    window.__contrast = { chart, scope, legend, tooltip, second };
    const geometry = chart.panes()[0].get_geometry();
    for (let y = Math.ceil(geometry.top); y < geometry.top + geometry.height; y += 5) {
      for (let x = 0; x < geometry.width; x += 5) {
        const hit = chart.general_hit_test(0, x, y);
        if (hit?.series === first.id && hit.row === 0) return { x: x + geometry.left, y };
      }
    }
    throw new Error("the first column has no exact hit");
  }, theme);
}

// Runs in the page: parses computed colors, composites translucent ink (including element
// opacity) over its opaque background, and returns the WCAG ratio.
const page_contrast = () => {
  const parse = (css) => {
    const probe = document.createElement("canvas").getContext("2d");
    probe.fillStyle = "#000";
    probe.fillStyle = css;
    const value = probe.fillStyle;
    if (value.startsWith("#")) {
      return [1, 3, 5].map((i) => parseInt(value.slice(i, i + 2), 16)).concat(1);
    }
    const parts = value.match(/[\d.]+/g).map(Number);
    return [parts[0], parts[1], parts[2], parts[3] ?? 1];
  };
  const over = (fg, bg, extra_alpha = 1) => {
    const a = fg[3] * extra_alpha;
    return [0, 1, 2].map((i) => fg[i] * a + bg[i] * (1 - a));
  };
  const luminance = (rgb) => {
    const [r, g, b] = rgb.map((c) => {
      const s = c / 255;
      return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  };
  const ratio = (a, b) => {
    const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
    return (hi + 0.05) / (lo + 0.05);
  };
  // The nearest ancestor with an opaque-enough background, composited over the scope surface.
  const background_of = (element) => {
    const layers = [];
    for (let node = element; node; node = node.parentElement) {
      const bg = parse(getComputedStyle(node).backgroundColor);
      if (bg[3] > 0) layers.push(bg);
      if (bg[3] >= 1) break;
    }
    let rgb = [255, 255, 255];
    for (const layer of layers.reverse()) rgb = over(layer, rgb);
    return rgb;
  };
  const opacity_of = (element) => {
    let value = 1;
    for (let node = element; node; node = node.parentElement) value *= Number(getComputedStyle(node).opacity);
    return value;
  };
  window.__contrast_pair = (element, color_property = "color") => {
    const style = getComputedStyle(element);
    const bg = background_of(element.parentElement ?? element);
    const fg = over(parse(style[color_property]), bg, color_property === "color" ? opacity_of(element) : 1);
    return {
      fg: style[color_property], bg: `rgb(${bg.map(Math.round).join(", ")})`,
      opacity: opacity_of(element), ratio: ratio(fg, bg),
    };
  };
};

for (const theme of ["light", "dark"]) {
  test(`DOM legend, tooltip, and focus ring keep WCAG AA contrast in the ${theme} theme`, async ({ page }) => {
    const point = await mount(page, theme);
    await page.evaluate(page_contrast);
    const report = {};

    // Hidden legend entries stay listed at reduced opacity; the dimmed title must still read.
    await page.evaluate(() => window.__contrast.second.set_visible(false));
    const legend = page.getByRole("group", { name: "Chart legend" });
    await expect(legend.getByRole("button", { name: "Show Margin" })).toHaveAttribute("aria-pressed", "false");
    report.legend_visible = await legend.getByRole("button", { name: "Hide Revenue" }).evaluate((button) => window.__contrast_pair(button.lastElementChild));
    report.legend_hidden = await legend.getByRole("button", { name: "Show Margin" }).evaluate((button) => window.__contrast_pair(button.lastElementChild));
    expect(report.legend_hidden.opacity).toBeLessThan(1);

    // Keyboard focus shows the shared :focus-visible ring; it must reach 3:1 against the surface.
    await page.locator("#contrast_sentinel").focus();
    await page.keyboard.press("Tab");
    // Recharts' default itemSorter="value" puts "Margin" before "Revenue".
    await expect(legend.getByRole("button", { name: "Show Margin" })).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(legend.getByRole("button", { name: "Hide Revenue" })).toBeFocused();
    const ring = await legend.getByRole("button", { name: "Hide Revenue" }).evaluate((button) => {
      const style = getComputedStyle(button);
      return {
        matches_focus_visible: button.matches(":focus-visible"),
        style: style.outlineStyle, width: parseFloat(style.outlineWidth),
        ...window.__contrast_pair(button, "outlineColor"),
      };
    });
    report.focus_ring = ring;
    expect(ring.matches_focus_visible).toBe(true);
    expect(ring.style).toBe("solid");
    expect(ring.width).toBeGreaterThanOrEqual(2);

    // The tooltip paints its own secondary surface: title, muted series labels, and values.
    await page.mouse.move(point.x, point.y);
    const tooltip = page.getByRole("tooltip");
    await expect(tooltip).toBeVisible();
    report.tooltip = await tooltip.evaluate((element) => {
      const pair = (node) => window.__contrast_pair(node);
      return {
        heading: pair(element.querySelector(".aeris_charts-tooltip__timestamp")),
        label: pair(element.querySelector(".aeris_charts-tooltip__label")),
        value: pair(element.querySelector(".aeris_charts-tooltip__value")),
      };
    });

    console.log(`[g8-contrast ${theme}] ${JSON.stringify(report)}`);
    for (const [name, pair] of [
      ["legend visible", report.legend_visible],
      ["legend hidden", report.legend_hidden],
      ["tooltip heading", report.tooltip.heading],
      ["tooltip label", report.tooltip.label],
      ["tooltip value", report.tooltip.value],
    ]) {
      expect(pair.ratio, `${theme} ${name} ${pair.fg} on ${pair.bg}`).toBeGreaterThanOrEqual(TEXT_AA);
    }
    expect(ring.ratio, `${theme} focus ring ${ring.fg} on ${ring.bg}`).toBeGreaterThanOrEqual(NON_TEXT_AA);

    await page.evaluate(() => {
      const fixture = window.__contrast;
      fixture.tooltip.detach(); fixture.legend.detach(); fixture.chart.remove(); fixture.scope.remove();
    });
  });
}
