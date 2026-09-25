# Aeris Charts Trading Expansion Plan

## Decision and scope

Aeris Charts will become a complete **headless** professional trading chart engine: order flow,
market depth, non-time bars, a professional indicator catalog, and a drawing system whose every
tool is as configurable as the tools in mature trading platforms. The primary consumer is the
Aeris Terminal GPUI platform; browser hosts consume the same engine through WASM.

Headless means Aeris owns semantics and pixels inside the chart, never application chrome:

| Aeris owns | Hosts own |
| --- | --- |
| Validated data models (trades, depth, bars), aggregation, classification and derived studies | Market-data subscriptions, provider normalization, reconnection and resync requests |
| Indicator and order-flow math, incremental updates, bounded caches | Symbol search, watchlists, exchange calendars and session definitions |
| Drawing geometry, placement, handles, hit testing, snapping, text layout, undo/redo | Toolbars, settings dialogs, property panels, color pickers, context menus |
| Typed option schemas, defaults, validation and style templates as data | Where templates are stored and how users pick them |
| Persistence schemas and migrations for everything above | Account/cloud storage, cross-device sync, sharing |
| Ordered backend-neutral frame output for every executor | Window, DOM or GPUI layout around the chart |

A feature is not delivered until a host can build its complete UI from typed engine APIs without
reimplementing chart math, and every executor (GPUI, WebGPU, Canvas2D, native) renders it from the
same ordered frame. [Architecture.md](../docs/Architecture.md) remains the authority for current ownership;
[plan.md](plan.md) covers general (non-financial) chart families. Both plans share one
`ChartEngine` and one frame contract and proceed independently. As of 2026-09-25 this plan is the
active program; plan.md is paused after its R3 range-bar batch.

Out of scope: a Pine-style scripting language, a bundled UI kit, broker connectivity, datafeed
adapters, and news/fundamental data. Custom studies are covered by a typed extension API (I4)
rather than an interpreter.

## Current baseline

Source-confirmed on 2026-09-24. This is the starting point, not a claim of completeness.

| Area | Present today | Evidence |
| --- | --- | --- |
| Footprint | Trade tape per series; aggressor classification (host side → quote → tick rule); bid/ask/unknown/total per level; Bid×Ask, Total and Delta cell modes; POC; diagonal and stacked imbalances; final/max/min delta; session cumulative delta per bar; three LODs; late-event and correction rebuild | `engine/src/footprint.rs`, `frame/footprint_geometry.rs`, [Footprint.md](../docs/Footprint.md) |
| Footprint bar policies | Time, trade-count and volume aggregation in Rust; only whole-second time bars are chart-integrated | `FootprintBarAggregation`, Footprint.md §3 |
| Volume profile | Visible-range profile computed from OHLCV candles; rows, value area, POC; at most 16 per chart; runtime-only | `engine/src/volume_profile.rs`, `indicators/src/volume_profile.rs` |
| Series types | Candlestick, bar, line, area, histogram, baseline, custom, feature (grouped/stacked bars, heatmap, HLC area, pretty histogram, background shade, stacked area, whisker box), footprint | `SeriesKind`, `FeatureSeriesKind` |
| Indicators | SMA, EMA, EMA ribbon, WMA, Bollinger, RSI, MACD, Stochastic, ATR, VWAP; incremental state; outputs are ordinary series, so indicator-on-indicator chaining already works | `engine/src/indicators.rs`, `indicators/src/lib.rs` |
| Indicator input | `IndicatorInput` carries times, high, low, close and volume only; no open, no selectable price source (hl2, hlc3, ohlc4) | `IndicatorInput` |
| Drawing tools | Trend line, horizontal line, horizontal ray, vertical line, rectangle, text, brush, path, long position, short position; static tool catalog; magnet; straighten; bounded undo/redo | `drawings/tools.rs`, `drawings.rs` |
| Drawing styling | One flat `Drawing` struct for every kind. Stroke color/width/style shared. Fill, border, axis labels and bands are rectangle-only. Text alignment and `+ Add text` exist only on the trend line; box background/border only on the text tool | `Drawing`, `frame/drawings.rs` |
| Drawing management | Selection, drag, undo/redo. No lock, hide, z-order, grouping, naming, per-interval visibility, templates or multi-select | `drawings.rs` |
| Trading and alerts | Positions, orders, brackets/OCO, drag intents, bracket from position drawing; alert lines (host evaluates) | `trading.rs`, `alerts.rs` |
| Workspace | Split-grid of chart cells with stable identities | `workspace.rs` |
| Persistence | V1 panes and built-in drawings; V2 adds general datasets/series; indicators and profiles are recreated by hosts | `persistence.rs` |
| Order book / depth | **Absent.** No Level 2 model, DOM, or liquidity heatmap. The feature heatmap accepts only host-precomputed cells and lowers each cell to its own rectangle primitive | `FeatureSeriesKind::Heatmap`, `HeatmapCell` |
| Non-time bars | **Absent on chart.** The shared time axis has one logical row per UTC second | Footprint.md §3 |
| Markers and executions | Series markers (circle, square, arrow up/down with optional text, size and price); point markers on line/area; trading executions drawn as B/S circles | `Marker`, `set_series_markers`, `frame/series_geometry.rs`, `TradingExecution`, `frame/trading_geometry.rs` |
| Line and scale variants | Stepped lines (`LineType::WithSteps`); hollow candles through transparent body colors; percentage and indexed-to-100 price scales; price lines with host titles | `draw_list.rs`, `price_scale_core.rs`, `PriceLine` |
| Native primitives | Series-attached vertical line, text and image watermarks, volume-profile handle | `native_primitives.rs` |
| General charts | Step interpolation, bubble, heatmap grid, column, axis-bound reference regions (general panes only, not the financial time axis) | `general_series.rs` |
| Telemetry | WASM `frame_stats` (CPU/GPU ms, draw calls, rebuild counters, buffer traffic); `ChartEngine::memory_usage` structural attribution | `wasm/src/telemetry.rs`, `EngineMemoryUsage` |
| Image export | Browser `take_screenshot` only; no native or GPUI image export | `packages/charts/src/types.ts` |
| Trading labels | Order and position chips are engine-formatted (quantity, kind, PnL); no host-supplied label or badge text | `trading_geometry.rs` |
| Cross-chart sync | **Absent.** Workspace cells never share crosshair, visible range or state | `workspace.rs` |
| Replay | **Absent.** No playback cursor or future masking; hosts can only replace and append data | — |

## Architecture principles for this expansion

- **One source of truth per market fact.** A trade tape or depth book is stored once per
  instrument stream and shared by every study and series derived from it. No study copies raw
  trades.
- **Derived state is disposable.** Bars, profiles, delta series and heatmap buckets can always be
  rebuilt from the retained source. Tip updates use incremental paths; historical corrections
  rebuild from a documented checkpoint and report the work done.
- **Bounded everything.** Every tape, ring, profile, level list, cache and drawing collection has an
  explicit cap, eviction rule and memory telemetry. Frame work is bounded by what is visible.
- **Typed, not stringly.** Each study and drawing kind has a typed option struct with validation and
  defaults. Hosts receive typed schemas (name, type, range, default) to generate their property
  panels; the engine never renders dialogs.
- **Shared geometry, not per-backend features.** New shapes (ellipses, arcs, arrows, gradient-free
  level fills, bubbles) are lowered to existing or new backend-neutral primitives implemented by
  every executor before a feature is complete.
- **No invented data.** Nothing guesses aggressor sides, fabricates timestamps for non-time bars, or
  rounds off-grid prices. Unknowns stay unknown and are reported.
- Follow [AGENTS.md](../AGENTS.md): no speculative crates, traits, plugin registries or feature flags.
  Extract modules only when a real responsibility justifies it.

## Foundations

These unblock most of the catalog. Each foundation ships with a public API, tests through the real
host paths, and an Architecture.md update.

### F1 — Logical bar identity for non-time bars

**Problem.** The time axis maps one logical row to one UTC second. Tick, volume, range, Renko,
Kagi, Point & Figure and Line Break bars can produce several bars within one second, or bars
whose position is not a function of time at all. Footprint.md already forbids faking timestamps.

**Required outcome.**

- A bar-sequence domain where each logical index is a bar with its own open and close time
  (microseconds), independent of whole-second alignment.
- Time labels, crosshair time, tick marks and gaps derive from bar open/close times, including
  many bars in one second and long gaps between bars.
- Drawings, alerts, trading lines and markers anchored to a non-time chart store both the logical
  bar and the time, and rebase deterministically when history is prepended or rebuilt.
- Define which series can share a pane with a non-time primary series. Recommended rule: series in a
  non-time pane must be derived from the same bar sequence (studies, footprint, delta), not
  independent time series. Mixing arbitrary time series requires an explicit mapping and is not
  silently aligned.
- Bar construction for tick, volume and range bars moves into one shared aggregator used by
  candles and footprint alike; footprint's existing trade-count and volume policies become
  chart-integrated.

**Exit.** Tick/volume/range candles and footprint render on every executor; many-bars-per-second
and gap fixtures pass; drawings survive history prepend and rebuild; performance gate shows tip
append without rebuilding closed bars.

### F2 — Shared trade tape and tape-derived studies

**Problem.** The footprint series owns its tape. CVD, delta histograms, trade bubbles, tape-based
volume profiles and VWAP-from-trades must read the same classified trades without duplicating them.

**Required outcome.**

- A chart-level trade stream handle, keyed by host-defined instrument stream, retaining the
  canonical classified tape with the existing ordering, correction and retention rules.
- Footprint, candles built from trades (F1), and every order-flow study bind to the stream by
  identity. Removing the stream removes or invalidates dependents explicitly.
- Classification runs once per event; dependents receive the classified event and a revision.
- Per-dependent incremental state plus a documented rebuild path on historical mutation.
- Memory telemetry per stream and per dependent.

**Exit.** A footprint series and a CVD study share one tape (verified by memory telemetry), a late
trade updates both consistently, and retention evicts tape and derived state together.

### F3 — Order-book (Level 2) depth model

**Problem.** A liquidity heatmap and DOM ladder need historical and live resting liquidity per
price level. No such model exists; the current heatmap series only accepts precomputed cells.

**Required outcome.**

- Ingest a full book snapshot and incremental level updates (price, side, size, sequence,
  timestamp). Prices validated against the tick grid.
- Sequence-gap detection that marks the book stale and emits a typed resync request to the host;
  the engine never invents missing levels.
- Live book state (bounded by a configurable number of levels around the touch) plus a
  time-bucketed history ring for heatmap rendering, with explicit bucket interval and caps.
- Queries: best bid/ask, size at price, cumulative depth, book imbalance over N levels.
- Typed columnar ingest at the WASM boundary; no per-update object conversion on the hot path.

**Exit.** Deterministic replay of a recorded snapshot-plus-update stream yields identical book
states; gap fixtures request resync; memory stays flat under a sustained update soak.

### F4 — Study input model

**Problem.** Indicators only receive high, low, close and volume, and always use close as the price
source. Professional studies need open, selectable sources and multi-input bindings.

**Required outcome.**

- `IndicatorInput` gains open; bindings select a source: open, high, low, close, hl2, hlc3, ohlc4,
  hlcc4, or any existing indicator output (chaining already works through series identity).
- Studies may bind several inputs (price series, volume series, trade stream, depth book) with
  typed validation.
- Every study exposes a typed parameter schema and typed output descriptors (name, kind: line,
  histogram, band fill, markers, levels) so hosts generate settings UI and legends without
  hard-coded knowledge.
- Per-output style (color, width, line style, visibility, histogram colors, band fill) remains
  engine state and persists.
- Study bindings join the persistence schema (next version) instead of being host-recreated.

**Exit.** An RSI of hlc3, an SMA of that RSI, and a Bollinger band fill round-trip through
persistence and render identically on every executor.

### F5 — Drawing model and customization contract

**Problem.** One flat struct serves every tool; fill, border, labels and bands are
rectangle-specific; text layout is hard-coded to the trend line and text tool. This cannot scale to
~80 tools with per-tool customization.

**Required outcome.** A drawing becomes a common core plus a typed per-kind option block. The
common contract applies to **every** tool unless a property is meaningless for its geometry, and
that exception is recorded in the tool's catalog entry.

Common properties:

| Group | Properties |
| --- | --- |
| Identity | Stable ID, user name, optional group ID, creation/modification revision |
| State | Visible, locked (no drag/edit, still selectable), z-order within the drawing layer (bring forward/backward/front/back) |
| Interval visibility | Show on selected interval ranges (seconds, minutes, hours, days, weeks, months, ticks, ranges), using host-supplied interval metadata |
| Stroke | Color with alpha, width, line style (solid, dotted, dashed, large dashed, sparse dotted), line cap/end style per end (none, arrow, circle) where the geometry has ends |
| Extension | Extend left/right for line-like tools; extend levels for level tools |
| Fill | Background enabled, color with alpha; per-zone fills for multi-zone tools |
| Text | Content (multi-line), font size, bold, italic, color, horizontal alignment, vertical alignment, placement relative to the geometry (above/below/on line, inside/outside shape), optional background box with border color/width and padding, wrap width |
| Labels and stats | Per tool: price, price change, percent change, ticks/pips, bar count, date/time range, duration, angle, distance, volume in range; each individually toggleable with label position |
| Coordinates | Numeric read/write of every anchor (time or bar plus price) through the typed API, so hosts can offer coordinate editors |
| Scale binding | Price scale (left/right/overlay), pane, magnet mode (off, weak, strong) |

Level-based tools (Fibonacci, Gann, pitchfork, position) add a level list: value, color, visible,
line style, fill-between-levels toggle and color, plus tool options such as reverse, log-scale
levels, show prices, show level values or percents, and label alignment.

Engine responsibilities around the contract:

- Text layout for every tool uses one shared layout path (measurement hook, alignment, placement,
  box, clipping) so hit boxes, editing caret anchors and rendering never disagree. The existing
  trend-line text behavior becomes one instance of it.
- Hosts receive a typed property schema per tool and read/write properties through
  `drawing_apply_options`-style atomic patches; invalid patches leave the drawing unchanged.
- Style templates are data: per-tool default overrides and named templates that the engine
  validates, applies at creation, and exports/imports. The host decides where templates live.
- Multi-select, clone, copy/paste as a serialized drawing payload, group move/lock/hide, and
  undo/redo of every property change as one history entry.
- Cross-chart sync (same symbol in several workspace cells) uses export/import of drawing payloads
  with revision tracking; the host coordinator routes changes and the engine prevents echo loops.
- Persistence migrates existing V1/V2 drawings into the new model losslessly.

**Exit.** Every existing tool supports the common text, stroke, state and label contract; a host
builds a generic property panel from schemas alone; persistence migrates old layouts; executor
parity fixtures cover text placement on lines, shapes and level tools.

### F6 — Engine-owned OHLCV resampling

Hosts own feeds, but higher-timeframe views and multi-timeframe studies need deterministic
aggregation of a lower-timeframe source into higher-timeframe bars with host-supplied session
boundaries. This powers multi-timeframe study inputs, session and weekly profiles, and derived
chart types without host-side re-aggregation. Timezone and session policy come from the host
explicitly; the engine never uses the browser timezone.

## Order-flow catalog

Each item names its data source and dependency. "Candle" means the item also works in a
candle-only approximation mode that is clearly labeled as such; tape items never silently fall back.

| ID | Item | Source | Depends on | Notes |
| --- | --- | --- | --- | --- |
| OF1 | Cumulative volume delta (CVD) pane: candles or line, session/continuous/anchored reset | Tape | F2 | Session delta already exists inside footprint bars; expose it as a proper series |
| OF2 | Bar delta histogram, delta %, max/min delta, volume split (buy/sell/unknown) histogram | Tape | F2 | |
| OF3 | Session, daily, weekly, composite volume profiles with developing POC/VAH/VAL lines | Tape (candle mode available) | F2, F6 | Replaces candle-only approximation for tape users |
| OF4 | Fixed-range volume profile drawing | Tape or candle | F2, F5 | Drawing whose statistics come from the engine |
| OF5 | Anchored volume profile drawing | Tape or candle | F2, F5 | |
| OF6 | Naked (virgin) POC and value-area level extension until touched | Tape | OF3 | |
| OF7 | Delta profile and bid/ask split profile | Tape | OF3 | |
| OF8 | TPO / Market Profile: letters or blocks, initial balance, single prints, POC, value area, split/merge sessions | Candle or tape | F6 | Period and session boundaries are host-supplied |
| OF9 | VWAP standard-deviation and percent bands; session/weekly/monthly reset | Candle or tape | F4 | Extends existing VWAP |
| OF10 | Anchored VWAP drawing with bands | Candle or tape | F4, F5 | |
| OF11 | Large-trade bubbles and volume dots: size by volume, color by side, threshold filters, aggregation of consecutive prints | Tape | F2 | New bounded marker primitive path |
| OF12 | Footprint variants: profile-in-bar, volume ladder, horizontal imbalance mode, delta-only, bid/ask histogram cells | Tape | F2 | Extends existing footprint LOD |
| OF13 | Unfinished auctions, absorption and exhaustion markers with explicit documented rules | Tape | OF12 | Rules must be deterministic and parameterized, never heuristic black boxes |
| OF14 | Tick, volume and range candles; footprint on the same bars | Tape | F1 | Trade-count and volume aggregators already exist |
| OF15 | Liquidity heatmap (resting depth over time) with color scaling, thresholds, and trades overlaid | Depth + tape | F3, OF11 | Bounded by visible time buckets × visible price rows |
| OF16 | DOM ladder data model: price ladder, bid/ask size, recent volume at price, own orders | Depth + trading | F3 | Engine exposes snapshot and geometry; ladder as chart-side panel primitive |
| OF17 | Depth-derived studies: book imbalance, cumulative depth curve, minimum-size and distance-from-touch filters | Depth | F3 | |
| OF18 | Time and sales data view model (bounded recent prints with filters) | Tape | F2 | Host renders the list; engine provides the filtered bounded window |

## Chart types

| ID | Type | Depends on |
| --- | --- | --- |
| CT1 | Hollow candles, columns, high-low bars, step line, line with markers | — |
| CT2 | Heikin Ashi (derived series with real OHLC exposed separately for trading and crosshair) | F4 |
| CT3 | Tick, volume and range bars | F1 |
| CT4 | Renko (box size fixed or ATR), Line Break, Kagi, Point & Figure | F1 |
| CT5 | Higher-timeframe overlay candles on a lower-timeframe chart | F6 |
| CT6 | Symbol comparison overlays: several instruments on one pane, shared comparison anchor bar, per-symbol legend values | — (percentage and indexed-to-100 scale modes already exist in `price_scale_core.rs`) |

## Indicator catalog

Current: SMA, EMA, EMA ribbon, WMA, Bollinger, RSI, MACD, Stochastic, ATR, VWAP. Deliver in tiers;
each indicator ships with incremental state, rebuild tests, typed schema, persistence and a
reference-value fixture computed independently.

| Tier | Indicators |
| --- | --- |
| I1 — core professional set | Volume (as a study with MA), OBV, ADX/DMI, Parabolic SAR, SuperTrend, Ichimoku, Keltner Channels, Donchian Channels, CCI, Williams %R, Stochastic RSI, ROC/Momentum, MFI, CMF, HMA, VWMA, DEMA, TEMA, SMMA/RMA, standard deviation, pivot points (standard, Fibonacci, Camarilla, Woodie, DeMark), ZigZag |
| I2 — breadth | Aroon, Awesome Oscillator, Chande Momentum, Chaikin Oscillator, Coppock, DPO, Elder Force, Ease of Movement, Fisher Transform, Historical Volatility, KST, Klinger, Linear Regression channel/curve, Mass Index, Ultimate Oscillator, TRIX, TSI, Vortex, Envelopes, ALMA, KAMA, McGinley Dynamic, Chop Zone/Choppiness, Bollinger %B and Bandwidth, ATR bands, Accumulation/Distribution, Price Volume Trend, Volume Oscillator, Relative Volume |
| I3 — structure | Swing highs/lows, market structure breaks, fair value gaps, order blocks, session highs/lows, previous day/week/month levels, opening range |
| I4 — extension API | Typed custom study API in Rust and TypeScript: declare inputs, parameters and outputs; provide incremental update and rebuild functions; engine owns scheduling, bounds, styles, persistence and rendering |

Multi-timeframe inputs (a daily RSI on a 5-minute chart) depend on F6. Alert conditions on study
outputs remain host-evaluated; the engine exposes the values and crossing snapshots.

## Drawing tool catalog

Every tool implements the F5 common contract. Placement types refer to `DrawingPlacement` in
`drawings/tools.rs`; new placement kinds are added only when a tool genuinely needs one.

| Family | Tools |
| --- | --- |
| Lines | Trend line ✓, ray, extended line, info line (price/bars/percent/angle), trend angle, horizontal line ✓, horizontal ray ✓, vertical line ✓, cross line, arrow line |
| Channels | Parallel channel, regression trend (with deviation settings), flat top/bottom, disjoint channel |
| Pitchforks | Andrews, Schiff, modified Schiff, inside pitchfork, pitchfan |
| Fibonacci | Retracement, trend-based extension, channel, time zones, trend-based time, speed resistance fan, speed resistance arcs, circles, spiral, wedge |
| Gann | Gann box, square, square fixed, fan |
| Patterns | XABCD, cypher, ABCD, head and shoulders, triangle pattern, three drives |
| Elliott waves | Impulse (12345), correction (ABC), triangle (ABCDE), double and triple combination, with degree labels |
| Cycles | Cyclic lines, time cycles, sine line |
| Projection and measuring | Long position ✓, short position ✓, forecast, bars pattern (ghost copy), price range, date range, date and price range, projection |
| Volume-based | Fixed-range volume profile (OF4), anchored volume profile (OF5), anchored VWAP (OF10) |
| Shapes | Rectangle ✓, rotated rectangle, ellipse, circle, triangle, arc, curve, double curve, polyline, path ✓, brush ✓, highlighter |
| Annotations | Text ✓, anchored text (screen-anchored), note, price note, callout, comment, price label, signpost, flag mark, arrow markers (up/down/left/right), icon/emoji stamp from a host-provided bounded image set |

Management features (part of F5, not per tool): object tree snapshot (order, groups, names,
visibility, lock), multi-select, group operations, clone, copy/paste payloads, templates, sync
across workspace cells, and bulk remove.

## Platform-driven requirements

The Aeris platform roadmap (`plan/trading_platform_feature_roadmap.md` in the platform
repository) adds risk controls, session replay, trade review, order-level analytics and
fundamentals context. Most of that work is host-owned: canonical market and account state, rule
evaluation, recording storage, data fetching and every panel or dialog stay in the platform. The
items below are what the engine must provide so the platform can render those features from typed
APIs without reimplementing chart math. They follow the same principles, verification and
headless boundary as the rest of this plan.

Aeris's `market_runtime` remains the canonical owner of order books, trades and order-level
(market-by-order) state. Engine stores such as F2 and F3 are chart-side projections fed from the
platform's publications, never a second canonical market model.

Aeris owns these outside the chart, so they are not engine work for this host:

- **DOM ladder.** Aeris's DOM is its own GPUI widget (`terminal_ui`, fed by the platform's
  canonical order book). OF16 remains in this plan for other hosts, such as browser consumers, but
  it is not an Aeris prerequisite.
- **Time and sales.** Aeris renders its panel from its own trade tape. OF18 is likewise for
  other hosts.
- **Trading lock for risk lockouts.** The host already controls chart trading: it forwards
  gestures (`trading_drag_start_at` and related calls) and drains `take_trading_intents`. When a
  lock is active, the host stops forwarding trading gestures, rejects intents and shows the lock in
  its own chrome. No engine state is required.

### PD1 — Host annotations on trading objects

**Problem.** Order and position chips show only engine-formatted quantity, kind and PnL. The
platform needs to show estimated queue position, fill likelihood, rule warnings ("breaks daily loss
limit at stop") and copier status on the same objects.

**Required outcome.**

- `WorkingOrder` and `TradingPosition` accept a bounded list of host annotations: short text,
  semantic tone (neutral, info, warning, danger), optional tooltip text and placement.
- Annotations are laid out with the existing chips by the shared trading geometry, clipped and
  hit-tested consistently; overflow collapses deterministically.
- Explicit caps on annotation count and text bytes per object; invalid annotations are rejected
  without changing the object.
- Annotations are presentation only. The engine never computes queue position or rule state.

**Exit.** An order line with two annotations renders identically on every executor, hit-tests to
the right annotation, and updates at live tick rates without rebuilding unrelated trading geometry.

### PD2 — Replay playback contract

**Problem.** Session replay feeds recorded trades, depth and bars into charts at 1× to 100× speed,
seeks backward, and must never show data from after the replay clock.

**Required outcome.**

- A replay clock supplied by the host (microseconds). The engine draws a replay cursor and masks
  or omits everything after it in every series, study, footprint cell, heatmap bucket and marker.
- Seek backward resets derived state from the nearest documented checkpoint (F2, F3 and indicator
  checkpoints) rather than rebuilding full history; seek cost is reported.
- Bulk ordered ingest paths for trades, depth and bars sized for high-speed replay without
  per-event object conversion at the WASM or GPUI boundary.
- Live and replay charts use the same series and study code paths; replay is a data-source mode,
  not a forked renderer.

**Exit.** A recorded session replays at 100× with bounded frame work and flat memory in
`perf_gate`, seek-back equals a fresh load to the same clock, and no fixture shows post-clock data.

### PD3 — Host event layer on the time axis

**Problem.** Economic releases, platform risk windows (no trading two minutes around a release),
session opens and contract roll dates must appear on charts. Drawings are user-editable and
persisted, so they are the wrong owner for host-generated context.

**Required outcome.**

- A non-persisted, non-editable host overlay layer with typed event markers (time, importance,
  short label, optional icon from a bounded host image set) and shaded time windows.
- Markers and windows render on price and study panes, collapse by LOD when dense, and hit-test to
  a host event identifier for tooltips.
- Explicit caps on markers and windows per chart.

**Exit.** Event markers and windows render identically on every executor, survive history prepend
and resampling, and never enter drawing persistence or undo history.

### PD4 — Execution markers and round trips

**Problem.** Trade review needs every fill, grouped into entry-to-exit round trips with their result,
directly on the chart.

**Required outcome.**

- Execution marker variants: circle (current), arrow and triangle, optional size by quantity.
- Round-trip connectors from entry executions to exit executions with a host-supplied result label
  (for example "+3.25 pts, +$162.50"), colored by outcome.
- Hit-testing returns the execution or round-trip identifier; caps on executions and connectors
  per chart.

**Exit.** A day with many round trips renders at every LOD without overlapping labels beyond the
documented collapse rule, and hit-testing selects the correct round trip on every executor.

### PD5 — Cross-chart synchronization

**Problem.** Linked charts (same symbol at several timeframes, or linked symbol groups) must share
crosshair position and optionally visible time range. `workspace.rs` deliberately shares no state.

**Shared contract.** This is the same capability as the linked-chart synchronization in
[plan.md](plan.md) R4. It is built once in the shared engine layer and follows plan.md's
synchronization rules, so financial and general charts use one mechanism. The financial slice is
delivered here first; plan.md R4 later extends the same contract to general domains rather than
adding a second one.

**Required outcome.**

- APIs to read the local crosshair (time, price, pane) and to set an external crosshair that
  renders without being treated as local pointer input.
- APIs to read and set the visible time range.
- Synchronization events carry semantic values (financial time and price here; general domain
  values or declared index matching in plan.md R4) with an explicit mismatch policy, plus a source
  and revision so a bounded host coordinator can route events between independent charts without
  echo loops. Each receiving engine resolves the values against its own data, and disposal removes
  its subscriptions.
- Symbol linking remains host-owned; the engine exposes only crosshair and range primitives.

**Exit.** Two charts with different timeframes track one crosshair and one time range with no
feedback oscillation, on GPUI and in the browser. The event and coordinator contract needs no
financial-only fields that would block its reuse in plan.md R4.

### PD6 — Native and GPUI image export

**Problem.** The platform journal attaches chart images to trades. Only the browser package can
export images today.

**Shared contract.** Image export is one frame-level capability for every chart kind. It also
serves as the frame-rendered image export required by [plan.md](plan.md) (R4 export behavior and
the "equivalent frame exports" coverage row). General panes use the same path; they do not get a
separate exporter. State exports (persistence) remain a separate gate, as plan.md requires.

**Required outcome.**

- Render a chart frame to an RGBA buffer at a requested size and scale on the native and GPUI
  paths, including or excluding the crosshair and trading layer, with the same composition rules as
  the browser `take_screenshot`.
- The export works from the ordered frame regardless of whether panes are financial or general,
  and documents which built-in chrome is included.
- Export never disturbs the live chart's state or frame pacing.

**Exit.** Exported images match on-screen output within the existing parity tolerances for a
financial chart and a general Cartesian chart.

### PD7 — Sparse fundamental series on intraday charts

**Problem.** Weekly and monthly context (EIA inventories, CFTC Commitments of Traders, USDA
reports) must be shown beside intraday prices without look-ahead: a value becomes visible only from
its release time.

**Required outcome.**

- Step-after rendering from host-supplied release timestamps, with documented behavior for sparse
  points on second-based axes and across history gaps. Confirm whether `LineType::WithSteps`
  already provides these semantics and extend it only if it does not.
- Column or histogram presentation of the same series in its own pane, and value labels that show
  the as-of release.
- The engine never fetches or interprets fundamental data; it renders host series.

**Exit.** A weekly series on a one-minute chart changes value exactly at each release bar, with no
interpolation and no look-ahead in fixtures and replay (PD2).

### PD8 — Order-level depth inputs and microstructure events on the chart

**Problem.** Rithmic supplies CME market-by-order data. Aeris's adapter already assembles an
order-level book but publishes aggregated levels. Queue position, iceberg detection, pulled
liquidity and order-size clustering are computed platform-side. The DOM shows them in Aeris's
own widget; the chart must show them on price panes and the liquidity heatmap.

**Required outcome.**

- F3 depth ingest accepts the optional per-level order count Aeris already carries, so heatmap
  cells and tooltips can show order counts beside size.
- A typed microstructure event marker (kind: iceberg refill, pulled liquidity, size cluster, sweep;
  price, time, size and host label) rendered in price panes and on the heatmap (OF15), with caps and
  LOD collapse.
- The engine does not implement detection rules. Deterministic detection lives in the platform,
  consistent with the OF13 rule that detections are never heuristic black boxes. Queue position on
  the chart is shown through PD1 order-line annotations.

**Exit.** A recorded order-level stream renders heatmap order counts and event markers identically
on every executor, and markers stay aligned with heatmap buckets after replay seeks.

### PD9 — Depth heatmap rendering budget

**Problem.** OF15 at full resolution means thousands of price rows by hundreds of time buckets.
Lowering each cell to a rectangle, as the feature heatmap does today, will not hold high refresh
rates on dense books.

**Required outcome.**

- OF15 lowers visible buckets to a texture or image primitive (`Prim::Image`, the WebGPU
  textured-quad pipeline and its GPUI and Canvas2D equivalents), with incremental column updates
  for the live edge.
- Color scaling, thresholds and minimum-size filters remain engine semantics applied before
  upload.
- Frame and upload budgets are measured in release builds and added to `perf_gate`.

**Exit.** A dense recorded book renders at the target refresh rate on GPUI with bounded upload per
frame, and results match the rectangle-based reference within parity tolerance.

### PD10 — Dense footprint text budget

**Problem.** Detailed footprint cells update many numeric text runs per frame during fast markets.
GPUI glyph shaping and atlas cost at that density is unmeasured. (Aeris's DOM ladder is a
platform widget, so its text performance is platform work, not part of this item.)

**Required outcome.**

- Measured release benchmarks for footprint text density on GPUI and WebGPU.
- Shared caching of repeated numeric runs where measurement shows shaping dominates, without
  changing text metrics or parity.

**Exit.** Documented frame budgets for a reference footprint view are met on GPUI and guarded by
`perf_gate`.

### Platform feature to engine prerequisite map

| Platform feature | Engine prerequisites |
| --- | --- |
| Footprint, volume profile and CVD panels | Existing footprint and profile; F2, OF1–OF3, OF12, PD10 |
| Big-trade bubbles and sweeps | F2, OF11; PD8 for sweep markers |
| Liquidity heatmap | F3, OF15, PD8, PD9 |
| Queue position, icebergs, pulled liquidity on charts | PD8; PD1 for queue position on the order line |
| Tick, volume and range charts | F1, OF14, CT3 |
| Chart trading and brackets | Existing trading layer; PD1 |
| Prop-firm rules, pre-trade checks, lockouts | PD1 for warnings on order lines; the lock itself is host-owned |
| Session replay and trade review | PD2, PD4, PD6; F1 for sub-second tape display |
| Economic calendar and risk windows | PD3 |
| Fundamentals dashboards on charts | PD7; existing panes, histogram and stepped lines |
| Linked charts and symbol groups | PD5 |
| Custom studies and study scene objects | I4, F4; platform study roadmap Phases F–G |
| DOM ladder and time and sales | None; Aeris platform widgets |

## Delivery sequence

Each phase ends only when its exit criteria pass through the real host and executor paths.

### E0 — Baseline and reference fixtures

Pin reference behavior for each catalog family from public documentation and observed behavior
(no copied implementation code or assets; see the licensing rule in AGENTS.md). Record current
release performance baselines for footprint, drawings and indicators. Deliver typed schema
conventions (parameter and property descriptors) used by F4 and F5.

### E1 — Drawing model and customization (F5) with existing tools

Migrate the ten existing tools to the common contract first: shared text layout on every tool,
state/lock/z-order, labels and stats, templates, multi-select, persistence migration. This fixes
today's biggest customization gap before adding tools, so new tools are built on the final model.

### E2 — Shared tape and first order-flow studies (F2, OF1, OF2, OF9, OF11)

CVD, delta histograms, VWAP bands and trade bubbles from the shared tape. These reuse the existing
footprint classification and are the fastest route to usable order flow.

### E3 — Study input model and indicator tier I1 (F4, CT1, CT2)

Selectable sources, typed schemas and persisted bindings, then the I1 indicator set, Heikin Ashi
and the simple chart types.

### E4 — Non-time bars (F1, OF14, CT3, CT4)

Logical bar identity, tick/volume/range bars, footprint on those bars, then Renko, Line Break,
Kagi and Point & Figure.

### E5 — Profiles and resampling (F6, OF3–OF8, OF10, CT5)

Session and composite profiles, developing levels, naked POCs, TPO, fixed-range and anchored
profile drawings, anchored VWAP drawing, higher-timeframe overlays and multi-timeframe study inputs.

### E6 — Depth (F3, OF15–OF18)

Order-book model, liquidity heatmap, DOM ladder model, depth studies, time-and-sales view model.

### E7 — Drawing catalog expansion

Lines and channels, Fibonacci, pitchforks, measuring tools and annotations first (highest daily
use), then Gann, patterns, Elliott waves, cycles and remaining shapes.

### E8 — Breadth and extension

Indicator tiers I2 and I3, custom study API (I4), comparison overlays (CT6), footprint variants and
auction/absorption markers (OF12, OF13).

### E9 — Platform-driven requirements (PD1–PD10)

These are scheduled by platform need rather than as one block:

- PD1 and PD4 extend the existing trading layer and can start immediately. The platform's risk
  warnings and trade review depend on them.
- PD3, PD5, PD6 and PD7 are independent of the other foundations and can run beside E1–E3.
- PD2 must be designed with F1, F2 and F3 so checkpoints support fast seeks from the start; it is
  delivered after E2 and extended when E4 and E6 land.
- PD8 and PD9 are part of E6 and its exit criteria. PD10 runs with E2 and OF12 footprint work.

Phases E1 and E2 are independent and can run in parallel. E4 is the largest architectural change
and should begin design during E2 so later phases do not build on the second-based axis
assumption.

## Verification and evidence

For every catalog item:

- Deterministic engine tests for math, including an independently computed reference fixture,
  edge cases (empty, one bar, gaps, unknown sides, off-grid rejections, corrections) and rebuild
  equivalence (incremental result equals full rebuild).
- Frame fixtures for ordering, clipping and LOD; parity across GPUI, WebGPU, Canvas2D and native;
  GPUI replay for executor changes.
- Browser Playwright tests through the published package for browser-facing APIs.
- Persistence round trip and migration from the previous schema version.
- Performance evidence in release builds added to `perf_gate`: tip update cost independent of
  history length, frame work bounded by visible bars/levels/buckets, steady-state allocation,
  retained memory under retention caps, and depth-update soak for F3.
- The complete gates in [AGENTS.md](../AGENTS.md) before each commit, and Architecture.md updated in
  the same commit as any ownership or data-flow change.

## Definition of completion

Aeris is a complete headless trading chart engine for this plan when a host can build a
professional order-flow and technical-analysis workstation — footprint, CVD, profiles, TPO,
liquidity heatmap, DOM, non-time bars, the I1–I3 indicator catalog, the full drawing catalog
with per-tool customization, and the platform-driven annotation, replay, event, execution, sync,
export, fundamentals and order-level display contracts (PD1–PD10) — using only typed engine APIs, with identical results across every
backend, bounded resources, deterministic persistence, and measured performance evidence. Until
then, report delivered items and remaining gaps precisely against the catalog IDs above.
