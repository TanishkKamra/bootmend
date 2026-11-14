use super::data::cpu_clocks;
use super::{plural, Action, Area, Confidence, Finding, Level};
use crate::collect::dumps::bugcheck_info;
use crate::fixes::Fix;
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

/// What to call a crashed app: the program's own description, else its file name tidied up.
pub fn crashed_app(c: &serde_json::Value) -> Option<String> {
    let exe = c.s("App")?;
    Some(c.s("Name").filter(|x| !x.trim().is_empty()).unwrap_or_else(|| {
        let stem = exe.strip_suffix(".exe").or_else(|| exe.strip_suffix(".EXE")).unwrap_or(&exe);
        super::security::friendly_app(stem)
    }))
}

/// The graphics driver freezing and being restarted by Windows, counted over 30 days.
fn gpu_resets(s: &Snapshot, f: &mut Vec<Finding>) {
    // Virtual screens (Parsec, remote desktop) log these too, with no card to fix.
    let real = s.system.a("gpus").into_iter().filter_map(|g| g.s("Name")).any(|n| {
        let n = n.to_lowercase();
        !super::speed::VIRTUAL_GPUS.iter().any(|v| n.contains(v))
    });
    if !real {
        return;
    }
    let r = s.health.g("gpuResets");
    let today = chrono::Local::now().date_naive();
    // One freeze can show up in both lists within the same minute, so times are counted once.
    let mut times: Vec<String> = r
        .strs("Tdr")
        .into_iter()
        .chain(r.strs("Watchdog"))
        .filter(|t| t.get(..10).and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()).is_some_and(|d| (today - d).num_days() <= 30))
        .collect();
    times.sort();
    times.dedup();
    // Resets come in bursts, so they count by day: one bad evening is a one-off, three different
    // days in a month is a pattern (our judgement).
    let mut days: Vec<&str> = times.iter().filter_map(|t| t.get(..10)).collect();
    days.dedup();
    if days.len() >= 3 {
        f.push(
            Finding::new("gpu-resets", STAB, Level::WorthDoing, format!("The graphics driver stopped responding on {} of the last 30 days", days.len()))
                .why("Each time, the screen freezes or goes black for a moment while Windows restarts the driver. Games and video apps can crash with it.")
                .step("Install the latest driver from the graphics card's maker: the NVIDIA app, AMD Software or Intel's Driver & Support Assistant.")
                .step("If the graphics card or memory is overclocked, set it back to default.")
                .step("If it keeps happening, check the card's power cable and that its fans and vents aren't blocked.")
                .details(times.iter().rev().take(5).map(|t| format!("Stopped responding: {t}"))),
        );
    }
}

/// Start-up settings meant for developers and troubleshooting, left on.
fn boot_options(s: &Snapshot, f: &mut Vec<Finding>) {
    let os = s.system.g("os");
    let threads = s.system.g("cpu").i("Threads");
    let mut found = vec![];
    for t in os.s("StartOptions").unwrap_or_default().to_uppercase().split_whitespace() {
        let (k, v) = t.split_once('=').unwrap_or((t, ""));
        let what = match k {
            "TESTSIGNING" => "Test mode is on, so drivers nobody has signed can load (bcdedit /set testsigning off)",
            "NOINTEGRITYCHECKS" => "Driver signature checks are off (bcdedit /set nointegritychecks off)",
            "DEBUG" => "Kernel debugging is on (bcdedit /debug off)",
            "NOEXECUTE" if v == "ALWAYSOFF" => "Data Execution Prevention is off (bcdedit /set nx OptIn)",
            "MAXMEM" | "TRUNCATEMEMORY" => "Windows is told to use less memory than the PC has (msconfig, Boot, Advanced options)",
            "NUMPROC" if threads.is_some_and(|n| v.parse::<i64>().is_ok_and(|p| p < n)) => {
                "Windows is told to use fewer processor cores than the PC has (msconfig, Boot, Advanced options)"
            }
            k if k.starts_with("SAFEBOOT") => "Windows is set to start in safe mode (msconfig, Boot, Safe boot)",
            _ => continue,
        };
        found.push(what.to_string());
    }
    if os.b("Verifier") == Some(true) {
        found.push("Driver Verifier is on: it makes drivers fail on purpose to test them, which causes blue screens (verifier /reset)".into());
    }
    if !found.is_empty() {
        f.push(
            Finding::new("boot-test-mode", STAB, Level::WorthDoing, "Windows is still set up for testing")
                .why("Settings meant for developers and troubleshooting are still on. They can slow the PC, cause blue screens or let unsafe drivers load.")
                .step("If you didn't turn these on yourself, each one below names where it's switched off. The commands go in a Command Prompt opened with Run as administrator; restart afterwards.")
                .details(found),
        );
    }
}

fn pagefile(s: &Snapshot, f: &mut Vec<Finding>) {
    let perf = s.health.g("perf");
    if !perf.g("Pagefiles").as_array().is_some_and(|a| a.is_empty()) {
        return;
    }
    let ram = perf.f("RamTotalGB").unwrap_or(0.0);
    let (lost, warned) = crash_file_events(&s.health);
    let level = if ram < 16.0 || warned || !lost.is_empty() { Level::WorthDoing } else { Level::NiceToHave };
    let mut x = Finding::new("no-pagefile", STAB, level, "Windows has no page file")
        .why("A page file gives Windows room to spare when memory runs short, and it's where Windows saves the details of a crash. Without one, big apps can stop with out-of-memory errors, and blue screens can't be looked into.")
        .step("Search Start for \"Adjust the appearance and performance of Windows\", open Advanced, then Virtual memory, Change, and tick Automatically manage paging file size for all drives. Restart afterwards.")
        .act(Action::Fix(Fix::SystemManagedPagefile));
    if !lost.is_empty() {
        x = x.detail(format!("{} couldn't be saved in the last 90 days", plural(lost.len(), "crash file", "crash files")));
    } else if warned {
        x = x.detail("Windows warned at start-up that it couldn't save crash files");
    }
    f.push(x);
}

/// Crash files Windows failed to write (their dates), and whether it warned at start-up that it
/// couldn't write one at all. volmgr 161 is a failed write, 162 a saved file, 46 the warning. A
/// failure within minutes of a saved file is the same crash, partly saved, so it doesn't count.
fn crash_file_events(h: &serde_json::Value) -> (Vec<String>, bool) {
    let ev = h.a("dumpEvents");
    let at = |d: &str| chrono::NaiveDateTime::parse_from_str(d, "%Y-%m-%d %H:%M").ok();
    let saved: Vec<_> = ev.iter().filter(|e| e.i("Id") == Some(162)).filter_map(|e| at(&e.s("Date")?)).collect();
    let lost = ev
        .iter()
        .filter(|e| e.i("Id") == Some(161))
        .filter_map(|e| e.s("Date"))
        .filter(|d| at(d).is_none_or(|t| saved.iter().all(|s| (*s - t).num_minutes().abs() > 5)))
        .collect();
    (lost, ev.iter().any(|e| e.i("Id") == Some(46)))
}

/// A crash file shows what made Windows crash. Three things stop one being saved: the setting is
/// off, there's no page file on the system drive to write it through, or the write itself failed.
/// No page file at all is the no-pagefile to-do's job.
fn crash_files(s: &Snapshot, f: &mut Vec<Finding>) {
    let h = &s.health;
    let files: Vec<String> = h.g("perf").strs("Pagefiles");
    if files.is_empty() {
        return;
    }
    let cc = h.g("crashControl");
    let crashed = !h.a("bugchecks").is_empty() || h.a("unexpectedShutdowns").iter().any(|e| e.s("Code").is_some_and(|c| c != "0x0"));
    let (lost, warned) = crash_file_events(h);
    if cc.i("Enabled") == Some(0) {
        f.push(
            Finding::new("crash-files-off", STAB, if crashed { Level::WorthDoing } else { Level::NiceToHave }, "Windows is set not to save crash files")
                .why("When Windows crashes, it normally saves a file that shows what caused it. That's switched off, so crashes can't be looked into. Tuning tools sometimes switch it off to save space.")
                .step("Search Start for \"View advanced system settings\", open Startup and Recovery, Settings, and set Write debugging information to Automatic memory dump.")
                .act(Action::Fix(Fix::EnableCrashFiles)),
        );
        return;
    }
    let drive = cc.s("SystemDrive").filter(|d| !d.is_empty()).unwrap_or_else(|| "C:".into()).to_ascii_uppercase();
    let on_drive = files.iter().any(|p| p.to_ascii_uppercase().starts_with(&drive));
    if !on_drive && cc.s("DedicatedDumpFile").is_none_or(|d| d.is_empty()) {
        let level = if crashed || warned || !lost.is_empty() { Level::WorthDoing } else { Level::NiceToHave };
        f.push(
            Finding::new("crash-files-no-room", STAB, level, "Windows can't save crash files")
                .why(format!("When Windows crashes, it saves what caused it through the page file on drive {drive}, and {drive} doesn't have one. Without that file, crashes can't be looked into."))
                .step("Search Start for \"Adjust the appearance and performance of Windows\", open Advanced, then Virtual memory, Change, and tick Automatically manage paging file size for all drives. Restart afterwards.")
                .details(files.iter().map(|p| format!("Page file: {p}")))
                .act(Action::Fix(Fix::SystemManagedPagefile)),
        );
        return;
    }
    if !lost.is_empty() {
        let these = if lost.len() == 1 { "this crash" } else { "these crashes" };
        f.push(
            Finding::new("crash-files-lost", STAB, Level::WorthDoing, format!("Windows couldn't save the details of {}", plural(lost.len(), "crash", "crashes")))
                .why(format!("When Windows crashes, it writes a file that shows what went wrong. For {these} it couldn't, which usually means the drive stopped responding, or the PC lost power, while the file was being written."))
                .step("Check the drive's health on the Storage page.")
                .step("If the PC also switches off by itself, the power supply may be struggling. Have it checked.")
                .details(lost)
                .conf(Confidence::BestGuess)
                .act(Action::Goto(Page::Storage)),
        );
    }
}

pub fn hardware_stress_codes(code: u32) -> bool {
    matches!(code, 0x124 | 0x101 | 0x7F)
}

pub fn check(s: &Snapshot, f: &mut Vec<Finding>) {
    gpu_resets(s, f);
    boot_options(s, f);
    pagefile(s, f);
    crash_files(s, f);
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

    // (shown name, program file, crashes in the last 30 days)
    let apps: Vec<(String, String, i64)> = h.a("appCrashes").into_iter().filter_map(|c| Some((crashed_app(c)?, c.s("App")?, c.i("Count")?))).collect();
    if apps.len() >= 3 {
        // Several different apps failing usually share one cause, so they get one to-do.
        f.push(
            Finding::new("appcrash-many", STAB, Level::WorthDoing, format!("{} crashed this month", plural(apps.len(), "app", "apps")))
                .why("When several different apps crash, they often share a cause, such as an outdated driver, a security tool or a memory problem.")
                .step("Install any waiting Windows and driver updates, then restart.")
                .step("If the crashes continue, search Start for \"Windows Memory Diagnostic\" and run it.")
                .details(apps.iter().map(|(name, _, n)| format!("{name}: {}", plural(*n as usize, "time", "times"))))
                .act(Action::Goto(Page::Crashes)),
        );
    } else {
        for (name, exe, n) in apps.iter().filter(|(_, _, n)| *n >= 5) {
            let mut x = Finding::new(format!("appcrash-{exe}"), STAB, Level::WorthDoing, format!("{name} crashed {n} times this month"))
                .why("Repeated crashes of one app are usually fixed by updating or reinstalling it.")
                .step("Update the app. If that doesn't help, uninstall it and install it again from the maker's site.")
                .open("Open installed apps", "ms-settings:appsfeatures");
            if name != exe {
                x = x.detail(format!("Program file: {exe}"));
            }
            f.push(x);
        }
    }

    if s.system.g("os").b("PendingReboot") == Some(true) {
        let since = s.system.g("winupdate").s("RestartWaitingSince").and_then(|d| chrono::NaiveDate::parse_from_str(d.get(..10)?, "%Y-%m-%d").ok());
        // After a week, fixes have sat idle through a lot of normal use. With no start date it may
        // have waited that long already.
        let level = if since.is_none_or(|d| (chrono::Local::now().date_naive() - d).num_days() >= 7) { Level::WorthDoing } else { Level::NiceToHave };
        let mut x = Finding::new("reboot", STAB, level, "Restart to finish installing updates")
            .why("Windows installed updates that only take effect after a restart. Until then, some fixes aren't active.");
        if let Some(d) = since {
            x = x.detail(format!("Waiting since {d}"));
        }
        f.push(x);
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
