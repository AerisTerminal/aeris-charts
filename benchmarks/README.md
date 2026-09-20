# Nucleus Charts evidence benchmarks

This subsystem is the source of truth for Nucleus Charts performance, artifact-size, and memory claims. It measures the production `@axiusflowhq/financial` package through its public browser API and keeps every number tied to source, environment, scenario, dataset, and raw samples. It does not optimize the product and it does not manufacture unsupported values.

## Requirements

- The repository's configured Rust toolchain and `wasm-pack` 0.15.0 for the production WASM build.
- Node.js 18 or newer.
- Chromium installed for Playwright (`cd examples/web_demo && npx playwright install chromium`).
- For official release results, a clean checkout on a controlled runner with `NUCLEUSCHARTS_BENCH_ENV_CLASS=official-benchmark-runner` and a stable `NUCLEUSCHARTS_BENCH_ENV_ID`.

The CLI runs `npm ci` when the package or demo dependencies are absent. Browser scenarios reuse the existing demo server and Playwright dependency. No benchmark package is shipped to consumers.

## Commands

```text
node benchmarks/benchmark.mjs test
node benchmarks/benchmark.mjs smoke
node benchmarks/benchmark.mjs nightly
node benchmarks/benchmark.mjs release
node benchmarks/benchmark.mjs soak --duration-ms 600000
node benchmarks/benchmark.mjs scenario pan-candlestick-100k
node benchmarks/benchmark.mjs size
node benchmarks/benchmark.mjs native
node benchmarks/benchmark.mjs compare <baseline.json> <current.json>
node benchmarks/benchmark.mjs report <result.json>
node benchmarks/benchmark.mjs public <official-release-result.json>
node benchmarks/benchmark.mjs baseline <official-release-result.json>
node benchmarks/benchmark.mjs claims
```

`release` includes the configured 60-minute soak and refuses a dirty worktree. `--duration-ms` may shorten a local validation run, but a changed duration is part of the scenario configuration and must not be compared as if it were the canonical release run.

## Profiles and authority

- `smoke` is fast shared-CI validation. It detects broken scenarios and gross regressions but is never public evidence.
- `nightly` collects broader shared-runner trends. Its absolute timings are non-authoritative.
- `release` is the full clean-worktree suite for a controlled machine. Only this profile on an `official-benchmark-runner` can generate `benchmark-public.json` or become a baseline.
- `soak` runs the long-lived workload alone. The canonical release duration is 60 minutes; a 10-minute smoke or 30-minute engineering run is selected explicitly with `--duration-ms`.

Environment classification defaults to `local`; shared workflows set `shared-ci`. Set a stable environment ID, for example `official-win-nvidia-01`. The result captures OS, architecture, CPU, logical cores, total RAM, runtime/browser, viewport, DPR, and GPU information where applicable. Viewport and DPR are `null` for size-only and native runs; unknown GPU driver, physical-core, and refresh-rate values also remain `null`. Build provenance records the exact package or native build command, Rust, wasm-pack, Node, npm, and esbuild versions, Cargo/package lock hashes, and applicable wasm-opt arguments. Hostname, username, home directory, IP addresses, environment variables, and local repository paths are never serialized.

## Deterministic data and scenarios

`shared.mjs` uses a versioned xorshift32 generator. A generator version, unsigned seed, start time, interval, start price, volatility, point count, series count, and pane count identify the dataset. Generated rows always satisfy `high >= open/close`, `low <= open/close`, and `high >= low`. Core sizes are 1K, 10K, 100K, 500K, and 1M candlesticks. Dataset creation occurs before timed installs.

Canonical scenario definitions live in `scenarios.json`; this is intentionally a small manifest rather than a benchmark DSL. Scenario IDs and versions make methodology changes explicit. A material change creates a new scenario version instead of silently rewriting history. Workloads cover startup, historical loading, current-candle and append streaming, pan/zoom/crosshair input, lifecycle retention, multi-chart, multi-series/multi-pane scaling, retained current-candle updates across 1/2/4/8/16 series and one/four panes, and soak stability. Retained scenarios record semantic rebuild counts plus WebGPU allocation, write, and upload volume alongside frame CPU percentiles. Native evidence additionally measures every representative indicator on 10K/100K/1M histories, typed-equivalent batches of 1/10/100/1K/10K rows, 1/4/8/16 mixed indicators on one source, and one active source across 1/2/4/8/16 source/indicator-pane pairs.

## Timing and statistics

Short durations use `performance.now()` in the browser and Node's monotonic timing facilities outside it. Wall time is metadata only. Cold startup uses a fresh browser context per sample. Steady-state scenarios declare warm-up and measured runs independently.

Raw samples are retained. `shared.mjs` is the sole statistical implementation: min, max, arithmetic mean, p50, p90, p95, p99, population standard deviation, and count. Percentiles use nearest rank: sort ascending and select `ceil(p * n)`, clamped to the first element. No outliers are removed.

`set_data_api_ms` measures the public synchronous typed-data call, which includes validation, engine install, frame construction, command encoding/submission, and the backend present call. `first_raf_after_*` ends at the next browser animation-frame callback and is labeled as a compositor opportunity, not photon-visible proof. The system records actual presentation counters exposed by the engine but reports visible presentation latency as unsupported because the browser supplies no reliable completion timestamp here.

Interaction traces use Playwright pointer and wheel input on the package's top overlay canvas. `raf_frame_interval_ms` includes display cadence and browser scheduling; `frame_cpu_ms` is the existing engine-to-command-encoding measurement. Effective FPS is derived from observed rAF callbacks and is always presented with frame durations and refresh-rate limitations.

## GPU and CPU methodology

On WebGPU, `gpu_render_pass_ms` is accepted only when the existing `frame_stats().gpu_ms` produces a resolved hardware timestamp-query sample. The value spans the WebGPU render pass and is asynchronously read back. On Canvas2D or an adapter without `timestamp-query`, it is `unsupported`, never zero and never replaced with CPU submission time.

Browser CPU is the Chromium DevTools `Performance.TaskDuration` delta across a scenario. It is a page main-thread task measurement, not whole-system CPU and not solely attributable to Nucleus Charts. Frame CPU comes from the existing bounded WASM telemetry record.

## Memory and lifecycle methodology

Memory labels stay distinct:

- `wasm_linear_memory_bytes` is reserved WASM linear memory. It is global to the module, grows in 64 KiB pages, does not shrink, and is not total RAM.
- `browser_js_heap_used_bytes` is Chromium's whole-page JavaScript heap after a documented DevTools GC.
- `browser_page_memory_*` uses `measureUserAgentSpecificMemory` when available and is whole-page memory, not exact Nucleus ownership.

Lifecycle scenarios first complete and discard one create/load/render/destroy warm-up so WASM initialization and initial allocator growth precede the baseline. They then repeat the same deterministic fixture, recording WASM linear memory on every loaded cycle. Because whole-page memory collection is disruptive, smoke takes one loaded sample while 50- and 100-cycle profiles take five and ten evenly spaced loaded samples; initial and final samples bracket every run. Results record whether GC was forced. Retained delta is final whole-page memory minus the warmed initial baseline; retained delta per cycle divides that value by completed cycles. Unsupported page-memory APIs remain explicit. Multi-chart and multi-series scaling load every configuration in a fresh page realm so non-shrinking WASM linear-memory high-water marks remain comparable; fixture creation and disruptive page-memory collection stay outside startup timing.

Soak sampling performs continuous current-candle updates, periodic appends, time-scale movement, crosshair movement, and periodic sampling without per-frame file writes. Lightweight frame, update-latency, and WASM-memory samples are taken every second. The more disruptive whole-page memory API is sampled once per minute so it does not dominate the workload. The result reports first-to-last memory growth normalized per hour and second-half versus first-half frame CPU degradation when enough observations exist; otherwise that derived metric is explicitly unsupported. Raw periodic samples remain in the result.

## Artifact sizes

The size scenario runs the same production `npm run build` used before publication and reads `npm pack --json --dry-run`. Metrics are unambiguous:

- npm tarball and unpacked bytes for exactly the files npm would publish;
- production JavaScript raw/gzip-9/Brotli bytes;
- production optimized WASM raw/gzip-9/Brotli bytes;
- total TypeScript declaration bytes;
- minimal, typical, and full minified consumer JavaScript bundles built by the package's existing esbuild dependency.

Consumer JavaScript bundle metrics explicitly exclude the separately shipped WASM asset, whose sizes are reported independently.

## Results, baselines, budgets, and reports

The versioned JSON contract is `schema/result-v1.schema.json`. Runtime validation rejects missing metadata, failed scenarios disguised as success, non-finite samples, and impossible negative durations. Scenario failures carry `status: failed` and an error; public summaries exclude them.

Local raw results are immutable files under `benchmarks/results/v<version>/<environment-id>/` and are gitignored. CI uploads them as artifacts. Release results should be attached immutably to the matching release. `baseline` copies only a clean official release result into `benchmarks/baselines/v<version>/<environment-id>.json` and refuses overwrite. The selected policy is an explicit previous-release baseline; it never rolls silently.

Comparison requires the same scenario version, generator version, seed, dataset configuration, point/series/pane counts, stable environment ID, OS, architecture, CPU, runtime/browser version, GPU identity, viewport, DPR, and refresh-rate metadata. Output includes baseline, current, absolute difference, percentage difference, direction, scenario/environment compatibility, and status. Budgets live only in `budgets.json`. Thresholds are intentionally empty until controlled baseline evidence exists; adding one requires both warning and failure percentages keyed as `<scenario>.<metric>.p50`.

`report` generates a human-readable Markdown artifact beside the raw result. `public` generates a stable `benchmark-public.json` containing only measured `public_candidate` metrics from a clean official release. The trace is: website field → public summary → immutable raw result → raw samples → versioned scenario → deterministic dataset/environment → commit.

## CI and limitations

PR smoke runs harness tests, production artifact sizes, and a short browser workload on shared CI. Nightly runs broader non-authoritative scenarios. The release workflow targets a future self-hosted Windows runner labeled `benchmark` and produces raw JSON, a readable report, and the public summary. Credentials and runner provisioning are deliberately outside this repository.

The `native` command writes a separate result for headless Rust ingestion, retained frame construction, and current-candle replacement. Native diagnostics are never mixed with browser product results or public summaries.

Competitor comparisons are deliberately not implemented here. A future comparison must use a separate result namespace, pin every library and browser version, disclose backend and feature configuration, use equivalent visible chart features and datasets, apply the same warm-up/statistics/environment rules, and publish methodology beside the numbers. Existing demo dependencies are not treated as benchmark competitors.

Not currently measurable with trustworthy semantics: photon-visible presentation completion, a separate first-`queue.submit` timestamp, GPU upload-only duration, GPU resource byte attribution, browser GPU driver version, physical core count, and native process RSS. They remain unsupported or `null`; no substitute number is inferred. Existing interaction and GPUI scene-plan examples remain useful internal diagnostics.
