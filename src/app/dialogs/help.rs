// File: src/app/dialogs/help.rs
// Purpose: Keyboard shortcuts & in-app Help dialog modal.

use eframe::egui;
use crate::app::widgets::{settings_card, shortcut_table};
use crate::app::PhotonioApp;
use crate::icons;

pub(crate) fn draw_help(app: &mut PhotonioApp, ctx: &egui::Context) {
    if !app.show_help {
        return;
    }
    let _ = std::mem::take(&mut app.help_opened_this_frame);
    let mut open = app.show_help;
    let mut open_about = false;
    let mut done = false;
    let colors = app.theme();

    let modal_frame = egui::Frame::none()
        .fill(colors.panel_fill)
        .stroke(egui::Stroke::new(1.0_f32, colors.border))
        .rounding(10.0)
        .inner_margin(egui::Margin::symmetric(14.0, 12.0))
        .shadow(egui::Shadow {
            offset: egui::vec2(0.0, 4.0),
            blur: 18.0,
            spread: 0.0,
            color: egui::Color32::from_black_alpha(150),
        });

    egui::Window::new(
        egui::RichText::new(format!("{}  Help & Shortcuts", icons::ICON_INFO)).size(13.0),
    )
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .fixed_size(egui::vec2(550.0, 500.0))
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .frame(modal_frame)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_source("help-scroll")
                .max_height(420.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    settings_card(ui, &colors, Some("Navigation & Browsing"), |ui| {
                        shortcut_table(
                            "help-grid-nav",
                            ui,
                            &colors,
                            &[
                                ("Previous / Next image", "←  →  /  A  D  /  Wheel"),
                                ("First / Last image", "Home  /  End"),
                                ("Toggle Multi-View Grid", "Enter  /  Double-click"),
                                ("Focus address bar", "Ctrl + L"),
                                ("Go to parent folder", "Backspace  /  Alt + ↑"),
                                ("Folder History: Back", "Alt + ←"),
                                ("Folder History: Forward", "Alt + →"),
                                ("Preview size: bigger", "Ctrl + Wheel Up  /  Ctrl + +"),
                                ("Preview size: smaller", "Ctrl + Wheel Down  /  Ctrl + −"),
                                ("Preview size: reset", "Ctrl + 0"),
                                ("Refresh folder", "F5"),
                            ],
                        );
                    });
                    ui.add_space(8.0);

                    settings_card(ui, &colors, Some("Viewing & Zoom"), |ui| {
                        shortcut_table(
                            "help-grid-zoom",
                            ui,
                            &colors,
                            &[
                                ("Zoom in / out", "Ctrl + Wheel"),
                                ("Fit to window", "0"),
                                ("Actual size (100%)", "1"),
                                ("Pan (when zoomed)", "Left-drag"),
                                ("Rotate 90° clockwise", "R"),
                                ("Rotate 90° counter-clockwise", "Ctrl + R"),
                                ("Toggle filmstrip", "T"),
                                ("Toggle fullscreen", "F11  /  Alt + Enter"),
                            ],
                        );
                    });
                    ui.add_space(8.0);

                    settings_card(ui, &colors, Some("File Operations & Tools"), |ui| {
                        shortcut_table(
                            "help-grid-file",
                            ui,
                            &colors,
                            &[
                                ("Crop image", "C"),
                                ("Image properties / EXIF", "I"),
                                ("Resize image", "Ctrl + Shift + R"),
                                ("Rename file", "F2"),
                                ("Copy file path", "Ctrl + C"),
                                ("Copy image to clipboard", "Ctrl + Shift + C"),
                                ("Save rotation", "Ctrl + S"),
                                ("Save image as…", "Ctrl + Shift + S"),
                                ("Print image…", "Ctrl + P"),
                                ("Move to Recycle Bin", "Del"),
                                ("Show in File Explorer", "Ctrl + E"),
                            ],
                        );
                    });
                    ui.add_space(8.0);

                    settings_card(ui, &colors, Some("General & Playback"), |ui| {
                        shortcut_table(
                            "help-grid-general",
                            ui,
                            &colors,
                            &[
                                ("Open file", "Ctrl + O"),
                                ("Open folder", "Ctrl + Shift + O"),
                                ("Close image", "Ctrl + W"),
                                ("Close folder", "Ctrl + Shift + W"),
                                ("Play / Pause animation", "Space"),
                                ("Step animation frame", "[  /  ]"),
                                ("Settings", "Ctrl + ,"),
                                ("Help & Shortcuts", "F1  /  H"),
                                ("Close / Dismiss", "Esc"),
                            ],
                        );
                    });
                });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                if colors
                    .button_secondary(
                        ui,
                        &format!("{}  About Photonio", icons::ICON_INFO),
                        egui::vec2(130.0, 24.0),
                        true,
                    )
                    .clicked()
                {
                    open_about = true;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if colors
                        .button_primary(ui, "Done", egui::vec2(80.0, 24.0), true)
                        .clicked()
                    {
                        done = true;
                    }
                });
            });
        });

    app.show_help = open;
    if done {
        app.show_help = false;
    }
    if open_about {
        app.open_about();
    }
}
