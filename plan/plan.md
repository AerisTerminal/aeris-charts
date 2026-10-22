# Aeris Charts All-in-One Architecture and Competitive Delivery Plan

Aeris will be a complete financial and general visualization library. Lightweight Charts is the
financial competitive reference; Recharts is the general charting competitive reference. This plan
covers the **general (non-financial) charting program** toward full Recharts parity and beyond.
Trading and order-flow work was delivered by the trading expansion plan (batches B1–B9, completed
2026-10-06 and removed after closure; the final text is `plan/Expansion.md` at commit `71b17a8`).
Both programs share one `ChartEngine` and one frame contract, documented in
[architecture.md](../docs/architecture.md).

How to read this file:

1. **Status at a glance** — where every phase and batch stands today.
2. **How work is delivered** — the batch, gate and commit rules.
3. **Delivered work** — what is implemented, with evidence.
4. **Known gaps** — source-confirmed problems and the phase that fixes each.
5. **Phases R0–R8** — remaining work as checklists, with exit criteria.
6. **Parity coverage matrix** — the capability rows that define parity.
7. **Scope, architecture rules, verification and completion** — the standing rules.

Keep this file current: when a batch lands, tick its checklist items, add it to **Delivered work**
with its commit, and update the status table in the same commit. A phase or matrix row changes to
**Verified** only with the evidence required in **Verification and evidence policy**.

## Status at a glance

Updated 2026-10-20. Plan baseline dated 2026-09-23. The 2026-09-25 pause ended after the trading
expansion completed on 2026-10-06; general-chart batches have resumed in dependency order.

| Batch | Phase | Scope | Status | Done so far | Next |
| --- | --- | --- | --- | --- | --- |
| G1 | R0 + R1 | Competitive baseline, lifecycle and mutable object foundations | **Complete** | Recharts 3.10.1 matrix and rendered-mark fixtures; general-first topology; atomic handles; host cleanup; focus retention; validated general-first V2 restore | G2 |
| G2 | R2 | Scales, axes and responsive layout | **Complete** | Numeric/log/symlog, UTC temporal and category views; typed ticks; grids/zero rules; multi-axis titles; padding; bounded responsive layout | G3 |
| G3 | R3 | Cartesian visual and data semantics (remainder) | **Complete** | Cartesian families, stack offsets, gradients, composition, per-item styles and required portable gates delivered | G4; calibrated GPUI images and official browser benchmark remain separately pending |
| G4 | R4 | Components and interaction | **Complete** | Default legend, tooltip and brush controls; engine selection and semantic linked sync; shared-frame export coverage | G5; manual assistive-technology and calibrated GPUI image evidence remain pending |
| G5 | R5 | React and framework-neutral authoring | **Complete** | Keyed declarations and mapped rows; atomic shared composition; controlled brush; first-pane ownership; lifecycle, SSR, packed-consumer and browser evidence | G6; manual assistive-technology review remains pending |
| G6 | R6 | Polar families and transitions | **Complete** | `f64` polar geometry and axes; pie/donut, radar, polar-area and radial-bar paths; single-fill ordered polar mesh; bounded cached hits, slice legends, shared-frame leader labels, pointer and keyboard selection; WASM/TS bindings; opt-in fixed-clock value and numeric/temporal X updates, Cartesian and angular category-axis and matched-row category movement, identity-matched entry and exit rows, missing/valid Y, error-bound and bubble-size motion, pie row-order motion, fixed-spoke and changing-count radar morphs, compatible optional-channel changes, collapsed error-bound entry, persisted duration and browser scheduling; polar and Cartesian family fixed-clock frame/hit fixtures, GPUI/Canvas2D structural fill, bounded Canvas2D/WebGPU pixels, and measured 1.25× GPUI/native polar window parity; strict release performance, six package-size limits, and 735 portable browser tests passed | G7; manual assistive-technology and broader device-scale image evidence remain pending |
| G7 | R7 | Hierarchy and flow families | **Complete** | Axis-free `layout` pane topology is available in engine, WASM, and TypeScript; lazy typed stage/tree/flow store validates IDs, values, cycles, depth and bounded retained capacity; funnel, treemap, Sankey and sunburst emit shared ordered-frame geometry with direct or cached exact hits; focused financial-isolation, persistence, atomic-store and geometry fixtures pass; V2 layout data/series round-trip and public restored-series handles, pointer hover/click/Escape and focused-key selection, bounded accessibility snapshots integrated with browser keyboard navigation and announcements, shared-frame visual focus, shared-dataset update fanout, built-in pointer tooltip and toggling legend, opt-in shared-frame labels, atomic live presentation patches, and fixed-clock compatible value transitions are implemented with focused engine/browser evidence; four-family Canvas2D/WebGPU interior pixels and GPUI/Canvas2D structural focus/mark parity pass | G8; manual assistive-technology, competitor screenshot and calibrated GPUI window evidence remain for parity closure |
| G8 | R8 | Parity closure and release readiness | **In progress** | Pinned Recharts 3.10.1 horizontal-bar default and positive/mixed-sign/expanded/positive-only/cumulative/silhouette/wiggle stack pairs pass in Chromium/Firefox/WebKit; a current G2 public browser recheck passes 34 cases with two expected non-Chromium CDP skips. Upstream scope recheck plus focused legend placement, touch, X-axis automatic sizing, responsive resize/reveal/aspect/zero-size/DPR, references/labels, Cell colors/rounded bars, French grouped ticks, polar and four-family hierarchy/flow geometry pairs, plus Sankey accessibility fixtures recorded below; the last full G8 defaults run passed 115 browser cases with 8 expected non-Chromium WebGPU/CDP skips, followed by 15 focused brush, path-family and composition passes. Shared and combined financial/Cartesian/polar/layout WebGPU loss fallback pass in Chromium; combined charts retain pre-update image identity. Mixed V2 restore and cleanup pass in three browsers; native four-family image export matches the live frame; full matrix and manual evidence remain open | Complete matrix evidence, documentation and export/recovery work; then full gate and phase closure |

Notes:

- R3 slices were delivered ahead of R0–R2. They count toward R3 only; R3 closes after R1–R2 land
  and its exit criteria pass.
- G1 is complete after the corrective general-first V2 restore regression, declaration snapshot
  correction, user-surface/lifecycle regressions, and full gates passed.
  The scales/axes and responsive-layout matrix rows are Verified. Other rows remain Partial
  because they include later-phase behavior.

Seven of eight delivery batches are complete. G8 is in progress. Batch completion
does not imply full parity-matrix verification.

## How work is delivered

Work proceeds in **large batches**, as defined in **Work cadence** in [AGENTS.md](../AGENTS.md).
Each batch is one row of the status table: a whole phase (R0 and R1 ship together as G1), not one
option or one series family at a time. Delivered items 1–9 were single-option slices; that cadence
is retired.

- **Implement the whole batch first.** Build every checklist item in the phase, with its regression
  tests and fixtures written as each item is built. Do not stop between items for full gates,
  commits or pushes.
- **Focused checks while implementing.** `cargo check`, unit tests and `cargo clippy` for touched
  crates, and frame fixtures for the affected families. Nothing broader.
- **One full gate at the end.** Run the complete gates in **Verification and evidence policy** once,
  plus Playwright when the batch changes browser-facing behavior and GPUI parity/replay when it
  changes GPUI execution. Fix every failure and rerun until green. If the cause is unclear, rerun
  focused checks item by item.
- **One commit and push per batch.** Commit with a structured message listing delivered checklist
  items and verification, then push `main`. Never commit a batch with a failing or skipped required
  gate.
- **Update this file in the same commit.** Tick the checklist, update **Delivered work** and the
  status table, and update `docs/architecture.md` when ownership or execution paths changed.
- **Manual evidence at phase closure, not per batch.** Screenshots, accessibility review,
  competitor comparisons and recorded benchmarks are collected once, when the phase closes.

A batch may be split into two commits only when it is too large to review as one, and each part
must pass the full gate on its own.

## Delivered work

G1 delivered the versioned Recharts 3.10.1 comparison matrix and executable private fixtures,
general-first and mixed topology, stable atomic general handles, last-pane ownership, failed-install
cleanup, series/row focus retention through reorder and relayout, live financial creation after
retiring an empty general-first pane, and policy-v7 release baselines. Evidence lives in
`examples/web_demo/tests/g1_foundations.spec.mjs`, the existing public general browser/native tests,
and `benchmarks/baselines/g1_2026_10_10.json`.

G2 closed the shared scale, axis and responsive-layout contract. Numeric, logarithmic, symmetric-log,
UTC epoch-millisecond temporal, band and point transforms now share runtime views with geometry, hits,
ticks, grids and zero rules. Numeric axes expose bounded transformed-space domain padding; explicit
typed ticks retain portable display labels, including duplicate display text over distinct category
identities. Multiple reversed axes and titles use bounded strip layout, and browser resize, DPR, font,
aspect-ratio, hidden/revealed and small-container paths retain hit/input alignment. Evidence lives in
`crates/aeris_charts_engine/src/frame/tests.rs`,
`crates/aeris_charts_render_gpui/tests/parity.rs`, and
`examples/web_demo/tests/general_charts.spec.mjs`; the complete Rust/WASM/package/Playwright/GPUI and
strict performance gates passed on 2026-10-10. The corrective package-size gate uses a WASM-only
Cargo optimization level `z` before wasm-opt `-Oz`, while native release remains at optimization
level 3. Policy-v7 artifact measurements are tarball 1,666,975 bytes, unpacked 5,230,364 bytes,
raw WASM 4,360,081 bytes and Brotli WASM 1,043,172 bytes, all under their unchanged ceilings.
The interactive general dashboard was checked in its
light-theme filtered-bar state with Canvas2D, while deterministic Chromium/Firefox/WebKit coverage owns
the resize, DPR, pointer and keyboard assertions. No separate assistive-technology session is claimed
for this engine-focused phase. These local artifact measurements establish the deterministic CI gate,
not a controlled-runner browser performance claim.

The items below were implemented earlier and belong to R3.

| # | Capability | Commit | Evidence |
| --- | --- | --- | --- |
| 1 | General-series `line_width` configurable through engine, WASM, TypeScript, persistence and public browser API | `d17a39f` | Slice gates below |
| 2 | General path `line_style`: portable `solid`, `dotted`, `dashed` through the shared frame contract | `819bcbb` | Slice gates below |
| 3 | `xy_area` explicit finite `baseline_value` with shared fill geometry, hit-testing, persistence, WASM/TypeScript and browser coverage | `6a5684b` | Slice gates below |
| 4 | Opt-in `point_markers` with configurable radius on line, area, stacked-area and range-area; shared geometry, matching hit targets, atomic live mutation, V2 persistence, WASM/TypeScript, browser coverage. Also repairs live mutation of items 1–3 | — | Slice gates below |
| 5 | Persisted marker symbols `circle`, `square`, `diamond`, `triangle` for scatter and path markers, with symbol-matched exact hit geometry. Bubble marks remain area-scaled circles | — | Slice gates below |
| 6 | Persisted `linear`, `step`, `curved` interpolation on line, area, stacked-area and range-area; shared path and coupled-band expansion on every renderer with matching exact/nearest hits; stacked areas reject mixed interpolation | — | Slice gates below |
| 7 | Opt-in persisted `connect_missing` on the same families. Missing rows stay queryable but no longer split runs; transform-invalid coordinates remain hard gaps; stacked members share one policy | — | Slice gates below |
| 8 | Bounded persisted `fill_opacity` on area and range-area fills (ordinary, stacked, coupled-band), preserving the gradient relationship; Rust persistence, WASM serialization, TypeScript options, Chromium/Firefox/WebKit round-trip coverage | `1db0dfe` | Workspace tests, workspace and WASM clippy, package lint/build/typecheck/smoke test, general-chart browser matrix (88 passed, 2 skipped) |
| 9 | Category `range_bar` series with low/high bounds, shared rectangle geometry and exact hits, typed/object browser ingestion, persistence-compatible kind mapping, accessibility text, and browser/native regression coverage | `3fe3b22` | Engine tests (including logarithmic Y geometry), workspace tests/clippy/WASM clippy, package lint/build/typecheck/pack smoke, native release perf gate, and the full Chromium/Firefox/WebKit browser suite (363 passed, 15 skipped) |

Slice gates for delivered R3 items (historical; remaining work uses batch gates): applicable Rust tests, clippy (including the WASM target), package
lint/build/typecheck/package smoke test, Chromium browser tests, formatting checks and the native
release performance gate.

These slices do not close R3. The phase still requires the rest of the Cartesian visual and data
matrix and its complete acceptance evidence.

The existing error-bar implementation already covers numeric and temporal X/Y bounds and
category-centered Y bounds. Engine geometry/hit and V2 persistence tests, GPUI parity cases in
`crates/aeris_charts_render_gpui/tests/parity.rs`, and the three Chromium browser cases in
`examples/web_demo/tests/general_charts.spec.mjs` establish the audited G3 checklist item.
The browser cases passed again on 2026-10-11. The G3 batch (this revision) completes shared
curve and gap geometry, area baselines, signed stack offsets and exposed bar corners, gradients,
row-level colors and symbols, composition, heatmap intensity, and box styling across engine,
executors, WASM and public TypeScript options. Its fresh-asset gate on 2026-10-11 passed locked
workspace tests and warning-denying workspace/wasm32 Clippy; formatting; package `npm ci`,
lint/build/typecheck, API/namespace/release guards and pack smoke; native frame/golden tests;
GPUI structural parity/replay; 564 portable Chromium/Firefox/WebKit tests (16 platform-intended
skips); strict native release performance (all targets); and all six policy-v7 package-size
ceilings (`node benchmarks/benchmark.mjs size`). The browser shared native/Canvas2D pane fixture
had 0 differing pixels out of 1,937,997; WebGPU/Canvas2D shared-frame residual was 2,207
antialiasing-edge pixels (maximum channel delta 32), within its existing portable fixture
tolerance. Neither comparison proves GPUI window pixels.

The optional Windows GPUI/WebGPU image matrix is **not passed**: its D1 canonical WebGPU hash
is `a8d1a034` versus `c53be636` on this machine, identically for clean `ba4723d` and the
G3 candidate (0/1,937,997 differing pixels). Physical monitor DPR is 1.25, not the required
1.5, so no actual GPUI window image could be captured. The CI matrix is machine-specific and
`continue-on-error`; GPUI pixel equivalence remains pending a calibrated runner, not inferred
from passing structural parity.

The post-delivery G3 scrutiny found that the new presentation validator rejected valid dormant
styles saved by earlier V2 writers or retained by partial browser option patches. A separately
gated corrective change preserves dormant marker radius, line width and dash style without
loosening numeric bounds or active-kind checks. Engine toggle and old-shaped V2 fixtures plus
public browser toggle/restore fixtures cover the compatibility boundary. The correction's
2026-10-11 gate passed locked workspace tests, warning-denying workspace/wasm32 Clippy,
formatting, package install/lint/build/typecheck/pack/API/namespace/release guards,
all six policy-v7 package-size ceilings, strict native performance, GPUI structural
parity/replay, and 570 portable Chromium/Firefox/WebKit browser tests (16 intended
platform skips). The physical-DPR GPUI pixel limitation above is unchanged.

A second G3 compatibility audit compared each `validate_presentation` branch introduced or
tightened in `ba4723d..864dd59` against pre-G3 accepted states. The new gradient, heatmap,
bar-sizing, corner, and baseline-policy fields are absent/defaulted in old V2 documents;
preexisting fill opacity, symbol, connection, interpolation, marker, width, baseline-kind,
group, and stack-kind checks remain unchanged. The first correction removed dormant
line-width/style/radius kind rejections. This follow-up removes the remaining new cross-field
stack/baseline rejection: valid explicit or policy baselines persist while stacked and apply again
when unstacked, but explicit-plus-policy conflict, nonfinite values and incompatible kinds remain
invalid. The newly unconditional radius bound had also rejected pre-G3 finite dormant zero (and
other finite out-of-range) values on nonmarking kinds/markers-off lines and the meaningful zero-cap
error-bar state. Dormant radii remain finite and unchanged; scatter/active markers retain [1, 64],
error-bar caps retain [0, 64]. Old V2 and merged browser patch regressions exercise these exact
states, atomic invalid updates, and retained identities. The public tests failed before the fix
against the previous built bundle with the stacked-baseline error; engine tests reproduced the same
rejection. New optional row metadata validators in `general_data.rs`, WASM and TypeScript have
empty defaults for pre-G3 documents and do not tighten existing populated columns. The 2026-10-11
follow-up gate passed locked workspace tests (1,086 engine unit tests), warning-denying workspace
and wasm32 Clippy, formatting, fresh browser/package builds, package `npm ci`/lint/typecheck/pack
and public API/namespace/release guards, all six policy-v7 size checks, GPUI structural
parity/replay, strict native release performance (all targets), and 576 portable
Chromium/Firefox/WebKit tests (16 intended skips). The first browser run had one transient
`ERR_NO_BUFFER_SPACE` loading an icon module; the failed test passed on isolated rerun, then the
entire portable suite passed. Calibrated GPUI pixels and clean official dashboard upload
evidence remain pending as above.

The first independent G3 user-testing round at `68f336c` found two real public-path defects
despite those historical green gates: XY line/area ink and exact hits entered the general
X-axis strip after a narrowed Y view, and a manual resize left bar/axis pixels at the old
size while the layout and exact hits moved. The corrective feature now scopes general
marks, references, labels and interaction ink to the engine-resolved plot scissor in the
shared ordered frame; financial panes, underlay, chrome and axis labels keep their
original clips. A layout-only browser resize leaves coordinate-dependent retained
layers and axes invalid until the next full paint, even when the host installed the
size before eager layout. Engine frame/hit, GPUI clip-plan, native pixels and rebuilt
Canvas2D/WebGPU screenshots, pixels, exact hits and real bar clicks exercise the defects.
The public hit coordinate origins are documented and the `range_bar` browser case now
actually calls exact hit testing. Its complete workspace, WASM Clippy, package,
six-ceiling size, strict native performance and GPUI structural/replay gates passed;
the rebuilt portable Chromium/Firefox/WebKit suite passed 584 tests with 20
platform-intended skips. This correction does not reclassify the optional
machine-specific GPUI image failure or the non-official 100k upload diagnostic; a
separate G3 evidence follow-up addresses the remaining interpolation/gap/style/hit
fixtures identified in that same user-testing round. Deterministic frame fixtures now
compare line, area, and range-area upper paths and exact hits across all eight supported
interpolations on increasing, reversed, and duplicate X; coupled band probes cover
interior joins and the shared render expansion remains finite and bounded. Each path
family distinguishes queryable missing-row split/bridge from logarithmic hard gaps with
connection off/on; stacked-area add and update rejections leave frame and rows unchanged.
Scatter and enabled line markers exercise all four symbols, a single explicit-ID
color/symbol override, unaffected series-colored strokes and rows, symbol-edge hits,
and typed styled replacement. Heatmap fixtures probe cell edges, absent and clipped
exact targets, adjacent-row nearest identities through series reorder and category
view change. These fixtures extend `crates/aeris_charts_engine/src/frame/tests.rs` and
`crates/aeris_charts_render/src/line.rs` without a new audit harness or production
ownership change. The 2026-10-11 follow-up gate passed locked workspace tests,
formatting, warning-denying workspace/wasm32 Clippy, package install/lint/build/
typecheck/pack/API/namespace/release guards, all six policy-v7 artifact-size
ceilings, native frame/golden tests, GPUI structural parity/replay, strict native
performance, and 584 portable Chromium/Firefox/WebKit tests (20 platform-intended
skips) on rebuilt assets. One initial native performance run had a noisy unrelated
order-flow Target P p99 at 8.82 ms versus 8.00 ms; an immediate repeat passed
at 5.31 ms without code or threshold change. Optional calibrated GPUI pixels
and clean official browser startup/upload evidence remain separately pending.

The rebuilt-assets `general-dashboard-100k` local diagnostic on 2026-10-11 used dirty
`ba4723d` plus the G3 worktree, Chromium 153, 1280×720 at DPR 1, WebGPU, one warm-up and five
measured runs. The command was
`AERIS_CHARTS_BENCH_PORT=4174 node benchmarks/benchmark.mjs scenario general-dashboard-100k`.
It recorded p50 startup 726.425 ms, first-frame CPU 92.59 ms, WASM
linear memory 406,847,488 bytes and p50 uploaded vertex bytes 137,585,528. The local
scenario process exited 1 because upload exceeds the unchanged 100,663,296-byte policy-v7
ceiling; its attempt to spawn another server on the occupied shared port also logged
`EADDRINUSE`, although the scenario ran against the existing server and recorded five samples.
This dirty/local run is diagnostic, **not an official budget pass**. Clean official-runner
startup/upload evidence and any needed remediation remain due at phase closure/G8. GPU timestamp
and photon-visible presentation were unsupported; repeat-frame/disposal metrics were not
measured by this scenario. Strict native line/mixed/error targets passed independently.

G4 adds package-owned default legends, tooltips, and brush controls to the existing general
series, axes, titles, labels, and references. The dashboard uses these public components rather
than maintaining its own legend and tooltip semantics. Legend actions retain series handles and
focus through visibility and reorder changes; the tooltip uses engine hits and bounded shared
snapshots. The engine input controller owns mouse and touch brush drags, cancellation, keyboard
selection, and semantic endpoint steps. Linked general charts exchange source/revision guarded
axis views and crosshair values; category identities and axis IDs allow unequal datasets and pane
orders without feedback loops. General crosshair rules rendered from the shared frame (superseded
by `867029a`, which removes general crosshair ink; see the post-G7 corrections below), and a
native regression checks general image export against the live frame and view restoration. Public
browser tests cover localization, overflow, focus, keyboard and touch controls, synchronization,
and resize. The portable Chromium/Firefox/WebKit gate passed 603 tests with 22 intended skips;
locked workspace tests, formatting, warning-denying workspace/wasm32 Clippy, package
install/lint/build/typecheck/pack, public API/namespace/release guards, and all six policy-v7
package-size thresholds passed on 2026-10-11. Structural GPUI parity/replay and strict native
release performance passed. Calibrated GPUI window pixels and an assistive-technology session
remain separate manual evidence.

G5 completes declarative and framework-neutral authoring over the same atomic pane transaction
introduced in `e3dd895` and `0dda695`. Keyed axes, canonical and mapped series, references,
labels, legends, tooltips, and controlled or uncontrolled brush controls retain engine identity
across ordinary updates. A general-only React chart can claim its explicit first pane without
allocating an empty financial pane; cleanup leaves that pane available through Strict Mode and
conditional remounts. The React reconciliation effect no longer repeats an unchanged composition
after its mount-state rerender. Browser fixtures cover invalid proposals, rollback, callbacks,
concurrent hydration, pane cleanup, first-pane ownership, and imperative parity. The installed
npm tarball bundles standalone, composed, linked, and mixed React consumer examples; the React
entry imports during SSR without creating a chart. The 2026-10-13 batch gate passed formatting,
locked workspace tests, warning-denying workspace/wasm32 Clippy, package install/lint/build/
typecheck/pack/SSR/API/namespace/release guards, all six policy-v7 size limits, strict native
release performance, and 684 portable Chromium/Firefox/WebKit tests with 22 intended skips on
fresh assets. An earlier combined browser run hit Firefox context-teardown errors; the complete
Firefox project and then the complete combined suite passed unchanged. Earlier native Target P
and Q timing outliers also passed on the final strict run without threshold changes. This batch
does not claim a manual assistive-technology session or calibrated GPUI window pixels.

G6 delivered all four polar series families through the shared frame and public browser package,
with deterministic sector/polygon geometry, exact hits, legends, labels, selection, keyboard input,
and persistence. One bounded engine clock now animates polar and Cartesian value, position, and
identity changes, including interruption and reduced motion. Shared frame fixtures, browser tests,
GPUI structural and 1.25× window pixels, and SwiftShader/Canvas2D pixel tests cover the affected
executors. On 2026-10-13, locked Rust and WASM checks, package checks, public API/namespace/release
guards, SSR/pack checks, strict native performance, all six policy-v7 size limits, and the portable
Chromium/Firefox/WebKit suite passed (735 tests, 26 intended skips). The strict native run measured
pie entry/exit plus frame p95 at 1.34 ms against 4 ms and 4,096-category reorder plus frame p95 at
9.02 ms against 12 ms. The real GPUI window capture at 1.25× matched four polar interiors within
one channel; 2,672 of 225,000 edge pixels differed, with maximum channel delta 101 under the
configured 2%/128 limits. Manual assistive-technology review, competitor screenshot comparison,
and further device-scale/platform window captures remain evidence work for parity closure.

G7 delivers funnel, treemap, Sankey and sunburst as dedicated typed layout families in the shared
engine. Each has validated bounded input, deterministic frame geometry and exact hits, live
presentation handles, V2 persistence, opt-in labels, pointer and keyboard selection, bounded
accessibility snapshots, built-in tooltip and legend behavior, and compatible fixed-clock value
transitions. Browser fixtures exercise public handles, shared datasets, restore, focus, animation
and Canvas2D/WebGPU interior pixels. GPUI/Canvas2D structural focus and mark parity passed at
DPR 1, 1.5 and 2. The 2026-10-18 batch gate passed formatting, warning-denying workspace and
wasm32 Clippy, workspace tests, package install/lint/build/typecheck/pack, SSR and public API/
namespace/release guards, 764 portable Chromium/Firefox/WebKit tests with 28 intended skips,
36 GPUI parity tests, 7 GPUI replay tests, the isolated strict native release performance gate,
and all six policy-v7 package-size thresholds. Manual assistive-technology review, competitor
screenshot comparison and calibrated GPUI window pixels remain for G8 parity closure.

Two post-G7 commits on 2026-10-18 change general presentation without closing an R8 checklist
item. `fb6ea5e` adds the `gpui_general_gallery` example: 20 live, scrollable GPUI charts covering
all 15 general series kinds, a donut variant and the four layout families through the shared frame
and the GPUI input adapter. `867029a` aligns general charts with Recharts presentation defaults.
General panes paint no crosshair rules or axis tags; hover stays engine state for tooltips,
promotion and sync, and GPUI hosts release the pointer on hover exit. Financial price and time
strips are reserved only when a financial-time pane exists, so general-only charts lose their empty
gutters. Automatic linear and radial domains include zero and round outward to a 1/2/5 step.
Vertical category axes list the first category at the top. Plot sides without an axis keep a
margin, and the plot scissor extends 6 px so edge markers stay whole. Polar angle labels sit
outside the ring. Its gate passed formatting, warning-denying workspace and wasm32 Clippy,
workspace tests, GPUI crate tests with `gpui-backend`, the native performance gate (an earlier
noisy run was re-measured against a HEAD baseline), package install/lint/build/typecheck/pack, and
770 Playwright tests after two general specs were updated for the new defaults. The verified G2
scale/axis and responsive-layout evidence predates these default changes, so G8 must recheck those
rows. The gallery is a manual inspection tool, not calibrated GPUI window evidence.

A later post-G7 change makes general hover visible in every host without closing an R8 item.
Cartesian panes with an index-axis series use Recharts' axis hover: the nearest X datum or
category band anywhere in the plot, a crosshair-styled cursor line (horizontal for horizontal
bars), active dots on line and area series, and an engine-drawn tooltip in the financial tooltip
style with series color markers (single-tone with bar markers since `d2ccc8d`). Other panes keep exact item hover with the same tooltip, and layout
panes describe the hovered stage, node or link. Click selection still needs a mark under the
pointer. The tooltip is ordered-frame content, so GPUI and the browser paint it identically;
`generalTooltip.visible` (default `true`) hides it, and the DOM `create_general_tooltip` now uses
the same structure and colors and hides the engine tooltip while attached. The gallery's header
caption hack was removed. This supersedes the earlier "no crosshair rules" note for the index-axis
cursor only; general panes still paint no horizontal crosshair rule or axis tags.

A follow-up post-G7 presentation fix, also without closing an R8 item, makes general axis lines
one whole device pixel at every DPR, drops grid and zero rules that would double an axis line, and
snaps the GPUI element origin to device pixels so scrolled charts keep crisp rules. The GPUI gallery
and browser dashboard drop card headers and enable grids, and the browser dashboard keeps charts
mounted after they scroll out of view instead of re-creating them blank.

A further post-G7 fix (`d2ccc8d`), also without closing an R8 item, removes browser blur from
1px rules. The wasm shell sizes each canvas CSS box to exactly bitmap / devicePixelRatio and
translates it by the fractional part of the container's composited device position, including
Chromium's whole-device-pixel snapping of scroll-container content. The package re-checks that
placement before each repaint and when an ancestor resizes. Pointer input stays container-relative,
and WebKit keeps the canvases on the container box. Chromium pixel scans at DPR 1, 1.25, 1.5, 1.75
and 2, Canvas2D and WebGPU, at rest, scrolled and after filtering, show every axis rule as one
device pixel (two at DPR 2). The same change makes the general tooltip one surface tone with no
tinted header and a vertical color bar per row, in the engine frame and `create_general_tooltip`.
It gives `create_general_legend` borderless compact buttons with vertical bar markers, and removes
axis titles from the dashboard demo.

A post-G7 general presentation batch, also without closing an R8 item, sizes the general Y-axis
strip to its measured labels and title, replaces square legend dashes with one rounded pill marker
shared by the engine legend, tooltip and DOM components, and adds an engine-drawn legend
(`generalLegend.visible`, off by default) that both GPUI and the browser paint from the frame.
`histogram` gains engine binning (automatic, bin count or bin width; count, percent, probability or
density; optional cumulative). Automatic linear, log and symlog domains of point and path marks get
a 4% margin so marks never sit on an axis line; point category axes default to half a step of outer
padding; axes accept `line_visible`. Unstacked bars round only their value end, column and bar data
labels stay on their own bar, and symlog axes tick clean signed decades. Treemaps inset nested
groups under a header, keep a 1 px cell gap and shade leaves by a signed `color_value`. The GPUI
gallery and browser dashboard render one shared 46-example catalog
(`examples/web_demo/general_catalog.json`) covering every series kind and mode, including bull/bear
grouped columns, three axes, hidden axis lines and three histogram modes. The general Y strip's
grow-only negotiation ignores the empty-axis placeholder, so layout no longer depends on whether a
chart was sized before or after its data. Its gate passed formatting, warning-denying workspace
and wasm32 Clippy, workspace tests, GPUI crate tests with `gpui-backend`, the native performance
gate, package install/lint/build/typecheck/pack/API checks and the Playwright suite. The catalog
cards were reviewed in headless Chromium screenshots; calibrated GPUI window pixels remain G8 work.

The first G8 pinned-default check adds a Recharts 3.10.1 horizontal-bar reference to the
versioned fixture and a matching Aeris general-only chart through the public browser API.
It checks zero-based bars, first-category-at-top order, visible pixels, exact hits and the
single-pane height. All six paired cases pass in Chromium, Firefox and WebKit. Fifteen
additional public browser cases for numeric and typed axes, multi-axis font/DPR/aspect
changes, hidden-container reveal and resize-order pixel/hit stability pass on the same
three engines. These focused checks recheck specific G2 defaults after `867029a`;
they do not establish the complete scales/axes or responsive-layout matrix rows, nor
manual screenshot parity.

A pinned Recharts `XAxis angle={-60} height="auto"` case now verifies four
rotated long labels and an enlarged strip in all three browsers. The paired
Aeris `tick_angle: -60` case verifies measured plot displacement, V2 round trip
and image-identical restore through the public API; engine and GPUI/Canvas2D
frame tests cover rotated primitive ordering and a small pane that keeps its
axis line/title when a long label cannot fit. A pinned custom Recharts X tick paints
two SVG lines per category; the Aeris public pair verifies newline labels, automatic
X strip growth, rotated multiline layout, Y strip width growth, exact row hits,
and image-identical V2 restore in Chromium, Firefox and WebKit. The engine caps
each tick at four measured and painted lines, and normalizes CRLF for paint while
V2 retains the input labels. A Chromium
presented-frame fixture counts multiline X/Y tick ink through Canvas2D and WebGPU
with bounded ink-count differences. Calibrated pixels and complete
constrained-layout evidence remain open.

A second pinned pair checks the Recharts 3.10.1 positive stacked-bar example against a
general-only Aeris column stack. It verifies category order, aligned segment widths,
contiguous stack boundaries, relative segment heights and Aeris exported blue/teal pixels.
Recharts SVG path coordinates are sampled after the segments become stable; Aeris uses
engine hits for geometry and the public image export for color. All six paired browser
cases pass in Chromium, Firefox and WebKit. Alternate offsets and the complete bars/stacks
visual row remain open.

The same pinned row now includes a Recharts `stackOffset="sign"` fixture with mixed signs
and both-positive/both-negative categories. Aeris `normal` column stacks match its zero
separation and segment adjacency through public hits and exported colors in Chromium,
Firefox and WebKit (six paired cases pass). The explicit offset is part of the mapping;
the complete visual/interaction row remains open.

An explicit pinned Recharts `stackOffset="expand"` example and an Aeris `percent` column
stack now compare four positive-total categories. Each pair verifies a common normalized
height, contiguous segments, expected fractional shares, aligned category widths and
exported Aeris colors. All six paired cases pass in Chromium, Firefox and WebKit.
Mixed-sign expansion is compared below.

The pinned `stackOffset="positive"` example and Aeris `stack_mode: "positive"` now
share the same visible A/B/D segments while suppressing negatives. Aeris still exposes
the negative source values through its data API. Recharts SVG geometry and Aeris public
hits/exported pixels pass as a paired fixture in Chromium, Firefox and WebKit.
The remaining offset comparisons are recorded below.

The pinned `stackOffset="none"` mixed-sign fixture and Aeris `cumulative` bars
have matching series-order overlap and visible blue/teal segments in all three
browsers. Positive-only `silhouette` and `wiggle` pairs have matching per-category
segment proportions and baseline displacement, normalized for the different
chart axis scales; the silhouette stack centers at zero. Six cumulative cases
and three combined silhouette/wiggle cases pass across Chromium, Firefox and
WebKit.

The pinned mixed-sign `stackOffset="expand"` fixture exposes a material
reference failure: its automatic axis clips the A/B base bars and omits their
top segments. Aeris `percent` keeps all eight segments visible by normalizing
positive and negative totals independently. Both behaviors pass public-path
fixtures in Chromium, Firefox and WebKit. Copying the clipped reference
behavior is not good for Aeris because users would lose visible data; keep the
independent-sign policy and document it as a deliberate migration difference.
Maintainer review of that product difference and the complete bars/stacks
visual and interaction row remain open.

Pinned Recharts 3.10.1 `XAxis height="auto"` reserves more space than a fixed
30 CSS-pixel strip for large ticks and an axis title. Aeris grows its horizontal
strip for a title and increased chart font size; an engine layout regression and
public exported-pixel checks pass in Chromium, Firefox and WebKit. Rotated and
multiline labels are covered by the separate pinned and Aeris pair above; the full
scales/responsive parity recheck is still open.

Pinned Recharts `ResponsiveContainer` and an Aeris general-only auto-size chart now
share a 700×350 to 420×210 resize followed by a processed hidden state and
480×260 reveal. The Aeris chart retains one pane and exact general-series hits
after reveal. The paired public browser fixtures pass in Chromium, Firefox and
WebKit. Additional responsive evidence is recorded below.
The pinned `ResponsiveContainer aspect={2}` also follows a width-only host from
600×300 to 360×180 in Chromium, Firefox and WebKit. The Aeris public-path
aspect resize/DPR/font alignment and hidden/reveal fixtures pass six focused
cases on the same engines. A separate Aeris zero-size host grows through 96×64
to 360×220 with finite small-plot geometry and exact hits on all three engines.
An additional device-scale fixture verifies 260×160 to 320×220 CSS resize,
device-pixel canvas dimensions and exact category hits at DPR 2 in Chromium
and WebKit; headless Firefox reports effective DPR 1 and passes at that scale.
A Chromium CDP fixture changes DPR on a live chart from the configured 1.5 to
2 and back, sends the platform orientation signal because CDP does not deliver
the resolution media-query change on this adapter, and verifies canvas dimensions
and exact hits after both transitions.
A current public G2 recheck of horizontal-bar defaults, linear/category/explicit
and reversed axes, resize/reveal/aspect/zero-size and DPR paths passed 34 cases
across Chromium, Firefox and WebKit, with two expected CDP-only skips.
The engine general-only chrome/domain/hover and automatic numeric-domain
regressions also pass. One earlier WebKit zero-size run reported a page error;
the same case passed ten isolated repeats and three ordered layout-suite repeats,
so its intermittent cause remains unconfirmed for the final gate.
This is focused evidence; manual viewport screenshots and the complete
responsive-layout policy still need phase closure.

The migration guide's fluid-width React example now sets the required
`autoSize: true`. A Strict Mode React fixture confirms that the general-first
chart follows 700×350 to 420×210 resize, retains its last size when hidden,
then reveals at 480×260 without changing its pane or series identity or losing
exact hits. The real React adapter test passes in Chromium, Firefox and WebKit.
The packed React recipe already used the correct option. Complete React
authoring and responsive visual comparison remain open.

The pinned Recharts reference fixture now checks that a February-to-March
ReferenceArea spans the plot, sits behind the area series, and that the Y=16
threshold line and four numeric labels remain visible. The paired Aeris
general-only public-path fixture confirms distinct exported pixels when a
region, line and data labels are applied, plus an exact series hit. The migration
guide records that Aeris regions map omitted X/Y endpoints to plot edges.
Twelve paired browser cases cover full-height, full-width and one-sided bands,
zoom/resize, V2 persistence, exported pixels and exact hits. A Chromium
Canvas2D/WebGPU presented-frame fixture confirms matching reference interiors
and byte-identical exports. Engine frame order keeps regions below series
and dots above; Canvas2D/GPUI structural parity retains all three region
shapes across three DPR values. Calibrated overlap and accessibility review remain open.

The pinned Recharts `type="step"` fixture paints two path segments around a null
unless `connectNulls` bridges it. Its Aeris public pair uses `step_middle` and
`connect_missing`, changes exported pixels, and keeps the missing row in V2
validity state. Six paired cases pass across Chromium, Firefox and WebKit.
The migration guide records the exact step and gap mapping.

The pinned range `Area` fixture paints one band with two boundaries. Its
Aeris `range_area` pair exports band pixels and retains four ordered low/high
rows and V2 kind. Six paired cases pass in Chromium, Firefox and WebKit.

The pinned Recharts `ErrorBar` fixture paints four capped Y whiskers;
its Aeris public pair exports four absolute low/high intervals and accessible
rows. Six paired cases pass in Chromium, Firefox and WebKit. The migration
guide maps relative Recharts margins to absolute Aeris bounds.

The pinned Recharts `ZAxis` bubble fixture and an Aeris public bubble chart
increase symbol/pixel area in the same four-value size order. Aeris also retains
the sizes and family in V2. Six paired cases pass in three browsers.

A pinned square `Scatter` fixture paints four equal-sided symbols. The Aeris
public pair changes exported pixels from round to square marks and preserves
the symbol in V2. Six paired cases pass across three browsers.

The pinned Recharts Cell fixture now uses deterministic, animation-disabled
bars and verifies four per-row fills and 6px rounded positive tops. The Aeris
public-path pair verifies the same four persisted row colors, exported interior
pixels, top corner cutouts and filled lower corners. Six paired cases pass in
Chromium, Firefox and WebKit. Aeris rounds the exposed lower end for negative
bars; this portable rule and wider customization/tooltip formatting still need
complete visual and manual comparison.

The pinned Recharts animation reference can now update its four-row line after
mount. Chromium, Firefox and WebKit observe distinct starting, intermediate and
settled SVG paths across its 400 ms transition. A matching Aeris public chart
uses the same values and duration; fixed-clock Canvas2D captures in all three
browsers differ at start, intermediate and settled times, stop scheduling at
rest and persist the target values. Under a browser reduced-motion request,
the pinned Recharts line still paints an intermediate path, while a matching
Aeris public line applies its updated values immediately and schedules no
transition in all three browsers. This is a deliberate accessibility behavior
difference. A Chromium presented-frame fixture freezes a public line update at
start, 160 ms and rest; Canvas2D and WebGPU move the line through the same pixel
centers with bounded painted counts. Exact easing, moving GPUI pixels and other
family executor comparisons remain open.

The pinned accessibility/localization fixture now uses numeric values large
enough to show French digit grouping and disables line animation for stable
geometry. Its focusable application surface exposes all five explicit Y ticks.
Aeris receives the same host-formatted strings as portable explicit labels;
the public-path fixture confirms changed export pixels, persisted labels and
unchanged exact series hits. Six cases pass across Chromium, Firefox and
WebKit. The migration guide explains that hosts update explicit ticks when
their domain changes. Manual assistive-technology and contrast review remain
open; these tests do not establish screen-reader parity.

A pinned Recharts 3.10.1 Sankey fixture confirms the default focusable SVG surface,
`application` role, and chart title/description in Chromium, Firefox and WebKit.
The matching Aeris public-path fixture confirms a focusable `application` layer,
bounded node/link values and keyboard announcements in the same engines. This
covers the new Sankey metadata check, while manual assistive-technology and
complete accessibility parity remain open.

The pinned hierarchy/flow reference renders four funnel stages, four treemap
leaves, three Sankey nodes with two links, and four sunburst sectors in all
three browsers. This records the G8 reference mark inventory against the
existing Aeris public family, engine-frame and GPUI fixtures. A paired Sankey
fixture confirms the pinned 18:11 SVG link-width ratio and that Aeris public
link hit regions rank 18 above 11 in Chromium, Firefox and WebKit. A paired
sunburst fixture confirms all four sector area shares track 12/14/18/24
within five percentage points in the same browsers. A pinned animation-free
funnel fixture and Aeris exact stage hits each track the four trapezoid
area shares within five percentage points. Other direct
geometry and manual visual comparison remain open.

A pinned declarative Recharts fixture now confirms its X/Y axes, two lines, two
legend entries and Jan tooltip in Chromium, Firefox and WebKit. Matching Aeris
React first-pane ownership through Strict Mode/remount, references and default
controls pass in all three browsers. Complete visual and authoring comparison
remains open.

A pinned Recharts `Brush` traveller drag narrows its selected slide in Chromium,
Firefox and WebKit. Aeris' existing public brush fixtures cover chart-routed
pointer/touch selection, accessible keyboard controls, semantic ranges and
linked no-echo sync. A pinned `syncId` hover now shows Feb in both Recharts
tooltips in Chromium, Firefox and WebKit; Aeris' public point-crosshair fixture
routes the same category identity across unequal category lists without echo. A full
paired interaction review remains open because Recharts' SVG traveller strip and
Aeris' engine selection/DOM controls have
different presentation paths.

The pinned line/area/scatter reference paints two line paths, one area fill and
stroke, and four scatter symbols. A paired Aeris chart keeps the same four rows
in one category line/area pane and one numeric scatter pane, with all four exact
row hits for each series and matching V2 values. The six cases pass across
Chromium, Firefox and WebKit. Range/interpolation/marker variants and calibrated
visual comparison remain open.

The pinned Recharts 3.10.1 package exports no first-class box-plot or heatmap
component. Its G1 Scatter shape and Treemap color examples are analogues.
A pinned three-browser fixture verifies four numeric Scatter points in source-value
order and four blue Treemap leaves with positive area; those marks are not claimed
as geometric equivalents of the Aeris families.
Aeris' typed box-plot and heatmap public fixtures already verify ordered data,
colors, atomic mutation, V2 restore, accessibility and exact hits. Their
presented Canvas2D/WebGPU frames have matching flat interiors; a Chromium
SwiftShader comparison found zero pixels beyond one channel of difference
for both families. GPUI/Canvas2D structural frame fixtures also pass.
Calibrated visual and legend review remains open.

The pinned `ComposedChart` paints area, four bars and line in that order over
the same rows. A paired Aeris first-pane chart retains area/column/line order,
matching V2 values, a visible export and exact hits for all three layers.
Six cases pass across Chromium, Firefox and WebKit; broader overlay and visual
comparisons remain open.

A Chromium public-path fixture now forces shared WebGPU device loss while financial and
general charts are live. Both report Canvas2D fallback; the financial series retains its
row count, accepts a tip replacement and exports a nonempty image, while the general pane
and series retain their identities, accept a later data update, export the new row and keep
visible pixels and exact hits. This is focused device-loss evidence, not the complete export/recovery
matrix across families and executors.

A second Chromium public-path fixture forces the same loss on one two-pane chart containing
financial candles and general scatter points. The chart falls back to Canvas2D without changing
pane indices or row counts; both series accept updates, its exported image changes, the general
marks appear in the general pane, its hit resolves the new row, and V2 state contains that row.
The exported screenshot is byte-identical immediately before and after loss, before either
dataset changes; removal clears the fallback host's canvases. The focused combined loss test passes. Family-wide export and recovery
evidence remains open.

A third Chromium public-path fixture puts a polar pie and layout funnel in one
WebGPU chart, forces device loss, then replaces both datasets. Canvas2D fallback
retains both panes, exact hits for the new slice and stage, V2 exported rows and
a changed screenshot. Its pre-update exported screenshot is also byte-identical
across WebGPU loss. This broadens shared-frame recovery evidence to polar and
layout families; removal clears its fallback host canvases. Native export and
complete family coverage were checked separately below.

A fourth Chromium fixture combines first-class box plots and heatmap cells in
one WebGPU chart. Its screenshot remains byte-identical after device loss and
Canvas2D fallback; both series then accept new rows with exact hits, exported
V2 data and a changed screenshot, and removal clears the host canvases.

The pinned Recharts host-state reference now has a three-browser interaction
fixture: reversing its source rows changes the SVG line path, and its explicit
serialization button returns the reordered values. This establishes that
the comparison's persistence belongs to its React host.

A three-browser public-path fixture exports one chart with financial candles,
Cartesian columns, a polar pie and a layout funnel, then restores its V2 state
into a fresh chart. After the host supplies the same financial OHLC rows, state,
pane and series order, general-family exact hits, row counts and post-restore
updates pass in Chromium, Firefox and WebKit; both source and restored hosts
have no chart canvases after removal. V2 does not serialize the current
viewport, so this fixture verifies visible exports rather than byte-identical
screenshots; every restored pane has colored marks. A native executor test also renders a mixed financial/Cartesian/polar/layout
chart, confirms image export matches the settled live native frame byte for
byte, and confirms export leaves the live frame unchanged. A public catalog audit now
installs all 46 examples covering 20 general, polar and layout kinds, then
compares each image export with its own empty frame and checks canvas disposal.
All 46 pass on Canvas2D in Chromium, Firefox and WebKit and on WebGPU in
Chromium, with no page errors; the two non-Chromium WebGPU cases skip by
design. This establishes catalog-wide export and ordinary disposal, while
per-family update/loss and calibrated image equivalence remain open.
The 46-example V2 image-restore assertion now compares byte-identical exports
after a real size cycle normalizes the Canvas backing surface on source and
restored charts. It passes Canvas2D in Chromium, Firefox and WebKit and WebGPU
in Chromium (four passes, two intended adapter skips). Without that surface
cycle, a semi-transparent gradient after four prior WebGPU charts produced
visually equivalent images with differing raster values despite identical V2
state and pane geometry; the fresh-surface assertion retains exact pixel
comparison without weakening the check.

The current package build passed `npm run test:pack`: it packed the publish artifact,
installed it into an empty consumer directory, imported the package entry, checked its
JavaScript, declarations, WASM, CSS and license, and bundled the standalone Cartesian,
standalone polar, standalone flow, mixed, linked and React consumer recipes from that installed tarball
(25 files; 4,855 KiB WASM on the focused 2026-10-20 run).
The final G8 gate will repeat this check against the final batch build.

The authoring guide now gives a concrete Recharts legend/tooltip formatter migration and
explains which labels and components enter a portable frame export versus stay in the host DOM.
This advances documentation and customization guidance; the broader documentation checklist
remains open until every matrix row has a reviewed migration path and its references resolve.
The guide also now maps Recharts pie/donut data and `Cell` colors to the actual polar
first-pane API and provides a packed-consumer donut recipe. Its earlier polar option and
row examples used `angle_axis_id`/`radius_axis_id`, `category`/`value`, and a nonexistent
`missing` option; these now match the published `x_axis_id`/`y_axis_id` and `x`/`y`
contract, with a donut correctly represented as a `pie` with `inner_radius > 0`.
The pinned Recharts migration fixture also confirms in Chromium, Firefox and
WebKit that its root import and host toggle replace a standalone line with one
composed line and four bars. The packed-consumer smoke and public pie/donut plus
slice-legend browser fixtures pass six cases across Chromium, Firefox and WebKit.
The guide also maps pinned line/area/scatter source rows to Aeris XY rows and distinguishes
Recharts custom box/heatmap analogues from Aeris typed `box_plot` and
`heatmap_grid` rows. Accessibility title/description, atomic V2 restore,
image export and automatic backend fallback now have a migration path. The guide still
needs a row-by-row review before G8 closes.

The pinned Recharts 3.10.1 donut has four `Cell` colors in source order and
sector path lengths ranked by value. A paired
Aeris first-pane donut with the same four values and colors retains them in V2,
exports a visible frame, and exposes all four exact slice hits with hit area
ordering matching their values. The six paired browser cases pass in Chromium,
Firefox and WebKit. Radar/radial-bar paired geometry and calibrated visual
review remain open. A separate pinned radar/radial-bar fixture confirms one
polygon and four radial-bar sectors in all three browsers. Aeris already has
four-row public hit fixtures for those families. A matching two-pane public
fixture now also persists the pinned four values and exposes all four row hits
for both families in Chromium, Firefox and WebKit; direct paired geometry and
manual visual review remain open.
A pinned/Aeris treemap pair now ranks four leaf rectangle areas and exact hit
regions in the same 12/14/18/24 value order across Chromium, Firefox and
WebKit. Each normalized area share is within five percentage points of its
input share. Calibrated dimensions and other layout families remain open.

The new axis-free Sankey recipe maps Recharts numeric node indexes to stable Aeris node IDs
and bundles through the installed tarball. The six-consumer pack smoke passes; public
layout-family hit/restore and Sankey accessibility fixtures pass nine cases across
Chromium, Firefox and WebKit. Complete hierarchy/flow visual and manual parity remains open.

The G8 browser benchmark now distinguishes a general-only 100K-row workload (five series in
one pane) from a 120K-row mixed workload (the same general series plus 20K candles in a
separate financial pane). The prior general scenario silently included the default empty
financial pane. Its version was advanced and a mixed scenario was added. Upload counters
now capture every synchronous public API construction/data-install repaint; the policy v7
upload metric is the largest single repaint, with cumulative upload volume reported separately.
The final unchanged repaint had previously made that metric read zero. Dataset metadata now
records the actual series, pane, row count and data type.

Local Windows/Chromium/WebGPU release diagnostics on 2026-10-19, after one warm-up and five
measured runs, passed both scenarios. After the shared `circle_segments` minimum fell from
24 to 8 while retaining the 0.1-device-pixel chord-error bound, p50 general-only startup
was 204.13 ms, last data-install frame CPU 40.32 ms, peak repaint upload 46,682,112 bytes,
and cumulative upload 84,627,368 bytes. The mixed workload measured 319.52 ms, 40.60 ms,
46,759,696 bytes and 85,376,056 bytes respectively. The same machine's before-change
observations were 242.50/64.40 ms and 76,634,112 peak bytes for general-only, and
394.96/64.62 ms and 76,711,696 peak bytes for mixed. These ignored local raw results are
engineering diagnostics, not controlled-runner claims. In particular the roughly 40 ms
dense data-install frames do not establish a 16 ms frame target; p95 input/frame, steady-state
allocation, retained CPU/GPU memory, cold startup and repeated-disposal budgets remain open.
Focused shared-render tessellation, WebGPU/Canvas2D circle and general-point parity, GPUI
scatter route, and benchmark harness tests passed. The final G8 batch must rerun all gates.

The native release performance harness now constructs Targets F (100K XY line), G (five-series
general dashboard), and I (100K numeric error bars) with one continuous-domain pane. Before the
correction, a new topology assertion failed at F because the general pane had been appended
beside an empty financial pane. The corrected F/G/I fixtures assert one pane and no financial
series; the separate H fixture still measures a financial and general pane together. The
2026-10-20 release run passed all targets. F measured 1.54 ms frame and 1.49 ms nearest hit;
G measured 1.32 ms frame, 1.95 ms hit, and 5.53 MiB retained; I measured 9.77 ms frame,
2.58 ms hit, and 6.88 MiB retained. These are local diagnostics; the final G8 batch gate
will repeat the run with the completed changes.

The current package's financial-only `historical-candlestick-100k` browser scenario
passed locally on 2026-10-20 with one candlestick series, one pane and WebGPU.
After one warm-up and five measured installs, p50 public `set_data_typed` time was
5.76 ms, p50 frame CPU was 1.59 ms and reserved WASM linear memory was
17,629,184 bytes. Its raw result is ignored local diagnostic output under
`benchmarks/results/v0.13.2/local/`. The general-only and combined scenario
measurements above use the same public benchmark harness, but a controlled
final-build run remains required for a release performance claim.

The engine-drawn general legend now accepts top/bottom lanes plus `center`, `insideTop`,
`insideBottom`, and four inset corners, with a bounded nonnegative CSS-pixel `offset`.
Top/bottom reserve plot height; inset entries overlay the measured plot. The same ordered
frame and engine click target use those coordinates, with no browser host routing. A top
regression failed against the prior bottom-only engine and then passed; a large-offset
regression confirms entries stay visible. The focused engine suite passes eight cases,
including all seven inset positions; warning-denying core/engine Clippy and package
typecheck pass. Real top and inset top-left pointer tests pass in Chromium, Firefox and
WebKit.

Outside left/right legends now reserve a bounded side gutter after the financial and
general axes, stack measured entries vertically, elide long Unicode titles, and fold
overflow into `+N more`. They paint in the shared unscissored axis layer; the engine
input controller promotes their presses ahead of axis scaling. Inside left/right use
the same vertical layout over the plot without reserving width. The native host-path
regression checks gutter width, axis-layer paint, pane-frame exclusion and pointer
toggle for both outside sides; a separate test checks inside-side width and bounded
Unicode/overflow behavior. Real outside-side clicks pass in Chromium, Firefox and
WebKit. A focused Chromium Canvas2D/WebGPU screenshot fixture verifies side-gutter
legend pill pixels from each executor. Custom `{x,y}` placement anchors the lower-right
corner using the full chart width and the target pane's height. It ignores
`offset`, reserves no plot space, and paints after axes in the unscissored layer.
Engine coordinate/hit and serde regressions, pinned Recharts placement,
Chromium/Firefox/WebKit in-plot, axis-strip and cross-pane clicks, and
Canvas2D/WebGPU image-export pixels pass. A real engine-frame custom legend in the
unscissored axis layer now has identical Canvas2D and GPUI quad streams and text anchors
at 1×, 1.5× and 2× DPR; calibrated GPUI window pixels and broader export parity remain open.
The engine input controller now gives touch presses a 44 CSS-pixel legend target while mouse
hits retain their precision. Overlapping touch bounds select the nearest painted entry, and
the `+N more` summary stays inert. A direct engine touch regression failed before this change.
The browser adapter also stops treating Pointer Events capture release as a Touch Events cancel;
Firefox and WebKit had cancelled a tap before `touchend` when both event IDs matched.
Fourteen focused legend tests, warning-denying engine Clippy, and real touchscreen taps through
the public browser host in Chromium, Firefox and WebKit pass. Wider touch/keyboard parity remains open.

## Known gaps

Remaining source-confirmed gaps from the 2026-09-23 review and the 2026-10-18 post-G7 changes. R3–R7 closed the earlier Cartesian,
component, authoring, polar, hierarchy and flow gaps. Full parity evidence remains open.
Earlier test counts and timings are historical observations in Git history, not proof of current completion.

| Gap | Evidence | Required correction | Phase |
| --- | --- | --- | --- |
| Parity evidence | Automated layout-family contracts and cross-backend fixtures pass; manual assistive-technology, competitor screenshot and calibrated GPUI window evidence are still due | Collect phase-closure evidence and record any differences against the pinned competitor | R8 |
| G2 evidence after default changes | `867029a` changed automatic domains, vertical category order, general-only gutters and plot clipping after the G2 rows were verified; 34 current public browser cases and focused engine regressions pass, with two expected CDP-only skips | Complete calibrated viewport/theme screenshots and investigate any recurrence of the one WebKit zero-size page error in the final gate | R8 |
| Pinned legend placement | Recharts 3.10.0 added legend `position`/`offset`; Aeris has shared top/bottom lanes, outside side gutters, inside side and center/corner insets with bounded offsets and engine hit coordinates; custom-coordinate placement now has chart-width/pane-height pixel or percent coordinates, lower-right anchoring, engine hits, Chromium/Firefox/WebKit canvas input, Canvas2D/WebGPU image-export pixels and GPUI/Canvas2D axis-layer stream parity at 1×, 1.5× and 2× DPR; calibrated GPUI window pixels remain open | Complete the legend/tooltip parity row and calibrated GPUI image evidence | R8 |

These are not an exhaustive defect audit. Coverage-matrix rows are required coverage to audit, not
assertions that every listed feature is absent.

## Phases

All phases are open unless marked otherwise in **Status at a glance**. Existing implementation
counts toward a phase only after its required behavior is demonstrated. Each phase is delivered as
one batch (see **How work is delivered**); its last checklist item is the batch gate, commit and
push. Do not delay fixes until a large framework rewrite, and do not skip foundation work to add a
demo chart.

### R0 — Auditable competitive baseline

**Batch:** G1, together with R1. **Depends on:** nothing. **Status:** complete.

- [x] Pin the Recharts release and source revision; retain the existing financial competitor
      baseline.
- [x] Inventory public props and components; map every relevant capability to the coverage matrix
      with supported combinations.
- [x] Add reference fixtures for each matrix row and record intentional differences. Executable
      fixtures stay in existing test infrastructure; transient screenshots and reports stay out of
      committed documentation.
- [x] Reconcile `general_charts_api.md`, `public_api.md`, examples and architecture claims against
      exports, manifests, scripts and actual call paths. Separate supported, experimental and
      proposed behavior.
- [x] Capture clean release financial, general and combined baselines and the current enforced
      budgets.

**Exit:** every required capability has a scoped owner, dependency, fixture and honest status; no
unsupported example or historical PASS substitutes for current evidence. Version, date and evidence
paths accompany future status changes. Unverified rows stay open.

### R1 — Lifecycle and mutable object foundations

**Batch:** G1, together with R0. **Depends on:** R0. **Status:** complete.

- [x] Initial general-domain creation through the canonical constructor.
- [x] Last-pane ownership defined together with adapter ownership.
- [x] Atomic in-place axis and series mutations through all public boundaries.
- [x] Failed installation and cleanup fixed.
- [x] Visibility and ordering affect domains, legends, hits and exports consistently.
- [x] G1 corrective general-first V2 restore, declaration snapshot and user-surface/lifecycle gates
      green; corrective commits pushed.

**Exit:** standalone general and mixed charts can create, update, rebind, reorder, hide, remove and
restore through actual browser and native paths. Invalid operations leave prior state intact.
Handles, explicit row focus and selection, and unaffected views survive routine changes; repeated
mount and dispose release resources.

### R2 — Scale, axis and responsive layout contracts

**Batch:** G2. **Depends on:** R1. **Status:** complete.

- [x] Temporal ticks, formatting and view operations.
- [x] Category behavior, including zoom/pan and duplicate labels.
- [x] Explicit ticks and formatters.
- [x] Numeric extremes and degenerate domains.
- [x] Grid and zero lines executed in the frame.
- [x] Domain padding and clipping.
- [x] Multiple axes.
- [x] Axis titles.
- [x] Small-container behavior.
- [x] Every reviewed silent-option gap corrected.
- [x] G2 full gate green; batch committed and pushed.

**Exit:** deterministic domain and coordinate round trips and frame fixtures cover all supported
scale and orientation combinations. Browser resize, font, DPR, pointer and keyboard view tests agree
with native and GPUI output; no accepted option silently does nothing, and layout work has an
enforced bound.

### R3 — Cartesian visual and data semantics

**Batch:** G3 (all remaining items in one batch). **Depends on:** R1–R2 (complete).
**Status:** complete.

- [x] Line width (item 1).
- [x] Line dash styles (item 2).
- [x] Area baseline value (item 3).
- [x] Point markers and marker symbols (items 4–5).
- [x] Linear, step and curved interpolation (item 6).
- [x] Explicit `step_before`, `step_middle`, and `step_after` interpolation across shared line/band
      expansion, exact hits, V2 persistence, WASM, browser options, and GPUI path parity; legacy
      `step` retains its step-after shape.
- [x] Shape-preserving `monotone` interpolation on shared lines and coupled bands, with bounded
      samples, reversed/duplicate-X regression, exact hits, V2/browser round trips, and GPUI paths.
- [x] Natural cubic interpolation with zero endpoint curvature across shared line/band
      expansion, bounded sampling, exact hits, V2/browser options, and GPUI path parity.
- [x] Missing-value connection policy (item 7).
- [x] Area fill opacity (item 8).
- [x] Curve, gap, and baseline policies across applicable path families: `xy_line`, `xy_area`,
      and `range_area` share interpolation and missing-row run handling; `xy_area` resolves zero,
      explicit, and effective-domain baselines in shared geometry, while ranged and stacked areas
      retain their data-defined lower boundary. Existing frame and exact-hit regressions cover
      range gaps, bridging, and baseline choices.
- [x] Unstacked area baseline policy for zero, effective domain minimum, and effective domain
      maximum, with explicit numeric baseline preserved and stacked-area conflicts rejected.
- [x] Bars and stacks: both orientations, groups, sizing and gaps, corners, mixed signs, stack order
      and required offset modes.
- [x] Bounded bar corner radius for vertical and horizontal bars, including exposed stack ends,
      shared rounded-rectangle frames, exact hits, persistence, and browser options.
- [x] Stack order through the existing atomic general-series ordering API, now verified against
      stack geometry, exposed corners, browser ordering, and V2 restore.
- [x] Positive-only stack offset for both bar orientations and X-aligned areas, with negative
      source rows retained for queries and removed from stacked marks and automatic value domains.
- [x] Cumulative signed stack offset for both bar orientations and X-aligned areas, with every
      intermediate boundary included in automatic domains, exposed bar ends, V2 persistence,
      and browser options.
- [x] Silhouette stack offset centers signed totals for both bar orientations and X-aligned
      areas, retaining prefix-aware autoscale, exposed bar ends, V2 persistence, and browser options.
- [x] Wiggle stack offset uses weighted adjacent-position baseline changes for both bar
      orientations and X-aligned areas, with shared auto domains, exact hits, V2 persistence,
      and browser options.
- [x] Range bars (category-band low/high rectangles and exact hits).
- [x] Error bars: numeric, temporal, and category bounds, updates, hits, persistence, and executor coverage audited against existing engine, browser, and GPUI tests.
- [x] Gradients across area and bar fill families.
- [x] `xy_area` and `range_area` explicit top/bottom CSS gradient colors use ordered `AreaFill`
      and `BandGradientFill` contracts across Canvas2D, WebGPU, and GPUI, with alpha scaled by fill
      opacity, atomic validation, V2 persistence, and browser options.
- [x] `column`, `horizontal_bar`, and `range_bar` use bounded top/bottom `fill_gradient` stops
      over each mark's own bounds; square/rounded paths, row-color precedence, Canvas2D/WebGPU/GPUI
      lowering, V2 persistence, and browser image assertions are covered.
- [x] Per-item customization (per-point color/symbol and per-bar color styles).
- [x] Sparse per-row CSS color overrides across object/typed ingestion, atomic explicit-ID updates,
      bounded retention, shared mark frames, V2 persistence, and browser round trip.
- [x] Sparse per-row point-symbol overrides for scatter and enabled path markers, with shared
      frame/exact-hit semantics, atomic updates, V2 persistence, and typed browser ingestion.
- [x] Composition of compatible families on shared axes: ordered category area/line/column/range/
      box/error and continuous line/scatter/bubble/error/heatmap browser fixtures, plus an engine
      frame-order, domain, and exact-hit regression.
- [x] Audit existing box plot, heatmap and bubble storage, geometry, update, hit, accessibility,
      persistence, and browser paths rather than rewriting them; four focused Chromium cases passed
      on 2026-10-10. Legends remain in R4.
- [x] Heatmap value-intensity domain can be fixed through shared series options, V2 persistence,
      and browser API; category, numeric, and temporal geometry tests and a Chromium restore case pass.
- [x] Heatmap low/high CSS color endpoints interpolate in the shared frame with the existing
      default palette preserved; engine frame, V2 persistence, and Chromium restore tests pass.
- [x] Box-plot fill and median CSS colors plus shared stroke width reach the ordered frame,
      with atomic validation, V2 persistence, and focused Chromium coverage.
- [x] G3 required portable full gate green; batch committed and pushed. Machine-specific GPUI
      pixel capture and clean official browser startup/upload budgets remain separately pending.

**Exit:** each family passes object and typed ingestion, atomic updates, missing, duplicate and
extreme data, visibility and stack changes, exact and nearest hits, labels, accessibility,
persistence and executor parity. Reference examples demonstrate visual configurability; high-density
fixtures demonstrate bounded work.

### R4 — Chart components and interaction

**Batch:** G4. **Depends on:** R1–R3. **Status:** complete with independently gated correction after `1a2318b`.

- [x] Usable default legends and tooltips.
- [x] Titles and labels.
- [x] References.
- [x] Keyboard and touch brush controls.
- [x] Selection.
- [x] Linked-chart synchronization, extending the shared contract delivered first for financial
      charts as PD5 (semantic crosshair and visible-range events with source and revision; see
      the trading contract in [architecture.md](../docs/architecture.md)).
- [x] Frame image export, extending the shared contract delivered first as PD6
      (`capture_export_frame` and the native export path in architecture.md).
- [x] Localization, overflow and focus behavior.
- [x] Original G4 gate green; `1a2318b` committed and pushed.
- [x] Construction-order correction: same horizontal-bar plot, ink and exact hits at 356×330
      for resize-before-populate, populate-before-resize and autoSize, with engine and rebuilt
      browser regressions. The G3 Y-view clip pixel/hit test must run without a post-zoom resize.
- [x] G4 corrective snapshot's complete applicable gate green; reviewed G4-only follow-up
      committed and pushed without provisional G5 work.

R4 extends the PD5 and PD6 contracts to general domains and chrome; it does not build a second
synchronization or export path.

The 2026-10-12 corrective gate used a clean `1a2318b` snapshot plus only G4-owned changes,
not the shared worktree's provisional G5 files. A new engine regression failed first at
356×330 (264 versus 284 px with fixture text metrics); the rebuilt public Canvas2D regression
failed first at 264 versus 288 px. After financial/general axis-label measurement was separated,
the engine test and the public resize-before/after/autoSize pixel-and-hit check passed.
The G3 Y-view clip test now probes visible pixels and exact hits before any post-zoom resize,
and passed on both Canvas2D and WebGPU. Complete locked Rust/WASM lint and workspace tests,
package lint/build/typecheck/pack/API/namespace/release guards and policy-v7 artifact ceilings,
native image/frame tests, GPUI structural parity/replay, strict native performance, and the
fresh-asset portable browser matrix passed (606 pass, 22 intended browser/platform skips).
The GPUI example also consumes the G4 brush-change event so its all-targets gate compiles.
Machine-specific GPUI pixels and the clean official browser benchmark remain separate pending
phase-closure evidence, not portable gate passes.

**Exit:** consumers build an interactive dashboard from published APIs without demo-owned semantic
logic. Legend toggles preserve identity, brushing survives resize, synchronization handles unequal
datasets without loops, and all controls are keyboard and screen-reader usable with bounded
snapshots.

### R5 — React and framework-neutral authoring

**Batch:** G5. **Depends on:** R1–R4. **Status:** complete.

- [x] Composable components and typed data mapping over canonical handles.
- [x] Controlled and uncontrolled behavior where applicable, events and documented defaults.
- [x] Migration recipes from Recharts.
- [x] Equivalent imperative composition for hosts that do not use React.
- [x] G5 full gate green; batch committed and pushed.

**Exit:** packed-consumer examples cover standalone, composed, synchronized and financial/general
charts. Prop changes retain engine identities; failure, Strict Mode and concurrent lifecycle tests
pass. SSR import, hydration setup, bundler and WASM asset resolution, and cleanup work without
repository paths.

### R6 — Polar families and shared transitions

**Batch:** G6. **Depends on:** R2–R5. **Status:** complete.

- [x] Angular and radial transforms with shared sector and polygon geometry.
- [x] Pie and donut.
- [x] Radar.
- [x] Radial bar.
- [x] Polar area.
- [x] Shared general transitions for every family, including existing Cartesian families, with
      interruption and reduced-motion behavior.
- [x] G6 full gate green; batch committed and pushed.

**Exit:** all polar variants cover degenerate, zero and missing data, angles and radii, label
collision, legends, selection, keyboard navigation, persistence and every executor. Fixed-clock
transition fixtures prove repeatability, correct interaction targets, bounded memory and zero idle
animation scheduling.

### R7 — Hierarchy and flow families

**Batch:** G7. **Depends on:** the lifecycle, layout and primitive contracts above. **Status:** complete.

- [x] Funnel.
- [x] Treemap.
- [x] Sankey.
- [x] Sunburst, using shared polar geometry where appropriate.
- [x] G7 full gate green; batch committed and pushed.

Each is a dedicated engine layout family with typed validated inputs; do not reuse incompatible XY
storage merely to avoid a proper owner.

**Exit:** each family has deterministic ordering and layout, declared cycle, depth and size policies,
bounded work, update and transition behavior, labels and styles, exact hits, tooltip and legend
behavior where applicable, accessibility, persistence and cross-backend fixtures. These families are
required for competitive closure.

### R8 — Parity closure and release readiness

**Batch:** G8. **Depends on:** R0–R7. **Status:** in progress.

- [ ] Run the complete matrix against the pinned competitor.
- [ ] Demonstrate financial-only, general-only and combined workloads.
- [ ] Finish documentation and customization/migration examples.
- [ ] Export, backend fallback and device-recovery evidence.
- [x] Clean-install evidence.
- [x] Recheck upstream scope; record and assess new upstream features explicitly.
- [ ] G8 full gate green; release evidence committed and pushed.

**Exit:** every required matrix row is verified or has a maintainer-approved, clearly documented
semantic alternative that satisfies the user task. No required family remains demand-deferred.
Publish capability claims only for verified behavior, with measured startup, size, frame, input and
memory results and known limits.

## Parity coverage matrix

Each row must acquire exact versioned reference examples, Aeris API mappings, named automated
fixtures, manual checks where needed, and recorded differences before it can be marked verified.
Status is deliberately conservative: **Partial** means code exists but the full row is unverified;
**Open** means the product contract still needs delivery. These are Aeris requirements, including
platform capabilities beyond the competitor's browser rendering model.

| Capability | Status | Required outcome | Phase |
| --- | --- | --- | --- |
| Standalone and composed charts | Partial | General-only, financial-only and mixed panes; compatible overlays, explicit axes, deterministic ordering and lifecycle | R1, R3 |
| Line, area, range and scatter/bubble | Partial (items 1–8 delivered; range bars are tracked below) | Linear/step/curved interpolation, gap/connection policy, baselines, symbols, active marks, fills/strokes and error bounds | R3 |
| Bars and stacks | Partial (range bars delivered) | Both orientations, groups, sizing/gaps, corners, per-item styling, mixed signs, range bars, stack order and required offset modes | R3 |
| Box plots and heatmaps | Partial | Preserve existing extra families; complete color domains, legends, missing values and interaction | R3 |
| Scales and axes | Verified (G2, 2026-10-10); defaults revised in `867029a`, recheck in R8 | Numeric/log/symlog, temporal, category/point, reversed/multiple axes, explicit/auto domains, ticks, formatting, overflow and grid policy | R2 |
| Pie/donut, radar, radial bar, polar area | Partial | Polar layout, start/end angles, inner/outer radii, padding, labels/leaders, angular/radial axes and interactions | R6 |
| Funnel, treemap, Sankey, sunburst | Partial (G7 family contracts and automated executor fixtures pass; manual parity evidence pending) | Purpose-built deterministic bounded layouts, data contracts, styling, labels, hits, accessibility and updates | R7 |
| Legend and tooltip | Partial | Default usable components, visibility controls, item/shared modes, placement, formatting, ordering, custom content and touch/keyboard behavior | R4 |
| References, labels, titles and grids | Partial | Engine layout and domain contribution, overlap/overflow policy, background/foreground order, style and export consistency | R2, R4 |
| Brush, selection and synchronization | Partial | Pointer/touch/keyboard controls, semantic range handles, domain-aware pan/zoom, linked charts and feedback-loop prevention | R4 |
| Responsive layout | Verified (G2, 2026-10-10); general-only gutters revised in `867029a`, recheck in R8 | Zero-size/hidden/revealed containers, constrained/aspect sizing, DPR/font changes, small plots and bounded layout convergence | R1, R2 |
| React authoring | Partial | Composable axes/series/components, typed data mapping, controlled updates, stable identities, Strict Mode, failure cleanup and SSR-safe import | R5 |
| Customization | Partial | Per-item styles, symbols, gradients, dash patterns, label/tooltip formatting, bounded custom marks and explicit host-only content boundaries | R3–R5 |
| Animation | Partial for general transitions | Enter/update/exit and interruption with stable identities, bounded retained state, shared timing semantics and reduced motion | R6 |
| Accessibility/localization | Partial | Keyboard operation of every family/control, meaningful bounded snapshots, focus retention, announcements, contrast, locale and text measurement | Every phase |
| Persistence, export and recovery | Partial (mixed V2 restore in three browsers; native mixed frame export; Chromium WebGPU loss fixtures) | Complete schema coverage, atomic restore, callback reattachment, equivalent frame exports, backend failover and clean disposal | Every phase, R8 |
| Packaging and migration | Partial | Packed-consumer examples, framework-neutral and React guides, discoverable API, reproducible competitor comparisons | R0, R5, R8 |

## Scope and parity rules

A basic working dashboard, a list of rendered chart types, or a thin React wrapper does not meet
this goal. Competitive quality includes authoring, visual control, interaction, accessibility,
lifecycle, responsive layout, documentation, distribution and measured performance.

Keep one public library, one `ChartEngine` and one ordered frame contract. Preserve the specialized
financial data and coordinate paths while completing general charting as a first-class capability.
The browser package is `@aeristerminal/aeris-charts` with an optional `/react` entry. Package naming
or distribution changes require a compatibility decision; this plan does not introduce another
product. [architecture.md](../docs/architecture.md) describes current ownership and execution; this document
specifies the target and acceptance gates.

The existing shared-engine direction is sound. Replacing it with a browser-only renderer or making
financial storage universally generic would harm Aeris: charts would diverge across backends or
financial updates would pay unnecessary work. Retain the working foundations and finish their
contracts.

**Parity definition.** Parity means equivalent user capabilities, predictable behavior and polished
results. It does not require copying React/SVG internals, identical method names, undocumented
quirks, or arbitrary DOM execution inside Rust. Record deliberate semantic differences and
demonstrate the migration path. An omission cannot be renamed a difference merely to close a
milestone.

**Out of scope for this baseline.** Gauge, arbitrary graph/network, geographic and 3D visualization.
They may be added later without postponing any required row. Full competitiveness is a release gate,
not a promise to implement every conceivable visualization.

**References.** Reviewed on 2026-10-10. R0 pins Recharts 3.10.1 at source revision
`ffb918798051ef040bb7f9922d3850c9c189f39f`; its package and source `LICENSE` files are MIT.
The executable mappings are in `examples/web_demo/fixtures/g1/recharts_3_10_1/matrix.json`. A moving
documentation site is not the permanent test baseline.

- [Recharts API catalog](https://recharts.github.io/en-US/api/) establishes Cartesian, polar, composed,
  funnel, treemap, Sankey and sunburst families plus shared components and synchronization.
- [Line API](https://recharts.github.io/en-US/api/Line/) supplies reference behavior for data mapping,
  dots, missing-point connections, styling and animation.
- [XAxis API](https://recharts.github.io/en-US/api/XAxis/) supplies axis/domain/tick configuration coverage.
- [Tooltip API](https://recharts.github.io/en-US/api/Tooltip/) supplies tooltip presentation and behavior coverage.
- [ResponsiveContainer API](https://recharts.github.io/en-US/api/ResponsiveContainer/) supplies sizing coverage.
- [FunnelChart API](https://recharts.github.io/en-US/api/FunnelChart/) documents stacking offsets and synchronization.
- [Sankey API](https://recharts.github.io/en-US/api/Sankey/),
  [Treemap API](https://recharts.github.io/api/Treemap/) and
  [Sunburst API](https://recharts.github.io/en-US/api/SunburstChart/) establish distinct flow/hierarchy contracts.
- [Accessibility guidance](https://github.com/recharts/recharts/wiki/Recharts-and-accessibility)
  informs keyboard and screen-reader comparisons.

### R8 upstream scope recheck (2026-10-19)

The [official Recharts release list](https://github.com/recharts/recharts/releases) still marks
3.10.1 as the latest stable release; 3.11.0-canary.6 is a prerelease. Keep the executable parity
baseline pinned to 3.10.1 so release claims have a reproducible comparison.

The 3.10.0 features already inside the pinned baseline need explicit G8 checks: legend
position/offset in the legend and tooltip row, automatic X-axis height in scales and responsive
layout, and Sankey accessibility metadata in the hierarchy/flow and accessibility rows. Their
presence in the pinned release notes is scope evidence, not proof that Aeris supports them.
The canary adds an experimental chart theme system with automatic color-scheme selection,
per-family active styles and hierarchy colors. Aeris has its own portable style/theme model;
compare visual and accessibility outcomes, but do not substitute the canary API for the pinned
baseline. The canary's Bar-local data prop maps conceptually to Aeris series-local data; angled
polar labels and text paths are additional presentation capabilities outside this stable baseline.
Reassess them when 3.11 becomes stable. The 3.11 canary release notes also contain correctness
fixes for stacks, brushes, axes, transitions, polar labels and synchronization; use them as
regression ideas, not evidence that current Aeris behavior is wrong.

## Architecture rules

```text
Framework-neutral API / React authoring / native host
    -> validated commands and bulk data normalization
    -> one ChartEngine
         financial: DataLayer, time union, price/time scales, financial interactions
         general: typed datasets, explicit domains/axes, series and layout families
    -> shared layout, semantic interaction snapshots and ordered ChartFrame
    -> DrawList
    -> Canvas2D | WebGPU | GPUI | native
```

### Ownership and dependency direction

| Owner | Responsibility |
| --- | --- |
| `aeris_charts_core` | Platform-free scale math, validation fundamentals, financial storage and shared option/value types; f64 media-space math |
| `aeris_charts_engine` | General datasets, axes, domain resolution, series/layout algorithms, mutations, interaction, transitions, persistence and frame construction |
| `aeris_charts_render` | Ordered primitives and shared lowering/tessellation math; no host or chart-family policy |
| Executors | Execute prepared primitives with equivalent clipping/blending/text; own bounded device/font/image resources and recovery |
| WASM and TypeScript | Bulk conversion, platform input, resource initialization, typed handles, host callbacks, DOM presentation and accessibility |
| React | Declarative ownership and reconciliation through the public imperative API; no duplicate data/geometry/interaction model |

Do not add crates, generic scene graphs, plugin registries, trait layers or speculative feature flags
just to accommodate the roadmap. Extract cohesive internal modules when their actual responsibilities
justify it. Do not turn growing `general_series.rs` into a universal layout abstraction: hierarchy
and flow need appropriate typed input and algorithms, while sharing lifecycle and frame output.

### Financial isolation and first-class general creation

Keep financial time union, compact OHLC/scalar columns, LOD, indicators, drawings, trading, price
scales and streaming updates authoritative. General work must add no per-row dispatch to those loops
and retain zero general dataset/cache capacity in a financial-only chart. Protect existing public
APIs, V1 restore, input behavior, whitespace and financial golden fixtures.

Add an explicit creation-time domain/topology contract at the engine owner and expose it consistently
through WASM, TypeScript and React. Defaults remain financial. General-only charts must reserve only
the chrome they use. Stable pane IDs must survive reorder; disposal must not require callers to seed
a temporary financial keeper pane. Define the engine's last-pane invariant and adapter ownership
together.

### Data, identity and atomic mutations

Retain typed general columns, explicit validity and stable row IDs. Distinguish financial UTC
seconds, continuous epoch milliseconds, category identity and display text without unit guessing.
Specify ordering, duplicate-X/category policy, missing values and generated-versus-explicit identity
per family.

Provide in-place axis/series option updates and visibility, order and compatible binding changes.
Validate dependent datasets, stacks, references and axes before mutation. Invalid updates must
preserve the entire prior state. A style change retains data, handles, focus/selection and runtime
view unless its documented semantics require otherwise. Structural domain/type changes must be
explicit.

Chart-level object data and React data keys normalize once per changed input; typed streaming
remains a bulk path. Decide shared-column ownership from real composed-chart callers and measurements
before adding public dataset machinery. Host accessors never run inside frame or hit-test loops.
Hierarchy and flow input require stable node/link identities, validation of references/cycles as
applicable, defined ordering and depth/size/work caps; they must not be forced into XY rows.

### Layout, scales and shared geometry

Resolve data domains and runtime views separately. The same transform must drive ticks, grids,
geometry, hits, brushes, references and accessibility. Complete temporal interval selection and
formatting with an explicit deterministic timezone policy; never silently use the browser timezone.
Category zoom/pan and duplicate labels need declared semantics. Test extreme and degenerate domains.

Layout reserves chart content, titles, legends, axis strips and plot regions through bounded passes.
Define behavior when text/axes do not fit rather than letting clipping or layout oscillation decide.
Font, DPR, formatter and locale changes invalidate measurements. Host measurement is permitted;
host-owned tick selection or autoscale is not.

General grids use resolved general ticks in the ordered background layer. Reference/background
fills, series, interaction chrome and labels need explicit order and clips. New curves, sectors,
polygons, symbols, rounded shapes or gradients must have shared geometry/lowering and every executor
implemented before their public feature is complete. Retain f64 until the documented encoding
boundary.

### Components, interactions and extension boundaries

The engine owns component meaning, content snapshots, anchors, selections and reserved plot space.
Hosts may present HTML tooltips, semantic controls or accessible DOM. Default legends/tooltips must
be usable without copying demo code; optional rich HTML does not become the only implementation of
built-in chart geometry. Define what exports include and provide frame-rendered equivalents for
built-in chrome. Document host-only custom content limitations.

Unify pointer, touch and keyboard commands for general selection, brushing and view changes. Derive
axis-versus-item tooltip membership from the correct oriented domain, including horizontal bars,
duplicate values, missing rows and mixed series. Synchronization uses semantic values or declared
index matching with explicit mismatch policy. A bounded host coordinator may route events between
independent charts; each receiving engine resolves its own semantics. Source/revision tracking must
prevent loops and disposal must remove subscriptions.

Customization receives bounded read-only snapshots or returns validated styles/marks through an
explicit host boundary. Do not promise portable execution of arbitrary SVG/React elements. Built-ins
must remain available on native and headless paths, with clear migration equivalents for common
Recharts customization tasks. Review licenses before copying any external code or assets.

### React and transitions

Build declarative axes, Cartesian/polar series, legend, tooltip, labels, references and brush over
complete imperative mutations. Retain GeneralPane compatibility. Stable keys retain identities;
ordinary options/visibility/data changes do not recreate chart objects. Validate a reconciliation
batch before destructive operations and roll back newly acquired resources on failure. Test changed
kinds, invalid data, callback failures, parent/child cleanup, async initialization, stale closures,
concurrent rerenders and Strict Mode. SSR-safe import and static server-rendered chart output are
different capabilities; document each accurately.

General transitions are engine-owned interpolation sampled with an explicit monotonic clock supplied
by hosts. Bound duration, retained prior geometry and active transitions; interrupt from the current
presentation, reconcile hit/focus behavior, and stop scheduling at rest. Reduced motion disables or
shortens transitions deterministically. Streaming financial updates retain their established policy.

### Invalidation, performance and persistence

Classify mutations by data, domain, layout, geometry, paint and interaction impact. Record generation
inputs for retained state; remove caches with their owner and rebuild device resources after loss.
A row cap alone does not prove bounded interactive latency. Measure worst-case dense/overlapping
hits, stack alignment, category unions, long labels, many axes/series and hierarchy depth. Reuse
existing LOD/index mechanisms where appropriate; add optimizations only after release measurements.

Persist new semantic configuration with versioned migrations and transactional restore. Preserve V1
financial compatibility and current V2 contracts; version schema extensions when compatibility
requires it. Runtime callbacks, DOM, device resources and transient animation/hover/focus are not
serialized. Restored live handles must not alias stale handles. State exports and image exports are
separate gates.

## Verification and evidence policy

Each implementation starts with a failing regression or measurable invariant through its actual
public host/executor path. Shared-engine unit tests alone cannot close a browser or rendering
requirement.

Verification cadence:

- **During a batch:** focused checks only (touched-crate check, tests and clippy, and the affected
  families' frame fixtures).
- **End of batch:** the complete gates below once, plus Playwright and GPUI parity/replay when the
  batch affects those paths; then commit and push the batch.
- **Phase closure:** coverage-matrix verification, manual screenshots, accessibility review,
  competitor comparisons and recorded benchmarks.

Run the complete gates required by [AGENTS.md](../AGENTS.md) at the end of each batch, before
committing code:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy -p aeris_charts_wasm --target wasm32-unknown-unknown --locked -- -D warnings
cargo test --workspace --locked
cargo run -p aeris_charts_native --example perf_gate --release

cd packages/charts
npm ci
npm run lint
npm run build
npm run typecheck
npm run test:pack
```

Also run public API/namespace/release-policy guards, applicable Chromium/Firefox/WebKit Playwright
coverage, native golden/frame checks, and GPUI parity/replay checks for affected execution. Keep
portable correctness blocking and calibrated machine-specific visual/performance evidence labeled.
Run performance thresholds in strict mode as CI does; never relax tests or budgets to conceal
regressions.

Use `benchmarks/benchmark.mjs` and `benchmarks/budgets.json` for versioned release evidence. Current
policy v7 includes a 2,000 ms general-dashboard startup ceiling and 100,663,296-byte peak per-repaint
upload ceiling, alongside package-size limits. These limits are existing guards, not a declaration
that their ceilings are competitive targets. Preserve them until measured evidence supports an
explicit revision. Add family-specific budgets before closure, including p95 input/frame latency,
steady-state allocation, retained CPU/GPU memory, upload work, cold startup and repeated disposal.
Compare equal data, viewport, DPR, interactions and release builds; disclose hardware/browser/font
versions and unsupported metrics.

A matrix entry can be marked **verified** only with a commit/revision, exact fixture commands and
results, backend/browser coverage, relevant performance evidence, and recorded manual checks.
Screenshots must cover small and large containers, light/dark themes, long/Unicode labels, overflow
and active states. Accessibility requires interaction and assistive-technology review, not just
snapshot existence.

Documentation-only revisions may skip runtime gates. Check diffs, links/paths, source consistency and
documentation hygiene. This plan revision makes no production ownership or execution change; update
`architecture.md` in the same commit as future code that changes those contracts. Preserve unrelated working-tree changes and stage only task-owned
files.

## Definition of completion

Aeris is competitively complete for this plan when a consumer can build the full required Recharts
capability matrix through a coherent published API, combine it with the established financial
product, and rely on equivalent semantic output across supported backends. Routine changes retain
identity; invalid updates are atomic; controls are accessible; styles and layouts are deliberate;
resources and work are bounded; installation and migration are documented; and all release gates have
current evidence.

Until then, report delivered items and remaining gaps precisely against the phase checklists and
matrix rows above. A working demo or a green subset of tests is progress, not completion of the
all-in-one library.
