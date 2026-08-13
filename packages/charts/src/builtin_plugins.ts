/**
 * Built-in plugins (plugin platform Phase 3.5) — Nucleus's ports of the reference charting library v5's own
 * plugin surface, re-expressed on the primitive platform (primitives.ts):
 *
 * - {@link create_series_markers} mirrors the reference's `createSeriesMarkers` (plugins/series-markers):
 *   a series primitive whose pane view records the exact geometry the engine's built-in
 *   marker builder emits (crates/nucleuscharts_engine/src/frame/series_geometry.rs `build_markers_frame`,
 *   ported below with `Math.fround` discipline so the decoded prims are bit-identical), with
 *   marker text painted through the overlay-text hook (the engine's own marker text path is
 *   the overlay too — frame/axis.rs `append_marker_labels`).
 * - {@link create_text_watermark} mirrors the reference's `createTextWatermark` (plugins/text-watermark):
 *   a pane primitive laying its lines out from the `text_views` info and painting through the
 *   overlay-text hook (the same slot the engine's chart-level `watermark` option paints in).
 *
 * Known divergences from the engine's built-in markers (parity is pixel-exact with
 * `auto_scale: false` on both sides):
 * - The platform's `autoscale_info` contributes `{min, max}` price bounds, while the engine's
 *   marker auto-scale applies pixel internal margins to the price scale. The plugin expands
 *   the visible price range by the margins' price equivalent (same margin math, frame/mod.rs
 *   `marker_auto_scale_margins`) — visually equivalent headroom, not bit-identical scaling.
 * - Marker math derives the bar spacing and pixel ratio from the draw context's nominal `dpr`;
 *   bit-exactness with the engine holds at integral device pixel ratios (the fixture's 1.5
 *   included, where the effective and nominal ratios coincide).
 */

import { time_to_utc_seconds } from "./impl.js";
import type {
  pane_primitive,
  pane_primitive_handle,
  primitive_text_context,
  primitive_text_view,
  primitive_z_order,
  series_primitive,
  series_primitive_draw_context,
} from "./primitives.js";
import type { pane_api, series_api, series_marker } from "./types.js";

// ---------------------------------------------------------------------------------------------
// Series markers (reference plugins/series-markers)
// ---------------------------------------------------------------------------------------------

/** Options for {@link create_series_markers} (reference `SeriesMarkersOptions`). */
export interface series_markers_options {
  /**
   * Expand the owning series' price scale so marker shapes stay visible (reference `autoScale`,
   * default `true`). See the module header for the pixel-margin vs price-bounds divergence.
   */
  auto_scale?: boolean;
  /** Layer the markers paint into (default `"normal"` — above the series, like the engine's). */
  z_order?: primitive_z_order;
}

/** Handle returned by {@link create_series_markers} (reference `ISeriesMarkersPluginApi`). */
export interface series_markers_handle {
  /** Replace the markers (pass `[]` to remove them all) and repaint. */
  set_markers(markers: readonly series_marker[]): void;
  /** The current markers array. */
  markers(): readonly series_marker[];
  /** Detach the plugin from the series and repaint. */
  detach(): void;
}

// The engine's marker sizing buckets (crates/nucleuscharts_engine/src/frame/mod.rs), ported so plugin
// markers resolve to the same pixel sizes as engine markers at every bar spacing.
function ceiled_odd(value: number): number {
  const ceiled = Math.ceil(value);
  return ceiled % 2 === 0 ? ceiled - 1 : ceiled;
}

function ceiled_even(value: number): number {
  const ceiled = Math.ceil(value);
  return ceiled % 2 !== 0 ? ceiled - 1 : ceiled;
}

function clamp_spacing(bar_spacing: number): number {
  return Math.min(Math.max(bar_spacing, 12), 30);
}

function marker_envelope_size(bar_spacing: number): number {
  return ceiled_even(ceiled_odd(clamp_spacing(bar_spacing)));
}

function marker_shape_size(envelope: number, coefficient: number): number {
  return ceiled_odd(Math.min(Math.max(envelope, 12), 30) * coefficient);
}

function marker_margin(bar_spacing: number): number {
  return Math.max(ceiled_odd(clamp_spacing(bar_spacing) * 0.1), 3);
}

// `Math.fround` mirrors the engine's f32 geometry ops so plugin commands decode to the same
// prim bits the engine's built-in marker builder emits.
const f32 = Math.fround;

type normalized_position = "above" | "below" | "in_bar";

interface normalized_marker {
  time: number;
  position: normalized_position;
  shape: "circle" | "square" | "arrowUp" | "arrowDown";
  color: string;
  text: string;
}

/** A non-whitespace bar of the owning series (single-value data maps to high=low=close). */
interface marker_bar {
  high: number;
  low: number;
  close: number;
}

/** Per-marker text anchor cached by the pane-view renderer for the `text_views` pass. */
interface text_anchor {
  text: string;
  color: string;
  x: number;
  /** Bitmap y of the marker's anchor price (high/low/close by position). */
  y_anchor: number;
  position: normalized_position;
}

function normalize_markers(markers: readonly series_marker[]): normalized_marker[] {
  return markers.map((marker) => ({
    // Normalize times to UTC seconds so business-day/string forms match their data points
    // (the same boundary conversion the engine marker path applies).
    time: time_to_utc_seconds(marker.time),
    position:
      marker.position === "belowBar" || marker.position === "below"
        ? "below"
        : marker.position === "inBar"
          ? "in_bar"
          : "above",
    shape: marker.shape ?? "circle",
    // The engine's marker fallback color (nucleuscharts_wasm inner_api.rs `set_series_markers`).
    color: marker.color === undefined || marker.color === "" ? "#2196f3" : marker.color,
    text: marker.text ?? "",
  }));
}

/**
 * The series primitive behind {@link create_series_markers}. Data and the visible range are
 * cached outside the render hooks (chart APIs are off-limits mid-render): data at create and
 * on every `subscribe_data_changed` callback, the visible logical range in `autoscale_info`
 * (the host calls it every frame, before the views run).
 */
class series_markers_primitive implements series_primitive {
  private request_update?: () => void;
  private normalized: normalized_marker[] = [];
  private raw: readonly series_marker[] = [];
  private readonly bars = new Map<number, marker_bar>();
  private times: number[] = [];
  private visible_from: number | null = null;
  private visible_to: number | null = null;
  private text_anchors: text_anchor[] = [];
  private spacing_media = 0;
  private slope: { price_per_media_px: number; min: number; max: number } | null = null;

  constructor(
    private readonly series: series_api,
    private readonly auto_scale: boolean,
    private readonly z_order: primitive_z_order,
  ) {
    this.refresh_data();
  }

  attached(params: { series_id: number; pane_index?: number; request_update?: () => void }): void {
    this.request_update = params.request_update;
    this.series.subscribe_data_changed(this.on_data_changed);
  }

  detached(): void {
    this.series.unsubscribe_data_changed(this.on_data_changed);
    this.request_update = undefined;
  }

  private readonly on_data_changed = (): void => {
    this.refresh_data();
    this.request_update?.();
  };

  private refresh_data(): void {
    this.bars.clear();
    const data = this.series.data();
    const times: number[] = [];
    for (const point of data) {
      const time = point.time as number;
      times.push(time);
      if ("value" in point && point.value !== undefined) {
        this.bars.set(time, { high: point.value, low: point.value, close: point.value });
      } else if ("open" in point && point.open !== undefined) {
        this.bars.set(time, { high: point.high, low: point.low, close: point.close });
      }
      // A whitespace point carries no bar, so a marker anchored to it draws nothing — the
      // engine's whitespace rule (series-markers pane-view `getPrice` returns undefined).
    }
    this.times = times;
  }

  set_markers(markers: readonly series_marker[]): void {
    this.raw = markers;
    this.normalized = normalize_markers(markers);
    if (this.request_update === undefined) return;
    this.request_update();
    if (this.auto_scale && this.normalized.length > 0) {
      // The autoscale contribution needs the scale slope the renderer caches mid-render, so
      // it can only take effect from the NEXT render — schedule one. (The engine's pixel
      // internal margins apply synchronously; see the module header for the divergence.)
      queueMicrotask(() => this.request_update?.());
    }
  }

  markers(): readonly series_marker[] {
    return this.raw;
  }

  pane_views() {
    return [
      {
        z_order: this.z_order,
        renderer: (ctx: series_primitive_draw_context) => this.render_markers(ctx),
      },
    ];
  }

  /**
   * Record every visible marker's shape, porting `build_markers_frame` (see the module
   * header). Also caches the text anchors for `text_views` and the scale slope for
   * `autoscale_info` — both consumed later in this same render pass.
   */
  private render_markers(ctx: series_primitive_draw_context): void {
    this.text_anchors = [];
    this.slope = null;
    const lx0 = ctx.logical_to_x(0);
    const lx1 = ctx.logical_to_x(1);
    if (lx0 === null || lx1 === null) return;
    const spacing_px = lx1 - lx0;
    if (spacing_px <= 0) return;
    // Media-px bar spacing for the engine's sizing buckets (exact at integral dpr).
    const spacing = spacing_px / ctx.dpr;
    this.spacing_media = spacing;
    const envelope = marker_envelope_size(spacing);
    const half_envelope = f32(envelope * 0.5 * ctx.dpr);
    const margin = f32(marker_margin(spacing) * ctx.dpr);
    const from = this.visible_from;
    const to = this.visible_to;
    for (const marker of this.normalized) {
      // `time_to_x` resolves only exact bar times (reference no-snap), so markers at non-bar times
      // are skipped like the engine's `binary_search` miss.
      const x = ctx.time_to_x(marker.time);
      if (x === null) continue;
      const row = this.bars.get(marker.time);
      if (row === undefined) continue;
      if (from !== null && to !== null) {
        // The engine draws only markers whose merged-time index lies in the visible strict
        // range; recover that index from the (linear) logical converter.
        const index = Math.round((x - lx0) / spacing_px);
        if (index < from || index > to) continue;
      }
      const y_high = ctx.price_to_y(row.high);
      const y_low = ctx.price_to_y(row.low);
      const y_close = ctx.price_to_y(row.close);
      if (y_high === null || y_low === null || y_close === null) continue;
      let y: number;
      if (marker.position === "above") {
        y = f32(f32(y_high) - half_envelope - margin);
      } else if (marker.position === "below") {
        y = f32(f32(y_low) + half_envelope + margin);
      } else {
        y = f32(y_close);
      }
      const x32 = f32(x);
      if (marker.shape === "square") {
        const size = f32(marker_shape_size(envelope, 0.7) * ctx.dpr);
        const half = f32(size * 0.5);
        ctx.round_rect(f32(x32 - half), f32(y - half), size, size, 0, marker.color);
      } else if (marker.shape === "arrowUp" || marker.shape === "arrowDown") {
        const arrow_size = marker_shape_size(envelope, 1.0);
        const half_arrow = f32((arrow_size - 1.0) * 0.5 * ctx.dpr);
        const base_size = ceiled_odd(envelope / 2.0);
        const half_base = f32((base_size - 1.0) * 0.5 * ctx.dpr);
        const up = marker.shape === "arrowUp";
        ctx.triangle(
          x32, f32(y + (up ? -half_arrow : half_arrow)),
          f32(x32 - half_arrow), y,
          f32(x32 + half_arrow), y,
          marker.color,
        );
        ctx.round_rect(
          f32(x32 - half_base),
          up ? y : f32(y - half_arrow),
          f32(half_base * 2),
          half_arrow,
          0,
          marker.color,
        );
      } else {
        const radius = f32((marker_shape_size(envelope, 0.8) - 1.0) * 0.5 * ctx.dpr);
        ctx.circle(x32, y, radius, marker.color);
      }
      if (marker.text !== "") {
        const y_anchor = marker.position === "above" ? y_high : marker.position === "below" ? y_low : y_close;
        this.text_anchors.push({
          text: marker.text,
          color: marker.color,
          x: x32,
          y_anchor: f32(y_anchor),
          position: marker.position,
        });
      }
    }
    this.cache_autoscale_slope(ctx, from, to);
  }

  /**
   * Cache the visible data range and the price-per-media-px slope for the autoscale
   * contribution, probed through the draw context's converter (the mid-render read path).
   */
  private cache_autoscale_slope(
    ctx: series_primitive_draw_context,
    from: number | null,
    to: number | null,
  ): void {
    if (!this.auto_scale || this.normalized.length === 0 || from === null || to === null) return;
    const lo = Math.max(0, Math.ceil(from));
    const hi = Math.min(this.times.length - 1, Math.floor(to));
    let min = Infinity;
    let max = -Infinity;
    for (let index = lo; index <= hi; index += 1) {
      const row = this.bars.get(this.times[index]!);
      if (row === undefined) continue;
      min = Math.min(min, row.low);
      max = Math.max(max, row.high);
    }
    if (!(min <= max)) return;
    const y_min = ctx.price_to_y(min);
    const y_max = ctx.price_to_y(max);
    if (y_min === null || y_max === null || y_min === y_max) return;
    this.slope = {
      price_per_media_px: ((max - min) * ctx.dpr) / Math.abs(y_max - y_min),
      min,
      max,
    };
  }

  /**
   * Marker text, placed with the engine's own marker-label math (frame/axis.rs
   * `append_marker_labels`) and painted by the host through the same overlay pass the engine
   * labels use. Geometry comes from the renderer's anchors (bitmap px); the font-dependent
   * offsets use this frame's exact ratio and layout font size from the info object.
   */
  text_views(info: primitive_text_context): primitive_text_view[] {
    if (this.text_anchors.length === 0 || this.spacing_media <= 0) return [];
    const envelope = marker_envelope_size(this.spacing_media);
    const margin = marker_margin(this.spacing_media);
    const out: primitive_text_view[] = [];
    for (const anchor of this.text_anchors) {
      let offset_media: number;
      if (anchor.position === "above") {
        offset_media = -(envelope + margin + info.font_size * 0.6);
      } else if (anchor.position === "below") {
        offset_media = envelope + margin * 2 + info.font_size * 0.6;
      } else {
        offset_media = envelope / 2 + margin + info.font_size * 0.6;
      }
      const y = anchor.y_anchor + offset_media * info.vpr;
      // The engine's visibility gate (media px): y inside the pane, x inside `[0, pane_w]`.
      if (y < info.pane_top || y > info.pane_top + info.pane_height) continue;
      if (anchor.x < 0 || anchor.x > info.pane_width) continue;
      out.push({
        text: anchor.text,
        x: anchor.x,
        y,
        color: anchor.color,
        align: "center",
        baseline: "middle",
      });
    }
    return out;
  }

  /**
   * Autoscale contribution (reference `autoScale`). The engine's marker auto-scale applies pixel
   * internal margins; the platform's `{min, max}` contract can only widen the price range, so
   * the margins (frame/mod.rs `marker_auto_scale_margins`, same math) are converted with the
   * cached slope. `from`/`to` are always cached first — the renderer's visible-range gate
   * reads them even when the contribution is disabled or impossible.
   */
  autoscale_info(from: number, to: number): { min: number; max: number } | null {
    this.visible_from = from;
    this.visible_to = to;
    if (!this.auto_scale || this.normalized.length === 0 || this.slope === null) return null;
    const margin_value = marker_envelope_size(this.spacing_media) * 1.5 + marker_margin(this.spacing_media) * 2;
    const has_above = this.normalized.some((marker) => marker.position === "above");
    const has_below = this.normalized.some((marker) => marker.position === "below");
    const has_in_bar = this.normalized.some((marker) => marker.position === "in_bar");
    const adjusted = Math.ceil(margin_value / 2);
    const above_px = has_above ? margin_value : has_in_bar ? adjusted : 0;
    const below_px = has_below ? margin_value : has_in_bar ? adjusted : 0;
    if (above_px === 0 && below_px === 0) return null;
    return {
      min: this.slope.min - below_px * this.slope.price_per_media_px,
      max: this.slope.max + above_px * this.slope.price_per_media_px,
    };
  }
}

/**
 * Create a series markers plugin on `series` (reference `createSeriesMarkers`). The markers paint
 * through the plugin platform's command-recording path, so they are pixel-identical across
 * the WebGPU and Canvas2D backends — and to the engine's built-in `series.set_markers` output
 * when both sides run with `auto_scale: false` (see the module header for the autoscale
 * divergence).
 *
 * @param series - The series to attach the markers to.
 * @param markers - The markers to display (positions `aboveBar`/`belowBar`/`inBar`, shapes
 *   `circle`/`square`/`arrowUp`/`arrowDown`).
 * @param options - `auto_scale` (default `true`) and `z_order` (default `"normal"`).
 *
 * @example
 * ```js
 * const markers = create_series_markers(series, [
 *   { time: 1556880900, position: "aboveBar", shape: "arrowDown", color: "#f7525f", text: "SELL" },
 * ]);
 * markers.set_markers([]); // remove all markers
 * markers.detach();
 * ```
 */
export function create_series_markers(
  series: series_api,
  markers?: readonly series_marker[],
  options?: series_markers_options,
): series_markers_handle {
  const primitive = new series_markers_primitive(
    series,
    options?.auto_scale ?? true,
    options?.z_order ?? "normal",
  );
  const handle = series.attach_primitive(primitive);
  // the reference's wrapper sets the initial markers after attach, so the update repaints through the
  // injected `request_update`.
  if (markers !== undefined) {
    primitive.set_markers(markers);
  }
  return {
    set_markers: (next: readonly series_marker[]) => primitive.set_markers(next),
    markers: () => primitive.markers(),
    detach: () => handle.detach(),
  };
}

// ---------------------------------------------------------------------------------------------
// Text watermark (reference plugins/text-watermark)
// ---------------------------------------------------------------------------------------------

/** One text line of a {@link text_watermark_options} watermark (reference `TextWatermarkLineOptions`). */
export interface text_watermark_line_options {
  /** Text of the line (word wrapping is not supported). */
  text: string;
  /** Line color (any CSS color; alpha honored). Default `'rgba(0, 0, 0, 0.5)'`. */
  color?: string;
  /** Font size in CSS px. Default `48`. */
  fontSize?: number;
  /** Font family. Default the reference's font stack. */
  fontFamily?: string;
  /** Font style prefix (e.g. `"bold"`, `"italic"`). Default `''`. */
  fontStyle?: string;
  /** Line height in CSS px. Default `1.2 * fontSize`. */
  lineHeight?: number;
}

/** Options for {@link create_text_watermark} (reference `TextWatermarkOptions`). */
export interface text_watermark_options {
  /** Horizontal alignment inside the pane. Default `'center'`. */
  horzAlign?: "left" | "center" | "right";
  /** Vertical alignment inside the pane. Default `'center'`. */
  vertAlign?: "top" | "center" | "bottom";
  /** The lines to display; each item is a new line. Default `[]`. */
  lines?: text_watermark_line_options[];
}

// reference `defaultFontFamily` (helpers/make-font.ts).
const watermark_default_font_family =
  "-apple-system, BlinkMacSystemFont, 'Trebuchet MS', Roboto, Ubuntu, sans-serif";

/** reference `makeFont`: the CSS font shorthand for a line (zoomed sizes included). */
function make_font(size: number, family: string, style: string): string {
  return style === "" ? `${size}px ${family}` : `${style} ${size}px ${family}`;
}

// Shared text measurement for the zoom-to-fit computation (reference measures with its render
// context; an offscreen context with the same font string measures identically). Created
// lazily so importing the package stays side-effect-free outside the browser.
let watermark_measure_ctx: CanvasRenderingContext2D | null = null;
function measure_watermark_text(font: string, text: string): number {
  if (typeof document === "undefined") return 0;
  if (watermark_measure_ctx === null) {
    watermark_measure_ctx = document.createElement("canvas").getContext("2d");
  }
  if (watermark_measure_ctx === null) return 0;
  watermark_measure_ctx.font = font;
  return watermark_measure_ctx.measureText(text).width;
}

interface watermark_line {
  text: string;
  color: string;
  font_size: number;
  font_family: string;
  font_style: string;
  line_height: number;
}

/**
 * Create a text watermark on `pane` (reference `createTextWatermark`). Painted through the plugin
 * platform's overlay-text hook — the same slot the engine's chart-level `watermark` option
 * paints in (below the axis chrome, above the pane, identical on both backends) — with the reference's
 * line stacking, zoom-to-fit, and alignment math (plugins/text-watermark/pane-renderer.ts).
 *
 * @example
 * ```js
 * const watermark = create_text_watermark(chart.panes()[0], {
 *   horzAlign: "center",
 *   vertAlign: "center",
 *   lines: [{ text: "NUCLEUS", color: "rgba(41, 98, 255, 0.2)", fontSize: 64, fontStyle: "bold" }],
 * });
 * watermark.detach();
 * ```
 */
export function create_text_watermark(pane: pane_api, options?: text_watermark_options): pane_primitive_handle {
  const horz_align = options?.horzAlign ?? "center";
  const vert_align = options?.vertAlign ?? "center";
  const lines: watermark_line[] = (options?.lines ?? []).map((line) => {
    const font_size = line.fontSize ?? 48;
    return {
      text: line.text,
      color: line.color ?? "rgba(0, 0, 0, 0.5)",
      font_size,
      font_family: line.fontFamily ?? watermark_default_font_family,
      font_style: line.fontStyle ?? "",
      line_height: line.lineHeight ?? font_size * 1.2,
    };
  });
  const primitive: pane_primitive = {
    text_views(info: primitive_text_context): primitive_text_view[] {
      const media_width = info.pane_width / info.hpr;
      const media_height = info.pane_height / info.vpr;
      // Per-line zoom-to-fit and the stacked text height (the reference's renderer, first pass).
      const laid: { line: watermark_line; font: string; zoom: number }[] = [];
      let text_height = 0;
      for (const line of lines) {
        if (line.text.length === 0) continue;
        const font = make_font(line.font_size, line.font_family, line.font_style);
        const width = measure_watermark_text(font, line.text);
        const zoom = width > 0 && width > media_width ? media_width / width : 1;
        laid.push({ line, font, zoom });
        text_height += line.line_height * zoom;
      }
      let offset = 0;
      if (vert_align === "center") {
        offset = Math.max((media_height - text_height) / 2, 0);
      } else if (vert_align === "bottom") {
        offset = Math.max(media_height - text_height, 0);
      }
      const out: primitive_text_view[] = [];
      for (const { line, font, zoom } of laid) {
        let x: number;
        let align: "left" | "center" | "right";
        if (horz_align === "left") {
          x = line.line_height / 2;
          align = "left";
        } else if (horz_align === "right") {
          x = media_width - 1 - line.line_height / 2;
          align = "right";
        } else {
          x = media_width / 2;
          align = "center";
        }
        out.push({
          text: line.text,
          // Descriptors take bitmap px (the draw context's space); the host paints media.
          x: info.pane_left + x * info.hpr,
          y: info.pane_top + offset * info.vpr,
          color: line.color,
          // reference scales the context by `zoom`; scaling the font size is the same layout.
          font: zoom === 1 ? font : make_font(line.font_size * zoom, line.font_family, line.font_style),
          align,
          baseline: "top",
        });
        offset += line.line_height * zoom;
      }
      return out;
    },
  };
  return pane.attach_primitive(primitive);
}
