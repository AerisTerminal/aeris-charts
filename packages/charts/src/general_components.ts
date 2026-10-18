/** Browser presentation for engine-owned general chart components. */
import { chart_impl } from "./impl.js";
import type {
  chart_api,
  general_axis_api,
  general_brush_snapshot,
  general_series_api,
  layout_series_api,
  general_tooltip_snapshot,
  mouse_event_params,
  pane_api,
} from "./types.js";
import type { popover_palette } from "./theme.js";

export interface general_legend_options {
  pane?: number | pane_api;
  format_title?: (title: string, series: general_series_api) => string;
  format_layout_title?: (title: string, series: layout_series_api) => string;
  label?: string;
  lang?: string;
  format_action?: (visible: boolean, title: string) => string;
}

export interface general_brush_controls_options {
  axis: string | general_axis_api;
  label?: string;
  locale?: string;
  labels?: Partial<{
    select_all: string;
    start_lower: string;
    start_higher: string;
    end_lower: string;
    end_higher: string;
    clear: string;
    no_range: string;
  }>;
}

/** Keyboard and touch controls for the engine's semantic brush and pointer gesture. */
export function create_general_brush_controls(
  chart: chart_api,
  container: HTMLElement,
  options: general_brush_controls_options,
): general_component_handle {
  const axis_id = typeof options.axis === "string" ? options.axis : options.axis.id;
  chart.set_general_brush_input_axis(axis_id);
  const root = document.createElement("div");
  root.className = "aeris_charts-general-component";
  root.setAttribute("role", "group");
  root.setAttribute("aria-label", options.label ?? "Chart range brush");
  if (options.locale !== undefined) root.lang = options.locale;
  root.style.cssText = "display:flex;flex-wrap:wrap;align-items:center;gap:4px";
  const status = document.createElement("output");
  status.setAttribute("aria-live", "polite");
  status.style.cssText = "min-width:8ch;max-width:100%;padding:0 4px;overflow-wrap:anywhere";
  const button = (label: string, action: () => void): HTMLButtonElement => {
    const element = document.createElement("button");
    element.type = "button";
    element.textContent = label;
    element.style.cssText = "min-height:28px;padding:2px 6px;border:1px solid currentColor;border-radius:4px;background:transparent;color:inherit;cursor:pointer";
    element.addEventListener("click", action);
    root.appendChild(element);
    return element;
  };
  const labels = {
    select_all: "Select all", start_lower: "Start −", start_higher: "Start +",
    end_lower: "End −", end_higher: "End +", clear: "Clear", no_range: "No range selected",
    ...options.labels,
  };
  const select = button(labels.select_all, () => chart.select_full_general_brush(axis_id));
  const start_lower = button(labels.start_lower, () => chart.nudge_general_brush("start", -1));
  const start_higher = button(labels.start_higher, () => chart.nudge_general_brush("start", 1));
  const end_lower = button(labels.end_lower, () => chart.nudge_general_brush("end", -1));
  const end_higher = button(labels.end_higher, () => chart.nudge_general_brush("end", 1));
  const clear = button(labels.clear, () => chart.clear_general_brush(axis_id));
  root.appendChild(status);
  container.appendChild(root);
  let detached = false;
  const range_text = (snapshot: general_brush_snapshot): string => {
    const { range } = snapshot;
    if (range.type === "category") return `${range.from} – ${range.to}`;
    if (range.type === "temporal") {
      const format = (value: number): string => {
        const date = new Date(value);
        return Number.isNaN(date.valueOf()) ? String(value) : date.toLocaleString(options.locale);
      };
      return `${format(range.from)} – ${format(range.to)}`;
    }
    return `${range.from} – ${range.to}`;
  };
  const refresh = (): void => {
    if (detached) return;
    const snapshot = chart.general_brush_snapshot();
    const active = snapshot !== null && snapshot.axis_id === axis_id;
    status.textContent = active ? range_text(snapshot) : labels.no_range;
    for (const control of [start_lower, start_higher, end_lower, end_higher, clear]) {
      control.disabled = !active;
    }
    select.disabled = chart.axis(axis_id) === null;
  };
  chart.subscribe_general_brush_change(refresh);
  refresh();
  return {
    refresh,
    detach(): void {
      if (detached) return;
      detached = true;
      chart.unsubscribe_general_brush_change(refresh);
      chart.set_general_brush_input_axis();
      root.remove();
    },
  };
}

export interface general_component_handle {
  /** Re-read engine state after external style, visibility, data, or view changes. */
  refresh(): void;
  detach(): void;
}

function pane_index(pane: number | pane_api | undefined): number | undefined {
  return typeof pane === "number" ? pane : pane?.pane_index();
}

/** Accessible default legend. Its buttons toggle engine visibility by stable series identity. */
export function create_general_legend(
  chart: chart_api,
  container: HTMLElement,
  options: general_legend_options = {},
): general_component_handle {
  const root = document.createElement("div");
  root.className = "aeris_charts-general-component";
  root.setAttribute("role", "group");
  root.setAttribute("aria-label", options.label ?? "Chart legend");
  if (options.lang !== undefined) root.lang = options.lang;
  root.style.cssText = "display:flex;flex-wrap:wrap;gap:4px 12px;align-items:center";
  container.appendChild(root);
  let detached = false;

  const refresh = (): void => {
    if (detached) return;
    const focused_series = root.contains(document.activeElement)
      ? (document.activeElement as HTMLElement).dataset.series
      : undefined;
    const pane = pane_index(options.pane);
    const series = new Map(chart.general_series_order(pane).map((item) => [item.id, item]));
    const snapshot = chart.general_legend_snapshot(pane);
    root.replaceChildren();
    for (const item of snapshot.items) {
      const handle = series.get(item.series);
      if (handle === undefined) continue;
      const group = document.createElement("div");
      group.style.cssText = "display:inline-flex;flex-direction:column;align-items:flex-start;gap:4px";
      const button = document.createElement("button");
      button.type = "button";
      button.dataset.series = String(item.series);
      button.setAttribute("aria-pressed", String(item.visible));
      button.setAttribute("aria-label", options.format_action?.(item.visible, item.title)
        ?? `${item.visible ? "Hide" : "Show"} ${item.title}`);
      button.style.cssText = "display:inline-flex;align-items:center;gap:6px;min-height:20px;padding:0 2px;border:0;border-radius:2px;background:transparent;color:inherit;font:inherit;line-height:16px;cursor:pointer";
      button.style.opacity = item.visible ? "1" : "0.55";
      const swatch = document.createElement("span");
      swatch.setAttribute("aria-hidden", "true");
      swatch.style.cssText = "display:inline-block;flex:none;width:4px;height:12px;border-radius:2px;background:currentColor";
      if (item.color !== null) swatch.style.color = item.color;
      const label = document.createElement("span");
      label.textContent = options.format_title?.(item.title, handle) ?? item.title;
      button.append(swatch, label);
      button.addEventListener("click", () => {
        handle.set_visible(!handle.options().visible);
        refresh();
      });
      group.appendChild(button);
      if (item.slices.length > 0 || item.slices_truncated) {
        const slices = document.createElement("div");
        slices.setAttribute("role", "list");
        slices.setAttribute("aria-label", `${item.title || item.kind} slices`);
        slices.style.cssText = "display:flex;flex-wrap:wrap;gap:4px 8px;padding:0 4px;max-height:120px;max-width:100%;overflow:auto;opacity:" + (item.visible ? "1" : "0.55");
        for (const slice of item.slices) {
          const entry = document.createElement("span");
          entry.setAttribute("role", "listitem");
          entry.style.cssText = "display:inline-flex;align-items:center;gap:4px;white-space:nowrap";
          const marker = document.createElement("span");
          marker.setAttribute("aria-hidden", "true");
          marker.style.cssText = `display:inline-block;flex:none;width:4px;height:12px;border-radius:2px;background:${slice.color}`;
          const name = document.createElement("span");
          name.textContent = `${slice.label}: ${slice.value}`;
          entry.append(marker, name);
          slices.appendChild(entry);
        }
        if (item.slices_truncated) {
          const more = document.createElement("span");
          more.textContent = "More slices in chart data";
          slices.appendChild(more);
        }
        group.appendChild(slices);
      }
      root.appendChild(group);
    }
    for (const handle of chart.layout_series(pane)) {
      const item = handle.options();
      const title = item.title && item.title.length > 0 ? item.title : handle.kind;
      const visible = item.visible !== false;
      const button = document.createElement("button");
      button.type = "button";
      button.dataset.series = `layout:${handle.id}`;
      button.setAttribute("aria-pressed", String(visible));
      button.setAttribute("aria-label", options.format_action?.(visible, title)
        ?? `${visible ? "Hide" : "Show"} ${title}`);
      button.style.cssText = "display:inline-flex;align-items:center;gap:6px;min-height:20px;padding:0 2px;border:0;border-radius:2px;background:transparent;color:inherit;font:inherit;line-height:16px;cursor:pointer";
      button.style.opacity = visible ? "1" : "0.55";
      const swatch = document.createElement("span");
      swatch.setAttribute("aria-hidden", "true");
      swatch.style.cssText = `display:inline-block;flex:none;width:4px;height:12px;border-radius:2px;background:${item.color ?? "#4682b4"}`;
      const label = document.createElement("span");
      label.textContent = options.format_layout_title?.(title, handle) ?? title;
      button.append(swatch, label);
      button.addEventListener("click", () => { handle.set_visible(!handle.options().visible); refresh(); });
      root.appendChild(button);
    }
    if (focused_series !== undefined) {
      for (const button of root.querySelectorAll<HTMLButtonElement>("button")) {
        if (button.dataset.series === focused_series) {
          button.focus();
          break;
        }
      }
    }
  };
  chart.subscribe_series_added(refresh);
  chart.subscribe_series_removed(refresh);
  chart.subscribe_general_legend_change(refresh);
  refresh();
  return {
    refresh,
    detach(): void {
      if (detached) return;
      detached = true;
      chart.unsubscribe_series_added(refresh);
      chart.unsubscribe_series_removed(refresh);
      chart.unsubscribe_general_legend_change(refresh);
      root.remove();
    },
  };
}

export interface general_tooltip_options {
  mode?: "item" | "shared";
  /** Restrict pointer tooltips to one pane; omitted uses every general or layout pane. */
  pane?: number | pane_api;
  format_value?: (value: number) => string;
  format_detail?: (item: general_tooltip_snapshot) => string | undefined;
  lang?: string;
}

function tooltip_detail(item: general_tooltip_snapshot, format: (value: number) => string): string {
  const parts: string[] = [];
  if (item.value !== null) parts.push(format(item.value));
  if (item.low !== null || item.high !== null) {
    parts.push(`${item.low === null ? "—" : format(item.low)}–${item.high === null ? "—" : format(item.high)}`);
  }
  if (item.q1 !== null || item.q3 !== null) {
    parts.push(`Q1 ${item.q1 === null ? "—" : format(item.q1)} · Q3 ${item.q3 === null ? "—" : format(item.q3)}`);
  }
  if (item.size !== null) parts.push(`size ${format(item.size)}`);
  if (item.y_label !== null) parts.push(item.y_label);
  return parts.join(" · ") || item.label || "No value";
}

/** Default pointer tooltip driven by engine hits and semantic item/shared snapshots. */
export function create_general_tooltip(
  chart: chart_api,
  chart_container: HTMLElement,
  options: general_tooltip_options = {},
): general_component_handle {
  if (!(chart instanceof chart_impl)) {
    throw new Error("engine-owned general tooltips require a Aeris chart instance");
  }
  const element = document.createElement("div");
  element.className = "aeris_charts-tooltip";
  element.setAttribute("role", "tooltip");
  if (options.lang !== undefined) element.lang = options.lang;
  // The financial tooltip's layout inline, so the look holds without the optional stylesheet.
  // The 1px highlight is an inset shadow because rows carry no background.
  element.style.cssText = "position:absolute;z-index:5;display:none;pointer-events:none;box-sizing:border-box;min-width:min(156px,100%);max-width:min(280px,100%);max-height:100%;overflow:hidden;overflow-wrap:anywhere;border:1px solid var(--nc-tooltip-border);border-radius:var(--radius-default,4px);background:var(--aeris-tooltip-background,var(--nc-tooltip-surface));color:var(--aeris-tooltip-color,var(--nc-tooltip-foreground));font-size:12px;line-height:16px;font-variant-numeric:tabular-nums;box-shadow:inset 0 0 0 1px var(--nc-tooltip-highlight)";
  // The engine's own tooltip would duplicate this one until the last DOM tooltip detaches.
  chart.retain_dom_general_tooltip(true);
  // Resolved on every show so any theme change, including `reset_style_to_defaults`, is followed.
  // The palettes are shared constants, so identity detects a theme switch.
  let popover: popover_palette | null = null;
  const apply_popover = (): void => {
    const next = chart.popover_palette();
    if (next === popover) return;
    popover = next;
    element.style.setProperty("--nc-tooltip-surface", next.surface);
    element.style.setProperty("--nc-tooltip-border", next.border);
    element.style.setProperty("--nc-tooltip-highlight", next.highlight);
    element.style.setProperty("--nc-tooltip-foreground", next.foreground);
    element.style.setProperty("--nc-tooltip-muted", next.muted_foreground);
  };
  const original_position = chart_container.style.position;
  if (getComputedStyle(chart_container).position === "static") chart_container.style.position = "relative";
  chart_container.appendChild(element);
  let last_event: mouse_event_params | null = null;
  let pending_frame = 0;
  let detached = false;
  const format = options.format_value ?? ((value: number) => String(value));

  const refresh = (): void => {
    if (detached) return;
    const event = last_event;
    const hit = event?.general_hit;
    const layout = event?.layout_hit;
    if ((hit == null && layout == null) || event?.point == null ||
      (options.pane !== undefined && event.pane_index !== pane_index(options.pane))) {
      element.style.display = "none";
      return;
    }
    const header = document.createElement("div");
    header.className = "aeris_charts-tooltip__header";
    header.style.cssText = "padding:7px 10px 0;background:none;border-bottom:0";
    const heading = document.createElement("div");
    heading.className = "aeris_charts-tooltip__timestamp";
    heading.style.cssText = "font-weight:590;white-space:nowrap;overflow:hidden;text-overflow:ellipsis";
    header.appendChild(heading);
    const body = document.createElement("div");
    body.className = "aeris_charts-tooltip__body";
    body.style.cssText = "display:grid;gap:2px;padding:4px 10px 7px;background:none";
    const add_row = (color: string | null, title: string, detail: string): void => {
      const row = document.createElement("div");
      row.className = "aeris_charts-tooltip__row";
      row.style.cssText = "display:flex;gap:12px;align-items:baseline;justify-content:space-between";
      const label = document.createElement("span");
      label.className = "aeris_charts-tooltip__label";
      label.style.cssText = "display:flex;align-items:center;gap:6px;min-width:0;color:var(--nc-tooltip-muted)";
      if (color !== null) {
        const dot = document.createElement("span");
        dot.style.cssText = `flex:none;align-self:center;width:4px;height:10px;border-radius:2px;background:${color}`;
        label.appendChild(dot);
      }
      label.appendChild(document.createTextNode(title));
      // Assistive technology reads "title: value" although the columns show no separator.
      const separator = document.createElement("span");
      separator.textContent = ": ";
      separator.style.cssText = "position:absolute;width:1px;height:1px;overflow:hidden;clip-path:inset(50%);white-space:nowrap";
      label.appendChild(separator);
      const value = document.createElement("span");
      value.className = "aeris_charts-tooltip__value";
      value.style.cssText = "text-align:right;white-space:nowrap";
      value.textContent = detail;
      row.append(label, value);
      body.appendChild(row);
    };
    if (layout != null) {
      heading.textContent = layout.label;
      add_row(null, layout.title, layout.value == null ? "No value" : format(layout.value));
    } else if (hit != null) {
      const items = options.mode === "item"
        ? [chart.general_series_order().find((series) => series.id === hit.series)?.data_at(hit.row)].filter(
            (item): item is general_tooltip_snapshot => item != null,
          )
        : chart.general_shared_tooltip(hit.series, hit.row)?.items;
      if (items == null || items.length === 0) {
        element.style.display = "none";
        return;
      }
      // Shared rows describe one X datum; an item tooltip can name the row by its own label.
      heading.textContent = (options.mode === "item" ? items[0]?.label : null) ?? items[0]?.x_label ?? "";
      for (const item of items) {
        add_row(item.color, item.title, options.format_detail?.(item) ?? tooltip_detail(item, format));
      }
    }
    element.replaceChildren(header, body);
    apply_popover();
    element.style.display = "block";
    const width = chart_container.clientWidth;
    const height = chart_container.clientHeight;
    element.style.left = `${Math.max(0, Math.min(event.point.x + 12, width - element.offsetWidth - 4))}px`;
    element.style.top = `${Math.max(0, Math.min(event.point.y + 12, height - element.offsetHeight - 4))}px`;
  };
  const on_crosshair = (event: mouse_event_params): void => {
    last_event = event;
    if (event.point === null) {
      if (pending_frame !== 0) cancelAnimationFrame(pending_frame);
      pending_frame = 0;
      refresh();
    } else if (pending_frame === 0) {
      pending_frame = requestAnimationFrame(() => {
        pending_frame = 0;
        refresh();
      });
    }
  };
  chart.subscribe_crosshair_move(on_crosshair);
  return {
    refresh,
    detach(): void {
      if (detached) return;
      detached = true;
      if (pending_frame !== 0) cancelAnimationFrame(pending_frame);
      chart.unsubscribe_crosshair_move(on_crosshair);
      element.remove();
      chart_container.style.position = original_position;
      chart.retain_dom_general_tooltip(false);
    },
  };
}
