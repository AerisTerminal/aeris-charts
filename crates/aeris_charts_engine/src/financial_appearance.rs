//! Typed financial-chart appearance and theme provenance.

use crate::{ChartEngine, ChartTheme, SeriesId};
use aeris_charts_core::style::{
    DARK_BORDER_CSS, DARK_CROSSHAIR_LINE_CSS, DARK_FOREGROUND_CSS, DARK_MARKET_DOWN_CSS,
    DARK_MARKET_UP_CSS, DARK_SURFACE_CSS, LIGHT_BORDER_CSS, LIGHT_CROSSHAIR_LINE_CSS,
    LIGHT_FOREGROUND_CSS, LIGHT_MARKET_DOWN_CSS, LIGHT_MARKET_UP_CSS, LIGHT_SURFACE_CSS,
};

/// A color either follows its field's canonical semantic theme role or is explicitly pinned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppearanceColor {
    Theme,
    Custom(String),
}

impl AppearanceColor {
    fn custom_or(&self, themed: &str) -> String {
        match self {
            Self::Theme => themed.to_string(),
            Self::Custom(color) => color.clone(),
        }
    }

    fn override_value(&self) -> Option<String> {
        match self {
            Self::Theme => None,
            Self::Custom(color) => Some(color.clone()),
        }
    }
}

/// Canonical effective colors for the semantic roles exposed by financial appearance controls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FinancialThemeColors {
    pub background: &'static str,
    pub text: &'static str,
    pub grid: &'static str,
    pub crosshair: &'static str,
    pub bullish: &'static str,
    pub bearish: &'static str,
}

impl FinancialThemeColors {
    #[must_use]
    pub const fn for_theme(theme: ChartTheme) -> Self {
        match theme {
            ChartTheme::Light => Self {
                background: LIGHT_SURFACE_CSS,
                text: LIGHT_FOREGROUND_CSS,
                grid: LIGHT_BORDER_CSS,
                crosshair: LIGHT_CROSSHAIR_LINE_CSS,
                bullish: LIGHT_MARKET_UP_CSS,
                bearish: LIGHT_MARKET_DOWN_CSS,
            },
            ChartTheme::Dark => Self {
                background: DARK_SURFACE_CSS,
                text: DARK_FOREGROUND_CSS,
                grid: DARK_BORDER_CSS,
                crosshair: DARK_CROSSHAIR_LINE_CSS,
                bullish: DARK_MARKET_UP_CSS,
                bearish: DARK_MARKET_DOWN_CSS,
            },
        }
    }
}

/// Durable financial canvas and primary-series appearance with explicit theme provenance.
#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct FinancialAppearance {
    pub background_color: AppearanceColor,
    pub text_color: AppearanceColor,
    pub grid_visible: bool,
    pub grid_color: AppearanceColor,
    pub grid_style: u8,
    pub crosshair_color: AppearanceColor,
    pub crosshair_width: u8,
    pub crosshair_style: u8,
    pub up_color: AppearanceColor,
    pub down_color: AppearanceColor,
    pub wick_up_color: AppearanceColor,
    pub wick_down_color: AppearanceColor,
    pub border_up_color: AppearanceColor,
    pub border_down_color: AppearanceColor,
    pub wick_visible: bool,
    pub border_visible: bool,
    pub open_visible: bool,
    pub thin_bars: bool,
    /// `Theme` follows the series kind's default stroke.
    pub line_color: AppearanceColor,
    pub line_width: u8,
    pub line_style: u8,
    /// `Theme` follows the canonical fill gradient of the series' own stroke.
    pub area_top_color: AppearanceColor,
    pub area_bottom_color: AppearanceColor,
    /// `Theme` follows the market up/down default.
    pub baseline_top_color: AppearanceColor,
    pub baseline_bottom_color: AppearanceColor,
}

/// The CSS colors the series paints for its line, area and baseline controls, whether each is
/// pinned or follows its default. Hosts show these in swatches instead of re-deriving defaults.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinancialSeriesColors {
    pub line: String,
    pub area_top: String,
    pub area_bottom: String,
    pub baseline_top: String,
    pub baseline_bottom: String,
}

impl Default for FinancialAppearance {
    fn default() -> Self {
        Self {
            background_color: AppearanceColor::Theme,
            text_color: AppearanceColor::Theme,
            grid_visible: true,
            grid_color: AppearanceColor::Theme,
            grid_style: 2,
            crosshair_color: AppearanceColor::Theme,
            crosshair_width: 1,
            crosshair_style: 2,
            up_color: AppearanceColor::Theme,
            down_color: AppearanceColor::Theme,
            wick_up_color: AppearanceColor::Theme,
            wick_down_color: AppearanceColor::Theme,
            border_up_color: AppearanceColor::Theme,
            border_down_color: AppearanceColor::Theme,
            wick_visible: true,
            border_visible: true,
            open_visible: true,
            thin_bars: true,
            line_color: AppearanceColor::Theme,
            line_width: 2,
            line_style: 0,
            area_top_color: AppearanceColor::Theme,
            area_bottom_color: AppearanceColor::Theme,
            baseline_top_color: AppearanceColor::Theme,
            baseline_bottom_color: AppearanceColor::Theme,
        }
    }
}

impl FinancialAppearance {
    #[must_use]
    pub fn effective_background_color(&self, theme: ChartTheme) -> String {
        self.background_color
            .custom_or(FinancialThemeColors::for_theme(theme).background)
    }

    #[must_use]
    pub fn effective_text_color(&self, theme: ChartTheme) -> String {
        self.text_color
            .custom_or(FinancialThemeColors::for_theme(theme).text)
    }

    #[must_use]
    pub fn effective_grid_color(&self, theme: ChartTheme) -> String {
        self.grid_color
            .custom_or(FinancialThemeColors::for_theme(theme).grid)
    }

    #[must_use]
    pub fn effective_crosshair_color(&self, theme: ChartTheme) -> String {
        self.crosshair_color
            .custom_or(FinancialThemeColors::for_theme(theme).crosshair)
    }

    #[must_use]
    pub fn effective_up_color(&self, theme: ChartTheme) -> String {
        self.up_color
            .custom_or(FinancialThemeColors::for_theme(theme).bullish)
    }

    #[must_use]
    pub fn effective_down_color(&self, theme: ChartTheme) -> String {
        self.down_color
            .custom_or(FinancialThemeColors::for_theme(theme).bearish)
    }
}

impl ChartEngine {
    /// Typed appearance snapshot. Theme-following colors remain semantic rather than being
    /// flattened to whichever theme happened to be active when the host persisted the value.
    #[must_use]
    pub fn financial_appearance(&self, series_id: SeriesId) -> Option<FinancialAppearance> {
        let series = self
            .series
            .iter()
            .find(|series| series.id == series_id && !series.removed)?;
        let options = self.options.get();
        let canvas_color = |follows_theme: bool, color: &str| {
            if follows_theme {
                AppearanceColor::Theme
            } else {
                AppearanceColor::Custom(color.to_string())
            }
        };
        Some(FinancialAppearance {
            background_color: canvas_color(
                self.background_color_follows_theme,
                &options.layout.background.color,
            ),
            text_color: canvas_color(self.text_color_follows_theme, &options.layout.text_color),
            grid_visible: options.grid.vert_lines.visible && options.grid.horz_lines.visible,
            grid_color: canvas_color(
                self.grid_color_follows_theme,
                &options.grid.vert_lines.color,
            ),
            grid_style: options.grid.vert_lines.style.min(4),
            crosshair_color: canvas_color(
                self.crosshair_color_follows_theme,
                &options.crosshair.vert_line.color,
            ),
            crosshair_width: bounded_width(options.crosshair.vert_line.width),
            crosshair_style: options.crosshair.vert_line.style.min(4),
            up_color: color_provenance(&series.up_color),
            down_color: color_provenance(&series.down_color),
            wick_up_color: color_provenance(&series.wick_up_color),
            wick_down_color: color_provenance(&series.wick_down_color),
            border_up_color: color_provenance(&series.border_up_color),
            border_down_color: color_provenance(&series.border_down_color),
            wick_visible: series.wick_visible.unwrap_or(true),
            border_visible: series.border_visible.unwrap_or(true),
            open_visible: series.open_visible,
            thin_bars: series.thin_bars,
            line_color: color_provenance(&series.line_color),
            line_width: bounded_width(series.line_width.unwrap_or(crate::frame::LINE_WIDTH)),
            line_style: series.line_style.min(4),
            area_top_color: color_provenance(&series.area_top_color),
            area_bottom_color: color_provenance(&series.area_bottom_color),
            baseline_top_color: color_provenance(&series.top_line_color),
            baseline_bottom_color: color_provenance(&series.bottom_line_color),
        })
    }

    /// The colors the series currently paints for its line, area and baseline controls.
    #[must_use]
    pub fn financial_series_colors(&self, series_id: SeriesId) -> Option<FinancialSeriesColors> {
        let series = self
            .series
            .iter()
            .find(|series| series.id == series_id && !series.removed)?;
        let market = self.market_colors();
        let (area_top, area_bottom) = crate::frame::area_fill_colors(series, market);
        let (baseline_top, baseline_bottom) = crate::frame::baseline_line_colors(series, market);
        Some(FinancialSeriesColors {
            line: crate::frame::series_stroke_color(series, market).to_css(),
            area_top: area_top.to_css(),
            area_bottom: area_bottom.to_css(),
            baseline_top: baseline_top.to_css(),
            baseline_bottom: baseline_bottom.to_css(),
        })
    }

    /// Apply only canvas appearance, retaining typed follow-theme provenance.
    pub fn apply_financial_canvas_appearance(&mut self, appearance: &FinancialAppearance) -> bool {
        let before = self.financial_appearance(0);
        let colors = FinancialThemeColors::for_theme(self.theme);
        self.background_color_follows_theme =
            matches!(appearance.background_color, AppearanceColor::Theme);
        self.text_color_follows_theme = matches!(appearance.text_color, AppearanceColor::Theme);
        self.grid_color_follows_theme = matches!(appearance.grid_color, AppearanceColor::Theme);
        self.crosshair_color_follows_theme =
            matches!(appearance.crosshair_color, AppearanceColor::Theme);
        let background = appearance.background_color.custom_or(colors.background);
        let text = appearance.text_color.custom_or(colors.text);
        let patch = serde_json::json!({
            "layout": {
                "background": {
                    "type": "solid",
                    "color": background,
                    "topColor": background,
                    "bottomColor": background,
                },
                "textColor": text,
            },
            "leftPriceScale": { "textColor": text },
            "rightPriceScale": { "textColor": text },
            "grid": {
                "vertLines": {
                    "visible": appearance.grid_visible,
                    "color": appearance.grid_color.custom_or(colors.grid),
                    "style": appearance.grid_style.min(4),
                },
                "horzLines": {
                    "visible": appearance.grid_visible,
                    "color": appearance.grid_color.custom_or(colors.grid),
                    "style": appearance.grid_style.min(4),
                }
            },
            "crosshair": {
                "vertLine": {
                    "color": appearance.crosshair_color.custom_or(colors.crosshair),
                    "width": appearance.crosshair_width.clamp(1, 4),
                    "style": appearance.crosshair_style.min(4),
                },
                "horzLine": {
                    "color": appearance.crosshair_color.custom_or(colors.crosshair),
                    "width": appearance.crosshair_width.clamp(1, 4),
                    "style": appearance.crosshair_style.min(4),
                }
            }
        });
        self.options.apply(&patch);
        self.route_price_scale_patch(&patch);
        self.invalidate_frame_all();
        before != self.financial_appearance(0)
    }

    /// Apply only primary-series appearance without JSON sentinels or host-side field knowledge.
    pub fn apply_financial_series_appearance(
        &mut self,
        series_id: SeriesId,
        appearance: &FinancialAppearance,
    ) -> bool {
        let before = self.financial_appearance(series_id);
        let Some(series) = self
            .series
            .iter_mut()
            .find(|series| series.id == series_id && !series.removed)
        else {
            return false;
        };
        series.up_color = appearance.up_color.override_value();
        series.down_color = appearance.down_color.override_value();
        series.wick_up_color = appearance.wick_up_color.override_value();
        series.wick_down_color = appearance.wick_down_color.override_value();
        series.border_up_color = appearance.border_up_color.override_value();
        series.border_down_color = appearance.border_down_color.override_value();
        series.wick_visible = Some(appearance.wick_visible);
        series.border_visible = Some(appearance.border_visible);
        series.open_visible = appearance.open_visible;
        series.thin_bars = appearance.thin_bars;
        series.line_color = appearance.line_color.override_value();
        series.line_width = Some(f64::from(appearance.line_width.clamp(1, 4)));
        series.line_style = appearance.line_style.min(4);
        series.area_top_color = appearance.area_top_color.override_value();
        series.area_bottom_color = appearance.area_bottom_color.override_value();
        series.top_line_color = appearance.baseline_top_color.override_value();
        series.bottom_line_color = appearance.baseline_bottom_color.override_value();
        self.invalidate_frame_all();
        before != self.financial_appearance(series_id)
    }
}

fn color_provenance(color: &Option<String>) -> AppearanceColor {
    color.as_ref().map_or(AppearanceColor::Theme, |color| {
        AppearanceColor::Custom(color.clone())
    })
}

fn bounded_width(width: f64) -> u8 {
    if !width.is_finite() {
        return 1;
    }
    width.round().clamp(1.0, 4.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_theme_provenance_survives_theme_changes() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let mut appearance = chart.financial_appearance(0).unwrap();
        appearance.grid_color = AppearanceColor::Custom("#123456".into());
        appearance.crosshair_color = AppearanceColor::Theme;
        assert!(chart.apply_financial_canvas_appearance(&appearance));

        chart.set_theme(ChartTheme::Light);
        let light = chart.financial_appearance(0).unwrap();
        assert_eq!(light.grid_color, AppearanceColor::Custom("#123456".into()));
        assert_eq!(light.crosshair_color, AppearanceColor::Theme);
        assert_eq!(
            chart.options.get().crosshair.vert_line.color,
            FinancialThemeColors::for_theme(ChartTheme::Light).crosshair
        );

        chart.set_theme(ChartTheme::Dark);
        assert_eq!(chart.options.get().grid.vert_lines.color, "#123456");
        assert_eq!(
            chart.options.get().crosshair.vert_line.color,
            FinancialThemeColors::for_theme(ChartTheme::Dark).crosshair
        );
    }

    #[test]
    fn custom_background_and_text_survive_theme_changes_until_reset() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let mut appearance = chart.financial_appearance(0).unwrap();
        appearance.background_color = AppearanceColor::Custom("#101820".into());
        appearance.text_color = AppearanceColor::Custom("#f0e68c".into());
        assert!(chart.apply_financial_canvas_appearance(&appearance));

        for theme in [ChartTheme::Light, ChartTheme::Dark] {
            chart.set_theme(theme);
            let options = chart.options.get();
            assert_eq!(options.layout.background.color, "#101820");
            assert_eq!(options.layout.background.bottom_color, "#101820");
            assert_eq!(options.layout.text_color, "#f0e68c");
            assert_eq!(
                chart.panes[0].price_scale.options().text_color.as_deref(),
                Some("#f0e68c")
            );
            let typed = chart.financial_appearance(0).unwrap();
            assert_eq!(
                typed.background_color,
                AppearanceColor::Custom("#101820".into())
            );
            assert_eq!(typed.text_color, AppearanceColor::Custom("#f0e68c".into()));
        }

        appearance.background_color = AppearanceColor::Theme;
        appearance.text_color = AppearanceColor::Theme;
        assert!(chart.apply_financial_canvas_appearance(&appearance));
        chart.set_theme(ChartTheme::Light);
        let light = FinancialThemeColors::for_theme(ChartTheme::Light);
        assert_eq!(
            chart.options.get().layout.background.color,
            light.background
        );
        assert_eq!(chart.options.get().layout.text_color, light.text);
        assert_eq!(
            chart.panes[0].price_scale.options().text_color.as_deref(),
            Some(light.text)
        );

        chart
            .apply_options(
                r##"{"layout":{"background":{"color":"#222222"},"textColor":"#eeeeee"}}"##,
            )
            .unwrap();
        chart.reset_style_to_theme_defaults(ChartTheme::Dark);
        let typed = chart.financial_appearance(0).unwrap();
        assert_eq!(typed.background_color, AppearanceColor::Theme);
        assert_eq!(typed.text_color, AppearanceColor::Theme);
        chart.set_theme(ChartTheme::Light);
        assert_eq!(
            chart.options.get().layout.background.color,
            light.background
        );
    }

    #[test]
    fn typed_series_theme_colors_clear_overrides_without_json_sentinels() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let mut appearance = chart.financial_appearance(0).unwrap();
        appearance.up_color = AppearanceColor::Custom("#abcdef".into());
        appearance.wick_up_color = AppearanceColor::Custom("#fedcba".into());
        assert!(chart.apply_financial_series_appearance(0, &appearance));
        assert_eq!(
            chart.financial_appearance(0).unwrap().up_color,
            AppearanceColor::Custom("#abcdef".into())
        );

        appearance.up_color = AppearanceColor::Theme;
        appearance.wick_up_color = AppearanceColor::Theme;
        assert!(chart.apply_financial_series_appearance(0, &appearance));
        let restored = chart.financial_appearance(0).unwrap();
        assert_eq!(restored.up_color, AppearanceColor::Theme);
        assert_eq!(restored.wick_up_color, AppearanceColor::Theme);
        assert!(
            chart
                .series
                .iter()
                .find(|series| series.id == 0)
                .unwrap()
                .up_color
                .is_none()
        );
    }

    #[test]
    fn default_line_area_and_baseline_colors_round_trip_without_pinning() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        chart.convert_series_kind(0, crate::SeriesKind::Area);
        let appearance = chart.financial_appearance(0).unwrap();
        for color in [
            &appearance.line_color,
            &appearance.area_top_color,
            &appearance.area_bottom_color,
            &appearance.baseline_top_color,
            &appearance.baseline_bottom_color,
        ] {
            assert_eq!(*color, AppearanceColor::Theme);
        }
        let painted = chart.financial_series_colors(0).unwrap();
        // Re-applying the reported appearance, as a saved layout does, keeps the translucent
        // default gradient instead of pinning an opaque fill.
        chart.apply_financial_series_appearance(0, &appearance);
        assert_eq!(chart.financial_series_colors(0).unwrap(), painted);
        let series = chart.series.iter().find(|series| series.id == 0).unwrap();
        assert!(series.line_color.is_none() && series.area_top_color.is_none());
        let stroke = crate::frame::series_stroke_color(series, chart.market_colors());
        let (strong, faint) = crate::frame::area_fill_gradient(stroke);
        assert_eq!(painted.line, stroke.to_css());
        assert_eq!(painted.area_top, strong.to_css());
        assert_eq!(painted.area_bottom, faint.to_css());
        assert!(strong.a() < 0xFF && faint.a() < strong.a());

        let mut custom = appearance;
        custom.line_color = AppearanceColor::Custom("#2962ff".into());
        custom.area_bottom_color = AppearanceColor::Custom("#2962ff00".into());
        assert!(chart.apply_financial_series_appearance(0, &custom));
        let typed = chart.financial_appearance(0).unwrap();
        assert_eq!(typed.area_top_color, AppearanceColor::Theme);
        assert_eq!(
            typed.area_bottom_color,
            AppearanceColor::Custom("#2962ff00".into())
        );
        let painted = chart.financial_series_colors(0).unwrap();
        let (strong, _) = crate::frame::area_fill_gradient(aeris_charts_render::color::Color::rgb(
            0x29, 0x62, 0xff,
        ));
        assert_eq!(painted.line, "#2962ff");
        assert_eq!(painted.area_top, strong.to_css());
        assert_eq!(painted.area_bottom, "rgba(41,98,255,0)");
    }

    #[test]
    fn raw_canvas_color_patch_is_explicit_and_is_not_retokenized() {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        chart
            .apply_options(r##"{"grid":{"vertLines":{"color":"#112233"}}}"##)
            .unwrap();
        chart.set_theme(ChartTheme::Light);
        assert_eq!(chart.options.get().grid.vert_lines.color, "#112233");
        assert!(matches!(
            chart.financial_appearance(0).unwrap().grid_color,
            AppearanceColor::Custom(_)
        ));
    }
}
