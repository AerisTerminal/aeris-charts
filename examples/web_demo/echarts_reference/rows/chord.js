// Chord: a directed trade-flow graph with the pinned ECharts 6.1.0 `chord` series defaults
// (70%-80% radius ring, 3° pads, clockwise from 12 o'clock, ribbons in the source color at 20%
// opacity, outside labels).
export const nodes = [
  { id: "us", label: "US" }, { id: "eu", label: "EU" }, { id: "cn", label: "China" },
  { id: "jp", label: "Japan" }, { id: "mx", label: "Mexico" },
];
export const edges = [
  { source: "us", target: "eu", value: 32 }, { source: "eu", target: "us", value: 28 },
  { source: "cn", target: "us", value: 41 }, { source: "us", target: "cn", value: 15 },
  { source: "cn", target: "eu", value: 30 }, { source: "eu", target: "cn", value: 22 },
  { source: "jp", target: "us", value: 14 }, { source: "cn", target: "jp", value: 17 },
  { source: "mx", target: "us", value: 26 }, { source: "us", target: "mx", value: 21 },
];

export default {
  id: "chord",
  title: "Chord",
  charts: [{
    id: "chord",
    title: "chord series over a directed graph",
    width: 360,
    height: 300,
    option: {
      series: [{
        type: "chord",
        data: nodes.map((node) => ({ name: node.id, label: { formatter: node.label } })),
        links: edges.map((edge) => ({ source: edge.source, target: edge.target, value: edge.value })),
      }],
    },
  }],
};
