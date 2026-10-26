//! Proof of concept, step 2: the glyph matcher on the lines `shop_poc::poc_extract` wrote, against Windows OCR.
//!
//! `MAPLELENS_POC_OUT=<folder with lines.json and labels.json> cargo test --release -p maplelens poc_atlas -- --ignored
//! --nocapture`. The atlas is learnt from hand-labelled lines and tested on others (5-fold cross-validation grouped by
//! the labelled text, so a name is never in both); "bootstrapped" adds lines Windows OCR read in a shape we know, but
//! never one naming a held-out player.

use std::collections::HashMap;
use std::time::Instant;

use maplelens_core::vision::Mask;
use maplelens_core::vision::glyphs::Atlas;
use serde_json::{Value, json};

use crate::ocr;
use crate::shop_poc::padded;

struct PocLine {
    id: usize,
    kind: String,
    windows: String,
    windows_ms: f64,
    /// What Windows OCR is given (the watcher's mask, threshold 150).
    ocr_mask: Mask,
    /// What the atlas reads: the line's grey window thresholded at `MAPLELENS_POC_THRESHOLD` (default 150).
    mask: Mask,
}

/// Ink where the grey is darker than `level`, trimmed to the ink.
fn threshold(gray: &[String], level: u8) -> Mask {
    let width = gray.iter().map(|r| r.len() / 2).max().unwrap_or(0);
    let mut mask = Mask::new(width, gray.len());
    for (y, row) in gray.iter().enumerate() {
        for x in 0..row.len() / 2 {
            mask.set(x, y, u8::from_str_radix(&row[2 * x..2 * x + 2], 16).unwrap_or(255) < level);
        }
    }
    let rows: Vec<usize> = (0..mask.height).filter(|&y| (0..mask.width).any(|x| mask.get(x, y))).collect();
    let cols: Vec<usize> = (0..mask.width).filter(|&x| (0..mask.height).any(|y| mask.get(x, y))).collect();
    match (rows.first(), rows.last(), cols.first(), cols.last()) {
        (Some(&y0), Some(&y1), Some(&x0), Some(&x1)) => mask.crop(x0, y0, x1 + 1, y1 + 1),
        _ => Mask::new(0, 0),
    }
}

fn mask_of(rows: &[String]) -> Mask {
    let width = rows.iter().map(String::len).max().unwrap_or(0);
    let mut mask = Mask::new(width, rows.len());
    for (y, row) in rows.iter().enumerate() {
        for (x, c) in row.chars().enumerate() {
            mask.set(x, y, c == '#');
        }
    }
    mask
}

/// What a player would not tell apart in a name: I / l, and O / 0.
fn fold(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'I' | 'l' | '|' => 'l',
            '0' => 'O',
            c => c,
        })
        .collect()
}

fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, &x) in a.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, &y) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (diagonal + usize::from(x != y)).min(row[j] + 1).min(above + 1);
            diagonal = above;
        }
    }
    row[b.len()]
}

/// The player's name in a chat line, and whether it is an entrance or a leaving.
fn chat_event(text: &str) -> Option<(&'static str, String)> {
    for (kind, tail) in [("entered", " has entered."), ("left", " has left the room.")] {
        if let Some(name) = text.strip_suffix(tail) {
            return Some((kind, name.to_string()));
        }
    }
    None
}

fn name_of(text: &str) -> Option<String> {
    chat_event(text).map(|(_, n)| fold(&n)).or_else(|| text.strip_prefix("Buyer: ").map(fold))
}

fn process_cpu_ms() -> f64 {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let (mut creation, mut exit, mut kernel, mut user) =
        (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: out-parameters are valid locals; the pseudo handle of the current process needs no closing.
    unsafe {
        let _ = GetProcessTimes(GetCurrentProcess(), &mut creation, &mut exit, &mut kernel, &mut user);
    }
    let ticks = |f: FILETIME| ((u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime)) as f64;
    (ticks(kernel) + ticks(user)) / 10_000.0
}

#[derive(Default)]
struct Score {
    lines: usize,
    answered: usize,
    exact: usize,
    exact_answered: usize,
    edits: usize,
    chars: usize,
    unseen_lines: usize,
    unseen_exact: usize,
}

impl Score {
    fn add(&mut self, truth: &str, read: &str, unseen: bool) {
        self.lines += 1;
        let answered = !read.contains('?') && !read.is_empty();
        let exact = fold(read) == fold(truth);
        self.answered += usize::from(answered);
        self.exact += usize::from(exact);
        self.exact_answered += usize::from(exact && answered);
        self.edits += edit_distance(&fold(read), &fold(truth));
        self.chars += truth.chars().count();
        if unseen {
            self.unseen_lines += 1;
            self.unseen_exact += usize::from(exact);
        }
    }

    fn report(&self, name: &str) -> String {
        let pct = |a: usize, b: usize| 100.0 * a as f64 / b.max(1) as f64;
        format!(
            "{name:<36} exact {:>5.1}%  precision {:>5.1}%  recall {:>5.1}%  abstained {:>4.1}%  CER {:>5.2}%  unseen-name exact {:>5.1}%",
            pct(self.exact, self.lines),
            pct(self.exact_answered, self.answered),
            pct(self.exact_answered, self.lines),
            pct(self.lines - self.answered, self.lines),
            pct(self.edits, self.chars),
            pct(self.unseen_exact, self.unseen_lines),
        )
    }
}

/// A line Windows OCR read in a shape we know: a visitor's line, or a sold row's buyer or quantity.
fn confident(text: &str) -> bool {
    chat_event(text).is_some_and(|(_, name)| name.chars().count() >= 3 && !name.contains(' '))
        || text.starts_with("Buyer: ")
        || text.starts_with("Number purchased: ")
}

#[test]
#[ignore = "needs the output of poc_extract and labels.json in MAPLELENS_POC_OUT"]
fn poc_atlas() {
    let Ok(out) = std::env::var("MAPLELENS_POC_OUT") else { return };
    let raw: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(format!("{out}/lines.json")).unwrap()).unwrap();
    let lines: Vec<PocLine> = raw
        .iter()
        .map(|e| PocLine {
            id: e["id"].as_u64().unwrap() as usize,
            kind: e["kind"].as_str().unwrap().to_string(),
            windows: e["windows_ocr"].as_str().unwrap().to_string(),
            windows_ms: e["windows_ms"].as_f64().unwrap(),
            ocr_mask: mask_of(
                &e["rows"].as_array().unwrap().iter().map(|r| r.as_str().unwrap().to_string()).collect::<Vec<_>>(),
            ),
            mask: threshold(
                &e["gray"].as_array().unwrap().iter().map(|r| r.as_str().unwrap().to_string()).collect::<Vec<_>>(),
                std::env::var("MAPLELENS_POC_THRESHOLD").ok().and_then(|v| v.parse().ok()).unwrap_or(150),
            ),
        })
        .collect();
    let labels: HashMap<usize, String> = serde_json::from_str::<HashMap<String, String>>(
        &std::fs::read_to_string(format!("{out}/labels.json")).unwrap(),
    )
    .unwrap()
    .into_iter()
    .map(|(k, v)| (k.parse().unwrap(), v))
    .collect();
    let labelled: Vec<&PocLine> = lines.iter().filter(|l| labels.contains_key(&l.id)).collect();

    // Windows OCR re-reading the labelled lines (no cache): wall and process CPU time.
    let engine = ocr::WindowsOcr::new(false).expect("Windows OCR");
    let _ = engine.recognize_gray(&ocr::upscaled(&padded(&labelled[0].ocr_mask)));
    let (cpu0, wall0) = (process_cpu_ms(), Instant::now());
    let windows: HashMap<usize, String> = labelled
        .iter()
        .map(|l| {
            let text = engine.recognize_gray(&ocr::upscaled(&padded(&l.ocr_mask))).map(|t| t.trim().to_string());
            (l.id, text.unwrap_or_default())
        })
        .collect();
    let (win_wall, win_cpu) = (wall0.elapsed().as_secs_f64() * 1000.0, process_cpu_ms() - cpu0);

    // The atlas's geometry (rows above and below the baseline, the width of a space) is picked on the labelled lines.
    let mut best = (0usize, (11usize, 4usize, 4usize));
    for above in 9..=14 {
        for below in 2..=5 {
            for gap in 3..=6 {
                let mut atlas = Atlas::empty(above, below, gap);
                for l in &labelled {
                    atlas.learn(&padded(&l.mask), &labels[&l.id]);
                }
                let right = labelled
                    .iter()
                    .filter(|l| fold(&atlas.read(&padded(&l.mask)).text) == fold(&labels[&l.id]))
                    .count();
                if right > best.0 {
                    best = (right, (above, below, gap));
                }
            }
        }
    }
    let (above, below, gap) = best.1;
    eprintln!("atlas geometry above {above} below {below} gap {gap}: {} of {} lines read back", best.0, labelled.len());

    let mut texts: Vec<&str> = labelled.iter().map(|l| labels[&l.id].as_str()).collect();
    texts.sort_unstable();
    texts.dedup();
    let fold_of = |text: &str| texts.iter().position(|t| *t == text).unwrap() % 5;
    let mut scores: Vec<(&str, Score)> =
        ["Windows OCR (current)", "Atlas, supervised only", "Atlas, bootstrapped", "Atlas + Windows OCR for the gaps"]
            .into_iter()
            .map(|n| (n, Score::default()))
            .collect();
    let mut events = [(0usize, 0usize, 0usize); 3];
    let (mut atlas_ms, mut atlas_reads) = (0.0, 0usize);
    let mut results = Vec::new();
    let mut trained = (0usize, 0usize);
    for k in 0..5 {
        let test: Vec<&PocLine> = labelled.iter().copied().filter(|l| fold_of(&labels[&l.id]) == k).collect();
        let test_names: Vec<String> = test.iter().filter_map(|l| name_of(&labels[&l.id])).collect();
        let train: Vec<&PocLine> = labelled.iter().copied().filter(|l| fold_of(&labels[&l.id]) != k).collect();
        let mut supervised = Atlas::empty(above, below, gap);
        for l in &train {
            supervised.learn(&padded(&l.mask), &labels[&l.id]);
        }
        let mut boot = supervised.clone();
        let held_out: Vec<usize> = test.iter().map(|l| l.id).collect();
        for l in lines.iter().filter(|l| !labels.contains_key(&l.id) && !held_out.contains(&l.id)) {
            if !confident(&l.windows) {
                continue;
            }
            // Never from a line naming a held-out player: those names must stay unseen.
            if name_of(&l.windows).is_some_and(|n| test_names.iter().any(|t| edit_distance(t, &n) <= 2)) {
                continue;
            }
            boot.learn(&padded(&l.mask), &l.windows);
        }
        trained = (supervised.glyphs.len(), boot.glyphs.len());
        for l in &test {
            let truth = labels[&l.id].as_str();
            let unseen = name_of(truth)
                .is_some_and(|n| !train.iter().any(|t| name_of(&labels[&t.id]).as_deref() == Some(n.as_str())));
            let start = Instant::now();
            let s = supervised.read(&padded(&l.mask)).text;
            atlas_ms += start.elapsed().as_secs_f64() * 1000.0;
            atlas_reads += 1;
            let b = boot.read(&padded(&l.mask)).text;
            let hybrid = if b.contains('?') { windows[&l.id].clone() } else { b.clone() };
            for (slot, read) in [(0, &windows[&l.id]), (1, &s), (2, &b), (3, &hybrid)] {
                scores[slot].1.add(truth, read, unseen);
            }
            for (slot, read) in [(0, &windows[&l.id]), (1, &s), (2, &b)] {
                let (reported, wanted) = (chat_event(read), chat_event(truth));
                events[slot].0 += usize::from(reported.is_some());
                events[slot].2 += usize::from(wanted.is_some());
                if let (Some(r), Some(w)) = (&reported, &wanted) {
                    events[slot].1 += usize::from(r.0 == w.0 && fold(&r.1) == fold(&w.1));
                }
            }
            results.push(json!({
                "id": l.id, "kind": l.kind, "truth": truth, "windows": windows[&l.id],
                "atlas_supervised": s, "atlas_bootstrapped": b, "unseen": unseen,
            }));
        }
    }
    std::fs::write(format!("{out}/results.json"), serde_json::to_string_pretty(&results).unwrap()).unwrap();
    let n = labelled.len() as f64;
    eprintln!(
        "\n{} labelled lines, 5-fold cross-validation grouped by text; last fold's atlas holds {} / {} glyph shapes (supervised / bootstrapped)",
        labelled.len(),
        trained.0,
        trained.1
    );
    for (name, score) in &scores {
        eprintln!("{}", score.report(name));
    }
    eprintln!("\nchat events (kind and name both right): reported / correct / in truth");
    for (name, e) in ["Windows OCR", "Atlas supervised", "Atlas bootstrapped"].iter().zip(events) {
        let pct = |a: usize, b: usize| 100.0 * a as f64 / b.max(1) as f64;
        eprintln!(
            "  {name:<20} {} / {} / {}   precision {:.1}%  recall {:.1}%",
            e.0,
            e.1,
            e.2,
            pct(e.1, e.0),
            pct(e.1, e.2)
        );
    }
    eprintln!(
        "\nspeed on {} lines: Windows OCR {:.2} ms/line wall, {:.2} ms/line CPU (whole process, its thread pool included); atlas {:.3} ms/line (one thread)",
        n,
        win_wall / n,
        win_cpu / n,
        atlas_ms / atlas_reads as f64
    );
    let first_pass: f64 = labelled.iter().map(|l| l.windows_ms).sum::<f64>() / n;
    eprintln!("(Windows OCR while extracting: {first_pass:.2} ms/line)");
}

#[test]
#[ignore = "diagnostic: do the letter runs line up with the letters?"]
fn poc_runs() {
    use maplelens_core::vision::segment;
    let Ok(out) = std::env::var("MAPLELENS_POC_OUT") else { return };
    let raw: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(format!("{out}/lines.json")).unwrap()).unwrap();
    let labels: HashMap<usize, String> = serde_json::from_str::<HashMap<String, String>>(
        &std::fs::read_to_string(format!("{out}/labels.json")).unwrap(),
    )
    .unwrap()
    .into_iter()
    .map(|(k, v)| (k.parse().unwrap(), v))
    .collect();
    let (mut equal, mut fewer, mut more, mut total) = (0, 0, 0, 0);
    for e in &raw {
        let id = e["id"].as_u64().unwrap() as usize;
        let Some(truth) = labels.get(&id) else { continue };
        let level = std::env::var("MAPLELENS_POC_THRESHOLD").ok().and_then(|v| v.parse().ok()).unwrap_or(150);
        let mask = padded(&threshold(
            &e["gray"].as_array().unwrap().iter().map(|r| r.as_str().unwrap().to_string()).collect::<Vec<_>>(),
            level,
        ));
        let baselines = segment::baselines(&mask);
        let letters = truth.chars().filter(|c| !c.is_whitespace()).count();
        let runs: usize = baselines.iter().map(|&b| segment::runs(&segment::cell(&mask, b, 14, 2)).len()).sum();
        total += 1;
        if runs == letters {
            equal += 1;
        } else if runs < letters {
            fewer += 1;
        } else {
            more += 1;
        }
        if total <= 12 {
            eprintln!("{id}: {truth:?} letters {letters} runs {runs} baselines {baselines:?} height {}", mask.height);
        }
    }
    eprintln!("runs == letters {equal}, fewer {fewer} (touching), more {more} (broken) of {total}");
}

#[test]
#[ignore = "does voting over margins lift Windows OCR's visitor names?"]
fn poc_vote() {
    use maplelens_core::shop::ShopEvent;
    use maplelens_core::shop::parse::parse_chat;
    let Ok(out) = std::env::var("MAPLELENS_POC_OUT") else { return };
    let raw: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(format!("{out}/lines.json")).unwrap()).unwrap();
    let labels: HashMap<usize, String> = serde_json::from_str::<HashMap<String, String>>(
        &std::fs::read_to_string(format!("{out}/labels.json")).unwrap(),
    )
    .unwrap()
    .into_iter()
    .map(|(k, v)| (k.parse().unwrap(), v))
    .collect();
    let engine = ocr::WindowsOcr::new(false).expect("Windows OCR");
    let read = |mask: &Mask, pad: usize| -> String {
        let mut padded = Mask::new(mask.width + 2 * pad, mask.height + 2 * pad);
        for y in 0..mask.height {
            for x in 0..mask.width {
                padded.set(x + pad, y + pad, mask.get(x, y));
            }
        }
        engine.recognize_gray(&ocr::upscaled(&padded)).map(|t| t.trim().to_string()).unwrap_or_default()
    };
    let key = |e: &ShopEvent| match e {
        ShopEvent::PlayerEntered(n) => Some(("entered", fold(n))),
        ShopEvent::PlayerLeft(n) => Some(("left", fold(n))),
        _ => None,
    };
    let (mut single, mut voted, mut total) = ([0usize; 2], [0usize; 2], 0usize);
    let (cpu0, wall0) = (process_cpu_ms(), Instant::now());
    for e in &raw {
        let id = e["id"].as_u64().unwrap() as usize;
        let Some(truth) = labels.get(&id).filter(|t| chat_event(t).is_some()) else { continue };
        let mask =
            mask_of(&e["rows"].as_array().unwrap().iter().map(|r| r.as_str().unwrap().to_string()).collect::<Vec<_>>());
        let wanted = chat_event(truth).map(|(k, n)| (k, fold(&n)));
        total += 1;
        let reads: Vec<Option<(&str, String)>> =
            [4usize, 1, 8, 2, 6].iter().map(|&pad| parse_chat(&read(&mask, pad)).as_ref().and_then(key)).collect();
        single[0] += usize::from(reads[0].is_some());
        single[1] += usize::from(reads[0] == wanted && wanted.is_some());
        let mut counts: Vec<(&Option<(&str, String)>, usize)> = Vec::new();
        for r in reads.iter().filter(|r| r.is_some()) {
            match counts.iter_mut().find(|(c, _)| *c == r) {
                Some(c) => c.1 += 1,
                None => counts.push((r, 1)),
            }
        }
        let best = counts.iter().max_by_key(|(_, n)| *n).map(|(r, _)| (*r).clone());
        voted[0] += usize::from(best.is_some());
        voted[1] += usize::from(best.as_ref() == Some(&wanted) && wanted.is_some());
    }
    let (wall, cpu) = (wall0.elapsed().as_secs_f64() * 1000.0, process_cpu_ms() - cpu0);
    let pct = |a: usize, b: usize| 100.0 * a as f64 / b.max(1) as f64;
    eprintln!(
        "visitor lines {total}: single read (pad 4)  reported {} correct {}  precision {:.1}%  recall {:.1}%",
        single[0],
        single[1],
        pct(single[1], single[0]),
        pct(single[1], total)
    );
    eprintln!(
        "visitor lines {total}: majority of 5 margins reported {} correct {}  precision {:.1}%  recall {:.1}%   ({:.1} ms/line wall, {:.1} ms CPU for 5 reads)",
        voted[0],
        voted[1],
        pct(voted[1], voted[0]),
        pct(voted[1], total),
        wall / total as f64,
        cpu / total as f64
    );
}
