/**
 * OHLC custom-series fixture for the Nucleus plugin contract (plugin platform Phase C-c).
 *
 * The draw body mirrors the reference's `_drawImpl` 1:1: the same up/down rule (close vs the PREVIOUS
 * close), the same crisp-position math, and the same media-px `radius`. The only adaptation is
 * the coordinate space: Nucleus's render context
 * carries absolute BITMAP px (item x and `price_to_y` outputs), where the reference's renderer receives
 * pane-media coordinates and scales by `horizontal/verticalPixelRatio` inside its
 * `useBitmapCoordinateSpace` scope — so the helpers below run at pixelRatio 1, with widths
 * pre-scaled by `ctx.dpr` (the exact value the reference's `positionsLine(x, hpr, w)` computes).
 * `ctx.round_rect` stands in for the canvas `roundRect`; `ctx.rect` for `fillRect`.
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
function positionsBox(position1Media, position2Media, pixelRatio) {
  const scaledPosition1 = Math.round(pixelRatio * position1Media);
  const scaledPosition2 = Math.round(pixelRatio * position2Media);
  return {
    position: Math.min(scaledPosition1, scaledPosition2),
    length: Math.abs(scaledPosition2 - scaledPosition1) + 1,
  };
}

// --- reference helpers/dimensions/candles.ts (verbatim) ------------------------------------------
function optimalCandlestickWidth(barSpacing, pixelRatio) {
  const barSpacingSpecialCaseFrom = 2.5;
  const barSpacingSpecialCaseTo = 4;
  const barSpacingSpecialCaseCoeff = 3;
  if (barSpacing >= barSpacingSpecialCaseFrom && barSpacing <= barSpacingSpecialCaseTo) {
    return Math.floor(barSpacingSpecialCaseCoeff * pixelRatio);
  }
  const barSpacingReducingCoeff = 0.2;
  const coeff =
    1 - (barSpacingReducingCoeff * Math.atan(Math.max(barSpacingSpecialCaseTo, barSpacing) - barSpacingSpecialCaseTo)) / (Math.PI * 0.5);
  const res = Math.floor(barSpacing * coeff * pixelRatio);
  const scaledBarSpacing = Math.floor(barSpacing * pixelRatio);
  const optimal = Math.min(res, scaledBarSpacing);
  return Math.max(Math.floor(pixelRatio), optimal);
}
function candlestickWidth(barSpacing, horizontalPixelRatio) {
  let width = optimalCandlestickWidth(barSpacing, horizontalPixelRatio);
  if (width >= 2) {
    const wickWidth = Math.floor(horizontalPixelRatio);
    if (wickWidth % 2 !== width % 2) {
      width--;
    }
  }
  return width;
}

// --- reference helpers/dimensions/crosshair-width.ts (verbatim) ----------------------------------
function gridAndCrosshairBitmapWidth(horizontalPixelRatio) {
  return Math.max(1, Math.floor(horizontalPixelRatio));
}
function gridAndCrosshairMediaWidth(horizontalPixelRatio) {
  return gridAndCrosshairBitmapWidth(horizontalPixelRatio) / horizontalPixelRatio;
}

/**
 * An OHLC pane view as a Nucleus `custom_series_pane_view`.
 * `overrides` supplies plugin-side rendering options; `hooks.on_render` is a demo/test
 * observability hook receiving each frame's visible items.
 */
export function custom_ohlc_pane_view(overrides = {}, hooks = {}) {
  // Rendering defaults stay plugin-side; the engine's custom-series color arrives through
  // `default_options` below.
  const options = {
    upColor: "#089981",
    downColor: "#f7525f",
    wickUpColor: "#089981",
    wickDownColor: "#f7525f",
    radius: (bs) => (bs < 4 ? 0 : bs / 3),
    ...overrides,
  };
  return {
    // OHLC values contributing to autoscale.
    price_value_builder: (item) => [item.high, item.low, item.close],
    // A missing close denotes whitespace.
    is_whitespace: (item) => item.close === undefined,
    // reference customStyleDefaults (custom-series.ts): the engine's custom-series `color` default.
    default_options: { color: "#2196f3" },
    render(ctx) {
      hooks.on_render?.(ctx.items);
      if (ctx.items.length === 0) return;
      // reference renderer.ts _drawImpl: up when close >= the PREVIOUS bar's close (the example's
      // own rule — not the open).
      let lastClose = -Infinity;
      const bars = ctx.items.map(({ x, item }) => {
        const isUp = item.close >= lastClose;
        lastClose = item.close ?? lastClose;
        return {
          x,
          openY: ctx.price_to_y(item.open) ?? 0,
          highY: ctx.price_to_y(item.high) ?? 0,
          lowY: ctx.price_to_y(item.low) ?? 0,
          closeY: ctx.price_to_y(item.close) ?? 0,
          isUp,
        };
      });
      const radius = options.radius(ctx.bar_spacing);
      // _drawWicks: reference runs positionsLine(x, hpr, gridAndCrosshairMediaWidth(hpr)); the width
      // collapses to gridAndCrosshairBitmapWidth, and x is already bitmap here.
      const wickBitmapWidth = Math.round(gridAndCrosshairMediaWidth(ctx.dpr) * ctx.dpr);
      for (const bar of bars) {
        const vertical = positionsBox(bar.lowY, bar.highY, 1);
        const line = positionsLine(bar.x, 1, wickBitmapWidth);
        ctx.rect(
          line.position, vertical.position, line.length, vertical.length,
          bar.isUp ? options.wickUpColor : options.wickDownColor,
        );
      }
      // _drawCandles: "we want this in media width therefore using 1" (reference comment), then
      // positionsLine scales by the ratio — exactly Math.round(mediaWidth * ctx.dpr) here.
      // reference falls back to fillRect when the canvas lacks roundRect; Nucleus's analogue for a
      // zero radius is `rect` (a zero-radius roundRect is a plain rect, and the crisp quad
      // family keeps the backends pixel-identical — the rounded path renders when radius > 0).
      const bodyBitmapWidth = Math.round(candlestickWidth(ctx.bar_spacing, 1) * ctx.dpr);
      for (const bar of bars) {
        const vertical = positionsBox(Math.min(bar.openY, bar.closeY), Math.max(bar.openY, bar.closeY), 1);
        const line = positionsLine(bar.x, 1, bodyBitmapWidth);
        const color = bar.isUp ? options.upColor : options.downColor;
        if (radius > 0) {
          ctx.round_rect(line.position, vertical.position, line.length, vertical.length, radius, color);
        } else {
          ctx.rect(line.position, vertical.position, line.length, vertical.length, color);
        }
      }
    },
    destroy() {
      hooks.on_destroy?.();
    },
  };
}
