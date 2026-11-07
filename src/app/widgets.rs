// File: src/app/widgets.rs
// Purpose: Reusable UI widgets, cards, table layouts, and image rect calculations.

use std::path::Path;
use eframe::egui;
use crate::{theme, viewer};

/// Render a clean, rounded, consistent "card" (grouped frame) for the Settings content.
/// Enforces full available width and unified margins/borders across all tabs.
pub(crate) fn settings_card<R>(
    ui: &mut egui::Ui,
    colors: &theme::ThemeColors,
    title: Option<&str>,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let frame = egui::Frame::none()
        .fill(if colors.dark_mode {
            egui::Color32::from_rgba_unmultiplied(13, 13, 17, 230)
        } else {
            egui::Color32::from_black_alpha(8)
        })
        .stroke(egui::Stroke::new(
            1.0_f32,
            if colors.dark_mode {
                egui::Color32::from_white_alpha(20)
            } else {
                egui::Color32::from_black_alpha(24)
            },
        ))
        .rounding(6.0)
        .inner_margin(egui::Margin::symmetric(14.0, 9.0));

    frame.show(ui, |ui| {
        // Enforce consistent card width across all tabs
        ui.set_min_width(ui.available_width());
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 5.0);

        if let Some(t) = title {
            ui.label(
                egui::RichText::new(t)
                    .strong()
                    .size(13.0)
                    .color(colors.primary),
            );
            ui.add_space(2.0);
        }

        add_contents(ui)
    }).inner
}

/// Render a two-column, striped key/value grid for a metadata section.
pub(crate) fn property_grid(
    id: &str,
    ui: &mut egui::Ui,
    rows: &[(&'static str, String)],
    colors: &theme::ThemeColors,
) {
    egui::Grid::new(id)
        .striped(true)
        .spacing([16.0, 5.0])
        .num_columns(2)
        .show(ui, |ui| {
            for (key, value) in rows {
                ui.label(
                    egui::RichText::new(*key)
                        .size(12.0)
                        .color(colors.muted),
                );
                ui.label(
                    egui::RichText::new(value)
                        .size(12.0)
                        .color(colors.primary),
                );
                ui.end_row();
            }
        });
}

/// Render a clean two-column grid of keyboard shortcuts with <kbd> pill badges.
pub(crate) fn shortcut_table(
    id: &str,
    ui: &mut egui::Ui,
    colors: &theme::ThemeColors,
    rows: &[(&str, &str)],
) {
    let pill_fill = if colors.dark_mode {
        egui::Color32::from_white_alpha(14)
    } else {
        egui::Color32::from_black_alpha(10)
    };
    let pill_stroke = egui::Stroke::new(
        1.0_f32,
        if colors.dark_mode {
            egui::Color32::from_white_alpha(24)
        } else {
            egui::Color32::from_black_alpha(20)
        },
    );

    egui::Grid::new(id)
        .num_columns(2)
        .spacing(egui::vec2(20.0, 6.0))
        .show(ui, |ui| {
            for (action, keys) in rows {
                ui.label(
                    egui::RichText::new(*action)
                        .size(12.5)
                        .color(colors.primary),
                );

                egui::Frame::none()
                    .fill(pill_fill)
                    .stroke(pill_stroke)
                    .rounding(4.0)
                    .inner_margin(egui::Margin::symmetric(7.0, 2.5))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(*keys)
                                .monospace()
                                .size(11.5)
                                .color(colors.primary),
                        );
                    });
                ui.end_row();
            }
        });
}

/// A flat, full-width, enabled menu item.
pub(crate) fn menu_item(ui: &mut egui::Ui, text: &str) -> egui::Response {
    menu_item_enabled(ui, text, true)
}

/// A flat, full-width menu item, optionally disabled/dimmed.
pub(crate) fn menu_item_enabled(ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(text)
            .frame(false)
            .min_size(egui::vec2(ui.available_width(), 20.0)),
    )
}

/// A compact, flat glyph button for the quick-action pill: a strict square
/// hit-box with the icon dead-centered.
pub(crate) fn pill_button(
    ui: &mut egui::Ui,
    glyph: &str,
    tooltip: &str,
    highlighted: bool,
    highlight_color: egui::Color32,
) -> egui::Response {
    let mut text = egui::RichText::new(glyph).size(18.0);
    if highlighted {
        text = text.color(highlight_color);
    }
    ui.add_sized(
        egui::vec2(32.0, 32.0),
        egui::Button::new(text)
            .frame(false)
            .min_size(egui::vec2(32.0, 32.0)),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .on_hover_text(tooltip)
}

/// Compact glyph button for the quick-action pill with a smooth, pulsing breathing
/// animation effect for high-priority pending actions (like pending rotation save).
pub(crate) fn pill_button_breathing(
    ui: &mut egui::Ui,
    glyph: &str,
    tooltip: &str,
    _colors: &theme::ThemeColors,
) -> egui::Response {
    let time = ui.input(|i| i.time);
    let wave = (time * 3.5).sin() as f32; // ~1.8 second smooth breath cycle
    let breath = wave * 0.5 + 0.5; // 0.0 to 1.0 (breath in, breath out)

    // Breathing glow fill and stroke in warm gold/amber - subtle, elegant, low outer glow
    let fill_alpha = (14.0 + 20.0 * breath) as u8;
    let stroke_alpha = (30.0 + 40.0 * breath) as u8;
    let fill = egui::Color32::from_rgba_unmultiplied(255, 195, 45, fill_alpha);
    let stroke = egui::Stroke::new(
        1.0_f32,
        egui::Color32::from_rgba_unmultiplied(255, 205, 55, stroke_alpha),
    );

    let icon_size = 16.5;
    let icon_color = egui::Color32::from_rgb(255, (195.0 + 35.0 * breath) as u8, 45);

    let (rect, response) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::click());
    let painter = ui.painter();

    // Draw breathing rounded card background & stroke
    painter.rect_filled(rect, 6.0, fill);
    painter.rect_stroke(rect, 6.0, stroke);

    // Draw centered icon
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(icon_size),
        icon_color,
    );

    // Repaint to keep breathing animation smoothly running at 60 FPS
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));

    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tooltip)
}

/// Fit an image of `dims` (width, height) inside `slot`, preserving its aspect
/// ratio and centering it. Returns the draw rect for the fitted image.
pub(crate) fn fit_rect_contain(dims: [usize; 2], slot: egui::Rect) -> egui::Rect {
    let img_w = dims[0].max(1) as f32;
    let img_h = dims[1].max(1) as f32;
    let scale = (slot.width() / img_w).min(slot.height() / img_h);
    let draw_w = img_w * scale;
    let draw_h = img_h * scale;
    let min = egui::pos2(
        slot.center().x - draw_w * 0.5,
        slot.center().y - draw_h * 0.5,
    );
    egui::Rect::from_min_size(min, egui::vec2(draw_w, draw_h))
}

/// UV rect (0..1) that center-crops `dims` to cover `slot`.
pub(crate) fn cover_uv(dims: [usize; 2], slot: egui::Rect) -> egui::Rect {
    let img_w = dims[0].max(1) as f32;
    let img_h = dims[1].max(1) as f32;
    let scale = (slot.width() / img_w).max(slot.height() / img_h);
    let fx = (slot.width() / (img_w * scale)).min(1.0);
    let fy = (slot.height() / (img_h * scale)).min(1.0);
    egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(fx, fy))
}

/// Decode + downscale an image to `max_side` for MultiView folder quad previews.
pub(crate) fn decode_quad_thumb(path: &Path, max_side: u32) -> Option<(Vec<u8>, [usize; 2])> {
    use std::io::Read;
    if let Ok(mut file) = std::fs::File::open(path) {
        let mut prefix = vec![0u8; 128 * 1024];
        if let Ok(n) = file.read(&mut prefix) {
            prefix.truncate(n);
            if let Some(thumb) = viewer::embedded_thumbnail(&prefix, max_side) {
                let dims = [thumb.width() as usize, thumb.height() as usize];
                return Some((thumb.to_rgba8().into_raw(), dims));
            }
        }
    }
    let bytes = std::fs::read(path).ok()?;
    let thumb = viewer::decode_thumbnail(&bytes, max_side).ok()?;
    let dims = [thumb.width() as usize, thumb.height() as usize];
    Some((thumb.to_rgba8().into_raw(), dims))
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct GripResponse {
    pub(crate) dragged: bool,
    pub(crate) drag_delta: egui::Vec2,
    pub(crate) double_clicked: bool,
}

/// Render a tactile drag grip handle.
/// In horizontal mode: shows `ICON_GRIP_VERTICAL` (`⋮⋮`) in a 10x28 slot on the left.
/// In vertical mode: shows `ICON_GRIP_HORIZONTAL` (`···/···`) in a 28x10 slot at the top,
/// rotated naturally across the pill width.
/// Double-clicking resets the panel to its default dock position.
/// The rest of the parent panel remains immune to accidental movement.
pub(crate) fn render_drag_grip(
    ui: &mut egui::Ui,
    vertical: bool,
    colors: &theme::ThemeColors,
) -> GripResponse {
    let (grip_w, grip_h) = if vertical { (28.0, 10.0) } else { (10.0, 28.0) };
    let (grip_rect, grip_response) = ui.allocate_exact_size(
        egui::vec2(grip_w, grip_h),
        egui::Sense::click_and_drag(),
    );
    let grip_response = grip_response
        .on_hover_cursor(egui::CursorIcon::Grab)
        .on_hover_text("Drag to move · Double-click to reset");

    if grip_response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }

    if grip_response.hovered() || grip_response.is_pointer_button_down_on() {
        ui.painter().rect_filled(
            grip_rect.expand(1.0),
            3.0,
            egui::Color32::from_white_alpha(15),
        );
    }

    let grip_color = if grip_response.is_pointer_button_down_on() {
        colors.gold
    } else if grip_response.hovered() {
        colors.primary
    } else {
        colors.muted.linear_multiply(0.8)
    };

    let icon = if vertical {
        crate::icons::ICON_GRIP_HORIZONTAL
    } else {
        crate::icons::ICON_GRIP_VERTICAL
    };

    ui.painter().text(
        grip_rect.center(),
        egui::Align2::CENTER_CENTER,
        icon,
        egui::FontId::proportional(12.0),
        grip_color,
    );

    GripResponse {
        dragged: grip_response.dragged(),
        drag_delta: grip_response.drag_delta(),
        double_clicked: grip_response.double_clicked(),
    }
}
