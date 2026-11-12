//! Small building blocks shared by every page.

use super::theme::{self, pal, Pal, RADIUS, S2, S3, S4};
use crate::rules::Level;
use eframe::egui::{self, Color32, CornerRadius, Margin, Response, RichText, Sense, Stroke, Ui};
use egui_extras::{Column, TableBuilder};

pub mod icon {
    pub const HOME: &str = "\u{E80F}";
    pub const BULB: &str = "\u{EA80}";
    pub const PC: &str = "\u{E7F8}";
    pub const DRIVE: &str = "\u{EDA2}";
    pub const NETWORK: &str = "\u{E968}";
    pub const SHIELD: &str = "\u{EA18}";
    pub const APPS: &str = "\u{E71D}";
    pub const POWER: &str = "\u{E7E8}";
    pub const CRASH: &str = "\u{E783}";
    pub const CHANGES: &str = "\u{E81C}";
    pub const SCAN: &str = "\u{E721}";
    pub const UNDO: &str = "\u{E7A7}";
    pub const REPORT: &str = "\u{E8A5}";
    pub const LOCK: &str = "\u{E72E}";
    pub const INFO: &str = "\u{E946}";
    pub const REFRESH: &str = "\u{E72C}";
    pub const CHECK: &str = "\u{E73E}";
    pub const WARN: &str = "\u{E7BA}";
    pub const OPEN: &str = "\u{E8A7}";
    pub const FOLDER: &str = "\u{E8B7}";
    pub const ADMIN: &str = "\u{E7EF}";
}

pub fn ico(ch: &str, size: f32) -> RichText {
    RichText::new(ch).family(theme::icons()).size(size)
}

pub fn level_colors(p: &Pal, l: Level) -> (Color32, Color32) {
    match l {
        Level::DoFirst => (p.red, p.red_bg),
        Level::WorthDoing => (p.amber, p.amber_bg),
        Level::NiceToHave => (p.blue, p.blue_bg),
        Level::AllGood => (p.green, p.green_bg),
    }
}

pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = pal(ui.ctx());
    egui::Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0_f32, p.line))
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::same(S4 as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Columns use a justified layout, which would stretch the spaces in wrapped text.
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), add).inner
        })
        .inner
}

/// A tinted strip for notes and warnings. Colour is never the only signal: each has an icon and words.
pub fn note(ui: &mut Ui, fg: Color32, bg: Color32, ic: &str, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new().fill(bg).corner_radius(CornerRadius::same(RADIUS)).inner_margin(Margin::symmetric(S4 as i8, S3 as i8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(ico(ic, 16.0).color(fg));
            ui.add_space(S2);
            ui.vertical(|ui| {
                ui.style_mut().visuals.override_text_color = Some(fg);
                add(ui);
            });
        });
    });
}

pub fn pill(ui: &mut Ui, text: &str, fg: Color32, bg: Color32) -> Response {
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(S2 as i8, 2))
        .show(ui, |ui| ui.label(RichText::new(text).size(12.5).color(fg).family(theme::semibold())))
        .response
}

pub fn level_pill(ui: &mut Ui, l: Level) -> Response {
    let (fg, bg) = level_colors(pal(ui.ctx()), l);
    pill(ui, l.name(), fg, bg)
}

pub fn primary(ui: &mut Ui, text: &str) -> Response {
    primary_enabled(ui, text, true)
}

pub fn primary_enabled(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let p = pal(ui.ctx());
    let b =
        egui::Button::new(RichText::new(text).color(p.on_accent).family(theme::semibold())).fill(p.accent).stroke(Stroke::NONE).min_size(egui::vec2(0.0, 32.0));
    ui.add_enabled(enabled, b)
}

pub fn secondary(ui: &mut Ui, text: &str) -> Response {
    secondary_enabled(ui, text, true)
}

pub fn secondary_enabled(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    ui.add_enabled(enabled, egui::Button::new(text).min_size(egui::vec2(0.0, 32.0)))
}

/// A secondary button with a leading icon.
pub fn icon_button(ui: &mut Ui, ic: &str, text: &str, enabled: bool) -> Response {
    let mut job = egui::text::LayoutJob::default();
    let color = pal(ui.ctx()).ink;
    job.append(ic, 0.0, egui::TextFormat { font_id: egui::FontId::new(14.0, theme::icons()), color, valign: egui::Align::Center, ..Default::default() });
    job.append(
        text,
        S2,
        egui::TextFormat { font_id: egui::FontId::new(14.5, egui::FontFamily::Proportional), color, valign: egui::Align::Center, ..Default::default() },
    );
    ui.add_enabled(enabled, egui::Button::new(job).min_size(egui::vec2(0.0, 32.0)))
}

pub fn section(ui: &mut Ui, title: &str) {
    ui.add_space(S3);
    ui.label(RichText::new(title).text_style(theme::h2()));
    ui.add_space(theme::S1);
}

pub fn muted(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.label(RichText::new(text.into()).color(pal(ui.ctx()).muted))
}

pub fn small(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.label(RichText::new(text.into()).small().color(pal(ui.ctx()).muted))
}

/// Label/value rows. Values are selectable so people can copy a model number.
pub fn kv(ui: &mut Ui, id: &str, rows: &[(&str, Option<String>)]) {
    let rows: Vec<_> = rows.iter().filter_map(|(k, v)| v.as_ref().filter(|v| !v.is_empty()).map(|v| (*k, v.clone()))).collect();
    if rows.is_empty() {
        muted(ui, "Windows didn't report these details.");
        return;
    }
    let p = pal(ui.ctx());
    egui::Grid::new(id).num_columns(2).spacing(egui::vec2(theme::S5, S2)).min_col_width(160.0).show(ui, |ui| {
        for (k, v) in rows {
            ui.label(RichText::new(k).color(p.muted));
            ui.add(egui::Label::new(v).wrap());
            ui.end_row();
        }
    });
}

/// A plain list table. Long cells are shortened with "…" and show in full on hover.
pub fn table(ui: &mut Ui, id: &str, head: &[&str], rows: &[Vec<String>]) {
    if rows.is_empty() {
        muted(ui, "Nothing to show here.");
        return;
    }
    let p = pal(ui.ctx());
    ui.push_id(id, |ui| {
        let n = head.len();
        let mut tb = TableBuilder::new(ui).striped(true).vscroll(false).cell_layout(egui::Layout::left_to_right(egui::Align::Center));
        for i in 0..n {
            tb = tb.column(if i + 1 == n {
                Column::remainder().at_least(80.0).clip(true)
            } else {
                Column::initial(if i == 0 { 240.0 } else { 130.0 }).at_least(60.0).resizable(true).clip(true)
            });
        }
        tb.header(26.0, |mut h| {
            for t in head {
                h.col(|ui| {
                    ui.label(RichText::new(*t).color(p.muted).family(theme::semibold()));
                });
            }
        })
        .body(|body| {
            body.rows(26.0, rows.len(), |mut row| {
                let r = &rows[row.index()];
                for c in r {
                    row.col(|ui| {
                        // A truncated label shows its full text on hover by itself.
                        ui.add(egui::Label::new(c.as_str()).truncate());
                    });
                }
            });
        });
    });
}

pub fn empty(ui: &mut Ui, ic: &str, title: &str, body: &str) {
    let p = pal(ui.ctx());
    card(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(S3);
            ui.label(ico(ic, 28.0).color(p.muted));
            ui.add_space(S2);
            ui.label(RichText::new(title).text_style(theme::h2()));
            ui.label(RichText::new(body).color(p.muted));
            ui.add_space(S3);
        });
    });
}

/// A horizontal usage bar, for free space and battery health.
pub fn bar(ui: &mut Ui, fraction: f32, fill: Color32) {
    let p = pal(ui.ctx());
    let w = ui.available_width().min(420.0);
    let (r, _) = ui.allocate_exact_size(egui::vec2(w, 8.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::same(4), p.surface2);
    let mut f = r;
    f.set_width(r.width() * fraction.clamp(0.0, 1.0));
    ui.painter().rect_filled(f, CornerRadius::same(4), fill);
}

/// A clickable row in the sidebar.
pub fn nav_item(ui: &mut Ui, ic: &str, text: &str, selected: bool, badge: Option<usize>) -> Response {
    let p = pal(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), Sense::click());
    let bg = if selected {
        p.accent_soft
    } else if resp.hovered() {
        p.surface2
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(6), bg);
    if selected {
        let bar = egui::Rect::from_min_size(rect.min + egui::vec2(0.0, 9.0), egui::vec2(3.0, 16.0));
        ui.painter().rect_filled(bar, CornerRadius::same(2), p.accent);
    }
    if resp.has_focus() {
        ui.painter().rect_stroke(rect.shrink(1.0), CornerRadius::same(6), Stroke::new(2.0_f32, p.accent), egui::StrokeKind::Inside);
    }
    let y = rect.center().y;
    ui.painter().text(egui::pos2(rect.left() + S3 + 2.0, y), egui::Align2::LEFT_CENTER, ic, egui::FontId::new(16.0, theme::icons()), p.ink);
    let font = if selected { egui::FontId::new(14.5, theme::semibold()) } else { egui::FontId::new(14.5, egui::FontFamily::Proportional) };
    ui.painter().text(egui::pos2(rect.left() + S3 + 30.0, y), egui::Align2::LEFT_CENTER, text, font, p.ink);
    if let Some(n) = badge.filter(|n| *n > 0) {
        let t = n.to_string();
        let c = egui::pos2(rect.right() - S3 - 6.0, y);
        ui.painter().circle_filled(c, 10.0, p.red_bg);
        ui.painter().text(c, egui::Align2::CENTER_CENTER, t, egui::FontId::new(12.0, theme::semibold()), p.red);
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, text));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}
