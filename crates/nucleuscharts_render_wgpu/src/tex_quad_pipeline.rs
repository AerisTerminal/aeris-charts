//! Textured-quad pipeline for atlas labels. Glyph runs are rasterized by the host (Canvas2D)
//! **in the run's own color** — Chrome's glyph anti-aliasing is color-dependent (sRGB mask
//! gamma), so a white-on-transparent raster tinted at draw time visibly diverges from direct
//! `fillText` for non-white text. The shader folds the straight-alpha texel into premultiplied
//! form and multiplies by the instance tint (white for text), reproducing the browser's blend
//! exactly. Nearest sampling: labels are drawn 1:1 at integer bitmap positions, matching
//! Canvas2D `fillText` crispness.

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TexQuadInstance {
    /// x, y, w, h in bitmap pixels.
    pub rect: [f32; 4],
    /// u0, v0, u1, v1 normalized atlas coords.
    pub uv: [f32; 4],
    /// Straight RGBA tint in 0..1.
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    viewport: [f32; 2],
    _pad: [f32; 2],
}

const SHADER: &str = r#"
struct Globals {
    viewport: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var atlas_tex: texture_2d<f32>;
@group(1) @binding(1) var atlas_samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(
    @builtin(vertex_index) vi: u32,
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) color: vec4<f32>,
) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let c = corners[vi];
    let px = rect.xy + c * rect.zw;
    let ndc = vec2<f32>(
        px.x / globals.viewport.x * 2.0 - 1.0,
        1.0 - px.y / globals.viewport.y * 2.0,
    );
    var out: VsOut;
    out.pos = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = mix(uv.xy, uv.zw, c);
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Texels are straight-alpha browser rasters in the run's color; fold to premultiplied and
    // tint. With the text path's white tint this is exactly the browser's source-over source.
    let t = textureSample(atlas_tex, atlas_samp, in.uv);
    let a = t.a * in.color.a;
    return vec4<f32>(t.rgb * in.color.rgb * a, a);
}
"#;

pub struct TexQuadRenderer {
    pipeline: wgpu::RenderPipeline,
    globals_buf: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    atlas_bg: wgpu::BindGroup,
}

impl TexQuadRenderer {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        atlas_view: &wgpu::TextureView,
        sample_count: u32,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tex_quad_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tex_quad_globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let globals_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tex_quad_globals_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tex_quad_globals_bg"),
            layout: &globals_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buf.as_entire_binding(),
            }],
        });

        let atlas_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tex_quad_atlas_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas_sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let atlas_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tex_quad_atlas_bg"),
            layout: &atlas_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tex_quad_layout"),
            bind_group_layouts: &[&globals_bgl, &atlas_bgl],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tex_quad_pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<TexQuadInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 16,
                            shader_location: 1,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 32,
                            shader_location: 2,
                        },
                    ],
                }],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: sample_count,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        Self {
            pipeline,
            globals_buf,
            globals_bg,
            atlas_bg,
        }
    }

    pub(crate) fn write_globals(&self, queue: &wgpu::Queue, width_px: u32, height_px: u32) {
        let globals = Globals {
            viewport: [width_px as f32, height_px as f32],
            _pad: [0.0, 0.0],
        };
        queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&globals));
    }

    pub(crate) fn draw<'p>(
        &'p self,
        pass: &mut wgpu::RenderPass<'p>,
        instances: &'p wgpu::Buffer,
        first: u32,
        count: u32,
    ) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.globals_bg, &[]);
        pass.set_bind_group(1, &self.atlas_bg, &[]);
        pass.set_vertex_buffer(0, instances.slice(..));
        pass.draw(0..6, first..first + count);
    }
}
