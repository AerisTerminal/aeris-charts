import React, { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import {
  FinancialSeries,
  GeneralPane,
  NucleusChart,
  type GeneralAxisSpec,
  type GeneralSeriesSpec,
} from "../../packages/charts/dist/react.js";
import type { chart_api, general_series_api, pane_api, series_api } from "../../packages/charts/dist/types.js";

const axes: readonly GeneralAxisSpec[] = [
  { id: "month", dimension: "x", position: "bottom", scale: "band" },
  { id: "revenue", dimension: "y", position: "left", scale: "linear" },
];

const pane_options = { horizontal_domain: { type: "category" as const, scale: "band" as const } };

function wait_until(predicate: () => boolean, timeout_ms = 10_000): Promise<void> {
  const started = performance.now();
  return new Promise((resolve, reject) => {
    const poll = () => {
      if (predicate()) {
        resolve();
        return;
      }
      if (performance.now() - started > timeout_ms) {
        reject(new Error("timed out waiting for React chart fixture"));
        return;
      }
      requestAnimationFrame(poll);
    };
    poll();
  });
}

function safely(predicate: () => boolean): boolean {
  try {
    return predicate();
  } catch {
    return false;
  }
}

/** Browser-only Phase 3 evidence used by Playwright; not part of the published package. */
export async function exerciseReactAdapter(): Promise<Record<string, unknown>> {
  const react_errors: string[] = [];
  const window_errors: string[] = [];
  const original_console_error = console.error;
  const on_window_error = (event: ErrorEvent) => {
    window_errors.push(event.error instanceof Error ? event.error.message : event.message);
  };
  window.addEventListener("error", on_window_error);
  console.error = (...args: unknown[]) => {
    react_errors.push(args.map((value) => String(value)).join(" "));
    original_console_error(...args);
  };
  const host = document.createElement("div");
  host.style.width = "720px";
  host.style.height = "480px";
  document.body.appendChild(host);
  const root = createRoot(host);

  let chart: chart_api | null = null;
  let financial: series_api | null = null;
  let general: general_series_api | null = null;
  let pane: pane_api | null = null;

  const render = (
    financial_close: number,
    revenue_value: number,
    show_children = true,
    title = "Revenue",
  ) => {
    const render_axes = axes.map((axis) => axis.id === "revenue"
      ? { ...axis, title: title === "Revenue" ? "Revenue axis" : "Updated revenue axis" }
      : axis);
    const general_series: readonly GeneralSeriesSpec[] = [{
      key: "revenue",
      kind: "column",
      options: { x_axis_id: "month", y_axis_id: "revenue", title, color: title === "Revenue" ? "#2563eb" : "#dc2626" },
      data: [{ id: "jan", x: "Jan", y: revenue_value }],
    }];
    root.render(
      <StrictMode>
        <NucleusChart
          options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
          style={{ width: "720px", height: "480px" }}
          onChartReady={(value) => { chart = value; }}
        >
          {show_children ? <>
            <FinancialSeries
              kind="candlestick"
              data={[{ time: 1735689600, open: 1, high: 4, low: 1, close: financial_close }]}
              onSeriesReady={(value) => { financial = value; }}
            />
            <GeneralPane
              options={pane_options}
              axes={render_axes}
              series={general_series}
              onPaneReady={(value) => { pane = value; }}
              onSeriesReady={(_key, value) => { general = value; }}
            />
          </> : null}
        </NucleusChart>
      </StrictMode>,
    );
  };

  render(2, 42);
  try {
    await wait_until(() => chart !== null && financial !== null && general !== null && pane !== null
      && safely(() => (chart as chart_api).backend().length > 0)
      && safely(() => (financial as series_api).data()[0]?.close === 2)
      && safely(() => (general as general_series_api).dataAt(0)?.value === 42)
      && safely(() => (pane as pane_api).paneIndex() >= 0));
  } catch {
    console.error = original_console_error;
    window.removeEventListener("error", on_window_error);
    throw new Error(
      `React fixture readiness: chart=${chart !== null} financial=${financial !== null} general=${general !== null} pane=${pane !== null} children=${host.childElementCount} errors=${react_errors.join(" | ")} window=${window_errors.join(" | ")}`,
    );
  }
  const first_chart = chart as chart_api;
  const first_financial = financial as series_api;
  const first_general = general as general_series_api;
  const first_pane = pane as pane_api;
  const financial_id = first_financial.id;
  const general_id = first_general.id;
  const pane_index = first_pane.paneIndex();

  render(3, 57, true, "Updated revenue");
  await wait_until(() => safely(() => (financial as series_api).data()[0]?.close === 3));
  await wait_until(() => safely(() => (general as general_series_api).dataAt(0)?.value === 57));
  await wait_until(() => safely(() => (general as general_series_api).options().title === "Updated revenue"));
  await wait_until(() => safely(() => first_chart.axis("revenue")?.options().title === "Updated revenue axis"));

  const result = {
    same_chart: chart === first_chart,
    same_financial_handle: financial === first_financial,
    same_general_handle: general === first_general,
    same_financial_id: (financial as series_api).id === financial_id,
    same_general_id: (general as general_series_api).id === general_id,
    updated_general_title: (general as general_series_api).options().title,
    updated_general_color: (general as general_series_api).options().color,
    updated_axis_title: first_chart.axis("revenue")?.options().title,
    pane_index_stable: (pane as pane_api).paneIndex() === pane_index,
    pane_count: first_chart.panes().length,
    axis_count: first_chart.axes(pane_index).length,
    general_legend_count: first_chart.general_legend_snapshot(pane_index).items.length,
  };

  render(3, 57, false);
  await wait_until(() => first_chart.panes().length === 1);
  await wait_until(() => first_chart.axes().length === 0);
  await wait_until(() => first_chart.series_order().length === 0);
  const child_cleanup = first_chart.general_legend_snapshot().items.length === 0;

  root.unmount();
  await new Promise((resolve) => setTimeout(resolve, 0));
  let disposed = false;
  try {
    first_chart.backend();
  } catch {
    disposed = true;
  }
  host.remove();
  console.error = original_console_error;
  window.removeEventListener("error", on_window_error);
  return { ...result, child_cleanup, disposed };
}

class FailureBoundary extends React.Component<{
  children: React.ReactNode;
  onFailure: (error: Error) => void;
}, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError(): { failed: boolean } {
    return { failed: true };
  }

  componentDidCatch(error: Error): void {
    this.props.onFailure(error);
  }

  render(): React.ReactNode {
    return this.state.failed ? null : this.props.children;
  }
}

/** Prove a rejected first data install cannot strand an untracked engine series or pane. */
export async function exerciseReactFailureCleanup(): Promise<Record<string, unknown>> {
  const host = document.createElement("div");
  host.style.cssText = "width:720px;height:480px";
  document.body.appendChild(host);
  const root = createRoot(host);
  let chart: chart_api | null = null;
  let failure: Error | null = null;
  const invalid_series: readonly GeneralSeriesSpec[] = [{
    key: "invalid",
    kind: "column",
    options: { x_axis_id: "month", y_axis_id: "revenue" },
    data: [{ id: "bad", x: 7 as unknown as string, y: 42 }],
  }];
  const original_console_error = console.error;
  console.error = () => {};
  try {
    root.render(
      <NucleusChart
        options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
        onChartReady={(value) => { chart = value; }}
      >
        <FailureBoundary onFailure={(error) => { failure = error; }}>
          <GeneralPane options={pane_options} axes={axes} series={invalid_series} />
        </FailureBoundary>
      </NucleusChart>,
    );
    await wait_until(() => chart !== null && failure !== null);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const live = chart as chart_api;
    const result = {
      failure: (failure as Error).message,
      pane_count: live.panes().length,
      axis_count: live.axes().length,
      legend_count: live.general_legend_snapshot().items.length,
    };
    root.unmount();
    await new Promise((resolve) => setTimeout(resolve, 0));
    return result;
  } finally {
    console.error = original_console_error;
    root.unmount();
    host.remove();
  }
}
