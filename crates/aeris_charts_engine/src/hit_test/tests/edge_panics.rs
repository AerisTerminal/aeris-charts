use super::*;

/// Regression for the panic at hit_test.rs:107 — eager `then_some(items[index + 1])` indexed
/// out of bounds whenever the hovered slot's candidate range included the LAST visible item,
/// and the abort left the wasm RefCell borrowed (the "crosshair stuck until reload" bug).
/// The fix is lazy `then(..)`; hovering the last visible bar must work on every kind.
#[test]
fn hovering_the_last_visible_bar_does_not_panic() {
    let chart = settled_candle_chart();
    let last_x = x_at(&chart, 9);
    let hit = chart.hit_test_series(last_x, y_at(&chart, 0, 10.5));
    assert_eq!(hit, Some(0));
    // One slot-width beyond the last bar is inside the tolerance sweep — must not panic either.
    let _ = chart.hit_test_series(last_x + 30.0, 250.0);

    let line = settled_line_chart(&[1.0, 2.0, 3.0, 4.0, 5.0]);
    let last_x = line.logical_to_coordinate(4.0).unwrap();
    let hit = line.hit_test_series(last_x, line.series_price_to_coordinate(0, 5.0).unwrap());
    assert_eq!(hit, Some(0));

    // Out-of-pane coordinates in every direction (dragged crosshair feeds clamped hits).
    for &(x, y) in &[
        (-500.0, 250.0),
        (10_000.0, 250.0),
        (250.0, -100.0),
        (250.0, 10_000.0),
        (1_000_000.0, 250.0),
    ] {
        let _ = chart.hit_test_series(x, y);
        let _ = chart.hit_test_one_series(0, x, y);
        let _ = line.hit_test_series(x, y);
        let _ = line.hit_test_one_series(0, x, y);
    }

    // Mid-drag states: scrolled past both edges into empty space.
    let mut chart = settled_candle_chart();
    chart.set_right_offset(100.0);
    chart.build_frame();
    let _ = chart.hit_test_series(10_000.0, 250.0);
    let _ = chart.hit_test_series(250.0, 250.0);
    chart.set_right_offset(-100.0);
    chart.build_frame();
    let _ = chart.hit_test_series(250.0, 250.0);
    let _ = chart.hit_test_series(10_000.0, 250.0);
}
