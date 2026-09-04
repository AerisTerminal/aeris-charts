//! Pixel-parity harness: diff what **official GPUI actually rasterized** against Nucleus's existing
//! `nucleuscharts_native` rasterizer, fixture by fixture.
//!
//! GPUI owns presentation through `PlatformWindow::draw(&Scene)` and exposes no framebuffer readback,
//! so the pixels are obtained by capturing the probe window's client area through DWM
//! (`tools/capture_window.ps1`, PrintWindow + PW_RENDERFULLCONTENT). That is a read of the composed
//! window — it does not patch or fork GPUI, inject a render pass, share a device, or CPU-rasterize
//! the chart, so it is outside every §12 stop condition.
//!
//! Each fixture isolates one *cause*, so a residual is attributable rather than a single opaque
//! number: crisp rects (coordinates/colour/coverage — legitimately held to zero), tessellated
//! geometry (triangle-edge antialiasing), gradients (interpolation), text (glyph rasterization).
//!
//! ```text
//! cargo run --release -p nucleuscharts_render_gpui --features gpui-backend --example pixel_parity
//! ```
//!
//! Writes `<out>/<fixture>_gpui.png`, `_native.png`, and `_diff.png` per fixture, plus
//! `results.json` with exact diff counts and SHA-256 hashes for all three images. Environment knobs:
//! - `NUCLEUSCHARTS_PARITY_OUT`  — output directory (default `target/pixel_parity`).
//! - `NUCLEUSCHARTS_PARITY_TOL`  — per-channel tolerance for the diff (default 0 — exact).

use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{self, Receiver};

use gpui::{
    canvas, div, prelude::*, px, size, App, Bounds, Context, Entity, Render, Window, WindowBounds,
    WindowOptions,
};
use gpui_platform::application;
use nucleuscharts_render_gpui::fixtures::{self, Fixture};
use nucleuscharts_render_gpui::{GpuiChartRenderer, NucleusViewport};
use sha2::{Digest, Sha256};

/// A unique window title, so the capture script can find exactly this window.
const WINDOW_TITLE: &str = "nucleuscharts-pixel-parity-harness";

/// Frames to paint before capturing, so the swapchain has certainly presented the fixture.
const WARMUP_FRAMES: u64 = 12;

/// Where the harness is in the render/capture cycle for the current fixture.
///
/// The capture **must not** run on the UI thread: `PrintWindow` posts `WM_PRINT` to the target
/// window and waits for it to be serviced, so calling it from inside the paint callback deadlocks
/// against our own message loop. It therefore runs on a worker thread while the main thread keeps
/// painting the same fixture, which also guarantees the window still shows that fixture when DWM
/// composes the capture.
enum Phase {
    /// Painting the fixture; capture once the counter passes [`WARMUP_FRAMES`].
    Warmup(u64),
    /// A capture is in flight on a worker thread.
    Capturing(Receiver<String>),
}

struct Harness {
    fixtures: Vec<Fixture>,
    /// Index of the fixture currently being displayed.
    current: usize,
    renderer: GpuiChartRenderer,
    phase: Phase,
    out_dir: PathBuf,
    tolerance: u8,
    results: Vec<Row>,
    /// Set once the last fixture has been captured.
    done: bool,
}

struct Row {
    name: String,
    attribution: String,
    width: u32,
    height: u32,
    differing: u32,
    max_delta: u8,
    total: u32,
    note: String,
}

impl Harness {
    fn new(dpr: f32, out_dir: PathBuf, tolerance: u8) -> Self {
        Self {
            fixtures: fixtures::all(dpr),
            current: 0,
            renderer: GpuiChartRenderer::new(),
            phase: Phase::Warmup(0),
            out_dir,
            tolerance,
            results: Vec::new(),
            done: false,
        }
    }

    /// Rasterize the reference and kick off the window capture on a worker thread.
    fn spawn_capture(&mut self) -> Receiver<String> {
        let (tx, rx) = mpsc::channel();
        let f = &self.fixtures[self.current];
        let gpui_png = self.out_dir.join(format!("{}_gpui.png", f.name));
        let native_png = self.out_dir.join(format!("{}_native.png", f.name));

        // Reference: Nucleus's existing native rasterizer over the identical prim list.
        let canvas = nucleuscharts_native::render_prims(
            f.width,
            f.height,
            f.background,
            &f.prims,
            &f.points,
        );
        if let Err(e) = canvas.save_png(native_png.to_str().unwrap_or_default()) {
            let _ = tx.send(format!("ERR native-render: {e}"));
            return rx;
        }

        // GPUI: read back what the window actually presented, off the UI thread.
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tools/capture_window.ps1");
        let out = gpui_png.clone();
        std::thread::spawn(move || {
            let output = Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                    script.to_str().unwrap_or_default(),
                    "-Title",
                    WINDOW_TITLE,
                    "-Out",
                    out.to_str().unwrap_or_default(),
                ])
                .output();
            let note = match &output {
                Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
                Err(e) => format!("ERR capture-spawn: {e}"),
            };
            let _ = tx.send(note);
        });
        rx
    }

    /// Diff the captured fixture against the reference and record the row.
    fn finish_capture(&mut self, note: String) {
        let f = &self.fixtures[self.current];
        let gpui_png = self.out_dir.join(format!("{}_gpui.png", f.name));
        let native_png = self.out_dir.join(format!("{}_native.png", f.name));
        let diff_png = self.out_dir.join(format!("{}_diff.png", f.name));
        if !note.starts_with("OK") {
            self.results.push(Row {
                name: f.name.into(),
                attribution: f.attribution.into(),
                width: f.width,
                height: f.height,
                differing: 0,
                max_delta: 0,
                total: 0,
                note: format!("capture failed: {note}"),
            });
            return;
        }

        // Diff with Nucleus's own comparator, at the requested tolerance.
        let (gpui_map, native_map) = (
            nucleuscharts_native::load_png(gpui_png.to_str().unwrap_or_default()),
            nucleuscharts_native::load_png(native_png.to_str().unwrap_or_default()),
        );
        let row = match (gpui_map, native_map) {
            (Ok(a), Ok(b)) => {
                if a.width() != b.width() || a.height() != b.height() {
                    Row {
                        name: f.name.into(),
                        attribution: f.attribution.into(),
                        width: f.width,
                        height: f.height,
                        differing: 0,
                        max_delta: 0,
                        total: 0,
                        note: format!(
                            "size mismatch: captured {}x{}, expected {}x{} — the window client area \
                             is not the fixture size, so no comparison was made",
                            a.width(),
                            a.height(),
                            b.width(),
                            b.height()
                        ),
                    }
                } else {
                    write_diff_image(&a, &b, &diff_png);
                    match nucleuscharts_native::diff_pixmaps(&a, &b, self.tolerance) {
                        Some(d) => Row {
                            name: f.name.into(),
                            attribution: f.attribution.into(),
                            width: a.width(),
                            height: a.height(),
                            differing: d.differing_pixels,
                            max_delta: d.max_channel_delta,
                            total: d.total_pixels,
                            note: String::new(),
                        },
                        None => Row {
                            name: f.name.into(),
                            attribution: f.attribution.into(),
                            width: f.width,
                            height: f.height,
                            differing: 0,
                            max_delta: 0,
                            total: 0,
                            note: "diff_pixmaps rejected the pair".into(),
                        },
                    }
                }
            }
            (a, b) => Row {
                name: f.name.into(),
                attribution: f.attribution.into(),
                width: f.width,
                height: f.height,
                differing: 0,
                max_delta: 0,
                total: 0,
                note: format!("png load failed: gpui={:?} native={:?}", a.err(), b.err()),
            },
        };
        self.results.push(row);
    }

    fn report(&self) {
        println!("\n=== nucleuscharts_render_gpui pixel parity ===");
        println!("reference: nucleuscharts_native (tiny-skia) over the identical Prim list");
        println!("gpui     : window client area captured through DWM (PrintWindow)");
        println!("tolerance: {} (per channel)\n", self.tolerance);
        println!(
            "{:<14} {:>11} {:>10} {:>9} {:>8}  attribution / note",
            "fixture", "size", "differing", "% of px", "maxdelta"
        );
        for r in &self.results {
            if !r.note.is_empty() {
                println!(
                    "{:<14} {:>11} {:>10} {:>9} {:>8}  {}",
                    r.name, "-", "-", "-", "-", r.note
                );
                continue;
            }
            let pct = if r.total == 0 {
                0.0
            } else {
                r.differing as f64 * 100.0 / r.total as f64
            };
            println!(
                "{:<14} {:>11} {:>10} {:>8.3}% {:>8}  {}",
                r.name,
                format!("{}x{}", r.width, r.height),
                r.differing,
                pct,
                r.max_delta,
                r.attribution
            );
        }
        println!("\nartifacts: {}", self.out_dir.display());

        // The crisp-rect fixture is the one that can legitimately be held to zero.
        if let Some(crisp) = self.results.iter().find(|r| r.name == "crisp_rects") {
            if crisp.note.is_empty() {
                println!(
                    "\ncrisp-rect gate: {} ({} differing pixels, max channel delta {})",
                    if crisp.differing == 0 { "PASS" } else { "FAIL" },
                    crisp.differing,
                    crisp.max_delta
                );
            } else {
                println!("\ncrisp-rect gate: NOT MEASURED — {}", crisp.note);
            }
        }
        write_results_json(&self.out_dir, &self.results);
    }
}

/// Write a human-visible diff image: differing pixels in magenta over a dimmed reference.
fn write_diff_image(a: &tiny_skia::Pixmap, b: &tiny_skia::Pixmap, path: &std::path::Path) {
    let Some(mut out) = tiny_skia::Pixmap::new(a.width(), a.height()) else {
        return;
    };
    let (pa, pb) = (a.data(), b.data());
    let dst = out.pixels_mut();
    for i in 0..(a.width() * a.height()) as usize {
        let differs = (0..4).any(|c| pa[i * 4 + c] != pb[i * 4 + c]);
        let px = if differs {
            tiny_skia::PremultipliedColorU8::from_rgba(0xff, 0x00, 0xff, 0xff)
        } else {
            let g = pb[i * 4] / 3 + 170;
            tiny_skia::PremultipliedColorU8::from_rgba(g, g, g, 0xff)
        };
        if let Some(px) = px {
            dst[i] = px;
        }
    }
    let _ = out.save_png(path);
}

fn sha256_file(path: &std::path::Path) -> String {
    let Ok(bytes) = std::fs::read(path) else {
        return String::new();
    };
    sha256_hex(&bytes)
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

/// Hash the decoded premultiplied RGBA bytes used by `diff_pixmaps`.
///
/// PNG-file hashes can differ for pixel-identical images because their encoders choose different
/// chunking/compression. This is the canonical visual hash required by the parity gate.
fn rgba_sha256_file(path: &std::path::Path) -> String {
    let Ok(pixmap) = nucleuscharts_native::load_png(path.to_str().unwrap_or_default()) else {
        return String::new();
    };
    sha256_hex(pixmap.data())
}

fn write_results_json(dir: &std::path::Path, rows: &[Row]) {
    let mut s = String::from("[\n");
    for (i, r) in rows.iter().enumerate() {
        let gpui_path = dir.join(format!("{}_gpui.png", r.name));
        let native_path = dir.join(format!("{}_native.png", r.name));
        let diff_path = dir.join(format!("{}_diff.png", r.name));
        let gpui_sha256 = sha256_file(&gpui_path);
        let native_sha256 = sha256_file(&native_path);
        let diff_sha256 = sha256_file(&diff_path);
        let gpui_rgba_sha256 = rgba_sha256_file(&gpui_path);
        let native_rgba_sha256 = rgba_sha256_file(&native_path);
        s.push_str(&format!(
            "  {{\"fixture\":\"{}\",\"attribution\":\"{}\",\"width\":{},\"height\":{},\
             \"differing_pixels\":{},\"max_channel_delta\":{},\"total_pixels\":{},\
             \"gpui_rgba_sha256\":\"{}\",\"native_rgba_sha256\":\"{}\",\
             \"gpui_png_sha256\":\"{}\",\"native_png_sha256\":\"{}\",\"diff_png_sha256\":\"{}\",\
             \"note\":\"{}\"}}{}\n",
            r.name,
            r.attribution,
            r.width,
            r.height,
            r.differing,
            r.max_delta,
            r.total,
            gpui_rgba_sha256,
            native_rgba_sha256,
            gpui_sha256,
            native_sha256,
            diff_sha256,
            r.note.replace('"', "'"),
            if i + 1 == rows.len() { "" } else { "," }
        ));
    }
    s.push_str("]\n");
    let _ = std::fs::write(dir.join("results.json"), s);
}

impl Render for Harness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.done {
            cx.quit();
            return div();
        }
        let entity: Entity<Harness> = cx.entity();
        window.request_animation_frame();

        div().size_full().child(
            canvas(
                move |bounds: Bounds<gpui::Pixels>, _window, _cx| bounds,
                move |bounds: Bounds<gpui::Pixels>, _prep, window, cx| {
                    entity.update(cx, |h: &mut Harness, cx| {
                        let sf = window.scale_factor();
                        // Paint the current fixture.
                        let f = &h.fixtures[h.current];
                        let plan_prims = f.prims.clone();
                        let plan_points = f.points.clone();
                        let viewport = NucleusViewport::from_bounds(
                            bounds.origin.x.into(),
                            bounds.origin.y.into(),
                            bounds.size.width.into(),
                            bounds.size.height.into(),
                        );
                        h.renderer
                            .paint_prims(&plan_prims, &plan_points, viewport, sf, window, cx);

                        // Advance the capture state machine. The main thread keeps painting the
                        // same fixture throughout, so the window content stays valid for DWM.
                        match &h.phase {
                            Phase::Warmup(n) if *n > WARMUP_FRAMES => {
                                let rx = h.spawn_capture();
                                h.phase = Phase::Capturing(rx);
                            }
                            Phase::Warmup(n) => h.phase = Phase::Warmup(n + 1),
                            Phase::Capturing(rx) => {
                                if let Ok(note) = rx.try_recv() {
                                    h.finish_capture(note);
                                    h.phase = Phase::Warmup(0);
                                    if h.current + 1 < h.fixtures.len() {
                                        h.current += 1;
                                    } else {
                                        h.report();
                                        h.done = true;
                                    }
                                }
                            }
                        }
                    });
                },
            )
            .size_full(),
        )
    }
}

fn main() {
    let out_dir = PathBuf::from(
        std::env::var("NUCLEUSCHARTS_PARITY_OUT").unwrap_or_else(|_| "target/pixel_parity".into()),
    );
    std::fs::create_dir_all(&out_dir).expect("output directory is creatable");
    let tolerance: u8 = std::env::var("NUCLEUSCHARTS_PARITY_TOL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    application().run(move |cx: &mut App| {
        // The window's client area must be exactly the fixture's logical size, so the captured
        // pixels are exactly the surface the adapter painted.
        let bounds = Bounds::centered(
            None,
            size(px(fixtures::LOGICAL_W), px(fixtures::LOGICAL_H)),
            cx,
        );
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some(WINDOW_TITLE.into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let scale = window.scale_factor();
                println!("harness: window scale factor {scale}");
                cx.new(|_| Harness::new(scale, out_dir.clone(), tolerance))
            },
        )
        .expect("the harness window opens");
        cx.activate(true);
    });
}
