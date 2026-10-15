# Photonio

<p align="center">
  <img src="images/logo.png" alt="Photonio logo" width="160">
</p>

<p align="center">
  <a href="#license"><img src="https://img.shields.io/badge/license-Apache%202.0-2ea44f" alt="License"></a>
  <img src="https://img.shields.io/badge/version-v0.9.9-3b82f6" alt="Version v0.9.9">
</p>

**A high-performance, lightweight, glassmorphic image viewer for Windows 10/11 — built in pure Rust with `egui`/`eframe`.**

Photonio puts your images on a clean, unobstructed canvas — no ribbon bars, no clutter. Decoding runs on dedicated background threads with priority scheduling, so browsing stays fluid even over large photo collections. The interface is a frosted-glass overlay with Font Awesome iconography and a dynamic Light/Dark theme.

## Features

- **Fast, non-blocking loading** — full-res and thumbnail decoders run on separate worker threads, with circular ring preloading and priority scheduling so the active image is always decoded first.
- **Broad format support** — JPEG, PNG, BMP, GIF, TIFF, WebP, and ICO, with EXIF rotation applied automatically.
- **Animated playback** — multi-frame **GIF** and **Animated WebP** play, loop, pause (`Space`), and step frame-by-frame (`[` / `]`).
- **Dynamic filmstrip** — 4-way docking (Bottom / Top / Left / Right), aspect-ratio-preserving thumbnails, and a hotkey toggle (`T`).
- **Quick-Action Pill** — a customizable top-right icon toolbar (Open File, Open Folder, Save As, Print, Share, Toggle Filmstrip) with horizontal or vertical layouts.
- **Windows shell integration** — portable `HKCU` default-app registration (no admin), an Explorer folder context menu ("Open folder with Photonio"), CLI / double-click argument support, and registry health checking with portable-path mismatch detection.
- **Glassmorphism theme** — translucent frosted panels with configurable opacity, dynamic Light/Dark switching, and Font Awesome 7 Solid iconography.
- **Persistence** — remembers the exact last-viewed image, its folder, window geometry, and all settings in `%APPDATA%\Photonio\config.json`.

## Keyboard shortcuts

| Action | Keys |
| --- | --- |
| Previous / Next image | `←` `→` · `A` `D` · Mouse wheel |
| Zoom in / out | `Ctrl` + Mouse wheel |
| Fit to window | `0` |
| Actual size (100%) | `1` |
| Pan (when zoomed) | Left-drag |
| Rotate 90° clockwise | `R` |
| Toggle filmstrip | `T` |
| Play / Pause animation | `Space` |
| Step animation frame | `[` `]` |
| Copy file path | `Ctrl` + `C` |
| Open file | `Ctrl` + `O` |
| Open folder | `Ctrl` + `Shift` + `O` |
| Save image as… | `Ctrl` + `S` |
| Print… | `Ctrl` + `P` |
| Close image | `Ctrl` + `W` |
| Close folder | `Ctrl` + `Shift` + `W` |
| Show in Explorer | `Ctrl` + `E` |
| Settings | `Ctrl` + `,` |
| Move to Recycle Bin | `Del` |
| Toggle fullscreen | `F11` · `Alt` + `Enter` |
| Help / shortcuts | `F1` · `H` |
| Context menu | Right-click |

## Building from source

**Prerequisites**

- Windows 10 or 11 (x64)
- Rust toolchain (stable, 2021 edition) — https://rustup.rs
- MSVC build tools — install the "Desktop development with C++" workload from the Visual Studio Build Tools.

```powershell
git clone https://github.com/limbo666/Photonio.git
cd Photonio
cargo build --release
.\target\release\photonio.exe
```

> **Always use `--release`.** Image decoding is 10–40× slower in debug builds.

The result is a **self-contained single executable** — no external C/C++ runtime, video codecs, or DLLs are required.

## Usage

- Open a file (`Ctrl+O`) or a folder (`Ctrl+Shift+O`), or drag-and-drop an image or folder onto the window.
- Pass a file or folder path on the command line, or use the Explorer **"Open folder with Photonio"** context menu.
- Photonio scans the containing folder with natural sort, so prev/next navigation and the filmstrip work around the opened image.

## Settings

Configured in **Settings** (`Ctrl+,`) and persisted to `%APPDATA%\Photonio\config.json`: image/folder persistence, navigation looping, wheel action, HUD position, filmstrip visibility + dock position, quick-action pill visibility + layout, canvas background (Glass / Dark / Light), glass opacity, and Windows integration (default-app registration + Explorer folder context menu).

## Project structure

| Path | Purpose |
| --- | --- |
| `src/main.rs` | Entry point, CLI arg ingestion, window lifecycle |
| `src/app.rs` | UI state, input dispatch, HUD, pill, filmstrip, settings |
| `src/viewer/mod.rs` | Image/EXIF + GIF/WebP animation decoding, texture cache, zoom/pan |
| `src/filesystem/mod.rs` | Directory scan + natural sort |
| `src/platform/mod.rs` | Windows shell (wallpaper, Explorer, print, clipboard) |
| `src/platform/windows.rs` | HKCU registry: default-app + folder context-menu integration |
| `src/config.rs` | JSON config persistence |
| `src/assets.rs` | Embedded logo |
| `src/icons.rs` | Font Awesome icon codepoints |
| `src/theme.rs` | Theme-derived glass panel colors |
| `src/loader.rs` | Background decode workers (full-res + thumbnail) |
| `src/about.rs` | About modal |

## License

Released under the **Apache License 2.0**. See [LICENSE](LICENSE).

Built by [Nikos Georgousis](https://www.georgousis.info).
