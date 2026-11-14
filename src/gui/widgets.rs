//! Small building blocks shared by every page.

use super::theme::{self, pal, CONTROL_RADIUS, RADIUS, S1, S2, S3, S4};
use eframe::egui::{self, Color32, CornerRadius, Margin, Response, RichText, Sense, Stroke, Ui};
use egui_extras::{Column, TableBuilder};

pub mod icon {
    pub const HOME: &str = "\u{E80F}";
    pub const PC: &str = "\u{E7F8}";
    pub const NETWORK: &str = "\u{E968}";
    pub const SHIELD: &str = "\u{EA18}";
    pub const CHANGES: &str = "\u{E81C}";
    pub const UNDO: &str = "\u{E7A7}";
    pub const REPORT: &str = "\u{E8A5}";
    pub const LOCK: &str = "\u{E72E}";
    pub const INFO: &str = "\u{E946}";
    pub const REFRESH: &str = "\u{E72C}";
    pub const CHECK: &str = "\u{E73E}";
    pub const HEALTH: &str = "\u{E95E}";
    pub const SPEED: &str = "\u{EC4A}";
    pub const CHIP: &str = "\u{E950}";
    pub const WARN: &str = "\u{E7BA}";
    pub const OPEN: &str = "\u{E8A7}";
    pub const FOLDER: &str = "\u{E8B7}";
    pub const ADMIN: &str = "\u{E7EF}";
    pub const CLEAN: &str = "\u{E74D}";
    pub const SETTINGS: &str = "\u{E713}";
    pub const REPAIR: &str = "\u{E90F}";
    pub const HISTORY: &str = "\u{E81C}";
    pub const CHEVRON_RIGHT: &str = "\u{E76C}";
    pub const CHEVRON_DOWN: &str = "\u{E70D}";
    pub const CHEVRON_UP: &str = "\u{E70E}";
    /// GlobalNavigationButton, the three lines that open a folded sidebar.
    pub const MENU: &str = "\u{E700}";
}

/// Below this width a card's rows stack: a control goes under its words and a value under its
/// name. It is WinUI's SettingsCard threshold, 476 px, less the card's padding.
pub const WRAP: f32 = 476.0 - 2.0 * S4;

pub fn ico(ch: &str, size: f32) -> RichText {
    RichText::new(ch).family(theme::icons()).size(size)
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
        ui.horizontal_top(|ui| {
            // The icon sits in a box one line of text tall, so it centres on the first line
            // however many lines and buttons follow.
            let line = ui.text_style_height(&egui::TextStyle::Body);
            let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, line), Sense::hover());
            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, ic, egui::FontId::new(16.0, theme::icons()), fg);
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
        .show(ui, |ui| ui.add(egui::Label::new(RichText::new(text).small().color(fg).family(theme::semibold())).extend()))
        .response
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

/// Said beside a button that's greyed out for want of administrator rights.
pub const NEEDS_ADMIN: &str = "Needs administrator rights";

/// Why the greyed-out button beside it can't be used, said in the open so nobody has to hover
/// to find out. The icon and words stay in reading order whichever way the row is filled.
pub fn off_reason(ui: &mut Ui, ic: &str, text: &str) {
    let w = off_reason_width(ui, text);
    let muted = pal(ui.ctx()).muted;
    ui.allocate_ui_with_layout(egui::vec2(w, ui.spacing().interact_size.y), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = S1;
        let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::hover());
        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, ic, egui::FontId::new(14.0, theme::icons()), muted);
        ui.label(RichText::new(text).small().color(muted));
    });
}

pub fn admin_note(ui: &mut Ui) {
    off_reason(ui, icon::ADMIN, NEEDS_ADMIN);
}

/// How wide `off_reason` is, so a row can leave room for it.
pub fn off_reason_width(ui: &Ui, text: &str) -> f32 {
    let g = egui::WidgetText::from(RichText::new(text).small()).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Small);
    14.0 + S1 + g.size().x + 1.0
}

/// A secondary button with a leading icon.
pub fn icon_button(ui: &mut Ui, ic: &str, text: &str, enabled: bool) -> Response {
    let mut job = egui::text::LayoutJob::default();
    let color = pal(ui.ctx()).ink;
    job.append(ic, 0.0, egui::TextFormat { font_id: egui::FontId::new(14.0, theme::icons()), color, valign: egui::Align::Center, ..Default::default() });
    job.append(text, S2, egui::TextFormat { font_id: egui::TextStyle::Button.resolve(ui.style()), color, valign: egui::Align::Center, ..Default::default() });
    ui.add_enabled(enabled, egui::Button::new(job).min_size(egui::vec2(0.0, 32.0)))
}

/// A button with no frame until the pointer is on it, for quiet actions beside a title.
pub fn subtle_button(ui: &mut Ui, ic: &str, text: &str) -> Response {
    let p = pal(ui.ctx());
    ui.scope(|ui| {
        let w = &mut ui.visuals_mut().widgets;
        for (v, wash) in [(&mut w.inactive, 0.0), (&mut w.hovered, 0.06), (&mut w.active, 0.09)] {
            v.weak_bg_fill = p.ink.gamma_multiply(wash);
            v.bg_stroke = Stroke::NONE;
        }
        icon_button(ui, ic, text, true)
    })
    .inner
}

/// Tabs across the top of a place, like Windows' selector bar: the current tab in the text
/// colour with a short accent bar under it, the others muted. Returns the tab clicked.
pub fn selector_bar(ui: &mut Ui, labels: &[&str], current: usize) -> Option<usize> {
    let p = pal(ui.ctx());
    let mut picked = None;
    // Too many tabs for a narrow window scroll sideways, and the current one is brought into view
    // whenever it changes.
    let shown_id = egui::Id::new(("tabs-shown", labels));
    let scroll_to = ui.data(|d| d.get_temp::<usize>(shown_id)) != Some(current);
    ui.data_mut(|d| d.insert_temp(shown_id, current));
    ui.scope(|ui| {
        // A plain mouse wheel scrolls the tabs too, as it does a browser's tab strip.
        ui.style_mut().always_scroll_the_only_direction = true;
        egui::ScrollArea::horizontal().id_salt(shown_id).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (i, label) in labels.iter().enumerate() {
                    let on = i == current;
                    // Measured in semibold so a tab never changes width when it's picked.
                    let wide = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::new(14.0, theme::semibold()), p.ink);
                    let (rect, r) = ui.allocate_exact_size(egui::vec2(wide.size().x + 2.0 * S3, 40.0), Sense::click());
                    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, *label));
                    ui.ctx().accesskit_node_builder(r.id, |b| {
                        b.set_role(egui::accesskit::Role::Tab);
                        b.set_selected(on);
                    });
                    let painter = ui.painter();
                    if r.hovered() && !on {
                        painter.rect_filled(rect.shrink2(egui::vec2(0.0, S1)), CornerRadius::same(CONTROL_RADIUS), p.ink.gamma_multiply(0.05));
                    }
                    let font = egui::FontId::new(14.0, if on { theme::semibold() } else { egui::FontFamily::Proportional });
                    painter.text(rect.center(), egui::Align2::CENTER_CENTER, *label, font, if on || r.hovered() { p.ink } else { p.muted });
                    if on {
                        let bar = egui::Rect::from_center_size(egui::pos2(rect.center().x, rect.bottom() - 1.5), egui::vec2(16.0, 3.0));
                        painter.rect_filled(bar, CornerRadius::same(u8::MAX), p.accent);
                        if scroll_to {
                            r.scroll_to_me(Some(egui::Align::Center));
                        }
                    }
                    // A tab reached with Tab comes into view, so its focus ring shows.
                    if r.gained_focus() {
                        r.scroll_to_me(None);
                    }
                    if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        picked = Some(i);
                    }
                }
            });
        });
    });
    let (line, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(line, CornerRadius::ZERO, p.line);
    picked
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

/// A heading inside a section, such as a part of one card.
pub fn subsection(ui: &mut Ui, title: &str) {
    ui.add_space(S3);
    let r = ui.label(RichText::new(title).text_style(theme::title()));
    heading_role(ui, &r, 3, title);
    ui.add_space(S1);
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
    if ui.available_width() < WRAP {
        for (i, (k, v)) in rows.into_iter().enumerate() {
            if i > 0 {
                ui.add_space(S2);
            }
            ui.label(RichText::new(k).color(p.muted));
            ui.add(egui::Label::new(v).wrap());
        }
        return;
    }
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
    // In a narrow card each row becomes a list item, as Windows Settings lists apps: the first
    // cell as its name and the others as "Heading: value" under it.
    if ui.available_width() < WRAP {
        for (i, r) in rows.iter().enumerate() {
            if i > 0 {
                ui.separator();
            }
            ui.horizontal_top(|ui| {
                let text_w = ui.available_width() - if button.is_some() { 64.0 + ui.spacing().item_spacing.x } else { 0.0 };
                ui.vertical(|ui| {
                    ui.set_width(text_w);
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let name = r.first().map(String::as_str).unwrap_or_default();
                    ui.add(egui::Label::new(RichText::new(name).family(theme::semibold())).wrap());
                    let rest: Vec<String> = head.iter().zip(r).skip(1).filter(|(_, v)| !v.is_empty()).map(|(h, v)| format!("{h}: {v}")).collect();
                    if !rest.is_empty() {
                        ui.add(egui::Label::new(RichText::new(rest.join(" · ")).small().color(p.muted)).wrap());
                    }
                });
                if let Some(label) = button {
                    if ui.small_button(label).clicked() {
                        pressed = Some(i);
                    }
                }
            });
        }
        return pressed;
    }
    ui.push_id(id, |ui| {
        let n = head.len();
        // With a button column, the text columns share what's left so the table never runs past the card.
        let avail = ui.available_width() - ui.spacing().item_spacing.x * (n + 1) as f32 - if button.is_some() { 72.0 } else { 0.0 };
        // Each column starts as wide as its widest entry, heading included, while the table fits
        // its card; columns too wide for that share what's left. Measured once, because the
        // widths are kept from then on, dragged or not.
        let key = ui.id().with("natural-widths");
        let widths = match ui.data(|d| d.get_temp::<Vec<f32>>(key)) {
            Some(w) if w.len() == n => w,
            _ => {
                let measure =
                    |t: RichText| egui::WidgetText::from(t).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Body).size().x;
                let natural: Vec<f32> = (0..n)
                    .map(|i| {
                        let heading = measure(RichText::new(head[i]).family(theme::semibold()));
                        rows.iter().take(200).filter_map(|r| r.get(i)).map(|c| measure(RichText::new(c.as_str()))).fold(heading, f32::max) + S1
                    })
                    .collect();
                let w = fit_widths(&natural, avail);
                ui.data_mut(|d| d.insert_temp(key, w.clone()));
                w
            }
        };
        let least = (avail / n as f32).min(60.0);
        // Columns can be dragged wider, as in File Explorer. A table dragged wider than its card
        // scrolls sideways instead of spilling out of it.
        egui::ScrollArea::horizontal().auto_shrink([false, true]).show(ui, |ui| {
            let gap = ui.spacing().item_spacing.x;
            let painter = ui.painter().clone();
            // The edges show only in the header, as File Explorer's do, and down the whole table
            // while one is pointed at or dragged.
            ui.visuals_mut().widgets.noninteractive.bg_stroke = Stroke::NONE;
            let mut tb = TableBuilder::new(ui).striped(true).resizable(true).vscroll(false).cell_layout(egui::Layout::left_to_right(egui::Align::Center));
            for (i, &w) in widths.iter().enumerate() {
                tb = tb.column(if i + 1 == n && button.is_none() {
                    Column::remainder().at_least(least).clip(true)
                } else {
                    Column::initial(w).at_least(least).clip(true)
                });
            }
            if button.is_some() {
                tb = tb.column(Column::exact(64.0).resizable(false));
            }
            // The last column has no edge of its own to drag: it takes what's left.
            let last = n + usize::from(button.is_some()) - 1;
            tb.header(26.0, |mut h| {
                for (i, t) in head.iter().enumerate() {
                    h.col(|ui| {
                        ui.label(RichText::new(*t).color(p.muted).family(theme::semibold()));
                        if i < last {
                            let r = ui.max_rect();
                            painter.vline(r.right() + gap / 2.0, r.center().y - 7.0..=r.center().y + 7.0, Stroke::new(1.0_f32, p.line));
                        }
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
    });
    pressed
}

/// Starting column widths that fit `avail`: columns narrower than an even share keep their
/// natural width, and the wider ones split what's left evenly.
fn fit_widths(natural: &[f32], avail: f32) -> Vec<f32> {
    let mut w = natural.to_vec();
    let mut open: Vec<usize> = (0..w.len()).collect();
    let mut left = avail;
    while !open.is_empty() {
        let share = left / open.len() as f32;
        let (fits, wide): (Vec<usize>, Vec<usize>) = open.into_iter().partition(|&i| natural[i] <= share);
        if fits.is_empty() {
            for &i in &wide {
                w[i] = share;
            }
            break;
        }
        left -= fits.iter().map(|&i| natural[i]).sum::<f32>();
        open = wide;
    }
    w
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

/// A progress line with one segment per to-do. Past 24 to-dos the segments would be slivers, so
/// it becomes one continuous line.
pub fn segments(ui: &mut Ui, name: &str, done: usize, total: usize, width: f32) -> Response {
    let p = pal(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(egui::vec2(width, 6.0), Sense::hover());
    let fraction = if total == 0 { 0.0 } else { done as f32 / total as f32 };
    resp.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::WidgetType::ProgressIndicator, true, format!("{name}: {done} of {total} done"));
        info.value = Some((fraction * 100.0).round() as f64);
        info
    });
    ui.ctx().accesskit_node_builder(resp.id, |b| {
        b.set_min_numeric_value(0.0);
        b.set_max_numeric_value(100.0);
    });
    let round = CornerRadius::same(u8::MAX);
    // The control stroke, thinned, so an empty segment shows on the page in both themes.
    let track = p.control.gamma_multiply(0.4);
    if total > 24 {
        ui.painter().rect_filled(r, round, track);
        ui.painter().rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(r.width() * fraction, r.height())), round, p.accent);
        return resp;
    }
    let gap = S1;
    let seg = (width - gap * (total.saturating_sub(1)) as f32) / total.max(1) as f32;
    for i in 0..total {
        let x = r.left() + i as f32 * (seg + gap);
        let rect = egui::Rect::from_min_size(egui::pos2(x, r.top()), egui::vec2(seg, r.height()));
        ui.painter().rect_filled(rect, round, if i < done { p.accent } else { track });
    }
    resp
}

/// A frameless chevron that opens and closes a row, as in a Windows settings expander.
pub fn chevron(ui: &mut Ui, open: bool) -> Response {
    subtle_icon(ui, if open { icon::CHEVRON_UP } else { icon::CHEVRON_DOWN }, 12.0, if open { "Show less" } else { "Show more" })
}

/// A frameless button that is only an icon, at least 32 px square. `name` is what a screen
/// reader says and the tooltip shows.
pub fn subtle_icon(ui: &mut Ui, ic: &str, glyph: f32, name: &str) -> Response {
    let p = pal(ui.ctx());
    let r = ui
        .scope(|ui| {
            let w = &mut ui.visuals_mut().widgets;
            for (v, wash) in [(&mut w.inactive, 0.0), (&mut w.hovered, 0.06), (&mut w.active, 0.09)] {
                v.weak_bg_fill = p.ink.gamma_multiply(wash);
                v.bg_stroke = Stroke::NONE;
            }
            ui.add(egui::Button::new(ico(ic, glyph).color(p.ink)).min_size(egui::vec2(32.0, 32.0)))
        })
        .inner;
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    r.on_hover_text(name)
}

/// A clickable row in the sidebar.
pub fn nav_item(ui: &mut Ui, ic: &str, text: &str, selected: bool, badge: Option<usize>) -> Response {
    let p = pal(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), Sense::click());
    // A faint wash of the text colour, as Windows uses, so it shows on any sidebar fill, Mica
    // included.
    let wash = match (selected, resp.hovered()) {
        (true, true) => 0.09,
        (true, false) => 0.06,
        (false, true) => 0.04,
        (false, false) => 0.0,
    };
    let bg = p.ink.gamma_multiply(wash);
    ui.painter().rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), bg);
    if selected {
        let bar = egui::Rect::from_min_size(rect.min + egui::vec2(0.0, 10.0), egui::vec2(3.0, 16.0));
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

    #[test]
    fn columns_start_as_wide_as_their_content_and_wide_ones_share_the_rest() {
        assert_eq!(fit_widths(&[100.0, 100.0], 600.0), [100.0, 100.0], "room for all");
        assert_eq!(fit_widths(&[100.0, 300.0, 400.0], 600.0), [100.0, 250.0, 250.0]);
        assert_eq!(fit_widths(&[500.0, 500.0], 600.0), [300.0, 300.0]);
        assert_eq!(fit_widths(&[120.0, 260.0, 700.0], 950.0), [120.0, 260.0, 570.0]);
    }

    /// Runs one frame with a single button and the focus ring pass, and says whether the ring
    /// (a 2 px stroke in the text colour) was painted.
    fn frame(ctx: &egui::Context, events: Vec<egui::Event>) -> bool {
        let input = egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0))), ..Default::default() };
        let out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.add_space(40.0);
                secondary(ui, "Check again");
            });
            focus_ring(ui.ctx());
        });
        let ink = pal(ctx).ink;
        let ring = out.shapes.iter().any(|c| matches!(&c.shape, egui::Shape::Rect(r) if r.stroke.width == 2.0 && r.stroke.color == ink));
        out.drop_without_applying_deltas();
        ring
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
        let click =
            egui::Event::PointerButton { pos: egui::pos2(5.0, 5.0), button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() };
        frame(&ctx, vec![egui::Event::PointerMoved(egui::pos2(5.0, 5.0)), click]);
        assert!(!frame(&ctx, vec![]), "a click hides the ring");
    }

    /// How wide `add` ends up drawing when a card gives it `width`.
    fn used_width(width: f32, add: impl Fn(&mut Ui)) -> f32 {
        let ctx = egui::Context::default();
        theme::fonts(&ctx);
        let mut used = 0.0;
        // Two frames: a table settles its column widths on the first.
        for _ in 0..2 {
            let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 2000.0))), ..Default::default() };
            let out = ctx.run_ui(input, |ui| {
                let mut card = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 2000.0))));
                add(&mut card);
                used = card.min_rect().width();
            });
            out.drop_without_applying_deltas();
        }
        used
    }

    #[test]
    fn rows_never_run_past_a_narrow_card() {
        let rows = [("Blocks adware and bundled extras", Some("Only reports".to_string())), ("Hardware address", Some("2C-F0-5D-73-F1-69".to_string()))];
        let head = ["Extension", "Browser", "From the store", "Reads all sites", "Status"];
        let cells = vec![vec!["Picture-in-Picture Extension (by Google)".to_string(), "Chrome (Your Chrome)".into(), "Yes".into(), "Yes".into(), "Off".into()]];
        for width in [300.0, 700.0] {
            let kv_w = used_width(width, |ui| kv(ui, "kv", &rows));
            assert!(kv_w <= width, "label/value rows take {kv_w} px of {width}");
            let table_w = used_width(width, |ui| {
                table_action(ui, "t", &head, &cells, Some("Open"));
            });
            assert!(table_w <= width, "a table takes {table_w} px of {width}");
        }
    }
}
