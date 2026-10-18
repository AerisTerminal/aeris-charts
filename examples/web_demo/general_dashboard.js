import { create_chart } from "./dist/aeris_charts_financial.js";

let dashboard_root = null;
let chart_grid = null;
let runtime_badge = null;
let dashboard_error = null;
let requested_backend = "auto";
let active_theme = "dark";

// The GPUI gallery (`gpui_general_gallery`) renders this same catalog, so both hosts show
// identical examples through the shared engine frame.
const catalog = await (await fetch(new URL("./general_catalog.json", import.meta.url))).json();
const examples = catalog.examples;

function create_card(example, index) {
  const card = document.createElement("article");
  card.className = "general-chart-card";
  card.dataset.category = example.category;
  card.dataset.chartIndex = String(index);
  card.setAttribute("aria-label", example.title);
  const host = document.createElement("div");
  host.className = "general-chart-host";
  host.setAttribute("role", "group");
  host.setAttribute("aria-label", `${example.title} chart`);
  card.appendChild(host);
  return card;
}

async function mount_example(example, card) {
  const host = card.querySelector(".general-chart-host");
  const grid = example.grid !== false;
  const chart = await create_chart(host, {
    autoSize: true,
    // Auto tries WebGPU first. Unsupported browsers and machines without an available adapter
    // must still render the same chart through the engine's Canvas2D fallback.
    backend: requested_backend,
    theme: active_theme,
    initialPane: { horizontal_domain: example.horizontal_domain },
    grid: { vertLines: { visible: grid }, horzLines: { visible: grid } },
    generalLegend: { visible: true },
    // This page is a vertically scrolling dashboard. Wheel/pinch gestures belong to the page,
    // otherwise every chart card traps the user's scroll and makes the workspace feel frozen.
    handle_scroll: {
      pressed_mouse_move: false,
      mouse_wheel: false,
      horz_touch_drag: false,
      vert_touch_drag: false,
    },
    handle_scale: {
      mouse_wheel: false,
      pinch: false,
      axis_double_click_reset: false,
      axis_pressed_mouse_move: false,
    },
    kinetic_scroll: false,
  });
  const pane = chart.panes()[0];
  const pane_index = pane.pane_index();
  for (const axis of example.axes ?? []) chart.add_axis({ ...axis, pane: pane_index });

  const series_handles = [];
  for (const { kind, data, ...options } of example.series ?? []) {
    const handle = chart.add_series(kind, { x_axis_id: "x", y_axis_id: "y", ...options, pane: pane_index });
    handle.set_data(data);
    series_handles.push(handle);
  }
  for (const { kind, data, ...options } of example.layout_series ?? []) {
    series_handles.push(chart.add_layout_series(kind, { ...options, pane: pane_index }, data));
  }
  for (const reference of example.references ?? []) {
    chart.add_general_reference({ ...reference, pane: pane_index });
  }
  chart.render();

  return {
    chart,
    pane,
    series: series_handles,
    card,
    example,
    dispose() {
      chart.remove();
    },
  };
}
let errors = [];
let states = [];
let observer = null;
let dashboard_active = false;

function update_runtime_status() {
  const active = states.flatMap((state) => state.entry === null ? [] : [state.entry]);
  const backend_counts = new Map();
  for (const entry of active) backend_counts.set(entry.chart.backend(), (backend_counts.get(entry.chart.backend()) ?? 0) + 1);
  const fallback = active
    .map((entry) => entry.chart.backend_status())
    .find((status) => status.requested_backend === "auto" && status.active_backend === "canvas2d");
  const summary = [...backend_counts.entries()]
    .map(([backend, count]) => `${count} × ${backend === "webgpu" ? "WebGPU" : "Canvas2D"}`)
    .join(" · ");
  runtime_badge.title = fallback?.detail ?? "";
  runtime_badge.textContent = summary === ""
    ? `${requested_backend === "auto" ? "WebGPU preferred" : "Canvas2D requested"} · waiting for viewport`
    : `${summary} live${fallback ? ` · WebGPU fallback (${fallback.reason})` : ""}`;
}

function update_error_status() {
  if (errors.length === 0) {
    dashboard_error.dataset.visible = "false";
    dashboard_error.textContent = "";
    return;
  }
  dashboard_error.textContent = `${errors.length} chart${errors.length === 1 ? "" : "s"} failed to initialize: ${errors.map((error) => error.title).join(", ")}`;
  dashboard_error.dataset.visible = "true";
}

async function mount_state(state) {
  if (!dashboard_active || !state.desired || state.entry !== null || state.mounting !== null || state.failed || state.card.hidden) return;
  state.mounting = mount_example(state.example, state.card);
  try {
    const entry = await state.mounting;
    if (!dashboard_active || !state.desired || state.card.hidden) {
      entry.dispose();
      state.card.dataset.mounted = "false";
    } else {
      state.entry = entry;
      state.card.dataset.mounted = "true";
    }
  } catch (error) {
    console.error(`General dashboard failed to mount ${state.example.title}`, error);
    state.failed = true;
    state.card.dataset.mounted = "failed";
    errors.push({ title: state.example.title, message: error?.message ?? String(error) });
    update_error_status();
  } finally {
    state.mounting = null;
    update_runtime_status();
  }
}

// Charts mount lazily as they approach the viewport and then stay mounted while the dashboard is
// active. Unmounting on scroll would blank every card the user scrolls back to until its chart
// re-initializes; only leaving the dashboard or filtering a card out releases its chart.
function reconcile_active_charts() {
  if (!dashboard_active) return;
  for (const state of states) {
    if (state.card.hidden || state.failed) {
      state.desired = false;
      unmount_state(state);
    } else if (state.visible) {
      state.desired = true;
      void mount_state(state);
    }
  }
}

function unmount_state(state) {
  if (state.entry === null) return;
  state.entry.dispose();
  state.entry = null;
  state.card.dataset.mounted = "false";
  update_runtime_status();
}

function ensure_observer() {
  if (observer !== null) return;
  observer = new IntersectionObserver((entries) => {
    for (const observed of entries) {
      const state = states[Number(observed.target.dataset.chartIndex)];
      if (state === undefined) continue;
      state.visible = observed.isIntersecting;
    }
    reconcile_active_charts();
  }, { root: dashboard_root, rootMargin: "400px 0px" });
}

function set_active(active) {
  dashboard_active = active;
  ensure_observer();
  if (active) {
    for (const state of states) observer.observe(state.card);
  } else {
    observer.disconnect();
    for (const state of states) {
      state.desired = false;
      state.visible = false;
      unmount_state(state);
    }
  }
  update_runtime_status();
}

function set_theme(theme) {
  active_theme = theme;
  for (const state of states) state.entry?.chart.apply_options({ theme });
}

export function install_general_dashboard({ root, backend = "auto", theme = "dark", active = false }) {
  if (!(root instanceof HTMLElement)) throw new Error("general dashboard requires a workspace root");
  if (window.__generalDashboard?.dispose instanceof Function) window.__generalDashboard.dispose();

  dashboard_root = root;
  chart_grid = root.querySelector("#general_chart_grid");
  runtime_badge = root.querySelector("#general_runtime_badge");
  dashboard_error = root.querySelector("#general_dashboard_error");
  requested_backend = backend === "canvas2d" ? "canvas2d" : "auto";
  active_theme = theme;
  errors = [];
  states = [];
  observer = null;
  dashboard_active = false;
  chart_grid.replaceChildren();

  states = examples.map((example, index) => {
    const card = create_card(example, index);
    card.dataset.mounted = "false";
    chart_grid.appendChild(card);
    return {
      example,
      card,
      entry: null,
      mounting: null,
      failed: false,
      visible: false,
      desired: false,
      index,
    };
  });

  const all_button = root.querySelector('[data-general-filter="all"]');
  for (const peer of root.querySelectorAll("[data-general-filter]")) {
    if (peer !== all_button) peer.remove();
  }
  all_button.after(...catalog.categories.map(({ id, label }) => {
    const button = document.createElement("button");
    button.type = "button";
    button.dataset.generalFilter = id;
    button.setAttribute("aria-pressed", "false");
    button.textContent = label;
    return button;
  }));
  for (const button of root.querySelectorAll("[data-general-filter]")) {
    button.addEventListener("click", () => {
      const filter = button.dataset.generalFilter;
      for (const peer of root.querySelectorAll("[data-general-filter]")) {
        peer.setAttribute("aria-pressed", String(peer === button));
      }
      for (const state of states) {
        state.card.hidden = filter !== "all" && state.card.dataset.category !== filter;
        if (state.card.hidden) state.visible = false;
      }
      reconcile_active_charts();
    });
  }

  const controller = {
    ready: true,
    errors,
    summary: examples.map((example) => ({
      id: example.id,
      title: example.title,
      category: example.category,
      series_kinds: [...(example.series ?? []), ...(example.layout_series ?? [])].map((series) => series.kind),
    })),
    active_summary() {
      return states.flatMap((state) => state.entry === null ? [] : [{
        title: state.example.title,
        backend: state.entry.chart.backend(),
        backend_status: state.entry.chart.backend_status(),
        pane_count: state.entry.chart.panes().length,
        pane_index: state.entry.pane.pane_index(),
        series_kinds: state.entry.series.map((handle) => handle.kind),
      }]);
    },
    set_active,
    set_theme,
    dispose() {
      set_active(false);
      observer?.disconnect();
      observer = null;
      chart_grid.replaceChildren();
    },
  };
  window.__generalDashboard = controller;
  set_active(active);
  window.dispatchEvent(new CustomEvent("general-dashboard-ready"));
  return controller;
}
