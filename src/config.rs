// File: src/config.rs
// Purpose: Photonio — persistent app configuration, stored as JSON in
//          %APPDATA%/Photonio/config.json.

use std::path::PathBuf;

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

/// Persisted state: settings + last directory + window geometry.
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub last_directory: Option<PathBuf>,
    /// Window position `[x, y]` in logical points.
    pub window_pos: Option<[f32; 2]>,
    /// Window size `[width, height]` in logical points.
    pub window_size: Option<[f32; 2]>,
    /// Reopen the last folder on startup.
    pub remember_last_folder: bool,
    /// Wrap navigation around at the folder boundaries.
    pub loop_navigation: bool,
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            last_directory: None,
            window_pos: None,
            window_size: None,
            remember_last_folder: true,
            loop_navigation: true,
            mouse_wheel_action: MouseWheelAction::Navigate,
            start_fullscreen: false,
            canvas_background: CanvasBackground::Glass,
            glass_opacity: 190,
            hud_position: HudPosition::Bottom,
            show_filmstrip: false,
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
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
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

    #[test]
    fn round_trips_through_json() {
        let config = Config {
            last_directory: Some(PathBuf::from("C:/pics")),
            window_pos: Some([100.0, 200.0]),
            window_size: Some([800.0, 600.0]),
            mouse_wheel_action: MouseWheelAction::Zoom,
            canvas_background: CanvasBackground::Dark,
            glass_opacity: 120,
            hud_position: HudPosition::Top,
            show_filmstrip: true,
            ..Default::default()
        };
        let json = serde_json::to_string(&config).expect("serialize");
        let decoded: Config = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded.last_directory, config.last_directory);
        assert_eq!(decoded.window_pos, config.window_pos);
        assert_eq!(decoded.window_size, config.window_size);
        assert_eq!(decoded.mouse_wheel_action, MouseWheelAction::Zoom);
        assert_eq!(decoded.canvas_background, CanvasBackground::Dark);
        assert_eq!(decoded.glass_opacity, 120);
        assert_eq!(decoded.hud_position, HudPosition::Top);
        assert!(decoded.show_filmstrip);
    }

    #[test]
    fn old_config_without_new_fields_gets_defaults() {
        let old = r#"{"last_directory": null, "window_pos": null, "window_size": null}"#;
        let decoded: Config = serde_json::from_str(old).expect("deserialize");
        assert!(decoded.remember_last_folder);
        assert!(decoded.loop_navigation);
        assert_eq!(decoded.mouse_wheel_action, MouseWheelAction::Navigate);
        assert_eq!(decoded.canvas_background, CanvasBackground::Glass);
        assert_eq!(decoded.glass_opacity, 190);
        assert_eq!(decoded.hud_position, HudPosition::Bottom);
        assert!(!decoded.show_filmstrip);
    }
}
