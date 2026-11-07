# Project Specification: Photonio (Rust)
**Target:** Lightweight, high-performance, distraction-free desktop image viewer with glassmorphic aesthetics.
**Target OS:** Windows 10 / 11 (x64)
**Current Version:** v0.9.17

## 1. Guiding Principles
- **Minimalist & Distraction-Free:** No heavy ribbon bars or multi-tiered legacy menus. Pure canvas with floating frosted glass HUD, toolbar pill, and contextual overlays.
- **Pure Rust Stack:** Zero external C++ or runtime DLL dependencies beyond the standard MSVC Windows toolchain.
- **Non-Blocking & Fluid:** Folder scanning, thumbnail generation, full-res image decoding, and registry operations execute on background worker threads without UI stutter.
- **Fail Gracefully:** Invalid, unreadable, or corrupted files display subtle placeholders and allow immediate navigation to neighboring images.
- **Design System Consistency:** Unified frosted glassmorphism across Light, Dark, and Glass canvas themes, with theme-tailored high-contrast gold accents, standard button heights, and pointing-hand cursor feedback.

## 2. Technology Stack
- **Language:** Rust (2021 edition)
- **GUI Engine:** `eframe` / `egui` (0.28+) with Font Awesome 7 Solid iconography
- **Image Pipeline:** `image` crate (PNG, JPEG/JFIF, TIFF, WebP, GIF, BMP, ICO, TGA, AVIF), `resvg` / `usvg` (SVG, SVGZ), `jxl-oxide` (JPEG XL / JXL), `kamadak-exif` (EXIF camera metadata inspection)
- **Platform APIs:**
  - `windows-sys` (Direct Win32 registry management, shell execution, wallpaper setting)
  - `rfd` (Native Windows fallback dialogs)
  - `trash` (Recycle Bin file deletion)

## 3. Scope & Feature Baseline

### 1. Viewport & Canvas
- Aspect-ratio-preserving fit to window (`0`), 1:1 pixel mode (`1`), and smooth mouse wheel zoom (`Ctrl + Wheel`).
- Free-form panning via left-drag when zoomed.
- Lossless 90° clockwise rotation (`R`) and counter-clockwise rotation (`Ctrl + R`) with breathing save indicator and save chords (`Ctrl + S` / `Ctrl + Shift + S`).
- Translucent frosted glass, solid dark, or clean light canvas backgrounds with configurable opacity.

### 2. Asynchronous Decoding & Preloading
- Decoupled worker threads: priority full-res decoder and batch thumbnail decoder (`src/loader.rs`).
- Circular ring preloading: decodes neighboring images ahead of navigation for near-instant transitions.
- Texture and thumbnail GPU caching with automatic LRU and index remapping on folder modifications.

### 3. Multi-View Thumbnail Grid & File System Navigation
- Toggle between single-image viewer and virtualized thumbnail grid (`Enter` or double-click canvas).
- Configurable "View folders on multi-view" (enabled by default) displaying directory subfolders alongside image files.
- Folders with media render a solid amber-gold base folder symbol filled to the tile with a 2x2 quad image overlay inset on top; folders without media display the solid golden folder symbol alone.
- Parent folder navigation via `Backspace` and `Alt + ↑` (and top-right Up button).
- Responsive single-line breadcrumb path bar: intermediate folders automatically collapse into an interactive `"..."` button when path exceeds the width, maintaining pointing-hand cursor feedback and opening a dropdown of hidden folders on click without destroying button behaviors.
- Interactive keyboard navigation (arrow keys, `Home`, `End`) across folders and images; pressing `Enter` on a folder navigates inside, while `Enter` on an image opens the single-image viewer.
- Direct path entry mode with `Ctrl + L`.

### 4. In-App Custom Folder Browser with Covers
- Modal directory picker with Quick Access pinned folders and folder history traversal.
- Live subfolder cover previews generated dynamically from the first image in each folder.
- View and show filters, inline new directory creation (`+ New Folder`), and pin/unpin with theme-adaptive gold accents.

### 5. Folder Navigation History Stack
- Traversal history stack with Back (`Alt + ←`) and Forward (`Alt + →`) navigation across directories in both viewer and grid modes.

### 6. Selectable Folder Sorting & Navigation Order
- Three selectable sort criteria: `Name` (natural case-insensitive alphanumeric), `Date Modified` (file system modification time), and `File Size` (file length in bytes).
- Dual direction support: `Ascending` (A-Z / Oldest / Smallest) and `Descending` (Z-A / Newest / Largest).
- Default behavior: `Name` + `Ascending` (100% backward compatible with prior versions).
- High-performance Schwartzian transform caching metadata attributes during scan, guaranteeing $O(N)$ system calls instead of $O(N \log N)$.
- Seamless in-place reconciliation: switches sort order without reloading the view, remapping decoded GPU texture and thumbnail caches and maintaining active image focus.
- Accessible via the MultiView grid header dropdown, right-click canvas context menu, and Settings (`Ctrl + ,`).
- Persisted automatically to `%APPDATA%\Photonio\config.json`.

### 7. Safe In-Place File Renaming
- Inline modal (`F2` or context menu) with live Windows file-naming validation.
- Guards against illegal characters (`< > : " / \ | ? *`), DOS reserved names (CON, PRN, AUX, NUL, COM1-9, LPT1-9), and file collisions.
- NTFS case-only rename safety through disambiguation temporary paths.
- Zero-flicker GPU texture remapping locking the active viewer onto the renamed file.

### 7. Image Resizer
- Resizing utility (`Ctrl + Shift + R`) with aspect ratio locking, percentage scaling, and pixel dimension inputs.
- 5 high-quality interpolation filters: Lanczos3, Gaussian, Catmull-Rom, Triangle, and Nearest.
- Real-time target dimension feedback, file size estimation, and overwrite confirmation guards.

### 8. Ultimate Interactive Crop Tool
- Visual on-canvas crop overlay (`C` or right-click menu or quick toolbar icon) with darkened outer mask.
- 8 tactile drag handles (4 corner brackets + 4 edge grip bars) with dynamic cursor changes and interior box panning.
- Aspect ratio constraint presets: `Free`, `Original`, `1:1 (Square)`, `16:9`, `4:3`, `3:2`.
- Instant Portrait/Landscape orientation swap (`⇋`) for all ratio presets.
- Toggleable composition guide overlays: Rule of Thirds (3×3) and Center Crosshair Grid.
- Real-time pixel-exact numeric inputs (`Width`, `Height`, `X`, `Y`) with live percentage-of-original feedback.
- Keyboard nudging: Arrow keys (`←`, `→`, `↑`, `↓`) nudge crop position by 1px (or 10px with `Shift`).
- Multi-action workflow: In-memory live apply (`Enter`), lossless overwrite with confirmation (`Ctrl + S`), save copy (`Ctrl + Shift + S`), and direct clipboard image export (`Ctrl + C`).
- Draggable floating toolbar with tactile grip handle (`⋮⋮`), boundary screen clamping, and double-click reset to default top-center.

### 9. Animated Playback Engine
- Multi-frame GIF and Animated WebP decoding with preserved per-frame delays.
- Play/pause toggle (`Space`) and frame-by-frame stepping (`[` and `]`).

### 9. Metadata & EXIF Camera Inspector
- Inspector window (`I`) detailing resolution, aspect ratio, megapixels, color depth, timestamps, and file size.
- Embedded EXIF camera data: camera make/model, lens, exposure time, F-number, ISO, focal length, flash, and GPS coordinates.
- Direct path copy buttons and Explorer folder launch.

### 10. Floating HUD & Quick-Action Pill
- Auto-hiding or pinned HUD bar with image index, file name, resolution, zoom percentage, and format badge.
- Floating quick-action icon pill with customizable tool buttons (Open File, Open Folder, Save As, Print, Share, Filmstrip), horizontal/vertical orientations, tactile drag grip handle (`⋮⋮`), screen boundary clamping, double-click reset to top-right, and session persistence.
- 4-way dockable thumbnail filmstrip (Bottom / Top / Left / Right) with toggle hotkey (`T`).

### 11. Windows Shell & Registry Integration
- Portable `HKCU` default-app registration without requiring administrator privileges.
- Explorer folder context menu ("Open folder with Photonio") using direct Win32 registry APIs.
- Non-blocking in-process registry health monitoring with portable path mismatch detection and one-click repair/clean.

### 12. Session Persistence
- Window geometry, last-viewed image, containing folder, pinned quick-access folders, and preferences persisted to `%APPDATA%\Photonio\config.json`.

### 13. Native Windows OLE Drag-and-Drop Out
- Initiate native drag-and-drop (`DoDragDrop` with `CF_HDROP` and `CF_UNICODETEXT`) directly out of Photonio into external Windows applications (Desktop, Windows File Explorer, Photoshop, MS Paint, Chrome, Discord, Slack, etc.).
- **Viewer Mode (Single Image):**
  - Left-drag when zoomed in continues to pan the viewport smoothly (existing panning 100% preserved).
  - Left-drag when NOT zoomed (fit-to-window or unscaled within bounds) with a $\ge 6\text{ px}$ deadzone initiates native OLE drag-out.
  - Clicks under threshold preserve normal interaction (double-click zoom toggle, HUD toggle, context menu).
- **MultiView Grid & Filmstrip Modes:**
  - Dragging any image thumbnail card or folder tile with $\ge 6\text{ px}$ threshold initiates native OLE drag-out.
  - Normal clicks, double-clicks, and background scrolling remain 100% intact.
- **Pure Win32 Implementation:**
  - Implements lightweight in-process COM `IDataObject`, `IDropSource`, and `IEnumFORMATETC` vtables via `windows-sys` with zero external bloat.

## 4. Architecture Contract

| Module | Responsibility |
| --- | --- |
| `src/main.rs` | Application entry point, CLI argument ingestion, window initialization and lifecycle |
| `src/app/mod.rs` | Main egui UI state machine, keyboard/mouse dispatch, lifecycle orchestrator |
| `src/app/` | UI submodules: `types`, `widgets`, `breadcrumbs`, `cache`, `dialogs/`, `hud`, `filmstrip`, `grid`, `context_menu` |
| `src/viewer/mod.rs` | Texture cache, animation playback loop, zoom/pan coordinate transformation |
| `src/viewer/folder_browser.rs` | In-app folder selector dialog, cover thumbnail generation, quick access, breadcrumbs |
| `src/viewer/metadata.rs` | File information, image properties, and EXIF camera metadata extraction |
| `src/viewer/resize.rs` | Image resampling, aspect ratio math, and interpolation algorithms |
| `src/filesystem/mod.rs` | Directory scanner, natural sort comparator (natord), directory watcher, safe rename |
| `src/platform/mod.rs` | Windows shell integration (wallpaper setting, Explorer launch, print, clipboard) |
| `src/platform/windows.rs` | Win32 registry integration (HKCU file associations & folder context menu) and native OLE Drag-and-Drop (CF_HDROP & CF_UNICODETEXT) |
| `src/theme.rs` | Visual design system: theme tokens, frosted glassmorphism, button styles, color palettes |
| `src/loader.rs` | Background decode workers: full-res priority ring preloading & batch thumbnail generator |
| `src/config.rs` | JSON configuration serialization and disk persistence |
| `src/about.rs` | Standalone About modal dialog with project info, donation link, and license attribution |
| `src/icons.rs` | Font Awesome 7 Solid icon glyph codepoints |
| `src/assets.rs` | Embedded application icon and logo assets |