// Bubble and flow maps over the choropleth row's regions, using the pinned ECharts 6.1.0 `geo`
// component with a `scatter` series (value-sized symbols) and a `lines` series (curved flows
// with arrowheads). Aeris anchors marks to feature centroids or longitude/latitude positions,
// projects them through the base map's d3-geo projection and draws flows as great-circle arcs;
// ECharts draws longitude/latitude linearly with its 0.75 aspect scale and bends lines by
// `curveness`, so marks match by anchor order, size order and arrow direction, not by pixel
// position.
import { geojson, series_color } from "./choropleth.js";

export const points = [
  { id: "west", at: "west", value: 40 },
  { id: "central", at: "central", value: 120 },
  { id: "east", at: "east", value: 75 },
  { id: "north", at: "north", value: 20 },
  { id: "port", label: "Port", at: [5, 43], value: 55 },
];
export const links = [
  { id: "w-e", source: "west", target: "east", value: 80 },
  { id: "n-c", source: "north", target: "central", value: 35 },
  { id: "port", source: [5, 43], target: "isles", source_label: "Port", value: 50 },
];
export const mark_color = "#ee6666";

// ECharts takes coordinates, so each feature anchor resolves to the vertex mean of its first
// outer ring, close to the spherical centroid Aeris uses for these small regions.
const anchor = (at) => {
  if (Array.isArray(at)) return at;
  const geometry = geojson.features.find((feature) => feature.id === at).geometry;
  const ring = (geometry.type === "Polygon" ? geometry.coordinates : geometry.coordinates[0])[0].slice(0, -1);
  return [0, 1].map((axis) => ring.reduce((sum, point) => sum + point[axis], 0) / ring.length);
};
const names = Object.fromEntries(geojson.features.map((item) => [item.id, item.properties.name]));
const geo = { map: "regions", roam: false, itemStyle: { areaColor: "#c8c8c8", borderColor: "#ffffff" } };
const largest = Math.max(...points.map((point) => point.value));
const widest = Math.max(...links.map((link) => link.value));

export default {
  id: "geo_marks",
  title: "Bubble and flow maps",
  maps: { regions: geojson },
  charts: [
    {
      id: "bubble_map",
      title: "geo + scatter with square-root value-sized symbols",
      width: 420,
      height: 320,
      option: {
        geo,
        series: [{
          type: "scatter",
          coordinateSystem: "geo",
          itemStyle: { color: mark_color, opacity: 0.75 },
          symbolSize: (value) => 2 * (3 + 21 * Math.sqrt(value[2] / largest)),
          data: points.map((point) => ({
            name: point.label ?? names[point.at] ?? point.id,
            value: [...anchor(point.at), point.value],
          })),
        }],
      },
    },
    {
      id: "flow_map",
      title: "geo + lines with linear widths and target arrows",
      width: 420,
      height: 320,
      option: {
        geo,
        series: [{
          type: "lines",
          coordinateSystem: "geo",
          symbol: ["none", "arrow"],
          lineStyle: { color: series_color, opacity: 0.75, curveness: 0.2 },
          data: links.map((link) => ({
            coords: [anchor(link.source), anchor(link.target)],
            lineStyle: { width: 1 + 5 * (link.value / widest) },
          })),
        }],
      },
    },
  ],
};
