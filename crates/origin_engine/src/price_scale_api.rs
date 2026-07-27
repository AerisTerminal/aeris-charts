//! Public price-scale state accessors (mode/autoscale/inversion/margins/visible range) for
//! pane, left/right and overlay scales, plus per-series scale binding. Extracted from `lib.rs`.

use super::*;

impl ChartEngine {
    pub fn price_scale_for(
        &self,
        pane: usize,
        target: PriceScaleTarget,
    ) -> Option<&PriceScaleCore> {
        let pane = self.panes.get(pane)?;
        Some(match target {
            PriceScaleTarget::Right => &pane.price_scale,
            PriceScaleTarget::Left => &pane.left_scale,
            PriceScaleTarget::Overlay => &pane.overlay_scale,
        })
    }

    pub fn price_scale_for_mut(
        &mut self,
        pane: usize,
        target: PriceScaleTarget,
    ) -> Option<&mut PriceScaleCore> {
        let pane = self.panes.get_mut(pane)?;
        Some(match target {
            PriceScaleTarget::Right => &mut pane.price_scale,
            PriceScaleTarget::Left => &mut pane.left_scale,
            PriceScaleTarget::Overlay => &mut pane.overlay_scale,
        })
    }

    /// Current visible raw-value range for a pane price scale.
    pub fn price_scale_visible_range(&self, pane: usize, overlay: bool) -> Option<(f64, f64)> {
        self.price_scale_visible_range_for(
            pane,
            if overlay {
                PriceScaleTarget::Overlay
            } else {
                PriceScaleTarget::Right
            },
        )
    }

    pub fn price_scale_visible_range_for(
        &self,
        pane: usize,
        target: PriceScaleTarget,
    ) -> Option<(f64, f64)> {
        let range = self.price_scale_for(pane, target)?.price_range_for_api()?;
        Some((range.min_value(), range.max_value()))
    }

    /// Install a manual raw-value range and disable autoscale, matching reference `setVisibleRange`.
    pub fn set_price_scale_visible_range(
        &mut self,
        pane: usize,
        overlay: bool,
        from: f64,
        to: f64,
    ) {
        self.set_price_scale_visible_range_for(
            pane,
            if overlay {
                PriceScaleTarget::Overlay
            } else {
                PriceScaleTarget::Right
            },
            from,
            to,
        );
    }

    pub fn set_price_scale_visible_range_for(
        &mut self,
        pane: usize,
        target: PriceScaleTarget,
        from: f64,
        to: f64,
    ) {
        if !from.is_finite() || !to.is_finite() || from >= to {
            return;
        }
        if let Some(scale) = self.price_scale_for_mut(pane, target) {
            scale.set_auto_scale(false);
            let range = scale.price_range_from_api(&PriceRange::new(from, to));
            scale.set_price_range(Some(range));
        }
    }

    pub fn price_scale_auto_scale(&self, pane: usize, overlay: bool) -> Option<bool> {
        self.price_scale_auto_scale_for(
            pane,
            if overlay {
                PriceScaleTarget::Overlay
            } else {
                PriceScaleTarget::Right
            },
        )
    }

    pub fn price_scale_auto_scale_for(
        &self,
        pane: usize,
        target: PriceScaleTarget,
    ) -> Option<bool> {
        Some(self.price_scale_for(pane, target)?.is_auto_scale())
    }

    pub fn set_price_scale_auto_scale(&mut self, pane: usize, overlay: bool, enabled: bool) {
        self.set_price_scale_auto_scale_for(
            pane,
            if overlay {
                PriceScaleTarget::Overlay
            } else {
                PriceScaleTarget::Right
            },
            enabled,
        );
    }

    pub fn set_price_scale_auto_scale_for(
        &mut self,
        pane: usize,
        target: PriceScaleTarget,
        enabled: bool,
    ) {
        if let Some(scale) = self.price_scale_for_mut(pane, target) {
            scale.set_auto_scale(enabled);
        }
    }

    pub fn price_scale_inverted(&self, pane: usize, overlay: bool) -> Option<bool> {
        self.price_scale_inverted_for(
            pane,
            if overlay {
                PriceScaleTarget::Overlay
            } else {
                PriceScaleTarget::Right
            },
        )
    }

    pub fn price_scale_inverted_for(&self, pane: usize, target: PriceScaleTarget) -> Option<bool> {
        Some(self.price_scale_for(pane, target)?.is_inverted())
    }

    pub fn set_price_scale_inverted(&mut self, pane: usize, overlay: bool, inverted: bool) {
        self.set_price_scale_inverted_for(
            pane,
            if overlay {
                PriceScaleTarget::Overlay
            } else {
                PriceScaleTarget::Right
            },
            inverted,
        );
    }

    pub fn set_price_scale_inverted_for(
        &mut self,
        pane: usize,
        target: PriceScaleTarget,
        inverted: bool,
    ) {
        if let Some(scale) = self.price_scale_for_mut(pane, target) {
            scale.set_invert_scale(inverted);
        }
    }

    pub fn price_scale_margins(&self, pane: usize, overlay: bool) -> Option<(f64, f64)> {
        self.price_scale_margins_for(
            pane,
            if overlay {
                PriceScaleTarget::Overlay
            } else {
                PriceScaleTarget::Right
            },
        )
    }

    pub fn price_scale_margins_for(
        &self,
        pane: usize,
        target: PriceScaleTarget,
    ) -> Option<(f64, f64)> {
        let margins = self.price_scale_for(pane, target)?.options().scale_margins;
        Some((margins.top, margins.bottom))
    }

    pub fn set_price_scale_margins(&mut self, pane: usize, overlay: bool, top: f64, bottom: f64) {
        self.set_price_scale_margins_for(
            pane,
            if overlay {
                PriceScaleTarget::Overlay
            } else {
                PriceScaleTarget::Right
            },
            top,
            bottom,
        );
    }

    pub fn set_price_scale_margins_for(
        &mut self,
        pane: usize,
        target: PriceScaleTarget,
        top: f64,
        bottom: f64,
    ) {
        if !top.is_finite()
            || !bottom.is_finite()
            || top < 0.0
            || bottom < 0.0
            || top > 1.0
            || bottom > 1.0
            || top + bottom > 1.0
        {
            return;
        }
        if let Some(scale) = self.price_scale_for_mut(pane, target) {
            scale.set_scale_margins(top, bottom);
        }
    }

    pub fn price_scale_mode(&self, pane: usize, overlay: bool) -> Option<PriceScaleMode> {
        self.price_scale_mode_for(
            pane,
            if overlay {
                PriceScaleTarget::Overlay
            } else {
                PriceScaleTarget::Right
            },
        )
    }

    pub fn price_scale_mode_for(
        &self,
        pane: usize,
        target: PriceScaleTarget,
    ) -> Option<PriceScaleMode> {
        Some(self.price_scale_for(pane, target)?.mode())
    }

    pub fn set_price_scale_mode(&mut self, pane: usize, overlay: bool, mode: PriceScaleMode) {
        self.set_price_scale_mode_for(
            pane,
            if overlay {
                PriceScaleTarget::Overlay
            } else {
                PriceScaleTarget::Right
            },
            mode,
        );
    }

    pub fn set_price_scale_mode_for(
        &mut self,
        pane: usize,
        target: PriceScaleTarget,
        mode: PriceScaleMode,
    ) {
        if let Some(scale) = self.price_scale_for_mut(pane, target) {
            scale.set_mode(mode);
        }
    }

    pub fn set_series_price_scale(&mut self, id: SeriesId, target: PriceScaleTarget) {
        if let Some(series) = self.series.iter_mut().find(|series| series.id == id) {
            series.overlay = target == PriceScaleTarget::Overlay;
            series.left_scale = target == PriceScaleTarget::Left;
        }
    }

    /// Merge a snake_case JSON patch of price-scale options into one pane scale (the engine
    /// backing of the TS `priceScale.applyOptions`; unknown keys are ignored gracefully).
    /// Keys: `mode` (0 normal, 1 log, 2 percentage, 3 indexed-to-100), `auto_scale`,
    /// `invert_scale`, `scale_margins` (`{top, bottom}`, each optional), `align_labels`,
    /// `ticks_visible`, `entire_text_only`, `minimum_width`, `text_color` (string, `""` or
    /// `null` clears back to `layout.textColor`). Returns false for a malformed patch or an
    /// unknown pane/target.
    pub fn price_scale_apply_options_json(
        &mut self,
        pane: usize,
        target: PriceScaleTarget,
        json: &str,
    ) -> bool {
        let Ok(serde_json::Value::Object(patch)) = serde_json::from_str::<serde_json::Value>(json)
        else {
            return false;
        };
        let Some(scale) = self.price_scale_for_mut(pane, target) else {
            return false;
        };
        let flag = |key: &str| patch.get(key).and_then(serde_json::Value::as_bool);
        let finite = |key: &str| {
            patch
                .get(key)
                .and_then(serde_json::Value::as_f64)
                .filter(|v| v.is_finite())
        };
        if let Some(mode) = patch.get("mode").and_then(serde_json::Value::as_u64) {
            scale.set_mode(match mode {
                1 => PriceScaleMode::Logarithmic,
                2 => PriceScaleMode::Percentage,
                3 => PriceScaleMode::IndexedTo100,
                _ => PriceScaleMode::Normal,
            });
        }
        if let Some(auto) = flag("auto_scale") {
            scale.set_auto_scale(auto);
        }
        if let Some(inverted) = flag("invert_scale") {
            scale.set_invert_scale(inverted);
        }
        if let Some(margins) = patch
            .get("scale_margins")
            .and_then(serde_json::Value::as_object)
        {
            let current = scale.options().scale_margins;
            let top = margins
                .get("top")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(current.top);
            let bottom = margins
                .get("bottom")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(current.bottom);
            // Same contract as `set_price_scale_margins_for`: fractions in 0..=1 summing
            // to at most 1; an out-of-contract patch leaves the margins untouched.
            if top.is_finite()
                && bottom.is_finite()
                && top >= 0.0
                && bottom >= 0.0
                && top <= 1.0
                && bottom <= 1.0
                && top + bottom <= 1.0
            {
                scale.set_scale_margins(top, bottom);
            }
        }
        if let Some(align) = flag("align_labels") {
            scale.set_align_labels(align);
        }
        if let Some(visible) = flag("ticks_visible") {
            scale.set_ticks_visible(visible);
        }
        if let Some(entire) = flag("entire_text_only") {
            scale.set_entire_text_only(entire);
        }
        if let Some(width) = finite("minimum_width") {
            scale.set_minimum_width(width);
        }
        if let Some(value) = patch.get("text_color") {
            if value.is_null() {
                scale.set_text_color(None);
            } else if let Some(css) = value.as_str() {
                scale.set_text_color((!css.is_empty()).then(|| css.to_string()));
            }
        }
        if let Some(value) = flag("bold_round_labels") {
            scale.set_bold_round_labels(value);
        }
        true
    }

    /// One pane scale's full options as a snake_case JSON object (reference `priceScale.options()`
    /// shape): `mode`, `auto_scale`, `invert_scale`, `scale_margins`, and the label
    /// cosmetics (`align_labels`, `ticks_visible`, `entire_text_only`, `minimum_width`,
    /// `text_color`). `None` for an unknown pane/target.
    pub fn price_scale_options_json(
        &self,
        pane: usize,
        target: PriceScaleTarget,
    ) -> Option<String> {
        let scale = self.price_scale_for(pane, target)?;
        let options = scale.options();
        Some(
            serde_json::json!({
                "mode": match options.mode {
                    PriceScaleMode::Logarithmic => 1,
                    PriceScaleMode::Percentage => 2,
                    PriceScaleMode::IndexedTo100 => 3,
                    PriceScaleMode::Normal => 0,
                },
                "auto_scale": options.auto_scale,
                "invert_scale": options.invert_scale,
                "scale_margins": {
                    "top": options.scale_margins.top,
                    "bottom": options.scale_margins.bottom,
                },
                "align_labels": options.align_labels,
                "ticks_visible": options.ticks_visible,
                "entire_text_only": options.entire_text_only,
                "minimum_width": options.minimum_width,
                "text_color": options.text_color,
                "bold_round_labels": options.bold_round_labels,
            })
            .to_string(),
        )
    }

    pub fn series_price_scale(&self, id: SeriesId) -> Option<(usize, PriceScaleTarget)> {
        self.series
            .iter()
            .find(|series| series.id == id)
            .map(|series| {
                let target = if series.overlay {
                    PriceScaleTarget::Overlay
                } else if series.left_scale {
                    PriceScaleTarget::Left
                } else {
                    PriceScaleTarget::Right
                };
                (series.pane_index, target)
            })
    }

    /// First close at or to the right of the visible left edge, matching reference series first-value
    /// selection for percentage/indexed coordinate modes. Whitespace rows are skipped (the reference's
    /// plot list never contains them, so its first-value search lands on a real bar). A custom
    /// series' rows are time-only, so its first value is the host-recorded frame value (the
    /// plugin's `priceValueBuilder` current value of the first visible non-whitespace item —
    /// reference `firstValue` reads the custom plot row's Close slot).
    pub(crate) fn series_base_value(&self, id: SeriesId, visible_from: i64) -> Option<f64> {
        let series = self.series.iter().find(|s| s.id == id)?;
        if series.kind == SeriesKind::Custom {
            return series
                .custom_frame
                .first_value
                .filter(|value| value.is_finite());
        }
        let plot = self.data.plot(id);
        let row = plot.first_non_whitespace_row(visible_from)?;
        let value = plot.value_at(row, PlotValueIndex::Close);
        value.is_finite().then_some(value)
    }

    pub(crate) fn visible_series_base_value(&self, id: SeriesId) -> Option<f64> {
        let from = self.time_scale.visible_strict_range()?.left();
        self.series_base_value(id, from)
    }
}
