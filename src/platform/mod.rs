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

/// Place `path` on the clipboard as `CF_HDROP` so Explorer can paste the file
/// itself (a real file copy, not just text).
pub fn copy_file_to_clipboard(path: &Path) -> Result<(), String> {
    // Keep the clipboard open across the write.
    let _clip = clipboard_win::Clipboard::new_attempts(5)
        .map_err(|e| format!("open clipboard: {e}"))?;
    clipboard_win::raw::set_file_list(&[path.display().to_string()])
        .map_err(|e| format!("set file list: {e}"))
}
