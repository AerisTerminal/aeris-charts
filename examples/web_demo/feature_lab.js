import {
  create_anchored_text,
  create_bands_indicator,
  create_delta_tooltip,
  create_highlight_bar_crosshair,
  create_image_watermark,
  create_overlay_price_scale,
  create_partial_price_line,
  create_rectangle_drawing_tool,
  create_session_highlighting,
  create_tooltip,
  create_trend_line,
  create_vertical_line,
  create_volume_profile,
  default_theme_name,
  enable_accessibility,
  enable_brushable_area_interaction,
  theme_palette,
} from "./dist/nucleuscharts_financial.js";
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

function footprint_trades(bars) {
  const tick_size = 0.25;
  const sample = bars.slice(-8);
  const trades = [];
  let trade_id = 1;
  for (let bar_index = 0; bar_index < sample.length; bar_index += 1) {
    const bar = sample[bar_index];
    const start_seconds = Math.floor(bar.time / 3600) * 3600;
    const center_level = Math.round(bar.close / tick_size) + (bar_index % 3) - 1;
    const ask_dominant = bar_index % 2 === 0;
    const levels = [-2, -1, 0, 1, 2].map((offset) => center_level + offset);
    const events = [];
    for (let index = 0; index < levels.length; index += 1) {
      const level = levels[index];
      const edge = index === 0 || index === levels.length - 1;
      const bid_volume = ask_dominant
        ? 4 + index
        : (index <= 2 ? 54 - index * 8 : 8 + index);
      const ask_volume = ask_dominant
        ? (index >= 2 ? 46 + (index - 2) * 9 : 6 + index)
        : 5 + index;
      events.push(
        { level, volume: edge ? Math.max(2, bid_volume - 2) : bid_volume, aggressor: "sell" },
        { level, volume: edge ? Math.max(2, ask_volume - 2) : ask_volume, aggressor: "buy" },
      );
    }
    // Reverse alternate bars so the running path visibly exercises both positive and negative
    // Max/Min Delta before mean-reverting to the same final accounting.
    if (!ask_dominant) events.reverse();
    for (let event_index = 0; event_index < events.length; event_index += 1) {
      const event = events[event_index];
      trades.push({
        timestamp_micros: start_seconds * 1_000_000 + event_index * 10_000 + 1,
        price: event.level * tick_size,
        volume: event.volume,
        aggressor: event.aggressor,
        sequence: event_index,
        trade_id,
        session_id: 1,
      });
      trade_id += 1;
    }
  }
  return trades;
}

function use_footprint_spacing(chart) {
  const scale = chart.time_scale();
  const previous = scale.options();
  scale.apply_options({ min_bar_spacing: 4, bar_spacing: 96, right_offset: 0 });
  scale.scroll_to_real_time();
  return () => scale.apply_options({
    min_bar_spacing: previous.min_bar_spacing,
    bar_spacing: previous.bar_spacing,
    right_offset: previous.right_offset,
  });
}

function series_features(bars) {
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
          tick_size: 0.25,
          interval_seconds: 3600,
          imbalance_ratio: 3,
          imbalance_minimum_volume: 20,
          stacked_imbalance_levels: 3,
          cell_mode: "bid_ask",
          font_size: 10,
          show_bar_summary: true,
          price_line_visible: false,
          last_value_visible: false,
          title: "ORDER FLOW",
        });
        footprint.set_trades(footprint_trades(bars));
        return footprint;
      },
      compose: (chart) => use_footprint_spacing(chart),
    },
    {
      id: "brushable-area", label: "Brushable area", detail: "Drag to brush", icon: "chart", interactive: true,
      series_kind: "brushable_area",
      options: { base_price: base },
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
      id: "shaded-background", label: "Shaded backdrop", detail: "LWC shade field + line", icon: "chart",
      series_kind: "background_shade", options: { low_value: 0, high_value: 1000 },
      data: () => shade_data,
      compose: (chart) => add_line_companion(
        chart,
        shade_data,
        { color: PRIMARY_BLUE, line_width: 3, price_line_visible: true },
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
    { id: "anchored-text", label: "Anchored text", detail: "Engine series primitive", icon: "draw", activate: () => { const handle = create_anchored_text(series, { text: "Anchored Text", vert_align: "middle", horz_align: "middle", line_height: 32, font: "italic bold 32px Arial", color: "#2962ff" }); return () => handle.detach(); } },
    { id: "bands-indicator", label: "Price bands", detail: "Official ±10% background", icon: "analysis", activate: () => { const handle = create_bands_indicator(series); return () => handle.detach(); } },
    { id: "rectangle", label: "Rectangle", detail: "Two-click tool + axis labels", icon: "draw", interactive: true, activate: () => { const tool = create_rectangle_drawing_tool(chart, series, undefined, { fill_color: "rgba(164,89,209,.75)", preview_fill_color: "rgba(164,89,209,.25)", label_color: "#a459d1" }); tool.start_drawing(); return () => tool.remove(); } },
    { id: "trend-line", label: "Trend line", detail: "Line + endpoint labels", icon: "draw", activate: () => { const handle = create_trend_line(series, [{ time: start.time, price: start.low }, { time: end.time, price: end.high }], { line_color: "#f59e0b" }); return () => handle.detach(); } },
    { id: "vertical-line", label: "Vertical line", detail: "Pane + time-axis primitive", icon: "draw", activate: () => { const handle = create_vertical_line(series, middle.time, { color: "#e1575a", label_text: "Event", label_background_color: "#e1575a", show_label: true }); return () => handle.detach(); } },
    { id: "overlay-scale", label: "Overlay scale", detail: "In-pane rounded price labels", icon: "analysis", activate: () => { const overlay = chart.add_series("line", { color: "#a459d1", line_width: 2, price_line_visible: false }); overlay.set_data(bars.filter((_, index) => index % 4 === 0).map((bar) => ({ time: bar.time, value: bar.close * .35 }))); const labels = create_overlay_price_scale(overlay); return () => { labels.detach(); chart.remove_series(overlay); }; } },
    { id: "partial-price-line", label: "Partial line", detail: "Last value → edge", icon: "analysis", activate: () => { const handle = create_partial_price_line(series); return () => handle.detach(); } },
    { id: "session-highlighting", label: "Sessions", detail: "Weekday / weekend", icon: "analysis", activate: () => { const handle = create_session_highlighting(series); return () => handle.detach(); } },
    { id: "highlight-crosshair", label: "Bar highlight", detail: "Follow crosshair", icon: "analysis", activate: () => { const handle = create_highlight_bar_crosshair(chart, series); return () => handle.detach(); } },
    { id: "image-watermark", label: "Image watermark", detail: "Engine raster primitive", icon: "lab", activate: () => { const image = document.createElement("canvas"); image.width = 96; image.height = 96; const context = image.getContext("2d"); context.fillStyle = "#2962ff"; context.beginPath(); context.roundRect(8, 8, 80, 80, 20); context.fill(); context.fillStyle = "white"; context.font = "700 52px Inter, sans-serif"; context.textAlign = "center"; context.textBaseline = "middle"; context.fillText("N", 48, 52); const handle = create_image_watermark(series, image, { max_width: 96, max_height: 96, alpha: .22 }); return () => handle.detach(); } },
    { id: "tooltip", label: "Tooltip", detail: "Hover values", icon: "lab", activate: () => { const handle = create_tooltip(chart, { series }); return () => handle.detach(); } },
    { id: "delta-tooltip", label: "Delta tooltip", detail: "Drag comparison", icon: "lab", activate: () => { const handle = create_delta_tooltip(chart, { series }); return () => handle.detach(); } },
    { id: "volume-profile", label: "Volume profile", detail: "Time-anchored rows", icon: "chart", activate: () => { const base = start.close; const profile = Array.from({ length: 15 }, (_, index) => ({ price: base + (index - 7) * .45, vol: 4 + (index * 13) % 25 })); const handle = create_volume_profile(series, { time: start.time, profile, width: 12 }); return () => handle.detach(); } },
    { id: "accessibility", label: "Accessibility", detail: "Keyboard + live text", icon: "lab", activate: () => { const handle = enable_accessibility(chart, { chart_title: "Nucleus feature lab chart" }); return () => handle.detach(); } },
  ];
}

function trading_features(chart, bars) {
  const middle = bars[Math.floor(bars.length * 0.62)];
  const entry = middle.close;
  const target = entry + Math.max(entry * 0.018, 1);
  const stop = entry - Math.max(entry * 0.012, 0.75);
  return [{
    id: "trading-bracket",
    label: "Trading bracket",
    detail: "Position, OCO orders, partial fill",
    icon: "analysis",
    activate: () => {
      const trading = chart.trading();
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
      return () => trading.apply_snapshot({});
    },
  }];
}

export function install_feature_lab({ chart, series, data }) {
  const grid = document.getElementById("feature_grid");
  const search = document.getElementById("feature_search");
  const empty = document.getElementById("feature_empty");
  const active_count = document.getElementById("feature_active_count");
  const series_items = series_features(data).map((feature) => ({ ...feature, kind: "series" }));
  const primitive_items = primitive_features(chart, series, data).map((feature) => ({ ...feature, kind: "primitive" }));
  const trading_items = trading_features(chart, data).map((feature) => ({ ...feature, kind: "trading" }));
  const features = [...series_items, ...primitive_items, ...trading_items];
  const cleanups = new Map();
  let active_series = null;
  let filter = "all";

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
  hydrate_icons(grid);

  const card = (id) => grid.querySelector(`[data-feature-id="${id}"]`);
  const update_status = (message) => {
    const count = cleanups.size + (active_series === null ? 0 : 1);
    active_count.textContent = message ?? (count === 0 ? "No features active" : `${count} feature${count === 1 ? "" : "s"} active`);
  };
  const clear_series = () => {
    if (active_series === null) return;
    active_series.cleanup?.();
    chart.remove_series(active_series.handle);
    card(active_series.id)?.setAttribute("aria-pressed", "false");
    active_series = null;
    series.apply_options({ visible: true });
  };
  const toggle = (feature) => {
    try {
      if (feature.kind === "series") {
        const was_active = active_series?.id === feature.id;
        clear_series();
        if (!was_active) {
          const handle = feature.create?.(chart) ?? chart.add_series(feature.series_kind, {
            ...feature.options,
            price_line_visible: false,
            last_value_visible: false,
          });
          if (feature.create === undefined) handle.set_data(feature.data());
          const interaction = feature.interactive === true
            ? enable_brushable_area_interaction(chart, handle)
            : null;
          const remove_companion = feature.compose?.(chart, handle) ?? null;
          active_series = {
            id: feature.id,
            handle,
            cleanup: () => { interaction?.detach(); remove_companion?.(); },
          };
          series.apply_options({ visible: feature.overlay === true });
          card(feature.id).setAttribute("aria-pressed", "true");
        }
      } else if (cleanups.has(feature.id)) {
        cleanups.get(feature.id)();
        cleanups.delete(feature.id);
        card(feature.id).setAttribute("aria-pressed", "false");
      } else {
        const cleanup = feature.activate();
        cleanups.set(feature.id, typeof cleanup === "function" ? cleanup : () => {});
        card(feature.id).setAttribute("aria-pressed", "true");
      }
      if (active_series?.id !== feature.id || feature.preserve_time_spacing !== true) {
        chart.time_scale().fit_content();
      }
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
      card(feature.id).hidden = !match;
      if (match) visible += 1;
    }
    empty.dataset.visible = String(visible === 0);
  };

  grid.addEventListener("click", (event) => {
    const button = event.target.closest("[data-feature-id]");
    const feature = features.find((item) => item.id === button?.dataset.featureId);
    if (feature !== undefined) toggle(feature);
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
    clear_series();
    for (const [id, cleanup] of cleanups) {
      cleanup();
      card(id)?.setAttribute("aria-pressed", "false");
    }
    cleanups.clear();
    chart.time_scale().fit_content();
    chart.render();
    update_status();
  });

  return {
    activate(id) { const feature = features.find((item) => item.id === id); if (feature !== undefined) toggle(feature); },
    active_ids() { return [...(active_series === null ? [] : [active_series.id]), ...cleanups.keys()]; },
    clear() { document.getElementById("feature_clear").click(); },
  };
}
