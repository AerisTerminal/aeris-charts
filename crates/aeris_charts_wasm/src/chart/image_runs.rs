//! WebGPU raster-image path. Immutable engine RGBA8 payloads upload once into a dedicated atlas;
//! retained frame groups then reuse the slot without competing with browser-rasterized text.

use aeris_charts_render::draw_list::{Prim, RasterImage};
use aeris_charts_render_wgpu::{ATLAS_SIZE, LabelAtlas, TexQuadInstance};
use std::borrow::Cow;

fn premultiplied_pixels(pixels: &[u8]) -> Cow<'_, [u8]> {
    if pixels.as_chunks::<4>().0.iter().all(|rgba| rgba[3] == 255) {
        Cow::Borrowed(pixels)
    } else {
        let mut converted = pixels.to_vec();
        aeris_charts_render::draw_list::premultiply_rgba8(&mut converted);
        Cow::Owned(converted)
    }
}

// Validate before looking up or uploading an atlas slot. This is also the image-run admission
// rule exercised by tests without a GPU device.
fn image_run(prim: &Prim) -> Option<(&RasterImage, [f32; 4], [f32; 4], f32)> {
    let Prim::Image {
        image,
        source,
        rect,
        opacity,
        ..
    } = prim
    else {
        return None;
    };
    let opacity = aeris_charts_render::draw_list::quantize_image_opacity(*opacity);
    if opacity == 0.0 {
        return None;
    }
    let rect = aeris_charts_render::draw_list::snap_image_rect(*rect)?;
    if image.width == 0
        || image.height == 0
        || image.width > ATLAS_SIZE
        || image.height > ATLAS_SIZE
        || image.pixels.len() != (image.width * image.height * 4) as usize
        || !image.contains_source(*source)
    {
        return None;
    }
    Some((image, *source, rect, opacity))
}

/// The atlas coordinates of `source` inside an image slot spanning `uv`.
fn source_uv(uv: [f32; 4], image: &RasterImage, [x, y, w, h]: [f32; 4]) -> [f32; 4] {
    let [u0, v0, u1, v1] = uv;
    let du = (u1 - u0) / image.width as f32;
    let dv = (v1 - v0) / image.height as f32;
    [
        u0 + x * du,
        v0 + y * dv,
        u0 + (x + w) * du,
        v0 + (y + h) * dv,
    ]
}

pub(super) fn resolve(
    atlas: &mut LabelAtlas,
    queue: &wgpu::Queue,
    prim: &Prim,
) -> Option<TexQuadInstance> {
    let (image, source, rect, opacity) = image_run(prim)?;
    let key = image.key.to_string();
    let slot = match atlas.get(&key) {
        Some(slot) => slot,
        None => {
            // Bilinear interpolation must operate on premultiplied colors, including the
            // transparent heatmap cells. This copy happens only on an image-atlas miss.
            let pixels = premultiplied_pixels(image.pixels.as_ref());
            atlas.insert(queue, key, image.width, image.height, pixels.as_ref())?
        }
    };
    Some(TexQuadInstance {
        rect,
        uv: source_uv(slot.uv(), image, source),
        color: [1.0, 1.0, 1.0, opacity],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn zero_byte_opacity_skips_the_webgpu_image_run_before_atlas_allocation() {
        // Keep the GPU entry point compiled in this host-only test module as well.
        let _gpu_resolve = resolve;
        assert_eq!(
            premultiplied_pixels(&[255, 0, 0, 255]).as_ref(),
            &[255, 0, 0, 255]
        );
        let image = RasterImage {
            key: 1,
            width: 1,
            height: 1,
            pixels: Arc::from([255, 0, 0, 255]),
        };
        let prim = |opacity| Prim::Image {
            image: image.clone(),
            source: image.full_source(),
            rect: [0.0, 0.0, 2.0, 2.0],
            opacity,
            sampling: aeris_charts_render::draw_list::ImageSampling::Linear,
        };
        for opacity in [-0.5, 0.0, 0.001] {
            assert!(image_run(&prim(opacity)).is_none(), "{opacity}");
        }
        assert_eq!(image_run(&prim(1.0 / 255.0)).unwrap().3, 1.0 / 255.0);
        assert_eq!(image_run(&prim(0.72)).unwrap().3, 184.0 / 255.0);
    }

    #[test]
    fn partial_sources_map_to_their_texels_inside_the_atlas_slot() {
        let image = RasterImage {
            key: 2,
            width: 32,
            height: 8,
            pixels: Arc::from(vec![255; 32 * 8 * 4]),
        };
        let uv = source_uv([0.25, 0.5, 0.75, 0.625], &image, [8.0, 2.0, 4.0, 4.0]);
        assert_eq!(uv, [0.375, 0.53125, 0.4375, 0.59375]);
        let outside = Prim::Image {
            image: image.clone(),
            source: [30.0, 0.0, 4.0, 8.0],
            rect: [0.0, 0.0, 2.0, 2.0],
            opacity: 1.0,
            sampling: aeris_charts_render::draw_list::ImageSampling::Nearest,
        };
        assert!(image_run(&outside).is_none());
    }
}
