//! The store status pill: how each status looks, where it goes, and that it scales with the game's UI.

mod common;

use common::fonts;
use maplelens_core::geometry::Rect;
use maplelens_core::render::RgbaImage;
use maplelens_core::render::status_pill::{place_status, render_status, text_for};
use maplelens_core::shop::ShopStatus;

const VISIBLE: [ShopStatus; 5] =
    [ShopStatus::Watching, ShopStatus::NeedSoldWindow, ShopStatus::Scrolled, ShopStatus::Covered, ShopStatus::NotOpen];

fn pill(status: ShopStatus) -> RgbaImage {
    render_status(fonts(), status, 1.0).expect("a visible status has a pill")
}

#[test]
fn every_visible_status_has_a_pill_and_hidden_has_none() {
    for status in VISIBLE {
        let pill = pill(status);
        assert!(
            pill.width > 120 && pill.width < 360 && pill.height == 24,
            "{status:?}: {}x{}",
            pill.width,
            pill.height
        );
        assert!(text_for(status).is_some());
        assert!(pill.alpha_bbox().is_some(), "{status:?}: something was drawn");
    }
    assert!(render_status(fonts(), ShopStatus::Hidden, 1.0).is_none());
    assert!(text_for(ShopStatus::Hidden).is_none());
}

#[test]
fn detected_is_green_and_warnings_are_amber() {
    let tint = |status| {
        let pill = pill(status);
        let strong: Vec<[u8; 4]> = pill
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| [p[0], p[1], p[2], p[3]])
            .filter(|p| p[3] > 250 && p[..3].iter().max().unwrap() - p[..3].iter().min().unwrap() > 60)
            .collect();
        let mean = |c: usize| strong.iter().map(|p| p[c] as f32).sum::<f32>() / strong.len().max(1) as f32;
        (mean(0), mean(1), mean(2))
    };
    let (green, amber) = (tint(ShopStatus::Watching), tint(ShopStatus::NeedSoldWindow));
    assert!(green.1 > green.0 && green.1 > green.2, "{green:?}");
    assert!(amber.0 > amber.2 && amber.0 > 200.0, "{amber:?}");
}

#[test]
fn pill_scales_with_the_game_ui() {
    let (one, big) = (pill(ShopStatus::Watching), render_status(fonts(), ShopStatus::Watching, 1.2).unwrap());
    assert_eq!(big.height, (one.height as f32 * 1.2).round() as usize);
    assert!((big.width as f32 - one.width as f32 * 1.2).abs() <= 1.0);
}

#[test]
fn pill_sits_at_the_bottom_left_of_the_game() {
    let client = Rect::new(784, 105, 1920, 1200);
    let (x, y) = place_status(client, (200, 24), 1.0);
    assert!(x > client.x && x - client.x < 30);
    let bottom = client.y + client.h;
    assert!(y + 24 < bottom && bottom - (y + 24) < 20);
}
