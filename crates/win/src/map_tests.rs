//! The map helper's Windows side: the lookup service and its caches (with stand-ins for maplestory.io), and real
//! Windows OCR on the sample world-map tooltips.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use maplelens_core::geometry::{Point, Rect};
use maplelens_core::image::{RgbImage, decode_png};
use maplelens_core::mapinfo::{Location, MapInfo, Mob, Npc};
use maplelens_core::pipeline::Screen;
use maplelens_core::pipeline::map::MapScanner;
use maplelens_core::vision::map_tooltip::{LineOcr, find_tooltip, text_mask};

use crate::maps::cache::{IconCache, MapStore, download_icon};
use crate::maps::service::{MapService, Prefetched, Status};
use crate::net::NetError;
use crate::ocr::CachedLines;

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("maplelens-map-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn png() -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, 20, 20);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&[200, 80, 80, 255].repeat(400)).unwrap();
    }
    out
}

fn info() -> MapInfo {
    MapInfo {
        id: 104040000,
        name: "Henesys Hunting Ground I".into(),
        street: "Victoria Road".into(),
        is_town: false,
        mobs: vec![
            Mob { id: 100101, name: "Blue Snail".into(), level: 2, exp: 4, hp: 15, count: 9 },
            Mob { id: 130101, name: "Red Snail".into(), level: 4, exp: 8, hp: 45, count: 3 },
        ],
        npcs: vec![Npc { id: 1, name: "Anne".into(), role: "Guide".into() }],
    }
}

/// A service over `dir` whose network calls are counted into `calls`.
fn service(dir: &std::path::Path, calls: Arc<Mutex<Vec<String>>>, now: Arc<Mutex<f64>>) -> Arc<MapService> {
    let (c1, c2, c3) = (calls.clone(), calls.clone(), calls);
    MapService::new(
        IconCache::new(dir.join("icons")),
        MapStore::new(dir.join("maps")),
        (
            Box::new(move |location: &Location| {
                c1.lock().unwrap().push(format!("find {}", location.name));
                match location.name.as_str() {
                    "Nowhere" => Ok(None),
                    "Flaky Place" => Err(NetError("offline".into())),
                    _ => Ok(Some(104040000)),
                }
            }),
            Box::new(move |_| {
                c2.lock().unwrap().push("fetch".into());
                Ok(info())
            }),
            Box::new(move |kind: &str, id| {
                c3.lock().unwrap().push(format!("icon {kind} {id}"));
                Ok(png())
            }),
        ),
        Box::new(move || *now.lock().unwrap()),
    )
}

#[test]
fn the_service_finds_the_map_and_downloads_sprites() {
    let dir = temp_dir("find");
    let service = service(&dir, Arc::default(), Arc::default());
    let here = Location::new("Victoria Road", "Henesys Hunting Ground I");
    assert_eq!(service.get(&here).status, Status::Loading, "queued, answered by the worker");
    service.refresh(&here);
    let result = service.get(&here);
    assert_eq!((result.status, result.info.as_deref(), result.icons), (Status::Found, Some(&info()), 3));
    assert!(service.icon("mob", 100101).is_some());
}

#[test]
fn unknown_names_are_remembered_and_failures_retried_later() {
    let dir = temp_dir("retry");
    let now = Arc::new(Mutex::new(0.0));
    let service = service(&dir, Arc::default(), now.clone());
    let (nowhere, flaky) = (Location::new("", "Nowhere"), Location::new("", "Flaky Place"));
    service.refresh(&nowhere);
    service.refresh(&flaky);
    assert_eq!(service.get(&nowhere).status, Status::Missing);
    assert_eq!(service.get(&flaky).status, Status::Failed);
    assert_eq!(service.queued(), 0, "neither is looked up again right away");
    *now.lock().unwrap() = 1e9;
    service.get(&flaky);
    assert_eq!(service.queued(), 1);
}

#[test]
fn a_restart_answers_from_the_disk_cache_without_the_network() {
    let dir = temp_dir("restart");
    let calls: Arc<Mutex<Vec<String>>> = Arc::default();
    let here = Location::new("Victoria Road", "Henesys Hunting Ground I");
    for _ in 0..2 {
        let service = service(&dir, calls.clone(), Arc::default());
        service.refresh(&here);
        assert_eq!(service.get(&here).info.as_deref(), Some(&info()));
    }
    let network: Vec<String> = calls.lock().unwrap().iter().filter(|c| !c.starts_with("icon")).cloned().collect();
    assert_eq!(network, ["find Henesys Hunting Ground I", "fetch"]);
    let icons = calls.lock().unwrap().iter().filter(|c| c.starts_with("icon")).count();
    assert_eq!(icons, 3, "sprites are downloaded once");
    // Look-alike glyphs find the same cached map.
    assert_eq!(
        MapStore::new(dir.join("maps")).find(&Location::new("Victoria Road", "Henesys Hunting Ground l")),
        Some(104040000)
    );
}

#[test]
fn a_failed_or_broken_sprite_download_leaves_nothing_behind() {
    let dir = temp_dir("icons");
    let tries = AtomicUsize::new(0);
    let offline = |_: &str, _: u32| {
        tries.fetch_add(1, Ordering::Relaxed);
        Err(NetError("offline".into()))
    };
    assert!(!download_icon(&dir, "npc", 1, offline));
    assert!(!download_icon(&dir, "npc", 1, |_, _| Ok(b"<html>not a png</html>".to_vec())));
    assert!(IconCache::new(dir.clone()).get("npc", 1).is_none());
    assert!(download_icon(&dir, "npc", 1, |_, _| Ok(png())));
    assert_eq!(IconCache::new(dir).get("npc", 1).map(|i| (i.width, i.height)), Some((20, 20)));
}

fn fixture(name: &str) -> RgbImage {
    let path = format!("{}/../../tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    decode_png(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn windows_ocr_reads_the_sample_tooltips() {
    if !crate::ocr::available() {
        return;
    }
    let mut ocr = CachedLines::new();
    for (name, (x, y, w, h), title, street) in [
        ("map_tooltip_town.png", (150, 150, 400, 150), "Henesys", "Victoria Road"),
        ("map_tooltip_hunting_ground.png", (100, 105, 500, 60), "Hunting Ground", ""),
    ] {
        let image = fixture(name).crop(x, y, x + w, y + h);
        let lines = ocr.read_lines(&text_mask(&image));
        let found = find_tooltip(&lines).unwrap_or_else(|| panic!("{name}: no title in {lines:?}"));
        assert!(found.location.name.contains(title) && found.location.street == street, "{name}: {found:?}");
        assert!(found.rect.w > 300, "{name}: {:?}", found.rect);
    }
}

struct Picture {
    image: RgbImage,
    cursor: Point,
}

impl Screen for Picture {
    fn grab(&mut self, area: Rect) -> RgbImage {
        let clip = |v: i32| v.max(0) as usize;
        self.image.crop(clip(area.x), clip(area.y), clip(area.right()), clip(area.bottom()))
    }

    fn cursor(&mut self) -> Option<Point> {
        Some(self.cursor)
    }
}

#[test]
#[cfg_attr(not(fixtures), ignore = "needs the local captures in tests/fixtures")]
fn the_scanner_finds_the_town_tooltip_next_to_the_cursor() {
    if !crate::ocr::available() {
        return;
    }
    let sample = fixture("map_tooltip_town.png");
    let mut image = RgbImage::new(1920, 1200); // the reference resolution, so the scan runs unscaled
    for y in 0..sample.height {
        for x in 0..sample.width {
            image.set_pixel(x, y, sample.pixel(x, y));
        }
    }
    let mut screen = Picture { image, cursor: Point { x: 127, y: 128 } }; // the fingertip of the hand
    let mut scanner = MapScanner::new(Box::new(CachedLines::new()));
    let bounds = Rect::new(0, 0, 1920, 1200);
    let first = scanner.step(&mut screen, bounds, 1.0, 0.0);
    scanner.step(&mut screen, bounds, 1.0, 0.1);
    let hover = scanner.step(&mut screen, bounds, 1.0, 0.2).expect("a map tooltip");
    assert!(first.is_none(), "a reading must repeat before a card may show");
    assert_eq!(hover.location, Location::new("Victoria Road", "Henesys"));
    // Right of and below the cursor; OCR noise in the blurry sample widens the box to the left.
    assert!(5 < hover.offset.0 && hover.offset.0 < 45 && 20 < hover.offset.1 && hover.offset.1 < 50, "{hover:?}");
    assert!(hover.size.0 > 300);
}

/// Live check against maplestory.io (`cargo test -p maplelens maplestory_io -- --ignored`).
#[test]
#[ignore = "needs the network"]
fn maplestory_io_lookup() {
    let here = Location::new("Victoria Road", "Henesys Hunting Ground I");
    let id = crate::maps::client::find_map_id(&here).unwrap().expect("a map");
    let map = crate::maps::client::fetch_map(id).unwrap();
    println!(
        "{id}: {} ({} kinds of monsters, {} spawns, {} NPCs)",
        map.name,
        map.mobs.len(),
        map.spawns(),
        map.npcs.len()
    );
    assert!(map.name.contains("Hunting Ground") && !map.mobs.is_empty());
    assert!(!crate::maps::client::fetch_icon("mob", map.mobs[0].id).unwrap().is_empty());
}

fn candidate(id: u32, street: &str, name: &str) -> maplelens_core::mapinfo::resolve::Candidate {
    maplelens_core::mapinfo::resolve::Candidate { id, name: name.into(), street: street.into() }
}

#[test]
fn the_map_list_resolves_a_reading_without_a_search_request() {
    let dir = temp_dir("catalog");
    let calls: Arc<Mutex<Vec<String>>> = Arc::default();
    let service = service(&dir, calls.clone(), Arc::default());
    service.set_catalog(vec![
        candidate(104040000, "Victoria Road", "Henesys Hunting Ground I"),
        candidate(104040001, "Victoria Road", "Henesys Hunting Ground II"),
    ]);
    let here = Location::new("Victoria Road", "Henesys Hunting Ground I");
    service.refresh(&here);
    assert_eq!(service.get(&here).status, Status::Found);
    assert!(calls.lock().unwrap().iter().all(|c| !c.starts_with("find")), "{:?}", calls.lock().unwrap());
    // The answer is remembered for the next run, and a reading the list cannot settle still searches.
    assert_eq!(MapStore::new(dir.join("maps")).find(&here), Some(104040000));
    service.refresh(&Location::new("", "Somewhere Else Entirely"));
    assert!(calls.lock().unwrap().iter().any(|c| c.starts_with("find")));
}

#[test]
fn the_prefetch_fills_the_disk_so_a_hover_needs_no_network() {
    let dir = temp_dir("prefetch");
    let calls: Arc<Mutex<Vec<String>>> = Arc::default();
    let service = service(&dir, calls.clone(), Arc::default());
    assert_eq!(service.prefetch(104040000), Prefetched::Done);
    assert!(service.store().has(104040000));
    assert!(service.icon("npc", 1).is_some() && service.icon("mob", 130101).is_some());
    let before = calls.lock().unwrap().len();
    assert_eq!(service.prefetch(104040000), Prefetched::Done, "a second pass is a no-op");
    assert_eq!(calls.lock().unwrap().len(), before);
}

#[test]
fn only_the_wanted_regions_and_missing_maps_are_prefetched() {
    use crate::maps::prefetch::wanted;
    let list = [
        candidate(100000000, "Victoria Road", "Henesys"),
        candidate(100000100, "Victoria Road", "Henesys Market"),
        candidate(200000000, "Orbis", "Orbis"),
        candidate(100000000, "Victoria Road", "Henesys"),
    ];
    assert_eq!(wanted(&list, &[10], |_| false), [100000000, 100000100]);
    assert_eq!(wanted(&list, &[10], |id| id == 100000000), [100000100]);
    assert_eq!(wanted(&list, &[], |_| false), Vec::<u32>::new());
}

#[test]
fn in_parallel_keeps_order_and_runs_all() {
    let items: Vec<u32> = (0..50).collect();
    let out = crate::maps::client::in_parallel(&items, |&n| {
        std::thread::sleep(std::time::Duration::from_millis(u64::from(50 - n) % 3));
        n * 2
    });
    assert_eq!(out, items.iter().map(|n| n * 2).collect::<Vec<_>>());
}

/// Every classic map named exactly resolves to itself against the full list (live: `-- --ignored`), except maps
/// whose street and name are exactly another's (a hundred-odd copies of event and instance maps): the tooltip
/// cannot tell those apart, so they rightly show nothing.
#[test]
#[ignore = "needs the network"]
fn live_every_map_resolves_to_itself() {
    use maplelens_core::mapinfo::resolve::pick;
    let catalog = crate::maps::client::fetch_catalog().unwrap();
    println!("{} maps", catalog.len());
    let (mut wrong, mut none) = (Vec::new(), 0);
    let twins = |c: &maplelens_core::mapinfo::resolve::Candidate| {
        catalog.iter().filter(|o| (&o.street, &o.name) == (&c.street, &c.name)).count() > 1
    };
    for c in catalog.iter().filter(|c| !twins(c)) {
        match pick(&Location::new(&c.street, &c.name), &catalog) {
            Some(id) if id == c.id => {}
            Some(id) => wrong.push(format!("{} : {} -> {id} (is {})", c.street, c.name, c.id)),
            None => none += 1,
        }
    }
    println!("{} wrong, {none} unresolved", wrong.len());
    for line in wrong.iter().take(20) {
        println!("  {line}");
    }
    assert!(wrong.is_empty(), "a wrong map is worse than none");
    assert!(none * 100 <= catalog.len() * 3, "{none} unresolved");
}

/// How long a cold map takes (data and sprites) and how long all of Victoria Island takes to download (live).
#[test]
#[ignore = "needs the network"]
fn live_prefetch_speed() {
    let dir = temp_dir("speed");
    let service = MapService::new(
        IconCache::new(dir.join("icons")),
        MapStore::new(dir.join("maps")),
        (
            Box::new(crate::maps::client::find_map_id),
            Box::new(crate::maps::client::fetch_map),
            Box::new(crate::maps::client::fetch_icon),
        ),
        Box::new(|| 0.0),
    );
    let catalog = crate::maps::client::fetch_catalog().unwrap();
    let ids = crate::maps::prefetch::wanted(&catalog, &[10], |_| false);
    println!("{} Victoria Island maps", ids.len());
    let start = std::time::Instant::now();
    assert_eq!(service.prefetch(ids[0]), Prefetched::Done);
    println!("first map cold: {:.2}s", start.elapsed().as_secs_f64());
    let start = std::time::Instant::now();
    let mut counts = [0; 3];
    for id in &ids[1..] {
        counts[service.prefetch(*id) as usize] += 1;
    }
    println!(
        "the other {} maps: {} done, {} not served, {} failed, in {:.1}s",
        ids.len() - 1,
        counts[0],
        counts[1],
        counts[2],
        start.elapsed().as_secs_f64()
    );
}
