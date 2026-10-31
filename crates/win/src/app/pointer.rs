//! The pointer over MapleLens's own cards, and what a press there does. The cards stay click-through (rule 3) and
//! nothing hooks the mouse (rule 1): the left button's state is read each tick (`game::left_button_down`), so the game
//! gets every press too, which on a field does nothing. The EXP strip's bag button opens the drops panel and its X
//! closes it; a press on a monster opens its page in the panel and Back closes it; a press on the EXP card or the
//! drops panel that moves drags it, and where it is let go is kept in
//! the settings as a share of the game window. The game draws its cursor into its own frame, under the cards, so over
//! them this draws the game's own (maplestory.io's frames of it, downloaded once into cache\cursor; game artwork, never
//! committed), a drawn arrow until they are on disk.

use std::sync::{Arc, Mutex};

use maplelens_core::geometry::{Point, Rect};
use maplelens_core::image::decode_png_rgba;
use maplelens_core::placement::fraction_of;
use maplelens_core::render::RgbaImage;
use maplelens_core::render::drops_card::Hit;
use maplelens_core::render::pointer::{ART_PER_UI_SCALE, TIP, parse_origin, render_game_cursor, render_pointer};
use maplelens_core::render::theme;
use maplelens_core::settings::Settings;
use windows::Win32::Foundation::HWND;

use super::App;
use crate::layered::LayeredWindow;
use crate::maps::cache::cache_dir;
use crate::{game, paths, potions};

/// A press that moved this far (at UI scale 1) is a drag, not a click.
const DRAG_START: f32 = 4.0;
/// The game's cursor frames: the glove, and the glove pressed.
const CURSOR_FRAMES: [u32; 2] = [0, 12];
const CURSOR_DATA: &str = "https://maplestory.io/api/wz/GMS/40/UI/Basic.img/Cursor";
const CURSOR_IMAGE: &str = "https://maplestory.io/api/wz/img/GMS/40/UI/Basic.img/Cursor";

/// A cursor frame and its hot spot.
type Cursor = (Arc<RgbaImage>, (f32, f32));

/// A card the player can drag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Card {
    Training,
    Drops,
}

struct Press {
    card: Card,
    /// What was under the pointer in the drops panel: a click counts when it is let go over the same.
    hit: Option<Hit>,
    /// Pressed on the EXP strip's drops button.
    button: bool,
    at: Point,
    /// The card's top-left when pressed.
    origin: (i32, i32),
    dragging: bool,
}

pub struct PointerUi {
    window: LayeredWindow,
    /// The pointer as drawn: (pressed, UI scale bits, the game's own).
    drawn: Option<(bool, u32, bool)>,
    down: bool,
    press: Option<Press>,
    /// The card being dragged, and where its top-left is now.
    dragged: Option<(Card, (i32, i32))>,
    /// The game's cursor, normal and pressed, once on disk (a thread fetches them the first time).
    cursors: Arc<Mutex<Option<[Cursor; 2]>>>,
}

impl PointerUi {
    pub fn new() -> windows::core::Result<Self> {
        let cursors = Arc::new(Mutex::new(None));
        let loading = cursors.clone();
        let started = std::thread::Builder::new().name("game-cursor".into()).spawn(move || {
            let frames = load_cursors();
            *loading.lock().unwrap_or_else(|e| e.into_inner()) = frames;
        });
        if let Err(error) = started {
            crate::warn!("could not start the cursor download: {error}");
        }
        Ok(Self { window: LayeredWindow::new()?, drawn: None, down: false, press: None, dragged: None, cursors })
    }

    /// Where `card` is being dragged to, while it is.
    pub fn dragged(&self, card: Card) -> Option<(i32, i32)> {
        self.dragged.filter(|(c, _)| *c == card).map(|(_, at)| at)
    }

    pub fn hide(&mut self) {
        self.window.hide();
        self.press = None;
        self.dragged = None;
    }
}

impl App {
    /// Runs every tick before the cards are placed: presses and drags first, so the cards follow at once.
    pub(super) fn follow_pointer(&mut self) {
        let hwnd = self.training.as_ref().map(|(hwnd, _)| HWND(*hwnd as *mut _));
        let in_front = hwnd.filter(|&h| self.settings.overlay_enabled && game::is_alive(h) && game::is_foreground(h));
        let (Some(client), Some(cursor)) = (in_front.and_then(game::client_rect), game::cursor()) else {
            return self.pointer.hide();
        };
        // The game's cursor at the game's size; the cards' button at theirs.
        let (scale, cards) = ((self.ui.of(client) * 100.0).round() / 100.0, self.card_scale(client));
        let down = game::left_button_down();
        let was_down = std::mem::replace(&mut self.pointer.down, down);
        let training = self.training_window.rect().filter(|_| self.training_window.visible());
        let panel = self.drops.panel_rect();
        match (was_down, down) {
            (false, true) => {
                let card = match (panel, training) {
                    (Some(rect), _) if rect.contains(cursor) => Some((Card::Drops, rect)),
                    (_, Some(rect)) if rect.contains(cursor) => Some((Card::Training, rect)),
                    _ => None,
                };
                self.pointer.press = card.map(|(card, rect)| Press {
                    card,
                    hit: (card == Card::Drops).then(|| self.drops.hit_at(cursor)).flatten(),
                    button: card == Card::Training && self.over_drops_button(cursor, cards),
                    at: cursor,
                    origin: (rect.x, rect.y),
                    dragging: false,
                });
            }
            (true, true) => {
                if let Some(press) = &mut self.pointer.press {
                    let (dx, dy) = (cursor.x - press.at.x, cursor.y - press.at.y);
                    press.dragging |= dx.abs().max(dy.abs()) as f32 > DRAG_START * scale;
                    if press.dragging {
                        self.pointer.dragged = Some((press.card, (press.origin.0 + dx, press.origin.1 + dy)));
                    }
                }
            }
            (true, false) => self.let_go(cursor, client, cards),
            (false, false) => {}
        }
        let over = [training, panel, self.drops.price_rect()].into_iter().flatten().any(|r| r.contains(cursor));
        if over || self.pointer.dragged.is_some() {
            self.draw_pointer(cursor, down, scale);
        } else {
            self.pointer.window.hide();
        }
    }

    /// The press ends: a drag keeps where the card was let go; a press let go where it began clicks it. `cards` is the
    /// scale the cards are drawn at.
    fn let_go(&mut self, cursor: Point, client: Rect, cards: f32) {
        let Some(press) = self.pointer.press.take() else { return };
        if let Some((card, at)) = self.pointer.dragged.take() {
            let at = Some(fraction_of(client, at));
            let settings = match card {
                Card::Training => Settings { training_at: at, ..self.settings.clone() },
                Card::Drops => Settings { drops_at: at, ..self.settings.clone() },
            };
            return self.update_settings(settings);
        }
        if press.button && self.over_drops_button(cursor, cards) {
            return self.drops.toggle();
        }
        if press.card != Card::Drops || self.drops.hit_at(cursor) != press.hit {
            return;
        }
        match press.hit {
            Some(Hit::Mob(mob)) => {
                let now = self.now();
                self.drops.open_page(mob, now);
            }
            Some(Hit::Back) => self.drops.close_page(),
            Some(Hit::Close) => self.drops.close(),
            _ => {}
        }
    }

    fn draw_pointer(&mut self, cursor: Point, pressed: bool, scale: f32) {
        let cursors = self.pointer.cursors.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let frame = cursors.map(|frames| frames[usize::from(pressed)].clone());
        let factor = theme::CARD_SCALE * scale;
        let tip = frame.as_ref().map_or((TIP.0 * factor, TIP.1 * factor), |(_, (x, y))| {
            (x * scale * ART_PER_UI_SCALE, y * scale * ART_PER_UI_SCALE)
        });
        let (x, y) = (cursor.x - tip.0.round() as i32, cursor.y - tip.1.round() as i32);
        let wanted = (pressed, scale.to_bits(), frame.is_some());
        if self.pointer.drawn != Some(wanted) || !self.pointer.window.visible() {
            let picture = match &frame {
                Some((image, _)) => render_game_cursor(&self.fonts, image, scale),
                None => render_pointer(&self.fonts, pressed, scale),
            };
            self.pointer.window.set_image(&picture, x, y);
            self.pointer.drawn = Some(wanted);
        } else {
            self.pointer.window.move_to(x, y);
        }
    }
}

/// The game's cursor frames from the cache, else downloaded into it; None (the drawn arrow) when unavailable.
fn load_cursors() -> Option<[Cursor; 2]> {
    let folder = cache_dir().join("cursor");
    let frame = |n: u32| -> Option<Cursor> {
        let (png_path, origin_path) = (folder.join(format!("{n}.png")), folder.join(format!("{n}.json")));
        let cached = std::fs::read(&png_path).ok().zip(std::fs::read_to_string(&origin_path).ok());
        let (png, origin) = match cached {
            Some(both) => both,
            None => {
                let png = potions::fetch(&format!("{CURSOR_IMAGE}/{n}/0")).ok()?;
                let origin = potions::fetch(&format!("{CURSOR_DATA}/{n}/0/origin")).ok()?;
                let origin = String::from_utf8_lossy(&origin).into_owned();
                let _ = paths::write_atomic(&png_path, &png);
                let _ = paths::write_atomic(&origin_path, origin.as_bytes());
                (png, origin)
            }
        };
        let (width, height, data) = decode_png_rgba(&png).ok()?;
        Some((Arc::new(RgbaImage { width, height, data }), parse_origin(&origin)?))
    };
    let frames = [frame(CURSOR_FRAMES[0])?, frame(CURSOR_FRAMES[1])?];
    crate::info!("the game's cursor is ready");
    Some(frames)
}
