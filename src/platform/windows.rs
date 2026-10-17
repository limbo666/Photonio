// File: src/platform/windows.rs
// Purpose: Photonio — Windows registry integration: default-app registration,
//          portable-path health checks, and re-registration.
//          Uses in-process Win32 Registry APIs so no console processes (reg.exe)
//          are ever spawned, completely eliminating window flickering/popups.

use std::path::{Path, PathBuf};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyW, RegDeleteKeyW, RegDeleteTreeW, RegDeleteValueW,
    RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_READ, KEY_WRITE, REG_NONE, REG_SZ,
};

/// Health of Photonio's Windows registration relative to the running exe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryStatus {
    NotRegistered,
    Valid { registered_path: PathBuf },
    Mismatch { registered_path: PathBuf, current_path: PathBuf },
}

/// Supported image extensions registered for Windows file association.
const REGISTERED_EXTENSIONS: &[&str] =
    &[".jpg", ".jpeg", ".jfif", ".png", ".webp", ".bmp", ".gif", ".tiff"];

/// Check whether the HKCU registration points at the current executable.
pub fn check_registration_status() -> RegistryStatus {
    let current = match std::env::current_exe() {
        Ok(path) => path,
        Err(_) => return RegistryStatus::NotRegistered,
    };

    match read_command_path(r"Software\Classes\Photonio.Viewer\shell\open\command") {
        None => RegistryStatus::NotRegistered,
        Some(registered) => {
            if paths_equal(&registered, &current) {
                RegistryStatus::Valid { registered_path: registered }
            } else {
                RegistryStatus::Mismatch {
                    registered_path: registered,
                    current_path: current,
                }
            }
        }
    }
}

/// (Re-)register Photonio with Windows using the current executable path,
/// overwriting all prior entries (no admin elevation required).
pub fn update_registration() -> Result<(), String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("resolve exe: {e}"))?
        .to_string_lossy()
        .to_string();

    // 1. ProgID.
    reg_set_string(
        HKEY_CURRENT_USER,
        r"Software\Classes\Photonio.Viewer",
        None,
        "Photonio Image",
    )?;
    let icon = format!("\"{}\",0", exe);
    reg_set_string(
        HKEY_CURRENT_USER,
        r"Software\Classes\Photonio.Viewer\DefaultIcon",
        None,
        &icon,
    )?;
    let open_cmd = format!("\"{}\" \"%1\"", exe);
    reg_set_string(
        HKEY_CURRENT_USER,
        r"Software\Classes\Photonio.Viewer\shell\open\command",
        None,
        &open_cmd,
    )?;

    // 2. Capabilities + FileAssociations.
    reg_set_string(
        HKEY_CURRENT_USER,
        r"Software\Photonio\Capabilities",
        Some("ApplicationName"),
        "Photonio",
    )?;
    reg_set_string(
        HKEY_CURRENT_USER,
        r"Software\Photonio\Capabilities",
        Some("ApplicationDescription"),
        "Fast, lightweight GPU image viewer",
    )?;
    for &ext in REGISTERED_EXTENSIONS {
        reg_set_string(
            HKEY_CURRENT_USER,
            r"Software\Photonio\Capabilities\FileAssociations",
            Some(ext),
            "Photonio.Viewer",
        )?;
    }

    // 3. RegisteredApplications entry.
    reg_set_string(
        HKEY_CURRENT_USER,
        r"Software\RegisteredApplications",
        Some("Photonio"),
        r"Software\Photonio\Capabilities",
    )?;

    // 4. OpenWithProgids (visible in Explorer "Open with" submenu).
    for &ext in REGISTERED_EXTENSIONS {
        let key = format!(r"Software\Classes\{}\OpenWithProgids", ext);
        reg_set_none(HKEY_CURRENT_USER, &key, "Photonio.Viewer")?;
    }

    Ok(())
}

/// Add "Open folder with Photonio" to the Explorer folder and folder-background
/// context menus (HKCU, no admin).
pub fn add_folder_context_menu() -> Result<(), String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("resolve exe: {e}"))?
        .to_string_lossy()
        .to_string();
    let icon = format!("\"{}\",0", exe);

    // Folder right-click.
    reg_set_string(
        HKEY_CURRENT_USER,
        r"Software\Classes\Directory\shell\Photonio",
        None,
        "Open folder with Photonio",
    )?;
    reg_set_string(
        HKEY_CURRENT_USER,
        r"Software\Classes\Directory\shell\Photonio",
        Some("Icon"),
        &icon,
    )?;
    let folder_cmd = format!("\"{}\" \"%1\"", exe);
    reg_set_string(
        HKEY_CURRENT_USER,
        r"Software\Classes\Directory\shell\Photonio\command",
        None,
        &folder_cmd,
    )?;

    // Folder background right-click.
    reg_set_string(
        HKEY_CURRENT_USER,
        r"Software\Classes\Directory\Background\shell\Photonio",
        None,
        "Open folder with Photonio",
    )?;
    reg_set_string(
        HKEY_CURRENT_USER,
        r"Software\Classes\Directory\Background\shell\Photonio",
        Some("Icon"),
        &icon,
    )?;
    let bg_cmd = format!("\"{}\" \"%V\"", exe);
    reg_set_string(
        HKEY_CURRENT_USER,
        r"Software\Classes\Directory\Background\shell\Photonio\command",
        None,
        &bg_cmd,
    )?;

    Ok(())
}

/// Remove the "Open folder with Photonio" Explorer folder context-menu entries.
pub fn remove_folder_context_menu() -> Result<(), String> {
    reg_delete_tree(HKEY_CURRENT_USER, r"Software\Classes\Directory\shell\Photonio")?;
    reg_delete_tree(HKEY_CURRENT_USER, r"Software\Classes\Directory\Background\shell\Photonio")?;
    Ok(())
}

/// True if the folder context-menu entries exist and point at the current exe.
pub fn is_folder_context_menu_registered() -> bool {
    let Ok(current) = std::env::current_exe() else {
        return false;
    };
    let folder = read_command_path(r"Software\Classes\Directory\shell\Photonio\command");
    let background =
        read_command_path(r"Software\Classes\Directory\Background\shell\Photonio\command");
    match (folder, background) {
        (Some(a), Some(b)) => paths_equal(&a, &current) && paths_equal(&b, &current),
        _ => false,
    }
}

/// Remove every registry entry Photonio may have written: ProgID, capabilities,
/// registered-applications value, open-with progids, and folder context menus.
pub fn clean_all_registry_entries() -> Result<(), String> {
    // ProgID tree.
    reg_delete_tree(HKEY_CURRENT_USER, r"Software\Classes\Photonio.Viewer")?;
    // Capabilities key, then the (now-empty) parent if possible.
    reg_delete_tree(HKEY_CURRENT_USER, r"Software\Photonio\Capabilities")?;
    let _ = reg_delete_tree(HKEY_CURRENT_USER, r"Software\Photonio");
    // RegisteredApplications value.
    reg_delete_value(HKEY_CURRENT_USER, r"Software\RegisteredApplications", "Photonio")?;
    // OpenWithProgids values, one per supported extension.
    for &ext in REGISTERED_EXTENSIONS {
        let key = format!(r"Software\Classes\{}\OpenWithProgids", ext);
        let _ = reg_delete_value(HKEY_CURRENT_USER, &key, "Photonio.Viewer");
    }
    // Folder context-menu entries.
    remove_folder_context_menu()?;
    Ok(())
}

// -----------------------------------------------------------------------------
// Native Win32 Registry Helpers
// -----------------------------------------------------------------------------

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Read the (Default) string value of `subkey` under `root` (e.g. `HKEY_CURRENT_USER`).
fn reg_read_default_string(root: HKEY, subkey: &str) -> Option<String> {
    let subkey_wide = to_wide(subkey);
    let mut key: HKEY = 0 as _;
    let status = unsafe { RegOpenKeyExW(root, subkey_wide.as_ptr(), 0, KEY_READ, &mut key) };
    if status != 0 {
        return None;
    }

    let mut data_type: u32 = 0;
    let mut data_len: u32 = 0;

    // First call to determine buffer length.
    let status = unsafe {
        RegQueryValueExW(
            key,
            std::ptr::null(),
            std::ptr::null(),
            &mut data_type,
            std::ptr::null_mut(),
            &mut data_len,
        )
    };
    if status != 0 || data_len == 0 {
        unsafe { RegCloseKey(key) };
        return None;
    }

    let mut buf = vec![0u8; data_len as usize];
    let status = unsafe {
        RegQueryValueExW(
            key,
            std::ptr::null(),
            std::ptr::null(),
            &mut data_type,
            buf.as_mut_ptr(),
            &mut data_len,
        )
    };
    unsafe { RegCloseKey(key) };

    if status != 0 {
        return None;
    }

    // REG_SZ (1) or REG_EXPAND_SZ (2).
    if data_type == REG_SZ || data_type == 2 {
        let u16_len = data_len as usize / 2;
        let u16_slice: &[u16] =
            unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u16, u16_len) };
        let len = u16_slice.iter().position(|&c| c == 0).unwrap_or(u16_slice.len());
        Some(String::from_utf16_lossy(&u16_slice[..len]))
    } else {
        None
    }
}

/// Set a string (REG_SZ) value on `subkey` under `root`. If `value_name` is `None`, sets (Default).
fn reg_set_string(
    root: HKEY,
    subkey: &str,
    value_name: Option<&str>,
    value: &str,
) -> Result<(), String> {
    let subkey_wide = to_wide(subkey);
    let mut key: HKEY = 0 as _;
    let status = unsafe { RegCreateKeyW(root, subkey_wide.as_ptr(), &mut key) };
    if status != 0 {
        return Err(format!("RegCreateKeyW failed for {subkey}: code {status}"));
    }

    let val_wide = to_wide(value);
    let byte_len = (val_wide.len() * std::mem::size_of::<u16>()) as u32;

    let val_name_wide = value_name.map(to_wide);
    let val_name_ptr = val_name_wide.as_ref().map_or(std::ptr::null(), |v| v.as_ptr());

    let status = unsafe {
        RegSetValueExW(
            key,
            val_name_ptr,
            0,
            REG_SZ,
            val_wide.as_ptr() as *const u8,
            byte_len,
        )
    };
    unsafe { RegCloseKey(key) };

    if status == 0 {
        Ok(())
    } else {
        Err(format!("RegSetValueExW failed for {subkey}: code {status}"))
    }
}

/// Set a REG_NONE (zero-length) value on `subkey` under `root` (used for OpenWithProgids).
fn reg_set_none(root: HKEY, subkey: &str, value_name: &str) -> Result<(), String> {
    let subkey_wide = to_wide(subkey);
    let mut key: HKEY = 0 as _;
    let status = unsafe { RegCreateKeyW(root, subkey_wide.as_ptr(), &mut key) };
    if status != 0 {
        return Err(format!("RegCreateKeyW failed for {subkey}: code {status}"));
    }

    let val_name_wide = to_wide(value_name);
    let status = unsafe {
        RegSetValueExW(
            key,
            val_name_wide.as_ptr(),
            0,
            REG_NONE,
            std::ptr::null(),
            0,
        )
    };
    unsafe { RegCloseKey(key) };

    if status == 0 {
        Ok(())
    } else {
        Err(format!("RegSetValueExW failed for {subkey}: code {status}"))
    }
}

/// Delete a registry key and all of its subkeys and values. Missing key is a no-op success.
fn reg_delete_tree(root: HKEY, subkey: &str) -> Result<(), String> {
    let subkey_wide = to_wide(subkey);
    let status = unsafe { RegDeleteTreeW(root, subkey_wide.as_ptr()) };
    // 0 = SUCCESS, 2 = ERROR_FILE_NOT_FOUND, 3 = ERROR_PATH_NOT_FOUND
    if status == 0 || status == 2 || status == 3 {
        // RegDeleteTreeW deletes descendants. Call RegDeleteKeyW to ensure key itself is removed.
        let _ = unsafe { RegDeleteKeyW(root, subkey_wide.as_ptr()) };
        Ok(())
    } else {
        Err(format!("RegDeleteTreeW failed for {subkey}: code {status}"))
    }
}

/// Delete a value from a registry key. Missing key or value is a no-op success.
fn reg_delete_value(root: HKEY, subkey: &str, value_name: &str) -> Result<(), String> {
    let subkey_wide = to_wide(subkey);
    let mut key: HKEY = 0 as _;
    let status = unsafe { RegOpenKeyExW(root, subkey_wide.as_ptr(), 0, KEY_WRITE, &mut key) };
    if status == 2 || status == 3 {
        return Ok(());
    }
    if status != 0 {
        return Err(format!("RegOpenKeyExW failed for {subkey}: code {status}"));
    }

    let val_name_wide = to_wide(value_name);
    let status = unsafe { RegDeleteValueW(key, val_name_wide.as_ptr()) };
    unsafe { RegCloseKey(key) };

    if status == 0 || status == 2 {
        Ok(())
    } else {
        Err(format!(
            "RegDeleteValueW failed for {subkey}\\{value_name}: code {status}"
        ))
    }
}

/// Read a command value and extract the executable path from it.
fn read_command_path(subkey: &str) -> Option<PathBuf> {
    let subkey = subkey.strip_prefix(r"HKCU\").unwrap_or(subkey);
    let text = reg_read_default_string(HKEY_CURRENT_USER, subkey)?;
    if let Some(start) = text.find('"') {
        let rest = &text[start + 1..];
        if let Some(end) = rest.find('"') {
            if end > 0 {
                return Some(PathBuf::from(&rest[..end]));
            }
        }
    }
    let first = text.split_whitespace().next()?;
    Some(PathBuf::from(first))
}

/// Compare two Windows paths case-insensitively.
fn paths_equal(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_equal_is_case_insensitive() {
        assert!(paths_equal(
            Path::new("C:/Apps/Photonio.exe"),
            Path::new("c:/apps/photonio.exe")
        ));
        assert!(!paths_equal(
            Path::new("C:/Apps/Photonio.exe"),
            Path::new("C:/Apps/Other.exe")
        ));
    }

    #[test]
    fn to_wide_appends_null_terminator() {
        let wide = to_wide("abc");
        assert_eq!(wide, vec!['a' as u16, 'b' as u16, 'c' as u16, 0]);
    }
}
