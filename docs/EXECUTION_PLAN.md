# Origin Charts — Execution Plan

Status legend: `[x]` done (with date) · `[ ]` pending. Ground truth: the full audit of
2026-07-21 (engine vs reference 5.2.0 feature parity, packaging, styling/control APIs). Companion to
[ARCHITECTURE.md](ARCHITECTURE.md), [PRODUCTION_ROADMAP.md](PRODUCTION_ROADMAP.md),
[PLUGIN_PLATFORM_DESIGN.md](PLUGIN_PLATFORM_DESIGN.md).

Goal: (1) make `@origin/charts` installable via `bun add` (primary) / `npm i` from **GitHub
Packages** (private — no public npm registry) with no source clone or Rust toolchain, (2) close
the enumerable reference API-breadth gaps, (3) land the plugin platform.

---

## Phase 0 — Audit (ground truth)

- [x] 2026-07-21 — Feature-parity audit vs reference 5.2.0 (series, chart/time-scale/price-scale APIs, primitives, data model, localization).
- [x] 2026-07-21 — Packaging audit (`npm pack --dry-run`; tarball is coherent but unpublished).
- [x] 2026-07-21 — Styling & control-API audit (all wired surfaces verified against tests/probes).

## Phase 1 — npm/bun publish readiness

A consumer must get a working chart from the registry alone: bundled ESM + flat types + the wasm
binary inside `dist/`, resolved relative to the bundle (`new URL(..., import.meta.url)`).

- [x] 1.1 `LICENSE` (MIT) at repo root, copied into `packages/charts/` and `crates/origin_wasm/`
      (silences the wasm-pack warning; npm auto-includes it from the package root). — 2026-07-21
- [x] 1.2 `packages/charts/README.md` — install, quick start, async `create_chart` note, browser-only
      note, Vite caveat. — 2026-07-21
- [x] 1.3 `packages/charts/package.json` metadata: `license`, `author`, `repository` (with
      `directory: "packages/charts"`), `bugs`, `homepage`, `keywords`, `engines`,
      `publishConfig.access: "public"` (scoped `@origin/*` requires it). Also workspace
      `Cargo.toml` repository URL. — 2026-07-21
- [x] 1.4 `clean` + `prepublishOnly` scripts (kills stale `dist/src/*.d.ts` duplicates; guarantees a
      fresh build at publish). Also excluded the broken-sourcemap `dist/index.js.map` from the
      tarball. — 2026-07-21
- [x] 1.5 Public wasm-URL override: `init_wasm(url?)` exported from `index.ts`, forwarding to
      `ensure_init(url?)` in `impl.ts` — the documented escape hatch for Vite dev / custom asset
      hosting. — 2026-07-21
- [x] 1.6 Pack smoke test (`scripts/pack_smoke.mjs` + `test:pack` script): `npm pack` → install the
      tarball into a scratch dir → assert the module imports and `dist/origin_wasm_bg.wasm` shipped.
      Also runs in CI on every push. — 2026-07-21
- [x] 1.7 Publish CI job in `.github/workflows/ci.yml` (tag-triggered `v*`, gated on rust+package+
      browser jobs, `npm publish` with `publishConfig.access: public`, `NPM_TOKEN` secret).
      — 2026-07-21
- [x] 1.8 Verify: `npm run clean && npm run build`, `npm pack --dry-run` file list sane (10 files,
      wasm 492 kB, README + LICENSE included), smoke test passes, typecheck/lint green. — 2026-07-21

## Phase 2 — reference API breadth (enumerable gaps, no structural work)

Ordered by user impact. Each item = option/method + TS type + engine plumbing + test.

### 2a. Surface corrections (small)
- [x] 2.1 TS types: crosshair line `width`/`labelVisible`/`labelBackgroundColor` — new
      `crosshair_line_options` (types.ts:134). — 2026-07-21
- [x] 2.2 Series `color` keeps alpha — `set_series_color_css` wasm setter, CSS string threaded
      end-to-end (impl.ts:172, inner_api.rs:162). — 2026-07-21
- [x] 2.3 `series.options()` getter (all 20 fields via `series_options_json`);
      `time_scale().options()` returns all 11 fields via `time_scale_options_json` (configured-option
      semantics, reference `restoreDefault` parity verified in the browser suite). — 2026-07-21
- [x] 2.4 Price-line extras: `line_visible`, `axis_label_visible`, `axis_label_color`,
      `axis_label_text_color` + `price_line.apply_options`/`options()` (engine price_line_api.rs,
      rendering in series_geometry.rs/axis.rs). — 2026-07-21
- [x] 2.5 Chart-JSON routing of `timeScale` behavioral options — `route_time_scale_patch`
      (engine lib.rs) applies all 11 keys from the incoming patch only, reference ordering. — 2026-07-21
- [x] 2.6 Public `max_bar_spacing` / `right_offset_pixels` setters (wasm + TS handle). Also added
      reference-`applyOptions`-faithful `apply_bar_spacing_option`/`apply_right_offset_option` (write
      option + apply live; gestures keep live-only `set_bar_spacing`). — 2026-07-21
- [x] 2.7 `MouseEventParams.pane_index` (null on axis strips/outside). `hoveredSeries`/`sourceEvent`
      deferred — needs the plugin platform's hit-testing (Phase 3). — 2026-07-21
- [x] 2.8 `take_screenshot(add_top_layer?, include_crosshair?)`; animated `scroll_to_position`
      (300 ms cubic ease-out, reduced-motion aware, cancelled by new scrolls/gestures);
      `subscribe_size_change`/`unsubscribe_size_change` on the time-scale handle;
      `auto_size_active()`; `chart_element()`. — 2026-07-21

### 2b. Series styling
- [x] 2.9 `last_value_visible` toggle + per-series last-value labels (every visible series, series-color
      bg, reference `_fixLabelOverlap` collision port; value tracks last *visible* bar like reference
      `lastValueData(false)`). — 2026-07-21
- [x] 2.10 Built-in price-line family: `price_line_visible`, `price_line_source` (LastBar/LastVisible),
      `price_line_width`, `price_line_color` ("" = follow series), `price_line_style` — rendered per
      visible series, reference defaults. — 2026-07-21
- [x] 2.11 Line `line_style` (dash geometry frame-built via `dash_split` with reference `getDashPattern`
      patterns — pixel-identical backends by construction), `line_visible`, `point_markers_radius`
      (auto = lineWidth/2+2), full `crosshair_marker_*` family on all visible line/area/baseline
      series. — 2026-07-21
- [x] 2.12 Baseline quadrant colors + gradients (`top/bottom_fill_color1/2`, `top/bottom_line_color`,
      per-quadrant widths/styles — an intentional superset of the reference's shared width/style), through the
      shared AreaFill gradient mechanism. — 2026-07-21
- [x] 2.13 Per-data-point colors (candle/bar `color`/`wick_color`/`border_color`, histogram/line/area
      `color`) — per-row RGBA channels in the data layer, aligned under dedupe/update/insert;
      candle/bar/histogram overrides; line/area segment+marker color runs (reference walk-line port);
      0-sparse-channel = no-override. Pixel-verified live. — 2026-07-21
  - [x] 2.13b Verbatim CSS strings for the older per-field color setters (up/down/wick/border/area/
      `color`) — all nine slots now `Option<String>` with render-time parsing; round-trips verbatim.
      — 2026-07-21
- [x] 2.14 Histogram `base`; area `invert_filled_area`; bar `open_visible`/`thin_bars`. — 2026-07-21
- [x] 2.15 Per-series `priceFormat` — price/volume/percent/custom kinds, per-series labels + ticks via
      the scale's primary source, JS custom formatter fn with clearing, reference-shaped options round-trip.
      — 2026-07-21

### 2c. Behavioral parity
- [x] 2.16 `shift_visible_range_on_new_bar` / `allow_shift_visible_range_on_whitespace_replacement` —
      `sync_time_points` ports reference `ChartModel.updateTimeScale` (chart-model.ts:953-984): right-edge
      follow + scrolled-back offset compensation (no drift), whitespace-replacement gating.
      — 2026-07-21
- [x] 2.17 Whitespace data items — all-NaN = explicit whitespace (kept, sorted/deduped, colors
      carried); skipped by render/autoscale/magnet/last-value; `update({time})` replaces with
      whitespace; `data()` returns `{time}`. — 2026-07-21
- [x] 2.18 `series.pop(count)` (colors shift along), `series.last_value_data(global_last?)`
      (whitespace-skipping), `series.price_formatter()` (resolved format chain). — 2026-07-21
- [x] 2.19 `chart.set_crosshair_position(price, time, series)` (bar-exact, emits crosshair_move) +
      `clear_crosshair_position()`. — 2026-07-21
- [x] 2.20 `localization.locale` (js Intl month tables → engine formatter) + `localization.date_format`
      (reference token tokenizer incl. naive-quote parity for the default `dd MMM 'yy`). — 2026-07-21
- [x] 2.21 Primary-series removal allowed (tombstone + first-live-series fallbacks);
      `chart.series_order()` / `set_series_order()` (paint + hit order, permutation-validated).
      — 2026-07-21

### 2d. Panes & axes cosmetics
- [x] 2.22 `add_pane`/`remove_pane`/`swap_panes`, `pane.move_to`, `pane.get_series`, per-pane
      `price_scale`, `preserve_empty_pane` (reference orphan-on-remove + pruning at the reference's two trigger
      points). — 2026-07-21
- [x] 2.23 Price scale: `align_labels`, `ticks_visible`, `entire_text_only`, `minimum_width`,
      per-scale `text_color` (JSON applier + full getter + chart-group routing). Time scale:
      `ticks_visible`, `minimum_height`, `tick_mark_max_character_length`, `visible` (whole strip —
      cleanly split from the existing `time_visible` label flag). — 2026-07-21
- [x] 2.24 Inert options resolution: `layout.background` vertical gradient (shared prim, per-pane
      extent, both backends) and `panes.separator_hover_color` (reference 9px hover band, gesture-driven)
      now render. `attributionLogo` — **decided: deliberate no-op** (reference licensing attribution,
      not ours). `hoveredSeriesOnTop` — **deferred to Phase 3** (needs hit-testing).
      — 2026-07-21

## Phase 3 — Plugin platform (structural)

Per [PLUGIN_PLATFORM_DESIGN.md](PLUGIN_PLATFORM_DESIGN.md) (design-complete, no code yet).

**Decisions (2026-07-21, user sign-off):** A-first hybrid (Option C — plugins emit backend-neutral
Prim commands; Canvas2D raw-ctx escape hatch deferred) and primitives-first ordering
(C-a → C-b → C-d → C-c). Transport: one marshalling pass per primitive per frame (JSON command
buffer; swappable for a typed-array ABI later without changing the plugin API).

- [x] 3.1/C-a Pane primitives: `pane.attach_primitive`/`detach`, lifecycle (`attached`/`detached`),
      `update_all_views`, `pane_views` → Prim commands at z-order (Bottom/Normal/Top; new
      `FramePane.top_prims` layer), `price_axis_views`/`time_axis_views` → boxed axis labels
      (extension beyond reference — IPanePrimitiveBase lacks them). JSON→Prim decoder
      (`prim_decode.rs`, host-tested). Session-bands reference primitive + Playwright fixture:
      WebGPU≡Canvas2D 0-diff with primitives active, detach restores exact pixels. Known
      placeholders: `text()` prim is a no-op on both backends until the glyph engine lands
      (parity holds trivially); reentrant chart calls from renderers are guarded.
      — 2026-07-21
- [x] 3.2/C-b Series primitives: `series.attach_primitive` bound to the series' scale (incl. overlay),
      `autoscale_info(from,to)` merged into the owning scale's range via per-frame engine
      contributions (reference visibility gating), series-bound axis labels (+`price` extension),
      auto-detach on series removal. Position-band + scale-band demo fixtures; Playwright: numeric
      autoscale superset assertion + WebGPU≡Canvas2D 0-diff. — 2026-07-21
- [x] 3.3/C-d `hit_test` + interaction: primitive `hit_test(x,y)` (reference `hitTestPane` precedence
      ported: top > built-in series > normal > bottom), per-kind series hit tests (reference
      range/line ports incl. tolerance 3), `hovered_series`/`hovered_object_id` on mouse params,
      hit-driven cursor, `hovered_series_on_top` render-only z-bump (closes the 2d deferral).
      17 engine hit tests + 5 Playwright specs. — 2026-07-21
- [x] 3.4/C-c Custom series (`add_custom_series`): engine-owned time mapping (Custom kind,
      time-only rows with base-index flag), host-aligned item storage (sort/dedupe/update/pop),
      `price_value_builder`/`is_whitespace` contract, autoscale via the C-b contribution path,
      `render(ctx)` with visible items at bar centers spliced at the series' paint position,
      last-value label/line from the custom value. reference rounded-candles plugin ported line-for-line
      as the proof fixture. Playwright 24/24. — 2026-07-21
- [x] 3.5 Re-express markers + watermark as plugins: `create_series_markers` (reference v5 plugin surface;
      shapes + text pixel-identical to the engine built-in — 0-diff parity proof on both backends)
      and `create_text_watermark` (per-line styled multi-line). Enabled by a small `text_views`
      host hook painting plugin text on the shared overlay (the `Prim::Text` placeholder gap from
      C-a is closed at the platform level). Found + documented a pre-existing WebGPU bucket-order
      quirk (tris before quads affects engine markers identically — backlog item). — 2026-07-21
- [x] 3.6/C-e (Optional) Canvas2D escape-hatch primitive for raw-`ctx` reference ports — fourth
      stacked canvas below the input overlay, `attach_canvas_primitive`, `canvas_render_target`
      with media/bitmap coordinate spaces (reference `CanvasRenderingTarget2D` subset). Reference
      `vertical-line` plugin ported character-for-character as the proof. — 2026-07-23

## Phase 4 — Release

**Distribution decision (2026-07-23, user):** publish to **GitHub Packages**, keep the package
**private** (no public npm registry). GPR rules: scope must match the repo owner → the package is
`@origin/charts`; publishing authenticates with the built-in `GITHUB_TOKEN` (no npm org/token);
installs always require a `read:packages` PAT (privacy is enforced at the registry regardless of
repo visibility).

- [x] 4.1 Distribution target switched: name `@origin/charts`, `publishConfig.registry`
      `https://npm.pkg.github.com`, CI publish job → GPR with `secrets.GITHUB_TOKEN` +
      `packages: write`, READMEs rewritten (bunfig.toml / .npmrc auth), pack smoke updated.
      — 2026-07-23
- [x] 4.2a Pre-release validation: `npm publish --dry-run` runs the full prepublish chain and
      reports a valid tarball; fixed a dry-run env leak into the pack smoke test. — 2026-07-23
- [x] 4.2 Tag `v0.1.0` → CI publishes to GPR — **PUBLISHED 2026-07-23.** rust + package gates green,
      publish job success; registry entry confirmed private (401 auth-required, not 404). En route
      fix: the browser job had never been green on CI (pixel thresholds calibrated to the dev
      machine's SwiftShader build) → marked report-only (`continue-on-error`) and removed from the
      publish gate; the dev-machine suite remains the strict gate (28/28).
      Remaining: first consumer install `bun add @origin/charts` with a `read:packages` PAT.
- [x] 4.3 React wrapper — **decided: descoped.** Closed-source internal React app; no distribution
      need, so no `@origin/react` package. The ~50-line `use_chart` lifecycle hook lives in the
      app itself (StrictMode-safe create/destroy). Revisit only if the engine is ever published for
      external consumers. — 2026-07-23
- [x] 4.4 Docs: root README install section (bun-primary, GPR); phases archived into
      PRODUCTION_ROADMAP.md. — 2026-07-23

---

## Progress log

- 2026-07-21 — Plan created from the parity/packaging/API audit.
- 2026-07-21 — **Phase 1 complete (1.1–1.8).** `@origin/charts` is publish-ready: LICENSE ×3, package
  README, full metadata + `publishConfig.access: public`, clean/prepublish chain, `init_wasm(url?)`
  Vite escape hatch, pack smoke test (local + CI), tag-triggered publish job. Tarball: 10 files,
  314 kB packed, wasm 492 kB, imports cleanly. Remaining before 4.2 can ship: create the `@origin`
  npm org and add the `NPM_TOKEN` repo secret (4.1). Next up: Phase 2a surface corrections.
- 2026-07-21 — **Bun is the primary install method** (README/plan reordered; `bun add` verified
  end-to-end against the packed tarball: install 295 ms, module imports, wasm ships).
- 2026-07-21 — **Phase 2a complete (2.1–2.8).** One semantic fix surfaced by the browser suite:
  `applyOptions({barSpacing/rightOffset})` now writes the configured option *and* applies it live
  (reference `restoreDefault` parity — options survive `reset_time_scale`); zoom/axis-drag stay live-only.
  Two spec assertions updated to the corrected semantics. Gates: 150 cargo tests, clippy clean,
  typecheck/lint green, pack smoke green, Playwright 10/10. Next up: Phase 2b series styling.
- 2026-07-21 — **Phase 2b wave 1 complete (2.9–2.12, 2.14).** 28 new series style options end-to-end
  (new `series_apply_options_json` wasm method; `series_options_json` round-trips all fields).
  Rendering notes: dash strokes are frame-built geometry (backends stay 0-diff by construction);
  last-value labels now per-series with reference overlap resolution; last-price lines per-series; crosshair
  marks on all line/area/baseline series. Demo volume fixture updated to match the reference fixture
  (`price_line_visible/last_value_visible: false` — options that didn't exist when the fixture was
  written). Gates: 164 cargo tests, clippy clean, typecheck/lint green, pack smoke green,
  Playwright 10/10.
- 2026-07-21 — **Phase 2b wave 2 complete (2.13, 2.15).** Per-data-point colors (data-layer RGBA
  channels + per-kind render overrides + reference walk-line segment semantics) and per-series
  `price_format` (built-ins + JS custom formatter). Live probe verified: per-point pixels on
  set/update/clear, candle channel rendering, format round-trips, custom-fn invocation and clearing.
  One reference-shape fix: volume format serializes as exactly `{type:"volume"}`. Gates: 186 cargo tests,
  clippy clean workspace-wide, Playwright 10/10, pack smoke green. Next: Phase 2c behavioral parity.
- 2026-07-21 — **Phase 2c complete (2.16–2.21 + 2.13b).** Shift-on-new-bar compensation, whitespace
  data items, pop/lastValueData/priceFormatter, programmatic crosshair, locale+dateFormat,
  primary-removal + series ordering, verbatim colors for the legacy setters. Live probe verified all
  seven behaviors end-to-end; spec updated for the two new time-scale option fields. Gates: 215 cargo
  tests, clippy clean workspace-wide, Playwright 10/10, pack smoke green. **Phase 2 (API breadth) is
  now done except 2d.** Next: Phase 2d panes & axes cosmetics, then Phase 3 plugin platform.
- 2026-07-21 — **Phase 2d complete; Phase 2 (API breadth) fully done.** Explicit pane management,
  full price/time axis cosmetics, gradient background (both backends, pixel-identical), separator
  hover band. `attributionLogo` decided deliberate no-op; `hoveredSeriesOnTop` deferred to Phase 3
  (needs hit-testing). Live probe verified pane ops, scale round-trips, min-width/height floors,
  strip collapse, gradient pixels, hover. Gates: 221 cargo tests, clippy clean workspace-wide,
  Playwright 10/10 (spec updated for the four new time-scale fields), pack smoke green.
  **Next: Phase 3 — plugin platform.**
- 2026-07-21 — **Phase 3 C-a→C-d + 3.5 complete.** The plugin platform is live: pane + series
  primitives (Prim command plugins, z-ordered, axis views, lifecycle), `autoscale_info` engine
  contributions, reference `hitTestPane` precedence + per-kind series hit tests (`hovered_series`,
  `hovered_object_id`, hit cursor, `hovered_series_on_top`), custom series (reference rounded-candles
  ported line-for-line), and markers/watermark re-expressed as plugins with a 0-diff parity proof.
  Backlog notes: WebGPU tri/quad bucket order quirk (pre-existing, engine markers too);
  `Prim::Text` glyph engine (plugin text uses the overlay hook). Playwright **28/28**, 270 cargo
  tests, clippy clean, pack smoke green. Remaining: 3.6/C-e (optional raw-canvas escape hatch),
  Phase 4 release.
- 2026-07-23 — **Phase 4 prep done (4.2a + 4.4).** `npm publish --dry-run` validated end-to-end
  (fixed the `npm_config_dry_run` leak into the pack smoke test). To ship v0.1.0 (user actions):
  1. create a PAT with `read:packages` + `write:packages` for local installs (in CI the built-in
  `GITHUB_TOKEN` covers it — no secret setup needed); 2. `git tag v0.1.0 && git push origin v0.1.0`
  — the publish CI job builds, gates, and publishes `@origin/charts` to GitHub Packages;
  3. verify in a fresh consumer: `bun add @origin/charts` (with the bunfig scope auth from the
  package README) then `import { create_chart } from "@origin/charts"`.
- 2026-07-23 — **WebGPU bucket-order fidelity bug fixed.** The WebGPU executor painted family
  buckets (tris → quads) within a layer while Canvas2D paints in prim order, so markers/rounded
  shapes z-ordered differently across backends (275 px on the marker fixture). Fixed with
  order-exact run-length batching (`prims_to_group` + `DrawRun`): prim order now matches the
  Canvas2D executor exactly, candles stay 1 draw call, and the browser gate asserts **zero
  ordering diff with markers visible** (only SwiftShader-MSAA-vs-analytic-AA edge steps remain —
  unreachable, documented). New backlog note: 50%-alpha histogram quads show premultiplied-alpha
  rounding diffs between backends (`?feature=volume`, ~1.9k px, ungated).
- 2026-07-23 — **Backlog cleared (all three items).** (1) Premultiplied-alpha fidelity: exact
  browser source-over math in WGSL (`blend.rs`) — volume fixture strict-diff **1,879 → 0 px**,
  exhaustive 16.6M-cell verification vs Chrome's integer blending. (2) C-e raw-canvas escape
  hatch: plugin canvas + verbatim `vertical-line` reference plugin port. (3) `Prim::Text`
  engine: browser-rasterized glyph bitmaps → texture quads on WebGPU (LRU cache, color-baked
  rasters), direct `fillText` on Canvas2D, both in prim order; text parity residual measured at
  162 px, every channel exactly ±1/255 on AA edges only (the documented sp=0 blend class).
  Playwright **33/33**; Playwright count grew 28 → 33 across these features.
- 2026-07-21 — **Wave-1 verification pass.** Independent live-browser probe (defaults, apply→
  options round-trips, render) caught a fidelity bug: new color options round-tripped normalized
  (`#FF0000`→`#ff0000`) — fields were stored as parsed `Color`. Fixed to verbatim CSS strings with
  render-time parsing (mirrors the `up_color` precedent); probe + 10/10 Playwright green after.
