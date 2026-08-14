//! `ChartInner` model/state API: series, panes, scales, coordinates, ranges. These are the
//! working halves of the thin `#[wasm_bindgen] impl NucleusChart` delegations in `chart.rs`.

use super::inner_render::measure_text_ctx;
use super::*;

impl ChartInner {
    pub(super) fn dispose_extensions(&mut self) {
        let pane_ids = self
            .primitives
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        for id in pane_ids {
            self.detach_pane_primitive(id);
        }
        let series_ids = self
            .series_primitives
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        for id in series_ids {
            self.detach_series_primitive(id);
        }
        self.destroy_all_custom_series();
        self.rings.clear();
        self.primitive_texts.clear();
    }

    /// Adds a series and returns its id. `kind`: 0 candles, 1 bars, 2 line, 3 area, 4 histogram.
    pub fn add_series(&mut self, kind: u8) -> u32 {
        self.engine.add_series(SeriesKind::from_u8(kind))
    }

    /// Remove a series and any indicators derived from it. Returns true if a live, non-primary
    /// series was removed. The primary series (id 0) cannot be removed.
    pub fn remove_series(&mut self, id: u32) -> bool {
        let removed = self.engine.remove_series(id as SeriesId);
        if removed {
            // Series primitives bound to the removed series (or to indicator outputs
            // tombstoned with it) auto-detach (reference `removeSeries` drops them with the series).
            self.detach_orphaned_series_primitives();
            // Custom series drop their entry with the series, firing the pane view's
            // `destroy` hook (reference `ICustomSeriesPaneView.destroy`).
            self.drop_orphaned_custom_series();
        }
        removed
    }

    /// `remove_series` that reports every tombstoned id (the series plus its derived indicator
    /// outputs) so the host can fire one removal event per series; empty = nothing removed.
    pub fn remove_series_tracked(&mut self, id: u32) -> Vec<u32> {
        let dropped = self.engine.remove_series_tracked(id as SeriesId);
        if !dropped.is_empty() {
            self.detach_orphaned_series_primitives();
            self.drop_orphaned_custom_series();
            // A removed series must not keep the engine holding views over its ring's shared
            // buffer, or a `remove_series` would leak the whole buffer for the chart's lifetime.
            let removed: Vec<SeriesId> = dropped.iter().map(|id| *id as SeriesId).collect();
            self.rings.retain(|r| !removed.contains(&r.series_id));
        }
        dropped
    }

    /// JSON [`IndicatorInfo`] for an indicator output series, or `null` for a plain/source
    /// series — the lineage a platform needs to render its own indicator chips.
    pub fn series_indicator_info_json(&self, id: u32) -> String {
        match self.engine.indicator_info(id as SeriesId) {
            Some(info) => serde_json::to_string(&info).unwrap_or_else(|_| "null".to_string()),
            None => "null".to_string(),
        }
    }

    pub fn add_sma(&mut self, source_id: u32, period: u32) -> u32 {
        self.engine
            .add_sma(source_id as SeriesId, period as usize)
            .unwrap_or(u32::MAX)
    }

    pub fn add_ema(&mut self, source_id: u32, period: u32) -> u32 {
        self.engine
            .add_ema(source_id as SeriesId, period as usize)
            .unwrap_or(u32::MAX)
    }

    pub fn add_bollinger(&mut self, source_id: u32, period: u32, deviation: f64) -> Vec<u32> {
        self.engine
            .add_bollinger(source_id as SeriesId, period as usize, deviation)
    }

    pub fn add_rsi(&mut self, source_id: u32, period: u32) -> u32 {
        self.engine
            .add_rsi(source_id as SeriesId, period as usize)
            .unwrap_or(u32::MAX)
    }

    pub fn add_macd(&mut self, source_id: u32, fast: u32, slow: u32, signal: u32) -> Vec<u32> {
        self.engine.add_macd(
            source_id as SeriesId,
            fast as usize,
            slow as usize,
            signal as usize,
        )
    }

    pub fn add_stochastic(&mut self, source_id: u32, k_period: u32, d_period: u32) -> Vec<u32> {
        self.engine
            .add_stochastic(source_id as SeriesId, k_period as usize, d_period as usize)
    }

    pub fn add_atr(&mut self, source_id: u32, period: u32) -> u32 {
        self.engine
            .add_atr(source_id as SeriesId, period as usize)
            .unwrap_or(u32::MAX)
    }

    /// `volume_source`: series id supplying the per-bar volume column, or -1 for unit weights.
    pub fn add_vwap(&mut self, source_id: u32, volume_source: i32) -> u32 {
        let volume = (volume_source >= 0).then_some(volume_source as SeriesId);
        self.engine
            .add_vwap(source_id as SeriesId, volume)
            .unwrap_or(u32::MAX)
    }

    pub fn add_wma(&mut self, source_id: u32, period: u32) -> u32 {
        self.engine
            .add_wma(source_id as SeriesId, period as usize)
            .unwrap_or(u32::MAX)
    }

    /// Sets the main series' data (series 0). `times` are ascending UTC seconds.
    pub fn set_data(
        &mut self,
        times: &[f64],
        open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
    ) {
        self.set_series_data(0, times, open, high, low, close);
    }

    /// Sets a series' data by id.
    pub fn set_series_data(
        &mut self,
        id: u32,
        times: &[f64],
        open: &[f64],
        high: &[f64],
        low: &[f64],
        close: &[f64],
    ) {
        // Repair messy feed data (out-of-order, duplicate times, NaN/Inf, length mismatch) at the
        // boundary so the DataLayer's ascending-unique-finite contract always holds — a malformed
        // feed yields a warning and a rendered chart, never a wasm panic (roadmap Phase A3).
        let s = match sanitize_ohlc(times, open, high, low, close) {
            Ok(s) => s,
            Err(e) => {
                web_sys::console::warn_1(
                    &format!("nucleuscharts: set_series_data rejected — {e}").into(),
                );
                return;
            }
        };
        if !s.report.is_clean() {
            web_sys::console::warn_1(
                &format!(
                    "nucleuscharts: set_series_data sanitized data — accepted {}, dropped {} invalid, {} duplicate{}",
                    s.report.accepted,
                    s.report.dropped_invalid,
                    s.report.dropped_duplicate,
                    if s.report.reordered { ", reordered" } else { "" },
                )
                .into(),
            );
        }
        self.engine
            .install_series_data(id as SeriesId, s.times, s.open, s.high, s.low, s.close);
    }

    pub fn set_series_data_typed(
        &mut self,
        id: u32,
        times: &Float64Array,
        open: &Float64Array,
        high: &Float64Array,
        low: &Float64Array,
        close: &Float64Array,
    ) -> Option<String> {
        if let Err(error) = self.engine.validate_series_id(id as SeriesId) {
            return Some(rejected_diagnostics_json(format_args!("{error:?}")));
        }
        let s = match nucleuscharts_core::model::data_validation::sanitize_ohlc_owned(
            times.to_vec(),
            open.to_vec(),
            high.to_vec(),
            low.to_vec(),
            close.to_vec(),
        ) {
            Ok(s) => s,
            Err(e) => {
                web_sys::console::warn_1(
                    &format!("nucleuscharts: set_series_data rejected — {e}").into(),
                );
                return Some(rejected_diagnostics_json(e));
            }
        };
        if !s.report.is_clean() {
            web_sys::console::warn_1(&format!("nucleuscharts: set_series_data sanitized data — accepted {}, dropped {} invalid, {} duplicate{}", s.report.accepted, s.report.dropped_invalid, s.report.dropped_duplicate, if s.report.reordered { ", reordered" } else { "" }).into());
        }
        let diagnostics = validation_diagnostics_json(&s.report);
        self.engine
            .install_series_data(id as SeriesId, s.times, s.open, s.high, s.low, s.close);
        diagnostics
    }

    /// Columnar streaming append (consumer Item 2): a batch of points in `set_series_data_typed`'s
    /// column layout, applied through the same per-row engine call the single-point `update` path
    /// uses — so a one-row batch is semantically identical to `update_series_bar`, and a longer one
    /// is exactly that repeated.
    ///
    /// The batch is first run through the shared repair pipeline (drop non-finite → stable sort by
    /// time → collapse duplicate times last-wins), matching `set_series_data_typed` rather than
    /// inventing a second policy. All-NaN rows survive as whitespace, as they do for `update`.
    ///
    /// For the streaming shape — rows at or past the chart's last timestamp — cost is linear in
    /// the batch and independent of series length: each row hits `sync_time_points`' single-append
    /// fast path and `DataLayer::update_styled`'s O(1) case, so a 500-row batch on a 1M-bar series
    /// does 500 O(1) appends. A row that lands *before* the chart's last timestamp is a
    /// mid-history insert and pays `rebuild_merged` + `reindex_all`, exactly as the same row does
    /// through the single-point `update` — the batch does not coalesce those into one reindex.
    /// Rewriting history belongs in `set_series_data_typed`.
    pub fn update_series_bars_typed(
        &mut self,
        id: u32,
        times: &Float64Array,
        open: &Float64Array,
        high: &Float64Array,
        low: &Float64Array,
        close: &Float64Array,
    ) -> Option<String> {
        // Ignore updates to an unknown series rather than corrupting the data layer (same guard
        // as the single-point path).
        if !self.series.iter().any(|s| s.id == id as SeriesId) {
            web_sys::console::warn_1(&"nucleuscharts: update_typed for unknown series id".into());
            return Some(rejected_diagnostics_json("unknown or stale series id"));
        }
        let s = match nucleuscharts_core::model::data_validation::sanitize_ohlc_owned(
            times.to_vec(),
            open.to_vec(),
            high.to_vec(),
            low.to_vec(),
            close.to_vec(),
        ) {
            Ok(s) => s,
            Err(e) => {
                web_sys::console::warn_1(
                    &format!("nucleuscharts: update_typed rejected — {e}").into(),
                );
                return Some(rejected_diagnostics_json(e));
            }
        };
        if !s.report.is_clean() {
            web_sys::console::warn_1(&format!("nucleuscharts: update_typed sanitized batch — accepted {}, dropped {} invalid, {} duplicate{}", s.report.accepted, s.report.dropped_invalid, s.report.dropped_duplicate, if s.report.reordered { ", reordered" } else { "" }).into());
        }
        let diagnostics = validation_diagnostics_json(&s.report);
        // `sanitize_ohlc_owned` yields integer-second times; the engine's update entry point takes
        // the f64 JS form, so hand them back in that shape and let it re-narrow.
        for row in 0..s.times.len() {
            self.engine.update_series_bar(
                id as SeriesId,
                s.times[row] as f64,
                [s.open[row], s.high[row], s.low[row], s.close[row]],
            );
        }
        diagnostics
    }

    /// Bind a ring source to a series (consumer Item 3), replacing any ring already bound to it.
    /// Returns `""` on success, else the reason the layout was rejected.
    pub fn set_ring_source(
        &mut self,
        series_id: u32,
        bytes: js_sys::Uint8Array,
        cursor_view: js_sys::Int32Array,
        layout_json: &str,
    ) -> String {
        if !self.series.iter().any(|s| s.id == series_id as SeriesId) {
            return "unknown or removed series id".into();
        }
        let layout: super::RingLayoutInput = match serde_json::from_str(layout_json) {
            Ok(layout) => layout,
            Err(e) => return format!("malformed ring_source_layout: {e}"),
        };
        match super::BoundRing::new(series_id as SeriesId, bytes, cursor_view, &layout) {
            Ok(ring) => {
                self.clear_ring_source(series_id);
                self.rings.push(ring);
                String::new()
            }
            Err(reason) => reason,
        }
    }

    /// Unbind a series' ring, dropping the engine's views over the shared buffer so the buffer can
    /// be collected once the host releases it too.
    pub fn clear_ring_source(&mut self, series_id: u32) {
        self.rings.retain(|r| r.series_id != series_id as SeriesId);
    }

    /// Drain every bound ring for this frame. See the wasm-facing wrapper for the `out` layout.
    pub fn drain_ring_sources(&mut self, out: &mut [f64]) -> u32 {
        let clock = self.clock.clone();
        let started = clock.as_ref().map(|clock| clock.now());
        // Destructure once so each ring's `&mut` and the engine's `&mut` are disjoint borrows.
        let Self {
            rings,
            engine,
            telemetry,
            ..
        } = self;
        let mut total = 0u32;
        let mut had_work = false;
        let mut pairs = 0usize;
        let mut lost = 0u32;
        for ring in rings.iter_mut() {
            let series_id = ring.series_id;
            let outcome = ring.drain(engine);
            had_work |= outcome.had_work;
            lost += outcome.lost_rows;
            if outcome.rows == 0 {
                continue;
            }
            total += outcome.rows;
            // `[pair_count, id, rows, ...]` — stop reporting if the caller's scratch is short, but
            // keep draining so no ring stalls behind a mis-sized buffer.
            if (pairs + 1) * 2 < out.len() {
                out[1 + pairs * 2] = series_id as f64;
                out[2 + pairs * 2] = f64::from(outcome.rows);
                pairs += 1;
            }
        }
        if !out.is_empty() {
            out[0] = pairs as f64;
        }
        if lost > 0 {
            // Surfaced on `frame_stats().ring_overruns` rather than as a console warning: an
            // overrun under load would otherwise flood the console at frame rate.
            telemetry.count_ring_overruns(lost);
        }
        if had_work {
            if let (Some(clock), Some(start)) = (clock.as_ref(), started) {
                telemetry.add_pending_ingest_ms(clock.now() - start);
            }
        }
        total
    }

    /// Streaming update of the main series (append new time or replace last).
    pub fn update_bar(&mut self, time: f64, open: f64, high: f64, low: f64, close: f64) {
        self.update_series_bar(0, time, open, high, low, close);
    }

    /// Streaming update of the series with `series_id` (append a new time or replace the last).
    pub fn update_series_bar(
        &mut self,
        series_id: u32,
        time: f64,
        open: f64,
        high: f64,
        low: f64,
        close: f64,
    ) {
        // Ignore updates to an unknown series rather than corrupting the data layer.
        if !self.series.iter().any(|s| s.id == series_id as SeriesId) {
            web_sys::console::warn_1(&"nucleuscharts: update_bar for unknown series id".into());
            return;
        }
        // Drop a bad tick rather than corrupting the series (roadmap Phase A3).
        if !self
            .engine
            .update_series_bar(series_id as SeriesId, time, [open, high, low, close])
        {
            web_sys::console::warn_1(
                &"nucleuscharts: update_bar dropped a non-finite point".into(),
            );
        }
    }

    /// Per-data-point color overrides (reference data-item colors; packed RGBA `0xRRGGBBAA`, 0 = no
    /// override at that row). Delegates to the engine; a rejection (unknown/removed id or a
    /// channel length that does not match the row count) warns and leaves no partial state.
    pub fn set_series_point_colors(
        &mut self,
        id: u32,
        body: Option<Vec<u32>>,
        wick: Option<Vec<u32>>,
        border: Option<Vec<u32>>,
    ) {
        if !self
            .engine
            .set_series_point_colors(id as SeriesId, body, wick, border)
        {
            web_sys::console::warn_1(
                &"nucleuscharts: set_series_point_colors rejected (unknown id or channel length != row count)".into(),
            );
        }
    }

    /// Streaming update like [`update_series_bar`] that also sets the target bar's per-point
    /// color channels (None = no custom color for that channel).
    #[allow(clippy::too_many_arguments)] // mirrors update_series_bar plus the three reference color slots
    pub fn update_series_bar_styled(
        &mut self,
        series_id: u32,
        time: f64,
        open: f64,
        high: f64,
        low: f64,
        close: f64,
        body: Option<u32>,
        wick: Option<u32>,
        border: Option<u32>,
    ) {
        // Ignore updates to an unknown series rather than corrupting the data layer.
        if !self.series.iter().any(|s| s.id == series_id as SeriesId) {
            web_sys::console::warn_1(
                &"nucleuscharts: update_series_bar_styled for unknown series id".into(),
            );
            return;
        }
        if !self.engine.update_series_bar_styled(
            series_id as SeriesId,
            time,
            [open, high, low, close],
            [body, wick, border],
        ) {
            web_sys::console::warn_1(
                &"nucleuscharts: update_series_bar_styled dropped a non-finite point".into(),
            );
        }
    }

    /// Apply a per-series `priceFormat` JSON patch (reference PriceFormat). Malformed JSON, an
    /// unknown type, or an unknown/removed id warns and is ignored.
    pub fn series_apply_price_format_json(&mut self, id: u32, json: &str) {
        if !self
            .engine
            .series_apply_price_format_json(id as SeriesId, json)
        {
            web_sys::console::warn_1(
                &"nucleuscharts: series_apply_price_format_json ignored (unknown id, type, or malformed JSON)".into(),
            );
        }
    }

    /// Install a series' custom price formatter fn (reference `priceFormat.formatter`), switching it
    /// to `type:"custom"`. Same boundary contract as the chart-level price formatter: a throw
    /// or non-string result falls back to the built-in formatter.
    pub fn set_series_price_formatter(&mut self, id: u32, formatter: js_sys::Function) {
        let installed = self.engine.set_series_price_formatter(
            id as SeriesId,
            Box::new(move |price: f64| {
                formatter
                    .call1(&JsValue::NULL, &JsValue::from_f64(price))
                    .ok()
                    .and_then(|v| v.as_string())
            }) as PriceFormatterFn,
        );
        if !installed {
            web_sys::console::warn_1(
                &"nucleuscharts: set_series_price_formatter ignored (unknown series id)".into(),
            );
        }
    }

    /// Sets a series' line/area color (overrides the kind default). The numeric r/g/b form
    /// stores the computed CSS string so `series_options_json` round-trips it exactly.
    pub fn set_series_color(&mut self, id: u32, r: u8, g: u8, b: u8) {
        if let Some(s) = self.series.iter_mut().find(|s| s.id == id as SeriesId) {
            s.line_color = Some(Color::rgb(r, g, b).to_css());
        }
    }

    /// Sets a series' line/area/histogram stroke color from a CSS string, preserving alpha
    /// (the r/g/b `set_series_color` form is opaque-only). Stored verbatim (reference `options()`
    /// returns the applied string); parsed at render time.
    pub fn set_series_color_css(&mut self, id: u32, css: &str) {
        if let Some(s) = self.series.iter_mut().find(|s| s.id == id as SeriesId) {
            s.line_color = Some(css.to_string());
        }
    }

    pub fn set_series_visible(&mut self, id: u32, visible: bool) {
        self.engine.set_series_visible(id as SeriesId, visible);
    }

    /// Retention ceiling for a series: at most `max_points` rows, oldest evicted first. A
    /// non-positive or non-finite value clears the cap (unbounded, the default).
    pub fn set_series_max_points(&mut self, id: u32, max_points: Option<f64>) {
        let cap = max_points
            .filter(|n| n.is_finite() && *n >= 1.0)
            .map(|n| n as usize);
        if !self.engine.set_series_max_points(id as SeriesId, cap) {
            web_sys::console::warn_1(
                &"nucleuscharts: set_series_max_points ignored (unknown or removed series id)"
                    .into(),
            );
        }
    }

    /// This series' retention ceiling, or `None` when unbounded.
    pub fn series_max_points(&self, id: u32) -> Option<f64> {
        self.engine
            .series_max_points(id as SeriesId)
            .map(|n| n as f64)
    }

    /// Set candlestick/bar up & down body colors, stored verbatim (reference `options()` returns
    /// the applied string). `""` clears the override back to the reference default palette; the
    /// strings are parsed at render time.
    pub fn set_series_updown_colors(&mut self, id: u32, up: &str, down: &str) {
        if let Some(s) = self.series.iter_mut().find(|s| s.id == id as SeriesId) {
            crate::color_policy::update_color_slot(&mut s.up_color, Some(up.to_string()));
            crate::color_policy::update_color_slot(&mut s.down_color, Some(down.to_string()));
        }
    }

    /// Set candlestick wick colors per direction. `undefined` = keep current, `""` = clear the
    /// override (follow the direction's body color), a CSS color = pin it verbatim.
    pub fn set_series_wick_colors(&mut self, id: u32, up: Option<String>, down: Option<String>) {
        if let Some(s) = self.series.iter_mut().find(|s| s.id == id as SeriesId) {
            crate::color_policy::update_color_slot(&mut s.wick_up_color, up);
            crate::color_policy::update_color_slot(&mut s.wick_down_color, down);
        }
    }

    /// Set candlestick border colors per direction; same keep/clear/pin contract as the wicks.
    pub fn set_series_border_colors(&mut self, id: u32, up: Option<String>, down: Option<String>) {
        if let Some(s) = self.series.iter_mut().find(|s| s.id == id as SeriesId) {
            crate::color_policy::update_color_slot(&mut s.border_up_color, up);
            crate::color_policy::update_color_slot(&mut s.border_down_color, down);
        }
    }

    /// Toggle candlestick wick visibility (default visible; bars ignore this).
    pub fn set_series_wick_visible(&mut self, id: u32, visible: bool) {
        if let Some(s) = self.series.iter_mut().find(|s| s.id == id as SeriesId) {
            s.wick_visible = Some(visible);
        }
    }

    /// Toggle candlestick body-border visibility (default visible; bars ignore this).
    pub fn set_series_border_visible(&mut self, id: u32, visible: bool) {
        if let Some(s) = self.series.iter_mut().find(|s| s.id == id as SeriesId) {
            s.border_visible = Some(visible);
        }
    }

    /// Set a line/area series' stroke width (css px; non-positive ignored).
    pub fn set_series_line_width(&mut self, id: u32, width: f64) {
        if width > 0.0 {
            if let Some(s) = self.series.iter_mut().find(|s| s.id == id as SeriesId) {
                s.line_width = Some(width);
            }
        }
    }

    /// Set an area series' fill gradient colors (top at the line, bottom at the base), stored
    /// verbatim like the other color slots (`""` clears back to the engine default; parsed at
    /// render time).
    pub fn set_series_area_colors(&mut self, id: u32, top: &str, bottom: &str) {
        if let Some(s) = self.series.iter_mut().find(|s| s.id == id as SeriesId) {
            crate::color_policy::update_color_slot(&mut s.area_top_color, Some(top.to_string()));
            crate::color_policy::update_color_slot(
                &mut s.area_bottom_color,
                Some(bottom.to_string()),
            );
        }
    }

    /// Color a histogram by the main price series' up/down direction per bar (TradingView volume).
    pub fn set_series_histogram_updown(&mut self, id: u32, enabled: bool) {
        if let Some(s) = self.series.iter_mut().find(|s| s.id == id as SeriesId) {
            s.histogram_updown = enabled;
        }
    }

    /// Set a line/area series' join type: 0 = simple, 1 = stepped, 2 = curved (roadmap Phase B3).
    pub fn set_series_line_type(&mut self, id: u32, line_type: u8) {
        let lt = match line_type {
            1 => LineType::WithSteps,
            2 => LineType::Curved,
            _ => LineType::Simple,
        };
        if let Some(s) = self.series.iter_mut().find(|s| s.id == id as SeriesId) {
            s.line_type = lt;
        }
    }

    /// Toggle per-point disc markers on a line/area series (roadmap Phase B3).
    pub fn set_series_point_markers(&mut self, id: u32, visible: bool) {
        if let Some(s) = self.series.iter_mut().find(|s| s.id == id as SeriesId) {
            s.point_markers = visible;
        }
    }

    /// Set a Baseline series' baseline price. `NaN` resets to auto (visible-range midpoint).
    pub fn set_series_baseline(&mut self, id: u32, price: f64) {
        if let Some(s) = self.series.iter_mut().find(|s| s.id == id as SeriesId) {
            s.baseline = if price.is_finite() { Some(price) } else { None };
        }
    }

    /// Add a horizontal price line to a series; returns its id (roadmap Phase B4).
    #[allow(clippy::too_many_arguments)]
    pub fn create_price_line(
        &mut self,
        series_id: u32,
        price: f64,
        r: u8,
        g: u8,
        b: u8,
        width: u32,
        style: u8,
        title: &str,
    ) -> u32 {
        self.engine.create_price_line(
            series_id as SeriesId,
            price,
            Color::rgb(r, g, b),
            width as i32,
            line_style_from_u8(style),
            title,
        )
    }

    /// Remove a price line by id (from whichever series holds it).
    pub fn remove_price_line(&mut self, id: u32) {
        self.engine.remove_price_line(id);
    }

    /// Merge a JSON options patch into the price line with `id` (reference `IPriceLine.applyOptions`).
    pub fn price_line_apply_options(&mut self, id: u32, json: &str) {
        if !self.engine.price_line_apply_options(id, json) {
            web_sys::console::warn_1(
                &"nucleuscharts: price_line_apply_options ignored (unknown id or malformed JSON)"
                    .into(),
            );
        }
    }

    /// The price line's full options as snake_case JSON ("" for an unknown id).
    pub fn price_line_options_json(&self, id: u32) -> String {
        self.engine.price_line_options_json(id).unwrap_or_default()
    }

    /// Replace a series' markers from a JSON array `[{time, position, shape, color, text}]`
    /// (position: above|below|inBar; shape: circle|square|arrowUp|arrowDown). Roadmap Phase B4.
    pub fn set_series_markers(&mut self, series_id: u32, json: &str) {
        let inputs: Vec<MarkerInput> = serde_json::from_str(json).unwrap_or_default();
        let markers: Vec<Marker> = inputs
            .into_iter()
            .map(|m| Marker {
                time: m.time as i64,
                position: match m.position.as_str() {
                    "below" | "belowBar" => marker_pos::BELOW,
                    "inBar" | "in" => marker_pos::IN_BAR,
                    _ => marker_pos::ABOVE,
                },
                shape: match m.shape.as_str() {
                    "square" => marker_shape::SQUARE,
                    "arrowUp" | "arrow_up" => marker_shape::ARROW_UP,
                    "arrowDown" | "arrow_down" => marker_shape::ARROW_DOWN,
                    _ => marker_shape::CIRCLE,
                },
                color: Color::parse_css(&m.color).unwrap_or(Color::rgb(0x21, 0x96, 0xf3)),
                text: m.text,
            })
            .collect();
        self.engine
            .set_series_markers(series_id as SeriesId, markers);
    }

    pub fn set_series_markers_auto_scale(&mut self, series_id: u32, enabled: bool) {
        self.engine
            .set_series_markers_auto_scale(series_id as SeriesId, enabled);
    }

    /// Toggle the pulsing last-price ring on a series (roadmap Phase B3).
    pub fn set_series_last_price_animation(&mut self, id: u32, enabled: bool) {
        if let Some(s) = self.series.iter_mut().find(|s| s.id == id as SeriesId) {
            s.last_price_animation = enabled;
        }
    }

    /// Whether any series wants the last-price pulse (so the host can start/stop its rAF loop).
    pub fn wants_animation(&self) -> bool {
        self.series
            .iter()
            .any(|s| !s.removed && s.last_price_animation)
    }

    /// Set the host animation clock (ms). The shell's rAF loop calls this then `render()`.
    pub fn set_animation_time(&mut self, t_ms: f64) {
        self.animation_time = t_ms;
    }

    /// Move a series onto its pane's bottom-band overlay scale (volume-style) and set that band's
    /// margins as fractions of the pane slot: `top` leaves that fraction above the band, `bottom`
    /// below it (e.g. top=0.8, bottom=0.0 ⇒ bottom 20%). Excludes the series from the pane's main
    /// autoscale (roadmap Phase B2).
    pub fn set_series_overlay(&mut self, id: u32, top: f64, bottom: f64) {
        let mut pane_index = 0;
        if let Some(s) = self.series.iter_mut().find(|s| s.id == id as SeriesId) {
            s.overlay = true;
            s.left_scale = false;
            pane_index = s.pane_index;
        }
        if let Some(p) = self.panes.get_mut(pane_index) {
            p.overlay_top = top.clamp(0.0, 1.0);
            p.overlay_bottom = bottom.clamp(0.0, 1.0);
            p.overlay_scale
                .set_scale_margins(p.overlay_top, p.overlay_bottom);
            p.refresh_internal_margins();
        }
    }

    /// Move a series into pane `pane_index`, creating panes (with the given stretch factor for a
    /// newly-created last pane) as needed. Pane 0 is the top/price pane (roadmap Phase B1).
    pub fn set_series_pane(&mut self, id: u32, pane_index: usize, stretch_factor: f64) {
        self.engine
            .set_series_pane(id as SeriesId, pane_index, stretch_factor);
    }

    /// Number of stacked panes.
    pub fn pane_count(&self) -> usize {
        self.panes.len()
    }

    pub fn pane_stable_id(&self, index: u32) -> Option<u32> {
        self.engine.pane_stable_id(index as usize)
    }

    pub fn pane_index_for_id(&self, stable_id: u32) -> Option<u32> {
        self.engine
            .pane_index_for_id(stable_id)
            .map(|index| index as u32)
    }

    /// CSS Y of each pane boundary (top edge of panes 1..n), for separator hit-testing by the host.
    /// Reflects the last layout pass.
    pub fn pane_separator_ys(&self) -> Vec<f64> {
        self.panes.iter().skip(1).map(|p| p.top).collect()
    }

    /// Drag the separator below pane `i` by `delta_css` (positive grows pane `i`, shrinks `i+1`),
    /// keeping both at least a minimum height. Shared engine policy freezes current heights as
    /// stretch factors so browser and native hosts resize panes identically.
    pub fn drag_pane_separator(&mut self, i: usize, delta_css: f64) {
        self.engine.drag_pane_separator(i, delta_css);
    }

    /// CSS height of pane `i` from the last layout pass.
    pub fn pane_height(&self, i: usize) -> f64 {
        self.panes.get(i).map(|p| p.height).unwrap_or(0.0)
    }

    /// JSON `{left, top, width, height}` of pane `i`'s content area in CSS px relative to the
    /// chart container's top-left (`{}` for a stale index). This is the anchor a platform
    /// absolutely-positions per-pane chrome against (e.g. a TradingView-style indicator chip
    /// at the pane's top-left). Reflects the last layout pass.
    pub fn pane_geometry_json(&self, i: usize) -> String {
        match self.panes.get(i) {
            Some(p) => format!(
                r#"{{"left":{},"top":{},"width":{},"height":{}}}"#,
                self.pane_left, p.top, self.pane_w, p.height
            ),
            None => "{}".to_string(),
        }
    }

    /// Relative stretch factor of pane `i`.
    pub fn pane_stretch(&self, i: usize) -> f64 {
        self.panes.get(i).map(|p| p.stretch_factor).unwrap_or(1.0)
    }

    /// Set pane `i`'s stretch factor (its share of the content height relative to the others).
    pub fn set_pane_stretch(&mut self, i: usize, factor: f64) {
        if let Some(p) = self.panes.get_mut(i) {
            p.stretch_factor = factor.max(0.01);
            if self.css_width > 0.0 {
                self.recompute_layout(false);
            }
        }
    }

    /// Resize pane `i` to `height_css`, absorbing the delta from its neighbour below (or above for
    /// the last pane) — the same freeze-and-redistribute behavior as dragging its separator.
    pub fn set_pane_height(&mut self, i: usize, height_css: f64) {
        if i >= self.panes.len() {
            return;
        }
        let current = self.panes[i].height;
        let delta = height_css - current;
        if i + 1 < self.panes.len() {
            self.drag_pane_separator(i, delta);
        } else if i > 0 {
            // last pane: move the separator above it the other way to grow/shrink it
            self.drag_pane_separator(i - 1, -delta);
        }
        if self.css_width > 0.0 {
            self.recompute_layout(false);
        }
    }

    /// reference v5 `chart.addPane(preserveEmptyPane)`: append a pane and return its index.
    pub fn add_pane(&mut self, preserve_empty: bool) -> u32 {
        self.engine.add_pane(preserve_empty) as u32
    }

    /// reference `chart.removePane`: refuses the last remaining pane and stale indices (false).
    /// The pane's series are NOT removed — they become pane-less (reference `paneForSource` →
    /// null) and render/scale nowhere until re-assigned; panes below shift one index up.
    pub fn remove_pane(&mut self, index: u32) -> bool {
        self.engine.remove_pane(index as usize)
    }

    /// reference `chart.swapPanes`: the two panes trade places — series assignments, stretch
    /// factors, scales, and preserve flags ride along with them.
    pub fn swap_panes(&mut self, first: u32, second: u32) -> bool {
        self.engine.swap_panes(first as usize, second as usize)
    }

    /// reference `IPaneApi.moveTo`: relocate the pane (with its series) to a new index; the panes
    /// in between shift one slot. False for a stale index.
    pub fn pane_move_to(&mut self, index: u32, target: u32) -> bool {
        self.engine.move_pane(index as usize, target as usize)
    }

    /// reference `IPaneApi.preserveEmptyPane` (false for a stale index).
    pub fn pane_preserve_empty(&self, index: u32) -> bool {
        self.engine.pane_preserve_empty(index as usize)
    }

    /// reference `IPaneApi.setPreserveEmptyPane`: an empty pane collapses on the next series
    /// removal/move-out unless this flag holds it open (chart-model.ts
    /// `_cleanupIfPaneIsEmpty`).
    pub fn pane_set_preserve_empty(&mut self, index: u32, flag: bool) {
        self.engine.pane_set_preserve_empty(index as usize, flag);
    }

    /// reference `IPaneApi.getSeries`: the pane's live series ids in render order (bottom first).
    pub fn pane_series_ids(&self, index: u32) -> Vec<u32> {
        self.engine.pane_series_ids(index as usize)
    }

    /// Attach a pane primitive (reference `IPaneApi.attachPrimitive`, plugin platform Phase C-a) and
    /// return its registry id (0 when the pane index is stale). Fires the plugin's `attached`
    /// hook with `{pane_index}` if present (reference `PaneAttachedParameter`, reduced to what the
    /// host can provide headlessly); a throwing hook detaches nothing and is only reported.
    pub fn attach_pane_primitive(&mut self, pane: u32, primitive: js_sys::Object) -> u32 {
        if pane as usize >= self.panes.len() {
            web_sys::console::warn_1(
                &format!("nucleuscharts: attach_pane_primitive ignored — stale pane index {pane}")
                    .into(),
            );
            return 0;
        }
        let id = self.next_primitive_id;
        self.next_primitive_id += 1;
        self.primitives.push(PanePrimitiveEntry {
            id,
            pane,
            obj: primitive.clone(),
        });
        if let Ok(hook) = js_sys::Reflect::get(&primitive, &"attached".into()) {
            if let Ok(hook) = hook.dyn_into::<js_sys::Function>() {
                let params = js_sys::Object::new();
                let _ = js_sys::Reflect::set(&params, &"pane_index".into(), &pane.into());
                if let Err(error) = hook.call1(&primitive, &params) {
                    web_sys::console::warn_1(
                        &format!("nucleuscharts: pane primitive `attached` hook threw — {error:?}")
                            .into(),
                    );
                }
            }
        }
        id
    }

    /// Detach a pane primitive by id (reference `IPaneApi.detachPrimitive`): fires its `detached`
    /// hook and drops the retained object so no JS reference leaks (mirrors the formatter
    /// clear paths). False for an unknown id.
    pub fn detach_pane_primitive(&mut self, id: u32) -> bool {
        let Some(position) = self.primitives.iter().position(|entry| entry.id == id) else {
            return false;
        };
        let entry = self.primitives.remove(position);
        if let Ok(hook) = js_sys::Reflect::get(&entry.obj, &"detached".into()) {
            if let Ok(hook) = hook.dyn_into::<js_sys::Function>() {
                if let Err(error) = hook.call0(&entry.obj) {
                    web_sys::console::warn_1(
                        &format!("nucleuscharts: pane primitive `detached` hook threw — {error:?}")
                            .into(),
                    );
                }
            }
        }
        true
    }

    /// Attach a series primitive (reference `ISeriesApi.attachPrimitive`, plugin platform Phase C-b)
    /// and return its registry id (0 when the series id is unknown or already removed). Fires
    /// the plugin's `attached` hook with `{series_id, pane_index}` if present (`pane_index`
    /// omitted while the series is pane-less); the TS adapter injects `request_update` before
    /// the plugin sees the params. A throwing hook detaches nothing and is only reported.
    pub fn attach_series_primitive(&mut self, series_id: u32, primitive: js_sys::Object) -> u32 {
        let Some(series) = self
            .series
            .iter()
            .find(|s| s.id == series_id as SeriesId && !s.removed)
        else {
            web_sys::console::warn_1(
                &format!(
                    "nucleuscharts: attach_series_primitive ignored — unknown/removed series {series_id}"
                )
                .into(),
            );
            return 0;
        };
        let pane_index = series.pane_index;
        let id = self.next_primitive_id;
        self.next_primitive_id += 1;
        self.series_primitives.push(SeriesPrimitiveEntry {
            id,
            series: series_id,
            obj: primitive.clone(),
        });
        if let Ok(hook) = js_sys::Reflect::get(&primitive, &"attached".into()) {
            if let Ok(hook) = hook.dyn_into::<js_sys::Function>() {
                let params = js_sys::Object::new();
                let _ = js_sys::Reflect::set(&params, &"series_id".into(), &series_id.into());
                if pane_index < self.panes.len() {
                    let _ = js_sys::Reflect::set(
                        &params,
                        &"pane_index".into(),
                        &(pane_index as u32).into(),
                    );
                }
                if let Err(error) = hook.call1(&primitive, &params) {
                    web_sys::console::warn_1(
                        &format!(
                            "nucleuscharts: series primitive `attached` hook threw — {error:?}"
                        )
                        .into(),
                    );
                }
            }
        }
        id
    }

    /// Detach a series primitive by id (reference `ISeriesApi.detachPrimitive`): fires its `detached`
    /// hook and drops the retained object. False for an unknown id.
    pub fn detach_series_primitive(&mut self, id: u32) -> bool {
        let Some(position) = self
            .series_primitives
            .iter()
            .position(|entry| entry.id == id)
        else {
            return false;
        };
        let entry = self.series_primitives.remove(position);
        fire_primitive_detached(&entry.obj);
        true
    }

    /// Auto-detach every series primitive whose owning series is gone (reference drops a removed
    /// series' primitives with it, chart-model.ts `removeSeries`). Called after any
    /// `remove_series` — indicator outputs tombstoned alongside their source are covered too,
    /// since they scan by live state rather than by the removed id.
    fn detach_orphaned_series_primitives(&mut self) {
        if self.series_primitives.is_empty() {
            return;
        }
        let entries = std::mem::take(&mut self.series_primitives);
        let mut orphans = Vec::new();
        for entry in entries {
            let live = self
                .series
                .iter()
                .any(|s| s.id == entry.series as SeriesId && !s.removed);
            if !live {
                orphans.push(entry);
            } else {
                self.series_primitives.push(entry);
            }
        }
        for entry in orphans {
            fire_primitive_detached(&entry.obj);
        }
    }

    /// Merge a snake_case JSON patch of price-scale options into one pane scale (reference
    /// `priceScale.applyOptions`; unknown keys are ignored). Mode/width-affecting keys force
    /// a full axis-width renegotiation like `set_price_scale_mode`.
    pub fn price_scale_apply_options_json(&mut self, pane: u32, target: u8, json: &str) {
        if !self.engine.price_scale_apply_options_json(
            pane as usize,
            price_scale_target_from_u8(target),
            json,
        ) {
            web_sys::console::warn_1(
                &"nucleuscharts: price_scale_apply_options_json ignored (unknown pane/target or malformed JSON)".into(),
            );
            return;
        }
        self.recompute_layout(true);
    }

    /// One pane scale's full options as a snake_case JSON string ("" for an unknown
    /// pane/target) — reference `priceScale.options()`.
    pub fn price_scale_options_json(&self, pane: u32, target: u8) -> String {
        self.engine
            .price_scale_options_json(pane as usize, price_scale_target_from_u8(target))
            .unwrap_or_default()
    }

    /// 0 = candlestick, 1 = OHLC bars, 2 = line, 3 = area, 4 = histogram (sets the main series).
    pub fn set_series_type(&mut self, kind: u8) {
        self.engine
            .convert_series_kind(0, SeriesKind::from_u8(kind));
    }

    pub fn set_time_visible(&mut self, visible: bool) {
        // reference `timeScale.timeVisible` — label semantics only (whether tick/crosshair labels
        // include the time of day). Strip reservation is `set_time_axis_visible`.
        self.engine.set_time_visible(visible);
    }

    /// reference `timeScale.visible`: reserve/collapse the whole time-axis strip.
    pub fn set_time_axis_visible(&mut self, visible: bool) {
        self.engine.set_time_axis_visible(visible);
        // The strip reservation feeds the pane content height — relayout immediately so
        // getters (`pane_height`, `time_scale_height`) agree before the next render.
        self.recompute_layout(true);
    }

    /// reference `timeScale.ticksVisible`: tick marks beside the time-axis labels.
    pub fn set_time_ticks_visible(&mut self, visible: bool) {
        self.engine.set_time_ticks_visible(visible);
    }

    /// reference `timeScale.minimumHeight` (CSS px): floor for the time-axis strip height.
    pub fn set_time_axis_minimum_height(&mut self, height: f64) {
        self.engine.set_time_axis_minimum_height(height);
        self.recompute_layout(true);
    }

    /// reference `timeScale.tickMarkMaxCharacterLength` (0 restores the default 8).
    pub fn set_tick_mark_max_character_length(&mut self, n: u32) {
        self.engine.set_tick_mark_max_character_length(n);
    }

    /// Set/clear the hovered pane separator (reference pane-separator.ts hover; -1 = none). The
    /// host repaints; the next axis frame carries the band position.
    pub fn set_separator_hover(&mut self, index: i32) {
        self.engine
            .set_separator_hover((index >= 0).then_some(index as usize));
    }

    /// reference `timeScale.secondsVisible`: include seconds in time labels when the time is shown.
    pub fn set_seconds_visible(&mut self, visible: bool) {
        self.engine.set_seconds_visible(visible);
    }

    /// reference `timeScale.minBarSpacing`.
    pub fn set_min_bar_spacing(&mut self, spacing: f64) {
        self.engine.set_min_bar_spacing(spacing);
    }

    /// reference `timeScale.maxBarSpacing` (CSS px; 0 restores the default half-width cap).
    pub fn set_max_bar_spacing(&mut self, spacing: f64) {
        self.engine.set_max_bar_spacing(spacing);
    }

    /// reference `timeScale().applyOptions({ barSpacing })`: write the option and apply it live.
    pub fn apply_bar_spacing_option(&mut self, spacing: f64) {
        self.engine.apply_bar_spacing_option(spacing);
    }

    /// reference `timeScale().applyOptions({ rightOffset })`: write the option and apply it live.
    pub fn apply_right_offset_option(&mut self, offset: f64) {
        self.engine.apply_right_offset_option(offset);
    }

    /// reference `timeScale.rightOffsetPixels`: pin the right offset in pixels.
    pub fn set_right_offset_pixels(&mut self, pixels: f64) {
        self.engine.set_right_offset_pixels(pixels);
    }

    /// reference `timeScale.fixLeftEdge`.
    pub fn set_fix_left_edge(&mut self, fix: bool) {
        self.engine.set_fix_left_edge(fix);
    }

    /// reference `timeScale.fixRightEdge`.
    pub fn set_fix_right_edge(&mut self, fix: bool) {
        self.engine.set_fix_right_edge(fix);
    }

    /// reference `timeScale.lockVisibleTimeRangeOnResize`.
    pub fn set_lock_visible_time_range_on_resize(&mut self, lock: bool) {
        self.engine.set_lock_visible_time_range_on_resize(lock);
    }

    /// reference `timeScale.rightBarStaysOnScroll`.
    pub fn set_right_bar_stays_on_scroll(&mut self, stays: bool) {
        self.engine.set_right_bar_stays_on_scroll(stays);
    }

    /// reference `timeScale.shiftVisibleRangeOnNewBar` (default true): when the last bar is
    /// visible, the view follows newly appended bars.
    pub fn set_shift_visible_range_on_new_bar(&mut self, shift: bool) {
        self.engine.set_shift_visible_range_on_new_bar(shift);
    }

    /// reference `timeScale.allowShiftVisibleRangeOnWhitespaceReplacement` (default false).
    pub fn set_allow_shift_visible_range_on_whitespace_replacement(&mut self, allow: bool) {
        self.engine
            .set_allow_shift_visible_range_on_whitespace_replacement(allow);
    }

    /// reference `timeScale.allowBoldLabels` (default true): bold the major time tick labels.
    pub fn set_allow_bold_labels(&mut self, allow: bool) {
        self.engine.set_allow_bold_labels(allow);
    }

    /// reference `localization.dateFormat` (default `dd MMM \'yy`): the crosshair time-label
    /// pattern. Tokens `dd`/`d`, `MM`/`M`/`MMM`/`MMMM`, `yy`/`yyyy` with `'…'` quoting.
    pub fn set_date_format(&mut self, pattern: &str) {
        self.engine.set_date_format(pattern);
    }

    /// reference `localization.locale` (default the browser language): regenerate the engine's
    /// month-name tables (12 short + 12 long) from `Intl.DateTimeFormat` so the date-format
    /// `MMM`/`MMMM` tokens and the month tick labels localize. An invalid/unsupported tag
    /// warns and keeps the current tables.
    pub fn set_locale(&mut self, locale: &str) {
        let Some((short, long)) = locale_month_names(locale) else {
            web_sys::console::warn_1(
                &format!("nucleuscharts: set_locale ignored unsupported locale {locale:?}").into(),
            );
            return;
        };
        self.engine.set_month_names(short, long);
    }

    /// reference v5.2 `ISeriesApi.pop(count)`: remove the last `count` data points (clamped to the
    /// data length; point colors shift along). Returns the new data length (0 for an
    /// unknown/removed id — such a series has no data anyway). A custom series' host-side
    /// items truncate in lockstep with the engine rows.
    pub fn series_pop(&mut self, id: u32, count: u32) -> u32 {
        let new_len = self
            .engine
            .series_pop(id as SeriesId, count as usize)
            .unwrap_or(0) as u32;
        if let Some(entry) = self.custom_series.iter_mut().find(|e| e.series == id) {
            crate::custom_align::pop_items(&mut entry.times, &mut entry.items, count as usize);
        }
        new_len
    }

    /// reference `ISeriesApi.lastValueData(globalLast)`: JSON `{"value","formatted","time"}` of the
    /// last (global) or last visible non-whitespace bar; "" when there is none.
    pub fn series_last_value_data(&self, id: u32, global_last: bool) -> String {
        self.engine
            .series_last_value_data(id as SeriesId, global_last)
            .unwrap_or_default()
    }

    /// Format a value with the series' resolved price format, backing the TS
    /// `series.priceFormatter()` ("" for an unknown/removed id).
    pub fn series_format_price(&self, id: u32, value: f64) -> String {
        self.engine
            .series_format_price(id as SeriesId, value)
            .unwrap_or_default()
    }

    /// reference `chart.setCrosshairPosition(price, time, series)`: position the crosshair at a
    /// data point with no DOM event; the next render draws it (the TS layer emits its
    /// crosshair-move event). False when the time is not a bar or the series/scale can't
    /// place it.
    pub fn set_crosshair_position(&mut self, price: f64, time: f64, series_id: u32) -> bool {
        self.engine
            .set_crosshair_position(price, time, series_id as SeriesId)
    }

    /// reference `chart.clearCrosshairPosition`: clear the programmatic crosshair (see the engine
    /// note on the stored position/offset).
    pub fn clear_crosshair_position(&mut self) {
        self.engine.clear_crosshair_position();
    }

    /// Series ids in current render order (topmost LAST) as a JSON array — backs the TS
    /// `chart.seriesOrder()`.
    pub fn series_order_json(&self) -> String {
        self.engine.series_order_json()
    }

    /// reference `chart.setSeriesOrder`: reorder which series paints on top. The ids must name
    /// every live series exactly once; a bad permutation is rejected (false, no change).
    pub fn set_series_order(&mut self, ids: Vec<u32>) -> bool {
        self.engine
            .set_series_order(ids.into_iter().map(|id| id as SeriesId).collect())
    }

    /// Host-pushed "all scaling and scrolling disabled" aggregate (reference
    /// `_isAllScalingAndScrollingDisabled`): forces fix-edge semantics on the time scale.
    pub fn set_interaction_disabled(&mut self, disabled: bool) {
        self.engine.set_interaction_disabled(disabled);
    }

    /// Install/clear the host price formatter (reference `localization.priceFormatter`). The JS callback
    /// receives the numeric price and returns a string; a throw or non-string result falls back to
    /// the built-in formatter.
    pub fn set_price_formatter(&mut self, f: Option<js_sys::Function>) {
        self.engine.set_price_formatter(f.map(|func| {
            Box::new(move |price: f64| {
                func.call1(&JsValue::NULL, &JsValue::from_f64(price))
                    .ok()
                    .and_then(|v| v.as_string())
            }) as PriceFormatterFn
        }));
    }

    /// Install/clear the host time-axis tick formatter (reference `timeScale.tickMarkFormatter`). The JS
    /// callback receives `(timeSeconds, tickMarkType)`.
    pub fn set_tick_mark_formatter(&mut self, f: Option<js_sys::Function>) {
        self.engine.set_tick_mark_formatter(f.map(|func| {
            Box::new(move |ts: i64, tick_type: u8| {
                func.call2(
                    &JsValue::NULL,
                    &JsValue::from_f64(ts as f64),
                    &JsValue::from_f64(tick_type as f64),
                )
                .ok()
                .and_then(|v| v.as_string())
            }) as TickMarkFormatterFn
        }));
    }

    /// Install/clear the host crosshair time formatter (reference `localization.timeFormatter`). The JS
    /// callback receives the UTC-second timestamp.
    pub fn set_time_formatter(&mut self, f: Option<js_sys::Function>) {
        self.engine.set_time_formatter(f.map(|func| {
            Box::new(move |ts: i64| {
                func.call1(&JsValue::NULL, &JsValue::from_f64(ts as f64))
                    .ok()
                    .and_then(|v| v.as_string())
            }) as TimeFormatterFn
        }));
    }

    /// 0 = normal, 1 = magnet (reference default), 2 = hidden, 3 = magnet OHLC.
    pub fn set_crosshair_mode(&mut self, mode: u8) {
        self.crosshair_mode = crosshair_mode_from_u8(mode);
        // keep the options store consistent so `options()` reflects it
        self.options.apply(&nucleuscharts_core::options::patch(
            "crosshair",
            serde_json::json!({ "mode": mode }),
        ));
    }

    /// Deep-merge a JSON options patch and apply the runtime-affecting fields (crosshair mode,
    /// plus any behavioral `timeScale` keys routed to the core scale by the engine). Colors
    /// (grid/crosshair/background) are read from the store during `render`. Call `render()`
    /// after to repaint (roadmap Phase A2).
    pub fn apply_options(&mut self, patch_json: &str) {
        // `localization.locale` needs the host's `Intl` (the engine is headless), so it is
        // intercepted here; the engine routes `localization.dateFormat` itself and the store
        // keeps both keys for the options round-trip.
        if let Ok(patch) = serde_json::from_str::<serde_json::Value>(patch_json) {
            if let Some(locale) = patch
                .get("localization")
                .and_then(|l| l.get("locale"))
                .and_then(serde_json::Value::as_str)
            {
                self.set_locale(locale);
            }
        }
        if let Err(e) = self.engine.apply_options(patch_json) {
            web_sys::console::warn_1(
                &format!("nucleuscharts: apply_options ignored malformed patch - {e}").into(),
            );
        }
        // reference `applyOptions` triggers a FULL update: the axis width renegotiates in both
        // directions (a smaller font/narrower labels shrink the strip). The grow-only rule
        // applies only to the incremental marks path (render()).
        self.recompute_layout(true);
    }

    /// Current options as a JSON string (round-trips the deep-merged state back to JS).
    pub fn options_json(&self) -> String {
        self.options.value().to_string()
    }

    /// A series' current options as a snake_case JSON string ("" for an unknown/removed id).
    pub fn series_options_json(&self, id: u32) -> String {
        self.engine
            .series_options_json(id as SeriesId)
            .unwrap_or_default()
    }

    /// Merge a snake_case JSON patch of series style options into the series (reference
    /// `series.applyOptions`; unknown keys are ignored gracefully).
    pub fn series_apply_options_json(&mut self, id: u32, json: &str) {
        if !self.engine.series_apply_options_json(id as SeriesId, json) {
            web_sys::console::warn_1(
                &"nucleuscharts: series_apply_options_json ignored (unknown id or malformed JSON)"
                    .into(),
            );
        }
    }

    /// All time-scale options as a snake_case JSON string.
    pub fn time_scale_options_json(&self) -> String {
        self.engine.time_scale_options_json()
    }

    /// Typed snapshot of the current options for the render path.
    pub(super) fn opts(&self) -> ChartOptions {
        self.options.get()
    }

    pub fn resize(&mut self, css_width: f64, css_height: f64, dpr: f64) {
        self.css_width = css_width;
        self.css_height = css_height;
        self.dpr = dpr;
        let bitmap_w = (css_width * dpr).round().max(1.0) as u32;
        let bitmap_h = (css_height * dpr).round().max(1.0) as u32;
        self.bitmap_w = bitmap_w;
        self.bitmap_h = bitmap_h;
        if let Some(gfx) = self.gfx.as_mut() {
            gfx.config.width = bitmap_w;
            gfx.config.height = bitmap_h;
            gfx.surface.configure(&gfx.shared.device, &gfx.config);
        }
        // Update geometry eagerly so fit_content/zoom/scroll called before the next render
        // (and the price_axis_width getter) see the new pane size, not a stale one.
        self.recompute_layout(true);
    }

    /// Negotiates the price-axis width against its labels and sets the time-scale width / price
    /// scale height. The shared engine policy owns all geometry; this browser host supplies only
    /// Canvas text widths.
    pub(super) fn recompute_layout(&mut self, allow_axis_shrink: bool) {
        let axis_ctx = self.axis_ctx.clone();
        let dpr = self.dpr;
        let layout = self.opts().layout;
        let font_size = layout.font_size;
        let font_family = layout.font_family;
        self.engine
            .recompute_layout_with_measure(allow_axis_shrink, |text| {
                measure_text_ctx(&axis_ctx, dpr, &font_family, font_size, text)
            });
    }

    // --- gestures ---

    pub fn zoom(&mut self, x_css: f64, scale: f64) {
        let x = x_css.max(1.0).min(self.time_scale.width());
        self.time_scale.zoom(x, scale);
    }
    pub fn scroll_start(&mut self, x_css: f64) {
        self.time_scale.start_scroll(x_css);
    }
    pub fn scroll_move(&mut self, x_css: f64) {
        self.time_scale.scroll_to(x_css);
    }
    pub fn scroll_end(&mut self) {
        self.time_scale.end_scroll();
    }

    // --- engine-owned interaction models (kinetic, axis drag-to-scale, price pan, eased
    // scroll): the TS recognizer forwards normalized samples and schedules frames; every
    // formula lives in the engine so the headless harness runs the same code. ---

    /// reference wheel zoom increment: `sign(deltaY) * min(1, |deltaY|)`.
    pub fn wheel_zoom_scale(&self, delta_y: f64) -> f64 {
        nucleuscharts_engine::wheel_zoom_scale(delta_y)
    }
    /// reference pinch zoom increment: the scale-ratio delta ×5.
    pub fn pinch_zoom_scale(&self, scale_delta: f64) -> f64 {
        nucleuscharts_engine::pinch_zoom_scale(scale_delta)
    }
    /// reference wheel scroll: `deltaX * -80` px ("made-up coefficient").
    pub fn wheel_scroll_delta(&self, delta_x: f64) -> f64 {
        delta_x * nucleuscharts_engine::WHEEL_SCROLL_PX_PER_DELTA
    }

    /// Open a kinetic sampling session alongside the drag (seeded with the first sample);
    /// `enabled = false` mirrors the reference's null animation (no coast on release).
    pub fn kinetic_begin_sampling(&mut self, enabled: bool, x_css: f64, now_ms: f64) {
        self.engine.kinetic_begin_sampling(enabled, x_css, now_ms);
    }
    pub fn kinetic_add_sample(&mut self, x_css: f64, now_ms: f64) {
        self.engine.kinetic_add_sample(x_css, now_ms);
    }
    /// The drag was released: returns whether a momentum coast engaged (the host then drives
    /// `kinetic_position` per frame instead of ending the scroll session).
    pub fn kinetic_release(&mut self, x_css: f64, now_ms: f64) -> bool {
        self.engine.kinetic_release(x_css, now_ms)
    }
    /// The coast's pointer position at `now_ms` (-1 sentinel-free: NaN when no coast runs).
    pub fn kinetic_position(&self, now_ms: f64) -> f64 {
        self.engine.kinetic_position(now_ms).unwrap_or(f64::NAN)
    }
    pub fn kinetic_finished(&self, now_ms: f64) -> bool {
        self.engine.kinetic_finished(now_ms)
    }
    pub fn kinetic_stop(&mut self) {
        self.engine.kinetic_stop();
    }

    /// Axis drag-to-scale arms/applies (reference `TimeAxisWidget`/`PriceAxisWidget`
    /// pressedMouseMove): y is chart-content CSS px for price axes, x pane-relative for time.
    pub fn time_axis_start_scale(&mut self, x_css: f64) {
        self.engine.time_axis_start_scale(x_css);
    }
    pub fn time_axis_scale_to(&mut self, x_css: f64) {
        self.engine.time_axis_scale_to(x_css);
    }
    pub fn time_axis_end_scale(&mut self) {
        self.engine.time_axis_end_scale();
    }
    /// Whether a price-axis drag can scale this scale (false in percentage/indexed-to-100
    /// modes or with no range — reference `PriceScale.scaleTo` no-ops there).
    pub fn price_axis_scalable(&self, pane: usize, target: u8) -> bool {
        self.engine
            .price_axis_scalable(pane, price_scale_target_from_u8(target))
    }
    pub fn price_axis_start_scale(&mut self, pane: usize, target: u8, y_css: f64) {
        self.engine
            .price_axis_start_scale(pane, price_scale_target_from_u8(target), y_css);
    }
    pub fn price_axis_scale_to(&mut self, pane: usize, target: u8, y_css: f64) {
        self.engine
            .price_axis_scale_to(pane, price_scale_target_from_u8(target), y_css);
    }
    pub fn price_axis_end_scale(&mut self, pane: usize, target: u8) {
        self.engine
            .price_axis_end_scale(pane, price_scale_target_from_u8(target));
    }
    /// Vertical price pan (reference `startScrollPrice`/`scrollPriceTo`; no-ops in autoscale).
    pub fn price_axis_start_scroll(&mut self, pane: usize, target: u8, y_css: f64) {
        self.engine
            .price_axis_start_scroll(pane, price_scale_target_from_u8(target), y_css);
    }
    pub fn price_axis_scroll_to(&mut self, pane: usize, target: u8, y_css: f64) {
        self.engine
            .price_axis_scroll_to(pane, price_scale_target_from_u8(target), y_css);
    }
    pub fn price_axis_end_scroll(&mut self, pane: usize, target: u8) {
        self.engine
            .price_axis_end_scroll(pane, price_scale_target_from_u8(target));
    }

    /// Eased scroll-to-position (cubic ease-out): the engine owns the easing and applies each
    /// tick; the host schedules frames and repaints. A newer start or a user gesture
    /// (`cancel_scroll_animation`) supersedes.
    pub fn start_scroll_animation(&mut self, target: f64, duration_ms: f64, now_ms: f64) {
        self.engine
            .start_scroll_animation(target, duration_ms, now_ms);
    }
    /// Apply the eased position for `now_ms`; returns NaN when the animation is finished or
    /// none is running (the host then stops scheduling frames).
    pub fn scroll_animation_tick(&mut self, now_ms: f64) -> f64 {
        self.engine
            .scroll_animation_tick(now_ms)
            .unwrap_or(f64::NAN)
    }
    pub fn cancel_scroll_animation(&mut self) {
        self.engine.cancel_scroll_animation();
    }
    /// Index of the stacked pane containing content-y `y` (engine-owned pane bounds).
    pub fn pane_index_at_y(&self, y_css: f64) -> usize {
        self.engine.pane_index_at_y(y_css)
    }

    pub fn fit_content(&mut self) {
        self.engine.fit_content();
    }
    pub fn scroll_position(&self) -> f64 {
        self.engine.scroll_position()
    }
    pub fn scroll_to_position(&mut self, position: f64) {
        self.engine.scroll_to_position(position);
    }
    pub fn scroll_to_real_time(&mut self) {
        self.engine.scroll_to_real_time();
    }
    pub fn reset_time_scale(&mut self) {
        self.engine.reset_time_scale();
    }
    pub fn time_scale_width(&self) -> f64 {
        self.time_scale.width()
    }
    pub fn time_scale_height(&self) -> f64 {
        // reference `timeScale().height()`: the reserved strip height (0 when `timeScale.visible`
        // is false, else the auto height floored at `minimumHeight`).
        self.engine.time_axis_height()
    }
    pub fn price_scale_width(&self, pane: usize, target: u8) -> f64 {
        if pane >= self.panes.len() {
            return 0.0;
        }
        match price_scale_target_from_u8(target) {
            PriceScaleTarget::Right => self.axis_w,
            PriceScaleTarget::Left => self.left_axis_w,
            PriceScaleTarget::Overlay => 0.0,
        }
    }
    pub fn price_scale_visible_range(&self, pane: usize, target: u8) -> Vec<f64> {
        self.engine
            .price_scale_visible_range_for(pane, price_scale_target_from_u8(target))
            .map(|(from, to)| vec![from, to])
            .unwrap_or_default()
    }
    pub fn set_price_scale_visible_range(&mut self, pane: usize, target: u8, from: f64, to: f64) {
        self.engine.set_price_scale_visible_range_for(
            pane,
            price_scale_target_from_u8(target),
            from,
            to,
        );
    }
    pub fn price_scale_auto_scale(&self, pane: usize, target: u8) -> Option<bool> {
        self.engine
            .price_scale_auto_scale_for(pane, price_scale_target_from_u8(target))
    }
    pub fn set_price_scale_auto_scale(&mut self, pane: usize, target: u8, enabled: bool) {
        self.engine.set_price_scale_auto_scale_for(
            pane,
            price_scale_target_from_u8(target),
            enabled,
        );
    }
    pub fn price_scale_inverted(&self, pane: usize, target: u8) -> Option<bool> {
        self.engine
            .price_scale_inverted_for(pane, price_scale_target_from_u8(target))
    }
    pub fn set_price_scale_inverted(&mut self, pane: usize, target: u8, inverted: bool) {
        self.engine.set_price_scale_inverted_for(
            pane,
            price_scale_target_from_u8(target),
            inverted,
        );
    }
    pub fn price_scale_margins(&self, pane: usize, target: u8) -> Vec<f64> {
        self.engine
            .price_scale_margins_for(pane, price_scale_target_from_u8(target))
            .map(|(top, bottom)| vec![top, bottom])
            .unwrap_or_default()
    }
    pub fn set_price_scale_margins(&mut self, pane: usize, target: u8, top: f64, bottom: f64) {
        self.engine.set_price_scale_margins_for(
            pane,
            price_scale_target_from_u8(target),
            top,
            bottom,
        );
    }
    pub fn price_scale_mode(&self, pane: usize, target: u8) -> Option<u8> {
        self.engine
            .price_scale_mode_for(pane, price_scale_target_from_u8(target))
            .map(price_scale_mode_to_u8)
    }
    pub fn set_price_scale_mode(&mut self, pane: usize, target: u8, mode: u8) {
        self.engine.set_price_scale_mode_for(
            pane,
            price_scale_target_from_u8(target),
            price_scale_mode_from_u8(mode),
        );
        // A mode change is a full layout invalidation in reference: label formatting can become wider
        // (percentage) or narrower (indexed/normal), so the grow-fast/shrink-on-full-layout axis
        // policy must be allowed to renegotiate in both directions immediately.
        self.recompute_layout(true);
    }
    pub fn series_pane_index(&self, id: u32) -> Option<usize> {
        self.engine
            .series_price_scale(id as SeriesId)
            .map(|(pane, _)| pane)
    }
    pub fn series_is_overlay(&self, id: u32) -> Option<bool> {
        self.engine
            .series_price_scale(id as SeriesId)
            .map(|(_, target)| target == PriceScaleTarget::Overlay)
    }
    pub fn series_price_scale_id(&self, id: u32) -> Option<u8> {
        self.engine
            .series_price_scale(id as SeriesId)
            .map(|(_, target)| price_scale_target_to_u8(target))
    }
    pub fn set_series_price_scale(&mut self, id: u32, target: u8) {
        self.engine
            .set_series_price_scale(id as SeriesId, price_scale_target_from_u8(target));
        self.recompute_layout(true);
    }
    pub fn series_price_to_coordinate(&self, id: u32, price: f64) -> Option<f64> {
        self.engine
            .series_price_to_coordinate(id as SeriesId, price)
    }
    pub fn series_coordinate_to_price(&self, id: u32, coordinate: f64) -> Option<f64> {
        self.engine
            .series_coordinate_to_price(id as SeriesId, coordinate)
    }
    pub fn series_kind(&self, id: u32) -> Option<u8> {
        self.engine
            .series_kind(id as SeriesId)
            .map(SeriesKind::to_u8)
    }
    pub fn series_data_by_index(&self, id: u32, index: f64, mismatch: i8) -> Vec<f64> {
        if !index.is_finite() || index.fract() != 0.0 {
            return Vec::new();
        }
        self.engine
            .series_data_by_index(
                id as SeriesId,
                index as i64,
                mismatch_direction_from_i8(mismatch),
            )
            .map(|point| {
                vec![
                    point.time as f64,
                    point.open,
                    point.high,
                    point.low,
                    point.close,
                ]
            })
            .unwrap_or_default()
    }
    pub fn series_data(&self, id: u32) -> Vec<f64> {
        let points = self.engine.series_data(id as SeriesId);
        let mut output = Vec::with_capacity(points.len() * 5);
        for point in points {
            output.extend_from_slice(&[
                point.time as f64,
                point.open,
                point.high,
                point.low,
                point.close,
            ]);
        }
        output
    }
    pub fn series_bars_in_logical_range(&self, id: u32, from: f64, to: f64) -> Vec<f64> {
        self.engine
            .series_bars_in_logical_range(id as SeriesId, from, to)
            .map(|info| {
                let mut output = vec![info.bars_before, info.bars_after];
                if let (Some(from), Some(to)) = (info.from, info.to) {
                    output.extend_from_slice(&[from as f64, to as f64]);
                }
                output
            })
            .unwrap_or_default()
    }
    pub fn set_crosshair(&mut self, x_css: f64, y_css: f64) {
        self.crosshair = Some((x_css, y_css));
    }
    pub fn clear_crosshair(&mut self) {
        self.crosshair = None;
    }

    // --- drawing tools (engine-owned drawing objects; nucleuscharts_engine drawings.rs) ---
    //
    // Thin wire adapters: kinds cross as `u8` (`DrawingKind::from_u8`), anchors and options as
    // JSON strings. All state, hit-testing, and drag math is engine-side; the gesture layer only
    // forwards pointer samples and repaints.

    /// Add a drawing from a JSON `[{logical, price}, ...]` anchor array plus an optional options
    /// patch ("" = defaults). Returns the drawing id, or 0 when the engine rejects it.
    pub fn add_drawing(
        &mut self,
        kind: u8,
        pane: usize,
        points_json: &str,
        options_json: &str,
    ) -> u32 {
        let Some(kind) = DrawingKind::from_u8(kind) else {
            return 0;
        };
        let Ok(points) = serde_json::from_str::<Vec<DrawingPoint>>(points_json) else {
            return 0;
        };
        let options = (!options_json.is_empty()).then_some(options_json);
        self.engine
            .add_drawing(kind, pane, points, options)
            .unwrap_or(0)
    }
    pub fn drawing_apply_options(&mut self, id: u32, options_json: &str) -> bool {
        self.engine.drawing_apply_options(id, options_json)
    }
    pub fn drawing_set_points(&mut self, id: u32, points_json: &str) -> bool {
        self.engine.drawing_set_points(id, points_json)
    }
    /// The drawing's options JSON ("" for an unknown id — the wasm boundary has no Option<String>).
    pub fn drawing_options_json(&self, id: u32) -> String {
        self.engine.drawing_options_json(id).unwrap_or_default()
    }
    pub fn drawing_points_json(&self, id: u32) -> String {
        self.engine.drawing_points_json(id).unwrap_or_default()
    }
    /// One anchor's CSS-px position `[x, y]` (x includes the pane's left offset — overlay
    /// space; empty when it cannot convert). The text editor positions itself with it.
    pub fn drawing_point_to_coordinate(&self, id: u32, index: usize) -> Vec<f64> {
        self.engine
            .drawing_point_to_coordinate(id, index)
            .map(|(x, y)| vec![x + self.pane_left, y])
            .unwrap_or_default()
    }
    pub fn drawings_json(&self) -> String {
        self.engine.drawings_json()
    }
    pub fn remove_drawing(&mut self, id: u32) -> bool {
        self.engine.remove_drawing(id)
    }
    pub fn clear_drawings(&mut self) {
        self.engine.clear_drawings();
    }
    /// Click-to-select arbitration: selects the drawing under the point (clearing on a miss) and
    /// reports whether one was hit, so the host can skip its series-selection path.
    pub fn select_drawing_at(&mut self, x_css: f64, y_css: f64) -> bool {
        self.engine.select_drawing_at(x_css, y_css)
    }
    pub fn set_selected_drawing(&mut self, id: Option<u32>) {
        self.engine.set_selected_drawing(id);
    }
    pub fn selected_drawing(&self) -> Option<u32> {
        self.engine.selected_drawing()
    }
    /// Delete/Backspace: remove the selected drawing. False while nothing is selected.
    pub fn remove_selected_drawing(&mut self) -> bool {
        self.engine.remove_selected_drawing()
    }
    /// Press routing: opens an anchor/body drag on the drawing under the point (false = the host
    /// falls through to pan/scroll).
    pub fn drawing_drag_start_at(&mut self, x_css: f64, y_css: f64) -> bool {
        self.engine.drawing_drag_start_at(x_css, y_css)
    }
    /// Forward a drag position with the modifier state (magnet = OHLC snap, straighten =
    /// 0°/45°/90° anchor constraint / dominant-axis body move; drawings.rs `DrawingModifiers`).
    pub fn drawing_drag_to(&mut self, x_css: f64, y_css: f64, magnet: bool, straighten: bool) {
        self.engine
            .drawing_drag_to(x_css, y_css, DrawingModifiers { magnet, straighten });
    }
    pub fn drawing_drag_end(&mut self) {
        self.engine.drawing_drag_end();
    }
    pub fn drawing_drag_active(&self) -> bool {
        self.engine.drawing_drag_active()
    }
    /// Arm interactive creation of a tool kind ("" options = defaults).
    pub fn drawing_create_begin(&mut self, kind: u8, options_json: &str) -> bool {
        let Some(kind) = DrawingKind::from_u8(kind) else {
            return false;
        };
        let options = (!options_json.is_empty()).then_some(options_json);
        self.engine.drawing_create_begin(kind, options)
    }
    /// Place the next creation anchor (modifiers snap it): 0 unarmed, -1 pending more anchors,
    /// > 0 the committed id.
    pub fn drawing_create_click(
        &mut self,
        x_css: f64,
        y_css: f64,
        magnet: bool,
        straighten: bool,
    ) -> i64 {
        self.engine
            .drawing_create_click(x_css, y_css, DrawingModifiers { magnet, straighten })
    }
    pub fn drawing_create_move(&mut self, x_css: f64, y_css: f64, magnet: bool, straighten: bool) {
        self.engine
            .drawing_create_move(x_css, y_css, DrawingModifiers { magnet, straighten });
    }
    pub fn drawing_create_cancel(&mut self) {
        self.engine.drawing_create_cancel();
    }
    pub fn drawing_create_active(&self) -> bool {
        self.engine.drawing_create_active()
    }

    /// Begin a freehand brush stroke (pointer-down with the brush tool armed; "" options =
    /// defaults). False off the panes/data.
    pub fn brush_create_start(&mut self, options_json: &str, x_css: f64, y_css: f64) -> bool {
        let options = (!options_json.is_empty()).then_some(options_json);
        self.engine.brush_create_start(options, x_css, y_css)
    }
    /// Capture the next stroke point (engine-decimated by distance).
    pub fn brush_create_add(&mut self, x_css: f64, y_css: f64) {
        self.engine.brush_create_add(x_css, y_css);
    }
    /// Commit the stroke (pointer-up): RDP-simplified and stored as a selected drawing. 0 =
    /// degenerate stroke discarded.
    pub fn brush_create_end(&mut self) -> u32 {
        self.engine.brush_create_end()
    }
    pub fn brush_create_cancel(&mut self) {
        self.engine.brush_create_cancel();
    }
    pub fn brush_create_active(&self) -> bool {
        self.engine.brush_create_active()
    }
    pub fn price_axis_width(&self) -> f64 {
        self.axis_w
    }
    pub fn pane_left(&self) -> f64 {
        self.pane_left
    }

    // --- coordinate & logical-range API (roadmap Phase A4) ---
    //
    // Reflects the state of the last render (scale height/width, price range). All coordinates
    // are media (CSS) pixels relative to the pane offset, matching the pointer coords JS passes
    // to `set_crosshair`. `None`/empty means the query falls off the chart or there is no data.

    /// The primary series id for the pane-level coordinate API: the first visible,
    /// non-removed series (id 0 may be tombstoned via `remove_series`).
    fn primary_series_id(&self) -> Option<SeriesId> {
        self.series
            .iter()
            .find(|s| s.visible && !s.removed)
            .map(|s| s.id)
    }

    /// Y (CSS px) for a price on the active price scale, or `None` if the scale has no range yet.
    /// In percentage/indexed modes the price is its own base value (as in the render path).
    pub fn price_to_coordinate(&self, price: f64) -> Option<f64> {
        self.engine
            .series_price_to_coordinate(self.primary_series_id()?, price)
    }

    /// Price for a Y (CSS px), or `None` if the scale has no range yet.
    pub fn coordinate_to_price(&self, y_css: f64) -> Option<f64> {
        self.engine
            .series_coordinate_to_price(self.primary_series_id()?, y_css)
    }

    /// X (CSS px) for a UTC-seconds timestamp that sits exactly on a data point, else `None`
    /// (mirrors reference `timeToCoordinate`, which does not snap to the nearest bar).
    pub fn time_to_coordinate(&self, time: f64) -> Option<f64> {
        self.engine.time_to_coordinate(time)
    }

    /// UTC-seconds timestamp of the data point nearest to X (CSS px), or `None` if X maps outside
    /// the data range (mirrors reference `coordinateToTime`).
    pub fn coordinate_to_time(&self, x_css: f64) -> Option<f64> {
        self.engine.coordinate_to_time(x_css)
    }

    /// Integer logical bar owning an X coordinate, or `None` when there is no data. May be negative
    /// or beyond the last bar, matching the reference's public `coordinateToLogical`.
    pub fn coordinate_to_logical(&self, x_css: f64) -> Option<f64> {
        self.engine.coordinate_to_logical(x_css)
    }

    pub fn logical_to_coordinate(&self, logical: f64) -> Option<f64> {
        self.engine.logical_to_coordinate(logical)
    }

    pub fn time_to_index(&self, time: f64, find_nearest: bool) -> Option<i64> {
        self.engine.time_to_index(time, find_nearest)
    }

    /// Per-series values at the bar under an X coordinate, flattened as groups of five:
    /// `[series_id, open, high, low, close, ...]`. Only series that actually have a point at that
    /// bar are included (single-value series report the value in all four slots; a whitespace
    /// row is no bar and is skipped). Empty when the cursor is off the data. Series are ordered
    /// topmost-first, matching the reference's hit-test order (pane-hit-test.ts reverses the z-order).
    /// Backs the façade's `seriesData` map for crosshair/click events.
    pub fn hover_data(&self, x_css: f64) -> Vec<f64> {
        use nucleuscharts_core::model::plot_list::MismatchDirection;
        let n = self.engine.data_layer().merged_times().len() as i64;
        if n == 0 {
            return Vec::new();
        }
        let index = self.time_scale.coordinate_to_index(x_css);
        if index < 0 || index >= n {
            return Vec::new();
        }
        let mut out = Vec::new();
        for &id in self.engine.series_order().iter().rev() {
            let plot = self.engine.data_layer().plot(id);
            if let Some(row) = plot.search(index, MismatchDirection::None) {
                if plot.is_whitespace_row(row) {
                    continue;
                }
                out.push(id as f64);
                out.push(plot.value_at(row, PlotValueIndex::Open));
                out.push(plot.value_at(row, PlotValueIndex::High));
                out.push(plot.value_at(row, PlotValueIndex::Low));
                out.push(plot.value_at(row, PlotValueIndex::Close));
            }
        }
        out
    }

    /// Visible window in logical (bar) units as `[from, to]`, or empty when there is no data.
    pub fn visible_logical_range(&self) -> Vec<f64> {
        match self.engine.visible_logical_range() {
            Some((from, to)) => vec![from, to],
            None => Vec::new(),
        }
    }

    /// Set the visible window in logical (bar) units. No-op if `from > to`. Call `render()` after.
    pub fn set_visible_logical_range(&mut self, from: f64, to: f64) {
        self.engine.set_visible_logical_range(from, to);
    }

    /// Visible window as `[from_time, to_time]` UTC seconds (data points nearest each edge), or
    /// empty when there is no data.
    pub fn visible_time_range(&self) -> Vec<f64> {
        self.engine
            .visible_time_range()
            .map(|(from, to)| vec![from, to])
            .unwrap_or_default()
    }

    /// Set the visible window to span the data points bracketing `[from_time, to_time]` (UTC
    /// seconds). No-op if the times are reversed or there is no data. Call `render()` after.
    pub fn set_visible_time_range(&mut self, from_time: f64, to_time: f64) {
        self.engine.set_visible_time_range(from_time, to_time);
    }
}

/// 2024-01-01T00:00:00Z in Unix-ms — the reference year for locale month-name generation
/// (mid-month UTC instants, so no time zone can shift the month).
const LOCALE_MONTH_YEAR0_MS: f64 = 1_704_067_200_000.0;
const LOCALE_MONTH_DAY_OFFSETS: [i64; 12] = [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335];

/// Build a month-formatting `Intl.DateTimeFormat` for `locale` (`{month: short|long}`), or
/// `None` when the tag is rejected (the constructor throws for malformed BCP 47 tags; the
/// `Reflect::construct` boundary catches that into a `None`).
fn intl_month_format(locale: &str, month: &str) -> Option<js_sys::Intl::DateTimeFormat> {
    let intl = js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("Intl")).ok()?;
    let ctor = js_sys::Reflect::get(&intl, &JsValue::from_str("DateTimeFormat"))
        .ok()?
        .dyn_into::<js_sys::Function>()
        .ok()?;
    let options = js_sys::Object::new();
    js_sys::Reflect::set(
        &options,
        &JsValue::from_str("month"),
        &JsValue::from_str(month),
    )
    .ok()?;
    let locales = js_sys::Array::of1(&JsValue::from_str(locale));
    let args = js_sys::Array::of2(&locales, &options);
    js_sys::Reflect::construct(&ctor, &args)
        .ok()?
        .dyn_into::<js_sys::Intl::DateTimeFormat>()
        .ok()
}

/// The 12 short and 12 long month names for `locale` (reference `localization.locale`), generated
/// through `Intl.DateTimeFormat` exactly like the reference's `format-date.ts` `toLocaleString` calls.
fn locale_month_names(locale: &str) -> Option<([String; 12], [String; 12])> {
    let short_fmt = intl_month_format(locale, "short")?;
    let long_fmt = intl_month_format(locale, "long")?;
    // js-sys stable models `DateTimeFormat.prototype.format` as its getter: the returned
    // bound function formats one date per call.
    let short_fn = short_fmt.format();
    let long_fn = long_fmt.format();
    let mut short: [String; 12] = Default::default();
    let mut long: [String; 12] = Default::default();
    for (m, offset) in LOCALE_MONTH_DAY_OFFSETS.iter().enumerate() {
        let ms = LOCALE_MONTH_YEAR0_MS + (*offset as f64 + 14.0) * 86_400_000.0;
        let date = js_sys::Date::new(&JsValue::from_f64(ms));
        short[m] = short_fn
            .call1(&JsValue::UNDEFINED, &date)
            .ok()
            .and_then(|v| v.as_string())?;
        long[m] = long_fn
            .call1(&JsValue::UNDEFINED, &date)
            .ok()
            .and_then(|v| v.as_string())?;
    }
    Some((short, long))
}

/// Fire a detached primitive's `detached` hook (shared by the series-primitive detach paths);
/// a throwing hook is only reported.
fn fire_primitive_detached(obj: &js_sys::Object) {
    if let Ok(hook) = js_sys::Reflect::get(obj, &"detached".into()) {
        if let Ok(hook) = hook.dyn_into::<js_sys::Function>() {
            if let Err(error) = hook.call0(obj) {
                web_sys::console::warn_1(
                    &format!("nucleuscharts: series primitive `detached` hook threw — {error:?}")
                        .into(),
                );
            }
        }
    }
}
