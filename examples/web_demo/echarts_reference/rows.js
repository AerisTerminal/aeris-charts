// Registry of ECharts comparison rows. Add one import per row module and list it below.
import bullet from "./rows/bullet.js";
import calendar_heatmap from "./rows/calendar_heatmap.js";
import chart3d from "./rows/chart3d.js";
import choropleth from "./rows/choropleth.js";
import chord from "./rows/chord.js";
import dendrogram from "./rows/dendrogram.js";
import gantt from "./rows/gantt.js";
import gauge from "./rows/gauge.js";
import geo_marks from "./rows/geo_marks.js";
import lollipop_dumbbell from "./rows/lollipop_dumbbell.js";
import marimekko from "./rows/marimekko.js";
import network from "./rows/network.js";
import slope_bump from "./rows/slope_bump.js";
import tree from "./rows/tree.js";
import waterfall from "./rows/waterfall.js";

export const rows = [
  gantt, calendar_heatmap, marimekko, waterfall, bullet, lollipop_dumbbell, slope_bump, gauge, chord,
  tree, dendrogram, network, choropleth, geo_marks, chart3d,
];
