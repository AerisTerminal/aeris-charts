# Origin Charts — Pixel-Fidelity Rendering Specification

This document captures the **exact rendering math of the reference charting library v5** (studied from
`tmp/refsrc/src`). Our Rust/WebGPU engine must reproduce these formulas to be visually
indistinguishable. Every formula below is transcribed from the source with file references.

Conventions:

- `hpr` / `vpr` = horizontal / vertical pixel ratio (devicePixelRatio; fancy-canvas allows them to differ).
- **Media coordinates** = CSS pixels (all model/view math happens here).
- **Bitmap coordinates** = physical pixels (all rasterization happens here). Conversion is always
  `round(media * pixelRatio)` at draw time — *never* pre-scaled in the model.
- All rectangles are integer bitmap-pixel rects (`fillRect`), which is why the engine looks crisp.
  Our WebGPU renderer must emit integer-aligned quads for these primitives (no fractional AA edges).

---

## 1. Coordinate systems

### 1.1 Time scale: index → x (media px)  — `time-scale.ts`

The time scale works on **integer time-point indices** (0..N-1), not timestamps. Layout state is
`(barSpacing, rightOffset, baseIndex, width)`:

```
deltaFromRight = baseIndex + rightOffset - index
x = width - (deltaFromRight + 0.5) * barSpacing - 1
```

- `baseIndex` = index of the latest point that has data.
- `rightOffset` = margin from the right edge measured **in bars** (float).
- x is the **center** of the bar.

Inverse (x → float index):

```
deltaFromRight = (width - 1 - x) / barSpacing
floatIndex = baseIndex + rightOffset - deltaFromRight
floatIndex = round(floatIndex * 1e6) / 1e6            // fp-noise cleanup
index = ceil(floatIndex)                              // coordinateToIndex
```

Visible logical range (float, bar-centric):

```
barsLength  = width / barSpacing
rightBorder = rightOffset + baseIndex
leftBorder  = rightBorder - barsLength + 1
```

Strict range = `[floor(left), ceil(right)]` clamped to be non-empty.

Constraints:

- `minBarSpacing` default **0.5**, `maxBarSpacing` default `width * 0.5` (or option if > 0).
- Scroll clamps: at most `MinVisibleBarsCount = 2` bars must remain visible on each side:
  - `minRightOffset = firstIndex - baseIndex - 1 + max(2, points)` (or `width/barSpacing` bars if `fixLeftEdge`)
  - `maxRightOffset = width/barSpacing - min(2, points)` (0 if `fixRightEdge`)

Zoom (mouse wheel / pinch), `zoom(zoomPoint, scale)`:

```
floatIndexUnderCursor = coordinateToFloatIndex(zoomPoint)
newBarSpacing = barSpacing + scale * (barSpacing / 10)
setBarSpacing(newBarSpacing)
if !rightBarStaysOnScroll:
    rightOffset += floatIndexUnderCursor - coordinateToFloatIndex(zoomPoint)   // keep point under cursor
```

Wheel event → scale: `zoomScale = sign(deltaY) * min(1, |deltaY|)` where
`deltaY = -(speedAdj * event.deltaY / 100)`; speedAdj = 120 (page mode), 32 (line mode), 1 otherwise
(and `1/devicePixelRatio` on Windows Chrome). Horizontal wheel scroll: `scrollChart(deltaX * -80)` with
`deltaX = speedAdj * event.deltaX / 100`. Pinch: `zoomScale = (scale - prevScale) * 5`.

Axis-drag scale (dragging the time axis): `newBarSpacing = startBarSpacing * (width - x) / (width - startX)`
(lengths measured from the right edge, clamped to [0, width]).

Scroll: `rightOffset = startRightOffset + (startX - x) / barSpacing`.

### 1.2 Price scale: price → y (media px) — `price-scale.ts`

State: `priceRange {min,max}` (in "logical" space), `height`, margins, `invertScale`, mode.

```
topMarginPx    = scaleMargins.top    * height + marginAbovePx   (swapped if inverted)
bottomMarginPx = scaleMargins.bottom * height + marginBelowPx
internalHeight = height - topMarginPx - bottomMarginPx

logical = transform(price)        // identity | log10 | percent | indexedTo100
invCoordinate = bottomMarginPx + (internalHeight - 1) * (logical - min) / (max - min)
y = inverted ? invCoordinate : height - 1 - invCoordinate
```

Mode transforms (`price-scale-conversions.ts`):

- Percentage: `100 * (price - base) / base` (negated when `base < 0`).
- IndexedTo100: same + 100.
- Log: `sign(p) * (log10(|p| + coordOffset) + logicalOffset)`, 0 if `|p| < 1e-15`.
  Default formula `{logicalOffset: 4, coordOffset: 1e-4}`; when the raw range diff < 1, offsets grow:
  `logicalOffset = 4 + ceil(|log10(diff)|)`, `coordOffset = 10^-logicalOffset`.

Autoscale: union of each visible source's min/max over the strict visible range (chunked min/max
cache, chunk = 30 rows), converted per mode, plus margins. Degenerate range (min==max) is expanded
by `±5 * minMove`. Default `scaleMargins = { top: 0.2, bottom: 0.1 }`.

Price-axis drag scale (`scaleTo`): with `x' = height - x` (inverted), start snapshot of range:

```
coeff = (startX' + (height-1)*0.2) / (x' + (height-1)*0.2)
coeff = max(coeff, 0.1)
range = startRange.scaleAroundCenter(coeff)
```

Price-axis drag scroll: `priceDelta = pixelDelta * range.length / (internalHeight - 1)`; shift range.

---

## 2. Candlestick rendering — `candlesticks-renderer.ts`, `optimal-bar-width.ts`

All in **bitmap** pixels. Candles are drawn as three passes over visible range:
**wicks → borders → bodies** (each pass batches color changes via `fillStyle` caching).

### 2.1 Body width

```
optimalCandlestickWidth(barSpacing, pixelRatio):
    if 2.5 <= barSpacing <= 4:  return floor(3 * pixelRatio)
    coeff = 1 - 0.2 * atan(max(4, barSpacing) - 4) / (PI * 0.5)     // 1 → 0.8 as spacing grows
    res = floor(barSpacing * coeff * pixelRatio)
    return max(floor(pixelRatio), min(res, floor(barSpacing * pixelRatio)))
```

**Parity correction** (crosshair symmetry): crosshair/grid line width is `floor(hpr)`; if
`barWidth >= 2` and `floor(hpr) % 2 != barWidth % 2` then `barWidth -= 1`.

### 2.2 Wick

```
wickWidth = min(floor(hpr), floor(barSpacing * hpr))
wickWidth = max(floor(hpr), min(wickWidth, barWidth))
wickOffset = floor(wickWidth * 0.5)

top    = round(min(openY, closeY) * vpr)
bottom = round(max(openY, closeY) * vpr)
high   = round(highY * vpr);  low = round(lowY * vpr)
scaledX = round(hpr * x)
left  = scaledX - wickOffset
right = left + wickWidth - 1
// anti-overlap with previous candle's wick:
if prevEdge != null: left = min(max(prevEdge + 1, left), right)
width = right - left + 1
fillRect(left, high,       width, top - high)      // upper wick: high → body top
fillRect(left, bottom + 1, width, low - bottom)    // lower wick: body bottom+1 → low
prevEdge = right
```

Note: the wick is **clipped at the body** (never drawn behind it), and starts 1px below the body.

### 2.3 Border

```
borderWidth = floor(1 * pixelRatio)                     // BarBorderWidth = 1
if barWidth <= 2*borderWidth: borderWidth = floor((barWidth - 1) * 0.5)
res = max(floor(pixelRatio), borderWidth)
if barWidth <= res*2:  res = max(floor(pixelRatio), floor(1 * pixelRatio))   // no body case
```

Body rect: `left = round(x*hpr) - floor(barWidth*0.5)`, `right = left + barWidth - 1`,
with the same `prevEdge` anti-overlap as wicks. If `barSpacing*hpr > 2*borderWidth`, draw a hollow
frame (`fillRectInnerBorder`: 4 fillRects inside the rect); otherwise fill the whole rect.

### 2.4 Body

Bodies are only drawn if `!borderVisible || barWidth > borderWidth*2`. When borders are visible the
body rect is inset by `borderWidth` on all four sides. Skip if `top > bottom` after inset.

### 2.5 Colors / defaults (`candlestick-series.ts`)

```
upColor #26a69a  downColor #ef5350
borderUpColor #26a69a  borderDownColor #ef5350   (borderVisible: true)
wickUpColor #26a69a    wickDownColor #ef5350     (wickVisible: true)
```

## 3. Bar (OHLC) rendering — `bars-renderer.ts`

```
barWidth = max(floor(hpr), floor(optimalBarWidth(barSpacing, hpr)))
optimalBarWidth = floor(barSpacing * 0.3 * pixelRatio)
```

Same parity correction vs `max(1, floor(hpr))`. `barLineWidth = thinBars ? min(barWidth, floor(hpr)) : barWidth`.
Vertical body: from `round(min(highY,lowY)*vpr) - floor(barLineWidth/2)` to
`round(max(...)*vpr) + floor(barLineWidth/2)`, height at least `barLineWidth`.
Open/close ticks drawn when `barSpacing >= floor(1.5*hpr)`; tick horizontal extent = `ceil(barWidth * 1.5)`
from center; tick height = `barLineWidth`, clamped inside the body's vertical range.

## 4. Histogram — `histogram-renderer.ts`

```
spacing = ceil(barSpacing*hpr) <= 1 ? 0 : max(1, floor(hpr))
columnWidth = round(barSpacing * hpr) - spacing
```

Odd `columnWidth`: symmetric `±(w-1)/2` around `x`; even: shifted one px left (`left = x - w/2`,
`right = x + w/2 - 1`). Then a **position-correction pass** aligns adjacent columns so gaps are
exactly `spacing + 1` px (widening/narrowing based on which side the rounding shifted), a min-width
pass equalizes widths when `minWidth < 4`. Base line: `histogramBase` rounded, tick width
`max(1, floor(vpr))`; columns grow up or down from the base.

## 5. Line / Area / Baseline — `walk-line.ts`, `line-renderer-base.ts`

- `ctx.lineCap = 'butt'`, `ctx.lineJoin = 'round'`, `lineWidth = lineWidth * vpr`.
- Points are `(x*hpr, y*vpr)` floats — **no rounding** (lines are anti-aliased, unlike rects).
- Line types: Simple (`lineTo`), WithSteps (`lineTo(x, prevY); lineTo(x, y)`), Curved (cubic bezier
  with control points `cp1 = p[i] + (p[i+1] - p[i-1])/6`, `cp2 = p[i+1] - (p[i+2] - p[i])/6`).
- Color changes mid-line finish the current stroke and begin a new path; dash offset is carried
  across style changes by accumulated distance modulo dash-pattern length.
- Single visible point: draw a horizontal segment of `barWidth` centered on the point.
- Area fill: same path, closed to `bottom` (pane height), filled with vertical linear gradient
  (`topColor` default `rgba(46,220,135,0.4)`, `bottomColor` `rgba(40,221,100,0)`), line on top.
- Whitespace/NaN values split the polyline into separate segments (items with NaN y are skipped
  when building visible items).

## 6. Line styles / dash patterns — `draw-line.ts`

```
Solid: []          Dotted: [w, w]        Dashed: [2w, 2w]
LargeDashed: [6w, 6w]   SparseDotted: [w, 4w]      (w = ctx.lineWidth in bitmap px)
```

Horizontal/vertical 1-px-class lines get the classic half-pixel correction:
`correction = (lineWidth % 2) ? 0.5 : 0` added to the fixed axis. Multi-segment strokes use
`strokeInPixel`: translate(0.5, 0.5) when lineWidth is odd.

## 7. Grid — `grid-renderer.ts`

- `lineWidth = max(1, floor(hpr))` for both directions.
- Vertical lines at `round(timeMark.coord * hpr)`, horizontal at `round(priceMark.coord * vpr)`,
  drawn edge-to-edge (extended by ±lineWidth), with `strokeInPixel` half-pixel translate.
- Defaults: color `#D6DCDE`, solid, both visible. Drawn **after** bottom primitives, **before** series.
- Grid vertical lines are drawn only at time-axis tick-mark positions; horizontal at price marks.

## 8. Crosshair — `crosshair-renderer.ts`, `crosshair.ts`, `magnet.ts`

- Lines: `x = round(mediaX * hpr)`, width `floor(lineWidth * hpr)` (option width 1..4, default 1),
  default color `#9598A1`, style LargeDashed, drawn full width/height of the pane **on the top canvas layer**.
- Vertical line spans all panes; horizontal only on the pane under the cursor.
- Position: `index = coordinateToIndex(x)` clamped to visible strict range; crosshair x snaps to
  `indexToCoordinate(index)` (bar center), i.e. **snapping to bars is index-quantized**.
- Magnet (the reference's default mode; Origin deliberately defaults to Normal): finds, among visible series containing that index, the closest of
  (Close) — or (O,H,L,C) in MagnetOHLC mode — in *pixel* space, and snaps `y`/price to it.
  In Magnet mode the horizontal line/price is locked to series values; Normal mode follows the mouse.
- Crosshair marker (on line/area series): radius 4px circle + 2px stroke (crosshairMarkerRadius default 4).
- Axis labels: see §10 geometry; label background default `#131722`.

## 9. Price axis ticks — `price-tick-mark-builder.ts`, `price-tick-span-calculator.ts`

```
tickMarkHeight = ceil(fontSize * tickMarkDensity)      // density default 2.5
maxTickSpan = (high - low) * tickMarkHeight / scaleHeight
span = min over three PriceTickSpanCalculators with divider cycles:
       [2, 2.5, 2], [2, 2, 2.5], [2.5, 2, 2]
```

Span calculator: start `10^max(0, ceil(log10(high-low)))`, repeatedly divide by cycling dividers
while `span >= minMove`, `span >= maxTickSpan*c`, `span >= 1` (epsilon 1e-14); then if span == 1 and
fractional dividers exist (base decimal → [2, 2.5, 2]), continue dividing below 1. `minMove = 1/base`
(base = priceScale, e.g. 100 → 0.01).

Marks are placed at `logical = high - (high mod span)` stepping down by `span` while `> low`,
skipping marks closer than `tickMarkHeight` px to the previous (log scale re-computes span each
step). Labels formatted through the price formatter; coordinate via `logicalToCoordinate`.

Price formatter (`price-formatter.ts`): fraction length = log10(priceScale); minus sign is U+2212
(same width as +); fractional part padded with leading zeros.

## 10. Price axis label geometry — `price-axis-view-renderer.ts`, `price-axis-renderer-options-provider.ts`

Renderer options (media px, fontSize default 12):

```
borderSize = 1, tickLength = 5
paddingTop = paddingBottom = 2.5/12 * fontSize
paddingInner = paddingOuter = fontSize/12 * tickLength
```

Label box: `totalHeight = fontSize + paddingTop + paddingBottom` (bitmap height forced to same
parity as tick height); `totalWidth = borderSize + paddingInner + paddingOuter + textWidth + tickLength`;
corner radius `2*hpr`, rounded only on the side away from the pane. Text baseline `middle` with a
per-string yMidCorrection from the text-width cache. Tick: 1-px-class horizontal rect from the
scale edge, `tickLength` long.

Axis width (`price-axis-widget.optimalWidth()`):

```
max text width over: first & last tick labels, all back labels (series last-value, price lines),
crosshair-representative strings (floor(min)+0.11111111, ceil(max)-0.11111111)
width = ceil(borderSize + tickLength + paddingInner + paddingOuter + 5 /*LabelOffset*/ + maxTextWidth)
width += width % 2      // make even (Hi-DPI crispness)
```

Axis grows immediately, shrinks only lazily (`<` comparison, not `!==`, on marks-changed → full update).
All panes share the same left/right axis widths = max over panes.

Label overlap resolution (`_alignLabels`/`recalculateOverlapping`): labels split into top/bottom
halves around the first series' label coordinate; sorted from center outward; each overlapping label
is pushed away by label height; groups shift together while space remains; edge labels clamped to
half-height from the scale bounds.

## 11. Time axis — `time-axis-widget.ts`, `tick-marks.ts`, `time-scale-point-weight-generator.ts`

Height:

```
paddingTop = paddingBottom = 3/12 * fontSize
paddingHorizontal = 9/12 * fontSize
labelBottomOffset = 4/12 * fontSize
tickLength = 5, borderSize = 1
optimalHeight = ceil(borderSize + tickLength + fontSize + paddingTop + paddingBottom + labelBottomOffset)
height += height % 2
```

Tick-mark **weights** are assigned per time-scale point by comparing consecutive UTC timestamps:
Year(10) > Month(9) > Day(8) > 12h(7) > 6h(6) > 3h(5) > 1h(4) > 30m(3) > 5m(2) > 1m(1) > seconds/less.
(First point's weight is guessed using the average time diff.)

Label density: with fontSize F, `pixelsPer8Chars = (F + 4) * 5`; `maxLabelWidth = pixelsPer8Chars/8 *
maxTickMarkCharLength(default 8)`; `indexPerLabel = round(maxLabelWidth / barSpacing)`.
Mark selection (`TickMarks.build`): `maxIndexesPerMark = ceil(maxLabelWidth / barSpacing)`; iterate
weights descending; a mark of lower weight is kept only if it's ≥ maxIndexesPerMark away from
already-placed marks on both sides. (uniformDistribution: all-or-nothing per weight level.)

Label text: weight → TickMarkType (Year/Month/DayOfMonth/Time/TimeWithSeconds per
timeVisible/secondsVisible). Labels with weight ≥ max weight on screen render **bold**
(if allowBoldLabels); max weight is clamped down to Hour1 for intraday sub-day weights.
Crosshair time label and tick text use the same box metrics as the price axis
(paddingHorizontal for width). Time is UTC-based; timezone support = shift timestamps (documented
reference approach) or custom tickMarkFormatter/timeFormatter.

## 12. Layout & canvas layering — `chart-widget.ts`, `pane-widget.ts`

- DOM: a table — rows = panes (+2px separators), last row = time axis; row cells =
  [left axis][pane][right axis]; time axis row has corner "stubs" under the axes.
- **Every pane and axis has two stacked canvases**:
  - main canvas (z=1): background → bottom primitives → grid → series (normal z) → label views
  - top canvas (z=2): crosshair → top primitives → label views. Cleared and redrawn on **Cursor**
    invalidation; the main canvas is untouched → mouse-move is cheap.
- Pane draw order per source group: pane primitives first, then z-ordered sources; each in two
  passes (background views then foreground views). Hovered series optionally re-drawn on top.
- Pane heights: distributed by stretch factors (`paneHeight = round(stretch * stretchPixels * dpr)/dpr`,
  last pane takes the remainder ceil'd to dpr grid; min 2px; MIN_PANE_HEIGHT = 30 for user resizes).
- Chart total size suggestion: floor to even integers (Hi-DPI).
- Background: solid or vertical gradient (per-pane background uses `backgroundColorAtYPercentFromTop`).

## 13. Invalidation & frame scheduling — `invalidate-mask.ts`, `chart-widget.ts`

Levels: `None=0 < Cursor=1 < Light=2 < Full=3`; a mask holds a global level, per-pane levels
(+ autoScale flag), and a queue of **time-scale invalidations** (FitContent / ApplyRange /
ApplyBarSpacing / ApplyRightOffset / Reset / Animation / StopAnimation). Masks merge by max.

Frame loop: every model mutation calls `_invalidateHandler(mask)` → merge into pending mask →
`requestAnimationFrame` once; on frame: Full ⇒ rebuild GUI (pane widgets, axis widths, sizes);
Full|Light ⇒ momentary autoscale for flagged panes, apply time-scale invalidations (incl. animation
position), update axes; then paint every pane at its level (Cursor repaints only top canvases).
Ongoing kinetic/offset animations re-post a light mask each frame until finished.

- Cursor: crosshair move.
- Light: scroll/zoom/data update (geometry recompute, same layout).
- Full: options, series add/remove, axis width change, resize.

Kinetic scroll (touch default on, mouse off): constants `MinScrollSpeed=0.2, MaxScrollSpeed=7,
DumpingCoeff=0.997, ScrollMinMove=15` (all divided by barSpacing → units are bars); weighted-average
speed over last 4 touch positions; position `p(t) = p0 + v * (γ^t - 1)/ln γ`; duration solves
1px residual. Double-click axis → reset scale (`restoreDefault` / autoscale on).

## 14. Data model — `data-layer.ts`, `plot-list.ts`

- The union of all series' times forms the **time-scale point list** (sorted, deduped). Each series
  row stores `(index, time, value[4] = [open, high, low, close] (or value in all 4), originalTime)`.
  Whitespace rows participate in the time scale but not in the series plot list.
- `setData`: full rebuild, diffed against old points → `firstChangedPointIndex`; weights recomputed
  from that index; incremental `update(bar)` appends/replaces the last bar (O(log n) insert for a
  new time point, index re-sync to the right, no full rebuild).
- Series rows live in a `PlotList`: sorted array + binary search, `search(index, NearestLeft/Right)`,
  chunked min/max cache (chunk = 30) for autoscale.
- New-bar shift: if the last bar is visible and `shiftVisibleRangeOnNewBar`, the window follows;
  otherwise `rightOffset` is compensated so the viewport stays put.

## 15. Series option defaults worth copying

- Common: `lastValueVisible: true`, `priceLineVisible: true` (source LastBar), priceLine width 1
  dashed, `baseLineVisible: true` (#B2B5BE solid 1), priceFormat `{type: price, precision: 2, minMove: 0.01}`,
  `hitTestTolerance: 3`.
- Line: color `#2196f3`, width 3, `crosshairMarkerVisible: true`, radius 4, `lineType: Simple`,
  pointMarkers off, lastPriceAnimation Disabled.
- Chart: barSpacing 6, rightOffset 0, minBarSpacing 0.5; layout font 12px
  `-apple-system, BlinkMacSystemFont, 'Trebuchet MS', Roboto, Ubuntu, sans-serif`; text `#191919`,
  background `#FFFFFF`.
- Last-value label color: series color; text contrast color computed (generateContrastColors).

## 16. Hit testing

- Series range hit test (bars/candles): x within `barSpacing/2 + hitTestTolerance(3px)` of bar
  center, y within `[highY - tol, lowY + tol]`.
- Line series: distance to segment ≤ lineWidth/2 + tolerance.
- Pane hit test order: primitives (top → bottom z), then sources by reversed render order; result
  carries `{source, object {externalId, hitTestData}, cursorStyle, itemType}` and feeds
  `hoveredSeriesOnTop` re-draw + crosshair events.
