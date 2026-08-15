/** First-class primitive and chart helpers built on Nucleus's existing extension boundaries. */

import { time_to_utc_seconds } from "./impl.js";
import type { canvas_primitive_handle } from "./canvas_plugins.js";
import type { series_primitive, series_primitive_draw_context, series_primitive_handle } from "./primitives.js";
import type {
  chart_api,
  drawing_api,
  drawing_options,
  drawing_point,
  mouse_event_params,
  pane_api,
  price_line_api,
  price_line_options,
  price_scale_api,
  series_api,
  series_data,
  series_options,
  time,
} from "./types.js";

const BLUE = "#2962ff";

export interface detachable_feature {
  detach(): void;
}

function point_value(point: series_data | undefined): number | null {
  if (point === undefined) return null;
  if ("value" in point && typeof point.value === "number") return point.value;
  if ("close" in point && typeof point.close === "number") return point.close;
  return null;
}

function format_time(value: time | null | undefined): string {
  if (value === null || value === undefined) return "";
  const seconds = time_to_utc_seconds(value);
  return new Date(seconds * 1000).toLocaleString();
}

/** Engine-owned anchored text drawing. */
export function create_anchored_text(
  chart: chart_api,
  point: drawing_point,
  options: Partial<drawing_options> = {},
  pane_index = 0,
): drawing_api {
  return chart.add_drawing("text", [point], options, pane_index);
}

/** Engine-owned Bollinger bands indicator, including its shared band fill. */
export function create_bands_indicator(
  chart: chart_api,
  source: series_api,
  period = 20,
  deviation = 2,
  options?: Partial<series_options>,
): [series_api, series_api, series_api] {
  return chart.add_bollinger(source, period, deviation, options);
}

/** Engine-owned rectangle drawing tool object. */
export function create_rectangle_drawing(
  chart: chart_api,
  points: [drawing_point, drawing_point],
  options: Partial<drawing_options> = {},
  pane_index = 0,
): drawing_api {
  return chart.add_drawing("rectangle", points, options, pane_index);
}

/** Engine-owned trend-line drawing object. */
export function create_trend_line(
  chart: chart_api,
  points: [drawing_point, drawing_point],
  options: Partial<drawing_options> = {},
  pane_index = 0,
): drawing_api {
  return chart.add_drawing("trend_line", points, options, pane_index);
}

/** Engine-owned full-height vertical line drawing. */
export function create_vertical_line(
  chart: chart_api,
  point: drawing_point,
  options: Partial<drawing_options> = {},
  pane_index = 0,
): drawing_api {
  return chart.add_drawing("vertical_line", [point], options, pane_index);
}

/** Engine-owned user price line. */
export function create_user_price_line(series: series_api, options: price_line_options): price_line_api {
  return series.create_price_line(options);
}

/** Move a series to the pane's independent overlay scale and return that scale. */
export function use_overlay_price_scale(series: series_api): price_scale_api {
  series.apply_options({ price_scale_id: "" });
  return series.price_scale();
}

export interface partial_price_line_options {
  price: number;
  from_time: time;
  to_time: time;
  color?: string;
  line_width?: number;
  line_style?: 0 | 1 | 2 | 3 | 4;
}

/** A horizontal price line restricted to an exact time range. */
export function create_partial_price_line(
  series: series_api,
  options: partial_price_line_options,
): series_primitive_handle {
  const from = time_to_utc_seconds(options.from_time);
  const to = time_to_utc_seconds(options.to_time);
  return series.attach_primitive({
    pane_views: () => [{
      renderer(ctx) {
        const x1 = ctx.time_to_x(Math.min(from, to));
        const x2 = ctx.time_to_x(Math.max(from, to));
        const y = ctx.price_to_y(options.price);
        if (x1 !== null && x2 !== null && y !== null) {
          ctx.hline(y, x1, x2, options.color ?? BLUE, (options.line_width ?? 2) * ctx.dpr, options.line_style ?? 0);
        }
      },
    }],
    price_axis_views: () => [{ text: String(options.price), price: options.price, coordinate: 0, background_color: options.color ?? BLUE }],
  });
}

export interface session_highlighting_options {
  start_hour_utc?: number;
  end_hour_utc?: number;
  color?: string;
}

/** Shade every bar whose UTC hour falls inside a market session. */
export function create_session_highlighting(
  chart: chart_api,
  series: series_api,
  options: session_highlighting_options = {},
): detachable_feature {
  const start = Math.min(23, Math.max(0, options.start_hour_utc ?? 13));
  const end = Math.min(24, Math.max(0, options.end_hour_utc ?? 20));
  let times = series.data().map((point) => time_to_utc_seconds(point.time));
  const visible_times: number[] = [];
  let request_update: (() => void) | undefined;
  const lower_bound = (value: number): number => {
    let left = 0;
    let right = times.length;
    while (left < right) {
      const middle = (left + right) >>> 1;
      if (times[middle]! < value) left = middle + 1;
      else right = middle;
    }
    return left;
  };
  const refresh_visible = (): void => {
    const range = chart.time_scale().get_visible_range();
    visible_times.length = 0;
    if (range !== null) {
      const from = lower_bound(time_to_utc_seconds(range.from));
      const to = lower_bound(time_to_utc_seconds(range.to) + 1);
      for (let index = from; index < to; index++) visible_times.push(times[index]!);
    }
    request_update?.();
  };
  const refresh_data = (): void => {
    times = series.data().map((point) => time_to_utc_seconds(point.time));
    refresh_visible();
  };
  series.subscribe_data_changed(refresh_data);
  chart.subscribe_visible_time_range_change(refresh_visible);
  refresh_visible();
  const primitive: series_primitive = {
    attached(params) { request_update = params.request_update; },
    detached() { request_update = undefined; },
    pane_views: () => [{
      z_order: "bottom",
      renderer(ctx) {
        const spacing = Math.abs((ctx.logical_to_x(1) ?? 0) - (ctx.logical_to_x(0) ?? 0));
        const width = Math.max(1, spacing);
        for (const timestamp of visible_times) {
          const hour = new Date(timestamp * 1000).getUTCHours();
          const inside = start <= end ? hour >= start && hour < end : hour >= start || hour < end;
          if (!inside) continue;
          const x = ctx.time_to_x(timestamp);
          if (x !== null) ctx.rect(x - width / 2, ctx.pane_top, width, ctx.pane_height, options.color ?? "#2962ff14");
        }
      },
    }],
  };
  const handle = series.attach_primitive(primitive);
  return {
    detach() {
      series.unsubscribe_data_changed(refresh_data);
      chart.unsubscribe_visible_time_range_change(refresh_visible);
      handle.detach();
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
  let timestamp: number | null = null;
  let request_update: (() => void) | undefined;
  const primitive: series_primitive = {
    attached(params) { request_update = params.request_update; },
    detached() { request_update = undefined; },
    pane_views: () => [{
      z_order: "bottom",
      renderer(ctx) {
        if (timestamp === null) return;
        const x = ctx.time_to_x(timestamp);
        const spacing = Math.abs((ctx.logical_to_x(1) ?? 0) - (ctx.logical_to_x(0) ?? 0));
        if (x !== null) ctx.rect(x - spacing / 2, ctx.pane_top, Math.max(1, spacing), ctx.pane_height, options.color ?? "#2962ff1a");
      },
    }],
  };
  const on_move = (event: mouse_event_params): void => {
    timestamp = event.time;
    request_update?.();
  };
  chart.subscribe_crosshair_move(on_move);
  const handle = series.attach_primitive(primitive);
  return {
    detach() {
      chart.unsubscribe_crosshair_move(on_move);
      handle.detach();
    },
  };
}

export interface image_watermark_options {
  x?: number;
  y?: number;
  width?: number;
  height?: number;
  opacity?: number;
}

/** Pane image watermark through the package's bounded Canvas2D escape hatch. */
export function create_image_watermark(
  pane: pane_api,
  image: CanvasImageSource,
  options: image_watermark_options = {},
): canvas_primitive_handle {
  return pane.attach_canvas_primitive({
    pane_views: () => [{
      z_order: "normal",
      renderer(target) {
        target.useMediaCoordinateSpace(({ context, mediaSize }) => {
          const width = options.width ?? Math.min(160, mediaSize.width * 0.3);
          const height = options.height ?? width;
          context.globalAlpha = Math.min(1, Math.max(0, options.opacity ?? 0.2));
          context.drawImage(image, options.x ?? (mediaSize.width - width) / 2, options.y ?? (mediaSize.height - height) / 2, width, height);
        });
      },
    }],
  });
}

export interface tooltip_options {
  series?: series_api;
  class_name?: string;
  format?: (event: mouse_event_params, value: number | null) => string;
}

function make_tooltip(chart: chart_api, class_name: string): HTMLDivElement {
  const host = chart.chart_element();
  const element = document.createElement("div");
  element.className = class_name;
  element.style.cssText = "position:absolute;display:none;pointer-events:none;z-index:5;padding:6px 8px;border-radius:4px;background:#131722e6;color:#f0f3fa;font:12px Inter,sans-serif;white-space:nowrap";
  host.appendChild(element);
  return element;
}

function position_tooltip(chart: chart_api, element: HTMLDivElement, event: mouse_event_params): void {
  if (event.point === null) return;
  const host = chart.chart_element();
  const geometry = event.pane_index === null ? undefined : chart.panes()[event.pane_index]?.get_geometry();
  const x = (geometry?.left ?? 0) + event.point.x + 12;
  const y = (geometry?.top ?? 0) + event.point.y + 12;
  element.style.left = `${Math.max(4, Math.min(x, host.clientWidth - element.offsetWidth - 4))}px`;
  element.style.top = `${Math.max(4, Math.min(y, host.clientHeight - element.offsetHeight - 4))}px`;
}

/** Crosshair tooltip rendered as bounded host DOM rather than canvas text. */
export function create_tooltip(chart: chart_api, options: tooltip_options = {}): detachable_feature {
  const element = make_tooltip(chart, options.class_name ?? "nucleuscharts-tooltip");
  const on_move = (event: mouse_event_params): void => {
    if (event.point === null || event.time === undefined) {
      element.style.display = "none";
      return;
    }
    const selected = options.series === undefined ? event.series_data.values().next().value : event.series_data.get(options.series);
    const value = point_value(selected);
    element.textContent = options.format?.(event, value) ?? `${format_time(event.time)}${value === null ? "" : `  ${value}`}`;
    element.style.display = "block";
    position_tooltip(chart, element, event);
  };
  chart.subscribe_crosshair_move(on_move);
  return {
    detach() {
      chart.unsubscribe_crosshair_move(on_move);
      element.remove();
    },
  };
}

export interface delta_tooltip_options extends tooltip_options {
  anchor_on_click?: boolean;
}

/** Tooltip reporting absolute and percentage change from a click-selected anchor. */
export function create_delta_tooltip(chart: chart_api, options: delta_tooltip_options = {}): detachable_feature {
  const element = make_tooltip(chart, options.class_name ?? "nucleuscharts-delta-tooltip");
  let anchor: number | null = null;
  const value_for = (event: mouse_event_params): number | null => {
    const selected = options.series === undefined ? event.series_data.values().next().value : event.series_data.get(options.series);
    return point_value(selected);
  };
  const on_click = (event: mouse_event_params): void => {
    const value = value_for(event);
    anchor = value === null || event.time === null ? null : value;
  };
  const on_move = (event: mouse_event_params): void => {
    const value = value_for(event);
    if (event.point === null || event.time === undefined || value === null) {
      element.style.display = "none";
      return;
    }
    if (anchor === null && options.anchor_on_click === false) anchor = value;
    const delta = anchor === null ? 0 : value - anchor;
    const percent = anchor === null || anchor === 0 ? null : delta / Math.abs(anchor) * 100;
    element.textContent = options.format?.(event, value) ?? `${format_time(event.time)}  ${value}  Δ ${delta.toFixed(2)}${percent === null ? "" : ` (${percent.toFixed(2)}%)`}`;
    element.style.display = "block";
    position_tooltip(chart, element, event);
  };
  chart.subscribe_click(on_click);
  chart.subscribe_crosshair_move(on_move);
  return {
    detach() {
      chart.unsubscribe_click(on_click);
      chart.unsubscribe_crosshair_move(on_move);
      element.remove();
    },
  };
}

export interface volume_profile_data {
  price: number;
  volume: number;
}

export interface volume_profile_options {
  bins?: number;
  width_percent?: number;
  color?: string;
  side?: "left" | "right";
}

export interface volume_profile_handle extends detachable_feature {
  set_data(data: readonly volume_profile_data[]): void;
}

/** Horizontal price-by-volume profile attached to a series' price scale. */
export function create_volume_profile(
  series: series_api,
  initial_data: readonly volume_profile_data[],
  options: volume_profile_options = {},
): volume_profile_handle {
  const count = Math.min(512, Math.max(1, Math.floor(options.bins ?? 24)));
  let profile: { low: number; high: number; volume: number }[] = [];
  let largest_volume = 1;
  let request_update: (() => void) | undefined;
  const rebuild = (data: readonly volume_profile_data[]): void => {
    let min = Number.POSITIVE_INFINITY;
    let max = Number.NEGATIVE_INFINITY;
    for (const point of data) {
      if (!Number.isFinite(point.price) || !Number.isFinite(point.volume) || point.volume <= 0) continue;
      min = Math.min(min, point.price);
      max = Math.max(max, point.price);
    }
    if (!Number.isFinite(min) || !Number.isFinite(max)) {
      profile = [];
      largest_volume = 1;
      return;
    }
    const step = (max - min || 1) / count;
    const volumes = Array<number>(count).fill(0);
    for (const point of data) {
      if (!Number.isFinite(point.price) || !Number.isFinite(point.volume) || point.volume <= 0) continue;
      volumes[Math.min(count - 1, Math.floor((point.price - min) / step))]! += point.volume;
    }
    profile = volumes.map((volume, index) => ({ low: min + index * step, high: min + (index + 1) * step, volume }));
    largest_volume = Math.max(...volumes, 1);
  };
  rebuild(initial_data);
  const primitive: series_primitive = {
    attached(params) { request_update = params.request_update; },
    detached() { request_update = undefined; },
    autoscale_info: () => {
      const first = profile[0];
      const last = profile[profile.length - 1];
      return first === undefined || last === undefined ? null : { min: first.low, max: last.high };
    },
    pane_views: () => [{
      renderer(ctx: series_primitive_draw_context) {
        if (profile.length === 0) return;
        const available = ctx.pane_width * Math.min(1, Math.max(0.01, options.width_percent ?? 0.3));
        profile.forEach((bin) => {
          if (bin.volume <= 0) return;
          const y1 = ctx.price_to_y(bin.low);
          const y2 = ctx.price_to_y(bin.high);
          if (y1 === null || y2 === null) return;
          const top = Math.min(y1, y2);
          const width = available * bin.volume / largest_volume;
          const x = options.side === "left" ? ctx.pane_left : ctx.pane_left + ctx.pane_width - width;
          ctx.rect(x, top, width, Math.max(1, Math.abs(y2 - y1)), options.color ?? "#2962ff66");
        });
      },
    }],
  };
  const handle = series.attach_primitive(primitive);
  return {
    set_data(next) {
      rebuild(next);
      request_update?.();
    },
    detach() { handle.detach(); },
  };
}

export interface price_alert {
  id: number;
  price: number;
  title: string;
  expires_at?: number;
}

export interface price_alert_options {
  color?: string;
  on_trigger?: (alert: price_alert, value: number) => void;
  /** Hard ceiling for retained alerts (default 1,000; oldest is removed first). */
  max_alerts?: number;
}

export interface price_alerts_handle extends detachable_feature {
  add(price: number, title?: string, expires_at?: number): price_alert;
  remove(id: number): void;
  alerts(): readonly price_alert[];
}

/** User and expiring price alerts backed by engine price lines and series updates. */
export function create_expiring_price_alerts(series: series_api, options: price_alert_options = {}): price_alerts_handle {
  let next_id = 1;
  let previous = series.last_value_data(true)?.value ?? null;
  const max_alerts = Math.min(10_000, Math.max(1, Math.floor(options.max_alerts ?? 1_000)));
  const entries = new Map<number, { alert: price_alert; line: price_line_api; timer?: ReturnType<typeof setTimeout> }>();
  const remove = (id: number): void => {
    const entry = entries.get(id);
    if (entry === undefined) return;
    if (entry.timer !== undefined) clearTimeout(entry.timer);
    entry.line.remove();
    entries.delete(id);
  };
  const on_data = (): void => {
    const value = series.last_value_data(true)?.value ?? null;
    if (value === null) return;
    if (previous !== null) {
      for (const { alert } of entries.values()) {
        if ((previous < alert.price && value >= alert.price) || (previous > alert.price && value <= alert.price)) options.on_trigger?.(alert, value);
      }
    }
    previous = value;
  };
  series.subscribe_data_changed(on_data);
  return {
    add(price, title = "Alert", expires_at) {
      while (entries.size >= max_alerts) {
        const oldest = entries.keys().next().value;
        if (oldest === undefined) break;
        remove(oldest);
      }
      const alert = { id: next_id++, price, title, ...(expires_at === undefined ? {} : { expires_at }) };
      const line = series.create_price_line({ price, title, color: options.color ?? "#f59e0b", line_style: "dashed" });
      const entry: { alert: price_alert; line: price_line_api; timer?: ReturnType<typeof setTimeout> } = { alert, line };
      if (expires_at !== undefined) entry.timer = setTimeout(() => remove(alert.id), Math.max(0, expires_at - Date.now()));
      entries.set(alert.id, entry);
      return alert;
    },
    remove,
    alerts: () => [...entries.values()].map((entry) => entry.alert),
    detach() {
      series.unsubscribe_data_changed(on_data);
      for (const id of [...entries.keys()]) remove(id);
    },
  };
}

export interface user_price_alerts_options extends price_alert_options {
  title?: (price: number) => string;
  expires_in_ms?: number;
}

/** Accessible context menu that places a user price alert at the pointer price. */
export function create_user_price_alerts(
  chart: chart_api,
  series: series_api,
  options: user_price_alerts_options = {},
): price_alerts_handle {
  const alerts = create_expiring_price_alerts(series, options);
  const host = chart.chart_element();
  let menu: HTMLDivElement | null = null;
  const close_menu = (): void => {
    menu?.remove();
    menu = null;
  };
  const on_context = (event: MouseEvent): void => {
    const bounds = host.getBoundingClientRect();
    const pane = chart.panes().find((candidate) => candidate.get_series().includes(series));
    const geometry = pane?.get_geometry();
    if (geometry === undefined) return;
    const local_y = event.clientY - bounds.top - geometry.top;
    if (local_y < 0 || local_y > geometry.height) return;
    const price = series.coordinate_to_price(local_y);
    if (price === null) return;
    event.preventDefault();
    close_menu();
    const popup = document.createElement("div");
    popup.className = "nucleuscharts-price-alert-menu";
    popup.setAttribute("role", "menu");
    popup.style.cssText = "position:absolute;z-index:10;padding:4px;border:1px solid #2a2e39;border-radius:4px;background:#131722;color:#f0f3fa;font:12px Inter,sans-serif";
    popup.style.left = `${event.clientX - bounds.left}px`;
    popup.style.top = `${event.clientY - bounds.top}px`;
    const add = document.createElement("button");
    add.type = "button";
    add.setAttribute("role", "menuitem");
    add.textContent = `Add alert at ${series.price_formatter()(price)}`;
    add.style.cssText = "border:0;background:transparent;color:inherit;padding:6px 8px;cursor:pointer";
    add.addEventListener("click", () => {
      alerts.add(price, options.title?.(price) ?? `Alert ${series.price_formatter()(price)}`, options.expires_in_ms === undefined ? undefined : Date.now() + options.expires_in_ms);
      close_menu();
    }, { once: true });
    popup.appendChild(add);
    host.appendChild(popup);
    menu = popup;
    add.focus();
  };
  const on_key = (event: KeyboardEvent): void => {
    if (event.key === "Escape") close_menu();
  };
  host.addEventListener("contextmenu", on_context);
  host.addEventListener("keydown", on_key);
  return {
    add: alerts.add,
    remove: alerts.remove,
    alerts: alerts.alerts,
    detach() {
      host.removeEventListener("contextmenu", on_context);
      host.removeEventListener("keydown", on_key);
      close_menu();
      alerts.detach();
    },
  };
}

export interface accessibility_options {
  chart_title?: string;
  lang?: string;
  page_step?: number;
  describe_point?: (point: accessibility_point) => string;
}

export interface accessibility_point {
  series_position: number;
  series_count: number;
  point_position: number;
  point_count: number;
  time: string;
  value: string | null;
}

export interface accessibility_handle extends detachable_feature {
  focus(): void;
  refresh(): void;
}

/** Fixed-size semantic layer with keyboard point/series navigation and live announcements. */
export function enable_accessibility(chart: chart_api, options: accessibility_options = {}): accessibility_handle {
  const host = chart.chart_element();
  const previous = {
    role: host.getAttribute("role"),
    label: host.getAttribute("aria-label"),
    tab: host.getAttribute("tabindex"),
  };
  host.setAttribute("role", "application");
  host.setAttribute("aria-label", options.chart_title ?? "Interactive financial chart");
  host.setAttribute("tabindex", "0");
  const canvases = [...host.querySelectorAll("canvas")].map((canvas) => ({
    canvas,
    aria_hidden: canvas.getAttribute("aria-hidden"),
  }));
  for (const { canvas } of canvases) canvas.setAttribute("aria-hidden", "true");
  const live = document.createElement("div");
  live.setAttribute("aria-live", "assertive");
  live.setAttribute("aria-atomic", "true");
  if (options.lang !== undefined) live.lang = options.lang;
  live.style.cssText = "position:absolute;width:1px;height:1px;padding:0;margin:-1px;overflow:hidden;clip:rect(0,0,0,0);white-space:nowrap;border:0";
  host.appendChild(live);
  let series = chart.panes().flatMap((pane) => pane.get_series());
  let series_index = 0;
  let point_index = Math.max(0, (series[0]?.data().length ?? 1) - 1);
  const announce = (): void => {
    const active = series[series_index];
    const data = active?.data() ?? [];
    if (active === undefined || data.length === 0) {
      live.textContent = "No chart data";
      return;
    }
    point_index = Math.min(Math.max(0, point_index), data.length - 1);
    const point = data[point_index];
    const value = point_value(point);
    const description: accessibility_point = {
      series_position: series_index + 1,
      series_count: series.length,
      point_position: point_index + 1,
      point_count: data.length,
      time: format_time(point?.time),
      value: value === null ? null : active.price_formatter()(value),
    };
    live.textContent = options.describe_point?.(description) ?? `Series ${description.series_position} of ${description.series_count}. Point ${description.point_position} of ${description.point_count}. ${description.time}. ${description.value ?? "No value"}.`;
    if (point !== undefined && value !== null) chart.set_crosshair_position(value, point.time, active);
  };
  const on_key = (event: KeyboardEvent): void => {
    const active = series[series_index];
    const length = active?.data().length ?? 0;
    if (length === 0) return;
    const page = Math.max(1, Math.floor(options.page_step ?? 10));
    if (event.key === "ArrowLeft") point_index--;
    else if (event.key === "ArrowRight") point_index++;
    else if (event.key === "PageDown") point_index -= page;
    else if (event.key === "PageUp") point_index += page;
    else if (event.key === "Home") point_index = 0;
    else if (event.key === "End") point_index = length - 1;
    else if (event.key === "ArrowUp") series_index = (series_index - 1 + series.length) % series.length;
    else if (event.key === "ArrowDown") series_index = (series_index + 1) % series.length;
    else if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    announce();
  };
  host.addEventListener("keydown", on_key);
  return {
    focus() { host.focus(); announce(); },
    refresh() {
      series = chart.panes().flatMap((pane) => pane.get_series());
      series_index = Math.min(series_index, Math.max(0, series.length - 1));
      announce();
    },
    detach() {
      host.removeEventListener("keydown", on_key);
      live.remove();
      chart.clear_crosshair_position();
      for (const { canvas, aria_hidden } of canvases) {
        if (aria_hidden === null) canvas.removeAttribute("aria-hidden");
        else canvas.setAttribute("aria-hidden", aria_hidden);
      }
      for (const [attribute, value] of [["role", previous.role], ["aria-label", previous.label], ["tabindex", previous.tab]] as const) {
        if (value === null) host.removeAttribute(attribute);
        else host.setAttribute(attribute, value);
      }
    },
  };
}
