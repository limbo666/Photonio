// File: src/config.rs
// Purpose: Photonio — persistent app configuration, stored as JSON in
//          %APPDATA%/Photonio/config.json.

use std::path::PathBuf;

use eframe::egui;
use serde::{Deserialize, Serialize};

/// What the mouse wheel does when no modifier is held.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum MouseWheelAction {
    Navigate,
    Zoom,
}

/// How the window backdrop is cleared.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum CanvasBackground {
    Glass,
    Dark,
    Light,
}

/// Where the floating navigation HUD is anchored.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum HudPosition {
    Bottom,
    Top,
    Hidden,
}

/// Which screen edge the thumbnail filmstrip is docked to.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum FilmstripPosition {
    Bottom,
    Top,
    Left,
    Right,
}

/// Layout direction for the quick-action pill.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    Horizontal,
    Vertical,
}

/// Tile size for the custom folder browser.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum FolderTileSize {
    Small,
    #[default]
    Medium,
    Large,
    List,
}

/// Which entries the custom folder browser shows.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContentFilter {
    #[default]
    Images,
    Media,
    All,
}

/// Criteria used to sort image files in folders.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum FileSortCriteria {
    #[default]
    Name,
    DateModified,
    Size,
}

impl FileSortCriteria {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::DateModified => "Date Modified",
            Self::Size => "Size",
        }
    }
}

/// Direction for file sorting.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    #[default]
    Ascending,
    Descending,
}

impl SortDirection {
    #[allow(dead_code)]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Ascending => "Ascending",
            Self::Descending => "Descending",
        }
    }

    #[allow(dead_code)]
    pub fn toggle(&self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }
}

/// Screen-edge relative anchor for floating overlays (Tool Pill, Crop Toolbar).
/// Anchors to the closest edges (Left vs. Right, Top vs. Bottom) with the
/// corresponding offset distance, ensuring the control maintains its relative
/// position when the window is resized.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub struct OverlayAnchor {
    pub from_right: bool,
    pub from_bottom: bool,
    pub offset_x: f32,
    pub offset_y: f32,
}

impl OverlayAnchor {
    pub fn new(from_right: bool, from_bottom: bool, offset_x: f32, offset_y: f32) -> Self {
        Self {
            from_right,
            from_bottom,
            offset_x: offset_x.max(8.0),
            offset_y: offset_y.max(8.0),
        }
    }

    /// Compute edge anchor from the panel's bounding rect and the visible screen rect.
    pub fn from_panel_and_screen(panel_rect: egui::Rect, screen: egui::Rect) -> Self {
        let center = panel_rect.center();
        let screen_center = screen.center();
        let from_right = center.x >= screen_center.x;
        let from_bottom = center.y >= screen_center.y;

        let offset_x = if from_right {
            (screen.max.x - panel_rect.max.x).max(8.0)
        } else {
            (panel_rect.min.x - screen.min.x).max(8.0)
        };

        let offset_y = if from_bottom {
            (screen.max.y - panel_rect.max.y).max(8.0)
        } else {
            (panel_rect.min.y - screen.min.y).max(8.0)
        };

        Self {
            from_right,
            from_bottom,
            offset_x,
            offset_y,
        }
    }

    /// Returns the egui alignment and screen-clamped offset vector for `Area::anchor`.
    pub fn to_align_and_offset(&self, screen: egui::Rect) -> (egui::Align2, egui::Vec2) {
        let align = match (self.from_right, self.from_bottom) {
            (false, false) => egui::Align2::LEFT_TOP,
            (true, false) => egui::Align2::RIGHT_TOP,
            (false, true) => egui::Align2::LEFT_BOTTOM,
            (true, true) => egui::Align2::RIGHT_BOTTOM,
        };

        let max_x = (screen.width() - 40.0).max(8.0);
        let max_y = (screen.height() - 40.0).max(8.0);
        let eff_x = self.offset_x.clamp(8.0, max_x);
        let eff_y = self.offset_y.clamp(8.0, max_y);

        let signed_x = if self.from_right { -eff_x } else { eff_x };
        let signed_y = if self.from_bottom { -eff_y } else { eff_y };

        (align, egui::vec2(signed_x, signed_y))
    }
}

/// Persisted state: settings + last directory + window geometry.
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub last_directory: Option<PathBuf>,
    /// Exact path of the image last viewed (restored on startup when it still
    /// exists); `last_directory` remains the fallback.
    pub last_file_path: Option<PathBuf>,
    /// Window position `[x, y]` in logical points.
    pub window_pos: Option<[f32; 2]>,
    /// Window size `[width, height]` in logical points.
    pub window_size: Option<[f32; 2]>,
    /// Reopen the last folder on startup.
    pub remember_last_folder: bool,
    /// Wrap navigation around at the folder boundaries.
    pub loop_navigation: bool,
    /// Active file sort criteria for browsing folders (Name, DateModified, Size).
    pub sort_criteria: FileSortCriteria,
    /// Active file sort direction (Ascending or Descending).
    pub sort_direction: SortDirection,
    /// Wheel behavior without a modifier.
    pub mouse_wheel_action: MouseWheelAction,
    /// Start the app in fullscreen.
    pub start_fullscreen: bool,
    /// Window backdrop style.
    pub canvas_background: CanvasBackground,
    /// Alpha (darkness) of the glass panels, `50..=255`.
    pub glass_opacity: u8,
    /// Where the floating HUD is shown.
    pub hud_position: HudPosition,
    /// Whether the thumbnail filmstrip is shown.
    pub show_filmstrip: bool,
    /// Which screen edge the thumbnail filmstrip is docked to.
    pub filmstrip_position: FilmstripPosition,
    /// Master toggle for the top-right quick-action pill.
    pub quick_pill_enabled: bool,
    /// Optional user-dragged position `[x, y]` for the quick-action tool pill.
    pub quick_pill_pos: Option<[f32; 2]>,
    /// Closest-edge relative anchor for the quick-action tool pill.
    #[serde(default)]
    pub quick_pill_anchor: Option<OverlayAnchor>,
    /// Individual quick-action pill button visibility.
    pub pill_open_file: bool,
    pub pill_open_folder: bool,
    pub pill_save_as: bool,
    pub pill_print: bool,
    pub pill_share: bool,
    pub pill_filmstrip: bool,
    /// Layout direction of the quick-action pill.
    pub quick_pill_orientation: Orientation,
    /// Number of filmstrip tiles shown (one of 7, 9, or 11).
    pub filmstrip_count: usize,
    /// Fade the HUD/pill/filmstrip overlays out after inactivity.
    pub auto_fade_overlays: bool,
    /// Seconds of inactivity before overlays fade, `1..=10`.
    pub fade_delay_secs: u64,
    /// Last folder opened via "Open Folder".
    pub last_open_folder: Option<PathBuf>,
    /// Last destination chosen for "Copy File to...".
    pub last_copy_folder: Option<PathBuf>,
    /// Last destination chosen for "Move File to...".
    pub last_move_folder: Option<PathBuf>,
    /// Tile size in the custom folder browser.
    pub folder_tile_size: FolderTileSize,
    /// Content filter in the custom folder browser.
    pub folder_content_filter: ContentFilter,
    /// Show files (not just folders) in the custom folder browser.
    pub folder_show_files: bool,
    /// User-pinned Quick Access folders (persisted; sanitized on load).
    pub pinned_folders: Vec<PathBuf>,
    /// Number of application launches (drives the periodic donation CTA).
    pub launch_count: u64,
    /// Whether Ctrl+Wheel / Ctrl+±/0 adjusts the MultiView grid tile size.
    pub adjustable_grid_tile_size: bool,
    /// Width of each MultiView grid tile in logical points (80..=300).
    pub grid_tile_size: f32,
    /// Whether to show subfolders in MultiView grid. Defaults to true.
    pub multiview_show_folders: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            last_directory: None,
            last_file_path: None,
            window_pos: None,
            window_size: None,
            remember_last_folder: true,
            loop_navigation: true,
            sort_criteria: FileSortCriteria::Name,
            sort_direction: SortDirection::Ascending,
            mouse_wheel_action: MouseWheelAction::Navigate,
            start_fullscreen: false,
            canvas_background: CanvasBackground::Light,
            glass_opacity: 240,
            hud_position: HudPosition::Bottom,
            show_filmstrip: false,
            filmstrip_position: FilmstripPosition::Bottom,
            quick_pill_enabled: true,
            quick_pill_pos: None,
            quick_pill_anchor: None,
            pill_open_file: true,
            pill_open_folder: true,
            pill_save_as: true,
            pill_print: true,
            pill_share: true,
            pill_filmstrip: true,
            quick_pill_orientation: Orientation::Horizontal,
            filmstrip_count: 7,
            auto_fade_overlays: false,
            fade_delay_secs: 3,
            last_open_folder: None,
            last_copy_folder: None,
            last_move_folder: None,
            folder_tile_size: FolderTileSize::Medium,
            folder_content_filter: ContentFilter::Images,
            folder_show_files: true,
            pinned_folders: Vec::new(),
            launch_count: 0,
            adjustable_grid_tile_size: true,
            grid_tile_size: 150.0,
            multiview_show_folders: true,
        }
    }
}

impl Config {
    /// Full path to the config file: `%APPDATA%/Photonio/config.json`.
    pub fn path() -> PathBuf {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("Photonio")
            .join("config.json")
    }

    /// Load config from disk; a missing or corrupt file yields defaults.
    pub fn load() -> Self {
        let mut config: Self = std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        config.sanitize_pinned();
        config
    }

    /// Drop duplicate and non-existent pinned folders (defensive on load).
    fn sanitize_pinned(&mut self) {
        let mut deduped: Vec<PathBuf> = Vec::new();
        for path in std::mem::take(&mut self.pinned_folders) {
            if path.is_dir() && !deduped.contains(&path) {
                deduped.push(path);
            }
        }
        self.pinned_folders = deduped;
    }

    /// Write config to disk, creating the directory if needed. Best-effort:
    /// persistence failure must never crash the viewer.
    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            if let Some(dir) = Self::path().parent() {
                let _ = std::fs::create_dir_all(dir);
                let _ = std::fs::write(Self::path(), json);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;


    #[test]
    fn round_trips_through_json() {
        let config = Config {
            last_directory: Some(PathBuf::from("C:/pics")),
            last_file_path: Some(PathBuf::from("C:/pics/img42.jpg")),
            window_pos: Some([100.0, 200.0]),
            window_size: Some([800.0, 600.0]),
            mouse_wheel_action: MouseWheelAction::Zoom,
            canvas_background: CanvasBackground::Dark,
            glass_opacity: 120,
            hud_position: HudPosition::Top,
            show_filmstrip: true,
            filmstrip_position: FilmstripPosition::Right,
            quick_pill_enabled: false,
            pill_save_as: false,
            quick_pill_orientation: Orientation::Vertical,
            pinned_folders: vec![PathBuf::from("C:/pics"), PathBuf::from("C:/pics/family")],
            ..Default::default()
        };
        let json = serde_json::to_string(&config).expect("serialize");
        let decoded: Config = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded.last_directory, config.last_directory);
        assert_eq!(decoded.last_file_path, config.last_file_path);
        assert_eq!(decoded.window_pos, config.window_pos);
        assert_eq!(decoded.window_size, config.window_size);
        assert_eq!(decoded.mouse_wheel_action, MouseWheelAction::Zoom);
        assert_eq!(decoded.canvas_background, CanvasBackground::Dark);
        assert_eq!(decoded.glass_opacity, 120);
        assert_eq!(decoded.hud_position, HudPosition::Top);
        assert!(decoded.show_filmstrip);
        assert_eq!(decoded.filmstrip_position, FilmstripPosition::Right);
        assert!(!decoded.quick_pill_enabled);
        assert!(!decoded.pill_save_as);
        assert_eq!(decoded.quick_pill_orientation, Orientation::Vertical);
        assert_eq!(decoded.filmstrip_count, 7);
        assert!(!decoded.auto_fade_overlays);
        assert_eq!(decoded.fade_delay_secs, 3);
        assert!(decoded.last_open_folder.is_none());
        assert!(decoded.last_copy_folder.is_none());
        assert!(decoded.last_move_folder.is_none());
        assert_eq!(decoded.folder_tile_size, FolderTileSize::Medium);
        assert_eq!(decoded.folder_content_filter, ContentFilter::Images);
        assert!(decoded.folder_show_files);
        assert_eq!(
            decoded.pinned_folders,
            vec![PathBuf::from("C:/pics"), PathBuf::from("C:/pics/family")]
        );
        assert!(decoded.adjustable_grid_tile_size);
        assert!((decoded.grid_tile_size - 150.0).abs() < f32::EPSILON);
        assert!(decoded.multiview_show_folders);
    }

    #[test]
    fn old_config_without_new_fields_gets_defaults() {
        let old = r#"{"last_directory": null, "window_pos": null, "window_size": null}"#;
        let decoded: Config = serde_json::from_str(old).expect("deserialize");
        assert!(decoded.remember_last_folder);
        assert!(decoded.last_file_path.is_none());
        assert!(decoded.loop_navigation);
        assert_eq!(decoded.mouse_wheel_action, MouseWheelAction::Navigate);
        assert_eq!(decoded.canvas_background, CanvasBackground::Light);
        assert_eq!(decoded.glass_opacity, 240);
        assert_eq!(decoded.hud_position, HudPosition::Bottom);
        assert!(!decoded.show_filmstrip);
        assert_eq!(decoded.filmstrip_position, FilmstripPosition::Bottom);
        assert!(decoded.quick_pill_enabled);
        assert!(decoded.pill_open_file);
        assert!(decoded.pill_print);
        assert_eq!(decoded.quick_pill_orientation, Orientation::Horizontal);
        assert_eq!(decoded.filmstrip_count, 7);
        assert!(!decoded.auto_fade_overlays);
        assert_eq!(decoded.fade_delay_secs, 3);
        assert!(decoded.last_open_folder.is_none());
        assert!(decoded.last_copy_folder.is_none());
        assert!(decoded.last_move_folder.is_none());
        assert_eq!(decoded.folder_tile_size, FolderTileSize::Medium);
        assert_eq!(decoded.folder_content_filter, ContentFilter::Images);
        assert!(decoded.folder_show_files);
        assert!(decoded.pinned_folders.is_empty());
        assert_eq!(decoded.launch_count, 0);
        assert!(decoded.adjustable_grid_tile_size);
        assert!((decoded.grid_tile_size - 150.0).abs() < f32::EPSILON);
        assert!(decoded.multiview_show_folders);
    }
}
