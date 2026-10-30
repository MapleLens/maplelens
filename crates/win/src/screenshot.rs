//! `maplelens --screenshot OUT.png`: a picture of the game with MapleLens's cards, for the README and bug
//! reports. The overlay keeps itself out of screen captures, so the cards are drawn into the picture here,
//! exactly where the overlay would put them.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use maplelens_core::config;
use maplelens_core::geometry::{Point, Rect, card_scale, scaled};
use maplelens_core::image::{RgbImage, encode_png};
use maplelens_core::market::catalog::Catalog;
use maplelens_core::pipeline::hover::HoverDetector;
use maplelens_core::pipeline::info::InfoDetector;
use maplelens_core::pipeline::{FrameScreen, Screen};
use maplelens_core::placement::{place_beside_window, place_card};
use maplelens_core::ranking::RankCache;
use maplelens_core::render::RgbaImage;
use maplelens_core::render::card::{CardSpec, render_card};
use maplelens_core::render::rank_card::render_rank_card;
use maplelens_core::vision::reader::{NoOcr, TextReader, name_atlas, title_atlas};

use crate::capture::GdiScreen;
use crate::{app, game, net, paths};

/// Pixelate `area` (client coordinates): other players' names must not end up in published screenshots.
fn pixelate(image: &mut RgbImage, (x, y, w, h): (usize, usize, usize, usize)) {
    const BLOCK: usize = 8;
    for by in (y..(y + h).min(image.height)).step_by(BLOCK) {
        for bx in (x..(x + w).min(image.width)).step_by(BLOCK) {
            let block = image.crop(bx, by, (bx + BLOCK).min(x + w), (by + BLOCK).min(y + h));
            let n = (block.width * block.height).max(1);
            let mean: Vec<u8> = (0..3)
                .map(|c| (block.data.iter().skip(c).step_by(3).map(|&v| v as usize).sum::<usize>() / n) as u8)
                .collect();
            for py in by..(by + block.height) {
                for px in bx..(bx + block.width) {
                    image.set_pixel(px, py, [mean[0], mean[1], mean[2]]);
                }
            }
        }
    }
}

/// Paint a straight-alpha card over the picture with its top-left at (x, y).
fn composite(image: &mut RgbImage, card: &RgbaImage, x: i32, y: i32) {
    for cy in 0..card.height {
        for cx in 0..card.width {
            let (px, py) = (x + cx as i32, y + cy as i32);
            if px < 0 || py < 0 || px as usize >= image.width || py as usize >= image.height {
                continue;
            }
            let [r, g, b, a] = card.pixel(cx, cy);
            let under = image.pixel(px as usize, py as usize);
            let mix = |s: u8, d: u8| ((s as u32 * a as u32 + d as u32 * (255 - a as u32) + 127) / 255) as u8;
            image.set_pixel(px as usize, py as usize, [mix(r, under[0]), mix(g, under[1]), mix(b, under[2])]);
        }
    }
}

/// Capture the game client after `wait_s` seconds (time to bring the game to the front and hover an item; the
/// glance card has grown into the full one by then) and save it with the cards to `out`.
pub fn take(out: &Path, wait_s: f64, blur: &[(usize, usize, usize, usize)], catalog: Catalog) -> Result<(), String> {
    println!("Bring MapleStory to the front and hover an item: capturing in {wait_s:.0} s...");
    std::thread::sleep(Duration::from_secs_f64(wait_s));
    let hwnd = game::find_game_window().ok_or("MapleStory is not running")?;
    if !game::is_foreground(hwnd) {
        // The capture is of the screen: with another window in front, that window would be in the picture.
        return Err("MapleStory is not the window in front".into());
    }
    let bounds = game::client_rect(hwnd).ok_or("could not read the game window's size")?;
    let cursor = game::cursor().ok_or("could not read the mouse position")?;
    let mut picture = GdiScreen::new().grab(bounds);
    let fonts = app::system_fonts().ok_or("no usable font")?;
    let catalog = Arc::new(catalog);

    // Detection runs on the picture itself, with the mouse where it was.
    let origin = Point { x: bounds.x, y: bounds.y };
    let mut screen = FrameScreen { frame: &picture, origin, cursor: Some(cursor) };
    let mut hover = HoverDetector::new(catalog.clone(), TextReader::new(title_atlas(), Box::new(NoOcr)));
    let mut cards: Vec<(RgbaImage, i32, i32)> = Vec::new();
    if let Some(found) = hover.step(&mut screen, bounds, 0.0) {
        let spec = CardSpec {
            item: found.item.as_deref(),
            title: &found.title,
            generated_at: catalog.generated_at.as_deref(),
            now: paths::now_unix(),
            expanded: true,
            ui_scale: card_scale(bounds, found.scale),
            wide: found.compare,
            source: if catalog.generated_at.is_some() { "meso.watch" } else { "demo data" },
        };
        let card = render_card(&fonts, &spec);
        let mut tooltip_y = cursor.y + scaled(config::TOOLTIP_OFFSET.1, found.scale);
        if let Some(height) = found.pushed_up_height {
            tooltip_y = tooltip_y.min(bounds.bottom() - height);
        }
        let tooltip = Rect::new(
            cursor.x + scaled(config::TOOLTIP_OFFSET.0, found.scale),
            tooltip_y,
            found.tooltip_size.0,
            found.tooltip_size.1,
        );
        let (x, y) = place_card(
            tooltip,
            (card.width as i32, card.height as i32),
            scaled(config::CARD_GAP_PX, found.scale),
            bounds,
            found.compare,
        );
        println!("tooltip: {}", found.title);
        cards.push((card, x, y));
    }
    let mut info = InfoDetector::new(TextReader::new(name_atlas(), Box::new(NoOcr))).with_ui_scale(hover.ui_scale());
    info.step(&mut screen, bounds); // a name counts once two reads agree
    if let Some(window) = info.step(&mut screen, bounds)
        && let Some(name) = window.name.clone()
    {
        let mut ranks = RankCache::default();
        ranks.get(&name, 0.0);
        ranks.store(&name, net::fetch_rank(&name), 0.0);
        let card =
            render_rank_card(&fonts, &ranks.get(&name, 0.0), paths::now_unix(), card_scale(bounds, window.scale));
        let gap = scaled(config::INFO_CARD_GAP_PX, window.scale);
        if let Some((x, y)) = place_beside_window(window.rect, (card.width as i32, card.height as i32), gap, bounds) {
            println!("Character Info: {name}");
            cards.push((card, x, y));
        }
    }
    for (card, x, y) in &cards {
        composite(&mut picture, card, x - bounds.x, y - bounds.y);
    }
    for &area in blur {
        pixelate(&mut picture, area);
    }
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(out, encode_png(&picture)).map_err(|e| e.to_string())?;
    println!("saved {} ({}x{})", out.display(), picture.width, picture.height);
    Ok(())
}
