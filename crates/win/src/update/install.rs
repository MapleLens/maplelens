//! Staging, verifying and swapping in a downloaded exe. A running exe can be renamed but not overwritten, so the
//! swap is rename-aside then move-in, with the old file put back if anything goes wrong.

use std::io::{Cursor, Read};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use super::{MAX_ZIP_BYTES, SELF_TEST_S, Update, UpdateError, fail, net};

pub const EXE_NAME: &str = "maplelens.exe";
pub const NEW_NAME: &str = "maplelens.new.exe";
pub const OLD_NAME: &str = "maplelens.old.exe";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DETACHED_PROCESS: u32 = 0x0000_0008;
const FILE_SHARE_READ: u32 = 0x0000_0001;

pub fn current_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from(EXE_NAME))
}

/// Only an installed exe replaces itself, and only where this process may write: installed for every account
/// (Program Files), a copy whose administrator prompt was declined shows the release page instead.
pub fn can_self_update(exe: &Path) -> bool {
    is_release_copy(exe) && exe.parent().is_some_and(can_write_in)
}

/// A development build (under cargo's `target`) is rebuilt instead, and a renamed exe would not be found again by
/// the relaunch.
pub fn is_release_copy(exe: &Path) -> bool {
    let named = exe.file_name().is_some_and(|n| n.eq_ignore_ascii_case(EXE_NAME));
    named && !exe.ancestors().any(|dir| dir.file_name().is_some_and(|n| n == "target"))
}

/// The swap renames the exe aside and moves the new one in, so it needs to create files in its folder.
fn can_write_in(dir: &Path) -> bool {
    let probe = dir.join(format!("maplelens.write-test.{}", std::process::id()));
    let created = std::fs::OpenOptions::new().write(true).create_new(true).open(&probe).is_ok();
    if created {
        let _ = std::fs::remove_file(&probe);
    }
    created
}

/// The previous version's file stays locked until that process has fully exited, so keep trying for a while.
pub fn cleanup_leftovers(exe: &Path, tries: u32, delay: Duration) {
    let pending: Vec<PathBuf> = [OLD_NAME, NEW_NAME].iter().map(|n| exe.with_file_name(n)).collect();
    for attempt in 0..tries {
        for path in pending.iter().filter(|p| p.exists()) {
            if std::fs::remove_file(path).is_err() {
                crate::info!("could not remove {} yet", path.display());
            }
        }
        if !pending.iter().any(|p| p.exists()) {
            return;
        }
        if attempt + 1 < tries {
            std::thread::sleep(delay);
        }
    }
}

/// Pull out the one expected member by exact name; nothing else in the zip is ever touched.
pub fn extract_exe(archive: &[u8], update: &Update, target: &Path) -> Result<(), UpdateError> {
    let result = (|| {
        let stem = update.manifest.asset.trim_end_matches(".zip");
        let member = format!("{stem}/{EXE_NAME}");
        let mut bundle = zip::ZipArchive::new(Cursor::new(archive))
            .map_err(|_| UpdateError("the update archive is not what was expected".into()))?;
        // Some zip writers store backslashes; match the one expected name either way, never a pattern.
        let index = (0..bundle.len())
            .find(|&i| bundle.name_for_index(i).is_some_and(|n| n.is_ok_and(|n| n.replace('\\', "/") == member)))
            .ok_or_else(|| UpdateError("the update archive is not what was expected".into()))?;
        let mut entry =
            bundle.by_index(index).map_err(|_| UpdateError("the update archive is not what was expected".into()))?;
        if entry.size() > MAX_ZIP_BYTES {
            return fail("the update contains an oversized program");
        }
        let expected = entry.size();
        // Written as a new file, not through whatever is at that name (a leftover, or a link another program made
        // there), and unshared for writing while it is written.
        let _ = std::fs::remove_file(target);
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(FILE_SHARE_READ)
            .open(target)
            .map_err(|e| UpdateError(format!("cannot write to {}: {e}", target.display())))?;
        let copied = std::io::copy(&mut (&mut entry).take(MAX_ZIP_BYTES + 1), &mut output)
            .map_err(|_| UpdateError("the update archive is corrupt".into()))?;
        if copied != expected {
            return fail("the update archive is corrupt"); // a zip lying about its size
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(target);
    }
    result
}

/// The new exe must start and report the version the signed manifest promised.
pub fn self_test(exe: &Path, version: &str) -> Result<(), UpdateError> {
    let mut child = Command::new(exe)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|_| UpdateError("the downloaded program does not start".into()))?;
    let deadline = Instant::now() + Duration::from_secs(SELF_TEST_S);
    let status = loop {
        if let Some(status) =
            child.try_wait().map_err(|_| UpdateError("the downloaded program does not start".into()))?
        {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            return fail("the downloaded program does not start");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut output = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut output);
    }
    if !status.success() || output.trim() != format!("maplelens {version}") {
        return fail("the downloaded program is not the expected version");
    }
    Ok(())
}

/// Download, verify, unpack next to `exe` and self-test. Returns the staged file.
pub fn stage(update: &Update, exe: &Path, progress: net::Progress<'_>) -> Result<PathBuf, UpdateError> {
    let staged = exe.with_file_name(NEW_NAME);
    let archive = net::download(&update.url, update.manifest.size, &update.manifest.sha256, progress)?;
    extract_exe(&archive, update, &staged)?;
    if let Err(error) = self_test(&staged, &update.version) {
        let _ = std::fs::remove_file(&staged);
        return Err(error);
    }
    Ok(staged)
}

pub fn swap(exe: &Path, staged: &Path) -> Result<(), UpdateError> {
    let old = exe.with_file_name(OLD_NAME);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old).map_err(|e| UpdateError(format!("cannot replace {}: {e}", exe.display())))?;
    if let Err(error) = std::fs::rename(staged, exe) {
        let _ = std::fs::rename(&old, exe); // put the working version back
        return fail(format!("cannot install the update: {error}"));
    }
    Ok(())
}

/// Start the new version once this one is gone (it removes the old file when it can).
pub fn relaunch(exe: &Path, old_version: &str) {
    if let Err(error) = Command::new(exe).args(["--updated-from", old_version]).creation_flags(DETACHED_PROCESS).spawn()
    {
        crate::error!("could not start the updated MapleLens: {error}");
    }
}

/// What the installer thread reports.
#[derive(Debug)]
pub enum InstallEvent {
    /// 0..1 while downloading.
    Progress(f64),
    /// Verified and ready to swap in.
    Staged(PathBuf),
    Failed(String),
}

/// Does the slow, failure-prone part off the UI thread; the UI thread performs the swap on `Staged`.
pub fn spawn_installer(update: Update, exe: PathBuf) -> Receiver<InstallEvent> {
    let (events, receiver) = channel();
    std::thread::Builder::new()
        .name("update-install".into())
        .spawn(move || {
            let progress_events = events.clone();
            let mut last = -1.0;
            let mut progress = |done: u64, total: u64| {
                let share = done as f64 / total.max(1) as f64;
                if share - last >= 0.01 || done == total {
                    last = share;
                    let _ = progress_events.send(InstallEvent::Progress(share));
                }
            };
            let event = match stage(&update, &exe, &mut progress) {
                Ok(staged) => InstallEvent::Staged(staged),
                Err(error) => {
                    crate::warn!("update to {} failed: {error}", update.version);
                    InstallEvent::Failed(error.0)
                }
            };
            let _ = events.send(event);
        })
        .expect("spawning the installer thread");
    receiver
}
