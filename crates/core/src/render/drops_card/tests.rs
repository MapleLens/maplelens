use super::*;
use crate::drops::{assemble, sample};

fn sample_drops() -> MapDrops {
    let prices = sample::prices();
    assemble(&sample::map(), &sample::table(), |id| prices.get(&id).copied()).expect("drops")
}

#[test]
fn scrolls_are_named_as_players_write_them() {
    assert_eq!(short_name("Scroll for Gloves for ATT 60%"), "Gloves ATT 60%");
    assert_eq!(short_name("Scroll for One-Handed Sword for ATT 10%"), "One-Handed Sword ATT 10%");
    assert_eq!(short_name("Orange Mushroom Cap"), "Orange Mushroom Cap");
}

#[test]
fn rarities_have_distinct_colours() {
    let all = [Rarity::Common, Rarity::Uncommon, Rarity::Rare, Rarity::VeryRare, Rarity::Legendary].map(colour);
    for (i, a) in all.iter().enumerate() {
        assert!(all[i + 1..].iter().all(|b| b != a));
    }
}

#[test]
fn the_list_has_every_monster_as_a_tile_and_the_map_s_rare_drops_once_each() {
    let drops = sample_drops();
    let list = PanelState::default();
    let at = |(x, y): (f32, f32)| hit(&drops, list, x, y);
    for mob in 0..3 {
        assert_eq!(at(list::tile_centre(mob)), Some(Hit::Mob(mob)));
    }
    // Rarest first, then dearest; the list shows the dearest: the gloves (2.4m), the earring (310k), the shoes...
    assert_eq!(drops.rare(), [(2, 0), (0, 0), (1, 0), (0, 1), (0, 2), (1, 1), (2, 1)]);
    let top = drops.top(10);
    assert_eq!(top, [(0, 0), (2, 0), (1, 0), (0, 1), (0, 2), (1, 1), (2, 1)]);
    assert_eq!(drops.top(2), [(0, 0), (2, 0)]);
    for (k, &slot) in top.iter().enumerate() {
        let (x, y) = list::slot_centre(&drops, k);
        assert_eq!(at((x, y)), Some(Hit::Slot(slot)));
        assert!((slot_top(&drops, list, slot) - y).abs() < 20.0);
    }
    assert_eq!(at(list::slot_centre(&drops, top.len())), None, "past the last drop");
    assert_eq!(at((PAD + 1.0, 10.0)), None, "the header");
    assert_eq!(drops.droppers(4_000_001), ["Orange Mushroom"]);
}

#[test]
fn a_short_panel_still_has_a_row_of_drops() {
    let drops = sample_drops();
    let short = PanelState { height: Some(100), ..PanelState::default() };
    let (x, y) = list::slot_centre(&drops, 0);
    assert_eq!(hit(&drops, short, x, y), Some(Hit::Slot((0, 0))));
}

#[test]
fn on_a_monster_s_page_every_drop_is_a_slot_and_back_is_a_button() {
    let drops = sample_drops();
    let page = PanelState { page: Some(0), ..PanelState::default() };
    let at = |(x, y): (f32, f32)| hit(&drops, page, x, y);
    assert_eq!(at(page::BACK_CENTRE), Some(Hit::Back));
    for drop in 0..drops.mobs[0].drops.len() {
        assert_eq!(at(page::slot_centre(drop)), Some(Hit::Slot((0, drop))));
    }
    assert_eq!(at(page::slot_centre(5)), None, "the mushroom has five drops");
    assert_eq!(slot_top(&drops, page, (0, 1)), page::slot_centre(1).1 - SLOT / 2.0);
    // A page for a monster that is no longer there is the list.
    let gone = PanelState { page: Some(9), ..PanelState::default() };
    assert_eq!(hit(&drops, gone, page::BACK_CENTRE.0, page::BACK_CENTRE.1), None);
}
