//! Where Windows keeps its own programs and folders, and what the programs Bootmend starts get to see.
//!
//! An administrator window shares the user's environment variables and per-user settings, which any
//! app the user runs can change. So Windows' folders come from Windows itself, programs start by
//! their full path, and in an administrator window a started program gets a clean set of variables
//! and a temporary folder only administrators can change.

use anyhow::{anyhow, Context, Result};
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use windows::core::{GUID, HSTRING};
use windows::Win32::UI::Shell::{
    FOLDERID_ProgramData, FOLDERID_ProgramFiles, FOLDERID_ProgramFilesCommon, FOLDERID_ProgramFilesCommonX86, FOLDERID_ProgramFilesX86,
};

/// Whether Bootmend is running as administrator. It can't change while it runs.
pub fn elevated() -> bool {
    static E: OnceLock<bool> = OnceLock::new();
    *E.get_or_init(crate::elevate::is_elevated)
}

fn windows_folder(get: unsafe fn(Option<&mut [u16]>) -> u32, fallback: &str) -> PathBuf {
    let mut buf = [0u16; 512];
    // SAFETY: the buffer is writable and its length is passed with it.
    let n = unsafe { get(Some(&mut buf)) } as usize;
    if n == 0 || n >= buf.len() {
        log!("app", "Windows didn't say where its folder is, so Bootmend uses {fallback}");
        return PathBuf::from(fallback);
    }
    PathBuf::from(OsString::from_wide(&buf[..n]))
}

/// `C:\Windows\System32`.
pub fn system_dir() -> &'static Path {
    static D: OnceLock<PathBuf> = OnceLock::new();
    D.get_or_init(|| windows_folder(windows::Win32::System::SystemInformation::GetSystemDirectoryW, r"C:\Windows\System32"))
}

/// `C:\Windows`.
pub fn windows_dir() -> &'static Path {
    static D: OnceLock<PathBuf> = OnceLock::new();
    D.get_or_init(|| windows_folder(windows::Win32::System::SystemInformation::GetSystemWindowsDirectoryW, r"C:\Windows"))
}

/// A folder Windows knows by id, such as Program Files.
pub fn known_folder(id: &GUID) -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    // SAFETY: Windows allocates the string, which is copied and then freed once.
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let path = p.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(p.0 as *const _));
        path
    }
}

/// Program Files (x86), then Program Files: where apps such as Edge and the debugger install.
pub fn program_files() -> Vec<PathBuf> {
    [FOLDERID_ProgramFilesX86, FOLDERID_ProgramFiles].iter().filter_map(known_folder).collect()
}

/// The user's own folder, from Windows' list of profiles, which only administrators can change.
pub fn user_profile() -> Option<PathBuf> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::TOKEN_QUERY;
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows::Win32::UI::Shell::GetUserProfileDirectoryW;
    let mut token = HANDLE::default();
    // SAFETY: this process's own token, closed below; the buffer is as long as Windows asked for.
    unsafe {
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).ok()?;
        let mut len = 0u32;
        let _ = GetUserProfileDirectoryW(token, None, &mut len);
        let mut buf = vec![0u16; len as usize];
        let got = GetUserProfileDirectoryW(token, Some(PWSTR(buf.as_mut_ptr())), &mut len);
        if let Err(e) = CloseHandle(token) {
            log!("app", "couldn't close this process's token: {e}");
        }
        got.ok()?;
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(PathBuf::from(OsString::from_wide(&buf[..end])))
    }
}

/// The full path of a Windows program given by name, such as `dism.exe`. Windows looks for a
/// program started by name beside bootmend.exe first, so a file of the same name in Downloads
/// would run instead.
pub fn program(name: &str) -> PathBuf {
    if name.contains(['\\', '/']) {
        return PathBuf::from(name);
    }
    let file = if Path::new(name).extension().is_some() { name.to_string() } else { format!("{name}.exe") };
    if file.eq_ignore_ascii_case("powershell.exe") {
        system_dir().join(r"WindowsPowerShell\v1.0\powershell.exe")
    } else {
        system_dir().join(file)
    }
}

/// Sets up a program Bootmend starts. It runs in System32, so it can't pick up settings files
/// (the debugger's ntsd.ini, say) from wherever Bootmend was started. In an administrator window
/// it also gets a clean set of variables: ones the user can set, such as PSModulePath, COR_PROFILER
/// or _NT_DEBUGGER_EXTENSION_PATH, would make Windows' tools load the user's code.
pub fn prepare(cmd: &mut Command) -> Result<()> {
    cmd.current_dir(system_dir());
    if elevated() {
        cmd.env_clear().envs(admin_env()?);
    }
    Ok(())
}

/// `C:`, from `C:\Windows`.
fn drive(dir: &Path) -> OsString {
    match dir.components().next() {
        Some(Component::Prefix(p)) => p.as_os_str().to_owned(),
        _ => "C:".into(),
    }
}

fn admin_env() -> Result<Vec<(&'static str, OsString)>> {
    let (sys, win, tmp) = (system_dir(), windows_dir(), tmp_dir()?);
    let folder = |id: &GUID| known_folder(id).ok_or_else(|| anyhow!("Windows didn't say where Program Files or ProgramData is"));
    let (pf, pf86, data) = (folder(&FOLDERID_ProgramFiles)?, folder(&FOLDERID_ProgramFilesX86)?, folder(&FOLDERID_ProgramData)?);
    let (common, common86) = (folder(&FOLDERID_ProgramFilesCommon)?, folder(&FOLDERID_ProgramFilesCommonX86)?);
    let list = |dirs: &[PathBuf]| std::env::join_paths(dirs).context("listing Windows' folders");
    let mut env: Vec<(&'static str, OsString)> = vec![
        ("SystemRoot", win.into()),
        ("windir", win.into()),
        ("SystemDrive", drive(win)),
        ("ComSpec", sys.join("cmd.exe").into()),
        ("PATH", list(&[sys.into(), win.into(), sys.join("Wbem"), sys.join(r"WindowsPowerShell\v1.0")])?),
        ("PATHEXT", ".COM;.EXE;.BAT;.CMD".into()),
        ("TEMP", tmp.clone().into()),
        ("TMP", tmp.clone().into()),
        ("PSModulePath", list(&[sys.join(r"WindowsPowerShell\v1.0\Modules"), pf.join(r"WindowsPowerShell\Modules")])?),
        ("PSModuleAnalysisCachePath", tmp.join("ModuleAnalysisCache").into()),
        ("ProgramData", data.clone().into()),
        ("ALLUSERSPROFILE", data.into()),
        ("ProgramFiles", pf.clone().into()),
        ("ProgramW6432", pf.into()),
        ("ProgramFiles(x86)", pf86.into()),
        ("CommonProgramFiles", common.clone().into()),
        ("CommonProgramW6432", common.into()),
        ("CommonProgramFiles(x86)", common86.into()),
    ];
    // Who the user is and where their own folders are: these say where to look, not what to load.
    for k in [
        "USERNAME",
        "USERDOMAIN",
        "COMPUTERNAME",
        "USERPROFILE",
        "HOMEDRIVE",
        "HOMEPATH",
        "APPDATA",
        "LOCALAPPDATA",
        "PUBLIC",
        "NUMBER_OF_PROCESSORS",
        "PROCESSOR_ARCHITECTURE",
        "PROCESSOR_IDENTIFIER",
        "PROCESSOR_LEVEL",
        "PROCESSOR_REVISION",
        "OS",
    ] {
        if let Some(v) = std::env::var_os(k) {
            env.push((k, v));
        }
    }
    Ok(env)
}

static ADMIN_TMP: OnceLock<std::result::Result<PathBuf, String>> = OnceLock::new();

/// Bootmend's folder for temporary files. Anything the user runs can change
/// `%LOCALAPPDATA%\Bootmend\tmp`, so an administrator window uses a new folder in Windows' own
/// Temp folder instead, which only administrators can change.
pub fn tmp_dir() -> Result<PathBuf> {
    if !elevated() {
        return crate::util::data_dir("tmp");
    }
    ADMIN_TMP.get_or_init(|| private_dir(&windows_dir().join("Temp")).map_err(|e| format!("{e:#}"))).clone().map_err(|e| anyhow!(e))
}

/// Makes a new folder in `parent` that only SYSTEM and Administrators can use.
fn private_dir(parent: &Path) -> Result<PathBuf> {
    use windows::Win32::Foundation::{LocalFree, ERROR_ALREADY_EXISTS, HLOCAL};
    use windows::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
    use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows::Win32::Storage::FileSystem::CreateDirectoryW;
    // SYSTEM and Administrators only, and nothing inherited: Temp lets Users add files to its subfolders.
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: Windows allocates the descriptor, freed below once the folder exists.
    unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(&HSTRING::from("D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)"), SDDL_REVISION_1, &mut sd, None) }
        .context("describing who may use Bootmend's temporary folder")?;
    let sa = SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: sd.0, bInheritHandle: false.into() };
    let mut made = Err(anyhow!("five temporary folder names were already taken in {}", parent.display()));
    for _ in 0..5 {
        // A new name each time: a folder someone else made first is never used.
        let dir = parent.join(format!("Bootmend-{}-{:x}", std::process::id(), crate::util::unique()));
        // SAFETY: `sa` and the descriptor it points to live until after the call.
        match unsafe { CreateDirectoryW(&HSTRING::from(dir.as_os_str()), Some(&sa)) } {
            Ok(()) => {
                made = Ok(dir);
                break;
            }
            Err(e) if e.code() == ERROR_ALREADY_EXISTS.to_hresult() => continue,
            Err(e) => {
                made = Err(anyhow!(e).context(format!("creating {}", dir.display())));
                break;
            }
        }
    }
    // SAFETY: the descriptor came from ConvertStringSecurityDescriptorToSecurityDescriptorW.
    unsafe { LocalFree(Some(HLOCAL(sd.0))) };
    if let Ok(d) = &made {
        log!("app", "made {}, which only administrators can change", d.display());
    }
    made
}

/// Tells Windows not to follow junctions made by anyone without administrator rights, for the rest
/// of this run. Used as administrator, so a folder of the user's swapped for a junction can't send
/// Bootmend into Windows' own folders. Windows 11 22H2 and later have it; elsewhere this fails and
/// the checks made before each change still apply.
pub fn guard_redirections() -> windows::core::Result<()> {
    use windows::Win32::System::Threading::{ProcessRedirectionTrustPolicy, SetProcessMitigationPolicy};
    // PROCESS_MITIGATION_REDIRECTION_TRUST_POLICY is a single set of flags: bit 0 turns it on.
    let enforce: u32 = 1;
    // SAFETY: `enforce` is the four bytes the policy reads, and outlives the call.
    unsafe { SetProcessMitigationPolicy(ProcessRedirectionTrustPolicy, &enforce as *const u32 as *const core::ffi::c_void, 4) }
}

/// Removes the administrator window's temporary folder as Bootmend closes.
pub fn remove_tmp() {
    if let Some(Ok(d)) = ADMIN_TMP.get() {
        if let Err(e) = std::fs::remove_dir_all(d) {
            log!("app", "couldn't remove the temporary folder {}: {e}", d.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn same(a: &Path, b: &Path) -> bool {
        a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy())
    }

    #[test]
    fn windows_folders_come_from_windows() {
        assert!(system_dir().join("kernel32.dll").is_file(), "{}", system_dir().display());
        assert!(same(&windows_dir().join("System32"), system_dir()));
        assert!(program_files().iter().any(|p| p.join("Windows Defender").is_dir() || p.join("Common Files").is_dir()), "{:?}", program_files());
        assert_eq!(drive(Path::new(r"D:\Windows")), "D:");
    }

    #[test]
    fn programs_named_without_a_folder_come_from_system32() {
        assert!(same(&program("dism.exe"), &system_dir().join("dism.exe")));
        assert!(same(&program("cmd"), &system_dir().join("cmd.exe")));
        let ps = program("powershell.exe");
        assert!(same(&ps, &system_dir().join(r"WindowsPowerShell\v1.0\powershell.exe")) && ps.is_file(), "{}", ps.display());
        assert_eq!(program(r"C:\Tools\x.exe"), PathBuf::from(r"C:\Tools\x.exe"));
    }

    #[test]
    fn an_admin_window_passes_on_only_harmless_variables() {
        const KNOWN: [&str; 33] = [
            "SystemRoot",
            "windir",
            "SystemDrive",
            "ComSpec",
            "PATH",
            "PATHEXT",
            "TEMP",
            "TMP",
            "ProgramData",
            "ALLUSERSPROFILE",
            "ProgramFiles",
            "ProgramW6432",
            "ProgramFiles(x86)",
            "CommonProgramFiles",
            "CommonProgramW6432",
            "CommonProgramFiles(x86)",
            "PSModulePath",
            "PSModuleAnalysisCachePath",
            "USERNAME",
            "USERDOMAIN",
            "COMPUTERNAME",
            "USERPROFILE",
            "HOMEDRIVE",
            "HOMEPATH",
            "APPDATA",
            "LOCALAPPDATA",
            "PUBLIC",
            "NUMBER_OF_PROCESSORS",
            "PROCESSOR_ARCHITECTURE",
            "PROCESSOR_IDENTIFIER",
            "PROCESSOR_LEVEL",
            "PROCESSOR_REVISION",
            "OS",
        ];
        let env = admin_env().unwrap();
        for (k, _) in &env {
            assert!(KNOWN.contains(k), "{k}");
        }
        let get = |k: &str| env.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string_lossy().to_string()).unwrap_or_default();
        assert!(same(Path::new(&get("SystemRoot")), windows_dir()));
        assert!(same(Path::new(&get("ComSpec")), &system_dir().join("cmd.exe")));
        // Only Windows' own module folders, never the one in Documents.
        let modules: Vec<String> = get("PSModulePath").split(';').map(str::to_string).collect();
        assert_eq!(modules.len(), 2, "{modules:?}");
        assert!(modules.iter().all(|m| m.ends_with(r"\Modules") && !m.contains("Documents")), "{modules:?}");
        assert!(get("PATH").split(';').all(|d| d.to_lowercase().starts_with(&windows_dir().to_string_lossy().to_lowercase())), "{}", get("PATH"));
    }

    /// Changes how this test process follows junctions, so it runs on its own.
    #[test]
    #[ignore = "changes the test process; run alone with --ignored --exact"]
    fn junctions_made_without_administrator_rights_are_refused() {
        use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessMitigationPolicy, ProcessRedirectionTrustPolicy};
        guard_redirections().unwrap();
        let mut flags = 0u32;
        unsafe { GetProcessMitigationPolicy(GetCurrentProcess(), ProcessRedirectionTrustPolicy, &mut flags as *mut u32 as *mut core::ffi::c_void, 4) }.unwrap();
        assert_eq!(flags & 1, 1, "{flags:#x}");
    }

    #[test]
    fn the_admin_temp_folder_turns_away_everyone_else() {
        use crate::util::{powershell, ps_quote, run_hidden};
        use std::time::Duration;
        let parent = std::env::temp_dir().join(format!("bootmend-private-{}", crate::util::unique()));
        std::fs::create_dir(&parent).unwrap();
        let dir = private_dir(&parent).unwrap();
        let d = dir.to_string_lossy().to_string();
        // Tests run without administrator rights, like any other app the user starts.
        let planted = std::fs::write(dir.join("run.ps1"), "x");
        let listed = std::fs::read_dir(&dir).map(|_| ());
        let sddl = powershell(&format!("(Get-Acl -LiteralPath {}).Sddl", ps_quote(&d)), Duration::from_secs(60)).unwrap().stdout;
        // Whoever made a folder may still change who can use it, so the test can tidy up.
        let reset = run_hidden("icacls.exe", &[&d, "/reset"], Duration::from_secs(30)).unwrap();
        assert_eq!(reset.code, Some(0), "{}", reset.stdout);
        std::fs::remove_dir_all(&parent).unwrap();
        assert_eq!(planted.unwrap_err().kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(listed.unwrap_err().kind(), std::io::ErrorKind::PermissionDenied);
        assert!(sddl.trim().ends_with("D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)"), "{sddl}");
    }
}
