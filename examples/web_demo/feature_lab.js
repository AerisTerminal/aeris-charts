import {
  create_anchored_text,
  create_bands_indicator,
  create_delta_tooltip,
  create_expiring_price_alerts,
  create_highlight_bar_crosshair,
  create_image_watermark,
  create_overlay_price_scale,
  create_partial_price_line,
  create_rectangle_drawing_tool,
  create_session_highlighting,
  create_tooltip,
  create_trend_line,
  create_user_price_alerts,
  create_user_price_lines,
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

function series_features(bars) {
  const sampled = bars.filter((_, index) => index % 5 === 0);
  const closes = sampled.map((bar) => bar.close);
  const shade_data = background_shade_data(bars);
  const base = Math.floor(Math.min(...closes) - 2);
  return [
    {
      id: "brushable-area", label: "Brushable area", detail: "Drag to brush", icon: "chart", interactive: true,
      series_kind: "brushable_area",
      options: { base_price: base },
      data: () => bars.map((bar) => ({ time: bar.time, value: bar.close })),
    },
    {
      id: "dual-range-histogram", label: "Dual range", detail: "Symmetric histogram", icon: "chart",
      preserve_time_spacing: true,
      series_kind: "dual_range_histogram", options: {},
      data: () => bars.map((bar, index) => ({ time: bar.time, values: [12 + index % 17, 6 + index % 9, -(8 + index % 13), -(4 + index % 7)] })),
      compose: (chart, histogram) => {
        const restore_spacing = use_official_feature_spacing(chart);
        const hovered_series_on_top = chart.options().hoveredSeriesOnTop;
        chart.apply_options({ hoveredSeriesOnTop: false });
        const values = bars.map((bar) => bar.close);
        const middle = (Math.min(...values) + Math.max(...values)) / 2;
        const remove_baseline = add_line_companion(
          chart,
          bars.map((bar) => ({ time: bar.time, value: bar.close - middle })),
          { baseline_value: 0 },
          "baseline",
        );
        const scale = histogram.price_scale();
        const previous_margins = scale.options().scale_margins;
        const update_margins = () => {
          const height = chart.panes()[0].get_geometry().height;
          const margin = Math.min(0.3, histogram.options().max_height / 2 / height);
          scale.apply_options({ scale_margins: { top: margin, bottom: margin } });
        };
        const resize_observer = new ResizeObserver(update_margins);
        resize_observer.observe(chart.chart_element());
        update_margins();
        return () => {
          resize_observer.disconnect();
          scale.apply_options({ scale_margins: previous_margins });
          remove_baseline();
          chart.apply_options({ hoveredSeriesOnTop: hovered_series_on_top });
          restore_spacing();
        };
      },
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
      id: "rounded-candles", label: "Rounded candles", detail: "Custom OHLC", icon: "candles",
      series_kind: "rounded_candles", options: {},
      data: () => sampled.map(({ time, open, high, low, close }) => ({ time, open, high, low, close })),
    },
    {
      id: "shaded-background", label: "Shaded backdrop", detail: "LWC shade field + line", icon: "chart",
      series_kind: "background_shade", options: { low_value: 0, high_value: 1000 },
      data: () => shade_data,
      compose: (chart) => add_line_companion(
        chart,
        shade_data,
        { color: "#000000", line_width: 3, price_line_visible: true },
      ),
    },
    {
      id: "stacked-area", label: "Stacked area", detail: "Cumulative layers", icon: "chart",
      series_kind: "stacked_area", options: {},
      data: () => sampled.map((bar, index) => ({ time: bar.time, values: [12 + index % 18, 8 + index % 10, 5 + index % 7] })),
    },
    {
      id: "stacked-bars", label: "Stacked bars", detail: "Cumulative columns", icon: "chart",
      series_kind: "stacked_bars", options: {},
      data: () => sampled.map((bar, index) => ({ time: bar.time, values: [12 + index % 18, 8 + index % 10, 5 + index % 7] })),
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
    { id: "user-price-line", label: "User price lines", detail: "Hover the pane's right edge", icon: "analysis", activate: () => { const handle = create_user_price_lines(chart, series, { color: "#f59e0b" }); return () => handle.detach(); } },
    { id: "overlay-scale", label: "Overlay scale", detail: "In-pane rounded price labels", icon: "analysis", activate: () => { const overlay = chart.add_series("line", { color: "#a459d1", line_width: 2, price_line_visible: false }); overlay.set_data(bars.filter((_, index) => index % 4 === 0).map((bar) => ({ time: bar.time, value: bar.close * .35 }))); const labels = create_overlay_price_scale(overlay); return () => { labels.detach(); chart.remove_series(overlay); }; } },
    { id: "partial-price-line", label: "Partial line", detail: "Last value → edge", icon: "analysis", activate: () => { const handle = create_partial_price_line(series); return () => handle.detach(); } },
    { id: "session-highlighting", label: "Sessions", detail: "Weekday / weekend", icon: "analysis", activate: () => { const handle = create_session_highlighting(series); return () => handle.detach(); } },
    { id: "highlight-crosshair", label: "Bar highlight", detail: "Follow crosshair", icon: "analysis", activate: () => { const handle = create_highlight_bar_crosshair(chart, series); return () => handle.detach(); } },
    { id: "image-watermark", label: "Image watermark", detail: "Engine raster primitive", icon: "lab", activate: () => { const image = document.createElement("canvas"); image.width = 96; image.height = 96; const context = image.getContext("2d"); context.fillStyle = "#2962ff"; context.beginPath(); context.roundRect(8, 8, 80, 80, 20); context.fill(); context.fillStyle = "white"; context.font = "700 52px Inter, sans-serif"; context.textAlign = "center"; context.textBaseline = "middle"; context.fillText("N", 48, 52); const handle = create_image_watermark(series, image, { max_width: 96, max_height: 96, alpha: .22 }); return () => handle.detach(); } },
    { id: "tooltip", label: "Tooltip", detail: "Hover values", icon: "lab", activate: () => { const handle = create_tooltip(chart, { series }); return () => handle.detach(); } },
    { id: "delta-tooltip", label: "Delta tooltip", detail: "Drag comparison", icon: "lab", activate: () => { const handle = create_delta_tooltip(chart, { series }); return () => handle.detach(); } },
    { id: "volume-profile", label: "Volume profile", detail: "Time-anchored rows", icon: "chart", activate: () => { const base = start.close; const profile = Array.from({ length: 15 }, (_, index) => ({ price: base + (index - 7) * .45, vol: 4 + (index * 13) % 25 })); const handle = create_volume_profile(series, { time: start.time, profile, width: 12 }); return () => handle.detach(); } },
    { id: "expiring-alerts", label: "Expiring alerts", detail: "Data-time ranges and directional crossing", icon: "analysis", activate: () => { const alerts = create_expiring_price_alerts(series); const last = bars[bars.length - 1]; alerts.add(middle.close, middle.time, last.time + 86_400 * 4, { title: "Crossing up", crossing_direction: "up" }); alerts.add(middle.close * 1.015, middle.time, last.time + 86_400 * 7, { title: "Crossing down", crossing_direction: "down" }); return () => alerts.detach(); } },
    { id: "user-alerts", label: "User alerts", detail: "Right-click chart", icon: "analysis", activate: () => { const handle = create_user_price_alerts(chart, series); return () => handle.detach(); } },
    { id: "accessibility", label: "Accessibility", detail: "Keyboard + live text", icon: "lab", activate: () => { const handle = enable_accessibility(chart, { chart_title: "Nucleus feature lab chart" }); return () => handle.detach(); } },
  ];
}

export function install_feature_lab({ chart, series, data }) {
  const grid = document.getElementById("feature_grid");
  const search = document.getElementById("feature_search");
  const empty = document.getElementById("feature_empty");
  const active_count = document.getElementById("feature_active_count");
  const series_items = series_features(data).map((feature) => ({ ...feature, kind: "series" }));
  const primitive_items = primitive_features(chart, series, data).map((feature) => ({ ...feature, kind: "primitive" }));
  const features = [...series_items, ...primitive_items];
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
          const handle = chart.add_series(feature.series_kind, {
            ...feature.options,
            price_line_visible: false,
            last_value_visible: false,
          });
          handle.set_data(feature.data());
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
