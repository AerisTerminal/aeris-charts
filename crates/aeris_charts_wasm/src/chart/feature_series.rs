//! Thin browser boundary for engine-owned advanced series.

use super::*;
use aeris_charts_engine::{
    FeatureDataPoint, FeatureSeriesOptionsPatch, FeatureValue, HeatmapCell, StackedAreaColor,
};

fn property(value: &JsValue, key: &str) -> Option<JsValue> {
    js_sys::Reflect::get(value, &JsValue::from_str(key))
        .ok()
        .filter(|value| !value.is_undefined() && !value.is_null())
}

fn number(value: &JsValue, key: &str) -> Option<f64> {
    property(value, key)?.as_f64()
}

fn color(value: &JsValue, key: &str) -> Option<Color> {
    Color::parse_css(&property(value, key)?.as_string()?)
}

fn numbers(value: &JsValue, key: &str) -> Option<Vec<f64>> {
    let array = js_sys::Array::from(&property(value, key)?);
    Some(
        array
            .iter()
            .map(|value| value.as_f64().unwrap_or(f64::NAN))
            .collect(),
    )
}

fn has_any(value: &JsValue, keys: &[&str]) -> bool {
    keys.iter().any(|key| property(value, key).is_some())
}

fn parse_feature_value(kind: FeatureSeriesKind, item: &JsValue) -> Option<FeatureValue> {
    let payload_present = match kind {
        FeatureSeriesKind::PrettyHistogram | FeatureSeriesKind::BackgroundShade => {
            has_any(item, &["value"])
        }
        FeatureSeriesKind::GroupedBars
        | FeatureSeriesKind::StackedArea
        | FeatureSeriesKind::StackedBars => has_any(item, &["values"]),
        FeatureSeriesKind::Heatmap => has_any(item, &["cells"]),
        FeatureSeriesKind::HlcArea => has_any(item, &["high", "low", "close"]),
        FeatureSeriesKind::WhiskerBox => has_any(item, &["quartiles", "outliers"]),
    };
    if !payload_present {
        return None;
    }
    Some(match kind {
        FeatureSeriesKind::GroupedBars => {
            let values = numbers(item, "values").unwrap_or_default();
            FeatureValue::GroupedBars { values }
        }
        FeatureSeriesKind::Heatmap => {
            let cells = property(item, "cells")
                .map(|value| js_sys::Array::from(&value))
                .unwrap_or_default();
            FeatureValue::Heatmap {
                cells: cells
                    .iter()
                    .map(|cell| HeatmapCell {
                        low: number(&cell, "low").unwrap_or(f64::NAN),
                        high: number(&cell, "high").unwrap_or(f64::NAN),
                        amount: number(&cell, "amount").unwrap_or(f64::NAN),
                        color: color(&cell, "color"),
                    })
                    .collect(),
            }
        }
        FeatureSeriesKind::HlcArea => FeatureValue::HlcArea {
            high: number(item, "high").unwrap_or(f64::NAN),
            low: number(item, "low").unwrap_or(f64::NAN),
            close: number(item, "close")?,
        },
        FeatureSeriesKind::PrettyHistogram => FeatureValue::PrettyHistogram {
            value: number(item, "value").unwrap_or(f64::NAN),
            color: color(item, "color"),
        },
        FeatureSeriesKind::BackgroundShade => FeatureValue::BackgroundShade {
            value: number(item, "value").unwrap_or(f64::NAN),
        },
        FeatureSeriesKind::StackedArea => {
            let values = numbers(item, "values").unwrap_or_default();
            FeatureValue::StackedArea { values }
        }
        FeatureSeriesKind::StackedBars => {
            let values = numbers(item, "values").unwrap_or_default();
            FeatureValue::StackedBars { values }
        }
        FeatureSeriesKind::WhiskerBox => {
            let quartiles = numbers(item, "quartiles").unwrap_or_default();
            let quartiles = if quartiles.len() == 5 {
                [
                    quartiles[0],
                    quartiles[1],
                    quartiles[2],
                    quartiles[3],
                    quartiles[4],
                ]
            } else {
                [f64::NAN; 5]
            };
            FeatureValue::WhiskerBox {
                quartiles,
                outliers: numbers(item, "outliers").unwrap_or_default(),
            }
        }
    })
}

fn set_property(object: &js_sys::Object, key: &str, value: impl Into<JsValue>) {
    let _ = js_sys::Reflect::set(object, &JsValue::from_str(key), &value.into());
}

fn number_array(values: &[f64]) -> js_sys::Array {
    values.iter().copied().map(JsValue::from_f64).collect()
}

fn feature_point_to_js(point: FeatureDataPoint) -> JsValue {
    let object = js_sys::Object::new();
    set_property(&object, "time", point.time);
    let Some(value) = point.value else {
        return object.into();
    };
    match value {
        FeatureValue::BackgroundShade { value } => set_property(&object, "value", value),
        FeatureValue::PrettyHistogram { value, color } => {
            set_property(&object, "value", value);
            if let Some(color) = color {
                set_property(&object, "color", color.to_css());
            }
        }
        FeatureValue::GroupedBars { values }
        | FeatureValue::StackedArea { values }
        | FeatureValue::StackedBars { values } => {
            set_property(&object, "values", number_array(&values));
        }
        FeatureValue::Heatmap { cells } => {
            let output = js_sys::Array::new();
            for cell in cells {
                let item = js_sys::Object::new();
                set_property(&item, "low", cell.low);
                set_property(&item, "high", cell.high);
                set_property(&item, "amount", cell.amount);
                output.push(&item);
            }
            set_property(&object, "cells", output);
        }
        FeatureValue::HlcArea { high, low, close } => {
            set_property(&object, "high", high);
            set_property(&object, "low", low);
            set_property(&object, "close", close);
        }
        FeatureValue::WhiskerBox {
            quartiles,
            outliers,
        } => {
            set_property(&object, "quartiles", number_array(&quartiles));
            set_property(&object, "outliers", number_array(&outliers));
        }
    }
    object.into()
}

fn json_number(value: &serde_json::Value, key: &str) -> Option<f64> {
    value.get(key)?.as_f64()
}

fn json_color(value: &serde_json::Value, key: &str) -> Option<Color> {
    Color::parse_css(value.get(key)?.as_str()?)
}

fn json_colors(value: &serde_json::Value, key: &str) -> Option<Vec<Color>> {
    let colors = value
        .get(key)?
        .as_array()?
        .iter()
        .filter_map(|value| value.as_str().and_then(Color::parse_css))
        .collect::<Vec<_>>();
    (!colors.is_empty()).then_some(colors)
}

fn parse_options(json: &str) -> FeatureSeriesOptionsPatch {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return FeatureSeriesOptionsPatch::default();
    };
    let stacked_area_colors = value
        .get("colors")
        .and_then(serde_json::Value::as_array)
        .map(|colors| {
            colors
                .iter()
                .filter_map(|entry| {
                    Some(StackedAreaColor {
                        line: json_color(entry, "line")?,
                        area: json_color(entry, "area")?,
                    })
                })
                .collect::<Vec<_>>()
        })
        .filter(|colors| !colors.is_empty());
    FeatureSeriesOptionsPatch {
        color: json_color(&value, "color"),
        colors: json_colors(&value, "colors"),
        stacked_area_colors,
        line_color: json_color(&value, "line_color"),
        top_color: json_color(&value, "top_color"),
        bottom_color: json_color(&value, "bottom_color"),
        line_width: json_number(&value, "line_width"),
        base_price: json_number(&value, "base_price"),
        cell_border_width: json_number(&value, "cell_border_width"),
        cell_border_color: json_color(&value, "cell_border_color"),
        high_line_color: json_color(&value, "high_line_color"),
        low_line_color: json_color(&value, "low_line_color"),
        close_line_color: json_color(&value, "close_line_color"),
        area_top_color: json_color(&value, "area_top_color"),
        area_bottom_color: json_color(&value, "area_bottom_color"),
        high_line_width: json_number(&value, "high_line_width"),
        low_line_width: json_number(&value, "low_line_width"),
        close_line_width: json_number(&value, "close_line_width"),
        width_percent: json_number(&value, "width_percent"),
        radius: json_number(&value, "radius"),
        low_color: json_color(&value, "low_color"),
        high_color: json_color(&value, "high_color"),
        low_value: json_number(&value, "low_value"),
        high_value: json_number(&value, "high_value"),
        opacity: json_number(&value, "opacity"),
        whisker_color: json_color(&value, "whisker_color"),
        lower_quartile_fill: json_color(&value, "lower_quartile_fill"),
        upper_quartile_fill: json_color(&value, "upper_quartile_fill"),
        outlier_color: json_color(&value, "outlier_color"),
    }
}

impl ChartInner {
    pub(super) fn add_feature_series(
        &mut self,
        kind: u8,
        adopt_primary: bool,
        options_json: &str,
    ) -> u32 {
        let Some(kind) = FeatureSeriesKind::from_u8(kind) else {
            return u32::MAX;
        };
        let options = parse_options(options_json);
        if adopt_primary {
            self.engine.configure_feature_series(0, kind, options);
            0
        } else {
            self.engine.add_feature_series(kind, options)
        }
    }

    pub(super) fn set_feature_series_data(
        &mut self,
        id: u32,
        items: js_sys::Array,
    ) -> Option<String> {
        let Some(kind) = self.engine.feature_series_kind(id) else {
            return Some(rejected_diagnostics_json(
                "unknown or non-feature series id",
            ));
        };
        let input = items
            .iter()
            .map(|item| FeatureDataPoint {
                time: number(&item, "time").unwrap_or(f64::NAN),
                value: parse_feature_value(kind, &item),
            })
            .collect();
        match self.engine.set_feature_series_data(id, input) {
            Ok(report) => validation_diagnostics_json(&report),
            Err(error) => Some(rejected_validation_diagnostics_json(error)),
        }
    }

    pub(super) fn update_feature_series_item(&mut self, id: u32, item: JsValue) -> Option<String> {
        let Some(kind) = self.engine.feature_series_kind(id) else {
            return Some(rejected_diagnostics_json(
                "unknown or non-feature series id",
            ));
        };
        let point = FeatureDataPoint {
            time: number(&item, "time").unwrap_or(f64::NAN),
            value: parse_feature_value(kind, &item),
        };
        match self.engine.update_feature_series_data(id, point) {
            Ok(report) => validation_diagnostics_json(&report),
            Err(error) => Some(rejected_validation_diagnostics_json(error)),
        }
    }

    pub(super) fn feature_series_data(&self, id: u32) -> JsValue {
        let Some(points) = self.engine.feature_series_data(id) else {
            return JsValue::NULL;
        };
        points
            .into_iter()
            .map(feature_point_to_js)
            .collect::<js_sys::Array>()
            .into()
    }

    pub(super) fn feature_series_data_by_index(
        &self,
        id: u32,
        index: f64,
        mismatch: i8,
    ) -> JsValue {
        if !index.is_finite() || index.fract() != 0.0 {
            return JsValue::NULL;
        }
        self.engine
            .feature_series_data_by_index(id, index as i64, mismatch_direction_from_i8(mismatch))
            .map(feature_point_to_js)
            .unwrap_or(JsValue::NULL)
    }

    pub(super) fn apply_feature_series_options(&mut self, id: u32, options_json: &str) -> bool {
        self.engine
            .apply_feature_series_options(id, parse_options(options_json))
    }
}
