//! Small building blocks shared by every page.

use super::theme::{self, pal, Pal, CONTROL_RADIUS, RADIUS, S2, S3, S4};
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
    pub const CLEAN: &str = "\u{E74D}";
    pub const ACTIVITY: &str = "\u{E9D9}";
    pub const SETTINGS: &str = "\u{E713}";
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
        .corner_radius(CornerRadius::same(u8::MAX))
        .inner_margin(Margin::symmetric(S2 as i8, 2))
        .show(ui, |ui| ui.label(RichText::new(text).small().color(fg).family(theme::semibold())))
        .response
}

pub fn level_pill(ui: &mut Ui, l: Level) -> Response {
    let (fg, bg) = level_colors(&pal(ui.ctx()), l);
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
    job.append(text, S2, egui::TextFormat { font_id: egui::TextStyle::Button.resolve(ui.style()), color, valign: egui::Align::Center, ..Default::default() });
    ui.add_enabled(enabled, egui::Button::new(job).min_size(egui::vec2(0.0, 32.0)))
}

/// An on/off switch, like the ones in Windows Settings. The caller shows "On"/"Off" beside it,
/// so the state never rests on colour alone.
pub fn switch(ui: &mut Ui, on: bool, enabled: bool, name: &str) -> Response {
    let p = pal(ui.ctx());
    let size = egui::vec2(40.0, 20.0);
    let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, r) = ui.allocate_exact_size(size, sense);
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, on, name));
    ring_radius(ui, &r, size.y / 2.0);
    if ui.is_rect_visible(rect) {
        let radius = CornerRadius::same(u8::MAX);
        let (track, edge, knob) = match (enabled, on) {
            (false, true) => (p.accent_soft, p.accent_soft, p.control),
            (false, false) => (p.surface2, p.line, p.control),
            (true, true) => (p.accent, p.accent, p.on_accent),
            (true, false) => (p.surface, p.control, p.muted),
        };
        let painter = ui.painter();
        painter.rect(rect, radius, track, Stroke::new(1.0_f32, edge), egui::StrokeKind::Inside);
        let x = if on { rect.right() - 10.0 } else { rect.left() + 10.0 };
        // The knob grows a little under the pointer, the way Windows' own switches respond.
        let grow = if r.is_pointer_button_down_on() {
            1.5
        } else if r.hovered() {
            1.0
        } else {
            0.0
        };
        painter.circle_filled(egui::pos2(x, rect.center().y), 6.0 + grow, knob);
    }
    r
}

fn ring_id(id: egui::Id) -> egui::Id {
    id.with("focus-ring")
}

/// Rounds the focus ring for a control whose corners aren't the standard 4 px.
pub fn ring_radius(ui: &Ui, r: &Response, radius: f32) {
    ui.ctx().data_mut(|d| d.insert_temp(ring_id(r.id), radius));
}

/// Draws the Windows focus visual around whatever has keyboard focus: a 2 px ring in the text
/// colour with a 1 px ring in the surface colour inside it, 3 px out from the control, so it
/// reads on any fill. Like `:focus-visible`, it shows once a key moves focus and goes away on
/// the next click. Call it after the whole UI is laid out.
pub fn focus_ring(ctx: &egui::Context) {
    let keyboard_id = egui::Id::new("bootmend-keyboard");
    let mut keyboard = ctx.data(|d| d.get_temp::<bool>(keyboard_id)).unwrap_or(false);
    ctx.input(|i| {
        for e in &i.events {
            match e {
                egui::Event::Key {
                    key: egui::Key::Tab | egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::ArrowLeft | egui::Key::ArrowRight,
                    pressed: true,
                    ..
                } => keyboard = true,
                egui::Event::PointerButton { pressed: true, .. } => keyboard = false,
                _ => {}
            }
        }
    });
    ctx.data_mut(|d| d.insert_temp(keyboard_id, keyboard));
    let Some(r) = keyboard.then(|| ctx.memory(|m| m.focused())).flatten().and_then(|id| ctx.read_response(id)) else { return };
    if r.interact_rect.area() <= 0.0 {
        return;
    }
    let p = pal(ctx);
    let radius = ctx.data(|d| d.get_temp::<f32>(ring_id(r.id))).unwrap_or(CONTROL_RADIUS as f32);
    let corner = |grow: f32| CornerRadius::same((radius + grow).round().min(u8::MAX as f32) as u8);
    // Clip to the visible part of the control, so a ring never spills over a header.
    let painter = ctx.layer_painter(r.layer_id).with_clip_rect(r.interact_rect.expand(4.0));
    painter.rect_stroke(r.rect.expand(3.0), corner(3.0), Stroke::new(2.0_f32, p.ink), egui::StrokeKind::Inside);
    painter.rect_stroke(r.rect.expand(1.0), corner(1.0), Stroke::new(1.0_f32, p.surface), egui::StrokeKind::Inside);
}

/// Tells screen readers a label is a heading, so Narrator can jump from one to the next.
pub fn heading_role(ui: &Ui, r: &Response, level: usize, text: &str) {
    ui.ctx().accesskit_node_builder(r.id, |b| {
        b.set_role(egui::accesskit::Role::Heading);
        b.set_level(level);
        b.set_label(text);
    });
}

/// A page title, in the Title size.
pub fn page_title(ui: &mut Ui, title: &str) -> Response {
    let r = ui.label(RichText::new(title).text_style(egui::TextStyle::Heading));
    heading_role(ui, &r, 1, title);
    r
}

pub fn section(ui: &mut Ui, title: &str) {
    ui.add_space(S3);
    let r = ui.label(RichText::new(title).text_style(theme::h2()));
    heading_role(ui, &r, 2, title);
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
    table_action(ui, id, head, rows, None);
}

/// A table with a small button at the end of each row. Returns the row whose button was pressed.
pub fn table_action(ui: &mut Ui, id: &str, head: &[&str], rows: &[Vec<String>], button: Option<&str>) -> Option<usize> {
    if rows.is_empty() {
        muted(ui, "Nothing to show here.");
        return None;
    }
    let p = pal(ui.ctx());
    let mut pressed = None;
    ui.push_id(id, |ui| {
        let n = head.len();
        // With a button column, the text columns share what's left so the table never runs past the card.
        let avail = ui.available_width() - ui.spacing().item_spacing.x * (n + 1) as f32 - if button.is_some() { 72.0 } else { 0.0 };
        let first = (avail * 0.32).min(240.0);
        let rest = ((avail - first) / (n.max(2) - 1) as f32).min(130.0);
        let mut tb = TableBuilder::new(ui).striped(true).vscroll(false).cell_layout(egui::Layout::left_to_right(egui::Align::Center));
        for i in 0..n {
            tb = tb.column(if i + 1 == n && button.is_none() {
                Column::remainder().at_least(80.0).clip(true)
            } else {
                Column::initial(if i == 0 { first } else { rest }).at_least(60.0).resizable(true).clip(true)
            });
        }
        if button.is_some() {
            tb = tb.column(Column::exact(64.0));
        }
        tb.header(26.0, |mut h| {
            for t in head {
                h.col(|ui| {
                    ui.label(RichText::new(*t).color(p.muted).family(theme::semibold()));
                });
            }
            if button.is_some() {
                h.col(|_| {});
            }
        })
        .body(|body| {
            body.rows(26.0, rows.len(), |mut row| {
                let r = &rows[row.index()];
                let i = row.index();
                for c in r {
                    row.col(|ui| {
                        // A truncated label shows its full text on hover by itself.
                        ui.add(egui::Label::new(c.as_str()).truncate());
                    });
                }
                if let Some(label) = button {
                    row.col(|ui| {
                        if ui.small_button(label).clicked() {
                            pressed = Some(i);
                        }
                    });
                }
            });
        });
    });
    pressed
}

pub fn empty(ui: &mut Ui, ic: &str, title: &str, body: &str) {
    let p = pal(ui.ctx());
    card(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(S3);
            ui.label(ico(ic, 28.0).color(p.muted));
            ui.add_space(S2);
            let r = ui.label(RichText::new(title).text_style(theme::h2()));
            heading_role(ui, &r, 2, title);
            ui.label(RichText::new(body).color(p.muted));
            ui.add_space(S3);
        });
    });
}

/// A horizontal usage bar, for free space and battery health. `name` is what a screen reader
/// says before the percentage.
pub fn bar(ui: &mut Ui, name: &str, fraction: f32, fill: Color32) {
    let p = pal(ui.ctx());
    let fraction = fraction.clamp(0.0, 1.0);
    let w = ui.available_width();
    let (r, resp) = ui.allocate_exact_size(egui::vec2(w, 8.0), Sense::hover());
    resp.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::WidgetType::ProgressIndicator, true, name);
        info.value = Some((fraction * 100.0).round() as f64);
        info
    });
    ui.ctx().accesskit_node_builder(resp.id, |b| {
        b.set_min_numeric_value(0.0);
        b.set_max_numeric_value(100.0);
    });
    ui.painter().rect_filled(r, CornerRadius::same(u8::MAX), p.surface2);
    let mut f = r;
    f.set_width(r.width() * fraction);
    ui.painter().rect_filled(f, CornerRadius::same(u8::MAX), fill);
}

/// A clickable row in the sidebar.
pub fn nav_item(ui: &mut Ui, ic: &str, text: &str, selected: bool, badge: Option<usize>) -> Response {
    let p = pal(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), Sense::click());
    let bg = if selected {
        p.accent_soft
    } else if resp.hovered() {
        // A faint wash of the text colour, as Windows uses, so hover shows on any sidebar fill,
        // Mica included.
        p.ink.gamma_multiply(0.07)
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), bg);
    if selected {
        let bar = egui::Rect::from_min_size(rect.min + egui::vec2(0.0, 9.0), egui::vec2(3.0, 16.0));
        ui.painter().rect_filled(bar, CornerRadius::same(u8::MAX), p.accent);
    }
    let y = rect.center().y;
    ui.painter().text(egui::pos2(rect.left() + S3 + 2.0, y), egui::Align2::LEFT_CENTER, ic, egui::FontId::new(16.0, theme::icons()), p.ink);
    let font = egui::FontId::new(14.0, if selected { theme::semibold() } else { egui::FontFamily::Proportional });
    ui.painter().text(egui::pos2(rect.left() + S3 + 30.0, y), egui::Align2::LEFT_CENTER, text, font, p.ink);
    if let Some(n) = badge.filter(|n| *n > 0) {
        let t = n.to_string();
        let c = egui::pos2(rect.right() - S3 - 6.0, y);
        ui.painter().circle_filled(c, 10.0, p.red_bg);
        ui.painter().text(c, egui::Align2::CENTER_CENTER, t, egui::FontId::new(12.0, theme::semibold()), p.red);
    }
    let name = match badge.filter(|n| *n > 0) {
        Some(n) => format!("{text}, {n} to look at"),
        None => text.to_string(),
    };
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, &name));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs one frame with a single button and the focus ring pass, and says whether the ring
    /// (a 2 px stroke in the text colour) was painted.
    fn frame(ctx: &egui::Context, events: Vec<egui::Event>) -> bool {
        let input = egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0))), ..Default::default() };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add_space(40.0);
                secondary(ui, "Check again");
            });
            focus_ring(ctx);
        });
        let ink = pal(ctx).ink;
        out.shapes.iter().any(|c| matches!(&c.shape, egui::Shape::Rect(r) if r.stroke.width == 2.0 && r.stroke.color == ink))
    }

    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() }
    }

    #[test]
    fn the_focus_ring_follows_the_keyboard_not_the_mouse() {
        let ctx = egui::Context::default();
        assert!(!frame(&ctx, vec![]), "no ring before anything has focus");
        frame(&ctx, vec![key(egui::Key::Tab)]);
        assert!(frame(&ctx, vec![]), "Tab gives the button focus and a ring");
        let click = egui::Event::PointerButton { pos: egui::pos2(5.0, 5.0), button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() };
        frame(&ctx, vec![egui::Event::PointerMoved(egui::pos2(5.0, 5.0)), click]);
        assert!(!frame(&ctx, vec![]), "a click hides the ring");
    }
}
