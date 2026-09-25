//! Render the shared D1 browser/native fixture through the native tiny-skia backend.
//!
//! Run: `cargo run -p aeris_charts_native --example parity_fixture -- native.png`

use aeris_charts_native::{engine_scene::parity_engine, render_engine};

fn main() {
    let output = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "native-parity.png".to_string());
    let mut chart = parity_engine();
    let canvas = render_engine(&mut chart);
    canvas.save_png(&output).expect("write native parity PNG");
    println!(
        "wrote {output} ({}x{})",
        canvas.pixmap().width(),
        canvas.pixmap().height()
    );
}
