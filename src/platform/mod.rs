// File: src/platform/mod.rs
// Purpose: Photonio — Windows shell integrations: Explorer reveal, "Open With",
//          desktop wallpaper (SystemParametersInfoW), and CF_HDROP file-copy.

use std::os::windows::ffi::OsStrExt; // UTF-16 encoding for Win32 wide-string APIs
use std::path::Path;

/// Reveal `path` in Windows Explorer, pre-selecting the file.
///
/// Uses the Shell COM API (`SHOpenFolderAndSelectItems`) rather than the
/// `explorer.exe /select,<path>` command line. Explorer does not parse its
/// command line with the usual MSVC quoting rules: once the path contains a
/// space, `Command::arg` wraps the whole `/select,<path>` argument in quotes
/// and Explorer silently falls back to opening the default shell folder
/// (Documents). Selecting through COM involves no command-line parsing.
pub fn reveal_in_explorer(path: &Path) {
    use windows_sys::Win32::UI::Shell::{
        Common::ITEMIDLIST, ILFindLastID, ILFree, SHOpenFolderAndSelectItems, SHParseDisplayName,
    };

    let Some(parent) = path.parent() else {
        return;
    };

    let item_wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let folder_wide: Vec<u16> = parent
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: `SHParseDisplayName` allocates PIDLs with the shell allocator and
    // `ILFree` releases them; the wide buffers are NUL-terminated and outlive
    // the calls. Every pointer passed is valid for the duration of the call.
    let opened = unsafe {
        let mut item: *mut ITEMIDLIST = std::ptr::null_mut();
        if SHParseDisplayName(
            item_wide.as_ptr(),
            std::ptr::null_mut(),
            &mut item,
            0,
            std::ptr::null_mut(),
        ) != 0
        {
            false
        } else {
            let mut folder: *mut ITEMIDLIST = std::ptr::null_mut();
            let ok = SHParseDisplayName(
                folder_wide.as_ptr(),
                std::ptr::null_mut(),
                &mut folder,
                0,
                std::ptr::null_mut(),
            ) == 0;
            if ok {
                // `ILFindLastID` yields the file's ID relative to its parent.
                let child: *const ITEMIDLIST = ILFindLastID(item);
                SHOpenFolderAndSelectItems(folder, 1, &child, 0);
            }
            ILFree(item);
            if !folder.is_null() {
                ILFree(folder);
            }
            ok
        }
    };

    // Extremely unusual parse failure: fall back to opening the containing
    // folder so the user still lands in the right place.
    if !opened {
        open_folder(parent);
    }
}

/// Open the Windows "How do you want to open this file?" dialog.
pub fn open_with(path: &Path) {
    let _ = std::process::Command::new("openwith.exe").arg(path).spawn();
}

/// Open the Windows Settings → Apps → Default apps page.
pub fn open_default_apps_settings() {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let target: Vec<u16> = "ms-settings:defaultapps"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            std::ptr::null(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
    }
}

/// Open `dir` (a directory) in a new Explorer window.
pub fn open_folder(dir: &Path) {
    let _ = std::process::Command::new("explorer.exe").arg(dir).spawn();
}

/// List available drive roots (e.g. `C:\`) via `GetLogicalDrives`. Unlike
/// probing `A:\`..`Z:\` with `Path::exists`, this never stalls on disconnected
/// network drives.
pub fn list_logical_drives() -> Vec<String> {
    use windows_sys::Win32::Storage::FileSystem::GetLogicalDrives;

    // SAFETY: `GetLogicalDrives` takes no arguments and returns a bitmask of
    // the currently mounted drive letters.
    let mask = unsafe { GetLogicalDrives() };
    let mut drives = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) != 0 {
            let letter = (b'A' + i as u8) as char;
            drives.push(format!("{letter}:\\"));
        }
    }
    drives
}

/// Query a drive's volume label (e.g. `C:\` → "Local Disk"). Returns `None`
/// for unlabeled drives or if the Win32 call fails. Callers should cache the
/// result — this touches the volume and must not run on a hot UI loop.
pub fn drive_volume_label(root: &str) -> Option<String> {
    use windows_sys::Win32::Storage::FileSystem::GetVolumeInformationW;

    let root_wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
    let mut label = vec![0u16; 261]; // MAX_PATH (260) + NUL

    // SAFETY: `root_wide` is a NUL-terminated buffer that outlives the call;
    // `label` is a writable buffer of `label.len()` u16s. All other out-params
    // are null pointers because only the label is needed.
    let ok = unsafe {
        GetVolumeInformationW(
            root_wide.as_ptr(),
            label.as_mut_ptr(),
            label.len() as u32,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    };
    if ok == 0 {
        return None;
    }

    let len = label.iter().position(|&c| c == 0).unwrap_or(label.len());
    let label = String::from_utf16_lossy(&label[..len]).trim().to_string();
    if label.is_empty() {
        None
    } else {
        Some(label)
    }
}

/// Set the desktop wallpaper to `path` via the Win32 API.
pub fn set_wallpaper(path: &Path) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_SETDESKWALLPAPER, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE,
    };

    // `pvParam` must be a NUL-terminated UTF-16 path buffer.
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: `wide` is a valid, NUL-terminated buffer and outlives the call.
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_SETDESKWALLPAPER,
            0,
            wide.as_ptr() as *mut core::ffi::c_void,
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        )
    };

    if ok != 0 {
        Ok(())
    } else {
        Err("SystemParametersInfoW reported failure".to_string())
    }
}

/// Launch the native Windows Print Wizard for `path` via the shell "print"
/// verb (works on Windows 10/11 regardless of which app owns the file type).
pub fn print_image(path: &Path) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    // NUL-terminated UTF-16 buffers for the verb and the file path.
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let verb: Vec<u16> = "print".encode_utf16().chain(std::iter::once(0)).collect();

    // SAFETY: `wide` and `verb` are valid NUL-terminated UTF-16 buffers that
    // outlive the call; the remaining parameters are valid nulls.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            wide.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };

    // ShellExecuteW returns a value greater than 32 on success.
    if (result as isize) > 32 {
        Ok(())
    } else {
        Err(format!("ShellExecuteW (print) failed: code {}", result as isize))
    }
}

/// Place `path` on the clipboard as `CF_HDROP` so Explorer can paste the file
/// itself (a real file copy, not just text).
pub fn copy_file_to_clipboard(path: &Path) -> Result<(), String> {
    // Keep the clipboard open across the write.
    let _clip = clipboard_win::Clipboard::new_attempts(5)
        .map_err(|e| format!("open clipboard: {e}"))?;
    clipboard_win::raw::set_file_list(&[path.display().to_string()])
        .map_err(|e| format!("set file list: {e}"))
}

/// Returns the primary monitor's working area `[x, y, width, height]`,
/// excluding the Windows taskbar and docked appbars.
pub fn get_work_area() -> Option<[f32; 4]> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETWORKAREA};

    let mut rect: RECT = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            &mut rect as *mut _ as *mut core::ffi::c_void,
            0,
        )
    };
    if ok != 0 {
        let w = (rect.right - rect.left).max(0) as f32;
        let h = (rect.bottom - rect.top).max(0) as f32;
        Some([rect.left as f32, rect.top as f32, w, h])
    } else {
        None
    }
}

/// Clamps a window size and position within a given monitor work area `[x, y, width, height]`.
///
/// Ensures minimum size (320x240), keeps the top-left / titlebar within visible bounds,
/// and shifts the window left/up if it overflows the right or bottom edge of the monitor.
pub fn clamp_to_work_area(
    pos: [f32; 2],
    size: [f32; 2],
    work_area: [f32; 4],
) -> ([f32; 2], [f32; 2]) {
    let [work_x, work_y, work_w, work_h] = work_area;
    let max_w = (work_w - 40.0).max(320.0);
    let max_h = (work_h - 40.0).max(240.0);

    let clamped_size = [
        size[0].clamp(320.0, max_w),
        size[1].clamp(240.0, max_h),
    ];

    // Keep window titlebar / frame accessible
    let max_x = (work_x + work_w - 100.0).max(work_x);
    let max_y = (work_y + work_h - 60.0).max(work_y);
    let mut x = pos[0].clamp(work_x, max_x);
    let mut y = pos[1].clamp(work_y, max_y);

    // Shift if bottom or right hangs off screen
    if x + clamped_size[0] > work_x + work_w {
        x = (work_x + work_w - clamped_size[0]).max(work_x);
    }
    if y + clamped_size[1] > work_y + work_h {
        y = (work_y + work_h - clamped_size[1]).max(work_y);
    }

    (clamped_size, [x, y])
}

/// Query the work area `[x, y, width, height]` of the monitor that intersects
/// the specified window rectangle. If the rectangle does not intersect any active
/// monitor (e.g. monitor was disconnected or coordinates are out of bounds), returns `None`.
#[cfg(target_os = "windows")]
pub fn find_monitor_work_area(x: f32, y: f32, w: f32, h: f32) -> Option<[f32; 4]> {
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONULL,
    };

    let rect = RECT {
        left: x.round() as i32,
        top: y.round() as i32,
        right: (x + w).round() as i32,
        bottom: (y + h).round() as i32,
    };

    let mut hmon = unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONULL) };
    if hmon.is_null() {
        let pt = POINT {
            x: x.round() as i32,
            y: y.round() as i32,
        };
        hmon = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONULL) };
    }

    if hmon.is_null() {
        return None;
    }

    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;

    let ok = unsafe { GetMonitorInfoW(hmon, &mut info) };
    if ok != 0 {
        let r = info.rcWork;
        let mw = (r.right - r.left).max(0) as f32;
        let mh = (r.bottom - r.top).max(0) as f32;
        if mw > 0.0 && mh > 0.0 {
            Some([r.left as f32, r.top as f32, mw, mh])
        } else {
            None
        }
    } else {
        None
    }
}

#[cfg(not(target_os = "windows"))]
pub fn find_monitor_work_area(_x: f32, _y: f32, _w: f32, _h: f32) -> Option<[f32; 4]> {
    None
}

/// Resolves and validates window size and position across multi-monitor topologies.
///
/// If a saved position is provided and intersects an active monitor (including secondary
/// monitors with negative or displaced coordinates), the window is safely constrained
/// within that monitor's usable work area.
///
/// If the saved position is out of bounds or the monitor was disconnected/removed,
/// the position resets to default parameters: centered on the Primary Monitor with safe dimensions.
pub fn resolve_window_geometry(
    saved_pos: Option<[f32; 2]>,
    saved_size: Option<[f32; 2]>,
    default_size: [f32; 2],
) -> ([f32; 2], Option<[f32; 2]>) {
    let primary_work = get_work_area().unwrap_or([0.0, 0.0, 1920.0, 1080.0]);

    if let Some(pos) = saved_pos {
        let req_size = saved_size.unwrap_or(default_size);
        if let Some(mon_work) = find_monitor_work_area(pos[0], pos[1], req_size[0], req_size[1]) {
            let (size, clamped_pos) = clamp_to_work_area(pos, req_size, mon_work);
            return (size, Some(clamped_pos));
        }
        // Monitor is disconnected or out of bounds -> fall through to reset on primary monitor!
    }

    // Default / Reset: center on primary monitor
    let [work_x, work_y, work_w, work_h] = primary_work;
    let max_w = (work_w - 40.0).max(320.0);
    let max_h = (work_h - 40.0).max(240.0);

    let size = if let Some(s) = saved_size {
        [s[0].clamp(320.0, max_w), s[1].clamp(240.0, max_h)]
    } else {
        [default_size[0].min(max_w), default_size[1].min(max_h)]
    };

    let cx = (work_x + (work_w - size[0]) / 2.0).max(work_x);
    let cy = (work_y + (work_h - size[1]) / 2.0).max(work_y);

    (size, Some([cx, cy]))
}

/// Attach a Win32 window subclass to handle taskbar "Close window" when minimized.
///
/// On Windows, the OS does not dispatch `WM_PAINT` to iconic (minimized) windows.
/// Because `winit`/`eframe` relies on `WM_PAINT` to dispatch `RedrawRequested` and
/// process `CloseRequested`, closing a minimized window from the taskbar context menu
/// stalls indefinitely in `GetMessageW` until the window is restored/maximized.
/// This subclass intercepts `WM_CLOSE` and `WM_SYSCOMMAND (SC_CLOSE)`. If the window
/// is currently iconic, it immediately terminates the process cleanly via `std::process::exit(0)`.
#[cfg(target_os = "windows")]
pub fn attach_minimized_close_handler(hwnd: windows_sys::Win32::Foundation::HWND) {
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IsIconic, WM_CLOSE, WM_NCDESTROY, WM_SYSCOMMAND,
    };

    type SubclassProc = unsafe extern "system" fn(
        HWND,
        u32,
        WPARAM,
        LPARAM,
        usize,
        usize,
    ) -> LRESULT;

    #[link(name = "comctl32")]
    extern "system" {
        fn SetWindowSubclass(
            hWnd: HWND,
            pfnSubclass: SubclassProc,
            uIdSubclass: usize,
            dwRefData: usize,
        ) -> i32;

        fn RemoveWindowSubclass(
            hWnd: HWND,
            pfnSubclass: SubclassProc,
            uIdSubclass: usize,
        ) -> i32;

        fn DefSubclassProc(
            hWnd: HWND,
            uMsg: u32,
            wParam: WPARAM,
            lParam: LPARAM,
        ) -> LRESULT;
    }

    const SUBCLASS_ID: usize = 0x5048_4F54; // 'PHOT'
    const SC_CLOSE_MASK: usize = 0xFFF0;
    const SC_CLOSE_VAL: usize = 0xF060;

    unsafe extern "system" fn subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        uid: usize,
        _ref_data: usize,
    ) -> LRESULT {
        let is_close_request = msg == WM_CLOSE
            || (msg == WM_SYSCOMMAND && (wparam & SC_CLOSE_MASK) == SC_CLOSE_VAL);

        if is_close_request && IsIconic(hwnd) != 0 {
            std::process::exit(0);
        } else if msg == WM_NCDESTROY {
            RemoveWindowSubclass(hwnd, subclass_proc, uid);
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }

    unsafe {
        SetWindowSubclass(hwnd, subclass_proc, SUBCLASS_ID, 0);
    }
}

/// Fallback helper to discover the main window `HWND` for the current thread.
#[cfg(target_os = "windows")]
pub fn get_current_thread_window() -> Option<windows_sys::Win32::Foundation::HWND> {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumThreadWindows, GetClassNameW};

    let thread_id = unsafe { GetCurrentThreadId() };
    let mut found_hwnd: HWND = std::ptr::null_mut();

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> i32 {
        let mut class_name = [0u16; 64];
        let len = GetClassNameW(hwnd, class_name.as_mut_ptr(), class_name.len() as i32);
        if len > 0 {
            let name = String::from_utf16_lossy(&class_name[..len as usize]);
            if name == "Window Class" {
                *(lparam as *mut HWND) = hwnd;
                return 0; // stop enumeration
            }
        }
        1 // continue
    }

    unsafe {
        EnumThreadWindows(
            thread_id,
            Some(enum_proc),
            &mut found_hwnd as *mut HWND as LPARAM,
        );
    }

    if !found_hwnd.is_null() {
        Some(found_hwnd)
    } else {
        None
    }
}

mod windows;

pub use windows::{
    add_folder_context_menu, check_registration_status, clean_all_registry_entries,
    is_folder_context_menu_registered, remove_folder_context_menu, start_drag_files,
    update_registration, RegistryStatus,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_area_returns_valid_dimensions() {
        if let Some([_x, _y, w, h]) = get_work_area() {
            assert!(w > 0.0);
            assert!(h > 0.0);
        }
    }

    #[test]
    fn test_clamp_to_work_area_preserves_valid_multimonitor_coordinates() {
        // Monitor 2 to the right: [1920, 0, 1920, 1080]
        let work_area = [1920.0, 0.0, 1920.0, 1080.0];
        let pos = [2100.0, 150.0];
        let size = [960.0, 640.0];

        let (clamped_size, clamped_pos) = clamp_to_work_area(pos, size, work_area);
        assert_eq!(clamped_size, size);
        assert_eq!(clamped_pos, pos);

        // Monitor 2 to the left: [-1920, 0, 1920, 1080]
        let work_area_left = [-1920.0, 0.0, 1920.0, 1080.0];
        let pos_left = [-1500.0, 100.0];
        let (clamped_size_left, clamped_pos_left) = clamp_to_work_area(pos_left, size, work_area_left);
        assert_eq!(clamped_size_left, size);
        assert_eq!(clamped_pos_left, pos_left);
    }

    #[test]
    fn test_clamp_to_work_area_shifts_overflowing_window() {
        let work_area = [0.0, 0.0, 1920.0, 1080.0];
        // Window placed too far right: 1500 + 960 = 2460 > 1920
        let pos = [1500.0, 100.0];
        let size = [960.0, 640.0];

        let (clamped_size, clamped_pos) = clamp_to_work_area(pos, size, work_area);
        assert_eq!(clamped_size, size);
        assert_eq!(clamped_pos[0], 1920.0 - 960.0);
        assert_eq!(clamped_pos[1], 100.0);
    }

    #[test]
    fn test_find_monitor_work_area_distinguishes_valid_and_out_of_bounds() {
        #[cfg(target_os = "windows")]
        {
            // Primary monitor (0, 0, 800, 600) should be found
            let primary = find_monitor_work_area(0.0, 0.0, 800.0, 600.0);
            assert!(primary.is_some());

            // Wild out-of-bounds coordinates should return None
            let out_of_bounds = find_monitor_work_area(999_999.0, 999_999.0, 800.0, 600.0);
            assert!(out_of_bounds.is_none());
        }
    }

    #[test]
    fn test_resolve_window_geometry_resets_when_out_of_bounds() {
        let default_size = [960.0, 640.0];
        let (size, pos) = resolve_window_geometry(
            Some([999_999.0, 999_999.0]),
            Some([1000.0, 700.0]),
            default_size,
        );

        assert!(size[0] >= 320.0);
        assert!(size[1] >= 240.0);
        assert!(pos.is_some());
        let [x, y] = pos.unwrap();
        // Since it reset to primary monitor, x and y must be within primary monitor work area
        if let Some([wx, wy, ww, wh]) = get_work_area() {
            assert!(x >= wx && x < wx + ww);
            assert!(y >= wy && y < wy + wh);
        }
    }
}
