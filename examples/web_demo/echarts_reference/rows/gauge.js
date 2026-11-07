// Gauge: one value on a 0-100 dial with three colored bands and a progress arc, using the pinned
// ECharts 6.1.0 `gauge` series defaults (75% radius, 225° to -45° track, ten splits). The pointer
// and progress take the series color so both libraries paint the same items.
export const gauge = {
  value: 62.5,
  min: 0,
  max: 100,
  label: "CPU load",
  unit: "%",
  progress: true,
  bands: [
    { to: 30, color: "#67e0e3", label: "Low" },
    { to: 70, color: "#37a2da", label: "Normal" },
    { to: 100, color: "#fd666d", label: "High" },
  ],
};
export const series_color = "#5070dd";

export default {
  id: "gauge",
  title: "Gauge",
  charts: [{
    id: "gauge",
    title: "gauge series with axisLine color stops, progress and a value detail",
    width: 360,
    height: 300,
    option: {
      series: [{
        type: "gauge",
        min: gauge.min,
        max: gauge.max,
        progress: { show: gauge.progress, itemStyle: { color: series_color } },
        pointer: { itemStyle: { color: series_color } },
        axisLine: {
          lineStyle: {
            color: gauge.bands.map((band) => [(band.to - gauge.min) / (gauge.max - gauge.min), band.color]),
          },
        },
        detail: { formatter: `{value}${gauge.unit}` },
        data: [{ value: gauge.value, name: gauge.label }],
      }],
    },
  }],
};
