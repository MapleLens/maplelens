//! The installer's entry in Windows' installed apps (installer/maplelens.iss). A self-update swaps the exe without
//! the installer, so at start the entry is set to this version: only its version, and only when the entry installed
//! this exe's folder (a portable copy leaves an installed one's entry alone). A per-user install files it under HKCU,
//! a per-machine one under HKLM, which only an elevated copy can change.

use std::path::Path;

use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_SET_VALUE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegGetValueW,
    RegOpenKeyExW, RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR, w};

/// Inno Setup files the entry under the script's `AppId` and "_is1".
pub const ENTRY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{702546F1-91F5-4C7A-BED5-11F8C72791DC}_is1";

/// Set the entry at `key` (under HKCU, else HKLM) to `version` when it installed `exe_dir`. True when it changed.
pub fn sync_version(key: &str, exe_dir: &Path, version: &str) -> bool {
    let key = HSTRING::from(key);
    [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE].into_iter().any(|root| {
        let installed_here =
            read(root, &key, w!("InstallLocation")).is_some_and(|dir| same_folder(Path::new(&dir), exe_dir));
        installed_here
            && read(root, &key, w!("DisplayVersion")).as_deref() != Some(version)
            && write(root, &key, w!("DisplayVersion"), version).is_ok()
    })
}

/// Inno Setup writes the folder with a trailing backslash, and either side may be spelt another way (case, 8.3 names).
fn same_folder(a: &Path, b: &Path) -> bool {
    matches!((std::fs::canonicalize(a), std::fs::canonicalize(b)), (Ok(a), Ok(b)) if a == b)
}

fn read(root: HKEY, key: &HSTRING, value: PCWSTR) -> Option<String> {
    let mut size = 0u32;
    // SAFETY: the first call only reports the size in bytes; the second writes at most that many into a buffer of it.
    unsafe {
        RegGetValueW(root, key, value, RRF_RT_REG_SZ, None, None, Some(&mut size)).ok().ok()?;
        let mut text = vec![0u16; (size as usize).div_ceil(2)];
        RegGetValueW(root, key, value, RRF_RT_REG_SZ, None, Some(text.as_mut_ptr().cast()), Some(&mut size))
            .ok()
            .ok()?;
        text.truncate(size as usize / 2);
        Some(String::from_utf16_lossy(&text).trim_end_matches('\0').to_string())
    }
}

fn write(root: HKEY, key: &HSTRING, value: PCWSTR, text: &str) -> windows::core::Result<()> {
    let data: Vec<u8> = text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
    let mut handle = HKEY::default();
    // SAFETY: an existing key opened to set one value, closed below.
    unsafe {
        RegOpenKeyExW(root, key, None, KEY_SET_VALUE, &mut handle).ok()?;
        let result = RegSetValueExW(handle, value, None, REG_SZ, Some(&data)).ok();
        let _ = RegCloseKey(handle);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Registry::{KEY_ALL_ACCESS, REG_OPTION_NON_VOLATILE, RegCreateKeyExW, RegDeleteTreeW};

    const ROOT: &str = r"Software\MapleLensTest.Installer";

    #[test]
    fn the_installers_entry_follows_a_self_update() {
        let key = format!(r"{ROOT}\{{00000000-0000-0000-0000-000000000000}}_is1");
        let dir = std::env::temp_dir().join(format!("maplelens-installer-{}", std::process::id()));
        let elsewhere = dir.join("portable");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let mut handle = HKEY::default();
        // SAFETY: a test key under HKCU, closed at once.
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                &HSTRING::from(&key),
                None,
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_ALL_ACCESS,
                None,
                &mut handle,
                None,
            )
            .ok()
            .unwrap();
            let _ = RegCloseKey(handle);
        }
        let name = HSTRING::from(&key);
        write(HKEY_CURRENT_USER, &name, w!("InstallLocation"), &format!("{}\\", dir.display())).unwrap();
        write(HKEY_CURRENT_USER, &name, w!("DisplayVersion"), "0.0.1").unwrap();

        assert!(!sync_version(&key, &elsewhere, "9.9.9"), "a copy in another folder");
        assert_eq!(read(HKEY_CURRENT_USER, &name, w!("DisplayVersion")).as_deref(), Some("0.0.1"));
        assert!(sync_version(&key, &dir, "9.9.9"));
        assert_eq!(read(HKEY_CURRENT_USER, &name, w!("DisplayVersion")).as_deref(), Some("9.9.9"));
        assert!(!sync_version(&key, &dir, "9.9.9"), "already current");

        // SAFETY: the test's own key.
        unsafe {
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(ROOT));
        }
        assert!(!sync_version(&key, &dir, "9.9.9"), "not installed");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
