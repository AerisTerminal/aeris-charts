# Origin Charts GPUI Engine Implementation Plan

**Hand this document directly to the implementation agent.**

**Target consumer:** **AxiusFlow is the GPUI platform where Origin Charts will be used.**
**Scope now:** make the Origin Charts engine and renderer fully ready for AxiusFlow's future
GPUI integration.
**Not in scope now:** building or migrating the AxiusFlow application itself.
**Expected duration:** **10–20 focused working days (2–4 weeks)** with an experienced engineer
and AI assistance.
**First GPUI-rendered chart target:** **2–3 working days**.
**GPUI source:** official `zed-industries/zed` only.

---

## 1. Agent mission

Add official-GPUI rendering support to Origin Charts without turning Origin into a
framework-specific engine.

Origin already emits a backend-neutral ordered `DrawList<Prim>`. WebGPU, Canvas2D, and
tiny-skia already consume that representation. Add a fourth optional executor,
`origin_render_gpui`, that translates the same prepared frame into official GPUI scene
primitives.

AxiusFlow will later create the GPUI application/window and feed Origin's prepared chart frame
to this adapter. Do not build AxiusFlow in this task. Provide a minimal GPUI probe only where it
is necessary to compile, render, test, and benchmark the adapter.

Work through the phases in order. Complete and validate each gate before continuing. Do not
weaken a gate to finish faster.

---

## Current workspace status — read before changing anything

### Status legend

This handoff distinguishes implementation presence from fresh verification:

- `[x]` means the implementation or artifact is present in the current workspace.
- `[ ]` means work or fresh verification still has to be performed by the receiving agent.
- **Committed baseline** means the capability already exists at `HEAD`.
- **Dirty-tree implementation** means the code exists in the current working tree but is not
  staged or committed. It must be preserved and freshly validated; do not describe it as newly
  published based only on this workspace.

### Already implemented in the committed baseline — keep all of it

- [x] **Item 1 — frame/backend telemetry:** frame statistics and memory/backend measurements.
- [x] **Item 2 — typed streaming append:** typed column APIs and allocation-conscious batch
      updates.
- [x] **Item 3 — SharedArrayBuffer ring source, initial implementation:** frame-driven ring
      draining and producer/consumer layout support.
- [x] **Item 6 — memory ceiling/windowing:** `max_points`, oldest-first retention, and memory
      observability.
- [x] **Item 7 — build configuration:** the accepted release/build optimization configuration.
- [x] Existing WebGPU, Canvas2D, tiny-skia/native, wasm, TypeScript, plugin, drawing, indicator,
      accessibility, and resize paths.

These are foundations for the GPUI adapter, not temporary experiments. Do not remove, revert,
rename, or replace them while implementing GPUI support.

### Implemented in the current dirty tree — preserve and revalidate

At handoff, `git status` shows **29 modified tracked files** plus untracked files. The following
feature work is already implemented in that dirty tree:

- [x] **Item 3 hardening:** per-slot sequence/seqlock validation, stable-window retry, a reused
      slab, batched engine application, overrun accounting, and expanded producer/tests.
- [x] **Item 4 — axis/crosshair in the backend-neutral primitive pass:** axis borders, ticks,
      separators, boxed labels, crosshair lines/labels, and watermark are converted into the
      shared `Prim` stream. WebGPU emits a final unscissored axis group; Canvas2D consumes the
      same axis primitives. Existing browser-rasterized text caching is retained.
- [x] **Item 5 — OffscreenCanvas worker rendering:** additive worker-safe wasm constructor,
      TypeScript `offscreen_chart` facade, normalized input injection, explicit resize/DPR,
      runtime WebGPU-to-Canvas2D fallback, cleanup, worker example, and worker tests.
- [x] Tests/specs and documentation for the dirty-tree work are present.

Key dirty-tree implementation paths that must not be lost include:

```text
crates/origin_engine/src/lib.rs
crates/origin_wasm/Cargo.toml
crates/origin_wasm/src/chart.rs
crates/origin_wasm/src/chart/inner_api.rs
crates/origin_wasm/src/chart/inner_render.rs
crates/origin_wasm/src/chart/primitives.rs
crates/origin_wasm/src/chart/ring.rs
crates/origin_wasm/src/chart/text_runs.rs
crates/origin_wasm/src/ring_source.rs
crates/origin_wasm/src/telemetry.rs
packages/charts/src/impl.ts
packages/charts/src/index.ts
packages/charts/src/primitives.ts
packages/charts/src/types.ts
packages/charts/src/offscreen.ts                 untracked at handoff
examples/web_demo/offscreen_chart_worker.js      untracked at handoff
examples/web_demo/tests/offscreen-worker.spec.mjs untracked at handoff
examples/web_demo/tests/*.spec.mjs                related modified tests
```

Use `git status --short` as the authoritative complete inventory; the list above highlights the
feature-critical files rather than replacing the Git inventory.

### Validation status of existing dirty-tree work

The code and tests are present, and `docs/AXIUSFLOW_PERF_REQUESTS.md` records these prior results:

- [x] Rust formatting, Clippy, wasm-target Clippy, and workspace tests are reported as passing.
- [x] Package typecheck, lint, release build, and package smoke are reported as passing.
- [x] The explicit frame-statistics, crosshair, worker, ring-source, and typed-allocation subset
      is reported as **24/24 passing**.
- [x] Native strict performance gates are reported as passing.
- [x] The default Chromium run is reported as **117 passed, 4 known baseline failures, 1
      skipped**; do not misrepresent that report as a fully green browser suite.
- [x] The existing WebGPU-versus-Canvas2D DPR 1.5 reference records a bounded residual of 2,550
      edge pixels with maximum channel delta 43. This is part of the current baseline; GPUI must
      match the approved current output rather than changing it.
- [ ] Freshly rerun all required validation on the exact handoff tree before using it as the
      GPUI baseline.
- [ ] Record raw command output and distinguish new failures from the four documented browser
      baseline failures.
- [ ] Confirm commit/publication state with the owner before claiming the dirty-tree work is
      committed or released.

The planning audit did not rerun builds or tests. Therefore, implementation presence is marked
complete, while fresh validation remains deliberately unchecked.

### GPUI implementation status — not started

- [x] Official GPUI source research and architecture assessment are complete.
- [x] Official source revision inspected during planning:
      `200fb85c902b823ccc8bec3ae87fda3059424e27`.
- [x] The direct `Prim -> GPUI Scene` adapter strategy is selected.
- [ ] No GPUI dependency has been added.
- [ ] No official GPUI revision has been selected as the implementation dependency pin.
- [ ] No `origin_render_gpui` crate exists.
- [ ] No GPUI executor, text adapter, cache, metrics, probe, parity capture, or benchmark exists.
- [ ] No AxiusFlow GPUI integration has started.

All unchecked GPUI work in this document is the receiving agent's task.

### Mandatory preservation protocol

Before making the first GPUI source change, the receiving agent must:

- [ ] Read the current `git status --short` and `git diff`.
- [ ] Treat the current dirty tree as user-owned work.
- [ ] Preserve every tracked and untracked feature file listed by Git.
- [ ] Revalidate the current tree and record the baseline without rewriting it.
- [ ] Add GPUI support on top of the current architecture.

The receiving agent must **not** run `git reset --hard`, `git clean`, broad `git restore`, broad
`git checkout`, or any revert that discards the existing work. Do not remove Item 3 hardening,
Item 4 axis/crosshair primitives, Item 5 OffscreenCanvas support, or Items 1/2/6/7. Do not create
a commit unless the owner explicitly requests one. If GPUI work conflicts with existing dirty
changes, preserve both behaviors and resolve the integration at the optional adapter boundary.

---

## 2. Definition of done

The engine is GPUI-ready when all items below are complete:

- [ ] A new optional `crates/origin_render_gpui` crate exists.
- [ ] It consumes the current backend-neutral frame/`DrawList<Prim>` without rebuilding chart
      models or layout.
- [ ] Every current `Prim` variant has a GPUI implementation.
- [ ] Paint order, clipping, opacity, blending intent, pixel snapping, and DPR handling are
      preserved.
- [ ] Chart text has a documented and tested measurement/rasterization path.
- [ ] Approved reference fixtures are pixel-identical to the current chart output.
- [ ] Current WebGPU, Canvas2D, tiny-skia, wasm, and TypeScript paths remain unchanged in
      behavior and output.
- [ ] `origin_core`, `origin_engine`, and `origin_render` have no GPUI dependency or GPUI type.
- [ ] GPUI is compiled only when the GPUI adapter is selected.
- [ ] The adapter exposes a clean API that AxiusFlow can call from its future GPUI chart
      element.
- [ ] A minimal probe proves the adapter renders, resizes, handles fractional DPR, and updates
      frames.
- [ ] Scene construction and memory behavior meet the performance gate.
- [ ] An independent agent can reproduce all validation from a clean checkout.

If one of these items is unverified, the task is not complete.

---

## 3. Non-negotiable constraints

### 3.1 Preserve framework independence

The dependency direction must remain:

```text
origin_core
    -> origin_engine
        -> origin_render::DrawList<Prim>
            -> origin_render_wgpu       existing
            -> Canvas2D executor        existing
            -> origin_native            existing
            -> origin_render_gpui       new optional leaf adapter
```

Forbidden:

```text
origin_core   -X-> gpui
origin_engine -X-> gpui
origin_render -X-> gpui
origin_wasm   -X-> gpui
packages/charts -X-> gpui
```

Rules:

1. No GPUI imports, types, events, lifecycle assumptions, or `#[cfg]` branches in
   `origin_core`, `origin_engine`, or `origin_render`.
2. Backend-neutral additions to `origin_render` are allowed only when at least two renderers
   can use them and their public types contain no GPUI detail.
3. All GPUI-specific conversion and cache state belongs in `origin_render_gpui`.
4. AxiusFlow-specific application policy does not belong in Origin's core or renderer.

### 3.2 Preserve current behavior

Do not remove, rename, or weaken:

- existing Rust or TypeScript APIs;
- WebGPU or Canvas2D rendering;
- the wasm/browser host;
- OffscreenCanvas support;
- plugins, drawings, indicators, gestures, accessibility, or resize behavior;
- existing build and performance features.

Any source change outside the new GPUI crate must be minimal, additive, and justified in the
final report.

### 3.3 Preserve visual identity

The requirement is the same chart output, not a GPUI-inspired redesign.

For approved deterministic fixtures:

- compare canonical RGBA output;
- the default allowed number of differing pixels is zero;
- antialiasing, text, baseline, gamma, or one-pixel differences are still differences;
- do not blur, threshold, mask, or normalize images to manufacture a pass;
- do not change the existing web output to make GPUI easier to match.

If exact identity cannot be achieved because official GPUI owns rasterization differently, stop
and report the exact blocker, affected fixtures, differing-pixel count, maximum channel delta,
and attempted remedies. Do not silently redefine pixel identity.

### 3.4 Official GPUI only

- Use only `https://github.com/zed-industries/zed` or an official publication from that project.
- Do not use `gpui-unofficial`, another fork, copied GPUI source, or an unofficial repackaging.
- Pin an exact audited official Zed revision; never build production work against moving
  `main`.
- The previously inspected official revision was
  `200fb85c902b823ccc8bec3ae87fda3059424e27`. Confirm it still suits the workspace before
  selecting the implementation pin.

---

## 4. Existing architecture to reuse

Do not rediscover or rewrite these pieces:

| Existing layer | Reuse for GPUI |
|---|---|
| `origin_core` | Domain types and options; unchanged |
| `origin_engine` | Models, scales, layout, autoscale, hit testing, frame generation; unchanged |
| `origin_render` | Ordered backend-neutral `Prim` IR and pixel math |
| `origin_render_wgpu` | Reference for lowering rectangles, geometry, ordering, and text quads |
| `origin_native` | Deterministic diagnostic rasterizer and PNG generation |
| `origin_wasm` | Current browser behavior and shipping visual reference |
| `packages/charts` | Current public behavior consumed by AxiusFlow |

Current `Prim` coverage includes:

- `Rect`;
- `RectFrame`;
- `HLine`;
- `VLine`;
- `Polyline`;
- `AreaFill`;
- `BandFill`;
- `RoundRect`;
- `Circle`;
- `Triangle`;
- `Background`;
- `Text`.

The WebGPU executor already demonstrates how rectangle families become quads, other geometry
becomes triangles, text becomes atlas quads, and ordering remains stable. Reuse those semantics.

`origin_native` is not yet the sole full-chart pixel oracle because browser-owned text/font
behavior and some full-chart coverage differ. Use the actual current browser/AxiusFlow chart
output as the primary visual reference. Use native output to isolate geometry and frame-generation
problems.

---

## 5. GPUI integration facts already established

The official source study is complete enough to start implementation:

1. GPUI custom elements use `request_layout -> prepaint -> paint`.
2. `Window::paint_quad` supports rectangle-family output.
3. `Window::paint_path` and `Path::push_triangle` support arbitrary Origin-tessellated
   geometry.
4. `Window::paint_image` can place cached raster/image data through GPUI's atlas.
5. GPUI owns final `PlatformWindow::draw(&Scene)` submission and presentation.
6. A normal GPUI `Window` does not expose a portable shared GPU device or custom render-pass
   injection API.
7. Windows GPUI uses D3D11/DirectWrite, macOS uses Metal/native text, and Linux uses GPUI's WGPU
   renderer.
8. Therefore, shared WGPU or an injected Origin render pass is not the primary architecture.
9. GPUI provides fractional `f32` scale factors; the adapter must preserve Origin's DPR and
   snapping rules.
10. GPUI remains pre-1.0, so the exact official revision must be pinned.

Official references:

- [GPUI README](https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md)
- [Element lifecycle](https://github.com/zed-industries/zed/blob/200fb85c902b823ccc8bec3ae87fda3059424e27/crates/gpui/src/element.rs#L51)
- [`Window::paint_quad`](https://github.com/zed-industries/zed/blob/200fb85c902b823ccc8bec3ae87fda3059424e27/crates/gpui/src/window.rs#L3966)
- [`Window::paint_path`](https://github.com/zed-industries/zed/blob/200fb85c902b823ccc8bec3ae87fda3059424e27/crates/gpui/src/window.rs#L4056)
- [`Path::push_triangle`](https://github.com/zed-industries/zed/blob/200fb85c902b823ccc8bec3ae87fda3059424e27/crates/gpui/src/scene.rs#L876)
- [`PlatformWindow::draw`](https://github.com/zed-industries/zed/blob/200fb85c902b823ccc8bec3ae87fda3059424e27/crates/gpui/src/platform.rs#L804)
- [Official Windows support](https://zed.dev/blog/zed-for-windows-is-here)

---

## 6. Deliverables

### 6.1 Required new crate

```text
crates/origin_render_gpui/
    Cargo.toml
    src/
        lib.rs
        executor.rs       ordered Prim dispatch
        geometry.rs       quads and triangle/path conversion
        text.rs           text measurement/raster/cache integration
        image_cache.rs    stable atlas/image keys and invalidation
        metrics.rs        adapter timing/count metrics
    tests/
        parity.rs
        ordering.rs
        clipping.rs
```

Adjust module names only if the existing repository conventions strongly favor another layout.
Do not create unnecessary abstraction layers.

### 6.2 Minimal probe

Provide the smallest official-GPUI element/example necessary to:

- open a test chart;
- feed a prepared Origin frame to `origin_render_gpui`;
- resize it;
- change DPR/scale factor;
- update data and repaint;
- capture or otherwise validate output;
- collect scene-construction metrics.

This probe is not AxiusFlow and must not grow into an application framework. AxiusFlow is the
target GPUI platform and will consume the finished adapter later.

### 6.3 Adapter API

Expose a small, documented API that a future AxiusFlow GPUI chart element can call during its
paint phase. Design it around actual repository types, but preserve this conceptual boundary:

```rust
// Conceptual only; use the repository's real frame, viewport, and error types.
pub fn paint_frame(
    &mut self,
    frame: &PreparedOriginFrame,
    viewport: OriginViewport,
    scale_factor: f32,
    window: &mut gpui::Window,
) -> Result<GpuiFrameMetrics>;
```

Requirements:

- consume prepared immutable frame data;
- do not update market data, models, indicators, or layout while painting;
- do not hold engine/data locks during GPUI scene emission;
- return backend metrics without leaking GPUI types back into the engine;
- support stable caches across frames;
- make resource invalidation explicit.

### 6.4 Required final report

At completion, create or update a concise implementation report containing:

- exact official GPUI revision and dependency features;
- files changed;
- `Prim` mapping table and any special cases;
- commands run and results;
- parity results and image hashes;
- performance results;
- known limitations or blockers;
- instructions for AxiusFlow's future GPUI element to call the adapter.

---

## 7. Primitive implementation map

Implement in this order:

| Order | Origin primitive | GPUI approach |
|---:|---|---|
| 1 | `Background`, `Rect` | `paint_quad` |
| 2 | `RectFrame`, `RoundRect` | bordered quad or Origin-generated strips |
| 3 | `HLine`, `VLine` | Origin-snapped thin quads |
| 4 | `Triangle`, `Circle` | Origin geometry through GPUI path triangles |
| 5 | `Polyline` | reuse Origin tessellation; emit triangle/path geometry |
| 6 | `AreaFill`, `BandFill` | reuse Origin triangles and preserve gradients/blending |
| 7 | `Text` | §8 text plan |

Rules for every mapping:

1. Preserve original draw-list order.
2. Preserve the current clip/content-mask behavior.
3. Use Origin's coordinate, bar-width, and snapping calculations.
4. Do not let GPUI layout recalculate chart geometry.
5. Do not merge semantic layers unless blending and ordering remain pixel-identical.
6. Cache only with complete invalidation keys.
7. Record emitted quad, path, triangle, text, and image counts.

---

## 8. Text plan

Text is the highest-risk part of pixel identity. Do not postpone it until the end.

### Stage 1: record current behavior

Capture the real current chart text inputs:

- font files/families;
- weight and style;
- size and DPR;
- baseline and alignment;
- label bounds and clipping;
- numeric formatting;
- locale and timezone;
- fallback and missing glyph behavior.

### Stage 2: try official GPUI text

Use GPUI's official text system with the same font inputs and compare:

- measured width and height;
- baseline position;
- glyph placement;
- full RGBA output.

### Stage 3: remediate if needed

If GPUI-native text differs:

1. confirm whether the mismatch is measurement, shaping, glyph rasterization, atlas filtering,
   gamma, or placement;
2. reuse or promote a backend-neutral Origin text measurement/raster request where necessary;
3. use an Origin-owned cached text atlas/image path for chart text if that can restore identity;
4. keep GPUI-native text available only if it passes the approved identity gate.

Do not change browser text behavior to match GPUI. Do not claim identity using only similar
metrics; compare pixels.

---

## 9. Execution schedule

### Phase 0 — Baseline and dependency audit

**Time:** 0.5–1 day

Tasks:

- [ ] Read workspace `Cargo.toml` and affected crate manifests.
- [ ] Confirm the complete current `Prim` definition.
- [ ] Read the WebGPU executor and text atlas path.
- [ ] Read the native executor and existing golden/parity tests.
- [ ] Record current build/test commands.
- [ ] Record clean baseline output and performance fixtures.
- [ ] Select and pin one exact official Zed revision.
- [ ] Confirm no unofficial GPUI package enters the dependency graph.

**Gate:** baseline is reproducible and dependency choice is documented.

### Phase 1 — Crate scaffold and first frame

**Time:** 1–2 days
**Cumulative target:** first GPUI-rendered chart in 2–3 working days.

Tasks:

- [ ] Add `origin_render_gpui` as an optional workspace crate.
- [ ] Add only the required official GPUI dependencies at exact revision.
- [ ] Define the adapter entry point and persistent cache state.
- [ ] Build the minimal probe.
- [ ] Render background, grid, and one rectangle-based series.
- [ ] Verify resize and fractional DPR propagation.
- [ ] Add initial scene-build metrics.

**Gate:** a real Origin-prepared frame renders through GPUI without GPUI changes below the
adapter crate.

### Phase 2 — Complete geometry executor

**Time:** 2–3 days

Tasks:

- [ ] Implement rectangle/frame/rounded-rectangle variants.
- [ ] Implement horizontal and vertical lines with exact snapping.
- [ ] Implement triangle and circle geometry.
- [ ] Implement polyline geometry, joins, caps, widths, and dashes.
- [ ] Implement area and band fills, gradients, and opacity.
- [ ] Preserve clipping and strict paint order.
- [ ] Add focused ordering, clipping, and geometry tests.

**Gate:** every non-text `Prim` renders; no full-frame bitmap and no shared-WGPU assumption is
used.

### Phase 3 — Text and full-chart coverage

**Time:** 2–5 days

Tasks:

- [ ] Implement the staged text plan in §8.
- [ ] Render price and time axes, tick labels, boxed labels, crosshair labels, watermark, title
      chips, and countdown text.
- [ ] Cover all series, overlays, indicators, drawings, price lines, and multi-pane output.
- [ ] Validate light/dark/custom themes.
- [ ] Validate DPR 1.0, 1.25, 1.5, 2.0, and 2.5.
- [ ] Validate odd/even and fractional viewport geometry.

**Gate:** every current `Prim` and chart-owned visual layer is present. Remaining pixel
differences are enumerated exactly.

### Phase 4 — Pixel-identity remediation

**Time:** 2–5 days, only as needed

For every non-zero diff:

- [ ] Record fixture, DPR, bounding box, differing-pixel count, and maximum channel delta.
- [ ] Classify the cause: frame data, snapping, clipping, geometry, blending, color, text,
      atlas, or capture.
- [ ] Fix the GPUI adapter without changing existing backend output.
- [ ] Rerun all affected fixtures and then the complete matrix.
- [ ] Keep a machine-readable result and a human-visible diff image.

**Gate:** approved reference captures have zero differing pixels. If this is technically
impossible with official GPUI, stop and report the blocker; do not alter current output or fork
GPUI without approval.

### Phase 5 — Performance and cache hardening

**Time:** 1–3 days

Tasks:

- [ ] Remove avoidable allocations in steady-state scene construction.
- [ ] Cache immutable geometry and text/image resources with bounded memory.
- [ ] Verify cache invalidation for data, options, theme, viewport, and DPR changes.
- [ ] Test 10K, 100K, and 1M source-point fixtures at representative visible ranges.
- [ ] Test live append, pan/zoom frame generation, resize, and theme change.
- [ ] Run at least a 10-minute update replay and verify stable memory.
- [ ] Confirm current non-GPUI benchmarks did not regress.

**Engine-adapter targets on the agreed reference machine:**

- p99 GPUI scene-construction overhead at or below 2 ms for the standard dense visible-range
  fixture;
- no unbounded cache, atlas, geometry, or allocation growth;
- no full-frame CPU raster/upload in the primary path;
- no regression attributable to the optional GPUI crate when GPUI is disabled.

**Gate:** targets pass with raw benchmark output retained.

### Phase 6 — Final verification and AxiusFlow handoff

**Time:** 0.5–1 day

Tasks:

- [ ] Run targeted tests for every changed crate.
- [ ] Run workspace checks required by repository policy.
- [ ] Verify current web/native reference output remains unchanged.
- [ ] Verify `cargo tree` dependency isolation.
- [ ] Verify all `Prim` variants are covered.
- [ ] Produce the final report in §6.4.
- [ ] Document the exact call boundary AxiusFlow will use from its future GPUI chart element.

**Gate:** the independent checklist in §11 passes.

### Total realistic duration

| Result | Expected time |
|---|---:|
| First real Origin frame in GPUI | 2–3 working days |
| All non-text primitives | 4–6 working days cumulative |
| Complete adapter with text and chart coverage | 7–12 working days cumulative |
| Exact-parity remediation and performance hardening | 3–8 additional working days |
| **Fully GPUI-ready engine** | **10–20 working days / 2–4 weeks** |

This estimate excludes building AxiusFlow's GPUI application. AxiusFlow is the target platform,
but it can integrate the finished engine adapter afterward.

---

## 10. Required validation matrix

Use deterministic fixtures that collectively cover:

### Chart content

- candlestick, bar, line, area, histogram, and Heikin-Ashi;
- volume overlay and separate volume pane;
- multiple indicators and scales;
- left, right, and dual price scales;
- time axis and session gaps;
- grid, crosshair, labels, watermark, title chips, and countdown;
- price lines;
- drawings, handles, hover, selected, and editing visuals;
- one and multiple panes;
- empty, sparse, dense, and partially visible data;
- light, dark, transparent, and custom themes.

### Geometry and environment

- DPR 1.0, 1.25, 1.5, 2.0, and 2.5;
- odd/even viewport sizes;
- small and large chart bounds;
- fractional pane sizes;
- every clipping edge;
- positive and negative coordinates where currently valid;
- gradients, opacity, and dashed lines.

### Text

- all chart fonts used by AxiusFlow;
- weights/styles used by current chart output;
- numeric labels, negative values, percentages, long prices, and scientific notation;
- representative Unicode and fallback glyphs;
- representative locales and timezones, including DST boundaries.

### Update behavior

A full AxiusFlow input adapter is not part of this task, but the engine adapter must render the
correct resulting states for:

- crosshair movement;
- pan and zoom results;
- visible-range changes;
- drawing edits;
- scale changes;
- live appends;
- pane resize;
- chart resize and DPR change;
- theme, locale, and timezone change.

---

## 11. Independent completion checklist

A second agent must verify this from a clean checkout.

### Architecture

- [ ] GPUI is imported only by the new optional adapter/probe.
- [ ] `origin_core`, `origin_engine`, and `origin_render` remain GPUI-free.
- [ ] The engine still emits one backend-neutral ordered frame.
- [ ] GPUI does not trigger model, layout, or indicator recomputation during paint.
- [ ] No AxiusFlow application code was implemented or modified by this engine task.
- [ ] The report clearly states AxiusFlow is the target GPUI consumer.

### Dependency

- [ ] GPUI resolves only from official `zed-industries/zed` or its official publication.
- [ ] The exact revision is pinned and recorded.
- [ ] No unofficial fork or package is present.
- [ ] Normal non-GPUI builds do not compile GPUI.

### Rendering

- [ ] Every current `Prim` variant is mapped.
- [ ] Paint order and clipping tests pass.
- [ ] DPR and snapping tests pass.
- [ ] Text and font cases pass.
- [ ] Current chart features are all represented.
- [ ] Canonical approved captures have zero differing pixels.
- [ ] No image normalization or tolerance hides differences.

### Preservation

- [ ] Existing public APIs remain compatible.
- [ ] Existing WebGPU, Canvas2D, native, wasm, and TypeScript checks pass.
- [ ] Existing reference hashes remain unchanged.
- [ ] No unrelated web capability was removed.

### Performance

- [ ] Scene-build p50/p95/p99 results are attached.
- [ ] Standard dense-fixture p99 meets the target.
- [ ] Memory remains bounded during replay.
- [ ] Primary rendering does not upload a full CPU chart bitmap each frame.
- [ ] GPUI-disabled builds show no attributable regression.

### Handoff verdict

The verifier must return one result:

- **READY:** every required item passes; AxiusFlow can begin integrating the adapter.
- **BLOCKED:** architecture is complete but one or more exact blockers are documented.
- **NOT READY:** required functionality, identity, isolation, or performance is missing.

Do not return READY with an unexplained or hidden visual difference.

---

## 12. Stop conditions

Stop implementation and report before taking any of these actions:

- adding GPUI to `origin_core`, `origin_engine`, or `origin_render`;
- modifying current chart visuals to make native matching easier;
- adopting `gpui-unofficial` or another fork;
- patching or forking official GPUI;
- relying on a shared WGPU device or injected render pass;
- replacing the primary path with per-frame full-chart CPU rasterization;
- removing a current web feature;
- expanding the minimal probe into the AxiusFlow application;
- accepting a non-zero pixel diff without explicit written approval.

When stopped, report the attempted approach, evidence, exact blocker, available alternatives,
and estimated impact. Preserve the current working paths.

---

## 13. Final implementation principle

Origin Charts remains one framework-agnostic engine with one ordered backend-neutral rendering
contract and multiple replaceable executors. `origin_render_gpui` is one additional executor.

AxiusFlow is the target GPUI platform that will use this engine. This task prepares the engine
and the stable adapter boundary; AxiusFlow can build its GPUI window, layout, application UI,
and interaction wiring later without requiring chart logic to be rewritten.

Content derived from official Zed sources was rephrased for compliance with licensing
restrictions.
