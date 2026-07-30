// Ring-source producer worker for the `series_api.set_ring_source` specs.
//
// Stands in for the consumer's real market-data worker: it writes fixed-stride OHLC rows into a
// `SharedArrayBuffer` ring and publishes a monotonic row count through an `Int32` cursor. The
// ordering here is the contract the engine relies on — **row bytes first, cursor second** — so that
// every row below the published count is complete.

/** @type {{f64: Float64Array, i32: Int32Array, layout: object, count: number, time: number} | null} */
let state = null;
/** Handle of the running paced loop, or null. */
let timer = null;

function write_row(price) {
  const { layout, f64, i32, count } = state;
  // The cursor counts rows ever written; the slot it lands in is that count modulo capacity.
  const slot = count % layout.capacity;
  const base = layout.data_offset + slot * layout.row_stride;
  const at = (byte_offset) => (base + byte_offset) / 8;
  const next = price + Math.sin(state.time * 1e-3) * 0.5;
  f64[at(layout.time_offset)] = state.time;
  f64[at(layout.open_offset)] = price;
  f64[at(layout.high_offset)] = Math.max(price, next) + 0.3;
  f64[at(layout.low_offset)] = Math.min(price, next) - 0.3;
  f64[at(layout.close_offset)] = next;
  state.count += 1;
  state.time += 1;
  // Publish only after the row's bytes are in place. The engine's `Atomics.load` of this value is
  // the other half of the handshake.
  Atomics.store(i32, layout.write_cursor_offset / 4, state.count);
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
      // Paced production: `rows_per_second` spread over ~1 ms ticks. The fractional remainder is
      // carried in `debt` rather than rounded per tick — rounding up to one row per tick would
      // floor the achievable rate at ~1000 rows/s, which silently turns a "100 rows/s" arm into a
      // 1000 rows/s one and destroys the low end of a rate sweep.
      const per_tick = message.rows_per_second / 1000;
      let debt = 0;
      const tick = () => {
        debt += per_tick;
        const rows = Math.floor(debt);
        if (rows > 0) {
          debt -= rows;
          write_batch(rows);
        }
        timer = setTimeout(tick, 1);
      };
      tick();
      self.postMessage({ type: "started", per_tick });
      break;
    }
    case "stop": {
      if (timer !== null) clearTimeout(timer);
      timer = null;
      self.postMessage({ type: "stopped", written: state === null ? 0 : state.count });
      break;
    }
    default:
      break;
  }
};
