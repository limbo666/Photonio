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

    let default_size = [960.0_f32, 640.0_f32];

    let (final_size, final_pos) = if let Some([work_x, work_y, work_w, work_h]) = platform::get_work_area() {
        // Enforce safe limits so window never exceeds screen work area minus margins
        let max_w = (work_w - 40.0).max(320.0);
        let max_h = (work_h - 40.0).max(240.0);

        let size = if let Some(s) = config.window_size {
            [s[0].clamp(320.0, max_w), s[1].clamp(240.0, max_h)]
        } else {
            [default_size[0].min(max_w), default_size[1].min(max_h)]
        };

        let pos = if let Some(p) = config.window_pos {
            // Keep window within visible screen boundaries
            let max_x = (work_x + work_w - 100.0).max(work_x);
            let max_y = (work_y + work_h - 60.0).max(work_y);
            let mut x = p[0].clamp(work_x, max_x);
            let mut y = p[1].clamp(work_y, max_y);

            // Shift if bottom or right hangs off screen
            if x + size[0] > work_x + work_w {
                x = (work_x + work_w - size[0]).max(work_x);
            }
            if y + size[1] > work_y + work_h {
                y = (work_y + work_h - size[1]).max(work_y);
            }
            [x, y]
        } else {
            // Default: center window in visible screen work area
            let cx = (work_x + (work_w - size[0]) / 2.0).max(work_x);
            let cy = (work_y + (work_h - size[1]) / 2.0).max(work_y);
            [cx, cy]
        };

        (size, Some(pos))
    } else {
        let size = config.window_size.unwrap_or(default_size);
        (size, config.window_pos)
    };

    let mut viewport = egui::ViewportBuilder::default()
        .with_title(format!("Photonio v{}", env!("CARGO_PKG_VERSION")))
        .with_inner_size(final_size)
        .with_min_inner_size([320.0, 240.0])
        .with_drag_and_drop(true)
        .with_transparent(true); // transparent window (prep for DWM Mica/Acrylic)

    if let Some(pos) = final_pos {
        viewport = viewport.with_position(pos);
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
        vsync: true,
        multisampling: 0,
        depth_buffer: 0,
        stencil_buffer: 0,
        ..Default::default()
    };

    eframe::run_native(
        "Photonio",
        native_options,
        Box::new(move |cc| Ok(Box::new(PhotonioApp::new(cc, config, cli_path)))),
    )
}
