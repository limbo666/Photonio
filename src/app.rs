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
/// Gold accent stroke for the active selection frame (grid + filmstrip).
const GOLD_COLOR: egui::Color32 = egui::Color32::from_rgb(255, 200, 50);

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
    up: bool,
    down: bool,
    home: bool,
    end: bool,
    enter: bool,
    reset_fit: bool,
    reset_100: bool,
    rotate: bool,
    open_file: bool,
    open_folder: bool,
    copy_path: bool,
    copy_image: bool,
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
    toggle_image_info: bool,
    resize_image: bool,
}

/// An action selected from the quick-action pill.
enum PillAction {
    OpenFile,
    OpenFolder,
    SaveAs,
    Print,
    ToggleFilmstrip,
}

/// Which pane of the two-pane Settings modal is active.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    Appearance,
    Filmstrip,
    QuickPills,
    Behavior,
    Integration,
}

/// Which full-window view is currently active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    Viewer,
    Grid,
}

/// An interaction reported by a grid thumbnail tile.
enum GridTileAction {
    Select(usize),
    Open(usize),
    ContextMenu(usize, egui::Pos2),
}

/// A folder-browser operation awaiting the user's "Select Folder" confirmation.
#[derive(Clone, Copy)]
enum PendingOperation {
    OpenFolder,
    CopyFileTo,
    MoveFileTo,
}

/// An action selected from the context menu.
enum MenuAction {
    OpenFile,
    OpenFolder,
    CloseImage,
    CloseFolder,
    ResizeImage,
    SaveRotation,
    OpenContainingFolder,
    OpenWith,
    CopyFileTo,
    MoveFileTo,
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
    /// On-screen rect of the last drawn image (for double-click routing).
    image_rect: Option<egui::Rect>,
    /// Active full-window view (Viewer or fullscreen grid selector).
    pub view_mode: ViewMode,
    /// Number of grid columns, computed each frame in `draw_grid_view`.
    grid_columns: usize,
    /// Set when keyboard navigation / grid open should scroll the selected
    /// card into view (consumed by `draw_grid_view`).
    scroll_to_selected: bool,
    /// Cumulative in-memory manual rotation (0/90/180/270), applied by `R`.
    rotation: u32,
    /// Resize modal state: target width/height, aspect lock, and confirmation.
    resize_width: u32,
    resize_height: u32,
    resize_aspect: bool,
    resize_confirm_overwrite: bool,
    /// True when the target is a fixed-dimension preset → cover & center-crop.
    resize_cover_crop: bool,
    /// Original (pre-resize) dimensions captured when the dialog opened.
    resize_orig: [usize; 2],
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
    show_image_info: bool,
    show_resize_dialog: bool,
    /// The custom folder browser is open.
    show_folder_browser: bool,
    /// What the folder browser should do when "Select Folder" is clicked.
    pending_operation: Option<PendingOperation>,
    /// Folder browser state (present while the browser is open).
    folder_browser: Option<viewer::folder_browser::Browser>,
    /// Set when help just opened, to ignore the same-frame click.
    help_opened_this_frame: bool,
    /// Transient feedback badge: (message, when shown).
    feedback: Option<(String, std::time::Instant)>,
    /// Windows registration health. `None` until the async inspector has run;
    /// refreshed (non-blocking) when Settings opens.
    registry_status: Option<platform::RegistryStatus>,
    /// Whether the Explorer folder context menu is registered.
    folder_menu_registered: bool,
    /// True while the background registry inspector thread is running.
    checking_registry: bool,
    /// Receiver for the async registry inspector result.
    registry_rx: Option<std::sync::mpsc::Receiver<(platform::RegistryStatus, bool)>>,
    /// When the user last interacted (pointer/keyboard), for overlay auto-fade.
    last_interaction_instant: std::time::Instant,
    /// The active tab in the two-pane Settings modal (persisted across frames).
    settings_tab: SettingsTab,
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
            image_rect: None,
            view_mode: ViewMode::Viewer,
            grid_columns: 1,
            scroll_to_selected: false,
            rotation: 0,
            resize_width: 0,
            resize_height: 0,
            resize_aspect: true,
            resize_confirm_overwrite: false,
            resize_cover_crop: false,
            resize_orig: [0, 0],
            config,
            window_rect: None,
            context_menu_pos: None,
            show_help: false,
            show_about: false,
            show_settings: false,
            show_image_info: false,
            show_resize_dialog: false,
            show_folder_browser: false,
            pending_operation: None,
            folder_browser: None,
            help_opened_this_frame: false,
            feedback: None,
            registry_status: None,
            folder_menu_registered: false,
            checking_registry: false,
            registry_rx: None,
            last_interaction_instant: std::time::Instant::now(),
            settings_tab: SettingsTab::Appearance,
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
        self.rotation = 0;
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
        self.rotation = 0;
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
        self.rotation = 0;
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

    /// Best-effort thumbnail request for one index, deduped against the cache
    /// and in-flight set. Dropped requests are retried on later frames.
    fn request_thumb_for(&mut self, index: usize) {
        if self.thumb_cache.contains_key(&index)
            || self.pending_thumb.contains(&index)
            || self.failed_thumb.contains(&index)
        {
            return;
        }
        if let Some(path) = self.files.get(index).cloned() {
            if self.loader.request_thumb(index, path) {
                self.pending_thumb.insert(index);
            }
        }
    }

    /// Request low-res thumbnails for the visible filmstrip window (circular,
    /// matching `draw_filmstrip`). Idempotent and retried every frame.
    fn request_thumbs(&mut self, index: usize) {
        if !self.config.show_filmstrip {
            return;
        }
        let len = self.files.len();
        let (count, half) = self.filmstrip_window();
        let window: Vec<usize> = if len >= count {
            (-half..=half)
                .map(|off| (index as isize + off).rem_euclid(len as isize) as usize)
                .collect()
        } else {
            (0..len).collect()
        };
        for i in window {
            self.request_thumb_for(i);
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
        let (count, _) = self.filmstrip_window();
        self.full_cache.retain(|&i, _| loader::ring_dist(i, cur, len) <= 3);
        self.thumb_cache.retain(|&i, _| loader::ring_dist(i, cur, len) <= count + 2);
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

    /// Select a grid tile (clamped); does not decode the full image.
    /// Returns `true` if the selection actually changed.
    fn grid_select(&mut self, index: usize) -> bool {
        if self.files.is_empty() {
            return false;
        }
        let index = index.min(self.files.len() - 1);
        if index != self.current_index {
            self.current_index = index;
            true
        } else {
            false
        }
    }

    /// Leave the grid selector and display the currently highlighted image.
    fn leave_grid(&mut self, ctx: &egui::Context) {
        self.view_mode = ViewMode::Viewer;
        if !self.files.is_empty() {
            self.navigate_to(self.current_index, ctx);
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

    fn open_folder_dialog(&mut self) {
        self.open_folder_browser(PendingOperation::OpenFolder);
    }

    /// Open the custom folder browser for a given operation.
    fn open_folder_browser(&mut self, op: PendingOperation) {
        let remembered = match op {
            PendingOperation::OpenFolder => self.config.last_open_folder.clone(),
            PendingOperation::CopyFileTo => self.config.last_copy_folder.clone(),
            PendingOperation::MoveFileTo => self.config.last_move_folder.clone(),
        };
        let start = viewer::folder_browser::resolve_start(remembered);
        self.pending_operation = Some(op);
        self.folder_browser = Some(viewer::folder_browser::Browser::new(start, &self.config));
        self.show_folder_browser = true;
        self.show_settings = false;
        self.show_help = false;
        self.show_about = false;
        self.show_image_info = false;
        self.show_resize_dialog = false;
        self.context_menu_pos = None;
    }

    /// Poll + render the folder browser, and dispatch its result.
    fn render_folder_browser(&mut self, ctx: &egui::Context) {
        if !self.show_folder_browser {
            return;
        }
        let colors = self.theme();

        let Some(browser) = self.folder_browser.as_mut() else {
            self.show_folder_browser = false;
            return;
        };

        browser.poll(ctx);
        let result = browser.render(ctx, &colors, &mut self.show_folder_browser);
        let tile = browser.tile_size;
        let filter = browser.filter;
        let show_files = browser.show_files;
        // (browser borrow of `self.folder_browser` ends here)

        // Persist view settings (in memory; written to disk only on change).
        let changed = self.config.folder_tile_size != tile
            || self.config.folder_content_filter != filter
            || self.config.folder_show_files != show_files;
        self.config.folder_tile_size = tile;
        self.config.folder_content_filter = filter;
        self.config.folder_show_files = show_files;
        if changed {
            self.save_config();
        }

        // The window's close (X) button flips the flag directly; clean up now.
        if !self.show_folder_browser {
            self.folder_browser = None;
            self.pending_operation = None;
            return;
        }

        match result {
            viewer::folder_browser::BrowserResult::None => {}
            viewer::folder_browser::BrowserResult::Cancel => {
                self.show_folder_browser = false;
                self.folder_browser = None;
                self.pending_operation = None;
            }
            viewer::folder_browser::BrowserResult::Select(path) => {
                self.show_folder_browser = false;
                self.folder_browser = None;
                match self.pending_operation.take() {
                    Some(PendingOperation::OpenFolder) => {
                        self.config.last_open_folder = Some(path.clone());
                        self.save_config();
                        self.open_directory(path, ctx);
                    }
                    Some(PendingOperation::CopyFileTo) => {
                        self.config.last_copy_folder = Some(path.clone());
                        self.save_config();
                        if let Some(src) = self.current_file().cloned() {
                            self.do_copy(src, path);
                        }
                    }
                    Some(PendingOperation::MoveFileTo) => {
                        self.config.last_move_folder = Some(path.clone());
                        self.save_config();
                        if let Some(src) = self.current_file().cloned() {
                            self.do_move(src, path, ctx);
                        }
                    }
                    None => {}
                }
            }
        }
    }

    fn copy_current_path(&mut self, ctx: &egui::Context) {
        if let Some(path) = self.current_file() {
            ctx.copy_text(path.display().to_string());
            self.show_success("Copied path to clipboard");
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
        // Copy the display-res RGBA pixels (borrowed, not cloned) to the
        // clipboard. A closure keeps the borrow of `self.image` scoped so we
        // can show the toast afterwards.
        let result = (|| -> Result<(), String> {
            let image = self.image.as_ref().ok_or("No image".to_string())?;
            let [width, height] = image.display_dimensions();
            let rgba = image.rgba_slice();
            let mut clipboard =
                arboard::Clipboard::new().map_err(|e| format!("Clipboard unavailable: {e}"))?;
            clipboard
                .set_image(arboard::ImageData {
                    width,
                    height,
                    bytes: std::borrow::Cow::Borrowed(rgba),
                })
                .map_err(|e| format!("Copy image failed: {e}"))
        })();

        match result {
            Ok(()) => self.show_success("Copied image to clipboard"),
            Err(e) => self.show_status(e),
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
                self.registry_status = Some(platform::check_registration_status());
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
            PillAction::OpenFolder => self.open_folder_dialog(),
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

    /// Open the resize dialog, seeded from the current image's dimensions.
    fn open_resize_dialog(&mut self) {
        if self.view_mode != ViewMode::Viewer {
            return; // resize applies only to the decoded Viewer image
        }
        let Some(dims) = self.image.as_ref().map(|im| im.dimensions) else {
            return; // resize applies only to static images
        };
        self.resize_orig = dims;
        self.resize_width = dims[0] as u32;
        self.resize_height = dims[1] as u32;
        self.resize_aspect = true;
        self.resize_confirm_overwrite = false;
        self.resize_cover_crop = false;
        self.show_resize_dialog = true;
        self.show_settings = false;
        self.show_help = false;
        self.show_about = false;
        self.show_image_info = false;
        self.context_menu_pos = None;
    }

    /// Decode, rotate, resize, and save the current image; then reload it.
    fn resize_save(&mut self, overwrite: bool, ctx: &egui::Context) {
        let Some(path) = self.current_file().cloned() else {
            return;
        };

        // Resolve the destination path first (so "Cancel" costs no decode).
        let target = if overwrite {
            path.clone()
        } else {
            let default_name = default_resized_name(&path);
            let mut dialog = rfd::FileDialog::new()
                .set_title("Save Resized Image As")
                .set_file_name(&default_name);
            if let Some(dir) = path.parent() {
                dialog = dialog.set_directory(dir);
            }
            match dialog.save_file() {
                Some(t) => t,
                None => return, // user canceled
            }
        };

        let result = (|| -> Result<(), String> {
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            let img = viewer::decode(&bytes)?;
            let img = viewer::resize::rotate(&img, self.rotation);
            let resized = if self.resize_cover_crop {
                viewer::resize::cover_crop(&img, self.resize_width, self.resize_height)
            } else {
                viewer::resize::resize_exact(&img, self.resize_width, self.resize_height)
            };
            resized.save(&target).map_err(|e| e.to_string())
        })();

        match result {
            Ok(()) => {
                self.show_success("Image resized and saved");
                self.show_resize_dialog = false;
                self.open_file(target, ctx);
            }
            Err(e) => self.show_status(format!("Resize failed: {e}")),
        }
    }

    /// Save the in-memory manual rotation back to disk (overwrite or as copy).
    fn save_rotation(&mut self, ctx: &egui::Context) {
        if self.rotation == 0 {
            return;
        }
        let Some(path) = self.current_file().cloned() else {
            return;
        };

        let default_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "image".to_string());
        let mut dialog = rfd::FileDialog::new()
            .set_title("Save Rotated Image")
            .set_file_name(&default_name);
        if let Some(dir) = path.parent() {
            dialog = dialog.set_directory(dir);
        }
        let Some(target) = dialog.save_file() else {
            return; // user canceled
        };

        let result = (|| -> Result<(), String> {
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            let img = viewer::decode(&bytes)?;
            let rotated = viewer::resize::rotate(&img, self.rotation);
            rotated.save(&target).map_err(|e| e.to_string())
        })();

        match result {
            Ok(()) => {
                self.rotation = 0;
                self.show_success("Rotation saved to file");
                self.open_file(target, ctx);
            }
            Err(e) => self.show_status(format!("Save rotation failed: {e}")),
        }
    }

    /// "Copy File to...": copy to the remembered folder, or browse on Ctrl.
    fn copy_file_to(&mut self, ctx: &egui::Context) {
        if self.current_file().is_none() {
            return;
        }
        let ctrl = ctx.input(|i| i.modifiers.ctrl);
        if ctrl || self.config.last_copy_folder.is_none() {
            self.open_folder_browser(PendingOperation::CopyFileTo);
        } else if let Some(dir) = self.config.last_copy_folder.clone() {
            if let Some(src) = self.current_file().cloned() {
                self.do_copy(src, dir);
            }
        }
    }

    fn do_copy(&mut self, src: PathBuf, dir: PathBuf) {
        let Some(name) = src.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            return;
        };
        let dest = unique_dest(&dir, &name);
        match std::fs::copy(&src, &dest) {
            Ok(_) => self.show_success(format!("Copied to {}", dir.display())),
            Err(e) => self.show_status(format!("Copy failed: {e}")),
        }
    }

    /// "Move File to...": move to the remembered folder, or browse on Ctrl.
    fn move_file_to(&mut self, ctx: &egui::Context) {
        if self.current_file().is_none() {
            return;
        }
        let ctrl = ctx.input(|i| i.modifiers.ctrl);
        if ctrl || self.config.last_move_folder.is_none() {
            self.open_folder_browser(PendingOperation::MoveFileTo);
        } else if let Some(dir) = self.config.last_move_folder.clone() {
            if let Some(src) = self.current_file().cloned() {
                self.do_move(src, dir, ctx);
            }
        }
    }

    fn do_move(&mut self, src: PathBuf, dir: PathBuf, ctx: &egui::Context) {
        let Some(name) = src.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            return;
        };
        let dest = unique_dest(&dir, &name);
        match move_file_fs(&src, &dest) {
            Ok(()) => {
                self.show_success(format!("Moved to {}", dir.display()));
                self.files.remove(self.current_index);
                self.advance_after_removal(ctx);
            }
            Err(e) => self.show_status(format!("Move failed: {e}")),
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
        self.rotation = (self.rotation + 90) % 360;
        if let Some(image) = &mut self.image {
            image.rotate_90_cw(ctx);
        }
    }

    // ------------------------------------------------------------------
    // Input
    // ------------------------------------------------------------------

    fn capture_input(ctx: &egui::Context) -> InputSnapshot {
        // egui converts Ctrl+C / Ctrl+Shift+C into `Event::Copy` (it does not
        // emit a raw `Key::C` event), so detect the copy event and disambiguate
        // via the Shift modifier.
        let copy_event = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)));
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
            up: i.key_pressed(egui::Key::ArrowUp) || i.key_pressed(egui::Key::W),
            down: i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::S),
            home: i.key_pressed(egui::Key::Home),
            end: i.key_pressed(egui::Key::End),
            enter: i.key_pressed(egui::Key::Enter),
            reset_fit: i.key_pressed(egui::Key::Num0),
            reset_100: i.key_pressed(egui::Key::Num1),
            rotate: i.key_pressed(egui::Key::R) && !(i.modifiers.ctrl && i.modifiers.shift),
            open_file: i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(egui::Key::O),
            open_folder: i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::O),
            copy_path: copy_event && !i.modifiers.shift,
            copy_image: copy_event && i.modifiers.shift,
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
            toggle_image_info: i.key_pressed(egui::Key::I),
            resize_image: i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::R),
        })
    }

    fn apply_hotkeys(&mut self, ctx: &egui::Context, input: &InputSnapshot) {
        // Viewport hotkeys only apply in Viewer mode, and not while a modal or
        // the context menu is open.
        if self.modal_open() || self.view_mode != ViewMode::Viewer {
            return;
        }
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
        if input.double_clicked && self.view_mode == ViewMode::Viewer {
            let over_image = self
                .image_rect
                .map_or(false, |r| input.pointer_pos.map_or(false, |p| r.contains(p)));
            if over_image {
                self.zoom = if self.zoom.is_none() { Some(1.0) } else { None };
                self.pan = egui::Vec2::ZERO;
            } else if !self.files.is_empty() {
                // Double-clicking the empty canvas opens the grid selector.
                self.view_mode = ViewMode::Grid;
                self.scroll_to_selected = true;
            }
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
        self.show_image_info = false;
        self.show_resize_dialog = false;
        self.close_folder_browser();
        self.context_menu_pos = None;
        self.help_opened_this_frame = true;
    }

    fn open_about(&mut self) {
        self.show_about = true;
        self.show_help = false;
        self.show_settings = false;
        self.show_image_info = false;
        self.show_resize_dialog = false;
        self.close_folder_browser();
        self.context_menu_pos = None;
    }

    fn open_settings(&mut self) {
        self.show_settings = true;
        self.show_help = false;
        self.show_about = false;
        self.show_image_info = false;
        self.show_resize_dialog = false;
        self.close_folder_browser();
        self.context_menu_pos = None;
        // Kick off a non-blocking registry health check for the Integration tab.
        self.start_registry_check();
    }

    /// Close the custom folder browser without running its pending operation.
    fn close_folder_browser(&mut self) {
        self.show_folder_browser = false;
        self.folder_browser = None;
        self.pending_operation = None;
    }

    /// Start the async registry inspector if it hasn't run yet and isn't
    /// already running. The result is delivered back through `registry_rx`.
    fn start_registry_check(&mut self) {
        if self.registry_status.is_some() || self.checking_registry {
            return;
        }
        self.checking_registry = true;
        let (tx, rx) = std::sync::mpsc::channel();
        self.registry_rx = Some(rx);
        std::thread::spawn(move || {
            let status = platform::check_registration_status();
            let folder = platform::is_folder_context_menu_registered();
            let _ = tx.send((status, folder));
        });
    }

    /// Drain a completed registry inspection and cache the result.
    fn poll_registry(&mut self) {
        let Some(rx) = &self.registry_rx else {
            return;
        };
        match rx.try_recv() {
            Ok((status, folder)) => {
                self.registry_status = Some(status);
                self.folder_menu_registered = folder;
                self.checking_registry = false;
                self.registry_rx = None;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // Inspector thread died unexpectedly; allow a future retry.
                self.checking_registry = false;
                self.registry_rx = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }

    /// Remove every registry entry Photonio wrote, then reflect the clean state.
    fn unregister_all(&mut self) {
        match platform::clean_all_registry_entries() {
            Ok(()) => {
                self.registry_status = Some(platform::RegistryStatus::NotRegistered);
                self.folder_menu_registered = false;
                self.show_success("Removed all Photonio registry entries");
            }
            Err(e) => self.show_status(format!("Cleanup failed: {e}")),
        }
    }

    /// True when a modal or the context menu is open — these must swallow
    /// mouse-wheel input so it never leaks to the background canvas.
    fn modal_open(&self) -> bool {
        self.show_settings
            || self.show_help
            || self.show_about
            || self.show_image_info
            || self.show_resize_dialog
            || self.show_folder_browser
            || self.context_menu_pos.is_some()
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
        self.image_rect = None; // reset; set below when an image is drawn

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
        let image_rect = egui::Rect::from_min_size(top_left, draw);
        self.image_rect = Some(image_rect);

        ui.painter().image(
            texture_id,
            image_rect,
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

    /// Normalized filmstrip window: `(tile_count, half_window)` where the count
    /// is forced odd (7/9/11) so the current tile always stays centered.
    fn filmstrip_window(&self) -> (usize, isize) {
        let count = self.config.filmstrip_count.clamp(7, 11) | 1;
        (count, ((count - 1) / 2) as isize)
    }

    /// Overlay visibility for auto-fade: `Some(alpha)` when the HUD/pill/
    /// filmstrip should render (fading to 0 over the last half-second), or
    /// `None` once fully faded so the overlays stop intercepting input.
    fn overlay_opacity(&self) -> Option<f32> {
        if !self.config.auto_fade_overlays {
            return Some(1.0);
        }
        let delay = self.config.fade_delay_secs.clamp(1, 10) as f32;
        let elapsed = self.last_interaction_instant.elapsed().as_secs_f32();
        if elapsed <= delay {
            return Some(1.0);
        }
        let alpha = ((delay + 0.5 - elapsed) / 0.5).clamp(0.0, 1.0);
        (alpha > 0.0).then_some(alpha)
    }

    fn draw_hud(&mut self, ctx: &egui::Context) {
        let Some(opacity) = self.overlay_opacity() else {
            return;
        };
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
                ui.set_opacity(opacity);
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
        let Some(opacity) = self.overlay_opacity() else {
            return None;
        };
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
                ui.set_opacity(opacity);
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
            if pill_button(ui, icons::ICON_OPEN_FILE, "Open File (Ctrl+O)", false).clicked() {
                return Some(PillAction::OpenFile);
            }
        }
        if self.config.pill_open_folder {
            if pill_button(ui, icons::ICON_OPEN_FOLDER, "Open Folder (Ctrl+Shift+O)", false)
                .clicked()
            {
                return Some(PillAction::OpenFolder);
            }
        }
        if self.config.pill_save_as {
            if pill_button(ui, icons::ICON_SAVE, "Save As (Ctrl+S)", false).clicked() {
                return Some(PillAction::SaveAs);
            }
        }
        if self.config.pill_print {
            if pill_button(ui, icons::ICON_PRINT, "Print (Ctrl+P)", false).clicked() {
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
            if pill_button(
                ui,
                icons::ICON_FILMSTRIP,
                "Toggle Filmstrip (T)",
                self.config.show_filmstrip,
            )
            .clicked()
            {
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

    /// Draw one grid thumbnail card; returns the interaction, if any.
    fn grid_tile(&mut self, ui: &mut egui::Ui, index: usize) -> Option<GridTileAction> {
        let colors = self.theme();
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(150.0, 170.0), egui::Sense::click());
        let painter = ui.painter();
        let card = rect.shrink(4.0);

        // Rounded card background (theme-aware glass tint).
        painter.rect_filled(card, 6.0, colors.placeholder);

        // Selection / hover accent frame.
        if index == self.current_index {
            painter.rect_stroke(card, 6.0, egui::Stroke::new(2.0_f32, GOLD_COLOR));
        } else if response.hovered() {
            painter.rect_stroke(card, 6.0, egui::Stroke::new(1.0_f32, colors.muted));
        }

        // Aspect-preserved thumbnail inside a 126×122 box.
        let thumb_box =
            egui::Rect::from_min_size(rect.min + egui::vec2(12.0, 8.0), egui::vec2(126.0, 122.0));
        match self.thumb_cache.get(&index) {
            Some(thumb) => {
                let img_rect = fit_rect_contain(thumb.dims, thumb_box);
                painter.image(
                    thumb.texture.id(),
                    img_rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            None if self.failed_thumb.contains(&index) => {
                painter.text(
                    thumb_box.center(),
                    egui::Align2::CENTER_CENTER,
                    "✕",
                    egui::FontId::proportional(18.0),
                    colors.muted,
                );
            }
            None => {
                // Still decoding — background card only.
            }
        }

        // Truncated filename label (11px).
        let name = self
            .files
            .get(index)
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        painter.text(
            rect.min + egui::vec2(12.0, 136.0),
            egui::Align2::LEFT_TOP,
            truncate(&name, 18),
            egui::FontId::proportional(11.0),
            colors.primary,
        );

        if response.double_clicked() {
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

    /// Full-window grid selector view (virtualized thumbnail browser).
    fn draw_grid_view(&mut self, ctx: &egui::Context) {
        let len = self.files.len();
        let mut back_clicked = false;
        let mut open_index: Option<usize> = None;
        let mut menu_index: Option<usize> = None;
        let mut menu_pos: Option<egui::Pos2> = None;

        let folder = self
            .files
            .first()
            .and_then(|p| p.parent())
            .map(|p| p.display().to_string())
            .unwrap_or_default();

        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                // Slim top header bar.
                ui.horizontal(|ui| {
                    ui.label(format!("{} {} · {} images", icons::ICON_OPEN_FOLDER, folder, len));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Back to Viewer (Enter)").clicked() {
                            back_clicked = true;
                        }
                    });
                });
                ui.separator();

                // Virtualized grid: only visible rows are laid out and only
                // their thumbnails are requested (bounds memory for huge folders).
                // Subtract a little for the vertical scrollbar so the last
                // column never slips underneath it.
                let columns = ((ui.available_width() - 12.0) / 150.0).floor().max(1.0) as usize;
                self.grid_columns = columns;
                let row_height = 170.0;
                let total_rows = if len == 0 { 0 } else { (len + columns - 1) / columns };

                // Boundary-aware auto-scroll: bring the selected card into view
                // only when it falls outside the current viewport. The card's
                // screen rect is computed from the fixed row/column geometry, so
                // this works even for off-screen rows (Up past the top edge and
                // Home/End jumps). `scroll_to_rect(..., None)` performs the
                // minimal scroll — no centering — so in-view movement never snaps.
                egui::ScrollArea::vertical()
                    .id_source("grid-scroll")
                    .auto_shrink([false, false])
                    .show_rows(ui, row_height, total_rows, |ui, row_range| {
                        if self.scroll_to_selected {
                            self.scroll_to_selected = false;
                            if total_rows > 0 {
                                let spacing = ui.spacing().item_spacing;
                                let min_row = row_range.start;
                                let row = self.current_index / columns;
                                let col = self.current_index % columns;
                                let origin = ui.max_rect().min;
                                let rect = egui::Rect::from_min_size(
                                    egui::pos2(
                                        origin.x + col as f32 * (150.0 + spacing.x),
                                        origin.y
                                            + (row as isize - min_row as isize) as f32
                                                * (row_height + spacing.y),
                                    ),
                                    egui::vec2(150.0, row_height),
                                );
                                ui.scroll_to_rect(rect, None);
                            }
                        }

                        for row in row_range {
                            ui.horizontal(|ui| {
                                for col in 0..columns {
                                    let index = row * columns + col;
                                    if index >= len {
                                        break;
                                    }
                                    self.request_thumb_for(index);
                                    if let Some(action) = self.grid_tile(ui, index) {
                                        match action {
                                            GridTileAction::Select(i) => {
                                                self.grid_select(i);
                                            }
                                            GridTileAction::Open(i) => open_index = Some(i),
                                            GridTileAction::ContextMenu(i, pos) => {
                                                menu_index = Some(i);
                                                menu_pos = Some(pos);
                                            }
                                        }
                                    }
                                }
                            });
                        }
                    });
            });

        // Post-actions (double-click open, right-click menu, back button).
        if let Some(index) = open_index {
            self.current_index = index;
            self.leave_grid(ctx);
            return;
        }
        if let Some(index) = menu_index {
            self.current_index = index;
            self.context_menu_pos = menu_pos;
        }
        if back_clicked {
            self.leave_grid(ctx);
        }
    }

    fn draw_filmstrip(&mut self, ctx: &egui::Context) {
        let Some(opacity) = self.overlay_opacity() else {
            return;
        };
        if !self.config.show_filmstrip || self.files.is_empty() {
            return;
        }
        let colors = self.theme();
        let len = self.files.len();

        // Circular wrap: `half` before + current + `half` after, looping around
        // the ends. Folders smaller than the configured window show every image.
        let (count, half) = self.filmstrip_window();
        let indices: Vec<usize> = if len >= count {
            (-half..=half)
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
                ui.set_opacity(opacity);
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
        // Image-edit actions (resize/rotation) only make sense in Viewer mode
        // where a decoded image is available; grid tiles are file-only targets.
        let has_static = self.view_mode == ViewMode::Viewer && self.image.is_some();
        let has_rotation = has_static && self.rotation != 0;
        let dims = self
            .image
            .as_ref()
            .map(|im| im.dimensions)
            .or_else(|| self.animation.as_ref().map(|a| a.dimensions))
            .unwrap_or([0, 0]);

        // "Copy/Move to..." destination labels (target folder name, or default).
        let copy_target_label = self
            .config
            .last_copy_folder
            .as_ref()
            .map(|dir| {
                let name = dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| dir.display().to_string());
                format!("Copy to {name}")
            })
            .unwrap_or_else(|| "Copy File to...".to_string());
        let move_target_label = self
            .config
            .last_move_folder
            .as_ref()
            .map(|dir| {
                let name = dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| dir.display().to_string());
                format!("Move to {name}")
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
        let colors = self.theme();

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
                    if menu_item_enabled(ui, "Resize Image...", has_static).clicked() {
                        action = Some(MenuAction::ResizeImage);
                        should_close = true;
                    }
                    if menu_item_enabled(ui, "Save Rotation", has_rotation).clicked() {
                        action = Some(MenuAction::SaveRotation);
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
        if open_info {
            self.show_image_info = true;
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
                        ("Toggle Grid / Viewer mode", "Enter"),
                        ("Zoom in / out", "Ctrl + Wheel"),
                        ("Fit to window", "0"),
                        ("Actual size (100%)", "1"),
                        ("Pan", "Left-drag (when zoomed)"),
                        ("Rotate 90° CW", "R"),
                        ("Resize image", "Ctrl + Shift + R"),
                        ("Toggle filmstrip", "T"),
                        ("Image properties / info", "I"),
                        ("Play / Pause animation", "Space"),
                        ("Copy path", "Ctrl + C"),
                        ("Copy image", "Ctrl + Shift + C"),
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
        let mut clean_clicked = false;
        let mut open_default_apps_clicked = false;
        let mut reset_clicked = false;
        let mut done_clicked = false;
        let mut open = self.show_settings;
        let colors = self.theme();

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

        egui::Window::new(format!("{}  Settings", icons::ICON_GEAR))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size(egui::vec2(620.0, 420.0))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .frame(modal_frame)
            .show(ctx, |ui| {
                // Enforce total inner bounds so content can never balloon.
                ui.set_min_size(egui::vec2(600.0, 370.0));
                ui.set_max_size(egui::vec2(600.0, 370.0));

                ui.horizontal_top(|ui| {
                    // Left navigation sidebar rail (fixed width).
                    ui.vertical(|ui| {
                        ui.set_width(140.0);
                        let tabs = [
                            (SettingsTab::Appearance, icons::ICON_PALETTE, "Appearance"),
                            (SettingsTab::Filmstrip, icons::ICON_FILMSTRIP, "Filmstrip"),
                            (SettingsTab::QuickPills, icons::ICON_BOLT, "Quick-Pills"),
                            (SettingsTab::Behavior, icons::ICON_GEAR, "Behavior"),
                            (SettingsTab::Integration, icons::ICON_PUZZLE, "Integration"),
                        ];
                        for (tab, icon, label) in tabs {
                            let text = format!(" {icon} {label}");
                            let selected = self.settings_tab == tab;
                            let response = ui.add_sized(
                                [135.0, 28.0],
                                egui::SelectableLabel::new(selected, text),
                            );
                            if response.clicked() {
                                self.settings_tab = tab;
                            }
                        }
                    });

                    // Vertical divider between the rail and the content pane.
                    ui.separator();

                    // Right content pane: scrollable, strictly top-down.
                    egui::ScrollArea::vertical()
                        .id_source("settings-content")
                        .max_height(320.0)
                        .show(ui, |ui| {
                            ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                                match self.settings_tab {
                                    SettingsTab::Appearance => {
                                        changed |= self.settings_appearance(ui);
                                    }
                                    SettingsTab::Filmstrip => {
                                        changed |= self.settings_filmstrip(ui);
                                    }
                                    SettingsTab::QuickPills => {
                                        changed |= self.settings_quick_pills(ui);
                                    }
                                    SettingsTab::Behavior => {
                                        changed |= self.settings_behavior(ui);
                                    }
                                    SettingsTab::Integration => {
                                        let (reg, folder, clean, apps) =
                                            self.settings_integration(ui, &colors);
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
                        if ui.button("Reset to Defaults").clicked() {
                            reset_clicked = true;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Done").clicked() {
                                done_clicked = true;
                            }
                        });
                    },
                );
            });

        // Reflect the window's close (X) button and the Done button on the
        // modal flag (a local bool avoids a `&mut self` borrow conflict with
        // the content closure that also borrows `self`).
        self.show_settings = open;
        if done_clicked {
            self.show_settings = false;
        }

        if changed {
            self.save_config();
        }
        if register_clicked {
            self.refresh_registration();
        }
        if folder_menu_clicked {
            self.toggle_folder_context_menu();
        }
        if clean_clicked {
            self.unregister_all();
        }
        if open_default_apps_clicked {
            platform::open_default_apps_settings();
        }
        if reset_clicked {
            self.reset_settings_to_defaults();
        }
    }

    /// Reset every setting to its default while preserving session memory
    /// (last directory/file and window geometry).
    fn reset_settings_to_defaults(&mut self) {
        let dir = self.config.last_directory.clone();
        let file = self.config.last_file_path.clone();
        let pos = self.config.window_pos;
        let size = self.config.window_size;
        self.config = config::Config::default();
        self.config.last_directory = dir;
        self.config.last_file_path = file;
        self.config.window_pos = pos;
        self.config.window_size = size;
        self.save_config();
        self.show_success("Settings reset to defaults");
    }

    /// Settings → Appearance: theme, backdrop, glass tint, HUD, overlay fade.
    fn settings_appearance(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;

        // Card 1: Theme & Background.
        settings_card(ui, |ui| {
            ui.label(egui::RichText::new("Theme & Background").strong());
            let mut light = self.config.canvas_background == config::CanvasBackground::Light;
            let mut glass = self.config.canvas_background == config::CanvasBackground::Glass;

            ui.horizontal(|ui| {
                ui.label("Theme:");
                ui.radio_value(&mut light, false, "Dark");
                ui.radio_value(&mut light, true, "Light");
            });
            ui.horizontal(|ui| {
                ui.label("Canvas Background:");
                ui.add_enabled_ui(!light, |ui| {
                    ui.radio_value(&mut glass, true, "Translucent Glass");
                    ui.radio_value(&mut glass, false, "Solid Color");
                });
            });
            ui.horizontal(|ui| {
                ui.label("Glass Opacity:");
                changed |= ui
                    .add(egui::Slider::new(&mut self.config.glass_opacity, 100..=255))
                    .changed();
            });

            // Commit the two toggles to the single backdrop enum.
            let background = background_from(light, glass);
            if background != self.config.canvas_background {
                self.config.canvas_background = background;
                changed = true;
            }
        });
        ui.add_space(6.0);

        // Card 2: Screen Overlays.
        settings_card(ui, |ui| {
            ui.label(egui::RichText::new("Screen Overlays").strong());
            ui.horizontal(|ui| {
                ui.label("Navigation HUD:");
                changed |= ui
                    .radio_value(&mut self.config.hud_position, config::HudPosition::Bottom, "Bottom")
                    .changed();
                changed |= ui
                    .radio_value(&mut self.config.hud_position, config::HudPosition::Top, "Top")
                    .changed();
                changed |= ui
                    .radio_value(&mut self.config.hud_position, config::HudPosition::Hidden, "Hidden")
                    .changed();
            });
            changed |= ui
                .checkbox(&mut self.config.auto_fade_overlays, "Auto-fade overlays when idle")
                .changed();
            if self.config.auto_fade_overlays {
                ui.horizontal(|ui| {
                    ui.label("Fade delay:");
                    changed |= ui
                        .add(egui::Slider::new(&mut self.config.fade_delay_secs, 1..=10).suffix(" s"))
                        .changed();
                });
            }
        });
        changed
    }

    /// Settings → Filmstrip: master toggle, dock, and thumbnail count.
    fn settings_filmstrip(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;

        // Card 1: master toggle.
        settings_card(ui, |ui| {
            changed |= ui
                .checkbox(&mut self.config.show_filmstrip, "Show Thumbnail Filmstrip (T)")
                .changed();
        });
        ui.add_space(6.0);

        // Card 2: indented options (disabled when the filmstrip is off).
        settings_card(ui, |ui| {
            ui.add_enabled_ui(self.config.show_filmstrip, |ui| {
                ui.indent("filmstrip_options", |ui| {
                    ui.label("Dock position:");
                    changed |= ui
                        .radio_value(&mut self.config.filmstrip_position, config::FilmstripPosition::Bottom, "Bottom")
                        .changed();
                    changed |= ui
                        .radio_value(&mut self.config.filmstrip_position, config::FilmstripPosition::Top, "Top")
                        .changed();
                    changed |= ui
                        .radio_value(&mut self.config.filmstrip_position, config::FilmstripPosition::Left, "Left")
                        .changed();
                    changed |= ui
                        .radio_value(&mut self.config.filmstrip_position, config::FilmstripPosition::Right, "Right")
                        .changed();
                    ui.label("Visible Thumbnails:");
                    ui.horizontal(|ui| {
                        for &count in &[7usize, 9, 11] {
                            if ui
                                .selectable_value(&mut self.config.filmstrip_count, count, count.to_string())
                                .changed()
                            {
                                changed = true;
                            }
                        }
                    });
                });
            });
        });
        changed
    }

    /// Settings → Quick-Pills: master toggle, layout, and per-button visibility.
    fn settings_quick_pills(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;

        // Card 1: master toggle.
        settings_card(ui, |ui| {
            changed |= ui
                .checkbox(&mut self.config.quick_pill_enabled, "Enable Top-Right Quick-Action Pill")
                .changed();
        });
        ui.add_space(6.0);

        // Card 2: indented options (disabled when the pill is off).
        settings_card(ui, |ui| {
            ui.add_enabled_ui(self.config.quick_pill_enabled, |ui| {
                ui.indent("quick_pill_options", |ui| {
                    ui.label("Layout:");
                    ui.horizontal(|ui| {
                        changed |= ui
                            .radio_value(&mut self.config.quick_pill_orientation, config::Orientation::Horizontal, "Horizontal")
                            .changed();
                        changed |= ui
                            .radio_value(&mut self.config.quick_pill_orientation, config::Orientation::Vertical, "Vertical")
                            .changed();
                    });
                    ui.label("Buttons:");
                    egui::Grid::new("quick_pill_buttons_grid")
                        .num_columns(2)
                        .spacing(egui::vec2(14.0, 2.0))
                        .show(ui, |ui| {
                            changed |= ui.checkbox(&mut self.config.pill_open_file, "Open File").changed();
                            changed |= ui.checkbox(&mut self.config.pill_open_folder, "Open Folder").changed();
                            ui.end_row();
                            changed |= ui.checkbox(&mut self.config.pill_save_as, "Save As...").changed();
                            changed |= ui.checkbox(&mut self.config.pill_print, "Print...").changed();
                            ui.end_row();
                            changed |= ui.checkbox(&mut self.config.pill_share, "Share...").changed();
                            changed |= ui.checkbox(&mut self.config.pill_filmstrip, "Toggle Filmstrip").changed();
                            ui.end_row();
                        });
                });
            });
        });
        changed
    }

    /// Settings → Behavior: wheel action and startup/browsing options.
    fn settings_behavior(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;

        // Card 1: mouse wheel action.
        settings_card(ui, |ui| {
            ui.label("Mouse Wheel:");
            ui.horizontal(|ui| {
                changed |= ui
                    .radio_value(&mut self.config.mouse_wheel_action, config::MouseWheelAction::Navigate, "Next / Previous Image")
                    .changed();
                changed |= ui
                    .radio_value(&mut self.config.mouse_wheel_action, config::MouseWheelAction::Zoom, "Zoom In / Out")
                    .changed();
            });
        });
        ui.add_space(6.0);

        // Card 2: startup & browsing.
        settings_card(ui, |ui| {
            changed |= ui
                .checkbox(&mut self.config.remember_last_folder, "Remember last viewed image and folder on startup")
                .changed();
            changed |= ui
                .checkbox(&mut self.config.loop_navigation, "Loop navigation at folder boundary")
                .changed();
        });
        ui.add_space(6.0);

        // Card 3: fullscreen.
        settings_card(ui, |ui| {
            changed |= ui
                .checkbox(&mut self.config.start_fullscreen, "Start in Fullscreen")
                .changed();
        });
        changed
    }

    /// Settings → Integration: registry health (async), Explorer context menu,
    /// and a full unregister. Returns `(register, folder_menu, clean, apps)`.
    fn settings_integration(
        &mut self,
        ui: &mut egui::Ui,
        colors: &theme::ThemeColors,
    ) -> (bool, bool, bool, bool) {
        let mut register_clicked = false;
        let mut folder_menu_clicked = false;
        let mut clean_clicked = false;
        let mut open_default_apps_clicked = false;
        let checking = self.checking_registry || self.registry_status.is_none();
        let current_exe = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "<unknown>".to_string());

        // Card 1: default app handler.
        settings_card(ui, |ui| {
            ui.label(egui::RichText::new("Default App Handler").strong());
            ui.add_space(4.0);
            if checking {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.label("Checking registry…");
                });
            } else {
                match self.registry_status.as_ref() {
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
            ui.label(format!("Executable: {current_exe}"));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Register as Default App Handler").clicked() {
                    register_clicked = true;
                }
                if ui.button("Open Windows Default Apps").clicked() {
                    open_default_apps_clicked = true;
                }
            });
        });
        ui.add_space(6.0);

        // Card 2: Explorer context menu.
        settings_card(ui, |ui| {
            ui.label(egui::RichText::new("Explorer Context Menu").strong());
            ui.add_space(4.0);
            if checking {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.label("Checking…");
                });
            } else if self.folder_menu_registered {
                ui.label(
                    egui::RichText::new(format!("{} Active", icons::ICON_CHECK))
                        .color(SUCCESS_COLOR),
                );
                if ui.button("Remove 'Open folder with Photonio'").clicked() {
                    folder_menu_clicked = true;
                }
            } else {
                ui.label(
                    egui::RichText::new(format!("{} Not in folder context menu", icons::ICON_CIRCLE))
                        .color(colors.muted),
                );
                if ui.button("Add 'Open folder with Photonio'").clicked() {
                    folder_menu_clicked = true;
                }
            }
        });
        ui.add_space(6.0);

        // Card 3: clean purge (danger).
        settings_card(ui, |ui| {
            ui.label(egui::RichText::new("Clean Purge").strong());
            ui.add_space(4.0);
            ui.label("Remove every registry entry Photonio has written.");
            let danger = egui::Button::new(
                egui::RichText::new("Unregister & Clean Registry").color(ERROR_COLOR),
            );
            if ui.add(danger).clicked() {
                clean_clicked = true;
            }
        });

        (register_clicked, folder_menu_clicked, clean_clicked, open_default_apps_clicked)
    }

    /// The "Resize Image" modal: presets, target inputs, and save actions.
    fn draw_resize_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_resize_dialog {
            return;
        }
        if self.image.is_none() {
            self.show_resize_dialog = false;
            return;
        }
        let colors = self.theme();
        let orig = self.resize_orig;
        let megapixels = (orig[0] as f64 * orig[1] as f64) / 1_000_000.0;

        let mut open = self.show_resize_dialog;
        let mut save_copy = false;
        let mut overwrite = false;
        let mut done = false;

        let modal_frame = egui::Frame::none()
            .fill(colors.panel_fill)
            .stroke(egui::Stroke::new(1.0_f32, colors.border))
            .rounding(10.0)
            .inner_margin(egui::Margin::symmetric(12.0, 10.0))
            .shadow(egui::Shadow {
                offset: egui::vec2(0.0, 4.0),
                blur: 18.0,
                spread: 0.0,
                color: egui::Color32::from_black_alpha(150),
            });

        egui::Window::new("Resize Image")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size(egui::vec2(380.0, 360.0))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .frame(modal_frame)
            .show(ctx, |ui| {
                ui.label(format!("Original: {} × {} px", orig[0], orig[1]));
                ui.label(format!("Megapixels: {megapixels:.2} MP"));
                ui.separator();

                // Presets. A fixed-dimension preset (1920×1080) uses cover &
                // center-crop when the source aspect ratio differs from 16:9.
                let is_16_9 = (orig[0] as f64 / orig[1].max(1) as f64 - 16.0 / 9.0).abs() < 0.01;
                ui.horizontal(|ui| {
                    if ui.button("25%").clicked() {
                        self.resize_width = ((orig[0] as f32) * 0.25).round().max(1.0) as u32;
                        self.resize_height = ((orig[1] as f32) * 0.25).round().max(1.0) as u32;
                        self.resize_cover_crop = false;
                    }
                    if ui.button("50%").clicked() {
                        self.resize_width = ((orig[0] as f32) * 0.5).round().max(1.0) as u32;
                        self.resize_height = ((orig[1] as f32) * 0.5).round().max(1.0) as u32;
                        self.resize_cover_crop = false;
                    }
                    if ui.button("75%").clicked() {
                        self.resize_width = ((orig[0] as f32) * 0.75).round().max(1.0) as u32;
                        self.resize_height = ((orig[1] as f32) * 0.75).round().max(1.0) as u32;
                        self.resize_cover_crop = false;
                    }
                    if ui.button("1920×1080").clicked() {
                        self.resize_width = 1920;
                        self.resize_height = 1080;
                        self.resize_cover_crop = true;
                    }
                });
                if !is_16_9 {
                    ui.label(
                        egui::RichText::new("(Center-cropped to fit)")
                            .small()
                            .italics()
                            .color(colors.muted),
                    );
                }
                ui.separator();

                // Target inputs. Manual edits clear the fixed-preset crop; the
                // aspect lock then updates the other axis proportionally.
                ui.horizontal(|ui| {
                    ui.label("Width:");
                    let w = ui.add(egui::DragValue::new(&mut self.resize_width).range(1..=100_000));
                    if w.changed() {
                        self.resize_cover_crop = false;
                        if self.resize_aspect {
                            let ow = orig[0].max(1) as f32;
                            let oh = orig[1].max(1) as f32;
                            self.resize_height = ((self.resize_width as f32) * oh / ow).round().max(1.0) as u32;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Height:");
                    let h = ui.add(egui::DragValue::new(&mut self.resize_height).range(1..=100_000));
                    if h.changed() {
                        self.resize_cover_crop = false;
                        if self.resize_aspect {
                            let ow = orig[0].max(1) as f32;
                            let oh = orig[1].max(1) as f32;
                            self.resize_width = ((self.resize_height as f32) * ow / oh).round().max(1.0) as u32;
                        }
                    }
                });
                ui.checkbox(&mut self.resize_aspect, "Maintain aspect ratio");
                ui.separator();

                // Save actions.
                if ui.button("Save as Copy...").clicked() {
                    save_copy = true;
                }
                if self.resize_confirm_overwrite {
                    ui.horizontal(|ui| {
                        ui.label("Overwrite original?");
                        if ui.button("Yes").clicked() {
                            overwrite = true;
                        }
                        if ui.button("Cancel").clicked() {
                            self.resize_confirm_overwrite = false;
                        }
                    });
                } else if ui.button("Overwrite Original").clicked() {
                    self.resize_confirm_overwrite = true;
                }

                ui.separator();
                if ui.button("Close").clicked() {
                    done = true;
                }
            });

        self.show_resize_dialog = open;
        if done {
            self.show_resize_dialog = false;
        }
        if save_copy {
            self.resize_save(false, ctx);
        }
        if overwrite {
            self.resize_confirm_overwrite = false;
            self.resize_save(true, ctx);
        }
    }

    /// The "Image Properties" modal: basic details, file details, and EXIF.
    fn draw_image_info(&mut self, ctx: &egui::Context) {
        if !self.show_image_info {
            return;
        }
        let Some(path) = self.current_file().cloned() else {
            self.show_image_info = false;
            return;
        };

        let dims = self
            .image
            .as_ref()
            .map(|im| im.dimensions)
            .or_else(|| self.animation.as_ref().map(|a| a.dimensions))
            .unwrap_or([0, 0]);

        let meta = viewer::metadata::collect(&path, dims);
        let colors = self.theme();
        let file_path = path.display().to_string();
        let folder_path = path
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default();

        let mut open = self.show_image_info;
        let mut copy_file = false;
        let mut copy_folder = false;
        let mut open_folder = false;
        let mut done = false;

        let modal_frame = egui::Frame::none()
            .fill(colors.panel_fill)
            .stroke(egui::Stroke::new(1.0_f32, colors.border))
            .rounding(10.0)
            .inner_margin(egui::Margin::symmetric(12.0, 10.0))
            .shadow(egui::Shadow {
                offset: egui::vec2(0.0, 4.0),
                blur: 18.0,
                spread: 0.0,
                color: egui::Color32::from_black_alpha(150),
            });

        egui::Window::new(format!("{}  Image Properties", icons::ICON_INFO))
            .open(&mut open)
            .default_size(egui::vec2(480.0, 480.0))
            .min_size(egui::vec2(440.0, 400.0))
            .max_size(egui::vec2(540.0, 600.0))
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .frame(modal_frame)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_source("image-info-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("General").strong());
                        property_grid("image_props_general", ui, &meta.general);
                        ui.separator();

                        ui.label(egui::RichText::new("File Details").strong());
                        property_grid("image_props_file", ui, &meta.file_details);
                        ui.separator();

                        ui.label(egui::RichText::new("EXIF / Camera").strong());
                        if meta.has_exif {
                            property_grid("image_props_exif", ui, &meta.exif);
                        } else {
                            ui.label(
                                egui::RichText::new("No EXIF camera metadata embedded")
                                    .italics()
                                    .color(colors.muted),
                            );
                        }
                    });

                ui.separator();

                // Pinned footer: file path, folder path, and Close.
                egui::Frame::group(ui.style())
                    .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui
                                .add(egui::Button::new(format!("{} Copy", icons::ICON_COPY)).small())
                                .clicked()
                            {
                                copy_file = true;
                            }
                            ui.add(egui::Label::new(&file_path).truncate())
                                .on_hover_text(&file_path);
                        });
                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::Button::new(format!("{} Open Folder", icons::ICON_OPEN_FOLDER))
                                        .small(),
                                )
                                .clicked()
                            {
                                open_folder = true;
                            }
                            if ui
                                .add(egui::Button::new(format!("{} Copy", icons::ICON_COPY)).small())
                                .clicked()
                            {
                                copy_folder = true;
                            }
                            ui.add(egui::Label::new(&folder_path).truncate())
                                .on_hover_text(&folder_path);
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Close").clicked() {
                                done = true;
                            }
                        });
                    });
            });

        self.show_image_info = open;
        if done {
            self.show_image_info = false;
        }

        if copy_file {
            ctx.copy_text(file_path.clone());
            self.show_success("File path copied");
        }
        if copy_folder {
            ctx.copy_text(folder_path.clone());
            self.show_success("Folder path copied");
        }
        if open_folder {
            if let Some(parent) = path.parent() {
                platform::open_folder(parent);
            }
        }
    }

    fn execute_menu_action(&mut self, action: MenuAction, ctx: &egui::Context) {
        match action {
            MenuAction::OpenFile => self.open_file_dialog(ctx),
            MenuAction::OpenFolder => self.open_folder_dialog(),
            MenuAction::CloseImage => self.close_current_file(ctx),
            MenuAction::CloseFolder => self.close_folder(),
            MenuAction::ResizeImage => self.open_resize_dialog(),
            MenuAction::SaveRotation => self.save_rotation(ctx),
            MenuAction::OpenContainingFolder => self.reveal_current(),
            MenuAction::OpenWith => self.open_with_current(),
            MenuAction::CopyFileTo => self.copy_file_to(ctx),
            MenuAction::MoveFileTo => self.move_file_to(ctx),
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

/// Render a compact, rounded "card" (grouped frame) for the Settings content.
fn settings_card<R>(
    ui: &mut egui::Ui,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::same(8.0))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
            add_contents(ui)
        })
}

/// Map the two Appearance toggles (light theme, glass backdrop) back onto the
/// single three-value `CanvasBackground` enum. Light is always solid; dark can
/// be translucent glass or an opaque solid color.
fn background_from(light: bool, glass: bool) -> config::CanvasBackground {
    if light {
        config::CanvasBackground::Light
    } else if glass {
        config::CanvasBackground::Glass
    } else {
        config::CanvasBackground::Dark
    }
}

/// Render a two-column, striped key/value grid for a metadata section.
fn property_grid(id: &str, ui: &mut egui::Ui, rows: &[(&'static str, String)]) {
    egui::Grid::new(id)
        .striped(true)
        .spacing([12.0, 6.0])
        .num_columns(2)
        .show(ui, |ui| {
            for (key, value) in rows {
                ui.label(*key);
                ui.label(value);
                ui.end_row();
            }
        });
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
fn pill_button(
    ui: &mut egui::Ui,
    glyph: &str,
    tooltip: &str,
    highlighted: bool,
) -> egui::Response {
    let mut text = egui::RichText::new(glyph).size(16.0);
    if highlighted {
        text = text.color(GIF_COLOR);
    }
    ui.add(
        egui::Button::new(text)
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

/// Move a file across filesystems: `rename` first, then copy + delete.
fn move_file_fs(src: &Path, dest: &Path) -> std::io::Result<()> {
    match std::fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(src, dest)?;
            std::fs::remove_file(src)
        }
    }
}

/// Build a non-colliding destination path in `dir` for a file named `name`.
fn unique_dest(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned());
    for i in 2u32..10_000 {
        let candidate_name = match &ext {
            Some(e) => format!("{stem} ({i}).{e}"),
            None => format!("{stem} ({i})"),
        };
        let candidate = dir.join(candidate_name);
        if !candidate.exists() {
            return candidate;
        }
    }
    first
}

/// Default "Save as Copy" filename: `stem_resized.ext`.
fn default_resized_name(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".to_string());
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("png");
    format!("{stem}_resized.{ext}")
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
    fn default_resized_name_appends_suffix() {
        assert_eq!(default_resized_name(Path::new("C:/pics/photo.jpg")), "photo_resized.jpg");
        assert_eq!(default_resized_name(Path::new("C:/pics/photo")), "photo_resized.png");
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
        self.poll_registry();
        // Retry visible-but-missing thumbnails every frame (Viewer filmstrip;
        // the grid requests its own visible tiles inside `draw_grid_view`).
        if self.view_mode == ViewMode::Viewer {
            self.request_thumbs(self.current_index);
        }
        // Re-queue the active image's full decode if a coalescing skip dropped
        // it, so the placeholder is always replaced by the real image.
        self.ensure_current_full_queued();

        let mut input = Self::capture_input(ctx);

        // While the user is typing in a text field (e.g. the folder browser's
        // path box), suppress the app's hotkeys so letters/shortcuts don't fire
        // global actions (Open Help, Rotate, Delete, etc.).
        if ctx.wants_keyboard_input() {
            input.prev = false;
            input.next = false;
            input.up = false;
            input.down = false;
            input.home = false;
            input.end = false;
            input.enter = false;
            input.reset_fit = false;
            input.reset_100 = false;
            input.rotate = false;
            input.delete = false;
            input.help = false;
            input.toggle_filmstrip = false;
            input.toggle_play = false;
            input.step_back = false;
            input.step_forward = false;
            input.toggle_image_info = false;
        }

        // Reset the overlay auto-fade timer on any user interaction.
        let any_key = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Key { .. })));
        if input.pointer_delta.length_sq() > 0.0
            || input.scroll != 0.0
            || input.primary_down
            || any_key
        {
            self.last_interaction_instant = std::time::Instant::now();
        }

        let (window_rect, close_requested) = ctx.input(|i| {
            (i.viewport().outer_rect, i.viewport().close_requested())
        });
        if window_rect.is_some() {
            self.window_rect = window_rect;
        }

        // Mode-aware keyboard navigation.
        let mut viewer_navigate: isize = 0;
        if self.view_mode == ViewMode::Grid {
            let cols = self.grid_columns.max(1) as isize;
            let mut grid_delta: isize = 0;
            if input.prev {
                grid_delta -= 1;
            }
            if input.next {
                grid_delta += 1;
            }
            if input.up {
                grid_delta -= cols;
            }
            if input.down {
                grid_delta += cols;
            }
            if input.home {
                if self.grid_select(0) {
                    self.scroll_to_selected = true;
                }
            } else if input.end {
                if self.grid_select(self.files.len().saturating_sub(1)) {
                    self.scroll_to_selected = true;
                }
            } else if grid_delta != 0 && !self.files.is_empty() {
                let len = self.files.len() as isize;
                let new = (self.current_index as isize + grid_delta).clamp(0, len - 1) as usize;
                if self.grid_select(new) {
                    self.scroll_to_selected = true;
                }
            }
        } else {
            if input.prev {
                viewer_navigate -= 1;
            }
            if input.next {
                viewer_navigate += 1;
            }
            let wheel_navigates =
                self.config.mouse_wheel_action == config::MouseWheelAction::Navigate;
            if wheel_navigates && !input.ctrl && input.scroll != 0.0 && !self.modal_open() {
                if input.scroll > 0.0 {
                    viewer_navigate -= 1;
                } else {
                    viewer_navigate += 1;
                }
            }
        }

        // Enter toggles between Viewer and the fullscreen grid selector.
        if input.enter && !self.modal_open() {
            if self.view_mode == ViewMode::Viewer {
                if !self.files.is_empty() {
                    self.view_mode = ViewMode::Grid;
                    self.scroll_to_selected = true;
                }
            } else {
                self.leave_grid(ctx);
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
        if input.toggle_image_info {
            if self.current_file().is_some() {
                self.show_image_info = !self.show_image_info;
            }
        }
        if input.resize_image {
            self.open_resize_dialog();
        }

        if self.view_mode == ViewMode::Grid {
            self.draw_grid_view(ctx);
        } else {
            egui::CentralPanel::default()
                .frame(egui::Frame::none())
                .show(ctx, |ui| {
                    self.draw_canvas(ui, &input);
                });

            if viewer_navigate != 0 {
                self.step(viewer_navigate, ctx);
            }

            self.draw_hud(ctx);

            if let Some(action) = self.draw_quick_pill(ctx) {
                self.execute_pill_action(action, ctx);
            }

            self.draw_filmstrip(ctx);
        }

        if let Some(action) = self.draw_context_menu(ctx) {
            self.execute_menu_action(action, ctx);
        }

        self.draw_help(ctx);
        self.draw_settings(ctx);
        self.draw_image_info(ctx);
        self.draw_resize_dialog(ctx);

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
            } else if self.show_image_info {
                self.show_image_info = false;
            } else if self.show_resize_dialog {
                self.show_resize_dialog = false;
            } else if self.show_folder_browser {
                self.close_folder_browser();
            } else if self.context_menu_pos.is_some() {
                self.context_menu_pos = None;
            } else if self.view_mode == ViewMode::Grid {
                self.leave_grid(ctx);
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        if input.open_file {
            self.open_file_dialog(ctx);
        }
        if input.open_folder {
            self.open_folder_dialog();
        }
        if input.copy_path {
            self.copy_current_path(ctx);
        }
        if input.copy_image {
            self.copy_current_image();
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

        // Overlay auto-fade: wake up exactly when the fade should begin, then
        // repaint during the fade window so the opacity animates; idle the rest
        // of the time so we don't waste CPU.
        if self.config.auto_fade_overlays {
            let delay = self.config.fade_delay_secs.clamp(1, 10) as f32;
            let elapsed = self.last_interaction_instant.elapsed().as_secs_f32();
            if elapsed < delay {
                ctx.request_repaint_after(std::time::Duration::from_secs_f32(delay - elapsed));
            } else if elapsed < delay + 0.5 {
                ctx.request_repaint_after(std::time::Duration::from_millis(32));
            }
        }

        // Safety net: while any decode is in flight, keep scheduling repaints
        // so a dropped worker wakeup can't leave the UI stuck on a placeholder.
        if !self.pending_full.is_empty() || !self.pending_thumb.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }

        // Folder browser modal — rendered last so it always sits on top of
        // every other element (HUD, pills, filmstrip, other modals).
        self.render_folder_browser(ctx);
    }
}
