//! Detection on labelled synthetic scenes drawn by the game's own engine (`scripts/scenes.py`):
//! `MAPLELENS_SCENES=<folder> [MAPLELENS_SCENE_SPLIT=eval|train|all] [MAPLELENS_SCENE_KIND=store]
//! [MAPLELENS_SCENE_LIMIT=N] cargo test --release -p maplelens scene_bench -- --ignored --nocapture`.
//!
//! Every scene is read three ways: the app's path (the mouse where the scene put it, Windows OCR behind the glyphs),
//! the browser's (no mouse, the frame as video gives it (`as_video`), Tesseract behind the fonts: the page's tesseract.js 7.0.0 and English data, served by a
//! local Node process at `MAPLELENS_TESSERACT`, e.g. `http://127.0.0.1:8799`, answering at once where the page
//! answers a look or two later) and the browser's floor (no mouse, the bundled fonts alone). `MAPLELENS_SCENE_PATHS`
//! picks some of `app,browser,floor`; the browser's is skipped when no Tesseract is given. Items count as right only when the catalog item's id is the scene's; stores by their events
//! (visitors, chat, sales) and the owner's name, each name compared exactly. Misses go to target\scene_bench.tsv.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use maplelens_core::geometry::{Point, Rect};
use maplelens_core::image::{GrayImage, RgbImage, decode_png, encode_png};
use maplelens_core::market::catalog::Catalog;
use maplelens_core::pipeline::FrameScreen;
use maplelens_core::pipeline::hover::HoverDetector;
use maplelens_core::shop::watcher::{ReadText, ShopWatcher};
use maplelens_core::shop::{ShopEvent, ShopStatus};
use maplelens_core::vision::Mask;
use maplelens_core::vision::reader::{NoOcr, Ocr, OcrAnswer, TextReader, ocr_image, title_atlas};
use serde_json::Value;

use crate::{ocr, paths, refresh, shop};

/// Looks at one frame, a second apart: the UI scale settles on three readings, a title on two alike.
const LOOKS: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Path {
    App,
    Browser,
    Floor,
}

/// The page's Tesseract, asked synchronously: `psm` 6 for titles (a block of one or two lines), 7 for a shop line.
fn tesseract(image: &GrayImage, psm: u8) -> String {
    let Ok(server) = std::env::var("MAPLELENS_TESSERACT") else { return String::new() };
    let mut rgb = RgbImage::new(image.width, image.height);
    for (p, &v) in rgb.data.as_chunks_mut::<3>().0.iter_mut().zip(&image.data) {
        p.copy_from_slice(&[v, v, v]);
    }
    let agent = crate::net::agent(60);
    match agent.post(format!("{server}/ocr?psm={psm}")).send(&encode_png(&rgb)[..]) {
        Ok(mut response) => response.body_mut().read_to_string().unwrap_or_default(),
        Err(_) => String::new(),
    }
}

struct Tesseract;

impl Ocr for Tesseract {
    fn recognize(&mut self, mask: &Mask) -> OcrAnswer {
        // The page's title queue does not thicken (crates/web/src/lib.rs).
        OcrAnswer::Text(tesseract(&ocr_image(mask, false), 6))
    }
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

/// The frame as a browser's shared window gives it: video, its colour kept at half resolution (4:2:0), brightness in
/// full. Small coloured marks (a title's bullet, a button's edge) blur by a pixel; text, mostly brightness, does not.
fn as_video(image: &RgbImage) -> RgbImage {
    let (w, h) = (image.width, image.height);
    let ycc = |p: [u8; 3]| {
        let [r, g, b] = p.map(f32::from);
        let y = 0.299 * r + 0.587 * g + 0.114 * b;
        (y, b - y, r - y)
    };
    let mut out = image.clone();
    for by in (0..h).step_by(2) {
        for bx in (0..w).step_by(2) {
            let block: Vec<(usize, usize)> = [(0, 0), (1, 0), (0, 1), (1, 1)]
                .iter()
                .map(|&(dx, dy)| (bx + dx, by + dy))
                .filter(|&(x, y)| x < w && y < h)
                .collect();
            let (mut cb, mut cr) = (0.0, 0.0);
            for &(x, y) in &block {
                let (_, b, r) = ycc(image.pixel(x, y));
                (cb, cr) = (cb + b, cr + r);
            }
            let n = block.len() as f32;
            let (cb, cr) = (cb / n, cr / n);
            for &(x, y) in &block {
                let (luma, _, _) = ycc(image.pixel(x, y));
                let (r, b) = (luma + cr, luma + cb);
                let g = (luma - 0.299 * r - 0.114 * b) / 0.587;
                let i = (y * w + x) * 3;
                out.data[i..i + 3].copy_from_slice(&[r, g, b].map(|v| v.round().clamp(0.0, 255.0) as u8));
            }
        }
    }
    out
}

fn reader(path: Path) -> TextReader {
    match path {
        Path::App => TextReader::new(title_atlas(), Box::new(ocr::WindowsOcr::new(false).expect("an OCR language"))),
        Path::Browser => TextReader::new(title_atlas(), Box::new(Tesseract)),
        Path::Floor => TextReader::new(title_atlas(), Box::new(NoOcr)),
    }
}

/// The item a fresh detector names after a few looks at `image`, and the title it read.
fn read_item(
    catalog: &Arc<Catalog>,
    image: &RgbImage,
    cursor: Option<Point>,
    path: Path,
    settled: Option<f32>,
) -> (Option<u64>, String, f32) {
    let mut hover = HoverDetector::new(catalog.clone(), reader(path));
    let bounds = Rect::new(0, 0, image.width as i32, image.height as i32);
    // A running app has measured the UI size off the status bar for seconds before an item is hovered (a tooltip
    // over the bar then changes nothing): the vote starts settled, as it would be.
    if let Some(scale) = settled {
        // Oldest first: a reading older than the last by more than a second is a clock gone back.
        let n = maplelens_core::config::SCALE_VOTE_MIN_READINGS;
        for k in 0..n {
            hover.ui_scale().set(bounds, scale, (k as f64) - n as f64);
        }
    }
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
    let measured = hover.ui_scale().of(bounds);
    match last {
        Some(h) => (h.item.as_ref().and_then(|it| digits(&it.item_id)), h.title, measured),
        None => (None, String::new(), measured),
    }
}

fn shop_reader(path: Path) -> ReadText {
    match path {
        Path::App => shop::reader(),
        // The fonts read what they know; nothing fills their gaps.
        Path::Browser => Box::new(|mask: &Mask| Some(tesseract(&ocr_image(mask, false), 7))),
        Path::Floor => Box::new(|_| Some(String::new())),
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
        && catalog.items().filter(|it| key(&it.name).starts_with(&key(g[0]))).count() > 1
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
    let frames: Vec<RgbImage> = files
        .iter()
        .filter_map(|f| frame(folder, f))
        .map(|f| if path == Path::App { f } else { as_video(&f) })
        .collect();
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
    if !watching {
        report.misses.push(format!("{p}	{id}	found	{group}	opened {opened}	{:?}", watcher.status()));
    }
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
                "eval" => id.is_multiple_of(5),
                "train" => !id.is_multiple_of(5),
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
        // Smaller windows hide part of the store, and the renderer draws the minimap over it at 1024x768.
        .filter(|l| l["display"]["width"].as_u64().is_some_and(|w| w >= MIN_WIDTH))
        .filter(|l| readable_items(l, &catalog))
        .filter(|l| !minimap_over_store(l))
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

/// The status bar's height at UI scale 1, with its outline, and how far below the cursor an item's tooltip usually
/// reaches: a cursor closer to the bottom than both may have the tooltip over the bar.
const BAR_HEIGHT: f64 = 110.0;
const TYPICAL_TOOLTIP: f64 = 290.0;

/// The smallest window scored.
const MIN_WIDTH: u64 = 1366;

/// Does every item the scene names have a name of its own in the catalog? One it lacks ("Rib"), or one the game titles
/// like another (the (M) and (F) vests), cannot be told by any reading: `scripts/scenes.py` no longer draws them.
fn readable_items(label: &Value, catalog: &Catalog) -> bool {
    let unique = |name: &str| {
        let name = name.trim().to_lowercase();
        catalog.items().filter(|it| it.name.trim().to_lowercase() == name).count() == 1
    };
    let item = label["item"].as_str().is_none_or(unique);
    let sales = label["events"].as_array().into_iter().flatten().filter(|e| e[0] == "sale");
    item && sales.into_iter().all(|e| e[1].as_str().is_some_and(unique))
}

/// The renderer draws the minimap over a store window opened in the top-left corner, where the game draws the store
/// (the window opened last) on top. The owner's name plate that close to the corner (UI px) is such a store.
fn minimap_over_store(label: &Value) -> bool {
    let d = &label["display"];
    let (w, h) = (d["width"].as_f64().unwrap_or(0.0), d["height"].as_f64().unwrap_or(0.0));
    let ui_px = if w < 1920.0 || h < 1080.0 || d["ui_size"] == "ideal" { 1.0 } else { h / 768.0 };
    let Some(owner) = label["owner"].as_str() else { return false };
    let plate = label["texts"][0].as_array().into_iter().flatten().find(|t| t[0] == owner);
    plate
        .and_then(|t| Some((t[1][0].as_f64()?, t[1][1].as_f64()?)))
        .is_some_and(|(x, y)| x < MINIMAP_CORNER.0 * ui_px && y < MINIMAP_CORNER.1 * ui_px)
}

/// How far from the top-left corner (UI px) an owner's plate puts its store window under the minimap.
const MINIMAP_CORNER: (f64, f64) = (120.0, 260.0);

fn paths() -> Vec<Path> {
    let chosen = std::env::var("MAPLELENS_SCENE_PATHS").unwrap_or_else(|_| "app,browser,floor".into());
    let tesseract = std::env::var("MAPLELENS_TESSERACT").is_ok();
    [("app", Path::App), ("browser", Path::Browser), ("floor", Path::Floor)]
        .into_iter()
        .filter(|(name, path)| chosen.contains(name) && (*path != Path::Browser || tesseract))
        .map(|(_, path)| path)
        .collect()
}

/// The UI scale the game draws a scene's window at: the bitmap UI (0.641) below 1080p and at Ideal Ratio, else the
/// client height over 1200.
fn expected_scale(d: &Value) -> f32 {
    let (w, h) = (d["width"].as_u64().unwrap_or(0), d["height"].as_u64().unwrap_or(0));
    if w < 1920 || h < 1080 || d["ui_size"] == "ideal" { 0.641 } else { h as f32 / 1200.0 }
}

fn bench_one(folder: &str, label: &Value, catalog: &Arc<Catalog>, names: &BTreeMap<u64, String>, report: &mut Report) {
    let kind = label["kind"].as_str().unwrap_or_default();
    let d = &label["display"];
    let group = format!("{}x{} {}", d["width"], d["height"], d["ui_size"].as_str().unwrap_or_default());
    for path in paths() {
        if kind == "store" || kind == "visitor" {
            bench_store(folder, label, catalog, names, path, report, &group);
        }
        let Some(want) = label["item_id"].as_u64() else { continue };
        let Some(image) = frame(folder, label["files"][0].as_str().unwrap()) else { continue };
        let image = if path == Path::App { image } else { as_video(&image) };
        let cursor = match path {
            Path::App => label["mouse"]
                .as_object()
                .map(|m| Point { x: m["x"].as_i64().unwrap() as i32, y: m["y"].as_i64().unwrap() as i32 }),
            Path::Browser | Path::Floor => None,
        };
        if path == Path::App && cursor.is_none() {
            continue;
        }
        let expected = expected_scale(d);
        // The bar itself, from this frame alone (where no tooltip covers it).
        let (_, _, measured) = read_item(catalog, &image, cursor, path, None);
        let (got, title, _) = read_item(catalog, &image, cursor, path, Some(expected));
        let scale_ok = (measured - expected).abs() < 0.02;
        // A tooltip near the bottom covers the status bar in this one frame; a running app read the bar before it
        // opened (the scale is voted over seconds), so such a frame says nothing about measuring the scale.
        let bar_covered = label["mouse"]["y"].as_f64().is_some_and(|y| {
            y + (TYPICAL_TOOLTIP + BAR_HEIGHT) * expected as f64 >= d["height"].as_f64().unwrap_or(0.0)
        });
        if !bar_covered {
            report.add(&format!("{path:?} ui-scale"), scale_ok, !scale_ok);
        }
        if !scale_ok && !bar_covered {
            report.misses.push(format!("{path:?}	{}	scale	{group}	{expected:.3}	{measured:.3}", label["id"]));
        }
        let known = names.contains_key(&want);
        let p = format!("{path:?}");
        // An item the price list has only under its plain name ('Red Bennis Chainmail' for the (F) one): the card
        // names that, as the catalog is meant to.
        let plain = maplelens_core::market::catalog::without_gender(label["item"].as_str().unwrap_or_default());
        // Two items of one name (the price list's 'Blue Cloth Vest' twice) cannot be told apart by any reading.
        let same_name = known && got.and_then(|g| names.get(&g)) == names.get(&want);
        let right =
            got == Some(want) || same_name || (!known && got.and_then(|g| names.get(&g)).is_some_and(|n| *n == plain));
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
