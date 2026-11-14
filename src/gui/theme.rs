//! Colours, type and spacing. Every text/background pair here was checked for WCAG AA
//! contrast (see DESIGN.md). The app follows the Windows light or dark setting, and can take
//! its accent from Windows too.

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Theme, Visuals};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Pal {
    pub bg: Color32,
    pub surface: Color32,
    pub surface2: Color32,
    pub sidebar: Color32,
    pub ink: Color32,
    pub muted: Color32,
    pub line: Color32,
    pub control: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub accent_soft: Color32,
    pub red: Color32,
    pub red_bg: Color32,
    pub amber: Color32,
    pub amber_bg: Color32,
    pub blue: Color32,
    pub blue_bg: Color32,
    pub green: Color32,
    pub green_bg: Color32,
}

const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

pub const LIGHT: Pal = Pal {
    bg: hex(0xF6F7F9),
    surface: hex(0xFFFFFF),
    surface2: hex(0xEEF0F3),
    sidebar: hex(0xEEF0F3),
    ink: hex(0x1B1F24),
    muted: hex(0x4A5361),
    line: hex(0xDADFE6),
    control: hex(0x8A93A0),
    accent: hex(0x2F5BEA),
    on_accent: hex(0xFFFFFF),
    accent_soft: hex(0xE1E8FD),
    red: hex(0xB42318),
    red_bg: hex(0xFDECEA),
    amber: hex(0x8A4B00),
    amber_bg: hex(0xFFF4E0),
    blue: hex(0x2B5797),
    blue_bg: hex(0xE8F0FB),
    green: hex(0x1E7B3C),
    green_bg: hex(0xE6F4EA),
};

pub const DARK: Pal = Pal {
    bg: hex(0x16181C),
    surface: hex(0x1F2227),
    surface2: hex(0x2A2E35),
    sidebar: hex(0x1A1D21),
    ink: hex(0xE8EAED),
    muted: hex(0xA9B0BB),
    line: hex(0x2E333A),
    control: hex(0x7D8590),
    accent: hex(0x8AABFF),
    on_accent: hex(0x0B1530),
    accent_soft: hex(0x24304F),
    red: hex(0xFF9C94),
    red_bg: hex(0x3A1D1B),
    amber: hex(0xFFC46B),
    amber_bg: hex(0x3A2C14),
    blue: hex(0x9EC2FF),
    blue_bg: hex(0x1C2A40),
    green: hex(0x7BD88F),
    green_bg: hex(0x17301F),
};

/// The palettes in use: cobalt, or the Windows accent when the person turned that on.
#[derive(Clone, Copy)]
struct Palettes {
    light: Pal,
    dark: Pal,
}

fn palettes_id() -> egui::Id {
    egui::Id::new("bootmend-palettes")
}

pub fn pal(ctx: &egui::Context) -> Pal {
    let dark = ctx.style().visuals.dark_mode;
    let ps = ctx.data(|d| d.get_temp::<Palettes>(palettes_id())).unwrap_or(Palettes { light: LIGHT, dark: DARK });
    if dark {
        ps.dark
    } else {
        ps.light
    }
}

/// Windows keeps the accent colour and six shades of it in `AccentPalette`: eight RGBA entries,
/// ordered Light3, Light2, Light1, Accent, Dark1, Dark2, Dark3 and one unused.
/// Windows 11 fills buttons with Dark1 in light mode and Light2 in dark mode.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct AccentShades {
    pub light2: Color32,
    pub dark1: Color32,
}

pub fn accent_shades(b: &[u8]) -> Option<AccentShades> {
    if b.len() != 32 {
        return None;
    }
    let at = |i: usize| Color32::from_rgb(b[i * 4], b[i * 4 + 1], b[i * 4 + 2]);
    Some(AccentShades { light2: at(1), dark1: at(4) })
}

/// WCAG 2 contrast ratio, 1 to 21.
pub fn contrast(a: Color32, b: Color32) -> f32 {
    fn luminance(c: Color32) -> f32 {
        let ch = |v: u8| {
            let s = v as f32 / 255.0;
            if s <= 0.04045 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
    }
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Every pair the accent takes part in: button text, links and icons on each background,
/// text on a selected row, and the selection bar against its row.
fn readable(p: &Pal) -> bool {
    let text = |fg, bg| contrast(fg, bg) >= 4.5;
    text(p.on_accent, p.accent)
        && [p.bg, p.surface, p.surface2, p.sidebar].into_iter().all(|bg| text(p.accent, bg))
        && text(p.ink, p.accent_soft)
        && text(p.muted, p.accent_soft)
        && contrast(p.accent, p.accent_soft) >= 3.0
}

/// The palette with the Windows accent in place of cobalt, the way Windows 11 uses it, or
/// None when some pair would be hard to read.
pub fn with_windows_accent(base: &Pal, s: &AccentShades, dark: bool) -> Option<Pal> {
    let (accent, on_accent, tint) = if dark { (s.light2, Color32::BLACK, 0.18) } else { (s.dark1, Color32::WHITE, 0.14) };
    let p = Pal { accent, on_accent, accent_soft: base.surface.lerp_to_gamma(accent, tint), ..*base };
    readable(&p).then_some(p)
}

/// Whether the Windows accent can be used in the theme on screen now.
pub fn accent_fits(ctx: &egui::Context, s: &AccentShades) -> bool {
    let dark = ctx.style().visuals.dark_mode;
    with_windows_accent(if dark { &DARK } else { &LIGHT }, s, dark).is_some()
}

/// Uses the Windows accent where it reads well, and cobalt everywhere else.
pub fn set_accent(ctx: &egui::Context, windows: Option<&AccentShades>) {
    let pick = |base: &Pal, dark| windows.and_then(|s| with_windows_accent(base, s, dark)).unwrap_or(*base);
    let ps = Palettes { light: pick(&LIGHT, false), dark: pick(&DARK, true) };
    ctx.data_mut(|d| d.insert_temp(palettes_id(), ps));
    for (theme, p, dark) in [(Theme::Light, ps.light, false), (Theme::Dark, ps.dark, true)] {
        ctx.style_mut_of(theme, |st| st.visuals = visuals(&p, dark));
    }
}

/// Spacing scale (px). Nothing in the UI uses a value outside it.
pub const S1: f32 = 4.0;
pub const S2: f32 = 8.0;
pub const S3: f32 = 12.0;
pub const S4: f32 = 16.0;
pub const S5: f32 = 24.0;
pub const S6: f32 = 32.0;
pub const RADIUS: u8 = 8;

pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}
pub fn icons() -> FontFamily {
    FontFamily::Name("icons".into())
}
pub fn h2() -> TextStyle {
    TextStyle::Name("h2".into())
}
/// Card titles: one step below section headings.
pub fn title() -> TextStyle {
    TextStyle::Name("title".into())
}
pub fn big() -> TextStyle {
    TextStyle::Name("big".into())
}

fn load(name: &str) -> Option<FontData> {
    let dir = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_else(|| r"C:\Windows".into());
    std::fs::read(dir.join("Fonts").join(name)).ok().map(FontData::from_owned)
}

/// Use Windows' own fonts so the app feels native; fall back to egui's built-in ones.
pub fn fonts(ctx: &egui::Context) {
    let mut f = FontDefinitions::default();
    let mut add = |key: &str, file: &[&str], fam: FontFamily| {
        if let Some(d) = file.iter().find_map(|n| load(n)) {
            f.font_data.insert(key.into(), Arc::new(d));
            f.families.entry(fam).or_default().insert(0, key.into());
        }
    };
    // Segoe UI Variable is Windows 11's own UI font. egui can't pick a weight from a variable
    // font, so it gives the regular text weight and headings keep classic Segoe UI Semibold.
    add("segoe", &["SegUIVar.ttf", "segoeui.ttf"], FontFamily::Proportional);
    add("consolas", &["consola.ttf"], FontFamily::Monospace);
    add("segoe-sb", &["seguisb.ttf", "segoeuib.ttf"], semibold());
    add("fluent", &["SegoeIcons.ttf", "segmdl2.ttf"], icons());
    // Every named family needs a fallback chain so missing glyphs still render.
    let fallback = f.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for fam in [semibold(), icons()] {
        let e = f.families.entry(fam).or_default();
        for k in &fallback {
            if !e.contains(k) {
                e.push(k.clone());
            }
        }
    }
    ctx.set_fonts(f);
}

fn visuals(p: &Pal, dark: bool) -> Visuals {
    let mut v = if dark { Visuals::dark() } else { Visuals::light() };
    v.panel_fill = p.bg;
    v.window_fill = p.surface;
    v.extreme_bg_color = p.surface;
    v.faint_bg_color = p.surface2;
    v.code_bg_color = p.surface2;
    v.hyperlink_color = p.accent;
    v.window_stroke = Stroke::new(1.0_f32, p.line);
    v.window_corner_radius = CornerRadius::same(RADIUS + 2);
    v.menu_corner_radius = CornerRadius::same(RADIUS);
    v.selection.bg_fill = p.accent_soft;
    v.selection.stroke = Stroke::new(1.0_f32, p.ink);
    v.striped = false;
    let r = CornerRadius::same(6);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.surface;
    w.noninteractive.weak_bg_fill = p.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.line);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.ink);
    w.noninteractive.corner_radius = r;
    w.inactive.bg_fill = p.surface2;
    w.inactive.weak_bg_fill = p.surface2;
    w.inactive.bg_stroke = Stroke::new(1.0_f32, p.control);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, p.ink);
    w.inactive.corner_radius = r;
    w.hovered.bg_fill = p.accent_soft;
    w.hovered.weak_bg_fill = p.accent_soft;
    w.hovered.bg_stroke = Stroke::new(1.0_f32, p.accent);
    w.hovered.fg_stroke = Stroke::new(1.5_f32, p.ink);
    w.hovered.corner_radius = r;
    w.hovered.expansion = 0.0;
    w.active.bg_fill = p.accent_soft;
    w.active.weak_bg_fill = p.accent_soft;
    w.active.bg_stroke = Stroke::new(2.0_f32, p.accent);
    w.active.fg_stroke = Stroke::new(2.0_f32, p.ink);
    w.active.corner_radius = r;
    w.active.expansion = 0.0;
    w.open = w.active;
    v
}

pub fn apply(ctx: &egui::Context) {
    fonts(ctx);
    ctx.options_mut(|o| o.theme_preference = egui::ThemePreference::System);
    for theme in [Theme::Light, Theme::Dark] {
        ctx.style_mut_of(theme, |st| {
            st.text_styles = [
                (TextStyle::Heading, FontId::new(26.0, semibold())),
                (h2(), FontId::new(18.0, semibold())),
                (title(), FontId::new(16.0, semibold())),
                (big(), FontId::new(30.0, semibold())),
                (TextStyle::Body, FontId::new(14.5, FontFamily::Proportional)),
                (TextStyle::Button, FontId::new(14.5, FontFamily::Proportional)),
                (TextStyle::Small, FontId::new(12.5, FontFamily::Proportional)),
                (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
            ]
            .into();
            let sp = &mut st.spacing;
            sp.item_spacing = egui::vec2(S2, S2);
            sp.button_padding = egui::vec2(S3 + 2.0, S1 + 2.0);
            sp.interact_size.y = 30.0;
            sp.indent = S5;
            sp.scroll = egui::style::ScrollStyle::solid();
            st.interaction.selectable_labels = true;
        });
    }
    set_accent(ctx, None);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AccentPalette as Windows 11 stores it for its default blue accent.
    const DEFAULT_BLUE: [u8; 32] = [
        0x99, 0xEB, 0xFF, 0x00, 0x4C, 0xC2, 0xFF, 0x00, 0x00, 0x91, 0xF8, 0x00, 0x00, 0x78, 0xD4, 0x00, 0x00, 0x67, 0xC0, 0x00, 0x00, 0x3E, 0x92, 0x00, 0x00,
        0x1A, 0x68, 0x00, 0xF7, 0x63, 0x0C, 0x00,
    ];

    #[test]
    fn reads_the_shades_windows_uses() {
        let s = accent_shades(&DEFAULT_BLUE).expect("32 bytes is a full palette");
        assert_eq!(s.light2, hex(0x4CC2FF));
        assert_eq!(s.dark1, hex(0x0067C0));
    }

    #[test]
    fn rejects_a_palette_of_the_wrong_size() {
        assert_eq!(accent_shades(&DEFAULT_BLUE[..31]), None);
        assert_eq!(accent_shades(&[0; 33]), None);
        assert_eq!(accent_shades(&[]), None);
    }

    #[test]
    fn contrast_matches_wcag() {
        assert!((contrast(Color32::WHITE, hex(0x2F5BEA)) - 5.52).abs() < 0.01);
        assert!((contrast(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 0.01);
        assert_eq!(contrast(hex(0x2F5BEA), Color32::WHITE), contrast(Color32::WHITE, hex(0x2F5BEA)));
    }

    #[test]
    fn brand_palettes_are_readable() {
        assert!(readable(&LIGHT), "light palette fails a contrast pair");
        assert!(readable(&DARK), "dark palette fails a contrast pair");
    }

    #[test]
    fn light_theme_takes_the_darker_shade_with_white_text() {
        let s = accent_shades(&DEFAULT_BLUE).unwrap();
        let p = with_windows_accent(&LIGHT, &s, false).expect("Windows' default blue is readable");
        assert_eq!(p.accent, hex(0x0067C0));
        assert_eq!(p.on_accent, Color32::WHITE);
        assert_eq!(p.ink, LIGHT.ink, "only the accent colours change");
    }

    #[test]
    fn dark_theme_takes_the_lighter_shade_with_black_text() {
        let s = accent_shades(&DEFAULT_BLUE).unwrap();
        let p = with_windows_accent(&DARK, &s, true).expect("Windows' default blue is readable");
        assert_eq!(p.accent, hex(0x4CC2FF));
        assert_eq!(p.on_accent, Color32::BLACK);
        assert_eq!(p.bg, DARK.bg, "only the accent colours change");
    }

    #[test]
    fn an_unreadable_accent_keeps_cobalt() {
        let pale = AccentShades { light2: hex(0x303030), dark1: hex(0xFFD700) };
        assert_eq!(with_windows_accent(&LIGHT, &pale, false), None);
        assert_eq!(with_windows_accent(&DARK, &pale, true), None);
    }
}
