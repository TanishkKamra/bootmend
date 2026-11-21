//! What reading the crash files found: the line on the Crash files card, and the popup that
//! opens when reading finishes.

use super::icon;
use super::theme::{self, S1, S2, S3, S4, S5};
use super::widgets::{self, muted, small};
use super::{App, Page};
use crate::collect::dumps::{driver_info, module_hint, windows_core, Kind};
use crate::rules::{plural, Action};
use crate::snapshot::{DumpAnalysis, Snapshot};
use crate::util::shell_open;
use eframe::egui::{self, RichText};
use std::cmp::Reverse;

/// The popup lists this many drivers; more would be a lot to take in.
const SHOWN: usize = 3;
const NO_TRACE: &str = "That usually means memory, other hardware, or a driver that left no trace.";

/// The popup's content.
#[derive(Debug, PartialEq)]
pub struct Summary {
    pub title: String,
    pub lead: String,
    /// The drivers involved, most crashes first. At most `SHOWN`.
    pub groups: Vec<Group>,
    /// Drivers left out of `groups`.
    pub more: usize,
    pub verdict: Option<String>,
}

#[derive(Debug, PartialEq)]
pub struct Group {
    pub label: String,
    pub file: String,
    pub crashes: usize,
}

fn may_be_damaged(n: usize) -> &'static str {
    if n == 1 {
        "It may be damaged."
    } else {
        "They may be damaged."
    }
}

fn not_read(n: usize) -> String {
    let who = if n == 1 { "One".to_string() } else { n.to_string() };
    format!("{who} couldn't be read. {}", may_be_damaged(n))
}

fn files_word(n: usize) -> &'static str {
    if n == 1 {
        "file"
    } else {
        "files"
    }
}

/// A driver's plain name without the explanation in brackets.
fn short(name: &str) -> &str {
    name.split(" (").next().unwrap_or(name)
}

/// One driver and how many crashes named it.
struct Seen {
    file: String,
    note: Option<String>,
    crashes: usize,
}

/// Each driver once, most crashes first. Among equals, drivers with a plain name come before
/// ones without, and a crash naming no file comes last.
fn drivers(read: &[DumpAnalysis]) -> Vec<Seen> {
    let mut seen: Vec<Seen> = vec![];
    for d in read {
        let file = d.driver_file().unwrap_or_default();
        match seen.iter_mut().find(|g| g.file.eq_ignore_ascii_case(&file)) {
            Some(g) => g.crashes += 1,
            None => {
                let note = d.note.clone().or_else(|| module_hint(&file).map(String::from));
                seen.push(Seen { file, note, crashes: 1 });
            }
        }
    }
    seen.sort_by_key(|g| (Reverse(g.crashes), g.file.is_empty(), g.note.is_none()));
    seen
}

/// What the crashes have in common.
enum Cause<'a> {
    /// They point at Windows itself, so the real cause left no trace.
    Windows,
    /// One driver was in more crashes than any other, or in the only one.
    Driver(&'a Seen),
    /// Different drivers, which often means memory or other hardware.
    Mixed,
    /// The only crash named no driver.
    Unnamed,
}

/// `seen` holds at least one driver, `total` the crashes read.
fn cause(seen: &[Seen], total: usize) -> Cause<'_> {
    let core = |f: &str| !f.is_empty() && windows_core(f);
    if total == 1 {
        let g = &seen[0];
        return if core(&g.file) {
            Cause::Windows
        } else if g.file.is_empty() {
            Cause::Unnamed
        } else {
            Cause::Driver(g)
        };
    }
    if seen.iter().all(|g| core(&g.file)) {
        return Cause::Windows;
    }
    // A driver stands out when it crashed more often than anything else, Windows included.
    let top =
        seen.iter().find(|g| !g.file.is_empty() && !core(&g.file)).filter(|t| t.crashes >= 2 && seen.iter().all(|g| g.file == t.file || g.crashes < t.crashes));
    top.map_or(Cause::Mixed, Cause::Driver)
}

/// `read` is every crash file read so far, `unread` how many the last try couldn't read.
pub fn summarize(read: &[DumpAnalysis], unread: usize) -> Summary {
    if read.is_empty() {
        return Summary {
            title: format!("Couldn't read the crash {}", files_word(unread)),
            lead: format!("{} You can try again from the Crashes page.", may_be_damaged(unread)),
            groups: vec![],
            more: 0,
            verdict: None,
        };
    }
    let total = read.len();
    let lead = if unread == 0 {
        format!("Bootmend read {}.", plural(total, "crash file", "crash files"))
    } else {
        format!("Bootmend read {total} of the {} crash files. {}", total + unread, not_read(unread))
    };

    let seen = drivers(read);
    let verdict = match (cause(&seen, total), total) {
        (Cause::Windows, 1) => Some(format!("This crash points at Windows itself. {NO_TRACE}")),
        (Cause::Windows, _) => Some(format!("Every crash points at Windows itself. {NO_TRACE}")),
        (Cause::Driver(g), 1) => Some(format!("One crash isn't enough to be sure, but it points at {}.", g.file)),
        (Cause::Driver(g), _) => Some(format!("{} was in {} of the {total} crashes, so it's the likeliest cause.", g.file, g.crashes)),
        (Cause::Mixed, _) => Some("No single driver stands out. When crashes point at different drivers, memory or other hardware is often the cause.".into()),
        (Cause::Unnamed, _) => None,
    };

    let more = seen.len().saturating_sub(SHOWN);
    let groups = seen
        .into_iter()
        .take(SHOWN)
        .map(|Seen { file, note, crashes }| Group {
            label: match (&note, file.is_empty()) {
                (_, true) => "No driver named".into(),
                // "Windows core (the real cause is often...)" is "Windows core" here; the verdict says the rest.
                (Some(n), _) => short(n).to_string(),
                (None, _) => "A driver Bootmend doesn't know".into(),
            },
            file,
            crashes,
        })
        .collect();
    Summary { title: "What the crash files say".into(), lead, groups, more, verdict }
}

/// The card's line once the files have been read: whether it went well, and what to say.
pub fn card_line(read: usize, unread: usize) -> (bool, String) {
    if read == 0 {
        return (false, format!("Couldn't read the crash {}. {}", files_word(unread), may_be_damaged(unread)));
    }
    if unread == 0 {
        return (true, format!("Read {}. The table below shows the driver involved in each crash.", plural(read, "crash file", "crash files")));
    }
    (true, format!("Read {read} of {} crash files. {}", read + unread, not_read(unread)))
}

/// How to stop the crashes, from what they have in common.
#[derive(Debug, PartialEq)]
pub struct Plan {
    pub title: String,
    /// The likeliest cause, in a few words.
    pub cause: String,
    pub why: String,
    pub steps: Vec<Step>,
}

#[derive(Debug, PartialEq)]
pub struct Step {
    pub text: String,
    /// A button that helps with this step.
    pub action: Option<Action>,
}

fn step(text: impl Into<String>) -> Step {
    Step { text: text.into(), action: None }
}

fn step_with(text: &str, action: Action) -> Step {
    Step { text: text.into(), action: Some(action) }
}

fn windows_update(text: &str) -> Step {
    step_with(text, Action::Open { label: "Open Windows Update", target: "ms-settings:windowsupdate".into() })
}

fn check_drive() -> Step {
    step_with("Check the drive's health.", Action::Goto(Page::Storage))
}

fn hardware_steps() -> Vec<Step> {
    vec![
        step_with(
            "Test the memory. Windows Memory Diagnostic restarts the PC and checks it.",
            Action::Open { label: "Open Memory Diagnostic", target: "mdsched.exe".into() },
        ),
        step("If the processor or memory is overclocked, or a memory profile (XMP, EXPO or DOCP) is on, set them back to default in the BIOS."),
        check_drive(),
    ]
}

/// Steps for crashes that point at `file`, which `info` describes when Bootmend knows it.
fn driver_steps(info: Option<(Kind, &str)>, file: &str) -> Vec<Step> {
    let board = "the PC or motherboard maker's website";
    match info {
        Some((Kind::Graphics, name)) => {
            let maker = ["NVIDIA", "AMD", "Intel"].into_iter().find(|m| name.starts_with(m)).unwrap_or("NVIDIA, AMD or Intel");
            vec![
                step(format!("Install the newest graphics driver from {maker}'s website, not through Windows Update.")),
                step("If the graphics card is overclocked, set it back to default."),
            ]
        }
        Some((Kind::AntiCheat, _)) => vec![
            step("Update the game that uses it. If that doesn't help, reinstall the game."),
            step("If crashes carry on, uninstall the game until its maker fixes the problem."),
        ],
        Some((Kind::Network, _)) => vec![
            step("Install the newest network driver from the PC maker's website, or from the maker of its Wi-Fi or network card."),
            windows_update("Install Windows updates."),
        ],
        Some((Kind::Storage, _)) => vec![check_drive(), step(format!("Install the newest storage and chipset drivers from {board}."))],
        Some((Kind::Usb, _)) => {
            vec![step("Unplug USB devices you don't need, and see if crashes stop."), step(format!("Install the newest chipset driver from {board}."))]
        }
        Some((Kind::Audio, _)) => vec![step(format!("Install the newest audio driver from {board}.")), windows_update("Install Windows updates.")],
        Some((Kind::Defender, _)) => vec![
            windows_update("Install Windows updates. They keep Microsoft Defender up to date."),
            step("If you have another antivirus program as well, update it or uninstall it."),
        ],
        Some((Kind::Sandbox, _)) => vec![
            windows_update("Install Windows updates. Microsoft fixes problems in its own files through them."),
            step("If the crashes began after you installed a program, uninstall it and see if they stop."),
        ],
        Some((Kind::Core, _)) | None => vec![
            step(format!("Search the web for {file} to find which program or device it belongs to.")),
            step("Update that program, or the device's driver from its maker's website. If you don't need the program, uninstall it."),
            windows_update("Install Windows updates."),
        ],
    }
}

/// `None` until at least one crash file has been read.
pub fn plan(read: &[DumpAnalysis]) -> Option<Plan> {
    if read.is_empty() {
        return None;
    }
    let total = read.len();
    let seen = drivers(read);
    let hardware = "Memory or other hardware".to_string();
    let (cause, why, steps) = match cause(&seen, total) {
        Cause::Windows => {
            let what = if total == 1 { "The crash points" } else { "The crashes point" };
            (hardware, format!("{what} at Windows itself, which usually means the real cause left no trace."), hardware_steps())
        }
        Cause::Unnamed => (hardware, "The crash file didn't name a driver, so memory or other hardware is the likeliest cause.".into(), hardware_steps()),
        Cause::Mixed => {
            let mut steps = hardware_steps();
            steps.push(step("Update the graphics, network and chipset drivers from the makers' websites."));
            (hardware, "The crashes point at different drivers. When that happens, memory or other hardware is often the cause.".into(), steps)
        }
        Cause::Driver(g) => {
            let info = driver_info(&g.file);
            let cause = match info {
                Some((_, name)) => format!("{} ({})", short(name), g.file),
                None => format!("{}, a driver Bootmend doesn't know", g.file),
            };
            let why = if total == 1 {
                "One crash isn't enough to be sure, but it points at this driver.".to_string()
            } else {
                format!("It was in {} of the {total} crashes.", g.crashes)
            };
            (cause, why, driver_steps(info, &g.file))
        }
    };
    let title = if total == 1 { "How to stop this crash" } else { "How to stop these crashes" }.to_string();
    Some(Plan { title, cause, why, steps })
}

/// `snap` after reading five crash files, one of them unreadable, for screenshots. Nothing is saved.
pub fn sample(snap: &Snapshot) -> Snapshot {
    let mut s = snap.clone();
    let read = [
        ("bfs.sys", "2026-11-14 15:37", 0x3B),
        ("vgk.sys", "2026-11-14 14:42", 0x7E),
        ("ntkrnlmp.exe", "2026-11-14 06:47", 0xA),
        ("ntkrnlmp.exe", "2026-11-01 21:06", 0x50),
    ];
    s.dumps = read
        .iter()
        .enumerate()
        .map(|(i, (image, date, code))| DumpAnalysis {
            file: format!(r"C:\WINDOWS\Minidump\sample-{i}.dmp"),
            date: date.to_string(),
            code: format!("0x{code:X}"),
            name: crate::collect::dumps::bugcheck_info(*code).0.into(),
            image: Some(image.to_string()),
            note: module_hint(image).map(String::from),
            ..Default::default()
        })
        .collect();
    s.dumps_unread = 1;
    let listed: Vec<_> = (0..5).map(|i| serde_json::json!({ "Path": format!(r"C:\WINDOWS\Minidump\sample-{i}.dmp") })).collect();
    if let Some(h) = s.health.as_object_mut() {
        h.insert("minidumps".into(), listed.into());
    }
    s
}

impl App {
    /// Says what reading the crash files found. It waits while another dialog is open.
    pub(super) fn crash_summary_dialog(&mut self, ctx: &egui::Context) {
        if self.early_notice || self.confirm.is_some() || self.copy.is_some() {
            return;
        }
        let Some(s) = &self.crash_summary else { return };
        let (mut close, mut fix) = (false, false);
        let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::same(S5 as i8));
        let m = egui::Modal::new(egui::Id::new("crash-summary")).frame(frame).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 2.0 * (S5 + S4)).min(440.0));
            let t = ui.label(RichText::new(&s.title).text_style(theme::h2()));
            widgets::heading_role(ui, &t, 2, &s.title);
            ui.add_space(S2);
            ui.add(egui::Label::new(&s.lead).wrap());
            if !s.groups.is_empty() {
                ui.add_space(S4);
                for g in &s.groups {
                    // A driver's name and file sit close, so each pair reads as one.
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = S1;
                        // Rows default to button height; text alone needs less.
                        ui.spacing_mut().interact_size.y = 0.0;
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&g.label).family(theme::semibold()));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| muted(ui, plural(g.crashes, "crash", "crashes")));
                        });
                        if !g.file.is_empty() {
                            small(ui, &g.file);
                        }
                    });
                    ui.add_space(S3);
                }
                if s.more > 0 {
                    muted(ui, format!("And {} more. The Crashes page lists them all.", plural(s.more, "driver", "drivers")));
                    ui.add_space(S2);
                }
            }
            if let Some(v) = &s.verdict {
                ui.add_space(S2);
                ui.add(egui::Label::new(v).wrap());
            }
            ui.add_space(S4);
            widgets::right_row(ui, |ui| {
                let ok = widgets::primary(ui, "Got it");
                if ui.memory(|m| m.focused().is_none()) {
                    ok.request_focus();
                }
                close = ok.clicked();
                fix = !s.groups.is_empty() && widgets::secondary(ui, "How to fix").clicked();
            });
        });
        if fix {
            log!("dialog", "crash file summary: how to fix");
            self.crash_summary = None;
            self.crash_fix = self.snap.as_ref().and_then(|s| plan(&s.dumps));
        } else if close || m.should_close() {
            log!("dialog", "crash file summary closed");
            self.crash_summary = None;
        }
    }

    /// Steps for the likeliest cause of the crashes. It waits while another dialog is open.
    pub(super) fn crash_fix_dialog(&mut self, ctx: &egui::Context) {
        if self.early_notice || self.confirm.is_some() || self.copy.is_some() || self.crash_summary.is_some() {
            return;
        }
        let Some(plan) = &self.crash_fix else { return };
        let (mut close, mut chosen) = (false, None);
        let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::same(S5 as i8));
        let m = egui::Modal::new(egui::Id::new("crash-fix")).frame(frame).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 2.0 * (S5 + S4)).min(440.0));
            let t = ui.label(RichText::new(&plan.title).text_style(theme::h2()));
            widgets::heading_role(ui, &t, 2, &plan.title);
            ui.add_space(S3);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = S1;
                small(ui, "Likeliest cause");
                ui.add(egui::Label::new(RichText::new(&plan.cause).family(theme::semibold())).wrap());
                muted(ui, &plan.why);
            });
            ui.add_space(S4);
            for (i, st) in plan.steps.iter().enumerate() {
                ui.horizontal_top(|ui| {
                    ui.label(RichText::new(format!("{}.", i + 1)).family(theme::semibold()));
                    ui.vertical(|ui| {
                        ui.add(egui::Label::new(&st.text).wrap());
                        let clicked = match &st.action {
                            Some(Action::Open { label, .. }) => widgets::icon_button(ui, icon::OPEN, label, true).clicked(),
                            Some(Action::Goto(pg)) => widgets::secondary(ui, &format!("Go to {}", pg.title())).clicked(),
                            _ => false,
                        };
                        if clicked {
                            chosen = st.action.clone();
                        }
                    });
                });
                ui.add_space(S2);
            }
            ui.add_space(S2);
            widgets::right_row(ui, |ui| {
                let ok = widgets::primary(ui, "Got it");
                if ui.memory(|m| m.focused().is_none()) {
                    ok.request_focus();
                }
                close = ok.clicked();
            });
        });
        match chosen {
            Some(Action::Open { target, .. }) => {
                log!("action", "opening {target}");
                if let Err(e) = shell_open(&target) {
                    self.toast(format!("{e:#}"), false);
                }
            }
            Some(Action::Goto(pg)) => {
                self.crash_fix = None;
                self.go(pg);
            }
            _ => {}
        }
        if close || m.should_close() {
            log!("dialog", "crash fix steps closed");
            self.crash_fix = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collect::dumps::module_hint;

    fn dump(image: &str) -> DumpAnalysis {
        DumpAnalysis { image: Some(image.into()), note: module_hint(image).map(String::from), ..Default::default() }
    }

    fn group(label: &str, file: &str, crashes: usize) -> Group {
        Group { label: label.into(), file: file.into(), crashes }
    }

    #[test]
    fn different_drivers_mean_none_stands_out() {
        // The crashes on the PC this was reported from, newest first.
        let read = [dump("bfs.sys"), dump("vgk.sys"), dump("ntkrnlmp.exe"), dump("ntkrnlmp.exe")];
        let s = summarize(&read, 1);
        assert_eq!(s.title, "What the crash files say");
        assert_eq!(s.lead, "Bootmend read 4 of the 5 crash files. One couldn't be read. It may be damaged.");
        assert_eq!(
            s.groups,
            [group("Windows core", "ntkrnlmp.exe", 2), group("Windows app sandbox", "bfs.sys", 1), group("Game anti-cheat driver", "vgk.sys", 1)]
        );
        assert_eq!(s.more, 0);
        assert_eq!(
            s.verdict.as_deref(),
            Some("No single driver stands out. When crashes point at different drivers, memory or other hardware is often the cause.")
        );
    }

    #[test]
    fn a_driver_in_most_crashes_is_the_likeliest_cause() {
        let s = summarize(&[dump("nvlddmkm.sys"), dump("ntkrnlmp.exe"), dump("nvlddmkm.sys"), dump("nvlddmkm.sys")], 0);
        assert_eq!(s.lead, "Bootmend read 4 crash files.");
        assert_eq!(s.groups[0], group("NVIDIA graphics driver", "nvlddmkm.sys", 3));
        assert_eq!(s.verdict.as_deref(), Some("nvlddmkm.sys was in 3 of the 4 crashes, so it's the likeliest cause."));
    }

    #[test]
    fn known_drivers_come_before_one_bootmend_cant_name_when_they_crashed_as_often() {
        let s = summarize(&[dump("xyzdrv.sys"), dump("vgk.sys")], 0);
        assert_eq!(s.groups, [group("Game anti-cheat driver", "vgk.sys", 1), group("A driver Bootmend doesn't know", "xyzdrv.sys", 1)]);
    }

    #[test]
    fn two_drivers_crashing_as_often_name_neither() {
        let s = summarize(&[dump("nvlddmkm.sys"), dump("vgk.sys"), dump("nvlddmkm.sys"), dump("vgk.sys")], 0);
        assert!(s.verdict.unwrap().starts_with("No single driver stands out."));
    }

    #[test]
    fn crashes_that_only_name_windows_point_past_it() {
        let s = summarize(&[dump("ntkrnlmp.exe"), dump("ntoskrnl.exe")], 0);
        assert_eq!(
            s.verdict.as_deref(),
            Some("Every crash points at Windows itself. That usually means memory, other hardware, or a driver that left no trace.")
        );
    }

    #[test]
    fn one_crash_is_named_without_certainty() {
        let s = summarize(&[dump("vgk.sys")], 0);
        assert_eq!(s.lead, "Bootmend read 1 crash file.");
        assert_eq!(s.verdict.as_deref(), Some("One crash isn't enough to be sure, but it points at vgk.sys."));
        let s = summarize(&[dump("ntkrnlmp.exe")], 2);
        assert_eq!(s.lead, "Bootmend read 1 of the 3 crash files. 2 couldn't be read. They may be damaged.");
        assert_eq!(
            s.verdict.as_deref(),
            Some("This crash points at Windows itself. That usually means memory, other hardware, or a driver that left no trace.")
        );
    }

    #[test]
    fn nothing_read_says_so_and_lists_nothing() {
        let s = summarize(&[], 1);
        assert_eq!(s.title, "Couldn't read the crash file");
        assert_eq!(s.lead, "It may be damaged. You can try again from the Crashes page.");
        assert!(s.groups.is_empty() && s.verdict.is_none());
        assert_eq!(summarize(&[], 3).title, "Couldn't read the crash files");
        assert_eq!(summarize(&[], 3).lead, "They may be damaged. You can try again from the Crashes page.");
    }

    #[test]
    fn only_three_drivers_are_listed() {
        let read = [dump("a.sys"), dump("b.sys"), dump("b.sys"), dump("c.sys"), dump("d.sys"), dump("e.sys")];
        let s = summarize(&read, 0);
        assert_eq!(s.groups.len(), 3);
        assert_eq!(s.groups[0].file, "b.sys");
        assert_eq!(s.more, 2);
    }

    #[test]
    fn a_crash_naming_no_file_is_grouped_as_unnamed() {
        let s = summarize(&[DumpAnalysis::default(), DumpAnalysis { module: Some("vgk".into()), ..Default::default() }], 0);
        // A file named only by its module still gets its plain name.
        assert_eq!(s.groups, [group("Game anti-cheat driver", "vgk.sys", 1), group("No driver named", "", 1)]);
    }

    fn texts(p: &Plan) -> Vec<&str> {
        p.steps.iter().map(|s| s.text.as_str()).collect()
    }

    fn opens(p: &Plan, target: &str) -> bool {
        p.steps.iter().any(|s| matches!(&s.action, Some(Action::Open { target: t, .. }) if t == target))
    }

    #[test]
    fn crashes_pointing_at_windows_lead_to_the_hardware() {
        let p = plan(&[dump("ntkrnlmp.exe"), dump("ntoskrnl.exe")]).unwrap();
        assert_eq!(p.title, "How to stop these crashes");
        assert_eq!(p.cause, "Memory or other hardware");
        assert_eq!(p.why, "The crashes point at Windows itself, which usually means the real cause left no trace.");
        assert!(opens(&p, "mdsched.exe"));
        assert!(p.steps.iter().any(|s| s.action == Some(Action::Goto(Page::Storage))));
        assert!(texts(&p)[1].contains("XMP"));
        // One crash gets the singular.
        let p = plan(&[dump("ntkrnlmp.exe")]).unwrap();
        assert_eq!(p.title, "How to stop this crash");
        assert_eq!(p.why, "The crash points at Windows itself, which usually means the real cause left no trace.");
    }

    #[test]
    fn mixed_drivers_add_driver_updates_to_the_hardware_steps() {
        let p = plan(&[dump("bfs.sys"), dump("vgk.sys"), dump("ntkrnlmp.exe"), dump("ntkrnlmp.exe")]).unwrap();
        assert_eq!(p.cause, "Memory or other hardware");
        assert_eq!(p.why, "The crashes point at different drivers. When that happens, memory or other hardware is often the cause.");
        assert!(opens(&p, "mdsched.exe"));
        assert_eq!(texts(&p).last().copied(), Some("Update the graphics, network and chipset drivers from the makers' websites."));
    }

    #[test]
    fn a_graphics_driver_that_stands_out_is_updated_from_its_maker() {
        let p = plan(&[dump("nvlddmkm.sys"), dump("ntkrnlmp.exe"), dump("nvlddmkm.sys")]).unwrap();
        assert_eq!(p.cause, "NVIDIA graphics driver (nvlddmkm.sys)");
        assert_eq!(p.why, "It was in 2 of the 3 crashes.");
        assert_eq!(texts(&p)[0], "Install the newest graphics driver from NVIDIA's website, not through Windows Update.");
        // Windows' own graphics layer names no maker.
        let p = plan(&[dump("dxgkrnl.sys"), dump("dxgkrnl.sys")]).unwrap();
        assert_eq!(texts(&p)[0], "Install the newest graphics driver from NVIDIA, AMD or Intel's website, not through Windows Update.");
    }

    #[test]
    fn one_crash_names_its_driver_without_certainty() {
        let p = plan(&[dump("vgk.sys")]).unwrap();
        assert_eq!(p.cause, "Game anti-cheat driver (vgk.sys)");
        assert_eq!(p.why, "One crash isn't enough to be sure, but it points at this driver.");
        assert!(texts(&p)[0].starts_with("Update the game that uses it."));
    }

    #[test]
    fn a_driver_bootmend_doesnt_know_is_looked_up() {
        let p = plan(&[dump("xyzdrv.sys"), dump("xyzdrv.sys")]).unwrap();
        assert_eq!(p.cause, "xyzdrv.sys, a driver Bootmend doesn't know");
        assert_eq!(texts(&p)[0], "Search the web for xyzdrv.sys to find which program or device it belongs to.");
        assert!(opens(&p, "ms-settings:windowsupdate"));
    }

    #[test]
    fn the_windows_app_sandbox_is_mended_by_windows_updates() {
        let p = plan(&[dump("bfs.sys"), dump("bfs.sys")]).unwrap();
        assert_eq!(p.cause, "Windows app sandbox (bfs.sys)");
        assert_eq!(texts(&p)[0], "Install Windows updates. Microsoft fixes problems in its own files through them.");
        assert!(opens(&p, "ms-settings:windowsupdate"));
    }

    #[test]
    fn every_kind_of_driver_has_steps() {
        for f in ["nvlddmkm.sys", "tcpip.sys", "stornvme.sys", "wdfilter.sys", "bfs.sys", "vgk.sys", "usbxhci.sys", "portcls.sys", "xyzdrv.sys"] {
            let p = plan(&[dump(f), dump(f)]).unwrap();
            assert!((2..=4).contains(&p.steps.len()), "{f}: {:?}", texts(&p));
            assert!(p.cause.contains(f), "{f}: {}", p.cause);
        }
    }

    #[test]
    fn nothing_read_means_no_plan() {
        assert_eq!(plan(&[]), None);
    }

    #[test]
    fn the_card_says_how_reading_went() {
        assert_eq!(card_line(5, 0), (true, "Read 5 crash files. The table below shows the driver involved in each crash.".into()));
        assert_eq!(card_line(4, 1), (true, "Read 4 of 5 crash files. One couldn't be read. It may be damaged.".into()));
        assert_eq!(card_line(1, 2), (true, "Read 1 of 3 crash files. 2 couldn't be read. They may be damaged.".into()));
        assert_eq!(card_line(0, 1), (false, "Couldn't read the crash file. It may be damaged.".into()));
        assert_eq!(card_line(0, 4), (false, "Couldn't read the crash files. They may be damaged.".into()));
    }
}
