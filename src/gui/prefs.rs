//! Preferences (port of `fsearch_preferences_dialog.c`, `fsearch_database_preferences_widget.c`,
//! `fsearch_filter_preferences_widget.c`, `fsearch_filter_editor.c`), redesigned: a modal with
//! side navigation; Appearance shows a live miniature of every look.

use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke, vec2};
use fsearch_core::config::Config;
use fsearch_core::filter::{Filter, default_filters};
use fsearch_core::query::flags;
use fsearch_core::rules::{Exclude, ExcludeType, Include, Scope, Target, add_include};

use super::theme::{self, Look, Palette};
use super::widgets::{self, Icon};

#[derive(PartialEq, Clone, Copy)]
enum Page {
    Appearance,
    Interface,
    Search,
    Database,
    Filters,
    Shortcuts,
}

const PAGES: [(Page, &str); 6] = [
    (Page::Appearance, "Appearance"),
    (Page::Interface, "Interface"),
    (Page::Search, "Search"),
    (Page::Database, "Index"),
    (Page::Filters, "Filters"),
    (Page::Shortcuts, "Shortcuts"),
];

pub struct Prefs {
    c: Config,
    page: Page,
    new_include: String,
    new_exclude: String,
}

pub enum Outcome {
    Open,
    Cancel,
    /// a look was picked: show it live (cancel restores the saved one)
    Preview(Look),
    Apply(Box<Config>),
}

impl Prefs {
    pub fn new(c: &Config) -> Prefs {
        Prefs {
            c: c.clone(),
            page: Page::Appearance,
            new_include: String::new(),
            new_exclude: String::new(),
        }
    }

    pub fn show_database(&mut self) {
        self.page = Page::Database;
    }

    pub fn show_shortcuts(&mut self) {
        self.page = Page::Shortcuts;
    }

    pub fn show(&mut self, ctx: &egui::Context, pal: &Palette) -> Outcome {
        let mut out = Outcome::Open;
        let screen = ctx.content_rect();
        let w = (screen.width() - 48.0).clamp(320.0, 760.0);
        let h = (screen.height() - 64.0).clamp(240.0, 560.0);
        let frame = egui::Frame::new()
            .fill(if pal.bevel { pal.bar } else { pal.page })
            .stroke(Stroke::new(
                1.0,
                if pal.bevel {
                    Color32::BLACK
                } else {
                    pal.hairline
                },
            ))
            .corner_radius(CornerRadius::same(pal.window_radius))
            .shadow(egui::Shadow {
                offset: [0, 12],
                blur: 32,
                spread: 0,
                color: Color32::from_black_alpha(if pal.dark { 120 } else { 40 }),
            })
            .inner_margin(egui::Margin::same(0));
        let modal = egui::Modal::new(egui::Id::unique("preferences"))
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_width(w);
                ui.set_height(h);
                let narrow = w < 560.0;
                ui.horizontal_top(|ui| {
                    // navigation
                    let nav_w = if narrow { 112.0 } else { 168.0 };
                    let nav_frame = egui::Frame::new()
                        .fill(pal.bar)
                        .inner_margin(egui::Margin::same(12))
                        .corner_radius(CornerRadius {
                            nw: pal.window_radius,
                            sw: pal.window_radius,
                            ne: 0,
                            se: 0,
                        });
                    nav_frame.show(ui, |ui| {
                        ui.set_width(nav_w - 24.0);
                        ui.set_height(h - 24.0);
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new("Preferences")
                                    .size(pal.body_size + 2.0)
                                    .color(pal.text),
                            );
                            ui.add_space(12.0);
                            for (pg, name) in PAGES {
                                let sel = self.page == pg;
                                let b = egui::Button::selectable(
                                    sel,
                                    RichText::new(name).color(if sel {
                                        pal.sel_fg
                                    } else {
                                        pal.text
                                    }),
                                )
                                .min_size(vec2(nav_w - 24.0, 32.0));
                                if ui.add(b).clicked() {
                                    self.page = pg;
                                }
                            }
                        });
                    });
                    // page
                    ui.vertical(|ui| {
                        ui.add_space(16.0);
                        let body_w = w - nav_w - 32.0;
                        egui::ScrollArea::vertical()
                            .max_height(h - 76.0)
                            .max_width(body_w + 16.0)
                            .show(ui, |ui| {
                                ui.set_width(body_w);
                                match self.page {
                                    Page::Appearance => {
                                        if let Some(l) = self.appearance(ui, pal, body_w) {
                                            out = Outcome::Preview(l);
                                        }
                                    }
                                    Page::Interface => self.interface(ui, pal),
                                    Page::Search => self.search(ui, pal),
                                    Page::Database => self.database(ui, pal),
                                    Page::Filters => self.filters(ui, pal),
                                    Page::Shortcuts => shortcuts(ui, pal),
                                }
                            });
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            let apply =
                                egui::Button::new(RichText::new("Apply").color(pal.on_accent))
                                    .fill(pal.accent)
                                    .min_size(vec2(88.0, 30.0));
                            if ui.add(apply).clicked() {
                                out = Outcome::Apply(Box::new(self.c.clone()));
                            }
                            if ui
                                .add(egui::Button::new("Cancel").min_size(vec2(88.0, 30.0)))
                                .clicked()
                            {
                                out = Outcome::Cancel;
                            }
                        });
                    });
                });
            });
        if modal.should_close() && matches!(out, Outcome::Open) {
            return Outcome::Cancel;
        }
        out
    }

    fn appearance(&mut self, ui: &mut egui::Ui, pal: &Palette, width: f32) -> Option<Look> {
        widgets::section(ui, pal, "Look");
        let current = Look::from_id(&self.c.look);
        let mut picked = None;
        let cols = if width > 480.0 { 3 } else { 2 };
        let card_w = (width - 12.0 * (cols as f32 - 1.0)) / cols as f32;
        egui::Grid::new("looks")
            .spacing(vec2(12.0, 12.0))
            .show(ui, |ui| {
                for (k, look) in theme::ALL.into_iter().enumerate() {
                    if look_card(ui, pal, look, look == current, card_w).clicked() {
                        self.c.look = look.id().to_string();
                        picked = Some(look);
                    }
                    if (k + 1) % cols == 0 {
                        ui.end_row();
                    }
                }
            });
        ui.add_space(8.0);
        ui.label(
            RichText::new("Ubuntu, macOS and Windows 95 are inspired by those desktops. Classic is the first port's look.")
                .color(pal.muted)
                .size(pal.small_size + 1.0),
        );
        widgets::section(ui, pal, "Result list");
        let c = &mut self.c;
        ui.checkbox(&mut c.show_listview_icons, "Show icons");
        ui.checkbox(&mut c.highlight_search_terms, "Highlight matches");
        ui.checkbox(&mut c.enable_list_tooltips, "Show the full path on hover");
        ui.checkbox(&mut c.show_base_2_units, "Sizes in base 2 (KiB, MiB)");
        picked
    }

    fn interface(&mut self, ui: &mut egui::Ui, pal: &Palette) {
        let c = &mut self.c;
        widgets::section(ui, pal, "Window");
        ui.checkbox(&mut c.show_menubar, "Show the menu bar");
        ui.checkbox(&mut c.show_statusbar, "Show the status bar");
        ui.checkbox(&mut c.show_filter, "Show the filter menu");
        ui.checkbox(&mut c.restore_window_size, "Remember the window size");
        ui.checkbox(&mut c.restore_sort_order, "Remember the sort order");
        ui.checkbox(&mut c.restore_column_config, "Remember the columns");
        ui.checkbox(&mut c.exit_on_escape, "Esc quits (otherwise it minimizes)");
        widgets::section(ui, pal, "Opening files");
        ui.checkbox(&mut c.single_click_open, "Open with a single click");
        ui.checkbox(&mut c.launch_desktop_files, "Launch .desktop files");
        ui.checkbox(
            &mut c.show_dialog_failed_opening,
            "Tell me when a file cannot be opened",
        );
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("After opening").color(pal.text2));
            egui::ComboBox::from_id_salt("after_open")
                .selected_text(
                    ["Do nothing", "Minimize", "Quit"]
                        [c.action_after_file_open.clamp(0, 2) as usize],
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut c.action_after_file_open, 0, "Do nothing");
                    ui.selectable_value(&mut c.action_after_file_open, 1, "Minimize");
                    ui.selectable_value(&mut c.action_after_file_open, 2, "Quit");
                });
            ui.checkbox(&mut c.action_after_file_open_keyboard, "from the keyboard");
            ui.checkbox(&mut c.action_after_file_open_mouse, "with the mouse");
        });
        ui.add_space(4.0);
        ui.label(RichText::new("Folder open command").color(pal.text2));
        let mut cmd = c.folder_open_cmd.clone().unwrap_or_default();
        if ui
            .add(
                egui::TextEdit::singleline(&mut cmd)
                    .hint_text("System default")
                    .desired_width(f32::INFINITY),
            )
            .changed()
        {
            c.folder_open_cmd = if cmd.trim().is_empty() {
                None
            } else {
                Some(cmd)
            };
        }
    }

    fn search(&mut self, ui: &mut egui::Ui, pal: &Palette) {
        let c = &mut self.c;
        widgets::section(ui, pal, "Matching");
        toggle_row(ui, pal, &mut c.match_case, "Match case", "Ctrl+I");
        toggle_row(
            ui,
            pal,
            &mut c.auto_match_case,
            "Match case when the query has capitals",
            "",
        );
        toggle_row(
            ui,
            pal,
            &mut c.enable_regex,
            "Regular expressions",
            "Ctrl+R",
        );
        toggle_row(ui, pal, &mut c.search_in_path, "Search in path", "Ctrl+U");
        toggle_row(
            ui,
            pal,
            &mut c.auto_search_in_path,
            "Search in path when the query has /",
            "",
        );
        widgets::section(ui, pal, "Results");
        ui.checkbox(&mut c.search_as_you_type, "Search as you type");
        ui.checkbox(
            &mut c.hide_results_on_empty_search,
            "Show nothing until I type",
        );
    }

    fn database(&mut self, ui: &mut egui::Ui, pal: &Palette) {
        widgets::section(ui, pal, "Folders to index");
        if self.c.includes.is_empty() {
            ui.label(RichText::new("No folders yet").color(pal.muted));
        }
        let mut remove = None;
        for (i, inc) in self.c.includes.iter_mut().enumerate() {
            egui::Frame::new()
                .fill(pal.raised)
                .stroke(Stroke::new(1.0, pal.hairline))
                .corner_radius(CornerRadius::same(pal.radius))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        widgets::icon(ui, Icon::Folder, pal.text2);
                        ui.label(RichText::new(&inc.path).color(pal.text));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if widgets::icon_button(ui, pal, Icon::Trash, "Remove").clicked() {
                                remove = Some(i);
                            }
                        });
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut inc.active, "Active");
                        ui.checkbox(&mut inc.one_file_system, "Stay on one file system");
                        ui.checkbox(&mut inc.monitored, "Watch for changes");
                        ui.checkbox(&mut inc.scan_after_launch, "Rescan at launch");
                    });
                });
            ui.add_space(4.0);
        }
        if let Some(i) = remove {
            self.c.includes.remove(i);
        }
        ui.horizontal(|ui| {
            let add = egui::TextEdit::singleline(&mut self.new_include)
                .hint_text("/path/to/folder")
                .desired_width(ui.available_width() - 200.0);
            ui.add(add);
            if ui.button("Browse…").clicked()
                && let Some(p) = rfd::FileDialog::new().pick_folder()
            {
                self.new_include = p.to_string_lossy().into_owned();
            }
            let valid = std::path::Path::new(self.new_include.trim()).is_absolute();
            if ui.add_enabled(valid, egui::Button::new("Add")).clicked() {
                add_include(
                    &mut self.c.includes,
                    Include::new(self.new_include.trim(), true, false, false, false, 0),
                );
                self.new_include.clear();
            }
        });
        widgets::section(ui, pal, "Exclude");
        let mut remove = None;
        egui::Grid::new("excludes")
            .num_columns(6)
            .spacing(vec2(8.0, 6.0))
            .show(ui, |ui| {
                for (i, e) in self.c.excludes.excludes.iter_mut().enumerate() {
                    let mut pattern = e.pattern.clone();
                    let (mut active, mut kind, mut scope, mut target) =
                        (e.active, e.kind, e.scope, e.target);
                    ui.checkbox(&mut active, "");
                    ui.add(egui::TextEdit::singleline(&mut pattern).desired_width(180.0));
                    egui::ComboBox::from_id_salt(("type", i))
                        .width(84.0)
                        .selected_text(kind.as_str())
                        .show_ui(ui, |ui| {
                            for k in [
                                ExcludeType::Fixed,
                                ExcludeType::Wildcard,
                                ExcludeType::Regex,
                            ] {
                                ui.selectable_value(&mut kind, k, k.as_str());
                            }
                        });
                    egui::ComboBox::from_id_salt(("scope", i))
                        .width(84.0)
                        .selected_text(scope.as_str())
                        .show_ui(ui, |ui| {
                            for s in [Scope::FullPath, Scope::Basename] {
                                ui.selectable_value(&mut scope, s, s.as_str());
                            }
                        });
                    egui::ComboBox::from_id_salt(("target", i))
                        .width(72.0)
                        .selected_text(target.as_str())
                        .show_ui(ui, |ui| {
                            for t in [Target::Both, Target::Files, Target::Folders] {
                                ui.selectable_value(&mut target, t, t.as_str());
                            }
                        });
                    if pattern != e.pattern
                        || active != e.active
                        || kind != e.kind
                        || scope != e.scope
                        || target != e.target
                    {
                        *e = Exclude::new(&pattern, active, kind, scope, target);
                    }
                    if widgets::icon_button(ui, pal, Icon::Trash, "Remove").clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
        if let Some(i) = remove {
            self.c.excludes.excludes.remove(i);
        }
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.new_exclude)
                    .hint_text("Path or pattern")
                    .desired_width(ui.available_width() - 64.0),
            );
            if ui
                .add_enabled(
                    !self.new_exclude.trim().is_empty(),
                    egui::Button::new("Add"),
                )
                .clicked()
            {
                self.c.excludes.add(Exclude::new(
                    self.new_exclude.trim(),
                    true,
                    ExcludeType::Fixed,
                    Scope::FullPath,
                    Target::Both,
                ));
                self.new_exclude.clear();
            }
        });
        ui.add_space(4.0);
        ui.checkbox(
            &mut self.c.excludes.exclude_hidden,
            "Skip hidden files and folders",
        );
    }

    fn filters(&mut self, ui: &mut egui::Ui, pal: &Palette) {
        widgets::section(ui, pal, "Filters");
        ui.label(
            RichText::new(
                "A filter limits every search; its macro (like pic:) can be typed in a query",
            )
            .color(pal.muted)
            .size(pal.small_size + 1.0),
        );
        ui.add_space(4.0);
        let mut remove = None;
        for (i, f) in self.c.filters.iter_mut().enumerate() {
            egui::Frame::new()
                .fill(pal.raised)
                .stroke(Stroke::new(1.0, pal.hairline))
                .corner_radius(CornerRadius::same(pal.radius))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut f.name)
                                .desired_width(140.0)
                                .hint_text("Name"),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut f.macro_name)
                                .desired_width(72.0)
                                .hint_text("macro"),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if widgets::icon_button(ui, pal, Icon::Trash, "Remove").clicked() {
                                remove = Some(i);
                            }
                            for (flag, label, tip) in [
                                (flags::REGEX, ".*", "Regular expressions"),
                                (flags::SEARCH_IN_PATH, "/", "Search in path"),
                                (flags::MATCH_CASE, "Aa", "Match case"),
                            ] {
                                let mut on = f.flags & flag != 0;
                                if widgets::keycap_toggle(ui, pal, label, &mut on, tip).changed() {
                                    f.flags ^= flag;
                                }
                            }
                        });
                    });
                    ui.add(
                        egui::TextEdit::singleline(&mut f.query)
                            .hint_text("Query, like ext:jpg;png")
                            .desired_width(f32::INFINITY),
                    );
                });
            ui.add_space(4.0);
        }
        if let Some(i) = remove {
            self.c.filters.remove(i);
        }
        ui.horizontal(|ui| {
            if ui.button("Add filter").clicked() {
                let mut n = 1;
                while self
                    .c
                    .filters
                    .iter()
                    .any(|f| f.name == format!("Filter {n}"))
                {
                    n += 1;
                }
                self.c
                    .filters
                    .push(Filter::new(&format!("Filter {n}"), None, None, 0));
            }
            if ui.button("Restore defaults").clicked() {
                self.c.filters = default_filters();
            }
        });
    }
}

fn toggle_row(ui: &mut egui::Ui, pal: &Palette, on: &mut bool, label: &str, keys: &str) {
    ui.horizontal(|ui| {
        ui.checkbox(on, label);
        if !keys.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                widgets::keycap(ui, pal, keys);
            });
        }
    });
}

fn shortcuts(ui: &mut egui::Ui, pal: &Palette) {
    widgets::section(ui, pal, "Keyboard");
    egui::Grid::new("keys")
        .num_columns(2)
        .spacing(vec2(24.0, 8.0))
        .show(ui, |ui| {
            for (what, keys) in super::SHORTCUTS {
                ui.label(RichText::new(*what).color(pal.text));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for k in keys.split(' ') {
                        widgets::keycap(ui, pal, k);
                    }
                });
                ui.end_row();
            }
        });
}

/// A miniature of the window in a look: header with search field and keycaps, three rows with a
/// highlighted match and a selection. The one rich element of the preferences.
fn look_card(
    ui: &mut egui::Ui,
    current: &Palette,
    look: Look,
    selected: bool,
    width: f32,
) -> egui::Response {
    let p = look.palette();
    let h = width * 0.62 + 28.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, h), egui::Sense::click());
    let painter = ui.painter_at(rect.expand(3.0));
    let preview = egui::Rect::from_min_size(rect.min, vec2(width, h - 28.0));
    let rad = (p.window_radius as f32 * 0.6).min(8.0);
    painter.rect_filled(preview, rad, p.page);
    // header
    let head = egui::Rect::from_min_size(preview.min, vec2(width, preview.height() * 0.30));
    painter.rect_filled(
        head,
        CornerRadius {
            nw: rad as u8,
            ne: rad as u8,
            sw: 0,
            se: 0,
        },
        p.bar,
    );
    let field = egui::Rect::from_min_max(
        head.min + vec2(10.0, head.height() * 0.28),
        head.max - vec2(10.0, head.height() * 0.28),
    );
    if p.bevel {
        painter.rect_filled(field, 0.0, Color32::WHITE);
        widgets::sunken(&painter, field);
    } else {
        painter.rect_filled(field, p.radius as f32 * 0.6, p.raised);
        painter.rect_stroke(
            field,
            p.radius as f32 * 0.6,
            Stroke::new(1.0, p.hairline),
            egui::StrokeKind::Inside,
        );
    }
    let fh = field.height();
    painter.text(
        field.left_center() + vec2(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        "report",
        egui::FontId::proportional(fh * 0.55),
        p.text,
    );
    for k in 0..2 {
        let kc = egui::Rect::from_min_size(
            egui::pos2(
                field.right() - 6.0 - (k as f32 + 1.0) * (fh * 0.9 + 3.0),
                field.top() + fh * 0.18,
            ),
            vec2(fh * 0.9, fh * 0.64),
        );
        painter.rect_stroke(
            kc,
            2.0,
            Stroke::new(1.0, if k == 1 { p.accent } else { p.hairline }),
            egui::StrokeKind::Inside,
        );
    }
    // rows
    let rows_top = head.bottom() + 4.0;
    let row_h = (preview.bottom() - rows_top - 4.0) / 3.0;
    let names = [
        ("report", ".pdf"),
        ("Report Final", ".PDF"),
        ("reports", ""),
    ];
    for (r, (stem, ext)) in names.iter().enumerate() {
        let y = rows_top + r as f32 * row_h;
        let row = egui::Rect::from_min_size(
            egui::pos2(preview.left() + 4.0, y),
            vec2(width - 8.0, row_h - 2.0),
        );
        let sel = r == 1;
        let filled = sel && !p.sel_marker;
        if let Some(s) = p.stripe
            && r % 2 == 1
        {
            painter.rect_filled(row, 2.0, s);
        }
        if sel {
            painter.rect_filled(row, if p.bevel { 0.0 } else { 3.0 }, p.sel_bg);
            if p.sel_marker {
                painter.rect_filled(
                    egui::Rect::from_min_size(row.min, vec2(2.0, row.height())),
                    0.0,
                    p.accent,
                );
            }
        }
        let fs = (row_h * 0.5).clamp(7.0, 12.0);
        let x = row.left() + 8.0;
        // the match ("report" / "Report") marked like the real list
        let g = painter.layout_no_wrap(
            stem.to_string(),
            egui::FontId::proportional(fs),
            if filled { p.sel_fg } else { p.match_fg },
        );
        let mr = egui::Rect::from_min_size(
            egui::pos2(x, row.center().y - g.size().y / 2.0),
            vec2(g.size().x.min(fs * 3.4), g.size().y),
        );
        if !filled {
            painter.rect_filled(mr, 1.0, p.match_bg);
        }
        let gw = g.size().x;
        painter.galley(egui::pos2(x, row.center().y - g.size().y / 2.0), g, p.text);
        painter.text(
            egui::pos2(x + gw, row.center().y),
            egui::Align2::LEFT_CENTER,
            *ext,
            egui::FontId::proportional(fs),
            if filled { p.sel_fg } else { p.text },
        );
        painter.text(
            egui::pos2(row.right() - 6.0, row.center().y),
            egui::Align2::RIGHT_CENTER,
            ["120 kB", "5.0 kB", "—"][r],
            egui::FontId::monospace(fs * 0.9),
            if filled { p.sel_fg } else { p.text2 },
        );
    }
    let outline = if selected {
        Stroke::new(2.0, current.accent)
    } else if resp.hovered() {
        Stroke::new(1.0, current.text2)
    } else {
        Stroke::new(1.0, current.hairline)
    };
    painter.rect_stroke(preview, rad, outline, egui::StrokeKind::Outside);
    // label
    let label_pos = egui::pos2(rect.left() + 2.0, preview.bottom() + 8.0);
    painter.text(
        label_pos,
        egui::Align2::LEFT_TOP,
        look.name(),
        egui::FontId::proportional(current.body_size - 1.0),
        if selected {
            current.text
        } else {
            current.text2
        },
    );
    if resp.has_focus() {
        painter.rect_stroke(
            preview.expand(2.0),
            rad + 2.0,
            Stroke::new(1.5, current.accent),
            egui::StrokeKind::Outside,
        );
    }
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::RadioButton, true, selected, look.name())
    });
    resp.on_hover_text(look.name())
}
