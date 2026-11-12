//! Every page in the sidebar.

use super::theme::{self, pal, S1, S2, S3, S4, S5};
use super::widgets::{self, card, ico, icon, kv, level_pill, muted, section, small, table};
use super::{App, Page, Pending};
use crate::diff::Kind as ChangeKind;
use crate::fixes::Fix;
use crate::rules::{plural, score, Action, Area, Finding, Level};
use crate::scan;
use crate::snapshot::Snapshot;
use crate::util::{shell_open, J};
use eframe::egui::{self, RichText, Ui};
use serde_json::Value;
use std::sync::Arc;

fn s(v: &Value, k: &str) -> String {
    v.s(k).unwrap_or_default()
}

fn num(v: Option<f64>, unit: &str) -> Option<String> {
    v.map(|x| if (x - x.round()).abs() < 0.05 { format!("{x:.0}{unit}") } else { format!("{x:.1}{unit}") })
}

fn yes_no(v: Option<bool>) -> Option<String> {
    v.map(|b| if b { "Yes" } else { "No" }.to_string())
}

fn on_off(v: Option<bool>) -> Option<String> {
    v.map(|b| if b { "On" } else { "Off" }.to_string())
}

fn hours(h: f64) -> String {
    if h >= 48.0 {
        format!("{:.0} hours (about {:.0} days)", h, h / 24.0)
    } else {
        format!("{h:.0} hours")
    }
}

fn fix_target(f: &Fix) -> String {
    match f {
        Fix::RemoveExclusion { value, .. } => value.clone(),
        Fix::DisableFirewallRule { display, .. } => display.clone(),
        Fix::DisableStartup { name, .. } | Fix::EnableStartup { name, .. } => name.clone(),
        Fix::ServiceManual { display, .. } => display.clone(),
        _ => String::new(),
    }
}

impl App {
    pub(super) fn page_ui(&mut self, ui: &mut Ui) {
        let Some(snap) = self.snap.clone() else { return };
        let page = self.page;
        let subtitle = match page {
            Page::Overview => format!("{} · {}", model(&snap), if snap.elevated { "checked as administrator" } else { "checked without administrator rights" }),
            Page::Suggestions => "Sorted by what matters most. Nothing changes unless you press a button.".into(),
            Page::ThisPc => "What's inside, as Windows reports it.".into(),
            Page::Storage => "Drive health, wear and free space.".into(),
            Page::Network => "Network adapters and what accepts connections.".into(),
            Page::Security => "Virus protection, firewall and access settings.".into(),
            Page::Software => "Installed apps and browser extensions.".into(),
            Page::Startup => "What runs when Windows starts, and in the background.".into(),
            Page::Crashes => "Blue screens, sudden shutdowns and hardware errors from the last 90 days.".into(),
            Page::Cleanup => "See what's taking up space before anything is removed.".into(),
            Page::Changes => match &self.prev_taken {
                Some(t) => format!("Compared with the check on {t}."),
                None => "What's different since the last check.".into(),
            },
            Page::Scan => "Run Windows Security's own scanner from here.".into(),
            Page::History => "Every change rigscan made, and how to undo it.".into(),
            Page::Report => "A copy of this check you can keep or send to someone helping you.".into(),
            Page::Privacy => "What rigscan reads, keeps and sends.".into(),
            Page::About => String::new(),
        };
        let title = if page == Page::Overview { "Your PC at a glance" } else { page.title() };
        self.header(ui, title, &subtitle);
        match page {
            Page::Overview => self.overview(ui, &snap),
            Page::Suggestions => self.suggestions(ui),
            Page::ThisPc => this_pc(ui, &snap),
            Page::Storage => {
                if widgets::icon_button(ui, icon::CLEAN, "Free up space", true).clicked() {
                    self.go(Page::Cleanup);
                }
                ui.add_space(S2);
                storage(ui, &snap)
            }
            Page::Cleanup => self.cleanup_page(ui),
            Page::Network => network(ui, &snap),
            Page::Security => self.security(ui, &snap),
            Page::Software => self.software(ui, &snap),
            Page::Startup => self.startup(ui, &snap),
            Page::Crashes => self.crashes(ui, &snap),
            Page::Changes => self.changes_page(ui),
            Page::Scan => self.scan_page(ui, &snap),
            Page::History => self.history_page(ui),
            Page::Report => self.report_page(ui, &snap),
            Page::Privacy => self.privacy(ui),
            Page::About => about(ui),
        }
    }

    // ---------- Overview ----------

    fn overview(&mut self, ui: &mut Ui, snap: &Arc<Snapshot>) {
        let p = pal(ui.ctx());
        if !snap.errors.is_empty() {
            widgets::note(ui, p.amber, p.amber_bg, icon::WARN, |ui| {
                ui.label(format!("{} couldn't finish, so a few details may be missing.", plural(snap.errors.len(), "part of the check", "parts of the check")));
                ui.collapsing("Show what happened", |ui| {
                    for e in &snap.errors {
                        ui.add(egui::Label::new(RichText::new(e).small()).wrap());
                    }
                });
            });
            ui.add_space(S4);
        }
        let findings = self.findings.clone();
        let cols = if ui.available_width() > 820.0 { 4 } else { 2 };
        let mut goto_area = None;
        for row in Area::ALL.chunks(cols) {
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.x = S3;
                ui.columns(cols, |uis| {
                    for (ui, a) in uis.iter_mut().zip(row) {
                        let sc = score(&findings, *a);
                        let (fg, ic) = match sc.value {
                            85..=100 => (p.green, icon::CHECK),
                            60..=84 => (p.amber, icon::WARN),
                            _ => (p.red, icon::WARN),
                        };
                        card(ui, |ui| {
                            ui.set_min_height(150.0);
                            ui.label(RichText::new(a.name()).color(p.muted));
                            ui.horizontal(|ui| {
                                ui.label(ico(ic, 16.0).color(fg));
                                ui.label(RichText::new(sc.verdict).text_style(theme::h2()).color(fg));
                            });
                            small(ui, a.blurb());
                            ui.add_space(S1);
                            if sc.open > 0 {
                                if ui.link(plural(sc.open, "suggestion", "suggestions")).clicked() {
                                    goto_area = Some(*a);
                                }
                            } else {
                                muted(ui, "Nothing to do");
                            }
                        });
                    }
                });
            });
            ui.add_space(S3);
        }
        if let Some(a) = goto_area {
            self.ui.area = Some(a);
            self.go(Page::Suggestions);
        }

        section(ui, "Start here");
        let top: Vec<Finding> = findings.iter().filter(|f| f.level != Level::AllGood).take(3).cloned().collect();
        if top.is_empty() {
            widgets::empty(
                ui,
                icon::CHECK,
                "Nothing needs your attention",
                "Every check came back fine. Check again now and then, or after installing something new.",
            );
        }
        for f in &top {
            let mut open = false;
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    level_pill(ui, f.level);
                    small(ui, f.area.name());
                });
                ui.add_space(S1);
                ui.label(RichText::new(&f.title).size(16.0).family(theme::semibold()));
                ui.add(egui::Label::new(&f.summary).wrap());
                ui.add_space(S1);
                open = widgets::secondary(ui, "Show me how").clicked();
            });
            ui.add_space(S2);
            if open {
                self.ui.area = None;
                self.ui.expanded.insert(f.id.clone());
                self.go(Page::Suggestions);
            }
        }
        let more = findings.iter().filter(|f| f.level != Level::AllGood).count();
        if more > 3 && ui.link(format!("See all {more} suggestions")).clicked() {
            self.ui.area = None;
            self.go(Page::Suggestions);
        }

        section(ui, "This PC");
        card(ui, |ui| {
            let sys = &snap.system;
            let cpu = sys.g("cpu");
            let gpu = sys.a("gpus").into_iter().filter_map(|g| g.s("Name")).filter(|n| !n.contains("Basic")).collect::<Vec<_>>().join(", ");
            kv(
                ui,
                "ov-pc",
                &[
                    ("Processor", cpu.s("Name")),
                    ("Memory", num(sys.g("memory").f("TotalGB"), " GB")),
                    ("Graphics", Some(gpu)),
                    ("Windows", sys.g("os").s("Edition").map(|e| format!("{e} {}", s(sys.g("os"), "Version")))),
                    ("Storage", Some(snap.drives.iter().map(|d| d.model.clone()).collect::<Vec<_>>().join(", "))),
                ],
            );
            ui.add_space(S2);
            if ui.link("See all hardware details").clicked() {
                self.go(Page::ThisPc);
            }
        });

        section(ui, "Since your last check");
        card(ui, |ui| match &self.prev_taken {
            None => {
                muted(ui, "This is the first check on this PC. Next time, rigscan will show what was installed, removed or changed in between.");
            }
            Some(t) => {
                let notable = self.changes.iter().filter(|c| c.notable).count();
                ui.label(format!(
                    "{} since {t}{}.",
                    plural(self.changes.len(), "change", "changes"),
                    if notable > 0 { format!(", {notable} worth a look") } else { String::new() }
                ));
                if ui.link("See what changed").clicked() {
                    self.go(Page::Changes);
                }
            }
        });
    }

    // ---------- Suggestions ----------

    fn suggestions(&mut self, ui: &mut Ui) {
        let findings = self.findings.clone();
        ui.horizontal_wrapped(|ui| {
            let all = findings.iter().filter(|f| f.level != Level::AllGood).count();
            if ui.selectable_label(self.ui.area.is_none(), format!("All ({all})")).clicked() {
                self.ui.area = None;
            }
            for a in Area::ALL {
                let n = findings.iter().filter(|f| f.area == a && f.level != Level::AllGood).count();
                if ui.selectable_label(self.ui.area == Some(a), format!("{} ({n})", a.name())).clicked() {
                    self.ui.area = Some(a);
                }
            }
        });
        ui.add_space(S2);
        for level in [Level::DoFirst, Level::WorthDoing, Level::NiceToHave, Level::AllGood] {
            let items: Vec<&Finding> = findings.iter().filter(|f| f.level == level && self.ui.area.is_none_or(|a| a == f.area)).collect();
            if items.is_empty() {
                continue;
            }
            section(ui, &format!("{} ({})", level.name(), items.len()));
            for f in items {
                self.finding_card(ui, f);
                ui.add_space(S2);
            }
        }
    }

    fn finding_card(&mut self, ui: &mut Ui, f: &Finding) {
        let p = pal(ui.ctx());
        let open = self.ui.expanded.contains(&f.id) || f.level == Level::DoFirst;
        let has_more = !f.steps.is_empty() || !f.details.is_empty() || !f.actions.is_empty();
        let mut toggle = false;
        card(ui, |ui| {
            ui.horizontal(|ui| {
                level_pill(ui, f.level);
                small(ui, f.area.name());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    small(ui, f.confidence.name()).on_hover_text(match f.confidence {
                        crate::rules::Confidence::Measured => "rigscan read this directly from Windows or the hardware.",
                        crate::rules::Confidence::BestGuess => "rigscan is inferring this from indirect signs, so it may be wrong.",
                    });
                });
            });
            ui.add_space(S1);
            ui.label(RichText::new(&f.title).size(16.5).family(theme::semibold()));
            ui.add(egui::Label::new(&f.summary).wrap());
            if open && has_more {
                if !f.steps.is_empty() {
                    ui.add_space(S2);
                    ui.label(RichText::new("What to do").family(theme::semibold()));
                    for (i, st) in f.steps.iter().enumerate() {
                        ui.horizontal_top(|ui| {
                            ui.label(RichText::new(format!("{}.", i + 1)).color(p.muted));
                            ui.add(egui::Label::new(st).wrap());
                        });
                    }
                }
                if !f.details.is_empty() {
                    ui.add_space(S2);
                    ui.label(RichText::new("Details").family(theme::semibold()));
                    for d in f.details.iter().take(40) {
                        ui.horizontal_top(|ui| {
                            ui.label(RichText::new("•").color(p.muted));
                            ui.add(egui::Label::new(RichText::new(d).color(p.muted)).wrap());
                        });
                    }
                    if f.details.len() > 40 {
                        small(ui, format!("and {} more", f.details.len() - 40));
                    }
                }
                ui.add_space(S3);
                self.actions(ui, f);
            }
            if has_more && f.level != Level::DoFirst {
                ui.add_space(S1);
                let t = if open {
                    "Show less"
                } else if f.level == Level::AllGood {
                    "Show details"
                } else {
                    "Show how to fix"
                };
                toggle = ui.link(t).clicked();
            }
        });
        if toggle && !self.ui.expanded.remove(&f.id) {
            self.ui.expanded.insert(f.id.clone());
        }
    }

    fn actions(&mut self, ui: &mut Ui, f: &Finding) {
        let fixes: Vec<&Fix> = f.actions.iter().filter_map(|a| if let Action::Fix(x) = a { Some(x) } else { None }).collect();
        if fixes.len() > 1 {
            for fx in &fixes {
                ui.horizontal(|ui| {
                    self.fix_button(ui, fx, &f.title);
                    ui.add(egui::Label::new(fix_target(fx)).truncate());
                });
            }
            ui.add_space(S2);
        }
        ui.horizontal_wrapped(|ui| {
            for a in &f.actions {
                match a {
                    Action::Fix(fx) if fixes.len() == 1 => self.fix_button(ui, fx, &f.title),
                    Action::Fix(_) => {}
                    Action::Open { label, target } => {
                        if widgets::icon_button(ui, icon::OPEN, label, true).clicked() {
                            if let Err(e) = shell_open(target) {
                                self.toast(format!("{e:#}"), false);
                            }
                        }
                    }
                    Action::Goto(pg) => {
                        if widgets::secondary(ui, &format!("Go to {}", pg.title())).clicked() {
                            self.go(*pg);
                        }
                    }
                    Action::AnalyzeDumps => self.dumps_button(ui),
                    Action::Elevate => {
                        if widgets::primary(ui, "Restart as administrator").clicked() {
                            self.ask(Pending::Elevate);
                        }
                    }
                    Action::RestorePoint => self.restore_point_button(ui),
                }
            }
        });
    }

    fn fix_button(&mut self, ui: &mut Ui, fx: &Fix, title: &str) {
        let p = pal(ui.ctx());
        if self.history.iter().any(|h| h.fix == *fx && h.undone.is_none()) {
            ui.label(ico(icon::CHECK, 14.0).color(p.green));
            ui.label(RichText::new("Done").color(p.green));
            return;
        }
        if self.busy.contains(&format!("fix:{fx:?}")) {
            ui.add(egui::Spinner::new().color(p.accent));
            muted(ui, "Working on it");
            return;
        }
        let blocked = fx.needs_admin() && !self.elevated;
        let r = widgets::primary_enabled(ui, fx.label(), !blocked);
        let r = if blocked { r.on_disabled_hover_text("Needs administrator rights. Restart rigscan as administrator first.") } else { r };
        if r.clicked() {
            self.ask(Pending::Fix(fx.clone(), title.to_string()));
        }
    }

    fn restore_point_button(&mut self, ui: &mut Ui) {
        if self.busy.contains("restore") {
            ui.add(egui::Spinner::new());
            muted(ui, "Saving a restore point");
            return;
        }
        let r = widgets::secondary_enabled(ui, "Save a restore point", self.elevated);
        let r = if self.elevated { r } else { r.on_disabled_hover_text("Needs administrator rights. Restart rigscan as administrator first.") };
        if r.clicked() {
            self.ask(Pending::RestorePoint);
        }
    }

    fn dumps_button(&mut self, ui: &mut Ui) {
        let cdb = crate::collect::dumps::cdb_path().is_some();
        let busy = self.busy.contains("dumps");
        if busy {
            ui.add(egui::Spinner::new());
            muted(ui, "Reading crash files. This can take a few minutes.");
            return;
        }
        let ok = cdb && self.elevated;
        let r = widgets::secondary_enabled(ui, "Read crash files", ok);
        let r = if !cdb {
            r.on_disabled_hover_text("Needs Microsoft's free Debugging Tools for Windows, part of the Windows SDK.")
        } else if !self.elevated {
            r.on_disabled_hover_text("Crash files can only be opened as administrator.")
        } else {
            r
        };
        if r.clicked() {
            self.ask(Pending::AnalyzeDumps);
        }
    }

    // ---------- Security ----------

    fn security(&mut self, ui: &mut Ui, snap: &Arc<Snapshot>) {
        let sec = &snap.security;
        let d = sec.g("defender");
        section(ui, "Windows Security");
        card(ui, |ui| {
            kv(
                ui,
                "sec-def",
                &[
                    ("Real-time protection", on_off(d.b("RealTime"))),
                    ("Tamper protection", on_off(d.b("Tamper"))),
                    (
                        "Blocks adware and bundled extras",
                        d.i("PUAProtection").map(|v| {
                            match v {
                                1 => "On",
                                2 => "Only reports",
                                _ => "Off",
                            }
                            .to_string()
                        }),
                    ),
                    ("Cloud protection", d.i("CloudProtection").map(|v| if v == 0 { "Off" } else { "On" }.to_string())),
                    ("Ransomware protection", d.i("ControlledFolderAccess").map(|v| if v == 1 { "On" } else { "Off" }.to_string())),
                    ("Definitions updated", d.s("SignaturesUpdated")),
                    ("Last quick scan", d.s("LastQuickScan")),
                    ("Last full scan", d.s("LastFullScan")),
                    ("Other antivirus", Some(sec.strs("otherAntivirus").join(", "))),
                ],
            );
        });
        section(ui, "Skip list");
        card(ui, |ui| {
            let paths = d.strs("ExclusionPaths");
            if paths.iter().any(|x| x.starts_with("N/A")) {
                muted(ui, "Windows only shows this list to administrators. Restart rigscan as administrator to see it.");
            } else {
                let all: Vec<Vec<String>> = paths
                    .iter()
                    .map(|x| vec!["Folder".into(), x.clone()])
                    .chain(d.strs("ExclusionProcesses").into_iter().map(|x| vec!["Program".into(), x]))
                    .chain(d.strs("ExclusionExtensions").into_iter().map(|x| vec!["File type".into(), x]))
                    .collect();
                if all.is_empty() {
                    muted(ui, "Windows Security checks everything. Nothing is skipped.");
                } else {
                    muted(ui, "Windows Security never checks these. Remove the ones you don't recognise from Suggestions.");
                    table(ui, "sec-ex", &["Type", "Skipped"], &all);
                }
            }
        });
        let threats: Vec<Vec<String>> = sec
            .a("threats")
            .iter()
            .map(|t| vec![s(t, "Date"), s(t, "Name"), if t.b("ActionSuccess") == Some(false) { "Waiting for you".into() } else { "Dealt with".into() }])
            .collect();
        section(ui, "Past detections");
        card(ui, |ui| {
            if threats.is_empty() {
                muted(ui, "Windows Security hasn't found anything harmful recently.");
            } else {
                table(ui, "sec-th", &["Date", "What", "Status"], &threats);
            }
        });
        section(ui, "Firewall");
        card(ui, |ui| {
            let rows: Vec<(&str, Option<String>)> = sec
                .a("firewallProfiles")
                .iter()
                .map(|f| {
                    (
                        match s(f, "Name").as_str() {
                            "Domain" => "Work networks",
                            "Private" => "Home networks",
                            _ => "Public networks",
                        },
                        on_off(f.b("Enabled")),
                    )
                })
                .collect();
            kv(ui, "sec-fw", &rows);
        });
        let a = sec.g("access");
        section(ui, "Access to this PC");
        card(ui, |ui| {
            kv(
                ui,
                "sec-ac",
                &[
                    ("Asks before system changes (UAC)", on_off(a.b("UAC"))),
                    ("Remote Desktop", on_off(a.b("RemoteDesktop"))),
                    ("Remote Assistance", on_off(a.b("RemoteAssistance"))),
                    ("Chrome Remote Desktop", a.s("ChromeRemoteDesktop")),
                    (
                        "Download warnings (SmartScreen)",
                        a.s("SmartScreen").map(|v| if v.eq_ignore_ascii_case("off") { "Off".into() } else { "On".into() }).or(Some("On (default)".into())),
                    ),
                    ("Old SMB1 file sharing", on_off(a.b("SMB1"))),
                    ("Signs in without a password", yes_no(a.b("AutoLogon"))),
                    ("Administrators", Some(sec.strs("admins").join(", "))),
                    ("Hosts file entries", sec.g("hosts").i("Entries").map(|n| n.to_string())),
                ],
            );
        });
        self.windows_update(ui, snap);
        self.system_restore(ui, snap);
        self.device_history(ui, snap);
        let accts: Vec<Vec<String>> = sec
            .a("accounts")
            .iter()
            .map(|x| vec![s(x, "Name"), if x.b("Enabled") == Some(true) { "On".into() } else { "Off".into() }, s(x, "LastLogon"), s(x, "Source")])
            .collect();
        section(ui, "Accounts on this PC");
        card(ui, |ui| table(ui, "sec-acct", &["Name", "Status", "Last sign-in", "Type"], &accts));
        let kd: Vec<Vec<String>> = sec.a("kernelDrivers").iter().map(|k| vec![s(k, "Display"), s(k, "Signer"), s(k, "Path")]).collect();
        section(ui, "Drivers from other makers");
        card(ui, |ui| {
            muted(ui, "Low-level drivers running right now that don't come from Microsoft. Most belong to your hardware, antivirus or games.");
            table(ui, "sec-kd", &["Driver", "Signed by", "File"], &kd);
        });
    }

    fn windows_update(&mut self, ui: &mut Ui, snap: &Snapshot) {
        let w = snap.system.g("winupdate");
        if w.is_null() {
            return;
        }
        section(ui, "Windows Update");
        card(ui, |ui| {
            let paused = w.s("PausedUntil").filter(|d| *d >= chrono::Local::now().format("%Y-%m-%d").to_string());
            kv(
                ui,
                "sec-wu",
                &[
                    ("Last update installed", w.s("LastInstalled").map(|d| format!("{d}, {}", s(w, "LastInstalledTitle")))),
                    ("Automatic updates", Some(if w.b("AutoUpdateOff") == Some(true) { "Off (set by a policy)" } else { "On" }.into())),
                    ("Paused", Some(paused.map(|d| format!("Until {d}")).unwrap_or_else(|| "No".into()))),
                ],
            );
            let failed: Vec<Vec<String>> = w
                .a("Failed")
                .iter()
                .map(|u| {
                    let (name, store) = crate::rules::update_name(&s(u, "Title"));
                    vec![
                        if store { format!("{name} (Store app)") } else { name },
                        s(u, "Last"),
                        u.i("Times").map(|t| t.to_string()).unwrap_or_default(),
                        s(u, "Code"),
                    ]
                })
                .collect();
            ui.add_space(S3);
            if failed.is_empty() {
                muted(ui, "No update failed to install in the last 60 days.");
            } else {
                muted(ui, "Updates that failed to install in the last 60 days:");
                table(ui, "sec-wu-failed", &["Update", "Last try", "Times", "Error code"], &failed);
            }
        });
    }

    fn system_restore(&mut self, ui: &mut Ui, snap: &Snapshot) {
        let r = snap.security.g("restore");
        if r.is_null() {
            return;
        }
        section(ui, "System Restore");
        card(ui, |ui| {
            let on = r.b("COn");
            if on.is_none() {
                muted(ui, "Windows only shows restore points to administrators. Restart rigscan as administrator to see them.");
                return;
            }
            kv(
                ui,
                "sec-rp",
                &[
                    ("System Restore for drive C", on_off(on)),
                    ("Latest restore point", r.g("Latest").s("When").map(|w| format!("{w}, {}", s(r.g("Latest"), "What")))),
                    ("Restore points saved", r.i("Count").map(|n| n.to_string())),
                ],
            );
            ui.add_space(S3);
            ui.horizontal_wrapped(|ui| {
                if on == Some(true) {
                    self.restore_point_button(ui);
                } else {
                    self.fix_button(ui, &Fix::EnableSystemProtection, "System Restore is off");
                }
                if widgets::icon_button(ui, icon::OPEN, "Open System Restore", true).clicked() {
                    if let Err(e) = shell_open("rstrui.exe") {
                        self.toast(format!("{e:#}"), false);
                    }
                }
            });
        });
    }

    fn device_history(&mut self, ui: &mut Ui, snap: &Snapshot) {
        let rows = crate::rules::security::device_use(snap);
        let sw = snap.security.g("deviceAccessSwitch");
        section(ui, "Camera, microphone and location");
        card(ui, |ui| {
            muted(ui, "Windows keeps a note of which apps used these and when. rigscan only reads that note; it can't see or hear anything.");
            let off: Vec<&str> = [("webcam", "camera"), ("microphone", "microphone"), ("location", "location")]
                .iter()
                .filter(|(k, _)| sw.s(k).as_deref() == Some("Deny"))
                .map(|(_, n)| *n)
                .collect();
            if !off.is_empty() {
                ui.add_space(S2);
                small(ui, format!("Turned off for all apps: {}.", off.join(", ")));
            }
            ui.add_space(S3);
            if rows.is_empty() {
                muted(ui, "No app has used the camera, microphone or location yet.");
            } else {
                // One row per app, newest first (the rows already come sorted that way).
                let mut apps: Vec<(String, Vec<&str>, String, bool)> = vec![];
                for d in &rows {
                    let name = if d.windows { format!("Windows ({})", d.app) } else { d.app.clone() };
                    match apps.iter_mut().find(|a| a.0 == name) {
                        Some(a) => {
                            if !a.1.contains(&d.what) {
                                a.1.push(d.what);
                            }
                            a.3 |= d.in_use;
                        }
                        None => apps.push((name, vec![d.what], d.last_used.clone(), d.in_use)),
                    }
                }
                let t: Vec<Vec<String>> =
                    apps.into_iter().map(|(name, what, last, now)| vec![name, if now { "In use now".into() } else { last }, what.join(", ")]).collect();
                let head = ["App", "Last used", "Used"];
                let recent = t.len().min(12);
                table(ui, "sec-dev", &head, &t[..recent]);
                if t.len() > recent {
                    ui.add_space(S2);
                    ui.collapsing(format!("{} used earlier", plural(t.len() - recent, "more app", "more apps")), |ui| {
                        table(ui, "sec-dev-old", &head, &t[recent..])
                    });
                }
            }
            ui.add_space(S3);
            ui.horizontal_wrapped(|ui| {
                for (label, target) in [
                    ("Camera settings", "ms-settings:privacy-webcam"),
                    ("Microphone settings", "ms-settings:privacy-microphone"),
                    ("Location settings", "ms-settings:privacy-location"),
                ] {
                    if widgets::icon_button(ui, icon::OPEN, label, true).clicked() {
                        if let Err(e) = shell_open(target) {
                            self.toast(format!("{e:#}"), false);
                        }
                    }
                }
            });
        });
    }

    // ---------- Apps ----------

    fn software(&mut self, ui: &mut Ui, snap: &Arc<Snapshot>) {
        let p = pal(ui.ctx());
        section(ui, "App updates");
        card(ui, |ui| {
            if !crate::collect::winget::available() {
                muted(ui, "Update checks use winget, Windows' own app installer, which isn't available on this PC. Install \"App Installer\" from the Microsoft Store to turn this on.");
                return;
            }
            let busy = self.busy.contains("winget");
            match &snap.upgrades {
                None => {
                    muted(ui, "rigscan can ask winget which of your apps have newer versions. This uses the internet, so it only runs when you ask.");
                    ui.add_space(S2);
                }
                Some(list) if list.is_empty() => {
                    muted(ui, "Everything winget knows about is up to date.");
                    ui.add_space(S2);
                }
                Some(list) => {
                    for u in list.clone() {
                        ui.horizontal(|ui| {
                            let id_ok = !u.id.ends_with('…');
                            let key = format!("upgrade:{}", u.id);
                            if self.busy.contains(&key) {
                                ui.add(egui::Spinner::new());
                            } else {
                                let r = widgets::secondary_enabled(ui, "Update", id_ok && !self.busy.iter().any(|b| b.starts_with("upgrade:")));
                                if !id_ok {
                                    r.clone().on_disabled_hover_text("The app's ID was cut short in winget's list. Update it from the app itself.");
                                }
                                if r.clicked() {
                                    self.ask(Pending::Upgrade(u.id.clone(), u.name.clone()));
                                }
                            }
                            ui.label(RichText::new(&u.name).family(theme::semibold()));
                            ui.label(RichText::new(format!("{} to {}", u.installed, u.available)).color(p.muted));
                        });
                    }
                    ui.add_space(S2);
                }
            }
            if busy {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    muted(ui, "Asking winget. This can take a minute.");
                });
            } else if widgets::icon_button(ui, icon::REFRESH, "Check for app updates", true).clicked() {
                self.ask(Pending::CheckUpdates);
            }
        });

        section(ui, &format!("Browser extensions ({})", snap.extensions.len()));
        card(ui, |ui| {
            muted(ui, "\"From the store\" means it came from the browser's official store. \"Reads all sites\" means it can see every page you open.");
            let rows: Vec<Vec<String>> = snap
                .extensions
                .iter()
                .map(|e| {
                    vec![
                        e.name.clone(),
                        format!("{} ({})", e.browser, e.profile),
                        yes(e.from_store),
                        yes(e.all_sites),
                        if e.enabled { "On".into() } else { "Off".into() },
                    ]
                })
                .collect();
            table(ui, "ext", &["Extension", "Browser", "From the store", "Reads all sites", "Status"], &rows);
        });

        let apps = snap.software.a("apps");
        section(ui, &format!("Installed apps ({})", apps.len()));
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.ui.search).hint_text("Search apps").desired_width(280.0));
                if ui.link("Open Installed apps in Settings").clicked() {
                    if let Err(e) = shell_open("ms-settings:appsfeatures") {
                        self.toast(format!("{e:#}"), false);
                    }
                }
            });
            ui.add_space(S2);
            let q = self.ui.search.to_lowercase();
            let rows: Vec<Vec<String>> = apps
                .iter()
                .filter(|a| q.is_empty() || s(a, "Name").to_lowercase().contains(&q) || s(a, "Publisher").to_lowercase().contains(&q))
                .map(|a| vec![s(a, "Name"), s(a, "Version"), s(a, "Publisher"), s(a, "Installed"), num(a.f("SizeMB"), " MB").unwrap_or_default()])
                .collect();
            table(ui, "apps", &["Name", "Version", "Publisher", "Installed", "Size"], &rows);
        });
        let rt = snap.software.strs("runtimes");
        if !rt.is_empty() {
            section(ui, "Runtimes");
            card(ui, |ui| {
                ui.add(egui::Label::new(rt.join(", ")).wrap());
            });
        }
    }

    // ---------- Startup ----------

    fn startup(&mut self, ui: &mut Ui, snap: &Arc<Snapshot>) {
        let p = pal(ui.ctx());
        section(ui, "Opens when you sign in");
        card(ui, |ui| {
            muted(ui, "Switching an app off here only stops it opening by itself. It stays installed, and you can switch it back on.");
            ui.add_space(S2);
            let items = snap.software.a("startup");
            if items.is_empty() {
                muted(ui, "Nothing starts with Windows.");
            }
            for it in items {
                let (name, key) = (s(it, "Name"), s(it, "ApprovedKey"));
                let on = it.b("Enabled") != Some(false);
                ui.horizontal(|ui| {
                    let fx = if on {
                        Fix::DisableStartup { name: name.clone(), approved_key: key.clone() }
                    } else {
                        Fix::EnableStartup { name: name.clone(), approved_key: key.clone() }
                    };
                    ui.allocate_ui(egui::vec2(200.0, 30.0), |ui| self.fix_button(ui, &fx, &name));
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&name).family(theme::semibold()));
                            if !on {
                                widgets::pill(ui, "Off", p.muted, p.surface2);
                            }
                        });
                        ui.add(egui::Label::new(RichText::new(s(it, "Command")).small().color(p.muted)).truncate()).on_hover_text(format!(
                            "{}\n{}",
                            s(it, "Scope"),
                            s(it, "Command")
                        ));
                    });
                });
                ui.add_space(S1);
            }
        });
        section(ui, "Background services from other makers");
        card(ui, |ui| {
            muted(
                ui,
                "Services run without a window. These aren't part of Windows. Setting one to start only when needed is safe for most updaters and helpers.",
            );
            ui.add_space(S2);
            for sv in snap.software.a("services") {
                let (name, disp) = (s(sv, "Name"), s(sv, "Display"));
                ui.horizontal(|ui| {
                    ui.allocate_ui(egui::vec2(200.0, 30.0), |ui| {
                        if s(sv, "StartMode") == "Auto" {
                            self.fix_button(ui, &Fix::ServiceManual { name: name.clone(), display: disp.clone() }, &disp);
                        } else {
                            small(ui, "Starts when needed");
                        }
                    });
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&disp).family(theme::semibold()));
                        ui.add(egui::Label::new(RichText::new(format!("{} · {}", s(sv, "State"), s(sv, "Signer"))).small().color(p.muted)).truncate())
                            .on_hover_text(s(sv, "Path"));
                    });
                });
                ui.add_space(S1);
            }
        });
        let tasks: Vec<Vec<String>> = snap.software.a("tasks").iter().map(|t| vec![s(t, "Name"), s(t, "Action"), s(t, "Author"), s(t, "Path")]).collect();
        section(ui, "Scheduled tasks from other makers");
        card(ui, |ui| table(ui, "tasks", &["Name", "Runs", "Made by", "Folder"], &tasks));
        let delays: Vec<Vec<String>> =
            snap.health.a("bootDelays").iter().map(|d| vec![s(d, "Name"), s(d, "Kind"), num(d.f("AvgSec"), " s").unwrap_or_default(), s(d, "Times")]).collect();
        if !delays.is_empty() {
            section(ui, "What slowed recent starts");
            card(ui, |ui| table(ui, "delays", &["Item", "Kind", "Average delay", "Times"], &delays));
        }
    }

    // ---------- Crashes ----------

    fn crashes(&mut self, ui: &mut Ui, snap: &Arc<Snapshot>) {
        let p = pal(ui.ctx());
        let h = &snap.health;
        if let Some(si) = h.f("stabilityIndex") {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{si:.1}")).text_style(theme::big()));
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Windows stability score, out of 10").family(theme::semibold()));
                        muted(ui, "Windows' own rating, based on crashes and failures over recent weeks.");
                    });
                });
            });
        }
        section(ui, "Blue-screen crashes");
        let bsod: Vec<Vec<String>> = h
            .a("bugchecks")
            .iter()
            .map(|b| {
                let code = b.s("Text").and_then(|t| crate::rules::bugcheck_code(&t));
                let (name, why) = code.map(crate::collect::dumps::bugcheck_info).unwrap_or(("Unknown", ""));
                vec![s(b, "Date"), code.map(|c| format!("0x{c:X} {name}")).unwrap_or_default(), why.to_string()]
            })
            .collect();
        card(ui, |ui| {
            if bsod.is_empty() {
                muted(ui, "No blue-screen crashes in the last 90 days.");
            } else {
                table(ui, "bsod", &["When", "Stop code", "What it usually means"], &bsod);
            }
        });
        let dumps = h.a("minidumps");
        section(ui, "Crash files");
        card(ui, |ui| {
            if dumps.is_empty() {
                muted(ui, if snap.elevated { "There are no crash files to read." } else { "Crash files can only be seen as administrator." });
                return;
            }
            muted(
                ui,
                format!("Windows saved {}. Reading them shows which driver was running when it crashed.", plural(dumps.len(), "crash file", "crash files")),
            );
            ui.add_space(S2);
            ui.horizontal(|ui| self.dumps_button(ui));
            if !snap.dumps.is_empty() {
                ui.add_space(S2);
                let rows: Vec<Vec<String>> = snap
                    .dumps
                    .iter()
                    .map(|d| {
                        vec![
                            d.date.clone(),
                            format!("{} {}", d.code, d.name),
                            d.image.clone().or(d.module.clone()).unwrap_or_default(),
                            d.note.clone().unwrap_or_default(),
                        ]
                    })
                    .collect();
                table(ui, "dumps", &["When", "Stop code", "Driver involved", "What that is"], &rows);
            }
        });
        section(ui, "Sudden shutdowns");
        let sd: Vec<Vec<String>> = h
            .a("unexpectedShutdowns")
            .iter()
            .map(|e| {
                vec![
                    s(e, "Date"),
                    if e.s("Code").as_deref() == Some("0x0") { "No crash recorded".into() } else { format!("Crash {}", s(e, "Code")) },
                    if e.b("PowerButton") == Some(true) { "Yes".into() } else { "No".into() },
                ]
            })
            .collect();
        card(ui, |ui| {
            if sd.is_empty() {
                muted(ui, "Windows always shut down properly in the last 90 days.");
            } else {
                table(ui, "sd", &["When", "Reason", "Power button held"], &sd);
            }
        });
        section(ui, "Hardware errors");
        let wh: Vec<Vec<String>> = h.a("whea").iter().map(|w| vec![s(w, "Date"), s(w, "Text")]).collect();
        card(ui, |ui| {
            if wh.is_empty() {
                muted(ui, "The hardware hasn't reported any errors.");
            } else {
                table(ui, "whea", &["When", "Message"], &wh);
            }
        });
        let ac: Vec<Vec<String>> = h.a("appCrashes").iter().map(|a| vec![s(a, "App"), s(a, "Count")]).collect();
        section(ui, "App crashes this month");
        card(ui, |ui| {
            if ac.is_empty() {
                muted(ui, "No app crashes this month.");
            } else {
                table(ui, "ac", &["App", "Times"], &ac);
            }
        });
        let st: Vec<Vec<String>> = h.a("serviceTimeouts").iter().map(|x| vec![s(x, "Count"), s(x, "Message")]).collect();
        if !st.is_empty() {
            section(ui, "Services that failed to start this week");
            card(ui, |ui| table(ui, "svc", &["Times", "Message"], &st));
        }
        let _ = p;
    }

    // ---------- Changes ----------

    fn changes_page(&mut self, ui: &mut Ui) {
        let p = pal(ui.ctx());
        if self.prev_taken.is_none() {
            widgets::empty(
                ui,
                icon::CHANGES,
                "Nothing to compare yet",
                "This is the first check on this PC. Next time, rigscan will list what was installed, removed or changed in between.",
            );
            return;
        }
        if self.changes.is_empty() {
            widgets::empty(ui, icon::CHECK, "Nothing changed", "Everything rigscan tracks is the same as last time.");
            return;
        }
        let mut groups: Vec<&str> = self.changes.iter().map(|c| c.group).collect();
        groups.dedup();
        let mut seen = std::collections::HashSet::new();
        groups.retain(|g| seen.insert(*g));
        for g in groups {
            let items: Vec<_> = self.changes.iter().filter(|c| c.group == g).collect();
            section(ui, &format!("{g} ({})", items.len()));
            card(ui, |ui| {
                for c in items {
                    ui.horizontal(|ui| {
                        let (t, fg, bg) = match c.kind {
                            ChangeKind::Added => ("Added", p.blue, p.blue_bg),
                            ChangeKind::Removed => ("Removed", p.muted, p.surface2),
                            ChangeKind::Changed => ("Changed", p.ink, p.surface2),
                        };
                        widgets::pill(ui, t, fg, bg);
                        ui.add(egui::Label::new(&c.text).truncate());
                        if c.notable {
                            widgets::pill(ui, "Worth a look", p.amber, p.amber_bg)
                                .on_hover_text("Changes like this are worth a look if you didn't make them yourself.");
                        }
                    });
                }
            });
        }
    }

    // ---------- Virus check ----------

    fn scan_page(&mut self, ui: &mut Ui, snap: &Arc<Snapshot>) {
        let p = pal(ui.ctx());
        if crate::scan::mpcmdrun().is_none() {
            widgets::empty(
                ui,
                icon::SHIELD,
                "Windows Security's scanner isn't available",
                "Another antivirus may have replaced it. Use that program's own scan instead.",
            );
            return;
        }
        let ex = snap.security.g("defender").strs("ExclusionPaths");
        let whole: Vec<String> = ex.iter().filter(|e| e.trim_end_matches('\\').len() <= 2 && e.contains(':')).cloned().collect();
        if !whole.is_empty() {
            widgets::note(ui, p.amber, p.amber_bg, icon::WARN, |ui| {
                ui.add(
                    egui::Label::new(format!(
                        "Windows Security is set to skip {}, so the quick and full checks won't look there. The folder check below ignores the skip list.",
                        whole.join(" and ")
                    ))
                    .wrap(),
                );
            });
            ui.add_space(S4);
        }
        if let (Some(started), true) = (self.ui.scan_started, self.busy.contains("scan")) {
            let mins = started.elapsed().as_secs() / 60;
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(20.0).color(p.accent));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(format!("{} running", self.ui.scan_label)).family(theme::semibold()));
                        muted(
                            ui,
                            if mins == 0 {
                                "Just started. You can keep using your PC.".to_string()
                            } else {
                                format!("{} so far. You can keep using your PC.", plural(mins as usize, "minute", "minutes"))
                            },
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::secondary(ui, "Stop").clicked() {
                            if let Some(c) = &self.ui.scan_cancel {
                                c.store(true, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                    });
                });
            });
            ui.add_space(S4);
        }
        let running = self.busy.contains("scan");
        let options = [
            (
                scan::Kind::Quick,
                "Quick check",
                "Looks where harmful software usually hides. A few minutes. Windows Security deals with anything it finds, as usual.",
            ),
            (scan::Kind::Full, "Full check", "Every file on every drive. Can take an hour or more. Best left running while you do something else."),
            (
                scan::Kind::Folder,
                "Check a folder or drive",
                "Checks one place you choose and only reports what it finds. Nothing is moved or deleted, so you decide what to do. Ignores the skip list.",
            ),
        ];
        for (kind, title, body) in options {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_max_width((ui.available_width() - 140.0).max(200.0));
                        ui.label(RichText::new(title).family(theme::semibold()));
                        ui.add(egui::Label::new(RichText::new(body).color(p.muted)).wrap());
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::secondary_enabled(ui, "Start", !running).clicked() {
                            if kind == scan::Kind::Folder {
                                if let Some(dir) = rfd::FileDialog::new().set_title("Choose a folder or drive to check").pick_folder() {
                                    self.ask(Pending::Scan(kind, Some(dir)));
                                }
                            } else {
                                self.ask(Pending::Scan(kind, None));
                            }
                        }
                    });
                });
            });
            ui.add_space(S2);
        }
        section(ui, "Results");
        if snap.scans.is_empty() {
            muted(ui, "No checks run from rigscan yet.");
        }
        for r in &snap.scans {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    let (ic, fg) = if r.ok { (icon::CHECK, p.green) } else { (icon::WARN, p.amber) };
                    ui.label(ico(ic, 16.0).color(fg));
                    ui.label(RichText::new(format!("{}: {}", r.kind, r.message)).family(theme::semibold()));
                });
                small(ui, format!("{} · {} · {:.1} minutes", r.started, r.target, r.minutes));
                if r.skipped_by_exclusions {
                    small(ui, "Some drives were on Windows Security's skip list and weren't checked.");
                }
                for t in &r.threats {
                    ui.label(RichText::new(t).color(p.red));
                }
                for f in &r.files {
                    small(ui, f);
                }
                if !r.threats.is_empty() {
                    ui.add_space(S2);
                    if widgets::icon_button(ui, icon::OPEN, "Open Windows Security", true).clicked() {
                        if let Err(e) = shell_open("windowsdefender://threat") {
                            self.toast(format!("{e:#}"), false);
                        }
                    }
                }
            });
            ui.add_space(S2);
        }
    }

    // ---------- History ----------

    fn history_page(&mut self, ui: &mut Ui) {
        let p = pal(ui.ctx());
        section(ui, "Changes made with rigscan");
        if self.history.is_empty() {
            widgets::empty(ui, icon::UNDO, "No changes yet", "When you use a fix, it shows up here with a button to undo it.");
        }
        let hist = self.history.clone();
        for (i, h) in hist.iter().enumerate().rev() {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(format!("{}: {}", h.fix.label(), fix_target(&h.fix))).family(theme::semibold()));
                        small(ui, format!("{} · for \"{}\"", h.when, h.title));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(u) = &h.undone {
                            ui.label(RichText::new(format!("Undone {u}")).color(p.muted));
                        } else if self.busy.contains(&format!("undo:{i}")) {
                            ui.add(egui::Spinner::new());
                        } else {
                            let blocked = h.fix.needs_admin() && !self.elevated;
                            let r = widgets::icon_button(ui, icon::UNDO, "Undo", !blocked);
                            if blocked {
                                r.clone().on_disabled_hover_text("Needs administrator rights, like the original change.");
                            }
                            if r.clicked() {
                                self.ask(Pending::Undo(i, h.title.clone()));
                            }
                        }
                    });
                });
            });
            ui.add_space(S2);
        }
        section(ui, "Saved checks");
        card(ui, |ui| {
            let n = self.snap.as_ref().and_then(|s| crate::snapshot::list(&s.host).ok()).map(|v| v.len()).unwrap_or(0);
            muted(ui, format!("rigscan keeps {} on this PC so it can show what changed between them.", plural(n, "check", "checks")));
            ui.add_space(S2);
            ui.horizontal(|ui| {
                if widgets::icon_button(ui, icon::FOLDER, "Open the folder", true).clicked() {
                    match crate::util::data_dir("") {
                        Ok(d) => {
                            if let Err(e) = shell_open(&d.to_string_lossy()) {
                                self.toast(format!("{e:#}"), false);
                            }
                        }
                        Err(e) => self.toast(format!("{e:#}"), false),
                    }
                }
            });
        });
    }

    // ---------- Report ----------

    fn report_page(&mut self, ui: &mut Ui, snap: &Arc<Snapshot>) {
        let busy = self.busy.contains("report");
        card(ui, |ui| {
            ui.label("The report includes the suggestions, hardware details, drive health, security settings and the list of apps and extensions. It's saved only where you choose.");
            ui.add_space(S3);
            ui.checkbox(&mut self.ui.hide_personal, "Hide personal details");
            small(
                ui,
                "Removes serial numbers, network addresses, the PC name, account names and email addresses. Keep this on if you'll send the report to someone.",
            );
            ui.add_space(S4);
            ui.horizontal(|ui| {
                for (label, ext, pdf) in [("Save as web page", "html", false), ("Save as PDF", "pdf", true)] {
                    let b = if pdf { widgets::secondary_enabled(ui, label, !busy) } else { widgets::primary_enabled(ui, label, !busy) };
                    if b.clicked() {
                        let named = Snapshot { host: if self.ui.hide_personal { "pc".into() } else { snap.host.clone() }, ..(**snap).clone() };
                        let name = crate::report::default_name(&named, ext);
                        let filter = if pdf { "PDF" } else { "Web page" };
                        if let Some(path) = rfd::FileDialog::new().set_title("Save the report").set_file_name(&name).add_filter(filter, &[ext]).save_file() {
                            let s2 = snap.clone();
                            let hide = self.ui.hide_personal;
                            self.busy.insert("report".into());
                            self.spawn(ui.ctx(), move || {
                                let r = if pdf { crate::report::write_pdf(&s2, hide, &path) } else { crate::report::write_html(&s2, hide, &path) };
                                super::Job::Saved(r.map(|_| path).map_err(|e| format!("{e:#}")))
                            });
                        }
                    }
                }
                if busy {
                    ui.add(egui::Spinner::new());
                    muted(ui, "Saving");
                }
            });
            if let Some(path) = self.ui.last_report.clone() {
                ui.add_space(S3);
                ui.horizontal(|ui| {
                    small(ui, format!("Last saved: {}", path.display()));
                    if ui.link("Open it").clicked() {
                        if let Err(e) = shell_open(&path.to_string_lossy()) {
                            self.toast(format!("{e:#}"), false);
                        }
                    }
                });
            }
        });
    }

    // ---------- Privacy ----------

    fn privacy(&mut self, ui: &mut Ui) {
        let p = pal(ui.ctx());
        let para = |ui: &mut Ui, t: &str| {
            ui.add(egui::Label::new(t).wrap());
            ui.add_space(S2);
        };
        section(ui, "The short version");
        card(ui, |ui| {
            para(ui, "rigscan works entirely on this PC. There's no account, no analytics, no ads and no \"cloud scan\". Nothing rigscan finds is sent to us or anyone else.");
            para(ui, "It only reads, until you press a fix button. Every fix explains itself first and can be undone from History.");
        });
        section(ui, "What it reads");
        card(ui, |ui| {
            for r in super::screens::READS {
                ui.horizontal_top(|ui| {
                    ui.label(RichText::new("•").color(p.muted));
                    ui.add(egui::Label::new(r).wrap());
                });
            }
        });
        section(ui, "When it uses the internet");
        card(ui, |ui| {
            para(ui, "Only when you press one of these, and each asks first:");
            para(ui, "• Check for app updates: winget, Windows' own app installer, compares your apps with Microsoft's public app catalog.");
            para(ui, "• Read crash files: Microsoft's debugger downloads matching debugging files from Microsoft. The crash files stay here.");
            para(ui, "• Update an app: winget downloads it from the app's publisher.");
            para(ui, "Saving a PDF uses Microsoft Edge to print the report, and virus checks are run by Windows Security. Both follow their own settings, as they always do.");
        });
        section(ui, "What it keeps, and where");
        card(ui, |ui| {
            para(ui, "Each check is saved as a file in your user folder (AppData\\Local\\rigscan), so it can show what changed next time. The undo history lives there too. Nothing else is stored.");
            ui.horizontal(|ui| {
                if widgets::icon_button(ui, icon::FOLDER, "Open the folder", true).clicked() {
                    if let Ok(d) = crate::util::data_dir("") {
                        if let Err(e) = shell_open(&d.to_string_lossy()) {
                            self.toast(format!("{e:#}"), false);
                        }
                    }
                }
                if widgets::secondary_enabled(ui, "Delete everything rigscan saved", !self.busy.contains("delete")).clicked() {
                    self.ask(Pending::DeleteData);
                }
            });
        });
    }
}

fn yes(b: bool) -> String {
    if b { "Yes" } else { "No" }.into()
}

/// Text motherboard makers leave in fields they never filled in.
fn is_placeholder(v: &str) -> bool {
    let v = v.to_ascii_lowercase();
    ["to be filled", "system product name", "system serial number", "default string", "not specified", "0123456789"].iter().any(|g| v.contains(g))
}

fn model(s: &Snapshot) -> String {
    let d = s.system.g("device");
    let m = format!("{} {}", d.s("Manufacturer").unwrap_or_default(), d.s("Model").unwrap_or_default()).trim().to_string();
    if m.is_empty() || is_placeholder(&m) {
        let b = format!("{} {}", d.s("BoardMaker").unwrap_or_default(), d.s("BoardProduct").unwrap_or_default()).trim().to_string();
        if b.is_empty() {
            "This PC".into()
        } else {
            format!("Custom PC with {b} motherboard")
        }
    } else {
        m
    }
}

// ---------- Detail pages that only display data ----------

/// One line on whether this Windows 10 PC can move to Windows 11.
fn win11_text(snap: &Snapshot) -> String {
    let r = crate::rules::win11_readiness(snap);
    if !r.missing.is_empty() {
        format!("No. Missing: {}", r.missing.join("; "))
    } else if !r.unsure.is_empty() {
        format!("Probably. Couldn't confirm: {}", r.unsure.join("; "))
    } else {
        "Looks ready. It's a free upgrade from Windows Update.".into()
    }
}

fn upgrade_advisor(ui: &mut Ui, snap: &Snapshot) {
    let p = pal(ui.ctx());
    let list = crate::rules::upgrades::upgrades(snap);
    section(ui, "Upgrades that would help most");
    card(ui, |ui| {
        if list.is_empty() {
            muted(ui, "No part stands out as holding this PC back, so there's nothing worth buying right now.");
            return;
        }
        for (i, u) in list.iter().enumerate() {
            if i > 0 {
                ui.add_space(S4);
            }
            ui.label(RichText::new(format!("{}. {}", i + 1, u.title)).family(theme::semibold()).size(16.0));
            ui.add_space(S1);
            ui.add(egui::Label::new(&u.why).wrap());
            ui.add_space(S2);
            for g in &u.get {
                ui.horizontal_top(|ui| {
                    ui.label(RichText::new("•").color(p.muted));
                    ui.add(egui::Label::new(RichText::new(g).color(p.muted)).wrap());
                });
            }
        }
        ui.add_space(S3);
        small(ui, "Biggest difference first, based on this check. rigscan doesn't sell anything or recommend brands.");
    });
}

fn this_pc(ui: &mut Ui, snap: &Snapshot) {
    let sys = &snap.system;
    let dev = sys.g("device");
    let os = sys.g("os");
    let cpu = sys.g("cpu");
    let mem = sys.g("memory");
    upgrade_advisor(ui, snap);
    section(ui, "Device");
    card(ui, |ui| {
        kv(
            ui,
            "pc-dev",
            &[
                ("Name", dev.s("Hostname")),
                ("Maker and model", Some(model(snap))),
                ("Motherboard", Some(format!("{} {} {}", s(dev, "BoardMaker"), s(dev, "BoardProduct"), s(dev, "BoardVersion")).trim().to_string())),
                ("BIOS", dev.s("BIOSVersion").map(|v| format!("{v}, dated {}", s(dev, "BIOSDate")))),
                ("Serial number", dev.s("Serial").filter(|v| !is_placeholder(v))),
                ("Start-up mode", dev.s("FirmwareMode")),
                ("Secure Boot", on_off(dev.b("SecureBoot"))),
                (
                    "Security chip (TPM)",
                    dev.s("TpmSpec").map(|v| match dev.b("TpmReady") {
                        Some(true) => format!("Version {v}, ready"),
                        Some(false) => format!("Version {v}, not ready"),
                        None => format!("Version {v}"),
                    }),
                ),
            ],
        );
    });
    section(ui, "Windows");
    card(ui, |ui| {
        kv(
            ui,
            "pc-os",
            &[
                ("Edition", os.s("Edition")),
                ("Version", os.s("Version").map(|v| format!("{v} (build {})", s(os, "Build")))),
                ("Ready for Windows 11", os.s("Edition").filter(|e| e.contains("Windows 10")).map(|_| win11_text(snap))),
                ("Installed on", os.s("Installed")),
                ("Activation", os.s("Activation")),
                ("Last restart", os.s("LastBoot")),
                ("Fast Startup", os.i("FastStartup").map(|v| if v == 1 { "On" } else { "Off" }.to_string())),
                ("Memory integrity", os.s("MemoryIntegrity")),
                ("Insider channel", os.s("InsiderChannel")),
                ("Language and region", os.s("Locale")),
            ],
        );
        let ups: Vec<Vec<String>> = sys.a("updates").iter().map(|u| vec![s(u, "Installed"), s(u, "Id"), s(u, "Type")]).collect();
        if !ups.is_empty() {
            ui.add_space(S3);
            ui.collapsing("Recent updates", |ui| table(ui, "ups", &["Installed", "Update", "Type"], &ups));
        }
    });
    section(ui, "Processor");
    card(ui, |ui| {
        kv(
            ui,
            "pc-cpu",
            &[
                ("Model", cpu.s("Name")),
                ("Cores", cpu.i("Cores").map(|c| format!("{c} cores, {} threads", cpu.i("Threads").unwrap_or(0)))),
                ("Rated speed", cpu.f("RatedMHz").map(|m| format!("{:.2} GHz", m / 1000.0))),
                ("Cache", cpu.f("L3KB").map(|k| format!("{:.0} MB L3", k / 1024.0))),
                ("Socket", cpu.s("Socket")),
                ("Virtualization", on_off(cpu.b("VirtualizationFirmware"))),
            ],
        );
    });
    section(ui, "Memory");
    card(ui, |ui| {
        let sticks = mem.a("Sticks");
        kv(
            ui,
            "pc-mem",
            &[
                ("Installed", num(mem.f("TotalGB"), " GB")),
                ("Slots", mem.i("Slots").map(|n| format!("{} of {n} used", sticks.len()))),
                ("Most the board supports", num(mem.f("MaxSupportedGB"), " GB")),
            ],
        );
        ui.add_space(S2);
        let rows: Vec<Vec<String>> = sticks
            .iter()
            .map(|m| {
                vec![
                    s(m, "Slot"),
                    num(m.f("SizeGB"), " GB").unwrap_or_default(),
                    s(m, "Type"),
                    num(m.f("SpeedMTs"), " MT/s").unwrap_or_default(),
                    num(m.f("ConfiguredMTs"), " MT/s").unwrap_or_default(),
                    format!("{} {}", s(m, "Maker"), s(m, "Part")),
                ]
            })
            .collect();
        table(ui, "sticks", &["Slot", "Size", "Type", "Rated", "Running at", "Maker and part"], &rows);
    });
    section(ui, "Graphics and displays");
    card(ui, |ui| {
        let g: Vec<Vec<String>> = sys
            .a("gpus")
            .iter()
            .map(|g| {
                vec![
                    s(g, "Name"),
                    num(g.f("VramMB").map(|v| v / 1024.0), " GB").unwrap_or_default(),
                    s(g, "Driver"),
                    s(g, "DriverDate"),
                    g.i("Width")
                        .filter(|w| *w > 0)
                        .map(|w| format!("{w} x {} at {} Hz", g.i("Height").unwrap_or(0), g.i("RefreshHz").unwrap_or(0)))
                        .unwrap_or_default(),
                ]
            })
            .collect();
        table(ui, "gpus", &["Graphics", "Memory", "Driver", "Driver date", "Current mode"], &g);
        ui.add_space(S3);
        let d: Vec<Vec<String>> = sys
            .a("displays")
            .iter()
            .map(|d| vec![s(d, "Name"), num(d.f("DiagonalIn"), " inch").unwrap_or_default(), s(d, "Connection"), s(d, "Year")])
            .collect();
        table(ui, "displays", &["Display", "Size", "Connection", "Made in"], &d);
        let dx = sys.g("directx");
        if !dx.is_null() {
            ui.add_space(S3);
            kv(
                ui,
                "pc-dx",
                &[("DirectX", dx.s("DirectX")), ("Driver model", dx.s("WDDM")), ("HDR", dx.s("HDR")), ("Hardware scheduling", dx.s("HWScheduling"))],
            );
        }
    });
    let bat = sys.g("battery");
    if !bat.is_null() {
        section(ui, "Battery");
        card(ui, |ui| {
            if let (Some(f), Some(d)) = (bat.f("FullChargemWh"), bat.f("DesignmWh")) {
                if d > 0.0 {
                    let pct = (f / d) as f32;
                    let p = pal(ui.ctx());
                    ui.label(format!("Holds {:.0}% of its original charge", pct * 100.0));
                    widgets::bar(
                        ui,
                        pct,
                        if pct < 0.5 {
                            p.red
                        } else if pct < 0.7 {
                            p.amber
                        } else {
                            p.green
                        },
                    );
                    ui.add_space(S2);
                }
            }
            kv(
                ui,
                "pc-bat",
                &[("Battery", bat.s("Name")), ("Maker", bat.s("Maker")), ("Charge cycles", bat.s("Cycles")), ("Charge now", num(bat.f("ChargePct"), "%"))],
            );
        });
    }
    section(ui, "Other devices");
    card(ui, |ui| {
        kv(
            ui,
            "pc-other",
            &[
                ("Sound", Some(sys.strs("audio").join(", "))),
                ("Cameras", Some(sys.strs("cameras").join(", "))),
                ("Keyboard and mouse", Some(sys.strs("input").join(", "))),
                ("Bluetooth", Some(sys.strs("bluetooth").join(", "))),
                ("USB", Some(sys.strs("usbControllers").join(", "))),
                ("Power plan", sys.g("power").s("Plan")),
            ],
        );
    });
    let bad: Vec<Vec<String>> = sys.a("problemDevices").iter().map(|d| vec![s(d, "Name"), s(d, "Class"), s(d, "Code")]).collect();
    if !bad.is_empty() {
        section(ui, "Devices with problems");
        card(ui, |ui| table(ui, "bad", &["Device", "Kind", "Problem code"], &bad));
    }
}

fn storage(ui: &mut Ui, snap: &Snapshot) {
    let p = pal(ui.ctx());
    section(ui, "Drives");
    if snap.drives.is_empty() {
        muted(ui, "Windows didn't report any drives.");
    }
    for d in &snap.drives {
        let disk = snap.storage.a("disks").into_iter().find(|x| x.i("Number") == Some(d.disk)).cloned().unwrap_or(Value::Null);
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&d.model).text_style(theme::h2()));
                let health = disk.s("Health").unwrap_or_else(|| "Unknown".into());
                let (fg, bg) = if health == "Healthy" { (p.green, p.green_bg) } else { (p.red, p.red_bg) };
                widgets::pill(ui, &health, fg, bg);
            });
            small(
                ui,
                format!(
                    "{} · {}{}",
                    d.kind,
                    num(disk.f("SizeGB"), " GB").unwrap_or_default(),
                    if disk.b("IsSystem") == Some(true) { " · Windows is on this drive" } else { "" }
                ),
            );
            ui.add_space(S2);
            kv(
                ui,
                &format!("drv-{}", d.disk),
                &[
                    ("Life used", d.life_used_pct.map(|w| format!("{w:.0}%"))),
                    ("Spare space left", d.spare_pct.map(|w| format!("{w:.0}%"))),
                    ("Temperature", d.temp_c.map(|t| format!("{t:.0} °C"))),
                    ("Powered on", d.power_on_hours.map(hours)),
                    ("Data written", d.tb_written.map(|t| format!("{t:.1} TB"))),
                    ("Sudden power losses", d.unsafe_shutdowns.map(|n| format!("{n:.0}"))),
                    ("Replaced damaged spots", d.reallocated.map(|n| format!("{n:.0}"))),
                    ("Spots waiting to be replaced", d.pending.map(|n| format!("{n:.0}"))),
                    ("Errors it couldn't fix", d.uncorrectable.map(|n| format!("{n:.0}"))),
                    ("Media errors", d.media_errors.map(|n| format!("{n:.0}"))),
                    ("Firmware", disk.s("Firmware")),
                    ("Read from", Some(d.source.clone())),
                ],
            );
            if !d.attributes.is_empty() {
                ui.add_space(S2);
                ui.collapsing("All SMART values", |ui| {
                    let rows: Vec<Vec<String>> = d
                        .attributes
                        .iter()
                        .map(|a| vec![a.id.to_string(), a.name.clone(), a.value.to_string(), a.worst.to_string(), a.raw.to_string()])
                        .collect();
                    table(ui, &format!("smart-{}", d.disk), &["ID", "Attribute", "Value", "Worst", "Raw"], &rows);
                });
            }
        });
        ui.add_space(S2);
    }
    section(ui, "Free space");
    card(ui, |ui| {
        for v in snap.storage.a("volumes") {
            let (Some(size), Some(free)) = (v.f("SizeGB"), v.f("FreeGB")) else { continue };
            if size <= 0.0 {
                continue;
            }
            let used = ((size - free) / size) as f32;
            let name = match (v.s("Letter"), v.s("Label")) {
                (Some(l), Some(n)) => format!("{l} {n}"),
                (Some(l), None) => l,
                (None, Some(n)) => n,
                (None, None) => "Hidden partition".into(),
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(&name).family(theme::semibold()));
                muted(ui, format!("{free:.0} GB free of {size:.0} GB"));
                if let Some(b) = v.s("BitLocker").filter(|b| b.starts_with("On")) {
                    small(ui, format!("· encrypted ({b})"));
                }
            });
            widgets::bar(
                ui,
                used,
                if used > 0.95 {
                    p.red
                } else if used > 0.9 {
                    p.amber
                } else {
                    p.accent
                },
            );
            ui.add_space(S3);
        }
    });
    let big: Vec<Vec<String>> = snap.storage.a("bigFolders").iter().map(|b| vec![s(b, "Path"), num(b.f("SizeGB"), " GB").unwrap_or_default()]).collect();
    if !big.is_empty() {
        section(ui, "Folders that tend to fill up");
        card(ui, |ui| table(ui, "big", &["Folder", "Size"], &big));
    }
}

fn network(ui: &mut Ui, snap: &Snapshot) {
    let n = &snap.network;
    section(ui, "Adapters");
    for a in n.a("adapters") {
        card(ui, |ui| {
            ui.label(RichText::new(s(a, "Name")).text_style(theme::h2()));
            small(ui, s(a, "Device"));
            ui.add_space(S2);
            kv(
                ui,
                &format!("ad-{}", s(a, "Name")),
                &[
                    ("Status", a.s("Status")),
                    ("Speed", a.s("Speed")),
                    ("Address", Some(a.strs("IPv4").join(", "))),
                    ("Router", Some(a.strs("Gateway").join(", "))),
                    ("DNS servers", Some(a.strs("DNS").join(", "))),
                    ("Hardware address", a.s("MAC")),
                    ("Driver", a.s("DriverVersion").map(|v| format!("{v} from {}, dated {}", s(a, "DriverProvider"), s(a, "DriverDate")))),
                ],
            );
        });
        ui.add_space(S2);
    }
    let w = n.g("wifi");
    if !w.is_null() {
        section(ui, "Wi-Fi");
        card(ui, |ui| {
            kv(
                ui,
                "wifi",
                &[
                    ("Network", w.s("SSID")),
                    ("Signal", w.s("Signal")),
                    ("Standard", w.s("Radio")),
                    ("Band and channel", w.s("Band").map(|b| format!("{b}, channel {}", s(w, "Channel")))),
                    ("Speed", w.s("RxMbps").map(|r| format!("{r} Mbps down, {} Mbps up", s(w, "TxMbps")))),
                    ("Security", w.s("Auth")),
                    ("Supports WPA3", yes_no(w.b("WPA3"))),
                    ("Adapter supports", w.s("RadiosSupported")),
                ],
            );
        });
    }
    section(ui, "Accepts connections from the network");
    card(ui, |ui| {
        muted(ui, "These apps are listening for connections from other devices. The firewall still decides who can reach them.");
        let rows: Vec<Vec<String>> = n.a("listening").iter().map(|l| vec![s(l, "Port"), s(l, "Process"), s(l, "Path")]).collect();
        table(ui, "ports", &["Port", "App", "File"], &rows);
    });
    let v = n.strs("virtualAdapters");
    if !v.is_empty() {
        section(ui, "Virtual adapters");
        card(ui, |ui| {
            muted(ui, "Created by VPNs, virtual machines and similar software.");
            for x in v {
                ui.label(x);
            }
        });
    }
}

fn about(ui: &mut Ui) {
    card(ui, |ui| {
        ui.label(RichText::new(format!("rigscan {}", env!("CARGO_PKG_VERSION"))).text_style(theme::h2()));
        ui.add_space(S2);
        ui.add(egui::Label::new("rigscan checks a Windows PC and explains, in plain words, what's worth fixing. It reads what Windows already knows about itself, runs no background service, and leaves no trace beyond its own saved checks.").wrap());
        ui.add_space(S2);
        ui.add(
            egui::Label::new("Every check is a small, readable script built into the app, and every fix shows exactly what it changes before it runs.").wrap(),
        );
        ui.add_space(S3);
        small(ui, "MIT licence. Built with Rust and egui.");
    });
    ui.add_space(S5);
}
