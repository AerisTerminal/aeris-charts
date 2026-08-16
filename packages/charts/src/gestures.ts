/**
 * Pointer/wheel/keyboard gesture recognizer wired onto the axis/input overlay canvas.
 *
 * Browser events are normalized here, while pointer membership, gesture transitions, pinch
 * centroid/distance, and rebasing live in `nucleuscharts_engine::interaction` through the WASM
 * input methods. Scale, drawing, trading, kinetic, and animation math remains engine-owned.
 * - touch: Pointer Events + pointer capture, long-press inspection, centroid pan/zoom pinch,
 * - keyboard: arrows pan, +/- zoom, Home fit-content, Escape clear crosshair.
 * All behavior is gated by the resolved gesture config (`chart.gesture_config()`).
 */

import type { chart_impl } from "./impl.js";

const SLOP_MANHATTAN = 5; // px before a press becomes a drag (reference CancelClick/CancelTapManhattanDistance)
const SEP_HIT = 4; // css px hit tolerance around a pane boundary
const LONGPRESS_MS = 240; // touch hold before entering crosshair tracking (reference Delay.LongTap)
const TAP_RESET_MS = 500; // window for a second tap to count as a double-tap (reference Delay.ResetClick)
const DBL_TAP_MANHATTAN = 30; // max distance between the taps of a double-tap (reference DoubleTapManhattanDistance)
const KEY_SCROLL_MS = 160; // keyboard scroll animation (TradingView-style smooth step)
const INPUT_UPDATE_LEN = 12;

const enum InputDeviceCode { Mouse = 0, Touch = 1, Pen = 2 }
const enum InputTargetCode { Pane = 0, Drawing = 1, Trading = 2, PriceAxis = 3, TimeAxis = 4, Separator = 5 }
const enum GestureUpdateCode {
  None = 0, Hover = 1, Pressed = 2, DragStarted = 3, DragMoved = 4,
  PinchStarted = 5, PinchMoved = 6, RebasedSinglePointer = 7, Released = 8,
  Cancelled = 9, LongPress = 10, Rejected = 11,
}

interface input_update {
  kind: GestureUpdateCode;
  pointer_id: number;
  target: InputTargetCode;
  device: InputDeviceCode;
  x: number;
  y: number;
  previous_x: number;
  previous_y: number;
  scale_delta: number;
  active_pointers: number;
  prevent_default: boolean;
}

/** Active axis drag-to-scale session; the engine owns the start snapshot and the formulas. */
type AxisDrag = { kind: "price"; pane: number; target: number } | { kind: "time" };

/** Where a press landed; drives the touch ownership rules (the reference's per-widget handlers). */
type press_region = "pane" | "price_axis" | "time_axis" | "separator";

export function install_gestures(chart: chart_impl): () => void {
  const overlay = chart.overlay_el();
  const wasm = chart.wasm;
  // Active captured pointers. The engine resolver owns the canonical membership/state.
  const pointers = new Map<number, { x: number; y: number }>();
  const pointer_targets = new Map<number, InputTargetCode>();
  const input_scratch = new Float64Array(INPUT_UPDATE_LEN);
  const one_touch_x = new Float64Array(1);
  const two_touch_xs = new Float64Array(2);
  let dragging = false; // a time-scale scroll session is active (mouse or touch)
  let sep_drag: { index: number; last_y: number } | null = null;
  let sep_hover = -1; // separator index last reported via set_separator_hover (-1 = none)
  let axis_drag: AxisDrag | null = null;
  let press_start: { x: number; y: number } | null = null;
  let moved = false; // mouse press moved past the click slop (reference _cancelClick)
  // Engine-owned drawing drag (anchor re-anchor or body move) started by a pane press on a
  // drawing (drawings.rs); mutually exclusive with a pan `dragging` session.
  let drawing_dragging = false;
  // Trading controls own the pointer before drawings and chart pan. Moves update only the
  // engine's local preview; confirmed broker state is never mutated by this gesture path.
  let trading_dragging = false;
  let trading_press = false;
  // Freehand brush capture in progress (the engine decimates/simplifies the stroke).
  let brush_drawing = false;
  // A text-tool press that already committed (mousedown placement) — the trailing click is
  // swallowed so it cannot re-open the typing-mode editor.
  let text_tool_press_committed = false;
  // Vertical price pan session (reference `startScrollPrice`): the engine holds the range
  // snapshot and shift math; armed only while the scale is NOT in autoscale (its no-op gate).
  let price_pan: { pane: number; target: number } | null = null;
  let last_pan_x: number | null = null;

  // Touch-only host state. Gesture classification itself is returned by the engine resolver.
  let active_touch_id: number | null = null;
  let touch_region: press_region = "pane";
  let touch_moved = false;
  let touch_scrolling = false;
  let longpress_timer: ReturnType<typeof setTimeout> | null = null;
  let long_tap_active = false;
  let tap_count = 0;
  let tap_timer: ReturnType<typeof setTimeout> | null = null;
  let tap_position = { x: 0, y: 0 }; // client coords of the first tap
  let suppress_compatibility_click = false;

  // Crosshair tracking mode (reference _startTrackPoint !== null).
  let touch_tracking = false;
  let track_point: { x: number; y: number } | null = null;
  let init_crosshair: { x: number; y: number } | null = null;
  let exit_tracking_on_next_try = false; // reference _exitTrackingModeOnNextTry
  let last_crosshair: { x: number; y: number } | null = null;

  // Pinch geometry is engine-owned; these fields only track the host scroll/price sessions.
  let pinch_active = false;

  let kinetic_raf: number | null = null; // RAF id while the engine's coast is being driven

  const local_xy = (e: { clientX: number; clientY: number }) => {
    const r = overlay.getBoundingClientRect();
    return { x: e.clientX - r.left - wasm.pane_left(), y: e.clientY - r.top };
  };

  const device_code = (pointer_type: string): InputDeviceCode =>
    pointer_type === "touch" ? InputDeviceCode.Touch
      : pointer_type === "pen" ? InputDeviceCode.Pen : InputDeviceCode.Mouse;
  const region_target = (region: press_region): InputTargetCode => {
    switch (region) {
      case "price_axis": return InputTargetCode.PriceAxis;
      case "time_axis": return InputTargetCode.TimeAxis;
      case "separator": return InputTargetCode.Separator;
      default: return InputTargetCode.Pane;
    }
  };
  const read_input_update = (): input_update => ({
    kind: input_scratch[0] as GestureUpdateCode,
    pointer_id: input_scratch[2]!,
    target: input_scratch[3] as InputTargetCode,
    device: input_scratch[4] as InputDeviceCode,
    x: input_scratch[5]!,
    y: input_scratch[6]!,
    previous_x: input_scratch[7]!,
    previous_y: input_scratch[8]!,
    scale_delta: input_scratch[9]!,
    active_pointers: input_scratch[10]!,
    prevent_default: input_scratch[11] !== 0,
  });
  const feed_pointer = (
    phase: "down" | "move" | "up",
    event: PointerEvent,
    target = pointer_targets.get(event.pointerId) ?? InputTargetCode.Pane,
  ): input_update => {
    const point = local_xy(event);
    const method = phase === "down" ? wasm.input_pointer_down.bind(wasm)
      : phase === "move" ? wasm.input_pointer_move.bind(wasm) : wasm.input_pointer_up.bind(wasm);
    const modifiers = (event.shiftKey ? 1 : 0) | (event.ctrlKey ? 2 : 0)
      | (event.altKey ? 4 : 0) | (event.metaKey ? 8 : 0);
    method(
      event.pointerId,
      device_code(event.pointerType),
      target,
      modifiers,
      point.x,
      point.y,
      event.timeStamp || performance.now(),
      event.pressure,
      event.tiltX,
      event.tiltY,
      input_scratch,
    );
    return read_input_update();
  };
  const set_touch_action = () => {
    const cfg = chart.gesture_config();
    overlay.style.touchAction = cfg.pan_vert_touch ? "none"
      : cfg.pan_horz_touch || cfg.pinch_zoom ? "pan-y" : "auto";
  };

  const separator_at = (y: number, touch = false): number => {
    const tolerance = touch ? 12 : SEP_HIT;
    const ys = wasm.pane_separator_ys();
    for (let i = 0; i < ys.length; i++) {
      if (Math.abs(y - ys[i]!) <= tolerance) return i;
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

  /** Exact price-axis target under `p`, including pane-local named scales. */
  const price_axis_target_at = (p: { x: number; y: number }): number | null => {
    return wasm.price_axis_target_at(pane_of(p.y), p.x) ?? null;
  };
  const is_time_axis = (p: { x: number; y: number }): boolean =>
    p.y > overlay.getBoundingClientRect().height - wasm.time_scale_height();
  const region_of = (p: { x: number; y: number }, touch = false): press_region => {
    if (separator_at(p.y, touch) >= 0) return "separator";
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
    chart.trading_hover_at(x, y);
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
    const scales = JSON.parse(wasm.price_scales_json(pane)) as {
      id: string; side: "left" | "right" | null; order: number | null; visible: boolean;
    }[];
    scales.sort((a, b) =>
      (a.order ?? Number.MAX_SAFE_INTEGER) - (b.order ?? Number.MAX_SAFE_INTEGER)
      || (a.side === "right" ? -1 : 1)
    );
    for (const scale of scales) {
      if (!scale.visible || scale.side === null) continue;
      const target = wasm.price_scale_target_by_id(pane, scale.id) ?? null;
      if (target === null) continue;
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
    trading_press = false;
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
  const arm_press = (p: { x: number; y: number }, touch = false): press_region => {
    const cfg = chart.gesture_config();
    sep_drag = null;
    end_axis_drag();
    disarm_price_pan();
    // separator drag takes precedence over any pan/scale (reference layout.panes.enableResize gates it)
    const si = separator_at(p.y, touch);
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
    const behavior = cfg.wheel_behavior === "pan" ? 1 : cfg.wheel_behavior === "zoom" ? 2 : 0;
    const intent = wasm.classify_wheel(behavior, delta_x, delta_y, e.deltaMode, e.ctrlKey);
    const pan_delta = cfg.wheel_behavior === "auto"
      ? delta_x : Math.abs(delta_x) >= Math.abs(delta_y) ? delta_x : -delta_y;
    const do_zoom = (intent & 2) !== 0 && delta_y !== 0 && cfg.wheel_zoom;
    const do_scroll = (intent & 1) !== 0 && pan_delta !== 0 && cfg.wheel_scroll;
    if (!do_zoom && !do_scroll) return; // let the page scroll
    if (e.cancelable) e.preventDefault();
    if (do_zoom) {
      const pane_left = wasm.pane_left();
      const x = e.offsetX;
      const pane = wasm.pane_index_at_y(e.offsetY);
      const target = wasm.price_axis_target_at(pane, x - pane_left) ?? null;
      if (target !== null) {
        // TradingView-style price-axis wheel zoom (the reference has no price wheel; the time
        // axis wheel is `_onMousewheel` → `zoomTime`): anchored at the cursor's price.
        wasm.price_axis_wheel_zoom(
          pane,
          target,
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
      wasm.scroll_move(wasm.wheel_scroll_delta(pan_delta));
      wasm.scroll_end();
    }
    chart.repaint();
  };

  // ---------------------------------------------------------------------------------------------
  // Mouse / pen (the same engine resolver also receives the touch path below)
  // ---------------------------------------------------------------------------------------------

  const on_down = (e: PointerEvent) => {
    if (e.pointerType === "touch") return on_touch_pointer_down(e);
    if (e.button !== 0) return; // primary button only (reference _mouseDownHandler)
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
    if (region !== "pane") {
      const target = region_target(region);
      pointer_targets.set(e.pointerId, target);
      feed_pointer("down", e, target);
      return;
    }
    const trading_hit = chart.trading_hit_at(p.x, p.y);
    if (trading_hit !== null) {
      pointer_targets.set(e.pointerId, InputTargetCode.Trading);
      feed_pointer("down", e, InputTargetCode.Trading);
      trading_dragging = chart.trading_drag_start_at(p.x, p.y);
      set_crosshair(p.x, p.y);
      chart.repaint();
      return;
    }
    chart.deactivate_trading_group();
    // Drawing tools: an armed tool consumes pane presses (anchors place on click, not drag); a
    // successful drawing grab starts an engine-owned anchor/body drag. Both skip the pan.
    if (chart.creation_armed()) {
      pointer_targets.set(e.pointerId, InputTargetCode.Drawing);
      feed_pointer("down", e, InputTargetCode.Drawing);
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
      pointer_targets.set(e.pointerId, InputTargetCode.Drawing);
      feed_pointer("down", e, InputTargetCode.Drawing);
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
    pointer_targets.set(e.pointerId, InputTargetCode.Pane);
    feed_pointer("down", e, InputTargetCode.Pane);
    // reference `mouseDownEvent` places the crosshair at the press point.
    set_crosshair(p.x, p.y);
    chart.repaint();
  };

  const on_move = (e: PointerEvent) => {
    if (e.pointerType === "touch") return on_touch_pointer_move(e);
    // Any mouse activity cancels touch tracking mode (reference `_onMouseEvent`).
    touch_tracking = false;
    track_point = null;
    // Ignore moves driven by a non-primary button drag (reference `_mouseMoveWithDownHandler`); a
    // hover (buttons === 0) or a left-drag (bit 0 set) passes.
    if (e.buttons !== 0 && (e.buttons & 1) === 0) return;
    const p = local_xy(e);
    feed_pointer("move", e);
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

    if (trading_dragging) {
      chart.trading_drag_to(p.y);
    } else if (brush_drawing) {
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
          ? (chart.trading_cursor_at(p.x, p.y) ?? chart.hover_cursor() ??
            (chart.hover_series_id() !== null ? "pointer" : region_cursor))
          : region_cursor;
    }
    chart.repaint();
  };

  const end_pointer = (e: PointerEvent) => {
    if (e.pointerType === "touch") return on_touch_pointer_up(e);
    if (e.button !== 0) return; // primary button only (reference _mouseUpHandler)
    // Any mouse activity cancels touch tracking mode (reference `_onMouseEvent`).
    touch_tracking = false;
    track_point = null;
    chart.native_delta_tooltip_mouse_up();
    feed_pointer("up", e);
    pointers.delete(e.pointerId);
    pointer_targets.delete(e.pointerId);
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
    if (trading_dragging) {
      trading_dragging = false;
      chart.trading_drag_end();
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
    if (e.pointerType === "touch") return on_touch_pointer_cancel(e);
    cancel_active_input();
  };

  const on_leave = (e: PointerEvent) => {
    if (e.pointerType !== "mouse") return;
    // reference `mouseLeaveEvent` hides the crosshair; an active captured drag is left alone.
    if (pointers.size > 0) return;
    chart.native_delta_tooltip_leave();
    set_sep_hover(-1);
    wasm.set_crosshair_ohlc_magnet(false); // release the Ctrl-magnet with the hover
    chart.clear_hover(); // Phase C-d: release the hover hit + hovered-series z-bump
    chart.clear_trading_hover();
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
    if (suppress_compatibility_click) return;
    const p = local_xy(e);
    run_dblclick(p.x, p.y);
  };

  const on_click = (e: MouseEvent) => {
    if (moved) return;
    if (suppress_compatibility_click) {
      suppress_compatibility_click = false;
      return;
    }
    const p = local_xy(e);
    // A text-tool press that already committed swallowed its trailing click (mousedown
    // placement opened the typing-mode editor).
    if (text_tool_press_committed) {
      text_tool_press_committed = false;
      return;
    }
    const trading_hit = chart.trading_hit_at(p.x, p.y);
    if (trading_hit !== null) {
      chart.trading_activate_at(p.x, p.y);
      chart.repaint();
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
  // Touch / pen direct manipulation (Pointer Events; gesture transitions live in Rust)
  // ---------------------------------------------------------------------------------------------

  const finish_scroll_without_coast = () => {
    if (!dragging) return;
    dragging = false;
    touch_scrolling = false;
    disarm_price_pan();
    wasm.kinetic_stop();
    wasm.scroll_end();
  };

  const cancel_active_input = () => {
    wasm.input_cancel_all(input_scratch);
    clear_longpress();
    reset_tap();
    finish_scroll_without_coast();
    end_axis_drag();
    sep_drag = null;
    pinch_active = false;
    if (brush_drawing) {
      brush_drawing = false;
      wasm.brush_create_cancel();
    }
    if (trading_dragging) {
      trading_dragging = false;
      chart.cancel_trading_drag();
    }
    if (drawing_dragging) {
      drawing_dragging = false;
      wasm.drawing_drag_cancel();
    }
    trading_press = false;
    touch_tracking = false;
    track_point = null;
    init_crosshair = null;
    active_touch_id = null;
    pointers.clear();
    pointer_targets.clear();
    chart.set_interacting(false);
    chart.native_delta_tooltip_mouse_up();
    chart.repaint();
  };

  const on_touch_pointer_down = (e: PointerEvent) => {
    stop_kinetic();
    stop_scroll_anim();
    const p = local_xy(e);
    try { overlay.setPointerCapture(e.pointerId); } catch { /* synthetic pointer */ }
    pointers.set(e.pointerId, p);
    chart.set_interacting(true);

    if (pointers.size > 1) {
      pointer_targets.set(e.pointerId, InputTargetCode.Pane);
      const update = feed_pointer("down", e, InputTargetCode.Pane);
      if (update.kind === GestureUpdateCode.Rejected) {
        pointers.delete(e.pointerId);
        pointer_targets.delete(e.pointerId);
        try { overlay.releasePointerCapture(e.pointerId); } catch { /* already released */ }
        return;
      }
      clear_longpress();
      reset_tap();
      touch_tracking = false;
      track_point = null;
      touch_moved = true;
      trading_press = false;
      if (drawing_dragging) {
        drawing_dragging = false;
        wasm.drawing_drag_cancel();
      }
      if (trading_dragging) {
        trading_dragging = false;
        chart.cancel_trading_drag();
      }
      if (brush_drawing) {
        brush_drawing = false;
        wasm.brush_create_cancel();
      }
      finish_scroll_without_coast();
      end_axis_drag();
      sep_drag = null;
      if (update.kind === GestureUpdateCode.PinchStarted) {
        pinch_active = true;
        dragging = true;
        touch_scrolling = true;
        last_pan_x = update.x;
        wasm.scroll_start(update.x);
        arm_price_pan(pane_of(update.y), update.y);
        if (e.cancelable && update.prevent_default) e.preventDefault();
      }
      return;
    }

    active_touch_id = e.pointerId;
    press_start = p;
    touch_region = region_of(p, true);
    touch_moved = false;
    touch_scrolling = false;
    long_tap_active = false;
    exit_tracking_on_next_try = touch_tracking;
    if (touch_tracking && last_crosshair !== null) {
      init_crosshair = last_crosshair;
      track_point = p;
    }

    const region = arm_press(p, true);
    let target = region_target(region);
    if (region === "pane") {
      if (chart.trading_hit_at_device(p.x, p.y, InputDeviceCode.Touch) !== null) {
        target = InputTargetCode.Trading;
        trading_press = true;
        trading_dragging = chart.trading_drag_start_at_device(p.x, p.y, InputDeviceCode.Touch);
      } else if (chart.creation_armed()) {
        target = InputTargetCode.Drawing;
        if (chart.active_drawing_tool() === "brush" && chart.brush_create_start(p.x, p.y)) {
          brush_drawing = true;
        }
      } else if (wasm.drawing_drag_start_at_device(p.x, p.y, InputDeviceCode.Touch)) {
        target = InputTargetCode.Drawing;
        drawing_dragging = true;
      } else {
        chart.deactivate_trading_group();
      }
    }
    pointer_targets.set(e.pointerId, target);
    const update = feed_pointer("down", e, target);
    if (update.kind === GestureUpdateCode.Rejected) {
      cancel_active_input();
      return;
    }

    clear_longpress();
    if (target === InputTargetCode.Pane) {
      longpress_timer = setTimeout(() => {
        longpress_timer = null;
        wasm.input_long_press(e.pointerId, input_scratch);
        const longpress = read_input_update();
        if (longpress.kind !== GestureUpdateCode.LongPress || press_start === null) return;
        long_tap_active = true;
        touch_tracking = true;
        exit_tracking_on_next_try = false;
        track_point = press_start;
        init_crosshair = press_start;
        set_crosshair(press_start.x, press_start.y);
        chart.repaint();
      }, LONGPRESS_MS);
    }

    if (tap_timer === null) {
      tap_count = 0;
      tap_timer = setTimeout(reset_tap, TAP_RESET_MS);
      tap_position = { x: e.clientX, y: e.clientY };
    }
  };

  const on_touch_pointer_move = (e: PointerEvent) => {
    const p = local_xy(e);
    pointers.set(e.pointerId, p);
    const xs = pointers.size > 1 ? two_touch_xs : one_touch_x;
    let xi = 0;
    for (const point of pointers.values()) {
      if (xi >= xs.length) break;
      xs[xi++] = point.x;
    }
    if (chart.native_delta_tooltip_touch_move(xs)) chart.repaint();

    const update = feed_pointer("move", e);
    if (update.kind === GestureUpdateCode.PinchMoved) {
      if (e.cancelable && update.prevent_default) e.preventDefault();
      if (!pinch_active) {
        pinch_active = true;
        dragging = true;
        touch_scrolling = true;
        wasm.scroll_start(update.previous_x);
        arm_price_pan(pane_of(update.previous_y), update.previous_y);
      }
      wasm.scroll_move(update.x);
      last_pan_x = update.x;
      apply_price_pan(update.y);
      if (chart.gesture_config().pinch_zoom && update.scale_delta !== 0) {
        wasm.zoom(update.x, wasm.pinch_zoom_scale(update.scale_delta));
      }
      chart.repaint();
      return;
    }
    if (e.pointerId !== active_touch_id || update.kind === GestureUpdateCode.None) return;
    if (update.kind === GestureUpdateCode.DragStarted) {
      touch_moved = true;
      clear_longpress();
      reset_tap();
    }
    if (e.cancelable && update.prevent_default) e.preventDefault();

    if (trading_dragging) {
      chart.trading_drag_to(p.y);
      set_crosshair(p.x, p.y);
    } else if (brush_drawing) {
      wasm.brush_create_add(p.x, p.y);
    } else if (drawing_dragging) {
      wasm.drawing_drag_to(p.x, p.y, e.ctrlKey || e.metaKey, e.shiftKey);
      set_crosshair(p.x, p.y);
    } else if (touch_tracking) {
      exit_tracking_on_next_try = false;
      if (init_crosshair !== null && track_point !== null) {
        set_crosshair(init_crosshair.x + (p.x - track_point.x), init_crosshair.y + (p.y - track_point.y));
      }
    } else if (axis_drag !== null) {
      apply_axis_drag(p);
    } else if (sep_drag !== null) {
      apply_sep_drag(p);
    } else if (touch_region === "pane") {
      if (!touch_scrolling) {
        touch_scrolling = true;
        begin_scroll(update.previous_x, "touch");
        arm_price_pan(pane_of(update.previous_y), update.previous_y);
      }
      wasm.scroll_move(p.x);
      last_pan_x = p.x;
      wasm.kinetic_add_sample(p.x, performance.now());
      apply_price_pan(p.y);
    }
    chart.repaint();
  };

  const on_touch_pointer_up = (e: PointerEvent) => {
    const p = local_xy(e);
    const update = feed_pointer("up", e);
    pointers.delete(e.pointerId);
    pointer_targets.delete(e.pointerId);
    clear_longpress();

    if (update.kind === GestureUpdateCode.RebasedSinglePointer) {
      finish_scroll_without_coast();
      pinch_active = false;
      active_touch_id = update.pointer_id;
      pointer_targets.set(update.pointer_id, InputTargetCode.Pane);
      dragging = true;
      touch_scrolling = true;
      touch_moved = true;
      last_pan_x = update.x;
      wasm.scroll_start(update.x);
      arm_price_pan(pane_of(update.y), update.y);
      suppress_compatibility_click = true;
      chart.repaint();
      return;
    }

    pinch_active = false;
    if (pointers.size === 0) chart.set_interacting(false);
    if (brush_drawing) {
      brush_drawing = false;
      chart.brush_create_end();
    } else if (trading_dragging) {
      trading_dragging = false;
      chart.trading_drag_end();
    } else if (drawing_dragging) {
      drawing_dragging = false;
      wasm.drawing_drag_end();
    } else {
      end_drag("touch");
    }

    if (chart.gesture_config().tracking_exit_mode === "on_touch_end") exit_tracking_on_next_try = true;
    if (touch_tracking && exit_tracking_on_next_try) {
      touch_tracking = false;
      track_point = null;
      init_crosshair = null;
      wasm.clear_crosshair();
      chart.emit_crosshair_left();
    } else if (!touch_tracking) {
      wasm.clear_crosshair();
      chart.emit_crosshair_left();
    }

    const was_tap = !touch_moved && !long_tap_active;
    tap_count += 1;
    if (tap_timer !== null && tap_count > 1) {
      const distance = Math.abs(e.clientX - tap_position.x) + Math.abs(e.clientY - tap_position.y);
      if (distance < DBL_TAP_MANHATTAN && was_tap) run_dblclick(p.x, p.y);
      reset_tap();
    } else if (was_tap) {
      const trading_hit = chart.trading_hit_at_device(p.x, p.y, InputDeviceCode.Touch);
      if (trading_press && trading_hit !== null) {
        chart.trading_activate_at(p.x, p.y);
      } else if (chart.creation_armed() && chart.creation_click(p.x, p.y, false, false)) {
        // creation handled
      } else {
        chart.emit_click(p.x, p.y);
      }
    }
    suppress_compatibility_click = true;
    if (e.cancelable) e.preventDefault();
    if (pointers.size === 0) active_touch_id = null;
    trading_press = false;
    long_tap_active = false;
    chart.repaint();
  };

  const on_touch_pointer_cancel = (e: PointerEvent) => {
    if (e.pointerType !== "touch") return;
    suppress_compatibility_click = true;
    cancel_active_input();
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
        // Discard a local trading preview before clearing the remaining transient interactions.
        chart.discard_trading_interaction();
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

  const on_lost_pointer_capture = (e: PointerEvent) => {
    if (pointers.has(e.pointerId)) on_cancel(e);
  };
  const cancel_if_active = () => {
    if (pointers.size > 0) cancel_active_input();
  };
  const on_visibility_change = () => {
    if (document.visibilityState !== "visible") cancel_if_active();
  };
  const gesture_resize_observer = new ResizeObserver(cancel_if_active);
  gesture_resize_observer.observe(overlay);
  set_touch_action();

  overlay.addEventListener("wheel", on_wheel, { passive: false });
  overlay.addEventListener("pointerdown", on_down);
  overlay.addEventListener("pointermove", on_move);
  overlay.addEventListener("pointerup", end_pointer);
  overlay.addEventListener("pointercancel", on_cancel);
  overlay.addEventListener("lostpointercapture", on_lost_pointer_capture);
  overlay.addEventListener("pointerleave", on_leave);
  overlay.addEventListener("dblclick", on_dblclick);
  overlay.addEventListener("click", on_click);
  overlay.addEventListener("keydown", on_keydown);
  if (is_chrome) {
    overlay.addEventListener("mousedown", on_mousedown);
  }
  // Ctrl/Cmd press/release refreshes the crosshair magnet live (TradingView parity).
  window.addEventListener("keydown", on_modifier_key);
  window.addEventListener("keyup", on_modifier_key);
  window.addEventListener("blur", cancel_if_active);
  window.addEventListener("nucleuscharts-chart-backend-lost", cancel_if_active);
  document.addEventListener("visibilitychange", on_visibility_change);

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
    overlay.removeEventListener("lostpointercapture", on_lost_pointer_capture);
    overlay.removeEventListener("pointerleave", on_leave);
    overlay.removeEventListener("dblclick", on_dblclick);
    overlay.removeEventListener("click", on_click);
    overlay.removeEventListener("keydown", on_keydown);
    overlay.removeEventListener("mousedown", on_mousedown);
    window.removeEventListener("keydown", on_modifier_key);
    window.removeEventListener("keyup", on_modifier_key);
    window.removeEventListener("blur", cancel_if_active);
    window.removeEventListener("nucleuscharts-chart-backend-lost", cancel_if_active);
    document.removeEventListener("visibilitychange", on_visibility_change);
    gesture_resize_observer.disconnect();
    cancel_if_active();
  };
}
