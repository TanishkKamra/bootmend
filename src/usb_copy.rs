//! Copying the person's own folders to a USB drive when the drive they live on is failing.
//! Nothing on the PC is moved or deleted, and nothing already on the USB drive is overwritten.

use crate::rules::plural;
use crate::util::powershell;
use anyhow::{anyhow, Result};
use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

/// The folders, then each drive plugged in by USB or as a memory card, never one Windows starts from.
const LOOK: &str = r#"foreach ($n in 'MyDocuments', 'Desktop', 'MyPictures') { "F`t$n`t$([Environment]::GetFolderPath($n))" }
Get-Volume | Where-Object { $_.DriveLetter -and $_.Size -gt 0 } | ForEach-Object {
  $d = Get-Partition -DriveLetter $_.DriveLetter -ErrorAction SilentlyContinue | Get-Disk -ErrorAction SilentlyContinue
  if ("$($_.DriveType)" -eq 'Removable' -or ($d -and "$($d.BusType)" -eq 'USB' -and -not $d.IsSystem -and -not $d.IsBoot)) {
    $name = if ($_.FileSystemLabel) { $_.FileSystemLabel } elseif ($d) { $d.FriendlyName } else { '' }
    "D`t$($_.DriveLetter)`t$($_.SizeRemaining)`t$($_.AllocationUnitSize)`t$name"
  }
}"#;

/// A folder to copy, under the name it gets inside the copy.
#[derive(Debug, Clone, PartialEq)]
pub struct Folder {
    pub name: String,
    pub path: PathBuf,
}

/// A drive the files can go to.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub letter: char,
    pub name: String,
    pub free: u64,
    /// Bytes per cluster: every file takes up whole ones.
    pub cluster: u64,
}

/// What's in the folders.
#[derive(Debug, Clone, Default)]
pub struct Found {
    files: Vec<Item>,
    pub bytes: u64,
    /// OneDrive files kept only online. Copying them would download them first.
    pub cloud: usize,
    /// Folders Windows wouldn't let Bootmend read.
    pub unreadable: usize,
}

#[derive(Debug, Clone)]
struct Item {
    from: PathBuf,
    /// Where it goes inside the copy, starting with the folder's name.
    rel: PathBuf,
    len: u64,
}

impl Found {
    /// One made-up file of `bytes`, for the off-screen screenshots.
    pub fn sample(bytes: u64) -> Found {
        Found { files: vec![Item { from: PathBuf::new(), rel: PathBuf::new(), len: bytes }], bytes, ..Found::default() }
    }

    pub fn files(&self) -> usize {
        self.files.len()
    }
}

/// How a copy went.
#[derive(Debug)]
pub struct Copied {
    pub dir: PathBuf,
    pub files: usize,
    pub bytes: u64,
    /// Where each file that couldn't be copied sits inside the copy.
    pub failed: Vec<String>,
    pub stopped: bool,
}

/// The person's Documents, Desktop and Pictures, and the USB drives plugged in now. Only reads.
pub fn look() -> Result<(Vec<Folder>, Vec<Target>)> {
    let out = powershell(LOOK, Duration::from_secs(60))?;
    parse_look(&out.stdout)
}

fn parse_look(text: &str) -> Result<(Vec<Folder>, Vec<Target>)> {
    let rows: Vec<Vec<&str>> = text.lines().map(|l| l.split('\t').collect()).collect();
    if !rows.iter().any(|r| r[0] == "F") {
        return Err(anyhow!("Windows didn't say where your folders are"));
    }
    let mut folders: Vec<Folder> = vec![];
    for r in rows.iter().filter(|r| r[0] == "F" && r.len() >= 3 && !r[2].trim().is_empty()) {
        let name = match r[1] {
            "MyDocuments" => "Documents",
            "MyPictures" => "Pictures",
            other => other,
        };
        let f = Folder { name: name.into(), path: PathBuf::from(r[2].trim()) };
        if folders.iter().any(|k| inside(&f.path, &k.path)) {
            continue;
        }
        folders.retain(|k| !inside(&k.path, &f.path));
        folders.push(f);
    }
    let home: Vec<char> = folders.iter().filter_map(|f| letter(&f.path)).collect();
    let drives = rows
        .iter()
        .filter(|r| r[0] == "D" && r.len() >= 3)
        .filter_map(|r| {
            let l = r[1].chars().next()?.to_ascii_uppercase();
            let cluster = r.get(3).and_then(|c| c.parse().ok()).filter(|c| *c > 0).unwrap_or(4096);
            let name = r.get(4).map(|n| n.trim()).filter(|n| !n.is_empty()).unwrap_or("USB drive");
            Some(Target { letter: l, name: name.into(), free: r[2].parse().ok()?, cluster })
        })
        .filter(|t| !home.contains(&t.letter))
        .collect();
    Ok((folders, drives))
}

fn letter(p: &Path) -> Option<char> {
    let s = p.to_str()?;
    (s.as_bytes().get(1) == Some(&b':')).then(|| s.chars().next().map(|c| c.to_ascii_uppercase())).flatten()
}

/// Whether `a` is `b` or inside it. Windows paths ignore case.
fn inside(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().trim_end_matches('\\').to_lowercase();
    let (a, b) = (norm(a), norm(b));
    a == b || a.starts_with(&format!("{b}\\"))
}

/// Walks the folders. Links and junctions are skipped: they point at folders copied in their own
/// right, or somewhere else entirely.
pub fn survey(folders: &[Folder]) -> Found {
    let mut found = Found::default();
    for f in folders {
        let mut stack = vec![(f.path.clone(), PathBuf::from(&f.name))];
        while let Some((dir, rel)) = stack.pop() {
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                Err(e) => {
                    log!("copy", "couldn't read a folder in {}: {e}", f.name);
                    found.unreadable += 1;
                    continue;
                }
            };
            for e in entries {
                let e = match e {
                    Ok(e) => e,
                    Err(err) => {
                        log!("copy", "couldn't read an entry in {}: {err}", f.name);
                        found.unreadable += 1;
                        continue;
                    }
                };
                // Both come from the folder listing, so an online-only file isn't opened, or downloaded.
                let (ft, md) = match (e.file_type(), e.metadata()) {
                    (Ok(t), Ok(m)) => (t, m),
                    (Err(err), _) | (_, Err(err)) => {
                        log!("copy", "couldn't read an entry in {}: {err}", f.name);
                        found.unreadable += 1;
                        continue;
                    }
                };
                let rel = rel.join(e.file_name());
                if ft.is_symlink() {
                    continue;
                }
                if ft.is_dir() {
                    stack.push((e.path(), rel));
                } else if is_cloud(md.file_attributes()) {
                    found.cloud += 1;
                } else {
                    found.bytes += md.len();
                    found.files.push(Item { from: e.path(), rel, len: md.len() });
                }
            }
        }
    }
    found
}

/// Offline, or Windows fetches it from the cloud when it's opened.
fn is_cloud(attrs: u32) -> bool {
    const OFFLINE: u32 = 0x1000;
    const RECALL_ON_OPEN: u32 = 0x0004_0000;
    const RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;
    attrs & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0
}

/// The room the files take on a drive with `cluster`-byte clusters.
pub fn needed(found: &Found, cluster: u64) -> u64 {
    let c = if cluster == 0 { 4096 } else { cluster };
    found.files.iter().map(|f| f.len.div_ceil(c) * c).sum()
}

/// The copy's folder on `root`, named for when it was made.
pub fn fresh_dir(root: &Path, now: chrono::NaiveDateTime) -> Result<PathBuf> {
    let base = format!("Bootmend copy {}", now.format("%Y-%m-%d %H.%M"));
    for n in 1..100 {
        let dir = root.join(if n == 1 { base.clone() } else { format!("{base} ({n})") });
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(anyhow!("Bootmend couldn't make a folder on the drive: {e}")),
        }
    }
    Err(anyhow!("The drive already has 99 copies from this minute"))
}

/// Copies everything found into `dir`, keeping `done` at the bytes gone through so far. A file
/// that can't be read is listed and skipped; a full or missing drive stops the copy.
pub fn copy_to(found: &Found, dir: &Path, done: &AtomicU64, stop: &AtomicBool) -> Result<Copied> {
    std::fs::create_dir_all(dir).map_err(|e| anyhow!("Bootmend couldn't write to the drive: {e}"))?;
    log!("copy", "copying {} files to {}", found.files.len(), dir.display());
    let mut c = Copied { dir: dir.to_path_buf(), files: 0, bytes: 0, failed: vec![], stopped: false };
    let mut buf = vec![0u8; 1 << 20];
    let mut through = 0;
    for it in &found.files {
        if stop.load(Ordering::Relaxed) {
            c.stopped = true;
            break;
        }
        match copy_one(&it.from, &dir.join(&it.rel), &mut buf, done, stop) {
            Ok(n) => {
                c.files += 1;
                c.bytes += n;
            }
            Err(_) if stop.load(Ordering::Relaxed) => {
                c.stopped = true;
                break;
            }
            Err(e) if e.kind() == ErrorKind::StorageFull => {
                log!("copy", "the drive filled up after {} files", c.files);
                return Err(anyhow!("The drive is full, so copying stopped after {}", plural(c.files, "file", "files")));
            }
            Err(e) => {
                if !dir.exists() {
                    log!("copy", "the drive went away after {} files: {e}", c.files);
                    return Err(anyhow!("The drive was unplugged or stopped answering, so copying stopped after {}", plural(c.files, "file", "files")));
                }
                log!("copy", "couldn't copy a file: {e}");
                c.failed.push(it.rel.display().to_string());
            }
        }
        through += it.len;
        done.store(through, Ordering::Relaxed);
    }
    log!("copy", "copied {} files ({} bytes); {} couldn't be copied; stopped: {}", c.files, c.bytes, c.failed.len(), c.stopped);
    Ok(c)
}

/// Copies one file and its date. A file already there is left alone; a half-made one is removed.
fn copy_one(from: &Path, to: &Path, buf: &mut [u8], done: &AtomicU64, stop: &AtomicBool) -> std::io::Result<u64> {
    let mut src = File::open(from)?;
    let modified = src.metadata()?.modified()?;
    if let Some(p) = to.parent() {
        std::fs::create_dir_all(p)?;
    }
    let mut dst = File::options().write(true).create_new(true).open(to)?;
    let r = fill(&mut src, &mut dst, buf, done, stop).and_then(|n| dst.set_modified(modified).map(|()| n));
    if r.is_err() {
        drop(dst);
        if let Err(e) = std::fs::remove_file(to) {
            log!("copy", "couldn't remove a half-copied file: {e}");
        }
    }
    r
}

fn fill(src: &mut File, dst: &mut File, buf: &mut [u8], done: &AtomicU64, stop: &AtomicBool) -> std::io::Result<u64> {
    let mut n = 0;
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err(std::io::Error::new(ErrorKind::Interrupted, "stopped"));
        }
        let k = match src.read(buf) {
            Ok(k) => k,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if k == 0 {
            return Ok(n);
        }
        dst.write_all(&buf[..k])?;
        n += k as u64;
        done.fetch_add(k as u64, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    use std::time::{Duration, SystemTime};

    /// A throwaway folder in the temp folder, removed when the test ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let d = std::env::temp_dir().join(format!("bootmend-copy-{name}-{}", std::process::id()));
            if d.exists() {
                std::fs::remove_dir_all(&d).unwrap();
            }
            std::fs::create_dir_all(&d).unwrap();
            Scratch(d)
        }

        fn file(&self, rel: &str, bytes: usize) -> PathBuf {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, vec![7u8; bytes]).unwrap();
            p
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            if let Err(e) = std::fs::remove_dir_all(&self.0) {
                eprintln!("couldn't remove {}: {e}", self.0.display());
            }
        }
    }

    fn folders(s: &Scratch) -> Vec<Folder> {
        vec![Folder { name: "Documents".into(), path: s.0.join("docs") }, Folder { name: "Desktop".into(), path: s.0.join("desk") }]
    }

    #[test]
    fn reads_the_folders_and_only_drives_plugged_in_by_usb() {
        let out = "F\tMyDocuments\tC:\\Users\\Sam\\OneDrive\\Documents\r\nF\tDesktop\tC:\\Users\\Sam\\Desktop\r\nF\tMyPictures\t\r\n\
                   D\tE\t62000000000\t131072\tSANDISK\r\nD\tF\t1000\t0\t\r\nD\tC\t5000\t4096\tWindows\r\nnoise\r\n";
        let (f, d) = parse_look(out).unwrap();
        assert_eq!(f.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["Documents", "Desktop"], "a missing folder is left out");
        assert_eq!(f[0].path, PathBuf::from(r"C:\Users\Sam\OneDrive\Documents"));
        assert_eq!(d.len(), 2, "never the drive the folders are on");
        assert_eq!(d[0], Target { letter: 'E', name: "SANDISK".into(), free: 62_000_000_000, cluster: 131_072 });
        assert_eq!((d[1].name.as_str(), d[1].cluster), ("USB drive", 4096), "no label or cluster size: plain defaults");
        assert!(parse_look("").is_err(), "PowerShell didn't run");
    }

    /// Only reads this PC's folder locations and drive list. Nothing is copied.
    #[test]
    #[ignore = "starts PowerShell; run with --ignored"]
    fn looks_at_this_pcs_folders_and_drives() {
        let (f, d) = look().unwrap();
        assert!(!f.is_empty() && f.iter().all(|x| x.path.is_dir()), "{f:?}");
        assert!(d.iter().all(|t| t.letter.is_ascii_uppercase() && t.cluster > 0), "{d:?}");
        let found = survey(&f);
        eprintln!("{} folders, {} drives, {} files, {} online-only, {} unreadable", f.len(), d.len(), found.files(), found.cloud, found.unreadable);
    }

    #[test]
    fn a_folder_inside_another_is_copied_once() {
        let out = "F\tMyDocuments\tC:\\Users\\Sam\\Documents\nF\tMyPictures\tC:\\Users\\Sam\\Documents\\Pictures\nF\tDesktop\tc:\\users\\sam\\documents\n";
        let (f, _) = parse_look(out).unwrap();
        assert_eq!(f.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["Documents"]);
    }

    #[test]
    fn online_only_onedrive_files_are_left_alone() {
        assert!(is_cloud(0x0040_0000), "recall on data access");
        assert!(is_cloud(0x0004_0000), "recall on open");
        assert!(is_cloud(0x1000), "offline");
        assert!(!is_cloud(0x20 | 0x400), "a OneDrive file kept on this PC is copied");
    }

    #[test]
    fn room_needed_counts_whole_clusters() {
        let s = Scratch::new("room");
        s.file("docs/a.txt", 1);
        s.file("docs/b.bin", 5000);
        let found = survey(&folders(&s));
        assert_eq!((found.files(), found.bytes), (2, 5001));
        assert_eq!(needed(&found, 4096), 4096 + 8192);
        assert_eq!(needed(&found, 0), 4096 + 8192, "an unknown cluster size counts as 4 KB");
    }

    #[test]
    fn copies_every_file_with_its_date_and_overwrites_nothing() {
        let s = Scratch::new("copy");
        let old = SystemTime::now() - Duration::from_secs(86_400 * 30);
        let a = s.file("docs/Taxes/2025.pdf", 3 * 1024 * 1024 + 5);
        std::fs::File::options().write(true).open(&a).unwrap().set_modified(old).unwrap();
        s.file("desk/note.txt", 10);
        std::fs::create_dir_all(s.0.join("desk/Empty")).unwrap();
        let found = survey(&folders(&s));
        assert_eq!((found.files(), found.unreadable), (2, 0));

        let usb = s.0.join("usb");
        std::fs::create_dir_all(&usb).unwrap();
        let now = chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap().and_hms_opt(13, 42, 0).unwrap();
        let dir = fresh_dir(&usb, now).unwrap();
        assert_eq!(dir, usb.join("Bootmend copy 2026-11-16 13.42"));
        assert_eq!(fresh_dir(&usb, now).unwrap(), usb.join("Bootmend copy 2026-11-16 13.42 (2)"), "a second copy that minute gets its own folder");

        let done = AtomicU64::new(0);
        let c = copy_to(&found, &dir, &done, &AtomicBool::new(false)).unwrap();
        assert_eq!((c.files, c.bytes, c.failed.len(), c.stopped), (2, found.bytes, 0, false));
        assert_eq!(done.load(Ordering::Relaxed), found.bytes);
        let copied = dir.join(r"Documents\Taxes\2025.pdf");
        assert_eq!(std::fs::read(&copied).unwrap(), std::fs::read(&a).unwrap());
        assert_eq!(std::fs::metadata(&copied).unwrap().modified().unwrap(), old);
        assert!(dir.join(r"Desktop\note.txt").is_file());
        assert!(a.is_file(), "the original stays where it was");

        let again = copy_to(&found, &dir, &AtomicU64::new(0), &AtomicBool::new(false)).unwrap();
        assert_eq!(again.failed.len(), 2, "files already there are never overwritten");
        assert_eq!(std::fs::read(&copied).unwrap().len(), 3 * 1024 * 1024 + 5);
    }

    #[test]
    fn a_file_that_cant_be_read_is_listed_and_the_rest_still_copied() {
        let s = Scratch::new("gone");
        let gone = s.file("docs/gone.txt", 10);
        s.file("docs/kept.txt", 10);
        let found = survey(&folders(&s));
        std::fs::remove_file(&gone).unwrap();
        let usb = s.0.join("usb");
        let done = AtomicU64::new(0);
        let c = copy_to(&found, &usb, &done, &AtomicBool::new(false)).unwrap();
        assert_eq!((c.files, c.failed.clone()), (1, vec![r"Documents\gone.txt".to_string()]));
        assert_eq!(done.load(Ordering::Relaxed), found.bytes, "the bar still reaches the end");
        assert!(!usb.join(r"Documents\gone.txt").exists(), "no half-made file is left behind");
    }

    #[test]
    fn stopping_keeps_what_was_copied() {
        let s = Scratch::new("stop");
        s.file("docs/a.txt", 10);
        let found = survey(&folders(&s));
        let c = copy_to(&found, &s.0.join("usb"), &AtomicU64::new(0), &AtomicBool::new(true)).unwrap();
        assert!(c.stopped && c.files == 0 && c.failed.is_empty());
    }
}
