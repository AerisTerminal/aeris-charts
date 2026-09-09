import type {
  chart_api,
  chart_options,
  feature_brush_style,
  feature_series_options,
  series_api,
} from "./types.js";
import { LINE_STYLE_TO_U8 } from "./types.js";
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
 * Compose the official delta-tooltip gesture with a Rust-native `brushable_area` series. Pointer
 * lookup, chronological delta direction, touch state, and tooltip geometry stay in the engine;
 * this host adapter applies the selected range's positive/negative style and reuses the chart's
 * native vertical crosshair as the single solid brush guide.
 */
export function enable_brushable_area_interaction(
  chart: chart_api,
  series: series_api,
  options: brushable_area_interaction_options = {},
): brushable_area_interaction_handle {
  if (series.series_type() !== "brushable_area") {
    throw new Error("enable_brushable_area_interaction requires a brushable_area series");
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
  const previous_options = chart.options() as chart_options;
  const previous_scroll = previous_options.handle_scroll;
  const previous_scale = previous_options.handle_scale;
  const previous_vertical_crosshair_style = previous_options.crosshair.vertLine.style;
  chart.apply_options({ handle_scroll: false, handle_scale: false });
  chart.apply_options({
    crosshair: {
      vertLine: { style: LINE_STYLE_TO_U8.solid },
    },
  });
  const tooltip = create_delta_tooltip(chart, {
    series,
    on_active_range_change(range) {
      if (range === null) {
        series.apply_options({ ...base, brush_ranges: [] });
        return;
      }
      series.apply_options({
        ...faded,
        brush_ranges: [{
          range: { from: range.from, to: range.to },
          style: range.positive ? positive : negative,
        }],
      } satisfies Partial<feature_series_options>);
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
      chart.apply_options({
        handle_scroll: previous_scroll,
        handle_scale: previous_scale,
        crosshair: {
          vertLine: { style: previous_vertical_crosshair_style },
        },
      });
    },
  };
}
