//! Bootmend's own updates: the daily check, the note on Home and the Updates card in Settings.

use super::pages::{setting, stacked};
use super::theme::{pal, S2, S5};
use super::widgets::{self, card, icon, section};
use super::{load_settings, write_settings, App, Job, Pending, Settings};
use crate::update::{self, Version};
use eframe::egui::{self, Ui};
use std::path::PathBuf;

/// Work that only reads, so a restart for the update loses nothing by cutting it short.
const READERS: [&str; 8] = ["update", "update-check", "cleanup-look", "trends", "points", "net", "winget", "dumps"];

fn today() -> String {
    chrono::Local::now().date_naive().to_string()
}

impl Settings {
    /// The automatic check is on and hasn't run today.
    fn update_due(&self) -> bool {
        self.auto_update && self.update_checked.as_deref() != Some(today().as_str())
    }
}

/// The update check after a scheduled check-up, which has no window: at most once a day, and
/// one notification per new version.
pub fn update_in_background() -> anyhow::Result<()> {
    let mut s = load_settings();
    if !s.privacy_ack || !s.update_due() || crate::sys::elevated() {
        return Ok(());
    }
    s.update_checked = Some(today());
    write_settings(&s);
    let Some(v) = update::check()?.map(|v| v.to_string()) else { return Ok(()) };
    if s.update_notified.as_deref() == Some(v.as_str()) {
        return Ok(());
    }
    crate::schedule::notify(&crate::schedule::update_toast_xml(&v))?;
    // Read again: the window may have saved a setting while GitHub answered.
    let mut s = load_settings();
    s.update_notified = Some(v);
    write_settings(&s);
    Ok(())
}

impl App {
    /// Says so once after an update, and remembers the version that ran.
    pub(super) fn note_new_version(&mut self) {
        let now = env!("CARGO_PKG_VERSION");
        if self.settings.ran_version == now {
            return;
        }
        if !self.settings.ran_version.is_empty() {
            log!("update", "Bootmend went from {} to {now}", self.settings.ran_version);
            self.toast(format!("Bootmend is now {now}."), true);
        }
        self.settings.ran_version = now.into();
        self.save_settings();
    }

    /// The once-a-day check when Bootmend opens. It waits until the welcome, which mentions it,
    /// has been answered.
    pub(super) fn auto_update_check(&mut self, ctx: &egui::Context) {
        if self.update_looked || self.snap_mode.is_some() || !self.settings.privacy_ack || self.elevated {
            return;
        }
        self.update_looked = true;
        if self.settings.update_due() {
            self.check_for_update(ctx, false);
        }
    }

    fn check_for_update(&mut self, ctx: &egui::Context, manual: bool) {
        if self.elevated || !self.busy.insert("update-check".into()) {
            return;
        }
        log!("action", "{}", if manual { "check for a new Bootmend" } else { "the daily check for a new Bootmend" });
        self.settings.update_checked = Some(today());
        self.save_settings();
        self.spawn(ctx, move || Job::UpdateChecked(update::check().map_err(|e| format!("{e:#}")), manual));
    }

    pub(super) fn on_update_checked(&mut self, r: Result<Option<Version>, String>, manual: bool) {
        self.busy.remove("update-check");
        match r {
            Ok(found) => {
                self.update = found;
                self.update_known = true;
                if manual {
                    match found {
                        Some(v) => self.toast(format!("Bootmend {v} is ready to install."), true),
                        None => self.toast("Bootmend is up to date.", true),
                    }
                }
            }
            Err(e) => {
                log!("update", "couldn't check for a new version: {e}");
                if manual {
                    self.toast(format!("Couldn't check for a new version: {e}."), false);
                }
            }
        }
    }

    pub(super) fn start_update(&mut self, ctx: &egui::Context, v: Version) {
        self.busy.insert("update".into());
        self.spawn(ctx, move || Job::Updated(update::install(v).map_err(|e| format!("{e:#}"))));
    }

    pub(super) fn on_updated(&mut self, ctx: &egui::Context, r: Result<PathBuf, String>) {
        self.busy.remove("update");
        let exe = match r {
            Ok(exe) => exe,
            Err(e) => {
                log!("update", "the update stopped: {e}");
                self.toast(format!("Couldn't update Bootmend: {e}."), false);
                return;
            }
        };
        self.update = None;
        // Something that changes the PC started while the update downloaded. It isn't cut short:
        // the new version is already in place and opens next time.
        if !self.can_update() {
            self.toast("The update is installed and starts next time Bootmend opens.", true);
            return;
        }
        match update::restart(&exe) {
            Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Err(e) => {
                log!("update", "the new version didn't start: {e:#}");
                self.toast("The update is installed and starts next time Bootmend opens.", true);
            }
        }
    }

    /// Nothing is running that the restart after an update would cut short.
    fn can_update(&self) -> bool {
        self.busy.iter().all(|b| READERS.contains(&b.as_str())) && !self.run.as_ref().is_some_and(|r| r.running()) && self.copy.is_none()
    }

    fn open_release(&mut self, v: Version) {
        let url = update::release_page(v);
        log!("action", "opening {url}");
        if let Err(e) = crate::util::shell_open(&url) {
            self.toast(format!("{e:#}"), false);
        }
    }

    /// On Home once a newer version is known, until "Not now".
    pub(super) fn update_note(&mut self, ui: &mut Ui) {
        let Some(v) = self.update.filter(|_| !self.update_hidden) else { return };
        let p = pal(ui.ctx());
        let updating = self.busy.contains("update");
        let ready = self.can_update() && !updating;
        let (mut install, mut notes, mut later) = (false, false, false);
        ui.add_space(S5);
        widgets::note(ui, p.blue, p.blue_bg, icon::REFRESH, |ui| {
            ui.label(if updating { format!("Downloading Bootmend {v}…") } else { format!("Bootmend {v} is ready.") });
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.y = S2;
                install = widgets::secondary_enabled(ui, "Update now", ready).clicked();
                notes = widgets::secondary(ui, "What's new").clicked();
                later = widgets::secondary_enabled(ui, "Not now", !updating).clicked();
            });
        });
        if install {
            self.ask(Pending::UpdateBootmend(v));
        }
        if notes {
            self.open_release(v);
        }
        self.update_hidden |= later;
    }

    fn update_words(&self) -> String {
        match self.update {
            _ if self.elevated => update::NOT_AS_ADMIN.into(),
            _ if self.busy.contains("update") => "Downloading the new version and checking its signature…".into(),
            _ if self.busy.contains("update-check") => "Checking for a new version…".into(),
            Some(v) => format!("Bootmend {v} is ready to install."),
            None if self.update_known => "This is the newest version.".into(),
            None => "Check now to see whether there's a newer version.".into(),
        }
    }

    /// The Updates card in Settings.
    pub(super) fn updates(&mut self, ui: &mut Ui) {
        section(ui, "Updates");
        let ctx = ui.ctx().clone();
        let words = self.update_words();
        let (checking, updating, ready) = (self.busy.contains("update-check"), self.busy.contains("update"), self.can_update());
        let (on, found, elevated) = (self.settings.auto_update, self.update, self.elevated);
        let (mut flip, mut check, mut install, mut notes) = (false, false, false, false);
        card(ui, |ui| {
            let name = "Check for updates automatically";
            flip = setting(ui, name, "Once a day, Bootmend asks GitHub whether there's a newer version.", |ui| {
                let clicked = widgets::switch(ui, on, true, name).clicked();
                ui.label(if on { "On" } else { "Off" });
                clicked
            });
            ui.separator();
            stacked(ui, &format!("Bootmend {}", env!("CARGO_PKG_VERSION")), &words, |ui| {
                if elevated {
                    return;
                }
                if found.is_some() {
                    install = widgets::primary_enabled(ui, "Update now", ready && !updating).clicked();
                    notes = widgets::icon_button(ui, icon::OPEN, "What's new", true).clicked();
                }
                check = widgets::secondary_enabled(ui, "Check now", !checking && !updating).clicked();
            });
        });
        if flip {
            log!("settings", "automatic update checks turned {}", if on { "off" } else { "on" });
            self.settings.auto_update = !on;
            self.save_settings();
        }
        if check {
            self.check_for_update(&ctx, true);
        }
        if let Some(v) = found {
            if install {
                self.ask(Pending::UpdateBootmend(v));
            }
            if notes {
                self.open_release(v);
            }
        }
    }
}
