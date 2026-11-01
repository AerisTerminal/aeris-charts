// Gantt schedule on a time axis. ECharts 6.1.0 has no Gantt series; its documented recipe is a
// `custom` series whose `renderItem` paints one bar per task on a category row, which is what
// this row pins. Group headers are empty category rows, milestones are diamonds, dependencies
// are finish-to-start orthogonal polylines, and the current-time line is a dashed `markLine`.
const DAY = 86_400_000;
const JAN_1_2026 = 1_767_225_600_000;
const day = (offset) => JAN_1_2026 + offset * DAY;

export const now = day(20);
export const tasks = [
  { id: "scope", label: "Scope", group: "Plan", start: day(4), end: day(8), progress: 1 },
  { id: "design", label: "Design", group: "Plan", start: day(8), end: day(15), progress: 0.6, dependencies: ["scope"] },
  { id: "signoff", label: "Sign-off", group: "Plan", start: day(15), end: day(15), dependencies: ["design"] },
  { id: "backend", label: "Backend", group: "Build", start: day(15), end: day(29), progress: 0.3, dependencies: ["signoff"] },
  { id: "frontend", label: "Frontend", group: "Build", start: day(18), end: day(32), progress: 0.1, dependencies: ["signoff"] },
  { id: "qa", label: "QA", group: "Build", start: day(32), end: day(39), dependencies: ["backend", "frontend"] },
  { id: "launch", label: "Launch", start: day(40), end: day(40), dependencies: ["qa"] },
];

// One category row per group header and per task, in input order.
export const rows = [];
const task_row = new Map();
let previous_group;
for (const task of tasks) {
  if (task.group !== undefined && task.group !== previous_group) rows.push({ header: task.group });
  previous_group = task.group;
  task_row.set(task.id, rows.length);
  rows.push({ task: task.id, label: task.label });
}
export const task_rows = Object.fromEntries(task_row);

const domain = [Math.min(...tasks.map((task) => task.start)), Math.max(...tasks.map((task) => task.end))];
const COLOR = "#5470c6";
const BAR = 18;

const bar = (params, api) => {
  const row = api.value(0);
  const start = api.coord([api.value(1), row]);
  const end = api.coord([api.value(2), row]);
  if (api.value(1) === api.value(2)) {
    const half = BAR / 2;
    return { type: "polygon", shape: { points: [
      [start[0], start[1] - half], [start[0] + half, start[1]], [start[0], start[1] + half], [start[0] - half, start[1]],
    ] }, style: { fill: COLOR } };
  }
  const width = end[0] - start[0];
  const y = start[1] - BAR / 2;
  return { type: "group", children: [
    { type: "rect", shape: { x: start[0], y, width, height: BAR }, style: { fill: "rgba(84,112,198,0.35)" } },
    { type: "rect", shape: { x: start[0], y, width: width * api.value(3), height: BAR }, style: { fill: COLOR } },
  ] };
};

const by_id = new Map(tasks.map((task) => [task.id, task]));
// Data: [predecessor end, its row, successor start, its row, predecessor milestone, successor
// milestone]; a milestone end point sits on its diamond's tip.
const link = (params, api) => {
  const from = api.coord([api.value(0), api.value(1)]);
  const to = api.coord([api.value(2), api.value(3)]);
  from[0] += api.value(4) * BAR / 2;
  to[0] -= api.value(5) * BAR / 2;
  const stub = from[0] + 8;
  const tip = to[0] - 6;
  const points = tip >= stub
    ? [from, [stub, from[1]], [stub, to[1]], [tip, to[1]]]
    : [from, [stub, from[1]], [stub, from[1] + 14], [to[0] - 8, from[1] + 14], [to[0] - 8, to[1]], [tip, to[1]]];
  return { type: "group", children: [
    { type: "polyline", shape: { points }, style: { stroke: "rgba(51,51,51,0.6)", fill: null, lineWidth: 1 } },
    { type: "polygon", shape: { points: [to, [tip, to[1] - 4], [tip, to[1] + 4]] }, style: { fill: "rgba(51,51,51,0.6)" } },
  ] };
};
const milestone = (task) => (task.start === task.end ? 1 : 0);

export default {
  id: "gantt",
  title: "Gantt schedule",
  charts: [{
    id: "gantt",
    title: "custom series bars, milestones, dependencies and a now markLine",
    width: 760,
    height: 320,
    option: {
      grid: { left: 96, right: 24, top: 32, bottom: 12 },
      xAxis: { type: "time", position: "top", min: domain[0], max: domain[1], splitLine: { show: true } },
      yAxis: {
        type: "category", inverse: true, axisTick: { show: false },
        data: rows.map((row) => row.header ?? row.label),
      },
      series: [
        {
          type: "custom", name: "Release plan", renderItem: bar, encode: { x: [1, 2], y: 0 },
          data: tasks.map((task) => [task_row.get(task.id), task.start, task.end, task.progress ?? 0]),
          markLine: { symbol: "none", silent: true, label: { show: false },
            lineStyle: { type: "dashed", color: "#EE6666", width: 1 }, data: [{ xAxis: now }] },
        },
        {
          type: "custom", name: "Dependencies", silent: true, renderItem: link,
          data: tasks.flatMap((task) => (task.dependencies ?? []).map((id) => {
            const from = by_id.get(id);
            return [from.end, task_row.get(id), task.start, task_row.get(task.id), milestone(from), milestone(task)];
          })),
        },
      ],
    },
  }],
};
