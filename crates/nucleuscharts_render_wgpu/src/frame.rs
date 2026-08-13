//! Frame composition: one MSAA render pass drawing scissored groups of triangle meshes and
//! solid/textured quads in the frame's prim order.
//!
//! Groups replicate the reference's pane/axis canvas separation: the pane group is scissored to the
//! pane rect. 4x MSAA smooths diagonal line edges while leaving pixel-aligned rects and
//! texture-alpha text bit-identical (their edges never straddle a pixel boundary), so
//! candles and labels stay crisp.
//!
//! Execution order matches the Canvas2D executor exactly: within a layer, prims paint in
//! list order (a marker emitted after the candles covers the wicks on both backends). The
//! group therefore keeps one vertex/instance buffer per pipeline plus a run-length schedule
//! ([`DrawRun`]) — one draw call per maximal run of the same pipeline, so a candle block of
//! thousands of quads still costs a single instanced draw.

use nucleuscharts_render::draw_list::Prim;

use crate::gpu_timer::{FrameTimestamps, GpuTimer};
use crate::quad_executor::prim_to_instances;
use crate::quad_pipeline::{QuadInstance, QuadRenderer};
use crate::tex_quad_pipeline::{TexQuadInstance, TexQuadRenderer};
use crate::tri_executor::geom_prim_to_tris;
use crate::tri_pipeline::{TriRenderer, TriVertex};

pub const SAMPLE_COUNT: u32 = 4;

/// The pipeline a [`DrawRun`] draws with; selects which group buffer the run indexes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunPipeline {
    /// Triangle mesh (fills, strokes, markers, background gradient); `first`/`count` are vertices.
    Tri,
    /// Solid instanced quads (rects, grid, candles, crosshair); `first`/`count` are instances.
    Quad,
    /// Textured instanced quads (label atlas); `first`/`count` are instances.
    TexQuad,
}

/// One draw call: `count` elements starting at `first` in the pipeline's group buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawRun {
    pub pipeline: RunPipeline,
    pub first: u32,
    pub count: u32,
}

#[derive(Default)]
pub struct DrawGroup {
    /// x, y, w, h in bitmap px; None = full target.
    pub scissor: Option<[u32; 4]>,
    /// Triangle-mesh vertices in prim order across all layers (under, then main, then top).
    pub tris: Vec<TriVertex>,
    /// Solid-quad instances in prim order across all layers.
    pub quads: Vec<QuadInstance>,
    /// Textured-quad instances (browser-rasterized text runs). Scheduled in prim order by
    /// [`prims_to_group`]; a buffer populated without any [`RunPipeline::TexQuad`] run keeps
    /// the previous whole-buffer, drawn-last behavior.
    pub tex_quads: Vec<TexQuadInstance>,
    /// Run-length draw schedule over `tris`/`quads`/`tex_quads`, in Canvas2D paint order.
    pub runs: Vec<DrawRun>,
}

impl DrawGroup {
    /// Reset the geometry and schedule for reuse next frame (keeps the allocations).
    pub fn clear(&mut self) {
        self.tris.clear();
        self.quads.clear();
        self.tex_quads.clear();
        self.runs.clear();
    }
}

/// Record a run covering `[first, first + count)` of `pipeline`'s buffer, merging with the
/// previous run when it is the immediately preceding range of the same pipeline (run-length
/// batching: consecutive same-family prims stay one draw call).
fn push_run(runs: &mut Vec<DrawRun>, pipeline: RunPipeline, first: u32, count: u32) {
    if count == 0 {
        return;
    }
    if let Some(last) = runs.last_mut() {
        if last.pipeline == pipeline && last.first + last.count == first {
            last.count += count;
            return;
        }
    }
    runs.push(DrawRun {
        pipeline,
        first,
        count,
    });
}

/// Append one layer's prims to the group in list order, exactly as the Canvas2D executor
/// would paint them: each rect-family prim (`Rect`/`RectFrame`/`HLine`/`VLine`) extends the
/// quad buffer, every other geometry prim extends the tri buffer, and each maximal
/// same-pipeline run records one [`DrawRun`]. `Text` prims resolve through `resolve_text` —
/// the host seam that maps a run to its browser-rasterized atlas quad (the IR/wgpu crates
/// stay DOM-free). A resolved text quad schedules in prim order like everything else; a
/// `None` (empty run, raster failure, oversized run) collapses the slot without splitting
/// the surrounding runs, exactly like a degenerate rect.
pub fn prims_to_group(
    prims: &[Prim],
    points: &[[f32; 2]],
    group: &mut DrawGroup,
    resolve_text: &mut dyn FnMut(&Prim) -> Option<TexQuadInstance>,
) {
    for prim in prims {
        match prim {
            Prim::Rect { .. }
            | Prim::RectFrame { .. }
            | Prim::HLine { .. }
            | Prim::VLine { .. } => {
                let first = group.quads.len() as u32;
                prim_to_instances(prim, &mut group.quads);
                push_run(
                    &mut group.runs,
                    RunPipeline::Quad,
                    first,
                    group.quads.len() as u32 - first,
                );
            }
            Prim::Text { .. } => {
                if let Some(instance) = resolve_text(prim) {
                    let first = group.tex_quads.len() as u32;
                    group.tex_quads.push(instance);
                    push_run(&mut group.runs, RunPipeline::TexQuad, first, 1);
                }
            }
            _ => {
                let first = group.tris.len() as u32;
                geom_prim_to_tris(prim, points, &mut group.tris);
                push_run(
                    &mut group.runs,
                    RunPipeline::Tri,
                    first,
                    group.tris.len() as u32 - first,
                );
            }
        }
    }
}

/// The MSAA color target; recreated on resize.
pub struct MsaaTarget {
    view: wgpu::TextureView,
    width: u32,
    height: u32,
}

impl MsaaTarget {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("msaa_target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: SAMPLE_COUNT,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            view,
            width,
            height,
        }
    }

    /// Recreates the target if the size changed.
    pub fn ensure(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) {
        if self.width != width || self.height != height {
            *self = Self::new(device, format, width, height);
        }
    }

    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }
}

struct GroupBuffers {
    tris: Option<wgpu::Buffer>,
    quads: Option<wgpu::Buffer>,
    tex: Option<wgpu::Buffer>,
}

/// Encode and submit one frame. Returns the number of draw calls issued, which the host
/// surfaces as `frame_stats().draw_calls`. `timer` opts into GPU-side pass timing when the
/// device supports `timestamp-query`; `None` skips the timestamp plumbing entirely.
#[allow(clippy::too_many_arguments)]
pub fn render_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    msaa_view: &wgpu::TextureView,
    resolve_view: &wgpu::TextureView,
    width_px: u32,
    height_px: u32,
    clear_color: wgpu::Color,
    quad: &QuadRenderer,
    tex: &TexQuadRenderer,
    tri: &TriRenderer,
    groups: &[DrawGroup],
    timer: Option<&GpuTimer>,
) -> u32 {
    let timestamps = FrameTimestamps::new(timer);
    let mut draw_calls = 0u32;
    quad.write_globals(queue, width_px, height_px);
    tex.write_globals(queue, width_px, height_px);
    tri.write_globals(queue, width_px, height_px);

    // Keep uploads on the WebGPU-native queue path. `DeviceExt::create_buffer_init` maps every
    // buffer at creation, which browser software adapters may reject even for tiny buffers.
    let vbuf = |contents: &[u8], label| {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: contents.len() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&buffer, 0, contents);
        buffer
    };

    // buffers must outlive the pass; draw counts come from the runs
    let buffers: Vec<GroupBuffers> = groups
        .iter()
        .map(|g| GroupBuffers {
            tris: (!g.tris.is_empty()).then(|| vbuf(bytemuck::cast_slice(&g.tris), "tris")),
            quads: (!g.quads.is_empty()).then(|| vbuf(bytemuck::cast_slice(&g.quads), "quads")),
            tex: (!g.tex_quads.is_empty()).then(|| vbuf(bytemuck::cast_slice(&g.tex_quads), "tex")),
        })
        .collect();

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("frame"),
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("frame_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: msaa_view,
                resolve_target: Some(resolve_view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear_color),
                    // resolve target holds the result; MSAA buffer itself can be discarded
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: timestamps.pass_writes(),
            occlusion_query_set: None,
        });

        for (group, bufs) in groups.iter().zip(&buffers) {
            let [sx, sy, sw, sh] = match group.scissor {
                Some([x, y, w, h]) => {
                    let x = x.min(width_px);
                    let y = y.min(height_px);
                    [x, y, w.min(width_px - x), h.min(height_px - y)]
                }
                None => [0, 0, width_px, height_px],
            };
            if sw == 0 || sh == 0 {
                continue;
            }
            pass.set_scissor_rect(sx, sy, sw, sh);

            // One draw call per scheduled run, in Canvas2D paint order.
            for run in &group.runs {
                match run.pipeline {
                    RunPipeline::Tri => {
                        if let Some(b) = &bufs.tris {
                            tri.draw(&mut pass, b, run.first, run.count);
                            draw_calls += 1;
                        }
                    }
                    RunPipeline::Quad => {
                        if let Some(b) = &bufs.quads {
                            quad.draw(&mut pass, b, run.first, run.count);
                            draw_calls += 1;
                        }
                    }
                    RunPipeline::TexQuad => {
                        if let Some(b) = &bufs.tex {
                            tex.draw(&mut pass, b, run.first, run.count);
                            draw_calls += 1;
                        }
                    }
                }
            }
            // A directly populated tex buffer with no scheduled runs keeps the previous
            // whole-buffer, drawn-last behavior (textured quads paint above everything).
            if !group.tex_quads.is_empty()
                && !group
                    .runs
                    .iter()
                    .any(|run| run.pipeline == RunPipeline::TexQuad)
            {
                if let Some(b) = &bufs.tex {
                    tex.draw(&mut pass, b, 0, group.tex_quads.len() as u32);
                    draw_calls += 1;
                }
            }
        }
    }

    timestamps.resolve_into_staging(&mut encoder);
    queue.submit(Some(encoder.finish()));
    timestamps.begin_readback();
    draw_calls
}
