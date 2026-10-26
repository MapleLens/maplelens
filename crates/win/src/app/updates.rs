//! The app's side of self-updating: show what the checker found (tray tooltip and menu, settings window), run
//! the installer on request, and swap the exe once it is verified. The update threads talk to the UI only
//! through their channels.

use std::path::PathBuf;

use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, w};

use super::App;
use crate::update::check::CheckResult;
use crate::update::install::{self, InstallEvent};
use crate::update::{Phase, UpdateState};

/// Open a web page in the player's browser.
fn open_page(url: &str) {
    // SAFETY: a plain shell "open" of an https URL we built ourselves.
    unsafe {
        ShellExecuteW(None, w!("open"), &HSTRING::from(url), None, None, SW_SHOWNORMAL);
    }
}

impl App {
    fn set_update_state(&mut self, state: UpdateState) {
        let announced = state.update.as_ref().filter(|_| state.phase != Phase::UpToDate).map(|u| u.version.clone());
        self.tray.set_update(announced);
        self.settings_window.show_update_state(&state);
        self.update_state = state;
    }

    pub(super) fn check_for_update(&mut self) {
        if !self.update_state.busy() {
            self.set_update_state(UpdateState::new(Phase::Checking, self.update_state.update.clone(), ""));
            self.updates.check_now();
        }
    }

    pub(super) fn start_update(&mut self) {
        let Some(update) = self.update_state.update.clone() else { return };
        if self.update_state.busy() {
            return;
        }
        if !install::can_self_update(&self.exe) {
            // A development build is rebuilt, not swapped: show the release instead.
            open_page(&update.page_url);
            return;
        }
        self.installer = Some(install::spawn_installer(update.clone(), self.exe.clone()));
        self.set_update_state(UpdateState::new(Phase::Downloading, Some(update), ""));
    }

    /// Apply the newest results of the update threads (called every tick).
    pub(super) fn follow_updates(&mut self) {
        while let Ok(result) = self.updates.events.try_recv() {
            if self.update_state.phase == Phase::Downloading {
                continue; // a download in progress outranks a background check
            }
            let state = match result {
                CheckResult::Failed(error) => UpdateState::new(Phase::Failed, self.update_state.update.clone(), error),
                CheckResult::Found(Some(update)) => UpdateState::new(Phase::Available, Some(update), ""),
                CheckResult::Found(None) => UpdateState::new(Phase::UpToDate, None, ""),
            };
            self.set_update_state(state);
        }
        let Some(installer) = &self.installer else { return };
        let events: Vec<InstallEvent> = installer.try_iter().collect();
        let update = self.update_state.update.clone();
        for event in events {
            match event {
                InstallEvent::Progress(share) => self.set_update_state(UpdateState::new(
                    Phase::Downloading,
                    update.clone(),
                    format!("{:.0}%", share * 100.0),
                )),
                InstallEvent::Failed(error) => {
                    self.installer = None;
                    return self.set_update_state(UpdateState::new(Phase::Failed, update, error));
                }
                InstallEvent::Staged(staged) => {
                    self.installer = None;
                    return self.finish_update(staged);
                }
            }
        }
    }

    fn finish_update(&mut self, staged: PathBuf) {
        if let Err(error) = install::swap(&self.exe, &staged) {
            let _ = std::fs::remove_file(&staged);
            let update = self.update_state.update.clone();
            return self.set_update_state(UpdateState::new(Phase::Failed, update, error.0));
        }
        crate::info!("updated the exe; restarting");
        // The new version starts once this one has closed its windows, tray icon and hotkey (see `main`).
        self.relaunch = Some(self.exe.clone());
        self.quit = true;
    }
}
