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
import { createChart } from "../../packages/charts/dist/index.js";
import type { chart_api, general_brush_range, general_brush_snapshot, general_reference_api, general_series_api, pane_api, series_api } from "../../packages/charts/dist/types.js";

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

function frames(count: number): Promise<void> {
  return new Promise((resolve) => {
    let remaining = count;
    const step = () => {
      remaining -= 1;
      if (remaining <= 0) resolve();
      else requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  });
}

/** Wait for a predicate, but return instead of throwing so the caller's assertions report the state. */
async function settle(predicate: () => boolean, timeout_ms = 2_000): Promise<void> {
  try {
    await wait_until(() => safely(predicate), timeout_ms);
  } catch {
    // The returned snapshot carries the mismatch to the Playwright assertion.
  }
  await frames(2);
}

function create_host(fixed = false): HTMLDivElement {
  const host = document.createElement("div");
  host.style.cssText = fixed
    ? "position:fixed;left:0;top:0;width:720px;height:480px;z-index:10000"
    : "width:720px;height:480px";
  document.body.appendChild(host);
  return host;
}

function series_rows(series: general_series_api): unknown[] {
  const total = series.accessibility_snapshot(0, 0).total_rows;
  const rows: unknown[] = [];
  for (let row = 0; row < total; row += 1) rows.push(series.data_at(row));
  return rows;
}

/** Complete public engine state of one general pane: axes, ordered series, rows, references, legend. */
function pane_state(chart: chart_api, pane: number) {
  return {
    axes: chart.axes(pane).map((axis) => axis.options()),
    series: chart.general_series_order(pane).map((series) => ({
      id: series.id,
      kind: series.kind,
      options: series.options(),
      rows: series_rows(series),
    })),
    references: chart.general_references(pane).map((reference) => ({
      id: reference.id,
      options: reference.options(),
    })),
    legend: chart.general_legend_snapshot(pane).items,
  };
}

/** Pane state with engine-assigned numeric identities replaced by series order positions. */
function portable_pane_state(chart: chart_api, pane: number) {
  const state = pane_state(chart, pane);
  const position = new Map(state.series.map((series, index) => [series.id, index]));
  return {
    axes: state.axes,
    series: state.series.map(({ kind, options, rows }) => ({
      kind,
      options,
      rows: rows.map((row) => row === null ? null : {
        ...(row as Record<string, unknown>),
        series: position.get((row as { series: number }).series),
      }),
    })),
    references: state.references.map(({ options }) => options),
    legend: state.legend.map((item) => ({ ...item, series: position.get(item.series) })),
  };
}

/**
 * Find a viewport point over a mark of `pane` by scanning public engine hits from the plot bottom
 * up. Hit-test X is plot-local and Y is chart-space; the chart host starts at viewport (0, 0).
 */
function general_hit_point(chart: chart_api, pane: pane_api, row = 0): { x: number; y: number } {
  const geometry = pane.get_geometry();
  const index = pane.pane_index();
  for (let y = Math.floor(geometry.top + geometry.height) - 2; y > geometry.top; y -= 3) {
    for (let x = 0; x < geometry.width; x += 3) {
      const hit = chart.general_hit_test(index, x, y);
      if (hit !== null && hit.row === row) return { x: x + geometry.left, y };
    }
  }
  throw new Error(`general pane ${index} has no exact hit for row ${row}`);
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

/** Verify GeneralPane uses one atomic public composition and skips semantically idle commits. */
export async function exerciseReactCompositionTransactions(): Promise<Record<string, unknown>> {
  const host = document.createElement("div");
  host.style.cssText = "width:720px;height:480px";
  document.body.appendChild(host);
  const root = createRoot(host);
  let chart: chart_api | null = null;
  const ready = new Map<string, general_series_api>();
  let map_calls = 0;
  const initial_rows = [{ month: "Jan", sales: 12 }, { month: "Feb", sales: 19 }];
  const updated_rows = [{ month: "Jan", sales: 21 }, { month: "Feb", sales: 27 }];
  const initial_sales_rows = [{ id: "jan-sales", x: "Jan", y: 12 }];
  const updated_sales_rows = [{ id: "jan-sales", x: "Jan", y: 27 }];
  const forecast_rows = [{ id: "jan-forecast", x: "Jan", y: 15 }];
  const updated_forecast_rows = [{ id: "jan-forecast", x: "Jan", y: 31 }];
  const initial_axes: readonly GeneralAxisSpec[] = [
    { id: "month", dimension: "x", position: "bottom", scale: "band" },
    { id: "sales", dimension: "y", position: "left", scale: "linear", title: "Sales" },
  ];
  const updated_axes: readonly GeneralAxisSpec[] = [
    { id: "month", dimension: "x", position: "bottom", scale: "band" },
    { id: "sales", dimension: "y", position: "left", scale: "linear", title: "Updated sales" },
  ];
  const initial_sales_options = { x_axis_id: "month", y_axis_id: "sales", title: "Sales", visible: true };
  const updated_sales_options = { x_axis_id: "month", y_axis_id: "sales", title: "Updated sales", visible: false };
  const initial_forecast_options = { x_axis_id: "month", y_axis_id: "sales", title: "Forecast" };
  const updated_forecast_options = { x_axis_id: "month", y_axis_id: "sales", title: "Updated forecast" };
  const mapRow = (row: (typeof initial_rows)[number]) => {
    map_calls += 1;
    return { id: row.month, x: row.month, y: row.sales };
  };
  const render = (updated: boolean, fragment: boolean) => {
    const pane_series: readonly GeneralSeriesSpec[] = updated
      ? [{
        key: "forecast",
        kind: "xy_line",
        options: updated_forecast_options,
        data: updated_forecast_rows,
      }, {
        key: "sales",
        kind: "column",
        options: updated_sales_options,
        data: updated_sales_rows,
      }]
      : [{
        key: "sales",
        kind: "column",
        options: initial_sales_options,
        data: initial_sales_rows,
      }, {
        key: "forecast",
        kind: "xy_line",
        options: initial_forecast_options,
        data: forecast_rows,
      }];
    const mapped = <MappedGeneralSeries key="mapped" seriesKey="mapped" kind="xy_line"
      options={{ x_axis_id: "month", y_axis_id: "sales", title: "Mapped" }}
      data={updated ? updated_rows : initial_rows} mapRow={mapRow} />;
    root.render(
      <AerisChart options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
        onChartReady={(value) => { chart = value; }}>
        <GeneralPane options={pane_options} axes={updated ? updated_axes : initial_axes} series={pane_series}
          onSeriesReady={(key, value) => { ready.set(key, value); }}>
          {fragment ? <>{[mapped]}</> : [mapped]}
        </GeneralPane>
      </AerisChart>,
    );
  };

  try {
    render(false, true);
    await wait_until(() => chart !== null && ready.size === 3
      && safely(() => ready.get("mapped")?.dataAt(1)?.value === 19)).catch(() => {
      throw new Error(`initial composition did not settle: chart=${chart !== null}, ready=${[...ready.keys()]}, mapped=${safely(() => ready.get("mapped")?.dataAt(1)?.value === 19)}`);
    });
    const live = chart as chart_api;
    const first_sales = ready.get("sales") as general_series_api;
    const first_forecast = ready.get("forecast") as general_series_api;
    const first_mapped = ready.get("mapped") as general_series_api;
    const calls = {
      compose: 0,
      validate: 0,
      direct_add: 0,
      direct_remove: 0,
      direct_reorder: 0,
    };
    const target = live as unknown as Record<string, (...args: unknown[]) => unknown>;
    const instrument = (name: string, counter: keyof typeof calls) => {
      const original = target[name];
      if (original === undefined) throw new Error(`missing chart method ${name}`);
      target[name] = (...args: unknown[]) => {
        calls[counter] += 1;
        return original.apply(live, args);
      };
    };
    instrument("compose_general_pane", "compose");
    instrument("validate_general_pane_proposal", "validate");
    for (const name of ["add_axis", "add_series", "add_general_reference"]) {
      instrument(name, "direct_add");
    }
    for (const name of ["remove_axis", "remove_series"]) {
      instrument(name, "direct_remove");
    }
    instrument("set_general_series_order", "direct_reorder");

    map_calls = 0;
    render(true, false);
    await wait_until(() => calls.compose === 1
      && safely(() => first_mapped.dataAt(1)?.value === 27)
      && safely(() => first_sales.options().visible === false)).catch(() => {
      throw new Error(`updated composition did not settle: calls=${JSON.stringify(calls)}, mapped=${safely(() => first_mapped.dataAt(1)?.value === 27)}, hidden=${safely(() => first_sales.options().visible === false)}`);
    });
    const multi_change = {
      calls: { ...calls },
      map_calls,
      same_sales: ready.get("sales") === first_sales,
      same_forecast: ready.get("forecast") === first_forecast,
      same_mapped: ready.get("mapped") === first_mapped,
      order: live.general_series_order(first_sales.options().pane).map((item) => item.id),
      expected_order: [first_forecast.id, first_sales.id, first_mapped.id],
      axis_title: live.axis("sales")?.options().title,
    };

    for (const key of Object.keys(calls) as (keyof typeof calls)[]) calls[key] = 0;
    map_calls = 0;
    for (let index = 0; index < 10; index += 1) {
      render(true, index % 2 === 0);
      await new Promise((resolve) => requestAnimationFrame(resolve));
    }
    await new Promise((resolve) => requestAnimationFrame(resolve));
    const idle = { calls: { ...calls }, map_calls };

    root.unmount();
    return { multi_change, idle };
  } finally {
    root.unmount();
    host.remove();
  }
}

/** A declarative general-only chart owns the canonical first pane across Strict Mode remounts. */
export async function exerciseReactGeneralFirstPane(): Promise<Record<string, unknown>> {
  const host = create_host();
  const root = createRoot(host);
  let chart: chart_api | null = null;
  let pane: pane_api | null = null;
  let series: general_series_api | null = null;
  const render = (visible: boolean) => root.render(
    <StrictMode>
      <AerisChart options={{ backend: "canvas2d", autoSize: false, accessibility: false,
        initialPane: pane_options }} onChartReady={(created) => { chart = created; }}>
        {visible && <GeneralPane options={pane_options} useInitialPane
          onPaneReady={(created) => { pane = created; }}
          onSeriesReady={(_key, created) => { series = created; }}>
          <GeneralAxis id="month" dimension="x" position="bottom" scale="band" />
          <GeneralAxis id="sales" dimension="y" position="left" scale="linear" />
          <GeneralSeries seriesKey="sales" kind="column"
            options={{ x_axis_id: "month", y_axis_id: "sales" }}
            data={[{ id: "jan", x: "Jan", y: 12 }]} />
        </GeneralPane>}
      </AerisChart>
    </StrictMode>,
  );
  try {
    render(true);
    await wait_until(() => chart !== null && pane !== null && series !== null
      && safely(() => series?.dataAt(0)?.value === 12)).catch(() => {
      throw new Error(`general-first initial: chart=${chart !== null}, pane=${pane !== null}, series=${series !== null}, axes=${chart === null ? "none" : (chart as chart_api).axes().map((axis) => axis.id)}`);
    });
    const live = chart as chart_api;
    const first_pane = pane as pane_api;
    const initial = { pane_count: live.panes().length, pane_index: first_pane.pane_index(),
      series_count: live.general_series_order(0).length, all_series_count: first_pane.get_series().length };
    render(false);
    await wait_until(() => live.axes(0).length === 0 && live.general_series_order(0).length === 0)
      .catch(() => { throw new Error(`general-first empty: axes=${live.axes(0).length}, series=${live.general_series_order(0).length}`); });
    const empty = { pane_count: live.panes().length, same_pane: live.panes()[0]?.pane_index() === first_pane.pane_index() };
    render(true);
    await wait_until(() => live.general_series_order(0).length === 1)
      .catch(() => { throw new Error(`general-first remount: axes=${live.axes(0).length}, series=${live.general_series_order(0).length}`); });
    const remounted = { pane_count: live.panes().length, pane_index: pane?.pane_index(),
      value: live.general_series_order(0)[0]?.dataAt(0)?.value };
    return { initial, empty, remounted };
  } finally {
    root.unmount();
    host.remove();
  }
}

/** Reject unsupported GeneralPane children through the public React declaration surface. */
export async function exerciseReactUnsupportedChild(): Promise<Record<string, unknown>> {
  const host = document.createElement("div");
  host.style.cssText = "width:720px;height:480px";
  document.body.appendChild(host);
  const root = createRoot(host);
  let chart: chart_api | null = null;
  let failure: Error | null = null;
  const original_console_error = console.error;
  console.error = () => {};
  try {
    root.render(
      <AerisChart options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
        onChartReady={(value) => { chart = value; }}>
        <FailureBoundary onFailure={(error) => { failure = error; }}>
          <GeneralPane options={pane_options}>
            <div />
          </GeneralPane>
        </FailureBoundary>
      </AerisChart>,
    );
    await wait_until(() => chart !== null && failure !== null);
    const live = chart as chart_api;
    return {
      failure: (failure as Error).message,
      pane_count: live.panes().length,
      axis_count: live.axes().length,
      series_count: live.general_series_order().length,
    };
  } finally {
    console.error = original_console_error;
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
  let validation_count = 0;
  const rows = [{ id: "jan", x: "Jan", y: 12 }];
  const render = (x_scale: "band" | "linear", title: string) => root.render(
    <AerisChart options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
      onChartReady={(created) => { chart = created; }}>
      <GeneralPane options={pane_options}
        onSeriesReady={(_key, created) => { series = created; }}
        onValidationError={(error) => { validation_error = String(error); validation_count += 1; }}>
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
    await frames(2);
    return {
      error: validation_error,
      validation_count,
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
  let validation_count = 0;
  const rows = [{ id: "jan", x: "Jan", y: 12 }];
  const render = (value: number | string, title: string) => root.render(
    <AerisChart options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
      onChartReady={(created) => { chart = created; }}>
      <GeneralPane options={pane_options}
        onSeriesReady={(_key, created) => { series = created; }}
        onReferenceReady={(_key, created) => { reference = created; }}
        onValidationError={(error) => { validation_error = String(error); validation_count += 1; }}>
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
    await frames(2);
    return {
      error: validation_error,
      validation_count,
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
  let validation_count = 0;
  const rows = [{ id: "jan", x: "Jan", y: 12 }];
  const render = (kind: "column" | "range_bar" | "scatter", title: string,
    data: readonly unknown[] = rows) => root.render(
    <AerisChart options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
      onChartReady={(created) => { chart = created; }}>
      <GeneralPane options={pane_options}
        onSeriesReady={(_key, created) => { series = created; }}
        onValidationError={(error) => { validation_error = String(error); validation_count += 1; }}>
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
    await frames(2);
    return { row_error, axis_error: validation_error, retained, after_axis_error,
      recovered, valid_transition, validation_count };
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
  let validation_count = 0;
  const rows = [{ id: "jan", x: "Jan", y: 12 }];
  const render = (scale: "linear" | "log", value: number) => root.render(
    <AerisChart options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
      onChartReady={(created) => { chart = created; }}>
      <GeneralPane options={pane_options}
        onSeriesReady={(_key, created) => { series = created; }}
        onReferenceReady={(_key, created) => { reference = created; }}
        onValidationError={(error) => { validation_error = String(error); validation_count += 1; }}>
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
    await frames(2);
    return { error: validation_error, validation_count, retained, applied: {
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
  let validation_count = 0;
  const render = (scale: "linear" | "log", y: number) => root.render(
    <AerisChart options={{ backend: "canvas2d", autoSize: false, accessibility: false }}
      onChartReady={(created) => { chart = created; }}>
      <GeneralPane options={{ horizontal_domain: { type: "continuous", scale: "linear" } }}
        onSeriesReady={(_key, created) => { series = created; }}
        onValidationError={(error) => { validation_error = String(error); validation_count += 1; }}>
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
    await frames(2);
    return { error: validation_error, validation_count, imperative_error, retained, applied: {
      axis_scale: first_chart.axis("y")?.options().scale,
      value: series?.dataAt(0)?.value,
    } };
  } finally {
    root.unmount();
    host.remove();
  }
}

const options_canvas = { backend: "canvas2d" as const, autoSize: false, accessibility: false };

interface retained_identity_state {
  title: string;
  color: string;
  visible: boolean;
  rows: readonly { id: string; x: string; y: number }[];
  axis_title: string;
  axis_position: "left" | "right";
  tick_count: number;
  grid_visible: boolean;
  reference_value: number;
  reference_color: string;
  labels: boolean;
}

/** Every presentation, data, axis, reference, and label edit retains each engine identity. */
export async function exerciseReactRetainedIdentities(): Promise<Record<string, unknown>> {
  const host = create_host();
  const root = createRoot(host);
  let chart: chart_api | null = null;
  let pane: pane_api | null = null;
  const series_ready: string[] = [];
  const reference_ready: string[] = [];
  const series = new Map<string, general_series_api>();
  let reference: general_reference_api | null = null;
  const forecast_rows = [{ id: "jan-f", x: "Jan", y: 3 }, { id: "feb-f", x: "Feb", y: 4 }];
  let state: retained_identity_state = {
    title: "Sales",
    color: "#2563eb",
    visible: true,
    rows: [{ id: "jan", x: "Jan", y: 12 }, { id: "feb", x: "Feb", y: 19 }],
    axis_title: "Sales axis",
    axis_position: "left",
    tick_count: 5,
    grid_visible: true,
    reference_value: 14,
    reference_color: "#f59e0b",
    labels: false,
  };
  const render = () => root.render(
    <StrictMode>
      <AerisChart options={options_canvas} onChartReady={(created) => { chart = created; }}>
        <GeneralPane options={pane_options}
          onPaneReady={(created) => { pane = created; }}
          onSeriesReady={(key, created) => { series_ready.push(key); series.set(key, created); }}
          onReferenceReady={(key, created) => { reference_ready.push(key); reference = created; }}>
          <GeneralAxis id="month" dimension="x" position="bottom" scale="band" />
          <GeneralAxis id="sales" dimension="y" position={state.axis_position} scale="linear"
            title={state.axis_title} tick_count={state.tick_count} grid_visible={state.grid_visible} />
          <GeneralAxis id="forecast-axis" dimension="y" position="right" scale="linear" />
          <GeneralSeries seriesKey="sales" kind="column"
            options={{ x_axis_id: "month", y_axis_id: "sales", title: state.title, color: state.color,
              visible: state.visible }}
            data={state.rows} />
          <GeneralSeries seriesKey="forecast" kind="xy_line"
            options={{ x_axis_id: "month", y_axis_id: "forecast-axis", title: "Forecast" }}
            data={forecast_rows} />
          <GeneralDataLabels seriesKey="sales" visible={state.labels} />
          <GeneralReference referenceKey="target" options={{ kind: "line", axis_id: "sales",
            value: state.reference_value, color: state.reference_color }} />
        </GeneralPane>
      </AerisChart>
    </StrictMode>,
  );
  try {
    render();
    await wait_until(() => chart !== null && pane !== null && series.size === 2 && reference !== null
      && safely(() => series.get("sales")?.dataAt(1)?.value === 19));
    await frames(2);
    const live = chart as chart_api;
    const index = (pane as pane_api).pane_index();
    const sales = series.get("sales") as general_series_api;
    const forecast = series.get("forecast") as general_series_api;
    const target = reference as general_reference_api;
    const held_axes = ["month", "sales", "forecast-axis"].map((id) => live.axis(id));
    const initial = {
      series_ids: live.general_series_order(index).map((item) => item.id),
      reference_ids: live.general_references(index).map((item) => item.id),
      series_ready: series_ready.length,
      reference_ready: reference_ready.length,
    };
    const observe = () => {
      const axis = live.axis("sales")?.options();
      const reference_options = target.options();
      return {
        series_ids: live.general_series_order(index).map((item) => item.id),
        reference_ids: live.general_references(index).map((item) => item.id),
        same_handles: series.get("sales") === sales && series.get("forecast") === forecast
          && reference === target,
        axes_live: held_axes.every((handle) => handle !== null && safely(() => handle.options().id.length > 0)),
        series_ready: series_ready.length,
        reference_ready: reference_ready.length,
        title: sales.options().title,
        color: sales.options().color,
        visible: sales.options().visible,
        values: [sales.dataAt(0)?.value, sales.dataAt(1)?.value],
        axis: { title: axis?.title, position: axis?.position, tick_count: axis?.tick_count,
          grid_visible: axis?.grid_visible },
        reference: reference_options.kind === "line"
          ? { value: reference_options.value, color: reference_options.color }
          : null,
        labels: sales.options().data_labels,
      };
    };
    const steps: [string, Partial<retained_identity_state>, () => boolean][] = [
      ["series_title_color", { title: "Revenue", color: "#dc2626" },
        () => sales.options().title === "Revenue"],
      ["series_visibility", { visible: false }, () => sales.options().visible === false],
      ["data_replacement", { rows: [{ id: "jan", x: "Jan", y: 33 }, { id: "feb", x: "Feb", y: 44 }] },
        () => sales.dataAt(1)?.value === 44],
      ["axis_presentation", { axis_title: "Revenue axis", axis_position: "right", tick_count: 3,
        grid_visible: false }, () => live.axis("sales")?.options().title === "Revenue axis"],
      ["reference_options", { reference_value: 18, reference_color: "#10b981" }, () => {
        const options = target.options();
        return options.kind === "line" && options.value === 18;
      }],
      ["labels_on", { labels: true }, () => sales.options().data_labels === true],
      ["labels_off", { labels: false }, () => sales.options().data_labels === false],
    ];
    const results: Record<string, unknown> = {};
    for (const [name, patch, predicate] of steps) {
      state = { ...state, ...patch };
      render();
      await settle(predicate);
      results[name] = observe();
    }
    return { initial, steps: results };
  } finally {
    root.unmount();
    host.remove();
  }
}

/** A reference whose axis the same commit removes, or that names no axis, rejects the whole commit. */
export async function exerciseReactReferenceAxisRemoval(): Promise<Record<string, unknown>> {
  const host = create_host();
  const root = createRoot(host);
  let chart: chart_api | null = null;
  let pane: pane_api | null = null;
  const series_ready: string[] = [];
  const reference_ready: string[] = [];
  const errors: string[] = [];
  const rows = [{ id: "jan", x: "Jan", y: 12 }, { id: "feb", x: "Feb", y: 19 }];
  const render = (variant: "valid" | "removed_axis" | "missing_axis", title: string) => root.render(
    <AerisChart options={options_canvas} onChartReady={(created) => { chart = created; }}>
      <GeneralPane options={pane_options}
        onPaneReady={(created) => { pane = created; }}
        onSeriesReady={(key) => { series_ready.push(key); }}
        onReferenceReady={(key) => { reference_ready.push(key); }}
        onValidationError={(error) => { errors.push(String(error)); }}>
        <GeneralAxis id="month" dimension="x" position="bottom" scale="band" />
        <GeneralAxis id="sales" dimension="y" position="left" scale="linear" />
        {variant === "removed_axis" ? null
          : <GeneralAxis id="alt" dimension="y" position="right" scale="linear" />}
        <GeneralSeries seriesKey="sales" kind="column"
          options={{ x_axis_id: "month", y_axis_id: "sales", title }} data={rows} />
        <GeneralSeries seriesKey="trend" kind="xy_line"
          options={{ x_axis_id: "month", y_axis_id: "sales", title: "Trend" }} data={rows} />
        <GeneralReference referenceKey="target" options={{ kind: "line", axis_id: "sales", value: 14 }} />
        <GeneralReference referenceKey="alt-target" options={{ kind: "line",
          axis_id: variant === "missing_axis" ? "missing" : "alt", value: 3 }} />
      </GeneralPane>
    </AerisChart>,
  );
  try {
    render("valid", "Sales");
    await wait_until(() => chart !== null && pane !== null && reference_ready.length === 2
      && series_ready.length === 2);
    await frames(2);
    const live = chart as chart_api;
    const index = (pane as pane_api).pane_index();
    const before = pane_state(live, index);
    render("removed_axis", "Should not apply");
    await settle(() => errors.length >= 1);
    await frames(2);
    const after_removed = { state: pane_state(live, index), errors: errors.length };
    render("missing_axis", "Should not apply either");
    await settle(() => errors.length >= 2);
    await frames(2);
    const after_missing = { state: pane_state(live, index), errors: errors.length };
    render("valid", "Recovered");
    await settle(() => live.general_series_order(index)[0]?.options().title === "Recovered");
    return {
      before,
      after_removed,
      after_missing,
      errors,
      recovered_title: live.general_series_order(index)[0]?.options().title,
      recovered_ids: live.general_series_order(index).map((item) => item.id),
      recovered_reference_ids: live.general_references(index).map((item) => item.id),
      series_ready: series_ready.length,
      reference_ready: reference_ready.length,
    };
  } finally {
    root.unmount();
    host.remove();
  }
}

/** Invalid rows in a new or retained series reject every valid change in the same commit. */
export async function exerciseReactInvalidSeriesBatch(): Promise<Record<string, unknown>> {
  const host = create_host();
  const root = createRoot(host);
  let chart: chart_api | null = null;
  let pane: pane_api | null = null;
  const errors: string[] = [];
  const series_ready: string[] = [];
  const rows = [{ id: "jan", x: "Jan", y: 12 }, { id: "feb", x: "Feb", y: 19 }];
  const invalid_rows = [{ id: "jan", x: 7 as unknown as string, y: 12 }];
  const render = (title: string, extra: boolean, data: readonly { id: string; x: string; y: number }[]) =>
    root.render(
      <AerisChart options={options_canvas} onChartReady={(created) => { chart = created; }}>
        <GeneralPane options={pane_options}
          onPaneReady={(created) => { pane = created; }}
          onSeriesReady={(key) => { series_ready.push(key); }}
          onValidationError={(error) => { errors.push(String(error)); }}>
          <GeneralAxis id="month" dimension="x" position="bottom" scale="band" />
          <GeneralAxis id="sales" dimension="y" position="left" scale="linear" />
          <GeneralSeries seriesKey="sales" kind="column"
            options={{ x_axis_id: "month", y_axis_id: "sales", title }} data={data} />
          {extra ? <GeneralSeries seriesKey="extra" kind="column"
            options={{ x_axis_id: "month", y_axis_id: "sales", title: "Extra" }} data={invalid_rows} /> : null}
          <GeneralReference referenceKey="target" options={{ kind: "line", axis_id: "sales", value: 14 }} />
        </GeneralPane>
      </AerisChart>,
    );
  try {
    render("Sales", false, rows);
    await wait_until(() => chart !== null && pane !== null && series_ready.length === 1);
    await frames(2);
    const live = chart as chart_api;
    const index = (pane as pane_api).pane_index();
    const before = pane_state(live, index);
    render("Updated sales", true, rows);
    await settle(() => errors.length >= 1);
    await frames(2);
    const new_invalid_series = {
      state: pane_state(live, index),
      errors: errors.length,
      series_count: live.general_series_order(index).length,
    };
    render("Sales", false, invalid_rows);
    await settle(() => errors.length >= 2);
    await frames(2);
    const invalid_replacement = { state: pane_state(live, index), errors: errors.length };
    const recovered_rows = [{ id: "jan", x: "Jan", y: 30 }];
    render("Recovered", false, recovered_rows);
    await settle(() => live.general_series_order(index)[0]?.options().title === "Recovered");
    const sales = live.general_series_order(index)[0];
    return {
      before,
      new_invalid_series,
      invalid_replacement,
      errors,
      recovered: {
        same_id: sales?.id === before.series[0]?.id,
        title: sales?.options().title,
        rows: sales === undefined ? [] : series_rows(sales).map((row) => (row as { value: number }).value),
        series_ready: series_ready.length,
        errors: errors.length,
      },
    };
  } finally {
    root.unmount();
    host.remove();
  }
}

interface callback_failure_state {
  title: string;
  extra: boolean;
  extra_reference: boolean;
  burst?: boolean;
  mapper: (row: { month: string; sales: number }) => { id: string; x: string; y: number };
  mapped_rows: readonly { month: string; sales: number }[];
}

/** Mapper failures keep the prior pane; ready-callback failures keep the complete new pane. */
export async function exerciseReactCallbackFailures(): Promise<Record<string, unknown>> {
  const host = create_host();
  const root = createRoot(host);
  let chart: chart_api | null = null;
  let pane: pane_api | null = null;
  const errors: string[] = [];
  const series_ready: string[] = [];
  const reference_ready: string[] = [];
  const handles = new Map<number, string>();
  let throw_series_for: readonly string[] = [];
  let throw_reference_for: string | null = null;
  const good_mapper = (row: { month: string; sales: number }) => ({ id: row.month, x: row.month, y: row.sales });
  const throwing_mapper = (): { id: string; x: string; y: number } => {
    throw new Error("expected mapper failure");
  };
  const first_rows = [{ month: "Jan", sales: 12 }, { month: "Feb", sales: 19 }];
  const second_rows = [{ month: "Jan", sales: 22 }, { month: "Feb", sales: 29 }];
  let state: callback_failure_state = {
    title: "Sales", extra: false, extra_reference: false, mapper: good_mapper, mapped_rows: first_rows,
  };
  const render = () => root.render(
    <AerisChart options={options_canvas} onChartReady={(created) => { chart = created; }}>
      <GeneralPane options={pane_options}
        onPaneReady={(created) => { pane = created; }}
        onSeriesReady={(key, created) => {
          series_ready.push(key);
          handles.set(created.id, key);
          if (throw_series_for.includes(key)) throw new Error("expected series ready failure");
        }}
        onReferenceReady={(key) => {
          reference_ready.push(key);
          if (key === throw_reference_for) throw new Error("expected reference ready failure");
        }}
        onValidationError={(error) => { errors.push(error instanceof Error ? error.message : String(error)); }}>
        <GeneralAxis id="month" dimension="x" position="bottom" scale="band" />
        <GeneralAxis id="sales" dimension="y" position="left" scale="linear" />
        <GeneralSeries seriesKey="sales" kind="column"
          options={{ x_axis_id: "month", y_axis_id: "sales", title: state.title }}
          data={[{ id: "jan-s", x: "Jan", y: 5 }]} />
        {state.extra ? <GeneralSeries seriesKey="extra" kind="xy_line"
          options={{ x_axis_id: "month", y_axis_id: "sales", title: "Extra" }}
          data={[{ id: "jan-e", x: "Jan", y: 7 }]} /> : null}
        {state.burst === true ? <>
          <GeneralSeries seriesKey="burst_a" kind="xy_line"
            options={{ x_axis_id: "month", y_axis_id: "sales", title: "Burst A" }}
            data={[{ id: "jan-a", x: "Jan", y: 3 }]} />
          <GeneralSeries seriesKey="burst_b" kind="xy_line"
            options={{ x_axis_id: "month", y_axis_id: "sales", title: "Burst B" }}
            data={[{ id: "jan-b", x: "Jan", y: 4 }]} />
        </> : null}
        <MappedGeneralSeries seriesKey="mapped" kind="xy_line"
          options={{ x_axis_id: "month", y_axis_id: "sales", title: "Mapped" }}
          data={state.mapped_rows} mapRow={state.mapper} />
        <GeneralReference referenceKey="target" options={{ kind: "line", axis_id: "sales", value: 14 }} />
        {state.extra_reference ? <GeneralReference referenceKey="floor"
          options={{ kind: "line", axis_id: "sales", value: 2 }} /> : null}
      </GeneralPane>
    </AerisChart>,
  );
  const sales_title = (live: chart_api, index: number): string | undefined => live.general_series_order(index)
    .find((item) => handles.get(item.id) === "sales")?.options().title;
  try {
    render();
    await wait_until(() => chart !== null && pane !== null && series_ready.length === 2
      && reference_ready.length === 1);
    await frames(2);
    const live = chart as chart_api;
    const index = (pane as pane_api).pane_index();
    const summary = () => ({
      errors: [...errors],
      keys: live.general_series_order(index).map((item) => handles.get(item.id) ?? null),
      sales_title: sales_title(live, index),
      mapped_values: series_rows(live.general_series_order(index)
        .find((item) => handles.get(item.id) === "mapped") as general_series_api)
        .map((row) => (row as { value: number }).value),
      reference_values: live.general_references(index).map((item) => {
        const options = item.options();
        return options.kind === "line" ? options.value : null;
      }),
      axis_count: live.axes(index).length,
      series_ready: [...series_ready],
      reference_ready: [...reference_ready],
    });
    const before = pane_state(live, index);

    state = { ...state, title: "Mapper title", mapper: throwing_mapper, mapped_rows: second_rows };
    render();
    await settle(() => errors.length >= 1);
    await frames(2);
    const mapper_failure = { ...summary(), state_unchanged: JSON.stringify(pane_state(live, index)) === JSON.stringify(before) };

    throw_series_for = ["extra"];
    state = { ...state, title: "Series ready title", extra: true, mapper: good_mapper, mapped_rows: first_rows };
    render();
    await settle(() => errors.length >= 2);
    await frames(2);
    const series_ready_failure = summary();

    throw_series_for = [];
    throw_reference_for = "floor";
    state = { ...state, title: "Reference ready title", extra_reference: true };
    render();
    await settle(() => errors.length >= 3);
    await frames(2);
    const reference_ready_failure = summary();

    throw_reference_for = null;
    state = { ...state, title: "Recovered", mapped_rows: second_rows };
    render();
    await settle(() => sales_title(live, index) === "Recovered");
    const recovered = summary();

    throw_series_for = ["burst_a", "burst_b"];
    state = { ...state, burst: true };
    render();
    await settle(() => errors.length >= 4);
    await frames(2);
    const multiple_ready_failures = summary();
    return { mapper_failure, series_ready_failure, reference_ready_failure, recovered, multiple_ready_failures };
  } finally {
    root.unmount();
    host.remove();
  }
}

/**
 * With `onValidationError`, engine and mapper failures are delivered once and never reach a
 * boundary. Without it, they reach a boundary inside the chart while the financial pane survives.
 */
export async function exerciseReactFailureDelivery(): Promise<Record<string, unknown>> {
  const host = create_host();
  const root = createRoot(host);
  let chart: chart_api | null = null;
  let financial: series_api | null = null;
  let general: general_series_api | null = null;
  const validation: string[] = [];
  const boundary: string[] = [];
  const candles = [
    { time: 1735689600, open: 1, high: 4, low: 1, close: 2 },
    { time: 1735776000, open: 2, high: 5, low: 2, close: 4 },
  ];
  const rows = [{ id: "jan", x: "Jan", y: 12 }];
  const invalid_rows = [{ id: "jan", x: 7 as unknown as string, y: 12 }];
  const records_a = [{ month: "Jan", sales: 12 }];
  const records_b = [{ month: "Jan", sales: 13 }];
  const good_mapper = (row: { month: string; sales: number }) => ({ id: row.month, x: row.month, y: row.sales });
  const throwing_mapper = (): { id: string; x: string; y: number } => {
    throw new Error("expected mapper failure");
  };
  const render = (mode: "valid" | "engine" | "mapper", handler: boolean, boundary_key: string) => root.render(
    <AerisChart options={options_canvas} onChartReady={(created) => { chart = created; }}>
      <FinancialSeries kind="candlestick" data={candles} onSeriesReady={(created) => { financial = created; }} />
      <FailureBoundary key={boundary_key} onFailure={(error) => { boundary.push(error.message); }}>
        <GeneralPane options={pane_options}
          onSeriesReady={(key, created) => { if (key === "sales") general = created; }}
          onValidationError={handler
            ? (error) => { validation.push(error instanceof Error ? error.message : String(error)); }
            : undefined}>
          <GeneralAxis id="month" dimension="x" position="bottom" scale="band" />
          <GeneralAxis id="sales" dimension="y" position="left" scale="linear" />
          <GeneralSeries seriesKey="sales" kind="column"
            options={{ x_axis_id: "month", y_axis_id: "sales" }}
            data={mode === "engine" ? invalid_rows : rows} />
          <MappedGeneralSeries seriesKey="mapped" kind="xy_line"
            options={{ x_axis_id: "month", y_axis_id: "sales" }}
            data={mode === "mapper" ? records_b : records_a}
            mapRow={mode === "mapper" ? throwing_mapper : good_mapper} />
        </GeneralPane>
      </FailureBoundary>
    </AerisChart>,
  );
  const original_console_error = console.error;
  console.error = () => {};
  try {
    render("valid", true, "handled");
    await wait_until(() => chart !== null && financial !== null && general !== null
      && safely(() => (financial as series_api).data()[1]?.close === 4));
    const live = chart as chart_api;
    const first_financial = financial as unknown as series_api;
    const financial_state = () => ({
      chart_live: safely(() => live.backend().length > 0),
      financial_retained: live.series_order()[0] === first_financial,
      financial_closes: first_financial.data().map((bar) => (bar as { close: number }).close),
      pane_count: live.panes().length,
    });

    render("engine", true, "handled");
    await settle(() => validation.length >= 1);
    await frames(3);
    const handled_engine = { validation: validation.length, boundary: boundary.length, ...financial_state() };
    render("valid", true, "handled");
    await frames(3);
    render("mapper", true, "handled");
    await settle(() => validation.length >= 2);
    await frames(3);
    const handled_mapper = { validation: validation.length, boundary: boundary.length, ...financial_state() };

    general = null;
    render("valid", false, "unhandled-engine");
    await settle(() => general !== null && live.panes().length === 2);
    render("engine", false, "unhandled-engine");
    await settle(() => boundary.length >= 1);
    await frames(3);
    const unhandled_engine = { validation: validation.length, boundary: [...boundary], ...financial_state() };

    general = null;
    render("valid", false, "unhandled-mapper");
    await settle(() => general !== null && live.panes().length === 2);
    const remounted_panes = live.panes().length;
    render("mapper", false, "unhandled-mapper");
    await settle(() => boundary.length >= 2);
    await frames(3);
    const unhandled_mapper = { validation: validation.length, boundary: [...boundary], ...financial_state() };
    return {
      validation_messages: validation,
      handled_engine,
      handled_mapper,
      unhandled_engine,
      remounted_panes,
      unhandled_mapper,
    };
  } finally {
    console.error = original_console_error;
    root.unmount();
    host.remove();
  }
}

declare global {
  interface Window {
    __reactFormatters?: {
      rerender(generation: number): Promise<Record<string, unknown>>;
      point(): { x: number; y: number };
      dispose(): void;
    };
    __reactTwoPanes?: {
      remove_a(): Promise<Record<string, unknown>>;
      summary(): Record<string, unknown>;
      point_a(): { x: number; y: number };
      point_b(): { x: number; y: number };
      dispose(): void;
    };
  }
}

/** Mount a pane whose legend and tooltip receive new inline formatters on each rerender. */
export async function mountReactFormatterFixture(): Promise<Record<string, unknown>> {
  const host = create_host(true);
  const root = createRoot(host);
  let chart: chart_api | null = null;
  let pane: pane_api | null = null;
  let ready = 0;
  const sales_rows = [{ id: "jan", x: "Jan", y: 42 }, { id: "feb", x: "Feb", y: 55 }];
  const margin_rows = [{ id: "jan-m", x: "Jan", y: 21 }, { id: "feb-m", x: "Feb", y: 28 }];
  const render = (generation: number) => root.render(
    <AerisChart options={options_canvas} style={{ width: 720, height: 480 }}
      onChartReady={(created) => { created.resize(720, 480, 1); chart = created; }}>
      <GeneralPane options={pane_options}
        onPaneReady={(created) => { pane = created; }}
        onSeriesReady={() => { ready += 1; }}>
        <GeneralAxis id="month" dimension="x" position="bottom" scale="band" />
        <GeneralAxis id="sales" dimension="y" position="left" scale="linear" />
        <GeneralSeries seriesKey="sales" kind="column"
          options={{ x_axis_id: "month", y_axis_id: "sales", title: "Sales" }} data={sales_rows} />
        <GeneralSeries seriesKey="margin" kind="column"
          options={{ x_axis_id: "month", y_axis_id: "sales", title: "Margin" }} data={margin_rows} />
        <GeneralLegend options={{
          format_title: (title) => `T${generation}:${title}`,
          format_action: (visible, title) => `A${generation} ${visible ? "hide" : "show"} ${title}`,
        }} />
        <GeneralTooltip options={{
          format_value: (value) => `V${generation}:${value}`,
          format_detail: (item) => item.title === "Margin" ? `D${generation}:${item.value}` : undefined,
        }} />
      </GeneralPane>
    </AerisChart>,
  );
  render(0);
  await wait_until(() => chart !== null && pane !== null && ready === 2
    && host.querySelector('[role="group"][aria-label="Chart legend"]') !== null
    && host.querySelector('[role="tooltip"]') !== null);
  await frames(2);
  const live = chart as chart_api;
  const live_pane = pane as pane_api;
  live.render();
  const legend = host.querySelector('[role="group"][aria-label="Chart legend"]') as HTMLElement;
  const tooltip = host.querySelector('[role="tooltip"]') as HTMLElement;
  const observe = () => ({
    same_legend: host.querySelector('[role="group"][aria-label="Chart legend"]') === legend,
    legend_connected: legend.isConnected,
    legend_count: host.querySelectorAll('[role="group"][aria-label="Chart legend"]').length,
    legend_text: Array.from(legend.querySelectorAll("button")).map((button) => button.textContent),
    legend_actions: Array.from(legend.querySelectorAll("button")).map((button) => button.getAttribute("aria-label")),
    same_tooltip: host.querySelector('[role="tooltip"]') === tooltip,
    tooltip_connected: tooltip.isConnected,
    tooltip_count: host.querySelectorAll('[role="tooltip"]').length,
    series_ready: ready,
  });
  window.__reactFormatters = {
    async rerender(generation) {
      render(generation);
      await frames(3);
      return observe();
    },
    point: () => general_hit_point(live, live_pane),
    dispose() {
      root.unmount();
      host.remove();
      delete window.__reactFormatters;
    },
  };
  return observe();
}

/** Mount two general panes, each with its own legend and tooltip, and expose removal of the first. */
export async function mountReactTwoPaneFixture(): Promise<Record<string, unknown>> {
  const host = create_host(true);
  const root = createRoot(host);
  let chart: chart_api | null = null;
  const panes = new Map<string, pane_api>();
  let ready = 0;
  const alpha_rows = [{ id: "jan-a", x: "Jan", y: 30 }, { id: "feb-a", x: "Feb", y: 35 }];
  const beta_rows = [{ id: "jan-b", x: "Jan", y: 40 }, { id: "feb-b", x: "Feb", y: 45 }];
  const gamma_rows = [{ id: "jan-g", x: "Jan", y: 20 }, { id: "feb-g", x: "Feb", y: 25 }];
  const render = (show_a: boolean) => root.render(
    <AerisChart options={options_canvas} style={{ width: 720, height: 480 }}
      onChartReady={(created) => { created.resize(720, 480, 1); chart = created; }}>
      {show_a ? <GeneralPane key="a" options={pane_options}
        onPaneReady={(created) => { panes.set("a", created); }}
        onSeriesReady={() => { ready += 1; }}>
        <GeneralAxis id="a-month" dimension="x" position="bottom" scale="band" />
        <GeneralAxis id="a-value" dimension="y" position="left" scale="linear" />
        <GeneralSeries seriesKey="alpha" kind="column"
          options={{ x_axis_id: "a-month", y_axis_id: "a-value", title: "Alpha" }} data={alpha_rows} />
        <GeneralLegend options={{ label: "Legend A" }} />
        <GeneralTooltip options={{ lang: "x-a" }} />
      </GeneralPane> : null}
      <GeneralPane key="b" options={pane_options}
        onPaneReady={(created) => { panes.set("b", created); }}
        onSeriesReady={() => { ready += 1; }}>
        <GeneralAxis id="b-month" dimension="x" position="bottom" scale="band" />
        <GeneralAxis id="b-value" dimension="y" position="left" scale="linear" />
        <GeneralSeries seriesKey="beta" kind="column"
          options={{ x_axis_id: "b-month", y_axis_id: "b-value", title: "Beta" }} data={beta_rows} />
        <GeneralSeries seriesKey="gamma" kind="column"
          options={{ x_axis_id: "b-month", y_axis_id: "b-value", title: "Gamma" }} data={gamma_rows} />
        <GeneralLegend options={{ label: "Legend B" }} />
        <GeneralTooltip options={{ lang: "x-b" }} />
      </GeneralPane>
    </AerisChart>,
  );
  render(true);
  await wait_until(() => chart !== null && panes.size === 2 && ready === 3
    && host.querySelector('[aria-label="Legend A"]') !== null
    && host.querySelector('[aria-label="Legend B"]') !== null
    && host.querySelectorAll('[role="tooltip"]').length === 2);
  await frames(2);
  const live = chart as chart_api;
  live.render();
  const legend_b = host.querySelector('[aria-label="Legend B"]') as HTMLElement;
  const tooltip_b = host.querySelector('[role="tooltip"][lang="x-b"]') as HTMLElement;
  const titles = (selector: string) => Array.from(host.querySelectorAll(`${selector} button`))
    .map((button) => button.textContent);
  const summary = () => ({
    pane_count: live.panes().length,
    pane_b_index: (panes.get("b") as pane_api).pane_index(),
    legend_a_present: host.querySelector('[aria-label="Legend A"]') !== null,
    legend_a_titles: titles('[aria-label="Legend A"]'),
    legend_b_titles: titles('[aria-label="Legend B"]'),
    same_legend_b: host.querySelector('[aria-label="Legend B"]') === legend_b && legend_b.isConnected,
    same_tooltip_b: host.querySelector('[role="tooltip"][lang="x-b"]') === tooltip_b && tooltip_b.isConnected,
    tooltip_langs: Array.from(host.querySelectorAll('[role="tooltip"]')).map((element) => element.getAttribute("lang")),
    general_series_count: live.general_series_order().length,
  });
  window.__reactTwoPanes = {
    async remove_a() {
      render(false);
      await settle(() => live.panes().length === 2
        && host.querySelector('[aria-label="Legend A"]') === null);
      live.render();
      return summary();
    },
    summary,
    point_a: () => general_hit_point(live, panes.get("a") as pane_api),
    point_b: () => general_hit_point(live, panes.get("b") as pane_api),
    dispose() {
      root.unmount();
      host.remove();
      delete window.__reactTwoPanes;
    },
  };
  return summary();
}

interface dual_brush_state {
  month: general_brush_range | null | undefined;
  sales: general_brush_range | null | undefined;
  scale: "linear" | "log";
}

/** Two brush controls share the engine's single brush without clearing each other's range. */
export async function exerciseReactDualBrush(): Promise<Record<string, unknown>> {
  const host = create_host();
  const root = createRoot(host);
  let chart: chart_api | null = null;
  const month_changes: (string | null)[] = [];
  const sales_changes: (general_brush_snapshot | null)[] = [];
  const rows = [{ id: "jan", x: "Jan", y: 12 }, { id: "feb", x: "Feb", y: 19 }, { id: "mar", x: "Mar", y: 25 }];
  let state: dual_brush_state = { month: { type: "category", from: "Feb", to: "Mar" }, sales: undefined, scale: "linear" };
  const render = () => root.render(
    <AerisChart options={options_canvas} onChartReady={(created) => { chart = created; }}>
      <GeneralPane options={pane_options}>
        <GeneralAxis id="month" dimension="x" position="bottom" scale="band" />
        <GeneralAxis id="sales" dimension="y" position="left" scale={state.scale} />
        <GeneralSeries seriesKey="sales" kind="xy_line"
          options={{ x_axis_id: "month", y_axis_id: "sales" }} data={rows} />
        <GeneralBrushControls options={{ axis: "month", label: "Month brush" }} range={state.month}
          onChange={(snapshot) => { month_changes.push(snapshot?.axis_id ?? null); }} />
        <GeneralBrushControls options={{ axis: "sales", label: "Sales brush" }} range={state.sales}
          onChange={(snapshot) => { sales_changes.push(snapshot); }} />
      </GeneralPane>
    </AerisChart>
  );
  const snapshot = () => {
    const current = (chart as chart_api).general_brush_snapshot();
    return current === null ? null : { axis_id: current.axis_id, dimension: current.dimension, range: current.range };
  };
  try {
    render();
    await wait_until(() => chart !== null
      && safely(() => (chart as chart_api).general_brush_snapshot()?.axis_id === "month")
      && host.querySelector('[aria-label="Sales brush"]') !== null
      && host.querySelector('[aria-label="Month brush"]') !== null);
    await frames(2);
    const live = chart as chart_api;
    const month_group = host.querySelector('[aria-label="Month brush"]') as HTMLElement;
    const sales_group = host.querySelector('[aria-label="Sales brush"]') as HTMLElement;
    const select_all = (group: HTMLElement) => (Array.from(group.querySelectorAll("button"))
      .find((button) => button.textContent === "Select all") as HTMLButtonElement).click();
    const month_controlled = snapshot();

    state = { ...state, sales: null };
    render();
    await frames(3);
    const other_cleared = snapshot();

    state = { ...state, month: null };
    render();
    await settle(() => live.general_brush_snapshot() === null);
    const own_cleared = snapshot();

    state = { ...state, month: undefined, sales: undefined };
    render();
    await frames(3);
    select_all(sales_group);
    const sales_selected = snapshot();

    // A controlled month range must not take the single brush back from the sales controls.
    state = { ...state, month: { type: "category", from: "Feb", to: "Mar" } };
    render();
    await settle(() => live.general_brush_snapshot()?.axis_id === "month");
    const month_reports_before = month_changes.length;
    select_all(sales_group);
    await frames(3);
    const not_stolen = {
      axis_id: snapshot()?.axis_id,
      month_reports: month_changes.slice(month_reports_before),
    };
    state = { ...state, month: undefined };
    render();
    await frames(3);
    const held_sales_axis = live.axis("sales");
    const sales_changes_before = sales_changes.length;

    state = { ...state, scale: "log" };
    render();
    await settle(() => live.axis("sales")?.options().scale === "log");
    const replaced = {
      snapshot: snapshot(),
      axis_scale: live.axis("sales")?.options().scale,
      prior_axis_stale: held_sales_axis !== null && !safely(() => held_sales_axis.options().id === "sales"),
      reported: sales_changes.slice(sales_changes_before).map((item) => item === null ? null : item.axis_id),
      sales_controls_retained: host.querySelector('[aria-label="Sales brush"]') === sales_group
        && sales_group.isConnected,
      month_controls_retained: host.querySelector('[aria-label="Month brush"]') === month_group
        && month_group.isConnected,
      status: sales_group.querySelector("output")?.textContent,
    };
    select_all(sales_group);
    const reselected = { snapshot: snapshot(), axis_scale: live.axis("sales")?.options().scale };
    return { month_controlled, other_cleared, own_cleared, sales_selected, not_stolen, replaced, reselected,
      month_changes_seen: month_changes.length > 0,
      month_reported_other_axis: month_changes.includes("sales") };
  } finally {
    root.unmount();
    host.remove();
  }
}

interface revenue_row {
  id: string;
  month: string;
  value: number;
}

const map_revenue = (row: revenue_row) => ({ id: row.id, x: row.month, y: row.value, label: `${row.value}` });

/**
 * The examples/all_in_one/react.tsx MarketDashboard declarations and the equivalent raw public API
 * calls produce identical pane state.
 */
export async function exerciseReactImperativeParity(): Promise<Record<string, unknown>> {
  const candles = [
    { time: 1735689600, open: 1, high: 4, low: 1, close: 2 },
    { time: 1735776000, open: 2, high: 5, low: 2, close: 4 },
  ];
  const revenue: revenue_row[] = [
    { id: "jan", month: "Jan", value: 42 },
    { id: "feb", month: "Feb", value: 55 },
    { id: "mar", month: "Mar", value: 48 },
  ];
  const domain = { type: "category" as const, scale: "band" as const };
  const react_host = create_host();
  const root = createRoot(react_host);
  let react_chart: chart_api | null = null;
  let react_pane: pane_api | null = null;
  let series_ready = 0;
  let reference_ready = 0;
  const compose_host = create_host();
  const direct_host = create_host();
  let compose_chart: chart_api | null = null;
  let direct_chart: chart_api | null = null;
  try {
    root.render(
      <AerisChart options={options_canvas} style={{ width: "100%", height: 560 }}
        onChartReady={(created) => { react_chart = created; }}>
        <FinancialSeries kind="candlestick" data={candles} />
        <GeneralPane options={{ horizontal_domain: domain }}
          onPaneReady={(created) => { react_pane = created; }}
          onSeriesReady={() => { series_ready += 1; }}
          onReferenceReady={() => { reference_ready += 1; }}>
          <GeneralAxis id="month" dimension="x" scale="band" position="bottom" />
          <GeneralAxis id="revenue" dimension="y" scale="linear" position="left" title="Revenue" />
          <MappedGeneralSeries seriesKey="revenue" kind="column"
            options={{ x_axis_id: "month", y_axis_id: "revenue", title: "Revenue" }}
            data={revenue} mapRow={map_revenue} />
          <GeneralDataLabels seriesKey="revenue" />
          <GeneralReference referenceKey="target"
            options={{ kind: "line", axis_id: "revenue", value: 50, color: "#f59e0b" }} />
          <GeneralLegend />
          <GeneralTooltip />
          <GeneralBrushControls options={{ axis: "month" }} />
        </GeneralPane>
      </AerisChart>,
    );
    await wait_until(() => react_chart !== null && react_pane !== null && series_ready === 1
      && reference_ready === 1);
    await frames(2);

    compose_chart = await createChart(compose_host, options_canvas);
    compose_chart.addSeries("candlestick").setData(candles);
    const composed_pane = compose_chart.addPane({ preserve_empty: true, horizontal_domain: domain });
    compose_chart.compose_general_pane({
      pane: composed_pane,
      axes: [
        { id: "month", dimension: "x", scale: "band", position: "bottom" },
        { id: "revenue", dimension: "y", scale: "linear", position: "left", title: "Revenue" },
      ],
      series: [{
        key: "revenue",
        kind: "column",
        options: { x_axis_id: "month", y_axis_id: "revenue", title: "Revenue", data_labels: true },
        data: revenue.map(map_revenue),
      }],
      references: [{
        key: "target",
        options: { kind: "line", axis_id: "revenue", value: 50, color: "#f59e0b" },
      }],
    });

    direct_chart = await createChart(direct_host, options_canvas);
    direct_chart.addSeries("candlestick").setData(candles);
    const direct_pane = direct_chart.addPane({ preserve_empty: true, horizontal_domain: domain }).pane_index();
    direct_chart.add_axis({ id: "month", pane: direct_pane, dimension: "x", scale: "band", position: "bottom" });
    direct_chart.add_axis({ id: "revenue", pane: direct_pane, dimension: "y", scale: "linear", position: "left",
      title: "Revenue" });
    direct_chart.add_series("column", { pane: direct_pane, x_axis_id: "month", y_axis_id: "revenue",
      title: "Revenue", data_labels: true }).set_data(revenue.map(map_revenue));
    direct_chart.add_general_reference({ kind: "line", pane: direct_pane, axis_id: "revenue", value: 50,
      color: "#f59e0b" });

    return {
      react: portable_pane_state(react_chart as chart_api, (react_pane as pane_api).pane_index()),
      composed: portable_pane_state(compose_chart, composed_pane.pane_index()),
      direct: portable_pane_state(direct_chart, direct_pane),
    };
  } finally {
    root.unmount();
    compose_chart?.remove();
    direct_chart?.remove();
    react_host.remove();
    compose_host.remove();
    direct_host.remove();
  }
}
