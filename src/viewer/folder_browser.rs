// File: src/viewer/folder_browser.rs
// Purpose: Photonio — custom in-app folder browser modal. Lists folders (with
//          image cover previews) and files on a background worker thread, with
//          a bounded thumbnail cache and a generation counter so results from a
//          previous navigation are discarded.

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

enum WorkerMsg {
    Listing { gen: u64, listing: Listing },
    Thumb { gen: u64, key: PathBuf, rgba: Vec<u8>, dims: [usize; 2] },
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
}

/// State for the custom folder browser modal.
pub struct Browser {
    pub current: PathBuf,
    pub selected: Option<PathBuf>,
    listing: Listing,
    scanning: bool,
    generation: u64,
    rx: Option<Receiver<WorkerMsg>>,
    thumbs: ThumbCache,
    path_edit: String,
    path_invalid: bool,
    pub tile_size: config::FolderTileSize,
    pub filter: config::ContentFilter,
    pub show_files: bool,
}

impl Browser {
    pub fn new(start: PathBuf, cfg: &config::Config) -> Self {
        let mut b = Self {
            current: start.clone(),
            selected: None,
            listing: Listing { folders: Vec::new(), files: Vec::new(), truncated: false },
            scanning: false,
            generation: 0,
            rx: None,
            thumbs: ThumbCache::new(THUMB_CACHE_CAP),
            path_edit: start.display().to_string(),
            path_invalid: false,
            tile_size: cfg.folder_tile_size,
            filter: cfg.folder_content_filter,
            show_files: cfg.folder_show_files,
        };
        b.navigate(start);
        b
    }

    /// Navigate to `path`, kicking off a background scan.
    pub fn navigate(&mut self, path: PathBuf) {
        self.current = path.clone();
        self.selected = None;
        self.path_edit = path.display().to_string();
        self.path_invalid = false;
        self.scanning = true;
        self.generation = self.generation.wrapping_add(1);
        let gen = self.generation;
        let (tx, rx) = channel();
        self.rx = Some(rx);

        let thumb_side = thumb_side(self.tile_size);
        let filter = self.filter;
        let show_files = self.show_files;
        std::thread::spawn(move || {
            let (listing, thumbs) = scan_directory(&path, filter, show_files, thumb_side);
            if tx.send(WorkerMsg::Listing { gen, listing }).is_err() {
                return;
            }
            for (key, rgba, dims) in thumbs {
                if tx.send(WorkerMsg::Thumb { gen, key, rgba, dims }).is_err() {
                    return;
                }
            }
        });
    }

    /// Jump to the path typed into the address field (no-op if invalid).
    fn jump_to_path(&mut self) {
        let path = PathBuf::from(self.path_edit.trim());
        if path.is_dir() {
            self.navigate(path);
        } else {
            self.path_invalid = true;
        }
    }

    /// Drain worker messages; upload textures for the current generation.
    pub fn poll(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.rx else { return };
        while let Ok(msg) = rx.try_recv() {
            match msg {
                WorkerMsg::Listing { gen, listing } => {
                    if gen == self.generation {
                        self.listing = listing;
                        self.scanning = false;
                    }
                }
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
            }
        }
        if self.scanning {
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

        // Global keys (gated by keyboard focus so typing doesn't navigate).
        let typing = ctx.wants_keyboard_input();
        let (up_key, enter_key) = ctx.input(|i| {
            let up = !typing
                && (i.key_pressed(egui::Key::Backspace)
                    || (i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft)));
            let enter = !typing && i.key_pressed(egui::Key::Enter);
            (up, enter)
        });
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

        egui::Window::new(format!("{} Browse Folders", icons::ICON_FOLDER))
            .open(open)
            .collapsible(false)
            .resizable(true)
            .default_size(egui::vec2(760.0, 480.0))
            .min_size(egui::vec2(400.0, 300.0))
            .max_size(max)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                // --- Top bar ---
                ui.horizontal(|ui| {
                    if ui.button(format!("{} Up", icons::ICON_UP)).clicked() {
                        if let Some(parent) = self.current.parent().map(|p| p.to_path_buf()) {
                            navigate_to = Some(parent);
                        }
                    }
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.path_edit)
                            .desired_width(ui.available_width())
                            .text_color(if self.path_invalid {
                                egui::Color32::from_rgb(235, 87, 87)
                            } else {
                                colors.primary
                            }),
                    );
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.jump_to_path();
                        text_consumed_enter = true;
                    }
                });

                // Breadcrumbs.
                ui.horizontal_wrapped(|ui| {
                    let crumbs = breadcrumbs(&self.current);
                    for (i, (label, path)) in crumbs.iter().enumerate() {
                        if i > 0 {
                            ui.label("›");
                        }
                        if ui
                            .add(egui::Button::new(egui::RichText::new(label).small()).frame(false))
                            .clicked()
                        {
                            navigate_to = Some(path.clone());
                        }
                    }
                });

                // View + filter + files toggle (wraps on narrow windows).
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
                    ui.selectable_value(&mut self.tile_size, config::FolderTileSize::List, "List");
                    ui.separator();
                    ui.label("Show:");
                    ui.selectable_value(
                        &mut self.filter,
                        config::ContentFilter::Images,
                        "Images",
                    );
                    ui.selectable_value(&mut self.filter, config::ContentFilter::Media, "Media");
                    ui.selectable_value(&mut self.filter, config::ContentFilter::All, "All");
                    ui.separator();
                    ui.checkbox(&mut self.show_files, "Show files");
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
                        ui.set_width(140.0);
                        ui.label(egui::RichText::new("Quick Access").strong());
                        for (label, path) in quick_access() {
                            if ui
                                .add(
                                    egui::Button::new(label)
                                        .frame(false)
                                        .min_size(egui::vec2(ui.available_width(), 20.0)),
                                )
                                .clicked()
                            {
                                navigate_to = Some(path);
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
                    egui::ScrollArea::vertical()
                        .id_source("folder-browser-content")
                        .auto_shrink([false, false])
                        .max_height(body_h)
                        .show(ui, |ui| {
                            if self.scanning && self.listing.folders.is_empty() {
                                ui.horizontal(|ui| {
                                    ui.add(egui::Spinner::new());
                                    ui.label("Scanning…");
                                });
                            }

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

                ui.separator();

                // --- Footer ---
                let selected_display = self
                    .selected
                    .clone()
                    .unwrap_or_else(|| self.current.clone());
                ui.horizontal(|ui| {
                    ui.label("Selected:");
                    ui.add(
                        egui::Label::new(selected_display.display().to_string()).truncate(),
                    )
                    .on_hover_text(selected_display.display().to_string());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Select Folder").clicked() {
                            result = BrowserResult::Select(
                                self.selected.clone().unwrap_or_else(|| self.current.clone()),
                            );
                        }
                        if ui.button("Cancel").clicked() {
                            result = BrowserResult::Cancel;
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
        let (tile_w, tile_h, _thumb) = tile_geom(self.tile_size);
        let folder_count = self.listing.folders.len();
        let file_count = self.listing.files.len();
        if folder_count + file_count == 0 {
            return;
        }

        // Natural wrapping; the enclosing ScrollArea provides vertical scroll.
        ui.horizontal_wrapped(|ui| {
            for index in 0..folder_count {
                self.render_folder_tile(ui, colors, tile_w, tile_h, index, navigate_to, select_path);
            }
            for index in 0..file_count {
                self.render_file_tile(ui, colors, tile_w, tile_h, index);
            }
        });
    }

    fn render_folder_tile(
        &mut self,
        ui: &mut egui::Ui,
        colors: &theme::ThemeColors,
        tile_w: f32,
        tile_h: f32,
        index: usize,
        navigate_to: &mut Option<PathBuf>,
        select_path: &mut Option<PathBuf>,
    ) {
        let Some(entry) = self.listing.folders.get(index) else { return };
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(tile_w, tile_h), egui::Sense::click());
        let painter = ui.painter();
        let card = rect.shrink(4.0);
        let selected = self.selected.as_deref() == Some(entry.path.as_path());

        painter.rect_filled(card, 6.0, colors.placeholder);
        if selected {
            painter.rect_stroke(card, 6.0, egui::Stroke::new(2.0_f32, colors.primary));
        } else if response.hovered() {
            painter.rect_stroke(card, 6.0, egui::Stroke::new(1.0_f32, colors.muted));
        }

        // Cover preview.
        let thumb_side = thumb_side(self.tile_size) as f32;
        let thumb_box = egui::Rect::from_center_size(
            egui::pos2(card.center().x, card.min.y + 6.0 + thumb_side / 2.0),
            egui::vec2(thumb_side, thumb_side),
        );
        match self.thumbs.get(&entry.path) {
            Some(cover) => {
                let img_rect = fit_rect_contain(cover.dims, thumb_box);
                painter.image(
                    cover.texture.id(),
                    img_rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            None => {
                painter.text(
                    thumb_box.center(),
                    egui::Align2::CENTER_CENTER,
                    icons::ICON_FOLDER,
                    egui::FontId::proportional(thumb_side * 0.5),
                    colors.muted,
                );
            }
        }

        // Name.
        painter.text(
            egui::pos2(card.min.x + 6.0, card.max.y - 16.0),
            egui::Align2::LEFT_CENTER,
            truncate(&entry.name, 20),
            egui::FontId::proportional(11.0),
            colors.primary,
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
        tile_w: f32,
        tile_h: f32,
        index: usize,
    ) {
        let Some(entry) = self.listing.files.get(index) else { return };
        let (rect, _response) =
            ui.allocate_exact_size(egui::vec2(tile_w, tile_h), egui::Sense::hover());
        let painter = ui.painter();
        let card = rect.shrink(4.0);

        // Dimmed (display-only) background.
        painter.rect_filled(card, 6.0, egui::Color32::from_white_alpha(6));

        let thumb_side = thumb_side(self.tile_size) as f32;
        let thumb_box = egui::Rect::from_center_size(
            egui::pos2(card.center().x, card.min.y + 6.0 + thumb_side / 2.0),
            egui::vec2(thumb_side, thumb_side),
        );
        match entry.kind {
            FileKind::Image => {
                if let Some(cover) = self.thumbs.get(&entry.path) {
                    let img_rect = fit_rect_contain(cover.dims, thumb_box);
                    painter.image(
                        cover.texture.id(),
                        img_rect,
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        egui::Color32::from_white_alpha(160),
                    );
                }
            }
            FileKind::Video => {
                painter.text(
                    thumb_box.center(),
                    egui::Align2::CENTER_CENTER,
                    icons::ICON_FILMSTRIP,
                    egui::FontId::proportional(thumb_side * 0.5),
                    colors.muted,
                );
            }
            FileKind::Other => {}
        }

        painter.text(
            egui::pos2(card.min.x + 6.0, card.max.y - 16.0),
            egui::Align2::LEFT_CENTER,
            truncate(&entry.name, 20),
            egui::FontId::proportional(11.0),
            colors.muted,
        );
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

        for index in 0..folder_count {
            let entry = &self.listing.folders[index];
            let selected = self.selected.as_deref() == Some(entry.path.as_path());
            let text = egui::RichText::new(format!("{}  {}", icons::ICON_FOLDER, entry.name))
                .small();
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
                ui.label(egui::RichText::new(icon).color(colors.muted));

                // The trailing "kind" and "size" columns have a fixed width.
                // Reserve it and let the name truncate to the remainder; if the
                // name consumed all available width first, the trailing columns
                // would overflow and expand the resizable window to its maximum
                // width (the same bug class as the grid).
                let small = egui::FontId::proportional(11.0);
                let trailing_w = ui.fonts(|f| {
                    f.layout_no_wrap(kind.to_owned(), small.clone(), egui::Color32::WHITE)
                        .size()
                        .x
                        + f.layout_no_wrap(size_str.clone(), small.clone(), egui::Color32::WHITE)
                            .size()
                            .x
                }) + ui.spacing().item_spacing.x * 2.0;
                let name_w = (ui.available_width() - trailing_w).max(30.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(name_w, ui.spacing().interact_size.y),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.add(egui::Label::new(&entry.name).truncate());
                    },
                );

                ui.label(egui::RichText::new(kind).small().color(colors.muted));
                ui.label(egui::RichText::new(size_str).small().color(colors.muted));
            });
        }
    }
}

// --- Helpers ---

fn thumb_side(size: config::FolderTileSize) -> u32 {
    match size {
        config::FolderTileSize::Small => 56,
        config::FolderTileSize::Medium => 84,
        config::FolderTileSize::Large => 124,
        config::FolderTileSize::List => 20,
    }
}

fn tile_geom(size: config::FolderTileSize) -> (f32, f32, u32) {
    match size {
        config::FolderTileSize::Small => (76.0, 88.0, 56),
        config::FolderTileSize::Medium => (104.0, 116.0, 84),
        config::FolderTileSize::Large => (144.0, 156.0, 124),
        config::FolderTileSize::List => (0.0, 24.0, 20),
    }
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

/// Scan `dir` on a background thread: folders + filtered files + thumbnails.
fn scan_directory(
    dir: &Path,
    filter: config::ContentFilter,
    show_files: bool,
    thumb_side: u32,
) -> (Listing, Vec<(PathBuf, Vec<u8>, [usize; 2])>) {
    let mut folders = Vec::new();
    let mut files = Vec::new();
    let mut thumbs = Vec::new();
    let mut truncated = false;

    let Ok(entries) = std::fs::read_dir(dir) else {
        return (Listing { folders, files, truncated: false }, thumbs);
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        // Skip hidden/system entries (folders AND files, e.g. desktop.ini).
        if is_hidden_system(&meta) {
            continue;
        }

        if meta.is_dir() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let cover = find_cover(&path);
            folders.push(FolderEntry { name, path: path.clone() });
            if let Some(cover_path) = cover {
                if thumbs.len() < MAX_THUMBS {
                    if let Some((rgba, dims)) = make_thumb(&cover_path, thumb_side) {
                        thumbs.push((path, rgba, dims));
                    }
                }
            }
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
                if kind == FileKind::Image && thumbs.len() < MAX_THUMBS {
                    if let Some((rgba, dims)) = make_thumb(&path, thumb_side) {
                        thumbs.push((path.clone(), rgba, dims));
                    }
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

    (Listing { folders, files, truncated }, thumbs)
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

fn quick_access() -> Vec<(String, PathBuf)> {
    let mut items = Vec::new();
    if let Some(d) = dirs::desktop_dir() {
        items.push(("Desktop".to_string(), d));
    }
    if let Some(d) = dirs::picture_dir() {
        items.push(("Pictures".to_string(), d));
    }
    if let Some(d) = dirs::document_dir() {
        items.push(("Documents".to_string(), d));
    }
    for drive in platform::list_logical_drives() {
        items.push((drive.clone(), PathBuf::from(&drive)));
    }
    items
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

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() > max_chars {
        let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    } else {
        s.to_string()
    }
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
