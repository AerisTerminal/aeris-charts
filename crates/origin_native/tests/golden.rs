//! Golden-image regression test (roadmap Phase D1).
//!
//! Re-renders the reference scene and diffs it against the committed golden PNG. This protects
//! the render path (executor + rasterizer) against regressions: any change that alters the output
//! fails here until the golden is deliberately regenerated (`cargo run -p origin_native --example
//! scene -- crates/origin_native/tests/goldens/scene.png`).
//!
//! The golden is currently our own deterministic render; when a headless-Chromium reference
//! pipeline exists, the reference charting library's PNGs drop in as additional goldens with the same diff.

use origin_native::{
    diff_pixmaps,
    engine_scene::{demo_engine, parity_engine},
    load_png, render_engine, render_prims,
    scene::demo_scene,
};

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/goldens/scene.png");
const ENGINE_GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/goldens/engine.png");

#[test]
fn scene_matches_golden() {
    let s = demo_scene();
    let canvas = render_prims(s.width, s.height, s.background, &s.prims, &s.points);
    let golden = load_png(GOLDEN).expect("committed golden PNG should load");

    // Same machine + deterministic CPU rasterizer => exact. Allow a hair of per-channel tolerance
    // and a tiny differing-pixel budget so a tiny-skia patch bump doesn't spuriously fail CI.
    let stats = diff_pixmaps(canvas.pixmap(), &golden, 2).expect("golden and render are same size");
    assert!(
        stats.fraction() < 0.001,
        "render drifted from golden: {} / {} px differ (max channel delta {}). \
         If intentional, regenerate the golden.",
        stats.differing_pixels,
        stats.total_pixels,
        stats.max_channel_delta,
    );
}

#[test]
fn diff_detects_a_changed_scene() {
    // Sanity: a modified scene must diff against the golden (guards against a no-op comparison).
    let s = demo_scene();
    let mut prims = s.prims.clone();
    prims.truncate(prims.len().saturating_sub(3)); // drop the marker + price line + polyline
    let changed = render_prims(s.width, s.height, s.background, &prims, &s.points);
    let golden = load_png(GOLDEN).unwrap();
    let stats = diff_pixmaps(changed.pixmap(), &golden, 2).unwrap();
    assert!(
        stats.differing_pixels > 0,
        "a changed scene should differ from the golden"
    );
}

#[test]
fn real_engine_frame_matches_golden() {
    let mut chart = demo_engine();
    let canvas = render_engine(&mut chart);
    let golden = load_png(ENGINE_GOLDEN).expect("real-engine golden PNG should load");
    let stats = diff_pixmaps(canvas.pixmap(), &golden, 2).expect("engine golden size must match");
    assert!(
        stats.fraction() < 0.001,
        "real engine frame drifted: {} / {} px differ (max {})",
        stats.differing_pixels,
        stats.total_pixels,
        stats.max_channel_delta
    );
}

#[test]
fn shared_browser_native_fixture_has_expected_pane_bitmap() {
    let mut chart = parity_engine();
    let canvas = render_engine(&mut chart);
    assert_eq!(
        (canvas.pixmap().width(), canvas.pixmap().height()),
        (1833, 1038)
    );
}
