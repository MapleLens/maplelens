//! The update chain: signatures, versions, URLs, downloads, the zip, the swap, and what the UI is told.

use std::io::Write;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

use super::check::{CheckResult, Fetch, UpdateChecker, download_url, find_update, release_page};
use super::install::{self, NEW_NAME, OLD_NAME};
use super::manifest::{Manifest, asset_name, manifest_name, verify_manifest};
use super::net::{check_url, stream};
use super::version::parse_version;
use super::{Phase, Update, UpdateError, UpdateState};

fn key() -> SigningKey {
    SigningKey::from_bytes(&[7u8; 32])
}

fn public() -> String {
    STANDARD.encode(key().verifying_key().to_bytes())
}

fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// A manifest for `version` and its signature, with fields overridden as JSON.
fn signed(version: &str, overrides: serde_json::Value) -> (Vec<u8>, Vec<u8>) {
    let mut fields = serde_json::json!({
        "version": version, "asset": asset_name(version), "size": 9, "sha256": sha256_hex(b"zip-bytes")
    });
    for (k, v) in overrides.as_object().cloned().unwrap_or_default() {
        fields[k] = v;
    }
    let data = serde_json::to_vec(&fields).unwrap();
    let signature = STANDARD.encode(key().sign(&data).to_bytes()).into_bytes();
    (data, signature)
}

fn fake_fetch(tag: &'static str, manifest_version: &'static str, prerelease: bool, tamper: bool) -> Box<Fetch> {
    let (mut data, signature) = signed(manifest_version, serde_json::json!({}));
    if tamper {
        data = String::from_utf8(data).unwrap().replace("zip", "zap").into_bytes();
    }
    Box::new(move |url: &str, _limit: u64| {
        if url.ends_with("/latest") {
            return Ok(serde_json::json!({"tag_name": tag, "prerelease": prerelease, "draft": false})
                .to_string()
                .into_bytes());
        }
        Ok(if url.ends_with(".sig") { signature.clone() } else { data.clone() })
    })
}

#[test]
fn versions_compare_numerically_and_reject_odd_tags() {
    assert!(parse_version("v0.10.0").unwrap() > parse_version("0.9.9").unwrap());
    for bad in ["v1.2", "1.2.3-rc1", "latest", "", "v1.2.3\n; x", "1.2.3.4"] {
        assert!(parse_version(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn a_valid_signature_gives_the_manifest() {
    let (data, signature) = signed("0.2.0", serde_json::json!({}));
    let manifest = verify_manifest(&data, &signature, &public()).unwrap();
    assert_eq!(
        manifest,
        Manifest { version: "0.2.0".into(), asset: asset_name("0.2.0"), size: 9, sha256: sha256_hex(b"zip-bytes") }
    );
}

#[test]
fn signed_but_malformed_manifests_are_refused() {
    for overrides in [
        serde_json::json!({"asset": "evil.zip"}),
        serde_json::json!({"size": 0}),
        serde_json::json!({"size": "9"}),
        serde_json::json!({"size": true}),
        serde_json::json!({"sha256": "ABC"}),
        serde_json::json!({"version": "0.2"}),
    ] {
        let (data, signature) = signed("0.2.0", overrides.clone());
        assert!(verify_manifest(&data, &signature, &public()).is_err(), "{overrides}");
    }
}

#[test]
fn a_wrong_key_tampered_bytes_or_garbage_signature_are_refused() {
    let (data, signature) = signed("0.2.0", serde_json::json!({}));
    let other = STANDARD.encode(SigningKey::from_bytes(&[9u8; 32]).verifying_key().to_bytes());
    assert!(verify_manifest(&data, &signature, &other).is_err());
    let mut tampered = data.clone();
    tampered[5] ^= 1;
    assert!(verify_manifest(&tampered, &signature, &public()).is_err());
    assert!(verify_manifest(&data, b"not base64!", &public()).is_err());
}

/// A release's manifest, signature and zip checked as the app checks them: `MAPLELENS_MANIFEST` is the .json (its .sig
/// and the zip beside it), and `MAPLELENS_PUBLIC_KEY` another key than the app's, to try scripts/sign-release.sh with
/// a throwaway one: `cargo test -p maplelens release_files_verify -- --ignored --nocapture`.
#[test]
#[ignore = "checks the release files named in MAPLELENS_MANIFEST"]
fn release_files_verify() {
    let Ok(path) = std::env::var("MAPLELENS_MANIFEST") else { return };
    let key = std::env::var("MAPLELENS_PUBLIC_KEY").unwrap_or_else(|_| super::PUBLIC_KEY.to_string());
    let (data, signature) = (std::fs::read(&path).unwrap(), std::fs::read(format!("{path}.sig")).unwrap());
    let manifest = verify_manifest(&data, &signature, &key).unwrap();
    println!("{manifest:?}");
    let zip = std::fs::read(std::path::Path::new(&path).with_file_name(&manifest.asset)).unwrap();
    assert_eq!((zip.len() as u64, sha256_hex(&zip)), (manifest.size, manifest.sha256));
}

#[test]
fn the_built_in_key_is_a_valid_ed25519_key() {
    let raw = STANDARD.decode(super::PUBLIC_KEY).unwrap();
    assert!(ed25519_dalek::VerifyingKey::from_bytes(&raw.try_into().unwrap()).is_ok());
}

#[test]
fn a_newer_signed_release_is_offered_with_urls_built_locally() {
    let update =
        find_update("0.1.3", fake_fetch("v0.2.0", "0.2.0", false, false).as_ref(), &public()).unwrap().unwrap();
    assert_eq!(update.version, "0.2.0");
    assert_eq!(update.url, download_url("0.2.0", &asset_name("0.2.0")));
    assert_eq!(update.page_url, release_page("0.2.0"));
}

#[test]
fn same_older_and_pre_releases_offer_nothing() {
    for (current, pre) in [("0.2.0", false), ("0.3.0", false), ("0.1.3", true)] {
        assert_eq!(find_update(current, fake_fetch("v0.2.0", "0.2.0", pre, false).as_ref(), &public()).unwrap(), None);
    }
}

#[test]
fn a_replayed_or_tampered_manifest_is_refused() {
    let replayed = find_update("0.1.3", fake_fetch("v0.3.0", "0.2.0", false, false).as_ref(), &public());
    assert_eq!(replayed, Err(UpdateError("the update manifest is for a different version".into())));
    assert!(find_update("0.1.3", fake_fetch("v0.2.0", "0.2.0", false, true).as_ref(), &public()).is_err());
}

#[test]
fn the_checker_reports_failures_and_stays_quiet_until_asked() {
    let offline: Box<Fetch> = Box::new(|_: &str, _: u64| Err(UpdateError("offline".into())));
    let checker = UpdateChecker::spawn("0.1.3", true, offline, &public());
    assert_eq!(checker.events.recv_timeout(Duration::from_secs(5)).unwrap(), CheckResult::Failed("offline".into()));

    let quiet = UpdateChecker::spawn("0.2.0", false, fake_fetch("v0.2.0", "0.2.0", false, false), &public());
    assert!(quiet.events.recv_timeout(Duration::from_millis(300)).is_err());
    quiet.check_now();
    assert_eq!(quiet.events.recv_timeout(Duration::from_secs(5)).unwrap(), CheckResult::Found(None));
}

#[test]
fn only_https_github_hosts_are_fetched() {
    for url in [
        "http://github.com/x",
        "https://evil.example/x",
        "https://github.com.evil.example/x",
        "https://user@evil.example/x",
    ] {
        assert!(check_url(url).is_err(), "{url}");
    }
    for url in
        ["https://objects.githubusercontent.com/x", "https://github.com/MapleLens/x?y=1", "https://api.github.com"]
    {
        assert!(check_url(url).is_ok(), "{url}");
    }
}

#[test]
fn a_redirect_the_allow_list_accepts_goes_where_the_client_would_connect() {
    // Each is read one way by a naive host check and maybe another by a URL parser: whenever `check_url` lets one
    // through, the HTTP client must either refuse it or connect to that same allowed host.
    for url in [
        r"https://evil.example\@github.com/x",
        "https://evil.example%40github.com/x",
        "https://github.com@evil.example/x",
        "https://github.com:443@evil.example/x",
        "https://github.com#@evil.example/x",
        "https://github.com?@evil.example/x",
        "https://github.com\t.evil.example/x",
        "https://github.com./x",
        "https://GITHUB.com/x",
        "https://[::1]/x",
    ] {
        if check_url(url).is_err() {
            continue;
        }
        if let Ok(parsed) = url.parse::<ureq::http::Uri>() {
            let host = parsed.host().unwrap_or_default();
            assert!(super::net::ALLOWED_HOSTS.contains(&host), "{url:?} is checked as allowed but goes to {host:?}");
        }
    }
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("maplelens-update-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn downloads_must_match_size_and_hash_exactly() {
    let payload = vec![b'x'; 1000];
    let good = stream(&mut payload.as_slice(), 1000, &sha256_hex(&payload), &mut |_, _| {}).unwrap();
    assert_eq!(good, payload);
    for (size, digest) in [(1000, "0".repeat(64)), (999, sha256_hex(&payload)), (1001, sha256_hex(&payload))] {
        assert!(stream(&mut payload.as_slice(), size, &digest, &mut |_, _| {}).is_err());
    }
}

fn make_update(version: &str) -> Update {
    let manifest = Manifest { version: version.into(), asset: asset_name(version), size: 1, sha256: "0".repeat(64) };
    Update { version: version.into(), manifest, url: "u".into(), page_url: "p".into() }
}

fn make_zip(members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in members {
        zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn only_the_expected_exe_is_taken_from_the_zip() {
    let dir = temp_dir("zip");
    let target = dir.join("out.exe");
    let archive = make_zip(&[
        ("maplelens-v0.2.0-windows/maplelens.exe", b"EXE"),
        ("maplelens-v0.2.0-windows/README.md", b"r"),
        ("../evil.exe", b"bad"),
    ]);
    install::extract_exe(&archive, &make_update("0.2.0"), &target).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"EXE");
    assert!(!dir.parent().unwrap().join("evil.exe").exists());

    let archive = make_zip(&[("maplelens-v0.2.0-windows\\maplelens.exe", b"EXE2")]); // backslashes are fine
    install::extract_exe(&archive, &make_update("0.2.0"), &target).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"EXE2");

    let archive = make_zip(&[("other/maplelens.exe", b"EXE")]);
    assert!(install::extract_exe(&archive, &make_update("0.2.0"), &target).is_err());
    assert!(!target.exists());
    assert!(install::extract_exe(b"not a zip", &make_update("0.2.0"), &target).is_err());
}

#[test]
fn the_staged_exe_is_a_new_file_never_written_through_a_link_left_at_its_name() {
    let dir = temp_dir("link");
    let (victim, target) = (dir.join("victim.dll"), dir.join(NEW_NAME));
    std::fs::write(&victim, b"keep").unwrap();
    // Another program made the staged name a hard link to a file of its choosing.
    std::fs::hard_link(&victim, &target).unwrap();
    let archive = make_zip(&[("maplelens-v0.2.0-windows/maplelens.exe", b"EXE")]);
    install::extract_exe(&archive, &make_update("0.2.0"), &target).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"EXE");
    assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
}

#[test]
fn swap_moves_the_new_exe_in_and_puts_the_old_back_on_failure() {
    let dir = temp_dir("swap");
    let (exe, staged) = (dir.join("maplelens.exe"), dir.join(NEW_NAME));
    std::fs::write(&exe, b"old").unwrap();
    std::fs::write(&staged, b"new").unwrap();
    install::swap(&exe, &staged).unwrap();
    assert_eq!(std::fs::read(&exe).unwrap(), b"new");
    assert_eq!(std::fs::read(dir.join(OLD_NAME)).unwrap(), b"old");

    assert!(install::swap(&exe, &dir.join("missing.exe")).is_err());
    assert_eq!(std::fs::read(&exe).unwrap(), b"new"); // the working version stays
}

#[test]
fn leftovers_are_cleaned_up() {
    let dir = temp_dir("cleanup");
    for name in [OLD_NAME, NEW_NAME] {
        std::fs::write(dir.join(name), b"x").unwrap();
    }
    install::cleanup_leftovers(&dir.join("maplelens.exe"), 3, Duration::from_millis(10));
    assert!(!dir.join(OLD_NAME).exists() && !dir.join(NEW_NAME).exists());
}

#[test]
fn only_an_installed_exe_updates_itself() {
    assert!(install::can_self_update(std::path::Path::new(r"C:\Games\MapleLens\maplelens.exe")));
    assert!(!install::can_self_update(std::path::Path::new(r"C:\src\maplelens\target\release\maplelens.exe")));
    assert!(!install::can_self_update(std::path::Path::new(r"C:\Games\MapleLens\maplelens (1).exe")));
}

/// The real exe, built by `cargo build --release -p maplelens`, if it is there.
fn built_exe() -> Option<std::path::PathBuf> {
    let deps = std::env::current_exe().ok()?;
    let target = deps.ancestors().find(|p| p.file_name().is_some_and(|n| n == "target"))?;
    let exe = target.join("release").join("maplelens.exe");
    exe.exists().then_some(exe)
}

#[test]
fn the_self_test_runs_the_new_exe_and_checks_its_version() {
    let Some(exe) = built_exe() else {
        eprintln!("no release build to self-test: skipped");
        return;
    };
    install::self_test(&exe, env!("CARGO_PKG_VERSION")).unwrap();
    assert!(install::self_test(&exe, "9.9.9").is_err());
    assert!(install::self_test(&exe.with_file_name("missing.exe"), "0.2.0").is_err());
}

#[test]
fn status_text_and_buttons() {
    let update = Some(make_update("0.2.0"));
    let state = |phase, update: Option<Update>, detail: &str| UpdateState::new(phase, update, detail);
    assert_eq!(
        state(Phase::Available, update.clone(), "").describe(),
        ("Version 0.2.0 is available.".into(), true, true)
    );
    assert_eq!(state(Phase::UpToDate, None, "").describe(), ("You have the latest version.".into(), true, false));
    let (text, check, go) = state(Phase::Downloading, update.clone(), "42%").describe();
    assert!(text.contains("42%") && !check && !go);
    assert!(state(Phase::Failed, update, "boom").describe().2); // can retry
    assert!(!state(Phase::Failed, None, "offline").describe().2);
    assert_eq!(manifest_name("0.2.0"), "maplelens-v0.2.0-manifest.json");
}

#[test]
fn the_tray_tooltip_announces_an_update() {
    assert_eq!(crate::tray::tooltip(None), concat!("MapleLens v", env!("CARGO_PKG_VERSION")));
    assert!(crate::tray::tooltip(Some("0.3.0")).ends_with("new update available (v0.3.0)"));
}
