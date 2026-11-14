//! Home: where the PC stands in one sentence, and the plan to work through.

use super::theme::{self, pal, RADIUS, S1, S2, S3, S4, S5, S6};
use super::widgets::{self, icon, small};
use super::{keystone, App, Page, Pending, COMPACT};
use crate::cleanup::size_text;
use crate::fixes::Fix;
use crate::rules::{plural, Action, Area, Finding, Level};
use crate::snapshot::Snapshot;
use eframe::egui::{self, CornerRadius, Margin, RichText, Sense, Stroke, Ui};
use std::sync::Arc;
use std::time::Instant;

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
/// With animation effects off, the keystone fades into its seat over this long instead of moving.
const FADE_MS: f32 = 200.0;

/// The leftover files Bootmend clears by itself, as a line on the plan.
struct Leftovers {
    todo: Finding,
    done: bool,
    /// What's still to clear, and its size.
    ids: Vec<&'static str>,
    left: u64,
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
        Some(Leftovers {
            todo,
            done: left_items.is_empty(),
            ids: left_items.iter().map(|i| i.id).collect(),
            left: left_items.iter().filter_map(|i| i.bytes).sum(),
        })
    }

    pub(super) fn home(&mut self, ui: &mut Ui, snap: &Arc<Snapshot>) {
        let p = pal(ui.ctx());
        // Measuring leftovers only reads folder sizes; clearing waits for a press.
        if self.cleanup.is_none() && !self.busy.contains("cleanup-look") {
            self.start_cleanup_look(&ui.ctx().clone());
        }
        let mut todos: Vec<(Finding, bool)> = self.findings.iter().filter(|f| f.level != Level::AllGood).map(|f| (f.clone(), self.todo_done(f))).collect();
        let leftovers = self.leftovers();
        if let Some(l) = &leftovers {
            todos.push((l.todo.clone(), l.done));
        }
        // Stable, so the leftovers line comes last within its level.
        todos.sort_by_key(|(f, _)| f.level);
        let prog = progress(todos.iter().map(|(f, d)| (f.level, *d)));
        let safe = leftovers.as_ref().filter(|l| !l.done).map(|l| (l.ids.clone(), l.left));

        self.hero(ui, snap, prog, safe.as_ref());
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
        ui.add_space(S6);
        self.plan_list(ui, &todos, safe.as_ref().map_or(&[][..], |s| &s.0[..]));
        self.schedule_offer(ui);
    }

    /// The verdict as the page title beside the Keystone, the progress line and the one button
    /// that does the safe work.
    fn hero(&mut self, ui: &mut Ui, snap: &Snapshot, prog: Progress, safe: Option<&(Vec<&'static str>, u64)>) {
        let p = pal(ui.ctx());
        let (title, lede) = verdict(prog);
        let avail = ui.available_width();
        let wide = avail >= 640.0;
        // Short of room, the Keystone and verdict stay side by side and the rest moves under
        // both, the way Settings moves the Windows Update tile under the PC's name.
        let compact = avail < COMPACT;
        let size = if wide { 112.0 } else { 72.0 };
        let keys = self.keystone_keys(ui.ctx(), prog.raised());
        let cleaning = self.busy.contains("cleanup");
        let mut clear = false;
        let rest = |ui: &mut Ui, clear: &mut bool| {
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
            if let Some((_, left)) = safe {
                ui.add_space(S2);
                ui.horizontal_wrapped(|ui| {
                    if cleaning {
                        ui.add(egui::Spinner::new().color(p.accent));
                        widgets::muted(ui, "Clearing leftover files");
                    } else {
                        *clear = widgets::primary(ui, "Fix the safe one for me").clicked();
                        // Short of room, the note goes under the button rather than wrapping around it.
                        if compact {
                            ui.end_row();
                        } else {
                            ui.add_space(S3);
                        }
                        small(ui, format!("Clears {} of leftover files. Nothing else changes.", size_text(*left)));
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
                    rest(ui, &mut clear);
                }
            });
            slot
        });
        let centre = egui::pos2(row.inner.center().x, row.response.rect.center().y);
        keystone::paint(ui, egui::Rect::from_center_size(centre, egui::vec2(size, size)), &keys);
        if compact {
            ui.add_space(S3);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = S2;
                rest(ui, &mut clear);
            });
        }
        if clear {
            if let Some((ids, _)) = safe {
                self.start_cleaning(&ui.ctx().clone(), ids.clone());
            }
        }
    }

    /// Where to draw the keystone this frame. It drops into its seat the moment the last thing
    /// that matters is fixed, and rises at once if something new turns up.
    fn keystone_keys(&mut self, ctx: &egui::Context, raised: bool) -> Vec<(f32, f32)> {
        if self.ui.keystone_raised == Some(true) && !raised {
            self.ui.keystone_drop = Some(Instant::now());
        }
        self.ui.keystone_raised = Some(raised);
        if raised {
            self.ui.keystone_drop = None;
            return vec![(keystone::RAISED, 1.0)];
        }
        let Some(start) = self.ui.keystone_drop else { return vec![(0.0, 1.0)] };
        let ms = start.elapsed().as_secs_f32() * 1000.0;
        let (t, keys) = if theme::motion(ctx) {
            let t = ms / keystone::DROP_MS;
            (t, vec![(keystone::lift_at(t), 1.0)])
        } else {
            let t = (ms / FADE_MS).min(1.0);
            (t, vec![(keystone::RAISED, 1.0 - t), (0.0, t)])
        };
        if t < 1.0 {
            ctx.request_repaint();
        } else {
            self.ui.keystone_drop = None;
        }
        keys
    }

    /// One card holding the whole plan, grouped by level, then what was checked and found fine.
    fn plan_list(&mut self, ui: &mut Ui, todos: &[(Finding, bool)], safe_ids: &[&'static str]) {
        let p = pal(ui.ctx());
        let fine: Vec<Finding> = self.findings.iter().filter(|f| f.level == Level::AllGood && !f.id.ends_with("-clear")).cloned().collect();
        if todos.is_empty() && fine.is_empty() {
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
                                ui.add_sized(egui::vec2(20.0, 20.0), egui::Label::new(widgets::ico(icon::CHECK, 14.0).color(p.green)));
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
            });
    }

    /// One to-do: a ring that fills when it's done, what it is, and one button. "Fix it for me"
    /// when one change does it; "Walk me through it" opens the steps in place.
    fn todo_row(&mut self, ui: &mut Ui, f: &Finding, done: bool, safe_ids: &[&'static str]) {
        let p = pal(ui.ctx());
        let fixes: Vec<&Fix> = f.actions.iter().filter_map(|a| if let Action::Fix(x) = a { Some(x) } else { None }).collect();
        let leftovers = f.id == LEFTOVERS;
        let one_fix = leftovers || fixes.len() == 1;
        let more = !f.steps.is_empty() || !f.details.is_empty() || f.actions.len() > usize::from(fixes.len() == 1);
        let open = self.ui.expanded.contains(&f.id);
        let avail = ui.available_width();
        // Short of room, the button moves under the text. Otherwise its column is measured
        // generously, so a row's text never runs under it.
        let compact = avail < COMPACT;
        // The ring, the gap the layout puts after it, and the space before the words.
        let indent = 20.0 + ui.spacing().item_spacing.x + S4;
        let text_w = if compact { avail - indent } else { (avail - indent - 230.0).max(160.0) };
        let title = egui::WidgetText::from(RichText::new(&f.title).family(theme::semibold())).into_galley(
            ui,
            Some(egui::TextWrapMode::Wrap),
            text_w,
            egui::TextStyle::Body,
        );
        let summary = egui::WidgetText::from(RichText::new(&f.summary).small().color(p.muted)).into_galley(
            ui,
            Some(egui::TextWrapMode::Wrap),
            text_w,
            egui::TextStyle::Small,
        );
        let h = (title.size().y + 2.0 + summary.size().y).max(32.0) + 2.0 * S3;
        let mut toggle = false;
        let mut clear = false;
        let chevron = more && (one_fix || done);
        let text = |ui: &mut Ui| {
            ring(ui, f.level, done);
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
            self.start_cleaning(&ui.ctx().clone(), safe_ids.to_vec());
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
                    ui.label(widgets::ico(icon::CHECK, 14.0).color(p.green));
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
            if self.busy.contains(&format!("fix:{fx:?}")) {
                status(ui, rtl, spinner, |ui| {
                    widgets::muted(ui, "Working on it");
                });
            } else {
                let blocked = fx.needs_admin() && !self.elevated;
                let r = widgets::secondary_enabled(ui, "Fix it for me", !blocked).on_hover_text(fx.label());
                let r = if blocked { r.on_disabled_hover_text("Needs administrator rights. Restart Bootmend as administrator first.") } else { r };
                if r.clicked() {
                    self.ask(Pending::Fix((*fx).clone(), f.title.clone()));
                }
            }
        } else {
            toggle = widgets::secondary(ui, if open { "Hide the steps" } else { "Walk me through it" }).clicked();
        }
        (toggle, clear)
    }
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

/// The ring at the start of a to-do: red for Do first, filled with a tick once done.
fn ring(ui: &mut Ui, level: Level, done: bool) {
    let p = pal(ui.ctx());
    let (r, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::hover());
    let c = r.center();
    if done {
        ui.painter().circle_filled(c, 10.0, p.accent);
        ui.painter().text(c, egui::Align2::CENTER_CENTER, icon::CHECK, egui::FontId::new(11.0, theme::icons()), p.on_accent);
    } else {
        ui.painter().circle_stroke(c, 9.25, Stroke::new(1.5_f32, if level == Level::DoFirst { p.red } else { p.control }));
    }
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
