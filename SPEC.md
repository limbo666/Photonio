# Project Specification: Photonio (Rust)
**Target:** Lightweight, distraction-free desktop image viewer inspired by ImageGlass.
**Target OS:** Windows 10 / 11 (x64)

## 1. Guiding Principles
- **Minimalist:** No heavy ribbon bars or multi-tiered settings menus. Pure canvas with floating HUD and context menu.
- **Pure Rust Stack:** Zero external C++ or system library dependencies beyond the standard MSVC toolchain.
- **Responsive:** Folder crawling and decoding should never block window drawing or UI interactivity.
- **Fail Gracefully:** Invalid/corrupted images display a clean error glyph and allow immediate navigation to the next file.

## 2. Technology Stack
- **Language:** Rust (2021 edition)
- **GUI Engine:** `eframe` / `egui` (0.28+)
- **Image Pipeline:** `image` crate (PNG, JPEG, WebP, GIF, BMP, ICO)
- **Platform APIs:**
  - `rfd` (Native Windows file picker dialogs)
  - `open` (Shell "Open With" integration)
  - `trash` (Recycle bin file deletion)
  - `windows` / `winapi` (System wallpaper setting via `SystemParametersInfoW`)

## 3. Scope & Feature Baseline
1. **Viewport:**
   - Auto-fit to window maintaining aspect ratio.
   - Smooth zoom (Ctrl + Scroll) and pan (Left-drag when zoomed).
2. **Directory Traversal:**
   - Natural alphabetical sort (e.g., `img1`, `img2`, `img10`).
   - Hotkeys: Left/Right Arrow, Mouse Wheel (Next/Prev), Home/End.
3. **Context Menu (Right Click):**
   - File details (Resolution, Size, Path).
   - "Open in File Explorer" (`explorer.exe /select,<path>`).
   - "Open With..." (System default app chooser).
   - "Set as Desktop Wallpaper".
   - "Copy Image to Clipboard" / "Copy File Path".
   - "Delete" (Send to Recycle Bin).

## 4. Architecture Contract
- `src/main.rs`: Entry point, CLI argument parsing, window lifecycle.
- `src/app.rs`: Main egui UI state, keyboard/mouse event dispatch, context menu drawing.
- `src/viewer/`: Texture cache, zoom/pan coordinate transformation.
- `src/filesystem/`: Directory scanner, natural sort comparator, file system watcher.
- `src/platform/`: Windows-specific shell integrations (wallpaper, explorer).