# Photonio — Release 5

**Version:** v0.9.18 · **Platform:** Windows 10 / 11 (x64)

A high-performance, lightweight image viewer built in pure Rust with `egui`/`eframe` — featuring an interactive on-canvas crop tool, native Windows OLE drag-out, draggable overlays with closest-edge anchoring, a custom in-app folder browser, multi-view thumbnail grid, glassmorphic panels, and a Light theme by default.

## What's new in v0.9.18

- **Ultimate Interactive Crop Tool (`C`):** Full-featured on-canvas cropping with 8 visual drag handles, aspect ratio presets (Freeform, Original, 1:1, 16:9, 4:3, 3:2), portrait/landscape orientation toggle, composition guides (Rule of Thirds and Center Grid), pixel-exact numeric inputs, keyboard arrow-key nudging, in-memory apply (`Enter`), lossless overwrite (`Ctrl+S`), save copy (`Ctrl+Shift+S`), and direct clipboard export.
- **Smart Detached Crop Workspace:** Inset canvas margins (32px top, 84px bottom, 32px lateral) and bottom-docked tool controls ensure the crop box boundaries, corner brackets, and handles never collide with toolbars or window edges. Competing background overlays (navigation HUD, filmstrip, quick pill, donation CTA) automatically hide during active crop sessions for a clean, distraction-free editing surface.
- **Native Windows Drag-and-Drop Out:** Drag images or folders directly out of Photonio into external Windows applications (Desktop, Windows File Explorer, Photoshop, MS Paint, Discord, Chrome, etc.) via pure Win32 OLE `DoDragDrop` (`CF_HDROP` & `CF_UNICODETEXT`). Zoomed-in canvas panning remains 100% active, while 1:1/fit view, MultiView grid cards, and filmstrip tiles support instantaneous drag-out past a 6px deadzone.
- **Draggable Overlays with Closest-Edge Relative Anchoring:** Floating controls (Quick-Action Tool Pill and Crop Toolbar) now feature tactile drag grip handles (`⋮⋮` vertical and `···` horizontal). Controls dynamically anchor to their closest screen edge or corner (Top-Left, Top-Right, Bottom-Left, Bottom-Right) and automatically maintain their relative distance when the window is resized. Double-clicking any grip resets it to its default dock position.
- **Expanded Next-Gen Format Support:** Added full native decoding for **AVIF**, **TGA**, **SVG / SVGZ**, and **JPEG XL (`.jxl`)** alongside existing JPEG/JFIF, PNG, GIF, WebP, BMP, ICO, and TIFF files.
- **Selectable Folder Sorting Order:** Sort and navigate collections by **Name**, **Date Modified**, or **File Size** in **Ascending** or **Descending** order via the MultiView grid header, right-click context menu, or Settings.
- **Multi-Monitor Position Memory & Out-of-Bounds Auto-Reset:** Photonio now remembers its exact window position across multi-monitor setups (secondary displays to the left with negative coordinates, to the right, above, or below). If a secondary monitor is disconnected, powered off, or the saved coordinates are out of bounds, Photonio automatically resets to default parameters and centers cleanly on the primary display. Also added a "Reset Window Position & Size" button in Settings (`Ctrl+,` → General → Startup & Window) to restore default 960×640 dimensions centered on the primary display at any time.
- **Fixed:** Resolved an issue where "Remember last folder/file on startup" remembered previous paths even when deselected; deselecting now reliably starts Photonio on a clean welcome screen.
- **Fixed:** Choosing "Close window" from the Windows taskbar context menu while minimized now terminates Photonio instantly and cleanly. Previously, Windows suppressed paint events for minimized windows causing the event loop to stall until restored; a Win32 subclass now intercepts taskbar close commands on iconic windows and exits immediately without hanging or flashing on screen.
- **Refined Welcome Screen:** Polished layout and centered typography with crisp logo presentation and clear shortcut hints.

## Run

Double-click **`photonio.exe`**. No installation or extra runtime files are required — it is a self-contained single executable (only standard Windows system DLLs are used).

Settings are stored automatically in `%APPDATA%\Photonio\config.json`.

## Features

- Fast, non-blocking decoding with background priority scheduling and circular ring preloading.
- Formats: JPEG (`.jpg` `.jpeg` `.jfif`), PNG, WebP, AVIF, GIF, BMP, ICO, TIFF, TGA, SVG / SVGZ, and JPEG XL (`.jxl`), with automatic EXIF rotation.
- Animated GIF and Animated WebP playback (`Space` play/pause, `[` / `]` step frames).
- **Interactive Crop Tool (`C`)** — visual crop overlay with 8 handles, aspect ratio presets, orientation swap, composition guides, numeric edits, lossless overwrite, and clipboard export.
- **Native Windows Drag-and-Drop Out** — drag images or folders directly into Photoshop, Paint, Explorer, or Desktop from canvas, MultiView cards, or filmstrip.
- **Draggable Overlays & Closest-Edge Anchoring** — tactile grip handles with natural orientation rotation, closest-edge resize tracking, and double-click reset.
- **Multi-View thumbnail grid** (`Enter` / double-click) with subfolder cards, 2×2 media quad previews, folder sorting, and a single-line breadcrumb bar with an overflow `…` dropdown and inline path editing (`Ctrl+L`).
- **Custom in-app Folder Browser** — instant directory scanning, tile (Small / Medium / Large) and List views, folder history (Back / Forward), a unified OS-style address bar, a Quick Access sidebar (Desktop, Downloads, Pictures, Documents + drives), pinned folders, and inline directory creation.
- **Lossless in-place rotation** (`R` clockwise, `Ctrl+R` counter-clockwise) with a pulsing "breathing" Save indicator — commit with `Ctrl+S`, save a copy with `Ctrl+Shift+S`.
- **Safe in-place file renaming** (`F2` / context menu) with live Windows name validation, DOS reserved-name rejection, and NTFS case-only rename safety.
- **Image resizer** (`Ctrl+Shift+R`) with aspect-ratio lock, percentage or pixel scaling, and 5 interpolation filters.
- **EXIF & metadata inspector** (`I`) — resolution, color depth, camera make/model, lens, exposure, ISO, and GPS.
- Drag-and-drop an image **or an entire folder** onto the window to open it directly.
- 4-way dockable filmstrip with aspect-ratio-preserving thumbnails (`T` toggles).
- Customizable Quick-Action Pill (Open, Folder, Save As, Print, Share, Filmstrip) in horizontal or vertical orientation.
- Windows integration: in-process Win32 registry default-app registration (no console popups) and an "Open folder with Photonio" Explorer context menu.
- Frosted-glass theme with Light / Dark / Glass switching (Light by default), configurable opacity, and Font Awesome icons.
- Support Photonio on PayPal — a periodic, dismissible donation pill plus an About-dialog link; a local `euro.png` / `euro.jpg` next to the exe suppresses it.
- Remembers your last image, folder, window geometry, and settings.

## Keyboard shortcuts

| Action | Keys |
| --- | --- |
| Previous / Next | `←` `→` · `A` `D` · Mouse wheel |
| First / Last | `Home` / `End` |
| Toggle Multi-View Grid | `Enter` · Double-click |
| Zoom | `Ctrl` + Mouse wheel |
| Fit / 100% | `0` / `1` |
| Rotate 90° CW / CCW | `R` / `Ctrl` + `R` |
| Save rotation / Save As | `Ctrl` + `S` / `Ctrl` + `Shift` + `S` |
| Crop image | `C` |
| Rename file | `F2` |
| Resize image | `Ctrl` + `Shift` + `R` |
| Image properties / EXIF | `I` |
| Toggle filmstrip | `T` |
| Play / Pause animation | `Space` |
| Step animation frame | `[` `]` |
| Copy path / copy image to clipboard | `Ctrl` + `C` / `Ctrl` + `Shift` + `C` |
| Open file / folder | `Ctrl` + `O` / `Ctrl` + `Shift` + `O` |
| Focus address bar | `Ctrl` + `L` |
| Go to parent folder | `Backspace` · `Alt` + `↑` |
| Folder history Back / Forward | `Alt` + `←` / `Alt` + `→` |
| Show in File Explorer | `Ctrl` + `E` |
| Print | `Ctrl` + `P` |
| Close image / folder | `Ctrl` + `W` / `Ctrl` + `Shift` + `W` |
| Refresh folder | `F5` |
| Settings | `Ctrl` + `,` |
| Delete | `Del` |
| Fullscreen | `F11` / `Alt` + `Enter` |
| Help | `F1` / `H` |

## Links

- **Source code:** https://github.com/limbo666/Photonio
- **Author:** https://www.georgousis.info

## License

Apache License 2.0 — see the included `LICENSE` file.
