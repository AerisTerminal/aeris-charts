// Pinned Apache ECharts 6.1.0 comparison fixture for chart families that Recharts 3.10.1 does
// not provide. Each comparison row lives in its own module under `rows/` and is registered in
// `rows.js`. A row exports `{ id, title, maps?, charts: [{ id, title, option, width?, height? }] }`;
// `maps` holds the GeoJSON its `map` series register through `echarts.registerMap`.
// `?case=<row id>` selects a row; `w`/`h` override every panel size. The page sets
// `window.__echarts_reference_ready` once every chart has finished its first render, and exposes
// each instance by chart id in `window.__echarts_instances` for `convertToPixel` geometry probes.
import * as echarts from "echarts";
import { rows } from "./rows.js";

const params = new URLSearchParams(location.search);
const size = (key, fallback) => {
  const value = Number(params.get(key));
  return Number.isFinite(value) && value >= 80 && value <= 1600 ? value : fallback;
};
const root = document.getElementById("root");
const selected = rows.find((row) => row.id === params.get("case")) ?? rows[0];

const nav = document.createElement("nav");
for (const row of rows) {
  const link = document.createElement("a");
  link.href = `?case=${row.id}`;
  link.textContent = row.title;
  if (row === selected) link.setAttribute("aria-current", "page");
  nav.appendChild(link);
}
root.appendChild(nav);

const pending = [];
window.__echarts_instances = {};
if (selected !== undefined) {
  for (const [name, geojson] of Object.entries(selected.maps ?? {})) echarts.registerMap(name, geojson);
  const section = document.createElement("section");
  section.dataset.case = selected.id;
  const heading = document.createElement("h2");
  heading.textContent = selected.title;
  const charts = document.createElement("div");
  charts.className = "charts";
  section.append(heading, charts);
  root.appendChild(section);
  for (const chart of selected.charts) {
    const figure = document.createElement("figure");
    const host = document.createElement("div");
    host.dataset.chart = chart.id;
    const width = size("w", chart.width ?? 350);
    const height = size("h", chart.height ?? 185);
    host.style.cssText = `width:${width}px;height:${height}px`;
    const caption = document.createElement("figcaption");
    caption.textContent = chart.title;
    figure.append(host, caption);
    charts.appendChild(figure);
    const instance = echarts.init(host, null, { renderer: "canvas", width, height });
    window.__echarts_instances[chart.id] = instance;
    pending.push(new Promise((resolve) => instance.on("finished", resolve)));
    instance.setOption({ animation: false, ...chart.option });
  }
}
Promise.all(pending).then(() => { window.__echarts_reference_ready = true; });
