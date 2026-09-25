//! WebGPU raster-image path. Immutable engine RGBA8 payloads upload once into a dedicated atlas;
//! retained frame groups then reuse the slot without competing with browser-rasterized text.

use aeris_charts_render::draw_list::Prim;
use aeris_charts_render_wgpu::{LabelAtlas, TexQuadInstance, ATLAS_SIZE};

pub(super) fn resolve(
    atlas: &mut LabelAtlas,
    queue: &wgpu::Queue,
    prim: &Prim,
) -> Option<TexQuadInstance> {
    let Prim::Image {
        image,
        rect,
        opacity,
    } = prim
    else {
        return None;
    };
    if image.width == 0
        || image.height == 0
        || image.width > ATLAS_SIZE
        || image.height > ATLAS_SIZE
        || image.pixels.len() != (image.width * image.height * 4) as usize
        || rect[2] <= 0.0
        || rect[3] <= 0.0
        || *opacity <= 0.0
    {
        return None;
    }
    let key = image.key.to_string();
    let slot = match atlas.get(&key) {
        Some(slot) => slot,
        None => atlas.insert(queue, key, image.width, image.height, image.pixels.as_ref())?,
    };
    Some(TexQuadInstance {
        rect: *rect,
        uv: slot.uv(),
        color: [1.0, 1.0, 1.0, opacity.clamp(0.0, 1.0)],
    })
}
