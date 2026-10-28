//! classicrank.com responses and the rank cache; user settings.

use maplelens_core::config;
use maplelens_core::ranking::{Fetched, Rank, RankCache, Status, look_alikes, parse_response, rank_url};
use maplelens_core::settings::{Corner, DEFAULT_HOTKEY, Settings, delay_text};
use serde_json::json;

fn raw() -> serde_json::Value {
    json!({"name": "masterball", "level": 27, "job": "Thief", "fame": 3, "overall_rank": 29516, "job_rank": 824,
           "fame_rank": 21312, "botting": false, "last_checked": "2026-10-22T10:16:24Z", "future_field": 1,
           "photo": "https://classicrank.com/api/avatar/masterball?v=1"})
}

#[test]
fn rank_parses_the_api_and_ignores_unknown_fields() {
    let rank = Rank::from_api(&raw()).unwrap();
    assert_eq!(
        (rank.overall_rank, rank.job_rank, rank.fame_rank, rank.botting),
        (Some(29516), Some(824), Some(21312), false)
    );
    assert_eq!(rank.photo.as_deref(), Some("https://classicrank.com/api/avatar/masterball?v=1"));
}

#[test]
fn rank_tolerates_unknown_job_and_fame() {
    let rank = Rank::from_api(&json!({"name": "x", "job": null, "fame": null, "job_rank": null})).unwrap();
    assert!(rank.job.is_none() && rank.fame.is_none() && rank.job_rank.is_none());
}

#[test]
fn response_is_unwrapped_and_the_name_quoted() {
    assert!(matches!(parse_response(&json!({"data": raw()}).to_string()), Fetched::Found(r) if r.name == "masterball"));
    assert!(matches!(parse_response("<html>"), Fetched::Failed(_)));
    assert!(rank_url("a b").contains("/characters/a%20b?fields="));
}

#[test]
fn cache_answers_loading_then_caches_the_result() {
    let mut cache = RankCache::default();
    assert_eq!(cache.get("masterball", 0.0).status, Status::Loading);
    let name = cache.next_due().unwrap();
    cache.store(&name, Fetched::Found(Rank::from_api(&raw()).unwrap()), 0.0);
    assert_eq!(cache.get("masterball", 0.0).status, Status::Found);
    assert_eq!(cache.next_due(), None); // fresh: no second lookup
    assert_eq!(cache.get("masterball", 1000.0).status, Status::Found); // a stale entry shows while it refreshes
    assert_eq!(cache.next_due().as_deref(), Some("masterball"));
}

#[test]
fn cache_remembers_unknown_names_and_failures() {
    let mut cache = RankCache::default();
    for (name, fetched, status) in
        [("nobody", Fetched::Missing, Status::Missing), ("boom", Fetched::Failed(15.0), Status::Failed)]
    {
        cache.get(name, 0.0);
        let due = cache.next_due().unwrap();
        cache.store(&due, fetched, 0.0);
        assert_eq!(cache.get(name, 0.0).status, status);
    }
    // Unknown names are not asked about again for a while (often a misread).
    assert!(cache.get("nobody", config::RANK_MISSING_TTL_S - 1.0).expires_at > config::RANK_MISSING_TTL_S - 1.0);
}

#[test]
fn a_failure_keeps_the_rank_already_known() {
    let mut cache = RankCache::default();
    cache.store("masterball", Fetched::Found(Rank::from_api(&raw()).unwrap()), 0.0);
    cache.store("masterball", Fetched::Failed(30.0), 100.0);
    let result = cache.get("masterball", 100.0);
    assert_eq!(result.status, Status::Found);
    assert!(result.rank.is_some());
}

#[test]
fn settings_default_roundtrip_and_validation() {
    assert_eq!(Settings::from_json(""), Settings::default());
    assert_eq!(Settings::from_json("{not json"), Settings::default());
    let wanted = Settings {
        launch_at_startup: true,
        overlay_enabled: false,
        toggle_hotkey: 0,
        expand_delay_s: 0.5,
        check_updates: false,
        store_status_bar: false,
        discord_webhook: "https://discord.com/api/webhooks/12/abc_DEF-9".into(),
        telegram_webhook:
            "https://api.telegram.org/bot123456789:AAFakeTokenForTests_abcdefghijklmnop-Q/sendMessage?chat_id=-100222"
                .into(),
        phone_key: "AbCdEfGhIjKlMnOp.qRsTuVwXyZ0123456789-_ab".into(),
        notify_sales: false,
        notify_store: false,
        training_card: false,
        quest_helper: false,
        map_drops: false,
        training_corner: Corner::BottomLeft,
        training_at: Some((0.25, 0.5)),
        drops_at: Some((0.75, 0.0)),
    };
    assert_eq!(Settings::from_json(&wanted.to_json()), wanted);
    // Invalid fields fall back one by one; unknown ones are ignored.
    let partly = r#"{"launch_at_startup": true, "expand_delay_s": 99, "overlay_enabled": "yes", "toggle_hotkey": -1, "future_option": 1}"#;
    assert_eq!(Settings::from_json(partly), Settings { launch_at_startup: true, ..Settings::default() });
    assert_eq!(Settings::default().toggle_hotkey, DEFAULT_HOTKEY);
    assert!(Settings::default().store_status_bar);
    assert!(Settings::default().training_card);
    assert!(!Settings::default().quest_helper, "the party quest helper is hidden");
    assert!(Settings::default().map_drops);
    assert_eq!(Settings::default().training_at, None);
    let dragged = Settings::from_json(r#"{"training_at": [0.5, 2.0], "drops_at": [0.1, 0.2, 0.3]}"#);
    assert_eq!((dragged.training_at, dragged.drops_at), (None, None), "off the window, or not a pair");
    assert_eq!(Settings::default().training_corner, Corner::BottomRight);
    assert_eq!(Settings::from_json(r#"{"training_corner": "middle"}"#).training_corner, Corner::BottomRight);
    for corner in Corner::ALL {
        assert_eq!(Corner::from_key(corner.key()), Some(corner));
        assert_eq!(Corner::of(corner.is_top(), corner.is_left()), corner);
    }
    assert!(Settings::from_json(r#"{"store_status_bar": "nope"}"#).store_status_bar, "a bad value keeps the default");
    // Only a real Discord webhook is kept; the secret survives a round trip and nothing else does.
    for bad in ["https://example.com/api/webhooks/12/abc", "not a url", "https://discord.com/api/webhooks/12/abc?x=1"] {
        let json = format!("{{\"discord_webhook\": \"{bad}\"}}");
        assert_eq!(Settings::from_json(&json).discord_webhook, "", "{bad}");
    }
    // Only a Telegram sendMessage URL (or a bot token) is kept, in the one URL form.
    let bad = r#"{"telegram_webhook": "https://api.telegram.org/bot123:short/sendMessage?chat_id=1"}"#;
    assert_eq!(Settings::from_json(bad).telegram_webhook, "");
    assert_eq!(Settings::from_json(r#"{"phone_key": "a.b"}"#).phone_key, "");
    let token = r#"{"telegram_webhook": " 123456789:AAFakeTokenForTests_abcdefghijklmnop-Q "}"#;
    assert_eq!(
        Settings::from_json(token).telegram_webhook,
        "https://api.telegram.org/bot123456789:AAFakeTokenForTests_abcdefghijklmnop-Q/sendMessage"
    );
    assert_eq!(wanted.notify().telegram_chat, -100_222);
    assert!(
        Settings::default().notify_sales
            && Settings::default().notify_store
            && Settings::default().discord_webhook.is_empty()
    );
    assert_eq!(delay_text(0.0), "instantly");
    assert_eq!(delay_text(1.25), "after 1.2 s");
}

#[test]
fn i_and_l_look_alikes_fewest_changes_first() {
    // The small name font draws capital I and lowercase l as the same bar.
    assert_eq!(look_alikes("SharkChiIi", 3), ["SharkChili"]);
    assert_eq!(look_alikes("IlI", 3), ["llI", "III", "Ill"]);
    assert_eq!(look_alikes("Cubix", 3), Vec::<String>::new());
    assert_eq!(look_alikes("IIIIIIIIIIIIII", 3), Vec::<String>::new(), "longer than any name: junk");
}
