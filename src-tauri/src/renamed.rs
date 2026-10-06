//! Dipstick was Pulse until 0.1.10. Pulse's folder moves in
//! `paths::data_dir`; its login entry moves here. An installed Pulse's entry
//! is carried over by the installer too (`installer-hooks.nsh`), but a dev
//! copy has no installer, and the installer can't read what Task Manager
//! says about the entry.

use tauri::AppHandle;

#[cfg(windows)]
const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Task Manager's on and off for each entry in `RUN`.
#[cfg(windows)]
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// Pulse started at login, so Dipstick does; turned off in Task Manager,
/// it stays off. Only once Pulse's folder has moved: until then, Pulse may
/// still be the one in use.
#[cfg(windows)]
pub fn carry_login_item(app: &AppHandle) {
    use tauri_plugin_autostart::ManagerExt;
    let _settled = crate::paths::data_dir();
    if crate::paths::old_data_dir().exists() {
        return;
    }
    let (old, new) = (crate::OLD_APP_NAME, crate::APP_NAME);
    if registry::exists(RUN, old) && app.autolaunch().enable().is_ok() {
        registry::delete(RUN, old);
    }
    if let Some(state) = registry::binary(APPROVED, old) {
        if !approved(&state) {
            registry::set_binary(APPROVED, new, &state);
        }
        registry::delete(APPROVED, old);
    }
}

#[cfg(not(windows))]
pub fn carry_login_item(_app: &AppHandle) {}

/// As the autostart plugin reads it: Task Manager keeps when it turned an
/// entry off in the last eight bytes, and zeros while it is on.
#[cfg_attr(not(windows), allow(dead_code))]
fn approved(state: &[u8]) -> bool {
    state.len() < 8 || state.iter().rev().take(8).all(|byte| *byte == 0)
}

/// Values under `HKEY_CURRENT_USER`.
#[cfg(windows)]
mod registry {
    use std::ptr::null_mut;
    use windows_sys::Win32::System::Registry::{
        RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_BINARY, RRF_RT_ANY, RRF_RT_REG_BINARY,
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn exists(key: &str, name: &str) -> bool {
        let (key, name) = (wide(key), wide(name));
        // SAFETY: both names are NUL-terminated and outlive the call. With no
        // buffer, it only says whether the value is there.
        unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_ANY, null_mut(), null_mut(), null_mut()) == 0 }
    }

    pub fn binary(key: &str, name: &str) -> Option<Vec<u8>> {
        let (key, name) = (wide(key), wide(name));
        let mut size: u32 = 0;
        // SAFETY: as in `exists`; the first call only asks the size, and the
        // second is given a buffer of that size.
        unsafe {
            let read = |buffer: *mut u8, size: &mut u32| {
                RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_BINARY, null_mut(), buffer.cast(), size)
            };
            if read(null_mut(), &mut size) != 0 {
                return None;
            }
            let mut bytes = vec![0u8; size as usize];
            if read(bytes.as_mut_ptr(), &mut size) != 0 {
                return None;
            }
            bytes.truncate(size as usize);
            Some(bytes)
        }
    }

    pub fn set_binary(key: &str, name: &str, bytes: &[u8]) {
        let (key, name) = (wide(key), wide(name));
        // SAFETY: both names are NUL-terminated, and `bytes` is as long as said.
        unsafe {
            RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), REG_BINARY, bytes.as_ptr().cast(), bytes.len() as u32);
        }
    }

    pub fn delete(key: &str, name: &str) {
        let (key, name) = (wide(key), wide(name));
        // SAFETY: both names are NUL-terminated and outlive the call.
        unsafe {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_managers_off_is_told_apart_from_on() {
        // What the autostart plugin writes when it turns an entry on.
        assert!(approved(&[2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        // Turned off: 3, then when, as a FILETIME.
        assert!(!approved(&[3, 0, 0, 0, 0x60, 0x2c, 0x8f, 0x1e, 0x3b, 0x37, 0xdc, 0x01]));
        // Too short to say: on, as the plugin reads it.
        assert!(approved(&[3]));
    }
}
