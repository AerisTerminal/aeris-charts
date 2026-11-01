// Bullet graph. ECharts 6.1.0 has no bullet series; the reference is one custom series whose
// renderItem paints three qualitative bands, a comparative bar, the measure bar and a target
// marker per category, in the proportions of Stephen Few's bullet graph specification.
export const bullets = [
  { x: "Revenue", y: 82, target: 90, comparative: 74, bands: [60, 85, 100] },
  { x: "Profit", y: 64, target: 70, comparative: 58, bands: [50, 75, 100] },
  { x: "Orders", y: 91, target: 85, comparative: 80, bands: [55, 80, 100] },
  { x: "Retention", y: 72, target: 80, comparative: 76, bands: [65, 85, 100] },
];
export const measure_color = "#4c8bf5";
const band_colors = ["#b3b3b3", "#cccccc", "#e3e3e3"];

const render = (params, api) => {
  const row = api.value(0);
  const center = api.coord([0, row])[1];
  const thickness = api.size([0, 1])[1] * 0.8;
  const x = (value) => api.coord([value, row])[0];
  const rect = (from, to, fraction, fill) => ({
    type: "rect",
    shape: { x: x(from), y: center - thickness * fraction / 2, width: x(to) - x(from), height: thickness * fraction },
    style: { fill },
  });
  const bands = [api.value(4), api.value(5), api.value(6)];
  let previous = 0;
  const children = bands.map((threshold, band) => {
    const child = rect(previous, threshold, 1, band_colors[band]);
    previous = threshold;
    return child;
  });
  children.push(rect(0, api.value(3), 1 / 2, "rgba(76, 139, 245, 0.5)"));
  children.push(rect(0, api.value(1), 1 / 3, measure_color));
  const target = x(api.value(2));
  children.push({
    type: "rect",
    shape: { x: target - 1, y: center - thickness / 3, width: 2, height: thickness * 2 / 3 },
    style: { fill: "#222222" },
  });
  return { type: "group", children };
};

export default {
  id: "bullet",
  title: "Bullet",
  charts: [{
    id: "bullet",
    title: "custom series bands, comparative, measure and target",
    width: 520,
    height: 300,
    option: {
      grid: { left: 72, right: 16, top: 16, bottom: 28 },
      xAxis: { type: "value", min: 0, max: 100 },
      yAxis: { type: "category", data: bullets.map((row) => row.x), inverse: true },
      series: [{
        type: "custom", name: "Attainment %", renderItem: render,
        encode: { x: [1, 2, 3, 4, 5, 6], y: 0 },
        data: bullets.map((row, index) => [index, row.y, row.target, row.comparative, ...row.bands]),
      }],
    },
  }],
};
