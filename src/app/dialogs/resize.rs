// File: src/app/dialogs/resize.rs
// Purpose: Image Resizer modal: percentage presets, dimension inputs, aspect locking, and file overwriting.

use eframe::egui;
use crate::app::PhotonioApp;

/// The "Resize Image" modal: presets, target inputs, and save actions.
pub(crate) fn draw_resize_dialog(app: &mut PhotonioApp, ctx: &egui::Context) {
    if !app.show_resize_dialog {
        return;
    }
    if app.image.is_none() {
        app.show_resize_dialog = false;
        return;
    }
    let colors = app.theme();
    let orig = app.resize_orig;
    let megapixels = (orig[0] as f64 * orig[1] as f64) / 1_000_000.0;

    let mut open = app.show_resize_dialog;
    let mut save_copy = false;
    let mut overwrite = false;
    let mut done = false;

    let modal_frame = egui::Frame::none()
        .fill(colors.panel_fill)
        .stroke(egui::Stroke::new(1.0_f32, colors.border))
        .rounding(10.0)
        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
        .shadow(egui::Shadow {
            offset: egui::vec2(0.0, 4.0),
            blur: 18.0,
            spread: 0.0,
            color: egui::Color32::from_black_alpha(150),
        });

    egui::Window::new("Resize Image")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .fixed_size(egui::vec2(380.0, 360.0))
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .frame(modal_frame)
        .show(ctx, |ui| {
            ui.label(format!("Original: {} × {} px", orig[0], orig[1]));
            ui.label(format!("Megapixels: {megapixels:.2} MP"));
            ui.separator();

            // Presets. A fixed-dimension preset (1920×1080) uses cover &
            // center-crop when the source aspect ratio differs from 16:9.
            let is_16_9 = (orig[0] as f64 / orig[1].max(1) as f64 - 16.0 / 9.0).abs() < 0.01;
            ui.horizontal(|ui| {
                if ui.button("25%").clicked() {
                    app.resize_width = ((orig[0] as f32) * 0.25).round().max(1.0) as u32;
                    app.resize_height = ((orig[1] as f32) * 0.25).round().max(1.0) as u32;
                    app.resize_cover_crop = false;
                }
                if ui.button("50%").clicked() {
                    app.resize_width = ((orig[0] as f32) * 0.5).round().max(1.0) as u32;
                    app.resize_height = ((orig[1] as f32) * 0.5).round().max(1.0) as u32;
                    app.resize_cover_crop = false;
                }
                if ui.button("75%").clicked() {
                    app.resize_width = ((orig[0] as f32) * 0.75).round().max(1.0) as u32;
                    app.resize_height = ((orig[1] as f32) * 0.75).round().max(1.0) as u32;
                    app.resize_cover_crop = false;
                }
                if ui.button("1920×1080").clicked() {
                    app.resize_width = 1920;
                    app.resize_height = 1080;
                    app.resize_cover_crop = true;
                }
            });
            if !is_16_9 {
                ui.label(
                    egui::RichText::new("(Center-cropped to fit)")
                        .small()
                        .italics()
                        .color(colors.muted),
                );
            }
            ui.separator();

            // Target inputs. Manual edits clear the fixed-preset crop; the
            // aspect lock then updates the other axis proportionally.
            ui.horizontal(|ui| {
                ui.label("Width:");
                let w = ui.add(egui::DragValue::new(&mut app.resize_width).range(1..=100_000));
                if w.changed() {
                    app.resize_cover_crop = false;
                    if app.resize_aspect {
                        let ow = orig[0].max(1) as f32;
                        let oh = orig[1].max(1) as f32;
                        app.resize_height = ((app.resize_width as f32) * oh / ow).round().max(1.0) as u32;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Height:");
                let h = ui.add(egui::DragValue::new(&mut app.resize_height).range(1..=100_000));
                if h.changed() {
                    app.resize_cover_crop = false;
                    if app.resize_aspect {
                        let ow = orig[0].max(1) as f32;
                        let oh = orig[1].max(1) as f32;
                        app.resize_width = ((app.resize_height as f32) * ow / oh).round().max(1.0) as u32;
                    }
                }
            });
            ui.checkbox(&mut app.resize_aspect, "Maintain aspect ratio");
            ui.separator();

            // Save actions.
            if ui.button("Save as Copy...").clicked() {
                save_copy = true;
            }
            if app.resize_confirm_overwrite {
                ui.horizontal(|ui| {
                    ui.label("Overwrite original?");
                    if ui.button("Yes").clicked() {
                        overwrite = true;
                    }
                    if ui.button("Cancel").clicked() {
                        app.resize_confirm_overwrite = false;
                    }
                });
            } else if ui.button("Overwrite Original").clicked() {
                app.resize_confirm_overwrite = true;
            }

            ui.separator();
            if ui.button("Close").clicked() {
                done = true;
            }
        });

    app.show_resize_dialog = open;
    if done {
        app.show_resize_dialog = false;
    }
    if save_copy {
        app.resize_save(false, ctx);
    }
    if overwrite {
        app.resize_confirm_overwrite = false;
        app.resize_save(true, ctx);
    }
}
