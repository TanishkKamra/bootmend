//! Regular check-ups (offered once the first check and its fixes are done) and the "Over time" charts.

use super::theme::{self, pal, S1, S2, S3, S4};
use super::widgets::{self, card, muted, section, small};
use super::{App, Job};
use crate::rules::{Action, Level};
use crate::schedule::{self, Every};
use crate::trends::{self, Trend};
use eframe::egui::{self, RichText, Stroke, Ui};

impl App {
    /// The offer waits until nothing is left that rigscan could fix, or the person has already used a fix.
    fn schedule_offer_due(&self) -> bool {
        let fixable = self.findings.iter().any(|f| f.level != Level::AllGood && f.actions.iter().any(|a| matches!(a, Action::Fix(_))));
        !self.settings.schedule_asked && self.settings.schedule == Every::Off && (!fixable || !self.history.is_empty())
    }

    pub(super) fn set_schedule(&mut self, ctx: &egui::Context, every: Every, hour: u8) {
        self.settings.schedule_asked = true;
        self.save_settings();
        self.busy.insert("schedule".into());
        self.spawn(ctx, move || Job::Scheduled(every, hour, schedule::set(every, hour).map_err(|e| format!("{e:#}"))));
    }

    pub(super) fn scheduled(&mut self, every: Every, hour: u8, r: Result<(), String>) {
        self.busy.remove("schedule");
        match r {
            Ok(()) => {
                self.settings.schedule = every;
                self.settings.schedule_hour = hour;
                self.save_settings();
                if every == Every::Off {
                    self.toast("Regular check-ups are off.", true);
                } else {
                    self.toast(
                        format!(
                            "Check-ups are on: {} at {}. rigscan lets you know only when something new needs a look.",
                            every.label().to_lowercase(),
                            schedule::hour_label(hour)
                        ),
                        true,
                    );
                }
            }
            Err(e) => self.toast(format!("Couldn't change check-ups: {e}"), false),
        }
    }

    pub(super) fn schedule_offer(&mut self, ui: &mut Ui) {
        if !self.schedule_offer_due() {
            return;
        }
        let mut pick = None;
        ui.add_space(S4);
        card(ui, |ui| {
            ui.label(RichText::new("Want rigscan to keep an eye on this PC?").text_style(theme::h2()));
            ui.add_space(S1);
            ui.add(egui::Label::new("It can run this same check quietly in the background and show a notification only when something new needs a look. It never fixes anything on its own.").wrap());
            ui.add_space(S3);
            ui.horizontal_wrapped(|ui| {
                if widgets::primary(ui, "Every week").clicked() {
                    pick = Some(Every::Weekly);
                }
                if widgets::secondary(ui, "Every day").clicked() {
                    pick = Some(Every::Daily);
                }
                if widgets::secondary(ui, "Every month").clicked() {
                    pick = Some(Every::Monthly);
                }
                if widgets::secondary(ui, "No thanks").clicked() {
                    pick = Some(Every::Off);
                }
            });
            ui.add_space(S1);
            small(
                ui,
                format!(
                    "Check-ups start at {}. You can change the time and pace later on the History & undo page.",
                    schedule::hour_label(self.settings.schedule_hour)
                ),
            );
        });
        match pick {
            Some(Every::Off) => {
                self.settings.schedule_asked = true;
                self.save_settings();
            }
            Some(e) => self.set_schedule(&ui.ctx().clone(), e, self.settings.schedule_hour),
            None => {}
        }
    }

    /// The setting on the History page; shown once the offer has been made.
    pub(super) fn schedule_setting(&mut self, ui: &mut Ui) {
        if !self.settings.schedule_asked && self.settings.schedule == Every::Off {
            return;
        }
        section(ui, "Regular check-ups");
        let busy = self.busy.contains("schedule");
        let mut pick = None;
        let mut hour = self.settings.schedule_hour;
        card(ui, |ui| {
            muted(
                ui,
                format!(
                    "rigscan checks quietly at {}, or as soon as the PC is on after that, and shows a notification only when something new needs a look.",
                    schedule::hour_label(hour)
                ),
            );
            ui.add_space(S2);
            ui.horizontal(|ui| {
                for e in [Every::Off, Every::Daily, Every::Weekly, Every::Monthly] {
                    let r = ui.add_enabled(!busy, egui::Button::new(e.label()).selected(self.settings.schedule == e));
                    if r.clicked() && self.settings.schedule != e {
                        pick = Some(e);
                    }
                }
                if busy {
                    ui.add(egui::Spinner::new());
                }
            });
            if self.settings.schedule != Every::Off {
                ui.add_space(S2);
                ui.horizontal(|ui| {
                    ui.label("Time");
                    ui.add_enabled_ui(!busy, |ui| {
                        egui::ComboBox::from_id_salt("checkup-hour").selected_text(schedule::hour_label(hour)).show_ui(ui, |ui| {
                            for h in 0..24 {
                                ui.selectable_value(&mut hour, h, schedule::hour_label(h));
                            }
                        });
                    });
                });
                ui.add_space(S2);
                small(
                    ui,
                    format!(
                        "Up to {} saved checks are kept; older ones are removed as new ones arrive. If you move rigscan to another folder, set this again.",
                        schedule::KEEP
                    ),
                );
            }
        });
        if let Some(e) = pick {
            self.set_schedule(&ui.ctx().clone(), e, hour);
        } else if hour != self.settings.schedule_hour {
            self.set_schedule(&ui.ctx().clone(), self.settings.schedule, hour);
        }
    }

    pub(super) fn trends_ui(&mut self, ui: &mut Ui) {
        let Some(id) = self.snap.as_ref().map(|s| s.id.clone()) else { return };
        if self.trends_for != id && !self.busy.contains("trends") {
            self.trends_for = id;
            self.busy.insert("trends".into());
            let host = self.snap.as_ref().map(|s| s.host.clone()).unwrap_or_default();
            self.spawn(&ui.ctx().clone(), move || Job::Trends(trends::load(&host)));
        }
        if self.trends.is_empty() {
            return;
        }
        section(ui, "Over time");
        muted(ui, format!("From the last {} saved checks.", self.trends.iter().map(|t| t.points.len()).max().unwrap_or(0)));
        ui.add_space(S2);
        let cols = if ui.available_width() > 820.0 { 2 } else { 1 };
        let trends = self.trends.clone();
        for row in trends.chunks(cols) {
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.x = S3;
                ui.columns(cols, |uis| {
                    for (ui, t) in uis.iter_mut().zip(row) {
                        card(ui, |ui| trend_card(ui, t));
                    }
                });
            });
            ui.add_space(S3);
        }
    }
}

fn trend_card(ui: &mut Ui, t: &Trend) {
    let p = pal(ui.ctx());
    ui.label(RichText::new(t.title).color(p.muted));
    if let Some((_, v)) = t.points.last() {
        ui.label(RichText::new(format!("{v:.0}{}", t.unit)).text_style(theme::h2()));
    }
    let (words, good) = trends::change_words(t);
    let color = match good {
        Some(true) => p.green,
        Some(false) => p.amber,
        None => p.muted,
    };
    ui.label(RichText::new(words).color(color));
    ui.add_space(S2);
    sparkline(ui, &t.points.iter().map(|x| x.1).collect::<Vec<_>>(), p.accent);
}

/// A small line chart without axes; the words above carry the numbers.
fn sparkline(ui: &mut Ui, v: &[f64], color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 40.0), egui::Sense::hover());
    if v.len() < 2 {
        return;
    }
    let (lo, hi) = v.iter().fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
    let span = if hi - lo < 1e-9 { 1.0 } else { hi - lo };
    let pts: Vec<egui::Pos2> = v
        .iter()
        .enumerate()
        .map(|(i, x)| {
            let fx = i as f32 / (v.len() - 1) as f32;
            let fy = if hi - lo < 1e-9 { 0.5 } else { ((x - lo) / span) as f32 };
            egui::pos2(rect.left() + fx * rect.width(), rect.bottom() - 4.0 - fy * (rect.height() - 8.0))
        })
        .collect();
    let last = *pts.last().expect("two points");
    ui.painter().add(egui::Shape::line(pts, Stroke::new(2.0_f32, color)));
    ui.painter().circle_filled(last, 3.5, color);
}
