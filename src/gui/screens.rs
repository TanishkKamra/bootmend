//! Full-window screens: the first-run welcome and the progress view while a check runs.

use super::theme::{pal, S2, S3, S4, S5, S6};
use super::widgets::{self, ico, icon};
use super::{App, Pending};
use eframe::egui::{self, RichText};

/// What Bootmend reads, in plain words. Shown on the welcome and privacy screens.
pub const READS: [&str; 9] = [
    "Hardware details: processor, memory, graphics, drives, battery and connected devices",
    "Windows version, update history and activation status",
    "Drive health counters and free space",
    "Windows Security and firewall settings, and its list of past detections",
    "Installed apps, browser extensions, apps that start with Windows, background services and scheduled tasks",
    "Network adapters and which apps accept connections from the network",
    "Windows' own logs of crashes, unexpected shutdowns and slow starts",
    "Sizes of a few folders that tend to fill up, such as Downloads and temporary files",
    "It never opens your documents, photos, emails or browsing history, and never reads passwords.",
];

impl App {
    pub(super) fn welcome_screen(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui.ctx());
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.set_max_width(ui.available_width().min(600.0));
                ui.add_space(S5);
                ui.add(widgets::Glyph(ico(icon::PC, 36.0).color(p.accent)));
                ui.add_space(S3);
                widgets::page_title(ui, "A plain-English check-up for your PC");
                ui.add_space(S2);
                ui.label(RichText::new("Bootmend looks at your hardware, security settings and recent problems, then explains what's worth fixing and how.").color(p.muted));
                ui.add_space(S5);
            });
            ui.vertical_centered(|ui| {
                ui.set_max_width(ui.available_width().min(600.0));
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    widgets::card(ui, |ui| {
                        for (ic, title, body) in [
                            (icon::LOCK, "Stays on this PC", "Everything Bootmend finds is kept on this computer. There's no account, no tracking and no ads."),
                            (icon::CHECK, "Changes nothing on its own", "Bootmend only looks. Every fix waits for you to press a button, tells you what it will do, and can be undone."),
                            (icon::NETWORK, "Goes online only when you ask", "Two optional features use the internet: checking for app updates and reading crash files. Each one asks first."),
                        ] {
                            ui.horizontal_top(|ui| {
                                ui.add(widgets::Glyph(ico(ic, 18.0).color(p.accent)));
                                ui.add_space(S2);
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(title).family(super::theme::semibold()));
                                    ui.add(egui::Label::new(RichText::new(body).color(p.muted)).wrap());
                                });
                            });
                            ui.add_space(S3);
                        }
                    });
                    ui.add_space(S5);
                    if let Some(e) = self.check_error.clone() {
                        widgets::note(ui, p.red, p.red_bg, icon::WARN, |ui| {
                            ui.label("The check didn't finish.");
                            ui.add(egui::Label::new(RichText::new(e).small()).wrap());
                        });
                        ui.add_space(S4);
                    }
                    // The note sits beside the button, or under it where there isn't room.
                    let label = if self.check_error.is_some() { "Try again" } else { "Start the check" };
                    let mut start = false;
                    let row = |ui: &mut egui::Ui| {
                        start = widgets::primary(ui, label).clicked();
                        ui.add(egui::Label::new(RichText::new("Takes about a minute. You can keep using your PC.").color(p.muted)).wrap());
                    };
                    if ui.available_width() < widgets::WRAP {
                        ui.vertical(row);
                    } else {
                        ui.horizontal(row);
                    }
                    if start {
                        self.start_check();
                    }
                    ui.add_space(S4);
                    let t = if self.ui.show_reads { "Hide what Bootmend reads" } else { "What exactly does Bootmend read?" };
                    if ui.link(t).clicked() {
                        self.ui.show_reads = !self.ui.show_reads;
                    }
                    if self.ui.show_reads {
                        ui.add_space(S2);
                        for r in READS {
                            ui.horizontal_top(|ui| {
                                ui.label(RichText::new("•").color(p.muted));
                                ui.add(egui::Label::new(r).wrap());
                            });
                        }
                    }
                    if !self.elevated {
                        ui.add_space(S5);
                        widgets::note(ui, p.blue, p.blue_bg, icon::ADMIN, |ui| {
                            ui.label("Running as administrator lets Bootmend read a few more things, such as Windows Security's skip list and crash files. It's optional.");
                            if widgets::secondary(ui, "Restart as administrator").clicked() {
                                self.ask(Pending::Elevate);
                            }
                        });
                    }
                    ui.add_space(S6);
                });
            });
        });
    }
}
