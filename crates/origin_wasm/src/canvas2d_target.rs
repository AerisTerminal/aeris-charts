//! Browser [`Canvas2d`] target: drives a real `CanvasRenderingContext2d` from the Prim-IR
//! executor (roadmap Phase D2). This is the in-browser fallback backend for machines without
//! WebGPU — the same draw list the wgpu path renders, issued as 2D canvas calls.

use origin_render::canvas2d::Canvas2d;
use origin_render::color::Color;
use origin_render::draw_list::TextAlign;
use wasm_bindgen::JsValue;
use web_sys::CanvasRenderingContext2d;

/// CSS color string that preserves alpha (unlike `Color::to_hex`, which drops it).
fn css(c: Color) -> String {
    c.to_css()
}

/// Wraps a 2D canvas context as a [`Canvas2d`] target. Errors from fallible context calls
/// (`arc`, `set_line_dash`, gradient construction) are swallowed — a bad draw call drops the
/// primitive rather than aborting the frame.
pub struct WasmCanvas2d<'a> {
    ctx: &'a CanvasRenderingContext2d,
    /// Paint ops issued through this target — the ones that put pixels on the canvas
    /// (`fillRect`/`stroke`/`fill`/`fillText`), not state setters or path building. Reported as
    /// part of `frame_stats().canvas2d_ops`.
    ops: u32,
}

impl<'a> WasmCanvas2d<'a> {
    pub fn new(ctx: &'a CanvasRenderingContext2d) -> Self {
        Self { ctx, ops: 0 }
    }

    /// Paint ops issued since construction.
    pub fn ops(&self) -> u32 {
        self.ops
    }
}

impl Canvas2d for WasmCanvas2d<'_> {
    fn save(&mut self) {
        self.ctx.save();
    }
    fn restore(&mut self) {
        self.ctx.restore();
    }
    fn clip_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.ctx.begin_path();
        self.ctx.rect(x as f64, y as f64, w as f64, h as f64);
        self.ctx.clip();
    }

    fn set_fill_solid(&mut self, color: Color) {
        self.ctx.set_fill_style_str(&css(color));
    }
    fn set_fill_vgradient(&mut self, y_top: f32, y_bottom: f32, top: Color, bottom: Color) {
        // A zero-length gradient is invalid; nudge the endpoint so it degenerates to a near-solid.
        let y1 = if (y_bottom - y_top).abs() < 1e-3 {
            y_top + 1.0
        } else {
            y_bottom
        };
        let grad = self
            .ctx
            .create_linear_gradient(0.0, y_top as f64, 0.0, y1 as f64);
        let _ = grad.add_color_stop(0.0, &css(top));
        let _ = grad.add_color_stop(1.0, &css(bottom));
        self.ctx.set_fill_style_canvas_gradient(&grad);
    }
    fn set_stroke(&mut self, color: Color) {
        self.ctx.set_stroke_style_str(&css(color));
    }
    fn set_line_width(&mut self, width: f32) {
        self.ctx.set_line_width(width as f64);
    }
    fn set_line_dash(&mut self, pattern: &[f32]) {
        let arr = js_sys::Array::new();
        for &seg in pattern {
            arr.push(&JsValue::from_f64(seg as f64));
        }
        let _ = self.ctx.set_line_dash(&arr);
    }

    fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.ops += 1;
        self.ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);
    }

    fn begin_path(&mut self) {
        self.ctx.begin_path();
    }
    fn move_to(&mut self, x: f32, y: f32) {
        self.ctx.move_to(x as f64, y as f64);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.ctx.line_to(x as f64, y as f64);
    }
    fn close_path(&mut self) {
        self.ctx.close_path();
    }
    fn arc(&mut self, cx: f32, cy: f32, r: f32, start: f32, end: f32) {
        let _ = self
            .ctx
            .arc(cx as f64, cy as f64, r as f64, start as f64, end as f64);
    }
    fn stroke(&mut self) {
        self.ops += 1;
        self.ctx.stroke();
    }
    fn fill(&mut self) {
        self.ops += 1;
        self.ctx.fill();
    }

    fn fill_text(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        font: &str,
        color: Color,
        align: TextAlign,
    ) {
        self.ctx.set_font(font);
        self.ctx.set_fill_style_str(&css(color));
        self.ctx.set_text_align(align.canvas_keyword());
        // House convention (axis labels): y is the vertical center of the run.
        self.ctx.set_text_baseline("middle");
        self.ops += 1;
        let _ = self.ctx.fill_text(text, x as f64, y as f64);
    }
}
