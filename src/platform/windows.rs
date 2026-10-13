// File: src/platform/windows.rs
// Purpose: Photonio — Windows registry integration: default-app registration,
//          portable-path health checks, and re-registration.

use std::path::{Path, PathBuf};

/// Health of Photonio's Windows registration relative to the running exe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryStatus {
    NotRegistered,
    Valid { registered_path: PathBuf },
    Mismatch { registered_path: PathBuf, current_path: PathBuf },
}

/// Supported image extensions registered for Windows file association.
const REGISTERED_EXTENSIONS: &[&str] =
    &[".jpg", ".jpeg", ".png", ".webp", ".bmp", ".gif", ".tiff"];

/// Check whether the HKCU registration points at the current executable.
pub fn check_registration_status() -> RegistryStatus {
    let current = match std::env::current_exe() {
        Ok(path) => path,
        Err(_) => return RegistryStatus::NotRegistered,
    };

    match read_command_path(r"HKCU\Software\Classes\Photonio.Viewer\shell\open\command") {
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
    reg_add(
        r"HKCU\Software\Classes\Photonio.Viewer",
        &["/ve", "/t", "REG_SZ", "/d", "Photonio Image", "/f"],
    )?;
    let icon = format!("\"{}\",0", exe);
    reg_add(
        r"HKCU\Software\Classes\Photonio.Viewer\DefaultIcon",
        &["/ve", "/t", "REG_SZ", "/d", icon.as_str(), "/f"],
    )?;
    let open_cmd = format!("\"{}\" \"%1\"", exe);
    reg_add(
        r"HKCU\Software\Classes\Photonio.Viewer\shell\open\command",
        &["/ve", "/t", "REG_SZ", "/d", open_cmd.as_str(), "/f"],
    )?;

    // 2. Capabilities + FileAssociations.
    reg_add(
        r"HKCU\Software\Photonio\Capabilities",
        &["/v", "ApplicationName", "/t", "REG_SZ", "/d", "Photonio", "/f"],
    )?;
    reg_add(
        r"HKCU\Software\Photonio\Capabilities",
        &[
            "/v",
            "ApplicationDescription",
            "/t",
            "REG_SZ",
            "/d",
            "Fast, lightweight GPU image viewer",
            "/f",
        ],
    )?;
    for &ext in REGISTERED_EXTENSIONS {
        reg_add(
            r"HKCU\Software\Photonio\Capabilities\FileAssociations",
            &["/v", ext, "/t", "REG_SZ", "/d", "Photonio.Viewer", "/f"],
        )?;
    }

    // 3. RegisteredApplications entry.
    reg_add(
        r"HKCU\Software\RegisteredApplications",
        &[
            "/v",
            "Photonio",
            "/t",
            "REG_SZ",
            "/d",
            r"Software\Photonio\Capabilities",
            "/f",
        ],
    )?;

    // 4. OpenWithProgids (visible in Explorer "Open with" submenu).
    for &ext in REGISTERED_EXTENSIONS {
        let key = format!(r"HKCU\Software\Classes\{}\OpenWithProgids", ext);
        reg_add(&key, &["/v", "Photonio.Viewer", "/t", "REG_NONE", "/f"])?;
    }

    Ok(())
}

/// Read a registry command value and extract the executable path from it.
fn read_command_path(key: &str) -> Option<PathBuf> {
    let output = std::process::Command::new("reg.exe")
        .args(["query", key, "/ve"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let start = text.find('"')? + 1;
    let rest = &text[start..];
    let end = rest.find('"')?;
    if end == 0 {
        return None;
    }
    Some(PathBuf::from(&rest[..end]))
}

/// Compare two Windows paths case-insensitively.
fn paths_equal(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

/// Run `reg.exe add` for a key with the given options.
fn reg_add(key: &str, args: &[&str]) -> Result<(), String> {
    let status = std::process::Command::new("reg.exe")
        .arg("add")
        .arg(key)
        .args(args)
        .status()
        .map_err(|e| format!("run reg.exe: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("reg add failed for key: {key}"))
    }
}

/// Run `reg.exe delete` for a key; a missing key is a no-op success.
fn reg_delete(key: &str) -> Result<(), String> {
    let _ = std::process::Command::new("reg.exe")
        .arg("delete")
        .arg(key)
        .arg("/f")
        .status()
        .map_err(|e| format!("run reg.exe: {e}"))?;
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
    reg_add(
        r"HKCU\Software\Classes\Directory\shell\Photonio",
        &["/ve", "/t", "REG_SZ", "/d", "Open folder with Photonio", "/f"],
    )?;
    reg_add(
        r"HKCU\Software\Classes\Directory\shell\Photonio",
        &["/v", "Icon", "/t", "REG_SZ", "/d", icon.as_str(), "/f"],
    )?;
    let folder_cmd = format!("\"{}\" \"%1\"", exe);
    reg_add(
        r"HKCU\Software\Classes\Directory\shell\Photonio\command",
        &["/ve", "/t", "REG_SZ", "/d", folder_cmd.as_str(), "/f"],
    )?;

    // Folder background right-click.
    reg_add(
        r"HKCU\Software\Classes\Directory\Background\shell\Photonio",
        &["/ve", "/t", "REG_SZ", "/d", "Open folder with Photonio", "/f"],
    )?;
    reg_add(
        r"HKCU\Software\Classes\Directory\Background\shell\Photonio",
        &["/v", "Icon", "/t", "REG_SZ", "/d", icon.as_str(), "/f"],
    )?;
    let bg_cmd = format!("\"{}\" \"%V\"", exe);
    reg_add(
        r"HKCU\Software\Classes\Directory\Background\shell\Photonio\command",
        &["/ve", "/t", "REG_SZ", "/d", bg_cmd.as_str(), "/f"],
    )?;

    Ok(())
}

/// Remove the "Open folder with Photonio" Explorer folder context-menu entries.
pub fn remove_folder_context_menu() -> Result<(), String> {
    reg_delete(r"HKCU\Software\Classes\Directory\shell\Photonio")?;
    reg_delete(r"HKCU\Software\Classes\Directory\Background\shell\Photonio")?;
    Ok(())
}

/// True if the folder context-menu entries exist and point at the current exe.
pub fn is_folder_context_menu_registered() -> bool {
    let Ok(current) = std::env::current_exe() else {
        return false;
    };
    let folder = read_command_path(r"HKCU\Software\Classes\Directory\shell\Photonio\command");
    let background =
        read_command_path(r"HKCU\Software\Classes\Directory\Background\shell\Photonio\command");
    match (folder, background) {
        (Some(a), Some(b)) => paths_equal(&a, &current) && paths_equal(&b, &current),
        _ => false,
    }
}
