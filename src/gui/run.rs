//! The Queue screen: one screen for a check-up and for fixing. The page turns into it, the logo
//! above it is the loader (Stones for a check-up, Healing for fixes), each piece of work is a box
//! that leaves once it's done, and a bar along the bottom says how far it has got.
//!
//! `Run` holds the whole screen. Its model (`step` and the `on_*` calls) is plain data and time,
//! so it's tested without a window; `ui` paints it.

use super::theme::{self, pal, Pal, S1, S2, S3, S4, S5};
use super::widgets::{self, icon};
use super::Page;
use crate::collect::STEPS;
use crate::fixes::Fix;
use crate::rules::Area;
use crate::snapshot::Snapshot;
use eframe::egui::{self, pos2, vec2, Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use serde::{Deserialize, Serialize};

/// A box, and the distance from one box to the next.
const BOX_H: f32 = 56.0;
const PITCH: f32 = 64.0;
/// The first box starts this long after the run does.
const FIRST_AT: f32 = 700.0;
/// A fix shows as being worked on for at least this long, so nobody misses it.
const MIN_NOW: f32 = 900.0;
/// The pause before the next fix, longer after a failure so its message can be read.
const GAP: f32 = 420.0;
const GAP_FAILED: f32 = 1600.0;
/// A finished box waits this long, then fades and slides away over `LEAVE_MS`.
const LEAVE_AFTER: f32 = 440.0;
const LEAVE_MS: f32 = 320.0;
/// A part of the check-up shows as being checked for at least this long.
const CHECK_MIN_NOW: f32 = 600.0;
const CHECK_STAGGER: f32 = 140.0;
/// How often the line under the queue moves on to another part of the check-up.
const ROTATE_MS: f32 = 2600.0;
const WRAP_MS: f32 = 820.0;
const PANEL_H: f32 = 200.0;
const PANEL_R: f32 = 10.0;
const TAPE: f32 = 6.0;

const APPS: &str = "\u{E71D}";
const PUZZLE: &str = "\u{EA86}";
const DRIVE: &str = "\u{EDA2}";

const FAILED_LINE: &str = "That one didn't work, so it'll wait on Home. Carrying on with the rest.";
const FAILED_FIX: &str = "This one didn't work. It'll wait on Home.";
const FAILED_STEP: &str = "Couldn't finish. The rest still works.";
const CLEAN_DOING: &str = "Clearing leftover files. Your own files stay put.";

fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub(super) fn ease_io(t: f32) -> f32 {
    let t = clamp01(t);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - clamp01(t)).powi(3)
}

/// A value easing from one place to another, starting at `at` (ms on the run's clock).
#[derive(Clone, Copy, Debug)]
struct Tween {
    from: f32,
    to: f32,
    at: f32,
    dur: f32,
}

impl Tween {
    fn new(v: f32) -> Tween {
        Tween { from: v, to: v, at: 0.0, dur: 1.0 }
    }
    fn value(&self, now: f32) -> f32 {
        lerp(self.from, self.to, ease_io((now - self.at) / self.dur))
    }
    fn go(&mut self, to: f32, now: f32, delay: f32, dur: f32) {
        self.from = self.value(now);
        self.to = to;
        self.at = now + delay;
        self.dur = dur.max(1.0);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Check,
    Fix,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Work {
    Fix(Fix),
    /// The leftover files, by cleanup item id.
    Clean(Vec<&'static str>),
    /// A part of the check-up, by step key.
    Step(&'static str),
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Wait,
    Now,
    Done,
    /// What Windows said, for the details panel.
    Failed(String),
}

impl State {
    fn resolved(&self) -> bool {
        matches!(self, State::Done | State::Failed(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Choose,
    Running,
    Over,
}

/// The restore point that comes before any fix.
#[derive(Clone, Debug, PartialEq)]
pub enum Gate {
    Needed,
    Saving,
    Saved,
    Failed(String),
    Off,
    TurningOn,
}

impl Gate {
    /// Where the gate starts. `latest` is when Windows last saved a restore point
    /// ("yyyy-MM-dd HH:mm"), and `on` whether System Restore is on for drive C.
    pub fn start(saved_this_session: bool, latest: Option<&str>, on: Option<bool>, today: chrono::NaiveDate) -> Gate {
        let day = today.format("%Y-%m-%d").to_string();
        if saved_this_session || latest.is_some_and(|w| w.starts_with(&day)) {
            Gate::Saved
        } else if on == Some(false) {
            Gate::Off
        } else {
            Gate::Needed
        }
    }

    /// After trying to save one. Windows skips a second restore point within a day, and the one
    /// it already has is just as good.
    pub fn after(r: &Result<(), String>) -> Gate {
        match r {
            Ok(()) => Gate::Saved,
            Err(e) if e.contains("last 24 hours") => Gate::Saved,
            Err(e) => Gate::Failed(e.clone()),
        }
    }
}

/// One box in the queue.
pub struct Item {
    pub work: Work,
    pub title: String,
    /// The fix's own name, or nothing.
    pub sub: String,
    pub icon: &'static str,
    /// The friendly line said while it's worked on.
    pub doing: String,
    pub ticked: bool,
    pub state: State,
    began: f32,
    took: Option<f32>,
    result: Option<Result<(), String>>,
    /// Where the box sits in the queue now, and where it's heading.
    y: f32,
    goal: f32,
    shown_at: f32,
    left_at: Option<f32>,
}

impl Item {
    fn new(work: Work, title: String, sub: String, icon: &'static str, doing: String) -> Item {
        Item {
            work,
            title,
            sub,
            icon,
            doing,
            ticked: true,
            state: State::Wait,
            began: 0.0,
            took: None,
            result: None,
            y: f32::NAN,
            goal: 0.0,
            shown_at: 0.0,
            left_at: None,
        }
    }

    /// A fix for the to-do called `title`.
    pub fn fix(fix: Fix, title: impl Into<String>, area: Area) -> Item {
        let sub = match fix.up_to_minutes() {
            Some(m) => format!("{} · up to {m} minutes", fix.label()),
            None => fix.label().to_string(),
        };
        let doing = fix.doing();
        Item::new(Work::Fix(fix), title.into(), sub, area_icon(area), doing)
    }

    /// The leftover files, as their line on Home.
    pub fn clean(ids: Vec<&'static str>, title: impl Into<String>) -> Item {
        Item::new(Work::Clean(ids), title.into(), String::new(), icon::CLEAN, CLEAN_DOING.into())
    }

    fn step(key: &'static str, label: &str) -> Item {
        Item::new(Work::Step(key), label.into(), String::new(), step_icon(key), step_line(key).into())
    }

    fn leaving(&self, now: f32) -> bool {
        self.left_at.is_some_and(|t| now >= t)
    }
}

pub fn area_icon(a: Area) -> &'static str {
    match a {
        Area::Security => icon::SHIELD,
        Area::Stability => icon::HEALTH,
        Area::Speed => icon::SPEED,
        Area::Hardware => icon::CHIP,
    }
}

fn step_icon(key: &str) -> &'static str {
    match key {
        "system" => icon::PC,
        "storage" => icon::FOLDER,
        "network" => icon::NETWORK,
        "security" => icon::SHIELD,
        "software" => APPS,
        "health" => icon::HEALTH,
        "extensions" => PUZZLE,
        _ => DRIVE,
    }
}

fn step_line(key: &str) -> &'static str {
    match key {
        "system" => "Reading the hardware and Windows details",
        "storage" => "Measuring drives and free space",
        "network" => "Looking over the network adapters",
        "security" => "Checking how well the PC is protected",
        "software" => "Going through apps, startup items and services",
        "health" => "Reading back through crashes and slow starts",
        "extensions" => "Looking at browser extensions",
        _ => "Reading the drives' health counters",
    }
}

/// What the person pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Cancel,
    SaveRestore,
    TurnOn,
    Start,
    Close,
}

struct Line {
    text: String,
    at: f32,
}

pub struct Run {
    pub mode: Mode,
    pub stage: Stage,
    pub items: Vec<Item>,
    pub origin: Page,
    /// The first check-up ever: the screen has the window to itself.
    pub first: bool,
    pub gate: Gate,
    /// The check-up's result, kept until its boxes have caught up.
    pub finished: Option<Box<Snapshot>>,
    /// Windows' animation effects are off: things fade instead of moving.
    pub reduced: bool,
    /// How much of the work is done, 0 to 1.
    pub p: f32,
    details: bool,
    clock: f32,
    began: f32,
    next_at: f32,
    line: Line,
    old: Option<Line>,
    rotate_at: f32,
    rotate_i: usize,
    wrap: Tween,
    lift: Tween,
    shown: f32,
    hop_at: f32,
    below: bool,
    ended: bool,
    rested_at: Option<f32>,
    /// The boxes in the order they started, for the details panel.
    order: Vec<usize>,
    settle: f32,
    gate_h: f32,
}

impl Run {
    fn new(mode: Mode, stage: Stage, items: Vec<Item>, origin: Page, first: bool, gate: Gate) -> Run {
        let mut r = Run {
            mode,
            stage,
            items,
            origin,
            first,
            gate,
            finished: None,
            reduced: false,
            p: 0.0,
            details: false,
            clock: 0.0,
            began: 0.0,
            next_at: FIRST_AT,
            line: Line { text: String::new(), at: -1e9 },
            old: None,
            rotate_at: ROTATE_MS,
            rotate_i: 0,
            wrap: Tween::new(0.0),
            lift: Tween::new(0.0),
            shown: 0.0,
            hop_at: -1e9,
            below: false,
            ended: false,
            rested_at: None,
            order: vec![],
            settle: 0.0,
            gate_h: 96.0,
        };
        for (i, it) in r.items.iter_mut().enumerate() {
            it.shown_at = 320.0 + i as f32 * 55.0;
        }
        r.settle = 320.0 + r.items.len() as f32 * 55.0 + 400.0;
        r
    }

    /// A queue of fixes to choose from. Everything starts ticked.
    pub fn fixes(items: Vec<Item>, origin: Page, gate: Gate) -> Run {
        Run::new(Mode::Fix, Stage::Choose, items, origin, false, gate)
    }

    /// A check-up, already under way.
    pub fn check(origin: Page, first: bool) -> Run {
        let items = STEPS.iter().map(|s| Item::step(s.key, s.label)).collect();
        let mut r = Run::new(Mode::Check, Stage::Running, items, origin, first, Gate::Saved);
        r.say("Starting the check-up. Nothing on your PC changes.");
        r
    }

    fn queued(&self) -> impl Iterator<Item = &Item> {
        self.items.iter().filter(|i| i.ticked)
    }

    pub fn ticked(&self) -> usize {
        self.queued().count()
    }

    /// Whether History can take back every change ticked. A repair has nothing to undo.
    fn undoable(&self) -> bool {
        let mut fixes = self.queued().filter_map(|i| if let Work::Fix(f) = &i.work { Some(f) } else { None }).peekable();
        fixes.peek().is_some() && fixes.all(Fix::undoable)
    }

    /// A restore point comes first unless nothing ticked changes Windows: clearing files isn't
    /// covered by System Restore, and turning System Restore on can't wait for one.
    pub fn needs_gate(&self) -> bool {
        self.queued().any(|i| matches!(&i.work, Work::Fix(f) if *f != Fix::EnableSystemProtection))
    }

    pub fn ready(&self) -> bool {
        self.ticked() > 0 && (!self.needs_gate() || self.gate == Gate::Saved)
    }

    /// Whether what comes next needs administrator rights.
    pub fn needs_admin(&self) -> bool {
        let gate = self.needs_gate() && matches!(self.gate, Gate::Needed | Gate::Failed(_) | Gate::Off);
        gate || self.queued().any(|i| matches!(&i.work, Work::Fix(f) if f.needs_admin()))
    }

    /// The work is under way and its boxes are moving.
    pub fn running(&self) -> bool {
        self.stage == Stage::Running
    }

    /// Whether anything on the screen still moves.
    pub fn moving(&self) -> bool {
        self.stage == Stage::Running || self.clock < self.settle || (self.shown - self.p).abs() > 0.001 || self.items.iter().any(|i| (i.y - i.goal).abs() > 0.3)
    }

    fn bump(&mut self, until: f32) {
        self.settle = self.settle.max(until);
    }

    fn say(&mut self, text: &str) {
        if self.line.text == text {
            return;
        }
        let now = self.clock;
        let old = std::mem::replace(&mut self.line, Line { text: text.to_string(), at: now });
        self.old = (!old.text.is_empty()).then_some(Line { text: old.text, at: now });
        self.bump(now + 300.0);
    }

    pub fn toggle(&mut self, i: usize) {
        if self.stage == Stage::Choose {
            if let Some(it) = self.items.get_mut(i) {
                it.ticked = !it.ticked;
            }
        }
    }

    /// Starts the fixes. The unticked boxes leave.
    pub fn start(&mut self) {
        let now = self.clock;
        self.stage = Stage::Running;
        self.began = now;
        self.next_at = now + FIRST_AT;
        for it in &mut self.items {
            if !it.ticked {
                it.left_at = Some(now);
            }
        }
        let line = match (self.undoable(), self.ticked()) {
            (false, _) => "Getting started.",
            (true, 1) => "Getting started. You can undo this from History.",
            (true, _) => "Getting started. Any of these can be undone from History.",
        };
        self.say(line);
    }

    /// A fix finished in the background: `ms` is how long it really took.
    pub fn on_result(&mut self, i: usize, r: Result<(), String>, ms: f32) {
        if let Some(it) = self.items.get_mut(i) {
            it.result = Some(r);
            it.took = Some(ms);
        }
    }

    /// A part of the check-up finished.
    pub fn on_step(&mut self, key: &str, ok: bool) {
        if let Some(it) = self.items.iter_mut().find(|i| matches!(i.work, Work::Step(k) if k == key) && !i.state.resolved()) {
            it.result = Some(if ok { Ok(()) } else { Err(String::new()) });
        }
    }

    /// The check-up is over. Anything that never reported counts as done.
    pub fn on_finished(&mut self, s: Box<Snapshot>) {
        self.finished = Some(s);
        for it in &mut self.items {
            if !it.state.resolved() && it.result.is_none() {
                it.result = Some(Ok(()));
            }
        }
    }

    /// The loader's tick has shown: the screen says the work is done.
    pub fn ticked_now(&mut self) {
        if self.stage == Stage::Running {
            self.stage = Stage::Over;
        }
    }

    /// The loader is back to the logo: the way back appears.
    pub fn rested(&mut self) {
        self.rested_at = Some(self.clock);
        self.bump(self.clock + 300.0);
    }

    pub fn toggle_details(&mut self) {
        let now = self.clock;
        self.details = !self.details;
        self.hop_at = now;
        let (open, r) = (self.details, self.reduced);
        self.wrap.go(f32::from(u8::from(open)), now, 0.0, if r { 1.0 } else { WRAP_MS });
        if open {
            self.lift.go(1.0, now, 0.0, if r { 1.0 } else { 260.0 });
        } else {
            self.lift.go(0.0, now, if r { 0.0 } else { 460.0 }, if r { 1.0 } else { 380.0 });
        }
        self.bump(now + 1000.0);
    }

    /// Moves time on by `dt` ms. Returns whether everything worked, once, when the work is over.
    pub fn step(&mut self, dt: f32) -> Option<bool> {
        self.clock += dt;
        let now = self.clock;
        let mut out = None;
        if self.stage == Stage::Running && !self.ended {
            match self.mode {
                Mode::Fix => self.pace_fixes(),
                Mode::Check => self.pace_checks(),
            }
            if self.work_over() {
                self.ended = true;
                self.p = 1.0;
                let bad = self.queued().filter(|i| matches!(i.state, State::Failed(_))).count();
                let line = match (self.mode, bad) {
                    (Mode::Check, _) => "All checked.".to_string(),
                    (Mode::Fix, 0) => "All done.".to_string(),
                    (Mode::Fix, 1) => "Done, apart from one that needs you.".to_string(),
                    (Mode::Fix, k) => format!("Done, apart from {k} that need you."),
                };
                self.say(&line);
                out = Some(bad == 0);
            } else {
                self.p = self.p.max(self.progress_now());
            }
        }
        // Boxes that have gone make room at once, and the rest ease up into it.
        let k = 1.0 - (-dt / 110.0).exp();
        let mut rank = 0.0;
        for it in &mut self.items {
            let in_line = !it.leaving(now) && (self.stage == Stage::Choose || it.ticked);
            if in_line {
                it.goal = rank * PITCH;
                rank += 1.0;
            }
            if it.y.is_nan() || self.reduced {
                it.y = it.goal;
            } else {
                it.y += (it.goal - it.y) * k;
            }
        }
        self.shown = if self.reduced { self.p } else { self.shown + (self.p - self.shown) * (1.0 - (-dt / 260.0).exp()) };
        if now >= self.hop_at + 170.0 {
            self.below = self.details;
        }
        out
    }

    fn work_over(&self) -> bool {
        match self.mode {
            Mode::Fix => self.ticked() > 0 && self.queued().all(|i| i.state.resolved()),
            Mode::Check => self.finished.is_some() && self.items.iter().all(|i| i.state.resolved()),
        }
    }

    fn progress_now(&self) -> f32 {
        let n = self.ticked().max(1) as f32;
        let done = self.queued().filter(|i| i.state.resolved()).count() as f32;
        let partial: f32 = self
            .queued()
            .filter(|i| i.state == State::Now)
            .map(|i| {
                let dur = match (self.mode, &i.work) {
                    // A long fix creeps along, reaching about half its share at half its longest time.
                    (Mode::Fix, Work::Fix(f)) => f.up_to_minutes().map_or(675.0, |m| m as f32 * 30_000.0),
                    (Mode::Fix, _) => 675.0,
                    (Mode::Check, Work::Step("extensions" | "drives")) => 3000.0,
                    (Mode::Check, _) => 15000.0,
                };
                0.9 * (1.0 - (-(self.clock - i.began) / dur).exp())
            })
            .sum();
        ((done + partial) / n).min(0.995)
    }

    fn pace_fixes(&mut self) {
        let now = self.clock;
        if let Some(i) = self.items.iter().position(|it| it.state == State::Now) {
            let it = &mut self.items[i];
            if now - it.began < MIN_NOW {
                return;
            }
            let Some(r) = it.result.take() else { return };
            it.took.get_or_insert(now - it.began);
            match r {
                Ok(()) => {
                    it.state = State::Done;
                    it.left_at = Some(now + LEAVE_AFTER);
                    self.next_at = now + GAP;
                    self.bump(now + LEAVE_AFTER + LEAVE_MS + 400.0);
                }
                Err(e) => {
                    it.state = State::Failed(e);
                    self.next_at = now + GAP_FAILED;
                    self.say(FAILED_LINE);
                }
            }
        } else if now >= self.next_at {
            if let Some(i) = self.items.iter().position(|it| it.ticked && it.state == State::Wait) {
                let it = &mut self.items[i];
                it.state = State::Now;
                it.began = now;
                let line = it.doing.clone();
                self.order.push(i);
                self.say(&line);
            }
        }
    }

    fn pace_checks(&mut self) {
        let now = self.clock;
        let t = now - self.began;
        for (i, it) in self.items.iter_mut().enumerate() {
            if it.state == State::Wait && t >= FIRST_AT + i as f32 * CHECK_STAGGER {
                it.state = State::Now;
                it.began = now;
                self.order.push(i);
            }
            if it.state == State::Now && now - it.began >= CHECK_MIN_NOW {
                if let Some(r) = it.result.take() {
                    it.took = Some(now - it.began);
                    match r {
                        Ok(()) => {
                            it.state = State::Done;
                            it.left_at = Some(now + LEAVE_AFTER);
                            self.settle = self.settle.max(now + LEAVE_AFTER + LEAVE_MS + 400.0);
                        }
                        Err(e) => it.state = State::Failed(e),
                    }
                }
            }
        }
        if now >= self.rotate_at {
            self.rotate_at = now + ROTATE_MS;
            let lines: Vec<String> = self.items.iter().filter(|i| i.state == State::Now).map(|i| i.doing.clone()).collect();
            if !lines.is_empty() {
                let l = lines[self.rotate_i % lines.len()].clone();
                self.rotate_i += 1;
                self.say(&l);
            }
        }
    }

    /// The title and the line under it.
    pub fn heads(&self) -> (String, String) {
        let n = self.ticked();
        let things = |n: usize| if n == 1 { "1 thing".to_string() } else { format!("{n} things") };
        let fixes = self.undoable();
        let (t, s): (String, String) = match (self.mode, self.stage) {
            (Mode::Check, Stage::Over) => ("Check-up done".into(), if self.first { "Your plan is ready." } else { "Home is up to date." }.into()),
            (Mode::Check, _) => ("Checking your PC".into(), "About a minute. You can keep using your PC.".into()),
            (Mode::Fix, Stage::Choose) if n == 0 => ("Nothing picked".into(), "Tick anything you'd like Bootmend to fix.".into()),
            (Mode::Fix, Stage::Choose) if self.items.len() == 1 => (
                "Ready to fix this".into(),
                if fixes { "Bootmend does it for you, and you can undo it from History." } else { "Bootmend does it for you. Your own files aren't touched." }
                    .into(),
            ),
            (Mode::Fix, Stage::Choose) => (
                format!("Ready to fix {}", things(n)),
                if fixes {
                    "Untick anything you'd rather leave as it is. You can undo any of them from History."
                } else {
                    "Untick anything you'd rather leave as it is. Your own files aren't touched."
                }
                .into(),
            ),
            (Mode::Fix, Stage::Running) => {
                let done = self.queued().filter(|i| i.state == State::Done).count();
                (format!("Fixing {}", things(n)), format!("{done} of {n} done"))
            }
            (Mode::Fix, Stage::Over) => {
                let bad = self.queued().filter(|i| matches!(i.state, State::Failed(_))).count();
                let ok = n - bad;
                match (ok, bad, n) {
                    (_, 0, 1) => ("Fixed".into(), if fixes { "You can undo it from History." } else { "Your own files weren't touched." }.into()),
                    (_, 0, _) => {
                        (format!("All {n} fixed"), if fixes { "You can undo any of them from History." } else { "Your own files weren't touched." }.into())
                    }
                    (0, _, 1) => ("That didn't work".into(), "It'll wait on Home for another go.".into()),
                    (0, _, _) => ("These didn't work this time".into(), "They're waiting on Home for another go.".into()),
                    (_, 1, _) => (format!("{ok} of {n} fixed"), "One needs you. It's waiting on Home.".into()),
                    (_, _, _) => (format!("{ok} of {n} fixed"), format!("{bad} need you. They're waiting on Home.")),
                }
            }
        };
        (t, s)
    }

    pub fn end_label(&self) -> String {
        match self.mode {
            Mode::Check if self.first => "See what Bootmend found".into(),
            Mode::Check => "See Home".into(),
            Mode::Fix if self.origin == Page::Home => "Back to Home".into(),
            Mode::Fix => format!("Back to {}", self.origin.title()),
        }
    }

    fn pill(&self, s: &State, p: &Pal) -> (&'static str, Color32, Option<Color32>) {
        let fix = self.mode == Mode::Fix;
        match s {
            State::Wait => ("Waiting", p.muted, None),
            State::Now => (if fix { "Working on it" } else { "Checking" }, p.accent, Some(p.accent_soft)),
            State::Done => ("Done", p.green, Some(p.green_bg)),
            State::Failed(_) => (if fix { "Needs you" } else { "Couldn't finish" }, p.amber, Some(p.amber_bg)),
        }
    }

    fn box_sub<'a>(&self, it: &'a Item) -> &'a str {
        match (&it.state, self.mode) {
            (State::Failed(_), Mode::Fix) => FAILED_FIX,
            (State::Failed(_), Mode::Check) => FAILED_STEP,
            _ => &it.sub,
        }
    }

    // -----------------------------------------------------------------------------------------
    // Painting

    /// Paints the screen over the whole of `ui`, with `margin` at the sides. Returns where the
    /// logo goes and what was pressed.
    /// `esc` says Escape is this screen's to handle, with nothing open over it.
    pub fn ui(&mut self, ui: &mut Ui, margin: f32, elevated: bool, esc: bool) -> (Rect, Option<Act>) {
        let p = pal(ui.ctx());
        let full = ui.max_rect();
        let now = self.clock;
        let mut act = None;
        let content = Rect::from_min_max(pos2(full.left() + margin, full.top()), pos2(full.right() - margin, full.bottom()));
        let cx = content.center().x;
        let s = if full.height() < 600.0 { 72.0 } else { 96.0 };
        let mark = Rect::from_min_size(pos2(cx - s / 2.0, full.top() + S5), Vec2::splat(s));
        ui.multiply_opacity(clamp01(now / 240.0));

        // The title and the line under it.
        let (title, sub) = self.heads();
        let tw = content.width().min(600.0);
        let head = Rect::from_min_size(pos2(cx - tw / 2.0, mark.bottom() + S4), vec2(tw, 400.0));
        let used = ui
            .scope_builder(egui::UiBuilder::new().max_rect(head).layout(egui::Layout::top_down(egui::Align::Center)), |ui| {
                ui.spacing_mut().item_spacing.y = S1;
                let r = ui.add(egui::Label::new(RichText::new(&title).text_style(egui::TextStyle::Heading)).wrap());
                widgets::heading_role(ui, &r, 1, &title);
                ui.add(egui::Label::new(RichText::new(&sub).color(p.muted)).wrap());
            })
            .response
            .rect;

        let bar_y = full.bottom() - 64.0;
        // Short of room, the panel takes the whole width and the percentage sits by the button.
        let narrow = content.width() < 520.0;
        let pw = if narrow { content.width() } else { (content.width() - 96.0).clamp(160.0, 720.0) };
        let panel = Rect::from_min_max(pos2(cx - pw / 2.0, bar_y - PANEL_H), pos2(cx + pw / 2.0, bar_y));
        let qw = content.width().min(600.0);
        let closed = match self.stage {
            Stage::Choose => full.bottom() - S5 - self.gate_h - S4,
            _ => bar_y - 40.0 - S4,
        };
        let qbottom = lerp(closed, panel.top() - S4, self.lift.value(now));
        let qtop = used.bottom() + S5;
        let queue = Rect::from_min_max(pos2(cx - qw / 2.0, qtop), pos2(cx + qw / 2.0, qbottom.max(qtop + PITCH)));
        if let Some(i) = self.queue_ui(ui, &p, queue) {
            self.toggle(i);
        }

        match self.stage {
            Stage::Choose => {
                let area = Rect::from_min_max(pos2(queue.left(), queue.bottom()), pos2(queue.right(), full.bottom() - S5));
                act = self.gate_ui(ui, &p, area, elevated);
                if esc && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    act = Some(Act::Cancel);
                }
            }
            Stage::Running | Stage::Over => {
                if self.dock_ui(ui, &p, panel, narrow) {
                    self.toggle_details();
                }
            }
        }
        if let (Stage::Over, Some(at)) = (self.stage, self.rested_at) {
            let rows = self.items.iter().filter(|i| !i.leaving(now) && i.ticked).count() as f32;
            let y = (queue.top() + rows * PITCH + S2).min(queue.bottom() - 32.0);
            let row = Rect::from_min_size(pos2(queue.left(), y), vec2(queue.width(), 32.0));
            let label = self.end_label();
            let r = ui
                .scope_builder(egui::UiBuilder::new().max_rect(row).layout(egui::Layout::top_down(egui::Align::Center)), |ui| {
                    ui.multiply_opacity(clamp01((now - at) / 240.0));
                    widgets::primary(ui, &label)
                })
                .inner;
            if r.clicked() || (esc && ui.input(|i| i.key_pressed(egui::Key::Escape))) {
                act = Some(Act::Close);
            }
        }
        (mark, act)
    }

    /// The boxes. Returns the box clicked, while choosing.
    fn queue_ui(&self, ui: &mut Ui, p: &Pal, queue: Rect) -> Option<usize> {
        let now = self.clock;
        let shown: Vec<usize> = (0..self.items.len())
            .filter(|&i| {
                let it = &self.items[i];
                let gone = it.left_at.is_some_and(|t| now >= t + LEAVE_MS);
                !gone && (self.stage == Stage::Choose || it.ticked || it.left_at.is_some())
            })
            .collect();
        let height = shown.iter().map(|&i| self.items[i].goal + BOX_H).fold(0.0, f32::max);
        let mut clicked = None;
        let mut overflow = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(queue), |ui| {
            let out = egui::ScrollArea::vertical().id_salt("run-queue").auto_shrink(false).show(ui, |ui| {
                let (area, _) = ui.allocate_exact_size(vec2(queue.width(), height), Sense::hover());
                for &i in &shown {
                    if self.box_ui(ui, p, i, area.min, queue.width()) {
                        clicked = Some(i);
                    }
                }
            });
            overflow = out.state.offset.y + out.inner_rect.height() < out.content_size.y - 1.0;
        });
        if overflow {
            // The queue goes on below: it fades into the page rather than stopping at an edge.
            let r = Rect::from_min_max(pos2(queue.left(), queue.bottom() - 30.0), queue.max);
            let mut mesh = egui::Mesh::default();
            for (pos, c) in [(r.left_top(), Color32::TRANSPARENT), (r.right_top(), Color32::TRANSPARENT), (r.right_bottom(), p.bg), (r.left_bottom(), p.bg)] {
                mesh.colored_vertex(pos, c);
            }
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            ui.painter().add(egui::Shape::mesh(mesh));
        }
        clicked
    }

    /// One box at its place under `origin`. Returns whether it was clicked.
    fn box_ui(&self, ui: &mut Ui, p: &Pal, i: usize, origin: Pos2, width: f32) -> bool {
        let now = self.clock;
        let it = &self.items[i];
        let k = (now - it.shown_at) / 380.0;
        let rise = if self.reduced { 0.0 } else { 18.0 * (1.0 - ease_out(k)) };
        let mut a = clamp01((now - it.shown_at) / 240.0);
        let mut dx = 0.0;
        if let Some(t) = it.left_at.filter(|&t| now >= t) {
            let k = clamp01((now - t) / LEAVE_MS);
            a *= 1.0 - k;
            if !self.reduced {
                dx = -24.0 * ease_io(k);
            }
        }
        if a <= 0.0 {
            return false;
        }
        let r = Rect::from_min_size(pos2(origin.x + dx, origin.y + it.y + rise), vec2(width, BOX_H));
        let choose = self.stage == Stage::Choose;
        let resp = ui.interact(r, ui.id().with(("run-box", i)), if choose { Sense::click() } else { Sense::hover() });
        if choose {
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, it.ticked, &it.title));
        }
        let mut painter = ui.painter().clone();
        painter.multiply_opacity(a);
        let fill = if choose && resp.hovered() { p.surface2 } else { p.surface };
        let edge = match it.state {
            State::Now => p.accent,
            State::Failed(_) => p.amber,
            _ => p.line,
        };
        painter.rect(r, CornerRadius::same(8), fill, Stroke::new(1.0, edge), egui::StrokeKind::Inside);
        let slot = Rect::from_center_size(pos2(r.left() + S3 + 16.0, r.center().y), Vec2::splat(32.0));
        if choose {
            let b = Rect::from_center_size(slot.center(), Vec2::splat(20.0));
            if it.ticked {
                painter.rect_filled(b, CornerRadius::same(4), p.accent);
                painter.text(b.center(), Align2::CENTER_CENTER, icon::CHECK, FontId::new(12.0, theme::icons()), p.on_accent);
            } else {
                painter.rect_stroke(b, CornerRadius::same(4), Stroke::new(1.0, p.control), egui::StrokeKind::Inside);
            }
        } else {
            let (bg, fg, g) = match it.state {
                State::Wait => (p.surface2, p.muted, it.icon),
                State::Now => (p.accent_soft, p.accent, it.icon),
                State::Done => (p.green_bg, p.green, icon::CHECK),
                State::Failed(_) => (p.amber_bg, p.amber, icon::WARN),
            };
            painter.circle_filled(slot.center(), 16.0, bg);
            painter.text(slot.center(), Align2::CENTER_CENTER, g, FontId::new(16.0, theme::icons()), fg);
        }
        let mut right = r.right() - S4;
        if !choose {
            let (label, fg, bg) = self.pill(&it.state, p);
            let g = painter.layout_no_wrap(label.to_string(), FontId::new(12.0, theme::semibold()), fg);
            let size = g.size() + vec2(16.0, 4.0);
            let pr = Rect::from_min_size(pos2(right - size.x, r.center().y - size.y / 2.0), size);
            if let Some(bg) = bg {
                painter.rect_filled(pr, CornerRadius::same(10), bg);
            }
            painter.galley(pr.min + vec2(8.0, 2.0), g, fg);
            right = pr.left() - S3;
        }
        let tx = slot.right() + S3;
        let tw = (right - tx).max(40.0);
        let galley = |text: RichText, style: egui::TextStyle| egui::WidgetText::from(text).into_galley(ui, Some(egui::TextWrapMode::Truncate), tw, style);
        let title = galley(RichText::new(&it.title).family(theme::semibold()).color(p.ink), egui::TextStyle::Body);
        let sub = self.box_sub(it);
        let sub = (!sub.is_empty()).then(|| galley(RichText::new(sub).small().color(p.muted), egui::TextStyle::Small));
        let h = title.size().y + sub.as_ref().map_or(0.0, |s| 2.0 + s.size().y);
        let ty = r.center().y - h / 2.0;
        let th = title.size().y;
        painter.galley(pos2(tx, ty), title, p.ink);
        if let Some(s) = sub {
            painter.galley(pos2(tx, ty + th + 2.0), s, p.muted);
        }
        resp.clicked()
    }

    /// Where the bar and the details panel would be while choosing: the restore point, and the
    /// buttons that start the work.
    fn gate_ui(&mut self, ui: &mut Ui, p: &Pal, area: Rect, elevated: bool) -> Option<Act> {
        let mut act = None;
        let gate = self.needs_gate();
        let top = area.bottom() - self.gate_h;
        let rect = Rect::from_min_max(pos2(area.left(), top), pos2(area.right(), area.bottom() + 400.0));
        let used = ui
            .scope_builder(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
                ui.spacing_mut().item_spacing.y = S2;
                if gate {
                    let (lead, text, hover): (Option<(&str, Color32)>, &str, Option<String>) = match &self.gate {
                        Gate::Needed => (None, "Save a restore point first. If anything goes wrong, you can go back to how things are now.", None),
                        Gate::Saving => (None, "Saving a restore point. This takes a minute or two.", None),
                        Gate::Saved => (Some((icon::CHECK, p.green)), "Restore point saved", None),
                        Gate::Failed(e) => (Some((icon::WARN, p.amber)), "Windows couldn't save a restore point. Try again in a moment.", Some(e.clone())),
                        Gate::Off => (None, "System Restore is off. Turn it on so Bootmend can save a restore point first.", None),
                        Gate::TurningOn => (None, "Turning System Restore on.", None),
                    };
                    let spin = matches!(self.gate, Gate::Saving | Gate::TurningOn);
                    ui.horizontal(|ui| {
                        if spin {
                            ui.add(egui::Spinner::new().size(16.0).color(p.accent));
                        } else if let Some((g, c)) = lead {
                            ui.label(widgets::ico(g, 16.0).color(c));
                        }
                        let r = ui.add(egui::Label::new(text).wrap());
                        if let Some(h) = hover {
                            r.on_hover_text(h);
                        }
                    });
                }
                if !elevated && self.needs_admin() {
                    ui.horizontal(|ui| {
                        ui.label(widgets::ico(icon::ADMIN, 14.0).color(p.muted));
                        ui.label(RichText::new("Windows will ask for permission once.").small().color(p.muted));
                    });
                }
                ui.add_space(S1);
                widgets::right_row(ui, |ui| {
                    if widgets::secondary(ui, "Not now").clicked() {
                        act = Some(Act::Cancel);
                    }
                    let extra = match self.gate {
                        Gate::Needed if gate => Some(("Save a restore point", Act::SaveRestore)),
                        Gate::Failed(_) if gate => Some(("Try again", Act::SaveRestore)),
                        Gate::Off if gate => Some(("Turn it on", Act::TurnOn)),
                        _ => None,
                    };
                    if let Some((label, a)) = extra {
                        if widgets::primary(ui, label).clicked() {
                            act = Some(a);
                        }
                    }
                    let label = if gate { "Start fixing" } else { "Start" };
                    let ready = self.ready();
                    let r = if ready { widgets::primary(ui, label) } else { widgets::secondary_enabled(ui, label, false) };
                    if r.clicked() {
                        act = Some(Act::Start);
                    }
                });
            })
            .response
            .rect;
        if (used.height() - self.gate_h).abs() > 0.5 {
            self.gate_h = used.height();
            ui.ctx().request_discard("measuring the restore point block");
        }
        act
    }

    /// The bar, the details panel it wraps round, the friendly line and the percentage.
    /// Returns whether Details was pressed.
    fn dock_ui(&self, ui: &mut Ui, p: &Pal, panel: Rect, narrow: bool) -> bool {
        let now = self.clock;
        let wv = self.wrap.value(now);
        let bar_y = panel.bottom();
        let path = tape_path(panel, PANEL_R);
        let len = path_len(&path);
        let f0 = (panel.width() - 2.0 * PANEL_R) / len;
        let f = lerp(f0, 1.0, wv);
        if wv > 0.01 {
            let mut painter = ui.painter().clone();
            painter.multiply_opacity(wv);
            painter.rect_filled(panel, CornerRadius::same(PANEL_R as u8), p.surface);
            self.log_ui(ui, p, panel, wv);
        }
        let painter = ui.painter();
        for (cut, color) in [(f, p.line), (f * self.shown.max(0.0), p.accent)] {
            let pts = cut_path(&path, cut * len);
            if let (Some(a), Some(b)) = (pts.first().copied(), pts.last().copied()) {
                painter.circle_filled(a, TAPE / 2.0, color);
                painter.circle_filled(b, TAPE / 2.0, color);
                if pts.len() > 1 {
                    painter.line(pts, Stroke::new(TAPE, color));
                }
            }
        }
        let bar = Rect::from_min_max(pos2(panel.left() + PANEL_R, bar_y - 4.0), pos2(panel.right() - PANEL_R, bar_y + 4.0));
        let pct = (self.shown * 100.0).floor().clamp(0.0, 100.0) as u32;
        ui.interact(bar, ui.id().with("run-progress"), Sense::hover()).widget_info(|| {
            let mut info = egui::WidgetInfo::labeled(egui::WidgetType::ProgressIndicator, true, "Progress");
            info.value = Some(f64::from(pct));
            info
        });

        // The line, the percentage and the button hop above or below the bar as the panel opens.
        let t = now - self.hop_at;
        let ha = if t < 160.0 {
            1.0 - t / 160.0
        } else if t < 170.0 {
            0.0
        } else {
            clamp01((t - 170.0) / 160.0)
        };
        let mut toggled = false;
        let label = if self.below { "Hide details" } else { "Details" };
        let chevron = if self.below { icon::CHEVRON_DOWN } else { icon::CHEVRON_UP };
        let bw = painter.layout_no_wrap(label.into(), FontId::new(14.0, theme::semibold()), p.accent).size().x + 14.0 + S1 + 2.0 * S2;
        let (line_y, row_y) = if self.below { (bar_y + 10.0, bar_y + 10.0) } else { (bar_y - 40.0, bar_y - 44.0) };
        let brect = Rect::from_min_size(pos2(panel.right() - PANEL_R - bw, row_y), vec2(bw, 28.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(brect).layout(egui::Layout::right_to_left(egui::Align::Center)), |ui| {
            ui.multiply_opacity(ha);
            let mut job = egui::text::LayoutJob::default();
            let fmt = |font: FontId| egui::TextFormat { font_id: font, color: p.accent, valign: egui::Align::Center, ..Default::default() };
            job.append(label, 0.0, fmt(FontId::new(14.0, theme::semibold())));
            job.append(chevron, S1, fmt(FontId::new(12.0, theme::icons())));
            toggled = ui.add(egui::Button::new(job).frame(false)).clicked();
        });
        let pct_text = format!("{pct}%");
        let mut pp = ui.painter().clone();
        pp.multiply_opacity(ha);
        let font = FontId::new(14.0, theme::semibold());
        let line_right = if self.below || narrow {
            let g = pp.layout_no_wrap(pct_text, font, p.ink);
            let at = pos2(brect.left() - S3 - g.size().x, brect.center().y - g.size().y / 2.0);
            pp.galley(at, g, p.ink);
            at.x - S3
        } else {
            pp.text(pos2(panel.right() - PANEL_R + S3, bar_y), Align2::LEFT_CENTER, pct_text, font, p.ink);
            brect.left() - S3
        };
        let left = panel.left() + PANEL_R;
        self.lines_ui(ui, p, pos2(left, line_y), line_right - left, ha);
        toggled
    }

    /// The friendly line: the new one rises in as the old one rises away.
    fn lines_ui(&self, ui: &Ui, p: &Pal, at: Pos2, width: f32, alpha: f32) {
        let now = self.clock;
        let draw = |text: &str, a: f32, dy: f32| {
            if a <= 0.0 || text.is_empty() {
                return;
            }
            let g = egui::WidgetText::from(RichText::new(text).color(p.ink)).into_galley(
                ui,
                Some(egui::TextWrapMode::Truncate),
                width.max(40.0),
                egui::TextStyle::Body,
            );
            let mut painter = ui.painter().clone();
            painter.multiply_opacity(a * alpha);
            painter.galley(at + vec2(0.0, dy), g, p.ink);
        };
        if let Some(old) = &self.old {
            let k = clamp01((now - old.at) / 260.0);
            draw(&old.text, 1.0 - k, if self.reduced { 0.0 } else { -6.0 * ease_out(k) });
        }
        let k = clamp01((now - self.line.at) / 220.0);
        draw(&self.line.text, k, if self.reduced { 0.0 } else { 6.0 * (1.0 - ease_out(k)) });
    }

    /// Inside the open panel: what's been done, in the order it started.
    fn log_ui(&self, ui: &mut Ui, p: &Pal, panel: Rect, alpha: f32) {
        let now = self.clock;
        let inner = panel.shrink2(vec2(S4, S3 + 2.0));
        let mut painter = ui.painter().clone();
        painter.multiply_opacity(alpha);
        painter.text(inner.left_top(), Align2::LEFT_TOP, "What Bootmend is doing", FontId::new(12.0, theme::semibold()), p.muted);
        let list = Rect::from_min_max(inner.min + vec2(0.0, 24.0), inner.max - vec2(0.0, TAPE));
        ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
            ui.multiply_opacity(alpha);
            egui::ScrollArea::vertical().id_salt("run-log").stick_to_bottom(true).auto_shrink(false).show(ui, |ui| {
                for &i in &self.order {
                    let it = &self.items[i];
                    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
                    let painter = ui.painter();
                    let (g, c) = match it.state {
                        State::Done => (icon::CHECK, p.green),
                        State::Failed(_) => (icon::WARN, p.amber),
                        _ => (it.icon, p.accent),
                    };
                    painter.text(pos2(row.left() + 7.0, row.center().y), Align2::CENTER_CENTER, g, FontId::new(14.0, theme::icons()), c);
                    let ms = match it.state {
                        State::Now => now - it.began,
                        _ => it.took.unwrap_or(0.0),
                    };
                    let took = if ms < 60_000.0 { format!("{:.1} s", ms / 1000.0) } else { format!("{} min", (ms / 60_000.0) as u32) };
                    let time = painter.layout_no_wrap(took, FontId::new(12.0, theme::semibold()), p.muted);
                    let tx = row.right() - time.size().x - S1;
                    painter.galley(pos2(tx, row.center().y - time.size().y / 2.0), time, p.muted);
                    let detail = match (&it.state, self.mode) {
                        (State::Now, Mode::Fix) => "Working on it".to_string(),
                        (State::Now, Mode::Check) => "Checking".to_string(),
                        (State::Failed(e), Mode::Fix) if !e.is_empty() => e.clone(),
                        (State::Failed(_), _) => "Couldn't finish".to_string(),
                        (_, Mode::Fix) => it.sub.clone(),
                        (_, Mode::Check) => "Done".to_string(),
                    };
                    let x0 = row.left() + 22.0;
                    let avail = (tx - S3 - x0).max(40.0);
                    let title = egui::WidgetText::from(RichText::new(&it.title).color(p.ink)).into_galley(
                        ui,
                        Some(egui::TextWrapMode::Truncate),
                        avail * 0.55,
                        egui::TextStyle::Body,
                    );
                    let tw = title.size().x;
                    painter.galley(pos2(x0, row.center().y - title.size().y / 2.0), title, p.ink);
                    let rest = avail - tw - S2;
                    if rest > 24.0 && !detail.is_empty() {
                        let d = egui::WidgetText::from(RichText::new(detail).small().color(p.muted)).into_galley(
                            ui,
                            Some(egui::TextWrapMode::Truncate),
                            rest,
                            egui::TextStyle::Small,
                        );
                        painter.galley(pos2(x0 + tw + S2, row.center().y - d.size().y / 2.0), d, p.muted);
                    }
                }
            });
        });
    }
}

/// The panel's edge as one line: along the bottom from its left corner, up the right, across
/// the top and down the left, back to the start.
fn tape_path(r: Rect, rad: f32) -> Vec<Pos2> {
    let mut pts = vec![pos2(r.left() + rad, r.bottom()), pos2(r.right() - rad, r.bottom())];
    let corner = |pts: &mut Vec<Pos2>, c: Pos2, a0: f32| {
        for k in 1..=8 {
            let a = (a0 + 90.0 * k as f32 / 8.0).to_radians();
            pts.push(pos2(c.x + rad * a.cos(), c.y - rad * a.sin()));
        }
    };
    corner(&mut pts, pos2(r.right() - rad, r.bottom() - rad), -90.0);
    pts.push(pos2(r.right(), r.top() + rad));
    corner(&mut pts, pos2(r.right() - rad, r.top() + rad), 0.0);
    pts.push(pos2(r.left() + rad, r.top()));
    corner(&mut pts, pos2(r.left() + rad, r.top() + rad), 90.0);
    pts.push(pos2(r.left(), r.bottom() - rad));
    corner(&mut pts, pos2(r.left() + rad, r.bottom() - rad), 180.0);
    pts
}

fn path_len(pts: &[Pos2]) -> f32 {
    pts.windows(2).map(|w| w[0].distance(w[1])).sum()
}

/// The first `d` points' worth of a line.
fn cut_path(pts: &[Pos2], d: f32) -> Vec<Pos2> {
    let mut out = vec![];
    let Some(&first) = pts.first() else { return out };
    out.push(first);
    let mut left = d;
    for w in pts.windows(2) {
        let l = w[0].distance(w[1]);
        if left <= l {
            if left > 0.0 {
                out.push(w[0] + (w[1] - w[0]) * (left / l.max(1e-6)));
            }
            return out;
        }
        out.push(w[1]);
        left -= l;
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Picking up after Windows' permission prompt, which starts Bootmend again as administrator.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Then {
    Restore,
    TurnOn,
    Start,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResumeItem {
    /// None is the leftover files.
    pub fix: Option<Fix>,
    pub title: String,
    pub ticked: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resume {
    pub items: Vec<ResumeItem>,
    /// The page the queue was opened from, by slug.
    pub origin: String,
    pub then: Then,
    /// Seconds since 1970.
    pub when: u64,
}

impl Resume {
    pub fn of(run: &Run, then: Then, when: u64) -> Resume {
        let items = run
            .items
            .iter()
            .filter_map(|i| match &i.work {
                Work::Fix(f) => Some(ResumeItem { fix: Some(f.clone()), title: i.title.clone(), ticked: i.ticked }),
                Work::Clean(_) => Some(ResumeItem { fix: None, title: i.title.clone(), ticked: i.ticked }),
                Work::Step(_) => None,
            })
            .collect();
        Resume { items, origin: run.origin.slug(), then, when }
    }

    /// A queue left more than ten minutes ago isn't picked up: the person may have moved on.
    pub fn fresh(&self, now: u64) -> bool {
        now >= self.when && now - self.when <= 600
    }

    pub fn origin(&self) -> Page {
        Page::ALL.iter().copied().find(|p| p.slug() == self.origin).unwrap_or(Page::Home)
    }
}

/// A queue of made-up to-dos for the screenshots: while choosing, or part way through with the
/// first box done and the second under way.
pub(super) fn sample(stage: Stage, details: bool) -> Run {
    let items = vec![
        Item::fix(Fix::EnablePua, "Protection from unwanted apps is off", Area::Security),
        Item::fix(Fix::DisableSmb1, "An old, unsafe way of sharing files is on", Area::Security),
        Item::fix(Fix::EnableTrim, "Your SSD isn't being kept tidy", Area::Speed),
        Item::clean(vec![], "Clear 3.2 GB of leftover files"),
    ];
    let mut r = Run::fixes(items, Page::Home, Gate::Needed);
    for _ in 0..120 {
        r.step(16.0);
    }
    if stage == Stage::Choose {
        return r;
    }
    r.gate = Gate::Saved;
    r.start();
    for _ in 0..600 {
        if r.items[0].state == State::Now && r.items[0].result.is_none() {
            r.on_result(0, Ok(()), 400.0);
        }
        if r.items[1].state == State::Now {
            break;
        }
        r.step(16.0);
    }
    for _ in 0..40 {
        r.step(16.0);
    }
    if details {
        r.toggle_details();
        for _ in 0..90 {
            r.step(16.0);
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixes(n: usize) -> Run {
        let all = [Fix::EnablePua, Fix::DisableSmb1, Fix::EnableTrim];
        let items = all.into_iter().take(n).map(|f| Item::fix(f, "A to-do", Area::Security)).collect();
        Run::fixes(items, Page::Home, Gate::Saved)
    }

    /// Runs until the work is over, answering each fix the moment it starts. Returns every
    /// value `step` gave and the progress after each frame.
    fn play(run: &mut Run, answer: impl Fn(usize) -> Result<(), String>) -> (Vec<Option<bool>>, Vec<f32>) {
        let (mut outs, mut ps) = (vec![], vec![]);
        for _ in 0..2000 {
            for i in 0..run.items.len() {
                if run.items[i].state == State::Now && run.items[i].result.is_none() && run.items[i].took.is_none() {
                    run.on_result(i, answer(i), 50.0);
                }
            }
            outs.push(run.step(16.0));
            ps.push(run.p);
        }
        (outs, ps)
    }

    /// A repair can't be undone and takes many minutes, so the queue neither promises an undo nor
    /// races its bar to the end.
    #[test]
    fn a_repair_promises_no_undo_and_its_bar_creeps() {
        let repair = || Item::fix(Fix::RepairWindowsFiles, "Some of Windows' own files are damaged", Area::Stability);
        let mut one = Run::fixes(vec![repair()], Page::Home, Gate::Saved);
        assert!(one.items[0].sub.contains("up to 30 minutes"), "{}", one.items[0].sub);
        let two = Run::fixes(vec![Item::fix(Fix::EnablePua, "Unwanted apps", Area::Security), repair()], Page::Home, Gate::Saved);
        for r in [&one, &two] {
            assert!(!r.heads().1.contains("undo"), "{:?}", r.heads());
        }
        one.start();
        assert!(!one.line.text.contains("undo"), "{}", one.line.text);
        for _ in 0..4000 {
            one.step(16.0);
        }
        assert_eq!(one.items[0].state, State::Now);
        assert!(one.p > 0.0 && one.p < 0.2, "a minute into a half-hour job: {}", one.p);
        play(&mut one, |_| Ok(()));
        one.ticked_now();
        assert_eq!(one.heads().0, "Fixed");
        assert!(!one.heads().1.contains("undo"), "{:?}", one.heads());
    }

    #[test]
    fn fixes_run_in_order_each_shown_long_enough_and_end_once() {
        let mut r = fixes(3);
        r.start();
        let mut began = vec![];
        let mut resolved = vec![];
        let (mut outs, mut ps) = (vec![], vec![]);
        for _ in 0..1000 {
            for (i, it) in r.items.iter().enumerate() {
                if it.state == State::Now && !began.iter().any(|&(j, _)| j == i) {
                    began.push((i, r.clock));
                }
                if it.state.resolved() && !resolved.iter().any(|&(j, _)| j == i) {
                    resolved.push((i, r.clock));
                }
            }
            for i in 0..3 {
                if r.items[i].state == State::Now && r.items[i].took.is_none() {
                    r.on_result(i, Ok(()), 10.0);
                }
            }
            outs.push(r.step(16.0));
            ps.push(r.p);
        }
        assert_eq!(began.iter().map(|b| b.0).collect::<Vec<_>>(), vec![0, 1, 2], "in order");
        assert!(began[0].1 >= FIRST_AT - 16.0);
        for ((i, b), (_, e)) in began.iter().zip(&resolved) {
            assert!(e - b >= MIN_NOW, "box {i} shown for {} ms", e - b);
        }
        assert!(ps.windows(2).all(|w| w[1] >= w[0]), "progress never goes back");
        assert_eq!(outs.iter().flatten().collect::<Vec<_>>(), vec![&true], "says it's over once");
        assert_eq!(r.p, 1.0);
        assert!(r.items.iter().all(|i| i.left_at.is_some()), "done boxes leave");
    }

    #[test]
    fn a_failed_fix_stays_and_the_run_reports_it() {
        let mut r = fixes(2);
        r.start();
        let (outs, _) = play(&mut r, |i| if i == 0 { Err("Access is denied".into()) } else { Ok(()) });
        assert_eq!(outs.iter().flatten().collect::<Vec<_>>(), vec![&false]);
        assert_eq!(r.items[0].state, State::Failed("Access is denied".into()));
        assert!(r.items[0].left_at.is_none(), "it stays on screen");
        assert_eq!(r.items[1].state, State::Done);
        r.ticked_now();
        assert_eq!(r.heads(), ("1 of 2 fixed".to_string(), "One needs you. It's waiting on Home.".to_string()));
        assert_eq!(r.box_sub(&r.items[0]), FAILED_FIX);
    }

    #[test]
    fn unticked_fixes_are_skipped_and_leave() {
        let mut r = fixes(3);
        r.toggle(1);
        r.start();
        play(&mut r, |_| Ok(()));
        assert_eq!(r.items[1].state, State::Wait, "never started");
        assert!(r.items[1].left_at.is_some());
        assert_eq!(r.order, vec![0, 2]);
    }

    #[test]
    fn a_check_up_follows_its_steps_and_ends_with_the_result() {
        let mut r = Run::check(Page::Home, true);
        for _ in 0..200 {
            assert_eq!(r.step(16.0), None);
        }
        assert!(r.items.iter().all(|i| i.state == State::Now), "every part is being checked");
        r.on_step("system", true);
        r.on_step("network", false);
        for _ in 0..60 {
            assert_eq!(r.step(16.0), None, "not over before the result arrives");
        }
        assert_eq!(r.items[0].state, State::Done);
        assert_eq!(r.items[2].state, State::Failed(String::new()));
        assert!(r.p > 0.2 && r.p < 1.0);
        r.on_finished(Box::default());
        let outs: Vec<Option<bool>> = (0..100).map(|_| r.step(16.0)).collect();
        assert_eq!(outs.iter().flatten().collect::<Vec<_>>(), vec![&false], "one part couldn't finish");
        r.ticked_now();
        assert_eq!(r.heads().0, "Check-up done");
        assert_eq!(r.heads().1, "Your plan is ready.");
        assert_eq!(r.end_label(), "See what Bootmend found");
    }

    #[test]
    fn a_restore_point_from_today_opens_the_gate() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
        assert_eq!(Gate::start(false, Some("2026-11-15 08:02"), Some(true), today), Gate::Saved);
        assert_eq!(Gate::start(true, None, None, today), Gate::Saved);
        assert_eq!(Gate::start(false, Some("2026-11-14 23:59"), Some(true), today), Gate::Needed);
        assert_eq!(Gate::start(false, None, Some(false), today), Gate::Off);
        assert_eq!(Gate::start(false, None, None, today), Gate::Needed);
        assert_eq!(Gate::after(&Err("Windows skipped it because it already saved a restore point in the last 24 hours".into())), Gate::Saved);
        assert_eq!(Gate::after(&Err("The service is busy".into())), Gate::Failed("The service is busy".into()));
    }

    #[test]
    fn only_changes_to_windows_wait_for_a_restore_point() {
        let mut r = Run::fixes(vec![Item::clean(vec!["temp"], "Clear 2 GB of leftover files")], Page::Home, Gate::Needed);
        assert!(!r.needs_gate() && r.ready(), "leftovers alone start at once");
        r.items.push(Item::fix(Fix::EnablePua, "Unwanted apps", Area::Security));
        assert!(r.needs_gate() && !r.ready());
        r.gate = Gate::Saved;
        assert!(r.ready());
        r.toggle(0);
        r.toggle(1);
        assert!(!r.ready(), "nothing ticked");
    }

    #[test]
    fn every_stage_says_where_things_stand() {
        let mut r = fixes(3);
        assert_eq!(r.heads().0, "Ready to fix 3 things");
        r.toggle(0);
        r.toggle(1);
        assert_eq!(r.heads().0, "Ready to fix 1 thing");
        r.toggle(2);
        assert_eq!(r.heads(), ("Nothing picked".to_string(), "Tick anything you'd like Bootmend to fix.".to_string()));
        let one = fixes(1);
        assert_eq!(one.heads().0, "Ready to fix this");
        assert!(one.heads().1.contains("undo it from History"));
        let mut all = fixes(2);
        all.start();
        assert_eq!(all.heads(), ("Fixing 2 things".to_string(), "0 of 2 done".to_string()));
        play(&mut all, |_| Ok(()));
        all.ticked_now();
        assert_eq!(all.heads().0, "All 2 fixed");
        let mut bad = fixes(1);
        bad.start();
        play(&mut bad, |_| Err("no".into()));
        bad.ticked_now();
        assert_eq!(bad.heads(), ("That didn't work".to_string(), "It'll wait on Home for another go.".to_string()));
        assert_eq!(bad.line.text, "Done, apart from one that needs you.");
        let check = Run::check(Page::Security, false);
        assert_eq!(check.heads().0, "Checking your PC");
        assert_eq!(check.end_label(), "See Home");
        assert_eq!(Run::fixes(vec![], Page::Security, Gate::Saved).end_label(), "Back to Security");
    }

    #[test]
    fn a_queue_survives_the_restart_as_administrator() {
        let mut r = fixes(2);
        r.items.push(Item::clean(vec!["temp"], "Clear 2 GB of leftover files"));
        r.toggle(1);
        r.origin = Page::Security;
        let saved = Resume::of(&r, Then::Restore, 1_000);
        let back: Resume = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        assert_eq!(back, saved);
        assert_eq!(back.items.len(), 3);
        assert!(!back.items[1].ticked);
        assert_eq!(back.items[2].fix, None);
        assert_eq!(back.origin(), Page::Security);
        assert!(back.fresh(1_000 + 600));
        assert!(!back.fresh(1_000 + 601));
        assert!(!back.fresh(999), "a clock that went back isn't trusted");
    }

    #[test]
    fn the_tape_wraps_the_whole_panel_and_cuts_anywhere() {
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 200.0));
        let path = tape_path(r, 10.0);
        let len = path_len(&path);
        let want = 2.0 * (280.0 + 180.0) + 2.0 * std::f32::consts::PI * 10.0;
        assert!((len - want).abs() < 0.5, "{len} vs {want}");
        let half = cut_path(&path, 140.0);
        assert_eq!(*half.last().unwrap(), pos2(150.0, 200.0), "the first part runs along the bottom");
        assert!((path_len(&cut_path(&path, len * 2.0)) - len).abs() < 1e-3);
    }
}
