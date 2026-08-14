import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

import { base_environment, benchmark_root, compare_runs, load_manifest, public_summary, validate_run } from "../core.mjs";
import { parse_pack_manifest } from "../size.mjs";
import { dataset_metadata, generate_ohlcv, metric, percentile, summarize } from "../shared.mjs";

function fixture() {
  return {
    schema_version: 1,
    product: { name: "nucleuscharts-financial", version: "0.8.13" },
    source: { git_commit: "abc", git_branch: "main", git_tag: null, dirty_worktree: false },
    build: { profile: "release", logging: "default-no-verbose-debug", build_command: "npm run build", rustc_version: "rustc test", wasm_pack_version: "wasm-pack test", node_version: process.version, npm_version: "test", esbuild_version: "test", package_lock_sha256: "0".repeat(64), cargo_lock_sha256: "1".repeat(64), wasm_opt_args: ["-Oz"] },
    environment: { ...base_environment("official-benchmark-runner"), id: "official-test" },
    execution: { profile: "release", started_at: "2026-01-01T00:00:00.000Z", completed_at: "2026-01-01T00:01:00.000Z", clock: "performance.now monotonic in browser and Node; std::time::Instant monotonic in native Rust; wall time is metadata only", command: "benchmark release" },
    scenarios: [{
      id: "historical-candlestick-10k", version: 1, status: "passed", error: null,
      reproduction_command: "benchmark scenario historical-candlestick-10k",
      dataset: dataset_metadata(10_000, 7),
      execution: { warmup_runs: 1, measured_runs: 3, benchmark_duration_ms: 10, sampling_method: "test", forced_gc: false },
      capabilities: {},
      metrics: { set_data_api_ms: { availability: "measured", unit: "ms", direction: "lower_is_better", visibility: "public_candidate", methodology: "test", samples: [1, 2, 3], summary: summarize([1, 2, 3]) } },
    }],
  };
}

test("dataset generation is deterministic and preserves OHLC invariants", () => {
  const first = generate_ohlcv(100, 42);
  const second = generate_ohlcv(100, 42);
  assert.deepEqual([...first.close], [...second.close]);
  assert.notDeepEqual([...first.close], [...generate_ohlcv(100, 43).close]);
  for (let index = 0; index < first.times.length; index += 1) {
    assert.ok(first.high[index] >= first.open[index]);
    assert.ok(first.high[index] >= first.close[index]);
    assert.ok(first.low[index] <= first.open[index]);
    assert.ok(first.low[index] <= first.close[index]);
    assert.ok(first.high[index] >= first.low[index]);
  }
});

test("statistics use documented nearest-rank percentiles and retain outliers", () => {
  const sorted = [1, 2, 3, 4, 100];
  assert.equal(percentile(sorted, 0.50), 3);
  assert.equal(percentile(sorted, 0.95), 100);
  const summary = summarize(sorted);
  assert.equal(summary.max, 100);
  assert.equal(summary.count, 5);
  assert.ok(summary.standard_deviation > 0);
});

test("unsupported capabilities never acquire placeholder samples", () => {
  const unavailable = metric([], "ms", "lower_is_better", "public_candidate", "test capability", "unsupported");
  assert.deepEqual(unavailable.samples, []);
  assert.equal(unavailable.summary, null);
  assert.equal(unavailable.availability, "unsupported");
});

test("result validation rejects non-finite and negative duration samples", () => {
  assert.equal(validate_run(fixture()).schema_version, 1);
  const invalid = fixture();
  invalid.scenarios[0].metrics.set_data_api_ms.samples = [Number.NaN];
  assert.throws(() => validate_run(invalid), /non-finite/);
  const negative = fixture();
  negative.scenarios[0].metrics.set_data_api_ms.samples = [-1];
  assert.throws(() => validate_run(negative), /negative/);

  const unavailable_with_value = fixture();
  unavailable_with_value.scenarios[0].metrics.set_data_api_ms.availability = "unsupported";
  assert.throws(() => validate_run(unavailable_with_value), /unavailable metric carries a value/);

  const passed_without_metrics = fixture();
  passed_without_metrics.scenarios[0].metrics = {};
  assert.throws(() => validate_run(passed_without_metrics), /passed scenario has no metrics/);

  const inconsistent = fixture();
  inconsistent.scenarios[0].metrics.set_data_api_ms.summary.p50 = 999;
  assert.throws(() => validate_run(inconsistent), /inconsistent p50/);
});

test("comparison enforces scenario, dataset, and environment compatibility", () => {
  const baseline = fixture();
  const current = fixture();
  current.scenarios[0].metrics.set_data_api_ms = { ...current.scenarios[0].metrics.set_data_api_ms, samples: [2, 4, 6], summary: summarize([2, 4, 6]) };
  const compared = compare_runs(baseline, current, { thresholds: { "historical-candlestick-10k.set_data_api_ms.p50": { warning_percent: 20, fail_percent: 50 } } });
  assert.deepEqual(compared.budget_policy, { status: "ENFORCED", threshold_count: 1 });
  assert.equal(compared.comparisons[0].percentage_change, 100);
  assert.equal(compared.comparisons[0].status, "fail");
  const no_budget = compare_runs(baseline, baseline);
  assert.deepEqual(no_budget.budget_policy, { status: "NO ENFORCED BUDGET", threshold_count: 0 });
  current.scenarios[0].dataset.seed += 1;
  assert.equal(compare_runs(baseline, current).comparisons[0].status, "incompatible");
  current.scenarios[0].dataset.seed -= 1;
  current.environment.runtime_version = "different-browser-version";
  assert.equal(compare_runs(baseline, current).comparisons[0].environment_compatible, false);
  current.environment.runtime_version = baseline.environment.runtime_version;
  current.build.rustc_version = "different-toolchain";
  assert.equal(compare_runs(baseline, current).comparisons[0].build_compatible, false);
  assert.throws(() => compare_runs(baseline, baseline, { thresholds: { "historical-candlestick-10k.set_data_api_ms.p50": { warning_percent: 20, fail_percent: 10 } } }), /invalid warning\/failure budget/);
  assert.throws(() => compare_runs(baseline, baseline, { thresholds: { "misspelled.metric.p50": { warning_percent: 20, fail_percent: 50 } } }), /matched no comparable metric/);
});

test("public summary includes only measured public candidates from official clean releases", () => {
  const run = fixture();
  run.scenarios[0].metrics.internal_only = metric([7], "count", "informational", "internal", "test");
  run.scenarios[0].metrics.unsupported_public = metric([], "ms", "lower_is_better", "public_candidate", "test", "unsupported");
  const summary = public_summary(run);
  assert.equal(summary.release, "0.8.13");
  assert.ok(summary.scenarios["historical-candlestick-10k@1"].metrics.set_data_api_ms);
  assert.equal(summary.scenarios["historical-candlestick-10k@1"].metrics.internal_only, undefined);
  assert.equal(summary.scenarios["historical-candlestick-10k@1"].metrics.unsupported_public, undefined);
  const dirty = fixture();
  dirty.source.dirty_worktree = true;
  assert.throws(() => public_summary(dirty), /clean worktree/);
});

test("environment capture is privacy-safe and pack output is validated", () => {
  const environment = base_environment("local");
  assert.equal("hostname" in environment, false);
  assert.equal("username" in environment, false);
  assert.equal(parse_pack_manifest('[{"size":123,"unpackedSize":456}]').size, 123);
  assert.throws(() => parse_pack_manifest("[]"), /one package/);
});

test("scenario registry and JSON schema remain versioned and complete", async () => {
  const manifest = await load_manifest();
  assert.equal(manifest.manifest_version, 1);
  assert.equal(new Set(manifest.scenarios.map(({ id }) => id)).size, manifest.scenarios.length);
  for (const scenario of manifest.scenarios) {
    assert.ok(scenario.version >= 1);
    assert.ok(scenario.profiles.length > 0);
  }
  const schema = JSON.parse(await readFile(path.join(benchmark_root, "schema", "result-v1.schema.json"), "utf8"));
  assert.equal(schema.properties.schema_version.const, 1);
  assert.ok(schema.required.includes("environment"));
  assert.ok(schema.$defs.summary.required.includes("p95"));
  assert.equal(schema.$defs.dataset.properties.seed.maximum, 0xffff_ffff);
});
