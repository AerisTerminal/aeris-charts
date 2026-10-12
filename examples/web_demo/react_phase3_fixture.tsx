import React, { StrictMode } from "react";
import { createRoot, hydrateRoot } from "react-dom/client";
import { renderToString } from "react-dom/server";

import {
  FinancialSeries,
  GeneralAxis,
  GeneralBrushControls,
  GeneralDataLabels,
  GeneralLegend,
  GeneralPane,
  GeneralReference,
  GeneralSeries,
  GeneralTooltip,
  MappedGeneralSeries,
  AerisChart,
  type GeneralAxisSpec,
  type GeneralSeriesSpec,
} from "../../packages/charts/dist/react.js";
import type { chart_api, general_brush_range, general_reference_api, general_series_api, pane_api, series_api } from "../../packages/charts/dist/types.js";

const axes: readonly GeneralAxisSpec[] = [
  { id: "month", dimension: "x", position: "bottom", scale: "band" },
  { id: "revenue", dimension: "y", position: "left", scale: "linear" },
  { id: "revenue-alt", dimension: "y", position: "right", scale: "linear" },
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
  let forecast: general_series_api | null = null;
  let pane: pane_api | null = null;

  const render = (
    financial_close: number,
    revenue_value: number,
    show_children = true,
    title = "Revenue",
    use_alternate_axis = false,
    reverse_general_order = false,
  ) => {
    const render_axes = axes.map((axis) => axis.id === "revenue"
      ? { ...axis, title: title === "Revenue" ? "Revenue axis" : "Updated revenue axis" }
      : axis);
    const general_series: readonly GeneralSeriesSpec[] = [{
      key: "revenue",
      kind: "column",
      options: {
        x_axis_id: "month",
        y_axis_id: use_alternate_axis ? "revenue-alt" : "revenue",
        title,
        color: title === "Revenue" ? "#2563eb" : "#dc2626",
      },
      data: [{ id: "jan", x: "Jan", y: revenue_value }],
    }, {
      key: "forecast",
      kind: "xy_line",
      options: { x_axis_id: "month", y_axis_id: "revenue", title: "Forecast", color: "#16a34a" },
      data: [{ id: "jan-forecast", x: "Jan", y: revenue_value + 5 }],
    }];
    const ordered_general_series = reverse_general_order
      ? [...general_series].reverse()
      : general_series;
    root.render(
      <StrictMode>
        <AerisChart
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
              series={ordered_general_series}
              onPaneReady={(value) => { pane = value; }}
              onSeriesReady={(key, value) => {
                if (key === "revenue") general = value;
                if (key === "forecast") forecast = value;
              }}
            />
          </> : null}
        </AerisChart>
      </StrictMode>,
    );
  };

  render(2, 42);
  try {
    await wait_until(() => chart !== null && financial !== null && general !== null && forecast !== null && pane !== null
      && safely(() => (chart as chart_api).backend().length > 0)
      && safely(() => (financial as series_api).data()[0]?.close === 2)
      && safely(() => (general as general_series_api).dataAt(0)?.value === 42)
      && safely(() => (pane as pane_api).paneIndex() >= 0));
  } catch {
    console.error = original_console_error;
    window.removeEventListener("error", on_window_error);
    throw new Error(
      `React fixture readiness: chart=${chart !== null} financial=${financial !== null} general=${general !== null} forecast=${forecast !== null} pane=${pane !== null} children=${host.childElementCount} errors=${react_errors.join(" | ")} window=${window_errors.join(" | ")}`,
    );
  }
  const first_chart = chart as chart_api;
  const first_financial = financial as series_api;
  const first_general = general as general_series_api;
  const first_forecast = forecast as general_series_api;
  const first_pane = pane as pane_api;
  const financial_id = first_financial.id;
  const general_id = first_general.id;
  const pane_index = first_pane.paneIndex();

  render(3, 57, true, "Updated revenue", true, true);
  await wait_until(() => safely(() => (financial as series_api).data()[0]?.close === 3));
  await wait_until(() => safely(() => (general as general_series_api).dataAt(0)?.value === 57));
  await wait_until(() => safely(() => (general as general_series_api).options().title === "Updated revenue"));
  await wait_until(() => safely(() => (general as general_series_api).options().y_axis_id === "revenue-alt"));
  await wait_until(() => safely(() => first_chart.general_series_order(pane_index)[0] === first_forecast));
  await wait_until(() => safely(() => first_chart.axis("revenue")?.options().title === "Updated revenue axis"));

  const result = {
    same_chart: chart === first_chart,
    same_financial_handle: financial === first_financial,
    same_general_handle: general === first_general,
    same_forecast_handle: forecast === first_forecast,
    same_financial_id: (financial as series_api).id === financial_id,
    same_general_id: (general as general_series_api).id === general_id,
    updated_general_title: (general as general_series_api).options().title,
    updated_general_color: (general as general_series_api).options().color,
    updated_general_axis: (general as general_series_api).options().y_axis_id,
    updated_general_order: (
      first_chart.general_series_order(pane_index)[0] === first_forecast
      && first_chart.general_series_order(pane_index)[1] === first_general
    ),
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
  const general_pane_stale = !safely(() => first_pane.pane_index() >= 0);

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
  return { ...result, child_cleanup, general_pane_stale, disposed };
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
      <AerisChart
        options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
        onChartReady={(value) => { chart = value; }}
      >
        <FailureBoundary onFailure={(error) => { failure = error; }}>
          <GeneralPane options={pane_options} axes={axes} series={invalid_series} />
        </FailureBoundary>
      </AerisChart>,
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

/** Prove a readiness callback failure releases committed child resources and permits a valid retry. */
export async function exerciseReactCallbackFailureCleanup(): Promise<Record<string, unknown>> {
  const host = document.createElement("div");
  host.style.cssText = "width:720px;height:480px";
  document.body.appendChild(host);
  const root = createRoot(host);
  let chart: chart_api | null = null;
  let failure: Error | null = null;
  let retried: general_series_api | null = null;
  const valid_series: readonly GeneralSeriesSpec[] = [{
    key: "valid",
    kind: "column",
    options: { x_axis_id: "month", y_axis_id: "revenue" },
    data: [{ id: "jan", x: "Jan", y: 42 }],
  }];
  const original_console_error = console.error;
  console.error = () => {};
  try {
    root.render(
      <AerisChart
        options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
        onChartReady={(value) => { chart = value; }}
      >
        <FailureBoundary key="failed" onFailure={(error) => { failure = error; }}>
          <GeneralPane
            options={pane_options}
            axes={axes}
            series={valid_series}
            onSeriesReady={() => { throw new Error("expected readiness callback failure"); }}
          />
        </FailureBoundary>
      </AerisChart>,
    );
    await wait_until(() => chart !== null && failure !== null);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const live = chart as chart_api;
    const after_failure = {
      pane_count: live.panes().length,
      axis_count: live.axes().length,
      legend_count: live.general_legend_snapshot().items.length,
    };

    root.render(
      <AerisChart
        options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
        onChartReady={(value) => { chart = value; }}
      >
        <FailureBoundary key="retry" onFailure={(error) => { failure = error; }}>
          <GeneralPane
            options={pane_options}
            axes={axes}
            series={valid_series}
            onSeriesReady={(_key, value) => { retried = value; }}
          />
        </FailureBoundary>
      </AerisChart>,
    );
    await wait_until(() => retried !== null && safely(() => (retried as general_series_api).dataAt(0)?.value === 42));
    const after_retry = {
      pane_count: live.panes().length,
      axis_count: live.axes().length,
      legend_count: live.general_legend_snapshot().items.length,
    };
    root.unmount();
    await new Promise((resolve) => setTimeout(resolve, 0));
    let disposed = false;
    try {
      live.backend();
    } catch {
      disposed = true;
    }
    return {
      failure: (failure as Error).message,
      after_failure,
      after_retry,
      disposed,
    };
  } finally {
    console.error = original_console_error;
    root.unmount();
    host.remove();
  }
}

/** Exercise composable declarations and record mapping through the real browser chart. */
export async function exerciseReactDeclarations(): Promise<Record<string, unknown>> {
  const host = document.createElement("div");
  host.style.cssText = "width:720px;height:480px";
  document.body.appendChild(host);
  const root = createRoot(host);
  let chart: chart_api | null = null;
  let series: general_series_api | null = null;
  let map_calls = 0;
  const original = [{ month: "Jan", sales: 12 }, { month: "Feb", sales: 19 }];
  const changed = [{ month: "Jan", sales: 21 }, { month: "Feb", sales: 27 }];
  const mapRow = (row: (typeof original)[number]) => {
    map_calls += 1;
    return { id: row.month, x: row.month, y: row.sales };
  };
  const render = (rows: typeof original, title: string) => root.render(
    <StrictMode>
      <AerisChart options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
        onChartReady={(value) => { chart = value; }}>
        <GeneralPane options={pane_options} onSeriesReady={(_key, value) => { series = value; }}>
          <GeneralAxis id="month" dimension="x" position="bottom" scale="band" />
          <GeneralAxis id="sales" dimension="y" position="left" scale="linear" />
          <MappedGeneralSeries seriesKey="sales" kind="column"
            options={{ x_axis_id: "month", y_axis_id: "sales", title }}
            data={rows} mapRow={mapRow} />
        </GeneralPane>
      </AerisChart>
    </StrictMode>,
  );
  try {
    render(original, "Sales");
    await wait_until(() => series !== null && safely(() => (series as general_series_api).dataAt(1)?.value === 19));
    const first_chart = chart;
    const first_series = series;
    const initial_calls = map_calls;
    render(original, "Updated sales");
    await wait_until(() => safely(() => (series as general_series_api).options().title === "Updated sales"));
    const retained_calls = map_calls;
    render(changed, "Updated sales");
    await wait_until(() => safely(() => (series as general_series_api).dataAt(1)?.value === 27));
    const result = {
      same_chart: chart === first_chart,
      same_series: series === first_series,
      initial_calls,
      retained_calls,
      changed_calls: map_calls,
      axis_count: (chart as chart_api | null)?.axes().length,
      data_value: (series as general_series_api | null)?.dataAt(1)?.value,
    };
    root.unmount();
    return result;
  } finally {
    root.unmount();
    host.remove();
  }
}

/** References and default chrome share the same pane lifecycle as declarative series. */
export async function exerciseReactComponents(): Promise<Record<string, unknown>> {
  const host = document.createElement("div");
  host.style.cssText = "width:720px;height:480px";
  document.body.appendChild(host);
  const root = createRoot(host);
  let chart: chart_api | null = null;
  let series: general_series_api | null = null;
  let reference: general_reference_api | null = null;
  const rows = [{ id: "jan", x: "Jan", y: 12 }, { id: "feb", x: "Feb", y: 19 }];
  const render = (value: number) => root.render(
    <StrictMode>
      <AerisChart options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
        onChartReady={(created) => { chart = created; }}>
        <GeneralPane options={pane_options}
          onSeriesReady={(_key, created) => { series = created; }}
          onReferenceReady={(_key, created) => { reference = created; }}>
          <GeneralAxis id="month" dimension="x" position="bottom" scale="band" />
          <GeneralAxis id="sales" dimension="y" position="left" scale="linear" />
          <GeneralSeries seriesKey="sales" kind="column"
            options={{ x_axis_id: "month", y_axis_id: "sales", title: "Sales" }} data={rows} />
          <GeneralDataLabels seriesKey="sales" />
          <GeneralReference referenceKey="target"
            options={{ kind: "line", axis_id: "sales", value, color: "#f00" }} />
          <GeneralLegend />
          <GeneralTooltip />
          <GeneralBrushControls options={{ axis: "month" }} />
        </GeneralPane>
      </AerisChart>
    </StrictMode>,
  );
  try {
    render(14);
    await wait_until(() => chart !== null && series !== null && reference !== null
      && host.querySelector('[role="group"][aria-label="Chart legend"]') !== null
      && host.querySelector('[role="group"][aria-label="Chart range brush"]') !== null
      && host.querySelector('[role="tooltip"]') !== null);
    const first_chart = chart as chart_api;
    const first_series = series as general_series_api;
    const first_reference = reference as general_reference_api;
    const brush = host.querySelector('[role="group"][aria-label="Chart range brush"]') as HTMLElement;
    (brush.querySelector("button") as HTMLButtonElement).click();
    const brushed_range = first_chart.general_brush_snapshot()?.range;
    const legend = host.querySelector('[role="group"][aria-label="Chart legend"]') as HTMLElement;
    (legend.querySelector("button") as HTMLButtonElement).click();
    const toggled = first_series.options().visible === false;
    render(16);
    await wait_until(() => safely(() => (reference as general_reference_api).options().kind === "line"
      && (reference as general_reference_api).options().value === 16));
    const result = {
      same_chart: chart === first_chart,
      same_series: series === first_series,
      same_reference: reference === first_reference,
      reference_id: first_reference.id,
      reference_count: first_chart.general_references().length,
      reference_value: first_reference.options().kind === "line" ? first_reference.options().value : null,
      toggled,
      labels: first_series.options().data_labels,
      brush: brushed_range,
      tooltip_count: host.querySelectorAll('[role="tooltip"]').length,
    };
    root.unmount();
    return result;
  } finally {
    root.unmount();
    host.remove();
  }
}

export async function exerciseReactControlledBrush(): Promise<Record<string, unknown>> {
  const host = document.createElement("div");
  host.style.cssText = "width:720px;height:480px";
  document.body.appendChild(host);
  const root = createRoot(host);
  let chart: chart_api | null = null;
  const changes: (general_brush_range | null)[] = [];
  const rows = [{ id: "jan", x: "Jan", y: 12 }, { id: "feb", x: "Feb", y: 19 }];
  const render = (range: general_brush_range | null | undefined) => root.render(
    <AerisChart options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
      onChartReady={(created) => { chart = created; }}>
      <GeneralPane options={pane_options}>
        <GeneralAxis id="month" dimension="x" position="bottom" scale="band" />
        <GeneralAxis id="sales" dimension="y" position="left" scale="linear" />
        <GeneralSeries seriesKey="sales" kind="column"
          options={{ x_axis_id: "month", y_axis_id: "sales" }} data={rows} />
        <GeneralBrushControls options={{ axis: "month" }} range={range}
          onChange={(snapshot) => { changes.push(snapshot?.range ?? null); }} />
      </GeneralPane>
    </AerisChart>,
  );
  try {
    render({ type: "category", from: "Feb", to: "Feb" });
    await wait_until(() => chart !== null
      && safely(() => (chart as chart_api).general_brush_snapshot()?.range.type === "category")
      && host.querySelector('[role="group"][aria-label="Chart range brush"]') !== null);
    const live = chart as chart_api;
    const first = live.general_brush_snapshot()?.range;
    const brush = host.querySelector('[role="group"][aria-label="Chart range brush"]') as HTMLElement;
    (brush.querySelector("button") as HTMLButtonElement).click();
    const callback = changes[0];
    const held = live.general_brush_snapshot()?.range;
    render({ type: "category", from: "Jan", to: "Feb" });
    await wait_until(() => {
      const current = live.general_brush_snapshot()?.range;
      return current?.type === "category" && current.from === "Jan";
    });
    const updated = live.general_brush_snapshot()?.range;
    render(undefined);
    await new Promise((resolve) => requestAnimationFrame(resolve));
    const clear = Array.from(brush.querySelectorAll("button")).find((button) => button.textContent === "Clear");
    (clear as HTMLButtonElement).click();
    const uncontrolled = live.general_brush_snapshot();
    return { first, callback, held, updated, uncontrolled, change_count: changes.length };
  } finally {
    root.unmount();
    host.remove();
  }
}

export async function exerciseReactChartReadyFailure(): Promise<Record<string, unknown>> {
  const host = document.createElement("div");
  host.style.cssText = "width:720px;height:480px";
  document.body.appendChild(host);
  const root = createRoot(host);
  let failed_chart: chart_api | null = null;
  let retried_chart: chart_api | null = null;
  let failure: Error | null = null;
  const original_console_error = console.error;
  console.error = () => {};
  try {
    root.render(
      <FailureBoundary key="failed" onFailure={(error) => { failure = error; }}>
        <AerisChart options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
          onChartReady={(created) => {
            failed_chart = created;
            throw new Error("expected chart readiness failure");
          }} />
      </FailureBoundary>,
    );
    await wait_until(() => failure !== null && failed_chart !== null);
    const stale = !safely(() => (failed_chart as chart_api).backend().length > 0);
    root.render(
      <FailureBoundary key="retry" onFailure={(error) => { failure = error; }}>
        <AerisChart options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
          onChartReady={(created) => { retried_chart = created; }} />
      </FailureBoundary>,
    );
    await wait_until(() => retried_chart !== null && safely(() => (retried_chart as chart_api).backend().length > 0));
    const result = { failure: (failure as Error).message, stale, retry_live: retried_chart !== failed_chart };
    root.unmount();
    return result;
  } finally {
    console.error = original_console_error;
    root.unmount();
    host.remove();
  }
}

export async function exerciseReactFinancialFailure(): Promise<Record<string, unknown>> {
  const host = document.createElement("div");
  host.style.cssText = "width:720px;height:480px";
  document.body.appendChild(host);
  const root = createRoot(host);
  let chart: chart_api | null = null;
  let failure: Error | null = null;
  let retried: series_api | null = null;
  const original_console_error = console.error;
  console.error = () => {};
  try {
    root.render(
      <AerisChart options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
        onChartReady={(created) => { chart = created; }}>
        <FailureBoundary key="failed" onFailure={(error) => { failure = error; }}>
          <FinancialSeries kind="candlestick"
            data={[{ time: 1735689600, open: 1, high: 3, low: 1, close: 2 }]}
            onSeriesReady={() => { throw new Error("expected financial readiness failure"); }} />
        </FailureBoundary>
      </AerisChart>,
    );
    await wait_until(() => chart !== null && failure !== null);
    const after_failure = (chart as chart_api).series_order().length;
    root.render(
      <AerisChart options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
        onChartReady={(created) => { chart = created; }}>
        <FailureBoundary key="retry" onFailure={(error) => { failure = error; }}>
          <FinancialSeries kind="candlestick"
            data={[{ time: 1735689600, open: 1, high: 3, low: 1, close: 2 }]}
            onSeriesReady={(created) => { retried = created; }} />
        </FailureBoundary>
      </AerisChart>,
    );
    await wait_until(() => retried !== null && safely(() => (retried as series_api).data()[0]?.close === 2));
    const result = {
      failure: (failure as Error).message,
      after_failure,
      after_retry: (chart as chart_api).series_order().length,
      retry_live: (chart as chart_api).series_order()[0] === retried,
    };
    root.unmount();
    return result;
  } finally {
    console.error = original_console_error;
    root.unmount();
    host.remove();
  }
}

export async function exerciseReactHydration(): Promise<Record<string, unknown>> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  let chart: chart_api | null = null;
  let series: general_series_api | null = null;
  const errors: string[] = [];
  const original_console_error = console.error;
  console.error = (...parts: unknown[]) => {
    errors.push(parts.map(String).join(" "));
    original_console_error(...parts);
  };
  const element = (value: number) => (
    <AerisChart options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
      style={{ width: 720, height: 480 }} onChartReady={(created) => { chart = created; }}>
      <GeneralPane options={pane_options} onSeriesReady={(_key, created) => { series = created; }}>
        <GeneralAxis id="month" dimension="x" scale="band" />
        <GeneralAxis id="sales" dimension="y" scale="linear" />
        <GeneralSeries seriesKey="sales" kind="column"
          options={{ x_axis_id: "month", y_axis_id: "sales" }}
          data={[{ id: "jan", x: "Jan", y: value }]} />
      </GeneralPane>
    </AerisChart>
  );
  const markup = renderToString(element(12));
  host.innerHTML = markup;
  const root = hydrateRoot(host, element(12));
  try {
    await wait_until(() => chart !== null && series !== null
      && safely(() => (series as general_series_api).dataAt(0)?.value === 12));
    const first_chart = chart;
    const first_series = series;
    root.render(element(18));
    root.render(element(27));
    await wait_until(() => safely(() => (series as general_series_api).dataAt(0)?.value === 27));
    return {
      server_has_canvas: markup.includes("canvas"),
      same_chart: chart === first_chart,
      same_series: series === first_series,
      value: (series as general_series_api).dataAt(0)?.value,
      errors,
    };
  } finally {
    root.unmount();
    console.error = original_console_error;
    host.remove();
  }
}

export async function exerciseReactAxisPreflight(): Promise<Record<string, unknown>> {
  const host = document.createElement("div");
  host.style.cssText = "width:720px;height:480px";
  document.body.appendChild(host);
  const root = createRoot(host);
  let chart: chart_api | null = null;
  let series: general_series_api | null = null;
  let validation_error: string | null = null;
  const rows = [{ id: "jan", x: "Jan", y: 12 }];
  const render = (x_scale: "band" | "linear", title: string) => root.render(
    <AerisChart options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
      onChartReady={(created) => { chart = created; }}>
      <GeneralPane options={pane_options}
        onSeriesReady={(_key, created) => { series = created; }}
        onValidationError={(error) => { validation_error = String(error); }}>
        <GeneralAxis id="month" dimension="x" scale={x_scale} />
        <GeneralAxis id="sales" dimension="y" scale="linear" />
        <GeneralSeries seriesKey="sales" kind="column"
          options={{ x_axis_id: "month", y_axis_id: "sales", title }} data={rows} />
      </GeneralPane>
    </AerisChart>,
  );
  try {
    render("band", "Sales");
    await wait_until(() => chart !== null && series !== null
      && safely(() => (series as general_series_api).dataAt(0)?.value === 12));
    const first_chart = chart as chart_api;
    const first_series = series as general_series_api;
    const first_axis = first_chart.axis("month");
    render("linear", "Should not apply");
    await wait_until(() => validation_error !== null);
    const after_invalid = {
      same_axis: first_axis !== null && safely(() => first_axis.options().scale === "band"),
      same_series: first_chart.general_series_order()[0] === first_series,
      axis_scale: first_chart.axis("month")?.options().scale,
      series_title: first_series.options().title,
      pane_count: first_chart.panes().length,
    };
    render("band", "Updated sales");
    await wait_until(() => first_series.options().title === "Updated sales");
    return {
      error: validation_error,
      after_invalid,
      recovered: chart === first_chart && series === first_series,
    };
  } finally {
    root.unmount();
    host.remove();
  }
}

export async function exerciseReactReferencePreflight(): Promise<Record<string, unknown>> {
  const host = document.createElement("div");
  host.style.cssText = "width:720px;height:480px";
  document.body.appendChild(host);
  const root = createRoot(host);
  let chart: chart_api | null = null;
  let series: general_series_api | null = null;
  let reference: general_reference_api | null = null;
  let validation_error: string | null = null;
  const rows = [{ id: "jan", x: "Jan", y: 12 }];
  const render = (value: number | string, title: string) => root.render(
    <AerisChart options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
      onChartReady={(created) => { chart = created; }}>
      <GeneralPane options={pane_options}
        onSeriesReady={(_key, created) => { series = created; }}
        onReferenceReady={(_key, created) => { reference = created; }}
        onValidationError={(error) => { validation_error = String(error); }}>
        <GeneralAxis id="month" dimension="x" scale="band" />
        <GeneralAxis id="sales" dimension="y" scale="linear" />
        <GeneralSeries seriesKey="sales" kind="column"
          options={{ x_axis_id: "month", y_axis_id: "sales", title }} data={rows} />
        <GeneralReference referenceKey="target" options={{ kind: "line", axis_id: "sales", value }} />
      </GeneralPane>
    </AerisChart>,
  );
  try {
    render(14, "Sales");
    await wait_until(() => chart !== null && series !== null && reference !== null);
    const first_series = series as general_series_api;
    const first_reference = reference as general_reference_api;
    render("wrong scale", "Should not apply");
    await wait_until(() => validation_error !== null);
    const retained_options = first_reference.options();
    const after_invalid = {
      series_title: first_series.options().title,
      reference_value: retained_options.kind === "line" ? retained_options.value : null,
      reference_count: (chart as chart_api).general_references().length,
    };
    render(16, "Updated sales");
    await wait_until(() => safely(() => {
      const current = first_reference.options();
      return first_series.options().title === "Updated sales"
        && current.kind === "line" && current.value === 16;
    }));
    return {
      error: validation_error,
      after_invalid,
      same_series: series === first_series,
      same_reference: reference === first_reference,
    };
  } finally {
    root.unmount();
    host.remove();
  }
}

export async function exerciseReactSeriesPreflight(): Promise<Record<string, unknown>> {
  const host = document.createElement("div");
  host.style.cssText = "width:720px;height:480px";
  document.body.appendChild(host);
  const root = createRoot(host);
  let chart: chart_api | null = null;
  let series: general_series_api | null = null;
  let validation_error: string | null = null;
  const rows = [{ id: "jan", x: "Jan", y: 12 }];
  const render = (kind: "column" | "range_bar" | "scatter", title: string,
    data: readonly unknown[] = rows) => root.render(
    <AerisChart options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
      onChartReady={(created) => { chart = created; }}>
      <GeneralPane options={pane_options}
        onSeriesReady={(_key, created) => { series = created; }}
        onValidationError={(error) => { validation_error = String(error); }}>
        <GeneralAxis id="month" dimension="x" scale="band" />
        <GeneralAxis id="sales" dimension="y" scale="linear" />
        <GeneralSeries seriesKey="sales" kind={kind}
          options={{ x_axis_id: "month", y_axis_id: "sales", title }}
          data={data as never} />
      </GeneralPane>
    </AerisChart>,
  );
  try {
    render("column", "Sales");
    await wait_until(() => chart !== null && series !== null
      && safely(() => (series as general_series_api).dataAt(0)?.value === 12));
    const first_chart = chart as chart_api;
    const first_series = series as general_series_api;
    render("range_bar", "Should not apply");
    await wait_until(() => validation_error !== null);
    const retained = {
      same_series: first_chart.general_series_order()[0] === first_series,
      kind: first_series.kind,
      value: first_series.dataAt(0)?.value,
    };
    const row_error = validation_error;
    validation_error = null;
    render("scatter", "Incompatible axes", [{ id: "jan", x: 1, y: 12 }]);
    await wait_until(() => validation_error !== null);
    const after_axis_error = {
      same_series: first_chart.general_series_order()[0] === first_series,
      kind: first_series.kind,
      value: first_series.dataAt(0)?.value,
    };
    render("column", "Updated sales");
    await wait_until(() => first_series.options().title === "Updated sales");
    const recovered = series === first_series;
    render("range_bar", "Range", [{ id: "jan", x: "Jan", low: 5, high: 15 }]);
    await wait_until(() => series !== first_series && series?.kind === "range_bar");
    const valid_transition = {
      replaced: series !== first_series,
      old_stale: !safely(() => first_series.options().title.length > 0),
      new_kind: series?.kind,
      new_value: series?.dataAt(0)?.value,
    };
    return { row_error, axis_error: validation_error, retained, after_axis_error,
      recovered, valid_transition };
  } finally {
    root.unmount();
    host.remove();
  }
}

export async function exerciseReactReferenceAxisPreflight(): Promise<Record<string, unknown>> {
  const host = document.createElement("div");
  host.style.cssText = "width:720px;height:480px";
  document.body.appendChild(host);
  const root = createRoot(host);
  let chart: chart_api | null = null;
  let series: general_series_api | null = null;
  let reference: general_reference_api | null = null;
  let validation_error: string | null = null;
  const rows = [{ id: "jan", x: "Jan", y: 12 }];
  const render = (scale: "linear" | "log", value: number) => root.render(
    <AerisChart options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
      onChartReady={(created) => { chart = created; }}>
      <GeneralPane options={pane_options}
        onSeriesReady={(_key, created) => { series = created; }}
        onReferenceReady={(_key, created) => { reference = created; }}
        onValidationError={(error) => { validation_error = String(error); }}>
        <GeneralAxis id="month" dimension="x" scale="band" />
        <GeneralAxis id="sales" dimension="y" scale={scale} />
        <GeneralSeries seriesKey="sales" kind="xy_line"
          options={{ x_axis_id: "month", y_axis_id: "sales", title: "Sales" }} data={rows} />
        <GeneralReference referenceKey="target" options={{ kind: "line", axis_id: "sales", value }} />
      </GeneralPane>
    </AerisChart>,
  );
  try {
    render("linear", 0);
    await wait_until(() => chart !== null && series !== null && reference !== null);
    const first_chart = chart as chart_api;
    const first_series = series as general_series_api;
    const first_reference = reference as general_reference_api;
    render("log", 0);
    await wait_until(() => validation_error !== null);
    const retained = {
      same_series: first_chart.general_series_order()[0] === first_series,
      same_reference: first_chart.general_references()[0]?.id === first_reference.id,
      axis_scale: first_chart.axis("sales")?.options().scale,
    };
    render("log", 20);
    await wait_until(() => first_chart.axis("sales")?.options().scale === "log"
      && series !== first_series && reference !== first_reference);
    return { error: validation_error, retained, applied: {
      axis_scale: first_chart.axis("sales")?.options().scale,
      reference_value: reference?.options().kind === "line" ? reference.options().value : null,
      series_replaced: series !== first_series,
    } };
  } finally {
    root.unmount();
    host.remove();
  }
}

export async function exerciseReactLogDataPreflight(): Promise<Record<string, unknown>> {
  const host = document.createElement("div");
  host.style.cssText = "width:720px;height:480px";
  document.body.appendChild(host);
  const root = createRoot(host);
  let chart: chart_api | null = null;
  let series: general_series_api | null = null;
  let validation_error: string | null = null;
  const render = (scale: "linear" | "log", y: number) => root.render(
    <AerisChart options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
      onChartReady={(created) => { chart = created; }}>
      <GeneralPane options={{ horizontal_domain: { type: "continuous", scale: "linear" } }}
        onSeriesReady={(_key, created) => { series = created; }}
        onValidationError={(error) => { validation_error = String(error); }}>
        <GeneralAxis id="x" dimension="x" scale="linear" />
        <GeneralAxis id="y" dimension="y" scale={scale} />
        <GeneralSeries seriesKey="points" kind="scatter"
          options={{ x_axis_id: "x", y_axis_id: "y" }}
          data={[{ id: "sample", x: 1, y }]} />
      </GeneralPane>
    </AerisChart>,
  );
  try {
    render("linear", -5);
    await wait_until(() => chart !== null && series !== null
      && safely(() => (series as general_series_api).dataAt(0)?.value === -5));
    const first_chart = chart as chart_api;
    const first_series = series as general_series_api;
    render("log", -5);
    await wait_until(() => validation_error !== null);
    let imperative_error: string | null = null;
    try {
      const pane = first_series.options().pane;
      first_chart.validate_general_pane_proposal(
        pane,
        [
          first_chart.axis("x")!.options(),
          { ...first_chart.axis("y")!.options(), scale: "log" },
        ],
        [{
          kind: "scatter",
          options: first_series.options(),
          data: [{ id: "sample", x: 1, y: -5 }],
        }],
        [],
      );
    } catch (error) {
      imperative_error = String(error);
    }
    const retained = {
      same_series: first_chart.general_series_order()[0] === first_series,
      axis_scale: first_chart.axis("y")?.options().scale,
      value: first_series.dataAt(0)?.value,
    };
    render("log", 5);
    await wait_until(() => first_chart.axis("y")?.options().scale === "log"
      && series !== first_series);
    return { error: validation_error, imperative_error, retained, applied: {
      axis_scale: first_chart.axis("y")?.options().scale,
      value: series?.dataAt(0)?.value,
    } };
  } finally {
    root.unmount();
    host.remove();
  }
}
