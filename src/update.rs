//! Keeps Bootmend up to date from its GitHub releases, and only from them.
//!
//! Each release carries bootmend.exe and bootmend.exe.minisig, a minisign signature the release
//! workflow makes with Bootmend's release key. The signature's signed comment says
//! "bootmend X.Y.Z". The daily check reads that comment from the latest release to learn the
//! newest version. Nothing is installed unless the downloaded exe verifies against the public key
//! below and its signed comment names the version that was asked for, so a tampered download, or
//! an older release passed off as a newer one, is refused. The check alone can't verify the
//! comment without the exe, so a forged answer could at most offer an update that then fails.
//!
//! Downloads go through Windows' own curl.exe, over HTTPS only, redirects included.

use anyhow::{anyhow, bail, Context, Result};
use minisign_verify::{PublicKey, Signature};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The public half of Bootmend's release signing key. The secret half lives only in the release
/// workflow's "release" environment, which only version tags can use.
const PUBLIC_KEY: &str = "RWTydMpxLepe5WOTSewxS1IiOxN889ygXhIEBVyedug0Jc3thANAWypQ";
const REPO: &str = env!("CARGO_PKG_REPOSITORY");
const EXE: &str = "bootmend.exe";
const SIG: &str = "bootmend.exe.minisig";
/// Far bigger than a release will be, so a runaway download stops.
const MAX_EXE: u64 = 64 << 20;
const MAX_SIG: u64 = 4096;
const SIGNED_PREFIX: &str = "bootmend ";
const NOT_SIGNED: &str = "the download isn't signed by Bootmend's release key, so it wasn't installed";

/// A release version: three numbers, compared in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(u32, u32, u32);

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

impl Version {
    /// "0.1.2" and nothing looser: three numbers of one to five digits.
    pub fn parse(s: &str) -> Option<Version> {
        let mut parts = s.split('.').map(|p| if (1..=5).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()) { p.parse().ok() } else { None });
        let v = Version(parts.next()??, parts.next()??, parts.next()??);
        parts.next().is_none().then_some(v)
    }

    pub fn current() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).expect("Cargo.toml's version is three plain numbers")
    }
}

/// The version a signature's signed comment names.
fn signed_version(sig: &Signature) -> Option<Version> {
    sig.trusted_comment().strip_prefix(SIGNED_PREFIX).and_then(Version::parse)
}

/// The version a release signature names, when it's newer than `current`.
fn newer_than(current: Version, sig_text: &str) -> Result<Option<Version>> {
    let v = Signature::decode(sig_text).ok().as_ref().and_then(signed_version).ok_or_else(|| anyhow!("GitHub's answer wasn't a Bootmend release"))?;
    Ok((v > current).then_some(v))
}

/// The exe must verify against `key` (its bytes and the signed comment both), and the signed
/// comment must name the version asked for.
fn verify_with(key: &str, exe: &[u8], sig_text: &str, want: Version) -> Result<()> {
    let key = PublicKey::from_base64(key).map_err(|e| anyhow!("Bootmend's release key couldn't be read: {e}"))?;
    let sig = Signature::decode(sig_text).map_err(|_| anyhow!(NOT_SIGNED))?;
    key.verify(exe, &sig, false).map_err(|_| anyhow!(NOT_SIGNED))?;
    if signed_version(&sig) != Some(want) {
        bail!("the download is signed for a different version, so it wasn't installed");
    }
    Ok(())
}

/// Windows lets a running exe be renamed but not overwritten, so the new exe goes in beside it,
/// the running one moves aside to .old, and the new one takes its name. If a step fails, the
/// running exe is left (or put back) where it was.
fn put_in_place(exe: &Path, bin: &[u8]) -> Result<()> {
    let (new, old) = (sibling(exe, "new"), sibling(exe, "old"));
    let folder = exe.parent().unwrap_or(exe).display().to_string();
    let cant = |e: std::io::Error| {
        log!("update", "couldn't replace {}: {e}", exe.display());
        anyhow!("Windows wouldn't let Bootmend replace itself in {folder}, so download the new version from GitHub instead")
    };
    let discard = || {
        if let Err(e) = std::fs::remove_file(&new) {
            log!("update", "couldn't remove {}: {e}", new.display());
        }
    };
    std::fs::write(&new, bin).map_err(cant)?;
    match std::fs::remove_file(&old) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            discard();
            return Err(cant(e));
        }
    }
    if let Err(e) = std::fs::rename(exe, &old) {
        discard();
        return Err(cant(e));
    }
    if let Err(e) = std::fs::rename(&new, exe) {
        if let Err(back) = std::fs::rename(&old, exe) {
            log!("update", "couldn't put {} back: {back}", exe.display());
        }
        discard();
        return Err(cant(e));
    }
    Ok(())
}

/// `bootmend.exe` → `bootmend.exe.<ext>`, in the same folder so a rename never has to copy.
fn sibling(exe: &Path, ext: &str) -> PathBuf {
    let mut s = exe.as_os_str().to_owned();
    s.push(".");
    s.push(ext);
    PathBuf::from(s)
}

fn curl_args(url: &str, out: &Path, max: u64, secs: u64) -> Vec<String> {
    let mut a: Vec<String> = [
        // First, so a curl config file in the profile can't loosen anything below.
        "--disable",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--tlsv1.2",
        "--fail",
        "--silent",
        "--show-error",
        "--location",
        "--max-redirs",
        "5",
    ]
    .map(String::from)
    .into();
    a.extend(["--max-filesize".into(), max.to_string(), "--max-time".into(), secs.to_string()]);
    a.extend(["--user-agent".into(), format!("Bootmend/{}", env!("CARGO_PKG_VERSION"))]);
    a.extend(["--output".into(), out.to_string_lossy().into_owned(), url.into()]);
    a
}

/// curl's exit codes in words.
fn curl_words(code: Option<i32>) -> &'static str {
    match code {
        Some(6 | 7) => "Bootmend couldn't reach GitHub. Is the PC online?",
        Some(28) => "GitHub took too long to answer",
        Some(22) => "GitHub didn't have the release files",
        Some(63) => "the download was bigger than an update should be",
        Some(35 | 60) => "the secure connection to GitHub failed",
        _ => "the download didn't finish",
    }
}

/// The release's page on GitHub, with its notes and files.
pub fn release_page(v: Version) -> String {
    format!("{REPO}/releases/tag/v{v}")
}

fn file_url(v: Version, file: &str) -> String {
    format!("{REPO}/releases/download/v{v}/{file}")
}

fn latest_url(file: &str) -> String {
    format!("{REPO}/releases/latest/download/{file}")
}

fn verify(exe: &[u8], sig_text: &str, want: Version) -> Result<()> {
    verify_with(PUBLIC_KEY, exe, sig_text, want)
}

/// Windows' own curl, not whichever curl.exe comes first on the PATH.
fn curl() -> PathBuf {
    crate::sys::system_dir().join("curl.exe")
}

/// Downloads `url` through `out` and returns its bytes. The file is removed again either way.
fn fetch(url: &str, out: &Path, max: u64, secs: u64) -> Result<Vec<u8>> {
    let args = curl_args(url, out, max, secs);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let run = crate::util::run_hidden(&curl().to_string_lossy(), &args, Duration::from_secs(secs + 10));
    let read = match run {
        Err(_) => Err(anyhow!("Windows' download tool, curl, couldn't start")),
        Ok(o) if o.timed_out => Err(anyhow!(curl_words(Some(28)))),
        Ok(o) if o.code != Some(0) => Err(anyhow!(curl_words(o.code))),
        Ok(_) => read_capped(out, max),
    };
    match std::fs::remove_file(out) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => log!("update", "couldn't remove {}: {e}", out.display()),
    }
    read
}

fn read_capped(path: &Path, max: u64) -> Result<Vec<u8>> {
    let len = std::fs::metadata(path).with_context(|| format!("reading {}", path.display()))?.len();
    if len > max {
        bail!(curl_words(Some(63)));
    }
    std::fs::read(path).with_context(|| format!("reading {}", path.display()))
}

/// Updates wait for a normal window: as administrator, the new version would start with full
/// rights from a folder the user's own apps can change.
pub const NOT_AS_ADMIN: &str = "Bootmend checks for updates when it isn't running as administrator.";

/// Asks GitHub for the newest release and returns its version if it's newer than this one.
pub fn check() -> Result<Option<Version>> {
    if crate::sys::elevated() {
        bail!(NOT_AS_ADMIN);
    }
    let dir = crate::util::data_dir("update")?;
    let sig = fetch(&latest_url(SIG), &dir.join(SIG), MAX_SIG, 20)?;
    let found = newer_than(Version::current(), &String::from_utf8_lossy(&sig))?;
    match found {
        Some(v) => log!("update", "Bootmend {v} is out"),
        None => log!("update", "Bootmend {} is the newest version", Version::current()),
    }
    Ok(found)
}

/// Downloads version `v`, checks its signature, and puts it in place of the running exe. The old
/// exe stays beside it as bootmend.exe.old until the next start. Returns the exe to start.
pub fn install(v: Version) -> Result<PathBuf> {
    if v <= Version::current() {
        bail!("Bootmend {v} isn't newer than this version");
    }
    if crate::sys::elevated() {
        bail!(NOT_AS_ADMIN);
    }
    let exe = std::env::current_exe().context("finding Bootmend's own file")?;
    if crate::elevate::others_can_change(&exe) {
        bail!(
            "Bootmend can't update itself in this folder, because other accounts on this PC can change files in it. Move {} to your Downloads folder, then try again.",
            crate::elevate::exe_name(&exe)
        );
    }
    let dir = crate::util::data_dir("update")?;
    let sig = fetch(&file_url(v, SIG), &dir.join(SIG), MAX_SIG, 30)?;
    let bin = fetch(&file_url(v, EXE), &dir.join(EXE), MAX_EXE, 600)?;
    verify(&bin, &String::from_utf8_lossy(&sig), v)?;
    put_in_place(&exe, &bin)?;
    log!("update", "installed Bootmend {v} in {}", exe.display());
    Ok(exe)
}

/// Starts the new copy. The caller then closes this one.
pub fn restart(exe: &Path) -> Result<()> {
    std::process::Command::new(exe).spawn().map(|_| ()).context("starting the new version")
}

/// Removes the exe an update replaced. Runs at start, while the old copy may still be closing, so
/// it keeps trying for a few seconds on its own thread.
pub fn tidy() {
    let Ok(exe) = std::env::current_exe() else { return };
    let old = sibling(&exe, "old");
    if !old.exists() {
        return;
    }
    std::thread::spawn(move || {
        let mut last = None;
        for _ in 0..20 {
            match std::fs::remove_file(&old) {
                Ok(()) => {
                    log!("update", "removed the copy the update replaced");
                    return;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
                Err(e) => last = Some(e),
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        if let Some(e) = last {
            log!("update", "couldn't remove {} yet, so the next start tries again: {e}", old.display());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const V: fn(u32, u32, u32) -> Version = Version;

    fn keys() -> minisign::KeyPair {
        minisign::KeyPair::generate_unencrypted_keypair().unwrap()
    }

    fn sign(kp: &minisign::KeyPair, bin: &[u8], comment: &str) -> String {
        minisign::sign(None, &kp.sk, bin, Some(comment), Some("test")).unwrap().into_string()
    }

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let d = std::env::temp_dir().join(format!("bootmend-update-{name}-{}", std::process::id()));
            if d.exists() {
                std::fs::remove_dir_all(&d).unwrap();
            }
            std::fs::create_dir_all(&d).unwrap();
            Scratch(d)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            if let Err(e) = std::fs::remove_dir_all(&self.0) {
                eprintln!("couldn't remove {}: {e}", self.0.display());
            }
        }
    }

    #[test]
    fn reads_only_plain_three_part_versions() {
        assert_eq!(Version::parse("0.1.0"), Some(V(0, 1, 0)));
        assert_eq!(Version::parse("12.0.345"), Some(V(12, 0, 345)));
        for bad in ["", "1", "1.2", "1.2.3.4", "1..3", "1.2.x", "+1.2.3", " 1.2.3", "1.2.3 ", "1.2.-3", "123456.0.0", "1.2.3-beta"] {
            assert_eq!(Version::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn compares_versions_by_number() {
        assert!(V(0, 1, 10) > V(0, 1, 9));
        assert!(V(1, 0, 0) > V(0, 99, 99));
        assert!(V(0, 2, 0) > V(0, 1, 5));
        assert_eq!(V(0, 1, 2).to_string(), "0.1.2");
        assert_eq!(Version::current().to_string(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn the_release_key_is_readable() {
        assert!(PublicKey::from_base64(PUBLIC_KEY).is_ok());
    }

    #[test]
    fn installs_only_what_the_release_key_signed_for_that_version() {
        let kp = keys();
        let key = kp.pk.to_base64();
        let bin = b"the new bootmend".to_vec();
        let sig = sign(&kp, &bin, "bootmend 0.2.0");
        assert!(verify_with(&key, &bin, &sig, V(0, 2, 0)).is_ok());

        let mut tampered = bin.clone();
        tampered[0] ^= 1;
        assert_eq!(verify_with(&key, &tampered, &sig, V(0, 2, 0)).unwrap_err().to_string(), NOT_SIGNED);

        let stranger = keys();
        assert_eq!(verify_with(&stranger.pk.to_base64(), &bin, &sig, V(0, 2, 0)).unwrap_err().to_string(), NOT_SIGNED);
        let forged = sign(&stranger, &bin, "bootmend 0.2.0");
        assert_eq!(verify_with(&key, &bin, &forged, V(0, 2, 0)).unwrap_err().to_string(), NOT_SIGNED);

        // A real but different release can't be passed off as the one asked for.
        let e = verify_with(&key, &bin, &sig, V(0, 3, 0)).unwrap_err().to_string();
        assert!(e.contains("different version"), "{e}");
        let loose = sign(&kp, &bin, "bootmend 0.2");
        assert!(verify_with(&key, &bin, &loose, V(0, 2, 0)).is_err());

        // A signed comment edited afterwards breaks the signature.
        let edited = sig.replace("bootmend 0.2.0", "bootmend 0.3.0");
        assert_eq!(verify_with(&key, &bin, &edited, V(0, 3, 0)).unwrap_err().to_string(), NOT_SIGNED);
        assert_eq!(verify_with(&key, &bin, "not a signature", V(0, 2, 0)).unwrap_err().to_string(), NOT_SIGNED);
    }

    #[test]
    fn the_check_offers_only_a_newer_version() {
        let kp = keys();
        let sig = sign(&kp, b"exe", "bootmend 0.2.0");
        assert_eq!(newer_than(V(0, 1, 0), &sig).unwrap(), Some(V(0, 2, 0)));
        assert_eq!(newer_than(V(0, 2, 0), &sig).unwrap(), None);
        assert_eq!(newer_than(V(0, 3, 0), &sig).unwrap(), None);
        assert!(newer_than(V(0, 1, 0), "<html>Not Found</html>").is_err());
        assert!(newer_than(V(0, 1, 0), &sign(&kp, b"exe", "timestamp:1 file:bootmend.exe")).is_err());
    }

    #[test]
    fn swaps_the_exe_and_keeps_the_old_one_beside_it() {
        let s = Scratch::new("swap");
        let exe = s.0.join("bootmend.exe");
        std::fs::write(&exe, b"old").unwrap();
        std::fs::write(sibling(&exe, "old"), b"older").unwrap();
        put_in_place(&exe, b"new").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        assert_eq!(std::fs::read(sibling(&exe, "old")).unwrap(), b"old");
        assert!(!sibling(&exe, "new").exists());
        assert_eq!(sibling(&exe, "old"), s.0.join("bootmend.exe.old"));
    }

    #[test]
    fn leaves_the_exe_alone_when_it_cant_be_swapped() {
        let s = Scratch::new("stuck");
        let exe = s.0.join("bootmend.exe");
        std::fs::write(&exe, b"old").unwrap();
        // The old copy from an earlier update can't be removed.
        std::fs::create_dir(sibling(&exe, "old")).unwrap();
        std::fs::write(sibling(&exe, "old").join("x"), b"x").unwrap();
        let e = put_in_place(&exe, b"new").unwrap_err().to_string();
        assert!(e.contains("download the new version from GitHub"), "{e}");
        assert_eq!(std::fs::read(&exe).unwrap(), b"old");
        assert!(!sibling(&exe, "new").exists());

        // The exe itself is held open by something else.
        std::fs::remove_dir_all(sibling(&exe, "old")).unwrap();
        use std::os::windows::fs::OpenOptionsExt;
        let held = std::fs::File::options().read(true).share_mode(0).open(&exe).unwrap();
        assert!(put_in_place(&exe, b"new").is_err());
        drop(held);
        assert_eq!(std::fs::read(&exe).unwrap(), b"old");
        assert!(!sibling(&exe, "new").exists());
        assert!(!sibling(&exe, "old").exists());
    }

    #[test]
    fn downloads_over_https_only_with_limits() {
        let a = curl_args("https://github.com/x", Path::new(r"C:\t\f.bin"), 4096, 20);
        // --disable must come first, or a curl config file in the profile could change the rules.
        assert_eq!(a[0], "--disable");
        let has = |k: &str, v: &str| a.windows(2).any(|w| w[0] == k && w[1] == v);
        assert!(has("--proto", "=https"));
        assert!(has("--proto-redir", "=https"));
        assert!(has("--max-filesize", "4096"));
        assert!(has("--max-time", "20"));
        assert!(has("--output", r"C:\t\f.bin"));
        assert!(has("--user-agent", &format!("Bootmend/{}", env!("CARGO_PKG_VERSION"))));
        assert!(a.iter().any(|x| x == "--fail"));
        assert!(a.iter().any(|x| x == "--tlsv1.2"));
        assert_eq!(a.last().unwrap(), "https://github.com/x");
        // Windows spells the folder "system32" or "System32"; either is the same folder.
        assert!(curl().to_string_lossy().to_lowercase().ends_with(r"\system32\curl.exe"));
    }

    #[test]
    fn says_why_a_download_failed() {
        assert!(curl_words(Some(6)).contains("online"));
        assert!(curl_words(Some(28)).contains("too long"));
        assert!(curl_words(Some(63)).contains("bigger"));
        assert!(curl_words(Some(60)).contains("secure connection"));
        assert!(!curl_words(None).is_empty());
    }

    #[test]
    fn release_files_come_from_bootmends_own_releases() {
        let v = V(0, 2, 0);
        assert_eq!(file_url(v, EXE), "https://github.com/TanishkKamra/bootmend/releases/download/v0.2.0/bootmend.exe");
        assert_eq!(latest_url(SIG), "https://github.com/TanishkKamra/bootmend/releases/latest/download/bootmend.exe.minisig");
        assert_eq!(release_page(v), "https://github.com/TanishkKamra/bootmend/releases/tag/v0.2.0");
    }
}
