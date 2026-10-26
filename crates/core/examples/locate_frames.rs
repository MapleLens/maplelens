//! Where the shop is found on each frame of a folder, followed from look to look as the watcher does: scale, marker
//! and Sold Items box. A held layout that still moves forgets the lists. `cargo run --release -p maplelens-core
//! --example locate_frames -- FRAMES_DIR`.

use maplelens_core::image::decode_png;
use maplelens_core::shop::{layout, soldlist};

fn main() {
    let dir = std::env::args().nth(1).expect("a frames folder");
    let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).collect();
    names.sort();
    let mut previous = None;
    for path in names.iter().filter(|p| p.extension().is_some_and(|e| e == "png")) {
        let frame = decode_png(&std::fs::read(path).unwrap()).unwrap();
        let located = layout::locate(&frame, previous.as_ref(), &soldlist::has_list);
        match &located {
            Some(l) => println!(
                "{}  scale {:.4} (marker {:.4}) at ({}, {})  sold box {:?}",
                path.file_name().unwrap().to_string_lossy(),
                l.scale,
                l.marker.scale,
                l.marker.x,
                l.marker.y,
                l.sold_box
            ),
            None => println!("{}  not found", path.file_name().unwrap().to_string_lossy()),
        }
        previous = located.or(previous);
    }
}
