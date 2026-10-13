// File: src/app.rs
// Purpose: Photonio — top-level egui application state and UI drawing.
//          Owns the directory listing, viewport (zoom/pan), config, context
//          menu, help/about/settings modals, branded empty state, and HUD.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use eframe::egui;

use crate::{about, assets, config, filesystem, icons, loader, platform, theme, viewer};

/// Color used for inline failure notices.
const ERROR_COLOR: egui::Color32 = egui::Color32::from_rgb(235, 87, 87);
/// Color used for success feedback in the HUD.
const SUCCESS_COLOR: egui::Color32 = egui::Color32::from_rgb(120, 220, 150);
/// Badge color for the active-GIF indicator in the HUD.
const GIF_COLOR: egui::Color32 = egui::Color32::from_rgb(255, 200, 50);
/// Amber warning color for registry path-mismatch notices.
const WARN_COLOR: egui::Color32 = egui::Color32::from_rgb(255, 170, 40);

/// Register Font Awesome Solid as a fallback font so icon glyphs render
/// natively and consistently across the UI.
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "font_awesome".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/fa-solid-900.otf")),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .push("font_awesome".to_owned());
    ctx.set_fonts(fonts);
}

/// Set the global egui widget visuals to match the backdrop theme, so
/// checkboxes, radio buttons, separators, and buttons follow Light/Dark mode.
fn apply_global_visuals(ctx: &egui::Context, dark_mode: bool) {
    if dark_mode {
        ctx.set_visuals(egui::Visuals::dark());
        return;
    }
    let mut visuals = egui::Visuals::light();
    visuals.widgets.noninteractive.bg_stroke =
        egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(30));
    visuals.widgets.inactive.bg_fill =
        egui::Color32::from_rgba_premultiplied(230, 233, 238, 200);
    visuals.widgets.inactive.bg_stroke =
        egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(40));
    visuals.widgets.hovered.bg_fill =
        egui::Color32::from_rgba_premultiplied(214, 218, 226, 200);
    visuals.widgets.active.bg_fill =
        egui::Color32::from_rgba_premultiplied(198, 204, 214, 220);
    visuals.selection.bg_fill = egui::Color32::from_rgb(255, 200, 50);
    visuals.selection.stroke = egui::Stroke::new(2.0_f32, egui::Color32::from_gray(30));
    ctx.set_visuals(visuals);
}

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
    toggle_play: bool,
    step_back: bool,
    step_forward: bool,
    save_as: bool,
    print: bool,
}

/// An action selected from the quick-action pill.
enum PillAction {
    OpenFile,
    OpenFolder,
    SaveAs,
    Print,
    ToggleFilmstrip,
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

/// Playback state for a multi-frame animation (GIF or WebP). Each frame is a
/// display-res GPU texture; `dimensions` is the shared canvas size for zoom math.
struct AnimationPlayer {
    frames: Vec<egui::TextureHandle>,
    dimensions: [usize; 2],
    delays_ms: Vec<u64>,
    current_frame: usize,
    paused: bool,
    next_frame: std::time::Instant,
    kind: loader::AnimKind,
}

impl AnimationPlayer {
    fn new(
        frames: Vec<(Vec<u8>, [usize; 2], [usize; 2])>,
        delays_ms: Vec<u64>,
        kind: loader::AnimKind,
        ctx: &egui::Context,
    ) -> Self {
        let dimensions = frames.first().map(|f| f.2).unwrap_or([0, 0]);
        let mut textures = Vec::with_capacity(frames.len());
        for (i, (rgba, display, _)) in frames.into_iter().enumerate() {
            let color = egui::ColorImage::from_rgba_unmultiplied(display, &rgba);
            let texture = ctx.load_texture(
                format!("gif-frame-{i}"),
                color,
                egui::TextureOptions::LINEAR,
            );
            textures.push(texture);
        }
        let first_delay = delays_ms.first().copied().unwrap_or(100);
        Self {
            frames: textures,
            dimensions,
            delays_ms,
            current_frame: 0,
            paused: false,
            next_frame: std::time::Instant::now() + std::time::Duration::from_millis(first_delay),
            kind,
        }
    }

    /// The texture id + canvas dimensions of the currently-shown frame.
    fn current_texture(&self) -> (egui::TextureId, [usize; 2]) {
        let id = self
            .frames
            .get(self.current_frame)
            .map(|t| t.id())
            .unwrap_or(egui::TextureId::Managed(0));
        (id, self.dimensions)
    }
}

pub struct PhotonioApp {
    /// Currently displayed image (decoded + GPU texture), if any.
    image: Option<viewer::ViewerImage>,
    /// Low-res thumbnail shown in the main viewport while the full-res decode
    /// for the current index is still in flight — keeps the main window in
    /// sync with the filmstrip during fast navigation.
    preview: Option<(egui::TextureHandle, [usize; 2])>,
    /// Active animated GIF playback state, if the current image is one.
    animation: Option<AnimationPlayer>,
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
    /// Windows registration health, refreshed when Settings opens.
    reg_status: platform::RegistryStatus,
    /// Whether the Explorer folder context menu is registered.
    folder_menu_registered: bool,
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
    /// Indices whose thumbnail decode definitively failed (shown as a
    /// placeholder; never retried until the folder changes).
    failed_thumb: HashSet<usize>,
    /// File index currently awaiting a full decode (to show a spinner).
    loading_index: Option<usize>,
    /// Cached thumbnails by file index.
    thumb_cache: HashMap<usize, Thumb>,
}

impl PhotonioApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        config: config::Config,
        cli_path: Option<PathBuf>,
    ) -> Self {
        apply_global_visuals(
            &cc.egui_ctx,
            config.canvas_background != config::CanvasBackground::Light,
        );
        setup_fonts(&cc.egui_ctx);

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
            animation: None,
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
            reg_status: platform::RegistryStatus::NotRegistered,
            folder_menu_registered: false,
            logo,
            loader: loader::Loader::new(cc.egui_ctx.clone()),
            full_cache: HashMap::new(),
            pending_full: HashSet::new(),
            pending_thumb: HashSet::new(),
            failed_thumb: HashSet::new(),
            loading_index: None,
            thumb_cache: HashMap::new(),
        };

        if let Some(path) = cli_path {
            // Explicit path (CLI / double-click / Open With / folder context
            // menu): skip config restoration and open the target directly.
            if path.is_dir() {
                app.open_directory(path, &cc.egui_ctx);
            } else if path.is_file() {
                app.open_file(path, &cc.egui_ctx);
            }
            // A missing/invalid path falls through to the branded empty state.
        } else if app.config.remember_last_folder {
            // Resume the exact last-viewed image on startup, if configured and it
            // still exists; otherwise fall back to the last folder (index 0), then
            // to the branded empty state.
            if let Some(file) = app.config.last_file_path.clone().filter(|p| p.is_file()) {
                app.open_file(file, &cc.egui_ctx);
            } else if let Some(dir) = app.config.last_directory.clone().filter(|p| p.is_dir()) {
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

        // Record the exact last-viewed image + its parent directory so the app
        // can resume at this image on next launch (persisted on exit/open).
        if let Some(path) = self.files.get(index) {
            self.config.last_file_path = Some(path.clone());
            if let Some(parent) = path.parent() {
                self.config.last_directory = Some(parent.to_path_buf());
            }
        }

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
                    self.loader.request_full(index, path, self.files.len());
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
        self.animation = None;
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

    /// Install a decoded animation as the active image (replacing any static
    /// image) and reset the viewport for the animation's canvas size.
    fn set_animation(
        &mut self,
        frames: Vec<(Vec<u8>, [usize; 2], [usize; 2])>,
        delays_ms: Vec<u64>,
        kind: loader::AnimKind,
        ctx: &egui::Context,
    ) {
        self.animation = Some(AnimationPlayer::new(frames, delays_ms, kind, ctx));
        self.image = None;
        self.error = None;
        self.reset_viewport();
    }

    /// Speculatively preload full images adjacent to `index`, plus thumbnails.
    fn request_preload(&mut self, index: usize) {
        let len = self.files.len();
        if len == 0 {
            return;
        }
        for delta in [-1isize, 1] {
            let i = if self.config.loop_navigation {
                // Wrap around the ring so folder-edge neighbors are preloaded.
                (index as isize + delta).rem_euclid(len as isize) as usize
            } else {
                let i = index as isize + delta;
                if i >= 0 && i < len as isize {
                    i as usize
                } else {
                    continue;
                }
            };
            if !self.full_cache.contains_key(&i) && !self.pending_full.contains(&i) {
                if let Some(path) = self.files.get(i).cloned() {
                    self.pending_full.insert(i);
                    self.loader.request_full(i, path, len);
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
            if self.thumb_cache.contains_key(&i)
                || self.pending_thumb.contains(&i)
                || self.failed_thumb.contains(&i)
            {
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
            self.loader.request_full(index, path, self.files.len());
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
                loader::ResponseKind::FullAnimated { index, frames, delays_ms, kind } => {
                    self.pending_full.remove(&index);
                    if self.loading_index == Some(index) && index == self.current_index {
                        self.preview = None;
                        self.set_animation(frames, delays_ms, kind, ctx);
                        self.loading_index = None;
                    }
                }
                loader::ResponseKind::FullFailed { index, error } => {
                    self.pending_full.remove(&index);
                    if self.loading_index == Some(index) && index == self.current_index {
                        self.preview = None;
                        self.image = None;
                        self.animation = None;
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
                loader::ResponseKind::ThumbFailed { index } => {
                    self.pending_thumb.remove(&index);
                    self.failed_thumb.insert(index);
                }
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
        self.failed_thumb.clear();
        self.loading_index = None;
        self.preview = None;
        self.animation = None;
    }

    /// Evict cache entries far from the current index to bound memory.
    fn evict_caches(&mut self) {
        let cur = self.current_index;
        let len = self.files.len();
        self.full_cache.retain(|&i, _| loader::ring_dist(i, cur, len) <= 3);
        self.thumb_cache.retain(|&i, _| loader::ring_dist(i, cur, len) <= 5);
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
            self.animation = None;
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
            self.error = Some("No supported images found in folder".to_string());
            self.current_index = 0;
            self.config.last_file_path = None;
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
            if path.is_dir() {
                self.open_directory(path, ctx);
            } else {
                self.open_file(path, ctx);
            }
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
        self.show_success("File closed");
    }

    fn close_folder(&mut self) {
        self.context_menu_pos = None;
        self.files.clear();
        self.image = None;
        self.error = None;
        self.current_index = 0;
        self.reset_viewport();
        self.clear_caches();
        // Forget the saved paths so a restart boots into the clean empty state.
        self.config.last_file_path = None;
        self.config.last_directory = None;
        self.save_config();
        self.show_success("Folder closed");
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
            self.show_success("Copied path");
        }
    }

    fn copy_current_file(&mut self) {
        let Some(path) = self.current_file().cloned() else {
            return;
        };
        match platform::copy_file_to_clipboard(&path) {
            Ok(()) => self.show_success("File copied"),
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
                    Ok(()) => self.show_success("Image copied"),
                    Err(e) => self.show_status(format!("Copy image failed: {e}")),
                }
            }
            Err(e) => self.show_status(format!("Clipboard unavailable: {e}")),
        }
    }

    /// Open a native "Save As" dialog and write the currently displayed image
    /// (with any in-memory manual rotations already applied) to the chosen path.
    fn save_current_as(&mut self) {
        let Some(image) = &self.image else {
            self.show_status("No image to save");
            return;
        };
        let rgba = image.rgba_bytes();
        let [w, h] = image.display_dimensions();

        let mut dialog = rfd::FileDialog::new()
            .set_title("Save Image As")
            .add_filter("PNG", &["png"])
            .add_filter("JPEG", &["jpg", "jpeg"])
            .add_filter("BMP", &["bmp"])
            .add_filter("WebP", &["webp"]);
        if let Some(dir) = &self.config.last_directory {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.save_file() else {
            return;
        };
        // rfd may return a path without an extension; default to PNG.
        let path = if path.extension().is_some() {
            path
        } else {
            path.with_extension("png")
        };

        let result = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
            .ok_or_else(|| "invalid image buffer".to_string())
            .and_then(|buf| {
                image::DynamicImage::ImageRgba8(buf)
                    .save(&path)
                    .map_err(|e| e.to_string())
            });

        match result {
            Ok(()) => self.show_success("Saved image"),
            Err(e) => self.show_status(format!("Save failed: {e}")),
        }
    }

    /// Launch the native Windows Print Wizard for the active image.
    fn print_current(&mut self) {
        let Some(path) = self.current_file().cloned() else {
            self.show_status("No image to print");
            return;
        };
        match platform::print_image(&path) {
            Ok(()) => self.show_success("Sent to Print Wizard"),
            Err(e) => self.show_status(format!("Print failed: {e}")),
        }
    }

    /// Re-run the HKCU registration, refresh the health status, and toast.
    fn refresh_registration(&mut self) {
        match platform::update_registration() {
            Ok(()) => {
                self.reg_status = platform::check_registration_status();
                self.show_success("Windows registration updated to current path");
            }
            Err(e) => self.show_status(format!("Registration failed: {e}")),
        }
    }

    /// Toggle the Explorer folder context-menu registration and toast.
    fn toggle_folder_context_menu(&mut self) {
        let result = if self.folder_menu_registered {
            platform::remove_folder_context_menu()
        } else {
            platform::add_folder_context_menu()
        };
        match result {
            Ok(()) => {
                self.folder_menu_registered = platform::is_folder_context_menu_registered();
                if self.folder_menu_registered {
                    self.show_success("Added 'Open folder with Photonio' to Explorer");
                } else {
                    self.show_success("Removed folder context menu");
                }
            }
            Err(e) => self.show_status(format!("Context menu update failed: {e}")),
        }
    }

    /// Execute an action reported by the quick-action pill.
    fn execute_pill_action(&mut self, action: PillAction, ctx: &egui::Context) {
        match action {
            PillAction::OpenFile => self.open_file_dialog(ctx),
            PillAction::OpenFolder => self.open_folder_dialog(ctx),
            PillAction::SaveAs => self.save_current_as(),
            PillAction::Print => self.print_current(),
            PillAction::ToggleFilmstrip => {
                self.config.show_filmstrip = !self.config.show_filmstrip;
                self.save_config();
            }
        }
    }

    fn set_wallpaper_current(&mut self) {
        let Some(path) = self.current_file().cloned() else {
            return;
        };
        match platform::set_wallpaper(&path) {
            Ok(()) => self.show_success("Wallpaper set"),
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
                self.show_status(format!("{} Moved to Recycle Bin", icons::ICON_TRASH));
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
            toggle_play: i.key_pressed(egui::Key::Space),
            step_back: i.key_pressed(egui::Key::OpenBracket),
            step_forward: i.key_pressed(egui::Key::CloseBracket),
            save_as: i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(egui::Key::S),
            print: i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(egui::Key::P),
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

    /// Handle animation playback input: Space toggles play/pause; `[` / `]`
    /// step a frame (useful while paused).
    fn handle_animation_input(&mut self, input: &InputSnapshot) {
        let Some(anim) = &mut self.animation else {
            return;
        };
        if input.toggle_play {
            anim.paused = !anim.paused;
            if !anim.paused {
                // Resume: give the current frame its full delay before advancing.
                let delay = anim.delays_ms.get(anim.current_frame).copied().unwrap_or(100);
                anim.next_frame =
                    std::time::Instant::now() + std::time::Duration::from_millis(delay);
            }
        }
        let step = if input.step_back {
            -1isize
        } else if input.step_forward {
            1isize
        } else {
            0isize
        };
        if step != 0 {
            let len = anim.frames.len();
            if len > 0 {
                anim.current_frame =
                    (anim.current_frame as isize + step).rem_euclid(len as isize) as usize;
                let delay = anim.delays_ms.get(anim.current_frame).copied().unwrap_or(100);
                anim.next_frame =
                    std::time::Instant::now() + std::time::Duration::from_millis(delay);
            }
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
        // Refresh the registry health check for the integration footer.
        self.reg_status = platform::check_registration_status();
        self.folder_menu_registered = platform::is_folder_context_menu_registered();
    }

    /// True when a modal or the context menu is open — these must swallow
    /// mouse-wheel input so it never leaks to the background canvas.
    fn modal_open(&self) -> bool {
        self.show_settings || self.show_help || self.show_about || self.context_menu_pos.is_some()
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

    /// Show a success toast, prefixed with the check icon.
    fn show_success(&mut self, message: impl Into<String>) {
        self.show_status(format!("{} {}", icons::ICON_CHECK, message.into()));
    }

    fn take_status_text(&mut self) -> Option<String> {
        let (message, at) = self.feedback.as_ref()?;
        if at.elapsed() > std::time::Duration::from_secs(2) {
            self.feedback = None;
            return None;
        }
        Some(message.clone())
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
        } else if let Some(anim) = &self.animation {
            anim.current_texture()
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
        if (input.ctrl || wheel_zooms) && input.scroll != 0.0 && !self.modal_open() {
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
        let dark = self.is_dark_theme();
        let title_color = if dark {
            egui::Color32::WHITE
        } else {
            egui::Color32::from_gray(25)
        };
        let version_color = if dark {
            egui::Color32::from_gray(140)
        } else {
            egui::Color32::from_gray(100)
        };
        let hint_color = if dark {
            egui::Color32::from_rgb(205, 205, 214)
        } else {
            egui::Color32::from_rgb(60, 60, 66)
        };

        egui::Area::new(egui::Id::new("empty-state"))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .order(egui::Order::Middle)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    // 1. Logo (native aspect ratio, subtle watermark).
                    if let Some((texture, dims)) = &self.logo {
                        let max = 180.0;
                        let scale = (max / dims[0] as f32).min(max / dims[1] as f32).min(1.0);
                        let size = egui::vec2(dims[0] as f32 * scale, dims[1] as f32 * scale);
                        let tint = egui::Color32::from_white_alpha(90);
                        ui.add(egui::Image::new(texture).fit_to_exact_size(size).tint(tint));
                    }
                    ui.add_space(12.0);
                    // 2. App title.
                    ui.label(
                        egui::RichText::new("Photonio")
                            .size(24.0)
                            .strong()
                            .color(title_color),
                    );
                    // 3. Version tag.
                    ui.label(
                        egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                            .size(13.0)
                            .color(version_color),
                    );
                    ui.add_space(16.0);
                    // 4. Guidance hint.
                    ui.label(
                        egui::RichText::new("Drop an image or press Ctrl+O to open")
                            .size(14.0)
                            .color(hint_color),
                    );
                });
            });
    }

    /// True when the app is in a dark backdrop mode (Glass or Dark).
    fn is_dark_theme(&self) -> bool {
        self.config.canvas_background != config::CanvasBackground::Light
    }

    /// Theme-derived glass colors for the current backdrop + opacity.
    fn theme(&self) -> theme::ThemeColors {
        theme::ThemeColors::new(self.is_dark_theme(), self.config.glass_opacity)
    }

    fn draw_hud(&mut self, ctx: &egui::Context) {
        if self.files.is_empty() {
            return;
        }
        // Hidden mode: no HUD bar (navigation via hotkeys/wheel only).
        if self.config.hud_position == config::HudPosition::Hidden {
            return;
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
        let gif_cur = self.animation.as_ref().map(|a| a.current_frame + 1);
        let gif_total = self.animation.as_ref().map(|a| a.frames.len());
        let gif_badge = self.animation.as_ref().map(|a| a.kind.badge());
        let colors = self.theme();
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
                colors.panel(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new(icons::ICON_PREV).size(14.0))
                                    .frame(false)
                                    .min_size(egui::vec2(24.0, 20.0)),
                            )
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

                        if let (Some(cur), Some(total)) = (gif_cur, gif_total) {
                            let badge = gif_badge.unwrap_or("[GIF]");
                            ui.label(egui::RichText::new(badge).color(GIF_COLOR).monospace());
                            ui.label(
                                egui::RichText::new(format!("Frame {cur} / {total}")).monospace(),
                            );
                        }

                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new(icons::ICON_NEXT).size(14.0))
                                    .frame(false)
                                    .min_size(egui::vec2(24.0, 20.0)),
                            )
                            .clicked()
                        {
                            clicked_next = true;
                        }
                    });
                });
            });

        if clicked_prev {
            self.step(-1, ctx);
        } else if clicked_next {
            self.step(1, ctx);
        }
    }

    /// Floating glass quick-action pill docked at the top-right of the canvas.
    fn draw_quick_pill(&mut self, ctx: &egui::Context) -> Option<PillAction> {
        if !self.config.quick_pill_enabled {
            return None;
        }
        let colors = self.theme();
        let vertical = self.config.quick_pill_orientation == config::Orientation::Vertical;
        let mut action = None;

        egui::Area::new(egui::Id::new("quick-action-pill"))
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 12.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                colors.panel(ui, |ui| {
                    if vertical {
                        ui.vertical(|ui| {
                            if let Some(a) = self.pill_buttons(ui) {
                                action = Some(a);
                            }
                        });
                    } else {
                        ui.horizontal(|ui| {
                            if let Some(a) = self.pill_buttons(ui) {
                                action = Some(a);
                            }
                        });
                    }
                });
            });

        action
    }

    /// Render the pill's buttons; returns the clicked action, if any.
    fn pill_buttons(&self, ui: &mut egui::Ui) -> Option<PillAction> {
        if self.config.pill_open_file {
            if pill_button(ui, icons::ICON_OPEN_FILE, "Open File (Ctrl+O)").clicked() {
                return Some(PillAction::OpenFile);
            }
        }
        if self.config.pill_open_folder {
            if pill_button(ui, icons::ICON_OPEN_FOLDER, "Open Folder (Ctrl+Shift+O)").clicked() {
                return Some(PillAction::OpenFolder);
            }
        }
        if self.config.pill_save_as {
            if pill_button(ui, icons::ICON_SAVE, "Save As (Ctrl+S)").clicked() {
                return Some(PillAction::SaveAs);
            }
        }
        if self.config.pill_print {
            if pill_button(ui, icons::ICON_PRINT, "Print (Ctrl+P)").clicked() {
                return Some(PillAction::Print);
            }
        }
        if self.config.pill_share {
            // Placeholder — disabled until sharing is implemented.
            ui.add_enabled(
                false,
                egui::Button::new(egui::RichText::new(icons::ICON_SHARE).size(16.0))
                    .frame(false)
                    .min_size(egui::vec2(28.0, 24.0)),
            )
            .on_hover_text("Share (coming soon)");
        }
        if self.config.pill_filmstrip {
            if pill_button(ui, icons::ICON_FILMSTRIP, "Toggle Filmstrip (T)").clicked() {
                return Some(PillAction::ToggleFilmstrip);
            }
        }
        None
    }

    /// Draw one filmstrip tile at the current cursor position; returns the
    /// index if the tile was clicked.
    fn filmstrip_tile(&self, ui: &mut egui::Ui, idx: usize) -> Option<usize> {
        let colors = self.theme();
        let size = 72.0;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
        let painter = ui.painter();

        // Uniform rounded tile background behind every state.
        painter.rect_filled(rect, 6.0, colors.placeholder);

        match self.thumb_cache.get(&idx) {
            Some(thumb) => {
                // Fit the thumbnail inside the slot, preserving aspect ratio.
                let img_rect = fit_rect_contain(thumb.dims, rect);
                painter.image(
                    thumb.texture.id(),
                    img_rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            None if self.failed_thumb.contains(&idx) => {
                // Definitively failed to decode: subtle placeholder mark.
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "✕",
                    egui::FontId::proportional(18.0),
                    colors.muted,
                );
            }
            None => {
                // Still decoding — background only.
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
            painter.rect_stroke(rect, 6.0, egui::Stroke::new(1.0_f32, colors.muted));
        }

        if response.clicked() {
            Some(idx)
        } else {
            None
        }
    }

    fn draw_filmstrip(&mut self, ctx: &egui::Context) {
        if !self.config.show_filmstrip || self.files.is_empty() {
            return;
        }
        let colors = self.theme();
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

        // Dock position determines the anchor, offset, and layout direction.
        let (anchor, offset, vertical) = match self.config.filmstrip_position {
            config::FilmstripPosition::Bottom => {
                (egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -64.0), false)
            }
            config::FilmstripPosition::Top => {
                (egui::Align2::CENTER_TOP, egui::vec2(0.0, 64.0), false)
            }
            config::FilmstripPosition::Left => {
                (egui::Align2::LEFT_CENTER, egui::vec2(64.0, 0.0), true)
            }
            config::FilmstripPosition::Right => {
                (egui::Align2::RIGHT_CENTER, egui::vec2(-64.0, 0.0), true)
            }
        };

        egui::Area::new(egui::Id::new("filmstrip"))
            .anchor(anchor, offset)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                colors.panel(ui, |ui| {
                    if vertical {
                        ui.vertical(|ui| {
                            for idx in indices {
                                if self.filmstrip_tile(ui, idx) == Some(idx) {
                                    clicked = Some(idx);
                                }
                            }
                        });
                    } else {
                        ui.horizontal(|ui| {
                            for idx in indices {
                                if self.filmstrip_tile(ui, idx) == Some(idx) {
                                    clicked = Some(idx);
                                }
                            }
                        });
                    }
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
        let dims = self
            .image
            .as_ref()
            .map(|im| im.dimensions)
            .or_else(|| self.animation.as_ref().map(|a| a.dimensions))
            .unwrap_or([0, 0]);

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
        let colors = self.theme();

        let response = egui::Area::new(egui::Id::new("context-menu"))
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                colors.panel(ui, |ui| {
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
        let colors = self.theme();

        let response = egui::Area::new(egui::Id::new("help-overlay"))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                colors.panel(ui, |ui| {
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
                        ("Toggle filmstrip", "T"),
                        ("Play / Pause animation", "Space"),
                        ("Copy path", "Ctrl + C"),
                        ("Save image as…", "Ctrl + S"),
                        ("Print…", "Ctrl + P"),
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
        let mut register_clicked = false;
        let mut folder_menu_clicked = false;
        let colors = self.theme();
        let reg_status = self.reg_status.clone();
        let folder_menu_registered = self.folder_menu_registered;

        egui::Area::new(egui::Id::new("settings-modal"))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                colors.panel(ui, |ui| {
                    ui.set_min_width(480.0);
                    ui.heading("Settings");
                    ui.separator();

                    egui::ScrollArea::vertical()
                        .max_height(640.0)
                        .show(ui, |ui| {
                            // General
                            ui.label(egui::RichText::new("General").strong());
                            changed |= ui
                                .checkbox(
                                    &mut self.config.remember_last_folder,
                                    "Remember last opened image and folder on startup",
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

                            // Filmstrip
                            ui.separator();
                            ui.label(egui::RichText::new("Filmstrip").strong());
                            changed |= ui
                                .checkbox(
                                    &mut self.config.show_filmstrip,
                                    "Show Thumbnail Filmstrip (T)",
                                )
                                .changed();
                            ui.indent("filmstrip_dock", |ui| {
                                ui.add_enabled_ui(self.config.show_filmstrip, |ui| {
                                    ui.label("Filmstrip dock position:");
                                    changed |= ui
                                        .radio_value(
                                            &mut self.config.filmstrip_position,
                                            config::FilmstripPosition::Bottom,
                                            "Bottom",
                                        )
                                        .changed();
                                    changed |= ui
                                        .radio_value(
                                            &mut self.config.filmstrip_position,
                                            config::FilmstripPosition::Top,
                                            "Top",
                                        )
                                        .changed();
                                    changed |= ui
                                        .radio_value(
                                            &mut self.config.filmstrip_position,
                                            config::FilmstripPosition::Left,
                                            "Left",
                                        )
                                        .changed();
                                    changed |= ui
                                        .radio_value(
                                            &mut self.config.filmstrip_position,
                                            config::FilmstripPosition::Right,
                                            "Right",
                                        )
                                        .changed();
                                });
                            });

                            // Quick-Action Pill
                            ui.separator();
                            ui.label(egui::RichText::new("Quick-Action Pill").strong());
                            changed |= ui
                                .checkbox(
                                    &mut self.config.quick_pill_enabled,
                                    "Enable Quick-Action Pill",
                                )
                                .changed();
                            ui.indent("quick_pill_buttons", |ui| {
                                ui.add_enabled_ui(self.config.quick_pill_enabled, |ui| {
                                    changed |= ui.checkbox(&mut self.config.pill_open_file, "Open File").changed();
                                    changed |= ui.checkbox(&mut self.config.pill_open_folder, "Open Folder").changed();
                                    changed |= ui.checkbox(&mut self.config.pill_save_as, "Save As…").changed();
                                    changed |= ui.checkbox(&mut self.config.pill_print, "Print…").changed();
                                    changed |= ui.checkbox(&mut self.config.pill_share, "Share…").changed();
                                    changed |= ui.checkbox(&mut self.config.pill_filmstrip, "Toggle Filmstrip").changed();
                                    ui.add_space(4.0);
                                    ui.label("Layout:");
                                    changed |= ui
                                        .radio_value(
                                            &mut self.config.quick_pill_orientation,
                                            config::Orientation::Horizontal,
                                            "Horizontal",
                                        )
                                        .changed();
                                    changed |= ui
                                        .radio_value(
                                            &mut self.config.quick_pill_orientation,
                                            config::Orientation::Vertical,
                                            "Vertical",
                                        )
                                        .changed();
                                });
                            });

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

                        });

                    ui.separator();

                    // System Integration footer.
                    match &reg_status {
                        platform::RegistryStatus::Valid { .. } => {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} Windows Integration: Active",
                                        icons::ICON_CHECK
                                    ))
                                    .color(SUCCESS_COLOR),
                                );
                                if ui.button("Re-register").clicked() {
                                    register_clicked = true;
                                }
                            });
                        }
                        platform::RegistryStatus::Mismatch { registered_path, current_path } => {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} Path Mismatch Detected",
                                    icons::ICON_WARN
                                ))
                                .color(WARN_COLOR)
                                .strong(),
                            );
                            ui.label(format!("Registered to: {}", registered_path.display()));
                            ui.label(format!("Current path: {}", current_path.display()));
                            if ui.button("Update Registry to Current Path").clicked() {
                                register_clicked = true;
                            }
                        }
                        platform::RegistryStatus::NotRegistered => {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} Windows Integration: Not Registered",
                                        icons::ICON_CIRCLE
                                    ))
                                    .color(colors.muted),
                                );
                                if ui.button("Register as Default App Handler").clicked() {
                                    register_clicked = true;
                                }
                            });
                        }
                    }

                    ui.separator();
                    ui.label(egui::RichText::new("Explorer Context Menu").strong());
                    if folder_menu_registered {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} Folder context menu active",
                                    icons::ICON_CHECK
                                ))
                                .color(SUCCESS_COLOR),
                            );
                            if ui.button("Remove Folder Context Menu").clicked() {
                                folder_menu_clicked = true;
                            }
                        });
                    } else {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} Not registered in folder context menu",
                                    icons::ICON_CIRCLE
                                ))
                                .color(colors.muted),
                            );
                            if ui.button("Add 'Open folder with Photonio' to Explorer").clicked() {
                                folder_menu_clicked = true;
                            }
                        });
                    }

                    ui.separator();
                    if colors.ghost_button(ui, "Close").clicked() {
                        self.show_settings = false;
                    }
                });
            });

        if changed {
            self.save_config();
        }
        if register_clicked {
            self.refresh_registration();
        }
        if folder_menu_clicked {
            self.toggle_folder_context_menu();
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

/// A compact, flat glyph button for the quick-action pill.
fn pill_button(ui: &mut egui::Ui, glyph: &str, tooltip: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(glyph).size(16.0))
            .frame(false)
            .min_size(egui::vec2(28.0, 24.0)),
    )
    .on_hover_text(tooltip)
}

/// Fit an image of `dims` (width, height) inside `slot`, preserving its aspect
/// ratio and centering it. Returns the draw rect for the fitted image.
fn fit_rect_contain(dims: [usize; 2], slot: egui::Rect) -> egui::Rect {
    let img_w = dims[0].max(1) as f32;
    let img_h = dims[1].max(1) as f32;
    let scale = (slot.width() / img_w).min(slot.height() / img_h);
    let draw_w = img_w * scale;
    let draw_h = img_h * scale;
    let min = egui::pos2(
        slot.center().x - draw_w * 0.5,
        slot.center().y - draw_h * 0.5,
    );
    egui::Rect::from_min_size(min, egui::vec2(draw_w, draw_h))
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
        // Keep global widget visuals in sync with the backdrop theme.
        if ctx.style().visuals.dark_mode != self.is_dark_theme() {
            apply_global_visuals(ctx, self.is_dark_theme());
        }

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
        if wheel_navigates && !input.ctrl && input.scroll != 0.0 && !self.modal_open() {
            if input.scroll > 0.0 {
                navigate -= 1;
            } else {
                navigate += 1;
            }
        }

        self.apply_hotkeys(ctx, &input);
        self.handle_animation_input(&input);

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

        self.draw_hud(ctx);

        if let Some(action) = self.draw_quick_pill(ctx) {
            self.execute_pill_action(action, ctx);
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

        if input.open_file {
            self.open_file_dialog(ctx);
        }
        if input.open_folder {
            self.open_folder_dialog(ctx);
        }
        if input.copy_path {
            self.copy_current_path(ctx);
        }
        if input.reveal {
            self.reveal_current();
        }
        if input.save_as {
            self.save_current_as();
        }
        if input.print {
            self.print_current();
        }

        if close_requested {
            self.save_config();
        }

        // About modal — rendered last, directly on ctx, outside every panel.
        let about_dark = self.is_dark_theme();
        about::AboutDialog::show(
            ctx,
            &mut self.show_about,
            self.logo.as_ref().map(|(texture, _)| texture),
            self.config.glass_opacity,
            about_dark,
        );

        // Animated GIF playback: advance frames on schedule and keep the
        // animation repainting without wasting CPU between frames.
        if let Some(anim) = &mut self.animation {
            if !anim.paused && anim.frames.len() > 1 {
                let now = std::time::Instant::now();
                if now >= anim.next_frame {
                    anim.current_frame = (anim.current_frame + 1) % anim.frames.len();
                    let delay = anim.delays_ms.get(anim.current_frame).copied().unwrap_or(100);
                    anim.next_frame = now + std::time::Duration::from_millis(delay);
                }
                ctx.request_repaint_after(anim.next_frame.saturating_duration_since(now));
            }
        }

        // Safety net: while any decode is in flight, keep scheduling repaints
        // so a dropped worker wakeup can't leave the UI stuck on a placeholder.
        if !self.pending_full.is_empty() || !self.pending_thumb.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
}
