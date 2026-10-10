import { test, expect } from "@playwright/test";

test("general charts can own the first pane and failed creation leaves no host resources", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.width = "640px";
    host.style.height = "320px";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d",
      autoSize: false,
      initialPane: { horizontal_domain: { type: "category", scale: "point" } },
    });
    const initial = {
      pane_count: chart.panes().length,
      series_count: chart.panes()[0].get_series().length,
      aria_label: host.getAttribute("aria-label"),
      domain: chart.export_state().panes[0].horizontal_domain,
    };
    const initial_pane = chart.panes()[0];
    initial.retired = chart.remove_pane(0);
    try {
      initial_pane.pane_index();
      initial.old_handle_stale = false;
    } catch (error) {
      initial.old_handle_stale = error.code === "stale_handle";
    }
    initial.replacement_schema = chart.export_state().schema_version;
    initial.replacement_is_not_removable = chart.remove_pane(0) === false;
    const first_financial = chart.addSeries("line");
    first_financial.setData([
      { time: 1, value: 10 },
      { time: 2, value: 12 },
    ]);
    const second_financial = chart.addSeries("area");
    second_financial.setData([
      { time: 1, value: 9 },
      { time: 2, value: 11 },
    ]);
    initial.financial_after_retirement = {
      first_id: first_financial.id,
      second_id: second_financial.id,
      pane_series: chart.panes()[0].get_series().map((series) => series.id),
      series_order: chart.series_order().map((series) => series.id),
      first_value: first_financial.data_by_index(1, 0)?.value,
      second_value: second_financial.data_by_index(1, 0)?.value,
    };
    chart.remove();

    const rejected_host = document.createElement("div");
    rejected_host.style.width = "320px";
    rejected_host.style.height = "180px";
    document.body.appendChild(rejected_host);
    let rejected = false;
    try {
      await create_chart(rejected_host, {
        backend: "canvas2d",
        initialPane: { horizontal_domain: { type: "unsupported" } },
      });
    } catch (error) {
      rejected = true;
      rejected_host.dataset.errorName = error.name;
      rejected_host.dataset.errorCode = error.code;
    }
    const failed = {
      rejected,
      child_count: rejected_host.childElementCount,
      inline_position: rejected_host.style.position,
      error_name: rejected_host.dataset.errorName,
      error_code: rejected_host.dataset.errorCode,
      role: rejected_host.getAttribute("role"),
      aria_label: rejected_host.getAttribute("aria-label"),
    };
    const late_rejected_host = document.createElement("div");
    late_rejected_host.style.cssText = "position: static !important; width: 320px; height: 180px";
    late_rejected_host.setAttribute("role", "region");
    late_rejected_host.setAttribute("aria-label", "Consumer chart host");
    document.body.appendChild(late_rejected_host);
    let late_rejected = false;
    try {
      const late_options = { backend: "canvas2d" };
      Object.defineProperty(late_options, "localization", {
        enumerable: true,
        get() { throw new Error("late creation failure"); },
      });
      await create_chart(late_rejected_host, late_options);
    } catch {
      late_rejected = true;
    }
    const late_failed = {
      rejected: late_rejected,
      child_count: late_rejected_host.childElementCount,
      inline_position: late_rejected_host.style.getPropertyValue("position"),
      inline_position_priority: late_rejected_host.style.getPropertyPriority("position"),
      role: late_rejected_host.getAttribute("role"),
      aria_label: late_rejected_host.getAttribute("aria-label"),
    };
    const disposed_host = document.createElement("div");
    disposed_host.style.cssText = "width: 320px; height: 180px";
    document.body.appendChild(disposed_host);
    const disposed_chart = await create_chart(disposed_host, { backend: "canvas2d", autoSize: false });
    disposed_host.setAttribute("role", "complementary");
    disposed_host.setAttribute("aria-label", "User changed chart label");
    disposed_host.style.setProperty("position", "sticky", "important");
    disposed_chart.remove();
    const disposed = {
      child_count: disposed_host.childElementCount,
      inline_position: disposed_host.style.getPropertyValue("position"),
      inline_position_priority: disposed_host.style.getPropertyPriority("position"),
      role: disposed_host.getAttribute("role"),
      aria_label: disposed_host.getAttribute("aria-label"),
    };
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position: static !important; width: 320px; height: 180px";
    restored_host.setAttribute("role", "region");
    restored_host.setAttribute("aria-label", "Existing chart host");
    document.body.appendChild(restored_host);
    const restored_chart = await create_chart(restored_host, { backend: "canvas2d", autoSize: false });
    restored_chart.remove();
    const restored = {
      child_count: restored_host.childElementCount,
      inline_position: restored_host.style.getPropertyValue("position"),
      inline_position_priority: restored_host.style.getPropertyPriority("position"),
      role: restored_host.getAttribute("role"),
      aria_label: restored_host.getAttribute("aria-label"),
    };
    const first_add_after_retirement = async (add) => {
      const branch_host = document.createElement("div");
      branch_host.style.cssText = "width: 320px; height: 180px";
      document.body.appendChild(branch_host);
      const branch_chart = await create_chart(branch_host, {
        backend: "canvas2d",
        autoSize: false,
        initialPane: { horizontal_domain: { type: "category", scale: "band" } },
      });
      branch_chart.remove_pane(0);
      const added = add(branch_chart);
      const ids = branch_chart.panes()[0].get_series().map((series) => series.id);
      branch_chart.remove();
      branch_host.remove();
      return { added: added.id, ids };
    };
    const analogous_first_adds = {
      footprint: await first_add_after_retirement((branch_chart) => branch_chart.addSeries("footprint")),
      feature: await first_add_after_retirement((branch_chart) => branch_chart.addSeries("grouped_bars")),
      custom: await first_add_after_retirement((branch_chart) => branch_chart.add_custom_series({
        price_value_builder: () => [1],
        render() {},
      })),
    };
    host.remove();
    rejected_host.remove();
    late_rejected_host.remove();
    disposed_host.remove();
    restored_host.remove();
    return { initial, failed, late_failed, disposed, restored, analogous_first_adds };
  });

  expect(result).toEqual({
    initial: {
      pane_count: 1,
      series_count: 0,
      aria_label: "General chart",
      domain: { Category: { scale: "Point" } },
      retired: true,
      old_handle_stale: true,
      replacement_schema: 1,
      replacement_is_not_removable: true,
      financial_after_retirement: {
        first_id: 0,
        second_id: 1,
        pane_series: [0, 1],
        series_order: [0, 1],
        first_value: 12,
        second_value: 11,
      },
    },
    failed: {
      rejected: true,
      child_count: 0,
      inline_position: "",
      error_name: "AerisChartsError",
      error_code: "invalid_options",
      role: null,
      aria_label: null,
    },
    late_failed: {
      rejected: true,
      child_count: 0,
      inline_position: "static",
      inline_position_priority: "important",
      role: "region",
      aria_label: "Consumer chart host",
    },
    disposed: {
      child_count: 0,
      inline_position: "sticky",
      inline_position_priority: "important",
      role: "complementary",
      aria_label: "User changed chart label",
    },
    restored: {
      child_count: 0,
      inline_position: "static",
      inline_position_priority: "important",
      role: "region",
      aria_label: "Existing chart host",
    },
    analogous_first_adds: {
      footprint: { added: 0, ids: [0] },
      feature: { added: 0, ids: [0] },
      custom: { added: 0, ids: [0] },
    },
  });
});

test("general row styles survive object updates, typed replacement, and V2 restore", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    Object.assign(host.style, { width: "640px", height: "360px" });
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    try {
      const pane = chart.add_pane({
        preserve_empty: true,
        horizontal_domain: { type: "category", scale: "band" },
      });
      chart.add_axis({ id: "color-x", pane: pane.pane_index(), dimension: "x", scale: "band" });
      chart.add_axis({ id: "color-y", pane: pane.pane_index(), dimension: "y", scale: "linear" });
      const bars = chart.add_series("column", {
        pane: pane.pane_index(), x_axis_id: "color-x", y_axis_id: "color-y",
        bar_corner_radius: 4,
      });
      bars.set_data([
        { id: "a", x: "A", y: 3, color: "#ff0000" },
        { id: "b", x: "B", y: 5, color: "#00ff00" },
      ]);
      const initial = chart.export_state().datasets[0].colors;
      const colored = chart.take_screenshot().toDataURL();
      bars.update_data([{ id: "b", x: "B", y: 6, color: "#0000ff" }]);
      const updated = chart.export_state().datasets[0].colors;
      const restored = chart.export_state();
      bars.set_data_typed({
        categories: ["A", "B"], category_indices: new Uint32Array([0, 1]),
        y: new Float64Array([3, 5]), colors: ["#ff00ff", null],
      });
      const typed = chart.export_state().datasets[0].colors;
      const points_pane = chart.add_pane({
        preserve_empty: true,
        horizontal_domain: { type: "continuous", scale: "linear" },
      });
      chart.add_axis({ id: "point-x", pane: points_pane.pane_index(), dimension: "x", scale: "linear" });
      chart.add_axis({ id: "point-y", pane: points_pane.pane_index(), dimension: "y", scale: "linear" });
      const points = chart.add_series("scatter", {
        pane: points_pane.pane_index(), x_axis_id: "point-x", y_axis_id: "point-y",
      });
      points.set_data_typed({
        x: new Float64Array([1, 2]), y: new Float64Array([2, 3]),
        colors: ["#112233", null], symbols: ["diamond", "square"],
      });
      const numeric_typed = chart.export_state().datasets[1].colors;
      const numeric_symbols = chart.export_state().datasets[1].symbols;
      points.set_data([{ x: 1, y: 2, symbol: "triangle" }, { x: 2, y: 3, symbol: "square" }]);
      const object_symbols = chart.export_state().datasets[1].symbols;
      const restored_host = document.createElement("div");
      Object.assign(restored_host.style, { width: "640px", height: "360px" });
      document.body.appendChild(restored_host);
      const restored_chart = await create_chart(restored_host, { backend: "canvas2d", autoSize: false });
      let round_trip;
      try {
        restored_chart.import_state(restored);
        round_trip = restored_chart.export_state().datasets[0].colors;
      } finally {
        restored_chart.remove();
        restored_host.remove();
      }
      return { initial, updated, round_trip, typed, numeric_typed, numeric_symbols, object_symbols, colored: colored.length > 100 };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  expect(result.initial).toEqual(["#ff0000ff", "#00ff00ff"]);
  expect(result.updated).toEqual(["#ff0000ff", "#0000ffff"]);
  expect(result.round_trip).toEqual(result.updated);
  expect(result.typed).toEqual(["#ff00ffff", null]);
  expect(result.numeric_typed).toEqual(["#112233ff", null]);
  expect(result.numeric_symbols).toEqual(["diamond", "square"]);
  expect(result.object_symbols).toEqual(["triangle", "square"]);
  expect(result.colored).toBe(true);
});

test("general keyboard focus keeps series and explicit row identity across reorder and relayout", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "width: 720px; height: 420px";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d",
      autoSize: false,
      initialPane: { horizontal_domain: { type: "continuous" } },
    });
    const pane = chart.panes()[0];
    chart.addAxis({ id: "x", pane: 0, dimension: "x", scale: "linear" });
    chart.addAxis({ id: "y", pane: 0, dimension: "y", scale: "linear" });
    const add = (title, prefix, offset) => {
      const series = chart.addSeries("scatter", {
        pane: 0,
        x_axis_id: "x",
        y_axis_id: "y",
        title,
      });
      series.setData([
        { id: `${prefix}0`, x: 1, y: offset + 1 },
        { id: `${prefix}1`, x: 2, y: offset + 2 },
        { id: `${prefix}2`, x: 3, y: offset + 3 },
      ]);
      return series;
    };
    const a = add("A", "a", 0);
    const b = add("B", "b", 10);
    const c = add("C", "c", 20);
    const accessibility = chart.accessibility();
    accessibility.refresh();
    accessibility.focus(0);
    for (const key of ["ArrowDown", "ArrowDown", "ArrowRight", "ArrowRight"]) {
      document.activeElement.dispatchEvent(new KeyboardEvent("keydown", {
        key, bubbles: true, cancelable: true,
      }));
    }
    await new Promise((resolve, reject) => {
      let frames = 0;
      const wait_for_focus = () => {
        const hit = chart.general_accessibility_focused_hit();
        const live = host.querySelector(".aeris_charts-a11y-live-region")?.textContent ?? "";
        if (hit?.series === c.id && hit?.row_id === "c1" && live.includes("C")) {
          resolve();
        } else if (++frames >= 120) {
          reject(new Error("general accessibility focus did not settle after keyboard navigation"));
        } else {
          requestAnimationFrame(wait_for_focus);
        }
      };
      wait_for_focus();
    });
    const focused = () => chart.general_accessibility_focused_hit();
    const before = focused();
    const before_live = host.querySelector(".aeris_charts-a11y-live-region")?.textContent ?? "";
    const reordered = chart.set_general_series_order([c, b, a], 0);
    chart.resize(760, 440, 1);
    chart.addAxis({ id: "y2", pane: 0, dimension: "y", scale: "linear" });
    const after = focused();
    const after_live = host.querySelector(".aeris_charts-a11y-live-region")?.textContent ?? "";
    document.activeElement.dispatchEvent(new KeyboardEvent("keydown", {
      key: "ArrowLeft", bubbles: true, cancelable: true,
    }));
    const after_key = focused();
    const focus_chrome = chart.take_screenshot().toDataURL();
    chart.remove();
    host.remove();
    return {
      ids: { a: a.id, b: b.id, c: c.id },
      reordered,
      before,
      after,
      after_key,
      before_live,
      after_live,
      focus_chrome_size: focus_chrome.length,
    };
  });
  expect(result.reordered).toBe(true);
  expect(result.before).toMatchObject({ series: result.ids.c, row_id: "c1" });
  expect(result.after).toEqual(result.before);
  expect(result.after_key).toMatchObject({ series: result.ids.c, row_id: "c0" });
  expect(result.before_live).toContain("C");
  expect(result.after_live).toContain("C");
  expect(result.focus_chrome_size).toBeGreaterThan(100);
});

test("general dashboard showcases every released Cartesian example", async ({ page, browserName }) => {
  const page_errors = [];
  page.on("pageerror", (error) => page_errors.push(error.message));
  const backend_query = browserName === "chromium" ? "" : "&backend=canvas2d";
  await page.goto(`/?theme=dark&demo=general${backend_query}`);
  await page.waitForFunction(() => window.__generalDashboard?.ready === true);
  await expect(page.locator("#general_workspace")).toBeVisible();
  await expect(page.locator("#chart_wrap")).toBeHidden();
  await expect(page.locator("#general_workspace .general-chart-card").first()).toHaveAttribute("data-mounted", "true");

  const result = await page.evaluate(() => ({
    errors: window.__generalDashboard.errors,
    summary: window.__generalDashboard.summary,
    active: window.__generalDashboard.active_summary(),
    cards: document.querySelectorAll("#general_workspace .general-chart-card").length,
    ready: document.getElementById("general_metric_ready")?.textContent,
    runtime: document.getElementById("general_runtime_badge")?.textContent,
    mode: document.getElementById("workspace")?.dataset.demoMode,
  }));

  expect(page_errors).toEqual([]);
  expect(result.errors).toEqual([]);
  expect(result.cards).toBe(14);
  expect(result.mode).toBe("general");
  expect(Number(result.ready)).toBeGreaterThan(0);
  expect(Number(result.ready)).toBeLessThan(14);
  if (browserName === "chromium") {
    expect(result.runtime).toContain("WebGPU");
    expect(result.active.every((entry) => entry.backend === "webgpu")).toBe(true);
    expect(result.active.every((entry) => entry.backend_status.active_backend === "webgpu")).toBe(true);
  } else {
    expect(result.runtime).toContain("Canvas2D");
    expect(result.active.every((entry) => entry.backend === "canvas2d")).toBe(true);
  }
  expect(result.summary).toHaveLength(14);
  expect(result.active.every((entry) => entry.pane_count === 1 && entry.pane_index === 0)).toBe(true);
  expect(result.summary.map((entry) => entry.label)).toEqual([
    "xy_line",
    "xy_area",
    "column",
    "grouped columns",
    "stacked columns",
    "horizontal_bar",
    "scatter",
    "bubble",
    "range_area",
    "range_bar",
    "error_bar",
    "box_plot",
    "heatmap_grid",
    "stacked area",
  ]);
  expect(new Set(result.summary.flatMap((entry) => entry.series_kinds))).toEqual(new Set([
    "xy_line", "xy_area", "column", "horizontal_bar", "scatter", "bubble", "range_area", "range_bar", "error_bar", "box_plot", "heatmap_grid",
  ]));

  const first_host = page.locator("#general_workspace .general-chart-host").first();
  await first_host.hover();
  const scroll_before = await page.locator("#general_workspace").evaluate((element) => element.scrollTop);
  await page.mouse.wheel(0, 420);
  await expect.poll(() => page.locator("#general_workspace").evaluate((element) => element.scrollTop))
    .toBeGreaterThan(scroll_before);

  await page.getByRole("button", { name: "Bars" }).click();
  await expect(page.locator("#general_workspace .general-chart-card:not([hidden])")).toHaveCount(5);
  await page.getByRole("button", { name: "All" }).click();
  await expect(page.locator("#general_workspace .general-chart-card:not([hidden])")).toHaveCount(14);
  const last = page.locator("#general_workspace .general-chart-card").last();
  await last.scrollIntoViewIfNeeded();
  await expect(last).toHaveAttribute("data-mounted", "true");
  await page.waitForTimeout(500);
  expect(await page.locator('#general_workspace .general-chart-card[data-mounted="true"]').count()).toBeLessThanOrEqual(4);

  // Product attribution is not part of the chart surface. The hosts contain canvases/tooltips only;
  // legacy attribution options are retired and cannot inject a mark back into general charts.
  expect(await page.locator("#general_workspace .aeris_charts-attribution-logo").count()).toBe(0);
  expect(await page.locator("#general_workspace .general-chart-host svg").count()).toBe(0);
  expect(await page.locator("#general_workspace .general-chart-host").evaluateAll((hosts) =>
    hosts.every((host) => !/Aeris Terminal|powered by/i.test(host.textContent ?? "")))).toBe(true);

  await page.getByRole("button", { name: "Financial" }).click();
  await expect(page.locator("#chart_wrap")).toBeVisible();
  await expect(page.locator("#general_workspace")).toBeHidden();
});

test("general dashboard keeps rendering when WebGPU has no adapter and does not retry on each card", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "WebGPU adapter interception is Chromium-specific");
  const fallback_warnings = [];
  const page_errors = [];
  page.on("console", (message) => {
    if (message.type() === "warning" && message.text().startsWith("aeris_charts: WebGPU fallback")) {
      fallback_warnings.push(message.text());
    }
  });
  page.on("pageerror", (error) => page_errors.push(error.message));
  await page.addInitScript(() => {
    window.__adapter_requests = 0;
    const gpu = navigator.gpu;
    if (gpu) {
      Object.defineProperty(gpu, "requestAdapter", {
        configurable: true,
        value: async () => {
          window.__adapter_requests += 1;
          return null;
        },
      });
    }
  });
  await page.goto("/?demo=general&theme=dark");
  await page.waitForFunction(() => window.__generalDashboard?.ready === true);
  await expect(page.locator("#general_workspace .general-chart-card").first()).toHaveAttribute("data-mounted", "true");

  const before = await page.evaluate(() => ({
    requests: window.__adapter_requests,
    errors: window.__generalDashboard.errors,
    active: window.__generalDashboard.active_summary(),
    badge: document.getElementById("general_runtime_badge")?.textContent,
  }));
  expect(before.requests).toBe(2); // HighPerformance + default, once for the entire page.
  expect(before.errors).toEqual([]);
  expect(before.active.length).toBeGreaterThan(0);
  expect(before.active.every(({ backend, backend_status }) =>
    backend === "canvas2d" && backend_status.reason === "adapter_unavailable")).toBe(true);
  expect(before.badge).toContain("WebGPU fallback (adapter_unavailable)");

  const last = page.locator("#general_workspace .general-chart-card").last();
  await last.scrollIntoViewIfNeeded();
  await expect(last).toHaveAttribute("data-mounted", "true");
  expect(await page.evaluate(() => window.__adapter_requests)).toBe(before.requests);
  expect(fallback_warnings).toHaveLength(1);
  expect(page_errors).toEqual([]);
});

test("React adapter keeps chart and series identities across rerenders and disposes under StrictMode", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const fixture = await import("/dist/react_phase3_fixture.js");
    return fixture.exerciseReactAdapter();
  });
  expect(result).toEqual({
    same_chart: true,
    same_financial_handle: true,
    same_general_handle: true,
    same_forecast_handle: true,
    same_financial_id: true,
    same_general_id: true,
    updated_general_title: "Updated revenue",
    updated_general_color: "#dc2626",
    updated_general_axis: "revenue-alt",
    updated_general_order: true,
    updated_axis_title: "Updated revenue axis",
    pane_index_stable: true,
    pane_count: 2,
    axis_count: 3,
    general_legend_count: 2,
    child_cleanup: true,
    general_pane_stale: true,
    disposed: true,
  });
});

test("React general-series installation rolls back rejected initial data", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const fixture = await import("/dist/react_phase3_fixture.js");
    return fixture.exerciseReactFailureCleanup();
  });
  expect(result.failure).toContain("category X values must be strings");
  expect(result).toMatchObject({ pane_count: 1, axis_count: 0, legend_count: 0 });
});

test("React readiness callback failure cleans up and permits a valid retry", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const fixture = await import("/dist/react_phase3_fixture.js");
    return fixture.exerciseReactCallbackFailureCleanup();
  });
  expect(result).toEqual({
    failure: "expected readiness callback failure",
    after_failure: { pane_count: 1, axis_count: 0, legend_count: 0 },
    after_retry: { pane_count: 2, axis_count: 3, legend_count: 1 },
    disposed: true,
  });
});

test("camel-case aliases share the original chart and series handles", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { createChart, create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    Object.assign(host.style, { width: "480px", height: "320px" });
    document.body.appendChild(host);
    const chart = await createChart(host, { backend: "canvas2d", autoSize: false });
    try {
      const pane = chart.addPane({ preserve_empty: true, horizontal_domain: { type: "category", scale: "band" } });
      chart.addAxis({ id: "category", pane: pane.pane_index(), dimension: "x", scale: "band" });
      chart.addAxis({ id: "value", pane: pane.pane_index(), dimension: "y", scale: "linear" });
      const series = chart.addSeries("column", { pane: pane.pane_index(), x_axis_id: "category", y_axis_id: "value" });
      series.setData([{ id: "a", x: "A", y: 3 }]);
      series.updateData([{ id: "a", x: "A", y: 7 }]);
      const data = series.data_at(0);
      const alias_data = series.dataAt(0);
      const financial = chart.addSeries("candlestick");
      financial.setData([{ time: 1735689600, open: 1, high: 3, low: 1, close: 2 }]);
      const same_financial_data = financial.data()[0]?.close === 2;
      const state = chart.exportState();
      return {
        exported_create_alias: createChart !== create_chart,
        same_series_data: data?.value === 7 && alias_data?.value === 7,
        same_financial_data,
        state_has_series: state !== null,
        same_scale_handle: chart.timeScale() === chart.time_scale(),
      };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  expect(result).toEqual({
    exported_create_alias: false,
    same_series_data: true,
    same_financial_data: true,
    state_has_series: true,
    same_scale_handle: true,
  });
});

test("public category-column and XY-scatter slices share the chart lifecycle", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    Object.assign(host.style, {
      position: "fixed",
      width: "640px",
      height: "480px",
      left: "0",
      top: "0",
      zIndex: "10000",
    });
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(640, 480, 1);
    const lifecycle = [];
    chart.subscribe_series_added(({ series, pane_index }) => lifecycle.push(["added", series.kind ?? series.series_type(), pane_index]));
    chart.subscribe_series_removed(({ series, pane_index }) => lifecycle.push(["removed", series.kind ?? series.series_type(), pane_index]));

    const category_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "category", scale: "band" },
    });
    chart.add_axis({ id: "month", pane: category_pane.pane_index(), dimension: "x", scale: "band" });
    chart.add_axis({ id: "revenue", pane: category_pane.pane_index(), dimension: "y", scale: "linear" });
    const columns = chart.add_series("column", {
      pane: category_pane.pane_index(),
      x_axis_id: "month",
      y_axis_id: "revenue",
      title: "Revenue",
      color: "#267f99",
      data_labels: true,
    });
    columns.set_data([
      { id: "jan", x: "Jan", y: 42, label: "January" },
      { x: "Feb", y: null },
      { id: "mar", x: "Mar", y: -17 },
    ]);

    const scatter_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" },
    });
    const x_axis = chart.add_axis({ id: "sample-x", pane: scatter_pane.pane_index(), dimension: "x", scale: "linear" });
    const y_axis = chart.add_axis({ id: "sample-y", pane: scatter_pane.pane_index(), dimension: "y", scale: "symlog" });
    const scatter = chart.add_series("scatter", {
      pane: scatter_pane.pane_index(),
      x_axis_id: "sample-x",
      y_axis_id: "sample-y",
      point_radius: 5,
      title: "Samples",
      color: "#7d52f4",
      data_labels: true,
    });
    scatter.set_data_typed({
      ids: [101, 102, 103],
      labels: [null, "Center", null],
      x: new Float64Array([-10, 0, 10]),
      y: new Float64Array([-5, 0, 5]),
    });

    chart.resize(640, 480, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));

    let hit = null;
    let hit_point = null;
    const geometry = scatter_pane.get_geometry();
    for (let y = geometry.top + 6; y <= geometry.top + geometry.height - 6 && hit === null; y += 2) {
      for (let x = 0; x <= geometry.width; x += 2) {
        const candidate = chart.general_hit_test(scatter_pane.pane_index(), x, y);
        if (candidate?.series === scatter.id) {
          hit = candidate;
          hit_point = { x, y };
          break;
        }
      }
    }

    let hover_event = null;
    let click_event = null;
    chart.subscribe_crosshair_move((params) => {
      if (params.general_hit !== null) {
        hover_event = { hit: params.general_hit, kind: params.hovered_series?.kind ?? null };
      }
    });
    chart.subscribe_click((params) => {
      click_event = params.general_hit;
    });
    const overlay = host.querySelectorAll("canvas")[3];
    const browser_x = hit_point.x + geometry.left;
    overlay.dispatchEvent(new PointerEvent("pointermove", {
      clientX: browser_x,
      clientY: hit_point.y,
      pointerId: 1,
      pointerType: "mouse",
      isPrimary: true,
      buttons: 0,
      bubbles: true,
    }));
    for (const [type, buttons] of [["pointerdown", 1], ["pointerup", 0]]) {
      overlay.dispatchEvent(new PointerEvent(type, {
        clientX: browser_x,
        clientY: hit_point.y,
        pointerId: 1,
        pointerType: "mouse",
        isPrimary: true,
        button: 0,
        buttons,
        detail: 1,
        bubbles: true,
      }));
    }
    await new Promise((resolve) => requestAnimationFrame(resolve));

    x_axis.zoom(2, 0);
    x_axis.pan(0.25);
    x_axis.reset_view();
    const blocked_axis_remove = chart.remove_axis("sample-x");
    const before_remove = {
      panes: chart.panes().length,
      axes: chart.axes().map((axis) => axis.id),
      column_row: columns.data_at(0),
      column_missing: columns.data_at(1),
      scatter_row: scatter.data_at(2),
      scatter_accessibility: scatter.accessibility_snapshot(1, 2),
      hit,
      hover_event,
      click_event,
      selected_hit: scatter.selected_hit(),
      chart_selected_hit: chart.general_selected_hit(),
      category_series: category_pane.get_series().map((series) => series.kind ?? series.series_type()),
      scatter_series: scatter_pane.get_series().map((series) => series.kind ?? series.series_type()),
      screenshot: chart.take_screenshot().toDataURL().length,
      blocked_axis_remove,
    };

    const before_custom_label = chart.take_screenshot().toDataURL();
    x_axis.apply_options({ grid_visible: false, zero_line: false });
    y_axis.apply_options({ grid_visible: false, zero_line: false });
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const grid_policy_changed = chart.take_screenshot().toDataURL() !== before_custom_label;
    x_axis.apply_options({ grid_visible: true, zero_line: true });
    y_axis.apply_options({ grid_visible: true, zero_line: true });
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    scatter.update_data_typed({
      ids: [102],
      labels: ["Zero marker"],
      x: new Float64Array([0]),
      y: new Float64Array([0]),
    });
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const custom_label_changed = chart.take_screenshot().toDataURL() !== before_custom_label;
    const updated_custom_label = scatter.data_at(1)?.label;
    let rejected_label = null;
    try {
      scatter.update_data_typed({
        ids: [102],
        labels: ["x".repeat(4097)],
        x: new Float64Array([0]),
        y: new Float64Array([99]),
      });
    } catch (error) {
      rejected_label = error.code;
    }
    const after_rejected_label = scatter.data_at(1);

    let rejected_update = null;
    try {
      scatter.update_data([
        { id: 105, x: 12, y: 7 },
        { id: 105, x: 13, y: 8 },
      ]);
    } catch (error) {
      rejected_update = error.code;
    }
    const after_rejected_update = scatter.accessibility_snapshot(0, 10);
    scatter.update_data_typed({
      ids: [102, 104],
      x: new Float64Array([0, 11]),
      y: new Float64Array([7, 6]),
    }, { max_rows: 3 });
    columns.update_data([
      { id: "jan", x: "Jan", y: 43 },
      { id: "apr", x: "Apr", y: 9, label: "April" },
    ], { max_rows: 3 });
    const after_update = {
      scatter: scatter.accessibility_snapshot(0, 10),
      columns: columns.accessibility_snapshot(0, 10),
      selected: scatter.selected_hit(),
    };

    scatter.remove();
    const removed_axis = x_axis.remove();
    const replacement_x_axis = chart.add_axis({
      id: "sample-x",
      pane: scatter_pane.pane_index(),
      dimension: "x",
      scale: "linear",
      title: "Replacement X",
    });
    let stale_axis_code = null;
    try { x_axis.options(); } catch (error) { stale_axis_code = error.code; }
    const replacement_axis_title = replacement_x_axis.options().title;
    replacement_x_axis.remove();
    chart.remove_axis("sample-y");
    chart.remove_pane(scatter_pane.pane_index());
    columns.remove();
    chart.remove_axis("month");
    chart.remove_axis("revenue");
    chart.remove_pane(category_pane.pane_index());
    const remaining_panes = chart.panes().length;
    chart.remove();
    host.remove();
    return {
      before_remove,
      grid_policy_changed,
      custom_label_changed,
      updated_custom_label,
      rejected_label,
      after_rejected_label,
      rejected_update,
      after_rejected_update,
      after_update,
      removed_axis,
      stale_axis_code,
      replacement_axis_title,
      remaining_panes,
      lifecycle,
    };
  });

  expect(result.before_remove.panes).toBe(3);
  expect(result.before_remove.axes).toEqual(["month", "revenue", "sample-x", "sample-y"]);
  expect(result.before_remove.column_row).toMatchObject({ row_id: "jan", label: "January", value: 42 });
  expect(result.before_remove.column_missing).toMatchObject({ row_id: { generated: "1" }, x_label: "Feb", value: null });
  expect(result.before_remove.scatter_row).toMatchObject({ row_id: 103, x_label: "10", value: 5 });
  expect(result.before_remove.scatter_accessibility).toMatchObject({
    total_rows: 3,
    offset: 1,
    items: [
      { row: 1, row_id: 102, x_label: "0", label: "Center", value: 0 },
      { row: 2, row_id: 103, x_label: "10", value: 5 },
    ],
  });
  expect(result.before_remove.hit).toMatchObject({ series: expect.any(Number), row_id: expect.any(Number) });
  expect(result.before_remove.hover_event).toMatchObject({
    hit: { series: expect.any(Number), row_id: expect.any(Number) },
    kind: "scatter",
  });
  expect(result.before_remove.click_event).toMatchObject({ series: expect.any(Number), row_id: expect.any(Number) });
  expect(result.before_remove.selected_hit).toEqual(result.before_remove.chart_selected_hit);
  expect(result.before_remove.selected_hit).toMatchObject({ series: expect.any(Number), row_id: expect.any(Number) });
  expect(result.before_remove.category_series).toEqual(["column"]);
  expect(result.before_remove.scatter_series).toEqual(["scatter"]);
  expect(result.before_remove.screenshot).toBeGreaterThan(1000);
  expect(result.before_remove.blocked_axis_remove).toBe(false);
  expect(result.grid_policy_changed).toBe(true);
  expect(result.custom_label_changed).toBe(true);
  expect(result.updated_custom_label).toBe("Zero marker");
  expect(result.rejected_label).toBe("resource_limit");
  expect(result.after_rejected_label).toMatchObject({ row_id: 102, value: 0, label: "Zero marker" });
  expect(result.rejected_update).toBe("invalid_data");
  expect(result.after_rejected_update.total_rows).toBe(3);
  expect(result.after_rejected_update.items.map((item) => item.row_id)).toEqual([101, 102, 103]);
  expect(result.after_update.scatter.items).toMatchObject([
    { row_id: 102, x_label: "0", label: null, value: 7 },
    { row_id: 103, x_label: "10", value: 5 },
    { row_id: 104, x_label: "11", value: 6 },
  ]);
  if (result.before_remove.selected_hit.row_id === 101) {
    expect(result.after_update.selected).toBeNull();
  } else {
    expect(result.after_update.selected).toMatchObject({
      row_id: result.before_remove.selected_hit.row_id,
      row: result.before_remove.selected_hit.row - 1,
    });
  }
  expect(result.after_update.columns.items).toMatchObject([
    { x_label: "Feb", value: null },
    { row_id: "mar", x_label: "Mar", value: -17 },
    { row_id: "apr", x_label: "Apr", label: "April", value: 9 },
  ]);
  expect(result.removed_axis).toBe(true);
  expect(result.stale_axis_code).toBe("stale_handle");
  expect(result.replacement_axis_title).toBe("Replacement X");
  expect(result.remaining_panes).toBe(1);
  expect(result.lifecycle).toEqual([
    ["added", "column", 1],
    ["added", "scatter", 2],
    ["removed", "scatter", 2],
    ["removed", "column", 1],
  ]);
});

test("general path styles, interpolation, missing connections, and point markers round-trip", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    Object.assign(host.style, { width: "640px", height: "360px" });
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    try {
      const pane = chart.add_pane({
        preserve_empty: true,
        horizontal_domain: { type: "continuous", scale: "linear" },
      });
      chart.add_axis({ id: "style-x", pane: pane.pane_index(), dimension: "x", scale: "linear" });
      chart.add_axis({ id: "style-y", pane: pane.pane_index(), dimension: "y", scale: "linear" });
      const series = chart.add_series("xy_line", {
        pane: pane.pane_index(), x_axis_id: "style-x", y_axis_id: "style-y",
        line_width: 2, line_style: "solid",
      });
      series.set_data([{ x: 0, y: 1 }, { x: 1, y: 3 }, { x: 2, y: 2 }]);
      series.apply_options({
        line_width: 4, line_style: "dashed", interpolation: "curved",
        connect_missing: true, point_markers: true, point_symbol: "diamond", point_radius: 7,
      });
      const steps = [];
      for (const interpolation of ["step_before", "step_middle", "step_after", "step", "monotone", "natural"]) {
        series.apply_options({ interpolation });
        steps.push(series.options().interpolation);
      }
      series.apply_options({ interpolation: "curved" });
      return { options: series.options(), steps };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  expect(result.options.line_width).toBe(4);
  expect(result.options.line_style).toBe("dashed");
  expect(result.options.interpolation).toBe("curved");
  expect(result.options.connect_missing).toBe(true);
  expect(result.options.point_markers).toBe(true);
  expect(result.options.point_symbol).toBe("diamond");
  expect(result.options.point_radius).toBe(7);
  expect(result.steps).toEqual(["step_before", "step_middle", "step_after", "step", "monotone", "natural"]);
});

test("public path marker toggle keeps chosen radius, rows, series, and view", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    Object.assign(host.style, { width: "640px", height: "360px" });
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    try {
      const pane = chart.add_pane({
        preserve_empty: true, horizontal_domain: { type: "continuous", scale: "linear" },
      });
      chart.add_axis({ id: "toggle-x", pane: pane.pane_index(), dimension: "x", scale: "linear" });
      chart.add_axis({ id: "toggle-y", pane: pane.pane_index(), dimension: "y", scale: "linear" });
      const series = chart.add_series("xy_line", {
        pane: pane.pane_index(), x_axis_id: "toggle-x", y_axis_id: "toggle-y",
      });
      series.set_data([{ id: "a", x: 0, y: 2 }, { id: "b", x: 1, y: 4 }]);
      series.apply_options({ point_markers: true, point_radius: 7 });
      const before = chart.export_state();
      series.apply_options({ point_markers: false });
      const dormant = chart.export_state();
      const disabled = series.options();
      series.apply_options({ point_markers: true });
      const restored = chart.export_state();
      let invalid;
      try {
        series.apply_options({ point_radius: 1000 });
      } catch (error) {
        invalid = error.code;
      }
      return {
        same_series: before.series[0].dataset === dormant.series[0].dataset
          && before.series[0].dataset === restored.series[0].dataset,
        same_data: JSON.stringify(before.datasets) === JSON.stringify(dormant.datasets),
        same_view: JSON.stringify(before.axes) === JSON.stringify(dormant.axes),
        restored: JSON.stringify(restored) === JSON.stringify(before),
        atomic: JSON.stringify(chart.export_state()) === JSON.stringify(before),
        disabled, invalid, id: series.id,
      };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  expect(result.disabled).toMatchObject({ point_markers: false, point_radius: 7 });
  expect(result.same_series).toBe(true);
  expect(result.same_data).toBe(true);
  expect(result.same_view).toBe(true);
  expect(result.restored).toBe(true);
  expect(result.atomic).toBe(true);
  expect(result.invalid).toBe("invalid_options");
});

test("public V2 restore accepts older dormant radius, width, and style combinations", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const hosts = [];
    const charts = [];
    const create = async () => {
      const host = document.createElement("div");
      Object.assign(host.style, { width: "640px", height: "360px" });
      document.body.appendChild(host);
      hosts.push(host);
      const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
      charts.push(chart);
      return chart;
    };
    try {
      const source = await create();
      const pane = source.add_pane({
        preserve_empty: true, horizontal_domain: { type: "category", scale: "band" },
      });
      source.add_axis({ id: "old-x", pane: pane.pane_index(), dimension: "x", scale: "band" });
      source.add_axis({ id: "old-y", pane: pane.pane_index(), dimension: "y", scale: "linear" });
      const line = source.add_series("xy_line", {
        pane: pane.pane_index(), x_axis_id: "old-x", y_axis_id: "old-y",
      });
      line.set_data([{ id: "a", x: "A", y: 2 }]);
      const column = source.add_series("column", {
        pane: pane.pane_index(), x_axis_id: "old-x", y_axis_id: "old-y",
      });
      column.set_data([{ id: "b", x: "A", y: 4 }]);
      const baseline = source.export_state();
      const values = [];
      for (const [index, field, value] of [
        [0, "point_radius", 7], [1, "line_width", 5], [1, "line_style", "Dashed"],
      ]) {
        const old = structuredClone(baseline);
        old.series[index][field] = value;
        for (const series of old.series) {
          for (const newer of [
            "fill_gradient", "baseline_policy", "bar_gap", "bar_max_width",
            "bar_corner_radius", "heatmap_value_domain", "heatmap_low_color",
            "heatmap_high_color", "box_fill_color", "box_median_color",
          ]) delete series[newer];
        }
        const target = await create();
        target.import_state(old);
        const saved = target.export_state();
        values.push([saved.series[index][field], saved.datasets.length,
          saved.series.length, saved.schema_version]);
      }
      return values;
    } finally {
      for (const chart of charts) chart.remove();
      for (const host of hosts) host.remove();
    }
  });
  expect(result).toEqual([[7, 2, 2, 2], [5, 2, 2, 2], ["Dashed", 2, 2, 2]]);
});

test("general area baselines and fill opacity round-trip through the public browser API", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    Object.assign(host.style, { width: "640px", height: "360px" });
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    try {
      const pane = chart.add_pane({
        preserve_empty: true,
        horizontal_domain: { type: "continuous", scale: "linear" },
      });
      chart.add_axis({ id: "baseline-x", pane: pane.pane_index(), dimension: "x", scale: "linear" });
      chart.add_axis({ id: "baseline-y", pane: pane.pane_index(), dimension: "y", scale: "linear" });
      const series = chart.add_series("xy_area", {
        pane: pane.pane_index(), x_axis_id: "baseline-x", y_axis_id: "baseline-y",
        baseline_value: 1.5, fill_opacity: 0.5,
      });
      series.set_data([{ x: 0, y: 1 }, { x: 1, y: 3 }, { x: 2, y: 2 }]);
      const domain_area = chart.add_series("xy_area", {
        pane: pane.pane_index(), x_axis_id: "baseline-x", y_axis_id: "baseline-y",
        baseline_policy: "domain_min",
      });
      domain_area.set_data([{ x: 0, y: 2 }, { x: 1, y: 4 }]);
      let invalid_baseline = null;
      try {
        chart.add_series("xy_area", {
          pane: pane.pane_index(), x_axis_id: "baseline-x", y_axis_id: "baseline-y",
          baseline_value: 1.5, baseline_policy: "domain_max",
        });
      } catch (error) {
        invalid_baseline = error.code;
      }
      const state = chart.export_state();
      const restored_host = document.createElement("div");
      document.body.appendChild(restored_host);
      const restored = await create_chart(restored_host, { backend: "canvas2d", autoSize: false });
      restored.import_state(state);
      const restored_policy = restored.panes().flatMap((entry) => entry.get_series())
        .filter((item) => item.kind === "xy_area")
        .map((item) => item.options().baseline_policy);
      restored.remove();
      restored_host.remove();
      return {
        value: series.options().baseline_value,
        opacity: series.options().fill_opacity,
        policy: domain_area.options().baseline_policy,
        invalid_baseline,
        persisted_policy: state.series.filter((item) => item.kind === "XyArea")
          .map((item) => item.baseline_policy),
        restored_policy,
      };
    } finally {
      chart.remove();
      host.remove();
    }
  });
  expect(result.value).toBe(1.5);
  expect(result.opacity).toBe(0.5);
  expect(result.policy).toBe("domain_min");
  expect(result.invalid_baseline).toBe("invalid_options");
  expect(result.persisted_policy).toEqual(["Zero", "DomainMin"]);
  expect(result.restored_policy).toEqual(["zero", "domain_min"]);
});

test("public linear axes render and navigate the complete finite numeric domain", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const page_errors = [];
  page.on("pageerror", (error) => page_errors.push(error.message));

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    Object.assign(host.style, { width: "640px", height: "400px" });
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(640, 400, 1);
    const pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" },
    });
    const x_axis = chart.add_axis({
      id: "extreme-x",
      pane: pane.pane_index(),
      dimension: "x",
      scale: "linear",
      domain: [-Number.MAX_VALUE, Number.MAX_VALUE],
    });
    chart.add_axis({
      id: "extreme-y",
      pane: pane.pane_index(),
      dimension: "y",
      scale: "linear",
      domain: [-1, 1],
    });
    const scatter = chart.add_series("scatter", {
      pane: pane.pane_index(),
      x_axis_id: "extreme-x",
      y_axis_id: "extreme-y",
      point_radius: 6,
    });
    scatter.set_data_typed({
      ids: [1, 2, 3],
      x: new Float64Array([-Number.MAX_VALUE, 0, Number.MAX_VALUE]),
      y: new Float64Array([-0.5, 0, 0.5]),
    });
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));

    const before = chart.take_screenshot().toDataURL();
    const geometry = pane.get_geometry();
    const hit_rows = new Set();
    for (let y = geometry.top; y <= geometry.top + geometry.height; y += 2) {
      for (let x = 0; x <= geometry.width; x += 2) {
        const hit = chart.general_hit_test(pane.pane_index(), x, y);
        if (hit?.series === scatter.id) hit_rows.add(hit.row_id);
      }
    }

    x_axis.zoom(2, 0);
    x_axis.pan(0.5);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const after = chart.take_screenshot().toDataURL();
    const accessibility = scatter.accessibility_snapshot(0, 10);
    chart.remove();
    host.remove();
    return {
      before_length: before.length,
      changed_after_navigation: before !== after,
      hit_rows: [...hit_rows].sort(),
      values: accessibility.items.map(({ x_label, value }) => ({ x: Number(x_label), value })),
    };
  });

  expect(page_errors).toEqual([]);
  expect(result.before_length).toBeGreaterThan(1000);
  expect(result.changed_after_navigation).toBe(true);
  expect(result.hit_rows).toEqual([1, 2, 3]);
  expect(result.values).toEqual([
    { x: -Number.MAX_VALUE, value: -0.5 },
    { x: 0, value: 0 },
    { x: Number.MAX_VALUE, value: 0.5 },
  ]);
});

test("public general axes retain typed explicit ticks and formatted labels", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    Object.assign(host.style, { width: "640px", height: "420px" });
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(640, 420, 1);
    const pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" },
    });
    const x_axis = chart.add_axis({
      id: "explicit-x",
      pane: pane.pane_index(),
      dimension: "x",
      scale: "linear",
      domain: [0, 10],
      domain_padding: 0.1,
      ticks: [
        { type: "numeric", value: 0, label: "Floor" },
        { type: "numeric", value: 5 },
        { type: "numeric", value: 20, label: "Clipped" },
      ],
    });
    chart.add_axis({
      id: "explicit-y",
      pane: pane.pane_index(),
      dimension: "y",
      scale: "linear",
      domain: [0, 10],
    });
    const scatter = chart.add_series("scatter", {
      pane: pane.pane_index(), x_axis_id: "explicit-x", y_axis_id: "explicit-y",
    });
    scatter.set_data([{ x: 0, y: 1 }, { x: 5, y: 5 }, { x: 10, y: 9 }]);

    const temporal_pane = chart.add_pane({ preserve_empty: true, horizontal_domain: { type: "temporal" } });
    const jan1 = new Date("2026-01-01T00:00:00.000Z");
    const jan2 = new Date("2026-01-02T00:00:00.000Z");
    const temporal_axis = chart.add_axis({
      id: "explicit-time",
      pane: temporal_pane.pane_index(),
      dimension: "x",
      scale: "temporal",
      domain: [jan1, jan2],
      ticks: [{ type: "temporal", value: jan1, label: "Open" }],
    });

    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const before = chart.take_screenshot().toDataURL();
    const initial_token = x_axis.options();
    x_axis.apply_options({
      ticks: [
        { type: "numeric", value: 0, label: "Baseline" },
        { type: "numeric", value: 10, label: "Ceiling" },
      ],
    });
    await new Promise((resolve) => requestAnimationFrame(resolve));
    const changed = before !== chart.take_screenshot().toDataURL();
    let rejected = null;
    try {
      x_axis.apply_options({ tick_count: 2 });
    } catch (error) {
      rejected = error.code;
    }
    let rejected_temporal_padding = null;
    try {
      temporal_axis.apply_options({ domain_padding: 0.1 });
    } catch (error) {
      rejected_temporal_padding = error.code;
    }
    const state = chart.export_state();
    const output = {
      initial_ticks: initial_token.ticks,
      initial_padding: initial_token.domain_padding,
      updated_ticks: x_axis.options().ticks,
      updated_padding: x_axis.options().domain_padding,
      temporal_ticks: temporal_axis.options().ticks,
      temporal_padding: temporal_axis.options().domain_padding,
      changed,
      rejected,
      rejected_temporal_padding,
      persisted_ticks: state.axes.find((axis) => axis.id === "explicit-x").ticks,
      persisted_padding: state.axes.find((axis) => axis.id === "explicit-x").domain_padding,
    };
    chart.remove();
    host.remove();
    return output;
  });

  expect(result).toEqual({
    initial_ticks: [
      { type: "numeric", value: 0, label: "Floor" },
      { type: "numeric", value: 5 },
      { type: "numeric", value: 20, label: "Clipped" },
    ],
    initial_padding: 0.1,
    updated_ticks: [
      { type: "numeric", value: 0, label: "Baseline" },
      { type: "numeric", value: 10, label: "Ceiling" },
    ],
    updated_padding: 0.1,
    temporal_ticks: [{ type: "temporal", value: 1767225600000, label: "Open" }],
    temporal_padding: 0,
    changed: true,
    rejected: "invalid_options",
    rejected_temporal_padding: "invalid_options",
    persisted_ticks: [
      { type: "numeric", value: 0, label: "Baseline" },
      { type: "numeric", value: 10, label: "Ceiling" },
    ],
    persisted_padding: 0.1,
  });
});

test("public category axes zoom by identity, pan by visible window, and reset", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    Object.assign(host.style, { width: "640px", height: "420px" });
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(640, 420, 1);
    const pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "category", scale: "band" },
    });
    const axis = chart.add_axis({
      id: "category-nav-x",
      pane: pane.pane_index(),
      dimension: "x",
      scale: "band",
      domain: ["A", "B", "C", "D", "E", "F"],
      ticks: ["A", "B", "C", "D", "E", "F"].map((value) => ({
        type: "category", value, label: value <= "C" ? "First half" : "Second half",
      })),
    });
    chart.add_axis({
      id: "category-nav-y", pane: pane.pane_index(), dimension: "y", scale: "linear", domain: [0, 10],
    });
    const columns = chart.add_series("column", {
      pane: pane.pane_index(), x_axis_id: "category-nav-x", y_axis_id: "category-nav-y",
    });
    columns.set_data(["A", "B", "C", "D", "E", "F"].map((x, index) => ({ x, y: index + 1 })));
    const settle = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));

    await settle();
    const before = chart.take_screenshot().toDataURL();
    axis.zoom(2, "C");
    await settle();
    const zoomed = chart.take_screenshot().toDataURL();
    axis.pan(0.34);
    await settle();
    const panned = chart.take_screenshot().toDataURL();
    let rejected = null;
    try {
      axis.zoom(2, "A");
    } catch (error) {
      rejected = error.code;
    }
    await settle();
    const after_rejection = chart.take_screenshot().toDataURL();
    axis.resetView();
    await settle();
    const reset = chart.take_screenshot().toDataURL();
    const accessibility = chart.accessibility();
    accessibility.refresh();
    accessibility.focus(pane.pane_index());
    document.activeElement.dispatchEvent(new KeyboardEvent("keydown", {
      key: "ArrowRight", bubbles: true, cancelable: true,
    }));
    await settle();
    const keyboard_before = chart.take_screenshot().toDataURL();
    document.activeElement.dispatchEvent(new KeyboardEvent("keydown", {
      key: "=", bubbles: true, cancelable: true,
    }));
    await settle();
    const keyboard_after = chart.take_screenshot().toDataURL();
    const duplicate_display_ticks = axis.options().ticks;
    chart.remove();
    host.remove();
    return {
      zoom_changed: zoomed !== before,
      pan_changed: panned !== zoomed,
      rejection_preserved_view: after_rejection === panned,
      reset_restored_frame: reset === before,
      keyboard_zoom_changed: keyboard_after !== keyboard_before,
      rejected,
      duplicate_display_ticks,
    };
  });

  expect(result).toEqual({
    zoom_changed: true,
    pan_changed: true,
    rejection_preserved_view: true,
    reset_restored_frame: true,
    keyboard_zoom_changed: true,
    rejected: "invalid_options",
    duplicate_display_ticks: [
      { type: "category", value: "A", label: "First half" },
      { type: "category", value: "B", label: "First half" },
      { type: "category", value: "C", label: "First half" },
      { type: "category", value: "D", label: "Second half" },
      { type: "category", value: "E", label: "Second half" },
      { type: "category", value: "F", label: "Second half" },
    ],
  });
});

test("multiple reversed axes stay aligned through aspect resize, DPR, and font changes", async ({ page }) => {
  const page_errors = [];
  page.on("pageerror", (error) => page_errors.push(error.message));
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:480px;aspect-ratio:2/1;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d",
      autoSize: true,
      initialPane: { horizontal_domain: { type: "continuous", scale: "linear" } },
    });
    const pane = chart.panes()[0];
    const axes = [
      chart.add_axis({ id: "bottom-x", pane: 0, dimension: "x", position: "bottom", scale: "linear", domain: [0, 10], domain_padding: 0.1, title: "Bottom X" }),
      chart.add_axis({ id: "top-x", pane: 0, dimension: "x", position: "top", scale: "linear", domain: [0, 100], reverse: true, title: "Top X" }),
      chart.add_axis({ id: "left-y", pane: 0, dimension: "y", position: "left", scale: "linear", domain: [0, 10], title: "Left Y" }),
      chart.add_axis({ id: "right-y", pane: 0, dimension: "y", position: "right", scale: "linear", domain: [0, 100], reverse: true, title: "Right Y" }),
    ];
    const left = chart.add_series("scatter", {
      pane: 0, x_axis_id: "bottom-x", y_axis_id: "left-y", title: "Left",
    });
    left.set_data([{ id: "left-row", x: 2, y: 2 }]);
    const right = chart.add_series("scatter", {
      pane: 0, x_axis_id: "top-x", y_axis_id: "right-y", title: "Right",
    });
    right.set_data([{ id: "right-row", x: 20, y: 20 }]);

    const wait_for_size = (width, height) => new Promise((resolve, reject) => {
      let frames = 0;
      const poll = () => {
        const canvas = host.querySelector("canvas");
        const size = canvas === null ? [0, 0] : [
          Number.parseFloat(canvas.style.width),
          Number.parseFloat(canvas.style.height),
        ];
        if (size[0] === width && size[1] === height) resolve(size);
        else if (++frames >= 120) reject(new Error(`chart did not resize to ${width}x${height}`));
        else requestAnimationFrame(poll);
      };
      poll();
    });
    const find_hit = (series) => {
      const geometry = pane.get_geometry();
      for (let y = geometry.top + 2; y < geometry.top + geometry.height - 2; y += 2) {
        for (let x = 0; x < geometry.width; x += 2) {
          const hit = chart.general_hit_test(0, x, y);
          if (hit?.series === series.id) return { hit, x, y, geometry };
        }
      }
      return null;
    };

    await wait_for_size(480, 240);
    const initial = {
      left: find_hit(left)?.hit ?? null,
      right: find_hit(right)?.hit ?? null,
      screenshot: chart.take_screenshot().toDataURL(),
    };
    host.style.width = "220px";
    await wait_for_size(220, 110);
    chart.apply_options({ layout: { fontSize: 16, fontFamily: "serif" } });
    Object.defineProperty(window, "devicePixelRatio", { configurable: true, value: 1.5 });
    window.dispatchEvent(new Event("orientationchange"));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const resized_hit = find_hit(right);
    let pointer_hit = null;
    chart.subscribe_crosshair_move((params) => {
      if (params.general_hit !== null) pointer_hit = params.general_hit;
    });
    const overlay = host.querySelectorAll("canvas")[3];
    overlay.dispatchEvent(new PointerEvent("pointermove", {
      clientX: resized_hit.geometry.left + resized_hit.x,
      clientY: resized_hit.y,
      pointerId: 1,
      pointerType: "mouse",
      isPrimary: true,
      buttons: 0,
      bubbles: true,
    }));
    await new Promise((resolve) => requestAnimationFrame(resolve));
    const canvas = host.querySelector("canvas");
    const resized = {
      hit: resized_hit.hit,
      pointer_hit,
      css: [Number.parseFloat(canvas.style.width), Number.parseFloat(canvas.style.height)],
      bitmap: [canvas.width, canvas.height],
      screenshot_changed: chart.take_screenshot().toDataURL() !== initial.screenshot,
    };
    axes[3].set_visible(false);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const hidden_axis = {
      visible: axes[3].options().visible,
      right_binding: right.options().y_axis_id,
      hit: find_hit(right)?.hit ?? null,
    };
    const axis_options = axes.map((axis) => axis.options());
    chart.remove();
    host.remove();
    return { initial, resized, hidden_axis, axis_options };
  });

  expect(page_errors).toEqual([]);
  expect(result.initial.left).toMatchObject({ row_id: "left-row" });
  expect(result.initial.right).toMatchObject({ row_id: "right-row" });
  expect(result.resized.hit).toMatchObject({ row_id: "right-row" });
  expect(result.resized.pointer_hit).toMatchObject({ row_id: "right-row" });
  expect(result.resized.css).toEqual([220, 110]);
  expect(result.resized.bitmap).toEqual([330, 165]);
  expect(result.resized.screenshot_changed).toBe(true);
  expect(result.hidden_axis).toMatchObject({
    visible: false,
    right_binding: "right-y",
    hit: { row_id: "right-row" },
  });
  expect(result.axis_options.map(({ position, title, reverse }) => ({ position, title, reverse }))).toEqual([
    { position: "bottom", title: "Bottom X", reverse: false },
    { position: "top", title: "Top X", reverse: true },
    { position: "left", title: "Left Y", reverse: false },
    { position: "right", title: "Right Y", reverse: true },
  ]);
});

test("general auto sizing disables cleanly and survives hidden-container reveal", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    Object.assign(host.style, { width: "320px", height: "180px" });
    document.body.appendChild(host);
    const chart = await create_chart(host, {
      backend: "canvas2d",
      autoSize: true,
      initialPane: { horizontal_domain: { type: "continuous", scale: "linear" } },
    });
    const settle = () => new Promise((resolve) => {
      requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(resolve, 20)));
    });
    const size = () => {
      const canvas = host.querySelector("canvas");
      return [Number.parseFloat(canvas.style.width), Number.parseFloat(canvas.style.height)];
    };

    await settle();
    const initial = size();
    chart.resize(222, 111, 1);
    const ignored_manual_resize = size();

    chart.apply_options({ autoSize: false });
    chart.resize(222, 111, 1);
    host.style.width = "500px";
    host.style.height = "300px";
    await settle();
    const disabled = { active: chart.auto_size_active(), size: size() };

    host.style.display = "none";
    chart.apply_options({ autoSize: true });
    await settle();
    const reenabled_hidden = { active: chart.auto_size_active(), size: size() };

    host.style.display = "block";
    await settle();
    const first_reveal = size();
    host.style.display = "none";
    window.dispatchEvent(new Event("orientationchange"));
    await settle();
    const hidden_again = size();
    host.style.display = "block";
    host.style.width = "280px";
    host.style.height = "160px";
    await settle();
    const revealed = size();

    chart.remove();
    host.remove();
    return { initial, ignored_manual_resize, disabled, reenabled_hidden, first_reveal, hidden_again, revealed };
  });

  expect(result).toEqual({
    initial: [320, 180],
    ignored_manual_resize: [320, 180],
    disabled: { active: false, size: [222, 111] },
    reenabled_hidden: { active: true, size: [222, 111] },
    first_reveal: [500, 300],
    hidden_again: [500, 300],
    revealed: [280, 160],
  });
});

test("general series rebind and reorder atomically while preserving legend, data, and V2 restore", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:720px;height:520px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(720, 520, 1);

    const first_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" },
    });
    const x_axis = chart.add_axis({ id: "legend-a-x", pane: first_pane.pane_index(), dimension: "x", scale: "linear" });
    chart.add_axis({ id: "legend-a-y", pane: first_pane.pane_index(), dimension: "y", scale: "linear" });
    x_axis.applyOptions({ title: "Updated X", reverse: true });
    let rejected_axis_update = null;
    try {
      x_axis.apply_options({ tick_count: 0 });
    } catch (error) {
      rejected_axis_update = error.code;
    }
    const x_axis_options = x_axis.options();
    x_axis.set_visible(false);
    const hidden_axis_visible = x_axis.options().visible;
    x_axis.setVisible(true);
    const revenue = chart.add_series("xy_line", {
      pane: first_pane.pane_index(),
      x_axis_id: "legend-a-x",
      y_axis_id: "legend-a-y",
      title: "Revenue",
      color: "#123456",
    });
    revenue.set_data([{ id: "r", x: 1, y: 2 }]);
    const revenue_id = revenue.id;
    revenue.applyOptions({ title: "Updated revenue", color: "#654321" });
    let rejected_update = null;
    try {
      revenue.apply_options({ color: "not-a-color" });
    } catch (error) {
      rejected_update = error.code;
    }
    const revenue_options = revenue.options();
    const hidden = chart.add_series("scatter", {
      pane: first_pane.pane_index(),
      x_axis_id: "legend-a-x",
      y_axis_id: "legend-a-y",
      title: "Hidden samples",
    });
    hidden.set_data([{ id: "h", x: 1, y: 3 }]);
    hidden.set_visible(false);

    const second_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" },
    });
    chart.add_axis({ id: "legend-b-x", pane: second_pane.pane_index(), dimension: "x", scale: "linear" });
    chart.add_axis({ id: "legend-b-y", pane: second_pane.pane_index(), dimension: "y", scale: "linear" });
    const margin = chart.add_series("xy_area", {
      pane: second_pane.pane_index(),
      x_axis_id: "legend-b-x",
      y_axis_id: "legend-b-y",
      title: "Margin",
      color: "#abcdef",
    });
    margin.set_data([{ id: "m", x: 1, y: 4 }]);

    const initial_order = chart.general_series_order().map((series) => series.id);
    const reordered_first = chart.set_general_series_order(
      [hidden, revenue],
      first_pane.pane_index(),
    );
    const rejected_order = chart.set_general_series_order([revenue], first_pane.pane_index());
    revenue.apply_options({
      pane: second_pane.pane_index(),
      x_axis_id: "legend-b-x",
      y_axis_id: "legend-b-y",
    });
    revenue.update_data([{ id: "r", x: 1, y: 5 }]);
    const reordered_second = chart.set_general_series_order(
      [margin, revenue],
      second_pane.pane_index(),
    );
    let rejected_rebind = null;
    try {
      revenue.apply_options({ x_axis_id: "missing-axis" });
    } catch (error) {
      rejected_rebind = error.code;
    }
    const final_order = chart.general_series_order().map((series) => series.id);
    const pane_order = chart
      .general_series_order(second_pane.pane_index())
      .map((series) => series.id);

    let invalid_pane = null;
    try {
      chart.general_legend_snapshot(-1);
    } catch (error) {
      invalid_pane = error.code;
    }
    const all = chart.general_legend_snapshot();
    const first_only = chart.general_legend_snapshot(first_pane.pane_index());

    const state = chart.export_state();
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:fixed;left:-10000px;top:0;width:720px;height:520px";
    document.body.appendChild(restored_host);
    const restored = await create_chart(restored_host, { backend: "canvas2d", autoSize: false });
    restored.resize(720, 520, 1);
    const restore = restored.import_state(state);
    const restored_legend = restored.general_legend_snapshot();

    hidden.remove();
    const after_remove = chart.general_legend_snapshot();
    const normalize = (snapshot) => snapshot.items.map(({ series, ...item }) => item);
    const output = {
      invalid_pane,
      hidden_axis_visible,
      rejected_axis_update,
      x_axis_options,
      rejected_update,
      rejected_rebind,
      reordered_first,
      reordered_second,
      rejected_order,
      initial_order,
      final_order,
      pane_order,
      series_identity_preserved: revenue.id === revenue_id,
      revenue_data: revenue.data_at(0),
      revenue_options,
      all,
      first_only,
      after_remove,
      restored: normalize(restored_legend),
      before_restore: normalize(all),
      restore_version: restore.schema_version,
    };
    restored.remove();
    restored_host.remove();
    chart.remove();
    host.remove();
    return output;
  });

  expect(result.invalid_pane).toBe("invalid_options");
  expect(result.hidden_axis_visible).toBe(false);
  expect(result.rejected_axis_update).toBe("invalid_options");
  expect(result.x_axis_options).toMatchObject({ title: "Updated X", reverse: true, tick_count: null });
  expect(result.rejected_update).toBe("invalid_options");
  expect(result.rejected_rebind).toBe("invalid_options");
  expect(result.reordered_first).toBe(true);
  expect(result.reordered_second).toBe(true);
  expect(result.rejected_order).toBe(false);
  expect(result.series_identity_preserved).toBe(true);
  expect(result.initial_order).toHaveLength(3);
  expect(result.final_order).toEqual([result.initial_order[1], result.initial_order[2], result.initial_order[0]]);
  expect(result.pane_order).toEqual([result.initial_order[2], result.initial_order[0]]);
  expect(result.revenue_data).toMatchObject({ row_id: "r", value: 5 });
  expect(result.revenue_options).toMatchObject({ title: "Updated revenue", color: "#654321" });
  expect(result.all.items).toMatchObject([
    { pane: 1, kind: "scatter", title: "Hidden samples", color: null, visible: false },
    { pane: 2, kind: "xy_area", title: "Margin", color: "#abcdef", visible: true },
    { pane: 2, kind: "xy_line", title: "Updated revenue", color: "#654321", visible: true },
  ]);
  expect(result.first_only.items).toMatchObject([
    { kind: "scatter", title: "Hidden samples", visible: false },
  ]);
  expect(result.after_remove.items).toMatchObject([
    { kind: "xy_area", title: "Margin" },
    { kind: "xy_line", title: "Updated revenue" },
  ]);
  expect(result.restore_version).toBe(2);
  expect(result.restored).toEqual(result.before_restore);
});

test("general shared tooltips group visible series by the exact horizontal datum", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:720px;height:520px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(720, 520, 1);
    const pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" },
    });
    chart.add_axis({ id: "shared-x", pane: pane.pane_index(), dimension: "x", scale: "linear" });
    chart.add_axis({ id: "shared-y", pane: pane.pane_index(), dimension: "y", scale: "linear" });

    const line = chart.add_series("xy_line", {
      pane: pane.pane_index(), x_axis_id: "shared-x", y_axis_id: "shared-y", title: "Line",
    });
    line.set_data([{ id: "line-1", x: 1, y: 10 }, { id: "line-2", x: 2, y: 20 }]);
    const scatter = chart.add_series("scatter", {
      pane: pane.pane_index(), x_axis_id: "shared-x", y_axis_id: "shared-y", title: "Scatter",
    });
    scatter.set_data([
      { id: "scatter-a", x: 1, y: 30 },
      { id: "scatter-b", x: 1, y: 31 },
      { id: "scatter-c", x: 3, y: 32 },
    ]);
    const hidden = chart.add_series("xy_area", {
      pane: pane.pane_index(), x_axis_id: "shared-x", y_axis_id: "shared-y",
      title: "Hidden", visible: false,
    });
    hidden.set_data([{ id: "hidden", x: 1, y: 40 }]);

    let invalid_row = null;
    try {
      chart.general_shared_tooltip(line, -1);
    } catch (error) {
      invalid_row = error.code;
    }
    const snapshot = chart.general_shared_tooltip(line, 0);
    const missing = chart.general_shared_tooltip(line, 99);
    chart.remove();
    host.remove();
    return { invalid_row, snapshot, missing };
  });

  expect(result.invalid_row).toBe("invalid_options");
  expect(result.missing).toBeNull();
  expect(result.snapshot).toMatchObject({ pane: 1, anchor_row: 0 });
  expect(result.snapshot.items).toMatchObject([
    { row: 0, row_id: "line-1", x_label: "1", value: 10, title: "Line" },
    { row: 0, row_id: "scatter-a", x_label: "1", value: 30, title: "Scatter" },
    { row: 1, row_id: "scatter-b", x_label: "1", value: 31, title: "Scatter" },
  ]);
  expect(result.snapshot.items.some((item) => item.title === "Hidden")).toBe(false);
});

test("general brushes retain semantic ranges, select visible rows, reproject, and clear", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:720px;height:520px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(720, 520, 1);
    const pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" },
    });
    chart.add_axis({
      id: "brush-x", pane: pane.pane_index(), dimension: "x", scale: "linear", domain: [0, 4],
    });
    chart.add_axis({
      id: "brush-y", pane: pane.pane_index(), dimension: "y", scale: "linear", domain: [0, 100],
    });
    const line = chart.add_series("xy_line", {
      pane: pane.pane_index(), x_axis_id: "brush-x", y_axis_id: "brush-y", title: "Visible",
    });
    line.set_data([
      { id: "zero", x: 0, y: 10 },
      { id: "one", x: 1, y: 20 },
      { id: "two", x: 2, y: 30 },
      { id: "three", x: 3, y: 40 },
      { id: "four", x: 4, y: 50 },
    ]);
    const hidden = chart.add_series("scatter", {
      pane: pane.pane_index(), x_axis_id: "brush-x", y_axis_id: "brush-y", visible: false,
    });
    hidden.set_data([{ id: "hidden", x: 2, y: 90 }]);
    chart.resize(720, 520, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));

    const geometry = pane.get_geometry();
    const first = chart.set_general_brush("brush-x", geometry.width * 0.25, geometry.width * 0.75);
    const screenshot = chart.take_screenshot().toDataURL().length;
    chart.axis("brush-x").zoom(2, 2);
    const after_zoom = chart.general_brush_snapshot();
    chart.clear_general_brush();
    const cleared = chart.general_brush_snapshot();
    chart.remove();
    host.remove();
    return { first, after_zoom, cleared, screenshot };
  });

  expect(result.first).toMatchObject({
    pane: 1,
    axis_id: "brush-x",
    dimension: "x",
    range: { type: "numeric", from: 1, to: 3 },
  });
  expect(result.first.items.map((item) => item.row_id)).toEqual(["one", "two", "three"]);
  expect(result.after_zoom.range).toEqual(result.first.range);
  expect(result.after_zoom.items.map((item) => item.row_id)).toEqual(["one", "two", "three"]);
  expect(result.cleared).toBeNull();
  expect(result.screenshot).toBeGreaterThan(1000);
});

test("general reference lines, dots, and regions survive lifecycle and V2 restore", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:760px;height:640px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(760, 640, 1);

    const pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" },
    });
    chart.add_axis({ id: "reference-x", pane: pane.pane_index(), dimension: "x", scale: "linear" });
    chart.add_axis({ id: "reference-y", pane: pane.pane_index(), dimension: "y", scale: "linear" });
    const line = chart.add_general_reference({
      kind: "line",
      pane: pane.pane_index(),
      axis_id: "reference-x",
      value: 10,
      color: "#112233",
      line_width: 2,
      extend_domain: true,
    });
    const dot = chart.add_general_reference({
      kind: "dot",
      pane: pane.pane_index(),
      x_axis_id: "reference-x",
      y_axis_id: "reference-y",
      x: 4,
      y: 6,
      color: "#445566",
      radius: 5,
      extend_domain: true,
    });
    const region = chart.add_general_reference({
      kind: "region",
      pane: pane.pane_index(),
      x_axis_id: "reference-x",
      y_axis_id: "reference-y",
      x_from: 2,
      x_to: 8,
      y_from: 2,
      y_to: 8,
      fill_color: "rgba(10,20,30,0.25)",
      extend_domain: true,
    });

    const temporal_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "temporal" },
    });
    chart.add_axis({
      id: "reference-time",
      pane: temporal_pane.pane_index(),
      dimension: "x",
      scale: "temporal",
    });
    const temporal = chart.add_general_reference({
      kind: "line",
      pane: temporal_pane.pane_index(),
      axis_id: "reference-time",
      value: new Date(1_700_000_000_000),
      extend_domain: true,
    });

    chart.resize(760, 640, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const before = chart.general_references().map((reference) => ({
      id: reference.id,
      options: reference.options(),
    }));
    const pane_only = chart.general_references(pane.pane_index()).map((reference) => reference.id);
    const axis_blocked = chart.remove_axis("reference-x");
    const screenshot = chart.take_screenshot().toDataURL().length;

    const state = chart.export_state();
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:fixed;left:-10000px;top:0;width:760px;height:640px";
    document.body.appendChild(restored_host);
    const restored = await create_chart(restored_host, { backend: "canvas2d", autoSize: false });
    restored.resize(760, 640, 1);
    const restore = restored.import_state(state);
    const restored_options = restored.general_references().map((reference) => reference.options());

    const removed = dot.remove();
    let stale = null;
    try {
      dot.options();
    } catch (error) {
      stale = error.code;
    }
    const after_remove = chart.general_references().map((reference) => reference.id);
    const output = {
      before,
      pane_only,
      axis_blocked,
      restored_options,
      restore_version: restore.schema_version,
      removed,
      stale,
      after_remove,
      ids: { line: line.id, dot: dot.id, region: region.id, temporal: temporal.id },
      screenshot,
    };
    restored.remove();
    restored_host.remove();
    chart.remove();
    host.remove();
    return output;
  });

  expect(result.before.map((entry) => entry.id)).toEqual([
    result.ids.line,
    result.ids.dot,
    result.ids.region,
    result.ids.temporal,
  ]);
  expect(result.before[0].options).toMatchObject({
    kind: "line", pane: 1, axis_id: "reference-x", value: 10, extend_domain: true,
  });
  expect(result.before[1].options).toMatchObject({
    kind: "dot", pane: 1, x: 4, y: 6, radius: 5,
  });
  expect(result.before[2].options).toMatchObject({
    kind: "region", pane: 1, x_from: 2, x_to: 8, y_from: 2, y_to: 8,
  });
  expect(result.before[3].options).toMatchObject({
    kind: "line", pane: 2, axis_id: "reference-time", value: 1_700_000_000_000,
  });
  expect(result.pane_only).toEqual([result.ids.line, result.ids.dot, result.ids.region]);
  expect(result.axis_blocked).toBe(false);
  expect(result.restore_version).toBe(2);
  expect(result.restored_options).toEqual(result.before.map((entry) => entry.options));
  expect(result.removed).toBe(true);
  expect(result.stale).toBe("stale_handle");
  expect(result.after_remove).toEqual([result.ids.line, result.ids.region, result.ids.temporal]);
  expect(result.screenshot).toBeGreaterThan(1000);
});

test("category area, line, and columns compose on shared axes", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:640px;height:400px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(640, 400, 1);
    const pane = chart.add_pane({ preserve_empty: true,
      horizontal_domain: { type: "category", scale: "band" } });
    const index = pane.pane_index();
    chart.add_axis({ id: "compose-x", pane: index, dimension: "x", scale: "band" });
    chart.add_axis({ id: "compose-y", pane: index, dimension: "y", scale: "linear" });
    const options = { pane: index, x_axis_id: "compose-x", y_axis_id: "compose-y" };
    const area = chart.add_series("xy_area", options);
    const line = chart.add_series("xy_line", options);
    const bars = chart.add_series("column", options);
    for (const series of [area, line, bars]) {
      series.set_data([{ id: "a", x: "A", y: 4 }, { id: "b", x: "B", y: 6 }]);
    }
    const ranges = chart.add_series("range_bar", options);
    ranges.set_data([{ id: "range-a", x: "A", low: 1, high: 2 }]);
    const band = chart.add_series("range_area", options);
    band.set_data([{ id: "band-a", x: "A", low: 2, high: 3 },
      { id: "band-b", x: "B", low: 3, high: 4 }]);
    const boxes = chart.add_series("box_plot", options);
    boxes.set_data([{ id: "box-b", x: "B", min: 1, q1: 2, median: 3, q3: 4, max: 5 }]);
    const errors = chart.add_series("error_bar", options);
    errors.set_data([{ id: "error-a", x: "A", y: 3, y_low: 2, y_high: 4 }]);
    chart.resize(640, 400, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const kinds = pane.get_series().map((series) => series.kind);
    const saved_kinds = chart.export_state().series.map((series) => series.kind);
    let bar_hit = false;
    const geometry = pane.get_geometry();
    for (let y = geometry.top + 2; y < geometry.top + geometry.height - 2 && !bar_hit; y += 3) {
      for (let x = 0; x < geometry.width; x += 3) {
        if (chart.general_hit_test(index, x, y)?.series === bars.id) { bar_hit = true; break; }
      }
    }
    bars.set_visible(false);
    const hidden = pane.get_series().map((series) => series.options().visible);
    const screenshot = chart.take_screenshot().toDataURL().length;
    chart.remove();
    host.remove();
    return { kinds, saved_kinds, bar_hit, hidden, screenshot };
  });
  expect(result.kinds).toEqual(["xy_area", "xy_line", "column", "range_bar", "range_area", "box_plot", "error_bar"]);
  expect(result.saved_kinds.slice(-7)).toEqual(["XyArea", "XyLine", "Column", "RangeBar", "RangeArea", "BoxPlot", "ErrorBar"]);
  expect(result.bar_hit).toBe(true);
  expect(result.hidden).toEqual([true, true, false, true, true, true, true]);
  expect(result.screenshot).toBeGreaterThan(1000);
});

test("continuous line, scatter, bubble, error, and heatmap share numeric axes", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:640px;height:400px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(640, 400, 1);
    const pane = chart.add_pane({ preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" } });
    const index = pane.pane_index();
    chart.add_axis({ id: "numeric-compose-x", pane: index, dimension: "x", scale: "linear" });
    chart.add_axis({ id: "numeric-compose-y", pane: index, dimension: "y", scale: "linear" });
    const options = { pane: index, x_axis_id: "numeric-compose-x", y_axis_id: "numeric-compose-y" };
    const line = chart.add_series("xy_line", options);
    line.set_data([{ id: "l1", x: 1, y: 2 }, { id: "l2", x: 2, y: 3 }]);
    const scatter = chart.add_series("scatter", options);
    scatter.set_data([{ id: "s", x: 1.5, y: 2.5 }]);
    const bubble = chart.add_series("bubble", options);
    bubble.set_data([{ id: "b", x: 2, y: 2, size: 36 }]);
    const error = chart.add_series("error_bar", options);
    error.set_data([{ id: "e", x: 1, y: 3, x_low: 0.8, x_high: 1.2, y_low: 2.5, y_high: 3.5 }]);
    const heatmap = chart.add_series("heatmap_grid", options);
    heatmap.set_data([{ id: "h1", x: 1, y: 1, value: 5 }, { id: "h2", x: 2, y: 1, value: 10 }]);
    chart.resize(640, 400, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const kinds = pane.get_series().map((series) => series.kind);
    const saved_kinds = chart.export_state().series.map((series) => series.kind);
    const screenshot = chart.take_screenshot().toDataURL().length;
    chart.remove();
    host.remove();
    return { kinds, saved_kinds, screenshot };
  });
  expect(result.kinds).toEqual(["xy_line", "scatter", "bubble", "error_bar", "heatmap_grid"]);
  expect(result.saved_kinds.slice(-5)).toEqual(["XyLine", "Scatter", "Bubble", "ErrorBar", "HeatmapGrid"]);
  expect(result.screenshot).toBeGreaterThan(1000);
});

test("general columns group and stack through the browser API and V2 persistence", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:720px;height:640px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(720, 640, 1);

    const grouped_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "category", scale: "band" },
    });
    chart.add_axis({ id: "grouped-x", pane: grouped_pane.pane_index(), dimension: "x", scale: "band" });
    chart.add_axis({ id: "grouped-y", pane: grouped_pane.pane_index(), dimension: "y", scale: "linear" });
    const grouped_a = chart.add_series("column", {
      pane: grouped_pane.pane_index(), x_axis_id: "grouped-x", y_axis_id: "grouped-y",
      group_id: "sales", title: "North", bar_gap: 8, bar_max_width: 20, bar_corner_radius: 4,
    });
    const grouped_b = chart.add_series("column", {
      pane: grouped_pane.pane_index(), x_axis_id: "grouped-x", y_axis_id: "grouped-y",
      group_id: "sales", title: "South", bar_gap: 8, bar_max_width: 20, bar_corner_radius: 4,
    });
    grouped_a.set_data([{ id: "north-jan", x: "Jan", y: 10 }, { id: "north-feb", x: "Feb", y: 14 }]);
    grouped_b.set_data([{ id: "south-jan", x: "Jan", y: 18 }, { id: "south-feb", x: "Feb", y: 7 }]);

    const stacked_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "category", scale: "band" },
    });
    chart.add_axis({ id: "stacked-x", pane: stacked_pane.pane_index(), dimension: "x", scale: "band" });
    chart.add_axis({ id: "stacked-y", pane: stacked_pane.pane_index(), dimension: "y", scale: "linear" });
    const stacked_a = chart.add_series("column", {
      pane: stacked_pane.pane_index(), x_axis_id: "stacked-x", y_axis_id: "stacked-y",
      group_id: "totals", stack_id: "combined", stack_mode: "normal", title: "Base", bar_corner_radius: 6,
    });
    const stacked_b = chart.add_series("column", {
      pane: stacked_pane.pane_index(), x_axis_id: "stacked-x", y_axis_id: "stacked-y",
      group_id: "totals", stack_id: "combined", stack_mode: "normal", title: "Top", bar_corner_radius: 6,
    });
    stacked_a.set_data([{ id: "base-up", x: "Up", y: 10 }, { id: "base-down", x: "Down", y: -4 }]);
    stacked_b.set_data([{ id: "top-up", x: "Up", y: 5 }, { id: "top-down", x: "Down", y: -2 }]);

    let invalid_stack_mode = null;
    try {
      chart.add_series("column", {
        pane: grouped_pane.pane_index(), x_axis_id: "grouped-x", y_axis_id: "grouped-y",
        stack_id: "bad", stack_mode: "invalid",
      });
    } catch (error) {
      invalid_stack_mode = error.code;
    }
    let invalid_bar_gap = null;
    try {
      grouped_a.apply_options({ bar_gap: -1 });
    } catch (error) {
      invalid_bar_gap = error.code;
    }
    let invalid_bar_corner_radius = null;
    try {
      stacked_b.apply_options({ bar_corner_radius: -1 });
    } catch (error) {
      invalid_bar_corner_radius = error.code;
    }

    chart.resize(720, 640, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));

    const scan_hits = (pane, seriesIds) => {
      const found = new Map();
      const geometry = pane.get_geometry();
      for (let y = geometry.top + 2; y < geometry.top + geometry.height - 2 && found.size < seriesIds.length; y += 2) {
        for (let x = 0; x < geometry.width; x += 2) {
          const hit = chart.general_hit_test(pane.pane_index(), x, y);
          if (hit && seriesIds.includes(hit.series) && !found.has(hit.series)) found.set(hit.series, hit);
        }
      }
      return Array.from(found.values());
    };
    const grouped_hits = scan_hits(grouped_pane, [grouped_a.id, grouped_b.id]);
    const stacked_hits = scan_hits(stacked_pane, [stacked_a.id, stacked_b.id]);
    const stack_reordered = chart.set_general_series_order([stacked_b, stacked_a], stacked_pane.pane_index());
    const stack_order = chart.general_series_order(stacked_pane.pane_index()).map((series) => series.options().title);

    const state = chart.export_state();
    const persisted_columns = state.series.filter((series) => series.kind === "Column");
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:fixed;left:-10000px;top:0;width:720px;height:640px";
    document.body.appendChild(restored_host);
    const restored = await create_chart(restored_host, { backend: "canvas2d", autoSize: false });
    restored.resize(720, 640, 1);
    const restore_result = restored.import_state(state);
    const restored_columns = restored.panes()
      .flatMap((pane) => pane.get_series())
      .filter((series) => series.kind === "column").length;
    const restored_stack_order = restored.general_series_order(stacked_pane.pane_index()).map((series) => series.options().title);

    const snapshot = {
      invalid_stack_mode,
      invalid_bar_gap,
      invalid_bar_corner_radius,
      grouped_width: grouped_a.options().bar_max_width,
      grouped_gap: grouped_a.options().bar_gap,
      grouped_hits,
      stacked_hits,
      stack_reordered,
      stack_order,
      restored_stack_order,
      persisted_columns: persisted_columns.map(({ group_id, stack_id, stack_mode, bar_gap, bar_max_width, bar_corner_radius }) => ({ group_id, stack_id, stack_mode, bar_gap, bar_max_width, bar_corner_radius })),
      restored_columns,
      restore_version: restore_result.schema_version,
      grouped_values: [grouped_a.data_at(0)?.value, grouped_b.data_at(0)?.value],
      stacked_values: [stacked_a.data_at(0)?.value, stacked_b.data_at(0)?.value],
    };
    restored.remove();
    restored_host.remove();
    chart.remove();
    host.remove();
    return snapshot;
  });

  expect(result.invalid_stack_mode).toBe("invalid_options");
  expect(result.invalid_bar_gap).toBe("invalid_options");
  expect(result.invalid_bar_corner_radius).toBe("invalid_options");
  expect([result.grouped_gap, result.grouped_width]).toEqual([8, 20]);
  expect(result.grouped_hits).toHaveLength(2);
  expect(new Set(result.grouped_hits.map((hit) => hit.series)).size).toBe(2);
  expect(result.stacked_hits).toHaveLength(2);
  expect(result.stack_reordered).toBe(true);
  expect(result.stack_order).toEqual(["Top", "Base"]);
  expect(result.restored_stack_order).toEqual(["Top", "Base"]);
  expect(new Set(result.stacked_hits.map((hit) => hit.series)).size).toBe(2);
  expect(result.grouped_values).toEqual([10, 18]);
  expect(result.stacked_values).toEqual([10, 5]);
  expect(result.persisted_columns).toEqual([
    { group_id: "sales", stack_id: null, stack_mode: "Normal", bar_gap: 8, bar_max_width: 20, bar_corner_radius: 4 },
    { group_id: "sales", stack_id: null, stack_mode: "Normal", bar_gap: 8, bar_max_width: 20, bar_corner_radius: 4 },
    { group_id: "totals", stack_id: "combined", stack_mode: "Normal", bar_gap: 0, bar_max_width: null, bar_corner_radius: 6 },
    { group_id: "totals", stack_id: "combined", stack_mode: "Normal", bar_gap: 0, bar_max_width: null, bar_corner_radius: 6 },
  ]);
  expect(result.restore_version).toBe(2);
  expect(result.restored_columns).toBe(4);
});

test("cumulative signed stacks round-trip through browser options and V2 state", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:720px;height:640px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(720, 640, 1);
    const pane = chart.add_pane({ preserve_empty: true, horizontal_domain: { type: "category", scale: "band" } });
    chart.add_axis({ id: "cum-x", pane: pane.pane_index(), dimension: "x", scale: "band" });
    chart.add_axis({ id: "cum-y", pane: pane.pane_index(), dimension: "y", scale: "linear" });
    const options = {
      pane: pane.pane_index(), x_axis_id: "cum-x", y_axis_id: "cum-y",
      stack_id: "signed", stack_mode: "cumulative", bar_corner_radius: 4,
    };
    const first = chart.add_series("column", { ...options, title: "First" });
    const second = chart.add_series("column", { ...options, title: "Second" });
    first.set_data([{ x: "A", y: 10 }, { x: "B", y: -3 }]);
    second.set_data([{ x: "A", y: -15 }, { x: "B", y: 5 }]);
    chart.resize(720, 640, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const geometry = pane.get_geometry();
    let second_hit = false;
    for (let y = geometry.top + 2; y < geometry.top + geometry.height - 2 && !second_hit; y += 3) {
      for (let x = 0; x < geometry.width && !second_hit; x += 3) {
        second_hit = chart.general_hit_test(pane.pane_index(), x, y)?.series === second.id;
      }
    }
    const state = chart.export_state();
    const serialized = state.series.filter((series) => series.stack_id === "signed").map((series) => series.stack_mode);
    const restore_host = document.createElement("div");
    restore_host.style.cssText = "position:fixed;left:-10000px;top:0;width:720px;height:640px";
    document.body.appendChild(restore_host);
    const restored = await create_chart(restore_host, { backend: "canvas2d", autoSize: false });
    restored.resize(720, 640, 1);
    restored.import_state(state);
    const round_trip = restored.export_state().series.filter((series) => series.stack_id === "signed").map((series) => series.stack_mode);
    const result = { second_hit, option: second.options().stack_mode, serialized, round_trip };
    restored.remove();
    chart.remove();
    restore_host.remove();
    host.remove();
    return result;
  });

  expect(result).toEqual({
    second_hit: true,
    option: "cumulative",
    serialized: ["Cumulative", "Cumulative"],
    round_trip: ["Cumulative", "Cumulative"],
  });
});

test("silhouette stacks center signed area totals and survive V2 restore", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:640px;height:400px";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(640, 400, 1);
    const pane = chart.add_pane({ preserve_empty: true, horizontal_domain: { type: "continuous", scale: "linear" } });
    chart.add_axis({ id: "sil-x", pane: pane.pane_index(), dimension: "x", scale: "linear" });
    chart.add_axis({ id: "sil-y", pane: pane.pane_index(), dimension: "y", scale: "linear" });
    const options = {
      pane: pane.pane_index(), x_axis_id: "sil-x", y_axis_id: "sil-y",
      stack_id: "centered", stack_mode: "silhouette",
    };
    const first = chart.add_series("xy_area", { ...options, title: "Silhouette first" });
    const second = chart.add_series("xy_area", { ...options, title: "Silhouette second" });
    first.set_data([{ x: 0, y: 10 }, { x: 1, y: -3 }]);
    second.set_data([{ x: 0, y: -15 }, { x: 1, y: 5 }]);
    const state = chart.export_state();
    const serialized = state.series.filter((series) => series.stack_id === "centered").map((series) => series.stack_mode);
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:fixed;left:-10000px;top:0;width:640px;height:400px";
    document.body.appendChild(restored_host);
    const restored = await create_chart(restored_host, { backend: "canvas2d", autoSize: false });
    restored.resize(640, 400, 1);
    restored.import_state(state);
    const round_trip = restored.export_state().series.filter((series) => series.stack_id === "centered").map((series) => series.stack_mode);
    const result = { option: second.options().stack_mode, serialized, round_trip };
    restored.remove();
    chart.remove();
    restored_host.remove();
    host.remove();
    return result;
  });
  expect(result).toEqual({
    option: "silhouette",
    serialized: ["Silhouette", "Silhouette"],
    round_trip: ["Silhouette", "Silhouette"],
  });
});

test("wiggle stacks change area geometry and survive V2 restore", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:640px;height:400px";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(640, 400, 1);
    const pane = chart.add_pane({ preserve_empty: true, horizontal_domain: { type: "continuous", scale: "linear" } });
    chart.add_axis({ id: "wig-x", pane: pane.pane_index(), dimension: "x", scale: "linear" });
    chart.add_axis({ id: "wig-y", pane: pane.pane_index(), dimension: "y", scale: "linear" });
    const options = { pane: pane.pane_index(), x_axis_id: "wig-x", y_axis_id: "wig-y",
      stack_id: "stream", stack_mode: "wiggle" };
    const first = chart.add_series("xy_area", { ...options, title: "First" });
    const second = chart.add_series("xy_area", { ...options, title: "Second" });
    first.set_data([{ x: 0, y: 2 }, { x: 1, y: 4 }]);
    second.set_data([{ x: 0, y: 2 }, { x: 1, y: 2 }]);
    const wiggle = chart.take_screenshot().toDataURL();
    const state = chart.export_state();
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:fixed;left:-10000px;top:0;width:640px;height:400px";
    document.body.appendChild(restored_host);
    const restored = await create_chart(restored_host, { backend: "canvas2d", autoSize: false });
    restored.resize(640, 400, 1);
    restored.import_state(state);
    const round_trip = restored.export_state().series.filter((series) => series.stack_id === "stream").map((series) => series.stack_mode);
    chart.remove_series(second);
    chart.remove_series(first);
    const cumulative_options = { ...options, stack_mode: "cumulative" };
    const base = chart.add_series("xy_area", { ...cumulative_options, title: "First" });
    const top = chart.add_series("xy_area", { ...cumulative_options, title: "Second" });
    base.set_data([{ x: 0, y: 2 }, { x: 1, y: 4 }]);
    top.set_data([{ x: 0, y: 2 }, { x: 1, y: 2 }]);
    const changed = wiggle !== chart.take_screenshot().toDataURL();
    const result = { option: restored.export_state().series.find((series) => series.stack_id === "stream")?.stack_mode,
      round_trip, changed };
    restored.remove(); chart.remove(); restored_host.remove(); host.remove();
    return result;
  });
  expect(result).toEqual({ option: "Wiggle", round_trip: ["Wiggle", "Wiggle"], changed: true });
});

test("custom area gradient validates and round-trips through browser options", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:640px;height:400px";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(640, 400, 1);
    const pane = chart.add_pane({ preserve_empty: true, horizontal_domain: { type: "continuous", scale: "linear" } });
    chart.add_axis({ id: "gradient-x", pane: pane.pane_index(), dimension: "x", scale: "linear" });
    chart.add_axis({ id: "gradient-y", pane: pane.pane_index(), dimension: "y", scale: "linear" });
    const area = chart.add_series("xy_area", {
      pane: pane.pane_index(), x_axis_id: "gradient-x", y_axis_id: "gradient-y",
      title: "Gradient area", fill_opacity: 0.5, fill_gradient: ["#ff000080", "#0000ff"],
    });
    area.set_data([{ x: 0, y: 1 }, { x: 1, y: 2 }]);
    const stacked = chart.add_series("xy_area", {
      pane: pane.pane_index(), x_axis_id: "gradient-x", y_axis_id: "gradient-y",
      title: "Gradient stack", stack_id: "gradient-stack", fill_opacity: 0.5,
      fill_gradient: ["#00ff00", "#000000"],
    });
    stacked.set_data([{ x: 0, y: 0.5 }, { x: 1, y: 1.5 }]);
    const range = chart.add_series("range_area", {
      pane: pane.pane_index(), x_axis_id: "gradient-x", y_axis_id: "gradient-y",
      title: "Gradient range", fill_opacity: 0.5,
      fill_gradient: ["#ffff00", "#000000"],
    });
    range.set_data([{ x: 0, low: 0.25, high: 1.25 }, { x: 1, low: 0.5, high: 2.5 }]);
    let invalid = null;
    try {
      area.apply_options({ fill_gradient: ["bad-color", "#0000ff"] });
    } catch (error) {
      invalid = error.code;
    }
    const option = area.options().fill_gradient;
    const state = chart.export_state();
    const serialized = state.series
      .filter((series) => series.title.startsWith("Gradient "))
      .map((series) => series.fill_gradient);
    const restore_host = document.createElement("div");
    restore_host.style.cssText = "position:fixed;left:-10000px;top:0;width:640px;height:400px";
    document.body.appendChild(restore_host);
    const restored = await create_chart(restore_host, { backend: "canvas2d", autoSize: false });
    restored.resize(640, 400, 1);
    restored.import_state(state);
    const round_trip = restored.export_state().series
      .filter((series) => series.title.startsWith("Gradient "))
      .map((series) => series.fill_gradient);
    restored.remove();
    chart.remove();
    restore_host.remove();
    host.remove();
    return { invalid, option, serialized, round_trip };
  });
  expect(result).toEqual({
    invalid: "invalid_options",
    option: ["#ff000080", "#0000ff"],
    serialized: [["#ff000080", "#0000ff"], ["#00ff00", "#000000"], ["#ffff00", "#000000"]],
    round_trip: [["#ff000080", "#0000ff"], ["#00ff00", "#000000"], ["#ffff00", "#000000"]],
  });
});

test("bar gradients render and persist across both orientations and range bars", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const results = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const output = [];
    for (const kind of ["column", "horizontal_bar", "range_bar"]) {
      const horizontal = kind === "horizontal_bar";
      const host = document.createElement("div");
      Object.assign(host.style, { width: "640px", height: "360px" });
      document.body.appendChild(host);
      const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
      try {
        const pane = chart.add_pane({
          preserve_empty: true,
          horizontal_domain: horizontal
            ? { type: "continuous", scale: "linear" }
            : { type: "category", scale: "band" },
        });
        chart.add_axis({ id: "bar-gradient-x", pane: pane.pane_index(), dimension: "x", scale: horizontal ? "linear" : "band" });
        chart.add_axis({ id: "bar-gradient-y", pane: pane.pane_index(), dimension: "y", scale: horizontal ? "band" : "linear" });
        const bars = chart.add_series(kind, {
          pane: pane.pane_index(), x_axis_id: "bar-gradient-x", y_axis_id: "bar-gradient-y",
          title: "Gradient bar",
          bar_corner_radius: kind === "range_bar" ? 0 : 5,
        });
        bars.set_data(kind === "range_bar"
          ? [{ x: "A", low: 1, high: 5 }, { x: "B", low: 2, high: 4 }]
          : [{ x: "A", y: 5 }, { x: "B", y: 3 }]);
        const solid = chart.take_screenshot().toDataURL();
        bars.apply_options({ fill_gradient: ["#ff0000", "#0000ff"] });
        const gradient = chart.take_screenshot().toDataURL();
        output.push({
          kind,
          changed: gradient !== solid,
          option: bars.options().fill_gradient,
          persisted: chart.export_state().series.find((series) => series.title === "Gradient bar")?.fill_gradient,
        });
      } finally {
        chart.remove();
        host.remove();
      }
    }
    return output;
  });
  for (const result of results) {
    expect(result.changed, result.kind).toBe(true);
    expect(result.option).toEqual(["#ff0000", "#0000ff"]);
    expect(result.persisted).toEqual(["#ff0000", "#0000ff"]);
  }
});

test("positive stack mode omits negative bars and survives V2 restore", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:640px;height:400px";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(640, 400, 1);
    const pane = chart.add_pane({ preserve_empty: true, horizontal_domain: { type: "category", scale: "band" } });
    chart.add_axis({ id: "positive-x", pane: pane.pane_index(), dimension: "x", scale: "band" });
    chart.add_axis({ id: "positive-y", pane: pane.pane_index(), dimension: "y", scale: "linear" });
    const add = (title, up, down) => {
      const series = chart.add_series("column", {
        pane: pane.pane_index(), x_axis_id: "positive-x", y_axis_id: "positive-y",
        stack_id: "positive", stack_mode: "positive", title,
      });
      series.set_data([{ id: `${title}-up`, x: "Up", y: up }, { id: `${title}-down`, x: "Down", y: down }]);
      return series;
    };
    const a = add("A", 10, -4);
    const b = add("B", 5, -2);
    chart.resize(640, 400, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const hits = new Map();
    const geometry = pane.get_geometry();
    for (let y = geometry.top + 2; y < geometry.top + geometry.height - 2; y += 2) {
      for (let x = 0; x < geometry.width; x += 2) {
        const hit = chart.general_hit_test(pane.pane_index(), x, y);
        if (hit) hits.set(`${hit.series}:${hit.row}`, hit);
      }
    }
    const state = chart.export_state();
    const restored_host = document.createElement("div");
    document.body.appendChild(restored_host);
    const restored = await create_chart(restored_host, { backend: "canvas2d", autoSize: false });
    restored.resize(640, 400, 1);
    restored.import_state(state);
    const snapshot = {
      hits: [...hits.values()].map(({ series, row }) => ({ series, row })),
      values: [a.data_at(1)?.value, b.data_at(1)?.value],
      modes: state.series.map((series) => series.stack_mode),
      restored_modes: restored.panes().flatMap((entry) => entry.get_series())
        .filter((series) => series.kind === "column")
        .map((series) => series.options().stack_mode),
    };
    restored.remove();
    restored_host.remove();
    chart.remove();
    host.remove();
    return snapshot;
  });
  expect(result.hits).toHaveLength(2);
  expect(result.hits.every(({ row }) => row === 0)).toBe(true);
  expect(result.values).toEqual([-4, -2]);
  expect(result.modes).toEqual(["Positive", "Positive"]);
  expect(result.restored_modes).toEqual(["positive", "positive"]);
});

test("horizontal bars use category Y axes with stacking, hits, typed updates, and V2 restore", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:720px;height:520px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(720, 520, 1);

    const pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" },
    });
    chart.add_axis({ id: "hbar-x", pane: pane.pane_index(), dimension: "x", scale: "linear" });
    chart.add_axis({ id: "hbar-y", pane: pane.pane_index(), dimension: "y", scale: "band" });
    const base = chart.add_series("horizontal_bar", {
      pane: pane.pane_index(),
      x_axis_id: "hbar-x",
      y_axis_id: "hbar-y",
      group_id: "totals",
      stack_id: "combined",
      stack_mode: "normal",
      title: "Base",
    });
    const top = chart.add_series("horizontal_bar", {
      pane: pane.pane_index(),
      x_axis_id: "hbar-x",
      y_axis_id: "hbar-y",
      group_id: "totals",
      stack_id: "combined",
      stack_mode: "normal",
      title: "Top",
    });
    base.set_data([
      { id: "base-up", x: "Up", y: 10 },
      { id: "base-down", x: "Down", y: -4 },
    ]);
    top.set_data_typed({
      ids: ["top-up", "top-down"],
      categories: ["Up", "Down"],
      category_indices: new Uint32Array([0, 1]),
      y: new Float64Array([5, -2]),
    });
    top.update_data_typed({
      ids: ["top-up"],
      categories: ["Up"],
      category_indices: new Uint32Array([0]),
      y: new Float64Array([6]),
    });

    chart.resize(720, 520, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const found = new Map();
    const geometry = pane.get_geometry();
    for (let y = geometry.top + 2; y < geometry.top + geometry.height - 2 && found.size < 2; y += 2) {
      for (let x = 0; x < geometry.width; x += 2) {
        const hit = chart.general_hit_test(pane.pane_index(), x, y);
        if (hit && [base.id, top.id].includes(hit.series) && !found.has(hit.series)) {
          found.set(hit.series, hit);
        }
      }
    }

    const state = chart.export_state();
    const persisted = state.series
      .filter((series) => series.kind === "HorizontalBar")
      .map(({ group_id, stack_id, stack_mode }) => ({ group_id, stack_id, stack_mode }));
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:fixed;left:-10000px;top:0;width:720px;height:520px";
    document.body.appendChild(restored_host);
    const restored = await create_chart(restored_host, { backend: "canvas2d", autoSize: false });
    restored.resize(720, 520, 1);
    const restore = restored.import_state(state);
    const restored_bars = restored.panes()
      .flatMap((candidate) => candidate.get_series())
      .filter((series) => series.kind === "horizontal_bar").length;
    const output = {
      hits: Array.from(found.values()),
      persisted,
      restore_version: restore.schema_version,
      restored_bars,
      base_values: base.accessibility_snapshot(0, 10).items.map((item) => item.value),
      top_values: top.accessibility_snapshot(0, 10).items.map((item) => item.value),
      screenshot: chart.take_screenshot().toDataURL().length,
    };
    restored.remove();
    restored_host.remove();
    chart.remove();
    host.remove();
    return output;
  });

  expect(result.hits).toHaveLength(2);
  expect(new Set(result.hits.map((hit) => hit.series)).size).toBe(2);
  expect(result.persisted).toEqual([
    { group_id: "totals", stack_id: "combined", stack_mode: "Normal" },
    { group_id: "totals", stack_id: "combined", stack_mode: "Normal" },
  ]);
  expect(result.restore_version).toBe(2);
  expect(result.restored_bars).toBe(2);
  expect(result.base_values).toEqual([10, -4]);
  expect(result.top_values).toEqual([6, -2]);
  expect(result.screenshot).toBeGreaterThan(1000);
});

test("xy_area percent stacks through browser hits and V2 persistence", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:720px;height:560px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(720, 560, 1);

    const pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "category", scale: "point" },
    });
    chart.add_axis({ id: "area-stack-x", pane: pane.pane_index(), dimension: "x", scale: "point" });
    chart.add_axis({ id: "area-stack-y", pane: pane.pane_index(), dimension: "y", scale: "linear" });
    const base = chart.add_series("xy_area", {
      pane: pane.pane_index(),
      x_axis_id: "area-stack-x",
      y_axis_id: "area-stack-y",
      stack_id: "share",
      stack_mode: "percent",
      title: "Base",
    });
    const top = chart.add_series("xy_area", {
      pane: pane.pane_index(),
      x_axis_id: "area-stack-x",
      y_axis_id: "area-stack-y",
      stack_id: "share",
      stack_mode: "percent",
      title: "Top",
    });
    base.set_data([
      { id: "base-a", x: "A", y: 1 },
      { id: "base-b", x: "B", y: -3 },
      { id: "base-c", x: "C", y: 2 },
    ]);
    top.set_data([
      { id: "top-a", x: "A", y: 3 },
      { id: "top-b", x: "B", y: -1 },
      { id: "top-c", x: "C", y: 2 },
    ]);

    let invalid_group = null;
    try {
      chart.add_series("xy_area", {
        pane: pane.pane_index(),
        x_axis_id: "area-stack-x",
        y_axis_id: "area-stack-y",
        group_id: "not-for-area",
      });
    } catch (error) {
      invalid_group = error.code;
    }

    chart.resize(720, 560, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const found = new Map();
    const geometry = pane.get_geometry();
    for (let y = geometry.top + 2; y < geometry.top + geometry.height - 2 && found.size < 2; y += 2) {
      for (let x = 0; x < geometry.width; x += 2) {
        const hit = chart.general_hit_test(pane.pane_index(), x, y);
        if (hit && [base.id, top.id].includes(hit.series) && !found.has(hit.series)) {
          found.set(hit.series, hit);
        }
      }
    }

    const state = chart.export_state();
    const persisted = state.series
      .filter((series) => series.kind === "XyArea")
      .map(({ stack_id, stack_mode }) => ({ stack_id, stack_mode }));
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:fixed;left:-10000px;top:0;width:720px;height:560px";
    document.body.appendChild(restored_host);
    const restored = await create_chart(restored_host, { backend: "canvas2d", autoSize: false });
    restored.resize(720, 560, 1);
    const restore = restored.import_state(state);
    const restored_areas = restored.panes()
      .flatMap((candidate) => candidate.get_series())
      .filter((series) => series.kind === "xy_area").length;
    const output = {
      invalid_group,
      hits: Array.from(found.values()),
      persisted,
      restore_version: restore.schema_version,
      restored_areas,
      base_values: base.accessibility_snapshot(0, 10).items.map((item) => item.value),
      top_values: top.accessibility_snapshot(0, 10).items.map((item) => item.value),
      screenshot: chart.take_screenshot().toDataURL().length,
    };
    restored.remove();
    restored_host.remove();
    chart.remove();
    host.remove();
    return output;
  });

  expect(result.invalid_group).toBe("invalid_options");
  expect(result.hits).toHaveLength(2);
  expect(new Set(result.hits.map((hit) => hit.series)).size).toBe(2);
  expect(result.persisted).toEqual([
    { stack_id: "share", stack_mode: "Percent" },
    { stack_id: "share", stack_mode: "Percent" },
  ]);
  expect(result.restore_version).toBe(2);
  expect(result.restored_areas).toBe(2);
  expect(result.base_values).toEqual([1, -3, 2]);
  expect(result.top_values).toEqual([3, -1, 2]);
  expect(result.screenshot).toBeGreaterThan(1000);
});

test("bubble size data drives marks, hits, updates, accessibility, and V2 restore", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:720px;height:520px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(720, 520, 1);

    const pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" },
    });
    chart.add_axis({ id: "bubble-x", pane: pane.pane_index(), dimension: "x", scale: "linear" });
    chart.add_axis({ id: "bubble-y", pane: pane.pane_index(), dimension: "y", scale: "linear" });
    const bubble = chart.add_series("bubble", {
      pane: pane.pane_index(),
      x_axis_id: "bubble-x",
      y_axis_id: "bubble-y",
      title: "Population",
      color: "#336699",
      data_labels: true,
    });
    bubble.set_data([
      { id: "small", x: 1, y: 1, size: 4 },
      { id: "large", x: 2, y: 2, size: 100 },
      { id: "zero", x: 3, y: 3, size: 0 },
      { id: "missing", x: 4, y: 4, size: null },
    ]);

    let negative_size_rejection = null;
    try {
      bubble.update_data([{ id: "large", x: 2, y: 2, size: -1 }]);
    } catch (error) {
      negative_size_rejection = error.code;
    }
    const after_rejection = bubble.data_at(1);

    bubble.update_data_typed({
      ids: ["large", "new"],
      x: new Float64Array([2.5, 5]),
      y: new Float64Array([2.5, 5]),
      size: new Float64Array([144, 25]),
    });
    chart.resize(720, 520, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));

    let hit = null;
    const geometry = pane.get_geometry();
    for (let y = geometry.top + 2; y < geometry.top + geometry.height - 2 && hit === null; y += 2) {
      for (let x = 0; x < geometry.width; x += 2) {
        const candidate = chart.general_hit_test(pane.pane_index(), x, y);
        if (candidate?.series === bubble.id) {
          hit = candidate;
          break;
        }
      }
    }

    const before_restore = bubble.accessibility_snapshot(0, 10);
    const state = chart.export_state();
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:fixed;left:-10000px;top:0;width:720px;height:520px";
    document.body.appendChild(restored_host);
    const restored = await create_chart(restored_host, { backend: "canvas2d", autoSize: false });
    restored.resize(720, 520, 1);
    const restore_result = restored.import_state(state);
    const restored_bubble = restored.panes()
      .flatMap((candidate) => candidate.get_series())
      .find((series) => series.kind === "bubble");
    const restored_snapshot = restored_bubble?.accessibility_snapshot(0, 10) ?? null;

    const snapshot = {
      negative_size_rejection,
      after_rejection,
      before_restore,
      restored_snapshot,
      restored_kind: restored_bubble?.kind ?? null,
      restore_version: restore_result.schema_version,
      hit,
      screenshot: chart.take_screenshot().toDataURL().length,
    };
    restored.remove();
    restored_host.remove();
    chart.remove();
    host.remove();
    return snapshot;
  });

  expect(result.negative_size_rejection).toBe("invalid_data");
  expect(result.after_rejection).toMatchObject({ row_id: "large", value: 2, size: 100 });
  expect(result.before_restore.items).toMatchObject([
    { row_id: "small", size: 4 },
    { row_id: "large", x_label: "2.5", value: 2.5, size: 144 },
    { row_id: "zero", size: 0 },
    { row_id: "missing", size: null },
    { row_id: "new", x_label: "5", value: 5, size: 25 },
  ]);
  expect(result.hit).toMatchObject({ series: expect.any(Number), row_id: expect.anything() });
  expect(result.restore_version).toBe(2);
  expect(result.restored_kind).toBe("bubble");
  expect(result.restored_snapshot).toEqual(result.before_restore);
  expect(result.screenshot).toBeGreaterThan(1000);
});

test("error bars preserve independent XY bounds through browser updates and restore", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:720px;height:520px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(720, 520, 1);
    const pane = chart.add_pane({ preserve_empty: true, horizontal_domain: { type: "continuous", scale: "linear" } });
    chart.add_axis({ id: "error-x", pane: pane.pane_index(), dimension: "x", scale: "linear" });
    chart.add_axis({ id: "error-y", pane: pane.pane_index(), dimension: "y", scale: "linear" });
    const errors = chart.add_series("error_bar", {
      pane: pane.pane_index(), x_axis_id: "error-x", y_axis_id: "error-y", title: "Uncertainty",
    });
    errors.set_data([
      { id: "full", x: 10, y: 20, x_low: 8, x_high: 13, y_low: 15, y_high: 26 },
      { id: "one-sided", x: 20, y: 30, x_high: 25, y_low: 24 },
      { id: "missing", x: 30, y: null, x_low: -1000, y_high: 2000 },
    ]);
    let invalid_range = null;
    try {
      errors.update_data([{ id: "full", x: 10, y: 20, x_low: 11 }]);
    } catch (error) {
      invalid_range = error.code;
    }
    const after_invalid = errors.data_at(0);
    errors.update_data_typed({
      ids: ["one-sided", "new"],
      x: new Float64Array([21, 40]), y: new Float64Array([31, 45]),
      x_low: new Float64Array([0, 38]), x_low_valid: new Uint8Array([0, 1]),
      x_high: new Float64Array([27, 42]),
      y_low: new Float64Array([25, 40]),
      y_high: new Float64Array([0, 49]), y_high_valid: new Uint8Array([0, 1]),
    }, { max_rows: 4 });
    chart.resize(720, 520, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const before_restore = errors.accessibility_snapshot(0, 10);
    let hit = null;
    const geometry = pane.get_geometry();
    for (let y = geometry.top + 2; y < geometry.top + geometry.height - 2 && hit === null; y += 2) {
      for (let x = 0; x < geometry.width; x += 2) {
        const candidate = chart.general_hit_test(pane.pane_index(), x, y);
        if (candidate?.series === errors.id) { hit = candidate; break; }
      }
    }
    const state = chart.export_state();
    errors.update_data([{ id: "tail", x: 50, y: 55, x_low: 48, y_high: 60 }], { max_rows: 3 });
    const retained = errors.accessibility_snapshot(0, 10);
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:fixed;left:-10000px;top:0;width:720px;height:520px";
    document.body.appendChild(restored_host);
    const restored = await create_chart(restored_host, { backend: "canvas2d", autoSize: false });
    restored.resize(720, 520, 1);
    const restore_result = restored.import_state(state);
    const restored_errors = restored.panes().flatMap((candidate) => candidate.get_series())
      .find((series) => series.kind === "error_bar");
    const snapshot = {
      invalid_range, after_invalid, before_restore, retained,
      restored_snapshot: restored_errors?.accessibility_snapshot(0, 10) ?? null,
      restored_kind: restored_errors?.kind ?? null,
      restore_version: restore_result.schema_version,
      hit,
      screenshot: chart.take_screenshot().toDataURL().length,
    };
    restored.remove(); restored_host.remove(); chart.remove(); host.remove();
    return snapshot;
  });

  expect(result.invalid_range).toBe("invalid_data");
  expect(result.after_invalid).toMatchObject({ row_id: "full", x_low: 8, x_high: 13, low: 15, high: 26 });
  expect(result.before_restore.items).toMatchObject([
    { row_id: "full", x_low: 8, x_high: 13, low: 15, high: 26 },
    { row_id: "one-sided", x_low: null, x_high: 27, low: 25, high: null },
    { row_id: "missing", value: null },
    { row_id: "new", x_low: 38, x_high: 42, low: 40, high: 49 },
  ]);
  expect(result.hit).toMatchObject({ series: expect.any(Number), row_id: expect.anything() });
  expect(result.restore_version).toBe(2);
  expect(result.retained.items.map((item) => item.row_id)).toEqual(["missing", "new", "tail"]);
  expect(result.retained.items[1]).toMatchObject({ x_low: 38, x_high: 42, low: 40, high: 49 });
  expect(result.restored_kind).toBe("error_bar");
  expect(result.restored_snapshot).toEqual(result.before_restore);
  expect(result.screenshot).toBeGreaterThan(1000);
});

test("category error bars use band and point centers with Y-only bounds", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:720px;height:760px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(720, 760, 1);
    const created = [];
    for (const scale of ["band", "point"]) {
      const pane = chart.add_pane({ preserve_empty: true, horizontal_domain: { type: "category", scale } });
      chart.add_axis({ id: `${scale}-x`, pane: pane.pane_index(), dimension: "x", scale });
      chart.add_axis({ id: `${scale}-y`, pane: pane.pane_index(), dimension: "y", scale: "linear" });
      const series = chart.add_series("error_bar", {
        pane: pane.pane_index(), x_axis_id: `${scale}-x`, y_axis_id: `${scale}-y`, title: `${scale} errors`,
      });
      if (scale === "band") {
        series.set_data([
          { id: "q1", x: "Q1", y: 20, y_low: 15, y_high: 26 },
          { id: "q2", x: "Q2", y: 30, y_low: 24 },
          { id: "gap", x: "Q3", y: null, y_low: -1000, y_high: 2000 },
        ]);
      } else {
        series.set_data_typed({
          ids: ["jan", "feb"], categories: ["Jan", "Feb"], category_indices: new Uint32Array([0, 1]),
          y: new Float64Array([10, 12]),
          y_low: new Float64Array([8, 0]), y_low_valid: new Uint8Array([1, 0]),
          y_high: new Float64Array([13, 15]),
        });
      }
      created.push({ pane, series });
    }
    let invalid = null;
    try { created[0].series.update_data([{ id: "q1", x: "Q1", y: 20, y_low: 21 }]); }
    catch (error) { invalid = error.code; }
    let invalid_x_bound = null;
    try { created[0].series.update_data([{ id: "q1", x: "Q1", y: 20, x_low: 1 }]); }
    catch (error) { invalid_x_bound = error.code; }
    const after_invalid = created[0].series.data_at(0);
    created[1].series.update_data_typed({
      ids: ["feb", "mar"], categories: ["Feb", "Mar"], category_indices: new Uint32Array([0, 1]),
      y: new Float64Array([14, 16]),
      y_low: new Float64Array([11, 13]),
      y_high: new Float64Array([0, 19]), y_high_valid: new Uint8Array([0, 1]),
    }, { max_rows: 2 });
    chart.resize(720, 760, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const snapshots = created.map(({ series }) => series.accessibility_snapshot(0, 10));
    let hit = null;
    const geometry = created[0].pane.get_geometry();
    for (let y = geometry.top + 2; y < geometry.top + geometry.height - 2 && hit === null; y += 2) {
      for (let x = 0; x < geometry.width; x += 2) {
        const candidate = chart.general_hit_test(created[0].pane.pane_index(), x, y);
        if (candidate?.series === created[0].series.id) { hit = candidate; break; }
      }
    }
    const state = chart.export_state();
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:fixed;left:-10000px;top:0;width:720px;height:760px";
    document.body.appendChild(restored_host);
    const restored = await create_chart(restored_host, { backend: "canvas2d", autoSize: false });
    restored.resize(720, 760, 1);
    const restore = restored.import_state(state);
    const restored_snapshots = restored.panes().flatMap((pane) => pane.get_series())
      .filter((series) => series.kind === "error_bar").map((series) => series.accessibility_snapshot(0, 10));
    const output = { invalid, invalid_x_bound, after_invalid, snapshots, restored_snapshots, restore_version: restore.schema_version, hit, screenshot: chart.take_screenshot().toDataURL().length };
    restored.remove(); restored_host.remove(); chart.remove(); host.remove();
    return output;
  });

  expect(result.invalid).toBe("invalid_data");
  expect(result.invalid_x_bound).toBe("invalid_data");
  expect(result.after_invalid).toMatchObject({ row_id: "q1", low: 15, high: 26, x_low: null });
  expect(result.snapshots[0].items).toMatchObject([
    { row_id: "q1", x_label: "Q1", value: 20, low: 15, high: 26, x_low: null },
    { row_id: "q2", x_label: "Q2", value: 30, low: 24, high: null },
    { row_id: "gap", value: null },
  ]);
  expect(result.snapshots[1].items).toMatchObject([
    { row_id: "feb", x_label: "Feb", value: 14, low: 11, high: null },
    { row_id: "mar", x_label: "Mar", value: 16, low: 13, high: 19 },
  ]);
  expect(result.hit).toMatchObject({ series: expect.any(Number), row_id: expect.anything() });
  expect(result.restore_version).toBe(2);
  expect(result.restored_snapshots).toEqual(result.snapshots);
  expect(result.screenshot).toBeGreaterThan(1000);
});

test("box plots preserve five-number summaries through updates, hits, accessibility, and V2 restore", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:720px;height:520px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(720, 520, 1);
    const pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "category", scale: "band" },
    });
    chart.add_axis({ id: "box-x", pane: pane.pane_index(), dimension: "x", scale: "band" });
    chart.add_axis({ id: "box-y", pane: pane.pane_index(), dimension: "y", scale: "linear" });
    const boxes = chart.add_series("box_plot", {
      pane: pane.pane_index(),
      x_axis_id: "box-x",
      y_axis_id: "box-y",
      title: "Distribution",
      color: "#345678",
      box_fill_color: "#112233",
      box_median_color: "#ff00ff",
      line_width: 3,
    });
    boxes.set_data([
      { id: "north", x: "North", min: 5, q1: 10, median: 15, q3: 20, max: 28 },
      { id: "south", x: "South", min: 12, q1: 18, median: 24, q3: 30, max: 40, label: "South spread" },
      { id: "missing", x: "Missing", min: -1000, q1: -500, median: null, q3: 500, max: 1000 },
    ]);

    let invalid_order = null;
    try {
      boxes.update_data([
        { id: "north", x: "North", min: 5, q1: 17, median: 15, q3: 20, max: 28 },
      ]);
    } catch (error) {
      invalid_order = error.code;
    }
    const after_invalid = boxes.data_at(0);
    let invalid_style = null;
    try {
      boxes.apply_options({ box_fill_color: "invalid" });
    } catch (error) {
      invalid_style = error.code;
    }

    boxes.update_data_typed({
      ids: ["south", "east"],
      categories: ["South", "East"],
      category_indices: new Uint32Array([0, 1]),
      min: new Float64Array([14, 7]),
      q1: new Float64Array([19, 11]),
      median: new Float64Array([25, 16]),
      q3: new Float64Array([31, 22]),
      max: new Float64Array([42, 29]),
    }, { max_rows: 3 });

    chart.resize(720, 520, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const before_restore = boxes.accessibility_snapshot(0, 10);
    const south = boxes.data_at(0);
    const missing = boxes.data_at(1);
    let hit = null;
    const geometry = pane.get_geometry();
    for (let y = geometry.top + 2; y < geometry.top + geometry.height - 2 && hit === null; y += 2) {
      for (let x = 0; x < geometry.width; x += 2) {
        const candidate = chart.general_hit_test(pane.pane_index(), x, y);
        if (candidate?.series === boxes.id) {
          hit = candidate;
          break;
        }
      }
    }

    const state = chart.export_state();
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:fixed;left:-10000px;top:0;width:720px;height:520px";
    document.body.appendChild(restored_host);
    const restored = await create_chart(restored_host, { backend: "canvas2d", autoSize: false });
    restored.resize(720, 520, 1);
    const restore = restored.import_state(state);
    const restored_boxes = restored.panes()
      .flatMap((candidate) => candidate.get_series())
      .find((series) => series.kind === "box_plot");
    const restored_snapshot = restored_boxes?.accessibility_snapshot(0, 10) ?? null;
    const persisted_boxes = state.series.find((series) => series.kind === "BoxPlot");
    const output = {
      invalid_order,
      invalid_style,
      persisted_style: persisted_boxes
        ? [persisted_boxes.box_fill_color, persisted_boxes.box_median_color, persisted_boxes.line_width] : null,
      restored_style: [restored_boxes?.options().box_fill_color,
        restored_boxes?.options().box_median_color, restored_boxes?.options().line_width],
      after_invalid,
      south,
      missing,
      before_restore,
      restored_snapshot,
      restored_kind: restored_boxes?.kind ?? null,
      restore_version: restore.schema_version,
      hit,
      screenshot: chart.take_screenshot().toDataURL().length,
    };
    restored.remove();
    restored_host.remove();
    chart.remove();
    host.remove();
    return output;
  });

  expect(result.invalid_order).toBe("invalid_data");
  expect(result.invalid_style).toBe("invalid_options");
  expect(result.persisted_style).toEqual(["#112233", "#ff00ff", 3]);
  expect(result.restored_style).toEqual(["#112233", "#ff00ff", 3]);
  expect(result.after_invalid).toMatchObject({
    row_id: "north", low: 5, q1: 10, value: 15, q3: 20, high: 28, x_low: null, x_high: null,
  });
  expect(result.before_restore.items).toMatchObject([
    { row_id: "south", x_label: "South", low: 14, q1: 19, value: 25, q3: 31, high: 42 },
    { row_id: "missing", x_label: "Missing", value: null, q1: -500, q3: 500 },
    { row_id: "east", x_label: "East", low: 7, q1: 11, value: 16, q3: 22, high: 29 },
  ]);
  expect(result.south).toMatchObject({
    row_id: "south", low: 14, q1: 19, value: 25, q3: 31, high: 42,
  });
  expect(result.missing).toMatchObject({ row_id: "missing", value: null, q1: -500, q3: 500 });
  expect(result.hit).toMatchObject({ series: expect.any(Number), row_id: expect.anything() });
  expect(result.restore_version).toBe(2);
  expect(result.restored_kind).toBe("box_plot");
  expect(result.restored_snapshot).toEqual(result.before_restore);
  expect(result.screenshot).toBeGreaterThan(1000);
});

test("heatmap grids preserve X/Y categories through typed updates, hits, accessibility, and V2 restore", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:720px;height:520px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(720, 520, 1);
    const pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "category", scale: "band" },
    });
    chart.add_axis({ id: "heat-x", pane: pane.pane_index(), dimension: "x", scale: "band" });
    chart.add_axis({ id: "heat-y", pane: pane.pane_index(), dimension: "y", scale: "band" });
    const heatmap = chart.add_series("heatmap_grid", {
      pane: pane.pane_index(),
      x_axis_id: "heat-x",
      y_axis_id: "heat-y",
      title: "Regional heat",
      color: "#336699",
      heatmap_value_domain: [0, 100],
      heatmap_low_color: "#0000ff",
      heatmap_high_color: "#ff0000",
    });
    heatmap.set_data([
      { id: "jan-north", x: "Jan", y: "North", value: 10 },
      { id: "jan-south", x: "Jan", y: "South", value: 30, label: "South January" },
      { id: "feb-north", x: "Feb", y: "North", value: 50 },
      { id: "feb-south", x: "Feb", y: "South", value: null },
    ]);

    let invalid_index = null;
    try {
      heatmap.update_data_typed({
        ids: ["jan-north"],
        x_categories: ["Jan"],
        x_category_indices: new Uint32Array([0]),
        y_categories: ["North"],
        y_category_indices: new Uint32Array([9]),
        value: new Float64Array([11]),
      });
    } catch (error) {
      invalid_index = error.code;
    }
    const after_invalid = heatmap.data_at(0);
    let invalid_domain = null;
    try {
      heatmap.apply_options({ heatmap_value_domain: [100, 0] });
    } catch (error) {
      invalid_domain = error.code;
    }

    heatmap.update_data_typed({
      ids: ["jan-south", "mar-west"],
      x_categories: ["Jan", "Mar"],
      x_category_indices: new Uint32Array([0, 1]),
      y_categories: ["South", "West"],
      y_category_indices: new Uint32Array([0, 1]),
      value: new Float64Array([35, 70]),
    }, { max_rows: 4 });

    chart.resize(720, 520, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const before_restore = heatmap.accessibility_snapshot(0, 10);
    let hit = null;
    const geometry = pane.get_geometry();
    for (let y = geometry.top + 2; y < geometry.top + geometry.height - 2 && hit === null; y += 2) {
      for (let x = 0; x < geometry.width; x += 2) {
        const candidate = chart.general_hit_test(pane.pane_index(), x, y);
        if (candidate?.series === heatmap.id) {
          hit = candidate;
          break;
        }
      }
    }

    const state = chart.export_state();
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:fixed;left:-10000px;top:0;width:720px;height:520px";
    document.body.appendChild(restored_host);
    const restored = await create_chart(restored_host, { backend: "canvas2d", autoSize: false });
    restored.resize(720, 520, 1);
    const restore = restored.import_state(state);
    const restored_heatmap = restored.panes()
      .flatMap((candidate) => candidate.get_series())
      .find((series) => series.kind === "heatmap_grid");
    const restored_snapshot = restored_heatmap?.accessibility_snapshot(0, 10) ?? null;
    const persisted_heatmap = state.series.find((series) => series.kind === "HeatmapGrid");
    const output = {
      invalid_index,
      invalid_domain,
      domain: heatmap.options().heatmap_value_domain,
      persisted_domain: persisted_heatmap?.heatmap_value_domain,
      restored_domain: restored_heatmap?.options().heatmap_value_domain,
      persisted_colors: persisted_heatmap
        ? [persisted_heatmap.heatmap_low_color, persisted_heatmap.heatmap_high_color] : null,
      restored_colors: [restored_heatmap?.options().heatmap_low_color,
        restored_heatmap?.options().heatmap_high_color],
      after_invalid,
      before_restore,
      restored_snapshot,
      restored_kind: restored_heatmap?.kind ?? null,
      restore_version: restore.schema_version,
      hit,
      screenshot: chart.take_screenshot().toDataURL().length,
    };
    restored.remove();
    restored_host.remove();
    chart.remove();
    host.remove();
    return output;
  });

  expect(result.invalid_index).toBe("invalid_data");
  expect(result.invalid_domain).toBe("invalid_options");
  expect(result.domain).toEqual([0, 100]);
  expect(result.persisted_domain).toEqual([0, 100]);
  expect(result.restored_domain).toEqual([0, 100]);
  expect(result.persisted_colors).toEqual(["#0000ff", "#ff0000"]);
  expect(result.restored_colors).toEqual(["#0000ff", "#ff0000"]);
  expect(result.after_invalid).toMatchObject({
    row_id: "jan-north", x_label: "Jan", y_label: "North", value: 10,
  });
  expect(result.before_restore.items).toMatchObject([
    { row_id: "jan-south", x_label: "Jan", y_label: "South", value: 35 },
    { row_id: "feb-north", x_label: "Feb", y_label: "North", value: 50 },
    { row_id: "feb-south", x_label: "Feb", y_label: "South", value: null },
    { row_id: "mar-west", x_label: "Mar", y_label: "West", value: 70 },
  ]);
  expect(result.hit).toMatchObject({ series: expect.any(Number), row_id: expect.anything() });
  expect(result.restore_version).toBe(2);
  expect(result.restored_kind).toBe("heatmap_grid");
  expect(result.restored_snapshot).toEqual(result.before_restore);
  expect(result.screenshot).toBeGreaterThan(1000);
});

test("numeric and temporal heatmap grids preserve coordinates through typed updates, hits, and V2 restore", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:760px;height:720px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(760, 720, 1);

    const numeric_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" },
    });
    chart.add_axis({ id: "numeric-heat-x", pane: numeric_pane.pane_index(), dimension: "x", scale: "linear" });
    chart.add_axis({ id: "numeric-heat-y", pane: numeric_pane.pane_index(), dimension: "y", scale: "linear" });
    const numeric = chart.add_series("heatmap_grid", {
      pane: numeric_pane.pane_index(),
      x_axis_id: "numeric-heat-x",
      y_axis_id: "numeric-heat-y",
      title: "Numeric heat",
    });
    numeric.set_data([
      { id: "n00", x: 0, y: 10, value: 1 },
      { id: "n01", x: 0, y: 20, value: 2 },
      { id: "n10", x: 10, y: 10, value: 3 },
      { id: "n11", x: 10, y: 20, value: null },
    ]);
    let invalid_numeric = null;
    try {
      numeric.update_data_typed({
        ids: ["n00"],
        x: new Float64Array([0]),
        y_coordinate: new Float64Array([10, 20]),
        value: new Float64Array([5]),
      });
    } catch (error) {
      invalid_numeric = error.code;
    }
    numeric.update_data_typed({
      ids: ["n00"],
      x: new Float64Array([0]),
      y_coordinate: new Float64Array([10]),
      value: new Float64Array([11]),
    });

    const temporal_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "temporal" },
    });
    chart.add_axis({ id: "temporal-heat-x", pane: temporal_pane.pane_index(), dimension: "x", scale: "temporal" });
    chart.add_axis({ id: "temporal-heat-y", pane: temporal_pane.pane_index(), dimension: "y", scale: "linear" });
    const temporal = chart.add_series("heatmap_grid", {
      pane: temporal_pane.pane_index(),
      x_axis_id: "temporal-heat-x",
      y_axis_id: "temporal-heat-y",
      title: "Temporal heat",
    });
    temporal.set_data([
      { id: "t00", x: new Date(1_700_000_000_000), y: 5, value: 10 },
      { id: "t01", x: 1_700_000_000_000, y: 15, value: 20 },
      { id: "t10", x: new Date(1_700_000_060_000), y: 5, value: 30 },
      { id: "t11", x: 1_700_000_060_000, y: 15, value: 40 },
    ]);
    temporal.update_data_typed({
      ids: ["t11"],
      x_epoch_ms: new Float64Array([1_700_000_060_000]),
      y_coordinate: new Float64Array([15]),
      value: new Float64Array([44]),
    });

    chart.resize(760, 720, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const snapshots = [numeric.accessibility_snapshot(0, 10), temporal.accessibility_snapshot(0, 10)];
    const findHit = (pane, series) => {
      const geometry = pane.get_geometry();
      for (let y = geometry.top + 2; y < geometry.top + geometry.height - 2; y += 2) {
        for (let x = 0; x < geometry.width; x += 2) {
          const hit = chart.general_hit_test(pane.pane_index(), x, y);
          if (hit?.series === series.id) return hit;
        }
      }
      return null;
    };
    const hits = [findHit(numeric_pane, numeric), findHit(temporal_pane, temporal)];

    const state = chart.export_state();
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:fixed;left:-10000px;top:0;width:760px;height:720px";
    document.body.appendChild(restored_host);
    const restored = await create_chart(restored_host, { backend: "canvas2d", autoSize: false });
    restored.resize(760, 720, 1);
    const restore = restored.import_state(state);
    const restored_heatmaps = restored.panes()
      .flatMap((candidate) => candidate.get_series())
      .filter((series) => series.kind === "heatmap_grid");
    const restored_snapshots = restored_heatmaps.map((series) => series.accessibility_snapshot(0, 10));
    const output = {
      invalid_numeric,
      numeric_first: numeric.data_at(0),
      temporal_last: temporal.data_at(3),
      snapshots,
      restored_snapshots,
      restore_version: restore.schema_version,
      hits,
      screenshot: chart.take_screenshot().toDataURL().length,
    };
    restored.remove();
    restored_host.remove();
    chart.remove();
    host.remove();
    return output;
  });

  expect(result.invalid_numeric).toBe("invalid_data");
  expect(result.numeric_first).toMatchObject({ row_id: "n00", x_label: "0", y_label: "10", value: 11 });
  expect(result.temporal_last).toMatchObject({
    row_id: "t11", x_label: "1700000060000", y_label: "15", value: 44,
  });
  expect(result.snapshots[0].items).toMatchObject([
    { row_id: "n00", x_label: "0", y_label: "10", value: 11 },
    { row_id: "n01", x_label: "0", y_label: "20", value: 2 },
    { row_id: "n10", x_label: "10", y_label: "10", value: 3 },
    { row_id: "n11", x_label: "10", y_label: "20", value: null },
  ]);
  expect(result.snapshots[1].items[3]).toMatchObject({
    row_id: "t11", x_label: "1700000060000", y_label: "15", value: 44,
  });
  expect(result.hits[0]).toMatchObject({ series: expect.any(Number), row_id: expect.anything() });
  expect(result.hits[1]).toMatchObject({ series: expect.any(Number), row_id: expect.anything() });
  expect(result.restore_version).toBe(2);
  expect(result.restored_snapshots).toEqual(result.snapshots);
  expect(result.screenshot).toBeGreaterThan(1000);
});

test("temporal error bars preserve Date and epoch-millisecond XY bounds", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:720px;height:520px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(720, 520, 1);
    const pane = chart.add_pane({ preserve_empty: true, horizontal_domain: { type: "temporal" } });
    chart.add_axis({ id: "temporal-error-x", pane: pane.pane_index(), dimension: "x", scale: "temporal" });
    chart.add_axis({ id: "temporal-error-y", pane: pane.pane_index(), dimension: "y", scale: "linear" });
    const errors = chart.add_series("error_bar", {
      pane: pane.pane_index(), x_axis_id: "temporal-error-x", y_axis_id: "temporal-error-y", title: "Timed uncertainty",
    });
    errors.set_data([
      {
        id: "first",
        x: new Date(1_700_000_000_000),
        y: 20,
        x_low: new Date(1_699_999_970_000),
        x_high: 1_700_000_030_000,
        y_low: 15,
        y_high: 26,
      },
      {
        id: "second",
        x: 1_700_000_060_000,
        y: 30,
        x_high: new Date(1_700_000_090_000),
        y_low: 25,
      },
    ]);

    let invalid_order = null;
    try {
      errors.update_data([{
        id: "first", x: 1_700_000_000_000, y: 20, x_low: 1_700_000_000_001,
      }]);
    } catch (error) {
      invalid_order = error.code;
    }
    const after_invalid_order = errors.data_at(0);

    let invalid_fractional = null;
    try {
      errors.update_data_typed({
        ids: ["first"],
        x_epoch_ms: new Float64Array([1_700_000_000_000]),
        y: new Float64Array([20]),
        x_low_epoch_ms: new Float64Array([1_699_999_999_999.5]),
        x_high_epoch_ms: new Float64Array([1_700_000_030_000]),
        y_low: new Float64Array([15]),
        y_high: new Float64Array([26]),
      });
    } catch (error) {
      invalid_fractional = error.code;
    }

    errors.update_data_typed({
      ids: ["second", "new"],
      x_epoch_ms: new Float64Array([1_700_000_070_000, 1_700_000_120_000]),
      y: new Float64Array([31, 45]),
      x_low_epoch_ms: new Float64Array([0, 1_700_000_100_000]),
      x_low_valid: new Uint8Array([0, 1]),
      x_high_epoch_ms: new Float64Array([1_700_000_095_000, 1_700_000_140_000]),
      y_low: new Float64Array([25, 40]),
      y_high: new Float64Array([0, 49]),
      y_high_valid: new Uint8Array([0, 1]),
    }, { max_rows: 3 });

    chart.resize(720, 520, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const before_restore = errors.accessibility_snapshot(0, 10);
    let hit = null;
    const geometry = pane.get_geometry();
    for (let y = geometry.top + 2; y < geometry.top + geometry.height - 2 && hit === null; y += 2) {
      for (let x = 0; x < geometry.width; x += 2) {
        const candidate = chart.general_hit_test(pane.pane_index(), x, y);
        if (candidate?.series === errors.id) { hit = candidate; break; }
      }
    }

    const state = chart.export_state();
    errors.update_data([{
      id: "tail",
      x: new Date(1_700_000_180_000),
      y: 55,
      x_low: new Date(1_700_000_160_000),
      y_high: 60,
    }], { max_rows: 3 });
    const retained = errors.accessibility_snapshot(0, 10);

    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:fixed;left:-10000px;top:0;width:720px;height:520px";
    document.body.appendChild(restored_host);
    const restored = await create_chart(restored_host, { backend: "canvas2d", autoSize: false });
    restored.resize(720, 520, 1);
    const restore = restored.import_state(state);
    const restored_errors = restored.panes().flatMap((candidate) => candidate.get_series())
      .find((series) => series.kind === "error_bar");
    const output = {
      invalid_order,
      invalid_fractional,
      after_invalid_order,
      before_restore,
      retained,
      restored_snapshot: restored_errors?.accessibility_snapshot(0, 10) ?? null,
      restore_version: restore.schema_version,
      hit,
      screenshot: chart.take_screenshot().toDataURL().length,
    };
    restored.remove(); restored_host.remove(); chart.remove(); host.remove();
    return output;
  });

  expect(result.invalid_order).toBe("invalid_data");
  expect(result.invalid_fractional).toBe("invalid_data");
  expect(result.after_invalid_order).toMatchObject({
    row_id: "first",
    x_label: "1700000000000",
    x_low: 1_699_999_970_000,
    x_high: 1_700_000_030_000,
    low: 15,
    high: 26,
  });
  expect(result.before_restore.items).toMatchObject([
    { row_id: "first", x_label: "1700000000000", x_low: 1_699_999_970_000, x_high: 1_700_000_030_000 },
    { row_id: "second", x_label: "1700000070000", x_low: null, x_high: 1_700_000_095_000, low: 25, high: null },
    { row_id: "new", x_label: "1700000120000", x_low: 1_700_000_100_000, x_high: 1_700_000_140_000, low: 40, high: 49 },
  ]);
  expect(result.retained.items.map((item) => item.row_id)).toEqual(["second", "new", "tail"]);
  expect(result.hit).toMatchObject({ series: expect.any(Number), row_id: expect.anything() });
  expect(result.restore_version).toBe(2);
  expect(result.restored_snapshot).toEqual(result.before_restore);
  expect(result.screenshot).toBeGreaterThan(1000);
});

test("range_area spans numeric, temporal, and category domains through the browser API", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:720px;height:760px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(720, 760, 1);

    const numeric_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" },
    });
    chart.add_axis({ id: "range-x", pane: numeric_pane.pane_index(), dimension: "x", scale: "linear" });
    chart.add_axis({ id: "range-y", pane: numeric_pane.pane_index(), dimension: "y", scale: "linear" });
    const numeric = chart.add_series("range_area", {
      pane: numeric_pane.pane_index(), x_axis_id: "range-x", y_axis_id: "range-y", title: "Forecast",
    });
    numeric.set_data([
      { id: "a", x: 1, low: 2, high: 5, label: "A" },
      { id: "b", x: 2, low: 3, high: 7, label: "B" },
      { id: "gap", x: 3, low: null, high: 8 },
    ]);

    let invalid_range = null;
    try {
      numeric.update_data([{ id: "b", x: 2, low: 9, high: 4 }]);
    } catch (error) {
      invalid_range = error.code;
    }
    const after_invalid = numeric.data_at(1);
    numeric.update_data_typed({
      ids: ["b", "c"],
      labels: ["B2", "C"],
      x: new Float64Array([2.5, 4]),
      low: new Float64Array([4, 5]),
      high: new Float64Array([8, 9]),
    }, { max_rows: 4 });

    const temporal_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "temporal" },
    });
    chart.add_axis({ id: "range-time", pane: temporal_pane.pane_index(), dimension: "x", scale: "temporal" });
    chart.add_axis({ id: "range-time-y", pane: temporal_pane.pane_index(), dimension: "y", scale: "linear" });
    const temporal = chart.add_series("range_area", {
      pane: temporal_pane.pane_index(), x_axis_id: "range-time", y_axis_id: "range-time-y",
    });
    temporal.set_data_typed({
      ids: ["t1", "t2"],
      x_epoch_ms: new Float64Array([Date.UTC(2026, 0, 1), Date.UTC(2026, 0, 2)]),
      low: new Float64Array([10, 12]),
      high: new Float64Array([14, 16]),
    });

    const category_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "category", scale: "point" },
    });
    chart.add_axis({ id: "range-category", pane: category_pane.pane_index(), dimension: "x", scale: "point" });
    chart.add_axis({ id: "range-category-y", pane: category_pane.pane_index(), dimension: "y", scale: "linear" });
    const category = chart.add_series("range_area", {
      pane: category_pane.pane_index(), x_axis_id: "range-category", y_axis_id: "range-category-y",
    });
    category.set_data_typed({
      ids: ["q1", "q2"],
      categories: ["Q1", "Q2"],
      category_indices: new Uint32Array([0, 1]),
      low: new Float64Array([20, 22]),
      high: new Float64Array([28, 30]),
    });

    chart.resize(720, 760, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    let hit = null;
    const geometry = numeric_pane.get_geometry();
    for (let y = geometry.top + 2; y < geometry.top + geometry.height - 2 && hit === null; y += 2) {
      for (let x = 0; x < geometry.width; x += 2) {
        const candidate = chart.general_hit_test(numeric_pane.pane_index(), x, y);
        if (candidate?.series === numeric.id) {
          hit = candidate;
          break;
        }
      }
    }

    const before_restore = [numeric, temporal, category].map((series) => series.accessibility_snapshot(0, 10));
    const state = chart.export_state();
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:fixed;left:-10000px;top:0;width:720px;height:760px";
    document.body.appendChild(restored_host);
    const restored = await create_chart(restored_host, { backend: "canvas2d", autoSize: false });
    restored.resize(720, 760, 1);
    const restore_result = restored.import_state(state);
    const restored_ranges = restored.panes()
      .flatMap((pane) => pane.get_series())
      .filter((series) => series.kind === "range_area")
      .map((series) => series.accessibility_snapshot(0, 10));

    const snapshot = {
      invalid_range,
      after_invalid,
      hit,
      before_restore,
      restored_ranges,
      restore_version: restore_result.schema_version,
      screenshot: chart.take_screenshot().toDataURL().length,
    };
    restored.remove();
    restored_host.remove();
    chart.remove();
    host.remove();
    return snapshot;
  });

  expect(result.invalid_range).toBe("invalid_data");
  expect(result.after_invalid).toMatchObject({ row_id: "b", low: 3, high: 7, value: 7 });
  expect(result.hit).toMatchObject({ series: expect.any(Number), row_id: expect.anything() });
  expect(result.before_restore[0].items).toMatchObject([
    { row_id: "a", x_label: "1", label: "A", low: 2, high: 5 },
    { row_id: "b", x_label: "2.5", label: "B2", low: 4, high: 8 },
    { row_id: "gap", low: null, high: 8 },
    { row_id: "c", x_label: "4", label: "C", low: 5, high: 9 },
  ]);
  expect(result.before_restore[1].items).toMatchObject([
    { row_id: "t1", low: 10, high: 14 },
    { row_id: "t2", low: 12, high: 16 },
  ]);
  expect(result.before_restore[2].items).toMatchObject([
    { row_id: "q1", x_label: "Q1", low: 20, high: 28 },
    { row_id: "q2", x_label: "Q2", low: 22, high: 30 },
  ]);
  expect(result.restore_version).toBe(2);
  expect(result.restored_ranges).toEqual(result.before_restore);
  expect(result.screenshot).toBeGreaterThan(1000);
});

test("range_bar renders category low/high rectangles with exact browser hits", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  const result = await page.evaluate(async () => {
    const { create_chart } = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:640px;height:420px;z-index:10000";
    document.body.appendChild(host);
    const chart = await create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(640, 420, 1);
    const pane = chart.add_pane({ preserve_empty: true, horizontal_domain: { type: "category", scale: "band" } });
    chart.add_axis({ id: "range-bar-x", pane: pane.pane_index(), dimension: "x", scale: "band" });
    chart.add_axis({ id: "range-bar-y", pane: pane.pane_index(), dimension: "y", scale: "linear" });
    const series = chart.add_series("range_bar", {
      pane: pane.pane_index(), x_axis_id: "range-bar-x", y_axis_id: "range-bar-y",
    });
    series.set_data([
      { id: "a", x: "A", low: 1, high: 3 },
      { id: "b", x: "B", low: 2, high: 5 },
    ]);
    chart.resize(640, 420, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const snapshot = series.data_at(0);
    const state = chart.export_state();
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:fixed;left:-10000px;top:0;width:640px;height:420px";
    document.body.appendChild(restored_host);
    const restored = await create_chart(restored_host, { backend: "canvas2d", autoSize: false });
    restored.resize(640, 420, 1);
    const restore_result = restored.import_state(state);
    const restored_series = restored.panes()[1]?.get_series().find((candidate) => candidate.kind === "range_bar")
      ?? restored.panes()[0]?.get_series().find((candidate) => candidate.kind === "range_bar");
    const restored_snapshot = restored_series?.data_at(0);
    restored.remove();
    restored_host.remove();
    chart.remove();
    host.remove();
    return { kind: series.kind, snapshot, restore_version: restore_result.schema_version, restored_kind: restored_series?.kind, restored_snapshot };
  });
  expect(result.kind).toBe("range_bar");
  expect(result.snapshot).toMatchObject({ low: 1, high: 3 });
  expect(result.restore_version).toBe(2);
  expect(result.restored_kind).toBe("range_bar");
  expect(result.restored_snapshot).toMatchObject({ low: 1, high: 3 });
});

test("xy_line and xy_area span general domains and restore through V2", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  const result = await page.evaluate(async () => {
    const api = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    Object.assign(host.style, {
      position: "fixed",
      width: "720px",
      height: "640px",
      left: "0",
      top: "0",
      zIndex: "10000",
    });
    document.body.appendChild(host);
    const chart = await api.create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(720, 640, 1);

    const numeric_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" },
    });
    chart.add_axis({ id: "line-x", pane: numeric_pane.pane_index(), dimension: "x", scale: "linear" });
    chart.add_axis({ id: "line-y", pane: numeric_pane.pane_index(), dimension: "y", scale: "linear" });
    const numeric_line = chart.add_series("xy_line", {
      pane: numeric_pane.pane_index(),
      x_axis_id: "line-x",
      y_axis_id: "line-y",
      title: "Numeric trend",
      color: "#336699",
      data_labels: true,
    });
    numeric_line.set_data([
      { id: "n0", x: 0, y: 0 },
      { id: "n1", x: 1, y: 1 },
      { id: "gap", x: 2, y: null },
      { id: "n3", x: 3, y: 3 },
      { id: "n4", x: 4, y: 4 },
    ]);

    const temporal_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "temporal" },
    });
    const temporal_axis = chart.add_axis({ id: "time-x", pane: temporal_pane.pane_index(), dimension: "x", scale: "temporal" });
    chart.add_axis({ id: "time-y", pane: temporal_pane.pane_index(), dimension: "y", scale: "linear" });
    const temporal_line = chart.add_series("xy_line", {
      pane: temporal_pane.pane_index(),
      x_axis_id: "time-x",
      y_axis_id: "time-y",
      title: "Temporal trend",
    });
    temporal_line.set_data_typed({
      ids: [1, 2, 3],
      x_epoch_ms: new Float64Array([1_700_000_000_000, 1_700_000_060_000, 1_700_000_120_000]),
      y: new Float64Array([10, 12, 11]),
    });
    temporal_line.update_data([
      { id: 2, x: new Date(1_700_000_060_000), y: 13 },
    ]);
    let temporal_rejection = null;
    try {
      temporal_line.set_data_typed({
        ids: [9],
        x_epoch_ms: new Float64Array([1_700_000_000_000.5]),
        y: new Float64Array([99]),
      });
    } catch (error) {
      temporal_rejection = error.code;
    }
    const temporal_after_rejection = temporal_line.accessibility_snapshot(0, 10);
    temporal_axis.zoom(2, 1_700_000_060_000);
    temporal_axis.pan(0.25);
    let temporal_view_rejection = null;
    try {
      temporal_axis.zoom(2, 1_700_000_060_000.5);
    } catch (error) {
      temporal_view_rejection = error.code;
    }
    temporal_axis.reset_view();

    const category_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "category", scale: "point" },
    });
    chart.add_axis({ id: "point-x", pane: category_pane.pane_index(), dimension: "x", scale: "point" });
    chart.add_axis({ id: "point-y", pane: category_pane.pane_index(), dimension: "y", scale: "linear" });
    const category_line = chart.add_series("xy_line", {
      pane: category_pane.pane_index(),
      x_axis_id: "point-x",
      y_axis_id: "point-y",
      title: "Category trend",
    });
    category_line.set_data([
      { id: "a", x: "A", y: 3 },
      { id: "b", x: "B", y: 1 },
      { id: "c", x: "C", y: 2 },
    ]);
    const category_area = chart.add_series("xy_area", {
      pane: category_pane.pane_index(),
      x_axis_id: "point-x",
      y_axis_id: "point-y",
      title: "Category area",
      color: "#7e57c2",
    });
    category_area.set_data([
      { id: "aa", x: "A", y: 1 },
      { id: "ab", x: "B", y: 2 },
      { id: "ac", x: "C", y: 1.5 },
    ]);

    chart.resize(720, 640, 1);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const geometry = numeric_pane.get_geometry();
    let hit = null;
    for (let y = geometry.top + 4; y < geometry.top + geometry.height - 4 && hit === null; y += 2) {
      for (let x = 0; x < geometry.width; x += 2) {
        const candidate = chart.general_hit_test(numeric_pane.pane_index(), x, y);
        if (candidate?.series === numeric_line.id) {
          hit = candidate;
          break;
        }
      }
    }

    const state = chart.export_state();
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:fixed;left:-10000px;top:0;width:720px;height:640px";
    document.body.appendChild(restored_host);
    const restored = await api.create_chart(restored_host, { backend: "canvas2d", autoSize: false });
    restored.resize(720, 640, 1);
    const restore_result = restored.import_state(state);
    const restored_kinds = restored.panes()
      .flatMap((pane) => pane.get_series())
      .map((series) => series.kind ?? series.series_type());

    const snapshot = {
      state_version: state.schema_version,
      restore_version: restore_result.schema_version,
      restored_kinds,
      numeric_gap: numeric_line.data_at(2),
      numeric_accessibility: numeric_line.accessibility_snapshot(0, 10),
      temporal_accessibility: temporal_after_rejection,
      category_accessibility: category_line.accessibility_snapshot(0, 10),
      area_accessibility: category_area.accessibility_snapshot(0, 10),
      temporal_rejection,
      temporal_view_rejection,
      hit,
      screenshot: chart.take_screenshot().toDataURL().length,
    };
    restored.remove();
    restored_host.remove();
    chart.remove();
    host.remove();
    return snapshot;
  });

  expect(result.state_version).toBe(2);
  expect(result.restore_version).toBe(2);
  expect(result.restored_kinds.filter((kind) => kind === "xy_line")).toHaveLength(3);
  expect(result.restored_kinds.filter((kind) => kind === "xy_area")).toHaveLength(1);
  expect(result.numeric_gap).toMatchObject({ row_id: "gap", x_label: "2", value: null });
  expect(result.numeric_accessibility.items.map((item) => item.value)).toEqual([0, 1, null, 3, 4]);
  expect(result.temporal_rejection).toBe("invalid_data");
  expect(result.temporal_view_rejection).toBe("invalid_options");
  expect(result.temporal_accessibility.items).toMatchObject([
    { row_id: 1, x_label: "1700000000000", value: 10 },
    { row_id: 2, x_label: "1700000060000", value: 13 },
    { row_id: 3, x_label: "1700000120000", value: 11 },
  ]);
  expect(result.category_accessibility.items.map((item) => item.x_label)).toEqual(["A", "B", "C"]);
  expect(result.area_accessibility.items.map((item) => item.value)).toEqual([1, 2, 1.5]);
  expect(result.hit).toMatchObject({ series: expect.any(Number), row_id: expect.anything() });
  expect(result.screenshot).toBeGreaterThan(1000);
});

test("general columns, points, lines, and ranges participate in the shared keyboard accessibility controller", async ({ page }) => {
  await page.goto("/?backend=canvas2d&forceFallbackAdapter=1");
  await page.waitForFunction(() => window.__chart?.backend?.() === "canvas2d");

  await page.evaluate(async () => {
    const api = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    Object.assign(host.style, {
      position: "fixed",
      width: "640px",
      height: "480px",
      left: "0",
      top: "0",
      zIndex: "10000",
    });
    host.id = "general-keyboard-host";
    document.body.appendChild(host);
    const chart = await api.create_chart(host, { backend: "canvas2d", autoSize: false });
    chart.resize(640, 480, 1);

    const category_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "category", scale: "band" },
    });
    chart.add_axis({ id: "kbd-month", pane: category_pane.pane_index(), dimension: "x", scale: "band" });
    chart.add_axis({ id: "kbd-revenue", pane: category_pane.pane_index(), dimension: "y", scale: "linear" });
    const columns = chart.add_series("column", {
      pane: category_pane.pane_index(),
      x_axis_id: "kbd-month",
      y_axis_id: "kbd-revenue",
      title: "Keyboard revenue",
    });
    columns.set_data([
      { id: "jan", x: "Jan", y: 10 },
      { id: "feb", x: "Feb", y: 20 },
      { id: "mar", x: "Mar", y: 30 },
    ]);

    const scatter_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" },
    });
    chart.add_axis({ id: "kbd-x", pane: scatter_pane.pane_index(), dimension: "x", scale: "linear" });
    chart.add_axis({ id: "kbd-y", pane: scatter_pane.pane_index(), dimension: "y", scale: "linear" });
    const scatter = chart.add_series("scatter", {
      pane: scatter_pane.pane_index(),
      x_axis_id: "kbd-x",
      y_axis_id: "kbd-y",
      title: "Keyboard samples",
    });
    scatter.set_data([
      { id: 1, x: -10, y: -5 },
      { id: 2, x: 0, y: 0, label: "Center" },
      { id: 3, x: 10, y: 5 },
    ]);

    const line_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" },
    });
    chart.add_axis({ id: "kbd-line-x", pane: line_pane.pane_index(), dimension: "x", scale: "linear" });
    chart.add_axis({ id: "kbd-line-y", pane: line_pane.pane_index(), dimension: "y", scale: "linear" });
    const line = chart.add_series("xy_line", {
      pane: line_pane.pane_index(),
      x_axis_id: "kbd-line-x",
      y_axis_id: "kbd-line-y",
      title: "Keyboard trend",
    });
    line.set_data([
      { id: "l1", x: -2, y: 1 },
      { id: "l2", x: 0, y: 3 },
      { id: "l3", x: 2, y: 2 },
    ]);

    const range_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" },
    });
    chart.add_axis({ id: "kbd-range-x", pane: range_pane.pane_index(), dimension: "x", scale: "linear" });
    chart.add_axis({ id: "kbd-range-y", pane: range_pane.pane_index(), dimension: "y", scale: "linear" });
    const range = chart.add_series("range_area", {
      pane: range_pane.pane_index(),
      x_axis_id: "kbd-range-x",
      y_axis_id: "kbd-range-y",
      title: "Keyboard forecast",
    });
    range.set_data([
      { id: "r1", x: -2, low: 1, high: 3 },
      { id: "r2", x: 0, low: 2, high: 5, label: "Expected" },
      { id: "r3", x: 2, low: 3, high: 6 },
    ]);

    const horizontal_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "continuous", scale: "linear" },
    });
    chart.add_axis({ id: "kbd-hbar-x", pane: horizontal_pane.pane_index(), dimension: "x", scale: "linear" });
    chart.add_axis({ id: "kbd-hbar-y", pane: horizontal_pane.pane_index(), dimension: "y", scale: "band" });
    const horizontal = chart.add_series("horizontal_bar", {
      pane: horizontal_pane.pane_index(),
      x_axis_id: "kbd-hbar-x",
      y_axis_id: "kbd-hbar-y",
      title: "Keyboard horizontal bars",
    });
    horizontal.set_data([
      { id: "h1", x: "North", y: 12 },
      { id: "h2", x: "South", y: -4, label: "South delta" },
      { id: "h3", x: "West", y: 8 },
    ]);

    const box_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "category", scale: "band" },
    });
    chart.add_axis({ id: "kbd-box-x", pane: box_pane.pane_index(), dimension: "x", scale: "band" });
    chart.add_axis({ id: "kbd-box-y", pane: box_pane.pane_index(), dimension: "y", scale: "linear" });
    const boxes = chart.add_series("box_plot", {
      pane: box_pane.pane_index(),
      x_axis_id: "kbd-box-x",
      y_axis_id: "kbd-box-y",
      title: "Keyboard distributions",
    });
    boxes.set_data([
      { id: "b1", x: "North", min: 5, q1: 10, median: 15, q3: 20, max: 28 },
      { id: "b2", x: "South", min: 12, q1: 18, median: 24, q3: 30, max: 40, label: "South spread" },
      { id: "b3", x: "West", min: 7, q1: 11, median: 16, q3: 22, max: 29 },
    ]);

    const heat_pane = chart.add_pane({
      preserve_empty: true,
      horizontal_domain: { type: "category", scale: "band" },
    });
    chart.add_axis({ id: "kbd-heat-x", pane: heat_pane.pane_index(), dimension: "x", scale: "band" });
    chart.add_axis({ id: "kbd-heat-y", pane: heat_pane.pane_index(), dimension: "y", scale: "band" });
    const heatmap = chart.add_series("heatmap_grid", {
      pane: heat_pane.pane_index(),
      x_axis_id: "kbd-heat-x",
      y_axis_id: "kbd-heat-y",
      title: "Keyboard heatmap",
    });
    heatmap.set_data([
      { id: "c1", x: "Jan", y: "North", value: 10 },
      { id: "c2", x: "Jan", y: "South", value: 20, label: "South January" },
      { id: "c3", x: "Feb", y: "North", value: 30 },
    ]);

    const accessibility = api.enable_accessibility(chart, {
      chart_title: (pane) => pane === category_pane.pane_index()
        ? "Category keyboard pane"
        : pane === scatter_pane.pane_index()
          ? "Scatter keyboard pane"
          : pane === line_pane.pane_index()
            ? "Line keyboard pane"
            : pane === range_pane.pane_index()
              ? "Range keyboard pane"
              : pane === horizontal_pane.pane_index()
                ? "Horizontal bar keyboard pane"
                : pane === box_pane.pane_index()
                  ? "Box plot keyboard pane"
                  : pane === heat_pane.pane_index()
                    ? "Heatmap keyboard pane"
              : "Financial pane",
      data_scope: "all",
      show_shortcuts: true,
    });
    window.__general_keyboard = { chart, host, columns, scatter, line, range, horizontal, boxes, heatmap, accessibility };
  });

  const layers = page.locator("#general-keyboard-host .aeris_charts-a11y-layer");
  await expect(layers).toHaveCount(8);

  await layers.nth(1).focus();
  await page.keyboard.press("Home");
  const category_region = layers.nth(1).locator(".aeris_charts-a11y-live-region");
  await expect(category_region).toContainText("Jan");

  await page.evaluate(() => {
    window.__general_keyboard.columns.set_data([
      { id: "feb", x: "Feb", y: 20 },
      { id: "jan", x: "Jan", y: 11 },
      { id: "mar", x: "Mar", y: 30 },
    ]);
  });
  await page.keyboard.press("ArrowRight");
  await expect(category_region).toContainText("Mar");
  const category_text = await category_region.textContent();
  expect(category_text).toContain("Mar");
  expect(category_text).toContain("Point 3 of 3");

  await layers.nth(2).focus();
  await page.keyboard.press("End");
  const scatter_region = layers.nth(2).locator(".aeris_charts-a11y-live-region");
  await expect(scatter_region).toContainText("Keyboard samples");
  const scatter_text = await scatter_region.textContent();
  expect(scatter_text).toContain("Keyboard samples");
  expect(scatter_text).toContain("10");
  expect(scatter_text).toContain("Point 3 of 3");

  const before_zoom = await page.evaluate(() => window.__general_keyboard.chart.take_screenshot().toDataURL());
  await page.keyboard.press("+");
  await page.waitForTimeout(30);
  const after_zoom = await page.evaluate(() => window.__general_keyboard.chart.take_screenshot().toDataURL());
  expect(after_zoom).not.toBe(before_zoom);

  await page.keyboard.press("Enter");
  await expect(scatter_region).toContainText("data points");

  await layers.nth(3).focus();
  await page.keyboard.press("Home");
  await page.keyboard.press("ArrowRight");
  const line_region = layers.nth(3).locator(".aeris_charts-a11y-live-region");
  await expect(line_region).toContainText("Keyboard trend");
  const line_text = await line_region.textContent();
  expect(line_text).toContain("Keyboard trend");
  expect(line_text).toContain("Point 2 of 3");
  const line_before_zoom = await page.evaluate(() => window.__general_keyboard.chart.take_screenshot().toDataURL());
  await page.keyboard.press("+");
  await page.waitForTimeout(30);
  const line_after_zoom = await page.evaluate(() => window.__general_keyboard.chart.take_screenshot().toDataURL());
  expect(line_after_zoom).not.toBe(line_before_zoom);

  await layers.nth(4).focus();
  await page.keyboard.press("Home");
  await page.keyboard.press("ArrowRight");
  const range_region = layers.nth(4).locator(".aeris_charts-a11y-live-region");
  await expect(range_region).toContainText("Keyboard forecast");
  const range_text = await range_region.textContent();
  expect(range_text).toContain("Expected");
  expect(range_text).toContain("2 to 5");
  expect(range_text).toContain("Point 2 of 3");
  const range_before_zoom = await page.evaluate(() => window.__general_keyboard.chart.take_screenshot().toDataURL());
  await page.keyboard.press("+");
  await page.waitForTimeout(30);
  const range_after_zoom = await page.evaluate(() => window.__general_keyboard.chart.take_screenshot().toDataURL());
  expect(range_after_zoom).not.toBe(range_before_zoom);

  await layers.nth(5).focus();
  await page.keyboard.press("Home");
  await page.keyboard.press("ArrowRight");
  const horizontal_region = layers.nth(5).locator(".aeris_charts-a11y-live-region");
  await expect(horizontal_region).toContainText("Keyboard horizontal bars");
  const horizontal_text = await horizontal_region.textContent();
  expect(horizontal_text).toContain("South delta");
  expect(horizontal_text).toContain("Point 2 of 3");

  await layers.nth(6).focus();
  await page.keyboard.press("Home");
  await page.keyboard.press("ArrowRight");
  const box_region = layers.nth(6).locator(".aeris_charts-a11y-live-region");
  await expect(box_region).toContainText("Keyboard distributions");
  const box_text = await box_region.textContent();
  expect(box_text).toContain("South spread");
  expect(box_text).toContain("q1");
  expect(box_text).toContain("median");
  expect(box_text).toContain("q3");
  expect(box_text).toContain("Point 2 of 3");

  await layers.nth(7).focus();
  await page.keyboard.press("Home");
  await page.keyboard.press("ArrowRight");
  const heat_region = layers.nth(7).locator(".aeris_charts-a11y-live-region");
  await expect(heat_region).toContainText("Keyboard heatmap");
  const heat_text = await heat_region.textContent();
  expect(heat_text).toContain("South January");
  expect(heat_text).toContain("South");
  expect(heat_text).toContain("20");
  expect(heat_text).toContain("Point 2 of 3");

  await page.evaluate(() => {
    window.__general_keyboard.accessibility.detach();
    window.__general_keyboard.chart.remove();
    window.__general_keyboard.host.remove();
    delete window.__general_keyboard;
  });
});
