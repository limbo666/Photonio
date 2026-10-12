# Photonio

**A fast, minimalist, distraction-free image viewer for Windows 10/11 — inspired by ImageGlass, built in pure Rust.**

Photonio puts your images on a clean, unobstructed canvas. There is no ribbon bar or clutter — just the picture, a floating glass HUD, and a right-click context menu. Image decoding runs on background worker threads, so browsing stays fluid even over large photo folders.

<p align="center">
  <img src="images/photonio-logo.png" alt="Photonio logo" width="160">
</p>

> **Status:** v0.9.5 — feature-complete for daily use. See [SPEC.md](SPEC.md) for the original design spec.

## Highlights

- **Non-blocking decoding** — full-res and thumbnail decoders run on dedicated worker threads, with a low-res placeholder shown instantly while the full image loads.
- **Natural sort** — `img2` sorts before `img10`, so folders play in the order you expect.
- **EXIF-aware** — photos honor their embedded orientation (portrait/landscape) automatically.
- **Smooth zoom & pan** — fit-to-window, 100%, Ctrl+wheel zoom, and drag-to-pan when zoomed.
- **Glass HUD** — a translucent navigation bar with previous/next, position, and quick actions.
- **Thumbnail filmstrip** — a wrap-around strip (toggle with `T`) for fast visual navigation.
- **Full shell integration** — "Open With…", reveal in Explorer, copy file / image / path, set as desktop wallpaper, and safe delete to the Recycle Bin.
- **Transparency & glass** — translucent window with configurable backdrop (glass / dark / light).
- **Persistent state** — remembers window size/position and your settings in `%APPDATA%\Photonio\config.json`.

## Requirements

- **Windows 10 or 11 (x64)**
- **Rust** toolchain (stable) — https://rustup.rs
- **MSVC** build tools (the default `windows-msvc` target) — install the "Desktop development with C++" workload from the Visual Studio Build Tools.

## Building

```powershell
git clone https://github.com/limbo666/Photonio.git
cd Photonio
cargo build --release
.\target\release\photonio.exe
```

> **Always use `--release`.** Image decoding is 10–40× slower in debug builds, which makes navigation feel laggy.

## Usage

Open a file or folder from the HUD buttons (`Ctrl`+`O` / `Ctrl`+`Shift`+`O`), or drag-and-drop an image onto the window. Photonio scans the containing folder and lets you move through it with the keyboard or mouse.

### Keyboard shortcuts

| Action | Keys |
| --- | --- |
| Previous / Next image | `←` `→` · `A` `D` · Mouse wheel |
| Zoom in / out | `Ctrl` + Mouse wheel |
| Fit to window | `0` |
| Actual size (100%) | `1` |
| Pan (when zoomed) | Left-drag |
| Rotate 90° clockwise | `R` |
| Toggle filmstrip | `T` |
| Copy file path | `Ctrl` + `C` |
| Open file | `Ctrl` + `O` |
| Open folder | `Ctrl` + `Shift` + `O` |
| Close image | `Ctrl` + `W` |
| Close folder | `Ctrl` + `Shift` + `W` |
| Show in Explorer | `Ctrl` + `E` |
| Settings | `Ctrl` + `,` |
| Move to Recycle Bin | `Del` |
| Toggle fullscreen | `F11` · `Alt` + `Enter` |
| Help / shortcuts | `F1` · `H` |
| Context menu | Right-click |

### Settings

Configured in the **Settings** dialog (`Ctrl`+`,`), persisted to `%APPDATA%\Photonio\config.json`:

- Remember last opened folder on startup
- Loop navigation at folder boundaries
- Mouse wheel action (navigate or zoom)
- HUD position (top / bottom / hidden)
- Thumbnail filmstrip on/off
- Start in fullscreen
- Canvas background (translucent glass / dark / light)
- Glass panel opacity

## Project structure

| Path | Purpose |
| --- | --- |
| `src/main.rs` | Entry point, window lifecycle, icon |
| `src/app.rs` | UI state, input dispatch, HUD, context menu, filmstrip, settings |
| `src/viewer/mod.rs` | Image decoding (EXIF), texture cache, zoom/pan math |
| `src/filesystem/mod.rs` | Directory scan + natural (alphanumeric) sort |
| `src/platform/mod.rs` | Windows shell: wallpaper, Explorer, Open With, clipboard |
| `src/config.rs` | JSON config persistence |
| `src/assets.rs` | Embedded logo |
| `src/loader.rs` | Background decode workers (full-res + thumbnail) |
| `src/about.rs` | About modal |

## License

Released under the **Apache License 2.0**. See [LICENSE](LICENSE).

Built by [Nikos Georgousis](https://www.georgousis.info).
