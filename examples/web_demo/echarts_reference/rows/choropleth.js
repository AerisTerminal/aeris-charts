// Choropleth: six regions filled by value through a continuous two-color ramp, using the pinned
// ECharts 6.1.0 `map` series over a map registered with `echarts.registerMap` and a continuous
// `visualMap`. Aeris projects the same GeoJSON with d3-geo 3 `geoEqualEarth` fitted to the plot;
// ECharts draws longitude/latitude linearly with its default 0.75 aspect scale, so shapes match
// by topology and color order, not by pixel position. The "Isles" feature has no value and takes
// the missing-value color in both libraries.
const square = (x0, y0, x1, y1) => [[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]];
const feature = (id, name, type, coordinates) => ({
  type: "Feature", id, properties: { name }, geometry: { type, coordinates },
});
export const geojson = {
  type: "FeatureCollection",
  features: [
    feature("west", "West", "Polygon", [square(-10, 40, 0, 55)]),
    feature("central", "Central", "Polygon", [square(0, 40, 10, 55), square(3, 45, 7, 49).reverse()]),
    feature("east", "East", "Polygon", [square(10, 40, 22, 55)]),
    feature("north", "North", "Polygon", [square(-10, 55, 22, 66)]),
    feature("south", "South", "Polygon", [square(-10, 32, 22, 40)]),
    feature("isles", "Isles", "MultiPolygon", [[square(24, 58, 27, 61)], [square(25, 48, 28, 51)]]),
  ],
};
export const values = { west: 2.5, central: 6.0, east: 9.5, north: 4.0, south: -1.5 };
export const series_color = "#5070dd";
export const low_color = "#dbe3f8";

const names = Object.fromEntries(geojson.features.map((item) => [item.id, item.properties.name]));
const finite = Object.values(values);

export default {
  id: "choropleth",
  title: "Choropleth",
  maps: { regions: geojson },
  charts: [{
    id: "choropleth",
    title: "map series over registerMap GeoJSON with a continuous visualMap",
    width: 420,
    height: 320,
    option: {
      visualMap: {
        type: "continuous",
        min: Math.min(...finite),
        max: Math.max(...finite),
        inRange: { color: [low_color, series_color] },
        calculable: false,
      },
      series: [{
        type: "map",
        map: "regions",
        roam: false,
        itemStyle: { areaColor: "#c8c8c8", borderColor: "#ffffff" },
        data: Object.entries(values).map(([id, value]) => ({ name: names[id], value })),
      }],
    },
  }],
};
