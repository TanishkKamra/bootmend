//! "What's running" and the connection check on the Network page.

use super::theme::{self, pal, S1, S2, S3, S4};
use super::widgets::{self, card, icon, kv, muted, section, small, table};
use super::{App, Job, Page, Pending};
use crate::cleanup::size_text;
use crate::live::{verdict, Live, Monitor};
use crate::ndt7::Phase;
use crate::nettest::{self, speed_text, NetCheck};
use crate::snapshot::Snapshot;
use crate::util::{shell_open, J};
use eframe::egui::{self, RichText, Ui};
use std::net::Ipv4Addr;

/// The router's address: the first gateway of an adapter that is up.
fn router(snap: &Snapshot) -> Option<Ipv4Addr> {
    snap.network.a("adapters").iter().filter(|a| a.s("Status").as_deref() == Some("Up")).flat_map(|a| a.strs("Gateway")).find_map(|g| g.parse().ok())
}

impl App {
    /// Samples only while the page is open.
    pub(super) fn keep_monitor(&mut self, ctx: &egui::Context) {
        if self.page == Page::Live {
            if self.live.is_none() {
                let c = ctx.clone();
                self.live = Some(Monitor::start(move || c.request_repaint()));
            }
        } else {
            self.live = None;
        }
    }

    pub(super) fn live_data(&self) -> Option<Live> {
        self.live.as_ref().and_then(|m| m.data.lock().ok().and_then(|g| g.clone()))
    }

    pub(super) fn start_net_check(&mut self, ctx: &egui::Context) {
        self.busy.insert("net".into());
        let r = self.snap.as_deref().and_then(router);
        let progress = std::sync::Arc::new(std::sync::Mutex::new((None, None)));
        self.net_progress = progress.clone();
        let c = ctx.clone();
        self.spawn(ctx, move || {
            Job::NetChecked(nettest::run(r, &|phase, mbps| {
                if let Ok(mut p) = progress.lock() {
                    *p = (phase, mbps);
                }
                c.request_repaint();
            }))
        });
    }

    pub(super) fn live_page(&mut self, ui: &mut Ui) {
        let p = pal(ui.ctx());
        let Some(l) = self.live_data() else {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.label("Watching for two seconds to see what's busy.");
                });
            });
            return;
        };
        card(ui, |ui| {
            ui.label(RichText::new(verdict(&l)).text_style(theme::h2()));
            ui.add_space(S3);
            let mem = if l.mem_total > 0 { l.mem_used as f32 / l.mem_total as f32 } else { 0.0 };
            for (label, value, frac) in [
                ("Processor", format!("{:.0}% in use", l.cpu), l.cpu / 100.0),
                ("Memory", format!("{} of {} in use", size_text(l.mem_used), size_text(l.mem_total)), mem),
            ] {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(label).family(theme::semibold()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(value);
                    });
                });
                let fill = if frac > 0.85 { p.amber } else { p.accent };
                widgets::bar(ui, label, frac, fill);
                ui.add_space(S2);
            }
        });

        section(ui, "Apps using the most");
        card(ui, |ui| {
            ui.horizontal(|ui| {
                small(ui, "Sort by");
                for (i, label) in ["Processor", "Memory", "Drive"].iter().enumerate() {
                    ui.selectable_value(&mut self.ui.live_sort, i, *label);
                }
            });
            ui.add_space(S2);
            let mut apps = l.apps.clone();
            match self.ui.live_sort {
                1 => apps.sort_by_key(|a| std::cmp::Reverse(a.mem)),
                2 => apps.sort_by(|a, b| b.disk.total_cmp(&a.disk)),
                _ => {}
            }
            let rows: Vec<Vec<String>> = apps
                .iter()
                .take(12)
                .map(|a| {
                    let name = if a.processes > 1 { format!("{} ({} parts)", a.name, a.processes) } else { a.name.clone() };
                    let disk = if a.disk < 1024.0 { "None".to_string() } else { format!("{}/s", size_text(a.disk as u64)) };
                    let cpu = if a.cpu >= 10.0 || a.cpu < 0.05 { format!("{:.0}%", a.cpu) } else { format!("{:.1}%", a.cpu) };
                    vec![name, cpu, size_text(a.mem), disk]
                })
                .collect();
            table(ui, "live", &["App", "Processor", "Memory", "Drive"], &rows);
            ui.add_space(S3);
            muted(ui, "Updates every two seconds while this page is open. To close an app that has stopped responding, use Task Manager.");
            ui.add_space(S2);
            if widgets::icon_button(ui, icon::OPEN, "Open Task Manager", true).clicked() {
                log!("action", "opening Task Manager");
                if let Err(e) = shell_open("taskmgr.exe") {
                    self.toast(format!("{e:#}"), false);
                }
            }
        });
    }

    pub(super) fn net_check(&mut self, ui: &mut Ui) {
        let busy = self.busy.contains("net");
        let mut start = false;
        card(ui, |ui| {
            if busy {
                let (phase, mbps) = self.net_progress.lock().map(|p| *p).unwrap_or_default();
                let so_far = mbps.map(|m| format!(": {}", speed_text(m))).unwrap_or_default();
                let words = match phase {
                    None => "Checking the router and name lookups".to_string(),
                    Some(Phase::Download) => format!("Measuring download speed{so_far}"),
                    Some(Phase::Upload) => format!("Measuring upload speed{so_far}"),
                };
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.label(words);
                });
                return;
            }
            match &self.net {
                Some(c) => net_result(ui, c),
                None => {
                    muted(ui, "Measure how fast this connection is, how quickly it answers, and whether any delay is in your home network or further out.");
                }
            }
            ui.add_space(S3);
            // The note sits beside the button, or under it in a narrow card.
            let muted = pal(ui.ctx()).muted;
            let again = self.net.is_some();
            let row = |ui: &mut Ui| {
                start = widgets::icon_button(ui, icon::NETWORK, if again { "Test again" } else { "Test my connection" }, true).clicked();
                ui.add(egui::Label::new(RichText::new("About 25 seconds. Uses M-Lab, which publishes the results.").small().color(muted)).wrap());
            };
            if ui.available_width() < widgets::WRAP {
                ui.vertical(row);
            } else {
                ui.horizontal(row);
            }
        });
        if start {
            if self.settings.speed_test_ok {
                let ctx = ui.ctx().clone();
                self.run(&ctx, Pending::NetCheck);
            } else {
                self.ask(Pending::NetCheck);
            }
        }
        ui.add_space(S4);
    }
}

/// One half of the speed headline: a small label over a big number, with its unit on the same
/// baseline.
fn speed_figure(ui: &mut Ui, label: &str, mbps: Option<f64>, style: egui::TextStyle) {
    let p = pal(ui.ctx());
    ui.vertical(|ui| {
        ui.label(RichText::new(label).color(p.muted));
        let Some(m) = mbps else {
            ui.label(RichText::new("Couldn't measure").color(p.muted));
            return;
        };
        let text = speed_text(m);
        let (number, unit) = text.split_once(' ').unwrap_or((&text, ""));
        let big = ui.painter().layout_no_wrap(number.to_string(), style.resolve(ui.style()), p.ink);
        let small = ui.painter().layout_no_wrap(unit.to_string(), egui::TextStyle::Body.resolve(ui.style()), p.muted);
        let baseline = |g: &egui::Galley| g.rows.first().and_then(|r| r.row.glyphs.first()).map_or(0.0, |c| c.pos.y);
        let drop = baseline(&big) - baseline(&small);
        let (rect, r) = ui.allocate_exact_size(egui::vec2(big.size().x + S1 + small.size().x, big.size().y), egui::Sense::hover());
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("{label}: {text}")));
        let unit_at = rect.min + egui::vec2(big.size().x + S1, drop);
        ui.painter().galley(rect.min, big, p.ink);
        ui.painter().galley(unit_at, small, p.muted);
    });
}

fn net_result(ui: &mut Ui, c: &NetCheck) {
    let p = pal(ui.ctx());
    if let Some(d) = nettest::diagnosis(c) {
        widgets::note(ui, p.amber, p.amber_bg, icon::WARN, |ui| {
            ui.label(d);
        });
        ui.add_space(S3);
    }
    let speed = c.speed.as_ref();
    if let Some(down) = speed.and_then(|s| s.down_mbps) {
        let up = speed.and_then(|s| s.up_mbps);
        // Two big numbers don't fit side by side in a narrow card at full size.
        let style = if ui.available_width() < widgets::WRAP { egui::TextStyle::Heading } else { theme::big() };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::S6;
            speed_figure(ui, "Download", Some(down), style.clone());
            speed_figure(ui, "Upload", up, style);
        });
        ui.add_space(S2);
        ui.add(egui::Label::new(nettest::good_for(down, up)).wrap());
        ui.add_space(S4);
    }
    let wifi = c.wifi.as_ref();
    let (lost, sent) = c.packets();
    kv(
        ui,
        "net",
        &[
            ("Response time", c.internet_ms.map(|ms| format!("{ms:.0} ms. {}", nettest::latency_words(ms)))),
            ("Lag when busy", speed.and_then(|s| s.lag_ms()).map(|x| format!("{x:.0} ms more. {}.", nettest::lag_words(x)))),
            ("Steadiness", nettest::steadiness(c.jitter_ms, lost, sent)),
            (
                "Wi-Fi",
                wifi.map(|w| {
                    let signal = w.signal.map(|s| format!("{} signal ({s}%)", nettest::signal_words(s)));
                    [signal, w.band.clone()].into_iter().flatten().collect::<Vec<_>>().join(", ")
                }),
            ),
        ],
    );
    ui.add_space(S2);
    ui.collapsing("More details", |ui| {
        kv(
            ui,
            "net-more",
            &[
                ("Jitter", c.jitter_ms.map(|j| if j < 10.0 { format!("{j:.1} ms") } else { format!("{j:.0} ms") })),
                (
                    "Router",
                    c.router_ms.map(|ms| {
                        let lost = if c.router_lost == 0 { "no pings lost".to_string() } else { format!("{} of {} pings lost", c.router_lost, c.router_sent) };
                        format!("{ms:.0} ms, {lost}")
                    }),
                ),
                ("Name lookups (DNS)", c.dns_ms.map(|ms| format!("{ms:.0} ms"))),
                (
                    "Wi-Fi link speed",
                    wifi.and_then(|w| {
                        w.link_mbps.as_ref().map(|l| match &w.channel {
                            Some(ch) => format!("{l} Mbps on channel {ch}"),
                            None => format!("{l} Mbps"),
                        })
                    }),
                ),
                ("Test server", speed.map(|s| s.place.as_ref().map_or("M-Lab".to_string(), |pl| format!("M-Lab, {pl}")))),
            ],
        );
    });
    for prob in &c.problems {
        ui.add_space(S1);
        ui.add(egui::Label::new(RichText::new(prob).color(p.muted)).wrap());
    }
}
