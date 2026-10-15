// File: src/about.rs
// Purpose: Photonio — the "About" modal as a standalone, fixed-size, centered
//          egui::Window. Links are centered manually: egui 0.28 left-aligns
//          every horizontal layout regardless of `main_align`, so we measure
//          the text and add leading space to center the row ourselves.

use eframe::egui::{
    self, Align2, Color32, RichText, Stroke, TextureHandle, Vec2, Window,
};

use crate::theme::ThemeColors;

pub struct AboutDialog;

impl AboutDialog {
    pub fn show(
        ctx: &egui::Context,
        open: &mut bool,
        logo_texture: Option<&TextureHandle>,
        glass_alpha: u8,
        dark_mode: bool,
    ) {
        if !*open {
            return;
        }
        let colors = ThemeColors::new(dark_mode, glass_alpha);

        Window::new("photonio_about_modal_v2")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(Vec2::new(340.0, 320.0))
            .frame(
                egui::Frame::none()
                    .fill(colors.panel_fill)
                    .rounding(12.0)
                    .stroke(Stroke::new(1.0_f32, colors.border))
                    .shadow(egui::Shadow {
                        offset: Vec2::ZERO,
                        blur: 24.0,
                        spread: 4.0,
                        color: Color32::from_black_alpha(200),
                    })
                    .inner_margin(egui::Margin::symmetric(24.0, 20.0)),
            )
            .show(ctx, |ui| {
                ui.set_width(340.0);
                ui.set_height(320.0);
                colors.apply(ui);

                ui.vertical_centered(|ui| {
                    // 1. Clean logo (no rings or borders).
                    if let Some(texture) = logo_texture {
                        ui.image(egui::load::SizedTexture::new(
                            texture.id(),
                            Vec2::new(88.0, 88.0),
                        ));
                    }

                    ui.add_space(8.0);

                    // 2. Title & version.
                    ui.heading(
                        RichText::new("Photonio Image Viewer")
                            .strong()
                            .color(colors.primary),
                    );
                    ui.label(
                        RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                            .size(12.0)
                            .color(colors.muted),
                    );

                    ui.add_space(4.0);

                    // 3. Tagline.
                    ui.label(
                        RichText::new("A fast, lightweight vibe coding project built with Rust.")
                            .italics()
                            .size(12.0)
                            .color(colors.muted),
                    );

                    ui.add_space(8.0);

                    // 4. Author.
                    ui.label(
                        RichText::new("Created by Nikos Georgousis")
                            .size(13.0)
                            .color(colors.primary),
                    );

                    ui.add_space(4.0);

                    // 5. Links — one centered row, two clickable parts.
                    centered_links_row(ui, &colors);

                    ui.add_space(4.0);

                    // 6. License — a single centered clickable block.
                    ui.hyperlink_to(
                        "Released under the Apache 2.0 License",
                        "https://www.apache.org/licenses/LICENSE-2.0",
                    );

                    ui.add_space(6.0);

                    // 6b. Icon attribution.
                    ui.label(
                        RichText::new(
                            "Icons powered by Font Awesome 7 Free (CC BY 4.0 / SIL OFL 1.1)",
                        )
                        .size(11.0)
                        .color(colors.muted),
                    );

                    ui.add_space(12.0);

                    // 7. Close button.
                    if colors
                        .ghost_button(ui, RichText::new("   Close   ").size(13.0))
                        .clicked()
                    {
                        *open = false;
                    }
                });
            });
    }
}

/// Render "www.georgousis.info • GitHub" as one centered row with two clickable
/// parts. egui 0.28 left-aligns horizontal layouts, so we measure the exact
/// text width and pad the left side to center it.
fn centered_links_row(ui: &mut egui::Ui, colors: &ThemeColors) {
    let body_font = egui::FontId::proportional(14.0);

    let (site_w, bullet_w, github_w) = ui.fonts(|f| {
        let m = |text: &str| {
            f.layout_no_wrap(text.to_owned(), body_font.clone(), Color32::WHITE)
                .size()
                .x
        };
        (m("www.georgousis.info"), m("•"), m("GitHub"))
    });

    // Two item-spacing gaps of 8.0 between the three parts.
    let total = site_w + bullet_w + github_w + 16.0;
    let offset = ((ui.available_width() - total) / 2.0).max(0.0);

    ui.horizontal(|ui| {
        ui.add_space(offset);
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.hyperlink_to("www.georgousis.info", "https://www.georgousis.info");
        ui.label(RichText::new("•").color(colors.muted));
        ui.hyperlink_to("GitHub", "https://github.com/limbo666/Photonio");
    });
}
