//! Administrator checks and relaunching with a UAC prompt.

use anyhow::{anyhow, Result};

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
    let r = unsafe { ShellExecuteW(None, &HSTRING::from("runas"), &HSTRING::from(exe.as_os_str()), &HSTRING::from(args), None, SW_SHOWNORMAL) };
    if r.0 as isize > 32 {
        Ok(())
    } else {
        Err(anyhow!("Windows did not start Bootmend as administrator. The prompt may have been cancelled."))
    }
}
