# Origin Charts browser demo and parity gate

The demo imports the built `@origin/charts` distribution from `dist/`; it does not contain a second
chart model or renderer.

## Build

```powershell
npm install
npm run build
```

Serve this directory with any static server, or use `node test_server.mjs` for the no-cache local
server on port 4174.

## Browser parity tests

Install the pinned Chromium runtime once, then run the D1/R5 gates:

```powershell
npx playwright install chromium
npm run test:browser
```

The Playwright project uses full Chromium in new-headless mode and explicitly selects Dawn's
SwiftShader WebGPU adapter. It fails if automatic mode falls back to Canvas2D. The suite verifies:

1. `chart.take_screenshot()` is pixel-identical for a live WebGPU chart and a forced Canvas2D chart.
2. External PNG captures of the actually presented WebGPU and Canvas2D frames are pixel-identical.
3. The raw browser Canvas2D pane and native tiny-skia pane consume the shared JSON fixture and are
   pixel-identical before browser compositor scaling.
4. Public time-scale scrolling/reset/index/coordinate/dimension methods and chart/series price-
   scale options, manual ranges, autoscale, and normal/log/percentage/indexed modes are exercised
   through the package handles. Percentage and logarithmic ranges, axis width, logical range, and
   series coordinates are compared directly with the reference library. Series `data`, `data_by_index`,
   `bars_in_logical_range`, type and data-change contracts are covered too. A dedicated left-scale
   fixture proves that the left strip reserves layout space and matches the reference's width, range, series
   coordinate, round-trip conversion and logical window while the right strip is disabled.
5. Pinned reference 5.2.0 renders that fixture twice to prove its reference capture is
   deterministic, then Origin is compared with it after the same Chromium compositor. Full-frame,
   pane, price-axis, and time-axis differences have separate versioned regression ceilings. This
   is a measured fidelity gate, not yet a cross-library pixel-parity claim.
6. A seven-case reference matrix covers DPR 1/1.25/2/3, explicit bar spacings 0.5/6/50, and light/dark
   themes. Every case asserts identical public spacing and price-axis width before applying its
   versioned regional ceilings. At DPR 1 and spacing 6, the pane is byte-identical to the reference library.
7. Shared marker and overlay-volume fixtures are rendered through each library's public API at
   DPR 1.5 / spacing 6. A no-feature control separates existing candle/axis raster differences
   from feature-specific differences. Default marker autoscale is enabled in both libraries and
   the gate asserts identical visible ranges, price extents, and price-axis widths.

The current reference baseline differs perceptually in 3.368% of the full frame: 3.254% in the pane,
6.818% in the price-axis region, and 2.083% in the time-axis region. Exact RGBA differences are
reported too, but are not used as the cross-library threshold because their Canvas2D
antialiasing differs. At DPR 1 / spacing 6 the pane is byte-identical, both axis regions are
perceptually identical, and the time axis is also byte-identical. Axis text is rasterized in media
coordinates like the reference library, while the headless frame chooses price-label midpoint correction, stable
crosshair-time correction, and bold maximum-weight time ticks.
The wide-spacing case initially exposed two real engine defects—invisible series affecting
autoscale and eager price-axis shrinking. After fixing them, its full-frame perceptual difference
fell from 10.26% to 1.15% and its pane difference from 10.52% to 0.88%.
The feature fixture exposed hard-coded marker sizing, incorrect `inBar` anchoring and text offsets,
missing arrow stems, and hidden-series marker leakage in the engine. With those fixed, markers add
only 0.053 percentage points over the 0.827% no-feature pane control with default autoscale enabled.
Overlay volume currently
measures 1.627% for the pane. A maximum-bar probe matched value, x, top, base, and 8-device-pixel
geometry; the remaining gap is dominated by the reference's layered-canvas gap smearing at fractional DPR,
so this remains a browser-raster refinement target rather than a headless scale correction.

Backend failures contain `webgpu.png`, `canvas2d.png`, and `diff.png`; native failures contain the
native pane, browser pane, and their diff. Runtime data, layout, viewport and DPR are fixed by
`fixtures/d1/candles.json`, and the test server disables caching.
