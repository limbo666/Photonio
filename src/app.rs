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
/// Amber warning color for registry path-mismatch notices.
const WARN_COLOR: egui::Color32 = egui::Color32::from_rgb(255, 170, 40);
/// PayPal donation link (opened in the default browser).
pub(crate) const DONATION_URL: &str = "https://www.paypal.com/donate/?hosted_button_id=G4LEB66MN2LRU";

/// Seconds for the status pill feedback badge to fade in.
pub(crate) const FEEDBACK_FADE_IN_SECS: f32 = 0.2;
/// Seconds for the status pill feedback badge to hold full visibility.
pub(crate) const FEEDBACK_HOLD_SECS: f32 = 2.5;
/// Seconds for the status pill feedback badge to fade out.
pub(crate) const FEEDBACK_FADE_OUT_SECS: f32 = 0.5;
/// Total lifespan of a transient status pill feedback badge before auto-clearance.
pub(crate) const FEEDBACK_TOTAL_SECS: f32 = FEEDBACK_HOLD_SECS + FEEDBACK_FADE_OUT_SECS;

/// Compute the opacity `(0.0..=1.0)` of a transient feedback pill given the elapsed seconds.
/// Returns `None` once the badge has completed its full lifecycle and should be cleared.
pub(crate) fn compute_feedback_alpha(elapsed: f32) -> Option<f32> {
    if elapsed >= FEEDBACK_TOTAL_SECS {
        return None;
    }
    let alpha = if elapsed < FEEDBACK_FADE_IN_SECS {
        (elapsed / FEEDBACK_FADE_IN_SECS).clamp(0.0, 1.0)
    } else if elapsed <= FEEDBACK_HOLD_SECS {
        1.0
    } else {
        ((FEEDBACK_TOTAL_SECS - elapsed) / FEEDBACK_FADE_OUT_SECS).clamp(0.0, 1.0)
    };
    Some(alpha)
}

/// True if `euro.png` or `euro.jpg` sits next to the executable (supporter key).
/// Checked once at startup and cached — never polled per frame.
fn is_supporter_check() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .map(|dir| dir.join("euro.png").is_file() || dir.join("euro.jpg").is_file())
        .unwrap_or(false)
}

/// Register Font Awesome Solid + Brands as fallback fonts so icon glyphs
/// (including brand marks like PayPal) render natively across the UI.
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "font_awesome".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/fa-solid-900.otf")),
    );
    // Brand icons (e.g. PayPal `\u{f1ed}`) live in a separate FA font.
    fonts.font_data.insert(
        "font_awesome_brands".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/fa-brands-400.otf")),
    );
    let proportional = fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default();
    proportional.push("font_awesome".to_owned());
    proportional.push("font_awesome_brands".to_owned());
    ctx.set_fonts(fonts);
}

/// Set the global egui widget visuals to match the backdrop theme, so
/// Global egui visual tweaks applied at startup and when theme switches so that
/// checkboxes, radio buttons, separators, and buttons follow Light/Dark mode.
fn apply_global_visuals(ctx: &egui::Context, dark_mode: bool) {
    if dark_mode {
        let mut visuals = egui::Visuals::dark();
        // Selected button / item background & border:
        // A luminous amber-gold glass tint.
        visuals.selection.bg_fill = egui::Color32::from_rgba_unmultiplied(255, 200, 50, 45);
        // In egui, selection.stroke defines the text stroke for selected items.
        // Crisp white-ish text provides maximum contrast (>11:1) over dark glass.
        visuals.selection.stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_gray(250));

        // When selected/active:
        visuals.widgets.active.bg_fill = egui::Color32::from_rgba_unmultiplied(255, 200, 50, 65);
        visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(255, 200, 50));
        visuals.widgets.active.fg_stroke.color = egui::Color32::WHITE;
        visuals.widgets.active.rounding = egui::Rounding::same(4.0);
        visuals.widgets.active.expansion = 0.0_f32;

        // Hovered widgets:
        visuals.widgets.hovered.bg_fill = egui::Color32::from_white_alpha(30);
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(70));
        visuals.widgets.hovered.fg_stroke.color = egui::Color32::WHITE;
        visuals.widgets.hovered.rounding = egui::Rounding::same(4.0);

        // Inactive widgets:
        visuals.widgets.inactive.bg_fill = egui::Color32::from_white_alpha(10);
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(25));
        visuals.widgets.inactive.fg_stroke.color = egui::Color32::from_gray(185);
        visuals.widgets.inactive.rounding = egui::Rounding::same(4.0);

        ctx.set_visuals(visuals);
        return;
    }

    let mut visuals = egui::Visuals::light();
    visuals.widgets.noninteractive.bg_stroke =
        egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(30));

    // Inactive widgets:
    visuals.widgets.inactive.bg_fill =
        egui::Color32::from_rgba_premultiplied(230, 233, 238, 200);
    visuals.widgets.inactive.bg_stroke =
        egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(40));
    visuals.widgets.inactive.fg_stroke.color = egui::Color32::from_gray(40);
    visuals.widgets.inactive.rounding = egui::Rounding::same(4.0);

    // Hovered widgets:
    visuals.widgets.hovered.bg_fill =
        egui::Color32::from_rgba_premultiplied(214, 218, 226, 200);
    visuals.widgets.hovered.bg_stroke =
        egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(70));
    visuals.widgets.hovered.fg_stroke.color = egui::Color32::BLACK;
    visuals.widgets.hovered.rounding = egui::Rounding::same(4.0);

    // Selected button / item background & border on Light mode:
    // Rich deep amber fill with clean amber stroke.
    visuals.selection.bg_fill = egui::Color32::from_rgb(178, 118, 10);
    // In egui, selection.stroke defines the text stroke for selected widgets.
    // Crisp white text provides strong contrast (4.8:1 WCAG AA) against the amber fill.
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, egui::Color32::WHITE);

    // When widgets are active (pressed) in light mode:
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(178, 118, 10);
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(148, 98, 8));
    // strong_text_color() derives from widgets.active.fg_stroke.color. Keep it dark charcoal
    // so bold headers and labels (e.g. in Settings cards) are dark and legible, not washed out white!
    visuals.widgets.active.fg_stroke.color = egui::Color32::from_gray(25);
    visuals.widgets.active.rounding = egui::Rounding::same(4.0);
    visuals.widgets.active.expansion = 0.0_f32;

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
    rotate_ccw: bool,
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
    save: bool,
    save_as: bool,
    print: bool,
    toggle_image_info: bool,
    resize_image: bool,
    refresh_folder: bool,
    rename: bool,
}

/// An action selected from the quick-action pill.
enum PillAction {
    OpenFile,
    OpenFolder,
    SaveAs,
    SaveRotation,
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

/// A subfolder displayed in the MultiView grid.
#[derive(Clone, Debug)]
pub struct MultiViewSubfolder {
    pub path: PathBuf,
    pub name: String,
    pub preview_paths: Vec<PathBuf>,
}

enum SubfolderWorkerMsg {
    Folders {
        gen: u64,
        folders: Vec<MultiViewSubfolder>,
    },
    Thumb {
        gen: u64,
        path: PathBuf,
        rgba: Vec<u8>,
        dims: [usize; 2],
    },
}

enum GridFolderAction {
    Select(usize),
    Open(usize),
    ContextMenu(usize, egui::Pos2),
}

/// A segment in the interactive breadcrumbs path bar.
#[derive(Debug, Clone, PartialEq)]
enum BreadcrumbSegment {
    /// A single navigable folder button.
    Crumb {
        label: String,
        path: PathBuf,
        is_current: bool,
    },
    /// Intermediate folders collapsed into a button ("...") to fit within available width.
    Ellipsis {
        hidden: Vec<(String, PathBuf)>,
    },
}

/// Compute the breadcrumb segments to render inside the available address bar width.
/// When the path exceeds the width, intermediate folders collapse into an Ellipsis segment ("..."),
/// preserving the root (drive) and the active folder as interactive buttons.
fn layout_breadcrumbs(
    crumbs: &[(String, PathBuf)],
    available_width: f32,
    measure_text_width: impl Fn(&str, bool) -> f32,
) -> Vec<BreadcrumbSegment> {
    if crumbs.is_empty() {
        return Vec::new();
    }

    let n = crumbs.len();
    let btn_pad = 8.0_f32; // 4.0 left + 4.0 right button padding
    let sep_w = measure_text_width("›", false) + 4.0;
    let ellipsis_w = measure_text_width("...", false) + btn_pad;

    let crumb_widths: Vec<f32> = crumbs
        .iter()
        .enumerate()
        .map(|(i, (label, _))| {
            let is_last = i == n - 1;
            measure_text_width(label, is_last) + btn_pad
        })
        .collect();

    let total_uncollapsed: f32 = crumb_widths.iter().sum::<f32>()
        + if n > 1 { (n - 1) as f32 * sep_w } else { 0.0 };

    if total_uncollapsed <= available_width || n <= 2 {
        if total_uncollapsed <= available_width || n <= 1 {
            return crumbs
                .iter()
                .enumerate()
                .map(|(i, (label, path))| BreadcrumbSegment::Crumb {
                    label: label.clone(),
                    path: path.clone(),
                    is_current: i == n - 1,
                })
                .collect();
        }
    }

    // Try keeping crumbs[0] (root), an Ellipsis for intermediate crumbs (1..k),
    // and as many trailing crumbs (k..n) as possible, always including crumbs[n-1].
    let mut best_k = None;
    for k in 2..n {
        let trailing_w: f32 = crumb_widths[k..n].iter().sum::<f32>()
            + (n - k) as f32 * sep_w;
        let candidate_w = crumb_widths[0] + sep_w + ellipsis_w + trailing_w;
        if candidate_w <= available_width {
            best_k = Some(k);
            break;
        }
    }

    if let Some(k) = best_k {
        let mut segments = Vec::new();
        segments.push(BreadcrumbSegment::Crumb {
            label: crumbs[0].0.clone(),
            path: crumbs[0].1.clone(),
            is_current: false,
        });
        segments.push(BreadcrumbSegment::Ellipsis {
            hidden: crumbs[1..k].to_vec(),
        });
        for i in k..n {
            segments.push(BreadcrumbSegment::Crumb {
                label: crumbs[i].0.clone(),
                path: crumbs[i].1.clone(),
                is_current: i == n - 1,
            });
        }
        return segments;
    }

    // If even root + ... + current does not fit, collapse root as well: ... + current
    let last_w = crumb_widths[n - 1];
    let candidate_no_root = ellipsis_w + sep_w + last_w;
    if candidate_no_root <= available_width {
        return vec![
            BreadcrumbSegment::Ellipsis {
                hidden: crumbs[0..n - 1].to_vec(),
            },
            BreadcrumbSegment::Crumb {
                label: crumbs[n - 1].0.clone(),
                path: crumbs[n - 1].1.clone(),
                is_current: true,
            },
        ];
    }

    // If even that exceeds available width, truncate the current folder's label
    let avail_for_last = (available_width - ellipsis_w - sep_w).max(10.0);
    let original_label = &crumbs[n - 1].0;
    let avg_char_w = (last_w / original_label.chars().count().max(1) as f32).max(6.0);
    let max_chars = ((avail_for_last - btn_pad) / avg_char_w).floor().max(3.0) as usize;
    let truncated_label = truncate(original_label, max_chars);

    vec![
        BreadcrumbSegment::Ellipsis {
            hidden: crumbs[0..n - 1].to_vec(),
        },
        BreadcrumbSegment::Crumb {
            label: truncated_label,
            path: crumbs[n - 1].1.clone(),
            is_current: true,
        },
    ]
}

/// A folder-browser operation awaiting the user's "Select Folder" confirmation.
#[derive(Clone, Copy)]
enum PendingOperation {
    OpenFolder,
    CopyFileTo,
    MoveFileTo,
}

/// State for the file rename modal dialog.
struct RenameState {
    old_path: PathBuf,
    new_name: String,
    focus_pending: bool,
}

/// An action selected from the context menu.
enum MenuAction {
    OpenFile,
    OpenFolder,
    CloseImage,
    CloseFolder,
    ResizeImage,
    SaveRotation,
    SaveAs,
    OpenContainingFolder,
    OpenWith,
    CopyFileTo,
    MoveFileTo,
    CopyFile,
    CopyImage,
    CopyPath,
    Rename,
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
    lod: loader::PreviewLod,
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
    /// Rename modal state (present while the dialog is open).
    rename_state: Option<RenameState>,
    /// The custom folder browser is open.
    show_folder_browser: bool,
    /// What the folder browser should do when "Select Folder" is clicked.
    pending_operation: Option<PendingOperation>,
    /// Folder browser state (present while the browser is open).
    folder_browser: Option<viewer::folder_browser::Browser>,
    /// Whether the grid view path bar is currently in raw text edit mode.
    grid_path_editing: bool,
    /// Text buffer for editing the folder path in grid view.
    grid_path_buffer: String,
    /// Focus request flag for the grid path edit field.
    grid_path_focus_pending: bool,
    /// Whether the user-typed path in grid view failed directory validation.
    grid_path_invalid: bool,
    /// History stack of previously visited folders for the Back button.
    folder_back_stack: Vec<PathBuf>,
    /// History stack of folders for the Forward button.
    folder_forward_stack: Vec<PathBuf>,
    /// Set when help just opened, to ignore the same-frame click.
    help_opened_this_frame: bool,
    /// Transient feedback badge: (message, when shown).
    feedback: Option<(String, std::time::Instant)>,
    /// When an empty (no supported images) folder was opened; drives a
    /// self-dismissing notice that returns to the start screen after 7s.
    empty_folder_since: Option<std::time::Instant>,
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
    /// Cached at startup: `euro.png` / `euro.jpg` sits next to the executable.
    pub is_supporter: bool,
    /// The branded watermark logo texture + dimensions, if decoded.
    logo: Option<(egui::TextureHandle, [usize; 2])>,
    /// Background image decoder + preload channel manager.
    loader: loader::Loader,
    /// Cached display-res RGBA buffers by file index: (rgba, display, full).
    full_cache: HashMap<usize, (Vec<u8>, [usize; 2], [usize; 2])>,
    /// Indices with a full decode currently in flight (dedup).
    pending_full: HashSet<usize>,
    /// Indices with a thumbnail decode currently in flight (dedup + retry by LOD).
    pending_thumb: HashMap<usize, loader::PreviewLod>,
    /// Indices whose thumbnail decode definitively failed (shown as a
    /// placeholder; never retried until the folder changes).
    failed_thumb: HashSet<usize>,
    /// File index currently awaiting a full decode (to show a spinner).
    loading_index: Option<usize>,
    /// Cached thumbnails by file index.
    thumb_cache: HashMap<usize, Thumb>,
    /// Receiver for asynchronous background folder rescanning: (scanned_dir, new_files, is_manual).
    refresh_rx: Option<std::sync::mpsc::Receiver<(PathBuf, Vec<PathBuf>, bool)>>,
    /// True while a background folder rescan thread is running.
    is_refreshing: bool,
    /// Currently hovered grid item index (used to anchor zoom around the pointer target).
    grid_hovered_index: Option<usize>,
    /// Last measured grid content area available width.
    grid_available_width: f32,
    /// Item spacing in the grid.
    grid_spacing_y: f32,
    /// Subfolders contained within the active directory for MultiView.
    multiview_subfolders: Vec<MultiViewSubfolder>,
    /// Decoded GPU textures for subfolder quad previews, keyed by image path.
    multiview_folder_textures: HashMap<PathBuf, egui::TextureHandle>,
    /// Generation counter for async subfolder scans (session dedup).
    multiview_folder_gen: u64,
    /// Receiver for async subfolder and quad thumbnail worker messages.
    multiview_folder_rx: Option<std::sync::mpsc::Receiver<SubfolderWorkerMsg>>,
    /// Currently selected subfolder index in the MultiView grid, if any.
    grid_selected_folder: Option<usize>,
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

    /// Best-effort thumbnail request for one index at the specified LOD, deduped
    /// against the cache and in-flight set. Upgrades existing lower-res thumbnails
    /// seamlessly when higher zoom levels are viewed.
    fn request_thumb_for(&mut self, index: usize, needed_lod: loader::PreviewLod) {
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
            self.request_thumb_for(i, loader::PreviewLod::Small);
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
    fn poll_subfolders(&mut self, ctx: &egui::Context) {
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
    fn refresh_multiview_subfolders(&mut self, ctx: &egui::Context) {
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
        self.multiview_folder_gen += 1;
        self.multiview_subfolders.clear();
        self.multiview_folder_textures.clear();
        self.multiview_folder_rx = None;
        self.grid_selected_folder = None;
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
        self.grid_path_editing = false;
        self.grid_path_invalid = false;
        self.view_mode = ViewMode::Viewer;
        if !self.files.is_empty() {
            self.navigate_to(self.current_index, ctx);
        }
        self.trigger_folder_refresh(ctx, false);
    }

    fn current_file(&self) -> Option<&PathBuf> {
        self.files.get(self.current_index)
    }

    fn current_folder(&self) -> Option<PathBuf> {
        self.files
            .first()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .or_else(|| self.config.last_directory.clone())
            .filter(|p| p.is_dir())
    }

    /// Asynchronously initiate a background folder scan and reconcile changes
    /// without disrupting the active view, viewport zoom/pan, or existing GPU thumbnail caches.
    fn trigger_folder_refresh(&mut self, ctx: &egui::Context, manual: bool) {
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
        std::thread::spawn(move || {
            let files = filesystem::scan_directory(&dir_clone);
            let _ = tx.send((dir_clone, files, manual));
            worker_ctx.request_repaint();
        });
    }

    /// Trigger a manual folder refresh (e.g. from F5 key).
    fn trigger_manual_refresh(&mut self, ctx: &egui::Context) {
        self.trigger_folder_refresh(ctx, true);
    }

    /// Drain a finished background folder scan and apply the reconciliation.
    fn poll_refresh(&mut self, ctx: &egui::Context) {
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
    fn reconcile_caches(&mut self, old_files: &[PathBuf]) {
        self.loader.bump_generation();

        self.full_cache = remap_cache_keys(&mut self.full_cache, old_files, &self.files);
        self.thumb_cache = remap_cache_keys(&mut self.thumb_cache, old_files, &self.files);
        self.pending_full.clear();
        self.pending_thumb.clear();
        self.failed_thumb.clear();
    }

    /// Reconcile cached decoded RGBA buffers and GPU thumbnail textures with an optional
    /// explicitly renamed item pair `(old_idx, new_idx)`.
    fn reconcile_caches_with_rename(
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
    fn apply_folder_refresh(
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
        }

        ctx.request_repaint();
    }

    /// Push current folder to history stack before switching to `dir`.
    fn push_folder_history(&mut self, dir: &Path) {
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

    fn open_file(&mut self, path: PathBuf, ctx: &egui::Context) {
        if !filesystem::is_supported_path(&path) {
            self.image = None;
            self.preview = None;
            self.animation = None;
            self.error = Some(format!("Unsupported file: {}", path.display()));
            return;
        }

        self.clear_caches();
        self.empty_folder_since = None;
        match path.parent() {
            Some(dir) => {
                self.push_folder_history(dir);
                self.config.last_directory = Some(dir.to_path_buf());
                self.save_config();
                self.files = filesystem::scan_directory(dir);
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
    fn open_directory(&mut self, dir: PathBuf, ctx: &egui::Context) {
        self.push_folder_history(&dir);
        self.open_directory_internal(dir, ctx);
    }

    /// Low-level folder opening without mutating the back/forward history stacks.
    fn open_directory_internal(&mut self, dir: PathBuf, ctx: &egui::Context) {
        self.config.last_directory = Some(dir.clone());
        self.save_config();

        self.clear_caches();
        self.files = filesystem::scan_directory(&dir);
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
    fn navigate_folder_back(&mut self, ctx: &egui::Context) -> bool {
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
    fn navigate_folder_forward(&mut self, ctx: &egui::Context) -> bool {
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

    fn ingest_drops(&mut self, ctx: &egui::Context) {
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
            PillAction::SaveRotation => self.save_rotation_overwrite(ctx),
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

    /// Open the rename dialog for the active or selected file.
    fn open_rename_dialog(&mut self) {
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
    fn commit_rename(&mut self, new_name: &str, ctx: &egui::Context) {
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


    /// Save the in-memory manual rotation directly back to the active file (overwrite).
    fn save_rotation_overwrite(&mut self, ctx: &egui::Context) {
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

    /// "Copy File to...": copy to the remembered folder, or browse on Ctrl.
    fn copy_file_to(&mut self, ctx: &egui::Context) {
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

    fn do_copy(&mut self, src: PathBuf, dir: PathBuf) {
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
    fn move_file_to(&mut self, ctx: &egui::Context) {
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

    fn do_move(&mut self, src: PathBuf, dir: PathBuf, ctx: &egui::Context) {
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

    fn reset_viewport(&mut self) {
        self.zoom = None;
        self.pan = egui::Vec2::ZERO;
    }

    fn effective_zoom(&self, dimensions: [usize; 2], available: egui::Vec2) -> f32 {
        self.zoom
            .unwrap_or_else(|| viewer::fit_scale(dimensions, available))
    }

    fn rotate_current_cw(&mut self, ctx: &egui::Context) {
        self.rotation = (self.rotation + 90) % 360;
        if let Some(image) = &mut self.image {
            image.rotate_90_cw(ctx);
        }
    }

    fn rotate_current_ccw(&mut self, ctx: &egui::Context) {
        self.rotation = (self.rotation + 270) % 360;
        if let Some(image) = &mut self.image {
            image.rotate_90_ccw(ctx);
        }
    }

    // ------------------------------------------------------------------
    // Grid tile sizing
    // ------------------------------------------------------------------

    /// Minimum allowed grid preview container width in logical points.
    const MIN_GRID_TILE_SIZE: f32 = 80.0;
    /// Maximum allowed grid preview container width in logical points.
    const MAX_GRID_TILE_SIZE: f32 = 500.0;
    /// Default grid preview container width.
    const DEFAULT_GRID_TILE_SIZE: f32 = 150.0;

    /// The current grid tile width, clamped to the valid range.
    fn grid_tile_width(&self) -> f32 {
        self.config
            .grid_tile_size
            .clamp(Self::MIN_GRID_TILE_SIZE, Self::MAX_GRID_TILE_SIZE)
    }

    /// The current grid tile height: thumbnail area plus standard label margin.
    fn grid_tile_height(&self) -> f32 {
        self.grid_tile_width() + 26.0
    }

    /// Adjust the grid tile size by `delta` points (positive = grow), persist,
    /// show a transient status pill, and anchor the scroll viewport so the target
    /// item stays fixed on screen rather than jumping or scrolling the background.
    fn adjust_grid_tile_size(&mut self, delta: f32, ctx: &egui::Context) {
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
    fn reset_grid_tile_size(&mut self, ctx: &egui::Context) {
        let delta = Self::DEFAULT_GRID_TILE_SIZE - self.grid_tile_width();
        self.adjust_grid_tile_size(delta, ctx);
        self.show_success("Preview size reset to default");
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
            refresh_folder: i.key_pressed(egui::Key::F5),
            rename: i.key_pressed(egui::Key::F2),
        })
    }

    fn apply_hotkeys(&mut self, ctx: &egui::Context, input: &InputSnapshot) {
        // Viewport hotkeys only apply in Viewer mode, and not while a modal or
        // the context menu is open.
        if self.modal_open() || self.view_mode != ViewMode::Viewer {
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
                self.view_mode = ViewMode::Grid;
                self.scroll_to_selected = true;
                self.refresh_multiview_subfolders(ctx);
                self.trigger_folder_refresh(ctx, false);
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
            || self.rename_state.is_some()
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
        self.last_interaction_instant = std::time::Instant::now();
    }

    /// Show a success toast, prefixed with the check icon.
    fn show_success(&mut self, message: impl Into<String>) {
        self.show_status(format!("{} {}", icons::ICON_CHECK, message.into()));
    }

    /// Returns the active transient feedback message and its computed opacity `(0.0..=1.0)`.
    /// If expired, clears `self.feedback` and returns `None`.
    fn feedback_info(&mut self) -> Option<(String, f32)> {
        let (message, at) = self.feedback.as_ref()?;
        let elapsed = at.elapsed().as_secs_f32();
        if let Some(alpha) = compute_feedback_alpha(elapsed) {
            Some((message.clone(), alpha))
        } else {
            self.feedback = None;
            None
        }
    }

    fn take_status_text(&mut self) -> Option<String> {
        self.feedback_info().map(|(msg, _)| msg)
    }

    // ------------------------------------------------------------------
    // Drawing
    // ------------------------------------------------------------------

    fn draw_canvas(&mut self, ui: &mut egui::Ui, input: &InputSnapshot) {
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

    /// Draw the error, empty-folder notice, or branded empty state.
    fn draw_no_image(&mut self, ui: &mut egui::Ui) {
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
        let key_color = if dark {
            egui::Color32::from_rgb(255, 200, 50)
        } else {
            egui::Color32::from_rgb(178, 118, 10)
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
                    // 4. Tagline.
                    ui.label(
                        egui::RichText::new(
                            "A high-performance, lightweight, image viewer for Windows",
                        )
                        .size(13.0)
                        .color(version_color),
                    );
                    ui.add_space(16.0);
                    // 5. Guidance hints (centered).
                    ui.label(
                        egui::RichText::new("Drop an image or folder.")
                            .size(14.0)
                            .color(hint_color),
                    );
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Press ").size(14.0).color(hint_color));
                        ui.label(egui::RichText::new("Ctrl + O").size(14.0).color(key_color));
                        ui.label(
                            egui::RichText::new(" to open file or ")
                                .size(14.0)
                                .color(hint_color),
                        );
                        ui.label(
                            egui::RichText::new("Ctrl + Shift + O")
                                .size(14.0)
                                .color(key_color),
                        );
                        ui.label(
                            egui::RichText::new(" to open folder.")
                                .size(14.0)
                                .color(hint_color),
                        );
                    });
                });
            });
    }

    /// "No supported images" notice: a large icon + message, shown briefly after
    /// opening a folder that contains no supported image files.
    fn draw_empty_folder_notice(&self, ctx: &egui::Context) {
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

                        if let Some(status) = &status {
                            let text_color = if status.contains("failed")
                                || status.contains("Failed")
                                || status.starts_with("No image")
                            {
                                ERROR_COLOR
                            } else {
                                SUCCESS_COLOR
                            };
                            ui.label(egui::RichText::new(status).color(text_color));
                        } else {
                            ui.label(name);
                        }

                        if let (Some(cur), Some(total)) = (gif_cur, gif_total) {
                            let badge = gif_badge.unwrap_or("[GIF]");
                            ui.label(egui::RichText::new(badge).color(colors.gold).monospace());
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

    /// Floating transient status pill displayed when an action produces feedback
    /// and the regular HUD navigation bar is not shown (multiview grid mode,
    /// empty folder viewer, or when HUD position is set to Hidden).
    fn draw_transient_status_pill(&mut self, ctx: &egui::Context) {
        let Some((message, alpha)) = self.feedback_info() else {
            return;
        };
        if alpha <= 0.0 {
            return;
        }

        let colors = self.theme();
        let (anchor, offset) = match self.config.hud_position {
            config::HudPosition::Top => (egui::Align2::CENTER_TOP, egui::vec2(0.0, 40.0)),
            _ => (egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -20.0)),
        };

        let text_color = if message.contains("failed")
            || message.contains("Failed")
            || message.starts_with("No image")
        {
            ERROR_COLOR
        } else {
            SUCCESS_COLOR
        };

        egui::Area::new(egui::Id::new("transient-status-pill"))
            .anchor(anchor, offset)
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                ui.set_opacity(alpha);
                colors.panel(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(message).color(text_color));
                    });
                });
            });
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

        // Orientation-specific inner padding: tight cross-axis margin and
        // slightly larger end caps, with a matching inter-button gap.
        let margin = if vertical {
            egui::Margin::symmetric(4.0, 7.0) // snug left/right, 7px vertical caps
        } else {
            egui::Margin::symmetric(7.0, 4.0) // 7px horizontal caps, tight top/bottom
        };

        egui::Area::new(egui::Id::new("quick-action-pill"))
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 12.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_opacity(opacity);
                egui::Frame::none()
                    .fill(colors.panel_fill)
                    .stroke(egui::Stroke::new(1.0_f32, colors.border))
                    .rounding(10.0)
                    .inner_margin(margin)
                    .shadow(egui::Shadow {
                        offset: egui::vec2(0.0, 4.0),
                        blur: 18.0,
                        spread: 0.0,
                        color: egui::Color32::from_black_alpha(150),
                    })
                    .show(ui, |ui| {
                        colors.apply(ui);
                        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
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

    /// Floating donation CTA pill docked top-left (mirrors the quick-action pill).
    fn draw_donation_pill(&mut self, ctx: &egui::Context) {
        // Periodic schedule: active for 2 runs, hidden for 10, then repeats.
        let show = !self.is_supporter
            && self.config.launch_count >= 10
            && ((self.config.launch_count - 10) % 12) < 2;
        if !show || self.modal_open() {
            return;
        }
        let Some(opacity) = self.overlay_opacity() else {
            return;
        };
        let colors = self.theme();

        egui::Area::new(egui::Id::new("donation-pill"))
            .anchor(egui::Align2::LEFT_TOP, egui::vec2(16.0, 16.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_opacity(opacity);
                let frame_resp = egui::Frame::none()
                    .fill(colors.panel_fill)
                    .stroke(egui::Stroke::new(1.0_f32, colors.border))
                    .rounding(10.0)
                    .inner_margin(egui::Margin::symmetric(12.0, 6.0))
                    .shadow(egui::Shadow {
                        offset: egui::vec2(0.0, 4.0),
                        blur: 18.0,
                        spread: 0.0,
                        color: egui::Color32::from_black_alpha(150),
                    })
                    .show(ui, |ui| {
                        colors.apply(ui);
                        let label = egui::RichText::new(format!(
                            "{} Support",
                            icons::ICON_PAYPAL
                        ))
                        .color(colors.gold)
                        .size(14.0);

                        let link = egui::Hyperlink::from_label_and_url(label, DONATION_URL);
                        ui.add(link)
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text("Support Photonio development on PayPal");
                    });

                if frame_resp.response.hovered() {
                    ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                }
            });
    }

    /// Render the pill's buttons; returns the clicked action, if any.
    fn pill_buttons(&self, ui: &mut egui::Ui) -> Option<PillAction> {
        let colors = self.theme();

        // When rotation is pending on the active static image, pop a breathing Save Rotation action
        if self.view_mode == ViewMode::Viewer && self.image.is_some() && self.rotation != 0 {
            if pill_button_breathing(
                ui,
                icons::ICON_SAVE,
                "Save Rotation (Ctrl+S)",
                &colors,
            )
            .clicked()
            {
                return Some(PillAction::SaveRotation);
            }
            ui.separator();
        }

        if self.config.pill_open_file {
            if pill_button(ui, icons::ICON_OPEN_FILE, "Open File (Ctrl+O)", false, colors.gold).clicked() {
                return Some(PillAction::OpenFile);
            }
        }
        if self.config.pill_open_folder {
            if pill_button(ui, icons::ICON_OPEN_FOLDER, "Open Folder (Ctrl+Shift+O)", false, colors.gold)
                .clicked()
            {
                return Some(PillAction::OpenFolder);
            }
        }
        if self.config.pill_save_as {
            if pill_button(ui, icons::ICON_SAVE, "Save As (Ctrl+Shift+S)", false, colors.gold).clicked() {
                return Some(PillAction::SaveAs);
            }
        }
        if self.config.pill_print {
            if pill_button(ui, icons::ICON_PRINT, "Print (Ctrl+P)", false, colors.gold).clicked() {
                return Some(PillAction::Print);
            }
        }
        if self.config.pill_share {
            // Placeholder — disabled until sharing is implemented.
            ui.add_enabled_ui(false, |ui| {
                ui.add_sized(
                    egui::vec2(32.0, 32.0),
                    egui::Button::new(egui::RichText::new(icons::ICON_SHARE).size(18.0))
                        .frame(false)
                        .min_size(egui::vec2(32.0, 32.0)),
                )
                .on_hover_text("Share (coming soon)");
            });
        }
        if self.config.pill_filmstrip {
            // Separator: file actions (above) vs. interface actions (filmstrip).
            ui.separator();
            if pill_button(
                ui,
                icons::ICON_FILMSTRIP,
                "Toggle Filmstrip (T)",
                self.config.show_filmstrip,
                colors.gold,
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
                egui::Stroke::new(2.0_f32, colors.gold),
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
        let tw = self.grid_tile_width();
        let th = self.grid_tile_height();

        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(tw, th), egui::Sense::click());
        let painter = ui.painter();
        let card = rect.shrink(3.0);

        // Rounded card background (theme-aware glass tint).
        painter.rect_filled(card, 6.0, colors.placeholder);

        // Selection / hover accent frame.
        if index == self.current_index && self.grid_selected_folder.is_none() {
            painter.rect_stroke(card, 6.0, egui::Stroke::new(2.0_f32, colors.gold));
        } else if response.hovered() {
            painter.rect_stroke(card, 6.0, egui::Stroke::new(1.0_f32, colors.muted));
        }

        if response.hovered() {
            self.grid_hovered_index = Some(index);
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
        let name = self
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

    /// Draw one grid folder card with either large folder icon or quad (2x2) image previews.
    fn grid_folder_tile(&mut self, ui: &mut egui::Ui, folder_index: usize) -> Option<GridFolderAction> {
        let colors = self.theme();
        let tw = self.grid_tile_width();
        let th = self.grid_tile_height();

        let folder = match self.multiview_subfolders.get(folder_index) {
            Some(f) => f.clone(),
            None => return None,
        };

        let (rect, response) = ui.allocate_exact_size(egui::vec2(tw, th), egui::Sense::click());
        let painter = ui.painter();
        let card = rect.shrink(3.0);

        // Rounded card background
        painter.rect_filled(card, 6.0, colors.placeholder);

        // Selection / hover accent frame
        let is_selected = self.grid_selected_folder == Some(folder_index);
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
                    if let Some(tex) = self.multiview_folder_textures.get(img_path) {
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

        if response.double_clicked() {
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

    /// Full-window grid selector view (virtualized thumbnail browser).
    fn draw_grid_view(&mut self, ctx: &egui::Context) {
        let len = self.files.len();
        let mut back_clicked = false;
        let mut open_index: Option<usize> = None;
        let mut menu_index: Option<usize> = None;
        let mut menu_pos: Option<egui::Pos2> = None;
        let mut navigate_dir: Option<PathBuf> = None;
        let mut navigate_back = false;
        let mut navigate_forward = false;
        let mut open_folder_browser = false;

        let colors = self.theme();
        let current_dir = self.current_folder();

        // Keep path buffer in sync with active folder when not in raw text edit mode.
        if !self.grid_path_editing {
            self.grid_path_buffer = current_dir
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
        }

        // Global shortcut Ctrl+L: focus the path bar for direct path entry in grid view.
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::L)) {
            self.grid_path_editing = true;
            self.grid_path_invalid = false;
            self.grid_path_focus_pending = true;
            if let Some(dir) = &current_dir {
                self.grid_path_buffer = dir.display().to_string();
            }
        }

        // Global shortcuts for folder navigation (suppressed while editing raw path or modal open)
        if !self.grid_path_editing && !self.modal_open() {
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
                            let folder_count = if self.config.multiview_show_folders {
                                self.multiview_subfolders.len()
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

                                // Left-side controls occupy the remaining space
                                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(2.0, 0.0);

                            // 1. Back button (chevron-left)
                            let can_back = !self.folder_back_stack.is_empty();
                            let back_tooltip = if let Some(p) = self.folder_back_stack.last() {
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
                            let can_forward = !self.folder_forward_stack.is_empty();
                            let forward_tooltip = if let Some(p) = self.folder_forward_stack.last() {
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
                            let frame_stroke = if self.grid_path_invalid {
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

                                        if self.grid_path_editing {
                                            // Raw path edit mode
                                            let mut output = egui::TextEdit::singleline(
                                                &mut self.grid_path_buffer,
                                            )
                                            .desired_width(f32::INFINITY)
                                            .frame(false)
                                            .hint_text("Enter folder path...")
                                            .text_color(if self.grid_path_invalid {
                                                ERROR_COLOR
                                            } else {
                                                colors.primary
                                            })
                                            .show(ui);

                                            if output.response.changed() {
                                                self.grid_path_invalid = false;
                                            }

                                            if self.grid_path_focus_pending {
                                                output.response.request_focus();
                                                let len = self.grid_path_buffer.len();
                                                let range = egui::text::CCursorRange::two(
                                                    egui::text::CCursor::new(0),
                                                    egui::text::CCursor::new(len),
                                                );
                                                output.state.cursor.set_char_range(Some(range));
                                                output.state.store(ui.ctx(), output.response.id);
                                                self.grid_path_focus_pending = false;
                                            }

                                            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                                            let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                                            if enter {
                                                let path = viewer::folder_browser::normalize_path(
                                                    &self.grid_path_buffer,
                                                );
                                                if path.is_dir() {
                                                    navigate_dir = Some(path);
                                                    self.grid_path_editing = false;
                                                    self.grid_path_invalid = false;
                                                } else {
                                                    self.grid_path_invalid = true;
                                                    self.show_status(format!(
                                                        "⚠️ Folder does not exist: {}",
                                                        path.display()
                                                    ));
                                                }
                                            } else if esc || (output.response.lost_focus() && !self.grid_path_invalid) {
                                                self.grid_path_editing = false;
                                                self.grid_path_invalid = false;
                                                if let Some(dir) = &current_dir {
                                                    self.grid_path_buffer = dir.display().to_string();
                                                } else {
                                                    self.grid_path_buffer.clear();
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
                                                self.grid_path_editing = true;
                                                if let Some(dir) = &current_dir {
                                                    self.grid_path_buffer = dir.display().to_string();
                                                } else {
                                                    self.grid_path_buffer.clear();
                                                }
                                                self.grid_path_invalid = false;
                                                self.grid_path_focus_pending = true;
                                            }
                                        }
                                    });
                                });
                        });
                    });
                });
            });
            ui.separator();

                let folder_count = if self.config.multiview_show_folders {
                    self.multiview_subfolders.len()
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
                let tile_w = self.grid_tile_width();
                let tile_h = self.grid_tile_height();
                let avail_w = ui.available_width();
                self.grid_available_width = avail_w;
                self.grid_spacing_y = ui.spacing().item_spacing.y;
                let columns = ((avail_w - 12.0) / tile_w).floor().max(1.0) as usize;
                self.grid_columns = columns;
                let row_height = tile_h;
                let total_rows = if total_items == 0 { 0 } else { (total_items + columns - 1) / columns };

                let needed_lod = loader::PreviewLod::from_tile_width(tile_w, ui.ctx().pixels_per_point());

                // Boundary-aware auto-scroll: bring the selected card into view
                // only when it falls outside the current viewport.
                egui::ScrollArea::vertical()
                    .id_source("grid-scroll")
                    .auto_shrink([false, false])
                    .show_rows(ui, row_height, total_rows, |ui, row_range| {
                        if self.scroll_to_selected {
                            self.scroll_to_selected = false;
                            if total_rows > 0 {
                                let spacing = ui.spacing().item_spacing;
                                let min_row = row_range.start;
                                let unified_idx = match self.grid_selected_folder {
                                    Some(f) if f < folder_count => f,
                                    _ => {
                                        if len > 0 {
                                            folder_count + self.current_index.min(len - 1)
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
                                        if let Some(action) = self.grid_folder_tile(ui, item_idx) {
                                            match action {
                                                GridFolderAction::Select(idx) => {
                                                    self.grid_selected_folder = Some(idx);
                                                }
                                                GridFolderAction::Open(idx) => {
                                                    if let Some(folder) = self.multiview_subfolders.get(idx) {
                                                        navigate_dir = Some(folder.path.clone());
                                                    }
                                                }
                                                GridFolderAction::ContextMenu(_idx, _pos) => {}
                                            }
                                        }
                                    } else {
                                        // Image item
                                        let img_idx = item_idx - folder_count;
                                        self.request_thumb_for(img_idx, needed_lod);
                                        if let Some(action) = self.grid_tile(ui, img_idx) {
                                            match action {
                                                GridTileAction::Select(i) => {
                                                    self.grid_selected_folder = None;
                                                    self.grid_select(i);
                                                }
                                                GridTileAction::Open(i) => {
                                                    self.grid_selected_folder = None;
                                                    open_index = Some(i);
                                                }
                                                GridTileAction::ContextMenu(i, pos) => {
                                                    self.grid_selected_folder = None;
                                                    menu_index = Some(i);
                                                    menu_pos = Some(pos);
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
            self.current_index = index;
            self.leave_grid(ctx);
            return;
        }
        if let Some(index) = menu_index {
            self.current_index = index;
            self.context_menu_pos = menu_pos;
        }
        if back_clicked {
            if !self.files.is_empty() {
                self.leave_grid(ctx);
            } else {
                self.show_status("No image to view in this folder");
            }
            return;
        }
        if navigate_back {
            self.navigate_folder_back(ctx);
            return;
        }
        if navigate_forward {
            self.navigate_folder_forward(ctx);
            return;
        }
        if open_folder_browser {
            self.grid_path_editing = false;
            self.grid_path_invalid = false;
            self.open_folder_dialog();
            return;
        }
        if let Some(path) = navigate_dir {
            if path.is_dir() {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                self.open_directory(path.clone(), ctx);
                self.scroll_to_selected = true;
                self.grid_selected_folder = None;
                self.grid_path_buffer = path.display().to_string();
                self.grid_path_editing = false;
                self.grid_path_invalid = false;
                let count = self.files.len();
                let f_count = self.multiview_subfolders.len();
                if count > 0 && f_count > 0 {
                    self.show_success(format!("{name} ({f_count} folders, {count} images)"));
                } else if count > 0 {
                    self.show_success(format!("{name} ({count} images)"));
                } else if f_count > 0 {
                    self.show_success(format!("{name} ({f_count} folders)"));
                } else {
                    self.show_success(format!("{name} (folder is empty)"));
                }
                self.show_success(format!(
                    "{} Opened folder \"{}\" ({} image{})",
                    icons::ICON_IMAGE,
                    name,
                    count,
                    if count == 1 { "" } else { "s" }
                ));
            } else {
                self.show_status(format!("⚠️ Folder does not exist: {}", path.display()));
            }
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
        let move_target_label = self
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
        let _ = std::mem::take(&mut self.help_opened_this_frame);
        let mut open = self.show_help;
        let mut open_about = false;
        let mut done = false;
        let colors = self.theme();

        let modal_frame = egui::Frame::none()
            .fill(colors.panel_fill)
            .stroke(egui::Stroke::new(1.0_f32, colors.border))
            .rounding(10.0)
            .inner_margin(egui::Margin::symmetric(14.0, 12.0))
            .shadow(egui::Shadow {
                offset: egui::vec2(0.0, 4.0),
                blur: 18.0,
                spread: 0.0,
                color: egui::Color32::from_black_alpha(150),
            });

        egui::Window::new(
            egui::RichText::new(format!("{}  Help & Shortcuts", icons::ICON_INFO)).size(13.0),
        )
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size(egui::vec2(550.0, 500.0))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .frame(modal_frame)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_source("help-scroll")
                    .max_height(420.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        settings_card(ui, &colors, Some("Navigation & Browsing"), |ui| {
                            shortcut_table(
                                "help-grid-nav",
                                ui,
                                &colors,
                                &[
                                    ("Previous / Next image", "←  →  /  A  D  /  Wheel"),
                                    ("First / Last image", "Home  /  End"),
                                    ("Toggle Multi-View Grid", "Enter  /  Double-click"),
                                    ("Focus address bar", "Ctrl + L"),
                                    ("Go to parent folder", "Backspace  /  Alt + ↑"),
                                    ("Folder History: Back", "Alt + ←"),
                                    ("Folder History: Forward", "Alt + →"),
                                    ("Preview size: bigger", "Ctrl + Wheel Up  /  Ctrl + +"),
                                    ("Preview size: smaller", "Ctrl + Wheel Down  /  Ctrl + −"),
                                    ("Preview size: reset", "Ctrl + 0"),
                                    ("Refresh folder", "F5"),
                                ],
                            );
                        });
                        ui.add_space(8.0);

                        settings_card(ui, &colors, Some("Viewing & Zoom"), |ui| {
                            shortcut_table(
                                "help-grid-zoom",
                                ui,
                                &colors,
                                &[
                                    ("Zoom in / out", "Ctrl + Wheel"),
                                    ("Fit to window", "0"),
                                    ("Actual size (100%)", "1"),
                                    ("Pan (when zoomed)", "Left-drag"),
                                    ("Rotate 90° clockwise", "R"),
                                    ("Rotate 90° counter-clockwise", "Ctrl + R"),
                                    ("Toggle filmstrip", "T"),
                                    ("Toggle fullscreen", "F11  /  Alt + Enter"),
                                ],
                            );
                        });
                        ui.add_space(8.0);

                        settings_card(ui, &colors, Some("File Operations & Tools"), |ui| {
                            shortcut_table(
                                "help-grid-file",
                                ui,
                                &colors,
                                &[
                                    ("Image properties / EXIF", "I"),
                                    ("Resize image", "Ctrl + Shift + R"),
                                    ("Rename file", "F2"),
                                    ("Copy file path", "Ctrl + C"),
                                    ("Copy image to clipboard", "Ctrl + Shift + C"),
                                    ("Save rotation", "Ctrl + S"),
                                    ("Save image as…", "Ctrl + Shift + S"),
                                    ("Print image…", "Ctrl + P"),
                                    ("Move to Recycle Bin", "Del"),
                                    ("Show in File Explorer", "Ctrl + E"),
                                ],
                            );
                        });
                        ui.add_space(8.0);

                        settings_card(ui, &colors, Some("General & Playback"), |ui| {
                            shortcut_table(
                                "help-grid-general",
                                ui,
                                &colors,
                                &[
                                    ("Open file", "Ctrl + O"),
                                    ("Open folder", "Ctrl + Shift + O"),
                                    ("Close image", "Ctrl + W"),
                                    ("Close folder", "Ctrl + Shift + W"),
                                    ("Play / Pause animation", "Space"),
                                    ("Step animation frame", "[  /  ]"),
                                    ("Settings", "Ctrl + ,"),
                                    ("Help & Shortcuts", "F1  /  H"),
                                    ("Close / Dismiss", "Esc"),
                                ],
                            );
                        });
                    });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    if colors
                        .button_secondary(
                            ui,
                            &format!("{}  About Photonio", icons::ICON_INFO),
                            egui::vec2(130.0, 24.0),
                            true,
                        )
                        .clicked()
                    {
                        open_about = true;
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if colors
                            .button_primary(ui, "Done", egui::vec2(80.0, 24.0), true)
                            .clicked()
                        {
                            done = true;
                        }
                    });
                });
            });

        self.show_help = open;
        if done {
            self.show_help = false;
        }
        if open_about {
            self.open_about();
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
                            let selected = self.settings_tab == tab;
                            let response = colors.sidebar_tab(
                                ui,
                                icon,
                                label,
                                selected,
                                egui::vec2(135.0, 30.0),
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
                        .max_height(content_height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                                match self.settings_tab {
                                    SettingsTab::Appearance => {
                                        changed |= self.settings_appearance(ui, &colors);
                                    }
                                    SettingsTab::Filmstrip => {
                                        changed |= self.settings_filmstrip(ui, &colors);
                                    }
                                    SettingsTab::QuickPills => {
                                        changed |= self.settings_quick_pills(ui, &colors);
                                    }
                                    SettingsTab::Behavior => {
                                        changed |= self.settings_behavior(ui, &colors);
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
        let launch_count = self.config.launch_count;
        self.config = config::Config::default();
        self.config.last_directory = dir;
        self.config.last_file_path = file;
        self.config.window_pos = pos;
        self.config.window_size = size;
        self.config.launch_count = launch_count;
        self.save_config();
        self.show_success("Settings reset to defaults");
    }

    /// Settings → Appearance: theme, backdrop, glass tint, HUD, overlay fade.
    fn settings_appearance(&mut self, ui: &mut egui::Ui, colors: &theme::ThemeColors) -> bool {
        let mut changed = false;

        // Card 1: Theme & Background.
        settings_card(ui, colors, Some("Theme & Background"), |ui| {
            let mut light = self.config.canvas_background == config::CanvasBackground::Light;
            let mut glass = self.config.canvas_background == config::CanvasBackground::Glass;

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
                        .add(egui::Slider::new(&mut self.config.glass_opacity, 100..=255))
                        .changed();
                    ui.end_row();
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
        settings_card(ui, colors, Some("Screen Overlays"), |ui| {
            egui::Grid::new("appearance_overlays_grid")
                .num_columns(2)
                .spacing(egui::vec2(16.0, 8.0))
                .show(ui, |ui| {
                    ui.label("Navigation HUD:");
                    ui.horizontal(|ui| {
                        if colors.segmented_value(ui, &mut self.config.hud_position, config::HudPosition::Bottom, "Bottom").clicked() {
                            changed = true;
                        }
                        if colors.segmented_value(ui, &mut self.config.hud_position, config::HudPosition::Top, "Top").clicked() {
                            changed = true;
                        }
                        if colors.segmented_value(ui, &mut self.config.hud_position, config::HudPosition::Hidden, "Hidden").clicked() {
                            changed = true;
                        }
                    });
                    ui.end_row();
                });

            ui.add_space(4.0);
            changed |= ui
                .checkbox(&mut self.config.auto_fade_overlays, "Auto-fade overlays when idle")
                .changed();
            if self.config.auto_fade_overlays {
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
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
    fn settings_filmstrip(&mut self, ui: &mut egui::Ui, colors: &theme::ThemeColors) -> bool {
        let mut changed = false;

        // Card 1: master toggle.
        settings_card(ui, colors, Some("Thumbnail Filmstrip"), |ui| {
            changed |= ui
                .checkbox(&mut self.config.show_filmstrip, "Show Thumbnail Filmstrip (T)")
                .changed();
        });
        ui.add_space(6.0);

        // Card 2: Layout & Options (enabled when the filmstrip is on).
        settings_card(ui, colors, Some("Layout & Capacity"), |ui| {
            ui.add_enabled_ui(self.config.show_filmstrip, |ui| {
                egui::Grid::new("filmstrip_layout_grid")
                    .num_columns(2)
                    .spacing(egui::vec2(16.0, 8.0))
                    .show(ui, |ui| {
                        ui.label("Dock position:");
                        ui.horizontal(|ui| {
                            if colors.segmented_value(ui, &mut self.config.filmstrip_position, config::FilmstripPosition::Bottom, "Bottom").clicked() {
                                changed = true;
                            }
                            if colors.segmented_value(ui, &mut self.config.filmstrip_position, config::FilmstripPosition::Top, "Top").clicked() {
                                changed = true;
                            }
                            if colors.segmented_value(ui, &mut self.config.filmstrip_position, config::FilmstripPosition::Left, "Left").clicked() {
                                changed = true;
                            }
                            if colors.segmented_value(ui, &mut self.config.filmstrip_position, config::FilmstripPosition::Right, "Right").clicked() {
                                changed = true;
                            }
                        });
                        ui.end_row();

                        ui.label("Visible Thumbnails:");
                        ui.horizontal(|ui| {
                            for &count in &[7usize, 9, 11] {
                                if colors.segmented_value(ui, &mut self.config.filmstrip_count, count, &count.to_string()).clicked() {
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
    fn settings_quick_pills(&mut self, ui: &mut egui::Ui, colors: &theme::ThemeColors) -> bool {
        let mut changed = false;

        // Card 1: master toggle.
        settings_card(ui, colors, Some("Quick-Action Pill"), |ui| {
            changed |= ui
                .checkbox(&mut self.config.quick_pill_enabled, "Enable Top-Right Quick-Action Pill")
                .changed();
        });
        ui.add_space(6.0);

        // Card 2: Layout & button selection.
        settings_card(ui, colors, Some("Layout & Visible Buttons"), |ui| {
            ui.add_enabled_ui(self.config.quick_pill_enabled, |ui| {
                egui::Grid::new("quick_pill_orientation_grid")
                    .num_columns(2)
                    .spacing(egui::vec2(16.0, 8.0))
                    .show(ui, |ui| {
                        ui.label("Orientation:");
                        ui.horizontal(|ui| {
                            if colors.segmented_value(ui, &mut self.config.quick_pill_orientation, config::Orientation::Horizontal, "Horizontal").clicked() {
                                changed = true;
                            }
                            if colors.segmented_value(ui, &mut self.config.quick_pill_orientation, config::Orientation::Vertical, "Vertical").clicked() {
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
        changed
    }

    /// Settings → Behavior: wheel action and startup/browsing options.
    fn settings_behavior(&mut self, ui: &mut egui::Ui, colors: &theme::ThemeColors) -> bool {
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
                                &mut self.config.mouse_wheel_action,
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
                                &mut self.config.mouse_wheel_action,
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
                    &mut self.config.loop_navigation,
                    "Loop navigation at folder boundary",
                )
                .changed();
        });
        ui.add_space(6.0);

        // Card 2: Startup & Window.
        settings_card(ui, colors, Some("Startup & Window"), |ui| {
            changed |= ui
                .checkbox(
                    &mut self.config.remember_last_folder,
                    "Remember last viewed image and folder on startup",
                )
                .changed();
            changed |= ui
                .checkbox(&mut self.config.start_fullscreen, "Start in Fullscreen")
                .changed();
        });
        ui.add_space(6.0);

        // Card 3: Multi-View & Navigation.
        settings_card(ui, colors, Some("Multi-View & Grid Navigation"), |ui| {
            let mut show_folders = self.config.multiview_show_folders;
            if ui
                .checkbox(
                    &mut show_folders,
                    "View folders on multi-view",
                )
                .on_hover_text("Show subfolders with 4-image quad previews inside the multi-view grid")
                .changed()
            {
                self.config.multiview_show_folders = show_folders;
                changed = true;
                self.refresh_multiview_subfolders(ui.ctx());
            }

            changed |= ui
                .checkbox(
                    &mut self.config.adjustable_grid_tile_size,
                    "Allow Ctrl + Wheel to adjust preview grid tile size",
                )
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

        // Card 1: Default App Handler.
        settings_card(ui, colors, Some("Default App Handler"), |ui| {
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
            } else if self.folder_menu_registered {
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
            .inner_margin(egui::Margin::symmetric(14.0, 12.0))
            .shadow(egui::Shadow {
                offset: egui::vec2(0.0, 4.0),
                blur: 18.0,
                spread: 0.0,
                color: egui::Color32::from_black_alpha(150),
            });

        egui::Window::new(
            egui::RichText::new(format!("{}  Image Properties", icons::ICON_INFO)).size(13.0),
        )
            .open(&mut open)
            .fixed_size(egui::vec2(540.0, 500.0))
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .frame(modal_frame)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_source("image-info-scroll")
                    .max_height(420.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        settings_card(ui, &colors, Some("General Details"), |ui| {
                            property_grid("image_props_general", ui, &meta.general, &colors);
                        });
                        ui.add_space(8.0);

                        settings_card(ui, &colors, Some("File Details"), |ui| {
                            property_grid("image_props_file", ui, &meta.file_details, &colors);
                        });
                        ui.add_space(8.0);

                        settings_card(ui, &colors, Some("EXIF / Camera Metadata"), |ui| {
                            if meta.has_exif {
                                property_grid("image_props_exif", ui, &meta.exif, &colors);
                            } else {
                                ui.label(
                                    egui::RichText::new("No EXIF camera metadata embedded in this image")
                                        .italics()
                                        .size(12.0)
                                        .color(colors.muted),
                                );
                            }
                        });
                        ui.add_space(8.0);

                        settings_card(ui, &colors, Some("Location & Paths"), |ui| {
                            ui.horizontal(|ui| {
                                if colors
                                    .button_secondary(
                                        ui,
                                        &format!("{} Copy", icons::ICON_COPY),
                                        egui::vec2(60.0, 22.0),
                                        true,
                                    )
                                    .on_hover_text("Copy full file path")
                                    .clicked()
                                {
                                    copy_file = true;
                                }
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&file_path)
                                            .size(12.0)
                                            .color(colors.primary),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(&file_path);
                            });
                            ui.add_space(3.0);
                            ui.horizontal(|ui| {
                                if colors
                                    .button_secondary(
                                        ui,
                                        &format!("{} Open", icons::ICON_OPEN_FOLDER),
                                        egui::vec2(60.0, 22.0),
                                        true,
                                    )
                                    .on_hover_text("Open containing folder in File Explorer")
                                    .clicked()
                                {
                                    open_folder = true;
                                }
                                if colors
                                    .button_secondary(
                                        ui,
                                        &format!("{} Copy", icons::ICON_COPY),
                                        egui::vec2(60.0, 22.0),
                                        true,
                                    )
                                    .on_hover_text("Copy folder path")
                                    .clicked()
                                {
                                    copy_folder = true;
                                }
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&folder_path)
                                            .size(12.0)
                                            .color(colors.muted),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(&folder_path);
                            });
                        });
                    });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if colors
                        .button_primary(ui, "Done", egui::vec2(80.0, 24.0), true)
                        .clicked()
                    {
                        done = true;
                    }
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

    /// Modal dialog for renaming the active file.
    fn draw_rename_dialog(&mut self, ctx: &egui::Context) {
        let colors = self.theme();
        let Some(state) = &mut self.rename_state else {
            return;
        };
        let orig_name = state
            .old_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let trimmed = state.new_name.trim().to_string();
        let validation_error = validate_file_name(&state.new_name, &state.old_path).err();
        let has_error = validation_error.is_some();
        let target_path = state.old_path.with_file_name(&trimmed);
        let is_unsupported_ext = !trimmed.is_empty()
            && !has_error
            && !filesystem::is_supported_path(&target_path);

        let mut open = true;
        let mut should_commit = false;
        let mut should_cancel = false;

        let modal_frame = egui::Frame::none()
            .fill(colors.panel_fill)
            .stroke(egui::Stroke::new(1.0_f32, colors.border))
            .rounding(10.0)
            .inner_margin(egui::Margin::symmetric(14.0, 12.0))
            .shadow(egui::Shadow {
                offset: egui::vec2(0.0, 4.0),
                blur: 18.0,
                spread: 0.0,
                color: egui::Color32::from_black_alpha(150),
            });

        egui::Window::new("Rename File")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size(egui::vec2(360.0, 140.0))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .frame(modal_frame)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(icons::ICON_INFO).color(colors.muted));
                    ui.label(
                        egui::RichText::new(format!("Current: {}", orig_name))
                            .color(colors.muted)
                            .size(12.0),
                    );
                });
                ui.add_space(6.0);

                let input_fill = if colors.dark_mode {
                    egui::Color32::from_black_alpha(60)
                } else {
                    egui::Color32::from_black_alpha(10)
                };
                let frame_stroke = if has_error {
                    egui::Stroke::new(1.0_f32, ERROR_COLOR)
                } else {
                    egui::Stroke::new(1.0_f32, colors.border)
                };

                egui::Frame::none()
                    .fill(input_fill)
                    .stroke(frame_stroke)
                    .rounding(4.0)
                    .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        let mut output = egui::TextEdit::singleline(&mut state.new_name)
                            .desired_width(f32::INFINITY)
                            .hint_text("New file name")
                            .text_color(if has_error {
                                ERROR_COLOR
                            } else {
                                colors.primary
                            })
                            .show(ui);

                        if state.focus_pending {
                            output.response.request_focus();
                            let stem_chars = Path::new(&state.new_name)
                                .file_stem()
                                .map(|s| s.to_string_lossy().chars().count())
                                .unwrap_or_else(|| state.new_name.chars().count());
                            let range = egui::text::CCursorRange::two(
                                egui::text::CCursor::new(0),
                                egui::text::CCursor::new(stem_chars),
                            );
                            output.state.cursor.set_char_range(Some(range));
                            output.state.store(ui.ctx(), output.response.id);
                            state.focus_pending = false;
                        }
                    });

                ui.add_space(4.0);
                if let Some(err) = &validation_error {
                    ui.label(egui::RichText::new(err).color(ERROR_COLOR).size(12.0));
                } else if is_unsupported_ext {
                    ui.label(
                        egui::RichText::new("⚠️ Warning: Extension is not a supported image format")
                            .color(WARN_COLOR)
                            .size(12.0),
                    );
                } else {
                    ui.label(egui::RichText::new(" ").size(12.0));
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let can_commit = !has_error;
                        let commit_btn = ui.add_enabled(can_commit, egui::Button::new("Rename"));
                        let cancel_btn = ui.button("Cancel");

                        if cancel_btn.clicked() {
                            should_cancel = true;
                        } else if commit_btn.clicked() && can_commit {
                            should_commit = true;
                        }
                    });
                });

                // Enter commits if valid, Escape cancels
                let enter_hit = ui.input(|i| i.key_pressed(egui::Key::Enter));
                let esc_hit = ui.input(|i| i.key_pressed(egui::Key::Escape));

                if esc_hit {
                    should_cancel = true;
                } else if enter_hit && !has_error {
                    should_commit = true;
                }
            });

        if !open || should_cancel {
            self.rename_state = None;
        } else if should_commit {
            self.commit_rename(&trimmed, ctx);
        }
    }

    fn execute_menu_action(&mut self, action: MenuAction, ctx: &egui::Context) {
        match action {
            MenuAction::OpenFile => self.open_file_dialog(ctx),
            MenuAction::OpenFolder => self.open_folder_dialog(),
            MenuAction::CloseImage => self.close_current_file(ctx),
            MenuAction::CloseFolder => self.close_folder(),
            MenuAction::ResizeImage => self.open_resize_dialog(),
            MenuAction::SaveRotation => self.save_rotation_overwrite(ctx),
            MenuAction::SaveAs => self.save_current_as(),
            MenuAction::OpenContainingFolder => self.reveal_current(),
            MenuAction::OpenWith => self.open_with_current(),
            MenuAction::CopyFileTo => self.copy_file_to(ctx),
            MenuAction::MoveFileTo => self.move_file_to(ctx),
            MenuAction::CopyFile => self.copy_current_file(),
            MenuAction::CopyImage => self.copy_current_image(),
            MenuAction::CopyPath => self.copy_current_path(ctx),
            MenuAction::Rename => self.open_rename_dialog(),
            MenuAction::SetWallpaper => self.set_wallpaper_current(),
            MenuAction::Delete => self.delete_current(ctx),
            MenuAction::Settings => self.open_settings(),
            MenuAction::About => self.open_about(),
            MenuAction::Help => self.open_help(),
            MenuAction::Exit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }
}

/// Render a clean, rounded, consistent "card" (grouped frame) for the Settings content.
/// Enforces full available width and unified margins/borders across all tabs.
fn settings_card<R>(
    ui: &mut egui::Ui,
    colors: &theme::ThemeColors,
    title: Option<&str>,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let frame = egui::Frame::none()
        .fill(if colors.dark_mode {
            egui::Color32::from_rgba_unmultiplied(13, 13, 17, 230)
        } else {
            egui::Color32::from_black_alpha(8)
        })
        .stroke(egui::Stroke::new(
            1.0_f32,
            if colors.dark_mode {
                egui::Color32::from_white_alpha(20)
            } else {
                egui::Color32::from_black_alpha(24)
            },
        ))
        .rounding(6.0)
        .inner_margin(egui::Margin::symmetric(14.0, 9.0));

    frame.show(ui, |ui| {
        // Enforce consistent card width across all tabs
        ui.set_min_width(ui.available_width());
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 5.0);

        if let Some(t) = title {
            ui.label(
                egui::RichText::new(t)
                    .strong()
                    .size(13.0)
                    .color(colors.primary),
            );
            ui.add_space(2.0);
        }

        add_contents(ui)
    }).inner
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
fn property_grid(
    id: &str,
    ui: &mut egui::Ui,
    rows: &[(&'static str, String)],
    colors: &theme::ThemeColors,
) {
    egui::Grid::new(id)
        .striped(true)
        .spacing([16.0, 5.0])
        .num_columns(2)
        .show(ui, |ui| {
            for (key, value) in rows {
                ui.label(
                    egui::RichText::new(*key)
                        .size(12.0)
                        .color(colors.muted),
                );
                ui.label(
                    egui::RichText::new(value)
                        .size(12.0)
                        .color(colors.primary),
                );
                ui.end_row();
            }
        });
}

/// Render a clean two-column grid of keyboard shortcuts with <kbd> pill badges.
fn shortcut_table(
    id: &str,
    ui: &mut egui::Ui,
    colors: &theme::ThemeColors,
    rows: &[(&str, &str)],
) {
    let pill_fill = if colors.dark_mode {
        egui::Color32::from_white_alpha(14)
    } else {
        egui::Color32::from_black_alpha(10)
    };
    let pill_stroke = egui::Stroke::new(
        1.0_f32,
        if colors.dark_mode {
            egui::Color32::from_white_alpha(24)
        } else {
            egui::Color32::from_black_alpha(20)
        },
    );

    egui::Grid::new(id)
        .num_columns(2)
        .spacing(egui::vec2(20.0, 6.0))
        .show(ui, |ui| {
            for (action, keys) in rows {
                ui.label(
                    egui::RichText::new(*action)
                        .size(12.5)
                        .color(colors.primary),
                );

                egui::Frame::none()
                    .fill(pill_fill)
                    .stroke(pill_stroke)
                    .rounding(4.0)
                    .inner_margin(egui::Margin::symmetric(7.0, 2.5))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(*keys)
                                .monospace()
                                .size(11.5)
                                .color(colors.primary),
                        );
                    });
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

/// A compact, flat glyph button for the quick-action pill: a strict square
/// hit-box with the icon dead-centered.
fn pill_button(
    ui: &mut egui::Ui,
    glyph: &str,
    tooltip: &str,
    highlighted: bool,
    highlight_color: egui::Color32,
) -> egui::Response {
    let mut text = egui::RichText::new(glyph).size(18.0);
    if highlighted {
        text = text.color(highlight_color);
    }
    ui.add_sized(
        egui::vec2(32.0, 32.0),
        egui::Button::new(text)
            .frame(false)
            .min_size(egui::vec2(32.0, 32.0)),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .on_hover_text(tooltip)
}

/// Compact glyph button for the quick-action pill with a smooth, pulsing breathing
/// animation effect for high-priority pending actions (like pending rotation save).
fn pill_button_breathing(
    ui: &mut egui::Ui,
    glyph: &str,
    tooltip: &str,
    _colors: &theme::ThemeColors,
) -> egui::Response {
    let time = ui.input(|i| i.time);
    let wave = (time * 3.5).sin() as f32; // ~1.8 second smooth breath cycle
    let breath = wave * 0.5 + 0.5; // 0.0 to 1.0 (breath in, breath out)

    // Breathing glow fill and stroke in warm gold/amber - subtle, elegant, low outer glow
    let fill_alpha = (14.0 + 20.0 * breath) as u8;
    let stroke_alpha = (30.0 + 40.0 * breath) as u8;
    let fill = egui::Color32::from_rgba_unmultiplied(255, 195, 45, fill_alpha);
    let stroke = egui::Stroke::new(
        1.0_f32,
        egui::Color32::from_rgba_unmultiplied(255, 205, 55, stroke_alpha),
    );

    let icon_size = 16.5;
    let icon_color = egui::Color32::from_rgb(255, (195.0 + 35.0 * breath) as u8, 45);

    let (rect, response) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::click());
    let painter = ui.painter();

    // Draw breathing rounded card background & stroke
    painter.rect_filled(rect, 6.0, fill);
    painter.rect_stroke(rect, 6.0, stroke);

    // Draw centered icon
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(icon_size),
        icon_color,
    );

    // Repaint to keep breathing animation smoothly running at 60 FPS
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));

    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
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

/// UV rect (0..1) that center-crops `dims` to cover `slot`.
fn cover_uv(dims: [usize; 2], slot: egui::Rect) -> egui::Rect {
    let img_w = dims[0].max(1) as f32;
    let img_h = dims[1].max(1) as f32;
    let scale = (slot.width() / img_w).max(slot.height() / img_h);
    let fx = (slot.width() / (img_w * scale)).min(1.0);
    let fy = (slot.height() / (img_h * scale)).min(1.0);
    egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(fx, fy))
}

/// Decode + downscale an image to `max_side` for MultiView folder quad previews.
fn decode_quad_thumb(path: &Path, max_side: u32) -> Option<(Vec<u8>, [usize; 2])> {
    use std::io::Read;
    if let Ok(mut file) = std::fs::File::open(path) {
        let mut prefix = vec![0u8; 128 * 1024];
        if let Ok(n) = file.read(&mut prefix) {
            prefix.truncate(n);
            if let Some(thumb) = viewer::embedded_thumbnail(&prefix, max_side) {
                let dims = [thumb.width() as usize, thumb.height() as usize];
                return Some((thumb.to_rgba8().into_raw(), dims));
            }
        }
    }
    let bytes = std::fs::read(path).ok()?;
    let thumb = viewer::decode_thumbnail(&bytes, max_side).ok()?;
    let dims = [thumb.width() as usize, thumb.height() as usize];
    Some((thumb.to_rgba8().into_raw(), dims))
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
        Some("jpg") | Some("jpeg") | Some("jfif") => "JPEG".into(),
        Some("webp") => "WebP".into(),
        Some("bmp") => "BMP".into(),
        Some("gif") => "GIF".into(),
        Some("ico") => "ICO".into(),
        Some(other) => other.to_uppercase(),
        None => "Unknown".into(),
    }
}

/// Validate candidate filename against Windows filesystem rules and collisions.
fn validate_file_name(name: &str, old_path: &Path) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("File name cannot be empty".to_string());
    }
    if trimmed.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*'])
        || trimmed.chars().any(|c| c.is_control())
    {
        return Err("Name contains invalid characters (< > : \" / \\ | ? *)".to_string());
    }
    if trimmed.ends_with('.') || trimmed.ends_with(' ') {
        return Err("Name cannot end with a period or space".to_string());
    }
    let stem = trimmed.split('.').next().unwrap_or(trimmed);
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
        "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        return Err(format!("\"{stem}\" is a reserved Windows device name"));
    }
    let orig_name = old_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if orig_name == trimmed {
        return Ok(trimmed.to_string());
    }
    let is_case_change = orig_name.eq_ignore_ascii_case(trimmed);
    let target = old_path.with_file_name(trimmed);
    if !is_case_change && target.exists() {
        return Err(format!("A file named \"{trimmed}\" already exists"));
    }
    Ok(trimmed.to_string())
}

/// Rename `old_path` to `new_path` safely on Windows, properly handling case-only changes.
fn safe_rename(old_path: &Path, new_path: &Path) -> std::io::Result<()> {
    let old_name = old_path.file_name().unwrap_or_default().to_string_lossy();
    let new_name = new_path.file_name().unwrap_or_default().to_string_lossy();
    if old_name == new_name {
        return Ok(());
    }
    if old_name.eq_ignore_ascii_case(&new_name) {
        // Case-only rename: use a temp name so Windows NTFS updates the casing reliably.
        let temp_name = format!("{}.{}.photonio_tmp", old_name, std::process::id());
        let temp_path = old_path.with_file_name(temp_name);
        std::fs::rename(old_path, &temp_path)?;
        if let Err(e) = std::fs::rename(&temp_path, new_path) {
            let _ = std::fs::rename(&temp_path, old_path);
            return Err(e);
        }
        Ok(())
    } else {
        std::fs::rename(old_path, new_path)
    }
}

/// Move entries from `cache` to their new indices according to file path matches,
/// optionally remapping an explicitly renamed item (`(old_idx, new_idx)`).
fn remap_cache_keys_with_rename<T>(
    cache: &mut HashMap<usize, T>,
    old_files: &[PathBuf],
    new_files: &[PathBuf],
    renamed: Option<(usize, usize)>,
) -> HashMap<usize, T> {
    let mut new_cache = HashMap::new();
    for (new_idx, new_path) in new_files.iter().enumerate() {
        if let Some((old_renamed_idx, new_renamed_idx)) = renamed {
            if new_idx == new_renamed_idx {
                if let Some(val) = cache.remove(&old_renamed_idx) {
                    new_cache.insert(new_idx, val);
                    continue;
                }
            }
        }
        if let Some(old_idx) = old_files.iter().position(|p| p == new_path) {
            if let Some(val) = cache.remove(&old_idx) {
                new_cache.insert(new_idx, val);
            }
        }
    }
    new_cache
}

/// Move entries from `cache` to their new indices according to file path matches.
fn remap_cache_keys<T>(
    cache: &mut HashMap<usize, T>,
    old_files: &[PathBuf],
    new_files: &[PathBuf],
) -> HashMap<usize, T> {
    remap_cache_keys_with_rename(cache, old_files, new_files, None)
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

    #[test]
    fn icon_folder_stretched_layout_and_quad_insets() {
        let ctx = egui::Context::default();
        setup_fonts(&ctx);
        let _ = ctx.run(egui::RawInput::default(), |_| {});

        // Test at default grid preview width (thumb_w ~138, thumb_h ~144)
        let thumb_w = 138.0_f32;
        let thumb_h = 144.0_f32;
        let border = 4.0_f32;
        let avail_w = (thumb_w - border * 2.0).max(10.0);
        let avail_h = (thumb_h - border * 2.0).max(10.0);

        // Natural visual aspect ratio is 1.0 : 0.82
        let font_size = avail_w.min(avail_h / 0.82);
        assert!(font_size > 50.0 && font_size <= avail_w);

        let folder_w = font_size;
        let folder_h = font_size * 0.82;
        let thumb_center = egui::pos2(thumb_w * 0.5, thumb_h * 0.5);
        let folder_rect = egui::Rect::from_center_size(thumb_center, egui::vec2(folder_w, folder_h));

        // Quad insets relative to folder_rect
        let quad_left = folder_rect.min.x + folder_rect.width() * 0.125;
        let quad_right = folder_rect.max.x - folder_rect.width() * 0.120;
        let quad_top = folder_rect.min.y + folder_rect.height() * 0.200;
        let quad_bottom = folder_rect.max.y - folder_rect.height() * 0.090;
        let quad_rect = egui::Rect::from_min_max(
            egui::pos2(quad_left, quad_top),
            egui::pos2(quad_right, quad_bottom),
        );

        // Quad must be strictly contained inside the folder bounding box
        assert!(quad_rect.min.x > folder_rect.min.x);
        assert!(quad_rect.max.x < folder_rect.max.x);
        assert!(quad_rect.min.y > folder_rect.min.y);
        assert!(quad_rect.max.y < folder_rect.max.y);
        assert!(quad_rect.width() > 0.0);
        assert!(quad_rect.height() > 0.0);

        // Verify font layout produces expected non-empty galley
        let galley = ctx.fonts(|f| {
            f.layout_no_wrap(
                icons::ICON_FOLDER.to_string(),
                egui::FontId::proportional(font_size),
                egui::Color32::WHITE,
            )
        });
        assert!(galley.size().x > 0.0);
        assert!(galley.size().y > 0.0);
    }

    #[test]
    fn remap_cache_keys_preserves_and_relocates_entries() {
        let old_files = vec![
            PathBuf::from("C:/pics/img1.png"),
            PathBuf::from("C:/pics/img2.png"),
            PathBuf::from("C:/pics/img3.png"),
        ];
        // img0 added at start, img2 deleted, img1 shifted 0->1, img3 shifted 2->2
        let new_files = vec![
            PathBuf::from("C:/pics/img0.png"),
            PathBuf::from("C:/pics/img1.png"),
            PathBuf::from("C:/pics/img3.png"),
        ];
        let mut cache = HashMap::new();
        cache.insert(0, "texture1");
        cache.insert(1, "texture2");
        cache.insert(2, "texture3");

        let remapped = remap_cache_keys(&mut cache, &old_files, &new_files);
        assert_eq!(remapped.get(&0), None);
        assert_eq!(remapped.get(&1), Some(&"texture1"));
        assert_eq!(remapped.get(&2), Some(&"texture3"));
        assert_eq!(remapped.len(), 2);
    }

    #[test]
    fn remap_cache_keys_with_rename_tracks_renamed_item() {
        let old_files = vec![
            PathBuf::from("C:/pics/alpha.png"),
            PathBuf::from("C:/pics/beta.png"),
            PathBuf::from("C:/pics/gamma.png"),
        ];
        // beta.png renamed to zulu.png -> moves from index 1 to index 2
        let new_files = vec![
            PathBuf::from("C:/pics/alpha.png"),
            PathBuf::from("C:/pics/gamma.png"),
            PathBuf::from("C:/pics/zulu.png"),
        ];
        let mut cache = HashMap::new();
        cache.insert(0, "alpha_tex");
        cache.insert(1, "beta_tex");
        cache.insert(2, "gamma_tex");

        let remapped =
            remap_cache_keys_with_rename(&mut cache, &old_files, &new_files, Some((1, 2)));
        assert_eq!(remapped.get(&0), Some(&"alpha_tex"));
        assert_eq!(remapped.get(&1), Some(&"gamma_tex"));
        assert_eq!(remapped.get(&2), Some(&"beta_tex"));
    }

    #[test]
    fn validate_file_name_checks_rules() {
        let original = PathBuf::from("C:/pics/sample.jpg");

        // Empty / whitespace
        assert!(validate_file_name("", &original).is_err());
        assert!(validate_file_name("   ", &original).is_err());

        // Invalid characters
        assert!(validate_file_name("sample?.jpg", &original).is_err());
        assert!(validate_file_name("sam/ple.jpg", &original).is_err());
        assert!(validate_file_name("sam\\ple.jpg", &original).is_err());
        assert!(validate_file_name("sample*.jpg", &original).is_err());
        assert!(validate_file_name("sam<ple>.jpg", &original).is_err());
        assert!(validate_file_name("sample:1.jpg", &original).is_err());
        assert!(validate_file_name("sample|test.jpg", &original).is_err());

        // Trailing dot
        assert!(validate_file_name("sample.jpg.", &original).is_err());

        // Reserved Windows names
        assert!(validate_file_name("con.jpg", &original).is_err());
        assert!(validate_file_name("NUL.png", &original).is_err());
        assert!(validate_file_name("com1.webp", &original).is_err());
        assert!(validate_file_name("lpt3.jpg", &original).is_err());

        // Same name
        assert_eq!(
            validate_file_name("sample.jpg", &original),
            Ok("sample.jpg".to_string())
        );
        assert_eq!(
            validate_file_name("  sample.jpg  ", &original),
            Ok("sample.jpg".to_string())
        );

        // Valid new name
        assert_eq!(
            validate_file_name("vacation_2026.png", &original),
            Ok("vacation_2026.png".to_string())
        );
    }

    #[test]
    fn compute_feedback_alpha_lifecycle() {
        // Fade-in phase (0.0s to 0.2s)
        let alpha_start = compute_feedback_alpha(0.0).unwrap();
        assert!((alpha_start - 0.0).abs() < 1e-4);

        let alpha_mid_in = compute_feedback_alpha(0.1).unwrap();
        assert!((alpha_mid_in - 0.5).abs() < 1e-4);

        let alpha_in_done = compute_feedback_alpha(0.2).unwrap();
        assert!((alpha_in_done - 1.0).abs() < 1e-4);

        // Hold phase (0.2s to 2.5s)
        let alpha_hold_1 = compute_feedback_alpha(1.0).unwrap();
        assert!((alpha_hold_1 - 1.0).abs() < 1e-4);

        let alpha_hold_end = compute_feedback_alpha(2.5).unwrap();
        assert!((alpha_hold_end - 1.0).abs() < 1e-4);

        // Fade-out phase (2.5s to 3.0s)
        let alpha_mid_out = compute_feedback_alpha(2.75).unwrap();
        assert!((alpha_mid_out - 0.5).abs() < 1e-4);

        let alpha_near_end = compute_feedback_alpha(2.95).unwrap();
        assert!((alpha_near_end - 0.1).abs() < 1e-3);

        // Expired (>= 3.0s)
        assert_eq!(compute_feedback_alpha(3.0), None);
        assert_eq!(compute_feedback_alpha(3.5), None);
    }

    #[test]
    fn folder_history_stack_management() {
        let mut back_stack: Vec<PathBuf> = Vec::new();
        let mut forward_stack: Vec<PathBuf> = Vec::new();

        // Helper closures that match PhotonioApp's push/back/forward stack semantics
        let push_history = |cur: PathBuf, target: PathBuf, back: &mut Vec<PathBuf>, fwd: &mut Vec<PathBuf>| {
            if cur != target {
                if back.last() != Some(&cur) {
                    back.push(cur);
                    if back.len() > 50 {
                        back.remove(0);
                    }
                }
                fwd.clear();
            }
        };

        let dir1 = PathBuf::from("C:\\photos\\vacation");
        let dir2 = PathBuf::from("C:\\photos\\work");
        let dir3 = PathBuf::from("C:\\photos\\family");

        // Navigate 1 -> 2
        push_history(dir1.clone(), dir2.clone(), &mut back_stack, &mut forward_stack);
        assert_eq!(back_stack, vec![dir1.clone()]);
        assert!(forward_stack.is_empty());

        // Consecutive navigation to same target should not duplicate
        push_history(dir2.clone(), dir2.clone(), &mut back_stack, &mut forward_stack);
        assert_eq!(back_stack, vec![dir1.clone()]);

        // Navigate 2 -> 3
        push_history(dir2.clone(), dir3.clone(), &mut back_stack, &mut forward_stack);
        assert_eq!(back_stack, vec![dir1.clone(), dir2.clone()]);
        assert!(forward_stack.is_empty());

        // Back: pop dir2, push current dir3 to forward_stack
        let prev = back_stack.pop().unwrap();
        assert_eq!(prev, dir2);
        forward_stack.push(dir3.clone());
        assert_eq!(back_stack, vec![dir1.clone()]);
        assert_eq!(forward_stack, vec![dir3.clone()]);

        // Forward: pop dir3, push current dir2 to back_stack
        let next = forward_stack.pop().unwrap();
        assert_eq!(next, dir3);
        back_stack.push(dir2.clone());
        assert_eq!(back_stack, vec![dir1.clone(), dir2.clone()]);
        assert!(forward_stack.is_empty());

        // Test capacity cap at 50
        let mut big_stack = Vec::new();
        let mut dummy_fwd = Vec::new();
        for i in 0..60 {
            push_history(
                PathBuf::from(format!("C:\\folder_{i}")),
                PathBuf::from(format!("C:\\folder_{}", i + 1)),
                &mut big_stack,
                &mut dummy_fwd,
            );
        }
        assert_eq!(big_stack.len(), 50);
        assert_eq!(big_stack.first(), Some(&PathBuf::from("C:\\folder_10")));
        assert_eq!(big_stack.last(), Some(&PathBuf::from("C:\\folder_59")));
    }

    #[test]
    fn grid_tile_sizing_and_clamping() {
        assert_eq!(PhotonioApp::DEFAULT_GRID_TILE_SIZE, 150.0);
        assert_eq!(PhotonioApp::MIN_GRID_TILE_SIZE, 80.0);
        assert_eq!(PhotonioApp::MAX_GRID_TILE_SIZE, 500.0);

        // Standard label row height offset
        let h = PhotonioApp::DEFAULT_GRID_TILE_SIZE + 26.0;
        assert!((h - 176.0).abs() < 1e-4);

        // Clamping checks
        let clamped_low = 50.0_f32.clamp(PhotonioApp::MIN_GRID_TILE_SIZE, PhotonioApp::MAX_GRID_TILE_SIZE);
        assert_eq!(clamped_low, 80.0);

        let clamped_high = 700.0_f32.clamp(PhotonioApp::MIN_GRID_TILE_SIZE, PhotonioApp::MAX_GRID_TILE_SIZE);
        assert_eq!(clamped_high, 500.0);
    }

    #[test]
    fn preview_lod_selection_and_target_sizes() {
        use crate::loader::PreviewLod;

        assert_eq!(PreviewLod::Small.target_size(), 384);
        assert_eq!(PreviewLod::Medium.target_size(), 640);
        assert_eq!(PreviewLod::Large.target_size(), 1024);

        // Standard DPI (1.0x)
        assert_eq!(PreviewLod::from_tile_width(120.0, 1.0), PreviewLod::Small);
        assert_eq!(PreviewLod::from_tile_width(200.0, 1.0), PreviewLod::Small);
        assert_eq!(PreviewLod::from_tile_width(300.0, 1.0), PreviewLod::Medium);
        assert_eq!(PreviewLod::from_tile_width(500.0, 1.0), PreviewLod::Large);

        // High DPI (1.5x)
        assert_eq!(PreviewLod::from_tile_width(120.0, 1.5), PreviewLod::Small);
        assert_eq!(PreviewLod::from_tile_width(200.0, 1.5), PreviewLod::Medium);
        assert_eq!(PreviewLod::from_tile_width(320.0, 1.5), PreviewLod::Large);

        // Retina / 4K (2.0x)
        assert_eq!(PreviewLod::from_tile_width(100.0, 2.0), PreviewLod::Small);
        assert_eq!(PreviewLod::from_tile_width(180.0, 2.0), PreviewLod::Medium);
        assert_eq!(PreviewLod::from_tile_width(250.0, 2.0), PreviewLod::Large);

        // Ordering allows upgrade comparison (e.g., existing.lod < needed_lod)
        assert!(PreviewLod::Small < PreviewLod::Medium);
        assert!(PreviewLod::Medium < PreviewLod::Large);
    }

    #[test]
    fn rotation_clockwise_and_counter_clockwise_arithmetic() {
        let mut rot = 0;
        // CW: +90 % 360
        rot = (rot + 90) % 360;
        assert_eq!(rot, 90);
        rot = (rot + 90) % 360;
        assert_eq!(rot, 180);
        rot = (rot + 90) % 360;
        assert_eq!(rot, 270);
        rot = (rot + 90) % 360;
        assert_eq!(rot, 0);

        // CCW: +270 % 360
        rot = (rot + 270) % 360;
        assert_eq!(rot, 270);
        rot = (rot + 270) % 360;
        assert_eq!(rot, 180);
        rot = (rot + 270) % 360;
        assert_eq!(rot, 90);
        rot = (rot + 270) % 360;
        assert_eq!(rot, 0);

        // Mixed CW then CCW brings back to 0
        rot = (rot + 90) % 360;
        assert_eq!(rot, 90);
        rot = (rot + 270) % 360;
        assert_eq!(rot, 0);
    }

    #[test]
    fn shortcut_save_and_save_as_chords() {
        // Ctrl + S (without Shift): Save / overwrite
        let ctrl_s = egui::Modifiers {
            ctrl: true,
            shift: false,
            alt: false,
            command: true,
            mac_cmd: false,
        };
        let is_save = ctrl_s.ctrl && !ctrl_s.shift;
        let is_save_as = ctrl_s.ctrl && ctrl_s.shift;
        assert!(is_save);
        assert!(!is_save_as);

        // Ctrl + Shift + S: Save As...
        let ctrl_shift_s = egui::Modifiers {
            ctrl: true,
            shift: true,
            alt: false,
            command: true,
            mac_cmd: false,
        };
        let is_save2 = ctrl_shift_s.ctrl && !ctrl_shift_s.shift;
        let is_save_as2 = ctrl_shift_s.ctrl && ctrl_shift_s.shift;
        assert!(!is_save2);
        assert!(is_save_as2);
    }

    #[test]
    fn cover_uv_aspect_calculation() {
        // Square image into square slot: UV is full (0..1)
        let slot = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 100.0));
        let uv_sq = cover_uv([200, 200], slot);
        assert!((uv_sq.min.x - 0.0).abs() < 1e-4);
        assert!((uv_sq.min.y - 0.0).abs() < 1e-4);
        assert!((uv_sq.max.x - 1.0).abs() < 1e-4);
        assert!((uv_sq.max.y - 1.0).abs() < 1e-4);

        // Landscape image (2:1) into square slot: height covers (y is 0..1), width is center-cropped (x > 0 and < 1)
        let uv_land = cover_uv([400, 200], slot);
        assert!((uv_land.min.y - 0.0).abs() < 1e-4);
        assert!((uv_land.max.y - 1.0).abs() < 1e-4);
        assert!(uv_land.min.x > 0.1 && uv_land.min.x < 0.4);
        assert!(uv_land.max.x > 0.6 && uv_land.max.x < 0.9);
    }

    #[test]
    fn multiview_unified_grid_selection_mapping() {
        let folder_count = 3;
        let image_count = 5;
        let total_items = folder_count + image_count;
        assert_eq!(total_items, 8);

        // Folder indices 0..3
        for f in 0..folder_count {
            let selected_folder = Some(f);
            let unified = match selected_folder {
                Some(idx) if idx < folder_count => idx,
                _ => folder_count,
            };
            assert_eq!(unified, f);
            assert!(unified < folder_count);
        }

        // Image indices 0..5 map to unified indices 3..8
        for i in 0..image_count {
            let selected_folder: Option<usize> = None;
            let current_image_idx = i;
            let unified = match selected_folder {
                Some(idx) if idx < folder_count => idx,
                _ => folder_count + current_image_idx,
            };
            assert_eq!(unified, folder_count + i);
            assert!(unified >= folder_count && unified < total_items);
        }
    }

    #[test]
    fn test_breadcrumbs_short_path_no_ellipsis() {
        let crumbs = vec![
            ("C:\\".to_string(), PathBuf::from("C:\\")),
            ("Photos".to_string(), PathBuf::from("C:\\Photos")),
        ];
        let segments = layout_breadcrumbs(&crumbs, 500.0, |t, _| t.chars().count() as f32 * 10.0);
        assert_eq!(segments.len(), 2);
        match &segments[0] {
            BreadcrumbSegment::Crumb { label, is_current, .. } => {
                assert_eq!(label, "C:\\");
                assert!(!is_current);
            }
            _ => panic!("Expected Crumb"),
        }
        match &segments[1] {
            BreadcrumbSegment::Crumb { label, is_current, .. } => {
                assert_eq!(label, "Photos");
                assert!(is_current);
            }
            _ => panic!("Expected Crumb"),
        }
    }

    #[test]
    fn test_breadcrumbs_intermediate_collapse_keeps_root_and_trailing() {
        let crumbs = vec![
            ("F:\\".to_string(), PathBuf::from("F:\\")),
            ("Collections".to_string(), PathBuf::from("F:\\Collections")),
            ("091. Files 3".to_string(), PathBuf::from("F:\\Collections\\091. Files 3")),
            ("kids on cam".to_string(), PathBuf::from("F:\\Collections\\091. Files 3\\kids on cam")),
            ("CaptureFiles".to_string(), PathBuf::from("F:\\Collections\\091. Files 3\\kids on cam\\CaptureFiles")),
            ("2018-04-05".to_string(), PathBuf::from("F:\\Collections\\091. Files 3\\kids on cam\\CaptureFiles\\2018-04-05")),
        ];

        let segments = layout_breadcrumbs(&crumbs, 260.0, |t, _| t.chars().count() as f32 * 8.0);
        assert!(segments.len() >= 3);
        assert!(matches!(&segments[0], BreadcrumbSegment::Crumb { label, .. } if label == "F:\\"));
        assert!(matches!(&segments[1], BreadcrumbSegment::Ellipsis { hidden } if !hidden.is_empty()));
        assert!(matches!(segments.last(), Some(BreadcrumbSegment::Crumb { is_current: true, .. })));
    }

    #[test]
    fn test_breadcrumbs_tight_space_collapses_to_ellipsis_and_current() {
        let crumbs = vec![
            ("F:\\".to_string(), PathBuf::from("F:\\")),
            ("VeryLongIntermediateFolderNameA".to_string(), PathBuf::from("F:\\A")),
            ("VeryLongIntermediateFolderNameB".to_string(), PathBuf::from("F:\\A\\B")),
            ("CurrentActiveFolder".to_string(), PathBuf::from("F:\\A\\B\\CurrentActiveFolder")),
        ];

        let segments = layout_breadcrumbs(&crumbs, 190.0, |t, _| t.chars().count() as f32 * 7.0);
        assert_eq!(segments.len(), 2);
        assert!(matches!(&segments[0], BreadcrumbSegment::Ellipsis { .. }));
        assert!(matches!(&segments[1], BreadcrumbSegment::Crumb { label, is_current: true, .. } if label == "CurrentActiveFolder"));
    }

    #[test]
    fn test_breadcrumbs_narrow_truncates_current() {
        let crumbs = vec![
            ("F:\\".to_string(), PathBuf::from("F:\\")),
            ("ExtremelySuperLongActiveFolderNameExceedingAllBounds".to_string(), PathBuf::from("F:\\Long")),
        ];
        let segments = layout_breadcrumbs(&crumbs, 60.0, |t, _| t.chars().count() as f32 * 8.0);
        assert_eq!(segments.len(), 2);
        assert!(matches!(&segments[0], BreadcrumbSegment::Ellipsis { .. }));
        if let BreadcrumbSegment::Crumb { label, is_current, .. } = &segments[1] {
            assert!(is_current);
            assert!(label.len() < "ExtremelySuperLongActiveFolderNameExceedingAllBounds".len());
        } else {
            panic!("Expected truncated crumb");
        }
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

        // Enter toggles between Viewer and the fullscreen grid selector, or navigates into a folder.
        if input.enter && !self.modal_open() && !self.grid_path_editing {
            if self.view_mode == ViewMode::Viewer {
                let folder_count = if self.config.multiview_show_folders {
                    self.multiview_subfolders.len()
                } else {
                    0
                };
                if !self.files.is_empty() || folder_count > 0 || self.current_folder().is_some() {
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
            // filmstrip) while the folder browser modal is open, so it stays
            // visually clean. Their config flags are left untouched.
            if !self.show_folder_browser {
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





