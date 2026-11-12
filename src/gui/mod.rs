//! The desktop window: navigation, background jobs and confirmations.

mod pages;
mod screens;
pub mod theme;
mod widgets;

use crate::collect::{self, Progress};
use crate::diff::{self, Change};
use crate::fixes::{self, Fix, HistoryEntry};
use crate::rules::{self, Area, Finding};
use crate::scan;
use crate::snapshot::{self, AppUpgrade, DumpAnalysis, ScanResult, Snapshot};
use crate::util::data_dir;
use eframe::egui::{self, RichText};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;
use theme::{pal, S2, S3, S4, S5, S6};
use widgets::{ico, icon};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    Overview,
    Suggestions,
    ThisPc,
    Storage,
    Network,
    Security,
    Software,
    Startup,
    Crashes,
    Changes,
    Scan,
    History,
    Report,
    Privacy,
    About,
}

impl Page {
    pub const ALL: [Page; 15] = [
        Page::Overview,
        Page::Suggestions,
        Page::ThisPc,
        Page::Storage,
        Page::Network,
        Page::Security,
        Page::Software,
        Page::Startup,
        Page::Crashes,
        Page::Changes,
        Page::Scan,
        Page::History,
        Page::Report,
        Page::Privacy,
        Page::About,
    ];
    pub fn title(self) -> &'static str {
        match self {
            Page::Overview => "Overview",
            Page::Suggestions => "Suggestions",
            Page::ThisPc => "This PC",
            Page::Storage => "Storage",
            Page::Network => "Network",
            Page::Security => "Security",
            Page::Software => "Apps & extensions",
            Page::Startup => "Startup & background",
            Page::Crashes => "Crashes",
            Page::Changes => "Changes",
            Page::Scan => "Virus check",
            Page::History => "History & undo",
            Page::Report => "Save a report",
            Page::Privacy => "Privacy",
            Page::About => "About",
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Page::Overview => icon::HOME,
            Page::Suggestions => icon::BULB,
            Page::ThisPc => icon::PC,
            Page::Storage => icon::DRIVE,
            Page::Network => icon::NETWORK,
            Page::Security => icon::SHIELD,
            Page::Software => icon::APPS,
            Page::Startup => icon::POWER,
            Page::Crashes => icon::CRASH,
            Page::Changes => icon::CHANGES,
            Page::Scan => icon::SCAN,
            Page::History => icon::UNDO,
            Page::Report => icon::REPORT,
            Page::Privacy => icon::LOCK,
            Page::About => icon::INFO,
        }
    }
    fn slug(self) -> String {
        self.title().to_lowercase().replace(['&', ' '], "-").replace("--", "-").replace("--", "-")
    }
}

/// Something the person asked for that needs a confirmation first.
#[derive(Clone)]
pub enum Pending {
    Fix(Fix, String),
    Undo(usize, String),
    Upgrade(String, String),
    Scan(scan::Kind, Option<PathBuf>),
    CheckUpdates,
    AnalyzeDumps,
    Elevate,
    DeleteData,
}

pub struct Confirm {
    pub title: String,
    pub body: Vec<String>,
    pub yes: String,
    pub action: Pending,
}

pub enum Job {
    Fixed(String, Result<(), String>),
    Undone(Result<(), String>),
    Scanned(Result<ScanResult, String>),
    Upgrades(Result<Vec<AppUpgrade>, String>),
    Upgraded(String, Result<String, String>),
    Dumps(Vec<DumpAnalysis>, Vec<String>),
    Saved(Result<PathBuf, String>),
    Deleted(Result<(), String>),
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    privacy_ack: bool,
}

fn settings_path() -> Option<PathBuf> {
    data_dir("").ok().map(|d| d.join("settings.json"))
}

fn load_settings() -> Settings {
    settings_path().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub struct Checking {
    rx: Receiver<Progress>,
    done: HashMap<&'static str, bool>,
    started: Instant,
}

#[derive(Default)]
pub struct UiState {
    pub area: Option<Area>,
    pub expanded: HashSet<String>,
    pub search: String,
    pub hide_personal: bool,
    pub banner_closed: bool,
    pub scan_cancel: Option<Arc<AtomicBool>>,
    pub scan_started: Option<Instant>,
    pub scan_label: String,
    pub last_report: Option<PathBuf>,
    pub show_reads: bool,
    pub scroll_top: bool,
}

struct SnapMode {
    dir: PathBuf,
    queue: Vec<Option<Page>>,
    frames: u32,
    asked: bool,
}

pub struct App {
    page: Page,
    snap: Option<Arc<Snapshot>>,
    findings: Vec<Finding>,
    changes: Vec<Change>,
    prev_taken: Option<String>,
    checking: Option<Checking>,
    check_error: Option<String>,
    tx: Sender<Job>,
    rx: Receiver<Job>,
    busy: HashSet<String>,
    confirm: Option<Confirm>,
    toasts: Vec<(String, bool, Instant)>,
    history: Vec<HistoryEntry>,
    settings: Settings,
    ui: UiState,
    elevated: bool,
    force_welcome: bool,
    snap_mode: Option<SnapMode>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext, snap_dir: Option<PathBuf>) -> Self {
        theme::apply(&cc.egui_ctx);
        let (tx, rx) = channel();
        let host = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "this-pc".into());
        let mut app = App {
            page: Page::Overview,
            snap: None,
            findings: vec![],
            changes: vec![],
            prev_taken: None,
            checking: None,
            check_error: None,
            tx,
            rx,
            busy: HashSet::new(),
            confirm: None,
            toasts: vec![],
            history: fixes::history(),
            settings: load_settings(),
            ui: UiState { hide_personal: true, ..Default::default() },
            elevated: crate::elevate::is_elevated(),
            force_welcome: false,
            snap_mode: None,
        };
        if let Some(s) = snapshot::latest(&host) {
            app.set_snapshot(s);
        }
        if let Some(dir) = snap_dir {
            let mut queue: Vec<Option<Page>> = vec![None];
            queue.extend(Page::ALL.iter().map(|p| Some(*p)));
            app.snap_mode = Some(SnapMode { dir, queue, frames: 0, asked: false });
        }
        app
    }

    fn set_snapshot(&mut self, s: Snapshot) {
        self.findings = rules::evaluate(&s);
        match snapshot::previous(&s.host, &s.id) {
            Some(prev) => {
                self.changes = diff::diff(&prev, &s);
                self.prev_taken = Some(prev.taken);
            }
            None => {
                self.changes.clear();
                self.prev_taken = None;
            }
        }
        self.snap = Some(Arc::new(s));
    }

    /// Re-run the rules after something inside the snapshot changed (scan, updates, dumps).
    fn update_snapshot(&mut self, f: impl FnOnce(&mut Snapshot)) {
        if let Some(s) = &self.snap {
            let mut s = (**s).clone();
            f(&mut s);
            if let Err(e) = snapshot::save(&s) {
                self.toast(format!("Couldn't save the result: {e}"), false);
            }
            self.findings = rules::evaluate(&s);
            self.snap = Some(Arc::new(s));
        }
    }

    fn toast(&mut self, text: impl Into<String>, ok: bool) {
        self.toasts.push((text.into(), ok, Instant::now()));
    }

    pub fn start_check(&mut self) {
        self.settings.privacy_ack = true;
        if let Some(p) = settings_path() {
            if let Err(e) = std::fs::write(&p, serde_json::to_vec(&self.settings).unwrap_or_default()) {
                eprintln!("could not save settings: {e}");
            }
        }
        let (tx, rx) = channel();
        std::thread::spawn(move || collect::collect(tx));
        self.check_error = None;
        self.force_welcome = false;
        self.checking = Some(Checking { rx, done: HashMap::new(), started: Instant::now() });
    }

    fn go(&mut self, p: Page) {
        self.page = p;
        self.ui.scroll_top = true;
    }

    fn spawn(&self, ctx: &egui::Context, work: impl FnOnce() -> Job + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(work());
            ctx.request_repaint();
        });
    }

    /// Ask before doing anything that changes the PC or goes online.
    pub fn ask(&mut self, action: Pending) {
        let c = match &action {
            Pending::Fix(f, title) => Confirm {
                title: f.label().to_string(),
                body: vec![f.describe(), format!("This is for: {title}.")],
                yes: f.label().to_string(),
                action,
            },
            Pending::Undo(_, title) => Confirm { title: "Undo this change?".into(), body: vec![format!("rigscan will put back how things were before \"{title}\".")], yes: "Undo".into(), action },
            Pending::Upgrade(_, name) => Confirm {
                title: format!("Update {name}?"),
                body: vec![
                    "winget, Windows' own app installer, will download the newest version from the app's publisher and install it quietly.".into(),
                    "Close the app first if it's open. Updates can't be undone from rigscan.".into(),
                ],
                yes: "Update".into(),
                action,
            },
            Pending::Scan(kind, target) => {
                let what = match kind {
                    scan::Kind::Quick => "Windows Security will check the places where harmful software usually hides. This takes a few minutes.".to_string(),
                    scan::Kind::Full => "Windows Security will check every file on every drive. This can take an hour or more, and the PC may feel slower meanwhile.".to_string(),
                    scan::Kind::Folder => format!("Windows Security will check {} and report what it finds. Nothing is moved or deleted.", target.as_ref().map(|t| t.display().to_string()).unwrap_or_default()),
                };
                Confirm { title: format!("Start a {}?", kind.label().to_lowercase()), body: vec![what, "You can keep using your PC, and stop the check at any time.".into()], yes: "Start".into(), action }
            }
            Pending::CheckUpdates => Confirm {
                title: "Check for app updates?".into(),
                body: vec![
                    "rigscan will ask winget, Windows' own app installer, which of your apps have newer versions.".into(),
                    "winget compares your installed apps with Microsoft's public app catalog online. rigscan itself sends nothing anywhere.".into(),
                ],
                yes: "Check now".into(),
                action,
            },
            Pending::AnalyzeDumps => Confirm {
                title: "Read the crash files?".into(),
                body: vec![
                    "rigscan will open each crash file with Microsoft's debugger to see which driver was involved.".into(),
                    "The first time, the debugger downloads matching debugging files from Microsoft (often 50 to 200 MB). It asks for them by driver name and version; the crash files themselves stay on this PC.".into(),
                    "This can take several minutes.".into(),
                ],
                yes: "Read crash files".into(),
                action,
            },
            Pending::Elevate => Confirm {
                title: "Restart as administrator?".into(),
                body: vec![
                    "Windows will ask for permission, then rigscan opens again with access to a few more details: Windows Security's skip list, crash files and some drive health data.".into(),
                    "Administrator rights are used for reading. Fixes still wait for you.".into(),
                ],
                yes: "Restart".into(),
                action,
            },
            Pending::DeleteData => Confirm {
                title: "Delete everything rigscan saved?".into(),
                body: vec![
                    "This removes all saved checks, the undo history and settings from this PC. Changes you already made to Windows stay as they are.".into(),
                    "This can't be undone.".into(),
                ],
                yes: "Delete".into(),
                action,
            },
        };
        self.confirm = Some(c);
    }

    fn run(&mut self, ctx: &egui::Context, action: Pending) {
        match action {
            Pending::Fix(fix, title) => {
                self.busy.insert(format!("fix:{fix:?}"));
                self.spawn(ctx, move || Job::Fixed(title.clone(), fixes::apply(&fix, &title).map_err(|e| format!("{e:#}"))));
            }
            Pending::Undo(i, _) => {
                self.busy.insert(format!("undo:{i}"));
                self.spawn(ctx, move || Job::Undone(fixes::undo(i).map_err(|e| format!("{e:#}"))));
            }
            Pending::Upgrade(id, _) => {
                self.busy.insert(format!("upgrade:{id}"));
                self.spawn(ctx, move || Job::Upgraded(id.clone(), collect::winget::upgrade(&id).map_err(|e| format!("{e:#}"))));
            }
            Pending::Scan(kind, target) => {
                let ex: Vec<String> =
                    self.snap.as_ref().map(|s| crate::util::J::strs(crate::util::J::g(&s.security, "defender"), "ExclusionPaths")).unwrap_or_default();
                let cancel = Arc::new(AtomicBool::new(false));
                self.ui.scan_cancel = Some(cancel.clone());
                self.ui.scan_started = Some(Instant::now());
                self.ui.scan_label = kind.label().to_string();
                self.busy.insert("scan".into());
                self.spawn(ctx, move || Job::Scanned(scan::run(kind, target.as_deref(), &ex, cancel).map_err(|e| format!("{e:#}"))));
            }
            Pending::CheckUpdates => {
                self.busy.insert("winget".into());
                self.spawn(ctx, || Job::Upgrades(collect::winget::check().map_err(|e| format!("{e:#}"))));
            }
            Pending::AnalyzeDumps => {
                let files: Vec<String> = self
                    .snap
                    .as_ref()
                    .map(|s| crate::util::J::a(&s.health, "minidumps").into_iter().filter_map(|d| crate::util::J::s(d, "Path")).collect())
                    .unwrap_or_default();
                self.busy.insert("dumps".into());
                self.spawn(ctx, move || {
                    let Some(cdb) = collect::dumps::cdb_path() else { return Job::Dumps(vec![], vec!["Microsoft's debugger isn't installed.".into()]) };
                    let (mut ok, mut errs) = (vec![], vec![]);
                    for f in files.iter().take(10) {
                        match collect::dumps::analyze(&cdb, std::path::Path::new(f)) {
                            Ok(a) => ok.push(a),
                            Err(e) => errs.push(format!("{e:#}")),
                        }
                    }
                    Job::Dumps(ok, errs)
                });
            }
            Pending::Elevate => match crate::elevate::relaunch_as_admin("") {
                Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Err(e) => self.toast(format!("{e:#}"), false),
            },
            Pending::DeleteData => {
                self.busy.insert("delete".into());
                self.spawn(ctx, || {
                    let r = data_dir("").map_err(|e| format!("{e:#}")).and_then(|d| std::fs::remove_dir_all(&d).map_err(|e| format!("{e}")));
                    Job::Deleted(r)
                });
            }
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(c) = &mut self.checking {
            let mut finished = None;
            while let Ok(m) = c.rx.try_recv() {
                match m {
                    Progress::StepDone { key, ok } => {
                        c.done.insert(key, ok);
                    }
                    Progress::Finished(s) => finished = Some(Ok(*s)),
                    Progress::Failed(e) => finished = Some(Err(e)),
                }
            }
            match finished {
                Some(Ok(s)) => {
                    self.checking = None;
                    self.set_snapshot(s);
                    self.go(Page::Overview);
                }
                Some(Err(e)) => {
                    self.checking = None;
                    self.check_error = Some(e);
                }
                None => ctx.request_repaint_after(std::time::Duration::from_millis(250)),
            }
        }
        while let Ok(j) = self.rx.try_recv() {
            match j {
                Job::Fixed(title, r) => {
                    self.busy.retain(|b| !b.starts_with("fix:"));
                    self.history = fixes::history();
                    match r {
                        Ok(()) => self.toast(format!("Done: {title}. You can undo it from History."), true),
                        Err(e) => self.toast(format!("That didn't work: {e}"), false),
                    }
                }
                Job::Undone(r) => {
                    self.busy.retain(|b| !b.starts_with("undo:"));
                    self.history = fixes::history();
                    match r {
                        Ok(()) => self.toast("Undone. Things are back how they were.", true),
                        Err(e) => self.toast(format!("Couldn't undo that: {e}"), false),
                    }
                }
                Job::Scanned(r) => {
                    self.busy.remove("scan");
                    self.ui.scan_cancel = None;
                    match r {
                        Ok(res) => {
                            self.toast(format!("{}: {}", res.kind, res.message), res.threats.is_empty());
                            self.update_snapshot(|s| s.scans.insert(0, res));
                        }
                        Err(e) => self.toast(format!("The check couldn't run: {e}"), false),
                    }
                }
                Job::Upgrades(r) => {
                    self.busy.remove("winget");
                    match r {
                        Ok(list) => {
                            self.toast(
                                if list.is_empty() {
                                    "All your apps that winget knows about are up to date.".to_string()
                                } else {
                                    format!("{} have updates available.", rules::plural(list.len(), "app", "apps"))
                                },
                                true,
                            );
                            self.update_snapshot(|s| s.upgrades = Some(list));
                        }
                        Err(e) => self.toast(format!("Couldn't check for updates: {e}"), false),
                    }
                }
                Job::Upgraded(id, r) => {
                    self.busy.remove(&format!("upgrade:{id}"));
                    match r {
                        Ok(_) => {
                            self.toast("Updated.", true);
                            self.update_snapshot(|s| {
                                if let Some(u) = &mut s.upgrades {
                                    u.retain(|x| x.id != id);
                                }
                            });
                        }
                        Err(e) => self.toast(e, false),
                    }
                }
                Job::Dumps(found, errs) => {
                    self.busy.remove("dumps");
                    let n = found.len();
                    if !found.is_empty() {
                        self.update_snapshot(|s| s.dumps = found);
                    }
                    if errs.is_empty() {
                        self.toast(format!("Read {}.", rules::plural(n, "crash file", "crash files")), true);
                    } else {
                        self.toast(format!("Read {}. {}", rules::plural(n, "crash file", "crash files"), errs.join(" ")), n > 0);
                    }
                }
                Job::Saved(r) => {
                    self.busy.remove("report");
                    match r {
                        Ok(p) => {
                            self.toast(format!("Saved to {}", p.display()), true);
                            self.ui.last_report = Some(p);
                        }
                        Err(e) => self.toast(format!("Couldn't save the report: {e}"), false),
                    }
                }
                Job::Deleted(r) => {
                    self.busy.remove("delete");
                    match r {
                        Ok(()) => {
                            self.snap = None;
                            self.findings.clear();
                            self.changes.clear();
                            self.history.clear();
                            self.settings = Settings::default();
                            self.toast("Everything rigscan saved has been deleted.", true);
                        }
                        Err(e) => self.toast(format!("Couldn't delete everything: {e}"), false),
                    }
                }
            }
        }
        self.toasts.retain(|t| t.2.elapsed().as_secs() < if t.1 { 6 } else { 12 });
        if !self.toasts.is_empty() || !self.busy.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui.ctx());
        ui.add_space(S5);
        ui.horizontal(|ui| {
            ui.add_space(S3);
            ui.label(ico(icon::PC, 20.0).color(p.accent));
            ui.label(RichText::new("rigscan").size(19.0).family(theme::semibold()));
        });
        ui.add_space(S5);
        let do_first = self.findings.iter().filter(|f| f.level == rules::Level::DoFirst).count();
        let groups: [(&str, &[Page]); 4] = [
            ("", &[Page::Overview, Page::Suggestions]),
            ("Details", &[Page::ThisPc, Page::Storage, Page::Network, Page::Security, Page::Software, Page::Startup, Page::Crashes]),
            ("Tools", &[Page::Changes, Page::Scan, Page::History, Page::Report]),
            ("This app", &[Page::Privacy, Page::About]),
        ];
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for (label, pages) in groups {
                if !label.is_empty() {
                    ui.add_space(S3);
                    ui.horizontal(|ui| {
                        ui.add_space(S3);
                        ui.label(RichText::new(label).small().color(p.muted));
                    });
                }
                for pg in pages {
                    let badge = (*pg == Page::Suggestions).then_some(do_first);
                    if widgets::nav_item(ui, pg.icon(), pg.title(), self.page == *pg, badge).clicked() {
                        self.go(*pg);
                    }
                }
            }
            ui.add_space(S4);
        });
    }

    fn header(&mut self, ui: &mut egui::Ui, title: &str, subtitle: &str) {
        let p = pal(ui.ctx());
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(title).text_style(egui::TextStyle::Heading));
                if !subtitle.is_empty() {
                    ui.label(RichText::new(subtitle).color(p.muted));
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::icon_button(ui, icon::REFRESH, "Check again", true).on_hover_text("Run all checks again. Takes about a minute.").clicked() {
                    self.start_check();
                }
                if let Some(s) = &self.snap {
                    widgets::small(ui, format!("Checked {}", s.taken));
                }
            });
        });
        ui.add_space(S4);
        if !self.elevated && !self.ui.banner_closed && !matches!(self.page, Page::Privacy | Page::About) {
            let mut close = false;
            let mut elevate = false;
            widgets::note(ui, p.blue, p.blue_bg, icon::ADMIN, |ui| {
                ui.label("A few details need administrator rights: Windows Security's skip list, crash files and some drive health data.");
                ui.horizontal(|ui| {
                    elevate = widgets::secondary(ui, "Restart as administrator").clicked();
                    close = widgets::secondary(ui, "Not now").clicked();
                });
            });
            if elevate {
                self.ask(Pending::Elevate);
            }
            self.ui.banner_closed |= close;
            ui.add_space(S4);
        }
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(c) = &self.confirm else { return };
        let (title, body, yes) = (c.title.clone(), c.body.clone(), c.yes.clone());
        let mut decision: Option<bool> = None;
        let m = egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(RichText::new(&title).text_style(theme::h2()));
            ui.add_space(S2);
            for b in &body {
                ui.label(b);
                ui.add_space(theme::S1);
            }
            ui.add_space(S3);
            ui.horizontal(|ui| {
                let y = widgets::primary(ui, &yes);
                if ui.memory(|m| m.focused().is_none()) {
                    y.request_focus();
                }
                if y.clicked() {
                    decision = Some(true);
                }
                if widgets::secondary(ui, "Cancel").clicked() {
                    decision = Some(false);
                }
            });
        });
        if m.should_close() && decision.is_none() {
            decision = Some(false);
        }
        if let Some(d) = decision {
            let c = self.confirm.take();
            if let (true, Some(c)) = (d, c) {
                self.run(ctx, c.action);
            }
        }
    }

    fn toasts_ui(&self, ctx: &egui::Context) {
        if self.toasts.is_empty() {
            return;
        }
        let p = pal(ctx);
        egui::Area::new(egui::Id::new("toasts")).anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-S5, -S5)).order(egui::Order::Foreground).show(ctx, |ui| {
            for (t, ok, _) in self.toasts.iter().rev().take(3) {
                let (fg, bg, ic) = if *ok { (p.green, p.green_bg, icon::CHECK) } else { (p.red, p.red_bg, icon::WARN) };
                egui::Frame::new()
                    .fill(bg)
                    .stroke(egui::Stroke::new(1.0_f32, p.line))
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                    .inner_margin(egui::Margin::symmetric(S4 as i8, S3 as i8))
                    .show(ui, |ui| {
                        ui.set_max_width(420.0);
                        ui.horizontal(|ui| {
                            ui.label(ico(ic, 16.0).color(fg));
                            ui.add(egui::Label::new(RichText::new(t).color(fg)).wrap());
                        });
                    });
                ui.add_space(S2);
            }
        });
    }

    fn snap_step(&mut self, ctx: &egui::Context) {
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let Some(m) = &mut self.snap_mode else { return };
        if let Some(img) = shot {
            let name = match m.queue.first().copied().flatten() {
                None => "welcome".to_string(),
                Some(p) => p.slug(),
            };
            let file = m.dir.join(format!("{name}.png"));
            if let Err(e) = save_png(&img, &file) {
                eprintln!("screenshot {}: {e}", file.display());
            }
            m.queue.remove(0);
            m.frames = 0;
            m.asked = false;
        }
        match m.queue.first().copied() {
            None => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Some(target) => {
                self.force_welcome = target.is_none();
                if let Some(p) = target {
                    if self.page != p {
                        self.page = p;
                        self.ui.scroll_top = true;
                    }
                }
                m.frames += 1;
                if m.frames > 6 && !m.asked {
                    m.asked = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                }
                ctx.request_repaint();
            }
        }
    }
}

fn save_png(img: &egui::ColorImage, path: &std::path::Path) -> anyhow::Result<()> {
    let f = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), img.size[0] as u32, img.size[1] as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header()?;
    let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    w.write_image_data(&bytes)?;
    Ok(())
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        self.snap_step(ctx);
        let p = pal(ctx);

        if self.checking.is_some() || self.snap.is_none() || self.force_welcome || self.check_error.is_some() && self.snap.is_none() {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin::same(S6 as i8))).show(ctx, |ui| {
                if self.checking.is_some() {
                    self.checking_screen(ui);
                } else {
                    self.welcome_screen(ui);
                }
            });
        } else {
            egui::SidePanel::left("nav")
                .resizable(false)
                .exact_width(232.0)
                .frame(egui::Frame::new().fill(p.sidebar).inner_margin(egui::Margin::symmetric(S2 as i8, 0)))
                .show(ctx, |ui| self.sidebar(ui));
            egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg)).show(ctx, |ui| {
                let mut sa = egui::ScrollArea::vertical().auto_shrink(false);
                if std::mem::take(&mut self.ui.scroll_top) {
                    sa = sa.vertical_scroll_offset(0.0);
                }
                sa.show(ui, |ui| {
                    // Fix the content width so wide rows never push past the window edge.
                    let w = (ui.available_width() - 2.0 * S6).clamp(320.0, 1040.0);
                    egui::Frame::new().inner_margin(egui::Margin { left: S6 as i8, right: S6 as i8, top: S5 as i8, bottom: S6 as i8 }).show(ui, |ui| {
                        ui.set_width(w);
                        self.page_ui(ui);
                    });
                });
            });
        }
        self.confirm_dialog(ctx);
        self.toasts_ui(ctx);
    }
}
