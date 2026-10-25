// Live mode for the demo's root chart. A worker simulates the exchange (live_feed_worker.js); this
// module installs its history and then applies the batches it posts once per animation frame,
// through the same public calls a production market-data host would use.

export const LIVE_TICK_SIZE = 0.05;
export const LIVE_INTERVAL_SECONDS = 60;
export const LIVE_SPEEDS = [1, 10, 60];

const HISTORY_BARS = 720;
const TAPE_BARS = 180;
const DEPTH_HISTORY_BARS = 180;
const DEPTH_BUCKET_SECONDS = 15;
// Series retention for an open-ended session; at 60x this is about five and a half hours of play.
const MAX_BARS = 20_000;
// Hidden tabs get no animation frames; past this many queued prints the batches apply at once so
// the queue stays bounded.
const MAX_PENDING_TRADES = 50_000;
const TRADE_STREAM_KEY = "DEMO:LIVE_TAPE";
const DEPTH_STREAM_KEY = "DEMO:LIVE_BOOK";
const UP_VOLUME = "#08998180";
const DOWN_VOLUME = "#f7525f80";
const UINT32 = 2 ** 32;

function concat(parts, Type) {
  let length = 0;
  for (const part of parts) length += part.length;
  if (parts.length === 1) return parts[0];
  const out = new Type(length);
  let offset = 0;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.length;
  }
  return out;
}

function concat_columns(groups) {
  const out = {};
  for (const key of Object.keys(groups[0])) {
    out[key] = concat(groups.map((group) => group[key]), groups[0][key].constructor);
  }
  return out;
}

function volume_point(bar) {
  return { time: bar.time, value: bar.volume, color: bar.close >= bar.open ? UP_VOLUME : DOWN_VOLUME };
}

function trim(rows) {
  // The same amortized eviction the engine applies under max_points.
  if (rows.length > MAX_BARS) rows.splice(0, rows.length - MAX_BARS + Math.floor(MAX_BARS / 32));
}

/**
 * `volume_series()` returns every histogram that mirrors the main series' volume; `catalogs`
 * rebuilds order-flow layers around a history install. `seed` pins the simulated market.
 */
export function create_live_market({ chart, main, catalogs, volume_series, seed, on_state, on_frame }) {
  let worker = null;
  let speed = LIVE_SPEEDS[0];
  let raf = null;
  let pending = [];
  let pending_trades = 0;
  let trade_stream = null;
  let depth_stream = null;
  let depth_sequence = 0;
  let installed = false;
  let bars = [];
  let volumes = [];
  let quote = null;
  let sim_time = null;
  const stats = { histories: 0, batches: 0, frames: 0, trades: 0, depth_rows: 0, bar_updates: 0 };

  const emit_state = () => on_state?.({ running: worker !== null, installed, speed });

  function apply_depth(rows) {
    const count = rows.timestamps_micros.length;
    if (count === 0) return;
    const sequences = { high: new Uint32Array(count), low: new Uint32Array(count) };
    const previous_sequences = { high: new Uint32Array(count), low: new Uint32Array(count) };
    for (let index = 0; index < count; index += 1) {
      const previous = depth_sequence + index;
      previous_sequences.high[index] = Math.floor(previous / UINT32);
      previous_sequences.low[index] = previous % UINT32;
      sequences.high[index] = Math.floor((previous + 1) / UINT32);
      sequences.low[index] = (previous + 1) % UINT32;
    }
    chart.update_depth_typed(depth_stream, {
      timestamps_micros: rows.timestamps_micros,
      sequences,
      previous_sequences,
      sides: rows.sides,
      prices: rows.prices,
      sizes: rows.sizes,
    });
    depth_sequence += count;
    stats.depth_rows += count;
  }

  function apply_bar(bar) {
    const last = bars.at(-1);
    if (last !== undefined && last.time === bar.time) bars[bars.length - 1] = bar;
    else bars.push(bar);
    const point = volume_point(bar);
    const last_volume = volumes.at(-1);
    if (last_volume !== undefined && last_volume.time === point.time) volumes[volumes.length - 1] = point;
    else volumes.push(point);
    trim(bars);
    trim(volumes);
    main.update({ time: bar.time, open: bar.open, high: bar.high, low: bar.low, close: bar.close });
    for (const series of volume_series()) series.update(point);
    stats.bar_updates += 1;
  }

  function install_history(message) {
    // Layers bound to the previous live streams are detached before those streams are replaced
    // and rebuilt on the new ones afterwards.
    const resume = catalogs.suspend_market_layers();
    try {
      trade_stream ??= chart.add_trade_stream(TRADE_STREAM_KEY, {
        tick_size: LIVE_TICK_SIZE,
        bar_type: "time",
        interval_seconds: LIVE_INTERVAL_SECONDS,
      });
      chart.set_trade_stream_trades_typed(trade_stream, message.trades);
      if (depth_stream !== null) chart.remove_depth_stream(depth_stream);
      depth_stream = chart.add_depth_stream(DEPTH_STREAM_KEY, {
        tick_size: LIVE_TICK_SIZE,
        max_levels_per_side: 64,
        history_bucket_micros: DEPTH_BUCKET_SECONDS * 1_000_000,
        max_history_buckets: 4096,
        max_history_cells: 400_000,
      });
      depth_sequence = 1;
      chart.set_depth_snapshot_typed(depth_stream, {
        ...message.depth_snapshot,
        sequence_high: 0,
        sequence_low: depth_sequence,
      });
      apply_depth(message.depth);

      const columns = message.bars;
      bars = Array.from(columns.times, (time, index) => ({
        time,
        open: columns.open[index],
        high: columns.high[index],
        low: columns.low[index],
        close: columns.close[index],
        volume: columns.volume[index],
      }));
      volumes = bars.map(volume_point);
      main.apply_options({ max_points: MAX_BARS });
      main.set_data_typed(columns);
      for (const series of volume_series()) {
        series.apply_options({ max_points: MAX_BARS });
        series.set_data(volumes);
      }
      installed = true;
      quote = message.quote;
      sim_time = message.sim_time;
      stats.trades += message.trades.timestamps_micros.length;
      stats.histories += 1;
    } finally {
      resume();
    }
    chart.time_scale().scroll_to_real_time();
    emit_state();
    on_frame?.({ bar: bars.at(-1), quote });
  }

  function apply_pending() {
    raf = null;
    if (pending.length === 0) return;
    const batches = pending;
    pending = [];
    pending_trades = 0;
    const trades = concat_columns(batches.map((batch) => batch.trades));
    if (trades.timestamps_micros.length > 0) {
      chart.update_trade_stream_trades_typed(trade_stream, trades);
      stats.trades += trades.timestamps_micros.length;
    }
    const changed = new Map();
    for (const batch of batches) for (const bar of batch.bars) changed.set(bar.time, bar);
    for (const bar of [...changed.values()].sort((a, b) => a.time - b.time)) apply_bar(bar);
    const last = batches.at(-1);
    quote = last.quote;
    sim_time = last.sim_time;
    on_frame?.({ bar: bars.at(-1), quote });
    // Depth last: the depth call repaints synchronously, which also paints this frame's bars.
    apply_depth(concat_columns(batches.map((batch) => batch.depth)));
    stats.batches += batches.length;
    stats.frames += 1;
  }

  function enqueue(batch) {
    pending.push(batch);
    pending_trades += batch.trades.timestamps_micros.length;
    if (pending_trades > MAX_PENDING_TRADES) {
      if (raf !== null) cancelAnimationFrame(raf);
      apply_pending();
      return;
    }
    raf ??= requestAnimationFrame(apply_pending);
  }

  function start() {
    if (worker !== null) return;
    const feed = new Worker(new URL("./live_feed_worker.js", import.meta.url), { type: "module" });
    worker = feed;
    feed.onmessage = (event) => {
      if (worker !== feed) return;
      if (event.data.type === "history") install_history(event.data);
      else enqueue(event.data);
    };
    feed.onerror = (event) => {
      console.error("Live market feed failed", event.message ?? event);
      stop();
    };
    feed.postMessage({
      type: "start",
      seed: seed ?? (crypto.getRandomValues(new Uint32Array(1))[0]),
      speed,
      start_price: 100,
      tick_size: LIVE_TICK_SIZE,
      interval_seconds: LIVE_INTERVAL_SECONDS,
      history_bars: HISTORY_BARS,
      tape_bars: TAPE_BARS,
      depth_history_bars: DEPTH_HISTORY_BARS,
      depth_bucket_seconds: DEPTH_BUCKET_SECONDS,
      now_seconds: Date.now() / 1000,
    });
    emit_state();
  }

  function stop() {
    if (worker === null) return;
    worker.terminate();
    worker = null;
    if (raf !== null) cancelAnimationFrame(raf);
    raf = null;
    pending = [];
    pending_trades = 0;
    emit_state();
  }

  return {
    start,
    stop,
    running: () => worker !== null,
    speed: () => speed,
    set_speed(value) {
      if (!LIVE_SPEEDS.includes(value)) return;
      speed = value;
      worker?.postMessage({ type: "speed", speed });
      emit_state();
    },
    quote: () => (worker !== null && installed ? quote : null),
    volume_data: () => (installed ? volumes : null),
    bars: () => bars,
    sim_time: () => sim_time,
    stats: () => ({ ...stats, installed, running: worker !== null, trade_stream, depth_stream }),
    // What the order-flow catalog binds to once live data has replaced the deterministic set.
    source() {
      if (!installed || trade_stream === null || depth_stream === null) return null;
      return {
        tick_size: LIVE_TICK_SIZE,
        interval_seconds: LIVE_INTERVAL_SECONDS,
        trade_stream,
        depth_stream,
        price_range() {
          let low = Infinity;
          let high = -Infinity;
          for (const bar of bars) {
            low = Math.min(low, bar.low);
            high = Math.max(high, bar.high);
          }
          const margin = Math.max((high - low) * 0.5, high * 0.04);
          const snap = (price) => Math.round(price / LIVE_TICK_SIZE) * LIVE_TICK_SIZE;
          return [snap(Math.max(LIVE_TICK_SIZE, low - margin)), snap(high + margin)];
        },
      };
    },
  };
}
