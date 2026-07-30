//! Rolling last-frame telemetry behind the public `chart_api.frame_stats()`.
//!
//! Design constraint from the consumer: collection must cost nothing measurable when nobody
//! reads it. So this is a **fixed-size record of the last frame plus two lifetime counters** —
//! there is no history buffer, no allocation per frame, and no string formatting. The per-frame
//! cost is two `performance.now()` reads and a handful of integer stores.
//!
//! GPU timing is the one part that is not free (a query set, a resolve, and an async buffer
//! readback per sampled frame), so it is *armed lazily*: the first `frame_stats()` read flips
//! [`FrameTelemetry::stats_requested`], and only from the next frame on does the WebGPU path
//! attach timestamp writes. A chart nobody instruments never creates a query set at all.
//!
//! Fields are `Cell`s because the Canvas2D axis-overlay pass paints through `&self`.

// The record's own logic is host-testable (see the tests below), but its writers all live in the
// wasm-only `chart` module, so on the native test build the setters read as dead code.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::cell::Cell;

/// `f64` slots written by `OriginChart::frame_stats_into`. The TypeScript façade owns one
/// scratch `Float64Array` of this length and re-reads it every frame, so a `frame_stats()` call
/// allocates nothing on either side of the boundary.
pub const FRAME_STATS_LEN: usize = 8;

/// Slot indices in the `frame_stats_into` buffer. Kept in lockstep with `read_frame_stats` in
/// `packages/charts/src/impl.ts` — append only, never reorder (the package pins `^0.8`).
pub mod slot {
    pub const CPU_MS: usize = 0;
    /// `f64::NAN` encodes the public `gpu_ms: null`.
    pub const GPU_MS: usize = 1;
    pub const DRAW_CALLS: usize = 2;
    pub const DROPPED_FRAMES: usize = 3;
    pub const PRESENTED_FRAMES: usize = 4;
    pub const MEMORY_BYTES: usize = 5;
    pub const CANVAS2D_OPS: usize = 6;
    pub const RING_OVERRUNS: usize = 7;
}

#[derive(Default)]
pub struct FrameTelemetry {
    /// Wall time in ms for the last frame's layout + command encoding (host-side CPU cost).
    last_cpu_ms: Cell<f64>,
    /// Draw calls issued for the last frame. WebGPU: the pass's draw-call count. Canvas2D: the
    /// pane executor's paint-op count (the nearest equivalent — a Canvas2D "draw call" is a
    /// `fillRect`/`stroke`/`fill`/`fillText`).
    last_draw_calls: Cell<u32>,
    /// Canvas2D paint ops issued by the engine for the last frame: the axis/crosshair overlay
    /// plus, on the Canvas2D backend, the pane executor. This is the metric the consumer's
    /// "zero Canvas2D draw operations per frame" check reads.
    last_canvas2d_ops: Cell<u32>,
    /// Frames begun but not presented since chart create (surface acquisition timed out or the
    /// swapchain was unavailable, so the previous frame stayed on screen).
    dropped_frames: Cell<u32>,
    /// Frames presented since chart create, on whichever backend was active.
    presented_frames: Cell<u32>,
    /// Ring-source producer overruns since chart create (see `set_ring_source`). Stays 0 while
    /// no series has a ring bound — the slot is part of the wire layout either way.
    pub ring_overruns: Cell<u32>,
    /// Set by the first `frame_stats()` read; arms the WebGPU timestamp path.
    stats_requested: Cell<bool>,
}

impl FrameTelemetry {
    /// Record the CPU cost of the frame that just finished encoding.
    pub fn set_cpu_ms(&self, ms: f64) {
        self.last_cpu_ms.set(ms);
    }

    pub fn set_draw_calls(&self, calls: u32) {
        self.last_draw_calls.set(calls);
    }

    /// Zero the per-frame Canvas2D op counter at the top of a frame.
    pub fn reset_canvas2d_ops(&self) {
        self.last_canvas2d_ops.set(0);
    }

    /// Count `n` Canvas2D paint ops into the current frame.
    pub fn add_canvas2d_ops(&self, n: u32) {
        self.last_canvas2d_ops
            .set(self.last_canvas2d_ops.get().saturating_add(n));
    }

    pub fn count_presented(&self) {
        self.presented_frames
            .set(self.presented_frames.get().saturating_add(1));
    }

    pub fn count_dropped(&self) {
        self.dropped_frames
            .set(self.dropped_frames.get().saturating_add(1));
    }

    /// True once the host has read `frame_stats()` at least once — the gate for arming GPU
    /// timestamp collection.
    pub fn stats_requested(&self) -> bool {
        self.stats_requested.get()
    }

    /// Fill `out` with the current record. `gpu_ms` is `NaN` when unavailable (no WebGPU
    /// backend, no `timestamp-query`, or no readback resolved yet), which the façade maps to
    /// `null`. Marks stats as requested so the next frame samples the GPU.
    pub fn write_into(&self, out: &mut [f64], gpu_ms: Option<f64>) {
        self.stats_requested.set(true);
        if out.len() < FRAME_STATS_LEN {
            return;
        }
        out[slot::CPU_MS] = self.last_cpu_ms.get();
        out[slot::GPU_MS] = gpu_ms.unwrap_or(f64::NAN);
        out[slot::DRAW_CALLS] = f64::from(self.last_draw_calls.get());
        out[slot::DROPPED_FRAMES] = f64::from(self.dropped_frames.get());
        out[slot::PRESENTED_FRAMES] = f64::from(self.presented_frames.get());
        out[slot::MEMORY_BYTES] = wasm_memory_bytes();
        out[slot::CANVAS2D_OPS] = f64::from(self.last_canvas2d_ops.get());
        out[slot::RING_OVERRUNS] = f64::from(self.ring_overruns.get());
    }
}

/// Wasm linear memory currently reserved, in bytes. Read straight from the module's own memory
/// size — no JS round trip, so `frame_stats()` stays allocation-free. Off wasm (the native
/// clippy/test pass over this crate) there is no linear memory to report.
#[cfg(target_arch = "wasm32")]
fn wasm_memory_bytes() -> f64 {
    /// wasm pages are 64 KiB.
    const PAGE_BYTES: usize = 65_536;
    (core::arch::wasm32::memory_size(0) * PAGE_BYTES) as f64
}

#[cfg(not(target_arch = "wasm32"))]
fn wasm_memory_bytes() -> f64 {
    0.0
}

/// The page's high-resolution clock, resolved off the global object so this works in a `Window`
/// and in a `Worker` alike (the latter matters for offscreen rendering). `None` leaves frame CPU
/// timing at zero rather than failing a render.
pub fn performance() -> Option<web_sys::Performance> {
    use wasm_bindgen::{JsCast, JsValue};
    js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("performance"))
        .ok()
        .and_then(|value| value.dyn_into::<web_sys::Performance>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_ms_absent_is_encoded_as_nan() {
        let telemetry = FrameTelemetry::default();
        let mut out = [0.0; FRAME_STATS_LEN];
        telemetry.write_into(&mut out, None);
        assert!(out[slot::GPU_MS].is_nan());
    }

    #[test]
    fn reading_stats_arms_gpu_sampling() {
        let telemetry = FrameTelemetry::default();
        assert!(!telemetry.stats_requested());
        telemetry.write_into(&mut [0.0; FRAME_STATS_LEN], Some(1.5));
        assert!(telemetry.stats_requested());
    }

    #[test]
    fn counters_accumulate_and_canvas_ops_reset_per_frame() {
        let telemetry = FrameTelemetry::default();
        telemetry.count_presented();
        telemetry.count_presented();
        telemetry.count_dropped();
        telemetry.add_canvas2d_ops(3);
        telemetry.reset_canvas2d_ops();
        telemetry.add_canvas2d_ops(2);
        let mut out = [0.0; FRAME_STATS_LEN];
        telemetry.write_into(&mut out, Some(2.0));
        assert_eq!(out[slot::PRESENTED_FRAMES], 2.0);
        assert_eq!(out[slot::DROPPED_FRAMES], 1.0);
        assert_eq!(out[slot::CANVAS2D_OPS], 2.0);
        assert_eq!(out[slot::GPU_MS], 2.0);
    }

    /// A short buffer must not panic at the JS boundary (a stale façade could pass one).
    #[test]
    fn short_buffer_is_ignored() {
        let telemetry = FrameTelemetry::default();
        let mut out = [0.0; 2];
        telemetry.write_into(&mut out, None);
        assert_eq!(out, [0.0; 2]);
    }
}
