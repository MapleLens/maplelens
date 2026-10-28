//! MapleLens for Windows: `maplelens [--snapshot PATH] [--debug] [--version]`.
#![cfg_attr(not(test), windows_subsystem = "windows")]

#[cfg(not(windows))]
fn main() {
    eprintln!("MapleLens for Windows only runs on Windows; the web version runs anywhere.");
}

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod capture;
#[cfg(windows)]
mod clipboard;
#[cfg(windows)]
mod game;
#[cfg(windows)]
mod icon;
#[cfg(windows)]
mod layered;
#[cfg(windows)]
#[macro_use]
mod log;
#[cfg(all(windows, test))]
mod live_frames;
#[cfg(all(windows, test))]
mod map_tests;
#[cfg(windows)]
mod maps;
#[cfg(windows)]
mod net;
#[cfg(windows)]
mod notify;
#[cfg(windows)]
mod ocr;
#[cfg(windows)]
mod paths;
#[cfg(windows)]
mod phone;
#[cfg(windows)]
mod potions;
#[cfg(all(windows, test))]
mod reading_bench;
#[cfg(windows)]
mod refresh;
#[cfg(all(windows, test))]
mod scene_bench;
#[cfg(windows)]
mod screenshot;
#[cfg(windows)]
mod seal;
#[cfg(windows)]
mod settings_window;
#[cfg(windows)]
mod shop;
#[cfg(all(windows, test))]
mod shop_bench;
#[cfg(all(windows, test))]
mod shop_poc;
#[cfg(all(windows, test))]
mod shop_poc_atlas;
#[cfg(all(windows, test))]
mod shop_tests;
#[cfg(windows)]
mod startup;
#[cfg(all(windows, test))]
mod stream_bench;
#[cfg(all(windows, test))]
mod tests;
#[cfg(windows)]
mod training;
#[cfg(all(windows, test))]
mod training_tests;
#[cfg(windows)]
mod tray;
#[cfg(windows)]
mod update;
#[cfg(windows)]
#[cfg(windows)]
mod wgc;
#[cfg(windows)]
mod winclass;
#[cfg(windows)]
mod workers;

#[cfg(windows)]
const HELP: &str = "MapleLens: screen-reading overlays for MapleStory Classic.

Usage: maplelens [--snapshot PATH] [--debug] [--version]
       maplelens --screenshot OUT.png [--wait SECONDS] [--blur X,Y,W,H]...
       maplelens --shop

  --snapshot PATH     use a local price snapshot instead of automatic hourly updates
  --debug             also log detection details to maplelens.log, save each new tooltip to debug\\ and
                      print your Store Permit's events (also kept in debug\\shop_events.log)
  --shop              only watch your Store Permit (sales, visitors, chat, closing) and print its events
  --version           print the version
  --screenshot FILE   save a picture of the game with MapleLens's cards drawn in (for docs and bug reports)
  --wait SECONDS      time to bring the game to the front before the screenshot (default 5)
  --blur X,Y,W,H      pixelate a rectangle of the screenshot (client coordinates), e.g. other players' names";

#[cfg(windows)]
struct Args {
    snapshot: Option<std::path::PathBuf>,
    debug: bool,
    shop: bool,
    screenshot: Option<std::path::PathBuf>,
    /// Set by the self-updater when it starts the new version.
    updated_from: Option<String>,
    wait_s: f64,
    blur: Vec<(usize, usize, usize, usize)>,
}

#[cfg(windows)]
fn rectangle(text: &str) -> Result<(usize, usize, usize, usize), String> {
    let parts: Vec<usize> = text
        .split(',')
        .map(|v| v.trim().parse())
        .collect::<Result<_, _>>()
        .map_err(|_| format!("bad rectangle {text}"))?;
    match parts[..] {
        [x, y, w, h] => Ok((x, y, w, h)),
        _ => Err(format!("--blur takes X,Y,W,H, got {text}")),
    }
}

#[cfg(windows)]
fn parse(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut parsed = Args {
        snapshot: None,
        debug: false,
        shop: false,
        screenshot: None,
        updated_from: None,
        wait_s: 5.0,
        blur: Vec::new(),
    };
    let mut args = args.skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--snapshot" => parsed.snapshot = Some(args.next().ok_or("--snapshot needs a path")?.into()),
            "--debug" => parsed.debug = true,
            "--shop" => parsed.shop = true,
            "--updated-from" => parsed.updated_from = Some(args.next().ok_or("--updated-from needs a version")?),
            "--screenshot" => parsed.screenshot = Some(args.next().ok_or("--screenshot needs a file name")?.into()),
            "--wait" => {
                parsed.wait_s = args.next().and_then(|v| v.parse().ok()).ok_or("--wait needs a number of seconds")?;
            }
            "--blur" => parsed.blur.push(rectangle(&args.next().ok_or("--blur needs X,Y,W,H")?)?),
            "--version" | "-V" => return Err(format!("maplelens {}", env!("CARGO_PKG_VERSION"))),
            "--help" | "-h" => return Err(HELP.to_string()),
            other => return Err(format!("unknown option {other}\n\n{HELP}")),
        }
    }
    Ok(parsed)
}

/// A GUI app has no console; when started from one, print there (help, version, startup notes).
#[cfg(windows)]
fn attach_console() {
    use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: attaching to the parent's console, if any; failing is fine (started from Explorer).
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

/// Two copies would draw two cards: the second one quits. The mutex lives as long as the process.
#[cfg(windows)]
fn already_running() -> bool {
    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows::Win32::System::Threading::CreateMutexW;
    // SAFETY: a named mutex owned by this process until it exits.
    unsafe {
        CreateMutexW(None, false, windows::core::w!("Local\\MapleLens.SingleInstance")).is_ok()
            && GetLastError() == ERROR_ALREADY_EXISTS
    }
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    attach_console();
    let maintainer = matches!(std::env::args().nth(1).as_deref(), Some("--sign-release" | "--make-update-key"));
    let args = match parse(std::env::args().take(if maintainer { 1 } else { usize::MAX })) {
        Ok(args) => args,
        Err(message) => {
            println!("{message}");
            return std::process::ExitCode::SUCCESS;
        }
    };
    game::make_dpi_aware();
    if let Some(out) = &args.screenshot {
        let path = args.snapshot.clone().unwrap_or_else(paths::snapshot_file);
        return match screenshot::take(out, args.wait_s, &args.blur, app::build_catalog(&path)) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => {
                println!("screenshot failed: {error}");
                std::process::ExitCode::FAILURE
            }
        };
    }
    // Maintainer tools for releases (see update::release), not shown in --help.
    let tool = match std::env::args().nth(1).as_deref() {
        Some("--sign-release") => std::env::args().nth(2).map(|zip| update::release::sign_release(zip.as_ref())),
        Some("--make-update-key") => std::env::args()
            .nth(2)
            .map(|out| update::release::make_update_key(out.as_ref(), !std::env::args().any(|a| a == "--no-secret"))),
        _ => None,
    };
    if let Some(result) = tool {
        return match result {
            Ok(message) => {
                println!("{message}");
                std::process::ExitCode::SUCCESS
            }
            Err(error) => {
                println!("{error}");
                std::process::ExitCode::FAILURE
            }
        };
    }
    // `--shop` is a read-only diagnostic (no tray, hotkey or windows): it may run beside the app, for comparing.
    if !args.shop && already_running() {
        println!("MapleLens is already running (see the tray icon).");
        return std::process::ExitCode::SUCCESS;
    }
    if let Some(path) = log::init(args.debug) {
        println!("Logging to {}", path.display());
    }
    if args.shop {
        shop::console();
        return std::process::ExitCode::SUCCESS;
    }
    if let Some(old) = &args.updated_from {
        info!("updated from {old} to {}", env!("CARGO_PKG_VERSION"));
    }
    // The previous version's file stays locked until that process has fully exited.
    let exe = update::install::current_exe();
    std::thread::spawn(move || update::install::cleanup_leftovers(&exe, 20, std::time::Duration::from_millis(500)));
    let path = args.snapshot.clone().unwrap_or_else(paths::snapshot_file);
    let catalog = app::build_catalog(&path);
    match app::App::new(catalog, args.snapshot.is_none(), args.debug) {
        Ok(mut app) => {
            info!("running");
            let relaunch = app.run();
            drop(app); // the tray icon, hotkey and windows go first, so the new instance finds them free
            info!("exit");
            if let Some(exe) = relaunch {
                update::install::relaunch(&exe, env!("CARGO_PKG_VERSION"));
            }
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            error!("could not start: {error}");
            println!("MapleLens could not start: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
