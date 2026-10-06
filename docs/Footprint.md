# Footprint / Numbers Bars Design

This document is the durable design contract for GitHub issue #23. It describes the tick-truth
model that Aeris Charts uses for professional footprint series. `Architecture.md` remains the
authority for crate ownership and dependency direction.

## 1. Trade event and ordering model

A footprint series ingests trades, never synthetic OHLC clusters. Each event carries:

- a signed Unix timestamp at microsecond resolution;
- tick-aligned price and positive volume;
- host-provided aggressor side (`buy`, `sell`, or `unknown`);
- optional contemporaneous bid and ask;
- optional provider sequence and stable trade ID;
- opaque condition bits for later microstructure extensions; and
- an optional host-defined session ID.

Microseconds preserve exchange ordering while remaining exactly representable for contemporary
dates in JavaScript numbers. The browser boundary rejects unsafe integers. Equal timestamps sort by
provider sequence, then stable input order. A trade ID makes replay idempotent and permits a provider
correction to replace the prior event. A batch containing the same trade ID twice is rejected
atomically rather than choosing an undocumented winner.

The retained trade tape is authoritative. Derived bars may be discarded and rebuilt. Tip events use
an incremental path. A late event or correction is inserted in canonical order; the shipped path
rebuilds the complete retained series once and reports that work. It never patches final bar totals
while leaving the Max/Min Delta path stale. A measured suffix-checkpoint path may replace that full
reconstruction later without changing the public result.

Prices must lie on the configured integer tick grid. Aeris rejects off-grid data rather than
rounding financial truth silently. Volume is finite and positive. A session-ID change closes the
current bar and resets session cumulative delta.

## 2. Aggressor-side classification

Classification is deterministic and ordered by authority:

1. A host-provided `buy` or `sell` side is accepted as authoritative.
2. For `unknown`, a trade at or above the supplied ask is a buy; at or below the supplied bid it is
   a sell.
3. Otherwise the tick rule compares with the previous canonical trade: higher is buy, lower is
   sell, and an unchanged price carries the previous classified side.
4. If none of those rules resolves the event, it remains unknown.

Unknown volume contributes to price-level, bar, and POC total volume, but not bid volume, ask volume,
delta, imbalance, or cumulative delta. Aeris never guesses a side merely to make a cluster look
complete. Historical reconstruction reruns classification in canonical event order, so inserting a
late event can correctly change the classification of later ambiguous events.

## 3. Aggregation and accounting

The aggregation model supports aligned time bars, fixed trade-count bars, whole-trade volume bars,
and tick-grid range bars. A trade is never split to hit an exact volume or range threshold. A new session always starts a new
bar. Time bars retain their whole-second display projection; trade-count, volume, and range bars
are chart-integrated through a chart-local logical row key and an engine-owned sequence sidecar.
That sidecar carries each bar's full-resolution open/close microsecond bounds for labels, crosshair,
and visible-range lookup, so several bars in one second are never assigned false timestamps.
`BarSequenceMapping` provides deterministic anchor rebasing when a sequence is prepended or rebuilt.
The chart retires the sidecar when its last non-time footprint and dependent are removed, so a
later time-only series cannot inherit stale logical labels.
Non-time tip updates replace only the affected suffix and keep derived delta studies on the same
logical row keys; capped series use the full path when retention can shift the prefix. Big-trades
bubbles also use logical bar indices while their order grouping windows retain microsecond precision.
Chart value snapshots and series queries expose the corresponding UTC-second label instead of the
internal row key.
Incremental replay and release performance evidence remain part of the B5 performance exit.

Each bar retains OHLC, bid/ask/unknown/total volume, trade count, final delta, delta percentage,
session cumulative delta, and sorted price levels. Each level retains bid, ask, unknown, total, and delta. Trades are
validated against the instrument `tick_size`; canonical level identity is
`round(price / tick_size)`, avoiding repeated floating-point price comparisons.
`ticks_per_row` (default 1) groups adjacent levels only for presentation; a displayed row's
price is the lowest tick of its group, while canonical levels and bar OHLC keep exact tick prices.

Running bar delta begins at zero. Each classified buy adds volume and each classified sell subtracts
volume. Max Delta is the highest running value observed after each event (including initial zero);
Min Delta is the lowest. They are retained during live aggregation and recomputed from the tape after
historical mutation. Final delta is not used to approximate either extreme.

POC is the level with maximum total volume. Ties select the level nearest the bar close, then the
lower level. This rule is stable across live and historical construction.

The professional diagonal imbalance rule compares:

- ask at level `p` against bid at `p - 1 tick`; and
- bid at level `p` against ask at `p + 1 tick`.

The dominant side must meet the configured minimum volume and the configured ratio. Zero opposite
volume qualifies once the minimum is met. Missing intermediate levels break a stack. Every member of
an adjacent run at least `consecutive_levels` long is marked as a stacked bid or ask imbalance.
Horizontal and alternative imbalance modes are visual projections only; they cannot alter the stored
bid/ask truth.

The visual aggregation mode is independent of bar construction: hosts can request Bid × Ask, Total,
Delta, profile-in-bar, volume-ladder, horizontal-imbalance, or bid/ask-histogram cells from the same
data without rebuilding the tape.

### Shared chart tape and derived studies

`ChartEngine::add_trade_stream` creates one bounded stream keyed by a host instrument identity.
The stream owns classification, canonical ordering, corrections, retention, and a monotonic revision;
footprints, CVD, delta histograms, and big-trades indicators bind to that identity rather than
retaining a second provider-event tape. CVD supports session, continuous, and anchored resets. Delta
dependents read final delta, Max/Min Delta, delta percentage, and bid/ask/unknown volumes from the
same bars.

Big trades (`ChartEngine::add_big_trades`, `big_trades.rs`) is a standalone indicator over any
candlestick, bar, line, area, baseline, or footprint series. An exchange reports one aggressive order
as several prints when it fills against several resting orders, so the indicator first rebuilds
orders: consecutive prints with the same classified aggressor and session, each within the grouping
window (default 1 ms) of the previous print, whose price never moves against the aggressor. Prints
the stream cannot classify never start an order. The filter then runs on the rebuilt orders, so a
large order filled as many small prints still qualifies. The automatic filter keeps orders strictly
above the weak (95th), medium (98th), or strong (99.5th) percentile of the last 4096 completed
orders, refreshed every 128 orders; the fixed filter keeps orders of at least a minimum volume.
Tip appends continue the still-open order incrementally; corrections and replay seeks replay from
the retained tape's sealed-history start state, preserving bubbles for released trades. Front
eviction rebases or drops old bubbles. Changing grouping or filtering rebuilds from the available
raw tape, so bubbles from sealed trades do not survive that options change. The newest 4096
qualifying orders are retained.

Each order draws one translucent circle with a side-colored outline at its volume-weighted price,
area proportional to volume relative to the largest retained order, largest first so smaller orders
stay on top. A sweep across several prices also draws a thin range line, and bubbles large enough
to hold it show the compact order volume (`250`, `1.2k`). Bubbles are pane chrome above every
series. Stream telemetry counts big-trades indicators as dependents.

### Auction markers

`add_auction_markers` binds a runtime-only marker set to the same canonical trade stream and a
price series, with at most 16 marker sets per chart. Marks use ascending **per-tick canonical
aggregator levels**, not visually merged display cells: changing `ticks_per_row` or imbalance
rules in place leaves them unchanged. Each rule emits at most one mark per side and kind per
bar. The last, still-forming bar is excluded unless `include_forming_bar` is true (default false).
Replay shows only bars available at its clock; late prints repair the affected suffix and retention
removes marks with their evicted bars. For late prints and rewritten host tape windows, time-bar
repair starts at `partition_point(start_timestamp_micros <= earliest_changed).saturating_sub(1)`;
non-time (trade-count, volume, range) repair starts at
`partition_point(start_timestamp_micros < earliest_changed).saturating_sub(1)`.
The strict comparison includes the bar before *all* equal-timestamp starts, which may itself
contain a changed print; marks depend only on their own bar, so the suffix needs no earlier
neighbour. Both rules clamp to bar zero and stay suffix-bounded even when the window shrinks.
Prepending history and front eviction rebase all marks; sealing raw trades leaves derived
bars and marks intact.
The snapshot reports bar time, kind, high/low side, level
price and qualifying volume. Marks are not clickable.

- **Unfinished auction**: the high (last level) or low (first level) has both bid and ask volumes
  strictly positive and each at least `min_side_volume` (default 0). When
  `extend_until_revisited` is enabled (default false), a dashed ray reaches the first later bar
  whose exact high/low range includes the level, or continues to the visible right edge.
- **Exhaustion**: the high ask or low bid is positive, no greater than
  `exhaustion_max_volume` (default 10), and strictly decreases toward the extreme over
  `exhaustion_levels` canonical levels (default 3, allowed 2–8). Equal adjacent volumes fail.
- **Absorption**: within `extreme_levels` levels of the low/high (default 2), bid/ask volume
  respectively is at least `absorption_min_volume` (default 100) and at least
  `absorption_ratio` times opposite volume (default 3). The close must be at least
  `min_rejection_rows` canonical levels beyond the candidate toward the opposite side (default
  1). The greatest qualifying aggressor volume wins, then the level nearest its extreme.

The snapshot's `volume` is always the volume of the bar level the mark
flags: an unfinished auction reports the sum of bid and ask volume at the unfilled extreme
level; an exhaustion reports the aggressor-side volume of the exhausted extreme level (ask
volume at the high, bid volume at the low); an absorption reports the aggressor-side volume of
the winning level (bid volume at the low, ask volume at the high).

Volumes and ratio must be finite and nonnegative; `extreme_levels` is 1–8 and
`min_rejection_rows` is 0–8. `visible` defaults true and only affects painting. Unfinished
auctions paint triangles, exhaustion circles, absorption framed squares with `ABS` labels when
bar spacing is at least 6 CSS px, and extensions dashed lines. All lower through the common frame
primitives on the bound series' price scale.

## 4. Rendering and LOD

Footprint geometry is constructed in `aeris_charts_engine` as ordinary backend-neutral primitives.
Backends preserve the resulting order, clipping, alpha, text alignment, and pixel coordinates; no
executor recalculates POC, delta, or imbalance.

At readable density, each visible bar paints:

1. optional delta-tinted bar background;
2. per-level bid and ask cells (or the selected Total/Delta layout);
3. POC emphasis;
4. bid/ask imbalance and stacked-imbalance emphasis;
5. aligned volume text; and
6. a bounded two-line bar summary containing final, Max, and Min Delta plus total, bid, and ask
   volume.

Text uses the chart's resolved font family and centered columns. Colors are resolved before frame
execution, and the default text color follows live chart-theme changes. Max/Min Delta are visible in
the summary and queryable even when LOD hides text.

LOD is selected from horizontal bar width and vertical tick-row height:

- **Detailed:** bid/ask (or selected mode) text, cells, POC, imbalance, and summary.
  The two-line summary is omitted while the bar is too narrow to fit it without
  overprinting neighboring bars (`bar_spacing < 9 × font_size`).
- **Cells:** colored level cells and POC/stacked emphasis, without glyphs.
- **Summary:** one delta/volume body plus POC marker per bar.
Frame work is bounded to the visible logical range. At the densest zoom, Summary mode is already one
body and one POC marker per visible bar; hidden text does not enter the draw list or text atlas.
Every backend therefore receives exactly the same chosen LOD.

## 5. Storage, invalidation, and recovery

One chart-level stream owns one canonical trade tape and one derived bar vector. A footprint series
owns only visual options and a stream handle; CVD, delta, and big-trades dependents own no provider
tape.
A bar owns sorted per-tick price levels; display grouping is applied only when presenting a bar.
There is no renderer-side cluster cache. Tip append mutates only the active bar or appends one bar,
updates its canonical scale projection, and invalidates that series. Closed bars are immutable on the
live path. Historical insertion/correction reconstructs canonical state once after the final tape is
known and replaces the affected projection once, rebuilding from the newest eligible retained tape
checkpoint. A rewritten bounded window replaces prints at or after its earliest timestamp, not
earlier sealed or raw history. Empty windows change nothing.

Vectors and the trade-ID index reuse their allocated capacity. Raw-tape retention seals complete
old bars and releases their trades while retaining their levels, session accounting and big-trades
bubbles. The five-session and stream-byte budgets eventually evict the oldest sealed bars and their
dependent data together. Backfill can prepend older pages until history has been evicted, after
which earlier pages are refused. Engine memory telemetry includes tape, levels, the ID index,
sealed history and retained capacities.

Device loss is irrelevant to this model: the headless tape and derived bars remain intact while the
browser executor falls back. Renderer caches are rebuilt from the same frame contract.

## 6. API surface

The Rust engine owns typed commands to create/configure a footprint series, atomically replace a
trade tape, append/correct one trade, ingest an ordered batch, query a bar and price level, and read
work/memory statistics. The WebAssembly boundary accepts typed columns for historical and live batch
ingest; object conversion is reserved for the low-frequency single-event API.

The TypeScript package exposes a dedicated `footprint_series_api` from
`chart.add_series("footprint", options)`. Its input is `footprint_trade`, not `series_data`; generic
OHLC `set_data` is rejected for this kind. Queries expose bar OHLC, price levels, POC, final/Max/Min
Delta, delta percentage, bid/ask/unknown/total volume, session delta, and stacked flags. Chart-level
methods create CVD and delta dependents, `add_big_trades` returns a handle for options, snapshot, and
removal, and stream methods query revision/telemetry. Data-change notifications use
`full` for replacement/historical reconstruction and `update` for a true tip update.

Options cover tick size, ticks per row, time-bar interval/anchor or trade-count/volume/range construction, imbalance
ratio/minimum/consecutive count, visual cell modes, colors, text size, summaries, and generic series
retention. Reconfiguring an order-flow presentation in place changes row size, imbalance and
display settings without replaying the tape; a different tick size or bar aggregation requires a
new stream. Visual-only options invalidate only the series frame layer.

Host callbacks receive derived snapshots only through ordinary chart query/event paths. Aeris Terminal
and other hosts remain authoritative for feed subscription, exchange calendars, and choosing
session IDs; none of those concerns enter the renderer.

## 7. Verification and performance evidence

### Reference fixture and release baseline

The dense order-flow reference view is deterministic: 20 one-minute bars, 11 price levels per
bar, a 0.25 tick size, and paired buy/sell prints at every level. It is used by the GPUI
`plan_bench` fixture at 1600×900 CSS pixels, DPR 1.5, and 72 px bar spacing, where detailed LOD
must emit the cell text runs. The WebGPU `perf_gate` Target J consumes the same frame contract
with a resolved atlas quad for every text primitive and guards a 2 ms p99 CPU-side encoding
budget; this measures scheduling and upload preparation, not device present time.

The sustained native release baseline remains Target D: 2,500 retained one-minute bars with 100
trades per bar, a 100-bar live batch, and a 10-bar correction batch. Its budgets are 300 ms for
historical load, 50 ms for the live batch, 300 ms for correction, and 16.67 ms for frame
construction, with retention bounded to the configured history. Commands and thresholds are kept
in the release examples so a clean `--release` run can be compared without importing machine-
specific timings into the repository.

The finite GPUI real-window probe was also exercised on the current Windows display with the
footprint fixture: 30 frames at DPR 1.25 and 500 source bars produced 24 cached text runs (zero
misses), with adapter p50/p99 of 2.005/4.508 ms and GPUI paint p50/p95/p99 of 2.005/2.369/4.341 ms.
These numbers are an observed host run, not a portable release budget; the probe does not expose
native WebGPU device-present timing.

The 2026-09-26 release gate measured the same dense fixture through the native WebGPU CPU-side
encoding path: 120 resolved text primitives produced 120 atlas instances, with a 0.00 ms p99
encoding sample against the 2.00 ms Target J budget. The Chromium footprint suite also passed all
six cases, including the WebGPU shared-frame path. These checks cover executor scheduling and
browser integration; native device-present timing and the GPUI display-specific numbers above
remain diagnostic rather than portable budgets.

Order-flow milestone evidence was captured on 2026-09-26. The GPUI pane capture used
`AERIS_CHARTS_GPUI_FEATURE=footprint` at DPR 1.25 and produced a 1543×873 image for the dense
12-bar fixture; the image was visually inspected for readable cell text, stable column alignment,
and unclipped pane content. The browser accessibility review passed the focused
`unified-interaction-accessibility.spec.mjs` contract: one bounded application surface, hidden
canvas pixels, a silent live region during streaming, and keyboard drawing edits that roll back.
The capture image remains a transient milestone artifact; the command and metadata are recorded
here so the evidence can be reproduced without adding binary fixtures to the repository.

Deterministic synthetic tapes cover grid boundaries, unknown-side handling, quote/tick-rule
classification, equal timestamps and sequences, late events, corrections, session resets, all bar
modes, bid/ask/total/delta levels, POC ties, mean-reverting Max/Min Delta paths, both imbalance sides,
and stacked-run breaks. Frame tests cover every shipped LOD and primitive ordering. Browser tests
exercise the same engine-built frame through Canvas2D and WebGPU as well as typed ingest and public
queries; GPUI and native consume those existing backend-neutral primitive kinds without footprint
math or footprint-specific executor branches.

The release evidence fixture measures a large historical tape plus sustained live append. It records
aggregation/rebuild work, frame CPU, draw count, upload bytes, retained memory, and allocations.
Acceptance requires tip updates not to rebuild prior bars, stable memory under configured retention,
and bounded visible-frame work as history grows.
