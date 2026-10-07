# Studies milestone evidence (B9)

Closure evidence for `plan/Expansion.md` batch B9 — breadth and extension (I2 breadth studies,
I3 structure studies, I4 custom studies, OF13 auction markers). Collected 2026-10-06 on the
closure working tree (Windows 11, 24 logical CPUs, 32 GB RAM; Rust 1.99, Chromium via
Playwright 1.63).

Screenshots are deliberately **not committed**. They live outside the repository at
`%TEMP%\aeris-b9-evidence\` on the collecting machine and can be regenerated with the
reproduction commands below.

## Scope delivered

- **I2 breadth tier** — 29 additional indicator kinds on the shared binding, schema,
  persistence, and checkpoint paths (catalog row in `plan/Expansion.md`, inventory and repair
  semantics in `docs/Architecture.md`).
- **I3 structure tier** — swing points, market structure (BOS/CHoCH), fair value gaps, order
  blocks, session high/low, previous day/week/month levels, and opening range, all as typed
  engine bindings with interval-indexed annotations painted by the shared frame (no new
  primitives, no backend forks).
- **I4 custom studies** — typed registration/binding API in Rust and TypeScript with
  engine-owned scheduling, bounds (64 types, 32 bindings, 1–5 outputs), fault stream, and V3
  persistence; distinct from external studies where the host pushes computed values.
- **OF13 auction markers** — unfinished auctions, exhaustion, and absorption with
  deterministic parameterized rules and optional revisitation rays, as a bounded dependent of
  the chart-level trade stream (16 handles).

## Whitespace and warm-up

Built-in and custom scalar study outputs begin at their first valued row. Missing source
rows before that row are not warm-up samples, and an output with no values is empty.
An interior missing row stays in the output's time range with no value. Recursive and
cumulative studies, including EMA and RSI, carry their state across it and resume using
the last valid input; window studies stay blank while their source window contains the
gap, then recover. A historical correction before an output's first row can move its
start, so the engine replaces that aligned output rather than updating an absent row.
These rules also apply when built-in and custom studies are chained in either order.
Market structure, fair value gap, and order block anchors are the exception: each has
one blank entry for every source time, including leading and interior gaps.

## Screenshots

| File (under `%TEMP%\aeris-b9-evidence\`) | Scene |
| --- | --- |
| `structure-studies-light.png` | All seven structure/session studies on the demo candles, light theme, Canvas2D |
| `structure-studies-dark.png` | Same scene, dark theme |
| `structure-studies-overflow.png` | Same studies with the full 1,000-bar history fitted (`fit_content`) — dense zoomed-out view exercising the bounded annotation painter |
| `auction-markers-dark.png` | Footprint series bound to a synthetic trade stream with 117 auction marks (39 unfinished auctions, 39 absorptions, 39 exhaustions) and revisitation rays, dark theme |

Reproduction (PowerShell, from the repository root):

```powershell
cd examples\web_demo
$env:AERIS_CHARTS_TEST_PORT = "4174"
node test_server.mjs   # leave running; stop the PID owning port 4174 afterwards

# In a second shell, with AGENT_BROWSER_CDP and AGENT_BROWSER_SESSION cleared:
$chrome = "$env:LOCALAPPDATA\ms-playwright\chromium-1243\chrome-win64\chrome.exe"
agent-browser --session b9evidence --executable-path $chrome `
  open "http://127.0.0.1:4174/?backend=canvas2d&theme=light"

# Structure studies scene (themed + overflow shots). Run through `eval --stdin` or `-b`:
#   const c = window.__chart, m = window.__main;
#   c.add_swing_points(m, 5, 5); c.add_market_structure(m, 5, 5);
#   c.add_fair_value_gaps(m, { show_mitigated: true });
#   c.add_order_blocks(m, { show_mitigated: true });
#   c.add_session_levels(m); c.add_previous_period_levels(m);
#   c.add_opening_range(m, 1800);
# Overflow shot additionally runs: c.time_scale().fit_content();
agent-browser --session b9evidence --executable-path $chrome screenshot <out.png>

# Auction markers scene: remove the demo series, add a footprint series plus trade stream
# (tick_size 1, 60 s bars), push a 40-bar tape alternating heavy absorption at one extreme
# with a strictly decreasing 3-level finish at the other, then:
#   chart.add_auction_markers(footprint, stream, { extend_until_revisited: true })
#   -> snapshot reports 117 marks (39 per kind).
```

WebGPU visuals use Playwright (the config's SwiftShader flags), e.g.
`npx playwright test tests/custom-studies.spec.mjs --project=chromium`, which includes the
combined Canvas2D/WebGPU parity scene for structure studies and auction markers.

## Accessibility review

The `examples/web_demo/tests/accessibility.spec.mjs` audit injects dev-only axe-core 4.12.1
and checks the full page with the `wcag2a`, `wcag2aa`, `wcag21a`, and `wcag21aa` tags.
Financial, General, split grid, chart-focused shortcuts panel, and 390px mobile layouts
each have zero violations in both light and dark (10 states). It also checks that
multiple charts' `aria-describedby` targets are distinct, resolved, and owned by their
respective charts. The previous demo `color-contrast`, `meta-viewport`, and
`aria-prohibited-attr` findings were fixed with demo-scoped contrast text tokens
(without changing shared brand tokens), a zoom-permitting viewport, explicit group/region
roles, globally unique pane description IDs, and a visible mobile heading. The chart
surface itself is canvas-rendered, so axe cannot audit its pixels or all manual contrast
questions. Chart keyboard interaction is
engine-owned (`aeris_charts_engine` `chart_input.rs`): arrow keys pan, `+`/`-` zoom around the
plot center, Escape cancels hover/drag state, and `ChartEngine::input_target_key_down` exposes
named focus targets (panes, trading objects) that accessibility hosts can surface as focusable
proxies. Studies and auction markers add no interactive overlay of their own — annotations
have no hit targets — so they inherit this behavior unchanged.

## Competitor comparison (public documentation only)

Sources: TradingView support/Pine Script docs, Sierra Chart documentation pages, ATAS
help/learn/docs portals (URLs below; accessed 2026-10-06). No account-only material was used.

| Capability | Aeris Charts (this milestone) | TradingView | Sierra Chart | ATAS |
| --- | --- | --- | --- | --- |
| Footprint / numbers bars | Engine-owned footprint series on the canonical trade stream, LOD-selected rendering | Volume Footprint chart type on Supercharts (support doc 43000726164) | Numbers Bars study with bid/ask text, imbalances (NumbersBars doc) | Cluster charts (footprint) as a core chart type |
| Unfinished auctions | Built-in mark kind with documented deterministic rules, optional revisitation ray, snapshot API | Not a documented footprint feature; community Pine approximations only | Supported on Numbers Bars (documented in support-board guidance and Numbers Bars inputs) | Dedicated Unfinished Auction indicator (help article 72000602495) |
| Exhaustion / absorption markers | Built-in parameterized kinds (strictly-decreasing finish, ratio + rejection rules) | Not documented as native marks | Available through Numbers Bars diagonal imbalance highlighting and ACSIL studies | Documented footprint patterns (absorption, finished/unfinished auctions) in ATAS learn portal |
| Market structure / FVG / order blocks | Typed engine bindings with interval-indexed annotations, rebuilt from source on restore | No native equivalents; community Pine scripts dominate | Via community/ACSIL custom studies | Via custom indicators (ATAS API) |
| Session / previous-period / opening-range levels | Built-in studies with UTC or host-supplied session calendars (≤ 20,000 spans, runtime-only) | Session-based drawing via Pine (`time`/`session` functions); no host-calendar injection | Daily/weekly/monthly levels via chart settings and studies | Session tools via indicator settings |
| Custom study API | Typed Rust + TypeScript: declared inputs/params/outputs, engine-owned incremental scheduling, bounded types/bindings/outputs, fault stream, V3 persistence | Pine Script v6: single language, script-managed drawing objects with per-script object limits | ACSIL: C++ DLL interface, full manual lifecycle | ATAS API: .NET/C# custom indicators |

Public-doc URLs: `tradingview.com/pine-script-reference/v6/`,
`tradingview.com/support/solutions/43000726164`, `sierrachart.com/index.php?page=doc/NumbersBars.php`,
`sierrachart.com/index.php?page=doc/AdvancedCustomStudyInterface...`,
`help.atas.net/en/support/solutions/articles/72000602495`, `docs.atas.net/en/`,
`learn.atas.net/volume-basics/volume-analysis/footprint-patterns`.

## Recorded release benchmarks

`cargo run -p aeris_charts_native --example perf_gate --release` on the closure tree, 2026-10-06,
with Target N remeasured after the axis fix on 2026-10-07: **ALL TARGETS PASS** on both runs.
Measured values (budget in parentheses):

| Target | Result |
| --- | --- |
| A — 60fps @ 10 series × 50k bars, build_frame | 0.35 ms (16.67) |
| B — 1,000,000 bar load | 96.32 ms (300) |
| C — 1M canonical pointer samples | 0.00 ms (0.01) |
| D — 250k footprint trades + live batch + correction | load 40.22 ms (300); batch 26.90 ms (50); correction 12.14 ms (300); frame 0.57 ms (16.67); retention ceiling pass |
| J — dense footprint WebGPU text encoding | p99 0.01 ms (2.00); all resolved runs preserved |
| E — 100k visible-bar volume profile | refresh 3.76 ms; cached 0.18 ms; developing 6.93 ms (all 16.67) |
| F — 100k-point XY line | frame 1.49 ms (16.67); hit 1.45 ms (8.00) |
| G — 5-series mixed dashboard, 100k rows | frame 1.52 ms (16.67); hit 1.99 ms (8.00); memory 5.51 MiB (12) |
| H — 50k bars + 50k general points | frame 0.83 ms (16.67); memory 5.68 MiB (16) |
| I — 100k error bars | frame 10.31 ms (16.67); hit 2.82 ms (8.00); memory 6.87 MiB (16) |
| K — 100× replay, 6,000 s / 60 frames | 0.58 ms (16.67); memory flat 5.65 MiB |
| L — two 1.2M-update depth soaks | worst batch 10.10 ms (150); frame 0.31 ms (16.67); live-edge image 0.00 MiB (0.02); memory flat 66.03 MiB |
| M — sustained order-flow tape (600 × 4-trade batches + frame) | p50 0.054 ms, p99 0.09 ms (4.00); worst 5.25 ms (16.67); late print 0.31 ms (16.67); ceiling pass |
| N — 7 structure studies × 1M rows | initial build 356.18 ms; tip-replacement p99 0.16 ms (8.00); one historical correction 20k rows back 9.89 ms (100); 3,999 FVG + 2,108 order-block zones retained |
| O — **new:** sustained tape with auction markers | p50 0.057 ms, p99 0.17 ms (4.00); worst 7.57 ms (16.67); late print 0.55 ms (16.67); 5,084 retained marks; ceiling pass |

Target N tip-replacement p99 is now 0.16 ms on this machine with all seven structure bindings.
The axis reads the 1M-row merged timeline in place on both synchronization passes rather than
copying it per update. Tick-time copies remain necessary for sequence axes and changing time
projections. The earlier whitespace-only structure-anchor optimization also avoids a
full-column backward base-index scan.

Supporting gates on the same tree: `cargo fmt --all -- --check` clean; `cargo clippy -p
aeris_charts_core -p aeris_charts_engine -p aeris_charts_native --all-targets -- -D warnings`
clean; `cargo test -p aeris_charts_core` 184 passed and `cargo test -p aeris_charts_engine`
981 passed (includes the new base-index and anchor-flag regression tests). Browser-side parity
and persistence evidence for these features was collected during milestone validation
(`custom-studies.spec.mjs`, `structure-studies.spec.mjs`, `session-studies.spec.mjs`,
`footprint.spec.mjs`; see `validation/extensions/` syntheses).
