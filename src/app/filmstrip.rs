// File: src/app/filmstrip.rs
// Purpose: 4-way dockable thumbnail filmstrip bar and circular wrap navigation.

use eframe::egui;
use crate::app::widgets::fit_rect_contain;
use crate::app::PhotonioApp;
use crate::config;

pub(crate) fn draw_filmstrip(app: &mut PhotonioApp, ctx: &egui::Context) {
    let Some(opacity) = app.overlay_opacity() else {
        return;
    };
    if !app.config.show_filmstrip || app.files.is_empty() {
        return;
    }
    let colors = app.theme();
    let len = app.files.len();

    // Circular wrap: `half` before + current + `half` after, looping around
    // the ends. Folders smaller than the configured window show every image.
    let (count, half) = app.filmstrip_window();
    let indices: Vec<usize> = if len >= count {
        (-half..=half)
            .map(|off| (app.current_index as isize + off).rem_euclid(len as isize) as usize)
            .collect()
    } else {
        (0..len).collect()
    };
    let mut clicked: Option<usize> = None;

    // Dock position determines the anchor, offset, and layout direction.
    let (anchor, offset, vertical) = match app.config.filmstrip_position {
        config::FilmstripPosition::Bottom => {
            (egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -64.0), false)
        }
        config::FilmstripPosition::Top => {
            (egui::Align2::CENTER_TOP, egui::vec2(0.0, 64.0), false)
        }
        config::FilmstripPosition::Left => {
            (egui::Align2::LEFT_CENTER, egui::vec2(64.0, 0.0), true)
        }
        config::FilmstripPosition::Right => {
            (egui::Align2::RIGHT_CENTER, egui::vec2(-64.0, 0.0), true)
        }
    };

    egui::Area::new(egui::Id::new("filmstrip"))
        .anchor(anchor, offset)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.set_opacity(opacity);
            colors.panel(ui, |ui| {
                if vertical {
                    ui.vertical(|ui| {
                        for idx in indices {
                            if filmstrip_tile(app, ui, idx) == Some(idx) {
                                clicked = Some(idx);
                            }
                        }
                    });
                } else {
                    ui.horizontal(|ui| {
                        for idx in indices {
                            if filmstrip_tile(app, ui, idx) == Some(idx) {
                                clicked = Some(idx);
                            }
                        }
                    });
                }
            });
        });

    if let Some(idx) = clicked {
        app.navigate_to(idx, ctx);
    }
}

/// Draw one filmstrip tile at the current cursor position; returns the
/// index if the tile was clicked.
pub(crate) fn filmstrip_tile(app: &PhotonioApp, ui: &mut egui::Ui, idx: usize) -> Option<usize> {
    let colors = app.theme();
    let size = 72.0;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click_and_drag());
    let painter = ui.painter();

    // Uniform rounded tile background behind every state.
    painter.rect_filled(rect, 6.0, colors.placeholder);

    match app.thumb_cache.get(&idx) {
        Some(thumb) => {
            // Fit the thumbnail inside the slot, preserving aspect ratio.
            let img_rect = fit_rect_contain(thumb.dims, rect);
            painter.image(
                thumb.texture.id(),
                img_rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        None if app.failed_thumb.contains(&idx) => {
            // Definitively failed to decode: subtle placeholder mark.
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "✕",
                egui::FontId::proportional(18.0),
                colors.muted,
            );
        }
        None => {
            // Still decoding — background only.
        }
    }

    if idx == app.current_index {
        painter.rect_stroke(
            rect,
            6.0,
            egui::Stroke::new(2.0_f32, colors.gold),
        );
    } else if response.hovered() {
        painter.rect_stroke(rect, 6.0, egui::Stroke::new(1.0_f32, colors.muted));
    }

    let has_incoming = ui.input(|i| !i.raw.hovered_files.is_empty() || !i.raw.dropped_files.is_empty());
    if !has_incoming && response.drag_started() {
        if let Some(path) = app.files.get(idx).cloned() {
            crate::platform::start_drag_files(&[path]);
            ui.ctx().request_repaint();
        }
        None
    } else if response.clicked() {
        Some(idx)
    } else {
        None
    }
}
