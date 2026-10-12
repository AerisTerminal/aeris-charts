import {
  Children,
  Fragment,
  createContext,
  createElement,
  isValidElement,
  useContext,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";

import {
  createChart,
  create_general_brush_controls,
  create_general_legend,
  create_general_tooltip,
} from "./index.js";
import type {
  general_brush_controls_options,
  general_legend_options,
  general_tooltip_options,
} from "./general_components.js";
import type {
  any_series_options,
  chart_api,
  chart_options,
  deep_partial,
  general_axis_api,
  general_axis_options,
  general_brush_range,
  general_brush_snapshot,
  general_pane_options,
  general_reference_api,
  general_reference_options,
  general_series_api,
  general_series_kind,
  general_series_options,
  pane_api,
  series_api,
  series_data,
  series_kind,
} from "./types.js";

const chart_context = createContext<chart_api | null>(null);
const chart_host_context = createContext<HTMLElement | null>(null);
const general_pane_context = createContext<pane_api | null>(null);

/** Return the live Aeris chart owned by the nearest {@link AerisChart}. */
export function useAerisChart(): chart_api {
  const chart = useContext(chart_context);
  if (chart === null) throw new Error("useAerisChart must be used inside <AerisChart>");
  return chart;
}

export interface AerisChartProps {
  options?: deep_partial<chart_options>;
  className?: string;
  style?: CSSProperties;
  children?: ReactNode;
  onChartReady?: (chart: chart_api) => void;
}

/**
 * React lifecycle adapter over the ordinary browser chart. The chart is created once per mounted
 * component, option changes are applied to the same handle, and cleanup calls the canonical
 * `chart.remove()` path. No DOM work runs during server rendering.
 */
export function AerisChart({ options, className, style, children, onChartReady }: AerisChartProps) {
  const host_ref = useRef<HTMLDivElement | null>(null);
  const on_ready_ref = useRef(onChartReady);
  const initial_options_ref = useRef(options);
  const [chart, set_chart] = useState<chart_api | null>(null);
  const [failure, set_failure] = useState<unknown>(null);
  on_ready_ref.current = onChartReady;

  useEffect(() => {
    const host = host_ref.current;
    if (host === null) return;
    let live = true;
    let owned: chart_api | null = null;

    void createChart(host, initial_options_ref.current).then((created) => {
      if (!live) {
        created.remove();
        return;
      }
      owned = created;
      try {
        on_ready_ref.current?.(created);
        set_chart(created);
      } catch (error) {
        owned = null;
        created.remove();
        set_failure(error);
      }
    }, (error: unknown) => {
      if (live) set_failure(error);
    });

    return () => {
      live = false;
      owned?.remove();
    };
  }, []);

  useEffect(() => {
    if (chart !== null && options !== undefined) chart.applyOptions(options);
  }, [chart, options]);

  if (failure !== null) throw failure;

  const contents = chart === null
    ? null
    : createElement(chart_context.Provider, { value: chart },
      createElement(chart_host_context.Provider, { value: host_ref.current }, children));
  return createElement("div", {
    ref: host_ref,
    className,
    style: { position: "relative", ...style },
  }, contents);
}

export type react_financial_series_kind = Exclude<series_kind, "custom">;

export interface FinancialSeriesProps {
  kind: react_financial_series_kind;
  data: readonly series_data[];
  options?: Partial<any_series_options>;
  onSeriesReady?: (series: series_api) => void;
}

/** A financial series whose engine identity is retained across ordinary React rerenders. */
export function FinancialSeries({ kind, data, options, onSeriesReady }: FinancialSeriesProps) {
  const chart = useAerisChart();
  const options_ref = useRef(options);
  const data_ref = useRef(data);
  const installed_data_ref = useRef<readonly series_data[] | null>(null);
  const active_series_ref = useRef<series_api | null>(null);
  const ready_ref = useRef(onSeriesReady);
  const [series, set_series] = useState<series_api | null>(null);
  options_ref.current = options;
  data_ref.current = data;
  ready_ref.current = onSeriesReady;

  useEffect(() => {
    const created = chart.addSeries(kind as series_kind, options_ref.current);
    try {
      created.setData(data_ref.current);
      installed_data_ref.current = data_ref.current;
      ready_ref.current?.(created);
    } catch (error) {
      chart.removeSeries(created);
      throw error;
    }
    active_series_ref.current = created;
    set_series(created);
    return () => {
      if (active_series_ref.current === created) active_series_ref.current = null;
      chart.removeSeries(created);
    };
  }, [chart, kind]);

  useEffect(() => {
    if (series !== null && active_series_ref.current === series && installed_data_ref.current !== data) {
      series.setData(data);
      installed_data_ref.current = data;
    }
  }, [series, data]);

  useEffect(() => {
    if (series !== null && active_series_ref.current === series && options !== undefined) series.applyOptions(options);
  }, [series, options]);

  return null;
}

export type GeneralAxisSpec = Omit<general_axis_options, "pane">;

export interface GeneralSeriesSpec {
  /** Stable React-side identity. Changing this value creates a different engine series. */
  key: string;
  kind: general_series_kind;
  options: Omit<general_series_options, "pane">;
  data: Parameters<general_series_api["set_data"]>[0];
}

type GeneralRow = GeneralSeriesSpec["data"][number];

/** An axis declaration owned by its nearest GeneralPane. */
export function GeneralAxis(_props: GeneralAxisSpec): null {
  return null;
}

/** A canonical general series declaration owned by its nearest GeneralPane. */
export interface GeneralSeriesDeclarationProps extends Omit<GeneralSeriesSpec, "key"> {
  /** Stable engine identity. React's reserved `key` is not available to this declaration. */
  seriesKey: string;
}

export function GeneralSeries(_props: GeneralSeriesDeclarationProps): null {
  return null;
}

export interface MappedGeneralSeriesProps<Row> extends Omit<GeneralSeriesDeclarationProps, "data"> {
  /** Source rows are mapped once per changed data or mapper identity, outside frame work. */
  data: readonly Row[];
  mapRow: (row: Row, index: number) => GeneralRow;
}

/** Map application records to canonical rows before installing them in the engine. */
export function MappedGeneralSeries<Row>(_props: MappedGeneralSeriesProps<Row>): null {
  return null;
}

export interface GeneralReferenceSpec {
  key: string;
  options: general_reference_options extends infer Option
    ? Option extends general_reference_options ? Omit<Option, "pane"> : never
    : never;
}

export interface GeneralReferenceDeclarationProps extends Omit<GeneralReferenceSpec, "key"> {
  /** Stable engine identity; React's reserved `key` is not available to this declaration. */
  referenceKey: string;
}

/** A shared-frame line, dot, or region declaration owned by its nearest GeneralPane. */
export function GeneralReference(_props: GeneralReferenceDeclarationProps): null {
  return null;
}

/** Show the engine's bounded frame labels for one declared series. Row `label` data supplies text. */
export function GeneralDataLabels(_props: { seriesKey: string; visible?: boolean }): null {
  return null;
}

/** Default accessible legend for the nearest GeneralPane. */
export function GeneralLegend({ options = {} }: { options?: Omit<general_legend_options, "pane"> }) {
  const chart = useAerisChart();
  const pane = useContext(general_pane_context);
  const container = useRef<HTMLDivElement | null>(null);
  const signature = stable_signature(options);
  useEffect(() => {
    if (pane === null || container.current === null) return;
    const component = create_general_legend(chart, container.current, {
      ...options,
      pane: pane.pane_index(),
    });
    return () => component.detach();
  }, [chart, pane, signature, options.format_action, options.format_title]);
  return createElement("div", { ref: container, style: {
    position: "absolute", top: 8, left: 8, zIndex: 6, maxWidth: "calc(100% - 16px)",
  } });
}

/** Default engine-hit tooltip mounted on the chart host. */
export function GeneralTooltip({ options = {} }: { options?: general_tooltip_options }) {
  const chart = useAerisChart();
  const host = useContext(chart_host_context);
  const pane = useContext(general_pane_context);
  const signature = stable_signature(options);
  useEffect(() => {
    if (host === null) return;
    const component = create_general_tooltip(chart, host, {
      ...options,
      pane: pane?.pane_index() ?? options.pane,
    });
    return () => component.detach();
  }, [chart, host, pane, signature, options.format_value, options.format_detail]);
  return null;
}

export interface GeneralBrushControlsProps {
  options: general_brush_controls_options;
  /** Defined values are controlled; `null` clears the range. */
  range?: general_brush_range | null;
  /** Initial range for uncontrolled use. */
  defaultRange?: general_brush_range | null;
  onChange?: (snapshot: general_brush_snapshot | null) => void;
}

/** Native-button and touch controls over the engine's semantic brush. */
export function GeneralBrushControls({ options, range, defaultRange, onChange }: GeneralBrushControlsProps) {
  const chart = useAerisChart();
  const container = useRef<HTMLDivElement | null>(null);
  const axis_id = typeof options.axis === "string" ? options.axis : options.axis.id;
  const signature = stable_signature({ ...options, axis: axis_id });
  const range_signature = stable_signature(range);
  const range_ref = useRef(range);
  const on_change_ref = useRef(onChange);
  const updating_ref = useRef(false);
  range_ref.current = range;
  on_change_ref.current = onChange;
  const apply_range = (next: general_brush_range | null): void => {
    updating_ref.current = true;
    try {
      if (next === null) chart.clear_general_brush();
      else chart.set_general_brush_range(axis_id, next);
    } finally {
      updating_ref.current = false;
    }
  };
  useEffect(() => {
    if (container.current === null) return;
    const component = create_general_brush_controls(chart, container.current, options);
    if (range_ref.current !== undefined) apply_range(range_ref.current);
    else if (defaultRange !== undefined) apply_range(defaultRange);
    const changed = (): void => {
      if (updating_ref.current) return;
      on_change_ref.current?.(chart.general_brush_snapshot());
      if (range_ref.current !== undefined) apply_range(range_ref.current);
    };
    chart.subscribe_general_brush_change(changed);
    return () => {
      chart.unsubscribe_general_brush_change(changed);
      component.detach();
    };
  }, [chart, signature]);
  useEffect(() => {
    if (range !== undefined) apply_range(range);
  }, [chart, axis_id, range_signature]);
  return createElement("div", { ref: container, style: {
    position: "absolute", bottom: 8, left: 8, zIndex: 6, maxWidth: "calc(100% - 16px)",
  } });
}

interface mapped_series_spec {
  seriesKey: string;
  kind: general_series_kind;
  options: GeneralSeriesSpec["options"];
  data: readonly unknown[];
  mapRow: (row: never, index: number) => GeneralRow;
}

function declared_general_items(children: ReactNode): {
  axes: GeneralAxisSpec[];
  series: GeneralSeriesSpec[];
  mapped: mapped_series_spec[];
  references: GeneralReferenceSpec[];
  controls: ReactNode[];
  labels: { seriesKey: string; visible: boolean }[];
} {
  const axes: GeneralAxisSpec[] = [];
  const series: GeneralSeriesSpec[] = [];
  const mapped: mapped_series_spec[] = [];
  const references: GeneralReferenceSpec[] = [];
  const controls: ReactNode[] = [];
  const labels: { seriesKey: string; visible: boolean }[] = [];
  const visit = (nodes: ReactNode): void => {
    Children.forEach(nodes, (node) => {
      if (node == null || typeof node === "boolean") return;
      if (!isValidElement(node)) throw new Error("GeneralPane children must be general declarations");
      if (node.type === Fragment) {
        visit((node.props as { children?: ReactNode }).children);
      } else if (node.type === GeneralAxis) {
        axes.push(node.props as GeneralAxisSpec);
      } else if (node.type === GeneralSeries) {
        const { seriesKey, ...spec } = node.props as GeneralSeriesDeclarationProps;
        series.push({ ...spec, key: seriesKey });
      } else if (node.type === MappedGeneralSeries) {
        mapped.push(node.props as mapped_series_spec);
      } else if (node.type === GeneralReference) {
        const { referenceKey, options } = node.props as GeneralReferenceDeclarationProps;
        references.push({ key: referenceKey, options });
      } else if (node.type === GeneralDataLabels) {
        const props = node.props as { seriesKey: string; visible?: boolean };
        labels.push({ seriesKey: props.seriesKey, visible: props.visible ?? true });
      } else if (
        node.type === GeneralLegend || node.type === GeneralTooltip || node.type === GeneralBrushControls
      ) {
        controls.push(node);
      } else {
        throw new Error("GeneralPane received an unsupported child declaration");
      }
    });
  };
  visit(children);
  return { axes, series, mapped, references, controls, labels };
}

export interface GeneralPaneProps {
  /** The horizontal-domain contract. The adapter retains this pane while mounted, even when empty. */
  options: Omit<general_pane_options, "preserve_empty">;
  axes?: readonly GeneralAxisSpec[];
  series?: readonly GeneralSeriesSpec[];
  references?: readonly GeneralReferenceSpec[];
  /** Axis and series declarations may be supplied as direct children or inside Fragments. */
  children?: ReactNode;
  onPaneReady?: (pane: pane_api) => void;
  onSeriesReady?: (key: string, series: general_series_api) => void;
  onReferenceReady?: (key: string, reference: general_reference_api) => void;
  /** Handle mapping or axis validation failure while retaining the previously committed pane. */
  onValidationError?: (error: unknown) => void;
}

interface axis_runtime {
  handle: general_axis_api;
  structure_signature: string;
  presentation_signature: string;
}

interface series_runtime {
  handle: general_series_api;
  binding_signature: string;
  presentation_signature: string;
  data: GeneralSeriesSpec["data"];
}

interface pane_runtime {
  pane: pane_api;
  axes: Map<string, axis_runtime>;
  series: Map<string, series_runtime>;
  references: Map<string, { handle: general_reference_api; signature: string }>;
}

function stable_signature(value: unknown): string {
  return JSON.stringify(value, (_key, item: unknown) => item instanceof Date ? item.getTime() : item);
}

function general_series_bindings(options: GeneralSeriesSpec["options"]): object {
  return { x_axis_id: options.x_axis_id, y_axis_id: options.y_axis_id };
}

function general_axis_structure(axis: GeneralAxisSpec): object {
  return { dimension: axis.dimension, scale: axis.scale };
}

function general_axis_presentation(
  axis: GeneralAxisSpec,
): Omit<GeneralAxisSpec, "id" | "dimension" | "scale"> {
  const { id, dimension, scale, ...presentation } = axis;
  void id;
  void dimension;
  void scale;
  return presentation;
}

function general_series_presentation(
  options: GeneralSeriesSpec["options"],
): Omit<GeneralSeriesSpec["options"], "x_axis_id" | "y_axis_id"> {
  const { x_axis_id, y_axis_id, ...presentation } = options;
  void x_axis_id;
  void y_axis_id;
  return presentation;
}

function dispose_pane_runtime(chart: chart_api, runtime: pane_runtime): void {
  try {
    chart.backend();
  } catch {
    // React may dispose the parent chart before child effect cleanup under Strict Mode/unmount.
    return;
  }
  for (const item of runtime.series.values()) {
    item.handle.remove();
  }
  runtime.series.clear();
  for (const item of runtime.references.values()) item.handle.remove();
  runtime.references.clear();
  for (const item of Array.from(runtime.axes.values()).reverse()) {
    if (!item.handle.remove()) throw new Error(`general axis ${item.handle.id} could not be removed during cleanup`);
  }
  runtime.axes.clear();
  const removed = chart.remove_pane(runtime.pane.pane_index());
  if (!removed) throw new Error("general pane could not be removed during cleanup");
}

/**
 * Own one general-domain pane, its axes, and its general series. Axes and series are reconciled by
 * stable IDs/keys; unchanged series keep their engine identity and a changed data reference calls
 * `set_data` on that same handle. Compatible series binding changes retain handles; structural axis
 * or kind changes replace only the affected general series, never the chart.
 */
export function GeneralPane({ options, axes: axis_specs = [], series: series_specs = [], references: reference_specs = [], children, onPaneReady, onSeriesReady, onReferenceReady, onValidationError }: GeneralPaneProps) {
  const chart = useAerisChart();
  const runtime_ref = useRef<pane_runtime | null>(null);
  const mapped_cache_ref = useRef(new Map<string, {
    input: readonly unknown[];
    mapper: mapped_series_spec["mapRow"];
    data: GeneralSeriesSpec["data"];
  }>());
  const ready_ref = useRef(onPaneReady);
  const series_ready_ref = useRef(onSeriesReady);
  const reference_ready_ref = useRef(onReferenceReady);
  const validation_error_ref = useRef(onValidationError);
  const [generation, set_generation] = useState(0);
  const domain_signature = stable_signature(options.horizontal_domain);
  const declared = declared_general_items(children);
  const axes = [...axis_specs, ...declared.axes];
  const series = [...series_specs, ...declared.series, ...declared.mapped.map((item) => ({
    key: item.seriesKey,
    kind: item.kind,
    options: item.options,
    data: item.data as GeneralSeriesSpec["data"],
  }))];
  const label_keys = new Set<string>();
  for (const label of declared.labels) {
    if (label_keys.has(label.seriesKey)) throw new Error(`duplicate general data labels for ${label.seriesKey}`);
    label_keys.add(label.seriesKey);
    const index = series.findIndex((item) => item.key === label.seriesKey);
    if (index < 0) throw new Error(`general data labels reference missing series ${label.seriesKey}`);
    const item = series[index];
    if (item === undefined) throw new Error(`general data labels reference missing series ${label.seriesKey}`);
    series[index] = { ...item, options: { ...item.options, data_labels: label.visible } };
  }
  const mapped_by_key = new Map(declared.mapped.map((item) => [item.seriesKey, item]));
  const references = [...reference_specs, ...declared.references];
  ready_ref.current = onPaneReady;
  series_ready_ref.current = onSeriesReady;
  reference_ready_ref.current = onReferenceReady;
  validation_error_ref.current = onValidationError;

  const axis_ids = new Set<string>();
  for (const axis of axes) {
    if (axis_ids.has(axis.id)) throw new Error(`duplicate general axis id: ${axis.id}`);
    axis_ids.add(axis.id);
  }
  const series_keys = new Set<string>();
  for (const item of series) {
    if (series_keys.has(item.key)) throw new Error(`duplicate general series key: ${item.key}`);
    series_keys.add(item.key);
  }
  const reference_keys = new Set<string>();
  for (const item of references) {
    if (reference_keys.has(item.key)) throw new Error(`duplicate general reference key: ${item.key}`);
    reference_keys.add(item.key);
  }

  useEffect(() => {
    const pane = chart.addPane({ preserve_empty: true, horizontal_domain: options.horizontal_domain });
    const runtime: pane_runtime = { pane, axes: new Map(), series: new Map(), references: new Map() };
    runtime_ref.current = runtime;
    try {
      ready_ref.current?.(pane);
    } catch (error) {
      runtime_ref.current = null;
      dispose_pane_runtime(chart, runtime);
      throw error;
    }
    set_generation((value) => value + 1);
    return () => {
      if (runtime_ref.current === runtime) runtime_ref.current = null;
      dispose_pane_runtime(chart, runtime);
    };
    // A horizontal-domain change is structural and intentionally replaces this one pane.
  }, [chart, domain_signature]);

  useEffect(() => {
    const runtime = runtime_ref.current;
    if (runtime === null) return;
    const pane = runtime.pane.pane_index();
    const desired_axes = new Map(axes.map((axis) => [axis.id, axis] as const));
    const desired_series = new Map(series.map((item) => [item.key, item] as const));
    const desired_references = new Map(references.map((item) => [item.key, item] as const));
    const desired_data = new Map<string, GeneralSeriesSpec["data"]>();
    const changed_axes = new Set<string>();
    try {
      for (const key of mapped_cache_ref.current.keys()) {
        if (!mapped_by_key.has(key)) mapped_cache_ref.current.delete(key);
      }
      for (const desired of series) {
        const mapped = mapped_by_key.get(desired.key);
        if (mapped === undefined) {
          desired_data.set(desired.key, desired.data);
          continue;
        }
        const cached = mapped_cache_ref.current.get(desired.key);
        const data = cached?.input === mapped.data && cached.mapper === mapped.mapRow
          ? cached.data
          : mapped.data.map((row, index) => mapped.mapRow(row as never, index));
        mapped_cache_ref.current.set(desired.key, {
          input: mapped.data,
          mapper: mapped.mapRow,
          data,
        });
        desired_data.set(desired.key, data);
      }
      chart.validate_general_pane_proposal(
        pane,
        axes,
        series.map((desired) => ({
          kind: desired.kind,
          options: desired.options,
          data: desired_data.get(desired.key) ?? desired.data,
        })),
        references.map((reference) => ({ ...reference.options, pane }) as general_reference_options),
      );
      for (const [id, current] of runtime.axes) {
        const desired = desired_axes.get(id);
        if (desired === undefined
          || stable_signature(general_axis_structure(desired)) !== current.structure_signature) {
          changed_axes.add(id);
        }
      }
    } catch (error) {
      if (validation_error_ref.current === undefined) throw error;
      validation_error_ref.current(error);
      return;
    }
    for (const [id, current] of runtime.axes) {
      const desired = desired_axes.get(id);
      if (!changed_axes.has(id) && desired !== undefined) {
        const presentation = general_axis_presentation(desired);
        const presentation_signature = stable_signature(presentation);
        if (presentation_signature !== current.presentation_signature) {
          current.handle.applyOptions(presentation);
          current.presentation_signature = presentation_signature;
        }
      }
    }

    for (const [key, current] of runtime.series) {
      const desired = desired_series.get(key);
      const depends_on_changed_axis = desired !== undefined
        && (changed_axes.has(desired.options.x_axis_id) || changed_axes.has(desired.options.y_axis_id));
      const changed = desired === undefined
        || desired.kind !== current.handle.kind
        || depends_on_changed_axis;
      if (changed) {
        current.handle.remove();
        runtime.series.delete(key);
      } else {
        const binding_signature = stable_signature(general_series_bindings(desired.options));
        const presentation = general_series_presentation(desired.options);
        const presentation_signature = stable_signature(presentation);
        if (
          binding_signature !== current.binding_signature
          || presentation_signature !== current.presentation_signature
        ) {
          current.handle.applyOptions({
            ...presentation,
            pane,
            x_axis_id: desired.options.x_axis_id,
            y_axis_id: desired.options.y_axis_id,
          });
          current.binding_signature = binding_signature;
          current.presentation_signature = presentation_signature;
        }
      }
    }

    for (const [key, current] of runtime.references) {
      const desired = desired_references.get(key);
      const bound_to_changed_axis = desired !== undefined && (
        "axis_id" in desired.options
          ? changed_axes.has(desired.options.axis_id)
          : changed_axes.has(desired.options.x_axis_id) || changed_axes.has(desired.options.y_axis_id)
      );
      if (desired === undefined || bound_to_changed_axis) {
        current.handle.remove();
        runtime.references.delete(key);
      }
    }

    for (const id of changed_axes) {
      const current = runtime.axes.get(id);
      if (current !== undefined) {
        if (!current.handle.remove()) throw new Error(`general axis ${id} could not be reconciled`);
        runtime.axes.delete(id);
      }
    }

    for (const axis of axes) {
      if (runtime.axes.has(axis.id)) continue;
      const handle = chart.addAxis({ ...axis, pane });
      runtime.axes.set(axis.id, {
        handle,
        structure_signature: stable_signature(general_axis_structure(axis)),
        presentation_signature: stable_signature(general_axis_presentation(axis)),
      });
    }

    for (const desired of series) {
      let current = runtime.series.get(desired.key);
      const data = desired_data.get(desired.key);
      if (data === undefined) throw new Error(`general series ${desired.key} has no data`);
      if (current === undefined) {
        const handle = chart.addSeries(desired.kind, { ...desired.options, pane });
        try {
          handle.setData(data);
          current = {
            handle,
            binding_signature: stable_signature(general_series_bindings(desired.options)),
            presentation_signature: stable_signature(general_series_presentation(desired.options)),
            data,
          };
          runtime.series.set(desired.key, current);
          series_ready_ref.current?.(desired.key, handle);
        } catch (error) {
          runtime.series.delete(desired.key);
          handle.remove();
          throw error;
        }
      } else if (current.data !== data) {
        current.handle.setData(data);
        current.data = data;
      }
    }

    for (const desired of references) {
      const options = { ...desired.options, pane } as general_reference_options;
      const signature = stable_signature(options);
      const current = runtime.references.get(desired.key);
      if (current === undefined) {
        const handle = chart.add_general_reference(options);
        runtime.references.set(desired.key, { handle, signature });
        try {
          reference_ready_ref.current?.(desired.key, handle);
        } catch (error) {
          runtime.references.delete(desired.key);
          handle.remove();
          throw error;
        }
      } else if (current.signature !== signature) {
        current.handle.apply_options(options);
        current.signature = signature;
      }
    }

    const desired_order = series
      .map((item) => runtime.series.get(item.key)?.handle)
      .filter((handle): handle is general_series_api => handle !== undefined);
    const current_order = chart.general_series_order(pane);
    if (
      desired_order.length === current_order.length
      && desired_order.some((handle, index) => handle !== current_order[index])
    ) {
      chart.set_general_series_order(desired_order, pane);
    }
  }, [chart, generation, axes, series, references]);

  const pane = runtime_ref.current?.pane;
  return pane === undefined ? null : createElement(
    general_pane_context.Provider,
    { value: pane },
    declared.controls,
  );
}
