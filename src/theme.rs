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
            }
        } else {
            Self {
                dark_mode,
                panel_fill: egui::Color32::from_rgba_unmultiplied(242, 244, 248, alpha),
                border: egui::Color32::from_black_alpha(35),
                primary: egui::Color32::from_gray(25),
                muted: egui::Color32::from_gray(90),
                placeholder: egui::Color32::from_rgba_unmultiplied(230, 233, 240, 220),
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
        visuals.override_text_color = Some(self.primary);
        visuals.widgets.noninteractive.fg_stroke.color = self.primary;
        visuals.widgets.inactive.fg_stroke.color = self.primary;
        visuals.widgets.hovered.fg_stroke.color = self.primary;
        visuals.widgets.active.fg_stroke.color = self.primary;
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
    pub fn ghost_button(
        &self,
        ui: &mut egui::Ui,
        text: impl Into<egui::WidgetText>,
    ) -> egui::Response {
        if self.dark_mode {
            return ui.button(text);
        }
        ui.scope(|ui| {
            let visuals = ui.visuals_mut();
            let fg = egui::Color32::from_gray(30);
            visuals.widgets.inactive.bg_fill = egui::Color32::from_black_alpha(15);
            visuals.widgets.inactive.bg_stroke =
                egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(40));
            visuals.widgets.inactive.fg_stroke.color = fg;
            visuals.widgets.hovered.bg_fill = egui::Color32::from_black_alpha(30);
            visuals.widgets.hovered.bg_stroke =
                egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(55));
            visuals.widgets.hovered.fg_stroke.color = fg;
            visuals.widgets.active.bg_fill = egui::Color32::from_black_alpha(45);
            visuals.widgets.active.bg_stroke =
                egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(70));
            visuals.widgets.active.fg_stroke.color = fg;
            ui.button(text)
        })
        .inner
    }
}
