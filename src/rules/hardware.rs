use super::{plural, Action, Area, Finding, Level};
use crate::gui::Page;
use crate::snapshot::Snapshot;
use crate::util::J;

const HW: Area = Area::Hardware;

pub fn check(s: &Snapshot, f: &mut Vec<Finding>) {
    drives(s, f);
    disk_errors(s, f);
    battery(s, f);
    devices(s, f);
}

/// Storage errors in Windows' System log over 90 days, on drives inside the PC.
fn disk_errors(s: &Snapshot, f: &mut Vec<Finding>) {
    // A failing drive already has its own Do first to-do.
    if f.iter().any(|x| x.id.starts_with("drive-failing")) {
        return;
    }
    let usb: Vec<i64> = s.storage.a("disks").into_iter().filter(|d| d.s("Bus").as_deref() == Some("USB")).filter_map(|d| d.i("Number")).collect();
    let volumes = s.storage.a("volumes");
    let (mut serious, mut retries, mut details) = (0, 0, vec![]);
    for e in s.storage.a("diskErrors") {
        let (Some(source), Some(id), Some(n)) = (e.s("Source"), e.i("Id"), e.i("Count")) else { continue };
        // Ntfs names a drive letter. A letter missing from the fixed drives is a memory stick or card.
        let disk = match e.s("Volume") {
            Some(l) => match volumes.iter().find(|v| v.s("Letter").is_some_and(|x| x.trim_end_matches(':').eq_ignore_ascii_case(&l))) {
                Some(v) => v.i("Disk"),
                None => continue,
            },
            None => e.i("Disk"),
        };
        if disk.is_some_and(|d| usb.contains(&d)) {
            continue;
        }
        let source = source.to_lowercase();
        let (bad, what) = match (source.as_str(), id) {
            ("disk", 7) => (true, "Damaged areas (bad blocks)"),
            ("disk", 52) => (true, "The drive warned it may fail"),
            (x, 55) if x.contains("ntfs") => (true, "File system damage"),
            // Also logged when a drive is pulled out mid-write, so these need repeating.
            (x, 50 | 140) if x.contains("ntfs") => (false, "Writes that didn't reach the drive"),
            ("disk", 11 | 15) => (false, "Controller errors"),
            ("disk", 51 | 153) => (false, "Reads or writes that had to be retried"),
            (_, 129) => (false, "The drive stopped responding and was reset"),
            _ => continue,
        };
        if bad {
            serious += n;
        } else {
            retries += n;
        }
        details.push(format!("{what}: {}{}", plural(n as usize, "time", "times"), disk.map(|d| format!(" (disk {d})")).unwrap_or_default()));
    }
    // One retry is often a hiccup; three or more is a pattern (our judgement).
    if serious > 0 || retries >= 3 {
        f.push(
            Finding::new("disk-errors", HW, Level::WorthDoing, "Windows logged errors reading or writing a drive")
                .why("In the last 90 days Windows had trouble with a drive inside this PC. A loose cable, a driver problem or a drive starting to wear out can all cause it.")
                .step("Back up anything important first.")
                .step("Then check the drive: in File Explorer, right-click it, choose Properties, Tools, Check. It scans first and asks before repairing anything.")
                .step("If the errors keep coming, update the storage driver from the PC or motherboard maker's site, or on a desktop, reseat the drive's cable.")
                .details(details),
        );
    }
}

fn drives(s: &Snapshot, f: &mut Vec<Finding>) {
    for d in &s.drives {
        let name = format!("{} ({})", d.model, d.kind);
        let windows_health = s.storage.a("disks").into_iter().find(|x| x.i("Number") == Some(d.disk)).and_then(|x| x.s("Health"));
        let bad_health = windows_health.as_deref().is_some_and(|h| h != "Healthy");
        let errors = d.pending.unwrap_or(0.0) + d.uncorrectable.unwrap_or(0.0) + d.media_errors.unwrap_or(0.0);
        if d.predict_failure == Some(true) || bad_health || errors > 0.0 {
            let mut x = Finding::new(format!("drive-failing-{}", d.disk), HW, Level::DoFirst, format!("Back up {} now", d.model))
                .why("The drive reports problems reading or keeping data. Drives in this state can fail without further warning, so copy anything you care about somewhere else today.")
                .step("Copy your files to an external drive or cloud storage, then plan to replace this drive.")
                .detail(format!("Drive: {name}"));
            if let Some(h) = windows_health.filter(|_| bad_health) {
                x = x.detail(format!("Windows health status: {h}"));
            }
            for (l, v) in [("Pending sectors", d.pending), ("Uncorrectable errors", d.uncorrectable), ("Media errors", d.media_errors)] {
                if let Some(v) = v.filter(|v| *v > 0.0) {
                    x = x.detail(format!("{l}: {v}"));
                }
            }
            f.push(x.act(Action::Goto(Page::Storage)));
            continue;
        }
        if let Some(r) = d.reallocated.filter(|r| *r > 0.0) {
            f.push(
                Finding::new(format!("drive-realloc-{}", d.disk), HW, Level::WorthDoing, format!("{} has replaced {} damaged spots", d.model, r))
                    .why("The drive swapped out some worn areas for spare ones. A small, steady number can last for years, but it should not keep growing. Bootmend will tell you if it does.")
                    .step("Make sure your important files are backed up.")
                    .act(Action::Goto(Page::Storage)),
            );
        }
        if let Some(w) = d.life_used_pct.filter(|w| *w >= 75.0) {
            f.push(
                Finding::new(format!("drive-wear-{}", d.disk), HW, if w >= 90.0 { Level::WorthDoing } else { Level::NiceToHave }, format!("{} has used {w:.0}% of its rated life", d.model))
                    .why("SSDs can only be written a certain amount. Many keep working well past 100%, but it's a good time to keep backups current and plan a replacement.")
                    .act(Action::Goto(Page::Storage)),
            );
        }
        if let Some(sp) = d.spare_pct.filter(|s| *s < 20.0) {
            f.push(
                Finding::new(format!("drive-spare-{}", d.disk), HW, Level::WorthDoing, format!("{} is running low on spare space", d.model))
                    .why(format!("Only {sp:.0}% of the drive's reserve for worn-out cells is left. When it runs out, the drive may switch to read-only."))
                    .act(Action::Goto(Page::Storage)),
            );
        }
        let hot = if d.kind.contains("hard disk") { 55.0 } else { 70.0 };
        if let Some(t) = d.temp_c.filter(|t| *t >= hot) {
            f.push(
                Finding::new(format!("drive-hot-{}", d.disk), HW, Level::WorthDoing, format!("{} is running hot ({t:.0} °C)", d.model))
                    .why("Heat shortens a drive's life and can make it slow itself down. Good airflow around the drive usually fixes it.")
                    .act(Action::Goto(Page::Storage)),
            );
        }
    }
}

fn battery(s: &Snapshot, f: &mut Vec<Finding>) {
    let b = s.system.g("battery");
    let (Some(design), Some(full)) = (b.f("DesignmWh"), b.f("FullChargemWh")) else { return };
    if design <= 0.0 {
        return;
    }
    let pct = full / design * 100.0;
    if pct < 70.0 {
        f.push(
            Finding::new(
                "battery-worn",
                HW,
                if pct < 50.0 { Level::WorthDoing } else { Level::NiceToHave },
                format!("The battery holds {pct:.0}% of its original charge"),
            )
            .why("All batteries wear with use. Below about 70% you'll notice shorter unplugged time, and a replacement brings it back.")
            .detail(format!("Holds {:.0} Wh now, {:.0} Wh when new", full / 1000.0, design / 1000.0))
            .details(b.i("Cycles").filter(|c| *c > 0).map(|c| format!("{c} charge cycles"))),
        );
    }
}

fn devices(s: &Snapshot, f: &mut Vec<Finding>) {
    let bad = s.system.a("problemDevices");
    if !bad.is_empty() {
        f.push(
            Finding::new("devices", HW, Level::WorthDoing, format!("{} not working properly", plural(bad.len(), "device is", "devices are")))
                .why("Windows couldn't start these, usually because a driver is missing or broken.")
                .step("Open Device Manager, right-click each device with a warning icon, and choose Update driver. The maker's website is the best source for drivers.")
                .details(bad.iter().map(|d| format!("{} ({}), code {}", d.s("Name").unwrap_or_else(|| "Unknown device".into()), d.s("Class").unwrap_or_default(), d.i("Code").unwrap_or(0))))
                .open("Open Device Manager", "devmgmt.msc"),
        );
    }
}
