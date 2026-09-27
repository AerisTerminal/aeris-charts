import type {
  chart_api,
  feature_brush_style,
  series_api,
} from "./types.js";
import { set_native_area_brush_state } from "./impl.js";
import { create_delta_tooltip } from "./primitive_features.js";

/**
 * Optional overrides for the brushable area. Every unset field keeps the engine default, which uses
 * the same fill strength as ordinary area and baseline series.
 */
export interface brushable_area_interaction_options {
  base_style?: Partial<feature_brush_style>;
  /** Overrides for the de-emphasized area outside the selection. */
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
  // The engine owns every default: the canonical area-fill strength, the brand up/down hues for
  // selected ranges, and the series' own stroke faded outside the selection. Only a host's explicit
  // overrides travel with the brush state.
  const outside: Partial<feature_brush_style> = { ...options.base_style, ...options.faded_style };
  const positive: Partial<feature_brush_style> = { ...options.selected_style, ...options.positive_style };
  const negative: Partial<feature_brush_style> = { ...options.selected_style, ...options.negative_style };
  const tooltip = create_delta_tooltip(chart, {
    series,
    on_active_range_change(range) {
      if (range === null) {
        set_native_area_brush_state(series, "null");
        return;
      }
      set_native_area_brush_state(series, JSON.stringify({
        outside,
        ranges: [{
          range: { from: range.from, to: range.to },
          tone: range.positive ? "positive" : "negative",
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
