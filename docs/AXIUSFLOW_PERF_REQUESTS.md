# Axiusflow performance requests — engine response

Response to the seven work items Axiusflow raised after its market-data path review. This document
is the PR body: what shipped, what was measured, what did not ship, and the answers to the three
questions asked.

Consumer constraints honoured throughout: **additive API only** (nothing renamed or removed),
snake_case public API, no regression to the Canvas2D fallback, and each item independently releasable.
Axiusflow pins `^0.8.3`, which under npm semver for a `0.x` package resolves to `>=0.8.3 <0.9.0`, so
all boundaries remain compatible without a manifest change.

The version column records the intended cumulative patch boundary at which each independently
releasable item becomes available. This working tree is the cumulative **0.8.11** state; it does not
claim that the local tree itself is a separately published 0.8.10 artifact.

| Item | Status | Version |
|---|---|---|
| 1 — Frame and backend telemetry | **Shipped** | 0.8.5 |
| 2 — Typed streaming append | **Shipped** | 0.8.6 |
| 6 — Memory ceiling and windowing | **Shipped** | 0.8.7 |
| 3 — `SharedArrayBuffer` ring source | **Shipped** | 0.8.8 |
| 7 — Build flags | **Shipped** (one flag deliberately omitted, see below) | 0.8.9 |
| 4 — Axis/crosshair into the GPU pass | **Shipped** | 0.8.10 |
| 5 — `OffscreenCanvas` worker rendering | **Shipped** | 0.8.11 |

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

**`canvas2d_ops`** counts the Canvas2D paint ops the *engine* issues per frame. On the Canvas2D
backend it covers both pane and shared axis/crosshair primitive execution. On WebGPU the axis,
crosshair and watermark now execute in a final unscissored GPU primitive group, so a moving
crosshair reports **zero** Canvas2D engine operations. Package canvas plugins remain package-owned
and are excluded; they are clipped to their owning pane so they cannot cover the price or time axes.

**`ring_overruns`** is Item 3's overrun counter, as suggested.

Implementation notes that affect how you use it:

- **`cpu_ms`** covers the whole frame: ring draining and stable-window ingestion, layout,
  axis-frame/primitive construction, engine frame build, plugin passes, command encoding and
  presentation. Two `performance.now()` reads per frame.
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

| Measurement | Observed across two acceptance runs |
|---|---|
| 1M points, 1000-row batches, appending at the tip | **311–319 ms (~3.13–3.21M points/sec)** |
| JS heap growth over that run | **−977,744 to −217,548 bytes** (no net growth) |
| CDP sampled allocations attributed to package/WASM glue | **1,843,856–1,951,052 bytes** (gate: <4 MiB) |
| All CDP sampled allocations, including browser/test harness | 25,941,540–26,374,292 bytes |
| Engine memory after the run | **120,520,704–123,142,144 bytes** |
| One-row batch vs `update()` | identical `data()`, identical `data_changed` sequence, identical `last_value_data`, byte-identical presented frame |
| 500-row batch | one call, one `data_changed("update")` |

The acceptance test now combines end-of-run `usedJSHeapSize` with CDP
`HeapProfiler.startSampling` at a 4096-byte interval. Heap delta alone can miss temporary allocations
that GC reclaims during the run; the sampled package-attributed total directly guards against
per-point JS object churn. `tests/update-typed.spec.mjs`.

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

1. The cursor is the count of rows **ever written**, not a ring slot. Slot is
   `count % capacity`; wrapping `Int32` arithmetic makes cursor overflow transparent.
2. For robust concurrent wrapping, set the optional, aligned per-row `sequence_offset` and publish
   logical count `next` in this order:
   1. `Atomics.store(sequence, ~next)` before touching the slot;
   2. write all row channels;
   3. `Atomics.store(sequence, next)`;
   4. `Atomics.store(write_cursor, next)`.
3. The engine validates every selected sequence before and after copying the complete window into
   its preallocated slab. A changed, stale, or in-progress generation retries without applying a
   partial window; after three failed attempts the frame consumes nothing and the next frame retries.
4. Layouts without `sequence_offset` remain source-compatible. Writing bytes before publishing the
   cursor and the engine's second cursor read reject fully published overlap, but that legacy
   handshake cannot detect a producer already midway through an unpublished overwrite. Use the
   sequence word whenever the producer can lap the consumer during a copy.

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
  misaligned cursor or sequence word, sequence/channel byte overlap, ring larger than the buffer,
  zero capacity), and a rejected bind leaves no ring bound.
- Requires the page to be cross-origin isolated, which is what makes `SharedArrayBuffer` available at
  all. The demo test server now sends `Cross-Origin-Opener-Policy: same-origin` and
  `Cross-Origin-Embedder-Policy: require-corp` for the specs.

### Measured

Real Web Worker producer, real `SharedArrayBuffer`, optional per-row sequence seqlock enabled,
`tests/ring-source.spec.mjs`:

| Measurement | Observed across two acceptance runs |
|---|---|
| Sustained producer throughput | **~49,413–49,967 rows/sec** |
| Median full frame `cpu_ms` at 50k rows/sec | **3.585–3.905 ms** |
| Median full frame `cpu_ms` at ~100 rows/sec | 1.115–1.140 ms |
| Rows delivered across both windows | 38,373–41,670 |
| Ring overruns | **0** |

`cpu_ms` includes ring draining, stable-window copying, parsing, every non-idle candidate attempt
(including failed seqlock retries and stable windows whose rows are all rejected), the single batched
`ChartEngine::update_series_bars` call, recomputation and rendering. Idle rings do not accrue ingest
time. Drain plans and staging storage are reused, and each accepted window applies through one engine
batch instead of one engine call per row. Across the repeated runs, a **497.4–502.7×** achieved-rate
increase raised median full-frame cost by **3.16–3.50×**, remaining below the 8 ms frame budget with
no overrun.

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
a channel overrunning `row_stride`, a misaligned cursor or sequence word, sequence bytes overlapping
any eight-byte time/OHLC channel, a ring larger than the buffer, a missing layout, and a plain
`ArrayBuffer` instead of a shared one — and no rejected attempt leaves a ring bound. A deterministic
in-progress-generation case publishes the cursor while the sequence remains `~next`, proves that the
drain consumes zero rows, then publishes `next` and proves the next drain consumes exactly that row.

Plus 13 unit tests on the drain arithmetic off-browser, covering contiguous windows, wrap splitting
oldest-first, a window ending exactly at the wrap, exact-capacity (not an overrun), `Int32` cursor
overflow, a producer reset, and an exhaustive sweep asserting no plan can ever address outside the
ring.

---

## Item 6 — Memory ceiling and windowing (0.8.7)

`memory_bytes` shipped with Item 1. Retention policy documented (see question 2). `max_points` added.

`series_options.max_points` is a **hard observable ceiling** with oldest-first eviction, applied at
option-set time, on full `set_data`/`set_data_typed` installs, and after every accepted streaming
operation. Ring windows are applied as one engine batch and enforce the cap once at the batch
boundary; transient internal growth during that synchronous batch is not observable by callers, and
the published series is back inside the documented interval before the drain returns.

**Eviction is amortized, not per-point, and this is observable.** Trimming shifts rows and rebuilds
the shared time axis, so it is O(total rows); evicting on every append would make a long streaming
session quadratic. Instead the engine trims back to `max_points - max_points / 32` once the count
exceeds the cap. The externally observable count therefore sits in
`[max_points - max_points/32, max_points]`, so `data()` can return slightly fewer rows than
`max_points`. Amortized cost per appended point is constant. The hysteresis and ring batch-boundary
cadence are stated here because consumers can observe the retained count.

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

## Item 4 — Axis and crosshair in the backend-neutral primitive pass (0.8.10)

Option A was selected and shipped: axis borders, ticks, separators, boxed labels, crosshair lines and
labels, and the watermark are converted from `AxisFrame` into the same backend-neutral `Prim` stream
used by the pane renderers.

- WebGPU executes those primitives in one final **unscissored** group after all pane groups. A
  continuously moving crosshair reports `frame_stats().canvas2d_ops === 0`; no hidden Canvas2D axis
  paint remains on the WebGPU frame path.
- Canvas2D executes the exact same `axis_prims`, preserving the fallback and keeping geometry,
  clipping, draw order and text inputs shared between backends.
- Package `attach_canvas_primitive` support remains intact. Its canvas is clipped to the primitive's
  owning pane, so package content cannot cover either price-axis strip or the time axis.
- Legacy `text_views` compatibility output remains available on the Canvas2D overlay on both
  backends, but every run is clipped to its owning pane so it cannot cover price/time-axis chrome.
  Layer-positioned text should use backend-neutral `primitive_draw_context.text`.
- `take_screenshot(add_top_layer)` retains its public contract: the Canvas2D snapshot helper receives
  the flag and omits shared axis/top-layer primitives when `false` on either live backend. After the
  returned bitmap is copied, the helper immediately repaints the complete warm/live Canvas2D surface;
  the pane-only capture therefore cannot leave a visible fallback chart without its axis chrome.

### Text and parity decision

The existing browser-rasterized whole-run text cache was retained. A true per-glyph atlas redesign
would risk kerning and font-shaping fidelity while the measured path is already comfortably inside
the budget; the continuous-crosshair fixture caused 181 text rasterizations in total.

Exact RGBA identity at fractional DPR is not promised because WebGPU texture sampling and browser
Canvas2D antialiasing can differ at glyph/rounded-edge boundaries. The guarantee is identical source
geometry, ordering, clipping, colors and text runs, with a measured bounded raster residual. On the
DPR 1.5 shared-frame fixture:

| Measurement | Result |
|---|---|
| Differing pixels | **2,550** AA-edge pixels (gate: <=5,000) |
| Maximum channel delta | **43/255** (gate: <=64) |
| Marker/overlay ordering mismatches | **0** |

### Continuous-crosshair acceptance

Chromium/SwiftShader with 50,000 bars and continuous pointer movement:

| Measurement | Observed across two acceptance runs |
|---|---|
| Engine `cpu_ms` p99 | **2.805–5.725 ms** |
| Wall-frame p99 | **2.895–5.775 ms** |
| WebGPU `canvas2d_ops` | **0** |

That clears the requested 8 ms p99 ceiling without replacing the browser-rasterized text cache.
`tests/frame-stats.spec.mjs`, `tests/canvas-primitives.spec.mjs`, and
`tests/backend-parity.spec.mjs` cover the performance, layering, screenshot and parity contracts.

---

## Item 5 — `OffscreenCanvas` worker rendering (0.8.11)

Worker rendering shipped as an additive façade; the existing DOM `create_chart` constructor and its
auto-resize, accessibility, event listeners and plugin model are unchanged.

Rust exports `create_offscreen_chart` for two transferred `OffscreenCanvas` surfaces: one WebGPU
surface and one already-available Canvas2D fallback surface. Two canvases are required because a
canvas cannot switch context type after WebGPU has claimed it. Construction and `resize` receive
explicit CSS width, height and DPR because a worker has no element geometry or `window.devicePixelRatio`.
Text measurement and whole-run rasterization use worker-safe `OffscreenCanvas` 2D contexts, and
backend-loss notification dispatches through `globalThis` rather than `window`.

The TypeScript `offscreen_chart` / `create_offscreen_chart` façade exposes:

- typed `set_data_typed` and `update_typed`, series creation and chart options;
- explicit `resize`, `render`, `fit_content`, `frame_stats` and visible logical range;
- normalized `inject_pointer_event`, `inject_wheel_event` and `inject_key_event` for samples relayed
  by the owner in full-chart CSS coordinates;
- `backend()` plus `subscribe_backend_change(handler)` so the owner can swap the visible stacked HTML
  canvas when runtime WebGPU loss activates the warm Canvas2D surface;
- deterministic cleanup.

The worker-specific option type and runtime validator reject DOM-only settings (`autoSize`,
localization, gesture/kinetic/tracking options and `layout.panes.enableResize`) instead of silently
accepting settings the façade cannot honor. Pointer injection tracks one drag-owning pointer, so a
second pointer cannot inherit scrolling when the owner is canceled.

DOM-only capabilities are deliberately not duplicated in the worker façade: `ResizeObserver`,
accessibility elements, HTML/canvas plugins, native DOM listeners and synchronous screenshots remain
on `create_chart`.

`tests/offscreen-worker.spec.mjs` passes with WebGPU and forced Canvas2D, including typed data,
input, multi-pointer ownership, resize and telemetry. A deterministic simulated device-loss case
proves the worker reports `webgpu` → `canvas2d` and the owner changes the two HTML canvas
visibilities to `["hidden", "visible"]`. In repeated independence gates, the main thread was blocked
for **550 ms** while the worker continued to present **32–35 frames**. That continued presentation,
not a single last-frame `cpu_ms` sample, is the worker-independence criterion. The separate
continuous-crosshair acceptance above is the 8 ms p99 frame-cost gate.

---

## Test suite state

Final validation on this source tree:

- **Rust:** `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  wasm-target Clippy with `-D warnings`, and `cargo test --workspace` all pass. The wasm lint exposed
  one explicit `drop` of a non-`Drop` closure in the new axis builder; its borrow is now ended by a
  lexical scope and both lint targets are clean.
- **Package:** package and lock metadata parse at `0.8.11`; typecheck, oxlint, release build and pack
  smoke pass. The packed artifact contains 17 files and a 903 kB wasm binary.
- **Chromium default suite:** **117 passed, 4 failed, 1 skipped** across 122 tests. The only failures
  are the four established environment-baseline cases: three cross-library `backend-parity`
  reference-fidelity ceilings and the `prim-text` fixture precondition (`fixture must have series
  pixels under the pink text for the probe`). No new behavioral failure remains.
- **Explicit acceptance subset:** all **24/24** frame-statistics, continuous-crosshair, worker,
  ring-source and typed-allocation tests pass. Items 2–5 report the observed ranges from repeated
  acceptance runs rather than selecting one run as canonical.
- **Native strict perf gate:** 10 × 50k-bar `build_frame` **0.68 ms** against 16.67 ms; 1M-bar
  `set_series_data` **71.54 ms** against 300 ms. Both pass with `ORIGIN_PERF_STRICT=1`.
- **Opt-in wasm/browser benchmark:** passes on WebGPU: 1M-bar install **162.565 ms**, autoscale
  **1.455 ms**, 50k frame build **0.640 ms**, 10k-drawing hit test **1,257.05 µs/probe**, and
  308,609,024 bytes wasm memory.

The shared-axis migration also exposed four stale full-frame byte-identity assertions in plugin and
primitive tests. They now keep exact pane-geometry checks and bound only the documented
fractional-DPR axis AA residual (<=5,000 pixels, <=64 channel delta). The plugin/engine marker check
measured 23 clipped-edge pixels at one channel step and **zero** ordering pixels.

Two things worth knowing about the browser specs:

- **The retention plateau spec takes ~6 minutes**, simulating 32 hours of one-bar-per-second streaming.
  That is the only way to distinguish a plateau from linear growth, so it earns its runtime.
- **`engine-bench.spec.mjs` is opt-in behind `ORIGIN_BENCH=1`.** It grows wasm linear memory to
  roughly 300 MB, so the default suite skips it to avoid starving the following page initialization.

## Non-goals

None of the following were undertaken: no change premised on sub-millisecond wire latency, no
replacement of the Canvas2D fallback or of any backend-neutral plugin guarantee, no breaking API
change or rename, no move away from snake_case, and no transport, networking or data-fetching layer
bundled into the engine.
