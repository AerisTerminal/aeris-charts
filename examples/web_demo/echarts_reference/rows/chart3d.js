// 3D bar, scatter, line and surface charts through the pinned ECharts GL 2.1.0 extension on
// ECharts 6.1.0 (`grid3D` with its default `viewControl`: alpha 20, beta 40, distance 200,
// perspective). Aeris ports the grid3D box (100 units per side, data `y` from near to far) and
// the OrbitControl camera, so a data point projects to the same normalized screen offset in
// both: `g14_3d.spec.mjs` fits the Aeris scatter symbol centers to the ECharts GL camera
// projection of the same points. ECharts GL renders through WebGL with lambert shading and a
// depth buffer; Aeris shades and depth-sorts triangles in the engine, so the colors and edges
// are compared by eye, not pixel by pixel.
export const series_color = "#5470c6";

export const quarters = ["Q1", "Q2", "Q3", "Q4"];
export const regions = ["North", "South", "West"];
export const bars = quarters.flatMap((quarter, x) => regions.map((region, y) => ({
  x, y, z: [[4, 7, 5], [6, 9, 3], [8, 5, 6], [10, 6, 4]][x][y], label: `${quarter} ${region}`,
})));

// Value extents chosen so both nice-tick algorithms land on [0, 100], [0, 50] and [0, 10].
export const scatter = [
  { x: 0, y: 0, z: 0, label: "zero" },
  { x: 100, y: 50, z: 10, label: "far top" },
  { x: 20, y: 40, z: 8, label: "A" },
  { x: 80, y: 10, z: 2, label: "B" },
  { x: 50, y: 25, z: 5, label: "center" },
  { x: 90, y: 45, z: 1, label: "C" },
  { x: 10, y: 5, z: 9, label: "D" },
];
export const scatter_size = 12;
// ECharts GL adds one unit to grid3D's `orthographicSize`, so 149 there is 150 in Aeris.
export const orthographic = { echarts_size: 149, aeris_size: 150, alpha: 35, beta: -60 };

export const helix = Array.from({ length: 41 }, (_, index) => {
  const t = index / 40 * 4 * Math.PI;
  return { x: Math.cos(t), y: Math.sin(t), z: t };
});

// ECharts GL samples `equation` on `x` and `y` from `min` through `max` by `step`; Aeris takes
// the same grid as positions and row-major heights.
export const surface_range = { min: -1, max: 1, step: 0.125 };
export const surface_height = (x, y) => Math.sin(x * Math.PI) * Math.cos(y * Math.PI);
const axis = Array.from({ length: Math.round((surface_range.max - surface_range.min) / surface_range.step) + 1 },
  (_, index) => surface_range.min + index * surface_range.step);
export const surface = {
  x: axis,
  y: axis,
  z: axis.flatMap((y) => axis.map((x) => surface_height(x, y))),
  colors: ["#313695", "#4575b4", "#74add1", "#abd9e9", "#e0f3f8", "#fee090", "#fdae61", "#f46d43", "#d73027"],
};

const size = { width: 420, height: 320 };

export default {
  id: "chart3d",
  title: "3D bar, scatter, line and surface (ECharts GL 2.1.0)",
  charts: [
    {
      id: "bar_3d",
      title: "bar3D on category axes",
      ...size,
      option: {
        grid3D: {},
        xAxis3D: { type: "category", data: quarters },
        yAxis3D: { type: "category", data: regions },
        zAxis3D: { type: "value" },
        series: [{
          type: "bar3D",
          shading: "lambert",
          itemStyle: { color: series_color },
          data: bars.map((bar) => ({ name: bar.label, value: [bar.x, bar.y, bar.z] })),
        }],
      },
    },
    {
      id: "scatter_3d",
      title: "scatter3D, perspective camera",
      ...size,
      option: {
        grid3D: {},
        xAxis3D: { type: "value" },
        yAxis3D: { type: "value" },
        zAxis3D: { type: "value" },
        series: [{
          type: "scatter3D",
          symbolSize: scatter_size,
          itemStyle: { color: series_color },
          data: scatter.map((point) => ({ name: point.label, value: [point.x, point.y, point.z] })),
        }],
      },
    },
    {
      id: "scatter_3d_orthographic",
      title: "scatter3D, orthographic camera",
      ...size,
      option: {
        grid3D: {
          viewControl: {
            projection: "orthographic",
            orthographicSize: orthographic.echarts_size,
            alpha: orthographic.alpha,
            beta: orthographic.beta,
          },
        },
        xAxis3D: { type: "value" },
        yAxis3D: { type: "value" },
        zAxis3D: { type: "value" },
        series: [{
          type: "scatter3D",
          symbolSize: scatter_size,
          itemStyle: { color: series_color },
          data: scatter.map((point) => ({ name: point.label, value: [point.x, point.y, point.z] })),
        }],
      },
    },
    {
      id: "line_3d",
      title: "line3D helix",
      ...size,
      option: {
        grid3D: {},
        xAxis3D: { type: "value" },
        yAxis3D: { type: "value" },
        zAxis3D: { type: "value" },
        series: [{
          type: "line3D",
          lineStyle: { width: 2, color: series_color },
          data: helix.map((point) => [point.x, point.y, point.z]),
        }],
      },
    },
    {
      id: "surface_3d",
      title: "surface from an equation with a color map",
      ...size,
      option: {
        grid3D: {},
        xAxis3D: { type: "value" },
        yAxis3D: { type: "value" },
        zAxis3D: { type: "value" },
        visualMap: { show: false, dimension: 2, min: -1, max: 1, inRange: { color: surface.colors } },
        series: [{
          type: "surface",
          wireframe: { show: true },
          equation: {
            x: { ...surface_range },
            y: { ...surface_range },
            z: surface_height,
          },
        }],
      },
    },
  ],
};
