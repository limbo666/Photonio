// File: src/app/mod.rs
// Purpose: Photonio — top-level egui application state and UI drawing.
//          Owns the directory listing, viewport (zoom/pan), config, context
//          menu, help/about/settings modals, branded empty state, and HUD.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use eframe::egui;

use crate::{about, assets, config, filesystem, icons, loader, platform, theme, viewer};
use crate::filesystem::{default_resized_name, move_file_fs, safe_rename, unique_dest};

pub mod breadcrumbs;
pub mod cache;
pub mod context_menu;
pub mod dialogs;
pub mod filmstrip;
pub mod grid;
pub mod hud;
pub mod types;
pub mod widgets;
pub mod crop;
#[cfg(test)]
mod tests;

pub(crate) use types::{
    apply_global_visuals, compute_feedback_alpha, is_supporter_check, setup_fonts, AnimationPlayer,
    InputSnapshot, MenuAction, MultiViewSubfolder, PendingOperation, PillAction, RenameState,
    SettingsTab, SubfolderWorkerMsg, Thumb, ViewMode, DONATION_URL, ERROR_COLOR,
    FEEDBACK_FADE_IN_SECS, FEEDBACK_HOLD_SECS, FEEDBACK_TOTAL_SECS,
};
pub(crate) use cache::{remap_cache_keys, remap_cache_keys_with_rename};
pub(crate) use widgets::decode_quad_thumb;

pub struct PhotonioApp {
    /// Currently displayed image (decoded + GPU texture), if any.
    pub(crate) image: Option<viewer::ViewerImage>,
    /// Low-res thumbnail shown in the main viewport while the full-res decode
    /// for the current index is still in flight — keeps the main window in
    /// sync with the filmstrip during fast navigation.
    pub(crate) preview: Option<(egui::TextureHandle, [usize; 2])>,
    /// Active animated GIF playback state, if the current image is one.
    pub(crate) animation: Option<AnimationPlayer>,
    /// Human-readable reason the last load failed, if it did.
    pub(crate) error: Option<String>,
    /// Naturally-sorted image files in the current directory.
    pub(crate) files: Vec<PathBuf>,
    /// Active directory opened in the current session (if any).
    pub(crate) opened_folder: Option<PathBuf>,
    /// Index into `files` of the currently displayed image.
    pub(crate) current_index: usize,
    /// `None` = fit to window; `Some(z)` = explicit zoom (1.0 = 100%).
    pub(crate) zoom: Option<f32>,
    /// Pan offset in points, applied after centering.
    pub(crate) pan: egui::Vec2,
    /// On-screen rect of the last drawn image (for double-click routing).
    pub(crate) image_rect: Option<egui::Rect>,
    /// Mouse drag start position for Viewer-mode native drag-out (when not zoomed).
    pub(crate) viewer_drag_start: Option<egui::Pos2>,
    /// Optional closest-edge anchor of the quick-action tool pill (None = default anchor).
    pub(crate) custom_pill_anchor: Option<config::OverlayAnchor>,
    /// Optional closest-edge anchor of the crop toolbar (None = default anchor).
    pub(crate) custom_crop_anchor: Option<config::OverlayAnchor>,
    /// Active interactive crop tool state (None when not in crop mode).
    pub(crate) crop_state: Option<crop::CropState>,
    /// Active full-window view (Viewer or fullscreen grid selector).
    pub view_mode: ViewMode,
    /// Number of grid columns, computed each frame in `draw_grid_view`.
    pub(crate) grid_columns: usize,
    /// Set when keyboard navigation / grid open should scroll the selected
    /// card into view (consumed by `draw_grid_view`).
    pub(crate) scroll_to_selected: bool,
    /// Cumulative in-memory manual rotation (0/90/180/270), applied by `R`.
    pub(crate) rotation: u32,
    /// Resize modal state: target width/height, aspect lock, and confirmation.
    pub(crate) resize_width: u32,
    pub(crate) resize_height: u32,
    pub(crate) resize_aspect: bool,
    pub(crate) resize_confirm_overwrite: bool,
    /// True when the target is a fixed-dimension preset → cover & center-crop.
    pub(crate) resize_cover_crop: bool,
    /// Original (pre-resize) dimensions captured when the dialog opened.
    pub(crate) resize_orig: [usize; 2],
    /// Persisted state (settings + last directory + window geometry).
    pub(crate) config: config::Config,
    /// Window outer rect, refreshed each frame for persistence.
    pub(crate) window_rect: Option<egui::Rect>,
    /// Window top-left position (outer coordinates), tracked for persistence.
    pub(crate) window_pos: Option<[f32; 2]>,
    /// Window client area size (inner coordinates), tracked for persistence.
    pub(crate) window_size: Option<[f32; 2]>,
    /// Position of the open context menu (`Some` = open).
    pub(crate) context_menu_pos: Option<egui::Pos2>,
    /// Modal visibility flags.
    pub(crate) show_help: bool,
    pub(crate) show_about: bool,
    pub(crate) show_settings: bool,
    pub(crate) show_image_info: bool,
    pub(crate) show_resize_dialog: bool,
    /// Rename modal state (present while the dialog is open).
    pub(crate) rename_state: Option<RenameState>,
    /// The custom folder browser is open.
    pub(crate) show_folder_browser: bool,
    /// What the folder browser should do when "Select Folder" is clicked.
    pub(crate) pending_operation: Option<PendingOperation>,
    /// Folder browser state (present while the browser is open).
    pub(crate) folder_browser: Option<viewer::folder_browser::Browser>,
    /// Whether the grid view path bar is currently in raw text edit mode.
    pub(crate) grid_path_editing: bool,
    /// Text buffer for editing the folder path in grid view.
    pub(crate) grid_path_buffer: String,
    /// Focus request flag for the grid path edit field.
    pub(crate) grid_path_focus_pending: bool,
    /// Whether the user-typed path in grid view failed directory validation.
    pub(crate) grid_path_invalid: bool,
    /// History stack of previously visited folders for the Back button.
    pub(crate) folder_back_stack: Vec<PathBuf>,
    /// History stack of folders for the Forward button.
    pub(crate) folder_forward_stack: Vec<PathBuf>,
    /// Set when help just opened, to ignore the same-frame click.
    pub(crate) help_opened_this_frame: bool,
    /// Transient feedback badge: (message, when shown).
    pub(crate) feedback: Option<(String, std::time::Instant)>,
    /// When an empty (no supported images) folder was opened; drives a
    /// self-dismissing notice that returns to the start screen after 7s.
    pub(crate) empty_folder_since: Option<std::time::Instant>,
    /// Windows registration health. `None` until the async inspector has run;
    /// refreshed (non-blocking) when Settings opens.
    pub(crate) registry_status: Option<platform::RegistryStatus>,
    /// Whether the Explorer folder context menu is registered.
    pub(crate) folder_menu_registered: bool,
    /// True while the background registry inspector thread is running.
    pub(crate) checking_registry: bool,
    /// Receiver for the async registry inspector result.
    pub(crate) registry_rx: Option<std::sync::mpsc::Receiver<(platform::RegistryStatus, bool)>>,
    /// When the user last interacted (pointer/keyboard), for overlay auto-fade.
    pub(crate) last_interaction_instant: std::time::Instant,
    /// Last recorded modification timestamp of the active directory (for auto-sensing changes).
    pub(crate) last_folder_mtime: Option<std::time::SystemTime>,
    /// Instant when the active directory was last checked for changes.
    pub(crate) last_folder_check: std::time::Instant,
    /// Previous frame's window focus state (detects focus regained).
    pub(crate) was_window_focused: bool,
    /// The active tab in the two-pane Settings modal (persisted across frames).
    pub(crate) settings_tab: SettingsTab,
    /// Cached at startup: `euro.png` / `euro.jpg` sits next to the executable.
    pub is_supporter: bool,
    /// The branded watermark logo texture + dimensions, if decoded.
    pub(crate) logo: Option<(egui::TextureHandle, [usize; 2])>,
    /// Background image decoder + preload channel manager.
    pub(crate) loader: loader::Loader,
    /// Cached display-res RGBA buffers by file index: (rgba, display, full).
    pub(crate) full_cache: HashMap<usize, (Vec<u8>, [usize; 2], [usize; 2])>,
    /// Indices with a full decode currently in flight (dedup).
    pub(crate) pending_full: HashSet<usize>,
    /// Indices with a thumbnail decode currently in flight (dedup + retry by LOD).
    pub(crate) pending_thumb: HashMap<usize, loader::PreviewLod>,
    /// Indices whose thumbnail decode definitively failed (shown as a
    /// placeholder; never retried until the folder changes).
    pub(crate) failed_thumb: HashSet<usize>,
    /// File index currently awaiting a full decode (to show a spinner).
    pub(crate) loading_index: Option<usize>,
    /// Cached thumbnails by file index.
    pub(crate) thumb_cache: HashMap<usize, Thumb>,
    /// Receiver for asynchronous background folder rescanning: (scanned_dir, new_files, is_manual).
    pub(crate) refresh_rx: Option<std::sync::mpsc::Receiver<(PathBuf, Vec<PathBuf>, bool)>>,
    /// True while a background folder rescan thread is running.
    pub(crate) is_refreshing: bool,
    /// Currently hovered grid item index (used to anchor zoom around the pointer target).
    pub(crate) grid_hovered_index: Option<usize>,
    /// Last measured grid content area available width.
    pub(crate) grid_available_width: f32,
    /// Item spacing in the grid.
    pub(crate) grid_spacing_y: f32,
    /// Subfolders contained within the active directory for MultiView.
    pub(crate) multiview_subfolders: Vec<MultiViewSubfolder>,
    /// Decoded GPU textures for subfolder quad previews, keyed by image path.
    pub(crate) multiview_folder_textures: HashMap<PathBuf, egui::TextureHandle>,
    /// Generation counter for async subfolder scans (session dedup).
    pub(crate) multiview_folder_gen: u64,
    /// Receiver for async subfolder and quad thumbnail worker messages.
    pub(crate) multiview_folder_rx: Option<std::sync::mpsc::Receiver<SubfolderWorkerMsg>>,
    /// Currently selected subfolder index in the MultiView grid, if any.
    pub(crate) grid_selected_folder: Option<usize>,
}

impl PhotonioApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        mut config: config::Config,
        cli_path: Option<PathBuf>,
    ) -> Self {
        apply_global_visuals(
            &cc.egui_ctx,
            config.canvas_background != config::CanvasBackground::Light,
        );
        setup_fonts(&cc.egui_ctx);

        // Disable egui's default keyboard-triggered whole-page zoom (Ctrl+±/0)
        // so hotkeys target image containers and content rather than scaling the entire UI.
        cc.egui_ctx.options_mut(|o| {
            o.zoom_with_keyboard = false;
        });
        cc.egui_ctx.set_zoom_factor(1.0);

        // Load the embedded logo once for the empty-state watermark + About.
        let logo = assets::logo_color_image().map(|(color, dims)| {
            let texture = cc
                .egui_ctx
                .load_texture("photonio-logo", color, egui::TextureOptions::LINEAR);
            (texture, dims)
        });

        // Cache the supporter check once at startup (no per-frame disk I/O).
        let is_supporter = is_supporter_check();
        // Bump the launch counter and persist it immediately.
        config.launch_count += 1;
        config.save();

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
            viewer_drag_start: None,
            custom_pill_anchor: config.quick_pill_anchor.or_else(|| {
                config
                    .quick_pill_pos
                    .map(|p| config::OverlayAnchor::new(false, false, p[0], p[1]))
            }),
            custom_crop_anchor: None,
            crop_state: None,
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
            window_pos: None,
            window_size: None,
            context_menu_pos: None,
            show_help: false,
            show_about: false,
            show_settings: false,
            show_image_info: false,
            show_resize_dialog: false,
            rename_state: None,
            show_folder_browser: false,
            pending_operation: None,
            folder_browser: None,
            grid_path_editing: false,
            grid_path_buffer: String::new(),
            grid_path_focus_pending: false,
            grid_path_invalid: false,
            folder_back_stack: Vec::new(),
            folder_forward_stack: Vec::new(),
            help_opened_this_frame: false,
            feedback: None,
            empty_folder_since: None,
            registry_status: None,
            folder_menu_registered: false,
            checking_registry: false,
            registry_rx: None,
            last_interaction_instant: std::time::Instant::now(),
            last_folder_mtime: None,
            last_folder_check: std::time::Instant::now(),
            was_window_focused: true,
            settings_tab: SettingsTab::Appearance,
            is_supporter,
            logo,
            loader: loader::Loader::new(cc.egui_ctx.clone()),
            full_cache: HashMap::new(),
            pending_full: HashSet::new(),
            pending_thumb: HashMap::new(),
            failed_thumb: HashSet::new(),
            loading_index: None,
            thumb_cache: HashMap::new(),
            refresh_rx: None,
            is_refreshing: false,
            grid_hovered_index: None,
            grid_available_width: 1000.0,
            grid_spacing_y: 6.0,
            multiview_subfolders: Vec::new(),
            multiview_folder_textures: HashMap::new(),
            multiview_folder_gen: 0,
            multiview_folder_rx: None,
            grid_selected_folder: None,
            opened_folder: None,
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
        } else {
            // Guarantee no stale folder/file state is retained when remember_last_folder is disabled.
            app.config.last_directory = None;
            app.config.last_file_path = None;
            app.opened_folder = None;
        }

        if app.current_folder().is_some() {
            app.refresh_multiview_subfolders(&cc.egui_ctx);
        }

        app
    }

    // ------------------------------------------------------------------
    // Loading & navigation
    // ------------------------------------------------------------------

    /// Show `index`: use a cached buffer instantly, or request a background
    /// decode and keep the previous image until it arrives.
    pub(crate) fn navigate_to(&mut self, index: usize, ctx: &egui::Context) {
        if index >= self.files.len() {
            return;
        }
        self.current_index = index;
        self.rotation = 0;
        self.crop_state = None;
        self.loader.set_current_index(index);

        // Record the exact last-viewed image + its parent directory so the app
        // can resume at this image on next launch (persisted on exit/open).
        if let Some(path) = self.files.get(index) {
            if let Some(parent) = path.parent() {
                self.opened_folder = Some(parent.to_path_buf());
            }
            if self.config.remember_last_folder {
                self.config.last_file_path = Some(path.clone());
                if let Some(parent) = path.parent() {
                    self.config.last_directory = Some(parent.to_path_buf());
                }
            } else {
                self.config.last_file_path = None;
                self.config.last_directory = None;
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
    pub(crate) fn set_image_from_rgba(
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
    pub(crate) fn set_preview(&mut self, index: usize) {
        if let Some(thumb) = self.thumb_cache.get(&index) {
            self.preview = Some((thumb.texture.clone(), thumb.dims));
            self.reset_viewport();
        }
    }

    /// Install a decoded animation as the active image (replacing any static
    /// image) and reset the viewport for the animation's canvas size.
    pub(crate) fn set_animation(
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
    pub(crate) fn request_preload(&mut self, index: usize) {
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

    /// Best-effort thumbnail request for one index at the specified LOD, deduped
    /// against the cache and in-flight set. Upgrades existing lower-res thumbnails
    /// seamlessly when higher zoom levels are viewed.
    pub(crate) fn request_thumb_for(&mut self, index: usize, needed_lod: loader::PreviewLod) {
        if self.failed_thumb.contains(&index) {
            return;
        }
        if let Some(existing) = self.thumb_cache.get(&index) {
            if existing.lod >= needed_lod {
                return;
            }
        }
        if let Some(&pending) = self.pending_thumb.get(&index) {
            if pending >= needed_lod {
                return;
            }
        }
        if let Some(path) = self.files.get(index).cloned() {
            if self.loader.request_thumb(index, path, needed_lod) {
                self.pending_thumb.insert(index, needed_lod);
            }
        }
    }

    /// Request low-res thumbnails for the visible filmstrip window (circular,
    /// matching `draw_filmstrip`). Idempotent and retried every frame.
    pub(crate) fn request_thumbs(&mut self, index: usize) {
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
            self.request_thumb_for(i, loader::PreviewLod::Small);
        }
    }

    /// Guarantee the currently-viewed image always has a full decode queued.
    /// A coalescing skip drops a request without a normal response, which can
    /// leave `pending_full` stale; retry here every frame (like thumbnails)
    /// so the full-res image reliably replaces the low-res placeholder.
    pub(crate) fn ensure_current_full_queued(&mut self) {
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
    pub(crate) fn poll_loader(&mut self, ctx: &egui::Context) {
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
                loader::ResponseKind::Thumb { index, rgba, dims, lod } => {
                    if self.pending_thumb.get(&index) == Some(&lod) {
                        self.pending_thumb.remove(&index);
                    }
                    let should_insert = match self.thumb_cache.get(&index) {
                        Some(existing) => lod >= existing.lod,
                        None => true,
                    };
                    if should_insert {
                        let color = egui::ColorImage::from_rgba_unmultiplied(dims, &rgba);
                        let tex = ctx.load_texture(
                            format!("thumb-{index}-{lod:?}"),
                            color,
                            egui::TextureOptions::LINEAR,
                        );
                        self.thumb_cache.insert(index, Thumb { texture: tex, dims, lod });
                    }
                    // If this is the image we're still waiting on, show it now.
                    if self.loading_index == Some(index) && index == self.current_index {
                        self.set_preview(index);
                    }
                }
                loader::ResponseKind::ThumbFailed { index, lod } => {
                    if self.pending_thumb.get(&index) == Some(&lod) {
                        self.pending_thumb.remove(&index);
                    }
                    if !self.thumb_cache.contains_key(&index) {
                        self.failed_thumb.insert(index);
                    }
                }
            }
        }
    }

    /// Drain worker messages for MultiView subfolders and quad thumbnail previews.
    pub(crate) fn poll_subfolders(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.multiview_folder_rx else { return };
        let mut had_updates = false;
        while let Ok(msg) = rx.try_recv() {
            match msg {
                SubfolderWorkerMsg::Folders { gen, folders } => {
                    if gen == self.multiview_folder_gen {
                        self.multiview_subfolders = folders;
                        had_updates = true;
                    }
                }
                SubfolderWorkerMsg::Thumb { gen, path, rgba, dims } => {
                    if gen == self.multiview_folder_gen {
                        let color = egui::ColorImage::from_rgba_unmultiplied(dims, &rgba);
                        let tex = ctx.load_texture(
                            format!("mv-folder-{}", path.display()),
                            color,
                            egui::TextureOptions::LINEAR,
                        );
                        self.multiview_folder_textures.insert(path, tex);
                        had_updates = true;
                    }
                }
            }
        }
        if had_updates {
            ctx.request_repaint();
        }
    }

    /// Asynchronously scan the active directory for direct subfolders and up to 4 preview images each,
    /// and asynchronously decode their quad thumbnails.
    pub(crate) fn refresh_multiview_subfolders(&mut self, ctx: &egui::Context) {
        self.multiview_folder_gen += 1;
        let gen = self.multiview_folder_gen;
        self.multiview_subfolders.clear();
        self.multiview_folder_textures.clear();
        self.grid_selected_folder = None;

        if !self.config.multiview_show_folders {
            self.multiview_folder_rx = None;
            return;
        }

        let Some(dir) = self.current_folder() else {
            self.multiview_folder_rx = None;
            return;
        };

        let (tx, rx) = std::sync::mpsc::channel();
        self.multiview_folder_rx = Some(rx);

        std::thread::spawn(move || {
            let mut subfolders = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }
                    if let Ok(meta) = entry.metadata() {
                        #[cfg(windows)]
                        {
                            use std::os::windows::fs::MetadataExt;
                            const HIDDEN: u32 = 0x2;
                            const SYSTEM: u32 = 0x4;
                            if meta.file_attributes() & (HIDDEN | SYSTEM) != 0 {
                                continue;
                            }
                        }
                    }
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.') {
                        continue;
                    }

                    // Hunt for up to 4 supported image files in this subfolder
                    let mut preview_paths = Vec::new();
                    if let Ok(sub_entries) = std::fs::read_dir(&path) {
                        for (i, sub_entry) in sub_entries.flatten().enumerate() {
                            if i >= 60 {
                                break;
                            }
                            let sub_path = sub_entry.path();
                            if sub_path.is_file() && filesystem::is_supported_path(&sub_path) {
                                preview_paths.push(sub_path);
                                if preview_paths.len() >= 4 {
                                    break;
                                }
                            }
                        }
                    }

                    subfolders.push(MultiViewSubfolder {
                        path,
                        name,
                        preview_paths,
                    });
                }
            }

            subfolders.sort_by(|a, b| natord::compare(&a.name, &b.name));

            // Send folder listing immediately so UI can display folder cards instantly
            if tx.send(SubfolderWorkerMsg::Folders { gen, folders: subfolders.clone() }).is_err() {
                return;
            }

            // Decode thumbnails for quad previews
            for folder in &subfolders {
                for img_path in &folder.preview_paths {
                    if let Some((rgba, dims)) = decode_quad_thumb(img_path, 160) {
                        if tx.send(SubfolderWorkerMsg::Thumb { gen, path: img_path.clone(), rgba, dims }).is_err() {
                            return;
                        }
                    }
                }
            }
        });

        ctx.request_repaint();
    }

    /// Clear all decode caches (used when the file list changes).
    pub(crate) fn clear_caches(&mut self) {
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
        self.multiview_folder_gen += 1;
        self.multiview_subfolders.clear();
        self.multiview_folder_textures.clear();
        self.multiview_folder_rx = None;
        self.grid_selected_folder = None;
    }

    /// Evict cache entries far from the current index to bound memory.
    pub(crate) fn evict_caches(&mut self) {
        let cur = self.current_index;
        let len = self.files.len();
        let (count, _) = self.filmstrip_window();
        self.full_cache.retain(|&i, _| loader::ring_dist(i, cur, len) <= 3);
        self.thumb_cache.retain(|&i, _| loader::ring_dist(i, cur, len) <= count + 2);
    }

    pub(crate) fn step(&mut self, delta: isize, ctx: &egui::Context) {
        let len = self.files.len();
        if len == 0 {
            self.check_folder_changes(ctx, true);
            return;
        }

        // Auto-sense folder changes during navigation (throttled to 500ms)
        if self.last_folder_check.elapsed() >= std::time::Duration::from_millis(500) {
            self.last_folder_check = std::time::Instant::now();
            self.check_folder_changes(ctx, false);
        }

        let next = if self.config.loop_navigation {
            // Wrap around both ends (ring-shaped).
            (self.current_index as isize + delta).rem_euclid(len as isize) as usize
        } else {
            // Clamp at the boundaries (no wrap).
            (self.current_index as isize + delta).clamp(0, len as isize - 1) as usize
        };

        // Defensive check: if target image was deleted on disk outside Photonio, refresh immediately
        if let Some(target) = self.files.get(next) {
            if !target.is_file() {
                self.check_folder_changes(ctx, true);
                return;
            }
        }

        if next != self.current_index {
            self.navigate_to(next, ctx);
        }
    }

    /// Select a grid tile (clamped); does not decode the full image.
    /// Returns `true` if the selection actually changed.
    pub(crate) fn grid_select(&mut self, index: usize) -> bool {
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
    pub(crate) fn leave_grid(&mut self, ctx: &egui::Context) {
        self.grid_path_editing = false;
        self.grid_path_invalid = false;
        self.view_mode = ViewMode::Viewer;
        if !self.files.is_empty() {
            self.navigate_to(self.current_index, ctx);
        }
        self.trigger_folder_refresh(ctx, false);
    }

    pub(crate) fn current_file(&self) -> Option<&PathBuf> {
        self.files.get(self.current_index)
    }

    pub(crate) fn current_folder(&self) -> Option<PathBuf> {
        self.files
            .first()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .or_else(|| self.opened_folder.clone())
            .filter(|p| p.is_dir())
    }

    /// Check whether the active directory was modified on disk (new files dropped, deleted, or renamed).
    /// If changes are detected, asynchronously triggers a non-disruptive folder refresh.
    pub(crate) fn check_folder_changes(&mut self, ctx: &egui::Context, force: bool) {
        if self.is_refreshing {
            return;
        }
        let Some(dir) = self.current_folder() else {
            return;
        };
        let current_mtime = std::fs::metadata(&dir).and_then(|m| m.modified()).ok();
        let changed = match (self.last_folder_mtime, current_mtime) {
            (Some(prev), Some(curr)) => prev != curr,
            (None, Some(_)) => false,
            _ => false,
        };

        if force || changed {
            self.last_folder_mtime = current_mtime;
            self.trigger_folder_refresh(ctx, false);
        }
    }

    /// Asynchronously initiate a background folder scan and reconcile changes
    /// without disrupting the active view, viewport zoom/pan, or existing GPU thumbnail caches.
    pub(crate) fn trigger_folder_refresh(&mut self, ctx: &egui::Context, manual: bool) {
        if self.is_refreshing {
            return;
        }
        let Some(dir) = self.current_folder() else {
            return;
        };
        self.is_refreshing = true;
        let (tx, rx) = std::sync::mpsc::channel();
        self.refresh_rx = Some(rx);
        let dir_clone = dir.clone();
        let worker_ctx = ctx.clone();
        let criteria = self.config.sort_criteria;
        let direction = self.config.sort_direction;
        std::thread::spawn(move || {
            let files = filesystem::scan_directory_sorted(&dir_clone, criteria, direction);
            let _ = tx.send((dir_clone, files, manual));
            worker_ctx.request_repaint();
        });
    }

    /// Trigger a manual folder refresh (e.g. from F5 key).
    pub(crate) fn trigger_manual_refresh(&mut self, ctx: &egui::Context) {
        self.trigger_folder_refresh(ctx, true);
    }

    /// Drain a finished background folder scan and apply the reconciliation.
    pub(crate) fn poll_refresh(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.refresh_rx else {
            return;
        };
        match rx.try_recv() {
            Ok((dir, new_files, manual)) => {
                self.is_refreshing = false;
                self.refresh_rx = None;
                if self.current_folder().as_ref() == Some(&dir) {
                    self.apply_folder_refresh(new_files, manual, ctx);
                }
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.is_refreshing = false;
                self.refresh_rx = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }

    /// Reconcile cached decoded RGBA buffers and GPU thumbnail textures from `old_files`
    /// to their new indices in `self.files` by matching canonical paths.
    pub(crate) fn reconcile_caches(&mut self, old_files: &[PathBuf]) {
        self.loader.bump_generation();

        self.full_cache = remap_cache_keys(&mut self.full_cache, old_files, &self.files);
        self.thumb_cache = remap_cache_keys(&mut self.thumb_cache, old_files, &self.files);
        self.pending_full.clear();
        self.pending_thumb.clear();
        self.failed_thumb.clear();
    }

    /// Reconcile cached decoded RGBA buffers and GPU thumbnail textures with an optional
    /// explicitly renamed item pair `(old_idx, new_idx)`.
    pub(crate) fn reconcile_caches_with_rename(
        &mut self,
        old_files: &[PathBuf],
        renamed: Option<(usize, usize)>,
    ) {
        if let Some(r) = renamed {
            self.loader.bump_generation();
            self.full_cache = remap_cache_keys_with_rename(
                &mut self.full_cache,
                old_files,
                &self.files,
                Some(r),
            );
            self.thumb_cache = remap_cache_keys_with_rename(
                &mut self.thumb_cache,
                old_files,
                &self.files,
                Some(r),
            );
            self.pending_full.clear();
            self.pending_thumb.clear();
            self.failed_thumb.clear();
        } else {
            self.reconcile_caches(old_files);
        }
    }

    /// Apply the newly scanned directory files non-disruptively.
    pub(crate) fn apply_folder_refresh(
        &mut self,
        new_files: Vec<PathBuf>,
        manual: bool,
        ctx: &egui::Context,
    ) {
        let changed = new_files != self.files;
        if !changed {
            if manual {
                // Invalidate active image's caches so in-place edits reload
                if !self.files.is_empty() {
                    self.full_cache.remove(&self.current_index);
                    self.thumb_cache.remove(&self.current_index);
                    self.navigate_to(self.current_index, ctx);
                }
                self.show_success("Folder up to date");
            }
            return;
        }

        let old_files = std::mem::replace(&mut self.files, new_files);
        let active_path = old_files.get(self.current_index).cloned();

        self.reconcile_caches(&old_files);

        if self.files.is_empty() {
            self.image = None;
            self.preview = None;
            self.animation = None;
            self.crop_state = None;
            self.current_index = 0;
            self.config.last_file_path = None;
            self.empty_folder_since = Some(std::time::Instant::now());
            self.reset_viewport();
            if manual {
                self.show_success("Folder is empty");
            }
            return;
        }

        if let Some(path) = &active_path {
            if let Some(new_idx) = self.files.iter().position(|p| p == path) {
                // Active image still exists in the folder: preserve its canvas/zoom/pan/rotation!
                self.current_index = new_idx;
                self.loader.set_current_index(new_idx);
                self.config.last_file_path = Some(path.clone());
            } else {
                // Active image was deleted/moved outside: advance to nearest neighbor
                let next_idx = self.current_index.min(self.files.len() - 1);
                self.navigate_to(next_idx, ctx);
            }
        } else {
            // Folder previously empty, now has images: open first image
            self.empty_folder_since = None;
            self.navigate_to(0, ctx);
        }

        if manual {
            self.show_success(format!("Folder refreshed ({} images)", self.files.len()));
        } else {
            let old_count = old_files.len();
            let new_count = self.files.len();
            if new_count > old_count {
                let diff = new_count - old_count;
                self.show_success(format!("+{} image{} added ({} total)", diff, if diff == 1 { "" } else { "s" }, new_count));
            } else if new_count < old_count {
                let diff = old_count - new_count;
                self.show_status(format!("{} image{} removed ({} total)", diff, if diff == 1 { "" } else { "s" }, new_count));
            }
        }

        if let Some(dir) = self.current_folder() {
            self.last_folder_mtime = std::fs::metadata(&dir).and_then(|m| m.modified()).ok();
        }

        ctx.request_repaint();
    }

    /// Push current folder to history stack before switching to `dir`.
    pub(crate) fn push_folder_history(&mut self, dir: &Path) {
        if let Some(cur) = self.current_folder() {
            if cur != dir {
                if self.folder_back_stack.last().map(|p| p.as_path()) != Some(&cur) {
                    self.folder_back_stack.push(cur);
                    if self.folder_back_stack.len() > 50 {
                        self.folder_back_stack.remove(0);
                    }
                }
                self.folder_forward_stack.clear();
            }
        }
    }

    pub(crate) fn open_file(&mut self, path: PathBuf, ctx: &egui::Context) {
        if !filesystem::is_supported_path(&path) {
            self.image = None;
            self.preview = None;
            self.animation = None;
            self.crop_state = None;
            self.error = Some(format!("Unsupported file: {}", path.display()));
            return;
        }

        self.clear_caches();
        self.crop_state = None;
        self.refresh_rx = None;
        self.is_refreshing = false;
        self.empty_folder_since = None;
        match path.parent() {
            Some(dir) => {
                self.push_folder_history(dir);
                self.opened_folder = Some(dir.to_path_buf());
                if self.config.remember_last_folder {
                    self.config.last_directory = Some(dir.to_path_buf());
                } else {
                    self.config.last_directory = None;
                    self.config.last_file_path = None;
                }
                self.save_config();
                self.files = filesystem::scan_directory_sorted(
                    dir,
                    self.config.sort_criteria,
                    self.config.sort_direction,
                );
                self.last_folder_mtime = dir.metadata().and_then(|m| m.modified()).ok();
                self.last_folder_check = std::time::Instant::now();
                self.grid_path_buffer = dir.display().to_string();
                self.grid_path_editing = false;
                self.grid_path_invalid = false;
                self.refresh_multiview_subfolders(ctx);
                let index = self.files.iter().position(|p| *p == path).unwrap_or(0);
                self.navigate_to(index, ctx);
            }
            None => {
                self.files = vec![path];
                self.navigate_to(0, ctx);
            }
        }
    }

    /// Open `dir` as the active folder, recording history for Back/Forward navigation.
    pub(crate) fn open_directory(&mut self, dir: PathBuf, ctx: &egui::Context) {
        self.push_folder_history(&dir);
        self.open_directory_internal(dir, ctx);
    }

    /// Low-level folder opening without mutating the back/forward history stacks.
    pub(crate) fn open_directory_internal(&mut self, dir: PathBuf, ctx: &egui::Context) {
        self.opened_folder = Some(dir.clone());
        if self.config.remember_last_folder {
            self.config.last_directory = Some(dir.clone());
        } else {
            self.config.last_directory = None;
            self.config.last_file_path = None;
        }
        self.save_config();

        self.clear_caches();
        self.crop_state = None;
        self.refresh_rx = None;
        self.is_refreshing = false;
        self.files = filesystem::scan_directory_sorted(
            &dir,
            self.config.sort_criteria,
            self.config.sort_direction,
        );
        self.last_folder_mtime = dir.metadata().and_then(|m| m.modified()).ok();
        self.last_folder_check = std::time::Instant::now();
        self.scroll_to_selected = true;
        self.grid_path_buffer = dir.display().to_string();
        self.grid_path_editing = false;
        self.grid_path_invalid = false;
        self.refresh_multiview_subfolders(ctx);
        if self.files.is_empty() {
            self.image = None;
            self.error = None;
            self.empty_folder_since = Some(std::time::Instant::now());
            self.current_index = 0;
            self.config.last_file_path = None;
            self.reset_viewport();
        } else {
            self.empty_folder_since = None;
            self.current_index = 0;
            self.navigate_to(0, ctx);
        }
    }

    /// Navigate back to the previous folder in history.
    pub(crate) fn navigate_folder_back(&mut self, ctx: &egui::Context) -> bool {
        while let Some(prev) = self.folder_back_stack.pop() {
            if prev.is_dir() {
                if let Some(cur) = self.current_folder() {
                    if self.folder_forward_stack.last().map(|p| p.as_path()) != Some(&cur) {
                        self.folder_forward_stack.push(cur);
                        if self.folder_forward_stack.len() > 50 {
                            self.folder_forward_stack.remove(0);
                        }
                    }
                }
                let name = prev
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| prev.display().to_string());
                self.open_directory_internal(prev, ctx);
                let count = self.files.len();
                self.show_success(format!(
                    "{} Back to \"{}\" ({} image{})",
                    icons::ICON_IMAGE,
                    name,
                    count,
                    if count == 1 { "" } else { "s" }
                ));
                return true;
            }
        }
        false
    }

    /// Navigate forward to the next folder in history.
    pub(crate) fn navigate_folder_forward(&mut self, ctx: &egui::Context) -> bool {
        while let Some(next) = self.folder_forward_stack.pop() {
            if next.is_dir() {
                if let Some(cur) = self.current_folder() {
                    if self.folder_back_stack.last().map(|p| p.as_path()) != Some(&cur) {
                        self.folder_back_stack.push(cur);
                        if self.folder_back_stack.len() > 50 {
                            self.folder_back_stack.remove(0);
                        }
                    }
                }
                let name = next
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| next.display().to_string());
                self.open_directory_internal(next, ctx);
                let count = self.files.len();
                self.show_success(format!(
                    "{} Forward to \"{}\" ({} image{})",
                    icons::ICON_IMAGE,
                    name,
                    count,
                    if count == 1 { "" } else { "s" }
                ));
                return true;
            }
        }
        false
    }

    /// Update the file sort order (criteria or direction) and re-sort the active folder in place.
    pub(crate) fn set_sort_order(
        &mut self,
        criteria: config::FileSortCriteria,
        direction: config::SortDirection,
        _ctx: &egui::Context,
    ) {
        if self.config.sort_criteria == criteria && self.config.sort_direction == direction {
            return;
        }
        self.config.sort_criteria = criteria;
        self.config.sort_direction = direction;
        self.save_config();

        if self.files.is_empty() {
            return;
        }

        let old_files = self.files.clone();
        let active_path = self.current_file().cloned();

        filesystem::sort_files(&mut self.files, criteria, direction);
        self.reconcile_caches(&old_files);

        if let Some(path) = &active_path {
            if let Some(new_idx) = self.files.iter().position(|p| p == path) {
                self.current_index = new_idx;
                self.loader.set_current_index(new_idx);
            }
        }

        if self.view_mode == ViewMode::Grid {
            self.scroll_to_selected = true;
        }

        let dir_str = match direction {
            config::SortDirection::Ascending => "Ascending",
            config::SortDirection::Descending => "Descending",
        };
        self.show_success(format!("Sorted by {} ({})", criteria.label(), dir_str));
    }

    /// Toggle sort direction between Ascending and Descending for the active criteria.
    #[allow(dead_code)]
    pub(crate) fn toggle_sort_direction(&mut self, ctx: &egui::Context) {
        let new_dir = self.config.sort_direction.toggle();
        self.set_sort_order(self.config.sort_criteria, new_dir, ctx);
    }

    pub(crate) fn ingest_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone()) // path is Option<PathBuf>
                .collect()
        });

        // Open the first valid drop: a directory is scanned as a folder, a file
        // is loaded as an image. Missing/special paths are skipped.
        for path in dropped {
            if path.is_dir() {
                self.open_directory(path, ctx);
                return;
            } else if path.is_file() {
                self.open_file(path, ctx);
                return;
            }
        }
    }

    // ------------------------------------------------------------------
    // Closing (session management, no disk change)
    // ------------------------------------------------------------------

    pub(crate) fn close_current_file(&mut self, ctx: &egui::Context) {
        if self.files.is_empty() {
            return;
        }
        self.context_menu_pos = None;
        self.files.remove(self.current_index);
        self.advance_after_removal(ctx);
        self.show_success("File closed");
    }

    pub(crate) fn close_folder(&mut self) {
        self.context_menu_pos = None;
        self.files.clear();
        self.opened_folder = None;
        self.image = None;
        self.crop_state = None;
        self.error = None;
        self.empty_folder_since = None;
        self.current_index = 0;
        self.reset_viewport();
        self.clear_caches();
        self.refresh_rx = None;
        self.is_refreshing = false;
        self.grid_path_editing = false;
        self.grid_path_invalid = false;
        self.grid_path_buffer.clear();
        self.folder_back_stack.clear();
        self.folder_forward_stack.clear();
        // Forget the saved paths so a restart boots into the clean empty state.
        self.config.last_file_path = None;
        self.config.last_directory = None;
        self.save_config();
        self.show_success("Folder closed");
    }

    pub(crate) fn advance_after_removal(&mut self, ctx: &egui::Context) {
        self.clear_caches();
        let len = self.files.len();
        if len == 0 {
            self.image = None;
            self.error = None;
            self.current_index = 0;
            self.crop_state = None;
            self.reset_viewport();
        } else {
            let next = self.current_index.min(len - 1);
            self.navigate_to(next, ctx);
        }
    }

    // ------------------------------------------------------------------
    // Dialogs & shell actions
    // ------------------------------------------------------------------

    pub(crate) fn open_file_dialog(&mut self, ctx: &egui::Context) {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Open Image")
            .add_filter(
                "Images",
                &["png", "jpg", "jpeg", "jfif", "webp", "bmp", "gif", "ico"],
            );
        if let Some(dir) = &self.config.last_directory {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.open_file(path, ctx);
        }
    }

    pub(crate) fn open_folder_dialog(&mut self) {
        self.open_folder_browser(PendingOperation::OpenFolder);
    }

    /// Open the custom folder browser for a given operation.
    pub(crate) fn open_folder_browser(&mut self, op: PendingOperation) {
        let remembered = match op {
            // Start the "Open Folder" browser at the folder currently being
            // browsed (kept in sync by Multi-View folder navigation), falling
            // back to the last folder picked through the browser itself.
            PendingOperation::OpenFolder => self
                .current_folder()
                .or_else(|| self.config.last_open_folder.clone()),
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
    pub(crate) fn render_folder_browser(&mut self, ctx: &egui::Context) {
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
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.display().to_string());
                        self.open_directory(path, ctx);
                        let count = self.files.len();
                        self.show_status(format!(
                            "{} {} | {} {}",
                            icons::ICON_FOLDER,
                            name,
                            icons::ICON_IMAGE,
                            count
                        ));
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
            viewer::folder_browser::BrowserResult::Pin(path) => {
                if !self.config.pinned_folders.contains(&path) {
                    self.config.pinned_folders.push(path);
                    self.save_config();
                }
                if let Some(browser) = self.folder_browser.as_mut() {
                    browser.pinned = self.config.pinned_folders.clone();
                }
            }
            viewer::folder_browser::BrowserResult::Unpin(path) => {
                self.config.pinned_folders.retain(|p| p != &path);
                self.save_config();
                if let Some(browser) = self.folder_browser.as_mut() {
                    browser.pinned = self.config.pinned_folders.clone();
                }
            }
        }
    }

    pub(crate) fn copy_current_path(&mut self, ctx: &egui::Context) {
        if let Some(path) = self.current_file() {
            ctx.copy_text(path.display().to_string());
            self.show_success("Copied path to clipboard");
        }
    }

    pub(crate) fn copy_current_file(&mut self) {
        let Some(path) = self.current_file().cloned() else {
            return;
        };
        match platform::copy_file_to_clipboard(&path) {
            Ok(()) => self.show_success("File copied"),
            Err(e) => self.show_status(format!("Copy file failed: {e}")),
        }
    }

    pub(crate) fn copy_current_image(&mut self) {
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
    pub(crate) fn save_current_as(&mut self) {
        let Some(image) = &self.image else {
            self.show_status("No image to save");
            return;
        };
        let rgba = image.rgba_bytes();
        let [w, h] = image.display_dimensions();

        let mut dialog = rfd::FileDialog::new()
            .set_title("Save Image As")
            .add_filter("PNG", &["png"])
            .add_filter("JPEG", &["jpg", "jpeg", "jfif"])
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
    pub(crate) fn print_current(&mut self) {
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
    pub(crate) fn refresh_registration(&mut self) {
        match platform::update_registration() {
            Ok(()) => {
                self.registry_status = Some(platform::check_registration_status());
                self.show_success("Windows registration updated to current path");
            }
            Err(e) => self.show_status(format!("Registration failed: {e}")),
        }
    }

    /// Toggle the Explorer folder context-menu registration and toast.
    pub(crate) fn toggle_folder_context_menu(&mut self) {
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
    pub(crate) fn execute_pill_action(&mut self, action: PillAction, ctx: &egui::Context) {
        match action {
            PillAction::OpenFile => self.open_file_dialog(ctx),
            PillAction::OpenFolder => self.open_folder_dialog(),
            PillAction::Crop => self.toggle_crop_mode(),
            PillAction::SaveAs => self.save_current_as(),
            PillAction::SaveRotation => self.save_rotation_overwrite(ctx),
            PillAction::Print => self.print_current(),
            PillAction::ToggleFilmstrip => {
                self.config.show_filmstrip = !self.config.show_filmstrip;
                self.save_config();
            }
        }
    }

    pub(crate) fn set_wallpaper_current(&mut self) {
        let Some(path) = self.current_file().cloned() else {
            return;
        };
        match platform::set_wallpaper(&path) {
            Ok(()) => self.show_success("Wallpaper set"),
            Err(e) => self.show_status(format!("Wallpaper failed: {e}")),
        }
    }

    pub(crate) fn reveal_current(&self) {
        if let Some(path) = self.current_file() {
            platform::reveal_in_explorer(path);
        } else if let Some(dir) = self.current_folder() {
            // No image is selected (e.g. browsing an empty folder in Multi-View):
            // open the active folder itself in Explorer instead of doing nothing.
            platform::open_folder(&dir);
        }
    }

    pub(crate) fn open_with_current(&self) {
        if let Some(path) = self.current_file() {
            platform::open_with(path);
        }
    }

    pub(crate) fn delete_current(&mut self, ctx: &egui::Context) {
        let Some(path) = self.current_file().cloned() else {
            return;
        };
        self.context_menu_pos = None;
        self.crop_state = None;

        match trash::delete(&path) {
            Ok(()) => {
                self.show_status(format!("{} Moved to Recycle Bin", icons::ICON_TRASH));
                self.files.remove(self.current_index);
                self.advance_after_removal(ctx);
            }
            Err(e) => self.show_status(format!("Delete failed: {e}")),
        }
    }

    /// Open the rename dialog for the active or selected file.
    pub(crate) fn open_rename_dialog(&mut self) {
        let Some(path) = self.current_file().cloned() else {
            return;
        };
        if !path.is_file() {
            self.show_status(format!("{} File not found", icons::ICON_WARN));
            return;
        }
        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if filename.is_empty() {
            return;
        }
        self.context_menu_pos = None;
        self.show_help = false;
        self.show_about = false;
        self.show_image_info = false;
        self.show_resize_dialog = false;
        self.rename_state = Some(RenameState {
            old_path: path,
            new_name: filename,
            focus_pending: true,
        });
    }

    /// Commit the rename operation on disk and update internal lists and caches.
    pub(crate) fn commit_rename(&mut self, new_name: &str, ctx: &egui::Context) {
        let Some(state) = self.rename_state.take() else {
            return;
        };
        let old_path = state.old_path;
        let trimmed = new_name.trim();
        let orig_name = old_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        if trimmed == orig_name {
            return;
        }

        let new_path = old_path.with_file_name(trimmed);

        match safe_rename(&old_path, &new_path) {
            Ok(()) => {
                let old_files = self.files.clone();
                let old_pos = self.files.iter().position(|p| p == &old_path);

                if filesystem::is_supported_path(&new_path) {
                    if let Some(pos) = old_pos {
                        self.files[pos] = new_path.clone();
                    } else {
                        self.files.push(new_path.clone());
                    }
                    filesystem::sort_naturally(&mut self.files);
                    let new_idx = self
                        .files
                        .iter()
                        .position(|p| p == &new_path)
                        .unwrap_or(0);

                    self.reconcile_caches_with_rename(
                        &old_files,
                        old_pos.map(|op| (op, new_idx)),
                    );

                    self.current_index = new_idx;
                    self.loader.set_current_index(new_idx);
                    self.config.last_file_path = Some(new_path);
                    if self.view_mode == ViewMode::Grid {
                        self.scroll_to_selected = true;
                    }
                } else {
                    if let Some(pos) = old_pos {
                        self.files.remove(pos);
                    }
                    self.reconcile_caches(&old_files);

                    if self.files.is_empty() {
                        self.image = None;
                        self.preview = None;
                        self.animation = None;
                        self.current_index = 0;
                        self.config.last_file_path = None;
                        self.empty_folder_since = Some(std::time::Instant::now());
                        self.reset_viewport();
                    } else {
                        let next_idx = self.current_index.min(self.files.len() - 1);
                        self.navigate_to(next_idx, ctx);
                    }
                }

                self.show_success(format!("Renamed to {trimmed}"));
                ctx.request_repaint();
            }
            Err(e) => {
                self.show_status(format!("Rename failed: {e}"));
                ctx.request_repaint();
            }
        }
    }

    /// Open the resize dialog, seeded from the current image's dimensions.
    pub(crate) fn open_resize_dialog(&mut self) {
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
        self.crop_state = None;
    }

    /// Decode, rotate, resize, and save the current image; then reload it.
    pub(crate) fn resize_save(&mut self, overwrite: bool, ctx: &egui::Context) {
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


    /// Save the in-memory manual rotation directly back to the active file (overwrite).
    pub(crate) fn save_rotation_overwrite(&mut self, ctx: &egui::Context) {
        if self.rotation == 0 {
            return;
        }
        let Some(path) = self.current_file().cloned() else {
            return;
        };

        let result = (|| -> Result<(), String> {
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            let img = viewer::decode(&bytes)?;
            let rotated = viewer::resize::rotate(&img, self.rotation);
            rotated.save(&path).map_err(|e| e.to_string())
        })();

        match result {
            Ok(()) => {
                self.rotation = 0;
                self.show_success("Rotation saved (file overwritten)");
                self.full_cache.remove(&self.current_index);
                self.thumb_cache.remove(&self.current_index);
                self.pending_full.remove(&self.current_index);
                self.pending_thumb.remove(&self.current_index);
                self.loader.bump_generation();
                self.navigate_to(self.current_index, ctx);
            }
            Err(e) => self.show_status(format!("Save rotation failed: {e}")),
        }
    }

    /// Open the interactive crop tool on the active image.
    pub(crate) fn open_crop_mode(&mut self) {
        if self.view_mode != ViewMode::Viewer {
            return;
        }
        let Some(dims) = self.image.as_ref().map(|i| i.dimensions) else {
            return;
        };
        self.show_resize_dialog = false;
        self.show_settings = false;
        self.show_help = false;
        self.show_about = false;
        self.show_image_info = false;
        self.context_menu_pos = None;
        self.rename_state = None;
        self.crop_state = Some(crop::CropState::new(dims[0], dims[1]));
        self.reset_viewport();
    }

    /// Close the crop mode without changes.
    pub(crate) fn close_crop_mode(&mut self) {
        self.crop_state = None;
    }

    /// Toggle crop mode on/off.
    pub(crate) fn toggle_crop_mode(&mut self) {
        if self.crop_state.is_some() {
            self.close_crop_mode();
        } else {
            self.open_crop_mode();
        }
    }

    /// Calculate the pixel coordinates `(x, y, width, height)` of the active crop box.
    pub(crate) fn get_crop_pixel_rect(&self) -> (u32, u32, u32, u32) {
        let Some(ref crop) = self.crop_state else {
            return (0, 0, 0, 0);
        };
        let Some(dims) = self.image.as_ref().map(|i| i.dimensions) else {
            return (0, 0, 0, 0);
        };
        let orig_w = dims[0].max(1);
        let orig_h = dims[1].max(1);
        let x = (crop.norm_rect[0] * orig_w as f32).round().max(0.0) as u32;
        let y = (crop.norm_rect[1] * orig_h as f32).round().max(0.0) as u32;
        let w = ((crop.norm_rect[2] - crop.norm_rect[0]) * orig_w as f32)
            .round()
            .max(1.0) as u32;
        let h = ((crop.norm_rect[3] - crop.norm_rect[1]) * orig_h as f32)
            .round()
            .max(1.0) as u32;
        let x = x.min((orig_w as u32).saturating_sub(1));
        let y = y.min((orig_h as u32).saturating_sub(1));
        let w = w.min((orig_w as u32).saturating_sub(x)).max(1);
        let h = h.min((orig_h as u32).saturating_sub(y)).max(1);
        (x, y, w, h)
    }

    /// Apply the active crop in-memory to the currently displayed texture.
    pub(crate) fn apply_crop_in_memory(&mut self, ctx: &egui::Context) {
        let (x, y, w, h) = self.get_crop_pixel_rect();
        if w == 0 || h == 0 {
            return;
        }
        if let Some(image) = self.image.as_mut() {
            image.crop_in_place(ctx, x, y, w, h);
            self.crop_state = None;
            self.reset_viewport();
            self.show_success(format!("Crop applied ({w} × {h} px). Save with Ctrl+S"));
        }
    }

    /// Save the cropped image directly back to the active file (lossless overwrite).
    pub(crate) fn save_crop_overwrite(&mut self, ctx: &egui::Context) {
        let Some(path) = self.current_file().cloned() else {
            return;
        };
        let (x, y, w, h) = self.get_crop_pixel_rect();
        if w == 0 || h == 0 {
            return;
        }

        let rotation = self.rotation;
        let result = (|| -> Result<(), String> {
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            let img = viewer::decode(&bytes)?;
            let img = if rotation != 0 {
                viewer::resize::rotate(&img, rotation)
            } else {
                img
            };
            let cropped = viewer::resize::crop_exact(&img, x, y, w, h);
            cropped.save(&path).map_err(|e| e.to_string())
        })();

        match result {
            Ok(()) => {
                self.rotation = 0;
                self.crop_state = None;
                self.show_success(format!("Cropped and saved ({w} × {h} px)"));
                self.full_cache.remove(&self.current_index);
                self.thumb_cache.remove(&self.current_index);
                self.pending_full.remove(&self.current_index);
                self.pending_thumb.remove(&self.current_index);
                self.loader.bump_generation();
                self.navigate_to(self.current_index, ctx);
            }
            Err(e) => self.show_status(format!("Save crop failed: {e}")),
        }
    }

    /// Save the cropped image as a new file copy via a native save file dialog.
    pub(crate) fn save_crop_as_copy(&mut self, ctx: &egui::Context) {
        let Some(path) = self.current_file().cloned() else {
            return;
        };
        let (x, y, w, h) = self.get_crop_pixel_rect();
        if w == 0 || h == 0 {
            return;
        }

        let default_name = crop::default_crop_name(&path);
        let mut dialog = rfd::FileDialog::new()
            .set_title("Save Cropped Image As")
            .set_file_name(&default_name);
        if let Some(dir) = path.parent() {
            dialog = dialog.set_directory(dir);
        }
        let Some(target) = dialog.save_file() else {
            return;
        };

        let rotation = self.rotation;
        let result = (|| -> Result<(), String> {
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            let img = viewer::decode(&bytes)?;
            let img = if rotation != 0 {
                viewer::resize::rotate(&img, rotation)
            } else {
                img
            };
            let cropped = viewer::resize::crop_exact(&img, x, y, w, h);
            cropped.save(&target).map_err(|e| e.to_string())
        })();

        match result {
            Ok(()) => {
                self.crop_state = None;
                self.show_success("Cropped image saved as copy");
                self.open_file(target, ctx);
            }
            Err(e) => self.show_status(format!("Save cropped copy failed: {e}")),
        }
    }

    /// Copy the cropped image region directly to the system clipboard.
    pub(crate) fn copy_crop_to_clipboard(&mut self) {
        let (x, y, w, h) = self.get_crop_pixel_rect();
        if w == 0 || h == 0 {
            return;
        }
        let Some(image) = self.image.as_ref() else {
            return;
        };
        let [disp_w, disp_h] = image.display_dimensions();
        let Some(img) = image::RgbaImage::from_raw(
            disp_w as u32,
            disp_h as u32,
            image.rgba_bytes(),
        ) else {
            self.show_status("Clipboard: image buffer unavailable");
            return;
        };
        let iw = img.width();
        let ih = img.height();
        if iw == 0 || ih == 0 {
            return;
        }
        let cx = x.min(iw.saturating_sub(1));
        let cy = y.min(ih.saturating_sub(1));
        let cw = w.min(iw - cx).max(1);
        let ch = h.min(ih - cy).max(1);
        let cropped = image::imageops::crop_imm(&img, cx, cy, cw, ch).to_image();
        let (cw, ch) = (cropped.width() as usize, cropped.height() as usize);
        let raw_bytes = cropped.into_raw();

        let res = (|| -> Result<(), String> {
            let mut clipboard =
                arboard::Clipboard::new().map_err(|e| format!("Clipboard unavailable: {e}"))?;
            clipboard
                .set_image(arboard::ImageData {
                    width: cw,
                    height: ch,
                    bytes: std::borrow::Cow::Borrowed(&raw_bytes),
                })
                .map_err(|e| format!("Copy to clipboard failed: {e}"))
        })();

        match res {
            Ok(()) => self.show_success(format!("Copied cropped image ({cw} × {ch} px) to clipboard")),
            Err(e) => self.show_status(e),
        }
    }

    /// "Copy File to...": copy to the remembered folder, or browse on Ctrl.
    pub(crate) fn copy_file_to(&mut self, ctx: &egui::Context) {
        if self.current_file().is_none() {
            return;
        }
        let ctrl = ctx.input(|i| i.modifiers.ctrl);
        let valid_dest = self
            .config
            .last_copy_folder
            .as_ref()
            .map(|d| d.is_dir())
            .unwrap_or(false);

        if ctrl || !valid_dest {
            if !ctrl && self.config.last_copy_folder.is_some() {
                self.config.last_copy_folder = None;
                self.save_config();
                self.show_status("Previous copy destination was moved or deleted; please choose a folder");
            }
            self.open_folder_browser(PendingOperation::CopyFileTo);
        } else if let Some(dir) = self.config.last_copy_folder.clone() {
            if let Some(src) = self.current_file().cloned() {
                self.do_copy(src, dir);
            }
        }
    }

    pub(crate) fn do_copy(&mut self, src: PathBuf, dir: PathBuf) {
        let Some(name) = src.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            return;
        };
        if !dir.is_dir() {
            if let Err(e) = std::fs::create_dir_all(&dir) {
                self.show_status(format!("Destination folder missing and cannot be created: {e}"));
                return;
            }
        }
        let dest = unique_dest(&dir, &name);
        match std::fs::copy(&src, &dest) {
            Ok(_) => self.show_success(format!("Copied to {}", dir.display())),
            Err(e) => self.show_status(format!("Copy failed: {e}")),
        }
    }

    /// "Move File to...": move to the remembered folder, or browse on Ctrl.
    pub(crate) fn move_file_to(&mut self, ctx: &egui::Context) {
        if self.current_file().is_none() {
            return;
        }
        let ctrl = ctx.input(|i| i.modifiers.ctrl);
        let valid_dest = self
            .config
            .last_move_folder
            .as_ref()
            .map(|d| d.is_dir())
            .unwrap_or(false);

        if ctrl || !valid_dest {
            if !ctrl && self.config.last_move_folder.is_some() {
                self.config.last_move_folder = None;
                self.save_config();
                self.show_status("Previous move destination was moved or deleted; please choose a folder");
            }
            self.open_folder_browser(PendingOperation::MoveFileTo);
        } else if let Some(dir) = self.config.last_move_folder.clone() {
            if let Some(src) = self.current_file().cloned() {
                self.do_move(src, dir, ctx);
            }
        }
    }

    pub(crate) fn do_move(&mut self, src: PathBuf, dir: PathBuf, ctx: &egui::Context) {
        let Some(name) = src.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            return;
        };
        if !dir.is_dir() {
            if let Err(e) = std::fs::create_dir_all(&dir) {
                self.show_status(format!("Destination folder missing and cannot be created: {e}"));
                return;
            }
        }
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

    pub(crate) fn reset_viewport(&mut self) {
        self.zoom = None;
        self.pan = egui::Vec2::ZERO;
    }

    pub(crate) fn effective_zoom(&self, dimensions: [usize; 2], available: egui::Vec2) -> f32 {
        self.zoom
            .unwrap_or_else(|| viewer::fit_scale(dimensions, available))
    }

    pub(crate) fn rotate_current_cw(&mut self, ctx: &egui::Context) {
        self.rotation = (self.rotation + 90) % 360;
        if let Some(image) = &mut self.image {
            image.rotate_90_cw(ctx);
        }
    }

    pub(crate) fn rotate_current_ccw(&mut self, ctx: &egui::Context) {
        self.rotation = (self.rotation + 270) % 360;
        if let Some(image) = &mut self.image {
            image.rotate_90_ccw(ctx);
        }
    }

    // ------------------------------------------------------------------
    // Grid tile sizing
    // ------------------------------------------------------------------

    /// Minimum allowed grid preview container width in logical points.
    pub const MIN_GRID_TILE_SIZE: f32 = 80.0;
    /// Maximum allowed grid preview container width in logical points.
    pub const MAX_GRID_TILE_SIZE: f32 = 500.0;
    /// Default grid preview container width.
    pub const DEFAULT_GRID_TILE_SIZE: f32 = 150.0;

    /// The current grid tile width, clamped to the valid range.
    pub(crate) fn grid_tile_width(&self) -> f32 {
        self.config
            .grid_tile_size
            .clamp(Self::MIN_GRID_TILE_SIZE, Self::MAX_GRID_TILE_SIZE)
    }

    /// The current grid tile height: thumbnail area plus standard label margin.
    pub(crate) fn grid_tile_height(&self) -> f32 {
        self.grid_tile_width() + 26.0
    }

    /// Adjust the grid tile size by `delta` points (positive = grow), persist,
    /// show a transient status pill, and anchor the scroll viewport so the target
    /// item stays fixed on screen rather than jumping or scrolling the background.
    pub(crate) fn adjust_grid_tile_size(&mut self, delta: f32, ctx: &egui::Context) {
        if !self.config.adjustable_grid_tile_size {
            return;
        }
        let old_tile_w = self.grid_tile_width();
        let new_tile_w = (old_tile_w + delta).clamp(Self::MIN_GRID_TILE_SIZE, Self::MAX_GRID_TILE_SIZE);
        if (new_tile_w - old_tile_w).abs() > 0.5 {
            // Anchor scroll position around the target item (hovered item or current_index)
            let scroll_id = egui::Id::new("grid-scroll");
            if let Some(mut state) = egui::scroll_area::State::load(ctx, scroll_id) {
                let target_index = self.grid_hovered_index.unwrap_or(self.current_index);
                if target_index < self.files.len() {
                    let old_columns = self.grid_columns.max(1);
                    let old_row = target_index / old_columns;
                    let old_row_h = self.grid_tile_height();
                    let spacing_y = self.grid_spacing_y;
                    let old_target_y = old_row as f32 * (old_row_h + spacing_y);
                    let viewport_offset = old_target_y - state.offset.y;

                    let avail_w = self.grid_available_width.max(100.0);
                    let new_columns = ((avail_w - 12.0) / new_tile_w).floor().max(1.0) as usize;
                    let new_row = target_index / new_columns;
                    let new_row_h = new_tile_w + 26.0;
                    let new_target_y = new_row as f32 * (new_row_h + spacing_y);

                    state.offset.y = (new_target_y - viewport_offset).max(0.0);
                    state.store(ctx, scroll_id);
                }
            }
            self.config.grid_tile_size = new_tile_w;
            self.save_config();
            self.show_status(format!("Preview size: {:.0}px", new_tile_w));
        }
    }

    /// Reset the grid tile size to the default, persist, and anchor viewport.
    pub(crate) fn reset_grid_tile_size(&mut self, ctx: &egui::Context) {
        if !self.config.adjustable_grid_tile_size {
            return;
        }
        let delta = Self::DEFAULT_GRID_TILE_SIZE - self.grid_tile_width();
        self.adjust_grid_tile_size(delta, ctx);
        self.show_success("Preview size reset to default");
    }

    // ------------------------------------------------------------------
    // Input
    // ------------------------------------------------------------------

    pub(crate) fn capture_input(ctx: &egui::Context) -> InputSnapshot {
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
            primary_pressed: i.pointer.primary_pressed(),
            has_hovered_files: !i.raw.hovered_files.is_empty(),
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
            rotate: i.key_pressed(egui::Key::R) && !i.modifiers.ctrl && !i.modifiers.shift,
            rotate_ccw: i.key_pressed(egui::Key::R) && i.modifiers.ctrl && !i.modifiers.shift,
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
            save: i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(egui::Key::S),
            save_as: i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::S),
            print: i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(egui::Key::P),
            toggle_image_info: i.key_pressed(egui::Key::I),
            resize_image: i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::R),
            crop: !i.modifiers.ctrl && !i.modifiers.shift && !i.modifiers.alt && i.key_pressed(egui::Key::C),
            refresh_folder: i.key_pressed(egui::Key::F5),
            rename: i.key_pressed(egui::Key::F2),
            shift: i.modifiers.shift,
        })
    }

    pub(crate) fn apply_hotkeys(&mut self, ctx: &egui::Context, input: &InputSnapshot) {
        // Viewport hotkeys only apply in Viewer mode, and not while a modal or
        // the context menu is open.
        if self.modal_open() || self.view_mode != ViewMode::Viewer {
            return;
        }
        if self.crop_state.is_some() {
            if input.rotate {
                if let Some(ref mut c) = self.crop_state {
                    if let Some(dims) = self.image.as_ref().map(|i| i.dimensions) {
                        c.reset_full(dims[0], dims[1]);
                    }
                }
            }
            return;
        }
        if input.rotate {
            self.rotate_current_cw(ctx);
        }
        if input.rotate_ccw {
            self.rotate_current_ccw(ctx);
        }
        if input.reset_fit {
            self.reset_viewport();
        }
        if input.reset_100 {
            self.zoom = Some(1.0);
            self.pan = egui::Vec2::ZERO;
        }
        if self.view_mode == ViewMode::Viewer && !self.modal_open() {
            if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft)) {
                self.navigate_folder_back(ctx);
            }
            if ctx.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight)) {
                self.navigate_folder_forward(ctx);
            }
        }
        if input.double_clicked && self.view_mode == ViewMode::Viewer {
            let over_image = self
                .image_rect
                .map_or(false, |r| input.pointer_pos.map_or(false, |p| r.contains(p)));
            if over_image {
                self.zoom = if self.zoom.is_none() { Some(1.0) } else { None };
                self.pan = egui::Vec2::ZERO;
            } else if !self.files.is_empty() || self.current_folder().is_some() {
                // Double-clicking the empty canvas opens the grid selector.
                self.crop_state = None;
                self.view_mode = ViewMode::Grid;
                self.scroll_to_selected = true;
                self.refresh_multiview_subfolders(ctx);
                self.trigger_folder_refresh(ctx, false);
            }
        }
    }

    /// Handle animation playback input: Space toggles play/pause; `[` / `]`
    /// step a frame (useful while paused).
    pub(crate) fn handle_animation_input(&mut self, input: &InputSnapshot) {
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

    pub(crate) fn toggle_fullscreen(ctx: &egui::Context) {
        let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
    }

    // ------------------------------------------------------------------
    // Modals (mutually exclusive)
    // ------------------------------------------------------------------

    pub(crate) fn open_help(&mut self) {
        self.show_help = true;
        self.show_about = false;
        self.show_settings = false;
        self.show_image_info = false;
        self.show_resize_dialog = false;
        self.close_folder_browser();
        self.context_menu_pos = None;
        self.help_opened_this_frame = true;
    }

    pub(crate) fn open_about(&mut self) {
        self.show_about = true;
        self.show_help = false;
        self.show_settings = false;
        self.show_image_info = false;
        self.show_resize_dialog = false;
        self.close_folder_browser();
        self.context_menu_pos = None;
    }

    pub(crate) fn open_settings(&mut self) {
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
    pub(crate) fn close_folder_browser(&mut self) {
        self.show_folder_browser = false;
        self.folder_browser = None;
        self.pending_operation = None;
    }

    /// Start the async registry inspector if it hasn't run yet and isn't
    /// already running. The result is delivered back through `registry_rx`.
    pub(crate) fn start_registry_check(&mut self) {
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
    pub(crate) fn poll_registry(&mut self) {
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
    pub(crate) fn unregister_all(&mut self) {
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
    pub(crate) fn modal_open(&self) -> bool {
        self.show_settings
            || self.show_help
            || self.show_about
            || self.show_image_info
            || self.show_resize_dialog
            || self.show_folder_browser
            || self.rename_state.is_some()
            || self.context_menu_pos.is_some()
    }

    // ------------------------------------------------------------------
    // Persistence & notifications
    // ------------------------------------------------------------------

    pub(crate) fn save_config(&mut self) {
        if !self.config.remember_last_folder {
            self.config.last_directory = None;
            self.config.last_file_path = None;
        }
        if let Some(pos) = self.window_pos {
            self.config.window_pos = Some(pos);
        } else if let Some(rect) = self.window_rect {
            self.config.window_pos = Some([rect.min.x, rect.min.y]);
        }
        if let Some(size) = self.window_size {
            self.config.window_size = Some(size);
        } else if let Some(rect) = self.window_rect {
            self.config.window_size = Some([rect.width(), rect.height()]);
        }
        self.config.quick_pill_anchor = self.custom_pill_anchor;
        self.config.quick_pill_pos = self.custom_pill_anchor.map(|a| [a.offset_x, a.offset_y]);
        self.config.save();
    }

    pub(crate) fn show_status(&mut self, message: impl Into<String>) {
        self.feedback = Some((message.into(), std::time::Instant::now()));
        self.last_interaction_instant = std::time::Instant::now();
    }

    /// Show a success toast, prefixed with the check icon.
    pub(crate) fn show_success(&mut self, message: impl Into<String>) {
        self.show_status(format!("{} {}", icons::ICON_CHECK, message.into()));
    }

    /// Returns the active transient feedback message and its computed opacity `(0.0..=1.0)`.
    /// If expired, clears `self.feedback` and returns `None`.
    pub(crate) fn feedback_info(&mut self) -> Option<(String, f32)> {
        let (message, at) = self.feedback.as_ref()?;
        let elapsed = at.elapsed().as_secs_f32();
        if let Some(alpha) = compute_feedback_alpha(elapsed) {
            Some((message.clone(), alpha))
        } else {
            self.feedback = None;
            None
        }
    }

    pub(crate) fn take_status_text(&mut self) -> Option<String> {
        self.feedback_info().map(|(msg, _)| msg)
    }

    // ------------------------------------------------------------------
    // Drawing
    // ------------------------------------------------------------------

    pub(crate) fn draw_canvas(&mut self, ui: &mut egui::Ui, input: &InputSnapshot) {
        let rect = ui.max_rect();
        self.image_rect = None; // reset; set below when an image is drawn

        // Suppress the global right-click menu while the folder browser modal is
        // open, so a right-click on a pinned item doesn't leak a second menu
        // behind the dialog.
        if !self.show_folder_browser
            && input.secondary_clicked
            && input.pointer_pos.map_or(false, |p| rect.contains(p))
        {
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

        let (center, available) = if self.crop_state.is_some() {
            // Dedicated Crop Mode workspace insets:
            // Top: 32px breathing room so top edges, corners, and handles are never flush with window top.
            // Bottom: 84px clearance so the floating crop toolbar never collides with the crop box.
            // Sides: 32px lateral margins.
            let crop_workspace = egui::Rect::from_min_max(
                egui::pos2(rect.min.x + 32.0, rect.min.y + 32.0),
                egui::pos2(
                    (rect.max.x - 32.0).max(rect.min.x + 64.0),
                    (rect.max.y - 84.0).max(rect.min.y + 64.0),
                ),
            );
            (crop_workspace.center(), crop_workspace.size())
        } else {
            (rect.center(), ui.available_size())
        };

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
        if self.crop_state.is_none() && overflows && input.primary_down && over_canvas && !over_hud {
            self.pan += input.pointer_delta;
        }

        let zoom = self.effective_zoom(dimensions, available);
        let draw = viewer::draw_size(dimensions, zoom);
        let top_left = center - draw * 0.5 + self.pan;
        let image_rect = egui::Rect::from_min_size(top_left, draw);
        self.image_rect = Some(image_rect);

        // Native Windows OLE drag-and-drop out:
        // Enabled when NOT zoomed (fit-to-window / no canvas overflow) and NOT receiving an incoming drop.
        // If the user clicks down on the image and drags >= 6px, initiates OLE DoDragDrop
        // so the image can be dropped into Desktop, Explorer, Photoshop, MS Paint, etc.
        // When overflows is true (zoomed in), panning remains 100% active instead.
        if self.crop_state.is_none()
            && !overflows
            && !over_hud
            && !self.modal_open()
            && !input.has_hovered_files
        {
            if input.primary_pressed {
                if let Some(pos) = input.pointer_pos {
                    if image_rect.contains(pos) {
                        self.viewer_drag_start = Some(pos);
                    }
                }
            } else if input.primary_down {
                if let Some(start_pos) = self.viewer_drag_start {
                    if let Some(curr_pos) = input.pointer_pos {
                        if curr_pos.distance(start_pos) >= 6.0 {
                            self.viewer_drag_start = None;
                            if let Some(path) = self.current_file().cloned() {
                                crate::platform::start_drag_files(&[path]);
                                ui.ctx().request_repaint();
                            }
                        }
                    }
                }
            } else {
                self.viewer_drag_start = None;
            }
        } else {
            self.viewer_drag_start = None;
        }

        ui.painter().image(
            texture_id,
            image_rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );

        if self.crop_state.is_some() {
            crop::draw_crop_overlay(self, ui, input);
        }
    }

    /// Draw the error, empty-folder notice, or branded empty state.
    pub(crate) fn draw_no_image(&mut self, ui: &mut egui::Ui) {
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

        // Empty-folder notice: show for 7s, then return to the start screen.
        if let Some(since) = self.empty_folder_since {
            const NOTICE_SECS: u64 = 7;
            let elapsed = since.elapsed();
            if elapsed < std::time::Duration::from_secs(NOTICE_SECS) {
                let remaining = std::time::Duration::from_secs(NOTICE_SECS) - elapsed;
                ui.ctx().request_repaint_after(remaining);
                self.draw_empty_folder_notice(ui.ctx());
                return;
            }
            // Expired → fall through to the branded empty state.
            self.empty_folder_since = None;
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

    /// Branded empty state: clean logo emblem, typography, interactive shortcut buttons,
    /// and supported formats reminder, perfectly centered in the viewport.
    pub(crate) fn draw_empty_state(&mut self, ctx: &egui::Context) {
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
        let subtitle_color = if dark {
            egui::Color32::from_gray(175)
        } else {
            egui::Color32::from_gray(90)
        };
        let hint_color = if dark {
            egui::Color32::from_rgb(205, 205, 214)
        } else {
            egui::Color32::from_rgb(60, 60, 66)
        };
        let key_color = if dark {
            egui::Color32::from_rgb(255, 200, 50)
        } else {
            egui::Color32::from_rgb(178, 118, 10)
        };

        egui::Area::new(egui::Id::new("empty-state"))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .order(egui::Order::Middle)
            .show(ctx, |ui| {
                let card_w = 640.0_f32.min(ctx.screen_rect().width() - 32.0);
                ui.set_width(card_w);

                ui.vertical_centered(|ui| {
                    // 1. Clean, vibrant unboxed logo (clean, crisp, full opacity, no box/frame)
                    if let Some((texture, dims)) = &self.logo {
                        let max_side = 112.0_f32;
                        let scale = (max_side / dims[0] as f32).min(max_side / dims[1] as f32).min(1.0);
                        let size = egui::vec2(dims[0] as f32 * scale, dims[1] as f32 * scale);
                        ui.add(egui::Image::new(texture).fit_to_exact_size(size));
                    }

                    ui.add_space(14.0);

                    // 2. App title & version
                    ui.label(
                        egui::RichText::new("Photonio")
                            .size(26.0)
                            .strong()
                            .color(title_color),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                            .size(12.0)
                            .color(version_color),
                    );
                    ui.add_space(4.0);

                    // 3. Tagline
                    ui.label(
                        egui::RichText::new(
                            "A high-performance, lightweight, image viewer for Windows",
                        )
                        .size(13.0)
                        .color(subtitle_color),
                    );

                    ui.add_space(20.0);

                    // 4. Primary guidance
                    ui.label(
                        egui::RichText::new("Drop an image or folder anywhere.")
                            .size(14.0)
                            .color(hint_color),
                    );
                    ui.add_space(8.0);

                    // 5. Interactive shortcut row: measured & mathematically centered
                    let key_bg = if dark {
                        egui::Color32::from_rgba_unmultiplied(255, 200, 50, 22)
                    } else {
                        egui::Color32::from_rgba_unmultiplied(178, 118, 10, 16)
                    };
                    let key_stroke = if dark {
                        egui::Color32::from_rgba_unmultiplied(255, 200, 50, 75)
                    } else {
                        egui::Color32::from_rgba_unmultiplied(178, 118, 10, 65)
                    };

                    let text1 = "Press ";
                    let btn1_text = "Ctrl + O";
                    let text2 = " to open file or ";
                    let btn2_text = "Ctrl + Shift + O";
                    let text3 = " to open folder.";

                    let font_label = egui::FontId::proportional(14.0);
                    let font_btn = egui::FontId::proportional(13.0);
                    let btn_pad = ui.spacing().button_padding.x * 2.0 + 4.0;
                    let item_spacing = ui.spacing().item_spacing.x;

                    let w1 = ui.fonts(|f| f.layout_no_wrap(text1.to_string(), font_label.clone(), hint_color).size().x);
                    let w_b1 = ui.fonts(|f| f.layout_no_wrap(btn1_text.to_string(), font_btn.clone(), key_color).size().x) + btn_pad;
                    let w2 = ui.fonts(|f| f.layout_no_wrap(text2.to_string(), font_label.clone(), hint_color).size().x);
                    let w_b2 = ui.fonts(|f| f.layout_no_wrap(btn2_text.to_string(), font_btn.clone(), key_color).size().x) + btn_pad;
                    let w3 = ui.fonts(|f| f.layout_no_wrap(text3.to_string(), font_label.clone(), hint_color).size().x);

                    let total_row_w = w1 + w_b1 + w2 + w_b2 + w3 + (4.0 * item_spacing);
                    let left_pad = ((card_w - total_row_w) * 0.5).max(0.0);

                    let mut open_file_clicked = false;
                    let mut open_folder_clicked = false;

                    ui.horizontal(|ui| {
                        if left_pad > 0.0 {
                            ui.add_space(left_pad);
                        }
                        ui.label(egui::RichText::new(text1).size(14.0).color(hint_color));

                        let file_btn = egui::Button::new(
                            egui::RichText::new(btn1_text)
                                .size(13.0)
                                .strong()
                                .color(key_color),
                        )
                        .fill(key_bg)
                        .stroke(egui::Stroke::new(1.0_f32, key_stroke))
                        .rounding(4.0);

                        if ui
                            .add(file_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("Open File dialog (Ctrl + O)")
                            .clicked()
                        {
                            open_file_clicked = true;
                        }

                        ui.label(egui::RichText::new(text2).size(14.0).color(hint_color));

                        let folder_btn = egui::Button::new(
                            egui::RichText::new(btn2_text)
                                .size(13.0)
                                .strong()
                                .color(key_color),
                        )
                        .fill(key_bg)
                        .stroke(egui::Stroke::new(1.0_f32, key_stroke))
                        .rounding(4.0);

                        if ui
                            .add(folder_btn)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("Open Folder dialog (Ctrl + Shift + O)")
                            .clicked()
                        {
                            open_folder_clicked = true;
                        }

                        ui.label(egui::RichText::new(text3).size(14.0).color(hint_color));
                    });

                    if open_file_clicked {
                        self.open_file_dialog(ctx);
                    }
                    if open_folder_clicked {
                        self.open_folder_dialog();
                    }

                    ui.add_space(18.0);

                    // 6. Supported formats reminder badge in a distinct text and pill frame.
                    let badge_bg = if dark {
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 10)
                    } else {
                        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 7)
                    };
                    let badge_border = if dark {
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 22)
                    } else {
                        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 15)
                    };
                    let format_hint_color = if dark {
                        egui::Color32::from_gray(145)
                    } else {
                        egui::Color32::from_gray(115)
                    };
                    let format_tag_color = if dark {
                        egui::Color32::from_gray(185)
                    } else {
                        egui::Color32::from_gray(75)
                    };

                    egui::Frame::none()
                        .fill(badge_bg)
                        .stroke(egui::Stroke::new(1.0_f32, badge_border))
                        .rounding(14.0)
                        .inner_margin(egui::Margin::symmetric(14.0, 6.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("Supports:")
                                        .size(11.5)
                                        .italics()
                                        .color(format_hint_color),
                                );
                                ui.label(
                                    egui::RichText::new("PNG • JPG (JFIF) • WEBP • AVIF • GIF • BMP • ICO • TIFF • TGA • SVG (SVGZ) • JXL")
                                        .size(11.0)
                                        .monospace()
                                        .color(format_tag_color),
                                );
                            });
                        });
                });
            });
    }

    /// "No supported images" notice: a large icon + message, shown briefly after
    /// opening a folder that contains no supported image files.
    pub(crate) fn draw_empty_folder_notice(&self, ctx: &egui::Context) {
        let dark = self.is_dark_theme();
        let icon_color = if dark {
            egui::Color32::from_gray(140)
        } else {
            egui::Color32::from_gray(120)
        };
        let text_color = if dark {
            egui::Color32::from_gray(190)
        } else {
            egui::Color32::from_gray(70)
        };

        egui::Area::new(egui::Id::new("empty-folder-notice"))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .order(egui::Order::Middle)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new(icons::ICON_IMAGE).size(84.0).color(icon_color),
                    );
                    ui.add_space(16.0);
                    ui.label(
                        egui::RichText::new("No supported images in this folder")
                            .size(16.0)
                            .color(text_color),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("Supports: PNG, JPG, JFIF, WEBP, AVIF, GIF, BMP, ICO, TIFF, TGA, SVG, JXL")
                            .size(12.0)
                            .color(if dark { egui::Color32::from_gray(140) } else { egui::Color32::from_gray(120) }),
                    );
                });
            });
    }

    /// True when the app is in a dark backdrop mode (Glass or Dark).
    pub(crate) fn is_dark_theme(&self) -> bool {
        self.config.canvas_background != config::CanvasBackground::Light
    }

    /// Theme-derived glass colors for the current backdrop + opacity.
    pub(crate) fn theme(&self) -> theme::ThemeColors {
        theme::ThemeColors::new(self.is_dark_theme(), self.config.glass_opacity)
    }

    /// Normalized filmstrip window: `(tile_count, half_window)` where the count
    /// is forced odd (7/9/11) so the current tile always stays centered.
    pub(crate) fn filmstrip_window(&self) -> (usize, isize) {
        let count = self.config.filmstrip_count.clamp(7, 11) | 1;
        (count, ((count - 1) / 2) as isize)
    }

    /// Overlay visibility for auto-fade: `Some(alpha)` when the HUD/pill/
    /// filmstrip should render (fading to 0 over the last half-second), or
    /// `None` once fully faded so the overlays stop intercepting input.
    pub(crate) fn overlay_opacity(&self) -> Option<f32> {
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

    // ------------------------------------------------------------------
    // Extracted UI delegators
    // ------------------------------------------------------------------

    fn draw_hud(&mut self, ctx: &egui::Context) {
        hud::draw_hud(self, ctx);
    }

    fn draw_transient_status_pill(&mut self, ctx: &egui::Context) {
        hud::draw_transient_status_pill(self, ctx);
    }

    fn draw_quick_pill(&mut self, ctx: &egui::Context) -> Option<PillAction> {
        hud::draw_quick_pill(self, ctx)
    }

    fn draw_donation_pill(&mut self, ctx: &egui::Context) {
        hud::draw_donation_pill(self, ctx);
    }

    fn draw_filmstrip(&mut self, ctx: &egui::Context) {
        filmstrip::draw_filmstrip(self, ctx);
    }

    fn draw_grid_view(&mut self, ctx: &egui::Context) {
        grid::draw_grid_view(self, ctx);
    }

    fn draw_context_menu(&mut self, ctx: &egui::Context) -> Option<MenuAction> {
        context_menu::draw_context_menu(self, ctx)
    }

    fn draw_help(&mut self, ctx: &egui::Context) {
        dialogs::draw_help(self, ctx);
    }

    fn draw_settings(&mut self, ctx: &egui::Context) {
        dialogs::draw_settings(self, ctx);
    }

    fn draw_resize_dialog(&mut self, ctx: &egui::Context) {
        dialogs::draw_resize_dialog(self, ctx);
    }

    fn draw_image_info(&mut self, ctx: &egui::Context) {
        dialogs::draw_image_info(self, ctx);
    }

    fn draw_rename_dialog(&mut self, ctx: &egui::Context) {
        dialogs::draw_rename_dialog(self, ctx);
    }

    fn execute_menu_action(&mut self, action: MenuAction, ctx: &egui::Context) {
        context_menu::execute_menu_action(self, action, ctx);
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
        // Enforce that egui never zooms the entire page via keyboard shortcuts.
        if ctx.options(|o| o.zoom_with_keyboard) {
            ctx.options_mut(|o| o.zoom_with_keyboard = false);
        }
        if (ctx.zoom_factor() - 1.0).abs() > f32::EPSILON {
            ctx.set_zoom_factor(1.0);
        }

        // Keep global widget visuals in sync with the backdrop theme.
        if ctx.style().visuals.dark_mode != self.is_dark_theme() {
            apply_global_visuals(ctx, self.is_dark_theme());
        }

        self.ingest_drops(ctx);
        self.poll_loader(ctx);
        self.poll_subfolders(ctx);
        self.poll_registry();
        self.poll_refresh(ctx);

        // Auto-sense folder changes when window regains focus (e.g. files dropped into folder externally)
        let is_focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if is_focused && !self.was_window_focused {
            self.check_folder_changes(ctx, false);
        }
        self.was_window_focused = is_focused;

        // Periodic background directory modification heartbeat (every 1.5s)
        if self.last_folder_check.elapsed() >= std::time::Duration::from_millis(1500) {
            self.last_folder_check = std::time::Instant::now();
            self.check_folder_changes(ctx, false);
        }

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
            input.rotate_ccw = false;
            input.delete = false;
            input.help = false;
            input.toggle_filmstrip = false;
            input.toggle_play = false;
            input.step_back = false;
            input.step_forward = false;
            input.toggle_image_info = false;
            input.refresh_folder = false;
            input.rename = false;
            input.crop = false;
            input.save = false;
            input.save_as = false;
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

        let (outer_rect, inner_rect, close_requested) = ctx.input(|i| {
            (
                i.viewport().outer_rect,
                i.viewport().inner_rect,
                i.viewport().close_requested(),
            )
        });
        if let Some(outer) = outer_rect {
            self.window_pos = Some([outer.min.x, outer.min.y]);
            self.window_rect = Some(outer);
        }
        if let Some(inner) = inner_rect {
            self.window_size = Some([inner.width(), inner.height()]);
        } else if let Some(outer) = outer_rect {
            self.window_size = Some([outer.width(), outer.height()]);
        }

        // Mode-aware keyboard navigation.
        let mut viewer_navigate: isize = 0;
        if self.view_mode == ViewMode::Grid {
            let cols = self.grid_columns.max(1) as isize;
            let folder_count = if self.config.multiview_show_folders {
                self.multiview_subfolders.len()
            } else {
                0
            };
            let image_count = self.files.len();
            let total_items = folder_count + image_count;

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

            if total_items > 0 {
                let cur_unified: usize = match self.grid_selected_folder {
                    Some(f) if f < folder_count => f,
                    _ => {
                        if image_count > 0 {
                            folder_count + self.current_index.min(image_count - 1)
                        } else {
                            0
                        }
                    }
                };

                if input.home {
                    if folder_count > 0 {
                        self.grid_selected_folder = Some(0);
                    } else if image_count > 0 {
                        self.grid_selected_folder = None;
                        self.grid_select(0);
                    }
                    self.scroll_to_selected = true;
                } else if input.end {
                    let last_idx = total_items.saturating_sub(1);
                    if last_idx < folder_count {
                        self.grid_selected_folder = Some(last_idx);
                    } else if image_count > 0 {
                        self.grid_selected_folder = None;
                        self.grid_select(last_idx - folder_count);
                    }
                    self.scroll_to_selected = true;
                } else if grid_delta != 0 {
                    let new_unified = (cur_unified as isize + grid_delta)
                        .clamp(0, (total_items - 1) as isize) as usize;
                    if new_unified < folder_count {
                        self.grid_selected_folder = Some(new_unified);
                    } else if image_count > 0 {
                        self.grid_selected_folder = None;
                        self.grid_select(new_unified - folder_count);
                    }
                    self.scroll_to_selected = true;
                }
            }

            // Preview windows & thumbnails size adjustment: Ctrl+Wheel / Ctrl+Plus / Ctrl+Minus / Ctrl+0.
            if !self.modal_open() {
                if input.ctrl {
                    if input.scroll != 0.0 {
                        let delta = input.scroll * 0.25;
                        self.adjust_grid_tile_size(delta, ctx);
                    }
                    // Strip mouse wheel and zoom events from egui input so the ScrollArea NEVER scrolls the background
                    ctx.input_mut(|i| {
                        i.raw_scroll_delta = egui::Vec2::ZERO;
                        i.smooth_scroll_delta = egui::Vec2::ZERO;
                        i.events.retain(|e| !matches!(e, egui::Event::MouseWheel { .. } | egui::Event::Zoom(..)));
                    });
                }
                if !ctx.wants_keyboard_input() {
                    let tile_keys = ctx.input(|i| {
                        let plus = i.modifiers.ctrl
                            && (i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals));
                        let minus = i.modifiers.ctrl && i.key_pressed(egui::Key::Minus);
                        let reset = i.modifiers.ctrl && i.key_pressed(egui::Key::Num0);
                        (plus, minus, reset)
                    });
                    if tile_keys.0 {
                        self.adjust_grid_tile_size(20.0, ctx);
                    }
                    if tile_keys.1 {
                        self.adjust_grid_tile_size(-20.0, ctx);
                    }
                    if tile_keys.2 {
                        self.reset_grid_tile_size(ctx);
                    }
                }
            }
        } else if self.crop_state.is_some() {
            let mut dx = 0.0_f32;
            let mut dy = 0.0_f32;
            let orig_w = self.image.as_ref().map(|i| i.dimensions[0]).unwrap_or(1).max(1);
            let orig_h = self.image.as_ref().map(|i| i.dimensions[1]).unwrap_or(1).max(1);
            let step_px = if input.shift { 10.0 } else { 1.0 };
            if input.prev {
                dx -= step_px / orig_w as f32;
            }
            if input.next {
                dx += step_px / orig_w as f32;
            }
            if input.up {
                dy -= step_px / orig_h as f32;
            }
            if input.down {
                dy += step_px / orig_h as f32;
            }
            if dx != 0.0 || dy != 0.0 {
                if let Some(ref mut c) = self.crop_state {
                    c.nudge(dx, dy, orig_w, orig_h);
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

        // Enter toggles between Viewer and the fullscreen grid selector, or applies crop if active.
        if input.enter && !self.modal_open() && !self.grid_path_editing {
            if self.crop_state.is_some() {
                self.apply_crop_in_memory(ctx);
            } else if self.view_mode == ViewMode::Viewer {
                let folder_count = if self.config.multiview_show_folders {
                    self.multiview_subfolders.len()
                } else {
                    0
                };
                if !self.files.is_empty() || folder_count > 0 || self.current_folder().is_some() {
                    self.crop_state = None;
                    self.view_mode = ViewMode::Grid;
                    self.scroll_to_selected = true;
                    self.refresh_multiview_subfolders(ctx);
                    self.trigger_folder_refresh(ctx, false);
                }
            } else {
                let folder_count = if self.config.multiview_show_folders {
                    self.multiview_subfolders.len()
                } else {
                    0
                };
                if let Some(f) = self.grid_selected_folder {
                    if f < folder_count {
                        let target = self.multiview_subfolders[f].path.clone();
                        self.open_directory(target, ctx);
                        self.scroll_to_selected = true;
                        self.grid_selected_folder = None;
                    } else {
                        self.leave_grid(ctx);
                    }
                } else {
                    self.leave_grid(ctx);
                }
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
        if input.crop && self.view_mode == ViewMode::Viewer && self.image.is_some() && !self.modal_open() {
            self.toggle_crop_mode();
        }
        if input.refresh_folder {
            self.trigger_manual_refresh(ctx);
        }
        if input.rename {
            self.open_rename_dialog();
        }

        if self.view_mode == ViewMode::Grid {
            self.draw_grid_view(ctx);
            if !self.show_folder_browser {
                self.draw_transient_status_pill(ctx);
            }
        } else {
            egui::CentralPanel::default()
                .frame(egui::Frame::none())
                .show(ctx, |ui| {
                    self.draw_canvas(ui, &input);
                });

            if viewer_navigate != 0 {
                self.step(viewer_navigate, ctx);
            }

            // Hide the background HUD (navigation pill, quick-action pill, and
            // filmstrip) while the folder browser modal is open or while Crop Mode
            // is active, so the editing surface remains visually clean and unobstructed.
            if !self.show_folder_browser && self.crop_state.is_none() {
                self.draw_hud(ctx);

                if self.files.is_empty() || self.config.hud_position == config::HudPosition::Hidden {
                    self.draw_transient_status_pill(ctx);
                }

                if let Some(action) = self.draw_quick_pill(ctx) {
                    self.execute_pill_action(action, ctx);
                }

                self.draw_filmstrip(ctx);
                self.draw_donation_pill(ctx);
            }
        }

        if let Some(action) = self.draw_context_menu(ctx) {
            self.execute_menu_action(action, ctx);
        }

        self.draw_help(ctx);
        self.draw_settings(ctx);
        self.draw_image_info(ctx);
        self.draw_resize_dialog(ctx);
        self.draw_rename_dialog(ctx);

        if self.crop_state.is_some() && self.view_mode == ViewMode::Viewer && self.image.is_some() {
            crop::draw_crop_toolbar(self, ctx);
            if input.save {
                self.save_crop_overwrite(ctx);
            }
            if input.save_as {
                self.save_crop_as_copy(ctx);
            }
        } else if self.crop_state.is_some() && (self.view_mode != ViewMode::Viewer || self.image.is_none()) {
            self.crop_state = None;
        }

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
            } else if self.crop_state.is_some() {
                self.crop_state = None;
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
            } else if self.rename_state.is_some() {
                self.rename_state = None;
            } else if self.context_menu_pos.is_some() {
                self.context_menu_pos = None;
            } else if self.grid_path_editing {
                self.grid_path_editing = false;
                self.grid_path_invalid = false;
                if let Some(dir) = self.current_folder() {
                    self.grid_path_buffer = dir.display().to_string();
                } else {
                    self.grid_path_buffer.clear();
                }
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
        if input.save {
            if self.rotation != 0 {
                self.save_rotation_overwrite(ctx);
            } else {
                self.show_status("No changes to save");
            }
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
            self.is_supporter,
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

        // Transient feedback status pill lifecycle: animate fade-in, hold full visibility,
        // animate fade-out, and auto-clear when expired so the message never
        // hijacks the status pill even if the user is completely idle.
        if let Some((_, at)) = &self.feedback {
            let elapsed = at.elapsed().as_secs_f32();
            if elapsed < FEEDBACK_FADE_IN_SECS {
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            } else if elapsed < FEEDBACK_HOLD_SECS {
                let remaining = FEEDBACK_HOLD_SECS - elapsed;
                ctx.request_repaint_after(std::time::Duration::from_secs_f32(remaining));
            } else if elapsed < FEEDBACK_TOTAL_SECS {
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            } else {
                self.feedback = None;
                ctx.request_repaint();
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





