import { generate_ohlcv } from "/benchmarks/shared.mjs";

const root = document.querySelector("#bench-root");
let package_module = null;
const live = [];
let frame_recording = null;
let lifecycle_fixture = null;

const next_frame = () => new Promise((resolve) => requestAnimationFrame(resolve));
const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

async function load_package() {
  package_module ??= await import("/dist/nucleuscharts_financial.js");
  return package_module;
}

function host() {
  const element = document.createElement("div");
  element.className = "bench-host";
  root.append(element);
  root.style.setProperty("--columns", String(Math.ceil(Math.sqrt(root.childElementCount))));
  return element;
}

async function create(points = 0, seed = 0x02f6e2b1, options = {}, fixture = null) {
  const api = await load_package();
  const container = host();
  const chart = await api.create_chart(container, { autoSize: true, ...options });
  const series = chart.add_series("candlestick");
  const columns = fixture ?? generate_ohlcv(points, seed);
  if (points > 0) series.set_data_typed(columns);
  const entry = { chart, series, container, columns };
  live.push(entry);
  return entry;
}

function remove_entry(entry) {
  entry.chart.remove();
  entry.container.remove();
  const index = live.indexOf(entry);
  if (index >= 0) live.splice(index, 1);
}

function reset() {
  while (live.length > 0) remove_entry(live.at(-1));
  root.replaceChildren();
  root.style.setProperty("--columns", "1");
}

async function page_memory() {
  if (typeof performance.measureUserAgentSpecificMemory !== "function") return null;
  try {
    return (await performance.measureUserAgentSpecificMemory()).bytes;
  } catch {
    return null;
  }
}

async function memory_snapshot(chart = live[0]?.chart ?? null) {
  if (typeof globalThis.gc === "function") globalThis.gc();
  await next_frame();
  return {
    page_bytes: await page_memory(),
    wasm_linear_memory_bytes: chart?.frame_stats().memory_bytes ?? null,
    forced_gc: typeof globalThis.gc === "function",
  };
}

async function environment() {
  let adapter_info = null;
  try {
    const adapter = await navigator.gpu?.requestAdapter();
    adapter_info = adapter?.info ?? null;
  } catch {
    adapter_info = null;
  }
  const probe = live[0]?.chart ?? null;
  return {
    user_agent: navigator.userAgent,
    device_pixel_ratio: devicePixelRatio,
    viewport: { width: innerWidth, height: innerHeight },
    gpu: adapter_info?.device ?? adapter_info?.description ?? null,
    gpu_vendor: adapter_info?.vendor ?? null,
    gpu_architecture: adapter_info?.architecture ?? null,
    backend: probe?.backend() ?? null,
    gpu_timestamp_supported: navigator.gpu === undefined ? false : null,
    page_memory_supported: typeof performance.measureUserAgentSpecificMemory === "function",
  };
}

async function startup(points, seed) {
  reset();
  package_module = null;
  const columns = generate_ohlcv(points, seed);
  const started = performance.now();
  const import_started = performance.now();
  const api = await load_package();
  const module_import_ms = performance.now() - import_started;
  const wasm_started = performance.now();
  await api.init_wasm();
  const wasm_init_ms = performance.now() - wasm_started;
  const container = host();
  const create_started = performance.now();
  const chart = await api.create_chart(container, { autoSize: true });
  const chart_create_api_ms = performance.now() - create_started;
  const series_started = performance.now();
  const series = chart.add_series("candlestick");
  const series_create_api_ms = performance.now() - series_started;
  let set_data_api_ms = 0;
  if (points > 0) {
    const data_started = performance.now();
    series.set_data_typed(columns);
    set_data_api_ms = performance.now() - data_started;
  }
  const stats = chart.frame_stats();
  await next_frame();
  const first_raf_after_ready_ms = performance.now() - started;
  live.push({ chart, series, container, columns });
  return { module_import_ms, wasm_init_ms, chart_create_api_ms, series_create_api_ms, set_data_api_ms, first_raf_after_ready_ms, first_frame_cpu_ms: stats.cpu_ms, presented_frames_at_ready: stats.presented_frames, backend: chart.backend() };
}

async function historical(points, seed, warmup_runs, measured_runs) {
  reset();
  const entry = await create(0, seed);
  const columns = generate_ohlcv(points, seed);
  const api_samples = [];
  const ready_samples = [];
  const cpu_samples = [];
  for (let run = 0; run < warmup_runs + measured_runs; run += 1) {
    const started = performance.now();
    entry.series.set_data_typed(columns);
    const api_ms = performance.now() - started;
    const cpu_ms = entry.chart.frame_stats().cpu_ms;
    await next_frame();
    const ready_ms = performance.now() - started;
    if (run >= warmup_runs) {
      api_samples.push(api_ms);
      ready_samples.push(ready_ms);
      cpu_samples.push(cpu_ms);
    }
  }
  return { api_samples, ready_samples, cpu_samples, stats: entry.chart.frame_stats(), backend: entry.chart.backend() };
}

async function realtime(points, seed, mode, update_rate_hz, duration_ms) {
  reset();
  const entry = await create(points, seed);
  const interval = 1000 / update_rate_hz;
  const deadline = performance.now() + duration_ms;
  const api_samples = [];
  const frame_cpu_samples = [];
  let updates = 0;
  let last_time = entry.columns.times.at(-1);
  let value = entry.columns.close.at(-1);
  let update_latency_total = 0;
  let update_latency_count = 0;
  const before = entry.chart.frame_stats();
  const started = performance.now();
  while (performance.now() < deadline) {
    const target = started + updates * interval;
    const wait = target - performance.now();
    if (wait > 0) await delay(wait);
    value += Math.sin(updates * 0.17) * 0.02;
    if (mode === "append") last_time += 60;
    const call_started = performance.now();
    entry.series.update({ time: last_time, open: value, high: value + 0.2, low: value - 0.2, close: value });
    api_samples.push(performance.now() - call_started);
    await next_frame();
    frame_cpu_samples.push(entry.chart.frame_stats().cpu_ms);
    updates += 1;
  }
  const elapsed_ms = performance.now() - started;
  const after = entry.chart.frame_stats();
  return {
    api_samples,
    frame_cpu_samples,
    elapsed_ms,
    updates,
    presented_frames: after.presented_frames - before.presented_frames,
    dropped_frames: after.dropped_frames - before.dropped_frames,
    ring_overruns: after.ring_overruns - before.ring_overruns,
    backend: entry.chart.backend(),
  };
}

async function prepare_interaction(points, seed) {
  reset();
  const entry = await create(points, seed);
  entry.chart.time_scale().fit_content();
  entry.chart.render();
  await next_frame();
  return { backend: entry.chart.backend() };
}

function start_frame_recording() {
  if (frame_recording !== null) throw new Error("frame recording already active");
  const record = { active: true, last: performance.now(), last_presented: null, frame_ms: [], cpu_ms: [], gpu_ms: [], draw_calls: [], started: performance.now() };
  frame_recording = record;
  const tick = (time) => {
    if (!record.active) return;
    const stats = live[0]?.chart.frame_stats();
    record.frame_ms.push(time - record.last);
    record.last = time;
    if (stats && stats.presented_frames !== record.last_presented) {
      record.cpu_ms.push(stats.cpu_ms);
      if (stats.gpu_ms !== null) record.gpu_ms.push(stats.gpu_ms);
      record.draw_calls.push(stats.draw_calls);
      record.last_presented = stats.presented_frames;
    }
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
}

async function stop_frame_recording() {
  const record = frame_recording;
  if (record === null) throw new Error("frame recording is not active");
  record.active = false;
  frame_recording = null;
  await next_frame();
  return { frame_ms: record.frame_ms.slice(2), cpu_ms: record.cpu_ms.slice(2), gpu_ms: record.gpu_ms.slice(2), draw_calls: record.draw_calls.slice(2), duration_ms: performance.now() - record.started, stats: live[0].chart.frame_stats() };
}

async function prepare_lifecycle(points, seed) {
  reset();
  lifecycle_fixture = { points, seed, columns: generate_ohlcv(points, seed) };
  const warmup = await create(points, seed, {}, lifecycle_fixture.columns);
  warmup.chart.time_scale().fit_content();
  warmup.chart.render();
  await next_frame();
  remove_entry(warmup);
}

async function lifecycle(points, seed, cycles) {
  reset();
  if (lifecycle_fixture?.points !== points || lifecycle_fixture?.seed !== seed) throw new Error("lifecycle warm-up fixture is missing");
  const baseline = await memory_snapshot(null);
  const wasm_samples = [];
  let peak_page_bytes = baseline.page_bytes;
  let backend = null;
  const page_sample_target = cycles <= 10 ? 1 : Math.min(10, Math.ceil(cycles / 10));
  const page_sample_stride = Math.ceil(cycles / page_sample_target);
  for (let cycle = 0; cycle < cycles; cycle += 1) {
    const entry = await create(points, seed, {}, lifecycle_fixture.columns);
    backend ??= entry.chart.backend();
    entry.chart.time_scale().fit_content();
    entry.chart.render();
    await next_frame();
    wasm_samples.push(entry.chart.frame_stats().memory_bytes);
    if ((cycle + 1) % page_sample_stride === 0) {
      const loaded = await memory_snapshot(entry.chart);
      if (loaded.page_bytes !== null) peak_page_bytes = Math.max(peak_page_bytes ?? 0, loaded.page_bytes);
    }
    remove_entry(entry);
  }
  const final = await memory_snapshot(null);
  return { baseline, final, peak_page_bytes, wasm_linear_memory_samples: wasm_samples, cycles, loaded_page_memory_sample_count: page_sample_target, backend };
}

async function multi_chart(points, seed, chart_counts) {
  reset();
  const rows = [];
  for (const count of chart_counts) {
    reset();
    const fixtures = Array.from({ length: count }, (_, index) => generate_ohlcv(points, seed + index));
    const started = performance.now();
    for (let index = 0; index < count; index += 1) await create(points, seed + index, {}, fixtures[index]);
    await next_frame();
    const startup_ms = performance.now() - started;
    rows.push({ count, startup_ms, wasm_linear_memory_bytes: live[0]?.chart.frame_stats().memory_bytes ?? null, frame_cpu_ms: live.map((entry) => entry.chart.frame_stats().cpu_ms) });
  }
  return { rows, backend: live[0]?.chart.backend() ?? null };
}

async function multi_series(points, seed, series_counts, pane_counts) {
  reset();
  const rows = [];
  const columns = generate_ohlcv(points, seed);
  for (const pane_count of pane_counts) {
    for (const series_count of series_counts) {
      reset();
      const api = await load_package();
      const container = host();
      const started = performance.now();
      const chart = await api.create_chart(container, { autoSize: true });
      for (let pane = 1; pane < pane_count; pane += 1) chart.add_pane(true);
      for (let index = 0; index < series_count; index += 1) {
        const series = chart.add_series(index === 0 ? "candlestick" : "line", { pane: index % pane_count });
        series.set_data_typed(columns);
      }
      chart.time_scale().fit_content();
      chart.render();
      await next_frame();
      const startup_ms = performance.now() - started;
      live.push({ chart, series: null, container, columns });
      const stats = chart.frame_stats();
      rows.push({ pane_count, series_count, startup_ms, wasm_linear_memory_bytes: stats.memory_bytes, frame_cpu_ms: stats.cpu_ms });
    }
  }
  return { rows, backend: live[0]?.chart.backend() ?? null };
}

async function soak(points, seed, duration_ms, sample_interval_ms, memory_sample_interval_ms) {
  reset();
  const entry = await create(points, seed);
  entry.chart.time_scale().fit_content();
  const started = performance.now();
  const samples = [];
  let next_sample = started;
  let next_memory_sample = started + memory_sample_interval_ms;
  let updates = 0;
  let last_time = entry.columns.times.at(-1);
  let value = entry.columns.close.at(-1);
  let update_latency_total = 0;
  let update_latency_count = 0;
  while (performance.now() - started < duration_ms) {
    value += Math.sin(updates * 0.17) * 0.02;
    if (updates > 0 && updates % 60 === 0) last_time += 60;
    const update_started = performance.now();
    entry.series.update({ time: last_time, open: value, high: value + 0.2, low: value - 0.2, close: value });
    update_latency_total += performance.now() - update_started;
    update_latency_count += 1;
    if (updates % 120 === 0) entry.chart.time_scale().scroll_to_position((updates / 120) % 20, false);
    if (updates % 180 === 0) entry.chart.set_crosshair_position(value, last_time, entry.series);
    if (updates > 0 && updates % 240 === 0) entry.chart.resize(updates % 480 === 0 ? 1280 : 1200, 720, 1);
    await next_frame();
    updates += 1;
    if (performance.now() >= next_sample) {
      let page_bytes = null;
      if (performance.now() >= next_memory_sample) {
        page_bytes = (await memory_snapshot(entry.chart)).page_bytes;
        next_memory_sample += memory_sample_interval_ms;
      }
      const stats = entry.chart.frame_stats();
      samples.push({ elapsed_ms: performance.now() - started, page_bytes, wasm_linear_memory_bytes: stats.memory_bytes, update_api_ms: update_latency_total / update_latency_count, ...stats });
      update_latency_total = 0;
      update_latency_count = 0;
      next_sample = performance.now() + sample_interval_ms;
    }
  }
  return { duration_ms: performance.now() - started, updates, samples, backend: entry.chart.backend() };
}

globalThis.__nucleus_bench = { environment, historical, lifecycle, memory_snapshot, multi_chart, multi_series, prepare_interaction, prepare_lifecycle, realtime, reset, soak, start_frame_recording, startup, stop_frame_recording };
globalThis.__nucleus_bench_ready = true;
