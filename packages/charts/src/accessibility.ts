/** Active-point-only accessibility controller modelled on TradingView's official pane plugin. */

import {
  attach_native_accessibility_focus,
  time_to_utc_seconds,
} from "./impl.js";
import type { native_accessibility_focus_handle } from "./impl.js";
import type {
  chart_api,
  pane_api,
  localization_options,
  series_api,
  series_data,
  time,
} from "./types.js";

const HIDDEN = "position:absolute;width:1px;height:1px;margin:-1px;padding:0;overflow:hidden;clip:rect(0 0 0 0);clip-path:inset(50%);white-space:nowrap;border:0";
const UPDATE_DEBOUNCE_MS = 150;
const UPDATE_MAX_SERIES = 3;
const MIN_ZOOM_SPAN = 2;
const ZOOM_STEP = 0.2;
const CANVAS_PREVIOUS_ARIA = "data-nucleuscharts-a11y-previous-aria-hidden";

export interface accessibility_point {
  series_position: number;
  series_count: number;
  point_position: number;
  point_count: number;
  time: string;
  value: string | null;
}

export interface accessibility_messages {
  role_description: string;
  no_value: string;
  no_data: (label: string, scope_note: string) => string;
  in_view: string;
  default_series_label: (position: number) => string;
  pane_label: (title: string, series_count: number, series_label: string | null) => string;
  description: (multi_series: boolean) => string;
  help: (multi_series: boolean, page_step: number) => string;
  shortcuts_hint: string;
  shortcuts_title: string;
  point: (point: accessibility_point, label: string, values: string) => string;
  series_position: (label: string, position: number, total: number, point: string) => string;
  summary: (summary: accessibility_summary) => string;
  series_update: (label: string, count: number, scope_note: string, latest: string) => string;
  data_updated: (summaries: readonly string[], total: number, shown_max: number) => string;
}

export interface accessibility_summary {
  label: string;
  count: number;
  scope_note: string;
  first_value: string;
  first_time: string;
  last_value: string;
  last_time: string;
  direction: "up" | "down" | "unchanged";
  change_value: string;
  percent: string | null;
  low_value: string;
  low_time: string;
  high_value: string;
  high_time: string;
}

export interface accessibility_options {
  chart_title?: string | ((pane_index: number) => string);
  show_focus_indicator?: boolean;
  focus_indicator_color?: string;
  focus_indicator_size?: number;
  announce_data_updates?: boolean | "active" | ((pane_index: number) => boolean);
  page_step?: number;
  data_scope?: "all" | "visible";
  price_formatter?: (value: number) => string;
  time_formatter?: (value: time) => string;
  series_label?: (series: series_api, index: number) => string;
  describe_chart?: (points: readonly series_data[], series_label: string) => string;
  /** Compatibility hook retained from the first Nucleus helper. */
  describe_point?: (point: accessibility_point) => string;
  messages?: Partial<accessibility_messages>;
  lang?: string;
  show_shortcuts?: boolean;
  high_contrast?: boolean | "auto" | (() => boolean);
  on_high_contrast_change?: (enabled: boolean) => void;
}

interface resolved_accessibility_options {
  chart_title: string | ((pane_index: number) => string);
  show_focus_indicator: boolean;
  focus_indicator_color: string;
  focus_indicator_size: number;
  announce_data_updates: boolean | "active" | ((pane_index: number) => boolean);
  page_step: number;
  data_scope: "all" | "visible";
  price_formatter?: (value: number) => string;
  time_formatter?: (value: time) => string;
  series_label?: (series: series_api, index: number) => string;
  describe_chart?: (points: readonly series_data[], series_label: string) => string;
  describe_point?: (point: accessibility_point) => string;
  messages: accessibility_messages;
  lang?: string;
  show_shortcuts: boolean;
  high_contrast: boolean | "auto" | (() => boolean);
  on_high_contrast_change?: (enabled: boolean) => void;
}

export interface accessibility_handle {
  detach(): void;
  focus(pane_index?: number): void;
  refresh(): void;
  apply_options(options: accessibility_options): void;
}

const default_messages: accessibility_messages = {
  role_description: "Interactive chart pane",
  no_value: "no value",
  no_data: (label, scope) => `${label}: no data points${scope}.`,
  in_view: "in view",
  default_series_label: (position) => `Series ${position}`,
  pane_label: (title, count, label) => `${title}. ${count > 1 ? `${count} series. ` : label ? `${label}. ` : ""}Press H for keyboard help.`,
  description: (multi) => multi
    ? "Use the left and right arrow keys to move between data points, and the up and down arrows to switch series."
    : "Use the left and right arrow keys to move between data points.",
  help: (multi, step) => `Keyboard controls. Left and right arrows move between data points. ${multi ? "Up and down arrows switch between series. " : ""}Page Up jumps ${step} points forward, Page Down ${step} points back. Home and End jump to the first and last points. Plus and minus zoom the chart in and out. Enter or Space reads a summary of the series.`,
  shortcuts_hint: "Press H for keyboard shortcuts",
  shortcuts_title: "Keyboard shortcuts",
  point: (point, label, values) => `${label} ${values}, ${point.time}. Point ${point.point_position} of ${point.point_count}.`,
  series_position: (label, position, total, point) => `${label}, series ${position} of ${total}.${point}`,
  summary: (s) => `${s.label} with ${s.count} data points${s.scope_note}. From ${s.first_value} on ${s.first_time} to ${s.last_value} on ${s.last_time}. Overall ${s.direction} by ${s.change_value}${s.percent === null ? "" : `, ${s.percent} percent`}. Lowest ${s.low_value} on ${s.low_time}, highest ${s.high_value} on ${s.high_time}.`,
  series_update: (label, count, scope, latest) => `${label}: ${count} data points${scope}. Latest ${latest}`,
  data_updated: (summaries, total, shown_max) => total === 1
    ? `Chart data updated. ${summaries[0]}.`
    : `Chart data updated. ${total} series changed. ${summaries.slice(0, shown_max).join(" ")}.${total > shown_max ? ` ${total - shown_max} more series changed.` : ""}`,
};

function resolve_options(options: accessibility_options): resolved_accessibility_options {
  return {
    chart_title: options.chart_title ?? "Interactive financial chart",
    show_focus_indicator: options.show_focus_indicator ?? true,
    focus_indicator_color: options.focus_indicator_color ?? "#2962FF",
    focus_indicator_size: Math.max(4, Math.min(128, options.focus_indicator_size ?? 14)),
    announce_data_updates: options.announce_data_updates ?? "active",
    page_step: Math.max(1, Math.floor(options.page_step ?? 10)),
    data_scope: options.data_scope ?? "visible",
    price_formatter: options.price_formatter,
    time_formatter: options.time_formatter,
    series_label: options.series_label,
    describe_chart: options.describe_chart,
    describe_point: options.describe_point,
    messages: { ...default_messages, ...options.messages },
    lang: options.lang,
    show_shortcuts: options.show_shortcuts ?? false,
    high_contrast: options.high_contrast ?? "auto",
    on_high_contrast_change: options.on_high_contrast_change,
  };
}

function point_value(point: series_data | undefined): number | undefined {
  if (point === undefined) return undefined;
  if ("value" in point && typeof point.value === "number") return point.value;
  if ("close" in point && typeof point.close === "number") return point.close;
  return undefined;
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value));
}

class LiveWriter {
  private frame: number | null = null;
  constructor(private readonly region: () => HTMLElement | null) {}
  write(message: string): void {
    const target = this.region();
    if (target === null || message.length === 0) return;
    target.textContent = "";
    if (this.frame !== null) cancelAnimationFrame(this.frame);
    this.frame = requestAnimationFrame(() => {
      this.frame = null;
      const current = this.region();
      if (current !== null) current.textContent = message;
    });
  }
  dispose(): void {
    if (this.frame !== null) cancelAnimationFrame(this.frame);
    this.frame = null;
  }
}

class PaneAccessibility {
  private readonly layer = document.createElement("div");
  private readonly description = document.createElement("div");
  private readonly live = document.createElement("div");
  private readonly hint = document.createElement("div");
  private readonly panel = document.createElement("div");
  private readonly writer = new LiveWriter(() => this.live);
  private series: series_api[] = [];
  private points: readonly series_data[] = [];
  private subscriptions = new Map<series_api, () => void>();
  private focus_handles = new Map<series_api, native_accessibility_focus_handle>();
  private dirty = new Set<series_api>();
  private series_index = 0;
  private point_index = -1;
  private focused = false;
  private shortcuts_open = false;
  private high_contrast = false;
  private contrast_queries: MediaQueryList[] = [];

  constructor(
    private readonly controller: AccessibilityController,
    readonly pane: pane_api,
    readonly pane_index: number,
  ) {
    this.layer.className = "nucleuscharts-a11y-layer";
    this.layer.tabIndex = 0;
    this.layer.setAttribute("role", "application");
    this.layer.style.cssText = "position:absolute;outline:none;outline-offset:-3px;pointer-events:none;z-index:5";

    this.description.id = `nucleuscharts-a11y-description-${controller.next_id()}`;
    this.description.className = "nucleuscharts-a11y-description";
    this.description.style.cssText = HIDDEN;
    this.layer.setAttribute("aria-describedby", this.description.id);
    this.layer.appendChild(this.description);

    this.live.className = "nucleuscharts-a11y-live-region";
    this.live.setAttribute("aria-live", "assertive");
    this.live.setAttribute("aria-atomic", "true");
    this.live.style.cssText = HIDDEN;
    this.layer.appendChild(this.live);

    this.hint.className = "nucleuscharts-a11y-shortcuts-hint";
    this.hint.setAttribute("aria-hidden", "true");
    this.layer.appendChild(this.hint);
    this.panel.className = "nucleuscharts-a11y-shortcuts-panel";
    this.panel.setAttribute("aria-hidden", "true");
    this.layer.appendChild(this.panel);

    controller.host.appendChild(this.layer);
    this.layer.addEventListener("keydown", this.on_key);
    this.layer.addEventListener("focusin", this.on_focus);
    this.layer.addEventListener("focusout", this.on_blur);
    this.contrast_queries = ["(prefers-contrast: more)", "(forced-colors: active)"].map((query) => matchMedia(query));
    for (const query of this.contrast_queries) query.addEventListener("change", this.on_contrast_change);
    this.sync_series();
    this.apply_options();
  }

  focus(): void {
    this.layer.focus();
  }

  apply_options(): void {
    const options = this.controller.options;
    this.high_contrast = typeof options.high_contrast === "function"
      ? options.high_contrast()
      : options.high_contrast === "auto"
        ? this.contrast_queries.some((query) => query.matches)
        : options.high_contrast;
    options.on_high_contrast_change?.(this.high_contrast);
    this.layer.setAttribute("aria-roledescription", options.messages.role_description);
    this.layer.setAttribute("aria-label", this.pane_label());
    const lang = options.lang ?? this.controller.locale();
    for (const element of [this.layer, this.live]) {
      if (lang === undefined || lang.length === 0) element.removeAttribute("lang");
      else element.lang = lang;
    }
    this.description.textContent = options.messages.description(this.series.length > 1);
    this.style_outline();
    this.render_shortcuts();
    this.style_shortcuts();
    this.update_focus_ring();
    this.update_geometry();
  }

  update_geometry(): void {
    const geometry = this.pane.get_geometry();
    this.layer.style.left = `${geometry.left}px`;
    this.layer.style.top = `${geometry.top}px`;
    this.layer.style.width = `${geometry.width}px`;
    this.layer.style.height = `${geometry.height}px`;
    this.update_focus_ring();
  }

  sync_series(): void {
    const current = this.pane.get_series();
    for (const [series, handler] of this.subscriptions) {
      if (!current.includes(series)) {
        series.unsubscribe_data_changed(handler);
        this.subscriptions.delete(series);
        this.focus_handles.get(series)?.detach();
        this.focus_handles.delete(series);
        this.dirty.delete(series);
      }
    }
    for (const series of current) {
      if (this.subscriptions.has(series)) continue;
      const handler = (): void => this.on_data_changed(series);
      series.subscribe_data_changed(handler);
      this.subscriptions.set(series, handler);
      this.focus_handles.set(series, attach_native_accessibility_focus(series, this.focus_options_json()));
    }
    this.series = current;
    this.series_index = clamp(this.series_index, 0, Math.max(0, this.series.length - 1));
    this.refresh_points();
    this.layer.setAttribute("aria-label", this.pane_label());
    this.description.textContent = this.controller.options.messages.description(this.series.length > 1);
    this.render_shortcuts();
    this.update_focus_ring();
  }

  take_update_summaries(): string[] {
    const changed = this.series.filter((series) => this.dirty.has(series));
    this.dirty.clear();
    return changed.map((series) => this.update_summary(series));
  }

  detach(): void {
    this.writer.dispose();
    this.layer.removeEventListener("keydown", this.on_key);
    this.layer.removeEventListener("focusin", this.on_focus);
    this.layer.removeEventListener("focusout", this.on_blur);
    for (const query of this.contrast_queries) query.removeEventListener("change", this.on_contrast_change);
    for (const [series, handler] of this.subscriptions) series.unsubscribe_data_changed(handler);
    for (const handle of this.focus_handles.values()) handle.detach();
    this.subscriptions.clear();
    this.focus_handles.clear();
    this.layer.remove();
  }

  private readonly on_focus = (): void => {
    this.focused = true;
    this.controller.set_active(this);
    this.style_outline();
    this.style_shortcuts();
    this.update_focus_ring();
  };

  private readonly on_blur = (event: FocusEvent): void => {
    if (event.relatedTarget instanceof Node && this.layer.contains(event.relatedTarget)) return;
    this.focused = false;
    this.shortcuts_open = false;
    this.style_outline();
    this.style_shortcuts();
    this.update_focus_ring();
  };

  private readonly on_contrast_change = (): void => this.apply_options();

  private readonly on_key = (event: KeyboardEvent): void => {
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    if (this.series.length === 0) return;
    switch (event.key) {
      case "ArrowRight": this.move_point(1); break;
      case "ArrowLeft": this.move_point(-1); break;
      case "ArrowUp": this.move_series(-1); break;
      case "ArrowDown": this.move_series(1); break;
      case "PageUp": this.move_point(this.controller.options.page_step); break;
      case "PageDown": this.move_point(-this.controller.options.page_step); break;
      case "Home": if (this.points.length > 0) this.set_point(0); break;
      case "End": if (this.points.length > 0) this.set_point(this.points.length - 1); break;
      case "+":
      case "=": this.zoom(true); break;
      case "-":
      case "_": this.zoom(false); break;
      case "Enter":
      case " ": this.writer.write(this.describe_chart()); break;
      case "h":
      case "H":
        this.writer.write(this.controller.options.messages.help(this.series.length > 1, this.controller.options.page_step));
        this.shortcuts_open = !this.shortcuts_open && this.controller.options.show_shortcuts;
        this.style_shortcuts();
        break;
      default: return;
    }
    event.preventDefault();
  };

  private on_data_changed(series: series_api): void {
    if (this.focused && series === this.active_series()) {
      this.refresh_points();
      this.update_focus_ring();
    }
    if (this.controller.announces(this.pane_index)) {
      this.dirty.add(series);
      this.controller.mark_dirty(this);
    }
  }

  private active_series(): series_api | undefined {
    return this.series[this.series_index];
  }

  private refresh_points(): void {
    this.points = this.active_series()?.data() ?? [];
    this.point_index = Math.min(this.point_index, this.points.length - 1);
  }

  private move_point(delta: number): void {
    if (this.points.length === 0) return;
    const next = this.point_index < 0 ? this.first_visible_index() : this.point_index + delta;
    this.set_point(clamp(next, 0, this.points.length - 1));
  }

  private move_series(delta: number): void {
    if (this.series.length <= 1) return;
    const next = clamp(this.series_index + delta, 0, this.series.length - 1);
    if (next === this.series_index) return;
    const previous = this.points[this.point_index];
    const target = previous === undefined ? null : this.logical_index(previous);
    this.series_index = next;
    this.refresh_points();
    if (this.points.length > 0 && this.point_index >= 0) {
      this.point_index = target === null ? clamp(this.point_index, 0, this.points.length - 1) : this.nearest_index(target);
      this.scroll_into_view();
    }
    this.layer.setAttribute("aria-label", this.pane_label());
    this.update_focus_ring();
    const point = this.point_index < 0 ? "" : ` ${this.describe_point(this.point_index)}`;
    this.writer.write(this.controller.options.messages.series_position(
      this.series_label(this.active_series(), next), next + 1, this.series.length, point,
    ));
  }

  private set_point(index: number): void {
    this.point_index = index;
    this.scroll_into_view();
    this.update_focus_ring();
    this.writer.write(this.describe_point(index));
  }

  private logical_index(point: series_data): number | null {
    return this.controller.chart.time_scale().time_to_index(time_to_utc_seconds(point.time), true);
  }

  private lower_bound(points: readonly series_data[], target: number): number {
    let low = 0;
    let high = points.length;
    while (low < high) {
      const middle = (low + high) >> 1;
      const logical = this.logical_index(points[middle] as series_data);
      if (logical !== null && logical < target) low = middle + 1;
      else high = middle;
    }
    return low;
  }

  private nearest_index(target: number): number {
    const upper = this.lower_bound(this.points, target);
    if (upper <= 0) return 0;
    if (upper >= this.points.length) return this.points.length - 1;
    const before = this.logical_index(this.points[upper - 1] as series_data);
    const after = this.logical_index(this.points[upper] as series_data);
    if (before === null || after === null) return upper;
    return target - before <= after - target ? upper - 1 : upper;
  }

  private visible_bounds(points: readonly series_data[]): { from: number; to: number } | null {
    const range = this.controller.chart.time_scale().get_visible_logical_range();
    if (range === null || points.length === 0) return null;
    const from = this.lower_bound(points, Math.ceil(range.from));
    const to = this.lower_bound(points, Math.floor(range.to) + 1) - 1;
    return from >= points.length || to < from ? null : { from, to };
  }

  private first_visible_index(): number {
    const range = this.controller.chart.time_scale().get_visible_logical_range();
    if (range === null || this.points.length === 0) return 0;
    return clamp(this.lower_bound(this.points, Math.ceil(range.from)), 0, this.points.length - 1);
  }

  private scroll_into_view(): void {
    const point = this.points[this.point_index];
    const logical = point === undefined ? null : this.logical_index(point);
    const scale = this.controller.chart.time_scale();
    const range = scale.get_visible_logical_range();
    if (logical === null || range === null) return;
    const span = range.to - range.from;
    const margin = span > 4 ? 1 : 0;
    let from: number;
    if (logical < range.from + margin) from = logical - margin;
    else if (logical > range.to - margin) from = logical - span + margin;
    else return;
    const last = this.points.at(-1);
    const last_logical = last === undefined ? logical : this.logical_index(last) ?? logical;
    from = clamp(from, 0, Math.max(0, last_logical));
    scale.set_visible_logical_range({ from, to: from + span });
  }

  private zoom(zoom_in: boolean): void {
    const scale = this.controller.chart.time_scale();
    const range = scale.get_visible_logical_range();
    if (range === null) return;
    const span = range.to - range.from;
    if (span <= 0) return;
    const next_span = Math.max(MIN_ZOOM_SPAN, span * (zoom_in ? 1 - ZOOM_STEP : 1 + ZOOM_STEP));
    const point = this.points[this.point_index];
    const anchor = point === undefined ? (range.from + range.to) / 2 : this.logical_index(point) ?? (range.from + range.to) / 2;
    const ratio = (anchor - range.from) / span;
    const from = anchor - ratio * next_span;
    scale.set_visible_logical_range({ from, to: from + next_span });
    this.update_focus_ring();
  }

  private scoped_points(points: readonly series_data[]): readonly series_data[] {
    if (this.controller.options.data_scope === "all") return points;
    const bounds = this.visible_bounds(points);
    return bounds === null ? [] : points.slice(bounds.from, bounds.to + 1);
  }

  private series_label(series: series_api | undefined, index: number): string {
    if (series === undefined) return "";
    const custom = this.controller.options.series_label;
    if (custom !== undefined) return custom(series, index);
    const title = series.options().title ?? "";
    return title.length > 0 ? title : this.controller.options.messages.default_series_label(index + 1);
  }

  private format_value(value: number | undefined, series = this.active_series()): string {
    if (value === undefined) return this.controller.options.messages.no_value;
    if (this.controller.options.price_formatter !== undefined) return this.controller.options.price_formatter(value);
    const chart_formatter = this.controller.localization().price_formatter;
    if (chart_formatter !== undefined) return chart_formatter(value);
    try { return series?.price_formatter()(value) ?? String(value); }
    catch { return String(value); }
  }

  private format_time(value: time): string {
    if (this.controller.options.time_formatter !== undefined) return this.controller.options.time_formatter(value);
    const formatter = this.controller.localization().time_formatter;
    const seconds = time_to_utc_seconds(value);
    if (formatter !== undefined) return formatter(seconds);
    return new Date(seconds * 1000).toLocaleDateString(this.controller.locale(), {
      year: "numeric", month: "short", day: "numeric", timeZone: "UTC",
    });
  }

  private describe_values(point: series_data, series = this.active_series()): string {
    if ("close" in point && typeof point.close === "number") {
      const parts: string[] = [];
      if ("open" in point && typeof point.open === "number") parts.push(`open ${this.format_value(point.open, series)}`);
      if (typeof point.high === "number") parts.push(`high ${this.format_value(point.high, series)}`);
      if (typeof point.low === "number") parts.push(`low ${this.format_value(point.low, series)}`);
      parts.push(`close ${this.format_value(point.close, series)}`);
      return parts.join(", ");
    }
    return this.format_value(point_value(point), series);
  }

  private describe_point(index: number): string {
    const point = this.points[index];
    if (point === undefined) return "";
    const value = point_value(point);
    const description: accessibility_point = {
      series_position: this.series_index + 1,
      series_count: this.series.length,
      point_position: index + 1,
      point_count: this.points.length,
      time: this.format_time(point.time),
      value: value === undefined ? null : this.format_value(value),
    };
    return this.controller.options.describe_point?.(description)
      ?? this.controller.options.messages.point(description, this.series_label(this.active_series(), this.series_index), this.describe_values(point));
  }

  private describe_chart(): string {
    const label = this.series_label(this.active_series(), this.series_index);
    const scoped = this.scoped_points(this.points);
    if (this.controller.options.describe_chart !== undefined) {
      return this.controller.options.describe_chart(scoped, label);
    }
    const valued = scoped.filter((point) => point_value(point) !== undefined);
    const scope = this.controller.options.data_scope === "visible" && this.controller.options.messages.in_view.length > 0
      ? ` ${this.controller.options.messages.in_view}`
      : "";
    if (valued.length === 0) return this.controller.options.messages.no_data(label, scope);
    const first = valued[0] as series_data;
    const last = valued.at(-1) as series_data;
    let low: series_data = first;
    let high: series_data = first;
    for (const point of valued) {
      if ((point_value(point) as number) < (point_value(low) as number)) low = point;
      if ((point_value(point) as number) > (point_value(high) as number)) high = point;
    }
    const first_value = point_value(first) as number;
    const last_value = point_value(last) as number;
    const change = last_value - first_value;
    const percent = first_value === 0 ? null : Math.abs(change / first_value * 100).toLocaleString(this.controller.locale(), {
      minimumFractionDigits: 2, maximumFractionDigits: 2,
    });
    return this.controller.options.messages.summary({
      label,
      count: valued.length,
      scope_note: scope,
      first_value: this.format_value(first_value),
      first_time: this.format_time(first.time),
      last_value: this.format_value(last_value),
      last_time: this.format_time(last.time),
      direction: change > 0 ? "up" : change < 0 ? "down" : "unchanged",
      change_value: this.format_value(Math.abs(change)),
      percent,
      low_value: this.format_value(point_value(low)),
      low_time: this.format_time(low.time),
      high_value: this.format_value(point_value(high)),
      high_time: this.format_time(high.time),
    });
  }

  private update_summary(series: series_api): string {
    const data = series.data();
    const scoped = this.scoped_points(data);
    let latest: number | undefined;
    for (let index = data.length - 1; index >= 0 && latest === undefined; index--) latest = point_value(data[index]);
    const scope = this.controller.options.data_scope === "visible" && this.controller.options.messages.in_view.length > 0
      ? ` ${this.controller.options.messages.in_view}`
      : "";
    return this.controller.options.messages.series_update(
      this.series_label(series, this.series.indexOf(series)), scoped.length, scope, this.format_value(latest, series),
    );
  }

  private pane_label(): string {
    const title = typeof this.controller.options.chart_title === "function"
      ? this.controller.options.chart_title(this.pane_index)
      : this.controller.options.chart_title;
    return this.controller.options.messages.pane_label(
      title, this.series.length, this.active_series() === undefined ? null : this.series_label(this.active_series(), this.series_index),
    );
  }

  private focus_options_json(): string {
    return JSON.stringify({
      color: this.controller.options.focus_indicator_color,
      size: this.controller.options.focus_indicator_size,
      high_contrast: this.high_contrast,
    });
  }

  private update_focus_ring(): void {
    const active = this.active_series();
    const point = this.points[this.point_index];
    const visible = this.focused && this.controller.options.show_focus_indicator && active !== undefined
      && point !== undefined && point_value(point) !== undefined;
    const options = this.focus_options_json();
    for (const [series, handle] of this.focus_handles) {
      const time = visible && series === active ? time_to_utc_seconds(point.time) : null;
      handle.set(time, options);
    }
  }

  private style_outline(): void {
    this.layer.style.outline = this.focused
      ? `${this.high_contrast ? 4 : 3}px solid ${this.controller.options.focus_indicator_color}`
      : "none";
  }

  private render_shortcuts(): void {
    const options = this.controller.options;
    this.hint.textContent = options.messages.shortcuts_hint;
    this.panel.textContent = "";
    const title = document.createElement("div");
    title.textContent = options.messages.shortcuts_title;
    title.style.cssText = "font-weight:600;margin-bottom:6px";
    this.panel.appendChild(title);
    const rows: [string, string][] = [
      ["← / →", "Move between data points"],
      ["Page Up / Page Down", `Jump ${options.page_step} points`],
      ["Home / End", "First / last point"],
      ["+ / −", "Zoom in / out"],
      ["H", "Show or hide this panel"],
    ];
    if (this.series.length > 1) rows.splice(1, 0, ["↑ / ↓", "Switch between series"]);
    for (const [keys, action] of rows) {
      const row = document.createElement("div");
      row.style.cssText = "display:flex;gap:10px;align-items:baseline;margin-top:3px";
      const key = document.createElement("kbd");
      key.textContent = keys;
      key.style.cssText = "flex:0 0 auto;border:1px solid currentColor;border-radius:3px;padding:0 5px;font-family:monospace;white-space:nowrap";
      const text = document.createElement("span");
      text.textContent = action;
      row.append(key, text);
      this.panel.appendChild(row);
    }
  }

  private style_shortcuts(): void {
    const base = "position:absolute;z-index:6;color:#fff;font-size:0.8125rem;line-height:1.45;pointer-events:none;";
    const surface = this.high_contrast
      ? "background:#000;border:2px solid #fff;"
      : "background:rgba(20,24,28,0.9);border:1px solid rgba(255,255,255,0.25);";
    this.hint.style.cssText = `${base}${surface}left:8px;bottom:8px;padding:3px 8px;border-radius:4px;white-space:nowrap;${this.controller.options.show_shortcuts && this.focused && !this.shortcuts_open ? "" : "display:none"}`;
    this.panel.style.cssText = `${base}${surface}left:8px;top:8px;max-width:calc(100% - 16px);padding:8px 11px;border-radius:6px;${this.high_contrast ? "" : "box-shadow:0 2px 10px rgba(0,0,0,0.45);"}${this.controller.options.show_shortcuts && this.shortcuts_open ? "" : "display:none"}`;
  }
}

class AccessibilityController implements accessibility_handle {
  readonly host: HTMLElement;
  options: resolved_accessibility_options;
  private raw_options: accessibility_options;
  private panes: PaneAccessibility[] = [];
  private active: PaneAccessibility | null = null;
  private readonly status = document.createElement("div");
  private readonly status_writer = new LiveWriter(() => this.status);
  private dirty = new Set<PaneAccessibility>();
  private update_timer: ReturnType<typeof setTimeout> | null = null;
  private description_id = 0;
  private detached = false;
  private refresh_queued = false;
  private readonly hidden_canvases = new Map<HTMLCanvasElement, string | null>();
  private readonly initial_canvas_states: (string | null)[];
  private readonly neutralised = new Map<HTMLElement, [string | null, string | null]>();
  private readonly observer: MutationObserver;
  private readonly resize_observer: ResizeObserver;

  constructor(readonly chart: chart_api, options: accessibility_options) {
    this.raw_options = { ...options };
    this.options = resolve_options(this.raw_options);
    this.host = chart.chart_element();
    this.initial_canvas_states = [...this.host.querySelectorAll("canvas")]
      .map((canvas) => canvas.getAttribute("aria-hidden"));
    this.status.className = "nucleuscharts-a11y-shared-status-region";
    this.status.setAttribute("aria-live", "polite");
    this.status.setAttribute("aria-atomic", "true");
    this.status.style.cssText = HIDDEN;
    this.host.appendChild(this.status);
    this.apply_status_lang();
    this.sweep_chart_dom();
    this.observer = new MutationObserver(() => this.sweep_chart_dom());
    this.observer.observe(this.host, { childList: true, subtree: true });
    this.resize_observer = new ResizeObserver(() => this.update_geometry());
    this.resize_observer.observe(this.host);
    chart.time_scale().subscribe_visible_logical_range_change(this.on_view_change);
    chart.time_scale().subscribe_size_change(this.on_size_change);
    chart.subscribe_series_added(this.on_series_change);
    chart.subscribe_series_removed(this.on_series_change);
    this.refresh();
  }

  next_id(): number {
    return ++this.description_id;
  }

  localization(): localization_options {
    return (this.chart.options() as { localization?: localization_options }).localization ?? {};
  }

  locale(): string | undefined {
    return this.localization().locale || undefined;
  }

  set_active(pane: PaneAccessibility): void {
    this.active = pane;
  }

  announces(pane_index: number): boolean {
    const mode = this.options.announce_data_updates;
    if (mode === false) return false;
    return typeof mode === "function" ? mode(pane_index) : true;
  }

  mark_dirty(pane: PaneAccessibility): void {
    this.dirty.add(pane);
    if (this.update_timer !== null) return;
    this.update_timer = setTimeout(() => {
      this.update_timer = null;
      this.flush_updates();
    }, UPDATE_DEBOUNCE_MS);
  }

  focus(pane_index = 0): void {
    this.panes[pane_index]?.focus();
  }

  refresh(): void {
    for (const pane of this.panes) pane.detach();
    this.panes = this.chart.panes().map((pane, index) => new PaneAccessibility(this, pane, index));
    this.active = this.panes[0] ?? null;
    this.update_geometry();
  }

  apply_options(options: accessibility_options): void {
    this.raw_options = { ...this.raw_options, ...options };
    this.options = resolve_options(this.raw_options);
    this.apply_status_lang();
    for (const pane of this.panes) pane.apply_options();
  }

  detach(): void {
    if (this.detached) return;
    this.detached = true;
    if (this.update_timer !== null) clearTimeout(this.update_timer);
    this.update_timer = null;
    this.status_writer.dispose();
    this.observer.disconnect();
    this.resize_observer.disconnect();
    this.chart.time_scale().unsubscribe_visible_logical_range_change(this.on_view_change);
    this.chart.time_scale().unsubscribe_size_change(this.on_size_change);
    this.chart.unsubscribe_series_added(this.on_series_change);
    this.chart.unsubscribe_series_removed(this.on_series_change);
    for (const pane of this.panes) pane.detach();
    this.panes = [];
    this.status.remove();
    for (const [canvas, previous] of this.hidden_canvases) {
      if (previous === null) canvas.removeAttribute("aria-hidden");
      else canvas.setAttribute("aria-hidden", previous);
      canvas.removeAttribute(CANVAS_PREVIOUS_ARIA);
    }
    // A backend canvas can be recreated while native focus primitives detach. Such a node did not
    // exist before this controller, so it has no host state to restore and must leave unhidden.
    for (const canvas of this.host.querySelectorAll("canvas")) {
      if (!this.hidden_canvases.has(canvas)) {
        const inherited = canvas.getAttribute(CANVAS_PREVIOUS_ARIA);
        if (inherited === null || inherited === "__none__") canvas.removeAttribute("aria-hidden");
        else canvas.setAttribute("aria-hidden", inherited);
        canvas.removeAttribute(CANVAS_PREVIOUS_ARIA);
      }
    }
    [...this.host.querySelectorAll("canvas")].forEach((canvas, index) => {
      const previous = this.initial_canvas_states[index] ?? null;
      if (previous === null) canvas.removeAttribute("aria-hidden");
      else canvas.setAttribute("aria-hidden", previous);
      canvas.removeAttribute(CANVAS_PREVIOUS_ARIA);
    });
    for (const [element, [tab, hidden]] of this.neutralised) {
      if (tab === null) element.removeAttribute("tabindex");
      else element.setAttribute("tabindex", tab);
      if (hidden === null) element.removeAttribute("aria-hidden");
      else element.setAttribute("aria-hidden", hidden);
    }
    this.hidden_canvases.clear();
    this.neutralised.clear();
    this.dirty.clear();
  }

  private readonly on_view_change = (): void => this.update_geometry();
  private readonly on_size_change = (): void => this.update_geometry();
  private readonly on_series_change = (): void => {
    if (this.refresh_queued) return;
    this.refresh_queued = true;
    queueMicrotask(() => {
      this.refresh_queued = false;
      if (!this.detached) this.refresh();
    });
  };

  private update_geometry(): void {
    for (const pane of this.panes) {
      pane.sync_series();
      pane.update_geometry();
    }
  }

  private flush_updates(): void {
    const mode = this.options.announce_data_updates;
    const summaries: string[] = [];
    const active = this.active ?? this.panes[0];
    for (const pane of this.panes) {
      if (!this.dirty.has(pane)) continue;
      const pending = pane.take_update_summaries();
      if (mode !== "active" || pane === active) summaries.push(...pending);
    }
    this.dirty.clear();
    if (summaries.length === 0) return;
    this.status_writer.write(this.options.messages.data_updated(summaries, summaries.length, UPDATE_MAX_SERIES));
  }

  private apply_status_lang(): void {
    const lang = this.options.lang ?? this.locale();
    if (lang === undefined || lang.length === 0) this.status.removeAttribute("lang");
    else this.status.lang = lang;
  }

  private sweep_chart_dom(): void {
    for (const canvas of this.host.querySelectorAll("canvas")) {
      if (!this.hidden_canvases.has(canvas)) {
        const inherited = canvas.getAttribute(CANVAS_PREVIOUS_ARIA);
        const previous = inherited === "__none__" ? null : inherited ?? canvas.getAttribute("aria-hidden");
        this.hidden_canvases.set(canvas, previous);
        canvas.setAttribute(CANVAS_PREVIOUS_ARIA, previous ?? "__none__");
      }
      canvas.setAttribute("aria-hidden", "true");
    }
    const selector = "a[href],button,input,select,textarea,iframe,[tabindex],[contenteditable=true],audio[controls],video[controls]";
    for (const element of this.host.querySelectorAll<HTMLElement>(selector)) {
      if (element.closest(".nucleuscharts-a11y-layer") !== null) continue;
      if (this.neutralised.has(element)) continue;
      const previous_hidden = element instanceof HTMLCanvasElement
        ? this.hidden_canvases.get(element) ?? null
        : element.getAttribute("aria-hidden");
      this.neutralised.set(element, [element.getAttribute("tabindex"), previous_hidden]);
      element.setAttribute("tabindex", "-1");
      element.setAttribute("aria-hidden", "true");
    }
  }
}

/**
 * Attach one official-style active-point-only semantic layer per pane. Financial coordinates and
 * the visible point focus ring remain in the shared Rust engine; this controller owns only DOM,
 * keyboard, localisation, and live-region behavior that cannot exist outside the browser host.
 */
export function enable_accessibility(
  chart: chart_api,
  options: accessibility_options = {},
): accessibility_handle {
  return new AccessibilityController(chart, options);
}
