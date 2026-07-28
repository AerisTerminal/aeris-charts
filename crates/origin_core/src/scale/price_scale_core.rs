//! Price scale coordinate math and interactions. Port of `src/model/price-scale.ts`
//! (data-source management and formatter selection live at a higher layer).

use crate::model::price_range::PriceRange;
use crate::scale::log_formula::{self, LogFormula, DEF_LOG_FORMULA};
use crate::scale::price_tick_span_calculator::composite_tick_span;
use crate::Coordinate;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PriceScaleMode {
    Normal,
    Logarithmic,
    Percentage,
    IndexedTo100,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PriceScaleMargins {
    /// 0..1 fraction of height.
    pub top: f64,
    pub bottom: f64,
}

#[derive(Clone, Debug)]
pub struct PriceScaleCoreOptions {
    pub mode: PriceScaleMode,
    pub invert_scale: bool,
    pub auto_scale: bool,
    pub scale_margins: PriceScaleMargins,
    /// Tick mark label density (default 2.5); higher = fewer marks.
    pub tick_mark_density: f64,
    /// Layout font size in px (used for tick mark height).
    pub font_size: f64,
    /// reference `alignLabels` (default true): push the axis' boxed labels apart so they cannot
    /// overlap each other or leave the pane edge.
    pub align_labels: bool,
    /// reference `ticksVisible` (default false): draw a small tick mark beside each axis label.
    pub ticks_visible: bool,
    /// reference `entireTextOnly` (default false): skip top/bottom tick marks whose label text
    /// would be clipped by the pane edge.
    pub entire_text_only: bool,
    /// reference `minimumWidth` (default 0): floor for the axis strip width; the measured width
    /// still wins when the labels need more room.
    pub minimum_width: f64,
    /// reference `textColor` (default `None` = follow `layout.textColor`). Stored verbatim as a
    /// CSS string; parsed at render time.
    pub text_color: Option<String>,
    /// Origin extension (TradingView-style, default true): draw round-figure tick labels in the
    /// bold font — multiples of `step × 10` on uniform ticks, exact powers of ten on
    /// non-uniform (log) ticks.
    pub bold_round_labels: bool,
}

impl Default for PriceScaleCoreOptions {
    fn default() -> Self {
        Self {
            mode: PriceScaleMode::Normal,
            invert_scale: false,
            auto_scale: true,
            scale_margins: PriceScaleMargins {
                top: 0.2,
                bottom: 0.1,
            },
            tick_mark_density: 2.5,
            font_size: 12.0,
            // reference defaults (price-scale-options-defaults.ts).
            align_labels: true,
            ticks_visible: false,
            entire_text_only: false,
            minimum_width: 0.0,
            text_color: None,
            bold_round_labels: true,
        }
    }
}

/// A generated tick mark: coordinate (media px) + the logical value it represents.
/// Label formatting is the caller's concern (price formatter / localization).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PriceMark {
    pub coord: Coordinate,
    pub logical: f64,
}

#[derive(Clone, Debug)]
pub struct PriceScaleCore {
    options: PriceScaleCoreOptions,
    height: f64,
    price_range: Option<PriceRange>,
    /// Extra px margins requested by autoscale info providers.
    margin_above: f64,
    margin_below: f64,
    /// Height the fractional scale margins resolve against (0 = the scale's own height).
    margins_height: f64,
    log_formula: LogFormula,
    min_move: f64,
    scale_start_point: Option<f64>,
    scroll_start_point: Option<f64>,
    price_range_snapshot: Option<PriceRange>,
}

impl PriceScaleCore {
    pub fn new(options: PriceScaleCoreOptions) -> Self {
        Self {
            options,
            height: 0.0,
            price_range: None,
            margin_above: 0.0,
            margin_below: 0.0,
            margins_height: 0.0,
            log_formula: DEF_LOG_FORMULA,
            min_move: 0.01,
            scale_start_point: None,
            scroll_start_point: None,
            price_range_snapshot: None,
        }
    }

    pub fn options(&self) -> &PriceScaleCoreOptions {
        &self.options
    }

    pub fn mode(&self) -> PriceScaleMode {
        self.options.mode
    }

    /// Change coordinate mode while keeping any convertible manual range coherent. Percentage and
    /// indexed modes require a series base value, so—as in reference—they always re-enter autoscale.
    pub fn set_mode(&mut self, mode: PriceScaleMode) {
        let old = self.options.mode;
        if old == mode {
            return;
        }
        let raw_range = match (old, self.price_range) {
            (_, None) => None,
            (PriceScaleMode::Logarithmic, Some(range)) => Some(
                log_formula::convert_price_range_from_log(&range, &self.log_formula),
            ),
            (PriceScaleMode::Normal, Some(range)) => Some(range),
            // Percentage/indexed ranges cannot be reversed without the source's base value.
            _ => None,
        };
        self.options.mode = mode;
        match mode {
            PriceScaleMode::Normal => self.price_range = raw_range,
            PriceScaleMode::Logarithmic => {
                self.log_formula = log_formula::log_formula_for_price_range(raw_range.as_ref());
                self.price_range = raw_range
                    .as_ref()
                    .map(|range| log_formula::convert_price_range_to_log(range, &self.log_formula));
            }
            PriceScaleMode::Percentage | PriceScaleMode::IndexedTo100 => {
                self.options.auto_scale = true;
                self.price_range = None;
            }
        }
    }

    pub fn is_log(&self) -> bool {
        self.options.mode == PriceScaleMode::Logarithmic
    }

    pub fn is_percentage(&self) -> bool {
        self.options.mode == PriceScaleMode::Percentage
    }

    pub fn is_indexed_to_100(&self) -> bool {
        self.options.mode == PriceScaleMode::IndexedTo100
    }

    pub fn is_inverted(&self) -> bool {
        self.options.invert_scale
    }

    pub fn set_invert_scale(&mut self, inverted: bool) {
        self.options.invert_scale = inverted;
    }

    /// reference `alignLabels` — gate the axis' label overlap resolution.
    pub fn set_align_labels(&mut self, align: bool) {
        self.options.align_labels = align;
    }

    /// reference `ticksVisible` — draw small tick marks beside the axis labels.
    pub fn set_ticks_visible(&mut self, visible: bool) {
        self.options.ticks_visible = visible;
    }

    /// reference `entireTextOnly` — skip corner tick marks whose label would be clipped.
    pub fn set_entire_text_only(&mut self, entire: bool) {
        self.options.entire_text_only = entire;
    }

    /// reference `minimumWidth` — floor for the axis strip width (non-negative, finite).
    pub fn set_minimum_width(&mut self, width: f64) {
        if width.is_finite() && width >= 0.0 {
            self.options.minimum_width = width;
        }
    }

    /// reference `textColor` — `None` follows `layout.textColor`; stored verbatim, parsed at
    /// render time.
    pub fn set_text_color(&mut self, css: Option<String>) {
        self.options.text_color = css;
    }

    /// Origin extension (TradingView-style, default true): bold round-figure tick labels.
    pub fn set_bold_round_labels(&mut self, bold: bool) {
        self.options.bold_round_labels = bold;
    }

    pub fn is_auto_scale(&self) -> bool {
        self.options.auto_scale
    }

    pub fn set_auto_scale(&mut self, v: bool) {
        self.options.auto_scale = v;
    }

    pub fn log_formula(&self) -> &LogFormula {
        &self.log_formula
    }

    pub fn height(&self) -> f64 {
        self.height
    }

    pub fn set_height(&mut self, height: f64) {
        self.height = height;
    }

    /// Set the height the FRACTIONAL scale margins (`scale_margins.top/bottom`) are computed
    /// against (default 0 = the scale's own height). Stacked panes pin this to the pane's own
    /// slot height: the scale itself spans the full content height (clipped into the slot by
    /// the pixel internal margins), so taking the fractions of the full height would subtract
    /// up to 30% of the WHOLE chart from a small pane and invert its coordinate mapping.
    pub fn set_margins_height(&mut self, height: f64) {
        self.margins_height = height;
    }

    /// The height the fractional margins resolve against (the scale's own height unless a
    /// pane pinned its slot height).
    fn margins_height(&self) -> f64 {
        if self.margins_height > 0.0 {
            self.margins_height
        } else {
            self.height
        }
    }

    pub fn price_range(&self) -> Option<&PriceRange> {
        self.price_range.as_ref()
    }

    /// Price-scale API representation. Logarithmic storage is converted back to raw prices; the
    /// other modes expose their current display-domain range directly, matching reference.
    pub fn price_range_for_api(&self) -> Option<PriceRange> {
        let range = self.price_range?;
        Some(if self.is_log() {
            let raw = log_formula::convert_price_range_from_log(&range, &self.log_formula);
            let precision = if self.min_move > 0.0 {
                (-self.min_move.log10()).ceil().clamp(0.0, 15.0) as i32
            } else {
                0
            };
            let factor = 10_f64.powi(precision);
            let rounded = |value: f64| {
                (((value / self.min_move).round() * self.min_move) * factor).round() / factor
            };
            PriceRange::new(rounded(raw.min_value()), rounded(raw.max_value()))
        } else {
            range
        })
    }

    pub fn price_range_from_api(&self, range: &PriceRange) -> PriceRange {
        if self.is_log() {
            log_formula::convert_price_range_to_log(range, &self.log_formula)
        } else {
            *range
        }
    }

    pub fn set_price_range(&mut self, range: Option<PriceRange>) {
        self.price_range = range;
    }

    pub fn set_internal_margins(&mut self, above_px: f64, below_px: f64) {
        self.margin_above = above_px;
        self.margin_below = below_px;
    }

    /// Set the fractional scale margins (`top`/`bottom` as fractions of scale height). Used to pin
    /// an overlay scale to a band of the pane (e.g. volume in the bottom fifth).
    pub fn set_scale_margins(&mut self, top: f64, bottom: f64) {
        self.options.scale_margins.top = top;
        self.options.scale_margins.bottom = bottom;
    }

    pub fn is_empty(&self) -> bool {
        self.height == 0.0
            || self.price_range.is_none()
            || self.price_range.as_ref().is_some_and(|r| r.is_empty())
    }

    fn top_margin_px(&self) -> f64 {
        if self.is_inverted() {
            self.options.scale_margins.bottom * self.margins_height() + self.margin_below
        } else {
            self.options.scale_margins.top * self.margins_height() + self.margin_above
        }
    }

    fn bottom_margin_px(&self) -> f64 {
        if self.is_inverted() {
            self.options.scale_margins.top * self.margins_height() + self.margin_above
        } else {
            self.options.scale_margins.bottom * self.margins_height() + self.margin_below
        }
    }

    pub fn internal_height(&self) -> f64 {
        self.height - self.top_margin_px() - self.bottom_margin_px()
    }

    fn inverted_coordinate(&self, coordinate: f64) -> f64 {
        if self.is_inverted() {
            coordinate
        } else {
            self.height - 1.0 - coordinate
        }
    }

    // --- mode transforms ---

    /// price -> logical (space in which the range is linear).
    fn price_to_logical(&self, price: f64, base_value: f64) -> f64 {
        match self.options.mode {
            PriceScaleMode::Normal => price,
            PriceScaleMode::Logarithmic => {
                if price != 0.0 {
                    log_formula::to_log(price, &self.log_formula)
                } else {
                    price
                }
            }
            PriceScaleMode::Percentage => log_formula::to_percent(price, base_value),
            PriceScaleMode::IndexedTo100 => log_formula::to_indexed_to_100(price, base_value),
        }
    }

    /// Convert a raw source range to this scale's logical coordinate domain using the source's
    /// first visible value. Returns `None` when the mode/base cannot produce finite coordinates.
    pub fn price_range_to_logical(&self, raw: &PriceRange, base_value: f64) -> Option<PriceRange> {
        let a = self.price_to_logical(raw.min_value(), base_value);
        let b = self.price_to_logical(raw.max_value(), base_value);
        if !a.is_finite() || !b.is_finite() {
            return None;
        }
        Some(PriceRange::new(a.min(b), a.max(b)))
    }

    /// Public scalar counterpart used by engine-owned labels and series API conversion.
    pub fn price_to_logical_value(&self, price: f64, base_value: f64) -> f64 {
        self.price_to_logical(price, base_value)
    }

    fn logical_to_price(&self, logical: f64, base_value: f64) -> f64 {
        match self.options.mode {
            PriceScaleMode::Normal => logical,
            PriceScaleMode::Logarithmic => log_formula::from_log(logical, &self.log_formula),
            PriceScaleMode::Percentage => log_formula::from_percent(logical, base_value),
            PriceScaleMode::IndexedTo100 => log_formula::from_indexed_to_100(logical, base_value),
        }
    }

    // --- coordinate conversion (RENDERING_SPEC.md §1.2) ---
    //
    // Note on log mode: the stored price range is already in log space, so
    // `logical_to_coordinate` applies `to_log` to its input (matching reference where
    // `_logicalToCoordinate` re-transforms), while percent/indexed inputs are pre-transformed.

    pub fn logical_to_coordinate(&self, logical: f64) -> Coordinate {
        if self.is_empty() {
            return 0.0;
        }

        let logical = if self.is_log() && logical != 0.0 {
            log_formula::to_log(logical, &self.log_formula)
        } else {
            logical
        };

        let Some(range) = self.price_range.as_ref() else {
            return 0.0;
        };
        let inv_coordinate = self.bottom_margin_px()
            + (self.internal_height() - 1.0) * (logical - range.min_value()) / range.length();
        self.inverted_coordinate(inv_coordinate)
    }

    pub fn coordinate_to_logical(&self, coordinate: f64) -> f64 {
        if self.is_empty() {
            return 0.0;
        }

        let inv_coordinate = self.inverted_coordinate(coordinate);
        let Some(range) = self.price_range.as_ref() else {
            return 0.0;
        };
        let logical = range.min_value()
            + range.length()
                * ((inv_coordinate - self.bottom_margin_px()) / (self.internal_height() - 1.0));

        if self.is_log() {
            log_formula::from_log(logical, &self.log_formula)
        } else {
            logical
        }
    }

    pub fn price_to_coordinate(&self, price: f64, base_value: f64) -> Coordinate {
        let logical = match self.options.mode {
            PriceScaleMode::Percentage => log_formula::to_percent(price, base_value),
            PriceScaleMode::IndexedTo100 => log_formula::to_indexed_to_100(price, base_value),
            _ => price,
        };
        self.logical_to_coordinate(logical)
    }

    pub fn coordinate_to_price(&self, coordinate: f64, base_value: f64) -> f64 {
        let logical = self.coordinate_to_logical(coordinate);
        match self.options.mode {
            PriceScaleMode::Percentage => log_formula::from_percent(logical, base_value),
            PriceScaleMode::IndexedTo100 => log_formula::from_indexed_to_100(logical, base_value),
            // log handled inside coordinate_to_logical; normal is identity
            _ => logical,
        }
    }

    /// Batch conversion for OHLC bars — hot path. `base_value` is the series' first value.
    /// Writes 4 y-coordinates per bar.
    #[allow(clippy::too_many_arguments)]
    pub fn bar_prices_to_coordinates(
        &self,
        open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
        base_value: f64,
        out: &mut [[f64; 4]],
    ) {
        if self.is_empty() {
            return;
        }
        let bh = self.bottom_margin_px();
        let Some(range) = self.price_range.as_ref() else {
            return;
        };
        let min = range.min_value();
        let max = range.max_value();
        let ih = self.internal_height() - 1.0;
        let is_inverted = self.is_inverted();
        let hmm = ih / (max - min);
        let needs_transform = self.options.mode != PriceScaleMode::Normal;

        for i in 0..out.len() {
            let mut prices = [open[i], high[i], low[i], close[i]];
            if needs_transform {
                for p in &mut prices {
                    *p = self.price_to_logical(*p, base_value);
                }
            }
            for (j, p) in prices.iter().enumerate() {
                let inv_coordinate = bh + hmm * (p - min);
                out[i][j] = if is_inverted {
                    inv_coordinate
                } else {
                    self.height - 1.0 - inv_coordinate
                };
            }
        }
    }

    // --- axis-drag scale (RENDERING_SPEC.md §1.2) ---

    pub fn start_scale(&mut self, x: f64) {
        if self.is_percentage() || self.is_indexed_to_100() {
            return;
        }
        if self.scale_start_point.is_some() || self.price_range_snapshot.is_some() {
            return;
        }
        if self.is_empty() {
            return;
        }

        // invert x
        self.scale_start_point = Some(self.height - x);
        self.price_range_snapshot = self.price_range;
    }

    pub fn scale_to(&mut self, x: f64) {
        if self.is_percentage() || self.is_indexed_to_100() {
            return;
        }
        let Some(scale_start_point) = self.scale_start_point else {
            return;
        };

        self.options.auto_scale = false;

        let x = (self.height - x).max(0.0);

        let mut scale_coeff =
            (scale_start_point + (self.height - 1.0) * 0.2) / (x + (self.height - 1.0) * 0.2);
        // Set together with `scale_start_point` in `start_scale`; bail if the pair desynced.
        let Some(mut new_price_range) = self.price_range_snapshot else {
            return;
        };

        scale_coeff = scale_coeff.max(0.1);
        new_price_range.scale_around_center(scale_coeff);
        self.price_range = Some(new_price_range);
    }

    pub fn end_scale(&mut self) {
        if self.is_percentage() || self.is_indexed_to_100() {
            return;
        }
        self.scale_start_point = None;
        self.price_range_snapshot = None;
    }

    // --- wheel zoom (TradingView-style; the reference has no price-axis wheel) ---

    /// Zoom the range by `factor` anchored at the price under pane-local coordinate `y`
    /// (that price stays fixed on screen). Mirrors the drag-to-scale guards: percentage and
    /// indexed-to-100 modes no-op, and an empty scale has nothing to zoom. Disables autoscale,
    /// like any manual range edit.
    pub fn zoom(&mut self, y: f64, factor: f64) {
        if self.is_percentage() || self.is_indexed_to_100() {
            return;
        }
        if self.is_empty() {
            return;
        }
        let Some(range) = self.price_range else {
            return;
        };
        // base_value is only read by the percentage/indexed modes, which bail above.
        let anchor = self.coordinate_to_price(y, 0.0);
        self.options.auto_scale = false;
        let mut new_range = range;
        new_range.scale_around_point(anchor, factor);
        self.price_range = Some(new_range);
    }

    // --- axis-drag scroll ---

    pub fn start_scroll(&mut self, x: f64) {
        if self.is_auto_scale() {
            return;
        }
        if self.scroll_start_point.is_some() || self.price_range_snapshot.is_some() {
            return;
        }
        if self.is_empty() {
            return;
        }
        self.scroll_start_point = Some(x);
        self.price_range_snapshot = self.price_range;
    }

    pub fn scroll_to(&mut self, x: f64) {
        if self.is_auto_scale() {
            return;
        }
        let Some(scroll_start_point) = self.scroll_start_point else {
            return;
        };

        // Both are set alongside `scroll_start_point` in `start_scroll`; bail if desynced.
        let (Some(price_range), Some(mut new_price_range)) =
            (self.price_range, self.price_range_snapshot)
        else {
            return;
        };
        let price_units_per_pixel = price_range.length() / (self.internal_height() - 1.0);
        let mut pixel_delta = x - scroll_start_point;
        if self.is_inverted() {
            pixel_delta = -pixel_delta;
        }

        let price_delta = pixel_delta * price_units_per_pixel;
        new_price_range.shift(price_delta);
        self.price_range = Some(new_price_range);
    }

    pub fn end_scroll(&mut self) {
        if self.is_auto_scale() {
            return;
        }
        if self.scroll_start_point.is_none() {
            return;
        }
        self.scroll_start_point = None;
        self.price_range_snapshot = None;
    }

    // --- autoscale range application (tail of `_recalculatePriceRangeImpl`) ---

    /// Applies a merged source range (already in logical space for the current mode).
    /// `min_move` = 1/base of the formatter source (e.g. 0.01).
    pub fn apply_autoscale_range(&mut self, merged: Option<PriceRange>, min_move: f64) {
        if min_move.is_finite() && min_move > 0.0 {
            self.min_move = min_move;
        }
        let Some(mut price_range) = merged else {
            // reset empty to default
            if self.price_range.is_none() {
                self.price_range = Some(PriceRange::new(-0.5, 0.5));
                self.log_formula = log_formula::log_formula_for_price_range(None);
            }
            return;
        };

        if price_range.min_value() == price_range.max_value() {
            // degenerate range: extend by 5 min-move values on each side (in raw space)
            let extend_value = 5.0 * min_move;
            if self.is_log() {
                price_range =
                    log_formula::convert_price_range_from_log(&price_range, &self.log_formula);
            }
            price_range = PriceRange::new(
                price_range.min_value() - extend_value,
                price_range.max_value() + extend_value,
            );
            if self.is_log() {
                price_range =
                    log_formula::convert_price_range_to_log(&price_range, &self.log_formula);
            }
        }

        if self.is_log() {
            let raw_range =
                log_formula::convert_price_range_from_log(&price_range, &self.log_formula);
            let new_formula = log_formula::log_formula_for_price_range(Some(&raw_range));
            if !log_formula::log_formulas_are_same(&new_formula, &self.log_formula) {
                let raw_snapshot = self
                    .price_range_snapshot
                    .map(|s| log_formula::convert_price_range_from_log(&s, &self.log_formula));
                self.log_formula = new_formula;
                price_range = log_formula::convert_price_range_to_log(&raw_range, &new_formula);
                if let Some(raw) = raw_snapshot {
                    self.price_range_snapshot =
                        Some(log_formula::convert_price_range_to_log(&raw, &new_formula));
                }
            }
        }

        self.price_range = Some(price_range);
    }

    // --- tick marks (port of PriceTickMarkBuilder, RENDERING_SPEC.md §9) ---

    fn tick_mark_height(&self) -> f64 {
        (self.options.font_size * self.options.tick_mark_density).ceil()
    }

    /// Generates tick marks for the current range. `base` is the formatter base (e.g. 100).
    /// `entire_text_only_margin` should be `font_size / 2` when the entireTextOnly option is on,
    /// else 0. Edge tick marks (ensureEdgeTickMarksVisible) are not implemented yet.
    pub fn build_tick_marks(&self, base: i64, entire_text_only_margin: f64) -> Vec<PriceMark> {
        let mut marks = Vec::new();

        if self.is_empty() {
            return marks;
        }

        let scale_height = self.height;
        let bottom = self.coordinate_to_logical_raw(scale_height - 1.0);
        let top = self.coordinate_to_logical_raw(0.0);

        let min_coord = entire_text_only_margin;
        let max_coord = scale_height - 1.0 - entire_text_only_margin;

        let high = bottom.max(top);
        let low = bottom.min(top);
        if high == low {
            return marks;
        }

        let mut span = composite_tick_span(high, low, base, scale_height, self.tick_mark_height());
        let mut modulo = high % span;
        if modulo < 0.0 {
            modulo += span;
        }

        let sign = if high >= low { 1.0 } else { -1.0 };
        let mut prev_coord: Option<f64> = None;

        let mut logical = high - modulo;
        while logical > low {
            let coord = self.logical_to_coordinate_raw(logical);

            // skip marks that don't fit (required for log scale)
            let fits =
                prev_coord.is_none_or(|prev| (coord - prev).abs() >= self.tick_mark_height());
            let visible = coord >= min_coord && coord <= max_coord;

            if fits && visible {
                marks.push(PriceMark { coord, logical });
                prev_coord = Some(coord);
                if self.is_log() {
                    span = composite_tick_span(
                        logical * sign,
                        low,
                        base,
                        scale_height,
                        self.tick_mark_height(),
                    );
                }
            }

            logical -= span;
        }

        marks
    }

    /// coordinate -> logical *without* undoing the log transform (the tick mark builder works in
    /// the transformed space; matches the closures reference passes to `PriceTickMarkBuilder`).
    fn coordinate_to_logical_raw(&self, coordinate: f64) -> f64 {
        if self.is_empty() {
            return 0.0;
        }
        let inv_coordinate = self.inverted_coordinate(coordinate);
        let Some(range) = self.price_range.as_ref() else {
            return 0.0;
        };
        // the reference's builder converters go through _coordinateToLogical which applies fromLog;
        // rebuildTickMarks then walks in *price* space for log scales.
        let logical = range.min_value()
            + range.length()
                * ((inv_coordinate - self.bottom_margin_px()) / (self.internal_height() - 1.0));
        if self.is_log() {
            log_formula::from_log(logical, &self.log_formula)
        } else {
            logical
        }
    }

    fn logical_to_coordinate_raw(&self, logical: f64) -> f64 {
        self.logical_to_coordinate(logical)
    }

    /// Convenience: format-facing conversion used by tests and axis code.
    pub fn logical_to_price_pub(&self, logical: f64, base_value: f64) -> f64 {
        self.logical_to_price(logical, base_value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scale_with_range(height: f64, min: f64, max: f64) -> PriceScaleCore {
        let mut s = PriceScaleCore::new(PriceScaleCoreOptions::default());
        s.set_height(height);
        s.set_price_range(Some(PriceRange::new(min, max)));
        s
    }

    #[test]
    fn price_to_coordinate_with_margins() {
        // height 100, margins 20/10 -> internalHeight 70
        let s = scale_with_range(100.0, 0.0, 10.0);
        // price 10 (top of range): inv = 10 + 69*1 = 79 -> y = 100-1-79 = 20 (= top margin)
        assert_eq!(s.price_to_coordinate(10.0, 10.0), 20.0);
        // price 0 (bottom): inv = 10 -> y = 89
        assert_eq!(s.price_to_coordinate(0.0, 10.0), 89.0);
    }

    #[test]
    fn coordinate_roundtrip() {
        let s = scale_with_range(300.0, 12.5, 87.5);
        for &p in &[12.5, 20.0, 55.5, 87.5] {
            let y = s.price_to_coordinate(p, p);
            let back = s.coordinate_to_price(y, p);
            assert!((back - p).abs() < 1e-9, "p={p} y={y} back={back}");
        }
    }

    #[test]
    fn wheel_zoom_keeps_the_cursor_price_fixed() {
        let mut s = scale_with_range(300.0, 12.5, 87.5);
        let anchor_price = 55.5;
        let anchor_y = s.price_to_coordinate(anchor_price, 0.0);
        // Zoom in (factor < 1 shrinks the range) and out again: the anchored price must stay
        // at the same coordinate, and autoscale drops like any manual range edit.
        s.zoom(anchor_y, 0.9);
        assert!(!s.options.auto_scale);
        let range_in = s.price_range().unwrap();
        assert!((range_in.length() - 75.0 * 0.9).abs() < 1e-9);
        assert!((s.coordinate_to_price(anchor_y, 0.0) - anchor_price).abs() < 1e-9);
        s.zoom(anchor_y, 1.0 / 0.9);
        let range_back = s.price_range().unwrap();
        assert!((range_back.length() - 75.0).abs() < 1e-8);
        assert!((range_back.min_value() - 12.5).abs() < 1e-8);
        assert!((s.coordinate_to_price(anchor_y, 0.0) - anchor_price).abs() < 1e-9);
    }

    #[test]
    fn wheel_zoom_no_ops_like_drag_to_scale() {
        // Percentage mode: no zoom (mirrors the drag-to-scale guard).
        let mut s = scale_with_range(300.0, 12.5, 87.5);
        s.options.mode = PriceScaleMode::Percentage;
        s.zoom(100.0, 0.9);
        assert!((s.price_range().unwrap().length() - 75.0).abs() < 1e-9);
        assert!(s.options.auto_scale);
    }

    #[test]
    fn inverted_scale_flips() {
        let mut s = scale_with_range(100.0, 0.0, 10.0);
        s.options.invert_scale = true;
        // inverted: margins swap and inv coordinate is used directly (y grows downward)
        // bottom_margin_px = scale_margins.top * h = 20; internal = 70
        // price 10 (max): inv = 20 + 69 = 89 -> highest price near the bottom
        assert_eq!(s.price_to_coordinate(10.0, 10.0), 89.0);
        assert_eq!(s.price_to_coordinate(0.0, 10.0), 20.0);
    }

    #[test]
    fn percentage_mode() {
        let mut s = scale_with_range(100.0, 0.0, 10.0); // range is in percent space
        s.options.mode = PriceScaleMode::Percentage;
        let base = 100.0;
        // price 110 -> +10% -> top of range
        assert_eq!(s.price_to_coordinate(110.0, base), 20.0);
        let p = s.coordinate_to_price(20.0, base);
        assert!((p - 110.0).abs() < 1e-9);
    }

    #[test]
    fn log_mode_roundtrip() {
        let mut s = PriceScaleCore::new(PriceScaleCoreOptions {
            mode: PriceScaleMode::Logarithmic,
            ..Default::default()
        });
        s.set_height(400.0);
        // store range in log space, as the model does
        let raw = PriceRange::new(1.0, 1000.0);
        let log_range = log_formula::convert_price_range_to_log(&raw, s.log_formula());
        s.set_price_range(Some(log_range));

        for &p in &[1.0, 10.0, 100.0, 999.0] {
            let y = s.price_to_coordinate(p, p);
            let back = s.coordinate_to_price(y, p);
            assert!((back - p).abs() < 1e-6, "p={p} back={back}");
        }
    }

    #[test]
    fn axis_drag_scale_matches_formula() {
        let mut s = scale_with_range(200.0, 0.0, 100.0);
        s.set_auto_scale(false);
        s.start_scale(150.0); // start point (inverted): 50
        s.scale_to(100.0); // x' = 100
                           // coeff = (50 + 199*0.2) / (100 + 199*0.2) = 89.8 / 139.8
        let coeff: f64 = 89.8 / 139.8;
        let r = s.price_range().unwrap();
        let expected_half = 50.0 * coeff;
        assert!((r.min_value() - (50.0 - expected_half)).abs() < 1e-9);
        assert!((r.max_value() - (50.0 + expected_half)).abs() < 1e-9);
        s.end_scale();
    }

    #[test]
    fn scroll_shifts_range_by_pixels() {
        let mut s = scale_with_range(100.0, 0.0, 69.0); // internalHeight-1 = 69 -> 1 price/px
        s.set_auto_scale(false);
        s.start_scroll(50.0);
        s.scroll_to(60.0); // +10 px down -> +10 price
        let r = s.price_range().unwrap();
        assert!((r.min_value() - 10.0).abs() < 1e-9);
        assert!((r.max_value() - 79.0).abs() < 1e-9);
    }

    #[test]
    fn degenerate_autoscale_range_expanded() {
        let mut s = PriceScaleCore::new(PriceScaleCoreOptions::default());
        s.set_height(100.0);
        s.apply_autoscale_range(Some(PriceRange::new(50.0, 50.0)), 0.01);
        let r = s.price_range().unwrap();
        assert!((r.min_value() - 49.95).abs() < 1e-12);
        assert!((r.max_value() - 50.05).abs() < 1e-12);
    }

    #[test]
    fn empty_autoscale_gets_default_range() {
        let mut s = PriceScaleCore::new(PriceScaleCoreOptions::default());
        s.set_height(100.0);
        s.apply_autoscale_range(None, 0.01);
        assert_eq!(s.price_range().unwrap(), &PriceRange::new(-0.5, 0.5));
    }

    #[test]
    fn tick_marks_are_spaced_and_round() {
        let mut s = scale_with_range(300.0, 0.0, 100.0);
        s.set_height(300.0);
        let marks = s.build_tick_marks(100, 0.0);
        assert!(!marks.is_empty());
        // marks must be at multiples of the span -> logical values divide evenly
        let span = (marks[0].logical - marks[1].logical).abs();
        for w in marks.windows(2) {
            assert!(((w[0].logical - w[1].logical).abs() - span).abs() < 1e-9);
        }
        // spacing respects tick mark height (30px for font 12, density 2.5)
        for w in marks.windows(2) {
            assert!((w[1].coord - w[0].coord).abs() >= 30.0 - 1e-9);
        }
        // coordinates within scale
        for m in &marks {
            assert!(m.coord >= 0.0 && m.coord <= 299.0);
        }
    }

    #[test]
    fn scroll_requires_manual_scale_mode() {
        let mut s = scale_with_range(100.0, 0.0, 10.0);
        assert!(s.is_auto_scale());
        s.start_scroll(10.0);
        s.scroll_to(20.0);
        // autoscale on -> no-op
        assert_eq!(s.price_range().unwrap(), &PriceRange::new(0.0, 10.0));
    }

    #[test]
    fn clamp_helper_sanity() {
        use crate::helpers::mathex::clamp;
        assert_eq!(clamp(5.0, 0.0, 10.0), 5.0);
        assert_eq!(clamp(-5.0, 0.0, 10.0), 0.0);
        assert_eq!(clamp(50.0, 0.0, 10.0), 10.0);
    }
}
