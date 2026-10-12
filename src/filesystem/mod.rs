// File: src/filesystem/mod.rs
// Purpose: Photonio — directory scanning and natural-sort ordering.
//          Lists supported image files in a folder and orders them the way
//          Windows Explorer does (case-insensitive, numeric-aware).

use std::path::{Path, PathBuf};

/// Extensions the viewer can decode (lowercase, no leading dot).
const SUPPORTED_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "gif", "ico"];

/// Return every supported image file in `directory`, naturally sorted.
/// Unreadable entries and subdirectories are skipped; an unreadable directory
/// yields an empty list rather than an error.
pub fn scan_directory(directory: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = match std::fs::read_dir(directory) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok()) // skip entries we can't read
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && has_supported_extension(path))
            .collect(),
        Err(_) => Vec::new(),
    };

    sort_naturally(&mut files);
    files
}

/// Sort paths by file name: case-insensitive and numeric-aware, so
/// `img2` < `img10` and `IMG_1` sorts beside `img_1`.
pub fn sort_naturally(files: &mut [PathBuf]) {
    files.sort_by(|a, b| {
        let a_name = a.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
        let b_name = b.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
        natord::compare(&a_name, &b_name)
    });
}

/// True if `path` ends with a supported extension (checked case-insensitively).
fn has_supported_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| SUPPORTED_EXTENSIONS.iter().any(|supported| supported.eq_ignore_ascii_case(ext)))
        .unwrap_or(false)
}

/// True if `path` has a supported image extension (case-insensitive).
pub fn is_supported_path(path: &Path) -> bool {
    has_supported_extension(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_naturally_and_case_insensitively() {
        let mut files = vec![
            PathBuf::from("C:/pics/IMG10.png"),
            PathBuf::from("C:/pics/img2.png"),
            PathBuf::from("C:/pics/Img1.png"),
        ];
        sort_naturally(&mut files);

        let names: Vec<String> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();

        assert_eq!(names, vec!["Img1.png", "img2.png", "IMG10.png"]);
    }

    #[test]
    fn accepts_supported_extensions_case_insensitively() {
        assert!(has_supported_extension(Path::new("photo.JPG")));
        assert!(has_supported_extension(Path::new("photo.png")));
        assert!(!has_supported_extension(Path::new("notes.txt")));
        assert!(!has_supported_extension(Path::new("no_extension")));
    }
}
