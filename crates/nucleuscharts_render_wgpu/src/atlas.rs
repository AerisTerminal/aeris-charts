//! Label atlas: whole label strings rasterized (by the host, e.g. Canvas2D) and shelf-packed
//! into one RGBA texture. Caching whole strings mirrors the reference's `TextWidthCache` granularity —
//! axis labels are few and short, so string-level caching beats per-glyph complexity.

use std::collections::HashMap;

pub const ATLAS_SIZE: u32 = 1024;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtlasSlot {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl AtlasSlot {
    /// Normalized UV rect [u0, v0, u1, v1].
    pub fn uv(&self) -> [f32; 4] {
        let s = ATLAS_SIZE as f32;
        [
            self.x as f32 / s,
            self.y as f32 / s,
            (self.x + self.w) as f32 / s,
            (self.y + self.h) as f32 / s,
        ]
    }
}

pub struct LabelAtlas {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    cursor_x: u32,
    cursor_y: u32,
    shelf_h: u32,
    entries: HashMap<String, AtlasSlot>,
    /// Bumped every full-atlas reset, so external slot holders (the host's text-run cache)
    /// can invalidate placements that point at reused texels.
    epoch: u64,
}

impl LabelAtlas {
    pub fn new(device: &wgpu::Device) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("label_atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            cursor_x: 0,
            cursor_y: 0,
            shelf_h: 0,
            entries: HashMap::new(),
            epoch: 0,
        }
    }

    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// Reset generation: changes every time [`Self::insert`] clears the full atlas.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn get(&self, key: &str) -> Option<AtlasSlot> {
        self.entries.get(key).copied()
    }

    /// Packs `pixels` (RGBA8, w*h*4 bytes) and uploads. Clears the whole atlas when full
    /// (rare for axis labels; entries simply re-rasterize on demand).
    pub fn insert(
        &mut self,
        queue: &wgpu::Queue,
        key: String,
        w: u32,
        h: u32,
        pixels: &[u8],
    ) -> AtlasSlot {
        debug_assert_eq!(pixels.len(), (w * h * 4) as usize);
        assert!(
            w <= ATLAS_SIZE && h <= ATLAS_SIZE,
            "label larger than atlas"
        );

        if self.cursor_x + w > ATLAS_SIZE {
            // new shelf
            self.cursor_x = 0;
            self.cursor_y += self.shelf_h;
            self.shelf_h = 0;
        }
        if self.cursor_y + h > ATLAS_SIZE {
            // atlas full: reset (entries re-rasterize lazily)
            self.entries.clear();
            self.cursor_x = 0;
            self.cursor_y = 0;
            self.shelf_h = 0;
            self.epoch += 1;
        }

        let slot = AtlasSlot {
            x: self.cursor_x,
            y: self.cursor_y,
            w,
            h,
        };
        self.cursor_x += w;
        self.shelf_h = self.shelf_h.max(h);

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: slot.x,
                    y: slot.y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );

        self.entries.insert(key, slot);
        slot
    }
}
