//! Where an item comes from and what NPCs or crafters make of it (a details-card section).

use super::canvas::{Anchor, Canvas, Ink};
use super::glance::PAD;
use super::theme;
use crate::market::format::format_mesos;
use crate::market::insights;
use crate::market::models::{Craft, Item};

/// An NPC price this small next to the market (scrolls an NPC buys for 1 meso) says nothing useful.
const NPC_FLOOR_NOTABLE: f64 = 0.02;
const INDENT: f32 = 10.0;
const LINE: f32 = 16.0;

fn obtain_name(source: &str) -> &str {
    match source {
        "mob" => "monster drop",
        "npc" => "NPC shop",
        "craft" => "crafting",
        "quest" => "quest reward",
        other => other,
    }
}

pub fn sources(cv: &mut Canvas<'_>, mut y: f32, item: &Item) -> f32 {
    let share = insights::npc_floor_share(item);
    if let Some(npc) = &item.npc {
        if let Some(sell) = npc.sell.filter(|&s| s != 0.0)
            && share.is_none_or(|s| s >= NPC_FLOOR_NOTABLE)
        {
            let extra =
                share.filter(|&s| s != 0.0).map_or(String::new(), |s| format!(" ({:.0}% of market)", s * 100.0));
            y = bullet(cv, y, &format!("NPC buys it for {}{extra}", format_mesos(Some(sell))));
        }
        if let Some(buy) = npc.buy.filter(|&b| b != 0.0) {
            let place = npc.shop.as_deref().map_or(String::new(), |shop| format!(" at {shop}"));
            y = bullet(cv, y, &format!("NPC sells it for {}{place}", format_mesos(Some(buy))));
        }
    }
    if let Some(craft) = &item.craft {
        y = craft_line(cv, y, item, craft);
    }
    if !item.obtain.is_empty() {
        let from: Vec<&str> = item.obtain.iter().map(|o| obtain_name(o)).collect();
        y = bullet(cv, y, &format!("From: {}", from.join(", ")));
    }
    y
}

/// Craft cost on one line with a coloured verdict at its end, the recipe dimmer underneath.
fn craft_line(cv: &mut Canvas<'_>, mut y: f32, item: &Item, craft: &Craft) -> f32 {
    let level = craft.level.filter(|&l| l != 0).map_or(String::new(), |l| format!(" Lv.{l}"));
    let mut head = format!("Craft ({}{level})", craft.discipline);
    if let Some(per_item) = craft.per_item.filter(|&p| p != 0.0) {
        head += &format!(": {} each", format_mesos(Some(per_item)));
    }
    marker(cv, y);
    let width = cv.text(PAD + INDENT, y, &head, Ink::new(11.0, theme::TEXT_DIM), Anchor::LA);
    if let Some(margin) = insights::craft_margin(item, Some(craft)) {
        // Selling a crafted one at the typical price.
        let verdict = format!("{:.0}% {}", margin.abs() * 100.0, if margin >= 0.0 { "profit" } else { "loss" });
        let colour = if margin >= 0.0 { theme::UP } else { theme::DOWN };
        cv.text(PAD + INDENT + width + 6.0, y, &verdict, Ink::new(11.0, colour).bold(), Anchor::LA);
    }
    let recipe: Vec<String> = craft.parts.iter().take(3).map(|p| format!("{}× {}", p.count, p.name)).collect();
    y += LINE;
    for line in cv.wrap(&recipe.join(", "), 10.0, cv.width - 2.0 * PAD - INDENT, false) {
        cv.text(PAD + INDENT, y, &line, Ink::new(10.0, theme::TEXT_FAINT), Anchor::LA);
        y += LINE - 2.0;
    }
    y + 2.0
}

fn bullet(cv: &mut Canvas<'_>, mut y: f32, text: &str) -> f32 {
    marker(cv, y);
    for line in cv.wrap(text, 11.0, cv.width - 2.0 * PAD - INDENT - 2.0, false) {
        cv.text(PAD + INDENT, y, &line, Ink::new(11.0, theme::TEXT_DIM), Anchor::LA);
        y += LINE;
    }
    y + 2.0
}

fn marker(cv: &mut Canvas<'_>, y: f32) {
    cv.rect(PAD, y + 6.0, 4.0, 4.0, theme::GOLD);
}
