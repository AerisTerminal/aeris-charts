import type {
  chart_api,
  feature_brush_style,
  series_api,
} from "./types.js";
import { set_native_area_brush_state } from "./impl.js";
import { create_delta_tooltip } from "./primitive_features.js";

export interface brushable_area_interaction_options {
  base_style?: Partial<feature_brush_style>;
  faded_style?: Partial<feature_brush_style>;
  /** Shared overrides for both selected states. */
  selected_style?: Partial<feature_brush_style>;
  positive_style?: Partial<feature_brush_style>;
  negative_style?: Partial<feature_brush_style>;
}

export interface brushable_area_interaction_handle {
  active_range(): import("./primitive_features.js").delta_tooltip_active_range | null;
  clear(): void;
  detach(): void;
}

/**
 * Compose Delta Tooltip range selection with an ordinary Area series. While attached, primary
 * pane-drag belongs to the comparison brush instead of canvas pan; axis gestures remain ordinary.
 * The brush is transient presentation state, so the Area series keeps its normal data, hit
 * testing, scales, LOD, and ingestion behavior.
 */
export function enable_brushable_area_interaction(
  chart: chart_api,
  series: series_api,
  options: brushable_area_interaction_options = {},
): brushable_area_interaction_handle {
  if (series.series_type() !== "area") {
    throw new Error("enable_brushable_area_interaction requires an area series");
  }
  const base: feature_brush_style = {
    line_color: "rgb(40,98,255)",
    top_color: "rgba(40,98,255,0.4)",
    bottom_color: "rgba(40,98,255,0)",
    line_width: 2,
    ...options.base_style,
  };
  const faded: feature_brush_style = {
    ...base,
    line_color: "rgba(40,98,255,0.2)",
    top_color: "rgba(40,98,255,0.05)",
    ...options.faded_style,
  };
  const positive: feature_brush_style = {
    ...base,
    line_color: "rgb(4,153,129)",
    top_color: "rgba(4,153,129,0.4)",
    bottom_color: "rgba(4,153,129,0)",
    line_width: 3,
    ...options.selected_style,
    ...options.positive_style,
  };
  const negative: feature_brush_style = {
    ...base,
    line_color: "rgb(239,83,80)",
    top_color: "rgba(239,83,80,0.4)",
    bottom_color: "rgba(239,83,80,0)",
    line_width: 3,
    ...options.selected_style,
    ...options.negative_style,
  };
  const tooltip = create_delta_tooltip(chart, {
    series,
    on_active_range_change(range) {
      if (range === null) {
        set_native_area_brush_state(series, "null");
        return;
      }
      set_native_area_brush_state(series, JSON.stringify({
        outside: faded,
        ranges: [{
          range: { from: range.from, to: range.to },
          style: range.positive ? positive : negative,
        }],
      }));
    },
  });
  let detached = false;
  const clear = (): void => {
    if (detached) return;
    tooltip.clear();
  };
  const on_dbl_click = (): void => clear();
  const on_keydown = (event: KeyboardEvent): void => {
    if (event.key === "Escape" && tooltip.active_range() !== null) clear();
  };
  chart.subscribe_dbl_click(on_dbl_click);
  chart.chart_element().addEventListener("keydown", on_keydown, { capture: true });
  return {
    active_range: tooltip.active_range,
    clear,
    detach() {
      if (detached) return;
      detached = true;
      chart.unsubscribe_dbl_click(on_dbl_click);
      chart.chart_element().removeEventListener("keydown", on_keydown, { capture: true });
      tooltip.clear();
      tooltip.detach();
      set_native_area_brush_state(series, "null");
    },
  };
}
