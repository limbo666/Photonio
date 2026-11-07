// File: src/app/hud.rs
// Purpose: Frosted glass HUD bar, quick-action icon pill, donation CTA pill, and transient feedback toast.

use eframe::egui;
use crate::app::types::{
    PillAction, ViewMode, DONATION_URL, ERROR_COLOR, SUCCESS_COLOR,
};
use crate::app::widgets::{pill_button, pill_button_breathing, render_drag_grip};
use crate::app::PhotonioApp;
use crate::{config, icons};

pub(crate) fn draw_hud(app: &mut PhotonioApp, ctx: &egui::Context) {
    let Some(opacity) = app.overlay_opacity() else {
        return;
    };
    if app.files.is_empty() {
        return;
    }
    // Hidden mode: no HUD bar (navigation via hotkeys/wheel only).
    if app.config.hud_position == config::HudPosition::Hidden {
        return;
    }

    let total = app.files.len();
    let position = app.current_index + 1;
    let name = app
        .files
        .get(app.current_index)
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let status = app.take_status_text();
    let gif_cur = app.animation.as_ref().map(|a| a.current_frame + 1);
    let gif_total = app.animation.as_ref().map(|a| a.frames.len());
    let gif_badge = app.animation.as_ref().map(|a| a.kind.badge());
    let colors = app.theme();
    let (anchor, offset) = match app.config.hud_position {
        config::HudPosition::Top => (egui::Align2::CENTER_TOP, egui::vec2(0.0, 12.0)),
        _ => (egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -12.0)),
    };

    let mut clicked_prev = false;
    let mut clicked_next = false;

    egui::Area::new(egui::Id::new("photonio-hud"))
        .anchor(anchor, offset)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.set_opacity(opacity);
            colors.panel(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new(icons::ICON_PREV).size(14.0))
                                .frame(false)
                                .min_size(egui::vec2(24.0, 20.0)),
                        )
                        .clicked()
                    {
                        clicked_prev = true;
                    }

                    ui.label(
                        egui::RichText::new(format!("{position} / {total}")).monospace(),
                    );
                    ui.add_space(8.0);

                    if let Some(status) = &status {
                        let text_color = if status.contains("failed")
                            || status.contains("Failed")
                            || status.starts_with("No image")
                        {
                            ERROR_COLOR
                        } else {
                            SUCCESS_COLOR
                        };
                        ui.label(egui::RichText::new(status).color(text_color));
                    } else {
                        ui.label(name);
                    }

                    if let (Some(cur), Some(total)) = (gif_cur, gif_total) {
                        let badge = gif_badge.unwrap_or("[GIF]");
                        ui.label(egui::RichText::new(badge).color(colors.gold).monospace());
                        ui.label(
                            egui::RichText::new(format!("Frame {cur} / {total}")).monospace(),
                        );
                    }

                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new(icons::ICON_NEXT).size(14.0))
                                .frame(false)
                                .min_size(egui::vec2(24.0, 20.0)),
                        )
                        .clicked()
                    {
                        clicked_next = true;
                    }
                });
            });
        });

    if clicked_prev {
        app.step(-1, ctx);
    } else if clicked_next {
        app.step(1, ctx);
    }
}

/// Floating transient status pill displayed when an action produces feedback
/// and the regular HUD navigation bar is not shown (multiview grid mode,
/// empty folder viewer, or when HUD position is set to Hidden).
pub(crate) fn draw_transient_status_pill(app: &mut PhotonioApp, ctx: &egui::Context) {
    let Some((message, alpha)) = app.feedback_info() else {
        return;
    };
    if alpha <= 0.0 {
        return;
    }

    let colors = app.theme();
    let (anchor, offset) = match app.config.hud_position {
        config::HudPosition::Top => (egui::Align2::CENTER_TOP, egui::vec2(0.0, 40.0)),
        _ => (egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -20.0)),
    };

    let text_color = if message.contains("failed")
        || message.contains("Failed")
        || message.starts_with("No image")
    {
        ERROR_COLOR
    } else {
        SUCCESS_COLOR
    };

    egui::Area::new(egui::Id::new("transient-status-pill"))
        .anchor(anchor, offset)
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            ui.set_opacity(alpha);
            colors.panel(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(message).color(text_color));
                });
            });
        });
}

/// Floating glass quick-action pill docked at the top-right of the canvas.
pub(crate) fn draw_quick_pill(app: &mut PhotonioApp, ctx: &egui::Context) -> Option<PillAction> {
    let Some(opacity) = app.overlay_opacity() else {
        return None;
    };
    if !app.config.quick_pill_enabled {
        return None;
    }
    let colors = app.theme();
    let vertical = app.config.quick_pill_orientation == config::Orientation::Vertical;
    let mut action = None;

    // Orientation-specific inner padding: tight cross-axis margin and
    // slightly larger end caps, with a matching inter-button gap.
    let margin = if vertical {
        egui::Margin::symmetric(4.0, 7.0) // snug left/right, 7px vertical caps
    } else {
        egui::Margin::symmetric(7.0, 4.0) // 7px horizontal caps, tight top/bottom
    };

    let area = if let Some(anchor) = app.custom_pill_anchor {
        let (align, offset) = anchor.to_align_and_offset(ctx.screen_rect());
        egui::Area::new(egui::Id::new("quick-action-pill"))
            .anchor(align, offset)
            .order(egui::Order::Foreground)
    } else {
        egui::Area::new(egui::Id::new("quick-action-pill"))
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 12.0))
            .order(egui::Order::Foreground)
    };

    let mut grip_res = None;

    let response = area.show(ctx, |ui| {
        ui.set_opacity(opacity);
        egui::Frame::none()
            .fill(colors.panel_fill)
            .stroke(egui::Stroke::new(1.0_f32, colors.border))
            .rounding(10.0)
            .inner_margin(margin)
            .shadow(egui::Shadow {
                offset: egui::vec2(0.0, 4.0),
                blur: 18.0,
                spread: 0.0,
                color: egui::Color32::from_black_alpha(150),
            })
            .show(ui, |ui| {
                colors.apply(ui);
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                if vertical {
                    ui.vertical(|ui| {
                        grip_res = Some(render_drag_grip(ui, true, &colors));
                        if let Some(a) = pill_buttons(app, ui) {
                            action = Some(a);
                        }
                    });
                } else {
                    ui.horizontal(|ui| {
                        grip_res = Some(render_drag_grip(ui, false, &colors));
                        if let Some(a) = pill_buttons(app, ui) {
                            action = Some(a);
                        }
                    });
                }
            });
    });

    if let Some(grip) = grip_res {
        if grip.double_clicked {
            app.custom_pill_anchor = None;
            app.config.quick_pill_anchor = None;
        } else if grip.dragged {
            let panel_rect = response.response.rect;
            let screen = ctx.screen_rect();
            let new_min = (panel_rect.min + grip.drag_delta).clamp(
                egui::pos2(screen.min.x + 8.0, screen.min.y + 8.0),
                egui::pos2(
                    (screen.max.x - panel_rect.width() - 8.0).max(screen.min.x + 8.0),
                    (screen.max.y - panel_rect.height() - 8.0).max(screen.min.y + 8.0),
                ),
            );
            let new_rect = egui::Rect::from_min_size(new_min, panel_rect.size());
            let anchor = crate::config::OverlayAnchor::from_panel_and_screen(new_rect, screen);
            app.custom_pill_anchor = Some(anchor);
            app.config.quick_pill_anchor = Some(anchor);
        }
    }

    action
}

/// Floating donation CTA pill docked top-left (mirrors the quick-action pill).
pub(crate) fn draw_donation_pill(app: &mut PhotonioApp, ctx: &egui::Context) {
    // Periodic schedule: active for 2 runs, hidden for 10, then repeats.
    let show = !app.is_supporter
        && app.config.launch_count >= 10
        && ((app.config.launch_count - 10) % 12) < 2;
    if !show || app.modal_open() {
        return;
    }
    let Some(opacity) = app.overlay_opacity() else {
        return;
    };
    let colors = app.theme();

    egui::Area::new(egui::Id::new("donation-pill"))
        .anchor(egui::Align2::LEFT_TOP, egui::vec2(16.0, 16.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.set_opacity(opacity);
            let frame_resp = egui::Frame::none()
                .fill(colors.panel_fill)
                .stroke(egui::Stroke::new(1.0_f32, colors.border))
                .rounding(10.0)
                .inner_margin(egui::Margin::symmetric(12.0, 6.0))
                .shadow(egui::Shadow {
                    offset: egui::vec2(0.0, 4.0),
                    blur: 18.0,
                    spread: 0.0,
                    color: egui::Color32::from_black_alpha(150),
                })
                .show(ui, |ui| {
                    colors.apply(ui);
                    let label = egui::RichText::new(format!(
                        "{} Support",
                        icons::ICON_PAYPAL
                    ))
                    .color(colors.gold)
                    .size(14.0);

                    let link = egui::Hyperlink::from_label_and_url(label, DONATION_URL);
                    ui.add(link)
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text("Support Photonio development on PayPal");
                });

            if frame_resp.response.hovered() {
                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        });
}

/// Render the pill's buttons; returns the clicked action, if any.
pub(crate) fn pill_buttons(app: &PhotonioApp, ui: &mut egui::Ui) -> Option<PillAction> {
    let colors = app.theme();

    // When rotation is pending on the active static image, pop a breathing Save Rotation action
    if app.view_mode == ViewMode::Viewer && app.image.is_some() && app.rotation != 0 {
        if pill_button_breathing(
            ui,
            icons::ICON_SAVE,
            "Save Rotation (Ctrl+S)",
            &colors,
        )
        .clicked()
        {
            return Some(PillAction::SaveRotation);
        }
        ui.separator();
    }

    if app.config.pill_open_file {
        if pill_button(ui, icons::ICON_OPEN_FILE, "Open File (Ctrl+O)", false, colors.gold).clicked() {
            return Some(PillAction::OpenFile);
        }
    }
    if app.config.pill_open_folder {
        if pill_button(ui, icons::ICON_OPEN_FOLDER, "Open Folder (Ctrl+Shift+O)", false, colors.gold)
            .clicked()
        {
            return Some(PillAction::OpenFolder);
        }
    }
    if app.view_mode == ViewMode::Viewer && app.image.is_some() {
        if pill_button(
            ui,
            icons::ICON_CROP,
            "Crop Image (C)",
            app.crop_state.is_some(),
            colors.gold,
        )
        .clicked()
        {
            return Some(PillAction::Crop);
        }
    }
    if app.config.pill_save_as {
        if pill_button(ui, icons::ICON_SAVE, "Save As (Ctrl+Shift+S)", false, colors.gold).clicked() {
            return Some(PillAction::SaveAs);
        }
    }
    if app.config.pill_print {
        if pill_button(ui, icons::ICON_PRINT, "Print (Ctrl+P)", false, colors.gold).clicked() {
            return Some(PillAction::Print);
        }
    }
    if app.config.pill_share {
        // Placeholder — disabled until sharing is implemented.
        ui.add_enabled_ui(false, |ui| {
            ui.add_sized(
                egui::vec2(32.0, 32.0),
                egui::Button::new(egui::RichText::new(icons::ICON_SHARE).size(18.0))
                    .frame(false)
                    .min_size(egui::vec2(32.0, 32.0)),
            )
            .on_hover_text("Share (coming soon)");
        });
    }
    if app.config.pill_filmstrip {
        // Separator: file actions (above) vs. interface actions (filmstrip).
        ui.separator();
        if pill_button(
            ui,
            icons::ICON_FILMSTRIP,
            "Toggle Filmstrip (T)",
            app.config.show_filmstrip,
            colors.gold,
        )
        .clicked()
        {
            return Some(PillAction::ToggleFilmstrip);
        }
    }
    None
}
