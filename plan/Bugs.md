# Backend and host parity bugs

Source: parity audit of 2026-10-03 against `main` at `b35026b`. Every item below was found by
reading the code. Items marked **Verified** were confirmed directly in source (including the
pinned GPUI checkout at Zed rev `1057c2c`). Items marked **Suspected** must be reproduced first;
if a suspected item turns out not to be a bug, tick it and record the evidence instead of
changing code.

The rule being enforced: one engine, one ordered frame, and every executor (WebGPU, Canvas2D,
GPUI, tiny-skia) and every host (browser, offscreen worker, GPUI/Aeris Terminal) must produce the
same chart and the same behavior.

## Instructions for the fixing agent

- Read `AGENTS.md` and `docs/Architecture.md` first. Their rules apply to every item.
- Fix each problem at its shared owner (engine, `aeris_charts_render`, or the one adapter), not
  by patching each renderer around it.
- For every item, first add a regression test or fixture that fails before the fix, then fix it.
  Never weaken an existing test, threshold, or lint to make a gate pass.
- Tick a box (`- [x]`) only when the fix and its test are done and the focused checks pass.
  Under the box, add one line: `Fixed: <short description>; test: <test name or file>`.
- Update `docs/Architecture.md` in the same change whenever an item changes an architectural
  claim. Items P1 and P2 in particular change documented host/input ownership.
- After all boxes and required gates pass, commit and push the reviewed batch under `AGENTS.md`.
- Do not modify Aeris Terminal. It inherits the GPUI fixes through `aeris_charts_render_gpui`.
- Work order: section A, then B, then C, D, E, F. Within a section, follow the numbering.

Severity key: **High** = users see a wrong chart or wrong behavior today. **Medium** = visible
in specific conditions or a divergence between hosts. **Low** = sub-pixel or latent.

---

## A. Rendering bugs users can see today

- [x] **A1. GPUI shows images with red and blue swapped** (High, Verified)
  - Fixed: cached GPUI images convert RGBA to BGRA; test: `raster_image_cache_passes_bgra_pixels_to_gpui` and exact `colored_image`/`depth_heatmap_colors` pixel-parity gates.
  - Where: `crates/aeris_charts_render_gpui/src/backend.rs:385-391` (`RasterImageCache::resolve`).
  - Problem: `Prim::Image` pixels are straight-alpha **RGBA**. The code passes them unchanged into
    `gpui::RenderImage::new`, which is documented as **BGRA** (`gpui/src/assets.rs:42`). GPUI's
    own loaders swap channels before creating a `RenderImage` (`gpui/src/elements/img.rs:679-680`,
    `gpui/src/platform.rs:2707-2708`); Aeris does not.
  - User impact: in Aeris Terminal the liquidity heatmap shows asks blue instead of red and bids
    olive instead of teal (defaults `depth.rs:147-148`). Every colored plugin/native image is
    wrong. The white crosshair icon hides the bug.
  - Fix: convert RGBA to BGRA once when building the cached `RenderImage` (the cache already
    copies the pixels to apply opacity, so this adds no extra copy).
  - Done when: a GPUI test renders a known asymmetric-color image (for example pure red over pure
    blue) and asserts the BGRA payload handed to GPUI has the channels in GPUI's order; the
    `gpui_pane_capture` / `pixel_parity` capture of the depth heatmap matches tiny-skia colors.

- [x] **A2. WebGPU draws dashed and dotted lines as solid** (High, Verified)
  - Fixed: WebGPU now splits expanded dashed and dotted paths with GPUI's shared routine; test: `styled_polylines_follow_shared_dashes_after_path_expansion` in `frame_contract.rs`.
  - Where: `crates/aeris_charts_render_wgpu/src/tri_executor.rs:261-268` drops `style` with `..`.
  - Emitters affected: drawings `crates/aeris_charts_engine/src/frame/drawings.rs:61-67` and
    `:800-806`; general series `frame/general_series_geometry.rs:255, 341, 460, 468`; host/plugin
    primitives decoded at `crates/aeris_charts_wasm/src/prim_decode.rs:138`.
  - User impact: a dashed or dotted trend line (and dashed general-chart lines, range-band edges,
    plugin lines) renders solid in the browser's default WebGPU backend, but dashed in Canvas2D,
    GPUI and tiny-skia.
  - Fix: one shared dash-splitting routine in `aeris_charts_render` (GPUI already has
    `dash_split` in `crates/aeris_charts_render_gpui/src/geometry.rs:298-323`, and the engine has
    `push_line_stroke` in `frame/series_geometry.rs:14-58`). Move the split into the render crate,
    use it from both GPU executors, and delete the duplicates. Dashes must follow the expanded
    (stepped/curved) path and use `LineStyle::dash_pattern`.
  - Done when: `crates/aeris_charts_render_wgpu/tests/frame_contract.rs` has a Dashed and a Dotted
    `Polyline` case (Simple and Curved) whose emitted geometry has gaps matching the dash pattern,
    and the GPUI and WebGPU dash geometry come from the same function.

## B. Rendering differences between backends

- [x] **B1. GPUI rotated text sits about 1 px lower than normal text** (Medium, Verified)
  - Fixed: sprite placement uses the ordinary measured middle baseline; test: `zero_degree_rotated_text_uses_the_unrotated_baseline`.
  - Where: `crates/aeris_charts_render_gpui/src/backend.rs:760-777` uses
    `height = (ascent + descent).max(font_size)`, `top = y - height/2`, baseline `pad + ascent`.
    Unrotated text uses `text::middle_baseline` (`backend.rs:1006`, `text.rs:124-126`).
  - Fix: place the rotated sprite baseline with the same `middle_baseline` formula as unrotated
    text. Also confirm the SVG `<text>` resolves the same face as `ShapedLine::paint` (comment at
    `backend.rs:694` admits it may not); if it cannot be guaranteed, document the fallback.
  - Done when: a GPUI test asserts a 0° `RotatedText` and a `Text` with the same run produce the
    same baseline y.

- [x] **B2. Image sampling filter differs on every backend** (Medium, Verified)
  - Fixed: shared edge snapping and bilinear sampling across executors, with atlas-edge protection and GPUI opacity rounding bounded to one channel value; test: `scaled four-color image matches WebGPU, Canvas2D, and native`, `image_rect_rounds_each_edge_with_gpui_half_toward_zero_rule`, and `scaled-image` pixel-parity gate.
  - WebGPU: nearest (`crates/aeris_charts_render_wgpu/src/tex_quad_pipeline.rs:151`).
    Canvas2D: browser default smoothing, `imageSmoothingEnabled` never set
    (`crates/aeris_charts_wasm/src/canvas2d_target.rs:203-224`). tiny-skia: bicubic
    (`crates/aeris_charts_native/src/lib.rs:479-483`). GPUI: polychrome sprite sampler (check the
    pinned source for its filter).
  - Fix: decide one image filter for the frame contract (recommended: linear/bilinear, because
    the heatmap is upscaled and must look smooth; if the heatmap needs crisp cells, nearest for
    all). Add it to the `Prim::Image` contract doc in `draw_list.rs` and set it explicitly in every
    executor (WebGPU needs a separate sampler for images vs. text; Canvas2D must set
    `imageSmoothingEnabled`/`imageSmoothingQuality`; tiny-skia must use the matching
    `FilterQuality`).
  - Also fix in the same item: GPUI rounds opacity into the alpha byte (`backend.rs:388`) while
    others apply float opacity, and GPUI snaps image destination rects to device pixels. Make the
    rect rounding rule part of the contract and apply it identically.
  - Done when: a 2×2 image scaled into a 20×20 rect produces matching pixels (within ±1/255) on
    Canvas2D, WebGPU, and tiny-skia in the browser parity suite, and GPUI uses the same filter.

- [x] **B3. WebGPU and GPUI rounded-rect borders overdraw the fill** (Medium, Verified, latent)
  - Fixed: both GPU executors draw the shared inside ring without painting behind transparent fill; test: `transparent_round_rect_fill_has_no_border_geometry_inside_the_inner_rect`, `transparent_round_rect_fill_leaves_the_border_interior_empty`, and browser `transparent rounded tooltip keeps its outlined interior clear`.
  - Where: WebGPU `crates/aeris_charts_render_wgpu/src/tri_executor.rs:155-167` and GPUI
    `crates/aeris_charts_render_gpui/src/executor.rs:269-287` fill the whole outer shape with
    `border_color`, then paint `fill` on top. Canvas2D/tiny-skia
    (`crates/aeris_charts_render/src/canvas2d.rs:366-395`) fill only the inner area and paint a
    ring.
  - Impact: today every bordered `RoundRect` uses an opaque fill, so output matches. A
    translucent or transparent fill (outline-only chip, hover state) would show a solid block of
    border color on WebGPU/GPUI.
  - Fix: add a shared ring tessellation (outer polygon minus inner polygon) in
    `crates/aeris_charts_render/src/line.rs` next to `round_rect_polygon`; both GPU executors emit
    the inner fill plus the ring, exactly like Canvas2D.
  - Done when: a `RoundRect` with `fill` alpha 0 and a 2 px border produces no border-colored
    pixels inside the inner rect on every backend (contract test for WebGPU and GPUI, pixel test
    in the browser suite).

- [x] **B4. Rounded-rect radius clamping differs** (Low, Verified)
  - Fixed: Canvas2D, shared polygon/ring, and both GPU executors use one CSS proportional radius rule; test: `oversized_round_rect_radii_match_the_shared_polygon` and GPUI pixel-parity gate.
  - Where: shared `round_rect_polygon` clamps radii to `min(w,h)/2`
    (`crates/aeris_charts_render/src/line.rs:816-819`); Canvas2D `round_rect_path`
    (`canvas2d.rs:488-502`) does not, so oversized radii fold the path.
  - Possible triggers: `crates/aeris_charts_engine/src/frame/alert_geometry.rs:263-272`,
    `frame/feature_geometry.rs:439-447`.
  - Fix: one clamping rule in the render crate used by `round_rect_path` and
    `round_rect_polygon` (CSS rule: scale all radii down proportionally when adjacent radii
    exceed a side).
  - Done when: a unit test with radii larger than half the side produces the same clamped radii
    from both paths.

- [x] **B5. GPUI fills have no edge smoothing when GPUI's MSAA is off** (Medium, Verified)
  - Fixed: AreaFill, BandFill, Triangle, and RoundRect meshes carry coverage fringes; test: `filled_mesh_boundaries_encode_coverage_without_msaa` plus the Linux X11 1× `pixel_parity` gate (exact crisp rects; 3,288 filled-scene edge pixels and max channel delta 236, inside measured 4,500/240 bounds). The Windows official-window parity gate still covers all fixture families.
  - Where: area, band, triangle and default rounded-rect meshes use `MeshVertex::solid`
    (`crates/aeris_charts_render_gpui/src/geometry.rs:122-133`, `:519-534`;
    `executor.rs:237-240`). GPUI path MSAA can fall back to 1× on Linux (`scene.rs:157-162`).
  - Fix: give these meshes the same per-edge coverage encoding the polyline/disc meshes already
    use, so GPUI edges are smooth regardless of MSAA.
  - Done when: a GPUI geometry test asserts the outer edge vertices of an AreaFill, BandFill,
    Triangle and RoundRect mesh carry a coverage ramp, and `pixel_parity` edge deltas stay within
    the crisp/AA tolerance with MSAA forced to 1×.

- [x] **B6. GPUI dots are about half a pixel larger** (Low, Verified)
  - Fixed: disc coverage now straddles the nominal radius with Windows-compatible constant-s encoding; test: `disc_mesh_integrated_coverage_matches_nominal_area` and GPUI `pixel_parity` (opaque AA max delta 217→90).
  - Where: `crates/aeris_charts_render_gpui/src/geometry.rs:420-440` keeps the disc solid to the
    nominal radius and fades 1 px outside it. Canvas2D, tiny-skia and WebGPU center the edge
    transition on the radius.
  - Fix: center the coverage ramp on the radius (half inside, half outside), like the ring and
    polyline encodings.
  - Done when: a test asserts integrated disc coverage equals πr² within tolerance.

- [x] **B7. Degenerate circles and line widths render differently** (Low, Verified)
  - Fixed: all executors share the positive finite extent rule for circle radii and polyline widths; test: `degenerate_circles_and_polyline_widths_emit_no_canvas_commands`, `degenerate_circles_and_polyline_widths_emit_no_triangles`, `degenerate_circles_and_polyline_widths_emit_no_scene_ops`, and `degenerate_circle_and_polyline_leave_native_pixels_untouched`.
  - Negative radius: WebGPU and tiny-skia draw a disc of |r|, the browser `arc` throws (error is
    swallowed), GPUI skips. Radius 0 with a stroke: GPUI draws a dot, WebGPU skips
    (`tri_executor.rs:113`, `gpui/geometry.rs:482-500`).
  - Zero/negative polyline width: browser Canvas ignores `lineWidth = 0` and reuses the previous
    width (`canvas2d.rs:273`), tiny-skia draws a 0.01 px hairline (`native/src/lib.rs:239`),
    WebGPU and GPUI draw nothing (`line.rs:450-452`).
  - Fix: define the rule once in `crates/aeris_charts_render/src/canvas2d.rs` and the GPU paths:
    non-finite or `radius < 0` circles and `width <= 0` polylines draw nothing; a `radius == 0`
    circle draws nothing (fill and stroke). Document it on the `Prim` variants.
  - Done when: unit tests for each executor assert nothing is emitted for those inputs.

- [x] **B8. Large circles are faceted on GPU backends and tiny-skia** (Low, Verified)
  - Fixed: shared radius-based circle segment count drives disc, ring, and native arc tessellation with bounded work; test: `circle_chord_error_stays_below_tenth_device_pixel` and GPUI `pixel_parity`.
  - Where: fixed 24 segments in `line.rs:844`, `tri_executor.rs:112`,
    `gpui/geometry.rs:423, 454`, `native/src/lib.rs:172`. The browser draws exact arcs. A 14 CSS px
    ring at DPR 3 shows about 0.36 px chord error.
  - Fix: pick segment count from the device radius, the same way rounded-rect corners already do
    (`line.rs:835-841`), in one shared helper used by all three.
  - Done when: a unit test asserts chord error stays under 0.1 device px for radii from 1 to 200.

- [x] **B9. Area and band fills over-cover where the edge crosses the base or the bands cross**
  - Fixed: shared tessellation splits crossing segments into non-overlapping lobes and GPUI traces each lobe's AA fringe separately; test: `area_crossing_base_has_exact_nonoverlapping_lobes`, `crossed_band_has_exact_nonoverlapping_lobes`, `crossed_area_and_band_keep_their_solid_cores_in_the_exact_lobes`, and GPUI `pixel_parity`.
  (Low, Verified geometry; trigger Suspected)
  - Where: per-segment quads down to `base_y` become bow-ties when a segment crosses the base
    (`line.rs:673-708`); same for `BandFill` when upper and lower cross
    (`tri_executor.rs:250-259`, `gpui/geometry.rs:585-595`). Canvas2D fills the exact region.
    Possible triggers: general area series with a baseline
    (`frame/general_series_geometry.rs:315-321`) and Catmull-Rom overshoot.
  - Fix: in the shared tessellator, split a segment at its crossing point before emitting the two
    triangles.
  - Done when: a tessellation test with a segment crossing `base_y` (and crossing bands) produces
    triangles whose union equals the exact region with no double coverage.

- [x] **B10. Translucent lines darken at joins on WebGPU** (Low, Verified)
  - Fixed: the shared stroke tessellator clips neighboring segments at joint bisectors using deterministic shared-edge intersections, preserving translucent coverage without curved-stroke seams. Tests: `translucent_zigzag_stroke_never_blends_two_triangles_at_one_sample`, `clipped_curved_stroke_keeps_inner_band`, browser `translucent zig-zag joins match Canvas2D on WebGPU`, and GPUI `translucent-join gate` (maximum join delta 17).
  - Reproduced: browser `translucent zig-zag joins match Canvas2D on WebGPU` fails at a join with 68/255 maximum channel delta; WebGPU blends overlapping stroke triangles.
  - Where: `stroke_aa` (`line.rs:441-449`) emits overlapping join/segment triangles; WebGPU blends
    each triangle separately (`tri_pipeline.rs:148`). Canvas2D/tiny-skia cover each pixel once.
  - First: render a 50%-alpha zig-zag polyline on WebGPU and Canvas2D and compare join pixels.
  - Fix if confirmed: make `stroke_aa` emit non-overlapping geometry, or use a stencil/max-blend
    pass per stroke. Check GPUI too.
  - Done when: the zig-zag comparison matches within the normal AA tolerance.

- [x] **B11. WebGPU rotated text has darkened edges** (Low, Verified in code)
  - Fixed: rotated text bilinearly reconstructs four premultiplied source texels inside its dedicated shader, clamped to the run's atlas slot. The browser regression first found 393 darkened edge samples versus Canvas2D; after the fix it found 356 within the measured rotated-raster residual, and verified zero white-label pixels darker than the dark background. The existing drawing parity test also passes.
  - Where: rotated text samples the straight-alpha atlas linearly
    (`tex_quad_pipeline.rs:167`) and premultiplies after sampling (`:125-128`); transparent
    texels have rgb 0, which darkens filtered edges.
  - Fix: store premultiplied coverage in the atlas (or premultiply before filtering) so linear
    sampling is correct.
  - Done when: a rotated white label on a dark background has no edge pixels darker than the
    background in the browser parity test.

- [x] **B12. WebGPU background and text failure differ from Canvas2D** (Low, Verified)
  - Fixed: the browser WebGPU surface uses premultiplied alpha with a premultiplied background clear; failed non-empty atlas text resolves through the warm Canvas2D pane for the complete frame and rebuilds its GPU group for recovery. Browser regressions reproduced opaque blue versus translucent purple and a >1024 px label with 6384 Canvas2D white pixels versus zero WebGPU pixels before the fix. After the fix, the translucent background matches within two channels, the oversized text fallback matches Canvas2D within two channels across all pixels, and removing the label restores WebGPU presentation.
  - WebGPU clears with alpha forced to 1 (`crates/aeris_charts_wasm/src/chart/inner_render.rs:443-448`);
    Canvas2D fills with the CSS string, which may carry alpha (`:637-639`).
  - WebGPU silently drops text when `TextRunStore` is missing or a run is wider than the 1024 px
    atlas (`crates/aeris_charts_wasm/src/chart/text_runs.rs:185-193`); Canvas2D still draws it.
  - Fix: honor background alpha on WebGPU (premultiplied clear plus a transparent-capable
    surface alpha mode), and route oversized runs through the existing Canvas2D-fallback or split
    them, instead of dropping them.
  - Done when: a translucent `layout.background` and a >1024 px text run render the same in both
    browser backends.

## C. Reference renderer (tiny-skia) bugs

tiny-skia makes the golden images that other backends are compared with, so these bugs hide
real divergences.

- [x] **C1. Native renderer does not clip panes** (High for evidence, Verified)
  - Fixed: `TinySkiaCanvas` implements pane save/restore and an integer scissor mask used by every paint route; both native chart render paths execute each pane inside its own clip. Regression `pane_zero_scatter_cannot_paint_into_the_next_pane` failed with 492 leaked pixels before the fix and now passes; native unit and golden tests pass.
  - Where: `crates/aeris_charts_native/src/lib.rs:525-551` and `:583-624` flatten every pane into
    one unclipped list; `TinySkiaCanvas` never implements `save`/`restore`/`clip_rect` (no-op
    defaults at `crates/aeris_charts_render/src/canvas2d.rs:24-28`). Canvas2D, WebGPU and GPUI
    clip each pane to its integer scissor.
  - Fix: implement `save`/`restore`/`clip_rect` on `TinySkiaCanvas` (tiny-skia `Mask`) and execute
    each pane through the same per-pane clip sequence the browser uses.
  - Done when: a native test draws a primitive that overflows pane 0 and asserts pane 1 pixels
    are untouched.

- [x] **C2. Band fills in the second and later panes read the wrong points** (High for evidence,
  Verified)
  - Fixed: both native chart render paths execute each pane directly with its own point pool, removing flattening and `remap_prim_points`. Regression `later_pane_band_uses_its_own_point_pool` checks exact interior band pixels against the same band rendered from its local points; native unit and golden tests pass.
  - Where: `remap_prim_points` (`crates/aeris_charts_native/src/lib.rs:660-692`) shifts only
    `Polyline` and `AreaFill`; `BandFill.upper_first/lower_first` stay pane-local. `pane.under` is
    pushed without remapping (`:532`, `:598`).
  - Fix: render each pane with its own point pool (preferred, removes the remapping entirely), or
    remap every point-pool primitive including `BandFill` and the `under` layer.
  - Done when: a two-pane native render with a Bollinger band in pane 1 matches the same band
    rendered alone.

- [x] **C3. Native text ignores family, weight and italic, and is never measured** (Medium,
  Verified)
  - Fixed: native painting resolves installed faces from the requested family stack, weight, and
    italic style, and installs the same face and advance calculation as the engine drawing
    measurer. The real chart regression reaches a label hit position near the painted advance;
    native unit and golden tests pass.
  - Where: one system face for every run (`native/src/lib.rs:28-43`, `:310-329`); native never
    installs `set_text_measure`, so the engine falls back to `chars × size × 0.6`
    (`crates/aeris_charts_engine/src/drawings.rs:2414-2417`). Label boxes and hit areas therefore
    don't match painted glyphs.
  - Fix: install a tiny-skia/ab_glyph text measurer in native rendering paths; honor weight and
    italic where the system face provides them (synthesize or select a face), and document what
    is honored.
  - Done when: a native test asserts the engine label box width equals the measured painted
    glyph advance.

- [x] **C4. Native image export uses the wrong background and blocky resizing** (Medium,
  Verified)
  - Fixed: export builds the frame at the requested DPR and CSS dimensions, paints the configured
    solid or gradient background, and restores the live chart view. Regression compares a 2×
    export pixel-for-pixel with a direct DPR 2 render, checks a resized export leaves the live
    view unchanged, and checks gradient stops; native unit and golden tests pass.
  - Where: `render_engine_rgba` clears to `DEFAULT_SURFACE_RGB` (`native/src/lib.rs:625-630`)
    while `render_engine` uses `layout.background` (`:538-543`); it rasterizes at the chart's DPR
    and resizes with nearest neighbor (`:591-652`).
  - Fix: use `layout.background` (including gradient) and rasterize directly at the requested
    export scale instead of resampling.
  - Done when: an export with a custom background and `scale = 2` equals a direct render at DPR 2.

- [x] **C5. Native text drops subpixel x and misplaces glyphs at negative x** (Low, Not reproduced)
  - Resolved by verification: pinned `ab_glyph` computes integer pixel bounds with `floor` and
    `ceil` before returning `OutlinedGlyph::px_bounds`; the outline rasterizer retains the
    fractional glyph position in coverage. Native test checks painted coverage columns against
    the glyph outline at x = −0.5 and x = 10.25. The existing cast therefore does not truncate a
    fractional bound, and the rotated path's integer local placement retains fractional placement in
    its final tiny-skia transform.
  - Where: `native/src/lib.rs:365` (`bounds.min.x as i32 + gx` truncates toward zero); rotated
    path rounds into an integer pixmap (`:434-459`).
  - Fix: floor instead of truncating and keep the fractional offset in glyph positioning.
  - Done when: a unit test with x = -0.5 and x = 10.25 places coverage at the expected columns.

## D. Browser and Aeris Terminal behave differently

Root cause for most of this section: Aeris Terminal routes input through the engine input
controller (`crates/aeris_charts_engine/src/chart_input.rs`, `ChartEngine::input_*`), but the
browser does not. `packages/charts/src/gestures.ts` and, separately,
`packages/charts/src/offscreen.ts` re-implement routing, key bindings, clicks, cursors and kinetic
scrolling in TypeScript, using only the low-level `GestureResolver` behind the WASM
`input_pointer_*` exports (`crates/aeris_charts_wasm/src/chart.rs:4490-4585`). This violates the
AGENTS.md rule "Interaction policy is engine-owned".

Unless the maintainer says otherwise, the engine controller's behavior is canonical. Where only
the browser has a feature (undo/redo, reduced motion), add it to the engine so every host gets
it.

- [x] **D1. Route browser and offscreen input through the engine input controller** (High,
  Verified)
  - Fixed: the browser brushable Area helper now attaches one engine-owned composition with explicit style overrides and no Escape/double-click listeners; engine override regression, Chromium brush cases, and 94 focused browser interaction cases pass.
  - In progress: browser and offscreen wheel events now call one WASM `input_wheel` operation backed
    by `ChartEngine::input_wheel`; the host wheel policy branches and intermediate calculator
    exports were removed. A Playwright regression proved wheel cancellation of a held keyboard pan
    and the browser reference/offscreen suites pass. Browser and offscreen keys now enter the same
    controller; `End` has a failing-before Playwright regression. Browser pointer, touch, cursor, and the
    full event queue still need migration. Offscreen mouse and pen pointer events now enter the
    controller; a failing-before worker regression proves the shared 5 px drag threshold. Pointer
    identity and device are now engine-owned, with a worker replay and engine regression for a
    foreign pointer's release/cancel.
    Browser mouse and pen pointer samples now use the controller for press, drag, hover, capture,
    context menu, and cursor; engine events notify browser click, double-click, drawing-created,
    and text-editor effects. The temporary legacy mouse branches and click timer are removed;
    59 focused browser interaction cases pass.
    Browser and offscreen touch-pinch zoom now call
    `ChartEngine::input_pinch`; a failing-before worker regression confirms the starting logical
    centroid remains anchored, and pinch enablement is independent of wheel zoom.
    The WASM pointer entry points now carry one packed device/modifier/options word shared by
    browser and worker adapters, satisfying strict Clippy without per-event allocation. Browser
    pointer-up dispatches the engine's trading intents and alert requests to host subscribers;
    window modifier events refresh the engine drawing magnet even without overlay key focus.
    Worker touch samples now use the same controller as mouse/pen. An engine regression proves
    two-touch pinch membership and primary-finger pan continuation; a failing-before worker
    regression proves touch panning waits for the shared threshold.
    The engine now owns the 240 ms long-press deadline, crosshair tracking, and its next-tap or
    touch-end exit rule. A failing-before worker replay covers hold-then-move without panning and
    next-tap exit. Browser Touch Events now enter that path; Chromium regressions cover the
    240 ms hold, tracking movement, next-tap exit, and DOM page-scroll handoff. The touch
    separator clamp regression fails on the former adapter and passes through the controller.
    The selected interaction, pane, named-scale, and worker suites pass (51 cases).
    The optional brushable Area helper now uses `ChartEngine::set_brushable_area_with_styles`;
    the engine applies defaults and explicit overrides, owns range clearing, and the helper
    only attaches, queries, clears, and detaches the composition.
  - Fix: expose `input_pointer_down/move/up/cancel/leave`, `input_wheel`, `input_pinch`,
    `input_key_down/up`, `input_cursor`, `flush_coalesced_input` and the `ChartInputEvent` queue
    from `aeris_charts_wasm`, backed by `ChartEngine::input_*`. Make `gestures.ts` and
    `offscreen.ts` thin adapters: DOM event translation, pointer capture, applying the cursor,
    `preventDefault`/page-scroll policy, timers, and DOM side effects only. Delete the duplicated
    TypeScript routing, press arbitration, click/double-click, key bindings, cursor choice,
    kinetic and dwell logic.
  - Touch: the controller must also own touch behavior (pinch, two-pointer membership, the touch
    separator tolerance) so the browser keeps it. Page-scroll direction arbitration stays in the
    browser host because it is a DOM-only effect.
  - Keep browser-only platform features working (accessibility layer, DOM text editing surface,
    context menu, clipboard).
  - Update `docs/Architecture.md`: input controller is used by **all** hosts, not only native.
  - Done when: `rg "begin_keyboard_scroll|on_keydown|dblclick|kinetic" packages/charts/src`
    finds no interaction policy, all existing Playwright interaction tests pass unchanged, and
    items D2–D12 are satisfied by the shared controller.

- [x] **D2. One set of key bindings on every host** (High, Verified)
  - Fixed: browser, offscreen, and GPUI key adapters now enter the engine controller. Engine tests
    cover navigation, Home/End, zoom, sequence Backspace/Enter, selection Delete, Escape, and
    Undo/Redo; the GPUI adapter drives the same bindings. Browser drawing, brush, navigation and
    selected-series Playwright cases pass. Worker navigation and selected-series Delete pass;
    GPUI probe drains `RemoveSeries` and removes the host-owned series. A browser `End` regression
    and a worker Delete regression failed before their fixes.

  | Key | Browser before fix | Engine before fix | Required |
  |---|---|---|---|
  | Home | `fit_content` | `reset_view` (also price scales) | Engine behavior |
  | End | nothing | scroll to latest | Engine behavior |
  | Page Up / Page Down | nothing | page by 0.8 of the plot | Engine behavior |
  | Delete / Backspace | selected drawing only | `delete_selection` (drawing, indicator, or series via `RemoveSeries`) | Engine behavior |
  | Escape | does not abandon a press or clear brushed ranges | abandons press, clears tool, selection, brush, hover | Engine behavior |
  | Ctrl/Cmd+Z, Ctrl/Cmd+Shift+Z | undo / redo drawing | not supported | Add `ChartKey::Undo`/`Redo` to the engine |
  | + / − | zoom | zoom | Same |

  - Done when: an engine unit test covers every row, and a Playwright test plus a GPUI adapter
    test drive the same keys and assert the same resulting state.

- [x] **D3. Horizontal wheel and trackpad pan are reversed in Aeris Terminal** (High, Verified)
  - Fixed: GPUI negates horizontal wheel deltas before feeding the engine. An adapter regression
    drives both Shift+wheel lines and horizontal pixel wheel events and matches the browser sign;
    the GPUI-backend focused test passes.
  - Where: `crates/aeris_charts_render_gpui/src/input.rs:116-134` passes GPUI `delta.x` unchanged.
    GPUI's horizontal sign is opposite to the DOM's (Windows: `gpui_windows/src/events.rs:589-593`
    Shift+wheel and `:625` horizontal wheel). The browser passes `+deltaX` and `-deltaY`
    (`gestures.ts:467-468`); GPUI's y is already flipped, x is not, so native x is reversed.
  - Fix: negate `delta_x` in the GPUI adapter, matching the y convention, and document the
    engine's sign convention on `WheelSample`.
  - Done when: an adapter test feeds Shift+wheel-down and a horizontal-wheel event and asserts
    the same `WheelSample` sign as the browser for the equivalent DOM event.

- [x] **D4. Wheel speed differs between hosts** (Medium, Verified)
  - Fixed: engine `WheelSample::normalize_delta` owns pixel, DOM line, and page conversion;
    browser and offscreen call the WASM export, while GPUI converts native line units to DOM line
    units before calling the same rule. Three default GPUI lines and one 100 px browser notch now
    produce the same zoom at DPR 1, 1.5, and 2. The public-reference browser wheel traces and
    offscreen worker suite pass unchanged.
  - Browser: Windows Chromium ≈100 px per notch scaled by `1/devicePixelRatio`; line mode 32 px,
    page mode 120 px (`gestures.ts:452-462`). GPUI: lines already multiplied by the Windows
    scroll-lines setting, then 32 px (`input.rs:24, 120`), ≈96 px per notch, no DPR correction,
    no page mode.
  - Fix: one normalization rule in the engine (`WheelSample` constructor or controller) that both
    adapters feed with raw deltas plus delta mode; one notch must produce the same pan distance
    and zoom step at DPR 1, 1.5 and 2.
  - Done when: an engine test asserts identical per-notch results for the browser and GPUI raw
    inputs at those DPRs.

- [x] **D5. Cursor choice differs** (High, Verified)
  - Fixed: browser mouse and pen listeners apply the shared `ChartCursor` code during hover and capture; tests: `a captured pane pan uses the shared grabbing cursor`, `an armed drawing tool keeps the engine crosshair cursor over series`, `anchor drag re-anchors one point`, and `trading-line hover and drag apply the engine cursor`.
  - Browser sets the cursor only when no button is held (`gestures.ts:729-749`), so it never
    shows grabbing while panning or dragging, and never forces the crosshair while a drawing tool
    is armed. Engine does both (`chart_input.rs:1274-1317`). Trading lines are `grab` in the
    browser (`impl.ts:3889-3891`).
  - Fix: the browser applies `ChartCursor` from the controller through one mapping function
    (mirror of `cursor_style` in `input.rs`). Windows' vertical-resize substitution stays a GPUI
    platform detail.
  - Done when: a Playwright test asserts the canvas cursor during pan, drawing drag, trading-line
    drag and armed tool equals the engine's `ChartCursor` mapping.

- [x] **D6. Click and double-click rules differ** (Medium, Verified)
  - Fixed: offscreen mouse/pen presses pass the host click count to the controller.
    A worker runtime test confirms time- and price-axis double-click resets. Browser click
    recognition uses the engine's 500 ms / 5 px rule because browser `pointerdown.detail` is
    zero. A stationary second release is the double-click; if the second press moves, its gesture
    remains active. Making the whole second press inert broke immediate brush-anchor editing in
    a browser regression, so the controller now applies the movement-aware rule even when GPUI
    supplies native click count 2. Browser, worker, and GPUI replay cover the moving second press.
    Browser brush double-click clearing and trading close-control press source pass, and an engine
    regression covers double-click clearing of the engine-owned Area range. The 111 affected
    Chromium cases pass, including the immediate brush-anchor edit that an inert second press broke.
  - Browser: its own 500 ms timer and <5 px Manhattan distance between `click` events
    (`gestures.ts:19-21, 888-938`); second press can still pan or drag; double-click doesn't clear
    brushable ranges (`impl.ts:5618`); a trading control activates on any release over it
    (`gestures.ts:868-872`).
  - Engine: platform click count at press time (`chart_input.rs:416`), second press is inert,
    double-click clears brushable ranges (`:1087`), trading controls require the press to start on
    the control (`:583-590`).
  - Fix: browser PointerEvent down carries zero click detail in Chromium, so the controller
    recognizes repeats from its own bounded history; delete the TypeScript timer. Native click
    counts enter that same controller rule.
  - Done when: browser and GPUI replays assert that a stationary second press double-clicks but
    a moving second press remains a drag, while brush clearing and press-source trading activation
    match the engine. Do not disable immediate anchor editing to make the second press inert.

- [x] **D7. Pane divider drag math differs** (Medium, Verified)
  - Fixed: mouse and touch browser paths use the controller's original grab-point rule. An
    engine regression and browser mouse/touch drags past the clamp assert correct reversal.
    Touch separator routing resolves through `HitProfile::TOUCH` in the controller.
  - Browser applies incremental deltas (`gestures.ts:372-377`); engine computes the absolute
    position from the grab offset (`chart_input.rs:491-495`). After hitting the clamp, the browser
    moves back immediately when the pointer reverses; native waits for the grab point. Touch uses
    a 12 px tolerance only in the browser (`gestures.ts:176`).
  - Fix: covered by D1; move the touch tolerance into the engine `HitProfile`.
  - Done when: an engine test drags past the clamp and back and asserts the absolute rule; touch
    hit uses `HitProfile`.

- [x] **D8. Wheel routing details differ** (Medium, Verified)
  - Fixed: browser and offscreen wheel events use the engine controller. A consumed wheel records
    its event position in the controller before resolving hover and cursor, and the browser applies
    that result. Browser regressions cover the price-axis option, the plot-height guard, held-key
    motion cancellation, and cursor refresh; a worker regression covers the price-axis option.
  - Before the fix, browser lacked `price_axis_wheel_zoom` (only explicit zoom mode zoomed the price axis,
    `gestures.ts:490`), skips the engine's in-pane check (`chart_input.rs:705`), doesn't stop
    kinetic/keyboard motion on wheel (`:701`) and doesn't refresh hover/cursor after wheel
    (`:723-724`).
  - Fix: covered by D1; add a Playwright test for each of the four behaviors.

- [x] **D9. Offscreen/worker charts follow a third rule set** (Medium, Verified)
  - Fixed: worker wheel, keys, and mouse/pen/touch pointer events route through the shared input
    controller. The worker replay covers mouse/touch drag thresholds, fixed-centroid pinch and
    continuation, foreign-pointer cancellation, price-axis wheel policy, axis double-click reset,
    arrow keys, and Home. All 14 worker Chromium cases pass; browser touch also enters the
    controller and its pinch, cancel, long-press, and separator cases pass.

- [x] **D10. Kinetic, reduced-motion and tooltip-dwell logic is duplicated** (Low, Verified)
  - Fixed: engine-owned motion and dwell policy drives browser mouse/touch and the GPUI adapter's
    host motion preference; tests: `reduced_motion_uses_discrete_arrow_steps_without_animation`,
    held-drag dwell regression, browser touch wake regression, and
    `native_motion_policy_tracks_host_setting` (active coast cancellation and discrete arrow replay).
  - Browser skips kinetic and keyboard animation for reduced motion (`gestures.ts:353, 1373`); the
    engine has no such option. `kinetic_touch` defaults to true in the browser (`impl.ts:3694`)
    while the engine's `kinetic_mouse` defaults to false. Kinetic samples use `performance.now()`
    instead of the event timestamp (`gestures.ts:709`). Trading tooltip dwell has a 450 ms
    TypeScript timer (`gestures.ts:18, 216-223`) copied from `TRADING_TOOLTIP_DWELL_MS`, and it
    restarts during a held drag (the engine does not, `chart_input.rs:1257`).
  - Fix: add `reduced_motion` to `InteractionOptions` (browser sets it from the media query,
    GPUI from the OS setting if available); one documented default for kinetic touch/mouse in the
    engine; event timestamps everywhere; the browser schedules the dwell from the engine deadline
    like the GPUI probe does.
  - Done when: engine tests cover reduced motion and dwell during drag; no dwell constant exists
    in TypeScript.

- [x] **D11. Browser text editor computes its own caret and font defaults** (Medium, Verified)
  - Fixed: `drawing_text_edit_layout` resolves the font, aligned run edge, rotated anchor,
    measured advance, and caret position from the engine typing session. The browser positions
    its transparent IME surface from that query and no longer measures glyphs or probes a
    separate baseline. An engine geometry test and a failing-before Chromium replay cover
    left/center/right alignment, a rotated trend label, and a caret inside the text; the
    existing text-tool and trend-label editor cases pass.

- [x] **D12. Axis label vertical centering is computed differently** (Medium, Verified)
  - Fixed: the engine lowers each axis label with the installed cap-center metric at that run's painted size and weight; browser and GPUI supply the same cap-height contract. Tests: `axis_text_uses_the_run_font_for_the_shared_cap_center_metric`, browser painted-size probe, 20 portable browser parity tests, 26 GPUI probe tests.
  - Browser derives a per-label ink correction at `layout.font_size × dpr` (12 px) while labels
    are drawn at `axis_font_size` (11 px) with no weight (`inner_render.rs:531-550`). Native passes
    0 and centers by ascent/descent (`gpui_probe.rs:1331`, `host_layout.rs:96`). Trading readouts
    use the ink of "0" in the browser (`chart.rs:795-809`) vs the font cap height in GPUI
    (`backend.rs:182-206`, `text.rs:130`).
  - Fix: one engine-defined metric (recommended: cap height at the exact size, weight and family
    of the run) that each host supplies through `set_text_cap_center`; the browser measures at the
    axis font size and weight.
  - Done when: both hosts supply the metric for the same font spec and a test asserts the
    browser measures at the axis size.

## E. Frame preparation and host-only features

- [x] **E1. Both hosts must use the engine's one frame-preparation call** (High, Verified)
  - Fixed: GPUI and browser render paths call `prepare_financial_frame_with_measure` for viewport,
    layout, base axis labels, and retained pane frame. The browser inserts plugin labels into the
    returned axis frame before lowering; synchronous geometry getters use the operation's
    layout-only phase. The engine owns the time-label cap at the painted axis font size and the
    grow-only repaint versus full-layout shrink rule. The source-path check finds no host call to
    `recompute_layout_with_measure` or `build_axis_frame`. Browser and GPUI deterministic fixtures
    with the same data and glyph widths both negotiate a 54 px axis. The browser reference matrix,
    named-scale, pane, and plugin tests and the GPUI probe tests pass.
  - Where: `ChartEngine::prepare_financial_frame_with_measure` (`host_layout.rs:41-103`) is only
    called from tests. `gpui_probe.rs:1302-1332` runs its own sequence; the browser
    (`crates/aeris_charts_wasm/src/chart/inner_render.rs:61-91`, `inner_api.rs:2159`) runs another.
    They use `axis_font_size()` for the label-width cap while the engine op uses
    `layout.font_size` (`host_layout.rs:92`), and relayout/shrink rules differ.
  - Fix: decide the correct font for the label-width cap (recommended: `axis_font_size`, since
    axis labels are drawn at that size), fix it in the engine op, and make the browser,
    offscreen and GPUI probe call it. Delete the host copies.
  - Done when: `rg "recompute_layout_with_measure|build_axis_frame" crates/aeris_charts_wasm
    crates/aeris_charts_render_gpui/examples` shows only the engine op being called, and a
    browser and a GPUI test produce the same axis width for the same chart.

- [x] **E2. Plugin text views are painted outside the frame in the browser** (Medium, Verified)
  - Fixed: browser pane and series `text_views` lower to ordered `Prim::Text` in each pane's
    scissored top layer before backend execution. The Canvas2D overlay path was deleted. A
    failing-before browser regression now sees the text in a pane-only frame capture and proves
    a later top-layer primitive covers it; Chromium also confirms WebGPU presented output and
    pane clipping. The native tiny-skia `Prim::Text` raster regression passes.
  - Where: `paint_primitive_text_overlay` (`crates/aeris_charts_wasm/src/chart/inner_render.rs:511,
    557-611`) paints primitive `text_views` on the overlay canvas after the whole frame, with its
    own DPR path. They always sit above axes and later primitives, and never appear in GPUI or
    native output.
  - Fix: convert `text_views` into ordered `Prim::Text` in the frame at their paint mark, like
    other host primitive output.
  - Done when: a test asserts plugin text appears in the frame `DrawList` at the right position
    and renders in tiny-skia output.

- [x] **E3. Browser-only tooltip and accessibility formatting ignores the chart formatter**
  (Low, Confirmed for time)
  - Fixed: a failing-before Chromium test showed a New York chart's December 31 label rendered
    as January 1 in the local-time tooltip. Tooltip and accessibility dates now use the engine's
    crosshair time formatter; an engine frame regression verifies it equals the actual zoned
    time-axis label. The existing tooltip test and nonlocal-zone regression pass. Tooltip OHLC
    lookup was already engine-owned, and financial accessibility prices already use the series'
    engine-backed `price_formatter()`. Their summary min/max and percent are aggregates over the
    selected accessibility scope, rather than alternate formatting of the engine value snapshot.
  - Where: `packages/charts/src/primitive_features.ts:671-682, 804-835` computes tooltip placement
    and formats timestamps with `Intl` in local time; `accessibility.ts:836-1009` computes
    min/max/percent change and dates with `toLocaleDateString`.
  - First: confirm whether these differ from the chart's configured time formatter/time zone.
  - Fix if confirmed: format through the engine's time formatter and value snapshot.
  - Done when: a test with a non-local chart time zone shows the same date text in the tooltip,
    the accessibility summary, and the time axis.

## F. Parity tests and CI

- [x] **F1. Add cross-backend cases for everything that has none** (High, Verified)
  - Fixed: the expanded native golden Prim scene is also consumed by GPUI/Canvas and WebGPU frame-contract tests; targeted tests cover image payloads, dashed/dotted and stepped polylines, bordered RoundRect, stroked Circle, and Background gradient extent. Browser parity exercises the same missing visual families through the public host APIs, including solid-ink geometry agreement within one device pixel; tests: `native_golden_scene_reaches_canvas_and_gpui_with_no_dropped_primitives`, `native_golden_scene_reaches_every_webgpu_pipeline_in_prim_order`, and `dashed and dotted pane paths, stepped series and stroked circles share browser geometry`.
  - Missing today: `Prim::Image`; `Polyline` with Dashed/Dotted style; `LineType::WithSteps`;
    `RoundRect` with `border_width > 0`; `Circle` with `stroke_width > 0`; `Background` in the
    WebGPU contract; Background gradient extent in GPUI parity (ignored at
    `crates/aeris_charts_render_gpui/tests/parity.rs:52`).
  - Fix: add each to the shared fixture set used by `parity.rs`, `frame_contract.rs`, the native
    goldens and the browser parity suite.

- [x] **F2. Compare geometry, not only routes and counts** (Medium, Verified)
  - Fixed: GPUI mesh positions now match every WebGPU contract vertex within 1e-3 device px for Polyline, AreaFill, BandFill, RoundRect and Triangle; Circle checks the nominal radius between GPUI's deliberate ±0.5 px coverage rows. Canvas2D/GPUI text checks include color and alignment; test: `gpui_meshes_contain_the_webgpu_contract_vertices_for_each_shape`, `text_runs_reach_both_backends_with_the_same_font_and_anchor`.
  - GPUI vs Canvas2D compares tessellated primitives only by route and triangle count; WebGPU
    contract compares only rect quads (`frame_contract.rs:263`). GPUI text parity ignores color
    and alignment (`parity.rs:75-85`).
  - Fix: compare vertex positions (within 1e-3 device px) for Polyline, AreaFill, BandFill,
    RoundRect, Circle and Triangle, and include text color and alignment in the recorder.

- [x] **F3. Make GPUI pixel parity run and assert** (High, Verified)
  - Fixed: Windows native-GPUI CI runs the official DWM capture harness; missing or nonidentical crisp-rect captures now fail alongside the image/icon gates. Test: local `cargo run -p aeris_charts_render_gpui --features gpui-backend --example pixel_parity` passes (0 crisp-rect differences); CI execution awaits review.
  - `examples/pixel_parity.rs` only prints PASS/FAIL for crisp rects (`:255-265`) and never runs
    in CI; `gpui-webgpu-matrix.spec.mjs:175-176` always skips in CI.
  - Fix: turn `pixel_parity` into an asserting test that runs in the `native-gpui` CI job on at
    least one OS with a GPU/software adapter, or document precisely why it cannot and add the
    closest asserting substitute.

- [x] **F4. Make browser pixel parity block merges** (High, Verified)
  - Fixed: the portable Chromium CI suite now includes deterministic WebGPU/Canvas2D and native/Canvas2D parity, while tagged reference fidelity reports remain machine evidence; test: portable `backend-parity.spec.mjs` (19 passing tests) and Playwright test-list partition.
  - `.github/workflows/ci.yml:140-149` runs `backend-parity.spec.mjs` (WebGPU vs Canvas2D presented
    frame, markers, native vs browser) with `continue-on-error: true`.
  - Fix: move the deterministic parts (WebGPU on SwiftShader vs Canvas2D, tiny-skia vs Canvas2D)
    into the blocking portable suite. Keep only timing-dependent measurements in the
    non-blocking machine step.

- [x] **F5. Review loose thresholds with evidence** (Medium, Verified)
  - Fixed: measured Windows SwiftShader residuals of 2,307/32 for the base, pane and series frames, 2,387/40 for custom series, 176/1 for primitive text, and 49 rotated-glyph pixels; tightened those budgets to 2,600/40, 2,700/48, 200/1 and 64 respectively. The >96 marker classifier stays just above its observed 90-step AA maximum, and drawings' >128 classifier stays just above the recorded CI 121-step diagonal AA maximum. Test: six targeted Chromium parity cases pass with the tighter limits.
  - The focused drawing WebGPU/Canvas2D case exposed 10 high-delta pixels on the rotated
    `+ Add text` prompt that its classifier treated as paint-order errors. The test now checks
    both rotated text transforms and bounds from the engine: 49 rotated-glyph pixels remain
    below its existing 128-pixel allowance and zero pixels fall outside those glyphs. Neither
    cutoff nor the zero-ordering-pixel assertion changed.
  - Full-frame ≤5000 px / max delta ≤64 (`backend-parity.spec.mjs:236-237`,
    `primitives.spec:159-160`, `series-primitives:167-168`, `custom-series:94-95`); ordering
    errors only counted at delta >96 (`backend-parity:276, 1102`); `drawings.spec:931` counts only
    delta >128 and exempts 128 rotated-glyph pixels (`:955`); `prim-text.spec:304-305` ≤250 px.
  - Fix: after A–E land, measure the actual residual differences and tighten each threshold to
    just above the measured value. Record the measurement in the test comment. Never loosen.

- [x] **F6. Make the native golden scene cover the full contract** (Medium, Verified)
  - Fixed: the reviewed 480×300 golden now includes Text, bordered RoundRect, BandFill, Triangle and RGBA Image; system-font glyphs are ink-checked and masked only for the exact bitmap diff. Test: `golden_scene_exercises_every_non_rect_contract_family` and the five-test native golden suite.
  - `crates/aeris_charts_native/tests/golden.rs` scene has no text, RoundRect, BandFill, Triangle
    or Image, and only compares tiny-skia with itself.
  - Fix: extend the scene (after C1–C5) and regenerate the golden once, with the diff reviewed.

## G. Documentation

- [x] **G1. Bring `docs/Architecture.md` back in line with the code**
  - Fixed: input, host lifecycle, frame preparation, primitive dash/image/color rules, and native/Linux parity gates now match the Cargo features, package exports/scripts, and actual call paths; test: architecture consistency, link/path, and documentation-hygiene review.
  - It currently states that hosts never re-implement routing, cursor priority or key bindings,
    which is false for the browser until D1 lands. After D1 and E1, update the input-controller,
    TypeScript package and frame-preparation sections, and add the image filter, RGBA/BGRA,
    degenerate-primitive and dash rules from A–B to the `aeris_charts_render` section.

## Final verification (for the reviewer)

Run every gate from `AGENTS.md` with zero warnings, plus Playwright and GPUI parity/replay
because this work changes browser and GPUI behavior:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p aeris_charts_wasm --target wasm32-unknown-unknown -- -D warnings
cargo test --workspace
cargo test -p aeris_charts_render_gpui --features gpui-backend
cargo run -p aeris_charts_native --example perf_gate --release

cd packages/charts
npm ci
npm run lint
npm run build
npm run typecheck
npm run test:pack
# plus the Playwright portable suite and backend-parity suite
```
