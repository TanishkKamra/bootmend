use super::{plural, Action, Area, Confidence, Finding, Level};
use crate::gui::Page;
use crate::snapshot::Snapshot;
use crate::util::J;
use serde_json::Value;

const HW: Area = Area::Hardware;

pub fn check(s: &Snapshot, f: &mut Vec<Finding>) {
    drives(s, f);
    disk_errors(s, f);
    battery(s, f);
    devices(s, f);
    memory_test(s, f);
    firmware(s, f);
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

/// Device Manager codes that aren't faults: waiting for a restart (14), being removed (21), switched
/// off (22, or 29 in the BIOS), stopped by an app (44), unplugged (45) or ready to eject (47).
const NOT_FAULTS: [i64; 7] = [14, 21, 22, 29, 44, 45, 47];
/// Code 28: no driver is installed.
const NO_DRIVER: i64 = 28;

/// A part of the motherboard rather than an add-on: platform devices Windows finds through ACPI, and
/// PCI devices whose class code is a storage controller (01), memory controller (05), bridge (06),
/// system peripheral (08), USB controller (0C03), SMBus (0C05), management engine or other
/// communication controller (0780), encryption (10), signal processing (11) or chipset
/// instrumentation (13, AMD's) part.
fn motherboard_part(d: &Value) -> bool {
    match d.s("Bus").as_deref() {
        Some("ACPI") => true,
        Some("PCI") => d
            .s("Cc")
            .map(|c| c.to_ascii_uppercase())
            .is_some_and(|c| ["01", "05", "06", "08", "10", "11", "13"].iter().any(|p| c.starts_with(p)) || ["0C03", "0C05", "0780"].contains(&c.as_str())),
        _ => false,
    }
}

fn device_name(d: &Value) -> String {
    let name = d.s("Name").unwrap_or_else(|| "Unknown device".into());
    match d.s("Class") {
        Some(c) => format!("{name} ({c})"),
        None => name,
    }
}

fn devices(s: &Snapshot, f: &mut Vec<Finding>) {
    let code = |d: &Value| d.i("Code").unwrap_or(0);
    let (missing, broken): (Vec<&Value>, Vec<&Value>) =
        s.system.a("problemDevices").into_iter().filter(|d| !NOT_FAULTS.contains(&code(d))).partition(|d| code(d) == NO_DRIVER);
    let (chipset, other): (Vec<&Value>, Vec<&Value>) = missing.into_iter().partition(|d| motherboard_part(d));
    if !chipset.is_empty() {
        f.push(
            Finding::new("drivers-chipset", HW, Level::WorthDoing, "Some of your motherboard's parts have no driver")
                .why("Windows is running these parts with no driver at all, which usually means the chipset driver is missing. The PC still works, but power saving, sleep or USB may not work as well as they should.")
                .step("Install the chipset driver from your PC maker's support page. For a self-built PC, use the motherboard maker's page, or Intel's or AMD's own driver page.")
                .step("Windows Update sometimes offers it too, under Optional updates.")
                .details(chipset.iter().map(|d| device_name(d)))
                .open("Open optional updates", "ms-settings:windowsupdate-optionalupdates")
                .open("Open Device Manager", "devmgmt.msc"),
        );
    }
    if !other.is_empty() {
        f.push(
            Finding::new("devices-no-driver", HW, Level::WorthDoing, format!("{} no driver", plural(other.len(), "device has", "devices have")))
                .why("Windows found these devices but has no driver for them, so they don't work yet.")
                .step("Look in Windows Update's Optional updates first. It often has the driver.")
                .step("If it isn't there, get the driver from the website of the device's or the PC's maker.")
                .details(other.iter().map(|d| device_name(d)))
                .open("Open optional updates", "ms-settings:windowsupdate-optionalupdates")
                .open("Open Device Manager", "devmgmt.msc"),
        );
    }
    if !broken.is_empty() {
        f.push(
            Finding::new("devices", HW, Level::WorthDoing, format!("{} not working properly", plural(broken.len(), "device is", "devices are")))
                .why("Windows couldn't start these, usually because a driver is missing or broken.")
                .step("Open Device Manager, right-click each device with a warning icon, and choose Update driver. The maker's website is the best source for drivers.")
                .details(broken.iter().map(|d| format!("{}, code {}", device_name(d), code(d))))
                .open("Open Device Manager", "devmgmt.msc"),
        );
    }
}

/// The latest Windows Memory Diagnostic result in 90 days. 1102 and 1202 mean it found errors.
fn memory_test(s: &Snapshot, f: &mut Vec<Finding>) {
    let t = s.health.g("memoryTest");
    if matches!(t.i("Id"), Some(1102 | 1202)) {
        f.push(
            Finding::new("memory-test-failed", HW, Level::DoFirst, "Windows' memory test found errors")
                .why("Windows Memory Diagnostic found faults in the PC's memory (RAM). Faulty memory can quietly damage files you save and cause crashes.")
                .step("Back up your important files first.")
                .step("If a memory profile (XMP, EXPO or DOCP) is switched on in the BIOS, switch it off and run the test again.")
                .step(
                    "If it still finds errors, the memory needs replacing. Testing one stick at a time shows which one is faulty, or a repair shop can do it.",
                )
                .step("Run the test again after any change. A clean result clears this to-do.")
                .details(t.s("Date").map(|d| format!("Test finished: {d}")))
                .open("Open Memory Diagnostic", "mdsched.exe"),
        );
    }
}

/// Drive firmware with a known fault that its maker has fixed. Samsung's two are widely reported;
/// the Western Digital ones, which crash Windows 11 24H2 and later, come from a reseller's table.
fn firmware(s: &Snapshot, f: &mut Vec<Finding>) {
    let build: u32 = s.system.g("os").s("Build").and_then(|b| b.split('.').next()?.parse().ok()).unwrap_or(0);
    for d in s.storage.a("disks") {
        let (Some(model), Some(fw), Some(n)) = (d.s("Model"), d.s("Firmware"), d.i("Number")) else { continue };
        let fw = fw.trim().to_ascii_uppercase();
        // Western Digital numbers its firmware "731100WD"; a lower number is older.
        let wd_before = |fixed: u32| fw.strip_suffix("WD").and_then(|v| v.parse::<u32>().ok()).is_some_and(|v| v < fixed);
        let two_tb = d.f("SizeGB").is_some_and(|g| (1700.0..2100.0).contains(&g));
        let samsung = "Samsung Magician, Samsung's free app";
        let (name, level, why, app, conf) = if model.contains("990 PRO") && fw == "0B2QJXD7" {
            ("Samsung 990 PRO".to_string(), Level::DoFirst, "This firmware has a known fault that wears the drive out far faster than normal. Samsung's update stops it, though it can't give back wear already done.", samsung, Confidence::Measured)
        } else if model.contains("980 PRO") && fw == "3B2QGXA7" {
            ("Samsung 980 PRO".to_string(), Level::DoFirst, "This firmware has a known fault that can make the drive's health drop quickly until it locks itself so files can only be read. Samsung's update fixes it.", samsung, Confidence::Measured)
        } else if build >= 26100
            && two_tb
            && ((model.contains("SN770") && wd_before(731130))
                || (model.contains("SN580") && wd_before(281050))
                || (model.contains("SN5000") && wd_before(291020)))
        {
            (
                model.clone(),
                Level::WorthDoing,
                "On Windows 11 24H2 and later, this firmware can cause blue-screen crashes. Western Digital's update fixes it.",
                "WD Dashboard, Western Digital's free app",
                Confidence::BestGuess,
            )
        } else {
            continue;
        };
        f.push(
            Finding::new(format!("ssd-firmware-{n}"), HW, level, format!("Your {name} needs a firmware update"))
                .why(why)
                .step("Back up your important files first.")
                .step(format!("Then update the drive's firmware with {app}. It takes a few minutes and keeps your files."))
                .detail(format!("Drive: {model}, firmware {fw}"))
                .conf(conf)
                .act(Action::Goto(Page::Storage)),
        );
    }
}
