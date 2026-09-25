/**
 * Development-only Apache-2.0 compatibility fixture derived from the public `vertical-line` plugin
 * example and adapted to the Aeris canvas-primitive contract. Source:
 * tmp/refsrc/plugin-examples/src/plugins/vertical-line/vertical-line.ts plus
 * src/helpers/dimensions/positions.ts.
 *
 * Verbatim-ness: the renderer (`VertLinePaneRenderer.draw`) and the `positionsLine` helper are
 * character-for-character the reference's — the proof that a reference plugin painting with raw
 * canvas calls through `CanvasRenderingTarget2D` drops onto Aeris's `canvas_render_target`
 * unchanged. The view/primitive classes keep the reference structure 1:1; the only adaptations
 * are the host seams exposed by the development-only public reference fixture:
 * - `chart.timeScale().timeToCoordinate(t)` → Aeris's `chart.time_scale().time_to_coordinate(t)`
 *   (safe to call from the canvas hooks: the pass runs after the engine frame, not mid-render);
 * - the reference's `paneViews()` returns views whose `renderer()` hands an
 *   `IPrimitivePaneRenderer` to the host, which calls its `draw(target)`; Aeris's
 *   `canvas_pane_view.renderer(target)` IS that call, so the adapter at the bottom delegates
 *   (`view.renderer().draw(target)`);
 * - the reference attaches to a series and adds a time-axis label view; the Aeris canvas
 *   primitive is pane-bound and raw-canvas only (no axis views — the Prim-command primitives
 *   carry those), so `VertLineTimeAxisView` is dropped (`showLabel` defaults off upstream).
 * - zOrder: the reference view doesn't implement it (default 'normal'); Aeris's
 *   `canvas_pane_view.z_order` defaults to "normal" the same way, so the adapter omits it.
 * - the reference paints on the PANE widget's canvas (pane-offset coordinates); Aeris's plugin
 *   canvas is whole-chart, so the verbatim bitmap-x math lands exactly while the pane's left
 *   edge is the chart's left edge (no left price axis — the canvas layer's documented
 *   whole-chart limit; the demo's left-scale fixture does not enable this plugin).
 */

// --- reference helpers/dimensions/positions.ts (verbatim) ----------------------------------------
function centreOffset(lineBitmapWidth) {
  return Math.floor(lineBitmapWidth * 0.5);
}
function positionsLine(positionMedia, pixelRatio, desiredWidthMedia = 1, widthIsBitmap) {
  const scaledPosition = Math.round(pixelRatio * positionMedia);
  const lineBitmapWidth = widthIsBitmap ? desiredWidthMedia : Math.round(desiredWidthMedia * pixelRatio);
  const offset = centreOffset(lineBitmapWidth);
  const position = scaledPosition - offset;
  return { position, length: lineBitmapWidth };
}

// --- reference vertical-line.ts (structure verbatim; draw body character-for-character) -----------
class VertLinePaneRenderer {
  constructor(x, options) {
    this._x = x;
    this._options = options;
  }
  draw(target) {
    target.useBitmapCoordinateSpace(scope => {
      if (this._x === null) return;
      const ctx = scope.context;
      const position = positionsLine(
        this._x,
        scope.horizontalPixelRatio,
        this._options.width
      );
      ctx.fillStyle = this._options.color;
      ctx.fillRect(
        position.position,
        0,
        position.length,
        scope.bitmapSize.height
      );
    });
  }
}

class VertLinePaneView {
  constructor(source, options) {
    this._source = source;
    this._options = options;
    this._x = null;
  }
  update() {
    const timeScale = this._source._chart.time_scale();
    this._x = timeScale.time_to_coordinate(this._source._time);
  }
  renderer() {
    return new VertLinePaneRenderer(this._x, this._options);
  }
}

const defaultOptions = {
  color: 'green',
  labelText: '',
  width: 3,
  labelBackgroundColor: 'green',
  labelTextColor: 'white',
  showLabel: false,
};

export class VertLine {
  constructor(chart, series, time, options) {
    const vertLineOptions = {
      ...defaultOptions,
      ...options,
    };
    this._chart = chart;
    this._series = series;
    this._time = time;
    this._paneViews = [new VertLinePaneView(this, vertLineOptions)];
  }
  updateAllViews() {
    this._paneViews.forEach(pw => pw.update());
  }
  paneViews() {
    return this._paneViews;
  }
}

/**
 * The Aeris `canvas_primitive` adapter: the reference classes above stay untouched; only the
 * view→host seam maps (`paneViews()` → `pane_views()`, `view.renderer().draw(target)` →
 * `renderer(target)`). Attach with `pane.attach_canvas_primitive(vert_line_canvas_primitive(...))`.
 */
export function vert_line_canvas_primitive(chart, series, time, options) {
  const vert_line = new VertLine(chart, series, time, options);
  return {
    update_all_views() {
      vert_line.updateAllViews();
    },
    pane_views() {
      return vert_line.paneViews().map((view) => ({
        renderer: (target) => view.renderer().draw(target),
      }));
    },
  };
}
