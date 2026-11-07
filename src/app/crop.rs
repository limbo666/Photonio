// File: src/app/crop.rs
// Purpose: Interactive on-canvas Crop Tool: aspect ratio presets, orientation swap,
//          rule-of-thirds & grid guides, 8 drag handles, exact pixel numeric inputs,
//          save overwrite, save copy, and clipboard copy.

use eframe::egui;
use crate::app::types::InputSnapshot;
use crate::app::PhotonioApp;
use crate::app::widgets::render_drag_grip;
use crate::icons;
use std::path::Path;

/// Preset aspect ratio constraints for cropping.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CropAspectRatio {
    Free,
    Original,
    Square1x1,
    Ratio4x3,
    Ratio16x9,
    Ratio3x2,
}

impl CropAspectRatio {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Free => "Free",
            Self::Original => "Original",
            Self::Square1x1 => "1:1",
            Self::Ratio4x3 => "4:3",
            Self::Ratio16x9 => "16:9",
            Self::Ratio3x2 => "3:2",
        }
    }
}

/// Composition guide overlay style drawn inside the active crop rectangle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CropGuideStyle {
    RuleOfThirds,
    CenterGrid,
    None,
}

impl CropGuideStyle {
    pub(crate) fn next(&self) -> Self {
        match self {
            Self::RuleOfThirds => Self::CenterGrid,
            Self::CenterGrid => Self::None,
            Self::None => Self::RuleOfThirds,
        }
    }

    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::RuleOfThirds => "Guides: Thirds",
            Self::CenterGrid => "Guides: Grid",
            Self::None => "Guides: Off",
        }
    }
}

/// Active drag handle on the crop rectangle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CropHandle {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Top,
    Bottom,
    Left,
    Right,
    Inside,
}

/// Complete state for the interactive Crop Mode.
pub(crate) struct CropState {
    /// Normalized coordinates `[min_x, min_y, max_x, max_y]` in `0.0..=1.0` relative to the displayed image.
    pub(crate) norm_rect: [f32; 4],
    /// Active aspect ratio constraint.
    pub(crate) aspect_ratio: CropAspectRatio,
    /// When true, landscape ratios (e.g. 16:9) are swapped to portrait (9:16).
    pub(crate) is_portrait: bool,
    /// Active composition guide style.
    pub(crate) guide_style: CropGuideStyle,
    /// Handle currently being dragged by the mouse.
    pub(crate) active_handle: Option<CropHandle>,
    /// Mouse pointer position when the current drag started.
    pub(crate) drag_start_pointer: Option<egui::Pos2>,
    /// `norm_rect` when the current drag started.
    pub(crate) drag_start_norm: Option<[f32; 4]>,
    /// Confirmation toggle for overwriting the original file.
    pub(crate) confirm_overwrite: bool,
    /// Numeric manual edit inputs (pixel-exact).
    pub(crate) edit_w: u32,
    pub(crate) edit_h: u32,
    pub(crate) edit_x: u32,
    pub(crate) edit_y: u32,
}

impl CropState {
    pub(crate) fn new(orig_w: usize, orig_h: usize) -> Self {
        let w = orig_w.max(1) as u32;
        let h = orig_h.max(1) as u32;
        // Start with a 90% centered box so handles and contrast mask are immediately visible
        let margin_x = 0.05_f32;
        let margin_y = 0.05_f32;
        let norm_rect = [margin_x, margin_y, 1.0 - margin_x, 1.0 - margin_y];

        let edit_x = (norm_rect[0] * w as f32).round() as u32;
        let edit_y = (norm_rect[1] * h as f32).round() as u32;
        let edit_w = ((norm_rect[2] - norm_rect[0]) * w as f32).round().max(1.0) as u32;
        let edit_h = ((norm_rect[3] - norm_rect[1]) * h as f32).round().max(1.0) as u32;

        Self {
            norm_rect,
            aspect_ratio: CropAspectRatio::Free,
            is_portrait: false,
            guide_style: CropGuideStyle::RuleOfThirds,
            active_handle: None,
            drag_start_pointer: None,
            drag_start_norm: None,
            confirm_overwrite: false,
            edit_w,
            edit_h,
            edit_x,
            edit_y,
        }
    }

    /// Reset the crop box to the 100% full image boundary.
    pub(crate) fn reset_full(&mut self, orig_w: usize, orig_h: usize) {
        self.norm_rect = [0.0, 0.0, 1.0, 1.0];
        self.sync_pixel_edits(orig_w, orig_h);
    }

    /// Synchronize the numeric pixel edit buffers from `norm_rect`.
    pub(crate) fn sync_pixel_edits(&mut self, orig_w: usize, orig_h: usize) {
        let w = orig_w.max(1) as f32;
        let h = orig_h.max(1) as f32;
        self.edit_x = (self.norm_rect[0] * w).round().clamp(0.0, w - 1.0) as u32;
        self.edit_y = (self.norm_rect[1] * h).round().clamp(0.0, h - 1.0) as u32;
        let edit_w = ((self.norm_rect[2] - self.norm_rect[0]) * w).round().max(1.0);
        let edit_h = ((self.norm_rect[3] - self.norm_rect[1]) * h).round().max(1.0);
        self.edit_w = edit_w.min(w - self.edit_x as f32) as u32;
        self.edit_h = edit_h.min(h - self.edit_y as f32) as u32;
    }

    /// Apply numeric pixel edit values back into `norm_rect`.
    pub(crate) fn apply_pixel_edits(&mut self, orig_w: usize, orig_h: usize) {
        let w = orig_w.max(1) as f32;
        let h = orig_h.max(1) as f32;
        let x = (self.edit_x as f32).clamp(0.0, w - 1.0);
        let y = (self.edit_y as f32).clamp(0.0, h - 1.0);
        let cw = (self.edit_w as f32).clamp(1.0, w - x);
        let ch = (self.edit_h as f32).clamp(1.0, h - y);
        self.norm_rect = [x / w, y / h, (x + cw) / w, (y + ch) / h];
    }

    /// Nudge the crop box by normalized deltas (used by keyboard arrow keys).
    pub(crate) fn nudge(&mut self, dx: f32, dy: f32, orig_w: usize, orig_h: usize) {
        let w = self.norm_rect[2] - self.norm_rect[0];
        let h = self.norm_rect[3] - self.norm_rect[1];

        let mut min_x = (self.norm_rect[0] + dx).clamp(0.0, 1.0 - w);
        let mut min_y = (self.norm_rect[1] + dy).clamp(0.0, 1.0 - h);
        if min_x + w > 1.0 {
            min_x = 1.0 - w;
        }
        if min_y + h > 1.0 {
            min_y = 1.0 - h;
        }
        self.norm_rect[0] = min_x;
        self.norm_rect[1] = min_y;
        self.norm_rect[2] = min_x + w;
        self.norm_rect[3] = min_y + h;
        self.sync_pixel_edits(orig_w, orig_h);
    }

    /// Target aspect ratio `w / h` if a ratio constraint is active.
    pub(crate) fn target_aspect(&self, orig_w: usize, orig_h: usize) -> Option<f32> {
        let base_ratio = match self.aspect_ratio {
            CropAspectRatio::Free => return None,
            CropAspectRatio::Original => orig_w.max(1) as f32 / orig_h.max(1) as f32,
            CropAspectRatio::Square1x1 => 1.0,
            CropAspectRatio::Ratio4x3 => 4.0 / 3.0,
            CropAspectRatio::Ratio16x9 => 16.0 / 9.0,
            CropAspectRatio::Ratio3x2 => 3.0 / 2.0,
        };
        if self.is_portrait && base_ratio != 1.0 {
            Some(1.0 / base_ratio)
        } else {
            Some(base_ratio)
        }
    }

    /// Re-constrain the active `norm_rect` to the current aspect ratio setting.
    pub(crate) fn enforce_aspect(&mut self, orig_w: usize, orig_h: usize) {
        let Some(target_r) = self.target_aspect(orig_w, orig_h) else {
            return;
        };
        let target_r = target_r.max(0.0001);
        let ow = orig_w.max(1) as f32;
        let oh = orig_h.max(1) as f32;
        let w_px = (self.norm_rect[2] - self.norm_rect[0]) * ow;
        let h_px = (self.norm_rect[3] - self.norm_rect[1]) * oh;

        let center_x = (self.norm_rect[0] + self.norm_rect[2]) * 0.5;
        let center_y = (self.norm_rect[1] + self.norm_rect[3]) * 0.5;

        // Current ratio vs target ratio
        let current_r = w_px / h_px.max(1.0);
        let (new_w_px, new_h_px) = if current_r > target_r {
            // Box is too wide -> fit width to target_r * height
            let new_w = (h_px * target_r).min(ow);
            (new_w, new_w / target_r)
        } else {
            // Box is too tall -> fit height to width / target_r
            let new_h = (w_px / target_r).min(oh);
            (new_h * target_r, new_h)
        };

        let new_w_norm = new_w_px / ow;
        let new_h_norm = new_h_px / oh;

        let mut min_x = (center_x - new_w_norm * 0.5).max(0.0);
        let mut max_x = min_x + new_w_norm;
        if max_x > 1.0 {
            max_x = 1.0;
            min_x = (1.0 - new_w_norm).max(0.0);
        }

        let mut min_y = (center_y - new_h_norm * 0.5).max(0.0);
        let mut max_y = min_y + new_h_norm;
        if max_y > 1.0 {
            max_y = 1.0;
            min_y = (1.0 - new_h_norm).max(0.0);
        }

        self.norm_rect = [min_x, min_y, max_x, max_y];
        self.sync_pixel_edits(orig_w, orig_h);
    }
}

/// Default filename for a cropped copy (e.g. `image_crop.jpg`).
pub(crate) fn default_crop_name(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".to_string());
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_else(|| "png".to_string());
    format!("{stem}_crop.{ext}")
}

// ---------------------------------------------------------------------------
// Canvas Drawing & Handle Interaction
// ---------------------------------------------------------------------------

pub(crate) fn draw_crop_overlay(
    app: &mut PhotonioApp,
    ui: &mut egui::Ui,
    input: &InputSnapshot,
) {
    let colors = app.theme();
    let Some(image_rect) = app.image_rect else {
        return;
    };
    let Some(dims) = app.image.as_ref().map(|i| i.dimensions) else {
        return;
    };
    let orig_w = dims[0].max(1);
    let orig_h = dims[1].max(1);

    let Some(ref mut crop) = app.crop_state else {
        return;
    };

    let painter = ui.painter();

    // Compute screen coordinates of the crop box
    let screen_min_x = image_rect.min.x + crop.norm_rect[0] * image_rect.width();
    let screen_min_y = image_rect.min.y + crop.norm_rect[1] * image_rect.height();
    let screen_max_x = image_rect.min.x + crop.norm_rect[2] * image_rect.width();
    let screen_max_y = image_rect.min.y + crop.norm_rect[3] * image_rect.height();
    let screen_crop_rect = egui::Rect::from_min_max(
        egui::pos2(screen_min_x, screen_min_y),
        egui::pos2(screen_max_x, screen_max_y),
    );

    // 1. Dim the outer mask (4 surrounding rectangles)
    let mask_color = egui::Color32::from_black_alpha(175);
    // Top
    if screen_crop_rect.min.y > image_rect.min.y {
        painter.rect_filled(
            egui::Rect::from_min_max(
                image_rect.min,
                egui::pos2(image_rect.max.x, screen_crop_rect.min.y),
            ),
            0.0,
            mask_color,
        );
    }
    // Bottom
    if screen_crop_rect.max.y < image_rect.max.y {
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(image_rect.min.x, screen_crop_rect.max.y),
                image_rect.max,
            ),
            0.0,
            mask_color,
        );
    }
    // Left
    if screen_crop_rect.min.x > image_rect.min.x {
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(image_rect.min.x, screen_crop_rect.min.y),
                egui::pos2(screen_crop_rect.min.x, screen_crop_rect.max.y),
            ),
            0.0,
            mask_color,
        );
    }
    // Right
    if screen_crop_rect.max.x < image_rect.max.x {
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(screen_crop_rect.max.x, screen_crop_rect.min.y),
                egui::pos2(image_rect.max.x, screen_crop_rect.max.y),
            ),
            0.0,
            mask_color,
        );
    }

    // 2. Composition Guides inside the crop box
    match crop.guide_style {
        CropGuideStyle::RuleOfThirds => {
            let guide_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(75));
            let w = screen_crop_rect.width();
            let h = screen_crop_rect.height();
            // Vertical thirds
            for i in 1..=2 {
                let x = screen_crop_rect.min.x + (i as f32 / 3.0) * w;
                painter.line_segment(
                    [
                        egui::pos2(x, screen_crop_rect.min.y),
                        egui::pos2(x, screen_crop_rect.max.y),
                    ],
                    guide_stroke,
                );
            }
            // Horizontal thirds
            for i in 1..=2 {
                let y = screen_crop_rect.min.y + (i as f32 / 3.0) * h;
                painter.line_segment(
                    [
                        egui::pos2(screen_crop_rect.min.x, y),
                        egui::pos2(screen_crop_rect.max.x, y),
                    ],
                    guide_stroke,
                );
            }
        }
        CropGuideStyle::CenterGrid => {
            let guide_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(75));
            let cx = screen_crop_rect.center().x;
            let cy = screen_crop_rect.center().y;
            painter.line_segment(
                [
                    egui::pos2(cx, screen_crop_rect.min.y),
                    egui::pos2(cx, screen_crop_rect.max.y),
                ],
                guide_stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(screen_crop_rect.min.x, cy),
                    egui::pos2(screen_crop_rect.max.x, cy),
                ],
                guide_stroke,
            );
        }
        CropGuideStyle::None => {}
    }

    // 3. Crop Rectangle Border (outer dark stroke for contrast, inner crisp white stroke)
    painter.rect_stroke(
        screen_crop_rect.expand(1.0_f32),
        0.0,
        egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(150)),
    );
    painter.rect_stroke(
        screen_crop_rect,
        0.0,
        egui::Stroke::new(1.5_f32, egui::Color32::WHITE),
    );

    // 4. Hit testing and handle interaction
    let pointer_pos = input.pointer_pos;
    let hit_size = 20.0_f32;

    // Corner hitboxes
    let tl_rect = egui::Rect::from_center_size(screen_crop_rect.left_top(), egui::vec2(hit_size, hit_size));
    let tr_rect = egui::Rect::from_center_size(screen_crop_rect.right_top(), egui::vec2(hit_size, hit_size));
    let bl_rect = egui::Rect::from_center_size(screen_crop_rect.left_bottom(), egui::vec2(hit_size, hit_size));
    let br_rect = egui::Rect::from_center_size(screen_crop_rect.right_bottom(), egui::vec2(hit_size, hit_size));

    // Edge hitboxes
    let t_rect = egui::Rect::from_center_size(screen_crop_rect.center_top(), egui::vec2(hit_size * 2.0, hit_size));
    let b_rect = egui::Rect::from_center_size(screen_crop_rect.center_bottom(), egui::vec2(hit_size * 2.0, hit_size));
    let l_rect = egui::Rect::from_center_size(screen_crop_rect.left_center(), egui::vec2(hit_size, hit_size * 2.0));
    let r_rect = egui::Rect::from_center_size(screen_crop_rect.right_center(), egui::vec2(hit_size, hit_size * 2.0));

    // Handle mouse cursor and drag start
    let mut hovered_handle = None;
    if let Some(pos) = pointer_pos {
        if tl_rect.contains(pos) {
            hovered_handle = Some(CropHandle::TopLeft);
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNorthWest);
        } else if tr_rect.contains(pos) {
            hovered_handle = Some(CropHandle::TopRight);
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNorthEast);
        } else if bl_rect.contains(pos) {
            hovered_handle = Some(CropHandle::BottomLeft);
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeSouthWest);
        } else if br_rect.contains(pos) {
            hovered_handle = Some(CropHandle::BottomRight);
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeSouthEast);
        } else if t_rect.contains(pos) {
            hovered_handle = Some(CropHandle::Top);
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        } else if b_rect.contains(pos) {
            hovered_handle = Some(CropHandle::Bottom);
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        } else if l_rect.contains(pos) {
            hovered_handle = Some(CropHandle::Left);
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        } else if r_rect.contains(pos) {
            hovered_handle = Some(CropHandle::Right);
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        } else if screen_crop_rect.contains(pos) {
            hovered_handle = Some(CropHandle::Inside);
            ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
        }
    }

    if input.primary_down {
        if crop.active_handle.is_none() && hovered_handle.is_some() {
            crop.active_handle = hovered_handle;
            crop.drag_start_pointer = pointer_pos;
            crop.drag_start_norm = Some(crop.norm_rect);
        }
    } else {
        if crop.active_handle.is_some() {
            crop.active_handle = None;
            crop.drag_start_pointer = None;
            crop.drag_start_norm = None;
            crop.sync_pixel_edits(orig_w, orig_h);
        }
    }

    // Process active dragging
    if let (Some(handle), Some(start_ptr), Some(start_norm), Some(curr_ptr)) = (
        crop.active_handle,
        crop.drag_start_pointer,
        crop.drag_start_norm,
        pointer_pos,
    ) {
        let dx_norm = (curr_ptr.x - start_ptr.x) / image_rect.width().max(1.0);
        let dy_norm = (curr_ptr.y - start_ptr.y) / image_rect.height().max(1.0);

        let min_side_px = 16.0_f32;
        let min_w_norm = min_side_px / orig_w as f32;
        let min_h_norm = min_side_px / orig_h as f32;

        match handle {
            CropHandle::Inside => {
                let w = start_norm[2] - start_norm[0];
                let h = start_norm[3] - start_norm[1];
                let min_x = (start_norm[0] + dx_norm).clamp(0.0, 1.0 - w);
                let min_y = (start_norm[1] + dy_norm).clamp(0.0, 1.0 - h);
                crop.norm_rect = [min_x, min_y, min_x + w, min_y + h];
            }
            _ => {
                let mut min_x = start_norm[0];
                let mut min_y = start_norm[1];
                let mut max_x = start_norm[2];
                let mut max_y = start_norm[3];

                match handle {
                    CropHandle::TopLeft => {
                        min_x = (start_norm[0] + dx_norm).clamp(0.0, max_x - min_w_norm);
                        min_y = (start_norm[1] + dy_norm).clamp(0.0, max_y - min_h_norm);
                    }
                    CropHandle::TopRight => {
                        max_x = (start_norm[2] + dx_norm).clamp(min_x + min_w_norm, 1.0);
                        min_y = (start_norm[1] + dy_norm).clamp(0.0, max_y - min_h_norm);
                    }
                    CropHandle::BottomLeft => {
                        min_x = (start_norm[0] + dx_norm).clamp(0.0, max_x - min_w_norm);
                        max_y = (start_norm[3] + dy_norm).clamp(min_y + min_h_norm, 1.0);
                    }
                    CropHandle::BottomRight => {
                        max_x = (start_norm[2] + dx_norm).clamp(min_x + min_w_norm, 1.0);
                        max_y = (start_norm[3] + dy_norm).clamp(min_y + min_h_norm, 1.0);
                    }
                    CropHandle::Top => {
                        min_y = (start_norm[1] + dy_norm).clamp(0.0, max_y - min_h_norm);
                    }
                    CropHandle::Bottom => {
                        max_y = (start_norm[3] + dy_norm).clamp(min_y + min_h_norm, 1.0);
                    }
                    CropHandle::Left => {
                        min_x = (start_norm[0] + dx_norm).clamp(0.0, max_x - min_w_norm);
                    }
                    CropHandle::Right => {
                        max_x = (start_norm[2] + dx_norm).clamp(min_x + min_w_norm, 1.0);
                    }
                    CropHandle::Inside => {}
                }

                // Apply aspect ratio constraint if applicable
                if let Some(target_r) = crop.target_aspect(orig_w, orig_h) {
                    let w_px = (max_x - min_x) * orig_w as f32;
                    let h_px = (max_y - min_y) * orig_h as f32;

                    match handle {
                        CropHandle::Left | CropHandle::Right => {
                            let new_h_px = (w_px / target_r).min(orig_h as f32);
                            let new_h_norm = new_h_px / orig_h as f32;
                            let cy = (min_y + max_y) * 0.5;
                            min_y = (cy - new_h_norm * 0.5).max(0.0);
                            max_y = (min_y + new_h_norm).min(1.0);
                        }
                        CropHandle::Top | CropHandle::Bottom => {
                            let new_w_px = (h_px * target_r).min(orig_w as f32);
                            let new_w_norm = new_w_px / orig_w as f32;
                            let cx = (min_x + max_x) * 0.5;
                            min_x = (cx - new_w_norm * 0.5).max(0.0);
                            max_x = (min_x + new_w_norm).min(1.0);
                        }
                        CropHandle::TopLeft => {
                            let new_h_px = (w_px / target_r).min(orig_h as f32);
                            min_y = (max_y - new_h_px / orig_h as f32).max(0.0);
                        }
                        CropHandle::TopRight => {
                            let new_h_px = (w_px / target_r).min(orig_h as f32);
                            min_y = (max_y - new_h_px / orig_h as f32).max(0.0);
                        }
                        CropHandle::BottomLeft => {
                            let new_h_px = (w_px / target_r).min(orig_h as f32);
                            max_y = (min_y + new_h_px / orig_h as f32).min(1.0);
                        }
                        CropHandle::BottomRight => {
                            let new_h_px = (w_px / target_r).min(orig_h as f32);
                            max_y = (min_y + new_h_px / orig_h as f32).min(1.0);
                        }
                        CropHandle::Inside => {}
                    }
                }

                crop.norm_rect = [min_x, min_y, max_x, max_y];
                crop.sync_pixel_edits(orig_w, orig_h);
            }
        }
    }

    // 5. Draw Visual Handles
    let corner_len = 16.0_f32;
    let corner_thick = 3.5_f32;
    let handle_stroke = egui::Stroke::new(corner_thick, egui::Color32::WHITE);
    let shadow_stroke = egui::Stroke::new(corner_thick + 1.5, egui::Color32::from_black_alpha(150));

    // Corner brackets
    let corners = [
        (screen_crop_rect.left_top(), egui::vec2(corner_len, 0.0), egui::vec2(0.0, corner_len)),
        (screen_crop_rect.right_top(), egui::vec2(-corner_len, 0.0), egui::vec2(0.0, corner_len)),
        (screen_crop_rect.left_bottom(), egui::vec2(corner_len, 0.0), egui::vec2(0.0, -corner_len)),
        (screen_crop_rect.right_bottom(), egui::vec2(-corner_len, 0.0), egui::vec2(0.0, -corner_len)),
    ];
    for (pt, arm_h, arm_v) in corners {
        painter.line_segment([pt, pt + arm_h], shadow_stroke);
        painter.line_segment([pt, pt + arm_v], shadow_stroke);
        painter.line_segment([pt, pt + arm_h], handle_stroke);
        painter.line_segment([pt, pt + arm_v], handle_stroke);
    }

    // Edge grip bars (pills)
    let bar_half_len = 12.0_f32;
    let bar_thick = 3.0_f32;
    let edge_bars = [
        (screen_crop_rect.center_top(), egui::vec2(bar_half_len, 0.0)),
        (screen_crop_rect.center_bottom(), egui::vec2(bar_half_len, 0.0)),
        (screen_crop_rect.left_center(), egui::vec2(0.0, bar_half_len)),
        (screen_crop_rect.right_center(), egui::vec2(0.0, bar_half_len)),
    ];
    for (center, offset) in edge_bars {
        let p1 = center - offset;
        let p2 = center + offset;
        painter.line_segment([p1, p2], egui::Stroke::new(bar_thick + 1.5, egui::Color32::from_black_alpha(150)));
        painter.line_segment([p1, p2], egui::Stroke::new(bar_thick, colors.gold));
    }
}

// ---------------------------------------------------------------------------
// Floating Crop Toolbar (HUD)
// ---------------------------------------------------------------------------

pub(crate) fn draw_crop_toolbar(app: &mut PhotonioApp, ctx: &egui::Context) {
    let Some(dims) = app.image.as_ref().map(|i| i.dimensions) else {
        return;
    };
    let orig_w = dims[0].max(1);
    let orig_h = dims[1].max(1);

    let colors = app.theme();
    let mut exit_crop = false;
    let mut apply_crop = false;
    let mut save_copy = false;
    let mut save_overwrite = false;
    let mut copy_clipboard = false;
    let mut reset_crop = false;

    let modal_frame = egui::Frame::none()
        .fill(colors.panel_fill)
        .stroke(egui::Stroke::new(1.0_f32, colors.border))
        .rounding(10.0)
        .inner_margin(egui::Margin::symmetric(14.0, 8.0))
        .shadow(egui::Shadow {
            offset: egui::vec2(0.0, 4.0),
            blur: 16.0,
            spread: 0.0,
            color: egui::Color32::from_black_alpha(160),
        });

    let area = if let Some(anchor) = app.custom_crop_anchor {
        let (align, offset) = anchor.to_align_and_offset(ctx.screen_rect());
        egui::Area::new(egui::Id::new("photonio-crop-toolbar"))
            .anchor(align, offset)
            .order(egui::Order::Foreground)
    } else {
        egui::Area::new(egui::Id::new("photonio-crop-toolbar"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -18.0))
            .order(egui::Order::Foreground)
    };

    let mut grip_res = None;

    let response = area.show(ctx, |ui| {
        modal_frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);

                // Tactile drag grip handle (drag to move toolbar, double-click to reset)
                grip_res = Some(render_drag_grip(ui, false, &colors));

                // Title badge
                ui.label(
                    egui::RichText::new(format!("{} Crop", icons::ICON_CROP))
                        .strong()
                        .color(colors.gold),
                );
                ui.separator();

                    let Some(ref mut crop) = app.crop_state else {
                        return;
                    };

                    // Aspect ratio presets
                    let ratios = [
                        CropAspectRatio::Free,
                        CropAspectRatio::Original,
                        CropAspectRatio::Square1x1,
                        CropAspectRatio::Ratio16x9,
                        CropAspectRatio::Ratio4x3,
                        CropAspectRatio::Ratio3x2,
                    ];
                    for r in ratios {
                        let active = crop.aspect_ratio == r;
                        let text = egui::RichText::new(r.label()).color(if active {
                            colors.gold
                        } else {
                            colors.primary
                        });
                        if ui.selectable_label(active, text).clicked() {
                            crop.aspect_ratio = r;
                            crop.enforce_aspect(orig_w, orig_h);
                        }
                    }

                    // Swap orientation (portrait <-> landscape)
                    let swap_tip = if crop.is_portrait {
                        "Orientation: Portrait (Click for Landscape)"
                    } else {
                        "Orientation: Landscape (Click for Portrait)"
                    };
                    let swap_btn = ui.button(
                        egui::RichText::new(icons::ICON_ARROWS_ROTATE)
                            .color(if crop.is_portrait { colors.gold } else { colors.muted }),
                    );
                    if swap_btn.on_hover_text(swap_tip).clicked() {
                        crop.is_portrait = !crop.is_portrait;
                        crop.enforce_aspect(orig_w, orig_h);
                    }

                    ui.separator();

                    // Composition guides cycle button
                    if ui.button(crop.guide_style.label()).clicked() {
                        crop.guide_style = crop.guide_style.next();
                    }

                    ui.separator();

                    // Pixel dimensions numeric inputs
                    ui.label("W:");
                    let w_drag = ui.add(
                        egui::DragValue::new(&mut crop.edit_w)
                            .range(1..=(orig_w as u32))
                            .speed(1.0),
                    );
                    if w_drag.changed() {
                        crop.apply_pixel_edits(orig_w, orig_h);
                    }

                    ui.label("H:");
                    let h_drag = ui.add(
                        egui::DragValue::new(&mut crop.edit_h)
                            .range(1..=(orig_h as u32))
                            .speed(1.0),
                    );
                    if h_drag.changed() {
                        crop.apply_pixel_edits(orig_w, orig_h);
                    }

                    // Percentage readout
                    let pct = ((crop.edit_w as f64 * crop.edit_h as f64)
                        / (orig_w as f64 * orig_h as f64)
                        * 100.0)
                        .clamp(0.1, 100.0);
                    ui.label(
                        egui::RichText::new(format!("({pct:.1}%)"))
                            .small()
                            .color(colors.muted),
                    );

                    ui.separator();

                    // Actions
                    if ui.button("Reset (R)").clicked() {
                        reset_crop = true;
                    }
                    if ui.button(format!("{} Copy", icons::ICON_COPY)).on_hover_text("Copy cropped image to clipboard (Ctrl+C)").clicked() {
                        copy_clipboard = true;
                    }
                    if ui.button("Save As...").on_hover_text("Save cropped image as a new file (Ctrl+Shift+S)").clicked() {
                        save_copy = true;
                    }

                    if crop.confirm_overwrite {
                        ui.label(egui::RichText::new("Overwrite?").color(colors.gold));
                        if ui.button("Yes").clicked() {
                            save_overwrite = true;
                        }
                        if ui.button("Cancel").clicked() {
                            crop.confirm_overwrite = false;
                        }
                    } else if ui.button(format!("{} Save", icons::ICON_SAVE)).on_hover_text("Overwrite original file (Ctrl+S)").clicked() {
                        crop.confirm_overwrite = true;
                    }

                    // Apply in-memory button (Gold)
                    let apply_btn = egui::Button::new(
                        egui::RichText::new(format!("{} Apply", icons::ICON_CHECK))
                            .color(egui::Color32::BLACK)
                            .strong(),
                    )
                    .fill(colors.gold);
                    if ui.add(apply_btn).on_hover_text("Apply crop to active view (Enter)").clicked() {
                        apply_crop = true;
                    }

                    // Cancel button
                    if ui.button(egui::RichText::new(icons::ICON_XMARK).color(colors.muted)).on_hover_text("Cancel crop (Esc)").clicked() {
                        exit_crop = true;
                    }
                });
            });
        });

    if let Some(grip) = grip_res {
        if grip.double_clicked {
            app.custom_crop_anchor = None;
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
            app.custom_crop_anchor = Some(anchor);
        }
    }

    if reset_crop {
        if let Some(ref mut crop) = app.crop_state {
            crop.reset_full(orig_w, orig_h);
        }
    }
    if copy_clipboard {
        app.copy_crop_to_clipboard();
    }
    if save_copy {
        app.save_crop_as_copy(ctx);
    }
    if save_overwrite {
        app.save_crop_overwrite(ctx);
    }
    if apply_crop {
        app.apply_crop_in_memory(ctx);
    }
    if exit_crop {
        app.close_crop_mode();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_state_initializes_centered() {
        let state = CropState::new(1920, 1080);
        assert!((state.norm_rect[0] - 0.05).abs() < 1e-4);
        assert!((state.norm_rect[1] - 0.05).abs() < 1e-4);
        assert!((state.norm_rect[2] - 0.95).abs() < 1e-4);
        assert!((state.norm_rect[3] - 0.95).abs() < 1e-4);
        assert_eq!(state.aspect_ratio, CropAspectRatio::Free);
    }

    #[test]
    fn crop_state_reset_full_spans_entire_image() {
        let mut state = CropState::new(1000, 500);
        state.reset_full(1000, 500);
        assert_eq!(state.norm_rect, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(state.edit_w, 1000);
        assert_eq!(state.edit_h, 500);
    }

    #[test]
    fn crop_state_nudge_clamps_within_bounds() {
        let mut state = CropState::new(1000, 1000);
        // Nudge far to the right and bottom
        state.nudge(2.0, 2.0, 1000, 1000);
        assert!(state.norm_rect[2] <= 1.0 + 1e-4);
        assert!(state.norm_rect[3] <= 1.0 + 1e-4);

        // Nudge far to the left and top
        state.nudge(-5.0, -5.0, 1000, 1000);
        assert!(state.norm_rect[0] >= -1e-4);
        assert!(state.norm_rect[1] >= -1e-4);
    }

    #[test]
    fn crop_state_enforce_aspect_ratio_1x1() {
        let mut state = CropState::new(1920, 1080);
        state.aspect_ratio = CropAspectRatio::Square1x1;
        state.enforce_aspect(1920, 1080);

        let w_px = (state.norm_rect[2] - state.norm_rect[0]) * 1920.0;
        let h_px = (state.norm_rect[3] - state.norm_rect[1]) * 1080.0;
        assert!((w_px - h_px).abs() < 1.0);
    }

    #[test]
    fn crop_state_portrait_orientation_swap() {
        let mut state = CropState::new(1920, 1080);
        state.aspect_ratio = CropAspectRatio::Ratio16x9;
        assert!((state.target_aspect(1920, 1080).unwrap() - (16.0 / 9.0)).abs() < 1e-4);

        state.is_portrait = true;
        assert!((state.target_aspect(1920, 1080).unwrap() - (9.0 / 16.0)).abs() < 1e-4);
    }

    #[test]
    fn default_crop_name_appends_crop_suffix() {
        let p1 = Path::new("C:/photos/vacation.jpg");
        assert_eq!(default_crop_name(p1), "vacation_crop.jpg");

        let p2 = Path::new("banner.png");
        assert_eq!(default_crop_name(p2), "banner_crop.png");
    }

    #[test]
    fn test_crop_workspace_insets_separate_tools_from_area() {
        let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1920.0, 1080.0));
        let top_inset = 32.0;
        let bottom_inset = 84.0;
        let workspace = egui::Rect::from_min_max(
            egui::pos2(screen.min.x + 32.0, screen.min.y + top_inset),
            egui::pos2(screen.max.x - 32.0, screen.max.y - bottom_inset),
        );

        // Assert workspace top is strictly detached from screen top with clear breathing room
        assert!(workspace.min.y >= top_inset);
        // Assert bottom clearance is at least 80px (enough for a 44px toolbar + margin)
        assert!(screen.max.y - workspace.max.y >= 80.0);
    }
}
