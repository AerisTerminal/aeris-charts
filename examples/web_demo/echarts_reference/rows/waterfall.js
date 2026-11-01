// Waterfall. ECharts 6.1.0 has no waterfall series; the documented recipe stacks a transparent
// placeholder bar under each visible step bar. Relative steps add to the running total; subtotal
// and total bars start at zero.
export const steps = [
  { x: "Revenue", y: 420 }, { x: "COGS", y: -170 }, { x: "Gross", y: null, step: "subtotal" },
  { x: "Opex", y: -120 }, { x: "Other", y: 24 }, { x: "Tax", y: -38 }, { x: "Net", y: null, step: "total" },
];
export const colors = { increase: "#089981", decrease: "#f23645", subtotal: "#7e57c2", total: "#5470c6" };

export const bars = [];
let running = 0;
for (const step of steps) {
  if (step.step === "subtotal" || step.step === "total") {
    bars.push({ x: step.x, start: 0, end: running, mark: step.step });
  } else {
    const start = running;
    running += step.y;
    bars.push({ x: step.x, start, end: running, mark: step.y >= 0 ? "increase" : "decrease" });
  }
}

const placeholder = {
  type: "bar", stack: "waterfall", silent: true,
  itemStyle: { color: "transparent", borderColor: "transparent" },
  data: bars.map((bar) => Math.min(bar.start, bar.end)),
};
const visible = {
  type: "bar", stack: "waterfall", name: "Profit bridge",
  data: bars.map((bar) => ({ value: Math.abs(bar.end - bar.start), itemStyle: { color: colors[bar.mark] } })),
};
const categories = bars.map((bar) => bar.x);

export default {
  id: "waterfall",
  title: "Waterfall",
  charts: [
    {
      id: "waterfall",
      title: "stacked placeholder recipe, vertical",
      width: 520,
      height: 300,
      option: {
        grid: { left: 48, right: 16, top: 16, bottom: 28 },
        xAxis: { type: "category", data: categories },
        yAxis: { type: "value" },
        series: [placeholder, { ...visible, label: { show: true, position: "top" } }],
      },
    },
    {
      id: "waterfall_horizontal",
      title: "stacked placeholder recipe, horizontal",
      width: 520,
      height: 300,
      option: {
        grid: { left: 72, right: 16, top: 16, bottom: 28 },
        xAxis: { type: "value" },
        yAxis: { type: "category", data: categories, inverse: true },
        series: [placeholder, { ...visible, label: { show: true, position: "right" } }],
      },
    },
  ],
};
