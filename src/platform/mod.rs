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

mod windows;

pub use windows::{
    add_folder_context_menu, check_registration_status, clean_all_registry_entries,
    is_folder_context_menu_registered, remove_folder_context_menu, update_registration,
    RegistryStatus,
};
