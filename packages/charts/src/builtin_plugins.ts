/**
 * Independently implemented built-in plugins informed by established public chart-plugin APIs.
 *
 * - {@link create_series_markers} provides familiar series-marker behavior:
 *   it delegates directly to the engine's canonical marker state and frame builder.
 * - {@link create_text_watermark} provides familiar text-watermark behavior:
 *   state, zoom-to-fit layout, and text primitives are produced by the shared Rust engine.
 */

import { attach_native_text_watermark } from "./impl.js";
import type { pane_api, series_api, series_marker } from "./types.js";

// ---------------------------------------------------------------------------------------------
// Series markers (reference plugins/series-markers)
// ---------------------------------------------------------------------------------------------

/** Options for {@link create_series_markers} (reference `SeriesMarkersOptions`). */
export interface series_markers_options {
  /**
   * Expand the owning series' price scale so marker shapes stay visible (reference `autoScale`,
   * default `true`).
   */
  auto_scale?: boolean;
  /** Official marker layer. Default `"normal"`. */
  z_order?: "normal" | "aboveSeries" | "top";
}

/** Handle returned by {@link create_series_markers} (reference `ISeriesMarkersPluginApi`). */
export interface series_markers_handle {
  /** Replace the markers (pass `[]` to remove them all) and repaint. */
  set_markers(markers: readonly series_marker[]): void;
  /** The current markers array. */
  markers(): readonly series_marker[];
  /** Merge marker-plugin options and repaint. */
  apply_options(options: Partial<series_markers_options>): void;
  /** Detach the plugin from the series and repaint. */
  detach(): void;
}

/**
 * Create a series markers plugin on `series` (reference `createSeriesMarkers`). The markers paint
 * through the engine's canonical marker path, shared by every backend.
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
  let current = [...(markers ?? [])];
  let current_options: Required<series_markers_options> = {
    auto_scale: options?.auto_scale ?? true,
    z_order: options?.z_order ?? "normal",
  };
  series.set_markers(current, current_options);
  return {
    set_markers(next) {
      current = [...next];
      series.set_markers(current, current_options);
    },
    markers: () => current,
    apply_options(patch) {
      current_options = { ...current_options, ...patch };
      series.set_markers(current, current_options);
    },
    detach() {
      current = [];
      series.set_markers([], current_options);
    },
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
  /** Display the watermark. Default `true`. */
  visible?: boolean;
  /** Horizontal alignment inside the pane. Default `'center'`. */
  horzAlign?: "left" | "center" | "right";
  /** Vertical alignment inside the pane. Default `'center'`. */
  vertAlign?: "top" | "center" | "bottom";
  /** The lines to display; each item is a new line. Default `[]`. */
  lines?: text_watermark_line_options[];
}

export interface text_watermark_api {
  apply_options(options: Partial<text_watermark_options>): void;
  detach(): void;
}

// reference `defaultFontFamily` (helpers/make-font.ts).
const watermark_default_font_family =
  "-apple-system, BlinkMacSystemFont, 'Trebuchet MS', Roboto, Ubuntu, sans-serif";

interface normalized_text_watermark_line {
  text: string;
  color: string;
  font_size: number;
  font_family: string;
  font_weight: number;
  italic: boolean;
  line_height: number;
}

function normalize_text_watermark(options: text_watermark_options): {
  visible: boolean;
  horizontal_align: "left" | "center" | "right";
  vertical_align: "top" | "center" | "bottom";
  lines: normalized_text_watermark_line[];
} {
  return {
    visible: options.visible ?? true,
    horizontal_align: options.horzAlign ?? "center",
    vertical_align: options.vertAlign ?? "center",
    lines: (options.lines ?? []).map((line) => {
      const font_size = line.fontSize ?? 48;
      const style = (line.fontStyle ?? "").toLowerCase().split(/\\s+/).filter(Boolean);
      const numeric_weight = style.find((token) => /^[1-9]00$/.test(token));
      return {
        text: line.text,
        color: line.color ?? "rgba(0, 0, 0, 0.5)",
        font_size,
        font_family: line.fontFamily ?? watermark_default_font_family,
        font_weight: numeric_weight === undefined ? (style.includes("bold") ? 700 : 400) : Number(numeric_weight),
        italic: style.includes("italic") || style.includes("oblique"),
        line_height: line.lineHeight ?? font_size * 1.2,
      };
    }),
  };
}

/** Official multi-line text watermark backed by Rust-owned state and frame geometry. */
export function create_text_watermark(
  pane: pane_api,
  options: text_watermark_options = {},
): text_watermark_api {
  let current: text_watermark_options = { ...options, lines: [...(options.lines ?? [])] };
  const handle = attach_native_text_watermark(pane, JSON.stringify(normalize_text_watermark(current)));
  return {
    apply_options(patch) {
      const next: text_watermark_options = {
        ...current,
        ...patch,
        lines: patch.lines === undefined ? current.lines : [...patch.lines],
      };
      if (!handle.set_options_json(JSON.stringify(normalize_text_watermark(next)))) {
        throw new Error("Nucleus rejected text-watermark options");
      }
      current = next;
    },
    detach: handle.detach,
  };
}
