//! Автозапуск при старте Windows.
//! Пишем/удаляем запись в реестре:
//! HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run

use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt;
use tracing::{info, warn};
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyW, RegDeleteValueW, RegQueryValueExW, RegSetValueExW,
    HKEY_CURRENT_USER, REG_SZ,
};

const SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "voicy";

fn to_wide(s: &str) -> Vec<u16> {
    OsString::from(s)
        .encode_wide()
        .chain(Some(0))
        .collect()
}

fn exe_path_quoted() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let path = exe.to_string_lossy().to_string();
    // Если в пути есть пробелы — оборачиваем в кавычки
    Some(if path.contains(' ') {
        format!("\"{}\"", path)
    } else {
        path
    })
}

/// Проверить, есть ли voicy в автозапуске.
pub fn is_enabled() -> bool {
    let subkey = to_wide(SUBKEY);
    let name = to_wide(VALUE_NAME);
    let mut hkey = 0;

    unsafe {
        let open = RegCreateKeyW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            &mut hkey,
        );
        if open != ERROR_SUCCESS {
            return false;
        }

        let mut buf_len: u32 = 0;
        let query = RegQueryValueExW(
            hkey,
            name.as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut buf_len,
        );
        RegCloseKey(hkey);
        query == ERROR_SUCCESS
    }
}

/// Добавить voicy в автозапуск.
pub fn enable() -> anyhow::Result<()> {
    let path = exe_path_quoted().ok_or_else(|| anyhow::anyhow!("не удалось получить путь к exe"))?;
    let subkey = to_wide(SUBKEY);
    let name = to_wide(VALUE_NAME);
    let data: Vec<u16> = OsString::from(&path)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let data_bytes = data.len() * 2;

    let mut hkey = 0;
    unsafe {
        let open = RegCreateKeyW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            &mut hkey,
        );
        if open != ERROR_SUCCESS {
            anyhow::bail!("RegCreateKeyExW failed: {}", open);
        }

        let set = RegSetValueExW(
            hkey,
            name.as_ptr(),
            0,
            REG_SZ,
            data.as_ptr() as *const u8,
            data_bytes as u32,
        );
        RegCloseKey(hkey);
        if set != ERROR_SUCCESS {
            anyhow::bail!("RegSetValueExW failed: {}", set);
        }
    }
    info!("[startup] добавлен в автозапуск: {}", path);
    Ok(())
}

/// Удалить voicy из автозапуска.
pub fn disable() -> anyhow::Result<()> {
    let subkey = to_wide(SUBKEY);
    let name = to_wide(VALUE_NAME);
    let mut hkey = 0;

    unsafe {
        let open = RegCreateKeyW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            &mut hkey,
        );
        if open != ERROR_SUCCESS {
            anyhow::bail!("RegCreateKeyExW failed: {}", open);
        }

        let del = RegDeleteValueW(hkey, name.as_ptr());
        RegCloseKey(hkey);
        if del != ERROR_SUCCESS && del != ERROR_FILE_NOT_FOUND {
            anyhow::bail!("RegDeleteValueW failed: {}", del);
        }
    }
    info!("[startup] удалён из автозапуска");
    Ok(())
}

/// Привести реестр в соответствие с флагом конфига.
pub fn sync_with_config(enabled: bool) {
    let currently = is_enabled();
    if enabled && !currently {
        if let Err(e) = enable() {
            warn!("[startup] не удалось включить автозапуск: {}", e);
        }
    } else if !enabled && currently {
        if let Err(e) = disable() {
            warn!("[startup] не удалось выключить автозапуск: {}", e);
        }
    }
}
