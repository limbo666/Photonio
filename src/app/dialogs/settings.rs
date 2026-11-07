// File: src/app/dialogs/settings.rs
// Purpose: Two-pane Settings modal and configuration cards.

use eframe::egui;
use crate::app::types::{SettingsTab, ERROR_COLOR, SUCCESS_COLOR, WARN_COLOR};
use crate::app::widgets::settings_card;
use crate::app::PhotonioApp;
use crate::{config, icons, platform, theme};

/// Map the two Appearance toggles (light theme, glass backdrop) back onto the
/// single three-value `CanvasBackground` enum. Light is always solid; dark can
/// be translucent glass or an opaque solid color.
pub(crate) fn background_from(light: bool, glass: bool) -> config::CanvasBackground {
    if light {
        config::CanvasBackground::Light
    } else if glass {
        config::CanvasBackground::Glass
    } else {
        config::CanvasBackground::Dark
    }
}

pub(crate) fn draw_settings(app: &mut PhotonioApp, ctx: &egui::Context) {
    if !app.show_settings {
        return;
    }
    let mut changed = false;
    let mut register_clicked = false;
    let mut folder_menu_clicked = false;
    let mut clean_clicked = false;
    let mut open_default_apps_clicked = false;
    let mut reset_clicked = false;
    let mut done_clicked = false;
    let mut open = app.show_settings;
    let colors = app.theme();

    // Theme-aware glassmorphic frame for the modal window.
    let modal_frame = egui::Frame::none()
        .fill(colors.panel_fill)
        .stroke(egui::Stroke::new(1.0_f32, colors.border))
        .rounding(10.0)
        .inner_margin(egui::Margin::symmetric(10.0, 10.0))
        .shadow(egui::Shadow {
            offset: egui::vec2(0.0, 4.0),
            blur: 18.0,
            spread: 0.0,
            color: egui::Color32::from_black_alpha(150),
        });

    egui::Window::new(
        egui::RichText::new(format!("{}  Settings", icons::ICON_GEAR)).size(13.0),
    )
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .fixed_size(egui::vec2(620.0, 465.0))
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .frame(modal_frame)
        .show(ctx, |ui| {
            // Enforce total inner bounds so content can never balloon or clip.
            let inner_size = egui::vec2(600.0, 410.0);
            ui.set_min_size(inner_size);
            ui.set_max_size(inner_size);

            let footer_height = 36.0;
            let separator_height = 9.0;
            let content_height = inner_size.y - footer_height - separator_height;

            ui.horizontal_top(|ui| {
                // Left navigation sidebar rail (fixed width).
                ui.vertical(|ui| {
                    ui.set_width(140.0);
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 3.0);
                    let tabs = [
                        (SettingsTab::Appearance, icons::ICON_PALETTE, "Appearance"),
                        (SettingsTab::Filmstrip, icons::ICON_FILMSTRIP, "Filmstrip"),
                        (SettingsTab::QuickPills, icons::ICON_BOLT, "Quick-Pills"),
                        (SettingsTab::Behavior, icons::ICON_GEAR, "Behavior"),
                        (SettingsTab::Integration, icons::ICON_PUZZLE, "Integration"),
                    ];
                    for (tab, icon, label) in tabs {
                        let selected = app.settings_tab == tab;
                        let response = colors.sidebar_tab(
                            ui,
                            icon,
                            label,
                            selected,
                            egui::vec2(135.0, 30.0),
                        );
                        if response.clicked() {
                            app.settings_tab = tab;
                        }
                    }
                });

                // Vertical divider between the rail and the content pane.
                ui.separator();

                // Right content pane: scrollable, strictly top-down.
                egui::ScrollArea::vertical()
                    .id_source("settings-content")
                    .max_height(content_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                            match app.settings_tab {
                                SettingsTab::Appearance => {
                                    changed |= settings_appearance(app, ui, &colors);
                                }
                                SettingsTab::Filmstrip => {
                                    changed |= settings_filmstrip(app, ui, &colors);
                                }
                                SettingsTab::QuickPills => {
                                    changed |= settings_quick_pills(app, ui, &colors);
                                }
                                SettingsTab::Behavior => {
                                    changed |= settings_behavior(app, ui, &colors);
                                }
                                SettingsTab::Integration => {
                                    let (reg, folder, clean, apps) =
                                        settings_integration(app, ui, &colors);
                                    register_clicked |= reg;
                                    folder_menu_clicked |= folder;
                                    clean_clicked |= clean;
                                    open_default_apps_clicked |= apps;
                                }
                            }
                        });
                    });
            });

            ui.separator();

            // Bottom action bar (fixed height): reset left, Done right.
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 36.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let reset_btn = colors.button_secondary(
                        ui,
                        format!("{} Reset to Defaults", icons::ICON_TRASH),
                        egui::vec2(150.0, 28.0),
                        true,
                    );
                    if reset_btn.clicked() {
                        reset_clicked = true;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let done_btn = colors.button_primary(
                            ui,
                            format!("{} Done", icons::ICON_CHECK),
                            egui::vec2(95.0, 28.0),
                            true,
                        );
                        if done_btn.clicked() {
                            done_clicked = true;
                        }
                    });
                },
            );
        });

    app.show_settings = open;
    if done_clicked {
        app.show_settings = false;
    }

    if changed {
        app.save_config();
    }
    if register_clicked {
        app.refresh_registration();
    }
    if folder_menu_clicked {
        app.toggle_folder_context_menu();
    }
    if clean_clicked {
        app.unregister_all();
    }
    if open_default_apps_clicked {
        platform::open_default_apps_settings();
    }
    if reset_clicked {
        reset_settings_to_defaults(app);
    }
}

/// Reset every setting to its default while preserving session memory
/// (last directory/file and window geometry).
pub(crate) fn reset_settings_to_defaults(app: &mut PhotonioApp) {
    let dir = app.config.last_directory.clone();
    let file = app.config.last_file_path.clone();
    let pos = app.config.window_pos;
    let size = app.config.window_size;
    let launch_count = app.config.launch_count;
    app.config = config::Config::default();
    app.config.last_directory = dir;
    app.config.last_file_path = file;
    app.config.window_pos = pos;
    app.config.window_size = size;
    app.config.launch_count = launch_count;
    app.save_config();
    app.show_success("Settings reset to defaults");
}

/// Settings → Appearance: theme, backdrop, glass tint, HUD, overlay fade.
pub(crate) fn settings_appearance(app: &mut PhotonioApp, ui: &mut egui::Ui, colors: &theme::ThemeColors) -> bool {
    let mut changed = false;

    // Card 1: Theme & Background.
    settings_card(ui, colors, Some("Theme & Background"), |ui| {
        let mut light = app.config.canvas_background == config::CanvasBackground::Light;
        let mut glass = app.config.canvas_background == config::CanvasBackground::Glass;

        egui::Grid::new("appearance_theme_grid")
            .num_columns(2)
            .spacing(egui::vec2(16.0, 8.0))
            .show(ui, |ui| {
                ui.label("Theme:");
                ui.horizontal(|ui| {
                    if colors.segmented_value(ui, &mut light, false, "Dark").clicked() {
                        changed = true;
                    }
                    if colors.segmented_value(ui, &mut light, true, "Light").clicked() {
                        changed = true;
                    }
                });
                ui.end_row();

                ui.label("Canvas Background:");
                ui.add_enabled_ui(!light, |ui| {
                    ui.horizontal(|ui| {
                        if colors.segmented_value(ui, &mut glass, true, "Translucent Glass").clicked() {
                            changed = true;
                        }
                        if colors.segmented_value(ui, &mut glass, false, "Solid Color").clicked() {
                            changed = true;
                        }
                    });
                });
                ui.end_row();

                ui.label("Glass Opacity:");
                changed |= ui
                    .add(egui::Slider::new(&mut app.config.glass_opacity, 100..=255))
                    .changed();
                ui.end_row();
            });

        // Commit the two toggles to the single backdrop enum.
        let background = background_from(light, glass);
        if background != app.config.canvas_background {
            app.config.canvas_background = background;
            changed = true;
        }
    });
    ui.add_space(6.0);

    // Card 2: Screen Overlays.
    settings_card(ui, colors, Some("Screen Overlays"), |ui| {
        egui::Grid::new("appearance_overlays_grid")
            .num_columns(2)
            .spacing(egui::vec2(16.0, 8.0))
            .show(ui, |ui| {
                ui.label("Navigation HUD:");
                ui.horizontal(|ui| {
                    if colors.segmented_value(ui, &mut app.config.hud_position, config::HudPosition::Bottom, "Bottom").clicked() {
                        changed = true;
                    }
                    if colors.segmented_value(ui, &mut app.config.hud_position, config::HudPosition::Top, "Top").clicked() {
                        changed = true;
                    }
                    if colors.segmented_value(ui, &mut app.config.hud_position, config::HudPosition::Hidden, "Hidden").clicked() {
                        changed = true;
                    }
                });
                ui.end_row();
            });

        ui.add_space(4.0);
        changed |= ui
            .checkbox(&mut app.config.auto_fade_overlays, "Auto-fade overlays when idle")
            .changed();
        if app.config.auto_fade_overlays {
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.label("Fade delay:");
                changed |= ui
                    .add(egui::Slider::new(&mut app.config.fade_delay_secs, 1..=10).suffix(" s"))
                    .changed();
            });
        }
    });
    changed
}

/// Settings → Filmstrip: master toggle, dock, and thumbnail count.
pub(crate) fn settings_filmstrip(app: &mut PhotonioApp, ui: &mut egui::Ui, colors: &theme::ThemeColors) -> bool {
    let mut changed = false;

    // Card 1: master toggle.
    settings_card(ui, colors, Some("Thumbnail Filmstrip"), |ui| {
        changed |= ui
            .checkbox(&mut app.config.show_filmstrip, "Show Thumbnail Filmstrip (T)")
            .changed();
    });
    ui.add_space(6.0);

    // Card 2: Layout & Options (enabled when the filmstrip is on).
    settings_card(ui, colors, Some("Layout & Capacity"), |ui| {
        ui.add_enabled_ui(app.config.show_filmstrip, |ui| {
            egui::Grid::new("filmstrip_layout_grid")
                .num_columns(2)
                .spacing(egui::vec2(16.0, 8.0))
                .show(ui, |ui| {
                    ui.label("Dock position:");
                    ui.horizontal(|ui| {
                        if colors.segmented_value(ui, &mut app.config.filmstrip_position, config::FilmstripPosition::Bottom, "Bottom").clicked() {
                            changed = true;
                        }
                        if colors.segmented_value(ui, &mut app.config.filmstrip_position, config::FilmstripPosition::Top, "Top").clicked() {
                            changed = true;
                        }
                        if colors.segmented_value(ui, &mut app.config.filmstrip_position, config::FilmstripPosition::Left, "Left").clicked() {
                            changed = true;
                        }
                        if colors.segmented_value(ui, &mut app.config.filmstrip_position, config::FilmstripPosition::Right, "Right").clicked() {
                            changed = true;
                        }
                    });
                    ui.end_row();

                    ui.label("Visible Thumbnails:");
                    ui.horizontal(|ui| {
                        for &count in &[7usize, 9, 11] {
                            if colors.segmented_value(ui, &mut app.config.filmstrip_count, count, &count.to_string()).clicked() {
                                changed = true;
                            }
                        }
                    });
                    ui.end_row();
                });
        });
    });
    changed
}

/// Settings → Quick-Pills: master toggle, layout, and per-button visibility.
pub(crate) fn settings_quick_pills(app: &mut PhotonioApp, ui: &mut egui::Ui, colors: &theme::ThemeColors) -> bool {
    let mut changed = false;

    // Card 1: master toggle.
    settings_card(ui, colors, Some("Quick-Action Pill"), |ui| {
        changed |= ui
            .checkbox(&mut app.config.quick_pill_enabled, "Enable Top-Right Quick-Action Pill")
            .changed();
    });
    ui.add_space(6.0);

    // Card 2: Layout & button selection.
    settings_card(ui, colors, Some("Layout & Visible Buttons"), |ui| {
        ui.add_enabled_ui(app.config.quick_pill_enabled, |ui| {
            egui::Grid::new("quick_pill_orientation_grid")
                .num_columns(2)
                .spacing(egui::vec2(16.0, 8.0))
                .show(ui, |ui| {
                    ui.label("Orientation:");
                    ui.horizontal(|ui| {
                        if colors.segmented_value(ui, &mut app.config.quick_pill_orientation, config::Orientation::Horizontal, "Horizontal").clicked() {
                            changed = true;
                        }
                        if colors.segmented_value(ui, &mut app.config.quick_pill_orientation, config::Orientation::Vertical, "Vertical").clicked() {
                            changed = true;
                        }
                    });
                    ui.end_row();
                });

            ui.add_space(4.0);
            ui.label(egui::RichText::new("Action Buttons:").size(12.0).color(colors.muted));
            egui::Grid::new("quick_pill_buttons_grid")
                .num_columns(2)
                .spacing(egui::vec2(24.0, 6.0))
                .show(ui, |ui| {
                    changed |= ui.checkbox(&mut app.config.pill_open_file, "Open File").changed();
                    changed |= ui.checkbox(&mut app.config.pill_open_folder, "Open Folder").changed();
                    ui.end_row();
                    changed |= ui.checkbox(&mut app.config.pill_save_as, "Save As...").changed();
                    changed |= ui.checkbox(&mut app.config.pill_print, "Print...").changed();
                    ui.end_row();
                    changed |= ui.checkbox(&mut app.config.pill_share, "Share...").changed();
                    changed |= ui.checkbox(&mut app.config.pill_filmstrip, "Toggle Filmstrip").changed();
                    ui.end_row();
                });
        });
    });
    changed
}

/// Settings → Behavior: wheel action and startup/browsing options.
pub(crate) fn settings_behavior(app: &mut PhotonioApp, ui: &mut egui::Ui, colors: &theme::ThemeColors) -> bool {
    let mut changed = false;

    // Card 1: Navigation & Input.
    settings_card(ui, colors, Some("Navigation & Input"), |ui| {
        egui::Grid::new("behavior_input_grid")
            .num_columns(2)
            .spacing(egui::vec2(16.0, 8.0))
            .show(ui, |ui| {
                ui.label("Mouse Wheel:");
                ui.horizontal(|ui| {
                    if colors
                        .segmented_value(
                            ui,
                            &mut app.config.mouse_wheel_action,
                            config::MouseWheelAction::Navigate,
                            "Next / Previous Image",
                        )
                        .clicked()
                    {
                        changed = true;
                    }
                    if colors
                        .segmented_value(
                            ui,
                            &mut app.config.mouse_wheel_action,
                            config::MouseWheelAction::Zoom,
                            "Zoom In / Out",
                        )
                        .clicked()
                    {
                        changed = true;
                    }
                });
                ui.end_row();
            });

        ui.add_space(4.0);
        changed |= ui
            .checkbox(
                &mut app.config.loop_navigation,
                "Loop navigation at folder boundary",
            )
            .changed();
    });
    ui.add_space(6.0);

    // Card: Folder Image Sorting Order.
    settings_card(ui, colors, Some("Folder Image Sorting"), |ui| {
        egui::Grid::new("folder_sort_grid")
            .num_columns(2)
            .spacing(egui::vec2(16.0, 8.0))
            .show(ui, |ui| {
                ui.label("Sort images by:");
                ui.horizontal(|ui| {
                    if colors
                        .segmented_value(
                            ui,
                            &mut app.config.sort_criteria,
                            crate::config::FileSortCriteria::Name,
                            "Name",
                        )
                        .clicked()
                    {
                        changed = true;
                        let crit = app.config.sort_criteria;
                        let dir = app.config.sort_direction;
                        app.set_sort_order(crit, dir, ui.ctx());
                    }
                    if colors
                        .segmented_value(
                            ui,
                            &mut app.config.sort_criteria,
                            crate::config::FileSortCriteria::DateModified,
                            "Date Modified",
                        )
                        .clicked()
                    {
                        changed = true;
                        let crit = app.config.sort_criteria;
                        let dir = app.config.sort_direction;
                        app.set_sort_order(crit, dir, ui.ctx());
                    }
                    if colors
                        .segmented_value(
                            ui,
                            &mut app.config.sort_criteria,
                            crate::config::FileSortCriteria::Size,
                            "File Size",
                        )
                        .clicked()
                    {
                        changed = true;
                        let crit = app.config.sort_criteria;
                        let dir = app.config.sort_direction;
                        app.set_sort_order(crit, dir, ui.ctx());
                    }
                });
                ui.end_row();

                ui.label("Sort direction:");
                ui.horizontal(|ui| {
                    if colors
                        .segmented_value(
                            ui,
                            &mut app.config.sort_direction,
                            crate::config::SortDirection::Ascending,
                            "Ascending",
                        )
                        .clicked()
                    {
                        changed = true;
                        let crit = app.config.sort_criteria;
                        let dir = app.config.sort_direction;
                        app.set_sort_order(crit, dir, ui.ctx());
                    }
                    if colors
                        .segmented_value(
                            ui,
                            &mut app.config.sort_direction,
                            crate::config::SortDirection::Descending,
                            "Descending",
                        )
                        .clicked()
                    {
                        changed = true;
                        let crit = app.config.sort_criteria;
                        let dir = app.config.sort_direction;
                        app.set_sort_order(crit, dir, ui.ctx());
                    }
                });
                ui.end_row();
            });
    });
    ui.add_space(6.0);

    // Card 2: Startup & Window.
    settings_card(ui, colors, Some("Startup & Window"), |ui| {
        let mut remember = app.config.remember_last_folder;
        if ui
            .checkbox(
                &mut remember,
                "Remember last viewed image and folder on startup",
            )
            .changed()
        {
            app.config.remember_last_folder = remember;
            if !remember {
                app.config.last_directory = None;
                app.config.last_file_path = None;
            } else if let Some(path) = app.current_file().cloned() {
                if let Some(p) = path.parent() {
                    app.config.last_directory = Some(p.to_path_buf());
                }
                app.config.last_file_path = Some(path);
            } else if let Some(dir) = app.current_folder() {
                app.config.last_directory = Some(dir);
            }
            changed = true;
        }
        changed |= ui
            .checkbox(&mut app.config.start_fullscreen, "Start in Fullscreen")
            .changed();
    });
    ui.add_space(6.0);

    // Card 3: Multi-View & Navigation.
    settings_card(ui, colors, Some("Multi-View & Grid Navigation"), |ui| {
        let mut show_folders = app.config.multiview_show_folders;
        if ui
            .checkbox(
                &mut show_folders,
                "View folders on multi-view",
            )
            .on_hover_text("Show subfolders with 4-image quad previews inside the multi-view grid")
            .changed()
        {
            app.config.multiview_show_folders = show_folders;
            changed = true;
            app.refresh_multiview_subfolders(ui.ctx());
        }

        changed |= ui
            .checkbox(
                &mut app.config.adjustable_grid_tile_size,
                "Allow Ctrl + Wheel to adjust preview grid tile size",
            )
            .changed();
    });
    changed
}

/// Settings → Integration: registry health (async), Explorer context menu,
/// and a full unregister. Returns `(register, folder_menu, clean, apps)`.
pub(crate) fn settings_integration(
    app: &mut PhotonioApp,
    ui: &mut egui::Ui,
    colors: &theme::ThemeColors,
) -> (bool, bool, bool, bool) {
    let mut register_clicked = false;
    let mut folder_menu_clicked = false;
    let mut clean_clicked = false;
    let mut open_default_apps_clicked = false;
    let checking = app.checking_registry || app.registry_status.is_none();
    let current_exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "<unknown>".to_string());

    // Card 1: Default App Handler.
    settings_card(ui, colors, Some("Default App Handler"), |ui| {
        if checking {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new());
                ui.label("Checking registry…");
            });
        } else {
            match app.registry_status.as_ref() {
                Some(platform::RegistryStatus::Valid { .. }) => {
                    ui.label(
                        egui::RichText::new(format!("{} Registered as default app", icons::ICON_CHECK))
                            .color(SUCCESS_COLOR),
                    );
                }
                Some(platform::RegistryStatus::Mismatch { registered_path, .. }) => {
                    ui.label(
                        egui::RichText::new(format!("{} Registered to a different path", icons::ICON_WARN))
                            .color(WARN_COLOR)
                            .strong(),
                    );
                    ui.label(format!("Registered: {}", registered_path.display()));
                }
                Some(platform::RegistryStatus::NotRegistered) | None => {
                    ui.label(
                        egui::RichText::new(format!("{} Not registered as default app", icons::ICON_CIRCLE))
                            .color(colors.muted),
                    );
                }
            }
        }
        ui.label(
            egui::RichText::new(format!("Executable: {current_exe}"))
                .size(11.0)
                .color(colors.muted),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let reg_btn = colors.button_primary(
                ui,
                format!("{} Register as Default App", icons::ICON_CHECK),
                egui::vec2(0.0, 26.0),
                !checking,
            );
            if reg_btn.clicked() {
                register_clicked = true;
            }

            let apps_btn = colors.button_secondary(
                ui,
                format!("{} Windows Default Apps", icons::ICON_GEAR),
                egui::vec2(0.0, 26.0),
                true,
            );
            if apps_btn.clicked() {
                open_default_apps_clicked = true;
            }
        });
    });
    ui.add_space(5.0);

    // Card 2: Explorer Context Menu.
    settings_card(ui, colors, Some("Explorer Context Menu"), |ui| {
        if checking {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new());
                ui.label("Checking…");
            });
        } else if app.folder_menu_registered {
            ui.label(
                egui::RichText::new(format!("{} Active in Explorer folder context menu", icons::ICON_CHECK))
                    .color(SUCCESS_COLOR),
            );
            ui.add_space(4.0);
            let remove_btn = colors.button_secondary(
                ui,
                format!("{} Remove from Context Menu", icons::ICON_XMARK),
                egui::vec2(0.0, 26.0),
                !checking,
            );
            if remove_btn.clicked() {
                folder_menu_clicked = true;
            }
        } else {
            ui.label(
                egui::RichText::new(format!("{} Not in folder context menu", icons::ICON_CIRCLE))
                    .color(colors.muted),
            );
            ui.add_space(4.0);
            let add_btn = colors.button_primary(
                ui,
                format!("{} Add 'Open folder with Photonio'", icons::ICON_PLUS),
                egui::vec2(0.0, 26.0),
                !checking,
            );
            if add_btn.clicked() {
                folder_menu_clicked = true;
            }
        }
    });
    ui.add_space(5.0);

    // Card 3: Clean Purge (Danger zone).
    settings_card(ui, colors, Some("Clean Purge"), |ui| {
        ui.label(
            egui::RichText::new("Remove every registry entry and file association Photonio has written.")
                .size(11.5)
                .color(colors.muted),
        );
        ui.add_space(4.0);
        let danger_btn = colors.button_accent(
            ui,
            format!("{} Unregister & Clean Registry", icons::ICON_TRASH),
            egui::vec2(0.0, 26.0),
            true,
            ERROR_COLOR,
        );
        if danger_btn.clicked() {
            clean_clicked = true;
        }
    });

    (register_clicked, folder_menu_clicked, clean_clicked, open_default_apps_clicked)
}
