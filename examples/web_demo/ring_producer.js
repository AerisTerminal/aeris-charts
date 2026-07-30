// Ring-source producer worker for the `series_api.set_ring_source` specs.
//
// Stands in for the consumer's real market-data worker: it writes fixed-stride OHLC rows into a
// `SharedArrayBuffer` ring and publishes a monotonic row count through an `Int32` cursor. The
// ordering here is a per-slot seqlock: mark the row as being written, fill its channels, publish
// that slot's completed logical count, then advance the global cursor.

/** @type {{f64: Float64Array, i32: Int32Array, layout: object, count: number, time: number} | null} */
let state = null;
/** Handle of the running paced loop, or null. */
let timer = null;
let run_start_count = 0;
let run_started_ms = 0;
let run_rate = 0;

function write_row(price) {
  const { layout, f64, i32, count } = state;
  // The cursor counts rows ever written; the slot it lands in is that count modulo capacity.
  const slot = count % layout.capacity;
  const base = layout.data_offset + slot * layout.row_stride;
  const at = (byte_offset) => (base + byte_offset) / 8;
  const next_count = count + 1;
  // Mark this slot unstable before overwriting any channel. The completed value is the same i32
  // logical count later published through the global cursor; its complement can never be mistaken
  // for that generation, including across i32 overflow.
  if (layout.sequence_offset !== undefined) {
    Atomics.store(i32, (base + layout.sequence_offset) / 4, ~next_count);
  }
  const next = price + Math.sin(state.time * 1e-3) * 0.5;
  f64[at(layout.time_offset)] = state.time;
  f64[at(layout.open_offset)] = price;
  f64[at(layout.high_offset)] = Math.max(price, next) + 0.3;
  f64[at(layout.low_offset)] = Math.min(price, next) - 0.3;
  f64[at(layout.close_offset)] = next;
  state.count = next_count;
  state.time += 1;
  if (layout.sequence_offset !== undefined) {
    Atomics.store(i32, (base + layout.sequence_offset) / 4, next_count);
  }
  Atomics.store(i32, layout.write_cursor_offset / 4, next_count);
}

function write_batch(rows) {
  for (let i = 0; i < rows; i += 1) write_row(100 + (state.count % 20) * 0.1);
}

self.onmessage = (event) => {
  const message = event.data;
  switch (message.type) {
    case "init": {
      const { buffer, layout, start_time } = message;
      state = {
        f64: new Float64Array(buffer),
        i32: new Int32Array(buffer),
        layout,
        count: 0,
        time: start_time,
      };
      // Start the cursor at 0 so the engine's bind-time read and ours agree.
      Atomics.store(state.i32, layout.write_cursor_offset / 4, 0);
      self.postMessage({ type: "ready" });
      break;
    }
    case "burst": {
      // Write a fixed number of rows as fast as possible, with no yielding — used to force a
      // producer overrun deterministically.
      write_batch(message.rows);
      self.postMessage({ type: "burst_done", written: state.count });
      break;
    }
    case "start": {
      // Pace against elapsed wall time rather than assuming setTimeout(1) really fires every 1ms.
      // Worker timers are commonly clamped/coalesced; catching up to the elapsed-time target makes
      // the measured 50k/s arm actually produce 50k/s on those browsers.
      run_start_count = state.count;
      run_started_ms = performance.now();
      run_rate = message.rows_per_second;
      const tick = () => {
        const target = Math.floor((performance.now() - run_started_ms) * run_rate / 1000);
        const produced = state.count - run_start_count;
        if (target > produced) write_batch(target - produced);
        timer = setTimeout(tick, 1);
      };
      tick();
      self.postMessage({ type: "started", rows_per_second: run_rate });
      break;
    }
    case "stop": {
      if (timer !== null) clearTimeout(timer);
      timer = null;
      self.postMessage({
        type: "stopped",
        written: state === null ? 0 : state.count,
        session_written: state === null ? 0 : state.count - run_start_count,
        elapsed_ms: performance.now() - run_started_ms,
      });
      break;
    }
    default:
      break;
  }
};
