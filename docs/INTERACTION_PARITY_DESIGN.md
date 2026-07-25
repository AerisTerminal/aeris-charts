# Aion Charts — Interaction & Accessibility Parity (Design)

Status: **implemented — engine-owned.** Companion to [ARCHITECTURE.md](ARCHITECTURE.md).
Covers the three interaction gaps vs the reference charting library: **axis drag-to-scale**, **touch behavior**
(kinetic scroll + tracking mode), and **accessibility**.

> **Update (implementation landed):** the interaction MODELS all live in the engine
> (`aion_engine::interaction`, plus `aion_core::KineticAnimation` and the scale cores'
> `start_scale`/`scale_to`/`start_scroll`/`scroll_to`): axis drag-to-scale, vertical price pan,
> kinetic coast (§3a **option B** — the JS loop was never kept), wheel/pinch zoom increments,
> and the eased scroll-to-position animation (cubic ease-out engine-side). The TS gesture
> recognizer ([`packages/charts/src/gestures.ts`](../packages/charts/src/gestures.ts)) only
> classifies events, resolves ownership/config, and forwards normalized samples over the wasm
> boundary while scheduling the animation frames. Headless tests pin the physics in
> `aion_engine::interaction::tests` and `aion_core::model::kinetic_animation::tests`; browser
> coverage is `examples/web_demo/tests/interaction-models.spec.mjs`.

All interaction lives in the TS gesture recognizer
([`packages/charts/src/gestures.ts`](../packages/charts/src/gestures.ts)) driving wasm handles; the
engine stays headless. Where a behavior needs new state (kinetic physics), we choose between a
JS-side loop and an engine-side model — flagged per section.

---

## 1. Current state (grounded)

`install_gestures` today:
- **wheel** → `zoom` (time scale), gated by `handle_scale.wheel_zoom`.
- **1-finger / mouse drag anywhere** → `scroll_start`/`scroll_move` (pan), gated by `handle_scroll.pan`.
- **2-finger pinch** → `zoom`, gated by `handle_scale.pinch_zoom`.
- **double-click** on an axis → reset time scale / re-enable price autoscale (gated by
  `handle_scale.axis_dblclick_reset`).
- **separator drag** → pane resize; **hover** → crosshair + `row-resize` over separators.

A drag on an **axis strip** is treated identically to a pane drag (it pans). Touch and mouse share
one path: a touch drag both pans *and* moves the crosshair. There is no momentum. Accessibility is
just `touch-action: none` on the canvases — no ARIA, `tabindex`, or keyboard.

---

## 2. Axis drag-to-scale (reference `handleScale.axisPressedMouseMove`)

reference: pressing on the **price axis** and dragging vertically manually scales that price scale
(disabling autoscale); pressing on the **time axis** and dragging horizontally scales bar spacing.
Both gated by `handleScale.axisPressedMouseMove: { time, price }` (default both on).

**Region detection** — reuse the double-click logic already in `on_dblclick`:
`on_time_axis = y > height - time_scale_height()`; `on_price_axis = x < 0 || x > time_scale_width()`;
pane index from `pane_separator_ys()`.

**On pointerdown over an axis** → enter an `axis_drag` mode instead of `scroll_start`:
`{ kind: "price" | "time", pane, target, last: {x,y}, range }`.

**Price-axis drag (vertical)** — ported from `PriceScale.scaleTo` (model/price-scale.ts):
1. On start, read `price_scale_visible_range(pane, target)` → `[from, to]` snapshot (skipping the
   drag entirely in percentage / indexed-to-100 modes, like reference).
2. On move, `coeff = max(0.1, (startY + (h-1)*0.2) / (currentY + (h-1)*0.2))` with both Ys
   measured **up from the pane bottom**; scale the *snapshot* around its center:
   `half' = half * coeff`; `set_price_scale_visible_range(pane, target, mid-half', mid+half')`
   — which flips the scale to manual (matching reference). Drag down = zoom out.

**Time-axis drag (horizontal)** — ported from `TimeScale.scaleTo` (model/time-scale.ts):
`spacing = startSpacing * clamp(paneW - x, 0, paneW) / clamp(paneW - xStart, 0, paneW)` — a
start-relative ratio of distances from the pane's right edge (drag right = zoom out), clamped by
the existing min/max bar-spacing rules.

**Keyboard** — TradingView semantics on top of the reference's `rightOffset` (larger = newer view):
`ArrowLeft` steps older, `ArrowRight` newer, Ctrl/Shift = 10-bar step, eased over ~160 ms for
the platform's smooth feel (the reference's own keyboard-lessness means the mapping lives in the host).

**Cursor feedback** — extend the hover branch: `ns-resize` over a price axis, `ew-resize` over the
time axis, `row-resize` over separators, else `crosshair`.

**No engine change** (as originally scoped; the landed implementation routes the recognizer onto
the scale cores' own `start_scale`/`scale_to` so the formulas run engine-side — see the header
note). New option: `handle_scale.axis_pressed_mouse_move: boolean | { time, price }`
added to `handle_scale_options` (defaults true), resolved in `resolved_gestures`.

---

## 3. Touch behavior

### 3a. Kinetic / momentum scroll (reference `kineticScroll: { touch, mouse }`)
reference coasts after a flick. Two implementations:

- **A — JS momentum loop (recommended first):** in `gestures.ts`, track recent pointer velocity;
  on touch pointerup with velocity above a threshold, run a `requestAnimationFrame` loop calling
  `scroll_move` with exponentially decaying velocity until it stops or the user touches again.
  No engine change; easy to gate and to cancel on new input.
- **B — engine-side kinetic model:** port the reference's `KineticAnimation` into `TimeScaleCore` and advance it
  from the existing animation tick. More faithful and headless-testable, but larger.

~~Recommend **A first** (fast, cancellable, `prefers-reduced-motion`-aware), leaving B as an upgrade.~~
**Landed as B:** `aion_core::model::kinetic_animation::KineticAnimation` is the faithful px-domain
port; the engine samples during the drag and the host drives the coast from its RAF loop
(`kinetic_begin_sampling`/`kinetic_add_sample`/`kinetic_release`/`kinetic_position`), so the
physics are headless-tested. New option `kinetic_scroll: boolean | { touch, mouse }` (reference
default `{ touch: true, mouse: false }`).

### 3b. Touch crosshair vs pan (tracking mode, reference `trackingMode`)
Today one finger pans **and** drives the crosshair. reference on touch: a drag **scrolls** (no crosshair);
the crosshair appears via a distinct **tracking mode** (press-and-hold, then move to inspect; the
next tap exits). Proposal:
- Distinguish pointer type. **Mouse:** unchanged (hover crosshair; drag pans).
- **Touch:** a drag pans with **no** crosshair; a long-press (~250 ms without moving past the tap
  slop) enters tracking mode — subsequent moves set the crosshair and emit `crosshair_move` instead
  of panning; lifting or a tap exits. Honors `handle_scroll.horz_touch_drag` / `vert_touch_drag`.

### 3c. Wire existing touch sub-options
`handle_scroll.horz_touch_drag` / `vert_touch_drag` are already typed but ignored — gate the touch
pan axes on them.

---

## 4. Accessibility

Canvas charts are opaque to assistive tech, so the win is an accessible *wrapper*, not per-candle
semantics:
- **Container**: `role="application"`, `aria-label` (e.g. "Price chart, <symbol>"), `tabindex="0"`.
- **Live summary**: a visually-hidden, `aria-live="polite"` element updated on data/crosshair change
  with the latest value / hovered OHLC + time, so screen-reader users get the key information.
- **Keyboard** (overlay focused): `←/→` pan, `+/-` (or `↑/↓`) zoom the time scale, `Home` fit-content,
  `Esc` clear crosshair. Reuses `scroll_*` / `zoom` / `fit_content` handles — no engine change.
- **Reduced motion**: gate kinetic scroll and the last-price pulse on `prefers-reduced-motion`.
- Keep `touch-action: none` (already present) so gestures aren't hijacked by browser scrolling.

reference itself offers little keyboard/ARIA, so items above are a modest **superset** of reference — called out
as an intentional divergence rather than strict parity.

---

## 5. Phased plan

| Phase | Scope | Engine change | Size |
|-------|-------|---------------|------|
| **I-a** | Axis drag-to-scale (price + time) + axis cursors + `axis_pressed_mouse_move` option | none | M |
| **I-b** | Kinetic scroll (JS loop) + `kinetic_scroll` option + reduced-motion | none | M |
| **I-c** | Touch tracking mode + wire `horz/vert_touch_drag` | none | M |
| **I-d** | Accessibility wrapper: ARIA, live summary, keyboard nav | none | M |

Each phase adds a Playwright interaction test (synthetic pointer/touch/keyboard events) and, for the
option surfaces, TS type + `resolved_gestures` coverage. ~~None require engine changes; kinetic can be
upgraded to the engine-side model (3a-B) later if we want headless physics tests.~~ The models were
subsequently moved engine-side (header note): the physics are pinned headlessly in
`aion_engine::interaction::tests` / `aion_core::model::kinetic_animation::tests` and in the browser
by `examples/web_demo/tests/interaction-models.spec.mjs`.

## 6. Decisions for you

1. **Kinetic**: ~~JS loop first (recommended) or go straight to the engine-side model?~~ **Resolved:
   engine-side model (3a-B)** — `aion_core::KineticAnimation` + the engine's kinetic session API.
2. **Touch crosshair**: adopt the reference's long-press tracking mode (recommended for parity), or keep our
   simpler "drag shows crosshair"?
3. **Accessibility scope**: minimal (ARIA label + focusable + keyboard pan/zoom) or also the
   `aria-live` OHLC summary (recommended, the real screen-reader win)?
4. **Order**: the table above (axis → kinetic → tracking → a11y), or reprioritize?
