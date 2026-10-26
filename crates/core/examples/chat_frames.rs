//! The chat page as the watcher sees it on each frame of a folder: lines found, or why none, and whether its scroll
//! thumb shows the live end. `cargo run --release -p maplelens-core --example chat_frames -- FRAMES_DIR`.

use maplelens_core::image::decode_png;
use maplelens_core::shop::layout::{self, View};
use maplelens_core::shop::scrollbar::thumb_position;
use maplelens_core::shop::{chatlog, soldlist};

fn main() {
    let dir = std::env::args().nth(1).expect("a frames folder");
    let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).collect();
    names.sort();
    let mut previous = None;
    for path in names.iter().filter(|p| p.extension().is_some_and(|e| e == "png")) {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let frame = decode_png(&std::fs::read(path).unwrap()).unwrap();
        let Some(located) = layout::locate(&frame, previous.as_ref(), &soldlist::has_list) else {
            println!("{name}  no shop");
            continue;
        };
        previous = Some(located);
        let view = View { frame: &frame, scale: located.scale };
        let enter = located.anchors.enter.expect("an ENTER anchor");
        let (column, track) = layout::chat_scroll(enter);
        let thumb = thumb_position(&view, column, track);
        match chatlog::read_lines(&view, enter) {
            Some(lines) => println!("{name}  {} lines, thumb {thumb:?}", lines.len()),
            None => println!("{name}  chat unreadable"),
        }
    }
}
