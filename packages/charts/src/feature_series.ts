import type {
  chart_api,
  feature_brush_style,
  feature_series_options,
  series_api,
} from "./types.js";

export interface brushable_area_interaction_options {
  base_style?: Partial<feature_brush_style>;
  faded_style?: Partial<feature_brush_style>;
  selected_style?: Partial<feature_brush_style>;
}

/**
 * Install the reference brush gesture for a Rust-native `brushable_area` series. Pointer capture
 * stays at the browser host boundary; the selected logical range and all resulting geometry stay
 * in the engine through `apply_options`.
 */
export function enable_brushable_area_interaction(
  chart: chart_api,
  series: series_api,
  options: brushable_area_interaction_options = {},
): { detach(): void } {
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
  const selected: feature_brush_style = {
    ...base,
    line_color: "rgb(4,153,129)",
    top_color: "rgba(4,153,129,0.4)",
    bottom_color: "rgba(4,153,129,0)",
    line_width: 3,
    ...options.selected_style,
  };
  const host = chart.chart_element();
  let pointer_id: number | null = null;
  let start: number | null = null;
  let active = false;

  const logical_at = (client_x: number): number | null => {
    const pane = chart.panes().find((candidate) => candidate.get_series().includes(series));
    if (pane === undefined) return null;
    const bounds = host.getBoundingClientRect();
    const geometry = pane.get_geometry();
    const x = client_x - bounds.left - geometry.left;
    if (x < 0 || x > geometry.width) return null;
    return chart.time_scale().coordinate_to_logical(x);
  };
  const apply = (patch: Partial<feature_series_options>): void => series.apply_options(patch);
  const claim = (event: PointerEvent): void => {
    event.preventDefault();
    event.stopImmediatePropagation();
  };
  const on_down = (event: PointerEvent): void => {
    if (event.button !== 0 || pointer_id !== null) return;
    const logical = logical_at(event.clientX);
    if (logical === null) return;
    claim(event);
    pointer_id = event.pointerId;
    start = logical;
    active = false;
    host.setPointerCapture(event.pointerId);
    apply({ ...base, brush_ranges: [] });
  };
  const on_move = (event: PointerEvent): void => {
    if (event.pointerId !== pointer_id || start === null) return;
    claim(event);
    const end = logical_at(event.clientX);
    if (end === null || end === start) return;
    active = true;
    apply({
      ...faded,
      brush_ranges: [{ range: { from: Math.min(start, end), to: Math.max(start, end) }, style: selected }],
    });
  };
  const finish = (event: PointerEvent): void => {
    if (event.pointerId !== pointer_id) return;
    claim(event);
    if (host.hasPointerCapture(event.pointerId)) host.releasePointerCapture(event.pointerId);
    pointer_id = null;
    start = null;
    if (!active) apply({ ...base, brush_ranges: [] });
    active = false;
  };
  host.addEventListener("pointerdown", on_down, true);
  host.addEventListener("pointermove", on_move, true);
  host.addEventListener("pointerup", finish, true);
  host.addEventListener("pointercancel", finish, true);
  return {
    detach() {
      host.removeEventListener("pointerdown", on_down, true);
      host.removeEventListener("pointermove", on_move, true);
      host.removeEventListener("pointerup", finish, true);
      host.removeEventListener("pointercancel", finish, true);
      pointer_id = null;
      start = null;
    },
  };
}
