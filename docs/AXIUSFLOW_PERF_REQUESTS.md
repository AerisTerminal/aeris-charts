# Axiusflow performance requests — engine response

Response to the seven work items Axiusflow raised after its market-data path review. This document
is the PR body: what shipped, what was measured, what did not ship, and the answers to the three
questions asked.

Consumer constraints honoured throughout: **additive API only** (nothing renamed or removed),
snake_case public API, no regression to the Canvas2D fallback, and each item releasable on its own.
Axiusflow pins `^0.8.3`, which under npm semver for a `0.x` package resolves to `>=0.8.3 <0.9.0`, so
each item ships as a patch bump within `0.8.x` and is picked up without a manifest change.

| Item | Status | Version |
|---|---|---|
| 1 — Frame and backend telemetry | **Shipped** | 0.8.5 |
| 2 — Typed streaming append | **Shipped** | 0.8.6 |
| 6 — Memory ceiling and windowing | **Shipped** | 0.8.7 |
| 3 — `SharedArrayBuffer` ring source | **Shipped** | 0.8.8 |
| 7 — Build flags | **Shipped** (one flag deliberately omitted, see below) | 0.8.9 |
| 4 — Axis/crosshair into the GPU pass | **Not shipped** — design validated, blocking decision below | — |
| 5 — `OffscreenCanvas` worker rendering | **Not started**, per the request's own precondition | — |

---

## Answers to the three questions

### 1. Does `set_data_typed` mutate its input arrays?

**No — and it never will.** This is now an explicit, documented guarantee on both
`series_api.set_data_typed` and `ohlc_columns` in the published types.

The engine copies each column into its own storage at the boundary (`Float64Array::to_vec` on the
wasm side) and does all sorting, deduping and sanitizing on those copies. Two consequences you can
rely on:

- **Aliasing is safe.** Passing one view as all four of `open`/`high`/`low`/`close` — the documented
  way to express the single-value convention for line/area/histogram series — cannot corrupt. Your
  volume histogram and line series are fine as written.
- **`SharedArrayBuffer` views are safe**, and the producer may rewrite the buffer as soon as the call
  returns. Nothing is retained past the call.

The same guarantee covers the new `update_typed`. There is a browser spec asserting it directly: it
passes one `Float64Array` as all four price channels in a 500-row batch and checks the array is
byte-identical afterwards.

### 2. What is the current series length cap?

**There was none, and that was undocumented.** A series grew for as long as the host appended to it.
Combined with wasm linear memory never shrinking, an open-ended live session grew monotonically with
no public way to observe it — exactly the gap Item 6 describes.

That is now both documented and addressable: the default is still unbounded (stated as such in the
published types), and `series_options.max_points` gives a hard per-series ceiling with oldest-first
eviction. See Item 6 below for the eviction schedule, which is amortized rather than per-point for a
reason worth knowing about.

### 3. Is `timestamp-query` available on the target platforms, or should `gpu_ms` be expected to be
`null` in practice?

**Available on Chromium-family desktop, and `gpu_ms` resolves there.** Verified end to end in the
browser suite rather than inferred: the spec queries `adapter.features.has("timestamp-query")` and,
where true, asserts `frame_stats().gpu_ms` becomes non-null and finite. It passes on Chromium with
the demo suite's launch flags (which include `--enable-dawn-features=allow_unsafe_apis` and
`--enable-webgpu-developer-features`).

Two caveats for your planning:

- **It is a per-adapter capability, not a browser-version guarantee.** Some drivers and most mobile
  adapters do not expose it, and Safari's WebGPU does not currently. Treat `null` as a normal
  runtime state, not an error — the engine requests the feature only when the adapter advertises it,
  so a device without it still creates a chart and simply reports `gpu_ms: null`.
- **Collection is armed lazily and readback is asynchronous.** The first `frame_stats()` call arms
  GPU timing; expect one or two `null` reads immediately after chart create even where the feature
  exists. Thereafter `gpu_ms` is the most recently *resolved* frame, which can lag the last presented
  frame by a frame or two. That is deliberate — it keeps the readback off the critical path — and it
  does not affect a rolling p99.

If you need a GPU-time number that is guaranteed present, do not build the 8 ms budget check on
`gpu_ms` alone. `cpu_ms` is always available on both backends and is the part of the frame the engine
actually controls.

---

## Item 1 — Frame and backend telemetry (0.8.5)

`chart_api.frame_stats()` returns the requested shape, plus two additions.

```ts
export interface frame_stats {
  cpu_ms: number
  gpu_ms: number | null
  draw_calls: number
  dropped_frames: number
  presented_frames: number
  memory_bytes: number
  canvas2d_ops: number   // addition
  ring_overruns: number  // addition
}
```

**`canvas2d_ops`** counts the Canvas2D paint ops the *engine* issues per frame — the axis/crosshair
overlay, plus the pane executor on the Canvas2D backend. It excludes your plugin canvas primitives,
which are package-side. It exists because Item 4's acceptance criterion ("zero Canvas2D draw
operations per frame") is otherwise unfalsifiable from outside, and because it makes the current cost
visible today: on the WebGPU backend this is the axis chrome, and it is non-zero on every frame.

**`ring_overruns`** is Item 3's overrun counter, as suggested.

Implementation notes that affect how you use it:

- **`cpu_ms`** covers the whole render: layout, axis-frame construction, engine frame build, plugin
  passes, and command encoding. Two `performance.now()` reads per frame.
- **`gpu_ms`** uses WebGPU timestamp queries with at most one readback in flight; frames skip their
  timestamp writes while a readback is pending, so the cost is bounded regardless of frame rate. See
  question 3 for the `null` cases.
- **`memory_bytes`** reads wasm linear memory directly (`memory_size(0) * 65536`) with no JS round
  trip. Also serves Item 6.
- **Collection is free when unread.** The record is fixed-size — no history buffer, no per-frame
  allocation, no string formatting. GPU timing is armed by the first `frame_stats()` call, so a
  chart nobody instruments never creates a query set at all.
- **Reading is free too.** The façade transfers through one scratch `Float64Array` per chart, sized
  by the engine, so polling every frame adds no JS-heap allocation.

### Measured

Chromium/SwiftShader, WebGPU backend, 1000-bar chart:

| Measurement | Result |
|---|---|
| `frame_stats()` read cost | **0.072 µs/call** (50k-iteration tight loop, best of 3) |
| Engine `cpu_ms` median, polled every frame | 0.6 ms |
| Engine `cpu_ms` median, unpolled | 0.6 ms |
| `gpu_ms` where `timestamp-query` supported | non-null, finite |
| `gpu_ms` on Canvas2D fallback | `null`, no throw |

The acceptance criterion was "reading every frame for 60s does not measurably raise `cpu_ms`". A
timed A/B against `render()` cannot resolve that here — a SwiftShader frame is ~25 ms with ~0.4 ms of
pass-to-pass spread, three orders of magnitude coarser than the cost in question — so the spec
measures the read directly in a tight loop and separately checks the engine's record does not drift.
`tests/frame-stats.spec.mjs`.

---

## Item 2 — Typed streaming append (0.8.6)

`series_api.update_typed(columns)` as specified, reusing the `ohlc_columns` shape.

Semantics are deliberately not a second policy. The batch runs through the same repair pipeline
`set_data_typed` uses — drop non-finite, stable sort by time, collapse duplicate times last-wins,
all-NaN rows surviving as whitespace — and then each surviving row goes through the same engine entry
point `update()` uses. So a one-row batch *is* `update()`, and a 500-row batch is that repeated, with
one `data_changed("update")` for the call rather than per row.

### Cost model — please read this one

For the streaming shape (every row at or past the chart's last timestamp) cost is linear in the batch
and independent of series length: each row takes the engine's single-append fast path.

**A row that lands before the chart's last timestamp is a mid-history insert and costs a reindex of
the shared time axis** — exactly as the same row does through `update()`. The batch does not coalesce
those into one reindex. This is not hypothetical: it surfaced while writing the acceptance test, when
a second series appending at timestamps below the main series' last point turned a 1M-point append
into an O(n²) walk. If you have several series on one chart with different time bases, make sure the
one you are streaming into is at the global tip, or use `set_data_typed` to rewrite history. Both
behaviours are stated in the published type docs.

### Measured

| Measurement | Result |
|---|---|
| 1M points, 1000-row batches, appending at the tip | **300 ms (~3.3M points/sec)** |
| JS heap growth over that run | **384 KB** |
| One-row batch vs `update()` | identical `data()`, identical `data_changed` sequence, identical `last_value_data`, byte-identical presented frame |
| 500-row batch | one call, one `data_changed("update")` |

1M per-point JS objects would be tens of megabytes; 384 KB is incidental loop churn.
`tests/update-typed.spec.mjs`.

---

## Item 3 — `SharedArrayBuffer` ring source (0.8.8)

`series_api.set_ring_source(buffer, layout)` with the requested `ring_source_layout`. The engine
drains once per frame on its own tick; `set_ring_source(null)` unbinds.

### On "rows are read in place — no copy into JS"

Worth being precise, because the literal reading is not achievable. wasm cannot address a
`SharedArrayBuffer` directly — every read of it crosses the JS boundary. Reading five `f64`s per row
through a `DataView` would be five JS calls per row, which is strictly worse than the object
allocation this API exists to remove.

What the implementation does instead: each drain copies the contiguous run(s) of new row bytes
straight into engine memory in **one `TypedArray.set` per run — at most two per frame**, since a ring
wrap splits the window — and parses them in Rust with no further boundary crossings. The staging
buffer is sized once at bind time to `capacity * row_stride`, so no allocation happens per row or per
frame, and nothing is ever materialized as a JS value. That is the property the request is actually
after; the byte copy is unavoidable and is one bulk memcpy of exactly the new rows.

### Contract with your producer

Documented on `ring_source_layout.write_cursor_offset`, restated here because getting it wrong
produces torn rows rather than an error:

1. **Write the row's bytes first, then publish the incremented cursor with `Atomics.store`.** The
   engine reads the cursor with `Atomics.load` and then reads only rows strictly below it, so a row
   is never read half-written *provided* that order holds.
2. The cursor is the count of rows **ever written**, not a ring slot. Slot is `count % capacity`.
3. The cursor may overflow `Int32` — at 50k rows/sec that is ~12 hours. Differences are computed with
   wrapping arithmetic, so the wrap is transparent. There is a unit test for it.

### Overrun

If the producer wrote more than `capacity` rows between two drains, the oldest are already
overwritten. The engine renders the **newest `capacity` rows** — a contiguous, in-order window, never
a torn mix of mixed-age slots — and adds the shortfall to `frame_stats().ring_overruns`. Watch that
counter to size `capacity` against your burst rate. It is reported there rather than as a console
warning because an overrun under load would otherwise flood the console at frame rate.

### Other behaviours

- Binding starts from the producer's **current** cursor, so it picks up new rows rather than replaying
  whatever is already in the ring.
- Rows apply in ring order, each appending or replace-lasting like `update`. Unlike `update_typed`
  there is no per-batch sort or dedupe — a ring is a stream and its producer writes in order — and a
  non-finite row is dropped.
- The drain loop is one `requestAnimationFrame` per chart, not per series, and it only exists while
  at least one ring is bound. A bound-but-quiet ring costs one atomic load per frame and does **not**
  force a repaint, so an idle ring does not pin the chart at frame rate.
- Removing a series drops its ring with it, so `remove_series` cannot leak the shared buffer.
- Malformed layouts are rejected at bind time with a specific reason (channel past `row_stride`,
  misaligned cursor, ring larger than the buffer, zero capacity), and a rejected bind leaves no ring
  bound.
- Requires the page to be cross-origin isolated, which is what makes `SharedArrayBuffer` available at
  all. The demo test server now sends `Cross-Origin-Opener-Policy: same-origin` and
  `Cross-Origin-Embedder-Policy: require-corp` for the specs.

### Measured

Real Web Worker producer, real `SharedArrayBuffer`, `tests/ring-source.spec.mjs`:

**Frame cost is flat as producer rate scales 500×.** Engine-reported `cpu_ms` median over 100 sampled
frames at each rate:

| Producer rate | Rows delivered in the window | Median `cpu_ms` |
|---|---|---|
| 100 rows/sec | 23 | 0.775 ms |
| 50,000 rows/sec | 10,873 | **0.510 ms** |

A 473× increase in delivered rows produced **0.66×** the frame cost — i.e. no scaling at all, with the
difference inside noise. This is the structural property the item asked for: the drain is one atomic
load plus at most two bulk copies per frame regardless of tick rate.

**No engine call per tick.** The spec instruments the façade's per-point engine entry points
(`update_series_bar_styled`, `update_series_bars_typed`) and asserts the count is **exactly 0** across
500 produced rows, while `drain_ring_sources` is called once per frame (<50 times). Notifications
follow the same shape: 800 rows produce fewer than 10 `data_changed("update")` events, one per drained
frame.

**Overrun.** 500 rows published into a 64-row ring, with the drain suppressed for the burst so the
timing is deterministic: exactly the newest 64 rows render, verified **contiguous and ascending** (the
failure mode being ruled out is a torn read mixing fresh and stale slots), the newest row is present,
and `ring_overruns` reports exactly 436.

A companion spec runs the same burst 5,000 rows deep with the drain loop left free to race it, where
the row count is genuinely unpredictable, and asserts the conservation invariant instead: every
published row is either in the series or counted in `ring_overruns`. Nothing is lost silently.

**Unbind.** `set_ring_source(null)` leaves 0 rings bound, delivers nothing further from a producer
that keeps writing, stops the per-frame drain loop **entirely** (asserted: zero `drain_ring_sources`
calls afterwards), and explicit `update_typed` works again on top of what the ring delivered.

**Rejection.** Every malformed layout is refused at bind time with a specific message — zero capacity,
a channel overrunning `row_stride`, a misaligned cursor, a ring larger than the buffer, a missing
layout, and a plain `ArrayBuffer` instead of a shared one — and no rejected attempt leaves a ring
bound.

Plus 13 unit tests on the drain arithmetic off-browser, covering contiguous windows, wrap splitting
oldest-first, a window ending exactly at the wrap, exact-capacity (not an overrun), `Int32` cursor
overflow, a producer reset, and an exhaustive sweep asserting no plan can ever address outside the
ring.

---

## Item 6 — Memory ceiling and windowing (0.8.7)

`memory_bytes` shipped with Item 1. Retention policy documented (see question 2). `max_points` added.

`series_options.max_points` is a **hard ceiling** — the series never holds more — with oldest-first
eviction, applied at option-set time, on full `set_data`/`set_data_typed` installs, and on every
streaming append.

**Eviction is amortized, not per-point, and this is observable.** Trimming shifts rows and rebuilds
the shared time axis, so it is O(total rows); evicting on every append would make a long streaming
session quadratic. Instead the engine trims back to `max_points - max_points / 32` once the count
exceeds the cap. The count therefore sits in `[max_points - max_points/32, max_points]`, so `data()`
can return slightly fewer rows than `max_points`. Amortized cost per appended point is constant. The
hysteresis is stated in the published types because you can see it.

Note this is a *point* count, not a time window — the retained span depends on your bar interval.

### Measured

32 simulated hours at one bar per second (115,200 points), comparing growth while the retention
window fills against growth long after it is full — a single 8-hour pass cannot distinguish a plateau
from linear growth, because memory legitimately rises while the window fills:

| Series | Early growth (first quarter) | Late growth (second half) | Rows held |
|---|---|---|---|
| Capped to an 8-hour window | 3.47 MB | **0 bytes** | 28,704 of 28,800 |
| Uncapped (control) | 1.05 MB | **+7.47 MB, still climbing** | 115,200 |

A flat plateau, with the uncapped control confirming the measurement is sensitive to the thing being
tested. `tests/retention.spec.mjs`.

---

## Item 7 — Build flags (0.8.9)

### What was already there, and what was missing

| Flag | Before | After |
|---|---|---|
| `codegen-units = 1` | already set | unchanged |
| `wasm-opt` in the release pipeline | already run, at `-Oz` | still run, `-Oz` retained — see below |
| `lto` | `"thin"` | `"fat"` |
| `RUSTFLAGS="-C target-feature=+simd128"` | **not set anywhere** | set in `.cargo/config.toml` |

`simd128` lives in `.cargo/config.toml` rather than as a CI environment variable, so it is part of
the checked-in build definition and applies identically to local builds, `wasm-pack`, and the release
pipeline. It also required adding `--enable-simd` to the `wasm-opt` flag list — without it wasm-opt
refuses to read a module containing `v128` instructions and the release build fails outright, which
is worth knowing if you ever build the engine yourself.

Baseline support for SIMD128: Chrome/Edge 91, Firefox 89, Safari 16.4. Every browser that can run the
WebGPU path is well past that, and so is the Canvas2D fallback target set.

### Measured — size

Four variants built from one identical source tree, differing only in flags:

| Variant | Raw `.wasm` | vs baseline | Gzipped | vs baseline |
|---|---|---|---|---|
| A — baseline (no simd, thin LTO, `-Oz`) | 919,432 | — | 360,741 | — |
| B — `+simd128` | **911,503** | **−7,929 (−0.86%)** | **359,312** | −1,429 (−0.40%) |
| C — `+simd128`, fat LTO | 911,612 | −7,820 (−0.85%) | 359,376 | −1,365 (−0.38%) |
| D — `+simd128`, fat LTO, `wasm-opt -O4` | 932,893 | +13,461 (+1.46%) | 362,456 | +1,715 (+0.48%) |

SIMD makes the binary *smaller* — some loops compile tighter with vector instructions than with
their scalar unrolled equivalents. Fat LTO is size-neutral here (+109 bytes over thin, noise). `-O4`
costs +21 KB raw over `-Oz`.

The shipped configuration is **C**.

### Measured — throughput

Same four variants, run through the browser benchmark round-robin (three interleaved rounds each, so
thermal drift and session variance spread across variants rather than favouring whichever ran first),
best-of-5 within each round. Chromium, WebGPU on SwiftShader. `tests/engine-bench.spec.mjs`.

| Metric | A baseline | B +simd128 | C +fat LTO | D +`-O4` |
|---|---|---|---|---|
| 1M-bar `set_data_typed` (ms) | 161.9 | 162.2 (+0.2%) | 163.4 (+0.9%) | 165.3 (+2.1%) |
| `fit_content` + render, 1M bars (ms) | 1.6 | 1.6 (+1.6%) | 1.3 (−21.9%) | 1.5 (−5.6%) |
| Frame build, 50k bars (ms) | 0.6 | 0.6 (−7.2%) | 0.5 (−20.1%) | 0.5 (−11.3%) |
| Hit test, 10k drawings (µs/probe) | 1220.7 | 1207.1 (−1.1%) | 1224.6 (+0.3%) | 1218.0 (−0.2%) |

**Read these as "no measurable difference", not as the percentages they appear to be.** The
within-variant spread across rounds exceeds every between-variant difference — the baseline's own 1M
install ranged 161.9–169.4 ms, and the two sub-millisecond metrics are quantized by Chrome's ~100 µs
`performance.now()` clamp, giving them a resolution of order 15%. The honest noise floor of this
harness is roughly ±5% on the 1M install and ±15% on the rest. Nothing here clears it. The raw
per-round samples are in the benchmark log.

**So: expect no throughput win from SIMD128 on these workloads.** That is worth stating plainly
rather than shipping the flag and implying a gain. The reason is visible in the profile shape — these
paths are branchy and allocating (a sanitize pass with a stable sort, R-tree traversal, column
installs) rather than the tight float loops LLVM auto-vectorizes. Dependencies are also pinned at
`opt-level = "z"` by the release profile, and the whole module then goes through a size-directed
`wasm-opt -Oz`.

SIMD128 is still enabled, on the merits that actually hold: it is free, it makes the artifact
slightly smaller, and it removes the flag as a variable for any future vectorizable work (the
autoscale and transform passes are the plausible candidates). What it is not is a fix for a
frame-time problem.

### Why `-O4` is omitted

`-O4` is the one requested flag not enabled. It costs +21 KB raw / +1.7 KB gzipped and returns
nothing resolvable above the noise floor on any of the four workloads. Download size is a product
metric and the engine's hot loops are already gated at 5× the 60 fps budget, so `-Oz` is retained —
the same trade the profile already documented, now with numbers behind it rather than an assertion.
The rationale is recorded in `crates/origin_wasm/Cargo.toml` next to the flag list so it is visible
at the point of change.

### One practical note if you build the engine yourself

Enabling `+simd128` **requires** adding `--enable-simd` to the `wasm-opt` flag list. Without it,
wasm-opt refuses to read a module containing `v128` instructions and the release build fails outright
(not silently — `wasm-pack` exits non-zero). Both changes are in this PR; they cannot be split.

---

## Item 4 — Axis and crosshair into the GPU pass: not shipped

This one has a decision in it that is yours, not ours, so it is written up rather than half-built.

### The premise is correct

On a WebGPU backend, every presented frame is still followed by Canvas2D paint work on the main
thread for the axis chrome and the crosshair labels. You can now measure it directly:
`frame_stats().canvas2d_ops` is non-zero on every WebGPU frame, and it counts exactly that work.

One correction to the framing: that work is issued from Rust through `web-sys`, not from JavaScript
(`canvas_plugins.d.ts`'s "no engine (wasm) involvement" describes the *plugin overlay*, a different
canvas). It still runs on the main thread inside the frame, so the cost is real and the conclusion is
unchanged — but it is engine code, which is why `canvas2d_ops` can count it precisely.

### The design is feasible and mostly already built

Everything needed exists:

- The engine already produces an `AxisFrame` with borders, ticks, separators, boxed labels and
  crosshair labels as pure geometry.
- The Prim IR already has every shape required: `Rect` for borders/ticks/separators, `RoundRect` with
  per-corner radii for boxed label backgrounds (matching the current TradingView-style side radius
  exactly), and `Text`.
- `Prim::Text` on the WebGPU path already resolves through a browser-rasterized glyph atlas
  (`chart/text_runs.rs`) that is measured 0-diff against the Canvas2D executor's `fillText`.

So the work is: convert `AxisFrame` to prims, render them as one additional unscissored draw group
after the pane groups, and delete the `draw_axes_2d` pass. Not a glyph-atlas project — the atlas is
already there and already proven.

### The blocking problem: text at fractional DPR

The axis overlay draws text as `font: {size}px` on a context **scaled by DPR**. The atlas path
rasterizes at `font: {size * dpr}px` on an **unscaled** context. Those are not the same pixels, and
the codebase already knows it — `inner_render.rs` carries this comment on the axis label pass:

> Using an independently hinted `size*dpr` bitmap font is observably different at fractional DPR even
> when every logical coordinate is identical.

The GPU path can only do the latter. So **exact pixel parity for axis text is not achievable at
fractional DPR**, which is the case that matters — your terminal runs on 1.25x, 1.5x and 1.75x
displays, and the browser parity suite itself runs at `deviceScaleFactor: 1.5`.

There is a second, smaller instance of the same issue: the current pass applies a per-label
`y_mid_correction` derived from a browser `actualBoundingBoxAscent/Descent` measurement. That is
solvable — the host can measure and bake the correction into the prim's `y` — but it is more evidence
that the two text paths are not interchangeable by construction.

### Which means the request as written cannot be satisfied as written

The request says the Canvas2D backend keeps its current path, and separately that the documented
backend-neutral guarantee (identical output across `webgpu` and `canvas2d`) must still hold. With
axis text moved to the atlas on WebGPU only, those two are in direct conflict: WebGPU axis text would
diverge from Canvas2D axis text at fractional DPR.

There are two coherent ways forward, and picking between them is a product call:

**Option A — move axis rendering into the frame for _both_ backends.** Axis prims are executed by the
Canvas2D executor on the 2D backend and by the GPU pass on the WebGPU backend. The two backends stay
identical to each other, the backend-neutral guarantee holds, and the axis chrome changes very
slightly versus today at fractional DPR on *both* backends. This is the design we would recommend. It
needs the browser parity suite re-baselined and the divergence-from-today documented.

**Option B — keep the axis on Canvas2D and reduce, not eliminate, the per-frame cost.** Repaint the
overlay only when the axis frame actually changes, and on a pure crosshair move repaint only the
crosshair labels. Pixel output is untouched. This does not reach "zero Canvas2D draw operations", but
it removes most of the per-frame variance for the crosshair-movement case specifically, which is the
case the request identifies as the largest contributor.

**What we need from you:** whether a small, documented change to axis text rendering at fractional DPR
on both backends is acceptable. If yes, Option A. If the axis chrome is pixel-frozen, Option B is the
honest ceiling and Item 4 should be rewritten around it.

`canvas2d_ops` shipped in Item 1 specifically so that whichever option is chosen can be verified, and
so the current cost is visible while the decision is open.

---

## Item 5 — `OffscreenCanvas` worker rendering: not started

Per the request's own precondition: not to be started until Items 3 and 4 are merged and measured,
and conditional on frame-time p99 still missing 8 ms after them. Item 4 is unresolved, so the
precondition is not met.

One piece of groundwork landed incidentally: the telemetry clock is resolved off the global object
rather than off `window`, so it works in a `Worker` as well as a `Window`. Nothing else in the
offscreen path was touched.

---

## Test suite state

Rust: `cargo fmt --check`, `cargo clippy --workspace --all-targets` and
`cargo clippy -p origin_wasm --target wasm32-unknown-unknown` are clean with zero warnings, and the
whole workspace test suite passes, including the golden-image regression. New coverage: 13 unit tests
on the ring drain arithmetic, 5 on the telemetry record, 4 on `max_points` retention in the engine,
and 1 on `DataLayer::trim_front`.

TypeScript: `tsc --noEmit` and `oxlint` clean.

Browser (Chromium, WebGPU on SwiftShader): **four specs fail, and all four fail identically on the
pre-PR commit** — three `backend-parity` fidelity comparisons against the reference library, and one
`prim-text` probe whose fixture assertion (`fixture must have series pixels under the pink text`)
reads 0. Verified by checking out `67c05ae`, rebuilding, and re-running: same four, same assertions.
They are consistent with the CI note that this job's pixel thresholds are calibrated to a specific
SwiftShader/Dawn build. **No new failures.**

Final full-suite run: **112 passed, 4 failed (all four pre-existing), 1 skipped.**

New browser specs added by this PR, all passing: `frame-stats.spec.mjs` (5), `update-typed.spec.mjs`
(5), `retention.spec.mjs` (5), `ring-source.spec.mjs` (8). Plus `engine-bench.spec.mjs`, the Item 7
measuring instrument, which is the skipped one.

Two things worth knowing about the new specs:

- **The retention plateau spec takes ~6 minutes**, simulating 32 hours of one-bar-per-second streaming.
  That is the only way to distinguish a plateau from linear growth, so it earns its runtime — but it
  should not surprise anyone.
- **`engine-bench.spec.mjs` is opt-in behind `ORIGIN_BENCH=1`.** It installs 1M bars five times over,
  growing wasm linear memory to ~300 MB; since linear memory never shrinks, that residue was measured
  starving the *next* spec's page init past a 30s timeout when the benchmark ran as part of the default
  suite. It is a measuring instrument rather than a gate, so gating it is the right shape anyway. Run
  it with `ORIGIN_BENCH=1 npx playwright test tests/engine-bench.spec.mjs`.

## Non-goals

None of the following were undertaken: no change premised on sub-millisecond wire latency, no
replacement of the Canvas2D fallback or of any backend-neutral plugin guarantee, no breaking API
change or rename, no move away from snake_case, and no transport, networking or data-fetching layer
bundled into the engine.
