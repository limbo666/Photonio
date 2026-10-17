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
const WARN_COLOR: egui::Color32 = egui::Color32::from_rgb(255, 170, 40);
const ERROR_COLOR: egui::Color32 = egui::Color32::from_rgb(235, 87, 87);

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
        if let Some(d) = dirs::download_dir() {
            folders.push(("Downloads".to_string(), d));
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
    pub back_stack: Vec<PathBuf>,
    pub forward_stack: Vec<PathBuf>,
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
    is_creating_folder: bool,
    new_folder_name: String,
    new_folder_focus_pending: bool,
    new_folder_error: Option<String>,
    path_create_prompt: Option<PathBuf>,
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
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
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
            is_creating_folder: false,
            new_folder_name: String::new(),
            new_folder_focus_pending: false,
            new_folder_error: None,
            path_create_prompt: None,
        };
        b.navigate_internal(start);
        b
    }

    pub fn start_create_folder(&mut self) {
        let default_name = unique_subfolder_name(&self.current, "New Folder");
        self.new_folder_name = default_name;
        self.is_creating_folder = true;
        self.new_folder_focus_pending = true;
        self.new_folder_error = None;
        self.path_create_prompt = None;
    }

    fn commit_create_folder(&mut self) -> Option<PathBuf> {
        let trimmed = self.new_folder_name.trim();
        if trimmed.is_empty() {
            self.new_folder_error = Some("Folder name cannot be empty".into());
            return None;
        }
        if trimmed.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*']) {
            self.new_folder_error = Some("Name contains invalid characters".into());
            return None;
        }
        let target = self.current.join(trimmed);
        if target.exists() {
            self.new_folder_error = Some("A folder with this name already exists".into());
            return None;
        }
        match std::fs::create_dir(&target) {
            Ok(()) => {
                self.is_creating_folder = false;
                self.new_folder_error = None;
                self.navigate_internal(self.current.clone());
                Some(target)
            }
            Err(e) => {
                self.new_folder_error = Some(format!("Create failed: {e}"));
                None
            }
        }
    }

    fn push_history(&mut self, from: PathBuf) {
        if self.back_stack.last() == Some(&from) {
            return;
        }
        self.back_stack.push(from);
        if self.back_stack.len() > 50 {
            self.back_stack.remove(0);
        }
        self.forward_stack.clear();
    }

    /// Navigate back to the previous folder in history.
    pub fn navigate_back(&mut self) -> bool {
        while let Some(prev) = self.back_stack.pop() {
            if prev.is_dir() {
                let cur = self.current.clone();
                if self.forward_stack.last() != Some(&cur) {
                    self.forward_stack.push(cur);
                    if self.forward_stack.len() > 50 {
                        self.forward_stack.remove(0);
                    }
                }
                self.navigate_internal(prev);
                return true;
            }
        }
        false
    }

    /// Navigate forward to the next folder in history.
    pub fn navigate_forward(&mut self) -> bool {
        while let Some(next) = self.forward_stack.pop() {
            if next.is_dir() {
                let cur = self.current.clone();
                if self.back_stack.last() != Some(&cur) {
                    self.back_stack.push(cur);
                    if self.back_stack.len() > 50 {
                        self.back_stack.remove(0);
                    }
                }
                self.navigate_internal(next);
                return true;
            }
        }
        false
    }

    /// Navigate to `path`: records the previous folder in history so the user can
    /// go Back/Forward, then enumerates direct children synchronously.
    pub fn navigate(&mut self, path: PathBuf) {
        if path != self.current {
            let cur = self.current.clone();
            self.push_history(cur);
        }
        self.navigate_internal(path);
    }

    /// Low-level folder navigation without recording back/forward history stacks.
    pub fn navigate_internal(&mut self, path: PathBuf) {
        self.current = path.clone();
        self.selected = None;
        self.temp_path_buffer = path.display().to_string();
        self.is_editing_path = false;
        self.path_invalid = false;
        self.is_creating_folder = false;
        self.new_folder_name.clear();
        self.new_folder_error = None;
        self.path_create_prompt = None;
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

        let mut navigate_back = false;
        let mut navigate_forward = false;

        // Global keys (gated by keyboard focus and address-bar edit mode).
        let typing = ctx.wants_keyboard_input();
        let (back_key, forward_key, up_key, enter_key) = ctx.input(|i| {
            let nav = !typing && !self.is_editing_path;
            let back = nav
                && (i.key_pressed(egui::Key::Backspace)
                    || (i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft)));
            let forward = nav && (i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight));
            let up = nav && (i.modifiers.alt && i.key_pressed(egui::Key::ArrowUp));
            let enter = nav && i.key_pressed(egui::Key::Enter);
            (back, forward, up, enter)
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
                    ui.spacing_mut().item_spacing = egui::vec2(2.0, 0.0);

                    // 1. Back button (chevron-left)
                    let can_back = !self.back_stack.is_empty();
                    let back_tooltip = if let Some(p) = self.back_stack.last() {
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
                    let can_forward = !self.forward_stack.is_empty();
                    let forward_tooltip = if let Some(p) = self.forward_stack.last() {
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

                    // 3. Up button (arrow-up)
                    let can_up = self.current.parent().is_some();
                    if colors
                        .nav_icon_button(
                            ui,
                            icons::ICON_UP,
                            13.0,
                            egui::vec2(26.0, 26.0),
                            can_up,
                            "Go to parent folder (Alt+Up)",
                        )
                        .clicked()
                    {
                        if let Some(parent) = self.current.parent().map(|p| p.to_path_buf()) {
                            navigate_to = Some(parent);
                        }
                    }

                    ui.add_space(2.0);

                    // Inset address-bar frame (input-like).
                    let frame_stroke = if self.path_invalid {
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
                            ui.set_min_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(3.0, 0.0);

                                // Folder icon on the left edge inside the address bar
                                let folder_resp = colors.nav_icon_button(
                                    ui,
                                    icons::ICON_FOLDER,
                                    13.0,
                                    egui::vec2(22.0, 22.0),
                                    true,
                                    "Folder path (Click to edit)",
                                );
                                if folder_resp.clicked() {
                                    self.is_editing_path = true;
                                    self.temp_path_buffer = self.current.display().to_string();
                                    self.path_invalid = false;
                                    self.address_focus_pending = true;
                                }

                                if self.is_editing_path {
                                    // Raw path edit mode.
                                    let mut output = egui::TextEdit::singleline(
                                        &mut self.temp_path_buffer,
                                    )
                                    .desired_width(f32::INFINITY)
                                    .frame(false)
                                    .text_color(if self.path_invalid {
                                        ERROR_COLOR
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
                                            self.path_create_prompt = None;
                                        } else {
                                            self.path_invalid = true;
                                            self.path_create_prompt = Some(path);
                                            self.is_creating_folder = false;
                                        }
                                    } else if esc || (output.response.lost_focus() && self.path_create_prompt.is_none()) {
                                        self.is_editing_path = false;
                                        self.path_invalid = false;
                                        self.temp_path_buffer = self.current.display().to_string();
                                    }
                                } else {
                                    // Breadcrumb mode.
                                    let mut enter_edit = false;
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing = egui::vec2(2.0, 0.0);
                                        let crumbs = breadcrumbs(&self.current);
                                        let crumb_count = crumbs.len();
                                        for (i, (label, path)) in crumbs.iter().enumerate() {
                                            if i > 0 {
                                                ui.label(
                                                    egui::RichText::new("›")
                                                        .size(12.0)
                                                        .color(colors.muted),
                                                );
                                            }
                                            let is_last = i == crumb_count - 1;
                                            let text = if is_last {
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
                });
                ui.separator();

                if let Some(target) = self.path_create_prompt.clone() {
                    let folder_name = target
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| target.display().to_string());
                    egui::Frame::none()
                        .fill(if colors.dark_mode {
                            egui::Color32::from_black_alpha(90)
                        } else {
                            egui::Color32::from_black_alpha(20)
                        })
                        .stroke(egui::Stroke::new(1.0_f32, WARN_COLOR))
                        .rounding(4.0)
                        .inner_margin(egui::Margin::symmetric(10.0, 5.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} Folder \"{}\" does not exist.",
                                        icons::ICON_WARN,
                                        folder_name
                                    ))
                                    .color(WARN_COLOR),
                                );
                                let create_btn = colors.button_primary(
                                    ui,
                                    format!("{} Create & Open", icons::ICON_FOLDER),
                                    egui::vec2(0.0, 24.0),
                                    true,
                                );
                                let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
                                if create_btn.clicked() || enter_pressed {
                                    match std::fs::create_dir_all(&target) {
                                        Ok(()) => {
                                            self.path_create_prompt = None;
                                            self.is_editing_path = false;
                                            self.path_invalid = false;
                                            navigate_to = Some(target.clone());
                                        }
                                        Err(e) => {
                                            self.new_folder_error = Some(format!("Create failed: {e}"));
                                        }
                                    }
                                }
                                let esc_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));
                                if colors
                                    .button_secondary(
                                        ui,
                                        format!("{} Cancel", icons::ICON_XMARK),
                                        egui::vec2(0.0, 24.0),
                                        true,
                                    )
                                    .clicked()
                                    || esc_pressed
                                {
                                    self.path_create_prompt = None;
                                    self.path_invalid = false;
                                    self.is_editing_path = false;
                                    self.temp_path_buffer = self.current.display().to_string();
                                }
                            });
                        });
                    ui.separator();
                }

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
                        let toolbar_h = 24.0;
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), toolbar_h),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(3.0, 0.0);
                                ui.label("View:");
                                colors.segmented_value(
                                    ui,
                                    &mut self.tile_size,
                                    config::FolderTileSize::Small,
                                    "Small",
                                );
                                colors.segmented_value(
                                    ui,
                                    &mut self.tile_size,
                                    config::FolderTileSize::Medium,
                                    "Medium",
                                );
                                colors.segmented_value(
                                    ui,
                                    &mut self.tile_size,
                                    config::FolderTileSize::Large,
                                    "Large",
                                );
                                colors.segmented_value(
                                    ui,
                                    &mut self.tile_size,
                                    config::FolderTileSize::List,
                                    "List",
                                );
                                ui.add_space(4.0);
                                ui.separator();
                                ui.add_space(4.0);
                                ui.label("Show:");
                                colors.segmented_value(
                                    ui,
                                    &mut self.filter,
                                    config::ContentFilter::Images,
                                    "Images",
                                );
                                colors.segmented_value(
                                    ui,
                                    &mut self.filter,
                                    config::ContentFilter::Media,
                                    "Media",
                                );
                                colors.segmented_value(
                                    ui,
                                    &mut self.filter,
                                    config::ContentFilter::All,
                                    "All",
                                );
                                ui.add_space(4.0);
                                ui.separator();
                                ui.add_space(4.0);
                                ui.checkbox(&mut self.show_files, "Show files")
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let can_create = self.current.is_dir();
                                    let btn = colors.button_secondary(
                                        ui,
                                        format!("{} New Folder", icons::ICON_PLUS),
                                        egui::vec2(105.0, 24.0),
                                        can_create,
                                    );
                                    if btn
                                        .on_hover_text("Create a new subfolder in the current directory")
                                        .clicked()
                                    {
                                        self.start_create_folder();
                                    }
                                });
                            },
                        );
                    });
                ui.separator();

                if self.is_creating_folder {
                    let frame_stroke = if self.new_folder_error.is_some() {
                        egui::Stroke::new(1.0_f32, ERROR_COLOR)
                    } else {
                        egui::Stroke::new(1.0_f32, colors.primary)
                    };
                    egui::Frame::none()
                        .fill(if colors.dark_mode {
                            egui::Color32::from_black_alpha(80)
                        } else {
                            egui::Color32::from_black_alpha(15)
                        })
                        .stroke(frame_stroke)
                        .rounding(4.0)
                        .inner_margin(egui::Margin::symmetric(10.0, 5.0))
                        .show(ui, |ui| {
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), 24.0),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    ui.label(
                                        egui::RichText::new(format!("{} Name:", icons::ICON_FOLDER))
                                            .color(colors.primary),
                                    );
                                    let mut output = egui::TextEdit::singleline(&mut self.new_folder_name)
                                        .desired_width(200.0)
                                        .show(ui);

                                    if self.new_folder_focus_pending {
                                        output.response.request_focus();
                                        let len = self.new_folder_name.len();
                                        let range = egui::text::CCursorRange::two(
                                            egui::text::CCursor::new(0),
                                            egui::text::CCursor::new(len),
                                        );
                                        output.state.cursor.set_char_range(Some(range));
                                        output.state.store(ui.ctx(), output.response.id);
                                        self.new_folder_focus_pending = false;
                                    }

                                    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                                    let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));

                                    let create_btn = colors.button_primary(
                                        ui,
                                        format!("{} Create", icons::ICON_CHECK),
                                        egui::vec2(75.0, 24.0),
                                        true,
                                    );
                                    if create_btn.clicked() || (output.response.has_focus() && enter) {
                                        if let Some(created) = self.commit_create_folder() {
                                            select_path = Some(created);
                                        }
                                    }
                                    let cancel_btn = colors.button_secondary(
                                        ui,
                                        format!("{} Cancel", icons::ICON_XMARK),
                                        egui::vec2(75.0, 24.0),
                                        true,
                                    );
                                    if cancel_btn.clicked() || esc {
                                        self.is_creating_folder = false;
                                        self.new_folder_error = None;
                                    }
                                    if let Some(err) = &self.new_folder_error {
                                        ui.label(
                                            egui::RichText::new(err)
                                                .color(ERROR_COLOR)
                                                .small(),
                                        );
                                    }
                                },
                            );
                        });
                    ui.separator();
                }

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
                                let exists = path.is_dir();
                                let name = path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| path.display().to_string());
                                let (icon, text_color, tip_suffix) = if exists {
                                    (icons::ICON_FOLDER, colors.gold, "")
                                } else {
                                    (icons::ICON_WARN, WARN_COLOR, " (deleted or missing)")
                                };
                                let display_name = if exists {
                                    truncate_label(&name, 22)
                                } else {
                                    format!("{} (missing)", truncate_label(&name, 13))
                                };
                                let text = format!("{} {}", icon, display_name);
                                // Flat button (not a selectable label) so the cursor
                                // stays a pointing hand instead of a text I-beam.
                                let resp = ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new(text).color(text_color),
                                        )
                                        .frame(false)
                                        .min_size(egui::vec2(ui.available_width(), 20.0)),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                let resp = resp.on_hover_text(format!("{}{tip_suffix}", path.display()));
                                if resp.clicked() {
                                    navigate_to = Some(path.clone());
                                }
                                resp.context_menu(|ui| {
                                    if !exists {
                                        if ui.button("+ Recreate Folder").clicked() {
                                            if std::fs::create_dir_all(path).is_ok() {
                                                navigate_to = Some(path.clone());
                                            }
                                            ui.close_menu();
                                        }
                                    }
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
                                if !self.current.exists() {
                                    ui.add_space(30.0);
                                    ui.vertical_centered(|ui| {
                                        ui.label(
                                            egui::RichText::new(icons::ICON_WARN)
                                                .size(28.0)
                                                .color(WARN_COLOR),
                                        );
                                        ui.add_space(6.0);
                                        let name = self
                                            .current
                                            .file_name()
                                            .map(|n| n.to_string_lossy().into_owned())
                                            .unwrap_or_else(|| self.current.display().to_string());
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "Folder \"{name}\" was not found"
                                            ))
                                            .strong()
                                            .size(15.0),
                                        );
                                        ui.label(
                                            egui::RichText::new(self.current.display().to_string())
                                                .color(colors.muted)
                                                .small(),
                                        );
                                        ui.add_space(14.0);

                                        ui.horizontal(|ui| {
                                            ui.add_space(
                                                (ui.available_width() - 260.0).max(0.0) / 2.0,
                                            );
                                            if colors
                                                .button_primary(
                                                    ui,
                                                    format!("{} Recreate Folder", icons::ICON_PLUS),
                                                    egui::vec2(0.0, 26.0),
                                                    true,
                                                )
                                                .clicked()
                                            {
                                                if std::fs::create_dir_all(&self.current).is_ok() {
                                                    navigate_to = Some(self.current.clone());
                                                }
                                            }
                                            ui.add_space(8.0);
                                            if let Some(parent) =
                                                self.current.parent().map(|p| p.to_path_buf())
                                            {
                                                if parent.is_dir() {
                                                    if colors
                                                        .button_secondary(
                                                            ui,
                                                            format!(
                                                                "{} Go to Parent",
                                                                icons::ICON_UP
                                                            ),
                                                            egui::vec2(0.0, 26.0),
                                                            true,
                                                        )
                                                        .clicked()
                                                    {
                                                        navigate_to = Some(parent);
                                                    }
                                                }
                                            }
                                        });

                                        let is_pinned = self.pinned.contains(&self.current);
                                        if is_pinned {
                                            ui.add_space(10.0);
                                            if colors
                                                .button_secondary(
                                                    ui,
                                                    format!(
                                                        "{} Remove from Quick Access",
                                                        icons::ICON_TRASH
                                                    ),
                                                    egui::vec2(0.0, 26.0),
                                                    true,
                                                )
                                                .clicked()
                                            {
                                                result = BrowserResult::Unpin(self.current.clone());
                                            }
                                        }
                                    });
                                } else {
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
                let target = self.selected.clone().unwrap_or_else(|| self.current.clone());
                let can_select = target.is_dir();
                let pin_target = target.clone();
                let is_pinned = self.pinned.contains(&pin_target);
                // Left-to-right: readout anchored far left, buttons far right.
                let footer_h = 28.0;
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), footer_h),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        // Far left: bounded readout so long names truncate before
                        // crowding the right-aligned buttons.
                        let label_max = (ui.available_width() - 420.0).max(60.0);
                        ui.allocate_ui_with_layout(
                            egui::vec2(label_max, footer_h),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                let resp = ui.add(egui::Label::new(selected_text).truncate());
                                resp.on_hover_text(full_path.display().to_string());
                            },
                        );
                        // Far right: [Pin to Quick Access] —— [Select Folder] —— [Cancel].
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);

                            // 1. Cancel button (secondary, xmark icon)
                            if colors
                                .button_secondary(
                                    ui,
                                    format!("{} Cancel", icons::ICON_XMARK),
                                    egui::vec2(90.0, 28.0),
                                    true,
                                )
                                .clicked()
                            {
                                result = BrowserResult::Cancel;
                            }

                            // 2. Select Folder button (primary, check icon)
                            let select_btn = colors.button_primary(
                                ui,
                                format!("{} Select Folder", icons::ICON_CHECK),
                                egui::vec2(125.0, 28.0),
                                can_select,
                            );
                            if select_btn.clicked() {
                                result = BrowserResult::Select(target);
                            }
                            if !can_select {
                                select_btn.on_hover_text("Cannot select a non-existing folder");
                            }

                            ui.add_space(8.0);

                            // 3. Pin to Quick Access button (accent, pin icon, larger size 168x28)
                            if is_pinned {
                                colors.button_gold(
                                    ui,
                                    format!("{} Pinned", icons::ICON_CHECK),
                                    egui::vec2(168.0, 28.0),
                                    false,
                                );
                            } else {
                                let pin_btn = colors.button_gold(
                                    ui,
                                    format!("{} Pin to Quick Access", icons::ICON_PIN),
                                    egui::vec2(168.0, 28.0),
                                    can_select,
                                );
                                if pin_btn.clicked() {
                                    result = BrowserResult::Pin(pin_target);
                                }
                                if !can_select {
                                    pin_btn.on_hover_text("Cannot pin a non-existing folder");
                                }
                            }
                        });
                    },
                );
            });

        // The window's close (X) button flips `open` directly; bail out early.
        if !*open {
            return BrowserResult::Cancel;
        }

        // Keyboard navigation (only when the modal is still open and no button
        // already fired).
        if matches!(result, BrowserResult::None) {
            if back_key {
                if !self.back_stack.is_empty() {
                    navigate_back = true;
                } else if let Some(parent) = self.current.parent().map(|p| p.to_path_buf()) {
                    navigate_to = Some(parent);
                }
            } else if forward_key {
                navigate_forward = true;
            } else if up_key {
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
        if navigate_back {
            self.navigate_back();
        } else if navigate_forward {
            self.navigate_forward();
        } else if let Some(path) = navigate_to {
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
            self.navigate_internal(self.current.clone());
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
        Some("jpg") | Some("jpeg") | Some("jfif") | Some("png") | Some("webp")
        | Some("tif") | Some("tiff") | Some("bmp") | Some("gif") => FileKind::Image,
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
pub(crate) fn normalize_path(input: &str) -> PathBuf {
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

/// Build a non-colliding new folder name in `parent` starting with `base` (e.g. "New Folder (2)").
fn unique_subfolder_name(parent: &Path, base: &str) -> String {
    let first = parent.join(base);
    if !first.exists() {
        return base.to_string();
    }
    for i in 2u32..10_000 {
        let candidate = format!("{base} ({i})");
        if !parent.join(&candidate).exists() {
            return candidate;
        }
    }
    base.to_string()
}

pub(crate) fn breadcrumbs(path: &Path) -> Vec<(String, PathBuf)> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path() {
        assert_eq!(normalize_path("  \"C:\\photos\\trip\"  "), PathBuf::from("C:\\photos\\trip"));
        assert_eq!(normalize_path("C:\\photos\\trip\\"), PathBuf::from("C:\\photos\\trip"));
        assert_eq!(normalize_path("D:"), PathBuf::from("D:\\"));
        assert_eq!(normalize_path("D:\\"), PathBuf::from("D:\\"));
    }

    #[test]
    fn test_breadcrumbs_splits_hierarchy() {
        let path = PathBuf::from("C:\\photos\\vacation\\beach");
        let crumbs = breadcrumbs(&path);
        let labels: Vec<&str> = crumbs.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(labels, vec!["C:\\", "photos", "vacation", "beach"]);
        assert_eq!(crumbs.last().unwrap().1, path);
    }

    #[test]
    fn test_unique_subfolder_name() {
        let unique_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("photonio_test_{unique_id}"));
        let _ = std::fs::create_dir_all(&temp_dir);

        // When base doesn't exist yet
        let name1 = unique_subfolder_name(&temp_dir, "New Folder");
        assert_eq!(name1, "New Folder");

        // Create "New Folder"
        let _ = std::fs::create_dir(temp_dir.join("New Folder"));
        let name2 = unique_subfolder_name(&temp_dir, "New Folder");
        assert_eq!(name2, "New Folder (2)");

        // Create "New Folder (2)"
        let _ = std::fs::create_dir(temp_dir.join("New Folder (2)"));
        let name3 = unique_subfolder_name(&temp_dir, "New Folder");
        assert_eq!(name3, "New Folder (3)");

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_resolve_start_with_missing_folder() {
        let temp_dir = std::env::temp_dir();
        let non_existent = temp_dir.join("definitely_missing_subfolder_12345");
        let resolved = resolve_start(Some(non_existent));
        // It must resolve to an existing directory (its parent temp_dir or higher)
        assert!(resolved.is_dir());
        assert_eq!(resolved, temp_dir);
    }

    #[test]
    fn test_browser_back_forward_navigation() {
        let unique_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base_dir = std::env::temp_dir().join(format!("photonio_browser_nav_{unique_id}"));
        let dir_a = base_dir.join("folder_a");
        let dir_b = base_dir.join("folder_b");
        let dir_c = base_dir.join("folder_c");
        let _ = std::fs::create_dir_all(&dir_a);
        let _ = std::fs::create_dir_all(&dir_b);
        let _ = std::fs::create_dir_all(&dir_c);

        let cfg = config::Config::default();
        let mut browser = Browser::new(dir_a.clone(), &cfg);
        assert_eq!(browser.current, dir_a);
        assert!(browser.back_stack.is_empty());
        assert!(browser.forward_stack.is_empty());

        // Navigate to B, then C
        browser.navigate(dir_b.clone());
        assert_eq!(browser.current, dir_b);
        assert_eq!(browser.back_stack, vec![dir_a.clone()]);
        assert!(browser.forward_stack.is_empty());

        browser.navigate(dir_c.clone());
        assert_eq!(browser.current, dir_c);
        assert_eq!(browser.back_stack, vec![dir_a.clone(), dir_b.clone()]);
        assert!(browser.forward_stack.is_empty());

        // Navigate back to B
        assert!(browser.navigate_back());
        assert_eq!(browser.current, dir_b);
        assert_eq!(browser.back_stack, vec![dir_a.clone()]);
        assert_eq!(browser.forward_stack, vec![dir_c.clone()]);

        // Navigate back to A
        assert!(browser.navigate_back());
        assert_eq!(browser.current, dir_a);
        assert!(browser.back_stack.is_empty());
        assert_eq!(browser.forward_stack, vec![dir_c.clone(), dir_b.clone()]);

        // Cannot navigate back past root history
        assert!(!browser.navigate_back());
        assert_eq!(browser.current, dir_a);

        // Navigate forward to B
        assert!(browser.navigate_forward());
        assert_eq!(browser.current, dir_b);
        assert_eq!(browser.back_stack, vec![dir_a.clone()]);
        assert_eq!(browser.forward_stack, vec![dir_c.clone()]);

        // Branch off: navigate to A again (clears forward stack)
        browser.navigate(dir_a.clone());
        assert_eq!(browser.current, dir_a);
        assert_eq!(browser.back_stack, vec![dir_a.clone(), dir_b.clone()]);
        assert!(browser.forward_stack.is_empty());

        let _ = std::fs::remove_dir_all(&base_dir);
    }
}
