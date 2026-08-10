/**
 * Canvas primitives (plugin platform Phase C-e) — the Canvas2D escape hatch, Option B of
 * The canvas callback path complements the Prim-command primitives (`primitives.ts`,
 * Phases C-a/C-b) record backend-neutral draw commands, a canvas primitive paints with
 * arbitrary Canvas2D calls through a {@link canvas_render_target} — a mirror of the
 * reference's `CanvasRenderingTarget2D` (fancy-canvas canvas-rendering-target.ts) — so
 * reference-style plugin renderers port near-verbatim.
 *
 * Locked limits (the design's Option B trade-offs):
 * - Plugin content is Canvas2D-only and paints on a dedicated **plugin overlay** canvas that
 *   sits above the pane canvases and below the axis/input overlay — so canvas primitives are
 *   always above the whole pane (the same whole-layer z-order the reference's top-layer
 *   primitives get), and always below the axis chrome and crosshair. Within the canvas the
 *   two z-order passes are honored: `normal` views draw first, `top` views above them.
 * - The pass is driven by the package's repaint flow after each engine frame; there is no
 *   engine (wasm) involvement, and no scissoring to the pane — clip manually if needed.
 *
 * The target mirrors the reference subset plugins actually use: both scopes expose the raw
 * `context` plus a size (`mediaSize`/`bitmapSize`) and the `horizontal`/`verticalPixelRatio`
 * pair. As in the reference, the scope is valid only for the duration of the synchronous
 * callback (the context's transform is restored afterwards).
 */

/** A width/height pair in px (reference fancy-canvas `Size`). */
export interface canvas_size {
  width: number;
  height: number;
}

/**
 * The media-space scope (reference `MediaCoordinatesRenderingScope`): the context is scaled
 * so 1 unit = 1 CSS px of the chart. `mediaSize` is the chart's CSS size.
 */
export interface media_coordinates_scope {
  readonly context: CanvasRenderingContext2D;
  readonly mediaSize: canvas_size;
  readonly horizontalPixelRatio: number;
  readonly verticalPixelRatio: number;
}

/**
 * The bitmap-space scope (reference `BitmapCoordinatesRenderingScope`): the context maps
 * 1:1 to device pixels. `bitmapSize` is the canvas backing-store size.
 */
export interface bitmap_coordinates_scope {
  readonly context: CanvasRenderingContext2D;
  readonly bitmapSize: canvas_size;
  readonly horizontalPixelRatio: number;
  readonly verticalPixelRatio: number;
}

/**
 * The draw target handed to a {@link canvas_pane_view} renderer (reference
 * `CanvasRenderingTarget2D`). Valid only for the duration of the synchronous `renderer` call.
 */
export interface canvas_render_target {
  /** Run `f` with the context in CSS-px coordinates (scale set so 1 unit = 1 CSS px). */
  useMediaCoordinateSpace<T>(f: (scope: media_coordinates_scope) => T): T;
  /** Run `f` with the context in raw bitmap (device) pixels. */
  useBitmapCoordinateSpace<T>(f: (scope: bitmap_coordinates_scope) => T): T;
}

/**
 * One paintable view of a canvas primitive (reference `IPanePrimitivePaneView`, reduced to
 * the layers a single above-pane canvas can express). `renderer` runs once per frame per
 * view; keep it cheap. Unlike the Prim-command primitives, a canvas renderer runs AFTER the
 * engine's frame is settled and presented, so reading chart state (coordinate converters)
 * from `update_all_views`/the renderer is safe — mutating chart state from them is not
 * (schedule a repaint from outside instead).
 */
export interface canvas_pane_view {
  /**
   * Draw pass within the plugin canvas (default `"normal"`): `normal` views paint first,
   * `top` views above them. Both passes composite above the whole pane (see the module
   * header's locked limits).
   */
  z_order?: "normal" | "top";
  /** Paint this view onto the plugin canvas. Synchronous; do not stash `target`. */
  renderer(target: canvas_render_target): void;
}

/**
 * A canvas primitive (the escape-hatch take on reference `IPanePrimitive`). Plain objects
 * and class instances both work — the package calls the hooks it finds as methods. Attach
 * with `pane_api.attach_canvas_primitive`.
 */
export interface canvas_primitive {
  /** Called once after attach with the owning pane's index. */
  attached?(params: { pane_index: number }): void;
  /** Called once after detach. */
  detached?(): void;
  /** Recompute cached geometry; called each frame before the views are read (like C-a). */
  update_all_views?(): void;
  /** The views to paint this frame. */
  pane_views?(): canvas_pane_view[];
}

/** Handle returned by `pane_api.attach_canvas_primitive`; drops the primitive from the chart. */
export interface canvas_primitive_handle {
  /** Remove the primitive, clear its paint, and repaint. Idempotent-ish: detaching twice is a no-op. */
  detach(): void;
}

/**
 * Build a {@link canvas_render_target} over an existing 2D context (reference
 * `tryCreateCanvasRenderingTarget2D`). `media_size` is the canvas' CSS size and
 * `bitmap_size` its backing-store size; the pixel ratios derive from the pair. The package
 * builds one per frame for the plugin canvas; this factory is exported for plugin tests and
 * offscreen use.
 */
export function create_canvas_render_target(
  context: CanvasRenderingContext2D,
  media_size: canvas_size,
  bitmap_size: canvas_size,
): canvas_render_target {
  const horizontal_pixel_ratio = bitmap_size.width / Math.max(media_size.width, 1e-9);
  const vertical_pixel_ratio = bitmap_size.height / Math.max(media_size.height, 1e-9);
  return {
    useMediaCoordinateSpace(f) {
      // reference useMediaCoordinateSpace: scale so one unit is one media (CSS) px.
      context.save();
      try {
        context.scale(horizontal_pixel_ratio, vertical_pixel_ratio);
        return f({
          context,
          mediaSize: media_size,
          horizontalPixelRatio: horizontal_pixel_ratio,
          verticalPixelRatio: vertical_pixel_ratio,
        });
      } finally {
        context.restore();
      }
    },
    useBitmapCoordinateSpace(f) {
      // reference useBitmapCoordinateSpace: the raw backing store, 1 unit = 1 device px.
      context.save();
      try {
        return f({
          context,
          bitmapSize: bitmap_size,
          horizontalPixelRatio: horizontal_pixel_ratio,
          verticalPixelRatio: vertical_pixel_ratio,
        });
      } finally {
        context.restore();
      }
    },
  };
}
