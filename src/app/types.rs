// File: src/app/types.rs
// Purpose: Core data types, enums, UI snapshots, and visual styling helpers for Photonio.

use std::path::PathBuf;
use eframe::egui;
use crate::loader;

/// Color used for inline failure notices.
pub(crate) const ERROR_COLOR: egui::Color32 = egui::Color32::from_rgb(235, 87, 87);
/// Color used for success feedback in the HUD.
pub(crate) const SUCCESS_COLOR: egui::Color32 = egui::Color32::from_rgb(120, 220, 150);
/// Amber warning color for registry path-mismatch notices.
pub(crate) const WARN_COLOR: egui::Color32 = egui::Color32::from_rgb(255, 170, 40);
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
pub(crate) fn is_supporter_check() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .map(|dir| dir.join("euro.png").is_file() || dir.join("euro.jpg").is_file())
        .unwrap_or(false)
}

/// Register Font Awesome Solid + Brands as fallback fonts so icon glyphs
/// (including brand marks like PayPal) render natively across the UI.
pub(crate) fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "font_awesome".to_owned(),
        egui::FontData::from_static(include_bytes!("../../assets/fonts/fa-solid-900.otf")),
    );
    // Brand icons (e.g. PayPal `\u{f1ed}`) live in a separate FA font.
    fonts.font_data.insert(
        "font_awesome_brands".to_owned(),
        egui::FontData::from_static(include_bytes!("../../assets/fonts/fa-brands-400.otf")),
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
pub(crate) fn apply_global_visuals(ctx: &egui::Context, dark_mode: bool) {
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
pub(crate) struct InputSnapshot {
    pub(crate) ctrl: bool,
    pub(crate) scroll: f32,
    pub(crate) pointer_pos: Option<egui::Pos2>,
    pub(crate) pointer_delta: egui::Vec2,
    pub(crate) primary_down: bool,
    pub(crate) primary_pressed: bool,
    pub(crate) has_hovered_files: bool,
    pub(crate) double_clicked: bool,
    pub(crate) secondary_clicked: bool,
    pub(crate) prev: bool,
    pub(crate) next: bool,
    pub(crate) up: bool,
    pub(crate) down: bool,
    pub(crate) home: bool,
    pub(crate) end: bool,
    pub(crate) enter: bool,
    pub(crate) reset_fit: bool,
    pub(crate) reset_100: bool,
    pub(crate) rotate: bool,
    pub(crate) rotate_ccw: bool,
    pub(crate) open_file: bool,
    pub(crate) open_folder: bool,
    pub(crate) copy_path: bool,
    pub(crate) copy_image: bool,
    pub(crate) reveal: bool,
    pub(crate) delete: bool,
    pub(crate) escape: bool,
    pub(crate) toggle_fullscreen: bool,
    pub(crate) help: bool,
    pub(crate) close_file: bool,
    pub(crate) close_folder: bool,
    pub(crate) open_settings: bool,
    pub(crate) toggle_filmstrip: bool,
    pub(crate) toggle_play: bool,
    pub(crate) step_back: bool,
    pub(crate) step_forward: bool,
    pub(crate) save: bool,
    pub(crate) save_as: bool,
    pub(crate) print: bool,
    pub(crate) toggle_image_info: bool,
    pub(crate) resize_image: bool,
    pub(crate) crop: bool,
    pub(crate) refresh_folder: bool,
    pub(crate) rename: bool,
    pub(crate) shift: bool,
}

/// An action selected from the quick-action pill.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PillAction {
    OpenFile,
    OpenFolder,
    Crop,
    SaveAs,
    SaveRotation,
    Print,
    ToggleFilmstrip,
}

/// Which pane of the two-pane Settings modal is active.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsTab {
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
pub(crate) enum GridTileAction {
    Select(usize),
    Open(usize),
    ContextMenu(usize, egui::Pos2),
    Drag(usize),
}

/// A subfolder displayed in the MultiView grid.
#[derive(Clone, Debug)]
pub struct MultiViewSubfolder {
    pub path: PathBuf,
    pub name: String,
    pub preview_paths: Vec<PathBuf>,
}

pub(crate) enum SubfolderWorkerMsg {
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

pub(crate) enum GridFolderAction {
    Select(usize),
    Open(usize),
    ContextMenu(usize, egui::Pos2),
    Drag(usize),
}

/// A folder-browser operation awaiting the user's "Select Folder" confirmation.
#[derive(Clone, Copy)]
pub(crate) enum PendingOperation {
    OpenFolder,
    CopyFileTo,
    MoveFileTo,
}

/// State for the file rename modal dialog.
pub(crate) struct RenameState {
    pub(crate) old_path: PathBuf,
    pub(crate) new_name: String,
    pub(crate) focus_pending: bool,
}

/// An action selected from the context menu.
pub(crate) enum MenuAction {
    OpenFile,
    OpenFolder,
    CloseImage,
    CloseFolder,
    CropImage,
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
    SetSort(crate::config::FileSortCriteria, crate::config::SortDirection),
    Settings,
    About,
    Help,
    Exit,
}

/// A cached thumbnail: its GPU texture plus the (low-res) pixel dimensions,
/// kept so the filmstrip can draw it and the main view can use it as an
/// instant placeholder while the full-res decode is still in flight.
pub(crate) struct Thumb {
    pub(crate) texture: egui::TextureHandle,
    pub(crate) dims: [usize; 2],
    pub(crate) lod: loader::PreviewLod,
}

/// Playback state for a multi-frame animation (GIF or WebP). Each frame is a
/// display-res GPU texture; `dimensions` is the shared canvas size for zoom math.
pub(crate) struct AnimationPlayer {
    pub(crate) frames: Vec<egui::TextureHandle>,
    pub(crate) dimensions: [usize; 2],
    pub(crate) delays_ms: Vec<u64>,
    pub(crate) current_frame: usize,
    pub(crate) paused: bool,
    pub(crate) next_frame: std::time::Instant,
    pub(crate) kind: loader::AnimKind,
}

impl AnimationPlayer {
    pub(crate) fn new(
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
    pub(crate) fn current_texture(&self) -> (egui::TextureId, [usize; 2]) {
        let id = self
            .frames
            .get(self.current_frame)
            .map(|t| t.id())
            .unwrap_or(egui::TextureId::Managed(0));
        (id, self.dimensions)
    }
}
