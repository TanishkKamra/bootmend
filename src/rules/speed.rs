use super::{plural, Action, Area, Confidence, Finding, Level};
use crate::fixes::Fix;
use crate::gui::Page;
use crate::snapshot::Snapshot;
use crate::util::J;

const SPD: Area = Area::Speed;
/// Screens made by software (remote desktop and game streaming tools, virtual machines), not a
/// graphics card, so their drivers' age means nothing.
pub(super) const VIRTUAL_GPUS: &[&str] = &["parsec", "virtual", "iddcx", "iddsample", "remote display", "hyper-v", "vmware"];

pub fn check(s: &Snapshot, f: &mut Vec<Finding>) {
    memory(s, f);
    links(s, f);
    storage(s, f);
    startup(s, f);
    misc(s, f);
}

fn memory(s: &Snapshot, f: &mut Vec<Finding>) {
    let m = s.system.g("memory");
    let sticks = m.a("Sticks");
    if let Some(total) = m.f("TotalGB").filter(|t| *t > 0.0 && *t < 7.5) {
        f.push(
            Finding::new("ram-low", SPD, Level::WorthDoing, format!("Only {total:.0} GB of memory"))
                .why("With a browser and a few apps open, Windows 11 often needs more than this, so it moves things to the much slower drive.")
                .step("Adding memory is one of the cheapest upgrades. Check the Hardware page for free slots and the type you need.")
                .conf(Confidence::BestGuess)
                .act(Action::Goto(Page::ThisPc)),
        );
    }
    if sticks.len() == 1 && m.i("Slots").unwrap_or(0) >= 2 {
        let st = sticks[0];
        f.push(
            Finding::new("ram-single-channel", SPD, Level::WorthDoing, "Memory is running on one channel")
                .why("This PC has a single memory stick, but it can use two at once. A matching pair can move up to twice as much data, which helps games and integrated graphics most.")
                .step(format!("Add a second stick matching the first: {} GB {} at {} MT/s.", st.f("SizeGB").unwrap_or(0.0), st.s("Type").unwrap_or_else(|| "memory".into()), st.i("SpeedMTs").unwrap_or(0)))
                .detail(format!("{} of {} slots used", sticks.len(), m.i("Slots").unwrap_or(0)))
                .details(st.s("Part").map(|p| format!("Current stick: {} {p}", st.s("Maker").unwrap_or_default())))
                .act(Action::Goto(Page::ThisPc)),
        );
    }
    let slow: Vec<_> = sticks.iter().filter(|x| matches!((x.i("SpeedMTs"), x.i("ConfiguredMTs")), (Some(r), Some(c)) if c > 0 && r - c >= 200)).collect();
    if let Some(st) = slow.first() {
        let unstable = !s.health.a("whea").is_empty() || !s.health.a("bugchecks").is_empty();
        let mut x = Finding::new("ram-slow", SPD, Level::NiceToHave, "Memory runs slower than it's rated for").why(format!(
            "The memory is rated for {} MT/s but runs at {} MT/s. That's the safe default, but it leaves some speed unused.",
            st.i("SpeedMTs").unwrap_or(0),
            st.i("ConfiguredMTs").unwrap_or(0)
        ));
        x = if unstable {
            x.step("This PC has had crashes or hardware errors, so leave this as it is until those are sorted out.")
        } else {
            x.step("In the BIOS, turn on the memory profile (called XMP on Intel boards, EXPO or DOCP on AMD). If the PC becomes unstable, turn it off again.")
        };
        f.push(x);
    }
    let perf = s.health.g("perf");
    if let (Some(used), Some(limit)) = (perf.f("CommitUsedGB"), perf.f("CommitLimitGB")) {
        if limit > 0.0 && used / limit > 0.85 {
            f.push(
                Finding::new("ram-pressure", SPD, Level::WorthDoing, "Memory was nearly full during the check")
                    .why(format!("{used:.1} GB of {limit:.1} GB was in use. When it fills up, apps slow down or close without warning."))
                    .step("Close apps you aren't using. The biggest users at the time are listed below.")
                    .details(
                        s.health.a("topMemory").into_iter().take(5).map(|p| format!("{}: {} MB", p.s("Name").unwrap_or_default(), p.i("MB").unwrap_or(0))),
                    ),
            );
        }
    }
    // Windows' own warnings that it ran out of memory in the last 30 days. Once can be a one-off;
    // twice is a pattern (our judgement).
    let low = s.health.a("lowMemory");
    if low.len() >= 2 {
        let mut apps: Vec<(String, usize)> = Vec::new();
        for exe in low.iter().flat_map(|e| e.strs("Apps")) {
            let stem = exe.strip_suffix(".exe").or_else(|| exe.strip_suffix(".EXE")).unwrap_or(&exe);
            let mut name = super::security::friendly_app(stem);
            if let Some(c) = name.get(..1) {
                name = c.to_uppercase() + &name[1..];
            }
            match apps.iter_mut().find(|(n, _)| *n == name) {
                Some((_, count)) => *count += 1,
                None => apps.push((name, 1)),
            }
        }
        apps.sort_by_key(|a| std::cmp::Reverse(a.1));
        f.push(
            Finding::new("low-memory", SPD, Level::WorthDoing, format!("Windows ran low on memory {} this month", plural(low.len(), "time", "times")))
                .why("Programs used up all the memory Windows could hand out. When that happens, apps slow down, freeze or close without warning.")
                .step("Close programs and browser tabs you aren't using.")
                .step("If the same program tops the list every time, update or reinstall it. It may be holding on to memory it doesn't need.")
                .step("If it keeps happening with only your usual apps open, more memory would help.")
                .details(apps.iter().take(5).map(|(n, c)| format!("{n} was among the biggest users {}", plural(*c, "time", "times")))),
        );
    }
}

fn links(s: &Snapshot, f: &mut Vec<Finding>) {
    // Wired ports that only do 100 Mbps say so in their name; any other is held back by its cable,
    // plug or router port.
    for a in s.network.a("adapters") {
        let device = a.s("Device").unwrap_or_default();
        let lower = device.to_lowercase();
        let wired = a.s("MediaType").is_some_and(|m| m.contains("802.3"));
        let slow = matches!(a.s("Speed").as_deref(), Some("100 Mbps" | "10 Mbps"));
        let fast_only = ["10/100", "fast ethernet", "fe family", "100m"].iter().any(|k| lower.contains(k));
        if wired && slow && !fast_only && a.s("Status").as_deref() == Some("Up") {
            f.push(
                Finding::new("link-slow", SPD, Level::NiceToHave, format!("Your network cable runs at {}", a.s("Speed").unwrap_or_default()))
                    .why("Network ports and routers today run at 1 Gbps, ten times faster. An old or damaged cable, or a loose plug, usually holds them back. It only matters if your internet or home network is faster than 100 Mbps.")
                    .step("Try another cable (marked Cat 5e or newer) and another port on the router.")
                    .detail(format!("Network card: {device}")),
            );
            break;
        }
    }
    let ssd = s.storage.a("disks").into_iter().any(|d| d.b("IsSystem") == Some(true) && d.s("Media").as_deref() == Some("SSD"));
    if ssd && s.storage.g("trim").b("Off") == Some(true) {
        f.push(
            Finding::new("trim-off", SPD, Level::NiceToHave, "TRIM is off for the SSD")
                .why("TRIM lets Windows tell the SSD which space is free, which keeps it fast and helps it last. Windows turns it on by default; old tuning guides switch it off.")
                .step("Search Start for Command Prompt, choose Run as administrator, and type: fsutil behavior set DisableDeleteNotify 0")
                .act(Action::Fix(Fix::EnableTrim)),
        );
    }
}

fn storage(s: &Snapshot, f: &mut Vec<Finding>) {
    for v in s.storage.a("volumes") {
        let (Some(letter), Some(size), Some(free)) = (v.s("Letter"), v.f("SizeGB"), v.f("FreeGB")) else { continue };
        if size < 20.0 {
            continue;
        }
        let pct = free / size * 100.0;
        let system = letter == "C:";
        // Windows needs 2-3 GB for a monthly update and 6-11 GB for a yearly one. The share of the
        // drive is capped, so a big drive with plenty left doesn't raise the alarm.
        let (urgent, low) = ((size * 0.05).clamp(5.0, 10.0), (size * 0.10).clamp(15.0, 25.0));
        let level = if system && free < urgent {
            Level::DoFirst
        } else if system && free < low {
            Level::WorthDoing
        } else if !system && pct < 5.0 {
            Level::NiceToHave
        } else {
            continue;
        };
        f.push(
            Finding::new(format!("space-{letter}"), SPD, level, format!("Drive {letter} is almost full"))
                .why(format!(
                    "{free:.1} GB of {size:.0} GB is free ({pct:.0}%). Windows needs room for updates and temporary files, and slows down when it runs short."
                ))
                .step("Free up space shows which caches and leftovers can go, and how big each is, before anything is removed.")
                .step("Then look at Downloads and large apps you no longer use.")
                .act(Action::Goto(Page::Cleanup)),
        );
    }
    let hdd_system = s.storage.a("disks").into_iter().find(|d| d.b("IsSystem") == Some(true) && d.s("Media").as_deref() == Some("HDD"));
    if let Some(d) = hdd_system {
        f.push(
            Finding::new("hdd-system", SPD, Level::WorthDoing, "Windows is installed on a hard disk")
                .why("A hard disk has moving parts and is many times slower than an SSD at the small reads Windows does all day. Moving to an SSD is the biggest speed upgrade most older PCs can get.")
                .detail(format!("System drive: {}", d.s("Model").unwrap_or_default())),
        );
    }
    let big: Vec<String> = s
        .storage
        .a("bigFolders")
        .into_iter()
        .filter(|b| b.f("SizeGB").unwrap_or(0.0) >= 10.0)
        .map(|b| format!("{}: {:.1} GB", b.s("Path").unwrap_or_default(), b.f("SizeGB").unwrap_or(0.0)))
        .collect();
    if !big.is_empty() {
        f.push(
            Finding::new("big-folders", SPD, Level::NiceToHave, "Some folders are taking a lot of space")
                .why("Downloads and temporary folders tend to grow without anyone noticing.")
                .details(big)
                .open("Open storage settings", "ms-settings:storagesense"),
        );
    }
}

fn startup(s: &Snapshot, f: &mut Vec<Finding>) {
    let on: Vec<String> = s.software.a("startup").into_iter().filter(|x| x.b("Enabled") != Some(false)).filter_map(|x| x.s("Name")).collect();
    if on.len() > 8 {
        f.push(
            Finding::new("startup-many", SPD, Level::NiceToHave, format!("{} open when you sign in", plural(on.len(), "app", "apps")))
                .why("Each one adds to the wait after you sign in and keeps using memory in the background. Most apps work fine if you open them yourself.")
                .step("On the Startup page, switch off the ones you don't need straight away. Nothing is uninstalled.")
                .details(on)
                .act(Action::Goto(Page::Startup)),
        );
    }
    let boots: Vec<f64> = s.health.a("boots").into_iter().filter_map(|b| b.f("MainPathSec")).collect();
    if boots.len() >= 2 {
        let avg = boots.iter().sum::<f64>() / boots.len() as f64;
        if avg > 45.0 {
            f.push(
                Finding::new("boot-slow", SPD, Level::NiceToHave, format!("Windows takes about {avg:.0} seconds to start"))
                    .why("Windows records what slowed each start. The biggest delays are listed below.")
                    .details(s.health.a("bootDelays").into_iter().take(5).map(|d| {
                        format!(
                            "{} ({}): about {} s, {} times",
                            d.s("Name").unwrap_or_default(),
                            d.s("Kind").unwrap_or_default(),
                            d.f("AvgSec").unwrap_or(0.0),
                            d.i("Times").unwrap_or(0)
                        )
                    }))
                    .act(Action::Goto(Page::Startup)),
            );
        }
    }
}

fn misc(s: &Snapshot, f: &mut Vec<Finding>) {
    let laptop = s.system.g("device").b("HasBattery") == Some(true);
    if !laptop && s.system.g("power").s("Plan").is_some_and(|p| p.to_lowercase().contains("saver")) {
        f.push(
            Finding::new("power-saver", SPD, Level::NiceToHave, "This desktop is on the Power saver plan")
                .why("Power saver holds the processor back. On a desktop the savings are small and the slowdown is noticeable.")
                .open("Open power settings", "ms-settings:powersleep"),
        );
    }
    for g in s.system.a("gpus") {
        let name = g.s("Name").unwrap_or_default();
        let lower = name.to_lowercase();
        if VIRTUAL_GPUS.iter().any(|v| lower.contains(v)) {
            continue;
        }
        if name.contains("Basic Display") || name.contains("Basic Render") {
            f.push(
                Finding::new("gpu-basic", SPD, Level::WorthDoing, "No proper graphics driver is installed")
                    .why("Windows is using its basic display driver, which is slow and can't use the graphics card's features.")
                    .step("Install the driver from your graphics card's or PC maker's website.")
                    .act(Action::Goto(Page::ThisPc)),
            );
        } else if let Some(date) = g.s("DriverDate") {
            if let Ok(d) = chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d") {
                let days = (chrono::Local::now().date_naive() - d).num_days();
                if days > 365 {
                    f.push(
                        Finding::new(format!("gpu-old-{name}"), SPD, Level::WorthDoing, format!("The {name} driver is over a year old"))
                            .why("Newer graphics drivers fix bugs and security holes, and often improve game performance.")
                            .step("Get the latest driver from the card maker's app or website: NVIDIA app, AMD Software or Intel Driver & Support Assistant.")
                            .detail(format!("Driver date: {date}"))
                            .conf(Confidence::Measured),
                    );
                }
            }
        }
    }
    if let Some(up) = s.health.g("perf").f("UptimeHours").filter(|h| *h > 24.0 * 14.0) {
        f.push(
            Finding::new("uptime", SPD, Level::NiceToHave, format!("The PC has been running for {:.0} days", up / 24.0)).why(
                "Shutting down with Fast Startup on doesn't fully restart Windows. A real restart now and then clears out leftovers and finishes updates.",
            ),
        );
    }
}
