//! Colours, type and spacing. Every text/background pair here was checked for WCAG AA
//! contrast (see DESIGN.md). The app follows the Windows light or dark setting.

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Theme, Visuals};
use std::sync::Arc;

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
    accent: hex(0x1F56C4),
    on_accent: hex(0xFFFFFF),
    accent_soft: hex(0xDCE6F8),
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
    accent: hex(0x8AB0FF),
    on_accent: hex(0x0B1220),
    accent_soft: hex(0x26324A),
    red: hex(0xFF9C94),
    red_bg: hex(0x3A1D1B),
    amber: hex(0xFFC46B),
    amber_bg: hex(0x3A2C14),
    blue: hex(0x9EC2FF),
    blue_bg: hex(0x1C2A40),
    green: hex(0x7BD88F),
    green_bg: hex(0x17301F),
};

pub fn pal(ctx: &egui::Context) -> &'static Pal {
    if ctx.style().visuals.dark_mode {
        &DARK
    } else {
        &LIGHT
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
    add("segoe", &["segoeui.ttf"], FontFamily::Proportional);
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
    for (theme, p, dark) in [(Theme::Light, &LIGHT, false), (Theme::Dark, &DARK, true)] {
        ctx.style_mut_of(theme, |st| {
            st.visuals = visuals(p, dark);
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
}
