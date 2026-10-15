// File: src/viewer/metadata.rs
// Purpose: Photonio — image & file metadata inspector. Gathers basic image
//          properties, file timestamps, and (when embedded) EXIF camera data
//          for the "Image Properties" window.

use std::path::Path;

use image::ImageDecoder;

/// A key/value row for the properties grid (key is a static label).
pub type Row = (&'static str, String);

/// Grouped metadata for a single image, ready to render in three sections.
pub struct ImageMetadata {
    pub general: Vec<Row>,
    pub file_details: Vec<Row>,
    pub exif: Vec<Row>,
    pub has_exif: bool,
}

/// Collect all available metadata for the image at `path`.
///
/// `dimensions` is the decoded pixel size (already EXIF-orientation-corrected)
/// supplied by the caller, so the inspector never needs a full decode.
pub fn collect(path: &Path, dimensions: [usize; 2]) -> ImageMetadata {
    let mut general = Vec::new();
    let mut file_details = Vec::new();

    // --- Basic details -------------------------------------------------
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_uppercase();

    general.push(("File Name", file_name));
    general.push(("Extension", extension));

    let [w, h] = dimensions;
    general.push(("Resolution", format!("{w} × {h} px")));
    general.push(("Aspect Ratio", aspect_ratio(w, h)));
    let megapixels = (w as f64) * (h as f64) / 1_000_000.0;
    general.push(("Megapixels", format!("{megapixels:.2} MP")));
    general.push(("Format", format_name(path)));
    general.push(("Color Depth", color_description(path)));

    // --- File details --------------------------------------------------
    if let Ok(meta) = std::fs::metadata(path) {
        file_details.push(("File Size", format_size(meta.len())));
        if let Ok(modified) = meta.modified() {
            file_details.push(("Modified", format_system_time(modified)));
        }
        if let Ok(created) = meta.created() {
            file_details.push(("Created", format_system_time(created)));
        }
    }

    // --- EXIF / camera -------------------------------------------------
    let (exif, has_exif) = read_exif(path);

    ImageMetadata {
        general,
        file_details,
        exif,
        has_exif,
    }
}

/// Reduced `width:height` aspect ratio, e.g. `16:9`, `4:3`, `1:1`.
fn aspect_ratio(w: usize, h: usize) -> String {
    if w == 0 || h == 0 {
        return "—".to_string();
    }
    let g = gcd(w, h);
    format!("{}:{}", w / g, h / g)
}

fn gcd(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

/// Human-readable file size, e.g. `2.4 MB`.
fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Display format name derived from the file extension.
fn format_name(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("png") => "PNG".into(),
        Some("jpg") | Some("jpeg") | Some("jfif") => "JPEG".into(),
        Some("webp") => "WebP".into(),
        Some("bmp") => "BMP".into(),
        Some("gif") => "GIF".into(),
        Some("ico") => "ICO".into(),
        Some("tif") | Some("tiff") => "TIFF".into(),
        Some(other) => other.to_uppercase(),
        None => "Unknown".into(),
    }
}

/// Original pixel format / bit depth, read from the image header (cheap — no
/// full decode). Falls back to `RGBA 8-bit` when the header can't be read.
fn color_description(path: &Path) -> String {
    let Ok(reader) = image::ImageReader::open(path) else {
        return "RGBA 8-bit".to_string();
    };
    let Ok(reader) = reader.with_guessed_format() else {
        return "RGBA 8-bit".to_string();
    };
    let Ok(decoder) = reader.into_decoder() else {
        return "RGBA 8-bit".to_string();
    };
    match decoder.color_type() {
        image::ColorType::L8 => "Grayscale 8-bit",
        image::ColorType::La8 => "Grayscale + Alpha 8-bit",
        image::ColorType::Rgb8 => "RGB 8-bit",
        image::ColorType::Rgba8 => "RGBA 8-bit",
        image::ColorType::L16 => "Grayscale 16-bit",
        image::ColorType::La16 => "Grayscale + Alpha 16-bit",
        image::ColorType::Rgb16 => "RGB 16-bit",
        image::ColorType::Rgba16 => "RGBA 16-bit",
        image::ColorType::Rgb32F => "RGB 32-bit float",
        image::ColorType::Rgba32F => "RGBA 32-bit float",
        _ => "Unknown",
    }
    .to_string()
}

/// Format a `SystemTime` as `YYYY-MM-DD HH:MM:SS` (UTC).
fn format_system_time(t: std::time::SystemTime) -> String {
    let secs = match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(_) => return "—".to_string(),
    };
    let days = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
}

/// Days-since-epoch → (year, month, day). Howard Hinnant's `civil_from_days`.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Read EXIF camera metadata from the file. Returns `(rows, found_any)`.
fn read_exif(path: &Path) -> (Vec<Row>, bool) {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return (Vec::new(), false),
    };
    let mut reader = std::io::BufReader::new(file);
    let exif = match exif::Reader::new().read_from_container(&mut reader) {
        Ok(e) => e,
        Err(_) => return (Vec::new(), false),
    };

    let mut rows: Vec<Row> = Vec::new();

    // Plain string fields (use the crate's tag-specific formatter).
    for (tag, key) in [
        (exif::Tag::Make, "Camera Make"),
        (exif::Tag::Model, "Camera Model"),
        (exif::Tag::DateTimeOriginal, "Date Taken"),
    ] {
        if let Some(field) = exif.get_field(tag, exif::In::PRIMARY) {
            let raw = field.display_value().to_string();
            if !raw.is_empty() {
                rows.push((key, raw));
            }
        }
    }

    // Numeric fields with custom units.
    if let Some(field) = exif.get_field(exif::Tag::ExposureTime, exif::In::PRIMARY) {
        let raw = field.display_value().to_string();
        if !raw.is_empty() {
            rows.push(("Shutter Speed", format!("{raw}s")));
        }
    }
    if let Some(field) = exif.get_field(exif::Tag::FNumber, exif::In::PRIMARY) {
        let raw = field.display_value().to_string();
        if !raw.is_empty() {
            rows.push(("Aperture", format!("f/{raw}")));
        }
    }
    if let Some(field) = exif.get_field(exif::Tag::PhotographicSensitivity, exif::In::PRIMARY) {
        let raw = field.display_value().to_string();
        if !raw.is_empty() {
            rows.push(("ISO Speed", format!("ISO {raw}")));
        }
    }
    if let Some(field) = exif.get_field(exif::Tag::FocalLength, exif::In::PRIMARY) {
        let raw = field.display_value().to_string();
        if !raw.is_empty() {
            rows.push(("Focal Length", format!("{raw} mm")));
        }
    }
    if let Some(field) = exif.get_field(exif::Tag::Flash, exif::In::PRIMARY) {
        let raw = field.display_value().to_string();
        if !raw.is_empty() {
            rows.push(("Flash", raw));
        }
    }
    if let Some(field) = exif.get_field(exif::Tag::ExposureProgram, exif::In::PRIMARY) {
        let raw = field.display_value().to_string();
        if !raw.is_empty() {
            rows.push(("Exposure Program", raw));
        }
    }

    let has_exif = !rows.is_empty();
    (rows, has_exif)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_days_epoch_is_1970_01_01() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(365), (1971, 1, 1)); // 1970 is not a leap year
    }

    #[test]
    fn aspect_ratio_reduces_common_resolutions() {
        assert_eq!(aspect_ratio(1920, 1080), "16:9");
        assert_eq!(aspect_ratio(4000, 3000), "4:3");
        assert_eq!(aspect_ratio(1000, 1000), "1:1");
        assert_eq!(aspect_ratio(0, 0), "—");
    }
}
