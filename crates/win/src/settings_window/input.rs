//! The settings window's pointer and keyboard: hover, clicks, the slider drag, Tab focus, and the hotkey box
//! listening for a combo.

use maplelens_core::hotkey::{self, Modifiers};
use maplelens_core::render::settings_view::{self, Control};
use maplelens_core::settings::Corner;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VIRTUAL_KEY, VK_BACK,
    VK_CONTROL, VK_DELETE, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_MENU, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB,
    VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, HTCAPTION, HTCLIENT, IDC_HAND, LoadCursorW, SetCursor, WM_CAPTURECHANGED, WM_CHAR, WM_CLOSE,
    WM_DPICHANGED, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCHITTEST,
    WM_SETCURSOR, WM_SYSCHAR, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use super::{BASE_DPI, State};

fn held() -> Modifiers {
    // SAFETY: plain keyboard-state queries; the high bit is set while a key is down.
    let down = |key: VIRTUAL_KEY| unsafe { GetKeyState(i32::from(key.0)) } < 0;
    Modifiers { ctrl: down(VK_CONTROL), alt: down(VK_MENU), shift: down(VK_SHIFT) }
}

fn low_high(value: isize) -> (f32, f32) {
    (f32::from(value as u16 as i16), f32::from((value >> 16) as u16 as i16))
}

impl State {
    /// A key while the hotkey box listens: a combo sets the hotkey, Esc gives up, Backspace or Delete removes it.
    fn capture_key(&mut self, vk: VIRTUAL_KEY) {
        let modifiers = held();
        let bare = modifiers == Modifiers::default();
        if bare && vk == VK_ESCAPE {
            self.stop_capture();
        } else if bare && (vk == VK_BACK || vk == VK_DELETE) {
            self.activate(Control::ClearHotkey);
        } else if let Some(packed) = hotkey::from_press(u32::from(vk.0), modifiers) {
            self.current.toggle_hotkey = packed;
            self.changed();
            self.stop_capture();
        } else {
            let partial = if bare { String::new() } else { hotkey::partial(modifiers) };
            self.capturing = Some(partial);
        }
    }

    /// Tab, Space/Enter, the arrows on the slider, Esc. True when the key was used.
    fn navigate(&mut self, vk: VIRTUAL_KEY) -> bool {
        let order = Control::order(self.panel());
        match vk {
            VK_TAB => {
                let back = held().shift;
                let at = self.focus.and_then(|f| order.iter().position(|&c| c == f));
                let next = match (at, back) {
                    (None, false) => 0,
                    (None, true) => order.len() - 1,
                    (Some(i), false) => (i + 1) % order.len(),
                    (Some(i), true) => (i + order.len() - 1) % order.len(),
                };
                self.focus = Some(order[next]);
            }
            VK_SPACE | VK_RETURN => {
                if let Some(focus) = self.focus.filter(|f| order.contains(f)) {
                    self.activate(focus);
                }
            }
            VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN if self.focus == Some(Control::Delay) => {
                let up = vk == VK_RIGHT || vk == VK_UP;
                self.set_delay(settings_view::delay_step(self.current.expand_delay_s, up));
            }
            // In the corner picker the arrows go to the neighbouring corner and pick it.
            VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN if matches!(self.focus, Some(Control::Corner(_))) => {
                let Some(Control::Corner(at)) = self.focus else { return false };
                let (top, left) = match vk {
                    VK_LEFT | VK_RIGHT => (at.is_top(), vk == VK_LEFT),
                    _ => (vk == VK_UP, at.is_left()),
                };
                let to = Control::Corner(Corner::of(top, left));
                self.focus = Some(to);
                self.activate(to);
            }
            // Esc closes the pairing panel first, then the window.
            VK_ESCAPE if self.panel() => self.events.push(super::SettingsEvent::Phone(super::PhoneAction::HidePanel)),
            VK_ESCAPE => self.hide(),
            _ => return false,
        }
        self.show_focus = true;
        true
    }

    pub(super) fn on_message(&mut self, hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        let at = self.design(low_high(lparam.0));
        let before = self.look();
        match msg {
            WM_NCHITTEST => {
                let mut window = RECT::default();
                // SAFETY: our own window.
                unsafe { GetWindowRect(hwnd, &mut window).ok()? };
                let (sx, sy) = low_high(lparam.0);
                let (x, y) = self.design((sx - window.left as f32, sy - window.top as f32));
                let area = if settings_view::is_title(x, y) { HTCAPTION } else { HTCLIENT };
                return Some(LRESULT(area as isize));
            }
            WM_SETCURSOR => {
                let clickable = self.hover.is_some_and(|c| c != Control::Delay || !self.dragging);
                if (lparam.0 & 0xFFFF) as u32 != HTCLIENT || !clickable {
                    return None;
                }
                // SAFETY: a system cursor.
                unsafe { SetCursor(LoadCursorW(None, IDC_HAND).ok()) };
                return Some(LRESULT(1));
            }
            WM_MOUSEMOVE => {
                if !self.tracking_leave {
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    // SAFETY: our own window.
                    self.tracking_leave = unsafe { TrackMouseEvent(&mut track) }.is_ok();
                }
                if self.dragging {
                    self.set_delay(settings_view::delay_at(at.0));
                }
                self.hover = settings_view::hit(at.0, at.1, self.panel());
            }
            WM_MOUSELEAVE => (self.tracking_leave, self.hover) = (false, None),
            WM_LBUTTONDOWN => {
                let control = settings_view::hit(at.0, at.1, self.panel());
                // SAFETY: our own window; the matching release is in WM_LBUTTONUP.
                unsafe { SetCapture(hwnd) };
                (self.pressed, self.show_focus) = (control, false);
                if control.is_some_and(|c| c != Control::Close) {
                    self.focus = control;
                }
                if control != Some(Control::Hotkey) && control != Some(Control::ClearHotkey) {
                    self.stop_capture();
                }
                match control {
                    Some(Control::Delay) => {
                        self.dragging = true;
                        self.set_delay(settings_view::delay_at(at.0));
                    }
                    Some(Control::Hotkey) => self.start_capture(),
                    _ => {}
                }
            }
            WM_LBUTTONUP => {
                let pressed = self.pressed.take();
                self.dragging = false;
                // SAFETY: releases the capture taken on the press.
                unsafe {
                    let _ = ReleaseCapture();
                }
                if let Some(control) = pressed.filter(|&p| settings_view::hit(at.0, at.1, self.panel()) == Some(p)) {
                    self.activate(control);
                }
            }
            WM_CAPTURECHANGED => (self.pressed, self.dragging) = (None, false),
            WM_KEYDOWN | WM_SYSKEYDOWN if self.capturing.is_some() => self.capture_key(VIRTUAL_KEY(wparam.0 as u16)),
            WM_KEYUP | WM_SYSKEYUP if self.capturing.is_some() => {
                let modifiers = held();
                let bare = modifiers == Modifiers::default();
                self.capturing = Some(if bare { String::new() } else { hotkey::partial(modifiers) });
            }
            // Alt+letter would beep (no menu has that key) while the box listens.
            WM_CHAR | WM_SYSCHAR if self.capturing.is_some() => return Some(LRESULT(0)),
            WM_KEYDOWN => {
                if !self.navigate(VIRTUAL_KEY(wparam.0 as u16)) {
                    return None;
                }
            }
            WM_KILLFOCUS => {
                self.stop_capture();
                (self.pressed, self.dragging) = (None, false);
            }
            WM_CLOSE => self.hide(),
            WM_DPICHANGED => {
                self.factor = f32::from((wparam.0 >> 16) as u16) / BASE_DPI;
                // SAFETY: Windows passes the suggested new window rectangle.
                let suggested = unsafe { *(lparam.0 as *const RECT) };
                self.paint_at(Some((suggested.left, suggested.top)));
                return Some(LRESULT(0));
            }
            _ => return None,
        }
        if self.look() != before {
            self.paint();
        }
        Some(LRESULT(0))
    }
}
