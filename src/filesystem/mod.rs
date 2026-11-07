// File: src/filesystem/mod.rs
// Purpose: Photonio — directory scanning and natural-sort ordering.
//          Lists supported image files in a folder and orders them the way
//          Windows Explorer does (case-insensitive, numeric-aware).

use std::path::{Path, PathBuf};

/// Extensions the viewer can decode (lowercase, no leading dot).
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "jfif", "webp", "bmp", "gif", "ico", "tif", "tiff",
    "tga", "svg", "svgz", "jxl", "avif",
];

pub use crate::config::{FileSortCriteria, SortDirection};

/// Return every supported image file in `directory`, sorted by the given criteria and direction.
pub fn scan_directory_sorted(
    directory: &Path,
    criteria: FileSortCriteria,
    direction: SortDirection,
) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = match std::fs::read_dir(directory) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok()) // skip entries we can't read
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && has_supported_extension(path))
            .collect(),
        Err(_) => Vec::new(),
    };

    sort_files(&mut files, criteria, direction);
    files
}

/// Return every supported image file in `directory`, naturally sorted (Name, Ascending).
#[allow(dead_code)]
pub fn scan_directory(directory: &Path) -> Vec<PathBuf> {
    scan_directory_sorted(directory, FileSortCriteria::Name, SortDirection::Ascending)
}

/// Sort paths by the chosen criteria (Name, DateModified, Size) and direction.
/// Uses a Schwartzian transform (decorate-sort-undecorate) for metadata-based sorting
/// so disk metadata queries run in O(N) rather than O(N log N).
pub fn sort_files(files: &mut [PathBuf], criteria: FileSortCriteria, direction: SortDirection) {
    match criteria {
        FileSortCriteria::Name => {
            files.sort_by(|a, b| {
                let a_name = a.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                let b_name = b.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                let cmp = natord::compare(&a_name, &b_name);
                if direction == SortDirection::Descending {
                    cmp.reverse()
                } else {
                    cmp
                }
            });
        }
        FileSortCriteria::DateModified => {
            let mut decorated: Vec<(PathBuf, std::time::SystemTime, String)> = files
                .iter()
                .map(|p| {
                    let mtime = std::fs::metadata(p)
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    let name = p.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                    (p.clone(), mtime, name)
                })
                .collect();

            decorated.sort_by(|a, b| {
                let cmp = a.1.cmp(&b.1).then_with(|| natord::compare(&a.2, &b.2));
                if direction == SortDirection::Descending {
                    cmp.reverse()
                } else {
                    cmp
                }
            });

            for (i, (p, _, _)) in decorated.into_iter().enumerate() {
                files[i] = p;
            }
        }
        FileSortCriteria::Size => {
            let mut decorated: Vec<(PathBuf, u64, String)> = files
                .iter()
                .map(|p| {
                    let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                    let name = p.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                    (p.clone(), size, name)
                })
                .collect();

            decorated.sort_by(|a, b| {
                let cmp = a.1.cmp(&b.1).then_with(|| natord::compare(&a.2, &b.2));
                if direction == SortDirection::Descending {
                    cmp.reverse()
                } else {
                    cmp
                }
            });

            for (i, (p, _, _)) in decorated.into_iter().enumerate() {
                files[i] = p;
            }
        }
    }
}

/// Sort paths by file name: case-insensitive and numeric-aware, so
/// `img2` < `img10` and `IMG_1` sorts beside `img_1`.
pub fn sort_naturally(files: &mut [PathBuf]) {
    sort_files(files, FileSortCriteria::Name, SortDirection::Ascending);
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

/// Move a file across filesystems: `rename` first, then copy + delete.
pub fn move_file_fs(src: &Path, dest: &Path) -> std::io::Result<()> {
    match std::fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(src, dest)?;
            std::fs::remove_file(src)
        }
    }
}

/// Build a non-colliding destination path in `dir` for a file named `name`.
pub fn unique_dest(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned());
    for i in 2u32..10_000 {
        let candidate_name = match &ext {
            Some(e) => format!("{stem} ({i}).{e}"),
            None => format!("{stem} ({i})"),
        };
        let candidate = dir.join(candidate_name);
        if !candidate.exists() {
            return candidate;
        }
    }
    first
}

/// Default "Save as Copy" filename: `stem_resized.ext`.
pub fn default_resized_name(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".to_string());
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("png");
    format!("{stem}_resized.{ext}")
}

/// Truncate a string to `max_chars`, appending an ellipsis.
pub fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() > max_chars {
        let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    } else {
        s.to_string()
    }
}

/// Human-readable file size, e.g. `2.4 MB`.
pub fn format_size(bytes: u64) -> String {
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
pub fn format_name(path: &Path) -> String {
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

/// Validate candidate filename against Windows filesystem rules and collisions.
pub fn validate_file_name(name: &str, old_path: &Path) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("File name cannot be empty".to_string());
    }
    if trimmed.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*'])
        || trimmed.chars().any(|c| c.is_control())
    {
        return Err("Name contains invalid characters (< > : \" / \\ | ? *)".to_string());
    }
    if trimmed.ends_with('.') || trimmed.ends_with(' ') {
        return Err("Name cannot end with a period or space".to_string());
    }
    let stem = trimmed.split('.').next().unwrap_or(trimmed);
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
        "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        return Err(format!("\"{stem}\" is a reserved Windows device name"));
    }
    let orig_name = old_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if orig_name == trimmed {
        return Ok(trimmed.to_string());
    }
    let is_case_change = orig_name.eq_ignore_ascii_case(trimmed);
    let target = old_path.with_file_name(trimmed);
    if !is_case_change && target.exists() {
        return Err(format!("A file named \"{trimmed}\" already exists"));
    }
    Ok(trimmed.to_string())
}

/// Rename `old_path` to `new_path` safely on Windows, properly handling case-only changes.
pub fn safe_rename(old_path: &Path, new_path: &Path) -> std::io::Result<()> {
    let old_name = old_path.file_name().unwrap_or_default().to_string_lossy();
    let new_name = new_path.file_name().unwrap_or_default().to_string_lossy();
    if old_name == new_name {
        return Ok(());
    }
    if old_name.eq_ignore_ascii_case(&new_name) {
        // Case-only rename: use a temp name so Windows NTFS updates the casing reliably.
        let temp_name = format!("{}.{}.photonio_tmp", old_name, std::process::id());
        let temp_path = old_path.with_file_name(temp_name);
        std::fs::rename(old_path, &temp_path)?;
        if let Err(e) = std::fs::rename(&temp_path, new_path) {
            let _ = std::fs::rename(&temp_path, old_path);
            return Err(e);
        }
        Ok(())
    } else {
        std::fs::rename(old_path, new_path)
    }
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

        // Test descending direction
        sort_files(&mut files, FileSortCriteria::Name, SortDirection::Descending);
        let desc_names: Vec<String> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(desc_names, vec!["IMG10.png", "img2.png", "Img1.png"]);
    }

    #[test]
    fn sorts_by_size_correctly() {
        let temp_dir = std::env::temp_dir().join(format!("photonio_test_sort_size_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);

        let small = temp_dir.join("small.png");
        let medium = temp_dir.join("medium.png");
        let large = temp_dir.join("large.png");

        std::fs::write(&small, b"123").unwrap(); // 3 bytes
        std::fs::write(&medium, b"12345678").unwrap(); // 8 bytes
        std::fs::write(&large, b"123456789012345").unwrap(); // 15 bytes

        let mut files = vec![large.clone(), small.clone(), medium.clone()];

        // Ascending
        sort_files(&mut files, FileSortCriteria::Size, SortDirection::Ascending);
        assert_eq!(files, vec![small.clone(), medium.clone(), large.clone()]);

        // Descending
        sort_files(&mut files, FileSortCriteria::Size, SortDirection::Descending);
        assert_eq!(files, vec![large.clone(), medium.clone(), small.clone()]);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn accepts_supported_extensions_case_insensitively() {
        assert!(has_supported_extension(Path::new("photo.JPG")));
        assert!(has_supported_extension(Path::new("photo.jpeg")));
        assert!(has_supported_extension(Path::new("photo.jfif")));
        assert!(has_supported_extension(Path::new("photo.png")));
        assert!(has_supported_extension(Path::new("art.tga")));
        assert!(has_supported_extension(Path::new("vector.svg")));
        assert!(has_supported_extension(Path::new("compressed.svgz")));
        assert!(has_supported_extension(Path::new("modern.jxl")));
        assert!(has_supported_extension(Path::new("web.avif")));
        assert!(!has_supported_extension(Path::new("notes.txt")));
        assert!(!has_supported_extension(Path::new("no_extension")));
    }
}
