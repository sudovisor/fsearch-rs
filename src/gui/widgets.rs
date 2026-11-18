//! Small drawn widgets shared by the window and the preferences: line icons in one stroke,
//! keycaps (the signature detail), keycap toggles, Windows 95 bevels and left-elided paths.

use eframe::egui::{self, Color32, Pos2, Rect, Response, Sense, Stroke, Ui, pos2, vec2};

use super::theme::Palette;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Search,
    Folder,
    File,
    Refresh,
    Gear,
    Menu,
    Trash,
    FolderOpen,
    Check,
}

pub const ICON: f32 = 16.0;

/// Paint `icon` into `rect` with a 1.5 stroke, lucide-like.
pub fn paint_icon(p: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let s = Stroke::new(1.5, color);
    let c = rect.center();
    let u = rect.width().min(rect.height()) / 16.0;
    let at = |x: f32, y: f32| pos2(c.x + (x - 8.0) * u, c.y + (y - 8.0) * u);
    match icon {
        Icon::Search => {
            p.circle_stroke(at(7.0, 7.0), 4.5 * u, s);
            p.line_segment([at(10.4, 10.4), at(14.0, 14.0)], s);
        }
        Icon::Folder | Icon::FolderOpen => {
            let pts = vec![
                at(1.5, 4.0),
                at(6.0, 4.0),
                at(7.5, 5.5),
                at(14.5, 5.5),
                at(14.5, 13.0),
                at(1.5, 13.0),
            ];
            p.add(egui::Shape::closed_line(pts, s));
            if icon == Icon::FolderOpen {
                p.line_segment([at(1.5, 8.0), at(14.5, 8.0)], s);
            }
        }
        Icon::File => {
            let pts = vec![
                at(3.5, 1.5),
                at(9.5, 1.5),
                at(12.5, 4.5),
                at(12.5, 14.5),
                at(3.5, 14.5),
            ];
            p.add(egui::Shape::closed_line(pts, s));
            p.line_segment([at(9.5, 1.5), at(9.5, 4.5)], s);
            p.line_segment([at(9.5, 4.5), at(12.5, 4.5)], s);
        }
        Icon::Refresh => {
            let r = 5.5 * u;
            let n = 20;
            let pts: Vec<Pos2> = (0..=n)
                .map(|i| {
                    let a = -0.3 + (i as f32 / n as f32) * 5.0;
                    pos2(c.x + r * a.cos(), c.y + r * a.sin())
                })
                .collect();
            let end = *pts.last().unwrap_or(&c);
            p.add(egui::Shape::line(pts, s));
            p.line_segment([end, end + vec2(3.0 * u, 0.3 * u)], s);
            p.line_segment([end, end + vec2(0.4 * u, -3.0 * u)], s);
        }
        Icon::Gear => {
            p.circle_stroke(c, 2.2 * u, s);
            p.circle_stroke(c, 5.2 * u, s);
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                let (dx, dy) = (a.cos(), a.sin());
                p.line_segment([c + vec2(dx, dy) * 5.2 * u, c + vec2(dx, dy) * 7.0 * u], s);
            }
        }
        Icon::Menu => {
            for y in [4.0, 8.0, 12.0] {
                p.line_segment([at(2.5, y), at(13.5, y)], s);
            }
        }
        Icon::Trash => {
            p.line_segment([at(2.5, 4.0), at(13.5, 4.0)], s);
            p.line_segment([at(6.0, 4.0), at(6.5, 2.0)], s);
            p.line_segment([at(6.5, 2.0), at(9.5, 2.0)], s);
            p.line_segment([at(9.5, 2.0), at(10.0, 4.0)], s);
            let pts = vec![at(4.0, 4.0), at(4.8, 14.0), at(11.2, 14.0), at(12.0, 4.0)];
            p.add(egui::Shape::line(pts, s));
        }
        Icon::Check => {
            let pts = vec![at(2.5, 8.5), at(6.5, 12.5), at(13.5, 3.5)];
            p.add(egui::Shape::line(pts, s));
        }
    }
}

/// A checkmark atom for menu toggles: paints `Icon::Check` when `on` is true,
/// or reserves equal space when false, keeping menu labels aligned.
pub fn check_atom(on: bool, color: Color32) -> egui::Atom<'static> {
    egui::AtomKind::closure(move |ui, _args| {
        let s = ui.spacing().icon_width;
        let size = vec2(s, s);
        egui::IntoSizedResult {
            intrinsic_size: size,
            sized: egui::SizedAtomKind::Paint {
                paint: std::sync::Arc::new(move |ui: &Ui, args: egui::AtomPaintArgs| {
                    if on {
                        paint_icon(ui.painter(), args.rect, Icon::Check, color);
                    }
                }),
                size,
            },
        }
    })
    .into()
}

/// An icon in a fixed box (so labels next to icons line up).
pub fn icon(ui: &mut Ui, icon: Icon, color: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ICON, ICON), Sense::hover());
    paint_icon(ui.painter(), rect, icon, color);
    resp
}

/// A borderless icon button with a hover surface; the tooltip names the action and its shortcut.
pub fn icon_button(ui: &mut Ui, pal: &Palette, icon: Icon, tip: &str) -> Response {
    let size = vec2(28.0, 28.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if pal.bevel {
        bevel(ui.painter(), rect, pal, !resp.is_pointer_button_down_on());
    } else if resp.hovered() || resp.has_focus() {
        ui.painter()
            .rect_filled(rect, pal.radius as f32, pal.raised);
        ui.painter().rect_stroke(
            rect,
            pal.radius as f32,
            Stroke::new(1.0, pal.hairline),
            egui::StrokeKind::Inside,
        );
    }
    let r = Rect::from_center_size(rect.center(), vec2(ICON, ICON));
    paint_icon(
        ui.painter(),
        r,
        icon,
        if resp.hovered() { pal.text } else { pal.text2 },
    );
    if resp.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(1.0),
            pal.radius as f32 + 1.0,
            Stroke::new(1.5, pal.accent),
            egui::StrokeKind::Outside,
        );
    }
    // screen readers (and UI tests) see a labelled button; the label is the tip without the shortcut
    let label = tip.split("  (").next().unwrap_or(tip).to_string();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    resp.on_hover_text(tip)
}

/// Windows 95 bevel: white/grey edges, raised or pressed.
pub fn bevel(p: &egui::Painter, r: Rect, pal: &Palette, raised: bool) {
    let (tl, br) = if raised {
        (Color32::WHITE, Color32::from_gray(0x80))
    } else {
        (Color32::from_gray(0x80), Color32::WHITE)
    };
    p.rect_filled(r, 0.0, pal.bar);
    let r = r.shrink(0.5);
    p.line_segment([r.left_bottom(), r.left_top()], Stroke::new(1.0, tl));
    p.line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, tl));
    p.line_segment(
        [r.right_top(), r.right_bottom()],
        Stroke::new(1.0, if raised { Color32::BLACK } else { tl }),
    );
    p.line_segment(
        [r.left_bottom(), r.right_bottom()],
        Stroke::new(1.0, if raised { Color32::BLACK } else { tl }),
    );
    let i = r.shrink(1.0);
    p.line_segment([i.right_top(), i.right_bottom()], Stroke::new(1.0, br));
    p.line_segment([i.left_bottom(), i.right_bottom()], Stroke::new(1.0, br));
}

/// Windows 95 sunken field (inputs, list, status panels).
pub fn sunken(p: &egui::Painter, r: Rect) {
    let r = r.shrink(0.5);
    p.line_segment(
        [r.left_bottom(), r.left_top()],
        Stroke::new(1.0, Color32::from_gray(0x80)),
    );
    p.line_segment(
        [r.left_top(), r.right_top()],
        Stroke::new(1.0, Color32::from_gray(0x80)),
    );
    p.line_segment(
        [r.right_top(), r.right_bottom()],
        Stroke::new(1.0, Color32::WHITE),
    );
    p.line_segment(
        [r.left_bottom(), r.right_bottom()],
        Stroke::new(1.0, Color32::WHITE),
    );
}

/// A keycap: small rounded box with a hairline and mono text.
pub fn keycap(ui: &mut Ui, pal: &Palette, text: &str) -> Response {
    let font = egui::FontId::monospace(pal.small_size);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, pal.text2);
    let size = vec2(galley.size().x + 10.0, (galley.size().y + 4.0).max(18.0));
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    paint_keycap(ui.painter(), rect, pal, false);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, pal.text2);
    resp
}

fn paint_keycap(p: &egui::Painter, rect: Rect, pal: &Palette, on: bool) {
    if pal.bevel {
        bevel(p, rect, pal, !on);
        return;
    }
    let rad = (pal.radius as f32 - 3.0).max(3.0);
    let fill = if on {
        pal.accent
            .gamma_multiply(if pal.dark { 0.28 } else { 0.16 })
    } else {
        pal.raised
    };
    p.rect_filled(rect, rad, fill);
    p.rect_stroke(
        rect,
        rad,
        Stroke::new(1.0, if on { pal.accent } else { pal.hairline }),
        egui::StrokeKind::Inside,
    );
    // the key's bottom edge
    p.line_segment(
        [
            rect.left_bottom() + vec2(rad, -0.5),
            rect.right_bottom() + vec2(-rad, -0.5),
        ],
        Stroke::new(
            1.0,
            if on {
                pal.accent
            } else {
                pal.hairline.gamma_multiply(1.6)
            },
        ),
    );
}

/// A keycap that toggles a search option (`Aa`, `.*`, `/`).
pub fn keycap_toggle(
    ui: &mut Ui,
    pal: &Palette,
    label: &str,
    on: &mut bool,
    tip: &str,
) -> Response {
    let font = egui::FontId::monospace(pal.small_size + 1.0);
    let color = if *on {
        if pal.dark || pal.bevel {
            pal.text
        } else {
            pal.accent
        }
    } else {
        pal.muted
    };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, color);
    let size = vec2((galley.size().x + 12.0).max(26.0), 22.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    paint_keycap(ui.painter(), rect, pal, *on);
    if resp.hovered() && !*on && !pal.bevel {
        ui.painter().rect_stroke(
            rect,
            pal.radius as f32 - 3.0,
            Stroke::new(1.0, pal.text2.gamma_multiply(0.6)),
            egui::StrokeKind::Inside,
        );
    }
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, color);
    if resp.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            pal.radius as f32,
            Stroke::new(1.5, pal.accent),
            egui::StrokeKind::Outside,
        );
    }
    let label = tip
        .split("  (")
        .next()
        .unwrap_or(tip)
        .lines()
        .next()
        .unwrap_or(tip)
        .to_string();
    let state = *on;
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, state, &label));
    resp.on_hover_text(tip)
}

/// Text that keeps its end when too long ("…/share/doc/fsearch"), for paths.
pub fn elide_left(
    ui: &Ui,
    text: &str,
    font: egui::FontId,
    color: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let full = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font.clone(), color);
    if full.size().x <= width || text.is_empty() {
        return full;
    }
    // binary search the longest suffix that fits after an ellipsis
    let idx: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
    let (mut lo, mut hi) = (0usize, idx.len());
    while lo < hi {
        let mid = (lo + hi) / 2;
        let s = format!("…{}", &text[idx[mid]..]);
        if ui.painter().layout_no_wrap(s, font.clone(), color).size().x <= width {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    let start = idx.get(lo).copied().unwrap_or(text.len());
    ui.painter()
        .layout_no_wrap(format!("…{}", &text[start..]), font, color)
}

/// A section label inside panels (sentence case, muted).
pub fn section(ui: &mut Ui, pal: &Palette, text: &str) {
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(text)
            .color(pal.muted)
            .size(pal.small_size),
    );
    ui.add_space(4.0);
}

/// Paint a line of text centered in `rect` (for empty states).
pub fn centered_text(ui: &Ui, rect: Rect, y: f32, text: &str, size: f32, color: Color32) -> f32 {
    let g = ui.painter().layout(
        text.to_owned(),
        egui::FontId::proportional(size),
        color,
        rect.width() - 48.0,
    );
    let h = g.size().y;
    ui.painter()
        .galley(pos2(rect.center().x - g.size().x / 2.0, y), g, color);
    h
}
