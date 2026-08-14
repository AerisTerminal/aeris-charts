import { execFileSync } from "node:child_process";
import { repository_root } from "./core.mjs";
import { dataset_metadata, metric } from "./shared.mjs";

export function measure_native(scenario) {
  const started = performance.now();
  const output = execFileSync("cargo", ["run", "--quiet", "--release", "-p", "nucleuscharts_native", "--example", "evidence_bench"], { cwd: repository_root, encoding: "utf8", windowsHide: true, stdio: ["ignore", "pipe", "inherit"] });
  const raw = JSON.parse(output);
  const dataset = dataset_metadata(raw.points, raw.seed);
  dataset.configuration.scenario = { runtime_target: "native-rust-headless" };
  const metrics = {
    set_series_data_ms: metric(raw.set_series_data_ms, "ms", "lower_is_better", "internal", "Headless ChartEngine set_series_data plus shared time-scale fit; deterministic data is generated before timing."),
    build_frame_ms: metric(raw.build_frame_ms, "ms", "lower_is_better", "internal", "Retained ChartFrame build through the backend-neutral engine path after warm-up."),
    replace_current_candle_us: metric(raw.replace_current_candle_us, "us", "lower_is_better", "internal", "Headless current-candle replacement through ChartEngine update_series_bar."),
  };
  for (const row of raw.indicator_current) {
    metrics[`indicator_current_${row.points}_${row.indicator}_us`] = metric(row.samples_us, "us", "lower_is_better", "internal", `Current-bar replacement on ${row.points} source rows with ${row.indicator} indicators.`);
  }
  for (const row of raw.indicator_batch) {
    metrics[`indicator_batch_${row.points}_${row.indicator}_${row.batch_size}_us`] = metric(row.samples_us, "us", "lower_is_better", "internal", `One engine batch of ${row.batch_size} rows on ${row.points} source rows with ${row.indicator} indicators.`);
  }
  for (const row of raw.indicator_scaling) {
    metrics[`indicator_scaling_${row.indicator_count}_us`] = metric(row.samples_us, "us", "lower_is_better", "internal", `Current-bar replacement with ${row.indicator_count} mixed indicators on one 100K source.`);
  }
  for (const row of raw.indicator_source_scaling) {
    metrics[`indicator_source_scaling_${row.source_count}_us`] = metric(row.samples_us, "us", "lower_is_better", "internal", `Current-bar replacement on one of ${row.source_count} 10K sources with RSI outputs distributed across ${row.pane_count} panes.`);
  }
  for (const [name, value] of Object.entries(raw.dense_upload)) {
    const unit = name.endsWith("bytes") ? "bytes" : "count";
    metrics[`dense_upload_${name}`] = metric([value], unit, "informational", "internal", `Changed dense source-group ${name.replaceAll("_", " ")} after a 1M-bar current update.`);
  }
  for (const row of raw.drawing_density) {
    const prefix = `drawing_${row.drawing_count}_${row.distribution}`;
    metrics[`${prefix}_initial_ms`] = metric([row.initial_ms], "ms", "lower_is_better", "internal", "Initial semantic bounds, pane index, geometry, and retained drawing-frame construction.");
    metrics[`${prefix}_frame_ms`] = metric(row.frame_ms, "ms", "lower_is_better", "internal", "Coordinate-changing drawing frame rebuild through pane-local viewport candidates.");
    metrics[`${prefix}_hit_us`] = metric(row.hit_us, "us", "lower_is_better", "internal", "Pointer hit testing through cached conservative bounds and precise candidate arbitration.");
    metrics[`${prefix}_drag_ms`] = metric(row.drag_ms, "ms", "lower_is_better", "internal", "One-drawing semantic mutation, bounds/index update, and retained frame rebuild.");
    metrics[`${prefix}_pan_ms`] = metric(row.pan_ms, "ms", "lower_is_better", "internal", "Small alternating time-axis pan plus drawing candidate/frame work.");
    metrics[`${prefix}_zoom_ms`] = metric(row.zoom_ms, "ms", "lower_is_better", "internal", "Small alternating time-axis zoom plus drawing candidate/frame work.");
    for (const name of ["frame_candidates", "visible_drawings", "hit_candidates", "precise_hit_tests", "geometry_rebuilds", "drawing_runtime_capacity_bytes"]) {
      const unit = name.endsWith("bytes") ? "bytes" : "count";
      metrics[`${prefix}_${name}`] = metric([row[name]], unit, "informational", "internal", `Drawing-density ${name.replaceAll("_", " ")}.`);
    }
  }
  return {
    id: scenario.id,
    version: scenario.version,
    status: "passed",
    error: null,
    reproduction_command: "node benchmarks/benchmark.mjs native",
    dataset,
    execution: { warmup_runs: raw.warmup_runs, measured_runs: raw.measured_runs, benchmark_duration_ms: performance.now() - started, sampling_method: "Rust std::time::Instant around headless engine operations in an optimized release build; outer duration includes Cargo's up-to-date check and process startup", forced_gc: false },
    capabilities: { runtime_target: "native-rust-headless", gpu: false, presentation: false },
    metrics,
  };
}
