//! Locate the MapleStory window and its client area on screen, and read the mouse.
//!
//! Anti-cheat friendly by construction: everything here is a read-only question to the window manager, the
//! same calls Discord's "share a window" picker, OBS or any screenshot tool makes. The game process is never
//! opened (not even to read its name: the process list snapshot carries names already), nothing is injected,
//! hooked or sent to it.

use maplelens_core::config::GAME_PROCESS;
use maplelens_core::geometry::{Point, Rect};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClientRect, GetCursorPos, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow,
    IsWindowVisible, SetProcessDPIAware,
};
use windows::core::BOOL;

/// Without this, Windows reports scaled coordinates on HiDPI displays and the overlay lands in the wrong place.
/// Must run before any coordinate is read.
pub fn make_dpi_aware() {
    // SAFETY: plain process-wide setting, called once at startup before any window exists.
    unsafe {
        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).is_err() {
            let _ = SetProcessDPIAware();
        }
    }
}

/// Process IDs whose executable is the game, from the system's process list.
fn game_pids() -> Vec<u32> {
    let mut pids = Vec::new();
    // SAFETY: the snapshot handle is closed below; PROCESSENTRY32W is initialised with its size as required.
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return pids };
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snapshot, &mut entry).is_ok();
        while ok {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            if String::from_utf16_lossy(&entry.szExeFile[..len]).eq_ignore_ascii_case(GAME_PROCESS) {
                pids.push(entry.th32ProcessID);
            }
            ok = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    pids
}

struct Search {
    pids: Vec<u32>,
    found: Option<HWND>,
}

unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> BOOL {
    // SAFETY: `data` is the &mut Search passed to EnumWindows below, alive for the whole enumeration.
    let search = unsafe { &mut *(data.0 as *mut Search) };
    let mut pid = 0u32;
    // SAFETY: plain queries on a window handle the system just gave us.
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if IsWindowVisible(hwnd).as_bool() && search.pids.contains(&pid) {
            search.found = Some(hwnd);
            return BOOL(0); // stop: the first visible game window is the one
        }
    }
    BOOL(1)
}

/// The visible MapleStory window, or None when the game is not running.
pub fn find_game_window() -> Option<HWND> {
    let pids = game_pids();
    if pids.is_empty() {
        return None;
    }
    let mut search = Search { pids, found: None };
    // SAFETY: the callback only runs during this call and only touches `search` through the pointer.
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM(&mut search as *mut Search as isize));
    }
    search.found
}

pub fn is_alive(hwnd: HWND) -> bool {
    // SAFETY: IsWindow accepts any value, including stale handles.
    unsafe { IsWindow(Some(hwnd)).as_bool() }
}

/// Game client area (no title bar or borders) in screen coordinates.
pub fn client_rect(hwnd: HWND) -> Option<Rect> {
    let mut rect = RECT::default();
    let mut origin = POINT::default();
    // SAFETY: out-parameters are valid locals.
    unsafe {
        GetClientRect(hwnd, &mut rect).ok()?;
        ClientToScreen(hwnd, &mut origin).ok().ok()?;
    }
    Some(Rect::new(origin.x, origin.y, rect.right - rect.left, rect.bottom - rect.top))
}

/// True while the game is the window the user is working in (not minimised, not behind another app).
pub fn is_foreground(hwnd: HWND) -> bool {
    // SAFETY: plain queries.
    unsafe { GetForegroundWindow() == hwnd && !IsIconic(hwnd).as_bool() }
}

pub fn cursor() -> Option<Point> {
    let mut point = POINT::default();
    // SAFETY: out-parameter is a valid local.
    unsafe { GetCursorPos(&mut point).ok()? };
    Some(Point { x: point.x, y: point.y })
}
