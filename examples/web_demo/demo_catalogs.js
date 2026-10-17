import {
  create_bands_indicator,
  create_delta_tooltip,
  create_highlight_bar_crosshair,
  create_overlay_price_scale,
  create_session_highlighting,
  create_tooltip,
  create_trend_line,
  create_vertical_line,
  default_theme_name,
  enable_brushable_area_interaction,
  theme_palette,
} from "./dist/aeris_charts_financial.js";
import { hydrate_icons } from "./demo_icons.js";

const PRIMARY_BLUE = theme_palette(default_theme_name).primary;

function rainbow_color(value) {
  const t = Math.max(0, Math.min(1, value));
  const stops = [[48, 24, 110], [32, 150, 210], [42, 220, 120], [250, 220, 45], [190, 30, 45]];
  const scaled = t * (stops.length - 1);
  const index = Math.min(stops.length - 2, Math.floor(scaled));
  const mix = scaled - index;
  const color = stops[index].map((channel, channel_index) => Math.round(
    channel + (stops[index + 1][channel_index] - channel) * mix,
  ));
  return `rgb(${color.join(",")})`;
}

function add_line_companion(chart, data, options = {}, kind = "line") {
  const line = chart.add_series(kind, {
    color: "#f4f7fb",
    line_width: 2,
    price_line_visible: false,
    last_value_visible: true,
    countdown_visible: false,
    ...options,
  });
  line.set_data(data);
  return () => chart.remove_series(line);
}

function use_official_feature_spacing(chart) {
  const scale = chart.time_scale();
  const previous = scale.options();
  scale.apply_options({ min_bar_spacing: 4, bar_spacing: 21 });
  return () => scale.apply_options({
    min_bar_spacing: previous.min_bar_spacing,
    bar_spacing: previous.bar_spacing,
  });
}

function background_shade_data(bars) {
  const last = bars.length - 1;
  return bars.map((bar, index) => {
    // The upstream example uses this smooth sample signal over 500 points. Stretch the same
    // domain across the demo's timestamps and pin randomFactor to its 25..50 midpoint.
    const i = last <= 0 ? 0 : index * 499 / last;
    const value = i * (
      0.5
      + Math.sin(i / 10) * 0.2
      + Math.sin(i / 20) * 0.4
      + Math.sin(i / 37.5) * 0.8
      + Math.sin(i / 500) * 0.5
    ) + 200;
    return { time: bar.time, value };
  });
}

/** Deterministic PRNG so stacked demos stay stable across reloads. */
function demo_rand(seed) {
  let state = seed >>> 0;
  return () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return state / 4294967296;
  };
}

/**
 * Realistic stacked composition: mean-reverting category shares + mild activity
 * scaling from bar range. Avoids the sine “mountain range” look.
 */
function stacked_series_data(bars, layer_count = 4) {
  const rand = demo_rand(0x51ac4ed);
  const means = [48, 31, 19, 12].slice(0, layer_count);
  const levels = means.slice();
  let activity = 1;
  return bars.map((bar) => {
    const close = Math.max(1e-6, bar.close ?? 1);
    const range = Math.max(0, (bar.high ?? close) - (bar.low ?? close)) / close;
    // Overall “volume” drifts slowly; wider bars → slightly busier stacks.
    activity = activity * 0.93 + (0.88 + Math.min(0.55, range * 14) + (rand() - 0.5) * 0.06) * 0.07;
    activity = Math.max(0.62, Math.min(1.45, activity));
    const values = levels.map((level, index) => {
      const noise = (rand() - 0.5) * (1.1 + index * 0.2);
      levels[index] = level * 0.91 + means[index] * 0.09 + noise;
      levels[index] = Math.max(means[index] * 0.4, Math.min(means[index] * 1.55, levels[index]));
      return Math.round(Math.max(2, levels[index] * activity) * 10) / 10;
    });
    return { time: bar.time, values };
  });
}

const FOOTPRINT_TICK = 0.1;
const FOOTPRINT_BARS = 30;
const FOOTPRINT_INTERVAL = 3600;

/**
 * Seeded order-flow tape for the footprint preview. Each bar trades inside its hidden candle's own
 * range (so price overlays stay aligned) along an open → extreme → extreme → close path, with a
 * time-at-price profile, a directional bid/ask skew, and occasional initiative runs that produce
 * stacked imbalances near the extreme the bar moved toward.
 */
function footprint_trades(bars) {
  const rand = demo_rand(0xf007);
  const clamp = (value, low, high) => Math.min(high, Math.max(low, value));
  const trades = [];
  let trade_id = 1;
  for (const bar of bars.slice(-FOOTPRINT_BARS)) {
    const low = Math.round(bar.low / FOOTPRINT_TICK);
    const high = Math.max(low + 2, Math.round(bar.high / FOOTPRINT_TICK));
    const open = clamp(Math.round(bar.open / FOOTPRINT_TICK), low, high);
    const close = clamp(Math.round(bar.close / FOOTPRINT_TICK), low, high);
    const span = high - low;
    const bias = (close - open) / span;
    const poc = Math.round(low + span * (0.3 + rand() * 0.4));
    const spread = Math.max(1.5, span * (0.2 + rand() * 0.14));
    const peak = 260 + rand() * 520;
    // An initiative run of 3-4 levels at the extreme the bar pushed toward.
    const run_length = rand() < 0.45 ? 3 + Math.floor(rand() * 2) : 0;
    const run_start = bias >= 0 ? high - run_length - Math.floor(rand() * 2) : low + Math.floor(rand() * 2);

    // Volume per level and side, then spread across every visit of the path.
    const volume = new Map();
    for (let level = low; level <= high; level += 1) {
      const profile = Math.exp(-((level - poc) ** 2) / (2 * spread * spread));
      const total = Math.max(1, Math.round(peak * profile * (0.7 + rand() * 0.6) + rand() * 6));
      const in_run = run_length > 0 && level >= run_start && level < run_start + run_length;
      const ask_share = in_run
        ? (bias >= 0 ? 0.86 : 0.14) + (rand() - 0.5) * 0.06
        : clamp(0.5 + bias * 0.26 + (rand() - 0.5) * 0.3, 0.06, 0.94);
      const ask = Math.round(total * ask_share);
      volume.set(level, { bid: total - ask, ask, visits: 0 });
    }
    const first = bias >= 0 ? low : high;
    const second = bias >= 0 ? high : low;
    const path = [];
    const walk = (from, to) => {
      const step = Math.sign(to - from);
      for (let level = from; level !== to; level += step) path.push(level);
    };
    walk(open, first);
    walk(first, second);
    walk(second, close);
    path.push(close);
    for (const level of path) volume.get(level).visits += 1;

    const start_micros = Math.floor(bar.time / FOOTPRINT_INTERVAL) * FOOTPRINT_INTERVAL * 1_000_000;
    const step_micros = Math.floor((FOOTPRINT_INTERVAL * 1_000_000) / (path.length * 2 + 1));
    let sequence = 0;
    const emit = (level, size, aggressor) => {
      if (size <= 0) return;
      sequence += 1;
      trades.push({
        timestamp_micros: start_micros + sequence * step_micros,
        price: Math.round(level * FOOTPRINT_TICK * 1e6) / 1e6,
        volume: size,
        aggressor,
        sequence,
        trade_id: trade_id++,
        session_id: 1,
      });
    };
    for (const level of path) {
      const row = volume.get(level);
      const bid = Math.round(row.bid / row.visits);
      const ask = Math.round(row.ask / row.visits);
      row.visits -= 1;
      row.bid -= bid;
      row.ask -= ask;
      emit(level, bid, "sell");
      emit(level, ask, "buy");
    }
  }
  return trades;
}

function use_footprint_view(chart, footprint) {
  const scale = chart.time_scale();
  const previous = scale.options();
  const price_scale = footprint.price_scale();
  const previous_margins = price_scale.options().scale_margins;
  // The configured spacing matches the engine's footprint zoom (FOOTPRINT_BAR_SPACING), so a time
  // scale reset returns to readable clusters instead of candle-width bars.
  scale.apply_options({ min_bar_spacing: 4, bar_spacing: 104, right_offset: 0 });
  // Clusters read best using the pane height; autoscale reserves room for the bar summary.
  price_scale.apply_options({ scale_margins: { top: 0.05, bottom: 0.03 } });
  footprint.fit_viewport();
  // One empty slot keeps the forming bar clear of the last-value title chip.
  scale.scroll_to_position(1, false);
  return () => {
    scale.apply_options({
      min_bar_spacing: previous.min_bar_spacing,
      bar_spacing: previous.bar_spacing,
      right_offset: previous.right_offset,
    });
    price_scale.apply_options({ scale_margins: previous_margins });
  };
}

// Presentation choices survive leaving and reopening the footprint preview.
const footprint_style = { cell_mode: "bid_ask", show_bar_summary: true };

function series_features(bars, primary_series) {
  const sampled = bars.filter((_, index) => index % 5 === 0);
  const closes = sampled.map((bar) => bar.close);
  const shade_data = background_shade_data(bars);
  const base = Math.floor(Math.min(...closes) - 2);
  return [
    {
      id: "footprint", label: "Footprint", detail: "Bid × Ask · POC · stacked delta", icon: "chart",
      preserve_time_spacing: true,
      create: (chart) => {
        const footprint = chart.add_series("footprint", {
          // Footprint replaces the main price series, so it must keep the main scale. Drawings,
          // price indicators, alerts, and trading lines are already bound to this scale.
          price_scale_id: primary_series.price_scale_id(),
          tick_size: FOOTPRINT_TICK,
          interval_seconds: FOOTPRINT_INTERVAL,
          imbalance_ratio: 3,
          imbalance_minimum_volume: 20,
          stacked_imbalance_levels: 3,
          ...footprint_style,
          price_line_visible: true,
          last_value_visible: true,
          countdown_visible: true,
          title: "ORDER FLOW",
        });
        footprint.set_trades(footprint_trades(bars));
        return footprint;
      },
      compose: (chart, footprint) => use_footprint_view(chart, footprint),
    },
    {
      id: "brushable-area", label: "Brushable area", detail: "Drag to compare", icon: "chart", interactive: true,
      series_kind: "area",
      options: {},
      data: () => bars.map((bar) => ({ time: bar.time, value: bar.close })),
    },
    {
      id: "grouped-bars", label: "Grouped bars", detail: "Side-by-side values", icon: "chart",
      series_kind: "grouped_bars", options: { base_price: 0 },
      data: () => sampled.map((bar, index) => ({ time: bar.time, values: [12 + index % 13, 18 + index % 9, 8 + index % 16] })),
    },
    {
      id: "heatmap-standalone", label: "Heatmap grid", detail: "Standalone multi-cell map", icon: "chart",
      preserve_time_spacing: true,
      series_kind: "heatmap",
      options: { cell_shader: (amount) => rainbow_color(amount / 36), cell_border_color: "rgba(255,255,255,.22)" },
      data: () => bars.map((bar, time_index) => ({
        time: bar.time,
        cells: Array.from({ length: 10 }, (_, price_index) => ({
          low: price_index * 10,
          high: (price_index + 1) * 10,
          amount: 18 + 18 * Math.sin(time_index * 0.21 + price_index * 0.47),
        })),
      })),
      compose: (chart) => use_official_feature_spacing(chart),
    },
    {
      id: "heatmap-line", label: "Heatmap + line", detail: "Multi-cell distribution around line", icon: "chart",
      preserve_time_spacing: true,
      series_kind: "heatmap",
      options: {
        cell_border_width: 0,
        cell_shader: (amount) => {
          const value = Math.max(0, Math.min(100, amount));
          return `rgba(${155 - value}, 0, ${155 + value}, ${0.05 + value * 0.01})`;
        },
      },
      data: () => bars.map((bar) => ({
        time: bar.time,
        cells: Array.from({ length: 13 }, (_, index) => {
          const offset = index - 6;
          return {
            low: bar.close + offset * 0.8,
            high: bar.close + (offset + 1) * 0.8,
            amount: 100 * Math.exp(-(offset * offset) / 8),
          };
        }),
      })),
      compose: (chart) => {
        const restore_spacing = use_official_feature_spacing(chart);
        const remove_line = add_line_companion(
          chart,
          bars.map((bar) => ({ time: bar.time, value: bar.close })),
          { color: PRIMARY_BLUE },
        );
        return () => { remove_line(); restore_spacing(); };
      },
    },
    {
      id: "hlc-area", label: "HLC area", detail: "High/low envelope", icon: "chart",
      series_kind: "hlc_area", options: {},
      data: () => sampled.map(({ time, high, low, close }) => ({ time, high, low, close })),
    },
    {
      id: "pretty-histogram", label: "Pretty histogram", detail: "Rounded columns", icon: "chart",
      series_kind: "pretty_histogram", options: { base_price: base, color: "#a459d1", width_percent: 64 },
      data: () => sampled.map((bar) => ({ time: bar.time, value: bar.close, color: bar.close >= bar.open ? "#089981" : "#f7525f" })),
    },
    {
      id: "shaded-background", label: "Shaded backdrop", detail: "Per-bar shade field + line", icon: "chart",
      series_kind: "background_shade", options: { low_value: 0, high_value: 1000 },
      data: () => shade_data,
      compose: (chart) => add_line_companion(
        chart,
        shade_data,
        { color: PRIMARY_BLUE, line_width: 2, price_line_visible: true },
      ),
    },
    {
      id: "stacked-area", label: "Stacked area", detail: "Cumulative layers", icon: "chart",
      preserve_time_spacing: true,
      series_kind: "stacked_area", options: {},
      data: () => stacked_series_data(bars, 4),
      compose: (chart) => use_official_feature_spacing(chart),
    },
    {
      id: "stacked-bars", label: "Stacked bars", detail: "Cumulative columns", icon: "chart",
      preserve_time_spacing: true,
      series_kind: "stacked_bars", options: {},
      data: () => stacked_series_data(bars, 4),
      compose: (chart) => use_official_feature_spacing(chart),
    },
    {
      id: "whisker-box", label: "Whisker box", detail: "Quartiles + outliers", icon: "chart",
      series_kind: "whisker_box", options: {},
      data: () => sampled.map((bar) => ({ time: bar.time, quartiles: [bar.low - .5, bar.low, bar.close, bar.high, bar.high + .5], outliers: [bar.high + 1] })),
    },
  ];
}

function primitive_features(chart, series, bars) {
  const middle_index = Math.floor(bars.length / 2);
  const start = bars[middle_index - 30];
  const middle = bars[middle_index];
  const end = bars[middle_index + 30];
  return [
    { id: "bands-indicator", label: "Price-band primitive", detail: "Official ±10% background", icon: "analysis", activate: () => { const handle = create_bands_indicator(series); return () => handle.detach(); } },
    { id: "trend-line", label: "Trend-line primitive", detail: "Series primitive + endpoint labels", icon: "draw", activate: () => { const handle = create_trend_line(series, [{ time: start.time, price: start.low }, { time: end.time, price: end.high }], { line_color: "#f59e0b" }); return () => handle.detach(); } },
    { id: "vertical-line", label: "Event-line primitive", detail: "Series primitive + time-axis label", icon: "draw", activate: () => { const handle = create_vertical_line(series, middle.time, { color: "#e1575a", label_text: "Event", label_background_color: "#e1575a", show_label: true }); return () => handle.detach(); } },
    { id: "overlay-scale", label: "Overlay-scale helper", detail: "In-pane rounded price labels", icon: "analysis", activate: () => { const overlay = chart.add_series("line", { color: "#a459d1", line_width: 2, price_line_visible: false, countdown_visible: false }); overlay.set_data(bars.filter((_, index) => index % 4 === 0).map((bar) => ({ time: bar.time, value: bar.close * .35 }))); const labels = create_overlay_price_scale(overlay); return () => { labels.detach(); chart.remove_series(overlay); }; } },
    { id: "session-highlighting", label: "Session-highlighting primitive", detail: "Weekday / weekend", icon: "analysis", activate: () => { const handle = create_session_highlighting(series); return () => handle.detach(); } },
    { id: "highlight-crosshair", label: "Crosshair-highlight helper", detail: "Bar highlight follows crosshair", icon: "analysis", activate: () => { const handle = create_highlight_bar_crosshair(chart, series); return () => handle.detach(); } },
    { id: "tooltip", label: "Tooltip", detail: "Hover values", icon: "lab", activate: () => { const handle = create_tooltip(chart, { series }); return () => handle.detach(); } },
    {
      id: "delta-tooltip",
      label: "Delta tooltip",
      detail: "Drag comparison",
      icon: "lab",
      available: () => series.series_type() !== "candlestick",
      activate: () => { const handle = create_delta_tooltip(chart, { series }); return () => handle.detach(); },
    },
  ];
}

function install_demo_bracket_host(chart) {
  const trading = chart.trading();
  const place_demo_bracket = (intent) => {
    if (intent.action !== "place_bracket_order") return;
    const entry_id = `demo-entry-${intent.sequence}`;
    const bracket_id = `demo-bracket-${intent.sequence}`;
    const oco_group_id = `demo-oco-${intent.sequence}`;
    const exit_side = intent.side === "buy" ? "sell" : "buy";
    trading.resolve_intent(intent.sequence, true);
    trading.update_order({
      id: entry_id,
      pane_index: intent.pane_index,
      price_scale: intent.price_scale,
      side: intent.side,
      kind: intent.kind,
      role: "working",
      status: "working",
      price: intent.price,
      quantity: intent.quantity,
      bracket_id,
    });
    trading.update_order({
      id: `demo-target-${intent.sequence}`,
      pane_index: intent.pane_index,
      price_scale: intent.price_scale,
      side: exit_side,
      kind: "limit",
      role: "take_profit",
      status: "working",
      price: intent.take_profit_price,
      quantity: intent.quantity,
      parent_order_id: entry_id,
      bracket_id,
      oco_group_id,
    });
    trading.update_order({
      id: `demo-stop-${intent.sequence}`,
      pane_index: intent.pane_index,
      price_scale: intent.price_scale,
      side: exit_side,
      kind: "stop",
      role: "stop_loss",
      status: "working",
      price: intent.stop_loss_price,
      quantity: intent.quantity,
      parent_order_id: entry_id,
      bracket_id,
      oco_group_id,
    });
  };
  trading.subscribe_intents(place_demo_bracket);
}

function trading_features(chart, bars) {
  const middle = bars[Math.floor(bars.length * 0.62)];
  const entry = middle.close;
  const target = entry + Math.max(entry * 0.018, 1);
  const stop = entry - Math.max(entry * 0.012, 0.75);
  // Fills sit inside their bar's time and price range: single and multi-fill bars on both
  // sides, so the shared arrow, stacked-chevron, and exact-fill hover paths are all visible.
  const execution_fills = () => {
    const step = bars[bars.length - 1].time - bars[bars.length - 2].time;
    const fill = (id, back, side, parts, price_at) => parts.map((part, index) => {
      const bar = bars[bars.length - back];
      return {
        id: `${id}-${index}`,
        side,
        kind: index === 0 ? "entry" : "partial_fill",
        time: bar.time + Math.floor(step * part),
        price: bar.low + (bar.high - bar.low) * price_at[index],
        quantity: index + 1,
      };
    });
    return [
      ...fill("single-buy", 34, "buy", [0.4], [0.35]),
      ...fill("single-sell", 29, "sell", [0.6], [0.8]),
      ...fill("multi-buy", 23, "buy", [0.1, 0.45, 0.8], [0.2, 0.45, 0.6]),
      ...fill("multi-sell", 17, "sell", [0.15, 0.4, 0.65, 0.9], [0.9, 0.75, 0.6, 0.5]),
      ...fill("both-buy", 11, "buy", [0.2], [0.3]),
      ...fill("both-sell", 11, "sell", [0.7], [0.85]),
      ...fill("pair-buy", 5, "buy", [0.1, 0.5], [0.25, 0.4]),
      ...fill("pair-sell", 5, "sell", [0.3, 0.9], [0.7, 0.9]),
    ];
  };
  return [{
    id: "execution-marks",
    label: "Execution marks",
    detail: "Single and multi-fill arrows, exact-fill hover",
    icon: "analysis",
    activate: () => {
      const trading = chart.trading();
      trading.apply_snapshot({ instrument: { price_precision: 2 }, executions: execution_fills() });
      // Frame the fills so single and stacked marks are readable at a glance. Wait for the first
      // painted frames: a scenario opened from the URL activates before the chart is sized.
      requestAnimationFrame(() => requestAnimationFrame(() =>
        chart.timeScale().setVisibleLogicalRange({ from: bars.length - 45, to: bars.length + 2 })));
      return () => trading.apply_snapshot({});
    },
  }, {
    id: "trading-bracket",
    label: "Trading bracket",
    detail: "Position, OCO orders, partial fill",
    icon: "analysis",
    activate: () => {
      const trading = chart.trading();
      const alerts = chart.alerts();
      const demo_alert_ids = new Set();
      const create_demo_alert = (request) => {
        const id = `demo-alert-${request.sequence}`;
        demo_alert_ids.add(id);
        alerts.update_line({
          id,
          pane_index: request.pane_index,
          price_scale: request.price_scale_id === "" ? "overlay" : request.price_scale_id,
          price: request.price,
          condition: "crossing",
          frequency: "only_once",
          status: "active",
        });
      };
      chart.subscribe_crosshair_action(create_demo_alert);
      trading.apply_snapshot({
        instrument: {
          tick_size: 0.01,
          price_precision: 2,
          quantity_precision: 0,
          point_value: 1,
          currency: "USD",
        },
        positions: [{
          id: "demo-position",
          side: "long",
          average_price: entry,
          quantity: 12,
          display_pnl: 184.5,
        }],
        orders: [
          {
            id: "demo-target",
            side: "sell",
            kind: "limit",
            role: "take_profit",
            status: "working",
            price: target,
            quantity: 12,
            position_id: "demo-position",
            bracket_id: "demo-bracket",
            oco_group_id: "demo-oco",
          },
          {
            id: "demo-stop",
            side: "sell",
            kind: "stop",
            role: "stop_loss",
            status: "working",
            price: stop,
            quantity: 12,
            position_id: "demo-position",
            bracket_id: "demo-bracket",
            oco_group_id: "demo-oco",
          },
          {
            id: "demo-partial",
            side: "buy",
            kind: "limit",
            status: "partially_filled",
            price: entry - Math.max(entry * 0.006, 0.4),
            quantity: 12,
            filled_quantity: 5,
          },
        ],
        executions: [{
          id: "demo-fill",
          side: "buy",
          kind: "partial_fill",
          time: middle.time,
          price: entry,
          quantity: 5,
          order_id: "demo-partial",
          position_id: "demo-position",
        }],
      });
      return () => {
        chart.unsubscribe_crosshair_action(create_demo_alert);
        for (const id of demo_alert_ids) alerts.remove_line(id);
        trading.apply_snapshot({});
      };
    },
  }, {
    id: "trading-lines-from-marker",
    label: "Lines from marker",
    detail: "Order lines start at their controls",
    icon: "analysis",
    activate: () => {
      const trading = chart.trading();
      trading.apply_options({ extend_lines_left: false });
      return () => trading.apply_options({ extend_lines_left: true });
    },
  }];
}

export function install_demo_catalogs({ chart, series, data, on_series_change }) {
  install_demo_bracket_host(chart);
  const grid = document.getElementById("feature_grid");
  const series_grid = document.getElementById("series_grid");
  const search = document.getElementById("feature_search");
  const empty = document.getElementById("feature_empty");
  const active_count = document.getElementById("feature_active_count");
  const series_items = series_features(data, series).map((feature) => ({ ...feature, kind: "series" }));
  const primitive_items = primitive_features(chart, series, data).map((feature) => ({ ...feature, kind: "primitive" }));
  const trading_items = trading_features(chart, data).map((feature) => ({ ...feature, kind: "trading" }));
  const features = [...primitive_items, ...trading_items];
  const cleanups = new Map();
  let active_series = null;
  let base_series_choice = document.querySelector('input[name="series"]:checked')?.value ?? "candlestick";
  let filter = "all";

  for (const feature of series_items) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "feature-card";
    button.dataset.seriesId = feature.id;
    button.setAttribute("aria-pressed", "false");
    button.title = `${feature.label}: ${feature.detail}`;
    button.innerHTML = `<span class="feature-icon" data-icon="${feature.icon}"></span><strong>${feature.label}</strong><small>${feature.detail}</small>`;
    series_grid.appendChild(button);
  }
  for (const feature of features) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "feature-card";
    button.dataset.featureId = feature.id;
    button.dataset.featureKind = feature.kind;
    button.setAttribute("aria-pressed", "false");
    button.title = `${feature.label}: ${feature.detail}`;
    button.innerHTML = `<span class="feature-icon" data-icon="${feature.icon}"></span><strong>${feature.label}</strong><small>${feature.detail}</small>`;
    grid.appendChild(button);
  }
  hydrate_icons(series_grid);
  hydrate_icons(grid);

  const feature_card = (id) => grid.querySelector(`[data-feature-id="${id}"]`);
  const series_card = (id) => series_grid.querySelector(`[data-series-id="${id}"]`);
  const feature_available = (feature) => feature.available?.() !== false;
  const update_status = (message) => {
    const count = cleanups.size;
    active_count.textContent = message ?? (count === 0 ? "No features active" : `${count} feature${count === 1 ? "" : "s"} active`);
  };
  const refresh_feature_availability = () => {
    for (const feature of features) {
      const available = feature_available(feature);
      const button = feature_card(feature.id);
      button.disabled = !available;
      button.setAttribute("aria-disabled", String(!available));
      if (!available && cleanups.has(feature.id)) {
        cleanups.get(feature.id)();
        cleanups.delete(feature.id);
        button.setAttribute("aria-pressed", "false");
      }
    }
    update_status();
  };
  const clear_series = () => {
    if (active_series === null) return;
    active_series.cleanup?.();
    chart.remove_series(active_series.handle);
    series_card(active_series.id)?.setAttribute("aria-pressed", "false");
    active_series = null;
    series.apply_options({ visible: true });
    const base = document.querySelector(`input[name="series"][value="${base_series_choice}"]`);
    if (base !== null) base.checked = true;
    on_series_change?.(series.series_type());
  };
  const toggle_series = (feature) => {
    try {
      const was_active = active_series?.id === feature.id;
      clear_series();
      if (!was_active) {
        const checked = document.querySelector('input[name="series"]:checked');
        if (checked !== null) base_series_choice = checked.value;
        for (const radio of document.querySelectorAll('input[name="series"]')) radio.checked = false;
        const handle = feature.create?.(chart) ?? chart.add_series(feature.series_kind, {
          ...feature.options,
          price_line_visible: false,
          last_value_visible: false,
          countdown_visible: false,
        });
        if (feature.create === undefined) handle.set_data(feature.data());
        const interaction = feature.interactive === true
          ? enable_brushable_area_interaction(chart, handle)
          : null;
        const remove_companion = feature.compose?.(chart, handle) ?? null;
        active_series = {
          id: feature.id,
          handle,
          interaction,
          cleanup: () => { interaction?.detach(); remove_companion?.(); },
        };
        series.apply_options({ visible: feature.overlay === true });
        series_card(feature.id).setAttribute("aria-pressed", "true");
        on_series_change?.(handle.series_type());
      }
      if (active_series?.id !== feature.id || feature.preserve_time_spacing !== true) {
        chart.time_scale().fit_content();
      }
      chart.render();
    } catch (error) {
      console.error(`Series catalog could not activate ${feature.id}`, error);
    }
  };
  const toggle_feature = (feature) => {
    try {
      if (!feature_available(feature)) return;
      if (cleanups.has(feature.id)) {
        cleanups.get(feature.id)();
        cleanups.delete(feature.id);
        feature_card(feature.id).setAttribute("aria-pressed", "false");
      } else {
        const cleanup = feature.activate();
        cleanups.set(feature.id, typeof cleanup === "function" ? cleanup : () => {});
        feature_card(feature.id).setAttribute("aria-pressed", "true");
      }
      chart.time_scale().fit_content();
      chart.render();
      update_status();
    } catch (error) {
      console.error(`Feature lab could not activate ${feature.id}`, error);
      update_status(`${feature.label} failed — see console`);
    }
  };
  const apply_filter = () => {
    const term = search.value.trim().toLowerCase();
    let visible = 0;
    for (const feature of features) {
      const match = (filter === "all" || feature.kind === filter) && `${feature.label} ${feature.detail}`.toLowerCase().includes(term);
      feature_card(feature.id).hidden = !match;
      if (match) visible += 1;
    }
    empty.dataset.visible = String(visible === 0);
  };

  series_grid.addEventListener("click", (event) => {
    const button = event.target.closest("[data-series-id]");
    const feature = series_items.find((item) => item.id === button?.dataset.seriesId);
    if (feature !== undefined) toggle_series(feature);
  });
  grid.addEventListener("click", (event) => {
    const button = event.target.closest("[data-feature-id]");
    const feature = features.find((item) => item.id === button?.dataset.featureId);
    if (feature !== undefined) toggle_feature(feature);
  });
  search.addEventListener("input", apply_filter);
  for (const button of document.querySelectorAll("[data-feature-filter]")) {
    button.addEventListener("click", () => {
      filter = button.dataset.featureFilter;
      for (const peer of document.querySelectorAll("[data-feature-filter]")) peer.setAttribute("aria-pressed", String(peer === button));
      apply_filter();
    });
  }
  document.getElementById("feature_clear").addEventListener("click", () => {
    for (const [id, cleanup] of cleanups) {
      cleanup();
      feature_card(id)?.setAttribute("aria-pressed", "false");
    }
    cleanups.clear();
    chart.time_scale().fit_content();
    chart.render();
    update_status();
  });
  refresh_feature_availability();

  return {
    series: {
      activate(id) {
        const feature = series_items.find((item) => item.id === id);
        if (feature !== undefined) toggle_series(feature);
      },
      active_id() { return active_series?.id ?? null; },
      interaction_series() { return active_series?.handle ?? series; },
      interaction_range() { return active_series?.interaction?.active_range?.() ?? null; },
      footprint_style() { return { ...footprint_style }; },
      set_footprint_style(patch) {
        Object.assign(footprint_style, patch);
        if (active_series?.id === "footprint") {
          active_series.handle.apply_options(footprint_style);
          chart.render();
        }
      },
      clear: clear_series,
      select_base(value) {
        base_series_choice = value;
        clear_series();
        queueMicrotask(refresh_feature_availability);
      },
    },
    lab: {
      activate(id) {
        const feature = features.find((item) => item.id === id);
        if (feature !== undefined) toggle_feature(feature);
      },
      active_ids() { return [...cleanups.keys()]; },
      clear() { document.getElementById("feature_clear").click(); },
      refresh: refresh_feature_availability,
    },
  };
}
