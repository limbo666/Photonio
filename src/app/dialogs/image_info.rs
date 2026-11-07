// File: src/app/dialogs/image_info.rs
// Purpose: Image Properties and EXIF metadata modal dialog.

use eframe::egui;
use crate::app::widgets::{property_grid, settings_card};
use crate::app::PhotonioApp;
use crate::{icons, platform, viewer};

/// The "Image Properties" modal: basic details, file details, and EXIF.
pub(crate) fn draw_image_info(app: &mut PhotonioApp, ctx: &egui::Context) {
    if !app.show_image_info {
        return;
    }
    let Some(path) = app.current_file().cloned() else {
        app.show_image_info = false;
        return;
    };

    let dims = app
        .image
        .as_ref()
        .map(|im| im.dimensions)
        .or_else(|| app.animation.as_ref().map(|a| a.dimensions))
        .unwrap_or([0, 0]);

    let meta = viewer::metadata::collect(&path, dims);
    let colors = app.theme();
    let file_path = path.display().to_string();
    let folder_path = path
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    let mut open = app.show_image_info;
    let mut copy_file = false;
    let mut copy_folder = false;
    let mut open_folder = false;
    let mut done = false;

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
        egui::RichText::new(format!("{}  Image Properties", icons::ICON_INFO)).size(13.0),
    )
        .open(&mut open)
        .fixed_size(egui::vec2(540.0, 500.0))
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .frame(modal_frame)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_source("image-info-scroll")
                .max_height(420.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    settings_card(ui, &colors, Some("General Details"), |ui| {
                        property_grid("image_props_general", ui, &meta.general, &colors);
                    });
                    ui.add_space(8.0);

                    settings_card(ui, &colors, Some("File Details"), |ui| {
                        property_grid("image_props_file", ui, &meta.file_details, &colors);
                    });
                    ui.add_space(8.0);

                    settings_card(ui, &colors, Some("EXIF / Camera Metadata"), |ui| {
                        if meta.has_exif {
                            property_grid("image_props_exif", ui, &meta.exif, &colors);
                        } else {
                            ui.label(
                                egui::RichText::new("No EXIF camera metadata embedded in this image")
                                    .italics()
                                    .size(12.0)
                                    .color(colors.muted),
                            );
                        }
                    });
                    ui.add_space(8.0);

                    settings_card(ui, &colors, Some("Location & Paths"), |ui| {
                        ui.horizontal(|ui| {
                            if colors
                                .button_secondary(
                                    ui,
                                    &format!("{} Copy", icons::ICON_COPY),
                                    egui::vec2(60.0, 22.0),
                                    true,
                                )
                                .on_hover_text("Copy full file path")
                                .clicked()
                            {
                                copy_file = true;
                            }
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&file_path)
                                        .size(12.0)
                                        .color(colors.primary),
                                )
                                .truncate(),
                            )
                            .on_hover_text(&file_path);
                        });
                        ui.add_space(3.0);
                        ui.horizontal(|ui| {
                            if colors
                                .button_secondary(
                                    ui,
                                    &format!("{} Open", icons::ICON_OPEN_FOLDER),
                                    egui::vec2(60.0, 22.0),
                                    true,
                                )
                                .on_hover_text("Open containing folder in File Explorer")
                                .clicked()
                            {
                                open_folder = true;
                            }
                            if colors
                                .button_secondary(
                                    ui,
                                    &format!("{} Copy", icons::ICON_COPY),
                                    egui::vec2(60.0, 22.0),
                                    true,
                                )
                                .on_hover_text("Copy folder path")
                                .clicked()
                            {
                                copy_folder = true;
                            }
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&folder_path)
                                        .size(12.0)
                                        .color(colors.muted),
                                )
                                .truncate(),
                            )
                            .on_hover_text(&folder_path);
                        });
                    });
                });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if colors
                    .button_primary(ui, "Done", egui::vec2(80.0, 24.0), true)
                    .clicked()
                {
                    done = true;
                }
            });
        });

    app.show_image_info = open;
    if done {
        app.show_image_info = false;
    }

    if copy_file {
        ctx.copy_text(file_path.clone());
        app.show_success("File path copied");
    }
    if copy_folder {
        ctx.copy_text(folder_path.clone());
        app.show_success("Folder path copied");
    }
    if open_folder {
        if let Some(parent) = path.parent() {
            platform::open_folder(parent);
        }
    }
}
