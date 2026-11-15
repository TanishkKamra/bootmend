//! Space cleanup: finds files Windows and apps keep but don't need, shows their size first,
//! and clears only what the person ticks. Uses Windows' and each tool's own clean-up where one exists.

use crate::util::run_hidden;
use anyhow::{anyhow, Context, Result};
use std::collections::HashSet;
use std::fs::Metadata;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// Safe to clear: ticked by default.
    Safe,
    /// Has a trade-off the person should weigh: unticked by default.
    Choice,
}

#[derive(Debug, Clone, PartialEq)]
pub enum How {
    /// Delete files under each root that haven't changed for at least the given number of days.
    /// The roots themselves stay. `only` limits it to matching file names.
    Files { roots: Vec<(PathBuf, u64)>, only: Option<&'static str> },
    /// Move these files to the Recycle Bin, so they can be restored.
    Recycle(Vec<PathBuf>),
    /// Run a tool's own clean-up command; the size of `measure` shows what it frees.
    Command { program: &'static str, args: Vec<&'static str>, measure: Vec<PathBuf> },
    /// Windows' own component store clean-up; freed space is measured on drive C.
    Dism,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub id: &'static str,
    pub title: &'static str,
    pub why: &'static str,
    pub tradeoff: Option<&'static str>,
    pub group: Group,
    pub admin: bool,
    /// Process names (without ".exe") that must be closed first, and the app's name to show.
    pub close_first: Option<(&'static str, Vec<&'static str>)>,
    pub how: How,
    /// Bytes that would be freed; `None` when only cleaning can tell.
    pub bytes: Option<u64>,
    pub files: u64,
}

/// The folders cleanup looks in. Passed in so tests can point them at a scratch folder.
#[derive(Debug, Clone)]
pub struct Places {
    pub temp: PathBuf,
    pub local: PathBuf,
    pub roaming: PathBuf,
    pub profile: PathBuf,
    pub windir: PathBuf,
    pub program_data: PathBuf,
    pub system_drive: PathBuf,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Cleaned {
    pub bytes: u64,
    /// Files left alone because an app was using them.
    pub skipped: u64,
}

impl Places {
    pub fn current() -> Option<Places> {
        let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
        Some(Places {
            temp: std::env::temp_dir(),
            local: env("LOCALAPPDATA")?,
            roaming: env("APPDATA")?,
            profile: env("USERPROFILE")?,
            windir: env("WINDIR")?,
            program_data: env("ProgramData")?,
            system_drive: env("SystemDrive").map(|d| PathBuf::from(format!("{}\\", d.display())))?,
        })
    }
}

const DAY_SECS: u64 = 86_400;

fn old_enough(m: &Metadata, days: u64, now: SystemTime) -> bool {
    days == 0 || m.modified().ok().and_then(|t| now.duration_since(t).ok()).is_some_and(|age| age.as_secs() >= days * DAY_SECS)
}

fn name_matches(path: &Path, only: Option<&str>) -> bool {
    only.is_none_or(|o| path.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().starts_with(&o.to_lowercase())))
}

/// Links and junctions are never followed, so cleanup can't wander outside the folder it was given.
fn is_link(m: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    m.file_type().is_symlink() || m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

/// Every eligible file under the roots, with its size.
fn eligible(roots: &[(PathBuf, u64)], only: Option<&str>, now: SystemTime) -> Vec<(PathBuf, u64)> {
    fn walk(dir: &Path, days: u64, only: Option<&str>, now: SystemTime, out: &mut Vec<(PathBuf, u64)>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let Ok(m) = e.metadata() else { continue };
            if is_link(&m) {
                continue;
            }
            let p = e.path();
            if m.is_dir() {
                walk(&p, days, only, now, out);
            } else if name_matches(&p, only) && old_enough(&m, days, now) {
                out.push((p, m.len()));
            }
        }
    }
    let mut out = vec![];
    for (root, days) in roots {
        match std::fs::symlink_metadata(root) {
            Ok(m) if m.is_file() && name_matches(root, only) && old_enough(&m, *days, now) => out.push((root.clone(), m.len())),
            Ok(m) if m.is_dir() && !is_link(&m) => walk(root, *days, only, now, &mut out),
            _ => {}
        }
    }
    out
}

/// Bytes and file count that `clean_files` would remove.
pub fn measure_files(roots: &[(PathBuf, u64)], only: Option<&str>, now: SystemTime) -> (u64, u64) {
    let found = eligible(roots, only, now);
    (found.iter().map(|(_, n)| n).sum(), found.len() as u64)
}

/// Delete eligible files. Files an app is using are skipped and counted. Folders that became
/// empty because of this are removed; folders that were already empty, and the roots, stay.
pub fn clean_files(roots: &[(PathBuf, u64)], only: Option<&str>, now: SystemTime) -> Cleaned {
    let mut c = Cleaned::default();
    let mut emptied: HashSet<PathBuf> = HashSet::new();
    for (p, n) in eligible(roots, only, now) {
        match std::fs::remove_file(&p) {
            Ok(()) => {
                c.bytes += n;
                if let Some(parent) = p.parent() {
                    emptied.insert(parent.to_path_buf());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => c.skipped += 1,
        }
    }
    let keep: HashSet<&PathBuf> = roots.iter().map(|(r, _)| r).collect();
    // Deepest folders first, so a parent can go once its children have.
    let mut dirs: Vec<PathBuf> = emptied.into_iter().collect();
    while let Some(d) = dirs.iter().max_by_key(|d| d.components().count()).cloned() {
        dirs.retain(|x| x != &d);
        let inside_a_root = roots.iter().any(|(r, _)| d.starts_with(r));
        if keep.contains(&d) || !inside_a_root {
            continue;
        }
        if std::fs::remove_dir(&d).is_ok() {
            if let Some(parent) = d.parent() {
                if !dirs.iter().any(|x| x == parent) {
                    dirs.push(parent.to_path_buf());
                }
            }
        }
    }
    c
}

/// Setup files at the top of the Downloads folder that are more than 30 days old.
pub fn old_installers(downloads: &Path, now: SystemTime) -> Vec<PathBuf> {
    const KINDS: [&str; 6] = ["exe", "msi", "msix", "msixbundle", "appx", "appxbundle"];
    let Ok(rd) = std::fs::read_dir(downloads) else { return vec![] };
    let mut out: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| e.metadata().is_ok_and(|m| m.is_file() && !is_link(&m) && old_enough(&m, 30, now)))
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| KINDS.contains(&x.to_string_lossy().to_lowercase().as_str())))
        .collect();
    out.sort();
    out
}

fn folder_size(paths: &[PathBuf]) -> u64 {
    let roots: Vec<(PathBuf, u64)> = paths.iter().map(|p| (p.clone(), 0)).collect();
    measure_files(&roots, None, SystemTime::now()).0
}

/// Whether a program can be started by name (looks for .exe, .cmd and .bat on PATH).
fn on_path(tool: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|d| ["exe", "cmd", "bat"].iter().any(|x| d.join(format!("{tool}.{x}")).is_file())))
}

/// Chromium-based browsers keep a cache per profile ("Default", "Profile 1", ...).
fn chromium_caches(user_data: &Path) -> Vec<(PathBuf, u64)> {
    let Ok(rd) = std::fs::read_dir(user_data) else { return vec![] };
    let mut out = vec![];
    for e in rd.flatten() {
        let p = e.path();
        if p.join("Preferences").is_file() {
            for c in ["Cache", "Code Cache", "GPUCache"] {
                if p.join(c).is_dir() {
                    out.push((p.join(c), 0));
                }
            }
        }
    }
    out
}

fn firefox_caches(local: &Path) -> Vec<(PathBuf, u64)> {
    let Ok(rd) = std::fs::read_dir(local.join(r"Mozilla\Firefox\Profiles")) else { return vec![] };
    rd.flatten().map(|e| e.path().join("cache2")).filter(|p| p.is_dir()).map(|p| (p, 0)).collect()
}

fn existing(roots: Vec<(PathBuf, u64)>) -> Vec<(PathBuf, u64)> {
    roots.into_iter().filter(|(p, _)| p.exists()).collect()
}

const BROWSER_WHY: &str = "Copies of websites the browser saved so pages load faster. Sign-ins, history, passwords and bookmarks aren't touched.";
const BROWSER_COST: &str = "Sites load a little slower the first time you visit them again.";
const APP_WHY: &str = "Temporary copies of images and pages the app saved. Your account, messages and settings aren't touched.";

/// Everything cleanup could clear on this PC, with sizes. Places that don't exist, or hold nothing, are left out.
pub fn catalog(p: &Places, elevated: bool, now: SystemTime) -> Vec<Item> {
    let mut items = vec![];
    let mut files = |id, title, why, tradeoff, group, admin, close_first: Option<(&'static str, Vec<&'static str>)>, roots: Vec<(PathBuf, u64)>, only| {
        let roots = existing(roots);
        if !roots.is_empty() {
            items.push(Item { id, title, why, tradeoff, group, admin, close_first, how: How::Files { roots, only }, bytes: None, files: 0 });
        }
    };

    let mut temp = vec![(p.temp.clone(), 7)];
    if elevated {
        temp.push((p.windir.join("Temp"), 7));
    }
    files(
        "temp",
        "Temporary files",
        "Files apps left behind while installing or running. Only files nobody has touched for a week are cleared, the same rule Windows' own Disk Cleanup uses.",
        None,
        Group::Safe,
        false,
        None,
        temp,
        None,
    );
    files(
        "wu-downloads",
        "Windows Update downloads",
        "Update files Windows has already installed. Anything downloaded in the last 10 days stays, in case an update is still in progress.",
        None,
        Group::Safe,
        true,
        None,
        vec![(p.windir.join(r"SoftwareDistribution\Download"), 10)],
        None,
    );
    let mut crash = vec![(p.local.join("CrashDumps"), 0), (p.local.join(r"Microsoft\Windows\WER"), 0), (p.windir.join("Minidump"), 90)];
    if elevated {
        crash.extend([
            (p.program_data.join(r"Microsoft\Windows\WER\ReportArchive"), 0),
            (p.program_data.join(r"Microsoft\Windows\WER\ReportQueue"), 0),
            (p.windir.join("LiveKernelReports"), 0),
            (p.windir.join("MEMORY.DMP"), 0),
        ]);
    }
    files(
        "crash-reports",
        "Crash reports",
        "Reports Windows saved after an app or Windows itself crashed. Crash files from the last 90 days stay, so Bootmend can still read them.",
        None,
        Group::Safe,
        false,
        None,
        crash,
        None,
    );
    files(
        "thumbnails",
        "Picture previews",
        "Small previews of your photos and videos. Windows makes them again as you open folders.",
        None,
        Group::Safe,
        false,
        None,
        vec![(p.local.join(r"Microsoft\Windows\Explorer"), 0)],
        Some("thumbcache_"),
    );
    files(
        "driver-setup",
        "Driver setup leftovers",
        "Unpacked copies of graphics driver installers. The drivers stay installed; only the setup files go.",
        None,
        Group::Safe,
        true,
        None,
        vec![(p.system_drive.join("AMD"), 0), (p.system_drive.join("NVIDIA"), 0)],
        None,
    );

    let browsers = [
        ("chrome", "Chrome's cache", "chrome", chromium_caches(&p.local.join(r"Google\Chrome\User Data"))),
        ("edge", "Edge's cache", "msedge", chromium_caches(&p.local.join(r"Microsoft\Edge\User Data"))),
        ("brave", "Brave's cache", "brave", chromium_caches(&p.local.join(r"BraveSoftware\Brave-Browser\User Data"))),
        ("firefox", "Firefox's cache", "firefox", firefox_caches(&p.local)),
    ];
    for (id, title, exe, roots) in browsers {
        let name = title.trim_end_matches("'s cache");
        files(id, title, BROWSER_WHY, Some(BROWSER_COST), Group::Safe, false, Some((name, vec![exe])), roots, None);
    }
    let disc = p.roaming.join("discord");
    files(
        "discord",
        "Discord's cache",
        APP_WHY,
        None,
        Group::Safe,
        false,
        Some(("Discord", vec!["Discord"])),
        vec![(disc.join("Cache"), 0), (disc.join("Code Cache"), 0), (disc.join("GPUCache"), 0)],
        None,
    );
    files(
        "steam-web",
        "Steam's web cache",
        "Store and community pages Steam saved. Your games and downloads aren't touched.",
        None,
        Group::Safe,
        false,
        Some(("Steam", vec!["steam", "steamwebhelper"])),
        vec![(p.local.join(r"Steam\htmlcache"), 0)],
        None,
    );
    let code = p.roaming.join("Code");
    files(
        "vscode",
        "VS Code's cache",
        APP_WHY,
        None,
        Group::Safe,
        false,
        Some(("VS Code", vec!["Code"])),
        vec![(code.join("Cache"), 0), (code.join("CachedData"), 0), (code.join("Code Cache"), 0), (code.join("GPUCache"), 0)],
        None,
    );
    files(
        "shaders",
        "Game shader caches",
        "Graphics data games and the graphics driver prepare so scenes load smoothly.",
        Some("Games may stutter for a few minutes the first time while the driver rebuilds it."),
        Group::Choice,
        false,
        None,
        vec![
            (p.local.join("D3DSCache"), 0),
            (p.local.join(r"AMD\DxCache"), 0),
            (p.local.join(r"AMD\DxcCache"), 0),
            (p.local.join(r"AMD\VkCache"), 0),
            (p.local.join(r"NVIDIA\DXCache"), 0),
            (p.local.join(r"NVIDIA\GLCache"), 0),
            (p.local.join(r"Intel\ShaderCache"), 0),
        ],
        None,
    );

    // Coding tools: cleared with each tool's own command, so their bookkeeping stays right.
    let tools = [
        ("npm", "npm's download cache", "npm", vec!["cache", "clean", "--force"], p.local.join("npm-cache"), Group::Safe, None),
        ("pnpm", "pnpm's unused packages", "pnpm", vec!["store", "prune"], p.local.join(r"pnpm\store"), Group::Safe, None),
        ("pip", "pip's download cache", "pip", vec!["cache", "purge"], p.local.join(r"pip\cache"), Group::Safe, None),
        ("yarn", "Yarn's download cache", "yarn", vec!["cache", "clean"], p.local.join(r"Yarn\Cache"), Group::Safe, None),
        (
            "nuget",
            "NuGet's package folders",
            "dotnet",
            vec!["nuget", "locals", "all", "--clear"],
            p.profile.join(r".nuget\packages"),
            Group::Choice,
            Some("Your .NET projects download their packages again on the next build."),
        ),
    ];
    for (id, title, tool, args, folder, group, tradeoff) in tools {
        if !folder.is_dir() || !on_path(tool) {
            continue;
        }
        let mut a = vec!["/c", tool];
        a.extend(args);
        items.push(Item {
            id,
            title,
            why: "Packages your coding tools downloaded. The tool's own clean-up command clears them, and anything a project needs downloads again.",
            tradeoff,
            group,
            admin: false,
            close_first: None,
            how: How::Command { program: "cmd", args: a, measure: vec![folder] },
            bytes: None,
            files: 0,
        });
    }

    let downloads = p.profile.join("Downloads");
    let installers = old_installers(&downloads, now);
    if !installers.is_empty() {
        items.push(Item {
            id: "installers",
            title: "Old installers in Downloads",
            why: "Setup files you downloaded more than a month ago. They go to the Recycle Bin, so you can still get one back.",
            tradeoff: None,
            group: Group::Safe,
            admin: false,
            close_first: None,
            files: 0,
            how: How::Recycle(installers),
            bytes: None,
        });
    }

    let delivery = p.windir.join(r"ServiceProfiles\NetworkService\AppData\Local\Microsoft\Windows\DeliveryOptimization\Cache");
    if p.windir.join(r"ServiceProfiles\NetworkService").is_dir() {
        {
            items.push(Item {
                id: "delivery",
                title: "Update sharing cache",
                why: "Copies of updates Windows keeps to share with other PCs. Windows downloads them again if it needs them.",
                tradeoff: None,
                group: Group::Safe,
                admin: true,
                close_first: None,
                how: How::Command {
                    program: "powershell.exe",
                    args: vec!["-NoProfile", "-Command", "Delete-DeliveryOptimizationCache -Force"],
                    measure: vec![delivery],
                },
                bytes: None,
                files: 0,
            });
        }
        items.push(Item {
            id: "components",
            title: "Replaced Windows components",
            why: "Older copies of Windows files that newer updates replaced. Windows' own clean-up tool works out what can go.",
            tradeoff: Some("Takes 5 to 15 minutes, and updates that were replaced can no longer be uninstalled."),
            group: Group::Choice,
            admin: true,
            close_first: None,
            how: How::Dism,
            bytes: None,
            files: 0,
        });
    }

    // Measuring big caches takes a while, so every item is measured at the same time.
    let sizes: Vec<(Option<u64>, u64)> = std::thread::scope(|sc| {
        let jobs: Vec<_> = items.iter().map(|i| sc.spawn(move || measure(i, now))).collect();
        jobs.into_iter().map(|j| j.join().unwrap_or((None, 0))).collect()
    });
    for (i, (bytes, n)) in items.iter_mut().zip(sizes) {
        // Without administrator rights some Windows folders can't be read, so their size is unknown rather than zero.
        i.bytes = if i.admin && !elevated && bytes == Some(0) { None } else { bytes };
        i.files = n;
    }
    items.retain(|i| i.bytes != Some(0));
    items
}

fn measure(i: &Item, now: SystemTime) -> (Option<u64>, u64) {
    match &i.how {
        How::Files { roots, only } => {
            let (b, n) = measure_files(roots, *only, now);
            (Some(b), n)
        }
        How::Recycle(files) => (Some(files.iter().filter_map(|f| std::fs::metadata(f).ok()).map(|m| m.len()).sum()), files.len() as u64),
        How::Command { measure, .. } => (Some(folder_size(measure)), 0),
        How::Dism => (None, 0),
    }
}

/// A size the way File Explorer shows it (1 KB = 1024 bytes).
pub fn size_text(bytes: u64) -> String {
    const K: f64 = 1024.0;
    let b = bytes as f64;
    if b >= 1000.0 * K * K {
        format!("{:.1} GB", b / (K * K * K))
    } else if b >= 1000.0 * K {
        format!("{:.0} MB", b / (K * K))
    } else if bytes == 0 {
        "0 KB".into()
    } else {
        format!("{:.0} KB", (b / K).max(1.0))
    }
}

/// Programs from `names` that are running now (names without ".exe", any case).
pub fn running(names: &[&str]) -> Vec<String> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    let mut found: Vec<String> = sys
        .processes()
        .values()
        .map(|p| p.name().to_string_lossy().trim_end_matches(".exe").to_string())
        .filter(|n| names.iter().any(|x| x.eq_ignore_ascii_case(n)))
        .collect();
    found.sort();
    found.dedup();
    found
}

pub fn free_space(drive: &Path) -> Option<u64> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let mut free = 0u64;
    unsafe { GetDiskFreeSpaceExW(&HSTRING::from(drive.as_os_str()), None, None, Some(&mut free)) }.ok()?;
    Some(free)
}

/// Move files to the Recycle Bin without any Windows dialogs.
fn to_recycle_bin(files: &[PathBuf]) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE, SHFILEOPSTRUCTW};
    let mut from: Vec<u16> = vec![];
    for f in files {
        from.extend(f.as_os_str().encode_wide());
        from.push(0);
    }
    from.push(0);
    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI).0 as u16,
        ..Default::default()
    };
    let code = unsafe { SHFileOperationW(&mut op) };
    if code != 0 {
        return Err(anyhow!("Windows couldn't move the files to the Recycle Bin (code {code})"));
    }
    Ok(())
}

/// Size and item count of the Recycle Bin on all drives.
pub fn recycle_bin() -> Option<(u64, u64)> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{SHQueryRecycleBinW, SHQUERYRBINFO};
    let mut info = SHQUERYRBINFO { cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32, ..Default::default() };
    unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut info) }.ok()?;
    Some((info.i64Size.max(0) as u64, info.i64NumItems.max(0) as u64))
}

pub fn empty_recycle_bin() -> Result<()> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{SHEmptyRecycleBinW, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND};
    let before = recycle_bin();
    if before.is_some_and(|(_, n)| n == 0) {
        log!("cleanup", "recycle bin was already empty");
        return Ok(());
    }
    let r = unsafe { SHEmptyRecycleBinW(None, PCWSTR::null(), SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND) }
        .context("Windows couldn't empty the Recycle Bin");
    match &r {
        Ok(()) => log!("cleanup", "emptied the recycle bin ({} MB)", before.map_or(0, |(b, _)| b) / 1_000_000),
        Err(e) => log!("cleanup", "emptying the recycle bin failed: {e:#}"),
    }
    r
}

/// Clear one item. Apps listed in `close_first` must be closed; the caller checks with `running`.
pub fn clean(item: &Item, system_drive: &Path) -> Result<Cleaned> {
    log!("cleanup", "cleaning \"{}\"", item.title);
    let t0 = Instant::now();
    let r = clean_item(item, system_drive);
    match &r {
        Ok(c) => log!(
            "cleanup",
            "freed {} MB from \"{}\" in {:.1}s, {} file{} skipped",
            c.bytes / 1_000_000,
            item.title,
            t0.elapsed().as_secs_f32(),
            c.skipped,
            if c.skipped == 1 { "" } else { "s" }
        ),
        Err(e) => log!("cleanup", "\"{}\" failed: {e:#}", item.title),
    }
    r
}

fn clean_item(item: &Item, system_drive: &Path) -> Result<Cleaned> {
    match &item.how {
        How::Files { roots, only } => Ok(clean_files(roots, *only, SystemTime::now())),
        How::Recycle(files) => {
            let present: Vec<PathBuf> = files.iter().filter(|f| f.exists()).cloned().collect();
            let sizes: Vec<u64> = present.iter().map(|f| std::fs::metadata(f).map(|m| m.len()).unwrap_or(0)).collect();
            to_recycle_bin(&present)?;
            let mut c = Cleaned::default();
            for (f, n) in present.iter().zip(sizes) {
                if f.exists() {
                    c.skipped += 1;
                } else {
                    c.bytes += n;
                }
            }
            Ok(c)
        }
        How::Command { program, args, measure } => {
            let before = folder_size(measure);
            let out = run_hidden(program, args, Duration::from_secs(900))?;
            if out.timed_out {
                return Err(anyhow!("{} took too long", item.title));
            }
            if out.code != Some(0) {
                let why = out.stderr.lines().chain(out.stdout.lines()).map(str::trim).find(|l| !l.is_empty()).unwrap_or("it didn't say why").to_string();
                return Err(anyhow!("{} didn't finish: {why}", item.title));
            }
            Ok(Cleaned { bytes: before.saturating_sub(folder_size(measure)), skipped: 0 })
        }
        How::Dism => {
            let before = free_space(system_drive).unwrap_or(0);
            let out = run_hidden("dism.exe", &["/Online", "/Cleanup-Image", "/StartComponentCleanup"], Duration::from_secs(45 * 60))?;
            if out.timed_out {
                return Err(anyhow!("Windows' component clean-up took more than 45 minutes"));
            }
            if out.code != Some(0) {
                return Err(anyhow!("Windows' component clean-up stopped with code {}", out.code.unwrap_or(-1)));
            }
            Ok(Cleaned { bytes: free_space(system_drive).unwrap_or(before).saturating_sub(before), skipped: 0 })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_like_file_explorer() {
        assert_eq!(size_text(0), "0 KB");
        assert_eq!(size_text(300), "1 KB");
        assert_eq!(size_text(5 * 1024 * 1024), "5 MB");
        assert_eq!(size_text(1536 * 1024 * 1024), "1.5 GB");
        assert_eq!(size_text(1023 * 1024 * 1024), "1.0 GB");
    }
    use std::fs;
    use std::time::{Duration, SystemTime};

    const DAY: Duration = Duration::from_secs(86_400);

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bootmend-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn file(path: &Path, bytes: usize, age_days: u64) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![7u8; bytes]).unwrap();
        let f = fs::File::options().write(true).open(path).unwrap();
        f.set_modified(SystemTime::now() - DAY * age_days as u32).unwrap();
    }

    fn places(root: &Path) -> Places {
        Places {
            temp: root.join("temp"),
            local: root.join("local"),
            roaming: root.join("roaming"),
            profile: root.join("profile"),
            windir: root.join("windows"),
            program_data: root.join("programdata"),
            system_drive: root.join("c"),
        }
    }

    #[test]
    fn only_files_older_than_the_limit_are_cleared() {
        let d = scratch("age");
        file(&d.join("old.tmp"), 100, 10);
        file(&d.join("new.tmp"), 50, 1);
        file(&d.join("sub/deeper/old.log"), 30, 30);
        let roots = vec![(d.clone(), 7)];
        assert_eq!(measure_files(&roots, None, SystemTime::now()), (130, 2));
        let c = clean_files(&roots, None, SystemTime::now());
        assert_eq!(c, Cleaned { bytes: 130, skipped: 0 });
        assert!(d.join("new.tmp").exists() && !d.join("old.tmp").exists());
        assert!(!d.join("sub").exists(), "emptied folders go too");
        assert!(d.exists(), "the root folder itself stays");
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn name_filter_limits_what_is_cleared() {
        let d = scratch("filter");
        file(&d.join("thumbcache_256.db"), 40, 0);
        file(&d.join("iconcache_16.db"), 40, 0);
        assert_eq!(measure_files(&[(d.clone(), 0)], Some("thumbcache_"), SystemTime::now()), (40, 1));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_file_in_use_is_skipped_not_fatal() {
        let d = scratch("locked");
        file(&d.join("busy.tmp"), 10, 10);
        file(&d.join("free.tmp"), 10, 10);
        // Windows refuses to delete a file opened without delete sharing.
        let _hold = {
            use std::os::windows::fs::OpenOptionsExt;
            fs::File::options().read(true).share_mode(0).open(d.join("busy.tmp")).unwrap()
        };
        let c = clean_files(&[(d.clone(), 7)], None, SystemTime::now());
        assert_eq!(c, Cleaned { bytes: 10, skipped: 1 });
        drop(_hold);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn installers_are_picked_by_type_and_age() {
        let d = scratch("downloads");
        file(&d.join("setup.exe"), 10, 40);
        file(&d.join("photo.jpg"), 10, 40);
        file(&d.join("new-setup.msi"), 10, 2);
        file(&d.join("folder/inner.exe"), 10, 40);
        let found = old_installers(&d, SystemTime::now());
        assert_eq!(found, vec![d.join("setup.exe")], "only old setup files at the top level");
        fs::remove_dir_all(&d).unwrap();
    }

    /// Lists what cleanup would offer on this PC, with sizes. Reads only; deletes nothing.
    #[test]
    #[ignore = "reads this PC's folders"]
    fn shows_this_pcs_cleanup() {
        let t = std::time::Instant::now();
        let items = catalog(&Places::current().unwrap(), false, SystemTime::now());
        for i in &items {
            println!("{:<14} {:>10} MB  {:>6} files  admin={} {:?}", i.id, i.bytes.map(|b| b / 1_000_000).unwrap_or(0), i.files, i.admin, i.group);
        }
        println!("bin: {:?}  running: {:?}  took {:?}", recycle_bin(), running(&["chrome", "Discord", "steam", "Code", "msedge"]), t.elapsed());
    }

    /// Really clears temp files, picture previews and pip's cache on this PC, then measures again.
    #[test]
    #[ignore = "deletes temp files and caches on this PC"]
    fn clears_low_risk_items_on_this_pc() {
        let p = Places::current().unwrap();
        let items = catalog(&p, false, SystemTime::now());
        for id in ["temp", "thumbnails", "pip"] {
            let Some(i) = items.iter().find(|i| i.id == id) else { continue };
            let r = clean(i, &p.system_drive).unwrap();
            let after = measure(i, SystemTime::now()).0;
            println!("{id:<11} before {:>10}  freed {:>10}  skipped {:>5}  after {:?}", i.bytes.unwrap_or(0), r.bytes, r.skipped, after);
            assert!(after.unwrap_or(0) < i.bytes.unwrap_or(0) || i.bytes == Some(0), "{id} didn't shrink");
        }
    }

    #[test]
    fn catalog_lists_only_places_that_exist() {
        let d = scratch("catalog");
        let p = places(&d);
        file(&p.temp.join("a.tmp"), 1000, 10);
        file(&p.local.join("D3DSCache/x.bin"), 500, 1);
        file(&p.roaming.join("discord/Cache/Cache_Data/f_0001"), 200, 0);
        file(&p.windir.join("SoftwareDistribution/Download/abc/update.cab"), 300, 20);
        let items = catalog(&p, false, SystemTime::now());
        let ids: Vec<&str> = items.iter().map(|i| i.id).collect();
        assert!(ids.contains(&"temp") && ids.contains(&"shaders") && ids.contains(&"discord") && ids.contains(&"wu-downloads"), "{ids:?}");
        assert!(!ids.contains(&"chrome"), "Chrome isn't installed here");
        let wu = items.iter().find(|i| i.id == "wu-downloads").unwrap();
        assert!(wu.admin);
        let temp = items.iter().find(|i| i.id == "temp").unwrap();
        assert_eq!((temp.bytes, temp.group), (Some(1000), Group::Safe));
        assert_eq!(items.iter().find(|i| i.id == "shaders").unwrap().group, Group::Choice);
        fs::remove_dir_all(&d).unwrap();
    }
}
