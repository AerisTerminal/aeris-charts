// Pinned Apache ECharts 6.1.0 comparison fixture for chart families that Recharts 3.10.1 does
// not provide. Each comparison row lives in its own module under `rows/` and is registered in
// `rows.js`. A row exports `{ id, title, maps?, charts: [{ id, title, option, width?, height? }] }`;
// `maps` holds the GeoJSON its `map` series register through `echarts.registerMap`.
// `?case=<row id>` selects a row; `w`/`h` override every panel size. The page sets
// `window.__echarts_reference_ready` once every chart has finished its first render, and exposes
// each instance by chart id in `window.__echarts_instances` for `convertToPixel` geometry probes;
// `window.__echarts_gl_project` does the same for ECharts GL grid3D charts.
import * as echarts from "echarts";
// Registers the grid3D component and the bar3D, scatter3D, line3D and surface series.
import "echarts-gl";
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

// Projects a data triple through an ECharts GL chart's grid3D camera. Returns the clip-space
// result as an offset from the viewport center in viewport heights (x right, y down), which
// is independent of the panel size, plus the viewport size in CSS px.
window.__echarts_gl_project = (chart_id, data) => {
  const coordinate_system = window.__echarts_instances[chart_id].getModel().getComponent("grid3D").coordinateSystem;
  const { camera, viewport } = coordinate_system.viewGL;
  camera.update(true);
  const [x, y, z] = coordinate_system.dataToPoint(data);
  const multiply = (m, [a, b, c, d]) => [0, 1, 2, 3].map((row) =>
    m[row] * a + m[4 + row] * b + m[8 + row] * c + m[12 + row] * d);
  const clip = multiply(camera.projectionMatrix.array, multiply(camera.viewMatrix.array, [x, y, z, 1]));
  return {
    x: (clip[0] / clip[3]) * viewport.width / (2 * viewport.height),
    y: -(clip[1] / clip[3]) / 2,
    width: viewport.width,
    height: viewport.height,
  };
};
