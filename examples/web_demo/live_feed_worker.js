// Simulated exchange feed for the demo's Live mode. The worker owns the market model and posts
// batched trade prints, incremental level-two changes, and bar updates the way a market-data
// gateway would; the page applies whatever has arrived once per animation frame.
//
// Model, from the slowest moving part to the fastest:
// - Volatility is the product of a slow log-OU regime factor (Heston-like, ~40 min) and a fast
//   GARCH(1,1) factor fed by one-second innovations, so calm and violent stretches cluster. News
//   shocks lift both factors, so their after-shocks decay over many minutes.
// - The efficient price moves with Student-t(4) innovations, an OU drift that lets trends emerge,
//   rare news jumps, frequent small liquidity gaps, and the permanent impact of informed flow.
//   Noise flow has no permanent impact; with it, gap-following aggressors would feed their own
//   impact back into the efficient price and turn every busy stretch into a runaway trend.
// - Market orders arrive as a Hawkes process whose base rate scales with volatility; meta-orders
//   add one-sided informed flow with larger impact. Aggressor side follows the gap between the
//   efficient price and the mid, so the book is dragged toward value by the tape and delta leads.
//   Sizes are lognormal with occasional block trades.
// - The book is a tick ladder. Orders sweep resting queues at the touch; quotes left behind by
//   the efficient price are picked off (partly printed, the rest cancelled); makers re-quote so
//   the spread is always one or two ticks; queues relax and churn; and round-number walls absorb
//   flow, reload like icebergs, hold the price, or are pulled.

const STEP_SECONDS = 0.1;
const POST_INTERVAL_MS = 50;
const MAX_ADVANCE_SECONDS = 120;
const DEPTH_LEVELS = 40;
const BOOK_TICKS = 1 << 16;
const BOOK_MARGIN = 512;
const MAX_BUFFERED_TRADES = 1 << 18;

const SIGMA = 0.00021;
const SLOW_TAU = 2400;
const SLOW_SD = 0.4;
const GARCH_ALPHA = 0.035;
const GARCH_BETA = 0.95;
const DRIFT_TAU = 900;
const DRIFT_SD = 0.035 * SIGMA;
const META_IMPACT_TICKS = 0.008;
const STALE_TICKS = 1;
const BASE_RATE = 2.2;
const HAWKES_DECAY = 1.2;
const HAWKES_JUMP = 0.7;
const MAX_RATE = 250;
const BLOCK_PROBABILITY = 0.004;
const NEWS_PER_SECOND = 1 / 5400;
const GAP_PER_SECOND = 1 / 360;
const META_PER_SECOND = 1 / 75;
const WALL_PER_SECOND = 1 / 70;
const MAX_WALLS = 8;

function create_random(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = Math.imul(state ^ (state >>> 15), 1 | state);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function column(Type) {
  return { data: new Type(4096), length: 0 };
}

function push(target, value) {
  if (target.length === target.data.length) {
    const next = new target.data.constructor(target.data.length * 2);
    next.set(target.data);
    target.data = next;
  }
  target.data[target.length++] = value;
}

function take(target) {
  const out = target.data.slice(0, target.length);
  target.length = 0;
  return out;
}

function create_market(config) {
  const tick_size = config.tick_size;
  const interval = config.interval_seconds;
  const random = create_random(config.seed);
  let spare = Number.NaN;
  const normal = () => {
    if (!Number.isNaN(spare)) {
      const value = spare;
      spare = Number.NaN;
      return value;
    }
    const radius = Math.sqrt(-2 * Math.log(1 - random()));
    const angle = 2 * Math.PI * random();
    spare = radius * Math.sin(angle);
    return radius * Math.cos(angle);
  };
  // Unit-variance Student-t with four degrees of freedom: z / sqrt(chi2(4) / 2).
  const student = () => normal() / Math.sqrt(-Math.log((1 - random()) * (1 - random())));
  const exponential = () => -Math.log(1 - random());
  const lognormal = (median, sigma) => median * Math.exp(sigma * normal());
  const poisson = (mean) => {
    if (mean <= 0) return 0;
    if (mean > 30) return Math.max(0, Math.round(mean + Math.sqrt(mean) * normal()));
    const limit = Math.exp(-mean);
    let count = -1;
    let product = 1;
    do {
      count += 1;
      product *= random();
    } while (product > limit);
    return count;
  };

  // Ladder indices are relative to `base` so the book array stays small and every index in use
  // stays inside it; the efficient price is clamped to keep a margin at both ends.
  const start_tick = Math.round(config.start_price / tick_size);
  const base = start_tick - BOOK_TICKS / 2;
  const min_log = Math.log(Math.max(20, base + BOOK_MARGIN) * tick_size);
  const max_log = Math.log((base + BOOK_TICKS - BOOK_MARGIN) * tick_size);

  let time = config.start_time;
  let log_price = Math.log(start_tick * tick_size);
  let slow = normal() * SLOW_SD * 0.5;
  let fast = 1;
  let drift = 0;
  let second_return = 0;
  let next_second = time + 1;
  let excitation = 0;
  let last_side = 0;
  let jump_left = 0;
  let jump_step = 0;
  const metas = [];

  const sizes = new Float64Array(BOOK_TICKS).fill(Number.NaN);
  const wall_at = new Uint8Array(BOOK_TICKS);
  const walls = [];
  const emitted_side = new Uint8Array(BOOK_TICKS);
  const emitted_size = new Float64Array(BOOK_TICKS);
  let emitted_low = 0;
  let emitted_high = -1;
  let last_depth_micros = 0;

  let best_bid = BOOK_TICKS / 2;
  let best_ask = best_bid + 1;

  let trade_id = 0;
  let last_trade_micros = 0;
  let record_trades = false;
  const trades = {
    times: column(Float64Array),
    prices: column(Float64Array),
    volumes: column(Float64Array),
    sides: column(Uint8Array),
    ids: column(Float64Array),
    sessions: column(Float64Array),
  };
  const depth_rows = {
    times: column(Float64Array),
    sides: column(Uint8Array),
    prices: column(Float64Array),
    sizes: column(Float64Array),
  };
  let bar = null;
  const finished_bars = [];

  const price_of = (tick) => Math.round((tick + base) * tick_size * 1e6) / 1e6;
  const queue_target = (distance) => 9 + 4 * Math.pow(distance, 0.65);
  const profile = (distance) => Math.max(1, Math.round(queue_target(distance) * Math.exp(0.45 * normal())));
  const touch_distance = (tick) => (tick <= best_bid ? best_bid - tick : tick - best_ask);
  const level = (tick) => {
    if (Number.isNaN(sizes[tick])) sizes[tick] = profile(Math.max(0, touch_distance(tick)));
    return sizes[tick];
  };
  const fair_ticks = () => Math.exp(log_price) / tick_size - base;
  const set_log_price = (value) => {
    log_price = Math.min(max_log, Math.max(min_log, value));
  };
  const shift_log_price = (ticks) => set_log_price(log_price + ticks * tick_size / Math.exp(log_price));
  const stamp = (seconds) => {
    last_trade_micros = Math.max(last_trade_micros, Math.floor(seconds * 1e6));
    return last_trade_micros;
  };

  for (let distance = 0; distance < DEPTH_LEVELS + 8; distance += 1) {
    sizes[best_bid - distance] = profile(distance);
    sizes[best_ask + distance] = profile(distance);
  }

  function roll_bar(bar_time, price) {
    if (bar !== null && bar.time === bar_time) return;
    if (bar !== null) finished_bars.push(bar);
    bar = { time: bar_time, open: price, high: price, low: price, close: price, volume: 0 };
  }

  // A tape longer than the engine retains is never useful, so the oldest half is dropped
  // instead of growing without bound.
  function drop_oldest_trades() {
    for (const target of Object.values(trades)) {
      const keep = target.length >> 1;
      target.data.copyWithin(0, target.length - keep, target.length);
      target.length = keep;
    }
  }

  function print(micros, tick, volume, side) {
    const price = price_of(tick);
    roll_bar(Math.floor(micros / 1e6 / interval) * interval, price);
    if (bar.volume === 0) {
      bar.open = price;
      bar.high = price;
      bar.low = price;
    }
    bar.high = Math.max(bar.high, price);
    bar.low = Math.min(bar.low, price);
    bar.close = price;
    bar.volume += volume;
    trade_id += 1;
    if (!record_trades) return;
    if (trades.times.length >= MAX_BUFFERED_TRADES) drop_oldest_trades();
    push(trades.times, micros);
    push(trades.prices, price);
    push(trades.volumes, volume);
    push(trades.sides, side > 0 ? 1 : 2);
    push(trades.ids, trade_id);
    push(trades.sessions, Math.floor(micros / 86_400_000_000));
  }

  function wall_at_tick(tick) {
    return wall_at[tick] === 1 ? walls.find((wall) => wall.tick === tick) : undefined;
  }

  function release_wall(tick) {
    if (wall_at[tick] === 0) return;
    wall_at[tick] = 0;
    const index = walls.findIndex((wall) => wall.tick === tick);
    if (index >= 0) walls.splice(index, 1);
  }

  function clear_touch(side) {
    const tick = side > 0 ? best_ask : best_bid;
    release_wall(tick);
    sizes[tick] = 0;
    if (side > 0) best_ask += 1;
    else best_bid -= 1;
  }

  function execute(side, quantity, at, impact) {
    const micros = stamp(at);
    let remaining = quantity;
    let swept = 0;
    while (remaining > 0 && swept < 12) {
      const tick = side > 0 ? best_ask : best_bid;
      let resting = level(tick);
      const fill = Math.min(remaining, resting);
      if (fill > 0) {
        print(micros, tick, fill, side);
        remaining -= fill;
        resting -= fill;
        sizes[tick] = resting;
      }
      if (resting > 0) break;
      const wall = wall_at_tick(tick);
      if (wall !== undefined && wall.refills > 0 && random() < 0.6) {
        wall.refills -= 1;
        sizes[tick] = Math.round(wall.size * (0.3 + 0.3 * random()));
        continue;
      }
      clear_touch(side);
      swept += 1;
    }
    last_side = side;
    shift_log_price(side * impact * Math.sqrt(quantity));
  }

  // A quote the efficient price has run through is hit by fast traders or cancelled by its owner.
  // A reloading wall instead holds and pushes the efficient price back to its own side.
  function pick_off(side) {
    const tick = side > 0 ? best_ask : best_bid;
    const resting = level(tick);
    const wall = wall_at_tick(tick);
    const micros = stamp(time);
    if (wall !== undefined && wall.refills > 0 && random() < 0.5) {
      if (resting > 0) print(micros, tick, resting, side);
      wall.refills -= 1;
      sizes[tick] = Math.round(wall.size * (0.3 + 0.3 * random()));
      set_log_price(Math.log((tick - side * 0.6 + base) * tick_size));
      jump_left = 0;
      return false;
    }
    const taken = wall !== undefined ? resting : Math.round(resting * (0.2 + 0.5 * random()));
    if (taken > 0) print(micros, tick, taken, side);
    clear_touch(side);
    return true;
  }

  function quote_inside(side) {
    const size = Math.max(1, Math.round(queue_target(0) * (0.3 + 0.5 * random())));
    if (side > 0) {
      best_bid += 1;
      release_wall(best_bid);
      sizes[best_bid] = size;
    } else {
      best_ask -= 1;
      release_wall(best_ask);
      sizes[best_ask] = size;
    }
  }

  function relax(tick, distance, dt) {
    if (wall_at[tick] === 1) return;
    let size = level(tick);
    size += (queue_target(distance) - size) * Math.min(1, dt * 0.25);
    if (random() < 0.3) size += Math.round(lognormal(2, 0.8)) * (random() < 0.55 ? 1 : -1);
    sizes[tick] = Math.max(1, Math.round(size));
  }

  function churn(tick) {
    if (wall_at[tick] === 1) return;
    const distance = Math.max(0, touch_distance(tick));
    sizes[tick] = Math.max(1, Math.round(level(tick) * (0.75 + 0.35 * random()) * 0.9 + profile(distance) * 0.1));
  }

  function maintain_book(dt) {
    for (let guard = 0; guard < 256 && best_ask < fair_ticks() - STALE_TICKS; guard += 1) {
      if (!pick_off(1)) break;
    }
    for (let guard = 0; guard < 256 && best_bid > fair_ticks() + STALE_TICKS; guard += 1) {
      if (!pick_off(-1)) break;
    }
    const fair = fair_ticks();
    while (best_ask - best_bid > 1) {
      if (best_bid + 1 < fair - 0.5) quote_inside(1);
      else if (best_ask - 1 > fair + 0.5) quote_inside(-1);
      else if (best_ask - best_bid > 2 || random() < 0.4) quote_inside(random() < 0.5 ? 1 : -1);
      else break;
    }
    for (let distance = 0; distance < 4; distance += 1) {
      relax(best_bid - distance, distance, dt);
      relax(best_ask + distance, distance, dt);
    }
    for (let count = 0; count < 2; count += 1) {
      churn(best_bid - 4 - Math.floor(random() * (DEPTH_LEVELS - 4)));
      churn(best_ask + 4 + Math.floor(random() * (DEPTH_LEVELS - 4)));
    }
  }

  function place_wall() {
    const ask = random() < 0.5;
    const distance = 6 + Math.floor(random() * 30);
    const tick = ask
      ? Math.ceil((best_ask + distance + base) / 10) * 10 - base
      : Math.floor((best_bid - distance + base) / 10) * 10 - base;
    if (wall_at[tick] === 1) return;
    const round_dollar = (tick + base) % 20 === 0 ? 1.6 : 1;
    const size = Math.round(lognormal(300, 0.45) * round_dollar);
    sizes[tick] = level(tick) + size;
    wall_at[tick] = 1;
    walls.push({
      tick,
      size,
      refills: random() < 0.35 ? 1 + Math.floor(random() * 2) : 0,
      expires: time + 120 + exponential() * 420,
    });
  }

  function update_second() {
    const unit = SIGMA * Math.exp(slow);
    const standardized = Math.min(100, (second_return / unit) ** 2);
    fast = Math.min(20, Math.max(0.15, (1 - GARCH_ALPHA - GARCH_BETA) + GARCH_ALPHA * standardized + GARCH_BETA * fast));
    slow += -slow / SLOW_TAU + SLOW_SD * Math.sqrt(2 / SLOW_TAU) * normal();
    slow = Math.min(1.4, Math.max(-1.2, slow));
    drift += -drift / DRIFT_TAU + DRIFT_SD * Math.sqrt(2 / DRIFT_TAU) * normal();
    second_return = 0;
    next_second += 1;

    for (let index = metas.length - 1; index >= 0; index -= 1) {
      if (time >= metas[index].ends) metas.splice(index, 1);
    }
    if (metas.length < 3 && random() < META_PER_SECOND) {
      const lean = drift / DRIFT_SD;
      metas.push({
        side: random() < 1 / (1 + Math.exp(-0.8 * lean)) ? 1 : -1,
        rate: lognormal(1.4, 0.5),
        child: lognormal(3, 0.4),
        ends: time + 15 + exponential() * 60,
      });
    }
    const price = Math.exp(log_price);
    if (jump_left === 0 && random() < NEWS_PER_SECOND) {
      const side = random() < 0.5 ? 1 : -1;
      const ticks = Math.min(80, 8 + exponential() * 20);
      jump_left = 6 + Math.floor(random() * 20);
      jump_step = side * ticks * tick_size / price / jump_left;
      slow = Math.min(1.4, slow + 0.2 + 0.2 * random());
      fast = Math.min(20, fast + 4);
      excitation += 25;
      metas.push({ side, rate: lognormal(5, 0.4), child: lognormal(5, 0.4), ends: time + 30 + exponential() * 90 });
    } else if (jump_left === 0 && random() < GAP_PER_SECOND) {
      const side = random() < 0.5 ? 1 : -1;
      jump_left = 2 + Math.floor(random() * 4);
      jump_step = side * (3 + random() * 5) * tick_size / price / jump_left;
    }

    if (walls.length < MAX_WALLS && random() < WALL_PER_SECOND) place_wall();
    for (let index = walls.length - 1; index >= 0; index -= 1) {
      const wall = walls[index];
      const distance = touch_distance(wall.tick);
      if (time > wall.expires || (distance <= 3 && random() < 0.05)) {
        sizes[wall.tick] = profile(Math.max(0, distance));
        release_wall(wall.tick);
      }
    }
  }

  function step(dt) {
    while (time >= next_second) update_second();
    const vol_ratio = Math.exp(slow) * Math.sqrt(fast);
    const innovation = SIGMA * vol_ratio * Math.sqrt(dt) * student();
    second_return += innovation;
    let move = innovation + drift * dt;
    if (jump_left > 0) {
      move += jump_step;
      jump_left -= 1;
    }
    set_log_price(log_price + move);

    excitation *= Math.exp(-HAWKES_DECAY * dt);
    let meta_rate = 0;
    let meta_bias = 0;
    for (const meta of metas) {
      meta_rate += meta.rate;
      meta_bias += meta.side * 0.3;
    }
    const rate = Math.min(MAX_RATE, BASE_RATE * Math.pow(Math.min(vol_ratio, 8), 1.1) + excitation + meta_rate);
    const count = poisson(rate * dt);
    const size_scale = Math.pow(vol_ratio, 0.3);
    for (let index = 0; index < count; index += 1) {
      const at = time + dt * (index + random()) / count;
      let side;
      let size;
      let impact = 0;
      if (metas.length > 0 && random() < meta_rate / rate) {
        let pick = random() * meta_rate;
        let meta = metas[0];
        for (const item of metas) {
          meta = item;
          pick -= item.rate;
          if (pick <= 0) break;
        }
        side = meta.side;
        size = lognormal(meta.child, 0.6);
        impact = META_IMPACT_TICKS;
      } else {
        const gap = Math.max(-3, Math.min(3, fair_ticks() - (best_bid + best_ask) / 2));
        const logit = 1.6 * gap + 0.5 * last_side + meta_bias;
        side = random() < 1 / (1 + Math.exp(-logit)) ? 1 : -1;
        size = random() < BLOCK_PROBABILITY ? lognormal(80, 0.6) : lognormal(2, 0.85);
      }
      execute(side, Math.max(1, Math.round(size * size_scale)), at, impact);
      excitation += HAWKES_JUMP;
    }
    maintain_book(dt);
    time += dt;
    const bar_time = Math.floor(time / interval) * interval;
    if (bar === null || bar_time > bar.time) roll_bar(bar_time, bar?.close ?? price_of(best_bid));
  }
  function window_side(tick) {
    if (tick <= best_bid && tick > best_bid - DEPTH_LEVELS) return 1;
    if (tick >= best_ask && tick < best_ask + DEPTH_LEVELS) return 2;
    return 0;
  }

  // Emit the ladder changes since the previous flush: removals first, then sizes, so a level that
  // changed side never appears on both sides of the book.
  function flush_depth(micros) {
    const stamp = Math.max(last_depth_micros, micros);
    last_depth_micros = stamp;
    const low = Math.min(emitted_low, best_bid - DEPTH_LEVELS + 1);
    const high = Math.max(emitted_high, best_ask + DEPTH_LEVELS - 1);
    for (let tick = low; tick <= high; tick += 1) {
      const side = window_side(tick);
      const size = side === 0 ? 0 : level(tick);
      const target_side = size > 0 ? side : 0;
      if (emitted_side[tick] !== 0 && emitted_side[tick] !== target_side) {
        push(depth_rows.times, stamp);
        push(depth_rows.sides, emitted_side[tick] - 1);
        push(depth_rows.prices, price_of(tick));
        push(depth_rows.sizes, 0);
        emitted_side[tick] = 0;
        emitted_size[tick] = 0;
      }
    }
    for (let tick = best_bid - DEPTH_LEVELS + 1; tick < best_ask + DEPTH_LEVELS; tick += 1) {
      const side = window_side(tick);
      if (side === 0) continue;
      const size = level(tick);
      if (size <= 0 || (emitted_side[tick] === side && emitted_size[tick] === size)) continue;
      push(depth_rows.times, stamp);
      push(depth_rows.sides, side - 1);
      push(depth_rows.prices, price_of(tick));
      push(depth_rows.sizes, size);
      emitted_side[tick] = side;
      emitted_size[tick] = size;
    }
    emitted_low = best_bid - DEPTH_LEVELS + 1;
    emitted_high = best_ask + DEPTH_LEVELS - 1;
  }

  function depth_snapshot() {
    for (let tick = emitted_low; tick <= emitted_high; tick += 1) {
      emitted_side[tick] = 0;
      emitted_size[tick] = 0;
    }
    const side = (bid) => {
      const prices = [];
      const amounts = [];
      for (let distance = 0; distance < DEPTH_LEVELS; distance += 1) {
        const tick = bid ? best_bid - distance : best_ask + distance;
        const size = level(tick);
        if (size <= 0) continue;
        prices.push(price_of(tick));
        amounts.push(size);
        emitted_side[tick] = bid ? 1 : 2;
        emitted_size[tick] = size;
      }
      return { prices: Float64Array.from(prices), sizes: Float64Array.from(amounts) };
    };
    emitted_low = best_bid - DEPTH_LEVELS + 1;
    emitted_high = best_ask + DEPTH_LEVELS - 1;
    last_depth_micros = Math.floor(time * 1e6);
    return { timestamp_micros: last_depth_micros, bids: side(true), asks: side(false) };
  }

  function take_trades() {
    const count = trades.times.length;
    const missing = () => new Float64Array(count).fill(Number.NaN);
    return {
      timestamps_micros: take(trades.times),
      prices: take(trades.prices),
      volumes: take(trades.volumes),
      aggressors: take(trades.sides),
      bids: missing(),
      asks: missing(),
      sequences: missing(),
      trade_ids: take(trades.ids),
      conditions: new Uint32Array(count),
      session_ids: take(trades.sessions),
    };
  }

  function take_depth() {
    return {
      timestamps_micros: take(depth_rows.times),
      sides: take(depth_rows.sides),
      prices: take(depth_rows.prices),
      sizes: take(depth_rows.sizes),
    };
  }

  function take_bars() {
    const out = finished_bars.splice(0, finished_bars.length);
    if (bar !== null) out.push({ ...bar });
    return out;
  }

  return {
    get time() { return time; },
    quote: () => ({ bid: price_of(best_bid), ask: price_of(best_ask) }),
    step,
    set_record_trades(value) { record_trades = value; },
    flush_depth,
    depth_snapshot,
    take_trades,
    take_depth,
    take_bars,
  };
}

function buffers(...groups) {
  return groups.flatMap((group) => Object.values(group).map((value) => value.buffer));
}

let market = null;
let speed = 1;
let timer = null;

function generate_history(config) {
  const interval = config.interval_seconds;
  const end = config.now_seconds;
  const first_bar = (Math.floor(end / interval) - config.history_bars + 1) * interval;
  const tape_start = end - config.tape_bars * interval;
  const depth_start = Math.floor((end - config.depth_history_bars * interval) / config.depth_bucket_seconds)
    * config.depth_bucket_seconds;
  market = create_market({ ...config, start_time: first_bar - 120 });
  // Two discarded minutes settle the book and the Hawkes intensity before the first shown bar.
  while (market.time < first_bar) market.step(STEP_SECONDS);
  market.take_bars();
  let snapshot = null;
  let next_bucket = depth_start + config.depth_bucket_seconds;
  while (market.time + STEP_SECONDS <= end) {
    market.set_record_trades(market.time >= tape_start);
    if (snapshot === null && market.time >= depth_start) snapshot = market.depth_snapshot();
    market.step(STEP_SECONDS);
    // A bucket's rows are stamped just inside it, so the engine closes each history column on
    // the book as it stood at the end of that bucket.
    if (snapshot !== null && market.time >= next_bucket) {
      market.flush_depth(Math.round(next_bucket * 1e6) - 1);
      next_bucket += config.depth_bucket_seconds;
    }
  }
  snapshot ??= market.depth_snapshot();
  market.flush_depth(Math.floor(market.time * 1e6));
  const bars = market.take_bars();
  const columns = {
    times: Float64Array.from(bars, (bar) => bar.time),
    open: Float64Array.from(bars, (bar) => bar.open),
    high: Float64Array.from(bars, (bar) => bar.high),
    low: Float64Array.from(bars, (bar) => bar.low),
    close: Float64Array.from(bars, (bar) => bar.close),
    volume: Float64Array.from(bars, (bar) => bar.volume),
  };
  const trades = market.take_trades();
  const depth = market.take_depth();
  self.postMessage({
    type: "history",
    sim_time: market.time,
    bars: columns,
    trades,
    depth_snapshot: snapshot,
    depth,
    quote: market.quote(),
  }, [
    ...buffers(columns, trades, depth),
    snapshot.bids.prices.buffer, snapshot.bids.sizes.buffer,
    snapshot.asks.prices.buffer, snapshot.asks.sizes.buffer,
  ]);
  market.set_record_trades(true);
}

function stream() {
  let last_real = performance.now();
  let target = market.time;
  timer = setInterval(() => {
    const now = performance.now();
    target += Math.min(MAX_ADVANCE_SECONDS, (now - last_real) / 1000 * speed);
    last_real = now;
    if (market.time + STEP_SECONDS > target) return;
    while (market.time + STEP_SECONDS <= target) market.step(STEP_SECONDS);
    market.flush_depth(Math.floor(market.time * 1e6));
    const trades = market.take_trades();
    const depth = market.take_depth();
    self.postMessage({
      type: "batch",
      sim_time: market.time,
      bars: market.take_bars(),
      trades,
      depth,
      quote: market.quote(),
    }, buffers(trades, depth));
  }, POST_INTERVAL_MS);
}

self.onmessage = (event) => {
  const message = event.data;
  if (message.type === "start" && market === null) {
    speed = message.speed;
    generate_history(message);
    stream();
  } else if (message.type === "speed") {
    speed = message.speed;
  }
};
