// File: src/viewer/resize.rs
// Purpose: Photonio — image resizing (Lanczos3) and rotation helpers for the
//          "Resize Image" modal and the "Save Rotation" context-menu action.

use image::{imageops, DynamicImage};

/// Resize `img` to exactly `width × height` using high-quality Lanczos3.
pub fn resize_exact(img: &DynamicImage, width: u32, height: u32) -> DynamicImage {
    img.resize_exact(width.max(1), height.max(1), imageops::FilterType::Lanczos3)
}

/// Cover & center-crop: scale the image so it fully covers `target_w × target_h`
/// (using the larger of the two scale factors), then crop the centered region to
/// the exact target size. This preserves the source aspect ratio — no stretching.
pub fn cover_crop(img: &DynamicImage, target_w: u32, target_h: u32) -> DynamicImage {
    let src_w = img.width();
    let src_h = img.height();
    let target_w = target_w.max(1);
    let target_h = target_h.max(1);
    if src_w == 0 || src_h == 0 {
        return img.clone();
    }

    // Larger scale factor guarantees the scaled image covers the target box.
    let scale = (target_w as f64 / src_w as f64).max(target_h as f64 / src_h as f64);
    let scaled_w = (src_w as f64 * scale).round().max(1.0) as u32;
    let scaled_h = (src_h as f64 * scale).round().max(1.0) as u32;

    let scaled = img.resize_exact(scaled_w, scaled_h, imageops::FilterType::Lanczos3);

    // When the aspect ratios already match, the proportional resize is exact.
    if scaled_w == target_w && scaled_h == target_h {
        return scaled;
    }

    let crop_x = (scaled_w - target_w) / 2;
    let crop_y = (scaled_h - target_h) / 2;
    scaled.crop_imm(crop_x, crop_y, target_w, target_h)
}

/// Rotate `img` by `degrees` (a multiple of 90), returning a new image.
pub fn rotate(img: &DynamicImage, degrees: u32) -> DynamicImage {
    match degrees % 360 {
        90 => img.rotate90(),
        180 => img.rotate180(),
        270 => img.rotate270(),
        _ => img.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn cover_crop_centers_and_crops_to_target() {
        // 4:3 (40×30) → 16:9 (16×9): cover-scale to 16×12, then crop center.
        let src = DynamicImage::ImageRgba8(RgbaImage::from_pixel(40, 30, Rgba([0, 0, 0, 255])));
        let out = cover_crop(&src, 16, 9);
        assert_eq!((out.width(), out.height()), (16, 9));
    }

    #[test]
    fn cover_crop_matching_aspect_is_exact() {
        let src = DynamicImage::ImageRgba8(RgbaImage::from_pixel(16, 9, Rgba([0, 0, 0, 255])));
        let out = cover_crop(&src, 16, 9);
        assert_eq!((out.width(), out.height()), (16, 9));
    }
}
