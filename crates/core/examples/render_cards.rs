//! Render sample cards to PNG files for a visual check: `cargo run -p maplelens-core --example render_cards -- OUT_DIR`.

use maplelens_core::mapinfo::{MapInfo, Mob, Npc};
use maplelens_core::market::sample::sample_items;
use maplelens_core::market::time::parse_iso;
use maplelens_core::notify::pairing::{Pairing, QR_SHOWN_S, Reply};
use maplelens_core::ranking::{Rank, RankResult, Status};
use maplelens_core::render::RgbaImage;
use maplelens_core::render::card::{CardSpec, render_card};
use maplelens_core::render::font::Fonts;
use maplelens_core::render::map_card::{render_loading_card, render_map_card};
use maplelens_core::render::rank_card::render_rank_card;
use maplelens_core::render::settings_view::{self, Control, View};
use maplelens_core::render::status_pill::render_status;
use maplelens_core::shop::ShopStatus;

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
