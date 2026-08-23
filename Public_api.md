# Public API and compatibility policy

## Supported product surface

The supported product is the pre-1.0 browser package `@nucleuscharts/financial`. Its root ESM
entry point and `./design.css` are the only npm export paths. The supported root surface is:

- chart creation and initialization;
- chart, series, time-scale, pane, price-scale, price-line, and drawing handles declared in
  `packages/charts/src/types.ts`;
- pane-local named price scales through `chart.add_price_scale()`, `chart.price_scales()`,
  `chart.move_price_scale()`, `chart.remove_price_scale()`, arbitrary string IDs in
  `chart.price_scale()`/`pane.price_scale()`, and series scale identity/rebinding;
- built-in series, indicators, drawing kinds, options, themes, data ingestion, interactions,
  subscriptions, screenshots, and lifecycle operations declared by those handles;
- first-party broker-neutral trading state, instant/manual confirmation, previews, hit testing,
  semantic style, and typed intent subscriptions exposed by `chart.trading()`;
- default chart accessibility, its additive `chart.accessibility()` singleton handle, compatibility
  `enable_accessibility()`, accessibility options, and keyboard data/drawing operation;
- the additive `wheel_behavior` chart option (`auto`, `pan`, or `zoom`); existing gesture option
  names remain compatible;
- `nucleuscharts_error` and its machine-readable error codes;
- chart-state persistence V1 through `chart.export_state()` and `chart.import_state()`.
- read-only backend diagnostics through `chart.backend_status()`, including the requested and active
  backend, stable fallback stage/reason, secure-context and `navigator.gpu` exposure, and optional
  unstable platform detail. `chart.backend()` retains its existing active-backend return value.

Generated `wasm-bindgen` classes, methods reachable only through implementation objects, telemetry,
benchmark counters, demo globals, fixtures, and test hooks are internal even when JavaScript can
inspect them at runtime. `drawings_json()` is an internal inspection shape, not persistence.

The declaration manifest at `packages/charts/api/public-api-v1.json` records every supported
declaration file. CI runs `npm run check:api`; after deliberate review, update it with
`npm run update:api`.

## Experimental surfaces

Custom series, pane/series/canvas primitives, the exported custom-series/primitive feature packs,
built-in plugin helpers, offscreen-worker charts, split-grid helpers, and shortcut helpers are
public experimental APIs. Their current lifecycle and containment behavior is tested, but their
exact types may change in a pre-1.0 minor release.
Extensions run at host render time, must not re-enter a chart mutation from a render callback, own
their external objects and persistence, and receive teardown exactly once. Callback failures are
contained at the host boundary so one extension cannot prevent other teardown. Arbitrary extension
objects or executable callbacks are never reconstructed from persisted JSON.

## Errors and lifecycle

Predictable failures throw `nucleuscharts_error`, an `Error` subclass with one of these stable
codes: `disposed`, `invalid_handle`, `stale_handle`, `invalid_data`, `invalid_options`,
`unsupported_operation`, `serialization_error`, `persistence_version_error`, `extension_error`,
`renderer_platform_error`, or `resource_limit`.

`chart.remove()` is idempotent. Every operation that needs live chart state throws `disposed`
after removal. Identity fields already held by the caller may still be read. Removed series,
drawings, panes, and price scales throw `stale_handle`; a stale handle never targets a replacement.
Extension cleanup exceptions remain contained and are reported as development warnings.

Named price-scale creation rejects empty/reserved/duplicate/overlong IDs, missing panes, and the
per-pane resource limit without partial mutation. Rebinding to an unknown scale throws
`invalid_options`; removing a built-in or populated scale throws `unsupported_operation`. Named
scale IDs are case-sensitive, pane-local UTF-8 strings of 1-128 bytes, with at most 16 per pane.

Clean batch/current-bar ingestion retains its allocation-free/null diagnostic path. Repaired,
dropped, reordered, deduplicated, rejected, or semantically anomalous input is available through
`series.last_ingestion_diagnostics()`; OHLC anomalies are reported without rewriting values.

## Persistence V1

Persistence schema versioning is independent of the npm package version. V1 contains only:

- ordered pane identities, stretch factors, and preserve-empty flags;
- ordered built-in drawings with persistent ID, kind, pane reference, semantic anchors, and style.

Host market history, series and indicator definitions, chart options, trading positions/orders/
executions/previews/intents, custom extensions, callbacks, subscriptions, selections, interaction
sessions, generations, LOD, drawing bounds/indexes, retained frames, and GPU resources are not
persisted. Hosts restore V1 into a fresh chart, then reinstall host-owned data, series/indicator
configuration, trading state, options, and extensions.

Named price-scale descriptors and series-to-scale bindings are also host-owned. Hosts recreate
named scales in each pane before restoring comparison-series bindings; chart-state V1 is unchanged.

Import checks the whole document before mutation and installs it as one transaction. Imported panes
receive fresh live handle IDs while retaining separate persistent pane IDs; pre-import pane and
price-scale handles therefore become stale. Import is accepted only before drawing IDs have been
issued and while the chart still has its initial pane topology. This prevents an old drawing handle
from retargeting a restored drawing with the same persistent ID.

Limits for untrusted input are 8 MiB per document, 64 panes, 10,000 drawings, 100,000 anchors per
drawing, 250,000 total anchors, 64 KiB text per drawing, and 1 MiB total drawing text. Unknown
optional V1 fields are ignored. Unknown schema versions, drawing kinds, pane references, duplicate
IDs, invalid anchor counts, non-finite/unsafe numbers, and limit violations fail structurally and
leave the chart unchanged. V1 fixtures are compatibility inputs; future versions must retain an
explicit V1 migration path for the documented compatibility window.

## Version policy

While the browser package is below 1.0:

- patch: compatible correctness, security, performance, documentation, and packaging fixes;
- minor: additive stable API, explicitly reviewed experimental API changes, or compatible behavior
  additions;
- major (including the eventual 1.0 boundary): removal/rename/signature change to stable API,
  incompatible stable behavior, or ending a documented persistence compatibility window.

Adding a drawing/series kind is normally a minor package feature, but changing the meaning of an
existing kind is incompatible. Adding a new persistence schema does not invalidate V1; removing V1
support follows the separately documented persistence window and is a major compatibility event.

## Rust distribution

Workspace Rust crates are internal implementation components. They are not published to crates.io,
do not share the npm package version, and carry no standalone semver compatibility promise. Repository
components and licensed integrators may consume an exact Git revision; floating Git dependencies are
unsupported. Public Rust visibility currently supports cross-crate repository boundaries and is
experimental, not a separately distributed product API. The GPUI backend is experimental. Official
Rust publication would require a separate product decision, crate/version finalization, packaging
tests, and release coordination; this repository does not reserve or publish crate names.

## Release policy

Tag publication depends on required Rust, package, and portable Chromium/Firefox/WebKit jobs. It also
checks persistence fixtures, public declarations, Node/SSR import, package contents, and configured
performance budgets. `continue-on-error` is forbidden for portable correctness.

Hardware- and machine-sensitive screenshot hashes, GPU timing, heap sampling, and wall-clock evidence
are calibration diagnostics. They remain non-authoritative and may not be approved merely to make one
runner green. Shared draw-stream parity, clipping/order/frame-contract tests, replay determinism, and
portable browser behavior are the authoritative gates. Scenarios without a configured benchmark
budget remain explicitly report-only.
