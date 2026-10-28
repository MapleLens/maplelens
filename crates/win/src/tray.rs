//! The notification-area icon: right-click menu (Show overlays, the training card's switches and session, Settings,
//! Update, Exit), double-click opens
//! settings, the tooltip names the version (and a waiting update), and the global show/hide hotkey
//! (RegisterHotKey: a registered shortcut, not a keyboard hook).

use std::cell::RefCell;
use std::rc::Rc;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, UnregisterHotKey,
};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_USER, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, DestroyWindow, GetCursorPos, HICON, MF_CHECKED, MF_SEPARATOR, MF_STRING,
    PostMessageW, RegisterWindowMessageW, SetForegroundWindow, TPM_BOTTOMALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TrackPopupMenu, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_HOTKEY, WM_LBUTTONDBLCLK, WM_NULL, WM_RBUTTONUP,
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::winclass;

/// Private message the shell sends us about mouse events on the icon.
const WM_APP_TRAY: u32 = WM_APP + 1;
const ID_TOGGLE: usize = 1;
const ID_SETTINGS: usize = 2;
const ID_EXIT: usize = 3;
const ID_UPDATE: usize = 4;
const ID_EXP: usize = 5;
const ID_PAUSE: usize = 6;
const ID_NEW_SESSION: usize = 7;
const ID_DROPS: usize = 8;
const HOTKEY_ID: i32 = 1;
const TIP: &str = concat!("MapleLens v", env!("CARGO_PKG_VERSION"));

/// The icon's hover text: the version, and a newer one when it is waiting.
pub fn tooltip(update: Option<&str>) -> String {
    match update {
        Some(version) => format!("{TIP} - new update available (v{version})"),
        None => TIP.to_string(),
    }
}

/// What the tray asks the app to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayCommand {
    Toggle,
    ToggleExp,
    /// The map drops panel's switch.
    ToggleDrops,
    /// Pause or resume the training session.
    PauseTraining,
    NewSession,
    Settings,
    Update,
    Exit,
}

/// Hotkey-control flags (HOTKEYF_*: shift 1, ctrl 2, alt 4) to RegisterHotKey flags (MOD_*): the two use
/// different bit values.
pub fn register_modifiers(packed: u32) -> HOT_KEY_MODIFIERS {
    let flags = packed >> 8;
    let mut mods = MOD_NOREPEAT;
    if flags & 0x4 != 0 {
        mods |= MOD_ALT;
    }
    if flags & 0x2 != 0 {
        mods |= MOD_CONTROL;
    }
    if flags & 0x1 != 0 {
        mods |= MOD_SHIFT;
    }
    mods
}

/// Messages arrive while the main loop pumps them, so commands are queued on the main thread.
pub struct Tray {
    hwnd: HWND,
    icon: HICON,
    hotkey: bool,
    state: Rc<RefCell<TrayState>>,
}

/// What the menu's check marks and labels show.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MenuState {
    pub overlay_enabled: bool,
    pub exp: bool,
    pub drops: bool,
    pub paused: bool,
    /// Set while a newer release is waiting.
    pub update: Option<String>,
}

#[derive(Default)]
struct TrayState {
    menu: MenuState,
    commands: Vec<TrayCommand>,
}

impl Tray {
    pub fn new(icon: HICON) -> windows::core::Result<Self> {
        let menu = MenuState { overlay_enabled: true, exp: true, drops: true, ..MenuState::default() };
        let state = Rc::new(RefCell::new(TrayState { menu, commands: Vec::new() }));
        // The shell broadcasts this when explorer restarts and the tray is rebuilt: our icon must be re-added.
        // SAFETY: plain registration of a system-wide message name.
        let taskbar_created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
        let shared = state.clone();
        let handler: winclass::Handler = Box::new(move |hwnd, msg, wparam, lparam| {
            if msg == WM_APP_TRAY {
                match lparam.0 as u32 {
                    WM_RBUTTONUP => {
                        let menu = shared.borrow().menu.clone();
                        if let Some(command) = show_menu(hwnd, &menu) {
                            shared.borrow_mut().commands.push(command);
                        }
                    }
                    WM_LBUTTONDBLCLK => shared.borrow_mut().commands.push(TrayCommand::Settings),
                    _ => {}
                }
                return Some(LRESULT(0));
            }
            if msg == WM_HOTKEY && wparam.0 == HOTKEY_ID as usize {
                shared.borrow_mut().commands.push(TrayCommand::Toggle);
                return Some(LRESULT(0));
            }
            if msg == taskbar_created {
                let tip = tooltip(shared.borrow().menu.update.as_deref());
                notify(NIM_ADD, hwnd, icon, &tip);
                return Some(LRESULT(0));
            }
            None
        });
        let hwnd = winclass::create_window(
            "MapleLensTray",
            "MapleLens",
            (WINDOW_EX_STYLE(0), WINDOW_STYLE(0)),
            (0, 0, 0, 0),
            icon,
            handler,
        )?;
        notify(NIM_ADD, hwnd, icon, &tooltip(None));
        Ok(Self { hwnd, icon, hotkey: false, state })
    }

    #[cfg(test)]
    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    /// The check mark in front of "Show overlays".
    pub fn set_overlay_enabled(&self, enabled: bool) {
        self.state.borrow_mut().menu.overlay_enabled = enabled;
    }

    /// The training card's and the map drops panel's switches, and whether the session is paused.
    pub fn set_training(&self, exp: bool, drops: bool, paused: bool) {
        let menu = &mut self.state.borrow_mut().menu;
        (menu.exp, menu.drops, menu.paused) = (exp, drops, paused);
    }

    /// Announce (or clear) a newer release in the icon's tooltip and the menu.
    pub fn set_update(&self, version: Option<String>) {
        if self.state.borrow().menu.update != version {
            notify(NIM_MODIFY, self.hwnd, self.icon, &tooltip(version.as_deref()));
            self.state.borrow_mut().menu.update = version;
        }
    }

    /// A notification from the icon (a toast on Windows 10 and 11).
    pub fn announce(&self, title: &str, text: &str) {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            uFlags: NIF_INFO,
            dwInfoFlags: NIIF_USER,
            hBalloonIcon: self.icon,
            ..Default::default()
        };
        copy_text(&mut data.szInfoTitle, title);
        copy_text(&mut data.szInfo, text);
        // SAFETY: valid struct for our own icon, which NIM_ADD made in `new`.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
        }
    }

    pub fn take_commands(&self) -> Vec<TrayCommand> {
        std::mem::take(&mut self.state.borrow_mut().commands)
    }

    /// Register the global toggle hotkey (0 clears it). False if another program already owns the combo.
    pub fn set_hotkey(&mut self, packed: u32) -> bool {
        // SAFETY: our own window and hotkey id.
        unsafe {
            if self.hotkey {
                let _ = UnregisterHotKey(Some(self.hwnd), HOTKEY_ID);
                self.hotkey = false;
            }
            if packed & 0xFF == 0 {
                return true;
            }
            self.hotkey = RegisterHotKey(Some(self.hwnd), HOTKEY_ID, register_modifiers(packed), packed & 0xFF).is_ok();
        }
        self.hotkey
    }

    /// Feed a message to the tray window as the shell would (tests).
    #[cfg(test)]
    pub fn simulate(&self, msg: u32, wparam: usize, lparam: isize) {
        // SAFETY: our own window; the message is handled before this returns.
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                self.hwnd,
                msg,
                Some(WPARAM(wparam)),
                Some(LPARAM(lparam)),
            );
        }
    }

    #[cfg(test)]
    pub const TRAY_MESSAGE: u32 = WM_APP_TRAY;
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.set_hotkey(0);
        notify(NIM_DELETE, self.hwnd, self.icon, TIP);
        // SAFETY: our own window.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// Add, change or remove our notification-area icon.
fn notify(action: windows::Win32::UI::Shell::NOTIFY_ICON_MESSAGE, hwnd: HWND, icon: HICON, tip: &str) {
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_APP_TRAY,
        hIcon: icon,
        ..Default::default()
    };
    copy_text(&mut data.szTip, tip);
    // SAFETY: valid struct for our own window.
    unsafe {
        let _ = Shell_NotifyIconW(action, &data);
    }
}

/// Into one of the shell's fixed text fields, cut to fit; the last slot stays 0 (a NUL-terminated string).
fn copy_text(field: &mut [u16], text: &str) {
    let room = field.len().saturating_sub(1);
    for (slot, unit) in field.iter_mut().take(room).zip(text.encode_utf16()) {
        *slot = unit;
    }
}

fn show_menu(hwnd: HWND, state: &MenuState) -> Option<TrayCommand> {
    let checked = |on: bool| if on { MF_STRING | MF_CHECKED } else { MF_STRING };
    // SAFETY: the menu is created and destroyed here; TrackPopupMenu blocks until the user chooses.
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        let _ = AppendMenuW(menu, checked(state.overlay_enabled), ID_TOGGLE, w!("Show overlays"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, checked(state.exp), ID_EXP, w!("EXP tracker"));
        let _ = AppendMenuW(menu, checked(state.drops), ID_DROPS, w!("Map drops"));
        let pause = if state.paused { w!("Resume training session") } else { w!("Pause training session") };
        let _ = AppendMenuW(menu, MF_STRING, ID_PAUSE, pause);
        let _ = AppendMenuW(menu, MF_STRING, ID_NEW_SESSION, w!("New training session"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_SETTINGS, w!("Settings"));
        if let Some(version) = &state.update {
            let _ = AppendMenuW(menu, MF_STRING, ID_UPDATE, &HSTRING::from(format!("Update to v{version}")));
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_EXIT, w!("Exit"));
        let mut point = Default::default();
        let _ = GetCursorPos(&mut point);
        // Windows only dismisses a tray menu on an outside click if our window is foreground when it opens.
        let _ = SetForegroundWindow(hwnd);
        let choice =
            TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, point.x, point.y, None, hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0)); // documented workaround for a sticky menu
        let _ = DestroyMenu(menu);
        match choice.0 as usize {
            ID_TOGGLE => Some(TrayCommand::Toggle),
            ID_EXP => Some(TrayCommand::ToggleExp),
            ID_DROPS => Some(TrayCommand::ToggleDrops),
            ID_PAUSE => Some(TrayCommand::PauseTraining),
            ID_NEW_SESSION => Some(TrayCommand::NewSession),
            ID_SETTINGS => Some(TrayCommand::Settings),
            ID_UPDATE => Some(TrayCommand::Update),
            ID_EXIT => Some(TrayCommand::Exit),
            _ => None,
        }
    }
}
