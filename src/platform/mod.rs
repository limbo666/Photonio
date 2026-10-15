// File: src/platform/mod.rs
// Purpose: Photonio — Windows shell integrations: Explorer reveal, "Open With",
//          desktop wallpaper (SystemParametersInfoW), and CF_HDROP file-copy.

use std::os::windows::ffi::OsStrExt; // UTF-16 encoding for Win32 wide-string APIs
use std::path::Path;

/// Reveal `path` in Windows Explorer, pre-selecting the file.
/// `/select,` must stay glued to the path as a single argument.
pub fn reveal_in_explorer(path: &Path) {
    let _ = std::process::Command::new("explorer.exe")
        .arg(format!("/select,{}", path.display()))
        .spawn();
}

/// Open the Windows "How do you want to open this file?" dialog.
pub fn open_with(path: &Path) {
    let _ = std::process::Command::new("openwith.exe").arg(path).spawn();
}

/// Open the Windows Settings → Apps → Default apps page.
pub fn open_default_apps_settings() {
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", "ms-settings:defaultapps"])
        .spawn();
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

mod windows;

pub use windows::{
    add_folder_context_menu, check_registration_status, clean_all_registry_entries,
    is_folder_context_menu_registered, remove_folder_context_menu, update_registration,
    RegistryStatus,
};
