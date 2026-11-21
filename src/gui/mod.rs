//! The desktop window: navigation, background jobs and confirmations.

mod cleanup_page;
mod copy_dialog;
mod home;
mod live_page;
mod mark;
mod pages;
mod run;
mod schedule_ui;
mod screens;
mod system;
pub mod theme;
mod update_ui;
mod widgets;

use crate::collect::{self, Progress};
use crate::diff::{self, Change};
use crate::fixes::{self, Fix, HistoryEntry};
use crate::rules::{self, Finding};
use crate::scan;
use crate::snapshot::{self, AppUpgrade, DumpAnalysis, ScanResult, Snapshot};
use crate::util::{data_dir, shell_open};
use eframe::egui::{self, RichText};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;
use theme::{pal, S1, S2, S3, S4, S5, S6};
use widgets::{ico, icon};

/// The frame adapts to the window the way Windows Settings does. From `DOCK_MIN` up the sidebar
/// stays open beside the page (WinUI's NavigationView switches at 1008); narrower, it folds
/// away behind a menu button and opens over the page.
const DOCK_MIN: f32 = 1008.0;
/// Below this window width the page margins tighten, so a narrow window keeps room for content.
const NARROW: f32 = 640.0;
/// Below this much content width, rows put their buttons under the text and the page's actions
/// show as icons, the way Settings moves the Windows Update tile under the PC's name.
pub(super) const COMPACT: f32 = 560.0;
const SIDEBAR_W: f32 = 208.0;

/// How the frame sits in a window of a given width.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Fit {
    docked: bool,
    margin: f32,
}

fn fit_for(width: f32) -> Fit {
    Fit { docked: width >= DOCK_MIN, margin: if width < NARROW { S5 } else { S6 } }
}

/// The places in the sidebar. Settings sits at its foot, the way Windows apps place it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Place {
    Home,
    TuneUp,
    MyPc,
    History,
    Settings,
}

impl Place {
    pub const MAIN: [Place; 4] = [Place::Home, Place::TuneUp, Place::MyPc, Place::History];
    pub fn title(self) -> &'static str {
        match self {
            Place::Home => "Home",
            Place::TuneUp => "Tune-up",
            Place::MyPc => "My PC",
            Place::History => "History",
            Place::Settings => "Settings",
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Place::Home => icon::HOME,
            Place::TuneUp => icon::REPAIR,
            Place::MyPc => icon::PC,
            Place::History => icon::HISTORY,
            Place::Settings => icon::SETTINGS,
        }
    }
    /// The tabs across the top of the place, in order. A place with one view has none.
    pub fn tabs(self) -> &'static [Page] {
        match self {
            Place::Home | Place::Settings => &[],
            Place::TuneUp => &[Page::Cleanup, Page::Startup, Page::Live, Page::NetCheck],
            Place::MyPc => &[Page::ThisPc, Page::Storage, Page::Security, Page::Software, Page::Network, Page::Crashes],
            Place::History => &[Page::History, Page::Changes, Page::Trends, Page::Saved],
        }
    }
    fn first(self) -> Page {
        match self {
            Place::Home => Page::Home,
            Place::Settings => Page::Settings,
            _ => self.tabs()[0],
        }
    }
}

/// One view: a tab inside a place, or the whole of a place that has no tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    Home,
    Report,
    Cleanup,
    Startup,
    Live,
    NetCheck,
    ThisPc,
    Storage,
    Security,
    Software,
    Network,
    Crashes,
    History,
    Changes,
    Trends,
    Saved,
    Settings,
    Privacy,
}

impl Page {
    pub const ALL: [Page; 18] = [
        Page::Home,
        Page::Report,
        Page::Cleanup,
        Page::Startup,
        Page::Live,
        Page::NetCheck,
        Page::ThisPc,
        Page::Storage,
        Page::Security,
        Page::Software,
        Page::Network,
        Page::Crashes,
        Page::History,
        Page::Changes,
        Page::Trends,
        Page::Saved,
        Page::Settings,
        Page::Privacy,
    ];
    pub fn place(self) -> Place {
        match self {
            Page::Home | Page::Report => Place::Home,
            Page::Cleanup | Page::Startup | Page::Live | Page::NetCheck => Place::TuneUp,
            Page::ThisPc | Page::Storage | Page::Security | Page::Software | Page::Network | Page::Crashes => Place::MyPc,
            Page::History | Page::Changes | Page::Trends | Page::Saved => Place::History,
            Page::Settings | Page::Privacy => Place::Settings,
        }
    }
    /// A view opened from inside its place rather than from a tab or the sidebar. Its header shows
    /// the way back, as Windows Settings does: "Home > Save a report".
    pub fn inner(self) -> bool {
        matches!(self, Page::Report | Page::Privacy)
    }
    /// The tab's name, or the view's own name for a view inside Home.
    pub fn title(self) -> &'static str {
        match self {
            Page::Home => "Home",
            Page::Report => "Save a report",
            Page::Cleanup => "Free up space",
            Page::Startup => "Startup apps",
            Page::Live => "What's running",
            Page::NetCheck => "Connection check",
            Page::ThisPc => "Hardware",
            Page::Storage => "Storage",
            Page::Security => "Security",
            Page::Software => "Apps",
            Page::Network => "Network",
            Page::Crashes => "Crashes",
            Page::History => "Fixes",
            Page::Changes => "Since last check",
            Page::Trends => "Over time",
            Page::Saved => "Saved checks",
            Page::Settings => "Settings",
            Page::Privacy => "Privacy",
        }
    }
    fn slug(self) -> String {
        self.title().to_lowercase().replace('\'', "").replace(['&', ' '], "-").replace("--", "-").replace("--", "-")
    }
}

/// Who shows the logo this frame: Home's hero, or the Queue screen above its boxes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Home,
    Run,
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
    RestorePoint,
    Clean(Vec<&'static str>),
    EmptyRecycleBin,
    NetCheck,
    UpdateBootmend(crate::update::Version),
}

pub struct Confirm {
    pub title: String,
    pub body: Vec<String>,
    pub yes: String,
    pub action: Pending,
}

pub enum Job {
    /// Title, result, and a note about the restore point saved first (if one was tried).
    Fixed(String, Result<(), String>, Option<String>),
    RestorePoint(Result<(), String>),
    Undone(Result<(), String>),
    Scanned(Result<ScanResult, String>),
    Upgrades(Result<Vec<AppUpgrade>, String>),
    Upgraded(String, Result<String, String>),
    Dumps(Vec<DumpAnalysis>, Vec<String>),
    Saved(Result<PathBuf, String>),
    Deleted(Result<(), String>),
    CleanupFound(Result<cleanup_page::Found, String>),
    Cleaned(Vec<(&'static str, Result<crate::cleanup::Cleaned, String>)>),
    RecycleEmptied(Result<(), String>),
    NetChecked(crate::nettest::NetCheck),
    Scheduled(crate::schedule::Every, u8, Result<(), String>),
    Trends(Vec<crate::trends::Trend>),
    /// One box of the queue finished: its index, the result, how long it took in ms, and what
    /// clearing did when it was the leftover files.
    Worked(usize, Result<(), String>, f32, Option<Vec<(&'static str, Result<crate::cleanup::Cleaned, String>)>>),
    /// System Restore was turned on so a restore point could be saved.
    TurnedOn(Result<(), String>),
    Points(Result<Vec<fixes::RestorePoint>, String>),
    CopyLooked(Result<copy_dialog::Looked, String>),
    Copied(Result<crate::usb_copy::Copied, String>),
    /// The update check: the newer version it found, and whether Check now was pressed.
    UpdateChecked(Result<Option<crate::update::Version>, String>, bool),
    /// The update was downloaded, checked and put in place: the exe to start.
    Updated(Result<PathBuf, String>),
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    privacy_ack: bool,
    /// How often check-ups run in the background.
    schedule: crate::schedule::Every,
    /// The hour check-ups start, 0-23.
    schedule_hour: u8,
    /// The check-up offer was answered (or the person chose a pace).
    schedule_asked: bool,
    /// Buttons, links and switches take the Windows accent colour instead of cobalt.
    use_windows_accent: bool,
    /// "Don't show again" was pressed on the early-version notice.
    early_notice_off: bool,
    /// Bootmend asks GitHub once a day whether there's a newer version.
    auto_update: bool,
    /// The day the update check last ran, such as 2026-11-19, so it runs at most once a day.
    update_checked: Option<String>,
    /// The newest version a notification has already announced.
    update_notified: Option<String>,
    /// The version that ran last, to say so after an update.
    ran_version: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            privacy_ack: false,
            schedule: Default::default(),
            schedule_hour: crate::schedule::NOON,
            schedule_asked: false,
            use_windows_accent: false,
            early_notice_off: false,
            auto_update: true,
            update_checked: None,
            update_notified: None,
            ran_version: String::new(),
        }
    }
}

/// GitHub's bug form, with this version filled in.
fn report_url() -> String {
    format!("{}/issues/new?template=bug_report.yml&version={}", env!("CARGO_PKG_REPOSITORY"), env!("CARGO_PKG_VERSION"))
}

/// How a background job ended, for the log.
fn ended<T>(r: &Result<T, String>) -> String {
    match r {
        Ok(_) => "worked".into(),
        Err(e) => format!("failed: {e}"),
    }
}

fn settings_path() -> Option<PathBuf> {
    data_dir("").ok().map(|d| d.join("settings.json"))
}

fn load_settings() -> Settings {
    settings_path().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write_settings(s: &Settings) {
    if let Some(p) = settings_path() {
        if let Err(e) = std::fs::write(&p, serde_json::to_vec(s).unwrap_or_default()) {
            log!("settings", "couldn't save settings: {e}");
        }
    }
}

pub use system::mica_available;
pub use update_ui::update_in_background;

/// The check-up pace and hour the person chose, for setting the task up again outside the window.
pub fn saved_schedule() -> (crate::schedule::Every, u8) {
    let s = load_settings();
    (s.schedule, s.schedule_hour)
}

#[derive(Default)]
pub struct UiState {
    /// Where the logo goes this frame, and who asked for it.
    pub mark_slot: Option<(Slot, egui::Rect)>,
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
    /// The tab last open in each place, so going back to a place returns to it.
    pub last_tab: HashMap<Place, Page>,
    /// What sorts the What's running table: 0 processor, 1 memory, 2 drive.
    pub live_sort: usize,
    /// Whether the window is wide enough to keep the sidebar open, this frame.
    pub docked: bool,
    /// The folded sidebar, opened over the page, and whether to focus it on its first frame.
    pub nav_open: bool,
    pub nav_focus: bool,
    /// The pane was closed from inside it, so focus goes back to the menu button that opened it.
    pub nav_refocus: bool,
    /// Where the menu button sits this frame, so the pane's close button can sit on top of it.
    pub nav_button: Option<egui::Rect>,
}

/// One screenshot: the first-run screen, a view, or Home with the folded sidebar open.
#[derive(Clone, Copy)]
enum Shot {
    Welcome,
    View(Page),
    Menu,
    /// A confirmation open over the Crashes page.
    Dialog,
    /// The early-version notice over Home.
    Early,
    /// The Queue screen while choosing, part way through, and part way through with its details open.
    Queue,
    Fixing,
    Details,
    /// The copy dialog choosing a drive (0), copying (1) and done (2).
    Copy(u8),
}

/// Where the logo was last drawn, and its flight from there when it changes hands between Home
/// and the Queue screen.
#[derive(Default)]
struct Flight {
    owner: Option<Slot>,
    rect: Option<egui::Rect>,
    pass: u64,
    from: Option<(egui::Rect, f32)>,
}

impl Flight {
    const MS: f32 = 620.0;

    /// Where to draw the logo for `owner`, whose slot is `to`. `jump` skips the flight.
    fn place(&mut self, owner: Slot, to: egui::Rect, now: f32, pass: u64, jump: bool) -> egui::Rect {
        if self.owner != Some(owner) {
            // Only from where it was just drawn: a logo that wasn't on screen appears in place.
            let seen = self.pass + 3 >= pass;
            self.from = self.rect.filter(|_| seen && !jump).map(|r| (r, now));
            self.owner = Some(owner);
        }
        let r = match self.from {
            Some((from, at)) if now - at < Self::MS => {
                let t = (now - at) / Self::MS;
                let e = run::ease_io(t);
                let r = egui::Rect::from_min_max(from.min.lerp(to.min, e), from.max.lerp(to.max, e));
                // Rising, it arcs a little on its way.
                let arc = if to.center().y < from.center().y { -14.0 * (std::f32::consts::PI * t).sin() } else { 0.0 };
                r.translate(egui::vec2(0.0, arc))
            }
            _ => {
                self.from = None;
                to
            }
        };
        self.rect = Some(r);
        self.pass = pass;
        r
    }
}

fn resume_path() -> Option<PathBuf> {
    data_dir("").ok().map(|d| d.join("resume.json"))
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

struct SnapMode {
    dir: PathBuf,
    queue: Vec<Shot>,
    frames: u32,
    asked: bool,
    /// Long pages are captured in screen-sized parts: which part this is, and whether more follows.
    part: u32,
    more: bool,
}

pub struct App {
    page: Page,
    snap: Option<Arc<Snapshot>>,
    findings: Vec<Finding>,
    changes: Vec<Change>,
    prev_taken: Option<String>,
    /// The Queue screen, while a check-up or fixes are chosen, run or just over.
    run: Option<run::Run>,
    check_rx: Option<Receiver<Progress>>,
    /// The logo, which is also the loader. One mark, so it can fly between Home and the queue.
    mark: mark::Mark,
    flight: Flight,
    /// Milliseconds since the window opened, as the animations count them.
    clock: f32,
    /// When the page came back after the queue closed, to fade it in.
    page_fade: Option<f32>,
    /// A queue picked up after the restart as administrator: what to do next, and whether the
    /// leftover files still need adding (and ticking).
    resume_wait: Option<(run::Then, Option<bool>)>,
    /// A queue saved before the restart as administrator, waiting for the check-up that runs first.
    resume_check: Option<run::Resume>,
    taskbar: Option<system::Taskbar>,
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
    /// A restore point is saved before the first fix of each session, when Windows allows it.
    restore_saved: bool,
    /// Windows' restore points as History last read them, or nothing until it reads them again.
    points: Option<Result<Vec<fixes::RestorePoint>, String>>,
    /// The "Copy my files to a USB drive" dialog, while it's open.
    copy: Option<copy_dialog::Flow>,
    force_welcome: bool,
    snap_mode: Option<SnapMode>,
    cleanup: Option<cleanup_page::CleanupView>,
    net: Option<crate::nettest::NetCheck>,
    live: Option<crate::live::Monitor>,
    trends: Vec<crate::trends::Trend>,
    /// The check the trends were drawn for.
    trends_for: String,
    /// The window shows Windows' Mica backdrop behind the sidebar.
    mica: bool,
    /// The Windows accent last read, and when, while the person has accent sync on.
    win_accent: Option<theme::AccentShades>,
    accent_read: Option<Instant>,
    motion_read: Option<Instant>,
    /// The early-version notice is open. It opens at each launch until "Don't show again".
    early_notice: bool,
    /// A newer Bootmend the update check found.
    update: Option<crate::update::Version>,
    /// An update check finished this session, so Settings can say this is the newest version.
    update_known: bool,
    /// "Not now" was pressed on Home's update note.
    update_hidden: bool,
    /// The automatic update check has had its look this session.
    update_looked: bool,
}

impl App {
    /// `mica` says the window was created transparent so the Mica backdrop can show through.
    /// `resume` picks up the queue saved before restarting as administrator.
    pub fn new(cc: &eframe::CreationContext, snap_dir: Option<PathBuf>, mica: bool, resume: bool) -> Self {
        theme::apply(&cc.egui_ctx);
        let mica = mica
            && match system::apply_mica(cc) {
                Ok(()) => true,
                Err(e) => {
                    log!("window", "the window stays solid because Mica couldn't be turned on: {e}");
                    false
                }
            };
        if let Some(h) = system::hwnd(cc) {
            crate::cleanup::OWNER.store(h.0 as isize, std::sync::atomic::Ordering::Relaxed);
        }
        let (tx, rx) = channel();
        let host = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "this-pc".into());
        let mut app = App {
            page: Page::Home,
            snap: None,
            findings: vec![],
            changes: vec![],
            prev_taken: None,
            run: None,
            check_rx: None,
            mark: mark::Mark::new(mark::Kind::Stones, 1.0),
            flight: Flight::default(),
            clock: 0.0,
            page_fade: None,
            resume_wait: None,
            resume_check: None,
            taskbar: if snap_dir.is_none() { system::Taskbar::new(cc) } else { None },
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
            restore_saved: false,
            points: None,
            copy: None,
            force_welcome: false,
            snap_mode: None,
            cleanup: None,
            net: None,
            live: None,
            trends: vec![],
            trends_for: String::new(),
            mica,
            win_accent: None,
            accent_read: None,
            motion_read: None,
            early_notice: false,
            update: None,
            update_known: false,
            update_hidden: false,
            update_looked: false,
        };
        if let Some(s) = snapshot::latest(&host) {
            app.set_snapshot(s);
        }
        app.mark.set_lift(if app.raised() { 1.0 } else { 0.0 }, true);
        // Not again when Bootmend restarts as administrator to carry on with a fix.
        app.early_notice = !app.settings.early_notice_off && snap_dir.is_none() && !resume;
        if snap_dir.is_none() {
            app.note_new_version();
        }
        if let Some(dir) = snap_dir {
            let mut queue = vec![Shot::Welcome];
            queue.extend(Page::ALL.iter().map(|p| Shot::View(*p)));
            queue.extend([Shot::Dialog, Shot::Early, Shot::Menu, Shot::Queue, Shot::Fixing, Shot::Details, Shot::Copy(0), Shot::Copy(1), Shot::Copy(2)]);
            app.snap_mode = Some(SnapMode { dir, queue, frames: 0, asked: false, part: 0, more: false });
        } else if resume {
            app.resume();
        }
        app
    }

    /// True when the next fix should be preceded by a restore point: once per session, with admin rights and System Restore on.
    fn restore_point_first(&self) -> bool {
        use crate::util::J;
        !self.restore_saved && self.elevated && self.snap.as_ref().is_some_and(|s| s.security.g("restore").b("COn") == Some(true))
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
        let text = text.into();
        log!("toast", "{text}");
        self.toasts.push((text, ok, Instant::now()));
    }

    /// Follows the Windows accent colour while accent sync is on. Reading it is cheap, and every
    /// return to the window repaints, so a change in Windows Settings shows up straight away.
    fn sync_accent(&mut self, ctx: &egui::Context) {
        if !self.settings.use_windows_accent || self.snap_mode.is_some() || self.accent_read.is_some_and(|t| t.elapsed().as_secs() < 1) {
            return;
        }
        self.accent_read = Some(Instant::now());
        let now = system::windows_accent();
        if now != self.win_accent {
            log!("window", "the Windows accent colour {}", if now.is_some() { "changed" } else { "couldn't be read" });
            self.win_accent = now;
            theme::set_accent(ctx, now.as_ref());
        }
    }

    /// Follows Windows' Animation effects setting, read at most once a second.
    fn sync_motion(&mut self, ctx: &egui::Context) {
        if self.motion_read.is_some_and(|t| t.elapsed().as_secs() < 1) {
            return;
        }
        self.motion_read = Some(Instant::now());
        let on = system::animations_on();
        if on != theme::motion(ctx) {
            log!("window", "Windows animation effects are now {}", if on { "on" } else { "off" });
            theme::set_motion(ctx, on);
        }
    }

    fn set_accent_sync(&mut self, ctx: &egui::Context, on: bool) {
        log!("settings", "using the Windows accent colour turned {}", if on { "on" } else { "off" });
        self.settings.use_windows_accent = on;
        self.save_settings();
        self.win_accent = None;
        self.accent_read = None;
        theme::set_accent(ctx, None);
        self.sync_accent(ctx);
        ctx.request_repaint();
    }

    fn save_settings(&self) {
        write_settings(&self.settings);
    }

    pub fn start_check(&mut self) {
        if self.run.as_ref().is_some_and(|r| r.running()) {
            return;
        }
        log!("action", "started a check-up");
        self.settings.privacy_ack = true;
        self.save_settings();
        let (tx, rx) = channel();
        std::thread::spawn(move || collect::collect(tx));
        self.check_rx = Some(rx);
        self.check_error = None;
        // A queue waiting for the check-up after the restart is picked up only by that one.
        self.resume_check = None;
        self.force_welcome = false;
        self.ui.nav_open = false;
        self.run = Some(run::Run::check(self.page, self.snap.is_none()));
        self.set_mark_kind(mark::Kind::Stones);
        self.mark.start();
    }

    /// Swaps the loader for the other kind while it rests; both rest as the same logo.
    fn set_mark_kind(&mut self, kind: mark::Kind) {
        if self.mark.kind() != kind && !self.mark.running() {
            let reduced = self.mark.reduced;
            self.mark = mark::Mark::new(kind, self.mark.lift());
            self.mark.reduced = reduced;
        }
    }

    /// Where the restore point gate starts for a new queue.
    fn gate_now(&self) -> run::Gate {
        use crate::util::J;
        let restore = self.snap.as_ref().map(|s| s.security.g("restore"));
        let latest = restore.and_then(|r| r.g("Latest").s("When"));
        let on = restore.and_then(|r| r.b("COn"));
        run::Gate::start(self.restore_saved, latest.as_deref(), on, chrono::Local::now().date_naive())
    }

    /// Opens fixes as a queue in place of the page, everything ticked.
    pub(super) fn open_fixes(&mut self, items: Vec<run::Item>, origin: Page) {
        if items.is_empty() || self.run.as_ref().is_some_and(|r| r.running()) {
            return;
        }
        let gate = self.gate_now();
        log!("queue", "opened from {origin:?} with {}", items.iter().map(|i| i.title.as_str()).collect::<Vec<_>>().join(", "));
        self.set_mark_kind(mark::Kind::Healing);
        self.run = Some(run::Run::fixes(items, origin, gate));
        self.ui.nav_open = false;
    }

    fn close_run(&mut self) {
        let Some(q) = self.run.take() else { return };
        log!("queue", "closed while {:?} ({:?})", q.stage, q.mode);
        self.resume_wait = None;
        self.resume_check = None;
        self.go(if q.mode == run::Mode::Check { Page::Home } else { q.origin });
        self.page_fade = Some(self.clock);
    }

    /// Runs the ticked boxes one after another in the background.
    fn start_fixing(&mut self, ctx: &egui::Context) {
        let jobs: Vec<(usize, run::Work, String)> = {
            let Some(q) = self.run.as_mut() else { return };
            if q.stage != run::Stage::Choose || !q.ready() {
                return;
            }
            q.start();
            let left: Vec<&str> = q.items.iter().filter(|it| !it.ticked).map(|it| it.title.as_str()).collect();
            log!(
                "queue",
                "started {} of {}{}",
                q.ticked(),
                q.items.len(),
                if left.is_empty() { String::new() } else { format!(", leaving out {}", left.join(", ")) }
            );
            q.items.iter().enumerate().filter(|(_, it)| it.ticked).map(|(i, it)| (i, it.work.clone(), it.title.clone())).collect()
        };
        let ids: Vec<&'static str> = jobs.iter().flat_map(|(_, w, _)| if let run::Work::Clean(ids) = w { ids.clone() } else { vec![] }).collect();
        let items = self.cleanup_items(&ids);
        self.set_mark_kind(mark::Kind::Healing);
        self.mark.start();
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            for (i, work, title) in jobs {
                let t = Instant::now();
                if matches!(work, run::Work::Step(_)) {
                    continue;
                }
                log!("queue", "{title}: started");
                // A panic inside one box ends that box, not the queue, which would wait for it forever.
                let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match &work {
                    run::Work::Fix(f) => (fixes::apply(f, &title).map_err(|e| format!("{e:#}")), None),
                    _ => {
                        let out = cleanup_page::clean_items(&items, |_| {});
                        let errs: Vec<String> = out.iter().filter_map(|(_, r)| r.as_ref().err().cloned()).collect();
                        (if errs.is_empty() { Ok(()) } else { Err(errs.join(" ")) }, Some(out))
                    }
                }));
                let (r, cleaned) = done.unwrap_or_else(|_| (Err("Bootmend hit a problem of its own with this one.".to_string()), None));
                log!("queue", "{title}: {} after {:.1}s", ended(&r), t.elapsed().as_secs_f32());
                // The window has closed; nobody is left to tell.
                if tx.send(Job::Worked(i, r, t.elapsed().as_secs_f32() * 1000.0, cleaned)).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        });
    }

    fn run_act(&mut self, ctx: &egui::Context, act: run::Act) {
        log!("queue", "pressed {act:?}");
        match act {
            run::Act::Cancel | run::Act::Close => self.close_run(),
            run::Act::SaveRestore if !self.elevated => self.elevate_for(ctx, run::Then::Restore),
            run::Act::SaveRestore => {
                if let Some(q) = &mut self.run {
                    q.gate = run::Gate::Saving;
                }
                self.busy.insert("restore".into());
                self.spawn(ctx, || Job::RestorePoint(fixes::restore_point("Before changes made with Bootmend").map_err(|e| format!("{e:#}"))));
            }
            run::Act::TurnOn if !self.elevated => self.elevate_for(ctx, run::Then::TurnOn),
            run::Act::TurnOn => {
                if let Some(q) = &mut self.run {
                    q.gate = run::Gate::TurningOn;
                }
                self.spawn(ctx, || Job::TurnedOn(fixes::apply(&Fix::EnableSystemProtection, "System Restore is off").map_err(|e| format!("{e:#}"))));
            }
            run::Act::Start if !self.elevated && self.run.as_ref().is_some_and(|q| q.needs_admin()) => self.elevate_for(ctx, run::Then::Start),
            run::Act::Start => self.start_fixing(ctx),
        }
    }

    /// Saves the queue, asks Windows to start Bootmend again as administrator, and closes this
    /// window. The new one picks the queue up and carries on with `then`.
    fn elevate_for(&mut self, ctx: &egui::Context, then: run::Then) {
        let Some(q) = &self.run else { return };
        let saved = run::Resume::of(q, then, unix_now());
        let Some(path) = resume_path() else {
            self.toast("Bootmend couldn't find its own folder, so it can't ask Windows for permission.", false);
            return;
        };
        let written = serde_json::to_vec(&saved).map_err(|e| e.to_string()).and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string()));
        if let Err(e) = written {
            self.toast(format!("Couldn't get ready to ask Windows for permission: {e}"), false);
            return;
        }
        log!("action", "restarting as administrator to carry on with the queue");
        match crate::elevate::relaunch_as_admin("--resume") {
            Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Err(e) => {
                if let Err(e) = std::fs::remove_file(&path) {
                    log!("action", "couldn't remove {}: {e}", path.display());
                }
                self.toast(format!("{e:#}"), false);
            }
        }
    }

    /// Picks up the queue saved before the restart as administrator, if it's recent. A check-up
    /// runs first, and only the fixes it still offers carry on (see `resumed_check`).
    fn resume(&mut self) {
        let Some(path) = resume_path() else { return };
        let Ok(bytes) = std::fs::read(&path) else { return };
        if let Err(e) = std::fs::remove_file(&path) {
            log!("action", "couldn't remove {}: {e}", path.display());
        }
        let saved: run::Resume = match serde_json::from_slice(&bytes) {
            Ok(r) => r,
            Err(e) => {
                log!("action", "the saved queue couldn't be read: {e}");
                return;
            }
        };
        if !saved.fresh(unix_now()) || saved.items.is_empty() {
            return;
        }
        let origin = saved.origin();
        log!("queue", "picked up after restarting as administrator; checking again before going back to {origin:?}");
        if self.page != origin {
            log!("page", "{:?} -> {origin:?}", self.page);
        }
        self.page = origin;
        self.start_check();
        self.resume_check = Some(saved);
    }

    /// The check-up run before picking up a saved queue is over. The fixes it still offers carry
    /// on, and anything else is left out with a line saying so.
    fn resumed_check(&mut self, ctx: &egui::Context, saved: run::Resume) {
        let Some(snap) = self.snap.clone() else {
            log!("queue", "the saved fixes were left out because the check-up left no result");
            return;
        };
        let (items, dropped) = saved.still_offered(&self.findings, &snap);
        let leftovers = saved.leftovers();
        log!("queue", "the new check-up still offers {} of the saved fixes, leaving out {dropped}", items.len());
        if items.is_empty() && leftovers.is_none() {
            self.toast(if dropped == 1 { "The fix you picked isn't needed now." } else { "The fixes you picked aren't needed now." }, true);
            return;
        }
        match dropped {
            0 => {}
            1 => self.toast("One fix you picked isn't needed now, so Bootmend left it out.", true),
            n => self.toast(format!("{n} fixes you picked aren't needed now, so Bootmend left them out."), true),
        }
        let origin = saved.origin();
        let gate = self.gate_now();
        self.set_mark_kind(mark::Kind::Healing);
        self.run = Some(run::Run::fixes(items, origin, gate));
        self.resume_wait = Some((saved.then, leftovers));
        if leftovers.is_some() {
            self.start_cleanup_look(ctx);
        } else {
            self.resume_then(ctx);
        }
    }

    /// The leftover files were measured again: they go back into the picked-up queue.
    fn resumed_leftovers(&mut self, ctx: &egui::Context) {
        let Some((_, Some(ticked))) = self.resume_wait else { return };
        let choosing = self.run.as_ref().is_some_and(|q| q.stage == run::Stage::Choose);
        if let (Some(mut it), true) = (self.leftovers_item(), choosing) {
            it.ticked = ticked;
            if let Some(q) = &mut self.run {
                q.items.push(it);
            }
        }
        self.resume_then(ctx);
    }

    /// Carries on with what was pressed before the restart, now that there's permission.
    fn resume_then(&mut self, ctx: &egui::Context) {
        let Some((then, _)) = self.resume_wait.take() else { return };
        let Some(q) = self.run.as_ref().filter(|q| q.stage == run::Stage::Choose) else { return };
        let act = match then {
            run::Then::Restore if q.gate != run::Gate::Saved => Some(run::Act::SaveRestore),
            run::Then::TurnOn if q.gate == run::Gate::Off => Some(run::Act::TurnOn),
            run::Then::Start if q.ready() => Some(run::Act::Start),
            _ => None,
        };
        // Without the rights after all, it waits for a press rather than asking again by itself.
        if let (Some(a), true) = (act, self.elevated) {
            self.run_act(ctx, a);
        }
    }

    /// Moves the queue, the loader and the taskbar on by one frame.
    fn tick(&mut self, ctx: &egui::Context) {
        // Screenshots are taken with everything standing still.
        let dt = if self.snap_mode.is_some() { 0.0 } else { (ctx.input(|i| i.stable_dt) * 1000.0).min(50.0) };
        let reduced = !theme::motion(ctx);
        self.clock += dt;
        self.mark.reduced = reduced;
        let over = self.run.as_mut().and_then(|q| {
            q.reduced = reduced;
            q.step(dt).map(|ok| (q.mode, ok))
        });
        if let Some((mode, ok)) = over {
            // The keystone seats only when the work went well and nothing that matters is left.
            let all = match mode {
                run::Mode::Check => {
                    if let Some(s) = self.run.as_mut().and_then(|q| q.finished.take()) {
                        self.set_snapshot(*s);
                    }
                    !self.raised()
                }
                run::Mode::Fix => ok && !self.raised(),
            };
            self.mark.finish(all);
        }
        if let Some(q) = self.run.as_ref().filter(|q| q.running()) {
            self.mark.set(q.p);
        }
        let ev = self.mark.step(dt);
        if let Some(q) = &mut self.run {
            if ev.ticked {
                q.ticked_now();
            }
            if ev.rested {
                q.rested();
            }
        }
        if ev.rested && self.run.as_ref().is_some_and(|q| q.mode == run::Mode::Check && q.stage == run::Stage::Over) {
            if let Some(saved) = self.resume_check.take() {
                self.resumed_check(ctx, saved);
            }
        }
        if let Some(tb) = &mut self.taskbar {
            tb.show(self.run.as_ref().filter(|q| q.running()).map(|q| q.p));
        }
        if self.mark.moving() || self.run.as_ref().is_some_and(|q| q.moving()) || self.flight.from.is_some() || self.page_fade.is_some() {
            ctx.request_repaint();
        }
    }

    /// The Queue screen over the whole central panel.
    fn run_screen(&mut self, ui: &mut egui::Ui, margin: f32) -> Option<run::Act> {
        let esc = !self.ui.nav_open && self.confirm.is_none();
        let elevated = self.elevated;
        // Nothing here scrolls, so a screenshot of it is always one part.
        if let Some(m) = &mut self.snap_mode {
            m.more = false;
        }
        let q = self.run.as_mut()?;
        // In a scope of its own, so its fade-in leaves the logo alone.
        let (slot, act) = ui.scope_builder(egui::UiBuilder::new().max_rect(ui.max_rect()), |ui| q.ui(ui, margin, elevated, esc)).inner;
        self.ui.mark_slot = Some((Slot::Run, slot));
        act
    }

    /// Draws the logo wherever this frame asked for it, flying it there from where it was.
    fn draw_mark(&mut self, ui: &egui::Ui) {
        let Some((owner, rect)) = self.ui.mark_slot else { return };
        let jump = self.snap_mode.is_some() || !theme::motion(ui.ctx());
        let r = self.flight.place(owner, rect, self.clock, ui.ctx().cumulative_pass_nr(), jump);
        self.mark.show(ui.painter(), r, egui::Id::new("bootmend-mark"));
    }

    fn go(&mut self, p: Page) {
        if self.page != p {
            log!("page", "{:?} -> {p:?}", self.page);
            // So a point Windows saved meanwhile, or one that failed to read, is read again.
            if p == Page::History && !self.busy.contains("points") {
                self.points = None;
            }
        }
        self.page = p;
        self.ui.last_tab.insert(p.place(), p);
        self.ui.scroll_top = true;
    }

    /// Opens a place at the tab last used there. Home always opens on its plan. A queue that
    /// isn't running closes.
    fn go_place(&mut self, pl: Place) {
        if self.run.as_ref().is_some_and(|q| !q.running()) {
            self.close_run();
        }
        let p = if pl == Place::Home { Page::Home } else { self.ui.last_tab.get(&pl).copied().unwrap_or(pl.first()) };
        self.go(p);
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
            Pending::Fix(f, title) => {
                let mut body = vec![f.describe(), format!("This is for: {title}.")];
                if self.restore_point_first() {
                    body.push("Bootmend saves a restore point first, so the whole PC can be rolled back if anything goes wrong. That can take up to a minute.".into());
                }
                Confirm { title: f.label().to_string(), body, yes: f.label().to_string(), action }
            }
            Pending::Undo(_, title) => Confirm { title: "Undo this change?".into(), body: vec![format!("Bootmend will put back how things were before \"{title}\".")], yes: "Undo".into(), action },
            Pending::Upgrade(_, name) => Confirm {
                title: format!("Update {name}?"),
                body: vec![
                    "winget, Windows' own app installer, will download the newest version from the app's publisher and install it quietly.".into(),
                    "Close the app first if it's open. Updates can't be undone from Bootmend.".into(),
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
                    "Bootmend will ask winget, Windows' own app installer, which of your apps have newer versions.".into(),
                    "winget compares your installed apps with Microsoft's public app catalog online. Bootmend itself sends nothing anywhere.".into(),
                ],
                yes: "Check now".into(),
                action,
            },
            Pending::AnalyzeDumps => Confirm {
                title: "Read the crash files?".into(),
                body: vec![
                    "Bootmend will open each crash file with Microsoft's debugger to see which driver was involved.".into(),
                    "The first time, the debugger downloads matching debugging files from Microsoft (often 50 to 200 MB). It asks for them by driver name and version; the crash files themselves stay on this PC.".into(),
                    "This can take several minutes.".into(),
                ],
                yes: "Read crash files".into(),
                action,
            },
            Pending::Elevate => Confirm {
                title: "Restart as administrator?".into(),
                body: vec![
                    "Windows will ask for permission, then Bootmend opens again with access to a few more details: Windows Security's skip list, crash files and some drive health data.".into(),
                    "Administrator rights are used for reading. Fixes still wait for you.".into(),
                ],
                yes: "Restart".into(),
                action,
            },
            Pending::RestorePoint => Confirm {
                title: "Save a restore point?".into(),
                body: vec![
                    "Windows saves a snapshot of its settings, drivers and installed apps. Your own files aren't part of it and aren't touched.".into(),
                    "If a later change causes trouble, you can roll the PC back from System Restore. Saving takes a minute or two.".into(),
                ],
                yes: "Save".into(),
                action,
            },
            Pending::Clean(ids) => {
                let (items, bytes, unknown) = match &self.cleanup {
                    Some(v) => {
                        let sel: Vec<&crate::cleanup::Item> = v.found.items.iter().filter(|i| ids.contains(&i.id)).collect();
                        let (b, u) = cleanup_page::total(&sel);
                        (sel.into_iter().cloned().collect::<Vec<_>>(), b, u)
                    }
                    None => (vec![], 0, false),
                };
                let size = crate::cleanup::size_text(bytes);
                let mut body = vec![format!(
                    "{} will be cleared: about {size}{}.",
                    rules::plural(items.len(), "item", "items"),
                    if unknown { ", plus what Windows' own clean-up finds" } else { "" }
                )];
                for i in items.iter().filter(|i| i.tradeoff.is_some()) {
                    body.push(format!("{}: {}", i.title, i.tradeoff.unwrap_or_default()));
                }
                let open: Vec<String> = items
                    .iter()
                    .filter_map(|i| i.close_first.as_ref())
                    .filter(|(_, procs)| !crate::cleanup::running(procs).is_empty())
                    .map(|(name, _)| name.to_string())
                    .collect();
                if !open.is_empty() {
                    body.push(format!("Close {} first. Anything still open is skipped.", open.join(", ")));
                }
                if items.iter().any(|i| i.id == "installers") {
                    body.push("Old installers go to the Recycle Bin. Everything else is deleted for good, and the apps rebuild what they need.".into());
                } else {
                    body.push("These are deleted for good, and the apps rebuild what they need.".into());
                }
                Confirm { title: format!("Free up {size}?"), body, yes: "Free up space".into(), action }
            }
            Pending::UpdateBootmend(v) => Confirm {
                title: format!("Update to Bootmend {v}?"),
                body: vec!["Bootmend downloads it from GitHub, checks it's signed by Bootmend, then restarts.".into()],
                yes: "Update and restart".into(),
                action,
            },
            Pending::NetCheck => Confirm {
                title: "Test your connection?".into(),
                body: vec![
                    "Bootmend pings your router, times a few website name lookups, then times how quickly Microsoft's website (www.microsoft.com) answers.".into(),
                    "It uses very little data and takes a few seconds. Like any website, Microsoft sees this connection's internet address. Nothing about the PC is sent.".into(),
                ],
                yes: "Test now".into(),
                action,
            },
            Pending::EmptyRecycleBin => {
                let (b, n) = self.cleanup.as_ref().and_then(|v| v.found.recycle).unwrap_or((0, 0));
                Confirm {
                    title: "Empty the Recycle Bin?".into(),
                    body: vec![
                        format!("{} ({}) will be deleted for good.", rules::plural(n as usize, "item", "items"), crate::cleanup::size_text(b)),
                        "This can't be undone. To check what's in it first, use Look inside first.".into(),
                    ],
                    yes: "Empty it".into(),
                    action,
                }
            }
            Pending::DeleteData => {
                let mut body = vec!["This removes all saved checks, the undo history and settings from this PC. Changes you already made to Windows stay as they are.".into()];
                if !self.elevated && fixes::admin_records_kept() {
                    body.push("Undo records for changes made as administrator stay until you do this from an administrator window.".into());
                }
                body.push("This can't be undone.".into());
                Confirm { title: "Delete everything Bootmend saved?".into(), body, yes: "Delete".into(), action }
            }
        };
        log!("dialog", "shown: {}", c.title);
        self.confirm = Some(c);
    }

    fn run(&mut self, ctx: &egui::Context, action: Pending) {
        let what = match &action {
            Pending::Fix(f, title) => format!("fix {f:?} ({title})"),
            Pending::Undo(i, title) => format!("undo change {i} ({title})"),
            Pending::Upgrade(id, _) => format!("update the app {id}"),
            Pending::Scan(kind, target) => {
                format!("{} scan{}", kind.label().to_lowercase(), target.as_ref().map(|t| format!(" of {}", t.display())).unwrap_or_default())
            }
            Pending::CheckUpdates => "check for app updates".into(),
            Pending::AnalyzeDumps => "read the crash files".into(),
            Pending::Elevate => "restart as administrator".into(),
            Pending::DeleteData => "delete everything Bootmend saved".into(),
            Pending::RestorePoint => "save a restore point".into(),
            Pending::Clean(ids) => format!("clear {}", ids.join(", ")),
            Pending::EmptyRecycleBin => "empty the Recycle Bin".into(),
            Pending::NetCheck => "test the connection".into(),
            Pending::UpdateBootmend(v) => format!("update Bootmend to {v}"),
        };
        log!("action", "{what}");
        match action {
            Pending::Fix(fix, title) => {
                self.busy.insert(format!("fix:{fix:?}"));
                let first = self.restore_point_first();
                self.restore_saved |= first;
                if first {
                    // Cleared with the fix's own marker when the job ends.
                    self.busy.insert("fix:restore".into());
                }
                self.spawn(ctx, move || {
                    let note = first.then(|| match fixes::restore_point("Before changes made with Bootmend") {
                        Ok(()) => "A restore point was saved first.".to_string(),
                        Err(e) => format!("No restore point was saved first: {e:#}."),
                    });
                    Job::Fixed(title.clone(), fixes::apply(&fix, &title).map_err(|e| format!("{e:#}")), note)
                });
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
            Pending::Clean(ids) => self.start_cleaning(ctx, ids),
            Pending::NetCheck => self.start_net_check(ctx),
            Pending::UpdateBootmend(v) => self.start_update(ctx, v),
            Pending::EmptyRecycleBin => {
                self.busy.insert("recycle".into());
                self.spawn(ctx, || Job::RecycleEmptied(crate::cleanup::empty_recycle_bin().map_err(|e| format!("{e:#}"))));
            }
            Pending::RestorePoint => {
                self.busy.insert("restore".into());
                self.spawn(ctx, || Job::RestorePoint(fixes::restore_point("Saved with Bootmend").map_err(|e| format!("{e:#}"))));
            }
            Pending::DeleteData => {
                self.busy.insert("delete".into());
                self.spawn(ctx, || {
                    let r = data_dir("")
                        .map_err(|e| format!("{e:#}"))
                        .and_then(|d| std::fs::remove_dir_all(&d).map_err(|e| format!("{e}")))
                        .and_then(|()| fixes::forget_changes().map_err(|e| format!("{e:#}")));
                    Job::Deleted(r)
                });
            }
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let msgs = self.check_rx.as_ref().map(drain_check).unwrap_or_default();
        for m in msgs {
            match m {
                Progress::StepDone { key, ok } => {
                    if let Some(q) = &mut self.run {
                        q.on_step(key, ok);
                    }
                }
                Progress::Finished(s) => {
                    self.check_rx = None;
                    if let Some(q) = &mut self.run {
                        q.on_finished(s);
                    }
                }
                Progress::Failed(e) => {
                    log!("check", "the check-up didn't finish: {e}");
                    if self.resume_check.take().is_some() {
                        log!("queue", "the saved fixes were left out because the check-up didn't finish");
                    }
                    self.check_rx = None;
                    self.run = None;
                    let reduced = self.mark.reduced;
                    self.mark = mark::Mark::new(mark::Kind::Stones, self.mark.lift());
                    self.mark.reduced = reduced;
                    if self.snap.is_some() {
                        self.toast(format!("The check didn't finish: {e}"), false);
                    }
                    self.check_error = Some(e);
                }
            }
        }
        while let Ok(j) = self.rx.try_recv() {
            match j {
                Job::Fixed(title, r, note) => {
                    log!("action", "fix \"{title}\" {}{}", ended(&r), note.as_deref().map(|n| format!(" ({n})")).unwrap_or_default());
                    self.busy.retain(|b| !b.starts_with("fix:"));
                    self.history = fixes::history();
                    let note = note.map(|n| format!(" {n}")).unwrap_or_default();
                    match r {
                        Ok(()) => self.toast(format!("Done: {title}.{note} You can undo it from History."), true),
                        Err(e) => self.toast(format!("That didn't work: {e}{note}"), false),
                    }
                }
                Job::RestorePoint(r) => {
                    log!("action", "restore point {}", ended(&r));
                    self.busy.remove("restore");
                    // History reads them again, so a point saved just now is listed.
                    self.points = None;
                    // Saved for the queue, the queue says so; saved from the Security page, a toast does.
                    let gate = self.run.as_mut().filter(|q| q.gate == run::Gate::Saving).map(|q| {
                        q.gate = run::Gate::after(&r);
                        q.gate == run::Gate::Saved
                    });
                    if gate == Some(true) {
                        self.restore_saved = true;
                    }
                    match r {
                        Ok(()) => {
                            if gate.is_none() {
                                self.toast("Restore point saved.", true);
                            }
                            self.restore_saved = true;
                            self.update_snapshot(|s| {
                                if let Some(r) = s.security.get_mut("restore").and_then(|r| r.as_object_mut()) {
                                    r.insert("Latest".into(), serde_json::json!({"When": crate::util::now_stamp(), "What": "Saved with Bootmend"}));
                                }
                            });
                        }
                        Err(e) if gate.is_none() => self.toast(format!("Couldn't save a restore point: {e}"), false),
                        Err(_) => {}
                    }
                }
                Job::TurnedOn(r) => {
                    log!("action", "turning System Restore on {}", ended(&r));
                    self.history = fixes::history();
                    match r {
                        Ok(()) => {
                            self.update_snapshot(|s| {
                                if let Some(r) = s.security.get_mut("restore").and_then(|r| r.as_object_mut()) {
                                    r.insert("COn".into(), serde_json::Value::Bool(true));
                                }
                            });
                            if let Some(q) = self.run.as_mut().filter(|q| q.stage == run::Stage::Choose) {
                                q.gate = run::Gate::Needed;
                                // Turning it on was this fix, so it's done already.
                                q.items.retain(|i| i.work != run::Work::Fix(Fix::EnableSystemProtection));
                            }
                        }
                        Err(e) => {
                            if let Some(q) = self.run.as_mut().filter(|q| q.stage == run::Stage::Choose) {
                                q.gate = run::Gate::Off;
                            }
                            self.toast(format!("Couldn't turn System Restore on: {e}"), false);
                        }
                    }
                }
                Job::Worked(i, r, ms, cleaned) => {
                    if let Some(c) = cleaned {
                        self.apply_cleaned(c);
                    }
                    self.history = fixes::history();
                    if let Some(q) = &mut self.run {
                        q.on_result(i, r, ms);
                    }
                }
                Job::Undone(r) => {
                    log!("action", "undo {}", ended(&r));
                    self.busy.retain(|b| !b.starts_with("undo:"));
                    self.history = fixes::history();
                    match r {
                        Ok(()) => self.toast("Undone. Things are back how they were.", true),
                        Err(e) => self.toast(format!("Couldn't undo that: {e}"), false),
                    }
                }
                Job::Scanned(r) => {
                    match &r {
                        Ok(res) => log!("action", "{} finished with {}", res.kind, rules::plural(res.threats.len(), "threat", "threats")),
                        Err(e) => log!("action", "the scan failed: {e}"),
                    }
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
                    match &r {
                        Ok(list) => log!("action", "{} have updates available", rules::plural(list.len(), "app", "apps")),
                        Err(e) => log!("action", "checking for app updates failed: {e}"),
                    }
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
                    log!("action", "update of {id} {}", ended(&r));
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
                    log!("action", "read {}, {} couldn't be read", rules::plural(found.len(), "crash file", "crash files"), errs.len());
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
                    log!("action", "saving the report {}", ended(&r));
                    self.busy.remove("report");
                    match r {
                        Ok(p) => {
                            self.toast(format!("Saved to {}", p.display()), true);
                            self.ui.last_report = Some(p);
                        }
                        Err(e) => self.toast(format!("Couldn't save the report: {e}"), false),
                    }
                }
                Job::CleanupFound(r) => {
                    self.cleanup_found(r);
                    self.resumed_leftovers(ctx);
                }
                Job::Cleaned(r) => self.cleaned(r),
                Job::RecycleEmptied(r) => self.recycle_emptied(r),
                Job::Scheduled(every, hour, r) => self.scheduled(every, hour, r),
                Job::Trends(t) => {
                    log!("action", "loaded {}", rules::plural(t.len(), "trend chart", "trend charts"));
                    self.busy.remove("trends");
                    self.trends = t;
                }
                Job::CopyLooked(r) => self.on_looked(r),
                Job::Copied(r) => {
                    if let Err(e) = &r {
                        log!("action", "copy stopped: {e}");
                    }
                    self.on_copied(r);
                }
                Job::Points(r) => {
                    match &r {
                        Ok(v) => log!("action", "read {}", rules::plural(v.len(), "restore point", "restore points")),
                        Err(e) => log!("action", "couldn't read restore points: {e}"),
                    }
                    self.busy.remove("points");
                    self.points = Some(r);
                }
                Job::NetChecked(c) => {
                    log!("action", "the connection check {}", if c.internet_ms.is_some() { "finished" } else { "couldn't finish" });
                    self.busy.remove("net");
                    self.toast(
                        if c.internet_ms.is_some() { "Connection check finished." } else { "The connection check couldn't finish. The Network page says why." },
                        c.internet_ms.is_some(),
                    );
                    self.net = Some(c);
                }
                Job::UpdateChecked(r, manual) => self.on_update_checked(r, manual),
                Job::Updated(r) => self.on_updated(ctx, r),
                Job::Deleted(r) => {
                    log!("action", "deleting everything saved {}", ended(&r));
                    self.busy.remove("delete");
                    match r {
                        Ok(()) => {
                            self.snap = None;
                            self.findings.clear();
                            self.changes.clear();
                            self.history.clear();
                            self.settings = Settings::default();
                            self.toast("Everything Bootmend saved has been deleted.", true);
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

    /// The places. `overlay` is the folded sidebar opened over the page: it starts with the
    /// menu button that closes it again, and closes when a place is picked.
    fn sidebar(&mut self, ui: &mut egui::Ui, overlay: bool) {
        // While work runs, the places wait.
        if self.run.as_ref().is_some_and(|q| q.running()) {
            ui.disable();
        }
        if overlay {
            // The close button sits exactly where the menu button that opened the pane is, as in
            // WinUI, so the same spot opens and closes it.
            let at = self.ui.nav_button.unwrap_or_else(|| egui::Rect::from_min_size(ui.cursor().min + egui::vec2(0.0, S2), egui::vec2(32.0, 32.0)));
            let r = ui.scope_builder(egui::UiBuilder::new().max_rect(at), |ui| widgets::subtle_icon(ui, icon::MENU, 16.0, "Close navigation")).inner;
            if r.clicked() {
                self.ui.nav_open = false;
                self.ui.nav_refocus = true;
            }
            ui.add_space(S2);
        } else {
            ui.add_space(S4);
        }
        ui.horizontal(|ui| {
            ui.add_space(S3);
            mark::sidebar(ui, 24.0);
            ui.add_space(S1);
            ui.label(RichText::new("Bootmend").text_style(theme::title()));
        });
        ui.add_space(S5);
        let do_first = self.findings.iter().filter(|f| f.level == rules::Level::DoFirst).count();
        let here = self.page.place();
        ui.spacing_mut().item_spacing.y = S1;
        // Opened from the menu button, keyboard focus moves to the place on screen.
        let focus = std::mem::take(&mut self.ui.nav_focus);
        let pick = |this: &mut Self, r: egui::Response, pl: Place| {
            if focus && here == pl {
                r.request_focus();
            }
            if r.clicked() {
                this.go_place(pl);
                this.ui.nav_open = false;
            }
        };
        for pl in Place::MAIN {
            let badge = (pl == Place::Home).then_some(do_first);
            let r = widgets::nav_item(ui, pl.icon(), pl.title(), here == pl, badge);
            pick(self, r, pl);
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.add_space(S3);
            let pl = Place::Settings;
            let r = widgets::nav_item(ui, pl.icon(), pl.title(), here == pl, None);
            pick(self, r, pl);
        });
    }

    /// The folded sidebar, sliding in over the page from the left. While it's open the page
    /// under it takes no clicks or focus, so Tab stays inside it. A click outside it, Escape or
    /// picking a place closes it.
    fn nav_overlay(&mut self, ctx: &egui::Context) {
        let open = self.ui.nav_open;
        let t = if self.snap_mode.is_some() || !theme::motion(ctx) {
            f32::from(u8::from(open))
        } else {
            ctx.animate_bool_with_time(egui::Id::new("nav-slide"), open, 0.2)
        };
        if t <= 0.0 {
            return;
        }
        let p = pal(ctx);
        let screen = ctx.content_rect();
        // Eases out: quick to arrive, gentle to stop.
        let ease = 1.0 - (1.0 - t).powi(3);
        let rect = egui::Rect::from_min_size(egui::pos2(screen.left() - SIDEBAR_W * (1.0 - ease), screen.top()), egui::vec2(SIDEBAR_W, screen.height()));
        let id = egui::Id::new("nav-pane");
        // The pane slides in; egui's own fade on a new Area would leave it see-through.
        egui::Area::new(id).order(egui::Order::Foreground).fixed_pos(rect.min).constrain(false).movable(false).fade_in(false).show(ctx, |ui| {
            let shadow = egui::Shadow { offset: [4, 0], blur: 24, spread: 0, color: egui::Color32::from_black_alpha(if p.bg.r() < 128 { 96 } else { 40 }) };
            ui.painter().add(shadow.as_shape(rect, egui::CornerRadius::ZERO));
            ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, p.sidebar);
            ui.painter().vline(rect.right(), rect.y_range(), egui::Stroke::new(1.0_f32, p.line));
            let mut pane = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(S2, 0.0))));
            self.sidebar(&mut pane, true);
            // The Area covers the whole pane, so a click on its empty parts stays in it.
            ui.allocate_rect(rect, egui::Sense::hover());
        });
        if open {
            ctx.memory_mut(|m| m.set_modal_layer(egui::LayerId::new(egui::Order::Foreground, id)));
        }
        // Only once it's fully open: the click that opened it lands outside it while it slides in.
        let outside = t >= 1.0 && ctx.input(|i| i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|q| !rect.contains(q)));
        if outside {
            self.ui.nav_open = false;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.ui.nav_open = false;
            self.ui.nav_refocus = true;
        }
    }

    /// The page title, the place's quiet actions, its tabs and a line on what this view shows.
    fn header(&mut self, ui: &mut egui::Ui, title: &str, subtitle: &str) {
        let p = pal(ui.ctx());
        let place = self.page.place();
        let crumb = self.page.inner();
        let fold = !self.ui.docked;
        let compact = ui.available_width() < COMPACT;
        let draw_title = |this: &mut Self, ui: &mut egui::Ui| {
            if fold {
                let r = widgets::subtle_icon(ui, icon::MENU, 16.0, "Open navigation");
                this.ui.nav_button = Some(r.rect);
                // Once the pane has gone and stopped holding focus, it comes back here.
                if this.ui.nav_refocus && !this.ui.nav_open && ui.ctx().memory(|m| m.top_modal_layer()).is_none() {
                    this.ui.nav_refocus = false;
                    r.request_focus();
                }
                if r.clicked() {
                    this.ui.nav_open = true;
                    this.ui.nav_focus = true;
                }
                ui.add_space(S1);
            }
            if crumb {
                // Short of room the way back folds to an ellipsis, as WinUI's BreadcrumbBar does.
                let words = if compact { "…" } else { place.title() };
                let back = egui::Button::new(RichText::new(words).text_style(egui::TextStyle::Heading).color(p.muted)).frame(false);
                let r = ui.add(back).on_hover_cursor(egui::CursorIcon::PointingHand);
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, place.title()));
                let r = if compact { r.on_hover_text(place.title()) } else { r };
                if r.clicked() {
                    this.go(place.first());
                }
                ui.add(widgets::Glyph(widgets::ico(icon::CHEVRON_RIGHT, 16.0).color(p.muted)));
            }
            if !title.is_empty() {
                widgets::page_title(ui, title);
            }
        };
        // One row, as in Settings: the menu button and title on the left, the place's actions on
        // the right. When room runs short the actions keep their icons and drop their words.
        ui.horizontal(|ui| {
            draw_title(self, ui);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| self.header_actions(ui, place, compact));
        });
        if compact {
            if let Some(w) = self.checked_words(place) {
                widgets::small(ui, w);
            }
        }
        let tabs = place.tabs();
        if let Some(cur) = tabs.iter().position(|t| *t == self.page) {
            ui.add_space(S2);
            let labels: Vec<&str> = tabs.iter().map(|t| t.title()).collect();
            if let Some(i) = widgets::selector_bar(ui, &labels, cur) {
                self.go(tabs[i]);
            }
        }
        if !subtitle.is_empty() {
            ui.add_space(S3);
            ui.label(RichText::new(subtitle).color(p.muted));
        }
        ui.add_space(S4);
        // Only where the missing details would show up; elsewhere the note is noise.
        if !self.elevated && !self.ui.banner_closed && matches!(self.page, Page::Home | Page::Security | Page::Crashes | Page::Storage) {
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

    /// When the check ran, said on My PC. Home says it beside the verdict.
    fn checked_words(&self, place: Place) -> Option<String> {
        let s = self.snap.as_ref().filter(|_| place == Place::MyPc)?;
        Some(format!("Checked {}", crate::util::when_words(&s.taken, chrono::Local::now().date_naive())))
    }

    /// Home and My PC can run the check again, and Home can save a report. They sit at the right
    /// edge, so they're added right to left. `compact` shows them as icons alone, and leaves
    /// "Checked ..." to a line under the title.
    fn header_actions(&mut self, ui: &mut egui::Ui, place: Place, compact: bool) {
        if !matches!(place, Place::Home | Place::MyPc) || self.page.inner() {
            return;
        }
        let button = |ui: &mut egui::Ui, ic: &str, text: &str| {
            if compact {
                widgets::subtle_icon(ui, ic, 16.0, text)
            } else {
                widgets::subtle_button(ui, ic, text)
            }
        };
        let run = button(ui, icon::REFRESH, "Check again").on_hover_text("Run every check again. Takes about a minute.").clicked();
        let save = place == Place::Home && button(ui, icon::REPORT, "Save a report").clicked();
        if !compact {
            if let Some(w) = self.checked_words(place) {
                widgets::small(ui, w);
            }
        }
        if run {
            self.start_check();
        }
        if save {
            self.go(Page::Report);
        }
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(c) = &self.confirm else { return };
        let (title, body, yes) = (c.title.clone(), c.body.clone(), c.yes.clone());
        let mut decision: Option<bool> = None;
        // Windows dialogs keep 24 px clear around their content.
        let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::same(S5 as i8));
        let m = egui::Modal::new(egui::Id::new("confirm")).frame(frame).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 2.0 * (S5 + S4)).min(440.0));
            let t = ui.label(RichText::new(&title).text_style(theme::h2()));
            widgets::heading_role(ui, &t, 2, &title);
            ui.add_space(S2);
            for b in &body {
                ui.label(b);
                ui.add_space(theme::S1);
            }
            ui.add_space(S4);
            widgets::right_row(ui, |ui| {
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
            log!("dialog", "{}: {title}", if d { "accepted" } else { "cancelled" });
            let c = self.confirm.take();
            if let (true, Some(c)) = (d, c) {
                self.run(ctx, c.action);
            }
        }
    }

    /// Says Bootmend is an early version and where to report problems.
    fn early_notice(&mut self, ctx: &egui::Context) {
        if !self.early_notice || self.confirm.is_some() || self.copy.is_some() {
            return;
        }
        let (mut close, mut hide, mut report) = (false, false, false);
        let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::same(S5 as i8));
        let m = egui::Modal::new(egui::Id::new("early")).frame(frame).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 2.0 * (S5 + S4)).min(440.0));
            let title = "Bootmend is new";
            let t = ui.label(RichText::new(title).text_style(theme::h2()));
            widgets::heading_role(ui, &t, 2, title);
            ui.add_space(S2);
            ui.add(egui::Label::new("This is an early version, so some things may not work quite right. If you find a problem, please tell us on GitHub. You can report it, or fix it and send a pull request.").wrap());
            ui.add_space(S4);
            // The quiet choice sits at the left, or under the buttons when the window is narrow.
            let narrow = ui.available_width() < 360.0;
            ui.horizontal(|ui| {
                if !narrow {
                    hide = widgets::subtle_text(ui, "Don't show again").clicked();
                }
                widgets::right_row(ui, |ui| {
                    let ok = widgets::primary(ui, "Got it");
                    if ui.memory(|m| m.focused().is_none()) {
                        ok.request_focus();
                    }
                    close = ok.clicked();
                    report = widgets::secondary(ui, "Report a problem").clicked();
                });
            });
            if narrow {
                ui.add_space(S2);
                hide = widgets::subtle_text(ui, "Don't show again").clicked();
            }
        });
        if !(close || hide || report || m.should_close()) {
            return;
        }
        log!(
            "dialog",
            "early-version notice: {}",
            if hide {
                "won't show again"
            } else if report {
                "report a problem"
            } else {
                "got it"
            }
        );
        self.early_notice = false;
        if hide {
            self.settings.early_notice_off = true;
            self.save_settings();
        }
        if report {
            self.report_problem();
        }
    }

    /// Opens GitHub's bug form in the browser.
    fn report_problem(&mut self) {
        log!("action", "pressed Report a problem");
        if let Err(e) = shell_open(&report_url()) {
            log!("action", "couldn't open the bug form: {e:#}");
            self.toast(format!("Couldn't open your browser. Problems can be reported at {}/issues.", env!("CARGO_PKG_REPOSITORY")), false);
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
                        ui.set_max_width((ctx.content_rect().width() - 2.0 * (S5 + S4)).min(420.0));
                        ui.horizontal(|ui| {
                            ui.add(widgets::Glyph(ico(ic, 16.0).color(fg)));
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
        let live_pending = self.page == Page::Live && self.live_data().is_none();
        let mut open_dialog = false;
        let Some(m) = &mut self.snap_mode else { return };
        if let Some(img) = shot {
            let name = match m.queue.first() {
                Some(Shot::View(p)) => p.slug(),
                Some(Shot::Menu) => "menu".to_string(),
                Some(Shot::Dialog) => "dialog".to_string(),
                Some(Shot::Early) => "early".to_string(),
                Some(Shot::Queue) => "queue".to_string(),
                Some(Shot::Fixing) => "fixing".to_string(),
                Some(Shot::Details) => "fixing-details".to_string(),
                Some(Shot::Copy(n)) => format!("copy-{}", ["pick", "copying", "done"][usize::from(*n).min(2)]),
                _ => "welcome".to_string(),
            };
            let file = m.dir.join(if m.part == 0 { format!("{name}.png") } else { format!("{name}-{}.png", m.part + 1) });
            if let Err(e) = save_png(&img, &file) {
                log!("window", "screenshot {}: {e}", file.display());
            }
            if m.more && m.part < 8 {
                m.part += 1;
            } else {
                if matches!(m.queue.first(), Some(Shot::Dialog)) {
                    self.confirm = None;
                }
                if matches!(m.queue.first(), Some(Shot::Early)) {
                    self.early_notice = false;
                }
                if matches!(m.queue.first(), Some(Shot::Copy(_))) {
                    self.copy = None;
                }
                if matches!(m.queue.first(), Some(Shot::Queue | Shot::Fixing | Shot::Details)) {
                    self.run = None;
                }
                m.queue.remove(0);
                m.part = 0;
            }
            m.more = false;
            m.frames = 0;
            m.asked = false;
        }
        match m.queue.first().copied() {
            None => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Some(Shot::Menu) if self.ui.docked => {
                m.queue.remove(0);
                ctx.request_repaint();
            }
            Some(target) => {
                self.force_welcome = matches!(target, Shot::Welcome);
                self.ui.nav_open = matches!(target, Shot::Menu);
                let page = match target {
                    Shot::View(p) => Some(p),
                    Shot::Menu | Shot::Early | Shot::Queue | Shot::Fixing | Shot::Details | Shot::Copy(_) => Some(Page::Home),
                    Shot::Dialog => Some(Page::Crashes),
                    Shot::Welcome => None,
                };
                if self.run.is_none() {
                    let stage = if matches!(target, Shot::Queue) { run::Stage::Choose } else { run::Stage::Running };
                    if matches!(target, Shot::Queue | Shot::Fixing | Shot::Details) {
                        let q = run::sample(stage, matches!(target, Shot::Details));
                        self.mark = mark::Mark::new(mark::Kind::Healing, 1.0);
                        if stage == run::Stage::Running {
                            self.mark.start();
                            for _ in 0..90 {
                                self.mark.set(q.p);
                                self.mark.step(16.0);
                            }
                        }
                        self.run = Some(q);
                    }
                }
                open_dialog = matches!(target, Shot::Dialog) && self.confirm.is_none();
                if matches!(target, Shot::Early) {
                    self.early_notice = true;
                }
                if let (Shot::Copy(n), None) = (target, &self.copy) {
                    self.copy = Some(copy_dialog::Flow::sample(n));
                }
                if let Some(p) = page {
                    if self.page != p {
                        self.page = p;
                        self.ui.scroll_top = true;
                    }
                }
                // Wait for background work the page started (such as measuring for cleanup) before capturing.
                let waiting = !self.busy.is_empty() || live_pending;
                if !waiting {
                    m.frames += 1;
                }
                if m.frames > 6 && !m.asked {
                    m.asked = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                }
                ctx.request_repaint();
            }
        }
        if open_dialog {
            self.ask(Pending::AnalyzeDumps);
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
    /// With Mica on, whatever egui leaves unpainted shows the backdrop.
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        if self.mica {
            [0.0; 4]
        } else {
            visuals.panel_fill.to_normalized_gamma_f32()
        }
    }

    /// Runs before every frame, and also while the window is hidden whenever something asks
    /// for a repaint, such as a check finishing.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        self.sync_accent(ctx);
        self.sync_motion(ctx);
        self.snap_step(ctx);
        self.tick(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let p = pal(&ctx);
        let fit = fit_for(ui.available_width());
        self.ui.docked = fit.docked;
        if fit.docked {
            self.ui.nav_open = false;
            self.ui.nav_refocus = false;
        }

        self.ui.mark_slot = None;
        let mut act = None;
        // The first check-up has the window to itself, like the welcome before it.
        let full_screen = self.run.as_ref().map_or(self.snap.is_none() || self.force_welcome, |q| q.first);
        if full_screen {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg)).show(ui, |ui| {
                if self.run.is_some() {
                    act = self.run_screen(ui, fit.margin);
                } else {
                    egui::Frame::new().inner_margin(egui::Margin::same(fit.margin as i8)).show(ui, |ui| self.welcome_screen(ui));
                }
                self.draw_mark(ui);
            });
        } else {
            if fit.docked {
                egui::Panel::left("nav")
                    .resizable(false)
                    .exact_size(SIDEBAR_W)
                    .frame(
                        egui::Frame::new()
                            .fill(if self.mica { egui::Color32::TRANSPARENT } else { p.sidebar })
                            .inner_margin(egui::Margin::symmetric(S2 as i8, 0)),
                    )
                    .show(ui, |ui| self.sidebar(ui, false));
            }
            egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg)).show(ui, |ui| {
                if self.run.is_some() {
                    act = self.run_screen(ui, fit.margin);
                    self.draw_mark(ui);
                    return;
                }
                let fade = self.page_fade.map(|at| ((self.clock - at) / 240.0).clamp(0.0, 1.0));
                if fade.is_some_and(|t| t >= 1.0) {
                    self.page_fade = None;
                }
                ui.scope(|ui| {
                    if let Some(t) = fade {
                        ui.multiply_opacity(t);
                    }
                    let mut sa = egui::ScrollArea::vertical().auto_shrink(false);
                    if std::mem::take(&mut self.ui.scroll_top) {
                        sa = sa.vertical_scroll_offset(0.0);
                    }
                    let view = ui.available_height();
                    if let Some(m) = &self.snap_mode {
                        sa = sa.vertical_scroll_offset(m.part as f32 * (view - 48.0));
                    }
                    let out = sa.show(ui, |ui| {
                        // Fix the content width so wide rows never push past the window edge.
                        let m = fit.margin;
                        let w = (ui.available_width() - 2.0 * m).clamp(0.0, 1040.0);
                        egui::Frame::new().inner_margin(egui::Margin { left: m as i8, right: m as i8, top: S5 as i8, bottom: S6 as i8 }).show(ui, |ui| {
                            ui.set_width(w);
                            self.page_ui(ui);
                        });
                    });
                    if let Some(m) = &mut self.snap_mode {
                        m.more = out.content_size.y > (m.part + 1) as f32 * (view - 48.0) + 48.0;
                    }
                });
                self.draw_mark(ui);
            });
        }
        if let Some(a) = act {
            self.run_act(&ctx, a);
        }
        // The folded sidebar belongs to the pages; the welcome and the first check-up have none.
        if !fit.docked && !full_screen {
            self.nav_overlay(&ctx);
        }
        self.auto_update_check(&ctx);
        self.early_notice(&ctx);
        self.confirm_dialog(&ctx);
        self.copy_dialog(&ctx);
        self.toasts_ui(&ctx);
        widgets::focus_ring(&ctx);
    }
}

/// Everything the check has sent since the last frame. The check hangs up as soon as it has
/// sent its result, so a hang-up only means it crashed when no result came first.
fn drain_check(rx: &Receiver<Progress>) -> Vec<Progress> {
    let mut msgs = vec![];
    loop {
        match rx.try_recv() {
            Ok(m) => {
                let last = matches!(m, Progress::Finished(_) | Progress::Failed(_));
                msgs.push(m);
                if last {
                    break;
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => break,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                msgs.push(Progress::Failed("The check stopped part way through.".to_string()));
                break;
            }
        }
    }
    msgs
}

#[cfg(test)]
mod tests {
    use super::{Page, Place};

    #[test]
    fn report_a_problem_opens_the_bug_form_with_the_version() {
        let form = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/.github/ISSUE_TEMPLATE/bug_report.yml")).unwrap();
        assert!(form.contains("id: version"), "the form has the field the link fills in");
        assert_eq!(
            super::report_url(),
            format!("https://github.com/TanishkKamra/bootmend/issues/new?template=bug_report.yml&version={}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn a_check_that_finishes_and_hangs_up_is_not_reported_as_failed() {
        use super::{drain_check, Progress};
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(Progress::StepDone { key: "drives", ok: true }).unwrap();
        tx.send(Progress::Finished(Box::default())).unwrap();
        drop(tx);
        let msgs = drain_check(&rx);
        assert_eq!(msgs.len(), 2, "nothing comes after the result");
        assert!(matches!(msgs[1], Progress::Finished(_)));
    }

    #[test]
    fn a_check_that_hangs_up_without_a_result_is_reported_as_failed() {
        use super::{drain_check, Progress};
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(Progress::StepDone { key: "system", ok: true }).unwrap();
        drop(tx);
        let msgs = drain_check(&rx);
        assert_eq!(msgs.len(), 2);
        assert!(matches!(msgs[1], Progress::Failed(_)));
    }

    #[test]
    fn every_view_has_one_home_and_every_tab_points_back_to_it() {
        for place in Place::MAIN.into_iter().chain([Place::Settings]) {
            assert_eq!(place.first().place(), place, "{place:?} opens on a view of its own");
            for tab in place.tabs() {
                assert_eq!(tab.place(), place, "{tab:?} sits under {place:?}");
            }
        }
        for page in Page::ALL {
            let place = page.place();
            let reachable = place.first() == page || place.tabs().contains(&page) || page.inner();
            assert!(reachable, "{page:?} can't be reached from the sidebar or a tab");
        }
    }

    #[test]
    fn the_frame_adapts_to_the_window_like_windows_settings() {
        use super::{fit_for, S5, S6};
        let wide = fit_for(1440.0);
        assert!(wide.docked, "a wide window shows the sidebar");
        assert_eq!(wide.margin, S6);
        assert!(fit_for(1008.0).docked, "WinUI keeps the pane open from 1008 up");
        let half = fit_for(1007.0);
        assert!(!half.docked, "narrower than that, the sidebar folds behind a menu button");
        assert_eq!(half.margin, S6);
        let phone = fit_for(360.0);
        assert!(!phone.docked);
        assert_eq!(phone.margin, S5, "a narrow window gives its margins to the content");
    }

    #[test]
    fn screenshot_names_are_unique() {
        let mut slugs: Vec<String> = Page::ALL.iter().map(|p| p.slug()).collect();
        slugs.sort();
        slugs.dedup();
        assert_eq!(slugs.len(), Page::ALL.len());
    }
}
