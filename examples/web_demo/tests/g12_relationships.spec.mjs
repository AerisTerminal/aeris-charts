import { expect } from "@playwright/test";
import { test } from "./page_ready.mjs";
import { gauge, series_color } from "../echarts_reference/rows/gauge.js";
import { edges, nodes } from "../echarts_reference/rows/chord.js";
import { nodes as hierarchy_nodes } from "../echarts_reference/rows/tree.js";
import { edges as network_edges, nodes as network_nodes } from "../echarts_reference/rows/network.js";

// G12 gauge, relationship (graph/chord) and hierarchy (tree, dendrogram, circle packing, icicle)
// layouts are complete-build only: this spec runs in the `chromium-complete` project (see
// playwright.config.mjs). The standard build rejects these kinds with `complete_build_required`;
// build_variant.spec.mjs proves that gating in both projects.

async function blank_page(page) {
  await page.route("**/g12_blank.html", (route) => route.fulfill({
    contentType: "text/html; charset=utf-8",
    body: "<!doctype html><meta charset=utf-8><title>g12</title><body></body>",
  }));
  await page.goto("/g12_blank.html");
}

for (const backend of ["canvas2d", "webgpu"]) test(`gauge and chord layouts render and answer hit tests through ${backend}`, async ({ page }) => {
  await blank_page(page);
  const report = await page.evaluate(async ({ backend, gauge, series_color, nodes, edges }) => {
    const api = await import("/dist/aeris_charts_financial.js");
    const scenario = async (kind, data, update) => {
      const host = document.createElement("div");
      host.style.cssText = "position:relative;width:420px;height:320px";
      document.body.append(host);
      const chart = await api.create_chart(host, {
        backend,
        __force_webgpu_fallback_adapter: true,
        autoSize: false,
        theme: "light",
        initialPane: { horizontal_domain: { type: "layout" } },
      });
      if (chart.backend() !== backend) throw new Error(`expected ${backend}, got ${chart.backend()}`);
      chart.resize(420, 320, 1);
      const series = chart.add_layout_series(kind, { pane: 0, title: kind, color: series_color }, data);
      chart.render();
      const canvas = chart.take_screenshot();
      const image = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
      const key = (offset) => `${image[offset]},${image[offset + 1]},${image[offset + 2]}`;
      const corner = key(0);
      let painted = 0;
      for (let offset = 0; offset < image.length; offset += 16) {
        if (key(offset) !== corner) painted += 1;
      }
      const hits = {};
      for (let y = 0; y < 320; y += 2) {
        for (let x = 0; x < 420; x += 2) {
          const hit = chart.layout_hit_test(0, x, y);
          if (hit !== null && hits[hit.id] === undefined) hits[hit.id] = hit;
        }
      }
      const snapshot = series.accessibility_snapshot(0, 512);
      let updated = null;
      if (update !== undefined) {
        series.set_data(update);
        chart.render();
        updated = series.accessibility_snapshot(0, 512);
      }
      chart.remove();
      host.remove();
      return { painted, hits, snapshot, updated };
    };
    return {
      gauge: await scenario("gauge", { type: "gauge", ...gauge }, { type: "gauge", ...gauge, value: 18 }),
      chord: await scenario("chord", { type: "graph", nodes, edges }),
    };
  }, { backend, gauge, series_color, nodes, edges });

  expect(report.gauge.painted, "gauge paints its dial").toBeGreaterThan(200);
  // The progress arc paints over the bands below the value, so the value answers there: band 0
  // (0..30) lies wholly under the progress at 62.5 and is reachable only through accessibility.
  expect(Object.keys(report.gauge.hits).sort()).toEqual(["band-1", "band-2", "value"]);
  expect(["pointer", "progress"]).toContain(report.gauge.hits.value.element);
  expect(report.gauge.hits["band-2"]).toMatchObject({ element: "band", label: "High", value: 100 });
  expect(report.gauge.snapshot.total).toBe(4);
  expect(report.gauge.snapshot.items[0]).toMatchObject({ element: "pointer", id: "value", label: "CPU load", value: 62.5 });
  expect(report.gauge.snapshot.items.slice(1).map((item) => item.label)).toEqual(["Low", "Normal", "High"]);
  expect(report.gauge.updated.items[0].value).toBe(18);

  expect(report.chord.painted, "chord paints groups and ribbons").toBeGreaterThan(200);
  const node_ids = nodes.map((node) => node.id);
  const edge_ids = edges.map((edge) => `${edge.source}→${edge.target}`);
  for (const id of node_ids) expect(report.chord.hits[id], id).toMatchObject({ element: "node" });
  const found_edges = edge_ids.filter((id) => report.chord.hits[id] !== undefined);
  expect(found_edges.length, `ribbons answer hits: ${found_edges}`).toBeGreaterThanOrEqual(edges.length - 2);
  for (const id of found_edges) expect(report.chord.hits[id].element).toBe("link");
  expect(report.chord.snapshot.total).toBe(nodes.length + edges.length);
  // Node values default to incoming plus outgoing edge value.
  const flow = (id) => edges.reduce((sum, edge) => sum + (edge.source === id ? edge.value : 0) + (edge.target === id ? edge.value : 0), 0);
  expect(report.chord.snapshot.items.slice(0, nodes.length).map((item) => item.value)).toEqual(node_ids.map(flow));
  expect(report.chord.snapshot.items[nodes.length]).toMatchObject({ element: "link", id: "us→eu", label: "US to EU", value: 32 });
});

test("gauge and graph data validate atomically and survive persistence", async ({ page }) => {
  await blank_page(page);
  const result = await page.evaluate(async ({ gauge, nodes, edges }) => {
    const api = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:relative;width:420px;height:320px";
    document.body.append(host);
    const options = {
      backend: "canvas2d", autoSize: false, theme: "light",
      initialPane: { horizontal_domain: { type: "layout" } },
    };
    const chart = await api.create_chart(host, options);
    const code = (action) => {
      try {
        action();
        return "ok";
      } catch (error) {
        return error.code;
      }
    };
    const bad_bands = code(() => chart.add_layout_series("gauge", { pane: 0 }, {
      type: "gauge", value: 5, bands: [{ to: 60, color: "#ff0000" }, { to: 40, color: "#00ff00" }],
    }));
    const duplicate_edge = code(() => chart.add_layout_series("chord", { pane: 0 }, {
      type: "graph", nodes, edges: [...edges, edges[0]],
    }));
    const missing_node = code(() => chart.add_layout_series("chord", { pane: 0 }, {
      type: "graph", nodes, edges: [{ source: "us", target: "nowhere", value: 1 }],
    }));
    const wrong_data = code(() => chart.add_layout_series("chord", { pane: 0 }, { type: "gauge", value: 1 }));
    const count_after_errors = chart.layout_series().length;
    chart.add_layout_series("gauge", { pane: 0, title: "Load" }, { type: "gauge", ...gauge });
    chart.add_layout_series("chord", { pane: 0, title: "Trade" }, { type: "graph", nodes, edges });
    const state = JSON.stringify(chart.export_state());
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:relative;width:420px;height:320px";
    document.body.append(restored_host);
    const restored = await api.create_chart(restored_host, options);
    restored.import_state(state);
    const restored_state = JSON.stringify(restored.export_state());
    const kinds = restored.layout_series().map((series) => series.kind);
    chart.remove();
    restored.remove();
    host.remove();
    restored_host.remove();
    return { bad_bands, duplicate_edge, missing_node, wrong_data, count_after_errors, same: state === restored_state, kinds };
  }, { gauge, nodes, edges });
  expect(result).toEqual({
    bad_bands: "invalid_data",
    duplicate_edge: "invalid_data",
    missing_node: "invalid_data",
    wrong_data: "invalid_data",
    count_after_errors: 0,
    same: true,
    kinds: ["gauge", "chord"],
  });
});

for (const backend of ["canvas2d", "webgpu"]) test(`hierarchy layouts render, answer hit tests and collapse through ${backend}`, async ({ page }) => {
  await blank_page(page);
  const report = await page.evaluate(async ({ backend, nodes, series_color }) => {
    const api = await import("/dist/aeris_charts_financial.js");
    const mount = async (kind, options) => {
      const host = document.createElement("div");
      host.style.cssText = "position:relative;width:420px;height:320px";
      document.body.append(host);
      const chart = await api.create_chart(host, {
        backend,
        __force_webgpu_fallback_adapter: true,
        autoSize: false,
        theme: "light",
        initialPane: { horizontal_domain: { type: "layout" } },
      });
      if (chart.backend() !== backend) throw new Error(`expected ${backend}, got ${chart.backend()}`);
      chart.resize(420, 320, 1);
      const series = chart.add_layout_series(kind, { pane: 0, title: kind, color: series_color, ...options },
        { type: "hierarchy", nodes });
      return { host, chart, series };
    };
    const measure = ({ chart, series }) => {
      chart.render();
      const canvas = chart.take_screenshot();
      const image = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
      const key = (offset) => `${image[offset]},${image[offset + 1]},${image[offset + 2]}`;
      const corner = key(0);
      let painted = 0;
      for (let offset = 0; offset < image.length; offset += 16) {
        if (key(offset) !== corner) painted += 1;
      }
      const hits = {};
      for (let y = 0; y < 320; y += 2) {
        for (let x = 0; x < 420; x += 2) {
          const hit = chart.layout_hit_test(0, x, y);
          if (hit !== null && hits[hit.id] === undefined) hits[hit.id] = { ...hit, x, y };
        }
      }
      return { painted, hits, snapshot: series.accessibility_snapshot(0, 512), options: series.options() };
    };
    const report = {};
    for (const [name, kind, options] of [
      ["tree", "tree", {}],
      ["tree_radial", "tree", { hierarchy_orientation: "radial" }],
      ["dendrogram", "dendrogram", { hierarchy_orientation: "vertical" }],
      ["circle_packing", "circle_packing", {}],
      ["icicle", "icicle", { hierarchy_orientation: "vertical" }],
    ]) {
      const mounted = await mount(kind, options);
      report[name] = measure(mounted);
      if (name === "tree") {
        window.__g12_tree = mounted;
      } else {
        mounted.chart.remove();
        mounted.host.remove();
      }
    }
    return report;
  }, { backend, nodes: hierarchy_nodes, series_color });

  const nodes = hierarchy_nodes;
  const ids = nodes.map((node) => node.id);
  const parents = ["markets", "equities", "rates", "fx"];
  for (const name of ["tree", "tree_radial", "dendrogram", "circle_packing", "icicle"]) {
    const entry = report[name];
    expect(entry.painted, `${name} paints`).toBeGreaterThan(200);
    expect(Object.keys(entry.hits).sort(), `${name} hits every node`).toEqual([...ids].sort());
    for (const id of ids) expect(entry.hits[id].element, `${name} ${id}`).toBe("node");
    expect(entry.snapshot.total).toBe(nodes.length);
  }
  expect(report.icicle.hits.us_eq.value).toBe(42);
  expect(report.dendrogram.options.hierarchy_orientation).toBe("vertical");
  // Trees list visible nodes depth-first and report whether each parent is expanded.
  expect(report.tree.snapshot.items.map((item) => item.id).slice(0, 5)).toEqual(["markets", "equities", "us_eq", "eu_eq", "asia_eq"]);
  for (const item of report.tree.snapshot.items) {
    expect(item.expanded, item.id).toBe(parents.includes(item.id) ? true : undefined);
  }
  expect(report.icicle.snapshot.items.every((item) => item.expanded === undefined)).toBe(true);

  // A real primary click on a parent folds it; the engine owns the binding, the page wires nothing.
  const target = report.tree.hits.equities;
  // Hit-test x is measured from the plot's left edge.
  const box = await page.evaluate(() => {
    const rect = window.__g12_tree.host.getBoundingClientRect();
    return { x: rect.left + window.__g12_tree.chart.panes()[0].get_geometry().left, y: rect.top };
  });
  const state = () => page.evaluate(() => {
    const { chart, series } = window.__g12_tree;
    chart.render();
    const hidden = ["us_eq", "eu_eq", "asia_eq"];
    let hidden_hits = 0;
    for (let y = 0; y < 320; y += 2) {
      for (let x = 0; x < 420; x += 2) {
        const hit = chart.layout_hit_test(0, x, y);
        if (hit !== null && hidden.includes(hit.id)) hidden_hits += 1;
      }
    }
    const snapshot = series.accessibility_snapshot(0, 512);
    return {
      collapsed: series.options().collapsed,
      total: snapshot.total,
      equities: snapshot.items.find((item) => item.id === "equities"),
      hidden_hits,
      persisted: chart.export_state().layout_series[0].collapsed ?? [],
    };
  });
  await page.mouse.click(box.x + target.x, box.y + target.y);
  await expect.poll(async () => (await state()).collapsed).toEqual(["equities"]);
  const folded = await state();
  expect(folded).toMatchObject({ total: nodes.length - 3, hidden_hits: 0, persisted: ["equities"] });
  expect(folded.equities).toMatchObject({ expanded: false, selected: true });
  expect(await page.evaluate(() => window.__g12_tree.chart.layout_selected_hit()?.id)).toBe("equities");
  await page.waitForTimeout(550); // Keep the next click outside the engine's 500 ms double-click window.
  await page.mouse.click(box.x + target.x, box.y + target.y);
  await expect.poll(async () => (await state()).collapsed).toEqual([]);
  expect(await state()).toMatchObject({ total: nodes.length, persisted: [] });
  await page.evaluate(() => {
    window.__g12_tree.chart.remove();
    window.__g12_tree.host.remove();
  });
});

test("hierarchy layout options validate atomically and survive persistence", async ({ page }) => {
  await blank_page(page);
  const result = await page.evaluate(async ({ nodes }) => {
    const api = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:relative;width:420px;height:320px";
    document.body.append(host);
    const options = {
      backend: "canvas2d", autoSize: false, theme: "light",
      initialPane: { horizontal_domain: { type: "layout" } },
    };
    const chart = await api.create_chart(host, options);
    const data = { type: "hierarchy", nodes };
    const code = (action) => {
      try {
        action();
        return "ok";
      } catch (error) {
        return error.code;
      }
    };
    const radial_icicle = code(() => chart.add_layout_series("icicle", { pane: 0, hierarchy_orientation: "radial" }, data));
    const vertical_packing = code(() => chart.add_layout_series("circle_packing", { pane: 0, hierarchy_orientation: "vertical" }, data));
    const leaf_collapsed = code(() => chart.add_layout_series("tree", { pane: 0, collapsed: ["us_eq"] }, data));
    const count_after_errors = chart.layout_series().length;
    const tree = chart.add_layout_series("tree", { pane: 0, collapsed: ["rates"], hierarchy_orientation: "vertical" }, data);
    const icicle = chart.add_layout_series("icicle", { pane: 0 }, data);
    const patch_error = code(() => icicle.apply_options({ hierarchy_orientation: "radial", title: "Changed" }));
    const icicle_after_error = icicle.options();
    tree.apply_options({ hierarchy_orientation: "radial" });
    const state = JSON.stringify(chart.export_state());
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:relative;width:420px;height:320px";
    document.body.append(restored_host);
    const restored = await api.create_chart(restored_host, options);
    restored.import_state(state);
    const restored_state = JSON.stringify(restored.export_state());
    const restored_tree = restored.layout_series()[0];
    const tree_options = restored_tree.options();
    const tree_total = restored_tree.accessibility_snapshot(0, 0).total;
    chart.remove();
    restored.remove();
    host.remove();
    restored_host.remove();
    return {
      radial_icicle, vertical_packing, leaf_collapsed, count_after_errors, patch_error,
      icicle_after_error: { orientation: icicle_after_error.hierarchy_orientation, title: icicle_after_error.title },
      same: state === restored_state,
      tree: { orientation: tree_options.hierarchy_orientation, collapsed: tree_options.collapsed, total: tree_total },
    };
  }, { nodes: hierarchy_nodes });
  expect(result).toEqual({
    radial_icicle: "invalid_options",
    vertical_packing: "invalid_options",
    leaf_collapsed: "invalid_options",
    count_after_errors: 0,
    patch_error: "invalid_options",
    icicle_after_error: { orientation: "horizontal", title: "" },
    same: true,
    tree: { orientation: "radial", collapsed: ["rates"], total: hierarchy_nodes.length - 2 },
  });
});

for (const backend of ["canvas2d", "webgpu"]) test(`network layout simulates, drags, zooms and settles through ${backend}`, async ({ page }) => {
  await blank_page(page);
  const first = await page.evaluate(async ({ backend, nodes, edges }) => {
    const api = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:420px;height:320px";
    document.body.append(host);
    const chart = await api.create_chart(host, {
      backend,
      __force_webgpu_fallback_adapter: true,
      autoSize: false,
      theme: "light",
      initialPane: { horizontal_domain: { type: "layout" } },
    });
    if (chart.backend() !== backend) throw new Error(`expected ${backend}, got ${chart.backend()}`);
    chart.resize(420, 320, 1);
    const series = chart.add_layout_series("network", { pane: 0, title: "Counterparties" }, { type: "graph", nodes, edges });
    chart.render();
    // Node centers are the centroids of the hit cells that answer each node id.
    const positions = () => {
      const sums = {};
      for (let y = 0; y < 320; y += 2) {
        for (let x = 0; x < 420; x += 2) {
          const hit = chart.layout_hit_test(0, x, y);
          if (hit === null || hit.element !== "node") continue;
          const sum = sums[hit.id] ?? (sums[hit.id] = { x: 0, y: 0, n: 0 });
          sum.x += x;
          sum.y += y;
          sum.n += 1;
        }
      }
      return Object.fromEntries(Object.entries(sums).map(([id, s]) => [id, { x: s.x / s.n, y: s.y / s.n }]));
    };
    const painted = () => {
      const canvas = chart.take_screenshot();
      const image = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
      const key = (offset) => `${image[offset]},${image[offset + 1]},${image[offset + 2]}`;
      const corner = key(0);
      let count = 0;
      for (let offset = 0; offset < image.length; offset += 16) if (key(offset) !== corner) count += 1;
      return count;
    };
    window.__network = { chart, series, positions, painted };
    return { positions: positions(), painted: painted() };
  }, { backend, nodes: network_nodes, edges: network_edges });

  const node_ids = network_nodes.map((node) => node.id);
  expect(first.painted, "network paints edges and nodes").toBeGreaterThan(200);
  expect(Object.keys(first.positions).sort()).toEqual([...node_ids].sort());

  const max_shift = (a, b) => Math.max(...node_ids.map((id) => Math.hypot(a[id].x - b[id].x, a[id].y - b[id].y)));
  const settle = async () => {
    await expect.poll(async () => {
      const before = await page.evaluate(() => window.__network.positions());
      await page.waitForTimeout(400);
      const after = await page.evaluate(() => window.__network.positions());
      return Object.keys(before).length === node_ids.length && Object.keys(after).length === node_ids.length
        ? max_shift(before, after) : Infinity;
    }, { timeout: 20_000, intervals: [100] }).toBeLessThan(0.5);
    return page.evaluate(() => window.__network.positions());
  };
  const settled = await settle();
  expect(max_shift(first.positions, settled), "the force simulation moves nodes from their seed").toBeGreaterThan(2);

  // A real pointer drag moves the node and pins it where it is released.
  const start = settled.jpm;
  await page.mouse.move(start.x, start.y);
  await page.mouse.down();
  for (let step = 1; step <= 8; step += 1) await page.mouse.move(start.x + step * 6, start.y + step * 4);
  await page.mouse.up();
  const dragged = await settle();
  expect(Math.hypot(dragged.jpm.x - start.x, dragged.jpm.y - start.y)).toBeGreaterThan(30);
  expect(Math.hypot(dragged.jpm.x - (start.x + 48), dragged.jpm.y - (start.y + 32))).toBeLessThan(6);
  const pinned = await page.evaluate(() => window.__network.series.options().pinned.map((pin) => pin.id));
  expect(pinned).toEqual(["jpm"]);

  // The wheel zooms the view around the pointer without touching the simulation.
  await page.mouse.move((dragged.jpm.x + dragged.lch.x) / 2, (dragged.jpm.y + dragged.lch.y) / 2);
  await page.mouse.wheel(0, -240);
  await expect.poll(() => page.evaluate(() => window.__network.series.options().network_view.zoom))
    .toBeGreaterThan(1);
  const zoomed = await settle();
  const spread = (p) => Math.hypot(p.jpm.x - p.lch.x, p.jpm.y - p.lch.y);
  expect(spread(zoomed)).toBeGreaterThan(spread(dragged) * 1.1);

  // Settled node positions persist and a restore reuses them directly: the restored chart
  // answers the same geometry immediately, with no simulation warm-up, and re-exports the
  // identical document.
  const restored = await page.evaluate(async () => {
    const { chart } = window.__network;
    const state = JSON.stringify(chart.export_state());
    const host = document.createElement("div");
    host.style.cssText = "position:fixed;left:0;top:0;width:420px;height:320px";
    document.body.append(host);
    const api = await import("/dist/aeris_charts_financial.js");
    const target = await api.create_chart(host, {
      backend: "canvas2d",
      autoSize: false,
      theme: "light",
      initialPane: { horizontal_domain: { type: "layout" } },
    });
    target.resize(420, 320, 1);
    target.import_state(state);
    target.render();
    const sums = {};
    for (let y = 0; y < 320; y += 2) {
      for (let x = 0; x < 420; x += 2) {
        const hit = target.layout_hit_test(0, x, y);
        if (hit === null || hit.element !== "node") continue;
        const sum = sums[hit.id] ?? (sums[hit.id] = { x: 0, y: 0, n: 0 });
        sum.x += x;
        sum.y += y;
        sum.n += 1;
      }
    }
    const same = JSON.stringify(target.export_state()) === state;
    target.remove();
    host.remove();
    return {
      positions: Object.fromEntries(Object.entries(sums).map(([id, s]) => [id, { x: s.x / s.n, y: s.y / s.n }])),
      same,
    };
  });
  expect(restored.same, "restored network re-exports the identical document").toBe(true);
  for (const id of node_ids) {
    expect(
      Math.hypot(restored.positions[id].x - zoomed[id].x, restored.positions[id].y - zoomed[id].y),
      `${id} restores its settled position`,
    ).toBeLessThan(2);
  }

  const disposed = await page.evaluate(() => {    const { chart } = window.__network;
    chart.remove();
    return true;
  });
  expect(disposed).toBe(true);
});

// Two-set-plus-overlap and three-set fixtures are exercised in the engine; the browser checks
// one three-set diagram and one word cloud through the real host path.
const venn_sets = {
  type: "sets",
  sets: [
    { sets: ["equities"], size: 18 },
    { sets: ["rates"], size: 14 },
    { sets: ["credit"], size: 12 },
    { sets: ["equities", "rates"], size: 6 },
    { sets: ["equities", "credit"], size: 4 },
    { sets: ["rates", "credit"], size: 5 },
    { sets: ["equities", "rates", "credit"], size: 3 },
  ],
};

const cloud_words = {
  type: "words",
  words: [
    { text: "liquidity", weight: 40 },
    { text: "volatility", weight: 34 },
    { text: "spread", weight: 28 },
    { text: "gamma", weight: 22 },
    { text: "basis", weight: 18, color: "#3ba272" },
    { text: "coupon", weight: 15 },
    { text: "duration", weight: 13 },
    { text: "convexity", weight: 11 },
    { text: "carry", weight: 9 },
    { text: "hedge", weight: 7 },
  ],
};

for (const backend of ["canvas2d", "webgpu"]) test(`venn and word cloud layouts render and answer hit tests through ${backend}`, async ({ page }) => {
  await blank_page(page);
  const report = await page.evaluate(async ({ backend, venn_sets, cloud_words, series_color }) => {
    const api = await import("/dist/aeris_charts_financial.js");
    const scenario = async (kind, data, options) => {
      const host = document.createElement("div");
      host.style.cssText = "position:relative;width:420px;height:320px";
      document.body.append(host);
      const chart = await api.create_chart(host, {
        backend,
        __force_webgpu_fallback_adapter: true,
        autoSize: false,
        theme: "light",
        initialPane: { horizontal_domain: { type: "layout" } },
      });
      if (chart.backend() !== backend) throw new Error(`expected ${backend}, got ${chart.backend()}`);
      chart.resize(420, 320, 1);
      const series = chart.add_layout_series(kind, { pane: 0, title: kind, color: series_color, ...options }, data);
      chart.render();
      const canvas = chart.take_screenshot();
      const image = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
      const key = (offset) => `${image[offset]},${image[offset + 1]},${image[offset + 2]}`;
      const corner = key(0);
      let painted = 0;
      for (let offset = 0; offset < image.length; offset += 16) {
        if (key(offset) !== corner) painted += 1;
      }
      const hits = {};
      for (let y = 0; y < 320; y += 2) {
        for (let x = 0; x < 420; x += 2) {
          const hit = chart.layout_hit_test(0, x, y);
          if (hit !== null && hits[hit.id] === undefined) hits[hit.id] = hit;
        }
      }
      const snapshot = series.accessibility_snapshot(0, 512);
      const series_options = series.options();
      chart.remove();
      host.remove();
      return { painted, hits, snapshot, series_options };
    };
    return {
      venn: await scenario("venn", venn_sets, {}),
      cloud: await scenario("word_cloud", cloud_words, { word_cloud_rotate: true }),
    };
  }, { backend, venn_sets, cloud_words, series_color });

  expect(report.venn.painted, "venn paints circles").toBeGreaterThan(200);
  const region_ids = venn_sets.sets.map((region) => region.sets.join("∩"));
  expect(Object.keys(report.venn.hits).sort(), "every exclusive region answers").toEqual([...region_ids].sort());
  for (const hit of Object.values(report.venn.hits)) expect(hit.element).toBe("region");
  expect(report.venn.hits["equities∩rates"]).toMatchObject({ label: "equities ∩ rates", value: 6 });
  expect(report.venn.hits.equities).toMatchObject({ label: "equities", value: 18 });
  expect(report.venn.snapshot.total).toBe(7);
  expect(report.venn.snapshot.items[0]).toMatchObject({ element: "region", id: "equities", label: "equities", value: 18 });
  expect(report.venn.snapshot.items.map((item) => item.id)).toEqual(region_ids);

  expect(report.cloud.painted, "word cloud paints words").toBeGreaterThan(200);
  expect(report.cloud.series_options.word_cloud_rotate).toBe(true);
  expect(report.cloud.series_options.word_cloud_scaling).toBe("sqrt");
  const cloud_hits = Object.values(report.cloud.hits);
  expect(cloud_hits.length, "placed words answer hits").toBeGreaterThan(3);
  for (const hit of cloud_hits) {
    expect(hit.element).toBe("word");
    expect(hit.id).toBe(hit.label);
  }
  expect(report.cloud.hits.liquidity).toMatchObject({ value: 40 });
  expect(report.cloud.snapshot.total).toBe(10);
  // Placement state agrees with the hits: every hit word reports placed, and no omitted word
  // answers a hit.
  const placed = report.cloud.snapshot.items.filter((item) => item.placed === true).map((item) => item.id);
  expect(placed.length).toBe(cloud_hits.length);
  for (const item of report.cloud.snapshot.items) {
    if (item.placed === false) expect(report.cloud.hits[item.id], item.id).toBeUndefined();
  }
});

test("venn and word cloud data validate atomically and survive persistence", async ({ page }) => {
  await blank_page(page);
  const result = await page.evaluate(async ({ venn_sets, cloud_words }) => {
    const api = await import("/dist/aeris_charts_financial.js");
    const host = document.createElement("div");
    host.style.cssText = "position:relative;width:420px;height:320px";
    document.body.append(host);
    const options = {
      backend: "canvas2d", autoSize: false, theme: "light",
      initialPane: { horizontal_domain: { type: "layout" } },
    };
    const chart = await api.create_chart(host, options);
    const code = (action) => {
      try {
        action();
        return "ok";
      } catch (error) {
        return error.code;
      }
    };
    const duplicate_region = code(() => chart.add_layout_series("venn", { pane: 0 }, {
      type: "sets", sets: [{ sets: ["a"], size: 1 }, { sets: ["a"], size: 2 }],
    }));
    const missing_singleton = code(() => chart.add_layout_series("venn", { pane: 0 }, {
      type: "sets", sets: [{ sets: ["a"], size: 1 }, { sets: ["a", "b"], size: 1 }],
    }));
    const negative_size = code(() => chart.add_layout_series("venn", { pane: 0 }, {
      type: "sets", sets: [{ sets: ["a"], size: -1 }],
    }));
    const empty_word = code(() => chart.add_layout_series("word_cloud", { pane: 0 }, {
      type: "words", words: [{ text: "  ", weight: 1 }],
    }));
    const bad_word_color = code(() => chart.add_layout_series("word_cloud", { pane: 0 }, {
      type: "words", words: [{ text: "a", weight: 1, color: "not-a-color" }],
    }));
    const wrong_data = code(() => chart.add_layout_series("venn", { pane: 0 }, cloud_words));
    const scoped_options = code(() => chart.add_layout_series("venn", { pane: 0, word_cloud_rotate: true }, venn_sets));
    const bad_bounds = code(() => chart.add_layout_series("word_cloud", { pane: 0, word_cloud_min_size: 100, word_cloud_max_size: 10 }, cloud_words));
    const count_after_errors = chart.layout_series().length;
    chart.add_layout_series("venn", { pane: 0, title: "Coverage" }, venn_sets);
    chart.add_layout_series("word_cloud", { pane: 0, title: "Chatter", word_cloud_min_size: 8, word_cloud_max_size: 30, word_cloud_scaling: "linear" }, cloud_words);
    const state = JSON.stringify(chart.export_state());
    const restored_host = document.createElement("div");
    restored_host.style.cssText = "position:relative;width:420px;height:320px";
    document.body.append(restored_host);
    const restored = await api.create_chart(restored_host, options);
    restored.import_state(state);
    const restored_state = JSON.stringify(restored.export_state());
    const kinds = restored.layout_series().map((series) => series.kind);
    const word_options = restored.layout_series()[1].options();
    chart.remove();
    restored.remove();
    host.remove();
    restored_host.remove();
    return {
      duplicate_region, missing_singleton, negative_size, empty_word, bad_word_color,
      wrong_data, scoped_options, bad_bounds, count_after_errors,
      same: state === restored_state, kinds,
      word_options: {
        min: word_options.word_cloud_min_size,
        max: word_options.word_cloud_max_size,
        scaling: word_options.word_cloud_scaling,
      },
    };
  }, { venn_sets, cloud_words });
  expect(result).toEqual({
    duplicate_region: "invalid_data",
    missing_singleton: "invalid_data",
    negative_size: "invalid_data",
    empty_word: "invalid_data",
    bad_word_color: "invalid_data",
    wrong_data: "invalid_data",
    scoped_options: "invalid_options",
    bad_bounds: "invalid_options",
    count_after_errors: 0,
    same: true,
    kinds: ["venn", "word_cloud"],
    word_options: { min: 8, max: 30, scaling: "linear" },
  });
});
