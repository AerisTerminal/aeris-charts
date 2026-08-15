/**
 * First-class custom-series features built on Nucleus's backend-neutral custom-series contract.
 * These mirror the useful series types from Lightweight Charts' plugin examples without making
 * applications own renderer callbacks or a second scene graph.
 */

import type { custom_series_item, custom_series_pane_view, custom_series_render_context } from "./custom_series.js";
import { time_to_utc_seconds } from "./impl.js";
import type { time } from "./types.js";

const BLUE = "#2962ff";
const UP = "#089981";
const DOWN = "#f23645";
const PALETTE = [BLUE, "#e1575a", "#f28e2c", "#a459d1", "#1b9c85"] as const;

function color_at(colors: readonly string[], index: number): string {
  return colors[index % colors.length] ?? BLUE;
}

function column_width(ctx: custom_series_render_context, fraction = 0.8): number {
  return Math.max(1, Math.floor(ctx.bar_spacing * fraction * ctx.dpr));
}

function box(a: number, b: number): { start: number; length: number } {
  const start = Math.floor(Math.min(a, b));
  return { start, length: Math.max(1, Math.ceil(Math.max(a, b)) - start) };
}

function finite(values: readonly unknown[]): number[] {
  return values.filter((value): value is number => typeof value === "number" && Number.isFinite(value));
}

function y_for(ctx: custom_series_render_context, price: number): number | null {
  return Number.isFinite(price) ? ctx.price_to_y(price) : null;
}

function line_points(
  ctx: custom_series_render_context,
  value: (item: Record<string, unknown>) => number,
): number[] {
  const points: number[] = [];
  for (const { x, item } of ctx.items) {
    const y = y_for(ctx, value(item));
    if (y !== null) points.push(x, y);
  }
  return points;
}

function draw_band_segment(
  ctx: custom_series_render_context,
  ax: number,
  a_top: number,
  a_bottom: number,
  bx: number,
  b_top: number,
  b_bottom: number,
  color: string,
): void {
  ctx.triangle(ax, a_top, bx, b_top, bx, b_bottom, color);
  ctx.triangle(ax, a_top, bx, b_bottom, ax, a_bottom, color);
}

function draw_polyline(
  ctx: custom_series_render_context,
  points: readonly number[],
  color: string,
  width: number,
): void {
  if (points.length >= 4) ctx.polyline(points, color, width, 0);
}

export interface brushable_area_data extends custom_series_item {
  value: number;
}

export interface brushable_area_style {
  line_color: string;
  top_color: string;
  bottom_color: string;
  line_width: number;
}

export interface brushable_area_range {
  from_time: time;
  to_time: time;
  style: Partial<brushable_area_style>;
}

export interface brushable_area_options extends Partial<brushable_area_style> {
  base_price?: number;
  brush_ranges?: readonly brushable_area_range[];
}

/** Area series whose time ranges can carry independent line and fill styles. */
export function create_brushable_area_series(options: brushable_area_options = {}): custom_series_pane_view {
  const base: brushable_area_style = {
    line_color: options.line_color ?? BLUE,
    top_color: options.top_color ?? "#2962ff66",
    bottom_color: options.bottom_color ?? "#2962ff00",
    line_width: options.line_width ?? 2,
  };
  const ranges = (options.brush_ranges ?? []).map((entry) => ({
    from: time_to_utc_seconds(entry.from_time),
    to: time_to_utc_seconds(entry.to_time),
    style: { ...base, ...entry.style },
  }));
  return {
    price_value_builder: (item: brushable_area_data) => [item.value],
    is_whitespace: (item: brushable_area_data) => !Number.isFinite(item.value),
    default_options: { color: base.line_color },
    render(ctx) {
      const base_y = y_for(ctx, options.base_price ?? 0) ?? ctx.pane_top + ctx.pane_height;
      for (let index = 1; index < ctx.items.length; index++) {
        const previous = ctx.items[index - 1];
        const current = ctx.items[index];
        if (previous === undefined || current === undefined) continue;
        const y1 = y_for(ctx, previous.item.value);
        const y2 = y_for(ctx, current.item.value);
        if (y1 === null || y2 === null) continue;
        const timestamp = time_to_utc_seconds(current.item.time);
        const style = ranges.find((range) => timestamp >= Math.min(range.from, range.to) && timestamp <= Math.max(range.from, range.to))?.style ?? base;
        const points = [previous.x, y1, current.x, y2];
        ctx.area_fill(points, base_y, style.top_color, style.bottom_color);
        ctx.polyline(points, style.line_color, style.line_width * ctx.dpr, 0);
      }
    },
  };
}

export interface dual_range_histogram_data extends custom_series_item {
  /** Positive and negative histogram ranges, colored by array position. */
  values: readonly number[];
}

export interface dual_range_histogram_options {
  colors?: readonly string[];
  border_radius?: readonly number[];
  max_height?: number;
}

/** Symmetric four-band histogram whose height is independent of the price scale. */
export function create_dual_range_histogram_series(options: dual_range_histogram_options = {}): custom_series_pane_view {
  const colors = options.colors?.length ? options.colors : ["#ace5dc", "#42bda8", "#fccacd", "#f77c80"];
  const radii = options.border_radius?.length ? options.border_radius : [2, 0, 2, 0];
  const max_height = Math.max(1, options.max_height ?? 130);
  return {
    price_value_builder: () => [0],
    is_whitespace: (item: dual_range_histogram_data) => finite(item.values).length === 0,
    default_options: { last_value_visible: false, price_line_visible: false },
    render(ctx) {
      const width = column_width(ctx);
      const left = (x: number) => Math.round(x - width / 2);
      const zero = y_for(ctx, 0) ?? ctx.pane_top + ctx.pane_height / 2;
      const largest = Math.max(1, ...ctx.items.flatMap(({ item }) => finite(item.values)).map(Math.abs));
      for (const { x, item } of ctx.items) {
        const values = finite(item.values);
        values.forEach((value, index) => {
          const height = (Math.abs(value) / largest) * Math.min(max_height * ctx.dpr, ctx.pane_height / 2);
          ctx.round_rect(left(x), value >= 0 ? zero - height : zero, width, height, (radii[index % radii.length] ?? 0) * ctx.dpr, color_at(colors, index));
        });
      }
    },
  };
}

export interface grouped_bars_data extends custom_series_item {
  values: readonly number[];
}

export interface grouped_bars_options {
  colors?: readonly string[];
  base_price?: number;
}

/** Multiple side-by-side columns per time point. */
export function create_grouped_bars_series(options: grouped_bars_options = {}): custom_series_pane_view {
  const colors = options.colors?.length ? options.colors : PALETTE;
  const base_price = options.base_price ?? 0;
  return {
    price_value_builder: (item: grouped_bars_data) => [base_price, ...finite(item.values)],
    is_whitespace: (item: grouped_bars_data) => finite(item.values).length === 0,
    render(ctx) {
      const base_y = y_for(ctx, base_price);
      if (base_y === null) return;
      for (const { x, item } of ctx.items) {
        const values = finite(item.values);
        if (values.length === 0) continue;
        const width = Math.max(1, Math.floor(ctx.bar_spacing * ctx.dpr / (values.length + 1)));
        const start = Math.round(x - (width * values.length) / 2);
        values.forEach((value, index) => {
          const y = y_for(ctx, value);
          if (y === null) return;
          const vertical = box(base_y, y);
          ctx.rect(start + index * width, vertical.start, width, vertical.length, color_at(colors, index));
        });
      }
    },
  };
}

export interface heatmap_cell {
  low: number;
  high: number;
  amount: number;
}

export interface heatmap_data extends custom_series_item {
  cells: readonly heatmap_cell[];
}

export interface heatmap_options {
  cell_shader?: (amount: number) => string;
  cell_border_width?: number;
  cell_border_color?: string;
}

/** Price/time heatmap cells with caller-defined amount shading. */
export function create_heatmap_series(options: heatmap_options = {}): custom_series_pane_view {
  const shade = options.cell_shader ?? ((amount: number) => {
    const value = Math.min(100, Math.max(0, amount));
    return `rgba(0, ${100 + value * 1.55}, ${value}, ${0.2 + value * 0.008})`;
  });
  const border = Math.max(0, options.cell_border_width ?? 1);
  const border_color = options.cell_border_color ?? "transparent";
  return {
    price_value_builder: (item: heatmap_data) => item.cells.flatMap((cell) => [cell.low, cell.high]),
    is_whitespace: (item: heatmap_data) => item.cells.length === 0,
    default_options: { last_value_visible: false, price_line_visible: false },
    render(ctx) {
      const width = column_width(ctx, 1);
      for (const { x, item } of ctx.items) {
        for (const cell of item.cells as readonly heatmap_cell[]) {
          const low = y_for(ctx, cell.low);
          const high = y_for(ctx, cell.high);
          if (low === null || high === null) continue;
          const vertical = box(low, high);
          const left = Math.round(x - width / 2);
          ctx.rect(left, vertical.start, width, vertical.length, shade(cell.amount));
          if (border > 0 && border_color !== "transparent") {
            ctx.rect_frame(left, vertical.start, width, vertical.length, border_color, border * ctx.dpr);
          }
        }
      }
    },
  };
}

export interface hlc_area_data extends custom_series_item {
  high: number;
  low: number;
  close: number;
}

export interface hlc_area_options {
  high_line_color?: string;
  low_line_color?: string;
  close_line_color?: string;
  area_top_color?: string;
  area_bottom_color?: string;
  high_line_width?: number;
  low_line_width?: number;
  close_line_width?: number;
}

/** High/low envelope with an independent close line. */
export function create_hlc_area_series(options: hlc_area_options = {}): custom_series_pane_view {
  const high_color = options.high_line_color ?? UP;
  const low_color = options.low_line_color ?? DOWN;
  const close_color = options.close_line_color ?? "#878993";
  return {
    price_value_builder: (item: hlc_area_data) => [item.high, item.low, item.close],
    is_whitespace: (item: hlc_area_data) => !Number.isFinite(item.close),
    default_options: { color: close_color },
    render(ctx) {
      const highs = line_points(ctx, (item) => item.high as number);
      const lows = line_points(ctx, (item) => item.low as number);
      const closes = line_points(ctx, (item) => item.close as number);
      for (let index = 2; index < Math.min(highs.length, lows.length); index += 2) {
        const a_mid = (highs[index - 1]! + lows[index - 1]!) / 2;
        const b_mid = (highs[index + 1]! + lows[index + 1]!) / 2;
        draw_band_segment(ctx, highs[index - 2]!, highs[index - 1]!, a_mid, highs[index]!, highs[index + 1]!, b_mid, options.area_top_color ?? "#08998133");
        draw_band_segment(ctx, highs[index - 2]!, a_mid, lows[index - 1]!, highs[index]!, b_mid, lows[index + 1]!, options.area_bottom_color ?? "#f2364533");
      }
      draw_polyline(ctx, highs, high_color, (options.high_line_width ?? 2) * ctx.dpr);
      draw_polyline(ctx, lows, low_color, (options.low_line_width ?? 2) * ctx.dpr);
      draw_polyline(ctx, closes, close_color, (options.close_line_width ?? 2) * ctx.dpr);
    },
  };
}

export interface pretty_histogram_data extends custom_series_item {
  value: number;
  color?: string;
}

export interface pretty_histogram_options {
  color?: string;
  width_percent?: number;
  radius?: number;
  base_price?: number;
}

/** Rounded histogram columns. */
export function create_pretty_histogram_series(options: pretty_histogram_options = {}): custom_series_pane_view {
  const base_price = options.base_price ?? 0;
  return {
    price_value_builder: (item: pretty_histogram_data) => [base_price, item.value],
    is_whitespace: (item: pretty_histogram_data) => !Number.isFinite(item.value),
    default_options: { color: options.color ?? "#d63864" },
    render(ctx) {
      const base_y = y_for(ctx, base_price);
      if (base_y === null) return;
      const width = column_width(ctx, Math.min(1, Math.max(0.01, (options.width_percent ?? 50) / 100)));
      for (const { x, item } of ctx.items) {
        const y = y_for(ctx, item.value);
        if (y === null) continue;
        const vertical = box(base_y, y);
        ctx.round_rect(Math.round(x - width / 2), vertical.start, width, vertical.length, (options.radius ?? 4) * ctx.dpr, item.color ?? options.color ?? "#d63864");
      }
    },
  };
}

export interface lollipop_data extends custom_series_item {
  value: number;
  color?: string;
}

export interface lollipop_options {
  color?: string;
  line_width?: number;
  radius?: number;
  base_price?: number;
}

/** Stem-and-disc lollipop series. */
export function create_lollipop_series(options: lollipop_options = {}): custom_series_pane_view {
  const base_price = options.base_price ?? 0;
  return {
    price_value_builder: (item: lollipop_data) => [base_price, item.value],
    is_whitespace: (item: lollipop_data) => !Number.isFinite(item.value),
    default_options: { color: options.color ?? BLUE },
    render(ctx) {
      const base_y = y_for(ctx, base_price);
      if (base_y === null) return;
      for (const { x, item } of ctx.items) {
        const y = y_for(ctx, item.value);
        if (y === null) continue;
        const color = item.color ?? options.color ?? BLUE;
        ctx.vline(x, base_y, y, color, (options.line_width ?? 2) * ctx.dpr, 0);
        ctx.circle(x, y, (options.radius ?? ctx.bar_spacing / 2) * ctx.dpr, color);
      }
    },
  };
}

export interface rounded_candle_data extends custom_series_item {
  open: number;
  high: number;
  low: number;
  close: number;
  rounded?: boolean;
}

export interface rounded_candle_options {
  up_color?: string;
  down_color?: string;
  wick_up_color?: string;
  wick_down_color?: string;
  wick_visible?: boolean;
  radius?: number | ((bar_spacing: number) => number);
}

/** Candles with rounded bodies and the reference example's previous-close direction rule. */
export function create_rounded_candle_series(options: rounded_candle_options = {}): custom_series_pane_view {
  return {
    price_value_builder: (item: rounded_candle_data) => [item.high, item.low, item.close],
    is_whitespace: (item: rounded_candle_data) => !Number.isFinite(item.close),
    default_options: { color: options.up_color ?? UP },
    render(ctx) {
      const width = column_width(ctx);
      const wick_width = Math.max(1, Math.floor(ctx.dpr));
      const radius_option = options.radius ?? ((spacing: number) => spacing < 4 ? 0 : spacing / 3);
      const radius = (typeof radius_option === "function" ? radius_option(ctx.bar_spacing) : radius_option) * ctx.dpr;
      let previous_close = Number.NEGATIVE_INFINITY;
      for (const { x, item } of ctx.items) {
        const high = y_for(ctx, item.high);
        const low = y_for(ctx, item.low);
        const open = y_for(ctx, item.open);
        const close = y_for(ctx, item.close);
        if (high === null || low === null || open === null || close === null) continue;
        const up = item.close >= previous_close;
        previous_close = item.close;
        const color = up ? (options.up_color ?? UP) : (options.down_color ?? DOWN);
        if (options.wick_visible !== false) {
          const wick = box(high, low);
          ctx.rect(Math.round(x - wick_width / 2), wick.start, wick_width, wick.length, up ? (options.wick_up_color ?? UP) : (options.wick_down_color ?? DOWN));
        }
        const body = box(open, close);
        const left = Math.round(x - width / 2);
        if (item.rounded === false || radius <= 0) ctx.rect(left, body.start, width, body.length, color);
        else ctx.round_rect(left, body.start, width, body.length, radius, color);
      }
    },
  };
}

export interface shaded_background_data extends custom_series_item {
  value: number;
}

export interface shaded_background_options {
  low_value?: number;
  high_value?: number;
  shade?: (normalized_value: number, value: number) => string;
}

/** Full-height per-bar background shading driven by a value. */
export function create_shaded_background_series(options: shaded_background_options = {}): custom_series_pane_view {
  const low = options.low_value ?? 0;
  const high = options.high_value ?? 100;
  const shade = options.shade ?? ((normalized: number) => `rgba(${Math.round(50 + normalized * 205)}, 50, ${Math.round(255 - normalized * 205)}, 0.8)`);
  return {
    price_value_builder: () => [],
    is_whitespace: (item: shaded_background_data) => !Number.isFinite(item.value),
    default_options: { last_value_visible: false, price_line_visible: false },
    render(ctx) {
      const width = column_width(ctx, 1);
      const span = high - low || 1;
      for (const { x, item } of ctx.items) {
        const normalized = Math.min(1, Math.max(0, (item.value - low) / span));
        ctx.rect(Math.round(x - width / 2), ctx.pane_top, width, ctx.pane_height, shade(normalized, item.value));
      }
    },
  };
}

export interface stacked_values_data extends custom_series_item {
  values: readonly number[];
}

export interface stacked_area_color {
  line: string;
  area: string;
}

export interface stacked_area_options {
  colors?: readonly stacked_area_color[];
  line_width?: number;
}

function cumulative(values: readonly number[]): number[] {
  let total = 0;
  return finite(values).map((value) => (total += value));
}

/** Cumulative stacked areas rendered as backend-neutral triangles and lines. */
export function create_stacked_area_series(options: stacked_area_options = {}): custom_series_pane_view {
  const colors = options.colors?.length ? options.colors : PALETTE.map((color) => ({ line: color, area: `${color}33` }));
  const area_colors = colors.map((entry) => entry.area);
  const line_colors = colors.map((entry) => entry.line);
  return {
    price_value_builder: (item: stacked_values_data) => [0, ...cumulative(item.values)],
    is_whitespace: (item: stacked_values_data) => finite(item.values).length === 0,
    render(ctx) {
      const stacks = ctx.items.map(({ x, item }) => ({ x, values: cumulative(item.values as readonly number[]) }));
      const zero = y_for(ctx, 0);
      if (zero === null) return;
      const lines: number[][] = [];
      for (let index = 1; index < stacks.length; index++) {
        const a = stacks[index - 1];
        const b = stacks[index];
        if (a === undefined || b === undefined) continue;
        const layers = Math.min(a.values.length, b.values.length);
        for (let layer = 0; layer < layers; layer++) {
          const a_top = y_for(ctx, a.values[layer]!);
          const b_top = y_for(ctx, b.values[layer]!);
          const a_bottom = layer === 0 ? zero : y_for(ctx, a.values[layer - 1]!);
          const b_bottom = layer === 0 ? zero : y_for(ctx, b.values[layer - 1]!);
          if (a_top === null || b_top === null || a_bottom === null || b_bottom === null) continue;
          draw_band_segment(ctx, a.x, a_top, a_bottom, b.x, b_top, b_bottom, color_at(area_colors, layer));
          (lines[layer] ??= []).push(a.x, a_top);
          if (index === stacks.length - 1) lines[layer]!.push(b.x, b_top);
        }
      }
      lines.forEach((points, index) => draw_polyline(ctx, points, color_at(line_colors, index), (options.line_width ?? 2) * ctx.dpr));
    },
  };
}

export interface stacked_bars_options {
  colors?: readonly string[];
}

/** Cumulative stacked columns. */
export function create_stacked_bars_series(options: stacked_bars_options = {}): custom_series_pane_view {
  const colors = options.colors?.length ? options.colors : PALETTE;
  return {
    price_value_builder: (item: stacked_values_data) => [0, ...cumulative(item.values)],
    is_whitespace: (item: stacked_values_data) => finite(item.values).length === 0,
    render(ctx) {
      const zero = y_for(ctx, 0);
      if (zero === null) return;
      const width = column_width(ctx);
      for (const { x, item } of ctx.items) {
        let previous = zero;
        cumulative(item.values as readonly number[]).forEach((value, index) => {
          const y = y_for(ctx, value);
          if (y === null) return;
          const vertical = box(previous, y);
          ctx.rect(Math.round(x - width / 2), vertical.start, width, vertical.length, color_at(colors, index));
          previous = y;
        });
      }
    },
  };
}

export interface whisker_box_data extends custom_series_item {
  quartiles: readonly [number, number, number, number, number];
  outliers?: readonly number[];
}

export interface whisker_box_options {
  whisker_color?: string;
  lower_quartile_fill?: string;
  upper_quartile_fill?: string;
  outlier_color?: string;
}

/** Five-number box-and-whisker series with optional outliers. */
export function create_whisker_box_series(options: whisker_box_options = {}): custom_series_pane_view {
  const whisker = options.whisker_color ?? "#6a1b9a";
  return {
    // Match the reference feature: outliers render but do not stretch the price scale.
    price_value_builder: (item: whisker_box_data) => [item.quartiles[4], item.quartiles[0], item.quartiles[2]],
    is_whitespace: (item: whisker_box_data) => item.quartiles?.length !== 5,
    default_options: { color: whisker },
    render(ctx) {
      const body_width = column_width(ctx);
      const cap_width = Math.max(body_width, Math.floor(ctx.bar_spacing * ctx.dpr));
      const stroke = Math.max(1, Math.floor(ctx.dpr));
      for (const { x, item } of ctx.items) {
        const ys = (item.quartiles as readonly number[]).map((value) => y_for(ctx, value));
        if (ys.some((value) => value === null)) continue;
        const [minimum, q1, median, q3, maximum] = ys as number[];
        ctx.vline(x, maximum!, q3!, whisker, stroke, 0);
        ctx.vline(x, q1!, minimum!, whisker, stroke, 0);
        ctx.hline(maximum!, x - cap_width / 2, x + cap_width / 2, whisker, stroke, 0);
        ctx.hline(minimum!, x - cap_width / 2, x + cap_width / 2, whisker, stroke, 0);
        const upper = box(q3!, median!);
        const lower = box(median!, q1!);
        const left = Math.round(x - body_width / 2);
        ctx.rect(left, upper.start, body_width, upper.length, options.upper_quartile_fill ?? "#e91e63");
        ctx.rect(left, lower.start, body_width, lower.length, options.lower_quartile_fill ?? "#673ab7");
        ctx.hline(median!, x - cap_width / 2, x + cap_width / 2, whisker, stroke, 0);
        for (const outlier of item.outliers ?? []) {
          const y = y_for(ctx, outlier);
          if (y !== null && body_width > 2) ctx.circle(x, y, Math.min(body_width / 2, 4 * ctx.dpr), options.outlier_color ?? "#9598a1");
        }
      }
    },
  };
}
