//! Render a real `aeris_charts_engine::ChartEngine` fixture through the native backend.

use aeris_charts_native::{engine_scene::demo_engine, render_engine};

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "engine.png".to_string());
    let mut chart = demo_engine();
    let canvas = render_engine(&mut chart);
    canvas.save_png(&out).expect("save png");
    println!("wrote {out}");
}
