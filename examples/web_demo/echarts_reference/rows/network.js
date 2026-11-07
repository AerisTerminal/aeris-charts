// Network: an undirected-looking counterparty graph with the pinned ECharts 6.1.0 `graph`
// series in its `force` and `circular` layouts. ECharts runs its own force model (repulsion,
// gravity, edgeLength); Aeris follows d3-force 3.0, so force positions are compared for overall
// structure, not coordinates.
export const nodes = [
  { id: "jpm", label: "JPM", category: "bank", value: 40 },
  { id: "gs", label: "GS", category: "bank", value: 32 },
  { id: "ms", label: "MS", category: "bank", value: 28 },
  { id: "citi", label: "Citi", category: "bank", value: 24 },
  { id: "bx", label: "Blackstone", category: "fund", value: 18 },
  { id: "brk", label: "Berkshire", category: "fund", value: 22 },
  { id: "vg", label: "Vanguard", category: "fund", value: 30 },
  { id: "cme", label: "CME", category: "venue", value: 26 },
  { id: "ice", label: "ICE", category: "venue", value: 16 },
  { id: "lch", label: "LCH", category: "venue", value: 14 },
  { id: "pimco", label: "PIMCO", category: "fund", value: 12 },
  { id: "bofa", label: "BofA", category: "bank", value: 26 },
];
export const edges = [
  ["jpm", "gs"], ["jpm", "ms"], ["jpm", "cme"], ["gs", "ms"], ["gs", "bx"], ["ms", "vg"],
  ["citi", "jpm"], ["citi", "ice"], ["bofa", "citi"], ["bofa", "lch"], ["brk", "bofa"],
  ["vg", "pimco"], ["cme", "lch"], ["ice", "lch"], ["bx", "pimco"], ["gs", "cme"],
].map(([source, target]) => ({ source, target }));

const categories = ["bank", "fund", "venue"];

const series = (layout) => ({
  type: "graph",
  layout,
  roam: true,
  draggable: layout === "force",
  categories: categories.map((name) => ({ name })),
  label: { show: true, position: "right" },
  force: { edgeLength: 30, repulsion: 30, gravity: 0.1 },
  data: nodes.map((node) => ({
    name: node.id,
    value: node.value,
    category: categories.indexOf(node.category),
    symbolSize: 8 + 20 * Math.sqrt(node.value / 40),
    label: { formatter: node.label },
  })),
  links: edges,
});

export default {
  id: "network",
  title: "Network",
  charts: [
    {
      id: "network_force",
      title: "graph series, force layout",
      width: 360,
      height: 300,
      option: { animation: false, series: [series("force")] },
    },
    {
      id: "network_circular",
      title: "graph series, circular layout",
      width: 360,
      height: 300,
      option: { animation: false, series: [series("circular")] },
    },
  ],
};
