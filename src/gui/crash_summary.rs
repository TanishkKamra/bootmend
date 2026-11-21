//! What reading the crash files found: the line on the Crash files card, and the popup that
//! opens when reading finishes.

use super::theme::{self, S1, S2, S3, S4, S5};
use super::widgets::{self, muted, small};
use super::App;
use crate::collect::dumps::{module_hint, windows_core};
use crate::rules::plural;
use crate::snapshot::{DumpAnalysis, Snapshot};
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

    // Each driver once, in the order the crashes came (newest first).
    let mut seen: Vec<(String, Option<String>, usize)> = vec![];
    for d in read {
        let file = d.driver_file().unwrap_or_default();
        match seen.iter_mut().find(|g| g.0.eq_ignore_ascii_case(&file)) {
            Some(g) => g.2 += 1,
            None => {
                let note = d.note.clone().or_else(|| module_hint(&file).map(String::from));
                seen.push((file, note, 1));
            }
        }
    }
    // Most crashes first. Among equals, drivers with a plain name come before ones without,
    // and a crash naming no file comes last.
    seen.sort_by_key(|g| (Reverse(g.2), g.0.is_empty(), g.1.is_none()));

    let core = |f: &str| !f.is_empty() && windows_core(f);
    let verdict = if total == 1 {
        let f = seen[0].0.as_str();
        if core(f) {
            Some(format!("This crash points at Windows itself. {NO_TRACE}"))
        } else if f.is_empty() {
            None
        } else {
            Some(format!("One crash isn't enough to be sure, but it points at {f}."))
        }
    } else if seen.iter().all(|g| core(&g.0)) {
        Some(format!("Every crash points at Windows itself. {NO_TRACE}"))
    } else {
        // A driver stands out when it crashed more often than anything else, Windows included.
        let top = seen.iter().find(|g| !g.0.is_empty() && !core(&g.0)).filter(|t| t.2 >= 2 && seen.iter().all(|g| g.0 == t.0 || g.2 < t.2));
        Some(match top {
            Some(t) => format!("{} was in {} of the {total} crashes, so it's the likeliest cause.", t.0, t.2),
            None => "No single driver stands out. When crashes point at different drivers, memory or other hardware is often the cause.".into(),
        })
    };

    let more = seen.len().saturating_sub(SHOWN);
    let groups = seen
        .into_iter()
        .take(SHOWN)
        .map(|(file, note, crashes)| Group {
            label: match (&note, file.is_empty()) {
                (_, true) => "No driver named".into(),
                // "Windows core (the real cause is often...)" is "Windows core" here; the verdict says the rest.
                (Some(n), _) => n.split(" (").next().unwrap_or(n).to_string(),
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
        let mut close = false;
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
            });
        });
        if close || m.should_close() {
            log!("dialog", "crash file summary closed");
            self.crash_summary = None;
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
        // Known drivers come before one Bootmend can't name when they crashed as often.
        assert_eq!(
            s.groups,
            [group("Windows core", "ntkrnlmp.exe", 2), group("Game anti-cheat driver", "vgk.sys", 1), group("A driver Bootmend doesn't know", "bfs.sys", 1)]
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

    #[test]
    fn the_card_says_how_reading_went() {
        assert_eq!(card_line(5, 0), (true, "Read 5 crash files. The table below shows the driver involved in each crash.".into()));
        assert_eq!(card_line(4, 1), (true, "Read 4 of 5 crash files. One couldn't be read. It may be damaged.".into()));
        assert_eq!(card_line(1, 2), (true, "Read 1 of 3 crash files. 2 couldn't be read. They may be damaged.".into()));
        assert_eq!(card_line(0, 1), (false, "Couldn't read the crash file. It may be damaged.".into()));
        assert_eq!(card_line(0, 4), (false, "Couldn't read the crash files. They may be damaged.".into()));
    }
}
