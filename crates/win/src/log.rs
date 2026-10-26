//! The log file players can send when something breaks: maplelens.log next to the exe (or, where that folder
//! is read-only, in the per-user data folder). Small and rotated, so it fits in a message.

use std::fmt::Arguments;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::paths;

pub const LOG_NAME: &str = "maplelens.log";
/// Small enough to attach to a message; two rotations keep the previous run.
const MAX_BYTES: u64 = 512 * 1024;
const BACKUPS: u32 = 2;

struct Sink {
    path: PathBuf,
    file: File,
}

static SINK: Mutex<Option<Sink>> = Mutex::new(None);
static DEBUG: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Debug,
    Info,
    Warning,
    Error,
}

/// Open the log file and route panics into it. Returns its path, or None when no folder is writable.
pub fn init(debug: bool) -> Option<PathBuf> {
    DEBUG.store(debug, Ordering::Relaxed);
    let folders = [paths::exe_dir(), Some(paths::data_dir())];
    let sink = folders.into_iter().flatten().find_map(|folder| {
        std::fs::create_dir_all(&folder).ok()?;
        let path = folder.join(LOG_NAME);
        let file = OpenOptions::new().create(true).append(true).open(&path).ok()?;
        Some(Sink { path, file })
    })?;
    let path = sink.path.clone();
    *SINK.lock().unwrap_or_else(|e| e.into_inner()) = Some(sink);
    std::panic::set_hook(Box::new(|info| {
        let thread = std::thread::current();
        write(Level::Error, format_args!("panic in thread {}: {info}", thread.name().unwrap_or("?")));
    }));
    write(Level::Info, format_args!("MapleLens {} ({})", env!("CARGO_PKG_VERSION"), std::env::consts::ARCH));
    Some(path)
}

pub fn enabled(level: Level) -> bool {
    level >= Level::Info || DEBUG.load(Ordering::Relaxed)
}

pub fn write(level: Level, message: Arguments<'_>) {
    if !enabled(level) {
        return;
    }
    let mut guard = SINK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(sink) = guard.as_mut() else { return };
    let stamp = timestamp();
    let _ = writeln!(sink.file, "{stamp} {level:?} {message}");
    if sink.file.metadata().is_ok_and(|m| m.len() > MAX_BYTES) {
        rotate(sink);
    }
}

fn rotate(sink: &mut Sink) {
    for n in (1..BACKUPS).rev() {
        let _ = std::fs::rename(backup(&sink.path, n), backup(&sink.path, n + 1));
    }
    let _ = std::fs::rename(&sink.path, backup(&sink.path, 1));
    if let Ok(file) = OpenOptions::new().create(true).append(true).open(&sink.path) {
        sink.file = file;
    }
}

fn backup(path: &std::path::Path, n: u32) -> PathBuf {
    path.with_extension(format!("log.{n}"))
}

pub(crate) fn timestamp() -> String {
    let now = paths::now_unix();
    let days = (now / 86400.0).floor() as i64;
    let (y, m, d) = maplelens_core::market::time::date_from_days(days);
    let secs = now - days as f64 * 86400.0;
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:06.3}Z",
        (secs / 3600.0) as u32,
        (secs % 3600.0 / 60.0) as u32,
        secs % 60.0
    )
}

#[macro_export]
macro_rules! debug { ($($t:tt)*) => { $crate::log::write($crate::log::Level::Debug, format_args!($($t)*)) } }
#[macro_export]
macro_rules! info { ($($t:tt)*) => { $crate::log::write($crate::log::Level::Info, format_args!($($t)*)) } }
#[macro_export]
macro_rules! warn { ($($t:tt)*) => { $crate::log::write($crate::log::Level::Warning, format_args!($($t)*)) } }
#[macro_export]
macro_rules! error { ($($t:tt)*) => { $crate::log::write($crate::log::Level::Error, format_args!($($t)*)) } }
