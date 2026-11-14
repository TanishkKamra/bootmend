//! The "Free up space" page: measure first, then clear only what the person ticked.

use super::theme::{self, pal, S1, S2, S3, S4};
use super::widgets::{self, card, icon, muted, section, small};
use super::{App, Job, Pending};
use crate::cleanup::{self, size_text, Cleaned, Group, Item, Places};
use crate::rules::plural;
use crate::util::shell_open;
use eframe::egui::{self, RichText, Ui};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

/// What a look around found.
pub struct Found {
    pub items: Vec<Item>,
    /// Apps that must be closed first and are open now, by item id.
    pub open: HashMap<&'static str, Vec<String>>,
    pub recycle: Option<(u64, u64)>,
    pub free: Option<u64>,
    pub windows_old: bool,
}

/// The page's state: what was found, what is ticked, and how cleaning went.
pub struct CleanupView {
    pub found: Found,
    pub ticked: HashSet<&'static str>,
    pub done: HashMap<&'static str, Result<Cleaned, String>>,
    /// Which ticked item is being cleared now.
    pub progress: Arc<AtomicUsize>,
    pub cleaning: Vec<&'static str>,
    pub error: Option<String>,
}

/// Places that are never cleared, with the reason. Shown so people know it was a choice, not an oversight.
const LEFT_ALONE: &[(&str, &str)] = &[
    ("Your own files", "Documents, pictures, the desktop and everything else in Downloads are never touched."),
    ("Browser history, cookies and saved passwords", "Clearing them signs you out of websites. Only the browsers' caches are cleared here."),
    ("Installer caches", "Apps keep these to repair, update or uninstall themselves. Removing them can leave an app stuck."),
    ("Driver store", "Windows uses it to set a device up again when you plug it back in."),
    ("Page file and hibernation file", "Windows sizes them itself. Removing them can cause crashes or turn off fast start-up."),
    ("Restore points", "They are your way back if a change goes wrong. Windows deletes old ones on its own."),
    ("Prefetch", "Windows uses it to open apps faster. Clearing it only makes the PC slower for a few days."),
    ("The registry", "Registry cleaners free no real space, and a wrong guess can stop an app from working."),
];

fn look(elevated: bool) -> Result<Found, String> {
    let places = Places::current().ok_or("Windows didn't say where your folders are.")?;
    let items = cleanup::catalog(&places, elevated, SystemTime::now());
    let mut open = HashMap::new();
    for i in &items {
        if let Some((_, procs)) = &i.close_first {
            let r = cleanup::running(procs);
            if !r.is_empty() {
                open.insert(i.id, r);
            }
        }
    }
    Ok(Found {
        windows_old: places.system_drive.join("Windows.old").is_dir(),
        free: cleanup::free_space(&places.system_drive),
        recycle: cleanup::recycle_bin(),
        items,
        open,
    })
}

/// Safe to clear without asking: in the safe group, allowed with these rights, and not held open
/// by an app.
fn safe(i: &Item, found: &Found, elevated: bool) -> bool {
    i.group == Group::Safe && (elevated || !i.admin) && !found.open.contains_key(i.id)
}

impl CleanupView {
    fn new(found: Found, elevated: bool) -> Self {
        let ticked = found.items.iter().filter(|i| safe(i, &found, elevated)).map(|i| i.id).collect();
        CleanupView { found, ticked, done: HashMap::new(), progress: Arc::new(AtomicUsize::new(0)), cleaning: vec![], error: None }
    }

    /// The items Bootmend clears without asking, ticked or not.
    pub fn safe(&self, elevated: bool) -> Vec<&Item> {
        self.found.items.iter().filter(|i| safe(i, &self.found, elevated)).collect()
    }

    pub fn selected(&self) -> Vec<&Item> {
        self.found.items.iter().filter(|i| self.ticked.contains(i.id)).collect()
    }
}

/// Total of the known sizes, and whether any ticked item's size is only known after cleaning.
pub fn total(items: &[&Item]) -> (u64, bool) {
    (items.iter().filter_map(|i| i.bytes).sum(), items.iter().any(|i| i.bytes.is_none()))
}

impl App {
    pub(super) fn start_cleanup_look(&mut self, ctx: &egui::Context) {
        self.busy.insert("cleanup-look".into());
        let elevated = self.elevated;
        self.spawn(ctx, move || Job::CleanupFound(look(elevated)));
    }

    pub(super) fn cleanup_found(&mut self, r: Result<Found, String>) {
        self.busy.remove("cleanup-look");
        self.cleanup = Some(match r {
            Ok(f) => CleanupView::new(f, self.elevated),
            Err(e) => {
                let empty = Found { items: vec![], open: HashMap::new(), recycle: None, free: None, windows_old: false };
                CleanupView { error: Some(e), ..CleanupView::new(empty, self.elevated) }
            }
        });
    }

    pub(super) fn start_cleaning(&mut self, ctx: &egui::Context, ids: Vec<&'static str>) {
        let Some(v) = &mut self.cleanup else { return };
        let items: Vec<Item> = v.found.items.iter().filter(|i| ids.contains(&i.id)).cloned().collect();
        v.progress.store(0, Ordering::Relaxed);
        v.cleaning = items.iter().map(|i| i.id).collect();
        let progress = v.progress.clone();
        self.busy.insert("cleanup".into());
        self.spawn(ctx, move || {
            let drive = Places::current().map(|p| p.system_drive).unwrap_or_else(|| "C:\\".into());
            let mut out = vec![];
            for (n, item) in items.iter().enumerate() {
                progress.store(n, Ordering::Relaxed);
                // Checked again right before, because the person may have opened the app since the confirmation.
                let open = item.close_first.as_ref().filter(|(_, procs)| !cleanup::running(procs).is_empty());
                let r = match open {
                    Some((app, _)) => Err(format!("{app} was open, so this was left alone.")),
                    None => cleanup::clean(item, &drive).map_err(|e| format!("{e:#}")),
                };
                out.push((item.id, r));
            }
            Job::Cleaned(out)
        });
    }

    pub(super) fn cleaned(&mut self, results: Vec<(&'static str, Result<Cleaned, String>)>) {
        self.busy.remove("cleanup");
        let Some(v) = &mut self.cleanup else { return };
        let (mut freed, mut skipped, mut failed) = (0, 0, 0);
        for (id, r) in results {
            match &r {
                Ok(c) => {
                    freed += c.bytes;
                    skipped += c.skipped;
                    v.ticked.remove(id);
                }
                Err(_) => failed += 1,
            }
            v.done.insert(id, r);
        }
        v.cleaning.clear();
        v.found.recycle = cleanup::recycle_bin();
        if let Some(d) = Places::current() {
            v.found.free = cleanup::free_space(&d.system_drive);
        }
        let mut msg = format!("Freed {}.", size_text(freed));
        if skipped > 0 {
            msg.push_str(&format!(" {} in use, so they were left alone.", plural(skipped as usize, "file was", "files were")));
        }
        if failed > 0 {
            msg.push_str(&format!(" {} couldn't be cleared; the list says why.", plural(failed, "item", "items")));
        }
        self.toast(msg, failed == 0);
    }

    pub(super) fn recycle_emptied(&mut self, r: Result<(), String>) {
        self.busy.remove("recycle");
        if let Some(v) = &mut self.cleanup {
            v.found.recycle = cleanup::recycle_bin();
            if let Some(d) = Places::current() {
                v.found.free = cleanup::free_space(&d.system_drive);
            }
        }
        match r {
            Ok(()) => self.toast("The Recycle Bin is empty.", true),
            Err(e) => self.toast(format!("Couldn't empty the Recycle Bin: {e}"), false),
        }
    }

    pub(super) fn cleanup_page(&mut self, ui: &mut Ui) {
        let p = pal(ui.ctx());
        let looking = self.busy.contains("cleanup-look");
        let cleaning = self.busy.contains("cleanup");
        if self.cleanup.is_none() && !looking {
            self.start_cleanup_look(&ui.ctx().clone());
        }
        muted(ui, "Bootmend measures first and clears only what you tick. Files an app is using are skipped, and nothing else on the PC changes.");
        ui.add_space(S4);

        let Some(v) = self.cleanup.as_mut().filter(|_| !looking) else {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.label("Measuring caches and leftovers. This takes about 20 seconds.");
                });
            });
            return;
        };
        if let Some(e) = &v.error {
            widgets::note(ui, p.red, p.red_bg, icon::WARN, |ui| {
                ui.label(format!("Couldn't look for files to clear: {e}"));
            });
            ui.add_space(S4);
        }

        // Summary and the one main action.
        let mut clean_now = false;
        let mut look_again = false;
        card(ui, |ui| {
            let sel = v.selected();
            let (bytes, unknown) = total(&sel);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(size_text(bytes)).text_style(theme::big()));
                    let more = if unknown { ", plus what Windows' own clean-up finds" } else { "" };
                    small(ui, format!("{} ticked{more}", plural(sel.len(), "item", "items")));
                    if let Some(f) = v.found.free {
                        small(ui, format!("Free on drive C now: {}", size_text(f)));
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if cleaning {
                        let i = v.progress.load(Ordering::Relaxed);
                        let title = v.cleaning.get(i).and_then(|id| v.found.items.iter().find(|x| x.id == *id)).map(|x| x.title).unwrap_or("");
                        ui.label(format!("Clearing {title} ({} of {})", i + 1, v.cleaning.len()));
                        ui.add(egui::Spinner::new());
                    } else {
                        clean_now = widgets::primary_enabled(ui, "Free up space", !sel.is_empty()).clicked();
                        look_again = widgets::icon_button(ui, icon::REFRESH, "Look again", true).clicked();
                    }
                });
            });
        });

        let elevated = self.elevated;
        for (group, title, sub) in [
            (Group::Safe, "Safe to clear", "Caches and leftovers that apps and Windows rebuild on their own when they need them."),
            (Group::Choice, "Your choice", "Each of these has a catch, written under it, so they start unticked."),
        ] {
            let items: Vec<&Item> = v.found.items.iter().filter(|i| i.group == group).collect();
            if items.is_empty() {
                continue;
            }
            section(ui, title);
            muted(ui, sub);
            ui.add_space(S2);
            card(ui, |ui| {
                for (n, item) in items.iter().enumerate() {
                    if n > 0 {
                        ui.add_space(S2);
                        ui.separator();
                        ui.add_space(S2);
                    }
                    let mut on = v.ticked.contains(item.id);
                    let done = v.done.get(item.id);
                    let needs_admin = item.admin && !elevated;
                    let enabled = !cleaning && !needs_admin && !matches!(done, Some(Ok(_)));
                    if row(ui, item, &mut on, enabled, done, v.found.open.get(item.id), needs_admin) {
                        if on {
                            v.ticked.insert(item.id);
                        } else {
                            v.ticked.remove(item.id);
                        }
                    }
                }
            });
        }

        section(ui, "Recycle Bin");
        let mut empty_bin = false;
        card(ui, |ui| match v.found.recycle {
            Some((b, n)) if n > 0 => {
                ui.label(RichText::new(format!("{}, {}", plural(n as usize, "item", "items"), size_text(b))).family(theme::semibold()));
                muted(ui, "Files you deleted wait here in case you want them back. Emptying it deletes them for good.");
                ui.add_space(S3);
                ui.horizontal_wrapped(|ui| {
                    empty_bin = widgets::secondary_enabled(ui, "Empty the Recycle Bin", !cleaning && !self.busy.contains("recycle")).clicked();
                    if widgets::icon_button(ui, icon::OPEN, "Look inside first", true).clicked() {
                        if let Err(e) = shell_open("shell:RecycleBinFolder") {
                            self.toasts.push((format!("{e:#}"), false, std::time::Instant::now()));
                        }
                    }
                });
            }
            Some(_) => {
                muted(ui, "The Recycle Bin is empty.");
            }
            None => {
                muted(ui, "Windows didn't say how full the Recycle Bin is.");
            }
        });

        if v.found.windows_old {
            ui.add_space(S4);
            widgets::note(ui, p.blue, p.blue_bg, icon::INFO, |ui| {
                ui.label("Your previous version of Windows is kept in a folder called Windows.old, so you can go back to it after a big update. Windows removes it on its own about 10 days after the update.");
                ui.label("To remove it sooner, open Storage settings, then Temporary files, and tick Previous Windows installation.");
                ui.add_space(S1);
                if widgets::secondary(ui, "Open storage settings").clicked() {
                    let _ = shell_open("ms-settings:storagesense");
                }
            });
        }

        section(ui, "Left alone on purpose");
        card(ui, |ui| {
            muted(ui, "Some cleaners clear these too. Bootmend doesn't, because the space isn't worth what can go wrong.");
            ui.add_space(S2);
            ui.collapsing("Show the list", |ui| {
                for (what, why) in LEFT_ALONE {
                    ui.label(RichText::new(*what).family(theme::semibold()));
                    muted(ui, *why);
                    ui.add_space(S2);
                }
            });
        });

        let ctx = ui.ctx().clone();
        if look_again {
            self.start_cleanup_look(&ctx);
        }
        if clean_now {
            let ids = self.cleanup.as_ref().map(|v| v.selected().iter().map(|i| i.id).collect()).unwrap_or_default();
            self.ask(Pending::Clean(ids));
        }
        if empty_bin {
            self.ask(Pending::EmptyRecycleBin);
        }
    }
}

/// One item: a checkbox with the title, its size on the right, and what it is underneath. Returns true when the tick changed.
fn row(ui: &mut Ui, item: &Item, on: &mut bool, enabled: bool, done: Option<&Result<Cleaned, String>>, open: Option<&Vec<String>>, needs_admin: bool) -> bool {
    let p = pal(ui.ctx());
    let mut changed = false;
    // A square, larger box: the theme's rounded corners otherwise make it look like a radio button.
    const BOX: f32 = 18.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().icon_width = BOX;
        ui.spacing_mut().icon_width_inner = 10.0;
        let ws = &mut ui.visuals_mut().widgets;
        for w in [&mut ws.inactive, &mut ws.hovered, &mut ws.active, &mut ws.noninteractive] {
            w.corner_radius = egui::CornerRadius::same(theme::CONTROL_RADIUS);
        }
        changed = ui.add_enabled(enabled, egui::Checkbox::new(on, RichText::new(item.title).family(theme::semibold()))).changed();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (text, color) = match (done, item.bytes) {
                (Some(Ok(c)), _) => (format!("Cleared {}", size_text(c.bytes)), p.green),
                (Some(Err(_)), _) => ("Not cleared".to_string(), p.red),
                (None, Some(b)) => (size_text(b), p.ink),
                (None, None) if needs_admin => ("Size unknown".to_string(), p.muted),
                (None, None) => ("Measured while clearing".to_string(), p.muted),
            };
            ui.label(RichText::new(text).color(color).family(theme::semibold()));
        });
    });
    let indent = BOX + ui.spacing().icon_spacing;
    ui.horizontal(|ui| {
        ui.add_space(indent);
        ui.vertical(|ui| {
            ui.add(egui::Label::new(RichText::new(item.why).color(p.muted)).wrap());
            if let Some(t) = item.tradeoff {
                ui.add(egui::Label::new(RichText::new(format!("The catch: {t}")).color(p.amber)).wrap());
            }
            if needs_admin {
                small(ui, "Needs administrator rights. Restart Bootmend as administrator to clear this.");
            }
            if let (Some(apps), Some((name, _))) = (open, &item.close_first) {
                if !apps.is_empty() && done.is_none() {
                    ui.label(RichText::new(format!("{name} is open. Close it first, or this is skipped.")).color(p.amber));
                }
            }
            if let Some(Err(e)) = done {
                ui.label(RichText::new(e).color(p.red));
            }
            if let Some(Ok(c)) = done {
                if c.skipped > 0 {
                    small(ui, format!("{} in use and left alone.", plural(c.skipped as usize, "file was", "files were")));
                }
            }
        });
    });
    changed
}
