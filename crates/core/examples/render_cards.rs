//! Render sample cards to PNG files for a visual check: `cargo run -p maplelens-core --example render_cards -- OUT_DIR`.

use std::sync::Arc;

use maplelens_core::drops::{self, sample as drop_sample};
use maplelens_core::geometry::Rect;
use maplelens_core::mapinfo::{MapInfo, Mob, Npc};
use maplelens_core::market::sample::sample_items;
use maplelens_core::market::time::parse_iso;
use maplelens_core::notify::pairing::{Pairing, QR_SHOWN_S, Reply};
use maplelens_core::ranking::{Rank, RankResult, Status};
use maplelens_core::render::RgbaImage;
use maplelens_core::render::canvas::Canvas;
use maplelens_core::render::card::{CardSpec, Heading, render_card, render_named_card};
use maplelens_core::render::drops_card::{Hit, PanelState, colour, render_drops_card};
use maplelens_core::render::font::Fonts;
use maplelens_core::render::map_card::{render_loading_card, render_map_card};
use maplelens_core::render::rank_card::render_rank_card;
use maplelens_core::render::settings_view::{self, Control, View};
use maplelens_core::render::status_pill::render_status;
use maplelens_core::render::training_card::{Button, Show, Strip, render_training_card};
use maplelens_core::settings::Corner;
use maplelens_core::shop::ShopStatus;
use maplelens_core::training::kpq::Run;
use maplelens_core::training::potions::{PotionLine, Potions};
use maplelens_core::training::session::{ExpGraph, MapStat, Session, TrainingView};
use maplelens_core::training::watcher::Training;
use maplelens_core::vision::gauge::ExpReading;

fn save(image: &RgbaImage, path: &std::path::Path) {
    let file = std::fs::File::create(path).expect("output file");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), image.width as u32, image.height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().expect("png header").write_image_data(&image.data).expect("png data");
}

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    let fonts = Fonts::from_bytes(
        std::fs::read("C:/Windows/Fonts/tahoma.ttf").expect("Tahoma"),
        std::fs::read("C:/Windows/Fonts/tahomabd.ttf").expect("Tahoma Bold"),
    )
    .expect("fonts parse");
    let now = parse_iso("2026-10-22T00:00:00Z").unwrap();
    let items = sample_items();
    for (name, expanded, wide) in [("glance", false, false), ("full", true, false), ("wide", true, true)] {
        let spec = CardSpec {
            item: Some(&items[0]),
            title: "",
            generated_at: Some("2026-10-21T23:45:49Z"),
            now,
            expanded,
            ui_scale: 1.0,
            wide,
            source: "meso.watch",
        };
        save(&render_card(&fonts, &spec), &out.join(format!("card_{name}.png")));
    }
    let spec = CardSpec {
        item: None,
        title: "Mystery Item",
        generated_at: None,
        now,
        expanded: false,
        ui_scale: 1.0,
        wide: false,
        source: "meso.watch",
    };
    save(&render_card(&fonts, &spec), &out.join("card_unknown.png"));
    let rank = Rank {
        name: "masterball".into(),
        level: Some(27),
        job: Some("Thief".into()),
        fame: Some(3),
        overall_rank: Some(29516),
        job_rank: Some(824),
        fame_rank: Some(21312),
        botting: false,
        last_checked: Some("2026-10-21T22:16:24Z".into()),
        profile_url: None,
        photo: None,
    };
    let found = RankResult { status: Status::Found, rank: Some(rank), expires_at: 0.0 };
    save(&render_rank_card(&fonts, &found, now, 1.0), &out.join("rank_found.png"));
    save(&render_rank_card(&fonts, &RankResult::of(Status::Loading), now, 1.0), &out.join("rank_loading.png"));
    for status in [
        ShopStatus::Watching,
        ShopStatus::NeedSoldWindow,
        ShopStatus::Scrolled,
        ShopStatus::Covered,
        ShopStatus::NotOpen,
    ] {
        let pill = render_status(&fonts, status, 1.0).expect("a visible status");
        save(&pill, &out.join(format!("pill_{status:?}.png").to_lowercase()));
    }
    let mob = |id, name: &str, level, exp, hp, count| Mob { id, name: name.into(), level, exp, hp, count };
    let map = MapInfo {
        id: 104040000,
        name: "Henesys Hunting Ground I".into(),
        street: "Victoria Road".into(),
        is_town: false,
        mobs: vec![mob(100101, "Blue Snail", 2, 4, 15, 9), mob(130101, "Red Snail", 4, 8, 45, 3)],
        npcs: vec![Npc { id: 1012000, name: "Athena Pierce".into(), role: "Bowman Instructor".into() }],
    };
    save(&render_map_card(&fonts, &map, &|_, _| None, 1.0, None), &out.join("map_card.png"));
    save(&render_loading_card(&fonts, &map.name, 3, false, 1.0), &out.join("map_loading.png"));
    save(&render_loading_card(&fonts, &map.name, 0, true, 1.0), &out.join("map_failed.png"));

    let view = View {
        version: "0.2.0",
        update_text: "You have the latest version.",
        can_check: true,
        update_label: "Update now",
        hover: Some(Control::Check),
        focus: Some(Control::Overlay),
        ..View::default()
    };
    save(&settings_view::render(&fonts, &view, 1.0), &out.join("settings.png"));
    // The training block's picker: the top-left card (beside the minimap) with a corner under the pointer and the
    // keyboard on another, and the card switched off.
    let picking = View {
        settings: maplelens_core::settings::Settings { training_corner: Corner::TopLeft, ..Default::default() },
        hover: Some(Control::Corner(Corner::BottomRight)),
        focus: Some(Control::Corner(Corner::TopRight)),
        ..view.clone()
    };
    save(&settings_view::render(&fonts, &picking, 2.0), &out.join("settings_corner.png"));
    let off = View {
        settings: maplelens_core::settings::Settings {
            training_card: false,
            quest_helper: false,
            ..Default::default()
        },
        hover: Some(Control::TrainingExp),
        ..view.clone()
    };
    save(&settings_view::render(&fonts, &off, 2.0), &out.join("settings_training_off.png"));
    let discord = View {
        settings: maplelens_core::settings::Settings {
            discord_webhook:
                "https://discord.com/api/webhooks/123456789012345678/abcdefgh_IJKLMNOP-qrstuvwxyz0123456789WXYZ".into(),
            telegram_webhook:
                "https://api.telegram.org/bot123456789:AAFakeTokenForTheExample_abcdefghijk-Q/sendMessage?chat_id=111"
                    .into(),
            ..Default::default()
        },
        notify_note: "Sent: check your Discord channel",
        telegram_note: "Sent: check your Telegram chat",
        hover: Some(Control::TestWebhook),
        ..view.clone()
    };
    save(&settings_view::render(&fonts, &discord, 1.0), &out.join("settings_discord.png"));
    let busy = View {
        capturing: Some("Ctrl + Alt + …"),
        update_text: "Could not check: the release server did not answer in time (connection timed out after 30 s,                       will retry tomorrow)",
        can_check: true,
        can_update: true,
        hover: None,
        focus: None,
        pressed: Some(Control::Update),
        ..view.clone()
    };
    save(&settings_view::render(&fonts, &busy, 1.5), &out.join("settings_busy.png"));
    phone_steps(&fonts, &out, &view);
    let exp_card = training(&fonts, &out);
    map_drops(&fonts, &out, &exp_card);
}

/// The phone's row and the pairing panel in each of their states.
fn phone_steps(fonts: &Fonts, out: &std::path::Path, view: &View<'_>) {
    let key = "AbCdEfGhIjKlMnOp.qRsTuVwXyZ0123456789-_ab";
    let paired = maplelens_core::settings::Settings { phone_key: key.into(), ..Default::default() };
    let mut code = Pairing::default();
    code.show_code(key, 0.0);
    let mut connected = code.clone();
    connected.on_ping(Reply { status: Some(200), phones: Some(1) });
    let mut expired = code.clone();
    expired.tick(QR_SHOWN_S);
    let mut tested = connected.clone();
    tested.on_test(Reply { status: Some(200), phones: None });
    let mut idle = Pairing::default();
    idle.on_ping(Reply { status: Some(200), phones: Some(1) });
    let steps = [
        ("code", code, Some(113)),
        ("connected", connected, None),
        ("tested", tested, None),
        ("expired", expired, None),
        ("idle", idle, None),
    ];
    for (name, phone, left) in steps {
        let shown = View {
            settings: paired.clone(),
            phone,
            phone_left_s: left,
            hover: Some(Control::PanelMain),
            ..view.clone()
        };
        save(&settings_view::render(fonts, &shown, 1.0), &out.join(format!("settings_phone_{name}.png")));
    }
}

/// The last ten minutes of a session an hour in: about 1,850 a minute, a slow stretch looting, a level up.
fn sample_graph() -> ExpGraph {
    let rates = (0..=60)
        .map(|i| {
            let t = i as f64;
            let looting = if (24..=30).contains(&i) { 0.55 } else { 1.0 };
            (1_850.0 + 260.0 * (t / 4.0).sin() + 140.0 * (t / 1.7).cos()) * looting
        })
        .collect();
    ExpGraph { rates, level_ups: vec![230.0] }
}

/// A session an hour in, and the party quest's first and fourth stages, as the web's tall card and the Windows app's
/// strip (with and without its graph, at each UI size, its drops button idle, pointed at and open); returns the strip.
fn training(fonts: &Fonts, out: &std::path::Path) -> RgbaImage {
    let map = MapStat { map: "Ant Tunnel I".into(), active_s: 2_100.0, gained: 61_400 };
    let session = Session {
        active_s: 3_725.0,
        gained: 108_250,
        percent: 412.0,
        levels: 2,
        best_minute: 2_480,
        maps: vec![map.clone()],
        ..Session::default()
    };
    let exp = TrainingView {
        reading: Some(ExpReading { exp: 12_128, hundredths: 5_999 }),
        live: true,
        session,
        last_minute: Some(1_920.0),
        recent_per_min: Some(1_850.0),
        average_per_min: Some(1_744.0),
        percent_per_hour: Some(398.2),
        to_level_s: Some(4.4 * 60.0),
        map: Some(map.map.clone()),
        map_stat: Some(map),
        graph: sample_graph(),
        ..TrainingView::default()
    };
    let both = Show { exp: true, quest: true, strip: None };
    // Potions with their icons when the local captures have them (tests/fixtures/icons: the game's artwork), else the
    // app's cache of them.
    let icon = |id: u32| {
        let fixture = format!("{}/../../tests/fixtures/icons/{id}.png", env!("CARGO_MANIFEST_DIR"));
        let cache = std::path::PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default())
            .join(format!("MapleLens/cache/potions/{id}.png"));
        let png = std::fs::read(fixture).or_else(|_| std::fs::read(cache)).ok()?;
        let (width, height, data) = maplelens_core::image::decode_png_rgba(&png).ok()?;
        Some(Arc::new(RgbaImage { width, height, data }))
    };
    let line = |id: u32, name: &str, used: u32, picked: u32| PotionLine {
        id,
        name: name.into(),
        used,
        picked,
        icon: icon(id),
    };
    let potions = Potions {
        lines: vec![
            line(2_000_002, "White Potion", 34, 0),
            line(2_000_003, "Blue Potion", 12, 3),
            line(2_010_005, "Lemon", 0, 5),
        ],
    };
    let exp_only = Training { exp: exp.clone(), potions, ..Training::default() };
    save(&render_training_card(fonts, &exp_only, both, 0.0, 1.0).expect("card"), &out.join("training_exp.png"));
    for stage in [0, 3] {
        let training =
            Training { exp: exp.clone(), quest: Some(Run { started: 0.0, stage, seen: 400.0 }), ..Training::default() };
        let card = render_training_card(fonts, &training, both, 412.0, 1.0).expect("card");
        save(&card, &out.join(format!("training_kpq_{stage}.png")));
    }
    let strip = |drops| Show { strip: Some(Strip { drops }), ..both };
    let idle = strip(Some(Button::Idle));
    for (name, scale) in [("1200p", 1.0), ("1080p", 0.9), ("ideal_ratio", 0.64)] {
        let card = render_training_card(fonts, &exp_only, idle, 0.0, scale).expect("strip");
        save(&card, &out.join(format!("training_strip_{name}.png")));
    }
    // Twenty seconds in: no graph yet, no potions, no level.
    let young = TrainingView {
        session: Session { active_s: 20.0, gained: 640, best_minute: 640, ..Session::default() },
        last_minute: None,
        recent_per_min: Some(1_920.0),
        average_per_min: Some(1_920.0),
        percent_per_hour: Some(31.0),
        graph: ExpGraph { rates: vec![1_920.0, 1_920.0], level_ups: Vec::new() },
        ..exp
    };
    let young = Training { exp: young, ..Training::default() };
    let card = render_training_card(fonts, &young, idle, 0.0, 0.9).expect("strip");
    save(&card, &out.join("training_strip_no_graph.png"));
    let card = render_training_card(fonts, &exp_only, strip(Some(Button::Lit)), 0.0, 0.9).expect("strip");
    save(&card, &out.join("training_strip_button_lit.png"));
    let card = render_training_card(fonts, &exp_only, strip(None), 0.0, 0.9).expect("strip");
    save(&card, &out.join("training_strip_no_button.png"));
    let quest = Training { quest: Some(Run { started: 0.0, stage: 3, seen: 400.0 }), ..exp_only.clone() };
    let card = render_training_card(fonts, &quest, idle, 412.0, 0.9).expect("strip");
    save(&card, &out.join("training_strip_kpq_3.png"));
    render_training_card(fonts, &exp_only, strip(Some(Button::On)), 0.0, 0.9).expect("strip")
}

/// The map drops panel on the sample field, alone and beside the EXP card in a left and a right corner. Sprites and
/// item icons come from `MAPLELENS_ICONS` (`mob/<id>.png`, `item/<id>.png`), by default the app's sprite cache.
fn map_drops(fonts: &Fonts, out: &std::path::Path, exp_card: &RgbaImage) {
    let folder = std::env::var_os("MAPLELENS_ICONS").map(std::path::PathBuf::from).unwrap_or_else(|| {
        std::path::PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default()).join("MapleLens/cache/icons")
    });
    let icons = |kind: &str, id: u32| {
        let png = std::fs::read(folder.join(kind).join(format!("{id}.png"))).ok()?;
        let (width, height, data) = maplelens_core::image::decode_png_rgba(&png).ok()?;
        Some(Arc::new(RgbaImage { width, height, data }))
    };
    let prices = drop_sample::prices();
    let drops = drops::assemble(&drop_sample::map(), &drop_sample::table(), |id| prices.get(&id).copied());
    let drops = drops.expect("the sample field has drops");
    let panel = render_drops_card(fonts, &drops, &icons, PanelState::default(), 1.0);
    save(&panel, &out.join("drops_card.png"));
    save(
        &render_drops_card(fonts, &drops, &icons, PanelState::default(), 0.64),
        &out.join("drops_card_ideal_ratio.png"),
    );
    // As the app shows them at 1080p (UI scale 0.9): the strip level with the status bar, the panel opened over it.
    let small_panel = render_drops_card(fonts, &drops, &icons, PanelState::default(), 0.9);
    for corner in [Corner::BottomRight, Corner::BottomLeft, Corner::TopRight] {
        let scene = scene(fonts, exp_card, &small_panel, corner);
        save(&scene, &out.join(format!("drops_over_exp_{}.png", corner.key())));
    }
    // The Orange Mushroom's dearest drop hovered: the slot lit up and its price card under its name.
    let hovered = render_drops_card(
        fonts,
        &drops,
        &icons,
        PanelState { hover: Some(Hit::Slot((0, 0))), ..PanelState::default() },
        1.0,
    );
    save(&hovered, &out.join("drops_card_hovered.png"));
    // The Orange Mushroom's page, its Back button under the pointer; and the list with its close box pointed at.
    let page = PanelState { page: Some(0), hover: Some(Hit::Back), height: None };
    save(&render_drops_card(fonts, &drops, &icons, page, 1.0), &out.join("drops_card_page.png"));
    let closing = PanelState { hover: Some(Hit::Close), ..PanelState::default() };
    save(&render_drops_card(fonts, &drops, &icons, closing, 1.0), &out.join("drops_card_close.png"));
    let drop = &drops.mobs[0].drops[0];
    let mut item = sample_items().into_iter().next().expect("a sample item");
    item.name = drop.name.clone();
    item.category = Some("Scrolls".into());
    item.price = drop.price;
    let spec = CardSpec {
        item: Some(&item),
        title: &item.name,
        generated_at: None,
        now: parse_iso("2026-10-22T00:00:00Z").unwrap(),
        expanded: false,
        ui_scale: 1.0,
        wide: false,
        source: "demo data",
    };
    // As NiaDB has it: no chance of its own (rarity by price), OSMLib's reference rate on its own line.
    let as_found = drops::Shown { chance: None, rarity: drops::Rarity::of(None, drop.price), ..drop.clone() };
    let note = drops::note(&as_found, &drops.mobs[0].name);
    let rate = drops::rate_note(&as_found, &drops.mobs[0].name, false).unwrap_or_default();
    let credit = &drops.mobs[0].credit;
    let heading =
        Heading { name: &drop.name, note: &note, colour: colour(as_found.rarity), rate: &rate, source: credit };
    save(&render_named_card(fonts, &spec, &heading), &out.join("drops_price_card.png"));
}

/// A 1920x1080 game at UI scale 0.9 with the status bar drawn in: the EXP strip in `corner` as the app places it, and
/// the drops panel opened from it.
fn scene(fonts: &Fonts, exp_card: &RgbaImage, panel: &RgbaImage, corner: Corner) -> RgbaImage {
    let (width, height) = (1920, 1080);
    let client = Rect::new(0, 0, width, height);
    let bar = Rect::new((width - 1125) / 2, height - 95, 1125, 95);
    let size = (exp_card.width as i32, exp_card.height as i32);
    let place = maplelens_core::placement::place_in_corner;
    let at = place(client, size, corner, 9, Some(bar), Some(200)).expect("card fits");
    let card = Rect::new(at.0, at.1, size.0, size.1);
    let panel_size = (panel.width as i32, panel.height as i32);
    let off = maplelens_core::placement::place_off_card(card, panel_size, corner, 5, client).expect("fits");
    let mut cv = Canvas::new(fonts, width as f32, height as f32, 1.0);
    cv.rect(0.0, 0.0, width as f32, height as f32, [118, 178, 236, 255]);
    cv.rect(0.0, height as f32 - 220.0, width as f32, 220.0, [124, 168, 82, 255]);
    cv.rect(0.0, height as f32 - 95.0, width as f32, 95.0, [10, 10, 12, 255]);
    cv.rect(bar.x as f32, bar.y as f32, bar.w as f32, bar.h as f32, [70, 74, 92, 255]);
    cv.rect(bar.x as f32 + 4.0, bar.y as f32 + 4.0, bar.w as f32 - 8.0, 20.0, [190, 170, 60, 255]);
    for (image, (x, y)) in [(exp_card, at), (panel, off)] {
        cv.image(x as f32, y as f32, image.width as f32, image.height as f32, (image.width, image.height, &image.data));
    }
    RgbaImage::from_canvas(&cv)
}
