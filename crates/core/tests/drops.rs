//! The map drops panel: its content from a map and a drop table, its picture, and its place beside the EXP card.

mod common;

use common::fonts;
use maplelens_core::drops::{MapDrops, Rarity, Shown, assemble, sample};
use maplelens_core::geometry::Rect;
use maplelens_core::market::models::Item;
use maplelens_core::placement::{fraction_of, nearest_corner, place_at, place_beside_card, place_from_panel};
use maplelens_core::render::card::{CardSpec, Heading, render_card, render_named_card};
use maplelens_core::render::drops_card::{CARD_WIDTH, Hit, PanelState, colour, render_drops_card};
use maplelens_core::render::pointer::{TIP, render_pointer};
use maplelens_core::render::theme;
use maplelens_core::settings::Corner;

fn sample_drops() -> MapDrops {
    let prices = sample::prices();
    assemble(&sample::map(), &sample::table(), |id| prices.get(&id).copied()).expect("drops")
}

#[test]
fn the_sample_field_shows_every_monster_with_its_rarest_drop_first() {
    let drops = sample_drops();
    let names: Vec<&str> = drops.mobs.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["Orange Mushroom", "Slime", "Stump"]);
    let first: Vec<Rarity> = drops.mobs.iter().map(|m| m.drops[0].rarity).collect();
    assert_eq!(first, [Rarity::VeryRare, Rarity::Rare, Rarity::Legendary]);
    let best = drops.best().expect("priced drops");
    assert_eq!((best.name.as_str(), best.price), ("Scroll for Gloves for ATT 60%", Some(2_400_000.0)));
}

#[test]
fn the_panel_grows_with_the_monsters_and_the_ui_scale() {
    let drops = sample_drops();
    let card = render_drops_card(fonts(), &drops, &|_, _| None, PanelState::default(), 1.0);
    assert_eq!(card.width, (CARD_WIDTH * theme::CARD_SCALE).round() as usize);
    // Five monsters fill a row of tiles; a sixth starts another.
    let six = MapDrops { mobs: drops.mobs.iter().cycle().take(6).cloned().collect(), ..drops.clone() };
    let tall = render_drops_card(fonts(), &six, &|_, _| None, PanelState::default(), 1.0);
    assert!(tall.height > card.height);
    let small = render_drops_card(fonts(), &drops, &|_, _| None, PanelState::default(), 0.64);
    assert!(small.width < card.width && small.height < card.height);
    // As tall as the EXP card beside it: shorter than the content leaves monsters out, taller pads the foot.
    for height in [220, 300] {
        let fixed = PanelState { height: Some(height), ..PanelState::default() };
        let panel = render_drops_card(fonts(), &drops, &|_, _| None, fixed, 1.0);
        assert_eq!(panel.height, (height as f32 * theme::CARD_SCALE).round() as usize);
    }
    // A monster's page has every drop, in the same width.
    let page = PanelState { page: Some(0), ..PanelState::default() };
    let open = render_drops_card(fonts(), &drops, &|_, _| None, page, 1.0);
    assert_eq!(open.width, card.width);
    let fixed_page = PanelState { height: Some(200), ..page };
    let fixed = render_drops_card(fonts(), &drops, &|_, _| None, fixed_page, 1.0);
    assert_eq!(fixed.height, (200.0 * theme::CARD_SCALE).round() as usize);
    // The source's credit takes a line at the foot.
    let uncredited = MapDrops { credit: String::new(), ..drops };
    assert!(render_drops_card(fonts(), &uncredited, &|_, _| None, PanelState::default(), 1.0).height < card.height);
}

#[test]
fn a_long_list_ends_in_a_count_of_the_rest_inside_the_panel() {
    let mut drops = sample_drops();
    let many = (0..12).map(|i| Shown {
        item: i,
        name: format!("Item {i}"),
        chance: None,
        price: None,
        rarity: Rarity::Common,
    });
    drops.mobs[0].drops = many.collect();
    let card = render_drops_card(fonts(), &drops, &|_, _| None, PanelState::default(), 1.0);
    // Opaque everywhere the frame is: nothing was drawn past its edge, and the frame covers the whole picture.
    assert_eq!(card.alpha_bbox(), Some((0, 0, card.width, card.height)));
}

#[test]
fn the_panel_extends_the_exp_card_towards_the_middle_and_away_from_the_corner() {
    let client = Rect::new(100, 50, 1920, 1200);
    let panel = (300, 200);
    // The EXP card, 320 x 260, in each corner.
    let top_left = Rect::new(110, 60, 320, 260);
    assert_eq!(place_beside_card(top_left, panel, Corner::TopLeft, 6, client), Some((436, 60)));
    let top_right = Rect::new(1700, 60, 320, 260);
    assert_eq!(place_beside_card(top_right, panel, Corner::TopRight, 6, client), Some((1394, 60)));
    // At the bottom the bottoms are level, so a shorter panel does not reach down over the status bar.
    let bottom_left = Rect::new(110, 900, 320, 260);
    assert_eq!(place_beside_card(bottom_left, panel, Corner::BottomLeft, 6, client), Some((436, 960)));
    let bottom_right = Rect::new(1700, 900, 320, 260);
    assert_eq!(place_beside_card(bottom_right, panel, Corner::BottomRight, 6, client), Some((1394, 960)));
    // A panel taller than the room above a bottom card's foot does not fit: the card is never moved for it.
    assert_eq!(place_beside_card(Rect::new(110, 90, 320, 100), panel, Corner::BottomLeft, 6, client), None);
    // No room towards the middle: the other side.
    assert_eq!(place_beside_card(Rect::new(1800, 60, 210, 100), panel, Corner::TopLeft, 6, client), Some((1494, 60)));
    assert_eq!(place_beside_card(Rect::new(100, 50, 1920, 100), panel, Corner::TopLeft, 6, client), None);
}

#[test]
fn a_dragged_card_keeps_its_place_in_the_window_and_stays_inside_it() {
    let client = Rect::new(100, 50, 1920, 1200);
    let at = (100 + 480, 50 + 600);
    let fraction = fraction_of(client, at);
    assert_eq!(fraction, (0.25, 0.5));
    assert_eq!(place_at(client, (300, 200), fraction), Some(at));
    // A smaller window: the same share of it, pulled back inside where the card would stick out.
    let small = Rect::new(0, 0, 1366, 768);
    assert_eq!(place_at(small, (300, 200), (0.25, 0.5)), Some((342, 384)));
    assert_eq!(place_at(small, (300, 200), (0.95, 0.95)), Some((1066, 568)));
    assert_eq!(place_at(small, (2000, 200), (0.0, 0.0)), None);
    // A panel beside a dragged card grows away from the corner it is nearest.
    assert_eq!(nearest_corner(Rect::new(150, 900, 300, 200), client), Corner::BottomLeft);
    assert_eq!(nearest_corner(Rect::new(1500, 100, 300, 200), client), Corner::TopRight);
}

#[test]
fn the_pointer_is_drawn_at_the_cards_size() {
    let pointer = render_pointer(fonts(), false, 1.0);
    let pressed = render_pointer(fonts(), true, 1.0);
    assert_eq!((pointer.width, pointer.height), (pressed.width, pressed.height));
    assert_ne!(pointer, pressed);
    assert!(render_pointer(fonts(), false, 0.64).width < pointer.width);
    // The tip is the arrow's: drawn there.
    let (tx, ty) = (TIP.0 * theme::CARD_SCALE, TIP.1 * theme::CARD_SCALE);
    assert!(pointer.pixel(tx as usize, ty as usize)[3] > 0);
}

#[test]
fn a_hovered_slot_lights_up_and_its_price_card_opens_beside_the_panel() {
    let drops = sample_drops();
    let plain = render_drops_card(fonts(), &drops, &|_, _| None, PanelState::default(), 1.0);
    let lit = render_drops_card(
        fonts(),
        &drops,
        &|_, _| None,
        PanelState { hover: Some(Hit::Slot((0, 1))), ..PanelState::default() },
        1.0,
    );
    assert_eq!((lit.width, lit.height), (plain.width, plain.height));
    assert_ne!(lit, plain);
    let client = Rect::new(0, 0, 1920, 1200);
    let card = (450, 300);
    // Towards the middle of the client, level with the hovered row as far as the client lets it.
    let left_panel = Rect::new(400, 60, 336, 300);
    assert_eq!(place_from_panel(left_panel, 200, card, Corner::TopLeft, 6, client), Some((742, 200)));
    let right_panel = Rect::new(1200, 800, 336, 300);
    assert_eq!(place_from_panel(right_panel, 1000, card, Corner::BottomRight, 6, client), Some((744, 900)));
    // No room on that side: the other one.
    let edge = Rect::new(1500, 60, 336, 300);
    assert_eq!(place_from_panel(edge, 60, card, Corner::TopLeft, 6, client), Some((1044, 60)));
}

#[test]
fn the_named_card_is_the_price_card_under_the_item_s_name() {
    let mut item = Item::named("Scroll for Gloves for ATT 60%");
    item.price = Some(2_400_000.0);
    item.category = Some("Scrolls".into());
    let spec = CardSpec {
        item: Some(&item),
        title: &item.name,
        generated_at: None,
        now: 0.0,
        expanded: false,
        ui_scale: 1.0,
        wide: false,
        source: "demo data",
    };
    let heading = Heading {
        name: &item.name,
        note: "Very rare drop · 0.03% a kill · Orange Mushroom",
        colour: colour(Rarity::VeryRare),
        source: "",
    };
    let plain = render_card(fonts(), &spec);
    let named = render_named_card(fonts(), &spec, &heading);
    assert_eq!(named.width, plain.width);
    assert!(named.height > plain.height, "the note takes a line of its own");
    let credited = Heading { source: "NiaDB · meowdb.com/msclassic/monsters/9", ..heading };
    assert!(render_named_card(fonts(), &spec, &credited).height > named.height, "and the source one more");
    let bare = render_named_card(fonts(), &spec, &Heading { note: "", ..heading });
    assert_eq!(bare.height, plain.height);
    assert_ne!(bare, plain);
}
