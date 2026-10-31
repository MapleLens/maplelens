//! The map drops panel on the UI thread: what the map being played drops, opened by the EXP strip's bag button and
//! closed by its X (never by itself), over the strip in a bottom corner and under it in a top one (or wherever the
//! player dragged it), only over the game (rule 5) and only on a field whose drops are known. It stays click-through
//! (rule 3): what is under the pointer is found from the cursor position alone (`pointer` reads the presses). A slot
//! opens that drop's price card under the item's name; a monster pressed opens its page, which goes back to the list
//! after a few seconds without the pointer.

use std::collections::HashMap;
use std::sync::Arc;

use maplelens_core::drops::{self, MapDrops};
use maplelens_core::geometry::{Point, Rect};
use maplelens_core::mapinfo::MapInfo;
use maplelens_core::market::models::Item;
use maplelens_core::placement::{fraction_of, nearest_corner, place_at, place_from_panel, place_off_card};
use maplelens_core::render::card::{CardSpec, Heading, render_named_card};
use maplelens_core::render::drops_card::{Hit, PanelState, Slot, colour, hit, render_drops_card, slot_top};
use maplelens_core::render::{RgbaImage, theme};
use windows::Win32::Foundation::HWND;

use super::App;
use super::pointer::Card;
use crate::drops::DropBook;
use crate::game;
use crate::layered::LayeredWindow;
use crate::maps::service::Status;
use crate::paths;

/// Between the EXP strip and the panel, and between the panel and a drop's price card, at the cards' scale 1.
const GAP: f32 = 6.0;
/// A monster's page left without the pointer for this long goes back to the list.
const PAGE_IDLE_S: f64 = 3.0;

/// What the panel's content comes from: (map, what the drop book knows, sprites ready, item icons ready, catalog, the
/// cards' scale bits).
type ContentKey = (u32, u64, usize, usize, usize, u32);
/// The panel's picture: its content and how it is drawn.
type PanelKey = (ContentKey, PanelState);
/// A drop's price card: (item, monster, expanded, catalog, the cards' scale bits).
type PriceKey = (u32, u32, bool, usize, u32);

pub struct DropsUi {
    book: DropBook,
    window: LayeredWindow,
    price_window: LayeredWindow,
    /// The map's drops (None: nothing known for its monsters).
    content: Option<(ContentKey, Option<Arc<MapDrops>>)>,
    /// The panel last drawn, and what it was drawn from (for hit tests between ticks).
    panel: Option<(PanelKey, Arc<RgbaImage>)>,
    drawn: Option<(Arc<MapDrops>, PanelState, f32)>,
    shown: Option<PanelKey>,
    price: Option<(PriceKey, Arc<RgbaImage>)>,
    shown_price: Option<PriceKey>,
    /// The catalog's items by game ID, and the catalog they came from.
    items: (usize, HashMap<u32, Arc<Item>>),
    /// The slot under the pointer and since when: its card opens up after the details delay, as the price card does.
    hovered: Option<(Slot, f64)>,
    /// The monster whose page is open (its game ID) and when the pointer was last over the panel.
    page: Option<(u32, f64)>,
    /// Opened from the EXP strip's button, until its X (or the strip going) closes it.
    open: bool,
    /// The map is a field with monsters: the strip offers the button.
    available: bool,
}

impl DropsUi {
    pub fn new() -> windows::core::Result<Self> {
        Ok(Self {
            book: DropBook::new(),
            window: LayeredWindow::new()?,
            price_window: LayeredWindow::new()?,
            content: None,
            panel: None,
            drawn: None,
            shown: None,
            price: None,
            shown_price: None,
            items: (0, HashMap::new()),
            hovered: None,
            page: None,
            open: false,
            available: false,
        })
    }

    pub fn hide(&mut self) {
        self.window.hide();
        self.price_window.hide();
        self.hovered = None;
    }

    pub fn panel_rect(&self) -> Option<Rect> {
        self.window.rect().filter(|_| self.window.visible())
    }

    pub fn price_rect(&self) -> Option<Rect> {
        self.price_window.rect().filter(|_| self.price_window.visible())
    }

    /// What is under `cursor` in the panel as last drawn.
    pub fn hit_at(&self, cursor: Point) -> Option<Hit> {
        let panel = self.panel_rect().filter(|r| r.contains(cursor))?;
        let (drops, state, scale) = self.drawn.as_ref()?;
        let factor = theme::CARD_SCALE * scale;
        let (x, y) = ((cursor.x - panel.x) as f32 / factor, (cursor.y - panel.y) as f32 / factor);
        hit(drops, PanelState { hover: None, ..*state }, x, y)
    }

    /// Open the page of the monster at `index` in the list as drawn.
    pub fn open_page(&mut self, index: usize, now: f64) {
        if let Some(mob) = self.drawn.as_ref().and_then(|(drops, ..)| drops.mobs.get(index)) {
            self.page = Some((mob.id, now));
        }
    }

    pub fn close_page(&mut self) {
        self.page = None;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn available(&self) -> bool {
        self.available
    }

    /// The strip's button: open the panel, or close it.
    pub fn toggle(&mut self) {
        if self.open { self.close() } else { self.open = true }
    }

    pub fn close(&mut self) {
        self.open = false;
        self.page = None;
    }

    /// The panel was put together for map `id` and nothing it drops is known.
    pub fn knows_nothing_of(&self, id: u32) -> bool {
        self.content.as_ref().is_some_and(|((map, ..), content)| *map == id && content.is_none())
    }
}

impl App {
    pub(super) fn follow_drops(&mut self) {
        if !self.place_drops() {
            self.drops.hide();
        }
    }

    /// Draw and place the panel and a hovered drop's price card; false when there is nothing to show. Nothing is
    /// asked of the drops' site until the panel is opened.
    fn place_drops(&mut self) -> bool {
        self.drops.available = false;
        if !self.settings.overlay_enabled {
            return false;
        }
        let Some((hwnd, training)) = &self.training else { return false };
        let Some(location) = training.location.clone() else { return false };
        let hwnd = HWND(*hwnd as *mut _);
        let client = (game::is_alive(hwnd) && game::is_foreground(hwnd)).then(|| game::client_rect(hwnd)).flatten();
        let Some(client) = client else { return false };
        let service = self.map.service().clone();
        let result = service.get(&location);
        let Some(info) = result.info.filter(|i| result.status == Status::Found && !i.is_town) else { return false };
        self.drops.available = !info.mobs.is_empty();
        if !self.drops.open || !self.drops.available {
            return false;
        }
        let scale = self.card_scale(client);
        let catalog = Arc::as_ptr(&self.catalog) as usize;
        if self.drops.items.0 != catalog {
            self.drops.items = (catalog, drops::items_by_id(self.catalog.items()));
        }
        let book = &self.drops.book;
        let key = (info.id, book.generation(), result.icons, book.icons_ready(), catalog, scale.to_bits());
        let Some(content) = self.drops_content(key, &info) else { return false };
        let state = self.panel_state(&content, scale);
        if self.drops.panel.as_ref().is_none_or(|(k, _)| *k != (key, state)) {
            let book = &self.drops.book;
            let icons = |kind: &str, id: u32| if kind == "item" { book.icon(id) } else { service.icon(kind, id) };
            let image = render_drops_card(&self.fonts, &content, &icons, state, scale);
            self.drops.panel = Some(((key, state), Arc::new(image)));
        }
        let Some((_, panel)) = self.drops.panel.clone() else { return false };
        let size = (panel.width as i32, panel.height as i32);
        let Some((x, y)) = self.drops_spot(client, size, scale) else { return false };
        if self.drops.shown != Some((key, state)) || !self.drops.window.visible() {
            self.drops.window.set_image(&panel, x, y);
            self.drops.shown = Some((key, state));
        } else {
            self.drops.window.move_to(x, y);
        }
        self.drops.drawn = Some((content.clone(), state, scale));
        let rect = Rect::new(x, y, size.0, size.1);
        match self.drops.hovered {
            Some((slot, since)) => self.show_drop_price(&content, state, slot, since, rect, client, scale),
            None => self.drops.price_window.hide(),
        }
        true
    }

    /// The open page (back to the list after a while without the pointer), and what the pointer is over.
    fn panel_state(&mut self, content: &MapDrops, scale: f32) -> PanelState {
        let now = self.now();
        let cursor = game::cursor();
        let pointed = cursor.zip(self.drops.panel_rect()).is_some_and(|(c, r)| r.contains(c));
        if let Some((_, seen)) = &mut self.drops.page
            && pointed
        {
            *seen = now;
        }
        if self.drops.page.is_some_and(|(_, seen)| now - seen > PAGE_IDLE_S) {
            self.drops.page = None;
        }
        let page = self.drops.page.and_then(|(id, _)| content.mobs.iter().position(|m| m.id == id));
        let factor = theme::CARD_SCALE * scale;
        let mut state = PanelState { page, hover: None, height: None };
        // What the pointer is over, on the panel as it will be drawn (a page opened this tick included).
        state.hover = cursor
            .zip(self.drops.panel_rect())
            .filter(|(c, r)| r.contains(*c))
            .and_then(|(c, r)| hit(content, state, (c.x - r.x) as f32 / factor, (c.y - r.y) as f32 / factor));
        let slot = match state.hover {
            Some(Hit::Slot(slot)) => Some(slot),
            _ => None,
        };
        if self.drops.hovered.is_none_or(|(s, _)| Some(s) != slot) {
            self.drops.hovered = slot.map(|slot| (slot, now));
        }
        state
    }

    /// Where the panel goes: where it is being dragged or was let go, else over the EXP strip in a bottom corner and
    /// under it in a top one (the corner that strip is nearest); nowhere without the strip, which it opens from.
    fn drops_spot(&self, client: Rect, size: (i32, i32), scale: f32) -> Option<(i32, i32)> {
        let dragged = self.pointer.dragged(Card::Drops).map(|at| fraction_of(client, at));
        if let Some(fraction) = dragged.or(self.settings.drops_at) {
            return place_at(client, size, fraction);
        }
        let gap = (GAP * scale).round() as i32;
        let card = self.training_window.rect().filter(|_| self.training_window.visible())?;
        place_off_card(card, size, nearest_corner(card, client), gap, client)
    }

    /// The map's drops for `key`, assembled again only when something it comes from changed.
    fn drops_content(&mut self, key: ContentKey, info: &MapInfo) -> Option<Arc<MapDrops>> {
        if self.drops.content.as_ref().is_none_or(|(k, _)| *k != key) {
            let (items, table) = (&self.drops.items.1, self.drops.book.table_for(info));
            let content = drops::assemble(info, &table, |id| items.get(&id).and_then(|item| item.price));
            self.drops.content = Some((key, content.map(Arc::new)));
        }
        self.drops.content.as_ref().and_then(|(_, content)| content.clone())
    }

    /// The hovered drop's price card under its name, beside the panel and level with the drop's slot.
    #[allow(clippy::too_many_arguments)]
    fn show_drop_price(
        &mut self,
        content: &MapDrops,
        state: PanelState,
        slot: Slot,
        since: f64,
        panel: Rect,
        client: Rect,
        scale: f32,
    ) {
        let Some(mob) = content.mobs.get(slot.0) else { return self.drops.price_window.hide() };
        let Some(drop) = mob.drops.get(slot.1) else { return self.drops.price_window.hide() };
        let expanded = self.now() - since >= self.settings.expand_delay_s;
        let key = (drop.item, mob.id, expanded, self.drops.items.0, scale.to_bits());
        if self.drops.price.as_ref().is_none_or(|(k, _)| *k != key) {
            let item = self.drops.items.1.get(&drop.item).cloned();
            let source = if self.catalog.generated_at.is_some() { "meso.watch" } else { "demo data" };
            let spec = CardSpec {
                item: item.as_deref(),
                title: &drop.name,
                generated_at: self.catalog.generated_at.as_deref(),
                now: paths::now_unix(),
                expanded,
                ui_scale: scale,
                wide: false,
                source,
            };
            let droppers = content.droppers(drop.item);
            let note = drops::note(drop, &droppers.join(", "));
            let rate = drops::rate_note(drop, &mob.name, droppers.len() > 1).unwrap_or_default();
            let heading = Heading {
                name: &drop.name,
                note: &note,
                colour: colour(drop.rarity),
                rate: &rate,
                source: &mob.credit,
            };
            self.drops.price = Some((key, Arc::new(render_named_card(&self.fonts, &spec, &heading))));
        }
        let Some((_, card)) = self.drops.price.clone() else { return };
        let size = (card.width as i32, card.height as i32);
        let top = panel.y + (slot_top(content, state, slot) * theme::CARD_SCALE * scale).round() as i32;
        let gap = (GAP * scale).round() as i32;
        let corner = nearest_corner(panel, client);
        let Some((x, y)) = place_from_panel(panel, top, size, corner, gap, client) else {
            return self.drops.price_window.hide();
        };
        if self.drops.shown_price != Some(key) || !self.drops.price_window.visible() {
            self.drops.price_window.set_image(&card, x, y);
            self.drops.shown_price = Some(key);
        } else {
            self.drops.price_window.move_to(x, y);
        }
    }
}
