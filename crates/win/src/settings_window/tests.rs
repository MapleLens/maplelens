//! The drawn settings window driven by window messages (as Windows would send them; no real input is made):
//! every control reports what the app needs.

use maplelens_core::render::settings_view::Control;
use maplelens_core::settings::{Corner, EXPAND_DELAY_RANGE_S, Settings};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_BACK, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_SPACE, VK_TAB, VK_UP};
use windows::Win32::UI::WindowsAndMessaging::{
    IsWindowVisible, SendMessageW, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
};

use maplelens_core::notify::pairing::{Pairing, Reply};

use super::{PhoneAction, SettingsEvent, SettingsWindow};
use crate::icon;
use crate::notify::Service;

fn send(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) {
    // SAFETY: a message to our own window, handled on this thread.
    unsafe { SendMessageW(hwnd, msg, Some(WPARAM(wparam)), Some(LPARAM(lparam))) };
}

/// Client pixel of design point (x, y), packed as mouse messages carry it.
fn point(hwnd: HWND, (x, y): (f32, f32)) -> isize {
    // SAFETY: plain query on our own window.
    let factor = unsafe { GetDpiForWindow(hwnd) } as f32 / 96.0;
    let (px, py) = ((x * factor).round() as isize, (y * factor).round() as isize);
    (py << 16) | px
}

fn click_at(hwnd: HWND, at: (f32, f32)) {
    let at = point(hwnd, at);
    send(hwnd, WM_MOUSEMOVE, 0, at);
    send(hwnd, WM_LBUTTONDOWN, 1, at);
    send(hwnd, WM_LBUTTONUP, 0, at);
}

fn click(hwnd: HWND, control: Control) {
    let (x, y, w, h) = control.area();
    click_at(hwnd, (x + w / 2.0, y + h / 2.0));
}

fn key(hwnd: HWND, vk: u16) {
    send(hwnd, WM_KEYDOWN, usize::from(vk), 0);
}

fn opened() -> SettingsWindow {
    let window = SettingsWindow::new(icon::load(None), true);
    window.show(Settings::default()).unwrap();
    assert!(window.take_events().is_empty(), "opening is not an edit");
    window
}

#[test]
fn check_boxes_flip_their_setting() {
    let window = opened();
    let hwnd = window.hwnd();
    let defaults = Settings::default();
    for (control, expected) in [
        (Control::Overlay, Settings { overlay_enabled: !defaults.overlay_enabled, ..defaults.clone() }),
        (Control::StatusBar, Settings { store_status_bar: !defaults.store_status_bar, ..defaults.clone() }),
        (Control::Startup, Settings { launch_at_startup: !defaults.launch_at_startup, ..defaults.clone() }),
        (Control::AutoUpdate, Settings { check_updates: !defaults.check_updates, ..defaults.clone() }),
        (Control::NotifySales, Settings { notify_sales: !defaults.notify_sales, ..defaults.clone() }),
        (Control::NotifyStore, Settings { notify_store: !defaults.notify_store, ..defaults.clone() }),
        (Control::TrainingExp, Settings { training_card: !defaults.training_card, ..defaults.clone() }),
    ] {
        window.sync(defaults.clone());
        click(hwnd, control);
        assert_eq!(window.take_events(), [SettingsEvent::Changed(expected)], "{control:?}");
    }
}

#[test]
fn a_corner_is_picked_by_a_click_or_the_arrows() {
    let window = opened();
    let hwnd = window.hwnd();
    let at = |corner| Settings { training_corner: corner, ..Settings::default() };
    for corner in Corner::ALL {
        window.sync(Settings::default());
        click(hwnd, Control::Corner(corner));
        let expected = if corner == Settings::default().training_corner {
            vec![]
        } else {
            vec![SettingsEvent::Changed(at(corner))]
        };
        assert_eq!(window.take_events(), expected, "{corner:?}");
    }
    // From the default bottom right: left, up, then down again, each picking the corner it lands on.
    window.sync(Settings::default());
    click(hwnd, Control::Corner(Corner::BottomRight));
    for (vk, corner) in [(VK_LEFT, Corner::BottomLeft), (VK_UP, Corner::TopLeft), (VK_DOWN, Corner::BottomLeft)] {
        key(hwnd, vk.0);
        assert_eq!(window.take_events(), [SettingsEvent::Changed(at(corner))], "{corner:?}");
    }
}

#[test]
fn the_webhook_buttons_do_nothing_without_a_webhook_and_clear_one() {
    let window = opened();
    let hwnd = window.hwnd();
    window.sync(Settings::default());
    for control in [Control::TestWebhook, Control::ClearWebhook] {
        click(hwnd, control);
    }
    assert_eq!(window.take_events(), []);
    let with = Settings { discord_webhook: "https://discord.com/api/webhooks/12/abc".into(), ..Settings::default() };
    window.sync(with.clone());
    click(hwnd, Control::TestWebhook);
    assert_eq!(window.take_events(), [SettingsEvent::TestNotify(Service::Discord, with.notify())]);
    click(hwnd, Control::ClearWebhook);
    assert_eq!(window.take_events(), [SettingsEvent::Changed(Settings::default())]);
}

#[test]
fn the_telegram_buttons_do_nothing_without_a_bot_and_clear_one_with_its_chat() {
    let window = opened();
    let hwnd = window.hwnd();
    window.sync(Settings::default());
    for control in [Control::TestTelegram, Control::ClearTelegram] {
        click(hwnd, control);
    }
    assert_eq!(window.take_events(), []);
    let with = Settings {
        telegram_webhook:
            "https://api.telegram.org/bot123456789:AAFakeTokenForTests_abcdefghijklmnop-Q/sendMessage?chat_id=111"
                .into(),
        ..Settings::default()
    };
    window.sync(with.clone());
    click(hwnd, Control::TestTelegram);
    assert_eq!(window.take_events(), [SettingsEvent::TestNotify(Service::Telegram, with.notify())]);
    click(hwnd, Control::ClearTelegram);
    assert_eq!(window.take_events(), [SettingsEvent::Changed(Settings::default())]);
}

#[test]
fn the_slider_sets_the_delay_where_it_is_pressed() {
    let window = opened();
    let (x, y, w, h) = Control::Delay.area();
    click_at(window.hwnd(), (x, y + h / 2.0));
    let low = Settings { expand_delay_s: 0.0, ..Settings::default() };
    assert_eq!(window.take_events(), [SettingsEvent::Changed(low)]);
    click_at(window.hwnd(), (x + w - 1.0, y + h / 2.0));
    let high = Settings { expand_delay_s: EXPAND_DELAY_RANGE_S.1, ..Settings::default() };
    assert_eq!(window.take_events(), [SettingsEvent::Changed(high)]);
}

#[test]
fn the_hotkey_box_listens_until_a_combo_escape_or_backspace() {
    let window = opened();
    let hwnd = window.hwnd();
    click(hwnd, Control::Hotkey);
    assert_eq!(window.take_events(), [SettingsEvent::Capturing(true)]);
    key(hwnd, u16::from(b'K'));
    assert!(window.take_events().is_empty(), "a bare key would fire while typing in the game");
    key(hwnd, VK_ESCAPE.0);
    assert_eq!(window.take_events(), [SettingsEvent::Capturing(false)]);
    // SAFETY: plain query.
    assert!(unsafe { IsWindowVisible(hwnd) }.as_bool(), "Esc in the box gives up listening, not the window");

    click(hwnd, Control::Hotkey);
    key(hwnd, VK_BACK.0);
    let none = Settings { toggle_hotkey: 0, ..Settings::default() };
    let events = window.take_events();
    assert_eq!(events, [SettingsEvent::Capturing(true), SettingsEvent::Capturing(false), SettingsEvent::Changed(none)]);
}

#[test]
fn clear_removes_the_hotkey_and_buttons_report_only_when_enabled() {
    let window = opened();
    let hwnd = window.hwnd();
    click(hwnd, Control::ClearHotkey);
    assert_eq!(window.take_events(), [SettingsEvent::Changed(Settings { toggle_hotkey: 0, ..Settings::default() })]);
    click(hwnd, Control::Check);
    assert_eq!(window.take_events(), [SettingsEvent::CheckUpdates]);
    click(hwnd, Control::Update);
    assert!(window.take_events().is_empty(), "nothing to install yet");
}

#[test]
fn tab_and_space_work_the_controls() {
    let window = opened();
    let hwnd = window.hwnd();
    key(hwnd, VK_TAB.0);
    key(hwnd, VK_SPACE.0);
    let overlay_off = Settings { overlay_enabled: false, ..Settings::default() };
    assert_eq!(window.take_events(), [SettingsEvent::Changed(overlay_off)]);
    key(hwnd, VK_ESCAPE.0);
    // SAFETY: plain query.
    assert!(!unsafe { IsWindowVisible(hwnd) }.as_bool(), "Esc closes the window");
}

#[test]
fn the_close_box_hides_the_window() {
    let window = opened();
    click(window.hwnd(), Control::Close);
    // SAFETY: plain query.
    assert!(!unsafe { IsWindowVisible(window.hwnd()) }.as_bool());
}

const PHONE_KEY: &str = "AbCdEfGhIjKlMnOp.qRsTuVwXyZ0123456789-_ab";

#[test]
fn the_phone_row_asks_the_app_and_test_and_remove_need_a_key() {
    let window = opened();
    let hwnd = window.hwnd();
    for control in [Control::TestPhone, Control::RemovePhone] {
        click(hwnd, control);
    }
    assert_eq!(window.take_events(), [], "no key: nothing to test or remove");
    click(hwnd, Control::ConnectPhone);
    assert_eq!(window.take_events(), [SettingsEvent::Phone(PhoneAction::ShowCode)]);
    window.sync(Settings { phone_key: PHONE_KEY.into(), ..Settings::default() });
    let mut paired = Pairing::default();
    paired.on_ping(Reply { status: Some(200), phones: Some(1) });
    window.set_phone(&paired, None);
    click(hwnd, Control::ConnectPhone);
    assert_eq!(window.take_events(), [], "one phone per pairing");
    click(hwnd, Control::TestPhone);
    click(hwnd, Control::RemovePhone);
    let asked = [SettingsEvent::Phone(PhoneAction::Test), SettingsEvent::Phone(PhoneAction::Remove)];
    assert_eq!(window.take_events(), asked);
    paired.removing();
    window.set_phone(&paired, None);
    click(hwnd, Control::RemovePhone);
    assert_eq!(window.take_events(), [], "one removal at a time");
}

#[test]
fn the_pairing_panel_covers_the_window_until_it_is_closed() {
    let window = opened();
    let hwnd = window.hwnd();
    window.sync(Settings { phone_key: PHONE_KEY.into(), ..Settings::default() });
    let mut pairing = Pairing::default();
    pairing.show_code(PHONE_KEY, 0.0);
    window.set_phone(&pairing, Some(120));
    click(hwnd, Control::Overlay);
    assert_eq!(window.take_events(), [], "the controls behind the panel can't be reached");
    click(hwnd, Control::PanelMain);
    assert_eq!(window.take_events(), [SettingsEvent::Phone(PhoneAction::Scanned)]);
    key(hwnd, VK_ESCAPE.0);
    assert_eq!(window.take_events(), [SettingsEvent::Phone(PhoneAction::HidePanel)]);
    // SAFETY: plain query.
    assert!(unsafe { IsWindowVisible(hwnd) }.as_bool(), "Esc closes the panel first");
    pairing.tick(120.0);
    window.set_phone(&pairing, None);
    click(hwnd, Control::PanelMain);
    assert_eq!(window.take_events(), [SettingsEvent::Phone(PhoneAction::ShowCode)], "Show it again");
    click(hwnd, Control::Close);
    assert_eq!(window.take_events(), [SettingsEvent::Phone(PhoneAction::HidePanel)], "the code never stays up unseen");
}
