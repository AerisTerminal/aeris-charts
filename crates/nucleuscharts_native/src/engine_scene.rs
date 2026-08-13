//! Deterministic real-engine fixture used by native golden tests and examples.

use nucleuscharts_core::options::ChartTheme;
use nucleuscharts_engine::ChartEngine;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct ParityFixture {
    pub schema: u32,
    pub name: String,
    pub css_width: f64,
    pub css_height: f64,
    pub pixel_ratio: f64,
    pub price_axis_width: f64,
    pub time_axis_height: f64,
    pub bar_count: usize,
    pub end_time: i64,
    pub seed: u32,
    pub start_price: f64,
    pub close_span: f64,
    pub wick_span: f64,
}

pub fn parity_fixture() -> ParityFixture {
    let fixture: ParityFixture = serde_json::from_str(include_str!(
        "../../../examples/web_demo/fixtures/d1/candles.json"
    ))
    .expect("D1 parity fixture JSON must remain valid");
    assert_eq!(fixture.schema, 1, "unsupported D1 parity fixture schema");
    fixture
}

pub fn parity_engine() -> ChartEngine {
    let fixture = parity_fixture();
    let mut times = Vec::with_capacity(fixture.bar_count);
    let mut open = Vec::with_capacity(fixture.bar_count);
    let mut high = Vec::with_capacity(fixture.bar_count);
    let mut low = Vec::with_capacity(fixture.bar_count);
    let mut close = Vec::with_capacity(fixture.bar_count);
    let mut seed = fixture.seed;
    let mut random = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        seed as f64 / u32::MAX as f64
    };
    let mut price = fixture.start_price;
    let start = fixture.end_time - (fixture.bar_count.saturating_sub(1) as i64) * 3_600;
    for index in 0..fixture.bar_count {
        let o = price;
        let c = (o + (random() - 0.5) * fixture.close_span).max(1.0);
        times.push((start + index as i64 * 3_600) as f64);
        open.push(o);
        high.push(o.max(c) + random() * fixture.wick_span);
        low.push(o.min(c) - random() * fixture.wick_span);
        close.push(c);
        price = c;
    }

    let mut chart = ChartEngine::new(fixture.css_width, fixture.css_height, fixture.pixel_ratio);
    chart.set_theme(ChartTheme::Light);
    chart
        .options
        .apply_str(
            r##"{
                "layout":{"background":{"type":"solid","color":"#ffffff"},"textColor":"#191919"},
                "grid":{"vertLines":{"color":"#d6dcde","visible":true},"horzLines":{"color":"#d6dcde","visible":true}},
                "leftPriceScale":{"borderColor":"#2b2b43","textColor":"#191919","boldRoundLabels":false},
                "rightPriceScale":{"borderColor":"#2b2b43","textColor":"#191919","boldRoundLabels":false},
                "timeScale":{"borderColor":"#2b2b43"}
            }"##,
        )
        .expect("shared D1 fixture options are valid");
    chart
        .set_series_data(0, &times, &open, &high, &low, &close)
        .expect("shared D1 fixture data is valid");
    chart.series[0].up_color = Some("#089981".into());
    chart.series[0].down_color = Some("#f7525f".into());
    chart.series[0].wick_up_color = Some("#089981".into());
    chart.series[0].wick_down_color = Some("#f7525f".into());
    chart.series[0].border_up_color = Some("#089981".into());
    chart.series[0].border_down_color = Some("#f7525f".into());
    chart.pane_w = fixture.css_width - fixture.price_axis_width;
    chart.pane_h = fixture.css_height - fixture.time_axis_height;
    chart.axis_w = fixture.price_axis_width;
    chart.layout_panes(chart.pane_h);
    chart.time_scale.set_width(chart.pane_w);
    chart.fit_content();
    chart
}

pub fn demo_engine() -> ChartEngine {
    let mut chart = ChartEngine::new(480.0, 300.0, 1.0);
    let mut times = Vec::with_capacity(64);
    let mut open = Vec::with_capacity(64);
    let mut high = Vec::with_capacity(64);
    let mut low = Vec::with_capacity(64);
    let mut close = Vec::with_capacity(64);
    let mut price = 100.0;
    for i in 0..64 {
        let o = price;
        let delta = ((i * 17 % 11) as f64 - 5.0) * 0.22;
        let c = (o + delta).max(1.0);
        times.push(i as f64);
        open.push(o);
        high.push(o.max(c) + 0.7 + (i % 3) as f64 * 0.1);
        low.push(o.min(c) - 0.7 - (i % 2) as f64 * 0.1);
        close.push(c);
        price = c;
    }
    chart
        .set_series_data(0, &times, &open, &high, &low, &close)
        .expect("fixture data is valid");
    chart.time_scale.set_width(480.0);
    chart.fit_content();
    chart
}
