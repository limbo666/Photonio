// File: src/app/grid.rs
// Purpose: Full-window Multi-View virtualized thumbnail grid, vector folder cards, and quad previews.

use std::path::PathBuf;
use eframe::egui;
use crate::app::breadcrumbs::{layout_breadcrumbs, BreadcrumbSegment};
use crate::app::types::{
    GridFolderAction, GridTileAction, ERROR_COLOR,
};
use crate::app::widgets::{cover_uv, fit_rect_contain};
use crate::app::PhotonioApp;
use crate::filesystem::truncate;
use crate::{icons, loader, viewer};

pub(crate) fn draw_grid_view(app: &mut PhotonioApp, ctx: &egui::Context) {
    let len = app.files.len();
    let mut back_clicked = false;
    let mut open_index: Option<usize> = None;
    let mut menu_index: Option<usize> = None;
    let mut menu_pos: Option<egui::Pos2> = None;
    let mut navigate_dir: Option<PathBuf> = None;
    let mut navigate_back = false;
    let mut navigate_forward = false;
    let mut open_folder_browser = false;

    let colors = app.theme();
    let current_dir = app.current_folder();

    // Keep path buffer in sync with active folder when not in raw text edit mode.
    if !app.grid_path_editing {
        app.grid_path_buffer = current_dir
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
    }

    // Global shortcut Ctrl+L: focus the path bar for direct path entry in grid view.
    if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::L)) {
        app.grid_path_editing = true;
        app.grid_path_invalid = false;
        app.grid_path_focus_pending = true;
        if let Some(dir) = &current_dir {
            app.grid_path_buffer = dir.display().to_string();
        }
    }

    // Global shortcuts for folder navigation (suppressed while editing raw path or modal open)
    if !app.grid_path_editing && !app.modal_open() {
        if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft)) {
            navigate_back = true;
        }
        if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight)) {
            navigate_forward = true;
        }
        let backspace = ctx.input(|i| !i.modifiers.any() && i.key_pressed(egui::Key::Backspace));
        let alt_up = ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowUp));
        if alt_up || backspace {
            if let Some(parent) = current_dir.as_ref().and_then(|p| p.parent()) {
                navigate_dir = Some(parent.to_path_buf());
            }
        }
    }

    egui::CentralPanel::default()
        .frame(egui::Frame::none())
        .show(ctx, |ui| {
            // Top header bar with active path bar & folder browser button.
            egui::Frame::none()
                .inner_margin(egui::Margin::symmetric(10.0, 5.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Right-side controls (Back button, Up one level button, item counts)
                        let folder_count = if app.config.multiview_show_folders {
                            app.multiview_subfolders.len()
                        } else {
                            0
                        };

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let back_btn = colors.button_secondary(
                                ui,
                                format!("{} Back to Viewer (Enter)", icons::ICON_IMAGE),
                                egui::vec2(0.0, 26.0),
                                true,
                            );
                            if back_btn.clicked() {
                                back_clicked = true;
                            }

                            ui.add_space(6.0);

                            let can_up = current_dir.as_ref().and_then(|p| p.parent()).is_some();
                            let up_btn = colors.button_secondary(
                                ui,
                                format!("{} Up", icons::ICON_UP),
                                egui::vec2(0.0, 26.0),
                                can_up,
                            );
                            if up_btn.on_hover_text("Go to parent folder (Backspace / Alt+Up)").clicked() && can_up {
                                if let Some(parent) = current_dir.as_ref().and_then(|p| p.parent()) {
                                    navigate_dir = Some(parent.to_path_buf());
                                }
                            }

                            ui.add_space(8.0);
                            let count_text = if folder_count > 0 && len > 0 {
                                format!("{folder_count} folder{}, {len} image{}", if folder_count == 1 { "" } else { "s" }, if len == 1 { "" } else { "s" })
                            } else if folder_count > 0 {
                                format!("{folder_count} folder{}", if folder_count == 1 { "" } else { "s" })
                            } else {
                                format!("{len} image{}", if len == 1 { "" } else { "s" })
                            };
                            ui.label(
                                egui::RichText::new(count_text)
                                    .color(colors.muted)
                                    .size(12.5),
                            );
                            ui.add_space(8.0);

                            // Sort criteria & direction menu
                            let sort_icon = match app.config.sort_direction {
                                crate::config::SortDirection::Ascending => icons::ICON_SORT_UP,
                                crate::config::SortDirection::Descending => icons::ICON_SORT_DOWN,
                            };
                            let sort_label = format!(
                                "{} Sort: {}",
                                sort_icon,
                                app.config.sort_criteria.label(),
                            );
                            ui.menu_button(
                                egui::RichText::new(&sort_label)
                                    .size(12.5)
                                    .color(colors.primary),
                                |ui| {
                                    let active_crit = app.config.sort_criteria;
                                    let active_dir = app.config.sort_direction;

                                    let name_chk = if active_crit == crate::config::FileSortCriteria::Name { icons::ICON_CHECK } else { "   " };
                                    if ui.button(format!("{name_chk} Name (Alphabetical)")).clicked() {
                                        app.set_sort_order(crate::config::FileSortCriteria::Name, active_dir, ctx);
                                        ui.close_menu();
                                    }

                                    let date_chk = if active_crit == crate::config::FileSortCriteria::DateModified { icons::ICON_CHECK } else { "   " };
                                    if ui.button(format!("{date_chk} Date Modified")).clicked() {
                                        app.set_sort_order(crate::config::FileSortCriteria::DateModified, active_dir, ctx);
                                        ui.close_menu();
                                    }

                                    let size_chk = if active_crit == crate::config::FileSortCriteria::Size { icons::ICON_CHECK } else { "   " };
                                    if ui.button(format!("{size_chk} File Size")).clicked() {
                                        app.set_sort_order(crate::config::FileSortCriteria::Size, active_dir, ctx);
                                        ui.close_menu();
                                    }

                                    ui.separator();

                                    let asc_chk = if active_dir == crate::config::SortDirection::Ascending { icons::ICON_CHECK } else { "   " };
                                    if ui.button(format!("{asc_chk} Ascending")).clicked() {
                                        app.set_sort_order(active_crit, crate::config::SortDirection::Ascending, ctx);
                                        ui.close_menu();
                                    }

                                    let desc_chk = if active_dir == crate::config::SortDirection::Descending { icons::ICON_CHECK } else { "   " };
                                    if ui.button(format!("{desc_chk} Descending")).clicked() {
                                        app.set_sort_order(active_crit, crate::config::SortDirection::Descending, ctx);
                                        ui.close_menu();
                                    }
                                },
                            );
                            ui.add_space(8.0);

                            // Left-side controls occupy the remaining space
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(2.0, 0.0);

                                // 1. Back button (chevron-left)
                                let can_back = !app.folder_back_stack.is_empty();
                                let back_tooltip = if let Some(p) = app.folder_back_stack.last() {
                                    let name = p
                                        .file_name()
                                        .map(|n| n.to_string_lossy().into_owned())
                                        .unwrap_or_else(|| p.display().to_string());
                                    format!("Back to \"{}\" (Alt+Left)", name)
                                } else {
                                    "Back (Alt+Left)".to_string()
                                };
                                if colors
                                    .nav_icon_button(
                                        ui,
                                        icons::ICON_PREV,
                                        13.0,
                                        egui::vec2(26.0, 26.0),
                                        can_back,
                                        &back_tooltip,
                                    )
                                    .clicked()
                                {
                                    navigate_back = true;
                                }

                                // 2. Forward button (chevron-right)
                                let can_forward = !app.folder_forward_stack.is_empty();
                                let forward_tooltip = if let Some(p) = app.folder_forward_stack.last() {
                                    let name = p
                                        .file_name()
                                        .map(|n| n.to_string_lossy().into_owned())
                                        .unwrap_or_else(|| p.display().to_string());
                                    format!("Forward to \"{}\" (Alt+Right)", name)
                                } else {
                                    "Forward (Alt+Right)".to_string()
                                };
                                if colors
                                    .nav_icon_button(
                                        ui,
                                        icons::ICON_NEXT,
                                        13.0,
                                        egui::vec2(26.0, 26.0),
                                        can_forward,
                                        &forward_tooltip,
                                    )
                                    .clicked()
                                {
                                    navigate_forward = true;
                                }

                                // 3. Up to parent directory button (arrow-up)
                                let can_up = current_dir.as_ref().and_then(|p| p.parent()).is_some();
                                if colors
                                    .nav_icon_button(
                                        ui,
                                        icons::ICON_UP,
                                        13.0,
                                        egui::vec2(26.0, 26.0),
                                        can_up,
                                        "Go to parent folder (Backspace / Alt+Up)",
                                    )
                                    .clicked()
                                {
                                    if let Some(parent) = current_dir.as_ref().and_then(|p| p.parent()) {
                                        navigate_dir = Some(parent.to_path_buf());
                                    }
                                }

                                ui.add_space(2.0);

                                // Inset active path bar (input-like frame with embedded folder button and breadcrumbs)
                                let frame_stroke = if app.grid_path_invalid {
                                    egui::Stroke::new(1.0_f32, ERROR_COLOR)
                                } else {
                                    egui::Stroke::new(1.0_f32, colors.border)
                                };
                                let input_fill = if colors.dark_mode {
                                    egui::Color32::from_black_alpha(60)
                                } else {
                                    egui::Color32::from_black_alpha(10)
                                };

                                egui::Frame::none()
                                    .fill(input_fill)
                                    .stroke(frame_stroke)
                                    .rounding(4.0)
                                    .inner_margin(egui::Margin {
                                        left: 3.0,
                                        right: 6.0,
                                        top: 2.0,
                                        bottom: 2.0,
                                    })
                                    .show(ui, |ui| {
                                        ui.set_min_height(22.0);
                                        ui.set_min_width(ui.available_width().max(60.0));
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing = egui::vec2(3.0, 0.0);

                                            // Folder symbol active button inside the left edge of the address bar
                                            if colors
                                                .nav_icon_button(
                                                    ui,
                                                    icons::ICON_OPEN_FOLDER,
                                                    13.0,
                                                    egui::vec2(22.0, 22.0),
                                                    true,
                                                    "Choose folder... (Ctrl+Shift+O)",
                                                )
                                                .clicked()
                                            {
                                                open_folder_browser = true;
                                            }

                                            if app.grid_path_editing {
                                                // Raw path edit mode
                                                let mut output = egui::TextEdit::singleline(
                                                    &mut app.grid_path_buffer,
                                                )
                                                .desired_width(f32::INFINITY)
                                                .frame(false)
                                                .hint_text("Enter folder path...")
                                                .text_color(if app.grid_path_invalid {
                                                    ERROR_COLOR
                                                } else {
                                                    colors.primary
                                                })
                                                .show(ui);

                                                if output.response.changed() {
                                                    app.grid_path_invalid = false;
                                                }

                                                if app.grid_path_focus_pending {
                                                    output.response.request_focus();
                                                    let len = app.grid_path_buffer.len();
                                                    let range = egui::text::CCursorRange::two(
                                                        egui::text::CCursor::new(0),
                                                        egui::text::CCursor::new(len),
                                                    );
                                                    output.state.cursor.set_char_range(Some(range));
                                                    output.state.store(ui.ctx(), output.response.id);
                                                    app.grid_path_focus_pending = false;
                                                }

                                                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                                                let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                                                if enter {
                                                    let path = viewer::folder_browser::normalize_path(
                                                        &app.grid_path_buffer,
                                                    );
                                                    if path.is_dir() {
                                                        navigate_dir = Some(path);
                                                        app.grid_path_editing = false;
                                                        app.grid_path_invalid = false;
                                                    } else {
                                                        app.grid_path_invalid = true;
                                                        app.show_status(format!(
                                                            "⚠️ Folder does not exist: {}",
                                                            path.display()
                                                        ));
                                                    }
                                                } else if esc || (output.response.lost_focus() && !app.grid_path_invalid) {
                                                    app.grid_path_editing = false;
                                                    app.grid_path_invalid = false;
                                                    if let Some(dir) = &current_dir {
                                                        app.grid_path_buffer = dir.display().to_string();
                                                    } else {
                                                        app.grid_path_buffer.clear();
                                                    }
                                                }
                                            } else {
                                                // Interactive breadcrumb mode with responsive "..." collapse
                                                let mut enter_edit = false;
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing = egui::vec2(2.0, 0.0);
                                                    if let Some(dir) = &current_dir {
                                                        let crumbs = viewer::folder_browser::breadcrumbs(dir);
                                                        let avail_crumb_w = (ui.available_width() - 24.0).max(40.0);
                                                        let segments = layout_breadcrumbs(&crumbs, avail_crumb_w, |text, is_bold| {
                                                            ui.fonts(|f| {
                                                                let font_id = if is_bold {
                                                                    egui::FontId::new(13.0, egui::FontFamily::Proportional)
                                                                } else {
                                                                    egui::FontId::proportional(13.0)
                                                                };
                                                                f.layout_no_wrap(text.to_owned(), font_id, egui::Color32::WHITE).size().x
                                                            })
                                                        });

                                                        for (i, segment) in segments.iter().enumerate() {
                                                            if i > 0 {
                                                                ui.label(
                                                                    egui::RichText::new("›")
                                                                        .size(12.0)
                                                                        .color(colors.muted),
                                                                );
                                                            }
                                                            match segment {
                                                                BreadcrumbSegment::Crumb { label, path, is_current } => {
                                                                    let text = if *is_current {
                                                                        egui::RichText::new(label)
                                                                            .size(13.0)
                                                                            .strong()
                                                                            .color(colors.primary)
                                                                    } else {
                                                                        egui::RichText::new(label)
                                                                            .size(13.0)
                                                                            .color(colors.muted)
                                                                    };
                                                                    let btn_resp = ui.scope(|ui| {
                                                                        let visuals = ui.visuals_mut();
                                                                        visuals.widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
                                                                        visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
                                                                        visuals.widgets.hovered.bg_fill = if colors.dark_mode {
                                                                            egui::Color32::from_white_alpha(22)
                                                                        } else {
                                                                            egui::Color32::from_black_alpha(18)
                                                                        };
                                                                        visuals.widgets.hovered.bg_stroke = egui::Stroke::NONE;
                                                                        visuals.widgets.active.bg_fill = if colors.dark_mode {
                                                                            egui::Color32::from_white_alpha(35)
                                                                        } else {
                                                                            egui::Color32::from_black_alpha(28)
                                                                        };
                                                                        visuals.widgets.active.bg_stroke = egui::Stroke::NONE;
                                                                        ui.spacing_mut().button_padding = egui::vec2(4.0, 1.5);
                                                                        ui.add(egui::Button::new(text).rounding(3.0))
                                                                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                                    });
                                                                    if btn_resp.inner.clicked() {
                                                                        navigate_dir = Some(path.clone());
                                                                    }
                                                                }
                                                                BreadcrumbSegment::Ellipsis { hidden } => {
                                                                    let popup_id = ui.make_persistent_id("breadcrumb_ellipsis_popup");
                                                                    let tooltip = format!(
                                                                        "Hidden folders:\n{}",
                                                                        hidden.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>().join(" › ")
                                                                    );
                                                                    let btn_resp = ui.scope(|ui| {
                                                                        let visuals = ui.visuals_mut();
                                                                        visuals.widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
                                                                        visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
                                                                        visuals.widgets.hovered.bg_fill = if colors.dark_mode {
                                                                            egui::Color32::from_white_alpha(22)
                                                                        } else {
                                                                            egui::Color32::from_black_alpha(18)
                                                                        };
                                                                        visuals.widgets.hovered.bg_stroke = egui::Stroke::NONE;
                                                                        visuals.widgets.active.bg_fill = if colors.dark_mode {
                                                                            egui::Color32::from_white_alpha(35)
                                                                        } else {
                                                                            egui::Color32::from_black_alpha(28)
                                                                        };
                                                                        visuals.widgets.active.bg_stroke = egui::Stroke::NONE;
                                                                        ui.spacing_mut().button_padding = egui::vec2(4.0, 1.5);
                                                                        ui.add(
                                                                            egui::Button::new(
                                                                                egui::RichText::new("...")
                                                                                    .size(13.0)
                                                                                    .color(colors.muted),
                                                                            )
                                                                            .rounding(3.0),
                                                                        )
                                                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                                        .on_hover_text(tooltip)
                                                                    });

                                                                    if btn_resp.inner.clicked() {
                                                                        ui.memory_mut(|mem| mem.toggle_popup(popup_id));
                                                                    }

                                                                    egui::popup_below_widget(
                                                                        ui,
                                                                        popup_id,
                                                                        &btn_resp.inner,
                                                                        egui::PopupCloseBehavior::CloseOnClick,
                                                                        |ui| {
                                                                            ui.set_min_width(140.0);
                                                                            for (hidden_label, hidden_path) in hidden {
                                                                                if ui.button(format!("{} {}", icons::ICON_FOLDER, hidden_label)).clicked() {
                                                                                    navigate_dir = Some(hidden_path.clone());
                                                                                }
                                                                            }
                                                                        },
                                                                    );
                                                                }
                                                            }
                                                        }
                                                    } else {
                                                        ui.label(
                                                            egui::RichText::new("No folder selected")
                                                                .size(13.0)
                                                                .color(colors.muted)
                                                                .italics(),
                                                        );
                                                    }

                                                    // Empty area to the right: clicking switches into raw text edit mode
                                                    let avail = ui.available_size();
                                                    if avail.x > 0.0 {
                                                        let (_empty_rect, empty_resp) = ui.allocate_exact_size(avail, egui::Sense::click());
                                                        if empty_resp.clicked() {
                                                            enter_edit = true;
                                                        }
                                                    }
                                                });

                                                if enter_edit {
                                                    app.grid_path_editing = true;
                                                    if let Some(dir) = &current_dir {
                                                        app.grid_path_buffer = dir.display().to_string();
                                                    } else {
                                                        app.grid_path_buffer.clear();
                                                    }
                                                    app.grid_path_invalid = false;
                                                    app.grid_path_focus_pending = true;
                                                }
                                            }
                                        });
                                    });
                            });
                        });
                    });
                });
            ui.separator();

            let folder_count = if app.config.multiview_show_folders {
                app.multiview_subfolders.len()
            } else {
                0
            };
            let total_items = folder_count + len;

            if total_items == 0 {
                ui.vertical_centered(|ui| {
                    ui.add_space(60.0);
                    ui.label(
                        egui::RichText::new(icons::ICON_IMAGE)
                            .size(48.0)
                            .color(colors.muted),
                    );
                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new("No supported images or folders in this location")
                            .size(16.0)
                            .strong()
                            .color(colors.primary),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("Click a folder in the path bar above, type a path, or click the folder button to browse.")
                            .size(12.0)
                            .color(colors.muted),
                    );
                });
            }

            // Virtualized grid: only visible rows are laid out and only
            // their thumbnails are requested (bounds memory for huge folders).
            let tile_w = app.grid_tile_width();
            let tile_h = app.grid_tile_height();
            let avail_w = ui.available_width();
            app.grid_available_width = avail_w;
            app.grid_spacing_y = ui.spacing().item_spacing.y;
            let columns = ((avail_w - 12.0) / tile_w).floor().max(1.0) as usize;
            app.grid_columns = columns;
            let row_height = tile_h;
            let total_rows = if total_items == 0 { 0 } else { (total_items + columns - 1) / columns };

            let needed_lod = loader::PreviewLod::from_tile_width(tile_w, ui.ctx().pixels_per_point());

            // Boundary-aware auto-scroll: bring the selected card into view
            // only when it falls outside the current viewport.
            egui::ScrollArea::vertical()
                .id_source("grid-scroll")
                .auto_shrink([false, false])
                .show_rows(ui, row_height, total_rows, |ui, row_range| {
                    if app.scroll_to_selected {
                        app.scroll_to_selected = false;
                        if total_rows > 0 {
                            let spacing = ui.spacing().item_spacing;
                            let min_row = row_range.start;
                            let unified_idx = match app.grid_selected_folder {
                                Some(f) if f < folder_count => f,
                                _ => {
                                    if len > 0 {
                                        folder_count + app.current_index.min(len - 1)
                                    } else {
                                        0
                                    }
                                }
                            };
                            let row = unified_idx / columns;
                            let col = unified_idx % columns;
                            let origin = ui.max_rect().min;
                            let rect = egui::Rect::from_min_size(
                                egui::pos2(
                                    origin.x + col as f32 * (tile_w + spacing.x),
                                    origin.y
                                        + (row as isize - min_row as isize) as f32
                                            * (row_height + spacing.y),
                                ),
                                egui::vec2(tile_w, row_height),
                            );
                            ui.scroll_to_rect(rect, None);
                        }
                    }

                    for row in row_range {
                        ui.horizontal(|ui| {
                            for col in 0..columns {
                                let item_idx = row * columns + col;
                                if item_idx >= total_items {
                                    break;
                                }
                                if item_idx < folder_count {
                                    // Folder item
                                    if let Some(action) = grid_folder_tile(app, ui, item_idx) {
                                        match action {
                                            GridFolderAction::Select(idx) => {
                                                app.grid_selected_folder = Some(idx);
                                            }
                                            GridFolderAction::Open(idx) => {
                                                if let Some(folder) = app.multiview_subfolders.get(idx) {
                                                    navigate_dir = Some(folder.path.clone());
                                                }
                                            }
                                            GridFolderAction::ContextMenu(_idx, _pos) => {}
                                            GridFolderAction::Drag(idx) => {
                                                if let Some(folder) = app.multiview_subfolders.get(idx) {
                                                    crate::platform::start_drag_files(&[folder.path.clone()]);
                                                    ctx.request_repaint();
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    // Image item
                                    let img_idx = item_idx - folder_count;
                                    app.request_thumb_for(img_idx, needed_lod);
                                    if let Some(action) = grid_tile(app, ui, img_idx) {
                                        match action {
                                            GridTileAction::Select(i) => {
                                                app.grid_selected_folder = None;
                                                app.grid_select(i);
                                            }
                                            GridTileAction::Open(i) => {
                                                app.grid_selected_folder = None;
                                                open_index = Some(i);
                                            }
                                            GridTileAction::ContextMenu(i, pos) => {
                                                app.grid_selected_folder = None;
                                                menu_index = Some(i);
                                                menu_pos = Some(pos);
                                            }
                                            GridTileAction::Drag(i) => {
                                                if let Some(path) = app.files.get(i).cloned() {
                                                    crate::platform::start_drag_files(&[path]);
                                                    ctx.request_repaint();
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        });
                    }
                });
        });

    // Post-actions (double-click open, right-click menu, back button, folder navigation).
    if let Some(index) = open_index {
        app.current_index = index;
        app.leave_grid(ctx);
        return;
    }
    if let Some(index) = menu_index {
        app.current_index = index;
        app.context_menu_pos = menu_pos;
    }
    if back_clicked {
        if !app.files.is_empty() {
            app.leave_grid(ctx);
        } else {
            app.show_status("No image to view in this folder");
        }
        return;
    }
    if navigate_back {
        app.navigate_folder_back(ctx);
        return;
    }
    if navigate_forward {
        app.navigate_folder_forward(ctx);
        return;
    }
    if open_folder_browser {
        app.grid_path_editing = false;
        app.grid_path_invalid = false;
        app.open_folder_dialog();
        return;
    }
    if let Some(path) = navigate_dir {
        if path.is_dir() {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            app.open_directory(path.clone(), ctx);
            app.scroll_to_selected = true;
            app.grid_selected_folder = None;
            app.grid_path_buffer = path.display().to_string();
            app.grid_path_editing = false;
            app.grid_path_invalid = false;
            let count = app.files.len();
            let f_count = app.multiview_subfolders.len();
            if count > 0 && f_count > 0 {
                app.show_success(format!("{name} ({f_count} folders, {count} images)"));
            } else if count > 0 {
                app.show_success(format!("{name} ({count} images)"));
            } else if f_count > 0 {
                app.show_success(format!("{name} ({f_count} folders)"));
            } else {
                app.show_success(format!("{name} (folder is empty)"));
            }
            app.show_success(format!(
                "{} Opened folder \"{}\" ({} image{})",
                icons::ICON_IMAGE,
                name,
                count,
                if count == 1 { "" } else { "s" }
            ));
        } else {
            app.show_status(format!("⚠️ Folder does not exist: {}", path.display()));
        }
    }
}

/// Draw one grid thumbnail card; returns the interaction, if any.
pub(crate) fn grid_tile(app: &mut PhotonioApp, ui: &mut egui::Ui, index: usize) -> Option<GridTileAction> {
    let colors = app.theme();
    let tw = app.grid_tile_width();
    let th = app.grid_tile_height();

    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(tw, th), egui::Sense::click_and_drag());
    let painter = ui.painter();
    let card = rect.shrink(3.0);

    // Rounded card background (theme-aware glass tint).
    painter.rect_filled(card, 6.0, colors.placeholder);

    // Selection / hover accent frame.
    if index == app.current_index && app.grid_selected_folder.is_none() {
        painter.rect_stroke(card, 6.0, egui::Stroke::new(2.0_f32, colors.gold));
    } else if response.hovered() {
        painter.rect_stroke(card, 6.0, egui::Stroke::new(1.0_f32, colors.muted));
    }

    if response.hovered() {
        app.grid_hovered_index = Some(index);
    }

    // Preview image container occupies nearly the entire card.
    // Margins and bottom label stay at clean, standard unscaled sizes.
    let pad = 6.0;
    let top_pad = 6.0;
    let label_row_h = 20.0;
    let thumb_w = (tw - pad * 2.0).max(10.0);
    let thumb_h = (th - top_pad - pad - label_row_h).max(10.0);
    let thumb_box =
        egui::Rect::from_min_size(rect.min + egui::vec2(pad, top_pad), egui::vec2(thumb_w, thumb_h));

    match app.thumb_cache.get(&index) {
        Some(thumb) => {
            let img_rect = fit_rect_contain(thumb.dims, thumb_box);
            painter.image(
                thumb.texture.id(),
                img_rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        None if app.failed_thumb.contains(&index) => {
            painter.text(
                thumb_box.center(),
                egui::Align2::CENTER_CENTER,
                "✕",
                egui::FontId::proportional(16.0),
                colors.muted,
            );
        }
        None => {
            // Still decoding — background card only.
        }
    }

    // Filename label stays at the standard readable view size (11.5px) regardless of preview zoom.
    let label_font = 11.5;
    let max_chars = ((tw - pad * 2.0) / (label_font * 0.55)) as usize;
    let name = app
        .files
        .get(index)
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    painter.text(
        rect.min + egui::vec2(pad, th - label_row_h),
        egui::Align2::LEFT_TOP,
        truncate(&name, max_chars.max(4)),
        egui::FontId::proportional(label_font),
        colors.primary,
    );

    let has_incoming = ui.input(|i| !i.raw.hovered_files.is_empty() || !i.raw.dropped_files.is_empty());
    if !has_incoming && response.drag_started() {
        Some(GridTileAction::Drag(index))
    } else if response.double_clicked() {
        Some(GridTileAction::Open(index))
    } else if response.secondary_clicked() {
        let pos = response.interact_pointer_pos.unwrap_or_else(|| rect.center());
        Some(GridTileAction::ContextMenu(index, pos))
    } else if response.clicked() {
        Some(GridTileAction::Select(index))
    } else {
        None
    }
}

/// Draw one grid folder card with either large folder icon or quad (2x2) image previews.
pub(crate) fn grid_folder_tile(app: &mut PhotonioApp, ui: &mut egui::Ui, folder_index: usize) -> Option<GridFolderAction> {
    let colors = app.theme();
    let tw = app.grid_tile_width();
    let th = app.grid_tile_height();

    let folder = match app.multiview_subfolders.get(folder_index) {
        Some(f) => f.clone(),
        None => return None,
    };

    let (rect, response) = ui.allocate_exact_size(egui::vec2(tw, th), egui::Sense::click_and_drag());
    let painter = ui.painter();
    let card = rect.shrink(3.0);

    // Rounded card background
    painter.rect_filled(card, 6.0, colors.placeholder);

    // Selection / hover accent frame
    let is_selected = app.grid_selected_folder == Some(folder_index);
    if is_selected {
        painter.rect_stroke(card, 6.0, egui::Stroke::new(2.0_f32, colors.gold));
    } else if response.hovered() {
        painter.rect_stroke(card, 6.0, egui::Stroke::new(1.0_f32, colors.muted));
    }

    let pad = 5.0;
    let top_pad = 5.0;
    let label_row_h = 20.0;
    let thumb_w = (tw - pad * 2.0).max(10.0);
    let thumb_h = (th - top_pad - pad - label_row_h).max(10.0);
    let thumb_box = egui::Rect::from_min_size(
        rect.min + egui::vec2(pad, top_pad),
        egui::vec2(thumb_w, thumb_h),
    );

    // Leave a small border around the folder symbol within thumb_box
    let border = 4.0_f32;
    let avail_w = (thumb_w - border * 2.0).max(10.0);
    let avail_h = (thumb_h - border * 2.0).max(10.0);

    // FontAwesome folder glyph (\u{f07b}) natural visual aspect ratio is 1.0 : 0.82.
    // Scale font_size so the glyph stretches to fill thumb_box leaving a small border.
    let font_size = avail_w.min(avail_h / 0.82);
    let folder_w = font_size;
    let folder_h = font_size * 0.82;

    let folder_rect = egui::Rect::from_center_size(
        thumb_box.center(),
        egui::vec2(folder_w, folder_h),
    );

    let folder_color = if colors.dark_mode {
        egui::Color32::from_rgb(228, 160, 30)
    } else {
        egui::Color32::from_rgb(215, 150, 25)
    };

    let galley = ui.fonts(|f| {
        f.layout_no_wrap(
            icons::ICON_FOLDER.to_string(),
            egui::FontId::proportional(font_size),
            folder_color,
        )
    });
    let galley_origin = folder_rect.min - egui::vec2(0.0, font_size * 0.02);
    painter.galley(galley_origin, galley, folder_color);

    if !folder.preview_paths.is_empty() {
        // Folders WITH images: the 4 first images stay ABOVE the folder symbol,
        // inset so the golden folder tab and borders frame the images.
        let quad_left = folder_rect.min.x + folder_rect.width() * 0.125;
        let quad_right = folder_rect.max.x - folder_rect.width() * 0.120;
        let quad_top = folder_rect.min.y + folder_rect.height() * 0.200;
        let quad_bottom = folder_rect.max.y - folder_rect.height() * 0.090;
        let quad_rect = egui::Rect::from_min_max(
            egui::pos2(quad_left, quad_top),
            egui::pos2(quad_right, quad_bottom),
        );

        let gap = 2.0_f32;
        let cell_w = ((quad_rect.width() - gap) * 0.5).max(1.0);
        let cell_h = ((quad_rect.height() - gap) * 0.5).max(1.0);

        for idx in 0..4 {
            let r = idx / 2;
            let c = idx % 2;
            let cell_rect = egui::Rect::from_min_size(
                quad_rect.min + egui::vec2(c as f32 * (cell_w + gap), r as f32 * (cell_h + gap)),
                egui::vec2(cell_w, cell_h),
            );

            if let Some(img_path) = folder.preview_paths.get(idx) {
                if let Some(tex) = app.multiview_folder_textures.get(img_path) {
                    let uv = cover_uv(tex.size(), cell_rect);
                    painter.add(egui::epaint::RectShape {
                        rect: cell_rect,
                        rounding: egui::Rounding::same(2.0),
                        fill: egui::Color32::WHITE,
                        stroke: egui::Stroke::NONE,
                        blur_width: 0.0,
                        fill_texture_id: tex.id(),
                        uv,
                    });
                } else {
                    // Image still decoding: subtle dark glass slot
                    let slot_fill = if colors.dark_mode {
                        egui::Color32::from_black_alpha(80)
                    } else {
                        egui::Color32::from_black_alpha(30)
                    };
                    painter.rect_filled(cell_rect, 2.0, slot_fill);
                }
            }
        }
    }

    // Folder name label (with small folder prefix)
    let label_font = 11.5;
    let max_chars = ((tw - pad * 2.0) / (label_font * 0.55)) as usize;
    let label_text = format!("{} {}", icons::ICON_FOLDER, folder.name);
    painter.text(
        rect.min + egui::vec2(pad, th - label_row_h),
        egui::Align2::LEFT_TOP,
        truncate(&label_text, max_chars.max(4)),
        egui::FontId::proportional(label_font),
        colors.primary,
    );

    let tooltip = format!(
        "Folder: {}\n{}",
        folder.name,
        if folder.preview_paths.is_empty() {
            "No images inside".to_string()
        } else {
            format!("{} image preview{}", folder.preview_paths.len(), if folder.preview_paths.len() == 1 { "" } else { "s" })
        }
    );
    let response = response.on_hover_text(tooltip).on_hover_cursor(egui::CursorIcon::PointingHand);

    let has_incoming = ui.input(|i| !i.raw.hovered_files.is_empty() || !i.raw.dropped_files.is_empty());
    if !has_incoming && response.drag_started() {
        Some(GridFolderAction::Drag(folder_index))
    } else if response.double_clicked() {
        Some(GridFolderAction::Open(folder_index))
    } else if response.secondary_clicked() {
        let pos = response.interact_pointer_pos.unwrap_or_else(|| rect.center());
        Some(GridFolderAction::ContextMenu(folder_index, pos))
    } else if response.clicked() {
        Some(GridFolderAction::Select(folder_index))
    } else {
        None
    }
}
