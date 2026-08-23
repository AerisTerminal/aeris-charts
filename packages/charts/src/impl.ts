/**
 * Handle implementations over the wasm engine: series, time scale, price scale, pane, chart.
 * Extracted from `index.ts`.
 */

// @ts-ignore -- pkg is a build artifact, present after build:wasm
import init, { NucleusChart } from "../pkg/nucleuscharts_wasm.js";

import { install_gestures } from "./gestures.js";
import type { pane_primitive, pane_primitive_handle, series_primitive, series_primitive_handle } from "./primitives.js";
import type { canvas_primitive, canvas_primitive_handle, canvas_pane_view } from "./canvas_plugins.js";
import { create_canvas_render_target } from "./canvas_plugins.js";
import type { custom_series_item, custom_series_pane_view } from "./custom_series.js";
import {
  enable_accessibility,
  type accessibility_handle,
  type accessibility_options,
} from "./accessibility.js";
import { nucleuscharts_error } from "./errors.js";
import type { nucleuscharts_error_code } from "./errors.js";
import type {
  any_series_options, backend_status, bars_info, chart_api, chart_options, chart_state_v1, data_changed_handler, dbl_click_handler,
  deep_partial, drawing_api, drawing_created_handler, drawing_info, drawing_kind, drawing_options,
  drawing_point, drawing_tool_change_handler,
  feature_series_kind, frame_stats,
  ingestion_diagnostics,
  handle_scale_options, handle_scroll_options, indicator_info, kinetic_scroll_options,
  last_value_data, localization_options, logical_range,
  mismatch_direction, mouse_event_handler, mouse_event_params, ohlc_columns, ohlc_data, options_change_handler, pane_api, pane_geometry, price_line_api, price_line_options,
  persistence_restore_result, price_range, price_scale_api, price_scale_create_options,
  price_scale_info, price_scale_options, ring_source_layout,
  series_api, series_change_handler, series_data, series_kind,
  series_marker, series_marker_options, series_options, single_value_data, size_change_handler, time, time_range,
  time_scale_api, time_scale_options, tracking_mode_options, trading_api, trading_confirmation_mode, trading_execution, trading_hit,
  trading_intent, trading_intent_handler, trading_position, trading_preview, trading_snapshot,
  trading_style_options, instrument_metadata, working_order,
  visible_logical_range_handler, visible_time_range_handler,
} from "./types.js";
import {
  DRAWING_KIND_TO_U8, FEATURE_KIND_TO_U8, KIND_TO_U8, LINE_STYLE_TO_U8, LINE_TYPE_TO_U8,
  is_feature_series_kind,
} from "./types.js";
import { default_theme_name, theme_palette } from "./theme.js";

// ---------------------------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------------------------

let init_promise: Promise<unknown> | null = null;

type persistence_error_result = {
  ok: false;
  error: { code: nucleuscharts_error_code; message: string };
};

function throw_persistence_error(result: persistence_error_result): never {
  throw new nucleuscharts_error(result.error.code, result.error.message);
}
/**
 * Instantiate the wasm module once per page. `wasm_url` overrides the default asset resolution
 * (`new URL("nucleuscharts_wasm_bg.wasm", import.meta.url)` beside the bundle) — the escape hatch for
 * bundlers that relocate the JS away from the .wasm (e.g. Vite's dev pre-bundler). Only the
 * first call's argument takes effect.
 */
export function ensure_init(wasm_url?: string | URL): Promise<unknown> {
  if (init_promise === null) {
    const initializing = wasm_url !== undefined ? init(wasm_url) : init();
    init_promise = initializing;
    return initializing;
  }
  return init_promise;
}

function undef_to_null<T>(v: T | undefined): T | null {
  return v === undefined ? null : v;
}

function same_logical_range(a: logical_range | null, b: logical_range | null): boolean {
  return a === b || (a !== null && b !== null && a.from === b.from && a.to === b.to);
}

function same_time_range(a: time_range | null, b: time_range | null): boolean {
  return a === b || (a !== null && b !== null && a.from === b.from && a.to === b.to);
}

/** Duration of the animated `scroll_to_position` ease (matches the reference smooth-scroll feel). */
const SCROLL_ANIM_MS = 300;

/**
 * Whether the candle-close countdown timer should run: any series with `countdown_visible`
 * and data. Factored pure so the start/stop logic is testable without a chart.
 */
function countdown_timer_needed(
  series: readonly { countdown_visible?: boolean; has_data: boolean }[],
): boolean {
  return series.some((s) => s.countdown_visible === true && s.has_data);
}

/**
 * Series style keys without a dedicated wasm setter, forwarded to `series_apply_options_json`
 * as one snake_case JSON patch (the engine ignores unknown keys).
 */
const SERIES_JSON_OPTION_KEYS = [
  "last_value_visible",
  "title",
  "title_visible",
  "countdown_visible",
  "price_line_visible",
  "price_line_source",
  "price_line_width",
  "price_line_color",
  "price_line_style",
  "bid_ask_visible",
  "bid_color",
  "ask_color",
  "bid_ask_line_width",
  "bid_ask_line_style",
  "line_style",
  "line_visible",
  "point_markers_radius",
  "crosshair_marker_visible",
  "crosshair_marker_radius",
  "crosshair_marker_border_color",
  "crosshair_marker_background_color",
  "crosshair_marker_border_width",
  "top_fill_color1",
  "top_fill_color2",
  "top_line_color",
  "top_line_width",
  "top_line_style",
  "bottom_fill_color1",
  "bottom_fill_color2",
  "bottom_line_color",
  "bottom_line_width",
  "bottom_line_style",
  "base",
  "invert_filled_area",
  "open_visible",
  "thin_bars",
] as const;

/**
 * Price-scale style keys without a dedicated wasm setter, forwarded to
 * `price_scale_apply_options_json` as one snake_case JSON patch (the engine ignores unknown keys).
 */
const PRICE_SCALE_JSON_OPTION_KEYS = [
  "align_labels",
  "ticks_visible",
  "entire_text_only",
  "minimum_width",
  "text_color",
  "bold_round_labels",
  "visible",
] as const;

/** Engine kind ordinal → public kind name (index-aligned with `KIND_TO_U8`). */
const KIND_NAMES = ["candlestick", "bar", "line", "area", "histogram", "baseline", "custom"] as const;
const FEATURE_KIND_NAMES = [
  "brushable_area",
  "dual_range_histogram",
  "grouped_bars",
  "heatmap",
  "hlc_area",
  "pretty_histogram",
  "rounded_candles",
  "background_shade",
  "stacked_area",
  "stacked_bars",
  "whisker_box",
] as const satisfies readonly feature_series_kind[];

/**
 * Slot layout of the `frame_stats_into` f64 buffer. Must match `crate::telemetry::slot` in
 * `nucleuscharts_wasm` exactly — append only, never reorder (the engine and the package version
 * together, but a stale bundle against a newer .wasm must still read the same slots).
 */
const FRAME_STATS_SLOT = {
  cpu_ms: 0,
  gpu_ms: 1,
  draw_calls: 2,
  dropped_frames: 3,
  presented_frames: 4,
  memory_bytes: 5,
  canvas2d_ops: 6,
  ring_overruns: 7,
  gpu_buffer_allocations: 8,
  gpu_write_calls: 9,
  gpu_uploaded_bytes: 10,
  layout_rebuilds: 11,
  autoscale_runs: 12,
  series_rebuilds: 13,
  drawing_rebuilds: 14,
  grid_rebuilds: 15,
  overlay_rebuilds: 16,
  axis_rebuilds: 17,
  text_resolutions: 18,
  trading_rebuilds: 19,
} as const;

/**
 * Convert a `time` input to the engine's UTC-seconds form. Business days and `"YYYY-MM-DD"` strings
 * are taken at UTC midnight (matching the reference's `Date.UTC(...)/1000`). A malformed value yields `NaN`,
 * which the engine's sanitizer drops as an invalid row.
 */
export function time_to_utc_seconds(t: time): number {
  if (typeof t === "number") return t;
  if (typeof t === "string") {
    const [y, m, d] = t.split("-").map(Number);
    return Date.UTC(y ?? NaN, (m ?? 1) - 1, d ?? 1) / 1000;
  }
  return Date.UTC(t.year, t.month - 1, t.day) / 1000;
}

function pack(data: readonly series_data[]): {
  times: Float64Array;
  open: Float64Array;
  high: Float64Array;
  low: Float64Array;
  close: Float64Array;
  body_colors?: Uint32Array;
  wick_colors?: Uint32Array;
  border_colors?: Uint32Array;
} {
  const n = data.length;
  const times = new Float64Array(n);
  const open = new Float64Array(n);
  const high = new Float64Array(n);
  const low = new Float64Array(n);
  const close = new Float64Array(n);
  // Per-point color channels (reference `color`/`wickColor`/`borderColor` on data items). A channel is
  // only allocated when at least one item carries the field; rows without it pad as 0, which the
  // engine treats as "no override" within a passed channel.
  let body_colors: Uint32Array | undefined;
  let wick_colors: Uint32Array | undefined;
  let border_colors: Uint32Array | undefined;
  for (let i = 0; i < n; i++) {
    const d = data[i] as series_data;
    times[i] = time_to_utc_seconds(d.time);
    if ("value" in d) {
      open[i] = high[i] = low[i] = close[i] = d.value;
      body_colors = pack_color_channel(body_colors, n, i, "color" in d ? d.color : undefined);
    } else if ("open" in d) {
      open[i] = d.open;
      high[i] = d.high;
      low[i] = d.low;
      close[i] = d.close;
      body_colors = pack_color_channel(body_colors, n, i, "color" in d ? d.color : undefined);
      wick_colors = pack_color_channel(wick_colors, n, i, "wick_color" in d ? d.wick_color : undefined);
      border_colors = pack_color_channel(border_colors, n, i, "border_color" in d ? d.border_color : undefined);
    } else {
      // Whitespace (reference `WhitespaceData`): an explicit empty slot, packed all-NaN. The engine
      // keeps the row as whitespace instead of dropping it.
      open[i] = high[i] = low[i] = close[i] = NaN;
    }
  }
  return { times, open, high, low, close, body_colors, wick_colors, border_colors };
}

/** Parse a CSS hex or rgb()/rgba() color to 8-bit RGBA channels (mirrors the Rust `Color::parse_css`). */
function parse_rgba(css: string): [number, number, number, number] | null {
  const s = css.trim();
  if (s.startsWith("#")) {
    const h = s.slice(1);
    const expand = (c: string) => parseInt(c + c, 16);
    if (h.length === 3 || h.length === 4) {
      return [expand(h[0]!), expand(h[1]!), expand(h[2]!), h.length === 4 ? expand(h[3]!) : 255];
    }
    if (h.length === 6 || h.length === 8) {
      return [
        parseInt(h.slice(0, 2), 16),
        parseInt(h.slice(2, 4), 16),
        parseInt(h.slice(4, 6), 16),
        h.length === 8 ? parseInt(h.slice(6, 8), 16) : 255,
      ];
    }
    return null;
  }
  const m = s.match(/^rgba?\(([^)]+)\)$/i);
  if (m) {
    const parts = m[1]!.split(",").map((p) => parseFloat(p.trim()));
    if (parts.length >= 3 && parts.every((p) => !Number.isNaN(p))) {
      const alpha = parts.length >= 4 ? Math.round(parts[3]! * 255) : 255;
      return [Math.round(parts[0]!), Math.round(parts[1]!), Math.round(parts[2]!), alpha];
    }
  }
  return null;
}

/** Parse a CSS hex or rgb()/rgba() color to 8-bit channels (mirrors the Rust `Color::parse_css`). */
function parse_rgb(css: string): [number, number, number] | null {
  const rgba = parse_rgba(css);
  return rgba === null || rgba.slice(0, 3).some(Number.isNaN) ? null : [rgba[0], rgba[1], rgba[2]];
}

/**
 * Parse a CSS color to the engine's packed per-point color word, 0xRRGGBBAA (alpha preserved).
 * Returns `null` for unparseable input; callers warn and skip that item's color, matching how
 * the engine sanitizer warns on bad data.
 */
function parse_css_to_u32(css: string): number | null {
  const rgba = parse_rgba(css);
  if (rgba === null || rgba.some(Number.isNaN)) return null;
  return ((rgba[0] << 24) | (rgba[1] << 16) | (rgba[2] << 8) | rgba[3]) >>> 0;
}

/**
 * Fold one data item's optional per-point color field into a channel array, allocating it lazily
 * on first use. The engine treats a channel as present only when the array is passed, and every
 * row of a passed channel as a custom color — so rows without the field pad as 0, which the
 * engine reads as "no override" (0x00000000 would otherwise be a valid transparent-black).
 * Unparseable colors warn and pad as 0, matching the engine sanitizer's warn-and-skip.
 */
function pack_color_channel(
  channel: Uint32Array | undefined,
  n: number,
  i: number,
  css: string | undefined,
): Uint32Array | undefined {
  const packed = point_color_to_u32(css);
  if (packed === undefined) return channel;
  const out = channel ?? new Uint32Array(n);
  out[i] = packed;
  return out;
}

/** Parse an optional per-point color field to 0xRRGGBBAA; `undefined` = no custom color. */
function point_color_to_u32(css: string | undefined): number | undefined {
  if (css === undefined) return undefined;
  const packed = parse_css_to_u32(css);
  if (packed === null) {
    console.warn(`nucleuscharts: ignoring unparseable data point color "${css}"`);
    return undefined;
  }
  return packed;
}

class series_impl implements series_api {
  protected readonly data_changed_subs = new Set<data_changed_handler>();
  private removed = false;
  private last_ingestion: ingestion_diagnostics | null = null;

  constructor(
    readonly id: number,
    readonly kind: series_kind,
    protected readonly chart: chart_impl,
  ) {}

  /** Called by `chart_impl.remove_series`; makes every subsequent method on this handle throw. */
  mark_removed(): void {
    this.removed = true;
    this.data_changed_subs.clear();
  }

  /**
   * Fire this series' `data_changed` after a ring drain delivered rows to it. Scope is `"update"`:
   * a drain appends or replace-lasts, exactly like `update`/`update_typed`. Called by the chart's
   * drain loop, which already owns the repaint, so this only notifies.
   */
  emit_ring_data_changed(): void {
    if (this.removed) return;
    for (const handler of this.data_changed_subs) handler("update");
  }
  protected assert_live(): void {
    void this.chart.wasm;
    if (this.removed) throw new nucleuscharts_error("stale_handle", "this series has been removed from the chart");
  }

  set_data(data: readonly series_data[]): void {
    this.assert_live();
    const p = pack(data);
    this.record_ingestion(this.chart.wasm.set_series_data_typed(this.id, p.times, p.open, p.high, p.low, p.close));
    // set_series_data resets point colors, so per-point channels must be applied after it.
    if (p.body_colors !== undefined || p.wick_colors !== undefined || p.border_colors !== undefined) {
      this.chart.wasm.set_series_point_colors(this.id, p.body_colors, p.wick_colors, p.border_colors);
    }
    this.chart.sync_countdown_timer();
    this.chart.repaint();
    for (const handler of this.data_changed_subs) handler("full");
  }

  /**
   * Columnar fast path: already-packed typed arrays go straight to the engine, skipping
   * the per-object JS packing of `set_data`, which is avoidable work for feed handlers that
   * already hold columnar data. `times` are UTC seconds
   * (the engine's time unit, see `time_to_utc_seconds`); single-value series repeat
   * their value across all four price channels. Per-point colors are not carried —
   * apply them after with the usual options/colors path. The engine's sort/dedupe/
   * sanitize rules apply exactly as with `set_data`.
   */
  set_data_typed(columns: ohlc_columns): void {
    this.assert_live();
    this.record_ingestion(this.chart.wasm.set_series_data_typed(
      this.id, columns.times, columns.open, columns.high, columns.low, columns.close,
    ));
    this.chart.sync_countdown_timer();
    this.chart.repaint();
    for (const handler of this.data_changed_subs) handler("full");
  }

  /**
   * Columnar streaming fast path: the batch goes straight to the engine as five typed arrays,
   * so a feed running at tens of thousands of ticks per second allocates no per-point JS object
   * at the boundary. One `data_changed("update")` per call regardless of batch size — the
   * notification describes the call, not the rows, exactly as it does for `update`.
   */
  update_typed(columns: ohlc_columns): void {
    this.assert_live();
    this.record_ingestion(this.chart.wasm.update_series_bars_typed(
      this.id, columns.times, columns.open, columns.high, columns.low, columns.close,
    ));
    // Same post-update bookkeeping as `update`: data arriving on a countdown-enabled series can
    // start the timer, and repaints coalesce onto the next frame rather than painting per batch.
    if (this.chart.countdown_series_present) this.chart.sync_countdown_timer();
    this.chart.schedule_repaint();
    for (const handler of this.data_changed_subs) handler("update");
  }

  /**
   * Bind (or with `null`, unbind) a `SharedArrayBuffer` ring the engine drains on its own frame
   * tick. The two typed-array views are built here, once, and handed to the engine — it holds them
   * for the lifetime of the binding so no view is constructed per frame.
   */
  set_ring_source(buffer: SharedArrayBuffer | null, layout?: ring_source_layout): void {
    this.assert_live();
    if (buffer === null) {
      this.chart.wasm.clear_ring_source(this.id);
      this.chart.sync_ring_drain_loop();
      return;
    }
    if (layout === undefined) {
      throw new nucleuscharts_error("invalid_options", "set_ring_source requires a layout when a buffer is given");
    }
    // A plain ArrayBuffer would work for the reads but defeats the point (the producer is a worker),
    // and `Atomics.load` on non-shared memory is a footgun rather than an error. Reject it here
    // where the message can say why.
    if (typeof SharedArrayBuffer !== "undefined" && !(buffer instanceof SharedArrayBuffer)) {
      throw new nucleuscharts_error(
        "invalid_data",
        "set_ring_source expects a SharedArrayBuffer (is the page cross-origin isolated?)",
      );
    }
    const reason = this.chart.wasm.set_ring_source(
      this.id, new Uint8Array(buffer), new Int32Array(buffer), JSON.stringify(layout),
    );
    if (reason !== "") throw new nucleuscharts_error("invalid_options", `set_ring_source rejected — ${reason}`);
    this.chart.sync_ring_drain_loop();
  }

  update(point: series_data): void {
    this.assert_live();
    // A whitespace point (`{time}` only) streams as an all-NaN bar; the engine keeps the slot.
    const o = "value" in point ? point.value : "open" in point ? point.open : NaN;
    const h = "value" in point ? point.value : "high" in point ? point.high : NaN;
    const l = "value" in point ? point.value : "low" in point ? point.low : NaN;
    const c = "value" in point ? point.value : "close" in point ? point.close : NaN;
    // undefined = no custom color; on a replace of the last bar this also clears a previously
    // set custom color for that channel. Whitespace points carry no color channels.
    const body = "color" in point ? point_color_to_u32(point.color) : undefined;
    const wick = "wick_color" in point ? point_color_to_u32(point.wick_color) : undefined;
    const border = "border_color" in point ? point_color_to_u32(point.border_color) : undefined;
    // Series-scoped streaming: append a new time point or replace the last on this series.
    const time = time_to_utc_seconds(point.time);
    this.record_single_ingestion(time, [o, h, l, c]);
    this.chart.wasm.update_series_bar_styled(
      this.id, time, o, h, l, c, body, wick, border,
    );
    // Data arriving on a countdown-enabled series can start the timer (cheap flag check).
    if (this.chart.countdown_series_present) this.chart.sync_countdown_timer();
    this.chart.schedule_repaint();
    for (const handler of this.data_changed_subs) handler("update");
  }

  last_ingestion_diagnostics(): ingestion_diagnostics | null {
    this.assert_live();
    return this.last_ingestion;
  }

  protected record_ingestion(json: string | undefined): void {
    this.last_ingestion = json === undefined ? null : JSON.parse(json) as ingestion_diagnostics;
  }

  private record_single_ingestion(time: number, values: [number, number, number, number]): void {
    const whitespace = values.every(Number.isNaN);
    const non_finite = !Number.isFinite(time) || (!whitespace && values.some((value) => !Number.isFinite(value)));
    const limit = Number.MAX_SAFE_INTEGER / 100;
    const out_of_range = !non_finite
      && !whitespace
      && values.some((value) => Math.abs(value) > limit);
    const [open, high, low, close] = values;
    const semantic = !whitespace && !non_finite && !out_of_range
      && (high < low || high < open || high < close || low > open || low > close);
    if (!non_finite && !out_of_range && !semantic) {
      this.last_ingestion = null;
      return;
    }
    this.last_ingestion = {
      status: non_finite || out_of_range ? "rejected" : "accepted_with_diagnostics",
      accepted: semantic ? 1 : 0,
      dropped_invalid: non_finite || out_of_range ? 1 : 0,
      dropped_non_finite: non_finite ? 1 : 0,
      dropped_out_of_range: out_of_range ? 1 : 0,
      deduplicated: 0,
      reordered: false,
      semantic_anomalies: semantic ? 1 : 0,
    };
  }

  pop(count = 1): void {
    this.assert_live();
    this.chart.wasm.series_pop(this.id, count);
    this.chart.repaint();
    // Like set_data, popping is a full-range change, not an incremental update.
    for (const handler of this.data_changed_subs) handler("full");
  }

  last_value_data(global_last = false): last_value_data | null {
    this.assert_live();
    const json = this.chart.wasm.series_last_value_data(this.id, global_last);
    return json === "" ? null : JSON.parse(json) as last_value_data;
  }

  price_formatter(): (price: number) => string {
    this.assert_live();
    const wasm = this.chart.wasm;
    const id = this.id;
    return (price: number) => wasm.series_format_price(id, price);
  }

  apply_options(options: Partial<any_series_options>): void {
    this.assert_live();
    if (options.max_points !== undefined) {
      // 0 (and anything below 1) clears the cap back to unbounded, matching the option's docs.
      this.chart.wasm.set_series_max_points(this.id, options.max_points);
      // Applying a cap may evict points, so this is a full-range data change even though it
      // arrived as an option. Emitted unconditionally rather than only when rows actually left:
      // over-notifying is safe (handlers re-read), and this is a configuration call, not a hot
      // path, so it is not worth a round trip to find out.
      this.chart.repaint();
      for (const handler of this.data_changed_subs) handler("full");
    }
    if (options.color !== undefined) {
      // CSS string passed through verbatim so the engine keeps any alpha channel.
      this.chart.wasm.set_series_color_css(this.id, options.color);
    }
    if (options.visible !== undefined) {
      this.chart.wasm.set_series_visible(this.id, options.visible);
    }
    if (options.up_color !== undefined || options.down_color !== undefined) {
      // CSS strings passed through so the engine keeps alpha; empty = leave unchanged.
      this.chart.wasm.set_series_updown_colors(this.id, options.up_color ?? "", options.down_color ?? "");
    }
    if (options.wick_up_color !== undefined || options.wick_down_color !== undefined) {
      // Pass each direction through unchanged: undefined = keep, "" = clear (follow body), CSS = pin.
      // (A plain `?? ""` here would wrongly clear the direction the caller left unspecified.)
      this.chart.wasm.set_series_wick_colors(this.id, options.wick_up_color, options.wick_down_color);
    }
    if (options.border_up_color !== undefined || options.border_down_color !== undefined) {
      this.chart.wasm.set_series_border_colors(this.id, options.border_up_color, options.border_down_color);
    }
    if (options.wick_visible !== undefined) {
      this.chart.wasm.set_series_wick_visible(this.id, options.wick_visible);
    }
    if (options.border_visible !== undefined) {
      this.chart.wasm.set_series_border_visible(this.id, options.border_visible);
    }
    if (options.line_width !== undefined) {
      this.chart.wasm.set_series_line_width(this.id, options.line_width);
    }
    if (options.area_top_color !== undefined || options.area_bottom_color !== undefined) {
      this.chart.wasm.set_series_area_colors(this.id, options.area_top_color ?? "", options.area_bottom_color ?? "");
    }
    if (options.histogram_updown !== undefined) {
      this.chart.wasm.set_series_histogram_updown(this.id, options.histogram_updown);
    }
    if (options.line_type !== undefined) {
      this.chart.wasm.set_series_line_type(this.id, LINE_TYPE_TO_U8[options.line_type]);
    }
    if (options.point_markers !== undefined) {
      this.chart.wasm.set_series_point_markers(this.id, options.point_markers);
    }
    if (options.baseline_value !== undefined) {
      this.chart.wasm.set_series_baseline(this.id, options.baseline_value);
    }
    const requested_scale = options.overlay
      ? ""
      : options.priceScaleId ?? options.price_scale_id;
    if (options.pane !== undefined || requested_scale !== undefined) {
      const pane = options.pane ?? this.pane_index();
      const scale = requested_scale ?? this.price_scale_id();
      if (!this.chart.wasm.try_set_series_pane_and_scale(
        this.id,
        pane,
        options.pane_stretch ?? 1,
        scale,
      )) {
        throw new nucleuscharts_error(
          "invalid_options",
          `price scale '${scale}' does not exist in pane ${pane}`,
        );
      }
      if (scale === "") {
        const m = options.scale_margins ?? { top: 0.8, bottom: 0 };
        this.chart.wasm.set_series_overlay(this.id, m.top, m.bottom);
      }
    }
    if (options.last_price_animation !== undefined) {
      this.chart.wasm.set_series_last_price_animation(this.id, options.last_price_animation);
      this.chart.sync_animation();
    }
    if (options.price_format !== undefined) {
      const pf = options.price_format;
      if (pf.type === "custom") {
        // The callback crosses into wasm; min_move (when given) rides the JSON patch.
        this.chart.wasm.set_series_price_formatter(this.id, pf.formatter);
        if (pf.min_move !== undefined) {
          this.chart.wasm.series_apply_price_format_json(
            this.id, JSON.stringify({ type: "custom", min_move: pf.min_move }),
          );
        }
      } else {
        this.chart.wasm.series_apply_price_format_json(
          this.id, JSON.stringify({ type: pf.type, precision: pf.precision, min_move: pf.min_move }),
        );
      }
    }
    // Style keys without a dedicated setter go to the engine as a single JSON patch.
    const json_patch: Record<string, unknown> = {};
    for (const key of SERIES_JSON_OPTION_KEYS) {
      const value = options[key];
      if (value !== undefined) json_patch[key] = value;
    }
    if (Object.keys(json_patch).length > 0) {
      this.chart.wasm.series_apply_options_json(this.id, JSON.stringify(json_patch));
    }
    this.chart.sync_countdown_timer();
    this.chart.repaint();
  }

  /** Push the current bid/ask quotes (`bid_ask_visible` renders them); `null` hides a side. */
  set_bid_ask(bid: number | null, ask: number | null): void {
    this.assert_live();
    this.chart.wasm.set_series_bid_ask(this.id, bid ?? Number.NaN, ask ?? Number.NaN);
    this.chart.repaint();
  }

  options(): any_series_options {
    this.assert_live();
    return JSON.parse(this.chart.wasm.series_options_json(this.id)) as series_options;
  }

  set_type(kind: series_kind): void {
    this.assert_live();
    if (kind === "custom" || is_feature_series_kind(kind)) {
      throw new nucleuscharts_error(
        "unsupported_operation",
        "set_type() only converts built-in series; remove and re-add custom or advanced series",
      );
    }
    if (!this.chart.wasm.set_series_kind(this.id, KIND_TO_U8[kind])) {
      throw new nucleuscharts_error("stale_handle", "this series has been removed from the chart");
    }
    this.chart.repaint();
  }

  move_to_pane(pane_index: number, stretch = 1): void {
    this.assert_live();
    if (!this.chart.wasm.try_set_series_pane(this.id, pane_index, stretch)) {
      throw new nucleuscharts_error(
        "invalid_options",
        `pane ${pane_index} does not contain price scale '${this.price_scale_id()}'`,
      );
    }
    this.chart.repaint();
  }

  create_price_line(options: price_line_options): price_line_api {
    this.assert_live();
    const rgb = parse_rgb(options.color ?? "#2196f3") ?? [0x21, 0x96, 0xf3];
    const style = LINE_STYLE_TO_U8[options.line_style ?? "solid"];
    const id = this.chart.wasm.create_price_line(
      this.id,
      options.price,
      rgb[0],
      rgb[1],
      rgb[2],
      options.line_width ?? 1,
      style,
      options.title ?? "",
    );
    // Extras the positional constructor doesn't take go through the JSON patch path.
    const extras: Partial<price_line_options> = {};
    if (options.line_visible !== undefined) extras.line_visible = options.line_visible;
    if (options.axis_label_visible !== undefined) extras.axis_label_visible = options.axis_label_visible;
    if (options.axis_label_color !== undefined) extras.axis_label_color = options.axis_label_color;
    if (options.axis_label_text_color !== undefined) extras.axis_label_text_color = options.axis_label_text_color;
    if (Object.keys(extras).length > 0) {
      this.chart.wasm.price_line_apply_options(id, JSON.stringify(extras));
    }
    this.chart.repaint();
    const chart = this.chart;
    return {
      id,
      remove() {
        chart.wasm.remove_price_line(id);
        chart.repaint();
      },
      apply_options(patch: Partial<price_line_options>) {
        chart.wasm.price_line_apply_options(id, JSON.stringify(patch));
        chart.repaint();
      },
      options() {
        return JSON.parse(chart.wasm.price_line_options_json(id)) as price_line_options;
      },
    };
  }

  set_markers(markers: readonly series_marker[], options?: Partial<series_marker_options>): void {
    this.assert_live();
    if (options?.auto_scale !== undefined) {
      this.chart.wasm.set_series_markers_auto_scale(this.id, options.auto_scale);
    }
    if (options?.z_order !== undefined) {
      const z_order = options.z_order === "aboveSeries" ? 1 : options.z_order === "top" ? 2 : 0;
      if (!this.chart.wasm.set_series_markers_z_order(this.id, z_order)) {
        throw new Error("Nucleus rejected the series-marker z-order");
      }
    }
    // Normalize marker times to UTC seconds so business-day/string forms match their data points
    // (the engine's marker JSON expects a numeric time).
    const normalized = markers.map((mk) => ({ ...mk, time: time_to_utc_seconds(mk.time) }));
    if (!this.chart.wasm.set_series_markers(this.id, JSON.stringify(normalized))) {
      throw new Error("Nucleus rejected invalid series markers");
    }
    this.chart.repaint();
  }

  price_scale(): price_scale_api {
    const pane = undef_to_null(this.chart.wasm.series_pane_index(this.id)) ?? 0;
    return new price_scale_impl(this.chart, pane, this.price_scale_id());
  }
  price_scale_id(): string {
    this.assert_live();
    return this.chart.wasm.series_price_scale_name(this.id);
  }
  move_to_price_scale(id: string): void {
    this.assert_live();
    if (!this.chart.wasm.set_series_price_scale_by_name(this.id, id)) {
      throw new nucleuscharts_error(
        "invalid_options",
        `price scale '${id}' does not exist in pane ${this.pane_index()}`,
      );
    }
    this.chart.repaint();
  }
  pane_index(): number {
    this.assert_live();
    return undef_to_null(this.chart.wasm.series_pane_index(this.id)) ?? 0;
  }
  price_to_coordinate(price: number): number | null {
    return undef_to_null(this.chart.wasm.series_price_to_coordinate(this.id, price));
  }
  coordinate_to_price(coordinate: number): number | null {
    return undef_to_null(this.chart.wasm.series_coordinate_to_price(this.id, coordinate));
  }
  bars_in_logical_range(range: logical_range): bars_info | null {
    const info = this.chart.wasm.series_bars_in_logical_range(this.id, range.from, range.to);
    if (info.length < 2) return null;
    return info.length === 4
      ? { bars_before: info[0]!, bars_after: info[1]!, from: info[2]!, to: info[3]! }
      : { bars_before: info[0]!, bars_after: info[1]! };
  }
  private unpack_point(values: Float64Array | number[], offset = 0): series_data | null {
    if (values.length < offset + 5) return null;
    const time = values[offset]!;
    const [o, h, l, c] = [values[offset + 1]!, values[offset + 2]!, values[offset + 3]!, values[offset + 4]!];
    // A whitespace row round-trips all-NaN; return it as `{time}` with no value keys.
    if (Number.isNaN(o) && Number.isNaN(h) && Number.isNaN(l) && Number.isNaN(c)) {
      return { time };
    }
    if (this.series_type() === "candlestick" || this.series_type() === "bar") {
      return { time, open: o, high: h, low: l, close: c };
    }
    return { time, value: c };
  }
  data_by_index(logical_index: number, mismatch_direction: mismatch_direction = 0): series_data | null {
    return this.unpack_point(
      this.chart.wasm.series_data_by_index(this.id, logical_index, mismatch_direction),
    );
  }
  data(): readonly series_data[] {
    const values = this.chart.wasm.series_data(this.id);
    const output: series_data[] = [];
    for (let offset = 0; offset + 4 < values.length; offset += 5) {
      const point = this.unpack_point(values, offset);
      if (point !== null) output.push(point);
    }
    return output;
  }
  series_type(): series_kind {
    const kind = this.chart.wasm.series_kind(this.id) ?? KIND_TO_U8[this.kind];
    return KIND_NAMES[kind] ?? "candlestick";
  }
  indicator_info(): indicator_info | null {
    const raw = JSON.parse(this.chart.wasm.series_indicator_info_json(this.id)) as
      | { kind: indicator_info["kind"]; period: number; deviation: number | null; source: number; output_index: number }
      | null;
    if (raw === null) return null;
    return {
      kind: raw.kind,
      period: raw.period,
      deviation: raw.deviation,
      source: this.chart.series_handle(raw.source),
      output_index: raw.output_index,
    };
  }
  subscribe_data_changed(handler: data_changed_handler): void {
    this.data_changed_subs.add(handler);
  }
  unsubscribe_data_changed(handler: data_changed_handler): void {
    this.data_changed_subs.delete(handler);
  }

  attach_primitive(primitive: series_primitive): series_primitive_handle {
    this.assert_live();
    // Bind the hooks the plugin actually implements into a plain object (the host reads own
    // properties; binding also pins `this` for class-instance primitives), then register.
    const adapted: Record<string, unknown> = {};
    for (const key of [
      "detached",
      "update_all_views",
      "pane_views",
      "price_axis_views",
      "time_axis_views",
      "text_views",
      "autoscale_info",
      "hit_test",
    ] as const) {
      const hook = primitive[key];
      if (typeof hook === "function") adapted[key] = hook.bind(primitive);
    }
    if (typeof primitive.attached === "function") {
      const attached = primitive.attached;
      // The host supplies `{series_id, pane_index}`; inject `request_update` (a repaint
      // scheduler, reference `requestUpdate`) before the plugin sees the params.
      adapted.attached = (params: { series_id: number; pane_index?: number; request_update?: () => void }) => {
        params.request_update = () => this.chart.repaint();
        attached.call(primitive, params);
      };
    }
    const id = this.chart.wasm.attach_series_primitive(this.id, adapted);
    this.chart.repaint();
    return new series_primitive_handle_impl(this.chart, id);
  }

  /** Package-internal boundary for the first-class Rust primitive helpers. */
  native_add_partial_price_line(): number {
    this.assert_live();
    return this.chart.wasm.add_native_partial_price_line(this.id);
  }
  native_add_image_watermark(
    width: number,
    height: number,
    pixels: Uint8Array,
    options_json: string,
  ): number {
    this.assert_live();
    return this.chart.wasm.add_native_image_watermark(
      this.id,
      width,
      height,
      pixels,
      options_json,
    );
  }
  native_add_anchored_text(options_json: string): number {
    this.assert_live();
    return this.chart.wasm.add_native_anchored_text(this.id, options_json);
  }
  native_set_anchored_text_options(id: number, options_json: string): boolean {
    this.assert_live();
    const changed = this.chart.wasm.set_native_anchored_text_options(id, options_json);
    if (changed) this.chart.repaint();
    return changed;
  }
  native_add_bands_indicator(options_json: string): number {
    this.assert_live();
    return this.chart.wasm.add_native_bands_indicator(this.id, options_json);
  }
  native_set_bands_indicator_options(primitive_id: number, options_json: string): boolean {
    this.assert_live();
    const changed = this.chart.wasm.set_native_bands_indicator_options(primitive_id, options_json);
    if (changed) this.chart.repaint();
    return changed;
  }
  native_add_overlay_price_scale(options_json: string): number {
    this.assert_live();
    return this.chart.wasm.add_native_overlay_price_scale(this.id, options_json);
  }
  native_set_overlay_price_scale_options(primitive_id: number, options_json: string): boolean {
    this.assert_live();
    const changed = this.chart.wasm.set_native_overlay_price_scale_options(primitive_id, options_json);
    if (changed) this.chart.repaint();
    return changed;
  }
  native_add_accessibility_focus(options_json: string): number {
    this.assert_live();
    return this.chart.wasm.add_native_accessibility_focus(this.id, options_json);
  }
  native_set_accessibility_focus(primitive_id: number, time: number | null, options_json: string): boolean {
    this.assert_live();
    const changed = this.chart.wasm.set_native_accessibility_focus(
      primitive_id,
      time ?? Number.NaN,
      options_json,
    );
    if (changed) this.chart.repaint();
    return changed;
  }
  native_add_session_highlighting(options_json: string): number {
    this.assert_live();
    return this.chart.wasm.add_native_session_highlighting(this.id, options_json);
  }
  native_set_session_highlighting_data(primitive_id: number, highlights_json: string): boolean {
    this.assert_live();
    const changed = this.chart.wasm.set_native_session_highlighting_data(primitive_id, highlights_json);
    if (changed) this.chart.repaint();
    return changed;
  }
  native_add_crosshair_highlight(color: string): number {
    this.assert_live();
    return this.chart.wasm.add_native_crosshair_highlight(this.id, color);
  }
  native_add_vertical_line(time: number, options_json: string): number {
    this.assert_live();
    return this.chart.wasm.add_native_vertical_line(this.id, time, options_json);
  }
  native_add_user_price_lines_button(options_json: string): number {
    this.assert_live();
    return this.chart.wasm.add_native_user_price_lines_button(this.id, options_json);
  }
  native_add_user_price_alerts(options_json: string): number {
    this.assert_live();
    return this.chart.wasm.add_native_user_price_alerts(this.id, options_json);
  }
  native_add_delta_tooltip(options_json: string): number {
    this.assert_live();
    return this.chart.wasm.add_native_delta_tooltip(this.id, options_json);
  }
  native_add_tooltip(options_json: string): number {
    this.assert_live();
    return this.chart.wasm.add_native_tooltip(this.id, options_json);
  }
  native_set_tooltip_options(primitive_id: number, options_json: string): boolean {
    this.assert_live();
    const accepted = this.chart.wasm.set_native_tooltip_options(primitive_id, options_json);
    if (accepted) this.chart.repaint();
    return accepted;
  }
  native_tooltip_snapshot_json(primitive_id: number): string {
    this.assert_live();
    return this.chart.wasm.native_tooltip_snapshot_json(primitive_id);
  }
  native_delta_tooltip_active_range_json(primitive_id: number): string {
    this.assert_live();
    return this.chart.wasm.native_delta_tooltip_active_range_json(primitive_id);
  }
  native_clear_delta_tooltip(primitive_id: number): boolean {
    this.assert_live();
    const changed = this.chart.wasm.clear_native_delta_tooltip(primitive_id);
    if (changed) this.chart.repaint();
    return changed;
  }
  native_add_user_price_alert(primitive_id: number, price: number): number {
    this.assert_live();
    const id = this.chart.wasm.add_native_user_price_alert(primitive_id, price);
    if (id !== 0) this.chart.repaint();
    return id;
  }
  native_remove_user_price_alert(primitive_id: number, alert_id: number): boolean {
    this.assert_live();
    const removed = this.chart.wasm.remove_native_user_price_alert(primitive_id, alert_id);
    if (removed) this.chart.repaint();
    return removed;
  }
  native_user_price_alerts_json(primitive_id: number): string {
    this.assert_live();
    return this.chart.wasm.native_user_price_alerts_json(primitive_id);
  }
  native_add_trend_line(
    first_time: number,
    first_price: number,
    second_time: number,
    second_price: number,
    options_json: string,
  ): number {
    this.assert_live();
    return this.chart.wasm.add_native_trend_line(
      this.id,
      first_time,
      first_price,
      second_time,
      second_price,
      options_json,
    );
  }
  native_add_volume_profile(data_json: string, options_json: string): number {
    this.assert_live();
    return this.chart.wasm.add_native_volume_profile(this.id, data_json, options_json);
  }
  native_set_volume_profile_data(id: number, data_json: string): boolean {
    this.assert_live();
    const changed = this.chart.wasm.set_native_volume_profile_data(id, data_json);
    if (changed) this.chart.repaint();
    return changed;
  }
  native_add_expiring_price_alerts(options_json: string): number {
    this.assert_live();
    return this.chart.wasm.add_native_expiring_price_alerts(this.id, options_json);
  }
  native_add_expiring_price_alert(
    primitive_id: number,
    price: number,
    start: number,
    end: number,
    title: string,
    crossing_direction: "up" | "down",
  ): number {
    this.assert_live();
    const id = this.chart.wasm.add_native_expiring_price_alert(
      primitive_id,
      price,
      start,
      end,
      title,
      crossing_direction,
    );
    if (id !== 0) this.chart.repaint();
    return id;
  }
  native_remove_expiring_price_alert(primitive_id: number, alert_id: number): boolean {
    this.assert_live();
    const removed = this.chart.wasm.remove_native_expiring_price_alert(primitive_id, alert_id);
    if (removed) this.chart.repaint();
    return removed;
  }
  native_refresh_expiring_price_alerts(
    primitive_id: number,
    time: number,
    value: number,
    now_ms: number,
  ): number {
    this.assert_live();
    const delay = this.chart.wasm.refresh_native_expiring_price_alerts(
      primitive_id,
      time,
      value,
      now_ms,
    );
    this.chart.repaint();
    return delay;
  }
  native_expiring_price_alerts_json(primitive_id: number): string {
    this.assert_live();
    return this.chart.wasm.native_expiring_price_alerts_json(primitive_id);
  }
  native_remove_primitive(id: number): void {
    if (this.chart.wasm.remove_native_primitive(id)) this.chart.repaint();
  }
  native_repaint(): void {
    this.chart.repaint();
  }
}

function assert_trading_result(json: string): void {
  const result = JSON.parse(json) as
    | { ok: true }
    | { ok: false; error: { code: nucleuscharts_error_code; message: string } };
  if (!result.ok) throw new nucleuscharts_error(result.error.code, result.error.message);
}

function parse_engine_result<T extends object>(json: string): T {
  const result = JSON.parse(json) as
    | ({ ok: true } & T)
    | { ok: false; error: { code: nucleuscharts_error_code; message: string } };
  if (!result.ok) throw new nucleuscharts_error(result.error.code, result.error.message);
  return result;
}

export interface native_primitive_handle {
  detach(): void;
}

export interface native_bands_indicator_handle extends native_primitive_handle {
  set_options_json(options_json: string): boolean;
}

export interface native_overlay_price_scale_handle extends native_primitive_handle {
  set_options_json(options_json: string): boolean;
}

export interface native_volume_profile_handle extends native_primitive_handle {
  set_data_json(data_json: string): boolean;
}

export interface native_accessibility_focus_handle extends native_primitive_handle {
  set(time: number | null, options_json: string): boolean;
}

export interface native_session_highlighting_handle extends native_primitive_handle {
  set_data_json(data_json: string): boolean;
}

export interface native_anchored_text_handle extends native_primitive_handle {
  set_options_json(options_json: string): boolean;
}

export interface native_text_watermark_handle extends native_primitive_handle {
  set_options_json(options_json: string): boolean;
}

export interface native_expiring_price_alerts_handle extends native_primitive_handle {
  add(
    price: number,
    start: number,
    end: number,
    title: string,
    crossing_direction: "up" | "down",
  ): number;
  remove(alert_id: number): boolean;
  alerts_json(): string;
}

export interface native_user_price_alerts_handle extends native_primitive_handle {
  add(price: number): number;
  remove(alert_id: number): boolean;
  alerts_json(): string;
}

export interface native_delta_tooltip_handle extends native_primitive_handle {
  active_range_json(): string;
  clear(): boolean;
}

export interface native_tooltip_handle extends native_primitive_handle {
  set_options_json(options_json: string): boolean;
  snapshot_json(): string;
}

function native_series(series: series_api): series_impl {
  if (!(series instanceof series_impl)) {
    throw new nucleuscharts_error(
      "invalid_handle",
      "engine-owned primitives require a series created by this Nucleus chart",
    );
  }
  return series;
}

function native_handle(series: series_impl, id: number): native_primitive_handle {
  if (id === 0) throw new nucleuscharts_error("invalid_data", "engine rejected native primitive data or options");
  series.native_repaint();
  let attached = true;
  return {
    detach() {
      if (!attached) return;
      attached = false;
      series.native_remove_primitive(id);
    },
  };
}

export function attach_native_partial_price_line(series: series_api): native_primitive_handle {
  const owner = native_series(series);
  return native_handle(owner, owner.native_add_partial_price_line());
}

export function attach_native_bands_indicator(
  series: series_api,
  options_json: string,
): native_bands_indicator_handle {
  const owner = native_series(series);
  const id = owner.native_add_bands_indicator(options_json);
  const base = native_handle(owner, id);
  return {
    set_options_json(next) {
      return owner.native_set_bands_indicator_options(id, next);
    },
    detach: base.detach,
  };
}

export function attach_native_overlay_price_scale(
  series: series_api,
  options_json: string,
): native_overlay_price_scale_handle {
  const owner = native_series(series);
  const id = owner.native_add_overlay_price_scale(options_json);
  const base = native_handle(owner, id);
  return {
    set_options_json(next) {
      return owner.native_set_overlay_price_scale_options(id, next);
    },
    detach: base.detach,
  };
}

export function attach_native_accessibility_focus(
  series: series_api,
  options_json: string,
): native_accessibility_focus_handle {
  const owner = native_series(series);
  const id = owner.native_add_accessibility_focus(options_json);
  const base = native_handle(owner, id);
  return {
    detach: base.detach,
    set(time, next_options_json) {
      return owner.native_set_accessibility_focus(id, time, next_options_json);
    },
  };
}

export function attach_native_session_highlighting(
  series: series_api,
  options_json: string,
): native_session_highlighting_handle {
  const owner = native_series(series);
  const id = owner.native_add_session_highlighting(options_json);
  const base = native_handle(owner, id);
  return {
    detach: base.detach,
    set_data_json(data_json) {
      return owner.native_set_session_highlighting_data(id, data_json);
    },
  };
}

export function attach_native_crosshair_highlight(
  series: series_api,
  color: string,
): native_primitive_handle {
  const owner = native_series(series);
  return native_handle(owner, owner.native_add_crosshair_highlight(color));
}

export function attach_native_vertical_line(
  series: series_api,
  time: number,
  options_json: string,
): native_primitive_handle {
  const owner = native_series(series);
  return native_handle(owner, owner.native_add_vertical_line(time, options_json));
}

export function attach_native_user_price_lines_button(
  series: series_api,
  options_json: string,
): native_primitive_handle {
  const owner = native_series(series);
  return native_handle(owner, owner.native_add_user_price_lines_button(options_json));
}

export function attach_native_user_price_alerts(
  series: series_api,
  options_json: string,
): native_user_price_alerts_handle {
  const owner = native_series(series);
  const id = owner.native_add_user_price_alerts(options_json);
  const base = native_handle(owner, id);
  return {
    add(price) {
      const alert_id = owner.native_add_user_price_alert(id, price);
      if (alert_id === 0) {
        throw new nucleuscharts_error("invalid_data", "engine rejected user price-alert data");
      }
      return alert_id;
    },
    remove(alert_id) {
      return owner.native_remove_user_price_alert(id, alert_id);
    },
    alerts_json() {
      return owner.native_user_price_alerts_json(id);
    },
    detach: base.detach,
  };
}

export function attach_native_delta_tooltip(
  series: series_api,
  options_json: string,
): native_delta_tooltip_handle {
  const owner = native_series(series);
  const id = owner.native_add_delta_tooltip(options_json);
  const base = native_handle(owner, id);
  return {
    active_range_json() {
      return owner.native_delta_tooltip_active_range_json(id);
    },
    clear() {
      return owner.native_clear_delta_tooltip(id);
    },
    detach: base.detach,
  };
}

export function attach_native_tooltip(
  series: series_api,
  options_json: string,
): native_tooltip_handle {
  const owner = native_series(series);
  const id = owner.native_add_tooltip(options_json);
  const base = native_handle(owner, id);
  return {
    set_options_json(next) {
      return owner.native_set_tooltip_options(id, next);
    },
    snapshot_json() {
      return owner.native_tooltip_snapshot_json(id);
    },
    detach: base.detach,
  };
}

export function attach_native_trend_line(
  series: series_api,
  first_time: number,
  first_price: number,
  second_time: number,
  second_price: number,
  options_json: string,
): native_primitive_handle {
  const owner = native_series(series);
  return native_handle(owner, owner.native_add_trend_line(
    first_time,
    first_price,
    second_time,
    second_price,
    options_json,
  ));
}

export function attach_native_volume_profile(
  series: series_api,
  data_json: string,
  options_json: string,
): native_volume_profile_handle {
  const owner = native_series(series);
  const id = owner.native_add_volume_profile(data_json, options_json);
  const base = native_handle(owner, id);
  return {
    detach: base.detach,
    set_data_json(next) {
      return owner.native_set_volume_profile_data(id, next);
    },
  };
}

export function attach_native_expiring_price_alerts(
  series: series_api,
  options_json: string,
): native_expiring_price_alerts_handle {
  const owner = native_series(series);
  const id = owner.native_add_expiring_price_alerts(options_json);
  const base = native_handle(owner, id);
  let timer: ReturnType<typeof setTimeout> | null = null;
  let attached = true;
  const schedule_refresh = (): void => {
    if (!attached) return;
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
    const last = owner.last_value_data(true);
    if (last === null) return;
    const delay = owner.native_refresh_expiring_price_alerts(
      id,
      time_to_utc_seconds(last.time),
      last.value,
      Date.now(),
    );
    if (delay >= 0) {
      timer = setTimeout(schedule_refresh, Math.max(0, delay));
    }
  };
  owner.subscribe_data_changed(schedule_refresh);
  return {
    add(price, start, end, title, crossing_direction) {
      const alert_id = owner.native_add_expiring_price_alert(
        id,
        price,
        start,
        end,
        title,
        crossing_direction,
      );
      if (alert_id === 0) {
        throw new nucleuscharts_error("invalid_data", "engine rejected expiring price-alert data");
      }
      return alert_id;
    },
    remove(alert_id) {
      return owner.native_remove_expiring_price_alert(id, alert_id);
    },
    alerts_json() {
      return owner.native_expiring_price_alerts_json(id);
    },
    detach() {
      if (!attached) return;
      attached = false;
      owner.unsubscribe_data_changed(schedule_refresh);
      if (timer !== null) clearTimeout(timer);
      base.detach();
    },
  };
}

function native_pane(pane: pane_api): pane_impl {
  if (!(pane instanceof pane_impl)) {
    throw new nucleuscharts_error(
      "invalid_handle",
      "engine-owned primitives require a pane created by this Nucleus chart",
    );
  }
  return pane;
}

export function attach_native_image_watermark(
  series: series_api,
  width: number,
  height: number,
  pixels: Uint8Array,
  options_json: string,
): native_primitive_handle {
  const owner = native_series(series);
  const id = owner.native_add_image_watermark(width, height, pixels, options_json);
  if (id === 0) throw new nucleuscharts_error("invalid_data", "engine rejected image watermark data or options");
  owner.native_repaint();
  let attached = true;
  return {
    detach() {
      if (!attached) return;
      attached = false;
      owner.native_remove_primitive(id);
    },
  };
}

export function attach_native_anchored_text(
  series: series_api,
  options_json: string,
): native_anchored_text_handle {
  const owner = native_series(series);
  const id = owner.native_add_anchored_text(options_json);
  if (id === 0) throw new nucleuscharts_error("invalid_data", "engine rejected anchored-text options");
  owner.native_repaint();
  let attached = true;
  return {
    set_options_json(next) {
      if (!attached) return false;
      return owner.native_set_anchored_text_options(id, next);
    },
    detach() {
      if (!attached) return;
      attached = false;
      owner.native_remove_primitive(id);
    },
  };
}

export function attach_native_text_watermark(
  pane: pane_api,
  options_json: string,
): native_text_watermark_handle {
  const owner = native_pane(pane);
  const id = owner.native_add_text_watermark(options_json);
  if (id === 0) throw new nucleuscharts_error("invalid_data", "engine rejected text-watermark options");
  owner.native_repaint();
  let attached = true;
  return {
    set_options_json(next) {
      if (!attached) return false;
      return owner.native_set_text_watermark_options(id, next);
    },
    detach() {
      if (!attached) return;
      attached = false;
      owner.native_remove_primitive(id);
    },
  };
}

/**
 * The handle for a custom series (plugin platform Phase C-c; the reference's `ISeriesApi<'Custom'>`).
 * Shares the built-in handle's options/coordinate/primitive surface; the data methods work on
 * the raw plugin items (the engine rows carry times only, so `data()`/`data_by_index()` come
 * from the host's aligned item store, and `last_value_data` resolves through the engine's
 * host-recorded frame values).
 */
class custom_series_impl extends series_impl {
  constructor(id: number, chart: chart_impl) {
    super(id, "custom", chart);
  }

  /** Replace the series' items (reference `setData`). Times convert to UTC seconds here (the same
   *  boundary conversion as the built-ins); sort/dedupe happens engine-side with the items
   *  carried along, so `data()` returns the aligned raw items. */
  set_data(data: readonly custom_series_item[]): void {
    this.assert_live();
    const converted = data.map((item) => ({ ...item, time: time_to_utc_seconds(item.time) }));
    this.chart.wasm.set_custom_series_data(this.id, converted);
    this.chart.repaint();
    for (const handler of this.data_changed_subs) handler("full");
  }

  /** Append a new item or replace the one at an existing time (reference `update`). */
  update(item: custom_series_item): void {
    this.assert_live();
    this.chart.wasm.update_custom_series_item(this.id, { ...item, time: time_to_utc_seconds(item.time) });
    // Custom series compute their price values through the JS pane view during render,
    // so this path must stay synchronous: last_value_data and friends read that
    // render-computed state immediately after an update.
    this.chart.repaint();
    for (const handler of this.data_changed_subs) handler("update");
  }

  set_data_typed(): void {
    // A custom series carries raw plugin items aligned by time, not OHLC columns.
    this.assert_live();
    throw new nucleuscharts_error("unsupported_operation", "set_data_typed() does not apply to a custom series");
  }

  update_typed(): void {
    // Same reason as `set_data_typed`: no OHLC columns to append.
    this.assert_live();
    throw new nucleuscharts_error("unsupported_operation", "update_typed() does not apply to a custom series");
  }

  set_ring_source(): void {
    // A ring carries OHLC rows; a custom series' values live in its host-side pane view.
    this.assert_live();
    throw new nucleuscharts_error("unsupported_operation", "set_ring_source() does not apply to a custom series");
  }

  /** The raw items aligned with the engine rows (sorted, last-wins deduped). */
  data(): readonly custom_series_item[] {
    this.assert_live();
    return this.chart.wasm.custom_series_data(this.id) as custom_series_item[];
  }

  data_by_index(logical_index: number, mismatch_direction: mismatch_direction = 0): custom_series_item | null {
    this.assert_live();
    return undef_to_null(
      this.chart.wasm.custom_series_data_by_index(this.id, logical_index, mismatch_direction),
    ) as custom_series_item | null;
  }

  series_type(): series_kind {
    return "custom";
  }

  set_type(): void {
    // A custom series' type IS the pane view; change it by removing and re-adding the series.
    this.assert_live();
    throw new nucleuscharts_error("unsupported_operation", "set_type() does not apply to a custom series");
  }
}

/** A public handle whose payload and renderer are both owned by the Rust feature-series state. */
class feature_series_impl extends series_impl {
  private heatmap_shader: ((amount: number) => string) | null = null;

  constructor(id: number, kind: feature_series_kind, chart: chart_impl) {
    super(id, kind, chart);
  }

  private engine_item(item: series_data): series_data {
    if (this.kind !== "heatmap" || this.heatmap_shader === null || !("cells" in item)) return item;
    const shader = this.heatmap_shader;
    return {
      ...item,
      cells: item.cells.map((cell) => ({ ...cell, color: shader(cell.amount) })),
    } as series_data;
  }

  set_data(data: readonly series_data[]): void {
    this.assert_live();
    const converted = data.map((item) => this.engine_item({
      ...item,
      time: time_to_utc_seconds(item.time),
    } as series_data));
    this.record_ingestion(this.chart.wasm.set_feature_series_data(this.id, converted));
    this.chart.sync_countdown_timer();
    this.chart.repaint();
    for (const handler of this.data_changed_subs) handler("full");
  }

  update(item: series_data): void {
    this.assert_live();
    this.record_ingestion(this.chart.wasm.update_feature_series_item(
      this.id,
      this.engine_item({ ...item, time: time_to_utc_seconds(item.time) } as series_data),
    ));
    if (this.chart.countdown_series_present) this.chart.sync_countdown_timer();
    this.chart.schedule_repaint();
    for (const handler of this.data_changed_subs) handler("update");
  }

  set_data_typed(): void {
    this.assert_live();
    throw new nucleuscharts_error(
      "unsupported_operation",
      "set_data_typed() does not apply to structured advanced-series payloads",
    );
  }

  update_typed(): void {
    this.assert_live();
    throw new nucleuscharts_error(
      "unsupported_operation",
      "update_typed() does not apply to structured advanced-series payloads",
    );
  }

  set_ring_source(): void {
    this.assert_live();
    throw new nucleuscharts_error(
      "unsupported_operation",
      "set_ring_source() does not apply to structured advanced-series payloads",
    );
  }

  data(): readonly series_data[] {
    this.assert_live();
    return this.chart.wasm.feature_series_data(this.id) as series_data[];
  }

  data_by_index(logical_index: number, mismatch_direction: mismatch_direction = 0): series_data | null {
    this.assert_live();
    return undef_to_null(
      this.chart.wasm.feature_series_data_by_index(this.id, logical_index, mismatch_direction),
    ) as series_data | null;
  }

  apply_options(options: Partial<any_series_options>): void {
    const { cell_shader, ...engine_options } = options;
    const shader_changed = this.kind === "heatmap" && cell_shader !== undefined;
    const current_data = shader_changed ? this.data() : [];
    if (shader_changed) {
      if (typeof cell_shader !== "function") {
        throw new nucleuscharts_error("invalid_options", "cell_shader must be a function");
      }
      this.heatmap_shader = cell_shader;
    }
    super.apply_options(engine_options);
    this.chart.wasm.apply_feature_series_options(this.id, JSON.stringify(engine_options));
    if (current_data.length > 0) {
      this.record_ingestion(this.chart.wasm.set_feature_series_data(
        this.id,
        current_data.map((item) => this.engine_item(item)),
      ));
    }
    this.chart.repaint();
  }

  options(): any_series_options {
    const options = {
      ...super.options(),
      ...(JSON.parse(this.chart.wasm.feature_series_options_json(this.id)) as Partial<any_series_options>),
    };
    return this.heatmap_shader === null ? options : { ...options, cell_shader: this.heatmap_shader };
  }

  series_type(): feature_series_kind {
    return this.kind as feature_series_kind;
  }

  set_type(): void {
    this.assert_live();
    throw new nucleuscharts_error(
      "unsupported_operation",
      "set_type() does not change an advanced series schema; remove and re-add the series",
    );
  }
}

class time_scale_impl implements time_scale_api {
  constructor(private readonly chart: chart_impl) {}

  scroll_position(): number {
    return this.chart.wasm.scroll_position();
  }
  /** Invalidate any in-flight animated scroll (a new scroll call or user gesture takes over). */
  cancel_scroll_animation(): void {
    this.chart.wasm.cancel_scroll_animation();
  }
  scroll_to_position(position: number, animated: boolean): void {
    if (!animated || this.chart.prefers_reduced_motion()) {
      this.chart.wasm.cancel_scroll_animation();
      this.chart.wasm.scroll_to_position(position);
      this.chart.repaint();
      return;
    }
    // The engine owns the cubic ease-out easing and applies every tick; this RAF loop is just
    // host-side frame scheduling (a newer scroll or a user gesture supersedes engine-side).
    if (this.chart.wasm.scroll_position() === position) {
      this.chart.wasm.cancel_scroll_animation();
      return;
    }
    this.chart.wasm.start_scroll_animation(position, SCROLL_ANIM_MS, performance.now());
    const step = () => {
      const done = Number.isNaN(this.chart.wasm.scroll_animation_tick(performance.now()));
      this.chart.repaint();
      if (!done) requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  }
  scroll_to_real_time(): void {
    this.cancel_scroll_animation();
    this.chart.wasm.scroll_to_real_time();
    this.chart.repaint();
  }
  reset_time_scale(): void {
    this.cancel_scroll_animation();
    this.chart.wasm.reset_time_scale();
    this.chart.repaint();
  }
  fit_content(): void {
    this.cancel_scroll_animation();
    this.chart.wasm.fit_content();
    this.chart.repaint();
  }
  apply_options(options: Partial<time_scale_options>): void {
  if (options.bar_spacing !== undefined) this.chart.wasm.apply_bar_spacing_option(options.bar_spacing);
  if (options.right_offset !== undefined) this.chart.wasm.apply_right_offset_option(options.right_offset);
    if (options.min_bar_spacing !== undefined) this.chart.wasm.set_min_bar_spacing(options.min_bar_spacing);
    if (options.max_bar_spacing !== undefined) this.chart.wasm.set_max_bar_spacing(options.max_bar_spacing);
    if (options.right_offset_pixels !== undefined) this.chart.wasm.set_right_offset_pixels(options.right_offset_pixels);
    if (options.time_visible !== undefined) this.chart.wasm.set_time_visible(options.time_visible);
    if (options.seconds_visible !== undefined) this.chart.wasm.set_seconds_visible(options.seconds_visible);
    if (options.fix_left_edge !== undefined) this.chart.wasm.set_fix_left_edge(options.fix_left_edge);
    if (options.fix_right_edge !== undefined) this.chart.wasm.set_fix_right_edge(options.fix_right_edge);
    if (options.lock_visible_time_range_on_resize !== undefined)
      this.chart.wasm.set_lock_visible_time_range_on_resize(options.lock_visible_time_range_on_resize);
    if (options.right_bar_stays_on_scroll !== undefined)
      this.chart.wasm.set_right_bar_stays_on_scroll(options.right_bar_stays_on_scroll);
    if (options.shift_visible_range_on_new_bar !== undefined)
      this.chart.wasm.set_shift_visible_range_on_new_bar(options.shift_visible_range_on_new_bar);
    if (options.allow_shift_visible_range_on_whitespace_replacement !== undefined)
      this.chart.wasm.set_allow_shift_visible_range_on_whitespace_replacement(
        options.allow_shift_visible_range_on_whitespace_replacement,
      );
    if (options.allow_bold_labels !== undefined)
      this.chart.wasm.set_allow_bold_labels(options.allow_bold_labels);
    if (options.ticks_visible !== undefined) this.chart.wasm.set_time_ticks_visible(options.ticks_visible);
    if (options.minimum_height !== undefined) this.chart.wasm.set_time_axis_minimum_height(options.minimum_height);
    if (options.tick_mark_max_character_length !== undefined)
      this.chart.wasm.set_tick_mark_max_character_length(options.tick_mark_max_character_length);
    if (options.visible !== undefined) this.chart.wasm.set_time_axis_visible(options.visible);
    if (options.tick_mark_formatter !== undefined)
      this.chart.wasm.set_tick_mark_formatter(options.tick_mark_formatter);
    this.chart.repaint();
  }
  options(): time_scale_options {
    return JSON.parse(this.chart.wasm.time_scale_options_json()) as time_scale_options;
  }
  get_visible_logical_range(): logical_range | null {
    const r = this.chart.wasm.visible_logical_range();
    return r.length === 2 ? { from: r[0]!, to: r[1]! } : null;
  }
  set_visible_logical_range(range: logical_range): void {
    this.cancel_scroll_animation();
    this.chart.wasm.set_visible_logical_range(range.from, range.to);
    this.chart.repaint();
  }
  get_visible_range(): time_range | null {
    const r = this.chart.wasm.visible_time_range();
    return r.length === 2 ? { from: r[0]!, to: r[1]! } : null;
  }
  set_visible_range(range: time_range): void {
    this.cancel_scroll_animation();
    this.chart.wasm.set_visible_time_range(range.from, range.to);
    this.chart.repaint();
  }
  subscribe_visible_logical_range_change(handler: visible_logical_range_handler): void {
    this.chart.subscribe_visible_logical_range_change(handler);
  }
  unsubscribe_visible_logical_range_change(handler: visible_logical_range_handler): void {
    this.chart.unsubscribe_visible_logical_range_change(handler);
  }
  subscribe_visible_time_range_change(handler: visible_time_range_handler): void {
    this.chart.subscribe_visible_time_range_change(handler);
  }
  unsubscribe_visible_time_range_change(handler: visible_time_range_handler): void {
    this.chart.unsubscribe_visible_time_range_change(handler);
  }
  subscribe_size_change(handler: size_change_handler): void {
    this.chart.subscribe_size_change(handler);
  }
  unsubscribe_size_change(handler: size_change_handler): void {
    this.chart.unsubscribe_size_change(handler);
  }
  time_to_coordinate(time: number): number | null {
    return undef_to_null(this.chart.wasm.time_to_coordinate(time));
  }
  coordinate_to_time(x: number): number | null {
    return undef_to_null(this.chart.wasm.coordinate_to_time(x));
  }
  logical_to_coordinate(logical: number): number | null {
    return undef_to_null(this.chart.wasm.logical_to_coordinate(logical));
  }
  coordinate_to_logical(x: number): number | null {
    return undef_to_null(this.chart.wasm.coordinate_to_logical(x));
  }
  time_to_index(time: number, find_nearest = false): number | null {
    const index = undef_to_null(this.chart.wasm.time_to_index(time, find_nearest));
    return index === null ? null : Number(index);
  }
  width(): number {
    return this.chart.wasm.time_scale_width();
  }
  height(): number {
    return this.chart.wasm.time_scale_height();
  }
}

class price_scale_impl implements price_scale_api {
  private readonly pane_id: number;

  constructor(
    private readonly chart: chart_impl,
    pane: number,
    private readonly id: string,
  ) {
    const pane_id = undef_to_null(chart.wasm.pane_stable_id(pane));
    if (pane_id === null) {
      throw new nucleuscharts_error("invalid_handle", `pane index ${pane} does not identify a live price scale`);
    }
    this.pane_id = pane_id;
    if (undef_to_null(chart.wasm.price_scale_target_by_id(pane, id)) === null) {
      throw new nucleuscharts_error("invalid_handle", `price scale '${id}' does not exist in pane ${pane}`);
    }
  }

  private pane(): number {
    const pane = undef_to_null(this.chart.wasm.pane_index_for_id(this.pane_id));
    if (pane === null) throw new nucleuscharts_error("stale_handle", "price-scale pane has been removed");
    return pane;
  }

  private target(): number {
    const target = undef_to_null(this.chart.wasm.price_scale_target_by_id(this.pane(), this.id));
    if (target === null) throw new nucleuscharts_error("stale_handle", "price scale has been removed");
    return target;
  }

  apply_options(options: deep_partial<price_scale_options>): void {
    if (options.mode !== undefined) {
      this.chart.wasm.set_price_scale_mode(this.pane(), this.target(), options.mode);
    }
    if (options.auto_scale !== undefined) {
      this.chart.wasm.set_price_scale_auto_scale(this.pane(), this.target(), options.auto_scale);
    }
    if (options.invert_scale !== undefined) {
      this.chart.wasm.set_price_scale_inverted(this.pane(), this.target(), options.invert_scale);
    }
    if (options.scale_margins !== undefined) {
      const current = this.options().scale_margins;
      this.chart.wasm.set_price_scale_margins(
        this.pane(),
        this.target(),
        options.scale_margins.top ?? current.top,
        options.scale_margins.bottom ?? current.bottom,
      );
    }
    // Style keys without a dedicated setter go to the engine as a single JSON patch.
    const json_patch: Record<string, unknown> = {};
    for (const key of PRICE_SCALE_JSON_OPTION_KEYS) {
      const value = options[key];
      if (value !== undefined) json_patch[key] = value;
    }
    if (Object.keys(json_patch).length > 0) {
      this.chart.wasm.price_scale_apply_options_json(this.pane(), this.target(), JSON.stringify(json_patch));
    }
    this.chart.repaint();
  }

  options(): price_scale_options {
    return JSON.parse(this.chart.wasm.price_scale_options_json(this.pane(), this.target())) as price_scale_options;
  }

  width(): number {
    return this.chart.wasm.price_scale_width(this.pane(), this.target());
  }

  set_visible_range(range: price_range): void {
    this.chart.wasm.set_price_scale_visible_range(this.pane(), this.target(), range.from, range.to);
    this.chart.repaint();
  }

  get_visible_range(): price_range | null {
    const range = this.chart.wasm.price_scale_visible_range(this.pane(), this.target());
    return range.length === 2 ? { from: range[0]!, to: range[1]! } : null;
  }

  set_auto_scale(on: boolean): void {
    this.chart.wasm.set_price_scale_auto_scale(this.pane(), this.target(), on);
    this.chart.repaint();
  }
}

class pane_impl implements pane_api {
  private readonly stable_id: number;

  constructor(private readonly chart: chart_impl, index: number) {
    const stable_id = undef_to_null(chart.wasm.pane_stable_id(index));
    if (stable_id === null) throw new nucleuscharts_error("invalid_handle", `pane index ${index} is not live`);
    this.stable_id = stable_id;
  }

  private index(): number {
    const index = undef_to_null(this.chart.wasm.pane_index_for_id(this.stable_id));
    if (index === null) throw new nucleuscharts_error("stale_handle", "pane has been removed");
    return index;
  }

  pane_index(): number {
    return this.index();
  }
  get_height(): number {
    return this.chart.wasm.pane_height(this.index());
  }
  get_geometry(): pane_geometry {
    // `{}` answers a stale index (e.g. after a pane removal) — report zeros.
    const g = JSON.parse(this.chart.wasm.pane_geometry_json(this.index())) as Partial<pane_geometry>;
    return { left: g.left ?? 0, top: g.top ?? 0, width: g.width ?? 0, height: g.height ?? 0 };
  }
  set_height(height: number): void {
    this.chart.wasm.set_pane_height(this.index(), height);
    this.chart.repaint();
  }
  get_stretch_factor(): number {
    return this.chart.wasm.pane_stretch(this.index());
  }
  set_stretch_factor(factor: number): void {
    this.chart.wasm.set_pane_stretch(this.index(), factor);
    this.chart.repaint();
  }
  move_to(target: number): boolean {
    // The engine answers false for a rejected move (e.g. a stale index after remove_pane);
    // on success this handle follows the pane to its new index.
    if (!this.chart.wasm.pane_move_to(this.index(), target)) return false;
    this.chart.repaint();
    return true;
  }
  preserve_empty_pane(): boolean {
    // The engine answers false for a stale index (e.g. after remove_pane).
    return this.chart.wasm.pane_preserve_empty(this.index());
  }
  set_preserve_empty_pane(flag: boolean): void {
    this.chart.wasm.pane_set_preserve_empty(this.index(), flag);
    this.chart.repaint();
  }
  get_series(): series_api[] {
    // Live handles from the engine's id list (empty for a stale index after remove_pane).
    const ids = this.chart.wasm.pane_series_ids(this.index());
    const out: series_api[] = [];
    for (const id of ids) {
      out.push(this.chart.series_handle(id));
    }
    return out;
  }
  price_scale(id: string): price_scale_api {
    return this.chart.price_scale(id, this.index());
  }

  attach_primitive(primitive: pane_primitive): pane_primitive_handle {
    // Bind the hooks the plugin actually implements into a plain object (the host reads own
    // properties; binding also pins `this` for class-instance primitives), then register.
    const adapted: Record<string, unknown> = {};
    for (const key of [
      "attached",
      "detached",
      "update_all_views",
      "pane_views",
      "price_axis_views",
      "time_axis_views",
      "text_views",
      "hit_test",
    ] as const) {
      const hook = primitive[key];
      if (typeof hook === "function") adapted[key] = hook.bind(primitive);
    }
    const id = this.chart.wasm.attach_pane_primitive(this.index(), adapted);
    this.chart.repaint();
    return new pane_primitive_handle_impl(this.chart, id);
  }

  attach_canvas_primitive(primitive: canvas_primitive): canvas_primitive_handle {
    // No wasm involvement: the package owns the plugin canvas and the per-frame pass.
    return this.chart.attach_canvas_primitive(this.index(), primitive);
  }

  /** Package-internal boundary for pane-scoped Rust primitives. */
  native_add_text_watermark(options_json: string): number {
    return this.chart.wasm.add_native_text_watermark(this.index(), options_json);
  }
  native_set_text_watermark_options(id: number, options_json: string): boolean {
    const changed = this.chart.wasm.set_native_text_watermark_options(id, options_json);
    if (changed) this.chart.repaint();
    return changed;
  }
  native_remove_primitive(id: number): void {
    if (this.chart.wasm.remove_native_primitive(id)) this.chart.repaint();
  }
  native_repaint(): void {
    this.chart.repaint();
  }
}

/** Detach handle for a registered pane primitive (the wasm registry owns the lifecycle). */
class pane_primitive_handle_impl implements pane_primitive_handle {
  constructor(private readonly chart: chart_impl, private readonly id: number) {}

  detach(): void {
    // The engine answers false for an unknown/already-detached id — repaint once, on change.
    if (this.chart.wasm.detach_pane_primitive(this.id)) {
      this.chart.repaint();
    }
  }
}

/** Detach handle for a registered series primitive (the wasm registry owns the lifecycle). */
class series_primitive_handle_impl implements series_primitive_handle {
  constructor(private readonly chart: chart_impl, private readonly id: number) {}

  detach(): void {
    // The engine answers false for an unknown/already-detached id — repaint once, on change.
    // (Detaching after the owning series was removed is a no-op: the removal auto-detached.)
    if (this.chart.wasm.detach_series_primitive(this.id)) {
      this.chart.repaint();
    }
  }
}

/** One registered canvas primitive (Phase C-e) in the package-side registry. */
interface canvas_primitive_entry {
  primitive: canvas_primitive;
  pane_index: number;
  detached: boolean;
}

/** Detach handle for a registered canvas primitive (the TS registry owns the lifecycle). */
/**
 * A live drawing handle (engine-owned drawing objects). `kind`/`pane_index` are cached at
 * creation — they never change over a drawing's lifetime; everything else queries the engine.
 */
class drawing_impl implements drawing_api {
  private removed = false;

  constructor(
    private readonly chart: chart_impl,
    readonly id: number,
    private readonly drawing_kind: drawing_kind,
    private readonly pane: number,
  ) {}
  private assert_live(): string {
    void this.chart.wasm;
    if (this.removed) throw new nucleuscharts_error("stale_handle", "drawing has been removed");
    const options = this.chart.wasm.drawing_options_json(this.id);
    if (options === "") throw new nucleuscharts_error("stale_handle", "drawing has been removed");
    return options;
  }
  kind(): drawing_kind {
    this.assert_live();
    return this.drawing_kind;
  }
  pane_index(): number {
    this.assert_live();
    return this.pane;
  }
  points(): drawing_point[] {
    this.assert_live();
    return JSON.parse(this.chart.wasm.drawing_points_json(this.id)) as drawing_point[];
  }
  set_points(points: drawing_point[]): void {
    this.assert_live();
    if (!this.chart.wasm.drawing_set_points(this.id, JSON.stringify(points))) {
      throw new nucleuscharts_error("invalid_data", "drawing anchors are malformed or invalid for this kind");
    }
    this.chart.repaint();
  }
  options(): drawing_options {
    return JSON.parse(this.assert_live()) as drawing_options;
  }
  apply_options(options: Partial<drawing_options>): void {
    this.assert_live();
    if (!this.chart.wasm.drawing_apply_options(this.id, JSON.stringify(options))) {
      throw new nucleuscharts_error("invalid_options", "drawing options are malformed");
    }
    this.chart.repaint();
  }
  remove(): void {
    this.assert_live();
    if (!this.chart.wasm.remove_drawing(this.id)) {
      throw new nucleuscharts_error("stale_handle", "drawing has been removed");
    }
    this.removed = true;
    this.chart.repaint();
  }
}

class canvas_primitive_handle_impl implements canvas_primitive_handle {  private detached = false;

  constructor(private readonly chart: chart_impl, private readonly entry: canvas_primitive_entry) {}

  detach(): void {
    // Idempotent-ish: detaching twice is a no-op (mirrors the wasm-registry handles).
    if (this.detached) return;
    this.detached = true;
    this.chart.detach_canvas_primitive(this.entry);
  }
}

/** Gesture toggles resolved to concrete values the recognizer reads on each event. */
export interface resolved_gestures {
  pan: boolean;
  pan_horz_touch: boolean;
  pan_vert_touch: boolean;
  wheel_scroll: boolean;
  wheel_zoom: boolean;
  pinch_zoom: boolean;
  axis_dblclick_reset_time: boolean;
  axis_dblclick_reset_price: boolean;
  axis_scale_price: boolean;
  axis_scale_time: boolean;
  kinetic_touch: boolean;
  kinetic_mouse: boolean;
  wheel_behavior: "auto" | "pan" | "zoom";
  panes_resize: boolean;
  tracking_exit_mode: "on_next_tap" | "on_touch_end";
}

function apply_scroll(v: boolean | handle_scroll_options, cfg: resolved_gestures): void {
  if (typeof v === "boolean") {
    // reference migrateHandleScaleScrollOptions: a boolean expands to all four scroll flags.
    cfg.pan = v;
    cfg.pan_horz_touch = v;
    cfg.pan_vert_touch = v;
    cfg.wheel_scroll = v;
    return;
  }
  // Object form merges over the current config; `pan` (the mouse-drag / generic pan) tracks
  // pressed_mouse_move, and the touch axes track their own flags.
  cfg.pan = v.pressed_mouse_move ?? cfg.pan;
  cfg.pan_horz_touch = v.horz_touch_drag ?? cfg.pan_horz_touch;
  cfg.pan_vert_touch = v.vert_touch_drag ?? cfg.pan_vert_touch;
  cfg.wheel_scroll = v.mouse_wheel ?? cfg.wheel_scroll;
}

function apply_scale(v: boolean | handle_scale_options, cfg: resolved_gestures): void {
  if (typeof v === "boolean") {
    // reference migrateHandleScaleScrollOptions: a boolean expands to every scale flag.
    cfg.wheel_zoom = v;
    cfg.pinch_zoom = v;
    cfg.axis_dblclick_reset_time = v;
    cfg.axis_dblclick_reset_price = v;
    cfg.axis_scale_price = v;
    cfg.axis_scale_time = v;
    return;
  }
  // Object form merges over the current config (reference applyOptions semantics).
  cfg.wheel_zoom = v.mouse_wheel ?? cfg.wheel_zoom;
  cfg.pinch_zoom = v.pinch ?? cfg.pinch_zoom;
  const adr = v.axis_double_click_reset;
  if (typeof adr === "boolean") {
    // reference migrateHandleScaleScrollOptions: a boolean expands to both axes.
    cfg.axis_dblclick_reset_time = adr;
    cfg.axis_dblclick_reset_price = adr;
  } else if (adr) {
    cfg.axis_dblclick_reset_time = adr.time ?? cfg.axis_dblclick_reset_time;
    cfg.axis_dblclick_reset_price = adr.price ?? cfg.axis_dblclick_reset_price;
  }
  const apm = v.axis_pressed_mouse_move;
  if (typeof apm === "boolean") {
    cfg.axis_scale_price = apm;
    cfg.axis_scale_time = apm;
  } else if (apm) {
    cfg.axis_scale_price = apm.price ?? cfg.axis_scale_price;
    cfg.axis_scale_time = apm.time ?? cfg.axis_scale_time;
  }
}

function apply_kinetic(v: boolean | kinetic_scroll_options, cfg: resolved_gestures): void {
  if (typeof v === "boolean") {
    cfg.kinetic_touch = v;
    cfg.kinetic_mouse = v;
    return;
  }
  cfg.kinetic_touch = v.touch ?? cfg.kinetic_touch;
  cfg.kinetic_mouse = v.mouse ?? cfg.kinetic_mouse;
}

function apply_tracking(v: tracking_mode_options, cfg: resolved_gestures): void {
  cfg.tracking_exit_mode = v.exit_mode ?? cfg.tracking_exit_mode;
}

class trading_impl implements trading_api {
  private readonly intent_handlers = new Set<trading_intent_handler>();

  constructor(private readonly chart: chart_impl) {}

  apply_snapshot(snapshot: trading_snapshot): void {
    assert_trading_result(this.chart.wasm.set_trading_snapshot_json(JSON.stringify(snapshot)));
    this.chart.repaint();
  }

  state(): Required<trading_snapshot> {
    return JSON.parse(this.chart.wasm.trading_snapshot_json()) as Required<trading_snapshot>;
  }

  update_position(position: trading_position): void {
    assert_trading_result(this.chart.wasm.update_trading_position_json(JSON.stringify(position)));
    this.chart.repaint();
  }

  remove_position(id: string): boolean {
    const changed = this.chart.wasm.remove_trading_position(id);
    if (changed) this.chart.repaint();
    return changed;
  }

  update_order(order: working_order): void {
    assert_trading_result(this.chart.wasm.update_working_order_json(JSON.stringify(order)));
    this.chart.repaint();
  }

  remove_order(id: string): boolean {
    const changed = this.chart.wasm.remove_working_order(id);
    if (changed) this.chart.repaint();
    return changed;
  }

  apply_execution(execution: trading_execution): void {
    assert_trading_result(this.chart.wasm.apply_trading_execution_json(JSON.stringify(execution)));
    this.chart.repaint();
  }

  remove_execution(id: string): boolean {
    const changed = this.chart.wasm.remove_trading_execution(id);
    if (changed) this.chart.repaint();
    return changed;
  }

  set_instrument(instrument: instrument_metadata): void {
    assert_trading_result(this.chart.wasm.set_instrument_metadata_json(JSON.stringify(instrument)));
    this.chart.repaint();
  }

  apply_options(options: Partial<trading_style_options>): void {
    assert_trading_result(this.chart.wasm.apply_trading_style_json(JSON.stringify(options)));
    this.chart.repaint();
  }

  set_confirmation_mode(mode: trading_confirmation_mode): void {
    this.chart.wasm.set_trading_manual_confirmation(mode === "manual");
    this.chart.repaint();
  }

  hit_at(x: number, y: number): trading_hit | null {
    return JSON.parse(this.chart.wasm.trading_hit_json(x, y)) as trading_hit | null;
  }

  preview(): trading_preview | null {
    return JSON.parse(this.chart.wasm.trading_preview_json()) as trading_preview | null;
  }

  take_intents(): trading_intent[] {
    return JSON.parse(this.chart.wasm.take_trading_intents_json()) as trading_intent[];
  }

  resolve_intent(sequence: number, accepted: boolean): boolean {
    const changed = this.chart.wasm.resolve_trading_intent(sequence, accepted);
    if (changed) this.chart.repaint();
    return changed;
  }

  subscribe_intents(handler: trading_intent_handler): void {
    this.intent_handlers.add(handler);
  }

  unsubscribe_intents(handler: trading_intent_handler): void {
    this.intent_handlers.delete(handler);
  }

  dispatch_pending_intents(): void {
    for (const intent of this.take_intents()) {
      this.chart.announce_trading_intent(intent);
      for (const handler of this.intent_handlers) handler(intent);
    }
  }
}

export class chart_impl implements chart_api {
  private wasm_instance: NucleusChart | null;
  private next_extra_series = false;
  private readonly gestures_cfg: resolved_gestures = {
    pan: true,
    pan_horz_touch: true,
    pan_vert_touch: true,
    wheel_scroll: true,
    wheel_zoom: true,
    pinch_zoom: true,
    axis_dblclick_reset_time: true,
    axis_dblclick_reset_price: true,
    axis_scale_price: true,
    axis_scale_time: true,
    kinetic_touch: true,
    kinetic_mouse: false,
    wheel_behavior: "auto",
    panes_resize: true,
    tracking_exit_mode: "on_next_tap",
  };
  private accessibility_handle: accessibility_handle | null = null;
  private readonly ts = new time_scale_impl(this);
  private readonly trading_handle = new trading_impl(this);
  private observer: ResizeObserver | null = null;
  private detach_gestures: (() => void) | null = null;
  private removed = false;
  private readonly series_by_id = new Map<number, series_impl>();
  private readonly crosshair_subs = new Set<mouse_event_handler>();
  private readonly click_subs = new Set<mouse_event_handler>();
  private readonly dbl_click_subs = new Set<dbl_click_handler>();
  private readonly visible_logical_range_subs = new Set<visible_logical_range_handler>();
  private readonly visible_time_range_subs = new Set<visible_time_range_handler>();
  private readonly size_change_subs = new Set<size_change_handler>();
  private readonly series_added_subs = new Set<series_change_handler>();
  private readonly series_removed_subs = new Set<series_change_handler>();
  private readonly options_change_subs = new Set<options_change_handler>();
  private readonly delta_tooltip_range_listeners = new Set<() => void>();
  private last_visible_logical_range: logical_range | null;
  private last_visible_time_range: time_range | null;
  private last_ts_width: number;
  private last_ts_height: number;
  private auto_size: boolean;
  private dpr_query: MediaQueryList | null = null;
  private readonly dpr_change_handler = (): void => {
    if (this.removed || !this.auto_size) return;
    const bounds = this.container.getBoundingClientRect();
    this.resize(bounds.width, bounds.height, window.devicePixelRatio || 1);
    this.bind_dpr_watcher();
  };
  /** Crosshair position tracked TS-side (for crosshair-less screenshots); `null` when hidden. */
  private last_crosshair: { x: number; y: number } | null = null;
  /** Last hover hit-test result (Phase C-d), refreshed on crosshair moves; feeds event params. */
  private hover: { series_id: number | null; object_id: string | null; cursor: string | null } | null = null;
  /** The armed interactive drawing tool (`null` = none) and its options template JSON. */
  private active_tool: drawing_kind | null = null;
  private active_tool_pane: number | null = null;
  private tool_options_json = "{}";
  private tool_listener: ((tool: drawing_kind | null) => void) | null = null;
  private readonly tool_change_subs = new Set<drawing_tool_change_handler>();
  private readonly drawing_created_subs = new Set<drawing_created_handler>();
  /** The text-tool editing session (a contentEditable over the chart, TradingView's typing mode). */
  private text_editor: HTMLElement | null = null;
  private text_editor_id = 0;
  /**
   * Re-anchor the open editor after any repaint-driving change (wheel zoom/scroll, pinch,
   * resize, data update): it re-queries the anchor's coordinates from the settled engine
   * scales and repositions the box, so the editor tracks its drawing instead of displacing.
   */
  private text_editor_reposition: (() => void) | null = null;
  private anim_frame: number | null = null;
  /** The 1s candle-close countdown interval; `null` while no countdown is visible. */
  private countdown_timer: ReturnType<typeof setInterval> | null = null;
  /** True while any pointer/touch is down — pauses the countdown tick so it can't repaint mid-gesture. */
  private interacting = false;
  /** Pending rAF handle for a coalesced repaint; `null` when no repaint is scheduled. */
  private repaint_raf: number | null = null;
  /** Pending rAF handle for the ring-drain loop; `null` while no ring source is bound. */
  private ring_raf: number | null = null;
  /** Reused `[pair_count, series_id, rows, ...]` report from `drain_ring_sources`. */
  private ring_report: Float64Array | null = null;
  /**
   * Reusable transfer buffer for `frame_stats()`. Sized by the engine itself so an engine that
   * appends a slot needs no matching bundle change, and allocated once per chart so a host
   * polling telemetry every frame adds no JS-heap allocation of its own (the acceptance bar is
   * that reading stats for 60s does not itself raise `cpu_ms`). Each read still copies these
   * few dozen bytes across the wasm boundary — fixed size, no growth.
   */
  private readonly stats_scratch = new Float64Array(NucleusChart.frame_stats_len());

  /** The gesture recognizer marks pointer/touch activity (down = true, all-up = false). */
  set_interacting(active: boolean): void {
    this.interacting = active;
  }

  trading(): trading_api {
    return this.trading_handle;
  }

  accessibility(): accessibility_handle {
    if (this.accessibility_handle === null) {
      throw new nucleuscharts_error("unsupported_operation", "accessibility is disabled for this chart");
    }
    return this.accessibility_handle;
  }

  /** Internal package hook used by the singleton accessibility controller. */
  set_accessibility_handle(handle: accessibility_handle | null): void {
    this.accessibility_handle = handle;
  }

  trading_hover_at(x: number, y: number): boolean {
    return this.wasm.trading_hover_at(x, y);
  }

  trading_hit_at(x: number, y: number): trading_hit | null {
    return this.trading_handle.hit_at(x, y);
  }

  trading_hit_at_device(x: number, y: number, device: number): trading_hit | null {
    return JSON.parse(this.wasm.trading_hit_json_device(x, y, device)) as trading_hit | null;
  }

  trading_cursor_at(x: number, y: number): string | null {
    const cursor = this.wasm.trading_cursor_at(x, y);
    return cursor === 2 ? "grab" : cursor === 1 ? "pointer" : null;
  }

  clear_trading_hover(): boolean {
    return this.wasm.clear_trading_hover();
  }

  deactivate_trading_group(): boolean {
    return this.wasm.deactivate_trading_group();
  }

  trading_drag_start_at(x: number, y: number): boolean {
    return this.wasm.trading_drag_start_at(x, y);
  }

  trading_drag_start_at_device(x: number, y: number, device: number): boolean {
    return this.wasm.trading_drag_start_at_device(x, y, device);
  }

  trading_keyboard_start_order(id: string): boolean {
    const started = this.wasm.trading_keyboard_start_order(id);
    if (started) this.repaint();
    return started;
  }

  trading_keyboard_adjust(ticks: number): boolean {
    const changed = this.wasm.trading_keyboard_adjust(ticks);
    if (changed) this.repaint();
    return changed;
  }

  trading_keyboard_commit(): void {
    this.wasm.trading_keyboard_commit_json();
    this.trading_handle.dispatch_pending_intents();
    this.repaint();
  }

  trading_keyboard_cancel(): boolean {
    const changed = this.discard_trading_interaction();
    if (changed) this.repaint();
    return changed;
  }

  trading_drag_to(y: number): boolean {
    return this.wasm.trading_drag_to(y);
  }

  trading_drag_end(): void {
    this.wasm.trading_drag_end_json();
    this.trading_handle.dispatch_pending_intents();
  }

  cancel_trading_drag(): void {
    this.wasm.cancel_trading_drag();
  }

  discard_trading_interaction(): boolean {
    return this.wasm.discard_trading_interaction();
  }

  trading_activate_at(x: number, y: number): boolean {
    const activated = this.wasm.trading_activate_at(x, y);
    this.trading_handle.dispatch_pending_intents();
    return activated;
  }

  /** Standard gesture forwarding for the engine-owned delta-tooltip interaction model. */
  native_delta_tooltip_mouse_down(x: number): void {
    this.wasm.native_delta_tooltip_mouse_down(x);
  }

  native_delta_tooltip_mouse_move(x: number): void {
    if (this.wasm.native_delta_tooltip_mouse_move(x)) this.notify_delta_tooltip_ranges();
  }

  native_delta_tooltip_mouse_up(): void {
    if (this.wasm.native_delta_tooltip_mouse_up()) this.notify_delta_tooltip_ranges();
  }

  native_delta_tooltip_touch_move(xs: Float64Array): boolean {
    const changed = this.wasm.native_delta_tooltip_touch_move(xs);
    if (changed) this.notify_delta_tooltip_ranges();
    return this.delta_tooltip_range_listeners.size > 0;
  }

  native_delta_tooltip_touch_active(): boolean {
    return this.delta_tooltip_range_listeners.size > 0;
  }

  native_delta_tooltip_leave(): boolean {
    const changed = this.wasm.native_delta_tooltip_leave();
    if (changed) this.notify_delta_tooltip_ranges();
    return changed;
  }

  add_delta_tooltip_range_listener(listener: () => void): () => void {
    this.delta_tooltip_range_listeners.add(listener);
    return () => this.delta_tooltip_range_listeners.delete(listener);
  }

  private notify_delta_tooltip_ranges(): void {
    for (const listener of this.delta_tooltip_range_listeners) listener();
  }
  /**
   * Cached "any live series has countdown_visible" flag (refreshed by `sync_countdown_timer`)
   * so streaming `update` calls can cheaply decide whether data-arrival may start the timer.
   */
  countdown_series_present = false;
  /** The Phase C-e plugin overlay canvas and its 2D context (package-owned host DOM). */
  private readonly plugin_ctx: CanvasRenderingContext2D;
  private readonly canvas_primitives: canvas_primitive_entry[] = [];
  private plugin_resize_observer: ResizeObserver | null = null;
  private backend_loss_count = 0;
  private readonly backend_loss_handler = (): void => {
    if (this.removed) return;
    this.backend_loss_count += 1;
    // The wgpu callback may arrive from a promise microtask. Defer the repaint once more so the
    // callback stack is fully unwound before Rust drops GPU resources and paints the warm 2D pane.
    queueMicrotask(() => this.repaint());
  };

  constructor(
    wasm: NucleusChart,
    private readonly container: HTMLElement,
    private readonly gpu_pane: HTMLCanvasElement,
    private readonly fallback_pane: HTMLCanvasElement,
    private readonly plugin_canvas: HTMLCanvasElement,
    private readonly overlay: HTMLCanvasElement,
    auto_size: boolean,
  ) {
    this.wasm_instance = wasm;
    const plugin_ctx = plugin_canvas.getContext("2d");
    if (plugin_ctx === null) {
      throw new nucleuscharts_error("renderer_platform_error", "plugin canvas 2D context is unavailable");
    }
    this.plugin_ctx = plugin_ctx;
    window.addEventListener("nucleuscharts-chart-backend-lost", this.backend_loss_handler);
    this.last_visible_logical_range = this.read_visible_logical_range();
    this.last_visible_time_range = this.read_visible_time_range();
    this.last_ts_width = this.wasm.time_scale_width();
    this.last_ts_height = this.wasm.time_scale_height();
    this.auto_size = auto_size;
    this.container.setAttribute("role", "group");
    if (!this.container.hasAttribute("aria-label")) {
      this.container.setAttribute("aria-label", "Financial chart");
    }
    this.detach_gestures = install_gestures(this);
    if (auto_size) {
      this.wasm.enable_auto_resize(container);
      this.bind_dpr_watcher();
    }
    // Canvas primitives (Phase C-e): the engine's own ResizeObserver (registered first, above)
    // re-renders on container resizes; this one re-runs the package-side canvas pass on the
    // settled frame so plugin content tracks the new size/DPR. The microtask defers past ALL
    // observer callbacks (microtasks drain after the notification task, before paint), so the
    // engine's auto-resize render has always completed by the time the pass runs.
    this.plugin_resize_observer = new ResizeObserver(() => {
      queueMicrotask(() => this.run_canvas_primitives());
    });
    this.plugin_resize_observer.observe(container);
  }

  /** Internal package boundary. Every post-disposal operation fails with one stable error. */
  get wasm(): NucleusChart {
    if (this.wasm_instance === null) {
      throw new nucleuscharts_error("disposed", "this chart has been disposed");
    }
    return this.wasm_instance;
  }

  /** Deterministic browser-test hook; intentionally absent from `chart_api`. */
  backend_loss_count_for_test(): number {
    return this.backend_loss_count;
  }

  /**
   * Coalesce renders onto the next animation frame. The streaming hot path
   * (series `update` on built-in series) must not pay a full render per tick: N calls
   * inside one frame produce exactly one repaint, which is what the engine's
   * invalidate-mask design assumes (reference behavior: mutations invalidate, the
   * frame flush renders). One-shot mutations (set_data, pop, options, gestures,
   * resize, explicit render(), take_screenshot) still repaint synchronously — axis
   * sizing hysteresis and custom-series render-time price computation make their
   * render counts observable. Any synchronous repaint cancels a pending scheduled
   * frame and paints immediately.
   */
  schedule_repaint(): void {
    if (this.removed || this.repaint_raf !== null) return;
    this.repaint_raf = requestAnimationFrame(() => {
      this.repaint_raf = null;
      this.repaint();
    });
  }

  /**
   * Start or stop the ring-drain frame loop to match the number of bound ring sources.
   *
   * The engine has no clock of its own — the package owns the frame loop — so a bound ring needs
   * something to tick it. While at least one ring is bound this runs one `requestAnimationFrame`
   * loop for the whole chart (not per series), and it stops as soon as the last ring unbinds, so a
   * chart with no rings pays nothing.
   *
   * Called on every bind/unbind rather than tracking a count: `ring_source_count()` is the engine's
   * own answer, which cannot drift from it.
   */
  sync_ring_drain_loop(): void {
    const rings = this.removed ? 0 : this.wasm.ring_source_count();
    if (rings === 0) {
      if (this.ring_raf !== null) {
        cancelAnimationFrame(this.ring_raf);
        this.ring_raf = null;
      }
      this.ring_report = null;
      return;
    }
    // `[pair_count, series_id, rows, ...]` — resized only when the ring count grows.
    const slots = 1 + rings * 2;
    if (this.ring_report === null || this.ring_report.length < slots) {
      this.ring_report = new Float64Array(slots);
    }
    if (this.ring_raf === null) this.ring_raf = requestAnimationFrame(this.ring_tick);
  }

  /**
   * One frame's ring drain. This is the whole per-frame cost of a ring source when the producer is
   * idle: one engine call that does an atomic load per ring. A repaint and `data_changed` only
   * happen when rows actually arrived, so a bound-but-quiet ring does not force the chart to
   * re-render at frame rate.
   */
  private readonly ring_tick = (): void => {
    this.ring_raf = null;
    if (this.removed) return;
    const report = this.ring_report;
    if (report === null) return;
    const rows = this.wasm.drain_ring_sources(report);
    if (rows > 0) {
      this.repaint();
      // Notify per series that actually received rows, using the engine's own report rather than
      // the set of bound rings — a quiet ring must not emit a spurious data change.
      const pairs = report[0] as number;
      for (let i = 0; i < pairs; i += 1) {
        const series = this.series_by_id.get(report[1 + i * 2] as number);
        series?.emit_ring_data_changed();
      }
      if (this.countdown_series_present) this.sync_countdown_timer();
    }
    this.ring_raf = requestAnimationFrame(this.ring_tick);
  };

  /** Repaint unless torn down. Named distinctly from the public `render` for internal use. */
  repaint(): void {
    if (this.repaint_raf !== null) {
      cancelAnimationFrame(this.repaint_raf);
      this.repaint_raf = null;
    }
    if (!this.removed) {
      this.wasm.render();
      // The text editor tracks its anchor through the change that drove this repaint
      // (wheel zoom/scroll, pinch, resize, data update) — before plugin passes composite.
      this.text_editor_reposition?.();
      this.run_canvas_primitives();
      this.emit_visible_range_changes();
    }
  }

  /**
   * Canvas primitives (plugin platform Phase C-e): after the engine frame, paint every
   * attached canvas primitive onto the package-owned DOM canvas. The canvas remains above pane
   * content for plugin z-order, but each renderer is clipped to its owning engine pane so it cannot
   * cover the shared price/time-axis chrome now rendered by WebGPU/Canvas2D.
   */
  private run_canvas_primitives(): void {
    if (this.removed) return;
    const canvas = this.plugin_canvas;
    if (canvas.width !== this.overlay.width) canvas.width = this.overlay.width;
    if (canvas.height !== this.overlay.height) canvas.height = this.overlay.height;
    const ctx = this.plugin_ctx;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    if (this.canvas_primitives.length === 0) return;
    // One target per frame, shared by every view (the reference shares one target per widget).
    // Media size is the canvas' own CSS box — pinned to the container, like the overlay's.
    const rect = canvas.getBoundingClientRect();
    const target = create_canvas_render_target(
      ctx,
      { width: rect.width, height: rect.height },
      { width: canvas.width, height: canvas.height },
    );
    // update_all_views before the frame's views (like C-a); views bucket by z_order so all
    // `normal` views paint before all `top` views (attach order within a pass).
    const normal: { renderer: canvas_pane_view["renderer"]; pane_index: number }[] = [];
    const top: { renderer: canvas_pane_view["renderer"]; pane_index: number }[] = [];
    for (const entry of this.canvas_primitives) {
      const primitive = entry.primitive;
      try {
        primitive.update_all_views?.();
      } catch (error) {
        console.warn(`nucleuscharts: canvas primitive \`update_all_views\` threw — ${error}`);
      }
      let views;
      try {
        views = primitive.pane_views?.();
      } catch (error) {
        console.warn(`nucleuscharts: canvas primitive \`pane_views\` threw — ${error}`);
        continue;
      }
      for (const view of views ?? []) {
        (view.z_order === "top" ? top : normal).push({
          renderer: view.renderer,
          pane_index: entry.pane_index,
        });
      }
    }
    const hpr = rect.width > 0 ? canvas.width / rect.width : 1;
    const vpr = rect.height > 0 ? canvas.height / rect.height : 1;
    for (const { renderer, pane_index } of [...normal, ...top]) {
      // Canvas primitives are pane-scoped. Keep their DOM layer above pane content while clipping
      // it to the owning engine pane, so the shared GPU/Canvas axis strips remain top-layer chrome.
      const geometry = JSON.parse(this.wasm.pane_geometry_json(pane_index)) as Partial<pane_geometry>;
      const left = (geometry.left ?? 0) * hpr;
      const top_px = (geometry.top ?? 0) * vpr;
      const width = (geometry.width ?? 0) * hpr;
      const height = (geometry.height ?? 0) * vpr;
      if (width <= 0 || height <= 0) continue;
      ctx.save();
      ctx.beginPath();
      ctx.rect(left, top_px, width, height);
      ctx.clip();
      try {
        renderer(target);
      } catch (error) {
        console.warn(`nucleuscharts: canvas primitive renderer threw — ${error}`);
      } finally {
        ctx.restore();
      }
    }
  }

  /** Register a canvas primitive (Phase C-e) on behalf of `pane_impl`. */
  attach_canvas_primitive(pane_index: number, primitive: canvas_primitive): canvas_primitive_handle {
    const entry: canvas_primitive_entry = { primitive, pane_index, detached: false };
    this.canvas_primitives.push(entry);
    try {
      primitive.attached?.({ pane_index });
    } catch (error) {
      console.warn(`nucleuscharts: canvas primitive \`attached\` threw — ${error}`);
    }
    this.repaint();
    return new canvas_primitive_handle_impl(this, entry);
  }

  /** Drop a canvas primitive; the repaint clears its paint (the pass re-clears the canvas). */
  detach_canvas_primitive(entry: canvas_primitive_entry): void {
    if (entry.detached) return;
    entry.detached = true;
    const index = this.canvas_primitives.indexOf(entry);
    if (index >= 0) this.canvas_primitives.splice(index, 1);
    try {
      entry.primitive.detached?.();
    } catch (error) {
      console.warn(`nucleuscharts: canvas primitive \`detached\` threw — ${error}`);
    }
    this.repaint();
  }

  private read_visible_logical_range(): logical_range | null {
    const r = this.wasm.visible_logical_range();
    return r.length === 2 ? { from: r[0]!, to: r[1]! } : null;
  }

  private read_visible_time_range(): time_range | null {
    const r = this.wasm.visible_time_range();
    return r.length === 2 ? { from: r[0]!, to: r[1]! } : null;
  }

  private emit_visible_range_changes(): void {
    const logical = this.read_visible_logical_range();
    const time = this.read_visible_time_range();
    const logical_changed = !same_logical_range(this.last_visible_logical_range, logical);
    const time_changed = !same_time_range(this.last_visible_time_range, time);
    this.last_visible_logical_range = logical;
    this.last_visible_time_range = time;

    if (logical_changed) {
      for (const handler of this.visible_logical_range_subs) {
        handler(logical ? { ...logical } : null);
      }
    }
    if (time_changed) {
      for (const handler of this.visible_time_range_subs) {
        handler(time ? { ...time } : null);
      }
    }

    // Size-change diff (reference `subscribeSizeChange`), fired on change only.
    const width = this.wasm.time_scale_width();
    const height = this.wasm.time_scale_height();
    if (width !== this.last_ts_width || height !== this.last_ts_height) {
      this.last_ts_width = width;
      this.last_ts_height = height;
      for (const handler of this.size_change_subs) {
        handler(width, height);
      }
    }
  }

  /** Start or stop the animation rAF loop to match whether any series wants the last-price pulse. */
  sync_animation(): void {
    if (this.removed) return;
    if (this.wasm.wants_animation()) {
      this.start_animation();
    } else {
      this.stop_animation();
    }
  }

  /**
   * Start or stop the 1s candle-close countdown interval to match whether any live series has
   * `countdown_visible` and data (TradingView-style countdown row in the last-value cluster).
   * Central rebuild point: called from the series apply-options/set-data/remove paths and on
   * chart teardown. Ticks pin the engine clock and repaint; ticks are skipped while the
   * document is hidden.
   */
  sync_countdown_timer(): void {
    if (this.removed) return;
    const candidates: { countdown_visible?: boolean; has_data: boolean }[] = [];
    this.countdown_series_present = false;
    for (const series of this.series_by_id.values()) {
      if (series.options().countdown_visible === true) {
        this.countdown_series_present = true;
        candidates.push({
          countdown_visible: true,
          has_data: this.wasm.series_last_value_data(series.id, true) !== "",
        });
      }
    }
    if (countdown_timer_needed(candidates)) {
      if (this.countdown_timer === null) {
        this.wasm.set_now_seconds(Date.now() / 1000);
        this.countdown_timer = setInterval(() => {
          // Skip while hidden or while the user is mid-gesture — a mid-drag repaint is the
          // visible lag/flicker when moving the chart.
          if (document.hidden || this.interacting) return;
          this.wasm.set_now_seconds(Date.now() / 1000);
          this.repaint();
        }, 1000);
      }
    } else if (this.countdown_timer !== null) {
      clearInterval(this.countdown_timer);
      this.countdown_timer = null;
    }
  }
  private stop_countdown_timer(): void {
    if (this.countdown_timer !== null) {
      clearInterval(this.countdown_timer);
      this.countdown_timer = null;
    }
  }
  private start_animation(): void {
    if (this.anim_frame !== null || this.removed) return;
    const tick = () => {
      if (this.removed || !this.wasm.wants_animation()) {
        this.anim_frame = null;
        return;
      }
      this.wasm.set_animation_time(performance.now());
      this.wasm.render();
      this.run_canvas_primitives();
      this.anim_frame = requestAnimationFrame(tick);
    };
    this.anim_frame = requestAnimationFrame(tick);
  }
  private stop_animation(): void {
    if (this.anim_frame !== null) {
      cancelAnimationFrame(this.anim_frame);
      this.anim_frame = null;
    }
  }

  add_series(kind: series_kind, options?: Partial<any_series_options>): series_api {
    if (kind === "custom") {
      throw new nucleuscharts_error(
        "invalid_options",
        "add_series does not accept 'custom'; use add_custom_series(pane_view)",
      );
    }
    if (is_feature_series_kind(kind)) {
      const adopt_primary = !this.next_extra_series;
      this.next_extra_series = true;
      const id = this.wasm.add_feature_series(FEATURE_KIND_TO_U8[kind], adopt_primary, "{}");
      if (id === 0xffffffff) {
        throw new nucleuscharts_error("invalid_options", `advanced series '${kind}' was rejected by the engine`);
      }
      const series = new feature_series_impl(id, kind, this);
      this.series_by_id.set(id, series);
      if (options) series.apply_options(options);
      this.emit_series_change(this.series_added_subs, series, this.pane_of_series(id));
      return series;
    }
    // Series 0 is created by the engine at construction; the first add_series adopts it so the
    // common "one chart, one series" path matches reference (add_series returns the primary series).
    let id: number;
    if (!this.next_extra_series) {
      this.next_extra_series = true;
      this.wasm.set_series_type(KIND_TO_U8[kind]);
      id = 0;
    } else {
      id = this.wasm.add_series(KIND_TO_U8[kind]);
    }
    const series = new series_impl(id, kind, this);
    this.series_by_id.set(id, series);
    if (options) {
      series.apply_options(options);
    }
    this.emit_series_change(this.series_added_subs, series, this.pane_of_series(id));
    return series;
  }

  add_custom_series(pane_view: custom_series_pane_view, options?: Partial<series_options>): series_api {
    // Bind the hooks the view actually implements into a plain object (the host reads own
    // properties; binding also pins `this` for class-instance views), then register — the same
    // adaptation the primitive handles use.
    const adapted: Record<string, unknown> = {};
    for (const key of ["price_value_builder", "is_whitespace", "render", "destroy"] as const) {
      const hook = pane_view[key];
      if (typeof hook === "function") adapted[key] = hook.bind(pane_view);
    }
    if (typeof adapted.price_value_builder !== "function" || typeof adapted.render !== "function") {
      throw new nucleuscharts_error(
        "invalid_options",
        "add_custom_series needs a pane view with `price_value_builder` and `render`",
      );
    }
    // The first-series adoption mirrors add_series (the engine's construction-time series 0
    // converts to Custom instead of leaving an empty built-in behind).
    let id: number;
    if (!this.next_extra_series) {
      this.next_extra_series = true;
      id = this.wasm.add_custom_series(adapted, true);
    } else {
      id = this.wasm.add_custom_series(adapted, false);
    }
    if (id === 0xffffffff) {
      throw new nucleuscharts_error("invalid_options", "add_custom_series was rejected by the engine");
    }
    const series = new custom_series_impl(id, this);
    this.series_by_id.set(id, series);
    // reference createCustomSeriesDefinition: the view's defaultOptions merge UNDER the caller's.
    const merged = { ...(pane_view.default_options ?? {}), ...(options ?? {}) } as Partial<series_options>;
    if (Object.keys(merged).length > 0) series.apply_options(merged);
    this.emit_series_change(this.series_added_subs, series, this.pane_of_series(id));
    return series;
  }

  remove_series(series: series_api): void {
    const impl = this.series_by_id.get(series.id);
    // Ignore a foreign handle or one already removed (idempotent, matching reference leniency).
    if (!impl) return;
    // Pane indices are captured BEFORE the engine tombstones: a removed series reports no pane,
    // and the removal can cascade to derived indicator outputs living on other panes.
    const pane_of = new Map<number, number>();
    for (const id of this.series_by_id.keys()) pane_of.set(id, this.pane_of_series(id));
    // The engine tombstones the primary series (id 0) safely, so no id is refused here. The
    // tracked removal reports the series plus every derived indicator output dropped with it.
    const dropped = this.wasm.remove_series_tracked(series.id);
    if (dropped.length === 0) return;
    for (const id of dropped) {
      const handle = this.series_by_id.get(id);
      if (!handle) continue;
      handle.mark_removed();
      this.series_by_id.delete(id);
      this.emit_series_change(this.series_removed_subs, handle, pane_of.get(id) ?? 0);
    }
    this.sync_countdown_timer();
    // The engine drops a removed series' ring with it, so the drain loop may now have nothing left
    // to do.
    this.sync_ring_drain_loop();
    this.repaint();
  }

  series_order(): series_api[] {
    const ids = JSON.parse(this.wasm.series_order_json()) as number[];
    // Only ids with a live TS handle are returned; a series the package no longer tracks is skipped.
    return ids
      .map((id) => this.series_by_id.get(id))
      .filter((s): s is series_impl => s !== undefined);
  }

  /**
   * The live handle for an engine series id: the registered handle when one exists, otherwise a
   * fresh handle adopted into the registry (the way `panes()`/`price_scale()` build handles on
   * demand). Backs `pane_api.get_series`.
   */
  series_handle(id: number): series_impl {
    let series = this.series_by_id.get(id);
    if (series === undefined) {
      const kind = this.wasm.series_kind(id) ?? KIND_TO_U8.candlestick;
      if (kind === 7) {
        const feature_kind = FEATURE_KIND_NAMES[this.wasm.feature_series_kind(id) ?? -1];
        series = feature_kind === undefined
          ? new series_impl(id, "candlestick", this)
          : new feature_series_impl(id, feature_kind, this);
      } else {
        series = new series_impl(id, KIND_NAMES[kind] ?? "candlestick", this);
      }
      this.series_by_id.set(id, series);
    }
    return series;
  }

  set_series_order(ordered: series_api[]): boolean {
    const ids = new Uint32Array(ordered.map((s) => s.id));
    if (!this.wasm.set_series_order(ids)) return false;
    this.repaint();
    return true;
  }

  private indicator_series(id: number, options?: Partial<series_options>): series_api {
    if (id === 0xffffffff) throw new nucleuscharts_error("invalid_options", "invalid indicator configuration");
    const series = new series_impl(id, "line", this);
    this.series_by_id.set(id, series);
    if (options) series.apply_options(options);
    this.emit_series_change(this.series_added_subs, series, this.pane_of_series(id));
    // Separate-pane indicators create their pane engine-side: relayout now so the new pane
    // gets real bounds on this frame instead of waiting for the next incidental repaint.
    this.repaint();
    return series;
  }

  add_sma(source: series_api, period: number, options?: Partial<series_options>): series_api {
    return this.indicator_series(this.wasm.add_sma(source.id, Math.max(1, Math.floor(period))), options);
  }

  add_ema(source: series_api, period: number, options?: Partial<series_options>): series_api {
    return this.indicator_series(this.wasm.add_ema(source.id, Math.max(1, Math.floor(period))), options);
  }

  add_bollinger(source: series_api, period: number, deviation = 2, options?: Partial<series_options>): [series_api, series_api, series_api] {
    const ids = this.wasm.add_bollinger(source.id, Math.max(1, Math.floor(period)), deviation);
    if (ids.length !== 3) throw new nucleuscharts_error("invalid_options", "invalid Bollinger configuration");
    return [this.indicator_series(ids[0]!, options), this.indicator_series(ids[1]!, options), this.indicator_series(ids[2]!, options)];
  }

  add_rsi(source: series_api, period: number, options?: Partial<series_options>): series_api {
    return this.indicator_series(this.wasm.add_rsi(source.id, Math.max(1, Math.floor(period))), options);
  }

  add_macd(source: series_api, fast: number, slow: number, signal: number, options?: Partial<series_options>): [series_api, series_api, series_api] {
    const ids = this.wasm.add_macd(source.id, Math.max(1, Math.floor(fast)), Math.max(1, Math.floor(slow)), Math.max(1, Math.floor(signal)));
    if (ids.length !== 3) throw new nucleuscharts_error("invalid_options", "invalid MACD configuration");
    return [this.indicator_series(ids[0]!, options), this.indicator_series(ids[1]!, options), this.indicator_series(ids[2]!, options)];
  }

  add_stochastic(source: series_api, k_period: number, d_period: number, options?: Partial<series_options>): [series_api, series_api] {
    const ids = this.wasm.add_stochastic(source.id, Math.max(1, Math.floor(k_period)), Math.max(1, Math.floor(d_period)));
    if (ids.length !== 2) throw new nucleuscharts_error("invalid_options", "invalid Stochastic configuration");
    return [this.indicator_series(ids[0]!, options), this.indicator_series(ids[1]!, options)];
  }

  add_atr(source: series_api, period: number, options?: Partial<series_options>): series_api {
    return this.indicator_series(this.wasm.add_atr(source.id, Math.max(1, Math.floor(period))), options);
  }

  add_vwap(source: series_api, volume_source?: series_api | null, options?: Partial<series_options>): series_api {
    return this.indicator_series(this.wasm.add_vwap(source.id, volume_source?.id ?? -1), options);
  }

  add_wma(source: series_api, period: number, options?: Partial<series_options>): series_api {
    return this.indicator_series(this.wasm.add_wma(source.id, Math.max(1, Math.floor(period))), options);
  }

  subscribe_crosshair_move(handler: mouse_event_handler): void {
    this.crosshair_subs.add(handler);
  }
  unsubscribe_crosshair_move(handler: mouse_event_handler): void {
    this.crosshair_subs.delete(handler);
  }
  subscribe_click(handler: mouse_event_handler): void {
    this.click_subs.add(handler);
  }
  unsubscribe_click(handler: mouse_event_handler): void {
    this.click_subs.delete(handler);
  }
  subscribe_dbl_click(handler: dbl_click_handler): void {
    this.dbl_click_subs.add(handler);
  }
  unsubscribe_dbl_click(handler: dbl_click_handler): void {
    this.dbl_click_subs.delete(handler);
  }
  subscribe_visible_logical_range_change(handler: visible_logical_range_handler): void {
    this.visible_logical_range_subs.add(handler);
  }
  unsubscribe_visible_logical_range_change(handler: visible_logical_range_handler): void {
    this.visible_logical_range_subs.delete(handler);
  }
  subscribe_visible_time_range_change(handler: visible_time_range_handler): void {
    this.visible_time_range_subs.add(handler);
  }
  unsubscribe_visible_time_range_change(handler: visible_time_range_handler): void {
    this.visible_time_range_subs.delete(handler);
  }
  subscribe_size_change(handler: size_change_handler): void {
    this.size_change_subs.add(handler);
  }
  unsubscribe_size_change(handler: size_change_handler): void {
    this.size_change_subs.delete(handler);
  }
  subscribe_series_added(handler: series_change_handler): void {
    this.series_added_subs.add(handler);
  }
  unsubscribe_series_added(handler: series_change_handler): void {
    this.series_added_subs.delete(handler);
  }
  subscribe_series_removed(handler: series_change_handler): void {
    this.series_removed_subs.add(handler);
  }
  unsubscribe_series_removed(handler: series_change_handler): void {
    this.series_removed_subs.delete(handler);
  }
  subscribe_options_change(handler: options_change_handler): void {
    this.options_change_subs.add(handler);
  }
  unsubscribe_options_change(handler: options_change_handler): void {
    this.options_change_subs.delete(handler);
  }

  /** A series' current pane index (0 fallback for a tombstoned/unknown id). */
  private pane_of_series(id: number): number {
    return undef_to_null(this.wasm.series_pane_index(id)) ?? 0;
  }

  /** Notify series-lifecycle subscribers (added and removed share the payload shape). */
  private emit_series_change(subs: Set<series_change_handler>, series: series_api, pane_index: number): void {
    if (subs.size === 0) return;
    for (const h of subs) h({ series, pane_index });
  }

  /** Invalidate any in-flight animated `scroll_to_position` (user gestures take over scrolling). */
  cancel_scroll_animation(): void {
    this.ts.cancel_scroll_animation();
  }

  /**
   * Which pane contains the pane-relative CSS point (x, y), or `null` when it falls on an axis
   * strip (price/time) or outside the pane area. Backs `mouse_event_params.pane_index`.
   */
  pane_index_at(x: number, y: number): number | null {
    if (x < 0 || x > this.wasm.time_scale_width()) return null;
    const pane_bottom = this.overlay.getBoundingClientRect().height - this.wasm.time_scale_height();
    if (y < 0 || y > pane_bottom) return null;
    return this.wasm.pane_index_at_y(y);
  }

  /** Build event params for a cursor at (x, y) in pane CSS px. */
  private build_params(x: number, y: number): mouse_event_params {
    const time = undef_to_null(this.wasm.coordinate_to_time(x));
    const logical = undef_to_null(this.wasm.coordinate_to_logical(x));
    const flat = this.wasm.hover_data(x); // groups of [id, o, h, l, c]
    const series_data = new Map<series_api, ohlc_data | single_value_data>();
    const t = time ?? 0;
    for (let i = 0; i + 4 < flat.length; i += 5) {
      const s = this.series_by_id.get(flat[i]!);
      if (!s) continue;
      const [o, h, l, c] = [flat[i + 1]!, flat[i + 2]!, flat[i + 3]!, flat[i + 4]!];
      series_data.set(
        s,
        s.kind === "candlestick" || s.kind === "bar"
          ? { time: t, open: o, high: h, low: l, close: c }
          : { time: t, value: c },
      );
    }
    const hovered_series = this.hover?.series_id != null ? this.series_handle(this.hover.series_id) : null;
    return {
      time, logical, point: { x, y }, pane_index: this.pane_index_at(x, y), series_data,
      hovered_series, hovered_object_id: this.hover?.object_id ?? null,
    };
  }

  /**
   * Refresh the hover hit-test state (Phase C-d) for a crosshair at pane CSS px (x, y): runs
   * the engine's series hit test plus the primitives' `hit_test`, stashes the result for
   * `build_params`, and updates the engine's hovered series for `hoveredSeriesOnTop` (the
   * caller repaints, so the z-bump lands on the next frame). Called by the gesture
   * recognizer on every crosshair move.
   */
  update_hover(x: number, y: number): void {
    this.hover = JSON.parse(this.wasm.hover_at(x, y)) as chart_impl["hover"];
  }

  /** Clear the hover state (cursor left the chart) and release the z-bump; caller repaints. */
  clear_hover(): void {
    this.hover = null;
    this.wasm.clear_hover();
  }

  /** The cursor a primitive's `hit_test` reports for the current hover, or `null`. */
  hover_cursor(): string | null {
    return this.hover?.cursor ?? null;
  }

  /** The engine-hit series under the current hover, or `null` (drives the pointer cursor). */
  hover_series_id(): number | null {
    return this.hover?.series_id ?? null;
  }

  /** Emit a crosshair-move event (called by the gesture recognizer). */
  emit_crosshair(x: number, y: number): void {
    this.last_crosshair = { x, y };
    if (this.crosshair_subs.size === 0) return;
    const params = this.build_params(x, y);
    for (const h of this.crosshair_subs) h(params);
  }
  /** Emit the "cursor left the chart" crosshair event (empty params). */
  emit_crosshair_left(): void {
    this.last_crosshair = null;
    if (this.crosshair_subs.size === 0) return;
    const params: mouse_event_params = {
      time: null, logical: null, point: null, pane_index: null, series_data: new Map(),
      hovered_series: null, hovered_object_id: null,
    };
    for (const h of this.crosshair_subs) h(params);
  }
  /** Emit a click event (called by the gesture recognizer). */
  emit_click(x: number, y: number): void {
    // A click/tap is a discrete, intentional action, so it is a good moment to announce the point
    // to assistive tech (unlike mouse hover, which would flood the live region).
    this.announce(x, y);
    // TradingView-style click-to-select: select the series under the click (the frame build
    // paints anchor points on it) and clear the selection on empty pane space. The hover
    // hit-test refreshes at the click point first, so a click without a preceding move still
    // arbitrates correctly.
    this.update_hover(x, y);
    this.wasm.click_native_primitives_at(x, y);
    // TradingView-style click-to-select, drawings first: a drawing hit selects it and clears
    // the series selection; a miss clears the drawing selection and falls through to the
    // series under the click (or clears that on empty pane space).
    const drawing_hit = this.wasm.select_drawing_at(x, y);
    this.wasm.set_selected_series(drawing_hit ? undefined : (this.hover?.series_id ?? undefined));
    // A click on a text drawing (its label, or the muted "+ Add Text" placeholder) goes
    // straight into typing mode (TradingView parity).
    if (drawing_hit) {
      const selected = this.selected_drawing();
      if (selected !== null && selected.kind() === "text") {
        this.open_text_editor(selected);
      }
    }
    this.repaint();
    if (this.click_subs.size === 0) return;
    const params = this.build_params(x, y);
    for (const h of this.click_subs) h(params);
  }

  /** Announce the current visible time range to assistive tech (used after keyboard navigation). */
  announce_view(): void {
    const r = this.wasm.visible_time_range();
    this.accessibility_handle?.announce(
      r.length === 2 ? `Showing time ${r[0]} to ${r[1]}` : "No data",
    );
  }

  emit_dbl_click(x: number, y: number): void {
    if (this.dbl_click_subs.size === 0) return;
    const params = this.build_params(x, y);
    for (const h of this.dbl_click_subs) h(params);
  }

  // ---------------------------------------------------------------------------------------------
  // Drawing tools (engine-owned drawing objects; the gesture layer routes presses/clicks/moves
  // through the creation helpers below, everything else is thin wasm delegation)
  // ---------------------------------------------------------------------------------------------

  add_drawing(
    kind: drawing_kind,
    points: drawing_point[],
    options?: Partial<drawing_options>,
    pane_index = 0,
  ): drawing_api {
    const id = this.wasm.add_drawing(
      DRAWING_KIND_TO_U8[kind],
      pane_index,
      JSON.stringify(points),
      JSON.stringify(options ?? {}),
    );
    if (id === 0) {
      throw new nucleuscharts_error(
        "invalid_data",
        "add_drawing rejected (stale pane, wrong anchor count, or non-finite anchors)",
      );
    }
    this.repaint();
    return new drawing_impl(this, id, kind, pane_index);
  }

  drawings(): drawing_api[] {
    const list = JSON.parse(this.wasm.drawings_json()) as drawing_info[];
    return list.map((d) => new drawing_impl(this, d.id, d.kind, d.pane_index));
  }

  export_state(): chart_state_v1 {
    const result = JSON.parse(this.wasm.export_state_result_json()) as
      | { ok: true; document: string }
      | persistence_error_result;
    if (!result.ok) throw_persistence_error(result);
    return JSON.parse(result.document) as chart_state_v1;
  }

  import_state(state: chart_state_v1 | string): persistence_restore_result {
    let document: string;
    try {
      document = typeof state === "string" ? state : JSON.stringify(state);
    } catch (error) {
      throw new nucleuscharts_error("serialization_error", `state is not JSON-serializable: ${error}`);
    }
    const response = JSON.parse(this.wasm.import_state_result_json(document)) as
      | { ok: true; result: persistence_restore_result }
      | persistence_error_result;
    if (!response.ok) throw_persistence_error(response);
    this.repaint();
    return response.result;
  }

  clear_drawings(): void {
    this.wasm.clear_drawings();
    this.repaint();
  }

  announce_trading_intent(intent: trading_intent): void {
    const target = intent.order_id ?? intent.position_id ?? "trading object";
    const price = intent.price === undefined ? "" : ` at ${intent.price}`;
    this.accessibility_handle?.announce(
      `Trading request ${intent.action.replaceAll("_", " ")} for ${target}${price}, awaiting confirmation`,
    );
  }

  undo_drawing(): boolean {
    this.close_text_editor(true);
    const changed = this.wasm.undo_drawing();
    if (changed) this.repaint();
    return changed;
  }

  redo_drawing(): boolean {
    this.close_text_editor(true);
    const changed = this.wasm.redo_drawing();
    if (changed) this.repaint();
    return changed;
  }

  can_undo_drawing(): boolean {
    return this.wasm.can_undo_drawing();
  }

  can_redo_drawing(): boolean {
    return this.wasm.can_redo_drawing();
  }

  set_drawing_tool(
    tool: drawing_kind | null,
    options?: Partial<drawing_options>,
    pane_index?: number,
  ): void {
    const next_pane = tool === null
      ? null
      : (pane_index ?? (this.active_tool === tool ? this.active_tool_pane : null));
    const changed = this.active_tool !== tool || this.active_tool_pane !== next_pane;
    if (changed) this.close_text_editor(true); // arming another tool commits the edit
    this.active_tool = tool;
    this.active_tool_pane = next_pane;
    if (options !== undefined) {
      this.tool_options_json = JSON.stringify(options);
      if (this.wasm.drawing_create_active()) {
        this.wasm.drawing_create_apply_options(this.tool_options_json);
      }
    }
    if (tool === null) {
      this.wasm.drawing_create_cancel();
      this.wasm.brush_create_cancel();
    }
    if (changed) {
      this.tool_listener?.(tool);
      for (const handler of this.tool_change_subs) handler(tool);
    }
  }

  active_drawing_tool(): drawing_kind | null {
    return this.active_tool;
  }

  set_drawing_tool_listener(listener: ((tool: drawing_kind | null) => void) | null): void {
    this.tool_listener = listener;
  }

  subscribe_drawing_tool_change(handler: drawing_tool_change_handler): void {
    this.tool_change_subs.add(handler);
  }

  unsubscribe_drawing_tool_change(handler: drawing_tool_change_handler): void {
    this.tool_change_subs.delete(handler);
  }

  subscribe_drawing_created(handler: drawing_created_handler): void {
    this.drawing_created_subs.add(handler);
  }

  unsubscribe_drawing_created(handler: drawing_created_handler): void {
    this.drawing_created_subs.delete(handler);
  }

  selected_drawing(): drawing_api | null {
    const id = this.wasm.selected_drawing();
    if (id === undefined) return null;
    const info = (JSON.parse(this.wasm.drawings_json()) as drawing_info[]).find((d) => d.id === id);
    return info ? new drawing_impl(this, info.id, info.kind, info.pane_index) : null;
  }

  /** Whether an interactive drawing tool is armed (the recognizer routes pane clicks to creation). */
  creation_armed(): boolean {
    return this.active_tool !== null;
  }

  /** Whether an interactive creation is mid-placement (the engine's pending drawing). */
  creation_active(): boolean {
    return this.wasm.drawing_create_active();
  }

  /** Forward a mouse move to the engine's creation preview (no-op while unarmed). Modifiers:
   * `magnet` snaps the preview anchor to the nearest bar's OHLC, `straighten` constrains a
   * second anchor to 0°/45°/90° (a rectangle to a square). */
  creation_move(x: number, y: number, magnet = false, straighten = false): void {
    if (this.active_tool_pane !== null && this.pane_index_at(x, y) !== this.active_tool_pane) return;
    this.wasm.drawing_create_move(x, y, magnet, straighten);
  }

  /**
   * Route a pane click into interactive creation (the gesture recognizer's click path): begins
   * the engine's creation flow on the first click and places anchors on each click, disarming
   * the tool after a commit (one-shot, TradingView default). `magnet` snaps the placed anchor
   * to the nearest bar's OHLC, `straighten` constrains a second anchor. Returns whether the
   * click was consumed (an armed tool over a pane).
   */
  creation_click(x: number, y: number, magnet = false, straighten = false): boolean {
    const pane = this.pane_index_at(x, y);
    if (
      this.active_tool === null || pane === null ||
      (this.active_tool_pane !== null && pane !== this.active_tool_pane)
    ) return false;
    if (!this.wasm.drawing_create_active()) {
      if (!this.wasm.drawing_create_begin(DRAWING_KIND_TO_U8[this.active_tool], this.tool_options_json)) {
        this.set_drawing_tool(null);
        return false;
      }
    }
    const created_id = Number(this.wasm.drawing_create_click(x, y, magnet, straighten));
    if (created_id > 0) {
      const tool = this.active_tool;
      const created = new drawing_impl(this, created_id, tool, pane);
      for (const handler of this.drawing_created_subs) handler(created);
      this.set_drawing_tool(null);
      // The text tool goes straight into typing mode after placement (TradingView parity).
      if (tool === "text") {
        const drawing = this.selected_drawing();
        if (drawing !== null) this.open_text_editor(drawing);
      }
    }
    return true;
  }

  /** Escape: disarm the tool (cancelling any pending creation) and deselect any drawing. */
  cancel_drawing_interaction(): void {
    this.set_drawing_tool(null);
    this.wasm.set_selected_drawing(undefined);
  }

  /**
   * Begin a freehand brush stroke (pointer-down with the brush tool armed): the engine captures
   * and decimates the path, and renders it as a smooth curve.
   * Returns whether the stroke started (a pane was hit).
   */
  brush_create_start(x: number, y: number): boolean {
    const pane = this.pane_index_at(x, y);
    if (pane === null || (this.active_tool_pane !== null && pane !== this.active_tool_pane)) return false;
    return this.wasm.brush_create_start(this.tool_options_json, x, y);
  }

  /** Forward a drag position to the engine's brush capture (no-op without an active stroke). */
  brush_create_add(x: number, y: number): void {
    this.wasm.brush_create_add(x, y);
  }

  /**
   * Commit the brush stroke (pointer-up): the captured path is stored as-is and rendered as a
   * smooth curved drawing, left selected. Disarms the tool on a commit (one-shot, TradingView
   * default); a click without a drag discards the stroke.
   */
  brush_create_end(): void {
    if (this.wasm.brush_create_end() > 0) {
      this.set_drawing_tool(null);
    }
  }

  // ---------------------------------------------------------------------------------------------
  // Text-tool editing session (TradingView's typing mode — package-owned host DOM; the engine
  // renders the label, the placeholder, and the container, and stays DOM-free)
  // ---------------------------------------------------------------------------------------------

  /**
   * Open the typing-mode editor for a text drawing (TradingView's editing chrome): a square,
   * thick blue-bordered box centered on the anchor, hugging the text, with a bold muted
   * "Add text" preview that disappears the moment the user types. Enter or blur commits,
   * Escape cancels. While open, the engine suppresses its own placeholder/label for this
   * drawing (the editor's preview is the only visual for it).
   */
  open_text_editor(drawing: drawing_api): void {
    this.close_text_editor(true);
    let coords = this.wasm.drawing_point_to_coordinate(drawing.id, 0);
    if (coords.length !== 2) return;
    const options = drawing.options();
    const layout = (this.options() as { layout?: { fontSize?: number; fontFamily?: string; textColor?: string; mutedTextColor?: string; background?: { color?: string } } }).layout ?? {};
    const font_size = Math.max(options.text_size ?? layout.fontSize ?? 12, 12);
    const font_family = layout.fontFamily ?? "sans-serif";
    // The exact CSS shorthand the engine rasterizes with. `line-height: normal` keeps the DOM
    // line box on the font's own ascent+descent metrics — the same midpoint Chrome's canvas
    // `textBaseline: "middle"` uses — so the editor's glyphs sit exactly on the engine run's
    // center (a fixed 1.2 line box lifted the text ~0.5 css px).
    const style_prefix = options.text_italic ? "italic " : "";
    const font = `${style_prefix}${options.text_weight ?? 400} ${font_size}px ${font_family}`;
    const preview_font = `${style_prefix}700 ${font_size}px ${font_family}`;

    const default_palette = theme_palette(default_theme_name);
    // Square container hugging the text, using the same interactive-primary role as drawing handles.
    const wrap = document.createElement("div");
    wrap.id = "nucleuscharts-text-editor";
    wrap.style.position = "absolute";
    wrap.style.zIndex = "10";
    wrap.style.border = `2px solid ${default_palette.primary}`;
    wrap.style.borderRadius = "0";
    wrap.style.background = options.box_color || layout.background?.color || default_palette.background;
    wrap.style.padding = "4px";

    // The "Add text" preview (bold, muted, ≥ 12px): hidden as soon as the user types.
    const preview = document.createElement("span");
    preview.id = "nucleuscharts-text-preview";
    preview.textContent = "Add text";
    preview.style.position = "absolute";
    preview.style.left = "4px";
    preview.style.top = "50%";
    preview.style.transform = "translateY(-50%)";
    preview.style.font = preview_font;
    preview.style.color = layout.mutedTextColor || default_palette.muted_foreground;
    preview.style.pointerEvents = "none";
    preview.style.whiteSpace = "nowrap";

    const editor = document.createElement("div");
    editor.id = "nucleuscharts-text-input";
    editor.contentEditable = "true";
    editor.textContent = options.text;
    editor.style.font = font;
    editor.style.lineHeight = "normal"; // the font's own ascent+descent center (see above)
    editor.style.color = options.text_color || layout.textColor || default_palette.foreground;
    editor.style.background = "transparent";
    editor.style.border = "none";
    editor.style.outline = "none";
    editor.style.padding = "0";
    editor.style.margin = "0";
    editor.style.display = "block";
    editor.style.whiteSpace = "nowrap";

    // Hug-the-text width: measure with the exact font spec the engine rasterizes with.
    const measure_ctx = document.createElement("canvas").getContext("2d");
    // Self-calibrating alignment (no font-metric guessing): the editable IS a real inline
    // element, so a Range over its text tells us where the glyphs ACTUALLY land (an <input>
    // has untrackable internal text layout); the box then shifts so that rect aligns to the
    // anchor exactly as the engine's placement does (Left = run starts at the anchor, Center =
    // centered, Right = ends; Top = above, Middle = centered, Bottom = below, same 4px pad).
    const position_editor = () => {
      const rect = this.container.getBoundingClientRect();
      const anchor_x = Math.min(Math.max(coords[0]!, 20), rect.width - 20);
      const anchor_y = Math.min(Math.max(coords[1]!, 20), rect.height - 20);
      // Provisional placement, then one measured correction.
      wrap.style.left = `${Math.round(anchor_x - wrap.offsetWidth / 2)}px`;
      wrap.style.top = `${Math.round(anchor_y - wrap.offsetHeight / 2)}px`;
      const range = document.createRange();
      range.selectNodeContents(editor);
      const r = range.getBoundingClientRect();
      // An empty editable has no text rects — fall back to the provisional centering.
      if (r.width === 0 && editor.textContent === "") return;
      const m = {
        left: r.left - rect.left,
        right: r.right - rect.left,
        top: r.top - rect.top,
        bottom: r.bottom - rect.top,
      };
      const pad = 4;
      let dx = 0;
      if (options.text_h_align === "left") dx = anchor_x + pad - m.left;
      else if (options.text_h_align === "right") dx = anchor_x - pad - m.right;
      else dx = anchor_x - (m.left + m.right) / 2;
      let dy = 0;
      if (options.text_v_align === "top") dy = anchor_y - pad - m.bottom;
      else if (options.text_v_align === "bottom") dy = anchor_y + pad - m.top;
      else dy = anchor_y - (m.top + m.bottom) / 2;
      wrap.style.left = `${Math.round(Number.parseFloat(wrap.style.left) + dx)}px`;
      wrap.style.top = `${Math.round(Number.parseFloat(wrap.style.top) + dy)}px`;
    };
    const set_width = () => {
      const text = editor.textContent ?? "";
      if (measure_ctx !== null) {
        measure_ctx.font = text === "" ? preview_font : font;
        const w = measure_ctx.measureText(text === "" ? preview.textContent : text).width;
        editor.style.width = `${Math.ceil(w) + 4}px`;
      }
      preview.style.display = text === "" ? "" : "none";
      position_editor();
    };
    set_width();
    editor.addEventListener("input", set_width);
    editor.addEventListener("keydown", (e) => {
      // The editor owns its keys (the chart's Delete/arrows/Escape must not fire).
      e.stopPropagation();
      if (e.key === "Enter") {
        e.preventDefault(); // no newline — single-line labels
        this.close_text_editor(true);
      } else if (e.key === "Escape") {
        e.preventDefault();
        this.close_text_editor(false);
      }
    });
    editor.addEventListener("blur", () => this.close_text_editor(true));

    wrap.appendChild(preview);
    wrap.appendChild(editor);
    this.container.appendChild(wrap);
    position_editor();

    this.text_editor = editor;
    this.text_editor_id = drawing.id;
    // Follow the anchor across repaints (wheel zoom/scroll, resize, data changes): the engine
    // settles the scales first, then the editor re-anchors. A vanished drawing closes the edit.
    this.text_editor_reposition = () => {
      if (this.text_editor === null) return;
      const fresh = this.wasm.drawing_point_to_coordinate(drawing.id, 0);
      if (fresh.length !== 2) {
        this.close_text_editor(false);
        return;
      }
      coords = fresh;
      position_editor();
    };
    // The engine's own placeholder/label is suppressed while editing (one visual only).
    this.wasm.set_editing_drawing(drawing.id);
    this.repaint();
    editor.focus();
    // Caret at the END of the existing text (TradingView's editing entry), not a select-all.
    const selection = window.getSelection();
    if (selection !== null) {
      const range = document.createRange();
      range.selectNodeContents(editor);
      range.collapse(false);
      selection.removeAllRanges();
      selection.addRange(range);
    }
  }

  /**
   * Close the typing-mode editor, applying the typed text on `commit` (an empty commit leaves
   * the drawing text empty — the muted "Add text" placeholder keeps it re-editable).
   */
  close_text_editor(commit: boolean): void {
    const editor = this.text_editor;
    if (editor === null) return;
    this.text_editor = null;
    this.text_editor_reposition = null;
    const wrap = this.container.querySelector("#nucleuscharts-text-editor");
    wrap?.remove();
    this.wasm.set_editing_drawing(undefined);
    if (commit) {
      // Single-line labels: pasted newlines flatten to spaces.
      const text = (editor.textContent ?? "").replace(/\s*\n\s*/g, " ");
      this.wasm.drawing_apply_options(this.text_editor_id, JSON.stringify({ text }));
    }
    this.repaint();
    // Hand keyboard focus back to the gesture overlay (Delete/arrows work again).
    this.overlay_el().focus();
  }

  apply_options(options: deep_partial<chart_options>): void {
    // handle_scroll / handle_scale / kinetic_scroll / tracking_mode (gestures), the pane-resize
    // toggle, and localization (JS callbacks) are package-level; intercept and strip them so only
    // engine-owned, JSON-serializable options reach the wasm store.
    const { handle_scroll, handle_scale, kinetic_scroll, wheel_behavior, tracking_mode, localization, accessibility, ...rest } =
      options as deep_partial<chart_options> & {
        handle_scroll?: boolean | handle_scroll_options;
        handle_scale?: boolean | handle_scale_options;
        kinetic_scroll?: boolean | kinetic_scroll_options;
        wheel_behavior?: "auto" | "pan" | "zoom";
        tracking_mode?: tracking_mode_options;
        localization?: localization_options;
        accessibility?: boolean | accessibility_options;
      };
    let engine_options: Record<string, unknown> = rest;
    // layout.panes.enableResize (reference) drives the separator drag here, not the engine; strip it
    // alongside the other gesture keys before forwarding.
    const panes = (rest.layout as { panes?: { enableResize?: boolean } } | undefined)?.panes;
    if (panes?.enableResize !== undefined) {
      const { enableResize, ...panes_rest } = panes;
      engine_options = { ...rest, layout: { ...(rest.layout as object), panes: panes_rest } };
      this.gestures_cfg.panes_resize = enableResize;
    }
    if (
      handle_scroll !== undefined || handle_scale !== undefined || kinetic_scroll !== undefined ||
      tracking_mode !== undefined
    ) {
      this.apply_gesture_options(handle_scroll, handle_scale, kinetic_scroll, tracking_mode);
    }
    if (wheel_behavior !== undefined) this.gestures_cfg.wheel_behavior = wheel_behavior;
    this.sync_touch_action();
    if (localization !== undefined) this.apply_localization(localization);
    if (accessibility !== undefined) {
      if (accessibility === false) this.accessibility_handle?.detach();
      else if (this.accessibility_handle !== null) {
        this.accessibility_handle.apply_options(accessibility === true ? {} : accessibility);
      } else {
        enable_accessibility(this, accessibility === true ? {} : accessibility);
      }
    }
    // autoSize stays in `rest` (the engine stores it); the active flag is tracked TS-side.
    if (options.autoSize !== undefined) this.set_auto_size(options.autoSize);
    // Gesture-only patches strip down to an empty object; an empty patch is a no-op for the
    // engine, so don't forward it (it would still trigger the full relayout that real patches
    // get — reference applyOptions(fullUpdate) semantics — with nothing to apply).
    if (Object.keys(engine_options).some((key) => {
      const value = (engine_options as Record<string, unknown>)[key];
      return value !== undefined && (typeof value !== "object" || value === null || Object.keys(value).length > 0);
    })) {
      this.wasm.apply_options(JSON.stringify(engine_options));
    }
    this.repaint();
    for (const h of this.options_change_subs) h(options);
  }

  /**
   * reference `autoSize`: enabling hands sizing to the engine's ResizeObserver; the flag is tracked
   * TS-side for `auto_size_active()`. The engine has no disable hook yet, so turning it off is
   * tracked here only (the engine keeps observing until teardown).
   */
  private set_auto_size(on: boolean): void {
    if (on && !this.auto_size && !this.removed) {
      this.wasm.enable_auto_resize(this.container);
    }
    this.auto_size = on;
    if (on) this.bind_dpr_watcher();
    else this.unbind_dpr_watcher();
  }

  nudge_selected_drawing(dx: number, dy: number, anchor: number | null): boolean {
    const changed = this.wasm.nudge_selected_drawing(dx, dy, anchor ?? -1);
    if (changed) this.repaint();
    return changed;
  }

  select_drawing_for_accessibility(id: number): void {
    this.wasm.set_selected_drawing(id);
    this.repaint();
  }

  /** DPR-only display transitions do not reliably resize CSS bounds on every WebKit host. */
  private bind_dpr_watcher(): void {
    this.unbind_dpr_watcher();
    this.dpr_query = window.matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
    this.dpr_query.addEventListener("change", this.dpr_change_handler, { once: true });
    window.addEventListener("orientationchange", this.dpr_change_handler);
    document.addEventListener("fullscreenchange", this.dpr_change_handler);
  }

  private unbind_dpr_watcher(): void {
    this.dpr_query?.removeEventListener("change", this.dpr_change_handler);
    this.dpr_query = null;
    window.removeEventListener("orientationchange", this.dpr_change_handler);
    document.removeEventListener("fullscreenchange", this.dpr_change_handler);
  }

  auto_size_active(): boolean {
    return this.auto_size;
  }

  chart_element(): HTMLElement {
    return this.container;
  }

  /** Install the host price/time formatters (reference `localization`). Callbacks cross into wasm. */
  apply_localization(loc: localization_options): void {
    if (loc.price_formatter !== undefined) this.wasm.set_price_formatter(loc.price_formatter);
    if (loc.time_formatter !== undefined) this.wasm.set_time_formatter(loc.time_formatter);
    if (loc.locale !== undefined) this.wasm.set_locale(loc.locale);
    if (loc.date_format !== undefined) this.wasm.set_date_format(loc.date_format);
    this.repaint();
  }

  /** Resolve and store the gesture toggles; the recognizer reads the result live. */
  apply_gesture_options(
    scroll?: boolean | handle_scroll_options,
    scale?: boolean | handle_scale_options,
    kinetic?: boolean | kinetic_scroll_options,
    tracking?: tracking_mode_options,
  ): void {
    if (scroll !== undefined) apply_scroll(scroll, this.gestures_cfg);
    if (scale !== undefined) apply_scale(scale, this.gestures_cfg);
    if (kinetic !== undefined) apply_kinetic(kinetic, this.gestures_cfg);
    if (tracking !== undefined) apply_tracking(tracking, this.gestures_cfg);
    this.sync_interaction_disabled();
    this.sync_touch_action();
  }

  private sync_touch_action(): void {
    const cfg = this.gestures_cfg;
    this.overlay.style.touchAction = cfg.pan_vert_touch ? "none"
      : cfg.pan_horz_touch || cfg.pinch_zoom ? "pan-y" : "auto";
  }

  /** Resolve the reference `layout.panes.enableResize` toggle (separator drag + hover cursor). */
  apply_panes_resize(enabled: boolean): void {
    this.gestures_cfg.panes_resize = enabled;
  }

  /** Mirror the engine's master interaction switch: off only when every scroll+scale flag is. */
  private sync_interaction_disabled(): void {
    const c = this.gestures_cfg;
    const all_off =
      !c.pan && !c.pan_horz_touch && !c.pan_vert_touch && !c.wheel_scroll && !c.wheel_zoom &&
      !c.pinch_zoom && !c.axis_dblclick_reset_time && !c.axis_dblclick_reset_price &&
      !c.axis_scale_time && !c.axis_scale_price;
    this.wasm.set_interaction_disabled(all_off);
  }

  /** Update the aria-live region with a compact description of the point under the cursor. */
  announce(x: number, y: number): void {
    const params = this.build_params(x, y);
    if (params.time === null || params.series_data.size === 0) return;
    const parts = [];
    for (const [, point] of params.series_data) {
      parts.push("value" in point ? `${point.value}` : `O ${point.open} H ${point.high} L ${point.low} C ${point.close}`);
    }
    this.accessibility_handle?.announce(`Time ${params.time}: ${parts.join("; ")}`);
  }

  /** Whether the user has requested reduced motion (gates kinetic scroll). */
  prefers_reduced_motion(): boolean {
    return this.container.ownerDocument.defaultView?.matchMedia?.("(prefers-reduced-motion: reduce)")
      .matches === true;
  }

  /** Current resolved gesture toggles (read by the gesture recognizer). */
  gesture_config(): resolved_gestures {
    return this.gestures_cfg;
  }

  options(): unknown {
    return JSON.parse(this.wasm.options_json());
  }

  backend(): "webgpu" | "canvas2d" {
    return this.wasm.backend_kind() as "webgpu" | "canvas2d";
  }

  backend_status(): Readonly<backend_status> {
    return Object.freeze(JSON.parse(this.wasm.backend_status_json()) as backend_status);
  }

  /**
   * Read the engine's last-frame record into the per-chart scratch array and shape it as
   * `frame_stats`. The scratch array is the whole point: the engine writes f64 slots in place,
   * so a host polling this every frame for minutes never grows the JS heap and never triggers a
   * GC pause that would itself corrupt the measurement.
   */
  frame_stats(): frame_stats {
    const out = this.stats_scratch;
    this.wasm.frame_stats_into(out);
    const gpu = out[FRAME_STATS_SLOT.gpu_ms] as number;
    return {
      cpu_ms: out[FRAME_STATS_SLOT.cpu_ms] as number,
      // NaN is the engine's encoding for "unavailable" (no WebGPU / no timestamp-query / no
      // readback resolved yet) — see the `frame_stats.gpu_ms` docs.
      gpu_ms: Number.isNaN(gpu) ? null : gpu,
      draw_calls: out[FRAME_STATS_SLOT.draw_calls] as number,
      dropped_frames: out[FRAME_STATS_SLOT.dropped_frames] as number,
      presented_frames: out[FRAME_STATS_SLOT.presented_frames] as number,
      memory_bytes: out[FRAME_STATS_SLOT.memory_bytes] as number,
      canvas2d_ops: out[FRAME_STATS_SLOT.canvas2d_ops] as number,
      ring_overruns: out[FRAME_STATS_SLOT.ring_overruns] as number,
      gpu_buffer_allocations: out[FRAME_STATS_SLOT.gpu_buffer_allocations] as number,
      gpu_write_calls: out[FRAME_STATS_SLOT.gpu_write_calls] as number,
      gpu_uploaded_bytes: out[FRAME_STATS_SLOT.gpu_uploaded_bytes] as number,
      layout_rebuilds: out[FRAME_STATS_SLOT.layout_rebuilds] as number,
      autoscale_runs: out[FRAME_STATS_SLOT.autoscale_runs] as number,
      series_rebuilds: out[FRAME_STATS_SLOT.series_rebuilds] as number,
      drawing_rebuilds: out[FRAME_STATS_SLOT.drawing_rebuilds] as number,
      grid_rebuilds: out[FRAME_STATS_SLOT.grid_rebuilds] as number,
      overlay_rebuilds: out[FRAME_STATS_SLOT.overlay_rebuilds] as number,
      axis_rebuilds: out[FRAME_STATS_SLOT.axis_rebuilds] as number,
      text_resolutions: out[FRAME_STATS_SLOT.text_resolutions] as number,
      trading_rebuilds: out[FRAME_STATS_SLOT.trading_rebuilds] as number,
    };
  }

  time_scale(): time_scale_api {
    return this.ts;
  }

  price_scale(price_scale_id = "right", pane_index = 0): price_scale_api {
    return new price_scale_impl(this, pane_index, price_scale_id);
  }

  add_price_scale(options: price_scale_create_options, pane_index = 0): price_scale_api {
    parse_engine_result<{ target: number }>(
      this.wasm.add_price_scale_result_json(pane_index, JSON.stringify(options)),
    );
    this.repaint();
    return new price_scale_impl(this, pane_index, options.id);
  }

  price_scales(pane_index = 0): price_scale_info[] {
    return JSON.parse(this.wasm.price_scales_json(pane_index)) as price_scale_info[];
  }

  move_price_scale(id: string, side: "left" | "right", order: number, pane_index = 0): void {
    const target = undef_to_null(this.wasm.price_scale_target_by_id(pane_index, id));
    if (target === null) {
      throw new nucleuscharts_error("invalid_handle", `price scale '${id}' does not exist in pane ${pane_index}`);
    }
    parse_engine_result<object>(
      this.wasm.move_price_scale_result_json(pane_index, target, side, order),
    );
    this.repaint();
  }

  remove_price_scale(id: string, pane_index = 0): void {
    const target = undef_to_null(this.wasm.price_scale_target_by_id(pane_index, id));
    if (target === null) {
      throw new nucleuscharts_error("invalid_handle", `price scale '${id}' does not exist in pane ${pane_index}`);
    }
    parse_engine_result<object>(this.wasm.remove_price_scale_result_json(pane_index, target));
    this.repaint();
  }

  panes(): pane_api[] {
    const n = this.wasm.pane_count();
    const out: pane_api[] = [];
    for (let i = 0; i < n; i++) {
      out.push(new pane_impl(this, i));
    }
    return out;
  }

  add_pane(preserve_empty = false): pane_api {
    const index = undef_to_null(this.wasm.add_pane(preserve_empty));
    if (index === null) throw new nucleuscharts_error("resource_limit", "pane identity space is exhausted");
    this.repaint();
    return new pane_impl(this, index);
  }

  remove_pane(index: number): boolean {
    if (!this.wasm.remove_pane(index)) return false;
    this.repaint();
    return true;
  }

  swap_panes(first: number, second: number): boolean {
    if (!this.wasm.swap_panes(first, second)) return false;
    this.repaint();
    return true;
  }

  reset_view(): void {
    this.wasm.reset_view();
    this.repaint();
  }

  price_to_coordinate(price: number): number | null {
    return undef_to_null(this.wasm.price_to_coordinate(price));
  }
  coordinate_to_price(y: number): number | null {
    return undef_to_null(this.wasm.coordinate_to_price(y));
  }

  set_crosshair_position(price: number, time: time, series: series_api): void {
    const seconds = time_to_utc_seconds(time);
    // false = the engine refused the position (e.g. unknown series); reference throws, we no-op.
    if (!this.wasm.set_crosshair_position(price, seconds, series.id)) return;
    this.repaint();
    // Emit the crosshair-move at the coordinates the position resolved to, on the given series'
    // own price scale.
    const x = undef_to_null(this.wasm.time_to_coordinate(seconds));
    const y = undef_to_null(this.wasm.series_price_to_coordinate(series.id, price));
    if (x === null || y === null) return;
    this.emit_crosshair(x, y);
  }

  clear_crosshair_position(): void {
    this.wasm.clear_crosshair_position();
    this.repaint();
    this.emit_crosshair_left();
  }

  resize(width: number, height: number, dpr?: number): void {
    const ratio = dpr ?? window.devicePixelRatio ?? 1;
    const bitmap_width = Math.max(1, Math.round(width * ratio));
    const bitmap_height = Math.max(1, Math.round(height * ratio));
    for (const canvas of [this.gpu_pane, this.fallback_pane, this.plugin_canvas, this.overlay]) {
      canvas.width = bitmap_width;
      canvas.height = bitmap_height;
      canvas.style.width = `${width}px`;
      canvas.style.height = `${height}px`;
    }
    this.wasm.resize(width, height, ratio);
    this.repaint();
  }

  render(): void {
    this.repaint();
  }

  take_screenshot(add_top_layer = true, include_crosshair = true): HTMLCanvasElement {
    // Browser WebGPU canvases are presentable but are not synchronously readable through
    // CanvasRenderingContext2D.drawImage (Chromium returns transparent pixels). Repaint the current
    // engine frame, then execute that same retained frame through the already-warm Canvas2D pane.
    // This keeps the reference-style synchronous API deterministic without duplicating chart state.
    this.repaint();
    // Hiding the crosshair for the capture is a clear → snapshot → restore cycle; the whole call
    // is synchronous, so the on-screen canvases never present the crosshair-less frame.
    const restore = include_crosshair ? null : this.last_crosshair;
    if (restore !== null) {
      this.wasm.clear_crosshair();
      this.wasm.render();
    }
    this.wasm.render_canvas2d_snapshot(add_top_layer);
    const output = document.createElement("canvas");
    output.width = this.overlay.width;
    output.height = this.overlay.height;
    const ctx = output.getContext("2d");
    if (ctx === null) {
      throw new nucleuscharts_error("renderer_platform_error", "screenshot Canvas2D context is unavailable");
    }
    ctx.drawImage(this.fallback_pane, 0, 0);
    // Canvas primitives composite at pane level (the reference paints primitives on the pane
    // canvas), so they are captured regardless of `add_top_layer`. The repaint above re-ran
    // the pass, so the plugin canvas is fresh.
    ctx.drawImage(this.plugin_canvas, 0, 0);
    if (add_top_layer) {
      ctx.drawImage(this.overlay, 0, 0);
    }
    // `render_canvas2d_snapshot(false)` executes into the warm fallback surface. When Canvas2D is
    // the live backend that surface is visible, so restore its full shared chrome after copying the
    // requested pane-only bitmap. (On WebGPU this simply keeps the warm fallback current.)
    if (!add_top_layer) {
      this.wasm.render_canvas2d_snapshot(true);
    }
    if (restore !== null) {
      this.wasm.set_crosshair(restore.x, restore.y);
      this.repaint();
    }
    return output;
  }

  overlay_el(): HTMLCanvasElement {
    return this.overlay;
  }

  remove(): void {
    if (this.removed) return;
    const wasm = this.wasm;
    // Package-owned canvas extensions live outside wasm; detach each independently so one
    // throwing hook cannot prevent the rest of the chart from being released.
    for (const entry of this.canvas_primitives.splice(0)) {
      if (entry.detached) continue;
      entry.detached = true;
      try {
        entry.primitive.detached?.();
      } catch (error) {
        console.warn(`nucleuscharts: canvas primitive \`detached\` threw — ${error}`);
      }
    }
    this.removed = true;
    if (this.repaint_raf !== null) {
      cancelAnimationFrame(this.repaint_raf);
      this.repaint_raf = null;
    }
    // Stop the ring-drain loop and drop its report buffer; the engine's views over any bound
    // shared buffer go with the chart itself.
    if (this.ring_raf !== null) {
      cancelAnimationFrame(this.ring_raf);
      this.ring_raf = null;
    }
    this.ring_report = null;
    this.close_text_editor(false);
    this.stop_animation();
    this.stop_countdown_timer();
    window.removeEventListener("nucleuscharts-chart-backend-lost", this.backend_loss_handler);
    this.unbind_dpr_watcher();
    this.detach_gestures?.();
    this.observer?.disconnect();
    this.plugin_resize_observer?.disconnect();
    this.observer = null;
    this.detach_gestures = null;
    this.plugin_resize_observer = null;
    for (const series of this.series_by_id.values()) series.mark_removed();
    this.series_by_id.clear();
    this.crosshair_subs.clear();
    this.click_subs.clear();
    this.dbl_click_subs.clear();
    this.visible_logical_range_subs.clear();
    this.visible_time_range_subs.clear();
    this.size_change_subs.clear();
    this.series_added_subs.clear();
    this.series_removed_subs.clear();
    this.options_change_subs.clear();
    this.delta_tooltip_range_listeners.clear();
    this.tool_listener = null;
    this.tool_change_subs.clear();
    this.drawing_created_subs.clear();
    this.accessibility_handle?.detach();
    this.accessibility_handle = null;
    wasm.dispose();
    wasm.free();
    this.wasm_instance = null;
    this.gpu_pane.remove();
    this.fallback_pane.remove();
    this.plugin_canvas.remove();
    this.overlay.remove();
  }
}
