import { test, expect } from "@playwright/test";

// `series_api.set_ring_source` (consumer Item 3). Even a batched append couples tick arrival to
// engine calls — the host has to decide when to flush. Binding a ring moves that decision into the
// engine's frame tick, which is what makes cost per frame independent of producer rate.
//
// The producer here is a real Web Worker (`ring_producer.js`) writing into a real
// `SharedArrayBuffer`, so the specs exercise the actual cross-thread handshake rather than a
// same-thread stand-in. The demo test server sends COOP/COEP for this reason.

/** Packed `f64[5]` + Int32 sequence rows after a 64-byte header; global cursor at byte 0. */
const LAYOUT = {
  data_offset: 64,
  row_stride: 48,
  capacity: 4096,
  time_offset: 0,
  open_offset: 8,
  high_offset: 16,
  low_offset: 24,
  close_offset: 32,
  sequence_offset: 40,
  write_cursor_offset: 0,
};
/** Past every series the demo installs, so ring rows append at the global tip. */
const START_TIME = 4_000_000_000;

async function wait_chart(page) {
  await page.waitForFunction(() => window.__chart?.backend?.() !== undefined);
  await page.evaluate(() => new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  }));
}

/** Install the shared harness: a hidden line series, a ring buffer, and the producer worker. */
async function setup_ring(page, layout = LAYOUT, start_time = START_TIME) {
  return page.evaluate(async ({ layout, start_time }) => {
    if (typeof SharedArrayBuffer === "undefined") throw new Error("no SharedArrayBuffer (not cross-origin isolated)");
    const bytes = layout.data_offset + layout.row_stride * layout.capacity;
    const buffer = new SharedArrayBuffer(bytes);
    const worker = new Worker("./ring_producer.js", { type: "module" });
    const replies = [];
    worker.onmessage = (event) => replies.push(event.data);
    const next = (type) => new Promise((resolve) => {
      const check = () => {
        const i = replies.findIndex((r) => r.type === type);
        if (i >= 0) resolve(replies.splice(i, 1)[0]);
        else setTimeout(check, 2);
      };
      check();
    });
    const series = window.__chart.add_series("line", { visible: false });
    window.__ring = { buffer, worker, series, layout, next };
    worker.postMessage({ type: "init", buffer, layout, start_time });
    await next("ready");
    return { bytes };
  }, { layout, start_time });
}

/** Let `frames` animation frames pass so the engine's drain loop runs that many times. */
async function pass_frames(page, frames) {
  await page.evaluate((n) => new Promise((resolve) => {
    let left = n;
    const step = () => (left-- <= 0 ? resolve() : requestAnimationFrame(step));
    step();
  }), frames);
}

test.beforeEach(async ({ page }) => {
  page.on("pageerror", (error) => console.log(`[browser:pageerror] ${error.message}`));
  await page.goto("/");
  await wait_chart(page);
});

test.afterEach(async ({ page }) => {
  await page.evaluate(() => window.__ring?.worker?.terminate()).catch(() => {});
});

test("the page is cross-origin isolated so SharedArrayBuffer exists", async ({ page }) => {
  expect(await page.evaluate(() => crossOriginIsolated)).toBe(true);
  expect(await page.evaluate(() => typeof SharedArrayBuffer)).toBe("function");
});

test("an in-progress sequence is rejected without consumption and succeeds on a stable retry", async ({ page }) => {
  const result = await page.evaluate(({ layout, start_time }) => {
    const buffer = new SharedArrayBuffer(layout.data_offset + layout.row_stride * layout.capacity);
    const bytes = new DataView(buffer);
    const atomics = new Int32Array(buffer);
    const series = window.__chart.add_series("line", { visible: false });
    series.set_ring_source(buffer, layout);

    const next = 1;
    const base = layout.data_offset;
    Atomics.store(atomics, (base + layout.sequence_offset) / 4, ~next);
    bytes.setFloat64(base + layout.time_offset, start_time, true);
    bytes.setFloat64(base + layout.open_offset, 100, true);
    bytes.setFloat64(base + layout.high_offset, 101, true);
    bytes.setFloat64(base + layout.low_offset, 99, true);
    bytes.setFloat64(base + layout.close_offset, 100.5, true);
    // Deliberately expose a cursor whose slot is still marked in-progress. Three retries must
    // consume nothing, leaving the same logical row available for the next stable drain.
    Atomics.store(atomics, layout.write_cursor_offset / 4, next);
    const report = new Float64Array(3);
    const rejected_rows = window.__chart.wasm.drain_ring_sources(report);
    const rows_after_reject = series.data().length;

    Atomics.store(atomics, (base + layout.sequence_offset) / 4, next);
    const accepted_rows = window.__chart.wasm.drain_ring_sources(report);
    const data = series.data();
    series.set_ring_source(null);
    return { rejected_rows, rows_after_reject, accepted_rows, data };
  }, { layout: LAYOUT, start_time: START_TIME });

  expect(result.rejected_rows).toBe(0);
  expect(result.rows_after_reject).toBe(0);
  expect(result.accepted_rows).toBe(1);
  expect(result.data).toHaveLength(1);
  expect(result.data[0].time).toBe(START_TIME);
});

test("rows a worker writes appear in the series with no per-tick engine call", async ({ page }) => {
  await setup_ring(page);
  const result = await page.evaluate(async () => {
    const { series, worker, next } = window.__ring;
    // Count every call the façade makes into the engine's streaming entry points, so "no JS calls
    // per tick" is asserted rather than asserted-by-comment.
    const wasm = window.__chart.wasm;
    let update_calls = 0;
    let drain_calls = 0;
    for (const name of ["update_series_bar_styled", "update_series_bars_typed"]) {
      const original = wasm[name].bind(wasm);
      wasm[name] = (...args) => { update_calls += 1; return original(...args); };
    }
    const drain = wasm.drain_ring_sources.bind(wasm);
    wasm.drain_ring_sources = (...args) => { drain_calls += 1; return drain(...args); };

    series.set_ring_source(window.__ring.buffer, window.__ring.layout);
    worker.postMessage({ type: "burst", rows: 500 });
    await next("burst_done");
    // A few frames for the engine's own tick to pick them up.
    await new Promise((resolve) => {
      let left = 5;
      const step = () => (left-- <= 0 ? resolve() : requestAnimationFrame(step));
      step();
    });
    const data = series.data();
    return {
      rows: data.length,
      first_time: data[0]?.time,
      last_time: data[data.length - 1]?.time,
      update_calls,
      drain_calls,
    };
  });

  expect(result.rows).toBe(500);
  expect(result.first_time).toBe(START_TIME);
  expect(result.last_time).toBe(START_TIME + 499);
  // The whole point: 500 ticks produced zero per-tick engine calls.
  expect(result.update_calls, "the façade called a per-point engine entry point").toBe(0);
  // And the drain ran per frame, not per tick.
  expect(result.drain_calls).toBeGreaterThan(0);
  expect(result.drain_calls).toBeLessThan(50);
});

test("a data_changed('update') fires per drained frame, not per row", async ({ page }) => {
  await setup_ring(page);
  const result = await page.evaluate(async () => {
    const { series, worker, next } = window.__ring;
    const scopes = [];
    series.subscribe_data_changed((scope) => scopes.push(scope));
    series.set_ring_source(window.__ring.buffer, window.__ring.layout);
    worker.postMessage({ type: "burst", rows: 800 });
    await next("burst_done");
    await new Promise((resolve) => {
      let left = 6;
      const step = () => (left-- <= 0 ? resolve() : requestAnimationFrame(step));
      step();
    });
    return { scopes, rows: series.data().length };
  });
  expect(result.rows).toBe(800);
  expect(result.scopes.length).toBeGreaterThan(0);
  // Far fewer notifications than rows, and every one is an "update".
  expect(result.scopes.length).toBeLessThan(10);
  expect(new Set(result.scopes)).toEqual(new Set(["update"]));
});

test("frame cost includes a sustained 50,000 rows/s drain and stays under budget", async ({ page }) => {
  test.setTimeout(180_000);
  await setup_ring(page);
  const result = await page.evaluate(async () => {
    const { series, worker, next } = window.__ring;
    series.set_ring_source(window.__ring.buffer, window.__ring.layout);

    const measure = async (rows_per_second) => {
      worker.postMessage({ type: "start", rows_per_second });
      await next("started");
      // Discard the first stretch (the rate ramps), then sample the engine's own reported frame
      // cost over a fixed number of frames.
      const samples = [];
      await new Promise((resolve) => {
        let frames = 0;
        const step = () => {
          frames += 1;
          if (frames > 20) samples.push(window.__chart.frame_stats().cpu_ms);
          if (frames >= 120) { resolve(); return; }
          requestAnimationFrame(step);
        };
        requestAnimationFrame(step);
      });
      worker.postMessage({ type: "stop" });
      const stopped = await next("stopped");
      samples.sort((a, b) => a - b);
      return {
        rate: rows_per_second,
        median_cpu_ms: samples[Math.floor(samples.length / 2)],
        rows_written: stopped.session_written,
        achieved_rate: stopped.session_written / (stopped.elapsed_ms / 1000),
      };
    };

    const low = await measure(100);
    const high = await measure(50_000);
    return {
      low,
      high,
      overruns: window.__chart.frame_stats().ring_overruns,
      rows: series.data().length,
    };
  });

  console.log(`ring frame cost: ${JSON.stringify(result)}`);
  // Both requested rates were actually exercised; a throttled worker must not turn this into a
  // comparison of two unrelated lower rates.
  expect(result.low.achieved_rate).toBeGreaterThan(80);
  expect(result.low.achieved_rate).toBeLessThan(140);
  expect(result.high.achieved_rate).toBeGreaterThan(45_000);

  // `cpu_ms` now includes the SAB copy, sequence validation, batch application, layout and command
  // encoding. Work grows with delivered rows, but batching keeps a 500x producer-rate increase well
  // below a per-tick-shaped 500x frame increase and inside the 8ms consumer budget.
  const rate_ratio = result.high.achieved_rate / result.low.achieved_rate;
  const cost_ratio = result.high.median_cpu_ms / result.low.median_cpu_ms;
  console.log(`ring: ${rate_ratio.toFixed(1)}x achieved rate -> ${cost_ratio.toFixed(2)}x frame cost`);
  expect(result.high.median_cpu_ms).toBeLessThan(8);
});

test("an induced overrun renders the newest window and reports the loss", async ({ page }) => {
  // A small ring so an overrun is easy to force deterministically.
  const layout = { ...LAYOUT, capacity: 64 };
  await setup_ring(page, layout);
  const result = await page.evaluate(async () => {
    const { series, worker, next, layout } = window.__ring;
    const wasm = window.__chart.wasm;
    series.set_ring_source(window.__ring.buffer, layout);
    const before = window.__chart.frame_stats().ring_overruns;

    // The producer runs on its own thread, so a frame can drain part-way through a burst — which is
    // real behaviour, but it makes the row count non-deterministic. Suppress the drain for the
    // duration of the burst so this measures exactly one overrunning drain: the engine sees 500 rows
    // published into a 64-row ring, 436 of them already overwritten.
    const drain = wasm.drain_ring_sources.bind(wasm);
    wasm.drain_ring_sources = () => 0;

    const WRITTEN = 500;
    worker.postMessage({ type: "burst", rows: WRITTEN });
    await next("burst_done");

    wasm.drain_ring_sources = drain;
    await new Promise((resolve) => {
      let left = 5;
      const step = () => (left-- <= 0 ? resolve() : requestAnimationFrame(step));
      step();
    });
    const data = series.data();
    const times = data.map((d) => d.time);
    return {
      written: WRITTEN,
      capacity: layout.capacity,
      rows: data.length,
      first_time: times[0],
      last_time: times[times.length - 1],
      // The window must be contiguous and ascending — the failure mode being ruled out is a torn
      // read that mixes fresh slots with stale ones.
      contiguous: times.every((t, i) => i === 0 || t === times[i - 1] + 1),
      overruns: window.__chart.frame_stats().ring_overruns - before,
    };
  });

  // Exactly the newest `capacity` rows rendered, contiguous and in order.
  expect(result.rows).toBe(result.capacity);
  expect(result.contiguous, "the rendered window was torn").toBe(true);
  expect(result.last_time).toBe(START_TIME + result.written - 1);
  expect(result.first_time).toBe(result.last_time - result.capacity + 1);
  // And the loss is reported rather than swallowed. Every written row is either in the series or
  // counted as lost — this is the invariant that holds regardless of drain timing.
  expect(result.overruns).toBe(result.written - result.rows);
});

test("a drain that races a burst still loses no row silently", async ({ page }) => {
  // The un-suppressed version of the test above: the drain loop is free to run mid-burst, so the
  // row count is not predictable. What must still hold is conservation — every row the producer
  // published is either in the series or counted in `ring_overruns`.
  await setup_ring(page, { ...LAYOUT, capacity: 64 });
  const result = await page.evaluate(async (start_time) => {
    const { series, worker, next, layout } = window.__ring;
    series.set_ring_source(window.__ring.buffer, layout);
    const before = window.__chart.frame_stats().ring_overruns;
    const WRITTEN = 5_000;
    worker.postMessage({ type: "burst", rows: WRITTEN });
    await next("burst_done");
    await new Promise((resolve) => {
      let left = 8;
      const step = () => (left-- <= 0 ? resolve() : requestAnimationFrame(step));
      step();
    });
    const data = series.data();
    return {
      written: WRITTEN,
      rows: data.length,
      last_time: data[data.length - 1]?.time,
      coherent: data.every(({ time, value }) => {
        const count = time - start_time;
        const price = 100 + (count % 20) * 0.1;
        const expected = price + Math.sin(time * 1e-3) * 0.5;
        return Math.abs(value - expected) < 1e-9;
      }),
      overruns: window.__chart.frame_stats().ring_overruns - before,
    };
  }, START_TIME);
  expect(result.rows + result.overruns, "rows were lost without being counted").toBe(result.written);
  expect(result.coherent, "a row mixed bytes from different ring generations").toBe(true);
  // The newest row always makes it through, whatever the timing.
  expect(result.last_time).toBe(START_TIME + result.written - 1);
});

test("set_ring_source(null) restores explicit updates and stops the drain loop", async ({ page }) => {
  await setup_ring(page);
  const result = await page.evaluate(async () => {
    const { series, worker, next } = window.__ring;
    series.set_ring_source(window.__ring.buffer, window.__ring.layout);
    worker.postMessage({ type: "burst", rows: 100 });
    await next("burst_done");
    await new Promise((resolve) => {
      let left = 5;
      const step = () => (left-- <= 0 ? resolve() : requestAnimationFrame(step));
      step();
    });
    const while_bound = series.data().length;

    // Unbind: the engine releases its views, and further producer writes are ignored.
    series.set_ring_source(null);
    const bound_after_unbind = window.__chart.wasm.ring_source_count();
    let drain_calls = 0;
    const drain = window.__chart.wasm.drain_ring_sources.bind(window.__chart.wasm);
    window.__chart.wasm.drain_ring_sources = (...args) => { drain_calls += 1; return drain(...args); };

    worker.postMessage({ type: "burst", rows: 100 });
    await next("burst_done");
    await new Promise((resolve) => {
      let left = 10;
      const step = () => (left-- <= 0 ? resolve() : requestAnimationFrame(step));
      step();
    });
    const after_unbind = series.data().length;

    // Explicit updates work again, on top of what the ring delivered.
    const last = series.data()[after_unbind - 1].time;
    const one = new Float64Array([last + 1]);
    const value = new Float64Array([123.5]);
    series.update_typed({ times: one, open: value, high: value, low: value, close: value });
    const explicit = series.data();

    return {
      while_bound,
      after_unbind,
      bound_after_unbind,
      drain_calls,
      explicit_rows: explicit.length,
      explicit_last: explicit[explicit.length - 1],
    };
  });

  expect(result.while_bound).toBe(100);
  // The unbound ring delivers nothing more, and the per-frame drain loop is gone entirely.
  expect(result.after_unbind).toBe(100);
  expect(result.bound_after_unbind).toBe(0);
  expect(result.drain_calls, "the drain loop kept running with no ring bound").toBe(0);
  // Explicit updates took over cleanly.
  expect(result.explicit_rows).toBe(101);
  expect(result.explicit_last.value).toBeCloseTo(123.5, 10);
});

test("a malformed layout is rejected at bind time with a specific reason", async ({ page }) => {
  await setup_ring(page);
  const errors = await page.evaluate(() => {
    const { series, buffer, layout } = window.__ring;
    const attempt = (patch) => {
      try {
        series.set_ring_source(buffer, { ...layout, ...patch });
        return "accepted";
      } catch (error) {
        return String(error.message);
      }
    };
    const results = {
      zero_capacity: attempt({ capacity: 0 }),
      channel_past_row: attempt({ close_offset: 44 }),
      misaligned_cursor: attempt({ write_cursor_offset: 2 }),
      misaligned_sequence: attempt({ sequence_offset: 41 }),
      sequence_past_row: attempt({ sequence_offset: 46 }),
      sequence_overlaps_close: attempt({ sequence_offset: 32 }),
      too_big_for_buffer: attempt({ capacity: 1_000_000 }),
      no_layout: (() => {
        try { series.set_ring_source(buffer); return "accepted"; } catch (e) { return String(e.message); }
      })(),
      plain_array_buffer: (() => {
        try { series.set_ring_source(new ArrayBuffer(1024), layout); return "accepted"; } catch (e) { return String(e.message); }
      })(),
    };
    // None of the rejected attempts may leave a ring bound.
    results.bound = window.__chart.wasm.ring_source_count();
    return results;
  });

  expect(errors.zero_capacity).toContain("capacity must be at least 1 row");
  expect(errors.channel_past_row).toContain("close_offset 44 + 8 bytes exceeds row_stride 48");
  expect(errors.misaligned_cursor).toContain("not 4-byte aligned");
  expect(errors.misaligned_sequence).toContain("4-byte-aligned Int32");
  expect(errors.sequence_past_row).toContain("sequence_offset 46 + 4 bytes exceeds row_stride 48");
  expect(errors.sequence_overlaps_close).toContain("overlaps close channel bytes 32..40");
  expect(errors.too_big_for_buffer).toContain("buffer is");
  expect(errors.no_layout).toContain("requires a layout");
  expect(errors.plain_array_buffer).toContain("SharedArrayBuffer");
  expect(errors.bound).toBe(0);
});
