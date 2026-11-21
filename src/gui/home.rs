//! Home: where the PC stands in one sentence, and the plan to work through.

use super::theme::{self, pal, RADIUS, S1, S2, S3, S4, S5, S6};
use super::widgets::{self, icon, small};
use super::{run, App, Page, Slot, COMPACT};
use crate::cleanup::size_text;
use crate::fixes::Fix;
use crate::rules::{plural, Action, Area, Finding, Level};
use crate::snapshot::Snapshot;
use eframe::egui::{self, CornerRadius, Margin, RichText, Sense, Stroke, Ui};
use std::collections::BTreeSet;
use std::sync::Arc;

/// How far through the plan the person is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Progress {
    pub do_first: usize,
    pub worth: usize,
    pub nice: usize,
    pub done: usize,
}

impl Progress {
    pub fn open(&self) -> usize {
        self.do_first + self.worth + self.nice
    }
    pub fn total(&self) -> usize {
        self.open() + self.done
    }
    /// The keystone waits above its seat while anything that matters is still open.
    pub fn raised(&self) -> bool {
        self.do_first + self.worth > 0
    }
}

/// Counts the plan from each to-do's level and whether it's done.
pub fn progress(todos: impl IntoIterator<Item = (Level, bool)>) -> Progress {
    let mut p = Progress::default();
    for (level, done) in todos {
        match (level, done) {
            (Level::AllGood, _) => {}
            (_, true) => p.done += 1,
            (Level::DoFirst, false) => p.do_first += 1,
            (Level::WorthDoing, false) => p.worth += 1,
            (Level::NiceToHave, false) => p.nice += 1,
        }
    }
    p
}

/// The page title and the line under it.
pub fn verdict(p: Progress) -> (String, String) {
    const CALM: &str = "Nothing changes until you press a button.";
    let things = |n: usize, one: &str, many: &str| if n == 1 { format!("1 thing {one}") } else { format!("{n} things {many}") };
    let then = |lead: String| if lead.is_empty() { CALM.to_string() } else { format!("{lead} {CALM}") };
    if p.total() == 0 {
        return ("Your PC is in good shape".into(), "Bootmend checked everything and found nothing to do.".into());
    }
    if p.open() == 0 {
        return ("All done".into(), "Everything on the list is fixed. The next check shows anything new.".into());
    }
    if p.do_first > 0 {
        let rest = p.worth + p.nice;
        let lead = if rest > 0 { format!("{rest} more can wait for a quiet moment.") } else { String::new() };
        return (things(p.do_first, "needs you soon", "need you soon"), then(lead));
    }
    if p.worth > 0 {
        let lead = if p.nice > 0 { format!("{} more if you have a moment.", p.nice) } else { String::new() };
        return (things(p.worth, "worth doing", "worth doing"), then(lead));
    }
    let lead = format!("{} if you have a moment.", plural(p.nice, "small improvement", "small improvements"));
    ("Your PC is in good shape".into(), then(lead))
}

/// Clearing leftovers is an improvement, unless the system drive is short of room; then it
/// helps with that and takes the same level.
pub fn leftovers_level(findings: &[Finding]) -> Level {
    findings.iter().find(|f| f.id == "space-C:").map_or(Level::NiceToHave, |f| f.level)
}

/// Leftovers smaller than this aren't worth a line on the plan.
const LEFTOVERS_MIN: u64 = 1 << 30;
const LEFTOVERS: &str = "leftovers";
/// The folded list of ignored to-dos, in the set of open rows.
const IGNORED: &str = "ignored";

/// The leftover files Bootmend clears by itself, as a line on the plan.
struct Leftovers {
    todo: Finding,
    done: bool,
    /// What's still to clear, and its size.
    ids: Vec<&'static str>,
}

/// The plan on Home: every to-do and whether it's done, the leftovers line, the count, and the
/// to-dos the person set aside.
struct Plan {
    todos: Vec<(Finding, bool)>,
    leftovers: Option<Leftovers>,
    prog: Progress,
    ignored: Vec<Finding>,
}

/// Sets the ignored to-dos apart, both lists keeping the plan's order. An ignored id the check
/// no longer finds is simply left out.
fn split_ignored(todos: Vec<(Finding, bool)>, ignored: &BTreeSet<String>) -> (Vec<(Finding, bool)>, Vec<Finding>) {
    let (set_aside, kept): (Vec<_>, Vec<_>) = todos.into_iter().partition(|(f, _)| ignored.contains(&f.id));
    (kept, set_aside.into_iter().map(|(f, _)| f).collect())
}

impl App {
    /// Whether this fix was made since the check and not undone.
    pub(super) fn fixed(&self, fx: &Fix) -> bool {
        let since = self.snap.as_ref().map_or("", |s| s.taken.as_str());
        self.history.iter().any(|h| h.fix == *fx && h.undone.is_none() && h.when.as_str() >= since)
    }

    /// A to-do is done once every fix it offers has been made. One that needs the person's own
    /// hands stays open until the next check finds it gone.
    fn todo_done(&self, f: &Finding) -> bool {
        let mut fixes = f.actions.iter().filter_map(|a| if let Action::Fix(x) = a { Some(x) } else { None }).peekable();
        fixes.peek().is_some() && fixes.all(|x| self.fixed(x))
    }

    fn leftovers(&self) -> Option<Leftovers> {
        let v = self.cleanup.as_ref()?;
        let safe = v.safe(self.elevated);
        let bytes: u64 = safe.iter().filter_map(|i| i.bytes).sum();
        if bytes < LEFTOVERS_MIN {
            return None;
        }
        let cleared = |id: &str| matches!(v.done.get(id), Some(Ok(_)));
        let left_items: Vec<_> = safe.iter().filter(|i| !cleared(i.id)).collect();
        let todo = Finding::new(LEFTOVERS, Area::Speed, leftovers_level(&self.findings), format!("Clear {} of leftover files", size_text(bytes)))
            .why("Temporary files, caches and old update files. Your own files are never touched.")
            .details(safe.iter().map(|i| format!("{}: {}", i.title, i.bytes.map_or("size shows after clearing".to_string(), size_text))));
        Some(Leftovers { todo, done: left_items.is_empty(), ids: left_items.iter().map(|i| i.id).collect() })
    }

    fn plan(&self) -> Plan {
        let mut todos: Vec<(Finding, bool)> = self.findings.iter().filter(|f| f.level != Level::AllGood).map(|f| (f.clone(), self.todo_done(f))).collect();
        let leftovers = self.leftovers();
        if let Some(l) = &leftovers {
            todos.push((l.todo.clone(), l.done));
        }
        // Stable, so the leftovers line comes last within its level.
        todos.sort_by_key(|(f, _)| f.level);
        let (todos, ignored) = split_ignored(todos, &self.settings.ignored);
        let prog = progress(todos.iter().map(|(f, d)| (f.level, *d)));
        Plan { todos, leftovers, prog, ignored }
    }

    /// Takes a to-do off the plan for good, until the person shows it again.
    fn ignore(&mut self, f: &Finding) {
        log!("settings", "ignored {}", f.id);
        self.settings.ignored.insert(f.id.clone());
        self.save_settings();
        self.ui.expanded.remove(&f.id);
        self.toast("Ignored. You'll find it at the end of Home.", true);
    }

    fn show_again(&mut self, f: &Finding) {
        log!("settings", "showing {} again", f.id);
        self.settings.ignored.remove(&f.id);
        self.save_settings();
        self.toast("Back on the plan.", true);
    }

    /// Whether the keystone waits above its seat: something that matters is still open.
    pub(super) fn raised(&self) -> bool {
        self.plan().prog.raised()
    }

    /// The leftovers line, for picking up a queue after a restart.
    pub(super) fn leftovers_item(&self) -> Option<run::Item> {
        self.leftovers().filter(|l| !l.done).map(|l| run::Item::clean(l.ids, l.todo.title))
    }

    /// The safe work Bootmend can do by itself, in the plan's order: each open to-do that one
    /// fix settles, and the leftover files.
    fn safe_queue(&self, plan: &Plan) -> Vec<run::Item> {
        plan.todos
            .iter()
            .filter(|(_, done)| !done)
            .filter_map(|(f, _)| {
                if f.id == LEFTOVERS {
                    return plan.leftovers.as_ref().map(|l| run::Item::clean(l.ids.clone(), f.title.clone()));
                }
                match fixes_of(f).as_slice() {
                    [fx] if fx.safe() => Some(run::Item::fix((*fx).clone(), f.title.clone(), f.area)),
                    _ => None,
                }
            })
            .collect()
    }

    pub(super) fn home(&mut self, ui: &mut Ui, snap: &Arc<Snapshot>) {
        let p = pal(ui.ctx());
        // Measuring leftovers only reads folder sizes; clearing waits for a press.
        if self.cleanup.is_none() && !self.busy.contains("cleanup-look") {
            self.start_cleanup_look(&ui.ctx().clone());
        }
        let plan = self.plan();
        let queue = self.safe_queue(&plan);
        let safe: Vec<&'static str> = plan.leftovers.as_ref().filter(|l| !l.done).map(|l| l.ids.clone()).unwrap_or_default();
        self.hero(ui, snap, plan.prog, queue);
        if !snap.errors.is_empty() {
            ui.add_space(S5);
            widgets::note(ui, p.amber, p.amber_bg, icon::WARN, |ui| {
                ui.label(format!("{} couldn't finish, so a few details may be missing.", plural(snap.errors.len(), "part of the check", "parts of the check")));
                ui.collapsing("Show what happened", |ui| {
                    for e in &snap.errors {
                        ui.add(egui::Label::new(RichText::new(e).small()).wrap());
                    }
                });
            });
        }
        self.update_note(ui);
        ui.add_space(S6);
        self.plan_list(ui, &plan.todos, &plan.ignored, &safe);
        self.schedule_offer(ui);
    }

    /// The verdict as the page title beside the Keystone, the progress line and the one button
    /// that opens the safe work as a queue.
    fn hero(&mut self, ui: &mut Ui, snap: &Snapshot, prog: Progress, queue: Vec<run::Item>) {
        let p = pal(ui.ctx());
        let (title, lede) = verdict(prog);
        let avail = ui.available_width();
        let wide = avail >= 640.0;
        // Short of room, the Keystone and verdict stay side by side and the rest moves under
        // both, the way Settings moves the Windows Update tile under the PC's name.
        let compact = avail < COMPACT;
        let size = if wide { 112.0 } else { 72.0 };
        // The keystone drops into its seat the moment the last thing that matters is fixed, and
        // rises at once if something new turns up.
        if !self.mark.running() {
            self.mark.set_lift(if prog.raised() { 1.0 } else { 0.0 }, self.snap_mode.is_some());
        }
        let cleaning = self.busy.contains("cleanup");
        let n = queue.len();
        let mut fix = false;
        let rest = |ui: &mut Ui, fix: &mut bool| {
            ui.add(egui::Label::new(RichText::new(&lede).text_style(theme::lede()).color(p.muted)).wrap());
            if prog.total() > 0 {
                ui.add_space(S1);
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - 120.0).clamp(120.0, 420.0);
                    widgets::segments(ui, "The plan", prog.done, prog.total(), w);
                    ui.add_space(S2);
                    small(ui, format!("{} of {} done", prog.done, prog.total()));
                });
            }
            if n > 0 {
                ui.add_space(S2);
                ui.horizontal(|ui| {
                    if cleaning {
                        ui.add(egui::Spinner::new().color(p.accent));
                        widgets::muted(ui, "Clearing leftover files");
                    } else {
                        let label = if n == 1 { "Fix the safe one for me".to_string() } else { format!("Fix the {n} safe ones for me") };
                        *fix = widgets::primary(ui, &label).clicked();
                    }
                });
            }
        };
        let row = ui.horizontal_top(|ui| {
            let (slot, _) = ui.allocate_exact_size(egui::vec2(size, size), Sense::hover());
            ui.add_space(if wide { S6 } else { S5 });
            ui.vertical(|ui| {
                ui.set_max_width(ui.available_width().min(560.0));
                ui.spacing_mut().item_spacing.y = S2;
                let when = crate::util::when_words(&snap.taken, chrono::Local::now().date_naive());
                small(ui, format!("Checked {when}"));
                // Title Large where there's room, Title where there isn't, as Settings' page titles.
                let style = if compact { egui::TextStyle::Heading } else { theme::big() };
                let r = ui.add(egui::Label::new(RichText::new(&title).text_style(style)).wrap());
                widgets::heading_role(ui, &r, 1, &title);
                if !compact {
                    rest(ui, &mut fix);
                }
            });
            slot
        });
        let centre = egui::pos2(row.inner.center().x, row.response.rect.center().y);
        self.ui.mark_slot = Some((Slot::Home, egui::Rect::from_center_size(centre, egui::vec2(size, size))));
        if compact {
            ui.add_space(S3);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = S2;
                rest(ui, &mut fix);
            });
        }
        if fix {
            self.open_fixes(queue, Page::Home);
        }
    }

    /// One card holding the whole plan, grouped by level, then what was checked and found fine,
    /// then what the person ignored.
    fn plan_list(&mut self, ui: &mut Ui, todos: &[(Finding, bool)], ignored: &[Finding], safe_ids: &[&'static str]) {
        let p = pal(ui.ctx());
        let fine: Vec<Finding> = self.findings.iter().filter(|f| f.level == Level::AllGood && !f.id.ends_with("-clear")).cloned().collect();
        if todos.is_empty() && fine.is_empty() && ignored.is_empty() {
            return;
        }
        egui::Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, p.line))
            .corner_radius(CornerRadius::same(RADIUS))
            .inner_margin(Margin::symmetric(S4 as i8, S1 as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let mut first = true;
                for level in [Level::DoFirst, Level::WorthDoing, Level::NiceToHave] {
                    let rows: Vec<&(Finding, bool)> = todos.iter().filter(|(f, _)| f.level == level).collect();
                    if rows.is_empty() {
                        continue;
                    }
                    if !first {
                        rule(ui);
                    }
                    first = false;
                    group_head(ui, level.name(), rows.len(), None);
                    for (i, (f, done)) in rows.into_iter().enumerate() {
                        if i > 0 {
                            rule(ui);
                        }
                        self.todo_row(ui, f, *done, safe_ids);
                    }
                }
                if !fine.is_empty() {
                    if !first {
                        rule(ui);
                    }
                    first = false;
                    let open = self.ui.expanded.contains("all-good");
                    if group_head(ui, Level::AllGood.name(), fine.len(), Some(open)) {
                        if open {
                            self.ui.expanded.remove("all-good");
                        } else {
                            self.ui.expanded.insert("all-good".into());
                        }
                    }
                    if open {
                        for f in &fine {
                            ui.horizontal_top(|ui| {
                                ui.add_sized(egui::vec2(20.0, 20.0), widgets::Glyph(widgets::ico(icon::CHECK, 14.0).color(p.green)));
                                ui.add_space(S4);
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&f.title).family(theme::semibold()));
                                    if !f.summary.is_empty() {
                                        ui.add(egui::Label::new(RichText::new(&f.summary).small().color(p.muted)).wrap());
                                    }
                                });
                            });
                            ui.add_space(S3);
                        }
                    }
                }
                if !ignored.is_empty() {
                    if !first {
                        rule(ui);
                    }
                    let open = self.ui.expanded.contains(IGNORED);
                    if group_head(ui, "Ignored", ignored.len(), Some(open)) {
                        if open {
                            self.ui.expanded.remove(IGNORED);
                        } else {
                            self.ui.expanded.insert(IGNORED.into());
                        }
                    }
                    if open {
                        for (i, f) in ignored.iter().enumerate() {
                            if i > 0 {
                                rule(ui);
                            }
                            self.ignored_row(ui, f);
                        }
                    }
                }
            });
    }

    /// An ignored to-do: what it is, quietly, and the button that puts it back on the plan.
    fn ignored_row(&mut self, ui: &mut Ui, f: &Finding) {
        let avail = ui.available_width();
        let compact = avail < COMPACT;
        let indent = 20.0 + ui.spacing().item_spacing.x + S4;
        let text_w = if compact { avail - indent } else { (avail - indent - 140.0).max(160.0) };
        let (title, summary, h) = row_text(ui, f, text_w);
        let text = |ui: &mut Ui| {
            // Muted whatever its level: it's set aside.
            mark(ui, f.area, Level::NiceToHave, false);
            ui.add_space(S4);
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(title);
                if !f.summary.is_empty() {
                    ui.label(summary);
                }
            });
        };
        let mut back = false;
        if compact {
            ui.add_space(S3);
            ui.horizontal(text);
            ui.add_space(S2);
            ui.horizontal(|ui| {
                ui.add_space(indent);
                back = named(widgets::secondary(ui, "Show again"), &format!("Show again: {}", f.title)).clicked();
            });
            ui.add_space(S3);
        } else {
            ui.allocate_ui_with_layout(egui::vec2(avail, h), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                back = named(widgets::secondary(ui, "Show again"), &format!("Show again: {}", f.title)).clicked();
                ui.add_space(S4);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), text);
            });
        }
        if back {
            self.show_again(f);
        }
    }

    /// One to-do: its area's icon (a tick once it's done), what it is, and one button. "Fix it for me"
    /// when one change does it; "Walk me through it" opens the steps in place.
    fn todo_row(&mut self, ui: &mut Ui, f: &Finding, done: bool, safe_ids: &[&'static str]) {
        let fixes = fixes_of(f);
        let leftovers = f.id == LEFTOVERS;
        let copy = fixes.is_empty() && f.actions.contains(&Action::CopyFiles);
        let one_fix = leftovers || fixes.len() == 1 || copy;
        let more = !f.steps.is_empty() || !f.details.is_empty() || f.actions.len() > usize::from(fixes.len() == 1 || copy);
        let open = self.ui.expanded.contains(&f.id);
        let avail = ui.available_width();
        // Short of room, the button moves under the text. Otherwise its column is measured
        // generously, so a row's text never runs under it.
        let compact = avail < COMPACT;
        // The mark, the gap the layout puts after it, and the space before the words.
        let indent = 20.0 + ui.spacing().item_spacing.x + S4;
        let action_w = 300.0;
        let text_w = if compact { avail - indent } else { (avail - indent - action_w).max(160.0) };
        let (title, summary, h) = row_text(ui, f, text_w);
        let mut toggle = false;
        let mut clear = false;
        let mut ignore = false;
        let chevron = more && (one_fix || done);
        let text = |ui: &mut Ui| {
            mark(ui, f.area, f.level, done);
            ui.add_space(S4);
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(title);
                if !f.summary.is_empty() {
                    ui.label(summary);
                }
            });
        };
        if compact {
            ui.add_space(S3);
            ui.horizontal(text);
            ui.add_space(S2);
            ui.horizontal(|ui| {
                ui.add_space(indent);
                (toggle, clear) = self.todo_action(ui, f, &fixes, done, open, false);
                if chevron {
                    ui.add_space(S1);
                    toggle |= widgets::chevron(ui, open).clicked();
                }
                if !done {
                    ui.add_space(S1);
                    ignore = ignore_button(ui, &f.title);
                }
            });
            ui.add_space(S3);
        } else {
            ui.allocate_ui_with_layout(egui::vec2(avail, h), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if chevron {
                    toggle = widgets::chevron(ui, open).clicked();
                    ui.add_space(S1);
                }
                let (t, c) = self.todo_action(ui, f, &fixes, done, open, true);
                toggle |= t;
                clear = c;
                if !done {
                    ui.add_space(S1);
                    ignore = ignore_button(ui, &f.title);
                }
                ui.add_space(S4);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), text);
            });
        }
        if toggle {
            if open {
                self.ui.expanded.remove(&f.id);
            } else {
                self.ui.expanded.insert(f.id.clone());
            }
        }
        if clear {
            self.open_fixes(vec![run::Item::clean(safe_ids.to_vec(), f.title.clone())], Page::Home);
        }
        if ignore {
            self.ignore(f);
            return;
        }
        if open && more {
            ui.horizontal(|ui| {
                ui.add_space(20.0 + S4);
                ui.vertical(|ui| {
                    if leftovers {
                        ui.add_space(S1);
                        for d in &f.details {
                            widgets::muted(ui, d);
                        }
                        ui.add_space(S2);
                        if ui.link("Choose what to clear").clicked() {
                            self.go(Page::Cleanup);
                        }
                    } else {
                        self.finding_body(ui, f, one_fix);
                    }
                });
            });
            ui.add_space(S3);
        }
    }
}

impl App {
    /// A to-do's one button, or what stands in for it: "Done", or a spinner while it works.
    /// `rtl` is true when the row is filled from the right. Returns whether the steps were asked
    /// for, and whether to clear the leftover files.
    fn todo_action(&mut self, ui: &mut Ui, f: &Finding, fixes: &[&Fix], done: bool, open: bool, rtl: bool) -> (bool, bool) {
        let p = pal(ui.ctx());
        let (mut toggle, mut clear) = (false, false);
        let spinner = |ui: &mut Ui| {
            ui.add(egui::Spinner::new().color(p.accent));
        };
        if done {
            status(
                ui,
                rtl,
                |ui| {
                    ui.add(widgets::Glyph(widgets::ico(icon::CHECK, 14.0).color(p.green)));
                },
                |ui| {
                    ui.label(RichText::new("Done").color(p.green));
                },
            );
        } else if f.id == LEFTOVERS {
            if self.busy.contains("cleanup") {
                status(ui, rtl, spinner, |ui| {
                    widgets::muted(ui, "Clearing");
                });
            } else {
                clear = widgets::secondary(ui, "Fix it for me").clicked();
            }
        } else if let [fx] = fixes {
            if widgets::secondary(ui, "Fix it for me").on_hover_text(fx.label()).clicked() {
                self.open_fixes(vec![run::Item::fix((*fx).clone(), f.title.clone(), f.area)], Page::Home);
            }
        } else if fixes.is_empty() && f.actions.contains(&Action::CopyFiles) {
            if widgets::secondary(ui, "Copy my files").on_hover_text("Copy your Documents, Desktop and Pictures to a USB drive").clicked() {
                self.open_copy(&ui.ctx().clone());
            }
        } else {
            toggle = widgets::secondary(ui, if open { "Hide the steps" } else { "Walk me through it" }).clicked();
        }
        (toggle, clear)
    }
}

/// A to-do's title and summary laid out `w` wide, and the height of the row around them.
fn row_text(ui: &Ui, f: &Finding, w: f32) -> (Arc<egui::Galley>, Arc<egui::Galley>, f32) {
    let wrap = Some(egui::TextWrapMode::Wrap);
    let title = egui::WidgetText::from(RichText::new(&f.title).family(theme::semibold())).into_galley(ui, wrap, w, egui::TextStyle::Body);
    let summary = egui::WidgetText::from(RichText::new(&f.summary).small().color(pal(ui.ctx()).muted)).into_galley(ui, wrap, w, egui::TextStyle::Small);
    let h = (title.size().y + 2.0 + summary.size().y).max(32.0) + 2.0 * S3;
    (title, summary, h)
}

/// The quiet button that takes a to-do off the plan, muted until the pointer is on it.
fn ignore_button(ui: &mut Ui, title: &str) -> bool {
    ui.scope(|ui| {
        ui.visuals_mut().widgets.inactive.fg_stroke.color = pal(ui.ctx()).muted;
        let r = widgets::subtle_text(ui, "Ignore").on_hover_text("Take it off the plan. It waits at the end of Home.");
        named(r, &format!("Ignore: {title}")).clicked()
    })
    .inner
}

/// A button that every row repeats, named with its to-do for a screen reader.
fn named(r: egui::Response, name: &str) -> egui::Response {
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    r
}

/// The fixes a to-do offers.
fn fixes_of(f: &Finding) -> Vec<&Fix> {
    f.actions.iter().filter_map(|a| if let Action::Fix(x) = a { Some(x) } else { None }).collect()
}

/// An icon or spinner and its words, in reading order whichever way the row is filled.
fn status(ui: &mut Ui, rtl: bool, lead: impl FnOnce(&mut Ui), words: impl FnOnce(&mut Ui)) {
    if rtl {
        words(ui);
        lead(ui);
    } else {
        lead(ui);
        words(ui);
    }
}

/// A hairline between rows of the plan.
fn rule(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::ZERO, pal(ui.ctx()).line);
}

/// A level's name and how many to-dos it holds. With `open`, it can be opened and closed;
/// returns whether it was pressed.
fn group_head(ui: &mut Ui, name: &str, count: usize, open: Option<bool>) -> bool {
    let mut pressed = false;
    ui.add_space(S3);
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 32.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if let Some(o) = open {
            pressed = widgets::chevron(ui, o).clicked();
            ui.add_space(S1);
        }
        small(ui, count.to_string());
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            let r = ui.label(RichText::new(name).text_style(theme::title()));
            widgets::heading_role(ui, &r, 2, name);
        });
    });
    pressed
}

/// The mark at the start of a to-do: its area's icon, red for Do first, and a green tick once
/// it's done. An icon rather than a ring, so it never looks like a box to tick.
fn mark(ui: &mut Ui, area: Area, level: Level, done: bool) {
    let p = pal(ui.ctx());
    let (r, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::hover());
    let (glyph, color) = if done {
        (icon::CHECK, p.green)
    } else {
        let g = match area {
            Area::Security => icon::SHIELD,
            Area::Stability => icon::HEALTH,
            Area::Speed => icon::SPEED,
            Area::Hardware => icon::CHIP,
        };
        (g, if level == Level::DoFirst { p.red } else { p.muted })
    };
    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, glyph, egui::FontId::new(16.0, theme::icons()), color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Area;

    fn p(do_first: usize, worth: usize, nice: usize, done: usize) -> Progress {
        Progress { do_first, worth, nice, done }
    }

    #[test]
    fn progress_counts_open_to_dos_by_level_and_done_ones_apart() {
        let got =
            progress([(Level::DoFirst, false), (Level::DoFirst, true), (Level::WorthDoing, false), (Level::NiceToHave, false), (Level::NiceToHave, true)]);
        assert_eq!(got, p(1, 1, 1, 2));
        assert_eq!(got.total(), 5);
        assert!(got.raised());
        assert!(!p(0, 0, 3, 1).raised(), "improvements alone don't hold the keystone up");
    }

    fn todo(id: &str, level: Level) -> (Finding, bool) {
        (Finding::new(id, Area::Security, level, id), false)
    }

    #[test]
    fn ignored_to_dos_move_out_of_the_plan() {
        let todos = vec![todo("remote", Level::WorthDoing), todo("crashes", Level::DoFirst), todo("camera", Level::WorthDoing)];
        let ignored = BTreeSet::from(["remote".to_string(), "crashes".to_string(), "gone-since".to_string()]);
        let (kept, set_aside) = split_ignored(todos, &ignored);
        assert_eq!(kept.iter().map(|(f, _)| f.id.as_str()).collect::<Vec<_>>(), ["camera"]);
        // In the plan's order, and an id the check no longer finds is simply left out.
        assert_eq!(set_aside.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), ["remote", "crashes"]);
        assert_eq!(progress(kept.iter().map(|(f, d)| (f.level, *d))), p(0, 1, 0, 0));
    }

    #[test]
    fn all_good_findings_are_not_to_dos() {
        assert_eq!(progress([(Level::AllGood, false)]).total(), 0);
    }

    #[test]
    fn the_verdict_leads_with_what_needs_you_first() {
        assert_eq!(verdict(p(2, 3, 5, 2)).0, "2 things need you soon");
        assert_eq!(verdict(p(1, 0, 0, 0)).0, "1 thing needs you soon");
        assert!(verdict(p(2, 3, 5, 2)).1.starts_with("8 more can wait for a quiet moment."));
        assert_eq!(verdict(p(0, 3, 1, 0)).0, "3 things worth doing");
        assert_eq!(verdict(p(0, 1, 0, 0)).0, "1 thing worth doing");
        assert_eq!(verdict(p(0, 0, 2, 0)).0, "Your PC is in good shape");
        assert!(verdict(p(0, 0, 2, 0)).1.starts_with("2 small improvements if you have a moment."));
        assert!(verdict(p(0, 0, 1, 0)).1.starts_with("1 small improvement if you have a moment."));
    }

    #[test]
    fn the_verdict_says_when_everything_is_done_or_nothing_was_found() {
        assert_eq!(verdict(p(0, 0, 0, 4)).0, "All done");
        assert_eq!(verdict(p(0, 0, 0, 0)).0, "Your PC is in good shape");
        assert!(verdict(p(0, 0, 0, 0)).1.contains("nothing to do"));
    }

    #[test]
    fn every_open_verdict_promises_nothing_changes_without_a_press() {
        for x in [p(2, 0, 0, 0), p(0, 1, 1, 0), p(0, 0, 1, 0)] {
            assert!(verdict(x).1.ends_with("Nothing changes until you press a button."), "{x:?}");
        }
    }

    #[test]
    fn leftovers_follow_a_full_system_drive_and_are_otherwise_an_improvement() {
        assert_eq!(leftovers_level(&[]), Level::NiceToHave);
        let full = Finding::new("space-C:", Area::Speed, Level::WorthDoing, "Drive C: is almost full");
        assert_eq!(leftovers_level(&[full]), Level::WorthDoing);
        let other = Finding::new("space-D:", Area::Speed, Level::NiceToHave, "Drive D: is almost full");
        assert_eq!(leftovers_level(&[other]), Level::NiceToHave);
    }
}
