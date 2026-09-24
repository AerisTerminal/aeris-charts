# Nucleus Charts All-in-One Architecture and Competitive Delivery Plan

## Decision and scope

Nucleus will be a complete financial and general visualization library. Lightweight Charts is the
financial competitive reference; Recharts is the general charting competitive reference. A basic
working dashboard, a list of rendered chart types, or a thin React wrapper does not meet this goal.
Competitive quality includes authoring, visual control, interaction, accessibility, lifecycle,
responsive layout, documentation, distribution, and measured performance.

Keep one public library, one ChartEngine, and one ordered frame contract. Preserve the specialized
financial data and coordinate paths while completing general charting as a first-class capability.
The browser package remains `@axiusflowhq/financial` with an optional `/react` entry. Package naming
or distribution changes require a compatibility decision; this plan does not introduce another product.

This revision is an architecture review and future delivery contract, dated 2026-09-23, with the
implementation status below updated on 2026-09-24. It does not claim that the open work below is
implemented. The working tree's earlier general-chart and demo changes were not release evidence;
the completed slices listed below have since been committed and verified.
[Architecture.md](Architecture.md) describes current ownership and execution. This document specifies
the target and acceptance gates. [General_charts_api.md](General_charts_api.md) mixes implemented
contracts with proposals and must be reconciled as part of R0 below.

## Current implementation status (2026-09-24)

The following narrow R3 visual-configuration slices are implemented and pushed to `github/main`:

- `d17a39f` — general-series `line_width` is configurable through the engine, WASM, TypeScript,
  persistence, and public browser API.
- `819bcbb` — general path `line_style` supports portable `solid`, `dotted`, and `dashed` rendering
  through the shared frame contract.
- `6a5684b` — `xy_area` accepts an explicit finite `baseline_value`, with shared fill geometry,
  hit-testing, persistence, WASM/TypeScript exposure, and public browser coverage.
- General line, area, stacked-area, and range-area series accept opt-in `point_markers`
  with configurable radius, shared frame geometry, matching hit targets, atomic live mutation,
  V2 persistence, WASM/TypeScript exposure, and public browser coverage. The same slice repairs
  live mutation of the previously released `line_width`, `line_style`, and `baseline_value` fields.
- Scatter and opt-in path markers accept persisted `circle`, `square`, `diamond`, and `triangle`
  symbols, lowered through existing shared primitives with symbol-matched exact hit geometry. Bubble
  marks remain area-scaled circles.
- General line, area, stacked-area, and range-area paths accept persisted `linear`, `step`, and
  `curved` interpolation. Shared path and coupled band expansion drive every renderer and matching
  exact/nearest hit geometry, while stacked areas reject mixed interpolation that could open seams.
- General line, area, stacked-area, and range-area paths accept opt-in persisted `connect_missing`
  semantics. Missing rows remain queryable but no longer split connected runs; transform-invalid
  coordinates remain hard gaps, and stacked members must share one connection policy.
- Area and range-area fills accept bounded persisted `fill_opacity` values from the browser API;
  the shared frame applies the value to ordinary, stacked, and coupled-band fills while preserving
  the existing gradient relationship. Rust persistence, WASM serialization, TypeScript options,
  and Chromium/Firefox/WebKit round-trip coverage are complete. The slice was verified with the
  workspace tests, workspace and WASM clippy, package lint/build/typecheck/package smoke test,
  and the general-chart browser matrix (88 passed, 2 skipped).

These slices passed the applicable Rust tests, clippy checks (including the WASM target), package
lint/build/typecheck/package smoke test, Chromium browser tests, formatting checks, and the native
release performance gate. They do not close R3: the phase still requires the rest of the Cartesian
visual/data matrix and its complete acceptance evidence. R0-R8 remain open, and no competitive
parity or release-completion claim should be inferred from these completed slices.

## Review conclusion and corrected status

The existing shared-engine direction is sound. Replacing it with a browser-only renderer or making
financial storage universally generic would harm Nucleus: charts would diverge across backends or
financial updates would pay unnecessary work. Retain the working foundations and finish their contracts.

The previous Phase 1-3 completion statements described narrower implementation slices. They are
superseded by this review: Cartesian product completeness and the all-in-one authoring experience
are **open**. Earlier test counts and timings are historical observations in Git history, not proof
of current completion. Existing code and tests remain valuable and must be preserved.

| Area | Code evidence reviewed | Assessment and required correction |
| --- | --- | --- |
| Shared ownership | `general_data.rs`, `general_series.rs`, frame construction, engine Cargo dependencies | Typed general data and semantics already live in Rust beside financial storage. Retain this boundary. |
| Standalone creation | `examples/web_demo/general_dashboard.js` creates a general pane then removes pane 0 | Provide an explicit initial general domain through the canonical constructor, without a transient financial pane or host cleanup recipe. Keep the financial default compatible. |
| Temporal axes | `general_axes.rs::axis_ticks`, `tick_labels_for_domain`, `pan_general_axis`, `zoom_general_axis` | Temporal data and geometry exist, but temporal ticks fall through to empty output and pan/zoom reject nonnumeric domains. Complete the full temporal coordinate contract. |
| Grid and zero lines | `GeneralAxis` stores policies; accessor callers currently serialize them in `persistence.rs` | Accepted options have no corresponding general grid execution. Implement shared frame output and observable toggle tests. |
| Mutable public objects | `packages/charts/src/types.ts::general_series_api` and `general_axis_api` | General handles lack option mutation. Rust has visibility mutation, but the browser handle lacks it. Add atomic mutations that preserve identity and invalidate the affected state. |
| React reconciliation | `packages/charts/src/react.ts::GeneralPane` | Changed series options recreate series; changed axes recreate dependent series. GeneralPane accepts configuration arrays rather than the proposed component composition. Complete both engine mutation and declarative authoring. |
| Failed React installation | `GeneralPane` creates a handle, calls `setData`, then records ownership | A failed initial data install can leave an untracked series. Add rollback and failure-path lifecycle tests; review callback exceptions and cleanup ordering too. |
| Visual configuration | `GeneralSeriesOptions`, `frame/general_series_geometry.rs` | The surface is narrow; general line width is fixed in frame construction. Audit and implement documented styles and geometry choices end to end. |
| Shared components | Legend/shared-tooltip/brush/reference snapshots in `general_series.rs` | Semantic building blocks exist. Snapshots alone do not establish a complete interactive legend, tooltip, brush, or export experience. |
| Chart breadth | `GeneralSeriesKind` has ten Cartesian variants | Polar and hierarchy/flow families remain open. Funnel, treemap, Sankey, and sunburst belong in the competitive target, not an indefinite demand backlog. |
| Documentation | Prior plan examples and `Architecture.md` | Prior scatter example omitted required axis bindings. Architecture places extensive engine-owned general behavior under the core heading and overgeneralizes retained React updates. Correct ownership wording without presenting future code as current. |

These are source-confirmed gaps or explicitly identified review risks. Remaining matrix entries below
are required coverage to audit, not assertions that every listed feature is absent. No claim of an
exhaustive defect audit or current benchmark pass is made.

## Competitive parity contract

Parity means equivalent user capabilities, predictable behavior, and polished results. It does not
require copying React/SVG internals, identical method names, undocumented quirks, or arbitrary DOM
execution inside Rust. Record deliberate semantic differences and demonstrate the migration path.
An omission cannot be renamed a difference merely to close a milestone.

The following official references were reviewed on 2026-09-23. R0 must pin the exact released Recharts
version/source revision used by executable comparisons; a moving documentation site is insufficient
as a permanent test baseline.

- [Recharts API catalog](https://recharts.github.io/en-US/api/) establishes Cartesian, polar, composed,
  funnel, treemap, Sankey, and sunburst families plus shared components and synchronization.
- [Line API](https://recharts.github.io/en-US/api/Line/) supplies reference behavior for data mapping,
  dots, missing-point connections, styling, and animation.
- [XAxis API](https://recharts.github.io/en-US/api/XAxis/) supplies axis/domain/tick configuration coverage.
- [Tooltip API](https://recharts.github.io/en-US/api/Tooltip/) supplies tooltip presentation and behavior coverage.
- [ResponsiveContainer API](https://recharts.github.io/en-US/api/ResponsiveContainer/) supplies sizing coverage.
- [FunnelChart API](https://recharts.github.io/en-US/api/FunnelChart/) documents stacking offsets and synchronization.
- [Sankey API](https://recharts.github.io/en-US/api/Sankey/),
  [Treemap API](https://recharts.github.io/api/Treemap/), and
  [Sunburst API](https://recharts.github.io/en-US/api/SunburstChart/) establish distinct flow/hierarchy contracts.
- [Accessibility guidance](https://github.com/recharts/recharts/wiki/Recharts-and-accessibility)
  informs keyboard and screen-reader comparisons.

### Required coverage matrix

Each row must acquire exact versioned reference examples, Nucleus API mappings, named automated
fixtures, manual checks where needed, and recorded differences before it can be marked verified.
The initial status is deliberately conservative: **partial** means code exists but the full row is
unverified; **open** means the product contract still needs delivery. These are Nucleus requirements,
including platform capabilities beyond the competitor's browser rendering model.

| Capability | Initial status | Required outcome | Delivery |
| --- | --- | --- | --- |
| Standalone and composed charts | Partial | General-only, financial-only, and mixed panes; compatible overlays, explicit axes, deterministic ordering and lifecycle | R1, R3 |
| Line, area, range and scatter/bubble | Partial | Linear/step/curved interpolation, gap/connection policy, baselines, symbols, active marks, fills/strokes and error bounds | R3 |
| Bars and stacks | Partial | Both orientations, groups, sizing/gaps, corners, per-item styling, mixed signs, range bars, stack order and required offset modes | R3 |
| Box plots and heatmaps | Partial | Preserve existing extra families; complete color domains, legends, missing values and interaction | R3 |
| Scales and axes | Partial | Numeric/log/symlog, temporal, category/point, reversed/multiple axes, explicit/auto domains, ticks, formatting, overflow and grid policy | R2 |
| Pie/donut, radar, radial bar, polar area | Open | Polar layout, start/end angles, inner/outer radii, padding, labels/leaders, angular/radial axes and interactions | R6 |
| Funnel, treemap, Sankey, sunburst | Open | Purpose-built deterministic bounded layouts, data contracts, styling, labels, hits, accessibility and updates | R7 |
| Legend and tooltip | Partial | Default usable components, visibility controls, item/shared modes, placement, formatting, ordering, custom content and touch/keyboard behavior | R4 |
| References, labels, titles and grids | Partial | Engine layout and domain contribution, overlap/overflow policy, background/foreground order, style and export consistency | R2, R4 |
| Brush, selection and synchronization | Partial | Pointer/touch/keyboard controls, semantic range handles, domain-aware pan/zoom, linked charts and feedback-loop prevention | R4 |
| Responsive layout | Partial | Zero-size/hidden/revealed containers, constrained/aspect sizing, DPR/font changes, small plots and bounded layout convergence | R1, R2 |
| React authoring | Partial | Composable axes/series/components, typed data mapping, controlled updates, stable identities, Strict Mode, failure cleanup and SSR-safe import | R5 |
| Customization | Partial | Per-item styles, symbols, gradients, dash patterns, label/tooltip formatting, bounded custom marks and explicit host-only content boundaries | R3-R5 |
| Animation | Open for general transitions | Enter/update/exit and interruption with stable identities, bounded retained state, shared timing semantics and reduced motion | R6 |
| Accessibility/localization | Partial | Keyboard operation of every family/control, meaningful bounded snapshots, focus retention, announcements, contrast, locale and text measurement | Every phase |
| Persistence, export and recovery | Partial | Complete schema coverage, atomic restore, callback reattachment, equivalent frame exports, backend failover and clean disposal | Every phase, R8 |
| Packaging and migration | Partial | Packed-consumer examples, framework-neutral and React guides, discoverable API, reproducible competitor comparisons | R0, R5, R8 |

Gauge, arbitrary graph/network, geographic, and 3D visualization are beyond this competitive baseline.
They may be added later without postponing any required row. Full competitiveness is a release gate,
not a promise to implement every conceivable visualization.

## Architecture to retain and complete

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
| `nucleuscharts_core` | Platform-free scale math, validation fundamentals, financial storage and shared option/value types; f64 media-space math |
| `nucleuscharts_engine` | General datasets, axes, domain resolution, series/layout algorithms, mutations, interaction, transitions, persistence and frame construction |
| `nucleuscharts_render` | Ordered primitives and shared lowering/tessellation math; no host or chart-family policy |
| Executors | Execute prepared primitives with equivalent clipping/blending/text; own bounded device/font/image resources and recovery |
| WASM and TypeScript | Bulk conversion, platform input, resource initialization, typed handles, host callbacks, DOM presentation and accessibility |
| React | Declarative ownership and reconciliation through the public imperative API; no duplicate data/geometry/interaction model |

Do not add crates, generic scene graphs, plugin registries, trait layers, or speculative feature flags
just to accommodate the roadmap. Extract cohesive internal modules when their actual responsibilities
justify it. Do not turn growing `general_series.rs` into a universal layout abstraction: hierarchy
and flow need appropriate typed input and algorithms, while sharing lifecycle and frame output.

### Financial isolation and first-class general creation

Keep financial time union, compact OHLC/scalar columns, LOD, indicators, drawings, trading, price scales,
and streaming updates authoritative. General work must add no per-row dispatch to those loops and
retain zero general dataset/cache capacity in a financial-only chart. Protect existing public APIs,
V1 restore, input behavior, whitespace and financial golden fixtures.

Add an explicit creation-time domain/topology contract at the engine owner and expose it consistently
through WASM, TypeScript and React. Defaults remain financial. General-only charts must reserve only
the chrome they use. Stable pane IDs must survive reorder; disposal must not require callers to seed
a temporary financial keeper pane. Define the engine's last-pane invariant and adapter ownership together.

### Data, identity and atomic mutations

Retain typed general columns, explicit validity and stable row IDs. Distinguish financial UTC seconds,
continuous epoch milliseconds, category identity and display text without unit guessing. Specify
ordering, duplicate-X/category policy, missing values and generated-versus-explicit identity per family.

Provide in-place axis/series option updates and visibility, order and compatible binding changes.
Validate dependent datasets, stacks, references and axes before mutation. Invalid updates must preserve
the entire prior state. A style change retains data, handles, focus/selection and runtime view unless
its documented semantics require otherwise. Structural domain/type changes must be explicit.

Chart-level object data and React data keys normalize once per changed input; typed streaming remains
a bulk path. Decide shared-column ownership from real composed-chart callers and measurements before
adding public dataset machinery. Host accessors never run inside frame or hit-test loops. Hierarchy
and flow input require stable node/link identities, validation of references/cycles as applicable,
defined ordering and depth/size/work caps; they must not be forced into XY rows.

### Layout, scales and shared geometry

Resolve data domains and runtime views separately. The same transform must drive ticks, grids,
geometry, hits, brushes, references and accessibility. Complete temporal interval selection and
formatting with an explicit deterministic timezone policy; never silently use the browser timezone.
Category zoom/pan and duplicate labels need declared semantics. Test extreme and degenerate domains.

Layout reserves chart content, titles, legends, axis strips and plot regions through bounded passes.
Define behavior when text/axes do not fit rather than letting clipping or layout oscillation decide.
Font, DPR, formatter and locale changes invalidate measurements. Host measurement is permitted;
host-owned tick selection or autoscale is not.

General grids use resolved general ticks in the ordered background layer. Reference/background fills,
series, interaction chrome and labels need explicit order and clips. New curves, sectors, polygons,
symbols, rounded shapes or gradients must have shared geometry/lowering and every executor implemented
before their public feature is complete. Retain f64 until the documented encoding boundary.

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
A row cap alone does not prove bounded interactive latency. Measure worst-case dense/overlapping hits,
stack alignment, category unions, long labels, many axes/series and hierarchy depth. Reuse existing
LOD/index mechanisms where appropriate; add optimizations only after release measurements.

Persist new semantic configuration with versioned migrations and transactional restore. Preserve V1
financial compatibility and current V2 contracts; version schema extensions when compatibility requires
it. Runtime callbacks, DOM, device resources and transient animation/hover/focus are not serialized.
Restored live handles must not alias stale handles. State exports and image exports are separate gates.

## Delivery sequence and exit gates

All phases below are open. Existing implementation counts toward them only after its required behavior
is demonstrated. Implement the next dependency-complete vertical slice; do not delay all fixes until a
large framework rewrite or skip foundation work to add a demo chart.

### R0 — Establish the auditable competitive baseline

1. Pin Recharts release/source and retain the existing financial competitor baseline. Inventory public
   props/components and map every relevant capability to the matrix above with supported combinations.
2. Add reference fixtures for each matrix row and record intentional differences. Keep executable fixtures
   in existing test infrastructure; transient screenshots/reports stay out of committed documentation.
3. Reconcile `General_charts_api.md`, `Public_api.md`, examples and architecture claims against exports,
   manifests, scripts and actual call paths. Separate supported, experimental and proposed behavior.
4. Capture clean release financial/general/combined baselines and current enforced budgets before code changes.

Exit: every required capability has a scoped owner, dependency, fixture and honest status; no unsupported
example or historical PASS substitutes for current evidence. Version/date and evidence paths accompany
future status changes. Unverified rows stay open.

### R1 — Repair lifecycle and mutable object foundations

Depends on R0. Implement initial general-domain creation, last-pane ownership and atomic in-place
axis/series mutations through all public boundaries. Fix failed installation/cleanup and ensure
visibility and ordering affect domains, legends, hits and exports consistently.

Exit: standalone general and mixed charts can create, update, rebind, reorder, hide, remove and restore
through actual browser and native paths. Invalid operations leave prior state intact. Handles, explicit
row focus/selection and unaffected views survive routine changes; repeated mount/dispose releases resources.

### R2 — Complete scale, axis and responsive layout contracts

Depends on R1. Finish temporal ticks/formatting/view operations, category behavior, explicit ticks and
formatters, numeric extremes, grid/zero lines, domain padding/clipping, multiple axes, titles and small
container behavior. Correct the reviewed silent-option gaps before declaring axes complete.

Exit: deterministic domain/coordinate round trips and frame fixtures cover all supported scale/orientation
combinations. Browser resize/font/DPR and pointer/keyboard view tests agree with native/GPUI output;
no accepted option silently does nothing and layout work has an enforced bound.

### R3 — Finish Cartesian visual and data semantics

Depends on R1-R2. Complete the Cartesian matrix, including curve/gap/baseline policies, range bars,
stack offsets/order, styles, symbols, gradients, error bars, composition, and per-item customization.
Audit existing box/heatmap/bubble behavior rather than rewriting completed storage and geometry.

Exit: each family passes object/typed ingestion, atomic updates, missing/duplicate/extreme data,
visibility/stack changes, exact/nearest hits, labels, accessibility, persistence and executor parity.
Reference examples demonstrate visual configurability; high-density fixtures demonstrate bounded work.

### R4 — Deliver complete chart components and interaction

Depends on R1-R3. Ship usable legends/tooltips, titles/labels, references, keyboard/touch brush controls,
selection and linked-chart synchronization. Complete localization, overflow, focus and export behavior.
Linked-chart synchronization and frame image export are shared engine contracts delivered first for
financial charts as [Expansion.md](Expansion.md) PD5 and PD6. R4 extends those same contracts to
general domains and chrome; it does not build a second synchronization or export path.

Exit: consumers build an interactive dashboard from published APIs without demo-owned semantic logic.
Legend toggles preserve identity, brushing survives resize, synchronization handles unequal datasets
without loops, and all controls are keyboard/screen-reader usable with bounded snapshots.

### R5 — Deliver competitive React and framework-neutral authoring

Depends on R1-R4. Add composable components and typed data mapping over canonical handles. Provide
controlled/uncontrolled behavior where applicable, events, documented defaults and migration recipes.
Support equivalent imperative composition for hosts that do not use React.

Exit: packed-consumer examples cover standalone, composed, synchronized and financial/general charts.
Prop changes retain engine identities; failure/Strict Mode/concurrent lifecycle tests pass. SSR import,
hydration setup, bundler/WASM asset resolution and cleanup work without repository paths.

### R6 — Deliver polar families and shared transitions

Depends on R2-R5. Add angular/radial transforms and shared sector/polygon geometry, then pie/donut,
radar, radial bar and polar area. Implement shared general transitions, including existing Cartesian
families, with interruption and reduced-motion behavior.

Exit: all polar variants cover degenerate/zero/missing data, angles/radii, label collision, legends,
selection, keyboard navigation, persistence and every executor. Fixed-clock transition fixtures prove
repeatability, correct interaction targets, bounded memory and zero idle animation scheduling.

### R7 — Deliver required hierarchy and flow families

Depends on the lifecycle, layout and primitive contracts above. Implement funnel, treemap, Sankey and
sunburst as dedicated engine layout families with typed validated inputs. Use shared polar geometry
for sunburst where appropriate; do not reuse incompatible XY storage merely to avoid a proper owner.

Exit: each family has deterministic ordering/layout, declared cycle/depth/size policies, bounded work,
update/transition behavior, labels/styles, exact hits, tooltip/legend behavior where applicable,
accessibility, persistence and cross-backend fixtures. These families are required for competitive closure.

### R8 — Close competitive parity and release readiness

Depends on R0-R7. Run the complete matrix against the pinned competitor and demonstrate financial-only,
general-only and combined workloads. Finish documentation, customization/migration examples, export,
backend fallback/device recovery and clean-install evidence. Recheck upstream scope before claiming parity;
new upstream features must be recorded and assessed explicitly.

Exit: every required matrix row is verified or has a maintainer-approved, clearly documented semantic
alternative that satisfies the user task. No required family remains demand-deferred. Publish capability
claims only for verified behavior, with measured startup/size/frame/input/memory results and known limits.

## Verification and evidence policy

Each implementation starts with a failing regression or measurable invariant through its actual public
host/executor path. Shared-engine unit tests alone cannot close a browser or rendering requirement.
Run the complete gates required by [AGENTS.md](AGENTS.md) before committing code:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy -p nucleuscharts_wasm --target wasm32-unknown-unknown --locked -- -D warnings
cargo test --workspace --locked
cargo run -p nucleuscharts_native --example perf_gate --release

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
Run performance thresholds in strict mode as CI does; never relax tests or budgets to conceal regressions.

Use `benchmarks/benchmark.mjs` and `benchmarks/budgets.json` for versioned release evidence. Current
policy v3 includes a 2,000 ms general-dashboard startup ceiling and 100,663,296-byte first-frame upload
ceiling, alongside package-size limits. These limits are existing guards, not a declaration that their
ceilings are competitive targets. Preserve them until measured evidence supports an explicit revision.
Add family-specific budgets before closure, including p95 input/frame latency, steady-state allocation,
retained CPU/GPU memory, upload work, cold startup and repeated disposal. Compare equal data, viewport,
DPR, interactions and release builds; disclose hardware/browser/font versions and unsupported metrics.

A matrix entry can be marked **verified** only with a commit/revision, exact fixture commands and results,
backend/browser coverage, relevant performance evidence, and recorded manual checks. Screenshots must
cover small and large containers, light/dark themes, long/Unicode labels, overflow and active states.
Accessibility requires interaction and assistive-technology review, not just snapshot existence.

Documentation-only revisions may skip runtime gates. Check diffs, links/paths, source consistency and
documentation hygiene. This plan revision makes no production ownership or execution change; update
`Architecture.md` in the same commit as future code that changes those contracts, and correct current
wording discrepancies during R0. Preserve unrelated working-tree changes and stage only task-owned files.

## Definition of completion

Nucleus is competitively complete for this plan when a consumer can build the full required Recharts
capability matrix through a coherent published API, combine it with the established financial product,
and rely on equivalent semantic output across supported backends. Routine changes retain identity;
invalid updates are atomic; controls are accessible; styles and layouts are deliberate; resources and
work are bounded; installation and migration are documented; and all release gates have current evidence.

Until then, report completed slices and remaining gaps precisely. A working demo or a green subset of
tests is progress, not completion of the all-in-one library.
