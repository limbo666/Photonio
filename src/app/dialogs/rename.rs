// File: src/app/dialogs/rename.rs
// Purpose: Safe in-place file rename modal dialog with live Windows name validation.

use std::path::Path;
use eframe::egui;
use crate::app::types::{ERROR_COLOR, WARN_COLOR};
use crate::app::PhotonioApp;
use crate::filesystem::validate_file_name;
use crate::{filesystem, icons};

/// Modal dialog for renaming the active file.
pub(crate) fn draw_rename_dialog(app: &mut PhotonioApp, ctx: &egui::Context) {
    let colors = app.theme();
    let Some(state) = &mut app.rename_state else {
        return;
    };
    let orig_name = state
        .old_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let trimmed = state.new_name.trim().to_string();
    let validation_error = validate_file_name(&state.new_name, &state.old_path).err();
    let has_error = validation_error.is_some();
    let target_path = state.old_path.with_file_name(&trimmed);
    let is_unsupported_ext = !trimmed.is_empty()
        && !has_error
        && !filesystem::is_supported_path(&target_path);

    let mut open = true;
    let mut should_commit = false;
    let mut should_cancel = false;

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

    egui::Window::new("Rename File")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .fixed_size(egui::vec2(360.0, 140.0))
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .frame(modal_frame)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(icons::ICON_INFO).color(colors.muted));
                ui.label(
                    egui::RichText::new(format!("Current: {}", orig_name))
                        .color(colors.muted)
                        .size(12.0),
                );
            });
            ui.add_space(6.0);

            let input_fill = if colors.dark_mode {
                egui::Color32::from_black_alpha(60)
            } else {
                egui::Color32::from_black_alpha(10)
            };
            let frame_stroke = if has_error {
                egui::Stroke::new(1.0_f32, ERROR_COLOR)
            } else {
                egui::Stroke::new(1.0_f32, colors.border)
            };

            egui::Frame::none()
                .fill(input_fill)
                .stroke(frame_stroke)
                .rounding(4.0)
                .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    let mut output = egui::TextEdit::singleline(&mut state.new_name)
                        .desired_width(f32::INFINITY)
                        .hint_text("New file name")
                        .text_color(if has_error {
                            ERROR_COLOR
                        } else {
                            colors.primary
                        })
                        .show(ui);

                    if state.focus_pending {
                        output.response.request_focus();
                        let stem_chars = Path::new(&state.new_name)
                            .file_stem()
                            .map(|s| s.to_string_lossy().chars().count())
                            .unwrap_or_else(|| state.new_name.chars().count());
                        let range = egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            egui::text::CCursor::new(stem_chars),
                        );
                        output.state.cursor.set_char_range(Some(range));
                        output.state.store(ui.ctx(), output.response.id);
                        state.focus_pending = false;
                    }
                });

            ui.add_space(4.0);
            if let Some(err) = &validation_error {
                ui.label(egui::RichText::new(err).color(ERROR_COLOR).size(12.0));
            } else if is_unsupported_ext {
                ui.label(
                    egui::RichText::new("⚠️ Warning: Extension is not a supported image format")
                        .color(WARN_COLOR)
                        .size(12.0),
                );
            } else {
                ui.label(egui::RichText::new(" ").size(12.0));
            }

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let can_commit = !has_error;
                    let commit_btn = ui.add_enabled(can_commit, egui::Button::new("Rename"));
                    let cancel_btn = ui.button("Cancel");

                    if cancel_btn.clicked() {
                        should_cancel = true;
                    } else if commit_btn.clicked() && can_commit {
                        should_commit = true;
                    }
                });
            });

            // Enter commits if valid, Escape cancels
            let enter_hit = ui.input(|i| i.key_pressed(egui::Key::Enter));
            let esc_hit = ui.input(|i| i.key_pressed(egui::Key::Escape));

            if esc_hit {
                should_cancel = true;
            } else if enter_hit && !has_error {
                should_commit = true;
            }
        });

    if !open || should_cancel {
        app.rename_state = None;
    } else if should_commit {
        app.commit_rename(&trimmed, ctx);
    }
}
