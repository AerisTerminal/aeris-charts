/**
 * Pointer/wheel/keyboard gesture recognizer wired onto the axis/input overlay canvas.
 *
 * The recognizer (event classification, slop, ownership, tracking mode) lives here per
 * browser event translation lives here, but every interaction model is engine-owned (`nucleuscharts_engine::interaction`)
 * and driven through the wasm handle: pan/scroll sessions, axis drag-to-scale, vertical price
 * pan, kinetic (momentum) coast, wheel/pinch zoom increments, and eased scroll animations —
 * the headless native harness runs the exact same code.
 * - touch: raw touch events with the reference's direction classification — a drag the chart does not
 *   own is released so the browser scrolls the page (reference does not use `touch-action: none`);
 *   long-press enters a crosshair "tracking" mode; two-finger pinch zooms,
 * - keyboard: arrows pan, +/- zoom, Home fit-content, Escape clear crosshair.
 * All behavior is gated by the resolved gesture config (`chart.gesture_config()`).
 */

import type { chart_impl } from "./impl.js";

const SLOP_MANHATTAN = 5; // px before a press becomes a drag (reference CancelClick/CancelTapManhattanDistance)
const SEP_HIT = 4; // css px hit tolerance around a pane boundary
const LONGPRESS_MS = 240; // touch hold before entering crosshair tracking (reference Delay.LongTap)
const TAP_RESET_MS = 500; // window for a second tap to count as a double-tap (reference Delay.ResetClick)
const DBL_TAP_MANHATTAN = 30; // max distance between the taps of a double-tap (reference DoubleTapManhattanDistance)
const TOUCH_MOUSE_SUPPRESS_MS = 500; // ignore synthetic mouse events after a touch (reference Delay.PreventFiresTouchEvents)
const KEY_SCROLL_MS = 160; // keyboard scroll animation (TradingView-style smooth step)

/** Active axis drag-to-scale session; the engine owns the start snapshot and the formulas. */
type AxisDrag = { kind: "price"; pane: number; target: number } | { kind: "time" };

/** Where a press landed; drives the touch ownership rules (the reference's per-widget handlers). */
type press_region = "pane" | "price_axis" | "time_axis" | "separator";

export function install_gestures(chart: chart_impl): () => void {
  const overlay = chart.overlay_el();
  const wasm = chart.wasm;
  // Active mouse/pen pointers (touch goes through the raw touch handlers below).
  const pointers = new Map<number, { x: number; y: number }>();
  let dragging = false; // a time-scale scroll session is active (mouse or touch)
  let sep_drag: { index: number; last_y: number } | null = null;
  let sep_hover = -1; // separator index last reported via set_separator_hover (-1 = none)
  let axis_drag: AxisDrag | null = null;
  let press_start: { x: number; y: number } | null = null;
  let moved = false; // mouse press moved past the click slop (reference _cancelClick)
  // Engine-owned drawing drag (anchor re-anchor or body move) started by a pane press on a
  // drawing (drawings.rs); mutually exclusive with a pan `dragging` session.
  let drawing_dragging = false;
  // Freehand brush capture in progress (the engine decimates/simplifies the stroke).
  let brush_drawing = false;
  // A text-tool press that already committed (mousedown placement) — the trailing click is
  // swallowed so it cannot re-open the typing-mode editor.
  let text_tool_press_committed = false;
  // Vertical price pan session (reference `startScrollPrice`): the engine holds the range
  // snapshot and shift math; armed only while the scale is NOT in autoscale (its no-op gate).
  let price_pan: { pane: number; target: number } | null = null;
  let last_pan_x: number | null = null;

  // --- touch-only state (mirrors the reference `MouseEventHandler` fields) ---
  let active_touch_id: number | null = null; // reference tracks only the first touch (plus pinch)
  let touch_start: { x: number; y: number } | null = null; // client coords at touchstart
  let touch_region: press_region = "pane";
  let touch_moved = false; // exceeded the tap slop (reference _touchMoveExceededManhattanDistance/_cancelTap)
  let touch_released = false; // gesture released to page scroll (reference _preventTouchDragProcess)
  let touch_scrolling = false; // deferred scroll session started (reference _isScrolling)
  let longpress_timer: ReturnType<typeof setTimeout> | null = null;
  let long_tap_active = false; // reference _longTapActive
  let tap_count = 0;
  let tap_timer: ReturnType<typeof setTimeout> | null = null;
  let tap_position = { x: 0, y: 0 }; // client coords of the first tap
  // No touch has happened yet; startup mouse input must never enter the 500 ms post-touch filter.
  let last_touch_ts = Number.NEGATIVE_INFINITY;
  const touch_regions = new Map<number, press_region>();

  // Crosshair tracking mode (reference _startTrackPoint !== null).
  let touch_tracking = false;
  let track_point: { x: number; y: number } | null = null;
  let init_crosshair: { x: number; y: number } | null = null;
  let exit_tracking_on_next_try = false; // reference _exitTrackingModeOnNextTry
  let last_crosshair: { x: number; y: number } | null = null;

  // Pinch (reference _startPinchMiddlePoint !== null); the zoom anchor is fixed at pinch start.
  let pinch_active = false;
  let pinch_mid_x = 0;
  let pinch_start_dist = 0;
  let pinch_prev_scale = 1; // reference _prevPinchScale
  let pinch_prevented = false; // reference _pinchPrevented

  let kinetic_raf: number | null = null; // RAF id while the engine's coast is being driven

  const local_xy = (e: { clientX: number; clientY: number }) => {
    const r = overlay.getBoundingClientRect();
    return { x: e.clientX - r.left - wasm.pane_left(), y: e.clientY - r.top };
  };

  const separator_at = (y: number): number => {
    const ys = wasm.pane_separator_ys();
    for (let i = 0; i < ys.length; i++) {
      if (Math.abs(y - ys[i]!) <= SEP_HIT) return i;
    }
    return -1;
  };

  /** Report the hovered separator to the engine (-1 = none); repaint only when it changes. */
  const set_sep_hover = (index: number) => {
    if (index === sep_hover) return;
    sep_hover = index;
    wasm.set_separator_hover(index);
    chart.repaint();
  };

  /** Price-axis target under `p` (0 = right, 1 = left), or null if not over a price axis. */
  const price_axis_target_at = (p: { x: number; y: number }): number | null => {
    if (p.x < 0) return 1;
    if (p.x > wasm.time_scale_width()) return 0;
    return null;
  };
  const is_time_axis = (p: { x: number; y: number }): boolean =>
    p.y > overlay.getBoundingClientRect().height - wasm.time_scale_height();
  const region_of = (p: { x: number; y: number }): press_region => {
    if (separator_at(p.y) >= 0) return "separator";
    if (price_axis_target_at(p) !== null) return "price_axis";
    if (is_time_axis(p)) return "time_axis";
    return "pane";
  };
  const pane_of = (y: number): number => wasm.pane_index_at_y(y);

  const set_crosshair = (x: number, y: number) => {
    last_crosshair = { x, y };
    wasm.set_crosshair(x, y);
    // Phase C-d: refresh the hover hit-test (primitives + series) before the repaint that
    // follows, so a hovered series' `hoveredSeriesOnTop` z-bump lands on the same frame.
    chart.update_hover(x, y);
    chart.emit_crosshair(x, y);
  };

  // TradingView's Ctrl-held magnet, scoped to DRAWING work: the Normal-mode crosshair snaps
  // to the hovered bar's OHLC only while a drawing tool is armed (anchor placement/preview) —
  // plain browsing never price-snaps on Ctrl. Forwarded on every pointer move/down and on
  // modifier key events, so a press/release without mouse movement still refreshes the snap live.
  const apply_crosshair_magnet = (e: { ctrlKey: boolean; metaKey: boolean }) => {
    wasm.set_crosshair_ohlc_magnet((e.ctrlKey || e.metaKey) && chart.creation_armed());
  };
  const on_modifier_key = (e: KeyboardEvent) => {
    if (e.key !== "Control" && e.key !== "Meta") return;
    apply_crosshair_magnet(e);
    if (last_crosshair !== null) {
      set_crosshair(last_crosshair.x, last_crosshair.y);
      chart.repaint();
    }
  };

  // reference `_firesTouchEvents`: synthetic mouse events fire within 500 ms of the last touch.
  const fires_touch_events = (e: { timeStamp: number; sourceCapabilities?: unknown }): boolean => {
    const caps = e.sourceCapabilities as { firesTouchEvents?: boolean } | null | undefined;
    if (caps && caps.firesTouchEvents !== undefined) return caps.firesTouchEvents;
    const ts = e.timeStamp || performance.now();
    return ts < last_touch_ts + TOUCH_MOUSE_SUPPRESS_MS;
  };

  const clear_longpress = () => {
    if (longpress_timer !== null) {
      clearTimeout(longpress_timer);
      longpress_timer = null;
    }
  };
  const reset_tap = () => {
    if (tap_timer !== null) {
      clearTimeout(tap_timer);
      tap_timer = null;
    }
    tap_count = 0;
  };

  const stop_kinetic = () => {
    wasm.kinetic_stop();
    if (kinetic_raf !== null) {
      cancelAnimationFrame(kinetic_raf);
      kinetic_raf = null;
      wasm.scroll_end();
    }
  };
  /** Drive the engine's kinetic coast; it continues the drag's scroll session (the RAF loop is
   *  host scheduling — the sampling, release speed, decay, and finish all live in the engine). */
  const start_kinetic = (release_x: number) => {
    const now = performance.now();
    if (!wasm.kinetic_release(release_x, now)) {
      wasm.scroll_end();
      return;
    }
    const step = () => {
      const t = performance.now();
      if (wasm.kinetic_finished(t)) {
        wasm.scroll_end();
        kinetic_raf = null;
        return;
      }
      wasm.scroll_move(wasm.kinetic_position(t));
      chart.repaint();
      kinetic_raf = requestAnimationFrame(step);
    };
    kinetic_raf = requestAnimationFrame(step);
  };

  /** Open a scroll session and (when the device wants a coast) start engine-side sampling. */
  const begin_scroll = (x: number, kind: "mouse" | "touch") => {
    wasm.scroll_start(x);
    dragging = true;
    last_pan_x = x;
    const cfg = chart.gesture_config();
    const enabled = kind === "touch" ? cfg.kinetic_touch : cfg.kinetic_mouse;
    wasm.kinetic_begin_sampling(enabled, x, performance.now());
  };
  /** Arm the vertical price pan on `pane` (reference `startScrollPrice`): prefers the right
   *  scale, falls back to the left; skipped under autoscale (the engine's own no-op gate). */
  const arm_price_pan = (pane: number, start_y: number) => {
    disarm_price_pan();
    for (const target of [0, 1]) {
      if (wasm.price_scale_auto_scale(pane, target) === false) {
        price_pan = { pane, target };
        wasm.price_axis_start_scroll(pane, target, start_y);
        break;
      }
    }
  };
  /** Close the engine's price-pan session (a no-op arm leaves nothing to close). */
  const disarm_price_pan = () => {
    if (price_pan === null) return;
    wasm.price_axis_end_scroll(price_pan.pane, price_pan.target);
    price_pan = null;
  };
  /** End a pan drag: coast when the flick qualifies, otherwise just close the session. */
  const end_drag = (kind: "mouse" | "touch") => {
    if (!dragging) return;
    dragging = false;
    touch_scrolling = false;
    disarm_price_pan();
    const cfg = chart.gesture_config();
    const enabled =
      (kind === "touch" ? cfg.kinetic_touch : cfg.kinetic_mouse) && !chart.prefers_reduced_motion();
    if (enabled && last_pan_x !== null) {
      start_kinetic(last_pan_x);
    } else {
      wasm.kinetic_stop();
      wasm.scroll_end();
    }
  };

  const apply_axis_drag = (p: { x: number; y: number }) => {
    if (axis_drag === null) return;
    // The engine owns the start snapshot and the scale formulas (reference `PriceScale.scaleTo`
    // / `TimeScale.scaleTo`); the recognizer just forwards the drag position.
    if (axis_drag.kind === "price") {
      wasm.price_axis_scale_to(axis_drag.pane, axis_drag.target, p.y);
    } else {
      wasm.time_axis_scale_to(p.x);
    }
  };
  const apply_sep_drag = (p: { x: number; y: number }) => {
    if (sep_drag === null) return;
    const dy = p.y - sep_drag.last_y;
    sep_drag.last_y = p.y;
    wasm.drag_pane_separator(sep_drag.index, dy);
  };
  const apply_price_pan = (y: number) => {
    if (price_pan === null) return;
    // Vertical price pan (reference `scrollPriceTo`): the engine shifts its armed snapshot by
    // dy * span/(h-1) — drag down moves the range up, so the candles follow the cursor.
    wasm.price_axis_scroll_to(price_pan.pane, price_pan.target, y);
  };

  /** Arm the press-region interaction shared by mousedown and touchstart; returns the region. */
  const arm_press = (p: { x: number; y: number }): press_region => {
    const cfg = chart.gesture_config();
    sep_drag = null;
    end_axis_drag();
    disarm_price_pan();
    // separator drag takes precedence over any pan/scale (reference layout.panes.enableResize gates it)
    const si = separator_at(p.y);
    if (si >= 0) {
      if (cfg.panes_resize) {
        sep_drag = { index: si, last_y: p.y };
        // The drag itself highlights the separator; clear the hover highlight.
        set_sep_hover(-1);
      }
      return "separator";
    }
    // axis drag-to-scale: price axis (vertical) / time axis (horizontal)
    const price_target = price_axis_target_at(p);
    if (price_target !== null) {
      const pane = pane_of(p.y);
      // reference `PriceScale.scaleTo` is a no-op in percentage and indexed-to-100 modes (and on
      // an empty scale) — the engine reports whether a drag can scale at all.
      if (cfg.axis_scale_price && wasm.price_axis_scalable(pane, price_target)) {
        axis_drag = { kind: "price", pane, target: price_target };
        wasm.price_axis_start_scale(pane, price_target, p.y);
      }
      return "price_axis"; // never pan from an axis strip
    }
    if (is_time_axis(p)) {
      if (cfg.axis_scale_time) {
        axis_drag = { kind: "time" };
        wasm.time_axis_start_scale(p.x);
      }
      return "time_axis";
    }
    return "pane";
  };
  /** Close the engine's axis scale session (reference `endScale` on pointer release). */
  const end_axis_drag = () => {
    if (axis_drag === null) return;
    if (axis_drag.kind === "price") {
      wasm.price_axis_end_scale(axis_drag.pane, axis_drag.target);
    } else {
      wasm.time_axis_end_scale();
    }
    axis_drag = null;
  };

  // ---------------------------------------------------------------------------------------------
  // Wheel (reference chart-widget.ts `_onMousewheel` + `_determineWheelSpeedAdjustment`)
  // ---------------------------------------------------------------------------------------------

  // reference `windowsChrome` = isChromiumBased() && isWindows(), resolved lazily for non-browser runs.
  let windows_chrome: boolean | null = null;
  const is_windows_chromium = (): boolean => {
    if (windows_chrome === null) {
      const nav = navigator as Navigator & {
        userAgentData?: { platform?: string; brands?: { brand: string }[] };
      };
      const chromium = nav.userAgentData?.brands?.some((b) => b.brand.includes("Chromium")) === true;
      const windows = nav.userAgentData?.platform
        ? nav.userAgentData.platform === "Windows"
        : navigator.userAgent.toLowerCase().indexOf("win") >= 0;
      windows_chrome = chromium && windows;
    }
    return windows_chrome;
  };
  const wheel_speed_adjustment = (e: WheelEvent): number => {
    switch (e.deltaMode) {
      case WheelEvent.DOM_DELTA_PAGE: // one screen at a time scroll mode
        return 120;
      case WheelEvent.DOM_DELTA_LINE: // one line at a time scroll mode
        return 32;
    }
    // Chromium on Windows mis-scales wheel deltas on high-density displays (Chromium issues
    // 1001735 / 1207308); reference corrects by 1/devicePixelRatio for consistent scroll speed.
    return is_windows_chromium() ? 1 / window.devicePixelRatio : 1;
  };

  const on_wheel = (e: WheelEvent) => {
    const cfg = chart.gesture_config();
    const adj = wheel_speed_adjustment(e);
    const delta_x = (adj * e.deltaX) / 100;
    const delta_y = -(adj * e.deltaY) / 100;
    const do_zoom = delta_y !== 0 && cfg.wheel_zoom;
    const do_scroll = delta_x !== 0 && cfg.wheel_scroll;
    if (!do_zoom && !do_scroll) return; // let the page scroll
    if (e.cancelable) e.preventDefault();
    if (do_zoom) {
      const pane_left = wasm.pane_left();
      const pane_w = wasm.time_scale_width();
      const x = e.offsetX;
      if (x < pane_left || x > pane_left + pane_w) {
        // TradingView-style price-axis wheel zoom (the reference has no price wheel; the time
        // axis wheel is `_onMousewheel` → `zoomTime`): anchored at the cursor's price.
        wasm.price_axis_wheel_zoom(
          wasm.pane_index_at_y(e.offsetY),
          x < pane_left ? 1 : 0,
          e.offsetY,
          wasm.wheel_zoom_scale(delta_y),
        );
      } else {
        // reference `_onMousewheel`: the normalized delta becomes the zoom increment (engine).
        wasm.zoom(x - pane_left, wasm.wheel_zoom_scale(delta_y));
      }
    }
    if (do_scroll) {
      // reference `scrollChart(deltaX * -80)`: "80 is a made up coefficient, and minus is for the
      // 'natural' scroll" (engine) — expressed as a scroll session spanning a single jump.
      wasm.scroll_start(0);
      wasm.scroll_move(wasm.wheel_scroll_delta(delta_x));
      wasm.scroll_end();
    }
    chart.repaint();
  };

  // ---------------------------------------------------------------------------------------------
  // Mouse / pen (pointer events; touch is handled by the raw touch handlers below)
  // ---------------------------------------------------------------------------------------------

  const on_down = (e: PointerEvent) => {
    if (e.pointerType === "touch") return;
    if (e.button !== 0) return; // primary button only (reference _mouseDownHandler)
    if (fires_touch_events(e)) return; // synthetic mouse event trailing a touch
    // Any mouse activity cancels touch tracking mode (reference `_onMouseEvent`).
    touch_tracking = false;
    track_point = null;
    stop_kinetic();
    stop_scroll_anim();
    try {
      overlay.setPointerCapture(e.pointerId);
    } catch {
      // ignore synthetic events with no active pointer
    }
    const p = local_xy(e);
    pointers.set(e.pointerId, p);
    apply_crosshair_magnet(e);
    // Any active pointer pauses the countdown timer (no mid-gesture repaint/lag).
    if (pointers.size === 1) chart.set_interacting(true);
    if (pointers.size !== 1) return;
    chart.native_delta_tooltip_mouse_down(p.x);
    press_start = p;
    moved = false;
    const region = arm_press(p);
    if (region !== "pane") return;
    // Drawing tools: an armed tool consumes pane presses (anchors place on click, not drag); a
    // successful drawing grab starts an engine-owned anchor/body drag. Both skip the pan.
    if (chart.creation_armed()) {
      // The brush captures the stroke as a press-drag (its own engine session, not clicks).
      if (chart.active_drawing_tool() === "brush" && chart.brush_create_start(p.x, p.y)) {
        brush_drawing = true;
      } else if (chart.active_drawing_tool() === "text") {
        // TradingView: the text tool places on PRESS and opens typing mode immediately.
        if (chart.creation_click(p.x, p.y, e.ctrlKey || e.metaKey, e.shiftKey)) {
          text_tool_press_committed = true;
          // Keep the browser's default mousedown focus-grab (to the overlay) from blurring
          // the just-opened editor.
          e.preventDefault();
        }
      }
      set_crosshair(p.x, p.y);
      chart.repaint();
      return;
    }
    if (wasm.drawing_drag_start_at(p.x, p.y)) {
      drawing_dragging = true;
      set_crosshair(p.x, p.y);
      chart.repaint();
      return;
    }
    // pane press: pan (time + price in one drag, like reference).
    if (chart.gesture_config().pan) {
      begin_scroll(p.x, "mouse");
      arm_price_pan(pane_of(p.y), p.y);
    }
    // reference `mouseDownEvent` places the crosshair at the press point.
    set_crosshair(p.x, p.y);
    chart.repaint();
  };

  const on_move = (e: PointerEvent) => {
    if (e.pointerType === "touch") return;
    if (fires_touch_events(e)) return;
    // Any mouse activity cancels touch tracking mode (reference `_onMouseEvent`).
    touch_tracking = false;
    track_point = null;
    // Ignore moves driven by a non-primary button drag (reference `_mouseMoveWithDownHandler`); a
    // hover (buttons === 0) or a left-drag (bit 0 set) passes.
    if (e.buttons !== 0 && (e.buttons & 1) === 0) return;
    const p = local_xy(e);
    chart.native_delta_tooltip_mouse_move(p.x);
    apply_crosshair_magnet(e);

    // active axis drag-to-scale
    if (axis_drag !== null) {
      apply_axis_drag(p);
      chart.repaint();
      return;
    }
    if (sep_drag !== null) {
      apply_sep_drag(p);
      // The separator is chrome (reference pane-separator.ts): the crosshair hides during the
      // resize drag instead of freezing mid-pane at the grab point.
      if (last_crosshair !== null) {
        last_crosshair = null;
        wasm.clear_crosshair();
        chart.clear_hover();
        chart.emit_crosshair_left();
      }
      chart.repaint();
      return;
    }

    // Separator hover highlight (no button pressed). The cursor itself is resolved after
    // the crosshair feed below, which refreshes the hover hit-test first.
    if (pointers.size === 0) {
      // Same gate as the row-resize cursor below: no hover highlight while resizing is off.
      set_sep_hover(chart.gesture_config().panes_resize ? separator_at(p.y) : -1);
    }

    if (pointers.has(e.pointerId)) pointers.set(e.pointerId, p);
    if (pointers.size > 0 && press_start !== null && !moved) {
      // reference CancelClickManhattanDistance = 5 (Manhattan).
      moved = Math.abs(p.x - press_start.x) + Math.abs(p.y - press_start.y) >= SLOP_MANHATTAN;
    }

    if (brush_drawing) {
      // Freehand brush: the engine decimates and captures the stroke points (drawings.rs).
      wasm.brush_create_add(p.x, p.y);
    } else if (drawing_dragging) {
      // Engine-owned anchor/body drag (drawings.rs): the engine re-anchors from the start
      // snapshot; the crosshair feed below keeps tracking the cursor. Modifier keys are
      // forwarded live (toggling mid-drag responds immediately, TradingView parity): Ctrl/Cmd =
      // magnet (snap anchors to the nearest bar's OHLC), Shift = straighten (0°/45°/90° anchor
      // constraint, dominant-axis body move). Ctrl never straightens.
      wasm.drawing_drag_to(p.x, p.y, e.ctrlKey || e.metaKey, e.shiftKey);
    } else if (dragging) {
      wasm.scroll_move(p.x);
      last_pan_x = p.x;
      wasm.kinetic_add_sample(p.x, performance.now());
      apply_price_pan(p.y);
    }
    // Crosshair: a hover over an axis strip is a pane mouseleave in the reference (its axis
    // strips are separate widgets) — the crosshair HIDES, and the hovered-source state clears
    // with it. The pane separator is chrome the same way (reference pane-separator.ts), so a
    // separator hover hides the crosshair too instead of pinning it onto the divider. During an
    // active captured drag keep feeding positions; the engine clamps them into the pane (the
    // reference's document-level drag listeners do the same).
    if (pointers.size > 0 || (price_axis_target_at(p) === null && !is_time_axis(p) && separator_at(p.y) < 0)) {
      set_crosshair(p.x, p.y);
    } else if (last_crosshair !== null) {
      last_crosshair = null;
      wasm.clear_crosshair();
      chart.clear_hover();
      chart.emit_crosshair_left();
    }
    // Interactive creation preview: the pending anchor follows the mouse (engine-owned), with
    // the same live modifier snaps as a placement click (Ctrl = magnet to OHLC, Shift =
    // straighten).
    if (chart.creation_active()) {
      chart.creation_move(p.x, p.y, e.ctrlKey || e.metaKey, e.shiftKey);
    }
    // Hover cursor feedback (no button pressed), resolved AFTER the crosshair feed refreshed
    // the hover hit-test, so a primitive's `hit_test` cursor applies on the same move it
    // starts hitting (reference applies the hovered source's cursorStyle on every crosshair move).
    if (pointers.size === 0) {
      const region_cursor =
        separator_at(p.y) >= 0 && chart.gesture_config().panes_resize
          ? "row-resize"
          : price_axis_target_at(p) !== null
            ? "ns-resize"
            : is_time_axis(p)
              ? "ew-resize"
              : "crosshair";
      // A primitive's cursor overrides the region cursor while its hit holds — but only over
      // the pane (the hover state is not refreshed over the axis strips). A series hit shows
      // the click affordance (TradingView-style: a series is selectable), falling back to the
      // region cursor off the geometry.
      overlay.style.cursor =
        region_cursor === "crosshair"
          ? (chart.hover_cursor() ??
            (chart.hover_series_id() !== null ? "pointer" : region_cursor))
          : region_cursor;
    }
    chart.repaint();
  };

  const end_pointer = (e: PointerEvent) => {
    if (e.pointerType === "touch") return;
    if (e.button !== 0) return; // primary button only (reference _mouseUpHandler)
    if (fires_touch_events(e)) return;
    // Any mouse activity cancels touch tracking mode (reference `_onMouseEvent`).
    touch_tracking = false;
    track_point = null;
    chart.native_delta_tooltip_mouse_up();
    pointers.delete(e.pointerId);
    if (pointers.size === 0) chart.set_interacting(false);
    if (pointers.size !== 0) return;
    if (sep_drag !== null) {
      sep_drag = null;
      return;
    }
    if (axis_drag !== null) {
      end_axis_drag();
      return;
    }
    if (brush_drawing) {
      // Commit the stroke (engine simplifies it into a smooth curved drawing, left selected).
      brush_drawing = false;
      chart.brush_create_end();
      chart.repaint();
      return;
    }
    if (drawing_dragging) {
      // End the engine's drawing drag (no coast, no scroll session to close — the pan path
      // never started). The click that follows (no move) routes to selection.
      drawing_dragging = false;
      wasm.drawing_drag_end();
      chart.repaint();
      return;
    }
    // reference `mouseUpEvent` ends the scroll (maybe starting a kinetic coast) but never hides the
    // crosshair — that only happens on mouse leave, Escape, or a touch end.
    end_drag("mouse");
    chart.repaint();
  };

  const on_cancel = (e: PointerEvent) => {
    if (e.pointerType === "touch") return;
    chart.native_delta_tooltip_mouse_up();
    pointers.delete(e.pointerId);
    if (pointers.size === 0) chart.set_interacting(false);
    if (pointers.size !== 0) return;
    sep_drag = null;
    end_axis_drag();
    if (brush_drawing) {
      brush_drawing = false;
      wasm.brush_create_cancel();
    }
    if (drawing_dragging) {
      drawing_dragging = false;
      wasm.drawing_drag_end();
    }
    end_drag("mouse");
    chart.repaint();
  };

  const on_leave = (e: PointerEvent) => {
    if (e.pointerType !== "mouse") return;
    if (fires_touch_events(e)) return;
    // reference `mouseLeaveEvent` hides the crosshair; an active captured drag is left alone.
    if (pointers.size > 0) return;
    chart.native_delta_tooltip_leave();
    set_sep_hover(-1);
    wasm.set_crosshair_ohlc_magnet(false); // release the Ctrl-magnet with the hover
    chart.clear_hover(); // Phase C-d: release the hover hit + hovered-series z-bump
    wasm.clear_crosshair();
    chart.emit_crosshair_left();
    chart.repaint();
  };

  const run_dblclick = (x: number, y: number) => {
    chart.emit_dbl_click(x, y);
    const cfg = chart.gesture_config();
    const rect = overlay.getBoundingClientRect();
    if (y > rect.height - wasm.time_scale_height()) {
      // reference time-axis-widget mouseDoubleClickEvent (handleScale.axisDoubleClickReset.time).
      if (cfg.axis_dblclick_reset_time) {
        wasm.reset_time_scale();
        chart.repaint();
      }
    } else if (x < 0 || x > wasm.time_scale_width()) {
      // reference price-axis-widget mouseDoubleClickEvent (handleScale.axisDoubleClickReset.price).
      if (cfg.axis_dblclick_reset_price) {
        wasm.set_price_scale_auto_scale(pane_of(y), x < 0 ? 1 : 0, true);
        chart.repaint();
      }
    }
  };

  const on_dblclick = (e: MouseEvent) => {
    if (moved) return;
    if (fires_touch_events(e)) return; // we already ran the double-tap path
    const p = local_xy(e);
    run_dblclick(p.x, p.y);
  };

  const on_click = (e: MouseEvent) => {
    if (moved) return;
    if (fires_touch_events(e)) return; // we already emitted the tap as a click
    const p = local_xy(e);
    // A text-tool press that already committed swallowed its trailing click (mousedown
    // placement opened the typing-mode editor).
    if (text_tool_press_committed) {
      text_tool_press_committed = false;
      return;
    }
    // An armed drawing tool consumes pane clicks for anchor placement (engine-owned creation);
    // modifiers snap the placed anchor (Ctrl = magnet to OHLC, Shift = straighten).
    if (chart.creation_armed() && chart.creation_click(p.x, p.y, e.ctrlKey || e.metaKey, e.shiftKey)) {
      chart.repaint();
      return;
    }
    chart.emit_click(p.x, p.y);
  };

  // reference `preventScrollByWheelClick` (helpers/events.ts): suppress Chrome's middle-click
  // autoscroll; registered Chrome-only like reference (`window.chrome !== undefined`).
  const on_mousedown = (e: MouseEvent) => {
    if (e.button === 1) e.preventDefault();
  };
  const is_chrome = (window as unknown as { chrome?: unknown }).chrome !== undefined;

  // ---------------------------------------------------------------------------------------------
  // Touch (reference MouseEventHandler touch path: no touch-action CSS, conditional preventDefault)
  // ---------------------------------------------------------------------------------------------

  const event_ts = (e: Event): number => e.timeStamp || performance.now();
  const touch_with_id = (list: TouchList, id: number): Touch | null => {
    for (let i = 0; i < list.length; i++) {
      if (list[i]!.identifier === id) return list[i]!;
    }
    return null;
  };

  const forward_delta_touches = (touches: TouchList): boolean => {
    const xs = new Float64Array(Math.min(2, touches.length));
    for (let index = 0; index < xs.length; index++) xs[index] = local_xy(touches[index]!).x;
    return chart.native_delta_tooltip_touch_move(xs);
  };

  // "Treat the drag as a page scroll" per press region (pane-widget.ts:142-143,
  // price-axis-widget.ts:206-207, time-axis-widget.ts:126-127, pane-separator.ts:154-155).
  const treat_vert_as_page_scroll = (): boolean => {
    const cfg = chart.gesture_config();
    switch (touch_region) {
      case "pane":
        return !touch_tracking && !cfg.pan_vert_touch;
      case "price_axis":
        return !cfg.pan_vert_touch;
      case "time_axis":
        return true;
      case "separator":
        // reference only gives the separator a handler while layout.panes.enableResize is on.
        return !cfg.panes_resize;
    }
  };
  const treat_horz_as_page_scroll = (): boolean => {
    const cfg = chart.gesture_config();
    switch (touch_region) {
      case "pane":
        return !touch_tracking && !cfg.pan_horz_touch;
      case "price_axis":
        return true;
      case "time_axis":
        return !cfg.pan_horz_touch;
      case "separator":
        return true;
    }
  };

  const end_pinch = () => {
    pinch_active = false;
  };
  /** reference `_startPinch`: fixed middle anchor, initial distance, prev scale 1, stop kinetic. */
  const start_pinch = (touches: TouchList) => {
    // reference registers pinch on the pane widget only — it never engages from an axis strip.
    const a = touches[0]!;
    const b = touches[1]!;
    if (touch_regions.get(a.identifier) !== "pane" || touch_regions.get(b.identifier) !== "pane") return;
    const rect = overlay.getBoundingClientRect();
    pinch_mid_x = (a.clientX - rect.left + (b.clientX - rect.left)) / 2 - wasm.pane_left();
    pinch_start_dist = Math.hypot(a.clientX - b.clientX, a.clientY - b.clientY);
    pinch_prev_scale = 1;
    pinch_active = true;
    stop_kinetic(); // reference pinchStartEvent → stopTimeScaleAnimation
    clear_longpress();
  };
  /** reference `_checkPinchState`, evaluated on every touchstart/touchend. */
  const check_pinch_state = (touches: TouchList) => {
    if (touches.length === 1) pinch_prevented = false;
    if (touches.length !== 2 || pinch_prevented || long_tap_active) {
      end_pinch();
    } else {
      start_pinch(touches);
    }
  };

  const on_touch_start = (e: TouchEvent) => {
    if (chart.native_delta_tooltip_touch_active() && e.cancelable) e.preventDefault();
    last_touch_ts = event_ts(e);
    stop_kinetic();
    stop_scroll_anim();
    // Any active touch pauses the countdown timer (no mid-gesture repaint/lag).
    if (e.touches.length > 0) chart.set_interacting(true);
    for (const t of Array.from(e.changedTouches)) {
      touch_regions.set(t.identifier, region_of(local_xy(t)));
    }
    check_pinch_state(e.touches);
    if (active_touch_id !== null) {
      // A second touch cancels the long-press, tracking mode, and any active pan.
      clear_longpress();
      touch_tracking = false;
      track_point = null;
      disarm_price_pan();
      end_axis_drag();
      sep_drag = null;
      if (dragging) {
        dragging = false;
        touch_scrolling = false;
        wasm.kinetic_stop();
        wasm.scroll_end();
      }
      return;
    }

    const touch = e.changedTouches[0]!;
    const p = local_xy(touch);
    active_touch_id = touch.identifier;
    touch_start = { x: touch.clientX, y: touch.clientY };
    press_start = p;
    touch_region = region_of(p);
    touch_moved = false;
    touch_released = false;
    touch_scrolling = false;
    long_tap_active = false;
    // reference `touchStartEvent`: a fresh touch while tracking arms the tracking-mode exit, and the
    // drag that follows re-anchors the crosshair on its current position.
    exit_tracking_on_next_try = touch_tracking;
    if (touch_tracking && last_crosshair !== null) {
      init_crosshair = last_crosshair;
      track_point = p;
    }

    // reference `longTapEvent` timer (Delay.LongTap); touchstart is passive — never preventDefault.
    clear_longpress();
    longpress_timer = setTimeout(on_longpress, LONGPRESS_MS);

    arm_press(p); // deferred: scroll/scale state only engages on the first owned move

    // reference tap bookkeeping (Delay.ResetClick window for double-tap detection).
    if (tap_timer === null) {
      tap_count = 0;
      tap_timer = setTimeout(reset_tap, TAP_RESET_MS);
      tap_position = { x: touch.clientX, y: touch.clientY };
    }
  };

  /** reference `longTapEvent`: enter tracking mode — crosshair at the press point, no panning. */
  const on_longpress = () => {
    longpress_timer = null;
    if (touch_moved || touch_released || active_touch_id === null || press_start === null) return;
    long_tap_active = true;
    if (!touch_tracking) {
      touch_tracking = true;
      exit_tracking_on_next_try = false;
      track_point = press_start;
      init_crosshair = press_start;
      set_crosshair(press_start.x, press_start.y);
      chart.repaint();
    }
  };

  const on_touch_move = (e: TouchEvent) => {
    const delta_tooltip_owns_move = forward_delta_touches(e.targetTouches);
    if (delta_tooltip_owns_move) {
      if (e.cancelable) e.preventDefault();
      chart.repaint();
    }
    // Pinch runs off the raw event (reference `_initPinch`), ahead of the single-touch machinery.
    if (pinch_active) {
      last_touch_ts = event_ts(e);
      if (e.touches.length === 2) {
        const a = e.touches[0]!;
        const b = e.touches[1]!;
        const dist = Math.hypot(a.clientX - b.clientX, a.clientY - b.clientY);
        if (chart.gesture_config().pinch_zoom) {
          // reference PaneWidget.pinchEvent: incremental scale ×5 (engine), no clamp (the engine
          // clamps spacing).
          const scale = dist / pinch_start_dist;
          const zoom_scale = wasm.pinch_zoom_scale(scale - pinch_prev_scale);
          pinch_prev_scale = scale;
          if (zoom_scale !== 0) {
            wasm.zoom(pinch_mid_x, zoom_scale);
            chart.repaint();
          }
        }
        if (e.cancelable) e.preventDefault();
      }
      return;
    }
    if (active_touch_id === null) return;
    const touch = touch_with_id(e.changedTouches, active_touch_id);
    if (touch === null) return;
    last_touch_ts = event_ts(e);
    if (touch_released) return;

    // Any move of the first touch before the second arrives prevents a later pinch
    // (reference `_pinchPrevented` — "prevent pinch if move event comes faster than the second touch").
    pinch_prevented = true;

    const dx = Math.abs(touch.clientX - touch_start!.x);
    const dy = Math.abs(touch.clientY - touch_start!.y);
    const manhattan = dx + dy;
    if (!touch_moved && manhattan < SLOP_MANHATTAN) return;

    if (!touch_moved) {
      // First move past the tap slop: classify the drag (reference `_touchMoveHandler`). The halved
      // x offset makes vertical drags win ties — "we scroll the page vertically more often".
      touch_moved = true;
      const corrected_x = dx * 0.5;
      const chart_owns =
        (dy >= corrected_x && !treat_vert_as_page_scroll()) ||
        (corrected_x > dy && !treat_horz_as_page_scroll());
      clear_longpress();
      reset_tap();
      if (!chart_owns) {
        // The page owns this gesture: release it and ignore the rest (reference _preventTouchDragProcess).
        touch_released = true;
        if (touch_scrolling) {
          touch_scrolling = false;
          dragging = false;
          disarm_price_pan();
          wasm.kinetic_stop();
          wasm.scroll_end();
        }
        return;
      }
    }

    if (e.cancelable) e.preventDefault(); // the chart owns the gesture — keep the page still
    const p = local_xy(touch);

    if (touch_tracking) {
      // Tracking mode: the drag moves the crosshair relative to its anchor (reference `touchMoveEvent`)
      // and disarms the exit a fresh touch had armed.
      exit_tracking_on_next_try = false;
      if (init_crosshair !== null && track_point !== null) {
        set_crosshair(init_crosshair.x + (p.x - track_point.x), init_crosshair.y + (p.y - track_point.y));
        chart.repaint();
      }
      return;
    }

    if (axis_drag !== null) {
      apply_axis_drag(p);
      chart.repaint();
      return;
    }
    if (sep_drag !== null) {
      apply_sep_drag(p);
      chart.repaint();
      return;
    }

    if (touch_region === "pane") {
      if (!touch_scrolling) {
        // Deferred scroll start (reference begins scrolling on the first qualifying move).
        touch_scrolling = true;
        begin_scroll(p.x, "touch");
        arm_price_pan(pane_of(p.y), p.y);
      }
      wasm.scroll_move(p.x);
      last_pan_x = p.x;
      wasm.kinetic_add_sample(p.x, performance.now());
      apply_price_pan(p.y);
      chart.repaint();
    }
  };

  const on_touch_end = (e: TouchEvent) => {
    if (e.targetTouches.length === 0 && chart.native_delta_tooltip_leave()) chart.repaint();
    check_pinch_state(e.touches);
    for (const t of Array.from(e.changedTouches)) {
      touch_regions.delete(t.identifier);
    }
    if (e.touches.length === 0) chart.set_interacting(false);
    let touch = active_touch_id !== null ? touch_with_id(e.changedTouches, active_touch_id) : null;
    if (touch === null && e.touches.length === 0) {
      // Somehow we missed the active touch's touchend (reference `_touchEndHandler` fallback).
      touch = e.changedTouches[0] ?? null;
    }
    if (touch === null) return;
    active_touch_id = null;
    last_touch_ts = event_ts(e);
    clear_longpress();

    // reference `touchEndEvent`: maybe exit tracking mode, then end the scroll.
    if (chart.gesture_config().tracking_exit_mode === "on_touch_end") {
      exit_tracking_on_next_try = true;
    }
    if (touch_tracking && exit_tracking_on_next_try) {
      touch_tracking = false;
      track_point = null;
      init_crosshair = null;
      wasm.clear_crosshair();
      chart.emit_crosshair_left();
    } else if (!touch_tracking) {
      // A plain touch never leaves a crosshair behind.
      wasm.clear_crosshair();
      chart.emit_crosshair_left();
    }
    end_drag("touch");
    chart.repaint();

    // Tap / double-tap (reference `_touchEndHandler`).
    const was_tap = !touch_moved && !long_tap_active;
    tap_count += 1;
    if (tap_timer !== null && tap_count > 1) {
      const d_tap =
        Math.abs(touch.clientX - tap_position.x) + Math.abs(touch.clientY - tap_position.y);
      if (d_tap < DBL_TAP_MANHATTAN && was_tap) {
        const p = local_xy(touch);
        run_dblclick(p.x, p.y);
      }
      reset_tap();
    } else if (was_tap) {
      // A tap: emit the click and suppress the synthetic one (reference preventDefault after tapEvent).
      const p = local_xy(touch);
      if (chart.creation_armed() && chart.creation_click(p.x, p.y, false, false)) {
        chart.repaint();
      } else {
        chart.emit_click(p.x, p.y);
      }
      if (e.cancelable) e.preventDefault();
    }
    if (tap_count === 0 && e.cancelable) {
      // A double-tap was just processed (reference: prevent Safari's dblclick zoom / fast-click).
      e.preventDefault();
    }
    if (e.touches.length === 0 && long_tap_active) {
      long_tap_active = false;
      if (e.cancelable) e.preventDefault(); // prevent the native click after a long-tap
    }
  };

  const on_touch_cancel = (e: TouchEvent) => {
    if (e.targetTouches.length === 0 && chart.native_delta_tooltip_leave()) chart.repaint();
    // reference clears the long-tap timeout on touchcancel. Additionally reset the active touch when
    // the browser stole the gesture (e.g. it took over for a page scroll): no touchend follows,
    // and a stuck active id would ignore the next touchstart.
    clear_longpress();
    check_pinch_state(e.touches);
    for (const t of Array.from(e.changedTouches)) {
      touch_regions.delete(t.identifier);
    }
    if (active_touch_id !== null && touch_with_id(e.touches, active_touch_id) === null) {
      active_touch_id = null;
      touch_released = true;
      if (dragging) {
        dragging = false;
        touch_scrolling = false;
        disarm_price_pan();
        wasm.kinetic_stop();
        wasm.scroll_end();
      }
    }
  };

  // ---------------------------------------------------------------------------------------------
  // Keyboard
  // ---------------------------------------------------------------------------------------------

  let scroll_anim: number | null = null;
  const stop_scroll_anim = () => {
    // A user gesture also supersedes an in-flight animated scroll_to_position.
    chart.cancel_scroll_animation();
    if (scroll_anim !== null) {
      cancelAnimationFrame(scroll_anim);
      scroll_anim = null;
    }
  };
  /** TradingView-style smooth keyboard scroll: the engine eases the scroll position to the
   *  target over ~160 ms (cubic ease-out, engine-owned) instead of jumping. The RAF loop is
   *  host scheduling; `rightOffset` semantics match reference: larger = newer view. */
  const animate_scroll_to = (target: number) => {
    stop_scroll_anim();
    if (wasm.scroll_position() === target) return;
    wasm.start_scroll_animation(target, KEY_SCROLL_MS, performance.now());
    const step_fn = () => {
      const done = Number.isNaN(wasm.scroll_animation_tick(performance.now()));
      chart.repaint();
      scroll_anim = done ? null : requestAnimationFrame(step_fn);
    };
    scroll_anim = requestAnimationFrame(step_fn);
  };

  const on_keydown = (e: KeyboardEvent) => {
    const cfg = chart.gesture_config();
    const step = e.ctrlKey || e.shiftKey ? 10 : 1;
    const center = wasm.time_scale_width() / 2;
    let handled = true;
    if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "z") {
      handled = e.shiftKey ? chart.redo_drawing() : chart.undo_drawing();
      if (handled) {
        e.preventDefault();
        stop_kinetic();
        chart.announce_view();
      }
      return;
    }
    switch (e.key) {
      // TradingView: Left scrolls back in time (older data), Right forward (newer data);
      // Ctrl/Shift steps 10 bars. reference rightOffset grows toward newer data, hence the signs.
      case "ArrowLeft":
        animate_scroll_to(wasm.scroll_position() - step);
        break;
      case "ArrowRight":
        animate_scroll_to(wasm.scroll_position() + step);
        break;
      case "+":
      case "=":
        if (cfg.wheel_zoom) wasm.zoom(center, 0.5);
        break;
      case "-":
      case "_":
        if (cfg.wheel_zoom) wasm.zoom(center, -0.5);
        break;
      case "Home":
        wasm.fit_content();
        break;
      case "Delete":
      case "Backspace":
        // TradingView-style: remove the selected drawing (engine-owned). Unhandled when
        // nothing is selected, so the keys keep their browser behavior then.
        handled = wasm.remove_selected_drawing();
        break;
      case "Escape":
        // Disarm a drawing tool / cancel a pending creation and deselect any drawing, then
        // the existing crosshair clear.
        chart.cancel_drawing_interaction();
        wasm.clear_crosshair();
        chart.repaint();
        chart.emit_crosshair_left();
        return;
      default:
        handled = false;
    }
    if (handled) {
      e.preventDefault();
      stop_kinetic();
      chart.repaint();
      chart.announce_view();
    }
  };

  overlay.addEventListener("wheel", on_wheel, { passive: false });
  overlay.addEventListener("pointerdown", on_down);
  overlay.addEventListener("pointermove", on_move);
  overlay.addEventListener("pointerup", end_pointer);
  overlay.addEventListener("pointercancel", on_cancel);
  overlay.addEventListener("pointerleave", on_leave);
  overlay.addEventListener("dblclick", on_dblclick);
  overlay.addEventListener("click", on_click);
  overlay.addEventListener("keydown", on_keydown);
  if (is_chrome) {
    overlay.addEventListener("mousedown", on_mousedown);
  }
  overlay.addEventListener("touchstart", on_touch_start, { passive: false });
  overlay.addEventListener("touchmove", on_touch_move, { passive: false });
  overlay.addEventListener("touchend", on_touch_end, { passive: false });
  overlay.addEventListener("touchcancel", on_touch_cancel, { passive: false });
  // Ctrl/Cmd press/release refreshes the crosshair magnet live (TradingView parity).
  window.addEventListener("keydown", on_modifier_key);
  window.addEventListener("keyup", on_modifier_key);
  // Hey mobile Safari, what's up? Without a non-passive touchmove listener Safari marks
  // touchstart and the following touchmoves cancelable=false, so the chart could not prevent
  // the page scroll once a drag starts (ported from reference mouse-event-handler.ts:654-659).
  const safari_dummy_touchmove = () => {};
  overlay.addEventListener("touchmove", safari_dummy_touchmove, { passive: false });

  return () => {
    stop_kinetic();
    stop_scroll_anim();
    clear_longpress();
    reset_tap();
    overlay.removeEventListener("wheel", on_wheel);
    overlay.removeEventListener("pointerdown", on_down);
    overlay.removeEventListener("pointermove", on_move);
    overlay.removeEventListener("pointerup", end_pointer);
    overlay.removeEventListener("pointercancel", on_cancel);
    overlay.removeEventListener("pointerleave", on_leave);
    overlay.removeEventListener("dblclick", on_dblclick);
    overlay.removeEventListener("click", on_click);
    overlay.removeEventListener("keydown", on_keydown);
    overlay.removeEventListener("mousedown", on_mousedown);
    overlay.removeEventListener("touchstart", on_touch_start);
    overlay.removeEventListener("touchmove", on_touch_move);
    overlay.removeEventListener("touchend", on_touch_end);
    overlay.removeEventListener("touchcancel", on_touch_cancel);
    overlay.removeEventListener("touchmove", safari_dummy_touchmove);
    window.removeEventListener("keydown", on_modifier_key);
    window.removeEventListener("keyup", on_modifier_key);
  };
}
