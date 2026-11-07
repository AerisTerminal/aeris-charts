// Dendrogram: the tree row's hierarchy drawn top-down. ECharts 6.1.0 has no cluster layout, so the
// reference is its `tree` series with `orient: "TB"` and polyline edges; Aeris uses the
// d3-hierarchy 3.1.2 `cluster()` algorithm, which aligns every leaf on the deepest level, and
// draws orthogonal elbow links.
import { nested, nodes, series_color } from "./tree.js";

export default {
  id: "dendrogram",
  title: "Dendrogram",
  charts: [{
    id: "dendrogram",
    title: "tree series, top-to-bottom with polyline edges",
    width: 360,
    height: 300,
    option: {
      series: [{
        type: "tree",
        data: [nested(nodes)],
        orient: "TB",
        edgeShape: "polyline",
        initialTreeDepth: -1,
        itemStyle: { borderColor: series_color },
      }],
    },
  }],
};
