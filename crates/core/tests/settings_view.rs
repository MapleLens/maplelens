//! The drawn settings window: every control can be reached by the pointer, the slider maps to the delay range,
//! and the picture follows the state it is given.

mod common;

use common::fonts;
use maplelens_core::notify::pairing::Pairing;
use maplelens_core::render::settings_view::{Control, SIZE, View, delay_at, delay_step, hit, is_title, render};
use maplelens_core::settings::{EXPAND_DELAY_RANGE_S, Settings};

const KEY: &str = "AbCdEfGhIjKlMnOp.qRsTuVwXyZ0123456789-_ab";

fn view() -> View<'static> {
    View { version: "0.2.0", update_label: "Update now", can_check: true, ..View::default() }
}

#[test]
fn every_control_sits_inside_the_window_and_answers_at_its_centre() {
    for panel in [false, true] {
        for &control in [Control::Close].iter().chain(Control::order(panel)) {
            let (x, y, w, h) = control.area();
            assert!(x >= 0.0 && y >= 0.0 && x + w <= SIZE.0 && y + h <= SIZE.1, "{control:?} is off the window");
            assert_eq!(hit(x + w / 2.0, y + h / 2.0, panel), Some(control));
        }
    }
    // Behind the pairing panel nothing answers.
    let (x, y, w, h) = Control::Overlay.area();
    assert_eq!(hit(x + w / 2.0, y + h / 2.0, true), None);
    assert!(is_title(40.0, 10.0));
    let (cx, cy, _, _) = Control::Close.area();
    assert!(!is_title(cx + 2.0, cy + 2.0), "the close box is not a drag handle");
    assert_eq!(hit(4.0, SIZE.1 - 4.0, false), None);
}

#[test]
fn the_slider_spans_the_delay_range_in_tenths() {
    let (x, _, w, _) = Control::Delay.area();
    assert_eq!(delay_at(x - 50.0), 0.0);
    assert_eq!(delay_at(x + w + 50.0), EXPAND_DELAY_RANGE_S.1);
    let middle = delay_at(x + w / 2.0);
    assert!((middle - 1.5).abs() < 0.11, "{middle}");
    assert_eq!((middle * 10.0).round() / 10.0, middle);
    assert_eq!(delay_step(1.2, true), 1.3);
    assert_eq!(delay_step(0.0, false), 0.0);
    assert_eq!(delay_step(EXPAND_DELAY_RANGE_S.1, true), EXPAND_DELAY_RANGE_S.1);
}

#[test]
fn the_picture_is_the_window_size_at_any_dpi_and_opaque_inside() {
    for factor in [1.0, 1.5, 2.0] {
        let image = render(fonts(), &view(), factor);
        assert_eq!((image.width, image.height), ((SIZE.0 * factor) as usize, (SIZE.1 * factor) as usize));
        let centre = image.pixel(image.width / 2, image.height / 2);
        assert_eq!(centre[3], 255, "the window body hides what is behind it");
    }
}

#[test]
fn the_picture_follows_the_state() {
    let mut code = Pairing::default();
    code.show_code(KEY, 0.0);
    let base = render(fonts(), &view(), 1.0);
    let changed = [
        View { settings: Settings { overlay_enabled: false, ..Settings::default() }, ..view() },
        View { settings: Settings { expand_delay_s: 0.0, ..Settings::default() }, ..view() },
        View { capturing: Some(""), ..view() },
        View { focus: Some(Control::Hotkey), ..view() },
        View { hover: Some(Control::Check), ..view() },
        View { can_update: true, ..view() },
        View { update_text: "Version 9.9.9 is available.", ..view() },
        View { phone: code.clone(), ..view() },
        View { phone: code.clone(), phone_left_s: Some(61), ..view() },
        View { settings: Settings { phone_key: KEY.into(), ..Settings::default() }, ..view() },
    ];
    for (i, other) in changed.iter().enumerate() {
        assert_ne!(render(fonts(), other, 1.0), base, "change {i} drew nothing new");
    }
    // A disabled button does not react to the pointer.
    let pressed = View { pressed: Some(Control::Update), ..view() };
    assert_eq!(render(fonts(), &pressed, 1.0), base);
}

#[test]
fn the_pairing_panel_shows_the_code_and_hides_it_with_the_panel() {
    let paired = Settings { phone_key: KEY.into(), ..Settings::default() };
    let mut phone = Pairing::default();
    phone.show_code(KEY, 0.0);
    let up = render(fonts(), &View { settings: paired.clone(), phone: phone.clone(), ..view() }, 1.0);
    // The code's navy dots are drawn in the middle of the panel.
    let (cx, cy) = (up.width / 2, 260);
    let dark = (cx - 100..cx + 100).filter(|&x| up.pixel(x, cy)[..3] == [44, 50, 71]).count();
    assert!(dark > 20, "{dark} navy pixels across the code");
    phone.hide_panel();
    let down = render(fonts(), &View { settings: paired, phone, ..view() }, 1.0);
    let dark = (cx - 100..cx + 100).filter(|&x| down.pixel(x, cy)[..3] == [44, 50, 71]).count();
    assert_eq!(dark, 0, "no code once the panel is closed");
}
