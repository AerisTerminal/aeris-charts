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
    entries: HashMap<String, AtlasSlot>,
    packer: AtlasPacker,
}

#[derive(Default)]
struct AtlasPacker {
    cursor_x: u32,
    cursor_y: u32,
    shelf_h: u32,
    epoch: u64,
    frame_insertions: u32,
    frame_overflowed: bool,
    reset_before_next_frame: bool,
}

impl AtlasPacker {
    fn begin_frame(&mut self) -> bool {
        let reset = self.reset_before_next_frame;
        if reset {
            self.reset();
            self.reset_before_next_frame = false;
        }
        self.frame_insertions = 0;
        self.frame_overflowed = false;
        reset
    }

    fn reset(&mut self) {
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.shelf_h = 0;
        self.epoch = self.epoch.wrapping_add(1);
    }

    fn allocate(&mut self, w: u32, h: u32) -> Option<AtlasSlot> {
        if self.cursor_x + w > ATLAS_SIZE {
            self.cursor_x = 0;
            self.cursor_y += self.shelf_h;
            self.shelf_h = 0;
        }
        if self.cursor_y + h > ATLAS_SIZE {
            if self.frame_insertions == 0 {
                self.reset();
            } else {
                self.frame_overflowed = true;
                self.reset_before_next_frame = true;
                return None;
            }
        }
        let slot = AtlasSlot {
            x: self.cursor_x,
            y: self.cursor_y,
            w,
            h,
        };
        self.cursor_x += w;
        self.shelf_h = self.shelf_h.max(h);
        self.frame_insertions += 1;
        Some(slot)
    }
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
            entries: HashMap::new(),
            packer: AtlasPacker::default(),
        }
    }

    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// Reset generation: changes every time [`Self::insert`] clears the full atlas.
    pub fn epoch(&self) -> u64 {
        self.packer.epoch
    }

    pub fn get(&self, key: &str) -> Option<AtlasSlot> {
        self.entries.get(key).copied()
    }

    /// Begin one submitted chart frame. A reset deferred by atlas pressure occurs before any
    /// quads for this frame are accepted, so no already-resolved quad can reference reused texels.
    pub fn begin_frame(&mut self) {
        if self.packer.begin_frame() {
            self.entries.clear();
        }
    }

    /// Whether every text run resolved during the current frame remains valid for submission.
    /// A false result tells the host to use its correct Canvas2D fallback for this frame.
    pub fn frame_valid(&self) -> bool {
        !self.packer.frame_overflowed
    }

    /// Record that the pending submission already contains retained quads referencing this
    /// epoch. Atlas exhaustion must then defer reset rather than overwrite those texels.
    pub fn protect_retained_frame_slots(&mut self) {
        self.packer.frame_insertions = self.packer.frame_insertions.max(1);
    }

    /// Packs `pixels` (RGBA8, w*h*4 bytes) and uploads. A full atlas resets immediately only
    /// before this frame references a slot; otherwise the reset is deferred to the next frame.
    pub fn insert(
        &mut self,
        queue: &wgpu::Queue,
        key: String,
        w: u32,
        h: u32,
        pixels: &[u8],
    ) -> Option<AtlasSlot> {
        debug_assert_eq!(pixels.len(), (w * h * 4) as usize);
        assert!(
            w <= ATLAS_SIZE && h <= ATLAS_SIZE,
            "label larger than atlas"
        );

        let epoch = self.packer.epoch;
        let slot = self.packer.allocate(w, h)?;
        if self.packer.epoch != epoch {
            self.entries.clear();
        }

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
        Some(slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_frame_pressure_never_reuses_an_accepted_slot() {
        let mut packer = AtlasPacker::default();
        packer.begin_frame();
        let first = packer.allocate(ATLAS_SIZE, ATLAS_SIZE / 2).unwrap();
        let second = packer.allocate(ATLAS_SIZE, ATLAS_SIZE / 2).unwrap();
        assert_ne!(first, second);
        assert!(packer.allocate(1, 1).is_none());
        assert!(packer.frame_overflowed);
        assert_eq!(packer.epoch, 0, "the live frame's texels were not reset");

        assert!(packer.begin_frame());
        assert_eq!(packer.epoch, 1);
        assert_eq!(packer.allocate(1, 1).unwrap().x, 0);
        assert!(packer.cursor_x <= ATLAS_SIZE && packer.cursor_y <= ATLAS_SIZE);
    }

    #[test]
    fn retained_quads_defer_a_reset_until_the_next_frame() {
        let mut packer = AtlasPacker::default();
        packer.begin_frame();
        packer.allocate(ATLAS_SIZE, ATLAS_SIZE).unwrap();
        packer.begin_frame();
        packer.frame_insertions = 1;
        assert!(packer.allocate(1, 1).is_none());
        assert_eq!(packer.epoch, 0);
        assert!(packer.begin_frame());
        assert_eq!(packer.epoch, 1);
    }
}
