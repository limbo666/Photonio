// File: src/viewer/folder_browser.rs
// Purpose: Photonio — custom in-app folder browser modal. Lists folders (with
//          image cover previews) and files on a background worker thread, with
//          a bounded thumbnail cache and a generation counter so results from a
//          previous navigation are discarded.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

use eframe::egui;

use crate::{config, icons, platform, theme, viewer};

/// Max files listed in one folder (older items are truncated).
const MAX_FILES: usize = 2000;
/// Max cover/thumbnail decodes per listing (bounds worker work).
const MAX_THUMBS: usize = 300;
/// Max entries inspected per subfolder while hunting for a cover image.
const COVER_SCAN_LIMIT: usize = 50;
/// Thumbnail cache capacity (textures).
const THUMB_CACHE_CAP: usize = 300;

#[derive(Clone, Copy, PartialEq, Eq)]
enum FileKind {
    Image,
    Video,
    Other,
}

pub struct FolderEntry {
    name: String,
    path: PathBuf,
}

pub struct FileEntry {
    name: String,
    path: PathBuf,
    kind: FileKind,
    size: u64,
}

struct Listing {
    folders: Vec<FolderEntry>,
    files: Vec<FileEntry>,
    truncated: bool,
}

/// A detected drive root plus its (lazily resolved) volume label.
struct DriveEntry {
    root: String,
    label: Option<String>,
}

/// Cached quick-access sidebar data (standard folders + drives), built once per
/// browser instance so drive/volume detection never runs on the UI frame loop.
struct QuickAccess {
    folders: Vec<(String, PathBuf)>,
    drives: Vec<DriveEntry>,
}

impl QuickAccess {
    fn build() -> Self {
        let mut folders = Vec::new();
        if let Some(d) = dirs::desktop_dir() {
            folders.push(("Desktop".to_string(), d));
        }
        if let Some(d) = dirs::picture_dir() {
            folders.push(("Pictures".to_string(), d));
        }
        if let Some(d) = dirs::document_dir() {
            folders.push(("Documents".to_string(), d));
        }
        let drives = platform::list_logical_drives()
            .into_iter()
            .map(|root| DriveEntry { root, label: None })
            .collect();
        Self { folders, drives }
    }
}

enum WorkerMsg {
    Thumb { gen: u64, key: PathBuf, rgba: Vec<u8>, dims: [usize; 2] },
    Done { gen: u64 },
}

struct Cover {
    texture: egui::TextureHandle,
    dims: [usize; 2],
}

/// Bounded, insertion-ordered thumbnail cache.
struct ThumbCache {
    map: std::collections::HashMap<PathBuf, Cover>,
    order: std::collections::VecDeque<PathBuf>,
    cap: usize,
}

impl ThumbCache {
    fn new(cap: usize) -> Self {
        Self { map: Default::default(), order: Default::default(), cap }
    }

    fn get(&self, path: &Path) -> Option<&Cover> {
        self.map.get(path)
    }

    fn insert(&mut self, path: PathBuf, cover: Cover) {
        if self.map.contains_key(&path) {
            return;
        }
        self.map.insert(path.clone(), cover);
        self.order.push_back(path.clone());
        while self.order.len() > self.cap {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
    }

    fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }
}

pub enum BrowserResult {
    None,
    Select(PathBuf),
    Cancel,
    Pin(PathBuf),
    Unpin(PathBuf),
}

/// State for the custom folder browser modal.
pub struct Browser {
    pub current: PathBuf,
    pub selected: Option<PathBuf>,
    listing: Listing,
    covers_loading: bool,
    generation: u64,
    rx: Option<Receiver<WorkerMsg>>,
    thumbs: ThumbCache,
    temp_path_buffer: String,
    is_editing_path: bool,
    address_focus_pending: bool,
    path_invalid: bool,
    pub tile_size: config::FolderTileSize,
    pub filter: config::ContentFilter,
    pub show_files: bool,
    quick: QuickAccess,
    labels_rx: Option<Receiver<HashMap<String, String>>>,
    pub pinned: Vec<PathBuf>,
}

impl Browser {
    pub fn new(start: PathBuf, cfg: &config::Config) -> Self {
        let quick = QuickAccess::build();

        // Resolve volume labels on a background thread (non-blocking): some
        // removable/network drives can stall `GetVolumeInformationW`. Results
        // are cached on `self.quick` once received in `poll()`.
        let drive_roots: Vec<String> = quick.drives.iter().map(|d| d.root.clone()).collect();
        let (tx, labels_rx) = channel();
        std::thread::spawn(move || {
            let mut labels = HashMap::new();
            for root in drive_roots {
                if let Some(label) = platform::drive_volume_label(&root) {
                    labels.insert(root, label);
                }
            }
            let _ = tx.send(labels);
        });

        let mut b = Self {
            current: start.clone(),
            selected: None,
            listing: Listing { folders: Vec::new(), files: Vec::new(), truncated: false },
            covers_loading: false,
            generation: 0,
            rx: None,
            thumbs: ThumbCache::new(THUMB_CACHE_CAP),
            temp_path_buffer: start.display().to_string(),
            is_editing_path: false,
            address_focus_pending: false,
            path_invalid: false,
            tile_size: cfg.folder_tile_size,
            filter: cfg.folder_content_filter,
            show_files: cfg.folder_show_files,
            quick,
            labels_rx: Some(labels_rx),
            pinned: cfg.pinned_folders.clone(),
        };
        b.navigate(start);
        b
    }

    /// Navigate to `path`: enumerate direct children synchronously so the cards
    /// appear on frame one, then lazily discover covers + decode thumbnails on a
    /// background thread (never blocking the UI).
    pub fn navigate(&mut self, path: PathBuf) {
        self.current = path.clone();
        self.selected = None;
        self.temp_path_buffer = path.display().to_string();
        self.is_editing_path = false;
        self.path_invalid = false;
        self.generation = self.generation.wrapping_add(1);
        let gen = self.generation;

        // Instant direct-children listing — no recursion, no image decode.
        self.listing = scan_direct_children(&path, self.filter, self.show_files);

        // Background: lazily resolve covers (one level) and decode thumbnails.
        let folders: Vec<PathBuf> =
            self.listing.folders.iter().map(|f| f.path.clone()).collect();
        let image_files: Vec<PathBuf> = self
            .listing
            .files
            .iter()
            .filter(|f| f.kind == FileKind::Image)
            .map(|f| f.path.clone())
            .collect();
        let thumb_side = thumb_side(self.tile_size);
        let (tx, rx) = channel();
        self.rx = Some(rx);
        self.covers_loading = true;
        std::thread::spawn(move || {
            let mut sent = 0usize;
            // Folder covers first (the "album jacket" thumbnails).
            for folder in folders {
                if sent >= MAX_THUMBS {
                    break;
                }
                if let Some(cover_path) = find_cover(&folder) {
                    if let Some((rgba, dims)) = make_thumb(&cover_path, thumb_side) {
                        if tx.send(WorkerMsg::Thumb { gen, key: folder, rgba, dims }).is_err() {
                            return;
                        }
                        sent += 1;
                    }
                }
            }
            // Loose image files.
            for file in image_files {
                if sent >= MAX_THUMBS {
                    break;
                }
                if let Some((rgba, dims)) = make_thumb(&file, thumb_side) {
                    if tx.send(WorkerMsg::Thumb { gen, key: file, rgba, dims }).is_err() {
                        return;
                    }
                    sent += 1;
                }
            }
            let _ = tx.send(WorkerMsg::Done { gen });
        });
    }

    /// Drain worker messages; upload textures for the current generation.
    pub fn poll(&mut self, ctx: &egui::Context) {
        // Cache volume labels once the background resolver finishes.
        if let Some(labels_rx) = self.labels_rx.take() {
            match labels_rx.try_recv() {
                Ok(labels) => {
                    for drive in &mut self.quick.drives {
                        if let Some(label) = labels.get(&drive.root) {
                            drive.label = Some(label.clone());
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.labels_rx = Some(labels_rx);
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {}
            }
        }

        let Some(rx) = &self.rx else { return };
        while let Ok(msg) = rx.try_recv() {
            match msg {
                WorkerMsg::Thumb { gen, key, rgba, dims } => {
                    if gen == self.generation {
                        let color = egui::ColorImage::from_rgba_unmultiplied(dims, &rgba);
                        let texture = ctx.load_texture(
                            format!("fb-thumb-{}", key.display()),
                            color,
                            egui::TextureOptions::LINEAR,
                        );
                        self.thumbs.insert(key, Cover { texture, dims });
                    }
                }
                WorkerMsg::Done { gen } => {
                    if gen == self.generation {
                        self.covers_loading = false;
                    }
                }
            }
        }
        if self.covers_loading {
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
        }
    }

    /// Render the modal; returns the user's confirm/cancel action, if any.
    /// `open` is bound directly to the app's visibility flag so the window's
    /// close (X) button syncs application state immediately.
    pub fn render(
        &mut self,
        ctx: &egui::Context,
        colors: &theme::ThemeColors,
        open: &mut bool,
    ) -> BrowserResult {
        let mut result = BrowserResult::None;

        let start_tile = self.tile_size;
        let start_filter = self.filter;
        let start_show_files = self.show_files;

        // Global keys (gated by keyboard focus and address-bar edit mode).
        let typing = ctx.wants_keyboard_input();
        let (up_key, enter_key) = ctx.input(|i| {
            let nav = !typing && !self.is_editing_path;
            let up = nav
                && (i.key_pressed(egui::Key::Backspace)
                    || (i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft)));
            let enter = nav && i.key_pressed(egui::Key::Enter);
            (up, enter)
        });

        // Ctrl+L focuses the address bar for raw-path editing.
        if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::L)) {
            if !self.is_editing_path {
                self.temp_path_buffer = self.current.display().to_string();
            }
            self.is_editing_path = true;
            self.path_invalid = false;
            self.address_focus_pending = true;
        }

        let mut navigate_to: Option<PathBuf> = None;
        let mut select_path: Option<PathBuf> = None;
        let mut text_consumed_enter = false;

        // Bound max size to the main app window (leaving a small margin), but
        // keep native resizing enabled so the user can drag the edges freely.
        let main_size = ctx
            .input(|i| i.viewport().inner_rect)
            .map(|r| r.size())
            .unwrap_or_else(|| ctx.screen_rect().size());
        let max = (main_size - egui::vec2(40.0, 60.0)).max(egui::vec2(200.0, 150.0));

        egui::Window::new(
            egui::RichText::new(format!("{} Browse Folders", icons::ICON_FOLDER)).size(13.0),
        )
            .open(open)
            .collapsible(false)
            .resizable(true)
            .default_size(egui::vec2(760.0, 480.0))
            .min_size(egui::vec2(400.0, 300.0))
            .max_size(max)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                // --- Unified address bar (Row 1) ---
                ui.horizontal(|ui| {
                    // Up button (disabled at the root / drive list).
                    let can_up = self.current.parent().is_some();
                    let up = ui.add_enabled(can_up, egui::Button::new(icons::ICON_UP));
                    if up.clicked() {
                        if let Some(parent) = self.current.parent().map(|p| p.to_path_buf()) {
                            navigate_to = Some(parent);
                        }
                    }

                    // Inset address-bar frame (input-like).
                    let frame_stroke = if self.path_invalid {
                        egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(235, 87, 87))
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
                        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            if self.is_editing_path {
                                // Raw path edit mode.
                                let mut output = egui::TextEdit::singleline(
                                    &mut self.temp_path_buffer,
                                )
                                .desired_width(f32::INFINITY)
                                .text_color(if self.path_invalid {
                                    egui::Color32::from_rgb(235, 87, 87)
                                } else {
                                    colors.primary
                                })
                                .show(ui);

                                if self.address_focus_pending {
                                    output.response.request_focus();
                                    let len = self.temp_path_buffer.len();
                                    let range = egui::text::CCursorRange::two(
                                        egui::text::CCursor::new(0),
                                        egui::text::CCursor::new(len),
                                    );
                                    output.state.cursor.set_char_range(Some(range));
                                    output.state.store(ui.ctx(), output.response.id);
                                    self.address_focus_pending = false;
                                }

                                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                                let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                                if enter {
                                    text_consumed_enter = true;
                                    let path = normalize_path(&self.temp_path_buffer);
                                    if path.is_dir() {
                                        navigate_to = Some(path);
                                        self.is_editing_path = false;
                                        self.path_invalid = false;
                                    } else {
                                        self.path_invalid = true;
                                        output.response.request_focus();
                                    }
                                } else if esc || output.response.lost_focus() {
                                    self.is_editing_path = false;
                                    self.path_invalid = false;
                                    self.temp_path_buffer = self.current.display().to_string();
                                }
                            } else {
                                // Breadcrumb mode.
                                let mut enter_edit = false;
                                ui.horizontal_wrapped(|ui| {
                                    let crumbs = breadcrumbs(&self.current);
                                    for (i, (label, path)) in crumbs.iter().enumerate() {
                                        if i > 0 {
                                            ui.label(
                                                egui::RichText::new("›")
                                                    .size(13.0)
                                                    .color(colors.muted),
                                            );
                                        }
                                        let text = egui::RichText::new(label).size(13.0);
                                        if ui.add(egui::Button::new(text).frame(false)).clicked() {
                                            navigate_to = Some(path.clone());
                                        }
                                    }
                                    // Empty area to the right → edit mode.
                                    let empty = ui.allocate_rect(
                                        ui.available_rect_before_wrap(),
                                        egui::Sense::click(),
                                    );
                                    if empty.clicked() {
                                        enter_edit = true;
                                    }
                                });
                                if enter_edit {
                                    self.is_editing_path = true;
                                    self.temp_path_buffer = self.current.display().to_string();
                                    self.path_invalid = false;
                                    self.address_focus_pending = true;
                                }
                            }
                        });
                });
                ui.separator();

                // View + filter toolbar: a full-width, subtle tinted strip.
                let strip_tint = if colors.dark_mode {
                    egui::Color32::from_white_alpha(8)
                } else {
                    egui::Color32::from_black_alpha(8)
                };
                egui::Frame::none()
                    .fill(strip_tint)
                    .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                    .show(ui, |ui| {
                        // Force the tint to span the entire window width.
                        ui.set_min_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            ui.label("View:");
                            ui.selectable_value(
                                &mut self.tile_size,
                                config::FolderTileSize::Small,
                                "Small",
                            );
                            ui.selectable_value(
                                &mut self.tile_size,
                                config::FolderTileSize::Medium,
                                "Medium",
                            );
                            ui.selectable_value(
                                &mut self.tile_size,
                                config::FolderTileSize::Large,
                                "Large",
                            );
                            ui.selectable_value(
                                &mut self.tile_size,
                                config::FolderTileSize::List,
                                "List",
                            );
                            ui.separator();
                            ui.label("Show:");
                            ui.selectable_value(
                                &mut self.filter,
                                config::ContentFilter::Images,
                                "Images",
                            );
                            ui.selectable_value(
                                &mut self.filter,
                                config::ContentFilter::Media,
                                "Media",
                            );
                            ui.selectable_value(&mut self.filter, config::ContentFilter::All, "All");
                            ui.separator();
                            ui.checkbox(&mut self.show_files, "Show files");
                        });
                    });
                ui.separator();

                // Reserve room below the body for the trailing separator and the
                // footer button row (plus their item spacings). This must be >= the
                // real footer height: `egui::Resize` grows a resizable window to fit
                // its content every frame and never shrinks, so under-reserving makes
                // the dialog creep taller each frame until `max_size` clamps it.
                // Derive the value from the live style (instead of a hardcoded 40px)
                // so it stays correct at any DPI / font / interact size.
                let sp = ui.spacing();
                let footer_h = sp.interact_size.y * 2.0 + sp.item_spacing.y * 3.0;
                let body_h = (ui.available_height() - footer_h).max(120.0);

                ui.horizontal_top(|ui| {
                    // --- Sidebar ---
                    ui.vertical(|ui| {
                        ui.set_width(160.0);

                        // Section header.
                        ui.label(
                            egui::RichText::new("QUICK ACCESS")
                                .size(10.0)
                                .color(colors.muted),
                        );
                        ui.add_space(4.0);

                        // Standard folders.
                        for (label, path) in &self.quick.folders {
                            let text = format!("{}  {}", icons::ICON_FOLDER, label);
                            if ui
                                .add(
                                    egui::Button::new(text)
                                        .frame(false)
                                        .min_size(egui::vec2(ui.available_width(), 20.0)),
                                )
                                .clicked()
                            {
                                navigate_to = Some(path.clone());
                            }
                        }

                        ui.separator();

                        // Drives (with cached volume labels, once resolved).
                        for drive in &self.quick.drives {
                            let text = match &drive.label {
                                Some(label) => {
                                    format!("{}  {} ({})", icons::ICON_HDD, drive.root, label)
                                }
                                None => format!("{}  {}", icons::ICON_HDD, drive.root),
                            };
                            if ui
                                .add(
                                    egui::Button::new(text)
                                        .frame(false)
                                        .min_size(egui::vec2(ui.available_width(), 20.0)),
                                )
                                .clicked()
                            {
                                navigate_to = Some(PathBuf::from(&drive.root));
                            }
                        }

                        ui.separator();

                        // Pinned Quick Access folders (below the drives list).
                        if !self.pinned.is_empty() {
                            ui.label(
                                egui::RichText::new("PINNED").size(10.0).color(colors.muted),
                            );
                            ui.add_space(4.0);
                            for path in &self.pinned {
                                let name = path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| path.display().to_string());
                                let text =
                                    format!("{} {}", icons::ICON_FOLDER, truncate_label(&name, 22));
                                // Flat button (not a selectable label) so the cursor
                                // stays a pointing hand instead of a text I-beam.
                                let resp = ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new(text).color(pin_accent()),
                                        )
                                        .frame(false)
                                        .min_size(egui::vec2(ui.available_width(), 20.0)),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                let resp = resp.on_hover_text(path.display().to_string());
                                if resp.clicked() {
                                    navigate_to = Some(path.clone());
                                }
                                resp.context_menu(|ui| {
                                    if ui
                                        .button(format!(
                                            "{} Remove from Quick Access",
                                            icons::ICON_TRASH
                                        ))
                                        .clicked()
                                    {
                                        result = BrowserResult::Unpin(path.clone());
                                        ui.close_menu();
                                    }
                                });
                            }
                        }
                    });

                    // Vertical divider between the sidebar and the content.
                    // `ui.separator()` in a horizontal layout spans the FULL
                    // available height, which makes this row (and therefore the
                    // resizable window) grow without bound — draw a line bounded
                    // to the body height instead.
                    let (div_rect, _) = ui.allocate_exact_size(
                        egui::vec2(6.0, body_h),
                        egui::Sense::hover(),
                    );
                    ui.painter().vline(
                        div_rect.center().x,
                        div_rect.y_range(),
                        ui.visuals().widgets.noninteractive.bg_stroke,
                    );

                    // --- Content (bounded scroll + natural wrap) ---
                    // The enclosing `horizontal_top` gives the ScrollArea a
                    // horizontal layout, which makes labels *extend* (no wrap)
                    // and overflow the window width. Force a vertical layout so
                    // rows stack and text wraps/truncates to the available width.
                    egui::ScrollArea::vertical()
                        .id_source("folder-browser-content")
                        .auto_shrink([false, false])
                        .max_height(body_h)
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                if self.listing.truncated {
                                    ui.label(
                                        egui::RichText::new("(more files not shown)")
                                            .small()
                                            .italics()
                                            .color(colors.muted),
                                    );
                                }

                                if self.tile_size == config::FolderTileSize::List {
                                    self.render_list(ui, colors, &mut navigate_to, &mut select_path);
                                } else {
                                    self.render_grid(ui, colors, &mut navigate_to, &mut select_path);
                                }
                            });
                        });
                });

                ui.separator();

                // --- Footer ---
                let (selected_label, full_path) = match &self.selected {
                    Some(sel) => (
                        sel.file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| sel.display().to_string()),
                        sel.clone(),
                    ),
                    None => {
                        let name = self
                            .current
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| self.current.display().to_string());
                        (format!("{name} (Current Folder)"), self.current.clone())
                    }
                };
                let selected_text = format!("{} Selected: {}", icons::ICON_FOLDER, selected_label);
                let pin_target = self.selected.clone().unwrap_or_else(|| self.current.clone());
                let is_pinned = self.pinned.contains(&pin_target);
                let accent = pin_accent();
                // Left-to-right: readout anchored far left, buttons far right.
                ui.horizontal(|ui| {
                    // Far left: bounded readout so long names truncate before
                    // crowding the right-aligned buttons.
                    let label_max = (ui.available_width() - 360.0).max(60.0);
                    ui.scope(|ui| {
                        ui.set_max_width(label_max);
                        let resp = ui.add(egui::Label::new(selected_text).truncate());
                        resp.on_hover_text(full_path.display().to_string());
                    });
                    // Far right: [Pin] —— [Select Folder] —— [Cancel].
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Cancel").clicked() {
                            result = BrowserResult::Cancel;
                        }
                        ui.add_space(8.0);
                        if ui.button("Select Folder").clicked() {
                            result = BrowserResult::Select(
                                self.selected.clone().unwrap_or_else(|| self.current.clone()),
                            );
                        }
                        ui.add_space(16.0);
                        if is_pinned {
                            ui.add_enabled(
                                false,
                                egui::Button::new(format!("{} Pinned", icons::ICON_CHECK)),
                            );
                        } else if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new(format!(
                                        "{} Pin to Quick Access",
                                        icons::ICON_PIN
                                    ))
                                    .color(accent),
                                ),
                            )
                            .clicked()
                        {
                            result = BrowserResult::Pin(pin_target.clone());
                        }
                    });
                });
            });

        // The window's close (X) button flips `open` directly; bail out early.
        if !*open {
            return BrowserResult::Cancel;
        }

        // Keyboard navigation (only when the modal is still open and no button
        // already fired).
        if matches!(result, BrowserResult::None) {
            if up_key {
                if let Some(parent) = self.current.parent().map(|p| p.to_path_buf()) {
                    navigate_to = Some(parent);
                }
            } else if enter_key && !text_consumed_enter {
                if let Some(sel) = self.selected.clone() {
                    navigate_to = Some(sel);
                }
            }
        }

        // Apply navigation / selection once.
        if let Some(path) = navigate_to {
            self.navigate(path);
        } else if let Some(path) = select_path {
            self.selected = Some(path);
        }

        // Persist view settings by re-scanning when they change.
        let size_changed = self.tile_size != start_tile;
        let settings_changed = size_changed
            || self.filter != start_filter
            || self.show_files != start_show_files;
        if size_changed {
            self.thumbs.clear();
        }
        if settings_changed {
            self.navigate(self.current.clone());
        }

        result
    }

    fn render_grid(
        &mut self,
        ui: &mut egui::Ui,
        colors: &theme::ThemeColors,
        navigate_to: &mut Option<PathBuf>,
        select_path: &mut Option<PathBuf>,
    ) {
        let geom = tile_geom(self.tile_size);
        let folder_count = self.listing.folders.len();
        let file_count = self.listing.files.len();
        if folder_count + file_count == 0 {
            return;
        }

        // Natural wrapping; the enclosing ScrollArea provides vertical scroll.
        ui.horizontal_wrapped(|ui| {
            for index in 0..folder_count {
                self.render_folder_tile(ui, colors, &geom, index, navigate_to, select_path);
            }
            for index in 0..file_count {
                self.render_file_tile(ui, colors, &geom, index);
            }
        });
    }

    fn render_folder_tile(
        &mut self,
        ui: &mut egui::Ui,
        colors: &theme::ThemeColors,
        geom: &TileGeom,
        index: usize,
        navigate_to: &mut Option<PathBuf>,
        select_path: &mut Option<PathBuf>,
    ) {
        let Some(entry) = self.listing.folders.get(index) else { return };
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(geom.card_w, geom.card_h), egui::Sense::click());
        let response = response.on_hover_text(entry.name.clone());
        let painter = ui.painter();
        let card = rect;
        let selected = self.selected.as_deref() == Some(entry.path.as_path());

        // Unified dark card frame.
        painter.rect_filled(card, 6.0, colors.placeholder);
        let border = if selected {
            egui::Stroke::new(2.0_f32, colors.primary)
        } else if response.hovered() {
            egui::Stroke::new(1.5_f32, colors.muted)
        } else {
            egui::Stroke::new(1.0_f32, colors.border)
        };
        painter.rect_stroke(card, 6.0, border);

        let thumb_box = egui::Rect::from_min_size(
            egui::pos2(card.min.x + CARD_PAD, card.min.y + CARD_PAD),
            egui::vec2(geom.thumb_w, geom.thumb_h),
        );
        match self.thumbs.get(&entry.path) {
            Some(cover) => {
                // Album jacket: letterboxed cover inside a subtle sleeve border.
                paint_thumb(&painter, cover, thumb_box, false, 3.0);
                painter.rect_stroke(thumb_box, 3.0, egui::Stroke::new(1.0_f32, colors.border));
            }
            None => {
                // Empty / non-image folder: centered muted folder glyph.
                painter.text(
                    thumb_box.center(),
                    egui::Align2::CENTER_CENTER,
                    icons::ICON_FOLDER,
                    egui::FontId::proportional(geom.thumb_h * 0.6),
                    colors.muted,
                );
            }
        }

        let label_rect = egui::Rect::from_min_size(
            egui::pos2(card.min.x + CARD_PAD, card.min.y + CARD_PAD + geom.thumb_h),
            egui::vec2(geom.thumb_w, LABEL_H),
        );
        // Folder label: muted folder glyph in front of the primary name.
        paint_card_label(
            ui,
            &painter,
            label_rect,
            &entry.name,
            colors.primary,
            Some(icons::ICON_FOLDER),
            colors.muted,
        );

        if response.double_clicked() {
            *navigate_to = Some(entry.path.clone());
        } else if response.clicked() {
            *select_path = Some(entry.path.clone());
        }
    }

    fn render_file_tile(
        &mut self,
        ui: &mut egui::Ui,
        colors: &theme::ThemeColors,
        geom: &TileGeom,
        index: usize,
    ) {
        let Some(entry) = self.listing.files.get(index) else { return };
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(geom.card_w, geom.card_h), egui::Sense::hover());
        let response = response.on_hover_text(entry.name.clone());
        let painter = ui.painter();
        let card = rect;

        // Unified dark card frame.
        painter.rect_filled(card, 6.0, colors.placeholder);
        let border = if response.hovered() {
            egui::Stroke::new(1.5_f32, colors.muted)
        } else {
            egui::Stroke::new(1.0_f32, colors.border)
        };
        painter.rect_stroke(card, 6.0, border);

        let thumb_box = egui::Rect::from_min_size(
            egui::pos2(card.min.x + CARD_PAD, card.min.y + CARD_PAD),
            egui::vec2(geom.thumb_w, geom.thumb_h),
        );
        match entry.kind {
            FileKind::Image => {
                if let Some(cover) = self.thumbs.get(&entry.path) {
                    // Standalone photo: fills the inner area edge-to-edge.
                    paint_thumb(&painter, cover, thumb_box, true, 3.0);
                }
            }
            FileKind::Video => {
                painter.text(
                    thumb_box.center(),
                    egui::Align2::CENTER_CENTER,
                    icons::ICON_FILMSTRIP,
                    egui::FontId::proportional(geom.thumb_h * 0.5),
                    colors.muted,
                );
            }
            FileKind::Other => {}
        }

        let label_rect = egui::Rect::from_min_size(
            egui::pos2(card.min.x + CARD_PAD, card.min.y + CARD_PAD + geom.thumb_h),
            egui::vec2(geom.thumb_w, LABEL_H),
        );
        // File label: no folder glyph.
        paint_card_label(ui, &painter, label_rect, &entry.name, colors.muted, None, colors.muted);
    }

    fn render_list(
        &mut self,
        ui: &mut egui::Ui,
        colors: &theme::ThemeColors,
        navigate_to: &mut Option<PathBuf>,
        select_path: &mut Option<PathBuf>,
    ) {
        let folder_count = self.listing.folders.len();
        let file_count = self.listing.files.len();
        if folder_count + file_count == 0 {
            return;
        }

        // Comfortable, legible row height (balanced vertical padding).
        ui.spacing_mut().interact_size.y = 26.0;

        for index in 0..folder_count {
            let entry = &self.listing.folders[index];
            let selected = self.selected.as_deref() == Some(entry.path.as_path());
            let text = egui::RichText::new(format!("{}  {}", icons::ICON_FOLDER, entry.name))
                .size(14.0);
            let resp = ui.add(egui::SelectableLabel::new(selected, text));
            if resp.double_clicked() {
                *navigate_to = Some(entry.path.clone());
            } else if resp.clicked() {
                *select_path = Some(entry.path.clone());
            }
        }

        for index in 0..file_count {
            let entry = &self.listing.files[index];
            let icon = match entry.kind {
                FileKind::Image => icons::ICON_OPEN_FILE,
                FileKind::Video => icons::ICON_FILMSTRIP,
                FileKind::Other => "•",
            };
            let kind = match entry.kind {
                FileKind::Image => "Image",
                FileKind::Video => "Video",
                FileKind::Other => "File",
            };
            let size_str = format_size(entry.size);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(icon).size(14.0).color(colors.muted));

                // The trailing "kind" and "size" columns have a fixed width.
                // Reserve it and let the name truncate to the remainder; if the
                // name consumed all available width first, the trailing columns
                // would overflow and expand the resizable window to its maximum
                // width (the same bug class as the grid).
                let small = egui::FontId::proportional(12.0);
                let trailing_w = ui.fonts(|f| {
                    f.layout_no_wrap(kind.to_owned(), small.clone(), egui::Color32::WHITE)
                        .size()
                        .x
                        + f.layout_no_wrap(size_str.clone(), small.clone(), egui::Color32::WHITE)
                            .size()
                            .x
                }) + ui.spacing().item_spacing.x * 2.0;
                let name_w = (ui.available_width() - trailing_w).max(40.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(name_w, ui.spacing().interact_size.y),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.add(egui::Label::new(&entry.name).truncate());
                    },
                );

                ui.label(egui::RichText::new(kind).size(12.0).color(colors.muted));
                ui.label(egui::RichText::new(size_str).size(12.0).color(colors.muted));
            });
        }
    }
}

// --- Helpers ---

/// Uniform inset of the thumbnail (and label side padding) within the card.
const CARD_PAD: f32 = 6.0;
/// Height reserved for the (two-line, ellipsized) filename label.
const LABEL_H: f32 = 30.0;

/// Card dimensions for a view-size preset (uniform across folders and files).
struct TileGeom {
    card_w: f32,
    card_h: f32,
    thumb_w: f32,
    thumb_h: f32,
}

/// Decode max-side for cover thumbnails (matches the larger thumb box edge).
fn thumb_side(size: config::FolderTileSize) -> u32 {
    match size {
        config::FolderTileSize::Small => 88,
        config::FolderTileSize::Medium => 128,
        config::FolderTileSize::Large => 168,
        config::FolderTileSize::List => 20,
    }
}

/// Card + thumbnail-box layout for a view-size preset.
fn tile_geom(size: config::FolderTileSize) -> TileGeom {
    let (card_w, card_h) = match size {
        config::FolderTileSize::Small => (100.0, 110.0),
        config::FolderTileSize::Medium => (140.0, 150.0),
        config::FolderTileSize::Large => (180.0, 190.0),
        config::FolderTileSize::List => (0.0, 0.0),
    };
    TileGeom {
        card_w,
        card_h,
        thumb_w: card_w - CARD_PAD * 2.0,
        thumb_h: card_h - CARD_PAD * 2.0 - LABEL_H,
    }
}

/// Draw a cover texture into `slot` with rounded corners. `cover_fit` fills the
/// slot edge-to-edge (cover crop); otherwise the image is letterboxed (contain).
fn paint_thumb(
    painter: &egui::Painter,
    cover: &Cover,
    slot: egui::Rect,
    cover_fit: bool,
    rounding: f32,
) {
    let (target, uv) = if cover_fit {
        (slot, cover_uv(cover.dims, slot))
    } else {
        (
            fit_rect_contain(cover.dims, slot),
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        )
    };
    painter.add(egui::epaint::RectShape {
        rect: target,
        rounding: egui::Rounding::same(rounding),
        fill: egui::Color32::WHITE,
        stroke: egui::Stroke::NONE,
        blur_width: 0.0,
        fill_texture_id: cover.texture.id(),
        uv,
    });
}

/// UV rect (0..1) that center-crops `dims` to cover `slot`.
fn cover_uv(dims: [usize; 2], slot: egui::Rect) -> egui::Rect {
    let img_w = dims[0].max(1) as f32;
    let img_h = dims[1].max(1) as f32;
    let scale = (slot.width() / img_w).max(slot.height() / img_h);
    let fx = (slot.width() / (img_w * scale)).min(1.0);
    let fy = (slot.height() / (img_h * scale)).min(1.0);
    egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(fx, fy))
}

/// Paint a filename label constrained to `rect`, wrapping to at most two lines
/// with an ellipsis; the text is centered horizontally. `icon` (optional) is a
/// leading FontAwesome glyph rendered in `icon_color` (e.g. a folder glyph).
fn paint_card_label(
    ui: &egui::Ui,
    painter: &egui::Painter,
    rect: egui::Rect,
    text: &str,
    color: egui::Color32,
    icon: Option<&str>,
    icon_color: egui::Color32,
) {
    let font = egui::FontId::proportional(11.0);
    let mut job = egui::text::LayoutJob::default();
    if let Some(icon) = icon {
        job.append(
            &format!("{icon}  "),
            0.0,
            egui::TextFormat {
                font_id: font.clone(),
                color: icon_color,
                ..Default::default()
            },
        );
    }
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: font,
            color,
            ..Default::default()
        },
    );
    job.wrap.max_width = rect.width();
    job.wrap.max_rows = 2;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    let galley = ui.fonts(|f| f.layout_job(job));
    let pos = egui::pos2(rect.center().x - galley.size().x * 0.5, rect.top());
    painter.galley(pos, galley, color);
}

fn file_kind(path: &Path) -> FileKind {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("mp4") | Some("mkv") | Some("avi") | Some("mov") | Some("webm") => FileKind::Video,
        Some("jpg") | Some("jpeg") | Some("png") | Some("webp") | Some("tif")
        | Some("tiff") | Some("bmp") | Some("gif") => FileKind::Image,
        _ => FileKind::Other,
    }
}

fn is_image_path(path: &Path) -> bool {
    file_kind(path) == FileKind::Image
}

/// True if the entry has the Hidden or System attribute (Windows).
fn is_hidden_system(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    let attrs = meta.file_attributes();
    attrs & (HIDDEN | SYSTEM) != 0
}

/// Find the first image inside `dir` (bounded scan).
fn find_cover(dir: &Path) -> Option<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return None };
    for (i, entry) in entries.flatten().enumerate() {
        if i >= COVER_SCAN_LIMIT {
            break;
        }
        let path = entry.path();
        if path.is_file() && is_image_path(&path) {
            return Some(path);
        }
    }
    None
}

/// Decode + downscale an image to `max_side` (worker thread).
fn make_thumb(path: &Path, max_side: u32) -> Option<(Vec<u8>, [usize; 2])> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut prefix = vec![0u8; 128 * 1024];
    let n = std::io::Read::read(&mut file, &mut prefix).ok()?;
    prefix.truncate(n);
    if let Some(thumb) = viewer::embedded_thumbnail(&prefix, max_side) {
        let dims = [thumb.width() as usize, thumb.height() as usize];
        return Some((thumb.to_rgba8().into_raw(), dims));
    }
    let bytes = std::fs::read(path).ok()?;
    let thumb = viewer::decode_thumbnail(&bytes, max_side).ok()?;
    let dims = [thumb.width() as usize, thumb.height() as usize];
    Some((thumb.to_rgba8().into_raw(), dims))
}

/// Enumerate `dir`'s direct children (folders + filtered files). No recursion
/// and no image decoding, so it is fast enough to run on the UI thread and the
/// listing appears on the first frame after navigation.
fn scan_direct_children(
    dir: &Path,
    filter: config::ContentFilter,
    show_files: bool,
) -> Listing {
    let mut folders = Vec::new();
    let mut files = Vec::new();
    let mut truncated = false;

    let Ok(entries) = std::fs::read_dir(dir) else {
        return Listing { folders, files, truncated: false };
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        // Skip hidden/system entries (folders AND files, e.g. desktop.ini).
        if is_hidden_system(&meta) {
            continue;
        }

        if meta.is_dir() {
            folders.push(FolderEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                path,
            });
        } else if show_files {
            let kind = file_kind(&path);
            let show = match filter {
                config::ContentFilter::Images => kind == FileKind::Image,
                config::ContentFilter::Media => {
                    kind == FileKind::Image || kind == FileKind::Video
                }
                config::ContentFilter::All => true,
            };
            if show {
                if files.len() >= MAX_FILES {
                    truncated = true;
                    continue;
                }
                files.push(FileEntry {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    path,
                    kind,
                    size: meta.len(),
                });
            }
        }
    }

    folders.sort_by(|a, b| natord::compare(&a.name.to_lowercase(), &b.name.to_lowercase()));
    files.sort_by(|a, b| natord::compare(&a.name.to_lowercase(), &b.name.to_lowercase()));

    Listing { folders, files, truncated }
}

/// Resolve a starting directory: use `remembered` if it still exists, else walk
/// up to the nearest existing parent, then Pictures, then Documents, then
/// Desktop, then the first available drive.
pub fn resolve_start(remembered: Option<PathBuf>) -> PathBuf {
    if let Some(p) = remembered {
        if p.is_dir() {
            return p;
        }
        let mut cur = p;
        while let Some(parent) = cur.parent() {
            if parent.is_dir() {
                return parent.to_path_buf();
            }
            cur = parent.to_path_buf();
        }
    }
    for dir in [dirs::picture_dir(), dirs::document_dir(), dirs::desktop_dir()].into_iter().flatten() {
        if dir.is_dir() {
            return dir;
        }
    }
    if let Some(d) = platform::list_logical_drives()
        .into_iter()
        .find(|d| PathBuf::from(d).is_dir())
    {
        return PathBuf::from(d);
    }
    PathBuf::from("C:\\")
}

/// Warm amber/gold accent shared by the Pin button and the Pinned sidebar
/// section, so the two read as a linked pair in both Light and Dark themes.
fn pin_accent() -> egui::Color32 {
    egui::Color32::from_rgb(224, 168, 44)
}

/// Truncate a label to at most `max_chars` characters with an ellipsis.
fn truncate_label(s: &str, max_chars: usize) -> String {
    if s.chars().count() > max_chars {
        let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    } else {
        s.to_string()
    }
}

/// Normalize a user-typed path: strip surrounding whitespace/quotes and
/// trailing (redundant) slashes, while preserving a drive-root slash (`C:\`).
fn normalize_path(input: &str) -> PathBuf {
    let trimmed = input
        .trim()
        .trim_matches('"')
        .trim_end_matches(|c: char| c == '\\' || c == '/');
    let mut s = trimmed.to_string();
    if s.len() == 2 && s.ends_with(':') {
        s.push('\\');
    }
    PathBuf::from(s)
}

fn breadcrumbs(path: &Path) -> Vec<(String, PathBuf)> {
    let mut crumbs: Vec<(String, PathBuf)> = path
        .ancestors()
        .map(|p| {
            let label = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.display().to_string());
            (label, p.to_path_buf())
        })
        .collect();
    crumbs.reverse();
    crumbs
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn fit_rect_contain(dims: [usize; 2], slot: egui::Rect) -> egui::Rect {
    let img_w = dims[0].max(1) as f32;
    let img_h = dims[1].max(1) as f32;
    let scale = (slot.width() / img_w).min(slot.height() / img_h);
    let draw_w = img_w * scale;
    let draw_h = img_h * scale;
    egui::Rect::from_center_size(slot.center(), egui::vec2(draw_w, draw_h))
}
