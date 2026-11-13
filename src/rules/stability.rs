use super::data::cpu_clocks;
use super::{plural, Action, Area, Confidence, Finding, Level};
use crate::collect::dumps::bugcheck_info;
use crate::gui::Page;
use crate::snapshot::Snapshot;
use crate::util::J;
use regex::Regex;
use std::collections::BTreeMap;

const STAB: Area = Area::Stability;

/// The stop code inside a WER 1001 message ("The bugcheck was: 0x000000d1 (...)").
pub fn bugcheck_code(text: &str) -> Option<u32> {
    let re = Regex::new(r"(?i)bug ?check was:?\s*0x([0-9a-f]+)").ok()?;
    re.captures(text).and_then(|c| u32::from_str_radix(&c[1], 16).ok())
}

pub fn hardware_stress_codes(code: u32) -> bool {
    matches!(code, 0x124 | 0x101 | 0x7F)
}

pub fn check(s: &Snapshot, f: &mut Vec<Finding>) {
    let h = &s.health;
    let mut codes: BTreeMap<u32, usize> = BTreeMap::new();
    for b in h.a("bugchecks") {
        if let Some(c) = b.s("Text").and_then(|t| bugcheck_code(&t)) {
            *codes.entry(c).or_default() += 1;
        }
    }
    let crashes: usize = codes.values().sum();
    let whea = h.a("whea").len();
    let stress = codes.keys().any(|c| hardware_stress_codes(*c));

    if crashes > 0 {
        let top = codes.iter().max_by_key(|(_, n)| **n).map(|(c, _)| *c).unwrap_or(0);
        let (name, meaning) = bugcheck_info(top);
        let mut x = Finding::new(
            "bsod",
            STAB,
            if crashes >= 3 { Level::DoFirst } else { Level::WorthDoing },
            format!("Windows crashed {} in the last 90 days", plural(crashes, "time", "times")),
        )
        .why(format!("These were blue-screen crashes. The most common one was {name}: {meaning}"))
        .details(codes.iter().map(|(c, n)| format!("0x{c:X} {}: {}", bugcheck_info(*c).0, plural(*n, "time", "times"))))
        .conf(Confidence::BestGuess);
        let blamed: Vec<String> = s.dumps.iter().filter_map(|d| d.note.clone()).collect();
        if let Some(n) = blamed.first() {
            x = x.step(format!("The crash files point to the {n}. Install the newest version from its maker's website, not through Windows Update."));
        } else if stress {
            x = x.step("These stop codes usually mean the hardware was pushed too hard. If the processor or memory is overclocked, or a memory profile (XMP, EXPO, DOCP) is on, set them back to default in the BIOS and see if the crashes stop.");
        } else if matches!(top, 0x1A | 0x50) {
            x = x.step("Test the memory: search Start for Windows Memory Diagnostic and let it restart the PC.").open("Open Memory Diagnostic", "mdsched.exe");
        } else if matches!(top, 0x116 | 0x117) {
            x = x.step("Install the newest graphics driver from NVIDIA, AMD or Intel's website.");
        } else if matches!(top, 0x7A | 0x77 | 0x154 | 0xF4) {
            x = x.step("Check the drive's health on the Storage page and back up your files.").act(Action::Goto(Page::Storage));
        } else {
            x = x.step("Update your drivers, starting with graphics, network and chipset, from the makers' websites.");
        }
        if !h.a("minidumps").is_empty() && s.dumps.is_empty() {
            x = x.step("Bootmend can read the crash files to see which driver was involved.").act(Action::AnalyzeDumps);
        }
        f.push(x.act(Action::Goto(Page::Crashes)));
    }

    let shutdowns = h.a("unexpectedShutdowns");
    let silent: Vec<_> = shutdowns.iter().filter(|e| e.s("Code").as_deref() == Some("0x0")).collect();
    let button = silent.iter().filter(|e| e.b("PowerButton") == Some(true)).count();
    if silent.len() >= 2 {
        let mut x = Finding::new("power-loss", STAB, Level::WorthDoing, format!("The PC switched off without shutting down {}", plural(silent.len(), "time", "times")))
            .why("Windows had no chance to record a reason. Common causes are power cuts, a loose power cable, overheating, a struggling power supply, or holding the power button.")
            .step("If it happens while gaming or under load, check that fans spin and vents are clear, and consider the power supply.")
            .details(silent.iter().take(8).filter_map(|e| e.s("Date")))
            .conf(Confidence::BestGuess);
        if button > 0 {
            x = x.detail(format!("{} followed the power button being held down", plural(button, "shutdown", "shutdowns")));
        }
        f.push(x.act(Action::Goto(Page::Crashes)));
    }

    if whea > 0 {
        f.push(
            Finding::new("whea", STAB, if whea >= 10 || stress { Level::WorthDoing } else { Level::NiceToHave }, format!("The hardware reported {} in the last 90 days", plural(whea, "error", "errors")))
                .why("The processor, memory or a connected card told Windows something went wrong. One now and then is usually harmless. Many of them often point to an unstable overclock or memory setting, or a failing part.")
                .details(h.a("whea").into_iter().take(6).map(|w| format!("{} {}", w.s("Date").unwrap_or_default(), w.s("Text").unwrap_or_default())))
                .conf(Confidence::BestGuess)
                .act(Action::Goto(Page::Crashes)),
        );
    }

    let cpu = s.system.g("cpu");
    if let (Some(name), Some((base, boost))) = (cpu.s("Name"), cpu.s("Name").and_then(|n| cpu_clocks(&n))) {
        let rated = cpu.f("RatedMHz").unwrap_or(0.0).max(cpu.f("CurrentMHz").unwrap_or(0.0)) / 1000.0;
        if rated > boost * 1.03 {
            f.push(
                Finding::new("overclock", STAB, if whea > 0 || stress { Level::WorthDoing } else { Level::NiceToHave }, "The processor seems to be overclocked")
                    .why(format!("Windows reports {rated:.1} GHz, while {name} is rated for {base:.1} GHz with boost up to {boost:.1} GHz. Overclocks can work fine, but they are a common cause of crashes and hardware errors."))
                    .step("If you see crashes or errors, set the CPU back to default in the BIOS first. It's the quickest way to rule it out.")
                    .conf(Confidence::BestGuess),
            );
        }
    }

    for c in h.a("appCrashes") {
        if let (Some(app), Some(n)) = (c.s("App"), c.i("Count")) {
            if n >= 5 {
                f.push(
                    Finding::new(format!("appcrash-{app}"), STAB, Level::NiceToHave, format!("{app} crashed {n} times this month"))
                        .why("Repeated crashes of one app are usually fixed by updating or reinstalling it.")
                        .step("Update the app. If that doesn't help, uninstall it and install it again from the maker's site.")
                        .open("Open installed apps", "ms-settings:appsfeatures"),
                );
            }
        }
    }

    if s.system.g("os").b("PendingReboot") == Some(true) {
        f.push(
            Finding::new("reboot", STAB, Level::NiceToHave, "Restart to finish installing updates")
                .why("Windows installed updates that only take effect after a restart. Until then, some fixes aren't active."),
        );
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn stop_code_is_read_from_event_text() {
        assert_eq!(super::bugcheck_code("The computer has rebooted from a bugcheck. The bugcheck was: 0x000000d1 (0x0, 0x2)."), Some(0xD1));
        assert_eq!(super::bugcheck_code("nothing here"), None);
    }
}
