//! Integration coverage for [`ChartLanguage`]: switching the chart language re-titles
//! engine-generated text, localizes legends and drawing schemas, installs the built-in
//! month-name tables, and never touches host-supplied strings.

use crate::{ChartEngine, ChartLanguage, DrawingKind, FinancialLegendRequest};

fn settled_chart() -> ChartEngine {
    let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
    let times = (0..10).map(|i| (i * 3600) as f64).collect::<Vec<_>>();
    let values = [11.0, 12.0, 11.0, 10.0, 11.0, 12.0, 13.0, 12.0, 11.0, 10.0];
    chart
        .set_series_data(0, &times, &values, &values, &values, &values)
        .unwrap();
    chart.time_scale.set_width(800.0);
    chart.fit_content();
    chart.build_frame();
    chart
}

#[test]
fn language_switch_retitles_engine_generated_indicator_titles() {
    let mut chart = settled_chart();
    let momentum = chart.add_momentum(0, 10).unwrap();
    assert_eq!(chart.series_entry(momentum).unwrap().title, "Momentum 10");
    chart.set_language(ChartLanguage::Russian);
    assert_eq!(chart.series_entry(momentum).unwrap().title, "Моментум 10");
    chart.set_language(ChartLanguage::English);
    assert_eq!(chart.series_entry(momentum).unwrap().title, "Momentum 10");
}

#[test]
fn host_overridden_indicator_title_survives_language_switches() {
    let mut chart = settled_chart();
    let momentum = chart.add_momentum(0, 10).unwrap();
    chart.series_entry_mut(momentum).unwrap().title = "My study".to_string();
    chart.set_language(ChartLanguage::Russian);
    assert_eq!(chart.series_entry(momentum).unwrap().title, "My study");
    chart.set_language(ChartLanguage::English);
    assert_eq!(chart.series_entry(momentum).unwrap().title, "My study");
}

#[test]
fn multi_output_indicator_titles_and_labels_follow_the_language() {
    let mut chart = settled_chart();
    let outputs = chart.add_aroon(0, 14);
    assert_eq!(outputs.len(), 2);
    let english = chart.indicator_info(outputs[0]).unwrap();
    // English keeps the persisted output identifier as the display label.
    assert_eq!(english.output_label, english.output_name);
    chart.set_language(ChartLanguage::Russian);
    let russian = chart.indicator_info(outputs[0]).unwrap();
    assert_eq!(russian.output_label, "Арун вверх");
    assert_eq!(russian.output_name, english.output_name);
    assert_eq!(
        chart.series_entry(outputs[0]).unwrap().title,
        "Арун вверх 14"
    );
    assert_eq!(
        chart.series_entry(outputs[1]).unwrap().title,
        "Арун вниз 14"
    );
}

#[test]
fn language_switch_installs_builtin_month_tables_and_repeating_it_is_a_noop() {
    let mut chart = settled_chart();
    assert_eq!(chart.month_names.short[0], "Jan");
    chart.set_language(ChartLanguage::Russian);
    assert_eq!(chart.month_names.short[0], "Янв");
    assert_eq!(chart.month_names.long[0], "Январь");
    let custom: [String; 12] = std::array::from_fn(|index| format!("M{index}"));
    chart.set_month_names(custom.clone(), custom);
    // An unchanged language must not touch a host-installed month table.
    chart.set_language(ChartLanguage::Russian);
    assert_eq!(chart.month_names.short[0], "M0");
    chart.set_language(ChartLanguage::English);
    assert_eq!(chart.month_names.short[0], "Jan");
}

#[test]
fn financial_legend_ohlc_labels_follow_the_language() {
    let mut chart = settled_chart();
    chart.set_language(ChartLanguage::Russian);
    let legend = chart.financial_legend(FinancialLegendRequest {
        logical_index: None,
        primary_title: "BTC",
        show_primary_ohlc: true,
        primary_values_series: None,
        leading_series: &[],
        trailing_series: &[],
    });
    let row = legend
        .iter()
        .find(|row| row.identity == crate::FinancialLegendIdentity::Primary)
        .unwrap();
    let prefixes: Vec<&str> = row
        .values
        .iter()
        .map(|value| value.text.split(' ').next().unwrap())
        .collect();
    assert_eq!(prefixes, ["Откр", "Макс", "Мин", "Закр"]);
}

#[test]
fn drawing_schema_labels_come_in_the_requested_language() {
    let english = crate::drawing_property_schema(DrawingKind::TrendLine, ChartLanguage::English);
    let russian = crate::drawing_property_schema(DrawingKind::TrendLine, ChartLanguage::Russian);
    fn label<'a>(schema: &'a crate::DrawingPropertySchema, name: &str) -> &'a str {
        schema
            .properties
            .iter()
            .find(|property| property.name == name)
            .unwrap()
            .label
            .as_str()
    }
    assert_eq!(label(&english, "color"), "Line");
    assert_eq!(label(&russian, "color"), "Линия");
    assert_eq!(label(&russian, "width"), "Толщина");
    assert_eq!(label(&russian, "visible"), "Видимость");
    // Anchor labels localize; property names stay the persisted identifiers.
    assert_eq!(russian.anchors[0].label, "Точка 1");
    assert_eq!(english.anchors[0].label, "Point 1");
}

#[test]
fn restored_indicator_titles_follow_the_target_charts_language() {
    let mut source = settled_chart();
    source.add_momentum(0, 10);
    let document = source.export_state_json().unwrap();
    let mut restored = settled_chart();
    restored.set_language(ChartLanguage::Russian);
    restored.import_state_json(&document).unwrap();
    let binding = &restored.indicator_bindings()[0];
    assert_eq!(
        restored.series_entry(binding.outputs[0]).unwrap().title,
        "Моментум 10"
    );
}

#[cfg(feature = "complete")]
#[test]
fn statistics_sample_counts_follow_language_plural_rules() {
    use crate::text::samples_count;
    assert_eq!(samples_count(ChartLanguage::English, 1), "1 sample");
    assert_eq!(samples_count(ChartLanguage::English, 3), "3 samples");
    assert_eq!(samples_count(ChartLanguage::Russian, 21), "21 наблюдение");
    assert_eq!(samples_count(ChartLanguage::Russian, 3), "3 наблюдения");
    assert_eq!(samples_count(ChartLanguage::Russian, 12), "12 наблюдений");
}
