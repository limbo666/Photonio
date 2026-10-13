// File: src/main.rs
// Purpose: Photonio — entry point and window lifecycle.
//          Loads persisted config, sets the window/taskbar icon, builds the
//          native window options, and hands control to the eframe event loop.

// Hide the console window in release builds (a GUI app shouldn't show a terminal).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod about;
mod app;
mod assets;
mod config;
mod filesystem;
mod icons;
mod loader;
mod platform;
mod theme;
mod viewer;

use app::PhotonioApp;
use eframe::egui;

fn main() -> eframe::Result {
    let config = config::Config::load();
    // Optional file path from CLI / double-click / "Open With".
    let cli_path = std::env::args_os().nth(1).map(std::path::PathBuf::from);

    let mut viewport = egui::ViewportBuilder::default()
        .with_title(format!("Photonio v{}", env!("CARGO_PKG_VERSION")))
        .with_inner_size([960.0, 640.0])
        .with_min_inner_size([320.0, 240.0])
        .with_transparent(true); // transparent window (prep for DWM Mica/Acrylic)

    // Restore saved window geometry, if any.
    if let Some(pos) = config.window_pos {
        viewport = viewport.with_position(pos);
    }
    if let Some(size) = config.window_size {
        viewport = viewport.with_inner_size(size);
    }

    // Window + taskbar icon (gracefully omitted if the logo fails to decode).
    if let Some(icon) = assets::logo_icon() {
        viewport = viewport.with_icon(icon);
    }

    // Start in fullscreen if configured.
    if config.start_fullscreen {
        viewport = viewport.with_fullscreen(true);
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "Photonio",
        native_options,
        Box::new(move |cc| Ok(Box::new(PhotonioApp::new(cc, config, cli_path)))),
    )
}
