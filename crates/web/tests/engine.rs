//! The browser engine on the demo frame the web page ships (docs/app/demo-frame.png): a Character Info window
//! and an Emerald Ore tooltip, no mouse position.

use maplelens_core::image::decode_png;
use maplelens_web::Engine;
use serde_json::Value;

fn docs(path: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../../docs/app/{path}", env!("CARGO_MANIFEST_DIR")))
        .unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn engine() -> Engine {
    Engine::new(docs("fonts/DejaVuSansCondensed.ttf"), docs("fonts/DejaVuSansCondensed-Bold.ttf"))
        .unwrap_or_else(|_| panic!("engine"))
}

fn demo_rgba() -> (Vec<u8>, u32, u32) {
    let frame = decode_png(&docs("demo-frame.png")).unwrap();
    let rgba = frame.data.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect();
    (rgba, frame.width as u32, frame.height as u32)
}

#[test]
fn the_demo_frame_shows_a_price_card_and_a_rank_panel() {
    let mut engine = engine();
    let (rgba, w, h) = demo_rgba();
    // A name counts once two frames read it alike, as the page's stream of frames does.
    let first: Value = serde_json::from_str(&engine.process(&rgba, w, h, 0, 1.0, 0.0, 1_761_000_000.0)).unwrap();
    assert!(first["info"]["name"].is_null());
    let state: Value = serde_json::from_str(&engine.process(&rgba, w, h, 0, 1.0, 0.0, 1_761_000_000.0)).unwrap();
    assert_eq!(state["hover"]["title"], "Emerald Ore");
    assert_eq!(state["hover"]["known"], true); // a demo item
    let at = &state["hover"]["at"];
    assert!((at[0].as_i64().unwrap() - 480).abs() <= 2 && (at[1].as_i64().unwrap() - 372).abs() <= 2, "{at}");
    assert_eq!(state["info"]["name"], "masterball");
    assert!(state["card"].as_u64().is_some());
    assert_eq!(engine.card_pixels().len(), (engine.card_width() * engine.card_height() * 4) as usize);

    // The rank lookup is the page's to make; its answer turns the "looking up" panel into ranks.
    let request: Value = serde_json::from_str(&engine.next_rank_request().expect("a lookup is due")).unwrap();
    assert_eq!(request["name"], "masterball");
    assert!(request["url"].as_str().unwrap().starts_with("https://classicrank.com/api/v1/characters/masterball?"));
    let loading_height = engine.rank_height();
    let body = r#"{"data": {"name": "masterball", "overall_rank": 29516, "job_rank": 824, "fame_rank": 21312, "botting": false}}"#;
    engine.rank_response("masterball", 200, body, 0.0, 0.1);
    let state: Value = serde_json::from_str(&engine.process(&rgba, w, h, 0, 1.0, 0.2, 1_761_000_000.0)).unwrap();
    assert!(state["rank"].as_u64().is_some());
    assert!(engine.rank_height() > loading_height);
}

#[test]
fn the_glance_card_expands_after_a_dwell() {
    let mut engine = engine();
    let (rgba, w, h) = demo_rgba();
    engine.process(&rgba, w, h, 0, 1.0, 0.0, 1_761_000_000.0);
    let glance = engine.card_height();
    engine.process(&rgba, w, h, 0, 1.0, 2.0, 1_761_000_000.0);
    assert!(engine.card_height() > glance);
}

#[test]
fn nothing_is_shown_once_the_tooltip_is_gone() {
    let mut engine = engine();
    let (rgba, w, h) = demo_rgba();
    engine.process(&rgba, w, h, 0, 1.0, 0.0, 0.0);
    let empty = vec![0u8; rgba.len()];
    let state: Value = serde_json::from_str(&engine.process(&empty, w, h, 0, 1.0, 5.0, 0.0)).unwrap();
    assert!(state["hover"].is_null() && state["card"].is_null() && state["info"].is_null());
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn unknown_letters_go_to_the_page_for_ocr_and_come_back() {
    // The Kitty Camping Hat tooltip: 'K' and the touching 'tty' are not in the bundled atlas.
    let (mut rgba, w, h) = demo_rgba();
    let hat = decode_png(
        &std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/tooltip_equip_kitty_camping_hat.png"
        ))
        .unwrap(),
    )
    .unwrap();
    for y in 0..hat.height {
        for x in 0..372 {
            let [r, g, b] = hat.pixel(x, y);
            let i = ((500 + y) * w as usize + 60 + x) * 4;
            rgba[i..i + 3].copy_from_slice(&[r, g, b]);
        }
    }
    let mut engine = engine();
    // Without the page's OCR (the box is not ticked) no job is made: nobody would answer it.
    engine.process(&rgba, w, h, 0, 1.0, 0.0, 0.0);
    assert_eq!(engine.ocr_jobs(), "[]");
    engine.set_ocr(true);
    engine.process(&rgba, w, h, 0, 1.0, 0.05, 0.0);
    let jobs: Value = serde_json::from_str(&engine.ocr_jobs()).unwrap();
    let job = jobs.as_array().unwrap().iter().find(|j| j["font"] == "title").expect("the title needs OCR");
    let id = job["id"].as_u64().unwrap() as u32;
    assert_eq!(
        engine.ocr_job_pixels(id).len() as u64,
        job["width"].as_u64().unwrap() * job["height"].as_u64().unwrap() * 4
    );
    engine.ocr_answer(id, "Kitty Camping Hat");
    // A title that names no item needs the same reading several frames in a row before a card shows for it.
    let mut state = Value::Null;
    for frame in 1..=6 {
        state = serde_json::from_str(&engine.process(&rgba, w, h, 0, 1.0, 0.1 * frame as f64, 0.0)).unwrap();
    }
    assert_eq!(state["hover"]["title"], "Kitty Camping Hat");
    assert_eq!(state["hover"]["known"], false); // not a demo item: the card says there is no price data
}

#[test]
fn a_character_info_window_anywhere_in_the_frame_is_found_within_a_second() {
    // The search looks at one band of the frame at a time: the window must turn up wherever it sits, here low in a
    // taller frame, once the bands have been round.
    let (rgba, w, h) = demo_rgba();
    let (taller, width) = (h as usize * 2, w as usize);
    let mut big = vec![0u8; width * taller * 4];
    for px in big.as_chunks_mut::<4>().0 {
        px[3] = 255;
    }
    let (dy, dx) = (h as usize - 40, 120);
    for y in 0..h as usize {
        for x in 0..w as usize - dx {
            let (src, dst) = ((y * width + x) * 4, ((y + dy) * width + x + dx) * 4);
            big[dst..dst + 4].copy_from_slice(&rgba[src..src + 4]);
        }
    }
    let mut engine = engine();
    let mut found = None;
    for frame in 0..12 {
        let now = frame as f64 * 0.12;
        let state: Value =
            serde_json::from_str(&engine.process(&big, w, taller as u32, 0, 1.0, now, 1_761_000_000.0)).unwrap();
        if !state["info"].is_null() && found.is_none() {
            found = Some(now);
        }
    }
    let at = found.expect("the window was never found");
    assert!(at <= 1.0, "found only at {at} s");
}

/// A 1920x1200 frame (RGBA) with a shop fixture (a crop of a native client) put back where it was cut from.
fn shop_frame(name: &str) -> Vec<u8> {
    let crop = decode_png(
        &std::fs::read(format!("{}/../../tests/fixtures/shop_{name}.png", env!("CARGO_MANIFEST_DIR"))).unwrap(),
    )
    .unwrap();
    let mut rgba = vec![255u8; 1920 * 1200 * 4];
    for y in 0..crop.height {
        for x in 0..crop.width {
            let [r, g, b] = crop.pixel(x, y);
            let i = ((235 + y) * 1920 + 340 + x) * 4;
            rgba[i..i + 4].copy_from_slice(&[r, g, b, 255]);
        }
    }
    rgba
}

fn shop_state(engine: &mut Engine, rgba: &[u8], now: f64) -> Value {
    serde_json::from_str(&engine.process(rgba, 1920, 1200, 0, 1.0, now, 1_761_000_000.0)).unwrap()
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_shop_is_followed_in_the_browser_and_its_sales_are_read_without_ocr() {
    let mut engine = engine();
    engine.set_shop(true);
    let open = shop_frame("025123");
    let mut state = Value::Null;
    for i in 0..4 {
        state = shop_state(&mut engine, &open, i as f64 * 1.5);
    }
    assert_eq!(state["shop"]["status"]["status"], "watching", "{state}");
    // The sold rows are read by the shop's own font: no OCR answer is ever given (only the owner's name plate asks
    // for one at this UI size), and the sales still come.
    engine.set_ocr(true);
    let mut events = Vec::new();
    let mut now = 10.0;
    let later = shop_frame("030620");
    for round in 0..6 {
        state = shop_state(&mut engine, if round < 3 { &open } else { &later }, now);
        now += 1.5;
        events.extend(state["shop"]["events"].as_array().unwrap().iter().cloned());
    }
    let sales: Vec<&Value> = events.iter().filter(|e| e["kind"] == "sale").collect();
    assert_eq!(sales.len(), 2, "{events:?}");
    for sale in sales {
        assert_eq!(sale["item"], "Arrows for Bows");
        assert_eq!(sale["buyer"], "TranzMog");
        assert_eq!(sale["quantity"], 1000);
    }
}

#[test]
fn sales_become_discord_requests_only_for_a_real_webhook() {
    let mut engine = engine();
    engine.set_notify("https://evil.example/api/webhooks/1/x", "123:short", 111.0, true, true);
    assert_eq!(engine.take_requests(), "[]");
    assert!(engine.notify_test_request("https://evil.example/api/webhooks/1/x", 0.0).is_none());
    let url = "https://discord.com/api/webhooks/12/abc_DEF";
    let test: Value = serde_json::from_str(&engine.notify_test_request(url, 0.0).unwrap()).unwrap();
    assert_eq!(test["url"], url);
    assert_eq!(test["service"], "Discord");
    assert!(Engine::webhook_masked(url).ends_with("…_DEF") || Engine::webhook_masked(url).contains('…'));
    assert_eq!(Engine::webhook_masked("nonsense"), "");
}

#[test]
fn a_telegram_bot_is_linked_to_its_chat_then_tested() {
    let engine = engine();
    let token = "123456789:AAFakeTokenForTests_abcdefghijklmnop-Q";
    assert!(Engine::telegram_link_request("nonsense").is_none());
    let link: Value = serde_json::from_str(&Engine::telegram_link_request(token).unwrap()).unwrap();
    assert!(link["url"].as_str().unwrap().ends_with("/getUpdates"));
    let answer = r#"{"ok":true,"result":[{"update_id":1,"message":{"chat":{"id":111,"type":"private"}}}]}"#;
    assert_eq!(Engine::telegram_chat(answer), 111.0);
    assert_eq!(Engine::telegram_chat(r#"{"ok":true,"result":[]}"#), 0.0);
    assert!(engine.telegram_test_request(token, 0.0, 0.0).is_none(), "no chat yet");
    let test: Value = serde_json::from_str(&engine.telegram_test_request(token, 111.0, 0.0).unwrap()).unwrap();
    assert_eq!(test["service"], "Telegram");
    assert!(test["url"].as_str().unwrap().ends_with("/sendMessage"));
    assert_eq!(serde_json::from_str::<Value>(test["body"].as_str().unwrap()).unwrap()["chat_id"], 111);
    assert_eq!(Engine::notify_refusal("Telegram", 401), "Telegram does not know that bot token (revoked?)");
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_status_reads_as_the_windows_pill_reads() {
    let mut engine = engine();
    engine.set_shop(true);
    let mut state = Value::Null;
    for (frame, now) in [("shop_not_open", 0.0), ("shop_not_open", 5.0), ("shop_not_open", 10.0)] {
        state = shop_state(&mut engine, &shop_frame(&frame["shop_".len()..]), now);
    }
    assert_eq!(state["shop"]["status"]["text"], "Store not open", "{state}");
    assert_eq!(state["shop"]["status"]["tone"], "idle");
    // Nothing to say about a screen with no shop: no text, so the page shows no strip.
    let blank = vec![0u8; 1920 * 1200 * 4];
    let mut state = Value::Null;
    for i in 0..4 {
        state = shop_state(&mut engine, &blank, 20.0 + f64::from(i) * 5.0);
    }
    assert!(state["shop"]["status"]["text"].is_null(), "{state}");
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn a_store_found_open_is_announced_to_discord_after_its_owner_wait() {
    let mut loud = engine();
    loud.set_shop(true);
    loud.set_notify("https://discord.com/api/webhooks/12/abc_DEF", "", 0.0, true, true);
    let open = shop_frame("025123");
    let mut requests = Vec::new();
    for i in 0..30 {
        shop_state(&mut loud, &open, f64::from(i) * 1.5);
        let taken: Value = serde_json::from_str(&loud.take_requests()).unwrap();
        requests.extend(taken.as_array().unwrap().iter().cloned());
    }
    assert_eq!(requests.len(), 1, "{requests:?}");
    let body: Value = serde_json::from_str(requests[0]["body"].as_str().unwrap()).unwrap();
    assert_eq!(body["embeds"][0]["title"], "Store detected");
    // With the store switch off nothing about it is sent.
    let mut quiet = engine();
    quiet.set_shop(true);
    quiet.set_notify("https://discord.com/api/webhooks/12/abc_DEF", "", 0.0, true, false);
    for i in 0..30 {
        shop_state(&mut quiet, &open, f64::from(i) * 1.5);
    }
    assert_eq!(quiet.take_requests(), "[]");
}
