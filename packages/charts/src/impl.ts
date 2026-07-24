/**
 * Handle implementations over the wasm engine: series, time scale, price scale, pane, chart.
 * Extracted from `index.ts`.
 */

// @ts-ignore -- pkg is a build artifact, present after build:wasm
import init, { AionChart } from "../pkg/aion_wasm.js";

import { install_gestures } from "./gestures.js";
import type { pane_primitive, pane_primitive_handle, series_primitive, series_primitive_handle } from "./primitives.js";
import type { canvas_primitive, canvas_primitive_handle, canvas_pane_view } from "./canvas_plugins.js";
import { create_canvas_render_target } from "./canvas_plugins.js";
import type { custom_series_item, custom_series_pane_view } from "./custom_series.js";
import type {
  bars_info, chart_api, chart_options, data_changed_handler, dbl_click_handler,
  deep_partial, handle_scale_options, handle_scroll_options, kinetic_scroll_options,
  last_value_data, localization_options, logical_range,
  mismatch_direction, mouse_event_handler, mouse_event_params, ohlc_data, pane_api, price_line_api, price_line_options,
  price_range, price_scale_api, price_scale_options, series_api, series_data, series_kind,
  series_marker, series_marker_options, series_options, single_value_data, size_change_handler, time, time_range,
  time_scale_api, time_scale_options, tracking_mode_options, visible_logical_range_handler, visible_time_range_handler,
} from "./types.js";
import { KIND_TO_U8, LINE_STYLE_TO_U8, LINE_TYPE_TO_U8 } from "./types.js";

// ---------------------------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------------------------

let init_promise: Promise<unknown> | null = null;
/**
 * Instantiate the wasm module once per page. `wasm_url` overrides the default asset resolution
 * (`new URL("aion_wasm_bg.wasm", import.meta.url)` beside the bundle) — the escape hatch for
 * bundlers that relocate the JS away from the .wasm (e.g. Vite's dev pre-bundler). Only the
 * first call's argument takes effect.
 */
export function ensure_init(wasm_url?: string | URL): Promise<unknown> {
  if (init_promise === null) {
    init_promise = wasm_url !== undefined ? init(wasm_url) : init();
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
export function countdown_timer_needed(
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
] as const;

/** Engine kind ordinal → public kind name (index-aligned with `KIND_TO_U8`). */
const KIND_NAMES = ["candlestick", "bar", "line", "area", "histogram", "baseline", "custom"] as const;

/**
 * Index of the stacked pane containing CSS-y `y`, counted from the pane separators (shared by
 * the gesture recognizer and the mouse-event params).
 */
export function pane_index_of_y(separator_ys: Float64Array, y: number): number {
  let pane = 0;
  for (const sy of separator_ys) if (y > sy) pane += 1;
  return pane;
}

/** Pack a data array into the six Float64Arrays the engine expects (single-value → o=h=l=c). */
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
      body_colors = pack_color_channel(body_colors, n, i, d.color);
    } else if ("open" in d) {
      open[i] = d.open;
      high[i] = d.high;
      low[i] = d.low;
      close[i] = d.close;
      body_colors = pack_color_channel(body_colors, n, i, d.color);
      wick_colors = pack_color_channel(wick_colors, n, i, d.wick_color);
      border_colors = pack_color_channel(border_colors, n, i, d.border_color);
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
    console.warn(`aion: ignoring unparseable data point color "${css}"`);
    return undefined;
  }
  return packed;
}

class series_impl implements series_api {
  protected readonly data_changed_subs = new Set<data_changed_handler>();
  private removed = false;

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
  protected assert_live(): void {
    if (this.removed) throw new Error("aion: this series has been removed from the chart");
  }

  set_data(data: readonly series_data[]): void {
    this.assert_live();
    const p = pack(data);
    this.chart.wasm.set_series_data_typed(this.id, p.times, p.open, p.high, p.low, p.close);
    // set_series_data resets point colors, so per-point channels must be applied after it.
    if (p.body_colors !== undefined || p.wick_colors !== undefined || p.border_colors !== undefined) {
      this.chart.wasm.set_series_point_colors(this.id, p.body_colors, p.wick_colors, p.border_colors);
    }
    this.chart.sync_countdown_timer();
    this.chart.repaint();
    for (const handler of this.data_changed_subs) handler("full");
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
    const body = "value" in point || "open" in point ? point_color_to_u32(point.color) : undefined;
    const wick = "open" in point ? point_color_to_u32(point.wick_color) : undefined;
    const border = "open" in point ? point_color_to_u32(point.border_color) : undefined;
    // Series-scoped streaming: append a new time point or replace the last on this series.
    this.chart.wasm.update_series_bar_styled(
      this.id, time_to_utc_seconds(point.time), o, h, l, c, body, wick, border,
    );
    // Data arriving on a countdown-enabled series can start the timer (cheap flag check).
    if (this.chart.countdown_series_present) this.chart.sync_countdown_timer();
    this.chart.repaint();
    for (const handler of this.data_changed_subs) handler("update");
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

  apply_options(options: Partial<series_options>): void {
    this.assert_live();
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
    if (options.pane !== undefined && options.pane > 0) {
      this.chart.wasm.set_series_pane(this.id, options.pane, options.pane_stretch ?? 1);
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
    if (options.overlay) {
      const m = options.scale_margins ?? { top: 0.8, bottom: 0 };
      this.chart.wasm.set_series_overlay(this.id, m.top, m.bottom);
    } else if (options.price_scale_id !== undefined || options.priceScaleId !== undefined) {
      const id = options.priceScaleId ?? options.price_scale_id;
      const target = id === "left" ? 1 : id === "" ? 2 : 0;
      this.chart.wasm.set_series_price_scale(this.id, target);
      if (target === 2) {
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

  options(): series_options {
    this.assert_live();
    return JSON.parse(this.chart.wasm.series_options_json(this.id)) as series_options;
  }

  set_type(kind: series_kind): void {
    this.assert_live();
    if (kind === "custom") {
      console.warn("aion: set_type() cannot convert a series to 'custom'; use chart.add_custom_series");
      return;
    }
    if (this.id === 0) {
      this.chart.wasm.set_series_type(KIND_TO_U8[kind]);
    } else {
      console.warn("aion: set_type() currently supports the primary series only");
    }
    this.chart.repaint();
  }

  move_to_pane(pane_index: number, stretch = 1): void {
    this.assert_live();
    this.chart.wasm.set_series_pane(this.id, pane_index, stretch);
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
    // Normalize marker times to UTC seconds so business-day/string forms match their data points
    // (the engine's marker JSON expects a numeric time).
    const normalized = markers.map((mk) => ({ ...mk, time: time_to_utc_seconds(mk.time) }));
    this.chart.wasm.set_series_markers(this.id, JSON.stringify(normalized));
    this.chart.repaint();
  }

  price_scale(): price_scale_api {
    const pane = undef_to_null(this.chart.wasm.series_pane_index(this.id)) ?? 0;
    const target = undef_to_null(this.chart.wasm.series_price_scale_id(this.id)) ?? 0;
    return new price_scale_impl(this.chart, pane, target);
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
    this.chart.repaint();
    for (const handler of this.data_changed_subs) handler("update");
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
    console.warn("aion: set_type() does not apply to a custom series");
  }
}

class time_scale_impl implements time_scale_api {
  /** Invalidation token: each new scroll call or user gesture supersedes an in-flight animation. */
  private scroll_anim_token = 0;

  constructor(private readonly chart: chart_impl) {}

  scroll_position(): number {
    return this.chart.wasm.scroll_position();
  }
  /** Invalidate any in-flight animated scroll (a new scroll call or user gesture takes over). */
  cancel_scroll_animation(): void {
    this.scroll_anim_token += 1;
  }
  scroll_to_position(position: number, animated: boolean): void {
    const token = ++this.scroll_anim_token;
    if (!animated || this.chart.prefers_reduced_motion()) {
      this.chart.wasm.scroll_to_position(position);
      this.chart.repaint();
      return;
    }
    const start = this.chart.wasm.scroll_position();
    if (start === position) return;
    const t0 = performance.now();
    const step = () => {
      if (token !== this.scroll_anim_token) return; // superseded mid-flight
      const t = Math.min(1, (performance.now() - t0) / SCROLL_ANIM_MS);
      const eased = 1 - Math.pow(1 - t, 3); // cubic ease-out
      this.chart.wasm.scroll_to_position(start + (position - start) * eased);
      this.chart.repaint();
      if (t < 1) requestAnimationFrame(step);
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
  constructor(
    private readonly chart: chart_impl,
    private readonly pane: number,
    private readonly target: number,
  ) {}

  apply_options(options: deep_partial<price_scale_options>): void {
    if (options.mode !== undefined) {
      this.chart.wasm.set_price_scale_mode(this.pane, this.target, options.mode);
    }
    if (options.auto_scale !== undefined) {
      this.chart.wasm.set_price_scale_auto_scale(this.pane, this.target, options.auto_scale);
    }
    if (options.invert_scale !== undefined) {
      this.chart.wasm.set_price_scale_inverted(this.pane, this.target, options.invert_scale);
    }
    if (options.scale_margins !== undefined) {
      const current = this.options().scale_margins;
      this.chart.wasm.set_price_scale_margins(
        this.pane,
        this.target,
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
      this.chart.wasm.price_scale_apply_options_json(this.pane, this.target, JSON.stringify(json_patch));
    }
    this.chart.repaint();
  }

  options(): price_scale_options {
    return JSON.parse(this.chart.wasm.price_scale_options_json(this.pane, this.target)) as price_scale_options;
  }

  width(): number {
    return this.chart.wasm.price_scale_width(this.pane, this.target);
  }

  set_visible_range(range: price_range): void {
    this.chart.wasm.set_price_scale_visible_range(this.pane, this.target, range.from, range.to);
    this.chart.repaint();
  }

  get_visible_range(): price_range | null {
    const range = this.chart.wasm.price_scale_visible_range(this.pane, this.target);
    return range.length === 2 ? { from: range[0]!, to: range[1]! } : null;
  }

  set_auto_scale(on: boolean): void {
    this.chart.wasm.set_price_scale_auto_scale(this.pane, this.target, on);
    this.chart.repaint();
  }
}

class pane_impl implements pane_api {
  constructor(private readonly chart: chart_impl, private index: number) {}

  pane_index(): number {
    return this.index;
  }
  get_height(): number {
    return this.chart.wasm.pane_height(this.index);
  }
  set_height(height: number): void {
    this.chart.wasm.set_pane_height(this.index, height);
    this.chart.repaint();
  }
  get_stretch_factor(): number {
    return this.chart.wasm.pane_stretch(this.index);
  }
  set_stretch_factor(factor: number): void {
    this.chart.wasm.set_pane_stretch(this.index, factor);
    this.chart.repaint();
  }
  move_to(target: number): boolean {
    // The engine answers false for a rejected move (e.g. a stale index after remove_pane);
    // on success this handle follows the pane to its new index.
    if (!this.chart.wasm.pane_move_to(this.index, target)) return false;
    this.index = target;
    this.chart.repaint();
    return true;
  }
  preserve_empty_pane(): boolean {
    // The engine answers false for a stale index (e.g. after remove_pane).
    return this.chart.wasm.pane_preserve_empty(this.index);
  }
  set_preserve_empty_pane(flag: boolean): void {
    this.chart.wasm.pane_set_preserve_empty(this.index, flag);
    this.chart.repaint();
  }
  get_series(): series_api[] {
    // Live handles from the engine's id list (empty for a stale index after remove_pane).
    const ids = this.chart.wasm.pane_series_ids(this.index);
    const out: series_api[] = [];
    for (const id of ids) {
      out.push(this.chart.series_handle(id));
    }
    return out;
  }
  price_scale(id: "left" | "right" | ""): price_scale_api {
    return this.chart.price_scale(id, this.index);
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
    const id = this.chart.wasm.attach_pane_primitive(this.index, adapted);
    this.chart.repaint();
    return new pane_primitive_handle_impl(this.chart, id);
  }

  attach_canvas_primitive(primitive: canvas_primitive): canvas_primitive_handle {
    // No wasm involvement: the package owns the plugin canvas and the per-frame pass.
    return this.chart.attach_canvas_primitive(this.index, primitive);
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
  detached: boolean;
}

/** Detach handle for a registered canvas primitive (the TS registry owns the lifecycle). */
class canvas_primitive_handle_impl implements canvas_primitive_handle {
  private detached = false;

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

export class chart_impl implements chart_api {
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
    panes_resize: true,
    tracking_exit_mode: "on_next_tap",
  };
  private a11y_live: HTMLElement | null = null;
  private readonly ts = new time_scale_impl(this);
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
  private last_visible_logical_range: logical_range | null;
  private last_visible_time_range: time_range | null;
  private last_ts_width: number;
  private last_ts_height: number;
  private auto_size: boolean;
  /** Crosshair position tracked TS-side (for crosshair-less screenshots); `null` when hidden. */
  private last_crosshair: { x: number; y: number } | null = null;
  /** Last hover hit-test result (Phase C-d), refreshed on crosshair moves; feeds event params. */
  private hover: { series_id: number | null; object_id: string | null; cursor: string | null } | null = null;
  private readonly backend_runtime_id: number;
  private anim_frame: number | null = null;
  /** The 1s candle-close countdown interval; `null` while no countdown is visible. */
  private countdown_timer: ReturnType<typeof setInterval> | null = null;
  /** True while any pointer/touch is down — pauses the countdown tick so it can't repaint mid-gesture. */
  private interacting = false;

  /** The gesture recognizer marks pointer/touch activity (down = true, all-up = false). */
  set_interacting(active: boolean): void {
    this.interacting = active;
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
  private readonly backend_loss_handler = (event: Event): void => {
    if ((event as CustomEvent<number>).detail !== this.backend_runtime_id || this.removed) return;
    // The wgpu callback may arrive from a promise microtask. Defer the repaint once more so the
    // callback stack is fully unwound before Rust drops GPU resources and paints the warm 2D pane.
    queueMicrotask(() => this.repaint());
  };

  constructor(
    readonly wasm: AionChart,
    private readonly container: HTMLElement,
    private readonly gpu_pane: HTMLCanvasElement,
    private readonly fallback_pane: HTMLCanvasElement,
    private readonly plugin_canvas: HTMLCanvasElement,
    private readonly overlay: HTMLCanvasElement,
    auto_size: boolean,
  ) {
    const plugin_ctx = plugin_canvas.getContext("2d");
    if (plugin_ctx === null) throw new Error("aion: plugin canvas 2D context is unavailable");
    this.plugin_ctx = plugin_ctx;
    this.backend_runtime_id = this.wasm.backend_runtime_id();
    window.addEventListener("aion-chart-backend-lost", this.backend_loss_handler);
    this.last_visible_logical_range = this.read_visible_logical_range();
    this.last_visible_time_range = this.read_visible_time_range();
    this.last_ts_width = this.wasm.time_scale_width();
    this.last_ts_height = this.wasm.time_scale_height();
    this.auto_size = auto_size;
    this.init_accessibility();
    this.detach_gestures = install_gestures(this);
    if (auto_size) {
      this.wasm.enable_auto_resize(container);
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

  /** Repaint unless torn down. Named distinctly from the public `render` for internal use. */
  repaint(): void {
    if (!this.removed) {
      this.wasm.render();
      this.run_canvas_primitives();
      this.emit_visible_range_changes();
    }
  }

  /**
   * Canvas primitives (plugin platform Phase C-e): after the engine frame, paint every
   * attached canvas primitive onto the plugin overlay. The canvas is package-owned host DOM —
   * no wasm involvement — so ordering with the axis chrome is compositor-level: the plugin
   * canvas sits above the pane canvases and below the axis/input overlay. Its backing store
   * tracks the engine-sized overlay's (bitmap = css * dpr at every size/DPR change), and
   * assigning `width`/`height` clears, so a resize never leaves stale pixels behind.
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
    const normal: canvas_pane_view["renderer"][] = [];
    const top: canvas_pane_view["renderer"][] = [];
    for (const entry of this.canvas_primitives) {
      const primitive = entry.primitive;
      try {
        primitive.update_all_views?.();
      } catch (error) {
        console.warn(`aion: canvas primitive \`update_all_views\` threw — ${error}`);
      }
      let views;
      try {
        views = primitive.pane_views?.();
      } catch (error) {
        console.warn(`aion: canvas primitive \`pane_views\` threw — ${error}`);
        continue;
      }
      for (const view of views ?? []) {
        (view.z_order === "top" ? top : normal).push(view.renderer);
      }
    }
    for (const renderer of [...normal, ...top]) {
      try {
        renderer(target);
      } catch (error) {
        console.warn(`aion: canvas primitive renderer threw — ${error}`);
      }
    }
  }

  /** Register a canvas primitive (Phase C-e) on behalf of `pane_impl`. */
  attach_canvas_primitive(pane_index: number, primitive: canvas_primitive): canvas_primitive_handle {
    const entry: canvas_primitive_entry = { primitive, detached: false };
    this.canvas_primitives.push(entry);
    try {
      primitive.attached?.({ pane_index });
    } catch (error) {
      console.warn(`aion: canvas primitive \`attached\` threw — ${error}`);
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
      console.warn(`aion: canvas primitive \`detached\` threw — ${error}`);
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

  add_series(kind: series_kind, options?: Partial<series_options>): series_api {
    if (kind === "custom") {
      throw new Error("aion: add_series does not accept 'custom'; use add_custom_series(pane_view)");
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
      throw new Error("aion: add_custom_series needs a pane view with `price_value_builder` and `render` (reference `ensure(customPaneView)`)");
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
    if (id === 0xffffffff) throw new Error("aion: add_custom_series was rejected by the engine");
    const series = new custom_series_impl(id, this);
    this.series_by_id.set(id, series);
    // reference createCustomSeriesDefinition: the view's defaultOptions merge UNDER the caller's.
    const merged = { ...(pane_view.default_options ?? {}), ...(options ?? {}) } as Partial<series_options>;
    if (Object.keys(merged).length > 0) series.apply_options(merged);
    return series;
  }

  remove_series(series: series_api): void {
    const impl = this.series_by_id.get(series.id);
    // Ignore a foreign handle or one already removed (idempotent, matching reference leniency).
    if (!impl) return;
    // The engine tombstones the primary series (id 0) safely, so no id is refused here.
    if (!this.wasm.remove_series(series.id)) return;
    impl.mark_removed();
    this.series_by_id.delete(series.id);
    this.sync_countdown_timer();
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
      series = new series_impl(id, KIND_NAMES[kind] ?? "candlestick", this);
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
    if (id === 0xffffffff) throw new Error("aion: invalid indicator configuration");
    const series = new series_impl(id, "line", this);
    this.series_by_id.set(id, series);
    if (options) series.apply_options(options);
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
    if (ids.length !== 3) throw new Error("aion: invalid Bollinger configuration");
    return [this.indicator_series(ids[0]!, options), this.indicator_series(ids[1]!, options), this.indicator_series(ids[2]!, options)];
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

  /** Invalidate any in-flight animated `scroll_to_position` (user gestures take over scrolling). */
  cancel_scroll_animation(): void {
    this.ts.cancel_scroll_animation();
  }

  /**
   * Which pane contains the pane-origin CSS point (x, y), or `null` when it falls on an axis
   * strip (price/time) or outside the pane area. Backs `mouse_event_params.pane_index`.
   */
  pane_index_at(x: number, y: number): number | null {
    if (x < 0 || x > this.wasm.time_scale_width()) return null;
    const pane_bottom = this.overlay.getBoundingClientRect().height - this.wasm.time_scale_height();
    if (y < 0 || y > pane_bottom) return null;
    return pane_index_of_y(this.wasm.pane_separator_ys(), y);
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
    if (this.click_subs.size === 0) return;
    const params = this.build_params(x, y);
    for (const h of this.click_subs) h(params);
  }

  /** Announce the current visible time range to assistive tech (used after keyboard navigation). */
  announce_view(): void {
    if (!this.a11y_live) return;
    const r = this.wasm.visible_time_range();
    this.a11y_live.textContent =
      r.length === 2 ? `Showing time ${r[0]} to ${r[1]}` : "No data";
  }

  emit_dbl_click(x: number, y: number): void {
    if (this.dbl_click_subs.size === 0) return;
    const params = this.build_params(x, y);
    for (const h of this.dbl_click_subs) h(params);
  }

  apply_options(options: deep_partial<chart_options>): void {
    // handle_scroll / handle_scale / kinetic_scroll / tracking_mode (gestures), the pane-resize
    // toggle, and localization (JS callbacks) are package-level; intercept and strip them so only
    // engine-owned, JSON-serializable options reach the wasm store.
    const { handle_scroll, handle_scale, kinetic_scroll, tracking_mode, localization, ...rest } =
      options as deep_partial<chart_options> & {
        handle_scroll?: boolean | handle_scroll_options;
        handle_scale?: boolean | handle_scale_options;
        kinetic_scroll?: boolean | kinetic_scroll_options;
        tracking_mode?: tracking_mode_options;
        localization?: localization_options;
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
    if (localization !== undefined) this.apply_localization(localization);
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

  /** Set up the accessible wrapper: focusable role on the overlay + an aria-live status region. */
  private init_accessibility(): void {
    this.container.setAttribute("role", "group");
    if (!this.container.hasAttribute("aria-label")) {
      this.container.setAttribute("aria-label", "Financial chart");
    }
    this.overlay.tabIndex = 0;
    this.overlay.setAttribute("role", "application");
    this.overlay.setAttribute(
      "aria-label",
      "Chart. Arrow keys pan, plus and minus zoom, Home fits content, Escape clears the crosshair.",
    );
    const live = this.container.ownerDocument.createElement("div");
    live.setAttribute("aria-live", "polite");
    live.setAttribute("role", "status");
    // Visually hidden but available to assistive tech.
    live.style.cssText =
      "position:absolute;width:1px;height:1px;margin:-1px;padding:0;overflow:hidden;clip:rect(0 0 0 0);white-space:nowrap;border:0;";
    this.container.appendChild(live);
    this.a11y_live = live;
  }

  /** Update the aria-live region with a compact description of the point under the cursor. */
  announce(x: number, y: number): void {
    if (!this.a11y_live) return;
    const params = this.build_params(x, y);
    if (params.time === null || params.series_data.size === 0) return;
    const parts = [];
    for (const [, point] of params.series_data) {
      parts.push("value" in point ? `${point.value}` : `O ${point.open} H ${point.high} L ${point.low} C ${point.close}`);
    }
    this.a11y_live.textContent = `Time ${params.time}: ${parts.join("; ")}`;
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

  time_scale(): time_scale_api {
    return this.ts;
  }

  price_scale(price_scale_id: "left" | "right" | "" = "right", pane_index = 0): price_scale_api {
    const target = price_scale_id === "left" ? 1 : price_scale_id === "" ? 2 : 0;
    return new price_scale_impl(this, pane_index, target);
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
    const index = this.wasm.add_pane(preserve_empty);
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
    this.wasm.resize(width, height, dpr ?? window.devicePixelRatio ?? 1);
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
    this.wasm.render_canvas2d_snapshot();
    const output = document.createElement("canvas");
    output.width = this.overlay.width;
    output.height = this.overlay.height;
    const ctx = output.getContext("2d");
    if (ctx === null) throw new Error("aion: screenshot Canvas2D context is unavailable");
    ctx.drawImage(this.fallback_pane, 0, 0);
    // Canvas primitives composite at pane level (the reference paints primitives on the pane
    // canvas), so they are captured regardless of `add_top_layer`. The repaint above re-ran
    // the pass, so the plugin canvas is fresh.
    ctx.drawImage(this.plugin_canvas, 0, 0);
    if (add_top_layer) {
      ctx.drawImage(this.overlay, 0, 0);
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
    this.removed = true;
    this.stop_animation();
    this.stop_countdown_timer();
    window.removeEventListener("aion-chart-backend-lost", this.backend_loss_handler);
    this.detach_gestures?.();
    this.observer?.disconnect();
    this.plugin_resize_observer?.disconnect();
    this.gpu_pane.remove();
    this.fallback_pane.remove();
    this.plugin_canvas.remove();
    this.overlay.remove();
  }
}
