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
const REGISTERED_EXTENSIONS: &[&str] = &[
    ".jpg", ".jpeg", ".jfif", ".png", ".webp", ".bmp", ".gif", ".tiff",
    ".tga", ".svg", ".svgz", ".jxl", ".avif",
];

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

// -----------------------------------------------------------------------------
// OLE Drag-and-Drop DropSource & DataObject (CF_HDROP)
// -----------------------------------------------------------------------------

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicU32, Ordering};
use windows_sys::core::GUID;
use windows_sys::Win32::Foundation::{
    DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DV_E_FORMATETC, E_NOINTERFACE, E_NOTIMPL, E_POINTER,
    S_FALSE, S_OK,
};
use windows_sys::Win32::System::Com::{FORMATETC, STGMEDIUM};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GHND};
use windows_sys::Win32::System::Ole::{
    DoDragDrop, OleInitialize, DROPEFFECT_COPY, DROPEFFECT_MOVE,
};
use windows_sys::Win32::UI::Shell::DROPFILES;

pub const CF_UNICODETEXT: u16 = 13;
pub const CF_HDROP: u16 = 15;
const TYMED_HGLOBAL: u32 = 1;
const DVASPECT_CONTENT: u32 = 1;
const DATADIR_GET: u32 = 1;
const DRAGDROP_S_USEDEFAULTCURSORS: i32 = 0x00040102;
const DATA_S_SAMEFORMATETC: i32 = 0x00040102;
const OLE_E_ADVISENOTSUPPORTED: i32 = 0x80040003u32 as i32;

const IID_IUNKNOWN: GUID = GUID {
    data1: 0x00000000,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

const IID_IDATAOBJECT: GUID = GUID {
    data1: 0x0000010e,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

const IID_IDROPSOURCE: GUID = GUID {
    data1: 0x00000121,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

const IID_IENUMFORMATETC: GUID = GUID {
    data1: 0x00000103,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

fn guid_eq(a: &GUID, b: &GUID) -> bool {
    a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
}

/// Create an HGLOBAL containing a `DROPFILES` header and double-null terminated
/// UTF-16 wide paths suitable for `CF_HDROP`.
pub fn create_hdrop_global(paths: &[PathBuf]) -> Option<*mut c_void> {
    if paths.is_empty() {
        return None;
    }

    let mut wide_paths: Vec<Vec<u16>> = Vec::with_capacity(paths.len());
    let mut total_chars = 0usize;
    for p in paths {
        let mut wide: Vec<u16> = p.as_os_str().encode_wide().collect();
        wide.push(0);
        total_chars += wide.len();
        wide_paths.push(wide);
    }
    // Extra terminating null for double-null termination
    total_chars += 1;

    let dropfiles_size = std::mem::size_of::<DROPFILES>();
    let total_bytes = dropfiles_size + total_chars * 2;

    unsafe {
        let hglobal = GlobalAlloc(GHND, total_bytes);
        if hglobal.is_null() {
            return None;
        }
        let ptr = GlobalLock(hglobal) as *mut u8;
        if ptr.is_null() {
            return None;
        }

        let dropfiles = ptr as *mut DROPFILES;
        (*dropfiles).pFiles = dropfiles_size as u32;
        (*dropfiles).pt.x = 0;
        (*dropfiles).pt.y = 0;
        (*dropfiles).fNC = 0;
        (*dropfiles).fWide = 1;

        let mut dest = ptr.add(dropfiles_size) as *mut u16;
        for wide in wide_paths {
            std::ptr::copy_nonoverlapping(wide.as_ptr(), dest, wide.len());
            dest = dest.add(wide.len());
        }
        *dest = 0;

        GlobalUnlock(hglobal);
        Some(hglobal as *mut c_void)
    }
}

/// Create an HGLOBAL containing UTF-16 text representation of paths (newline separated).
pub fn create_text_global(paths: &[PathBuf]) -> Option<*mut c_void> {
    if paths.is_empty() {
        return None;
    }
    let text = paths
        .iter()
        .map(|p| p.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\r\n");
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);

    let total_bytes = wide.len() * 2;
    unsafe {
        let hglobal = GlobalAlloc(GHND, total_bytes);
        if hglobal.is_null() {
            return None;
        }
        let ptr = GlobalLock(hglobal) as *mut u16;
        if ptr.is_null() {
            return None;
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
        GlobalUnlock(hglobal);
        Some(hglobal as *mut c_void)
    }
}

// --- COM Vtables ---

#[allow(non_snake_case)]
#[repr(C)]
struct IDataObjectVtbl {
    QueryInterface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32,
    AddRef: unsafe extern "system" fn(*mut c_void) -> u32,
    Release: unsafe extern "system" fn(*mut c_void) -> u32,
    GetData: unsafe extern "system" fn(*mut c_void, *const FORMATETC, *mut STGMEDIUM) -> i32,
    GetDataHere: unsafe extern "system" fn(*mut c_void, *const FORMATETC, *mut STGMEDIUM) -> i32,
    QueryGetData: unsafe extern "system" fn(*mut c_void, *const FORMATETC) -> i32,
    GetCanonicalFormatEtc:
        unsafe extern "system" fn(*mut c_void, *const FORMATETC, *mut FORMATETC) -> i32,
    SetData:
        unsafe extern "system" fn(*mut c_void, *const FORMATETC, *const STGMEDIUM, i32) -> i32,
    EnumFormatEtc: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32,
    DAdvise: unsafe extern "system" fn(
        *mut c_void,
        *const FORMATETC,
        u32,
        *mut c_void,
        *mut u32,
    ) -> i32,
    DUnadvise: unsafe extern "system" fn(*mut c_void, u32) -> i32,
    EnumDAdvise: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
}

#[allow(non_snake_case)]
#[repr(C)]
struct IDropSourceVtbl {
    QueryInterface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32,
    AddRef: unsafe extern "system" fn(*mut c_void) -> u32,
    Release: unsafe extern "system" fn(*mut c_void) -> u32,
    QueryContinueDrag: unsafe extern "system" fn(*mut c_void, i32, u32) -> i32,
    GiveFeedback: unsafe extern "system" fn(*mut c_void, u32) -> i32,
}

#[allow(non_snake_case)]
#[repr(C)]
struct IEnumFORMATETCVtbl {
    QueryInterface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32,
    AddRef: unsafe extern "system" fn(*mut c_void) -> u32,
    Release: unsafe extern "system" fn(*mut c_void) -> u32,
    Next: unsafe extern "system" fn(*mut c_void, u32, *mut FORMATETC, *mut u32) -> i32,
    Skip: unsafe extern "system" fn(*mut c_void, u32) -> i32,
    Reset: unsafe extern "system" fn(*mut c_void) -> i32,
    Clone: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
}

// --- COM Implementations ---

#[repr(C)]
struct DataObject {
    vtable: *const IDataObjectVtbl,
    ref_count: AtomicU32,
    paths: Vec<PathBuf>,
}

static DATA_OBJECT_VTBL: IDataObjectVtbl = IDataObjectVtbl {
    QueryInterface: data_obj_query_interface,
    AddRef: data_obj_add_ref,
    Release: data_obj_release,
    GetData: data_obj_get_data,
    GetDataHere: data_obj_get_data_here,
    QueryGetData: data_obj_query_get_data,
    GetCanonicalFormatEtc: data_obj_get_canonical,
    SetData: data_obj_set_data,
    EnumFormatEtc: data_obj_enum_format_etc,
    DAdvise: data_obj_d_advise,
    DUnadvise: data_obj_d_unadvise,
    EnumDAdvise: data_obj_enum_d_advise,
};

impl DataObject {
    fn new(paths: Vec<PathBuf>) -> *mut c_void {
        let obj = Box::new(DataObject {
            vtable: &DATA_OBJECT_VTBL,
            ref_count: AtomicU32::new(1),
            paths,
        });
        Box::into_raw(obj) as *mut c_void
    }
}

unsafe extern "system" fn data_obj_query_interface(
    this: *mut c_void,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> i32 {
    if ppv.is_null() || riid.is_null() {
        return E_POINTER;
    }
    let riid = &*riid;
    if guid_eq(riid, &IID_IUNKNOWN) || guid_eq(riid, &IID_IDATAOBJECT) {
        *ppv = this;
        data_obj_add_ref(this);
        S_OK
    } else {
        *ppv = std::ptr::null_mut();
        E_NOINTERFACE
    }
}

unsafe extern "system" fn data_obj_add_ref(this: *mut c_void) -> u32 {
    let obj = &*(this as *const DataObject);
    obj.ref_count.fetch_add(1, Ordering::SeqCst) + 1
}

unsafe extern "system" fn data_obj_release(this: *mut c_void) -> u32 {
    let obj = &*(this as *const DataObject);
    let prev = obj.ref_count.fetch_sub(1, Ordering::SeqCst);
    if prev == 1 {
        drop(Box::from_raw(this as *mut DataObject));
        0
    } else {
        prev - 1
    }
}

unsafe extern "system" fn data_obj_get_data(
    this: *mut c_void,
    pformatetc: *const FORMATETC,
    pmedium: *mut STGMEDIUM,
) -> i32 {
    if pformatetc.is_null() || pmedium.is_null() {
        return E_POINTER;
    }
    let format = &*pformatetc;
    let obj = &*(this as *const DataObject);

    if (format.tymed & TYMED_HGLOBAL) == 0 {
        return DV_E_FORMATETC;
    }

    if format.cfFormat == CF_HDROP {
        if let Some(hglobal) = create_hdrop_global(&obj.paths) {
            let med = &mut *pmedium;
            med.tymed = TYMED_HGLOBAL;
            med.u.hGlobal = hglobal;
            med.pUnkForRelease = std::ptr::null_mut();
            return S_OK;
        }
    } else if format.cfFormat == CF_UNICODETEXT {
        if let Some(hglobal) = create_text_global(&obj.paths) {
            let med = &mut *pmedium;
            med.tymed = TYMED_HGLOBAL;
            med.u.hGlobal = hglobal;
            med.pUnkForRelease = std::ptr::null_mut();
            return S_OK;
        }
    }

    DV_E_FORMATETC
}

unsafe extern "system" fn data_obj_get_data_here(
    _this: *mut c_void,
    _pformatetc: *const FORMATETC,
    _pmedium: *mut STGMEDIUM,
) -> i32 {
    E_NOTIMPL
}

unsafe extern "system" fn data_obj_query_get_data(
    _this: *mut c_void,
    pformatetc: *const FORMATETC,
) -> i32 {
    if pformatetc.is_null() {
        return E_POINTER;
    }
    let format = &*pformatetc;
    if (format.tymed & TYMED_HGLOBAL) != 0
        && (format.cfFormat == CF_HDROP || format.cfFormat == CF_UNICODETEXT)
    {
        S_OK
    } else {
        DV_E_FORMATETC
    }
}

unsafe extern "system" fn data_obj_get_canonical(
    _this: *mut c_void,
    _pformatetc_in: *const FORMATETC,
    pformatetc_out: *mut FORMATETC,
) -> i32 {
    if pformatetc_out.is_null() {
        return E_POINTER;
    }
    (*pformatetc_out).ptd = std::ptr::null_mut();
    DATA_S_SAMEFORMATETC
}

unsafe extern "system" fn data_obj_set_data(
    _this: *mut c_void,
    _pformatetc: *const FORMATETC,
    _pmedium: *const STGMEDIUM,
    _release: i32,
) -> i32 {
    E_NOTIMPL
}

unsafe extern "system" fn data_obj_enum_format_etc(
    _this: *mut c_void,
    direction: u32,
    ppenum: *mut *mut c_void,
) -> i32 {
    if ppenum.is_null() {
        return E_POINTER;
    }
    if direction == DATADIR_GET {
        let formats = vec![
            FORMATETC {
                cfFormat: CF_HDROP,
                ptd: std::ptr::null_mut(),
                dwAspect: DVASPECT_CONTENT,
                lindex: -1,
                tymed: TYMED_HGLOBAL,
            },
            FORMATETC {
                cfFormat: CF_UNICODETEXT,
                ptd: std::ptr::null_mut(),
                dwAspect: DVASPECT_CONTENT,
                lindex: -1,
                tymed: TYMED_HGLOBAL,
            },
        ];
        *ppenum = EnumFormatEtc::new(formats);
        S_OK
    } else {
        *ppenum = std::ptr::null_mut();
        E_NOTIMPL
    }
}

unsafe extern "system" fn data_obj_d_advise(
    _this: *mut c_void,
    _pformatetc: *const FORMATETC,
    _advf: u32,
    _padvsink: *mut c_void,
    _pdwconnection: *mut u32,
) -> i32 {
    OLE_E_ADVISENOTSUPPORTED
}

unsafe extern "system" fn data_obj_d_unadvise(_this: *mut c_void, _dwconnection: u32) -> i32 {
    OLE_E_ADVISENOTSUPPORTED
}

unsafe extern "system" fn data_obj_enum_d_advise(_this: *mut c_void, ppenum: *mut *mut c_void) -> i32 {
    if !ppenum.is_null() {
        *ppenum = std::ptr::null_mut();
    }
    OLE_E_ADVISENOTSUPPORTED
}

// --- IDropSource ---

#[repr(C)]
struct DropSource {
    vtable: *const IDropSourceVtbl,
    ref_count: AtomicU32,
}

static DROP_SOURCE_VTBL: IDropSourceVtbl = IDropSourceVtbl {
    QueryInterface: drop_src_query_interface,
    AddRef: drop_src_add_ref,
    Release: drop_src_release,
    QueryContinueDrag: drop_src_query_continue,
    GiveFeedback: drop_src_give_feedback,
};

impl DropSource {
    fn new() -> *mut c_void {
        let obj = Box::new(DropSource {
            vtable: &DROP_SOURCE_VTBL,
            ref_count: AtomicU32::new(1),
        });
        Box::into_raw(obj) as *mut c_void
    }
}

unsafe extern "system" fn drop_src_query_interface(
    this: *mut c_void,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> i32 {
    if ppv.is_null() || riid.is_null() {
        return E_POINTER;
    }
    let riid = &*riid;
    if guid_eq(riid, &IID_IUNKNOWN) || guid_eq(riid, &IID_IDROPSOURCE) {
        *ppv = this;
        drop_src_add_ref(this);
        S_OK
    } else {
        *ppv = std::ptr::null_mut();
        E_NOINTERFACE
    }
}

unsafe extern "system" fn drop_src_add_ref(this: *mut c_void) -> u32 {
    let obj = &*(this as *const DropSource);
    obj.ref_count.fetch_add(1, Ordering::SeqCst) + 1
}

unsafe extern "system" fn drop_src_release(this: *mut c_void) -> u32 {
    let obj = &*(this as *const DropSource);
    let prev = obj.ref_count.fetch_sub(1, Ordering::SeqCst);
    if prev == 1 {
        drop(Box::from_raw(this as *mut DropSource));
        0
    } else {
        prev - 1
    }
}

unsafe extern "system" fn drop_src_query_continue(
    _this: *mut c_void,
    f_escape_pressed: i32,
    grf_key_state: u32,
) -> i32 {
    const MK_LBUTTON: u32 = 0x0001;
    const MK_RBUTTON: u32 = 0x0002;
    if f_escape_pressed != 0 {
        DRAGDROP_S_CANCEL
    } else if (grf_key_state & (MK_LBUTTON | MK_RBUTTON)) == 0 {
        DRAGDROP_S_DROP
    } else {
        S_OK
    }
}

unsafe extern "system" fn drop_src_give_feedback(_this: *mut c_void, _dw_effect: u32) -> i32 {
    DRAGDROP_S_USEDEFAULTCURSORS
}

// --- IEnumFORMATETC ---

#[repr(C)]
struct EnumFormatEtc {
    vtable: *const IEnumFORMATETCVtbl,
    ref_count: AtomicU32,
    formats: Vec<FORMATETC>,
    index: usize,
}

static ENUM_FORMAT_ETC_VTBL: IEnumFORMATETCVtbl = IEnumFORMATETCVtbl {
    QueryInterface: enum_format_etc_query_interface,
    AddRef: enum_format_etc_add_ref,
    Release: enum_format_etc_release,
    Next: enum_format_etc_next,
    Skip: enum_format_etc_skip,
    Reset: enum_format_etc_reset,
    Clone: enum_format_etc_clone,
};

impl EnumFormatEtc {
    fn new(formats: Vec<FORMATETC>) -> *mut c_void {
        let obj = Box::new(EnumFormatEtc {
            vtable: &ENUM_FORMAT_ETC_VTBL,
            ref_count: AtomicU32::new(1),
            formats,
            index: 0,
        });
        Box::into_raw(obj) as *mut c_void
    }
}

unsafe extern "system" fn enum_format_etc_query_interface(
    this: *mut c_void,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> i32 {
    if ppv.is_null() || riid.is_null() {
        return E_POINTER;
    }
    let riid = &*riid;
    if guid_eq(riid, &IID_IUNKNOWN) || guid_eq(riid, &IID_IENUMFORMATETC) {
        *ppv = this;
        enum_format_etc_add_ref(this);
        S_OK
    } else {
        *ppv = std::ptr::null_mut();
        E_NOINTERFACE
    }
}

unsafe extern "system" fn enum_format_etc_add_ref(this: *mut c_void) -> u32 {
    let obj = &*(this as *const EnumFormatEtc);
    obj.ref_count.fetch_add(1, Ordering::SeqCst) + 1
}

unsafe extern "system" fn enum_format_etc_release(this: *mut c_void) -> u32 {
    let obj = &*(this as *const EnumFormatEtc);
    let prev = obj.ref_count.fetch_sub(1, Ordering::SeqCst);
    if prev == 1 {
        drop(Box::from_raw(this as *mut EnumFormatEtc));
        0
    } else {
        prev - 1
    }
}

unsafe extern "system" fn enum_format_etc_next(
    this: *mut c_void,
    celt: u32,
    rgelt: *mut FORMATETC,
    pcelt_fetched: *mut u32,
) -> i32 {
    if rgelt.is_null() || (celt > 1 && pcelt_fetched.is_null()) {
        return E_POINTER;
    }
    let obj = &mut *(this as *mut EnumFormatEtc);
    let mut fetched = 0u32;
    while fetched < celt && obj.index < obj.formats.len() {
        *rgelt.add(fetched as usize) = obj.formats[obj.index];
        obj.index += 1;
        fetched += 1;
    }
    if !pcelt_fetched.is_null() {
        *pcelt_fetched = fetched;
    }
    if fetched == celt {
        S_OK
    } else {
        S_FALSE
    }
}

unsafe extern "system" fn enum_format_etc_skip(this: *mut c_void, celt: u32) -> i32 {
    let obj = &mut *(this as *mut EnumFormatEtc);
    obj.index = (obj.index + celt as usize).min(obj.formats.len());
    if obj.index < obj.formats.len() {
        S_OK
    } else {
        S_FALSE
    }
}

unsafe extern "system" fn enum_format_etc_reset(this: *mut c_void) -> i32 {
    let obj = &mut *(this as *mut EnumFormatEtc);
    obj.index = 0;
    S_OK
}

unsafe extern "system" fn enum_format_etc_clone(
    this: *mut c_void,
    ppenum: *mut *mut c_void,
) -> i32 {
    if ppenum.is_null() {
        return E_POINTER;
    }
    let obj = &*(this as *const EnumFormatEtc);
    let cloned = EnumFormatEtc {
        vtable: &ENUM_FORMAT_ETC_VTBL,
        ref_count: AtomicU32::new(1),
        formats: obj.formats.clone(),
        index: obj.index,
    };
    *ppenum = Box::into_raw(Box::new(cloned)) as *mut c_void;
    S_OK
}

/// Initiate a native Windows OLE drag-and-drop operation for one or more files.
/// Blocks until the user drops the file onto an external target or cancels (e.g. Esc).
/// Returns true if dropped successfully, false if cancelled or error.
pub fn start_drag_files(paths: &[PathBuf]) -> bool {
    let existing: Vec<PathBuf> = paths.iter().filter(|p| p.exists()).cloned().collect();
    if existing.is_empty() {
        return false;
    }

    unsafe {
        let _ = OleInitialize(std::ptr::null_mut());

        let data_obj = DataObject::new(existing);
        let drop_src = DropSource::new();
        let mut effect = 0u32;

        let hr = DoDragDrop(
            data_obj,
            drop_src,
            DROPEFFECT_COPY | DROPEFFECT_MOVE,
            &mut effect,
        );

        data_obj_release(data_obj);
        drop_src_release(drop_src);

        // NOTE: We deliberately do NOT call OleUninitialize() here.
        // Winit initializes OLE on the main thread and registers the window's IDropTarget.
        // Calling OleUninitialize() revokes the window's IDropTarget and permanently breaks
        // incoming drag-and-drop into Photonio.

        hr == DRAGDROP_S_DROP
    }
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

    #[test]
    fn test_create_hdrop_global_layout() {
        let paths = vec![
            PathBuf::from("C:\\test\\img1.png"),
            PathBuf::from("C:\\test\\img2.jpg"),
        ];
        let hglobal = create_hdrop_global(&paths).expect("hglobal should allocate");
        assert!(!hglobal.is_null());

        unsafe {
            let ptr = GlobalLock(hglobal) as *const u8;
            assert!(!ptr.is_null());
            let dropfiles = ptr as *const DROPFILES;
            let p_files = (*dropfiles).pFiles;
            assert_eq!(p_files, std::mem::size_of::<DROPFILES>() as u32);
            let f_wide = (*dropfiles).fWide;
            assert_eq!(f_wide, 1);

            let wide_slice = std::slice::from_raw_parts(
                ptr.add(std::mem::size_of::<DROPFILES>()) as *const u16,
                100,
            );
            let s = String::from_utf16_lossy(wide_slice);
            assert!(s.starts_with("C:\\test\\img1.png\0C:\\test\\img2.jpg\0\0"));

            GlobalUnlock(hglobal);
        }
    }

    #[test]
    fn test_create_text_global_layout() {
        let paths = vec![
            PathBuf::from("C:\\test\\a.png"),
            PathBuf::from("C:\\test\\b.png"),
        ];
        let hglobal = create_text_global(&paths).expect("text hglobal should allocate");
        assert!(!hglobal.is_null());

        unsafe {
            let ptr = GlobalLock(hglobal) as *const u16;
            assert!(!ptr.is_null());
            let mut len = 0;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            let slice = std::slice::from_raw_parts(ptr, len);
            let text = String::from_utf16_lossy(slice);
            assert_eq!(text, "C:\\test\\a.png\r\nC:\\test\\b.png");
            GlobalUnlock(hglobal);
        }
    }

    #[test]
    fn test_data_object_query_interface_and_get_data() {
        let paths = vec![PathBuf::from("C:\\test\\sample.png")];
        let data_obj = DataObject::new(paths);

        unsafe {
            let mut queried: *mut c_void = std::ptr::null_mut();
            let hr = data_obj_query_interface(data_obj, &IID_IDATAOBJECT, &mut queried);
            assert_eq!(hr, S_OK);
            assert_eq!(queried, data_obj);

            let hdrop_format = FORMATETC {
                cfFormat: CF_HDROP,
                ptd: std::ptr::null_mut(),
                dwAspect: DVASPECT_CONTENT,
                lindex: -1,
                tymed: TYMED_HGLOBAL,
            };

            let hr = data_obj_query_get_data(data_obj, &hdrop_format);
            assert_eq!(hr, S_OK);

            let mut stg: STGMEDIUM = std::mem::zeroed();
            let hr = data_obj_get_data(data_obj, &hdrop_format, &mut stg);
            assert_eq!(hr, S_OK);
            assert_eq!(stg.tymed, TYMED_HGLOBAL);
            assert!(!stg.u.hGlobal.is_null());

            let bad_format = FORMATETC {
                cfFormat: 9999,
                ptd: std::ptr::null_mut(),
                dwAspect: DVASPECT_CONTENT,
                lindex: -1,
                tymed: TYMED_HGLOBAL,
            };
            let hr = data_obj_query_get_data(data_obj, &bad_format);
            assert_eq!(hr, DV_E_FORMATETC);

            let mut ppenum: *mut c_void = std::ptr::null_mut();
            let hr = data_obj_enum_format_etc(data_obj, DATADIR_GET, &mut ppenum);
            assert_eq!(hr, S_OK);
            assert!(!ppenum.is_null());

            // Release enum
            enum_format_etc_release(ppenum);

            // Release queried and initial
            data_obj_release(queried);
            data_obj_release(data_obj);
        }
    }

    #[test]
    fn test_drop_source_continue_and_feedback() {
        let drop_src = DropSource::new();
        unsafe {
            // Escape pressed -> cancel
            assert_eq!(drop_src_query_continue(drop_src, 1, 0x0001), DRAGDROP_S_CANCEL);
            // Button released -> drop
            assert_eq!(drop_src_query_continue(drop_src, 0, 0), DRAGDROP_S_DROP);
            // Left button still held -> continue S_OK
            assert_eq!(drop_src_query_continue(drop_src, 0, 0x0001), S_OK);

            assert_eq!(drop_src_give_feedback(drop_src, 1), DRAGDROP_S_USEDEFAULTCURSORS);

            drop_src_release(drop_src);
        }
    }
}
