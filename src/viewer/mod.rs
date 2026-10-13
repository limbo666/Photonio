// File: src/viewer/mod.rs
// Purpose: Photonio — image decoding (EXIF-aware), the GPU texture cache, and
//          viewport coordinate math (zoom/pan helpers).

use eframe::egui;
use image::codecs::gif::GifDecoder;
use image::codecs::webp::WebPDecoder;
use image::{AnimationDecoder, DynamicImage};

/// A decoded image plus its allocated GPU texture.
pub struct ViewerImage {
    /// Original (full-resolution) pixel dimensions — for metadata + zoom math.
    pub dimensions: [usize; 2],
    /// Dimensions of the display-res RGBA buffer (the texture size).
    display: [usize; 2],
    /// The GPU texture egui uses to draw the image.
    pub texture: egui::TextureHandle,
    /// Display-res RGBA8 pixels, kept for rotation and clipboard copy.
    rgba: Vec<u8>,
}

impl ViewerImage {
    /// Build a texture-backed image from a display-res RGBA buffer.
    pub fn from_rgba(
        ctx: &egui::Context,
        rgba: Vec<u8>,
        display: [usize; 2],
        full: [usize; 2],
    ) -> Self {
        let color = egui::ColorImage::from_rgba_unmultiplied(display, &rgba);
        let texture = ctx.load_texture("photonio-image", color, egui::TextureOptions::LINEAR);
        Self {
            dimensions: full,
            display,
            texture,
            rgba,
        }
    }

    /// Rotate the displayed image 90° clockwise in place (the `R` hotkey).
    pub fn rotate_90_cw(&mut self, ctx: &egui::Context) {
        let Some(img) = image::RgbaImage::from_raw(
            self.display[0] as u32,
            self.display[1] as u32,
            std::mem::take(&mut self.rgba),
        ) else {
            return; // buffer/dimensions out of sync (defensive no-op)
        };
        let rotated = image::imageops::rotate90(&img);
        self.display = [rotated.width() as usize, rotated.height() as usize];
        self.rgba = rotated.into_raw();
        self.dimensions = [self.dimensions[1], self.dimensions[0]];

        let color = egui::ColorImage::from_rgba_unmultiplied(self.display, &self.rgba);
        self.texture = ctx.load_texture("photonio-image", color, egui::TextureOptions::LINEAR);
    }

    /// Raw RGBA8 pixel bytes, used by the "Copy Image" clipboard action.
    pub fn rgba_bytes(&self) -> Vec<u8> {
        self.rgba.clone()
    }

    /// Dimensions of the display-res RGBA buffer (the actual stored pixels).
    pub fn display_dimensions(&self) -> [usize; 2] {
        self.display
    }
}

/// Decode raw bytes into an EXIF-oriented image.
pub fn decode(bytes: &[u8]) -> Result<DynamicImage, String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let orientation = read_exif_orientation(bytes);
    Ok(apply_orientation(img, orientation))
}

/// A decoded animation (GIF or WebP): composited frames plus per-frame delays.
pub struct DecodedAnimation {
    pub frames: Vec<DynamicImage>,
    pub delays: Vec<std::time::Duration>,
}

/// Decode every frame of an animated GIF (each frame composited onto the full
/// canvas) together with its delay. Pure Rust via the `image` crate's
/// `GifDecoder` — no external C library or video codec.
pub fn decode_gif(bytes: &[u8]) -> Result<DecodedAnimation, String> {
    let cursor = std::io::Cursor::new(bytes);
    let decoder = GifDecoder::new(cursor).map_err(|e| e.to_string())?;
    let frames = decoder
        .into_frames()
        .collect_frames()
        .map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(frames.len());
    let mut delays = Vec::with_capacity(frames.len());
    for frame in frames {
        let delay = frame.delay();
        let buffer = frame.into_buffer();
        delays.push(std::time::Duration::from(delay));
        out.push(DynamicImage::ImageRgba8(buffer));
    }
    Ok(DecodedAnimation { frames: out, delays })
}

/// Decode every frame of an animated WebP. Returns an error for still WebP
/// images so callers fall back to the static decode path.
pub fn decode_webp(bytes: &[u8]) -> Result<DecodedAnimation, String> {
    let cursor = std::io::Cursor::new(bytes);
    let decoder = WebPDecoder::new(cursor).map_err(|e| e.to_string())?;
    if !decoder.has_animation() {
        return Err("not an animated WebP".to_string());
    }
    let frames = decoder
        .into_frames()
        .collect_frames()
        .map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(frames.len());
    let mut delays = Vec::with_capacity(frames.len());
    for frame in frames {
        let delay = frame.delay();
        let buffer = frame.into_buffer();
        delays.push(std::time::Duration::from(delay));
        out.push(DynamicImage::ImageRgba8(buffer));
    }
    Ok(DecodedAnimation { frames: out, delays })
}

/// Decode a thumbnail, preferring the embedded EXIF thumbnail (fast) and
/// falling back to a full decode + downscale only when none exists.
///
/// For animated images (GIF/WebP), `image::load_from_memory` below decodes
/// only the very first frame, so filmstrip thumbnails never pay for the whole
/// animation loop.
pub(crate) fn decode_thumbnail(bytes: &[u8], max_side: u32) -> Result<DynamicImage, String> {
    if let Some(thumb) = embedded_thumbnail(bytes, max_side) {
        return Ok(thumb);
    }
    let orientation = read_exif_orientation(bytes);
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    Ok(apply_orientation(img.thumbnail(max_side, max_side), orientation))
}

/// Extract and decode the embedded EXIF thumbnail (~5-10 KB) if present.
/// Returns `None` when absent or undecodable, so callers can fall back.
pub(crate) fn embedded_thumbnail(bytes: &[u8], max_side: u32) -> Option<DynamicImage> {
    let mut cursor = std::io::Cursor::new(bytes);
    let exif = exif::Reader::new().read_from_container(&mut cursor).ok()?;
    let orientation = exif
        .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
        .and_then(|f| f.value.get_uint(0))
        .unwrap_or(1);
    let thumb_bytes = extract_thumbnail(&exif)?;
    let img = image::load_from_memory(&thumb_bytes).ok()?;
    Some(apply_orientation(img.thumbnail(max_side, max_side), orientation))
}

/// Extract the embedded JPEG thumbnail bytes from an `Exif` object. Most
/// phones/DSLRs (Sony/Canon/Nikon, DSC*.JPG) store a small preview here.
fn extract_thumbnail(exif: &exif::Exif) -> Option<Vec<u8>> {
    let offset = exif
        .get_field(exif::Tag::JPEGInterchangeFormat, exif::In::THUMBNAIL)?
        .value
        .get_uint(0)? as usize;
    let length = exif
        .get_field(exif::Tag::JPEGInterchangeFormatLength, exif::In::THUMBNAIL)?
        .value
        .get_uint(0)? as usize;
    let data = exif.buf();
    let end = offset.checked_add(length)?;
    if end <= data.len() {
        Some(data[offset..end].to_vec())
    } else {
        None
    }
}

/// Read the EXIF Orientation tag; default to 1 ("normal") when absent/unknown.
fn read_exif_orientation(bytes: &[u8]) -> u32 {
    let mut cursor = std::io::Cursor::new(bytes);
    match exif::Reader::new().read_from_container(&mut cursor) {
        Ok(exif) => exif
            .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
            .and_then(|field| field.value.get_uint(0))
            .unwrap_or(1),
        Err(_) => 1, // no EXIF (or unsupported container) → assume upright
    }
}

/// Apply the EXIF orientation so the image displays upright.
///
/// Orientation tag meanings (EXIF 2.3 / CIPA DC-008):
///   1 = none,                2 = flip horizontal,  3 = 180°,
///   4 = flip vertical,       5 = transpose (90° CW + flip H),
///   6 = 90° CW,             7 = transverse (90° CW + flip V),
///   8 = 270° CW (= 90° CCW).
fn apply_orientation(img: DynamicImage, orientation: u32) -> DynamicImage {
    match orientation {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate90().flipv(),
        8 => img.rotate270(),
        _ => img, // 1 (and any unknown value) → leave as-is
    }
}

/// Downscale `pixels` so both sides are ≤ `max_side`, preserving aspect ratio.
/// Returns the original (cloned) image when it already fits.
pub(crate) fn fit_within(pixels: &DynamicImage, max_side: u32) -> DynamicImage {
    let (w, h) = (pixels.width(), pixels.height());
    if w <= max_side && h <= max_side {
        return pixels.clone();
    }
    // Fast box-filter downscale (`thumbnail`) — far cheaper than a general
    // Triangle/Lanczos resize for the common "slightly over the limit" case.
    pixels.thumbnail(max_side, max_side)
}

/// Scale that fits `dimensions` into `available`, preserving aspect ratio.
pub fn fit_scale(dimensions: [usize; 2], available: egui::Vec2) -> f32 {
    let [w, h] = dimensions.map(|n| n as f32);
    (available.x / w).min(available.y / h)
}

/// On-screen size of the image at a given zoom factor.
pub fn draw_size(dimensions: [usize; 2], zoom: f32) -> egui::Vec2 {
    let [w, h] = dimensions.map(|n| n as f32);
    egui::vec2(w * zoom, h * zoom)
}

// `load_image`/`from_dynamic` removed: decoding now happens on the background
// thread in `loader.rs`; the UI thread only uploads pre-decoded RGBA buffers.

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GenericImageView, Rgba, RgbaImage};

    /// A 2x3 image with a single red pixel at (0,0) — used to assert transforms.
    fn tiny_rgba() -> DynamicImage {
        let mut img = RgbaImage::from_pixel(2, 3, Rgba([0, 0, 0, 255]));
        img.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        DynamicImage::ImageRgba8(img)
    }

    #[test]
    fn decodes_png() {
        let mut png_bytes = Vec::new();
        tiny_rgba()
            .write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
            .expect("png encode");
        let decoded = decode(&png_bytes).expect("decode should succeed");
        assert_eq!((decoded.width(), decoded.height()), (2, 3));
    }

    #[test]
    fn rejects_garbage_bytes() {
        assert!(decode(b"this is not an image").is_err());
    }

    #[test]
    fn orientation_6_rotates_90_cw() {
        let rotated = apply_orientation(tiny_rgba(), 6);
        assert_eq!((rotated.width(), rotated.height()), (3, 2));
        // The red top-left pixel moves to the top-right after a 90° CW rotation.
        assert_eq!(rotated.get_pixel(2, 0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn orientation_1_is_identity() {
        let img = apply_orientation(tiny_rgba(), 1);
        assert_eq!((img.width(), img.height()), (2, 3));
    }

    #[test]
    fn fit_scale_preserves_aspect_ratio() {
        // 100x50 into 1000x1000 → limited by width → scale 10.0
        assert_eq!(fit_scale([100, 50], egui::vec2(1000.0, 1000.0)), 10.0);
    }

    #[test]
    fn fit_within_downscales_oversized_images() {
        let big = DynamicImage::ImageRgba8(RgbaImage::from_pixel(4160, 2336, Rgba([0, 0, 0, 255])));
        let fitted = fit_within(&big, 2048);
        assert_eq!(fitted.width(), 2048);
        assert!(fitted.height() <= 2048);
        let ratio = 4160.0 / 2336.0;
        let fitted_ratio = fitted.width() as f32 / fitted.height() as f32;
        assert!((ratio - fitted_ratio).abs() < 0.02);
    }

    #[test]
    fn fit_within_keeps_small_images() {
        let small = DynamicImage::ImageRgba8(RgbaImage::from_pixel(100, 50, Rgba([0, 0, 0, 255])));
        let fitted = fit_within(&small, 2048);
        assert_eq!((fitted.width(), fitted.height()), (100, 50));
    }

    #[test]
    fn decodes_animated_gif_frames_and_delays() {
        use image::{Delay, Frame};
        let mut bytes = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
            encoder
                .encode_frame(Frame::from_parts(
                    RgbaImage::from_pixel(2, 2, Rgba([255, 0, 0, 255])),
                    0,
                    0,
                    Delay::from_numer_denom_ms(100, 1),
                ))
                .unwrap();
            encoder
                .encode_frame(Frame::from_parts(
                    RgbaImage::from_pixel(2, 2, Rgba([0, 255, 0, 255])),
                    0,
                    0,
                    Delay::from_numer_denom_ms(200, 1),
                ))
                .unwrap();
        }
        let gif = decode_gif(&bytes).expect("decode animated gif");
        assert_eq!(gif.frames.len(), 2);
        assert_eq!(gif.delays.len(), 2);
        assert_eq!(gif.delays[0], std::time::Duration::from_millis(100));
        assert_eq!(gif.delays[1], std::time::Duration::from_millis(200));
        assert_eq!((gif.frames[0].width(), gif.frames[0].height()), (2, 2));
    }
}
