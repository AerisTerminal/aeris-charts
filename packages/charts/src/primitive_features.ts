/** First-class primitive and chart helpers built on Nucleus's existing extension boundaries. */

import { chart_impl, time_to_utc_seconds } from "./impl.js";
import {
  attach_native_bands_indicator,
  attach_native_anchored_text,
  attach_native_crosshair_highlight,
  attach_native_delta_tooltip,
  attach_native_expiring_price_alerts,
  attach_native_image_watermark,
  attach_native_overlay_price_scale,
  attach_native_partial_price_line,
  attach_native_session_highlighting,
  attach_native_tooltip,
  attach_native_trend_line,
  attach_native_user_price_alerts,
  attach_native_vertical_line,
  attach_native_user_price_lines_button,
  attach_native_volume_profile,
} from "./impl.js";
import type {
  chart_api,
  drawing_api,
  drawing_kind,
  drawing_options,
  drawing_point,
  mouse_event_params,
  price_line_api,
  price_line_options,
  price_scale_api,
  series_api,
  time,
} from "./types.js";

export interface detachable_feature {
  detach(): void;
}

export interface anchored_text_options {
  vert_align?: "top" | "middle" | "bottom";
  /** Official plugin spelling; `vert_align` remains the Nucleus alias. */
  vertAlign?: "top" | "middle" | "bottom";
  horz_align?: "left" | "middle" | "right";
  /** Official plugin spelling; `horz_align` remains the Nucleus alias. */
  horzAlign?: "left" | "middle" | "right";
  text: string;
  line_height?: number;
  /** Official plugin spelling; `line_height` remains the Nucleus alias. */
  lineHeight?: number;
  /** CSS font shorthand containing a pixel size, for example `italic bold 54px Arial`. */
  font?: string;
  color?: string;
}

export interface anchored_text_api extends detachable_feature {
  apply_options(options: Partial<anchored_text_options>): void;
}

interface parsed_anchored_text_options {
  vertical_align: "top" | "middle" | "bottom";
  horizontal_align: "left" | "middle" | "right";
  text: string;
  line_height: number;
  font_size: number;
  font_family: string;
  font_weight: number;
  italic: boolean;
  color: string;
}

function parse_anchored_font(font: string): Pick<parsed_anchored_text_options, "font_size" | "font_family" | "font_weight" | "italic"> {
  const match = /(?:^|\s)(\d+(?:\.\d+)?)px\s+(.+)$/i.exec(font.trim());
  if (match === null || match.index === undefined) {
    throw new Error("anchored text font must contain a pixel size and family, for example '20px Arial'");
  }
  const size = Number(match[1]);
  const family = match[2]?.trim() ?? "";
  const prefix = font.trim().slice(0, match.index).trim().toLowerCase().split(/\s+/).filter(Boolean);
  const numeric_weight = prefix.find((token) => /^[1-9]00$/.test(token));
  const weight = numeric_weight === undefined ? (prefix.includes("bold") ? 700 : 400) : Number(numeric_weight);
  if (!Number.isFinite(size) || size <= 0 || family.length === 0) {
    throw new Error("anchored text font has an invalid size or family");
  }
  return { font_size: size, font_family: family, font_weight: weight, italic: prefix.includes("italic") };
}

function normalize_anchored_text(options: anchored_text_options): parsed_anchored_text_options {
  const font = parse_anchored_font(options.font ?? "20px Arial");
  return {
    vertical_align: options.vert_align ?? options.vertAlign ?? "middle",
    horizontal_align: options.horz_align ?? options.horzAlign ?? "middle",
    text: options.text,
    line_height: options.line_height ?? options.lineHeight ?? font.font_size,
    ...font,
    color: options.color ?? "#000000",
  };
}

/** Official viewport-anchored text primitive, rendered by the shared Rust frame. */
export function create_anchored_text(
  series: series_api,
  options: anchored_text_options,
): anchored_text_api {
  let current = { ...options };
  const handle = attach_native_anchored_text(series, JSON.stringify(normalize_anchored_text(current)));
  return {
    apply_options(patch) {
      const next = { ...current, ...patch };
      if (!handle.set_options_json(JSON.stringify(normalize_anchored_text(next)))) {
        throw new Error("Nucleus rejected anchored-text options");
      }
      current = next;
    },
    detach: handle.detach,
  };
}

export interface bands_indicator_options {
  line_color?: string;
  fill_color?: string;
  line_width?: number;
}

export interface bands_indicator_api extends detachable_feature {
  apply_options(options: Partial<bands_indicator_options>): void;
}

/** Official ±10% source-price bands, drawn behind the source by the shared Rust frame. */
export function create_bands_indicator(
  source: series_api,
  options: bands_indicator_options = {},
): bands_indicator_api {
  let current = { ...options };
  const native = attach_native_bands_indicator(source, JSON.stringify(current));
  return {
    apply_options(patch) {
      const next = { ...current, ...patch };
      if (!native.set_options_json(JSON.stringify(next))) {
        throw new Error("Nucleus rejected bands-indicator options");
      }
      current = next;
    },
    detach: native.detach,
  };
}

export interface rectangle_drawing_tool_options extends Partial<drawing_options> {
  fill_color?: string;
  preview_fill_color?: string;
  label_color?: string;
  label_text_color?: string;
  show_labels?: boolean;
}

export interface rectangle_drawing_tool_api {
  start_drawing(): void;
  stop_drawing(): void;
  is_drawing(): boolean;
  apply_options(options: rectangle_drawing_tool_options): void;
  remove(): void;
}

const RECTANGLE_DEFAULTS = {
  fill_color: "rgba(200, 50, 100, 0.75)",
  preview_fill_color: "rgba(200, 50, 100, 0.25)",
  label_color: "rgba(200, 50, 100, 1)",
  label_text_color: "#ffffff",
  show_labels: true,
} as const;

function normalize_rectangle_options(
  options: rectangle_drawing_tool_options,
): Partial<drawing_options> {
  return {
    ...options,
    color: options.color ?? options.label_color ?? RECTANGLE_DEFAULTS.label_color,
    fill_color: options.fill_color ?? options.color ?? RECTANGLE_DEFAULTS.fill_color,
    preview_fill_color: options.preview_fill_color ?? RECTANGLE_DEFAULTS.preview_fill_color,
    border_visible: options.border_visible ?? false,
    show_labels: options.show_labels ?? RECTANGLE_DEFAULTS.show_labels,
    axis_bands_visible: options.axis_bands_visible ?? true,
    label_color: options.label_color ?? options.color ?? RECTANGLE_DEFAULTS.label_color,
    label_text_color: options.label_text_color ?? RECTANGLE_DEFAULTS.label_text_color,
    snap_time_to_data: options.snap_time_to_data ?? true,
  };
}

/** A committed official rectangle, with shared-engine pane and axis geometry. */
export function create_rectangle_drawing(
  chart: chart_api,
  points: [drawing_point, drawing_point],
  options: rectangle_drawing_tool_options = {},
  pane_index = 0,
): drawing_api {
  return chart.add_drawing("rectangle", points, normalize_rectangle_options(options), pane_index);
}

/**
 * Official two-click rectangle tool. Pointer placement, preview, commit, snapping, selection,
 * dragging, axis geometry, and persistence stay in Rust; this controller owns only its optional
 * DOM toolbar and the lifecycle of rectangles it created.
 */
export function create_rectangle_drawing_tool(
  chart: chart_api,
  series: series_api,
  toolbar_container?: HTMLElement,
  options: rectangle_drawing_tool_options = {},
): rectangle_drawing_tool_api {
  let current = { ...options };
  let active = false;
  let removed = false;
  const drawings: drawing_api[] = [];
  let button: HTMLButtonElement | undefined;
  let color_picker: HTMLInputElement | undefined;

  const bound_options = (): Partial<drawing_options> => {
    const series_scale = series.options().price_scale_id;
    const price_scale_id = series_scale === "left"
      ? "left"
      : series_scale === "right"
        ? "right"
        : "overlay";
    return normalize_rectangle_options({ ...current, price_scale_id });
  };
  const arm_engine_tool = () => {
    chart.set_drawing_tool("rectangle", bound_options(), series.pane_index());
  };

  const sync_button = () => {
    if (button === undefined) return;
    const drawing = active && chart.active_drawing_tool() === "rectangle";
    button.dataset.active = String(drawing);
    button.setAttribute("aria-pressed", String(drawing));
    button.style.color = drawing ? "rgb(100, 150, 250)" : "currentColor";
  };
  const tool_change = (tool: drawing_kind | null) => {
    if (tool !== "rectangle") active = false;
    sync_button();
  };
  const drawing_created = (drawing: drawing_api) => {
    if (!active || drawing.kind() !== "rectangle" || drawing.pane_index() !== series.pane_index()) return;
    drawings.push(drawing);
    active = false;
    sync_button();
  };
  chart.subscribe_drawing_tool_change(tool_change);
  chart.subscribe_drawing_created(drawing_created);

  const start_drawing = () => {
    if (removed) return;
    active = true;
    arm_engine_tool();
    sync_button();
  };
  const stop_drawing = () => {
    if (removed) return;
    active = false;
    if (chart.active_drawing_tool() === "rectangle") chart.set_drawing_tool(null);
    sync_button();
  };

  if (toolbar_container !== undefined) {
    button = document.createElement("button");
    button.type = "button";
    button.className = "nucleus-rectangle-tool-button";
    button.setAttribute("aria-label", "Draw rectangle");
    button.setAttribute("aria-pressed", "false");
    button.style.width = "20px";
    button.style.height = "20px";
    button.style.padding = "0";
    button.style.border = "0";
    button.style.background = "transparent";
    button.style.cursor = "pointer";
    button.innerHTML = '<svg aria-hidden="true" viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.8"><rect x="4" y="4" width="16" height="16" rx="1"/><path d="M4 9h16M9 4v16" opacity=".5"/></svg>';
    button.addEventListener("click", () => {
      if (active && chart.active_drawing_tool() === "rectangle") stop_drawing();
      else start_drawing();
    });
    toolbar_container.appendChild(button);

    color_picker = document.createElement("input");
    color_picker.type = "color";
    color_picker.value = "#c83264";
    color_picker.setAttribute("aria-label", "Rectangle color");
    color_picker.style.width = "24px";
    color_picker.style.height = "20px";
    color_picker.style.border = "0";
    color_picker.style.padding = "0";
    color_picker.style.background = "transparent";
    color_picker.addEventListener("change", () => {
      const color = color_picker?.value ?? "#c83264";
      current = {
        ...current,
        fill_color: `${color}bf`,
        preview_fill_color: `${color}40`,
        label_color: color,
      };
      if (active) arm_engine_tool();
    });
    toolbar_container.appendChild(color_picker);
  }

  return {
    start_drawing,
    stop_drawing,
    is_drawing: () => active && chart.active_drawing_tool() === "rectangle",
    apply_options(patch) {
      current = { ...current, ...patch };
      if (active) arm_engine_tool();
    },
    remove() {
      if (removed) return;
      stop_drawing();
      removed = true;
      chart.unsubscribe_drawing_tool_change(tool_change);
      chart.unsubscribe_drawing_created(drawing_created);
      const live = new Set(chart.drawings().map((drawing) => drawing.id));
      for (const drawing of drawings) {
        if (live.has(drawing.id)) drawing.remove();
      }
      button?.remove();
      color_picker?.remove();
    },
  };
}

export interface trend_line_point {
  time: time;
  price: number;
}

export interface trend_line_options {
  line_color?: string;
  width?: number;
  show_labels?: boolean;
  label_background_color?: string;
  label_text_color?: string;
}

/** Official engine-owned trend line with endpoint labels and autoscale participation. */
export function create_trend_line(
  series: series_api,
  points: [trend_line_point, trend_line_point],
  options: trend_line_options = {},
): detachable_feature {
  const [first, second] = points;
  return attach_native_trend_line(
    series,
    time_to_utc_seconds(first.time),
    first.price,
    time_to_utc_seconds(second.time),
    second.price,
    JSON.stringify(options),
  );
}

export interface vertical_line_options {
  color?: string;
  label_text?: string;
  width?: number;
  label_background_color?: string;
  label_text_color?: string;
  show_label?: boolean;
}

/** Official engine-owned full-pane vertical line with optional time-axis label. */
export function create_vertical_line(
  series: series_api,
  line_time: time,
  options: vertical_line_options = {},
): detachable_feature {
  return attach_native_vertical_line(
    series,
    time_to_utc_seconds(line_time),
    JSON.stringify(options),
  );
}

/** Engine-owned user price line. */
export function create_user_price_line(series: series_api, options: price_line_options): price_line_api {
  return series.create_price_line(options);
}

export interface user_price_lines_options {
  color?: string;
  hover_color?: string;
  limit_to_one?: boolean;
}

/** Official crosshair-following add-price-line button at the pane's right edge. */
export function create_user_price_lines(
  chart: chart_api,
  series: series_api,
  options: user_price_lines_options = {},
): detachable_feature {
  chart.apply_options({ crosshair: { mode: 0 } });
  return attach_native_user_price_lines_button(series, JSON.stringify(options));
}

/** Move a series to the pane's independent overlay scale and return that scale. */
export function use_overlay_price_scale(series: series_api): price_scale_api {
  series.apply_options({ price_scale_id: "" });
  return series.price_scale();
}

export interface overlay_price_scale_options {
  text_color?: string;
  background_color?: string;
  side?: "left" | "right";
}

export interface overlay_price_scale_api extends detachable_feature {
  apply_options(options: Partial<overlay_price_scale_options>): void;
  price_scale(): price_scale_api;
}

/** Official in-pane overlay price-scale labels, backed by the attached series' engine scale. */
export function create_overlay_price_scale(
  series: series_api,
  options: overlay_price_scale_options = {},
): overlay_price_scale_api {
  const scale = use_overlay_price_scale(series);
  let current = { ...options };
  const native = attach_native_overlay_price_scale(series, JSON.stringify(current));
  return {
    apply_options(patch) {
      const next = { ...current, ...patch };
      if (!native.set_options_json(JSON.stringify(next))) {
        throw new Error("Nucleus rejected overlay-price-scale options");
      }
      current = next;
    },
    price_scale: () => scale,
    detach: native.detach,
  };
}

/** Official partial-last-price line: the final value's x-coordinate to the pane's right edge. */
export function create_partial_price_line(
  series: series_api,
): detachable_feature {
  series.apply_options({ price_line_visible: false });
  return attach_native_partial_price_line(series);
}

export interface session_highlighting_options {
  start_hour_utc?: number;
  end_hour_utc?: number;
  weekday_color?: string;
  weekend_color?: string;
}

/** Official per-source-bar highlighter callback. Returned colors become engine-owned records. */
export type session_highlighter = (date: time) => string;

export function create_session_highlighting(
  series: series_api,
  highlighter: session_highlighter,
  options?: session_highlighting_options,
): detachable_feature;
export function create_session_highlighting(
  series: series_api,
  options?: session_highlighting_options,
): detachable_feature;
/**
 * Engine-owned source-bar shading. The callback overload matches TradingView's official plugin:
 * the host evaluates the user callback when source data changes, then Rust retains the aligned
 * `{time,color}` records and owns coordinate conversion, clipping, merging, and rendering.
 */
export function create_session_highlighting(
  series: series_api,
  highlighter_or_options: session_highlighter | session_highlighting_options = {},
  options: session_highlighting_options = {},
): detachable_feature {
  if (typeof highlighter_or_options !== "function") {
    return attach_native_session_highlighting(series, JSON.stringify(highlighter_or_options));
  }
  const highlighter = highlighter_or_options;
  const native = attach_native_session_highlighting(series, JSON.stringify(options));
  let attached = true;
  const sync = (): void => {
    const highlights = series.data().map((point) => ({
      time: time_to_utc_seconds(point.time),
      color: highlighter(point.time) || "rgba(0, 0, 0, 0)",
    }));
    if (!native.set_data_json(JSON.stringify(highlights))) {
      throw new Error("Nucleus rejected session-highlighting data that was not aligned to its source series");
    }
  };
  series.subscribe_data_changed(sync);
  try {
    sync();
  } catch (error) {
    series.unsubscribe_data_changed(sync);
    native.detach();
    throw error;
  }
  return {
    detach() {
      if (!attached) return;
      attached = false;
      series.unsubscribe_data_changed(sync);
      native.detach();
    },
  };
}

export interface highlight_bar_crosshair_options {
  color?: string;
}

/** Highlight the complete bar slot under the chart crosshair. */
export function create_highlight_bar_crosshair(
  chart: chart_api,
  series: series_api,
  options: highlight_bar_crosshair_options = {},
): detachable_feature {
  chart.apply_options({
    crosshair: { mode: 0, vertLine: { visible: false } },
  });
  return attach_native_crosshair_highlight(series, options.color ?? "rgba(0, 0, 0, 0.2)");
}

export interface image_watermark_options {
  max_width?: number;
  /** Official plugin spelling; `max_width` remains the Nucleus alias. */
  maxWidth?: number;
  max_height?: number;
  /** Official plugin spelling; `max_height` remains the Nucleus alias. */
  maxHeight?: number;
  padding?: number;
  alpha?: number;
}

function normalize_image_watermark_options(options: image_watermark_options): image_watermark_options {
  return {
    max_width: options.max_width ?? options.maxWidth,
    max_height: options.max_height ?? options.maxHeight,
    padding: options.padding,
    alpha: options.alpha,
  };
}

function numeric_image_dimension(image: CanvasImageSource, keys: readonly string[]): number {
  const source = image as unknown as Record<string, unknown>;
  for (const key of keys) {
    const value = source[key];
    if (typeof value === "number" && Number.isFinite(value) && value > 0) return value;
  }
  return 0;
}

/** Decode a browser image source once; financial geometry and rendering stay Rust-owned. */
function image_rgba(image: CanvasImageSource): { width: number; height: number; pixels: Uint8Array } {
  const source_width = numeric_image_dimension(image, ["naturalWidth", "videoWidth", "displayWidth", "width"]);
  const source_height = numeric_image_dimension(image, ["naturalHeight", "videoHeight", "displayHeight", "height"]);
  if (source_width === 0 || source_height === 0) {
    throw new Error("image watermark source is not loaded or has no intrinsic dimensions");
  }
  const scale = Math.min(1, 1024 / source_width, 1024 / source_height);
  const width = Math.max(1, Math.round(source_width * scale));
  const height = Math.max(1, Math.round(source_height * scale));
  const canvas = typeof OffscreenCanvas === "function"
    ? new OffscreenCanvas(width, height)
    : document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext("2d", { willReadFrequently: true });
  if (context === null) throw new Error("image watermark raster canvas is unavailable");
  try {
    context.drawImage(image, 0, 0, width, height);
    const data = context.getImageData(0, 0, width, height).data;
    return { width, height, pixels: new Uint8Array(data.buffer, data.byteOffset, data.byteLength) };
  } catch (error) {
    throw new Error(`image watermark source could not be decoded (cross-origin images require CORS): ${String(error)}`);
  }
}

/** Engine-owned official centered, aspect-preserving image watermark. */
export function create_image_watermark(
  series: series_api,
  image: CanvasImageSource | string,
  options: image_watermark_options = {},
): detachable_feature {
  const attach = (source: CanvasImageSource): detachable_feature => {
    const raster = image_rgba(source);
    return attach_native_image_watermark(
      series,
      raster.width,
      raster.height,
      raster.pixels,
      JSON.stringify(normalize_image_watermark_options(options)),
    );
  };
  if (typeof image !== "string") return attach(image);

  // Official lifecycle: URL loading begins on attachment and requests a repaint when complete.
  // The browser remains responsible only for decoding; RGBA storage, placement, and rendering
  // move into Rust once loaded so native and browser backends execute the same primitive.
  const element = new Image();
  element.crossOrigin = "anonymous";
  let native: detachable_feature | null = null;
  let detached = false;
  element.onload = () => {
    if (detached) return;
    native = attach(element);
  };
  element.src = image;
  return {
    detach() {
      if (detached) return;
      detached = true;
      element.onload = null;
      native?.detach();
      native = null;
    },
  };
}

export interface tooltip_options {
  series?: series_api;
  class_name?: string;
  title?: string;
  line_color?: string;
  follow_mode?: "top" | "tracking";
  horizontal_deadzone_width?: number;
  vertical_deadzone_height?: number;
  vertical_spacing?: number;
  top_offset?: number;
  format?: (event: mouse_event_params, value: number | null) => string;
}

export interface tooltip_handle extends detachable_feature {
  apply_options(options: Partial<tooltip_options>): void;
}

interface native_tooltip_snapshot {
  x: number;
  index: number;
  price: number;
  time: number;
}

function tooltip_text(element: HTMLDivElement, value: string): void {
  if (element.innerText !== value) element.innerText = value;
  element.style.display = value.length > 0 ? "block" : "none";
}

function tooltip_date_time(timestamp: number): [string, string] {
  if (timestamp === 0) return ["", ""];
  const date = new Date(timestamp * 1_000);
  const day = String(date.getDate()).padStart(2, "0");
  const month = date.toLocaleString(undefined, { month: "short" });
  const year = date.getFullYear();
  const hours = String(date.getHours()).padStart(2, "0");
  const minutes = String(date.getMinutes()).padStart(2, "0");
  return [`${day} ${month} ${year}`, `${hours}:${minutes}`];
}

/** Official structured DOM tooltip; source lookup and its vertical guide are engine-owned. */
export function create_tooltip(chart: chart_api, options: tooltip_options = {}): tooltip_handle {
  let current: Required<Omit<tooltip_options, "series" | "format">> & Pick<tooltip_options, "series" | "format"> = {
    series: options.series,
    class_name: options.class_name ?? "nucleuscharts-tooltip",
    title: options.title ?? "",
    line_color: options.line_color ?? "rgba(0, 0, 0, 0.2)",
    follow_mode: options.follow_mode ?? "tracking",
    horizontal_deadzone_width: options.horizontal_deadzone_width ?? 45,
    vertical_deadzone_height: options.vertical_deadzone_height ?? 100,
    vertical_spacing: options.vertical_spacing ?? 20,
    top_offset: options.top_offset ?? 20,
    format: options.format,
  };
  const series = current.series ?? chart.panes().flatMap((pane) => pane.get_series())[0];
  if (series === undefined) throw new Error("tooltip requires a chart series");
  const native_options = (): string => JSON.stringify({
    line_color: current.line_color,
    top_margin: current.follow_mode === "top" ? current.top_offset + 10 : 0,
  });
  const native = attach_native_tooltip(series, native_options());
  const host = chart.chart_element();
  const element = document.createElement("div");
  element.className = current.class_name;
  element.style.cssText = "display:flex;flex-direction:column;align-items:center;position:absolute;transform:translate(calc(0px - 50%),0);opacity:0;left:0;top:0;z-index:100;background-color:white;border-radius:4px;padding:5px 10px;font-family:-apple-system,BlinkMacSystemFont,'Trebuchet MS',Roboto,Ubuntu,sans-serif;font-size:12px;font-weight:400;box-shadow:0 2px 4px rgba(0,0,0,.2);line-height:16px;pointer-events:none;color:#131722";
  const title = document.createElement("div");
  title.style.cssText = "font-size:16px;line-height:24px;font-weight:590";
  const price = document.createElement("div");
  price.style.cssText = "font-size:14px;line-height:18px;font-weight:590";
  const date = document.createElement("div");
  date.style.color = "#787B86";
  const time_element = document.createElement("div");
  time_element.style.color = "#787B86";
  element.append(title, price, date, time_element);
  host.appendChild(element);
  tooltip_text(title, current.title);
  tooltip_text(price, "");
  tooltip_text(date, "");
  tooltip_text(time_element, "");

  const hide = (): void => {
    element.style.opacity = "0";
  };
  const on_move = (event: mouse_event_params): void => {
    if (event.point === null || event.pane_index === null) {
      hide();
      return;
    }
    const snapshot = JSON.parse(native.snapshot_json()) as native_tooltip_snapshot | null;
    if (snapshot === null) {
      hide();
      return;
    }
    const [date_text, time_text] = tooltip_date_time(snapshot.time);
    tooltip_text(title, current.title);
    tooltip_text(price, current.format?.(event, snapshot.price) ?? snapshot.price.toFixed(2));
    tooltip_text(date, date_text);
    tooltip_text(time_element, time_text);
    const geometry = chart.panes()[event.pane_index]?.get_geometry();
    if (geometry === undefined) {
      hide();
      return;
    }
    const deadzone = element.getBoundingClientRect().width > 0
      ? Math.ceil(element.getBoundingClientRect().width / 2)
      : current.horizontal_deadzone_width;
    const raw_x = geometry.left + event.point.x;
    const x = Math.min(Math.max(geometry.left + deadzone, raw_x), geometry.left + geometry.width - deadzone);
    const pane_y = event.point.y - geometry.top;
    const y = current.follow_mode === "top"
      ? geometry.top + current.top_offset
      : event.point.y + (pane_y <= current.vertical_spacing + current.vertical_deadzone_height
        ? current.vertical_spacing
        : -current.vertical_spacing);
    const y_percent = current.follow_mode === "tracking"
      && pane_y > current.vertical_spacing + current.vertical_deadzone_height
      ? " - 100%"
      : "";
    element.style.transform = `translate(calc(${x}px - 50%), calc(${y}px${y_percent}))`;
    element.style.opacity = "1";
  };
  chart.apply_options({
    crosshair: {
      mode: 1,
      vertLine: { visible: false, labelVisible: false },
      horzLine: { visible: false, labelVisible: false },
    },
  });
  chart.subscribe_crosshair_move(on_move);
  return {
    apply_options(patch) {
      current = { ...current, ...patch };
      if (!native.set_options_json(native_options())) {
        throw new Error("Nucleus rejected tooltip options");
      }
      tooltip_text(title, current.title);
    },
    detach() {
      chart.unsubscribe_crosshair_move(on_move);
      element.remove();
      native.detach();
    },
  };
}

export interface delta_tooltip_active_range {
  from: number;
  to: number;
  positive: boolean;
}

export interface delta_tooltip_options {
  series: series_api;
  line_color?: string;
  show_time?: boolean;
  top_offset?: number;
  on_active_range_change?: (range: delta_tooltip_active_range | null) => void;
}

export interface delta_tooltip_handle extends detachable_feature {
  active_range(): delta_tooltip_active_range | null;
}

/** Official one/two-pointer delta tooltip, with all bar lookup and geometry owned by Rust. */
export function create_delta_tooltip(
  chart: chart_api,
  options: delta_tooltip_options,
): delta_tooltip_handle {
  const { series, on_active_range_change, ...native_options } = options;
  if (!(chart instanceof chart_impl)) {
    throw new Error("engine-owned delta tooltips require a Nucleus chart instance");
  }
  const native = attach_native_delta_tooltip(series, JSON.stringify(native_options));
  let last_range_json = "null";
  const active_range = (): delta_tooltip_active_range | null =>
    JSON.parse(native.active_range_json()) as delta_tooltip_active_range | null;
  const notify_range = (): void => {
    const json = native.active_range_json();
    if (json === last_range_json) return;
    last_range_json = json;
    on_active_range_change?.(JSON.parse(json) as delta_tooltip_active_range | null);
  };
  const remove_range_listener = chart.add_delta_tooltip_range_listener(notify_range);
  chart.apply_options({
    crosshair: {
      mode: 1,
      vertLine: { visible: false, labelVisible: false },
      horzLine: { visible: false, labelVisible: false },
    },
  });
  return {
    active_range,
    detach() {
      remove_range_listener();
      native.detach();
    },
  };
}

export interface volume_profile_point {
  price: number;
  vol: number;
}

export interface volume_profile_data {
  time: time;
  profile: readonly volume_profile_point[];
  /** Profile width in time-scale bar slots. */
  width: number;
}

export interface volume_profile_options {
  background_color?: string;
  color?: string;
}

export interface volume_profile_handle extends detachable_feature {
  set_data(data: volume_profile_data): void;
}

function serialize_volume_profile(data: volume_profile_data): string {
  return JSON.stringify({ ...data, time: time_to_utc_seconds(data.time) });
}

/** Official time-anchored price-by-volume profile, binned upstream and rendered by Rust. */
export function create_volume_profile(
  series: series_api,
  initial_data: volume_profile_data,
  options: volume_profile_options = {},
): volume_profile_handle {
  const handle = attach_native_volume_profile(
    series,
    serialize_volume_profile(initial_data),
    JSON.stringify(options),
  );
  return {
    set_data(next) {
      if (!handle.set_data_json(serialize_volume_profile(next))) {
        throw new Error("Nucleus rejected malformed volume-profile data");
      }
    },
    detach: handle.detach,
  };
}

export interface expiring_price_alert_parameters {
  title: string;
  crossing_direction: "up" | "down";
}

export interface expiring_price_alert {
  id: number;
  price: number;
  /** UTC seconds after conversion at the package boundary. */
  start: number;
  /** UTC seconds after conversion at the package boundary. */
  end: number;
  parameters: expiring_price_alert_parameters;
  crossed: boolean;
  expired: boolean;
}

export interface expiring_price_alerts_options {
  /** Interval between the engine-owned future whitespace points, in seconds. */
  interval?: number;
  /** Delay before a crossed or expired alert is removed, in milliseconds. */
  clear_timeout?: number;
}

export interface expiring_price_alerts_handle extends detachable_feature {
  add(
    price: number,
    start: time,
    end: time,
    parameters: expiring_price_alert_parameters,
  ): expiring_price_alert;
  remove(id: number): void;
  alerts(): readonly expiring_price_alert[];
}

/**
 * Reference-compatible expiring price alerts. Timeline extension, crossing/expiry state,
 * autoscaling, and pane geometry are all owned by the shared Rust engine.
 */
export function create_expiring_price_alerts(
  series: series_api,
  options: expiring_price_alerts_options = {},
): expiring_price_alerts_handle {
  const native = attach_native_expiring_price_alerts(series, JSON.stringify(options));
  const alerts = (): readonly expiring_price_alert[] =>
    JSON.parse(native.alerts_json()) as expiring_price_alert[];
  return {
    add(price, start, end, parameters) {
      const id = native.add(
        price,
        time_to_utc_seconds(start),
        time_to_utc_seconds(end),
        parameters.title,
        parameters.crossing_direction,
      );
      const added = alerts().find((alert) => alert.id === id);
      if (added === undefined) throw new Error("Nucleus failed to retain the expiring price alert");
      return added;
    },
    remove(id) {
      native.remove(id);
    },
    alerts,
    detach: native.detach,
  };
}

export interface user_price_alert {
  id: number;
  price: number;
}

export interface user_price_alerts_options {
  symbol_name?: string;
  color?: string;
  hover_color?: string;
}

export interface user_price_alerts_handle extends detachable_feature {
  add(price: number): user_price_alert;
  remove(id: number): void;
  alerts(): readonly user_price_alert[];
}

/** Official price-scale button, alert lines, hover label, and removal interaction. */
export function create_user_price_alerts(
  chart: chart_api,
  series: series_api,
  options: user_price_alerts_options = {},
): user_price_alerts_handle {
  const native = attach_native_user_price_alerts(series, JSON.stringify(options));
  const alerts = (): readonly user_price_alert[] =>
    JSON.parse(native.alerts_json()) as user_price_alert[];
  return {
    add(price) {
      const id = native.add(price);
      const added = alerts().find((alert) => alert.id === id);
      if (added === undefined) throw new Error("Nucleus failed to retain the user price alert");
      return added;
    },
    remove(id) {
      native.remove(id);
    },
    alerts,
    detach: native.detach,
  };
}
