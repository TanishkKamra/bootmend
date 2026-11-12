//! "What's running" and the connection check on the Network page.

use super::theme::{self, pal, S1, S2, S3, S4};
use super::widgets::{self, card, icon, kv, muted, section, small, table};
use super::{App, Job, Page, Pending};
use crate::cleanup::size_text;
use crate::live::{verdict, Live, Monitor};
use crate::nettest::{self, NetCheck};
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
        self.spawn(ctx, move || Job::NetChecked(nettest::run(r)));
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
                widgets::bar(ui, frac.clamp(0.0, 1.0), fill);
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
                if let Err(e) = shell_open("taskmgr.exe") {
                    self.toast(format!("{e:#}"), false);
                }
            }
        });
    }

    pub(super) fn net_check(&mut self, ui: &mut Ui) {
        section(ui, "Connection check");
        let busy = self.busy.contains("net");
        let mut start = false;
        card(ui, |ui| {
            if busy {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.label("Testing. This takes under a minute.");
                });
                return;
            }
            match &self.net {
                Some(c) => net_result(ui, c),
                None => {
                    muted(ui, "Measure how fast this connection is, and whether any delay is in your home network or further out.");
                }
            }
            ui.add_space(S3);
            ui.horizontal(|ui| {
                start = widgets::icon_button(ui, icon::NETWORK, if self.net.is_some() { "Test again" } else { "Test my connection" }, true).clicked();
                small(ui, "Uses Cloudflare's public speed test. Asks before going online.");
            });
        });
        if start {
            self.ask(Pending::NetCheck);
        }
        ui.add_space(S4);
    }
}

fn net_result(ui: &mut Ui, c: &NetCheck) {
    let p = pal(ui.ctx());
    if let Some(d) = nettest::diagnosis(c) {
        widgets::note(ui, p.amber, p.amber_bg, icon::WARN, |ui| {
            ui.label(d);
        });
        ui.add_space(S3);
    }
    kv(
        ui,
        "net",
        &[
            ("Download", c.down_mbps.map(|m| format!("{m:.0} Mbps. {}", nettest::speed_words(m)))),
            ("Upload", c.up_mbps.map(|m| format!("{m:.0} Mbps"))),
            ("Response time", c.internet_ms.map(|ms| format!("{ms:.0} ms. {}", nettest::latency_words(ms)))),
            (
                "Router",
                c.router_ms.map(|ms| {
                    let lost = if c.router_lost == 0 { "no pings lost".to_string() } else { format!("{} of {} pings lost", c.router_lost, c.router_sent) };
                    format!("{ms:.0} ms, {lost}")
                }),
            ),
            ("Name lookups (DNS)", c.dns_ms.map(|ms| format!("{ms:.0} ms"))),
        ],
    );
    for prob in &c.problems {
        ui.add_space(S1);
        ui.add(egui::Label::new(RichText::new(prob).color(p.muted)).wrap());
    }
}
