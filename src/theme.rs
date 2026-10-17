// File: src/theme.rs
// Purpose: Photonio — theme-derived glass panel colors, unified across all
//          floating containers so Light mode doesn't leave dark panels behind.

use eframe::egui;

/// Colors for the glassmorphism panels, computed from the active theme.
pub struct ThemeColors {
    pub dark_mode: bool,
    pub panel_fill: egui::Color32,
    pub border: egui::Color32,
    pub primary: egui::Color32,
    pub muted: egui::Color32,
    /// Fill for "loading"/"failed" filmstrip placeholder tiles.
    pub placeholder: egui::Color32,
    /// Theme-aware gold accent: radiant gold in dark mode, rich dark amber in light mode.
    pub gold: egui::Color32,
}

impl ThemeColors {
    pub fn new(dark_mode: bool, alpha: u8) -> Self {
        if dark_mode {
            Self {
                dark_mode,
                panel_fill: egui::Color32::from_rgba_unmultiplied(20, 20, 25, alpha),
                border: egui::Color32::from_white_alpha(45),
                primary: egui::Color32::WHITE,
                muted: egui::Color32::from_gray(160),
                placeholder: egui::Color32::from_rgba_unmultiplied(30, 32, 42, 220),
                gold: egui::Color32::from_rgb(255, 200, 50),
            }
        } else {
            Self {
                dark_mode,
                panel_fill: egui::Color32::from_rgba_unmultiplied(242, 244, 248, alpha),
                border: egui::Color32::from_black_alpha(35),
                primary: egui::Color32::from_gray(25),
                muted: egui::Color32::from_gray(90),
                placeholder: egui::Color32::from_rgba_unmultiplied(230, 233, 240, 220),
                gold: egui::Color32::from_rgb(178, 118, 10),
            }
        }
    }

    /// The shared glass frame built from these colors.
    pub fn frame(&self) -> egui::Frame {
        egui::Frame::none()
            .fill(self.panel_fill)
            .stroke(egui::Stroke::new(1.0_f32, self.border))
            .rounding(10.0)
            .inner_margin(egui::Margin::symmetric(14.0, 8.0))
            .shadow(egui::Shadow {
                offset: egui::vec2(0.0, 4.0),
                blur: 18.0,
                spread: 0.0,
                color: egui::Color32::from_black_alpha(150),
            })
    }

    /// Apply this theme's text/widget colors to a panel's `Ui`.
    pub fn apply(&self, ui: &mut egui::Ui) {
        let visuals = ui.visuals_mut();
        visuals.widgets.noninteractive.fg_stroke.color = self.primary;
        visuals.widgets.inactive.fg_stroke.color = self.primary;
        visuals.widgets.hovered.fg_stroke.color = if self.dark_mode {
            egui::Color32::WHITE
        } else {
            egui::Color32::BLACK
        };
        visuals.widgets.active.fg_stroke.color = if self.dark_mode {
            egui::Color32::WHITE
        } else {
            egui::Color32::from_gray(25)
        };
        if self.dark_mode {
            visuals.selection.bg_fill = egui::Color32::from_rgba_unmultiplied(255, 200, 50, 45);
            visuals.selection.stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_gray(250));
        } else {
            visuals.selection.bg_fill = self.gold;
            visuals.selection.stroke = egui::Stroke::new(1.0_f32, egui::Color32::WHITE);
        }
    }

    /// Render a glass panel with the theme applied to its contents.
    pub fn panel<R>(
        &self,
        ui: &mut egui::Ui,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::InnerResponse<R> {
        self.frame().show(ui, |ui| {
            self.apply(ui);
            add_contents(ui)
        })
    }

    /// Render a "ghost" button (subtle translucent fill + hairline stroke).
    /// In light mode this avoids the dark charcoal box for modal Close buttons
    /// and brightens on hover; in dark mode it falls back to the default.
    /// Render a primary action button (e.g. "Select Folder", "Save", "Done").
    /// Prominent glass accent, highlighted border, crisp text, and pointing hand cursor.
    pub fn button_primary(
        &self,
        ui: &mut egui::Ui,
        text: impl Into<egui::WidgetText>,
        min_size: egui::Vec2,
        enabled: bool,
    ) -> egui::Response {
        let text = text.into();
        ui.scope(|ui| {
            ui.spacing_mut().button_padding = egui::vec2(12.0, 4.5);
            let visuals = ui.visuals_mut();
            if self.dark_mode {
                visuals.widgets.inactive.bg_fill = egui::Color32::from_white_alpha(32);
                visuals.widgets.inactive.bg_stroke =
                    egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(75));
                visuals.widgets.inactive.fg_stroke.color = egui::Color32::WHITE;

                visuals.widgets.hovered.bg_fill = egui::Color32::from_white_alpha(55);
                visuals.widgets.hovered.bg_stroke =
                    egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(120));
                visuals.widgets.hovered.fg_stroke.color = egui::Color32::WHITE;

                visuals.widgets.active.bg_fill = egui::Color32::from_white_alpha(75);
                visuals.widgets.active.bg_stroke =
                    egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(150));
                visuals.widgets.active.fg_stroke.color = egui::Color32::WHITE;

                visuals.widgets.noninteractive.bg_fill = egui::Color32::from_white_alpha(10);
                visuals.widgets.noninteractive.bg_stroke =
                    egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(20));
                visuals.widgets.noninteractive.fg_stroke.color = egui::Color32::from_white_alpha(45);
            } else {
                let fg = egui::Color32::from_gray(20);
                visuals.widgets.inactive.bg_fill = egui::Color32::from_black_alpha(24);
                visuals.widgets.inactive.bg_stroke =
                    egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(65));
                visuals.widgets.inactive.fg_stroke.color = fg;

                visuals.widgets.hovered.bg_fill = egui::Color32::from_black_alpha(38);
                visuals.widgets.hovered.bg_stroke =
                    egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(95));
                visuals.widgets.hovered.fg_stroke.color = egui::Color32::BLACK;

                visuals.widgets.active.bg_fill = egui::Color32::from_black_alpha(55);
                visuals.widgets.active.bg_stroke =
                    egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(120));
                visuals.widgets.active.fg_stroke.color = egui::Color32::BLACK;

                visuals.widgets.noninteractive.bg_fill = egui::Color32::from_black_alpha(8);
                visuals.widgets.noninteractive.bg_stroke =
                    egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(20));
                visuals.widgets.noninteractive.fg_stroke.color = egui::Color32::from_black_alpha(40);
            }
            let btn = egui::Button::new(text).min_size(min_size).rounding(5.0);
            let resp = ui.add_enabled(enabled, btn);
            if enabled {
                resp.on_hover_cursor(egui::CursorIcon::PointingHand)
            } else {
                resp
            }
        })
        .inner
    }

    /// Render a secondary / ghost action button (e.g. "Cancel", "+ New Folder", "Back to Viewer").
    /// Subtle translucent glass fill, hairline border, smooth hover brightening, and pointing hand cursor.
    pub fn button_secondary(
        &self,
        ui: &mut egui::Ui,
        text: impl Into<egui::WidgetText>,
        min_size: egui::Vec2,
        enabled: bool,
    ) -> egui::Response {
        let text = text.into();
        ui.scope(|ui| {
            ui.spacing_mut().button_padding = egui::vec2(10.0, 4.5);
            let visuals = ui.visuals_mut();
            if self.dark_mode {
                visuals.widgets.inactive.bg_fill = egui::Color32::from_white_alpha(14);
                visuals.widgets.inactive.bg_stroke =
                    egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(35));
                visuals.widgets.inactive.fg_stroke.color = egui::Color32::from_gray(225);

                visuals.widgets.hovered.bg_fill = egui::Color32::from_white_alpha(28);
                visuals.widgets.hovered.bg_stroke =
                    egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(60));
                visuals.widgets.hovered.fg_stroke.color = egui::Color32::WHITE;

                visuals.widgets.active.bg_fill = egui::Color32::from_white_alpha(45);
                visuals.widgets.active.bg_stroke =
                    egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(75));
                visuals.widgets.active.fg_stroke.color = egui::Color32::WHITE;

                visuals.widgets.noninteractive.bg_fill = egui::Color32::from_white_alpha(6);
                visuals.widgets.noninteractive.bg_stroke =
                    egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(18));
                visuals.widgets.noninteractive.fg_stroke.color = egui::Color32::from_white_alpha(40);
            } else {
                let fg = egui::Color32::from_gray(35);
                visuals.widgets.inactive.bg_fill = egui::Color32::from_black_alpha(12);
                visuals.widgets.inactive.bg_stroke =
                    egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(35));
                visuals.widgets.inactive.fg_stroke.color = fg;

                visuals.widgets.hovered.bg_fill = egui::Color32::from_black_alpha(25);
                visuals.widgets.hovered.bg_stroke =
                    egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(55));
                visuals.widgets.hovered.fg_stroke.color = egui::Color32::from_gray(15);

                visuals.widgets.active.bg_fill = egui::Color32::from_black_alpha(40);
                visuals.widgets.active.bg_stroke =
                    egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(70));
                visuals.widgets.active.fg_stroke.color = egui::Color32::from_gray(10);

                visuals.widgets.noninteractive.bg_fill = egui::Color32::from_black_alpha(5);
                visuals.widgets.noninteractive.bg_stroke =
                    egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(15));
                visuals.widgets.noninteractive.fg_stroke.color = egui::Color32::from_black_alpha(35);
            }
            let btn = egui::Button::new(text).min_size(min_size).rounding(5.0);
            let resp = ui.add_enabled(enabled, btn);
            if enabled {
                resp.on_hover_cursor(egui::CursorIcon::PointingHand)
            } else {
                resp
            }
        })
        .inner
    }

    /// Render an accent action button (e.g. "Pin to Quick Access").
    /// Tinted with `accent` color (warm amber/gold), smooth glow on hover, pointing hand cursor.
    pub fn button_accent(
        &self,
        ui: &mut egui::Ui,
        text: impl Into<egui::WidgetText>,
        min_size: egui::Vec2,
        enabled: bool,
        accent: egui::Color32,
    ) -> egui::Response {
        let text = text.into();
        ui.scope(|ui| {
            ui.spacing_mut().button_padding = egui::vec2(12.0, 4.5);
            let visuals = ui.visuals_mut();
            let [r, g, b, _] = accent.to_array();

            visuals.widgets.inactive.bg_fill = egui::Color32::from_rgba_unmultiplied(r, g, b, 24);
            visuals.widgets.inactive.bg_stroke =
                egui::Stroke::new(1.0_f32, egui::Color32::from_rgba_unmultiplied(r, g, b, 70));
            visuals.widgets.inactive.fg_stroke.color = accent;

            visuals.widgets.hovered.bg_fill = if self.dark_mode {
                egui::Color32::from_rgba_unmultiplied(r, g, b, 55)
            } else {
                egui::Color32::from_rgba_unmultiplied(r, g, b, 180)
            };
            visuals.widgets.hovered.bg_stroke =
                egui::Stroke::new(1.0_f32, egui::Color32::from_rgba_unmultiplied(r, g, b, 160));
            visuals.widgets.hovered.fg_stroke.color = egui::Color32::WHITE;

            visuals.widgets.active.bg_fill = if self.dark_mode {
                egui::Color32::from_rgba_unmultiplied(r, g, b, 85)
            } else {
                accent
            };
            visuals.widgets.active.bg_stroke =
                egui::Stroke::new(1.0_f32, egui::Color32::from_rgba_unmultiplied(r, g, b, 200));
            visuals.widgets.active.fg_stroke.color = egui::Color32::WHITE;

            // Noninteractive / selected status (e.g. "✓ Pinned" badge)
            visuals.widgets.noninteractive.bg_fill = if self.dark_mode {
                egui::Color32::from_rgba_unmultiplied(r, g, b, 50)
            } else {
                accent
            };
            visuals.widgets.noninteractive.bg_stroke = if self.dark_mode {
                egui::Stroke::new(1.0_f32, egui::Color32::from_rgba_unmultiplied(r, g, b, 120))
            } else {
                egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(148, 98, 8))
            };
            visuals.widgets.noninteractive.fg_stroke.color = if self.dark_mode {
                egui::Color32::from_gray(245)
            } else {
                egui::Color32::WHITE
            };

            let btn = egui::Button::new(text).min_size(min_size).rounding(5.0);
            let resp = ui.add_enabled(enabled, btn);
            if enabled {
                resp.on_hover_cursor(egui::CursorIcon::PointingHand)
            } else {
                resp
            }
        })
        .inner
    }

    /// Render an accent action button using the theme's gold accent.
    pub fn button_gold(
        &self,
        ui: &mut egui::Ui,
        text: impl Into<egui::WidgetText>,
        min_size: egui::Vec2,
        enabled: bool,
    ) -> egui::Response {
        self.button_accent(ui, text, min_size, enabled, self.gold)
    }

    /// Render a "ghost" button (subtle translucent fill + hairline stroke).
    /// Kept for convenience and backward compatibility.
    pub fn ghost_button(
        &self,
        ui: &mut egui::Ui,
        text: impl Into<egui::WidgetText>,
    ) -> egui::Response {
        self.button_secondary(ui, text, egui::vec2(0.0, 26.0), true)
    }

    /// Render a sleek, frameless navigation icon button (used in multiview grid and folder browser).
    /// Transparent when idle, smooth rounded translucent highlight on hover/press, muted when disabled.
    pub fn nav_icon_button(
        &self,
        ui: &mut egui::Ui,
        icon: &str,
        icon_size: f32,
        size: egui::Vec2,
        enabled: bool,
        tooltip: &str,
    ) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(
            size,
            if enabled { egui::Sense::click() } else { egui::Sense::hover() },
        );

        let painter = ui.painter();
        if enabled {
            if response.is_pointer_button_down_on() {
                let press_fill = if self.dark_mode {
                    egui::Color32::from_white_alpha(35)
                } else {
                    egui::Color32::from_black_alpha(25)
                };
                painter.rect_filled(rect, 4.0, press_fill);
            } else if response.hovered() {
                let hover_fill = if self.dark_mode {
                    egui::Color32::from_white_alpha(20)
                } else {
                    egui::Color32::from_black_alpha(15)
                };
                painter.rect_filled(rect, 4.0, hover_fill);
            }
        }

        let text_color = if enabled {
            if response.hovered() {
                self.primary
            } else if self.dark_mode {
                egui::Color32::from_gray(215)
            } else {
                egui::Color32::from_gray(55)
            }
        } else if self.dark_mode {
            egui::Color32::from_white_alpha(45)
        } else {
            egui::Color32::from_black_alpha(40)
        };

        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            icon,
            egui::FontId::proportional(icon_size),
            text_color,
        );

        if enabled {
            response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tooltip)
        } else {
            response
        }
    }

    /// Render a compact segmented option button (e.g. "Small", "Medium", "Large", "List", "Images", "Media", "All").
    /// In dark mode: selected has a warm gold outline, dark gold glass fill, and crisp white-ish text.
    /// In light mode: selected has an amber fill and crisp white text.
    /// Unselected options have clean, subtle hover and pointing hand cursor.
    pub fn segmented_pill(
        &self,
        ui: &mut egui::Ui,
        text: &str,
        selected: bool,
    ) -> egui::Response {
        let font_id = egui::TextStyle::Button.resolve(ui.style());
        let padding = egui::vec2(7.0, 3.0);
        let galley = ui.painter().layout_no_wrap(text.to_string(), font_id.clone(), egui::Color32::PLACEHOLDER);
        let desired_size = galley.size() + padding * 2.0;
        let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());

        let painter = ui.painter();
        let rounding = egui::Rounding::same(4.0);

        if selected {
            let (fill, stroke, text_color) = if self.dark_mode {
                (
                    egui::Color32::from_rgba_unmultiplied(255, 200, 50, 45),
                    egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(255, 200, 50)),
                    egui::Color32::from_gray(250),
                )
            } else {
                (
                    self.gold,
                    egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(148, 98, 8)),
                    egui::Color32::WHITE,
                )
            };
            painter.rect(rect, rounding, fill, stroke);
            let text_pos = egui::pos2(
                rect.min.x + padding.x,
                rect.min.y + padding.y,
            );
            painter.text(text_pos, egui::Align2::LEFT_TOP, text, font_id, text_color);
        } else {
            if response.hovered() {
                let hover_fill = if self.dark_mode {
                    egui::Color32::from_white_alpha(20)
                } else {
                    egui::Color32::from_black_alpha(15)
                };
                painter.rect_filled(rect, rounding, hover_fill);
            }
            let text_color = if response.hovered() {
                self.primary
            } else {
                self.muted
            };
            let text_pos = egui::pos2(
                rect.min.x + padding.x,
                rect.min.y + padding.y,
            );
            painter.text(text_pos, egui::Align2::LEFT_TOP, text, font_id, text_color);
        }

        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    }

    /// Render a segmented option bound to an enum value.
    pub fn segmented_value<T: PartialEq + Copy>(
        &self,
        ui: &mut egui::Ui,
        current: &mut T,
        option: T,
        text: &str,
    ) -> egui::Response {
        let selected = *current == option;
        let response = self.segmented_pill(ui, text, selected);
        if response.clicked() {
            *current = option;
        }
        response
    }

    /// Render a Settings navigation sidebar tab button.
    /// Perfectly styled for both Dark and Light themes with guaranteed contrast,
    /// smooth hover states, and pointing hand cursor.
    pub fn sidebar_tab(
        &self,
        ui: &mut egui::Ui,
        icon: &str,
        label: &str,
        selected: bool,
        size: egui::Vec2,
    ) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        let painter = ui.painter();
        let rounding = egui::Rounding::same(5.0);

        let (fill, stroke, text_color) = if selected {
            if self.dark_mode {
                (
                    egui::Color32::from_rgba_unmultiplied(255, 200, 50, 45),
                    egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(255, 200, 50)),
                    egui::Color32::from_gray(250),
                )
            } else {
                (
                    self.gold,
                    egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(148, 98, 8)),
                    egui::Color32::WHITE,
                )
            }
        } else if response.is_pointer_button_down_on() {
            let press_fill = if self.dark_mode {
                egui::Color32::from_white_alpha(35)
            } else {
                egui::Color32::from_black_alpha(25)
            };
            (press_fill, egui::Stroke::NONE, self.primary)
        } else if response.hovered() {
            let hover_fill = if self.dark_mode {
                egui::Color32::from_white_alpha(20)
            } else {
                egui::Color32::from_black_alpha(15)
            };
            (hover_fill, egui::Stroke::NONE, self.primary)
        } else {
            (egui::Color32::TRANSPARENT, egui::Stroke::NONE, self.primary)
        };

        if fill != egui::Color32::TRANSPARENT || stroke.width > 0.0 {
            painter.rect(rect, rounding, fill, stroke);
        }

        // Fixed column for icon (centered in a neat box on the left)
        let icon_center = egui::pos2(rect.min.x + 16.0, rect.center().y);
        painter.text(
            icon_center,
            egui::Align2::CENTER_CENTER,
            icon,
            egui::FontId::proportional(13.5),
            text_color,
        );

        // Fixed left-aligned column for label so all tab texts line up vertically
        let label_pos = egui::pos2(rect.min.x + 30.0, rect.center().y);
        painter.text(
            label_pos,
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(13.0),
            text_color,
        );

        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    }
}
