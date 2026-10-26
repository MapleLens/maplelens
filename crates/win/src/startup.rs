//! Launch at Windows sign-in through the per-user Run key (no admin rights; removing the value undoes it).

use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW,
    RegDeleteValueW, RegSetValueExW,
};
use windows::core::{HSTRING, w};

pub const VALUE_NAME: &str = "MapleLens";

/// The exe, quoted (paths with spaces must survive the shell's parsing of the Run entry).
pub fn launch_command() -> String {
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
    format!("\"{exe}\"")
}

#[cfg(test)]
pub fn is_enabled(name: &str) -> bool {
    use windows::Win32::System::Registry::{KEY_QUERY_VALUE, RegOpenKeyExW, RegQueryValueExW};
    let mut key = HKEY::default();
    // SAFETY: the key is closed before returning.
    unsafe {
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Run"),
            None,
            KEY_QUERY_VALUE,
            &mut key,
        )
        .is_err()
        {
            return false;
        }
        let found = RegQueryValueExW(key, &HSTRING::from(name), None, None, None, None).is_ok();
        let _ = RegCloseKey(key);
        found
    }
}

pub fn set_enabled(enabled: bool, name: &str) -> windows::core::Result<()> {
    let mut key = HKEY::default();
    // SAFETY: CreateKeyEx, not OpenKey: a fresh profile (e.g. a CI runner) has no Run key yet. Closed below.
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Run"),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .ok()?;
        let result = if enabled {
            let value: Vec<u8> = launch_command().encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
            RegSetValueExW(key, &HSTRING::from(name), None, REG_SZ, Some(&value)).ok()
        } else {
            let status = RegDeleteValueW(key, &HSTRING::from(name));
            if status == ERROR_FILE_NOT_FOUND { Ok(()) } else { status.ok() }
        };
        let _ = RegCloseKey(key);
        result
    }
}
