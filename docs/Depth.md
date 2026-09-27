# Depth, liquidity, and tape views

## Ownership

A depth stream is a bounded chart-side projection of a host publication. The host owns provider
sessions, market-by-order assembly, recovery, and the authoritative market book. Aeris Charts owns
only the validated state needed to render and query one chart consistently. Reusing a host key
returns the existing stream only when its options match; dependents hold the stream identity and do
not copy the book.

The same rule applies to time and sales: it is a newest-first filtered view of the existing shared
classified trade tape. It does not retain a second trade list or classify events again at query
time.

## Book contract

A full snapshot supplies a signed microsecond timestamp, exact `u64` sequence, bid and ask levels,
and optional order counts. Prices must be finite and exactly aligned to `tick_size`; sizes and
counts are validated together; duplicate levels and crossed or locked snapshots are rejected
atomically. Incremental rows additionally carry their previous sequence. A mismatch returns a
typed `DepthResyncRequest`, marks the stream stale, and rejects every later delta until a valid new
snapshot replaces the state. The engine never fills a missing sequence.

Each side, batch, history bucket ring, total retained history cells, event list, replay tape,
checkpoint set, heatmap count, and event layer has an explicit cap. History rolls at the configured
microsecond interval and evicts the oldest buckets until both the bucket and cell ceilings hold.
Memory telemetry includes current levels, history vectors, event labels, replay state, checkpoint
snapshots, and retained heatmap pixels.

Queries read one canonical view:

- best bid/ask and size at an exact price;
- cumulative depth and imbalance over N levels;
- a touch-centered ladder with minimum-size and maximum-distance filters;
- filtered cumulative bid/ask curves derived from the ladder;
- newest-first time and sales with minimum-volume, side, and row-count filters.

## Rendering

The heatmap maps the configured fixed price extent into RGBA rows. Minimum and maximum size define
the intensity scale before upload; bid/ask colors and opacity remain engine options. Finalized
history is packed into absolute 32-bucket chunks with stable image keys. Only the incomplete edge
chunk changes when a bucket closes, and every live update replaces one one-pixel-wide image. A
512-bucket view therefore emits at most 17 finalized images plus its live edge instead of one draw
per cell or bucket. Ordinary series paint after the underlay, so host trade series overlay the
liquidity image without a renderer-specific path.

Microstructure markers are host facts. Iceberg refill, pulled liquidity, size cluster, and sweep
events carry time, price, size, optional side, and a label capped at 256 bytes. Charts validates,
retains, replay-masks, LOD-collapses, and renders them through an explicitly pane-bound event layer;
it does not detect them. DOM and time-and-sales list presentation remains host UI.

All executors consume the same `Prim::Image` and marker primitives. Canvas2D, WebGPU, native, and
GPUI do not reconstruct book state, color scaling, LOD, or replay semantics.

## Browser boundary

Snapshots and updates cross WASM as parallel typed arrays. Timestamps must be exact JavaScript-safe
integer microseconds. Provider sequences use high/low `Uint32Array` words so the boundary never
rounds a `u64`; returned sequences and trade IDs are decimal strings. Optional order-count arrays
use `0xffffffff` for unavailable values. Event numeric fields remain columnar, with an optional
aligned JSON label vector because JavaScript has no typed string array.

## Replay and recovery

The host replay clock never deletes future source truth. Depth queries switch to a projection built
from the newest eligible snapshot/checkpoint and apply only events at or before the clock. A seek
backward restores the nearest checkpoint, recorded every 1,024 events and capped at 64, then reports
the suffix work. Heatmap finalized buckets and the active edge use the same cutoff; event layers omit
future markers. Clearing the clock immediately returns every query and frame to the canonical live
view.

## Verification evidence

Deterministic engine fixtures cover snapshot validation, negative and fixed-grid prices, optional
counts, atomic batches, gaps and resync fencing, bounded history, ladder/study filters, event caps
and LOD, backward checkpoint seeks, future ingest during replay, stable heatmap chunk keys, and
live-edge replacement. Browser fixtures cover Canvas2D and WebGPU typed ingest, gap recovery,
replay masking, heatmap/marker rendering, exact sequence strings, and classified time-and-sales.

The 2026-09-27 release `perf_gate` Target L ran two 1.2-million-update passes. Its worst 100,000-row
batch was 10.05 ms, heatmap frame construction was 0.34 ms, the dense 512-bucket view used 17 image
primitives, live-edge upload was 512 bytes, and retained depth memory stayed flat at 66.03 MiB. The
GPUI scene-construction gate rendered the same dense 512-bucket/128-row heatmap as 17 image runs at
0.053 ms p99 against the shared 2 ms budget. These are observed measurements on the milestone
machine, not claims about device-present time.

The WebGPU focused fixture produced and inspected a transient chart screenshot with the heatmap and
host marker visible. The accessibility review uses the existing unified chart contract: one bounded
application surface, canvas pixels hidden from the accessibility tree, silent streaming updates,
and keyboard interaction owned by the host surface. Depth adds no focusable DOM, announcements, or
parallel interaction model. Binary screenshots remain transient; their deterministic setup is kept
in `examples/web_demo/tests/depth.spec.mjs`.
