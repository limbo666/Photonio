// File: src/app.rs
// Purpose: Photonio — top-level egui application state and UI drawing.
//          Owns the directory listing, viewport (zoom/pan), config, context
//          menu, help/about/settings modals, branded empty state, and HUD.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use eframe::egui;

use crate::{about, assets, config, filesystem, loader, platform, viewer};

/// Color used for inline failure notices.
const ERROR_COLOR: egui::Color32 = egui::Color32::from_rgb(235, 87, 87);
/// Color used for success feedback in the HUD.
const SUCCESS_COLOR: egui::Color32 = egui::Color32::from_rgb(120, 220, 150);

/// Raw input collected once per frame, then acted on.
struct InputSnapshot {
    ctrl: bool,
    scroll: f32,
    pointer_pos: Option<egui::Pos2>,
    pointer_delta: egui::Vec2,
    primary_down: bool,
    double_clicked: bool,
    secondary_clicked: bool,
    prev: bool,
    next: bool,
    reset_fit: bool,
    reset_100: bool,
    rotate: bool,
    open_file: bool,
    open_folder: bool,
    copy_path: bool,
    reveal: bool,
    delete: bool,
    escape: bool,
    toggle_fullscreen: bool,
    help: bool,
    close_file: bool,
    close_folder: bool,
    open_settings: bool,
    toggle_filmstrip: bool,
}

/// Button actions reported back from the HUD.
#[derive(Default)]
struct HudActions {
    open_file: bool,
    open_folder: bool,
    toggle_filmstrip: bool,
}

/// An action selected from the context menu.
enum MenuAction {
    OpenFile,
    OpenFolder,
    CloseImage,
    CloseFolder,
    OpenContainingFolder,
    OpenWith,
    CopyFile,
    CopyImage,
    CopyPath,
    SetWallpaper,
    Delete,
    Settings,
    About,
    Help,
    Exit,
}

/// A cached thumbnail: its GPU texture plus the (low-res) pixel dimensions,
/// kept so the filmstrip can draw it and the main view can use it as an
/// instant placeholder while the full-res decode is still in flight.
struct Thumb {
    texture: egui::TextureHandle,
    dims: [usize; 2],
}

pub struct PhotonioApp {
    /// Currently displayed image (decoded + GPU texture), if any.
    image: Option<viewer::ViewerImage>,
    /// Low-res thumbnail shown in the main viewport while the full-res decode
    /// for the current index is still in flight — keeps the main window in
    /// sync with the filmstrip during fast navigation.
    preview: Option<(egui::TextureHandle, [usize; 2])>,
    /// Human-readable reason the last load failed, if it did.
    error: Option<String>,
    /// Naturally-sorted image files in the current directory.
    files: Vec<PathBuf>,
    /// Index into `files` of the currently displayed image.
    current_index: usize,
    /// `None` = fit to window; `Some(z)` = explicit zoom (1.0 = 100%).
    zoom: Option<f32>,
    /// Pan offset in points, applied after centering.
    pan: egui::Vec2,
    /// Persisted state (settings + last directory + window geometry).
    config: config::Config,
    /// Window outer rect, refreshed each frame for persistence.
    window_rect: Option<egui::Rect>,
    /// Position of the open context menu (`Some` = open).
    context_menu_pos: Option<egui::Pos2>,
    /// Modal visibility flags.
    show_help: bool,
    show_about: bool,
    show_settings: bool,
    /// Set when help just opened, to ignore the same-frame click.
    help_opened_this_frame: bool,
    /// Transient feedback badge: (message, when shown).
    feedback: Option<(String, std::time::Instant)>,
    /// The branded watermark logo texture + dimensions, if decoded.
    logo: Option<(egui::TextureHandle, [usize; 2])>,
    /// Background image decoder + preload channel manager.
    loader: loader::Loader,
    /// Cached display-res RGBA buffers by file index: (rgba, display, full).
    full_cache: HashMap<usize, (Vec<u8>, [usize; 2], [usize; 2])>,
    /// Indices with a full decode currently in flight (dedup).
    pending_full: HashSet<usize>,
    /// Indices with a thumbnail decode currently in flight (dedup + retry).
    pending_thumb: HashSet<usize>,
    /// File index currently awaiting a full decode (to show a spinner).
    loading_index: Option<usize>,
    /// Cached thumbnails by file index.
    thumb_cache: HashMap<usize, Thumb>,
}

impl PhotonioApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: config::Config) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        // Load the embedded logo once for the empty-state watermark + About.
        let logo = assets::logo_color_image().map(|(color, dims)| {
            let texture = cc
                .egui_ctx
                .load_texture("photonio-logo", color, egui::TextureOptions::LINEAR);
            (texture, dims)
        });

        let mut app = Self {
            image: None,
            preview: None,
            error: None,
            files: Vec::new(),
            current_index: 0,
            zoom: None,
            pan: egui::Vec2::ZERO,
            config,
            window_rect: None,
            context_menu_pos: None,
            show_help: false,
            show_about: false,
            show_settings: false,
            help_opened_this_frame: false,
            feedback: None,
            logo,
            loader: loader::Loader::new(),
            full_cache: HashMap::new(),
            pending_full: HashSet::new(),
            pending_thumb: HashSet::new(),
            loading_index: None,
            thumb_cache: HashMap::new(),
        };

        // Resume the last directory on startup, if configured.
        if app.config.remember_last_folder {
            if let Some(dir) = app.config.last_directory.clone() {
                app.open_directory(dir, &cc.egui_ctx);
            }
        }

        app
    }

    // ------------------------------------------------------------------
    // Loading & navigation
    // ------------------------------------------------------------------

    /// Show `index`: use a cached buffer instantly, or request a background
    /// decode and keep the previous image until it arrives.
    fn navigate_to(&mut self, index: usize, ctx: &egui::Context) {
        if index >= self.files.len() {
            return;
        }
        self.current_index = index;
        self.loader.set_current_index(index);

        if let Some((rgba, display, full)) = self.full_cache.get(&index).cloned() {
            self.preview = None;
            self.set_image_from_rgba(rgba, display, full, ctx);
            self.loading_index = None;
        } else {
            self.loading_index = Some(index);
            // Drop any stale placeholder, then show this image's thumbnail
            // immediately so the main view tracks the filmstrip while the
            // full-res decode runs; swap in full-res when it lands.
            self.preview = None;
            self.set_preview(index);
            // Request only if not already cached or in flight.
            if !self.pending_full.contains(&index) {
                if let Some(path) = self.files.get(index).cloned() {
                    self.pending_full.insert(index);
                    self.loader.request_full(index, path);
                }
            }
        }

        self.request_preload(index);
        self.evict_caches();
    }

    /// Upload a decoded RGBA buffer as the current image.
    fn set_image_from_rgba(
        &mut self,
        rgba: Vec<u8>,
        display: [usize; 2],
        full: [usize; 2],
        ctx: &egui::Context,
    ) {
        self.image = Some(viewer::ViewerImage::from_rgba(ctx, rgba, display, full));
        self.error = None;
        self.reset_viewport();
    }

    /// Show `index`'s cached thumbnail as a low-res placeholder in the main
    /// viewport. No-op when that thumbnail isn't decoded yet.
    fn set_preview(&mut self, index: usize) {
        if let Some(thumb) = self.thumb_cache.get(&index) {
            self.preview = Some((thumb.texture.clone(), thumb.dims));
            self.reset_viewport();
        }
    }

    /// Speculatively preload full images adjacent to `index`, plus thumbnails.
    fn request_preload(&mut self, index: usize) {
        let len = self.files.len() as i64;
        let cur = index as i64;
        for delta in [-1i64, 1] {
            let i = cur + delta;
            if i >= 0 && i < len {
                let i = i as usize;
                if !self.full_cache.contains_key(&i) && !self.pending_full.contains(&i) {
                    if let Some(path) = self.files.get(i).cloned() {
                        self.pending_full.insert(i);
                        self.loader.request_full(i, path);
                    }
                }
            }
        }
        self.request_thumbs(index);
    }

    /// Request low-res thumbnails for the visible filmstrip window (circular,
    /// matching `draw_filmstrip`). Idempotent — skips cached/in-flight entries —
    /// and retried every frame so dropped requests are never permanently lost.
    fn request_thumbs(&mut self, index: usize) {
        if !self.config.show_filmstrip {
            return;
        }
        let len = self.files.len();
        let window: Vec<usize> = if len >= 7 {
            (-3isize..=3)
                .map(|off| (index as isize + off).rem_euclid(len as isize) as usize)
                .collect()
        } else {
            (0..len).collect()
        };
        for i in window {
            if self.thumb_cache.contains_key(&i) || self.pending_thumb.contains(&i) {
                continue;
            }
            if let Some(path) = self.files.get(i).cloned() {
                if self.loader.request_thumb(i, path) {
                    self.pending_thumb.insert(i);
                }
            }
        }
    }

    /// Guarantee the currently-viewed image always has a full decode queued.
    /// A coalescing skip drops a request without a normal response, which can
    /// leave `pending_full` stale; retry here every frame (like thumbnails)
    /// so the full-res image reliably replaces the low-res placeholder.
    fn ensure_current_full_queued(&mut self) {
        let index = self.current_index;
        // Only retry while we're actively waiting on this index (`loading_index`
        // is cleared once the full image is shown or a load definitively fails).
        if self.loading_index != Some(index) {
            return;
        }
        if self.full_cache.contains_key(&index) || self.pending_full.contains(&index) {
            return;
        }
        if let Some(path) = self.files.get(index).cloned() {
            self.pending_full.insert(index);
            self.loader.request_full(index, path);
        }
    }

    /// Drain ready decode responses and update caches / the current image.
    fn poll_loader(&mut self, ctx: &egui::Context) {
        let current = self.loader.generation();
        while let Some(resp) = self.loader.try_recv() {
            if resp.generation != current {
                continue; // stale session → discard
            }
            match resp.kind {
                loader::ResponseKind::Full {
                    index,
                    rgba,
                    display,
                    full,
                } => {
                    self.pending_full.remove(&index);
                    self.full_cache.insert(index, (rgba.clone(), display, full));
                    if self.loading_index == Some(index) && index == self.current_index {
                        self.preview = None;
                        self.set_image_from_rgba(rgba, display, full, ctx);
                        self.loading_index = None;
                    }
                }
                loader::ResponseKind::FullFailed { index, error } => {
                    self.pending_full.remove(&index);
                    if self.loading_index == Some(index) && index == self.current_index {
                        self.preview = None;
                        self.image = None;
                        self.error = Some(error);
                        self.loading_index = None;
                    }
                }
                loader::ResponseKind::FullSkipped { index } => {
                    // Coalesced away: drop the pending flag so the per-frame
                    // `ensure_current_full_queued` can re-request it if it's
                    // the image currently being viewed.
                    self.pending_full.remove(&index);
                }
                loader::ResponseKind::Thumb { index, rgba, dims } => {
                    self.pending_thumb.remove(&index);
                    let color = egui::ColorImage::from_rgba_unmultiplied(dims, &rgba);
                    let tex = ctx.load_texture(
                        format!("thumb-{index}"),
                        color,
                        egui::TextureOptions::LINEAR,
                    );
                    self.thumb_cache.insert(index, Thumb { texture: tex, dims });
                    // If this is the image we're still waiting on, show it now.
                    if self.loading_index == Some(index) && index == self.current_index {
                        self.set_preview(index);
                    }
                }
                loader::ResponseKind::ThumbFailed => {}
            }
        }
    }

    /// Clear all decode caches (used when the file list changes).
    fn clear_caches(&mut self) {
        // Invalidate any in-flight/queued decode work from a previous session.
        self.loader.bump_generation();
        self.full_cache.clear();
        self.pending_full.clear();
        self.thumb_cache.clear();
        self.pending_thumb.clear();
        self.loading_index = None;
        self.preview = None;
    }

    /// Evict cache entries far from the current index to bound memory.
    fn evict_caches(&mut self) {
        let cur = self.current_index as i64;
        self.full_cache.retain(|&i, _| (i as i64 - cur).abs() <= 3);
        self.thumb_cache.retain(|&i, _| (i as i64 - cur).abs() <= 5);
    }

    fn step(&mut self, delta: isize, ctx: &egui::Context) {
        let len = self.files.len();
        if len == 0 {
            return;
        }
        let next = if self.config.loop_navigation {
            // Wrap around both ends (ring-shaped).
            (self.current_index as isize + delta).rem_euclid(len as isize) as usize
        } else {
            // Clamp at the boundaries (no wrap).
            (self.current_index as isize + delta).clamp(0, len as isize - 1) as usize
        };
        if next != self.current_index {
            self.navigate_to(next, ctx);
        }
    }

    fn current_file(&self) -> Option<&PathBuf> {
        self.files.get(self.current_index)
    }

    fn open_file(&mut self, path: PathBuf, ctx: &egui::Context) {
        if !filesystem::is_supported_path(&path) {
            self.image = None;
            self.preview = None;
            self.error = Some(format!("Unsupported file: {}", path.display()));
            return;
        }

        self.clear_caches();
        match path.parent() {
            Some(dir) => {
                self.config.last_directory = Some(dir.to_path_buf());
                self.save_config();
                self.files = filesystem::scan_directory(dir);
                let index = self.files.iter().position(|p| *p == path).unwrap_or(0);
                self.navigate_to(index, ctx);
            }
            None => {
                self.files = vec![path];
                self.navigate_to(0, ctx);
            }
        }
    }

    fn open_directory(&mut self, dir: PathBuf, ctx: &egui::Context) {
        self.config.last_directory = Some(dir.clone());
        self.save_config();

        self.clear_caches();
        self.files = filesystem::scan_directory(&dir);
        if self.files.is_empty() {
            self.image = None;
            self.error = Some(format!("No supported images found in {}", dir.display()));
            self.current_index = 0;
            self.reset_viewport();
        } else {
            self.current_index = 0;
            self.navigate_to(0, ctx);
        }
    }

    fn ingest_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone()) // path is Option<PathBuf>
                .collect()
        });

        if let Some(path) = dropped.into_iter().next() {
            self.open_file(path, ctx);
        }
    }

    // ------------------------------------------------------------------
    // Closing (session management, no disk change)
    // ------------------------------------------------------------------

    fn close_current_file(&mut self, ctx: &egui::Context) {
        if self.files.is_empty() {
            return;
        }
        self.context_menu_pos = None;
        self.files.remove(self.current_index);
        self.advance_after_removal(ctx);
        self.show_status("File closed");
    }

    fn close_folder(&mut self) {
        self.context_menu_pos = None;
        self.files.clear();
        self.image = None;
        self.error = None;
        self.current_index = 0;
        self.reset_viewport();
        self.clear_caches();
        self.show_status("Folder closed");
    }

    fn advance_after_removal(&mut self, ctx: &egui::Context) {
        self.clear_caches();
        let len = self.files.len();
        if len == 0 {
            self.image = None;
            self.error = None;
            self.current_index = 0;
            self.reset_viewport();
        } else {
            let next = self.current_index.min(len - 1);
            self.navigate_to(next, ctx);
        }
    }

    // ------------------------------------------------------------------
    // Dialogs & shell actions
    // ------------------------------------------------------------------

    fn open_file_dialog(&mut self, ctx: &egui::Context) {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Open Image")
            .add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp", "gif", "ico"]);
        if let Some(dir) = &self.config.last_directory {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.open_file(path, ctx);
        }
    }

    fn open_folder_dialog(&mut self, ctx: &egui::Context) {
        let mut dialog = rfd::FileDialog::new().set_title("Open Folder");
        if let Some(dir) = &self.config.last_directory {
            dialog = dialog.set_directory(dir);
        }
        if let Some(dir) = dialog.pick_folder() {
            self.open_directory(dir, ctx);
        }
    }

    fn copy_current_path(&mut self, ctx: &egui::Context) {
        if let Some(path) = self.current_file() {
            ctx.copy_text(path.display().to_string());
            self.show_status("Path copied");
        }
    }

    fn copy_current_file(&mut self) {
        let Some(path) = self.current_file().cloned() else {
            return;
        };
        match platform::copy_file_to_clipboard(&path) {
            Ok(()) => self.show_status("File copied"),
            Err(e) => self.show_status(format!("Copy file failed: {e}")),
        }
    }

    fn copy_current_image(&mut self) {
        let (bytes, width, height) = match &self.image {
            Some(image) => (image.rgba_bytes(), image.dimensions[0], image.dimensions[1]),
            None => return,
        };

        match arboard::Clipboard::new() {
            Ok(mut clipboard) => {
                let data = arboard::ImageData {
                    width,
                    height,
                    bytes: std::borrow::Cow::Owned(bytes),
                };
                match clipboard.set_image(data) {
                    Ok(()) => self.show_status("Image copied"),
                    Err(e) => self.show_status(format!("Copy image failed: {e}")),
                }
            }
            Err(e) => self.show_status(format!("Clipboard unavailable: {e}")),
        }
    }

    fn set_wallpaper_current(&mut self) {
        let Some(path) = self.current_file().cloned() else {
            return;
        };
        match platform::set_wallpaper(&path) {
            Ok(()) => self.show_status("Wallpaper set"),
            Err(e) => self.show_status(format!("Wallpaper failed: {e}")),
        }
    }

    fn reveal_current(&self) {
        if let Some(path) = self.current_file() {
            platform::reveal_in_explorer(path);
        }
    }

    fn open_with_current(&self) {
        if let Some(path) = self.current_file() {
            platform::open_with(path);
        }
    }

    fn delete_current(&mut self, ctx: &egui::Context) {
        let Some(path) = self.current_file().cloned() else {
            return;
        };
        self.context_menu_pos = None;

        match trash::delete(&path) {
            Ok(()) => {
                self.show_status("Deleted");
                self.files.remove(self.current_index);
                self.advance_after_removal(ctx);
            }
            Err(e) => self.show_status(format!("Delete failed: {e}")),
        }
    }

    // ------------------------------------------------------------------
    // Viewport state
    // ------------------------------------------------------------------

    fn reset_viewport(&mut self) {
        self.zoom = None;
        self.pan = egui::Vec2::ZERO;
    }

    fn effective_zoom(&self, dimensions: [usize; 2], available: egui::Vec2) -> f32 {
        self.zoom
            .unwrap_or_else(|| viewer::fit_scale(dimensions, available))
    }

    fn rotate_current(&mut self, ctx: &egui::Context) {
        if let Some(image) = &mut self.image {
            image.rotate_90_cw(ctx);
        }
    }

    // ------------------------------------------------------------------
    // Input
    // ------------------------------------------------------------------

    fn capture_input(ctx: &egui::Context) -> InputSnapshot {
        ctx.input(|i| InputSnapshot {
            ctrl: i.modifiers.ctrl,
            scroll: i.raw_scroll_delta.y,
            pointer_pos: i.pointer.interact_pos(),
            pointer_delta: i.pointer.delta(),
            primary_down: i.pointer.primary_down(),
            double_clicked: i.pointer.button_double_clicked(egui::PointerButton::Primary),
            secondary_clicked: i.pointer.secondary_clicked(),
            prev: i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::A),
            next: i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::D),
            reset_fit: i.key_pressed(egui::Key::Num0),
            reset_100: i.key_pressed(egui::Key::Num1),
            rotate: i.key_pressed(egui::Key::R),
            open_file: i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(egui::Key::O),
            open_folder: i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::O),
            copy_path: i.modifiers.ctrl && i.key_pressed(egui::Key::C),
            reveal: i.modifiers.ctrl && i.key_pressed(egui::Key::E),
            delete: i.key_pressed(egui::Key::Delete),
            escape: i.key_pressed(egui::Key::Escape),
            toggle_fullscreen: i.key_pressed(egui::Key::F11)
                || (i.modifiers.alt && i.key_pressed(egui::Key::Enter)),
            help: i.key_pressed(egui::Key::F1) || i.key_pressed(egui::Key::H),
            close_file: i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(egui::Key::W),
            close_folder: i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::W),
            open_settings: i.modifiers.ctrl && i.key_pressed(egui::Key::Comma),
            toggle_filmstrip: i.key_pressed(egui::Key::T),
        })
    }

    fn apply_hotkeys(&mut self, ctx: &egui::Context, input: &InputSnapshot) {
        if input.rotate {
            self.rotate_current(ctx);
        }
        if input.reset_fit {
            self.reset_viewport();
        }
        if input.reset_100 {
            self.zoom = Some(1.0);
            self.pan = egui::Vec2::ZERO;
        }
        if input.double_clicked {
            self.zoom = if self.zoom.is_none() { Some(1.0) } else { None };
            self.pan = egui::Vec2::ZERO;
        }
    }

    fn toggle_fullscreen(ctx: &egui::Context) {
        let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
    }

    // ------------------------------------------------------------------
    // Modals (mutually exclusive)
    // ------------------------------------------------------------------

    fn open_help(&mut self) {
        self.show_help = true;
        self.show_about = false;
        self.show_settings = false;
        self.context_menu_pos = None;
        self.help_opened_this_frame = true;
    }

    fn open_about(&mut self) {
        self.show_about = true;
        self.show_help = false;
        self.show_settings = false;
        self.context_menu_pos = None;
    }

    fn open_settings(&mut self) {
        self.show_settings = true;
        self.show_help = false;
        self.show_about = false;
        self.context_menu_pos = None;
    }

    // ------------------------------------------------------------------
    // Persistence & notifications
    // ------------------------------------------------------------------

    fn save_config(&mut self) {
        if let Some(rect) = self.window_rect {
            self.config.window_pos = Some([rect.min.x, rect.min.y]);
            self.config.window_size = Some([rect.width(), rect.height()]);
        }
        self.config.save();
    }

    fn show_status(&mut self, message: impl Into<String>) {
        self.feedback = Some((message.into(), std::time::Instant::now()));
    }

    fn take_status_text(&mut self) -> Option<String> {
        let (message, at) = self.feedback.as_ref()?;
        if at.elapsed() > std::time::Duration::from_secs(2) {
            self.feedback = None;
            return None;
        }
        Some(format!("✓ {message}"))
    }

    // ------------------------------------------------------------------
    // Drawing
    // ------------------------------------------------------------------

    fn draw_canvas(&mut self, ui: &mut egui::Ui, input: &InputSnapshot) {
        let rect = ui.max_rect();

        if input.secondary_clicked && input.pointer_pos.map_or(false, |p| rect.contains(p)) {
            self.context_menu_pos = input.pointer_pos;
            self.show_help = false;
            self.show_about = false;
            self.show_settings = false;
        }

        let (texture_id, dimensions) = if let Some((tex, dims)) = &self.preview {
            (tex.id(), *dims)
        } else if let Some(image) = &self.image {
            (image.texture.id(), image.dimensions)
        } else {
            self.draw_no_image(ui);
            return;
        };

        let center = rect.center();
        let available = ui.available_size();

        // Wheel zoom when Ctrl is held, or always in "Zoom" wheel mode.
        let wheel_zooms = self.config.mouse_wheel_action == config::MouseWheelAction::Zoom;
        if (input.ctrl || wheel_zooms) && input.scroll != 0.0 {
            let old = self.effective_zoom(dimensions, available);
            let factor = (input.scroll * 0.003).exp();
            let new = (old * factor).clamp(0.02, 64.0);
            match input.pointer_pos {
                Some(cursor) => {
                    let rel = cursor - center;
                    self.pan = rel - (rel - self.pan) * (new / old);
                }
                None => self.pan *= new / old,
            }
            self.zoom = Some(new);
        }

        let zoom = self.effective_zoom(dimensions, available);
        let draw = viewer::draw_size(dimensions, zoom);
        let overflows = draw.x > available.x || draw.y > available.y;
        let over_canvas = input.pointer_pos.map_or(false, |p| rect.contains(p));
        let over_hud = ui.ctx().is_pointer_over_area();
        if overflows && input.primary_down && over_canvas && !over_hud {
            self.pan += input.pointer_delta;
        }

        let zoom = self.effective_zoom(dimensions, available);
        let draw = viewer::draw_size(dimensions, zoom);
        let top_left = center - draw * 0.5 + self.pan;

        ui.painter().image(
            texture_id,
            egui::Rect::from_min_size(top_left, draw),
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }

    /// Draw the error or branded empty state, dead-centered via an Area.
    fn draw_no_image(&self, ui: &mut egui::Ui) {
        // A background decode is in flight → show a centered spinner.
        if self.loading_index.is_some() {
            egui::Area::new(egui::Id::new("loading-spinner"))
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .order(egui::Order::Middle)
                .show(ui.ctx(), |ui| {
                    ui.add(egui::Spinner::new());
                });
            return;
        }

        match &self.error {
            Some(err) => {
                let message = format!("Failed to load image: {err}");
                egui::Area::new(egui::Id::new("error-state"))
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .order(egui::Order::Middle)
                    .show(ui.ctx(), |ui| {
                        ui.colored_label(ERROR_COLOR, message);
                    });
            }
            None => self.draw_empty_state(ui.ctx()),
        }
    }

    /// Branded empty state: a soft-opacity logo watermark + helper text,
    /// anchored at the exact viewport center.
    fn draw_empty_state(&self, ctx: &egui::Context) {
        egui::Area::new(egui::Id::new("empty-state"))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .order(egui::Order::Middle)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    if let Some((texture, dims)) = &self.logo {
                        let max = 180.0;
                        let scale = (max / dims[0] as f32).min(max / dims[1] as f32).min(1.0);
                        let size = egui::vec2(dims[0] as f32 * scale, dims[1] as f32 * scale);
                        let tint = egui::Color32::from_white_alpha(90);
                        ui.add(egui::Image::new(texture).fit_to_exact_size(size).tint(tint));
                    }
                    ui.add_space(16.0);
                    let text_color = match self.config.canvas_background {
                        config::CanvasBackground::Light => egui::Color32::from_rgb(60, 60, 66),
                        _ => egui::Color32::from_rgb(205, 205, 214),
                    };
                    ui.label(
                        egui::RichText::new("Drop an image or press Ctrl+O to open")
                            .color(text_color),
                    );
                });
            });
    }

    fn glass_frame(opacity: u8) -> egui::Frame {
        egui::Frame::none()
            .fill(egui::Color32::from_rgba_unmultiplied(20, 20, 25, opacity))
            .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(40)))
            .rounding(10.0)
            .inner_margin(egui::Margin::symmetric(14.0, 8.0))
            .shadow(egui::Shadow {
                offset: egui::vec2(0.0, 4.0),
                blur: 18.0,
                spread: 0.0,
                color: egui::Color32::from_black_alpha(150),
            })
    }

    fn draw_hud(&mut self, ctx: &egui::Context) -> HudActions {
        let mut actions = HudActions::default();

        if self.files.is_empty() {
            return actions;
        }
        // Hidden mode: no HUD bar (navigation via hotkeys/wheel only).
        if self.config.hud_position == config::HudPosition::Hidden {
            return actions;
        }

        let total = self.files.len();
        let position = self.current_index + 1;
        let name = self
            .files
            .get(self.current_index)
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let status = self.take_status_text();
        let opacity = self.config.glass_opacity;
        let (anchor, offset) = match self.config.hud_position {
            config::HudPosition::Top => (egui::Align2::CENTER_TOP, egui::vec2(0.0, 12.0)),
            _ => (egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -12.0)),
        };

        let mut clicked_prev = false;
        let mut clicked_next = false;

        egui::Area::new(egui::Id::new("photonio-hud"))
            .anchor(anchor, offset)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                Self::glass_frame(opacity).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .add(egui::Button::new("<").frame(false).min_size(egui::vec2(24.0, 20.0)))
                            .clicked()
                        {
                            clicked_prev = true;
                        }

                        ui.label(
                            egui::RichText::new(format!("{position} / {total}")).monospace(),
                        );
                        ui.add_space(8.0);

                        if let Some(status) = status {
                            ui.label(egui::RichText::new(status).color(SUCCESS_COLOR));
                        } else {
                            ui.label(name);
                        }

                        if ui
                            .add(egui::Button::new(">").frame(false).min_size(egui::vec2(24.0, 20.0)))
                            .clicked()
                        {
                            clicked_next = true;
                        }

                        ui.separator();

                        if ui.add(egui::Button::new("Open File").frame(false)).clicked() {
                            actions.open_file = true;
                        }
                        if ui.add(egui::Button::new("Open Folder").frame(false)).clicked() {
                            actions.open_folder = true;
                        }
                        if ui.add(egui::Button::new("Filmstrip").frame(false)).clicked() {
                            actions.toggle_filmstrip = true;
                        }
                    });
                });
            });

        if clicked_prev {
            self.step(-1, ctx);
        } else if clicked_next {
            self.step(1, ctx);
        }

        actions
    }

    fn draw_filmstrip(&mut self, ctx: &egui::Context) {
        if !self.config.show_filmstrip || self.files.is_empty() {
            return;
        }
        let opacity = self.config.glass_opacity;
        let len = self.files.len();

        // Circular wrap: 3 before + current + 3 after, looping around the ends.
        // Folders with fewer than 7 images show every image once (no duplicates).
        let indices: Vec<usize> = if len >= 7 {
            (-3isize..=3)
                .map(|off| (self.current_index as isize + off).rem_euclid(len as isize) as usize)
                .collect()
        } else {
            (0..len).collect()
        };
        let mut clicked: Option<usize> = None;

        egui::Area::new(egui::Id::new("filmstrip"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -56.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                Self::glass_frame(opacity).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for idx in indices {
                            let size = 72.0;
                            let (rect, response) =
                                ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
                            let painter = ui.painter();
                            match self.thumb_cache.get(&idx).map(|t| t.texture.id()) {
                                Some(id) => {
                                    painter.image(
                                        id,
                                        rect,
                                        egui::Rect::from_min_max(
                                            egui::Pos2::ZERO,
                                            egui::pos2(1.0, 1.0),
                                        ),
                                        egui::Color32::WHITE,
                                    );
                                }
                                None => {
                                    painter.rect_filled(
                                        rect,
                                        6.0,
                                        egui::Color32::from_rgba_unmultiplied(30, 32, 42, 220),
                                    );
                                }
                            }
                            if idx == self.current_index {
                                painter.rect_stroke(
                                    rect,
                                    6.0,
                                    egui::Stroke::new(
                                        2.0_f32,
                                        egui::Color32::from_rgba_unmultiplied(255, 200, 50, 220),
                                    ),
                                );
                            } else if response.hovered() {
                                painter.rect_stroke(
                                    rect,
                                    6.0,
                                    egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(90)),
                                );
                            }
                            if response.clicked() {
                                clicked = Some(idx);
                            }
                        }
                    });
                });
            });

        if let Some(idx) = clicked {
            self.navigate_to(idx, ctx);
        }
    }

    fn draw_context_menu(&mut self, ctx: &egui::Context) -> Option<MenuAction> {
        let raw_pos = self.context_menu_pos?;

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

        let current = self.current_file().cloned();
        let has_file = current.is_some();
        let dims = self.image.as_ref().map(|im| im.dimensions).unwrap_or([0, 0]);

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
        let opacity = self.config.glass_opacity;

        let response = egui::Area::new(egui::Id::new("context-menu"))
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                Self::glass_frame(opacity).show(ui, |ui| {
                    ui.set_min_width(210.0);
                    ui.set_max_width(230.0);

                    if let Some((filename, size, format)) = &header {
                        ui.label(egui::RichText::new(filename).strong());
                        ui.label(format!("{} x {} px", dims[0], dims[1]));
                        ui.label(format!("{}  ·  {}", format_size(*size), format));
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
                    if menu_item_enabled(ui, "Open Containing Folder", has_file).clicked() {
                        action = Some(MenuAction::OpenContainingFolder);
                        should_close = true;
                    }
                    if menu_item_enabled(ui, "Open With...", has_file).clicked() {
                        action = Some(MenuAction::OpenWith);
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
                    if menu_item_enabled(ui, "Set as Desktop Wallpaper", has_file).clicked() {
                        action = Some(MenuAction::SetWallpaper);
                        should_close = true;
                    }
                    if menu_item_enabled(ui, "Move to Recycle Bin", has_file).clicked() {
                        action = Some(MenuAction::Delete);
                        should_close = true;
                    }

                    ui.separator();
                    if menu_item(ui, "Settings...").clicked() {
                        action = Some(MenuAction::Settings);
                        should_close = true;
                    }
                    if menu_item(ui, "About Photonio").clicked() {
                        action = Some(MenuAction::About);
                        should_close = true;
                    }
                    if menu_item(ui, "Help / Shortcuts").clicked() {
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
            self.context_menu_pos = None;
        }

        action
    }

    fn draw_help(&mut self, ctx: &egui::Context) {
        if !self.show_help {
            return;
        }
        let just_opened = std::mem::take(&mut self.help_opened_this_frame);
        let mut open_about = false;
        let opacity = self.config.glass_opacity;

        let response = egui::Area::new(egui::Id::new("help-overlay"))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                Self::glass_frame(opacity).show(ui, |ui| {
                    ui.set_min_width(320.0);
                    ui.heading("Shortcuts");
                    ui.separator();

                    let rows: &[(&str, &str)] = &[
                        ("Previous / Next", "← → / A D / Wheel"),
                        ("Zoom in / out", "Ctrl + Wheel"),
                        ("Fit to window", "0"),
                        ("Actual size (100%)", "1"),
                        ("Pan", "Left-drag (when zoomed)"),
                        ("Rotate 90° CW", "R"),
                        ("Copy path", "Ctrl + C"),
                        ("Open file", "Ctrl + O"),
                        ("Open folder", "Ctrl + Shift + O"),
                        ("Close image", "Ctrl + W"),
                        ("Close folder", "Ctrl + Shift + W"),
                        ("Show in Explorer", "Ctrl + E"),
                        ("Settings", "Ctrl + ,"),
                        ("Move to Recycle Bin", "Del"),
                        ("Toggle fullscreen", "F11 / Alt+Enter"),
                        ("Close this help", "Esc / H / F1"),
                    ];

                    egui::Grid::new("shortcut-grid")
                        .num_columns(2)
                        .spacing(egui::vec2(24.0, 6.0))
                        .show(ui, |ui| {
                            for (action, keys) in rows {
                                ui.label(*action);
                                ui.label(egui::RichText::new(*keys).monospace());
                                ui.end_row();
                            }
                        });

                    ui.separator();
                    if ui.button("About Photonio").clicked() {
                        open_about = true;
                    }
                });
            });

        if open_about {
            self.open_about();
        }

        if !just_opened {
            let rect = response.response.rect;
            let clicked_outside = ctx.input(|i| {
                (i.pointer.primary_clicked() || i.pointer.secondary_clicked())
                    && i
                        .pointer
                        .interact_pos()
                        .map_or(true, |p| !rect.expand(2.0).contains(p))
            });
            if clicked_outside {
                self.show_help = false;
            }
        }
    }

    fn draw_settings(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut changed = false;
        let opacity = self.config.glass_opacity;

        egui::Area::new(egui::Id::new("settings-modal"))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                Self::glass_frame(opacity).show(ui, |ui| {
                    ui.set_min_width(380.0);
                    ui.heading("Settings");
                    ui.separator();

                    // General
                    ui.label(egui::RichText::new("General").strong());
                    changed |= ui
                        .checkbox(
                            &mut self.config.remember_last_folder,
                            "Remember last opened folder on startup",
                        )
                        .changed();
                    changed |= ui
                        .checkbox(
                            &mut self.config.loop_navigation,
                            "Loop navigation at folder boundary",
                        )
                        .changed();
                    ui.add_space(4.0);
                    ui.label("Mouse wheel action:");
                    changed |= ui
                        .radio_value(
                            &mut self.config.mouse_wheel_action,
                            config::MouseWheelAction::Navigate,
                            "Next / Previous image",
                        )
                        .changed();
                    changed |= ui
                        .radio_value(
                            &mut self.config.mouse_wheel_action,
                            config::MouseWheelAction::Zoom,
                            "Zoom in / out",
                        )
                        .changed();

                    // Navigation
                    ui.separator();
                    ui.label(egui::RichText::new("Navigation").strong());
                    changed |= ui
                        .radio_value(
                            &mut self.config.hud_position,
                            config::HudPosition::Bottom,
                            "HUD on bottom",
                        )
                        .changed();
                    changed |= ui
                        .radio_value(
                            &mut self.config.hud_position,
                            config::HudPosition::Top,
                            "HUD on top",
                        )
                        .changed();
                    changed |= ui
                        .radio_value(
                            &mut self.config.hud_position,
                            config::HudPosition::Hidden,
                            "Hide HUD",
                        )
                        .changed();
                    changed |= ui
                        .checkbox(&mut self.config.show_filmstrip, "Show Thumbnail Filmstrip")
                        .changed();

                    // Window & Appearance
                    ui.separator();
                    ui.label(egui::RichText::new("Window & Appearance").strong());
                    changed |= ui
                        .checkbox(&mut self.config.start_fullscreen, "Start in Fullscreen")
                        .changed();
                    ui.add_space(4.0);
                    ui.label("Canvas background:");
                    changed |= ui
                        .radio_value(
                            &mut self.config.canvas_background,
                            config::CanvasBackground::Glass,
                            "Translucent Glass",
                        )
                        .changed();
                    changed |= ui
                        .radio_value(
                            &mut self.config.canvas_background,
                            config::CanvasBackground::Dark,
                            "Dark Solid",
                        )
                        .changed();
                    changed |= ui
                        .radio_value(
                            &mut self.config.canvas_background,
                            config::CanvasBackground::Light,
                            "Light Solid",
                        )
                        .changed();
                    ui.add_space(4.0);
                    ui.label("Glassmorphism tint / opacity:");
                    changed |= ui
                        .add(egui::Slider::new(&mut self.config.glass_opacity, 50..=255))
                        .changed();

                    ui.separator();
                    if ui.button("Close").clicked() {
                        self.show_settings = false;
                    }
                });
            });

        if changed {
            self.save_config();
        }
    }

    fn execute_menu_action(&mut self, action: MenuAction, ctx: &egui::Context) {
        match action {
            MenuAction::OpenFile => self.open_file_dialog(ctx),
            MenuAction::OpenFolder => self.open_folder_dialog(ctx),
            MenuAction::CloseImage => self.close_current_file(ctx),
            MenuAction::CloseFolder => self.close_folder(),
            MenuAction::OpenContainingFolder => self.reveal_current(),
            MenuAction::OpenWith => self.open_with_current(),
            MenuAction::CopyFile => self.copy_current_file(),
            MenuAction::CopyImage => self.copy_current_image(),
            MenuAction::CopyPath => self.copy_current_path(ctx),
            MenuAction::SetWallpaper => self.set_wallpaper_current(),
            MenuAction::Delete => self.delete_current(ctx),
            MenuAction::Settings => self.open_settings(),
            MenuAction::About => self.open_about(),
            MenuAction::Help => self.open_help(),
            MenuAction::Exit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }
}

/// A flat, full-width, enabled menu item.
fn menu_item(ui: &mut egui::Ui, text: &str) -> egui::Response {
    menu_item_enabled(ui, text, true)
}

/// A flat, full-width menu item, optionally disabled/dimmed.
fn menu_item_enabled(ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(text)
            .frame(false)
            .min_size(egui::vec2(ui.available_width(), 20.0)),
    )
}

/// Truncate a string to `max_chars`, appending an ellipsis.
fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() > max_chars {
        let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    } else {
        s.to_string()
    }
}

/// Human-readable file size, e.g. `2.4 MB`.
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

/// Display format name derived from the file extension.
fn format_name(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("png") => "PNG".into(),
        Some("jpg") | Some("jpeg") => "JPEG".into(),
        Some("webp") => "WebP".into(),
        Some("bmp") => "BMP".into(),
        Some("gif") => "GIF".into(),
        Some("ico") => "ICO".into(),
        Some(other) => other.to_uppercase(),
        None => "Unknown".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_long_strings_with_ellipsis() {
        assert_eq!(truncate("short.png", 28), "short.png");
        let t = truncate("a_very_long_filename_that_needs_ellipsis.png", 20);
        assert!(t.ends_with('…'));
        assert!(t.chars().count() <= 20);
    }

    #[test]
    fn formats_file_sizes() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(2_500_000), "2.4 MB");
    }

    #[test]
    fn embedded_logo_decodes() {
        assert!(assets::logo_color_image().is_some());
    }
}

impl eframe::App for PhotonioApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let color = match self.config.canvas_background {
            config::CanvasBackground::Glass => egui::Color32::TRANSPARENT,
            config::CanvasBackground::Dark => egui::Color32::from_rgb(20, 20, 25),
            config::CanvasBackground::Light => egui::Color32::from_rgb(245, 245, 248),
        };
        color.to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ingest_drops(ctx);
        self.poll_loader(ctx);
        // Retry visible-but-missing thumbnails every frame (handles dropped
        // requests during fast scrolling and the wrapped left-edge tiles).
        self.request_thumbs(self.current_index);
        // Re-queue the active image's full decode if a coalescing skip dropped
        // it, so the placeholder is always replaced by the real image.
        self.ensure_current_full_queued();

        let input = Self::capture_input(ctx);

        let (window_rect, close_requested) = ctx.input(|i| {
            (i.viewport().outer_rect, i.viewport().close_requested())
        });
        if window_rect.is_some() {
            self.window_rect = window_rect;
        }

        // Navigation keys; wheel navigates only in "Navigate" mode.
        let mut navigate: isize = 0;
        if input.prev {
            navigate -= 1;
        }
        if input.next {
            navigate += 1;
        }
        let wheel_navigates = self.config.mouse_wheel_action == config::MouseWheelAction::Navigate;
        if wheel_navigates && !input.ctrl && input.scroll != 0.0 {
            if input.scroll > 0.0 {
                navigate -= 1;
            } else {
                navigate += 1;
            }
        }

        self.apply_hotkeys(ctx, &input);

        if input.toggle_fullscreen {
            Self::toggle_fullscreen(ctx);
        }

        if input.help {
            if self.show_help {
                self.show_help = false;
            } else {
                self.open_help();
            }
        }
        if input.open_settings {
            self.open_settings();
        }
        if input.toggle_filmstrip {
            self.config.show_filmstrip = !self.config.show_filmstrip;
            self.save_config();
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                self.draw_canvas(ui, &input);
            });

        if navigate != 0 {
            self.step(navigate, ctx);
        }

        let hud = self.draw_hud(ctx);
        if hud.toggle_filmstrip {
            self.config.show_filmstrip = !self.config.show_filmstrip;
            self.save_config();
        }

        if let Some(action) = self.draw_context_menu(ctx) {
            self.execute_menu_action(action, ctx);
        }

        self.draw_help(ctx);
        self.draw_settings(ctx);
        self.draw_filmstrip(ctx);

        if input.delete {
            self.delete_current(ctx);
        }
        if input.close_file {
            self.close_current_file(ctx);
        }
        if input.close_folder {
            self.close_folder();
        }

        // Escape priority: fullscreen → settings → about → help → menu → exit.
        if input.escape {
            let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            if fullscreen {
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            } else if self.show_settings {
                self.show_settings = false;
            } else if self.show_about {
                self.show_about = false;
            } else if self.show_help {
                self.show_help = false;
            } else if self.context_menu_pos.is_some() {
                self.context_menu_pos = None;
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        if input.open_file || hud.open_file {
            self.open_file_dialog(ctx);
        }
        if input.open_folder || hud.open_folder {
            self.open_folder_dialog(ctx);
        }
        if input.copy_path {
            self.copy_current_path(ctx);
        }
        if input.reveal {
            self.reveal_current();
        }

        if close_requested {
            self.save_config();
        }

        // About modal — rendered last, directly on ctx, outside every panel.
        about::AboutDialog::show(
            ctx,
            &mut self.show_about,
            self.logo.as_ref().map(|(texture, _)| texture),
            self.config.glass_opacity,
        );
    }
}
