//! One copy at a time (two would draw two cards), and the message boxes a start can end with: started from
//! Explorer there is no console to print to, and a start that only quits looks like a broken download.
//!
//! A second start is caught before it asks to run as administrator (`elevate`), which would otherwise be a prompt
//! only to be told MapleLens is already running.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, GetLastError};
use windows::Win32::System::Threading::{CreateMutexW, OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE};
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND, MessageBoxW};
use windows::core::{HSTRING, w};

const MUTEX: &str = "Local\\MapleLens.SingleInstance";
/// An update starts the new version while the old one is still on its way out, holding the mutex.
pub const UPDATE_HANDOVER: Duration = Duration::from_secs(10);
const RETRY: Duration = Duration::from_millis(250);

pub const ALREADY_RUNNING: &str = "MapleLens is already running.\n\nIts meso coin is in the tray, by the clock \
                                   (Windows may tuck it under the ^ arrow). Right-click it for Settings or Exit.";

/// Is another copy running? Only looks, never creates the mutex, so it can be asked before the elevation prompt (the
/// copy started as administrator creates it).
pub fn another_is_running() -> bool {
    exists(&HSTRING::from(MUTEX))
}

/// Hold the mutex for the life of the process; false while another copy holds it, after waiting up to `wait` for it
/// to leave.
pub fn claim(wait: Duration) -> bool {
    take(&HSTRING::from(MUTEX), wait)
}

/// Say something in a box of its own (the player started MapleLens from Explorer: nothing else would show it).
pub fn message(text: &str, error: bool) {
    let icon = if error { MB_ICONERROR } else { MB_ICONINFORMATION };
    // SAFETY: plain strings that outlive the call, which returns once the box is closed.
    unsafe {
        let _ = MessageBoxW(None, &HSTRING::from(text), w!("MapleLens"), MB_OK | icon | MB_SETFOREGROUND);
    }
}

fn exists(name: &HSTRING) -> bool {
    // SAFETY: opening a named mutex only to learn that it exists; the handle is closed at once.
    unsafe {
        match OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, name) {
            Ok(handle) => {
                let _ = CloseHandle(handle);
                true
            }
            // A copy running as administrator keeps this one out of its mutex: it is there all the same.
            Err(error) => error.code() == ERROR_ACCESS_DENIED.to_hresult(),
        }
    }
}

fn take(name: &HSTRING, wait: Duration) -> bool {
    let deadline = Instant::now() + wait;
    loop {
        // SAFETY: the mutex made here is kept until the process exits; one another copy made is closed again.
        let taken = unsafe {
            match CreateMutexW(None, false, name) {
                Ok(handle) if GetLastError() == ERROR_ALREADY_EXISTS => {
                    let _ = CloseHandle(handle);
                    false
                }
                Ok(_) => true,
                Err(error) => error.code() != ERROR_ACCESS_DENIED.to_hresult(),
            }
        };
        if taken || Instant::now() >= deadline {
            return taken;
        }
        std::thread::sleep(RETRY);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::core::HSTRING;

    use super::{exists, take};

    fn test_name(case: &str) -> HSTRING {
        HSTRING::from(format!("Local\\MapleLens.Test.{case}.{}", std::process::id()))
    }

    #[test]
    fn a_second_copy_sees_the_first() {
        let name = test_name("second");
        assert!(!exists(&name));
        assert!(take(&name, Duration::ZERO));
        assert!(exists(&name));
        assert!(!take(&name, Duration::ZERO));
    }

    #[test]
    fn an_updated_copy_waits_for_the_old_one_to_leave() {
        let name = test_name("handover");
        // SAFETY: a test mutex, closed by the thread below.
        let old = unsafe { CreateMutexW(None, false, &name) }.unwrap().0 as usize;
        let leaving = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            // SAFETY: the handle made above, closed once.
            unsafe { CloseHandle(HANDLE(old as *mut _)) }.unwrap();
        });
        assert!(!take(&name, Duration::ZERO));
        assert!(take(&name, Duration::from_secs(5)));
        leaving.join().unwrap();
    }

    #[test]
    #[ignore = "needs MapleLens running, ideally as administrator with this test not"]
    fn live_a_running_copy_is_seen_before_the_prompt() {
        assert!(super::another_is_running());
    }
}
