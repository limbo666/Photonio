// File: src/app/context_menu.rs
// Purpose: Canvas right-click context menu and command action execution.

use eframe::egui;
use crate::app::types::{MenuAction, ViewMode};
use crate::app::widgets::{menu_item, menu_item_enabled};
use crate::app::PhotonioApp;
use crate::filesystem::{format_name, format_size, truncate};
use crate::icons;

pub(crate) fn draw_context_menu(app: &mut PhotonioApp, ctx: &egui::Context) -> Option<MenuAction> {
    let raw_pos = app.context_menu_pos?;

    let screen = ctx.screen_rect();
    let est = egui::vec2(240.0, 510.0);
    let pos = egui::pos2(
        raw_pos
            .x
            .clamp(screen.min.x + 8.0, (screen.max.x - est.x - 8.0).max(screen.min.x + 8.0)),
        raw_pos
            .y
            .clamp(screen.min.y + 8.0, (screen.max.y - est.y - 8.0).max(screen.min.y + 8.0)),
    );

    let current = app.current_file().cloned();
    let has_file = current.is_some();
    // Image-edit actions (resize/rotation) only make sense in Viewer mode
    // where a decoded image is available; grid tiles are file-only targets.
    let has_static = app.view_mode == ViewMode::Viewer && app.image.is_some();
    let has_rotation = has_static && app.rotation != 0;
    let dims = app
        .image
        .as_ref()
        .map(|im| im.dimensions)
        .or_else(|| app.animation.as_ref().map(|a| a.dimensions))
        .unwrap_or([0, 0]);

    // "Copy/Move to..." destination labels (target folder name, or default).
    let copy_target_label = app
        .config
        .last_copy_folder
        .as_ref()
        .and_then(|dir| {
            if dir.is_dir() {
                let name = dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| dir.display().to_string());
                Some(format!("Copy to {name}"))
            } else {
                None
            }
        })
        .unwrap_or_else(|| "Copy File to...".to_string());
    let move_target_label = app
        .config
        .last_move_folder
        .as_ref()
        .and_then(|dir| {
            if dir.is_dir() {
                let name = dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| dir.display().to_string());
                Some(format!("Move to {name}"))
            } else {
                None
            }
        })
        .unwrap_or_else(|| "Move File to...".to_string());

    let header: Option<(String, u64, String)> = current.as_ref().map(|path| {
        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let filename = truncate(&filename, 28);
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let format = format_name(path);
        (filename, size, format)
    });

    let mut action = None;
    let mut should_close = false;
    let mut open_info = false;
    let colors = app.theme();

    let response = egui::Area::new(egui::Id::new("context-menu"))
        .fixed_pos(pos)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            colors.panel(ui, |ui| {
                ui.set_min_width(210.0);
                ui.set_max_width(230.0);

                if let Some((filename, size, format)) = &header {
                    // Interactive info card → opens the Image Properties window.
                    let card = egui::Frame::group(ui.style())
                        .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(icons::ICON_INFO).color(colors.muted));
                                ui.label(egui::RichText::new(filename).strong());
                            });
                            ui.label(format!("{} × {} px", dims[0], dims[1]));
                            ui.label(format!("{}  ·  {}", format_size(*size), format));
                        });
                    let card_response = card.response.interact(egui::Sense::click());
                    if card_response.hovered() {
                        ui.painter().rect_filled(
                            card.response.rect,
                            6.0,
                            egui::Color32::from_white_alpha(16),
                        );
                    }
                    let card_response = card_response.on_hover_cursor(egui::CursorIcon::PointingHand);
                    if card_response.clicked() {
                        open_info = true;
                        should_close = true;
                    }
                    ui.separator();
                }

                // Navigation / file
                if menu_item(ui, "Open File...").clicked() {
                    action = Some(MenuAction::OpenFile);
                    should_close = true;
                }
                if menu_item(ui, "Open Folder...").clicked() {
                    action = Some(MenuAction::OpenFolder);
                    should_close = true;
                }
                if menu_item_enabled(ui, "Close Image", has_file).clicked() {
                    action = Some(MenuAction::CloseImage);
                    should_close = true;
                }
                if menu_item_enabled(ui, "Close Folder", has_file).clicked() {
                    action = Some(MenuAction::CloseFolder);
                    should_close = true;
                }

                ui.separator();
                if menu_item_enabled(ui, "Crop Image... (C)", has_static).clicked() {
                    action = Some(MenuAction::CropImage);
                    should_close = true;
                }
                if menu_item_enabled(ui, "Resize Image... (Ctrl+Shift+R)", has_static).clicked() {
                    action = Some(MenuAction::ResizeImage);
                    should_close = true;
                }
                if menu_item_enabled(ui, "Save Rotation (Ctrl+S)", has_rotation).clicked() {
                    action = Some(MenuAction::SaveRotation);
                    should_close = true;
                }
                if menu_item_enabled(ui, "Save As... (Ctrl+Shift+S)", has_file).clicked() {
                    action = Some(MenuAction::SaveAs);
                    should_close = true;
                }

                ui.separator();
                if menu_item_enabled(ui, "Open Containing Folder (Ctrl+E)", has_file).clicked() {
                    action = Some(MenuAction::OpenContainingFolder);
                    should_close = true;
                }
                if menu_item_enabled(ui, "Open With...", has_file).clicked() {
                    action = Some(MenuAction::OpenWith);
                    should_close = true;
                }

                ui.separator();
                if menu_item_enabled(ui, &copy_target_label, has_file).clicked() {
                    action = Some(MenuAction::CopyFileTo);
                    should_close = true;
                }
                if menu_item_enabled(ui, &move_target_label, has_file).clicked() {
                    action = Some(MenuAction::MoveFileTo);
                    should_close = true;
                }

                ui.separator();
                if menu_item_enabled(ui, "Copy File", has_file).clicked() {
                    action = Some(MenuAction::CopyFile);
                    should_close = true;
                }
                if menu_item_enabled(ui, "Copy Image", has_file).clicked() {
                    action = Some(MenuAction::CopyImage);
                    should_close = true;
                }
                if menu_item_enabled(ui, "Copy Path", has_file).clicked() {
                    action = Some(MenuAction::CopyPath);
                    should_close = true;
                }

                ui.separator();
                if menu_item_enabled(ui, "Rename File... (F2)", has_file).clicked() {
                    action = Some(MenuAction::Rename);
                    should_close = true;
                }
                if menu_item_enabled(ui, "Move to Recycle Bin", has_file).clicked() {
                    action = Some(MenuAction::Delete);
                    should_close = true;
                }
                if menu_item_enabled(ui, "Set as Desktop Wallpaper", has_file).clicked() {
                    action = Some(MenuAction::SetWallpaper);
                    should_close = true;
                }

                ui.separator();
                let sort_icon = match app.config.sort_direction {
                    crate::config::SortDirection::Ascending => icons::ICON_SORT_UP,
                    crate::config::SortDirection::Descending => icons::ICON_SORT_DOWN,
                };
                ui.menu_button(
                    format!(
                        "{} Sort: {} ({})",
                        sort_icon,
                        app.config.sort_criteria.label(),
                        app.config.sort_direction.label()
                    ),
                    |ui| {
                        let active_crit = app.config.sort_criteria;
                        let active_dir = app.config.sort_direction;

                        let name_chk = if active_crit == crate::config::FileSortCriteria::Name { icons::ICON_CHECK } else { "   " };
                        if ui.button(format!("{name_chk} Name (Alphabetical)")).clicked() {
                            action = Some(MenuAction::SetSort(crate::config::FileSortCriteria::Name, active_dir));
                            should_close = true;
                        }

                        let date_chk = if active_crit == crate::config::FileSortCriteria::DateModified { icons::ICON_CHECK } else { "   " };
                        if ui.button(format!("{date_chk} Date Modified")).clicked() {
                            action = Some(MenuAction::SetSort(crate::config::FileSortCriteria::DateModified, active_dir));
                            should_close = true;
                        }

                        let size_chk = if active_crit == crate::config::FileSortCriteria::Size { icons::ICON_CHECK } else { "   " };
                        if ui.button(format!("{size_chk} File Size")).clicked() {
                            action = Some(MenuAction::SetSort(crate::config::FileSortCriteria::Size, active_dir));
                            should_close = true;
                        }

                        ui.separator();

                        let asc_chk = if active_dir == crate::config::SortDirection::Ascending { icons::ICON_CHECK } else { "   " };
                        if ui.button(format!("{asc_chk} Ascending")).clicked() {
                            action = Some(MenuAction::SetSort(active_crit, crate::config::SortDirection::Ascending));
                            should_close = true;
                        }

                        let desc_chk = if active_dir == crate::config::SortDirection::Descending { icons::ICON_CHECK } else { "   " };
                        if ui.button(format!("{desc_chk} Descending")).clicked() {
                            action = Some(MenuAction::SetSort(active_crit, crate::config::SortDirection::Descending));
                            should_close = true;
                        }
                    },
                );

                ui.separator();
                if menu_item(ui, "Settings... (Ctrl+,)").clicked() {
                    action = Some(MenuAction::Settings);
                    should_close = true;
                }
                if menu_item(ui, "About Photonio").clicked() {
                    action = Some(MenuAction::About);
                    should_close = true;
                }
                if menu_item(ui, "Help / Shortcuts (F1)").clicked() {
                    action = Some(MenuAction::Help);
                    should_close = true;
                }
                if menu_item(ui, "Exit").clicked() {
                    action = Some(MenuAction::Exit);
                    should_close = true;
                }
            });
        });

    let menu_rect = response.response.rect;
    let clicked_outside = ctx.input(|i| {
        (i.pointer.primary_clicked() || i.pointer.secondary_clicked())
            && i.pointer.interact_pos().map_or(true, |p| !menu_rect.expand(2.0).contains(p))
    });
    if clicked_outside {
        should_close = true;
    }

    if should_close {
        app.context_menu_pos = None;
    }
    if open_info {
        app.show_image_info = true;
    }

    action
}

pub(crate) fn execute_menu_action(app: &mut PhotonioApp, action: MenuAction, ctx: &egui::Context) {
    match action {
        MenuAction::OpenFile => app.open_file_dialog(ctx),
        MenuAction::OpenFolder => app.open_folder_dialog(),
        MenuAction::CloseImage => app.close_current_file(ctx),
        MenuAction::CloseFolder => app.close_folder(),
        MenuAction::CropImage => app.open_crop_mode(),
        MenuAction::ResizeImage => app.open_resize_dialog(),
        MenuAction::SaveRotation => app.save_rotation_overwrite(ctx),
        MenuAction::SaveAs => app.save_current_as(),
        MenuAction::OpenContainingFolder => app.reveal_current(),
        MenuAction::OpenWith => app.open_with_current(),
        MenuAction::CopyFileTo => app.copy_file_to(ctx),
        MenuAction::MoveFileTo => app.move_file_to(ctx),
        MenuAction::CopyFile => app.copy_current_file(),
        MenuAction::CopyImage => app.copy_current_image(),
        MenuAction::CopyPath => app.copy_current_path(ctx),
        MenuAction::Rename => app.open_rename_dialog(),
        MenuAction::SetWallpaper => app.set_wallpaper_current(),
        MenuAction::Delete => app.delete_current(ctx),
        MenuAction::SetSort(crit, dir) => app.set_sort_order(crit, dir, ctx),
        MenuAction::Settings => app.open_settings(),
        MenuAction::About => app.open_about(),
        MenuAction::Help => app.open_help(),
        MenuAction::Exit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
    }
}
