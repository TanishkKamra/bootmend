//! A plain-text record of everything the app does, one file per day in a `logs` folder beside
//! the app. The Windows username, home folder and PC name are swapped for placeholders before
//! anything is written. App, file and extension names stay, since fixing problems needs them.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};
use std::time::Duration;

const KEEP_DAYS: i64 = 14;
/// A runaway loop shouldn't be able to fill the disk.
const MAX_BYTES: u64 = 5 * 1024 * 1024;

/// Write one line to the log: `log!("check", "{} finished", key)`.
macro_rules! log {
    ($area:expr, $($arg:tt)*) => {
        $crate::log::write($area, &format!($($arg)*))
    };
}

struct Log {
    dir: PathBuf,
    day: String,
    file: Option<File>,
    full: bool,
    swaps: Vec<(String, &'static str)>,
}

static LOG: Mutex<Option<Log>> = Mutex::new(None);

/// Start logging. Until this runs (and in tests), `log!` does nothing.
pub fn init() {
    let beside = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("logs")));
    // A copy in a read-only folder such as Program Files can't write beside itself.
    // Only reached when needed: making the fallback folder too early would stop the move from the old app name.
    let fallback = std::iter::once_with(|| crate::util::data_dir("logs").ok()).flatten();
    if beside.into_iter().chain(fallback).any(start_in) {
        catch_crashes();
    }
}

fn start_in(dir: PathBuf) -> bool {
    let day = today();
    let Some(file) = open(&dir, &day) else { return false };
    *lock(&LOG) = Some(Log { dir: dir.clone(), day, file: Some(file), full: false, swaps: private_swaps() });
    prune(&dir, chrono::Local::now().date_naive());
    true
}

/// A crash while the log was held marks it "poisoned", but the log itself is still fine, so keep using it.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Like `lock`, but gives up at once if another thread holds it.
fn try_lock<T>(m: &Mutex<T>) -> Option<MutexGuard<'_, T>> {
    match m.try_lock() {
        Ok(g) => Some(g),
        Err(TryLockError::Poisoned(p)) => Some(p.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}

/// A crash in the windowed app otherwise vanishes without a trace, so record where it happened.
fn catch_crashes() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let at = info.location().map(|l| format!(" at {}:{}", l.file(), l.line())).unwrap_or_default();
        let thread = std::thread::current();
        let msg = info.payload_as_str().unwrap_or("no message");
        let line = format!("thread '{}' crashed{at}: {msg}", thread.name().unwrap_or("unnamed"));
        // The crash may have happened while this thread held the log, so never wait on it for long.
        for _ in 0..50 {
            if let Some(mut g) = try_lock(&LOG) {
                if let Some(l) = g.as_mut() {
                    l.line("crash", &line);
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        default(info);
    }));
}

/// The folder the log is written to, once logging has started.
pub fn dir() -> Option<PathBuf> {
    lock(&LOG).as_ref().map(|l| l.dir.clone())
}

pub fn write(area: &str, msg: &str) {
    if let Some(l) = lock(&LOG).as_mut() {
        l.line(area, msg);
    }
}

impl Log {
    fn line(&mut self, area: &str, msg: &str) {
        let now = chrono::Local::now();
        let day = now.format("%Y-%m-%d").to_string();
        if day != self.day {
            self.file = open(&self.dir, &day);
            self.day = day;
            self.full = false;
        }
        let Some(f) = self.file.as_mut() else { return };
        let time = now.format("%H:%M:%S%.3f");
        // The log is the place errors get reported, so a failed write has nowhere left to go.
        // Past the cap, crashes and errors are still kept: a runaway loop is when they matter most.
        let important = matches!(area, "crash" | "error");
        if !important && (self.full || f.metadata().is_ok_and(|m| m.len() > MAX_BYTES)) {
            if !self.full {
                self.full = true;
                let _ = f.write_all(format!("{time} the log is full for today; only crashes and errors are kept until tomorrow\n").as_bytes());
            }
            return;
        }
        let text = redact(msg.trim_end(), &self.swaps).replace('\n', "\n        ");
        // One write per line, so the window and a scheduled check-up writing at once never mix their lines.
        let _ = f.write_all(format!("{time} #{} {area}: {text}\n", std::process::id()).as_bytes());
    }
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn open(dir: &Path, day: &str) -> Option<File> {
    std::fs::create_dir_all(dir).ok()?;
    OpenOptions::new().create(true).append(true).open(dir.join(format!("bootmend-{day}.log"))).ok()
}

/// Delete this app's own day files older than two weeks. Nothing else in the folder is touched.
fn prune(dir: &Path, today: chrono::NaiveDate) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(day) = name.strip_prefix("bootmend-").and_then(|n| n.strip_suffix(".log")) else { continue };
        let Ok(d) = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d") else { continue };
        if (today - d).num_days() > KEEP_DAYS {
            if let Err(err) = std::fs::remove_file(e.path()) {
                write("log", &format!("couldn't remove the old log {name}: {err}"));
            }
        }
    }
}

/// The personal details to hide, longest first so a home folder is swapped before the bare username.
fn private_swaps() -> Vec<(String, &'static str)> {
    let var = |k: &str| std::env::var(k).ok().filter(|v| v.len() >= 2);
    let mut swaps = vec![];
    if let Some(home) = var("USERPROFILE") {
        swaps.push((home.replace('\\', "/"), "%USERPROFILE%"));
        swaps.push((home, "%USERPROFILE%"));
    }
    if let Some(pc) = var("COMPUTERNAME") {
        swaps.push((pc, "%COMPUTERNAME%"));
    }
    if let Some(user) = var("USERNAME") {
        swaps.push((user, "%USERNAME%"));
    }
    swaps
}

fn redact(s: &str, swaps: &[(String, &str)]) -> String {
    swaps.iter().fold(s.to_string(), |acc, (from, to)| replace_ignoring_case(&acc, from, to))
}

/// Windows paths and names ignore case, so "c:\users\sam" must be hidden as well as "C:\Users\Sam".
/// Only whole words match, so a user called Sam doesn't turn "Samsung" into "%USERNAME%sung".
/// Letters in any language join a word ("Zoë"); punctuation such as ’ or — ends one.
fn replace_ignoring_case(s: &str, from: &str, to: &str) -> String {
    let (hay, pat) = (s.as_bytes(), from.as_bytes());
    if pat.is_empty() {
        return s.to_string();
    }
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut out = String::with_capacity(s.len());
    let (mut i, mut last) = (0, 0);
    while i + pat.len() <= hay.len() {
        let end = i + pat.len();
        let whole = s.is_char_boundary(i) && s.is_char_boundary(end) && hay[i..end].eq_ignore_ascii_case(pat);
        if whole && !word(s[..i].chars().next_back()) && !word(s[end..].chars().next()) {
            out.push_str(&s[last..i]);
            out.push_str(to);
            i += pat.len();
            last = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&s[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::{prune, redact};

    #[test]
    fn past_the_daily_cap_only_crashes_and_errors_are_kept() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("bootmend-cap-test-{}", std::process::id()));
        let day = super::today();
        let mut f = super::open(&dir, &day).unwrap();
        f.write_all(&vec![b'.'; super::MAX_BYTES as usize + 1]).unwrap();
        let mut log = super::Log { dir: dir.clone(), day: day.clone(), file: Some(f), full: false, swaps: vec![] };
        for (area, msg) in [("check", "dropped"), ("crash", "kept"), ("page", "dropped too"), ("error", "kept too")] {
            log.line(area, msg);
        }
        drop(log);
        let text = std::fs::read_to_string(dir.join(format!("bootmend-{day}.log"))).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        let tail = text.trim_start_matches('.');
        assert_eq!(tail.matches("the log is full").count(), 1, "{tail}");
        assert!(tail.contains("crash: kept") && tail.contains("error: kept too"), "{tail}");
        assert!(!tail.contains("dropped"), "{tail}");
    }

    #[test]
    fn a_crash_on_any_thread_is_written_down_with_where_it_happened() {
        let dir = std::env::temp_dir().join(format!("bootmend-crash-test-{}", std::process::id()));
        assert!(super::start_in(dir.clone()));
        super::catch_crashes();
        let crashed = std::thread::Builder::new().name("worker".into()).spawn(|| panic!("index out of range")).unwrap().join();
        assert!(crashed.is_err());
        let text = std::fs::read_to_string(super::dir().unwrap().join(format!("bootmend-{}.log", super::today()))).unwrap();
        // Let go of the file so the folder can be removed, and stop other tests writing here.
        *super::lock(&super::LOG) = None;
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(text.contains("crash: thread 'worker' crashed at src"), "{text}");
        assert!(text.contains("log.rs:") && text.contains(": index out of range"), "{text}");
    }

    #[test]
    fn the_log_keeps_working_after_a_crash_while_it_was_held() {
        let m = std::sync::Mutex::new(1);
        let _ = std::thread::scope(|s| {
            s.spawn(|| {
                let _held = m.lock();
                panic!("crashed mid-write");
            })
            .join()
        });
        assert!(m.is_poisoned());
        assert_eq!(*super::lock(&m), 1);
        assert_eq!(super::try_lock(&m).map(|g| *g), Some(1));
    }

    #[test]
    fn personal_details_are_hidden_whatever_their_case_or_slashes() {
        let swaps = vec![
            ("C:/Users/Sam".to_string(), "%USERPROFILE%"),
            ("C:\\Users\\Sam".to_string(), "%USERPROFILE%"),
            ("DESKTOP-7Q2".to_string(), "%COMPUTERNAME%"),
            ("Sam".to_string(), "%USERNAME%"),
        ];
        let line = "saved c:\\users\\sam\\Desktop\\r.html and C:/Users/Sam/x on desktop-7q2 for sam, née Zoë";
        assert_eq!(redact(line, &swaps), "saved %USERPROFILE%\\Desktop\\r.html and %USERPROFILE%/x on %COMPUTERNAME% for %USERNAME%, née Zoë");
        assert_eq!(redact("Sam’s PC—“sam”…", &swaps), "%USERNAME%’s PC—“%USERNAME%”…");
        let zo = vec![("Zo".to_string(), "%USERNAME%")];
        assert_eq!(redact("Zoë and Zo’s", &zo), "Zoë and %USERNAME%’s", "a letter with an accent still counts as part of a word");
        let untouched = "Samsung SSD in C:\\Users\\Samantha, by Sam_2";
        assert_eq!(redact(untouched, &swaps), untouched);
    }

    #[test]
    fn only_old_day_files_of_this_app_are_cleared() {
        let dir = std::env::temp_dir().join(format!("bootmend-log-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["bootmend-2026-10-01.log", "bootmend-2026-11-10.log", "notes.log", "bootmend-old.log"] {
            std::fs::write(dir.join(f), "x").unwrap();
        }
        prune(&dir, chrono::NaiveDate::from_ymd_opt(2026, 11, 15).unwrap());
        let mut left: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        left.sort();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(left, ["bootmend-2026-11-10.log", "bootmend-old.log", "notes.log"]);
    }
}
