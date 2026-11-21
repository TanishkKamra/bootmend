//! Administrator checks and relaunching with a UAC prompt.

use anyhow::{anyhow, Context, Result};
use std::path::Path;

pub fn is_elevated() -> bool {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut e = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(token, TokenElevation, Some(&mut e as *mut _ as *mut _), std::mem::size_of::<TOKEN_ELEVATION>() as u32, &mut len).is_ok();
        let _ = windows::Win32::Foundation::CloseHandle(token);
        ok && e.TokenIsElevated != 0
    }
}

/// Start this app again as administrator. Returns once Windows has accepted the request.
pub fn relaunch_as_admin(args: &str) -> Result<()> {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let exe = std::env::current_exe()?;
    if others_can_change(&exe) {
        return Err(anyhow!(
            "Bootmend can't run as administrator from this folder, because other accounts on this PC can change files in it. Move {} to your Downloads folder, then try again.",
            exe_name(&exe)
        ));
    }
    log!("admin", "asking Windows to restart Bootmend as administrator");
    let r = unsafe { ShellExecuteW(None, &HSTRING::from("runas"), &HSTRING::from(exe.as_os_str()), &HSTRING::from(args), None, SW_SHOWNORMAL) };
    if r.0 as isize > 32 {
        log!("admin", "administrator restart accepted");
        Ok(())
    } else {
        log!("admin", "administrator restart declined or failed (code {})", r.0 as isize);
        Err(anyhow!("Windows did not start Bootmend as administrator. The prompt may have been cancelled."))
    }
}

/// The exe's own file name, for messages that say to move it.
pub fn exe_name(exe: &Path) -> String {
    exe.file_name().map_or_else(|| "bootmend.exe".into(), |n| n.to_string_lossy().into_owned())
}

/// Whether an account other than this one, SYSTEM, Administrators or Windows' installer could
/// change Bootmend's exe or its folder, or rename the folder away and put another in its place.
/// That account could put its own program in Bootmend's place, which would then run with full
/// rights. When Windows can't say, the answer is yes.
pub fn others_can_change(exe: &Path) -> bool {
    others_can_change_or_err(exe).unwrap_or_else(|e| {
        log!("admin", "couldn't read who can change {}: {e:#}", exe.display());
        true
    })
}

fn others_can_change_or_err(exe: &Path) -> Result<bool> {
    // Where the exe really is: through a junction, the folder above it would be the wrong one.
    let exe: &Path = &std::fs::canonicalize(exe).with_context(|| format!("finding where {} really is", exe.display()))?;
    // Adding, changing or deleting files, or changing who may.
    const CHANGE: u32 = 0x2 | 0x4 | 0x40 | 0x1_0000 | 0x4_0000 | 0x8_0000 | 0x1000_0000 | 0x4000_0000;
    // Deleting or renaming what's in a folder, or changing who may.
    const REPLACE_INSIDE: u32 = 0x40 | 0x4_0000 | 0x8_0000 | 0x1000_0000;
    const TRUSTED_INSTALLER: &str = "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";
    let me = current_user()?;
    // SYSTEM, Administrators, Creator Owner and Owner Rights; the owner is checked on its own.
    // S-1-15 entries are for sandboxed apps, which Windows lets in only on top of their account's
    // own access, so they never let another account in.
    let trusted = |sid: &str| [me.as_str(), "S-1-5-18", "S-1-5-32-544", TRUSTED_INSTALLER, "S-1-3-0", "S-1-3-4"].contains(&sid) || sid.starts_with("S-1-15-");
    let dir = exe.parent().ok_or_else(|| anyhow!("{} has no folder", exe.display()))?;
    let mut checks = vec![(exe, CHANGE), (dir, CHANGE)];
    checks.extend(dir.parent().map(|p| (p, REPLACE_INSIDE)));
    for (path, mask) in checks {
        let Some(Access { owner, allowed }) = who_can(path)? else {
            log!("admin", "anyone can change {}", path.display());
            return Ok(true);
        };
        if !trusted(&owner) {
            log!("admin", "{} belongs to {owner}", path.display());
            return Ok(true);
        }
        if let Some((sid, _)) = allowed.iter().find(|(sid, m)| m & mask != 0 && !trusted(sid)) {
            log!("admin", "{sid} can change {}", path.display());
            return Ok(true);
        }
    }
    Ok(false)
}

/// The SID of the account Bootmend runs as.
fn current_user() -> Result<String> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    let mut token = HANDLE::default();
    // SAFETY: the token handle is closed below; the buffer outlives the reads from it.
    unsafe {
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
        // u64s keep the buffer aligned for TOKEN_USER.
        let mut buf = vec![0u64; 64];
        let mut len = 0u32;
        let got = GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr().cast()), (buf.len() * 8) as u32, &mut len);
        let _ = CloseHandle(token);
        got?;
        sid_text((*(buf.as_ptr() as *const TOKEN_USER)).User.Sid)
    }
}

/// A SID written out, such as "S-1-5-18".
fn sid_text(sid: windows::Win32::Security::PSID) -> Result<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
    let mut text = PWSTR::null();
    // SAFETY: `sid` points at a valid SID; Windows allocates `text`, freed here.
    unsafe {
        ConvertSidToStringSidW(sid, &mut text)?;
        let s = text.to_string();
        LocalFree(Some(HLOCAL(text.0.cast())));
        Ok(s?)
    }
}

/// Who owns a file or folder, and each account allowed in with the access it gets.
struct Access {
    owner: String,
    allowed: Vec<(String, u32)>,
}

/// Who can use a file or folder. `None` when nothing limits it, as on a FAT-formatted USB drive.
fn who_can(path: &Path) -> Result<Option<Access>> {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows::Win32::Security::{
        GetAce, ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    };
    const INHERIT_ONLY: u8 = 0x08;
    let (mut owner, mut dacl, mut sd) = (PSID::default(), std::ptr::null_mut::<ACL>(), PSECURITY_DESCRIPTOR::default());
    // SAFETY: Windows fills the pointers, which point into `sd`; `sd` is freed after the last read.
    unsafe {
        let r = GetNamedSecurityInfoW(
            &HSTRING::from(path.as_os_str()),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            Some(&mut dacl),
            None,
            &mut sd,
        );
        r.ok().with_context(|| format!("reading who can use {}", path.display()))?;
        let read = (|| -> Result<Option<Access>> {
            if dacl.is_null() {
                return Ok(None);
            }
            let mut allowed = vec![];
            for i in 0..(*dacl).AceCount {
                let mut ace = std::ptr::null_mut();
                GetAce(dacl, i as u32, &mut ace)?;
                let header = &*(ace as *const ACE_HEADER);
                if header.AceFlags & INHERIT_ONLY != 0 {
                    continue;
                }
                match header.AceType {
                    // Allowed, and allowed under a condition.
                    0 | 9 => {
                        let a = &*(ace as *const ACCESS_ALLOWED_ACE);
                        allowed.push((sid_text(PSID(&a.SidStart as *const u32 as *mut _))?, a.Mask));
                    }
                    // Denied: only takes access away.
                    1 | 10 => {}
                    // Kinds files don't normally carry: assume the worst.
                    other => allowed.push((format!("an entry of kind {other}"), u32::MAX)),
                }
            }
            Ok(Some(Access { owner: sid_text(owner)?, allowed }))
        })();
        LocalFree(Some(HLOCAL(sd.0)));
        read
    }
}

/// Starts a program, or opens a link, folder or file, as the signed-in user through the desktop's
/// own shell. From an administrator window this keeps administrator rights away from whatever
/// opens it, which the user's apps can choose: links, folders and file types open with the
/// programs the user's settings name.
pub fn open_as_user(file: &str, params: &str, show: i32) -> Result<()> {
    use windows::core::BSTR;
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    use windows::Win32::System::Variant::{VariantClear, VT_BSTR, VT_I4};
    // SAFETY: COM is started for this call, and stopped again only if this call started it.
    let started = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    let r = desktop_shell().and_then(|shell| {
        let mut args = [
            variant(VT_BSTR, |v| v.bstrVal = std::mem::ManuallyDrop::new(BSTR::from(params))),
            variant(VT_BSTR, |v| v.bstrVal = Default::default()),
            variant(VT_BSTR, |v| v.bstrVal = std::mem::ManuallyDrop::new(BSTR::from("open"))),
            variant(VT_I4, |v| v.lVal = show),
        ];
        // SAFETY: the VARIANTs are valid for the call and cleared after it.
        let r = unsafe { shell.ShellExecute(&BSTR::from(file), &args[0], &args[1], &args[2], &args[3]) };
        for a in &mut args {
            // SAFETY: each VARIANT was made by `variant` and is cleared once.
            if let Err(e) = unsafe { VariantClear(a) } {
                log!("admin", "couldn't free a value passed to the shell: {e}");
            }
        }
        r
    });
    if started {
        // SAFETY: matches the CoInitializeEx above; every COM object here is already released.
        unsafe { CoUninitialize() };
    }
    r.with_context(|| format!("the desktop's shell couldn't open {file}"))
}

/// A VARIANT of type `vt`, with its value set by `set`.
fn variant(
    vt: windows::Win32::System::Variant::VARENUM,
    set: impl FnOnce(&mut windows::Win32::System::Variant::VARIANT_0_0_0),
) -> windows::Win32::System::Variant::VARIANT {
    let mut v = windows::Win32::System::Variant::VARIANT::default();
    // SAFETY: the VARIANT starts zeroed; this sets its type and the matching value.
    unsafe {
        let inner = &mut *v.Anonymous.Anonymous;
        inner.vt = vt;
        set(&mut inner.Anonymous);
    }
    v
}

/// The desktop's shell object, which belongs to Explorer and so runs as the signed-in user.
fn desktop_shell() -> windows::core::Result<windows::Win32::UI::Shell::IShellDispatch2> {
    use windows::core::Interface;
    use windows::Win32::System::Com::{CoCreateInstance, IDispatch, IServiceProvider, CLSCTX_LOCAL_SERVER};
    use windows::Win32::System::Variant::VT_I4;
    use windows::Win32::UI::Shell::{
        IShellBrowser, IShellFolderViewDual, IShellView, IShellWindows, SID_STopLevelBrowser, ShellWindows, CSIDL_DESKTOP, SVGIO_BACKGROUND, SWC_DESKTOP,
        SWFO_NEEDDISPATCH,
    };
    // SAFETY: plain COM calls; each object is released when it goes out of scope.
    unsafe {
        let windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER)?;
        let (at, empty) = (variant(VT_I4, |v| v.lVal = CSIDL_DESKTOP as i32), windows::Win32::System::Variant::VARIANT::default());
        let mut hwnd = 0i32;
        let desktop = windows.FindWindowSW(&at, &empty, SWC_DESKTOP, &mut hwnd, SWFO_NEEDDISPATCH)?;
        let browser: IShellBrowser = desktop.cast::<IServiceProvider>()?.QueryService(&SID_STopLevelBrowser)?;
        let view: IShellView = browser.QueryActiveShellView()?;
        let folder: IShellFolderViewDual = view.GetItemObject::<IDispatch>(SVGIO_BACKGROUND)?.cast()?;
        folder.Application()?.cast()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::run_hidden;
    use std::time::Duration;

    #[test]
    fn a_folder_other_accounts_can_change_is_spotted() {
        let dir = std::env::temp_dir().join(format!("bootmend-who-{:x}", crate::util::unique()));
        std::fs::create_dir(&dir).unwrap();
        // Only this account, SYSTEM and Administrators, as in a Downloads folder. Some PCs let
        // other accounts into the temp folder, which the new folder would otherwise take on.
        let me = format!("*{}:(OI)(CI)F", current_user().unwrap());
        let d = dir.to_string_lossy().to_string();
        let own = run_hidden("icacls.exe", &[&d, "/inheritance:r", "/grant:r", &me, "*S-1-5-18:(OI)(CI)F", "*S-1-5-32-544:(OI)(CI)F"], Duration::from_secs(30))
            .unwrap();
        let exe = dir.join("bootmend.exe");
        std::fs::write(&exe, "x").unwrap();
        let before = others_can_change(&exe);
        // Lets everyone in the Users group change the folder and what's in it.
        let grant = run_hidden("icacls.exe", &[&d, "/grant", "*S-1-5-32-545:(OI)(CI)M"], Duration::from_secs(30)).unwrap();
        let after = others_can_change(&exe);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(own.code, Some(0), "{}", own.stdout);
        assert_eq!(grant.code, Some(0), "{}", grant.stdout);
        assert!(!before, "the user's own temp folder was taken for a shared one");
        assert!(after, "a folder everyone can change was taken for a private one");
    }

    #[test]
    fn a_folder_reached_through_a_junction_is_judged_by_where_it_really_is() {
        let base = std::env::temp_dir().join(format!("bootmend-real-{:x}", crate::util::unique()));
        let (shared, mine) = (base.join("shared"), base.join("mine"));
        let real = shared.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::create_dir_all(&mine).unwrap();
        let me = format!("*{}:(OI)(CI)F", current_user().unwrap());
        let b = base.to_string_lossy().to_string();
        let own = run_hidden("icacls.exe", &[&b, "/inheritance:r", "/grant:r", &me, "*S-1-5-18:(OI)(CI)F", "*S-1-5-32-544:(OI)(CI)F"], Duration::from_secs(30))
            .unwrap();
        // Everyone in the Users group may replace what's in `shared`, such as the folder the exe is in.
        let grant = run_hidden("icacls.exe", &[&shared.to_string_lossy(), "/grant", "*S-1-5-32-545:(F)"], Duration::from_secs(30)).unwrap();
        std::fs::write(real.join("bootmend.exe"), "x").unwrap();
        let link = mine.join("link");
        let made = run_hidden("cmd.exe", &["/c", "mklink", "/J", &link.to_string_lossy(), &real.to_string_lossy()], Duration::from_secs(30)).unwrap();
        let direct = others_can_change(&real.join("bootmend.exe"));
        let through = others_can_change(&link.join("bootmend.exe"));
        let _ = std::fs::remove_dir(&link);
        std::fs::remove_dir_all(&base).unwrap();
        assert_eq!((own.code, grant.code, made.code), (Some(0), Some(0), Some(0)), "{} {} {}", own.stdout, grant.stdout, made.stdout);
        assert!(direct, "the real folder's parent lets other accounts in");
        assert!(through, "a junction hid where the exe really is");
    }

    #[test]
    fn the_desktop_shell_can_be_reached() {
        use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
        let started = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
        let shell = desktop_shell().map(|_| ());
        if started {
            unsafe { CoUninitialize() };
        }
        shell.unwrap();
    }

    /// Starts a hidden, windowless command through the desktop's shell and waits for the file it writes.
    #[test]
    #[ignore = "starts a program through Explorer; run with --ignored"]
    fn a_program_started_through_the_desktop_shell_runs() {
        let mark = std::env::temp_dir().join(format!("bootmend-shell-{:x}.txt", crate::util::unique()));
        let conhost = crate::sys::program("conhost.exe");
        let cmd = crate::sys::program("cmd.exe");
        open_as_user(&conhost.to_string_lossy(), &format!("--headless \"{}\" /c type nul > \"{}\"", cmd.display(), mark.display()), 0).unwrap();
        let t0 = std::time::Instant::now();
        while !mark.exists() && t0.elapsed() < Duration::from_secs(15) {
            std::thread::sleep(Duration::from_millis(200));
        }
        let made = mark.exists();
        if made {
            std::fs::remove_file(&mark).unwrap();
        }
        assert!(made, "the command never ran");
    }
}
