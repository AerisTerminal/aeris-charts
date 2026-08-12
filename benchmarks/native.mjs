import { execFileSync } from "node:child_process";
import { repository_root } from "./core.mjs";
import { dataset_metadata, metric } from "./shared.mjs";

export function measure_native(scenario) {
  const started = performance.now();
  const output = execFileSync("cargo", ["run", "--quiet", "--release", "-p", "nucleuscharts_native", "--example", "evidence_bench"], { cwd: repository_root, encoding: "utf8", windowsHide: true, stdio: ["ignore", "pipe", "inherit"] });
  const raw = JSON.parse(output);
  const dataset = dataset_metadata(raw.points, raw.seed);
  dataset.configuration.scenario = { runtime_target: "native-rust-headless" };
  return {
    id: scenario.id,
    version: scenario.version,
    status: "passed",
    error: null,
    reproduction_command: "node benchmarks/benchmark.mjs native",
    dataset,
    execution: { warmup_runs: raw.warmup_runs, measured_runs: raw.measured_runs, benchmark_duration_ms: performance.now() - started, sampling_method: "Rust std::time::Instant around headless engine operations in an optimized release build; outer duration includes Cargo's up-to-date check and process startup", forced_gc: false },
    capabilities: { runtime_target: "native-rust-headless", gpu: false, presentation: false },
    metrics: {
      set_series_data_ms: metric(raw.set_series_data_ms, "ms", "lower_is_better", "internal", "Headless ChartEngine set_series_data plus shared time-scale fit; deterministic data is generated before timing."),
      build_frame_ms: metric(raw.build_frame_ms, "ms", "lower_is_better", "internal", "Retained ChartFrame build through the backend-neutral engine path after warm-up."),
      replace_current_candle_us: metric(raw.replace_current_candle_us, "us", "lower_is_better", "internal", "Headless current-candle replacement through ChartEngine update_series_bar."),
    },
  };
}
