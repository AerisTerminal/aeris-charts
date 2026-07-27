//! Renders the reference chart scene through the Canvas2D executor + tiny-skia target and saves a
//! PNG. Proves the Prim IR rasterizes correctly off-GPU (golden/SSR path).
//!
//! Run: `cargo run -p origin_native --example scene -- out.png`

use origin_native::{render_prims, scene::demo_scene};

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "scene.png".to_string());
    let s = demo_scene();
    let canvas = render_prims(s.width, s.height, s.background, &s.prims, &s.points);
    canvas.save_png(&out).expect("save png");
    println!(
        "wrote {out} ({}x{}, {} prims)",
        s.width,
        s.height,
        s.prims.len()
    );
}
