// File: src/assets.rs
// Purpose: Photonio — compile-time embedded binary assets (the logo).
//          `include_bytes!` bakes the PNG into the binary so it never depends
//          on a runtime file path.

use eframe::egui;

/// The in-app logo PNG (empty-state watermark + About), embedded at compile time.
pub const LOGO_PNG: &[u8] = include_bytes!("../images/logo.png");

/// The app icon PNG (window/taskbar icon), embedded at compile time.
/// Distinct from `LOGO_PNG` so the on-screen artwork stays unchanged while the
/// window + executable use this asset.
pub const APP_ICON_PNG: &[u8] = include_bytes!("../images/logof.png");

/// Decode the in-app logo into an egui `ColorImage` + dimensions.
/// Returns `None` if the bytes are not a valid image (graceful fallback).
pub fn logo_color_image() -> Option<(egui::ColorImage, [usize; 2])> {
    let img = image::load_from_memory(LOGO_PNG).ok()?.to_rgba8();
    let dims = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(dims, img.as_raw());
    Some((color, dims))
}

/// Decode the app icon into window/taskbar icon data.
/// Returns `None` on decode failure (graceful fallback).
pub fn logo_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(APP_ICON_PNG).ok()?.to_rgba8();
    let (width, height) = (img.width(), img.height());
    Some(egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    })
}
