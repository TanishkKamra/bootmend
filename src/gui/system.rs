//! What Windows says about its own look: the accent colour, transparency effects and the
//! Mica backdrop. When Windows says nothing, Bootmend keeps its own look.

use super::theme::{accent_shades, AccentShades};
use windows::core::{w, PCWSTR};
use windows::Win32::System::Registry::{
    RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_ROUTINE_FLAGS, RRF_RT_REG_BINARY, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
};

/// Reads a registry value into `buf` and returns how many bytes Windows wrote. A missing value,
/// or one bigger than `buf`, gives None.
fn reg(root: HKEY, key: PCWSTR, name: PCWSTR, kind: REG_ROUTINE_FLAGS, buf: &mut [u8]) -> Option<usize> {
    let mut len = buf.len() as u32;
    // Safety: `buf` outlives the call and `len` tells Windows how much room it has.
    let r = unsafe { RegGetValueW(root, key, name, kind, None, Some(buf.as_mut_ptr().cast()), Some(&mut len)) };
    r.is_ok().then_some(len as usize)
}

/// The accent colour picked in Settings > Personalisation > Colours.
pub fn windows_accent() -> Option<AccentShades> {
    let mut buf = [0u8; 32];
    let n = reg(HKEY_CURRENT_USER, w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent"), w!("AccentPalette"), RRF_RT_REG_BINARY, &mut buf)?;
    accent_shades(&buf[..n])
}

/// Settings > Accessibility > Visual effects > Animation effects. Windows starts with it on,
/// so a failed read keeps motion on.
pub fn animations_on() -> bool {
    use windows::core::BOOL;
    use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};
    let mut on = BOOL(1);
    // Safety: this action writes one BOOL, and `on` lives across the call.
    let read =
        unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some(std::ptr::from_mut(&mut on).cast()), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) };
    match read {
        Ok(()) => on.as_bool(),
        Err(e) => {
            eprintln!("couldn't read the Animation effects setting, so motion stays on: {e}");
            true
        }
    }
}

/// Mica needs Windows 11 version 22H2 (build 22621) or later, with Transparency effects on.
pub fn mica_available() -> bool {
    let mut on = [0u8; 4];
    let personalize = w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let transparency = reg(HKEY_CURRENT_USER, personalize, w!("EnableTransparency"), RRF_RT_REG_DWORD, &mut on).is_some() && u32::from_le_bytes(on) == 1;
    let mut build = [0u8; 32];
    let build = reg(HKEY_LOCAL_MACHINE, w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"), w!("CurrentBuildNumber"), RRF_RT_REG_SZ, &mut build)
        .and_then(|n| utf16_number(&build[..n]));
    transparency && build.is_some_and(|b| b >= 22621)
}

/// A number Windows stores as UTF-16 text, such as the build number.
fn utf16_number(b: &[u8]) -> Option<u32> {
    let text: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).take_while(|&c| c != 0).collect();
    String::from_utf16(&text).ok()?.trim().parse().ok()
}

/// Puts the Mica backdrop behind the window, which must have been created transparent.
pub fn apply_mica(cc: &eframe::CreationContext) -> windows::core::Result<()> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{E_FAIL, HWND};
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMSBT_MAINWINDOW, DWMWA_SYSTEMBACKDROP_TYPE};
    let hwnd = match cc.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => HWND(h.hwnd.get() as *mut core::ffi::c_void),
        _ => return Err(windows::core::Error::new(E_FAIL, "the window has no Win32 handle")),
    };
    let kind = DWMSBT_MAINWINDOW;
    // Safety: `kind` lives across the call and the size passed is its size.
    unsafe { DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, std::ptr::from_ref(&kind).cast(), size_of_val(&kind) as u32) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wide(s: &str) -> Vec<u8> {
        s.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn reads_a_build_number_stored_as_text() {
        assert_eq!(utf16_number(&wide("26200")), Some(26200));
        assert_eq!(utf16_number(&wide("22621")), Some(22621));
    }

    /// Reads this PC's real settings; run with `cargo test -- --ignored` on Windows 11.
    #[test]
    #[ignore]
    fn reads_this_pcs_look() {
        let accent = windows_accent();
        eprintln!("accent: {accent:?}, mica: {}, animations: {}", mica_available(), animations_on());
        assert!(accent.is_some(), "Windows 11 always stores an AccentPalette");
    }

    #[test]
    fn rejects_text_that_is_not_a_number() {
        assert_eq!(utf16_number(&wide("")), None);
        assert_eq!(utf16_number(&wide("22H2")), None);
        assert_eq!(utf16_number(&[0x32]), None, "half a character");
    }
}
