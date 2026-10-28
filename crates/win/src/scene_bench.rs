//! Detection on labelled synthetic scenes drawn by the game's own engine (`scripts/scenes.py`):
//! `MAPLELENS_SCENES=<folder> [MAPLELENS_SCENE_SPLIT=eval|train|all] [MAPLELENS_SCENE_KIND=store]
//! [MAPLELENS_SCENE_LIMIT=N] cargo test --release -p maplelens scene_bench -- --ignored --nocapture`.
//!
//! Every scene is read twice: the app's path (the mouse where the scene put it, Windows OCR behind the glyphs) and
//! the browser's (no mouse, the bundled atlas and the shop fonts alone: Tesseract cannot run here, so this is the
//! browser's floor). Items count as right only when the catalog item's id is the scene's; stores by their events
//! (visitors, chat, sales) and the owner's name, each name compared exactly. Misses go to target\scene_bench.tsv.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use maplelens_core::geometry::{Point, Rect};
use maplelens_core::image::{RgbImage, decode_png};
use maplelens_core::market::catalog::Catalog;
use maplelens_core::pipeline::FrameScreen;
use maplelens_core::pipeline::hover::HoverDetector;
use maplelens_core::shop::watcher::{ReadText, ShopWatcher};
use maplelens_core::shop::{ShopEvent, ShopStatus};
use maplelens_core::vision::reader::{NoOcr, TextReader, title_atlas};
use serde_json::Value;

use crate::{ocr, paths, refresh, shop};

/// Looks at one frame, a second apart: the UI scale settles on three readings, a title on two alike.
const LOOKS: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Path {
    App,
    Browser,
}

#[derive(Default, Clone)]
struct Tally {
    total: usize,
    right: usize,
    wrong: usize,
}

impl Tally {
    fn add(&mut self, right: bool, wrong: bool) {
        self.total += 1;
        self.right += right as usize;
        self.wrong += wrong as usize;
    }

    fn line(&self) -> String {
        let pct = |n: usize| 100.0 * n as f64 / self.total.max(1) as f64;
        format!("{:>5} {:>6.2}% right {:>5.2}% wrong", self.total, pct(self.right), pct(self.wrong))
    }
}

#[derive(Default)]
struct Report {
    tallies: BTreeMap<String, Tally>,
    misses: Vec<String>,
}

impl Report {
    fn add(&mut self, key: &str, right: bool, wrong: bool) {
        self.tallies.entry(key.to_string()).or_default().add(right, wrong);
    }
}

fn digits(item_id: &str) -> Option<u64> {
    item_id.rsplit(':').next()?.parse().ok()
}

fn frame(folder: &str, file: &str) -> Option<RgbImage> {
    decode_png(&std::fs::read(format!("{folder}/{file}")).ok()?).ok()
}

fn reader(path: Path) -> TextReader {
    match path {
        Path::App => TextReader::new(title_atlas(), Box::new(ocr::WindowsOcr::new(false).expect("an OCR language"))),
        Path::Browser => TextReader::new(title_atlas(), Box::new(NoOcr)),
    }
}

/// The item a fresh detector names after a few looks at `image`, and the title it read.
fn read_item(catalog: &Arc<Catalog>, image: &RgbImage, cursor: Option<Point>, path: Path) -> (Option<u64>, String) {
    let mut hover = HoverDetector::new(catalog.clone(), reader(path));
    let bounds = Rect::new(0, 0, image.width as i32, image.height as i32);
    let mut screen = FrameScreen { frame: image, origin: Point::default(), cursor };
    let mut last = None;
    for k in 0..LOOKS {
        let found = hover.step(&mut screen, bounds, k as f64 * 1.1);
        if std::env::var("MAPLELENS_SCENE_ID").is_ok() {
            let read = found.as_ref().map(|h| (h.title.clone(), h.found_at));
            println!("{path:?} look {k}: scale {:.3} {read:?}", hover.ui_scale().of(bounds));
        }
        if let Some(found) = found {
            last = Some(found);
        }
    }
    match last {
        Some(h) => (h.item.as_ref().and_then(|it| digits(&it.item_id)), h.title),
        None => (None, String::new()),
    }
}

fn shop_reader(path: Path) -> ReadText {
    match path {
        Path::App => shop::reader(),
        // The fonts read what they know; nothing fills their gaps.
        Path::Browser => Box::new(|_| Some(String::new())),
    }
}

/// Expected events as the watcher would describe them.
fn expected(label: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for e in label["events"].as_array().into_iter().flatten() {
        let e = e.as_array().unwrap();
        let kind = e[0].as_str().unwrap();
        let text = match kind {
            "sale" => format!("{}|{}|{}", e[1].as_str().unwrap(), e[2].as_str().unwrap(), e[3]),
            _ => e[1].as_str().unwrap().to_string(),
        };
        out.push((kind.to_string(), text));
    }
    out
}

fn as_pair(event: &ShopEvent, names: &BTreeMap<u64, String>) -> Option<(String, String)> {
    Some(match event {
        ShopEvent::PlayerEntered(n) => ("entered".into(), n.clone()),
        ShopEvent::PlayerLeft(n) => ("left".into(), n.clone()),
        ShopEvent::ChatMessage(t) => ("chat".into(), t.clone()),
        ShopEvent::Sale { item, buyer, quantity, item_id, .. } => {
            let item = item_id.and_then(|id| names.get(&id).cloned()).unwrap_or_else(|| item.clone());
            ("sale".into(), format!("{item}|{buyer}|{quantity}"))
        }
        ShopEvent::StoreOpened | ShopEvent::StoreClosed => return None,
        other => ("other".into(), other.describe()),
    })
}

/// A sale whose item the game cut ("Two-handed Axe Attack Scr..") is reported as cut when several items start
/// so: that is right, not a miss.
fn same_event(kind: &str, got: &str, want: &str, catalog: &Catalog) -> bool {
    if got == want {
        return true;
    }
    if kind != "sale" {
        return false;
    }
    let (g, w): (Vec<&str>, Vec<&str>) = (got.split('|').collect(), want.split('|').collect());
    let key = |t: &str| t.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_lowercase();
    g.len() == 3
        && g[1..] == w[1..]
        && g[0].ends_with("..")
        && key(w[0]).starts_with(&key(g[0]))
        && catalog.lookup_truncated(g[0]).is_none()
}

/// Capital I and lowercase l are one bar in both of the game's fonts: no reader can tell them apart, so names are
/// compared with them folded (as the watcher keys visitors).
fn fold(text: &str) -> String {
    text.replace('I', "l")
}

fn normal(kind: &str, text: &str) -> String {
    let text = fold(&text.split_whitespace().collect::<Vec<_>>().join(" "));
    if kind == "chat" { text.replace(" : ", ":").replace(" :", ":").replace(": ", ":") } else { text }
}

fn bench_store(
    folder: &str,
    label: &Value,
    catalog: &Arc<Catalog>,
    names: &BTreeMap<u64, String>,
    path: Path,
    report: &mut Report,
    group: &str,
) {
    let files: Vec<&str> = label["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
    let frames: Vec<RgbImage> = files.iter().filter_map(|f| frame(folder, f)).collect();
    if frames.len() != files.len() {
        return;
    }
    let mut watcher = ShopWatcher::new(shop_reader(path));
    watcher.set_catalog(catalog.clone());
    let mut seen = Vec::new();
    let mut opened = false;
    for (k, image) in frames.iter().enumerate() {
        for j in 0..3 {
            let events = watcher.update(image, (k * 3 + j) as f64);
            if std::env::var("MAPLELENS_SCENE_ID").is_ok() {
                println!("{path:?} frame {k} look {j}: {:?} {events:?}", watcher.status());
            }
            for e in events {
                opened |= e == ShopEvent::StoreOpened;
                if k > 0 {
                    seen.extend(as_pair(&e, names));
                }
            }
        }
    }
    let p = format!("{path:?}");
    let kind = label["kind"].as_str().unwrap();
    let id = &label["id"];
    if kind == "visitor" {
        let quiet = !opened && seen.is_empty();
        report.add(&format!("{p} store/visitor quiet"), quiet, !quiet);
        if !quiet {
            report.misses.push(format!("{p}\t{id}\tvisitor\t{group}\tnoise\t{opened} {seen:?}"));
        }
        return;
    }
    let watching = opened && watcher.status() == ShopStatus::Watching;
    report.add(&format!("{p} store/found"), watching, false);
    report.add(&format!("{p} store/found {group}"), watching, false);
    let owner = label["owner"].as_str().unwrap();
    let read_owner = watcher.context().owner.unwrap_or_default();
    let same = fold(&read_owner) == fold(owner);
    report.add(&format!("{p} names/owner"), same, !read_owner.is_empty() && !same);
    if !same {
        report.misses.push(format!("{p}\t{id}\towner\t{group}\t{owner}\t{read_owner}"));
    }
    let want = expected(label);
    let mut got: Vec<(String, String)> = seen.iter().map(|(k, t)| (k.clone(), normal(k, t))).collect();
    for (kind, text) in &want {
        let at = got.iter().position(|(k, t)| k == kind && same_event(kind, t, &normal(kind, text), catalog));
        let found = at.is_some();
        if let Some(at) = at {
            got.remove(at);
        }
        report.add(&format!("{p} store/{kind}"), found, false);
        if kind == "entered" || kind == "left" {
            report.add(&format!("{p} names/visitor"), found, false);
        }
        if !found {
            report.misses.push(format!("{p}\t{id}\t{kind}\t{group}\t{text}\t{seen:?}"));
        }
    }
    // Whatever is left was never there: a wrong name, a misread item, a doubled line.
    for (kind, text) in got {
        report.add(&format!("{p} store/extra"), false, true);
        report.misses.push(format!("{p}\t{id}\textra-{kind}\t{group}\t{text}\t"));
    }
}

#[test]
#[ignore = "needs a folder of rendered scenes (scripts/scenes.py) and the local price snapshot"]
fn scene_bench() {
    let Ok(folder) = std::env::var("MAPLELENS_SCENES") else {
        println!("set MAPLELENS_SCENES to a folder made by scripts/scenes.py");
        return;
    };
    let split = std::env::var("MAPLELENS_SCENE_SPLIT").unwrap_or_else(|_| "eval".into());
    let only_kind = std::env::var("MAPLELENS_SCENE_KIND").ok();
    let only_id: Option<u64> = std::env::var("MAPLELENS_SCENE_ID").ok().and_then(|n| n.parse().ok());
    let limit: usize = std::env::var("MAPLELENS_SCENE_LIMIT").ok().and_then(|n| n.parse().ok()).unwrap_or(usize::MAX);
    let threads: usize = std::env::var("MAPLELENS_SCENE_THREADS").ok().and_then(|n| n.parse().ok()).unwrap_or(8);
    let catalog = Arc::new(refresh::load_catalog(&paths::snapshot_file()).expect("the local price snapshot"));
    let names: BTreeMap<u64, String> =
        catalog.items().filter_map(|it| Some((digits(&it.item_id)?, it.name.clone()))).collect();
    let labels: Vec<Value> = std::fs::read_to_string(format!("{folder}/labels.jsonl"))
        .unwrap()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .filter(|l: &Value| {
            let id = l["id"].as_u64().unwrap_or(0);
            match split.as_str() {
                "eval" => id % 5 == 0,
                "train" => id % 5 != 0,
                _ => true,
            }
        })
        .filter(|l| only_kind.as_deref().is_none_or(|k| l["kind"] == k))
        .filter(|l| only_id.is_none_or(|id| l["id"] == id))
        // The renderer draws Ideal Ratio at 1440 px high at 1x; the game draws it at 2x (tests/fixtures/
        // shop_live_1440.png): those scenes are not what a player sees.
        .filter(|l| {
            !(l["display"]["ui_size"] == "ideal"
                && l["display"]["height"].as_u64().is_some_and(|h| (1440..1536).contains(&h)))
        })
        .take(limit)
        .collect();
    let queue = Mutex::new(labels.iter().collect::<Vec<_>>());
    let report = Mutex::new(Report::default());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let Some(label) = queue.lock().unwrap().pop() else { break };
                    let mut local = Report::default();
                    bench_one(&folder, label, &catalog, &names, &mut local);
                    let mut all = report.lock().unwrap();
                    for (k, t) in local.tallies {
                        let entry = all.tallies.entry(k).or_default();
                        entry.total += t.total;
                        entry.right += t.right;
                        entry.wrong += t.wrong;
                    }
                    all.misses.extend(local.misses);
                }
            });
        }
    });
    let report = report.into_inner().unwrap();
    println!("{} scenes ({split})", labels.len());
    for (key, tally) in &report.tallies {
        println!("{key:<44} {}", tally.line());
    }
    let mut misses = report.misses;
    misses.sort();
    std::fs::create_dir_all("target").ok();
    let out = std::env::var("MAPLELENS_SCENE_OUT").unwrap_or_else(|_| "target/scene_bench.tsv".into());
    std::fs::write(&out, misses.join("\n")).unwrap();
    println!("misses: {out}");
}

fn bench_one(folder: &str, label: &Value, catalog: &Arc<Catalog>, names: &BTreeMap<u64, String>, report: &mut Report) {
    let kind = label["kind"].as_str().unwrap_or_default();
    let d = &label["display"];
    let group = format!("{}x{} {}", d["width"], d["height"], d["ui_size"].as_str().unwrap_or_default());
    for path in [Path::App, Path::Browser] {
        if kind == "store" || kind == "visitor" {
            bench_store(folder, label, catalog, names, path, report, &group);
        }
        let Some(want) = label["item_id"].as_u64() else { continue };
        let Some(image) = frame(folder, label["files"][0].as_str().unwrap()) else { continue };
        let cursor = match path {
            Path::App => label["mouse"]
                .as_object()
                .map(|m| Point { x: m["x"].as_i64().unwrap() as i32, y: m["y"].as_i64().unwrap() as i32 }),
            Path::Browser => None,
        };
        if path == Path::App && cursor.is_none() {
            continue;
        }
        let (got, title) = read_item(catalog, &image, cursor, path);
        let known = names.contains_key(&want);
        let p = format!("{path:?}");
        // An item the price list has only under its plain name ('Red Bennis Chainmail' for the (F) one): the card
        // names that, as the catalog is meant to.
        let plain = maplelens_core::market::catalog::without_gender(label["item"].as_str().unwrap_or_default());
        let right = got == Some(want) || (!known && got.and_then(|g| names.get(&g)).is_some_and(|n| *n == plain));
        let wrong = got.is_some() && !right;
        report.add(&format!("{p} items/{kind}"), right, wrong);
        report.add(&format!("{p} items/all"), right, wrong);
        report.add(&format!("{p} items/all {group}"), right, wrong);
        if !right {
            let name = label["item"].as_str().unwrap_or_default();
            report.misses.push(format!(
                "{p}\t{}\titem-{kind}\t{group}\t{name}{}\t{title}\t{got:?}",
                label["id"],
                if known { "" } else { " (not in catalog)" }
            ));
        }
    }
}
