// Calendar heatmap over the leap year 2024 with Monday-first weeks. ECharts 6.1.0 pins the
// `calendar` coordinate system, a `heatmap` series on it and a continuous `visualMap` legend.
const DAY = 86_400_000;
const JAN_1_2024 = 1_704_067_200_000;
const iso = (time) => new Date(time).toISOString().slice(0, 10);

export const week_start = 1;
export const days = Array.from({ length: 366 }, (_, index) => ({
  date: JAN_1_2024 + index * DAY,
  value: Math.round(40 + 30 * Math.sin(index / 9) + ((index * 37) % 23)),
})).filter((_, index) => index % 11 !== 5);
export const probes = ["2024-01-01", "2024-01-07", "2024-02-29", "2024-03-02", "2024-07-04", "2024-12-31"];
const values = days.map((day) => day.value);

export default {
  id: "calendar_heatmap",
  title: "Calendar heatmap",
  charts: [{
    id: "calendar_heatmap",
    title: "calendar coordinate system, heatmap series and continuous visualMap",
    width: 760,
    height: 220,
    option: {
      visualMap: {
        type: "continuous", min: Math.min(...values), max: Math.max(...values), calculable: false,
        orient: "horizontal", left: "center", bottom: 4, inRange: { color: ["#d8f2e8", "#3fbe95"] },
      },
      calendar: {
        range: "2024", top: 36, left: 44, right: 16, cellSize: ["auto", 16],
        dayLabel: { firstDay: week_start, nameMap: "en" }, monthLabel: { nameMap: "en" },
        yearLabel: { show: true },
      },
      series: [{
        type: "heatmap", name: "Daily commits", coordinateSystem: "calendar",
        data: days.map((day) => [iso(day.date), day.value]),
      }],
    },
  }],
};
